//! The board: one bordered tile per agent, live preview inside, border colour
//! by state. Not one of the five #54 screens; it is the spike's grid kept
//! because it is the one view the status line cannot approximate.
use crate::data::{Pane, fmt_dur, hash_rows, list_panes, log, preview, trunc};
use crate::theme::Theme;
use ansi_to_tui::IntoText;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};
use std::time::{Duration, Instant};

pub fn cols_for(width: u16) -> usize {
    if width >= 160 {
        3
    } else if width >= 100 {
        2
    } else {
        1
    }
}

pub fn render(f: &mut Frame, area: Rect, panes: &[Pane], pvs: &[String], sel: usize, th: &Theme) {
    let cols = cols_for(area.width);
    let [top, grid] = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).areas(area);
    f.render_widget(
        Line::from(vec![
            Span::styled(format!(" board · {} agents   ", panes.len()), Style::new().fg(th.label)),
            Span::styled("⇥", Style::new().fg(th.accent)),
            Span::styled(" next   ", Style::new().fg(th.meta)),
            Span::styled("↵", Style::new().fg(th.accent)),
            Span::styled(" jump   ", Style::new().fg(th.meta)),
            Span::styled("q", Style::new().fg(th.accent)),
            Span::styled(" close", Style::new().fg(th.meta)),
        ]),
        top,
    );
    if panes.is_empty() {
        return;
    }
    let rows = panes.len().div_ceil(cols);
    let row_areas = Layout::vertical(vec![Constraint::Ratio(1, rows as u32); rows]).split(grid);
    for (i, p) in panes.iter().enumerate() {
        let cells = Layout::horizontal(vec![Constraint::Ratio(1, cols as u32); cols]).split(row_areas[i / cols]);
        let cell = cells[i % cols];
        if cell.height < 3 {
            continue;
        }
        let (g, gc) = th.glyph(&p.state);
        let needs = p.state == "needs-input";
        let mut block = Block::bordered()
            .border_style(Style::new().fg(if i == sel {
                th.accent
            } else if needs {
                th.needs
            } else {
                th.meta
            }))
            .title(Line::from(vec![
                Span::raw(" "),
                Span::styled(g.to_string(), Style::new().fg(gc)),
                Span::styled(format!(" {} · ", p.wher), Style::new().fg(th.meta)),
                Span::styled(format!("{} ", trunc(p.label(), 28)), Style::new().fg(th.value)),
            ]));
        if needs {
            block = block.title_bottom(Line::from(Span::styled(
                format!(" needs you: {} ", trunc(&p.msg, cell.width.saturating_sub(16) as usize)),
                Style::new().fg(th.needs),
            )));
        }
        let inner = block.inner(cell);
        f.render_widget(block, cell);
        if inner.height < 2 {
            continue;
        }
        let [hd, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
        let doing = if needs { format!("needs you: {}", p.msg) } else { p.doing.clone() };
        let age = p.since.map(fmt_dur).unwrap_or_default();
        f.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(vec![
                    Span::styled(
                        trunc(&doing, inner.width.saturating_sub(8) as usize),
                        Style::new().fg(if needs { th.needs } else { th.value }),
                    ),
                    Span::styled(format!("  {age}"), Style::new().fg(th.meta)),
                ]),
                Line::from(Span::styled(trunc(&p.said, inner.width as usize), Style::new().fg(th.quote))),
            ])),
            hd,
        );
        let text = pvs.get(i).map(|s| s.as_bytes().into_text().unwrap_or_default()).unwrap_or_default();
        f.render_widget(Paragraph::new(text), body);
    }
}

pub fn run() -> Option<String> {
    let th = Theme::load();
    let mut term = ratatui::init();
    let (mut panes, mut sel, mut pvs) = (Vec::<Pane>::new(), 0usize, Vec::<String>::new());
    let mut last_hash = 0u64;
    let mut tick = Instant::now() - Duration::from_secs(2);
    let mut dirty = true;
    let mut action = None;
    loop {
        if tick.elapsed() >= Duration::from_secs(1) {
            let t0 = Instant::now();
            let size = term.size().unwrap_or_default();
            panes = list_panes();
            if sel >= panes.len() {
                sel = panes.len().saturating_sub(1);
            }
            let cols = cols_for(size.width);
            let rows = panes.len().div_ceil(cols).max(1);
            let tile_h = (size.height.saturating_sub(1) as usize) / rows;
            pvs = panes.iter().map(|p| preview(&p.id, tile_h.saturating_sub(4).max(2))).collect();
            let h = hash_rows(&panes, &pvs.concat());
            if h != last_hash {
                last_hash = h;
                dirty = true;
            }
            log(&format!("board tick {} tiles {:?} redraw={}", panes.len(), t0.elapsed(), dirty));
            tick = Instant::now();
        }
        if dirty {
            let _ = term.draw(|f| render(f, f.area(), &panes, &pvs, sel, &th));
            dirty = false;
        }
        if event::poll(Duration::from_millis(100)).unwrap_or(false)
            && let Ok(Event::Key(k)) = event::read()
            && k.kind == KeyEventKind::Press
        {
            dirty = true;
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right | KeyCode::Char('j') => {
                    if !panes.is_empty() {
                        sel = (sel + 1) % panes.len();
                    }
                }
                KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('k') => {
                    if !panes.is_empty() {
                        sel = (sel + panes.len() - 1) % panes.len();
                    }
                }
                KeyCode::Enter => {
                    if let Some(p) = panes.get(sel) {
                        action = Some(format!("jump {}", p.id));
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
