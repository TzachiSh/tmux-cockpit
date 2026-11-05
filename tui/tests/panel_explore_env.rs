//! Explore first (#416) under an inherited git environment: these tests set
//! process env, so they run in a binary of their own, one at a time.
use cockpit_tui::panel::app::App;
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

/// A fresh temp dir, removed on drop.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("cockpit-panel-explore-env-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Scratch(d)
}

fn git_init(dir: &Path) {
    let ok = std::process::Command::new("git")
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
    let argv = a.profile_for(&flow).argv();
    let i = argv.iter().position(|x| x == "--add-dir")?;
    Some(argv[i + 1].clone())
}

/// `vars` set while `f` runs, then removed, under the lock.
fn with_env<T>(vars: &[(&str, &Path)], f: impl FnOnce() -> T) -> T {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: every test here takes ENV first, so no other thread reads the env meanwhile.
    unsafe { vars.iter().for_each(|(k, v)| std::env::set_var(k, v)) };
    let r = f();
    unsafe { vars.iter().for_each(|(k, _)| std::env::remove_var(k)) };
    r
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
