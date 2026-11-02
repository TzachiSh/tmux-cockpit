//! The chrome the board and the panel share (D14): the footer's hints, the
//! full-popup `?` help and the floor card. Callers bring their own key tables.
use crate::panel::view::wrap;
use crate::theme::Theme;
use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

/// The way out and the way to the help: never dropped while anything else can be.
fn kept(k: &str) -> bool {
    k.starts_with("esc") || matches!(k, "?" | "F1" | "q")
}

/// Drawn as ` k v` hints `gap` apart with a space at each end.
fn width(ps: &[(String, String)], gap: usize) -> usize {
    let n = ps.len();
    2 + ps.iter().map(|(k, v)| k.width() + 1 + v.width()).sum::<usize>() + gap * n.saturating_sub(1)
}

/// The hints that fit `w`, whole, lowest priority (rightmost) dropped first:
/// half a hint is unreadable. The help key goes before the way out.
pub fn fit(mut ps: Vec<(String, String)>, w: usize, gap: usize) -> Vec<(String, String)> {
    while width(&ps, gap) > w {
        let Some(i) = ps
            .iter()
            .rposition(|(k, _)| !kept(k))
            .or_else(|| ps.iter().position(|(k, _)| !k.starts_with("esc") && k != "q"))
        else {
            break;
        };
        ps.remove(i);
    }
    ps
}

/// One footer line. The verb is the only copy of what the key does, so it is
/// never in `meta` (D17).
pub fn footer(th: &Theme, ps: Vec<(String, String)>, w: usize, gap: usize) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, (k, v)) in fit(ps, w, gap).into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(gap - 1)));
        }
        spans.push(Span::styled(k, Style::new().fg(th.accent)));
        spans.push(Span::styled(format!(" {v} "), Style::new().fg(th.value)));
    }
    Line::from(spans)
}

/// The help's offset after a scroll key, or `None` for any other key, which
/// closes it. `bottom` is the last offset, `room` the rows on screen.
pub fn scroll(code: KeyCode, top: usize, bottom: usize, room: usize) -> Option<usize> {
    let top = top.min(bottom);
    if room == 0 {
        // no rows on screen: an offset that moved would show nothing
        return matches!(
            code,
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Char('j' | 'k')
        )
        .then_some(top);
    }
    let page = room.saturating_sub(1).max(1);
    let t = match code {
        KeyCode::Up | KeyCode::Char('k') => top.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => top + 1,
        KeyCode::PageUp => top.saturating_sub(page),
        KeyCode::PageDown => top + page,
        _ => return None,
    };
    Some(t.min(bottom))
}

/// The `?` help over the whole area, one column: a screen too small for a card
/// would clip the rows it exists to show. A row with an empty key is a heading.
/// Returns the row count and the rows shown, to clamp the scroll by.
pub fn help(f: &mut Frame, area: Rect, th: &Theme, ks: &[(String, String)], top: usize) -> (usize, usize) {
    let (total, rows) = (ks.len(), (area.height as usize).saturating_sub(2));
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
        format!("{} scroll  any other key closes", th.g.key_updown)
    } else {
        "any key closes".into()
    };
    let close = format!(" {} ", th.g.trunc(&close, (area.width as usize).saturating_sub(4)));
    block = block.title_bottom(Line::from(Span::styled(close, Style::new().fg(th.value))));
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
        .iter()
        .skip(top)
        .map(|(k, v)| {
            if k.is_empty() {
                return Line::from(Span::styled(th.g.trunc(v, inner.width as usize), Style::new().fg(th.label)));
            }
            Line::from(vec![
                Span::styled(format!("{k}{}  ", " ".repeat(kw - k.width())), Style::new().fg(th.accent)),
                Span::styled(th.g.trunc(v, room), Style::new().fg(th.value)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
    (total, rows)
}

/// The one card drawn under a screen's floor: what is wrong, then the way out.
pub fn floor(f: &mut Frame, area: Rect, th: &Theme, title: &str, msg: &str, back: &str) {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_set(th.g.round)
        .border_style(Style::new().fg(th.label))
        .title(Line::from(Span::styled(format!(" {title} "), Style::new().fg(th.accent))));
    let inner = block.inner(area);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    let mut lines: Vec<Line> = wrap(msg, w)
        .into_iter()
        .map(|l| Line::styled(l, Style::new().fg(th.needs)))
        .collect();
    lines.extend(wrap(back, w).into_iter().map(|l| Line::styled(l, Style::new().fg(th.value))));
    f.render_widget(Paragraph::new(lines), inner);
}
