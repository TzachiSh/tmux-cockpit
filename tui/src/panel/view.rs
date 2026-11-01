//! The panel's screens and its event loop. Every body is wrapped by hand: a
//! ratatui `Wrap` panics on double-width graphemes (#2679).
use super::app::{App, InputKind, Level, MODELS, QUICK, View, needs_y, risky};
use super::envelope::Flow;
use super::request::Call;
use crate::theme::Theme;
use crossterm::event::{self, Event};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    /// The selected entry's lines, kept on screen over `scroll`.
    sel: Option<(usize, usize)>,
    /// Footer hints, `(key, verb)`, the most wanted first.
    footer: Vec<(String, String)>,
    /// Shows "N-M of T" when the lines do not fit.
    count: bool,
    /// A cut-off body ends in "… N more lines".
    more: bool,
}

/// Under 28 columns the armed Answer's esc hint or the title's pane id is cut;
/// under 7 rows Explain's heading leaves no body row (`every_view_fits_at_the_floor`).
pub const MIN_W: u16 = 28;
pub const MIN_H: u16 = 7;

pub fn too_small(area: Rect) -> bool {
    area.width < MIN_W || area.height < MIN_H
}

fn plain(s: impl Into<String>) -> Line<'static> {
    Line::from(s.into())
}

fn styled(s: impl Into<String>, st: Style) -> Line<'static> {
    Line::from(Span::styled(s.into(), st))
}

/// Tabs become 4 spaces first: a tab's width is the terminal's choice, and the
/// widget would count it as one column.
fn many(text: &str, w: usize, indent: &str, st: Style) -> Vec<Line<'static>> {
    wrap(&text.replace('\t', "    "), w.saturating_sub(indent.chars().count()))
        .into_iter()
        .map(|l| styled(format!("{indent}{l}"), st))
        .collect()
}

fn pills(xs: &[(&str, &str)]) -> Vec<(String, String)> {
    xs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// A picker entry: the cursor, the number, the text wrapped under itself, and
/// a description only where it fits whole or ends in an ellipsis.
fn tile(th: &Theme, sel: bool, n: usize, mark: &str, text: &str, extra: &str, w: usize) -> Vec<Line<'static>> {
    let cur = if sel { th.g.active } else { " " };
    let st = if sel {
        Style::new().fg(th.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(th.value)
    };
    let lead = format!("{cur}{n} {mark}");
    let pad = " ".repeat(lead.width());
    // the description is a gloss on the name: under 50 columns it would be cut to nothing useful
    let show = !extra.is_empty() && w >= 50;
    let text = if show { format!("{text:<16}") } else { text.to_string() };
    let mut out: Vec<Line<'static>> = wrap(&text, w.saturating_sub(lead.width()))
        .into_iter()
        .enumerate()
        .map(|(i, l)| styled(format!("{}{l}", if i == 0 { &lead } else { &pad }), st))
        .collect();
    let room = w.saturating_sub(out[0].width() + 2);
    if show && room >= 10 {
        out[0]
            .spans
            .push(Span::styled(format!("  {}", th.g.trunc(extra, room)), Style::new().fg(th.meta)));
    }
    out
}

/// Appends a tile, returning its line range for [`Body::sel`].
fn push_tile(b: &mut Body, rows: Vec<Line<'static>>) -> (usize, usize) {
    let at = b.lines.len();
    b.lines.extend(rows);
    (at, b.lines.len())
}

fn body(app: &App, th: &Theme, w: usize, tick: u64) -> Body {
    let star = if ascii(th) { "*" } else { "★" };
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
        sel: None,
        footer: vec![],
        count: false,
        more: false,
    };
    if let Some(n) = &app.note {
        let (g, fg) = match n.level {
            Level::Info => (th.g.idle, th.sky),
            Level::Warn => (th.g.needs, th.needs),
            Level::Error => (th.g.error, th.error),
        };
        b.head.extend(many(&format!("{g} {}", n.text), w, "", Style::new().fg(fg)));
    }
    let run = app.run.as_ref();
    let back = app.leave_to().to_string();
    let help = ("?", "keys");
    match &app.view {
        View::Home { sel } => {
            for (i, (_, name, desc)) in app.tiles().iter().enumerate() {
                let mark = if i == 0 { star } else { " " };
                let r = push_tile(&mut b, tile(th, i == *sel, i + 1, &format!("{mark} "), name, desc, w));
                if i == *sel {
                    b.sel = Some(r);
                }
            }
            b.lines.push(plain(""));
            let cap = format!("{} {d} {} {d} {}", app.cap.name, app.cap.cmd, app.cap.path);
            b.lines.push(styled(th.g.trunc(&cap, w), meta));
            b.lines.extend(many(
                &format!("will send: screen {} lines, {} hidden", app.screen_lines(), app.screen_hidden),
                w,
                "",
                meta,
            ));
            let n = format!("1-{}", app.tiles().len());
            b.footer = pills(&[(&n, "pick"), help, ("esc", "close")]);
        }
        View::Compose => {
            let r = app.write_route();
            b.head
                .push(styled(format!("type: {}   model: {}", app.out.as_str(), MODELS[app.model]), label));
            b.lines = many(&format!("{}▏", app.draft), w, "", value);
            // the cursor's line, over the route lines under it
            b.sel = Some((b.lines.len() - 1, b.lines.len()));
            b.lines.push(plain(""));
            b.lines.extend(many(&format!("→ {} {d} {}", r.model, r.why), w, "", meta));
            if let Some((m, why)) = &r.suggest {
                b.lines.extend(many(
                    &format!("{star} suggest stronger: {m} {d} {why}"),
                    w,
                    "",
                    Style::new().fg(th.accent),
                ));
            }
            b.scroll = usize::MAX;
            b.footer = pills(&[
                ("^S", "write"),
                ("^F", "fix spelling"),
                ("^T", "insert as typed"),
                ("⇥", "type"),
                ("^N", "model"),
                ("F1", "keys"),
                ("esc", "home"),
            ]);
        }
        View::Busy => {
            let frames: &[&str] = if ascii(th) { &SPIN_ASCII } else { &SPIN };
            let (model, why) = run.map_or(("", ""), |r| (r.model.as_str(), r.why.as_str()));
            let (what, flow) = match run.map(|r| &r.flow) {
                Some(Flow::Write { out, .. }) => ("writing", format!("write ({})", out.as_str())),
                Some(Flow::Fix { .. }) => ("fixing the spelling", "fix spelling".into()),
                _ => ("reading the screen", "explain".into()),
            };
            let spin = frames[(tick as usize) % frames.len()];
            b.lines.extend(many(
                &format!("{spin} {what} {d} {model} {d} {}s", tick / 10),
                w,
                "",
                Style::new().fg(th.running),
            ));
            b.lines.push(plain(""));
            // the envelope's count also covers the conversation turns: they are not screen
            let talk = match app.hidden.saturating_sub(app.screen_hidden) {
                0 => String::new(),
                n => format!(", {n} more in the conversation"),
            };
            let sent = format!(
                "sent: screen {} lines, {} hidden{talk} {d} {flow}",
                app.screen_lines(),
                app.screen_hidden
            );
            b.lines.extend(many(&sent, w, "", meta));
            b.lines.extend(many(&format!("model {model}: {why}"), w, "", meta));
            if !app.timeout.is_zero() {
                b.lines.push(styled(format!("times out at {} s", app.timeout.as_secs()), meta));
            }
            // tick is tenths of a second, so 300 is 30 s
            if tick >= 300 {
                b.lines
                    .push(styled(format!("still working{}", th.g.ellipsis), Style::new().fg(th.needs)));
            }
            b.footer = pills(&[help, ("esc", "cancel")]);
        }
        View::Question { sel } => {
            let Some(r) = run else { return b };
            // in the body, not the head: a long question must not push the options off
            b.lines.extend(many(&r.question, w, "", bold));
            b.lines.push(plain(""));
            for (i, o) in r.options.iter().enumerate() {
                let mark = if o.recommended { star } else { " " };
                let t = push_tile(&mut b, tile(th, i == *sel, i + 1, &format!("{mark} "), &o.label, "", w));
                if i == *sel {
                    b.sel = Some(t);
                }
            }
            if r.options.is_empty() {
                b.lines.extend(many("no options: o types your answer", w, "", meta));
            }
            b.lines.push(plain(""));
            b.lines
                .extend(many(&format!("question {} of at most {}", r.rounds, r.max), w, "", meta));
            let n = match r.options.len() {
                0 | 1 => "1".to_string(),
                n => format!("1-{}", n.min(4)),
            };
            b.footer = pills(&[
                (&n, "pick"),
                ("o", "other"),
                ("g", "best guess"),
                ("⌫", "back"),
                help,
                ("esc", &back),
            ]);
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
            b.footer = pills(&[("↵", "send"), ("F1", "keys"), ("esc", "back")]);
        }
        View::Result { label: l, text, scroll } => {
            b.head.push(styled(th.g.trunc(l, w), heading));
            if let Some(x) = app.line_warning(text) {
                b.head.extend(many(&x, w, "", Style::new().fg(th.error)));
            }
            b.head.push(plain(""));
            b.lines = many(text, w, "", value);
            b.scroll = *scroll;
            b.count = true;
            b.footer = pills(&[
                ("i", "insert"),
                ("s", "send"),
                ("r", "retry"),
                ("c", "edit"),
                ("↑↓", "scroll"),
                help,
                ("esc", &back),
            ]);
        }
        View::Steps {
            label: l,
            items,
            sel,
            done,
        } => {
            b.head.push(styled(th.g.trunc(l, w), heading));
            b.head.push(plain(""));
            for (i, s) in items.iter().enumerate() {
                let at = b.lines.len();
                let note = if s.note.is_empty() { "(run this)" } else { s.note.as_str() };
                let mut rows = tile(th, i == *sel, i + 1, "", note, "", w);
                if done.get(i) == Some(&true) {
                    let mark = format!("  {} sent", th.g.done);
                    let last = rows.last_mut().expect("wrap returns a line");
                    if last.width() + mark.width() > w {
                        rows.push(plain("    "));
                    }
                    let last = rows.last_mut().expect("pushed above");
                    last.spans.push(Span::styled(mark, Style::new().fg(th.done)));
                }
                b.lines.extend(rows);
                if !s.cmd.is_empty() {
                    for c in s.cmd.split('\n') {
                        b.lines.extend(many(c, w, "     $ ", value));
                    }
                }
                if let Some(x) = app.line_warning(&s.cmd) {
                    b.lines.extend(many(&x, w, "     ", Style::new().fg(th.error)));
                }
                if let Some(r) = risky(&s.cmd) {
                    let mark = if ascii(th) { "!" } else { "⚠" };
                    b.lines
                        .extend(many(&format!("{mark} risky: {r}"), w, "     ", Style::new().fg(th.error)));
                }
                if i == *sel {
                    b.scroll = at.saturating_sub(2);
                    b.sel = Some((at, b.lines.len()));
                }
            }
            b.count = true;
            b.footer = pills(&[("i", "insert"), ("s", "send"), ("↑↓", "pick"), ("r", "retry"), help, ("esc", &back)]);
        }
        View::ExplainAsk { sel } => {
            let what = match app.session {
                Some(s) => format!("Claude Code here {}", s.text()),
                None => format!("{} {d} {}", app.cap.name, app.cap.cmd),
            };
            b.head.push(styled("what do you want to know?", bold));
            b.head.extend(many(&what, w, "", meta));
            b.head.push(plain(""));
            let words = "In your own words";
            for (i, shown) in QUICK.iter().map(|q| q.0).chain([words]).enumerate() {
                let t = push_tile(&mut b, tile(th, i == *sel, i + 1, "", shown, "", w));
                if i == *sel {
                    b.sel = Some(t);
                }
            }
            let n = format!("1-{}", QUICK.len() + 1);
            b.footer = pills(&[(&n, "pick"), ("↵", "ask"), help, ("esc", "home")]);
        }
        View::Answer { scroll, armed } => {
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
                b.footer = pills(&[
                    ("i", "insert"),
                    ("s", "send"),
                    ("f", "follow up"),
                    ("↑↓", "scroll"),
                    help,
                    ("esc", "home"),
                ]);
            } else {
                b.footer = pills(&[("f", "follow up"), ("↑↓", "scroll"), help, ("esc", "home")]);
            }
            if *armed {
                b.footer = pills(&[("esc q", "again discards it"), ("any other key", "keeps it")]);
            }
            b.scroll = *scroll;
            b.count = true;
        }
        View::Confirm {
            text,
            send,
            warn: ws,
            scroll,
            ..
        } => {
            for x in ws {
                b.head.extend(many(x, w, "", Style::new().fg(th.error)));
            }
            let ask = if *send {
                format!("send this to {} ({}) and press Enter?", app.pane, app.cap.cmd)
            } else {
                format!("insert this into {} ({}), no Enter?", app.pane, app.cap.cmd)
            };
            b.head.extend(many(&ask, w, "", bold));
            b.head.push(plain(""));
            b.lines = many(text, w, "  ", value);
            b.scroll = *scroll;
            b.count = true;
            b.more = true;
            b.footer = if needs_y(text, *send) {
                pills(&[
                    ("y", "confirm"),
                    ("↑↓", "scroll"),
                    ("↵", "is off: risky or multi-line"),
                    help,
                    ("esc n", "cancel"),
                ])
            } else {
                pills(&[("↵ y", "confirm"), ("↑↓", "scroll"), help, ("esc n", "cancel")])
            };
        }
        View::Error { line, hint } => {
            b.lines = many(line, w, "", Style::new().fg(th.error));
            if let Some(h) = hint {
                b.lines.push(plain(""));
                b.lines.extend(many(h, w, "", value));
            }
            b.footer = pills(&[("r", "retry"), help, ("esc", "back")]);
        }
    }
    b
}

/// The footer hints that fit `w`, whole: half a hint is not a smaller button,
/// it is an unreadable one. The lowest-priority hint goes first; `esc` and the
/// help key go last, so the way out and the way to every key always show.
fn footer(th: &Theme, mut ps: Vec<(String, String)>, w: usize) -> Line<'static> {
    let keep = |k: &str| k.starts_with("esc") || k == "?" || k == "F1";
    let width = |ps: &[(String, String)]| ps.iter().map(|(k, v)| k.width() + v.width() + 3).sum::<usize>();
    while width(&ps) > w {
        let Some(i) = ps
            .iter()
            .rposition(|(k, _)| !keep(k))
            .or_else(|| ps.iter().position(|(k, _)| !k.starts_with("esc")))
        else {
            break;
        };
        ps.remove(i);
    }
    let mut spans = vec![Span::raw(" ")];
    for (i, (k, v)) in ps.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(k, Style::new().fg(th.accent)));
        spans.push(Span::styled(format!(" {v} "), Style::new().fg(th.meta)));
    }
    Line::from(spans)
}

/// The left title and the "N-M of T" indicator in `w` columns: the title gives
/// way first, then the arrows; the count is never cut, only dropped whole.
fn titles(th: &Theme, title: &str, ind: Option<(String, String)>, w: usize) -> (String, Option<String>) {
    // " panel · %3 ": cut shorter, the title no longer says which pane it acts on
    const TITLE_MIN: usize = 12;
    let fit = |room: usize| {
        if room < 3 {
            String::new()
        } else {
            format!(" {} ", th.g.trunc(title, room - 2))
        }
    };
    let Some((full, bare)) = ind.filter(|_| w >= 18) else {
        return (fit(w), None);
    };
    for i in [&full, &bare] {
        let room = w.saturating_sub(i.width() + 1);
        if room >= TITLE_MIN {
            return (fit(room), Some(i.clone()));
        }
    }
    if bare.width() <= w {
        return (fit(w.saturating_sub(bare.width() + 1)), Some(bare));
    }
    (fit(w), None)
}

/// Drawn in place of every view under [`MIN_W`] x [`MIN_H`].
fn small(f: &mut Frame, area: Rect, th: &Theme) {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_set(th.g.round)
        .border_style(Style::new().fg(th.label))
        .title(Line::from(Span::styled(" panel ", Style::new().fg(th.accent))));
    let inner = block.inner(area);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    f.render_widget(block, area);
    let w = inner.width as usize;
    let mut lines = many("make the window larger or rotate", w, "", Style::new().fg(th.needs));
    lines.extend(many("esc closes", w, "", Style::new().fg(th.meta)));
    f.render_widget(Paragraph::new(lines), inner);
}

/// What esc on `v` goes back to, in the words the `?` overlay uses.
fn named(v: Option<&View>) -> &'static str {
    match v {
        None | Some(View::Home { .. }) => "home",
        Some(View::Compose) => "the draft",
        Some(View::Question { .. }) => "the question",
        Some(View::Result { .. }) => "the result",
        Some(View::Steps { .. }) => "the steps",
        Some(View::ExplainAsk { .. }) => "the questions",
        Some(View::Answer { .. }) => "the answer",
        _ => "the last screen",
    }
}

/// Every key of the view on screen, `(keys, what)`, for the `?` overlay. Each
/// esc row names where it goes, so the way back to home reads off the chain.
pub fn keys(app: &App) -> Vec<(&'static str, String)> {
    let s = |k: &'static str, v: &str| (k, v.to_string());
    let left = match app.leave_to() {
        "home" => "back to home".to_string(),
        x => format!("back to the {x}"),
    };
    let move_ = s("↑↓ j k", "move");
    let quit = s("q ^C", "close the panel");
    let help = s("? F1", "this help");
    let scroll = s("PgUp PgDn", "scroll a page");
    match &app.view {
        View::Home { .. } => vec![
            move_,
            s("↵", "open the selected tile"),
            s("1-9", "open a tile by number"),
            help,
            s("esc q ^C", "close the panel"),
        ],
        View::Compose => vec![
            s("^S F2", "write it with Claude"),
            s("^F", "fix the spelling"),
            s("^T", "insert as typed"),
            s("⇥", "next output type"),
            s("^N", "next model"),
            s("↵", "new line"),
            s("^W ^U", "delete a word, the box"),
            s("F1", "this help"),
            s("esc", "back to home"),
            s("^C", "close the panel"),
        ],
        View::Busy => vec![
            help,
            s("esc", &format!("cancel, back to {}", named(app.busy_back.as_ref()))),
            s("q ^C", "cancel and close the panel"),
        ],
        View::Question { .. } => vec![
            move_,
            s("↵", "answer the selected option"),
            s("1-4", "answer by number"),
            s("o", "type your own answer"),
            s("g", "let Claude guess"),
            s("⌫ ←", "the previous question"),
            help,
            s("esc", &left),
            quit,
        ],
        View::Input { back, .. } => vec![
            s("↵", "send"),
            s("^W ^U", "delete a word, the box"),
            s("F1", "this help"),
            s("esc", &format!("back to {}", named(Some(back)))),
            s("^C", "close the panel"),
        ],
        View::Result { .. } => vec![
            s("i", "insert into the pane, no Enter"),
            s("s", "send with Enter, asks first"),
            s("r", "ask again"),
            s("c", "edit it as a new draft"),
            s("↑↓ j k", "scroll"),
            scroll,
            help,
            s("esc", &left),
            quit,
        ],
        View::Steps { .. } => vec![
            s("↑↓ j k", "pick a step"),
            s("1-9", "pick a step by number"),
            s("i", "insert the step, no Enter"),
            s("s", "send the step with Enter"),
            s("r", "ask again"),
            help,
            s("esc", &left),
            quit,
        ],
        View::ExplainAsk { .. } => vec![
            move_,
            s("↵", "ask the selected question"),
            s("1-5", "ask by number, 5 in your words"),
            help,
            s("esc", "back to home"),
            quit,
        ],
        View::Answer { .. } => vec![
            s("i", "insert the prompt, no Enter"),
            s("s", "send the prompt with Enter"),
            s("f", "ask a follow-up"),
            s("↑↓ j k", "scroll"),
            scroll,
            help,
            s("esc q", "twice: drop it, back to home"),
            s("^C", "close the panel"),
        ],
        View::Confirm { back, .. } => vec![
            s("y", "confirm"),
            s("↵", "confirm, unless risky or multi-line"),
            s("↑↓ j k", "scroll"),
            scroll,
            help,
            s("esc n q", &format!("cancel, back to {}", named(Some(back)))),
            s("^C", "close the panel"),
        ],
        View::Error { .. } => vec![
            s("r", "try again"),
            help,
            s("esc", &format!("back to {}", named(app.busy_back.as_ref()))),
            quit,
        ],
    }
}

/// The `?` overlay over the whole panel: the popup is small enough that a
/// card inside it would clip the very rows it exists to show.
fn overlay(f: &mut Frame, area: Rect, app: &App, th: &Theme, top: usize) {
    let ks = keys(app);
    let rows = (area.height as usize).saturating_sub(2);
    let top = top.min(ks.len().saturating_sub(rows));
    let mut block = Block::new()
        .borders(Borders::ALL)
        .border_set(th.g.round)
        .border_style(Style::new().fg(th.label))
        .title(Line::from(Span::styled(" keys ", Style::new().fg(th.accent))));
    let last = (top + rows).min(ks.len());
    let close = if top > 0 || last < ks.len() {
        let n = format!(" {}-{last} of {} ", top + 1, ks.len());
        block = block.title(Line::from(Span::styled(n, Style::new().fg(th.meta))).right_aligned());
        " ↑↓ scroll  any other key closes "
    } else {
        " any key closes "
    };
    block = block.title_bottom(Line::from(Span::styled(close, Style::new().fg(th.meta))));
    let inner = block.inner(area);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    let kw = ks.iter().map(|(k, _)| k.width()).max().unwrap_or(0);
    let room = (inner.width as usize).saturating_sub(kw + 2);
    let lines: Vec<Line> = ks
        .into_iter()
        .skip(top)
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!("{k}{}  ", " ".repeat(kw - k.width())), Style::new().fg(th.accent)),
                Span::styled(th.g.trunc(&v, room), Style::new().fg(th.meta)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// `tick` is tenths of a second since the call went out, so a golden never reads
/// the clock. Returns the body's line count and rows, for [`App::fit`].
pub fn render(f: &mut Frame, area: Rect, app: &App, th: &Theme, tick: u64) -> (usize, usize) {
    if too_small(area) {
        small(f, area, th);
        return (0, 0);
    }
    let d = th.g.dot;
    let w = area.width.saturating_sub(4) as usize;
    let mut b = body(app, th, w, tick);
    let rows = (area.height as usize).saturating_sub(2);
    // short of a body row: the head's spacer goes, then the head scrolls with the body
    if b.head.len() >= rows && b.head.last().is_some_and(|l| l.width() == 0) {
        b.head.pop();
    }
    if b.head.len() >= rows {
        let n = b.head.len();
        b.lines.splice(0..0, std::mem::take(&mut b.head));
        b.sel = b.sel.map(|(s, e)| (s + n, e + n));
    }
    let room = rows.saturating_sub(b.head.len());
    let total = b.lines.len();
    let mut top = b.scroll.min(total.saturating_sub(room));
    if let Some((s, e)) = b.sel {
        top = top.min(s).max(e.saturating_sub(room).min(s));
    }
    let cut = b.more && room > 1 && top + room < total;
    let last = top + if cut { room - 1 } else { room.min(total - top) };
    // the board's "N-M of T" (board.rs header), arrows only where it can go
    let ind = (b.count && room > 0 && (top > 0 || last < total)).then(|| {
        let bare = format!(" {}-{last} of {total} ", top + 1);
        let mut full = bare.clone();
        if top > 0 {
            full.push_str(&format!("{}{top} ", th.g.up));
        }
        if last < total {
            full.push_str(&format!("{}{} ", th.g.down, total - last));
        }
        (full, bare)
    });
    let name = format!("panel {d} {} {d} {}", app.pane, th.g.trunc(&app.cap.name, 30));
    let (title, ind) = titles(th, &name, ind, (area.width as usize).saturating_sub(2));
    let mut block = Block::new()
        .borders(Borders::ALL)
        .border_set(th.g.round)
        .border_style(Style::new().fg(th.label))
        .title(Line::from(Span::styled(title, Style::new().fg(th.accent))))
        .title_bottom(footer(th, b.footer, (area.width as usize).saturating_sub(2)));
    if let Some(n) = ind {
        block = block.title(Line::from(Span::styled(n, Style::new().fg(th.meta))).right_aligned());
    }
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    let mut lines = b.head;
    lines.extend(b.lines.into_iter().skip(top).take(last - top));
    if cut {
        let n = total - last;
        let s = if n == 1 { "" } else { "s" };
        lines.push(styled(format!("{} {n} more line{s}", th.g.ellipsis), Style::new().fg(th.meta)));
    }
    f.render_widget(Paragraph::new(lines), inner);
    if let Some(top) = app.help {
        overlay(f, area, app, th, top);
    }
    (total, room)
}

static STOP: AtomicBool = AtomicBool::new(false);

/// Only a flag and a raise, both async-signal-safe: the loop does the cancel
/// and the restore, on its own thread.
extern "C" fn on_stop(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
    // crossterm turns SIGWINCH into an event, which wakes a blocked read.
    unsafe { libc::raise(libc::SIGWINCH) };
}

fn transient(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock)
}

/// The panel's event loop; what to paste or send is left in `app.action`.
pub fn run(app: &mut App) {
    if app.reopened {
        // before the probe: its first poll already reads the tty into crossterm's queue
        crate::backend::flush_input();
    }
    // crossterm caches a failed event-reader init forever: probe before init.
    if let Err(e) = event::poll(Duration::ZERO) {
        eprintln!("cockpit-tui: no usable terminal: {e}");
        std::process::exit(1);
    }
    // a dead tty writes no action; the claude child goes with us by PDEATHSIG
    crate::backend::exit_on_hangup(|| {});
    let th = Theme::load();
    // SAFETY: on_stop only stores an atomic and raises a signal.
    unsafe {
        libc::signal(libc::SIGHUP, on_stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_stop as *const () as libc::sighandler_t);
    }
    let mut term = match crate::backend::init(true) {
        Ok(t) => t,
        Err(e) => {
            // init may fail after raw mode and the paste mode are already on
            crate::backend::restore(true);
            eprintln!("cockpit-tui: {e}");
            std::process::exit(1);
        }
    };
    let call = Call::from_env();
    loop {
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        app.launch(&call);
        app.poll();
        if app.quit {
            break;
        }
        let tick = app.started.map_or(0, |t| (t.elapsed().as_millis() / 100) as u64);
        let (mut page, mut tiny) = ((0, 0), false);
        let drawn = term.draw(|f| {
            tiny = too_small(f.area());
            page = render(f, f.area(), app, &th, tick);
        });
        if drawn.is_err() {
            break;
        }
        app.tiny = tiny;
        // the floor message shows no body: fitting to it would lose the scroll
        if !tiny {
            app.fit(page.0, page.1);
        }
        if std::mem::take(&mut app.reopened) {
            // a key typed before this first frame was shown is dropped, crossterm's queue too
            crate::backend::flush_input();
            while matches!(event::poll(Duration::ZERO), Ok(true)) {
                let _ = event::read();
            }
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
            Ok(Some(Event::Paste(s))) => app.paste(&s),
            Ok(_) => {}
            Err(e) if transient(&e) => {}
            Err(_) => break,
        }
    }
    if !app.quit || STOP.load(Ordering::SeqCst) {
        app.stop();
    }
    // the SIGHUP/SIGTERM path breaks out of the loop above and lands here too
    crate::backend::restore(true);
}
