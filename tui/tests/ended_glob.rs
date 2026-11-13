//! #301: with no agent list (`@cockpit_agents_glob` empty, the plugin not
//! loaded) a live `claude` looks like any command, so nothing is ended. Its own
//! test binary: the agent list is a thread-local read once, the env process-wide.
mod fake;

use cockpit_tui::data::list_panes;
use fake::write_exec;

#[test]
fn an_empty_agent_list_ends_nothing() {
    let dir = std::env::temp_dir().join(format!("cockpit-tui-ended-glob-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let d = dir.display().to_string();
    let row = [
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
    ]
    .join("\u{1f}");
    std::fs::write(dir.join("rows"), row + "\n").unwrap();
    // show-option prints nothing: the glob is empty
    write_exec(
        &dir.join("bin/tmux"),
        &format!("#!/bin/sh\ncase \"$1\" in list-panes) cat {d}/rows ;; esac\n"),
    );
    write_exec(&dir.join("bin/ps"), "#!/bin/sh\necho '100 1 claude'\n");
    write_exec(&dir.join("bin/git"), "#!/bin/sh\nexit 1\n");
    let path = fake::rig_path(&dir.join("bin"));
    // SAFETY: this test binary holds exactly one test, so nothing runs beside it.
    unsafe {
        std::env::set_var("PATH", path);
        std::env::set_var("COCKPIT_TMUX", format!("{d}/bin/tmux"));
        std::env::set_var("HOME", &d);
        std::env::set_var("COCKPIT_READONLY", "1");
        // #505: no proc under it, so the walk asks the fake ps, not the live pid of the same number
        std::env::set_var("COCKPIT_PROC_DIR", format!("{d}/proc"));
    }
    fake::assert_claude_is_the_rigs(&dir);

    let ps = list_panes().unwrap();
    assert_eq!(ps.len(), 1, "the marked pane is listed");
    assert!(!ps[0].ended, "a claude the empty agent list cannot name is not ended");
    let _ = std::fs::remove_dir_all(&dir);
}
