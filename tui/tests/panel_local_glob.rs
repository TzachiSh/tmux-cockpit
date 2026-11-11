//! #840: the panel's `is_local` reads the agent test the board uses, so a window
//! whose command tmux shows as a relative path (`./claude`) is not local: no repo
//! block, no read of its folder. Every shell glob site says no agent there too.
//! Its own test binary: the agent list is a thread-local read once, the env
//! process-wide.
mod fake;

use cockpit_tui::panel::capture::is_local;
use fake::{Scratch, write_exec};

#[test]
fn a_relative_claude_window_is_not_local() {
    let s = Scratch::fresh(std::env::temp_dir().join(format!("cockpit-tui-local-glob-{}", std::process::id())));
    let d = s.0.display().to_string();
    // the one tmux read is the glob; zzagent is local through it alone
    write_exec(
        &s.0.join("tmux"),
        "#!/bin/sh\ncase \"$1\" in show-option) echo '|claude|zzagent|' ;; esac\n",
    );
    // SAFETY: this test binary holds exactly one test, so nothing runs beside it.
    unsafe { std::env::set_var("COCKPIT_TMUX", format!("{d}/tmux")) };
    let rows = [
        ("./claude", false),
        ("x/claude", false),
        ("zzagent", true),   // the glob is read
        ("./2.1.318", true), // a version title, path and all
    ];
    let wrong: Vec<_> = rows.iter().filter(|(c, w)| is_local(c) != *w).collect();
    assert!(wrong.is_empty(), "is_local differs: {wrong:?}");
}
