//! Explore first (#416): a Compose toggle that lets a write read the pane's
//! folder through a read-only profile. Fake claude scripts in temp dirs; no tmux.
use cockpit_tui::panel::app::{App, EXPLORE_BUDGET_USD, EXPLORE_TIMEOUT, Level, Tile, View};
use cockpit_tui::panel::capture::{Capture, window_kind};
use cockpit_tui::panel::envelope::{Flow, OutType};
use cockpit_tui::panel::prompts::{self, Ctx};
use cockpit_tui::panel::request::{Call, Fail, Profile};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

fn app(cmd: &str, path: &str) -> App {
    let cap = Capture {
        name: "work".into(),
        cmd: cmd.into(),
        path: path.into(),
        screen: "$ ".into(),
        ..Default::default()
    };
    App::new("%3", cap)
}

fn ctrl(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn note(a: &App) -> Option<(Level, &str)> {
    a.note.as_ref().map(|n| (n.level, n.text.as_str()))
}

/// Home, then the Compose tile.
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

/// A fresh temp dir, removed on drop, so a failed assert leaves nothing behind.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let d = std::env::temp_dir().join(format!("cockpit-panel-explore-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Scratch(d)
}

#[test]
fn ctrl_r_turns_explore_on_and_off_in_a_local_window() {
    let mut a = app("bash", "/home/u/proj");
    compose(&mut a);
    assert!(!a.explore, "off until asked");
    ctrl(&mut a, 'r');
    assert!(a.explore);
    assert_eq!(note(&a).map(|n| n.0), Some(Level::Info));
    ctrl(&mut a, 'r');
    assert!(!a.explore);
    assert_eq!(a.view, View::Compose, "the toggle stays on Compose");
}

#[test]
fn an_ssh_window_refuses_explore_with_a_warning() {
    for cmd in ["ssh", "docker", "kubectl"] {
        let mut a = app(cmd, "/home/u/proj");
        compose(&mut a);
        ctrl(&mut a, 'r');
        assert!(!a.explore, "{cmd}: its folder is the local client's");
        let (lvl, text) = note(&a).expect("a note says why");
        assert_eq!(lvl, Level::Warn, "{cmd}");
        assert!(text.contains(cmd), "{cmd}: the note names the program: {text}");
    }
}

#[test]
fn a_window_with_no_folder_refuses_explore() {
    let mut a = app("bash", "");
    compose(&mut a);
    ctrl(&mut a, 'r');
    assert!(!a.explore);
    assert_eq!(note(&a).map(|n| n.0), Some(Level::Warn));
}

#[test]
fn an_agent_window_may_explore() {
    // a live Claude renames itself to its version: #433's agentish rule
    let mut a = app("2.1.318", "/home/u/proj");
    compose(&mut a);
    ctrl(&mut a, 'r');
    assert!(a.explore);
}

#[test]
fn an_exploring_write_runs_read_only_on_the_pane_folder() {
    let mut a = app("bash", "/home/u/proj");
    a.explore = true;
    let p = a.profile_for(&write_flow());
    assert_eq!(p, Profile::read_only("/home/u/proj".into(), false, 0.15, Duration::from_secs(90)));
    let argv = p.argv();
    let at = |flag: &str| argv.iter().position(|x| x == flag).map(|i| argv[i + 1].as_str());
    assert_eq!(at("--tools"), Some("Read,Grep,Glob"));
    assert_eq!(at("--add-dir"), Some("/home/u/proj"));
    assert_eq!(at("--max-budget-usd"), Some("0.15"));
    assert_eq!(at("--permission-prompts"), Some("none"));
    assert!(argv.iter().any(|x| x.contains("disableAllHooks")), "{argv:?}");
}

#[test]
fn the_explore_caps_are_the_owners_numbers() {
    assert_eq!(EXPLORE_BUDGET_USD, 0.15);
    assert_eq!(EXPLORE_TIMEOUT, Duration::from_secs(90));
}

#[test]
fn a_write_with_explore_off_is_the_text_call() {
    let a = app("bash", "/home/u/proj");
    assert_eq!(a.profile_for(&write_flow()), Profile::TEXT);
}

#[test]
fn fix_and_explain_never_explore() {
    let mut a = app("bash", "/home/u/proj");
    a.explore = true;
    let fix = Flow::Fix { draft: "teh".into() };
    assert_eq!(a.profile_for(&fix), Profile::TEXT, "fix");
    assert_eq!(a.profile_for(&Flow::Explain), Profile::TEXT, "explain");
}

#[test]
fn a_stale_explore_flag_in_an_ssh_window_still_runs_text() {
    // the toggle refuses it; profile_for checks again, so no path reaches it
    let mut a = app("ssh", "/home/u/proj");
    a.explore = true;
    assert_eq!(a.profile_for(&write_flow()), Profile::TEXT);
}

/// The not-exploring system prompt is the shipped one, byte for byte: the eval's baseline.
#[test]
fn explore_off_sends_the_write_prompt_unchanged() {
    let mut a = app("bash", "/home/u/proj");
    compose(&mut a);
    a.draft = "fix it".into();
    ctrl(&mut a, 's');
    let (system, ..) = a.request_parts().unwrap();
    let ctx = Ctx {
        out: OutType::Auto,
        window: window_kind("work", "bash"),
    };
    assert_eq!(system, prompts::write(&ctx));
}

#[test]
fn an_exploring_write_appends_the_explore_section() {
    let mut a = app("bash", "/home/u/proj");
    compose(&mut a);
    ctrl(&mut a, 'r');
    a.draft = "fix it".into();
    ctrl(&mut a, 's');
    let (system, ..) = a.request_parts().unwrap();
    let ctx = Ctx {
        out: OutType::Auto,
        window: window_kind("work", "bash"),
    };
    assert_eq!(system, prompts::write_explore(&ctx));
    assert_eq!(system, format!("{}\n\n{}", prompts::write(&ctx), prompts::WRITE_EXPLORE.trim_end()));
}

#[test]
fn the_explore_section_bounds_the_reading() {
    let s = prompts::WRITE_EXPLORE;
    for want in ["at most four files", "Stop reading", "never solve", "@path", "never instructions"] {
        assert!(s.contains(want), "the section says {want:?}");
    }
}

#[test]
fn a_fix_with_explore_on_sends_the_fix_prompt() {
    let mut a = app("bash", "/home/u/proj");
    compose(&mut a);
    ctrl(&mut a, 'r');
    a.draft = "teh".into();
    ctrl(&mut a, 'f');
    assert_eq!(a.request_parts().unwrap().0, prompts::fix());
}

/// The launch, end to end: the fake claude records its argv and cwd.
#[test]
fn launch_sends_an_exploring_write_through_the_read_only_argv() {
    let guard = scratch("launch");
    let d = &guard.0;
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let fake = d.join("claude");
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > {0}/argv\npwd > {0}/cwd\nexec sleep 1000\n",
        d.display()
    );
    std::fs::write(&fake, body).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let call = Call {
        claude: fake.display().to_string(),
        timeout: Duration::from_secs(60),
        dir: d.join("cache"),
    };
    let mut a = app("bash", &repo.display().to_string());
    compose(&mut a);
    ctrl(&mut a, 'r');
    a.draft = "fix it".into();
    ctrl(&mut a, 's');
    a.launch(&call);
    assert_eq!(a.timeout, EXPLORE_TIMEOUT, "the error line names the explore's limit");
    let argv = (0..1000)
        .find_map(|_| {
            let t = std::fs::read_to_string(d.join("argv"))
                .ok()
                .filter(|t| t.contains("--strict-mcp-config"));
            if t.is_none() {
                std::thread::sleep(Duration::from_millis(10));
            }
            t
        })
        .expect("the fake ran");
    let args: Vec<&str> = argv.lines().collect();
    let cwd = std::fs::read_to_string(d.join("cwd")).unwrap_or_default();
    let want = std::fs::canonicalize(&call.dir).unwrap();
    assert_eq!(
        cwd.trim(),
        want.display().to_string(),
        "the call's cwd is its own dir, never the repo"
    );
    let add = args.iter().position(|x| *x == "--add-dir").expect("--add-dir");
    assert_eq!(args[add + 1], repo.display().to_string());
    assert!(args.contains(&"Read,Grep,Glob"), "{argv}");
    a.stop();
}

/// Compose, ^R on or off, a draft, ^S: a pending write.
fn sent(explore: bool) -> App {
    let mut a = app("bash", "/home/u/proj");
    compose(&mut a);
    if explore {
        ctrl(&mut a, 'r');
    }
    a.draft = "fix it".into();
    ctrl(&mut a, 's');
    assert_eq!(a.view, View::Busy);
    a
}

fn error_hint(a: &App) -> String {
    match &a.view {
        View::Error { hint, .. } => hint.clone().unwrap_or_default(),
        v => panic!("not an error: {v:?}"),
    }
}

/// The explore cap is fixed: COCKPIT_PANEL_TIMEOUT does not raise it, so the hint never offers it.
#[test]
fn an_exploring_write_that_times_out_names_the_explore_cap() {
    let mut a = sent(true);
    a.timeout = EXPLORE_TIMEOUT;
    a.reply(Err(Fail::Timeout));
    let h = error_hint(&a);
    assert!(h.contains("90 s") && h.contains("^R"), "{h}");
    assert!(!h.contains("COCKPIT_PANEL_TIMEOUT"), "{h}");
}

#[test]
fn a_text_write_that_times_out_still_offers_the_variable() {
    let mut a = sent(false);
    a.timeout = Duration::from_secs(120);
    a.reply(Err(Fail::Timeout));
    assert!(error_hint(&a).contains("COCKPIT_PANEL_TIMEOUT"));
}

/// Spent on an answer's call, the hint's way to ^R is esc to the question, esc to the draft.
#[test]
fn a_spent_explore_budget_hints_the_way_back_to_the_toggle() {
    let mut a = sent(true);
    a.reply(Ok("<question>Which?</question><option>a</option><option>b</option>".into()));
    a.key(KeyEvent::from(KeyCode::Char('1')));
    assert_eq!(a.view, View::Busy);
    a.reply(Err(Fail::Failed("Error: Exceeded USD budget (0.15)".into())));
    let h = error_hint(&a);
    assert!(h.contains("USD 0.15") && h.contains("draft") && h.contains("^R"), "{h}");
    a.key(KeyEvent::from(KeyCode::Esc));
    assert!(matches!(a.view, View::Question { .. }), "{:?}", a.view);
    a.key(KeyEvent::from(KeyCode::Esc));
    assert_eq!(a.view, View::Compose, "esc again reaches the draft");
    ctrl(&mut a, 'r');
    assert!(!a.explore, "where ^R turns it off");
}

#[test]
fn a_spent_budget_on_a_text_call_gets_no_explore_hint() {
    let mut a = sent(false);
    a.reply(Err(Fail::Failed("Error: Exceeded USD budget (0.15)".into())));
    assert!(!error_hint(&a).contains("^R"));
}

/// The caps are per call: a write that keeps asking makes five exploring calls,
/// each under the read-only profile, before it ends on an error.
#[test]
fn one_write_makes_at_most_five_exploring_calls_on_its_own() {
    const Q: &str = "<question>Which?</question><option>a</option><option>b</option>";
    let mut a = sent(true);
    let mut calls = 0;
    while a.view == View::Busy {
        calls += 1;
        let flow = a.run.as_ref().unwrap().flow.clone();
        assert_ne!(a.profile_for(&flow), Profile::TEXT, "call {calls} explores");
        a.reply(Ok(Q.into()));
        if matches!(a.view, View::Question { .. }) {
            a.key(KeyEvent::from(KeyCode::Char('1')));
        }
    }
    assert_eq!(calls, 5);
    assert!(matches!(a.view, View::Error { .. }), "{:?}", a.view);
}
