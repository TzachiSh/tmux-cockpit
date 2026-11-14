//! The push channel's receiving half, against a real tmux server on a private
//! `-L` socket (AGENTS.md: never the user's).
//!
//! A board waits in a `tmux wait-for` child on a channel of its own. That
//! child must go with the board on every exit path -- `q`, SIGTERM and SIGHUP
//! from a torn-down popup, and SIGKILL -- or it lingers on the server.
mod fake;

use cockpit_tui::push::{OPTION, channel, entry, register_args};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

/// Polled, never slept for: each assertion waits for the thing it is about to
/// read. The cap is a hang guard only a red run pays for (a true condition returns at once):
/// 5 s lost a process start to a box at load 28 (#373), as the shell suite's $POLLS cap did.
const CAP: Duration = Duration::from_secs(30);

fn until(mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < CAP {
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

/// Whether `pid` holds a unix socket: a `socket:[N]` link under /proc/<pid>/fd.
/// A probe that has exec'd `wait-for` but not yet connected holds none.
fn connected(pid: u32) -> bool {
    std::fs::read_dir(format!("/proc/{pid}/fd")).is_ok_and(|d| {
        d.flatten().any(|e| {
            let l = std::fs::read_link(e.path());
            l.is_ok_and(|l| l.to_string_lossy().starts_with("socket:["))
        })
    })
}

/// The probe has entered `wait-for <chan>` and connected, or has already exited.
fn probe_in(chan: &str, probe: &mut Child) -> bool {
    (waiters(chan) == 1 && connected(probe.id())) || probe.try_wait().unwrap().is_some()
}

/// The process environment: `q_unlists...` sets COCKPIT_TMUX in-process, and a
/// set_var racing a spawn that reads environ is undefined. That test holds it
/// for writing; every test that spawns holds it for reading.
static ENV: RwLock<()> = RwLock::new(());

enum EnvLock {
    #[allow(dead_code)]
    Read(RwLockReadGuard<'static, ()>),
    #[allow(dead_code)]
    Write(RwLockWriteGuard<'static, ()>),
}

fn env_read() -> EnvLock {
    EnvLock::Read(ENV.read().unwrap_or_else(|e| e.into_inner()))
}

/// Everything this test must take down even when an assertion panics. A leaked
/// child holds the harness's captured stderr open and `cargo test` then waits
/// on that pipe forever -- the failing run hangs instead of reporting.
struct Rig {
    kids: Vec<Child>,
    sock: String,
    dir: PathBuf,
    // last, so it is released after Drop has taken the rest down
    _env: EnvLock,
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
        let mut c = Command::new(example("push_hold"));
        // a row's SIGTERM reaches the board's handler however the suite was started (#1061)
        fake::at_default(&mut c, &[libc::SIGTERM]);
        self.kids.push(
            c.env("COCKPIT_TMUX", format!("tmux -L {}", self.sock))
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
        // learned that after ~850 dead inodes piled up (tests/run.sh, the note above
        // kill_socks).
        let uid = unsafe { libc::getuid() };
        let tmp = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let _ = std::fs::remove_file(PathBuf::from(tmp).join(format!("tmux-{uid}")).join(&self.sock));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A private server and no holder yet; `None` without tmux.
fn rig(tag: &str) -> Option<Rig> {
    rig_env(tag, env_read())
}

fn rig_env(tag: &str, env: EnvLock) -> Option<Rig> {
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("no tmux on PATH -- skipping");
        return None;
    }
    let sock = format!("cockpit-tui-{tag}-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The server first: Push::start() lists its channel the moment it runs. Its
    // kill-server is a TERM it sends itself, then a HUP to each pane, which starts
    // with the server's mask. So the server starts with both at their default and
    // unblocked: a suite started with them blocked left it and its pane running (#1061).
    let mut server = Command::new("tmux");
    fake::at_default(&mut server, &[libc::SIGTERM, libc::SIGHUP]);
    let _ = server
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
        _env: env,
    })
}

fn woke(log: &PathBuf, n: usize) -> bool {
    until(|| std::fs::read_to_string(log).unwrap_or_default().matches("push wake").count() >= n)
}

#[test]
#[cfg(target_os = "linux")]
fn a_wake_reaches_the_loop_and_sigterm_takes_the_waiter_along() {
    let Some(mut rig) = rig("push") else { return };
    // what a board from before #224 left standing; the start unsets it
    rig.tmux(&["set-option", "-g", "@cockpit_tui_pid", "999999"]);
    let (a, log) = rig.hold();
    let ch = rig.chan(a);
    assert!(
        until(|| rig.listed() == entry(&ch)),
        "the channel was never listed: {:?}",
        rig.listed()
    );
    assert!(until(|| waiters(&ch) == 1), "no waiter on {ch}");
    assert_eq!(
        rig.tmux(&["show-option", "-gqv", "@cockpit_tui_pid"]),
        "",
        "the old pid option survived"
    );

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
    // Planted: `+pid` parses as the first board's pid, and as an ERE `-+pid`
    // it would match the first board's real entry. The start must skip it.
    let planted = entry(&format!("cockpit-board-+{}", rig.kids[a].id()));
    rig.tmux(&["set-option", "-ga", OPTION, &planted]);
    let (b, lb) = rig.hold();
    let cb = rig.chan(b);
    assert!(until(|| waiters(&cb) == 1), "no waiter on {cb}");
    assert_eq!(
        rig.listed(),
        format!("{}{planted}{}", entry(&ca), entry(&cb)),
        "the second start dropped a live board, or touched the planted entry"
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
    let w = EnvLock::Write(ENV.write().unwrap_or_else(|e| e.into_inner()));
    let Some(rig) = rig_env("push3", w) else { return };
    // SAFETY: the write lock keeps every other test of this binary from
    // spawning, so nothing reads the environment while it changes.
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
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // A vacuous pass is a probe that has not reached the server yet. A connected
    // socket proves it has (stdio is null above, so it cannot be an inherited
    // one); the `display-message` round trip then covers the server running the
    // probe's `wait-for`.
    let entered = until(|| probe_in(&me, &mut probe));
    rig.tmux(&["display-message", "-p", "sync"]);
    let latched = probe.try_wait().unwrap().is_some();
    let _ = probe.kill();
    let _ = probe.wait();
    // SAFETY: as above, still under the write lock.
    unsafe { std::env::remove_var("COCKPIT_TMUX") };
    assert!(entered, "the probe never ran, so the wait was not tested");
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
            "#{s/wait-for -S cockpit-board-1 [^ ] //:@cockpit_board_wake}",
            ";",
            "wait-for",
            "-S",
            "cockpit-board-1",
            ";",
            "set-option",
            "-gu",
            "@cockpit_tui_pid",
            ";",
            "set-option",
            "-ga",
            OPTION,
            &entry(&me),
        ]
    );
    // an empty option, words that are not entries, and channels that are not
    // `cockpit-board-<digits>` exactly cost nothing past the unset and the append
    let base = [
        "set-option",
        "-gu",
        "@cockpit_tui_pid",
        ";",
        "set-option",
        "-ga",
        OPTION,
        &entry(&me),
    ];
    assert_eq!(register_args("", &me), base);
    assert_eq!(register_args("junk ; wait-for -S other", &me), base);
    for odd in ["+1", "", "1x", " 1", "-1", "1.0"] {
        let listed = entry(&format!("cockpit-board-{odd}"));
        assert_eq!(register_args(&listed, &me), base, "acted on cockpit-board-{odd:?}");
    }
}

/// #295: a cockpit-tui ignores SIGUSR1, so an agent-state from before #224 that
/// signals a recycled @cockpit_tui_pid cannot kill it. `info` is held inside
/// its tmux read (a stub that waits for `go`), signalled there, then let go.
#[test]
fn a_cockpit_tui_survives_sigusr1() {
    let _env = env_read();
    use std::os::unix::process::ExitStatusExt;
    let dir = std::env::temp_dir().join(format!("cockpit-tui-usr1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stub = dir.join("tmux");
    let d = dir.display();
    fake::write_exec(
        &stub,
        &format!("#!/bin/sh\n: > {d}/ready\nwhile [ ! -e {d}/go ]; do sleep 0.05; done\nexit 1\n"),
    );
    let mut c = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"));
    // USR1 at its default, so that only the board's own ignore keeps it alive (#1061)
    fake::at_default(&mut c, &[libc::SIGUSR1]);
    let mut child = c
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

/// The processes left of the `push` rig of the test binary `pid`, read from /proc: its
/// server, by the socket its command line names, and its pane, by the folder it runs
/// in. Linux only, as are the rows that count them.
fn left_by(pid: u32) -> usize {
    let sock = format!("-L\0cockpit-tui-push-{pid}\0");
    let dir = format!("/cockpit-tui-push-{pid}");
    let left = |e: &std::fs::DirEntry| {
        let line = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
        let cwd = std::fs::read_link(e.path().join("cwd")).unwrap_or_default();
        let cwd = cwd.to_string_lossy();
        let in_dir = cwd.ends_with(&dir) || cwd.ends_with(&format!("{dir} (deleted)"));
        String::from_utf8_lossy(&line).contains(&sock) || in_dir
    };
    std::fs::read_dir("/proc").map_or(0, |d| d.flatten().filter(left).count())
}

/// #1061: the two rows that signal a board, in this binary started again with TERM, HUP
/// and USR1 ignored and blocked. Each holds there by its own reset, and the rig's server,
/// told to go by a TERM it sends itself, is gone afterwards with its pane.
#[test]
#[cfg(target_os = "linux")]
fn the_signalling_rows_hold_in_a_binary_handed_an_ignored_and_blocked_term_hup_and_usr1() {
    let rows = [
        "a_wake_reaches_the_loop_and_sigterm_takes_the_waiter_along",
        "a_cockpit_tui_survives_sigusr1",
    ];
    let pid = fake::rows_pass_handed(&rows, &[libc::SIGTERM, libc::SIGHUP, libc::SIGUSR1]);
    assert!(until(|| left_by(pid) == 0), "the child's rig left its server or its pane");
}
