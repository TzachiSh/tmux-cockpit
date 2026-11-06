//! The batched tiles and `data::preview`, one call per pane, fed the same
//! fixed captures, read byte-identical tiles (#492). Its own binary:
//! `preview` reads COCKPIT_TMUX, which this sets.
mod fake;
#[path = "tiles/stub.rs"]
mod stub;

use cockpit_tui::board;
use cockpit_tui::data::preview;

#[test]
fn the_batch_and_preview_read_the_same_tiles_off_the_same_captures() {
    let (dir, argv) = stub::stub("same");
    // SAFETY: this test binary holds exactly one test, so nothing runs beside it.
    unsafe { std::env::set_var("COCKPIT_TMUX", &argv[0]) };
    let all: Vec<&str> = stub::screens().into_iter().map(|(id, _)| id).collect();
    // (name, the tiles, the stub's calls for the batch)
    let rows: [(&str, &[&str], usize); 3] = [
        ("one batch", &all[..6], 1),
        ("a pane holding its own mark", &all[..], 1 + 7),
        // review-510's probe: a forged mark, then a pane gone in the same call
        ("its own mark, then a gone pane", &["%7", "%1", "%999", "%2"], 1 + 4),
    ];
    for (name, ids, calls) in rows {
        let caps: Vec<(String, usize)> = ids.iter().map(|id| (id.to_string(), 5)).collect();
        let caps = &caps[..];
        let want: Vec<_> = caps.iter().map(|(id, n)| (id.clone(), preview(id, *n))).collect();
        let before = stub::calls(&dir);
        assert_eq!(board::previews(&argv, caps), want, "{name}");
        assert_eq!(stub::calls(&dir) - before, calls, "{name}");
    }
}
