//! A resumed panel says when its pane's work folder moved since the hide (#1354): the
//! folder cockpit-panel kept beside the hidden state (#1344) against the one captured now.
#[allow(dead_code)] // for fixed_env and its Scratch guard only
mod fake;

use cockpit_tui::data::workdir::WorkDir;
use cockpit_tui::panel::action::{Hidden, hide_body, write_hidden};
use cockpit_tui::panel::app::{App, Level, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::OutType;
use fake::Scratch;
use std::path::PathBuf;

/// A panel opened again on a pane whose work folder is `now`.
fn app(now: &str) -> App {
    fake::fixed_env();
    let cap = Capture {
        name: "work".into(),
        cmd: "bash".into(),
        path: now.into(),
        screen: "$ ".into(),
        work: WorkDir {
            dir: now.into(),
            cwd: now.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    App::new("%3", cap)
}

/// What ^Z kept on a bash pane on Home: nothing that moves the note but the folder.
fn hidden() -> Hidden {
    Hidden {
        cmd: "bash".into(),
        view: View::Home { sel: 0 },
        busy_back: None,
        draft: String::new(),
        cur: 0,
        out: OutType::Auto,
        model: 0,
        explore: false,
        run: None,
    }
}

/// The note a resume on `now` writes, `was` being the kept folder.
fn note(now: &str, was: Option<&str>) -> (Level, String) {
    let mut a = app(now);
    a.unhide_in(hidden(), was);
    let n = a.note.expect("a resume always says where it is");
    (n.level, n.text)
}

const BACK: &str = "back where you left it";

#[test]
fn a_resume_names_both_folders_when_the_work_folder_moved() {
    let want = format!("{BACK}; the folder is now /w/lane-b, not /w/lane-a");
    assert_eq!(note("/w/lane-b", Some("/w/lane-a")), (Level::Warn, want));
}

#[test]
fn a_resume_says_nothing_of_the_folder_when_it_is_the_same() {
    assert_eq!(note("/w/lane-a", Some("/w/lane-a")), (Level::Info, BACK.into()));
}

/// A hide from before #1344 has no file beside it.
#[test]
fn a_resume_with_no_wd_file_says_nothing_of_the_folder() {
    assert_eq!(note("/w/lane-a", None), (Level::Info, BACK.into()));
}

#[test]
fn an_empty_wd_file_says_nothing_of_the_folder() {
    assert_eq!(note("/w/lane-a", Some("")), (Level::Info, BACK.into()));
}

/// The kept folder is a file's text: an ESC or a newline in it is never drawn.
#[test]
fn a_control_character_in_a_kept_folder_is_not_drawn() {
    let want = format!("{BACK}; the folder is now /w/b, not /w/a[31m x");
    assert_eq!(note("/w/b", Some("/w/a\u{1b}[31m\nx")), (Level::Warn, want));
}

/// When the hide was saved, in unix seconds: the resume reads it at the same time.
const NOW: u64 = 1_800_000_000;

/// A hide of a panel on `was`, saved as cockpit-panel keeps it: `act.state`, and the
/// folder in `act.state.wd` when `kept`. The files live as long as the guard.
fn saved(tag: &str, was: &str, kept: bool) -> (Scratch, PathBuf) {
    let name = format!("cockpit-resume-folder-{tag}-{}", std::process::id());
    let d = Scratch::fresh(std::env::temp_dir().join(name));
    let body = hide_body(&app(was), NOW).expect("a Home panel is kept");
    write_hidden(&d.join("act"), &body).unwrap();
    if kept {
        std::fs::write(d.join("act.state.wd"), was).unwrap();
    }
    let state = d.join("act.state");
    (d, state)
}

/// The note `--resume <state> --last hidden` writes on a pane whose work folder is `now`.
fn resumed(state: &std::path::Path, now: &str) -> (Level, String) {
    let mut a = app(now);
    a.unhide_file(state, NOW);
    let n = a.note.expect("a resume always says where it is");
    (n.level, n.text)
}

#[test]
fn a_resume_from_its_files_reads_the_folder_kept_beside_the_state() {
    let (_d, state) = saved("kept", "/w/lane-a", true);
    let want = format!("{BACK}; the folder is now /w/lane-b, not /w/lane-a");
    assert_eq!(resumed(&state, "/w/lane-b"), (Level::Warn, want));
}

#[test]
fn a_resume_from_a_state_with_no_folder_beside_it_says_nothing_of_the_folder() {
    let (_d, state) = saved("none", "/w/lane-a", false);
    assert_eq!(resumed(&state, "/w/lane-b"), (Level::Info, BACK.into()));
}
