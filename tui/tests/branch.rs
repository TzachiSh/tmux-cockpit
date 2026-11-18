//! The branch popup (#246): what git is asked, which key gives which action
//! line, what the popup's own switch, track and create do (#228), and goldens at
//! 40/60/80/120 and the floor. Scratch repos only; no tmux, no tty, no clock --
//! commit dates are fixed and goldens use literal ages.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
#[allow(dead_code)] // for its Scratch guard and write_exec only
mod fake;

use cockpit_tui::branch::{self, Ui};
use cockpit_tui::data::branches::{self, Branch};
use cockpit_tui::pick::Outcome;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use fake::Scratch;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::path::Path;
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
    let full = std::fs::canonicalize(&wt.0).unwrap();
    assert_eq!(get("held").elsewhere.as_deref(), full.to_str(), "the full path, not its leaf");
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
    v[2].elsewhere = Some("/work/wt-fix".into());
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
fn enter_on_a_branch_held_by_another_worktree_prints_goto_with_its_full_path() {
    let mut u = ui(fixture());
    type_str(&mut u, "left");
    assert_eq!(choose(&mut u), "goto /work/wt-fix");
}

#[test]
fn a_path_that_cannot_ride_one_line_gives_a_note_and_no_action() {
    let mut bs = fixture();
    bs[2].elsewhere = Some("/work/wt\nfix".into());
    let mut u = ui(bs);
    type_str(&mut u, "left");
    assert_eq!(press(&mut u, KeyCode::Enter, KeyModifiers::NONE), Outcome::Continue);
    assert!(
        u.pick.header.starts_with("✕ fix/Left-Click is checked out in the worktree"),
        "{}",
        u.pick.header
    );
    // the next key clears the note
    type_str(&mut u, "x");
    assert!(!u.pick.header.starts_with('✕'));
}

#[test]
fn a_worktree_path_with_a_newline_is_listed_whole_and_gets_a_note() {
    let (r, _rm, wt) = repo();
    git(&r.0, "2020-01-01T00:00:00Z", &["worktree", "remove", "--force", wt.s()]);
    let odd = r.0.join("wt\nnl");
    git(
        &r.0,
        "2020-01-01T00:00:00Z",
        &["worktree", "add", "-q", odd.to_str().unwrap(), "held"],
    );
    let bs = branches::list(r.s());
    let held = bs.iter().find(|b| b.name == "held").unwrap();
    assert_eq!(held.elsewhere.as_deref(), std::fs::canonicalize(&odd).unwrap().to_str());
    let mut u = ui(bs);
    type_str(&mut u, "held");
    assert_eq!(press(&mut u, KeyCode::Enter, KeyModifiers::NONE), Outcome::Continue);
    assert!(u.pick.header.starts_with("✕ held is checked out"), "{}", u.pick.header);
}

#[test]
fn the_header_says_detached_head_when_no_branch_is_current() {
    let (r, _rm, _wt) = repo();
    let g = Theme::fixed().g;
    let on = |r: &Scratch| branch::header(r.s(), &branches::list(r.s()), &g);
    assert!(on(&r).starts_with("on main "), "{}", on(&r));
    git(&r.0, "2020-01-01T00:00:00Z", &["checkout", "-q", "--detach"]);
    assert!(on(&r).starts_with("on detached HEAD "), "{}", on(&r));
}

#[test]
fn golden_detached_head() {
    let mut bs = fixture();
    bs[0].current = false;
    let mut u = Ui::new(".", bs, "on detached HEAD · repo", &Theme::fixed().g, None);
    u.feed("main".into(), "* dea82ca Merge pull request #238\n");
    golden("branch-detached-80", &draw(80, 24, &u));
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
    let mut bs = fixture();
    bs[2].elsewhere = Some("/work/wt-fix\n".into());
    let mut u = ui(bs);
    type_str(&mut u, "left");
    press(&mut u, KeyCode::Enter, KeyModifiers::NONE);
    u.feed("fix/Left-Click".into(), "* 1a2b3c4 Fix the click\n");
    golden("branch-note-80", &draw(80, 24, &u));
}

#[test]
fn f1_help_has_an_alt_enter_row_sized_for_scrolling() {
    let mut u = ui(fixture());
    assert_eq!(u.pick.help_len, branch::help(&Theme::fixed().g).len());
    assert!(
        branch::help(&Theme::fixed().g)
            .iter()
            .any(|(k, v)| k == "alt-↵" && v.contains("create"))
    );
    press(&mut u, KeyCode::F(1), KeyModifiers::NONE);
    assert_eq!(u.pick.help, Some(0));
    golden("branch-help-40", &draw(40, 14, &u));
}

// --- #228: the popup runs switch, track and create itself ----------------------

/// The git `act` runs reads no config of the developer's and speaks English: a global
/// `core.hooksPath` would hide the hook a test plants, and a translated git would fail
/// the wording asserted. Set once for this binary, under the lock, never restored, as
/// `fake::fixed_env` does; `act` runs git in this process, so the child's own env is no use.
fn plain_git() {
    let held = fake::env_lock();
    static SET: std::sync::Once = std::sync::Once::new();
    SET.call_once(|| {
        // SAFETY: under the lock; every other reader here is a spawn, which std orders
        // with this write (fake::fixed_env says the same of its three)
        unsafe {
            fake::set_var(&held, "GIT_CONFIG_GLOBAL", "/dev/null");
            fake::set_var(&held, "GIT_CONFIG_NOSYSTEM", "1");
            fake::set_var(&held, "LC_ALL", "C");
        }
    });
}

/// main with `a` = main; old and a shell-metacharacter name at main; conf with `a` =
/// conf; a bare remote holding main as `only-remote`, fetched. On main. Git is plain.
fn act_repo() -> (Scratch, Scratch) {
    plain_git();
    let (r, rm, d) = (Scratch::new(), Scratch::new(), "2020-01-01T00:00:00Z");
    git(&r.0, d, &["init", "-q", "-b", "main", "."]);
    std::fs::write(r.0.join("a"), "main\n").unwrap();
    git(&r.0, d, &["add", "a"]);
    git(&r.0, d, &["commit", "-q", "-m", "main"]);
    git(&r.0, d, &["branch", "old"]);
    git(&r.0, d, &["branch", "feat/$(touch${IFS}x);`id`"]);
    git(&r.0, d, &["switch", "-q", "-c", "conf"]);
    std::fs::write(r.0.join("a"), "conf\n").unwrap();
    git(&r.0, d, &["commit", "-q", "-am", "conf"]);
    git(&r.0, d, &["switch", "-q", "main"]);
    git(&rm.0, d, &["init", "-q", "--bare", "."]);
    git(&r.0, d, &["remote", "add", "origin", rm.s()]);
    git(&r.0, d, &["push", "-q", "origin", "main:only-remote"]);
    git(&r.0, d, &["fetch", "-q", "origin"]);
    (r, rm)
}

fn out(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim_end().to_string()
}

/// `act` on `line`, which must succeed; what git said.
fn acted(r: &Scratch, line: &[u8]) -> String {
    branch::act(r.s(), line).unwrap_or_else(|e| panic!("{}: {e}", String::from_utf8_lossy(line)))
}

#[test]
fn act_switch_checks_out_the_local_branch_and_returns_gits_line() {
    let (r, _rm) = act_repo();
    assert_eq!(acted(&r, b"switch old"), "Switched to branch 'old'");
    assert_eq!(branches::current(r.s()), "old");
}

#[test]
fn act_track_makes_a_local_branch_that_tracks_the_remote_one() {
    let (r, _rm) = act_repo();
    acted(&r, b"track origin/only-remote");
    assert_eq!(branches::current(r.s()), "only-remote");
    assert_eq!(out(&r.0, &["rev-parse", "--abbrev-ref", "@{upstream}"]), "origin/only-remote");
}

#[test]
fn act_create_makes_the_branch_and_checks_it_out() {
    let (r, _rm) = act_repo();
    acted(&r, b"create fresh");
    assert_eq!(branches::current(r.s()), "fresh");
}

#[test]
fn act_refuses_what_tmux_branch_refused_and_checks_out_nothing() {
    let (r, _rm) = act_repo();
    for (row, line, want) in [
        ("an unknown verb", "rm -rf /", "unexpected action"),
        ("a verb with no name", "switch", "not a branch name"),
        ("a name git rejects", "switch ../x", "not a branch name"),
        ("a branch that does not exist", "switch nope", "no such branch"),
        ("track of a local branch", "track old", "no such branch"),
        ("create with an invalid name", "create bad..name", "not a branch name"),
        ("two lines", "switch old\nswitch main", "not a branch name"),
        ("goto, still the script's", "goto /tmp", "unexpected action"),
        ("a remote's name alone", "track origin", "no such branch"),
    ] {
        let got = branch::act(r.s(), line.as_bytes());
        assert!(got.as_ref().is_err_and(|e| e.contains(want)), "{row}: {got:?}");
        assert_eq!(branches::current(r.s()), "main", "{row}: nothing checked out");
    }
}

#[test]
fn act_switches_to_a_metacharacter_name_literally() {
    let (r, _rm) = act_repo();
    let name = "feat/$(touch${IFS}x);`id`";
    acted(&r, format!("switch {name}").as_bytes());
    assert_eq!(branches::current(r.s()), name);
    assert!(!r.0.join("x").exists(), "a command in the name ran");
}

#[test]
fn a_dirty_tree_refusal_is_gits_own_words_and_switches_nothing() {
    let (r, _rm) = act_repo();
    std::fs::write(r.0.join("a"), "dirty\n").unwrap();
    let got = branch::act(r.s(), b"switch conf");
    assert!(got.as_ref().is_err_and(|e| e.contains("overwritten")), "{got:?}");
    assert_eq!(branches::current(r.s()), "main");
    assert_eq!(std::fs::read_to_string(r.0.join("a")).unwrap(), "dirty\n");
}

#[test]
fn a_filter_driver_named_with_an_equals_sign_refuses_the_switch() {
    let (r, _rm) = act_repo();
    git(&r.0, "2020-01-01T00:00:00Z", &["config", "filter.a=b.smudge", "true"]);
    let want = Err(cockpit_tui::data::gitsafe::REFUSAL.to_string());
    assert_eq!(branch::act(r.s(), b"switch conf"), want);
    assert_eq!(branches::current(r.s()), "main");
}

#[test]
fn the_switch_blanks_the_repos_filter_driver() {
    let (r, _rm) = act_repo();
    let (smudged, d) = (r.0.join("smudged"), "2020-01-01T00:00:00Z");
    // attr changes `a`, so the switch writes it and a live smudge would run
    git(&r.0, d, &["switch", "-q", "-c", "attr"]);
    std::fs::write(r.0.join(".gitattributes"), "a filter=x\n").unwrap();
    std::fs::write(r.0.join("a"), "attr\n").unwrap();
    git(&r.0, d, &["add", ".gitattributes", "a"]);
    git(&r.0, d, &["commit", "-q", "-m", "attr"]);
    git(&r.0, d, &["switch", "-q", "main"]);
    let smudge = format!("touch '{}'; cat", smudged.display());
    git(&r.0, d, &["config", "filter.x.smudge", &smudge]);
    acted(&r, b"switch attr");
    assert!(!smudged.exists(), "the repo's smudge ran");
}

/// The act repo with submodule `sub` (`*.t filter=evil`, two commits to f.t) on its newer
/// commit, and branch `oldsub` pinning the older one. The smudge, named in the submodule's
/// own config, touches the file returned; the superproject sets submodule.recurse.
fn sub_repo() -> (Scratch, Scratch, Scratch, std::path::PathBuf) {
    let (r, rm) = act_repo();
    let (sub, d) = (Scratch::new(), "2020-01-01T00:00:00Z");
    git(&sub.0, d, &["init", "-q", "-b", "main", "."]);
    std::fs::write(sub.0.join(".gitattributes"), "*.t filter=evil\n").unwrap();
    for v in ["one", "two"] {
        std::fs::write(sub.0.join("f.t"), format!("{v}\n")).unwrap();
        git(&sub.0, d, &["add", "-A"]);
        git(&sub.0, d, &["commit", "-q", "-m", v]);
    }
    let add = ["-c", "protocol.file.allow=always", "submodule", "add", "-q", sub.s(), "sub"];
    git(&r.0, d, &add);
    git(&r.0, d, &["commit", "-q", "-m", "sub"]);
    git(&r.0, d, &["switch", "-q", "-c", "oldsub"]);
    git(&r.0.join("sub"), d, &["checkout", "-q", "HEAD~1"]);
    git(&r.0, d, &["add", "sub"]);
    git(&r.0, d, &["commit", "-q", "-m", "oldsub"]);
    git(&r.0, d, &["switch", "-q", "main"]);
    git(&r.0.join("sub"), d, &["checkout", "-q", "main"]);
    let smudged = r.0.join("smudged");
    let smudge = format!("touch '{}'; cat", smudged.display());
    git(&r.0.join("sub"), d, &["config", "filter.evil.smudge", &smudge]);
    git(&r.0, d, &["config", "submodule.recurse", "true"]);
    (r, rm, sub, smudged)
}

#[test]
fn the_switch_runs_no_filter_driver_a_submodules_own_config_names() {
    let (r, _rm, _sub, smudged) = sub_repo();
    assert_eq!(out(&r.0.join("sub"), &["log", "-1", "--format=%s"]), "two", "fixture: newer");
    acted(&r, b"switch oldsub");
    assert_eq!(branches::current(r.s()), "oldsub");
    assert!(!smudged.exists(), "the submodule's smudge ran");
}

#[test]
fn a_track_runs_no_filter_driver_a_submodules_own_config_names() {
    let (r, _rm, _sub, smudged) = sub_repo();
    let (d, quiet) = ("2020-01-01T00:00:00Z", ["-c", "submodule.recurse=false"]);
    git(&r.0, d, &[&quiet[..], &["push", "-q", "origin", "oldsub:remote-oldsub"]].concat());
    git(&r.0, d, &[&quiet[..], &["fetch", "-q", "origin"]].concat());
    acted(&r, b"track origin/remote-oldsub");
    assert_eq!(branches::current(r.s()), "remote-oldsub");
    assert!(!smudged.exists(), "the submodule's smudge ran");
}

#[test]
fn the_switch_runs_no_hook() {
    let (r, _rm) = act_repo();
    let hooked = r.0.join("hooked");
    let body = format!("#!/bin/sh\ntouch '{}'\n", hooked.display());
    std::fs::create_dir_all(r.0.join(".git/hooks")).unwrap();
    fake::write_exec(&r.0.join(".git/hooks/post-checkout"), &body);
    acted(&r, b"switch conf");
    assert!(!hooked.exists(), "the repo's post-checkout hook ran");
}

#[test]
fn a_failed_action_is_a_note_and_its_message_fills_the_preview_until_the_next_key() {
    let mut u = ui(fixture());
    u.feed("main".into(), "* abc init\n");
    let msg = "\nerror: your local changes would be overwritten\n  a\nAborting";
    u.failed(msg);
    assert_eq!(u.pick.header, "✕ error: your local changes would be overwritten");
    assert_eq!(u.pick.preview.as_deref(), Some(msg));
    press(&mut u, KeyCode::Down, KeyModifiers::NONE);
    press(&mut u, KeyCode::Up, KeyModifiers::NONE);
    assert!(!u.pick.header.starts_with('✕'), "{}", u.pick.header);
    assert_eq!(u.pick.preview.as_deref(), Some("* abc init"));
}

#[test]
fn a_failed_actions_message_outlasts_a_late_graph_and_the_mouse_until_a_key() {
    let mut u = ui(fixture());
    u.failed("error: refused");
    u.feed("main".into(), "* abc init\n");
    assert_eq!(u.pick.preview.as_deref(), Some("error: refused"), "after a late graph");
    let wheel = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    };
    u.mouse(wheel, BIG);
    assert_eq!(u.pick.preview.as_deref(), Some("error: refused"), "after the wheel");
    // Home: back on main whatever row the wheel left selected
    press(&mut u, KeyCode::Home, KeyModifiers::NONE);
    assert_eq!(u.pick.preview.as_deref(), Some("* abc init"), "a key brings the graph back");
}
