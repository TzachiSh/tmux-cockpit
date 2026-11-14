//! Which pane reads Claude Code's `@path#La-b` (#365 child 5, D4): an agent's
//! pane (#519's is_agent) whose command is `claude` or its version title. On a
//! private `-L` server, never the user's; one test, as it sets COCKPIT_TMUX.
mod fake;

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

    /// A pane as the board's resume key starts claude under a dash default shell
    /// (#895): `dash -c <string>`, the string with or without `exec`. bash, this
    /// server's shell, execs dash; dash forks the last command unless told to exec.
    fn resumed(&self, dash: &str, exec: bool) -> String {
        let bin = self.dir.join("claude");
        let pre = if exec { "exec " } else { "" };
        let cmd = format!("{dash} -c \"{pre}{} -c 'sleep 300; :'\"", bin.display());
        let d = self.dir.display().to_string();
        self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd])
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
    // bash, whatever $SHELL is (#882): tmux runs a pane's command in the default
    // shell. dash forks the command and stays the pane's process, so the pane is
    // never `claude`; bash execs it.
    // Set in the command that starts the server, so the first pane has it too.
    // SHELL: dash if there, else /bin/sh, so CI, whose SHELL is bash, exercises the
    // default-shell line (#896). Where /bin/sh execs the command (a host with no
    // /usr/bin/dash, such as macOS), the test passes and proves nothing about that line.
    let dash = std::path::Path::new("/usr/bin/dash").exists();
    new.env("SHELL", if dash { "/usr/bin/dash" } else { "/bin/sh" })
        .args(["-L", &sock, "-f", "/dev/null"])
        .args(["set-option", "-g", "default-shell", "/bin/bash", ";"])
        .args(["new-session", "-d", "-c"])
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
    // Each pane starts in its own row's turn, so a held row's hold is not spent
    // waiting on the rows before it, whatever their order (#1056).
    type Start<'a> = &'a dyn Fn() -> String;
    let held = || fake::held_pane(&s.sock, &s.dir, "2.1.318");
    let named = || fake::named_pane(&s.sock, &s.dir, "2.1.318");
    let rows: [(&str, Start, &str, bool); 7] = [
        ("claude", &|| s.pane("claude"), "claude", true),
        ("its version title", &|| s.pane("2.1.318"), "2.1.318", true),
        ("its version title, its start held before the exec", &held, "2.1.318", true),
        ("its version title, comm ahead of its argv", &named, "2.1.318", true),
        ("codex", &|| s.pane("codex"), "codex", false),
        ("a shell", &|| s.pane("zsh"), "zsh", false),
        ("a stale @panestate on a shell", &|| stale.clone(), "fish", false),
    ];
    for (row, start, name, want) in rows {
        let pane = start();
        fake::settle(&s.sock, &pane, name);
        assert_eq!(basket::is_claude(&pane), want, "{row} ({pane})");
    }
    // The resume key's string under dash (#895): with exec the pane is claude, as the
    // key's live-copy check reads it; without, it is the shell, and no claude to either.
    if dash {
        let cases = [("with exec", true, "claude", true), ("without exec", false, "dash", false)];
        for (row, exec, settled, want) in cases {
            let pane = s.resumed("/usr/bin/dash", exec);
            // bash, then dash, then claude exec'd or forked: wait for the pane's process
            fake::settle(&s.sock, &pane, settled);
            let cmd = s.tmux(&["display-message", "-p", "-t", &pane, "#{pane_current_command}"]);
            assert_eq!(cmd, settled, "a resume string {row}, under dash: the pane's command");
            assert_eq!(basket::is_claude(&pane), want, "a resume string {row}, under dash ({pane})");
        }
    }
    assert!(!basket::is_claude("%99999"), "a pane that is not there");
}
