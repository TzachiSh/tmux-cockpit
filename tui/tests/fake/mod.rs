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
#[allow(dead_code)]
const NO_CLAUDE: &str = "#!/bin/sh\nexit 127\n";

/// The PATH for a rig whose folder of fakes is `bin`: `bin`, then the PATH of
/// this moment. `bin` is made if it is not there and gets a `claude`
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
        // no trailing `:` then: an empty entry is the cwd
        Ok(_) | Err(std::env::VarError::NotPresent) => first.to_string(),
        Err(e) => panic!("PATH: {e}"),
    }
}

/// Whether a spawn could start `p`: a file with an exec bit.
#[allow(dead_code)]
fn runs(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The first `claude` a spawn would find on `path`, the value of a PATH: `Ok`
/// with it when it lies under `dir`, else `Err` with it, or with `None` when
/// no entry holds one. It walks as execvp does: an empty entry is the cwd, and
/// a `claude` that is no file or has no exec bit is passed over. It reads and
/// never runs what it finds, so checking a rig that forgot [`rig_path`] starts
/// nothing.
#[allow(dead_code)]
pub fn rigs_claude(path: &std::ffi::OsStr, dir: &Path) -> Result<PathBuf, Option<PathBuf>> {
    assert!(dir.is_absolute(), "dir must be absolute: {}", dir.display());
    let cwd = |e: PathBuf| if e.as_os_str().is_empty() { PathBuf::from(".") } else { e };
    match std::env::split_paths(path).map(|e| cwd(e).join("claude")).find(|p| runs(p)) {
        Some(found) if found.starts_with(dir) => Ok(found),
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
