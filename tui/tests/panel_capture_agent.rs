//! #1353: the panel's capture carries the board's facts on the pane's agent, off the one
//! read the work folder already takes, on a real tmux, a private `-L` server, never the
//! user's. One test, as it sets COCKPIT_TMUX.
mod fake;

use cockpit_tui::panel::capture::capture;
use std::path::PathBuf;
use std::process::Command;

struct Rig {
    sock: String,
    dir: PathBuf,
    /// killed with its socket before the folder goes
    server: fake::Server,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.server.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Rig {
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    fn pane(&self) -> String {
        let d = self.dir.display().to_string();
        self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, "sleep 300"])
    }
}

/// Rows: an agent pane whose hooks set its state, which carries the board's state; a
/// shell pane the board does not count, which carries none.
#[test]
fn the_capture_carries_the_boards_agent_facts() {
    let sock = format!("cockpit-capagent-{}", std::process::id());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(&sock);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut r = Rig {
        server: fake::Server::new(&sock),
        sock,
        dir: dir.canonicalize().unwrap(),
    };
    let first = r.dir.display().to_string();
    if !r.server.start(|c| {
        c.args(["new-session", "-d", "-c", &first, "sleep 300"]);
    }) {
        eprintln!("SKIP: no tmux");
        return;
    }
    // SAFETY: the one test of this binary, so no other thread reads the env.
    unsafe { fake::set_var(&fake::env_lock(), "COCKPIT_TMUX", format!("tmux -L {}", r.sock)) };
    let agent = r.pane();
    r.tmux(&["set-option", "-p", "-t", &agent, "@panestate", "needs-input"]);
    let shell = r.pane();
    let rows = [("an agent pane", &agent, Some("needs-input")), ("a shell pane", &shell, None)];
    for (name, id, want) in rows {
        let c = capture(id).unwrap_or_else(|| panic!("{name}: no capture"));
        assert_eq!(c.agent.as_ref().map(|a| a.state.as_str()), want, "{name}");
    }
}
