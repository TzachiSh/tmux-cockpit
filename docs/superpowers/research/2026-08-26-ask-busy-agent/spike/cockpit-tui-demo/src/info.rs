//! `prefix i`, the info screen. screens.md §1: label field 8, glyph gutter 2,
//! value at column 11, one 5-column right rail carrying an elapsed duration.
use crate::data::{Pane, fmt_dur, home_rel, trunc, trunc_left, wrap};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};

const LABEL: usize = 8;
const GUTTER: usize = 2;
const RAIL: usize = 5;
/// Two spaces minimum before the rail (screens.md §0), which is why the rail
/// budget is W-17 and not the doc's looser W-15.
const RAIL_GAP: usize = 2;

struct Grid<'a> {
    th: &'a Theme,
    w: usize,
    lines: Vec<Line<'static>>,
}

impl<'a> Grid<'a> {
    fn row(&mut self, label: &str, g: Option<(&str, Color)>, value: &str, style: Style, rail: Option<String>) {
        let budget = match rail {
            Some(_) => self.w.saturating_sub(LABEL + GUTTER + RAIL + RAIL_GAP),
            None => self.w.saturating_sub(LABEL + GUTTER),
        };
        let text = trunc(value, budget);
        let used = text.chars().count();
        let mut sp = vec![Span::styled(format!("{label:<LABEL$}"), Style::new().fg(self.th.label))];
        match g {
            Some((s, c)) => sp.push(Span::styled(format!("{s} "), Style::new().fg(c))),
            None => sp.push(Span::raw("  ")),
        }
        sp.push(Span::styled(text, style));
        if let Some(r) = rail {
            sp.push(Span::raw(" ".repeat(budget - used + RAIL_GAP)));
            sp.push(Span::styled(format!("{r:>RAIL$}"), Style::new().fg(self.th.meta)));
        }
        self.lines.push(Line::from(sp));
    }

    fn cont(&mut self, value: &str, style: Style) {
        let text = trunc(value, self.w.saturating_sub(LABEL + GUTTER));
        self.lines.push(Line::from(vec![Span::raw(" ".repeat(LABEL + GUTTER)), Span::styled(text, style)]));
    }

    fn blank(&mut self) {
        self.lines.push(Line::from(""));
    }
}

pub fn render(f: &mut Frame, area: Rect, p: &Pane, th: &Theme) {
    let block = Block::bordered()
        .border_style(Style::new().fg(th.meta))
        .title(Span::styled(format!(" info · {} ", p.id), Style::new().fg(th.label)));
    let inner = block.inner(area);
    let w = inner.width as usize;
    let value = Style::new().fg(th.value);
    let quote = Style::new().fg(th.quote);
    let meta = Style::new().fg(th.meta);
    let mut g = Grid { th, w, lines: vec![] };

    g.row("pane", None, &format!("{}  {} {}  ({})", p.id, p.wher, p.name, p.cmd), value, None);
    g.row("root", None, &trunc_left(&home_rel(&p.root), w.saturating_sub(LABEL + GUTTER)), value, None);
    // cwd only when it differs from root; unconditional printing is a duplicate
    // line in the worktree case
    if !p.root.is_empty() && p.path != p.root {
        g.row("cwd", None, &trunc_left(&home_rel(&p.path), w.saturating_sub(LABEL + GUTTER)), value, None);
    }
    let (gl, gc) = th.glyph(&p.state);
    g.row(
        "state",
        Some((gl, gc)),
        if p.state.is_empty() { "idle" } else { &p.state },
        Style::new().fg(gc),
        p.idle_since.map(fmt_dur),
    );
    if !p.doing.is_empty() {
        g.row("doing", None, &p.doing, value, p.since.map(fmt_dur));
    }
    if !p.said.is_empty() {
        let budget = w.saturating_sub(LABEL + GUTTER);
        let mut it = wrap(&p.said, budget, 2).into_iter();
        g.row("said", None, &it.next().unwrap_or_default(), quote, None);
        if let Some(l2) = it.next() {
            g.cont(&l2, quote);
        }
    }
    if !p.asked.is_empty() {
        g.row("asked", None, &p.asked, quote, p.asked_since.map(fmt_dur));
    }
    let tail = " — answers while it works";
    let head = "/btw <question> in the pane";
    let ask = if head.len() + tail.len() <= w.saturating_sub(LABEL + GUTTER) { format!("{head}{tail}") } else { head.into() };
    g.row("ask it", None, &ask, Style::new().fg(th.accent), None);
    let (rm, rc) = th.review_mark(&p.review);
    g.row(
        "review",
        if p.review.is_empty() { None } else { Some((rm, rc)) },
        if p.review.is_empty() { "—" } else { &p.review },
        value,
        None,
    );
    g.row("hold", None, if p.hold.is_empty() { "—" } else { &p.hold }, value, None);
    g.row("session", None, if p.sess.is_empty() { "—" } else { &p.sess }, value, None);
    g.row("title", None, if p.title.is_empty() { "—" } else { &p.title }, value, None);
    if !p.spool.is_empty() {
        g.row("reviews", None, &trunc_left(&home_rel(&p.spool), w.saturating_sub(LABEL + GUTTER)), value, None);
        g.cont(&format!("{} pinned · {} delivered · ^A removes it", p.pinned, p.delivered), meta);
    }
    g.blank();
    g.lines.push(Line::from(vec![
        Span::styled("↵", Style::new().fg(th.accent)),
        Span::styled(" close", meta),
    ]));

    f.render_widget(Paragraph::new(Text::from(g.lines)).block(block), area);
}

/// The screen has a known line count, so the popup is sized to it rather than
/// to a percentage that clips at 24 rows.
pub fn run(id: &str) {
    let th = Theme::load();
    let p = crate::data::one_pane(id);
    let mut term = ratatui::init();
    let _ = term.draw(|f| render(f, f.area(), &p, &th));
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
    loop {
        if let Ok(Event::Key(k)) = event::read()
            && k.kind == KeyEventKind::Press
            && matches!(k.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q'))
        {
            break;
        }
    }
    ratatui::restore();
}
