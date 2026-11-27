//! The flow renderer (#935): a Mermaid `sequenceDiagram` subset drawn as a
//! ladder or a numbered list that fits a width. Pure functions, literal
//! samples in tests/fixtures/flow, goldens in tests/golden.
//!
//! COCKPIT_BLESS=1 cargo test --test panel_flow  rewrites the goldens.
use cockpit_tui::highlight;
use cockpit_tui::panel::flow::{self, Block, Shape, Status};
use cockpit_tui::panel::prompts::EXPLAIN;
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

#[test]
fn goldens_six_actors_compact_at_61_and_normal_at_73() {
    // Slots of 10 and 12, the narrowest of each spacing (#1538).
    for w in [61, 73] {
        golden(&format!("flow-six-{w}"), &drawn(SIX, w));
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
fn six_actors_fall_back_to_the_list_where_a_slot_is_under_10() {
    let f = parsed(SIX);
    assert_eq!(f.actors.len(), 6);
    assert_eq!(flow::shape(&f, 200), Shape::Ladder);
    assert!(
        flow::render(SIX, 60, &UNI)[0].contains("owner─▶lead"),
        "{:?}",
        flow::render(SIX, 60, &UNI)
    );
}

/// six.mmd less GitHub.
const FIVE: &str = "sequenceDiagram\nautonumber\nparticipant O as owner\nparticipant L as lead\nparticipant A as agent\n\
participant R as reviewer\nparticipant C as CI\nO->>L: brief\nL->>A: task\nA->>C: push\nC-->>R: green\nR-->>L: VERDICT\nL-->>O: merge?\n";

#[test]
fn five_and_six_actors_draw_a_ladder_where_they_fit() {
    // No status and no time: each actor gets (w - 1) / actors columns, FIVE 11 at 60 and 12 at 61, SIX 9 at 60
    // and 10 at 61. A slot of 12 or more draws the ladder, 10 or 11 the compact one, less the list (#1538).
    assert_eq!(flow::shape(&parsed(FIVE), 60), Shape::Compact);
    assert_eq!(flow::shape(&parsed(FIVE), 61), Shape::Ladder);
    assert_eq!(flow::shape(&parsed(SIX), 60), Shape::List);
    assert_eq!(flow::shape(&parsed(SIX), 61), Shape::Compact);
}

#[test]
fn under_60_columns_the_list_draws_whatever_the_slot() {
    // The floor stays: two actors at 59 would get slots of 29, and FIVE slots of 11 (#1538).
    assert_eq!(flow::shape(&parsed("sequenceDiagram\nA->>B: x\n"), 59), Shape::List);
    assert_eq!(flow::shape(&parsed(FIVE), 59), Shape::List);
}

#[test]
fn the_slot_leaves_out_the_status_and_time_columns() {
    // With a status and a time each of SIX's actors gets (w - 3 - 6) / 6 columns: 9 at 68, 10 at 69, 11 at 80
    // and 12 at 81 (#1538).
    let timed = parsed(&SIX.replacen("O->>L: brief\n", "O->>L: brief\n%% status: done 09:00\n", 1));
    let got: Vec<Shape> = [68, 69, 80, 81].into_iter().map(|w| flow::shape(&timed, w)).collect();
    assert_eq!(got, [Shape::List, Shape::Compact, Shape::Compact, Shape::Ladder]);
}

#[test]
fn a_compact_ladder_keeps_each_lifeline_on_every_label_row() {
    // FIVE at 60, a slot of 11: its lifelines at 6, 17, 28, 39 and 50, each label cut before the next (#1538).
    let out = flow::render(FIVE, 60, &UNI);
    for row in (1..12).step_by(2) {
        assert_eq!(cols_of(&out[row], '│'), [6, 17, 28, 39, 50], "row {row}: {out:?}");
    }
    assert!(out[9].contains("│ 5 VERDI… │"), "{out:?}");
}

/// Six actors with a CJK name, label and note, inside a loop and an alt: at 61 each gets a slot of 10.
const WIDE_SIX: &str = "sequenceDiagram\nparticipant 名 as 名前の長い人\nparticipant B\nparticipant C\nparticipant D\n\
participant E\nparticipant F\nloop 長い\n名->>B: 漢字のラベル\nalt 赤\nB->>C: x\nNote over C: 漢字のノート\nend\nend\n";

#[test]
fn a_slot_of_10_keeps_its_lifelines_beside_labels_notes_and_frames() {
    // At 61 the lifelines are at 6, 16, 26, 36, 46 and 56, the frames' sides at 1 and 60, then 3 and 58. The
    // label's room is 7, so its CJK text is cut to 6 columns and the ellipsis before B's lifeline. The note
    // over C grows to 16 columns, 18 to 34, and covers C's lifeline alone. The alt's bottom crosses every
    // lifeline (#1538).
    assert_eq!(flow::shape(&parsed(WIDE_SIX), 61), Shape::Compact);
    let out = flow::render(WIDE_SIX, 61, &UNI);
    assert!(out[2].contains("│ 漢字の… │"), "{out:?}");
    let got = [(2, '│'), (5, '│'), (7, '│'), (8, '┼')].map(|(row, c)| cols_of(&out[row], c));
    let want: [&[usize]; 4] = [
        &[1, 6, 16, 26, 36, 46, 56, 60],
        &[1, 3, 6, 16, 26, 36, 46, 56, 58, 60],
        &[1, 3, 6, 16, 36, 46, 56, 58, 60],
        &[6, 16, 26, 36, 46, 56],
    ];
    assert_eq!(got, want, "{out:?}");
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

// ------------------------------------------- blocks as frames, Err the rest (#1536)

#[test]
fn loop_alt_opt_else_and_end_keep_their_steps_in_frames() {
    let f = parsed(LOOP);
    let labels: Vec<&str> = f.steps.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["push", "failure log", "pass", "rerun"]);
    // Each frame opens and closes after this many steps, and none of their notes: LOOP has none.
    let spot = |steps: usize| flow::Spot { steps, notes: 0 };
    let frame = |kind: Block, label: &str, depth: usize, open: usize, close: usize| flow::Frame {
        kind,
        label: label.into(),
        depth,
        open: spot(open),
        dividers: vec![],
        close: spot(close),
    };
    let mut alt = frame(Block::Alt, "red", 1, 1, 3);
    alt.dividers.push((spot(2), "green".into()));
    let want = [frame(Block::Loop, "until green", 0, 0, 3), alt, frame(Block::Opt, "flaky", 0, 3, 4)];
    assert_eq!(f.frames, want);
}

#[test]
fn a_line_no_grammar_knows_and_unbalanced_blocks_are_not_drawn() {
    for (row, src) in [
        ("unknown word", "sequenceDiagram\nA->>B: x\nfrobnicate A\n"),
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

// ------------------------------------------------- a label cut before the next lifeline (#1534)

/// A label longer than its gap: three actors, at 60 their lifelines at 10, 29 and 48.
const LONG_LABEL: &str = "sequenceDiagram\nparticipant A\nparticipant B\nparticipant C\n\
A->>B: a very long label that runs past the next lifeline\nB->>C: ok\n";

/// A long self step on the last actor, with a time: at 60 the lifelines are at 15 and 40, the time at 55.
const LAST_SELF: &str = "sequenceDiagram\nparticipant A\nparticipant B\n\
B->>B: a self step label long enough to reach the time column\n%% status: done 09:00\n";

#[test]
fn a_label_never_writes_over_a_lifeline() {
    assert_eq!(flow::render(LONG_LABEL, 60, &UNI)[1].matches('│').count(), 3);
    for w in [60, 100] {
        let out = flow::render(WIDE, w, &UNI);
        assert_eq!(out[1].matches('│').count(), 2, "w={w}");
        // Beside a CJK label each lifeline keeps the display column it has on the step's arrow row.
        assert_eq!(cols_of(&out[1], '│'), cols_of(&out[2], '│'), "w={w}: {out:?}");
    }
}

#[test]
fn a_cut_label_ends_a_blank_cell_before_the_next_lifeline() {
    // 16 columns from 12, two after A's lifeline, to 27: the label's first 15, then the ellipsis.
    // Column 28 stays blank before B's lifeline at 29.
    let row = format!("{}│ a very long lab… │{}│", " ".repeat(10), " ".repeat(18));
    assert_eq!(flow::render(LONG_LABEL, 60, &UNI)[1], row);
}

#[test]
fn a_label_right_of_the_last_lifeline_keeps_its_room_to_the_time_column() {
    // No lifeline right of B's at 40: 12 columns from 42 to 53, column 54 blank, the time at 55.
    let row = format!(" ✓{}│{}│ a self step… 09:00", " ".repeat(13), " ".repeat(24));
    assert_eq!(flow::render(LAST_SELF, 60, &UNI)[1], row);
}

// ------------------------------------------------- what the drawing leaves out (#1535)

#[test]
fn what_the_drawing_leaves_out_is_named_after_the_steps() {
    // The list at 50, the ladder at 60: the ledger follows the last step's rows and its frames' bottoms,
    // and comes before the NEED box.
    let named = "(dropped: frames past depth 3)";
    for w in [50, 60] {
        assert_eq!(flow::render(DEEP, w, &UNI).last().map(String::as_str), Some(named), "w={w}");
        assert!(!flow::render(BASE, w, &UNI).iter().any(|l| l.starts_with("(dropped:")), "w={w}");
        let need = flow::render(&format!("{DEEP}  Note over A: NEED FROM YOU: ok\n"), w, &UNI);
        let top = need.iter().position(|l| l.starts_with("┌ NEED FROM YOU")).expect("a NEED box");
        assert_eq!(need[top - 1], named, "w={w}");
    }
}

#[test]
fn a_kind_dropped_twice_is_named_once() {
    // Two nests four loops deep, one after the other: one name for both.
    let src = format!("{DEEP}{}", DEEP.replacen("sequenceDiagram", "", 1));
    let out = flow::render(&src, 60, &UNI);
    assert_eq!(out.last().map(String::as_str), Some("(dropped: frames past depth 3)"), "{out:?}");
}

// ------------------------------------------------- frames for loop, alt and opt (#1536)

/// Four loops, each inside the one before: the fourth is past MAX_FRAME_DEPTH.
const DEEP: &str = "sequenceDiagram\nloop a\nloop b\nloop c\nloop d\nA->>B: x\nend\nend\nend\nend\n";

/// Frames around everything else: four actors with a status and a time, a note before the first step
/// inside a frame and one after an end, an else, a self step on the last actor, and a block past
/// MAX_FRAME_DEPTH.
const FRAMES: &str = "sequenceDiagram\nparticipant O as owner\nparticipant A as agent\nparticipant R as reviewer\n\
participant C as CI\nloop until merged\nNote over O: starts here\nO->>A: brief\n%% status: done 09:00\nalt red\n\
opt flaky\nloop again\nA->>C: push\nC->>C: rerun\n%% status: running\nend\nend\nelse green\nC-->>R: pass\n\
Note right of C: a note in the alt\nend\nend\nNote over O,C: after the loop\nR->>O: merge?\n";

/// The index of the first row holding `s`, or `usize::MAX`.
fn at_in(rows: &[String], s: &str) -> usize {
    rows.iter().position(|l| l.contains(s)).unwrap_or(usize::MAX)
}

#[test]
fn loop_alt_and_opt_draw_as_frames_with_an_else_divider() {
    let ladder = flow::render(LOOP, 60, &UNI);
    assert!(ladder[1].starts_with(" ┌─[loop until green]"), "{ladder:?}");
    let at = |s: &str| at_in(&ladder, s);
    assert!(
        at("[alt red]") < at("failure log") && at("failure log") < at("├┄[else green]"),
        "{ladder:?}"
    );
    assert!(at("├┄[else green]") < at("pass") && at("┌─[opt flaky]") != usize::MAX, "{ladder:?}");
    // The alt inside the loop: its sides two columns in from the loop's, which stay drawn, its rule
    // crossing both lifelines (at 15 and 44) and its tag two columns in.
    let alt = format!(" │ ┌─[alt red]─┼{}┼{}┐ │", "─".repeat(28), "─".repeat(12));
    assert_eq!(ladder[4], alt, "{ladder:?}");
    // The opt opens at depth 0, once the alt and the loop have closed.
    assert!(ladder[12].starts_with(" ┌─[opt flaky]─┼"), "{ladder:?}");
    let list = flow::render(LOOP, 40, &UNI);
    assert_eq!(list[0], "┌─[loop until green]");
    assert!(list.iter().any(|l| l == "│├┄[else green]"), "{list:?}");
    assert!(
        flow::parse("sequenceDiagram\nloop x\nA->>B: y\nelse z\nend\n")
            .unwrap_err()
            .contains("else")
    );
    let after = flow::render("sequenceDiagram\nloop x\nA->>B: y\nend\nNote over A: after\n", 60, &UNI);
    let (note, close) = (at_in(&after, "after"), at_in(&after, " └"));
    assert!(close < note && note != usize::MAX, "{after:?}");
    // Outside the frame its row has no side: over A from the gutter to halfway to B, centred.
    assert_eq!(after[note], format!("{}↳ after{}│", " ".repeat(11), " ".repeat(26)), "{after:?}");
}

#[test]
fn an_else_right_before_a_block_draws_its_divider_first() {
    // The divider and the loop sit at one spot. The source has the else first, so the loop is in its branch.
    let src = "sequenceDiagram\nalt a\nA->>B: x\nelse b\nloop c\nB->>A: y\nend\nend\n";
    let out = flow::render(src, 60, &UNI);
    let at: Vec<usize> = ["[alt a]", "│ x", "├┄[else b]", "┌─[loop c]", "│ y"]
        .iter()
        .map(|s| at_in(&out, s))
        .collect();
    assert!(at.windows(2).all(|p| p[0] < p[1]) && at[4] != usize::MAX, "{at:?}: {out:?}");
}

#[test]
fn an_end_closes_the_innermost_open_block() {
    // The first end closes the alt, after x: y is inside the loop alone, with the loop's sides at 1 and 59.
    let src = "sequenceDiagram\nloop a\nalt b\nA->>B: x\nend\nB->>A: y\nend\n";
    let out = flow::render(src, 60, &UNI);
    let alt_bottom = format!(" │ └{}┼{}┼{}┘ │", "─".repeat(11), "─".repeat(28), "─".repeat(12));
    assert_eq!(out[5], alt_bottom, "{out:?}");
    assert_eq!(
        out[6],
        format!(" │{}│ y{}│{}│", " ".repeat(13), " ".repeat(26), " ".repeat(14)),
        "{out:?}"
    );
}

#[test]
fn the_list_draws_a_frame_after_the_step_numbers() {
    // The rule rows and the rails sit after the number column, and so does a note before the first step
    // inside the frame; one outside every frame starts its line as before. Aligned at 40, compact at 10.
    let src = "sequenceDiagram\nautonumber\nNote over A: out\nloop x\nNote over A: in\nA->>B: y\nend\n";
    assert_eq!(
        flow::render(src, 40, &UNI),
        ["↳ out", "   ┌─[loop x]", "   │↳ in", " 1 │y  A─▶B", "   └─"]
    );
    assert_eq!(
        flow::render(src, 10, &UNI),
        ["↳ out", "   ┌─[loo…", "   │↳ in", " 1 │y A─▶B", "   └─"]
    );
}

#[test]
fn a_block_past_depth_3_draws_its_steps_and_names_the_loss() {
    let out = flow::render(DEEP, 60, &UNI);
    assert_eq!(out.last().map(String::as_str), Some("(dropped: frames past depth 3)"), "{out:?}");
    // Frames for a, b and c around the step, none for d.
    assert_eq!(out.iter().filter(|l| l.contains("┌─[loop ")).count(), 3, "{out:?}");
    assert!(at_in(&out, "┌─[loop c]") < at_in(&out, "│ x"), "{out:?}");
}

#[test]
fn a_frame_keeps_every_lifeline_column_beside_wide_text() {
    let bare = "sequenceDiagram\nparticipant 名 as 名前\nparticipant B as b\n名->>B: 漢字\n";
    let lines = cols_of(&flow::render(bare, 60, &UNI)[2], '│');
    let framed = "sequenceDiagram\nparticipant 名 as 名前\nparticipant B as b\nloop 長い\n名->>B: 漢字\nend\n";
    let out = flow::render(framed, 60, &UNI);
    for row in [1, 4] {
        assert_eq!(cols_of(&out[row], '┼'), lines, "rule row {row}: {out:?}");
    }
    for row in [2, 3] {
        assert!(lines.iter().all(|c| cols_of(&out[row], '│').contains(c)), "row {row}: {out:?}");
    }
}

#[test]
fn a_note_in_a_frame_slides_inside_its_sides() {
    // Right of B at 44, inside a frame with its sides at 1 and 59: the note may use columns 3 to 57,
    // so its 16 columns slide in from 46..62 to 42..58.
    let src = "sequenceDiagram\nparticipant A\nparticipant B\nloop x\nA->>B: y\nNote right of B: in the loop\nend\n";
    let row = format!(" │{}│{}↳ in the loop{}│", " ".repeat(13), " ".repeat(26), " ".repeat(4));
    let out = flow::render(src, 60, &UNI);
    assert_eq!(out[4], row, "{out:?}");
}

#[test]
fn a_self_step_on_the_last_actor_keeps_inside_the_innermost_frame() {
    // FRAMES' rerun, a self step on C inside three frames: its arrow row at three widths (#1558). With no
    // status C's lifeline is at 53. At 61 the innermost side is at 56, so the row keeps to column 54 and the
    // mark keeps its head alone; at 62 the side is at 57, so it keeps its turn and head. At 65 with a status
    // and a time C's lifeline is at 52 and the side at 54, so no column is left for the mark.
    let bare = FRAMES.replace("%% status: done 09:00\n", "").replace("%% status: running\n", "");
    let gap = |n: usize| " ".repeat(n);
    let want = [
        format!(" │ │ │  │{0}│{0}│{0}│↩ │ │ │", gap(14)),
        format!(" │ │ │  │{0}│{0}│{0}│╮↩ │ │ │", gap(14)),
        format!("   │ │ │  │{0}│{0}│{0}│ │ │ │", gap(13)),
    ];
    let got: Vec<String> = [(bare.as_str(), 61), (bare.as_str(), 62), (FRAMES, 65)]
        .into_iter()
        .map(|(src, w)| flow::render(src, w, &UNI)[10].clone())
        .collect();
    assert_eq!(got, want);
}

/// Six actors and three loops, each inside the one before, around one step from the first actor to the last.
const NESTED_SIX: &str = "sequenceDiagram\nparticipant A\nparticipant B\nparticipant C\nparticipant D\nparticipant E\n\
participant F\nloop a\nloop b\nloop c\nA->>F: x\nend\nend\nend\n";

/// NESTED_SIX with a status and a time on its step.
fn nested_six_timed() -> String {
    NESTED_SIX.replace("A->>F: x\n", "A->>F: x\n%% status: done 09:00\n")
}

#[test]
fn a_frame_side_never_lands_on_the_last_lifeline() {
    // Wherever NESTED_SIX draws as a ladder, with no status and with a status and a time, the third frame's
    // corners sit right of the last lifeline. Rows: the header, three tops, the step's label and arrow, then
    // the third frame's bottom. A slot of 10 with nothing left over put both at 56 at 61, and at 58 at 69
    // with a status and a time (#1578).
    let timed = nested_six_timed();
    for src in [NESTED_SIX, timed.as_str()] {
        for w in (60..=130).filter(|&w| flow::shape(&parsed(src), w) != Shape::List) {
            let out = flow::render(src, w, &UNI);
            let last = cols_of(&out[5], '▶')[0] + 1;
            let sides = [cols_of(&out[3], '┐'), cols_of(&out[6], '┘')];
            assert!(sides.iter().all(|s| s.len() == 1 && s[0] > last), "w={w} last={last}: {out:?}");
        }
    }
}

#[test]
fn where_a_frame_side_would_land_on_the_last_lifeline_the_list_draws() {
    // At a slot of 10 with nothing left over the third frame's side would sit on the last lifeline, so the
    // list draws. A column more, or a frame less, and the compact ladder draws (#1578).
    let shapes = |src: &str, ws: [usize; 2]| ws.map(|w| flow::shape(&parsed(src), w));
    assert_eq!(shapes(NESTED_SIX, [61, 62]), [Shape::List, Shape::Compact]);
    assert_eq!(shapes(nested_six_timed().as_str(), [69, 70]), [Shape::List, Shape::Compact]);
    let two = NESTED_SIX.replacen("loop c\n", "", 1).replacen("end\n", "", 1);
    assert_eq!(flow::shape(&parsed(&two), 61), Shape::Compact);
}

// ------------------------------------------------- keep or drop each statement (#1537)

#[test]
fn statements_we_do_not_draw_are_dropped_and_named() {
    let created = "sequenceDiagram\nA->>B: x\ncreate participant C as Carol\nB->>C: hi\n";
    let unknown = "sequenceDiagram\nA->>B: x\nactivate Z\ndestroy Z\n";
    // A rect around three loops: the rect is no frame, so the third loop is still drawn as one.
    let rect_deep = "sequenceDiagram\nrect rgb(0, 0, 255)\nloop a\nloop b\nloop c\nA->>B: x\nend\nend\nend\nend\n";
    for (src, ledger) in [
        (
            "sequenceDiagram\nA->>B: x\nactivate B\nB-->>A: y\ndeactivate B\n",
            "(dropped: activations)",
        ),
        ("sequenceDiagram\nA->>+B: x\nB -->>- A: y\n", "(dropped: activations)"),
        ("sequenceDiagram\nA->>()B: x\nB()-->>A: y\n", "(dropped: central connections)"),
        (
            "sequenceDiagram\nrect rgb(0, 0, 255)\nA->>B: x\nend\n",
            "(dropped: rect highlights)",
        ),
        (
            "sequenceDiagram\nbox Aqua team\nparticipant A\nparticipant B\nend\nA->>B: x\n",
            "(dropped: participant boxes)",
        ),
        (
            "sequenceDiagram\nA->>B: x\ncreate participant C\nB->>C: hi\ndestroy C\nC-->>B: bye\n",
            "(dropped: create/destroy)",
        ),
        (created, "(dropped: create/destroy)"),
        (unknown, "(dropped: activations, create/destroy)"),
        (rect_deep, "(dropped: rect highlights)"),
    ] {
        let out = flow::render(src, 60, &UNI);
        assert_eq!(out.last().map(String::as_str), Some(ledger), "{src:?}: {out:?}");
    }
    // A dropped statement or mark adds no actor.
    for src in [
        "sequenceDiagram\nA->>+B: x\nB -->>- A: y\n",
        "sequenceDiagram\nA->>()B: x\nB()-->>A: y\n",
        unknown,
    ] {
        let ids: Vec<String> = parsed(src).actors.into_iter().map(|a| a.id).collect();
        assert_eq!(ids, ["A", "B"], "{src:?}");
    }
    assert!(
        !flow::render("sequenceDiagram\nrect rgb(0, 0, 255)\nA->>B: x\nend\n", 60, &UNI)
            .iter()
            .any(|l| l.contains('┌'))
    );
    // create reads its rest as a participant line, so C is named Carol.
    let out = flow::render(created, 60, &UNI);
    assert!(out[0].contains("Carol"), "{out:?}");
}

#[test]
fn the_ledger_names_each_kind_in_the_order_first_met() {
    for (src, ledger) in [
        (
            "sequenceDiagram\nA->>+B: x\nB()-->>A: y\n",
            "(dropped: activations, central connections)",
        ),
        (
            "sequenceDiagram\nA->>()B: x\nB-->>-A: y\n",
            "(dropped: central connections, activations)",
        ),
    ] {
        let out = flow::render(src, 60, &UNI);
        assert_eq!(out.last().map(String::as_str), Some(ledger), "{src:?}: {out:?}");
    }
}

#[test]
fn par_critical_and_break_draw_as_frames() {
    let rows: [(&str, &[&str]); 3] = [
        (
            "sequenceDiagram\npar to CI\nA->>C: push\nand to review\nA->>R: open\nend\n",
            &["┌─[par to CI]", "├┄[and to review]"],
        ),
        (
            "sequenceDiagram\ncritical lock held\nA->>B: x\noption timeout\nB->>A: y\nend\n",
            &["┌─[critical lock held]", "├┄[option timeout]"],
        ),
        ("sequenceDiagram\nbreak on red\nA->>B: x\nend\n", &["┌─[break on red]"]),
    ];
    for (src, marks) in rows {
        let out = flow::render(src, 60, &UNI);
        for m in marks {
            assert!(out.iter().any(|l| l.contains(m)), "{m}: {out:?}");
        }
    }
    // Each divider word divides its own block only: and a par, option a critical.
    for src in [
        "sequenceDiagram\nalt x\nA->>B: y\nand z\nend\n",
        "sequenceDiagram\npar x\nA->>B: y\noption z\nend\n",
    ] {
        assert!(flow::parse(src).is_err(), "{src:?}");
    }
}

#[test]
fn a_semicolon_ends_a_statement() {
    let labels = |src: &str| parsed(src).steps.into_iter().map(|s| s.label).collect::<Vec<_>>();
    assert_eq!(labels("sequenceDiagram\nA->>B: x; B-->>A: y\n"), ["x", "y"]);
    // The ; that closes an entity code ends nothing: #59; reads as ;, and any other code stays as written.
    assert_eq!(labels("sequenceDiagram\nA->>B: a #59; b\n"), ["a ; b"]);
    assert_eq!(labels("sequenceDiagram\nA->>B: a #27; b\n"), ["a #27; b"]);
    // A %% line is never split.
    assert_eq!(labels("sequenceDiagram\nA->>B: x\n%% a; b\n"), ["x"]);
}

#[test]
fn autonumber_starts_and_steps_as_written() {
    let src = "sequenceDiagram\nautonumber 10 5\nA->>B: x\nB->>A: y\n";
    assert_eq!(flow::render(src, 40, &UNI), ["10 x  A─▶B", "15 y  B─▶A"]);
    let ladder = flow::render(src, 60, &UNI);
    assert!(ladder[1].contains("10 x") && ladder[3].contains("15 y"), "{ladder:?}");
    // The list's number column is as wide as the largest number.
    let past = "sequenceDiagram\nautonumber 98 1\nA->>B: x\nB->>A: y\nA->>B: z\n";
    assert_eq!(flow::render(past, 40, &UNI), [" 98 x  A─▶B", " 99 y  B─▶A", "100 z  A─▶B"]);
    assert!(flow::parse("sequenceDiagram\nautonumber ten\nA->>B: x\n").is_err());
}

/// Every statement #1537 keeps or drops, two statements on one line, an entity code, and an autonumber start
/// near usize::MAX.
const STATEMENTS: &str = "sequenceDiagram\nautonumber 18446744073709551615 9\nbox Aqua team\nparticipant A\n\
participant B\nend\ncreate participant C as Carol\nrect rgb(0, 0, 255)\nA->>+B: x #59; y\nB-->>-A: z; activate A\nend\n\
par one\nA->>()C: p\nand two\nC()->>A: q\nend\ncritical lock\nB->>A: r\noption timeout\nA->>B: s\nend\nbreak red\n\
deactivate A\nA-xB: t\nend\ndestroy C\n%% a; b\n";

// ------------------------------------------- keep or drop the statements #1537 left (#1568)

/// `src` at 60, its first two actors A and B: it parses, its first label rows hold `labels` between their
/// lifelines, and its last line is the ledger `ledger`, or no ledger when that is none.
fn kept(src: &str, labels: &[&str], ledger: Option<&str>) {
    let got = flow::parse(src);
    assert!(got.is_ok(), "{src:?}: {got:?}");
    let out = flow::render(src, 60, &UNI);
    let between = |row: &String| row.split('│').nth(1).unwrap_or_default().trim().to_string();
    let rows: Vec<String> = out.iter().skip(1).step_by(2).take(labels.len()).map(between).collect();
    assert_eq!(rows, labels, "{src:?}: {out:?}");
    let last = out.last().filter(|l| l.starts_with("(dropped: ")).map(String::as_str);
    assert_eq!(last, ledger, "{src:?}: {out:?}");
}

#[test]
fn autonumber_with_a_start_counts_from_it_by_1() {
    kept("sequenceDiagram\nautonumber 5\nA->>B: x\nB->>A: y\n", &["5 x", "6 y"], None);
}

#[test]
fn autonumber_off_hides_the_numbers_while_the_count_runs_on() {
    let src = "sequenceDiagram\nautonumber\nA->>B: x\nautonumber off\nB->>A: y\nautonumber\nA->>B: z\n";
    kept(src, &["1 x", "y", "3 z"], None);
    // The list keeps the number column blank on a step with no number.
    assert_eq!(flow::render(src, 40, &UNI), [" 1 x  A─▶B", "   y  B─▶A", " 3 z  A─▶B"]);
}

#[test]
fn a_later_autonumber_restarts_with_a_start_or_goes_on_from_the_count() {
    // A start restarts the count, by 1 unless an increment is written.
    let reset = "sequenceDiagram\nautonumber 10 5\nA->>B: x\nautonumber 1 1\nB->>A: y\n";
    kept(reset, &["10 x", "1 y"], None);
    let restart = "sequenceDiagram\nautonumber 10 5\nA->>B: x\nautonumber 1\nB->>A: y\nA->>B: z\n";
    kept(restart, &["10 x", "1 y", "2 z"], None);
    // A step before the first autonumber is counted and draws no number.
    kept("sequenceDiagram\nA->>B: x\nautonumber\nB->>A: y\n", &["x", "2 y"], None);
    // The list's number column fits the largest number, wherever it is.
    let largest = "sequenceDiagram\nautonumber 100\nA->>B: x\nautonumber 1\nB->>A: y\n";
    assert_eq!(flow::render(largest, 40, &UNI), ["100 x  A─▶B", "  1 y  B─▶A"]);
}

#[test]
fn one_autonumber_at_the_top_or_none_draws_as_before() {
    // The list at 40 as before #1568, and the ladder at 60 with each number the first word of its label.
    let top = "sequenceDiagram\nautonumber\nA->>B: x\nB->>A: y\n";
    let none = "sequenceDiagram\nA->>B: x\nB->>A: y\n";
    assert_eq!(flow::render(top, 40, &UNI), [" 1 x  A─▶B", " 2 y  B─▶A"]);
    assert_eq!(flow::render(none, 40, &UNI), ["x  A─▶B", "y  B─▶A"]);
    let by_hand = flow::render("sequenceDiagram\nA->>B: 1 x\nB->>A: 2 y\n", 60, &UNI);
    assert_eq!(flow::render(top, 60, &UNI), by_hand);
}

#[test]
fn title_is_dropped_and_named() {
    for src in [
        "sequenceDiagram\ntitle Deploy flow\nA->>B: x\nB->>A: y\n",
        "sequenceDiagram\nA->>B: x\ntitle: Deploy flow\nB->>A: y\n",
    ] {
        kept(src, &["x", "y"], Some("(dropped: title)"));
    }
    // A title needs its text, as Mermaid's lexer does.
    assert!(flow::parse("sequenceDiagram\nA->>B: x\ntitle\n").is_err());
}

#[test]
fn acc_title_is_dropped_and_named() {
    // Its text runs to the end of the line, a `;` and all.
    for src in [
        "sequenceDiagram\nA->>B: x\naccTitle: Deploy steps\nB->>A: y\n",
        "sequenceDiagram\nA->>B: x\naccTitle: one; A->>B: two\nB->>A: y\n",
    ] {
        kept(src, &["x", "y"], Some("(dropped: accessibility text)"));
    }
}

#[test]
fn acc_descr_and_its_block_are_dropped_and_named() {
    for src in [
        "sequenceDiagram\nA->>B: x\naccDescr : The steps; all of them\nB->>A: y\n",
        // A block, over lines and statements, is dropped up to its `}`, and what follows it is read.
        "sequenceDiagram\nA->>B: x\naccDescr {\nA->>B: inside\n}\nB->>A: y\n",
        "sequenceDiagram\nA->>B: x\naccDescr { one; two } B->>A: y\n",
    ] {
        kept(src, &["x", "y"], Some("(dropped: accessibility text)"));
    }
}

/// An actor's menu line, `kw`, between two steps: dropped and named, and the participant it names is
/// kept, as Mermaid adds it.
fn a_menu_is_dropped(kw: &str) {
    let src = format!("sequenceDiagram\nparticipant A\n{kw} A: {{\"Dashboard\": \"https://x.test/d\"}}\nA->>B: x\nB->>A: y\n");
    kept(&src, &["x", "y"], Some("(dropped: actor menus)"));
    let named = parsed(&format!("sequenceDiagram\nA->>B: x\n{kw} C: Repo @ https://x.test/r\n"));
    let ids: Vec<String> = named.actors.into_iter().map(|a| a.id).collect();
    assert_eq!(ids, ["A", "B", "C"], "{kw}");
    // It needs a participant id and a ':'.
    for bad in [format!("{kw} A B: x"), format!("{kw} A")] {
        assert!(flow::parse(&format!("sequenceDiagram\nA->>B: x\n{bad}\n")).is_err(), "{bad}");
    }
}

#[test]
fn links_is_dropped_and_its_participant_kept() {
    a_menu_is_dropped("links");
}

#[test]
fn link_is_dropped_and_its_participant_kept() {
    a_menu_is_dropped("link");
}

#[test]
fn properties_is_dropped_and_its_participant_kept() {
    a_menu_is_dropped("properties");
}

#[test]
fn details_is_dropped_and_its_participant_kept() {
    a_menu_is_dropped("details");
}

#[test]
fn par_over_draws_as_a_par_frame() {
    let src = "sequenceDiagram\npar_over one\nA->>B: x\nand two\nB->>A: y\nend\n";
    let got = flow::parse(src);
    assert!(got.is_ok(), "{got:?}");
    let out = flow::render(src, 60, &UNI);
    for m in ["┌─[par one]", "├┄[and two]"] {
        assert!(out.iter().any(|l| l.contains(m)), "{m}: {out:?}");
    }
    assert!(!out.iter().any(|l| l.starts_with("(dropped:")), "{out:?}");
}

/// Every statement #1568 reads: a title, accessibility text and its block, autonumber's start, off and a
/// later one, the four menus, and a par_over.
const MORE_STATEMENTS: &str = "sequenceDiagram\ntitle Deploy flow\naccTitle: Deploy; steps\naccDescr {\nall; of it\n}\n\
autonumber 7\nparticipant A\nlinks A: {\"Dashboard\": \"https://x.test/d\"}\nlink B: Repo @ https://x.test/r\n\
properties A: {\"class\": \"internal\"}\ndetails B: {\"owner\": \"ops\"}\npar_over one\nA->>B: x\nautonumber off\n\
and two\nB->>A: y\nend\nautonumber\nA->>B: z\naccDescr: done; really\n";

// ------------------------------------------- the Mermaid 11.15.0 forms #1568 left (#1586)

/// `src` is not drawn: it is an `Err`, and at 60 render shows `not_drawn`.
fn refused(src: &str) {
    let got = flow::parse(src);
    assert!(got.is_err(), "{src:?}: {got:?}");
    let out = flow::render(src, 60, &UNI);
    assert!(out[0].starts_with("(not drawn: "), "{src:?}: {out:?}");
}

#[test]
fn a_half_arrow_is_not_drawn() {
    // Mermaid 11.15.0's sixteen half arrows: drawing one needs a half head in every glyph tier.
    let halves = [
        "-|\\", "-|/", "-\\\\", "-//", "--|\\", "--|/", "--\\\\", "--//", "/|-", "\\|-", "//-", "\\\\-", "/|--", "\\|--", "//--", "\\\\--",
    ];
    for a in halves {
        refused(&format!("sequenceDiagram\nA{a}B: x\n"));
    }
}

#[test]
fn a_keyword_in_another_case_is_not_drawn() {
    // Mermaid's lexer reads a keyword in any case. Here keywords are lowercase, and a note may start `Note`.
    for src in [
        "sequenceDiagram\nParticipant A\nA->>B: x\n",
        "sequenceDiagram\nAUTONUMBER\nA->>B: x\n",
        "sequenceDiagram\nA->>B: x\nNOTE over A: n\n",
        "sequenceDiagram\nLoop l\nA->>B: x\nend\n",
        "sequenceDiagram\nA-XB: x\n",
    ] {
        refused(src);
    }
    kept("sequenceDiagram\nA->>B: x\nNote over A: n\n", &["x"], None);
}

#[test]
fn a_participant_config_is_not_drawn() {
    // Mermaid's `@{ }` gives a participant a drawn shape and can rename it with "alias": not read here.
    for p in [
        "participant A@{ \"type\": \"database\" }",
        "participant A@{ \"type\": \"queue\" } as Alice",
        "actor A@{ \"type\": \"boundary\" }",
    ] {
        refused(&format!("sequenceDiagram\n{p}\nA->>B: x\n"));
    }
}

#[test]
fn a_hash_stays_in_the_text() {
    // Mermaid ends text at a `#` that starts no entity code. Here it stays, so an issue number keeps its `#`.
    kept(
        "sequenceDiagram\nA->>B: merge PR #1585\nB->>A: ok # late\n",
        &["merge PR #1585", "ok # late"],
        None,
    );
    let out = flow::render("sequenceDiagram\nloop every #3\nA->>B: x\nend\n", 60, &UNI);
    assert!(out.iter().any(|l| l.contains("[loop every #3]")), "{out:?}");
    // A line that starts with `#` is no statement here.
    refused("sequenceDiagram\n# a comment\nA->>B: x\n");
}

#[test]
fn a_zero_autonumber_start_or_increment_keeps_the_count_or_the_increment() {
    // Mermaid 11.15.0's renderer reads a 0 as none written (`start ||`, `step ||`): these are the numbers its
    // render drew for each.
    let rows: [(&str, &[&str]); 6] = [
        ("autonumber 0\nA->>B: x\nB->>A: y\n", &["1 x", "2 y"]),
        ("autonumber 5 0\nA->>B: x\nB->>A: y\n", &["5 x", "6 y"]),
        ("autonumber 0 2\nA->>B: x\nB->>A: y\n", &["1 x", "3 y"]),
        ("autonumber 0 0\nA->>B: x\nB->>A: y\n", &["1 x", "2 y"]),
        (
            "autonumber 10 5\nA->>B: x\nautonumber 0\nB->>A: y\nA->>B: z\n",
            &["10 x", "15 y", "16 z"],
        ),
        (
            "autonumber 10 5\nA->>B: x\nautonumber 3 0\nB->>A: y\nA->>B: z\n",
            &["10 x", "3 y", "8 z"],
        ),
    ];
    for (src, labels) in rows {
        kept(&format!("sequenceDiagram\n{src}"), labels, None);
    }
}

#[test]
fn an_acc_descr_block_never_closed_drops_the_rest() {
    // As Mermaid 11.15.0 reads it: the steps before the block draw, and everything after its `{` is dropped.
    for src in [
        "sequenceDiagram\nA->>B: x\naccDescr {\nsome text\nB->>A: y\n",
        "sequenceDiagram\nA->>B: x\naccDescr {\n",
    ] {
        kept(src, &["x"], Some("(dropped: accessibility text)"));
    }
    // A block whose end it drops is still never closed.
    refused("sequenceDiagram\nloop l\nA->>B: x\naccDescr {\nend\n");
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
            ("long label", LONG_LABEL),
            ("last self", LAST_SELF),
            ("deep", DEEP),
            ("frames", FRAMES),
            ("statements", STATEMENTS),
            ("more statements", MORE_STATEMENTS),
            ("five", FIVE),
            ("wide six", WIDE_SIX),
            ("nested six", NESTED_SIX),
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
    // WIDE and WIDE_SIX stay out: their own words are CJK, and the tier covers glyphs, not text.
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
        ("long label", LONG_LABEL),
        ("last self", LAST_SELF),
        ("deep", DEEP),
        ("frames", FRAMES),
        ("statements", STATEMENTS),
        ("more statements", MORE_STATEMENTS),
        ("five", FIVE),
        ("nested six", NESTED_SIX),
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
    // A frame's label is untrusted text too: its escape sequence goes whole and its bidi control goes (#1536).
    let framed = "sequenceDiagram\nloop a\x1b[31mb\u{202E}c\nA->>B: x\nend\n";
    let bad = |c: char| c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    for w in [0, 20, 60, 100] {
        for src in [WIDE, framed] {
            let out = flow::render(src, w, &UNI);
            assert!(out.iter().all(|l| !l.chars().any(bad)), "w={w}: {out:?}");
        }
        if w > 0 {
            let out = flow::render(framed, w, &UNI);
            assert!(out.iter().any(|l| l.contains("[loop abc]")), "w={w}: {out:?}");
        }
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
    // A frame deeper than parse makes, every spot past the end (#1536).
    let past = flow::Spot { steps: 99, notes: 99 };
    f.frames.push(flow::Frame {
        kind: Block::Loop,
        label: "past the end".into(),
        depth: 9,
        open: past,
        dividers: vec![(past, "and past".into())],
        close: past,
    });
    let no_actors = flow::Flow {
        steps: f.steps.clone(),
        frames: f.frames.clone(),
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

// ------------------------------------------- the Explain prompt's example (#1539)

#[test]
fn the_explain_prompts_example_flow_draws_whole_as_a_ladder_at_60() {
    // The prompt's mermaid fences, by the rule the Answer view draws them by.
    let lines: Vec<&str> = EXPLAIN.split('\n').collect();
    let bodies: Vec<String> = highlight::split(&lines)
        .iter()
        .filter_map(|p| highlight::flow_body(p, &lines))
        .collect();
    let body = bodies.first().map_or("", String::as_str);
    // 60 is the ladder's floor: the example's slots only widen past it. Its parties and labels keep to the
    // budget the prompt teaches, at most 4 parties and 9 columns a label, so nothing is cut.
    let out = flow::render(body, 60, &UNI);
    let checks = [
        ("one mermaid fence", bodies.len() == 1),
        ("a ladder", flow::parse(body).is_ok_and(|f| flow::shape(&f, 60) == Shape::Ladder)),
        ("drawn", !out.iter().any(|l| l.starts_with("(not drawn"))),
        ("nothing dropped", !out.iter().any(|l| l.starts_with("(dropped"))),
        ("nothing cut", !out.iter().any(|l| l.contains(UNI.ellipsis))),
    ];
    let failed: Vec<&str> = checks.iter().filter(|(_, ok)| !ok).map(|(name, _)| *name).collect();
    assert!(failed.is_empty(), "failed: {failed:?}\n{out:?}");
}
