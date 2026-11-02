//! The picker: rows in, one key out. It replaces fzf in the session picker, the
//! branch picker and open-at-line. Like the board it never mutates: the chosen
//! key goes to stdout and the caller acts on it.
//!
//! Rows arrive on stdin as `<key>\t<display>`, so the keys come from /dev/tty and
//! the screen is drawn there too: stdout is the answer, and `$(...)` would swallow
//! a UI drawn on it.
use crate::backend::{self, WideCellBackend};
use crate::chrome;
use crate::theme::{Glyphs, Theme};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::io::{Read, Write};
use unicode_width::UnicodeWidthStr;

/// Below this the list cannot show a row and the footer's way out together.
const MIN_W: u16 = 28;
const MIN_H: u16 = 12;
/// The preview needs a list worth reading beside it.
const PREVIEW_MIN_W: u16 = 60;
/// A wheel notch, in rows.
const WHEEL: usize = 3;
/// Rows and preview are text for a person: a runaway producer must not fill memory.
const MAX_INPUT: usize = 8 << 20;
const MAX_PREVIEW: u64 = 64 << 10;

pub struct Row {
    /// Raw bytes: a path need not be UTF-8, and the caller gets exactly what it sent.
    pub key: Vec<u8>,
    pub text: String,
}

#[derive(Debug)]
pub struct Opts {
    pub prompt: String,
    pub header: String,
    /// A path, read once before the screen opens.
    pub preview: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Choose(Vec<u8>),
    Cancel,
}

pub struct Pick {
    pub rows: Vec<Row>,
    pub query: String,
    /// Indexes into `rows` that match `query`, in input order.
    pub shown: Vec<usize>,
    /// An index into `shown`.
    pub sel: usize,
    pub top: usize,
    pub help: Option<usize>,
    pub prompt: String,
    pub header: String,
    pub preview: Option<String>,
}

/// Characters that reorder what is drawn: the direction marks and the bidi
/// embeddings, overrides and isolates. A row could otherwise display one name and
/// print another. U+200C and U+200D stay: Persian, Indic scripts and emoji need them.
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Untrusted text made safe to draw: a tab is a space; every other control
/// character (ESC, BEL, the C1 range) and every bidi control is dropped.
pub fn clean(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            c if c.is_control() || is_bidi(c) => None,
            c => Some(c),
        })
        .collect()
}

/// `clean` per line, keeping the line breaks.
fn clean_lines(s: &str) -> String {
    s.lines().map(clean).collect::<Vec<_>>().join("\n")
}

/// `<key>\t<display>` per line, as bytes. No tab: the line is both. A line with
/// no key is skipped, since choosing it would print nothing. The display is
/// decoded lossily; the key never is.
pub fn parse_rows(input: &[u8]) -> Vec<Row> {
    input
        .split(|&b| b == b'\n')
        .filter_map(|l| {
            let l = l.strip_suffix(b"\r").unwrap_or(l);
            let (key, text) = match l.iter().position(|&b| b == b'\t') {
                Some(i) => (&l[..i], &l[i + 1..]),
                None => (l, l),
            };
            (!key.is_empty()).then(|| Row {
                key: key.to_vec(),
                text: clean(&String::from_utf8_lossy(text)),
            })
        })
        .collect()
}

/// Lines that have a tab but nothing before it: the rows `parse_rows` skips. A
/// line with no tab at all is its own key, so it is never counted.
pub fn keyless(input: &[u8]) -> usize {
    input.split(|&b| b == b'\n').filter(|l| l.first() == Some(&b'\t')).count()
}

/// The one stderr line for them, or `None` when nothing was skipped.
pub fn keyless_note(n: usize) -> Option<String> {
    (n > 0).then(|| format!("cockpit-tui: pick: skipped {n} row{} with no key", if n == 1 { "" } else { "s" }))
}

/// At most `cap` bytes, ending on a line boundary so a key is never cut; and
/// whether anything was dropped.
pub fn cap_input(mut b: Vec<u8>, cap: usize) -> (Vec<u8>, bool) {
    if b.len() <= cap {
        return (b, false);
    }
    let end = b[..=cap].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
    b.truncate(end);
    (b, true)
}

/// Writes the answer. 0 for a chosen key, 1 for a cancel, 2 when stdout is gone.
pub fn answer(out: &mut impl Write, o: &Outcome) -> i32 {
    let Outcome::Choose(k) = o else { return 1 };
    match out.write_all(k).and_then(|_| out.write_all(b"\n")).and_then(|_| out.flush()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cockpit-tui: pick: cannot write the key: {e}");
            2
        }
    }
}

/// Case-insensitive substring over the display text; the indexes that match.
pub fn filter(rows: &[Row], q: &str) -> Vec<usize> {
    let q = q.to_lowercase();
    (0..rows.len())
        .filter(|&i| q.is_empty() || rows[i].text.to_lowercase().contains(&q))
        .collect()
}

pub fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts {
        prompt: "pick".into(),
        header: String::new(),
        preview: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let slot = match a.as_str() {
            "--prompt" => &mut o.prompt,
            "--header" => &mut o.header,
            "--preview" => {
                o.preview = Some(it.next().ok_or("--preview needs a file")?.clone());
                continue;
            }
            other => return Err(format!("unknown argument {other}")),
        };
        *slot = it.next().ok_or_else(|| format!("{a} needs a value"))?.clone();
    }
    Ok(o)
}

/// Under the floor the card hides the list, so only its way out does anything.
pub fn too_small(area: Rect) -> bool {
    area.width < MIN_W || area.height < MIN_H
}

struct Geo {
    prompt: Rect,
    header: Rect,
    list: Rect,
    preview: Option<Rect>,
    footer: Rect,
}

fn geo(area: Rect, p: &Pick) -> Geo {
    let hh = u16::from(!p.header.is_empty());
    let [prompt, header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(hh),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    if p.preview.is_some() && area.width >= PREVIEW_MIN_W {
        let [list, pv] = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(body);
        return Geo {
            prompt,
            header,
            list,
            preview: Some(pv),
            footer,
        };
    }
    Geo {
        prompt,
        header,
        list: body,
        preview: None,
        footer,
    }
}

impl Pick {
    pub fn new(rows: Vec<Row>, o: &Opts, preview_text: Option<String>) -> Self {
        let shown = filter(&rows, "");
        Self {
            rows,
            query: String::new(),
            shown,
            sel: 0,
            top: 0,
            help: None,
            prompt: clean(&o.prompt),
            header: clean(&o.header.replace('\n', " ")),
            preview: preview_text.map(|t| clean_lines(&t)),
        }
    }

    fn room(&self, area: Rect) -> usize {
        geo(area, self).list.height as usize
    }

    /// Keeps the selection on a row and the window around it.
    pub fn clamp(&mut self, area: Rect) {
        let room = self.room(area).max(1);
        self.sel = self.sel.min(self.shown.len().saturating_sub(1));
        self.top = self.top.min(self.shown.len().saturating_sub(room));
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + room {
            self.top = self.sel + 1 - room;
        }
    }

    fn requery(&mut self, area: Rect) {
        self.shown = filter(&self.rows, &self.query);
        self.sel = 0;
        self.top = 0;
        self.clamp(area);
    }

    fn choose(&self) -> Outcome {
        match self.shown.get(self.sel) {
            Some(&i) => Outcome::Choose(self.rows[i].key.clone()),
            None => Outcome::Continue,
        }
    }

    /// The picker is a text-entry screen: every printable key is a letter of the
    /// query, so nothing a person types first is a command.
    pub fn key(&mut self, k: KeyEvent, area: Rect) -> Outcome {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if (ctrl && k.code == KeyCode::Char('c')) || (k.code == KeyCode::Esc && self.help.is_none()) {
            return Outcome::Cancel;
        }
        if too_small(area) {
            self.help = None;
            return Outcome::Continue;
        }
        if let Some(t) = self.help {
            // any key but a scroll key closes it, and never acts
            let rows = area.height.saturating_sub(2) as usize;
            self.help = chrome::scroll(k.code, t, help_rows(&Glyphs::unicode()).len().saturating_sub(rows), rows);
            return Outcome::Continue;
        }
        let last = self.shown.len().saturating_sub(1);
        let code = match (ctrl, k.code) {
            (true, KeyCode::Char('n')) => KeyCode::Down,
            (true, KeyCode::Char('p')) => KeyCode::Up,
            (_, c) => c,
        };
        match code {
            KeyCode::Enter => return self.choose(),
            KeyCode::F(1) => self.help = Some(0),
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                if let Some(s) = chrome::scroll(code, self.sel, last, self.room(area)) {
                    self.sel = s;
                }
            }
            KeyCode::Home => self.sel = 0,
            KeyCode::End => self.sel = last,
            KeyCode::Backspace => {
                self.query.pop();
                self.requery(area);
            }
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.requery(area);
            }
            // Shift is how a capital arrives; Ctrl and Alt make a chord, not a letter
            KeyCode::Char(c) if k.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                self.query.push(c);
                self.requery(area);
            }
            _ => {}
        }
        self.clamp(area);
        Outcome::Continue
    }

    /// A click selects, the wheel scrolls the window. Neither chooses.
    pub fn mouse(&mut self, m: MouseEvent, area: Rect) {
        if too_small(area) || self.help.is_some() {
            return;
        }
        let g = geo(area, self);
        let room = g.list.height as usize;
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let inside = g.list.contains((m.column, m.row).into());
                let i = self.top + (m.row.saturating_sub(g.list.y)) as usize;
                if inside && i < self.shown.len() {
                    self.sel = i;
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let bottom = self.shown.len().saturating_sub(room);
                self.top = if m.kind == MouseEventKind::ScrollDown {
                    (self.top + WHEEL).min(bottom)
                } else {
                    self.top.saturating_sub(WHEEL)
                };
                // the selection follows the window, never leaves it
                self.sel = self
                    .sel
                    .clamp(self.top, (self.top + room.max(1) - 1).min(self.shown.len().saturating_sub(1)));
            }
            _ => {}
        }
    }
}

/// Section heading rows have an empty key (see `chrome::help`).
pub fn help_rows(g: &Glyphs) -> Vec<(String, String)> {
    let r = |k: &str, v: &str| (k.to_string(), v.to_string());
    vec![
        r("", "type"),
        r("any key", "adds to the filter, j k q ? included"),
        r("⌫  ^U", "delete a letter / clear the filter"),
        r("", "move"),
        r(&format!("{}  ^N ^P", g.key_updown), "one row"),
        r("PgUp PgDn", "a page"),
        r("Home End", "first / last"),
        r("click", "select the row, ↵ chooses it"),
        r("wheel", "scroll"),
        r("", "choose"),
        r(g.key_enter, "print this row's key, exit"),
        r("", "leave"),
        r("esc  ^C", "cancel, print nothing"),
        r("F1", "this help"),
    ]
}

/// ↵ and esc come first: at the narrowest popup they are all that fits, and
/// `chrome::fit` would drop the choose key before the help key.
fn hints(g: &Glyphs, w: usize) -> Vec<(String, String)> {
    let h = |k: &str, v: &str| (k.to_string(), v.to_string());
    let fitted = chrome::fit(
        vec![
            h(g.key_enter, "choose"),
            h(g.key_updown, "move"),
            h("F1", "keys"),
            h("esc", "cancel"),
        ],
        w,
        3,
    );
    if fitted.iter().any(|(k, _)| k == g.key_enter) {
        fitted
    } else {
        vec![h(g.key_enter, "choose"), h("esc", "cancel")]
    }
}

pub fn render(f: &mut Frame, area: Rect, p: &Pick, th: &Theme) {
    if area.width < MIN_W {
        chrome::floor(f, area, th, "pick", "make the window larger or rotate", "esc closes");
        return;
    }
    if area.height < MIN_H {
        let msg = format!("too short {} make the window larger or rotate", th.g.dot);
        chrome::floor(f, area, th, "pick", &msg, "esc closes");
        return;
    }
    if let Some(top) = p.help {
        chrome::help(f, area, th, &help_rows(&th.g), top);
        return;
    }
    let g = geo(area, p);
    let value = Style::new().fg(th.value);
    prompt_line(f, g.prompt, p, th);
    if !p.header.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(th.g.trunc(&p.header, area.width as usize - 2), value)),
            pad(g.header),
        );
    }
    list(f, g.list, p, th);
    if let Some(pv) = g.preview {
        let b = Block::new()
            .borders(Borders::LEFT)
            .border_set(th.g.round)
            .border_style(Style::new().fg(th.faint));
        let inner = b.inner(pv);
        f.render_widget(b, pv);
        let w = (inner.width as usize).saturating_sub(1);
        let lines: Vec<Line> = p
            .preview
            .as_deref()
            .unwrap_or_default()
            .lines()
            .take(inner.height as usize)
            .map(|l| Line::styled(th.g.trunc(l, w), value))
            .collect();
        f.render_widget(Paragraph::new(lines), pad(inner));
    }
    f.render_widget(
        Paragraph::new(chrome::footer(th, hints(&th.g, area.width as usize), area.width as usize, 3)),
        g.footer,
    );
}

/// One column in from the left edge.
fn pad(r: Rect) -> Rect {
    Rect {
        x: r.x + 1,
        width: r.width.saturating_sub(1),
        ..r
    }
}

fn prompt_line(f: &mut Frame, r: Rect, p: &Pick, th: &Theme) {
    let (accent, value) = (Style::new().fg(th.accent), Style::new().fg(th.value));
    let count = if p.shown.is_empty() {
        "0/0".to_string()
    } else if p.query.is_empty() {
        format!("{}/{}", p.sel + 1, p.shown.len())
    } else {
        format!("{}/{} of {}", p.sel + 1, p.shown.len(), p.rows.len())
    };
    let [left, right] = Layout::horizontal([Constraint::Min(0), Constraint::Length(count.width() as u16 + 1)]).areas(r);
    let prompt = th.g.trunc(&p.prompt, (left.width as usize / 2).max(1));
    let room = (left.width as usize).saturating_sub(prompt.width() + 4);
    let mut spans = vec![Span::raw(" "), Span::styled(format!("{prompt}>"), accent), Span::raw(" ")];
    spans.push(Span::styled(th.g.trunc_left(&p.query, room), value));
    spans.push(Span::styled(th.g.cursor, accent));
    f.render_widget(Paragraph::new(Line::from(spans)), left);
    f.render_widget(Paragraph::new(Span::styled(count, value)), right);
}

fn list(f: &mut Frame, r: Rect, p: &Pick, th: &Theme) {
    let (accent, value) = (Style::new().fg(th.accent), Style::new().fg(th.value));
    if p.shown.is_empty() {
        let msg = format!("no match for \"{}\"", p.query);
        let msg = th.g.trunc(&msg, (r.width as usize).saturating_sub(2));
        f.render_widget(Paragraph::new(Span::styled(msg, value)), pad(r));
        return;
    }
    let w = (r.width as usize).saturating_sub(2);
    let lines: Vec<Line> = (p.top..p.shown.len().min(p.top + r.height as usize))
        .map(|i| {
            let text = th.g.trunc(&p.rows[p.shown[i]].text, w);
            if i == p.sel {
                Line::from(vec![
                    Span::styled(th.g.active, accent),
                    Span::styled(format!(" {text}"), value.add_modifier(Modifier::BOLD)),
                ])
            } else {
                Line::from(vec![Span::raw(" "), Span::styled(format!(" {text}"), value)])
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), r);
}

const USAGE: &str = "cockpit-tui pick [--prompt <text>] [--header <text>] [--preview <file>]  (rows on stdin: <key>TAB<display>)";

/// Leaves raw mode and the alternate screen, on the tty the screen was drawn on.
fn restore_tty() {
    let _ = crossterm::terminal::disable_raw_mode();
    if let Ok(mut t) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        let _ = t.write_all(format!("{}\x1b[?25h\x1b[?1049l", backend::MOUSE_OFF).as_bytes());
    }
}

fn read_limited(r: impl Read, cap: u64) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::new();
    r.take(cap).read_to_end(&mut b)?;
    Ok(b)
}

/// Exit 0 with the key on stdout, 1 for esc, q or no rows (stdout empty), 2 for
/// a bad argument or no terminal.
pub fn main(args: &[String]) -> i32 {
    let o = match parse_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("cockpit-tui: pick: {e}\n{USAGE}");
            return 2;
        }
    };
    let preview = match &o.preview {
        None => None,
        Some(f) => match std::fs::File::open(f).and_then(|f| read_limited(f, MAX_PREVIEW)) {
            Ok(t) => Some(String::from_utf8_lossy(&t).into_owned()),
            Err(e) => {
                eprintln!("cockpit-tui: pick: cannot read {f}: {e}");
                return 2;
            }
        },
    };
    let input = match read_limited(std::io::stdin().lock(), MAX_INPUT as u64 + 1) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cockpit-tui: pick: cannot read the rows: {e}");
            return 2;
        }
    };
    let (input, cut) = cap_input(input, MAX_INPUT);
    let rows = parse_rows(&input);
    let note = keyless_note(keyless(&input));
    if rows.is_empty() {
        if let Some(n) = note {
            eprintln!("{n}");
        }
        return 1;
    }
    let tty = match std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty") {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cockpit-tui: pick: no terminal to draw on: {e}");
            return 2;
        }
    };
    // crossterm caches a failed event-reader init forever: probe before the screen opens
    if let Err(e) = event::poll(std::time::Duration::ZERO) {
        eprintln!("cockpit-tui: pick: no usable terminal: {e}");
        return 2;
    }
    backend::exit_on_hangup(restore_tty);
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_tty();
        hook(info);
    }));
    let th = Theme::load();
    let mut p = Pick::new(rows, &o, preview);
    if cut {
        let note = format!("(first {} rows)", p.rows.len());
        p.header = if p.header.is_empty() {
            note
        } else {
            format!("{}  {note}", p.header)
        };
    }
    let mut term = match crossterm::terminal::enable_raw_mode().and_then(|_| {
        let mut t = tty;
        t.write_all(format!("\x1b[?1049h{}", backend::MOUSE_ON).as_bytes())?;
        Terminal::new(WideCellBackend::new(t))
    }) {
        Ok(t) => t,
        Err(e) => {
            restore_tty();
            eprintln!("cockpit-tui: pick: {e}");
            return 2;
        }
    };
    let area = |t: &Terminal<WideCellBackend<std::fs::File>>| {
        let s = t.size().unwrap_or_default();
        Rect::new(0, 0, s.width, s.height)
    };
    let out = loop {
        p.clamp(area(&term));
        if term.draw(|f| render(f, f.area(), &p, &th)).is_err() {
            break Outcome::Cancel;
        }
        match event::read() {
            Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => match p.key(k, area(&term)) {
                Outcome::Continue => {}
                o => break o,
            },
            Ok(Event::Mouse(m)) => p.mouse(m, area(&term)),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break Outcome::Cancel,
        }
    };
    drop(term);
    restore_tty();
    if let Some(n) = note {
        eprintln!("{n}");
    }
    answer(&mut std::io::stdout().lock(), &out)
}
