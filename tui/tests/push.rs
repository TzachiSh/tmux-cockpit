//! The push channel's receiving half, against a real tmux server on a private
//! `-L` socket (AGENTS.md: never the user's).
//!
//! A board waits in a `tmux wait-for` child on a channel of its own. That
//! child must go with the board on every exit path -- `q`, SIGTERM and SIGHUP
//! from a torn-down popup, and SIGKILL -- or it lingers on the server.
use cockpit_tui::push::{OPTION, channel, entry, register_args};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Polled, never slept for: each assertion waits for the thing it is about to
/// read and gives up after 5 s.
fn until(mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(5) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// `cargo test` builds examples too, and drops them one directory up from the
/// test binary's own `deps/`. (`cargo test --test push` does NOT rebuild them:
/// mutate the library and run the whole suite, or you grade a stale example.)
fn example(name: &str) -> PathBuf {
    let mut p = std::env::current_exe().expect("test exe");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("examples").join(name)
}

/// The `tmux wait-for <chan>` processes alive, read from /proc. Linux only, as
/// are the tests that count them.
fn waiters(chan: &str) -> usize {
    let want = format!("wait-for\0{chan}\0");
    std::fs::read_dir("/proc")
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
                .filter(|e| std::fs::read(e.path().join("cmdline")).is_ok_and(|c| String::from_utf8_lossy(&c).contains(&want)))
                .count()
        })
        .unwrap_or(0)
}

/// Everything this test must take down even when an assertion panics. A leaked
/// child holds the harness's captured stderr open and `cargo test` then waits
/// on that pipe forever -- the failing run hangs instead of reporting.
struct Rig {
    kids: Vec<Child>,
    sock: String,
    dir: PathBuf,
}

impl Rig {
    fn tmux(&self, args: &[&str]) -> String {
        let mut a = vec!["-L", &self.sock];
        a.extend_from_slice(args);
        String::from_utf8_lossy(
            &Command::new("tmux")
                .args(&a)
                .env_remove("TMUX")
                .env_remove("TMUX_PANE")
                .output()
                .map(|o| o.stdout)
                .unwrap_or_default(),
        )
        .trim_end_matches('\n')
        .to_string()
    }
    fn listed(&self) -> String {
        self.tmux(&["show-option", "-gqv", OPTION])
    }
    /// What `scripts/agent-state` runs: every listed board, in one command.
    fn wake(&self) {
        self.tmux(&["run-shell", "-C", &format!("#{{{OPTION}}}")]);
    }
    /// A `push_hold` child listing itself on this server; its index and log.
    fn hold(&mut self) -> (usize, PathBuf) {
        let log = self.dir.join(format!("tui{}.log", self.kids.len()));
        self.kids.push(
            Command::new(example("push_hold"))
                .env("COCKPIT_TMUX", format!("tmux -L {}", self.sock))
                .env("COCKPIT_LOG", &log)
                .env_remove("COCKPIT_READONLY")
                .env_remove("TMUX")
                .env_remove("TMUX_PANE")
                // the child hands the terminal back on SIGTERM; that escape
                // sequence is not this test's output
                .stdout(Stdio::null())
                .spawn()
                .expect("run the push_hold example"),
        );
        (self.kids.len() - 1, log)
    }
    fn chan(&self, i: usize) -> String {
        channel(self.kids[i].id())
    }
    fn signal(&self, i: usize, sig: libc::c_int) {
        unsafe { libc::kill(self.kids[i].id() as i32, sig) };
    }
    fn exited(&mut self, i: usize) -> bool {
        until(|| self.kids[i].try_wait().ok().flatten().is_some())
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        for k in &mut self.kids {
            let _ = k.kill();
            let _ = k.wait();
        }
        self.tmux(&["kill-server"]);
        // kill-server never unlinks the socket, by any exit path; the suite
        // learned that after ~850 dead inodes piled up (tests/run.sh:59-63).
        let uid = unsafe { libc::getuid() };
        let tmp = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let _ = std::fs::remove_file(PathBuf::from(tmp).join(format!("tmux-{uid}")).join(&self.sock));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A private server and no holder yet; `None` without tmux.
fn rig(tag: &str) -> Option<Rig> {
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("no tmux on PATH -- skipping");
        return None;
    }
    let sock = format!("cockpit-tui-{tag}-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The server first: Push::start() lists its channel the moment it runs.
    let _ = Command::new("tmux")
        .args(["-L", &sock, "-f", "/dev/null", "new-session", "-d", "-c"])
        .arg(&dir)
        .args(["-s", "t", "sleep 300"])
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .output();
    Some(Rig {
        kids: Vec::new(),
        sock,
        dir,
    })
}

fn woke(log: &PathBuf, n: usize) -> bool {
    until(|| std::fs::read_to_string(log).unwrap_or_default().matches("push wake").count() >= n)
}

#[test]
#[cfg(target_os = "linux")]
fn a_wake_reaches_the_loop_and_sigterm_takes_the_waiter_along() {
    let Some(mut rig) = rig("push") else { return };
    let (a, log) = rig.hold();
    let ch = rig.chan(a);
    assert!(
        until(|| rig.listed() == entry(&ch)),
        "the channel was never listed: {:?}",
        rig.listed()
    );
    assert!(until(|| waiters(&ch) == 1), "no waiter on {ch}");

    rig.wake();
    assert!(woke(&log, 1), "the wake never reached the loop");
    // and it waits again: a second wake is a second refresh
    assert!(until(|| waiters(&ch) == 1), "the waiter was not re-armed");
    rig.wake();
    assert!(woke(&log, 2), "the re-armed waiter never woke");

    rig.signal(a, libc::SIGTERM);
    assert!(rig.exited(a), "SIGTERM did not end the holder");
    assert!(until(|| waiters(&ch) == 0), "SIGTERM left the waiter behind");
}

/// SIGKILL runs nothing of ours: PDEATHSIG takes the waiter, and the entry it
/// leaves is dropped by the next board to start, which then wakes as usual.
#[test]
#[cfg(target_os = "linux")]
fn a_sigkilled_board_leaves_no_waiter_and_the_next_one_drops_its_entry() {
    let Some(mut rig) = rig("pushk") else { return };
    let (a, _) = rig.hold();
    let dead = rig.chan(a);
    assert!(until(|| waiters(&dead) == 1), "no waiter on {dead}");
    rig.signal(a, libc::SIGKILL);
    assert!(rig.exited(a), "SIGKILL did not end the holder");
    assert!(until(|| waiters(&dead) == 0), "SIGKILL left the waiter behind");
    assert_eq!(rig.listed(), entry(&dead), "the killed board's entry should still be there");

    let (b, log) = rig.hold();
    let ch = rig.chan(b);
    assert!(
        until(|| rig.listed() == entry(&ch)),
        "the dead entry was not dropped: {:?}",
        rig.listed()
    );
    assert!(until(|| waiters(&ch) == 1), "no waiter on {ch}");
    rig.wake();
    assert!(woke(&log, 1), "the next board never woke");
}

#[test]
#[cfg(target_os = "linux")]
fn one_wake_reaches_two_boards() {
    let Some(mut rig) = rig("push2") else { return };
    let (a, la) = rig.hold();
    let ca = rig.chan(a);
    assert!(until(|| waiters(&ca) == 1), "no waiter on {ca}");
    let (b, lb) = rig.hold();
    let cb = rig.chan(b);
    assert!(until(|| waiters(&cb) == 1), "no waiter on {cb}");
    assert_eq!(
        rig.listed(),
        format!("{}{}", entry(&ca), entry(&cb)),
        "the second start dropped a live board"
    );
    rig.wake();
    assert!(woke(&la, 1), "the first board never woke");
    assert!(woke(&lb, 1), "the second board never woke");
}

/// The `q` half, in-process: `Drop` runs `cleanup()`, not the signal arm. It
/// unlists only its own entry and frees its channel: nothing latched remains.
#[test]
#[cfg(target_os = "linux")]
fn q_unlists_its_own_channel_and_only_its_own() {
    let Some(rig) = rig("push3") else { return };
    // SAFETY: the only test in this binary that reads COCKPIT_* in-process.
    unsafe {
        std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", rig.sock));
        std::env::remove_var("COCKPIT_READONLY");
    }
    let me = channel(std::process::id());
    let p = cockpit_tui::push::Push::start();
    assert_eq!(rig.listed(), entry(&me));
    assert!(until(|| waiters(&me) == 1), "no waiter on {me}");
    // another board's entry, listed after this one started
    let other = entry("cockpit-board-999999");
    rig.tmux(&["set-option", "-ga", OPTION, &other]);
    drop(p);
    assert_eq!(rig.listed(), other, "q took the wrong entry");
    assert!(until(|| waiters(&me) == 0), "q left the waiter behind");
    // Not latched: a wait on the channel blocks rather than returning at once.
    let mut probe = Command::new("tmux")
        .args(["-L", &rig.sock, "wait-for", &me])
        .env_remove("TMUX")
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let latched = probe.try_wait().unwrap().is_some();
    let _ = probe.kill();
    let _ = probe.wait();
    assert!(!latched, "q left {me} latched on the server");
}

#[test]
fn a_start_drops_dead_entries_and_keeps_its_own_and_live_ones() {
    // pid 1 is never a cockpit-tui; ours is listed already on a restart
    let me = channel(std::process::id());
    let listed = format!("{}{}", entry("cockpit-board-1"), entry(&me));
    let a = register_args(listed.trim_end(), &me);
    assert_eq!(
        a,
        [
            "set-option",
            "-gF",
            OPTION,
            "#{s/wait-for -S cockpit-board-1 . //:@cockpit_board_wake}",
            ";",
            "wait-for",
            "-S",
            "cockpit-board-1",
            ";",
            "set-option",
            "-ga",
            OPTION,
            &entry(&me),
        ]
    );
    // an empty option, and words that are not entries, cost nothing
    assert_eq!(register_args("", &me), ["set-option", "-ga", OPTION, &entry(&me)]);
    assert_eq!(register_args("junk ; wait-for -S other", &me).len(), 4);
}

/// #295: a cockpit-tui ignores SIGUSR1, so an agent-state from before #224 that
/// signals a recycled @cockpit_tui_pid cannot kill it. `info` is held inside
/// its tmux read (a stub that waits for `go`), signalled there, then let go.
#[test]
fn a_cockpit_tui_survives_sigusr1() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;
    let dir = std::env::temp_dir().join(format!("cockpit-tui-usr1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stub = dir.join("tmux");
    let d = dir.display();
    std::fs::write(
        &stub,
        format!("#!/bin/sh\n: > {d}/ready\nwhile [ ! -e {d}/go ]; do sleep 0.05; done\nexit 1\n"),
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["info", "--text", "%1"])
        .env("COCKPIT_TMUX", &stub)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let ready = until(|| dir.join("ready").exists());
    // our own unreaped child: the pid cannot have been recycled
    unsafe { libc::kill(child.id() as i32, libc::SIGUSR1) };
    std::fs::write(dir.join("go"), "").unwrap();
    let st = child.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ready, "info never reached its tmux read");
    assert_eq!(st.signal(), None, "SIGUSR1 killed a cockpit-tui: {st:?}");
}
