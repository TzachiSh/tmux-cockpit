//! What a child `fake::handed` started leaves behind once that child is ended (#1205): the
//! processes it started itself, a `sleep` its shell waits for and one its shell left as an
//! orphan, go with it.
mod fake;

use std::path::Path;
use std::process::{Command, Stdio};

/// Set for the child half alone: the file its shell writes the `sleep`'s pid and its own
/// parent's, the child half, to.
const PIDS: &str = "COCKPIT_TEST_HANDED_PIDS";

/// Set beside [`PIDS`]: `kept`, the shell waits for its `sleep`, or `orphan`, it exits.
const HOW: &str = "COCKPIT_TEST_HANDED_HOW";

/// The child half's name, [`forks_a_sleep_and_hangs`].
#[cfg(target_os = "linux")]
const FORKS: &str = "forks_a_sleep_and_hangs";

/// The child half: a shell that starts a `sleep` and writes both pids, then no end. Only
/// where [`PIDS`] names a file a row made, so a value exported in a shell does nothing.
#[test]
fn forks_a_sleep_and_hangs() {
    let Some(at) = std::env::var_os(PIDS).filter(|p| Path::new(p).is_file()) else {
        return;
    };
    let tail = if std::env::var(HOW).as_deref() == Ok("kept") { "wait" } else { ":" };
    // written aside and renamed, so a reader never sees half of it
    let body = format!("sleep 1000 & echo \"$! $PPID\" > \"$1.new\" && mv \"$1.new\" \"$1\"; {tail}");
    let mut sh = Command::new("sh")
        .args(["-c", &body, "sh"])
        .arg(&at)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    sh.wait().unwrap();
    loop {
        std::thread::park();
    }
}

/// Whether `f` holds within 500 polls 20 ms apart: a failsafe only, each wait here ends at
/// what it waits for.
#[cfg(target_os = "linux")]
fn within(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..500 {
        if f() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    f()
}

/// `pid`'s state and start time, fields 3 and 22 of its /proc stat: `None` where it is gone.
#[cfg(target_os = "linux")]
fn stat(pid: u32) -> Option<(char, u64)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let f: Vec<&str> = s.rsplit_once(") ")?.1.split(' ').collect();
    Some((f.first()?.chars().next()?, f.get(19)?.parse().ok()?))
}

/// Whether `pid` still runs as the process that started at `start`: a zombie, or another
/// process under a reused pid, is gone.
#[cfg(target_os = "linux")]
fn alive(pid: u32, start: u64) -> bool {
    stat(pid).is_some_and(|(s, t)| s != 'Z' && t == start)
}

/// The `sleep` and the child half named in `file`, each with its start time, once written.
#[cfg(target_os = "linux")]
fn pids(file: &Path) -> Option<[(u32, u64); 2]> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut it = text.split_whitespace().map(|p| p.parse::<u32>().ok());
    let mut one = || it.next().flatten().and_then(|p| Some((p, stat(p)?.1)));
    Some([one()?, one()?])
}

/// The child half in mode `how`, started by [`fake::handed_start`] and ended by `end`
/// once its `sleep` runs: the pids of that `sleep` and that child still running then, each
/// killed afterwards, read again as itself just before.
#[cfg(target_os = "linux")]
fn left_after(how: &str, end: impl FnOnce(fake::Handed)) -> Vec<u32> {
    let top = Path::new(env!("CARGO_TARGET_TMPDIR"));
    // one folder per call: the rows that call this run at once in one process
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let s = fake::Scratch::fresh(top.join(format!("handed-{how}-{}-{n}", std::process::id())));
    let file = s.join("pids");
    std::fs::write(&file, "").unwrap();
    let env = [(PIDS, file.to_str().unwrap()), (HOW, how)];
    let h = fake::handed_start(&[FORKS], &[], &env);
    // read while both run: once the end has killed the child half, its stat is gone
    let mut both = None;
    within(|| {
        both = pids(&file);
        both.is_some()
    });
    end(h);
    let Some(both) = both else {
        panic!("{how}: the child half's sleep never wrote its pid");
    };
    left_running(both)
}

/// Of `both`, as [`pids`] read them, the pids still running once each is gone or a poll
/// has run out, each then killed, read again as itself just before.
#[cfg(target_os = "linux")]
fn left_running(both: [(u32, u64); 2]) -> Vec<u32> {
    within(|| both.iter().all(|&(p, t)| !alive(p, t)));
    let left: Vec<_> = both.into_iter().filter(|&(p, t)| alive(p, t)).collect();
    for &(p, t) in &left {
        if alive(p, t) {
            // SAFETY: kill only signals; `p` was read again as this row's own just above.
            unsafe { libc::kill(p as libc::pid_t, libc::SIGKILL) };
        }
    }
    left.into_iter().map(|(p, _)| p).collect()
}

/// #1205: the bound's end takes along what the child it kills started.
#[test]
#[cfg(target_os = "linux")]
fn the_bound_ends_what_a_handed_child_started() {
    let end = |mut h: fake::Handed| drop(h.end_at_bound(fake::HANDED_FOR));
    let got = ["kept", "orphan"].map(|how| (how, left_after(how, end)));
    assert_eq!(got, [("kept", vec![]), ("orphan", vec![])]);
}

/// #1205: a row that panics while its handed child runs takes that child along, and what
/// the child started, in its unwind.
#[test]
#[cfg(target_os = "linux")]
fn an_unwind_ends_a_handed_child_and_what_it_started() {
    let end = |h: fake::Handed| {
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _h = h;
            panic!("#1205: a row's own panic");
        }));
        assert!(unwound.is_err());
    };
    let got = ["kept", "orphan"].map(|how| (how, left_after(how, end)));
    assert_eq!(got, [("kept", vec![]), ("orphan", vec![])]);
}

/// Set for [`starts_a_handed_child_and_parks`] alone: the file its handed child's shell
/// writes the pids to. Its `.ready` beside it is written once that child is started.
#[cfg(target_os = "linux")]
const OUTER: &str = "COCKPIT_TEST_HANDED_OUTER";

/// The name of [`starts_a_handed_child_and_parks`].
#[cfg(target_os = "linux")]
const PARKS: &str = "starts_a_handed_child_and_parks";

/// The outer child half: starts [`forks_a_sleep_and_hangs`] as `fake::handed` starts a
/// child, says so, then no end but a signal. Only where [`OUTER`] names a file a row made.
#[test]
#[cfg(target_os = "linux")]
fn starts_a_handed_child_and_parks() {
    let Some(at) = std::env::var_os(OUTER).filter(|p| Path::new(p).is_file()) else {
        return;
    };
    let env = [(PIDS, at.to_str().unwrap()), (HOW, "kept")];
    let _h = fake::handed_start(&[FORKS], &[], &env);
    std::fs::write(Path::new(&at).with_extension("ready"), "").unwrap();
    loop {
        std::thread::park();
    }
}

/// [`starts_a_handed_child_and_parks`] in a child of this binary with the three signals at
/// their default, sent `sig` once its handed child's `sleep` runs: the signal it died by,
/// and the pids of that `sleep` and that handed child still running, each then killed.
#[cfg(target_os = "linux")]
fn left_after_a_signal(sig: libc::c_int) -> (Option<i32>, Vec<u32>) {
    use std::os::unix::process::ExitStatusExt;
    let top = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let s = fake::Scratch::fresh(top.join(format!("handed-sig{sig}-{}", std::process::id())));
    let file = s.join("pids");
    std::fs::write(&file, "").unwrap();
    let mut c = Command::new(std::env::current_exe().unwrap());
    // its own handed folder under this one: it dies by the signal, and drops nothing
    c.args(["--exact", PARKS]).env(OUTER, &file).env("TMPDIR", &*s);
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    fake::at_default(&mut c, &[libc::SIGINT, libc::SIGTERM, libc::SIGHUP]);
    let mut outer = fake::spawn(&mut c).unwrap();
    // read while both run: once the signal has ended them, their stat is gone
    let mut both = None;
    within(|| {
        both = pids(&file).filter(|_| file.with_extension("ready").exists());
        both.is_some()
    });
    // SAFETY: kill only signals; `outer` is this row's own child, not yet reaped.
    unsafe { libc::kill(outer.id() as libc::pid_t, sig) };
    if !within(|| outer.try_wait().unwrap().is_some()) {
        outer.kill().unwrap();
    }
    let died = outer.wait().unwrap().signal();
    let Some(both) = both else {
        panic!("signal {sig}: the handed child's sleep never wrote its pid");
    };
    (died, left_running(both))
}

/// #1205: a handed child leads a group of its own, which a ^C at the terminal does not
/// reach. INT, TERM or HUP to the runner kills that group before the runner dies by it.
#[test]
#[cfg(target_os = "linux")]
fn a_signal_to_the_runner_ends_its_handed_children() {
    let rows = [("INT", libc::SIGINT), ("TERM", libc::SIGTERM), ("HUP", libc::SIGHUP)];
    let got = rows.map(|(name, sig)| (name, left_after_a_signal(sig)));
    assert_eq!(got, rows.map(|(name, sig)| (name, (Some(sig), vec![]))));
}

// --- a handed child's rig servers, in sessions of their own (#1233) ---

/// Set for [`starts_a_rig_and_hangs`] alone: the file it writes its server's pid to, or
/// `none` where tmux is not there to run.
#[cfg(target_os = "linux")]
const RIG: &str = "COCKPIT_TEST_HANDED_RIG";

/// The name of [`starts_a_rig_and_hangs`].
#[cfg(target_os = "linux")]
const RIGS: &str = "starts_a_rig_and_hangs";

/// The rig child half: a private server on `cockpit-handed-<its pid>`, the name a sweep by
/// that pid takes for a rig's, with one pane in the row's folder, then no end. The server
/// forks off and leads a session of its own, out of the child's group. Only where [`RIG`]
/// names a file a row made.
#[test]
#[cfg(target_os = "linux")]
fn starts_a_rig_and_hangs() {
    let Some(at) = std::env::var_os(RIG).filter(|p| Path::new(p).is_file()) else {
        return;
    };
    let at = Path::new(&at);
    let dir = at.parent().unwrap();
    let mut s = fake::Server::new(&format!("cockpit-handed-{}", std::process::id()));
    let started = s.start(|c| {
        c.args(["new-session", "-d", "-c"]).arg(dir).arg("sleep 1000");
    });
    let said = if started {
        let o = Command::new("tmux")
            .args(["-L", &s.sock, "display-message", "-p", "#{pid}"])
            .output();
        String::from_utf8_lossy(&o.unwrap().stdout).trim().to_string()
    } else {
        "none".to_string()
    };
    // written aside and renamed, so a reader never sees half of it
    std::fs::write(at.with_extension("new"), said).unwrap();
    std::fs::rename(at.with_extension("new"), at).unwrap();
    loop {
        std::thread::park();
    }
}

/// The socket of a rig child half's server, at the exact path tmux gives the name
/// `cockpit-handed-<pid>` in this binary's TMUX_TMPDIR, removed when dropped: a red run
/// kills that server, and a killed server leaves its socket for the runner to name.
#[cfg(target_os = "linux")]
struct RigSocket(std::path::PathBuf);

#[cfg(target_os = "linux")]
impl Drop for RigSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The rig child half started by [`fake::handed_start`] and ended by `end` once its
/// server runs: the child's pid, its server's socket and the folder its pane runs in, or
/// `None` where tmux is not there.
#[cfg(target_os = "linux")]
fn rig_ended(end: impl FnOnce(fake::Handed)) -> Option<(u32, RigSocket, fake::Scratch)> {
    let top = Path::new(env!("CARGO_TARGET_TMPDIR"));
    // one folder per call: the rows that call this run at once in one process
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let s = fake::Scratch::fresh(top.join(format!("handed-rig-{}-{n}", std::process::id())));
    let file = s.join("server");
    std::fs::write(&file, "").unwrap();
    let h = fake::handed_start(&[RIGS], &[], &[(RIG, file.to_str().unwrap())]);
    let pid = h.pid();
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let name = format!("cockpit-handed-{pid}");
    let sock = RigSocket(fake::checked_tmux().join(format!("tmux-{uid}")).join(name));
    let read = || std::fs::read_to_string(&file).unwrap_or_default();
    let ready = within(|| !read().is_empty());
    let said = read();
    end(h);
    assert!(ready, "the rig child half never wrote its server's pid");
    if said == "none" {
        eprintln!("SKIP: no tmux on PATH");
        return None;
    }
    Some((pid, sock, s))
}

/// #1233: the bound's end takes along the servers and panes of the rigs its child started,
/// which lead sessions of their own, and their sockets.
#[test]
#[cfg(target_os = "linux")]
fn the_bound_ends_a_handed_childs_rig_servers() {
    let Some((pid, sock, _dir)) = rig_ended(|mut h| drop(h.end_at_bound(fake::HANDED_FOR))) else {
        return;
    };
    fake::assert_no_server_left(pid);
    assert!(!sock.0.exists(), "{} is left", sock.0.display());
}

/// #1233: so does the unwind of a row that panicked while its handed child ran.
#[test]
#[cfg(target_os = "linux")]
fn an_unwind_ends_a_handed_childs_rig_servers() {
    let end = |h: fake::Handed| {
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _h = h;
            panic!("#1233: a row's own panic");
        }));
        assert!(unwound.is_err());
    };
    let Some((pid, sock, _dir)) = rig_ended(end) else {
        return;
    };
    fake::assert_no_server_left(pid);
    assert!(!sock.0.exists(), "{} is left", sock.0.display());
}
