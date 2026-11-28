//! Which pane reads Claude Code's `@path#La-b` (#365 child 5, D4): an agent's
//! pane (#519's is_agent) whose command is `claude` or its version title. On a
//! private `-L` server, never the user's; one test, as it sets COCKPIT_TMUX.
mod fake;

use cockpit_tui::basket;
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

    /// A pane running a copy of bash named `name`: tmux reports that name.
    fn pane(&self, name: &str) -> (String, Vec<String>) {
        let bin = self.dir.join(name);
        fake::copy_exec(std::path::Path::new("/bin/bash"), &bin);
        let cmd = format!("{} -c 'sleep 300; :'", bin.display());
        let d = self.dir.display().to_string();
        let pane = self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd]);
        (pane, fake::started(&bin))
    }

    /// A pane as the board's resume key starts claude under a dash default shell
    /// (#895): `dash -c <string>`, the string with or without `exec`. bash, this
    /// server's shell, execs dash; dash forks the last command unless told to exec.
    fn resumed(&self, dash: &str, exec: bool) -> (String, Vec<String>) {
        let bin = self.dir.join("claude");
        let pre = if exec { "exec " } else { "" };
        let cmd = format!("{dash} -c \"{pre}{} -c 'sleep 300; :'\"", bin.display());
        let d = self.dir.display().to_string();
        let pane = self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd]);
        // with exec, dash's argv gives way to claude's; without, dash keeps its own
        let argv = if exec {
            fake::started(&bin)
        } else {
            vec![dash.to_string(), "-c".into(), format!("{} -c 'sleep 300; :'", bin.display())]
        };
        (pane, argv)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self._server.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn claude_and_its_version_title_read_at_mentions_and_nothing_else_does() {
    let sock = format!("cockpit-claude-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    std::fs::create_dir_all(&dir).unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir,
    };
    // bash, whatever $SHELL is (#882): tmux runs a pane's command in the default
    // shell. dash forks the command and stays the pane's process, so the pane is
    // never `claude`; bash execs it.
    // Set in the command that starts the server, so the first pane has it too.
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
    s.tmux(&["set-option", "-g", "@cockpit_agents_glob", "|claude|codex|aider|gemini|"]);
    // SAFETY: the one test of this binary that writes the env; the others read it only
    // in their spawns, which std orders with this write (fake::rows_pass_handed).
    unsafe { fake::set_var(&fake::env_lock(), "COCKPIT_TMUX", format!("tmux -L {}", s.sock)) };
    let (stale, stale_argv) = s.pane("fish");
    // @panestate outlives the claude that set it: on a shell it is no claude
    s.tmux(&["set-option", "-p", "-t", &stale, "@panestate", "working"]);
    // Each pane starts in its own row's turn, so a held row's hold is not spent
    // waiting on the rows before it, whatever their order (#1056).
    type Start<'a> = &'a dyn Fn() -> (String, Vec<String>);
    let held = || fake::held_pane(&s.sock, &s.dir, "2.1.318");
    let named = || fake::named_pane(&s.sock, &s.dir, "2.1.318");
    let rows: [(&str, Start, &str, bool); 8] = [
        ("claude", &|| s.pane("claude"), "claude", true),
        ("its version title", &|| s.pane("2.1.318"), "2.1.318", true),
        ("its version title, its start held before the exec", &held, "2.1.318", true),
        ("its version title, comm ahead of its argv", &named, "2.1.318", true),
        ("codex", &|| s.pane("codex"), "codex", false),
        ("a shell", &|| s.pane("zsh"), "zsh", false),
        (
            "a name past comm's 15 bytes",
            &|| s.pane("claude-code-nightly"),
            "claude-code-nightly",
            false,
        ),
        (
            "a stale @panestate on a shell",
            &|| (stale.clone(), stale_argv.clone()),
            "fish",
            false,
        ),
    ];
    for (row, start, name, want) in rows {
        let (pane, argv) = start();
        fake::settle(&s.sock, &pane, name, &argv.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(basket::is_claude(&pane), want, "{row} ({pane})");
    }
    // The resume key's string under dash (#895): with exec the pane is claude, as the
    // key's live-copy check reads it; without, it is the shell, and no claude to either.
    if dash {
        let cases = [("with exec", true, "claude", true), ("without exec", false, "dash", false)];
        for (row, exec, settled, want) in cases {
            let (pane, argv) = s.resumed("/usr/bin/dash", exec);
            // bash, then dash, then claude exec'd or forked: wait for the pane's process
            fake::settle(&s.sock, &pane, settled, &argv.iter().map(String::as_str).collect::<Vec<_>>());
            let cmd = s.tmux(&["display-message", "-p", "-t", &pane, "#{pane_current_command}"]);
            assert_eq!(cmd, settled, "a resume string {row}, under dash: the pane's command");
            assert_eq!(basket::is_claude(&pane), want, "a resume string {row}, under dash ({pane})");
        }
    }
    assert!(!basket::is_claude("%99999"), "a pane that is not there");
}

/// #1184: tmux 3.8 runs a one-string start as `<default-shell> -c '<string>'`, so
/// under a bash default shell a pane wanted as bash reads comm bash and argv[0]
/// bash before the bash it execs exists. Held on a fifo in that -c stage, a
/// bounded settle is red; let go, settle ends on the exec'd bash, whose whole
/// argv is the one wanted. As tests/run.sh block 313 holds pane_runs (#1079).
#[test]
#[cfg(target_os = "linux")]
fn settle_does_not_end_on_the_default_shells_c_stage() {
    use std::os::unix::ffi::OsStrExt;
    let sock = format!("cockpit-settle-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    std::fs::create_dir_all(&dir).unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir,
    };
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
    let (fifo, held) = (s.dir.join("f"), s.dir.join("in"));
    let c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: mkfifo takes a path and a mode.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", fifo.display());
    let q = |p: &std::path::Path| format!("'{}'", p.display().to_string().replace('\'', r"'\''"));
    let (f, i) = (q(&fifo), q(&held));
    let start = format!("exec 9<>{f}; printf x > {i}; read -r -t 20 -u 9 _; exec bash --norc --noprofile");
    let d = s.dir.display().to_string();
    let pane = s.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &start]);
    // the -c shell has the fifo open and waits on it
    for _ in 0..2000 {
        if std::fs::read_to_string(&held).is_ok_and(|x| x == "x") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let at = std::fs::read_to_string(&held).unwrap_or_default();
    assert_eq!(at, "x", "pane {pane} never reached its hold in the -c shell");
    let want = ["bash", "--norc", "--noprofile"];
    let early = fake::settle_within(&s.sock, &pane, "bash", &want, 3);
    let e = early.expect_err("settle ended on the -c shell, whose comm and argv[0] are bash");
    // red for that shell, not for some other read
    assert!(
        e.contains(r#"read comm "bash", argv Some(["bash", "-c", "exec 9<>"#),
        "pane {pane}: {e}"
    );
    // read and write, so the open cannot wait for a reader
    let mut w = std::fs::OpenOptions::new().read(true).write(true).open(&fifo).unwrap();
    std::io::Write::write_all(&mut w, b"x\n").unwrap();
    assert_eq!(
        fake::settle_within(&s.sock, &pane, "bash", &want, 500),
        Ok(()),
        "and ends on the bash it execs"
    );
    let pid = s.tmux(&["display-message", "-p", "-t", &pane, "#{pane_pid}"]);
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let raw = String::from_utf8_lossy(&raw);
    let argv: Vec<&str> = raw.strip_suffix('\0').unwrap_or(&raw).split('\0').collect();
    assert_eq!(argv, want, "pane {pane}'s argv once settled");
}

/// #1339: a pane's copy of bash is written by a `cp` child, never by this process. A
/// fork on any other thread holds a copy of every fd this process has open until it
/// execs, a writer on the copy among them, and the pane's exec of the copy is then
/// ETXTBSY (#541). That race cannot be held, so the row reads its cause: this thread's
/// count of bytes written (wchar in /proc/thread-self/io) grows by less than the copy.
#[test]
#[cfg(target_os = "linux")]
fn a_panes_bash_copy_is_written_by_a_cp_child() {
    use std::os::unix::fs::PermissionsExt;
    let io = "/proc/thread-self/io";
    let wrote = || {
        let s = std::fs::read_to_string(io).unwrap_or_else(|e| panic!("{io}: {e}"));
        let Some(n) = s.lines().find_map(|l| l.strip_prefix("wchar:")) else {
            panic!("no wchar in {io}: {s:?}");
        };
        n.trim().parse::<u64>().unwrap()
    };
    let dir = fake::Scratch::fresh(std::env::temp_dir().join(format!("cockpit-copy-{}", std::process::id())));
    let (bash, bin) = (std::path::Path::new("/bin/bash"), dir.join("fish"));
    let before = wrote();
    fake::copy_exec(bash, &bin);
    let grew = wrote() - before;
    let len = std::fs::metadata(bash).unwrap().len();
    assert!(grew < len, "this thread wrote {grew} bytes while it made a copy of {len}");
    assert_eq!(std::fs::read(&bin).unwrap(), std::fs::read(bash).unwrap(), "the copy is bash");
    let mode = std::fs::metadata(&bin).unwrap().permissions().mode();
    assert_ne!(mode & 0o111, 0, "the copy runs: mode {mode:o}");
}

/// #1339: a pane whose process ended has no pane_pid, as tmux 3.8 gives none once the
/// pane's pty is closed (format.c, format_cb_pane_pid). settle says so at once, rather
/// than reading /proc//cmdline, which is the kernel's command line, for its whole wait.
/// remain-on-exit keeps the dead pane, so its pid reads empty every time.
#[test]
fn settle_names_a_pane_whose_process_ended() {
    let sock = format!("cockpit-ended-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    std::fs::create_dir_all(&dir).unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir,
    };
    let ok = s._server.start(|c| {
        c.args(["set-option", "-g", "remain-on-exit", "on", ";"])
            .args(["new-session", "-d", "-c"])
            .arg(&s.dir)
            .arg("sleep 300");
    });
    if !ok {
        eprintln!("SKIP: no tmux");
        return;
    }
    let d = s.dir.display().to_string();
    let pane = s.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, "exit 3"]);
    let dead = || s.tmux(&["display-message", "-p", "-t", &pane, "#{pane_dead}"]) == "1";
    for _ in 0..2000 {
        if dead() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(dead(), "pane {pane} never ended");
    let said = fake::settle_within(&s.sock, &pane, "bash", &[], 3);
    assert_eq!(said, Err(format!("pane {pane} has no process (it ended)")));
}

/// #1061, #1066: this binary's rig row, started again with TERM and HUP ignored and blocked, as a
/// suite under nohup or with them blocked hands them down: the rig's server, told to
/// go by a TERM it sends itself, is gone afterwards with its panes.
#[test]
#[cfg(target_os = "linux")]
fn the_rig_rows_leave_no_server_handed_a_blocked_term_and_hup() {
    let rows = [
        "claude_and_its_version_title_read_at_mentions_and_nothing_else_does",
        "settle_does_not_end_on_the_default_shells_c_stage",
    ];
    let pid = fake::rows_pass_handed(&rows, &[libc::SIGTERM, libc::SIGHUP]);
    fake::assert_no_server_left(pid);
}
