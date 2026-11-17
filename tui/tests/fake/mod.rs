//! Fake commands for the tests to exec, written so a fork cannot make that exec
//! fail ETXTBSY.
//!
//! A child forked by any thread holds a copy of every fd its parent had open
//! until it execs, and the kernel refuses to exec a file that has a writer. A
//! fake this process wrote itself is busy for as long as any such child lives.
//! A rename does not help: the new name points at the same file, and the
//! child's fd still writes to it. So the bytes go to a side file, a `cp` child
//! writes the file that runs, and this process never opens that file for writing.
//!
//! Also the shared [`Scratch`] guard (#544), the one environment a test
//! binary can take for itself ([`fixed_env`], #957), the `claude` a rig
//! answers a lookup with ([`rig_path`], #972), the rigs' private tmux servers
//! ([`Server`], #1066), the waits for a pane's start-up ([`held_pane`],
//! [`named_pane`] and [`settle`], #1056, #1184), the environment lock every check
//! reads under ([`env_lock`], #1110), and the canary a row holds set under it
//! beside a child's start ([`beside_a_held_canary`], #1142).
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
#[allow(unused_imports)]
pub use test_env::{EnvHeld, env_lock, env_poisoned, remove_var, set_var};

/// The lock's token as a rig keeps it: its own, taken for the rig's life, or lent by a
/// holder that outlives the rig, for one built inside another (#1132).
#[allow(dead_code)]
pub enum Held<'a> {
    Own(EnvHeld),
    Lent(&'a EnvHeld),
}

#[allow(dead_code)]
impl Held<'_> {
    /// The token, for [`set_var`] and [`remove_var`].
    pub fn token(&self) -> &EnvHeld {
        match self {
            Held::Own(h) => h,
            Held::Lent(h) => h,
        }
    }
}

fn beside(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

/// `body` as an executable at `path`, replacing whatever was there.
pub fn write_exec(path: &Path, body: &str) {
    let (src, new) = (beside(path, ".src"), beside(path, ".new"));
    std::fs::write(&src, body).unwrap();
    let st = std::process::Command::new("cp").arg(&src).arg(&new).status().unwrap();
    assert!(st.success(), "cp {} {}: {st}", src.display(), new.display());
    std::fs::remove_file(&src).unwrap();
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A new file under the old name: a fake still running keeps the one it had.
    std::fs::rename(&new, path).unwrap();
}

/// A test's own directory, removed when the guard drops: on a pass and on a
/// failing assert's unwind alike, so a run leaves nothing in the temp dir
/// (#543, #544). A crate that loads this module only for `write_exec` leaves it
/// unused, hence the allows.
#[allow(dead_code)]
pub struct Scratch(pub PathBuf);

impl Scratch {
    /// `dir`, emptied of a killed run's leftovers and created.
    #[allow(dead_code)]
    pub fn fresh(dir: PathBuf) -> Self {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// HOME and XDG_CONFIG_HOME for a test binary that calls [`fixed_env`]. Nothing
/// is ever made at it: sysfs takes no entry from a user, and a box without /sys
/// has no such folder.
#[allow(dead_code)]
pub const NO_HOME: &str = "/sys/absent-948";

/// One environment for the whole test binary, set on the first call and never
/// restored. A prompt override under the developer's own config folder and
/// their HOME changed what a write sends (#948), an exported
/// COCKPIT_PANEL_MODEL changed the model and the frames that name it (#953),
/// and other binaries read the same three (#957). Each binary is a process of
/// its own, so each calls this from the helper its tests reach the product
/// through. A test that reaches the product without that helper, or reads one
/// of the three itself, calls this first. Not for a binary whose tests set one
/// of the three and put the old value back: a guard taken before the first
/// call would put the developer's value back after it. A crate that loads this
/// module and takes no fixed environment leaves these unused, hence the allows.
/// Writes under [`env_lock`]'s lock, which it takes after its check, so a caller
/// that holds the lock calls this before it takes it: the check's panic says so
/// (#1132).
#[allow(dead_code)]
pub fn fixed_env() {
    // the product's plain tmux reads the caller's server where the runner did
    // not clear its session: refused, in every test that comes through here (#1066)
    checked_tmux();
    // the lock outside the Once, so every caller takes the two in one order: a holder
    // waiting in call_once for a caller that waits inside it for the lock is a hang
    let held = env_lock();
    static SET: Once = Once::new();
    SET.call_once(|| {
        // SAFETY: under the lock, so no check reads meanwhile. std's getenv and
        // setenv share one lock, and a spawn holds it while the child takes the
        // environment. Every read a test thread makes there is a Rust read; the
        // readers that are not Rust are child processes, which get their copy at
        // the spawn. A C function called in the test process that reads the
        // environment (a local time call, a getenv in a linked library) would
        // break this: no binary that calls this makes one today.
        unsafe {
            set_var(&held, "HOME", NO_HOME);
            set_var(&held, "XDG_CONFIG_HOME", NO_HOME);
            remove_var(&held, "COCKPIT_PANEL_MODEL");
        }
    });
}

/// What [`assert_fixed_env`] writes to stderr once its three checks held, for
/// [`fixed_env_in_a_child`].
#[allow(dead_code)]
pub const ENV_CHECKED: &str = "the-fixed-environment-held";

/// The three values [`fixed_env`] leaves, then the [`ENV_CHECKED`] line. A
/// binary's own test calls this right after the helper that calls `fixed_env`.
#[allow(dead_code)]
pub fn assert_fixed_env() {
    let now = |k: &str| std::env::var(k).ok();
    assert_eq!(now("HOME").as_deref(), Some(NO_HOME));
    assert_eq!(now("XDG_CONFIG_HOME").as_deref(), Some(NO_HOME));
    assert_eq!(now("COCKPIT_PANEL_MODEL"), None);
    eprintln!("{ENV_CHECKED}");
}

/// `test` of this binary, alone, in a process started with all three variables
/// set to other values, with `more` as further arguments of the harness.
#[allow(dead_code)]
fn rerun(test: &str, more: &[&str]) -> std::process::Output {
    let set = [("HOME", "/sys/other-948"), ("XDG_CONFIG_HOME", "/sys/other-948")];
    std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test])
        .args(more)
        .envs(set)
        .env("COCKPIT_PANEL_MODEL", "opus")
        .output()
        .unwrap()
}

/// What a failed rerun says: the test asked for, how the child ended and all
/// it wrote, so a child that ran no test is told apart from one that failed.
#[allow(dead_code)]
fn told(test: &str, out: &std::process::Output) -> String {
    let (o, e) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    format!("{test} in a child: {}\nstdout:\n{o}\nstderr:\n{e}", out.status)
}

/// `test`, this binary's test that ends in [`assert_fixed_env`], in a process
/// started with all three variables set. On a box that sets none of them, CI
/// included, a COCKPIT_PANEL_MODEL left in place would pass that test: here it
/// does not (#953). The exit status alone would pass a name that matches no
/// test, so the marker line is asked for too. A test must not pass its own
/// name: its child would start a child of its own, without end.
#[allow(dead_code)]
pub fn fixed_env_in_a_child(test: &str) {
    let out = rerun(test, &["--nocapture"]);
    let said = String::from_utf8_lossy(&out.stderr);
    let held = said.lines().any(|l| l == ENV_CHECKED);
    assert!(out.status.success() && held, "{}", told(test, &out));
}

/// `test`, a test of this binary that reads one of the three itself and so
/// calls [`fixed_env`] before that read, in a process started with all three
/// set to other values. Run alone there, it compares what it read with what
/// the product reads after the helper's call, and the two differ once its own
/// call is gone. It writes no marker, so the line the harness prints for its
/// pass is asked for beside the exit status. Not its own name, as above.
#[allow(dead_code)]
pub fn passes_in_a_child(test: &str) {
    let out = rerun(test, &[]);
    let (said, passed) = (String::from_utf8_lossy(&out.stdout), format!("test {test} ... ok"));
    let ran = said.lines().any(|l| l == passed);
    assert!(out.status.success() && ran, "{}", told(test, &out));
}

/// The `claude` a rig holds unless it wrote its own: no output and exit 127,
/// the status a shell gives for a command that is not there. The product reads
/// that as no answer, so a lookup goes as it does where claude is not
/// installed, the state CI runs in. It must never print: a JSON array on its
/// stdout would be an answer, and the rig's panes would take sessions from it.
/// Each start appends one line to `claude.started` beside it, so a rig that
/// looks up can hold that its own `claude` was the one started.
#[allow(dead_code)]
const NO_CLAUDE: &str = "#!/bin/sh\necho x >> \"$0.started\"\nexit 127\n";

/// What a spawn searches when PATH is not set at all: glibc's `_CS_PATH`, as
/// `getconf PATH` prints it.
#[allow(dead_code)]
const DEFAULT_PATH: &str = "/bin:/usr/bin";

/// The PATH for a rig whose folder of fakes is `bin`: `bin`, then the PATH of
/// this moment, or [`DEFAULT_PATH`] where none is set, so the rig still finds
/// what a spawn found before it. `bin` is made if it is not there and gets a `claude`
/// ([`NO_CLAUDE`]) unless the rig put one there already. The product looks
/// agents up by starting `claude agents --json`, found by PATH, so a rig that
/// reaches a lookup with no `claude` of its own starts the developer's (#972).
/// This sets nothing: the caller sets PATH where it sets the rest of its
/// environment, under its own lock, and then calls [`assert_claude_is_the_rigs`].
#[allow(dead_code)]
pub fn rig_path(bin: &Path) -> String {
    assert!(bin.is_absolute(), "bin must be absolute: {}", bin.display());
    // a `:` would split it into two entries of the PATH it opens
    let Some(first) = bin.to_str().filter(|b| !b.contains(':')) else {
        panic!("bin must hold no ':' and be UTF-8: {}", bin.display());
    };
    std::fs::create_dir_all(bin).unwrap();
    if !bin.join("claude").exists() {
        write_exec(&bin.join("claude"), NO_CLAUDE);
    }
    match std::env::var("PATH") {
        Ok(old) if !old.is_empty() => format!("{first}:{old}"),
        // an empty PATH names the cwd alone, and so would a trailing `:`
        Ok(_) => first.to_string(),
        Err(std::env::VarError::NotPresent) => format!("{first}:{DEFAULT_PATH}"),
        Err(e) => panic!("PATH: {e}"),
    }
}

/// `c` with each of `signals` at its default action and unblocked in its child,
/// set between the fork and the exec. A process starts with its parent's
/// ignored signals and signal mask: a suite started with `&` from a script
/// ignores INT and QUIT, one under nohup HUP, and bash keeps a mask that dash
/// clears. A shell cannot trap a signal ignored when it started, and a handler
/// never runs for a blocked one. A row that signals a child, or has one signal
/// itself, measures that child and not how the suite was started (#1044, #1061).
/// KILL and STOP can be neither ignored nor blocked.
#[allow(dead_code)]
pub fn at_default(c: &mut Command, signals: &'static [libc::c_int]) {
    use std::os::unix::process::CommandExt;
    let reset = move || {
        // SAFETY: signal, sigemptyset, sigaddset and sigprocmask are
        // async-signal-safe, and nothing here allocates.
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for &s in signals {
                libc::signal(s, libc::SIG_DFL);
                libc::sigaddset(&mut set, s);
            }
            libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        }
        Ok(())
    };
    // SAFETY: `reset` makes only async-signal-safe calls and allocates nothing.
    unsafe { c.pre_exec(reset) };
}

/// How long [`rows_pass_handed`] gives its child: past the rows' own bounds, so
/// theirs fire first and a row still ends what it started. The longest is
/// claude_guard.rs's signal row, whose waits are checked against this when
/// that file builds. A bound at or under a row's own killed that child
/// before it could kill its runner, and left the runner running (#1061). A
/// child alive after this is killed, so a hung harness cannot hold the suite for
/// good.
#[allow(dead_code)]
pub const HANDED_FOR: std::time::Duration = std::time::Duration::from_secs(300);

/// `rows` of this binary, matched exactly, in this binary started again with
/// each of `signals` ignored and blocked: the worst state a suite can hand
/// down. Each row that signals a child holds there only by its own
/// [`at_default`] (#1061). The run must name every row passed, since a renamed
/// row runs none. A row must not pass its own name: its child would start a
/// child of its own, without end. Returns the child's pid, for a row that looks
/// for what the child left.
#[allow(dead_code)]
pub fn rows_pass_handed(rows: &[&str], signals: &'static [libc::c_int]) -> u32 {
    handed(rows, signals, &[], HANDED_FOR).unwrap_or_else(|e| panic!("{e}"))
}

/// The bits of the `field` line, `SigIgn:` or `SigBlk:`, of `pid`'s
/// /proc status: None where /proc has no such process.
#[allow(dead_code)]
fn status_bits(pid: u32, field: &str) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let bits = s.lines().find_map(|l| l.strip_prefix(field))?;
    u64::from_str_radix(bits.trim(), 16).ok()
}

/// [`rows_pass_handed`]'s run, with `env` set for the child and `bound` for its
/// wait: the child's pid, or what went wrong with all the child wrote. Where
/// /proc is, the child's own status must show each of `signals` ignored and
/// blocked, read right after its exec, so a helper that hands nothing down
/// fails. The child writes to files in a folder of its own and never to a pipe,
/// which a red row writing more than a pipe holds would fill and then wait on
/// for the whole bound (#1061). The spawn takes [`env_lock`]'s lock and the wait
/// does not, so the child starts with no write a sibling holds under it (#1142).
/// Under that lock already, [`handed_held`]: the rule for a holder is test-env's.
#[allow(dead_code)]
pub fn handed(
    rows: &[&str],
    signals: &'static [libc::c_int],
    // variables set for the child alone
    env: &[(&str, &str)],
    bound: std::time::Duration,
) -> Result<u32, String> {
    handed_with(rows, signals, env, bound, |c| {
        let spawned = {
            let env = test_env::for_check("handed");
            spawn_under(&env, c)
        };
        spawned.unwrap()
    })
}

/// [`handed`] for a caller that holds [`env_lock`]'s lock, as `held` proves.
#[allow(dead_code)]
pub fn handed_held(
    held: &EnvHeld,
    rows: &[&str],
    signals: &'static [libc::c_int],
    env: &[(&str, &str)],
    bound: std::time::Duration,
) -> Result<u32, String> {
    handed_with(rows, signals, env, bound, |c| spawn_held(held, c).unwrap())
}

/// What shows that this thread holds [`env_lock`]'s lock at a spawn: the guard a
/// guard-less start takes for it, or a holder's token.
#[allow(dead_code)]
trait Holds {}
impl Holds for std::sync::MutexGuard<'static, ()> {}
impl Holds for EnvHeld {}

/// `c` spawned while `_lock` is alive, so the child starts with no write a sibling holds
/// under the lock (#1142). The one spawn of handed, handed_held, handed_start, run_by_hand,
/// run_by_hand_held, [`spawn_held`] and [`spawn`], so a spawn there after the
/// guard was let go does not build: the guard is gone before it can be lent. The file's
/// other starts of a child stay outside the lock (#1206): each sets what its child reads
/// itself, or lives only in binaries with no transient writer, one that sets a value
/// under the lock and puts it back later. And `write_exec`'s `cp`, like panel_env's
/// ForkStorm threads, runs under a holder or beside one, where a take here would panic
/// on the holder's thread or wait on it for ever; `Server::stop` and `StandIn`'s Drop
/// run in a drop, where that panic would abort an unwind.
#[allow(dead_code)]
fn spawn_under(_lock: &impl Holds, c: &mut Command) -> std::io::Result<std::process::Child> {
    c.spawn()
}

/// `c` spawned by a caller that holds [`env_lock`]'s lock, as `held` proves: a row that
/// starts a child of its own while its rig lives (#1206). It takes an [`EnvHeld`] alone,
/// which only [`env_lock`] makes, so a token let go before the spawn does not build. The
/// generic [`spawn_under`] stays private: public, it would take any
/// `MutexGuard<'static, ()>`, one a test file makes from a static `Mutex` of its own as
/// well as this lock's. The wait is the caller's.
#[allow(dead_code)]
pub fn spawn_held(held: &EnvHeld, c: &mut Command) -> std::io::Result<std::process::Child> {
    spawn_under(held, c)
}

/// `c` spawned under [`env_lock`]'s lock, taken for the spawn alone, so the child starts
/// with no write a sibling holds under it, and the wait, the caller's, holds nothing
/// (#1206). For a row that starts a child of its own and holds no rig; on a thread that
/// holds the lock already it panics, naming [`spawn_held`].
#[allow(dead_code)]
pub fn spawn(c: &mut Command) -> std::io::Result<std::process::Child> {
    let env = test_env::for_check("spawn");
    spawn_under(&env, c)
}

/// Set by a row that starts a child beside a held canary, for that child alone: the file
/// it writes the moment it starts (#1142).
#[allow(dead_code)]
pub const STARTED: &str = "COCKPIT_TEST_ENV_LOCK_STARTED";

/// The variable [`beside_a_held_canary`] holds set under the lock, as a sibling holds
/// its write.
#[allow(dead_code)]
pub const CANARY: &str = "COCKPIT_TEST_ENV_LOCK_CANARY";

/// How long [`beside_a_held_canary`] waits for its child to start under the canary:
/// 1000 polls of 5 ms. With the fix the child never starts while the lock is held, so
/// every green run waits this out once.
#[allow(dead_code)]
pub const POLLS: usize = 1000;

/// The child half of a row that starts a child beside a held canary, run as a row of
/// its own binary: writes the file [`STARTED`] names, then finds [`CANARY`] unset.
/// Where `STARTED` is unset it is no child and does nothing.
#[allow(dead_code)]
pub fn sees_no_canary() {
    let Some(started) = std::env::var_os(STARTED) else {
        return;
    };
    std::fs::write(started, "").unwrap();
    assert_eq!(std::env::var_os(CANARY), None, "the child took a write held under the lock");
}

/// `start` on a thread of its own while this thread holds the lock with [`CANARY`] set,
/// as a sibling holds its write: whether the child wrote `started` within [`POLLS`],
/// then, once the canary is removed and the lock let go, what `start` returned. Never a
/// fixed sleep: the poll ends at the child's start or at its bound.
#[allow(dead_code)]
pub fn beside_a_held_canary<F>(started: &Path, start: F) -> (bool, Result<(), String>)
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    let held = env_lock();
    // SAFETY: the lock is held, and every write of this binary takes it.
    unsafe { set_var(&held, CANARY, "/sibling-1142") };
    let t = std::thread::spawn(start);
    let mut began = false;
    for _ in 0..POLLS {
        if started.exists() {
            began = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // SAFETY: as above, still under the lock.
    unsafe { remove_var(&held, CANARY) };
    drop(held);
    let ran = t
        .join()
        .unwrap_or_else(|e| Err(e.downcast_ref::<String>().cloned().unwrap_or_default()));
    (began, ran)
}

/// A child [`handed`] started and has not yet read the end of: its pid, the files it
/// writes to, and why it fails where it was handed less than asked.
#[allow(dead_code)]
pub struct Handed {
    child: std::process::Child,
    pid: u32,
    what: String,
    rows: usize,
    out: PathBuf,
    err: PathBuf,
    unhanded: Option<String>,
    // last, so the folder goes after the child's end is read
    _dir: Scratch,
}

/// [`handed`]'s child, started as it starts it, for a row that ends it itself: by
/// [`Handed::end_at_bound`], the bound's own end, without waiting the bound out (#1205).
#[allow(dead_code)]
pub fn handed_start(rows: &[&str], signals: &'static [libc::c_int], env: &[(&str, &str)]) -> Handed {
    start_handed(rows, signals, env, |c| {
        let spawned = {
            let env = test_env::for_check("handed");
            spawn_under(&env, c)
        };
        spawned.unwrap()
    })
}

/// [`handed`]'s start, its child started by `spawn`.
#[allow(dead_code)]
fn start_handed(
    rows: &[&str],
    signals: &'static [libc::c_int],
    env: &[(&str, &str)],
    spawn: impl FnOnce(&mut Command) -> std::process::Child,
) -> Handed {
    use std::os::unix::process::CommandExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, Ordering::SeqCst);
    let me = std::process::id();
    let dir = Scratch::fresh(std::env::temp_dir().join(format!("cockpit-handed-{me}-{run}")));
    let (out, err) = (dir.join("out"), dir.join("err"));
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.arg("--exact").args(rows).envs(env.iter().copied());
    let file = |p: &Path| std::fs::File::create(p).unwrap();
    c.stdin(std::process::Stdio::null()).stdout(file(&out)).stderr(file(&err));
    let hand_down = move || {
        // SAFETY: as in `at_default`, the calls here are async-signal-safe.
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for &s in signals {
                libc::signal(s, libc::SIG_IGN);
                libc::sigaddset(&mut set, s);
            }
            libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        }
        Ok(())
    };
    // SAFETY: `hand_down` makes only async-signal-safe calls and allocates nothing.
    unsafe { c.pre_exec(hand_down) };
    let child = spawn(&mut c);
    let pid = child.id();
    let want = signals.iter().fold(0u64, |m, &s| m | 1 << (s - 1));
    let got = (status_bits(pid, "SigIgn:"), status_bits(pid, "SigBlk:"));
    let has = |bits: Option<u64>| bits.is_some_and(|b| b & want == want);
    // a child handed less runs on to its end: killed now, it would leave what it began
    let unhanded = Path::new("/proc/self").is_dir() && !(has(got.0) && has(got.1));
    let why = || format!("the child was not handed {want:#x} ignored and blocked: {got:x?}");
    let unhanded = unhanded.then(why);
    let what = rows.join(" ");
    Handed {
        child,
        pid,
        what,
        rows: rows.len(),
        out,
        err,
        unhanded,
        _dir: dir,
    }
}

#[allow(dead_code)]
impl Handed {
    /// The child's pid.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The run's report: how it ended, then all it wrote.
    fn said(&self, status: &str) -> String {
        let read = |p: &Path| std::fs::read(p).unwrap_or_default();
        let read = |p: &Path| String::from_utf8_lossy(&read(p)).into_owned();
        let (what, out, err) = (&self.what, read(&self.out), read(&self.err));
        format!("{what} in a child: {status}\nstdout:\n{out}\nstderr:\n{err}")
    }

    /// `None` while the child runs; once it has ended, reaped, the run's result:
    /// its pid where it passed every row it was handed.
    fn ended(&mut self) -> Option<Result<u32, String>> {
        let status = self.child.try_wait().unwrap()?;
        let passed = std::fs::read_to_string(&self.out).unwrap_or_default();
        let all = passed.contains(&format!(" {} passed; 0 failed", self.rows));
        if let Some(why) = self.unhanded.take() {
            return Some(Err(why));
        }
        Some(if status.success() && all {
            Ok(self.pid)
        } else {
            Err(self.said(&status.to_string()))
        })
    }

    /// The end at `bound`: the child killed and reaped, and the run's report.
    pub fn end_at_bound(&mut self, bound: std::time::Duration) -> String {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
        self.said(&format!("had not ended after {bound:?} and was killed"))
    }
}

/// [`handed`]'s body, its child started by `spawn`.
#[allow(dead_code)]
fn handed_with(
    rows: &[&str],
    signals: &'static [libc::c_int],
    env: &[(&str, &str)],
    bound: std::time::Duration,
    spawn: impl FnOnce(&mut Command) -> std::process::Child,
) -> Result<u32, String> {
    let mut h = start_handed(rows, signals, env, spawn);
    let step = std::time::Duration::from_millis(20);
    for tries in 0.. {
        if let Some(run) = h.ended() {
            return run;
        }
        if tries == bound.as_millis() / step.as_millis() {
            return Err(h.end_at_bound(bound));
        }
        std::thread::sleep(step);
    }
    unreachable!()
}

/// Whether a spawn by this user could start `p`: a file the kernel lets it
/// execute, which a mode without the owner's bit or a noexec mount does not.
#[allow(dead_code)]
fn runs(p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a NUL-terminated path, and access only reads it.
    p.is_file() && unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0
}

/// The first `claude` a spawn would find on `path`, the value of a PATH: `Ok`
/// with it when it lies under `dir`, else `Err` with it, or with `None` when
/// no entry holds one. It reads and never runs what it finds, so checking a
/// rig that forgot [`rig_path`] starts nothing.
///
/// The walk follows a spawn's for a rig this module builds, and no further
/// (#975). An empty entry is the cwd. A `claude` that is no file, or that this
/// user may not execute (its mode, a noexec mount), is passed over. The one
/// found is the rig's by where its name leads, not by its spelling: a link in
/// the rig's bin to a `claude` outside and an entry `<rig>/../out` are not the
/// rig's. One case is left out: a `claude` whose interpreter is missing. A
/// spawn passes over it and starts the next one, and this walk stops at it and
/// says `Ok`. [`write_exec`] with a `#!/bin/sh` body does not make one.
#[allow(dead_code)]
pub fn rigs_claude(path: &std::ffi::OsStr, dir: &Path) -> Result<PathBuf, Option<PathBuf>> {
    assert!(dir.is_absolute(), "dir must be absolute: {}", dir.display());
    let cwd = |e: PathBuf| if e.as_os_str().is_empty() { PathBuf::from(".") } else { e };
    let real = |p: &Path| p.canonicalize().ok();
    let inside = |p: &Path| real(p).zip(real(dir)).is_some_and(|(p, d)| p.starts_with(d));
    match std::env::split_paths(path).map(|e| cwd(e).join("claude")).find(|p| runs(p)) {
        Some(found) if inside(&found) => Ok(found),
        other => Err(other),
    }
}

/// For a rig, once its environment is set: the `claude` a lookup would start
/// now is one under `dir`, the rig's own folder. It fails where no `claude` is
/// on PATH at all too, so it holds on a box without one as on a developer's.
#[allow(dead_code)]
pub fn assert_claude_is_the_rigs(dir: &Path) {
    let path = std::env::var_os("PATH").expect("PATH is not set");
    if let Err(found) = rigs_claude(&path, dir) {
        let dir = dir.display();
        panic!("the first claude on PATH is {found:?}, not one under {dir} (#972)");
    }
}

// --- the rigs' private tmux servers (#1066) ---

/// The variables of a caller's tmux session: the server and pane a plain
/// `tmux` reaches, and the live plugin's folder.
#[allow(dead_code)]
const SESSION: [&str; 3] = ["TMUX", "TMUX_PANE", "TMUX_COCKPIT_DIR"];

/// What a refusal hands the reader to paste: a folder for TMUX_TMPDIR and what
/// the runner clears, ahead of the command.
#[allow(dead_code)]
const CLEARED: &str = "d=$(mktemp -d) && env -u TMUX -u TMUX_PANE -u TMUX_COCKPIT_DIR -u COCKPIT_LOG \
                       -u XDG_CACHE_HOME -u XDG_STATE_HOME TMUX_TMPDIR=\"$d\"";

/// The message a check refuses with, three lines: what is set or unset and what
/// that would do, the gate and a bare cargo run that clear it, and this binary
/// alone. cargo's runner clears these for a test binary on the six targets
/// .cargo/config.toml names (#1066), and clears nothing with
/// COCKPIT_TEST_CLAUDE_GUARD=0 or on another target (#1090), so a run there
/// meets this from any shell: TMUX_TMPDIR is unset in one outside tmux too.
#[allow(dead_code)]
fn refusal(what: &str, why: &[String], risk: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    format!(
        "{what} refused (#1066): {}. {risk}\n\
         Run make tui-test from the checkout: it clears these on the six targets \
         .cargo/config.toml names (another: #1090). Or, in tui/: {CLEARED} cargo test\n\
         This binary alone: {CLEARED} {}",
        why.join(", "),
        sh_quoted(&exe.to_string_lossy())
    )
}

/// `s` as one word for sh, in single quotes, each `'` in it closed, escaped and
/// reopened: a checkout's path may hold a space (#997) or a quote.
#[allow(dead_code)]
fn sh_quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The variables of `vars` that are set, as one reason, or none.
#[allow(dead_code)]
fn still_set(vars: &[&str]) -> Vec<String> {
    let set: Vec<&str> = vars.iter().copied().filter(|v| std::env::var_os(v).is_some()).collect();
    if set.is_empty() {
        vec![]
    } else {
        vec![format!("{} set", set.join(", "))]
    }
}

/// The line [`checked_tmux`] refuses with, or `None` where this binary may run
/// tmux: none of [`SESSION`] set, and TMUX_TMPDIR an existing folder. Read under
/// [`env_lock`]'s lock, by the checks alone.
#[allow(dead_code)]
fn tmux_refusal() -> Option<String> {
    let mut why = still_set(&SESSION);
    let mut risk = vec![];
    if !why.is_empty() {
        risk.push("A plain tmux in a test would reach your own tmux server.");
    }
    let folder = match std::env::var_os("TMUX_TMPDIR").map(PathBuf::from) {
        None => Some("TMUX_TMPDIR not set".to_string()),
        Some(f) if !f.is_dir() => Some(format!("TMUX_TMPDIR {} no folder", f.display())),
        Some(_) => None,
    };
    if let Some(f) = folder {
        why.push(f);
        risk.push("A rig's tmux socket would land beside your own tmux server's.");
    }
    (!why.is_empty()).then(|| refusal("tmux", &why, &risk.join(" ")))
}

/// For a rig that sets HOME and reaches the product's log, agents cache or state:
/// red with [`refusal`]'s line where COCKPIT_LOG, XDG_CACHE_HOME or XDG_STATE_HOME
/// is still set, as the runner unsets all three (#1066). A caller's log took the
/// rig's lines (data.rs:58), a caller's cache folder held the agents cache
/// (data/agents.rs:162), and a caller's state folder the hooks' subagent log
/// (data/subagents.rs:525) and the review folder (data/info.rs:108), each in
/// place of the one under the rig's HOME. Takes [`env_lock`]'s lock for the read
/// and lets it go before it panics, so a sibling's write under that lock is never
/// read as the caller's (#1110). Under that lock already, [`checked_state_held`].
#[allow(dead_code)]
pub fn checked_state() {
    let why = {
        let _read = test_env::for_check("checked_state");
        state_refusal()
    };
    if let Some(why) = why {
        panic!("{why}");
    }
}

/// [`checked_state`] for a caller that holds [`env_lock`]'s lock, as `held` proves.
#[allow(dead_code)]
pub fn checked_state_held(_held: &EnvHeld) {
    if let Some(why) = state_refusal() {
        panic!("{why}");
    }
}

/// The line [`checked_state`] refuses with, or `None`. Read under [`env_lock`]'s lock.
#[allow(dead_code)]
fn state_refusal() -> Option<String> {
    let why = still_set(&["COCKPIT_LOG", "XDG_CACHE_HOME", "XDG_STATE_HOME"]);
    let risk = "A rig would write your own log, agents cache or state, \
                not the ones under its HOME.";
    (!why.is_empty()).then(|| refusal("the caller's state", &why, risk))
}

/// `test` of this binary, alone, in a child started by hand with `set` in its
/// environment and no runner between it and this one: how it ended and all it
/// wrote, for a row that holds a refusal word for word. Not the row's own name.
/// As in [`handed`], the spawn takes [`env_lock`]'s lock and the wait does not
/// (#1142). Under that lock already, [`run_by_hand_held`].
#[allow(dead_code)]
pub fn run_by_hand(test: &str, set: &[(&str, &str)]) -> (Option<i32>, String) {
    let mut c = by_hand(test, set);
    let spawned = {
        let env = test_env::for_check("run_by_hand");
        spawn_under(&env, &mut c)
    };
    said_by_hand(spawned.unwrap())
}

/// [`run_by_hand`] for a caller that holds [`env_lock`]'s lock, as `held` proves.
#[allow(dead_code)]
pub fn run_by_hand_held(held: &EnvHeld, test: &str, set: &[(&str, &str)]) -> (Option<i32>, String) {
    said_by_hand(spawn_held(held, &mut by_hand(test, set)).unwrap())
}

/// [`run_by_hand`]'s command. Its own pipes: a spawn hands the child this
/// process's stdout and stderr unless told otherwise, where `output` makes pipes.
#[allow(dead_code)]
fn by_hand(test: &str, set: &[(&str, &str)]) -> Command {
    use std::process::Stdio;
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", test]).envs(set.iter().copied());
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    c
}

/// How `child`, a [`run_by_hand`] child, ended and all it wrote.
#[allow(dead_code)]
fn said_by_hand(child: std::process::Child) -> (Option<i32>, String) {
    let out = child.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.code(), said)
}

/// What a plain `tmux` got from this binary's tests, every test but `skip`, started by
/// hand with no TMUX_TMPDIR, as on a target with no runner key (#1090) or with the
/// claude guard at 0: one argv per call, from a stub `tmux` first on PATH that exits 1
/// and opens no socket. A plain tmux that gets past the product's own refusal
/// (`data::tmux`, #1089) shows here. Started that way for real, it would reach the
/// default socket in /tmp/tmux-<uid>, beside the caller's server (#1066).
#[allow(dead_code)]
pub fn unchecked_tmux_calls(skip: &str) -> Vec<String> {
    let s = Scratch::fresh(std::env::temp_dir().join(format!("cockpit-unchecked-{}", std::process::id())));
    write_exec(&s.join("tmux"), "#!/bin/sh\necho \"$*\" >> \"$0.calls\"\nexit 1\n");
    let path = format!("{}:{}", s.display(), std::env::var("PATH").unwrap_or_default());
    let mut c = Command::new(std::env::current_exe().unwrap());
    // as a bare start: cargo's COCKPIT_TEST_TMUX_GUARD=1 removed, so a debug build's own
    // keying is what refuses (#1089)
    c.args(["--skip", skip])
        .env_remove("TMUX_TMPDIR")
        .env_remove("COCKPIT_TEST_TMUX_GUARD");
    c.env("PATH", path);
    let out = c.stdin(std::process::Stdio::null()).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("test result:"), "the child ran no tests: {said}");
    let calls = std::fs::read_to_string(s.join("tmux.calls")).unwrap_or_default();
    calls.lines().map(String::from).collect()
}

/// This binary's socket folder, TMUX_TMPDIR, which the runner made for it with
/// TMUX, TMUX_PANE and TMUX_COCKPIT_DIR cleared (#1066). Red with
/// [`tmux_refusal`]'s line before any tmux runs where that does not hold: from
/// inside a tmux session a plain `tmux` reached the caller's server and a rig's
/// socket went beside it, and tmux 3.8 puts a socket in /tmp when TMUX_TMPDIR
/// names no folder. Read under [`env_lock`]'s lock, which it takes and lets go before
/// it panics: outside it, a sibling's value written under it passed the check (#1089,
/// #1110). Under that lock already, [`checked_tmux_held`].
#[allow(dead_code)]
pub fn checked_tmux() -> PathBuf {
    let read = {
        let _read = test_env::for_check("checked_tmux");
        tmux_read()
    };
    read.unwrap_or_else(|why| panic!("{why}"))
}

/// [`checked_tmux`] for a caller that holds [`env_lock`]'s lock, as `held` proves.
#[allow(dead_code)]
pub fn checked_tmux_held(_held: &EnvHeld) -> PathBuf {
    tmux_read().unwrap_or_else(|why| panic!("{why}"))
}

/// TMUX_TMPDIR, or [`tmux_refusal`]'s line. Read under [`env_lock`]'s lock.
#[allow(dead_code)]
fn tmux_read() -> Result<PathBuf, String> {
    match tmux_refusal() {
        Some(why) => Err(why),
        None => Ok(PathBuf::from(std::env::var_os("TMUX_TMPDIR").unwrap())),
    }
}

/// A rig's private tmux server on the socket `sock`, named by `-L`, never the
/// user's (AGENTS.md), in the folder [`checked_tmux`] holds. Every rig starts
/// its server through [`Server::start`].
/// Dropped, it kills the server and removes its socket file, which kill-server
/// leaves behind by any exit path (tests/run.sh, the note above kill_socks).
#[allow(dead_code)]
pub struct Server {
    pub sock: String,
    /// the socket file, asked of the server once it runs; empty before
    path: String,
    /// whether a start was run: with none there is nothing to kill, and a kill-server
    /// would make tmux's own folder in the socket folder for nothing
    started: bool,
}

#[allow(dead_code)]
impl Server {
    /// The guard of `sock`, with no server yet.
    pub fn new(sock: &str) -> Server {
        Server {
            sock: sock.to_string(),
            path: String::new(),
            started: false,
        }
    }

    /// Starts the server: `tmux -L <sock> -f /dev/null`, then what `with` adds,
    /// the command that makes its first session. Whether that ran and exited 0.
    /// A start that ran and failed is red with its status and stderr, so a rig
    /// names why (#1084). `false` where tmux is not there to run.
    pub fn start(&mut self, with: impl FnOnce(&mut Command)) -> bool {
        checked_tmux();
        let mut c = Command::new("tmux");
        c.args(["-L", &self.sock, "-f", "/dev/null"]);
        // kill-server is a TERM the server sends itself, then a HUP to each pane,
        // which starts with the server's mask: both at their default and unblocked,
        // so a suite started with them blocked leaves no server or pane (#1061, #1066)
        at_default(&mut c, &[libc::SIGTERM, libc::SIGHUP]);
        with(&mut c);
        self.started = true;
        let Ok(o) = c.output() else { return false };
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(o.status.success(), "the server on {} did not start: {}: {err}", self.sock, o.status);
        self.path = tmux_on(&self.sock, &["display-message", "-p", "#{socket_path}"]);
        true
    }
}

impl Server {
    /// Kills the server and removes its socket file, once. A rig calls it before it
    /// removes the folder its panes run in, so the server ends, and kill-server's HUP
    /// reaches each pane, while that folder is still there (#1066).
    pub fn stop(&mut self) {
        if !std::mem::take(&mut self.started) {
            return;
        }
        let _ = Command::new("tmux").args(["-L", &self.sock, "kill-server"]).output();
        if self.path.ends_with(&self.sock) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A stand-in for the caller's own tmux server (#1066): a `-S` server in a
/// folder of its own, `<at>/s/tmux-<uid>/default`, so that a plain `tmux` finds it
/// both ways it finds the caller's, by TMUX ([`StandIn::tmux`]) and as the
/// default socket of TMUX_TMPDIR `<at>/s`. It holds a hostile agent list, one
/// that names ssh and the shells, and a hostile theme, and logs every command a
/// client sends it (`-vv`, in `at`). Never the live server: its socket is a path
/// under `at`, started with none of the caller's session. Killed when dropped.
#[allow(dead_code)]
pub struct StandIn {
    at: PathBuf,
    sock: PathBuf,
    /// TMUX, as tmux sets it in a pane of this server
    pub tmux: String,
    pub pane: String,
}

#[allow(dead_code)]
impl StandIn {
    /// `None` where tmux is not there to run.
    pub fn start(at: &Path) -> Option<StandIn> {
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let folder = at.join("s").join(format!("tmux-{uid}"));
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&folder).unwrap();
        let mut s = StandIn {
            at: at.to_path_buf(),
            sock: folder.join("default"),
            tmux: String::new(),
            pane: String::new(),
        };
        let hostile = "|bash|sh|dash|zsh|fish|ssh|mosh|vim|nvim|less|man|claude|codex|";
        let mut c = s.command();
        c.args(["-vv", "-f", "/dev/null", "new-session", "-d", "-c"])
            .arg(at)
            .arg("sleep 300");
        c.args([";", "set-option", "-s", "theme", "light"]);
        c.args([";", "set-option", "-g", "@cockpit_agents_glob", hostile]);
        for role in ["@thm_fg", "@thm_dim", "@thm_error", "@thm_sel"] {
            c.args([";", "set-option", "-g", role, "#ff00ff"]);
        }
        let o = c.output().ok()?;
        assert!(
            o.status.success(),
            "the stand-in did not start: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let ask = |what: &str| {
            let o = s.command().args(["display-message", "-p", what]).output().unwrap();
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        let (pid, pane) = (ask("#{pid}"), ask("#{pane_id}"));
        s.tmux = format!("{},{pid},0", s.sock.display());
        s.pane = pane;
        Some(s)
    }

    /// The folder to hand on as TMUX_TMPDIR, where its socket is the default.
    pub fn folder(&self) -> PathBuf {
        self.at.join("s")
    }

    fn command(&self) -> Command {
        let mut c = Command::new("tmux");
        c.arg("-S").arg(&self.sock).current_dir(&self.at);
        c.env_remove("TMUX").env_remove("TMUX_PANE").env_remove("TMUX_COCKPIT_DIR");
        c
    }

    /// The commands its clients have sent it so far, read from its `-vv` log.
    pub fn commands(&self) -> usize {
        let logs = std::fs::read_dir(&self.at).unwrap().flatten().map(|e| e.path());
        let name = |p: &PathBuf| p.file_name().unwrap().to_string_lossy().starts_with("tmux-server-");
        let text = logs.filter(name).map(|p| std::fs::read_to_string(p).unwrap_or_default());
        let line = |l: &str| l.contains(" message: client-") && l.contains(" command: ");
        text.map(|t| t.lines().filter(|l| line(l)).count()).sum()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = self.command().arg("kill-server").output();
    }
}

/// Whether the process `p` is a server or a pane of a rig of this binary started again
/// as `pid` ([`rows_pass_handed`]), read from /proc/<p> alone. A rig's socket is named
/// `cockpit-<what>-<pid>`, the pid of the binary that started it, here that child of
/// this row's own. A server is a `tmux` whose `-L` names such a socket, a pane a process
/// whose TMUX, which tmux sets for each pane, names one. Both must also carry this
/// binary's own TMUX_TMPDIR, which the child had from it and handed to every server
/// and pane, so a server another run leaked under a reused pid is never taken.
#[allow(dead_code)]
fn rig_process(p: u32, pid: u32) -> bool {
    let at = Path::new("/proc").join(p.to_string());
    let rigs = |sock: &str| sock.starts_with("cockpit-") && sock.ends_with(&format!("-{pid}"));
    let own = std::env::var_os("TMUX_TMPDIR").map(|d| {
        let mut v = b"TMUX_TMPDIR=".to_vec();
        v.extend_from_slice(std::os::unix::ffi::OsStrExt::as_bytes(d.as_os_str()));
        v
    });
    let line = std::fs::read(at.join("cmdline")).unwrap_or_default();
    let args: Vec<String> = line.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    let server = args.first().is_some_and(|a| a.rsplit('/').next() == Some("tmux")) && args.windows(2).any(|w| w[0] == "-L" && rigs(&w[1]));
    let env = std::fs::read(at.join("environ")).unwrap_or_default();
    let vars: Vec<&[u8]> = env.split(|b| *b == 0).collect();
    let pane = vars.iter().filter_map(|v| v.strip_prefix(b"TMUX=")).any(|v| {
        let sock = String::from_utf8_lossy(v).split(',').next().unwrap_or_default().to_string();
        rigs(sock.rsplit('/').next().unwrap_or_default())
    });
    let ours = own.is_some_and(|o| vars.contains(&o.as_slice()));
    (server || pane) && ours
}

/// The processes [`rig_process`] takes for `pid`, read from /proc. Linux only, as are
/// the rows that ask.
#[allow(dead_code)]
pub fn servers_left(pid: u32) -> Vec<u32> {
    let Ok(d) = std::fs::read_dir("/proc") else { return vec![] };
    let pids = d.flatten().filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok());
    pids.filter(|p| rig_process(*p, pid)).collect()
}

/// For a row that started this binary again as `pid`: no server and no pane of
/// its rigs is left. Polled for 10 s, since kill-server ends a server and its
/// panes after the command returns. A red run ends what it found with KILL: each
/// pid is read again, /proc/<pid> alone, by [`rig_process`] just before its own
/// kill, so it names them and leaves none, and signals no process it did not just
/// verify.
#[allow(dead_code)]
pub fn assert_no_server_left(pid: u32) {
    let step = std::time::Duration::from_millis(20);
    for _ in 0..500 {
        if servers_left(pid).is_empty() {
            return;
        }
        std::thread::sleep(step);
    }
    let left = servers_left(pid);
    for p in &left {
        if rig_process(*p, pid) {
            // SAFETY: kill only signals; `p` was read again as this run's just above.
            unsafe { libc::kill(*p as libc::pid_t, libc::SIGKILL) };
        }
    }
    panic!("the child {pid} left its rigs' servers or panes, now killed: {left:?}");
}

/// cargo's runner for this repo, which a row starts itself to show what it hands
/// a test binary (.cargo/config.toml, #973, #1066).
#[allow(dead_code)]
const RUNNER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/claude-guard/runner");

/// This binary's tests but `skip`, started again through the runner beside a
/// [`StandIn`] for the caller's own tmux server, with TMUX, TMUX_PANE and
/// TMUX_TMPDIR in the runner's environment naming it, as from a shell inside
/// that session (#1066). Red where a test fails there, or where any test sent
/// the stand-in a command. `skip` is the calling row's own name, so its child
/// starts no child of its own.
#[allow(dead_code)]
pub fn tests_beside_a_server(skip: &str) {
    let at = Scratch::fresh(std::env::temp_dir().join(format!("cstand-{}", std::process::id())));
    let Some(s) = StandIn::start(&at.join("s")) else {
        eprintln!("SKIP: no tmux on PATH");
        return;
    };
    let mut c = Command::new(RUNNER);
    c.arg(std::env::current_exe().unwrap()).args(["--skip", skip, "--exact"]);
    c.env("TMUX", &s.tmux).env("TMUX_PANE", &s.pane).env("TMUX_TMPDIR", s.folder());
    c.env("TMUX_COCKPIT_DIR", at.join("plugin"));
    let before = s.commands();
    let out = c.stdin(std::process::Stdio::null()).output().unwrap();
    let got = s.commands() - before;
    assert!(out.status.success(), "{}", told(&format!("all but {skip}"), &out));
    assert_eq!(got, 0, "the tests sent the caller's server {got} commands");
}

/// Set in the child of a row that starts this whole binary again, so that rows
/// of that kind return at once there rather than start children of their own.
#[allow(dead_code)]
const WHOLE_CHILD: &str = "COCKPIT_TEST_WHOLE_CHILD";

/// Whether this binary runs as the child of [`tests_leave_alone`]: a row of
/// that kind then has nothing to do.
#[allow(dead_code)]
pub fn in_a_whole_child() -> bool {
    std::env::var_os(WHOLE_CHILD).is_some()
}

/// The files the product's reading names under each of these variables, made
/// in a canary so that a read of one is an open the watch sees: a lookup of a
/// path that is not there makes no event. data/agents.rs:162 and
/// panel/prompts.rs:30.
#[allow(dead_code)]
fn canary_files(var: &str) -> &'static [&'static str] {
    match var {
        "XDG_CACHE_HOME" => &["tmux-cockpit/agents.json"],
        "XDG_CONFIG_HOME" => &[
            "tmux-cockpit/prompts/ask-code.txt",
            "tmux-cockpit/prompts/explain.txt",
            "tmux-cockpit/prompts/fix.txt",
            "tmux-cockpit/prompts/write.txt",
            "tmux-cockpit/prompts/write.examples.txt",
            "tmux-cockpit/prompts/write.explore.txt",
        ],
        _ => &[],
    }
}

/// This binary's tests, started again with each of `vars` set to a canary of
/// this call's own: COCKPIT_LOG a file, any other a folder holding the files
/// [`canary_files`] names. An inotify watch on every canary folder sees each
/// open, write, create, rename and delete under them while the child runs. Red
/// where a test fails there. Returns what the child touched, as `<var>: <path>`:
/// nothing, where no test reads or writes the caller's copy (#1066). In that
/// child it returns nothing at once, so the row that calls it starts no other.
/// Linux only.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn tests_leave_alone(vars: &[&str]) -> Vec<String> {
    if in_a_whole_child() {
        return vec![];
    }
    // one folder per call: two such rows of one binary run at once
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let at = std::env::temp_dir().join(format!("ccan-{}-{n}", std::process::id()));
    let at = Scratch::fresh(at);
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.env(WHOLE_CHILD, "1");
    let mut folders = vec![];
    for v in vars {
        let dir = at.join(v);
        std::fs::create_dir(&dir).unwrap();
        for f in canary_files(v) {
            std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
            std::fs::write(dir.join(f), "canary #1066 {{\n").unwrap();
        }
        c.env(v, if *v == "COCKPIT_LOG" { dir.join("log") } else { dir.clone() });
        let walk = |d: &Path| std::fs::read_dir(d).unwrap().flatten().map(|e| e.path());
        let mut todo = vec![dir.clone()];
        while let Some(d) = todo.pop() {
            todo.extend(walk(&d).filter(|p| p.is_dir()));
            folders.push((v.to_string(), d));
        }
    }
    // SAFETY: inotify_init1 takes flags only.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    assert!(fd >= 0, "inotify_init1: {}", std::io::Error::last_os_error());
    let mask = libc::IN_OPEN | libc::IN_MODIFY | libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_DELETE;
    let mut watched = std::collections::HashMap::new();
    for (v, d) in &folders {
        let path = std::ffi::CString::new(d.to_str().unwrap()).unwrap();
        // SAFETY: `fd` is this call's inotify, `path` a NUL-terminated folder name.
        let wd = unsafe { libc::inotify_add_watch(fd, path.as_ptr(), mask) };
        assert!(wd >= 0, "inotify_add_watch {}: {}", d.display(), std::io::Error::last_os_error());
        watched.insert(wd, (v.clone(), d.clone()));
    }
    let out = c.stdin(std::process::Stdio::null()).output().unwrap();
    assert!(out.status.success(), "{}", told("the whole binary beside canaries", &out));
    let mut touched = vec![];
    let mut buf = [0u8; 65536];
    loop {
        // SAFETY: `buf` is ours and its length is passed.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            break;
        }
        let mut i = 0;
        while i < n as usize {
            // SAFETY: the kernel writes whole inotify_event records into `buf`.
            let e = unsafe { std::ptr::read_unaligned(buf[i..].as_ptr().cast::<libc::inotify_event>()) };
            let name = &buf[i + 16..i + 16 + e.len as usize];
            let name = String::from_utf8_lossy(name).trim_end_matches('\0').to_string();
            if let Some((v, d)) = watched.get(&e.wd) {
                touched.push(format!("{v}: {}", d.join(name).strip_prefix(&at.0).unwrap().display()));
            }
            i += 16 + e.len as usize;
        }
    }
    // SAFETY: closes this call's own fd.
    unsafe { libc::close(fd) };
    touched.sort();
    touched.dedup();
    touched
}

// --- pane start-up waits for the basket_claude tests (#1056) ---

/// One tmux command on the private server `sock`, its output trimmed.
#[allow(dead_code)]
fn tmux_on(sock: &str, args: &[&str]) -> String {
    let out = std::process::Command::new("tmux").args(["-L", sock]).args(args).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

/// As a basket_claude test's `Server::pane`, a new window on the private server
/// `sock` running a copy of bash named `name` in `dir`, but the pane's process
/// holds 0.3 s before it execs `name` (#1056): the start-up window held open, so
/// a read that does not wait for [`settle`] finds the `held` script, not `name`.
/// The pane, and the argv its process has once settled ([`started`]).
#[allow(dead_code)]
pub fn held_pane(sock: &str, dir: &Path, name: &str) -> (String, Vec<String>) {
    let bin = dir.join(name);
    // a copy a pane already runs cannot be written over (ETXTBSY)
    if !bin.exists() {
        std::fs::copy("/bin/bash", &bin).unwrap();
    }
    let hold = dir.join("held");
    write_exec(&hold, "#!/bin/sh\nsleep 0.3\nexec \"$@\"\n");
    let cmd = format!("{} {} -c 'sleep 300; :'", hold.display(), bin.display());
    let d = dir.display().to_string();
    let pane = tmux_on(sock, &["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd]);
    (pane, started(&bin))
}

/// As [`held_pane`], but the start script first writes `name` into its own
/// comm, so for its 0.3 s comm reads `name` while argv[0] is still the script's
/// shell: the gap inside an exec, where the kernel sets comm before the argv
/// tmux reads (#1056). Where a process cannot write its own comm, the row would
/// hold no such gap: it says SKIP with the cause ([`comm_skip`]) and starts a
/// [`held_pane`]. So it does where there is no /proc, as `ps` gives no argv.
#[allow(dead_code)]
pub fn named_pane(sock: &str, dir: &Path, name: &str) -> (String, Vec<String>) {
    if !Path::new("/proc/self").is_dir() {
        eprintln!("SKIP: no /proc, so no comm to set ahead of argv[0]; a held row instead");
        return held_pane(sock, dir, name);
    }
    if let Some(skip) = comm_skip(Path::new("/proc/thread-self/comm")) {
        eprintln!("{skip}");
        return held_pane(sock, dir, name);
    }
    let bin = dir.join(name);
    // a copy a pane already runs cannot be written over (ETXTBSY)
    if !bin.exists() {
        std::fs::copy("/bin/bash", &bin).unwrap();
    }
    let named = dir.join("named");
    let body = concat!(
        "#!/bin/sh\n",
        // a failed write is said on the pane's screen, which settle's red prints
        "printf %s \"${1##*/}\" > /proc/$$/comm ||\n",
        "    { echo \"$0: no comm write\"; exec sleep 300; }\n",
        "sleep 0.3\nexec \"$@\"\n"
    );
    write_exec(&named, body);
    let cmd = format!("{} {} -c 'sleep 300; :'", named.display(), bin.display());
    let d = dir.display().to_string();
    let pane = tmux_on(sock, &["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd]);
    (pane, started(&bin))
}

/// The argv of a pane once its start script has exec'd `bin -c 'sleep 300; :'`.
#[allow(dead_code)]
pub fn started(bin: &Path) -> Vec<String> {
    vec![bin.display().to_string(), "-c".into(), "sleep 300; :".into()]
}

/// Where a process cannot write its own comm at `comm`, the SKIP line that says
/// so with the error, else `None`. Tried on this thread's own name, under the
/// kernel's rule the start script's write meets too. A box that mounts /proc
/// read-only, or forbids the write by policy, holds no comm-ahead gap (#1068).
#[allow(dead_code)]
pub fn comm_skip(comm: &Path) -> Option<String> {
    let me = std::fs::read_to_string(comm).unwrap_or_default();
    let e = std::fs::write(comm, me.trim_end()).err()?;
    let at = comm.display();
    Some(format!("SKIP: {at} cannot be written ({e}): no comm-ahead row, a held one instead"))
}

/// Waits until the pane's own process, on the private server `sock`, runs as
/// `comm` with exactly the arguments `argv` (#1056, #1184). Not by
/// pane_current_command: while a pane starts, tmux 3.8 reads it as the pane's
/// argv, then `tmux`, then the shell, then the name, so its first read can
/// already be the name. The pane's pid keeps through each exec.
/// Two checks: its comm (cut to 15 bytes, as the kernel cuts it), which only
/// moves forward, and its whole NUL-split cmdline, as the kernel sets comm
/// before the new argv. Not argv[0] alone: tmux runs a one-string start as
/// `<default-shell> -c '<string>'`, so a pane wanted as bash under a bash
/// default shell reads comm bash and argv[0] bash before the bash it execs
/// exists; only the rest of its argv tells them apart (#1184).
/// Red, naming both reads last and the pane's screen, when it never gets there.
/// On Linux no row pins the comm check, as the argv is the new one only after
/// comm is. Without /proc there is no argv to read (`ps` gives one name): the
/// check is comm alone, the whole name.
#[allow(dead_code)]
pub fn settle(sock: &str, pane: &str, comm: &str, argv: &[&str]) {
    if let Err(e) = settle_within(sock, pane, comm, argv, 500) {
        panic!("{e}");
    }
}

/// [`settle`] bounded to `tries` reads, 10 ms apart: `Err` with what it read
/// last where [`settle`] would be red, so a row can hold a pane short of its
/// exec and see the wait not end there (#1184). It takes no env lock.
#[allow(dead_code)]
pub fn settle_within(sock: &str, pane: &str, comm: &str, argv: &[&str], tries: usize) -> Result<(), String> {
    let pid = tmux_on(sock, &["display-message", "-p", "-t", pane, "#{pane_pid}"]);
    let mut last = (String::new(), None);
    for _ in 0..tries {
        last = names(&pid);
        let (got, args) = &last;
        // /proc's comm is cut to 15 bytes; `ps` keeps the whole name
        let comm_ok = got == comm || (args.is_some() && got == comm.get(..15).unwrap_or(comm));
        // and the whole argv: a -c shell shares comm and argv[0] with the shell it execs
        if comm_ok && args.as_deref().is_none_or(|a| a == argv) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (got, args) = last;
    let last = format!("comm {got:?}, argv {args:?}");
    let screen = tmux_on(sock, &["capture-pane", "-p", "-t", pane]);
    Err(format!(
        "pane {pane} never ran comm {comm:?}, argv {argv:?}: its process {pid} read {last} last; screen {screen:?}"
    ))
}

/// What [`settle`] reads of a process: its comm, and its cmdline split at each
/// NUL, the argv tmux names a pane by. Where there is no /proc, `ps` gives one
/// name (macOS prints a path, cut here to its last part), so there is no argv
/// to check: `None`.
#[allow(dead_code)]
fn names(pid: &str) -> (String, Option<Vec<String>>) {
    if Path::new("/proc/self").is_dir() {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let raw = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        // each argument ends in a NUL; an empty cmdline is no argument at all
        let raw = raw.strip_suffix(&[0]).unwrap_or(&raw);
        let argv = raw.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned());
        let argv = if raw.is_empty() { vec![] } else { argv.collect() };
        return (comm.trim_end().to_string(), Some(argv));
    }
    let out = std::process::Command::new("ps").args(["-o", "comm=", "-p", pid]).output();
    let c = out
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    (c.rsplit('/').next().unwrap_or_default().to_string(), None)
}

/// Until the thread `tid` of `t` sleeps or has ended: 4000 tries of 5 ms at most, a
/// failsafe only, since a row's verdict is the thread's own end. A thread that waits
/// for a lock sleeps there.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn asleep_or_done(t: &std::thread::JoinHandle<()>, tid: i32) {
    let state = || std::fs::read_to_string(format!("/proc/self/task/{tid}/stat")).unwrap_or_default();
    for tries in 0.. {
        let s = state();
        let asleep = s.rsplit_once(") ").is_some_and(|(_, r)| r.starts_with('S'));
        if t.is_finished() || asleep {
            return;
        }
        assert!(tries < 4000, "the thread neither slept nor ended after {tries} tries");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
