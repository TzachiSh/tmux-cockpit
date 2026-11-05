//! #452: the info screen names the work dir the board's stat used. Its own
//! binary: `gather` reads COCKPIT_TMUX and CLAUDE_CONFIG_DIR, process-global.
use cockpit_tui::data::info::{Facts, gather};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// Pane %1 runs in `cwd`, `@cockpit_dir` is `opt`, and its transcript says `said`.
fn facts(d: &PathBuf, cwd: &str, opt: &str, said: &str) -> Facts {
    std::fs::create_dir_all(d.join("projects/-w")).unwrap();
    let t = d.join("projects/-w/s1.jsonl");
    std::fs::write(&t, format!("{}\n", serde_json::json!({"type": "user", "cwd": said, "message": {}}))).unwrap();
    let f = ["%1", "w:0 win", "claude", cwd, opt, "running", "", "", "", "s1", "", "1", ""];
    std::fs::write(d.join("display"), f.join("\u{1f}")).unwrap();
    let shim = d.join("tmux");
    let sh = format!(
        "#!/bin/sh\ncase \"$1\" in\ndisplay-message) cat {0}/display ;;\nshow-option) echo {1} ;;\nesac\n",
        d.display(),
        t.display()
    );
    std::fs::write(&shim, sh).unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: the one test in this binary, so no thread reads the environment meanwhile.
    unsafe {
        std::env::set_var("COCKPIT_TMUX", &shim);
        std::env::set_var("CLAUDE_CONFIG_DIR", d);
    }
    gather("%1").unwrap()
}

/// Rows: the transcript's cwd over the pane's; `@cockpit_dir` over both; a
/// dangling transcript cwd falls to the pane's. `root` stays tmux-review's.
#[test]
fn the_work_dir_follows_the_board_order_and_root_does_not() {
    let d = std::env::temp_dir().join(format!("cockpit-info-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let (wt, opt) = (d.join("wt"), d.join("opt"));
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::create_dir_all(&opt).unwrap();
    let (wt, opt, cwd) = (wt.display().to_string(), opt.display().to_string(), d.display().to_string());
    let x = facts(&d, &cwd, "", &wt);
    assert_eq!((x.work.as_str(), x.root.as_str()), (wt.as_str(), cwd.as_str()));
    assert_eq!(facts(&d, &cwd, &opt, &wt).work, opt);
    assert_eq!(facts(&d, &cwd, "", "/no/such/wt").work, cwd);
    let _ = std::fs::remove_dir_all(&d);
}
