//! The panel's screens and its event loop. Every body is wrapped by hand: a
//! ratatui `Wrap` panics on double-width graphemes (#2679).
use super::app::{App, DRAFT_COUNT, InputKind, Level, MAX_DRAFT, MODELS, QUICK, Tap, View, boundary, needs_y};
use super::envelope::Flow;
use super::highlight::{self, Role};
use super::request::Call;
use crate::theme::{Glyphs, Theme};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// app.rs words its warnings in the unicode tier: the ascii tier swaps the glyphs in.
fn tiered(g: &Glyphs, s: &str) -> String {
    if !g.ascii {
        return s.to_string();
    }
    let u = Glyphs::unicode();
    s.replace(u.warn, g.warn).replace(u.ndash, g.ndash)
}

/// Word wrap to `w` columns, splitting a word longer than the line. Widths are
/// per grapheme cluster, as the buffer draws them: ❤️ is 2 columns, not 1.
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let w = w.max(4);
    let width = |s: &str| s.graphemes(true).map(|g| g.width()).sum::<usize>();
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
                let cut = rest.grapheme_indices(true).find(|(_, g)| {
                    used += g.width();
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

/// The cursor after Up or Down in a text box `w` columns wide: the same column
/// on the row above or below, as drawn. Rows are visual: the box word-wraps
/// with the cursor glyph in it (`typed`), so this lays the text out the same way
/// and reads each grapheme's column back off the rows [`wrap`] made. A column
/// inside a wide symbol takes the symbol, a short row takes its end, and the
/// first or last row keeps the cursor where it is.
pub fn vertical(text: &str, cur: usize, w: usize, up: bool) -> usize {
    // one unit per drawn grapheme: (text, the offset a cursor there takes)
    let c = boundary(text, cur);
    let mut units: Vec<(&str, Option<usize>)> = Vec::new();
    let mut glyph = 0;
    for (o, g) in text.grapheme_indices(true).chain([(text.len(), "")]) {
        if o == c {
            glyph = units.len();
            units.push(("|", None));
        }
        match g {
            "" => {}
            // `many` makes a tab 4 spaces before wrapping
            "\t" => units.extend([(" ", Some(o)), (" ", None), (" ", None), (" ", None)]),
            g => units.push((g, Some(o))),
        }
    }
    units.push(("\n", Some(text.len())));
    // each row's stops, (offset, column), and the cursor's row and column
    let (mut rows, mut here) = (Vec::new(), (0, 0));
    let mut i = 0;
    while i < units.len() {
        let n = units[i..].iter().position(|u| u.0 == "\n").expect("ends in a newline");
        let para = &units[i..i + n];
        let lines = wrap(&para.iter().map(|u| u.0).collect::<String>(), w);
        let indent = para.iter().take_while(|u| u.0 == " ").count();
        let mut j = 0;
        for (r, line) in lines.iter().enumerate() {
            let (mut stops, mut rest, mut col) = (Vec::new(), line.as_str(), 0);
            if r == 0 {
                // wrap keeps half a row of indent at most: the rest share its last column
                let k = line.len() - line.trim_start_matches(' ').len();
                for (x, u) in para[..indent].iter().enumerate() {
                    stops.extend(u.1.map(|o| (o, x.min(k))));
                }
                (j, col, rest) = (indent, k, &line[k..]);
            }
            while let Some(u) = para.get(j) {
                let Some(more) = rest.strip_prefix(u.0) else { break };
                if i + j == glyph {
                    here = (rows.len(), col);
                }
                stops.extend(u.1.map(|o| (o, col)));
                (col, rest, j) = (col + u.0.width(), more, j + 1);
            }
            if r + 1 == lines.len() {
                stops.extend(units[i + n].1.map(|o| (o, col)));
            } else if para.get(j).is_some_and(|u| u.0 == " ") {
                // the space a word break dropped: a cursor on it ends this row
                stops.extend(para[j].1.map(|o| (o, col)));
                j += 1;
            }
            rows.push(stops);
        }
        i += n + 1;
    }
    let (row, col) = here;
    let to = if up { row.checked_sub(1) } else { Some(row + 1) };
    let Some(stops) = to.and_then(|t| rows.get(t)) else { return c };
    stops.iter().rev().find(|s| s.1 <= col).or(stops.first()).map_or(c, |s| s.0)
}

struct Body {
    head: Vec<Line<'static>>,
    /// The note's rows at the head's top: they never scroll away.
    pin: usize,
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
    /// Line ranges a tap selects or picks, in tablet mode.
    taps: Vec<((usize, usize), Tap)>,
    /// Confirm's yes and no, drawn big above the tap buttons in tablet mode.
    big: Vec<(String, Tap)>,
}

/// A rect a tap lands on, recorded by [`render`] every frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub at: Rect,
    pub tap: Tap,
}

/// Under 28 columns the armed Answer's esc hint or the title's pane id is cut;
/// under 7 rows Explain's heading leaves no body row (`every_view_fits_at_the_floor`).
pub const MIN_W: u16 = 28;
pub const MIN_H: u16 = 7;

/// The tablet's buttons take rows of their own: under 12, Result and Confirm at
/// 28 columns keep under 2 body rows beside them (`every_tablet_view_keeps_two_body_rows`).
pub const TABLET_MIN_W: u16 = 28;
pub const TABLET_MIN_H: u16 = 12;

pub fn too_small(area: Rect, tablet: bool) -> bool {
    let (w, h) = if tablet { (TABLET_MIN_W, TABLET_MIN_H) } else { (MIN_W, MIN_H) };
    area.width < w || area.height < h
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

/// [`many`] for text that may hold fenced code (#282): the rows are the same,
/// only a fenced block's tokens take a colour, so no width moves.
fn coded(th: &Theme, text: &str, w: usize, st: Style) -> Vec<Line<'static>> {
    if !text.contains("```") {
        return many(text, w, "", st);
    }
    let text = text.replace('\t', "    ");
    highlight::fenced(&text).iter().flat_map(|l| painted(th, l, w, "", st)).collect()
}

/// A lexed step command, one source line per entry, each under `indent`.
fn command(th: &Theme, cmd: &str, lang: &str, w: usize, indent: &str, st: Style) -> Vec<Line<'static>> {
    let cmd = cmd.replace('\t', "    ");
    let lines: Vec<&str> = cmd.split('\n').collect();
    highlight::highlight(lang, &lines)
        .iter()
        .flat_map(|l| painted(th, l, w, indent, st))
        .collect()
}

fn role_style(th: &Theme, r: Role, st: Style) -> Style {
    match r {
        Role::Plain => st,
        Role::Keyword => st.fg(th.accent),
        Role::Str => st.fg(th.done),
        Role::Comment | Role::Fence => st.fg(th.meta),
        Role::Number => st.fg(th.running),
    }
}

/// One source line's spans laid onto [`wrap`]'s rows of it. wrap only drops
/// spaces (at a break, and indentation past half the width), so each row's
/// chars are found in order in the source, its spaces skipped where they went.
fn painted(th: &Theme, spans: &[(Role, &str)], w: usize, indent: &str, st: Style) -> Vec<Line<'static>> {
    let src: String = spans.iter().map(|s| s.1).collect();
    let roles: Vec<Role> = spans.iter().flat_map(|(r, t)| std::iter::repeat_n(*r, t.len())).collect();
    let mut p = 0;
    let rows = wrap(&src, w.saturating_sub(indent.chars().count()));
    rows.into_iter()
        .map(|row| {
            let mut out: Vec<Span<'static>> = vec![];
            if !indent.is_empty() {
                out.push(Span::styled(indent.to_string(), st));
            }
            let mut run = (st, String::new());
            for c in row.chars() {
                while !src[p..].starts_with(c) && src[p..].starts_with(' ') {
                    p += 1;
                }
                let style = if src[p..].starts_with(c) {
                    p += c.len_utf8();
                    role_style(th, roles[p - c.len_utf8()], st)
                } else {
                    st
                };
                if style != run.0 && !run.1.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut run.1), run.0));
                }
                run.0 = style;
                run.1.push(c);
            }
            if !run.1.is_empty() || out.is_empty() {
                out.push(Span::styled(run.1, run.0));
            }
            Line::from(out)
        })
        .collect()
}

/// A text box's lines with the cursor drawn in at `cur`, and the cursor's line.
/// The text may hold the cursor glyph itself, so those are counted past.
fn typed(g: &Glyphs, text: &str, cur: usize, w: usize, st: Style, cs: Style) -> (Vec<Line<'static>>, usize) {
    let c = boundary(text, cur);
    let mut before = text[..c].matches(g.cursor).count();
    let mut lines = many(&format!("{}{}{}", &text[..c], g.cursor, &text[c..]), w, "", st);
    let at = lines.iter().position(|l| {
        let n = l.to_string().matches(g.cursor).count();
        let here = n > before;
        before -= if here { 0 } else { n };
        here
    });
    let at = at.unwrap_or(lines.len() - 1);
    // the cursor gets a span of its own: a `|` typed in the ascii tier looks just like it
    let t = lines[at].to_string();
    if let Some((i, _)) = t.match_indices(g.cursor).nth(before) {
        let j = i + g.cursor.len();
        let spans = vec![
            Span::styled(t[..i].to_string(), st),
            Span::styled(g.cursor, cs),
            Span::styled(t[j..].to_string(), st),
        ];
        lines[at] = Line::from(spans);
    }
    (lines, at)
}

/// The text box's cursor: accent and reversed, so it is never read as a typed char.
fn cursor_style(th: &Theme) -> Style {
    Style::new().fg(th.accent).add_modifier(Modifier::REVERSED)
}

/// A picker's number keys, from its real count: none for no entries, and
/// the empty hint is dropped, so no key is shown that does nothing.
fn digits(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => "1".into(),
        _ => format!("1-{}", n.min(9)),
    }
}

fn pills(xs: &[(&str, &str)]) -> Vec<(String, String)> {
    xs.iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
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

/// `rows` in a rounded box `w` wide: a tall tile, or a big button.
fn boxed(th: &Theme, sel: bool, rows: Vec<Line<'static>>, w: usize) -> Vec<Line<'static>> {
    let st = Style::new().fg(if sel { th.accent } else { th.label });
    let s = th.g.round;
    let bar = |c: &str| c.repeat(w.saturating_sub(2));
    let mut out = vec![styled(format!("{}{}{}", s.top_left, bar(s.horizontal_top), s.top_right), st)];
    for mut l in rows {
        let pad = " ".repeat(w.saturating_sub(l.width() + 4));
        l.spans.insert(0, Span::styled(format!("{} ", s.vertical_left), st));
        l.spans.push(Span::styled(format!("{pad} {}", s.vertical_right), st));
        out.push(l);
    }
    out.push(styled(
        format!("{}{}{}", s.bottom_left, bar(s.horizontal_bottom), s.bottom_right),
        st,
    ));
    out
}

/// A tappable entry: boxed when `tall`, its lines recorded for the hit map.
fn push_entry(b: &mut Body, th: &Theme, tall: bool, sel: bool, rows: Vec<Line<'static>>, w: usize, tap: Tap) -> (usize, usize) {
    let rows = if tall { boxed(th, sel, rows, w) } else { rows };
    let r = push_tile(b, rows);
    b.taps.push((r, tap));
    r
}

/// The key a footer hint names, as a tap; its last word, so `↵ y` is `y`. A
/// range or the arrows name no single key, and become no button.
fn tap_of(g: &Glyphs, k: &str) -> Option<Tap> {
    let t = k.rsplit(' ').next()?;
    let code = match t {
        "esc" => KeyCode::Esc,
        "F1" => KeyCode::F(1),
        _ if t == g.key_tab => KeyCode::Tab,
        _ if t == g.key_bksp => KeyCode::Backspace,
        _ if t == g.key_enter => KeyCode::Enter,
        _ => {
            let mut cs = t.chars();
            match (cs.next(), cs.next(), cs.next()) {
                (Some('^'), Some(c), None) => {
                    let k = KeyEvent::new(KeyCode::Char(c.to_ascii_lowercase()), KeyModifiers::CONTROL);
                    return Some(Tap::Key(k));
                }
                (Some(c), None, None) if c.is_ascii_graphic() && !c.is_ascii_digit() => KeyCode::Char(c),
                _ => return None,
            }
        }
    };
    Some(Tap::Key(KeyEvent::from(code)))
}

fn rect(x: usize, y: usize, w: usize, h: usize) -> Rect {
    Rect::new(x as u16, y as u16, w as u16, h as u16)
}

/// The big pair, `w` wide, for the top rows: the buttons that open them sit at
/// the bottom, so a double tap never lands on yes. Hits are relative to its top left.
fn bigs(th: &Theme, b: &Body, w: usize, big_tall: bool) -> (Vec<Line<'static>>, Vec<Hit>) {
    let (mut lines, mut hits) = (vec![], vec![]);
    if !b.big.is_empty() {
        let n = b.big.len();
        let each = w.saturating_sub(n - 1) / n;
        let mut rows: Vec<Line<'static>> = vec![];
        for (i, (label, tap)) in b.big.iter().enumerate() {
            // the last one takes the columns the division left over
            let bw = if i + 1 == n { w.saturating_sub(i * (each + 1)) } else { each };
            let st = Style::new().fg(th.accent).add_modifier(Modifier::BOLD);
            let one = if big_tall {
                let width = bw.saturating_sub(4);
                let text = th.g.trunc(label, width);
                boxed(th, i == 0, vec![Line::styled(format!("{text:^width$}"), st)], bw)
            } else {
                let width = bw.saturating_sub(2);
                let text = th.g.trunc(label, width);
                vec![Line::styled(format!("[{text:^width$}]"), st)]
            };
            hits.push(Hit {
                at: rect(i * (each + 1), 0, bw, one.len()),
                tap: *tap,
            });
            for (r, l) in one.into_iter().enumerate() {
                match rows.get_mut(r) {
                    Some(row) => {
                        row.spans.push(Span::raw(" "));
                        row.spans.extend(l.spans);
                    }
                    None => rows.push(l),
                }
            }
        }
        lines.extend(rows);
    }
    (lines, hits)
}

/// A `[ verb ]` chip per footer hint that names a key, wrapped to `w`, for the
/// bottom rows. Hits are relative to the rows' top left.
fn chips(th: &Theme, b: &Body, w: usize) -> (Vec<Line<'static>>, Vec<Hit>) {
    let (mut lines, mut hits) = (vec![], vec![]);
    let mut row: Vec<Span<'static>> = vec![];
    let mut x = 0;
    for (k, v) in &b.footer {
        let Some(tap) = tap_of(&th.g, k) else { continue };
        let chip = format!("[ {} ]", th.g.trunc(v, w.saturating_sub(4)));
        let cw = chip.width();
        if x > 0 && x + 1 + cw > w {
            lines.push(Line::from(std::mem::take(&mut row)));
            x = 0;
        }
        if x > 0 {
            row.push(Span::raw(" "));
            x += 1;
        }
        hits.push(Hit {
            at: rect(x, lines.len(), cw, 1),
            tap,
        });
        row.push(Span::styled(chip, Style::new().fg(th.accent)));
        x += cw;
    }
    if !row.is_empty() {
        lines.push(Line::from(row));
    }
    (lines, hits)
}

/// `tall` boxes the tappable entries, three rows each, for tablet mode.
fn body(app: &App, th: &Theme, w: usize, tick: u64, tall: bool) -> Body {
    let star = th.g.star;
    let g = &th.g;
    let meta = Style::new().fg(th.meta);
    let label = Style::new().fg(th.label);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let value = Style::new().fg(th.value);
    let heading = Style::new().fg(th.accent).add_modifier(Modifier::BOLD);
    let d = th.g.dot;
    let mut b = Body {
        head: vec![],
        pin: 0,
        lines: vec![],
        scroll: 0,
        sel: None,
        footer: vec![],
        count: false,
        more: false,
        taps: vec![],
        big: vec![],
    };
    // a tall tile's text sits inside its box
    let tw = if tall { w.saturating_sub(4) } else { w };
    if let Some(n) = &app.note {
        let (g, fg) = match n.level {
            Level::Info => (th.g.idle, th.sky),
            Level::Warn => (th.g.needs, th.needs),
            Level::Error => (th.g.error, th.error),
        };
        b.head.extend(many(&format!("{g} {}", n.text), w, "", Style::new().fg(fg)));
        b.pin = b.head.len();
    }
    let run = app.run.as_ref();
    let back = app.leave_to().to_string();
    let help = ("?", "keys");
    match &app.view {
        View::Home { sel } => {
            for (i, (_, name, desc)) in app.tiles().iter().enumerate() {
                let mark = if i == 0 { star } else { " " };
                let rows = tile(th, i == *sel, i + 1, &format!("{mark} "), name, desc, tw);
                let r = push_entry(&mut b, th, tall, i == *sel, rows, w, Tap::Pick(i));
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
            let n = digits(app.tiles().len());
            b.footer = pills(&[(&n, "pick"), help, ("esc", "close")]);
        }
        View::Compose => {
            let r = app.write_route();
            let head = format!("type: {}   model: {}", app.out.as_str(), MODELS[app.model]);
            let mut top = styled(th.g.trunc(&head, w), label);
            let n = app.draft.chars().count();
            if n > DRAFT_COUNT {
                let fg = if n >= MAX_DRAFT { th.needs } else { th.meta };
                let count = Span::styled(format!("{n}/{MAX_DRAFT}"), Style::new().fg(fg));
                if top.width() + 3 + count.width() <= w {
                    top.spans.extend([Span::raw("   "), count]);
                } else {
                    b.head.push(top);
                    top = Line::from(count);
                }
            }
            b.head.push(top);
            let at;
            if app.draft.is_empty() {
                // drawn, never typed: the draft stays empty
                let hint = th.g.trunc(&format!("type what you want{}", g.ellipsis), w.saturating_sub(1));
                let dim = meta.add_modifier(Modifier::DIM);
                (b.lines, at) = (
                    vec![Line::from(vec![Span::styled(g.cursor, cursor_style(th)), Span::styled(hint, dim)])],
                    0,
                );
            } else {
                (b.lines, at) = typed(g, &app.draft, app.cur, w, value, cursor_style(th));
            }
            // the cursor's line, over the route lines under it
            b.sel = Some((at, at + 1));
            b.lines.push(plain(""));
            b.lines.extend(many(&format!("{} {} {d} {}", g.arrow, r.model, r.why), w, "", meta));
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
                (g.key_tab, "type"),
                ("^N", "model"),
                ("F1", "keys"),
                ("esc", "home"),
            ]);
        }
        View::Busy => {
            let frames = g.spin;
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
            b.more = true;
            b.footer = pills(&[help, ("esc", "cancel")]);
        }
        View::Question { sel, scroll } => {
            let Some(r) = run else { return b };
            // in the body, not the head: a long question must not push the options off
            b.lines.extend(many(&r.question, w, "", bold));
            b.lines.push(plain(""));
            for (i, o) in r.options.iter().enumerate() {
                let mark = if o.recommended { star } else { " " };
                let rows = tile(th, i == *sel, i + 1, &format!("{mark} "), &o.label, "", tw);
                let t = push_entry(&mut b, th, tall, i == *sel, rows, w, Tap::Pick(i));
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
            if let Some(s) = scroll {
                (b.scroll, b.sel) = (*s, None);
            }
            b.count = true;
            let n = digits(r.options.len().min(4));
            b.footer = pills(&[
                (&n, "pick"),
                ("o", "other"),
                ("g", "best guess"),
                (g.key_bksp, "back"),
                help,
                ("esc", &back),
            ]);
        }
        View::Input { kind, text, cur, .. } => {
            let title = match kind {
                InputKind::Other => format!("your answer: {}", run.map_or("", |r| r.question.as_str())),
                InputKind::Explain => "what do you want to know about this screen?".into(),
                InputKind::FollowUp => "your follow-up question".into(),
            };
            b.head.extend(many(&title, w, "", label));
            b.head.push(plain(""));
            let at;
            (b.lines, at) = typed(g, text, *cur, w, value, cursor_style(th));
            (b.scroll, b.sel) = (usize::MAX, Some((at, at + 1)));
            b.footer = pills(&[(g.key_enter, "send"), ("F1", "keys"), ("esc", "back")]);
        }
        View::Result { label: l, text, scroll } => {
            b.head.push(styled(th.g.trunc(l, w), heading));
            if let Some(x) = app.line_warning(text) {
                b.head.extend(many(&tiered(g, &x), w, "", Style::new().fg(th.error)));
            }
            b.head.push(plain(""));
            b.lines = coded(th, text, w, value);
            b.scroll = *scroll;
            b.count = true;
            b.footer = pills(&[
                ("i", "insert"),
                ("s", "send"),
                ("r", "retry"),
                ("c", "edit"),
                (g.key_updown, "scroll"),
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
                    // the verb is known for the step this run resumed on; earlier ones only as done
                    let what = match app.last_step {
                        Some((at, "send")) if at == i => "sent",
                        Some((at, _)) if at == i => "inserted",
                        _ => "done",
                    };
                    let mark = format!("  {} {what}", th.g.done);
                    let last = rows.last_mut().expect("wrap returns a line");
                    if last.width() + mark.width() > w {
                        rows.push(plain("    "));
                    }
                    let last = rows.last_mut().expect("pushed above");
                    last.spans.push(Span::styled(mark, Style::new().fg(th.done)));
                }
                b.lines.extend(rows);
                if !s.cmd.is_empty() {
                    b.lines.extend(command(th, &s.cmd, &s.lang, w, "     $ ", value));
                }
                if let Some(x) = app.line_warning(&s.cmd) {
                    b.lines.extend(many(&tiered(g, &x), w, "     ", Style::new().fg(th.error)));
                }
                if i == *sel {
                    b.scroll = at.saturating_sub(2);
                    b.sel = Some((at, b.lines.len()));
                }
                b.taps.push(((at, b.lines.len()), Tap::Select(i)));
            }
            b.count = true;
            b.footer = pills(&[
                ("i", "insert"),
                ("s", "send"),
                (g.key_updown, "pick"),
                ("r", "retry"),
                help,
                ("esc", &back),
            ]);
        }
        View::ExplainAsk { sel } => {
            let what = match app.session {
                Some(s) => format!("Claude Code here {}", s.text()),
                None => format!("{} {d} {}", app.cap.name, app.cap.cmd),
            };
            b.head.push(styled(th.g.trunc("what do you want to know?", w), bold));
            b.head.extend(many(&what, w, "", meta));
            b.head.push(plain(""));
            let words = "In your own words";
            for (i, shown) in QUICK.iter().map(|q| q.0).chain([words]).enumerate() {
                let rows = tile(th, i == *sel, i + 1, "", shown, "", tw);
                let t = push_entry(&mut b, th, tall, i == *sel, rows, w, Tap::Pick(i));
                if i == *sel {
                    b.sel = Some(t);
                }
            }
            let n = digits(QUICK.len() + 1);
            b.footer = pills(&[(&n, "pick"), (g.key_enter, "ask"), help, ("esc", "home")]);
        }
        View::Answer { scroll, armed } => {
            let Some(r) = run else { return b };
            for (you, t) in &r.log {
                if *you {
                    b.lines.extend(many(&format!("you: {t}"), w, "", label));
                } else {
                    b.lines.extend(coded(th, t, w, value));
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
                    (g.key_updown, "scroll"),
                    help,
                    ("esc", "home"),
                ]);
            } else {
                b.footer = pills(&[("f", "follow up"), (g.key_updown, "scroll"), help, ("esc", "home")]);
            }
            if *armed && app.tablet {
                // the esc chip becomes keep it in place, so a second tap on home keeps;
                // n is a key Answer leaves unbound, and discard is up top, away from it
                for p in b.footer.iter_mut().filter(|p| p.0 == "esc") {
                    *p = ("n".into(), "keep it".into());
                }
                let k = |c| Tap::Key(KeyEvent::from(c));
                b.big = vec![("discard it".into(), k(KeyCode::Esc)), ("keep it".into(), k(KeyCode::Char('n')))];
            } else if *armed {
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
                b.head.extend(many(&tiered(g, x), w, "", Style::new().fg(th.error)));
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
            b.footer = if app.tablet {
                // yes is y, never Enter: a tap held over must not fire a multi-line send
                let yes = if *send { "yes, send" } else { "yes, insert" };
                let y = |c| Tap::Key(KeyEvent::from(KeyCode::Char(c)));
                b.big = vec![(yes.into(), y('y')), ("no".into(), y('n'))];
                pills(&[help])
            } else if needs_y(text, *send) {
                pills(&[
                    ("y", "confirm"),
                    (g.key_updown, "scroll"),
                    (g.key_enter, "is off: multi-line"),
                    help,
                    ("esc n", "cancel"),
                ])
            } else {
                pills(&[
                    (&format!("{} y", g.key_enter), "confirm"),
                    (g.key_updown, "scroll"),
                    help,
                    ("esc n", "cancel"),
                ])
            };
        }
        View::Error { line, hint } => {
            b.lines = many(line, w, "", Style::new().fg(th.error));
            if let Some(h) = hint {
                b.lines.push(plain(""));
                b.lines.extend(many(h, w, "", value));
            }
            b.more = true;
            b.footer = pills(&[("r", "retry"), help, ("esc", "back")]);
        }
    }
    let copies = matches!(app.view, View::Result { .. } | View::Steps { .. } | View::Answer { .. });
    if app.tablet && copies {
        b.footer.insert(2.min(b.footer.len()), ("x".into(), "copy".into()));
    }
    // before the help key, so a narrow footer drops it first; not where the view is
    // not kept as it is: a call out, a confirm, an armed answer
    let hides = !matches!(app.view, View::Busy | View::Confirm { .. } | View::Answer { armed: true, .. });
    if hides && let Some(i) = b.footer.iter().position(|p| p.0 == "?" || p.0 == "F1") {
        b.footer.insert(i, ("^Z".into(), "hide".into()));
    }
    b
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

/// Drawn in place of every view under the floor: [`MIN_W`] x [`MIN_H`], or the tablet's.
fn small(f: &mut Frame, area: Rect, th: &Theme) {
    crate::chrome::floor(f, area, th, "panel", "make the window larger or rotate", "esc goes back");
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
pub fn keys(app: &App, g: &Glyphs) -> Vec<(String, String)> {
    let s = |k: &str, v: &str| (k.to_string(), v.to_string());
    let left = match app.leave_to() {
        "home" => "back to home".to_string(),
        x => format!("back to the {x}"),
    };
    let ud = format!("{} j k", g.key_updown);
    let (ret, bksp) = (g.key_enter, g.key_bksp);
    let lr = format!("{} {}", g.key_left, g.key_right);
    let move_ = s(&ud, "move");
    let quit = s("q ^C", "close the panel");
    let help = s("? F1", "this help");
    let scroll = s("PgUp PgDn", "scroll a page");
    let all = match &app.view {
        View::Home { .. } => vec![
            move_,
            s(ret, "open the selected tile"),
            s(&digits(app.tiles().len()), "open a tile by number"),
            help,
            s("esc q ^C", "close the panel"),
        ],
        View::Compose => vec![
            s("^S F2", "write it with Claude"),
            s("^F", "fix the spelling"),
            s("^T", "insert as typed"),
            s(g.key_tab, "next output type"),
            s("^N", "next model"),
            s(ret, "new line"),
            s(&format!("{bksp} ^W ^U"), "delete a char, a word, all before"),
            s(&lr, &format!("move; ^{} ^{} by a word", g.key_left, g.key_right)),
            s(g.key_updown, "the row above, below"),
            s("Home End", "line start, end; ^A ^E too"),
            s("Del", "delete the char after"),
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
            s(ret, "answer the selected option"),
            s(&digits(app.run.as_ref().map_or(0, |r| r.options.len()).min(4)), "answer by number"),
            s("o", "type your own answer"),
            s("g", "let Claude guess"),
            s(&format!("{bksp} {}", g.key_left), "the previous question"),
            s("PgUp PgDn", "scroll the question"),
            help,
            s("esc", &left),
            quit,
        ],
        View::Input { back, .. } => vec![
            s(ret, "send"),
            s(&format!("{bksp} ^W ^U"), "delete a char, a word, all before"),
            s(&lr, &format!("move; ^{} ^{} by a word", g.key_left, g.key_right)),
            s("Home End", "line start, end; ^A ^E too"),
            s("Del", "delete the char after"),
            s("F1", "this help"),
            s("esc", &format!("back to {}", named(Some(back)))),
            s("^C", "close the panel"),
        ],
        View::Result { .. } => vec![
            s("i", "insert into the pane, no Enter"),
            s("s", "send with Enter, asks first"),
            s("r", "ask again"),
            s("c", "edit it as a new draft"),
            s("x", "copy it (OSC 52)"),
            s(&ud, "scroll"),
            scroll,
            help,
            s("esc", &left),
            quit,
        ],
        View::Steps { items, .. } => vec![
            s(&ud, "pick a step"),
            s(&digits(items.len()), "pick a step by number"),
            s("i", "insert the step, no Enter"),
            s("s", "send the step with Enter"),
            s("x", "copy the step (OSC 52)"),
            s("r", "ask again"),
            help,
            s("esc", &left),
            quit,
        ],
        View::ExplainAsk { .. } => vec![
            move_,
            s(ret, "ask the selected question"),
            s(&digits(QUICK.len() + 1), "ask by number, 5 in your words"),
            help,
            s("esc", "back to home"),
            quit,
        ],
        View::Answer { .. } => vec![
            s("i", "insert the prompt, no Enter"),
            s("s", "send the prompt with Enter"),
            s("f", "ask a follow-up"),
            s("x", "copy the prompt, else the answer"),
            s(&ud, "scroll"),
            scroll,
            help,
            s("esc q", "twice: drop it, back to home"),
            s("^C", "close the panel"),
        ],
        View::Confirm { back, .. } => vec![
            s("y", "confirm"),
            s(ret, "confirm, unless multi-line"),
            s(&ud, "scroll"),
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
    };
    let mut all: Vec<_> = all.into_iter().filter(|(k, _)| !k.is_empty()).collect();
    let hide = if app.view == View::Busy {
        s("^Z", "hide, once the call is back")
    } else {
        s("^Z", "hide; prefix f on this pane brings it back")
    };
    let at = all.iter().position(|(k, _)| k == "? F1" || k == "F1").unwrap_or(all.len());
    all.insert(at, hide);
    all
}

/// The `?` overlay over the whole panel. Returns the key count and rows, for
/// [`App::fit`] to clamp the scroll by.
fn overlay(f: &mut Frame, area: Rect, app: &App, th: &Theme, top: usize) -> (usize, usize) {
    crate::chrome::help(f, area, th, &keys(app, &th.g), top)
}

/// `tick` is tenths of a second since the call went out, so a golden never reads
/// the clock. Returns the line count and rows, for [`App::fit`] (the overlay's
/// while open), and the frame's hit map: empty unless the panel is in tablet mode.
pub fn render(f: &mut Frame, area: Rect, app: &App, th: &Theme, tick: u64) -> (usize, usize, Vec<Hit>) {
    let esc = Tap::Key(KeyEvent::from(KeyCode::Esc));
    let anywhere = |on: bool| if on { vec![Hit { at: area, tap: esc }] } else { vec![] };
    // no hits under the floor: the card hides what esc would do (arm, discard, cancel a call)
    if too_small(area, app.tablet) {
        small(f, area, th);
        return (0, 0, vec![]);
    }
    let d = th.g.dot;
    let w = area.width.saturating_sub(4) as usize;
    let all = (area.height as usize).saturating_sub(2);
    // the buttons go first: the big pair 3 rows tall only while the body keeps half the rows
    let pad = |b: &Body| {
        if !app.tablet {
            return ((vec![], vec![]), (vec![], vec![]));
        }
        let low = chips(th, b, w);
        (bigs(th, b, w, low.0.len() + 3 <= all / 2), low)
    };
    let mut b = body(app, th, w, tick, app.tablet);
    let (mut up, mut btn) = pad(&b);
    // tall tiles only while every row of the view fits: never a scroll they caused
    if app.tablet && b.head.len() + b.lines.len() + up.0.len() + btn.0.len() > all {
        b = body(app, th, w, tick, false);
        (up, btn) = pad(&b);
    }
    let rows = all.saturating_sub(btn.0.len() + up.0.len());
    // short of a body row: the head's spacer goes, then the head scrolls with the body
    if b.head.len() >= rows && b.head.last().is_some_and(|l| l.width() == 0) {
        b.head.pop();
    }
    if b.head.len() >= rows {
        // the note stays put: a cap note scrolled off behind the cursor is never read
        let keep = b.pin.min(rows.saturating_sub(1));
        let rest = b.head.split_off(b.pin.min(b.head.len()));
        if keep < b.head.len() {
            b.head.truncate(keep);
            if let Some(l) = b.head.last_mut() {
                let t: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
                *l = Line::styled(th.g.trunc(&format!("{t} {}", th.g.ellipsis), w), l.spans[0].style);
            }
        }
        let n = rest.len();
        b.lines.splice(0..0, rest);
        b.sel = b.sel.map(|(s, e)| (s + n, e + n));
        for ((s, e), _) in &mut b.taps {
            (*s, *e) = (*s + n, *e + n);
        }
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
        .title(Line::from(Span::styled(title, Style::new().fg(th.accent))));
    if !app.tablet {
        block = block.title_bottom(crate::chrome::footer(
            th,
            std::mem::take(&mut b.footer),
            (area.width as usize).saturating_sub(2),
            2,
        ));
    }
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
    let (tops, top_hits) = up;
    let high = Rect {
        height: (tops.len() as u16).min(inner.height),
        ..inner
    };
    f.render_widget(Paragraph::new(tops), high);
    let inner = Rect {
        y: inner.y + high.height,
        height: inner.height - high.height,
        ..inner
    };
    let head = b.head.len();
    let mut lines = b.head;
    lines.extend(b.lines.into_iter().skip(top).take(last - top));
    if cut {
        let n = total - last;
        let s = if n == 1 { "" } else { "s" };
        lines.push(styled(format!("{} {n} more line{s}", th.g.ellipsis), Style::new().fg(th.meta)));
    }
    let (pads, pad_hits) = btn;
    let low = Rect {
        y: inner.y + inner.height.saturating_sub(pads.len() as u16),
        height: (pads.len() as u16).min(inner.height),
        ..inner
    };
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            height: inner.height - low.height,
            ..inner
        },
    );
    f.render_widget(Paragraph::new(pads), low);
    if let Some(top) = app.help {
        let (t, r) = overlay(f, area, app, th, top);
        return (t, r, anywhere(app.tablet));
    }
    let mut hits = vec![];
    for ((s, e), tap) in b.taps {
        let (s, e) = (s.max(top), e.min(last));
        if s < e {
            let y = inner.y + (head + s - top) as u16;
            hits.push(Hit {
                at: Rect::new(inner.x, y, inner.width, (e - s) as u16),
                tap,
            });
        }
    }
    let place = |hs: Vec<Hit>, r: Rect| {
        hs.into_iter().map(move |h| Hit {
            at: Rect {
                x: r.x + h.at.x,
                y: r.y + h.at.y,
                ..h.at
            }
            .intersection(r),
            ..h
        })
    };
    hits.extend(place(top_hits, high));
    hits.extend(place(pad_hits, low));
    hits.retain(|h| app.tablet && !h.at.is_empty());
    (total, room, hits)
}

/// The tap a frame's hit map puts under a cell.
pub fn hit(hits: &[Hit], x: u16, y: u16) -> Option<Tap> {
    let p = ratatui::layout::Position { x, y };
    hits.iter().find(|h| h.at.contains(p)).map(|h| h.tap)
}

/// A mouse event in tablet mode: a bare left press taps, the wheel scrolls.
/// The release is ignored, so one tap is one action; `now` feeds the tap guard.
/// Shift, Ctrl or Alt held is no tap: a click meant for the terminal.
pub fn mouse(app: &mut App, hits: &[Hit], m: MouseEvent, now: u64) {
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) if m.modifiers.is_empty() => {
            if let Some(t) = hit(hits, m.column, m.row) {
                app.tap(t, now);
            }
        }
        MouseEventKind::ScrollUp => app.wheel(true),
        MouseEventKind::ScrollDown => app.wheel(false),
        _ => {}
    }
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
    crate::backend::exit_on_hangup(|| {
        let _ = crate::backend::mouse(false);
    });
    let th = Theme::load();
    // SAFETY: on_stop only stores an atomic and raises a signal.
    unsafe {
        libc::signal(libc::SIGHUP, on_stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_stop as *const () as libc::sighandler_t);
    }
    let mut term = match crate::backend::init(true, app.tablet) {
        Ok(t) => t,
        Err(e) => {
            // init may fail after raw mode and the paste mode are already on
            crate::backend::restore(true);
            eprintln!("cockpit-tui: {e}");
            std::process::exit(1);
        }
    };
    let call = Call::from_env();
    let born = std::time::Instant::now();
    let ms = || born.elapsed().as_millis() as u64;
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
        let (mut page, mut tiny, mut size) = ((0, 0, vec![]), false, (0, 0));
        let drawn = term.draw(|f| {
            size = (f.area().width, f.area().height);
            tiny = too_small(f.area(), app.tablet);
            page = render(f, f.area(), app, &th, tick);
        });
        if drawn.is_err() {
            break;
        }
        app.shrunk(tiny);
        app.size = Some(size);
        app.frame(ms());
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
            Ok(Some(Event::Mouse(m))) if app.tablet => mouse(app, &page.2, m, ms()),
            Ok(_) => {}
            Err(e) if transient(&e) => {}
            Err(_) => break,
        }
        if let Some(t) = app.copy.take()
            && let Err(e) = crate::backend::copy(&t)
        {
            app.say(Level::Warn, format!("could not copy: {e}"));
        }
        // the Home tile flips the mode between two frames
        if let Err(e) = crate::backend::mouse(app.tablet) {
            app.say(Level::Warn, format!("could not switch the mouse: {e}"));
        }
    }
    if !app.quit || STOP.load(Ordering::SeqCst) {
        app.stop();
    }
    // the SIGHUP/SIGTERM path breaks out of the loop above and lands here too
    crate::backend::restore(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    /// Highlighting adds style and nothing else: any text, any width, the rows
    /// are `many`'s rows exactly.
    #[test]
    fn coded_rows_are_the_plain_rows() {
        let th = Theme::fixed();
        let atoms = [
            "```",
            "```sh\n",
            "```rust\n",
            "\n```\n",
            "\n",
            " ",
            "  ",
            "\t",
            "\"",
            "'",
            "#",
            "/*",
            "fn ",
            "if ",
            "日本語",
            "👍🏽",
            "word",
            "x\u{fe0f}",
            "<<EOF\n",
            "1.5",
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..3000 {
            let n = (next() % 30) as usize;
            let text: String = (0..n).map(|_| atoms[(next() % atoms.len() as u64) as usize]).collect();
            let w = (next() % 60) as usize;
            let st = Style::new().fg(th.value);
            assert_eq!(rows(&coded(&th, &text, w, st)), rows(&many(&text, w, "", st)), "{text:?} at {w}");
        }
    }

    /// wrap keeps at most half the width of indentation: the dropped spaces
    /// must not shift the colours onto the wrong chars.
    #[test]
    fn clamped_indentation_keeps_the_colours_on_their_chars() {
        let th = Theme::fixed();
        let st = Style::new().fg(th.value);
        let got = coded(&th, "x\n```py\n            s = 'ab'\n```", 12, st);
        let row = &got[2];
        assert_eq!(row.to_string(), "      s =", "{got:?}");
        let next = &got[3];
        assert_eq!(next.to_string(), "'ab'");
        assert!(next.spans.iter().all(|s| s.style.fg == Some(th.done)), "{next:?}");
    }
}
