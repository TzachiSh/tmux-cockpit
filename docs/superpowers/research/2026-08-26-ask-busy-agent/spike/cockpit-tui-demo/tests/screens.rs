//! TestBackend snapshots. No tmux, no transcripts, no clock: every fixture is
//! a literal, so a failure means the layout moved and nothing else.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
use cockpit_tui::data::Pane;
use cockpit_tui::theme::Theme;
use cockpit_tui::{ask, fleet, info};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::widgets::TableState;

fn golden(name: &str, got: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    let path = format!("{dir}/{name}.txt");
    if std::env::var("COCKPIT_BLESS").as_deref() == Ok("1") {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path}; run COCKPIT_BLESS=1 cargo test"));
    assert_eq!(want, got, "\n{name} changed:\n--- want ---\n{want}\n--- got ---\n{got}");
}

fn draw(w: u16, h: u16, f: impl FnOnce(&mut ratatui::Frame)) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(f).unwrap();
    t.backend().to_string()
}

fn pane(id: &str, wher: &str, title: &str, state: &str) -> Pane {
    Pane {
        id: id.into(),
        wher: wher.into(),
        title: title.into(),
        state: state.into(),
        cmd: "claude".into(),
        ..Default::default()
    }
}

/// The five agents of screens.md §2's mockups.
fn fixture() -> Vec<Pane> {
    let mut a = pane("%9", "henosis:1", "Openvpn setup", "needs-input");
    a.msg = "permission: Bash(sudo openvpn --config /etc/openvpn/work.conf --daemon --log /var/log/ovpn.log)".into();
    a.pm = "—".into();

    let mut b = pane("%14", "henosis:3", "Implement with test review and rig", "running");
    b.doing = "Bash: shellcheck on cockpit.tmux and tests/run.sh".into();
    b.said = "While the suite and rig run: shellcheck on the two scripts, and the independent review pass on the diff.".into();
    b.asked = "do it review it test rig and implaments".into();
    b.since = Some(32);
    b.idle_since = Some(432);
    b.asked_since = Some(2460);
    b.loc = "cockpit⑂lowercase-keys".into();
    b.pm = "+214 -63".into();
    b.review = "pending".into();
    b.sess = "098c323f-5ffe-4309-a3dc-996d24d6f00a".into();
    b.name = "Claude Code".into();
    b.path = "/home/u/.cache/cockpit-wt/lowercase-keys".into();
    b.root = "/home/u/.cache/cockpit-wt/lowercase-keys".into();
    b.spool = "/home/u/.local/state/tmux-cockpit/review/14-3187720301".into();
    b.delivered = 2;

    let mut c = pane("%21", "work:2", "Fleet gate review", "running");
    c.doing = "Agent: whole-branch review of phase 1".into();
    c.since = Some(252);
    c.loc = "cockpit⑂fleet-timing".into();
    c.pm = "+12 -4".into();

    let mut d = pane("%30", "work:4", "Left click issue", "done");
    d.doing = "idle · handover pending".into();
    d.since = Some(720);
    d.loc = "cockpit⑂master".into();
    d.pm = "+3 -0".into();

    let mut e = pane("%31", "work:5", "", "");
    e.loc = "cockpit⑂master".into();
    e.pm = "clean".into();

    vec![a, b, c, d, e]
}

fn fleet_at(w: u16) -> String {
    let ps = fixture();
    let th = Theme::fixed();
    let mut st = TableState::new();
    let v = fleet::View { panes: &ps, sel: 1, th: &th, preview: "$ make check\n1001 assertions, 0 failures", overlay: None };
    draw(w, 16, |f| fleet::render(f, f.area(), &v, &mut st))
}

#[test]
fn fleet_profile_a_50() {
    golden("fleet-50", &fleet_at(52)); // R = 50
}

#[test]
fn fleet_profile_b_77() {
    golden("fleet-77", &fleet_at(79)); // R = 77
}

#[test]
fn fleet_profile_c_106() {
    golden("fleet-106", &fleet_at(108)); // R = 106
}

#[test]
fn fleet_too_narrow() {
    let out = fleet_at(30);
    assert!(out.contains("terminal too narrow"), "{out}");
    assert!(out.contains("prefix i on the pane"), "{out}");
}

#[test]
fn needs_you_overlay() {
    let ps = fixture();
    let th = Theme::fixed();
    let mut st = TableState::new();
    let n = fleet::Needs {
        id: "%9".into(),
        wher: "henosis:1".into(),
        title: "Openvpn setup".into(),
        msg: "Bash(sudo openvpn --config /etc/openvpn/work.conf --daemon)".into(),
        left: 7,
    };
    let v = fleet::View { panes: &ps, sel: 0, th: &th, preview: "", overlay: Some(&n) };
    golden("needs-you", &draw(79, 16, |f| fleet::render(f, f.area(), &v, &mut st)));
}

#[test]
fn info_54() {
    let p = fixture().remove(1);
    let th = Theme::fixed();
    golden("info-54", &draw(56, 20, |f| info::render(f, f.area(), &p, &th)));
}

#[test]
fn info_82() {
    let p = fixture().remove(1);
    let th = Theme::fixed();
    golden("info-82", &draw(84, 20, |f| info::render(f, f.area(), &p, &th)));
}

#[test]
fn ask_answer() {
    let p = fixture().remove(1);
    let th = Theme::fixed();
    let phase = ask::Phase::Done(ask::Answer {
        text: "Waiting on a code review report it dispatched earlier. The suite passed (1014/0, two runs) and the rig showed no delta vs master. Nothing is blocked — the last tool call returned 32s ago.".into(),
        cost: 0.05,
        ms: 7000,
    });
    let v = ask::View { pane: &p, q: "is it stuck or waiting on something?", cursor: 0, phase: &phase, th: &th };
    golden("ask-82", &draw(84, 14, |f| ask::render(f, f.area(), &v)));
}

/// The collapse order of screens.md §2: loc first, then ±, then where. A width
/// never drops a later column while keeping an earlier one.
#[test]
fn collapse_order() {
    const ORDER: [&str; 3] = ["loc", "pm", "where"];
    let mut prev = 0usize;
    for r in (30..=200).rev() {
        let d = fleet::dropped(r);
        assert_eq!(d, &ORDER[..d.len()], "at R={r} the dropped set is not a prefix of the collapse order");
        assert!(d.len() >= prev, "at R={r} a column came back as the width shrank");
        prev = d.len();
    }
    assert_eq!(fleet::dropped(106), Vec::<&str>::new());
    assert_eq!(fleet::dropped(77), vec!["loc"]);
    assert_eq!(fleet::dropped(50), vec!["loc", "pm", "where"]);
}

/// Every profile fits its width up to the last column, which screens.md marks
/// "may overflow": at R=68 profile B's fixed columns take 65 and ± spills.
#[test]
fn profiles_fit() {
    for r in [50u16, 67, 68, 99, 100, 160] {
        let p = fleet::profile(r);
        let mut cols: Vec<u16> = [fleet::MARK, p.wher, p.title, p.doing, p.age, p.loc, p.pm].into_iter().filter(|w| *w > 0).collect();
        cols.pop();
        let total: u16 = cols.iter().sum::<u16>() + cols.len() as u16;
        assert!(total <= r, "profile {} needs {total} before its last column at R={r}", p.name);
    }
}

#[test]
fn truncation_marker_and_wrap() {
    use cockpit_tui::data::{trunc, trunc_left, wrap};
    assert_eq!(trunc("abcdef", 4), "abc…");
    assert_eq!(trunc("abc", 4), "abc");
    assert_eq!(trunc_left("/a/b/c/d", 5), "…/c/d");
    assert_eq!(wrap("one two three", 9, 2), vec!["one two", "three"]);
    // text remains past the line cap, so the last line takes the … marker
    assert_eq!(wrap("one two three four", 9, 2), vec!["one two", "three fo…"]);
}

/// The rail is 5 columns wide (screens.md §0). A duration that needs 6 is
/// clipped by the Table and loses its leading digit, so no duration may.
#[test]
fn rail_never_exceeds_five_columns() {
    use cockpit_tui::data::fmt_dur;
    for s in [0u64, 1, 59, 60, 61, 599, 600, 605, 3599, 3600, 3661, 35999, 36000, 86399, 86400, 999_999] {
        let d = fmt_dur(s);
        assert!(d.chars().count() <= 5, "fmt_dur({s}) = {d:?} is {} columns", d.chars().count());
    }
    assert_eq!(fmt_dur(32), "32s");
    assert_eq!(fmt_dur(432), "7m12s");
    assert_eq!(fmt_dur(605), "10m");
    assert_eq!(fmt_dur(3840), "1h04m");
    assert_eq!(fmt_dur(86399), "23h");
    assert_eq!(fmt_dur(3 * 86400), "3d");
}
