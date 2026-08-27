//! The board: a grid of tiles, one per agent pane, each a live capture-pane
//! preview. Sorted needs-first so page one is always the triage page. `render`
//! is pure over `&[Pane]` — it reads no tmux and no clock; `run` is the only
//! impure entry and returns one action line.
use crate::data::{Pane, fmt_dur, hash_rows, list_panes, log, preview};
use crate::push::Push;
use crate::theme::{Glyphs, Theme};
use ansi_to_tui::IntoText;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Padding, Paragraph};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

/// 48 = the narrowest useful tile: two header lines carry the meaning, so it
/// survives half the width crmux needs for a bare pane tail.
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
    pub help: bool,
}

// ------------------------------------------------------------------ ordering

pub fn rank(state: &str) -> u8 {
    match state {
        "needs-input" => 0,
        "error" => 1,
        "running" => 2,
        "done" => 3,
        _ => 4,
    }
}

fn pane_num(id: &str) -> u64 {
    id.trim_start_matches('%').parse().unwrap_or(u64::MAX)
}

/// `(state rank, pane id)`. Never a clock: rank changes are rare and
/// event-driven, ages tick every second and would reorder tiles as you read.
pub fn order(ps: &[Pane]) -> Vec<usize> {
    let mut o: Vec<usize> = (0..ps.len()).collect();
    o.sort_by_key(|&i| (rank(&ps[i].state), pane_num(&ps[i].id)));
    o
}

// -------------------------------------------------------------------- layout

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub rows_shown: usize,
    pub full: bool,
}

/// The density ladder. Tile height is `avail / rows_shown`, never fitted to the
/// tiles that happen to be on screen, so scrolling to a half-empty last row
/// renders a normal tile and blank space.
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

// -------------------------------------------------------------------- render

/// Below this the tiles are narrower than a pane name, so a row would lie about
/// which agent it names. fleet.rs refuses at the same width.
const MIN_BOARD_W: u16 = 40;
/// Header, footer and one tile with a border and a content row.
const MIN_BOARD_H: u16 = 5;

pub fn render(f: &mut Frame, area: Rect, v: &View) {
    // below either floor every tile fails its own size guard and the board is a
    // blank rectangle with nothing saying why
    if area.width < MIN_BOARD_W || area.height < MIN_BOARD_H {
        narrow(f, area, v.th);
        return;
    }
    if v.panes.is_empty() {
        empty(f, area, v.th);
        return;
    }
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
    footer(f, foot, v);
    if v.help {
        help(f, area, v.th);
    }
}

fn header(f: &mut Frame, area: Rect, v: &View, g: &Grid, n: usize) {
    let th = v.th;
    let needs = v.panes.iter().filter(|p| p.state == "needs-input").count();
    let mut left = vec![Span::styled(format!(" board {} {n} agents", th.g.dot), Style::new().fg(th.label))];
    if needs > 0 {
        left.push(Span::styled(format!(" {} {needs} needs you", th.g.dot), Style::new().fg(th.needs)));
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

fn footer(f: &mut Frame, area: Rect, v: &View) {
    let th = v.th;
    if let Some(n) = v.note {
        f.render_widget(Line::from(Span::styled(format!(" {n}"), Style::new().fg(th.meta))), area);
        return;
    }
    let text = footer_text(&th.g, area.width.saturating_sub(1) as usize);
    if text.is_empty() {
        return;
    }
    let mut spans = vec![Span::raw(" ")];
    for (i, pill) in text.split("   ").enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        let (k, verb) = pill.split_once(' ').unwrap_or((pill, ""));
        spans.push(Span::styled(k.to_string(), Style::new().fg(th.accent)));
        spans.push(Span::styled(format!(" {verb}"), Style::new().fg(th.meta)));
    }
    f.render_widget(Line::from(spans), area);
}

/// Pills drop whole, never truncated: half a label is not a smaller button, it
/// is an unreadable one. Widest candidate that measures under the width.
pub fn footer_text(g: &Glyphs, w: usize) -> String {
    let e = g.enter;
    for t in [
        format!("{e} jump   a ask   r review   f fix   i info   ? help   q close"),
        format!("{e} jump   ? help   q close"),
        "? help".to_string(),
    ] {
        if t.width() <= w {
            return t;
        }
    }
    String::new()
}

fn tiles(f: &mut Frame, area: Rect, v: &View, ord: &[usize], g: &Grid) {
    let rows = Layout::vertical(vec![Constraint::Ratio(1, g.rows_shown as u32); g.rows_shown]).split(area);
    for (r, row) in rows.iter().enumerate() {
        let cells = Layout::horizontal(vec![Constraint::Ratio(1, g.cols as u32); g.cols]).split(*row);
        for (c, cell) in cells.iter().enumerate() {
            let k = (v.top + r) * g.cols + c;
            let Some(&i) = ord.get(k) else { continue };
            // the printed number is absolute (ux.md §1.7 labels the last page's
            // lone tile `5`, beside a `5-5 of 5` header) and Act::Tile matches it
            // a short pvs is "not fetched yet" and draws nothing; a stored None
            // is a failed capture and says so
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

fn dimmed(state: &str) -> bool {
    rank(state) >= 3
}

#[allow(clippy::too_many_arguments)]
fn tile(f: &mut Frame, cell: Rect, p: &Pane, num: usize, pv: Option<Option<&str>>, focused: bool, active: bool, full: bool, th: &Theme) {
    // ratatui caches layouts, so the grid is re-split every frame; a tile too
    // small for a border and one content row is skipped rather than drawn torn.
    if cell.width < 8 || cell.height < 3 {
        return;
    }
    let g = &th.g;
    let needs = p.state == "needs-input";
    let (gl, gl_c) = th.glyph(&p.state);
    let (rv, rv_c) = th.review_mark(&p.review);

    let border_c = if needs {
        th.needs
    } else if p.state == "error" {
        th.error
    } else if focused {
        th.accent
    } else {
        th.meta
    };

    let inner_w = cell.width.saturating_sub(2) as usize;
    let n = num.to_string();
    let fixed = 1 + n.width() + 1 + gl.width() + rv.width() + 1;
    let wher = g.trunc(&p.wher, inner_w.saturating_sub(fixed + 1));
    let sep = format!(" {} ", g.dot);
    let title_w = inner_w.saturating_sub(fixed + wher.width() + sep.width() + 1);

    let title_c = if dimmed(&p.state) { th.meta } else { th.value };
    let mut top = vec![
        Span::styled(if active { g.active } else { " " }, Style::new().fg(th.accent)),
        Span::styled(n, Style::new().fg(if focused { th.accent } else { th.meta })),
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

    let mut block = Block::bordered()
        .border_set(if focused { g.thick } else { g.plain })
        .border_style(Style::new().fg(border_c))
        .title(Line::from(top));
    if needs {
        // the ▲, the red border and the red question already say "needs you";
        // the bottom title is where the eye looks for what to do
        block = block.title_bottom(Line::from(Span::styled(format!(" {} answer ", g.enter), Style::new().fg(th.needs))));
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
    if dimmed(&p.state) {
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
                Style::new().fg(if dimmed(&p.state) { th.meta } else { th.value }),
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

// ------------------------------------------------------------------ overlays

fn narrow(f: &mut Frame, area: Rect, th: &Theme) {
    let block = Block::bordered()
        .border_set(th.g.round)
        .border_style(Style::new().fg(th.meta))
        .title(Line::from(Span::styled(" board ", Style::new().fg(th.label))));
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(th.g.trunc("terminal too narrow", w), Style::new().fg(th.needs))),
            Line::from(Span::styled(
                th.g.trunc("prefix i on the pane instead", w),
                Style::new().fg(th.meta),
            )),
        ]),
        inner,
    );
}

fn empty(f: &mut Frame, area: Rect, th: &Theme) {
    let g = &th.g;
    let body = [
        String::new(),
        " no agent panes".into(),
        String::new(),
        " cockpit tracks a pane once an agent runs in it.".into(),
        " start one, or press q and use prefix r for the full fleet.".into(),
        String::new(),
    ];
    let w = 62.min(area.width);
    let r = area.centered(Constraint::Length(w), Constraint::Length(body.len() as u16 + 2));
    let block = Block::bordered()
        .border_set(g.round)
        .border_style(Style::new().fg(th.meta))
        .title(Line::from(Span::styled(" board ", Style::new().fg(th.label))))
        .title_bottom(Line::from(vec![
            Span::styled(" q", Style::new().fg(th.accent)),
            Span::styled(" close ", Style::new().fg(th.meta)),
        ]));
    let inner = block.inner(r);
    f.render_widget(Clear, r);
    f.render_widget(block, r);
    let lines: Vec<Line> = body
        .iter()
        .map(|l| Line::from(Span::styled(g.trunc(l, inner.width as usize), Style::new().fg(th.value))))
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn help(f: &mut Frame, area: Rect, th: &Theme) {
    // the description column is aligned at the widest key in its own column,
    // never per group, so `act` and `view` stay on one grid
    let kw = |gs: &[Grp]| {
        KEYS.iter()
            .filter(|k| gs.contains(&k.grp))
            .map(|k| k.keys.width())
            .max()
            .unwrap_or(0)
    };
    let row = |k: &Key, w: usize| {
        Line::from(vec![
            Span::styled(pad(k.keys.to_string(), w), Style::new().fg(th.accent)),
            Span::styled(format!("  {}", k.desc), Style::new().fg(th.meta)),
        ])
    };
    let head = |t: &'static str| Line::from(Span::styled(t, Style::new().fg(th.label)));

    let (lw_k, rw_k) = (kw(&[Grp::Move]), kw(&[Grp::Act, Grp::View]));
    let mut left = vec![head("move")];
    left.extend(KEYS.iter().filter(|k| k.grp == Grp::Move).map(|k| row(k, lw_k)));
    let mut right = vec![head("act")];
    right.extend(KEYS.iter().filter(|k| k.grp == Grp::Act).map(|k| row(k, rw_k)));
    right.push(Line::default());
    right.push(head("view"));
    right.extend(KEYS.iter().filter(|k| k.grp == Grp::View).map(|k| row(k, rw_k)));

    let lw = left.iter().map(Line::width).max().unwrap_or(0);
    let rw = right.iter().map(Line::width).max().unwrap_or(0);
    let w = (lw + rw + 6).min(area.width as usize) as u16;
    let h = (left.len().max(right.len()) + 2).min(area.height as usize) as u16;
    let r = area.centered(Constraint::Length(w), Constraint::Length(h));

    let block = Block::bordered()
        .border_set(th.g.round)
        .border_style(Style::new().fg(th.meta))
        .padding(Padding::horizontal(1))
        .title(Line::from(Span::styled(" keys ", Style::new().fg(th.label))))
        .title_bottom(Line::from(Span::styled(" any key closes ", Style::new().fg(th.meta))));
    let inner = block.inner(r);
    f.render_widget(Clear, r);
    f.render_widget(block, r);
    let [a, b] = Layout::horizontal([Constraint::Length(lw as u16 + 2), Constraint::Min(0)]).areas(inner);
    f.render_widget(Paragraph::new(left), a);
    f.render_widget(Paragraph::new(right), b);
}

// ------------------------------------------------------------------- the keymap

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
    Fix,
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
}

/// One table feeds `action` and the `?` overlay. A second key list is how a
/// help screen ends up describing a key that was renamed two releases ago.
pub const KEYS: &[Key] = &[
    Key {
        keys: "h l  ← →",
        desc: "column",
        grp: Grp::Move,
        acts: &[Act::Left, Act::Right],
    },
    Key {
        keys: "j k  ↑ ↓",
        desc: "row",
        grp: Grp::Move,
        acts: &[Act::Up, Act::Down],
    },
    Key {
        keys: "⇥ ⇧⇥",
        desc: "next / previous",
        grp: Grp::Move,
        acts: &[Act::Next, Act::Prev],
    },
    Key {
        keys: "1-9",
        desc: "tile by number",
        grp: Grp::Move,
        acts: &[Act::Tile(0)],
    },
    Key {
        keys: "g G",
        desc: "first / last",
        grp: Grp::Move,
        acts: &[Act::First, Act::Last],
    },
    Key {
        keys: "u",
        desc: "next blocked",
        grp: Grp::Move,
        acts: &[Act::NextNeeds],
    },
    Key {
        keys: "[ ]",
        desc: "page",
        grp: Grp::Move,
        acts: &[Act::PageUp, Act::PageDown],
    },
    Key {
        keys: "↵",
        desc: "jump to this pane, close the board",
        grp: Grp::Act,
        acts: &[Act::Jump],
    },
    Key {
        keys: "a",
        desc: "answer / ask this agent",
        grp: Grp::Act,
        acts: &[Act::Ask],
    },
    Key {
        keys: "r",
        desc: "review its diff",
        grp: Grp::Act,
        acts: &[Act::Review],
    },
    Key {
        keys: "f",
        desc: "side fix from it",
        grp: Grp::Act,
        acts: &[Act::Fix],
    },
    Key {
        keys: "i",
        desc: "info for this pane",
        grp: Grp::Act,
        acts: &[Act::Info],
    },
    Key {
        keys: "?",
        desc: "this help",
        grp: Grp::View,
        acts: &[Act::Help],
    },
    Key {
        keys: "^L",
        desc: "refresh now",
        grp: Grp::View,
        acts: &[Act::Refresh],
    },
    Key {
        keys: "q",
        desc: "close   ·   esc close overlay",
        grp: Grp::View,
        acts: &[Act::Quit],
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
        C::Char('f') => Act::Fix,
        C::Char('i') => Act::Info,
        C::Char('?') => Act::Help,
        C::Char('q') | C::Esc => Act::Quit,
        _ => return None,
    })
}

// ---------------------------------------------------------------------- run

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
            .find(|&k| rank(&ps[ord[k]].state) == 0)
            .unwrap_or(sel),
        Act::PageUp => sel.saturating_sub(g.cols * g.rows_shown),
        Act::PageDown => (sel + g.cols * g.rows_shown).min(last),
        _ => sel,
    };
    let top = match a {
        // a literal screenful, stopping on the last row rather than on the last
        // full page: ux.md §1.7 wants `5-5 of 5 ▲4`, one tile at its normal size
        Act::PageUp => top.saturating_sub(g.rows_shown),
        Act::PageDown => (top + g.rows_shown).min(g.rows.saturating_sub(1)),
        _ => top,
    };
    (sel, follow(sel, top, g))
}

/// Pull the viewport to the selection. Also runs on every tick, because the
/// terminal can shrink between frames and leave the focused tile off-screen
/// with nothing to point at it — focus is border weight, not a colour.
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

const DRAW_BUDGET: u8 = 5;

fn transient(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock)
}

pub fn run() -> Option<String> {
    // crossterm builds its event reader on the first poll and caches a failure
    // forever (no controlling terminal, fd exhaustion). Probe before init claims
    // the terminal, or a hook-spawned run paints a TUI that ignores every key.
    if let Err(e) = event::poll(Duration::ZERO) {
        eprintln!("cockpit-tui: no usable terminal: {e}");
        std::process::exit(1);
    }
    let th = Theme::load();
    let active = crate::data::tmux(&["display-message", "-p", "#{pane_id}"])
        .unwrap_or_default()
        .trim()
        .to_string();
    let push = Push::start();
    let mut term = match crate::backend::init() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cockpit-tui: {e}");
            std::process::exit(1);
        }
    };
    let (mut panes, mut pvs) = (Vec::<Pane>::new(), Vec::<Option<String>>::new());
    let (mut sel, mut top, mut help_open) = (0usize, 0usize, false);
    let mut note: Option<(String, Instant)> = None;
    let mut last_hash = 0u64;
    let mut tick = Instant::now().checked_sub(Duration::from_secs(2)).unwrap_or_else(Instant::now);
    let (mut dirty, mut draw_fails) = (true, 0u8);
    let mut action_line = None;

    loop {
        // a push pulls the next tick forward, it never replaces it, so a missed
        // signal costs one second and never a hang
        if push.pending() {
            tick = tick.checked_sub(Duration::from_secs(1)).unwrap_or(tick);
        }
        if note.as_ref().is_some_and(|(_, t)| t.elapsed() >= Duration::from_secs(2)) {
            note = None;
            dirty = true;
        }
        if tick.elapsed() >= Duration::from_secs(1) {
            let t0 = Instant::now();
            let size = term.size().unwrap_or_default();
            // None is "tmux did not answer"; keep the last good tile list rather
            // than blanking the board on one failed read
            if let Some(ps) = list_panes() {
                panes = ps;
            }
            sel = sel.min(panes.len().saturating_sub(1));
            let area = Rect::new(0, 0, size.width, size.height);
            let g = grid(area, panes.len());
            // the same ceiling moved() pages to, or `]` snaps back within a second
            top = follow(sel, top.min(g.rows.saturating_sub(1)), &g);
            let tile_h = (size.height.saturating_sub(2) as usize) / g.rows_shown;
            let ord = order(&panes);
            // visible tiles only: crmux runs one capture-pane exec per tile on
            // every ~50 ms loop tick, forever, whether anything changed or not
            let mut fresh: Vec<Option<String>> = vec![None; panes.len()];
            for k in top * g.cols..panes.len().min((top + g.rows_shown) * g.cols) {
                let rows = tile_h.saturating_sub(if g.full { 4 } else { 3 }).max(1);
                fresh[ord[k]] = preview(&panes[ord[k]].id, rows);
            }
            pvs = fresh;
            let shown: String = pvs.iter().map(|p| p.as_deref().unwrap_or("\u{1}")).collect();
            // hash_rows leaves the age fields out so an idle server never
            // redraws; the rail is rendered text, so hash what it renders
            let rails: String = panes.iter().filter_map(|p| p.since).map(fmt_dur).collect();
            let h = hash_rows(&panes, &(shown + &rails));
            if h != last_hash {
                last_hash = h;
                dirty = true;
            }
            log(&format!("board tick {} tiles {:?} redraw={}", panes.len(), t0.elapsed(), dirty));
            tick = Instant::now();
        }
        if dirty {
            let v = View {
                panes: &panes,
                pvs: &pvs,
                th: &th,
                sel,
                top,
                active: &active,
                note: note.as_ref().map(|(s, _)| s.as_str()),
                help: help_open,
            };
            // size() errors are ENOTTY/ENOENT/EBADF and never Interrupted, and
            // the errno is read after a tput fallback exec, so it names the
            // fallback and not the terminal: count failures instead of printing
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

        let ev = match event::poll(Duration::from_millis(100)) {
            Ok(true) => match event::read() {
                Ok(e) => Some(e),
                Err(e) if transient(&e) => None,
                Err(_) => break,
            },
            Ok(false) => None,
            Err(e) if transient(&e) => None,
            Err(_) => break,
        };
        // a resize is the same request as USR1: look again. It costs nothing to
        // honour, and it means a real terminal resize refreshes the data too.
        if let Some(Event::Resize(..)) = &ev {
            tick = tick.checked_sub(Duration::from_secs(1)).unwrap_or(tick);
            dirty = true;
        }
        let Some(Event::Key(k)) = ev else { continue };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        dirty = true;
        if help_open {
            // a help screen you have to learn a key to leave is a joke
            help_open = false;
            continue;
        }
        let Some(a) = action(k) else { continue };
        if panes.is_empty() {
            if a == Act::Quit {
                break;
            }
            continue;
        }
        let ord = order(&panes);
        let size = term.size().unwrap_or_default();
        let g = grid(Rect::new(0, 0, size.width, size.height), panes.len());
        let p = &panes[ord[sel]];
        let id = p.id.clone();
        match a {
            Act::Quit => break,
            Act::Help => help_open = true,
            Act::Refresh => tick -= Duration::from_secs(1),
            Act::Jump => {
                action_line = Some(format!("jump {id}"));
                break;
            }
            Act::Review => {
                action_line = Some(format!("review {id}"));
                break;
            }
            Act::Fix => {
                action_line = Some(format!("fix {id}"));
                break;
            }
            Act::Info => {
                action_line = Some(format!("info {id}"));
                break;
            }
            // phase 1: `a` on a blocked tile is a jump; the inline reply is L7
            Act::Ask if p.state == "needs-input" => {
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
            _ => (sel, top) = moved(a, sel, panes.len(), &g, top, &panes, &ord),
        }
    }
    ratatui::restore();
    action_line
}
