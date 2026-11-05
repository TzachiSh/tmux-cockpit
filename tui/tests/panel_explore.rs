//! Explore first (#416): a Compose toggle that lets a write read the pane's
//! folder through a read-only profile. Fake claude scripts in temp dirs; no tmux.
use cockpit_tui::panel::app::{App, EXPLORE_BUDGET_USD, EXPLORE_TIMEOUT, Level, Tile, View};
use cockpit_tui::panel::capture::{Capture, window_kind};
use cockpit_tui::panel::envelope::{Flow, OutType};
use cockpit_tui::panel::prompts::{self, Ctx};
use cockpit_tui::panel::request::{Call, Fail, Profile, read_dir_ok};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
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

/// A fork made while another test holds a fake open for writing carries that fd
/// to its exec, which then fails ETXTBSY: writes and spawns take turns, as in panel_request.
static FORK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn forking() -> std::sync::MutexGuard<'static, ()> {
    FORK.lock().unwrap_or_else(|e| e.into_inner())
}

/// `body` as an executable at `path`, written under the fork lock.
fn fake_at(path: &Path, body: &str) {
    let _g = forking();
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
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
    // what read_only passes is request.rs's to pin; this PR owns the folder and the caps
    let argv = p.argv();
    let at = |flag: &str| argv.iter().position(|x| x == flag).map(|i| argv[i + 1].as_str());
    assert_eq!(at("--add-dir"), Some("/home/u/proj"));
    assert_eq!(at("--max-budget-usd"), Some("0.15"));
    assert_eq!(p.timeout, Some(Duration::from_secs(90)));
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
    // cwd first, and argv renamed in whole: the test never reads a file half written
    let body = format!(
        "#!/bin/sh\npwd > {0}/cwd\nprintf '%s\\n' \"$@\" > {0}/argv.tmp\nmv {0}/argv.tmp {0}/argv\nexec sleep 1000\n",
        d.display()
    );
    fake_at(&fake, &body);
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
    {
        let _g = forking();
        a.launch(&call);
    }
    assert_eq!(a.timeout, EXPLORE_TIMEOUT, "the error line names the explore's limit");
    let argv = (0..1000)
        .find_map(|_| {
            let t = std::fs::read_to_string(d.join("argv")).ok();
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
    let at = args
        .iter()
        .position(|x| *x == "--system-prompt-file")
        .expect("a system prompt file");
    let want = Profile::read_only(repo.clone(), false, EXPLORE_BUDGET_USD, EXPLORE_TIMEOUT).argv();
    assert_eq!(args[at + 2..], want.iter().map(String::as_str).collect::<Vec<_>>()[..], "{argv}");
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
    error(a).1
}

/// The Error view's line and hint.
fn error(a: &App) -> (String, String) {
    match &a.view {
        View::Error { line, hint } => (line.clone(), hint.clone().unwrap_or_default()),
        v => panic!("not an error: {v:?}"),
    }
}

/// The explore cap is fixed: COCKPIT_PANEL_TIMEOUT does not raise it, so the hint never offers it.
#[test]
fn an_exploring_write_that_times_out_names_the_explore_cap() {
    let mut a = sent(true);
    a.timeout = EXPLORE_TIMEOUT;
    a.reply(Err(Fail::Timeout));
    let (line, h) = error(&a);
    assert!(line.contains("90 s"), "the line names the cap: {line}");
    assert!(h.contains("draft") && h.contains("^R"), "{h}");
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
    a.reply(Err(Fail::Budget { limit_usd: 0.15 }));
    let (line, h) = error(&a);
    assert!(line.contains("USD 0.15"), "the line names the cap: {line}");
    assert!(h.contains("draft") && h.contains("^R"), "{h}");
    a.key(KeyEvent::from(KeyCode::Esc));
    assert!(matches!(a.view, View::Question { .. }), "{:?}", a.view);
    a.key(KeyEvent::from(KeyCode::Esc));
    assert_eq!(a.view, View::Compose, "esc again reaches the draft");
    ctrl(&mut a, 'r');
    assert!(!a.explore, "where ^R turns it off");
}

/// A unit check on fail_hint: a text call has no budget, so this stop is fed in by hand.
#[test]
fn a_budget_stop_on_a_text_call_gets_no_explore_hint() {
    let mut a = sent(false);
    a.reply(Err(Fail::Budget { limit_usd: 0.15 }));
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

// ---------------------------------------------------------------- the folder check (#465)

#[test]
fn the_root_folder_is_refused_with_request_rs_words() {
    let mut a = app("bash", "/");
    compose(&mut a);
    ctrl(&mut a, 'r');
    assert!(!a.explore);
    let (lvl, text) = note(&a).expect("a note");
    assert_eq!(lvl, Level::Warn);
    let why = read_dir_ok(Path::new("/")).unwrap_err();
    assert_eq!(text, format!("explore refused: {why}"));
}

#[test]
fn home_and_the_folders_above_it_are_refused() {
    let home = std::env::var("HOME").expect("HOME");
    let above = Path::new(&home).parent().expect("home has a parent").display().to_string();
    for dir in [home.as_str(), above.as_str()] {
        let mut a = app("bash", dir);
        compose(&mut a);
        ctrl(&mut a, 'r');
        assert!(!a.explore, "{dir}");
        assert_eq!(a.profile_for(&write_flow()), Profile::TEXT, "{dir}");
    }
}

#[test]
fn a_folder_with_a_glob_character_is_refused() {
    let mut a = app("bash", "/srv/a*b");
    compose(&mut a);
    ctrl(&mut a, 'r');
    assert!(!a.explore);
    assert!(note(&a).unwrap().1.contains("glob"), "{:?}", note(&a));
}

#[test]
fn a_plain_project_folder_is_allowed() {
    let d = scratch("plain");
    let mut a = app("bash", &d.0.display().to_string());
    compose(&mut a);
    ctrl(&mut a, 'r');
    assert!(a.explore, "{:?}", note(&a));
}

// ---------------------------------------------------------------- files read (#416)

/// A pane folder and a fake claude that prints `out` on stdout and exits `code`.
fn faked(tag: &str, out: &str, code: i32) -> (Scratch, Call, String) {
    let g = scratch(tag);
    let d = &g.0;
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(d.join("out"), out.replace("$R", &repo.display().to_string())).unwrap();
    let fake = d.join("claude");
    let body = format!("#!/bin/sh\ncat >/dev/null\ncat {}/out\nexit {code}\n", d.display());
    fake_at(&fake, &body);
    let call = Call {
        claude: fake.display().to_string(),
        timeout: Duration::from_secs(30),
        dir: d.join("cache"),
    };
    (g, call, repo.display().to_string())
}

/// A write on `repo`, explore on or off, launched and polled to its answer.
fn answered(call: &Call, repo: &str, explore: bool) -> App {
    let mut a = app("bash", repo);
    compose(&mut a);
    if explore {
        ctrl(&mut a, 'r');
    }
    a.draft = "fix it".into();
    ctrl(&mut a, 's');
    {
        let _g = forking();
        a.launch(call);
    }
    let done = (0..1000).any(|_| {
        let got = a.poll();
        if !got {
            std::thread::sleep(Duration::from_millis(10));
        }
        got
    });
    assert!(done, "no answer in 10 s");
    a
}

fn read(id: &str, path: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"Read","input":{{"file_path":"{path}"}}}}]}}}}"#
    )
}

fn result(id: &str, error: bool) -> String {
    format!(r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}","is_error":{error}}}]}}}}"#)
}

const OK: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":"<prompt>Fix @src/a.rs</prompt>"}"#;
const SPENT: &str = r#"{"type":"result","subtype":"error_max_budget_usd","is_error":true,"errors":["Reached maximum budget ($0.15)"]}"#;

#[test]
fn an_exploring_result_notes_the_files_it_read() {
    let out = format!(
        "[{},{},{},{},{OK}]",
        read("a", "$R/src/a.rs"),
        result("a", false),
        read("b", "$R/lib/b.rs"),
        result("b", false)
    );
    let (_g, call, repo) = faked("reads", &out, 0);
    let a = answered(&call, &repo, true);
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
    let (lvl, text) = note(&a).expect("a note");
    assert_eq!((lvl, text), (Level::Info, "read 2 files: src/a.rs, lib/b.rs"));
}

#[test]
fn a_refused_read_is_not_counted() {
    let out = format!(
        "[{},{},{},{},{OK}]",
        read("a", "/etc/passwd"),
        result("a", true),
        read("b", "$R/src/a.rs"),
        result("b", false)
    );
    let (_g, call, repo) = faked("refused", &out, 0);
    let a = answered(&call, &repo, true);
    assert_eq!(note(&a).map(|n| n.1), Some("read 1 file: src/a.rs"));
}

#[test]
fn an_exploring_call_that_read_nothing_says_so() {
    let (_g, call, repo) = faked("none", &format!("[{OK}]"), 0);
    let a = answered(&call, &repo, true);
    assert_eq!(note(&a).map(|n| n.1), Some("read no files"));
}

#[test]
fn a_text_call_gets_no_reads_note() {
    let (_g, call, repo) = faked("text", "<prompt>Fix it</prompt>", 0);
    let a = answered(&call, &repo, false);
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
    assert_eq!(note(&a), None);
}

/// A budget stop, in claude's json, after a read: the Error says what was read and how to stop.
#[test]
fn a_budget_stop_notes_its_reads_and_hints_the_toggle() {
    let out = format!("[{},{},{SPENT}]", read("a", "$R/src/a.rs"), result("a", false));
    let (_g, call, repo) = faked("spent", &out, 1);
    let a = answered(&call, &repo, true);
    let (line, h) = error(&a);
    assert!(line.contains("USD 0.15"), "{line}");
    assert!(h.contains("^R"), "{h}");
    assert_eq!(note(&a).map(|n| n.1), Some("read 1 file: src/a.rs"));
}

/// A reply cut off before its closing tag keeps its warning, with the reads after it.
#[test]
fn the_reads_join_a_warning_and_never_replace_it() {
    let cut = r#"{"type":"result","subtype":"success","is_error":false,"result":"<prompt>Fix @src/a.rs"}"#;
    let out = format!("[{},{},{cut}]", read("a", "$R/src/a.rs"), result("a", false));
    let (_g, call, repo) = faked("cut", &out, 0);
    let a = answered(&call, &repo, true);
    let (lvl, text) = note(&a).expect("a note");
    assert_eq!(lvl, Level::Warn);
    assert!(text.contains("cut off") && text.ends_with("; read 1 file: src/a.rs"), "{text}");
}

#[test]
fn many_reads_list_three_names_and_count_the_rest() {
    let mut ev = vec![];
    for (i, f) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        ev.push(read(&i.to_string(), &format!("$R/{f}.rs")));
        ev.push(result(&i.to_string(), false));
    }
    let (_g, call, repo) = faked("many", &format!("[{},{OK}]", ev.join(",")), 0);
    let a = answered(&call, &repo, true);
    assert_eq!(note(&a).map(|n| n.1), Some("read 5 files: a.rs, b.rs, c.rs and 2 more"));
}

/// claude's API error: `subtype` success, `is_error` true. An Error in claude's
/// words whatever the exit, and no reads note for a call that read nothing.
#[test]
fn an_api_error_marked_success_is_an_error_with_no_reads_note() {
    let api = r#"[{"type":"result","subtype":"success","is_error":true,"result":"API Error: 400 bad request"}]"#;
    for code in [1, 0] {
        let (_g, call, repo) = faked(&format!("api{code}"), api, code);
        let a = answered(&call, &repo, true);
        let (line, _) = error(&a);
        assert!(line.contains("API Error: 400 bad request"), "exit {code}: {line}");
        assert_eq!(note(&a), None, "exit {code}");
    }
}

/// The same error after a read: the read was paid for, so the Error says so.
#[test]
fn an_api_error_after_a_read_still_notes_it() {
    let api = r#"{"type":"result","subtype":"success","is_error":true,"result":"API Error: 500 overloaded"}"#;
    let out = format!("[{},{},{api}]", read("a", "$R/src/a.rs"), result("a", false));
    let (_g, call, repo) = faked("api-read", &out, 1);
    let a = answered(&call, &repo, true);
    assert!(error(&a).0.contains("API Error: 500"), "{:?}", a.view);
    assert_eq!(note(&a).map(|n| n.1), Some("read 1 file: src/a.rs"));
}

// ---------------------------------------------------------------- the repo, not the pane's folder

fn git_init(dir: &Path) {
    let ok = std::process::Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init", "-q"])
        .arg(dir)
        .status()
        .unwrap();
    assert!(ok.success());
}

/// The folder the explore call would add, from its argv.
fn added(a: &App) -> Option<String> {
    let argv = a.profile_for(&write_flow()).argv();
    let i = argv.iter().position(|x| x == "--add-dir")?;
    Some(argv[i + 1].clone())
}

#[test]
fn a_pane_in_a_repos_subfolder_explores_the_repo() {
    let g = scratch("sub");
    let repo = g.0.join("repo");
    std::fs::create_dir_all(repo.join("src/deep")).unwrap();
    git_init(&repo);
    let mut a = app("bash", &repo.join("src/deep").display().to_string());
    compose(&mut a);
    ctrl(&mut a, 'r');
    assert!(a.explore, "{:?}", note(&a));
    let root = std::fs::canonicalize(&repo).unwrap().display().to_string();
    assert_eq!(added(&a), Some(root.clone()));
    assert!(
        note(&a).unwrap().1.contains(&root),
        "the note names what it may read: {:?}",
        note(&a)
    );
}

#[test]
fn a_folder_outside_any_repo_keeps_the_pane_folder() {
    let g = scratch("norepo");
    let dir = g.0.join("notes");
    std::fs::create_dir_all(&dir).unwrap();
    let mut a = app("bash", &dir.display().to_string());
    a.explore = true;
    assert_eq!(added(&a), Some(dir.display().to_string()));
}

/// A file name is the model's to report: a bidi or control character in it never reaches the screen.
#[test]
fn a_read_path_with_a_bidi_mark_is_noted_clean() {
    let out = format!("[{},{},{OK}]", read("a", "$R/src/a\\u202eb\\u0007.rs"), result("a", false));
    let (_g, call, repo) = faked("bidi", &out, 0);
    let a = answered(&call, &repo, true);
    assert_eq!(note(&a).map(|n| n.1), Some("read 1 file: src/ab.rs"));
}
