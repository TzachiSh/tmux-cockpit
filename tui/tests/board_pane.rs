//! What the board's worker captures, and (#284) the list's right pane.
//! No tmux, no clock, no env: every fixture is a literal.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
use cockpit_tui::board::{self, Nav, Sel, View, Want};
use cockpit_tui::data::Pane;
use cockpit_tui::data::subagents::{Sub, SubKind, SubState};
use cockpit_tui::theme::{Glyphs, Theme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::collections::{BTreeSet, HashMap};

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
        pick: None,
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
    let w = Want {
        pick: Some("%3".into()),
        ..want(100, 30, false)
    };
    assert!(board::captures(&eight(), false, &w).is_empty());
}

// ------------------------------------------------ the split (D1)

#[test]
fn the_list_splits_at_96_columns() {
    assert_eq!(board::SPLIT_W, 96);
    let rows = [
        (0, None),
        (40, None),
        (95, None),
        (96, Some(40)),
        (100, Some(41)),
        (120, Some(48)),
        (180, Some(64)),
        (u16::MAX, Some(64)),
    ];
    for (w, lw) in rows {
        assert_eq!(board::list_w(w), lw, "the list's width at {w}");
    }
}

#[test]
fn the_list_captures_its_selection_only_and_only_when_split() {
    let ps = eight();
    let w = Want {
        pick: Some("%5".into()),
        ..want(100, 30, false)
    };
    let caps = board::captures(&ps, true, &w);
    assert_eq!(caps, [("%5".to_string(), board::pane_rows(w.area))], "one capture, the selection's");
    let narrow = Want {
        pick: Some("%5".into()),
        ..want(95, 30, false)
    };
    assert!(board::captures(&ps, true, &narrow).is_empty(), "no right pane at 95, no capture");
    assert!(
        board::captures(&ps, true, &want(100, 30, false)).is_empty(),
        "no selection, no capture"
    );
    let gone = Want {
        pick: Some("%99".into()),
        ..want(100, 30, false)
    };
    assert!(board::captures(&ps, true, &gone).is_empty(), "a pane that went is not captured");
}

#[test]
fn the_tiles_ignore_the_lists_pick() {
    let ps = eight();
    let w = Want {
        pick: Some("%5".into()),
        ..want(100, 8, true)
    };
    assert_eq!(ids(&board::captures(&ps, true, &w)), ["%1", "%2"]);
}

#[test]
fn the_preview_rows_are_the_pane_body() {
    // top line, frame, five text rows, a blank, the rule, frame, footer
    assert_eq!(board::pane_rows(Rect::new(0, 0, 100, 30)), 20);
    assert_eq!(board::pane_rows(Rect::new(0, 0, 180, 40)), 30);
    assert_eq!(board::pane_rows(Rect::new(0, 0, 100, 8)), 0);
}

// ------------------------------------------------ the right pane

fn th() -> Theme {
    Theme::fixed()
}

fn ascii() -> Theme {
    let mut t = Theme::fixed();
    t.g = Glyphs::ascii();
    t
}

/// A blocked agent with everything the pane shows, a runner and an ended one.
fn trio() -> Vec<Pane> {
    let mut a = pane("%3", "api:0", "needs-input");
    a.title = "server".into();
    a.msg = "Do you want to run npm test?".into();
    a.asked = "add a token-bucket rate limiter to the router".into();
    a.said = "I added the token-bucket limiter and wired it into the router.".into();
    a.path = "/src/api".into();
    a.loc = "api⑂feat/rate-limit".into();
    a.pm = "+42 -7".into();
    a.review = "pending".into();
    a.since = Some(252);
    let mut b = pane("%4", "api:1", "running");
    b.title = "tests".into();
    b.doing = "Bash: cargo test --workspace".into();
    b.since = Some(63);
    let mut c = pane("%5", "web:0", "done");
    c.title = "ui".into();
    c.ended = true;
    vec![a, b, c]
}

fn nav(ps: &[Pane], sel: Sel) -> Nav {
    let mut n = Nav::new(sel, BTreeSet::new());
    n.settle(ps, 26);
    n
}

fn list<'a>(ps: &'a [Pane], pvs: &'a [Option<String>], th: &'a Theme, nav: &'a Nav) -> View<'a> {
    View {
        panes: ps,
        pvs,
        th,
        sel: 0,
        top: 0,
        active: "%4",
        note: None,
        help: None,
        stale: None,
        lost: false,
        never: false,
        list: Some(nav),
        confirm: None,
    }
}

fn buf(w: u16, h: u16, v: &View) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| board::render(f, f.area(), v)).unwrap();
    t.backend().buffer().clone()
}

fn text(b: &Buffer, y: u16, x0: u16, x1: u16) -> String {
    (x0..x1).map(|x| b[(x, y)].symbol()).collect()
}

/// The right pane's rows, its frame included, from x = `x0`.
fn pane_rows_text(b: &Buffer, x0: u16) -> Vec<String> {
    (0..b.area.height).map(|y| text(b, y, x0, b.area.width)).collect()
}

fn pvs_for(ps: &[Pane], id: &str, pv: Option<&str>) -> Vec<Option<String>> {
    ps.iter()
        .map(|p| if p.id == id { pv.map(String::from) } else { Some(String::new()) })
        .collect()
}

#[test]
fn at_95_the_list_takes_the_whole_width_and_at_96_it_splits() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%3".into()));
    let pvs = pvs_for(&ps, "%3", Some("hello"));
    let b = buf(95, 30, &list(&ps, &pvs, &th, &n));
    assert_eq!(b[(94, 1)].symbol(), "┐", "the list's frame ends at the edge");
    let b = buf(96, 30, &list(&ps, &pvs, &th, &n));
    assert_eq!(b[(39, 1)].symbol(), "┐", "the list's frame ends at 40");
    assert_eq!(b[(40, 1)].symbol(), "┏", "the pane's frame starts at 40");
}

#[test]
fn below_96_the_capture_changes_nothing_on_screen() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%3".into()));
    let pvs = pvs_for(&ps, "%3", Some("a line only the pane would show"));
    for w in [60u16, 80, 95] {
        assert_eq!(
            buf(w, 30, &list(&ps, &pvs, &th, &n)),
            buf(w, 30, &list(&ps, &[], &th, &n)),
            "at {w}"
        );
    }
}

#[test]
fn the_pane_shows_the_live_screen_bottom_aligned() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%4".into()));
    let cap: Vec<String> = (1..=40).map(|k| format!("line {k}")).collect();
    let pvs = pvs_for(&ps, "%4", Some(&cap.join("\n")));
    let b = buf(100, 30, &list(&ps, &pvs, &th, &n));
    // the body's last row is 27: footer at 29, the frame's bottom at 28
    assert_eq!(text(&b, 27, 43, 50), "line 40");
    assert_eq!(text(&b, 8, 43, 50), "line 21", "20 rows, the capture's last 20");
    let pvs = pvs_for(&ps, "%4", Some("first\nsecond"));
    let b = buf(100, 30, &list(&ps, &pvs, &th, &n));
    assert_eq!(text(&b, 27, 43, 49), "second", "a short screen sits at the bottom");
    assert_eq!(text(&b, 26, 43, 48), "first");
    assert_eq!(text(&b, 8, 43, 98).trim(), "", "and the top is empty");
}

#[test]
fn the_pane_names_what_was_asked_said_and_is_being_done() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%4".into()));
    let b = buf(120, 30, &list(&ps, &[], &th, &n));
    let rows = pane_rows_text(&b, 48);
    // row 0 is the board's top line, 1 the pane's title
    assert!(rows[1].contains("api:1 · tests"), "{rows:#?}");
    assert!(rows[1].contains("running · 1m03s"), "{rows:#?}");
    assert!(rows[5].contains("doing  Bash: cargo test --workspace"), "{rows:#?}");
    assert!(rows[4].contains("said   —"), "nothing said reads as a dash: {rows:#?}");
}

#[test]
fn a_blocked_agent_shows_its_question_and_where_it_is() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%3".into()));
    let b = buf(180, 40, &list(&ps, &[], &th, &n));
    let rows = pane_rows_text(&b, 64);
    assert!(rows[1].contains("needs you · 4m12s"), "{rows:#?}");
    assert!(rows[2].contains("/src/api · ⑂feat/rate-limit · %3"), "{rows:#?}");
    assert!(rows[2].contains("◆ review pending"), "{rows:#?}");
    assert!(
        rows[3].contains("asked  add a token-bucket rate limiter to the router"),
        "{rows:#?}"
    );
    assert!(rows[4].contains("said   I added the token-bucket limiter"), "{rows:#?}");
    assert!(rows[5].contains("needs  Do you want to run npm test?"), "{rows:#?}");
    let y = rows.iter().position(|r| r.contains("needs  Do")).unwrap() as u16;
    let x = (64..180).find(|&x| b[(x, y)].symbol() == "D").unwrap();
    assert_eq!(b[(x, y)].fg, th.needs, "the question is in the needs role");
}

#[test]
fn an_ended_agent_says_so_and_its_screen_is_dim() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%5".into()));
    let pvs = pvs_for(&ps, "%5", Some("last words"));
    let b = buf(100, 30, &list(&ps, &pvs, &th, &n));
    let rows = pane_rows_text(&b, 41);
    assert!(rows[1].contains(" ended "), "{rows:#?}");
    assert!(rows[5].contains("doing  ended"), "{rows:#?}");
    assert_eq!(text(&b, 27, 43, 53), "last words");
    assert!(b[(43, 27)].modifier.contains(Modifier::DIM), "an ended agent's screen is dim");
}

#[test]
fn a_header_selected_reads_no_agent_selected() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Head("api".into()));
    let b = buf(100, 30, &list(&ps, &[], &th, &n));
    let rows = pane_rows_text(&b, 41);
    assert!(rows[1].starts_with("┌ no agent selected "), "{rows:#?}");
    assert!(rows[2].contains("the agent you pick shows here."), "{rows:#?}");
}

#[test]
fn a_failed_capture_is_not_an_empty_screen_nor_one_not_read_yet() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%4".into()));
    let shot = |pvs: &[Option<String>]| pane_rows_text(&buf(100, 30, &list(&ps, pvs, &th, &n)), 41).join("\n");
    let failed = shot(&pvs_for(&ps, "%4", None));
    let blank = shot(&pvs_for(&ps, "%4", Some("")));
    let unread = shot(&[]);
    assert!(failed.contains("capture failed"), "{failed}");
    assert!(blank.contains("live · blank") && !blank.contains("capture failed"), "{blank}");
    assert!(
        unread.contains("reading the screen") && !unread.contains("capture failed"),
        "{unread}"
    );
    let b = buf(100, 30, &list(&ps, &pvs_for(&ps, "%4", None), &th, &n));
    assert_eq!(b[(43, 8)].fg, th.error, "the failure is in the error role");
}

#[test]
fn a_line_wider_than_the_pane_ends_in_the_clip_mark() {
    // at 100 the pane's text runs x 43..=97: 55 cells, the mark on the last
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%4".into()));
    let long = "x".repeat(200);
    let fits = "y".repeat(55);
    let padded = format!("z{}", " ".repeat(200));
    let wide = "中".repeat(60);
    let cap = [long.as_str(), &fits, &padded, &wide].join("\n");
    let b = buf(100, 30, &list(&ps, &pvs_for(&ps, "%4", Some(&cap)), &th, &n));
    assert_eq!((b[(96, 24)].symbol(), b[(97, 24)].symbol()), ("x", "›"), "a cut line is marked");
    assert_eq!(b[(97, 25)].symbol(), "y", "a line that fits exactly is not");
    assert_eq!(b[(97, 26)].symbol(), " ", "nor one cut only in its trailing blanks");
    assert_eq!(
        b[(97, 27)].symbol(),
        "›",
        "a wide glyph that would straddle the edge gives way to the mark"
    );
    let th = ascii();
    let b = buf(100, 30, &list(&ps, &pvs_for(&ps, "%4", Some(&long)), &th, &n));
    assert_eq!(b[(97, 27)].symbol(), ">", "the ascii tier's mark");
}

#[test]
fn no_pane_row_exceeds_the_area() {
    use unicode_width::UnicodeWidthStr;
    let ps = trio();
    // a tab, a C0 control and an emoji's VS16 measure 0 by the char and 1+ by the str
    let cap = [
        "中".repeat(90),
        format!("\x1b[31mred\x1b[0m {}", "w".repeat(300)),
        "a\tb".repeat(40),
        "\u{26a0}\u{fe0f} ".repeat(30),
        "a\u{1}b".repeat(40),
    ]
    .join("\n");
    for th in [th(), ascii()] {
        for w in [96u16, 97, 100, 133, 164, 180, 240] {
            for sel in ["%3", "%4", "%5"] {
                let n = nav(&ps, Sel::Pane(sel.into()));
                let pvs = pvs_for(&ps, sel, Some(&cap));
                let mut t = Terminal::new(TestBackend::new(w, 24)).unwrap();
                t.draw(|f| board::render(f, f.area(), &list(&ps, &pvs, &th, &n))).unwrap();
                // the backend's Display skips a wide glyph's second cell, as a terminal does
                let out = t.backend().to_string();
                for (y, r) in out.lines().enumerate() {
                    // a row with a wide glyph is followed by the backend's "Hidden by" note
                    let r = r.split(" Hidden by multi-width symbols").next().unwrap_or(r);
                    let r = r.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(r);
                    assert_eq!(r.width(), w as usize, "row {y} at {w} on {sel}:\n{out}");
                }
            }
        }
    }
}

/// The list's rows, between the top line and the footer, `w` cells from the left.
fn list_part(b: &Buffer, w: u16) -> Vec<String> {
    (1..b.area.height - 1).map(|y| text(b, y, 0, w)).collect()
}

/// #313's tree, #301's ended row and #312's worktree cell, in one list.
fn busy() -> Vec<Pane> {
    let mut ps = trio();
    ps[1].subs = vec![Sub {
        id: "s1".into(),
        kind: SubKind::Subagent,
        ty: "Explore".into(),
        desc: "find auth callers in src/auth".into(),
        state: SubState::Running,
        started: Some(1_000),
        tokens: 12_400,
    }];
    (ps[1].loc, ps[1].pm) = ("api⑂feat · wt fix-312-row".into(), "clean".into());
    ps
}

#[test]
fn split_the_list_draws_what_a_board_of_its_width_draws() {
    let (ps, th) = (busy(), th());
    for (w, lw) in [(96u16, 40u16), (100, 41), (120, 48), (180, 64)] {
        for sel in [Sel::Pane("%4".into()), Sel::Pane("%5".into()), Sel::Head("web".into())] {
            let mut n = nav(&ps, sel.clone());
            n.seen.now = 1_100;
            n.trees.insert("%4".into());
            n.settle(&ps, 26);
            let wide = buf(w, 30, &list(&ps, &[], &th, &n));
            let alone = buf(lw, 30, &list(&ps, &[], &th, &n));
            assert_eq!(list_part(&wide, lw), list_part(&alone, lw), "at {w} on {sel:?}");
        }
    }
    // and the rows hold what the three issues put there
    let mut n = nav(&ps, Sel::Pane("%4".into()));
    n.seen.now = 1_100;
    n.trees.insert("%4".into());
    n.settle(&ps, 26);
    let rows = list_part(&buf(180, 30, &list(&ps, &[], &th, &n)), 64).join("\n");
    assert!(rows.contains("├ ● Explore") || rows.contains("└ ● Explore"), "the tree: {rows}");
    // 64 columns: #312's narrowing has dropped the repo and the worktree name
    assert!(
        rows.contains("clean ⑂feat ") && !rows.contains("wt fix"),
        "the worktree cell: {rows}"
    );
    assert!(rows.contains("◌  0 ui") && rows.contains("ended"), "the ended row: {rows}");
}

#[test]
fn a_tab_moves_to_the_next_stop_of_eight_and_a_control_goes() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%4".into()));
    // the pane's text starts at x 43 at 100 columns
    let b = buf(100, 30, &list(&ps, &pvs_for(&ps, "%4", Some("ab\tc\n\tz\nx\u{1}y")), &th, &n));
    assert_eq!(text(&b, 25, 43, 52), "ab      c", "a tab after two cells pads to 8");
    assert_eq!(text(&b, 26, 43, 52), "        z", "a leading tab is 8");
    assert_eq!(text(&b, 27, 43, 46), "xy ", "a C0 control takes no cell");
}

#[test]
fn a_long_tabbed_line_is_clipped_not_overflowed() {
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%4".into()));
    let b = buf(100, 30, &list(&ps, &pvs_for(&ps, "%4", Some(&"a\tb".repeat(40))), &th, &n));
    assert_eq!(b[(97, 27)].symbol(), "›", "the tabbed line is cut and marked");
}

#[test]
fn an_emoji_widened_by_vs16_is_measured_as_drawn() {
    // 30 of them are 30 cells by the char but 60 as drawn: wider than the 55
    let (ps, th) = (trio(), th());
    let n = nav(&ps, Sel::Pane("%4".into()));
    let b = buf(
        100,
        30,
        &list(&ps, &pvs_for(&ps, "%4", Some(&"\u{26a0}\u{fe0f}".repeat(30))), &th, &n),
    );
    assert_eq!(b[(97, 27)].symbol(), "›", "a cut line of wide emoji is marked");
}

// ------------------------------------------------ the key loop's picks

#[test]
fn the_pick_is_the_lists_selection_only_when_split() {
    let ps = trio();
    let a = Rect::new(0, 0, 100, 30);
    let on = nav(&ps, Sel::Pane("%4".into()));
    assert_eq!(board::pick_of(true, a, &on, &ps).as_deref(), Some("%4"));
    assert_eq!(board::pick_of(true, Rect::new(0, 0, 95, 30), &on, &ps), None, "no pane at 95");
    assert_eq!(board::pick_of(false, a, &on, &ps), None, "the tiles pick nothing");
    assert_eq!(board::pick_of(true, a, &nav(&ps, Sel::Head("api".into())), &ps), None, "a header");
}

#[test]
fn a_selection_asks_the_worker_once_however_often_it_redraws() {
    let mut pick = None;
    let asks: Vec<bool> = [Some("%4"), Some("%4"), Some("%4"), Some("%5"), Some("%5"), None, None]
        .into_iter()
        .map(|cur| board::repick(&mut pick, cur.map(String::from)))
        .collect();
    assert_eq!(asks, [true, false, false, true, false, true, false]);
    assert_eq!(pick, None);
}

fn seen(entries: &[(&str, Option<&str>)]) -> HashMap<String, Option<String>> {
    entries.iter().map(|(k, v)| (k.to_string(), v.map(String::from))).collect()
}

#[test]
fn a_pick_no_capture_has_read_yet_is_unread_never_blank() {
    let ps = trio();
    let pvs = pvs_for(&ps, "%4", Some(""));
    let (th, n) = (th(), nav(&ps, Sel::Pane("%4".into())));
    let shot = |pvs: &[Option<String>]| pane_rows_text(&buf(100, 30, &list(&ps, pvs, &th, &n)), 41).join("\n");
    let unread = board::shown(Some("%4"), &seen(&[("%3", Some("x"))]), &pvs);
    assert!(unread.is_empty(), "no entry for the pick: not read yet");
    assert!(shot(unread).contains("reading the screen"));
    let read = board::shown(Some("%4"), &seen(&[("%4", Some(""))]), &pvs);
    assert_eq!(read.len(), pvs.len());
    assert!(shot(read).contains("live · blank"));
    assert_eq!(board::shown(None, &seen(&[]), &pvs).len(), pvs.len(), "the tiles keep theirs");
}

#[test]
fn a_new_pick_forgets_every_other_screen() {
    let mut s = seen(&[("%3", Some("old")), ("%4", Some("mine")), ("%5", None)]);
    board::forget(&mut s, Some("%4"));
    assert_eq!(s, seen(&[("%4", Some("mine"))]), "an old screen is never shown as live");
    let mut s = seen(&[("%3", Some("old"))]);
    board::forget(&mut s, None);
    assert_eq!(s.len(), 1, "no pick (the tiles): every screen stays");
}

// ------------------------------------------------ the goldens (the 100/180 mockups)

/// The mockups' fleet, the selection's screen as Claude Code draws a permission prompt.
fn mock() -> (Vec<Pane>, Vec<Option<String>>) {
    let mut ps = trio();
    let more = [
        ("%6", "api:3", "migrate", "running", "Edit src/db/migrate.rs", Some(22)),
        ("%7", "web:2", "docs", "needs-input", "plan ready: approve?", Some(38)),
        ("%8", "infra:0", "terraform", "error", "terraform plan: state lock held", Some(362)),
    ];
    for (id, wher, title, st, line, since) in more {
        let mut p = pane(id, wher, st);
        p.title = title.into();
        if st == "running" {
            p.doing = line.into();
        } else {
            p.msg = line.into();
        }
        p.since = since;
        ps.push(p);
    }
    let screen = [
        "⏺ I added the token-bucket limiter and wired it into the router. Running the suite before I commit, as the issue asks.",
        "",
        "⏺ Bash(npm test -- --runInBand)",
        "",
        " Bash command",
        "",
        "   npm test -- --runInBand",
        "   Run the API test suite serially",
        "",
        " Do you want to proceed?",
        " ❯ 1. Yes",
        "   2. Yes, and don't ask again for npm test commands in /home/user/src/api",
        "   3. No, and tell Claude what to do differently (esc)",
    ]
    .join("\n");
    let pvs = pvs_for(&ps, "%3", Some(&screen));
    (ps, pvs)
}

#[test]
fn preview_goldens() {
    let th = th();
    let (ps, pvs) = mock();
    let n = nav(&ps, Sel::Pane("%3".into()));
    for (w, h) in [(100u16, 30u16), (180, 40)] {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| board::render(f, f.area(), &list(&ps, &pvs, &th, &n))).unwrap();
        golden(&format!("board-preview-{w}"), &t.backend().to_string());
    }
}
