//! The panel reads the pane's work folder, `Capture.work.dir` (#1354, the freeze's
//! amendment 9), not the pane's cwd: Explore's root and the refs' base follow it.
#[allow(dead_code)] // for its Scratch guard and fixed_env only
mod fake;

use cockpit_tui::data::workdir::{Source, WorkDir};
use cockpit_tui::panel::app::App;
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType};
use cockpit_tui::panel::request::Effort;
use cockpit_tui::refs::Ref;
use fake::Scratch;
use std::path::Path;

fn scratch(tag: &str) -> Scratch {
    Scratch::fresh(std::env::temp_dir().join(format!("cockpit-workroot-{tag}-{}", std::process::id())))
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

/// A shell pane whose cwd is `cwd` and whose work folder, by the shared rule, is `work`.
fn app(cwd: &Path, work: &Path) -> App {
    fake::fixed_env();
    let cap = Capture {
        name: "work".into(),
        cmd: "bash".into(),
        path: cwd.display().to_string(),
        screen: "$ ".into(),
        work: WorkDir {
            dir: work.display().to_string(),
            source: Source::ActiveIn,
            cwd: cwd.display().to_string(),
            recent: vec![],
        },
        ..Default::default()
    };
    App::new("%3", cap)
}

fn write() -> Flow {
    Flow::Write {
        draft: "fix it".into(),
        out: OutType::Auto,
    }
}

/// The folder an exploring write would add, as its argv names it.
fn explore_root(a: &mut App) -> Option<String> {
    a.explore = true;
    let argv = a.profile_for(&write()).argv(Effort::Medium);
    let i = argv.iter().position(|x| x == "--add-dir")?;
    Some(argv[i + 1].clone())
}

fn canon(p: &Path) -> String {
    std::fs::canonicalize(p).unwrap().display().to_string()
}

/// The two folders and a repo at `<d>/work`: the pane's cwd stays outside it.
fn tree(d: &Path) {
    std::fs::create_dir_all(d.join("cwd")).unwrap();
    std::fs::create_dir_all(d.join("work/src")).unwrap();
    git_init(&d.join("work"));
    std::fs::write(d.join("work/src/a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
}

#[test]
fn explore_reads_the_work_folder_not_the_pane_cwd() {
    let d = scratch("root");
    tree(&d);
    let mut a = app(&d.join("cwd"), &d.join("work"));
    assert_eq!(explore_root(&mut a), Some(canon(&d.join("work"))));
}

#[test]
fn explore_reads_the_repo_the_work_folder_is_in() {
    let d = scratch("sub");
    tree(&d);
    let mut a = app(&d.join("cwd"), &d.join("work/src"));
    assert_eq!(explore_root(&mut a), Some(canon(&d.join("work"))));
}

#[test]
fn a_ref_in_the_work_folder_is_named_from_it() {
    let d = scratch("refs");
    tree(&d);
    let mut a = app(&d.join("cwd"), &d.join("work"));
    a.basket = vec![Ref::new(d.join("work/src/a.rs"), Some((1, 2)), None).unwrap()];
    let items = a.write_refs(&write());
    let texts: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
    assert_eq!(texts, ["src/a.rs:1-2"]);
}

/// A fixture with no work folder set keeps reading the pane's cwd, as `capture`
/// does where the shared rule has no answer.
#[test]
fn no_work_folder_falls_back_to_the_pane_cwd() {
    let d = scratch("empty");
    tree(&d);
    let mut a = app(&d.join("work"), Path::new(""));
    assert_eq!(explore_root(&mut a), Some(canon(&d.join("work"))));
}
