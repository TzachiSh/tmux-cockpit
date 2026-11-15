//! The batched tiles read byte for byte what `data::preview` reads, one fork
//! per pane, as the board did before #492. Its own binary: `preview` reads
//! COCKPIT_TMUX, which this sets.
// rig.rs writes its counting wrapper with it, which this binary never asks for
#[allow(dead_code)]
mod fake;
#[path = "tiles/rig.rs"]
mod rig;

use cockpit_tui::board;
use cockpit_tui::data::preview;

#[test]
fn the_batch_reads_what_preview_reads_per_pane() {
    let Some(r) = rig::rig("preview") else { return };
    // SAFETY: the one test of this binary that writes the env; the others read it only
    // in their spawns, which std orders with this write (fake::rows_pass_handed).
    unsafe { fake::set_var_unlocked("COCKPIT_TMUX", format!("tmux -L {}", r.sock)) };
    let caps: Vec<(String, usize)> = r.ids.iter().map(|id| (id.clone(), 5)).collect();
    let want: Vec<_> = caps.iter().map(|(id, n)| (id.clone(), preview(id, *n))).collect();
    assert!(want.iter().all(|(_, pv)| pv.as_ref().is_some_and(|s| !s.is_empty())), "{want:?}");
    assert_eq!(board::previews(&r.argv(), &caps), want);
}

/// #1061, #1066: this binary's rig row, started again with TERM and HUP ignored and blocked, as a
/// suite under nohup or with them blocked hands them down: the rig's server, told to
/// go by a TERM it sends itself, is gone afterwards with its panes.
#[test]
#[cfg(target_os = "linux")]
fn the_rig_rows_leave_no_server_handed_a_blocked_term_and_hup() {
    let rows = ["the_batch_reads_what_preview_reads_per_pane"];
    let pid = fake::rows_pass_handed(&rows, &[libc::SIGTERM, libc::SIGHUP]);
    fake::assert_no_server_left(pid);
}
