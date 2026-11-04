//! The sessions view: parsing, order, the key table, the action files and
//! goldens. No tmux, no tty, no clock: every input is a literal and the theme is
//! `Theme::fixed()`.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
use cockpit_tui::data::State;
use cockpit_tui::data::sessions::{self, Sess};
use cockpit_tui::sessions::{Act, Args, Step, View, draw, emit};
use cockpit_tui::theme::{Glyphs, Theme};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

const NOW: u64 = 10_000;
const US: &str = "\u{1f}";

#[allow(clippy::too_many_arguments)]
fn rec(id: &str, att: u32, ago: u64, widx: u32, active: u8, agent: u8, st: &str, msg: &str, wname: &str, name: &str) -> String {
    rec_since(id, att, ago, widx, active, agent, st, msg, "", wname, name)
}

/// `rec` with the pane's `@panestate_since` (#225).
#[allow(clippy::too_many_arguments)]
fn rec_since(
    id: &str,
    att: u32,
    ago: u64,
    widx: u32,
    active: u8,
    agent: u8,
    st: &str,
    msg: &str,
    since: &str,
    wname: &str,
    name: &str,
) -> String {
    let last = NOW - ago;
    [
        id,
        &att.to_string(),
        &last.to_string(),
        &widx.to_string(),
        &active.to_string(),
        "%1",
        &agent.to_string(),
        st,
        msg,
        since,
        wname,
        name,
    ]
    .join(US)
}

fn fixture() -> Vec<Sess> {
    let l = [
        rec(
            "$1",
            0,
            45,
            0,
            1,
            1,
            "needs-input",
            "Run rm -rf build/ before the tests?",
            "claude",
            "api",
        ),
        rec("$1", 0, 45, 1, 0, 0, "", "", "shell", "api"),
        rec("$2", 1, 65, 0, 1, 1, "running", "", "claude", "web"),
        rec("$3", 0, 252, 0, 1, 0, "", "", "notes", "docs"),
        rec("$3", 0, 252, 1, 0, 0, "", "", "build", "docs"),
        rec("$4", 1, 3780, 0, 1, 1, "done", "", "claude", "tmux-cockpit"),
        rec("$5", 0, 5, 0, 1, 1, "error", "exit 2", "claude", "ops"),
    ];
    sessions::parse(&l.join("\n"))
}

fn args(needs: bool, cur: Option<&str>) -> Args {
    Args {
        needs,
        current: cur.map(Into::into),
    }
}

const BIG: Rect = Rect::new(0, 0, 80, 24);

fn view_with(needs: bool, carry: bool) -> View {
    View::new(fixture(), &args(needs, Some("$4")), carry, NOW, Glyphs::unicode())
}

fn view() -> View {
    view_with(false, true)
}

fn k(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn press(v: &mut View, e: KeyEvent) -> Step {
    v.key(e, BIG)
}

fn type_str(v: &mut View, s: &str) {
    for c in s.chars() {
        assert_eq!(press(v, k(KeyCode::Char(c))), Step::Continue);
    }
}

fn line(s: &str) -> Act {
    Act {
        line: s.into(),
        text: None,
        expect: None,
    }
}

fn draw_at(v: &mut View, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    let th = Theme::fixed();
    t.draw(|f| draw(f, f.area(), v, &th)).unwrap();
    t.backend().to_string()
}

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

fn names(v: &View) -> Vec<&str> {
    v.sess.iter().map(|s| s.name.as_str()).collect()
}

// --- the data ---------------------------------------------------------------

#[test]
fn parse_groups_panes_into_sessions_and_windows() {
    let v = fixture();
    assert_eq!(v.len(), 5);
    let api = &v[0];
    assert_eq!(
        (api.id.as_str(), api.name.as_str(), api.wins.len(), api.agents()),
        ("$1", "api", 2, 1)
    );
    assert_eq!(api.wins[0].ask, "Run rm -rf build/ before the tests?");
    assert_eq!(api.state(), Some(State::NeedsInput));
}

#[test]
fn a_session_without_an_agent_has_no_state() {
    assert_eq!(fixture()[2].state(), None);
}

#[test]
fn the_worst_pane_names_the_session() {
    let l = [
        rec("$1", 0, 1, 0, 1, 1, "done", "", "a", "s"),
        rec("$1", 0, 1, 0, 1, 1, "error", "boom", "a", "s"),
        rec("$1", 0, 1, 1, 0, 1, "running", "", "b", "s"),
    ];
    let v = sessions::parse(&l.join("\n"));
    assert_eq!(
        (v[0].state(), v[0].wins[0].ask.as_str(), v[0].agents()),
        (Some(State::Error), "boom", 3)
    );
}

#[test]
fn a_stale_state_on_a_pane_that_is_not_an_agent_is_not_believed() {
    let v = sessions::parse(&rec("$1", 0, 1, 0, 1, 0, "needs-input", "?", "sh", "s"));
    assert_eq!((v[0].state(), v[0].agents()), (None, 0));
}

#[test]
fn a_line_that_does_not_split_is_skipped() {
    let good = rec("$1", 0, 1, 0, 1, 1, "done", "", "a", "s");
    let bad = ["short", &format!("x{US}1"), &rec("1", 0, 1, 0, 1, 1, "done", "", "a", "no-dollar")].join("\n");
    assert_eq!(sessions::parse(&format!("{bad}\n{good}")).len(), 1);
    assert!(sessions::parse("").is_empty());
}

#[test]
fn a_separator_inside_a_name_stays_in_the_name() {
    let v = sessions::parse(&rec("$1", 0, 1, 0, 1, 0, "", "", "w", &format!("a{US}b")));
    assert_eq!(v[0].name, format!("a{US}b"));
}

#[test]
fn is_sid_takes_dollar_digits_only() {
    for ok in ["$0", "$12"] {
        assert!(sessions::is_sid(ok), "{ok}");
    }
    for bad in ["", "$", "12", "$1a", "$-1", "%1", "@1", "$1 ", "$1\n", " $1"] {
        assert!(!sessions::is_sid(bad), "{bad:?}");
    }
}

#[test]
fn order_is_waiting_then_failed_then_recent_and_your_own_last() {
    let v = view();
    assert_eq!(names(&v), ["api", "ops", "web", "docs", "tmux-cockpit"]);
}

#[test]
fn needs_keeps_only_the_waiting_and_the_failed() {
    let v = view_with(true, true);
    assert_eq!(names(&v), ["api", "ops"]);
    assert_eq!(v.header(), "needs you · ▲1 ✕1");
}

#[test]
fn needs_with_nothing_waiting_falls_back_to_every_session() {
    let l = [
        rec("$1", 0, 1, 0, 1, 1, "done", "", "a", "one"),
        rec("$2", 0, 9, 0, 1, 0, "", "", "a", "two"),
    ];
    let v = View::new(sessions::parse(&l.join("\n")), &args(true, None), true, NOW, Glyphs::unicode());
    assert_eq!(names(&v), ["one", "two"]);
    assert_eq!(v.header(), "nothing needs you · 2 sessions");
}

#[test]
fn a_refresh_keeps_the_order_drops_the_gone_and_appends_the_new() {
    let mut v = view();
    let mut fresh = fixture();
    fresh.retain(|s| s.name != "web");
    fresh.push(sessions::parse(&rec("$9", 0, 1, 0, 1, 0, "", "", "w", "newest")).remove(0));
    // the waiting one is now done: order must not move
    fresh[0].wins[0].state = State::Done;
    v.refresh(fresh, NOW, BIG);
    assert_eq!(names(&v), ["api", "ops", "docs", "tmux-cockpit", "newest"]);
}

#[test]
fn a_needs_view_takes_no_new_sessions_on_refresh() {
    let mut v = view_with(true, true);
    let mut fresh = fixture();
    fresh.push(sessions::parse(&rec("$9", 0, 1, 0, 1, 1, "needs-input", "?", "w", "newer")).remove(0));
    v.refresh(fresh, NOW, BIG);
    assert_eq!(names(&v), ["api", "ops"]);
}

#[test]
fn clean_name_follows_tms_rules() {
    for (i, o) in [
        ("a b.c:d", "a_b_c_d"),
        ("--x", "x"),
        ("a\x1bb", "ab"),
        ("", ""),
        ("---", ""),
        ("ü ñ", "ü_ñ"),
    ] {
        assert_eq!(sessions::clean_name(i), o, "{i:?}");
    }
}

// --- the keys ---------------------------------------------------------------

#[test]
fn enter_switches_to_the_selected_session_by_id() {
    let mut v = view();
    press(&mut v, k(KeyCode::Down));
    assert_eq!(press(&mut v, k(KeyCode::Enter)), Step::Act(line("switch $5")));
}

#[test]
fn a_digit_on_an_empty_filter_jumps_to_that_row() {
    let mut v = view();
    assert_eq!(press(&mut v, k(KeyCode::Char('3'))), Step::Act(line("switch $2")));
}

#[test]
fn a_digit_past_the_last_row_does_nothing() {
    let mut v = view();
    assert_eq!(press(&mut v, k(KeyCode::Char('9'))), Step::Continue);
}

#[test]
fn a_digit_after_a_letter_filters_instead_of_jumping() {
    let mut v = view();
    type_str(&mut v, "a2");
    assert_eq!(v.pick.query, "a2");
    assert!(v.pick.shown.is_empty());
}

#[test]
fn enter_with_no_match_creates_the_typed_name_in_text() {
    let mut v = view();
    type_str(&mut v, "new thing");
    let want = Act {
        line: "new-session -".into(),
        text: Some("new_thing".into()),
        expect: None,
    };
    assert_eq!(press(&mut v, k(KeyCode::Enter)), Step::Act(want));
}

#[test]
fn alt_enter_creates_even_when_a_row_matches() {
    let mut v = view();
    type_str(&mut v, "we");
    let e = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    let Step::Act(a) = press(&mut v, e) else { panic!("no action") };
    assert_eq!((a.line.as_str(), a.text.as_deref()), ("new-session -", Some("we")));
}

#[test]
fn a_name_that_cleans_to_nothing_creates_nothing() {
    let mut v = view();
    type_str(&mut v, "---");
    assert_eq!(press(&mut v, k(KeyCode::Enter)), Step::Continue);
    assert!(v.header().contains("type a name"), "{}", v.header());
}

#[test]
fn enter_on_an_empty_list_and_query_does_nothing() {
    let mut v = View::new(vec![], &args(false, None), true, NOW, Glyphs::unicode());
    assert_eq!(press(&mut v, k(KeyCode::Enter)), Step::Continue);
}

#[test]
fn ctrl_x_arms_and_names_what_dies() {
    let mut v = view();
    assert_eq!(press(&mut v, ctrl('x')), Step::Continue);
    assert_eq!(v.header(), "kill api? 1 agent (1 waiting) · 2 windows");
}

#[test]
fn an_attached_session_says_so_before_it_dies() {
    let mut v = view();
    type_str(&mut v, "web");
    press(&mut v, ctrl('x'));
    assert_eq!(v.header(), "kill web? attached elsewhere · 1 agent · 1 window");
}

#[test]
fn a_session_without_agents_dies_as_no_agents() {
    let mut v = view();
    type_str(&mut v, "docs");
    press(&mut v, ctrl('x'));
    assert_eq!(v.header(), "kill docs? no agents · 2 windows");
}

#[test]
fn the_second_ctrl_x_kills_and_expects_the_shown_name() {
    let mut v = view();
    press(&mut v, ctrl('x'));
    let want = Act {
        line: "kill-session $1".into(),
        text: None,
        expect: Some("api".into()),
    };
    assert_eq!(press(&mut v, ctrl('x')), Step::Act(want));
}

#[test]
fn any_other_key_keeps_the_session_and_is_spent() {
    for e in [k(KeyCode::Char('y')), k(KeyCode::Enter), k(KeyCode::Esc), k(KeyCode::Down)] {
        let mut v = view();
        press(&mut v, ctrl('x'));
        assert_eq!(press(&mut v, e), Step::Continue, "{e:?}");
        assert_eq!(v.header(), "5 sessions · ▲1 ✕1 ●1", "{e:?}");
        assert_eq!(v.pick.query, "", "the key must not reach the filter");
    }
}

#[test]
fn your_own_session_refuses_to_be_killed() {
    let mut v = view();
    press(&mut v, k(KeyCode::End));
    assert_eq!(press(&mut v, ctrl('x')), Step::Continue);
    assert!(v.header().starts_with("✕ tmux-cockpit is your own session"), "{}", v.header());
    // and a second ^X still kills nothing
    assert_eq!(press(&mut v, ctrl('x')), Step::Continue);
}

#[test]
fn a_session_that_goes_while_armed_disarms() {
    let mut v = view();
    press(&mut v, ctrl('x'));
    let mut fresh = fixture();
    fresh.retain(|s| s.name != "api");
    v.refresh(fresh, NOW, BIG);
    assert_eq!(v.header(), "✕ that session is gone");
    assert_eq!(press(&mut v, ctrl('x')), Step::Continue);
}

#[test]
fn f2_renames_and_expects_the_old_name() {
    let mut v = view();
    press(&mut v, k(KeyCode::F(2)));
    assert_eq!(v.pick.entry.as_deref(), Some("api"));
    for _ in 0..3 {
        press(&mut v, k(KeyCode::Backspace));
    }
    type_str(&mut v, "gateway v2");
    let want = Act {
        line: "rename-session $1".into(),
        text: Some("gateway_v2".into()),
        expect: Some("api".into()),
    };
    assert_eq!(press(&mut v, k(KeyCode::Enter)), Step::Act(want));
}

#[test]
fn esc_in_a_rename_cancels_the_rename_not_the_picker() {
    let mut v = view();
    press(&mut v, k(KeyCode::F(2)));
    assert_eq!(press(&mut v, k(KeyCode::Esc)), Step::Continue);
    assert_eq!((v.pick.entry.as_deref(), v.pick.prompt.as_str()), (None, "session"));
    assert_eq!(press(&mut v, k(KeyCode::Esc)), Step::Cancel);
}

#[test]
fn an_unchanged_or_empty_rename_sends_nothing() {
    let mut v = view();
    press(&mut v, k(KeyCode::F(2)));
    assert_eq!(press(&mut v, k(KeyCode::Enter)), Step::Continue);
    press(&mut v, k(KeyCode::F(2)));
    press(&mut v, ctrl('u'));
    assert_eq!(press(&mut v, k(KeyCode::Enter)), Step::Continue);
}

#[test]
fn typing_in_a_rename_leaves_the_filter_alone() {
    let mut v = view();
    press(&mut v, k(KeyCode::F(2)));
    type_str(&mut v, "zzz");
    assert_eq!((v.pick.query.as_str(), v.pick.shown.len()), ("", 5));
}

#[test]
fn esc_and_ctrl_c_cancel() {
    assert_eq!(press(&mut view(), k(KeyCode::Esc)), Step::Cancel);
    assert_eq!(press(&mut view(), ctrl('c')), Step::Cancel);
    let mut v = view();
    press(&mut v, ctrl('x'));
    assert_eq!(press(&mut v, ctrl('c')), Step::Cancel);
}

#[test]
fn without_an_action_file_only_switching_is_offered() {
    for e in [ctrl('x'), k(KeyCode::F(2))] {
        let mut v = view_with(false, false);
        assert_eq!(press(&mut v, e), Step::Continue);
        assert!(v.header().contains("needs COCKPIT_ACTION"), "{}", v.header());
    }
    let mut v = view_with(false, false);
    type_str(&mut v, "nothing");
    assert_eq!(press(&mut v, k(KeyCode::Enter)), Step::Continue);
    assert!(v.header().contains("needs COCKPIT_ACTION"));
    let mut v = view_with(false, false);
    assert_eq!(press(&mut v, k(KeyCode::Enter)), Step::Act(line("switch $1")));
}

fn click(row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn a_click_selects_and_a_second_click_on_it_switches() {
    let mut v = view();
    // row 0 is the prompt, 1 the header: the list starts at 2
    assert_eq!(v.mouse(click(4), BIG), Step::Continue);
    assert_eq!(v.selected().unwrap().name, "web");
    assert_eq!(v.mouse(click(4), BIG), Step::Act(line("switch $2")));
}

#[test]
fn a_click_below_the_last_row_selects_nothing() {
    let mut v = view();
    assert_eq!(v.mouse(click(20), BIG), Step::Continue);
    assert_eq!(v.selected().unwrap().name, "api");
}

#[test]
fn a_click_while_armed_keeps_the_session() {
    let mut v = view();
    press(&mut v, ctrl('x'));
    assert_eq!(v.mouse(click(3), BIG), Step::Continue);
    assert!(v.header().starts_with("5 sessions"));
}

#[test]
fn a_refresh_keeps_the_selection_on_its_session() {
    let mut v = view();
    press(&mut v, k(KeyCode::Down));
    press(&mut v, k(KeyCode::Down));
    let mut fresh = fixture();
    fresh.retain(|s| s.name != "api");
    v.refresh(fresh, NOW, BIG);
    assert_eq!(v.selected().unwrap().name, "web");
}

// --- the action files -------------------------------------------------------

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cockpit-sessions-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn emit_writes_text_and_expect_before_the_line_all_private() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp("emit");
    let f = d.join("act");
    let a = Act {
        line: "rename-session $3".into(),
        text: Some("new".into()),
        expect: Some("old".into()),
    };
    emit(&a, f.to_str()).unwrap();
    let r = |s: &str| std::fs::read_to_string(format!("{}{s}", f.display())).unwrap();
    assert_eq!(
        (r(""), r(".text"), r(".expect")),
        ("rename-session $3\n".into(), "new\n".into(), "old\n".into())
    );
    for s in ["", ".text", ".expect"] {
        let m = std::fs::metadata(format!("{}{s}", f.display())).unwrap().permissions().mode();
        assert_eq!(m & 0o777, 0o600, "{s}");
    }
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn emit_refuses_a_file_that_is_already_there_and_a_link() {
    let d = tmp("fresh");
    let f = d.join("act");
    std::fs::write(&f, "x").unwrap();
    assert!(emit(&line("switch $1"), f.to_str()).is_err());
    let g = d.join("act2");
    std::os::unix::fs::symlink(d.join("elsewhere"), format!("{}.text", g.display())).unwrap();
    let a = Act {
        line: "new-session -".into(),
        text: Some("n".into()),
        expect: None,
    };
    assert!(emit(&a, g.to_str()).is_err());
    assert!(!d.join("elsewhere").exists(), "the link was followed");
    assert!(!g.exists(), "the line must not be written after its text failed");
    std::fs::remove_dir_all(d).unwrap();
}

// --- args -------------------------------------------------------------------

fn sv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn args_take_needs_and_a_session_id() {
    let a = cockpit_tui::sessions::parse_args(&sv(&["--needs", "--current", "$7"])).unwrap();
    assert_eq!((a.needs, a.current.as_deref()), (true, Some("$7")));
    let a = cockpit_tui::sessions::parse_args(&[]).unwrap();
    assert_eq!((a.needs, a.current), (false, None));
}

#[test]
fn args_refuse_a_bad_id_a_missing_value_and_a_stranger() {
    for bad in [
        &["--current"][..],
        &["--current", "api"],
        &["--current", "$"],
        &["--current", "%3"],
        &["--nope"],
        &["x"],
    ] {
        assert!(cockpit_tui::sessions::parse_args(&sv(bad)).is_err(), "{bad:?}");
    }
}

// --- the screens ------------------------------------------------------------

#[test]
fn golden_80() {
    golden("sessions-80", &draw_at(&mut view(), 80, 14));
}

#[test]
fn golden_120() {
    golden("sessions-120", &draw_at(&mut view(), 120, 14));
}

#[test]
fn golden_60() {
    golden("sessions-60", &draw_at(&mut view(), 60, 14));
}

#[test]
fn golden_40() {
    golden("sessions-40", &draw_at(&mut view(), 40, 14));
}

#[test]
fn golden_floor() {
    golden("sessions-floor", &draw_at(&mut view(), 27, 11));
}

#[test]
fn golden_armed() {
    let mut v = view();
    press(&mut v, ctrl('x'));
    golden("sessions-armed", &draw_at(&mut v, 80, 14));
}

#[test]
fn golden_rename() {
    let mut v = view();
    press(&mut v, k(KeyCode::F(2)));
    golden("sessions-rename", &draw_at(&mut v, 80, 14));
}

#[test]
fn golden_create() {
    let mut v = view();
    type_str(&mut v, "scratch");
    golden("sessions-create", &draw_at(&mut v, 80, 14));
}

#[test]
fn golden_needs() {
    golden("sessions-needs", &draw_at(&mut view_with(true, true), 80, 14));
}

#[test]
fn golden_help() {
    let mut v = view();
    press(&mut v, k(KeyCode::F(1)));
    golden("sessions-help", &draw_at(&mut v, 80, 24));
}

#[test]
fn golden_ascii() {
    let mut v = View::new(fixture(), &args(false, Some("$4")), true, NOW, Glyphs::ascii());
    let mut t = Terminal::new(TestBackend::new(80, 14)).unwrap();
    let th = Theme::no_color();
    let th = Theme { g: Glyphs::ascii(), ..th };
    t.draw(|f| draw(f, f.area(), &mut v, &th)).unwrap();
    golden("sessions-ascii", &t.backend().to_string());
}

#[test]
fn the_screen_never_shows_a_control_character_from_a_name() {
    let mut s = fixture();
    s[0].name = "a\u{1b}[31mred".into();
    s[0].wins[0].ask = "x\u{7}y".into();
    let mut v = View::new(s, &args(false, None), true, NOW, Glyphs::unicode());
    let out = draw_at(&mut v, 120, 14);
    assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out}");
}

// --- .expect is the raw name -------------------------------------------------

fn spoofed() -> View {
    // U+202E reorders what is drawn, so the screen strips it; tmux still has it
    let name = "ab\u{202e}cd";
    let l = rec("$1", 0, 1, 0, 1, 0, "", "", "w", name);
    View::new(sessions::parse(&l), &args(false, None), true, NOW, Glyphs::unicode())
}

#[test]
fn a_kill_expects_the_name_tmux_has_not_the_one_drawn() {
    let mut v = spoofed();
    press(&mut v, ctrl('x'));
    assert_eq!(v.header(), "kill abcd? no agents · 1 window");
    let Step::Act(a) = press(&mut v, ctrl('x')) else {
        panic!("no action")
    };
    assert_eq!(a.expect.as_deref(), Some("ab\u{202e}cd"));
}

#[test]
fn a_rename_expects_the_name_tmux_has_not_the_one_drawn() {
    let mut v = spoofed();
    press(&mut v, k(KeyCode::F(2)));
    type_str(&mut v, "x");
    let Step::Act(a) = press(&mut v, k(KeyCode::Enter)) else {
        panic!("no action")
    };
    assert_eq!((a.text.as_deref(), a.expect.as_deref()), (Some("abcdx"), Some("ab\u{202e}cd")));
}

// --- #225: waiting timers -----------------------------------------------------

/// Three waiting sessions (one with no stamp, from before #225), one failed, one idle.
fn waiting() -> Vec<Sess> {
    let t = |ago: u64| (NOW - ago).to_string();
    let l = [
        rec_since("$1", 0, 5, 0, 1, 1, "needs-input", "?", &t(30), "claude", "short"),
        rec_since("$2", 0, 900, 0, 1, 1, "needs-input", "?", "", "claude", "unstamped"),
        rec_since("$3", 0, 50, 0, 1, 1, "needs-input", "?", &t(250), "claude", "long"),
        // a second waiting pane of $3: the session waits as long as its oldest
        rec_since("$3", 0, 50, 1, 0, 1, "needs-input", "?", &t(40), "claude", "long"),
        rec_since("$4", 0, 1, 0, 1, 1, "error", "boom", "", "claude", "failed"),
        rec_since("$5", 0, 2, 0, 1, 1, "done", "", "", "claude", "idle"),
    ];
    sessions::parse(&l.join("\n"))
}

#[test]
fn a_session_waits_as_long_as_its_longest_waiting_pane() {
    let v = waiting();
    let wait = |id: &str| v.iter().find(|s| s.id == id).unwrap().wait(NOW);
    assert_eq!((wait("$1"), wait("$2"), wait("$3"), wait("$4")), (Some(30), None, Some(250), None));
}

#[test]
fn a_stamp_counts_only_on_a_waiting_agent_pane() {
    let l = [
        // a stale stamp on a pane that moved on, and one on a pane that is not an agent
        rec_since("$1", 0, 1, 0, 1, 1, "running", "", "100", "a", "s"),
        rec_since("$1", 0, 1, 1, 0, 0, "needs-input", "?", "200", "sh", "s"),
        rec_since("$2", 0, 1, 0, 1, 1, "needs-input", "?", "junk", "a", "t"),
    ];
    let v = sessions::parse(&l.join("\n"));
    assert_eq!((v[0].since(), v[1].since(), v[1].state()), (None, None, Some(State::NeedsInput)));
}

#[test]
fn a_stamp_from_the_future_waits_zero() {
    let ahead = (NOW + 60).to_string();
    let v = sessions::parse(&rec_since("$1", 0, 1, 0, 1, 1, "needs-input", "?", &ahead, "a", "s"));
    assert_eq!(v[0].wait(NOW), Some(0));
}

#[test]
fn waiting_sorts_longest_first_then_the_unstamped_then_failed_then_the_rest() {
    let mut v = waiting();
    sessions::order(&mut v, None, NOW);
    let n: Vec<&str> = v.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(n, ["long", "short", "unstamped", "failed", "idle"]);
}

#[test]
fn a_waiting_row_says_waiting_and_for_how_long_when_wide() {
    let mut v = View::new(waiting(), &args(false, None), true, NOW, Glyphs::unicode());
    let out = draw_at(&mut v, 120, 14);
    let row = |name: &str| out.lines().find(|l| l.contains(name)).unwrap_or_default().to_string();
    assert!(row(" long ").contains("waiting 4m10s"), "{out}");
    assert!(row(" short ").contains("waiting 30s"), "{out}");
    // no stamp, no label: its age is the last-used one, as before
    assert!(
        !row(" unstamped ").contains("waiting") && row(" unstamped ").contains("15m"),
        "{out}"
    );
    assert!(!row(" idle ").contains("waiting"), "{out}");
}

#[test]
fn a_narrow_row_drops_the_label_before_the_name() {
    let mut v = View::new(waiting(), &args(false, None), true, NOW, Glyphs::unicode());
    let out = draw_at(&mut v, 60, 14);
    assert!(!out.contains("waiting"), "{out}");
    let row = out.lines().find(|l| l.contains("unstamped")).map(str::to_string);
    assert!(row.is_some(), "the name is whole:\n{out}");
    let long = out.lines().find(|l| l.contains(" long ")).unwrap_or_default();
    assert!(long.contains("4m10s"), "the bare wait stays:\n{out}");
}

#[test]
fn a_popup_too_narrow_for_the_label_keeps_the_long_counts() {
    // a 100-column popup gives the list 54: the wide tier, but not the 8 more the label needs
    let mut v = View::new(waiting(), &args(false, None), true, NOW, Glyphs::unicode());
    let out = draw_at(&mut v, 100, 14);
    let long = out.lines().find(|l| l.contains(" long ")).unwrap_or_default();
    assert!(
        long.contains("2 agents") && long.contains("4m10s") && !out.contains("waiting"),
        "{out}"
    );
    let mut v = View::new(waiting(), &args(false, None), true, NOW, Glyphs::unicode());
    assert!(draw_at(&mut v, 110, 14).contains("waiting 4m10s"));
}

#[test]
fn your_own_waiting_session_widens_no_row() {
    let two = |since: &str| {
        let l = [
            rec_since("$1", 0, 5, 0, 1, 1, "needs-input", "?", since, "claude", "mine"),
            rec_since("$2", 0, 65, 0, 1, 1, "done", "", "", "claude", "other"),
        ];
        let mut v = View::new(
            sessions::parse(&l.join("\n")),
            &args(false, Some("$1")),
            true,
            NOW,
            Glyphs::unicode(),
        );
        draw_at(&mut v, 120, 14)
    };
    // it prints no age, so it draws as if it carried no stamp
    assert_eq!(two(&(NOW - 30).to_string()), two(""));
}
