//! The batched tiles read byte for byte what `data::preview` reads, one fork
//! per pane, as the board did before #492. Its own binary: `preview` reads
//! COCKPIT_TMUX, which this sets.
#[path = "tiles/rig.rs"]
mod rig;

use cockpit_tui::board;
use cockpit_tui::data::preview;

#[test]
fn the_batch_reads_what_preview_reads_per_pane() {
    let Some(r) = rig::rig("preview") else { return };
    // SAFETY: this test binary holds exactly one test, so nothing runs beside it.
    unsafe { std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", r.sock)) };
    let caps: Vec<(String, usize)> = r.ids.iter().map(|id| (id.clone(), 5)).collect();
    let want: Vec<_> = caps.iter().map(|(id, n)| (id.clone(), preview(id, *n))).collect();
    assert!(want.iter().all(|(_, pv)| pv.as_ref().is_some_and(|s| !s.is_empty())), "{want:?}");
    assert_eq!(board::previews(&r.argv(), &caps), want);
}
