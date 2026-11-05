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
        copy: false,
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
    p.shown.iter().map(|&i| std::str::from_utf8(&p.rows[i].key).unwrap()).collect()
}

const PREVIEW: &str = "branch feat/237-pick\n\n  dea82ca Merge pull request #236\n  3 files changed\n";

// ---------------------------------------------------------------- the rows

#[test]
fn rows_split_at_the_first_tab_and_the_display_keeps_the_rest() {
    let r = pick::parse_rows(b"k1\tone\ttwo\nk2\tthree\n");
    assert_eq!((r[0].key.as_slice(), r[0].text.as_str()), (&b"k1"[..], "one two"));
    assert_eq!((r[1].key.as_slice(), r[1].text.as_str()), (&b"k2"[..], "three"));
}

#[test]
fn a_row_with_no_tab_is_its_own_key_and_display() {
    let r = pick::parse_rows(b"plain\n");
    assert_eq!((r[0].key.as_slice(), r[0].text.as_str()), (&b"plain"[..], "plain"));
}

#[test]
fn empty_input_and_blank_lines_give_no_rows() {
    assert!(pick::parse_rows(b"").is_empty());
    assert!(pick::parse_rows(b"\n\r\n\n").is_empty());
}

#[test]
fn a_crlf_row_loses_its_carriage_return() {
    let r = pick::parse_rows(b"k\tshown\r\n");
    assert_eq!((r[0].key.as_slice(), r[0].text.as_str()), (&b"k"[..], "shown"));
}

#[test]
fn control_characters_are_stripped_from_the_display() {
    let r = pick::parse_rows("k\tev\x1b[2Jil\x07\u{9b}x\n".as_bytes());
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
fn down_and_up_move_by_one_and_stop_at_both_ends() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::Up, BIG);
    assert_eq!(p.sel, 0, "up at the top stays");
    for _ in 0..9 {
        press(&mut p, KeyCode::Down, BIG);
    }
    assert_eq!(p.sel, 5, "down at the bottom stays on the last row");
}

#[test]
fn ctrl_n_and_ctrl_p_move_and_home_and_end_jump() {
    let mut p = picker("pick", "", None);
    let ctrl = |p: &mut Pick, c| p.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL), BIG);
    ctrl(&mut p, 'n');
    ctrl(&mut p, 'n');
    assert_eq!(p.sel, 2);
    ctrl(&mut p, 'p');
    assert_eq!(p.sel, 1);
    press(&mut p, KeyCode::End, BIG);
    assert_eq!(p.sel, 5);
    press(&mut p, KeyCode::Home, BIG);
    assert_eq!(p.sel, 0);
    assert_eq!(p.query, "", "a chord is not a letter");
}

#[test]
fn the_arrows_and_the_pages_move_through_chrome_scroll() {
    let many: Vec<(String, String)> = (0..40).map(|i| (format!("k{i}"), format!("row {i}"))).collect();
    let r: Vec<Row> = many
        .iter()
        .map(|(k, t)| Row {
            key: k.clone().into_bytes(),
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
            key: format!("k{i}").into_bytes(),
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
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Choose(b"feat/237-pick".to_vec()));
}

#[test]
fn enter_after_a_filter_chooses_from_the_filtered_list() {
    let mut p = picker("pick", "", None);
    type_str(&mut p, "RELEASE");
    assert_eq!(names(&p), vec!["release/0.2"]);
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Choose(b"release/0.2".to_vec()));
}

#[test]
fn enter_with_no_match_chooses_nothing() {
    let mut p = picker("pick", "", None);
    type_str(&mut p, "zzz");
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Continue);
}

#[test]
fn a_new_query_puts_the_selection_back_on_the_first_match() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::End, BIG);
    type_str(&mut p, "f");
    assert_eq!((p.sel, p.top), (0, 0));
}

#[test]
fn backspace_widens_the_filter_and_on_an_empty_query_does_nothing() {
    let mut p = picker("pick", "", None);
    type_str(&mut p, "zz");
    assert!(p.shown.is_empty());
    press(&mut p, KeyCode::Backspace, BIG);
    press(&mut p, KeyCode::Backspace, BIG);
    assert_eq!(p.shown.len(), 6);
    assert_eq!(press(&mut p, KeyCode::Backspace, BIG), Outcome::Continue);
    assert_eq!(p.query, "");
}

#[test]
fn ctrl_u_clears_the_filter() {
    let mut p = picker("pick", "", None);
    type_str(&mut p, "feat");
    p.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL), BIG);
    assert_eq!((p.query.as_str(), p.shown.len()), ("", 6));
}

const SESSIONS: [(&str, &str); 6] = [
    ("kitchen", "kitchen"),
    ("queue", "queue"),
    ("main", "main"),
    ("jira-main", "jira-main"),
    ("git-hooks", "git-hooks"),
    ("ketchup", "ketchup"),
];

fn sessions() -> Pick {
    Pick::new(rows(&SESSIONS), &opts("session", ""), None)
}

/// The first letter of a name is never a command: `q` would have cancelled.
#[test]
fn typing_queue_filters_to_the_queue_row_and_enter_prints_its_key() {
    let mut p = sessions();
    for c in "queue".chars() {
        assert_eq!(press(&mut p, KeyCode::Char(c), BIG), Outcome::Continue, "{c}");
    }
    assert_eq!(names(&p), vec!["queue"]);
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Choose(b"queue".to_vec()));
}

/// `g` would have jumped to the top, leaving `it` and `kitchen` above `git-hooks`.
#[test]
fn typing_git_puts_git_hooks_first() {
    let mut p = sessions();
    type_str(&mut p, "git");
    assert_eq!((p.query.as_str(), names(&p)), ("git", vec!["git-hooks"]));
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Choose(b"git-hooks".to_vec()));
}

#[test]
fn typing_jira_keeps_the_j_and_k_h_q_g_and_question_mark_are_letters() {
    let mut p = sessions();
    type_str(&mut p, "jira");
    assert_eq!((p.query.as_str(), names(&p)), ("jira", vec!["jira-main"]));
    let mut p = Pick::new(rows(&[("a", "what? quick Gk")]), &opts("pick", ""), None);
    type_str(&mut p, "? q");
    assert_eq!((p.query.as_str(), p.help), ("? q", None));
    assert_eq!(p.shown.len(), 1);
}

#[test]
fn alt_letter_is_not_a_letter() {
    let mut p = picker("pick", "", None);
    let o = p.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT), BIG);
    assert_eq!((o, p.query.as_str()), (Outcome::Continue, ""));
    p.key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::SHIFT), BIG);
    assert_eq!(p.query, "B", "shift is how a capital arrives");
}

// ---------------------------------------------------------------- cancel

#[test]
fn esc_cancels_with_or_without_a_filter() {
    assert_eq!(press(&mut picker("p", "", None), KeyCode::Esc, BIG), Outcome::Cancel);
    let mut p = picker("p", "", None);
    type_str(&mut p, "feat");
    assert_eq!(press(&mut p, KeyCode::Esc, BIG), Outcome::Cancel);
}

#[test]
fn ctrl_c_cancels_even_while_filtering() {
    let mut p = picker("pick", "", None);
    type_str(&mut p, "fe");
    let o = p.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL), BIG);
    assert_eq!(o, Outcome::Cancel);
}

#[test]
fn empty_input_has_no_selection_and_enter_does_nothing() {
    let mut p = Pick::new(vec![], &opts("pick", ""), None);
    assert_eq!(press(&mut p, KeyCode::Enter, BIG), Outcome::Continue);
    assert_eq!(press(&mut p, KeyCode::Down, BIG), Outcome::Continue);
    assert_eq!(press(&mut p, KeyCode::Esc, BIG), Outcome::Cancel);
}

#[test]
fn under_the_floor_only_the_way_out_acts() {
    let mut p = picker("pick", "", None);
    let a = area(27, 11);
    assert!(pick::too_small(a) && !pick::too_small(area(28, 12)));
    assert_eq!(press(&mut p, KeyCode::Enter, a), Outcome::Continue);
    assert_eq!(press(&mut p, KeyCode::Char('x'), a), Outcome::Continue);
    assert_eq!(p.query, "");
    assert_eq!(press(&mut p, KeyCode::Esc, a), Outcome::Cancel);
}

#[test]
fn f1_opens_the_help_and_any_other_key_closes_it_without_acting() {
    let mut p = picker("pick", "", None);
    press(&mut p, KeyCode::F(1), BIG);
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
            key: format!("k{i}").into_bytes(),
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
    press(&mut p, KeyCode::F(1), BIG);
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
fn pick_scrolled_keeps_the_marker_on_the_last_row() {
    let r: Vec<Row> = (0..40)
        .map(|i| Row {
            key: format!("k{i}").into_bytes(),
            text: format!("row {i}"),
        })
        .collect();
    let mut p = Pick::new(r, &opts("pick", ""), None);
    let a = area(40, 12);
    press(&mut p, KeyCode::End, a);
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
    let p = Pick::new(pick::parse_rows(b"k\tev\x1b]0;pwned\x07il\n"), &opts("pick", ""), None);
    let s = draw(40, 12, &p);
    assert!(!s.contains('\x1b') && !s.contains('\x07'), "{s:?}");
}

// ---------------------------------------------------------------- untrusted text

const HOSTILE: &str = "x\x1b]0;pwned\x07y\u{9b}z";

/// ratatui drops control characters when it writes a cell, so the screen cannot
/// show whether `Pick::new` cleaned: the fields are the contract.
#[test]
fn prompt_header_and_preview_hold_no_control_character_after_new() {
    let o = Opts {
        prompt: HOSTILE.into(),
        header: format!("{HOSTILE}\nsecond"),
        preview: None,
        copy: false,
    };
    let p = Pick::new(vec![], &o, Some(format!("a{HOSTILE}\n{HOSTILE}b\tc\n")));
    let bad = |s: &str| s.chars().any(|c| c.is_control() && c != '\n');
    assert!(!bad(&p.prompt) && !bad(&p.header), "{:?} {:?}", p.prompt, p.header);
    assert_eq!(p.preview.as_deref(), Some("ax]0;pwnedyz\nx]0;pwnedyzb c"));
    assert_eq!(p.header, "x]0;pwnedyz second");
}

#[test]
fn bidi_controls_are_dropped_so_a_name_cannot_lie() {
    assert_eq!(pick::clean("bidi \u{202e}txt.exe\u{2066}\u{2069}\u{200f}\u{202a}"), "bidi txt.exe");
    let r = pick::parse_rows("k\tevil\u{202e}gpj.sh".as_bytes());
    assert_eq!(r[0].text, "evilgpj.sh");
}

// ---------------------------------------------------------------- the key is bytes

#[test]
fn a_key_that_is_not_utf8_comes_back_byte_for_byte() {
    let r = pick::parse_rows(b"/tmp/caf\xe9.txt\tcaf\xe9\n");
    assert_eq!(r[0].key, b"/tmp/caf\xe9.txt");
    let mut out = Vec::new();
    assert_eq!(pick::answer(&mut out, &Outcome::Choose(r[0].key.clone())), 0);
    assert_eq!(out, b"/tmp/caf\xe9.txt\n");
}

#[test]
fn a_cancel_answers_one_and_writes_nothing() {
    let mut out = Vec::new();
    assert_eq!(pick::answer(&mut out, &Outcome::Cancel), 1);
    assert!(out.is_empty());
}

#[test]
fn an_unwritable_stdout_answers_two_not_a_panic() {
    let mut full = std::fs::OpenOptions::new().write(true).open("/dev/full").unwrap();
    assert_eq!(pick::answer(&mut full, &Outcome::Choose(b"k".to_vec())), 2);
}

#[test]
fn the_input_cap_stops_at_a_line_boundary_and_says_it_cut() {
    let (b, cut) = pick::cap_input(b"aa\nbb\ncc\n".to_vec(), 7);
    assert_eq!((b.as_slice(), cut), (&b"aa\nbb\n"[..], true));
    let (b, cut) = pick::cap_input(b"aa\nbb\n".to_vec(), 6);
    assert_eq!((b.as_slice(), cut), (&b"aa\nbb\n"[..], false), "exactly the cap is not cut");
    let (b, cut) = pick::cap_input(b"aaaaaaaaa".to_vec(), 4);
    assert_eq!(
        (b.len(), cut),
        (0, true),
        "one line longer than the cap leaves nothing, never half a key"
    );
    let (b, _) = pick::cap_input(b"aa\nbb\ncc\n".to_vec(), 6);
    assert_eq!(b, b"aa\nbb\n", "a cap landing on a newline keeps that line");
}

#[test]
fn the_28x12_footer_keeps_choose_and_cancel() {
    let s = draw(28, 12, &picker("branch", "", None));
    let last = s.lines().rev().find(|l| l.contains("choose")).unwrap_or("");
    assert!(last.contains("choose") && last.contains("cancel"), "{s}");
}

#[test]
fn joiners_survive_the_strip_because_persian_and_emoji_need_them() {
    let s = "می\u{200c}خواهم \u{1f468}\u{200d}\u{1f469}";
    assert_eq!(pick::clean(s), s);
}

#[test]
fn only_lines_with_a_tab_and_no_key_are_counted_as_skipped() {
    let input = b"\tno key\nplain\nk\tok\n\n\tagain\n\tthird";
    assert_eq!(pick::parse_rows(input).len(), 2);
    assert_eq!(pick::keyless(input), 3, "a blank line and a tab-less line are not keyless");
    assert_eq!(
        pick::keyless_note(2).as_deref(),
        Some("cockpit-tui: pick: skipped 2 rows with no key")
    );
    assert_eq!(
        pick::keyless_note(1).as_deref(),
        Some("cockpit-tui: pick: skipped 1 row with no key")
    );
    assert_eq!(pick::keyless_note(0), None);
}

#[test]
fn the_floor_card_names_esc_alone_as_the_way_out() {
    let s = draw(27, 11, &picker("branch", "", None));
    assert!(s.contains("esc closes") && !s.contains("or q"), "{s}");
}

// --- the reusable pieces (#245) ---------------------------------------------

#[test]
fn set_rows_keeps_the_query_and_the_selection_on_its_key() {
    let mut p = picker("branch", "", None);
    type_str(&mut p, "e");
    press(&mut p, KeyCode::Down, BIG);
    let want = names(&p)[p.sel].to_string();
    // a refresh that puts a new row first and drops another
    let mut next = rows(&[("new", "new  e")]);
    next.extend(rows(&BRANCHES).into_iter().filter(|r| r.key != b"main"));
    p.set_rows(next, BIG);
    assert_eq!(p.query, "e");
    assert_eq!(names(&p)[p.sel], want);
}

#[test]
fn set_rows_with_the_selected_row_gone_stays_on_a_row() {
    let mut p = picker("branch", "", None);
    p.sel = 5;
    p.set_rows(rows(&BRANCHES[..2]), BIG);
    assert_eq!(p.sel, 1);
}

#[test]
fn set_rows_to_nothing_leaves_an_empty_list() {
    let mut p = picker("branch", "", None);
    p.set_rows(vec![], BIG);
    assert!(p.shown.is_empty());
    assert_eq!(p.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), BIG), Outcome::Continue);
}

#[test]
fn render_with_the_default_look_is_render() {
    let p = picker("branch", "pick one", Some(PREVIEW));
    let mut t = Terminal::new(TestBackend::new(80, 14)).unwrap();
    t.draw(|f| pick::render_with(f, f.area(), &p, &Theme::fixed(), &pick::Look::default()))
        .unwrap();
    assert_eq!(t.backend().to_string(), draw(80, 14, &p));
}

#[test]
fn a_look_brings_its_own_footer_and_floor_title() {
    let p = picker("branch", "", None);
    let look = pick::Look {
        title: Some("mine"),
        hints: Some(vec![("^Z".into(), "undo".into()), ("esc".into(), "cancel".into())]),
        ..Default::default()
    };
    let mut t = Terminal::new(TestBackend::new(40, 14)).unwrap();
    t.draw(|f| pick::render_with(f, f.area(), &p, &Theme::fixed(), &look)).unwrap();
    assert!(t.backend().to_string().contains("^Z undo"));
    let mut t = Terminal::new(TestBackend::new(20, 14)).unwrap();
    t.draw(|f| pick::render_with(f, f.area(), &p, &Theme::fixed(), &look)).unwrap();
    assert!(t.backend().to_string().contains(" mine "));
}

#[test]
fn an_entry_replaces_the_query_on_the_prompt_line_only() {
    let mut p = picker("rename", "", None);
    type_str(&mut p, "rel");
    p.entry = Some("typed-elsewhere".into());
    let out = draw(80, 14, &p);
    assert!(out.lines().next().unwrap().contains("rename> typed-elsewhere"));
    assert_eq!((p.query.as_str(), p.shown.len()), ("rel", 1));
}

#[test]
fn a_list_share_moves_the_preview_divider() {
    let mut p = picker("branch", "", Some(PREVIEW));
    let at = |p: &Pick| pick::geo(area(100, 14), p).preview.unwrap().x;
    assert_eq!(at(&p), 40);
    p.list_pct = 55;
    assert_eq!(at(&p), 55);
}

fn alt(p: &mut Pick, c: char) -> Outcome {
    p.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT), BIG)
}

#[test]
fn alt_c_copies_the_selected_row_when_copy_is_on() {
    let mut p = Pick::new(
        rows(&BRANCHES),
        &Opts {
            copy: true,
            ..opts("p", "")
        },
        None,
    );
    press(&mut p, KeyCode::Down, BIG);
    assert_eq!(alt(&mut p, 'c'), Outcome::Copy(b"feat/237-pick".to_vec()));
}

#[test]
fn alt_c_is_nothing_when_copy_is_off_and_never_a_letter() {
    let mut p = picker("p", "", None);
    assert_eq!(alt(&mut p, 'c'), Outcome::Continue);
    assert_eq!(p.query, "");
}

#[test]
fn alt_c_on_an_empty_list_chooses_nothing() {
    let mut p = Pick::new(
        rows(&BRANCHES),
        &Opts {
            copy: true,
            ..opts("p", "")
        },
        None,
    );
    type_str(&mut p, "zzzz");
    assert_eq!(alt(&mut p, 'c'), Outcome::Continue);
}

#[test]
fn a_copy_answers_ten_with_the_key_and_the_flag_parses() {
    let mut out = Vec::new();
    assert_eq!(pick::answer(&mut out, &Outcome::Copy(b"3".to_vec())), 10);
    assert_eq!(out, b"3\n");
    let a = |v: &[&str]| pick::parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert!(a(&["--copy"]).unwrap().copy);
    assert!(!a(&[]).unwrap().copy);
}

#[test]
fn the_footer_names_alt_c_only_when_copy_is_on() {
    let on = Pick::new(
        rows(&BRANCHES),
        &Opts {
            copy: true,
            ..opts("p", "")
        },
        None,
    );
    assert!(draw(80, 24, &on).contains("alt-c"));
    assert!(!draw(80, 24, &picker("p", "", None)).contains("alt-c"));
}

#[test]
fn the_help_lists_alt_c_only_when_copy_is_on() {
    let g = cockpit_tui::theme::Glyphs::unicode();
    let has = |c| pick::help_rows_for(&g, c).iter().any(|(k, _)| k == "alt-c");
    assert!(has(true));
    assert!(!has(false));
    assert_eq!(pick::help_rows_for(&g, true).len(), pick::help_rows(&g).len() + 1);
    let on = Pick::new(
        rows(&BRANCHES),
        &Opts {
            copy: true,
            ..opts("p", "")
        },
        None,
    );
    assert_eq!(on.help_len, pick::help_rows(&g).len() + 1);
}

fn copy_picker() -> Pick {
    Pick::new(
        rows(&BRANCHES),
        &Opts {
            copy: true,
            ..opts("p", "")
        },
        None,
    )
}

#[test]
fn the_f1_screen_draws_the_alt_c_row_only_when_copy_is_on() {
    let mut on = copy_picker();
    press(&mut on, KeyCode::F(1), BIG);
    assert!(draw(80, 24, &on).contains("copy: print this row's key, exit 10"));
    let mut off = picker("p", "", None);
    press(&mut off, KeyCode::F(1), BIG);
    assert!(!draw(80, 24, &off).contains("alt-c"));
}

#[test]
fn the_alt_c_help_row_follows_the_enter_row_in_both_glyph_sets() {
    for g in [cockpit_tui::theme::Glyphs::unicode(), cockpit_tui::theme::Glyphs::ascii()] {
        let rows = pick::help_rows_for(&g, true);
        let at = rows.iter().position(|(k, _)| k == "alt-c").expect("an alt-c row");
        assert_eq!(rows[at - 1].0, g.key_enter, "alt-c sits under the choose key");
    }
}
