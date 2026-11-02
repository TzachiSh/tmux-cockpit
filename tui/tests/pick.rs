//! The picker's filter, selection, cancel and goldens. No tmux, no tty, no
//! clock: every input is a literal and the theme is `Theme::fixed()`.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
use cockpit_tui::pick::{self, Opts, Outcome, Pick, Row};
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

fn draw(w: u16, h: u16, p: &Pick) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| pick::render(f, f.area(), p, &Theme::fixed())).unwrap();
    t.backend().to_string()
}

fn area(w: u16, h: u16) -> Rect {
    Rect::new(0, 0, w, h)
}

const BIG: Rect = Rect::new(0, 0, 80, 24);

fn rows(n: &[(&str, &str)]) -> Vec<Row> {
    n.iter()
        .map(|(k, t)| Row {
            key: (*k).into(),
            text: (*t).into(),
        })
        .collect()
}

fn opts(prompt: &str, header: &str) -> Opts {
    Opts {
        prompt: prompt.into(),
        header: header.into(),
        preview: None,
    }
}

const BRANCHES: [(&str, &str); 6] = [
    ("main", "main"),
    ("feat/237-pick", "feat/237-pick  Add a Rust picker"),
    ("feat/board-chrome", "feat/board-chrome  The shared chrome"),
    ("fix/Left-Click", "fix/Left-Click  Status bar click"),
    ("release/0.2", "release/0.2"),
    ("wip/長い名前", "wip/長い名前  wide glyphs"),
];

fn picker(prompt: &str, header: &str, preview: Option<&str>) -> Pick {
    Pick::new(rows(&BRANCHES), &opts(prompt, header), preview.map(Into::into))
}

fn press(p: &mut Pick, code: KeyCode, a: Rect) -> Outcome {
    p.key(KeyEvent::new(code, KeyModifiers::NONE), a)
}

fn type_str(p: &mut Pick, s: &str) {
    for c in s.chars() {
        press(p, KeyCode::Char(c), BIG);
    }
}

fn names(p: &Pick) -> Vec<&str> {
    p.shown.iter().map(|&i| p.rows[i].key.as_str()).collect()
}

const PREVIEW: &str = "branch feat/237-pick\n\n  dea82ca Merge pull request #236\n  3 files changed\n";

// ---------------------------------------------------------------- the rows

#[test]
fn rows_split_at_the_first_tab_and_the_display_keeps_the_rest() {
    let r = pick::parse_rows("k1\tone\ttwo\nk2\tthree\n");
    assert_eq!((r[0].key.as_str(), r[0].text.as_str()), ("k1", "one two"));
    assert_eq!((r[1].key.as_str(), r[1].text.as_str()), ("k2", "three"));
}

#[test]
fn a_row_with_no_tab_is_its_own_key_and_display() {
    let r = pick::parse_rows("plain\n");
    assert_eq!((r[0].key.as_str(), r[0].text.as_str()), ("plain", "plain"));
}

#[test]
fn empty_input_and_blank_lines_give_no_rows() {
    assert!(pick::parse_rows("").is_empty());
    assert!(pick::parse_rows("\n\r\n\n").is_empty());
}

#[test]
fn a_crlf_row_loses_its_carriage_return() {
    let r = pick::parse_rows("k\tshown\r\n");
    assert_eq!((r[0].key.as_str(), r[0].text.as_str()), ("k", "shown"));
}

#[test]
fn control_characters_are_stripped_from_the_display() {
    let r = pick::parse_rows("k\tev\x1b[2Jil\x07\u{9b}x\n");
    assert_eq!(r[0].text, "ev[2Jilx");
    assert_eq!(pick::clean("a\tb\x00c"), "a bc");
}

// ---------------------------------------------------------------- the filter

#[test]
fn the_filter_ignores_case_in_the_row_and_in_the_query() {
    let r = rows(&BRANCHES);
    assert_eq!(pick::filter(&r, "left-click"), vec![3]);
    assert_eq!(pick::filter(&r, "LEFT-CLICK"), vec![3]);
    assert_eq!(pick::filter(&r, "FEAT/"), vec![1, 2]);
}

#[test]
fn the_filter_matches_the_display_and_not_the_key() {
    let r = rows(&[("secret-key", "visible")]);
    assert!(pick::filter(&r, "secret").is_empty());
    assert_eq!(pick::filter(&r, "visi"), vec![0]);
}

#[test]
fn a_query_that_matches_nothing_shows_no_rows() {
    assert!(pick::filter(&rows(&BRANCHES), "zzz").is_empty());
}

#[test]
fn the_empty_query_shows_every_row_in_order() {
    assert_eq!(pick::filter(&rows(&BRANCHES), ""), vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn a_wide_glyph_row_is_found_by_its_glyphs() {
    assert_eq!(pick::filter(&rows(&BRANCHES), "長い"), vec![5]);
}

// ---------------------------------------------------------------- selection

#[test]
fn j_and_k_move_by_one_and_stop_at_both_ends() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Char('k'), BIG);
    assert_eq!(p.sel, 0, "k at the top stays");
    for _ in 0..9 {
        press(&mut p, KeyCode::Char('j'), BIG);
    }
    assert_eq!(p.sel, 5, "j at the bottom stays on the last row");
}

#[test]
fn the_arrows_and_the_pages_move_through_chrome_scroll() {
    let many: Vec<(String, String)> = (0..40).map(|i| (format!("k{i}"), format!("row {i}"))).collect();
    let r: Vec<Row> = many
        .iter()
        .map(|(k, t)| Row {
            key: k.clone(),
            text: t.clone(),
        })
        .collect();
    let mut p = Pick::new(r, &opts("pick", ""), None);
    let a = area(60, 12);
    press(&mut p, KeyCode::Down, a);
    assert_eq!(p.sel, 1);
    press(&mut p, KeyCode::PageDown, a);
    let room = p.sel - 1;
    assert!(room >= 5 && p.sel < 40, "a page is a screenful less one, got {}", p.sel);
    press(&mut p, KeyCode::PageUp, a);
    assert_eq!(p.sel, 1);
    press(&mut p, KeyCode::Up, a);
    assert_eq!(p.sel, 0);
}

#[test]
fn the_selection_stays_inside_the_window_it_scrolls() {
    let r: Vec<Row> = (0..40)
        .map(|i| Row {
            key: format!("k{i}"),
            text: format!("row {i}"),
        })
        .collect();
    let mut p = Pick::new(r, &opts("pick", ""), None);
    let a = area(60, 12);
    for _ in 0..25 {
        press(&mut p, KeyCode::Char('j'), a);
    }
    let room = 12 - 2; // prompt and footer
    assert!(
        p.top <= p.sel && p.sel < p.top + room,
        "sel {} outside top {}..+{room}",
        p.sel,
        p.top
    );
}

#[test]
fn enter_chooses_the_selected_rows_key_not_its_display() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Down, BIG);
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Choose("feat/237-pick".into()));
}

#[test]
fn enter_after_a_filter_chooses_from_the_filtered_list() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Char('/'), BIG);
    type_str(&mut p, "RELEASE");
    assert_eq!(names(&p), vec!["release/0.2"]);
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Choose("release/0.2".into()));
}

#[test]
fn enter_with_no_match_chooses_nothing() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Char('/'), BIG);
    type_str(&mut p, "zzz");
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Continue);
}

#[test]
fn a_new_query_puts_the_selection_back_on_the_first_match() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Char('G'), BIG);
    press(&mut p, KeyCode::Char('/'), BIG);
    type_str(&mut p, "f");
    assert_eq!((p.sel, p.top), (0, 0));
}

#[test]
fn backspace_widens_the_filter_and_on_an_empty_query_leaves_filtering() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Char('/'), BIG);
    type_str(&mut p, "zz");
    assert!(p.shown.is_empty());
    press(&mut p, KeyCode::Backspace, BIG);
    press(&mut p, KeyCode::Backspace, BIG);
    assert_eq!(p.shown.len(), 6);
    press(&mut p, KeyCode::Backspace, BIG);
    assert!(!p.filtering, "j k q are commands again");
}

#[test]
fn typing_a_letter_that_is_not_a_command_starts_the_filter() {
    let mut p = picker("pick", "", None);
    type_str(&mut p, "rel");
    assert!(p.filtering);
    assert_eq!(names(&p), vec!["release/0.2"]);
}

#[test]
fn while_filtering_j_k_and_q_are_letters() {
    let mut p = Pick::new(rows(&[("a", "quick jack")]), &opts("pick", ""), None);
    press(&mut p, KeyCode::Char('/'), BIG);
    type_str(&mut p, "qu");
    assert_eq!(p.query, "qu");
    assert_eq!(press(&mut p, KeyCode::Char('j'), BIG), Outcome::Continue);
    assert_eq!(p.query, "quj".to_string());
}

// ---------------------------------------------------------------- cancel

#[test]
fn q_and_esc_cancel_from_the_list() {
    assert_eq!(press(&mut picker("p", "", None), KeyCode::Char('q'), BIG), Outcome::Cancel);
    assert_eq!(press(&mut picker("p", "", None), KeyCode::Esc, BIG), Outcome::Cancel);
}

#[test]
fn ctrl_c_cancels_even_while_filtering() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Char('/'), BIG);
    let o = p.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL), BIG);
    assert_eq!(o, Outcome::Cancel);
}

#[test]
fn esc_while_filtering_clears_the_filter_and_a_second_esc_cancels() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Char('/'), BIG);
    type_str(&mut p, "feat");
    assert_eq!(press(&mut p, KeyCode::Esc, BIG), Outcome::Continue);
    assert_eq!((p.query.as_str(), p.shown.len(), p.filtering), ("", 6, false));
    assert_eq!(press(&mut p, KeyCode::Esc, BIG), Outcome::Cancel);
}

#[test]
fn empty_input_has_no_selection_and_enter_does_nothing() {
    let mut p = Pick::new(vec![], &opts("pick", ""), None);
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Continue);
    assert_eq!(press(&mut p, KeyCode::Char('j'), BIG), Outcome::Continue);
    assert_eq!(press(&mut p, KeyCode::Char('q'), BIG), Outcome::Cancel);
}

#[test]
fn under_the_floor_only_the_way_out_acts() {
    let mut p = picker("pick", "", None);
    let a = area(27, 11);
    assert!(pick::too_small(a) && !pick::too_small(area(28, 12)));
    assert_eq!(press(&mut p, KeyCode::Enter, a), Outcome::Continue);
    assert_eq!(press(&mut p, KeyCode::Char('x'), a), Outcome::Continue);
    assert!(!p.filtering);
    assert_eq!(press(&mut p, KeyCode::Esc, a), Outcome::Cancel);
}

#[test]
fn the_help_swallows_keys_and_any_other_key_closes_it() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Char('?'), BIG);
    assert!(p.help.is_some());
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Continue, "enter closes, never chooses");
    assert!(p.help.is_none());
}

// ---------------------------------------------------------------- mouse

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn a_click_selects_the_row_under_it_and_does_not_choose() {
    let mut p = picker("pick", "", None);
    p.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 3), BIG);
    assert_eq!(p.sel, 2, "row 0 is the prompt, so y=3 is the third item");
}

#[test]
fn a_click_on_the_prompt_the_footer_or_below_the_last_row_changes_nothing() {
    let mut p = picker("pick", "", None);
    for y in [0, 7, 23] {
        p.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, y), BIG);
        assert_eq!(p.sel, 0, "y={y}");
    }
}

#[test]
fn the_wheel_scrolls_the_window_and_keeps_the_selection_in_it() {
    let r: Vec<Row> = (0..40)
        .map(|i| Row {
            key: format!("k{i}"),
            text: format!("row {i}"),
        })
        .collect();
    let mut p = Pick::new(r, &opts("pick", ""), None);
    let a = area(60, 12);
    p.mouse(mouse(MouseEventKind::ScrollDown, 5, 3), a);
    assert!(p.top > 0 && p.top <= p.sel && p.sel < p.top + 10, "top {} sel {}", p.top, p.sel);
    for _ in 0..50 {
        p.mouse(mouse(MouseEventKind::ScrollDown, 5, 3), a);
    }
    assert_eq!(p.top, 30, "the last window, not past it");
    p.mouse(mouse(MouseEventKind::ScrollUp, 5, 3), a);
    assert!(p.top < 30);
}

// ---------------------------------------------------------------- arguments

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn the_flags_fill_the_options_and_none_means_the_defaults() {
    let o = pick::parse_args(&args(&["--prompt", "branch", "--header", "pick one", "--preview", "/tmp/x"])).unwrap();
    assert_eq!(
        (o.prompt.as_str(), o.header.as_str(), o.preview.as_deref()),
        ("branch", "pick one", Some("/tmp/x"))
    );
    let d = pick::parse_args(&[]).unwrap();
    assert_eq!((d.prompt.as_str(), d.header.as_str(), d.preview), ("pick", "", None));
}

#[test]
fn a_flag_without_its_value_and_an_unknown_flag_are_errors_naming_the_flag() {
    assert!(pick::parse_args(&args(&["--prompt"])).unwrap_err().contains("--prompt"));
    assert!(pick::parse_args(&args(&["--bogus"])).unwrap_err().contains("--bogus"));
}

// ---------------------------------------------------------------- goldens

#[test]
fn pick_40() {
    golden("pick-40", &draw(40, 14, &picker("branch", "pick the branch to switch to", None)));
}

#[test]
fn pick_60_with_a_preview() {
    golden("pick-60", &draw(60, 14, &picker("branch", "", Some(PREVIEW))));
}

#[test]
fn pick_80_with_a_preview() {
    golden(
        "pick-80",
        &draw(80, 14, &picker("branch", "pick the branch to switch to", Some(PREVIEW))),
    );
}

#[test]
fn pick_120_with_a_preview() {
    golden(
        "pick-120",
        &draw(120, 14, &picker("branch", "pick the branch to switch to", Some(PREVIEW))),
    );
}

#[test]
fn pick_80_filtered() {
    let mut p = picker("branch", "", None);
    type_str(&mut p, "FEAT");
    golden("pick-80-filtered", &draw(80, 14, &p));
}

#[test]
fn pick_80_no_match() {
    let mut p = picker("branch", "", None);
    type_str(&mut p, "zzz");
    golden("pick-80-nomatch", &draw(80, 14, &p));
}

#[test]
fn pick_80_help() {
    let mut p = picker("branch", "", None);
    press(&mut p, KeyCode::Char('?'), BIG);
    golden("pick-80-help", &draw(80, 24, &p));
}

#[test]
fn pick_floor_27x11() {
    golden("pick-27x11", &draw(27, 11, &picker("branch", "", None)));
}

#[test]
fn pick_28x12_is_the_smallest_list() {
    golden("pick-28x12", &draw(28, 12, &picker("branch", "", None)));
}

#[test]
fn pick_empty_input_draws_a_card_not_a_blank() {
    golden("pick-80-empty", &draw(80, 14, &Pick::new(vec![], &opts("branch", ""), None)));
}

#[test]
fn pick_scrolled_keeps_the_marker_on_the_last_row() {
    let r: Vec<Row> = (0..40)
        .map(|i| Row {
            key: format!("k{i}"),
            text: format!("row {i}"),
        })
        .collect();
    let mut p = Pick::new(r, &opts("pick", ""), None);
    let a = area(40, 12);
    press(&mut p, KeyCode::Char('G'), a);
    let s = draw(40, 12, &p);
    assert!(s.contains("row 39"), "{s}");
    golden("pick-40-last", &s);
}

/// D17: the text is `value`, never `meta`. A row, the prompt, the header and
/// the count must not wear the dim colour or the DIM modifier.
#[test]
fn no_text_is_drawn_in_meta_or_dim() {
    use ratatui::style::Modifier;
    let th = Theme::fixed();
    let p = picker("branch", "pick the branch", Some(PREVIEW));
    let mut t = Terminal::new(TestBackend::new(80, 14)).unwrap();
    t.draw(|f| pick::render(f, f.area(), &p, &th)).unwrap();
    for c in t.backend().buffer().content() {
        if c.symbol().trim().is_empty() {
            continue;
        }
        assert_ne!(c.fg, th.meta, "{:?} is drawn in meta", c.symbol());
        assert!(!c.modifier.contains(Modifier::DIM), "{:?} is dim", c.symbol());
    }
}

#[test]
fn control_bytes_in_a_row_never_reach_the_screen() {
    let p = Pick::new(pick::parse_rows("k\tev\x1b]0;pwned\x07il\n"), &opts("pick", ""), None);
    let s = draw(40, 12, &p);
    assert!(!s.contains('\x1b') && !s.contains('\x07'), "{s:?}");
}
