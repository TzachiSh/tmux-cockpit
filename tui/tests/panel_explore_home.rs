//! Explore first (#416) with HOME itself a git repo: the one test that sets HOME,
//! alone in its own binary so no other test reads it changed.
use cockpit_tui::panel::app::{App, Level, Tile, View};
use cockpit_tui::panel::capture::Capture;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A pane in a plain folder under a HOME that is a repo: the repo is HOME, so it is refused.
#[test]
fn a_repo_at_home_is_refused_from_a_folder_inside_it() {
    let d = std::env::temp_dir().join(format!("cockpit-panel-explore-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let home = d.join("home");
    std::fs::create_dir_all(home.join("proj/src")).unwrap();
    let ok = std::process::Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init", "-q"])
        .arg(&home)
        .status()
        .unwrap();
    assert!(ok.success());
    // SAFETY: this binary runs this one test, so no other thread reads HOME.
    unsafe { std::env::set_var("HOME", &home) };
    let cap = Capture {
        name: "work".into(),
        cmd: "bash".into(),
        path: home.join("proj/src").display().to_string(),
        screen: "$ ".into(),
        ..Default::default()
    };
    let mut a = App::new("%3", cap);
    let i = a.tiles().iter().position(|t| t.0 == Tile::Compose).unwrap();
    a.key(KeyEvent::from(KeyCode::Char(char::from(b'1' + i as u8))));
    assert_eq!(a.view, View::Compose);
    a.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let refused = !a.explore;
    let n = a.note.clone();
    let _ = std::fs::remove_dir_all(&d);
    assert!(refused, "the repo is HOME: {n:?}");
    let n = n.expect("a note");
    assert_eq!(n.level, Level::Warn);
    assert!(n.text.contains("home folder"), "{}", n.text);
}
