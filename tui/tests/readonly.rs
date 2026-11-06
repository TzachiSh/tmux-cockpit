//! COCKPIT_READONLY must silence every write to a tmux server, the board's wake
//! channel included: a probe run without it once left a dead `@cockpit_tui_pid`
//! on the owner's live server.
mod fake;

use cockpit_tui::data::tmux_set;
use cockpit_tui::push::Push;
use std::sync::Mutex;

/// COCKPIT_* are process-global and cargo runs tests on threads, so every test
/// that touches them serialises here and puts them back on the way out.
static ENV: Mutex<()> = Mutex::new(());

struct Env(Vec<(&'static str, Option<String>)>);

impl Env {
    fn set(vars: &[(&'static str, &str)]) -> Self {
        let old = vars.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect();
        for (k, v) in vars {
            // SAFETY: the ENV mutex is held for the whole lifetime of this guard.
            unsafe { std::env::set_var(k, v) };
        }
        Env(old)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for (k, v) in &self.0 {
            // SAFETY: as above.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}

/// A stand-in tmux that records its argv and runs nothing.
fn shim(dir: &std::path::Path) -> (String, std::path::PathBuf) {
    let (bin, log) = (dir.join("tmux"), dir.join("argv"));
    fake::write_exec(&bin, &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n", log.display()));
    (bin.display().to_string(), log)
}

/// A private scratch dir, removed when the guard drops: on a pass, and on a
/// failing assert's unwind too (#543).
struct Scratch(std::path::PathBuf);

impl std::ops::Deref for Scratch {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("cockpit-tui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Scratch(d)
}

/// #543: the scratch dir goes on every exit path, a failing assert's unwind included.
#[test]
fn a_scratch_dir_is_removed_on_a_pass_and_on_a_panic() {
    let passed = scratch("guard-pass").to_path_buf();
    let d = scratch("guard-panic");
    let panicked = d.to_path_buf();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _d = d;
        panic!("an assert failed");
    }))
    .unwrap_err();
    assert!(!passed.exists() && !panicked.exists(), "{passed:?} {panicked:?}");
}

#[test]
fn readonly_writes_nothing_not_even_the_wake_channel() {
    let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("ro");
    let (bin, log) = shim(&dir);
    let _env = Env::set(&[("COCKPIT_TMUX", &bin), ("COCKPIT_READONLY", "1")]);

    tmux_set(&["set-option", "-g", "@cockpit_board_wake", "1234"]);
    drop(Push::start()); // start lists its channel, Drop unlists it; neither may run

    assert!(!log.exists(), "COCKPIT_READONLY still wrote: {:?}", std::fs::read_to_string(&log));
}

/// The negative half: without it the same call does reach tmux, so the test
/// above is not passing because the shim is broken.
#[test]
fn without_readonly_the_write_reaches_tmux() {
    let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("rw");
    let (bin, log) = shim(&dir);
    let _env = Env::set(&[("COCKPIT_TMUX", &bin), ("COCKPIT_READONLY", "")]);

    tmux_set(&["set-option", "-g", "@cockpit_board_wake", "1234"]);

    assert_eq!(
        std::fs::read_to_string(&log).unwrap().trim(),
        "set-option -g @cockpit_board_wake 1234"
    );
}
