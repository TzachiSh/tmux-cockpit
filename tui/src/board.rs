//! The board: one live-preview tile per agent pane, needs-first. `render` is pure
//! over `&[Pane]` (no tmux, no clock); `run` is the only impure entry.
use crate::data::{Pane, State, fmt_dur, hash_rows, list_panes, log, preview};
use crate::panel::view::wrap;
use crate::push::Push;
use crate::theme::{Glyphs, Theme};
use ansi_to_tui::IntoText;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Padding, Paragraph};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

/// The narrowest tile whose two header lines still carry the meaning.
const MIN_TILE_W: u16 = 48;
/// The elapsed rail. A 6-column duration loses its leading digit.
const RAIL_W: usize = 5;

pub struct View<'a> {
    pub panes: &'a [Pane],
    /// previews, indexed like `panes`, not like the sorted order. `None` is a
    /// failed capture-pane, which is "the pane is gone", not an empty pane.
    pub pvs: &'a [Option<String>],
    pub th: &'a Theme,
    /// position in `order()`, not an index into `panes`
    pub sel: usize,
    /// first visible grid row
    pub top: usize,
    /// tmux's active pane id
    pub active: &'a str,
    pub note: Option<&'a str>,
    /// the help's scroll offset while it is open
    pub help: Option<usize>,
    /// seconds since the last good read, while reads fail or hang
    pub stale: Option<u64>,
    /// tmux has not answered for long enough to say so over the tiles (D16)
    pub lost: bool,
    /// no read has worked since the board opened: no age, no tiles to vouch for
    pub never: bool,
}

/// Three missed 1 s ticks: a read that hangs is as stale as three that failed.
const HUNG: Duration = Duration::from_secs(3);

/// What the key loop knows about its reads. The clock is passed in, so the
/// stale rules are tested without one.
#[derive(Clone, Copy, Debug)]
pub struct Reads {
    pub start: Instant,
    /// when the last snapshot of any kind arrived; `start` before the first
    pub snap: Instant,
    pub good: Option<Instant>,
    /// failed reads in a row
    pub fails: u32,
}

impl Reads {
    pub fn new(now: Instant) -> Self {
        Self {
            start: now,
            snap: now,
            good: None,
            fails: 0,
        }
    }

    pub fn got(&mut self, ok: bool, now: Instant) {
        self.snap = now;
        if ok {
            (self.good, self.fails) = (Some(now), 0);
        } else {
            self.fails += 1;
        }
    }

    /// Seconds since the last good read, once a read fails or the worker has
    /// been silent for [`HUNG`]: tmux can block a read rather than fail it.
    pub fn stale(&self, now: Instant) -> Option<u64> {
        let hung = now.saturating_duration_since(self.snap) >= HUNG;
        (self.fails > 0 || hung).then(|| now.saturating_duration_since(self.good.unwrap_or(self.start)).as_secs())
    }

    /// D16: the card replaces the stale header after 30 s or three failed
    /// reads in a row, and at once when no read has ever worked.
    pub fn lost(&self, now: Instant) -> bool {
        let stale = self.stale(now);
        stale.is_some_and(|s| s >= 30) || self.fails >= 3 || (stale.is_some() && self.good.is_none())
    }
}

/// tmux's active pane, at the tile's top-right end: `▎` is the selection (D14).
/// Open question 1 of the board-panel design; one place to change it.
/// `@` in ascii: every other ascii glyph, `<` (review sent, enter) included, is taken.
const PANE_MARK: (&str, &str) = ("◂", "@");

fn pane_mark(g: &Glyphs) -> &'static str {
    if g.ascii { PANE_MARK.1 } else { PANE_MARK.0 }
}

fn pane_num(id: &str) -> u64 {
    id.trim_start_matches('%').parse().unwrap_or(u64::MAX)
}

/// `(state rank, pane id)`. Never a clock: rank changes are rare and
/// event-driven, ages tick every second and would reorder tiles as you read.
pub fn order(ps: &[Pane]) -> Vec<usize> {
    let mut o: Vec<usize> = (0..ps.len()).collect();
    o.sort_by_key(|&i| (ps[i].state, pane_num(&ps[i].id)));
    o
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub rows_shown: usize,
    pub full: bool,
}

/// The density ladder. Tile height is `avail / rows_shown`, so a half-empty last
/// row renders normal tiles and blank space.
pub fn grid(area: Rect, n: usize) -> Grid {
    let cols = ((area.width / MIN_TILE_W) as usize).clamp(1, 3).min(n.max(1));
    let rows = n.div_ceil(cols).max(1);
    let avail = area.height.saturating_sub(2) as usize;
    let rows_shown = if avail / rows >= 5 { rows } else { (avail / 5).max(1).min(rows) };
    Grid {
        cols,
        rows,
        rows_shown,
        full: avail / rows_shown >= 7,
    }
}

/// Below this the tiles are narrower than a pane name, so a row would lie about
/// which agent it names. fleet.rs refuses at the same width.
const MIN_BOARD_W: u16 = 40;
/// Header, footer and one tile with a border, its header and a preview row.
const MIN_BOARD_H: u16 = 8;

/// Under the floor the card hides the board, so only its way out does anything.
pub fn too_small(area: Rect) -> bool {
    area.width < MIN_BOARD_W || area.height < MIN_BOARD_H
}

pub fn render(f: &mut Frame, area: Rect, v: &View) {
    // below either floor every tile fails its own size guard and the board is a
    // blank rectangle with nothing saying why
    if area.width < MIN_BOARD_W {
        crate::chrome::floor(f, area, v.th, "board", "make the window larger or rotate", "esc or q closes");
        return;
    }
    if area.height < MIN_BOARD_H {
        let msg = format!("too short {} make the window larger or rotate", v.th.g.dot);
        crate::chrome::floor(f, area, v.th, "board", &msg, "esc or q closes");
        return;
    }
    if let Some(top) = v.help {
        crate::chrome::help(f, area, v.th, &help_rows(&v.th.g), top);
        return;
    }
    if v.panes.is_empty() {
        let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        header(f, head, v, &grid(area, 0), 0);
        // the empty card's border would peek out from under the lost one
        if !v.lost {
            empty(f, body, v.th);
        }
    } else {
        grid_board(f, area, v);
    }
    if v.lost {
        lost_card(f, area, v);
    }
}

fn grid_board(f: &mut Frame, area: Rect, v: &View) {
    let ord = order(v.panes);
    let g = grid(area, ord.len());
    // View.top is caller-supplied and the terminal can shrink under it
    let v = &View {
        top: v.top.min(g.rows.saturating_sub(1)),
        ..*v
    };
    let [head, body, foot] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(area);

    header(f, head, v, &g, ord.len());
    tiles(f, body, v, &ord, &g);
    let needs = v.panes[ord[v.sel.min(ord.len() - 1)]].state == State::NeedsInput;
    footer(f, foot, v, needs);
}

fn header(f: &mut Frame, area: Rect, v: &View, g: &Grid, n: usize) {
    let th = v.th;
    let needs = v.panes.iter().filter(|p| p.state == State::NeedsInput).count();
    let mut left = vec![Span::styled(
        format!(" board {} {n} agent{}", th.g.dot, if n == 1 { "" } else { "s" }),
        Style::new().fg(th.label),
    )];
    if needs > 0 {
        left.push(Span::styled(format!(" {} {needs} needs you", th.g.dot), Style::new().fg(th.needs)));
    }
    if let Some(s) = v.stale.filter(|_| !v.never) {
        left.push(Span::styled(
            format!(" {} stale {}", th.g.dot, fmt_dur(s)),
            Style::new().fg(th.error),
        ));
    }
    f.render_widget(Line::from(left), area);

    let first = v.top * g.cols;
    let last = n.min((v.top + g.rows_shown) * g.cols);
    if first == 0 && last == n {
        return;
    }
    let mut right = format!("{}-{last} of {n}   ", first + 1);
    if first > 0 {
        right.push_str(&format!("{}{first} ", th.g.up));
    }
    if last < n {
        right.push_str(&format!("{}{} ", th.g.down, n - last));
    }
    f.render_widget(Line::from(Span::styled(right, Style::new().fg(th.meta))).right_aligned(), area);
}

fn footer(f: &mut Frame, area: Rect, v: &View, needs: bool) {
    let th = v.th;
    if let Some(n) = v.note {
        f.render_widget(Line::from(Span::styled(format!(" {n}"), Style::new().fg(th.value))), area);
        return;
    }
    f.render_widget(crate::chrome::footer(th, hints(&th.g, needs), area.width as usize, 3), area);
}

/// The footer's hints from the key table, in priority order, for the selection:
/// `a` answers a ▲ and asks anything else (D14). Only keys the dispatch takes.
pub fn hints(g: &Glyphs, needs: bool) -> Vec<(String, String)> {
    KEYS.iter()
        .filter(|k| !k.hint.is_empty())
        .map(|k| {
            let key = k.keys.split_whitespace().next().unwrap_or(k.keys);
            let verb = if k.acts == [Act::Ask] && needs { "answer" } else { k.hint };
            (names(g, key), verb.to_string())
        })
        .collect()
}

/// The key table speaks unicode; the ascii tier names the keys instead.
fn names(g: &Glyphs, s: &str) -> String {
    if !g.ascii {
        return s.to_string();
    }
    s.replace("⇧⇥", &format!("S-{}", g.key_tab))
        .replace('⇥', g.key_tab)
        .replace('↵', g.key_enter)
        .replace('←', g.key_left)
        .replace('→', g.key_right)
        .replace("↑ ↓", g.key_updown)
}

/// The redraw gate's extra input: `hash_rows` skips the ages, so hash the rail's
/// rendered text, which changes only when the printed age does.
pub fn gate_extra(ps: &[Pane], pvs: &[Option<String>]) -> String {
    let mut out: String = pvs.iter().map(|p| p.as_deref().unwrap_or("\u{1}")).collect();
    out.extend(ps.iter().filter_map(|p| p.since).map(fmt_dur));
    out
}

fn tiles(f: &mut Frame, area: Rect, v: &View, ord: &[usize], g: &Grid) {
    let rows = Layout::vertical(vec![Constraint::Ratio(1, g.rows_shown as u32); g.rows_shown]).split(area);
    for (r, row) in rows.iter().enumerate() {
        let cells = Layout::horizontal(vec![Constraint::Ratio(1, g.cols as u32); g.cols]).split(*row);
        for (c, cell) in cells.iter().enumerate() {
            // the printed number is absolute, the one Act::Tile resolves
            let k = (v.top + r) * g.cols + c;
            let Some(&i) = ord.get(k) else { continue };
            // a short pvs is "not fetched yet" and draws nothing; a stored
            // None is a failed capture and says so
            let pv = v.pvs.get(i).map(|o| o.as_deref());
            tile(
                f,
                *cell,
                &v.panes[i],
                k + 1,
                pv,
                k == v.sel,
                v.panes[i].id == v.active,
                g.full,
                v.th,
            );
        }
    }
}

fn dimmed(state: State) -> bool {
    state >= State::Done
}

#[allow(clippy::too_many_arguments)]
fn tile(f: &mut Frame, cell: Rect, p: &Pane, num: usize, pv: Option<Option<&str>>, focused: bool, active: bool, full: bool, th: &Theme) {
    // ratatui caches layouts, so the grid is re-split every frame; a tile too
    // small for a border and one content row is skipped rather than drawn torn.
    if cell.width < 8 || cell.height < 3 {
        return;
    }
    let g = &th.g;
    let needs = p.state == State::NeedsInput;
    let (gl, gl_c) = th.glyph(p.state);
    let (rv, rv_c) = th.review_mark(&p.review);

    let border_c = if needs {
        th.needs
    } else if p.state == State::Error {
        th.error
    } else if focused {
        th.accent
    } else {
        th.meta
    };

    // the pane mark takes the title's right end, so the left one budgets for it
    let inner_w = (cell.width.saturating_sub(2) as usize).saturating_sub(if active { 3 } else { 0 });
    let n = num.to_string();
    let fixed = 1 + n.width() + 1 + gl.width() + rv.width() + 1;
    let wher = g.trunc(&p.wher, inner_w.saturating_sub(fixed + 1));
    let sep = format!(" {} ", g.dot);
    let title_w = inner_w.saturating_sub(fixed + wher.width() + sep.width() + 1);

    let title_c = if dimmed(p.state) { th.meta } else { th.value };
    // the selection is ▎ plus reverse, so it survives NO_COLOR (D17)
    let num_st = if focused {
        Style::new().fg(th.accent).add_modifier(Modifier::REVERSED)
    } else {
        Style::new().fg(th.meta)
    };
    let mut top = vec![
        Span::styled(if focused { g.active } else { " " }, Style::new().fg(th.accent)),
        Span::styled(n, num_st),
        Span::raw(" "),
        Span::styled(
            gl,
            Style::new()
                .fg(gl_c)
                .add_modifier(if needs { Modifier::BOLD } else { Modifier::empty() }),
        ),
        Span::styled(rv, Style::new().fg(rv_c)),
        Span::raw(" "),
        Span::styled(wher, Style::new().fg(th.meta)),
    ];
    if title_w > 0 {
        top.push(Span::styled(sep, Style::new().fg(th.meta)));
        top.push(Span::styled(g.trunc(p.label(), title_w), Style::new().fg(title_c)));
        top.push(Span::raw(" "));
    }

    // no key hints on the frame: the footer is the board's one key bar (D19)
    let mut block = Block::bordered()
        .border_set(if focused { g.thick } else { g.plain })
        .border_style(Style::new().fg(border_c))
        .title(Line::from(top));
    if active {
        block = block.title(Line::from(Span::styled(format!(" {} ", pane_mark(g)), Style::new().fg(th.accent))).right_aligned());
    }
    let inner = block.inner(cell);
    f.render_widget(block, cell);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let hdr = (if full { 2u16 } else { 1 }).min(inner.height);
    let [hd, body] = Layout::vertical([Constraint::Length(hdr), Constraint::Min(0)]).areas(inner);
    f.render_widget(Paragraph::new(head_lines(p, inner.width as usize, hdr as usize, needs, th)), hd);

    let mut para = match pv {
        Some(Some(text)) => Paragraph::new(text.as_bytes().into_text().unwrap_or_default()),
        // capture-pane failed: never leave the body blank, or a gone pane and a
        // quiet one look the same
        Some(None) => Paragraph::new(Line::from(Span::styled("capture failed", Style::new().fg(th.error)))),
        None => return,
    };
    if dimmed(p.state) {
        // expressed twice: a terminal that ignores DIM still gets the meta title
        para = para.style(Style::new().add_modifier(Modifier::DIM));
    }
    f.render_widget(para, body);
}

fn head_lines(p: &Pane, iw: usize, rows: usize, needs: bool, th: &Theme) -> Vec<Line<'static>> {
    let g = &th.g;
    // one column of air before the rail, or a title that fills its budget
    // runs straight into the elapsed time
    let text_w = iw.saturating_sub(RAIL_W + 1);
    let age = p.since.map(fmt_dur).unwrap_or_default();
    // claudectl's escalation: "blocked 4m12s" in meta reads as a fact, in red as a queue
    let rail_c = match (needs, p.since) {
        (true, Some(s)) if s >= 300 => th.needs,
        (true, Some(s)) if s >= 60 => th.error,
        _ => th.meta,
    };
    // {:>w} is a minimum, not a maximum: a duration past 9999d would take six
    // columns and render an age with its unit clipped off
    let rail = Span::styled(format!(" {:>RAIL_W$}", g.trunc(&age, RAIL_W)), Style::new().fg(rail_c));

    let mut out = Vec::new();
    if needs {
        let parts = fold(&p.msg, text_w, iw, rows, g);
        out.push(Line::from(vec![
            Span::styled(pad(parts[0].clone(), text_w), Style::new().fg(th.needs)),
            rail,
        ]));
        for extra in &parts[1..] {
            out.push(Line::from(Span::styled(extra.clone(), Style::new().fg(th.needs))));
        }
    } else {
        // never the word "unknown": a pane with no transcript reads as a dash
        let doing = if p.doing.is_empty() {
            g.trunc(g.dash, text_w)
        } else {
            g.trunc_middle(&p.doing, text_w)
        };
        out.push(Line::from(vec![
            Span::styled(
                pad(doing, text_w),
                Style::new().fg(if dimmed(p.state) { th.meta } else { th.value }),
            ),
            rail,
        ]));
        if rows > 1 && !p.said.is_empty() {
            out.push(Line::from(Span::styled(g.trunc(&p.said, iw), Style::new().fg(th.quote))));
        }
    }
    out
}

fn pad(mut s: String, w: usize) -> String {
    for _ in s.width()..w {
        s.push(' ');
    }
    s
}

/// Greedy fold of our own text across the header rows; the last row takes the
/// ellipsis. Safe because it is never arbitrary pane output.
fn fold(s: &str, first_w: usize, rest_w: usize, rows: usize, g: &Glyphs) -> Vec<String> {
    let words: Vec<&str> = s.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut budget = first_w;
    let mut taken = 0;
    for (i, word) in words.iter().enumerate() {
        let want = if cur.is_empty() {
            word.width()
        } else {
            cur.width() + 1 + word.width()
        };
        if want > budget && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            budget = rest_w;
            if out.len() == rows {
                break;
            }
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
        taken = i + 1;
    }
    if out.len() < rows && !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    // a line that fits but leaves words behind still needs the marker, or the
    // reader cannot tell the question was cut
    let rest = words[taken.min(words.len())..].join(" ");
    if !rest.is_empty() {
        let last = out.len() - 1;
        out[last] = format!("{} {rest}", out[last]);
    }
    // every line, not just the last: a single token wider than its budget is
    // accepted whole above, and pad() cannot shrink it, so it would eat the rail
    for (i, l) in out.iter_mut().enumerate() {
        *l = g.trunc(l, if i == 0 { first_w } else { rest_w });
    }
    out
}

/// A centred card, the shape every state screen shares. The body wraps, never
/// cut: at 60 columns the advice is the half a reader needs.
fn card(f: &mut Frame, area: Rect, th: &Theme, title: &str, body: &[&str], foot: &[(&str, &str)]) {
    let g = &th.g;
    let w = 62.min(area.width);
    let lines: Vec<Line> = body
        .iter()
        .flat_map(|l| {
            if l.is_empty() {
                return vec![Line::default()];
            }
            wrap(l, w.saturating_sub(4) as usize)
                .into_iter()
                .map(|l| Line::from(Span::styled(l, Style::new().fg(th.value))))
                .collect()
        })
        .collect();
    let r = area.centered(Constraint::Length(w), Constraint::Length(lines.len() as u16 + 2));
    let mut block = Block::bordered()
        .border_set(g.round)
        .border_style(Style::new().fg(th.meta))
        .padding(Padding::horizontal(1))
        .title(Line::from(Span::styled(format!(" {title} "), Style::new().fg(th.label))));
    if !foot.is_empty() {
        let mut spans = vec![];
        for (i, (key, verb)) in foot.iter().enumerate() {
            spans.push(Span::styled(
                format!("{}{key}", if i == 0 { " " } else { "  " }),
                Style::new().fg(th.accent),
            ));
            spans.push(Span::styled(format!(" {verb} "), Style::new().fg(th.value)));
        }
        block = block.title_bottom(Line::from(spans));
    }
    let inner = block.inner(r);
    f.render_widget(Clear, r);
    f.render_widget(block, r);
    f.render_widget(Paragraph::new(lines), inner);
}

fn empty(f: &mut Frame, area: Rect, th: &Theme) {
    card(
        f,
        area,
        th,
        "board",
        &[
            "",
            "no agent panes",
            "",
            "cockpit tracks a pane once an agent runs in it.",
            "start an agent in any pane and it shows up here.",
            "",
        ],
        &[("q", "close")],
    );
}

/// D16's second stage, over the last good read's tiles.
fn lost_card(f: &mut Frame, area: Rect, v: &View) {
    let d = v.th.g.dot;
    if v.never {
        let body = format!("tmux is not answering {d} no read has worked yet.");
        return card(f, area, v.th, "board", &["", &body, ""], &[("^L", "retry"), ("q", "close")]);
    }
    let age = fmt_dur(v.stale.unwrap_or(0));
    let read = format!("last good read {age} ago {d} the server may have exited.");
    card(
        f,
        area,
        v.th,
        &format!("board {d} stale {age}"),
        &["", "tmux is not answering", "", &read, "the tiles below are from that read.", ""],
        &[("^L", "retry"), ("q", "close")],
    );
}

/// Drawn before the first fetch, which is slow enough that a black popup reads
/// as a hang.
pub fn loading(f: &mut Frame, area: Rect, th: &Theme) {
    card(f, area, th, "board", &["", "reading the fleet \u{2026}", ""], &[]);
}

/// The `?` rows: the key table by group, then what each glyph on a tile means.
pub fn help_rows(g: &Glyphs) -> Vec<(String, String)> {
    let head = |t: &str| (String::new(), t.to_string());
    let mut out = vec![];
    for (grp, title) in [(Grp::Move, "move"), (Grp::Act, "act on the selected agent"), (Grp::View, "view")] {
        if !out.is_empty() {
            out.push((String::new(), String::new()));
        }
        out.push(head(title));
        out.extend(KEYS.iter().filter(|k| k.grp == grp).map(|k| (names(g, k.keys), k.desc.to_string())));
    }
    out.push((String::new(), String::new()));
    out.push(head("on a tile"));
    for (k, v) in [
        (g.needs, "needs you"),
        (g.error, "error"),
        (g.running, "running"),
        (g.done, "done"),
        (g.idle, "idle"),
        (g.review_pending, "review pending"),
        (g.review_sent, "review sent"),
        (g.active, "the selection"),
        (pane_mark(g), "tmux's active pane"),
    ] {
        out.push((k.to_string(), v.to_string()));
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Act {
    Left,
    Right,
    Up,
    Down,
    Next,
    Prev,
    Tile(u8),
    First,
    Last,
    NextNeeds,
    PageUp,
    PageDown,
    Jump,
    Ask,
    Review,
    Panel,
    Info,
    Help,
    Refresh,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Grp {
    Move,
    Act,
    View,
}

pub struct Key {
    pub keys: &'static str,
    pub desc: &'static str,
    pub grp: Grp,
    pub acts: &'static [Act],
    /// the footer's verb; empty keeps the key out of the footer
    pub hint: &'static str,
}

/// One table feeds `action`, the footer and the `?` help, so none can drift.
pub const KEYS: &[Key] = &[
    Key {
        keys: "h l  ← →",
        desc: "column",
        grp: Grp::Move,
        acts: &[Act::Left, Act::Right],
        hint: "",
    },
    Key {
        keys: "j k  ↑ ↓",
        desc: "row",
        grp: Grp::Move,
        acts: &[Act::Up, Act::Down],
        hint: "",
    },
    Key {
        keys: "⇥ ⇧⇥",
        desc: "next / previous",
        grp: Grp::Move,
        acts: &[Act::Next, Act::Prev],
        hint: "",
    },
    Key {
        keys: "1-9",
        desc: "tile by number",
        grp: Grp::Move,
        acts: &[Act::Tile(0)],
        hint: "",
    },
    Key {
        keys: "g G",
        desc: "first / last",
        grp: Grp::Move,
        acts: &[Act::First, Act::Last],
        hint: "",
    },
    Key {
        keys: "u",
        desc: "next blocked",
        grp: Grp::Move,
        acts: &[Act::NextNeeds],
        hint: "",
    },
    Key {
        keys: "[ ]",
        desc: "page",
        grp: Grp::Move,
        acts: &[Act::PageUp, Act::PageDown],
        hint: "",
    },
    Key {
        keys: "↵",
        desc: "jump to this pane, close the board",
        grp: Grp::Act,
        acts: &[Act::Jump],
        hint: "jump",
    },
    Key {
        keys: "a",
        desc: "answer / ask this agent",
        grp: Grp::Act,
        acts: &[Act::Ask],
        hint: "ask",
    },
    Key {
        keys: "r",
        desc: "review its diff",
        grp: Grp::Act,
        acts: &[Act::Review],
        hint: "review",
    },
    Key {
        keys: "f",
        desc: "panel for this pane",
        grp: Grp::Act,
        acts: &[Act::Panel],
        hint: "panel",
    },
    Key {
        keys: "i",
        desc: "info for this pane",
        grp: Grp::Act,
        acts: &[Act::Info],
        hint: "info",
    },
    Key {
        keys: "?",
        desc: "this help",
        grp: Grp::View,
        acts: &[Act::Help],
        hint: "keys",
    },
    Key {
        keys: "^L",
        desc: "refresh now",
        grp: Grp::View,
        acts: &[Act::Refresh],
        hint: "",
    },
    Key {
        keys: "q esc",
        desc: "close the board",
        grp: Grp::View,
        acts: &[Act::Quit],
        hint: "close",
    },
];

pub fn action(k: KeyEvent) -> Option<Act> {
    use KeyCode as C;
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        return matches!(k.code, C::Char('l')).then_some(Act::Refresh);
    }
    Some(match k.code {
        C::Char('h') | C::Left => Act::Left,
        C::Char('l') | C::Right => Act::Right,
        C::Char('k') | C::Up => Act::Up,
        C::Char('j') | C::Down => Act::Down,
        C::Tab => Act::Next,
        C::BackTab => Act::Prev,
        C::Char(c @ '1'..='9') => Act::Tile(c as u8 - b'0'),
        C::Char('g') => Act::First,
        C::Char('G') => Act::Last,
        C::Char('u') => Act::NextNeeds,
        C::Char('[') | C::PageUp => Act::PageUp,
        C::Char(']') | C::PageDown => Act::PageDown,
        C::Enter => Act::Jump,
        C::Char('a') => Act::Ask,
        C::Char('r') => Act::Review,
        C::Char('f') => Act::Panel,
        C::Char('i') => Act::Info,
        C::Char('?') => Act::Help,
        C::Char('q') | C::Esc => Act::Quit,
        _ => return None,
    })
}

/// Move the selection and pull the viewport after it. Index arithmetic, never
/// a neighbour graph: the grid is uniform except for a short last row.
pub fn moved(a: Act, sel: usize, n: usize, g: &Grid, top: usize, ps: &[Pane], ord: &[usize]) -> (usize, usize) {
    let Some(last) = n.checked_sub(1) else { return (sel, top) };
    let sel = match a {
        // a column move, not a reading-order move: that is what tab is for
        Act::Left if sel.is_multiple_of(g.cols) => sel,
        Act::Left => sel - 1,
        Act::Right if sel % g.cols == g.cols - 1 => sel,
        Act::Right => (sel + 1).min(last),
        Act::Up => sel.saturating_sub(g.cols),
        Act::Down => (sel + g.cols).min(last),
        Act::Next => (sel + 1) % n,
        Act::Prev => (sel + n - 1) % n,
        Act::First => 0,
        Act::Last => last,
        Act::Tile(t) => (t.max(1) as usize - 1).min(last),
        Act::NextNeeds => (1..=n)
            .map(|d| (sel + d) % n)
            .find(|&k| ps[ord[k]].state == State::NeedsInput)
            .unwrap_or(sel),
        Act::PageUp => sel.saturating_sub(g.cols * g.rows_shown),
        Act::PageDown => (sel + g.cols * g.rows_shown).min(last),
        _ => sel,
    };
    let top = match a {
        // a literal screenful, stopping on the last row rather than the last
        // full page, so a lone last tile keeps its normal size
        Act::PageUp => top.saturating_sub(g.rows_shown),
        Act::PageDown => (top + g.rows_shown).min(g.rows.saturating_sub(1)),
        _ => top,
    };
    (sel, follow(sel, top, g))
}

/// Pull the viewport to the selection. Also runs every tick: the terminal can
/// shrink between frames and leave the focused tile off-screen.
fn follow(sel: usize, top: usize, g: &Grid) -> usize {
    let row = sel / g.cols;
    if row < top {
        row
    } else if row >= top + g.rows_shown {
        row + 1 - g.rows_shown
    } else {
        top
    }
}

/// Clamp the selection and viewport to a pane count; the worker and the key
/// loop both call it, so they agree on which tiles are visible.
fn fit(area: Rect, n: usize, sel: usize, top: usize) -> (usize, usize, Grid) {
    let sel = sel.min(n.saturating_sub(1));
    let g = grid(area, n);
    // the same ceiling moved() pages to, or `]` snaps back within a second
    (sel, follow(sel, top.min(g.rows.saturating_sub(1)), &g), g)
}

const DRAW_BUDGET: u8 = 5;

fn transient(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock)
}

/// What the worker needs from the key loop to capture the visible tiles.
#[derive(Clone, Copy)]
struct Want {
    area: Rect,
    sel: usize,
    top: usize,
}

struct Snapshot {
    panes: Vec<Pane>,
    /// visible tiles only, by pane id
    pvs: Vec<(String, Option<String>)>,
    took: Duration,
    /// the list-panes read worked; `panes` is the last good one either way
    ok: bool,
}

/// The fetch is fork-heavy, so it runs here and never on the key thread. A wake
/// during a fetch queues one more, not many.
fn worker(rx: Receiver<Want>, tx: Sender<Snapshot>, mut want: Want) {
    let mut panes: Vec<Pane> = vec![];
    loop {
        let t0 = Instant::now();
        // None is "tmux did not answer"; keep the last good tile list rather
        // than blanking the board on one failed read
        let got = list_panes();
        let ok = got.is_some();
        if let Some(ps) = got {
            panes = ps;
        }
        let (_, top, g) = fit(want.area, panes.len(), want.sel, want.top);
        let tile_h = (want.area.height.saturating_sub(2) as usize) / g.rows_shown;
        let rows = tile_h.saturating_sub(if g.full { 4 } else { 3 }).max(1);
        let ord = order(&panes);
        // visible tiles only: each capture is a fork. A server that did not
        // answer list-panes fails every capture too, and the last ones stay
        let shown = if ok {
            top * g.cols..panes.len().min((top + g.rows_shown) * g.cols)
        } else {
            0..0
        };
        let pvs = shown
            .map(|k| {
                let id = &panes[ord[k]].id;
                (id.clone(), preview(id, rows))
            })
            .collect();
        let snap = Snapshot {
            panes: panes.clone(),
            pvs,
            took: t0.elapsed(),
            ok,
        };
        if tx.send(snap).is_err() {
            return;
        }
        let due = Instant::now() + Duration::from_secs(1);
        match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
            Ok(w) => want = rx.try_iter().last().unwrap_or(w),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

pub fn run() -> Option<String> {
    // crossterm caches a failed event-reader init forever: probe before init, or
    // a run with no controlling terminal paints a TUI that ignores every key.
    if let Err(e) = event::poll(Duration::ZERO) {
        eprintln!("cockpit-tui: no usable terminal: {e}");
        std::process::exit(1);
    }
    // the SIGHUP arm usually wins; this covers a hangup that brings no signal
    crate::backend::exit_on_hangup(crate::push::cleanup);
    let th = Theme::load();
    let active = crate::data::tmux(&["display-message", "-p", "#{pane_id}"])
        .unwrap_or_default()
        .trim()
        .to_string();
    let push = Push::start();
    // no text box to paste into: the board keeps the input it always had
    let mut term = match crate::backend::init(false, false) {
        Ok(t) => t,
        Err(e) => {
            // Push::start() already published the pid and exit() skips Drop
            crate::push::cleanup();
            eprintln!("cockpit-tui: {e}");
            std::process::exit(1);
        }
    };
    let (mut panes, mut pvs) = (Vec::<Pane>::new(), Vec::<Option<String>>::new());
    // previews survive between snapshots, so a tile scrolled into view shows
    // its last capture instead of a false "capture failed"
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    let (mut sel, mut top, mut loaded) = (0usize, 0usize, false);
    let mut help: Option<usize> = None;
    // D16: when the last good read was, and how many have failed since
    let (mut reads, mut stale_shown) = (Reads::new(Instant::now()), None::<u64>);
    let mut note: Option<(String, Instant)> = None;
    let mut last_hash = 0u64;
    let (mut dirty, mut draw_fails) = (true, 0u8);
    let mut action_line = None;
    let area = |s: ratatui::layout::Size| Rect::new(0, 0, s.width, s.height);

    let (want_tx, want_rx) = mpsc::channel::<Want>();
    let (snap_tx, snap_rx) = mpsc::channel::<Snapshot>();
    let first = Want {
        area: area(term.size().unwrap_or_default()),
        sel,
        top,
    };
    std::thread::spawn(move || worker(want_rx, snap_tx, first));
    // a failed send means the worker is gone, which the snapshot arm notices
    let ask = |a: Rect, sel: usize, top: usize| {
        let _ = want_tx.send(Want { area: a, sel, top });
    };

    loop {
        // a push only pulls the next fetch forward, so a missed signal costs
        // one second and never a hang
        if push.pending() {
            ask(area(term.size().unwrap_or_default()), sel, top);
        }
        if note.as_ref().is_some_and(|(_, t)| t.elapsed() >= Duration::from_secs(2)) {
            note = None;
            dirty = true;
        }
        match snap_rx.try_recv() {
            Ok(s) => {
                let t0 = Instant::now();
                let s = snap_rx.try_iter().last().unwrap_or(s);
                reads.got(s.ok, Instant::now());
                dirty |= !s.ok;
                panes = s.panes;
                seen.retain(|id, _| panes.iter().any(|p| &p.id == id));
                seen.extend(s.pvs);
                // never fetched draws as an empty body, not as a failed capture
                pvs = panes
                    .iter()
                    .map(|p| seen.get(&p.id).cloned().unwrap_or(Some(String::new())))
                    .collect();
                (sel, top, _) = fit(area(term.size().unwrap_or_default()), panes.len(), sel, top);
                let h = hash_rows(&panes, &gate_extra(&panes, &pvs));
                if h != last_hash || !loaded {
                    last_hash = h;
                    dirty = true;
                }
                loaded = true;
                log(&format!(
                    "board tick {} tiles fetch {:?} apply {:?} redraw={}",
                    panes.len(),
                    s.took,
                    t0.elapsed(),
                    dirty
                ));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }
        if dirty && !loaded && stale_shown.is_none() {
            // the first fetch can take most of a second, and a popup that is a
            // blank rectangle until it returns reads as a hang
            if term.draw(|f| loading(f, f.area(), &th)).is_ok() {
                dirty = false;
            }
        }
        let now = Instant::now();
        let stale = reads.stale(now);
        if stale != stale_shown {
            stale_shown = stale;
            dirty = true;
        }
        // a first read that hangs must not leave the loading card up forever
        if dirty && (loaded || stale.is_some()) {
            let v = View {
                panes: &panes,
                pvs: &pvs,
                th: &th,
                sel,
                top,
                active: &active,
                note: note.as_ref().map(|(s, _)| s.as_str()),
                help,
                stale,
                lost: reads.lost(now),
                never: reads.good.is_none(),
            };
            // size() errno can name its tput fallback rather than the terminal,
            // so count failures instead of printing them
            match term.draw(|f| render(f, f.area(), &v)) {
                Ok(_) => {
                    draw_fails = 0;
                    dirty = false;
                }
                Err(_) => {
                    draw_fails += 1;
                    if draw_fails >= DRAW_BUDGET {
                        break;
                    }
                }
            }
        }

        // short, because this poll is also how soon a finished fetch is drawn
        let ev = match event::poll(Duration::from_millis(50)) {
            Ok(true) => match event::read() {
                Ok(e) => Some(e),
                Err(e) if transient(&e) => None,
                Err(_) => break,
            },
            Ok(false) => None,
            Err(e) if transient(&e) => None,
            Err(_) => break,
        };
        // a resize is the same request as USR1: look again
        if let Some(Event::Resize(..)) = &ev {
            ask(area(term.size().unwrap_or_default()), sel, top);
            dirty = true;
        }
        let Some(Event::Key(k)) = ev else { continue };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        dirty = true;
        let here = area(term.size().unwrap_or_default());
        if too_small(here) {
            help = None;
        }
        if let Some(t) = help {
            // a help screen you have to learn a key to leave is a joke: any
            // key but a scroll key closes it
            let rows = here.height.saturating_sub(2) as usize;
            help = crate::chrome::scroll(k.code, t, help_rows(&th.g).len().saturating_sub(rows), rows);
            continue;
        }
        let Some(a) = action(k) else { continue };
        // under the floor or with no tiles nobody can see what a key would do
        if too_small(here) || panes.is_empty() {
            match a {
                Act::Quit => break,
                Act::Help if !too_small(here) => help = Some(0),
                Act::Refresh => ask(here, sel, top),
                _ => {}
            }
            continue;
        }
        let ord = order(&panes);
        let g = grid(here, panes.len());
        let p = &panes[ord[sel]];
        let id = p.id.clone();
        match a {
            Act::Quit => break,
            Act::Help => help = Some(0),
            Act::Refresh => ask(here, sel, top),
            Act::Jump => {
                action_line = Some(format!("jump {id}"));
                break;
            }
            Act::Review => {
                action_line = Some(format!("review {id}"));
                break;
            }
            Act::Panel => {
                action_line = Some(format!("panel {id}"));
                break;
            }
            Act::Info => {
                action_line = Some(format!("info {id}"));
                break;
            }
            // `a` on a blocked tile jumps; there is no inline reply yet
            Act::Ask if p.state == State::NeedsInput => {
                action_line = Some(format!("jump {id}"));
                break;
            }
            Act::Ask => {
                let msg = format!(
                    "{} is not waiting on you {} {} jumps there, /btw asks it while it works",
                    p.wher, th.g.dot, th.g.enter
                );
                note = Some((msg, Instant::now()));
            }
            _ => {
                let was = top;
                (sel, top) = moved(a, sel, panes.len(), &g, top, &panes, &ord);
                // newly visible tiles need a capture now, not at the next second
                if top != was {
                    ask(here, sel, top);
                }
            }
        }
    }
    ratatui::restore();
    action_line
}
