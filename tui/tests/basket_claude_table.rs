//! basket::is_claude over #1359's table: a pane runs Claude Code when the board
//! calls it an agent's and its command is Claude-shaped. The window name is no
//! evidence. On a private `-L` server, never the user's. Its own test binary: data
//! reads the agent list once per thread, so each list is read on a thread of its own.
mod fake;

use cockpit_tui::basket;
use cockpit_tui::data::agentid;
use std::process::Command;

struct Server {
    sock: String,
    dir: std::path::PathBuf,
    /// killed with its socket before the folder goes
    _server: fake::Server,
}

impl Server {
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// A window named `win` whose pane runs a copy of bash named `name`, marked with
    /// @panestate when `marked`: tmux reports `name` as its command.
    fn pane(&self, win: &str, name: &str, marked: bool) -> String {
        let bin = self.dir.join(name);
        if !bin.exists() {
            std::fs::copy("/bin/bash", &bin).unwrap();
        }
        let cmd = format!("{} -c 'sleep 300; :'", bin.display());
        let d = self.dir.display().to_string();
        let new = ["new-window", "-d", "-n", win, "-P", "-F", "#{pane_id}", "-c", &d, &cmd];
        let pane = self.tmux(&new);
        let argv = fake::started(&bin);
        fake::settle(&self.sock, &pane, name, &argv.iter().map(String::as_str).collect::<Vec<_>>());
        if marked {
            self.tmux(&["set-option", "-p", "-t", &pane, "@panestate", "working"]);
        }
        pane
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self._server.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A row of the table: what it is, the window name, the command, whether the hooks
/// marked the pane, and the answer under the rule.
type Row = (&'static str, &'static str, &'static str, bool, bool);

/// With claude on the agent list (the plugin's default).
const LISTED: [Row; 9] = [
    ("1 claude", "w1", "claude", false, true),
    ("2 its version title", "w2", "2.1.318", false, true),
    ("3 claude in upper case", "w3", "CLAUDE", false, true),
    ("4 a shell in a window named claude", "claude", "zsh", false, false),
    ("5 a stale @panestate on a shell, window claude", "claude", "fish", true, false),
    ("6 node in a window named claude, marked", "claude", "node", true, false),
    ("7 node, marked", "w7", "node", true, false),
    ("11 another agent", "w11", "codex", false, false),
    ("13 a wrapper: bash, marked", "w13", "bash", true, false),
];

/// With an agent list that leaves claude out, and with none (the plugin not loaded).
const UNLISTED: [Row; 2] = [
    ("8/9 claude off the list, unmarked", "w8", "claude", false, false),
    ("10 claude off the list, marked", "w10", "claude", true, true),
];

/// Each row's pane under the agent list `glob`, read on a thread of its own.
fn rows_under(s: &Server, glob: Option<&str>, rows: &[Row]) {
    match glob {
        Some(g) => s.tmux(&["set-option", "-g", "@cockpit_agents_glob", g]),
        None => s.tmux(&["set-option", "-gu", "@cockpit_agents_glob"]),
    };
    let panes: Vec<String> = rows.iter().map(|r| s.pane(r.1, r.2, r.3)).collect();
    std::thread::scope(|t| {
        t.spawn(|| {
            for (r, pane) in rows.iter().zip(&panes) {
                assert_eq!(basket::is_claude(pane), r.4, "{} under {glob:?} ({pane})", r.0);
                // the board's agent verdict and the command, through the shared rule
                let agent = cockpit_tui::data::is_agent(pane);
                assert_eq!(agentid::is_claude(r.2, agent), r.4, "{}: the rule ({pane})", r.0);
            }
        });
    });
}

#[test]
fn the_basket_answers_as_the_table_does() {
    let sock = format!("cockpit-claude-table-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    std::fs::create_dir_all(&dir).unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir,
    };
    // bash execs a pane's command, so the pane's process is the copy (#882, #896)
    let ok = s._server.start(|c| {
        c.args(["set-option", "-g", "default-shell", "/bin/bash", ";"])
            .args(["new-session", "-d", "-c"])
            .arg(&s.dir)
            .arg("sleep 300");
    });
    if !ok {
        eprintln!("SKIP: no tmux");
        return;
    }
    // SAFETY: the one test of this binary, and it writes the env before any spawn
    unsafe { fake::set_var(&fake::env_lock(), "COCKPIT_TMUX", format!("tmux -L {}", s.sock)) };
    rows_under(&s, Some("|claude|codex|aider|gemini|"), &LISTED);
    rows_under(&s, Some("|codex|"), &UNLISTED);
    rows_under(&s, None, &UNLISTED);
}
