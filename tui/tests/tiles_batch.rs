//! The tiles' previews in one tmux call (#492): the split at the marks, and
//! the batch against a private server, on an explicit argv (no env read).
mod fake;
#[path = "tiles/rig.rs"]
mod rig;

use cockpit_tui::board::{self, Batch};
use cockpit_tui::data::{preview_args, shape_preview};

const N: usize = 5;

fn caps(ids: &[&str]) -> Vec<(String, usize)> {
    ids.iter().map(|id| (id.to_string(), N)).collect()
}

/// One capture alone on `argv`, shaped: the per-pane read the batch replaces.
fn alone(argv: &[String], id: &str) -> Option<String> {
    let o = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .args(preview_args(id, N))
        .output()
        .ok()?;
    o.status.success().then(|| shape_preview(&String::from_utf8_lossy(&o.stdout), N))
}

#[test]
fn the_split_cuts_at_whole_mark_lines_only() {
    let m = "tok#";
    // (name, stdout, captures, tmux exit 0, the split)
    let rows: [(&str, &[u8], usize, bool, Batch); 10] = [
        (
            "two captures",
            b"a\nb\ntok#\nc\ntok#\n",
            2,
            true,
            Batch::Done(vec![b"a\nb\n", b"c\n"]),
        ),
        ("an empty capture", b"tok#\nc\ntok#\n", 2, true, Batch::Done(vec![b"", b"c\n"])),
        ("the mark inside a line", b"x tok#\ntok#\n", 1, true, Batch::Done(vec![b"x tok#\n"])),
        ("the argv's spelling", b"tok##\ntok#\n", 1, true, Batch::Done(vec![b"tok##\n"])),
        ("tmux stopped at the second", b"a\ntok#\n", 3, false, Batch::Cut(vec![b"a\n"])),
        ("tmux stopped at the first", b"", 2, false, Batch::Cut(vec![])),
        ("a mark too many", b"tok#\ntok#\ntok#\n", 2, true, Batch::Lost),
        ("text after the last mark", b"a\ntok#\nb\n", 1, true, Batch::Lost),
        ("exit 0 but a mark short", b"a\ntok#\n", 2, true, Batch::Lost),
        ("a failure with every mark", b"tok#\ntok#\n", 2, false, Batch::Lost),
    ];
    for (name, out, n, ok, want) in rows {
        assert_eq!(board::split_batch(out, m, n, ok), want, "{name}");
    }
}

#[test]
fn the_batch_is_each_capture_then_its_mark() {
    let mut want: Vec<String> = preview_args("%1", N).into();
    want.extend([";", "display-message", "-p", "tok##", ";"].map(String::from));
    want.extend(preview_args("%2", N));
    want.extend([";", "display-message", "-p", "tok##"].map(String::from));
    assert_eq!(board::batch_args(&caps(&["%1", "%2"]), "tok"), want);
}

#[test]
fn every_call_draws_a_new_mark() {
    assert_ne!(board::token(), board::token());
}

#[test]
fn every_tile_is_one_tmux_call() {
    let Some(r) = rig::rig("one") else { return };
    let ids: Vec<&str> = r.ids.iter().map(String::as_str).collect();
    let got = board::previews(&r.counting(""), &caps(&ids));
    assert_eq!(r.calls(), 1, "{got:?}");
    assert!(got.iter().all(|(_, pv)| pv.is_some()), "{got:?}");
}

#[test]
fn the_batch_reads_what_each_capture_alone_reads() {
    let Some(r) = rig::rig("same") else { return };
    let ids: Vec<&str> = r.ids.iter().map(String::as_str).collect();
    let got = board::previews(&r.argv(), &caps(&ids));
    let want: Vec<_> = ids.iter().map(|id| (id.to_string(), alone(&r.argv(), id))).collect();
    assert_eq!(got, want);
}

#[test]
fn a_pane_gone_mid_batch_reads_none_and_the_rest_go_again() {
    let Some(r) = rig::rig("gone") else { return };
    let ids = [r.ids[0].as_str(), "%999", r.ids[1].as_str(), r.ids[2].as_str()];
    let got = board::previews(&r.counting(""), &caps(&ids));
    let want: Vec<_> = ids.iter().map(|id| (id.to_string(), alone(&r.argv(), id))).collect();
    assert_eq!(got, want);
    assert_eq!(want[1].1, None, "the gone pane");
    assert_eq!(r.calls(), 2, "the batch, then the panes after the gone one");
}

#[test]
fn the_last_pane_gone_costs_no_second_call() {
    let Some(r) = rig::rig("last") else { return };
    let got = board::previews(&r.counting(""), &caps(&[r.ids[0].as_str(), "%999"]));
    assert_eq!(got[1], ("%999".to_string(), None));
    assert_eq!(r.calls(), 1);
}

#[test]
fn a_mark_in_the_output_falls_back_to_a_call_per_pane() {
    let Some(r) = rig::rig("mark") else { return };
    let ids: Vec<&str> = r.ids.iter().map(String::as_str).collect();
    // the stand-in prints this batch's mark once more, as a pane holding it would
    let got = board::previews(&r.counting(r#"[ -n "$mark" ] && echo "$mark""#), &caps(&ids));
    let want: Vec<_> = ids.iter().map(|id| (id.to_string(), alone(&r.argv(), id))).collect();
    assert_eq!(got, want);
    assert_eq!(r.calls(), 1 + ids.len());
}

#[test]
fn a_tmux_that_does_not_start_reads_none_for_every_tile() {
    let got = board::previews(&["/nonexistent/tmux".to_string()], &caps(&["%1", "%2"]));
    assert_eq!(got, vec![("%1".to_string(), None), ("%2".to_string(), None)]);
}
