//! #1353: the panel's capture carries the board's facts on the pane's agent, off the one
//! read the work folder already takes, on a real tmux, a private `-L` server, never the
//! user's. One test, as it sets COCKPIT_TMUX.
mod fake;

use cockpit_tui::data::AgentFacts;
use cockpit_tui::panel::capture::{Capture, capture};
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

/// The session's transcript: a prompt the user typed, then a model call that runs a tool.
fn transcript() -> String {
    let typed = serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:05.000Z",
        "message": {"content": "fix the tests"}});
    let tool = serde_json::json!({"type": "assistant", "timestamp": "2026-08-27T00:00:06.000Z",
        "message": {"model": "claude-opus-4-5", "stop_reason": "tool_use",
        "content": [{"type": "tool_use", "id": "b1", "name": "Bash", "input": {"command": "cargo test"}}],
        "usage": {"input_tokens": 1, "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0, "output_tokens": 1}}});
    format!("{typed}\n{tool}\n")
}

/// Rows: an agent pane whose hooks set its state and whose transcript names its doing,
/// asked and model, which carries all four as the board reads them; a shell pane the
/// board does not count, which carries none. Each pane again through `Capture::refresh`
/// (#1352) of a capture holding stale facts: the refreshed capture carries what a fresh
/// one would.
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
    let conf = r.dir.join("conf");
    let tr = conf.join("projects/-rig/s1.jsonl");
    std::fs::create_dir_all(tr.parent().unwrap()).unwrap();
    std::fs::write(&tr, transcript()).unwrap();
    {
        let held = fake::env_lock();
        // SAFETY: the one test of this binary, so no other thread reads the env.
        unsafe {
            fake::set_var(&held, "COCKPIT_TMUX", format!("tmux -L {}", r.sock));
            fake::set_var(&held, "CLAUDE_CONFIG_DIR", conf.display().to_string());
        }
    }
    let agent = r.pane();
    r.tmux(&["set-option", "-p", "-t", &agent, "@panestate", "needs-input"]);
    r.tmux(&["set-option", "-p", "-t", &agent, "@cockpit_transcript", &tr.display().to_string()]);
    let shell = r.pane();
    let facts = AgentFacts {
        state: "needs-input".into(),
        doing: "Bash: cargo test".into(),
        asked: "fix the tests".into(),
        model: "opus".into(),
    };
    let rows = [("an agent pane", &agent, Some(facts)), ("a shell pane", &shell, None)];
    for (name, id, want) in rows {
        let c = capture(id).unwrap_or_else(|| panic!("{name}: no capture"));
        assert_eq!(c.agent, want, "{name}");
        let mut old = Capture {
            agent: Some(AgentFacts {
                state: "stale".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        old.refresh(id);
        assert_eq!(old.agent, want, "{name}, refreshed");
    }
}
