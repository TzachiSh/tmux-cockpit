//! `prefix r`, the fleet list. screens.md §2: three width profiles, the
//! collapse order loc -> ± -> where, a 2-cell glyph pair instead of the state
//! word, and needs-input rows whose question replaces everything right of the
//! title.
use crate::data::{Pane, fmt_dur, hash_rows, list_panes, log, preview, trunc, trunc_left, wrap};
use crate::push::Push;
use crate::theme::Theme;
use ansi_to_tui::IntoText;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Cell, Clear, HighlightSpacing, Paragraph, Row, Table, TableState};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The glyph pair: status glyph + review mark.
pub const MARK: u16 = 2;
/// The keyboard wait, and so the ceiling on how late a push wakeup can be.
/// ponytail: 10 ms costs ~0.8 % of a core in idle wakeups (0.1 % at 100 ms).
/// Worth it only while a screen is open. If it ever matters, move accept() to a
/// blocking thread that kill()s our own pid, and this goes back to 100 ms.
const POLL: Duration = Duration::from_millis(10);
/// Below this the row would lie about which agent it names (screens.md §"80 columns").
pub const MIN_R: u16 = 40;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Profile {
    pub name: char,
    pub wher: u16,
    pub title: u16,
    pub doing: u16,
    pub age: u16,
    pub loc: u16,
    pub pm: u16,
}

/// R is the list's inner width. The caps come from the terminal, never from a
/// constant: fixed caps are what made the old row overflow and silently drop ±.
pub fn profile(r: u16) -> Profile {
    if r >= 100 {
        Profile { name: 'C', wher: 11, title: 22, doing: 28, age: 5, loc: 20, pm: 10 }
    } else if r >= 68 {
        Profile { name: 'B', wher: 10, title: 20, doing: 24, age: 5, loc: 0, pm: 10 }
    } else {
        Profile { name: 'A', wher: 0, title: 20, doing: 20, age: 5, loc: 0, pm: 0 }
    }
}

/// Which optional columns a width drops, in drop order. The contract the
/// collapse-order test pins.
pub fn dropped(r: u16) -> Vec<&'static str> {
    let p = profile(r);
    ["loc", "pm", "where"]
        .into_iter()
        .filter(|c| match *c {
            "loc" => p.loc == 0,
            "pm" => p.pm == 0,
            _ => p.wher == 0,
        })
        .collect()
}

pub struct Needs {
    pub id: String,
    pub wher: String,
    pub title: String,
    pub msg: String,
    pub left: u64,
}

pub struct View<'a> {
    pub panes: &'a [Pane],
    pub sel: usize,
    pub th: &'a Theme,
    pub preview: &'a str,
    pub overlay: Option<&'a Needs>,
}

fn counts(ps: &[Pane]) -> String {
    let blocked = ps.iter().filter(|p| p.state == "needs-input").count();
    let review = ps.iter().filter(|p| p.review == "pending").count();
    let mut s = format!(" {} agents", ps.len());
    if blocked > 0 {
        s += &format!(" · {blocked} blocked");
    }
    if review > 0 {
        s += &format!(" · {review} review pending");
    }
    s + " "
}

fn legend(th: &Theme, r: u16) -> Line<'static> {
    let pairs: &[(&str, &str)] = if r < 68 {
        &[("needs-input", "needs"), ("running", "running"), ("done", "done"), ("", "review")]
    } else {
        &[
            ("needs-input", "needs"),
            ("running", "running"),
            ("done", "done"),
            ("idle", "idle"),
            ("", "review pending"),
            ("+", "pasted"),
        ]
    };
    let sep = if r < 68 { " " } else { "  " };
    let mut sp = vec![];
    for (i, (st, word)) in pairs.iter().enumerate() {
        if i > 0 {
            sp.push(Span::raw(sep.to_string()));
        }
        let (g, c) = match *st {
            "" => th.review_mark("pending"),
            "+" => th.review_mark("sent"),
            s => th.glyph(s),
        };
        sp.push(Span::styled(g.to_string(), Style::new().fg(c)));
        sp.push(Span::styled(format!(" {word}"), Style::new().fg(th.meta)));
    }
    Line::from(sp)
}

fn row_height(p: &Pane) -> u16 {
    if p.state == "needs-input" { 2 } else { 1 }
}

/// x of the doing column inside the list, so the needs-input question can be
/// drawn across the columns a Table cell cannot span.
fn doing_x(inner: Rect, pr: &Profile) -> u16 {
    inner.x + MARK + 1 + if pr.wher > 0 { pr.wher + 1 } else { 0 } + pr.title + 1
}

pub fn render(f: &mut Frame, area: Rect, v: &View, st: &mut TableState) {
    let th = v.th;
    let keys = Line::from(vec![
        Span::styled(" ↵", Style::new().fg(th.accent)),
        Span::styled(" jump   ", Style::new().fg(th.meta)),
        Span::styled("r", Style::new().fg(th.accent)),
        Span::styled(" review   ", Style::new().fg(th.meta)),
        Span::styled("f", Style::new().fg(th.accent)),
        Span::styled(" fix   ", Style::new().fg(th.meta)),
        Span::styled("q", Style::new().fg(th.accent)),
        Span::styled(" close ", Style::new().fg(th.meta)),
    ]);
    let r = area.width.saturating_sub(2);
    if r < MIN_R {
        let b = Block::bordered().border_style(Style::new().fg(th.meta)).title(Span::styled(" fleet ", Style::new().fg(th.label)));
        f.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(Span::styled("terminal too narrow", Style::new().fg(th.needs))),
                Line::from(Span::styled("prefix i on the pane instead", Style::new().fg(th.meta))),
            ]))
            .block(b),
            area,
        );
        return;
    }
    let pr = profile(r);

    // list height: header + rows + borders, capped so the preview keeps 3 rows
    let body: u16 = v.panes.iter().map(row_height).sum();
    let list_h = (body + 3).min(area.height.saturating_sub(5)).max(4);
    let [list_a, prev_a] = Layout::vertical([Constraint::Length(list_h), Constraint::Min(0)]).areas(area);

    let mut widths = vec![Constraint::Length(MARK)];
    if pr.wher > 0 {
        widths.push(Constraint::Length(pr.wher));
    }
    widths.push(Constraint::Length(pr.title));
    widths.push(Constraint::Length(pr.doing));
    widths.push(Constraint::Length(pr.age));
    if pr.loc > 0 {
        widths.push(Constraint::Length(pr.loc));
    }
    if pr.pm > 0 {
        widths.push(Constraint::Length(pr.pm));
    }

    let rows: Vec<Row> = v
        .panes
        .iter()
        .map(|p| {
            let needs = p.state == "needs-input";
            let (g, gc) = th.glyph(&p.state);
            let (m, mc) = th.review_mark(&p.review);
            let mut cells = vec![Cell::from(Line::from(vec![
                Span::styled(g.to_string(), Style::new().fg(gc)),
                Span::styled(m.to_string(), Style::new().fg(mc)),
            ]))];
            if pr.wher > 0 {
                cells.push(Cell::from(trunc(&p.wher, pr.wher as usize)).style(Style::new().fg(th.meta)));
            }
            let tstyle = if needs { Style::new().fg(th.needs) } else { Style::new().fg(th.value) };
            cells.push(Cell::from(trunc(p.label(), pr.title as usize)).style(tstyle));
            if needs {
                // the question is painted across these columns after the table
                cells.push(Cell::from(""));
                cells.push(Cell::from(""));
                if pr.loc > 0 {
                    cells.push(Cell::from(""));
                }
                if pr.pm > 0 {
                    cells.push(Cell::from(""));
                }
            } else {
                let doing = if p.doing.is_empty() { "—".to_string() } else { p.doing.clone() };
                cells.push(Cell::from(trunc(&doing, pr.doing as usize)).style(Style::new().fg(th.value)));
                let age = p.since.map(fmt_dur).unwrap_or_default();
                cells.push(Cell::from(Line::from(age).right_aligned()).style(Style::new().fg(th.meta)));
                if pr.loc > 0 {
                    cells.push(Cell::from(trunc(&p.loc, pr.loc as usize)).style(Style::new().fg(th.meta)));
                }
                if pr.pm > 0 {
                    cells.push(Cell::from(p.pm.clone()).style(Style::new().fg(th.meta)));
                }
            }
            Row::new(cells).height(row_height(p))
        })
        .collect();

    let block = Block::bordered()
        .border_style(Style::new().fg(th.meta))
        .title(Span::styled(counts(v.panes), Style::new().fg(th.label)))
        .title_bottom(keys);
    let inner = block.inner(list_a);
    f.render_widget(block, list_a);
    let [legend_a, table_a] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    f.render_widget(legend(th, r), legend_a);
    let table = Table::new(rows, widths)
        .column_spacing(1)
        .row_highlight_style(th.sel())
        .highlight_spacing(HighlightSpacing::Never);
    st.select(Some(v.sel));
    f.render_stateful_widget(table, table_a, st);

    // needs-input questions, drawn over the columns a Table row cannot span
    let qx = doing_x(inner, &pr);
    let qw = inner.right().saturating_sub(qx) as usize;
    let mut y = inner.y + 1;
    for p in v.panes.iter().skip(st.offset()) {
        let h = row_height(p);
        if y + h > inner.bottom() {
            break;
        }
        if p.state == "needs-input" {
            let msg = if p.msg.is_empty() { "waiting for you".to_string() } else { p.msg.clone() };
            let lines = wrap(&trunc(&msg, qw * 2), qw, 2);
            let first = lines.first().cloned().unwrap_or_default();
            f.render_widget(
                Span::styled(first, Style::new().fg(th.needs)),
                Rect { x: qx, y, width: qw as u16, height: 1 },
            );
            if let Some(second) = lines.get(1) {
                f.render_widget(
                    Line::from(vec![
                        Span::styled("↳ ", Style::new().fg(th.meta)),
                        Span::styled(second.clone(), Style::new().fg(th.needs)),
                    ]),
                    Rect { x: qx - 2, y: y + 1, width: qw as u16 + 2, height: 1 },
                );
            }
        }
        y += h;
    }

    // preview; when loc was collapsed its text moves to the preview title
    if prev_a.height >= 3 {
        let sel = v.panes.get(v.sel);
        let head = match sel {
            Some(p) => {
                let mut s = String::new();
                if pr.loc == 0 && !p.loc.is_empty() {
                    s += &format!("{} · {} · ", p.loc, p.pm);
                }
                s += &p.state;
                if let Some(d) = p.since {
                    s += &format!(" {}", fmt_dur(d));
                }
                format!(" {} · {} ", p.id, trunc(&s, prev_a.width.saturating_sub(12) as usize))
            }
            None => " preview ".into(),
        };
        let pb = Block::bordered().border_style(Style::new().fg(th.meta)).title(Span::styled(head, Style::new().fg(th.label)));
        let text = v.preview.as_bytes().into_text().unwrap_or_default();
        f.render_widget(Paragraph::new(text).block(pb), prev_a);
    }

    if let Some(n) = v.overlay {
        render_needs_popup(f, area, th, n);
    }
}

/// screens.md §3's opt-in popup, as an overlay inside the fleet rather than a
/// second display-popup: a Clear'd centred block.
pub fn render_needs_popup(f: &mut Frame, area: Rect, th: &Theme, n: &Needs) {
    let w = 56u16.min(area.width.saturating_sub(2));
    let pop = area.centered(Constraint::Length(w), Constraint::Length(6));
    let (g, gc) = th.glyph("needs-input");
    let inner_w = w.saturating_sub(4) as usize;
    let body = Text::from(vec![
        Line::from(vec![
            Span::styled(format!("{g} "), Style::new().fg(gc)),
            Span::styled(format!("{} ", n.wher), Style::new().fg(th.meta)),
            Span::styled(trunc(&n.title, inner_w.saturating_sub(n.wher.len() + 3)), Style::new().fg(th.value)),
        ]),
        Line::from(Span::styled(trunc(&n.msg, inner_w), Style::new().fg(th.quote))),
        Line::from(""),
        Line::from(vec![
            Span::styled("↵", Style::new().fg(th.accent)),
            Span::styled(" jump   ", Style::new().fg(th.meta)),
            Span::styled("esc", Style::new().fg(th.accent)),
            Span::styled(" dismiss", Style::new().fg(th.meta)),
            Span::styled(format!("{:>w$}", format!("closes in {:2}s", n.left), w = inner_w.saturating_sub(20)), Style::new().fg(th.meta)),
        ]),
    ]);
    let b = Block::bordered()
        .border_style(Style::new().fg(th.needs))
        .padding(ratatui::widgets::Padding::horizontal(1))
        .title(Span::styled(" needs you ", Style::new().fg(th.needs)));
    f.render_widget(Clear, pop);
    f.render_widget(Paragraph::new(body).block(b), pop);
}

/// Returns the action line for the shell wrapper, e.g. "jump %14".
pub fn run(sock: bool) -> Option<String> {
    let th = Theme::load();
    let push = Push::start(sock);
    let mut term = ratatui::init();
    let mut st = TableState::new();
    let (mut panes, mut sel, mut pv) = (Vec::<Pane>::new(), 0usize, String::new());
    let mut seen: HashMap<String, String> = HashMap::new();
    // the popup announces a flip, not a fact: a pane already blocked when the
    // screen opens is on the first row, it does not need a popup over it
    let mut first_tick = true;
    let mut overlay: Option<(Needs, Instant)> = None;
    let mut last_hash = 0u64;
    let mut tick = Instant::now() - Duration::from_secs(2);
    let mut dirty = true;
    let mut action = None;
    loop {
        // a push says "look now": it pulls the next tick forward, it does not
        // replace it, so a missed message costs a second and never a hang
        if push.pending() {
            tick -= Duration::from_secs(1);
        }
        if tick.elapsed() >= Duration::from_secs(1) {
            let t0 = Instant::now();
            let size = term.size().unwrap_or_default();
            panes = list_panes();
            if sel >= panes.len() {
                sel = panes.len().saturating_sub(1);
            }
            let body: u16 = panes.iter().map(row_height).sum();
            let list_h = (body + 3).min(size.height.saturating_sub(5)).max(4);
            pv = panes
                .get(sel)
                .map(|p| preview(&p.id, size.height.saturating_sub(list_h + 2).max(3) as usize))
                .unwrap_or_default();
            // a pane that just flipped to needs-input raises the popup once
            for p in &panes {
                let was = seen.insert(p.id.clone(), p.state.clone());
                if !first_tick && p.state == "needs-input" && was.as_deref() != Some("needs-input") && overlay.is_none() {
                    overlay = Some((
                        Needs {
                            id: p.id.clone(),
                            wher: p.wher.clone(),
                            title: p.label().to_string(),
                            msg: if p.msg.is_empty() { "waiting for you".into() } else { p.msg.clone() },
                            left: 10,
                        },
                        Instant::now(),
                    ));
                }
            }
            if let Some((n, since)) = &mut overlay {
                let e = since.elapsed().as_secs();
                if e >= 10 {
                    overlay = None;
                    dirty = true;
                } else if n.left != 10 - e {
                    n.left = 10 - e;
                    dirty = true;
                }
            }
            first_tick = false;
            let h = hash_rows(&panes, &pv);
            if h != last_hash {
                last_hash = h;
                dirty = true;
            }
            log(&format!("fleet tick {} panes {:?} redraw={}", panes.len(), t0.elapsed(), dirty));
            tick = Instant::now();
        }
        if dirty {
            let v = View { panes: &panes, sel, th: &th, preview: &pv, overlay: overlay.as_ref().map(|(n, _)| n) };
            let _ = term.draw(|f| render(f, f.area(), &v, &mut st));
            log("draw");
            dirty = false;
        }
        if event::poll(POLL).unwrap_or(false)
            && let Ok(Event::Key(k)) = event::read()
            && k.kind == KeyEventKind::Press
        {
            dirty = true;
            if let Some((n, _)) = &overlay {
                match k.code {
                    KeyCode::Enter => {
                        action = Some(format!("jump {}", n.id));
                        break;
                    }
                    KeyCode::Esc => {
                        overlay = None;
                        continue;
                    }
                    _ => {}
                }
            }
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('j') | KeyCode::Down => {
                    if sel + 1 < panes.len() {
                        sel += 1;
                        tick -= Duration::from_secs(1);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if sel > 0 {
                        sel -= 1;
                        tick -= Duration::from_secs(1);
                    }
                }
                KeyCode::Enter | KeyCode::Char('r') | KeyCode::Char('f') => {
                    let verb = match k.code {
                        KeyCode::Char('r') => "review",
                        KeyCode::Char('f') => "fix",
                        _ => "jump",
                    };
                    if let Some(p) = panes.get(sel) {
                        action = Some(format!("{verb} {}", p.id));
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    ratatui::restore();
    action
}

/// `--rows`, the headless tab-separated form the bash suite can diff.
pub fn rows_text(ps: &[Pane], th: &Theme) -> String {
    ps.iter()
        .map(|p| {
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                th.glyph(&p.state).0,
                p.id,
                p.wher,
                p.label(),
                if p.state == "needs-input" { &p.msg } else { &p.doing },
                p.since.map(fmt_dur).unwrap_or_default(),
                p.loc,
                p.pm
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn rows_json(ps: &[Pane]) -> String {
    let v: Vec<serde_json::Value> = ps
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id, "where": p.wher, "state": p.state, "title": p.label(),
                "doing": p.doing, "said": p.said, "asked": p.asked, "msg": p.msg,
                "since": p.since, "loc": p.loc, "diff": p.pm, "review": p.review,
                "session": p.sess, "path": p.path,
            })
        })
        .collect();
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Left-truncated path, used by the info screen and kept here beside trunc.
pub fn path_fit(s: &str, n: usize) -> String {
    trunc_left(s, n)
}
