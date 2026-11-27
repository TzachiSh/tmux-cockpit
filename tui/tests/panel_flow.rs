//! The flow renderer (#935): a Mermaid `sequenceDiagram` subset drawn as a
//! ladder or a numbered list that fits a width. Pure functions, literal
//! samples in tests/fixtures/flow, goldens in tests/golden.
//!
//! COCKPIT_BLESS=1 cargo test --test panel_flow  rewrites the goldens.
use cockpit_tui::panel::flow::{self, Shape, Status};
use cockpit_tui::theme::Glyphs;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const UNI: Glyphs = Glyphs::unicode();
const ASC: Glyphs = Glyphs::ascii();

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
    flow::render(src, w, &UNI).iter().map(|l| format!("{l}\n")).collect()
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
    assert!(
        flow::render(SIX, 200, &UNI)[0].contains("owner─▶lead"),
        "{:?}",
        flow::render(SIX, 200, &UNI)
    );
}

// ---------------------------------- decision 2: no status, no column, no current

#[test]
fn no_status_on_any_step_means_no_current_step_and_no_status_column() {
    let f = parsed(BASE);
    assert!(f.steps.iter().all(|s| s.status.is_none()));
    assert_eq!(f.current(), None);
    assert!(!flow::render(BASE, 100, &UNI).iter().any(|l| l.starts_with('▶')));
    let list = flow::render(BASE, 50, &UNI);
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
    let ladder = flow::render(STATUS, 60, &UNI);
    let marked: Vec<usize> = ladder
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with('▶'))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(marked, vec![1 + 2 * 3], "{ladder:?}");
    let list = flow::render(STATUS, 59, &UNI);
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
    let out = flow::render(BASE, 100, &UNI);
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
    let note = flow::Note {
        text: "reads the issue first".into(),
        place: flow::Place::Right(1),
    };
    assert_eq!(f.steps[0].notes, [note]);
    assert_eq!(f.need, vec!["pick a name".to_string()]);
    let out = flow::render(NOTE, 60, &UNI);
    // Right of the last lifeline it has 12 columns, so it grows to 16 over that lifeline and wraps (#1533).
    assert!(out[3].ends_with("↳ reads the") && out[4].ends_with("issue first"), "{out:?}");
    let list = flow::render(NOTE, 40, &UNI);
    assert!(list[1].contains("reads the issue first"), "{list:?}");
}

#[test]
fn only_a_need_from_you_note_goes_in_the_closing_box() {
    let out = flow::render(BASE, 60, &UNI);
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
    ] {
        assert!(flow::parse(src).is_err(), "{row} parsed");
    }
}

#[test]
fn a_malformed_sample_shows_why_then_the_raw_lines_cut_to_the_width() {
    let out = flow::render(BAD, 30, &UNI);
    assert!(out[0].starts_with("(not drawn: "), "{out:?}");
    assert_eq!(out[1], "sequenceDiagram");
    assert_eq!(out[2], "  participant O as owner");
    assert_eq!(out.len(), 1 + BAD.lines().count());
}

// ------------------------------------------------- Mermaid's arrow kinds (#1532)

/// Each of Mermaid's ten arrows between three actors, both ways, and three self steps.
const ARROW_KINDS: &str = "sequenceDiagram\nparticipant A\nparticipant B\nparticipant C\nA->>B: one\nB-->>A: two\n\
A->C: three\nC-->A: four\nB-xC: five\nC--xA: six\nA-)C: seven\nC--)B: eight\nB<<->>A: nine\nC<<-->>A: ten\n\
A-)A: open self\nB-xB: cross self\nC->C: plain self\n";

#[test]
fn each_arrow_kind_draws_its_own_line_and_head() {
    let (s, d) = (|n: usize| "─".repeat(n), |n: usize| "┄".repeat(n));
    let rows: [(&str, String, &str, &str); 10] = [
        ("->>", s(27) + "▶", "─▶", "->"),
        ("-->>", d(27) + "▶", "┄▶", ".>"),
        ("->", s(28), "──", "--"),
        ("-->", d(28), "┄┄", ".."),
        ("-x", s(27) + "✕", "─✕", "-x"),
        ("--x", d(27) + "✕", "┄✕", ".x"),
        ("-)", s(27) + "▷", "─▷", "-)"),
        ("--)", d(27) + "▷", "┄▷", ".)"),
        ("<<->>", format!("◀{}▶", s(26)), "◀─▶", "<->"),
        ("<<-->>", format!("◀{}▶", d(26)), "◀┄▶", "<.>"),
    ];
    for (spell, cells, uni, asc) in rows {
        let src = format!("sequenceDiagram\nA{spell}B: m\n");
        assert!(flow::parse(&src).is_ok(), "{spell}");
        assert_eq!(flow::render(&src, 60, &UNI)[2], format!("{}│{cells}│", " ".repeat(15)), "{spell}");
        assert_eq!(flow::render(&src, 40, &UNI)[0], format!("m  A{uni}B"), "{spell}");
        assert_eq!(flow::render(&src, 40, &ASC)[0], format!("m  A{asc}B"), "{spell}");
    }
}

#[test]
fn a_dashed_self_arrow_draws_dashed() {
    assert_eq!(
        flow::render("sequenceDiagram\nA-->>A: x\n", 60, &UNI)[2],
        format!("{}│┄╮↩", " ".repeat(30))
    );
}

#[test]
fn a_right_to_left_or_self_arrow_ends_in_its_own_head() {
    // The ladder's arrow row with A declared first, so B to A is drawn right to left.
    let rows: [(&str, String, String); 5] = [
        ("B-)A", format!("│◁{}│", "─".repeat(27)), format!("|({}|", "-".repeat(27))),
        ("B-xA", format!("│✕{}│", "─".repeat(27)), format!("|x{}|", "-".repeat(27))),
        ("A-)A", format!("│─╮◁{}│", " ".repeat(25)), format!("|-+({}|", " ".repeat(25))),
        ("A-xA", format!("│─╮✕{}│", " ".repeat(25)), format!("|-+x{}|", " ".repeat(25))),
        ("A->A", format!("│─╮{}│", " ".repeat(26)), format!("|-+{}|", " ".repeat(26))),
    ];
    let pad = " ".repeat(15);
    for (step, uni, asc) in rows {
        let src = format!("sequenceDiagram\nparticipant A\nparticipant B\n{step}: m\n");
        assert_eq!(flow::render(&src, 60, &UNI)[2], format!("{pad}{uni}"), "{step}");
        assert_eq!(flow::render(&src, 60, &ASC)[2], format!("{pad}{asc}"), "{step}");
    }
}

// ------------------------------------------------- where a ladder note sits (#1533)

/// A note right of, left of and over lifelines: three actors, at 60 their lifelines at 10, 29 and 48.
const PLACED: &str = "sequenceDiagram\nparticipant A\nparticipant B\nparticipant C\nA->>B: x\n\
Note right of B: hi\nNote left of B: lo\nNote over B,C: up\n";

/// Notes with too few columns where they are placed: four actors and a status with a time, so at 61
/// the lifelines are at 9, 22, 35 and 48 and a note may use columns 3 to 54.
const SLID: &str = "sequenceDiagram\nparticipant A\nparticipant B\nparticipant C\nparticipant D\nA->>B: x\n\
%% status: done 09:00\nNote right of D: rd\nNote left of A: la\nNote over A: oa\nNote over D: od\nNote over B: ob\n";

/// A note over each of three lifelines: at 100 they are at 17, 50 and 83.
const OVER_ONE: &str = "sequenceDiagram\nparticipant A\nparticipant B\nparticipant C\nA->>B: x\n\
Note over A: owner reads the brief\nNote over B: agent runs the suite\nNote over C: CI reports back green\n";

#[test]
fn a_ladder_note_starts_where_its_placement_says() {
    // Right of B two columns after its lifeline, left of B ending two before it, over B and C centred on both.
    let out = flow::render(PLACED, 60, &UNI);
    let col = |l: &String| l.chars().position(|c| c == '↳');
    assert_eq!(
        (col(&out[3]), col(&out[4]), col(&out[5])),
        (Some(31), Some(24), Some(37)),
        "{out:?}"
    );
}

#[test]
fn a_ladder_note_too_narrow_for_its_place_slides_inward() {
    // Each region grows to 16 columns about its centre and slides back inside 3..55: right of D
    // from 50..55 to 39..55, left of A from 3..8 to 3..19 (its row ends at 18), over A from 3..15
    // (halfway to B) to 3..19, over D from 41..55 to 39..55, and over B from 15..28 (halfway to A
    // and to C) to 13..29, each over row centred in its region.
    let out = flow::render(SLID, 61, &UNI);
    let col = |l: &String| l.chars().position(|c| c == '↳');
    assert_eq!(
        (col(&out[3]), col(&out[4]), col(&out[5]), col(&out[6]), col(&out[7])),
        (Some(39), Some(15), Some(9), Some(45), Some(19)),
        "{out:?}"
    );
}

#[test]
fn a_ladder_note_over_one_lifeline_spans_halfway_to_its_neighbours() {
    // Over A from the gutter to halfway to B (1..33), over B halfway to A and to C (33..66), over C
    // from halfway to B to the edge (66..100): each note fits on one row, centred, where 16 columns
    // would wrap it.
    let out = flow::render(OVER_ONE, 100, &UNI);
    let col = |l: &String| l.chars().position(|c| c == '↳');
    assert_eq!((col(&out[3]), col(&out[4]), col(&out[5])), (Some(5), Some(38), Some(71)), "{out:?}");
    for (row, text) in [
        (3, "owner reads the brief"),
        (4, "agent runs the suite"),
        (5, "CI reports back green"),
    ] {
        assert!(out[row].contains(&format!("↳ {text}")), "{out:?}");
    }
}

/// The display columns of `c` in `l`, counted by grapheme cluster width.
fn cols_of(l: &str, c: char) -> Vec<usize> {
    let mut at = 0;
    let mut out = vec![];
    for g in l.graphemes(true) {
        if g.starts_with(c) {
            out.push(at);
        }
        at += g.width();
    }
    out
}

#[test]
fn a_placed_note_keeps_the_lifelines_it_does_not_reach() {
    let out = flow::render(
        "sequenceDiagram\nparticipant 名 as 名前\nparticipant B as b\n名->>B: x\nNote right of 名: 漢字のノート\n",
        60,
        &UNI,
    );
    assert_eq!(cols_of(&out[3], '│'), cols_of(&out[2], '│'), "{out:?}");
}

// --------------------------------------------- decision 7: never wider, no panic

#[test]
fn every_prefix_of_every_sample_fits_every_width_from_0_to_130() {
    for g in [UNI, ASC] {
        for (name, src) in [
            ("base", BASE),
            ("status", STATUS),
            ("six", SIX),
            ("bad", BAD),
            ("loop", LOOP),
            ("note", NOTE),
            ("wide", WIDE),
            ("arrows", ARROW_KINDS),
            ("placed", PLACED),
            ("slid", SLID),
            ("over one", OVER_ONE),
        ] {
            for cut in (0..=src.len()).filter(|&i| src.is_char_boundary(i)) {
                for w in 0..=130 {
                    for l in flow::render(&src[..cut], w, &g) {
                        assert!(l.width() <= w, "ascii={} {name} cut={cut} w={w}: {l:?}", g.ascii);
                    }
                }
            }
        }
    }
}

/// Every drawn shape from an ASCII source (flow-r2's GLY).
const GLY: &str = "sequenceDiagram\nparticipant A as alpha\nparticipant B as beta\nNote over A: lead note\n\
A->>B: one\n%% status: done 09:00\nB-->>A: two is a label long enough to be cut in the list\n%% status: failed\n\
A->>A: self\n%% status: running\nNote over A: a note\nB->>A: four\n%% status: pending\nNote over A,B: NEED FROM YOU: pick\n";

#[test]
fn the_ascii_tier_draws_nothing_but_ascii() {
    // WIDE stays out: its own words are CJK, and the tier covers glyphs, not text.
    for (name, src) in [
        ("base", BASE),
        ("status", STATUS),
        ("six", SIX),
        ("bad", BAD),
        ("loop", LOOP),
        ("note", NOTE),
        ("gly", GLY),
        ("arrows", ARROW_KINDS),
        ("placed", PLACED),
        ("slid", SLID),
        ("over one", OVER_ONE),
    ] {
        for w in 0..=130 {
            let out = flow::render(src, w, &ASC);
            assert!(out.iter().all(|l| l.is_ascii()), "{name} w={w}: {out:?}");
        }
    }
    assert_eq!(flow::not_drawn("x", "a reason long enough to be cut", 12, &ASC)[0], "(not draw...");
    assert_eq!(
        flow::render(BASE, 60, &ASC)[2],
        "        |------------>|             |             |"
    );
}

#[test]
fn the_ascii_tier_draws_a_control_left_in_a_hand_built_field_as_a_question_mark() {
    // A parsed line holds no control after tidy, so only a field set by hand reaches the replacement mark.
    let mut f = parsed("sequenceDiagram\nA->>B: x\n");
    f.steps[0].label = "one\ntwo".into();
    for (w, want) in [(60, Shape::Ladder), (40, Shape::List)] {
        assert_eq!(flow::shape(&f, w), want, "w={w}");
        let out = flow::draw(&f, w, &ASC);
        assert!(out.iter().all(|l| l.is_ascii()), "w={w}: {out:?}");
        assert!(out.iter().any(|l| l.contains("one?two")), "w={w}: {out:?}");
    }
}

#[test]
fn control_characters_never_reach_a_line() {
    for w in [0, 20, 60, 100] {
        let out = flow::render(WIDE, w, &UNI);
        assert!(out.iter().all(|l| !l.chars().any(char::is_control)), "w={w}: {out:?}");
    }
}

#[test]
fn empty_input_is_not_drawn_and_width_0_draws_nothing_visible() {
    assert!(flow::parse("").is_err());
    assert!(flow::render("", 40, &UNI)[0].starts_with("(not drawn: "));
    assert!(flow::render(STATUS, 0, &UNI).iter().all(String::is_empty));
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
            let out = flow::render(&src, w, &UNI);
            let bad = |c: char| matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}');
            assert!(out.iter().all(|l| !l.chars().any(bad)), "{row} w={w}: {out:?}");
        }
    }
}

#[test]
fn an_escape_sequence_in_a_label_leaves_no_trace() {
    // A CSI, an OSC 8 link ended by ST, an OSC 0 ended by BEL, a DCS: each goes whole, payload and all (#1530).
    for label in ["x\x1b[31my", "x\x1b]8;;https://e.com\x1b\\y", "x\x1b]0;t\x07y", "x\x1bPq;p\x1b\\y"] {
        let src = format!("sequenceDiagram\nA->>B: {label}\n");
        assert_eq!(parsed(&src).steps[0].label, "xy", "{label:?}");
        for w in [40, 60] {
            assert!(flow::render(&src, w, &UNI).iter().any(|l| l.contains("xy")), "{label:?} w={w}");
        }
    }
}

#[test]
fn need_from_you_is_a_whole_prefix_not_the_start_of_a_longer_word() {
    let f = parsed("sequenceDiagram\nA->>B: x\nNote over A: NEED FROM YOUR call is wanted\nNote over A: NEED FROM YOU pick one\n");
    let note = flow::Note {
        text: "NEED FROM YOUR call is wanted".into(),
        place: flow::Place::Over(0, 0),
    };
    assert_eq!(f.steps[0].notes, [note]);
    assert_eq!(f.need, vec!["pick one".to_string()]);
}

#[test]
fn goldens_wide_graphemes_at_60_and_100() {
    // CJK names and labels, an emoji ZWJ sequence, a flag: each lifeline must
    // keep its column on every row, which the final cut to the width can hide.
    for w in [60, 100] {
        golden(&format!("flow-wide-{w}"), &drawn(WIDE, w));
    }
}

#[test]
fn the_list_aligns_who_to_whom_whatever_the_label_widths() {
    // Where the head "▶" sits on each line: one column for every step, short or long labels.
    for (row, labels) in [
        ("all short", ["go", "run", "ok"]),
        ("short and long", ["go", "run the suite", "ok"]),
    ] {
        let src = format!(
            "sequenceDiagram\nO->>A: {}\nA->>O: {}\nO->>A: {}\n",
            labels[0], labels[1], labels[2]
        );
        let out = flow::render(&src, 40, &UNI);
        let cols: Vec<usize> = out.iter().map(|l| l.split('▶').next().unwrap().width()).collect();
        assert!(cols.windows(2).all(|p| p[0] == p[1]), "{row}: {out:?}");
    }
}

#[test]
fn a_list_line_that_fits_exactly_is_not_cut() {
    // A step with no time: the compact line's space before the empty time is no column (#1527).
    assert_eq!(flow::render("sequenceDiagram\nA->>B: x\n", 6, &UNI), ["x A─▶B"]);
    assert_eq!(flow::render(BASE, 21, &UNI)[0], " 1 brief owner─▶agent");
}

#[test]
fn an_aligned_list_with_no_time_keeps_no_time_column() {
    // No step has a time: the aligned label column keeps no gap for a time column (#1528).
    assert_eq!(flow::render("sequenceDiagram\nA->>B: abcdefgh\n", 14, &UNI), ["abcdefgh  A─▶B"]);
    assert_eq!(flow::render(BASE, 26, &UNI)[1], " 2 PR op…  agent─▶reviewer");
}

#[test]
fn a_list_with_empty_labels_is_not_cut_where_compact_fits() {
    // Every label empty, no time: an aligned line needs 2 + 4 columns here, so at 5 the compact one shows (#1541).
    let src = "sequenceDiagram\nA->>B:\nB->>A:\n";
    assert_eq!(flow::render(src, 5, &UNI), [" A─▶B", " B─▶A"]);
    assert_eq!(flow::render(src, 6, &UNI), ["  A─▶B", "  B─▶A"]);
}

#[test]
fn a_hand_built_step_naming_a_missing_actor_draws_without_a_panic() {
    let mut f = parsed("sequenceDiagram\nA->>B: x\n");
    f.steps[0].to = 7;
    f.steps[0].from = usize::MAX;
    f.steps[0].notes.push(flow::Note {
        text: "over actors past the end".into(),
        place: flow::Place::Over(7, usize::MAX),
    });
    let no_actors = flow::Flow {
        steps: f.steps.clone(),
        ..Default::default()
    };
    for (row, f) in [("past the end", &f), ("no actors at all", &no_actors)] {
        for w in [0, 40, 60, 100] {
            assert!(flow::draw(f, w, &UNI).iter().all(|l| l.width() <= w), "{row} w={w}");
        }
    }
}

const LONG_NOTE: &str = "the suite runs twice because the first run on this box was flaky and the second decides";

fn lead_and_long_note() -> String {
    format!("sequenceDiagram\nNote over A: starts after lunch\nA->>B: x\nNote over A: {LONG_NOTE}\n")
}

#[test]
fn a_note_before_the_first_step_shows_under_the_header() {
    let f = parsed(&lead_and_long_note());
    let note = flow::Note {
        text: "starts after lunch".into(),
        place: flow::Place::Over(0, 0),
    };
    assert_eq!(f.lead, [note]);
    assert!(flow::render(&lead_and_long_note(), 60, &UNI)[1].contains("↳ starts after lunch"));
    assert!(flow::render(&lead_and_long_note(), 40, &UNI)[0].contains("↳ starts after lunch"));
}

#[test]
fn a_long_note_wraps_onto_more_rows_and_keeps_every_word() {
    // The note's rows come after the step: in the list after the lead note and the step's line,
    // in the ladder after the header, the lead note and the step's two rows.
    for (w, first) in [(40, 2), (60, 4)] {
        let out = flow::render(&lead_and_long_note(), w, &UNI);
        let rows = &out[first..];
        assert!(rows.len() >= 2, "w={w}: {out:?}");
        let words: Vec<&str> = rows
            .iter()
            .flat_map(|l| l.split_whitespace())
            .filter(|t| LONG_NOTE.contains(*t))
            .collect();
        assert_eq!(words.join(" "), LONG_NOTE, "w={w}: {out:?}");
    }
}

#[test]
fn a_wrapped_note_marks_only_its_first_row() {
    // One note reads as one: its later rows line up under its text with no mark of their own (#1531).
    let src = lead_and_long_note();
    assert_eq!(
        flow::render(&src, 40, &UNI)[2..],
        [
            "↳ the suite runs twice because the first",
            "  run on this box was flaky and the",
            "  second decides"
        ]
    );
    assert_eq!(flow::render(&src, 60, &UNI).iter().filter(|l| l.contains('↳')).count(), 2);
}

#[test]
fn a_wrapped_lead_note_in_the_list_marks_only_its_first_row() {
    // The list draws a note before the first step from its own call, so it gets its own row (#1531).
    let src = format!("sequenceDiagram\nNote over A: {LONG_NOTE}\nA->>B: x\n");
    assert_eq!(
        flow::render(&src, 40, &UNI),
        [
            "↳ the suite runs twice because the first",
            "  run on this box was flaky and the",
            "  second decides",
            "x  A─▶B"
        ]
    );
}

#[test]
fn the_status_key_is_read_in_any_case() {
    for (row, line) in [("Status", "%% Status: done"), ("STATUS", "%% STATUS: ✓ 09:02")] {
        let f = parsed(&format!("sequenceDiagram\nA->>B: x\n{line}\n"));
        assert_eq!(f.steps[0].status, Some(Status::Done), "{row}");
    }
    assert!(flow::parse("sequenceDiagram\nA->>B: x\n%% Status: maybe\n").is_err());
}

#[test]
fn a_time_past_23_59_is_not_drawn() {
    for (row, time, ok) in [
        ("23:59", "23:59", true),
        ("00:00", "00:00", true),
        ("hour 24", "24:00", false),
        ("minute 60", "12:60", false),
        ("99:99", "99:99", false),
    ] {
        let got = flow::parse(&format!("sequenceDiagram\nA->>B: x\n%% status: ✓ {time}\n"));
        assert_eq!(got.is_ok(), ok, "{row}: {got:?}");
    }
}
