//! #301: an ended row (a marked pane with no agent in its command) asks `ps`
//! for its process tree once per cache window, not once per tick. Its own test
//! binary: the verdict cache is a thread-local and the env is process-wide.
mod fake;

use cockpit_tui::data::list_panes;
use fake::write_exec;

#[test]
fn two_ticks_on_an_ended_row_fork_ps_once() {
    let dir = std::env::temp_dir().join(format!("cockpit-tui-forks-ended-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let d = dir.display().to_string();
    let row = [
        "%1",
        "w:0",
        "win",
        "sleep",
        "needs-input",
        "-",
        "-",
        "s1",
        "/w",
        "100",
        "-",
        "-",
        "@1",
    ]
    .join("\u{1f}");
    std::fs::write(dir.join("rows"), row + "\n").unwrap();
    write_exec(
        &dir.join("bin/tmux"),
        &format!("#!/bin/sh\ncase \"$1\" in list-panes) cat {d}/rows ;; show-option) echo '|claude|' ;; esac\n"),
    );
    write_exec(&dir.join("bin/ps"), &format!("#!/bin/sh\necho 1 >> {d}/ps.n\necho '100 1 sleep'\n"));
    write_exec(&dir.join("bin/git"), "#!/bin/sh\nexit 1\n");
    let path = fake::rig_path(&dir.join("bin"));
    // the runner unset the caller's log, which a lookup writes to (#1066)
    fake::checked_state();
    // SAFETY: the one test of this binary that writes the env. The other reads it only
    // in its spawn, which std orders with these writes (fake::run_by_hand).
    unsafe {
        std::env::set_var("PATH", path);
        std::env::set_var("COCKPIT_TMUX", format!("{d}/bin/tmux"));
        std::env::set_var("HOME", &d);
        std::env::set_var("COCKPIT_READONLY", "1");
        // #505: no proc under it, so the walk asks the fake ps, not the live pid of the same number
        std::env::set_var("COCKPIT_PROC_DIR", format!("{d}/proc"));
    }
    fake::assert_claude_is_the_rigs(&dir);

    assert!(list_panes().unwrap()[0].ended, "the first tick reads it ended");
    assert!(list_panes().unwrap()[0].ended, "and the second, off the cache");

    let forks = std::fs::read_to_string(dir.join("ps.n")).map(|s| s.lines().count()).unwrap_or(0);
    assert_eq!(forks, 1, "an ended row forked ps on every tick");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #1066: started by hand with a caller's COCKPIT_LOG kept, this binary's rig
/// refuses before a lookup can write a line there. The runner unsets it.
#[test]
fn a_kept_cockpit_log_is_refused() {
    let (code, said) = fake::run_by_hand("two_ticks_on_an_ended_row_fork_ps_once", &[("COCKPIT_LOG", "/canary-1066/log")]);
    let refused = said.contains("the caller's state refused (#1066): COCKPIT_LOG set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}
