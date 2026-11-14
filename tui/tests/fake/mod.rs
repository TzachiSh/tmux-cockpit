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
//! binary can take for itself ([`fixed_env`], #957), and the `claude` a rig
//! answers a lookup with ([`rig_path`], #972).
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

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
#[allow(dead_code)]
pub fn fixed_env() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        // SAFETY: holds for each test binary that calls this, one by one. std's
        // getenv and setenv share one lock, and a spawn holds it while the
        // child takes the environment. Every read a test thread makes there is
        // a Rust read; the readers that are not Rust are child processes, which
        // get their copy at the spawn. A C function called in the test process
        // that reads the environment (a local time call, a getenv in a linked
        // library) would break this: no binary that calls this makes one today.
        unsafe {
            std::env::set_var("HOME", NO_HOME);
            std::env::set_var("XDG_CONFIG_HOME", NO_HOME);
            std::env::remove_var("COCKPIT_PANEL_MODEL");
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
/// child of its own, without end. Returns the child's pid, for a row that looks for
/// what the child left.
#[allow(dead_code)]
pub fn rows_pass_handed(rows: &[&str], signals: &'static [libc::c_int]) -> u32 {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.arg("--exact")
        .args(rows)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
    let mut child = unsafe { c.pre_exec(hand_down) }.spawn().unwrap();
    let pid = child.id();
    let step = std::time::Duration::from_millis(20);
    for tries in 0.. {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if tries == HANDED_FOR.as_millis() / step.as_millis() {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{rows:?} in a child had not ended after {HANDED_FOR:?} and was killed");
        }
        std::thread::sleep(step);
    }
    let out = child.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    let all = said.contains(&format!(" {} passed; 0 failed", rows.len()));
    assert!(out.status.success() && all, "{}", told(&rows.join(" "), &out));
    pid
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
#[allow(dead_code)]
pub fn held_pane(sock: &str, dir: &Path, name: &str) -> String {
    let bin = dir.join(name);
    // a copy a pane already runs cannot be written over (ETXTBSY)
    if !bin.exists() {
        std::fs::copy("/bin/bash", &bin).unwrap();
    }
    let hold = dir.join("held");
    std::fs::write(&hold, "#!/bin/sh\nsleep 0.3\nexec \"$@\"\n").unwrap();
    std::fs::set_permissions(&hold, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cmd = format!("{} {} -c 'sleep 300; :'", hold.display(), bin.display());
    let d = dir.display().to_string();
    tmux_on(sock, &["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd])
}

/// As [`held_pane`], but the start script first writes `name` into its own
/// comm, so for its 0.3 s comm reads `name` while argv[0] is still the script's
/// shell: the gap inside an exec, where the kernel sets comm before the argv
/// tmux reads (#1056). Red at once where /proc's comm cannot be written, as the
/// row would then hold no such gap and pass for nothing. Where there is no /proc
/// it says SKIP and starts a [`held_pane`], since `ps` gives no argv[0] to check.
#[allow(dead_code)]
pub fn named_pane(sock: &str, dir: &Path, name: &str) -> String {
    if !Path::new("/proc/self").is_dir() {
        eprintln!("SKIP: no /proc, so no comm to set ahead of argv[0]; a held row instead");
        return held_pane(sock, dir, name);
    }
    // the kernel's rule for the script's own write, tried on this thread's name
    let me = std::fs::read_to_string("/proc/thread-self/comm").unwrap_or_default();
    if let Err(e) = std::fs::write("/proc/thread-self/comm", me.trim_end()) {
        panic!("/proc/thread-self/comm cannot be written ({e}): no comm-ahead row here");
    }
    let bin = dir.join(name);
    // a copy a pane already runs cannot be written over (ETXTBSY)
    if !bin.exists() {
        std::fs::copy("/bin/bash", &bin).unwrap();
    }
    let named = dir.join("named");
    let body = concat!(
        "#!/bin/sh\n",
        "printf %s \"${1##*/}\" > /proc/$$/comm || exit 3\n",
        "sleep 0.3\nexec \"$@\"\n"
    );
    std::fs::write(&named, body).unwrap();
    std::fs::set_permissions(&named, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cmd = format!("{} {} -c 'sleep 300; :'", named.display(), bin.display());
    let d = dir.display().to_string();
    tmux_on(sock, &["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd])
}

/// Waits until the pane's own process, on the private server `sock`, has exec'd
/// `name` (#1056). Not by pane_current_command: while a pane starts, tmux 3.8
/// reads it as the pane's argv, then `tmux`, then the shell, then the name, so
/// its first read can already be `name`. The pane's pid keeps through each exec.
/// Two checks: its comm, which only moves forward, and the argv[0] tmux reads,
/// as the kernel sets comm before the new argv and an exec's empty cmdline sends
/// tmux back to the pane's start argv. Red, naming both reads last, when it
/// never gets there.
#[allow(dead_code)]
pub fn settle(sock: &str, pane: &str, name: &str) {
    let pid = tmux_on(sock, &["display-message", "-p", "-t", pane, "#{pane_pid}"]);
    let mut last = (String::new(), None);
    for _ in 0..500 {
        last = names(&pid);
        let (comm, argv0) = &last;
        // /proc's comm is cut to 15 bytes; argv[0] and `ps` keep the whole name
        let comm_ok = comm == name || (argv0.is_some() && comm == name.get(..15).unwrap_or(name));
        // two checks: comm, then the argv[0] tmux reads, which the kernel sets after it
        if comm_ok && argv0.as_deref().is_none_or(|a| a == name) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (comm, argv0) = last;
    let last = format!("comm {comm:?}, argv[0] {argv0:?}");
    panic!("pane {pane} never ran {name}: its process {pid} read {last} last");
}

/// What [`settle`] reads of a process: its comm, and the last path part of the
/// argv[0] in the cmdline tmux names a pane by. Where there is no /proc, `ps`
/// gives one name (macOS prints a path), so there is no argv[0] to check: `None`.
#[allow(dead_code)]
fn names(pid: &str) -> (String, Option<String>) {
    if Path::new("/proc/self").is_dir() {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let argv = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let argv0 = String::from_utf8_lossy(argv.split(|b| *b == 0).next().unwrap_or_default());
        let base = argv0.rsplit('/').next().unwrap_or_default().to_string();
        return (comm.trim_end().to_string(), Some(base));
    }
    let out = std::process::Command::new("ps").args(["-o", "comm=", "-p", pid]).output();
    let c = out
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    (c.rsplit('/').next().unwrap_or_default().to_string(), None)
}
