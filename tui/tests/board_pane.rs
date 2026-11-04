//! What the board's worker captures, and (#284) the list's right pane.
//! No tmux, no clock, no env: every fixture is a literal.
use cockpit_tui::board::{self, Want};
use cockpit_tui::data::Pane;
use ratatui::layout::Rect;

fn pane(id: &str, wher: &str, state: &str) -> Pane {
    Pane {
        id: id.into(),
        wher: wher.into(),
        state: state.into(),
        cmd: "claude".into(),
        ..Default::default()
    }
}

/// Eight running agents, `%1`..`%8`, one session each.
fn eight() -> Vec<Pane> {
    (1..=8).map(|n| pane(&format!("%{n}"), &format!("s{n}:0"), "running")).collect()
}

fn want(w: u16, h: u16, tiles: bool) -> Want {
    Want {
        area: Rect::new(0, 0, w, h),
        sel: 0,
        top: 0,
        tiles,
    }
}

fn ids(caps: &[(String, usize)]) -> Vec<&str> {
    caps.iter().map(|(id, _)| id.as_str()).collect()
}

#[test]
fn the_tiles_capture_their_visible_tiles_only() {
    // 100x30: two columns, four rows of two, every row on screen
    let ps = eight();
    let caps = board::captures(&ps, true, &want(100, 30, true));
    assert_eq!(ids(&caps), ["%1", "%2", "%3", "%4", "%5", "%6", "%7", "%8"]);
    let caps = board::captures(&ps, true, &want(100, 8, true));
    assert_eq!(ids(&caps), ["%1", "%2"], "one row of two fits in 8 rows");
}

#[test]
fn a_failed_read_captures_nothing() {
    assert!(board::captures(&eight(), false, &want(100, 30, true)).is_empty());
}
