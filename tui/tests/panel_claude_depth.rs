//! #1359: the panel reads 200 lines of a pane only when it runs Claude Code by the shared
//! rule (`agentid::is_claude`): the board's facts on its agent and a Claude-shaped command.
//! A claude with no facts, and a window named claude, get the 80 lines any pane gets. On
//! a real tmux, a private `-L` server, never the user's. One test, as it sets COCKPIT_TMUX.
mod fake;

use cockpit_tui::panel::capture::capture;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

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

    /// A window named `name` that prints the lines 1 to 150, then runs as `argv0`: bash
    /// under that name, which tmux reads as the pane's command. With `agent`, the hooks'
    /// @panestate, which makes it an agent's to the board.
    fn pane(&self, name: &str, argv0: &str, agent: bool) -> String {
        let d = self.dir.display().to_string();
        let run = format!("exec bash -c 'seq 1 150; exec -a {argv0} bash -c \"sleep 300; :\"'");
        let id = self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-n", name, "-c", &d, &run]);
        if agent {
            self.tmux(&["set-option", "-p", "-t", &id, "@panestate", "running"]);
        }
        // polled: until its last exec, the window's command is the bash that prints
        for _ in 0..500 {
            if self.tmux(&["display", "-p", "-t", &id, "#{pane_current_command}"]) == argv0 {
                return id;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{id} never ran as {argv0}");
    }
}

/// Rows: a claude the board counts, read from line 1; the same claude with no facts, and a
/// window named claude whose agent runs another command, both from 80 lines up.
#[test]
fn only_claude_code_by_the_shared_rule_is_captured_deeper() {
    let sock = format!("cockpit-capdepth-{}", std::process::id());
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
    let conf = r.dir.join("conf");
    std::fs::create_dir_all(&conf).unwrap();
    {
        let held = fake::env_lock();
        // SAFETY: the one test of this binary, so no other thread reads the env.
        unsafe {
            fake::set_var(&held, "COCKPIT_TMUX", format!("tmux -L {}", r.sock));
            fake::set_var(&held, "CLAUDE_CONFIG_DIR", conf.display().to_string());
        }
    }
    let rows = [
        ("claude, an agent", r.pane("work", "claude", true), true, true),
        ("claude, no board facts", r.pane("work", "claude", false), false, false),
        (
            "a window named claude, an agent running another command",
            r.pane("claude", "sleepy", true),
            true,
            false,
        ),
    ];
    let mut bad = Vec::new();
    for (row, id, agent, deep) in rows {
        let c = capture(&id).unwrap_or_else(|| panic!("{row}: no capture"));
        let first = c.screen.lines().next().unwrap_or_default().to_string();
        if (c.agent.is_some(), first == "1") != (agent, deep) {
            bad.push(format!(
                "{row}: facts {}, first line {first:?}, want facts {agent}, deep {deep}",
                c.agent.is_some()
            ));
        }
    }
    assert!(bad.is_empty(), "{} of 3 rows wrong:\n{}", bad.len(), bad.join("\n"));
}
