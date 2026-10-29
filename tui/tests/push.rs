//! The push channel's receiving half, against a real tmux server on a private
//! `-L` socket (AGENTS.md: never the user's).
//!
//! `display-popup -E` tears its child down with a signal, so Drop never runs
//! and the pid would stay published -- and a published pid is one the hook goes
//! on signalling. SIGTERM must therefore unpublish it from the handler itself.
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
        std::thread::sleep(Duration::from_millis(50));
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

/// Everything this test must take down even when an assertion panics. A leaked
/// child holds the harness's captured stderr open and `cargo test` then waits
/// on that pipe forever -- the failing run hangs instead of reporting.
struct Rig {
    child: Child,
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
        .trim()
        .to_string()
    }
    fn published(&self) -> String {
        self.tmux(&["show-option", "-gqv", "@cockpit_tui_pid"])
    }
    fn signal(&self, sig: libc::c_int) {
        unsafe { libc::kill(self.child.id() as i32, sig) };
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.tmux(&["kill-server"]);
        // kill-server never unlinks the socket, by any exit path; the suite
        // learned that after ~850 dead inodes piled up (tests/run.sh:59-63).
        let uid = unsafe { libc::getuid() };
        let tmp = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let _ = std::fs::remove_file(PathBuf::from(tmp).join(format!("tmux-{uid}")).join(&self.sock));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A private server and a `push_hold` child publishing into it; `None` without tmux.
fn rig(tag: &str) -> Option<(Rig, PathBuf)> {
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("no tmux on PATH -- skipping");
        return None;
    }
    let sock = format!("cockpit-tui-{tag}-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("tui.log");
    // The server first: Push::start() publishes into it the moment it runs.
    let _ = Command::new("tmux")
        .args(["-L", &sock, "-f", "/dev/null", "new-session", "-d", "-s", "t", "sleep 300"])
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .output();
    let rig = Rig {
        child: Command::new(example("push_hold"))
            .env("COCKPIT_TMUX", format!("tmux -L {sock}"))
            .env("COCKPIT_LOG", &log)
            .env_remove("COCKPIT_READONLY")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            // the child hands the terminal back on SIGTERM; that escape
            // sequence is not this test's output
            .stdout(Stdio::null())
            .spawn()
            .expect("run the push_hold example"),
        sock,
        dir,
    };
    Some((rig, log))
}

#[test]
fn a_signal_wakes_it_and_sigterm_unpublishes_the_pid() {
    let Some((rig, log)) = rig("push") else { return };
    let pid = rig.child.id().to_string();
    assert!(until(|| rig.published() == pid), "the pid was never published");

    // The wake the hook sends. The handler sets the flag and raises SIGWINCH;
    // pending() is what turns that into the log line asserted here.
    rig.signal(libc::SIGUSR1);
    assert!(
        until(|| std::fs::read_to_string(&log).unwrap_or_default().contains("push wake")),
        "SIGUSR1 never reached the loop"
    );

    rig.signal(libc::SIGTERM);
    assert!(until(|| rig.published().is_empty()), "SIGTERM left @cockpit_tui_pid published");
}

/// The option is global: a second board that opened later owns it, and the
/// first one quitting must not take the second one's push away.
#[test]
fn quitting_leaves_another_boards_pid_published() {
    let Some((mut rig, _)) = rig("push2") else { return };
    let pid = rig.child.id().to_string();
    assert!(until(|| rig.published() == pid), "the pid was never published");

    rig.tmux(&["set-option", "-g", "@cockpit_tui_pid", "999999"]);
    rig.signal(libc::SIGTERM);
    assert!(
        until(|| rig.child.try_wait().ok().flatten().is_some()),
        "SIGTERM did not end the child"
    );
    assert_eq!(rig.published(), "999999", "the exit unpublished a pid that was not its own");
}

/// The `q` half, in-process: `Drop` runs `cleanup()`, not the signal arm.
#[test]
fn q_unpublishes_its_own_pid_and_only_its_own() {
    let Some((rig, _)) = rig("push3") else { return };
    // the child publishes once at start; let that land before ours does
    let child = rig.child.id().to_string();
    assert!(until(|| rig.published() == child), "the child never published");
    // SAFETY: the only test in this binary that reads COCKPIT_* in-process.
    unsafe {
        std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", rig.sock));
        std::env::remove_var("COCKPIT_READONLY");
    }
    let me = std::process::id().to_string();

    let p = cockpit_tui::push::Push::start();
    assert_eq!(rig.published(), me);
    drop(p);
    assert_eq!(rig.published(), "", "q left its own pid published");

    let p = cockpit_tui::push::Push::start();
    rig.tmux(&["set-option", "-g", "@cockpit_tui_pid", "999999"]);
    drop(p);
    assert_eq!(rig.published(), "999999", "q unpublished another board's pid");
}
