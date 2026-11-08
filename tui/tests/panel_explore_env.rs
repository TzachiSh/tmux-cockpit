//! Explore first (#416) under an inherited git environment: these tests set
//! process env, so they run in a binary of their own. Its tests still run in
//! parallel threads, and only `with_env` takes `ENV`: setup outside it sees
//! whatever another test has set at that moment.
#[allow(dead_code)] // for its Scratch guard only, not write_exec
mod fake;

use cockpit_tui::data::gitsafe::REDIRECT;
use cockpit_tui::panel::app::App;
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType};
use cockpit_tui::panel::request::Effort;
use fake::Scratch;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

/// A fresh temp dir in the shared guard, removed on drop.
fn scratch(tag: &str) -> Scratch {
    let name = format!("cockpit-panel-explore-env-{tag}-{}", std::process::id());
    Scratch::fresh(std::env::temp_dir().join(name))
}

/// A repo at `dir` whatever another test's [`with_env`] has set: [`REDIRECT`],
/// gitsafe's list of what could point git elsewhere, is stripped as the code
/// under test strips it. GIT_CEILING_DIRECTORIES, which a test here sets too,
/// cannot move a `git init` of a named dir; it goes as well, defensively, and
/// no row tests it.
fn git_init(dir: &Path) {
    let mut c = std::process::Command::new("git");
    REDIRECT
        .iter()
        .chain(&["GIT_CEILING_DIRECTORIES"])
        .for_each(|v| _ = c.env_remove(v));
    let ok = c
        .args(["-c", "init.defaultBranch=main", "init", "-q"])
        .arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .unwrap();
    assert!(ok.success());
}

/// The folder an exploring write on `pane` would add.
fn root_of(pane: &Path) -> Option<String> {
    let cap = Capture {
        name: "work".into(),
        cmd: "bash".into(),
        path: pane.display().to_string(),
        screen: "$ ".into(),
        ..Default::default()
    };
    let mut a = App::new("%3", cap);
    a.explore = true;
    let flow = Flow::Write {
        draft: "fix it".into(),
        out: OutType::Auto,
    };
    let argv = a.profile_for(&flow).argv(Effort::Medium);
    let i = argv.iter().position(|x| x == "--add-dir")?;
    Some(argv[i + 1].clone())
}

/// A panicking test inside `with_env` leaves no var behind for the next one.
#[test]
fn a_panic_inside_with_env_still_restores_the_vars() {
    let g = scratch("panic");
    let before = {
        let _l = ENV.lock().unwrap_or_else(|e| e.into_inner());
        std::env::var_os("GIT_WORK_TREE")
    };
    let r = std::panic::catch_unwind(|| with_env(&[("GIT_WORK_TREE", &g.0)], || panic!("in the test body")));
    assert!(r.is_err(), "the body panicked");
    let _l = ENV.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(std::env::var_os("GIT_WORK_TREE"), before);
}

/// The values a [`with_env`] replaced, put back on drop: a panic in its body
/// unwinds through here too.
struct Restore(Vec<(String, Option<OsString>)>);

impl Drop for Restore {
    fn drop(&mut self) {
        for (k, v) in &self.0 {
            // SAFETY: dropped before the ENV guard that every test here takes first.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}

/// `vars` set while `f` runs, then put back, under the lock.
fn with_env<T>(vars: &[(&str, &Path)], f: impl FnOnce() -> T) -> T {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let _r = Restore(vars.iter().map(|(k, _)| (k.to_string(), std::env::var_os(k))).collect());
    // SAFETY: every test here takes ENV first, so no other thread reads the env meanwhile.
    unsafe { vars.iter().for_each(|(k, v)| std::env::set_var(k, v)) };
    f()
}

/// The setup's `git init` makes `dir` a repo while another test's var names
/// somewhere else: rows are the [`REDIRECT`] vars this file sets, each pointing
/// at `other`.
#[test]
fn git_init_ignores_a_var_another_test_set() {
    let g = scratch("setup");
    let other = g.0.join("other");
    for (var, val) in [("GIT_DIR", other.join(".git")), ("GIT_WORK_TREE", other.clone())] {
        let dir = g.0.join(var);
        with_env(&[(var, &val)], || git_init(&dir));
        assert!(dir.join(".git/HEAD").is_file(), "{var}: no repo at {}", dir.display());
    }
}

/// An inherited GIT_DIR and GIT_WORK_TREE naming an outer repo would pass every
/// check on the root: only stripping them keeps the pane's own repo.
#[test]
fn an_inherited_git_dir_cannot_move_the_root() {
    let g = scratch("gitdir");
    let (outer, inner) = (g.0.join("outer"), g.0.join("outer/inner"));
    std::fs::create_dir_all(inner.join("src")).unwrap();
    git_init(&outer);
    git_init(&inner);
    let got = with_env(&[("GIT_DIR", &outer.join(".git")), ("GIT_WORK_TREE", &outer)], || {
        root_of(&inner.join("src"))
    });
    let want = std::fs::canonicalize(&inner).unwrap().display().to_string();
    assert_eq!(got, Some(want));
}

/// Outside any repo the pane's folder stays: the ceiling stops discovery at this
/// test's dir, so a TMPDIR inside a repo cannot change the answer.
#[test]
fn a_folder_outside_any_repo_keeps_the_pane_folder() {
    let g = scratch("norepo");
    let dir = g.0.join("notes");
    std::fs::create_dir_all(&dir).unwrap();
    let got = with_env(&[("GIT_CEILING_DIRECTORIES", &g.0)], || root_of(&dir));
    assert_eq!(got, Some(dir.display().to_string()));
}
