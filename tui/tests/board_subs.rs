//! #313: the subagent badge, the tree lines under an agent, their budget, and the
//! keys that never select them. No tmux, no clock: `NOW` is the read's clock.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
use cockpit_tui::board::{self, Act, FindKey, Nav, Row, Sel, View};
use cockpit_tui::data::Pane;
use cockpit_tui::data::subagents::{Sub, SubKind, SubState};
use cockpit_tui::theme::{Glyphs, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::collections::BTreeSet;

const NOW: u64 = 1_800_000_000;

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

fn draw(w: u16, h: u16, f: impl FnOnce(&mut ratatui::Frame)) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(f).unwrap();
    t.backend().to_string()
}

fn lines(screen: &str) -> Vec<String> {
    screen
        .lines()
        .map(|l| l.strip_prefix('"').and_then(|l| l.strip_suffix('"')).unwrap_or(l).to_string())
        .collect()
}

fn sub(ty: &str, desc: &str, state: SubState, ago: u64, tokens: u64) -> Sub {
    Sub {
        id: format!("{ty}-{ago}"),
        kind: SubKind::Subagent,
        ty: ty.into(),
        desc: desc.into(),
        state,
        started: Some(NOW - ago),
        tokens,
    }
}

fn agent(id: &str, wher: &str, title: &str, state: &str, line: &str) -> Pane {
    let mut p = Pane {
        id: id.into(),
        wher: wher.into(),
        title: title.into(),
        state: state.into(),
        cmd: "claude".into(),
        since: Some(63),
        ..Default::default()
    };
    if matches!(state, "needs-input" | "error") {
        p.msg = line.into();
    } else {
        p.doing = line.into();
    }
    p
}

/// The design's mockup: `tests` runs 2 subagents, 1 failed this turn, 1 done
/// lately and 2 done long ago; 1 more failed before the last prompt.
fn mock() -> Vec<Pane> {
    use SubState::*;
    let mut server = agent("%3", "api:0", "server", "needs-input", "Do you want to run npm test?");
    server.since = Some(252);
    let mut tests = agent("%4", "api:1", "tests", "running", "Bash: cargo test --workspace");
    (tests.loc, tests.pm) = ("api⑂main".into(), "+42 -7".into());
    tests.asked_since = Some(300);
    tests.subs = vec![
        sub("Explore", "list the crates", Done, 1200, 30_000),
        sub("general-purpose", "an earlier turn's failure", Failed, 900, 500),
        sub("Plan", "an old plan", Done, 800, 2_000),
        sub("Explore", "find auth callers in src/auth", Running, 132, 12_400),
        sub("Explore", "map callers", Done, 95, 9_000),
        sub("general-purpose", "fix lint in tui/src", Failed, 68, 842),
        sub("Plan", "draft the migration plan", Running, 45, 3_100),
    ];
    let mut migrate = agent("%5", "api:3", "migrate", "running", "Edit src/db/migrate.rs");
    migrate.subs_n = 1;
    let docs = agent("%7", "web:0", "docs", "", "");
    vec![docs, migrate, tests, server]
}

fn at(ps: &[Pane], id: &str) -> usize {
    ps.iter().position(|p| p.id == id).unwrap()
}

fn nav(ps: &[Pane], sel: &str, open: &[&str], room: usize) -> Nav {
    let mut n = Nav::new(Sel::Pane(sel.into()), BTreeSet::new());
    n.now = NOW;
    n.trees = open.iter().map(|s| s.to_string()).collect();
    n.settle(ps, room);
    n
}

/// The rows as text: `[api]`, a pane id, `<pane>/<desc>`, `<pane>:done N`, `<pane>:+N`.
fn shown(ps: &[Pane], n: &Nav) -> Vec<String> {
    n.rows(ps)
        .into_iter()
        .map(|r| match r {
            Row::Head(s) => format!("[{s}]"),
            Row::Agent(i) => ps[i].id.clone(),
            Row::Sub(i, j) => format!("{}/{}", ps[i].id, ps[i].subs[j].desc),
            Row::Done(i, k) => format!("{}:done {k}", ps[i].id),
            Row::More(i, k) => format!("{}:+{k}", ps[i].id),
        })
        .collect()
}

fn tree_of(ps: &[Pane], n: &Nav, id: &str) -> Vec<String> {
    let pre = format!("{id}/");
    let more = format!("{id}:");
    shown(ps, n)
        .into_iter()
        .filter(|s| s.starts_with(&pre) || s.starts_with(&more))
        .collect()
}

/// The extra cells as text; a pane with no `loc` ends in the ` no repo` stat.
fn extra(p: &Pane, th: &Theme, room: u16) -> String {
    board::row_extra(p, th, room, NOW).iter().map(|s| s.content.to_string()).collect()
}

/// The first extra cell, which is the badge when there is one.
fn badge(p: &Pane, th: &Theme, room: u16) -> String {
    board::row_extra(p, th, room, NOW)
        .first()
        .map(|s| s.content.to_string())
        .unwrap_or_default()
}

fn badge_pane(run: usize, failed: usize, done: usize) -> Pane {
    let mut p = Pane::default();
    let k = |n, st| std::iter::repeat_n(sub("Explore", "x", st, 30, 1), n);
    p.subs = k(run, SubState::Running)
        .chain(k(failed, SubState::Failed))
        .chain(k(done, SubState::Done))
        .collect();
    p
}

fn ascii() -> Theme {
    Theme {
        g: Glyphs::ascii(),
        ..Theme::fixed()
    }
}

// ---------------------------------------------------------------- the badge

#[test]
/// Running and failed only: a done subagent never counts in the badge.
fn the_badge_has_two_tiers_by_room() {
    let (p, th) = (badge_pane(2, 1, 1), Theme::fixed());
    assert_eq!(badge(&p, &th, 40), " ✦2·1✕");
    assert_eq!(badge(&p, &th, 6), " ✦2·1✕", "a cell leaves one column of air");
    assert_eq!(badge(&p, &th, 5), " ✦3", "the narrow tier is running plus failed");
    assert_eq!(badge(&p, &th, 3), " ✦3");
    assert_eq!(badge(&p, &th, 2), "");
}

#[test]
fn done_subagents_alone_show_no_badge() {
    assert_eq!(extra(&badge_pane(0, 0, 3), &Theme::fixed(), 40), " no repo");
    assert_eq!(badge(&badge_pane(2, 0, 3), &Theme::fixed(), 40), " ✦2");
    assert_eq!(badge(&badge_pane(2, 0, 3), &Theme::fixed(), 3), " ✦2", "done is not in the total");
}

#[test]
fn the_badge_in_the_ascii_and_nerd_tiers() {
    let p = badge_pane(2, 1, 1);
    assert_eq!(badge(&p, &ascii(), 40), " &2.1x");
    assert_eq!(badge(&p, &ascii(), 3), " &3");
    let nerd = Theme {
        g: Glyphs::nerd(),
        ..Theme::fixed()
    };
    assert_eq!(badge(&p, &nerd, 40), " \u{f52e}2·1✕");
}

#[test]
fn no_subagents_no_badge() {
    let p = badge_pane(0, 0, 0);
    assert_eq!(extra(&p, &Theme::fixed(), 40), " no repo");
}

#[test]
fn the_badge_goes_before_the_stat_and_branch() {
    let mut p = badge_pane(1, 0, 0);
    (p.loc, p.pm) = ("api⑂main".into(), "+42 -7".into());
    assert_eq!(extra(&p, &Theme::fixed(), 40), " ✦1 +42 -7 ⑂main");
    assert_eq!(extra(&p, &Theme::fixed(), 6), " ✦1", "the stat drops first");
}

#[test]
fn with_no_subagent_files_the_hook_count_is_the_running_count() {
    let p = Pane {
        subs_n: 3,
        ..Default::default()
    };
    assert_eq!(badge(&p, &Theme::fixed(), 40), " ✦3");
}

#[test]
fn the_badge_is_the_failed_role_when_one_failed() {
    let th = Theme::fixed();
    let style = |p: &Pane| board::row_extra(p, &th, 40, NOW)[0].style.fg;
    assert_eq!(style(&badge_pane(2, 1, 0)), Some(th.error));
    assert_eq!(style(&badge_pane(2, 0, 1)), Some(th.running));
}

/// A failure from before the agent's last prompt is an earlier turn's: off the badge.
#[test]
fn a_failure_before_the_last_prompt_leaves_the_badge() {
    let mut p = badge_pane(1, 0, 0);
    p.subs.push(sub("Plan", "old", SubState::Failed, 900, 1));
    p.asked_since = Some(300);
    assert_eq!(badge(&p, &Theme::fixed(), 40), " ✦1");
    p.asked_since = Some(1000);
    assert_eq!(badge(&p, &Theme::fixed(), 40), " ✦1·1✕");
}

/// At 40 columns the badge is 3 cells and the name keeps 6, the message 12.
#[test]
fn the_badge_on_the_row_at_40_60_and_100() {
    let ps = mock();
    let th = Theme::fixed();
    let n = nav(&ps, "%3", &[], 12);
    for (w, want) in [(40u16, "✦3"), (60, "✦2·1✕"), (100, "✦2·1✕ +42 -7 ⑂main")] {
        let out = draw(w, 16, |f| board::render(f, f.area(), &list_view(&ps, &th, &n)));
        let row = lines(&out).into_iter().find(|l| l.contains("tests")).unwrap();
        assert!(row.contains(want), "{w}: {row}");
        if w == 40 {
            assert!(row.contains(" tests  Bash: "), "the name keeps 6: {row}");
        }
    }
}

// ---------------------------------------------------------------- the tree

#[test]
fn trees_never_open_by_themselves() {
    let ps = mock();
    let n = nav(&ps, "%4", &[], 20);
    assert!(n.rows(&ps).iter().all(|r| !r.tree()), "{:?}", shown(&ps, &n));
}

/// Old done subagents fold into one line, first; a failure from before the last
/// prompt is gone; the rest are one line each, oldest first.
#[test]
fn an_open_tree_folds_old_done_and_drops_an_earlier_turns_failure() {
    let ps = mock();
    let n = nav(&ps, "%4", &["%4"], 20);
    assert_eq!(
        tree_of(&ps, &n, "%4"),
        [
            "%4:done 2",
            "%4/find auth callers in src/auth",
            "%4/map callers",
            "%4/fix lint in tui/src",
            "%4/draft the migration plan",
        ]
    );
}

#[test]
fn a_done_subagent_folds_at_two_minutes() {
    let mut ps = mock();
    let i = at(&ps, "%4");
    ps[i].subs = vec![
        sub("Explore", "fresh", SubState::Done, board::SUB_FOLD, 1),
        sub("Explore", "old", SubState::Done, board::SUB_FOLD + 1, 1),
    ];
    let n = nav(&ps, "%4", &["%4"], 20);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4:done 1", "%4/fresh"]);
}

#[test]
fn five_lines_at_most_the_last_saying_how_many_more() {
    let mut ps = mock();
    let i = at(&ps, "%4");
    ps[i].subs = (0..7).map(|k| sub("Explore", &format!("s{k}"), SubState::Running, 10, 1)).collect();
    let n = nav(&ps, "%4", &["%4"], 20);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/s0", "%4/s1", "%4/s2", "%4/s3", "%4:+3"]);
}

#[test]
fn s_toggles_the_selected_agents_tree() {
    let ps = mock();
    let mut n = nav(&ps, "%4", &[], 20);
    n.act(Act::Tree, &ps, 20);
    assert_eq!(n.trees, BTreeSet::from(["%4".to_string()]));
    assert_eq!(tree_of(&ps, &n, "%4").len(), 5);
    assert_eq!(n.pane(&ps).map(|p| p.id.as_str()), Some("%4"), "the selection stays");
    n.act(Act::Tree, &ps, 20);
    assert!(n.trees.is_empty() && tree_of(&ps, &n, "%4").is_empty());
}

#[test]
fn s_on_a_header_toggles_every_tree_in_the_session() {
    let mut ps = mock();
    let i = at(&ps, "%3");
    ps[i].subs = vec![sub("Plan", "p", SubState::Running, 5, 1)];
    let mut n = nav(&ps, "%3", &["%4"], 20);
    n.act(Act::First, &ps, 20);
    n.act(Act::Tree, &ps, 20);
    assert_eq!(
        n.trees,
        BTreeSet::from(["%3".to_string(), "%4".to_string()]),
        "one closed: all open"
    );
    n.act(Act::Tree, &ps, 20);
    assert!(n.trees.is_empty(), "all open: all close");
}

/// The fold shows a folded group's ▲ rows; their trees stay shut, the badge says it.
#[test]
fn a_folded_group_shows_only_badges() {
    let mut ps = mock();
    let i = at(&ps, "%3");
    ps[i].subs = vec![sub("Plan", "p", SubState::Running, 5, 1)];
    let mut n = nav(&ps, "%7", &["%3", "%4"], 20);
    n.folds.insert("api".into());
    n.settle(&ps, 20);
    assert!(shown(&ps, &n).contains(&"%3".to_string()));
    assert!(n.rows(&ps).iter().all(|r| !r.tree()), "{:?}", shown(&ps, &n));
}

#[test]
fn find_reads_subagent_types_and_descriptions() {
    let ps = mock();
    let p = &ps[at(&ps, "%4")];
    assert!(board::find_match(p, "explore"));
    assert!(board::find_match(p, "auth callers"));
    assert!(!board::find_match(&ps[at(&ps, "%3")], "explore"));
}

// ---------------------------------------------------------------- the budget

/// 10 agents with 5 subagents each, every tree open, the ▲ last in board order.
fn crowd() -> Vec<Pane> {
    (0..10)
        .map(|k| {
            let st = if k == 9 { "needs-input" } else { "running" };
            let mut p = agent(&format!("%{}", k + 1), &format!("s:{k}"), &format!("a{k}"), st, "x");
            p.subs = (0..5)
                .map(|j| sub("Explore", &format!("{k}.{j}"), SubState::Running, 10, 1))
                .collect();
            p
        })
        .collect()
}

fn ids(ps: &[Pane]) -> Vec<&str> {
    ps.iter().map(|p| p.id.as_str()).collect()
}

/// Tree lines only fill rows the list leaves empty: with any on screen, every
/// row fits, the ▲ included.
#[test]
fn ten_agents_with_five_subagents_never_push_a_blocked_row_off() {
    let ps = crowd();
    for room in 1..40 {
        let n = nav(&ps, "%1", &ids(&ps), room);
        let rows = n.rows(&ps);
        let k = rows.iter().position(|r| matches!(r, Row::Agent(i) if ps[*i].id == "%10")).unwrap();
        assert!(
            rows.iter().all(|r| !r.tree()) || rows.len() <= room,
            "room {room}: {} rows",
            rows.len()
        );
        assert!(k < room.max(2), "room {room}: ▲ at row {k}");
    }
}

#[test]
fn tree_lines_take_the_slack_and_never_more_than_half_the_room() {
    let ps = crowd();
    let lines = |room| nav(&ps, "%1", &ids(&ps), room).rows(&ps).iter().filter(|r| r.tree()).count();
    assert_eq!(lines(11), 0, "no slack, no lines");
    assert_eq!(lines(14), 3, "the slack: 3 rows");
    assert_eq!(lines(30), 15, "slack 19, capped at half of 30");
}

/// Session `a` holds a ✕ and two running agents, `b` a ▲, every one with 5 subagents.
fn hot() -> Vec<Pane> {
    let mut ps = crowd();
    ps.truncate(4);
    for (p, (wher, st)) in ps
        .iter_mut()
        .zip([("a:0", "error"), ("a:1", "running"), ("a:2", "running"), ("b:0", "needs-input")])
    {
        (p.wher, p.state) = (wher.into(), st.into());
    }
    ps
}

/// A slack of 7 (2 headers and 4 agents in 14): the selected ✕ takes 5, then the
/// ▲ takes 2 ahead of the running agents above it.
#[test]
fn the_selected_agent_gets_lines_first_then_blocked_then_board_order() {
    let ps = hot();
    let n = nav(&ps, "%1", &ids(&ps), 14);
    let owners: Vec<String> = n
        .rows(&ps)
        .iter()
        .filter_map(|r| match r {
            Row::Sub(i, _) | Row::More(i, _) | Row::Done(i, _) => Some(ps[*i].id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(owners, ["%1", "%1", "%1", "%1", "%1", "%4", "%4"], "board order on screen");
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/3.0", "%4:+4"], "a short tree ends in +N more");
    // selected last on screen, it still goes first
    let n = nav(&ps, "%4", &ids(&ps), 14);
    assert_eq!(tree_of(&ps, &n, "%4").len(), 5);
    assert_eq!(tree_of(&ps, &n, "%1"), ["%1/0.0", "%1:+4"]);
}

#[test]
fn the_crowd_selected_first() {
    let ps = crowd();
    // a slack of 7: the selected %3 takes 5, the ▲ %10 the next 2
    let n = nav(&ps, "%3", &ids(&ps), 18);
    let owners: Vec<String> = n
        .rows(&ps)
        .iter()
        .filter_map(|r| match r {
            Row::Sub(i, _) | Row::More(i, _) | Row::Done(i, _) => Some(ps[*i].id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(owners, ["%10", "%10", "%3", "%3", "%3", "%3", "%3"], "board order on screen");
    assert_eq!(tree_of(&ps, &n, "%10"), ["%10/9.0", "%10:+4"], "a short tree ends in +N more");
}

// ---------------------------------------------------------------- never selected

const MOVES: [Act; 21] = [
    Act::Up,
    Act::Down,
    Act::Next,
    Act::Prev,
    Act::First,
    Act::Last,
    Act::NextNeeds,
    Act::PageUp,
    Act::PageDown,
    Act::NextSess,
    Act::PrevSess,
    Act::Tile(1),
    Act::Tile(2),
    Act::Tile(3),
    Act::Tile(4),
    Act::Tile(5),
    Act::Tile(6),
    Act::Tile(7),
    Act::Tile(8),
    Act::Tile(9),
    Act::Toggle,
];

fn on_tree(ps: &[Pane], n: &Nav) -> bool {
    let rows = n.rows(ps);
    rows[n.find(&rows, ps)].tree()
}

/// From every row, every move key lands on a header or an agent.
#[test]
fn no_key_selects_a_tree_line() {
    let ps = mock();
    for room in [8usize, 12, 20] {
        let base = nav(&ps, "%3", &["%4"], room);
        for start in 0..base.rows(&ps).len() {
            for a in MOVES {
                let mut n = base.clone();
                n.act(Act::First, &ps, room);
                for _ in 0..start {
                    n.act(Act::Down, &ps, room);
                }
                n.act(a, &ps, room);
                assert!(!on_tree(&ps, &n), "room {room}, row {start}, {a:?}: {:?}", n.sel);
            }
        }
    }
}

/// ↓ from the agent over a tree goes to the next agent; ↑ comes back to it.
#[test]
fn up_and_down_step_over_a_tree() {
    let ps = mock();
    let mut n = nav(&ps, "%4", &["%4"], 20);
    n.act(Act::Down, &ps, 20);
    assert_eq!(n.pane(&ps).unwrap().id, "%5");
    n.act(Act::Up, &ps, 20);
    assert_eq!(n.pane(&ps).unwrap().id, "%4");
}

#[test]
fn find_moves_skip_tree_lines() {
    let ps = mock();
    let mut n = nav(&ps, "%4", &["%4"], 20);
    n.open_find(&ps, 20);
    n.find_act(FindKey::Type('a'), &ps, 20);
    for k in [
        FindKey::Move(Act::Down),
        FindKey::Move(Act::Up),
        FindKey::Move(Act::Down),
        FindKey::Keep,
    ] {
        n.find_act(k, &ps, 20);
        assert!(!on_tree(&ps, &n), "{k:?}");
    }
}

/// A gone agent's place under its header can be a tree line: in 8 rows `%3`'s
/// tree has 2 (a line and `+2 more`); with `%4` gone it has 3, and `%4`'s
/// offset, 4, is its last line. The agent above it takes the selection.
#[test]
fn a_gone_agents_place_is_never_a_tree_line() {
    let mut ps = mock();
    let i = at(&ps, "%3");
    ps[i].subs = ["p", "q", "r"].map(|d| sub("Plan", d, SubState::Running, 5, 1)).into();
    let mut n = nav(&ps, "%4", &["%3"], 8);
    assert_eq!(n.rows(&ps)[4], Row::Agent(at(&ps, "%4")));
    let gone: Vec<Pane> = ps.iter().filter(|p| p.id != "%4").cloned().collect();
    n.settle(&gone, 8);
    assert!(n.rows(&gone)[4].tree());
    assert_eq!(n.pane(&gone).map(|p| p.id.as_str()), Some("%3"));
    // and its place is %3's row, not the tree line's: with %3 gone, %5 takes it
    let both: Vec<Pane> = gone.iter().filter(|p| p.id != "%3").cloned().collect();
    n.settle(&both, 8);
    assert_eq!(n.pane(&both).map(|p| p.id.as_str()), Some("%5"));
}

// ---------------------------------------------------------------- the viewport

/// A top on a tree line moves to its parent when the selection still fits,
/// else past the tree.
#[test]
fn the_top_row_is_never_a_tree_line() {
    let rows = vec![
        Row::Head("a".into()),
        Row::Agent(0),
        Row::Sub(0, 0),
        Row::Sub(0, 1),
        Row::More(0, 3),
        Row::Agent(1),
        Row::Agent(2),
        Row::Agent(3),
    ];
    assert_eq!(board::view_top(&rows, 3, 5, 5), 1, "up to the parent");
    assert_eq!(board::view_top(&rows, 3, 7, 4), 5, "past the tree");
    assert_eq!(board::view_top(&rows, 0, 7, 4), 5);
    for top in 0..rows.len() {
        for r in [0, 1, 5, 6, 7] {
            for room in 1..9 {
                let t = board::view_top(&rows, top, r, room);
                assert!(!rows[t].tree() && t <= r && r < t + room, "top {top} sel {r} room {room}: {t}");
            }
        }
    }
}

// ---------------------------------------------------------------- the footer and help

fn foot(ps: &[Pane], n: &Nav, w: u16) -> String {
    let th = Theme::fixed();
    let out = draw(w, 16, |f| board::render(f, f.area(), &list_view(ps, &th, n)));
    lines(&out).pop().unwrap()
}

#[test]
fn the_footer_offers_s_on_an_agent_with_subagents() {
    let ps = mock();
    assert!(foot(&ps, &nav(&ps, "%4", &[], 12), 100).contains("s tree"));
    assert!(foot(&ps, &nav(&ps, "%4", &["%4"], 12), 100).contains("s hide"));
    assert!(
        foot(&ps, &nav(&ps, "%4", &["%4"], 12), 60).contains("s hide"),
        "{}",
        foot(&ps, &nav(&ps, "%4", &["%4"], 12), 60)
    );
    assert!(!foot(&ps, &nav(&ps, "%7", &[], 12), 100).contains(" s "), "none without subagents");
}

#[test]
fn the_help_lists_s_and_the_badge() {
    let rows = board::help_rows(&Glyphs::unicode(), true);
    assert!(rows.iter().any(|(k, d)| k == "s" && d.contains("subagent")), "{rows:?}");
    assert!(rows.iter().any(|(k, _)| k == "✦"), "{rows:?}");
    let tiles = board::help_rows(&Glyphs::unicode(), false);
    assert!(!tiles.iter().any(|(k, _)| k == "s"), "a list key");
}

// ---------------------------------------------------------------- the redraw gate

/// The data's hash skips `started`; an open tree's printed ages go in the gate.
#[test]
fn an_open_trees_ages_move_the_redraw_gate() {
    let ps = mock();
    let shut = nav(&ps, "%4", &[], 20);
    let mut later = shut.clone();
    later.now += 1;
    assert_eq!(shut.tree_gate(&ps), later.tree_gate(&ps), "a shut tree prints no age");
    let open = nav(&ps, "%4", &["%4"], 20);
    let mut later = open.clone();
    later.now += 1;
    assert_ne!(open.tree_gate(&ps), later.tree_gate(&ps));
}

// ---------------------------------------------------------------- goldens

fn list_view<'a>(ps: &'a [Pane], th: &'a Theme, nav: &'a Nav) -> View<'a> {
    View {
        panes: ps,
        pvs: &[],
        th,
        sel: 0,
        top: 0,
        active: "%7",
        note: None,
        help: None,
        stale: None,
        lost: false,
        never: false,
        list: Some(nav),
        confirm: None,
    }
}

/// The design's two mockups, `tests`'s tree open under the selected ▲.
#[test]
fn subagent_goldens() {
    let ps = mock();
    for (w, th) in [(40u16, Theme::fixed()), (100, Theme::fixed()), (60, ascii())] {
        let n = nav(&ps, "%3", &["%4"], 12);
        let out = draw(w, 16, |f| board::render(f, f.area(), &list_view(&ps, &th, &n)));
        let name = if th.g.ascii {
            format!("board-subs-ascii-{w}")
        } else {
            format!("board-subs-{w}")
        };
        golden(&name, &out);
        let tree: Vec<String> = lines(&out).into_iter().filter(|l| l.contains("Plan")).collect();
        assert_eq!(tree.len(), 1, "{out}");
        assert!(tree[0].contains("45s"), "the age stays: {}", tree[0]);
    }
    let n = nav(&ps, "%4", &["%4"], 12);
    let th = Theme::fixed();
    let out = draw(100, 16, |f| board::render(f, f.area(), &list_view(&ps, &th, &n)));
    let l = lines(&out);
    assert!(l.iter().any(|l| l.contains("├ ✓ 2 done")), "{out}");
    assert!(l.iter().any(|l| l.contains("12.4k") && l.contains("2m12s")), "{out}");
    assert!(l.iter().any(|l| l.contains("└ ● Plan")), "{out}");
}

#[test]
fn tokens_print_in_at_most_six_cells() {
    let t = board::fmt_tokens;
    assert_eq!(
        [t(0), t(842), t(9_000), t(12_400), t(999_949)],
        ["0", "842", "9k", "12.4k", "999.9k"]
    );
    assert_eq!([t(999_950), t(3_100_000)], ["1M", "3.1M"]);
}

/// As the row narrows the tokens go first, then the description shortens; the
/// type and the age stay.
#[test]
fn a_narrowing_tree_line_drops_tokens_then_description() {
    let ps = mock();
    let th = Theme::fixed();
    let n = nav(&ps, "%4", &["%4"], 12);
    let line = |w: u16| {
        let out = draw(w, 16, |f| board::render(f, f.area(), &list_view(&ps, &th, &n)));
        lines(&out).into_iter().find(|l| l.contains("2m12s")).unwrap()
    };
    let (wide, mid, narrow) = (line(100), line(50), line(40));
    assert!(wide.contains("find auth callers in src/auth") && wide.contains("12.4k"), "{wide}");
    assert!(!mid.contains("12.4k") && mid.contains("find auth call"), "{mid}");
    assert!(narrow.contains("Explore") && narrow.contains("find au…"), "{narrow}");
}

/// Off until the reader has `SubState::Stopped` (#324's data owner adds it); then
/// drop the `cfg` and add its arm in `sub_counts` and `sub_glyph`. A subagent the
/// user stopped is neither failed nor running: no badge, no red, `■` in the tree.
#[cfg(any())]
#[test]
fn a_stopped_subagent_is_not_a_failure() {
    let mut ps = mock();
    let i = at(&ps, "%4");
    ps[i].subs = vec![sub("Plan", "stopped one", SubState::Stopped, 30, 1)];
    assert_eq!(extra(&ps[i], &Theme::fixed(), 40), " +42 -7 ⑂main", "no badge");
    ps[i].subs.push(sub("Plan", "running", SubState::Running, 10, 1));
    let th = Theme::fixed();
    let b = &board::row_extra(&ps[i], &th, 40, NOW)[0];
    assert_eq!((b.content.as_ref(), b.style.fg), (" ✦1", Some(th.running)));
    let n = nav(&ps, "%4", &["%4"], 12);
    let out = draw(100, 16, |f| board::render(f, f.area(), &list_view(&ps, &th, &n)));
    assert!(lines(&out).iter().any(|l| l.contains("├ ■ Plan")), "{out}");
}
