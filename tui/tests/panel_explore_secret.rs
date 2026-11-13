//! #966: a read dir's name goes into the system prompt (`Profile::where_note`)
//! and to `--add-dir` whole, and a redacted name would name no folder. So a
//! folder that `redact::redact_clean` would hide part of is never read: `^R`
//! refuses it and the write runs on the screen alone, an Ask on a repo under
//! it is refused, and no claude starts on it. Each line names the folder as
//! redacted. Every key here is made up.
mod fake;

use cockpit_tui::panel::app::{App, Level, Tile, View};
use cockpit_tui::panel::ask;
use cockpit_tui::panel::capture::{Capture, window_kind};
use cockpit_tui::panel::envelope::{Flow, OutType};
use cockpit_tui::panel::prompts::{self, Ctx};
use cockpit_tui::panel::request::{Call, Effort, Fail, Profile, Request, read_dir_ok, secret_dir};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the refusal says after the folder's redacted name.
const WHY: &str = concat!(
    "holds what looks like a secret, and the call would send its name; ",
    "rename the folder to read it"
);

/// A made-up key as a folder's whole name.
const PLAIN: &str = "/u/deploy-sk-ant-MADEUPFAKEFAKE00";
/// The key in a parent folder of the one read.
const PARENT: &str = "/u/deploy-sk-ant-MADEUPFAKEFAKE00/app";
/// The key behind U+2066, the only boundary before it.
const GLUED: &str = "/u/w\u{2066}sk-ant-MADEUPFAKEFAKE00";
/// The key cut in two by U+202E: too short to match on either side.
const SPLIT: &str = "/u/sk-ant-MADEUP\u{202E}FAKEFAKE00";

/// Each folder form, and the name the user is shown for it.
const FORMS: [(&str, &str, &str); 4] = [
    ("plain", PLAIN, "/u/deploy-[hidden]"),
    ("parent", PARENT, "/u/deploy-[hidden]/app"),
    ("glued", GLUED, "/u/w[hidden]"),
    ("split", SPLIT, "/u/[hidden]"),
];

/// The refusal for a folder shown as `shown`.
fn refusal(shown: &str) -> String {
    format!("{shown:?} {WHY}")
}

fn app(path: &str) -> App {
    fake::fixed_env();
    let cap = Capture {
        name: "work".into(),
        cmd: "bash".into(),
        path: path.into(),
        screen: "$ ".into(),
        ..Default::default()
    };
    App::new("%3", cap)
}

fn ctrl(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn compose(a: &mut App) {
    let i = a.tiles().iter().position(|t| t.0 == Tile::Compose).unwrap();
    a.key(KeyEvent::from(KeyCode::Char(char::from(b'1' + i as u8))));
    assert_eq!(a.view, View::Compose);
}

fn write_flow() -> Flow {
    Flow::Write {
        draft: "fix it".into(),
        out: OutType::Auto,
    }
}

/// Once an `App` is built, the binary runs under the fixed environment (#957).
#[test]
fn an_app_is_built_under_the_fixed_environment() {
    let _ = app("/home/u/proj");
    fake::assert_fixed_env();
}

#[test]
fn the_fixed_environment_replaces_values_already_set() {
    fake::fixed_env_in_a_child("an_app_is_built_under_the_fixed_environment");
}

// ------------------------------------------------------------- the check

/// Each table compares every row at once, so a failure names each row it hit.
#[test]
fn secret_dir_names_each_form_as_redacted() {
    let got: Vec<_> = FORMS.iter().map(|(r, d, _)| (*r, secret_dir(Path::new(d)))).collect();
    let want: Vec<_> = FORMS.iter().map(|(r, _, s)| (*r, Some(s.to_string()))).collect();
    assert_eq!(got, want);
}

/// A folder the matchers leave whole is no secret: empty, plain, a short `sk-`.
#[test]
fn secret_dir_passes_a_folder_it_would_not_change() {
    for dir in ["", "/home/u/proj", "/u/sk-short", "/u/task-ant-x"] {
        assert_eq!(secret_dir(Path::new(dir)), None, "{dir:?}");
    }
}

#[test]
fn read_dir_ok_refuses_each_form_by_its_redacted_name() {
    let got: Vec<_> = FORMS.iter().map(|(r, d, _)| (*r, read_dir_ok(Path::new(d)))).collect();
    let want: Vec<_> = FORMS.iter().map(|(row, _, shown)| (*row, Err(refusal(shown)))).collect();
    assert_eq!(got, want);
}

/// A name that is not UTF-8 is read lossily for the check, so its key is
/// still found and never shown.
#[test]
fn read_dir_ok_refuses_a_key_in_a_name_not_utf8_by_its_redacted_name() {
    use std::os::unix::ffi::OsStrExt;
    let d = PathBuf::from(std::ffi::OsStr::from_bytes(b"/u/deploy-sk-ant-MADEUPFAKEFAKE00/\xff"));
    assert_eq!(read_dir_ok(&d), Err(refusal("/u/deploy-[hidden]/\u{FFFD}")));
}

// ------------------------------------------------------------- a write

/// ^R on the folder: whether explore came on, and the note it left.
fn toggled(dir: &str) -> (bool, Option<(Level, String)>) {
    let mut a = app(dir);
    compose(&mut a);
    ctrl(&mut a, 'r');
    (a.explore, a.note.as_ref().map(|n| (n.level, n.text.clone())))
}

#[test]
fn ctrl_r_on_a_secret_folder_is_refused_in_one_line_naming_it_redacted() {
    let got: Vec<_> = FORMS.iter().map(|(row, dir, _)| (*row, toggled(dir))).collect();
    let line = |shown| Some((Level::Warn, format!("explore refused: {}", refusal(shown))));
    let want: Vec<_> = FORMS.iter().map(|(row, _, shown)| (*row, (false, line(shown)))).collect();
    assert_eq!(got, want);
}

/// The symptom: master put the whole folder in the system prompt.
#[test]
fn no_where_note_is_built_for_a_secret_folder() {
    let mut a = app(PLAIN);
    a.explore = true;
    assert_eq!(a.profile_for(&write_flow()).where_note(), None);
}

/// A toggle left on (a hidden panel, a test) still runs the plain write.
/// A write sent from `dir` with the toggle forced on: its profile, and whether
/// its system prompt is the plain write's.
fn forced(dir: &str) -> (Profile, bool) {
    let mut a = app(dir);
    compose(&mut a);
    a.explore = true;
    a.draft = "fix it".into();
    ctrl(&mut a, 's');
    let ctx = Ctx {
        out: OutType::Auto,
        window: window_kind("work", "bash"),
        ..Ctx::default()
    };
    (a.profile_for(&write_flow()), a.request_parts().unwrap().0 == prompts::write(&ctx))
}

#[test]
fn a_stale_explore_flag_on_a_secret_folder_runs_the_plain_write() {
    let got: Vec<_> = FORMS.iter().map(|(row, dir, _)| (*row, forced(dir))).collect();
    let want: Vec<_> = FORMS.iter().map(|(row, ..)| (*row, (Profile::TEXT, true))).collect();
    assert_eq!(got, want);
}

/// Pinned: an ordinary folder explores as before, its note byte for byte.
#[test]
fn an_ordinary_folder_explores_as_before() {
    let mut a = app("/home/u/proj");
    compose(&mut a);
    ctrl(&mut a, 'r');
    assert!(a.explore);
    let want = "The repository is at \"/home/u/proj\". Read, Grep and Glob take absolute \
                paths under it, and anything outside it is refused; your working directory \
                is not the repository.";
    assert_eq!(a.profile_for(&write_flow()).where_note().as_deref(), Some(want));
}

// ------------------------------------------------------------- the call

/// The check `start` runs: whatever profile names such a folder (a resumed
/// Ask's), no claude starts. The fake leaves a file when it runs.
#[test]
fn no_claude_starts_on_a_secret_read_dir() {
    let name = format!("cockpit-966-call-{}", std::process::id());
    let d = fake::Scratch::fresh(std::env::temp_dir().join(name));
    let claude = d.join("claude");
    fake::write_exec(&claude, &format!("#!/bin/sh\n: > '{}'\necho ok\n", d.join("ran").display()));
    let call = Call {
        claude: claude.display().to_string(),
        timeout: Duration::from_secs(30),
        dir: d.join("cache"),
        effort: Effort::Medium,
    };
    let p = Profile::read_only(PathBuf::from(PLAIN), true, 0.25, Duration::from_secs(30));
    // a refusal is sent before spawn_with returns, so the first poll holds it
    let mut req = Request::spawn_with(&call, &p, "SYS", "p", "default");
    assert_eq!(req.poll(), Some(Err(Fail::Failed(refusal("/u/deploy-[hidden]")))));
    assert!(!d.join("ran").exists(), "a claude ran");
}

// ------------------------------------------------------------- an Ask

/// A repo whose folder's name is `name`, under the temp dir, holding `src/x.rs`.
fn repo(name: &str) -> (fake::Scratch, PathBuf) {
    fake::fixed_env();
    let top = std::env::temp_dir().canonicalize().unwrap();
    let d = fake::Scratch::fresh(top.join(format!("{name}{}", std::process::id())));
    std::fs::create_dir_all(d.join(".git")).unwrap();
    std::fs::create_dir_all(d.join("src")).unwrap();
    let f = d.join("src/x.rs");
    std::fs::write(&f, "a\nb\n").unwrap();
    (d, f)
}

#[test]
fn an_ask_on_a_repo_under_a_secret_folder_is_refused_naming_it_redacted() {
    let (d, f) = repo("cockpit-966-deploy-sk-ant-MADEUPFAKEFAKE00-");
    let top = d.parent().unwrap().display().to_string();
    let why = ask::load(&f, 1, 2).err();
    assert_eq!(why, Some(refusal(&format!("{top}/cockpit-966-deploy-[hidden]"))));
}

/// Glued and split, the file's own path holds the control, which `load`
/// refuses first: the line names no folder and no key.
#[test]
fn an_ask_on_a_glued_or_split_secret_folder_is_refused_without_the_key() {
    let rows = [
        ("glued", "cockpit-966-w\u{2066}sk-ant-MADEUPFAKEFAKE00-"),
        ("split", "cockpit-966-sk-ant-MADEUP\u{202E}FAKEFAKE00-"),
    ];
    for (row, name) in rows {
        let (_d, f) = repo(name);
        let why = ask::load(&f, 1, 2).expect_err(row);
        assert!(!why.contains("MADEUP") && !why.contains("FAKEFAKE00"), "{row}: {why}");
    }
}

/// Pinned: an Ask in an ordinary repo opens as before.
#[test]
fn an_ask_in_an_ordinary_repo_opens() {
    let (d, f) = repo("cockpit-966-plain-");
    let q = ask::load(&f, 1, 2).unwrap();
    assert_eq!(q.repo, d.0);
    let mut a = app("/home/u/proj");
    a.ask_open(q);
    assert!(matches!(a.view, View::Input { .. }), "{:?}", a.view);
}
