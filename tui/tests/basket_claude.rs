//! Which pane reads Claude Code's `@path#La-b` (#365 child 5, D4): an agent's
//! pane (#519's is_agent) whose command is `claude` or its version title. On a
//! private `-L` server, never the user's; one test, as it sets COCKPIT_TMUX.
use cockpit_tui::basket;
use std::process::Command;

struct Server {
    sock: String,
    dir: std::path::PathBuf,
    path: String,
}

impl Server {
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// A pane running a copy of bash named `name`: tmux reports that name.
    fn pane(&self, name: &str) -> String {
        let bin = self.dir.join(name);
        std::fs::copy("/bin/bash", &bin).unwrap();
        let cmd = format!("{} -c 'sleep 300; :'", bin.display());
        let d = self.dir.display().to_string();
        self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd])
    }

    /// A pane running sleep as `argv0`, once tmux shows that name: a relative path
    /// (#840) cannot be a copied file's name, as tmux cuts only an absolute one.
    fn pane_as(&self, argv0: &str) -> String {
        let d = self.dir.display().to_string();
        let cmd = format!("exec -a {argv0} sleep 300");
        let p = self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd]);
        let shown = || self.tmux(&["display-message", "-p", "-t", &p, "#{pane_current_command}"]);
        // a poll: the shell execs sleep a moment after new-window returns
        for _ in 0..500 {
            if shown() == argv0 {
                return p;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("{p} never showed {argv0}: {}", shown());
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = Command::new("tmux").args(["-L", &self.sock, "kill-server"]).status();
        if self.path.ends_with(&self.sock) {
            let _ = std::fs::remove_file(&self.path);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn claude_and_its_version_title_read_at_mentions_and_nothing_else_does() {
    let sock = format!("cockpit-claude-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    std::fs::create_dir_all(&dir).unwrap();
    let mut new = Command::new("tmux");
    new.args(["-L", &sock, "-f", "/dev/null", "new-session", "-d", "-c"])
        .arg(&dir)
        .arg("sleep 300");
    let ok = new.status().is_ok_and(|s| s.success());
    if !ok {
        eprintln!("SKIP: no tmux");
        return;
    }
    let mut s = Server {
        sock,
        dir,
        path: String::new(),
    };
    s.path = s.tmux(&["display-message", "-p", "#{socket_path}"]);
    s.tmux(&["set-option", "-g", "@cockpit_agents_glob", "|claude|codex|aider|gemini|"]);
    // SAFETY: the only test in this binary; nothing else reads the env meanwhile.
    unsafe { std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", s.sock)) };
    let stale = s.pane("fish");
    // @panestate outlives the claude that set it: on a shell it is no claude
    s.tmux(&["set-option", "-p", "-t", &stale, "@panestate", "working"]);
    let rows = [
        ("claude", s.pane("claude"), true),
        ("its version title", s.pane("2.1.318"), true),
        // #840: tmux keeps a relative path, and no glob site calls ./claude an agent
        ("./claude", s.pane_as("./claude"), false),
        ("codex", s.pane("codex"), false),
        ("a shell", s.pane("zsh"), false),
        ("a stale @panestate on a shell", stale, false),
    ];
    for (row, pane, want) in rows {
        assert_eq!(basket::is_claude(&pane), want, "{row} ({pane})");
    }
    assert!(!basket::is_claude("%99999"), "a pane that is not there");
}
