//! #301: with no agent list (`@cockpit_agents_glob` empty, the plugin not
//! loaded) a live `claude` looks like any command, so nothing is ended. Its own
//! test binary: the agent list is a thread-local read once, the env process-wide.
mod fake;

use cockpit_tui::data::list_panes;
use fake::write_exec;

#[test]
fn an_empty_agent_list_ends_nothing() {
    // first, before the rig's folder: the runner unset the caller's
    // agents cache and state folder, which a lookup uses (#1066)
    fake::checked_state();
    let dir = std::env::temp_dir().join(format!("cockpit-tui-ended-glob-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let d = dir.display().to_string();
    let row = fake::board_row(&[
        "%1",
        "w:0",
        "win",
        "claude",
        "needs-input",
        "-",
        "-",
        "s1",
        "/w",
        "100",
        "-",
        "-",
        "@1",
    ]);
    std::fs::write(dir.join("rows"), row + "\n").unwrap();
    // show-option prints nothing: the glob is empty
    write_exec(
        &dir.join("bin/tmux"),
        &format!("#!/bin/sh\ncase \"$1\" in list-panes) cat {d}/rows ;; esac\n"),
    );
    write_exec(&dir.join("bin/ps"), "#!/bin/sh\necho '100 1 claude'\n");
    write_exec(&dir.join("bin/git"), "#!/bin/sh\nexit 1\n");
    let path = fake::rig_path(&dir.join("bin"));
    let held = fake::env_lock();
    // SAFETY: the one test of this binary that writes the env. The other reads it only
    // in its spawn, which std orders with these writes (fake::run_by_hand).
    unsafe {
        fake::set_var(&held, "PATH", path);
        fake::set_var(&held, "COCKPIT_TMUX", format!("{d}/bin/tmux"));
        fake::set_var(&held, "HOME", &d);
        fake::set_var(&held, "COCKPIT_READONLY", "1");
        // #505: no proc under it, so the walk asks the fake ps, not the live pid of the same number
        fake::set_var(&held, "COCKPIT_PROC_DIR", format!("{d}/proc"));
    }
    drop(held);
    fake::assert_claude_is_the_rigs(&dir);

    let ps = list_panes().unwrap();
    assert_eq!(ps.len(), 1, "the marked pane is listed");
    assert!(!ps[0].ended, "a claude the empty agent list cannot name is not ended");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #1066: started by hand with a caller's XDG_CACHE_HOME kept, this binary's rig
/// refuses before a lookup can read or write the agents cache there.
#[test]
fn a_kept_agents_cache_is_refused() {
    let kept = [("XDG_CACHE_HOME", "/canary-1066/cache")];
    let (code, said) = fake::run_by_hand("an_empty_agent_list_ends_nothing", &kept);
    let refused = said.contains("the caller's state refused (#1066): XDG_CACHE_HOME set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}
