//! FIELDS against a real tmux server on a private socket (#313): the options the hooks write
//! come back as `Pane.subs_n` and `Pane.subs`. `subs_fields.rs` pins the parser on canned
//! rows; this pins the format string. Its own binary: `list_panes` reads the env.
use cockpit_tui::data::list_panes;
use std::path::PathBuf;
use std::process::Command;

struct Server {
    sock: String,
    dir: PathBuf,
}

impl Server {
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.tmux(&["kill-server"]);
        let tmp = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".into());
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let _ = std::fs::remove_file(PathBuf::from(tmp).join(format!("tmux-{uid}")).join(&self.sock));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn the_hook_options_reach_the_pane_through_the_format() {
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
    let s = Server {
        sock: format!("cockpit-subs-{pid}"),
        dir: dir.clone(),
    };
    let d = dir.display().to_string();
    s.tmux(&["-f", "/dev/null", "new-session", "-d", "-s", "t", "-c", &d, "exec sleep 60"]);
    let p = s.tmux(&["display-message", "-p", "-t", "t", "#{pane_id}"]);
    let t = t.display().to_string();
    for (k, v) in [("@panestate", "running"), ("@cockpit_subs", "3"), ("@cockpit_transcript", &t)] {
        s.tmux(&["set-option", "-p", "-t", &p, k, v]);
    }
    // SAFETY: this test binary holds exactly one test, so nothing runs beside it.
    unsafe {
        std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", s.sock));
        std::env::set_var("HOME", &d);
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::env::set_var("COCKPIT_READONLY", "1");
    }
    let ps = list_panes().unwrap();
    let got: Vec<(u32, Vec<String>)> = ps
        .iter()
        .map(|p| (p.subs_n, p.subs.iter().map(|s| s.desc.clone()).collect()))
        .collect();
    assert_eq!(got, [(3, vec!["live".to_string()])]);
}
