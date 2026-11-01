//! The panel's screens and its event loop. Every body is wrapped by hand: a
//! ratatui `Wrap` panics on double-width graphemes (#2679).
use super::app::{App, InputKind, MODELS, QUICK, View, risky};
use super::envelope::Flow;
use super::request::Call;
use crate::theme::Theme;
use crossterm::event::{self, Event};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::time::Duration;
use unicode_width::UnicodeWidthChar;

const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPIN_ASCII: [&str; 4] = ["|", "/", "-", "\\"];

fn ascii(th: &Theme) -> bool {
    th.g.ellipsis == "..."
}

/// Word wrap to `w` columns, keeping line breaks and splitting a word longer
/// than the line.
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let w = w.max(4);
    let width = |s: &str| s.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>();
    let mut out = Vec::new();
    for para in text.split('\n') {
        // Indentation is meaning in code and YAML: the first line keeps it.
        let body = para.trim_start_matches(' ');
        let mut line = " ".repeat((para.len() - body.len()).min(w / 2));
        let mut fresh = true;
        for word in body.split(' ') {
            let cand = if fresh { format!("{line}{word}") } else { format!("{line} {word}") };
            if width(&cand) <= w {
                line = cand;
                fresh = false;
                continue;
            }
            if !fresh {
                out.push(std::mem::take(&mut line));
            }
            fresh = false;
            let mut rest = word.to_string();
            while width(&rest) > w {
                let mut used = 0;
                let cut = rest.char_indices().find(|(_, c)| {
                    used += c.width().unwrap_or(0);
                    used > w
                });
                let at = cut.map_or(rest.len(), |(i, _)| i);
                out.push(rest[..at].to_string());
                rest = rest[at..].to_string();
            }
            line = rest;
        }
        out.push(line);
    }
    out
}

struct Body {
    head: Vec<Line<'static>>,
    lines: Vec<Line<'static>>,
    /// `usize::MAX` pins the bottom.
    scroll: usize,
    footer: String,
}

fn plain(s: impl Into<String>) -> Line<'static> {
    Line::from(s.into())
}

fn styled(s: impl Into<String>, st: Style) -> Line<'static> {
    Line::from(Span::styled(s.into(), st))
}

fn many(text: &str, w: usize, indent: &str, st: Style) -> Vec<Line<'static>> {
    wrap(text, w.saturating_sub(indent.chars().count()))
        .into_iter()
        .map(|l| styled(format!("{indent}{l}"), st))
        .collect()
}

/// A picker row: the cursor, the number, then the text.
fn tile(th: &Theme, sel: bool, n: usize, text: String, extra: &str) -> Line<'static> {
    let cur = if sel { th.g.active } else { " " };
    let st = if sel {
        Style::new().fg(th.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(th.value)
    };
    let mut spans = vec![Span::styled(format!("{cur}{n} {text}"), st)];
    if !extra.is_empty() {
        spans.push(Span::styled(format!("  {extra}"), Style::new().fg(th.meta)));
    }
    Line::from(spans)
}

fn body(app: &App, th: &Theme, w: usize, tick: u64) -> Body {
    let star = if ascii(th) { "*" } else { "★" };
    let warn = Style::new().fg(th.needs);
    let meta = Style::new().fg(th.meta);
    let label = Style::new().fg(th.label);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let value = Style::new().fg(th.value);
    let heading = Style::new().fg(th.accent).add_modifier(Modifier::BOLD);
    let d = th.g.dot;
    let mut b = Body {
        head: vec![],
        lines: vec![],
        scroll: 0,
        footer: String::new(),
    };
    if let Some(n) = &app.note {
        b.head.push(styled(n.clone(), warn));
    }
    let run = app.run.as_ref();
    match &app.view {
        View::Home { sel } => {
            for (i, (_, name, desc)) in app.tiles().iter().enumerate() {
                let mark = if i == 0 { star } else { " " };
                b.lines.push(tile(th, i == *sel, i + 1, format!("{mark} {name:<16}"), desc));
            }
            b.lines.push(plain(""));
            b.lines
                .push(styled(format!("{} {d} {} {d} {}", app.cap.name, app.cap.cmd, app.cap.path), meta));
            b.footer = format!("will send: screen {} lines {d} 1-4 pick {d} q close", app.screen_lines());
        }
        View::Compose => {
            let r = app.write_route();
            b.head
                .push(styled(format!("type: {}   model: {}", app.out.as_str(), MODELS[app.model]), label));
            b.lines = many(&format!("{}▏", app.draft), w, "", value);
            b.lines.push(plain(""));
            b.lines.push(styled(format!("→ {} {d} {}", r.model, r.why), meta));
            if let Some((m, why)) = &r.suggest {
                b.lines.push(styled(
                    format!("{star} suggest stronger: {m} {d} {why}"),
                    Style::new().fg(th.accent),
                ));
            }
            b.scroll = usize::MAX;
            b.footer = "^S write  ^F fix spelling  ^T insert as typed  ⇥ type  ^N model  esc back".into();
        }
        View::Busy => {
            let frames: &[&str] = if ascii(th) { &SPIN_ASCII } else { &SPIN };
            let model = run.map_or("", |r| r.route.model.as_str());
            let what = match run.map(|r| &r.flow) {
                Some(Flow::Write { .. }) => "writing",
                Some(Flow::Fix { .. }) => "fixing the spelling",
                _ => "reading the screen",
            };
            let spin = frames[(tick as usize) % frames.len()];
            b.lines.push(styled(
                format!("{spin} {what} {d} {model} {d} {}s", tick / 10),
                Style::new().fg(th.running),
            ));
            b.footer = "esc cancel".into();
        }
        View::Question { sel } => {
            let Some(r) = run else { return b };
            b.head.extend(many(&r.question, w, "", bold));
            b.head.push(plain(""));
            for (i, o) in r.options.iter().enumerate() {
                let mark = if o.recommended { star } else { " " };
                b.lines.push(tile(th, i == *sel, i + 1, format!("{mark} {}", o.label), ""));
            }
            if r.options.is_empty() {
                b.lines.push(styled("no options: o types your answer", meta));
            }
            b.lines.push(plain(""));
            b.lines.push(styled(format!("question {} of at most {}", r.rounds, r.max), meta));
            b.footer = "1-4 pick  o other  g best guess  ⌫ back  esc leave".into();
        }
        View::Input { kind, text, .. } => {
            let title = match kind {
                InputKind::Other => format!("your answer: {}", run.map_or("", |r| r.question.as_str())),
                InputKind::Explain => "what do you want to know about this screen?".into(),
                InputKind::FollowUp => "your follow-up question".into(),
            };
            b.head.extend(many(&title, w, "", label));
            b.head.push(plain(""));
            b.lines = many(&format!("{text}▏"), w, "", value);
            b.scroll = usize::MAX;
            b.footer = "↵ send  esc back".into();
        }
        View::Result { label: l, text, scroll } => {
            b.head.push(styled(l.clone(), heading));
            if let Some(x) = app.line_warning(text) {
                b.head.push(styled(x, Style::new().fg(th.error)));
            }
            b.head.push(plain(""));
            b.lines = many(text, w, "", value);
            b.scroll = *scroll;
            b.footer = "i insert  s send  r retry  c edit  ↑↓ scroll  esc back".into();
        }
        View::Steps { label: l, items, sel } => {
            b.head.push(styled(l.clone(), heading));
            b.head.push(plain(""));
            let mut at = 0;
            for (i, s) in items.iter().enumerate() {
                if i == *sel {
                    at = b.lines.len();
                }
                let note = if s.note.is_empty() {
                    "(run this)".to_string()
                } else {
                    s.note.clone()
                };
                b.lines.push(tile(th, i == *sel, i + 1, note, ""));
                if !s.cmd.is_empty() {
                    for c in s.cmd.split('\n') {
                        b.lines.extend(many(c, w, "     $ ", value));
                    }
                }
                if let Some(r) = risky(&s.cmd) {
                    let mark = if ascii(th) { "!" } else { "⚠" };
                    b.lines.push(styled(format!("     {mark} risky: {r}"), Style::new().fg(th.error)));
                }
            }
            b.scroll = at.saturating_sub(2);
            b.footer = "↑↓ pick  i insert  s run  r retry  esc back".into();
        }
        View::ExplainAsk { sel } => {
            let what = match app.session {
                Some(s) => format!("Claude Code here {}", s.text()),
                None => format!("{} {d} {}", app.cap.name, app.cap.cmd),
            };
            b.head.push(styled("what do you want to know?", bold));
            b.head.extend(many(&what, w, "", meta));
            b.head.push(plain(""));
            for (i, (shown, _)) in QUICK.iter().enumerate() {
                b.lines.push(tile(th, i == *sel, i + 1, shown.to_string(), ""));
            }
            b.lines
                .push(tile(th, *sel == QUICK.len(), QUICK.len() + 1, "In your own words".into(), ""));
            b.footer = "1-5 pick  ↵ ask  esc back".into();
        }
        View::Answer { scroll } => {
            let Some(r) = run else { return b };
            for (you, t) in &r.log {
                if *you {
                    b.lines.extend(many(&format!("you: {t}"), w, "", label));
                } else {
                    b.lines.extend(many(t, w, "", value));
                }
                b.lines.push(plain(""));
            }
            if let Some(p) = &r.ready {
                b.lines.push(styled("prompt for the agent:", heading));
                b.lines.extend(many(p, w, "  ", Style::new().fg(th.done)));
                b.footer = "f follow up  i insert  s send  ↑↓ scroll  esc home".into();
            } else {
                b.footer = "f follow up  ↑↓ scroll  esc home".into();
            }
            b.scroll = *scroll;
        }
        View::Confirm { text, send, warn: ws, .. } => {
            for x in ws {
                b.head.extend(many(x, w, "", Style::new().fg(th.error)));
            }
            let ask = if *send {
                format!("send this to {} ({}) and press Enter?", app.pane, app.cap.cmd)
            } else {
                format!("insert this into {} ({}), no Enter?", app.pane, app.cap.cmd)
            };
            b.head.push(styled(ask, bold));
            b.head.push(plain(""));
            b.lines = many(text, w, "  ", value);
            b.footer = "↵ y confirm  esc n cancel".into();
        }
        View::Error { line } => {
            b.lines.push(styled(format!("claude failed: {line}"), Style::new().fg(th.error)));
            b.footer = "r retry  esc back".into();
        }
    }
    b
}

/// `tick` is tenths of a second since the call went out; a parameter, so a
/// golden never reads the clock.
pub fn render(f: &mut Frame, area: Rect, app: &App, th: &Theme, tick: u64) {
    let d = th.g.dot;
    let w = area.width.saturating_sub(4) as usize;
    let b = body(app, th, w, tick);
    let title = format!(" panel {d} {} {d} {} ", app.pane, th.g.trunc(&app.cap.name, 30));
    let footer = format!(" {} ", th.g.trunc(&b.footer, w));
    let block = Block::new()
        .borders(Borders::ALL)
        .border_set(th.g.round)
        .border_style(Style::new().fg(th.label))
        .title(Line::from(Span::styled(title, Style::new().fg(th.accent))))
        .title_bottom(Line::from(Span::styled(footer, Style::new().fg(th.meta))));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    let room = (inner.height as usize).saturating_sub(b.head.len());
    let top = b.scroll.min(b.lines.len().saturating_sub(room));
    let mut lines = b.head;
    lines.extend(b.lines.into_iter().skip(top).take(room));
    f.render_widget(Paragraph::new(lines), inner);
}

fn transient(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock)
}

/// The panel's event loop; what to paste or send is left in `app.action`.
pub fn run(app: &mut App) {
    // crossterm caches a failed event-reader init forever: probe before init.
    if let Err(e) = event::poll(Duration::ZERO) {
        eprintln!("cockpit-tui: no usable terminal: {e}");
        std::process::exit(1);
    }
    let th = Theme::load();
    let mut term = match crate::backend::init() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cockpit-tui: {e}");
            std::process::exit(1);
        }
    };
    let call = Call::from_env();
    loop {
        app.launch(&call);
        app.poll();
        if app.quit {
            break;
        }
        let tick = app.started.map_or(0, |t| (t.elapsed().as_millis() / 100) as u64);
        if term.draw(|f| render(f, f.area(), app, &th, tick)).is_err() {
            break;
        }
        // Ticking only while a call is out, so an idle panel costs no wakeups.
        let ev = if app.req.is_some() {
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => event::read().map(Some),
                Ok(false) => Ok(None),
                Err(e) => Err(e),
            }
        } else {
            event::read().map(Some)
        };
        match ev {
            Ok(Some(Event::Key(k))) => app.key(k),
            Ok(_) => {}
            Err(e) if transient(&e) => {}
            Err(_) => break,
        }
    }
    ratatui::restore();
}
