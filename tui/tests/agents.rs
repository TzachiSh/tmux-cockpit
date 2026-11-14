//! `claude agents --json` off the fetch path and its cache file (#492 fix 5).
//! Gates use files and polls, never a clock: the stub waits on a `go` file.
mod fake;

use cockpit_tui::data::agents::{Background, Entry, Stage, flush, lookup_pid, lookups};
use cockpit_tui::data::agents::{is_lookup_child, read_file, resolve};
use cockpit_tui::data::agents::{set_background, set_stage_hook};
use cockpit_tui::data::list_panes;
use fake::write_exec;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());
type Lock = MutexGuard<'static, ()>;

/// field 22 of a stat line is `start`
fn stat(start: &str) -> String {
    format!("100 (cl aude) S 1 100 100 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 {start} 0 0\n")
}

struct Rig {
    dir: PathBuf,
    old: Vec<(&'static str, Option<String>)>,
    /// how long the drop waits for the lookups to end before it gives up
    wait: std::time::Duration,
    /// the shell the give-up execs: [`SH`], or a missing file for a box without one
    sh: PathBuf,
    _lock: Option<Lock>,
}

/// A lookup ends within the product's own 2.5 s deadline, so one still running after
/// this is a thread that hangs, on a box however loaded (#969).
const DROP_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// Each variable back to the value it had, or unset again. The caller holds `ENV`.
fn put_back(old: &[(&'static str, Option<String>)]) {
    for (k, v) in old {
        // SAFETY: the ENV mutex is held, as it was when these were set.
        unsafe {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

/// Polls until no lookup thread is left. False once `bound` has passed with one still running.
fn lookups_end_within(bound: std::time::Duration) -> bool {
    let end = std::time::Instant::now() + bound;
    while lookups() > 0 {
        if std::time::Instant::now() >= end {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    true
}

impl Rig {
    /// One agent pane, pid 100, no @cockpit_session, in a fake proc root at start `5`.
    /// `claude` runs `claude_body` and logs one line per call to `forks`.
    fn new(tag: &str, claude_body: &str) -> Rig {
        Rig::with(tag, claude_body, true)
    }

    /// `board` false is a one-shot caller (`--json`, `--rows`): it waits for the lookup.
    fn with(tag: &str, claude_body: &str, board: bool) -> Rig {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        Rig::build(tag, claude_body, board, Some(lock))
    }

    /// `lock` is `ENV`, held from here to the drop. None when the caller holds it.
    fn build(tag: &str, claude_body: &str, board: bool, lock: Option<Lock>) -> Rig {
        let dir = std::env::temp_dir().join(format!("cockpit-agents-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let d = dir.display().to_string();
        let row = ["%1", "w:0", "win", "claude", "running", "-", "-", "-", "/w", "100", "-", "-", "@1"].join("\u{1f}");
        std::fs::write(dir.join("rows"), row + "\n").unwrap();
        write_exec(
            &dir.join("bin/tmux"),
            &format!("#!/bin/sh\ncase \"$1\" in list-panes) cat {d}/rows ;; show-option) printf '|claude|' ;; esac\n"),
        );
        write_exec(&dir.join("bin/ps"), "#!/bin/sh\nexit 1\n");
        write_exec(&dir.join("bin/git"), "#!/bin/sh\nexit 1\n");
        // `exec $HOME/bin/sleep`: argv[0] keeps the rig's dir, so Drop can still verify the stub
        let real = ["/usr/bin/sleep", "/bin/sleep"]
            .into_iter()
            .find(|p| std::path::Path::new(p).exists());
        std::os::unix::fs::symlink(real.unwrap_or("/bin/sleep"), dir.join("bin/sleep")).unwrap();
        write_exec(&dir.join("bin/claude"), &format!("#!/bin/sh\necho 1 >> {d}/forks\n{claude_body}\n"));
        let root = dir.join("proc");
        std::fs::create_dir_all(root.join("self/task")).unwrap();
        std::fs::create_dir_all(root.join("100/task/100")).unwrap();
        std::fs::write(root.join("100/task/100/children"), "").unwrap();
        std::fs::write(root.join("100/comm"), "claude\n").unwrap();
        std::fs::write(root.join("100/stat"), stat("5")).unwrap();
        set_background(board);
        let path = std::env::var("PATH").unwrap_or_default();
        let vars = [
            ("PATH", format!("{d}/bin:{path}")),
            ("COCKPIT_TMUX", format!("{d}/bin/tmux")),
            ("COCKPIT_PROC_DIR", format!("{d}/proc")),
            ("HOME", d),
            ("COCKPIT_READONLY", "1".into()),
        ];
        let mut old: Vec<_> = vars.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect();
        old.push(("XDG_CACHE_HOME", std::env::var("XDG_CACHE_HOME").ok()));
        // SAFETY: the ENV mutex is held until Drop restores these.
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
            for (k, v) in &vars {
                std::env::set_var(k, v);
            }
        }
        let rig = Rig {
            dir,
            old,
            wait: DROP_WAIT,
            sh: PathBuf::from(SH),
            _lock: lock,
        };
        // once the rig exists, so that a failure here drops it and the variables go back (#972)
        fake::assert_claude_is_the_rigs(&rig.dir);
        rig
    }

    /// A board's rig inside `_dev`, which holds `ENV`: its drop puts back what `_dev` set.
    fn inside(_dev: &Dev, tag: &str, claude_body: &str) -> Rig {
        Rig::build(tag, claude_body, true, None)
    }

    fn proc_dir(&self, pid: &str, comm: &str, kids: &str) {
        let d = self.dir.join("proc").join(pid);
        std::fs::create_dir_all(d.join("task").join(pid)).unwrap();
        std::fs::write(d.join("task").join(pid).join("children"), kids).unwrap();
        std::fs::write(d.join("comm"), format!("{comm}\n")).unwrap();
    }

    fn cache(&self) -> PathBuf {
        self.dir.join(".cache/tmux-cockpit/agents.json")
    }

    fn forks(&self) -> usize {
        std::fs::read_to_string(self.dir.join("forks")).map_or(0, |s| s.lines().count())
    }

    fn sess(&self) -> String {
        list_panes().unwrap().into_iter().next().unwrap().sess
    }

    /// Poll until the file the thread writes last is there.
    fn wait_cache(&self) -> serde_json::Value {
        for _ in 0..2000 {
            if let Ok(s) = std::fs::read_to_string(self.cache()) {
                return serde_json::from_str(&s).unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("no cache file appeared");
    }

    fn seed(&self, body: &str) {
        std::fs::create_dir_all(self.cache().parent().unwrap()).unwrap();
        std::fs::write(self.cache(), body).unwrap();
    }
}

/// Is `pid` still this rig's stub? Its cmdline holds the stub's own path, which
/// lives under the rig's dir, so a reused pid of anything else never matches.
fn stub_of(pid: i32, dir: &std::path::Path) -> bool {
    let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    // "<dir>/": a bare prefix would also match another run's dir whose pid has more digits
    let mut dir = dir.as_os_str().as_encoded_bytes().to_vec();
    dir.push(b'/');
    dir.len() > 1 && cmd.windows(dir.len()).any(|w| w == dir.as_slice())
}

/// The shell the give-up becomes, by full path so that no PATH is searched for it (#988).
const SH: &str = "/bin/sh";

/// The give-up's last line, printed by [`SH`] from the environment the exec handed it,
/// then SIGABRT under the same pid. The exec sets SIGPIPE back to its default, so the
/// trap keeps a closed stderr from ending it by SIGPIPE first. `command` passes over a
/// function of that name imported from the environment, which bash does even as `sh`.
/// `${HOME-}` holds under an exported `SHELLOPTS=nounset`. `exit 134` only if the kill
/// did not end it (#988).
const SAY: &str = concat!(
    r#"trap '' PIPE; command printf 'At the abort PATH began with %s and HOME was %s\n' "#,
    r#""${PATH%%:*}" "${HOME-}" >&2; command kill -s ABRT "$$"; exit 134"#,
);

/// The give-up's last line as this binary writes it when there is no shell to exec.
fn last_line(path: &str, home: &str) -> String {
    let first = path.split(':').next().unwrap_or_default();
    format!("At the abort PATH began with {first} and HOME was {home}\n")
}

/// The fallback's line ahead of the last: which path wrote it, and why (#988).
fn exec_failed(sh: &Path, e: &std::io::Error) -> String {
    format!("No shell from {}: {e}. The line below is this binary's own read.\n", sh.display())
}

/// The drop gave up: says so on the real stderr, past the harness's capture, and ends
/// the run there. The variables stay the rig's and `dir` is left (#969).
fn give_up(dir: &Path, bound: std::time::Duration, sh: &Path) -> ! {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    let me = std::thread::current();
    let (test, n) = (me.name().unwrap_or("the test that ran last"), lookups());
    let how = if std::thread::panicking() { ", while it was panicking" } else { "" };
    let msg = format!(
        "\nThe agents test rig stopped this test binary. This is the rig's guard for issue \
         #969, not a product crash.\n{n} lookup thread(s) were still running {bound:?} after \
         the rig of {test} began its drop{how}. Putting PATH and HOME back would let that \
         lookup start the real `claude`, so they stay the rig's and the run aborts here.\n\
         What to do: rerun {test} alone, with `cargo test --test agents -- --exact {test}`.\n\
         The rig's folder was left behind: {}\n",
        dir.display(),
    );
    let _ = std::io::stderr().write_all(msg.as_bytes());
    // The last line is printed by the shell this process becomes. No line of this binary
    // runs after the exec, so the line shows PATH and HOME as a late lookup would have
    // found them, whatever was written above it (#988). An exec keeps an ignored action
    // and the signal mask, which abort() would undo for itself: undone here, so that the
    // shell's kill ends the process.
    // SAFETY: sets the default action and unblocks SIGABRT on this thread, no handler.
    unsafe {
        libc::signal(libc::SIGABRT, libc::SIG_DFL);
        let mut abrt: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut abrt);
        libc::sigaddset(&mut abrt, libc::SIGABRT);
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &abrt, std::ptr::null_mut());
    }
    let failed = std::process::Command::new(sh).arg0("sh").args(["-c", SAY]).exec();
    // No shell: the line from this binary's own read, which a put-back between this read
    // and the abort would get past, as one anywhere after the read did before #988. The
    // line ahead of it says so, for the reader of a give-up (#988).
    let path = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let read = last_line(&path, &home);
    let _ = std::io::stderr().write_all((exec_failed(sh, &failed) + &read).as_bytes());
    std::process::abort();
}

impl Drop for Rig {
    fn drop(&mut self) {
        // a hook set under this rig ends with it, on a panic too: left set, it would
        // run in every later test's lookups (#1024)
        set_stage_hook(None);
        // a red run must not leave a stub looping on a file that never comes
        let _ = std::fs::write(self.dir.join("go"), "");
        // AGENTS.md: never signal a pid nobody verified. The stub may be long gone and
        // its pid reused, so kill it only while its cmdline still names this rig's dir.
        if let Ok(p) = std::fs::read_to_string(self.dir.join("stubpid"))
            && let Ok(p) = p.trim().parse::<i32>()
            && stub_of(p, &self.dir)
        {
            // SAFETY: a plain signal to a pid whose cmdline names this rig's own dir.
            unsafe { libc::kill(p, libc::SIGKILL) };
        }
        // A lookup thread forks `claude` on its own schedule, by the PATH of that moment.
        // Put back first, the PATH would hand one that has not forked yet the developer's
        // own `claude`, under the run's HOME (#969). So the variables stay the rig's until
        // no lookup is left, and for good if one never ends. The wait also covers the
        // cache write, which would make the dir again after it is removed (#608).
        if !lookups_end_within(self.wait) {
            give_up(&self.dir, self.wait, &self.sh);
        }
        put_back(&self.old);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A developer's side of the environment, made up: a HOME of its own and a PATH whose
/// first `claude` logs each start to `outside`. A rig built inside it inherits both,
/// and its drop puts them back as it would a developer's own (#969).
struct Dev {
    dir: PathBuf,
    old: Vec<(&'static str, Option<String>)>,
    _lock: Lock,
}

impl Dev {
    fn new(tag: &str) -> Dev {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let name = format!("cockpit-agents-{tag}-dev-{}", std::process::id());
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let d = dir.display().to_string();
        write_exec(
            &dir.join("bin/claude"),
            &format!("#!/bin/sh\necho \"HOME=$HOME\" >> {d}/outside\nexit 1\n"),
        );
        let path = std::env::var("PATH").unwrap_or_default();
        let vars = [("PATH", format!("{d}/bin:{path}")), ("HOME", d)];
        let mut old: Vec<_> = vars.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect();
        old.push(("XDG_CACHE_HOME", std::env::var("XDG_CACHE_HOME").ok()));
        // SAFETY: the ENV mutex is held until Drop restores these.
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
            for (k, v) in &vars {
                std::env::set_var(k, v);
            }
        }
        Dev { dir, old, _lock: lock }
    }

    /// One line per start of the `claude` outside the rig.
    fn outside(&self) -> String {
        std::fs::read_to_string(self.dir.join("outside")).unwrap_or_default()
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        put_back(&self.old);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The dir of the rig whose lookup thread [`ask_again_at_the_drop`] is to hold.
static HELD: Mutex<Option<PathBuf>> = Mutex::new(None);

/// On the lookup thread at its end, still counted: waits for the rig's drop to begin
/// (it writes `go`), then asks once more on this thread, as a one-shot caller does.
/// The product has no stage ahead of its fork, so this second ask stands in for a
/// lookup thread that has not forked yet when its rig drops (#969).
fn ask_again_at_the_drop(at: Stage, _pid: u32) {
    if at != Stage::Reaping {
        return;
    }
    // taken, so the ask below, which ends in this stage too, passes through
    let Some(dir) = HELD.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    std::fs::write(dir.join("held"), "").unwrap();
    let end = std::time::Instant::now() + GONE_BY;
    while !dir.join("go").exists() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    set_background(false);
    resolve(&[("100".into(), "claude".into())]);
}

/// A rig dropped while its lookup thread still has an ask to make, by returning or
/// by a panic: how many asks reached the rig's `claude`, and the log of the one outside.
fn asks_of_a_lookup_held_past_the_drop(tag: &str, panics: bool) -> (usize, String) {
    let dev = Dev::new(tag);
    let inside = dev.dir.join("inside");
    let body = format!("echo 1 >> {}\n{ONE}", inside.display());
    let rig = std::panic::AssertUnwindSafe(|| {
        let r = Rig::inside(&dev, tag, &body);
        *HELD.lock().unwrap_or_else(|e| e.into_inner()) = Some(r.dir.clone());
        set_stage_hook(Some(ask_again_at_the_drop));
        r.sess();
        wait_file(&r.dir.join("held"));
        assert!(!panics, "a failed assert between the fetch and its wait");
    });
    assert_eq!(std::panic::catch_unwind(rig).is_err(), panics);
    assert!(lookups_end_within(GONE_BY), "the lookup never finished");
    set_stage_hook(None);
    let inside = std::fs::read_to_string(inside).map_or(0, |s| s.lines().count());
    (inside, dev.outside())
}

/// #969: a lookup that has not forked when its rig drops still finds the rig's stub.
#[test]
fn a_lookup_pending_at_the_drop_starts_no_claude_outside_the_rig() {
    let (inside, outside) = asks_of_a_lookup_held_past_the_drop("late", false);
    assert_eq!(outside, "", "a `claude` outside the rig was started");
    assert_eq!(inside, 2, "both asks reached the rig's stub");
}

/// A second case: the drop runs in a panic's unwind, as after any failed assert.
#[test]
fn nor_does_one_pending_at_a_drop_during_a_panic() {
    let (inside, outside) = asks_of_a_lookup_held_past_the_drop("latepanic", true);
    assert_eq!(outside, "", "a `claude` outside the rig was started");
    assert_eq!(inside, 2, "both asks reached the rig's stub");
}

/// #1024: a rig dropped in a panic's unwind leaves no stage hook set for the next rig.
#[test]
fn a_rig_dropped_in_a_panic_leaves_no_stage_hook_set() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn count(_at: Stage, _pid: u32) {
        CALLS.fetch_add(1, Ordering::SeqCst);
    }
    // the dev holds ENV across both rigs, so no other test clears the hook in between
    let dev = Dev::new("hookleft");
    let red = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _r = Rig::inside(&dev, "hookleft", ONE);
        set_stage_hook(Some(count));
        panic!("a red between the set and the test's own clear");
    }));
    assert!(red.is_err(), "the forced panic did not fire");
    Rig::inside(&dev, "hookleft2", ONE).sess();
    assert_eq!(CALLS.load(Ordering::SeqCst), 0, "the hook ran in a later rig's lookup");
}

/// The child half of the give-up test: a lookup thread that never ends, and a rig
/// whose drop waits 200 ms for it. Should the rig's HOME ever be put back under that
/// thread, it asks again, and the `claude` outside the rig logs it. `HUNG_LOOKUP_CHILD`
/// says how the rig goes: `panic` drops it in a panic's unwind, `nosh` gives it a
/// shell path that does not exist, and anything else drops it plainly (#988).
#[test]
fn hung_lookup_child() {
    let Ok(how) = std::env::var("HUNG_LOOKUP_CHILD") else {
        return;
    };
    fn hang(at: Stage, _pid: u32) {
        if at == Stage::Reaping {
            let home = std::env::var("HOME").unwrap();
            std::fs::write(Path::new(&home).join("held"), "").unwrap();
            loop {
                if std::env::var("HOME").unwrap() != home {
                    set_background(false);
                    resolve(&[("100".into(), "claude".into())]);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
    // SAFETY: setrlimit reads one struct. No core file for an abort that is meant.
    unsafe { libc::setrlimit(libc::RLIMIT_CORE, &libc::rlimit { rlim_cur: 0, rlim_max: 0 }) };
    let dev = Dev::new("hung");
    std::fs::write(dev.dir.join("made"), "").unwrap();
    let mut r = Rig::inside(&dev, "hung", ONE);
    r.wait = std::time::Duration::from_millis(200);
    if how == "nosh" {
        r.sh = r.dir.join("no-such-sh");
    }
    set_stage_hook(Some(hang));
    r.sess();
    wait_file(&r.dir.join("held"));
    assert!(how != "panic", "a failed assert while the lookup hangs");
    drop(r);
    std::process::exit(3);
}

/// Runs the child half `how`: its exit, its stderr, and the rig's and the dev's dirs it
/// names. `ignored` starts it with SIGABRT ignored and `blocked` with SIGABRT blocked,
/// as a parent may hand either down (#988).
fn hung_lookup_run(how: &str) -> (std::process::ExitStatus, String, PathBuf, PathBuf) {
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "hung_lookup_child", "--test-threads=1"])
        .env("HUNG_LOOKUP_CHILD", how);
    let (ignore, block) = (how == "ignored", how == "blocked");
    let hand_down = move || {
        // SAFETY: sets an action or the mask, no handler.
        unsafe {
            let mut abrt: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut abrt);
            libc::sigaddset(&mut abrt, libc::SIGABRT);
            if ignore {
                libc::signal(libc::SIGABRT, libc::SIG_IGN);
            }
            if block {
                libc::sigprocmask(libc::SIG_BLOCK, &abrt, std::ptr::null_mut());
            }
        }
        Ok(())
    };
    // SAFETY: the calls above are async-signal-safe, made between fork and exec.
    unsafe { child.pre_exec(hand_down) };
    let child = child.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    let child = child.spawn().unwrap();
    let pid = child.id();
    let out = child.wait_with_output().unwrap();
    let left = |tag: &str| std::env::temp_dir().join(format!("cockpit-agents-{tag}-{pid}"));
    (
        out.status,
        String::from_utf8_lossy(&out.stderr).into_owned(),
        left("hung"),
        left("hung-dev"),
    )
}

/// Runs the child half `how` and asserts the give-up of #969: SIGABRT, the message with
/// the rig's own PATH and HOME on its last line, the dirs left, no `claude` outside the
/// rig started. Returns the child's stderr and the rig's dir.
fn gave_up(how: &str) -> (String, PathBuf) {
    let _r = Rig::new("giveup", ONE);
    let (status, err, rig, dev) = hung_lookup_run(how);
    let seen = (rig.join("go").exists(), dev.join("made").exists(), dev.join("outside").exists());
    let _ = (std::fs::remove_dir_all(&rig), std::fs::remove_dir_all(&dev));
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc::SIGABRT), "{status:?}");
    let r = rig.display();
    let left = format!("The rig's folder was left behind: {r}\n");
    let kept = format!("At the abort PATH began with {r}/bin and HOME was {r}\n");
    let said = ["not a product crash", "rerun hung_lookup_child alone", &left, &kept];
    assert!(said.iter().all(|s| err.contains(s)), "{err}");
    assert_eq!(seen, (true, true, false), "(rig dir left, dev dir left, no outside claude)");
    (err, rig)
}

/// #969: a drop whose lookup never ends does not put PATH and HOME back. It says
/// why and aborts, and leaves its dirs, the made-up developer's `claude` unstarted.
#[test]
fn a_drop_whose_lookup_never_ends_aborts_with_the_variables_kept() {
    gave_up("return");
}

/// #988: the same when the rig drops in a panic's unwind, as after a failed assert.
#[test]
fn so_does_one_dropped_in_a_panics_unwind() {
    let (err, _) = gave_up("panic");
    assert!(err.contains("began its drop, while it was panicking."), "{err}");
}

/// #988: with no shell at the path, the binary reads PATH and HOME itself and aborts,
/// and the line ahead of the last says that the shell could not be started, and why.
#[test]
fn so_does_one_with_no_shell_to_exec() {
    let (err, rig) = gave_up("nosh");
    let enoent = std::io::Error::from_raw_os_error(libc::ENOENT);
    let r = rig.display();
    let kept = format!("At the abort PATH began with {r}/bin and HOME was {r}\n");
    assert!(err.contains(&(exec_failed(&rig.join("no-such-sh"), &enoent) + &kept)), "{err}");
}

/// #988: an ignored SIGABRT, which an exec hands down, does not keep the shell alive.
#[test]
fn so_does_one_started_with_sigabrt_ignored() {
    gave_up("ignored");
}

/// #988: nor does a blocked one, which an exec hands down too.
#[test]
fn so_does_one_started_with_sigabrt_blocked() {
    gave_up("blocked");
}

/// #988: with nobody left to read stderr the shell still dies by SIGABRT, as abort()
/// does, though the exec has set SIGPIPE back to its default.
#[test]
fn the_shell_dies_by_sigabrt_on_a_stderr_nobody_reads() {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let (read, write) = std::io::pipe().unwrap();
    drop(read);
    let mut sh = std::process::Command::new(SH);
    let status = sh.arg0("sh").args(["-c", SAY]).stderr(write).status().unwrap();
    assert_eq!(status.signal(), Some(libc::SIGABRT), "{status:?}");
}

/// Rows of a PATH's first entry and a HOME, each by name. `None` leaves HOME unset.
const ODD: [(&str, Option<&str>); 9] = [
    ("quotes", Some("q'x\"y")),
    ("blank", Some("a b")),
    ("percent", Some("%s%d%%")),
    ("backslash", Some("a\\nb\\")),
    ("newline", Some("a\nb")),
    ("expansion", Some("$(id)`id`*")),
    ("dash", Some("-n")),
    ("empty", Some("")),
    ("unset", None),
];

/// #988: the shell prints PATH's first entry and HOME whole, neither expanded nor
/// read as a format, as this binary's own read would, and dies by SIGABRT.
#[test]
fn the_shell_prints_odd_values_whole() {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    for (row, v) in ODD {
        let path = format!("{}:/second", v.unwrap_or("/first"));
        let mut sh = std::process::Command::new(SH);
        sh.arg0("sh").args(["-c", SAY]).env_clear().env("PATH", &path);
        if let Some(v) = v {
            sh.env("HOME", v);
        }
        let out = sh.output().unwrap();
        let want = last_line(&path, v.unwrap_or_default());
        assert_eq!(String::from_utf8_lossy(&out.stderr), want, "row {row}");
        assert_eq!(out.status.signal(), Some(libc::SIGABRT), "row {row}");
    }
}

const ONE: &str = r#"printf '[{"pid":100,"sessionId":"s-100","cwd":"/launch","kind":"interactive"}]'"#;

/// A board's lookup that answers only once the rig is dropped (Drop writes `go`)
/// writes the cache after that: the rig must still leave no dir behind (#608).
#[test]
fn a_rig_leaves_no_dir_behind_a_background_lookup() {
    // it answers once Drop says go, or once Drop has removed the dir: either way it writes
    let body = format!("while [ ! -f $HOME/go ] && [ -d $HOME ]; do sleep 0.02; done\n{ONE}");
    let dir = {
        let r = Rig::new("leak", &body);
        r.sess();
        wait_forks(&r, 1);
        r.dir.clone()
    };
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while lookups() > 0 {
        assert!(std::time::Instant::now() < end, "the lookup never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!dir.exists(), "{} was left behind", dir.display());
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// A stub pid that was reused by an unrelated process is not killed: the rig only
/// signals a pid whose cmdline still names its own dir (AGENTS.md).
#[test]
fn the_rig_never_kills_a_pid_that_is_not_its_stub() {
    let mut other = {
        let r = Rig::new("reused", ONE);
        // After Rig::new, which can wait on the ENV lock behind other rigs: a sleep started
        // before it could end on its own and be blamed on the drop. An absolute path, since
        // a bare `sleep` is the rig's bin/sleep, whose cmdline names the rig's dir.
        let other = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
        std::fs::write(r.dir.join("stubpid"), other.id().to_string()).unwrap();
        other
    };
    // Child::kill is a SIGKILL too, so a look right after the drop could not tell a kill
    // still on its way from none (#527). This ends it with SIGTERM: a SIGKILL already sent
    // wins, and the exit status says which signal it died of.
    // SAFETY: a plain signal to this test's own unreaped child.
    unsafe { libc::kill(other.id() as libc::pid_t, libc::SIGTERM) };
    let st = other.wait().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        st.signal(),
        Some(libc::SIGTERM),
        "the rig's drop killed a process that was not its stub"
    );
}

/// A stub that has gone, its pid reaped, is no stub of the rig's: nothing is signalled.
#[test]
fn a_gone_stub_pid_is_not_the_rigs() {
    let r = Rig::new("gone", ONE);
    let mut child = std::process::Command::new(r.dir.join("bin/sleep")).arg("0").spawn().unwrap();
    let pid = child.id() as i32;
    child.wait().unwrap();
    assert!(!stub_of(pid, &r.dir), "pid {pid}, reaped, still read as the rig's stub");
}

/// The positive path: an exec'd stub (argv[0] under the rig's bin) is reaped by Drop.
#[test]
fn the_rig_kills_its_own_execd_stub() {
    let (pid, mut child) = {
        let r = Rig::new("own", ONE);
        let child = std::process::Command::new(r.dir.join("bin/sleep")).arg("30").spawn().unwrap();
        std::fs::write(r.dir.join("stubpid"), child.id().to_string()).unwrap();
        // between fork and exec the child still shows this test's own cmdline; wait for the exec
        let execd = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(5));
            stub_of(child.id() as i32, &r.dir)
        });
        assert!(execd, "the stub never showed the rig's dir in its cmdline");
        (child.id(), child)
    };
    let gone = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        matches!(child.try_wait(), Ok(Some(_)))
    });
    if !gone {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert!(gone, "the rig's drop left its own stub {pid} running");
}

/// The mechanism: the fetch waited for `claude agents`. The stub blocks until
/// the test says go, so a fetch that returns did not wait.
#[test]
fn the_fetch_returns_while_the_lookup_is_still_running() {
    let r = Rig::new(
        "slow",
        &format!("touch {d}/started\nwhile [ ! -f {d}/go ]; do sleep 0.02; done\n{ONE}", d = "$HOME"),
    );
    assert_eq!(r.sess(), "", "the first answer is not in yet");
    assert!(!r.cache().exists(), "the stub is still blocked");
    std::fs::write(r.dir.join("go"), "").unwrap();
    r.wait_cache();
    assert_eq!(r.sess(), "s-100", "the next fetch uses the answer");
}

/// At most one lookup at a time: ten fetches while the first is blocked fork once.
#[test]
fn fetches_during_a_lookup_start_no_second_one() {
    let r = Rig::new("one", &format!("while [ ! -f $HOME/go ]; do sleep 0.02; done\n{ONE}"));
    for _ in 0..10 {
        r.sess();
    }
    std::fs::write(r.dir.join("go"), "").unwrap();
    r.wait_cache();
    // the fetches that waited left their pid on the list; the answer settled it
    assert_eq!(r.sess(), "s-100");
    assert_eq!(r.sess(), "s-100");
    assert_eq!(r.forks(), 1);
}

#[test]
fn the_answer_is_written_in_the_agreed_format_mode_600() {
    let r = Rig::new("fmt", ONE);
    r.sess();
    let v = r.wait_cache();
    assert_eq!(v["v"], 1);
    assert!(v["at"].as_u64().unwrap().abs_diff(now()) < 60);
    let e = &v["by_pid"]["100:5"];
    assert_eq!(
        (e["session"].as_str(), e["cwd"].as_str(), e["kind"].as_str()),
        (Some("s-100"), Some("/launch"), Some("interactive"))
    );
    assert_eq!(v["none"], serde_json::json!([]));
    assert_eq!(std::fs::metadata(r.cache()).unwrap().mode() & 0o777, 0o600);
}

/// Atomic: the file is a rename of a temp file, which leaves no temp behind and
/// a new inode where a plain write would reuse the old one.
#[test]
fn the_cache_is_replaced_by_rename_leaving_no_temp() {
    let r = Rig::new("atomic", ONE);
    r.seed("old");
    let before = std::fs::metadata(r.cache()).unwrap().ino();
    r.sess();
    for _ in 0..2000 {
        if std::fs::metadata(r.cache()).unwrap().ino() != before {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_ne!(std::fs::metadata(r.cache()).unwrap().ino(), before);
    let names: Vec<_> = std::fs::read_dir(r.cache().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["agents.json"]);
}

/// A pid `claude agents` did not list is cached as a negative: three more
/// fetches ask nothing, and the file says so.
#[test]
fn a_pid_claude_does_not_know_is_not_asked_again() {
    let r = Rig::new("neg", "printf '[]'");
    r.sess();
    let v = r.wait_cache();
    assert_eq!(v["none"], serde_json::json!(["100:5"]));
    for _ in 0..3 {
        assert_eq!(r.sess(), "");
    }
    assert_eq!(r.forks(), 1);
}

/// A second case of the class: a failed spawn (no `claude` that runs) rests too.
#[test]
fn a_claude_that_exits_empty_is_not_asked_again_either() {
    let r = Rig::new("neg2", "exit 1");
    r.sess();
    wait_forks(&r, 1);
    // no file to wait on (a failed call writes none): fetch over a window instead
    for _ in 0..100 {
        r.sess();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(r.forks(), 1);
}

fn seeded(r: &Rig, start: &str) {
    r.seed(&format!(
        r#"{{"v":1,"at":{},"by_pid":{{"100:{start}":{{"session":"s-seed","cwd":"/c","kind":"background"}}}},"none":[]}}"#,
        now()
    ));
}

/// Another board's fresh answer is used with no call of our own.
#[test]
fn a_fresh_file_with_a_matching_start_answers_without_a_call() {
    let r = Rig::new("hit", ONE);
    seeded(&r, "5");
    assert_eq!(r.sess(), "s-seed");
    assert_eq!(r.forks(), 0);
}

/// A reused pid has another start time: the entry is absent and the call runs.
#[test]
fn a_pid_with_a_mismatched_start_counts_as_absent() {
    let r = Rig::new("reuse", ONE);
    seeded(&r, "999");
    assert_eq!(r.sess(), "");
    for _ in 0..2000 {
        if r.forks() == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(r.forks(), 1);
}

#[test]
fn the_reader_guards_age_version_syntax_start_and_proc() {
    let r = Rig::new("guards", ONE);
    let root = r.dir.join("proc");
    let at =
        |t: u64, v: u64| format!(r#"{{"v":{v},"at":{t},"by_pid":{{"100:5":{{"session":"a","cwd":"/c","kind":"k"}}}},"none":["100:5"]}}"#);
    let t = now();
    let fresh = Entry {
        session: "a".into(),
        cwd: "/c".into(),
        kind: "k".into(),
    };
    let p = r.dir.join("f.json");
    let rows: [(&str, String, bool); 8] = [
        ("fresh", at(t, 1), true),
        ("exactly 90 s old", at(t - 90, 1), true),
        ("91 s old", at(t - 91, 1), false),
        ("wrong v", at(t, 2), false),
        ("v 0", at(t, 0), false),
        ("unparseable", "{not json".into(), false),
        ("empty", String::new(), false),
        ("array", "[]".into(), false),
    ];
    for (name, body, ok) in rows {
        std::fs::write(&p, body).unwrap();
        let got = read_file(&p, &root, t);
        assert_eq!(got.is_some(), ok, "{name}");
        if ok {
            let s = got.unwrap();
            assert_eq!(s.by_pid.get("100"), Some(&fresh), "{name}");
            assert!(s.none.contains("100"), "{name}");
        }
    }
    // the start moved: the same file is empty of entries, not absent
    std::fs::write(root.join("100/stat"), stat("6")).unwrap();
    std::fs::write(&p, at(t, 1)).unwrap();
    let s = read_file(&p, &root, t).unwrap();
    assert!(s.by_pid.is_empty() && s.none.is_empty(), "start 6 vs key 5");
    // no proc root (macOS): every entry is absent
    let s = read_file(&p, &r.dir.join("nope"), t).unwrap();
    assert!(s.by_pid.is_empty() && s.none.is_empty());
}

/// A file keyed with a path-like pid never reaches outside the proc root.
#[test]
fn a_key_that_is_not_digits_is_dropped() {
    let r = Rig::new("keys", ONE);
    let p = r.dir.join("f.json");
    // what "../100" would reach if the key were not refused
    std::fs::create_dir_all(r.dir.join("100")).unwrap();
    std::fs::write(r.dir.join("100/stat"), stat("5")).unwrap();
    let body = format!(
        r#"{{"v":1,"at":{},"by_pid":{{"../100:5":{{"session":"x"}},"100:":{{"session":"x"}},"100":{{"session":"x"}}}},"none":["../100:5"]}}"#,
        now()
    );
    std::fs::write(&p, body).unwrap();
    let s = read_file(&p, &r.dir.join("proc"), now()).unwrap();
    assert!(s.by_pid.is_empty() && s.none.is_empty());
}

/// What the lookup thread holds back, between `spawn`'s return and the store (#976).
#[derive(Clone, Copy, PartialEq)]
enum Hold {
    Nothing,
    /// the store, until the test has read the stub's pid twice
    Store,
    /// the stub's pid, until the test has read stubpid empty, and then as `Store`
    EmptyFile,
}

/// Polls of [`wait_in_flight`] that read a pid, and that read stubpid empty (#976).
static PID_READS: AtomicUsize = AtomicUsize::new(0);
static EMPTY_READS: AtomicUsize = AtomicUsize::new(0);

/// On the lookup thread: polls until `done`, or the test's `go` on a red run, for a
/// bounded count of polls.
fn hold_until(done: impl Fn() -> bool) {
    let go = Path::new(&std::env::var("HOME").unwrap()).join("go");
    for _ in 0..1000 {
        if done() || go.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Holds the store until the test has read the stub's pid twice while the board held
/// none. A helper that walks on its first read of a pid walks inside the hold.
fn hold_the_store(at: Stage, _pid: u32) {
    if at == Stage::Spawned {
        let from = PID_READS.load(Ordering::SeqCst);
        hold_until(|| PID_READS.load(Ordering::SeqCst) >= from + 2);
    }
}

/// Holds the stub's pid back until the test has read stubpid empty, then holds the
/// store as [`hold_the_store`]. A helper that takes an empty read walks inside the hold.
fn hold_an_empty_stubpid(at: Stage, pid: u32) {
    if at == Stage::Spawned {
        let from = EMPTY_READS.load(Ordering::SeqCst);
        hold_until(|| EMPTY_READS.load(Ordering::SeqCst) > from);
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        std::fs::write(home.join("released"), "").unwrap();
        hold_the_store(at, pid);
    }
}

/// The lookup is a child of the board, and the board sits in a pane: a walk of
/// that pane's tree meets a `claude` and the pane, which runs no agent, turns
/// into a row until the call ends.
fn own_pane_stays_no_agent(tag: &str, grandchild: bool, by_ps: bool, hold: Hold) {
    const GO: &str = "while [ ! -f $HOME/go ]; do sleep 0.02; done\n";
    let wait = if hold == Hold::EmptyFile {
        ": > $HOME/stubpid\nwhile [ ! -f $HOME/released ]; do sleep 0.02; done\n"
    } else {
        ""
    };
    let r = Rig::new(tag, &format!("{wait}echo $$ > $HOME/stubpid\n{GO}{ONE}"));
    let row = ["%2", "w:1", "board", "bash", "-", "-", "-", "-", "/w", "200", "-", "-", "@2"].join("\u{1f}");
    let mut rows = std::fs::read_to_string(r.dir.join("rows")).unwrap();
    rows.push_str(&(row + "\n"));
    std::fs::write(r.dir.join("rows"), rows).unwrap();
    let root = r.dir.join("proc");
    r.proc_dir("200", "bash", "");
    match hold {
        Hold::Nothing => {}
        Hold::Store => set_stage_hook(Some(hold_the_store)),
        Hold::EmptyFile => set_stage_hook(Some(hold_an_empty_stubpid)),
    }
    r.sess();
    // `spawn` returns only after the exec, so the stub writes stubpid while the board
    // may not hold its pid yet, and a walk then meets a child it does not know. And
    // an empty read, between the stub's truncate and its write, checks nothing (#976).
    let pid = wait_in_flight().to_string();
    // the stub is the board's child, and it is named claude
    std::fs::write(root.join("200/task/200/children"), &pid).unwrap();
    // the real `claude` forks helpers; one named claude, under the child, counts as the child
    let (comm, kids) = if grandchild { ("node", "999999") } else { ("claude", "") };
    r.proc_dir(&pid, comm, kids);
    if grandchild {
        r.proc_dir("999999", "claude", "");
    }
    if by_ps {
        // no proc root to walk: the fake `ps` is the tree
        std::fs::remove_dir_all(root.join("self")).unwrap();
        let kid = if grandchild {
            format!("999999 {pid} claude\n")
        } else {
            String::new()
        };
        let ps = format!("100 1 claude\n200 1 bash\n{pid} 200 claude\n{kid}");
        std::fs::write(r.dir.join("ps"), ps).unwrap();
        write_exec(&r.dir.join("bin/ps"), &format!("#!/bin/sh\ncat {}/ps\n", r.dir.display()));
    }
    let ids: Vec<_> = list_panes().unwrap().into_iter().map(|p| p.id).collect();
    set_stage_hook(None);
    assert_eq!(ids, ["%1"], "the board's own pane showed as an agent");
    // an empty stubpid read names no child, and the walk then checks nothing (#1024)
    assert!(is_lookup_child(&pid), "the walk met no lookup child: {pid:?}");
    std::fs::write(r.dir.join("go"), "").unwrap();
    r.wait_cache();
}

#[test]
fn the_lookup_child_does_not_make_the_boards_own_pane_an_agent() {
    own_pane_stays_no_agent("selfrow", false, false, Hold::Nothing);
}

#[test]
fn nor_does_a_claude_under_the_lookup_child() {
    own_pane_stays_no_agent("selfrow2", true, false, Hold::Nothing);
}

#[test]
fn the_ps_walk_skips_the_lookup_child_too() {
    own_pane_stays_no_agent("selfrow3", true, true, Hold::Nothing);
}

/// #976: the board stores its child only once `spawn` has returned. Held there, a
/// walk on the stub's pid alone meets a child the board does not know yet.
#[test]
fn the_own_pane_check_waits_for_the_board_to_hold_the_stub() {
    own_pane_stays_no_agent("selfrow4", false, false, Hold::Store);
}

/// #976: the stub's `echo` truncates stubpid before it writes. Held empty there, a
/// helper that takes the empty read walks a tree that is not the stub's.
#[test]
fn the_own_pane_check_takes_no_pid_from_an_empty_stubpid() {
    own_pane_stays_no_agent("selfrow5", false, false, Hold::EmptyFile);
}

fn seeded_at(r: &Rig, age: u64, session: &str) {
    r.seed(&format!(
        r#"{{"v":1,"at":{},"by_pid":{{"100:5":{{"session":"{session}","cwd":"/c","kind":"k"}}}},"none":[]}}"#,
        now() - age
    ));
}

/// Polls until `p` is there, failing once [`GONE_BY`] has passed.
fn wait_file(p: &Path) {
    let end = std::time::Instant::now() + GONE_BY;
    while !p.exists() {
        assert!(std::time::Instant::now() < end, "{} never appeared", p.display());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn wait_forks(r: &Rig, n: usize) {
    for _ in 0..2000 {
        if r.forks() >= n {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// How long a killed stub may take to leave /proc. The stubs sleep 60 s, so a
/// stub gone within this was killed, however loaded the box: a deadline, not a
/// count of polls, which a busy scheduler stretches past any window (#564).
const GONE_BY: std::time::Duration = std::time::Duration::from_secs(10);

/// Waits until the stub runs and the board holds its pid, and returns that pid.
/// The stub writes its pid before the board has stored it, and a hangup in
/// that gap finds no child to kill: the old tests raced it under load (#564).
/// An empty or missing stubpid never counts, even while the board holds none (#976).
fn wait_in_flight() -> u32 {
    let stub = Path::new(&std::env::var("HOME").unwrap()).join("stubpid");
    let end = std::time::Instant::now() + GONE_BY;
    loop {
        let raw = std::fs::read_to_string(&stub).ok();
        let pid = raw.as_deref().and_then(|s| s.trim().parse::<u32>().ok());
        if pid.is_some() {
            PID_READS.fetch_add(1, Ordering::SeqCst);
        } else if raw.as_deref() == Some("") {
            EMPTY_READS.fetch_add(1, Ordering::SeqCst);
        }
        if let Some(p) = pid
            && pid == lookup_pid()
        {
            return p;
        }
        let board = lookup_pid();
        assert!(
            std::time::Instant::now() < end,
            "the lookup never started: stubpid {pid:?}, board {board:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Polls until `pid` has left /proc, failing once [`GONE_BY`] has passed.
fn wait_gone(pid: &str) {
    let end = std::time::Instant::now() + GONE_BY;
    while Path::new("/proc").join(pid).exists() {
        assert!(std::time::Instant::now() < end, "{pid} is still running after {GONE_BY:?}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The mechanism: a positive was served for ever. One 70 s old is served, then
/// re-asked, and the changed session id shows on the next fetch.
#[test]
fn a_session_that_changed_under_the_same_pid_is_picked_up() {
    let r = Rig::new("stale", ONE);
    seeded_at(&r, 70, "s-old");
    assert_eq!(r.sess(), "s-old", "served while the call runs");
    for _ in 0..2000 {
        if r.wait_cache()["by_pid"]["100:5"]["session"] == "s-100" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(r.sess(), "s-100");
    assert_eq!(r.forks(), 1);
}

/// `at` is the time of an answer: fetches over a fresh file leave it as it is.
#[test]
fn fetches_never_restamp_the_file() {
    let r = Rig::new("nostamp", ONE);
    seeded_at(&r, 30, "s-seed");
    let before = std::fs::read_to_string(r.cache()).unwrap();
    for _ in 0..5 {
        assert_eq!(r.sess(), "s-seed");
    }
    assert_eq!((std::fs::read_to_string(r.cache()).unwrap(), r.forks()), (before, 0));
}

/// A second case: a stale positive and a stub that fails: asked once, then rests.
#[test]
fn a_failed_refresh_of_a_stale_positive_rests_a_minute() {
    let r = Rig::new("stalefail", "exit 1");
    seeded_at(&r, 70, "s-old");
    r.sess();
    wait_forks(&r, 1);
    for _ in 0..5 {
        assert_eq!(r.sess(), "s-old");
    }
    assert_eq!(r.forks(), 1);
}

/// One-shot `--json` and `--rows`: no thread to come back to, so the call is waited for.
#[test]
fn a_one_shot_read_waits_for_the_lookup() {
    let r = Rig::with("oneshot", &format!("sleep 0.2\n{ONE}"), false);
    assert_eq!(r.sess(), "s-100");
    assert!(r.cache().exists());
}

/// Several panes, one call: what the answer did not name is not asked again.
#[test]
fn a_one_shot_read_asks_once_for_pids_the_answer_lacks() {
    let r = Rig::with("oneshot2", "printf '[]'", false);
    assert_eq!(r.sess(), "");
    assert_eq!(r.sess(), "");
    assert_eq!(r.forks(), 1);
}

/// #294 on the one-shot path: a hung `claude` is killed at the deadline.
#[test]
fn a_one_shot_read_kills_a_hung_lookup() {
    let r = Rig::with("hung", "echo $$ > $HOME/stubpid\nexec $HOME/bin/sleep 60", false);
    assert_eq!(r.sess(), "");
    let pid = std::fs::read_to_string(r.dir.join("stubpid")).unwrap();
    wait_gone(pid.trim());
}

/// The child half of the quit test: a board with a lookup in flight that quits,
/// and then exits as a board does, so no deadline of its own is left to kill the
/// lookup for it.
#[test]
fn quit_child() {
    if std::env::var_os("QUIT_CHILD").is_none() {
        return;
    }
    let board = Background::start();
    list_panes();
    wait_in_flight();
    drop(board);
    std::process::exit(0);
}

/// A board that quits mid-lookup takes the child with it. Quit in a process of its
/// own: in this one the lookup's 2.5 s deadline would kill the stub anyway.
#[test]
fn the_board_quitting_kills_a_lookup_in_flight() {
    let r = Rig::new("quit", "echo $$ > $HOME/stubpid\nexec $HOME/bin/sleep 60");
    let me = std::env::current_exe().unwrap();
    let st = std::process::Command::new(me)
        .args(["--exact", "quit_child", "--test-threads=1"])
        .env("QUIT_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(0), "the child quit");
    let pid = std::fs::read_to_string(r.dir.join("stubpid")).unwrap();
    wait_gone(pid.trim());
}

/// The file is data from disk: a session that is no id never reaches a path.
#[test]
fn a_hostile_session_in_the_file_or_the_answer_is_dropped() {
    let r = Rig::new("hostile", r#"printf '[{"pid":100,"sessionId":"../../x"}]'"#);
    let p = r.dir.join("f.json");
    let at = now();
    let ent = |s: &str| format!(r#""100:5":{{"session":"{s}"}}"#);
    for bad in ["../../x", "a b", "", &"a".repeat(65), "x/y"] {
        std::fs::write(&p, format!(r#"{{"v":1,"at":{at},"by_pid":{{{}}},"none":[]}}"#, ent(bad))).unwrap();
        assert!(read_file(&p, &r.dir.join("proc"), at).unwrap().by_pid.is_empty(), "{bad:?}");
    }
    std::fs::write(
        &p,
        format!(r#"{{"v":1,"at":{at},"by_pid":{{{}}},"none":[]}}"#, ent(&"a-1".repeat(21))),
    )
    .unwrap();
    assert_eq!(read_file(&p, &r.dir.join("proc"), at).unwrap().by_pid.len(), 1);
    r.sess();
    r.wait_cache();
    assert_eq!(r.sess(), "", "the answer's hostile id");
}

/// A FIFO, a symlink and a huge file at the cache path are not read, and the fetch goes on.
#[test]
fn a_fifo_a_symlink_or_a_huge_file_at_the_path_is_not_read() {
    let r = Rig::new("nodes", ONE);
    let (cache, root) = (r.cache(), r.dir.join("proc"));
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    let good = r.dir.join("good.json");
    let body = format!(r#"{{"v":1,"at":{},"by_pid":{{"100:5":{{"session":"s-seed"}}}},"none":[]}}"#, now());
    std::fs::write(&good, &body).unwrap();
    assert!(read_file(&good, &root, now()).is_some(), "the control reads");
    std::os::unix::fs::symlink(&good, &cache).unwrap();
    assert_eq!(read_file(&cache, &root, now()), None, "symlink");
    std::fs::remove_file(&cache).unwrap();
    std::fs::write(&cache, format!("{body}{}", " ".repeat(2 << 20))).unwrap();
    assert_eq!(read_file(&cache, &root, now()), None, "2 MiB");
    std::fs::remove_file(&cache).unwrap();
    let c = std::ffi::CString::new(cache.to_str().unwrap()).unwrap();
    // SAFETY: mkfifo takes a path and a mode.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert_eq!(read_file(&cache, &root, now()), None, "fifo");
    assert_eq!(r.sess(), "", "the fetch over a fifo");
    // the lookup that fetch started renames its answer over the fifo: wait for it, as a
    // read of the path would hang on a fifo still there (#969). Nothing in the tree
    // pins this wait: the drop's order alone closes the window. It stays as a second
    // barrier, by ruling on #969.
    assert!(lookups_end_within(GONE_BY), "the lookup never finished");
}

/// By pid, not by name: a real `claude` beside the lookup child in the board's
/// pane is still an agent, so the pane stays a row.
#[test]
fn a_real_claude_beside_the_lookup_child_keeps_the_pane_an_agent() {
    let r = Rig::new(
        "beside",
        &format!("echo $$ > $HOME/stubpid\nwhile [ ! -f $HOME/go ]; do sleep 0.02; done\n{ONE}"),
    );
    let row = ["%2", "w:1", "board", "bash", "-", "-", "-", "-", "/w", "200", "-", "-", "@2"].join("\u{1f}");
    let rows = std::fs::read_to_string(r.dir.join("rows")).unwrap() + &row + "\n";
    std::fs::write(r.dir.join("rows"), rows).unwrap();
    r.proc_dir("200", "bash", "");
    r.sess();
    // as in the own-pane check: walk only once the board holds the stub (#976)
    let pid = wait_in_flight().to_string();
    std::fs::write(r.dir.join("proc/200/task/200/children"), format!("{pid} 777")).unwrap();
    r.proc_dir(&pid, "claude", "");
    r.proc_dir("777", "claude", "");
    let ids: Vec<_> = list_panes().unwrap().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, ["%1", "%2"], "a real claude in the board's pane was hidden");
    std::fs::write(r.dir.join("go"), "").unwrap();
    r.wait_cache();
}

/// On the `ps` path a snapshot can be walked after the call ended: its child is
/// still skipped for a few seconds.
#[test]
fn the_ps_walk_still_skips_a_lookup_child_that_has_just_ended() {
    let r = Rig::new("ghost", &format!("echo $$ > $HOME/stubpid\n{ONE}"));
    let row = ["%2", "w:1", "board", "bash", "-", "-", "-", "-", "/w", "200", "-", "-", "@2"].join("\u{1f}");
    let rows = std::fs::read_to_string(r.dir.join("rows")).unwrap() + &row + "\n";
    std::fs::write(r.dir.join("rows"), rows).unwrap();
    r.sess();
    r.wait_cache();
    let pid = std::fs::read_to_string(r.dir.join("stubpid")).unwrap();
    std::fs::remove_dir_all(r.dir.join("proc/self")).unwrap();
    std::fs::write(r.dir.join("ps"), format!("100 1 claude\n200 1 bash\n{} 200 claude\n", pid.trim())).unwrap();
    write_exec(&r.dir.join("bin/ps"), &format!("#!/bin/sh\ncat {}/ps\n", r.dir.display()));
    let ids: Vec<_> = list_panes().unwrap().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, ["%1"]);
}

/// `at` is the time of an answer: a stub that fails leaves a 70 s-old file as it was.
#[test]
fn a_failed_call_does_not_move_at() {
    let r = Rig::new("atfail", "exit 1");
    seeded_at(&r, 70, "s-old");
    let before = std::fs::read_to_string(r.cache()).unwrap();
    r.sess();
    wait_forks(&r, 1);
    // a write would come within ms of the stub's exit
    for _ in 0..200 {
        assert_eq!(std::fs::read_to_string(r.cache()).unwrap(), before);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// A second case: an answer that is not JSON.
#[test]
fn a_reply_that_is_no_json_array_does_not_move_at() {
    let r = Rig::new("atjunk", "echo not-json");
    seeded_at(&r, 70, "s-old");
    let before = std::fs::read_to_string(r.cache()).unwrap();
    r.sess();
    wait_forks(&r, 1);
    for _ in 0..200 {
        assert_eq!(std::fs::read_to_string(r.cache()).unwrap(), before);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// A one-shot read asks for a pid the file does not list, however fresh the file.
#[test]
fn a_one_shot_asks_for_a_pane_a_fresh_file_lacks() {
    let r = Rig::with("oneshot3", ONE, false);
    r.seed(&format!(r#"{{"v":1,"at":{},"by_pid":{{}},"none":[]}}"#, now()));
    assert_eq!(r.sess(), "s-100");
    assert_eq!(r.forks(), 1);
}

/// The child half of the SIGHUP test: a board with a lookup in flight, hung up on.
#[test]
fn hup_child() {
    if std::env::var_os("HUP_CHILD").is_none() {
        return;
    }
    let _board = Background::start();
    let _push = cockpit_tui::push::Push::start();
    list_panes();
    wait_in_flight();
    // SAFETY: raises the signal the board's handler is installed for.
    unsafe { libc::raise(libc::SIGHUP) };
    std::thread::sleep(std::time::Duration::from_secs(5));
    std::process::exit(3);
}

/// The child half of the start-window test: the board is hung up on right after
/// the fork, before the child's pid is in `CHILD`, so `kill_lookup` finds none.
#[test]
fn spawned_hup_child() {
    if std::env::var_os("SPAWNED_HUP_CHILD").is_none() {
        return;
    }
    fn hang_up(at: Stage, pid: u32) {
        if at == Stage::Spawned {
            let home = std::env::var("HOME").unwrap();
            std::fs::write(Path::new(&home).join("spawnedpid"), pid.to_string()).unwrap();
            // SAFETY: raises the signal the board's handler is installed for.
            unsafe { libc::raise(libc::SIGHUP) };
        }
    }
    let _board = Background::start();
    let _push = cockpit_tui::push::Push::start();
    set_stage_hook(Some(hang_up));
    list_panes();
    std::thread::sleep(std::time::Duration::from_secs(5));
    std::process::exit(3);
}

/// #607: a hang-up between the fork and the store in `CHILD` still takes the child.
#[test]
fn a_hang_up_before_the_child_is_stored_still_takes_it() {
    let r = Rig::new("spawned", "exec $HOME/bin/sleep 60");
    let me = std::env::current_exe().unwrap();
    let st = std::process::Command::new(me)
        .args(["--exact", "spawned_hup_child", "--test-threads=1"])
        .env("SPAWNED_HUP_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(0), "the handler's _exit(0), not the child's own exit");
    let pid = std::fs::read_to_string(r.dir.join("spawnedpid")).unwrap();
    wait_gone(pid.trim());
}

/// #607: the lookup's end clears `CHILD` before it reaps, so a `kill_lookup` can
/// never name a pid the kernel may have handed on.
#[test]
fn the_end_clears_the_child_before_the_reap() {
    static SEEN: std::sync::Mutex<Vec<(u32, Option<u32>)>> = std::sync::Mutex::new(Vec::new());
    fn record(at: Stage, pid: u32) {
        if at == Stage::Reaping {
            SEEN.lock().unwrap().push((pid, lookup_pid()));
        }
    }
    let r = Rig::with("reaping", ONE, false);
    set_stage_hook(Some(record));
    r.sess();
    set_stage_hook(None);
    let seen = SEEN.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "one lookup: {seen:?}");
    assert_eq!(seen[0].1, None, "CHILD at the reap of {}", seen[0].0);
}

/// The child half of the failed-spawn test: no thread can start (RLIMIT_NPROC 0),
/// so the board's lookup spawn fails, and the count must come back to 0.
#[test]
fn failed_spawn_child() {
    if std::env::var_os("FAILED_SPAWN_CHILD").is_none() {
        return;
    }
    let _board = Background::start();
    resolve(&[("100".into(), "claude".into())]);
    let mut lim = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: get/setrlimit read and write one struct; this child process only lowers
    // its own soft limit, under the hard one it has.
    unsafe {
        libc::getrlimit(libc::RLIMIT_NPROC, &mut lim);
        lim.rlim_cur = 0;
        assert_eq!(libc::setrlimit(libc::RLIMIT_NPROC, &lim), 0, "lower RLIMIT_NPROC");
    }
    // root, CAP_SYS_RESOURCE or macOS (NPROC counts processes there): the spawn works
    let got = match std::panic::catch_unwind(flush) {
        Ok(()) => "skip".to_string(),
        Err(_) => format!("false {}", lookups()),
    };
    let home = std::env::var("HOME").unwrap();
    std::fs::write(Path::new(&home).join("counted"), got).unwrap();
    std::process::exit(0);
}

/// #616's nit: a lookup spawn that fails takes its count back down, or every
/// later wait for no lookups (the rig's Drop) would wait out its deadline.
#[test]
fn a_failed_lookup_spawn_leaves_no_count() {
    let r = Rig::new("nospawn", ONE);
    let me = std::env::current_exe().unwrap();
    let st = std::process::Command::new(me)
        .args(["--exact", "failed_spawn_child", "--test-threads=1"])
        .env("FAILED_SPAWN_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(0), "the child ran to its end");
    let got = std::fs::read_to_string(r.dir.join("counted")).unwrap();
    if got == "skip" {
        eprintln!("RLIMIT_NPROC did not stop the spawn (root or macOS?) -- skipping");
        return;
    }
    assert_eq!(got, "false 0", "(the spawn failed, lookups left counted)");
}

/// SIGHUP (a `kill-pane`) and SIGTERM end the board with `_exit`: the lookup child goes too.
#[test]
fn a_signalled_board_kills_a_lookup_in_flight() {
    let r = Rig::new("sighup", "echo $$ > $HOME/stubpid\nexec $HOME/bin/sleep 60");
    let me = std::env::current_exe().unwrap();
    let st = std::process::Command::new(me)
        .args(["--exact", "hup_child", "--test-threads=1"])
        .env("HUP_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(0), "the handler's _exit(0), not the child's own exit");
    let pid = std::fs::read_to_string(r.dir.join("stubpid")).unwrap();
    wait_gone(pid.trim());
}
