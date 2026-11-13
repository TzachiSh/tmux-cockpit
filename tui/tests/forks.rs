//! `claude agents --json` costs ~380 ms, an order of magnitude more than the
//! rest of a tick, and it used to run on every one. Its cache is a thread-local
//! with no reset, so this test owns a whole test binary: a second test in this
//! file would inherit a warm cache and count zero forks.
mod fake;

use cockpit_tui::data::list_panes;
use fake::write_exec;

#[test]
fn two_ticks_inside_the_ttl_fork_claude_once() {
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
    // SAFETY: this test binary holds exactly one test, so nothing runs beside it.
    unsafe {
        std::env::set_var("PATH", format!("{d}/bin:{}", std::env::var("PATH").unwrap_or_default()));
        std::env::set_var("COCKPIT_TMUX", format!("{d}/bin/tmux"));
        std::env::set_var("HOME", &d);
        std::env::set_var("COCKPIT_READONLY", "1");
        // #505: no proc under it, so the walk asks the fake ps, not the live pid of the same number
        std::env::set_var("COCKPIT_PROC_DIR", format!("{d}/proc"));
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
