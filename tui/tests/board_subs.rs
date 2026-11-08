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
        ..Default::default()
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
    n.seen.now = NOW;
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
            Row::Done(i, k, _) => format!("{}:done {k}", ps[i].id),
            Row::Quiet(i, k) => format!("{}:quiet {k}", ps[i].id),
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
    board::row_extra(p, th, room, &board::Seen::at(NOW))
        .iter()
        .map(|s| s.content.to_string())
        .collect()
}

/// The first extra cell, which is the badge when there is one.
fn badge(p: &Pane, th: &Theme, room: u16) -> String {
    board::row_extra(p, th, room, &board::Seen::at(NOW))
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
    let style = |p: &Pane| board::row_extra(p, &th, 40, &board::Seen::at(NOW))[0].style.fg;
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
/// 95, not 100: at 96 the list shares the board with the right pane (#284).
#[test]
fn the_badge_on_the_row_at_40_60_and_95() {
    let ps = mock();
    let th = Theme::fixed();
    let n = nav(&ps, "%3", &[], 12);
    for (w, want) in [(40u16, "✦3"), (60, "✦2·1✕"), (95, "✦2·1✕ +42 -7 ⑂main")] {
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

/// One more read of `ps` at `now`, as the run loop makes every second.
fn read(n: &mut Nav, ps: &[Pane], now: u64) {
    n.seen.now = now;
    n.settle(ps, 20);
}

/// A done subagent folds 2 minutes after a read first saw it done, however
/// long ago it started: a 10-minute run that just finished is a line.
#[test]
fn a_done_subagent_folds_two_minutes_after_it_is_seen_done() {
    let mut ps = mock();
    let i = at(&ps, "%4");
    ps[i].subs = vec![sub("Explore", "long", SubState::Running, 600, 1)];
    let mut n = nav(&ps, "%4", &["%4"], 20);
    ps[i].subs[0].state = SubState::Done;
    read(&mut n, &ps, NOW + 5);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/long"]);
    read(&mut n, &ps, NOW + 5 + board::SUB_FOLD);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/long"]);
    read(&mut n, &ps, NOW + 6 + board::SUB_FOLD);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4:done 1"]);
}

/// On the board's first read, a done subagent with no start, or one that
/// started before the newest prompt, is old: its finish is unknown.
#[test]
fn a_done_subagent_already_done_at_the_first_read() {
    let mut ps = mock();
    let i = at(&ps, "%4");
    let mut unstarted = sub("Plan", "unstarted", SubState::Done, 0, 1);
    unstarted.started = None;
    ps[i].subs = vec![unstarted, sub("Plan", "this turn", SubState::Done, 200, 1)];
    let n = nav(&ps, "%4", &["%4"], 20);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4:done 1", "%4/this turn"]);
}

/// A background subagent started last turn and failed this one stays until
/// the next prompt.
#[test]
fn a_failure_after_the_prompt_stays_whenever_it_started() {
    let mut ps = mock();
    let i = at(&ps, "%4");
    ps[i].asked_since = Some(60);
    ps[i].subs = vec![sub("Explore", "bg", SubState::Running, 600, 1)];
    let mut n = nav(&ps, "%4", &["%4"], 20);
    ps[i].subs[0].state = SubState::Failed;
    ps[i].asked_since = Some(65);
    read(&mut n, &ps, NOW + 5);
    assert_eq!(board::row_extra(&ps[i], &Theme::fixed(), 40, &n.seen)[0].content, " ✦0·1✕");
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/bg"]);
    // the next prompt clears it from both
    ps[i].asked_since = Some(1);
    read(&mut n, &ps, NOW + 10);
    assert!(tree_of(&ps, &n, "%4").is_empty());
    assert!(!extra(&ps[i], &Theme::fixed(), 40).contains('✦'));
}

/// A prompt out of the transcript's tail reads as no prompt: the board keeps
/// the newest it saw, so an earlier turn's failure stays gone.
#[test]
fn a_prompt_out_of_the_tail_is_remembered() {
    let mut ps = mock();
    let i = at(&ps, "%4");
    ps[i].asked_since = Some(300);
    ps[i].subs = vec![sub("Plan", "old", SubState::Failed, 900, 1)];
    let mut n = nav(&ps, "%4", &["%4"], 20);
    ps[i].asked_since = None;
    read(&mut n, &ps, NOW + 1);
    assert!(!board::row_extra(&ps[i], &Theme::fixed(), 40, &n.seen)[0].content.contains('✦'));
    assert!(tree_of(&ps, &n, "%4").is_empty());
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
            Row::Sub(i, _) | Row::More(i, _) | Row::Done(i, ..) | Row::Quiet(i, _) => Some(ps[*i].id.clone()),
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
            Row::Sub(i, _) | Row::More(i, _) | Row::Done(i, ..) | Row::Quiet(i, _) => Some(ps[*i].id.clone()),
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
    later.seen.now += 1;
    assert_eq!(shut.tree_gate(&ps), later.tree_gate(&ps), "a shut tree prints no age");
    let open = nav(&ps, "%4", &["%4"], 20);
    let mut later = open.clone();
    later.seen.now += 1;
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
    // 95: the widest list with no right pane (#284), room for the token column
    let out = draw(95, 16, |f| board::render(f, f.area(), &list_view(&ps, &th, &n)));
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
    // 95: the widest list with no right pane (#284)
    let (wide, mid, narrow) = (line(95), line(50), line(40));
    assert!(wide.contains("find auth callers in src/auth") && wide.contains("12.4k"), "{wide}");
    assert!(!mid.contains("12.4k") && mid.contains("find auth call"), "{mid}");
    assert!(narrow.contains("Explore") && narrow.contains("find au…"), "{narrow}");
}

// ---------------------------------------------------------------- an ended agent (#301)

/// A crash leaves its subagents `Running` and `@cockpit_subs` set: none runs.
fn ended_pane() -> Vec<Pane> {
    let mut ps = mock();
    let i = at(&ps, "%4");
    ps[i].ended = true;
    ps[i].subs_n = 2;
    ps[i].subs = vec![sub("Explore", "orphan", SubState::Running, 5000, 1)];
    ps
}

#[test]
fn an_ended_agent_counts_no_running_subagent() {
    let ps = ended_pane();
    let p = &ps[at(&ps, "%4")];
    assert!(!extra(p, &Theme::fixed(), 40).contains('✦'), "{}", extra(p, &Theme::fixed(), 40));
    let empty = Pane {
        ended: true,
        subs_n: 2,
        ..Default::default()
    };
    assert!(!extra(&empty, &Theme::fixed(), 40).contains('✦'), "nor the hook's count");
}

#[test]
fn an_ended_agents_failure_shows_dimmed_in_meta() {
    let mut ps = ended_pane();
    let i = at(&ps, "%4");
    ps[i].subs.push(sub("Plan", "broke", SubState::Failed, 10, 1));
    let th = Theme::fixed();
    let b = &board::row_extra(&ps[i], &th, 40, &board::Seen::at(NOW))[0];
    assert_eq!(b.content, " ✦0·1✕");
    assert_eq!(b.style.fg, Some(th.meta));
    assert!(b.style.add_modifier.contains(ratatui::style::Modifier::DIM));
}

/// Its tree's `Running` lines take the ended glyph and print no age.
#[test]
fn an_ended_agents_running_subagent_is_drawn_ended() {
    let ps = ended_pane();
    let th = Theme::fixed();
    let n = nav(&ps, "%3", &["%4"], 12);
    let out = draw(100, 16, |f| board::render(f, f.area(), &list_view(&ps, &th, &n)));
    let line = lines(&out).into_iter().find(|l| l.contains("orphan")).unwrap();
    assert!(line.contains("└ ◌ Explore"), "{line}");
    assert!(!line.contains("1h23m"), "no age ticks: {line}");
    let mut later = n.clone();
    later.seen.now += 60;
    assert_eq!(n.tree_gate(&ps), later.tree_gate(&ps));
}

// ---------------------------------------------------------------- s where no tree can show

#[test]
fn s_on_an_agent_without_subagents_does_nothing() {
    let ps = mock();
    let mut n = nav(&ps, "%5", &[], 20);
    n.act(Act::Tree, &ps, 20);
    assert!(n.trees.is_empty(), "%5 has only the hook's count");
}

/// A ▲ in a folded group shows, its tree does not: `s` neither opens it nor is offered.
#[test]
fn s_is_not_offered_where_the_tree_cannot_show() {
    let ps = mock();
    let mut n = nav(&ps, "%3", &[], 12);
    let i = at(&ps, "%3");
    let mut ps = ps;
    ps[i].subs = vec![sub("Plan", "p", SubState::Running, 5, 1)];
    n.folds.insert("api".into());
    n.settle(&ps, 12);
    assert!(!n.shows(&ps, "%3"));
    n.act(Act::Tree, &ps, 12);
    assert!(n.trees.is_empty(), "{:?}", n.trees);
    assert!(!foot(&ps, &n, 100).contains(" s "), "{}", foot(&ps, &n, 100));
}

/// A list that does not fit leaves no slack for any tree line.
#[test]
fn s_does_nothing_on_a_list_that_does_not_fit() {
    let ps = crowd();
    let mut n = nav(&ps, "%1", &[], 11);
    assert!(!n.shows(&ps, "%1"));
    n.act(Act::Tree, &ps, 11);
    assert!(n.trees.is_empty());
    let n = nav(&ps, "%1", &[], 14);
    assert!(n.shows(&ps, "%1"));
}

/// `/clear` empties the subagents: the tree shuts, and the new session's first
/// subagent does not open it again.
#[test]
fn a_tree_whose_subagents_go_shuts() {
    let mut ps = mock();
    let i = at(&ps, "%4");
    let mut n = nav(&ps, "%4", &["%4"], 20);
    let subs = std::mem::take(&mut ps[i].subs);
    n.settle(&ps, 20);
    ps[i].subs = subs;
    n.settle(&ps, 20);
    assert!(tree_of(&ps, &n, "%4").is_empty(), "{:?}", n.trees);
}

// ---------------------------------------------------------------- pins

/// The data's hash keeps tokens by the thousand; the line prints `12.4k`.
#[test]
fn an_open_trees_tokens_move_the_redraw_gate() {
    let mut ps = mock();
    let n = nav(&ps, "%4", &["%4"], 20);
    let before = n.tree_gate(&ps);
    let i = at(&ps, "%4");
    let j = ps[i].subs.iter().position(|s| s.tokens == 12_400).unwrap();
    ps[i].subs[j].tokens = 12_900;
    assert_ne!(before, n.tree_gate(&ps));
}

/// A one-line tree is `+N more`, the fold's N counted in.
#[test]
fn a_one_line_tree_counts_every_subagent() {
    let ps = mock();
    let n = nav(&ps, "%4", &["%4"], 7);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4:+6"], "2 folded and 4 lines");
}

/// The open find field shows a folded group unfolded (D7), its open trees too.
#[test]
fn the_find_field_shows_a_folded_groups_trees() {
    let ps = mock();
    let mut n = nav(&ps, "%4", &["%4"], 20);
    n.folds.insert("api".into());
    n.open_find(&ps, 20);
    for c in "tests".chars() {
        n.find_act(FindKey::Type(c), &ps, 20);
    }
    assert_eq!(tree_of(&ps, &n, "%4").len(), 5, "{:?}", shown(&ps, &n));
    n.find_act(FindKey::Cancel, &ps, 20);
    assert!(tree_of(&ps, &n, "%4").is_empty(), "the fold holds again");
}

#[test]
fn a_user_stop_counts_as_neither_running_nor_failed() {
    let mut p = agent("%9", "api:9", "t", "running", "x");
    p.subs = vec![sub("Plan", "stopped", SubState::Stopped, 30, 1)];
    let mut seen = board::Seen::at(NOW);
    seen.observe(std::slice::from_ref(&p));
    assert_eq!(board::sub_counts(&p, &seen), (0, 0));
}

/// The Cause of #336: a stopped subagent was drawn as failed. Not Done's exact glyph: the
/// tree's follow-up gives Stopped one of its own.
#[test]
fn a_stopped_sub_is_not_drawn_with_the_error_glyph_or_role() {
    let mut p = agent("%4", "api:1", "tests", "running", "x");
    p.asked_since = Some(300);
    p.subs = vec![sub("Plan", "stopped", SubState::Stopped, 30, 1)];
    let ps = vec![p];
    let (n, th) = (nav(&ps, "%4", &["%4"], 12), Theme::fixed());
    let mut t = Terminal::new(TestBackend::new(95, 8)).unwrap();
    t.draw(|f| board::render(f, f.area(), &list_view(&ps, &th, &n))).unwrap();
    let buf = t.backend().buffer().clone();
    let row = (0..buf.area.height)
        .find(|&y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains("Plan")
        })
        .expect("the stopped sub is drawn");
    let cells: Vec<_> = (0..buf.area.width).map(|x| &buf[(x, row)]).collect();
    assert!(cells.iter().all(|c| c.symbol() != th.g.error), "drawn with {}", th.g.error);
    assert!(cells.iter().all(|c| c.fg != th.error), "drawn in the error role");
}

/// A failure stays a line until the agent's next prompt, however old. A stopped sub is not
/// kept so; when it leaves the tree is the tree's business (it folds on its finish time).
#[test]
fn a_stopped_sub_is_not_kept_like_failed_until_the_next_prompt() {
    let mut ps = mock();
    let i = at(&ps, "%4");
    ps[i].asked_since = Some(60);
    ps[i].subs = vec![sub("Explore", "bg", SubState::Running, 600, 1)];
    let mut n = nav(&ps, "%4", &["%4"], 20);
    ps[i].subs[0].state = SubState::Stopped;
    ps[i].asked_since = Some(65);
    read(&mut n, &ps, NOW + 5);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/bg"], "a line while it is fresh");
    read(&mut n, &ps, NOW + 10 * board::SUB_FOLD);
    assert!(
        !tree_of(&ps, &n, "%4").contains(&"%4/bg".to_string()),
        "{:?}",
        tree_of(&ps, &n, "%4")
    );
}

/// The hooks' two new states (#361): a teammate between turns, and an agent whose end nobody
/// recorded. Neither is running, and neither is a failure.
#[test]
fn an_idle_or_end_unknown_sub_counts_as_neither_running_nor_failed() {
    for st in [SubState::Idle, SubState::EndUnknown] {
        let mut p = agent("%9", "api:9", "t", "running", "x");
        p.subs = vec![sub("Plan", "quiet", st, 30, 1)];
        let mut seen = board::Seen::at(NOW);
        seen.observe(std::slice::from_ref(&p));
        assert_eq!(board::sub_counts(&p, &seen), (0, 0), "{st:?}");
    }
}

// ---------------------------------------------------------------- #361: the states' own looks

/// `tests`, alone in its session, with `subs` and its tree open, prompted 300 s ago.
fn one(subs: Vec<Sub>) -> Vec<Pane> {
    let mut p = agent("%4", "api:1", "tests", "running", "x");
    p.asked_since = Some(300);
    p.subs = subs;
    vec![p]
}

fn with(mut s: Sub, finished: Option<u64>, active: Option<u64>) -> Sub {
    (s.finished_at, s.active_at) = (finished.map(|a| NOW - a), active.map(|a| NOW - a));
    s
}

/// The tree line holding `text`, as cells.
fn cells_of(ps: &[Pane], th: &Theme, text: &str) -> Vec<ratatui::buffer::Cell> {
    let n = nav(ps, "%4", &["%4"], 12);
    let mut t = Terminal::new(TestBackend::new(100, 10)).unwrap();
    t.draw(|f| board::render(f, f.area(), &list_view(ps, th, &n))).unwrap();
    let buf = t.backend().buffer().clone();
    let y = (0..buf.area.height)
        .find(|&y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().contains(text))
        .unwrap_or_else(|| panic!("no line holds {text}"));
    (0..buf.area.width).map(|x| buf[(x, y)].clone()).collect()
}

fn text(cells: &[ratatui::buffer::Cell]) -> String {
    cells.iter().map(|c| c.symbol()).collect()
}

/// Stopped (#336) has its own mark, `∎` (`=` in ascii since #399: `#` is review
/// pending's), in meta and dim.
#[test]
fn a_stopped_sub_has_its_own_dim_mark() {
    let ps = one(vec![with(sub("Plan", "halted", SubState::Stopped, 30, 1), Some(10), None)]);
    let th = Theme::fixed();
    let c = cells_of(&ps, &th, "halted");
    let mark = c.iter().find(|c| c.symbol() == "∎").unwrap_or_else(|| panic!("{}", text(&c)));
    assert_eq!(mark.fg, th.meta);
    assert!(mark.modifier.contains(ratatui::style::Modifier::DIM));
    assert!(
        text(&cells_of(&ps, &ascii(), "halted")).contains("|- = Plan") || text(&cells_of(&ps, &ascii(), "halted")).contains("`- = Plan")
    );
}

/// Idle (#361) is a teammate between turns: alive, its own mark, aged from its last sign
/// of life, and never folded however long it waits.
#[test]
fn an_idle_teammate_stays_a_line_with_its_own_mark() {
    let mut ps = one(vec![with(sub("mate", "between turns", SubState::Idle, 900, 1), None, Some(40))]);
    let th = Theme::fixed();
    let line = text(&cells_of(&ps, &th, "between turns"));
    assert!(line.contains("└ ◦ mate") && line.contains("40s") && !line.contains("15m"), "{line}");
    assert!(text(&cells_of(&ps, &ascii(), "between turns")).contains("`- o mate"));
    let mut n = nav(&ps, "%4", &["%4"], 20);
    ps[0].asked_since = Some(300 + 10 * board::SUB_FOLD);
    read(&mut n, &ps, NOW + 10 * board::SUB_FOLD);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/between turns"]);
}

/// EndUnknown (#361) is 15 minutes without a transcript write, past SUB_FOLD by
/// definition: it folds at once, into its own line, never into done, in no red.
#[test]
fn end_unknown_subs_fold_into_their_own_quiet_line() {
    let ps = one(vec![
        with(sub("Explore", "q1", SubState::EndUnknown, 4000, 1), None, Some(3000)),
        with(sub("Explore", "q2", SubState::EndUnknown, 2000, 1), None, Some(960)),
        with(sub("Plan", "ok", SubState::Done, 100, 1), Some(5), None),
    ]);
    let n = nav(&ps, "%4", &["%4"], 12);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4:quiet 2", "%4/ok"]);
    let th = Theme::fixed();
    let c = cells_of(&ps, &th, "2 quiet");
    let line = text(&c);
    assert!(
        line.contains("├ ◌ 2 quiet") && line.contains("16m") && !line.contains("50m"),
        "the newest sign of life: {line}"
    );
    assert!(c.iter().all(|c| c.fg != th.error && c.fg != th.needs), "{line}");
}

/// The owner's session 51267349: 16 done, 51 end unknown, 3 idle teammates. Two fold
/// lines and the three teammates: five lines, nothing swamped.
#[test]
fn the_owners_session_fits_its_five_lines() {
    let mut subs = vec![];
    for k in 0..16 {
        subs.push(with(
            sub("Explore", &format!("d{k}"), SubState::Done, 5000 + k, 1),
            Some(4000),
            None,
        ));
    }
    for k in 0..51 {
        subs.push(with(
            sub("Explore", &format!("u{k}"), SubState::EndUnknown, 4000 + k, 1),
            None,
            Some(2000),
        ));
    }
    for k in 0..3 {
        subs.push(with(sub("mate", &format!("i{k}"), SubState::Idle, 3000 + k, 1), None, Some(30)));
    }
    let ps = one(subs);
    let n = nav(&ps, "%4", &["%4"], 12);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4:done 16", "%4:quiet 51", "%4/i0", "%4/i1", "%4/i2"]);
}

/// #399: when the tree's room is short, its fold rows give way first and their
/// counts join `+N more`: the owner's three idle teammates stay, the 16 done and
/// 51 quiet are counted. Two rows keep one teammate.
#[test]
fn a_short_tree_keeps_its_live_lines_before_its_folds() {
    let mut subs = vec![];
    for k in 0..16 {
        subs.push(with(
            sub("Explore", &format!("d{k}"), SubState::Done, 5000 + k, 1),
            Some(4000),
            None,
        ));
    }
    for k in 0..51 {
        subs.push(with(
            sub("Explore", &format!("u{k}"), SubState::EndUnknown, 4000 + k, 1),
            None,
            Some(2000),
        ));
    }
    for k in 0..3 {
        subs.push(with(sub("mate", &format!("i{k}"), SubState::Idle, 3000 + k, 1), None, Some(30)));
    }
    let ps = one(subs);
    let n = nav(&ps, "%4", &["%4"], 8);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/i0", "%4/i1", "%4/i2", "%4:+67"]);
    let n = nav(&ps, "%4", &["%4"], 4);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/i0", "%4:+69"]);
}

/// #399: the done fold names what the user stopped apart from what finished,
/// `✓ 16 done · 2 stopped`, and says nothing of stops when there were none.
#[test]
fn the_done_fold_names_the_stopped_apart() {
    let old = |st, k: u64| with(sub("Explore", &format!("x{k}"), st, 5000 + k, 1), Some(4000), None);
    let mut subs: Vec<_> = (0..16).map(|k| old(SubState::Done, k)).collect();
    subs.extend((16..18).map(|k| old(SubState::Stopped, k)));
    let line = text(&cells_of(&one(subs), &Theme::fixed(), "done"));
    assert!(line.contains("✓ 16 done · 2 stopped"), "{line}");
    let line = text(&cells_of(
        &one((0..16).map(|k| old(SubState::Done, k)).collect()),
        &Theme::fixed(),
        "done",
    ));
    assert!(line.contains("✓ 16 done") && !line.contains("stopped"), "{line}");
}

/// With a finish time, a done sub folds 2 minutes after it, whatever the first sight says.
#[test]
fn a_done_sub_folds_on_its_finish_time() {
    // started after the prompt: first sight would stamp it now and keep it a line
    let ps = one(vec![with(
        sub("Plan", "late", SubState::Done, 200, 1),
        Some(board::SUB_FOLD + 1),
        None,
    )]);
    assert_eq!(tree_of(&ps, &nav(&ps, "%4", &["%4"], 12), "%4"), ["%4:done 1"]);
    // started before the prompt: first sight would stamp it old and fold it
    let ps = one(vec![with(sub("Plan", "fresh", SubState::Done, 900, 1), Some(10), None)]);
    assert_eq!(tree_of(&ps, &nav(&ps, "%4", &["%4"], 12), "%4"), ["%4/fresh"]);
}

/// With a finish time, a failure is an earlier turn's when it finished before the prompt.
#[test]
fn a_failure_clears_on_its_finish_time() {
    // started last turn, failed this one: first sight alone would drop it
    let ps = one(vec![with(sub("Plan", "bg", SubState::Failed, 900, 1), Some(100), None)]);
    let n = nav(&ps, "%4", &["%4"], 12);
    assert_eq!(tree_of(&ps, &n, "%4"), ["%4/bg"]);
    assert_eq!(board::sub_counts(&ps[0], &n.seen), (0, 1));
    // finished before the prompt, though first seen in a known pane after it
    let mut ps = one(vec![]);
    let mut n = nav(&ps, "%4", &["%4"], 12);
    ps[0].subs = vec![with(sub("Plan", "old", SubState::Failed, 900, 1), Some(400), None)];
    read(&mut n, &ps, NOW);
    assert_eq!(board::sub_counts(&ps[0], &n.seen), (0, 0));
}

/// An ended agent's (#301) idle teammate is ended too: nothing waits for a next turn.
#[test]
fn an_ended_agents_idle_teammate_is_drawn_ended() {
    let mut ps = one(vec![with(sub("mate", "orphan", SubState::Idle, 900, 1), None, Some(40))]);
    ps[0].ended = true;
    let line = text(&cells_of(&ps, &Theme::fixed(), "orphan"));
    assert!(line.contains("└ ◌ mate") && !line.contains("40s"), "{line}");
}

/// #399: the quiet line's age is its newest sign of life, `active_at`, which the
/// data's hash does not keep: a newer one alone must move the redraw gate.
#[test]
fn a_quiet_lines_newer_sign_of_life_moves_the_redraw_gate() {
    let mut ps = one(vec![with(sub("Explore", "q1", SubState::EndUnknown, 4000, 1), None, Some(3000))]);
    let n = nav(&ps, "%4", &["%4"], 12);
    let before = n.tree_gate(&ps);
    ps[0].subs[0].active_at = ps[0].subs[0].active_at.map(|t| t + 600);
    assert_ne!(before, n.tree_gate(&ps), "only active_at moved");
}

/// #399: a quiet subagent with no recorded sign of life ages from its start.
#[test]
fn a_quiet_line_with_no_sign_of_life_ages_from_the_start() {
    let ps = one(vec![with(sub("Explore", "q1", SubState::EndUnknown, 1500, 1), None, None)]);
    let line = text(&cells_of(&ps, &Theme::fixed(), "1 quiet"));
    assert!(line.contains("◌ 1 quiet") && line.contains("25m"), "{line}");
}
