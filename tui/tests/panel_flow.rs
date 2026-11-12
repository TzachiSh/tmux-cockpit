//! The flow renderer (#935): a Mermaid `sequenceDiagram` subset drawn as a
//! ladder or a numbered list that fits a width. Pure functions, literal
//! samples in tests/fixtures/flow, goldens in tests/golden.
//!
//! COCKPIT_BLESS=1 cargo test --test panel_flow  rewrites the goldens.
use cockpit_tui::panel::flow::{self, Shape, Status};
use unicode_width::UnicodeWidthStr;

const BASE: &str = include_str!("fixtures/flow/base.mmd");
const STATUS: &str = include_str!("fixtures/flow/status.mmd");
const SIX: &str = include_str!("fixtures/flow/six.mmd");
const BAD: &str = include_str!("fixtures/flow/bad.mmd");
const LOOP: &str = include_str!("fixtures/flow/loop.mmd");
const NOTE: &str = include_str!("fixtures/flow/note.mmd");
const WIDE: &str = include_str!("fixtures/flow/wide.mmd");

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

fn drawn(src: &str, w: usize) -> String {
    flow::render(src, w).iter().map(|l| format!("{l}\n")).collect()
}

fn parsed(src: &str) -> flow::Flow {
    flow::parse(src).unwrap_or_else(|e| panic!("sample should parse: {e}"))
}

// ------------------------------------------------------------ the goldens

#[test]
fn goldens_four_actors_at_40_60_100_without_and_with_status() {
    for (name, src) in [("base", BASE), ("status", STATUS)] {
        for w in [40, 60, 100] {
            golden(&format!("flow-{name}-{w}"), &drawn(src, w));
        }
    }
}

// ------------------------------------------------- decision 1: ladder or list

#[test]
fn four_actors_draw_a_ladder_from_width_60_and_a_list_below() {
    let f = parsed(BASE);
    assert_eq!(f.actors.len(), 4);
    assert_eq!(flow::shape(&f, 60), Shape::Ladder);
    assert_eq!(flow::shape(&f, 59), Shape::List);
}

#[test]
fn six_actors_fall_back_to_the_list_at_any_width() {
    let f = parsed(SIX);
    assert_eq!(f.actors.len(), 6);
    assert_eq!(flow::shape(&f, 200), Shape::List);
    assert!(flow::render(SIX, 200)[0].contains("owner→lead"), "{:?}", flow::render(SIX, 200));
}

// ---------------------------------- decision 2: no status, no column, no current

#[test]
fn no_status_on_any_step_means_no_current_step_and_no_status_column() {
    let f = parsed(BASE);
    assert!(f.steps.iter().all(|s| s.status.is_none()));
    assert_eq!(f.current(), None);
    assert!(!flow::render(BASE, 100).iter().any(|l| l.starts_with('▶')));
    let list = flow::render(BASE, 50);
    assert!(list[0].starts_with(" 1 brief"), "{list:?}");
}

#[test]
fn a_status_only_on_its_own_line_after_a_step_sets_glyph_and_time() {
    let f = parsed(STATUS);
    let got: Vec<_> = f.steps.iter().map(|s| (s.status, s.time.as_deref())).collect();
    use Status::*;
    let want = [
        (Some(Done), Some("09:02")),
        (Some(Done), Some("09:20")),
        (Some(Done), Some("09:41")),
        (Some(Running), Some("09:44")),
        (Some(Needs), None),
    ];
    assert_eq!(got, want);
    let trailing = parsed("sequenceDiagram\nA->>B: x %% status: ✓ 09:02\n");
    assert_eq!(trailing.steps[0].status, None);
    assert_eq!(trailing.steps[0].label, "x %% status: ✓ 09:02");
}

#[test]
fn a_status_word_outside_the_set_is_not_drawn() {
    let err = flow::parse("sequenceDiagram\nA->>B: x\n%% status: maybe\n").unwrap_err();
    assert!(err.contains("maybe"), "{err}");
}

// ------------------------------------- decision 3: the current step and its mark

#[test]
fn the_current_step_is_the_first_not_done_marked_in_ladder_and_list() {
    assert_eq!(parsed(STATUS).current(), Some(3));
    let ladder = flow::render(STATUS, 60);
    let marked: Vec<usize> = ladder
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with('▶'))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(marked, vec![1 + 2 * 3], "{ladder:?}");
    let list = flow::render(STATUS, 59);
    let here: Vec<usize> = list
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("◀ here"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(here, vec![3], "{list:?}");
}

#[test]
fn every_step_done_leaves_no_current_step() {
    let f = parsed("sequenceDiagram\nA->>B: x\n%% status: ✓\nB->>A: y\n%% status: done\n");
    assert_eq!(f.current(), None);
}

// ------------------------------------ decision 4: two rows a step, label above

#[test]
fn the_ladder_draws_two_rows_a_step_with_the_label_above_the_arrow() {
    let out = flow::render(BASE, 100);
    let labels = ["1 brief", "2 PR open", "3 NIT x2", "4 suite", "5 merge?"];
    for (i, label) in labels.iter().enumerate() {
        assert!(out[1 + 2 * i].contains(label), "row {} not {label}: {out:?}", 1 + 2 * i);
        assert!(out[2 + 2 * i].contains(['▶', '◀']), "row {} has no arrow: {out:?}", 2 + 2 * i);
    }
}

// --------------------------------------------- decision 5: notes and the NEED box

#[test]
fn a_note_not_starting_need_from_you_shows_under_its_step_not_in_the_box() {
    let f = parsed(NOTE);
    assert_eq!(f.steps[0].notes, vec!["reads the issue first".to_string()]);
    assert_eq!(f.need, vec!["pick a name".to_string()]);
    let out = flow::render(NOTE, 60);
    assert!(out[3].contains("reads the issue first"), "{out:?}");
    let list = flow::render(NOTE, 40);
    assert!(list[1].contains("reads the issue first"), "{list:?}");
}

#[test]
fn only_a_need_from_you_note_goes_in_the_closing_box() {
    let out = flow::render(BASE, 60);
    let top = out.iter().position(|l| l.contains("NEED FROM YOU")).expect("a NEED box");
    assert!(out[top].starts_with('┌'), "{out:?}");
    assert!(out[top + 1].contains("approve merge after CI green"), "{out:?}");
    assert!(out.last().unwrap().starts_with('└'), "{out:?}");
}

// --------------------------------------- decision 6: flatten blocks, Err the rest

#[test]
fn loop_alt_opt_else_and_end_are_flattened_to_their_steps() {
    let f = parsed(LOOP);
    let labels: Vec<&str> = f.steps.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["push", "failure log", "pass", "rerun"]);
}

#[test]
fn other_syntax_and_unbalanced_blocks_are_not_drawn() {
    for (row, src) in [
        ("par block", "sequenceDiagram\npar a\nA->>B: x\nend\n"),
        ("activate", "sequenceDiagram\nA->>B: x\nactivate B\n"),
        ("end without a block", "sequenceDiagram\nA->>B: x\nend\n"),
        ("unclosed loop", "sequenceDiagram\nloop x\nA->>B: x\n"),
        ("cross arrow", "sequenceDiagram\nA-xB: x\n"),
    ] {
        assert!(flow::parse(src).is_err(), "{row} parsed");
    }
}

#[test]
fn a_malformed_sample_shows_why_then_the_raw_lines_cut_to_the_width() {
    let out = flow::render(BAD, 30);
    assert!(out[0].starts_with("(not drawn: "), "{out:?}");
    assert_eq!(out[1], "sequenceDiagram");
    assert_eq!(out[2], "  participant O as owner");
    assert_eq!(out.len(), 1 + BAD.lines().count());
}

// --------------------------------------------- decision 7: never wider, no panic

#[test]
fn every_prefix_of_every_sample_fits_every_width_from_0_to_130() {
    for (name, src) in [
        ("base", BASE),
        ("status", STATUS),
        ("six", SIX),
        ("bad", BAD),
        ("loop", LOOP),
        ("note", NOTE),
        ("wide", WIDE),
    ] {
        for cut in (0..=src.len()).filter(|&i| src.is_char_boundary(i)) {
            for w in 0..=130 {
                for l in flow::render(&src[..cut], w) {
                    assert!(l.width() <= w, "{name} cut={cut} w={w}: {l:?}");
                }
            }
        }
    }
}

#[test]
fn control_characters_never_reach_a_line() {
    for w in [0, 20, 60, 100] {
        let out = flow::render(WIDE, w);
        assert!(out.iter().all(|l| !l.chars().any(char::is_control)), "w={w}: {out:?}");
    }
}

#[test]
fn empty_input_is_not_drawn_and_width_0_draws_nothing_visible() {
    assert!(flow::parse("").is_err());
    assert!(flow::render("", 40)[0].starts_with("(not drawn: "));
    assert!(flow::render(STATUS, 0).iter().all(String::is_empty));
}

#[test]
fn bidi_controls_and_line_separators_never_reach_a_line() {
    for (row, label) in [
        ("U+202E", "abc\u{202E}fed"),
        ("U+2028", "one\u{2028}two"),
        ("U+2029", "one\u{2029}two"),
    ] {
        let src = format!("sequenceDiagram\nA->>B: {label}\n");
        for w in [40, 60] {
            let out = flow::render(&src, w);
            let bad = |c: char| matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}');
            assert!(out.iter().all(|l| !l.chars().any(bad)), "{row} w={w}: {out:?}");
        }
    }
}
