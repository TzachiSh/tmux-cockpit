//! The panel reads the pane's work folder, `Capture.work.dir` (#1354, the freeze's
//! amendments 9, 12 and 15), not the pane's cwd: Explore's root and Explain's
//! what-changed list follow it. A ref sent to the agent is still named from the pane's
//! cwd, the folder the agent resolves it in.
#[allow(dead_code)] // for its Scratch guard and fixed_env only
mod fake;

use cockpit_tui::data::workdir::{Source, WorkDir};
use cockpit_tui::panel::app::{App, Tile, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::changes::{Collector, Gh};
use cockpit_tui::panel::envelope::{Flow, OutType};
use cockpit_tui::panel::look::Looks;
use cockpit_tui::panel::request::Effort;
use cockpit_tui::refs::Ref;
use crossterm::event::{KeyCode, KeyEvent};
use fake::Scratch;
use std::path::Path;
use std::time::Duration;

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

/// A ref goes to the agent named from the pane's cwd, the agent's own folder, as ^T
/// names it: from the cwd under it, absolute outside it (#1342 amendment 15).
#[test]
fn a_ref_is_named_from_the_pane_cwd_as_the_agent_resolves_it() {
    let d = scratch("refs");
    tree(&d);
    std::fs::write(d.join("cwd/b.rs"), "fn b() {}\nfn c() {}\n").unwrap();
    let abs = Ref::new(d.join("work/src/a.rs"), Some((1, 2)), None).unwrap();
    let rows = [
        ("under the work folder only", abs.clone(), format!("{}:1-2", abs.path().display())),
        (
            "under the pane cwd",
            Ref::new(d.join("cwd/b.rs"), Some((1, 2)), None).unwrap(),
            "b.rs:1-2".into(),
        ),
    ];
    for (row, r, want) in rows {
        let mut a = app(&d.join("cwd"), &d.join("work"));
        a.basket = vec![r];
        let items = a.write_refs(&write());
        let texts: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, [want.as_str()], "{row}");
    }
}

/// A capture with no work folder gives explore no root. The panel reads
/// `work.dir` alone and never falls back to the pane's cwd (#1362, check 3).
#[test]
fn no_work_folder_refuses_explore() {
    let d = scratch("empty");
    tree(&d);
    let mut a = app(&d.join("work"), Path::new(""));
    assert_eq!(explore_root(&mut a), None);
}

/// A collector whose gh is never reached: the repo has no GitHub remote.
fn looks(d: &Path) -> Looks {
    let gh = Gh {
        program: d.join("no-gh").into_os_string(),
        path: None,
        budget: Duration::from_secs(20),
    };
    Looks::new(Collector::new(gh, Some(d.join("store")), Box::new(|| 1_000_000)))
}

fn press(a: &mut App, c: char) {
    a.key(KeyEvent::from(KeyCode::Char(c)));
}

#[test]
fn explain_lists_what_changed_in_the_work_folder() {
    let d = scratch("look");
    tree(&d);
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(d.join("work"))
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(["commit", "-q", "--allow-empty", "-m", "first"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .unwrap();
    assert!(ok.success());
    let mut a = app(&d.join("cwd"), &d.join("work"));
    a.changes = Some(looks(&d));
    let i = a.tiles().iter().position(|t| t.0 == Tile::Explain).unwrap();
    press(&mut a, char::from(b'1' + i as u8));
    press(&mut a, '1');
    assert_eq!(a.view, View::Busy);
    let l = a.changes.as_mut().unwrap();
    let settled = (0..6000).any(|_| {
        l.poll();
        let done = !l.waiting();
        if !done {
            std::thread::sleep(Duration::from_millis(5));
        }
        done
    });
    assert!(settled, "the list never came");
    assert!(a.changes_shown().is_some(), "no list for the work folder's repo");
}
