//! The viewer's line selection and `?` ask (#365 child 4, PR 2): `v`, the moving
//! end, the mouse and two taps, esc's layers, help on `F1` and `h`, and the
//! `ask <a> <b>` the viewer exits with. No tmux and no tty.
use cockpit_tui::mdview::render::Opts;
use cockpit_tui::mdview::{self, Mode, Out, Screen};
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn golden(name: &str, got: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    let path = format!("{dir}/{name}.txt");
    if std::env::var("COCKPIT_BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path}; run COCKPIT_BLESS=1 cargo test"));
    assert_eq!(want, got, "\n{name} changed:\n--- want ---\n{want}\n--- got ---\n{got}");
}

/// 60 by 13: the frame's top border is row 0, the body rows 1 to 10, the footer row 12.
const AREA: Rect = Rect::new(0, 0, 60, 13);

/// 40 numbered lines, `l1` to `l40`, as plain code.
fn code() -> Screen {
    let src: String = (1..=40).map(|i| format!("l{i}\n")).collect();
    let mut s = Screen::new("x.txt", Ok(src), Opts::default()).with_mode(Mode::Code(None));
    s.can_ask = true;
    s.layout(mdview::body_width(AREA));
    s
}

fn key(s: &mut Screen, c: KeyCode) -> Out {
    s.key(KeyEvent::new(c, KeyModifiers::NONE), AREA)
}

fn keys(s: &mut Screen, cs: &str) {
    for c in cs.chars() {
        key(s, KeyCode::Char(c));
    }
}

fn mouse(s: &mut Screen, kind: MouseEventKind, row: u16, modifiers: KeyModifiers) -> Out {
    s.mouse(
        MouseEvent {
            kind,
            column: 10,
            row,
            modifiers,
        },
        AREA,
    )
}

fn click(s: &mut Screen, row: u16) -> Out {
    mouse(s, MouseEventKind::Down(MouseButton::Left), row, KeyModifiers::NONE)
}

fn draw(s: &mut Screen) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(AREA.width, AREA.height)).unwrap();
    t.draw(|f| mdview::render(f, f.area(), &Theme::fixed(), s)).unwrap();
    let b = t.backend().buffer().clone();
    (0..b.area.height)
        .map(|y| (0..b.area.width).map(|x| b[(x, y)].symbol()).collect())
        .collect()
}

// ---------------------------------------------------------------- the keys

#[test]
fn v_selects_the_keys_move_its_end_and_v_ends_it() {
    let mut s = code();
    key(&mut s, KeyCode::Char('v'));
    assert_eq!(s.selection(), Some((1, 1)), "from the top line");
    keys(&mut s, "jjj");
    assert_eq!(s.selection(), Some((1, 4)));
    assert_eq!(s.top, 0, "the view did not scroll: the end moved");
    key(&mut s, KeyCode::Char('v'));
    keys(&mut s, "jj");
    assert_eq!(s.selection(), Some((1, 4)), "ended: j scrolls again");
    assert_eq!(s.top, 2);
    assert_eq!(key(&mut s, KeyCode::Char('?')), Out::Ask(1, 4));
}

#[test]
fn an_end_above_the_anchor_still_reads_low_to_high() {
    let mut s = Screen::new("x.txt", Ok((1..=40).map(|i| format!("l{i}\n")).collect()), Opts::default())
        .with_mode(Mode::Code(None))
        .at_line(10);
    s.can_ask = true;
    s.layout(mdview::body_width(AREA));
    keys(&mut s, "vkk");
    assert_eq!(s.selection(), Some((8, 10)));
    assert_eq!(key(&mut s, KeyCode::Char('?')), Out::Ask(8, 10));
}

/// The view follows the moving end, past the screen and back.
#[test]
fn the_view_follows_the_end() {
    let mut s = code();
    key(&mut s, KeyCode::Char('v'));
    for _ in 0..15 {
        key(&mut s, KeyCode::Down);
    }
    let room = mdview::body_rows(AREA);
    assert_eq!(s.selection(), Some((1, 16)));
    assert!(s.top + room > 15 && s.top <= 15, "line 16's row on screen, top {}", s.top);
    key(&mut s, KeyCode::Char('G'));
    assert_eq!(s.selection(), Some((1, 40)));
    key(&mut s, KeyCode::PageUp);
    assert_eq!(s.selection(), Some((1, 40 - (room - 1))));
    key(&mut s, KeyCode::Char('g'));
    assert_eq!((s.selection(), s.top), (Some((1, 1)), 0));
}

/// `?` with nothing selected asks about the top line.
#[test]
fn a_bare_question_mark_asks_about_one_line() {
    let mut s = code();
    keys(&mut s, "jjj");
    assert_eq!(key(&mut s, KeyCode::Char('?')), Out::Ask(4, 4));
}

#[test]
fn help_is_on_f1_and_h() {
    let mut s = code();
    key(&mut s, KeyCode::Char('h'));
    assert_eq!(s.help, Some(0));
    key(&mut s, KeyCode::Char('q'));
    key(&mut s, KeyCode::F(1));
    assert_eq!(s.help, Some(0));
    let rows = mdview::help_for(false, Mode::Code(None));
    assert!(rows.iter().any(|(k, v)| k == "?" && v.contains("ask")), "{rows:?}");
    assert!(rows.iter().any(|(k, _)| k == "h F1"), "{rows:?}");
}

/// esc takes one layer at a time: the selection, then the matches, then the viewer.
#[test]
fn esc_clears_the_selection_then_the_matches_then_closes() {
    let mut s = code();
    key(&mut s, KeyCode::Char('/'));
    keys(&mut s, "l3");
    key(&mut s, KeyCode::Enter);
    assert!(!s.hits.is_empty());
    keys(&mut s, "vj");
    assert_eq!(key(&mut s, KeyCode::Esc), Out::Stay);
    assert_eq!((s.sel, s.hits.is_empty()), (None, false));
    assert_eq!(key(&mut s, KeyCode::Esc), Out::Stay);
    assert!(s.hits.is_empty());
    assert_eq!(key(&mut s, KeyCode::Esc), Out::Close);
}

/// `n` while selecting moves the end to the next match: "select down to here".
#[test]
fn n_moves_the_end_to_a_match() {
    let mut s = code();
    key(&mut s, KeyCode::Char('/'));
    keys(&mut s, "l30");
    key(&mut s, KeyCode::Enter);
    key(&mut s, KeyCode::Char('g'));
    keys(&mut s, "vn");
    assert_eq!(s.selection(), Some((1, 30)));
}

#[test]
fn e_edits_at_the_selections_end() {
    let mut s = code();
    keys(&mut s, "vG");
    assert_eq!(key(&mut s, KeyCode::Char('e')), Out::Edit(40), "the end, not the top row's line");
}

/// A markdown block is one source line to the rows: an ask takes the whole of
/// the last block, up to the line before the next one, blank lines left out.
#[test]
fn a_markdown_selection_covers_its_blocks_lines() {
    let src = "para one\nstill one\n\n\npara two\nmore\n";
    let mut s = Screen::new("x.md", Ok(src.into()), Opts::default());
    s.can_ask = true;
    s.layout(mdview::body_width(AREA));
    key(&mut s, KeyCode::Char('v'));
    assert_eq!(key(&mut s, KeyCode::Char('?')), Out::Ask(1, 2));
    let mut s = Screen::new("x.md", Ok(src.into()), Opts::default());
    s.can_ask = true;
    s.layout(mdview::body_width(AREA));
    keys(&mut s, "vj");
    assert_eq!(key(&mut s, KeyCode::Char('?')), Out::Ask(1, 6), "to the end of the file");
}

/// Under the floor nothing is drawn to select or ask about.
#[test]
fn under_the_floor_v_and_the_question_mark_do_nothing() {
    let mut s = code();
    let tiny = Rect::new(0, 0, 20, 5);
    for c in ['v', '?'] {
        assert_eq!(s.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), tiny), Out::Stay);
    }
    assert_eq!(s.sel, None);
}

// ---------------------------------------------------------------- the mouse and taps

/// A tap drops the anchor, a second tap ends the range: the phone's way.
#[test]
fn two_taps_make_a_range() {
    let mut s = code();
    click(&mut s, 3);
    assert_eq!(s.selection(), Some((3, 3)));
    click(&mut s, 6);
    assert_eq!(s.selection(), Some((3, 6)));
    assert!(!s.sel.unwrap().live);
    click(&mut s, 2);
    assert_eq!(s.selection(), Some((2, 2)), "a third tap starts again");
}

#[test]
fn a_drag_selects_and_a_shift_click_extends() {
    let mut s = code();
    click(&mut s, 2);
    mouse(&mut s, MouseEventKind::Drag(MouseButton::Left), 5, KeyModifiers::NONE);
    mouse(&mut s, MouseEventKind::Up(MouseButton::Left), 5, KeyModifiers::NONE);
    assert_eq!(s.selection(), Some((2, 5)));
    assert!(!s.sel.unwrap().live, "the drag ended it");
    mouse(&mut s, MouseEventKind::Down(MouseButton::Left), 8, KeyModifiers::SHIFT);
    assert_eq!(s.selection(), Some((2, 8)));
}

/// A tap on the frame or the footer's empty part selects nothing.
#[test]
fn a_tap_outside_the_rows_selects_nothing() {
    let mut s = code();
    click(&mut s, 0);
    click(&mut s, 11);
    assert_eq!(s.sel, None);
}

/// The footer's `? ask` is a button: a tap on it asks.
#[test]
fn the_footers_ask_is_tappable() {
    let mut s = code();
    keys(&mut s, "vjv");
    let foot = draw(&mut s)[12].clone();
    let at = foot.find("? ask").unwrap_or_else(|| panic!("{foot}"));
    let col = foot[..at].chars().count() as u16;
    let out = s.mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col + 2,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        AREA,
    );
    assert_eq!(out, Out::Ask(1, 2));
    // the chips line up with the drawing: a tap on `esc`'s first column clears,
    // and one on the gap before it does nothing
    let tap = |s: &mut Screen, column: u16| {
        s.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row: 12,
                modifiers: KeyModifiers::NONE,
            },
            AREA,
        )
    };
    let esc = foot[..foot.find("esc clear").unwrap()].chars().count() as u16;
    assert_eq!(tap(&mut s, esc - 1), Out::Stay);
    assert!(s.sel.is_some(), "the gap is no chip");
    tap(&mut s, esc);
    assert_eq!(s.sel, None, "esc's chip cleared the selection");
}

// ---------------------------------------------------------------- the drawing

#[test]
fn the_selected_rows_carry_a_bar_and_the_footer_says_how_many() {
    let mut s = code();
    keys(&mut s, "vjj");
    let rows = draw(&mut s);
    let bar = Theme::fixed().g.active;
    for (y, want) in [(1, true), (2, true), (3, true), (4, false)] {
        assert_eq!(
            rows[y].chars().nth(1).map(String::from).as_deref() == Some(bar),
            want,
            "row {y}: {}",
            rows[y]
        );
    }
    assert!(rows[12].contains("1-3 · 3 lines"), "{}", rows[12]);
    assert!(rows[12].contains("? ask"), "{}", rows[12]);
}

/// A selection on the sample, as drawn: the bar, the footer's lines and hints.
#[test]
fn a_selection_golden() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/view-sample.rs")).unwrap();
    let mut s = Screen::new("view-sample.rs", Ok(src), Opts::default()).with_mode(Mode::Code(Some(cockpit_tui::highlight::Lang::Rust)));
    let a = Rect::new(0, 0, 80, 16);
    let mut t = Terminal::new(TestBackend::new(a.width, a.height)).unwrap();
    t.draw(|f| mdview::render(f, f.area(), &Theme::fixed(), &mut s)).unwrap();
    for _ in 0..5 {
        s.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), a);
    }
    for c in ['v', 'j', 'j', 'j', 'j', 'j', 'j'] {
        s.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), a);
    }
    t.draw(|f| mdview::render(f, f.area(), &Theme::fixed(), &mut s)).unwrap();
    golden("view-select-80", &t.backend().to_string());
}

/// A launcher that does not route `ask` gets a note, not a closed viewer: `?`
/// was the help until now, and a line nothing reads would lose the screen.
#[test]
fn without_the_launchers_word_the_question_mark_stays() {
    let mut s = code();
    s.can_ask = false;
    assert_eq!(key(&mut s, KeyCode::Char('?')), Out::Stay);
    assert!(s.note.as_deref().is_some_and(|n| n.contains("h shows the keys")), "{:?}", s.note);
}
