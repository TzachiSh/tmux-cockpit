//! The viewer opened from the file browser (`COCKPIT_VIEW_BACK=1`): q and esc
//! go back to the browser, so the footer and the help say `back`, not `close`.
//! No tmux and no tty.
use cockpit_tui::mdview::Screen;
use cockpit_tui::mdview::render::Opts;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

/// Tall enough for the whole help, so its last row (q esc ^C) is drawn.
const AREA: Rect = Rect::new(0, 0, 70, 30);

fn screen(back: bool) -> Screen {
    let mut s = Screen::new("x.md", Ok("# t\n\nbody\n".into()), Opts::default());
    s.back = back;
    s
}

fn draw(s: &mut Screen) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(AREA.width, AREA.height)).unwrap();
    t.draw(|f| cockpit_tui::mdview::render(f, f.area(), &Theme::fixed(), s)).unwrap();
    let b = t.backend().buffer().clone();
    (0..b.area.height)
        .map(|y| (0..b.area.width).map(|x| b[(x, y)].symbol()).collect())
        .collect()
}

fn footer(back: bool) -> String {
    draw(&mut screen(back)).pop().unwrap()
}

/// The help's row for q, esc and ^C, as drawn.
fn help_q_row(back: bool) -> String {
    let mut s = screen(back);
    s.key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE), AREA);
    draw(&mut s).into_iter().find(|l| l.contains("q esc ^C")).unwrap_or_default()
}

#[test]
fn from_the_browser_the_footer_says_q_back() {
    let f = footer(true);
    assert!(f.contains("q back") && !f.contains("close"), "{f}");
}

#[test]
fn elsewhere_the_footer_still_says_q_close() {
    let f = footer(false);
    assert!(f.contains("q close") && !f.contains("back"), "{f}");
}

#[test]
fn from_the_browser_the_help_says_back_for_q_esc_and_ctrl_c() {
    let r = help_q_row(true);
    assert!(r.contains("back") && !r.contains("close"), "{r:?}");
}

#[test]
fn elsewhere_the_help_still_says_close() {
    let r = help_q_row(false);
    assert!(r.contains("close") && !r.contains("back"), "{r:?}");
}
