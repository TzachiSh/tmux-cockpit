//! The markdown viewer (#283): `cockpit-tui view [--line N] [--] <file>`, what
//! the path picker opens a lone `.md` in. It renders the file read-only on
//! /dev/tty and prints `edit <line>` when `e` hands it to the editor. Nothing in
//! the file is run or followed; a file that is too big, binary or not UTF-8 is
//! a note, never a hang.
pub mod render;

use crate::chrome;
use crate::pick::{clean, open_screen, restore_tty, screen};
use crate::theme::Theme;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use render::{Ink, Kind, Opts, Row};
use std::io::Read;

/// The largest file shown. A markdown file past it is not documentation, and
/// its rows would cost a few hundred bytes each in memory.
pub const MAX_FILE: u64 = 1 << 20;
/// Narrower or shorter than this the frame and footer leave no body.
pub const MIN_W: u16 = 24;
pub const MIN_H: u16 = 6;
/// Rows one wheel notch moves.
const WHEEL: usize = 3;

/// The file's text, or the note shown in its place.
pub fn load(path: &std::path::Path, cap: u64) -> Result<String, String> {
    use std::os::unix::fs::OpenOptionsExt;
    // O_NONBLOCK: opening a FIFO for reading would otherwise wait for a writer
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| format!("cannot open it: {e}"))?;
    let meta = f.metadata().map_err(|e| format!("cannot read it: {e}"))?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    // one byte past the cap is enough to know, and a file still growing is caught too
    let mut b = Vec::new();
    f.take(cap + 1).read_to_end(&mut b).map_err(|e| format!("cannot read it: {e}"))?;
    if b.len() as u64 > cap {
        return Err(format!("too large to show: over {} KB", cap / 1024));
    }
    if b.contains(&0) {
        return Err("a binary file".into());
    }
    let s = String::from_utf8(b).map_err(|_| "not UTF-8 text".to_string())?;
    Ok(s.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(s))
}

/// A search match: a row and a byte range of its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub row: usize,
    pub from: usize,
    pub to: usize,
}

/// Every match of `q` in the rows, in order. Case-insensitive unless `q` has
/// a capital in it, as less and vim's smartcase read it.
pub fn find(texts: &[String], q: &str) -> Vec<Hit> {
    if q.is_empty() {
        return vec![];
    }
    let fold = !q.chars().any(char::is_uppercase);
    let (q, _) = folded(q, fold);
    let mut out = Vec::new();
    for (row, t) in texts.iter().enumerate() {
        let (f, back) = folded(t, fold);
        let mut at = 0;
        while let Some(i) = f[at..].find(&q) {
            let (a, b) = (at + i, at + i + q.len());
            out.push(Hit {
                row,
                from: back[a],
                to: back[b],
            });
            at = b;
        }
    }
    out
}

/// `s` lowercased where a char lowers to one char, and each folded byte's
/// offset in `s` (plus the end), so a match maps back onto the row.
fn folded(s: &str, fold: bool) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut back = Vec::with_capacity(s.len() + 1);
    for (i, c) in s.char_indices() {
        let mut low = c.to_lowercase();
        let one = match (low.next(), low.next()) {
            (Some(l), None) if fold => l,
            _ => c,
        };
        out.push(one);
        back.extend(std::iter::repeat_n(i, one.len_utf8()));
    }
    back.push(s.len());
    (out, back)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Out {
    Stay,
    Close,
    /// `e`: the editor, at this source line
    Edit(usize),
}

pub struct Screen {
    /// the file's name as the title shows it, made safe to draw
    pub name: String,
    pub doc: Result<String, String>,
    pub opts: Opts,
    rows: Vec<Row>,
    texts: Vec<String>,
    /// the width `rows` were laid out at; 0 before the first frame
    width: usize,
    pub top: usize,
    /// the `?` help's scroll offset while it is open
    pub help: Option<usize>,
    /// the find field's text while it is open, and the top it opened at
    pub field: Option<(String, usize)>,
    pub query: String,
    pub hits: Vec<Hit>,
    pub cur: Option<usize>,
    /// a one-frame note in the footer: `no match for …`
    pub note: Option<String>,
    /// the source line to open at, until the first layout finds its row
    want: Option<usize>,
}

impl Screen {
    pub fn new(name: &str, doc: Result<String, String>, opts: Opts) -> Self {
        Screen {
            name: clean(name),
            doc,
            opts,
            rows: vec![],
            texts: vec![],
            width: 0,
            top: 0,
            help: None,
            field: None,
            query: String::new(),
            hits: vec![],
            cur: None,
            note: None,
            want: None,
        }
    }

    /// Opens at the first row of the block holding source line `n`.
    pub fn at_line(mut self, n: usize) -> Self {
        self.want = Some(n);
        self
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Lays the rows out for the body `w` wide, keeping the top row's source
    /// line on top across a resize or a toggle.
    pub fn layout(&mut self, w: usize) {
        if w == self.width && !self.rows.is_empty() {
            return;
        }
        let keep = self.want.take().or_else(|| self.rows.get(self.top).map(|r| r.line));
        self.rows = match &self.doc {
            Ok(s) => render::render(s, w, self.opts),
            Err(_) => vec![],
        };
        self.texts = self.rows.iter().map(Row::text).collect();
        self.width = w;
        if let Some(l) = keep {
            self.top = self.rows.iter().position(|r| r.line >= l).unwrap_or(self.rows.len());
        }
        self.refind();
    }

    fn refind(&mut self) {
        self.hits = find(&self.texts, &self.query);
        self.cur = self.cur.filter(|c| *c < self.hits.len());
    }

    /// The source line `e` opens at: the top row's, else the first.
    pub fn line(&self) -> usize {
        self.rows.get(self.top).map_or(1, |r| r.line.max(1))
    }

    fn bottom(&self, room: usize) -> usize {
        self.rows.len().saturating_sub(room)
    }

    /// Puts hit `i` on screen, a third of the way down when it was off it.
    fn show(&mut self, i: usize, room: usize) {
        self.cur = Some(i);
        let r = self.hits[i].row;
        if r < self.top || r >= self.top + room {
            self.top = r.saturating_sub(room / 3);
        }
        self.top = self.top.min(self.bottom(room));
    }

    /// The first hit at or after row `from`, wrapping round to the first.
    fn first_from(&self, from: usize) -> Option<usize> {
        self.hits
            .iter()
            .position(|h| h.row >= from)
            .or((!self.hits.is_empty()).then_some(0))
    }

    /// `n` (or `N` when `back`): the next match, round the end to the start.
    pub fn next(&mut self, back: bool, room: usize) {
        let n = self.hits.len();
        if n == 0 {
            if !self.query.is_empty() {
                self.note = Some(format!("no match for {}", self.query));
            }
            return;
        }
        let i = match (self.cur, back) {
            (Some(c), false) => (c + 1) % n,
            (Some(c), true) => (c + n - 1) % n,
            (None, false) => self.first_from(self.top).unwrap_or(0),
            (None, true) => self.hits.iter().rposition(|h| h.row < self.top).unwrap_or(n - 1),
        };
        self.show(i, room);
    }

    /// A key in the find field: every printable key is text.
    fn field_key(&mut self, k: KeyEvent, room: usize) {
        let Some((text, origin)) = &mut self.field else { return };
        let origin = *origin;
        match k.code {
            KeyCode::Esc => {
                self.field = None;
                self.query.clear();
                self.top = origin;
                self.refind();
                return;
            }
            KeyCode::Enter => {
                self.field = None;
                if self.hits.is_empty() && !self.query.is_empty() {
                    self.note = Some(format!("no match for {}", self.query));
                }
                return;
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => text.clear(),
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => text.push(c),
            _ => return,
        }
        self.query = clean(text);
        self.refind();
        self.cur = None;
        self.top = origin.min(self.bottom(room));
        if let Some(i) = self.first_from(origin) {
            self.show(i, room);
        }
    }

    /// A key over `area`.
    pub fn key(&mut self, k: KeyEvent, area: Rect) -> Out {
        self.note = None;
        let room = body_rows(area);
        if let Some(t) = self.help {
            let rows = (area.height as usize).saturating_sub(2);
            self.help = chrome::scroll(k.code, t, help_rows(self.opts.ascii).len().saturating_sub(rows), rows);
            return Out::Stay;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // ^C closes from anywhere, the find field included (D14)
        if ctrl && k.code == KeyCode::Char('c') {
            return Out::Close;
        }
        if self.field.is_some() {
            self.field_key(k, room);
            return Out::Stay;
        }
        let small = area.width < MIN_W || area.height < MIN_H;
        match k.code {
            KeyCode::Char('q') => return Out::Close,
            KeyCode::Esc if !small && !self.query.is_empty() => {
                self.query.clear();
                self.refind();
                self.cur = None;
            }
            KeyCode::Esc => return Out::Close,
            KeyCode::Char('e') => return Out::Edit(self.line()),
            _ if small => {}
            KeyCode::Char('?') | KeyCode::F(1) => self.help = Some(0),
            KeyCode::Char('/') => self.field = Some((String::new(), self.top)),
            KeyCode::Char('n') => self.next(false, room),
            KeyCode::Char('N') => self.next(true, room),
            KeyCode::Char('u') => {
                self.opts.urls = !self.opts.urls;
                self.width = 0;
                self.layout(body_width(area));
            }
            KeyCode::Char('g') | KeyCode::Home => self.top = 0,
            KeyCode::Char('G') | KeyCode::End => self.top = self.bottom(room),
            KeyCode::Char(' ') => self.scroll(KeyCode::PageDown, room),
            c => self.scroll(c, room),
        }
        Out::Stay
    }

    fn scroll(&mut self, c: KeyCode, room: usize) {
        if let Some(t) = chrome::scroll(c, self.top, self.bottom(room), room) {
            self.top = t;
        }
    }

    /// The wheel scrolls; a click does nothing.
    pub fn mouse(&mut self, m: MouseEvent, area: Rect) {
        let room = body_rows(area);
        match m.kind {
            MouseEventKind::ScrollUp => self.top = self.top.saturating_sub(WHEEL),
            MouseEventKind::ScrollDown => self.top = (self.top + WHEEL).min(self.bottom(room)),
            _ => {}
        }
    }
}

/// The body's rows: the frame takes two and the footer one.
pub fn body_rows(area: Rect) -> usize {
    (area.height as usize).saturating_sub(3)
}

/// The body's columns: the frame and a column of padding each side.
pub fn body_width(area: Rect) -> usize {
    (area.width as usize).saturating_sub(4)
}

pub fn help_rows(ascii: bool) -> Vec<(String, String)> {
    let kv = |k: &str, v: &str| (k.to_string(), v.to_string());
    let ud = if ascii { "up/dn j k" } else { "↑↓ j k" };
    vec![
        kv("", "move"),
        kv(ud, "scroll a line"),
        kv("pgup pgdn space", "a page"),
        kv("g G home end", "the top, the end"),
        kv("wheel", "scroll three lines"),
        kv("", "find"),
        kv("/", "find; a capital makes case count"),
        kv("n N", "the next, the previous match"),
        kv("esc", "clear the matches"),
        kv("", "the file"),
        kv("e", "close, and edit it at the top line"),
        kv("u", "show or hide link addresses"),
        kv("?", "this help"),
        kv("q esc ^C", "close"),
    ]
}

fn ink_style(th: &Theme, i: Ink) -> Style {
    let mut s = match i.kind {
        Kind::Text => Style::new().fg(th.value),
        Kind::Heading(1) => Style::new().fg(th.accent).add_modifier(Modifier::UNDERLINED),
        Kind::Heading(2) => Style::new().fg(th.accent),
        Kind::Heading(_) => Style::new().fg(th.value),
        Kind::Quote => Style::new().fg(th.quote).add_modifier(Modifier::ITALIC),
        Kind::Code(r) => crate::panel::view::role_style(th, r, Style::new().fg(th.value)),
        Kind::Inline => Style::new().fg(th.sky),
        Kind::Link => Style::new().fg(th.accent).add_modifier(Modifier::UNDERLINED),
        Kind::Url | Kind::Meta => Style::new().fg(th.meta),
        Kind::Bullet => Style::new().fg(th.accent),
        Kind::Frame => Style::new().fg(th.faint),
    };
    if i.bold {
        s = s.add_modifier(Modifier::BOLD);
    }
    if i.italic {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if i.strike {
        s = s.add_modifier(Modifier::CROSSED_OUT);
    }
    s
}

/// One row's spans, a match reversed and the current one bold as well, so
/// both read without colour.
fn line(th: &Theme, row: &Row, hits: &[(usize, usize, bool)]) -> Line<'static> {
    let mut out = Vec::new();
    let mut at = 0;
    for (t, ink) in &row.cells {
        let base = ink_style(th, *ink);
        let end = at + t.len();
        let mut i = at;
        while i < end {
            let hit = hits.iter().find(|h| h.0 <= i && i < h.1);
            let next = hits
                .iter()
                .flat_map(|h| [h.0, h.1])
                .filter(|&b| b > i && b < end)
                .min()
                .unwrap_or(end);
            let st = match hit {
                Some((_, _, true)) => base.add_modifier(Modifier::REVERSED | Modifier::BOLD),
                Some(_) => base.add_modifier(Modifier::REVERSED),
                None => base,
            };
            out.push(Span::styled(t[i - at..next - at].to_string(), st));
            i = next;
        }
        at = end;
    }
    Line::from(out)
}

fn hints(s: &Screen, th: &Theme) -> Vec<(String, String)> {
    let kv = |k: &str, v: &str| (k.to_string(), v.to_string());
    let mut h = vec![kv("/", "find")];
    if !s.hits.is_empty() {
        h.push(kv("n N", "next"));
    }
    h.push(kv("e", "edit"));
    h.push(kv(th.g.key_updown, "scroll"));
    h.push(kv("?", "keys"));
    h.push(kv("q", "close"));
    h
}

/// The footer while the find field is open: the text, the cursor, the count.
fn field_line(th: &Theme, s: &Screen, text: &str, w: usize) -> Line<'static> {
    let count = match (s.hits.len(), s.cur) {
        (0, _) if text.is_empty() => String::new(),
        (0, _) => "  no match".into(),
        (n, Some(c)) => format!("  {} of {n}", c + 1),
        (n, None) => format!("  {n}"),
    };
    let room = w.saturating_sub(3 + render::cols(&count) + 1);
    let shown = th.g.trunc_left(text, room);
    Line::from(vec![
        Span::styled(" /", Style::new().fg(th.accent)),
        Span::styled(shown, Style::new().fg(th.value)),
        Span::styled(th.g.cursor, Style::new().fg(th.accent)),
        Span::styled(count, Style::new().fg(th.meta)),
    ])
}

pub fn render(f: &mut Frame, area: Rect, th: &Theme, s: &mut Screen) {
    if area.width < MIN_W || area.height < MIN_H {
        chrome::floor(f, area, th, "view", "make the window larger", "q closes");
        return;
    }
    if let Some(top) = s.help {
        chrome::help(f, area, th, &help_rows(th.g.ascii), top);
        return;
    }
    s.layout(body_width(area));
    let [body, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    let room = body_rows(area);
    s.top = s.top.min(s.bottom(room));
    let total = s.rows.len();
    let last = (s.top + room).min(total);
    let title_w = (area.width as usize).saturating_sub(4 + if total > room { 18 } else { 0 });
    let mut block = Block::new()
        .borders(Borders::ALL)
        .border_set(th.g.round)
        .border_style(Style::new().fg(th.label))
        .title(Line::from(Span::styled(
            format!(" {} ", th.g.trunc_middle(&s.name, title_w.saturating_sub(2))),
            Style::new().fg(th.accent),
        )));
    if total > room {
        let n = format!(" {}-{last} of {total} ", s.top + 1);
        block = block.title(Line::from(Span::styled(n, Style::new().fg(th.meta))).right_aligned());
    }
    let inner = block.inner(body);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    f.render_widget(Clear, area);
    f.render_widget(block, body);
    let w = inner.width as usize;
    let lines: Vec<Line> = match &s.doc {
        Err(e) => crate::panel::view::wrap(&format!("{e}: e opens it in the editor"), w)
            .into_iter()
            .map(|t| Line::styled(t, Style::new().fg(th.needs)))
            .collect(),
        Ok(_) if total == 0 => vec![Line::styled("an empty file", Style::new().fg(th.meta))],
        Ok(_) => (s.top..last)
            .map(|r| {
                // hits are in row order: the row's run of them
                let from = s.hits.partition_point(|h| h.row < r);
                let hs: Vec<(usize, usize, bool)> = s.hits[from..]
                    .iter()
                    .take_while(|h| h.row == r)
                    .enumerate()
                    .map(|(i, h)| (h.from, h.to, s.cur == Some(from + i)))
                    .collect();
                line(th, &s.rows[r], &hs)
            })
            .collect(),
    };
    f.render_widget(Paragraph::new(lines), inner);
    let w = area.width as usize;
    let foot_line = match (&s.field, &s.note) {
        (Some((text, _)), _) => field_line(th, s, text, w),
        (None, Some(n)) => Line::styled(format!(" {}", th.g.trunc(n, w.saturating_sub(2))), Style::new().fg(th.needs)),
        _ => chrome::footer(th, hints(s, th), w, 3),
    };
    f.render_widget(Paragraph::new(foot_line), foot);
}

const USAGE: &str = "cockpit-tui view [--line <n>] [--] <file>";

/// `cockpit-tui view [--line N] [--] <file>`: exit 0, with `edit <line>` on
/// stdout after `e` and nothing after a close; 2 for a bad argument or no
/// terminal. A file it cannot show is a note on the screen, not an exit.
pub fn main(args: &[String]) -> i32 {
    let (mut line, mut file) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--line" if line.is_none() => match it.next().and_then(|n| n.parse::<usize>().ok()) {
                Some(n) => line = Some(n),
                None => return usage("--line takes a number"),
            },
            "--" => {
                file = it.next().cloned();
                if it.next().is_some() {
                    return usage("one file");
                }
                break;
            }
            s if s.starts_with('-') => return usage(&format!("unknown argument {s}")),
            _ if file.is_none() => file = Some(a.clone()),
            _ => return usage("one file"),
        }
    }
    let Some(file) = file.filter(|f| !f.is_empty()) else {
        return usage("it needs a file");
    };
    let path = std::path::Path::new(&file);
    let name = path.file_name().map_or(file.clone(), |n| n.to_string_lossy().into_owned());
    let th = Theme::load();
    let opts = Opts {
        ascii: th.g.ascii,
        urls: true,
        colour: th.accent != Color::Reset,
    };
    let mut s = Screen::new(&name, load(path, MAX_FILE), opts);
    if let Some(n) = line {
        s = s.at_line(n);
    }
    let mut term = match open_screen("view") {
        Ok(t) => t,
        Err(c) => return c,
    };
    let out = loop {
        if term.draw(|f| render(f, f.area(), &th, &mut s)).is_err() {
            break Out::Close;
        }
        match event::read() {
            Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => match s.key(k, screen(&term)) {
                Out::Stay => {}
                o => break o,
            },
            Ok(Event::Mouse(m)) => s.mouse(m, screen(&term)),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break Out::Close,
        }
    };
    drop(term);
    restore_tty();
    if let Out::Edit(n) = out {
        println!("edit {n}");
    }
    0
}

fn usage(why: &str) -> i32 {
    eprintln!("cockpit-tui: view: {why}\n{USAGE}");
    2
}
