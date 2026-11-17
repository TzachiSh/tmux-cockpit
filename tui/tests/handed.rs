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
