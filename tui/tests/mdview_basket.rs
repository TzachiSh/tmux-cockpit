//! The viewer's `a` and `y` (#365 child 5): add the selection, or the whole
//! file, to the pane's prompt basket; copy its reference. The viewer only says
//! which; its run loop does the tmux and the clipboard. No tmux and no tty.
use cockpit_tui::mdview::render::Opts;
use cockpit_tui::mdview::{self, Mode, Out, Screen};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

const AREA: Rect = Rect::new(0, 0, 60, 13);

fn code(can_add: bool) -> Screen {
    let src: String = (1..=40).map(|i| format!("l{i}\n")).collect();
    let mut s = Screen::new("x.txt", Ok(src), Opts::default()).with_mode(Mode::Code(None));
    s.can_add = can_add;
    s.layout(mdview::body_width(AREA));
    s
}

fn key(s: &mut Screen, c: KeyCode) -> Out {
    s.key(KeyEvent::new(c, KeyModifiers::NONE), AREA)
}

#[test]
fn a_adds_the_selection() {
    let mut s = code(true);
    for c in "vjj".chars() {
        key(&mut s, KeyCode::Char(c));
    }
    assert_eq!(key(&mut s, KeyCode::Char('a')), Out::Add(Some((1, 3))));
}

#[test]
fn a_with_no_selection_adds_the_whole_file() {
    assert_eq!(key(&mut code(true), KeyCode::Char('a')), Out::Add(None));
}

/// Without a launcher that names the pane there is no basket to add to: `a`
/// says so and stays.
#[test]
fn a_without_a_pane_says_so_and_stays() {
    let mut s = code(false);
    assert_eq!(key(&mut s, KeyCode::Char('a')), Out::Stay);
    assert!(s.note.as_deref().is_some_and(|n| n.contains("pane")), "{:?}", s.note);
}

#[test]
fn y_copies_the_selection_or_the_file() {
    let mut s = code(false);
    assert_eq!(key(&mut s, KeyCode::Char('y')), Out::Copy(None));
    for c in "vj".chars() {
        key(&mut s, KeyCode::Char(c));
    }
    assert_eq!(key(&mut s, KeyCode::Char('y')), Out::Copy(Some((1, 2))));
}

/// An empty or refused file has nothing to add or copy.
#[test]
fn an_empty_file_has_nothing_to_add_or_copy() {
    for k in ['a', 'y'] {
        let mut s = Screen::new("x.txt", Err("a binary file".into()), Opts::default());
        s.can_add = true;
        s.layout(mdview::body_width(AREA));
        assert_eq!(key(&mut s, KeyCode::Char(k)), Out::Stay, "{k}");
        assert!(s.note.is_some(), "{k} said nothing");
    }
}

/// The footer as drawn, its last row.
fn footer(s: &mut Screen) -> String {
    use ratatui::{Terminal, backend::TestBackend};
    let mut t = Terminal::new(TestBackend::new(AREA.width, AREA.height)).unwrap();
    let th = cockpit_tui::theme::Theme::fixed();
    t.draw(|f| mdview::render(f, f.area(), &th, s)).unwrap();
    let buf = t.backend().buffer().clone();
    (0..AREA.width).map(|x| buf[(x, AREA.height - 1)].symbol().to_string()).collect()
}

/// Touch: the footer shows `a add` when there is a basket, and a tap on it adds.
#[test]
fn the_footer_offers_add_and_a_tap_adds() {
    let mut s = code(true);
    let row = footer(&mut s);
    let x = row.find("a add").unwrap_or_else(|| panic!("no `a add` in [{row}]"));
    let x = row[..x].chars().count() as u16 + 1;
    let ev = |kind| MouseEvent {
        kind,
        column: x,
        row: AREA.height - 1,
        modifiers: KeyModifiers::NONE,
    };
    let down = s.mouse(ev(MouseEventKind::Down(MouseButton::Left)), AREA);
    let up = s.mouse(ev(MouseEventKind::Up(MouseButton::Left)), AREA);
    assert!(down == Out::Add(None) || up == Out::Add(None), "{down:?} {up:?}");
    assert!(!footer(&mut code(false)).contains("a add"), "offered with no basket");
}
