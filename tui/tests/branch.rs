//! The branch popup (#246): what git is asked, which key prints which action
//! line, and goldens at 40/60/80/120 and the floor. Scratch repos only; no
//! tmux, no tty, no clock -- commit dates are fixed and goldens use literal ages.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
use cockpit_tui::branch::{self, Ui};
use cockpit_tui::data::branches::{self, Branch};
use cockpit_tui::pick::Outcome;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn golden(name: &str, got: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    let path = format!("{dir}/{name}.txt");
    if std::env::var("COCKPIT_BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path}; run COCKPIT_BLESS=1 cargo test"));
    assert_eq!(want, got, "\n{name} changed:\n--- want ---\n{want}\n--- got ---\n{got}");
}

static N: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "cockpit-branch-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn s(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

fn git(dir: &Path, date: &str, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// main (newest) and, older, old, held and a shell-metacharacter name; a bare remote holding
/// main and `only-remote`, fetched; a linked worktree on `held`.
fn repo() -> (Scratch, Scratch, Scratch) {
    let (r, rm, wt) = (Scratch::new(), Scratch::new(), Scratch::new());
    git(&r.0, "2020-01-01T00:00:00Z", &["init", "-q", "-b", "main", "."]);
    git(&r.0, "2020-01-01T00:00:00Z", &["commit", "-q", "--allow-empty", "-m", "init"]);
    git(&r.0, "2020-01-02T00:00:00Z", &["branch", "old"]);
    git(&r.0, "2020-01-02T00:00:00Z", &["branch", "feat/$(touch${IFS}x);`id`"]);
    git(&r.0, "2020-01-02T00:00:00Z", &["branch", "held"]);
    git(&r.0, "2020-01-03T00:00:00Z", &["commit", "-q", "--allow-empty", "-m", "second"]);
    git(&rm.0, "2020-01-01T00:00:00Z", &["init", "-q", "--bare", "."]);
    git(&r.0, "2020-01-01T00:00:00Z", &["remote", "add", "origin", rm.s()]);
    git(&r.0, "2020-01-01T00:00:00Z", &["branch", "only-remote"]);
    git(&r.0, "2020-01-01T00:00:00Z", &["push", "-q", "origin", "main", "only-remote"]);
    git(&r.0, "2020-01-01T00:00:00Z", &["branch", "-q", "-D", "only-remote"]);
    git(&r.0, "2020-01-01T00:00:00Z", &["fetch", "-q", "origin"]);
    std::fs::remove_dir_all(&wt.0).unwrap();
    git(&r.0, "2020-01-01T00:00:00Z", &["worktree", "add", "-q", wt.s(), "held"]);
    (r, rm, wt)
}

fn names(bs: &[Branch]) -> Vec<&str> {
    bs.iter().map(|b| b.name.as_str()).collect()
}

#[test]
fn list_is_local_by_recency_then_remote_only() {
    let (r, _rm, _wt) = repo();
    let bs = branches::list(r.s());
    // main is the newest commit; the rest share a date and git breaks the tie by name
    assert_eq!(names(&bs)[0], "main");
    assert_eq!(names(&bs).last(), Some(&"origin/only-remote"));
    assert!(!names(&bs).contains(&"origin/main"), "a remote with a local twin is listed once");
    assert!(!names(&bs).iter().any(|n| n.ends_with("/HEAD")));
    assert_eq!(bs.iter().filter(|b| b.remote).count(), 1);
}

#[test]
fn current_and_other_worktree_are_marked() {
    let (r, _rm, wt) = repo();
    let bs = branches::list(r.s());
    let get = |n: &str| bs.iter().find(|b| b.name == n).unwrap();
    assert!(get("main").current);
    assert!(!get("old").current);
    let leaf = wt.0.file_name().unwrap().to_str().unwrap();
    assert_eq!(get("held").elsewhere.as_deref(), Some(leaf));
    assert_eq!(get("old").elsewhere, None);
    // from inside the linked worktree its own branch is current, and main is the one held elsewhere
    let inside = branches::list(wt.s());
    let main = inside.iter().find(|b| b.name == "main").unwrap();
    assert!(inside.iter().find(|b| b.name == "held").unwrap().current);
    assert!(main.elsewhere.is_some());
    assert!(inside.iter().find(|b| b.name == "held").unwrap().elsewhere.is_none());
}

#[test]
fn a_shell_metacharacter_name_is_listed_and_keyed_literally() {
    let (r, _rm, _wt) = repo();
    let bs = branches::list(r.s());
    assert!(names(&bs).contains(&"feat/$(touch${IFS}x);`id`"));
    let ui = Ui::new(r.s(), bs, "", &Theme::fixed().g, None);
    let keys: Vec<String> = ui.pick.rows.iter().map(|r| String::from_utf8(r.key.clone()).unwrap()).collect();
    assert!(keys.contains(&"switch feat/$(touch${IFS}x);`id`".to_string()), "{keys:?}");
    assert!(!r.0.join("x").exists());
}

#[test]
fn valid_name_follows_git() {
    let (r, _rm, _wt) = repo();
    for (n, want) in [
        ("fresh", true),
        ("feat/ok-1", true),
        ("", false),
        ("has space", false),
        ("a..b", false),
        ("-dash", false),
        ("@{-1}", false),
        ("end.lock", false),
        ("tab\there", false),
    ] {
        assert_eq!(branches::valid_name(r.s(), n), want, "{n:?}");
    }
}

#[test]
fn graph_lists_the_selected_branchs_commits() {
    let (r, _rm, _wt) = repo();
    let bs = branches::list(r.s());
    let main = bs.iter().find(|b| b.name == "main").unwrap();
    let g = branches::graph(r.s(), main);
    assert!(g.contains("second") && g.contains("init"), "{g}");
    let old = bs.iter().find(|b| b.name == "old").unwrap();
    assert!(!branches::graph(r.s(), old).contains("second"));
}

fn b(name: &str, remote: bool, age: &str) -> Branch {
    Branch {
        name: name.into(),
        remote,
        age: age.into(),
        current: false,
        elsewhere: None,
    }
}

fn fixture() -> Vec<Branch> {
    let mut v = vec![
        b("main", false, "2 hours ago"),
        b("feat/246-branch", false, "3 hours ago"),
        b("fix/Left-Click", false, "2 days ago"),
        b("wip/長い名前", false, "3 weeks ago"),
        b("origin/release/0.2", true, "5 weeks ago"),
    ];
    v[0].current = true;
    v[2].elsewhere = Some("wt-fix".into());
    v
}

fn ui(bs: Vec<Branch>) -> Ui {
    Ui::new(".", bs, "on main · repo", &Theme::fixed().g, None)
}

const BIG: Rect = Rect::new(0, 0, 80, 24);

fn press(u: &mut Ui, code: KeyCode, m: KeyModifiers) -> Outcome {
    u.key(KeyEvent::new(code, m), BIG)
}

fn type_str(u: &mut Ui, s: &str) {
    for c in s.chars() {
        press(u, KeyCode::Char(c), KeyModifiers::NONE);
    }
}

fn choose(u: &mut Ui) -> String {
    match press(u, KeyCode::Enter, KeyModifiers::NONE) {
        Outcome::Choose(k) => String::from_utf8(k).unwrap(),
        o => panic!("not chosen: {o:?}"),
    }
}

#[test]
fn enter_prints_switch_for_a_local_and_track_for_a_remote_only() {
    let mut u = ui(fixture());
    assert_eq!(choose(&mut u), "switch main");
    type_str(&mut u, "release");
    assert_eq!(choose(&mut u), "track origin/release/0.2");
}

#[test]
fn a_branch_held_by_another_worktree_gives_a_note_and_no_action() {
    let mut u = ui(fixture());
    type_str(&mut u, "left");
    assert_eq!(press(&mut u, KeyCode::Enter, KeyModifiers::NONE), Outcome::Continue);
    assert!(
        u.pick.header.starts_with("✕ fix/Left-Click is checked out in the worktree wt-fix"),
        "{}",
        u.pick.header
    );
    // the next key clears the note
    type_str(&mut u, "x");
    assert!(!u.pick.header.starts_with('✕'));
}

#[test]
fn alt_enter_creates_from_the_filter_text_after_git_validates_it() {
    let (r, _rm, _wt) = repo();
    let mut u = Ui::new(r.s(), branches::list(r.s()), "", &Theme::fixed().g, None);
    type_str(&mut u, "brand-new");
    assert_eq!(
        press(&mut u, KeyCode::Enter, KeyModifiers::ALT),
        Outcome::Choose(b"create brand-new".to_vec())
    );
}

#[test]
fn alt_enter_refuses_an_empty_or_invalid_name_with_a_note() {
    let (r, _rm, _wt) = repo();
    let mut u = Ui::new(r.s(), branches::list(r.s()), "", &Theme::fixed().g, None);
    assert_eq!(press(&mut u, KeyCode::Enter, KeyModifiers::ALT), Outcome::Continue);
    assert!(u.pick.header.contains("type a name"), "{}", u.pick.header);
    type_str(&mut u, "bad..name");
    assert_eq!(press(&mut u, KeyCode::Enter, KeyModifiers::ALT), Outcome::Continue);
    assert!(u.pick.header.contains("not a valid branch name: bad..name"), "{}", u.pick.header);
}

#[test]
fn esc_and_ctrl_c_cancel() {
    let mut u = ui(fixture());
    assert_eq!(press(&mut u, KeyCode::Esc, KeyModifiers::NONE), Outcome::Cancel);
    assert_eq!(press(&mut u, KeyCode::Char('c'), KeyModifiers::CONTROL), Outcome::Cancel);
}

#[test]
fn a_control_character_in_a_name_is_not_drawn_but_the_key_keeps_the_ref() {
    let u = ui(vec![b("a\x1b[31mb\u{202e}c", false, "now")]);
    assert!(!u.pick.rows[0].text.contains('\x1b') && !u.pick.rows[0].text.contains('\u{202e}'));
    assert_eq!(u.pick.rows[0].key, "switch a\x1b[31mb\u{202e}c".as_bytes());
}

#[test]
fn the_selection_asks_the_worker_once_and_shows_loading_until_it_answers() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut u = Ui::new(".", fixture(), "", &Theme::fixed().g, Some(tx));
    assert!(u.pick.preview.as_deref().unwrap().contains("loading"));
    press(&mut u, KeyCode::Down, KeyModifiers::NONE);
    press(&mut u, KeyCode::Up, KeyModifiers::NONE);
    let asked: Vec<String> = rx.try_iter().collect();
    assert_eq!(asked, ["main", "feat/246-branch"], "each branch is asked once");
    u.feed("main".into(), "* abc init\n");
    assert_eq!(u.pick.preview.as_deref(), Some("* abc init"));
}

#[test]
fn the_worker_answers_with_the_graph_of_the_branch_asked() {
    let (r, _rm, _wt) = repo();
    let bs = branches::list(r.s());
    let (tx, rx) = branch::worker(r.s().into(), bs);
    tx.send("main".into()).unwrap();
    // blocks until the answer: a hang is the failure, not a clock
    let (name, text) = rx.recv().unwrap();
    assert_eq!(name, "main");
    assert!(text.contains("second"), "{text}");
}

#[test]
fn bad_arguments_are_named() {
    let a = |v: &[&str]| branch::parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(a(&[]), Ok(".".into()));
    assert_eq!(a(&["-C", "/x"]), Ok("/x".into()));
    assert!(a(&["-C"]).unwrap_err().contains("-C needs"));
    assert!(a(&["-C", ""]).is_err());
    assert!(a(&["--bogus"]).unwrap_err().contains("--bogus"));
}

fn draw(w: u16, h: u16, u: &Ui) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| branch::draw(f, f.area(), u, &Theme::fixed())).unwrap();
    t.backend().to_string()
}

fn shot(name: &str, w: u16, h: u16) {
    let mut u = ui(fixture());
    u.feed(
        "main".into(),
        "* dea82ca (HEAD -> main) Merge pull request #238\n|\\\n| * 3f1c2d0 Add a Rust picker\n|/\n* 9b8a7c6 Initial commit\n",
    );
    u.pick.clamp(Rect::new(0, 0, w, h));
    golden(name, &draw(w, h, &u));
}

#[test]
fn goldens() {
    for (w, h) in [(40, 14), (60, 16), (80, 24), (120, 24)] {
        shot(&format!("branch-{w}"), w, h);
    }
    shot("branch-floor", 24, 8);
}

#[test]
fn golden_with_a_note() {
    let mut u = ui(fixture());
    type_str(&mut u, "left");
    press(&mut u, KeyCode::Enter, KeyModifiers::NONE);
    u.feed("fix/Left-Click".into(), "* 1a2b3c4 Fix the click\n");
    golden("branch-note-80", &draw(80, 24, &u));
}
