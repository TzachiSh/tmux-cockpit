//! A resumed panel says when its pane's work folder moved since the hide (#1354): the
//! folder cockpit-panel kept beside the hidden state (#1344) against the one captured now.
#[allow(dead_code)] // for fixed_env only
mod fake;

use cockpit_tui::data::workdir::WorkDir;
use cockpit_tui::panel::action::Hidden;
use cockpit_tui::panel::app::{App, Level, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::OutType;

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
