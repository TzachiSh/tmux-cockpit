//! is_claude's is_agent half (review-534): with an agent list that leaves
//! `claude` out, a pane named `claude` is no agent's, so no @ refs. A test
//! binary of its own, as data reads the agent list once per thread.
mod fake;

use cockpit_tui::basket;
use std::process::Command;

struct Server {
    sock: String,
    dir: std::path::PathBuf,
    /// killed with its socket after the folder goes
    _server: fake::Server,
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
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_claude_the_agent_list_leaves_out_is_no_claude() {
    let sock = format!("cockpit-claude-glob-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    std::fs::create_dir_all(&dir).unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir,
    };
    // bash, whatever $SHELL is (#882): tmux runs a pane's command in the default
    // shell. Under dash, which forks the command and stays the pane's process, the
    // `claude` row would pass for the wrong reason. bash execs it.
    // Set as the server starts.
    // SHELL: dash if there, else /bin/sh, so CI, whose SHELL is bash, exercises the
    // default-shell line (#896). Where /bin/sh execs the command (a host with no
    // /usr/bin/dash, such as macOS), the test passes and proves nothing about that line.
    let dash = std::path::Path::new("/usr/bin/dash").exists();
    let ok = s._server.start(|c| {
        c.env("SHELL", if dash { "/usr/bin/dash" } else { "/bin/sh" })
            .args(["set-option", "-g", "default-shell", "/bin/bash", ";"])
            .args(["new-session", "-d", "-c"])
            .arg(&s.dir)
            .arg("sleep 300");
    });
    if !ok {
        eprintln!("SKIP: no tmux");
        return;
    }
    s.tmux(&["set-option", "-g", "@cockpit_agents_glob", "|codex|"]);
    // SAFETY: the only test in this binary; nothing else reads the env meanwhile.
    unsafe { std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", s.sock)) };
    // Each pane starts in its own row's turn, so a held row's hold is not spent
    // waiting on the rows before it, whatever their order (#1056).
    type Start<'a> = &'a dyn Fn() -> String;
    let held = || fake::held_pane(&s.sock, &s.dir, "2.1.318");
    let named = || fake::named_pane(&s.sock, &s.dir, "2.1.318");
    let rows: [(&str, Start, &str, bool); 4] = [
        ("claude, not in the agent list", &|| s.pane("claude"), "claude", false),
        (
            "the version title, agent-shaped all the same",
            &|| s.pane("2.1.318"),
            "2.1.318",
            true,
        ),
        ("the version title, its start held before the exec", &held, "2.1.318", true),
        ("the version title, comm ahead of its argv", &named, "2.1.318", true),
    ];
    for (row, start, name, want) in rows {
        let pane = start();
        fake::settle(&s.sock, &pane, name);
        assert_eq!(basket::is_claude(&pane), want, "{row} ({pane})");
    }
    assert!(!basket::is_claude("%99999"), "a pane that is not there");
}
