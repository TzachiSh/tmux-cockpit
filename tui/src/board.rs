//! The board: a list of agents grouped by tmux session (the default), or one
//! live-preview tile per agent behind `v`. `render` is pure over `&[Pane]` (no
//! tmux, no clock); `run` is the only impure entry.
use crate::data::fleet as facts;
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
use std::collections::{BTreeSet, HashMap};
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
    /// the grouped list and its selection; `None` draws the tiles (`v`)
    pub list: Option<&'a Nav>,
    /// the confirm over either layout, which then owns the keys (D18)
    pub confirm: Option<&'a Confirm>,
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
        let ks = match v.confirm {
            Some(_) => confirm_help_rows(),
            None => help_rows(&v.th.g, v.list.is_some()),
        };
        crate::chrome::help(f, area, v.th, &ks, top);
        return;
    }
    if v.panes.is_empty() {
        // no key bar here, but a note still has to be seen: a ✕ one takes the next esc
        let foot = u16::from(v.note.is_some());
        let [head, body, row] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(foot)]).areas(area);
        header(f, head, v, &grid(area, 0), 0, None);
        if let Some(n) = v.note {
            f.render_widget(note_line(v.th, n, row.width as usize), row);
        }
        // the empty card's border would peek out from under the lost one
        if !v.lost {
            empty(f, body, v.th);
        }
    } else if let Some(nav) = v.list {
        list_board(f, area, v, nav);
    } else {
        grid_board(f, area, v);
    }
    if let Some(c) = v.confirm.filter(|_| !v.panes.is_empty()) {
        let [_, body, _] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(area);
        confirm_card(f, body, v.th, c);
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

    header(f, head, v, &g, ord.len(), None);
    tiles(f, body, v, &ord, &g);
    let needs = v.panes[ord[v.sel.min(ord.len() - 1)]].state == State::NeedsInput;
    footer(f, foot, v, Focus::Agent(needs));
}

/// `sessions` is the list's: it adds the session and error counts (6.1).
fn header(f: &mut Frame, area: Rect, v: &View, g: &Grid, n: usize, sessions: Option<usize>) {
    let th = v.th;
    let needs = v.panes.iter().filter(|p| p.state == State::NeedsInput).count();
    let mut left = vec![Span::styled(
        format!(" board {} {n} agent{}", th.g.dot, if n == 1 { "" } else { "s" }),
        Style::new().fg(th.label),
    )];
    if let Some(s) = sessions {
        left.push(Span::styled(
            format!(" {} {s} session{}", th.g.dot, if s == 1 { "" } else { "s" }),
            Style::new().fg(th.label),
        ));
    }
    if needs > 0 {
        left.push(Span::styled(format!(" {} {needs} needs you", th.g.dot), Style::new().fg(th.needs)));
    }
    let errors = v.panes.iter().filter(|p| p.state == State::Error).count();
    if sessions.is_some() && errors > 0 {
        left.push(Span::styled(
            format!(" {} {errors} error{}", th.g.dot, if errors == 1 { "" } else { "s" }),
            Style::new().fg(th.error),
        ));
    }
    if let Some(s) = v.stale.filter(|_| !v.never) {
        left.push(Span::styled(
            format!(" {} stale {}", th.g.dot, fmt_dur(s)),
            Style::new().fg(th.error),
        ));
    }
    f.render_widget(Line::from(left), area);
    // the list says where it is in its own frame's title (D19)
    if sessions.is_some() {
        return;
    }

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

fn footer(f: &mut Frame, area: Rect, v: &View, focus: Focus) {
    let th = v.th;
    // the confirm owns the keys, so its keys are the bar, over any note (D19)
    if let Some(c) = v.confirm {
        f.render_widget(crate::chrome::footer(th, confirm_hints(c), area.width as usize, 3), area);
        return;
    }
    if let Some(n) = v.note {
        f.render_widget(note_line(th, n, area.width as usize), area);
        return;
    }
    let ks = hints(&th.g, v.list.is_some(), focus);
    f.render_widget(crate::chrome::footer(th, ks, area.width as usize, 3), area);
}

/// A failure note ends with its way out, `esc dismiss`, which the cut never
/// takes (D20). The ascii tier gets its own `✕`.
fn note_line(th: &Theme, n: &str, w: usize) -> Line<'static> {
    let g = &th.g;
    let Some(rest) = n.strip_prefix('✕') else {
        return Line::from(Span::styled(
            format!(" {}", g.trunc(n, w.saturating_sub(1))),
            Style::new().fg(th.value),
        ));
    };
    let tail = format!(" {} ", g.dot);
    let text = g.trunc(
        &format!("{}{rest}", g.error),
        w.saturating_sub(1 + tail.width() + "esc dismiss".width()),
    );
    Line::from(vec![
        Span::styled(format!(" {text}"), Style::new().fg(th.error)),
        Span::styled(tail, Style::new().fg(th.value)),
        Span::styled("esc", Style::new().fg(th.accent)),
        Span::styled(" dismiss", Style::new().fg(th.value)),
    ])
}

/// What has the keys: an agent (`true` on a `▲`), or a list header (`true`
/// when folded). The tiles only ever have an agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Agent(bool),
    Head(bool),
}

/// The footer's hints from the key table, in priority order, for what has the
/// keys (D19): `a` answers a ▲ and asks anything else (D14), and a header
/// lists the fold keys instead. Only keys the dispatch takes there.
pub fn hints(g: &Glyphs, list: bool, focus: Focus) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = KEYS
        .iter()
        .filter(|k| k.on.shown(list))
        .filter_map(|k| {
            let verb = match focus {
                Focus::Agent(_) => k.hint,
                Focus::Head(_) => k.head,
            };
            if verb.is_empty() {
                return None;
            }
            let verb = match (k.acts, focus) {
                ([Act::Ask], Focus::Agent(true)) => "answer",
                ([Act::Toggle], Focus::Head(true)) => "unfold",
                _ => verb,
            };
            // `n p` is one hint for two keys, as the mockups draw it
            let key = if k.acts == [Act::NextSess, Act::PrevSess] {
                k.keys
            } else {
                k.keys.split_whitespace().next().unwrap_or(k.keys)
            };
            Some((names(g, key), verb.to_string()))
        })
        .collect();
    // #228's actions need an agent; they go last before `?`, so the cut takes them first
    if let Focus::Agent(_) = focus {
        let at = out.iter().position(|(k, _)| k == "?").unwrap_or(out.len());
        out.splice(at..at, EXTRAS.iter().map(|e| (e.key.to_string(), e.label.to_string())));
    }
    out
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
        top.push(Span::styled(sep, Style::new().fg(th.faint)));
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
        .border_style(Style::new().fg(th.faint))
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

/// The `?` rows for the layout on screen (D2): the key table by group, then
/// what each glyph means.
pub fn help_rows(g: &Glyphs, list: bool) -> Vec<(String, String)> {
    let head = |t: &str| (String::new(), t.to_string());
    let mut out = vec![];
    let grps: &[(Grp, &str)] = if list {
        &[
            (Grp::Move, "move"),
            (Grp::Fold, "groups"),
            (Grp::Act, "act on the selected agent"),
            (Grp::View, "view"),
        ]
    } else {
        &[(Grp::Move, "move"), (Grp::Act, "act on the selected agent"), (Grp::View, "view")]
    };
    for &(grp, title) in grps {
        if !out.is_empty() {
            out.push((String::new(), String::new()));
        }
        out.push(head(title));
        out.extend(
            KEYS.iter()
                .filter(|k| k.grp == grp && k.on.shown(list))
                .map(|k| (names(g, k.keys), k.desc.to_string())),
        );
        if grp == Grp::Act {
            out.extend(EXTRAS.iter().map(|e| (e.key.to_string(), e.help.to_string())));
        }
    }
    out.push((String::new(), String::new()));
    out.push(head(if list { "on a row" } else { "on a tile" }));
    let mut legend = vec![
        (g.needs, "needs you"),
        (g.error, "error"),
        (g.running, "running"),
        (g.done, "done"),
        (g.idle, "idle"),
        (g.review_pending, "review pending"),
        (g.review_sent, "review sent"),
    ];
    if list {
        legend.extend([(fold_mark(g, false), "an open group"), (fold_mark(g, true), "a folded group")]);
    }
    legend.extend([(g.active, "the selection"), (pane_mark(g), "tmux's active pane")]);
    out.extend(legend.into_iter().map(|(k, v)| (k.to_string(), v.to_string())));
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
    Fold,
    Unfold,
    Toggle,
    FoldAll,
    UnfoldAll,
    NextSess,
    PrevSess,
    Layout,
    /// one of [`EXTRAS`], by its key
    Extra(char),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Grp {
    Move,
    Fold,
    Act,
    View,
}

/// Which layout a key row belongs to: `h`, `l` and the digits mean different
/// things in the list and the tiles (D2, 9.1).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum On {
    Both,
    List,
    Tiles,
}

impl On {
    pub fn shown(self, list: bool) -> bool {
        match self {
            On::Both => true,
            On::List => list,
            On::Tiles => !list,
        }
    }
}

pub struct Key {
    pub keys: &'static str,
    pub desc: &'static str,
    pub grp: Grp,
    pub acts: &'static [Act],
    /// the footer's verb on an agent; empty keeps the key out of that footer
    pub hint: &'static str,
    /// the footer's verb on a list header (D19); empty keeps it out
    pub head: &'static str,
    pub on: On,
}

/// One table feeds `action`, the footer and the `?` help, so none can drift.
/// Its order is the footer's priority: the fold keys follow the acts, and `n p`
/// follows the fold keys, as a header's footer lists them (6.2).
pub const KEYS: &[Key] = &[
    Key {
        keys: "h l  ← →",
        desc: "column",
        grp: Grp::Move,
        acts: &[Act::Left, Act::Right],
        hint: "",
        head: "",
        on: On::Tiles,
    },
    Key {
        keys: "j k  ↑ ↓",
        desc: "row",
        grp: Grp::Move,
        acts: &[Act::Up, Act::Down],
        hint: "",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "⇥ ⇧⇥",
        desc: "next / previous agent",
        grp: Grp::Move,
        acts: &[Act::Next, Act::Prev],
        hint: "",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "1-9",
        desc: "tile by number",
        grp: Grp::Move,
        acts: &[Act::Tile(0)],
        hint: "",
        head: "",
        on: On::Tiles,
    },
    Key {
        keys: "1-9",
        desc: "agent by its number on screen",
        grp: Grp::Move,
        acts: &[Act::Tile(0)],
        hint: "",
        head: "",
        on: On::List,
    },
    Key {
        keys: "g G",
        desc: "first / last",
        grp: Grp::Move,
        acts: &[Act::First, Act::Last],
        hint: "",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "u",
        desc: "next blocked",
        grp: Grp::Move,
        acts: &[Act::NextNeeds],
        hint: "",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "[ ]",
        desc: "page",
        grp: Grp::Move,
        acts: &[Act::PageUp, Act::PageDown],
        hint: "",
        head: "",
        on: On::Tiles,
    },
    Key {
        keys: "[ ]",
        desc: "scroll a screenful",
        grp: Grp::Move,
        acts: &[Act::PageUp, Act::PageDown],
        hint: "",
        head: "",
        on: On::List,
    },
    Key {
        keys: "↵",
        desc: "jump to this pane, close the board",
        grp: Grp::Act,
        acts: &[Act::Jump],
        hint: "jump",
        head: "",
        on: On::Tiles,
    },
    Key {
        keys: "↵",
        desc: "jump to this pane; on a header, fold / unfold",
        grp: Grp::Act,
        acts: &[Act::Jump],
        hint: "jump",
        head: "",
        on: On::List,
    },
    Key {
        keys: "a",
        desc: "answer / ask this agent",
        grp: Grp::Act,
        acts: &[Act::Ask],
        hint: "ask",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "r",
        desc: "review its diff",
        grp: Grp::Act,
        acts: &[Act::Review],
        hint: "review",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "f",
        desc: "panel for this pane",
        grp: Grp::Act,
        acts: &[Act::Panel],
        hint: "panel",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "i",
        desc: "info for this pane",
        grp: Grp::Act,
        acts: &[Act::Info],
        hint: "info",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "space",
        desc: "fold / unfold the group",
        grp: Grp::Fold,
        acts: &[Act::Toggle],
        hint: "fold",
        head: "fold",
        on: On::List,
    },
    Key {
        keys: "h ←",
        desc: "fold the group",
        grp: Grp::Fold,
        acts: &[Act::Fold],
        hint: "",
        head: "",
        on: On::List,
    },
    Key {
        keys: "l →",
        desc: "unfold, or enter an open group",
        grp: Grp::Fold,
        acts: &[Act::Unfold],
        hint: "",
        head: "open",
        on: On::List,
    },
    Key {
        keys: "-",
        desc: "fold all",
        grp: Grp::Fold,
        acts: &[Act::FoldAll],
        hint: "",
        head: "fold all",
        on: On::List,
    },
    Key {
        keys: "=",
        desc: "unfold all",
        grp: Grp::Fold,
        acts: &[Act::UnfoldAll],
        hint: "",
        head: "open all",
        on: On::List,
    },
    Key {
        keys: "n p",
        desc: "next / previous session",
        grp: Grp::Move,
        acts: &[Act::NextSess, Act::PrevSess],
        hint: "",
        head: "sessions",
        on: On::List,
    },
    Key {
        keys: "?",
        desc: "this help",
        grp: Grp::View,
        acts: &[Act::Help],
        hint: "keys",
        head: "keys",
        on: On::Both,
    },
    Key {
        keys: "v",
        desc: "tiles / list",
        grp: Grp::View,
        acts: &[Act::Layout],
        hint: "",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "^L",
        desc: "refresh now",
        grp: Grp::View,
        acts: &[Act::Refresh],
        hint: "",
        head: "",
        on: On::Both,
    },
    Key {
        keys: "q esc",
        desc: "close the board",
        grp: Grp::View,
        acts: &[Act::Quit],
        hint: "close",
        head: "close",
        on: On::Both,
    },
];

/// The key's act in the layout on screen: `h` `l` fold in the list and move a
/// column in the tiles, tree-view convention against grid convention (9.1).
pub fn action(k: KeyEvent, list: bool) -> Option<Act> {
    use KeyCode as C;
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        return matches!(k.code, C::Char('l')).then_some(Act::Refresh);
    }
    Some(match k.code {
        C::Char('h') | C::Left if list => Act::Fold,
        C::Char('l') | C::Right if list => Act::Unfold,
        C::Char(' ') if list => Act::Toggle,
        C::Char('-') if list => Act::FoldAll,
        C::Char('=') if list => Act::UnfoldAll,
        C::Char('n') if list => Act::NextSess,
        C::Char('p') if list => Act::PrevSess,
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
        C::Char('v') => Act::Layout,
        C::Char('?') => Act::Help,
        C::Char('q') | C::Esc => Act::Quit,
        C::Char(c) if extra(c).is_some() => Act::Extra(c),
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

// ------------------------------------------------ the list (D1, D3-D6)

/// A list header's fold state. Local, as `PANE_MARK` is: theme.rs has no
/// fold glyph. The ascii tier borrows tree-view convention.
const FOLD_MARK: [(&str, &str); 2] = [("▾", "v"), ("▸", ">")];

fn fold_mark(g: &Glyphs, folded: bool) -> &'static str {
    let (u, a) = FOLD_MARK[usize::from(folded)];
    if g.ascii { a } else { u }
}

/// The tmux session a pane lives in: `wher` is `session:window`, and a
/// session name never holds a `:` (tmux swaps it for `_`).
pub fn session(p: &Pane) -> &str {
    p.wher.rsplit_once(':').map_or(p.wher.as_str(), |(s, _)| s)
}

fn window(p: &Pane) -> &str {
    p.wher.rsplit_once(':').map_or("", |(_, w)| w)
}

/// `▲` and `✕`: the rows a fold never hides (D4) and the hot band (D3).
fn urgent(s: State) -> bool {
    matches!(s, State::NeedsInput | State::Error)
}

/// D3: one group per session. Sessions holding a `▲` or `✕` first, then the
/// rest, each band by name; inside a group, `(state rank, pane id)`. No clock:
/// a group moves only when it starts or stops needing you.
pub fn groups(ps: &[Pane]) -> Vec<(&str, Vec<usize>)> {
    let mut gs: Vec<(&str, Vec<usize>)> = vec![];
    for i in order(ps) {
        let s = session(&ps[i]);
        match gs.iter_mut().find(|(n, _)| *n == s) {
            Some((_, v)) => v.push(i),
            None => gs.push((s, vec![i])),
        }
    }
    gs.sort_by_key(|(n, v)| (!v.iter().any(|&i| urgent(ps[i].state)), *n));
    gs
}

/// Every agent in list order, folds ignored: what D5's "first `▲`" reads.
pub fn list_order(ps: &[Pane]) -> Vec<usize> {
    groups(ps).into_iter().flat_map(|(_, v)| v).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// a session's header
    Head(String),
    /// an index into the panes
    Agent(usize),
}

/// The rows on the list: each header, then its agents. A folded group keeps
/// every `▲` and `✕` row, so folding never hides an agent waiting on you (D4).
pub fn rows(ps: &[Pane], folds: &BTreeSet<String>) -> Vec<Row> {
    let mut out = vec![];
    for (s, v) in groups(ps) {
        let shut = folds.contains(s);
        out.push(Row::Head(s.to_string()));
        out.extend(v.into_iter().filter(|&i| !shut || urgent(ps[i].state)).map(Row::Agent));
    }
    out
}

/// The selection is an identity, never an index (D5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    Pane(String),
    Head(String),
}

/// The list's state: the selection, the viewport and the folds. Every method
/// takes the panes, so a refresh and a key go through the same rules.
#[derive(Clone, Debug, PartialEq)]
pub struct Nav {
    pub sel: Sel,
    /// the first row on screen
    pub top: usize,
    /// folded sessions; nothing unfolds by itself (D4)
    pub folds: BTreeSet<String>,
    /// the row the selection was last placed on
    row: usize,
    /// its session and its offset under that header, for D5's fallback
    at: (String, usize),
    /// no row yet: the first placement scrolls only as far as it must
    placed: bool,
}

/// The list rows a screen of `area` shows: the header, the frame's two
/// borders and the footer take four.
pub fn list_room(area: Rect) -> usize {
    area.height.saturating_sub(4) as usize
}

/// `n` steps along `ks` (sorted rows) from `r`, wrapping; `None` when empty.
fn cycle(ks: &[usize], r: usize, fwd: bool) -> Option<usize> {
    if fwd {
        ks.iter().find(|&&k| k > r).or(ks.first()).copied()
    } else {
        ks.iter().rev().find(|&&k| k < r).or(ks.last()).copied()
    }
}

impl Nav {
    pub fn new(sel: Sel, folds: BTreeSet<String>) -> Self {
        Self {
            sel,
            top: 0,
            folds,
            row: 0,
            at: (String::new(), 0),
            placed: false,
        }
    }

    /// The selection's row: by identity; a pane a fold hides, its header; a
    /// pane that is gone, the row now at its place in its session, else that
    /// session's header; else the first row (D5). Pure, so render can call it.
    pub fn find(&self, rows: &[Row], ps: &[Pane]) -> usize {
        let head = |s: &str| rows.iter().position(|r| matches!(r, Row::Head(h) if h == s));
        let hit = match &self.sel {
            Sel::Pane(id) => rows
                .iter()
                .position(|r| matches!(r, Row::Agent(i) if ps[*i].id == *id))
                .or_else(|| ps.iter().find(|p| p.id == *id).and_then(|p| head(session(p)))),
            Sel::Head(s) => head(s),
        };
        hit.or_else(|| {
            let (s, k) = &self.at;
            let h = head(s)?;
            let end = rows[h + 1..]
                .iter()
                .position(|r| matches!(r, Row::Head(_)))
                .map_or(rows.len(), |e| h + 1 + e);
            Some(if h + k < end { h + k } else { h })
        })
        .unwrap_or(0)
    }

    /// Make row `r` the selection, and remember where it sat.
    fn put(&mut self, rows: &[Row], ps: &[Pane], r: usize) {
        let Some(row) = rows.get(r) else { return };
        self.sel = match row {
            Row::Head(s) => Sel::Head(s.clone()),
            Row::Agent(i) => Sel::Pane(ps[*i].id.clone()),
        };
        let h = rows[..=r].iter().rposition(|r| matches!(r, Row::Head(_))).unwrap_or(0);
        if let Row::Head(s) = &rows[h] {
            self.at = (s.clone(), r - h);
        }
        self.row = r;
    }

    /// Keep the viewport in the list and the selection in the viewport.
    fn clamp(&mut self, n: usize, room: usize) {
        let room = room.max(1);
        self.top = self.top.min(n.saturating_sub(room));
        if self.row < self.top {
            self.top = self.row;
        } else if self.row >= self.top + room {
            self.top = self.row + 1 - room;
        }
    }

    /// Re-place the selection after the rows changed under it, a refresh or a
    /// fold: by identity, on the screen row it had where that fits, so the
    /// viewport moves and the cursor does not (D5).
    pub fn settle(&mut self, ps: &[Pane], room: usize) {
        let rows = rows(ps, &self.folds);
        let r = self.find(&rows, ps);
        if self.placed {
            let screen = self.row.saturating_sub(self.top);
            self.top = r.saturating_sub(screen);
        }
        self.placed = !rows.is_empty();
        self.put(&rows, ps, r);
        self.clamp(rows.len(), room);
    }

    /// The selection's session and whether it is a header.
    fn group_of(&self, rows: &[Row], ps: &[Pane], r: usize) -> Option<(String, bool)> {
        match rows.get(r)? {
            Row::Head(s) => Some((s.clone(), true)),
            Row::Agent(i) => Some((session(&ps[*i]).to_string(), false)),
        }
    }

    /// What has the keys, for the footer.
    pub fn focus(&self, ps: &[Pane]) -> Focus {
        let rows = rows(ps, &self.folds);
        match rows.get(self.find(&rows, ps)) {
            Some(Row::Head(s)) => Focus::Head(self.folds.contains(s)),
            Some(Row::Agent(i)) => Focus::Agent(ps[*i].state == State::NeedsInput),
            None => Focus::Agent(false),
        }
    }

    /// The selected agent; `None` on a header.
    pub fn pane<'p>(&self, ps: &'p [Pane]) -> Option<&'p Pane> {
        let rows = rows(ps, &self.folds);
        match rows.get(self.find(&rows, ps)) {
            Some(Row::Agent(i)) => Some(&ps[*i]),
            _ => None,
        }
    }

    /// One key on the list, `room` rows on screen (D4, D6).
    pub fn act(&mut self, a: Act, ps: &[Pane], room: usize) {
        let rows = rows(ps, &self.folds);
        let Some(last) = rows.len().checked_sub(1) else { return };
        let room = room.max(1);
        // the viewport render draws, which a resize may have moved
        let r = self.find(&rows, ps);
        self.row = r;
        self.clamp(rows.len(), room);
        let agents: Vec<usize> = (0..rows.len()).filter(|&k| matches!(rows[k], Row::Agent(_))).collect();
        let heads: Vec<usize> = (0..rows.len()).filter(|&k| matches!(rows[k], Row::Head(_))).collect();
        let to = match a {
            Act::Up => Some(r.saturating_sub(1)),
            Act::Down => Some((r + 1).min(last)),
            Act::First => Some(0),
            Act::Last => Some(last),
            Act::Next => cycle(&agents, r, true),
            Act::Prev => cycle(&agents, r, false),
            Act::NextSess => cycle(&heads, r, true),
            Act::PrevSess => cycle(&heads, r, false),
            Act::NextNeeds => {
                let needs: Vec<usize> = agents
                    .iter()
                    .copied()
                    .filter(|&k| matches!(rows[k], Row::Agent(i) if ps[i].state == State::NeedsInput))
                    .collect();
                cycle(&needs, r, true)
            }
            // the Nth agent row on screen, never an absolute number (D6)
            Act::Tile(n) => agents
                .iter()
                .copied()
                .filter(|&k| k >= self.top && k < self.top + room)
                .nth(usize::from(n.max(1)) - 1),
            Act::PageUp | Act::PageDown => {
                // a screenful; the selection only clamps to stay on screen
                self.top = if a == Act::PageUp {
                    self.top.saturating_sub(room)
                } else {
                    (self.top + room).min(rows.len().saturating_sub(room))
                };
                let k = r.clamp(self.top, (self.top + room - 1).min(last));
                self.put(&rows, ps, k);
                return;
            }
            Act::Fold | Act::Unfold | Act::Toggle | Act::FoldAll | Act::UnfoldAll => {
                return self.fold(a, &rows, ps, r, room);
            }
            _ => None,
        };
        let Some(t) = to else { return };
        self.put(&rows, ps, t);
        // a digit or `u` that lands in a folded group unfolds it (D4)
        if matches!(a, Act::Tile(_) | Act::NextNeeds)
            && let Some((s, _)) = self.group_of(&rows, ps, t)
            && self.folds.remove(&s)
        {
            return self.settle(ps, room);
        }
        self.clamp(rows.len(), room);
    }

    fn fold(&mut self, a: Act, rows: &[Row], ps: &[Pane], r: usize, room: usize) {
        let Some((s, on_head)) = self.group_of(rows, ps, r) else { return };
        let folded = self.folds.contains(&s);
        match a {
            // on an open header, `l` enters the group
            Act::Unfold if on_head && !folded => {
                if let Some(Row::Agent(_)) = rows.get(r + 1) {
                    self.put(rows, ps, r + 1);
                    self.clamp(rows.len(), room);
                }
                return;
            }
            Act::Unfold => {
                self.folds.remove(&s);
            }
            Act::Fold => {
                self.folds.insert(s);
            }
            Act::Toggle if folded => {
                self.folds.remove(&s);
            }
            Act::Toggle => {
                self.folds.insert(s);
            }
            Act::FoldAll => self.folds.extend(rows.iter().filter_map(|r| match r {
                Row::Head(h) => Some(h.clone()),
                Row::Agent(_) => None,
            })),
            _ => self.folds.clear(),
        }
        // a selected agent the fold hid gives way to its header (find's rule)
        self.settle(ps, room);
    }
}

/// The list row's extra cells (#253), drawn between the message and the age:
/// the diff stat, then the branch (the review mark is the leading column's).
/// `room` is the width they may take, which `agent_row` sizes so the name and
/// message keep at least 18 cells; a cell that does not fit is dropped, with every cell
/// to its right, rather than cut. The facts come off `Pane`, filled on the
/// fetch thread by `data::fleet`: nothing here forks.
pub fn row_extra(p: &Pane, th: &Theme, room: u16) -> Vec<Span<'static>> {
    let g = &th.g;
    let stat = facts::stat_text(&p.loc, &p.pm);
    let stat_c = match stat.as_str() {
        "clean" | "no repo" | "?" => th.meta,
        _ => th.running,
    };
    let br = facts::branch(&p.loc);
    let mut cells = vec![(stat, stat_c)];
    if !br.is_empty() {
        cells.push((format!("{}{}", if g.ascii { "@" } else { "⑂" }, g.trunc(br, 16)), th.label));
    }
    let mut left = room as usize;
    let mut out = vec![];
    for (text, c) in cells {
        let w = text.width() + 1;
        if w > left {
            break;
        }
        left -= w;
        out.push(Span::styled(format!(" {text}"), Style::new().fg(c)));
    }
    out
}

/// The window index column: as wide as the widest index in the fleet.
fn win_w(ps: &[Pane]) -> usize {
    ps.iter().map(|p| window(p).width()).max().unwrap_or(1).max(1)
}

/// The selected row's text: the bar's colour is fg, never the dim role (D17).
/// The band is reverse video, which reads in every tier and under NO_COLOR.
/// #222 adds a `sel` background role to theme.rs; once it lands, the band
/// becomes that background and REVERSED goes.
fn band(st: Style, sel: bool, th: &Theme) -> Style {
    if sel {
        st.fg(th.value).add_modifier(Modifier::REVERSED)
    } else {
        st
    }
}

/// One agent row (6.1): `▎ 1 ▲◆ 0 server  Do you want…  4m12◂`.
#[allow(clippy::too_many_arguments)]
fn agent_row(p: &Pane, num: Option<usize>, sel: bool, active: bool, ww: usize, w: usize, th: &Theme) -> Line<'static> {
    let g = &th.g;
    let (gl, gl_c) = th.glyph(p.state);
    let (rv, rv_c) = th.review_mark(&p.review);
    // a third of what the name and message would share, and never so much that
    // the name keeps under 6 cells and the message under 12
    let flex0 = w.saturating_sub(7 + ww + 1 + 1 + 1 + RAIL_W + 1);
    let extra = row_extra(p, th, (flex0 / 3).min(flex0.saturating_sub(18)) as u16);
    let extra_w: usize = extra.iter().map(|s| s.content.width()).sum();
    // `▎ 1 ▲◆ ` + window + ` ` + name + ` ` + msg + extra + ` ` + rail + mark
    let fixed = 7 + ww + 1 + 1 + 1 + RAIL_W + 1;
    let flex = w.saturating_sub(fixed + extra_w);
    let name_w = (flex / 4).clamp(6, 20).min(flex);
    let msg_w = flex.saturating_sub(name_w);
    let msg = match p.state {
        State::Unknown => "idle".to_string(),
        State::NeedsInput | State::Error if !p.msg.is_empty() => g.trunc(&p.msg, msg_w),
        _ if p.doing.is_empty() => g.trunc(g.dash, msg_w),
        _ => g.trunc_middle(&p.doing, msg_w),
    };
    // idle has no age worth reading: the word says it (6.1)
    let age = match p.state {
        State::Unknown => String::new(),
        _ => p.since.map(fmt_dur).unwrap_or_default(),
    };
    let rail_c = match (p.state == State::NeedsInput, p.since) {
        (true, Some(s)) if s >= 300 => th.needs,
        (true, Some(s)) if s >= 60 => th.error,
        _ => th.meta,
    };
    let text_c = if dimmed(p.state) { th.quote } else { th.value };
    let num = num.map_or(" ".to_string(), |n| n.to_string());
    let mut spans = vec![
        Span::styled(if sel { g.active } else { " " }, Style::new().fg(th.accent)),
        Span::styled(" ", band(Style::new(), sel, th)),
        Span::styled(num, band(Style::new().fg(th.meta), sel, th)),
        Span::styled(" ", band(Style::new(), sel, th)),
        Span::styled(
            gl,
            band(
                Style::new()
                    .fg(gl_c)
                    .add_modifier(if urgent(p.state) { Modifier::BOLD } else { Modifier::empty() }),
                sel,
                th,
            ),
        ),
        Span::styled(rv, band(Style::new().fg(rv_c), sel, th)),
        Span::styled(" ", band(Style::new(), sel, th)),
        Span::styled(
            format!("{:>ww$} ", g.trunc(window(p), ww)),
            band(Style::new().fg(th.label), sel, th),
        ),
        Span::styled(
            pad(g.trunc(p.label(), name_w), name_w + 1),
            band(Style::new().fg(th.value), sel, th),
        ),
        Span::styled(pad(msg, msg_w), band(Style::new().fg(text_c), sel, th)),
    ];
    spans.extend(extra.into_iter().map(|s| {
        let st = band(s.style, sel, th);
        s.style(st)
    }));
    spans.push(Span::styled(
        format!(" {:>RAIL_W$}", g.trunc(&age, RAIL_W)),
        band(Style::new().fg(rail_c), sel, th),
    ));
    spans.push(Span::styled(
        if active { pane_mark(g) } else { " " },
        band(Style::new().fg(th.accent), sel, th),
    ));
    Line::from(spans)
}

/// One header (6.1, 6.2): `▾ api ──── ▲1 ●2 ✓1`, folded `▸ web ── 3 folded · ▲1 ●1`.
fn head_row(name: &str, members: &[usize], ps: &[Pane], folded: bool, sel: bool, w: usize, th: &Theme) -> Line<'static> {
    let g = &th.g;
    let mut counts: Vec<Span> = vec![];
    for st in [State::NeedsInput, State::Error, State::Running, State::Done, State::Unknown] {
        let n = members.iter().filter(|&&i| ps[i].state == st).count();
        if n > 0 {
            let (gl, c) = th.glyph(st);
            counts.push(Span::styled(format!(" {gl}"), band(Style::new().fg(c), sel, th)));
            counts.push(Span::styled(n.to_string(), band(Style::new().fg(th.value), sel, th)));
        }
    }
    if folded {
        let hid = members.iter().filter(|&&i| !urgent(ps[i].state)).count();
        let lead = if hid > 0 {
            format!(" {hid} folded {}", g.dot)
        } else {
            format!(" folded {}", g.dot)
        };
        counts.insert(0, Span::styled(lead, band(Style::new().fg(th.value), sel, th)));
    }
    let counts_w: usize = counts.iter().map(|s| s.content.width()).sum::<usize>() + 2;
    // `▎ ▾ ` + name + ` ` + rule + counts + ` ` + the active column
    let name = g.trunc(name, w.saturating_sub(4 + 1 + 3 + counts_w).max(1));
    let rule = w.saturating_sub(4 + name.width() + 1 + counts_w);
    let mut spans = vec![
        Span::styled(if sel { g.active } else { " " }, Style::new().fg(th.accent)),
        Span::styled(format!(" {} ", fold_mark(g, folded)), band(Style::new().fg(th.label), sel, th)),
        Span::styled(
            format!("{name} "),
            band(Style::new().fg(th.value).add_modifier(Modifier::BOLD), sel, th),
        ),
        Span::styled(g.plain.horizontal_top.repeat(rule), band(Style::new().fg(th.faint), sel, th)),
    ];
    spans.extend(counts);
    spans.push(Span::styled("  ", band(Style::new(), sel, th)));
    Line::from(spans)
}

fn list_board(f: &mut Frame, area: Rect, v: &View, nav: &Nav) {
    let th = v.th;
    let [head, body, foot] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(area);
    let gs = groups(v.panes);
    header(f, head, v, &grid(area, 0), v.panes.len(), Some(gs.len()));

    let rows = rows(v.panes, &nav.folds);
    let room = list_room(area).max(1);
    let r = nav.find(&rows, v.panes);
    // the same clamp Nav applies; a resize can land between a key and a draw
    let mut top = nav.top.min(rows.len().saturating_sub(room));
    if r < top {
        top = r;
    } else if r >= top + room {
        top = r + 1 - room;
    }
    // a confirm holds the rows still, and the title says so (D18)
    let title = if v.confirm.is_some() {
        format!(" sessions {} held ", th.g.dot)
    } else {
        " sessions ".to_string()
    };
    let mut block = Block::bordered()
        .border_set(th.g.plain)
        .border_style(Style::new().fg(th.faint))
        .title(Line::from(Span::styled(title, Style::new().fg(th.label))));
    let last = rows.len().min(top + room);
    if top > 0 || last < rows.len() {
        // where the list is, when it does not fit (D19, P10)
        let n = format!(" {}-{last} of {} ", top + 1, rows.len());
        block = block.title(Line::from(Span::styled(n, Style::new().fg(th.value))).right_aligned());
    }
    let inner = block.inner(body);
    f.render_widget(block, body);

    let w = inner.width as usize;
    let ww = win_w(v.panes);
    let mut digit = 0;
    let lines: Vec<Line> = rows[top..last]
        .iter()
        .enumerate()
        .map(|(k, row)| {
            let sel = top + k == r;
            match row {
                Row::Head(s) => {
                    let members = gs.iter().find(|(n, _)| n == s).map(|(_, m)| m.as_slice()).unwrap_or(&[]);
                    head_row(s, members, v.panes, nav.folds.contains(s), sel, w, th)
                }
                Row::Agent(i) => {
                    // the first nine agent rows on screen carry a digit (D6)
                    digit += 1;
                    let num = (digit <= 9).then_some(digit);
                    let p = &v.panes[*i];
                    agent_row(p, num, sel, p.id == v.active, ww, w, th)
                }
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
    footer(f, foot, v, nav.focus(v.panes));
}

// ------------------------------------------------ the return trip (D9, D20)

/// A return-trip note that reports success stays this long (D9).
pub const NOTE_TTL: Duration = Duration::from_secs(5);
/// D8's note on `a`, unchanged by D20.
const SHORT_NOTE: Duration = Duration::from_secs(2);

/// A footer note. `ttl: None` stays until a key.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    pub at: Instant,
    pub ttl: Option<Duration>,
}

/// The wrapper starts a failure or a refusal with `✕`; the board keys on that (D20).
pub fn failure(s: &str) -> bool {
    s.starts_with('✕')
}

impl Note {
    pub fn timed(text: String, ttl: Duration, now: Instant) -> Self {
        Self {
            text,
            at: now,
            ttl: Some(ttl),
        }
    }

    /// A line `--note` handed over: 5 s, or until a key when it is a failure.
    pub fn returned(text: String, now: Instant) -> Self {
        let ttl = (!failure(&text)).then_some(NOTE_TTL);
        Self { text, at: now, ttl }
    }

    pub fn gone(&self, now: Instant) -> bool {
        self.ttl.is_some_and(|t| now.saturating_duration_since(self.at) >= t)
    }
}

/// A key pressed under a note: `(the note goes, the key still runs)`. A sticky
/// note goes on any key, which also does its job, except `esc` (D20).
pub fn note_key(n: &Note, code: KeyCode) -> (bool, bool) {
    if n.ttl.is_some() {
        return (false, true);
    }
    (true, code != KeyCode::Esc)
}

/// `cockpit-tui board [--select %N] [--note <file>]`, each at most once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opts {
    pub select: Option<String>,
    pub note: Option<std::path::PathBuf>,
}

pub fn is_pane_id(s: &str) -> bool {
    s.strip_prefix('%')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

pub fn opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().ok_or(format!("{a} needs a value"))?;
        match a.as_str() {
            "--select" if o.select.is_none() && is_pane_id(v) => o.select = Some(v.clone()),
            "--select" => return Err(format!("--select takes one pane id like %3, not [{v}]")),
            "--note" if o.note.is_none() && !v.is_empty() => o.note = Some(v.into()),
            "--note" => return Err("--note takes one file".into()),
            _ => return Err(format!("board takes only --select %N and --note <file>, not [{a}]")),
        }
    }
    Ok(o)
}

/// D5 on open, as a position in `order()`: the pane asked for, else the first
/// `▲`, else the client's pane, else the first row.
pub fn open_sel(ps: &[Pane], ord: &[usize], want: Option<&str>, active: &str) -> usize {
    let at = |f: &dyn Fn(&Pane) -> bool| ord.iter().position(|&i| f(&ps[i]));
    want.and_then(|w| at(&|p| p.id == w))
        .or_else(|| at(&|p| p.state == State::NeedsInput))
        .or_else(|| at(&|p| p.id == active))
        .unwrap_or(0)
}

pub const STATE_HEAD: &str = "cockpit-board-state 1";
pub const STATE_MAX: u64 = 64 * 1024;
/// One line of the wrapper's; far over any message it writes.
pub const NOTE_MAX: u64 = 4 * 1024;

/// What `board.state` keeps: the selection, then one `fold <session>` line per
/// folded session. Step 4 appends its `filter` line to the same version.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Saved {
    pub sel: Option<String>,
    pub folds: BTreeSet<String>,
}

/// `None` with no usable HOME: a relative path would land in the cwd.
pub fn state_path() -> Option<std::path::PathBuf> {
    let p = crate::panel::xdg("XDG_STATE_HOME", ".local/state").join("tmux-cockpit/board.state");
    p.is_absolute().then_some(p)
}

pub fn state_text(s: &Saved) -> String {
    let mut b = format!("{STATE_HEAD}\n");
    if let Some(p) = &s.sel {
        b.push_str(&format!("sel {p}\n"));
    }
    for f in &s.folds {
        b.push_str(&format!("fold {f}\n"));
    }
    b
}

/// A session name as a `fold` line holds it: one line, no control character.
fn fold_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}

/// Strict: the head, then known `key value` lines, each in its place.
pub fn parse_state(body: &str) -> Result<Saved, String> {
    let body = body.strip_suffix('\n').ok_or("cut short")?;
    let mut lines = body.split('\n');
    if lines.next() != Some(STATE_HEAD) {
        return Err("not a version 1 state file".into());
    }
    let mut s = Saved::default();
    for l in lines {
        match l.split_once(' ') {
            Some(("sel", p)) if s.sel.is_none() && s.folds.is_empty() && is_pane_id(p) => s.sel = Some(p.to_string()),
            Some(("fold", n)) if fold_name(n) && s.folds.insert(n.to_string()) => {}
            _ => return Err(format!("a bad line [{}]", l.chars().take(40).collect::<String>())),
        }
    }
    Ok(s)
}

/// The panel's state-file rules: no symlink, a small regular file, checked
/// before the open (a FIFO would block it) and again on the open file.
fn read_small(path: &std::path::Path, max: u64) -> Result<String, String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let small = |m: std::fs::Metadata| m.is_file() && m.len() <= max;
    if !std::fs::symlink_metadata(path).is_ok_and(small) {
        return Err("not a small regular file".into());
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !f.metadata().is_ok_and(small) {
        return Err("not a small regular file".into());
    }
    let mut body = String::new();
    f.take(max).read_to_string(&mut body).map_err(|e| e.to_string())?;
    Ok(body)
}

/// `Ok(None)`: no file yet, which is a fresh board and no note.
pub fn read_state(path: &std::path::Path) -> Result<Option<Saved>, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        _ => {}
    }
    parse_state(&read_small(path, STATE_MAX)?).map(Some)
}

/// Created new beside it with `O_NOFOLLOW`, mode 0600, then renamed over it:
/// a rename replaces a symlink rather than following it.
pub fn write_state(path: &std::path::Path, s: &Saved) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Some(d) = path.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(d)?;
    }
    let tmp = path.with_extension(format!("state.{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(&tmp)?;
    let r = f.write_all(state_text(s).as_bytes()).and_then(|_| std::fs::rename(&tmp, path));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// The note file's one line, control characters out; `Ok(None)` when empty.
pub fn read_note(path: &std::path::Path) -> Result<Option<String>, String> {
    let body = read_small(path, NOTE_MAX)?;
    let line = match body.split_once('\n') {
        Some((l, "")) => l,
        Some(_) => return Err("more than one line".into()),
        None if body.is_empty() => "",
        None => return Err("cut short".into()),
    };
    let line: String = line.chars().filter(|c| !c.is_control()).collect();
    let line = line.trim();
    Ok((!line.is_empty()).then(|| line.to_string()))
}

/// The tile a pane id sits on, `0` when it is gone.
fn tile_of(ps: &[Pane], id: &str) -> usize {
    order(ps).iter().position(|&i| ps[i].id == id).unwrap_or(0)
}

/// The list selection as a pane id: a header gives its first agent, which is
/// what `sel %N` can hold. `ps` is never empty here.
fn sel_id(ps: &[Pane], nav: &Nav) -> String {
    if let Some(p) = nav.pane(ps) {
        return p.id.clone();
    }
    let lo = list_order(ps);
    let first = match &nav.sel {
        Sel::Head(s) => lo.iter().find(|&&i| session(&ps[i]) == s),
        Sel::Pane(_) => None,
    };
    ps[*first.unwrap_or(&lo[0])].id.clone()
}

// #228: the one place `i` opens a view. `None` keeps the board open: the info
// screen is drawn in-process, over the selected tile (#252).
fn info(_id: &str) -> Option<String> {
    None
}

// ------------------------------------------------ extra actions and the confirm (D8, D18)

/// One board action beyond the key table: a key on an agent that exits with
/// `<verb> %N` for the wrapper to run. Never on a list header.
pub struct Extra {
    pub key: char,
    /// the footer's verb, and `y`'s on the confirm
    pub label: &'static str,
    /// the `?` help row
    pub help: &'static str,
    /// the action line's verb, `[a-z]+` as `cockpit-board` checks it
    pub verb: &'static str,
    /// the question naming the target; `None` writes the action at once
    pub confirm: Option<fn(&Pane) -> String>,
    /// `$COCKPIT_ACTION.expect`, for the wrapper to check the target against
    pub expect: Option<fn(&Pane) -> String>,
}

// #228: the board's extra actions. A key here must not clash with KEYS, D6's
// keys, D7's `/` or D8's `a`; the wrapper's arms live under `# #228 verbs` in
// scripts/cockpit-board, which ends the loop on a verb it does not know.
pub static EXTRAS: &[Extra] = &[
    Extra {
        key: 'x',
        label: "kill",
        help: "kill its window, after a confirm",
        verb: "kill",
        confirm: Some(|p| format!("kill window {} · {}?", p.wher, p.name)),
        // the id, never the name: agent windows tend to share one (`claude`),
        // and a pane moved into a namesake would pass a name check
        expect: Some(|p| if is_window_id(&p.win) { p.win.clone() } else { String::new() }),
    },
    Extra {
        key: 'R',
        label: "request",
        help: "review it and hand the review to its agent",
        verb: "request",
        confirm: None,
        expect: None,
    },
];

pub fn extra(key: char) -> Option<&'static Extra> {
    EXTRAS.iter().find(|e| e.key == key)
}

/// A tmux window id as `#{window_id}` prints it: `@` and digits.
pub fn is_window_id(s: &str) -> bool {
    s.strip_prefix('@')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The `✕` note when `e` cannot be offered on `p`: its `.expect` would be
/// empty, and an empty value checks nothing. `None` when it can go ahead.
pub fn refused(e: &Extra, p: &Pane) -> Option<String> {
    let f = e.expect?;
    f(p).trim()
        .is_empty()
        .then(|| format!("✕ nothing done: {} has no window id to check, {} not sent", p.wher, e.verb))
}

/// What the board exits with: the action line and, when the action names one,
/// the value `$COCKPIT_ACTION.expect` carries.
#[derive(Clone, Debug, PartialEq)]
pub struct Exit {
    pub line: String,
    pub expect: Option<String>,
}

/// A confirm that is open: the action it writes on `y` and what it says.
#[derive(Clone, Debug, PartialEq)]
pub struct Confirm {
    /// the target, `%N`
    pub id: String,
    pub verb: &'static str,
    /// `y`'s verb in the footer
    pub yes: &'static str,
    /// the frame's title: the action and the target as the board names it
    pub title: String,
    pub ask: String,
    pub expect: Option<String>,
}

impl Confirm {
    /// `None` for an action that asks nothing.
    pub fn open(e: &Extra, p: &Pane) -> Option<Self> {
        Some(Self {
            id: p.id.clone(),
            verb: e.verb,
            yes: e.label,
            title: format!("{} · {} · {}", e.label, p.wher, p.label()),
            ask: (e.confirm?)(p),
            expect: e.expect.map(|f| f(p)),
        })
    }

    pub fn exit(&self) -> Exit {
        Exit {
            line: format!("{} {}", self.verb, self.id),
            expect: self.expect.clone(),
        }
    }
}

/// An action with no confirm, written as soon as its key is pressed.
pub fn at_once(e: &Extra, p: &Pane) -> Exit {
    Exit {
        line: format!("{} {}", e.verb, p.id),
        expect: e.expect.map(|f| f(p)),
    }
}

/// What a key does on the confirm. It owns the keyboard: any other key is
/// `None` and does nothing, `↵` included, so a held Enter cannot send (D8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    Back,
    Help,
    Close,
}

pub fn confirm_key(k: KeyEvent) -> Option<Answer> {
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        return matches!(k.code, KeyCode::Char('c')).then_some(Answer::Close);
    }
    match k.code {
        KeyCode::Char('y') => Some(Answer::Yes),
        KeyCode::Char('n') | KeyCode::Esc => Some(Answer::Back),
        KeyCode::Char('?') => Some(Answer::Help),
        KeyCode::Char('q') => Some(Answer::Close),
        _ => None,
    }
}

/// The confirm's footer, those keys only (D19).
pub fn confirm_hints(c: &Confirm) -> Vec<(String, String)> {
    [("y", c.yes), ("n", "back"), ("esc", "back"), ("?", "keys"), ("q", "close")]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into()
}

/// `?` on the confirm: its keys, and nothing it would ignore.
pub fn confirm_help_rows() -> Vec<(String, String)> {
    [
        ("", "on the confirm"),
        ("y", "yes, do it"),
        ("n esc", "back to the board, nothing done"),
        ("?", "this help"),
        ("q ^C", "close the board, nothing done"),
        ("", ""),
        ("", "every other key does nothing here, ↵ included"),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .into()
}

/// D18 on a refresh while the confirm is open: no row moves, folds or goes,
/// so each pane keeps its state and its place and takes the rest of its row
/// (the age, the message) from `new`. A target that is gone closes the confirm:
/// `Err` is the note to show.
pub fn held(c: &Confirm, old: &[Pane], new: &[Pane]) -> Result<Vec<Pane>, String> {
    if !new.iter().any(|p| p.id == c.id) {
        let who = old.iter().find(|p| p.id == c.id).map_or(c.id.as_str(), |p| p.wher.as_str());
        return Err(format!("✕ nothing done: {who} is gone, {} not sent", c.verb));
    }
    Ok(old
        .iter()
        .map(|o| match new.iter().find(|n| n.id == o.id) {
            Some(n) => Pane {
                state: o.state,
                wher: o.wher.clone(),
                ..n.clone()
            },
            None => o.clone(),
        })
        .collect())
}

/// The confirm, a focus frame over the foot of the body. Must-read text is fg (D17).
fn confirm_card(f: &mut Frame, area: Rect, th: &Theme, c: &Confirm) {
    let g = &th.g;
    let dot = |s: &str| if g.ascii { s.replace('·', g.dot) } else { s.to_string() };
    let w = 62.min(area.width);
    let tw = w.saturating_sub(4) as usize;
    let enter = format!("{} is off here, so a held Enter cannot send.", g.enter);
    let mut lines: Vec<Line> = vec![];
    for (i, para) in [dot(&c.ask), String::new(), enter, "the list holds still while this is open.".into()]
        .iter()
        .enumerate()
    {
        if para.is_empty() {
            lines.push(Line::default());
            continue;
        }
        let st = Style::new().fg(th.value);
        let st = if i == 0 { st.add_modifier(Modifier::BOLD) } else { st };
        lines.extend(wrap(para, tw).into_iter().map(|l| Line::styled(l, st)));
    }
    let h = (lines.len() as u16 + 2).min(area.height);
    // at the foot of the body, over the list's empty rows rather than its top,
    // which is where the selection opens; the frame's bottom edge stays
    let r = area.centered_horizontally(Constraint::Length(w));
    let r = Rect {
        y: area.y + area.height.saturating_sub(h + 1),
        height: h,
        ..r
    };
    let title = g.trunc(&dot(&c.title), tw.saturating_sub(10));
    let block = Block::bordered()
        .border_set(g.thick)
        .border_style(Style::new().fg(th.accent))
        .padding(Padding::horizontal(1))
        .title(Line::from(Span::styled(format!(" {title} "), Style::new().fg(th.value))))
        .title(Line::from(Span::styled(" confirm ", Style::new().fg(th.label))).right_aligned());
    let inner = block.inner(r);
    f.render_widget(Clear, r);
    f.render_widget(block, r);
    f.render_widget(Paragraph::new(lines), inner);
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
    /// the list draws no preview, so it needs no capture
    tiles: bool,
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
        let shown = if ok && want.tiles {
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

pub fn run(o: &Opts) -> Option<Exit> {
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
    // read before the terminal is taken, so a refusal can still reach stderr
    let t0 = Instant::now();
    let mut note = o.note.as_deref().and_then(|f| match read_note(f) {
        Ok(n) => n.map(|n| Note::returned(n, t0)),
        Err(e) => {
            eprintln!("cockpit-tui: cannot read the note {}: {e}", f.display());
            None
        }
    });
    let path = state_path();
    let saved = match path.as_deref().map(read_state) {
        Some(Ok(s)) => s.unwrap_or_default(),
        Some(Err(e)) => {
            eprintln!("cockpit-tui: board.state ignored: {e}");
            note.get_or_insert(Note::timed(format!("board.state ignored ({e}); starting fresh"), NOTE_TTL, t0));
            Saved::default()
        }
        None => Saved::default(),
    };
    // the selection to restore once the first good read names its pane
    let mut want = Some(o.select.clone().or(saved.sel));
    // the list is the default layout; `v` swaps to the tiles (D2)
    let (mut list, mut nav) = (true, Nav::new(Sel::Head(String::new()), saved.folds));
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
    // the info screen over the board (#252), while it is open
    let mut overlay: Option<crate::info::Screen> = None;
    // D16: when the last good read was, and how many have failed since
    let (mut reads, mut stale_shown) = (Reads::new(Instant::now()), None::<u64>);
    let mut last_hash = 0u64;
    let (mut dirty, mut draw_fails) = (true, 0u8);
    let (mut action_line, mut expect) = (None, None::<String>);
    let mut confirm: Option<Confirm> = None;
    let area = |s: ratatui::layout::Size| Rect::new(0, 0, s.width, s.height);

    let (want_tx, want_rx) = mpsc::channel::<Want>();
    let (snap_tx, snap_rx) = mpsc::channel::<Snapshot>();
    let first = Want {
        area: area(term.size().unwrap_or_default()),
        sel,
        top,
        tiles: !list,
    };
    std::thread::spawn(move || worker(want_rx, snap_tx, first));
    // a failed send means the worker is gone, which the snapshot arm notices
    let ask = |a: Rect, sel: usize, top: usize, list: bool| {
        let _ = want_tx.send(Want {
            area: a,
            sel,
            top,
            tiles: !list,
        });
    };

    loop {
        // a push only pulls the next fetch forward, so a missed signal costs
        // one second and never a hang
        if push.pending() {
            ask(area(term.size().unwrap_or_default()), sel, top, list);
        }
        if note.as_ref().is_some_and(|n| n.gone(Instant::now())) {
            note = None;
            dirty = true;
        }
        match snap_rx.try_recv() {
            Ok(s) => {
                let t0 = Instant::now();
                let s = snap_rx.try_iter().last().unwrap_or(s);
                reads.got(s.ok, Instant::now());
                dirty |= !s.ok;
                // D18: a confirm holds the rows still, and closes if its target went
                match confirm.as_ref().map(|c| held(c, &panes, &s.panes)) {
                    None => panes = s.panes,
                    Some(_) if !s.ok => {}
                    Some(Ok(ps)) => panes = ps,
                    Some(Err(msg)) => {
                        confirm = None;
                        note = Some(Note::returned(msg, Instant::now()));
                        panes = s.panes;
                        dirty = true;
                    }
                }
                seen.retain(|id, _| panes.iter().any(|p| &p.id == id));
                seen.extend(s.pvs);
                // never fetched draws as an empty body, not as a failed capture
                pvs = panes
                    .iter()
                    .map(|p| seen.get(&p.id).cloned().unwrap_or(Some(String::new())))
                    .collect();
                let here = area(term.size().unwrap_or_default());
                if s.ok
                    && !panes.is_empty()
                    && let Some(w) = want.take()
                {
                    // D5 reads "the first ▲" in the order on screen: the list's
                    let lo = list_order(&panes);
                    let id = panes[lo[open_sel(&panes, &lo, w.as_deref(), &active)]].id.clone();
                    sel = tile_of(&panes, &id);
                    nav.sel = Sel::Pane(id);
                }
                nav.settle(&panes, list_room(here));
                let was = top;
                (sel, top, _) = fit(here, panes.len(), sel, top);
                // the restored selection can sit below the first fetch's viewport
                if top != was && !list {
                    ask(here, sel, top, list);
                }
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
                note: note.as_ref().map(|n| n.text.as_str()),
                help,
                stale,
                lost: reads.lost(now),
                never: reads.good.is_none(),
                list: list.then_some(&nav),
                confirm: confirm.as_ref(),
            };
            // size() errno can name its tput fallback rather than the terminal,
            // so count failures instead of printing them
            match term.draw(|f| {
                render(f, f.area(), &v);
                if let Some(o) = &overlay {
                    crate::info::render(f, f.area(), &th, o);
                }
            }) {
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
            let here = area(term.size().unwrap_or_default());
            nav.settle(&panes, list_room(here));
            ask(here, sel, top, list);
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
        if let Some(o) = &mut overlay {
            if let crate::info::Out::Close = crate::info::key(o, &th, k.code, here) {
                overlay = None;
            }
            continue;
        }
        if let Some(t) = help {
            // a help screen you have to learn a key to leave is a joke: any
            // key but a scroll key closes it
            let rows = here.height.saturating_sub(2) as usize;
            let n = if confirm.is_some() {
                confirm_help_rows().len()
            } else {
                help_rows(&th.g, list).len()
            };
            help = crate::chrome::scroll(k.code, t, n.saturating_sub(rows), rows);
            continue;
        }
        // the confirm owns the keyboard: every key it does not name does nothing (D18)
        if let Some(c) = &confirm {
            match confirm_key(k) {
                // under the floor the confirm is not on screen to say yes to
                Some(Answer::Yes) if too_small(here) => {}
                Some(Answer::Yes) => {
                    let e = c.exit();
                    (action_line, expect) = (Some(e.line), e.expect);
                    break;
                }
                Some(Answer::Back) => {
                    confirm = None;
                    ask(here, sel, top, list);
                }
                Some(Answer::Help) if !too_small(here) => help = Some(0),
                Some(Answer::Close) => break,
                _ => {}
            }
            continue;
        }
        if !too_small(here)
            && let Some(n) = &note
        {
            let (drop, run) = note_key(n, k.code);
            if drop {
                note = None;
            }
            if !run {
                continue;
            }
        }
        let Some(a) = action(k, list) else { continue };
        // under the floor or with no tiles nobody can see what a key would do
        if too_small(here) || panes.is_empty() {
            match a {
                Act::Quit => break,
                Act::Help if !too_small(here) => help = Some(0),
                Act::Refresh => ask(here, sel, top, list),
                _ => {}
            }
            continue;
        }
        let ord = order(&panes);
        let g = grid(here, panes.len());
        let room = list_room(here);
        // the agent the act keys work on; `None` on a list header, which offers none
        let p = if list { nav.pane(&panes) } else { Some(&panes[ord[sel]]) };
        let id = p.map(|p| p.id.clone()).unwrap_or_default();
        match (a, p) {
            (Act::Quit, _) => break,
            (Act::Help, _) => help = Some(0),
            (Act::Refresh, _) => ask(here, sel, top, list),
            (Act::Layout, _) => {
                // the selection crosses over by identity; a header gives its first agent
                if list {
                    sel = tile_of(&panes, &sel_id(&panes, &nav));
                    (sel, top, _) = fit(here, panes.len(), sel, 0);
                } else {
                    nav.sel = Sel::Pane(id);
                    nav.settle(&panes, room);
                }
                list = !list;
                ask(here, sel, top, list);
            }
            // ↵ on a header toggles its fold (D4)
            (Act::Jump, None) => nav.act(Act::Toggle, &panes, room),
            (Act::Ask | Act::Review | Act::Panel | Act::Info | Act::Extra(_), None) => {}
            (Act::Extra(c), Some(p)) => {
                let Some(e) = extra(c) else { continue };
                if let Some(msg) = refused(e, p) {
                    note = Some(Note::returned(msg, Instant::now()));
                    continue;
                }
                confirm = Confirm::open(e, p);
                if confirm.is_none() {
                    let x = at_once(e, p);
                    (action_line, expect) = (Some(x.line), x.expect);
                    break;
                }
            }
            (Act::Jump, Some(_)) => {
                action_line = Some(format!("jump {id}"));
                break;
            }
            (Act::Review, Some(_)) => {
                action_line = Some(format!("review {id}"));
                break;
            }
            (Act::Panel, Some(_)) => {
                action_line = Some(format!("panel {id}"));
                break;
            }
            (Act::Info, Some(_)) => {
                if let Some(line) = info(&id) {
                    action_line = Some(line);
                    break;
                }
                overlay = Some(crate::info::Screen::open(&id));
            }
            // `a` on a blocked agent jumps; there is no inline reply yet
            (Act::Ask, Some(p)) if p.state == State::NeedsInput => {
                action_line = Some(format!("jump {id}"));
                break;
            }
            (Act::Ask, Some(p)) => {
                let msg = format!(
                    "{} is not waiting on you {} {} jumps there, /btw asks it while it works",
                    p.wher, th.g.dot, th.g.enter
                );
                note = Some(Note::timed(msg, SHORT_NOTE, Instant::now()));
            }
            _ if list => nav.act(a, &panes, room),
            _ => {
                let was = top;
                (sel, top) = moved(a, sel, panes.len(), &g, top, &panes, &ord);
                // newly visible tiles need a capture now, not at the next second
                if top != was {
                    ask(here, sel, top, list);
                }
            }
        }
    }
    ratatui::restore();
    // the selection the next board opens on, this trip's or the next prefix w's
    // a quit before a good read placed the selection would save row 0 over the kept one
    if let (Some(p), false) = (&path, crate::data::readonly() || panes.is_empty() || want.is_some()) {
        let id = if list {
            sel_id(&panes, &nav)
        } else {
            panes[order(&panes)[sel.min(panes.len() - 1)]].id.clone()
        };
        // a fold outlives its session only until the next save
        let folds = nav
            .folds
            .iter()
            .filter(|f| panes.iter().any(|p| session(p) == *f))
            .cloned()
            .collect();
        if let Err(e) = write_state(p, &Saved { sel: Some(id), folds }) {
            eprintln!("cockpit-tui: cannot write {}: {e}", p.display());
        }
    }
    action_line.map(|line| Exit { line, expect })
}
