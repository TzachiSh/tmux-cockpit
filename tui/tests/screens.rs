//! Contracts a golden cannot hold: the collapse order across every width, the
//! rail's column budget, and the redraw hash. No clock and no tmux -- every
//! fixture is a literal, so a failure means the logic moved and nothing else.
//!
//! The board's goldens land here with L2. There is deliberately no golden for a
//! ratatui fleet list: `prefix r` stays fzf.
use cockpit_tui::data::{Pane, hash_rows};
use cockpit_tui::fleet;

/// The collapse order of screens.md §2: loc first, then ±, then where. A width
/// never drops a later column while keeping an earlier one.
#[test]
fn collapse_order() {
    const ORDER: [&str; 3] = ["loc", "pm", "where"];
    let mut prev = 0usize;
    for r in (30..=200).rev() {
        let d = fleet::dropped(r);
        assert_eq!(
            d,
            &ORDER[..d.len()],
            "at R={r} the dropped set is not a prefix of the collapse order"
        );
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
        let mut cols: Vec<u16> = [fleet::MARK, p.wher, p.title, p.doing, p.age, p.loc, p.pm]
            .into_iter()
            .filter(|w| *w > 0)
            .collect();
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

/// The rail is 5 columns wide (screens.md §0). A duration needing 6 is clipped
/// by the row and loses its leading digit, so none may -- u64::MAX included,
/// which is why the day count clamps.
#[test]
fn rail_never_exceeds_five_columns() {
    use cockpit_tui::data::fmt_dur;
    let boundaries = [
        0u64,
        1,
        59,
        60,
        61,
        599,
        600,
        605,
        3599,
        3600,
        3661,
        35999,
        36000,
        86399,
        86400,
        999_999,
        9999 * 86400,
        10_000 * 86400,
        u64::MAX,
    ];
    for s in boundaries {
        let d = fmt_dur(s);
        assert!(d.chars().count() <= 5, "fmt_dur({s}) = {d:?} is {} columns", d.chars().count());
    }
    assert_eq!(fmt_dur(32), "32s");
    assert_eq!(fmt_dur(432), "7m12s");
    assert_eq!(fmt_dur(605), "10m");
    assert_eq!(fmt_dur(3840), "1h04m");
    assert_eq!(fmt_dur(86399), "23h");
    assert_eq!(fmt_dur(3 * 86400), "3d");
    assert_eq!(fmt_dur(u64::MAX), "9999d");
}

/// The redraw gate. `age()` recomputes since/asked_since/idle_since against
/// `now` every tick, so hashing them leaves `dirty` true forever and the board
/// repaints at 1 Hz against a server where nothing happened. No sleep: the ages
/// are simply set to what the next tick would compute.
#[test]
fn hash_ignores_the_ages_but_not_the_facts() {
    fn h(p: &Pane, pv: &str) -> u64 {
        hash_rows(std::slice::from_ref(p), pv)
    }
    let mut a = Pane {
        id: "%14".into(),
        state: "running".into(),
        doing: "Bash: make check".into(),
        since: Some(32),
        asked_since: Some(2460),
        idle_since: Some(432),
        ..Default::default()
    };
    let b = Pane {
        since: Some(33),
        asked_since: Some(2461),
        idle_since: Some(433),
        ..a.clone()
    };
    assert_eq!(h(&a, "pv"), h(&b, "pv"), "one tick of ageing redrew the board");

    // and the gate still notices what actually changed
    a.doing = "Bash: make tui".into();
    assert_ne!(h(&a, "pv"), h(&b, "pv"), "a new tool call did not redraw");
    let c = Pane {
        state: "needs-input".into(),
        ..b.clone()
    };
    assert_ne!(h(&b, "pv"), h(&c, "pv"), "a state flip did not redraw");
    assert_ne!(h(&b, "pv"), h(&b, "other preview"), "a changed preview did not redraw");
}
