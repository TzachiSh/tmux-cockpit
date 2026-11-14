//! FIELDS against a real tmux server on a private socket (#313): the options the hooks write
//! come back as `Pane.subs_n` and `Pane.subs`. `subs_fields.rs` pins the parser on canned
//! rows; this pins the format string. Its own binary: `list_panes` reads the env.
mod fake;
use cockpit_tui::data::list_panes;
use std::path::PathBuf;
use std::process::Command;

struct Server {
    sock: String,
    dir: PathBuf,
    /// killed with its socket before the folder goes
    _server: fake::Server,
}

impl Server {
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self._server.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn the_hook_options_reach_the_pane_through_the_format() {
    // first, before the rig's folder: the runner unset the caller's
    // log, agents cache and state folder, which a lookup uses (#1066)
    fake::checked_state();
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("no tmux on PATH -- skipping");
        return;
    }
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cockpit-subs-tmux-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    let subs = dir.join(".claude/projects/-p/s1/subagents");
    std::fs::create_dir_all(&subs).unwrap();
    std::fs::write(subs.join("agent-a.meta.json"), r#"{"agentType":"x","description":"live"}"#).unwrap();
    let t = dir.join(".claude/projects/-p/s1.jsonl");
    std::fs::write(&t, "").unwrap();
    let sock = format!("cockpit-subs-{pid}");
    let d = dir.display().to_string();
    // built first, so a start that panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir: dir.clone(),
    };
    s._server.start(|c| {
        c.args(["new-session", "-d", "-s", "t", "-c", &d, "exec sleep 60"]);
    });
    let p = s.tmux(&["display-message", "-p", "-t", "t", "#{pane_id}"]);
    let t = t.display().to_string();
    for (k, v) in [("@panestate", "running"), ("@cockpit_subs", "3"), ("@cockpit_transcript", &t)] {
        s.tmux(&["set-option", "-p", "-t", &p, k, v]);
    }
    // The pane has no session, so `list_panes` looks one up: `claude agents --json`,
    // found by PATH. The rig's own answers it, the one `rig_path` writes, which counts
    // its starts beside itself. `tmux` still comes from the rest of PATH (#972).
    let path = fake::rig_path(&dir.join("bin"));
    // SAFETY: the one test of this binary that writes the env. The others read it only
    // in their spawns, which std orders with these writes (fake::run_by_hand).
    unsafe {
        std::env::set_var("PATH", path);
        std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", s.sock));
        std::env::set_var("HOME", &d);
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::env::set_var("COCKPIT_READONLY", "1");
    }
    fake::assert_claude_is_the_rigs(&dir);
    let ps = list_panes().unwrap();
    let started = std::fs::read_to_string(dir.join("bin/claude.started")).unwrap_or_default();
    assert_eq!(started, "x\n", "the lookup did not start the rig's claude once");
    let got: Vec<(u32, Vec<String>)> = ps
        .iter()
        .map(|p| (p.subs_n, p.subs.iter().map(|s| s.desc.clone()).collect()))
        .collect();
    assert_eq!(got, [(3, vec!["live".to_string()])]);
}

/// #1066: started by hand with a caller's COCKPIT_LOG kept, this binary's rig
/// refuses before a lookup can write a line there. The runner unsets it.
#[test]
fn a_kept_cockpit_log_is_refused() {
    let kept = [("COCKPIT_LOG", "/canary-1066/log")];
    let (code, said) = fake::run_by_hand("the_hook_options_reach_the_pane_through_the_format", &kept);
    let refused = said.contains("the caller's state refused (#1066): COCKPIT_LOG set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}

/// #1066: started by hand with a caller's XDG_CACHE_HOME kept, this binary's rig
/// refuses before a lookup can read or write the agents cache there.
#[test]
fn a_kept_agents_cache_is_refused() {
    let kept = [("XDG_CACHE_HOME", "/canary-1066/cache")];
    let (code, said) = fake::run_by_hand("the_hook_options_reach_the_pane_through_the_format", &kept);
    let refused = said.contains("the caller's state refused (#1066): XDG_CACHE_HOME set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}

/// #1066: started by hand with a caller's XDG_STATE_HOME kept, this binary's rig
/// refuses before it can read the hooks' subagent log there.
#[test]
fn a_kept_state_folder_is_refused() {
    let kept = [("XDG_STATE_HOME", "/canary-1066/state")];
    let (code, said) = fake::run_by_hand("the_hook_options_reach_the_pane_through_the_format", &kept);
    let refused = said.contains("the caller's state refused (#1066): XDG_STATE_HOME set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}

/// #1061, #1066: this binary's rig row, started again with TERM and HUP ignored and blocked, as a
/// suite under nohup or with them blocked hands them down: the rig's server, told to
/// go by a TERM it sends itself, is gone afterwards with its panes.
#[test]
#[cfg(target_os = "linux")]
fn the_rig_rows_leave_no_server_handed_a_blocked_term_and_hup() {
    let rows = ["the_hook_options_reach_the_pane_through_the_format"];
    let pid = fake::rows_pass_handed(&rows, &[libc::SIGTERM, libc::SIGHUP]);
    fake::assert_no_server_left(pid);
}
