//! `claude agents --json` costs ~380 ms, an order of magnitude more than the
//! rest of a tick, and it used to run on every one. Its cache is a thread-local
//! with no reset, so this test owns a whole test binary: a second test in this
//! file would inherit a warm cache and count zero forks.
mod fake;

use cockpit_tui::data::list_panes;
use fake::write_exec;

#[test]
fn two_ticks_inside_the_ttl_fork_claude_once() {
    // first, before the rig's folder: the runner unset the caller's
    // log, agents cache and state folder, which a lookup uses (#1066)
    fake::checked_state();
    let dir = std::env::temp_dir().join(format!("cockpit-tui-forks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let d = dir.display().to_string();
    // One agent pane with no @cockpit_session, which is the only path that asks
    // `claude` at all.
    let row = ["%1", "w:0", "win", "claude", "running", "-", "-", "-", "/w", "100", "-", "-", "@1"].join("\u{1f}");
    std::fs::write(dir.join("rows"), row + "\n").unwrap();
    std::fs::write(dir.join("ps"), "100 1 claude\n").unwrap();
    write_exec(
        &dir.join("bin/tmux"),
        &format!("#!/bin/sh\ncase \"$1\" in list-panes) cat {d}/rows ;; esac\n"),
    );
    write_exec(&dir.join("bin/ps"), &format!("#!/bin/sh\ncat {d}/ps\n"));
    write_exec(&dir.join("bin/claude"), &format!("#!/bin/sh\necho 1 >> {d}/forks\nprintf '[]'\n"));
    write_exec(&dir.join("bin/git"), "#!/bin/sh\nexit 1\n");
    let path = format!("{d}/bin:{}", std::env::var("PATH").unwrap_or_default());
    // SAFETY: the one test of this binary that writes the env. The other reads it only
    // in its spawn, which std orders with these writes (fake::run_by_hand).
    unsafe {
        fake::set_var_unlocked("PATH", path);
        fake::set_var_unlocked("COCKPIT_TMUX", format!("{d}/bin/tmux"));
        fake::set_var_unlocked("HOME", &d);
        fake::set_var_unlocked("COCKPIT_READONLY", "1");
        // #505: no proc under it, so the walk asks the fake ps, not the live pid of the same number
        fake::set_var_unlocked("COCKPIT_PROC_DIR", format!("{d}/proc"));
    }
    fake::assert_claude_is_the_rigs(&dir);

    list_panes().unwrap();
    // the lookup runs on a thread of its own (#492): wait for its answer file
    let cache = dir.join(".cache/tmux-cockpit/agents.json");
    for _ in 0..2000 {
        if cache.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    list_panes().unwrap();
    list_panes().unwrap();

    let forks = std::fs::read_to_string(dir.join("forks")).map(|s| s.lines().count()).unwrap_or(0);
    assert_eq!(forks, 1, "the 60 s cache did not hold across two ticks");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #1066: started by hand with a caller's COCKPIT_LOG kept, this binary's rig
/// refuses before a lookup can write a line there. The runner unsets it.
#[test]
fn a_kept_cockpit_log_is_refused() {
    let kept = [("COCKPIT_LOG", "/canary-1066/log")];
    let (code, said) = fake::run_by_hand("two_ticks_inside_the_ttl_fork_claude_once", &kept);
    let refused = said.contains("the caller's state refused (#1066): COCKPIT_LOG set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}

/// #1066: started by hand with a caller's XDG_CACHE_HOME kept, this binary's rig
/// refuses before a lookup can read or write the agents cache there.
#[test]
fn a_kept_agents_cache_is_refused() {
    let kept = [("XDG_CACHE_HOME", "/canary-1066/cache")];
    let (code, said) = fake::run_by_hand("two_ticks_inside_the_ttl_fork_claude_once", &kept);
    let refused = said.contains("the caller's state refused (#1066): XDG_CACHE_HOME set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}
