//! is_claude's is_agent half (review-534): with an agent list that leaves
//! `claude` out, a pane named `claude` is no agent's, so no @ refs. A test
//! binary of its own, as data reads the agent list once per thread.
use cockpit_tui::basket;
use std::os::unix::fs::PermissionsExt;
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

    /// As [`Server::pane`], but the pane's process holds 0.3 s before it execs
    /// `name` (#1056): the start-up window held open, so a read that does not
    /// wait for [`Server::settle`] finds the `held` script, not `name`.
    fn held(&self, name: &str) -> String {
        let bin = self.dir.join(name);
        // a copy a pane already runs cannot be written over (ETXTBSY)
        if !bin.exists() {
            std::fs::copy("/bin/bash", &bin).unwrap();
        }
        let hold = self.dir.join("held");
        std::fs::write(&hold, "#!/bin/sh\nsleep 0.3\nexec \"$@\"\n").unwrap();
        std::fs::set_permissions(&hold, std::fs::Permissions::from_mode(0o755)).unwrap();
        let cmd = format!("{} {} -c 'sleep 300; :'", hold.display(), bin.display());
        let d = self.dir.display().to_string();
        self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd])
    }

    /// Waits until the pane's own process has exec'd `name` (#1056). Not by
    /// pane_current_command: while a pane starts, tmux 3.8 reads it as the pane's
    /// argv, then `tmux`, then the shell, then the name, so its first read can
    /// already be `name`. The pane's pid keeps through each exec, and its comm
    /// only moves forward. Red, naming the comm read last, when it never gets there.
    /// The kernel sets comm before the new argv, and an exec's empty cmdline sends
    /// tmux back to the pane's start argv, so the argv tmux reads must agree too.
    fn settle(&self, pane: &str, name: &str) {
        let pid = self.tmux(&["display-message", "-p", "-t", pane, "#{pane_pid}"]);
        let mut last = String::new();
        for _ in 0..500 {
            last = comm(&pid);
            if last == name {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("pane {pane} never ran {name}: its process {pid} read {last:?} last");
    }
}

/// A process's name: /proc's comm, with the argv[0] tmux reads (/proc's cmdline)
/// added when the two differ; else `ps` where there is no /proc (macOS prints a path).
fn comm(pid: &str) -> String {
    if std::path::Path::new("/proc/self").is_dir() {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let argv = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let argv0 = argv.split(|b| *b == 0).next().unwrap_or_default();
        let argv0 = String::from_utf8_lossy(argv0).to_string();
        let base = argv0.rsplit('/').next().unwrap_or_default();
        let comm = comm.trim_end();
        return if base == comm {
            comm.to_string()
        } else {
            format!("{comm} (argv[0] {argv0:?})")
        };
    }
    let out = Command::new("ps").args(["-o", "comm=", "-p", pid]).output();
    let c = out
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    c.rsplit('/').next().unwrap_or_default().to_string()
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
fn a_claude_the_agent_list_leaves_out_is_no_claude() {
    let sock = format!("cockpit-claude-glob-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    std::fs::create_dir_all(&dir).unwrap();
    let mut new = Command::new("tmux");
    // bash, whatever $SHELL is (#882): tmux runs a pane's command in the default
    // shell. Under dash, which forks the command and stays the pane's process, the
    // `claude` row would pass for the wrong reason. bash execs it.
    // Set as the server starts.
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
    s.tmux(&["set-option", "-g", "@cockpit_agents_glob", "|codex|"]);
    // SAFETY: the only test in this binary; nothing else reads the env meanwhile.
    unsafe { std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", s.sock)) };
    let rows = [
        ("claude, not in the agent list", s.pane("claude"), "claude", false),
        ("the version title, agent-shaped all the same", s.pane("2.1.318"), "2.1.318", true),
        (
            "the version title, its start held before the exec",
            s.held("2.1.318"),
            "2.1.318",
            true,
        ),
    ];
    for (row, pane, name, want) in rows {
        s.settle(&pane, name);
        assert_eq!(basket::is_claude(&pane), want, "{row} ({pane})");
    }
    assert!(!basket::is_claude("%99999"), "a pane that is not there");
}
