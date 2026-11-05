//! The info screen (#252): what the plugin knows about one pane, in the order
//! you ask it. `cockpit-tui info %N` runs it alone in a popup (prefix i); the
//! board draws the same screen as an overlay over the selected tile. It is not
//! a text-entry screen, so esc and q both close it.
use crate::chrome;
use crate::data::fmt_dur;
use crate::data::info::{Facts, WorkFrom, gather};
use crate::panel::view::wrap;
use crate::theme::{Glyphs, Theme};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

/// The label column: `review dir` is the longest word, and a space after it.
const LABEL_W: usize = 11;
/// Narrower than this the label column eats the value.
const MIN_W: u16 = 40;
const MIN_H: u16 = 8;

pub struct Screen {
    pub facts: Result<Facts, String>,
    pub top: usize,
    /// the `?` help's scroll offset while it is open
    pub help: Option<usize>,
}

pub enum Out {
    Stay,
    Close,
}

impl Screen {
    pub fn open(pane: &str) -> Self {
        Self::of(gather(pane))
    }

    pub fn of(facts: Result<Facts, String>) -> Self {
        Self { facts, top: 0, help: None }
    }
}

/// `label value` rows, a long value wrapped under its own column.
pub fn rows(x: &Facts, g: &Glyphs, w: usize) -> Vec<(String, String)> {
    let dash = g.dash;
    let mut r: Vec<(&str, String)> = vec![("pane", format!("{}  {}  ({})", x.id, x.wher, x.cmd)), ("cwd", x.cwd.clone())];
    // #452: the transcript's cwd, when the board's stat used it, is its own row
    let said = x.work_from == WorkFrom::Transcript;
    if let Some(d) = &x.dir {
        let note = match (x.dir_missing, said) {
            (false, _) => "@cockpit_dir",
            (true, false) => "@cockpit_dir, missing -- using cwd",
            (true, true) => "@cockpit_dir, missing -- using transcript cwd",
        };
        r.push(("dir", format!("{d}  ({note})")));
    }
    if said {
        r.push(("dir", format!("{}  (transcript cwd)", crate::data::info::clean(&x.work))));
    }
    r.push(("root", x.root.clone()));
    let msg = if x.msg.is_empty() {
        String::new()
    } else {
        format!("  {} {}", g.dot, x.msg)
    };
    r.push(("state", format!("{}{msg}", if x.state.is_empty() { dash } else { &x.state })));
    if let Some((name, id)) = &x.job {
        r.push(("job", format!("{name}  (@cockpit_job {id})")));
    }
    for d in &x.doing {
        let age = |pre: &str| d.age.map(|a| format!(" ({pre}{})", fmt_dur(a))).unwrap_or_default();
        match d.kind.as_str() {
            "doing" => r.push(("doing", format!("{}{}", d.text, age("since ")))),
            "said" => r.push(("said", d.text.clone())),
            _ => r.push(("asked", format!("{}{}", d.text, age("")))),
        }
    }
    if x.agent {
        r.push(("ask it", "/btw <question> in the pane (answers while it works)".into()));
    }
    let spool = if x.spool.is_empty() {
        String::new()
    } else {
        format!("  {} queued: {}", g.dot, x.spool)
    };
    r.push(("review", format!("{}{spool}", if x.review.is_empty() { dash } else { &x.review })));
    if let Some(b) = &x.base {
        r.push(("base", format!("{}  ({})", b.label, b.rev)));
    }
    r.push(("session", if x.sid.is_empty() { dash.into() } else { x.sid.clone() }));
    r.push(("title", if x.title.is_empty() { dash.into() } else { x.title.clone() }));
    r.push(("tui", x.tui.clone()));
    if let Some((d, pinned, delivered)) = &x.rev_dir {
        r.push(("review dir", d.clone()));
        r.push((
            "",
            format!(
                "{pinned} pinned {} {delivered} delivered {} a at the prompt removes it",
                g.dot, g.dot
            ),
        ));
    }
    let room = w.saturating_sub(LABEL_W).max(4);
    let mut out = vec![];
    for (k, v) in r {
        for (i, l) in wrap(&v, room).into_iter().enumerate() {
            out.push((if i == 0 { k.to_string() } else { String::new() }, l));
        }
    }
    out
}

fn help_rows(g: &Glyphs) -> Vec<(String, String)> {
    let kv = |k: &str, v: &str| (k.to_string(), v.to_string());
    vec![
        kv("esc q", "close"),
        kv(&format!("{} j k", g.key_updown), "scroll"),
        kv("pgup pgdn", "a page"),
        kv("?", "this help"),
    ]
}

/// The body's first and last offsets: the card's border and the footer take three rows.
fn room(area: Rect) -> usize {
    (area.height as usize).saturating_sub(3)
}

fn inner_w(area: Rect) -> usize {
    (area.width as usize).saturating_sub(4)
}

fn bottom(x: &Facts, g: &Glyphs, area: Rect) -> usize {
    rows(x, g, inner_w(area)).len().saturating_sub(room(area))
}

pub fn render(f: &mut Frame, area: Rect, th: &Theme, s: &Screen) {
    if area.width < MIN_W || area.height < MIN_H {
        chrome::floor(f, area, th, "info", "make the window larger or rotate", "esc or q closes");
        return;
    }
    let x = match &s.facts {
        Ok(x) => x,
        Err(e) => {
            chrome::floor(f, area, th, "info", e, "esc or q closes");
            return;
        }
    };
    if let Some(top) = s.help {
        chrome::help(f, area, th, &help_rows(&th.g), top);
        return;
    }
    let [body, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    let all = rows(x, &th.g, inner_w(area));
    let room = room(area);
    let top = s.top.min(all.len().saturating_sub(room));
    let mut block = Block::new()
        .borders(Borders::ALL)
        .border_set(th.g.round)
        .border_style(Style::new().fg(th.label))
        .title(Line::from(Span::styled(
            format!(" info {} {} ", th.g.dot, x.id),
            Style::new().fg(th.accent),
        )));
    let last = (top + room).min(all.len());
    if all.len() > room {
        let n = format!(" {}-{last} of {} ", top + 1, all.len());
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
    let lines: Vec<Line> = all[top..last]
        .iter()
        .map(|(k, v)| {
            let pad = " ".repeat(LABEL_W.saturating_sub(k.width()));
            Line::from(vec![
                Span::styled(format!("{k}{pad}"), Style::new().fg(th.label)),
                Span::styled(v.clone(), Style::new().fg(th.value)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
    let hints = vec![
        ("esc".to_string(), "close".to_string()),
        (th.g.key_updown.to_string(), "scroll".to_string()),
        ("?".to_string(), "keys".to_string()),
    ];
    f.render_widget(chrome::footer(th, hints, area.width as usize, 3), foot);
}

/// A key over `area`. The help takes any key but a scroll key as its close.
pub fn key(s: &mut Screen, th: &Theme, code: KeyCode, area: Rect) -> Out {
    if let Some(t) = s.help {
        let rows = (area.height as usize).saturating_sub(2);
        s.help = chrome::scroll(code, t, help_rows(&th.g).len().saturating_sub(rows), rows);
        return Out::Stay;
    }
    if matches!(code, KeyCode::Esc | KeyCode::Char('q')) || area.width < MIN_W || area.height < MIN_H {
        return Out::Close;
    }
    let Ok(x) = &s.facts else { return Out::Stay };
    match code {
        KeyCode::Char('?') | KeyCode::F(1) => s.help = Some(0),
        c => {
            let b = bottom(x, &th.g, area);
            if let Some(t) = chrome::scroll(c, s.top, b, room(area)) {
                s.top = t;
            }
        }
    }
    Out::Stay
}

/// The rows as the shell printed them: a label padded to ten, a space, the
/// value, nothing wrapped. `cockpit-tui info --text %N` prints these, for a
/// script and for the suite's field checks.
pub fn text(x: &Facts) -> String {
    rows(x, &Glyphs::unicode(), 100_000)
        .iter()
        // the shell's continuation line is eleven spaces, not ten
        .map(|(k, v)| {
            if k.is_empty() {
                format!("{:11}{v}\n", "")
            } else {
                format!("{k:<9} {v}\n")
            }
        })
        .collect()
}

/// `cockpit-tui info [--text|--json] %N`: the screen alone, until esc or q. Exit 2
/// for a bad pane id, 1 for no terminal or no such pane. `--json` is for scripts:
/// `{"dir": ...}`, the work dir the board's stat uses (#452), as lossy UTF-8.
pub fn main(args: &[String]) -> i32 {
    let (flag, args) = match args.split_first() {
        Some((f, rest)) if f == "--text" || f == "--json" => (f.as_str(), rest),
        _ => ("", args),
    };
    let Some(pane) = args.first().filter(|p| {
        args.len() == 1
            && p.strip_prefix('%')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    }) else {
        eprintln!("cockpit-tui: info needs one pane id like %3, after an optional --text or --json");
        return 2;
    };
    if !flag.is_empty() {
        return match gather(pane) {
            Ok(x) if flag == "--json" => {
                println!("{}", serde_json::json!({"dir": x.work}));
                0
            }
            Ok(x) => {
                print!("{}", text(&x));
                0
            }
            Err(e) => {
                eprintln!("cockpit-tui: {e}");
                1
            }
        };
    }
    // a popup whose window is killed leaves a tty that reads EOF forever
    crate::backend::exit_on_hangup(ratatui::restore);
    let th = Theme::load();
    let mut s = Screen::open(pane);
    if event::poll(Duration::ZERO).is_err() {
        eprintln!("cockpit-tui: info: no usable terminal");
        return 1;
    }
    let mut term = match crate::backend::init(false, false) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cockpit-tui: info: {e}");
            return 1;
        }
    };
    let code = loop {
        let _ = term.draw(|f| render(f, f.area(), &th, &s));
        match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => {
                let sz = term.size().unwrap_or_default();
                if let Out::Close = key(&mut s, &th, k.code, Rect::new(0, 0, sz.width, sz.height)) {
                    break 0;
                }
            }
            Ok(_) => {}
            Err(_) => break 1,
        }
    };
    drop(term);
    ratatui::restore();
    code
}
