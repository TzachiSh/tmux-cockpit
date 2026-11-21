//! Explain wired to the what-changed collector (#1000, step 2 of #935): the
//! collect starts when the question list opens, no call waits for it, the
//! request does not carry it, the first answer draws it at its top when it
//! lands, and last-seen moves once a frame showed it there.
//! Fixture repos under the temp dir, never the user's; the rig's own `gh` and
//! `claude`, each started by its full path, so no row reaches a real one.
mod fake;
use cockpit_tui::data::workdir::WorkDir;
use cockpit_tui::panel::app::{App, QUICK, Tile, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::changes::{Changes, Collector, Gh, Note, Seen};
use cockpit_tui::panel::changes::{mark_seen, read_seen, repo_key};
use cockpit_tui::panel::look::{Looks, mark_at};
use cockpit_tui::panel::request::{Call, Effort};
use cockpit_tui::panel::view::render;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fake::{Scratch, assert_gh_is_the_rigs, rig_path, write_exec};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// 2025-11-13T02:13:20Z: when the user last looked, in every row.
const T0: u64 = 1_763_000_000;
const SINCE: &str = "2025-11-13T02:13:20Z";
/// The rig's clock while the panel collects.
const T1: u64 = T0 + 100;

/// A stand-in gh: counts its starts, holds while `gh.hold` exists, prints the
/// file named after its first word, and exits with the code in `gh.exit`.
const GH: &str = r#"#!/bin/sh
echo x >> "$0.started"
while [ -e "$0.hold" ]; do sleep 0.05; done
cat "$0.$1"
exit "$(cat "$0.exit" 2>/dev/null || echo 0)"
"#;

/// A stand-in claude that reads the prompt and answers.
const ANSWERS: &str = "#!/bin/sh\ncat >/dev/null\nprintf 'it is'\n";

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A commit with `msg` as its subject, and its full sha.
fn commit(dir: &Path, msg: &str) -> String {
    git(dir, &["commit", "-q", "--allow-empty", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"])
}

fn short(sha: &str) -> String {
    sha[..12].to_string()
}

struct Rig {
    s: Scratch,
    bin: PathBuf,
    path: OsString,
    clock: Arc<AtomicU64>,
    repo: PathBuf,
    /// The repo's first commit: what the user last saw, at [`T0`].
    first: String,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let name = format!("cockpit-wired-{tag}-{}", std::process::id());
        let s = Scratch::fresh(std::env::temp_dir().join(name));
        let bin = s.join("rig/bin");
        std::fs::create_dir_all(&bin).unwrap();
        write_exec(&bin.join("gh"), GH);
        for w in ["pr", "issue", "run"] {
            std::fs::write(bin.join(format!("gh.{w}")), "[]").unwrap();
        }
        let path = OsString::from(rig_path(&bin));
        assert_gh_is_the_rigs(&path, &s.join("rig"));
        let repo = s.join("r");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["remote", "add", "origin", "https://github.com/o/r.git"]);
        let first = commit(&repo, "first");
        let clock = Arc::new(AtomicU64::new(T1));
        let r = Rig {
            s,
            bin,
            path,
            clock,
            repo,
            first,
        };
        r.seen(&r.first, T0);
        r
    }

    fn seen(&self, head: &str, at: u64) {
        mark_seen(&self.store(), &self.key(), head, at).unwrap();
    }

    fn read(&self) -> Option<Seen> {
        read_seen(&self.store(), &self.key())
    }

    fn key(&self) -> String {
        repo_key(self.repo.to_str().unwrap())
    }

    fn store(&self) -> PathBuf {
        self.s.join("store")
    }

    fn collector(&self, budget: Duration) -> Collector {
        let gh = Gh {
            program: self.bin.join("gh").into_os_string(),
            path: Some(self.path.clone()),
            budget,
        };
        let clock = self.clock.clone();
        Collector::new(gh, self.store(), Box::new(move || clock.load(Ordering::SeqCst)))
    }

    /// The panel on a pane running `cmd` in the repo, with the rig's collector.
    fn app_with(&self, cmd: &str, budget: Duration) -> App {
        fake::fixed_env();
        let cap = Capture {
            name: "w".into(),
            cmd: cmd.into(),
            path: self.repo.display().to_string(),
            work: WorkDir {
                dir: self.repo.display().to_string(),
                ..Default::default()
            },
            screen: "$ ".into(),
            ..Default::default()
        };
        let mut a = App::new("%3", cap);
        a.changes = Some(Looks::new(self.collector(budget)));
        a
    }

    fn app(&self) -> App {
        self.app_with("bash", Duration::from_secs(20))
    }

    fn starts(&self) -> usize {
        let s = std::fs::read_to_string(self.bin.join("gh.started"));
        s.map(|s| s.lines().count()).unwrap_or(0)
    }

    /// A claude by its full path, with `body`.
    fn claude(&self, body: &str) -> Call {
        let bin = self.s.join("claude");
        write_exec(&bin, body);
        Call {
            claude: bin.display().to_string(),
            timeout: Duration::from_secs(30),
            dir: self.s.join("cache"),
            effort: Effort::Medium,
        }
    }
}

fn key(a: &mut App, c: KeyCode) {
    a.key(KeyEvent::new(c, KeyModifiers::NONE));
}

/// Home's Explain tile, then the quick question `n`.
fn explain(a: &mut App, n: char) {
    let i = a.tiles().iter().position(|t| t.0 == Tile::Explain).unwrap();
    key(a, KeyCode::Char(char::from(b'1' + i as u8)));
    assert!(matches!(a.view, View::ExplainAsk { .. }), "{:?}", a.view);
    key(a, KeyCode::Char(n));
    assert_eq!(a.view, View::Busy);
}

/// Polls until the list is in or its wait is over: a count, never a clock.
fn settle(a: &mut App) {
    for _ in 0..6000 {
        let l = a.changes.as_mut().unwrap();
        l.poll();
        if !l.waiting() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the list never came");
}

/// The call goes out and its answer lands.
fn answered(a: &mut App, call: &Call) {
    a.launch(call);
    assert!(a.req.is_some(), "the call went out");
    for _ in 0..6000 {
        if a.poll() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("no answer came");
}

/// Polls the panel as its loop does until the list is drawn: a count, never a clock.
fn landed(a: &mut App) -> (String, Vec<String>) {
    for _ in 0..6000 {
        a.poll();
        if let Some(s) = a.changes_shown() {
            return s;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the list never landed");
}

fn hold(r: &Rig) {
    std::fs::write(r.bin.join("gh.hold"), "").unwrap();
}

fn release(r: &Rig) {
    std::fs::remove_file(r.bin.join("gh.hold")).unwrap();
}

/// What the Answer draws atop the transcript: its heading and its lines.
fn shown(head: &str, lines: &[String]) -> Option<(String, Vec<String>)> {
    Some((head.to_string(), lines.to_vec()))
}

fn frame(a: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| drop(render(f, f.area(), a, &Theme::fixed(), 0))).unwrap();
    t.backend().to_string()
}

/// Every `App` here is built under the binary's one environment (#957).
#[test]
fn an_explain_app_is_built_under_the_fixed_environment() {
    let _ = Rig::new("env").app();
    fake::assert_fixed_env();
}

#[test]
fn the_fixed_environment_replaces_values_already_set() {
    fake::fixed_env_in_a_child("an_explain_app_is_built_under_the_fixed_environment");
}

#[test]
fn the_collect_starts_when_the_question_list_opens() {
    let r = Rig::new("tile");
    hold(&r);
    let mut a = r.app();
    let i = a.tiles().iter().position(|t| t.0 == Tile::Explain).unwrap();
    key(&mut a, KeyCode::Char(char::from(b'1' + i as u8)));
    let started = a.changes.as_ref().unwrap().waiting();
    release(&r);
    assert!(matches!(a.view, View::ExplainAsk { .. }), "{:?}", a.view);
    assert!(started, "no collect under way on the question list");
}

#[test]
fn explain_never_waits_for_the_list() {
    let r = Rig::new("nowait");
    commit(&r.repo, "two");
    hold(&r);
    let mut a = r.app();
    explain(&mut a, '1');
    a.launch(&r.claude(ANSWERS));
    let out = a.req.is_some();
    release(&r);
    assert!(out, "the call waited for the list");
    // both finish before the rig's folder goes
    settle(&mut a);
    for _ in 0..6000 {
        if a.poll() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("no answer came");
}

/// #935 task 4 adds the list to the request with its prompt text; until then
/// the request, its system prompt, its user prompt and its model, is the one
/// an App with no collector builds on the same Capture after the same keys.
#[test]
fn the_request_is_the_same_with_the_list_collected() {
    let r = Rig::new("bytes");
    let two = commit(&r.repo, "two");
    let mut a = r.app();
    explain(&mut a, '1');
    let (_, lines) = landed(&mut a);
    assert_eq!(lines, [format!("{} two", short(&two))]);
    let mut b = App::new("%3", a.cap.clone());
    assert!(b.changes.is_none());
    explain(&mut b, '1');
    assert!(a.request_parts().is_some());
    assert_eq!(a.request_parts(), b.request_parts());
}

#[test]
fn the_list_is_drawn_atop_the_first_answer_opened_at_its_top() {
    let r = Rig::new("drawn");
    let (two, three) = (commit(&r.repo, "two"), commit(&r.repo, "three"));
    let mut a = r.app();
    explain(&mut a, '1');
    settle(&mut a);
    answered(&mut a, &r.claude(ANSWERS));
    assert_eq!(a.view, View::Answer { scroll: 0, armed: false });
    let head = format!("what changed since you last looked, {SINCE}");
    let lines = [format!("{} three", short(&three)), format!("{} two", short(&two))];
    assert_eq!(a.changes_shown(), shown(&head, &lines));
    let f = frame(&a, 80, 20);
    let at = |t: &str| f.find(t).unwrap_or_else(|| panic!("no {t:?} in\n{f}"));
    assert!(at(&head) < at(&lines[1]) && at(&lines[1]) < at(&format!("you: {}", QUICK[0].0)));
}

/// The answer lands first; the list lands while the user reads it.
#[test]
fn a_list_landing_after_the_answer_is_drawn_atop_it_and_marked_then() {
    let r = Rig::new("late");
    let two = commit(&r.repo, "two");
    hold(&r);
    let mut a = r.app();
    explain(&mut a, '1');
    answered(&mut a, &r.claude(ANSWERS));
    let open = (a.view.clone(), a.changes_shown(), a.ticks());
    a.frame(1);
    let before = r.read();
    release(&r);
    assert_eq!(open, (View::Answer { scroll: 0, armed: false }, None, true));
    let old = Seen {
        head: r.first.clone(),
        at: T0,
    };
    assert_eq!(before, Some(old), "marked before the list was drawn");
    let (head, _) = landed(&mut a);
    assert!(frame(&a, 80, 20).contains(&head));
    a.frame(2);
    assert_eq!(r.read(), Some(Seen { head: two, at: T1 }));
}

#[test]
fn last_seen_is_written_once_the_answer_is_drawn_and_not_before() {
    let r = Rig::new("mark");
    let two = commit(&r.repo, "two");
    let mut a = r.app();
    explain(&mut a, '1');
    settle(&mut a);
    answered(&mut a, &r.claude(ANSWERS));
    let before = r.read();
    a.frame(1);
    let old = Seen {
        head: r.first.clone(),
        at: T0,
    };
    assert_eq!((before, r.read()), (Some(old), Some(Seen { head: two, at: T1 })));
}

/// The list lands after the user scrolled the answer off its top: a frame there
/// has not shown it and marks nothing; one back at the top does (#1269).
#[test]
fn a_list_landing_while_scrolled_off_the_top_is_marked_back_at_it() {
    let r = Rig::new("scrolled");
    let two = commit(&r.repo, "two");
    hold(&r);
    let mut a = r.app();
    explain(&mut a, '1');
    answered(&mut a, &r.claude(ANSWERS));
    // the user's scroll leaves the top while the list is still out
    a.view = View::Answer { scroll: 3, armed: false };
    release(&r);
    landed(&mut a);
    a.frame(1);
    let away = r.read();
    a.view = View::Answer { scroll: 0, armed: false };
    a.frame(2);
    let old = Seen {
        head: r.first.clone(),
        at: T0,
    };
    assert_eq!((away, r.read()), (Some(old), Some(Seen { head: two, at: T1 })));
}

/// Under the size floor the card is drawn in the body's place: no list on
/// screen, so nothing is marked until a frame shows the body (#1269).
#[test]
fn the_floor_card_marks_nothing_until_the_body_shows() {
    let r = Rig::new("floor");
    let two = commit(&r.repo, "two");
    let mut a = r.app();
    explain(&mut a, '1');
    settle(&mut a);
    answered(&mut a, &r.claude(ANSWERS));
    let (head, _) = a.changes_shown().expect("the list");
    assert!(!frame(&a, 20, 5).contains(&head), "the floor card drew the list");
    a.shrunk(true);
    a.frame(1);
    let under = r.read();
    a.shrunk(false);
    a.frame(2);
    let old = Seen {
        head: r.first.clone(),
        at: T0,
    };
    assert_eq!((under, r.read()), (Some(old), Some(Seen { head: two, at: T1 })));
}

/// The help overlay covers the list: nothing is marked until it closes (#1269).
#[test]
fn the_help_overlay_marks_nothing_until_it_closes() {
    let r = Rig::new("help");
    let two = commit(&r.repo, "two");
    let mut a = r.app();
    explain(&mut a, '1');
    settle(&mut a);
    answered(&mut a, &r.claude(ANSWERS));
    key(&mut a, KeyCode::F(1));
    let (head, _) = a.changes_shown().expect("the list");
    assert!(a.help.is_some() && !frame(&a, 80, 20).contains(&head), "the list shows");
    a.frame(1);
    let under = r.read();
    key(&mut a, KeyCode::Esc);
    assert_eq!((a.help, a.view.clone()), (None, View::Answer { scroll: 0, armed: false }));
    a.frame(2);
    let old = Seen {
        head: r.first.clone(),
        at: T0,
    };
    assert_eq!((under, r.read()), (Some(old), Some(Seen { head: two, at: T1 })));
}

#[test]
fn a_failed_or_cancelled_explain_leaves_last_seen_as_it_was() {
    for row in ["failed", "cancelled"] {
        let r = Rig::new(row);
        commit(&r.repo, "two");
        let mut a = r.app();
        explain(&mut a, '1');
        settle(&mut a);
        if row == "failed" {
            answered(&mut a, &r.claude("#!/bin/sh\ncat >/dev/null\nexit 3\n"));
            assert!(matches!(a.view, View::Error { .. }), "row {row}: {:?}", a.view);
        } else {
            a.launch(&r.claude("#!/bin/sh\nexec sleep 1000\n"));
            assert!(a.req.is_some(), "row {row}: the call went out");
            key(&mut a, KeyCode::Esc);
            assert!(matches!(a.view, View::ExplainAsk { .. }), "row {row}: {:?}", a.view);
        }
        a.frame(1);
        let old = Seen {
            head: r.first.clone(),
            at: T0,
        };
        assert_eq!(r.read(), Some(old), "row {row}");
    }
}

#[test]
fn a_second_question_reuses_the_unseen_list() {
    let r = Rig::new("reuse");
    let two = commit(&r.repo, "two");
    let mut a = r.app();
    explain(&mut a, '1');
    settle(&mut a);
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::ExplainAsk { .. }), "{:?}", a.view);
    // a commit made since: a second collect would list it
    commit(&r.repo, "three");
    key(&mut a, KeyCode::Char('2'));
    settle(&mut a);
    let (_, lines) = a.changes_shown().expect("the list");
    assert_eq!(lines, [format!("{} two", short(&two))]);
}

#[test]
fn an_explain_after_the_list_was_seen_lists_from_the_new_last_seen() {
    let r = Rig::new("again");
    commit(&r.repo, "two");
    let mut a = r.app();
    explain(&mut a, '1');
    settle(&mut a);
    answered(&mut a, &r.claude(ANSWERS));
    a.frame(1);
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Home { sel: 0 });
    let three = commit(&r.repo, "three");
    explain(&mut a, '1');
    settle(&mut a);
    let head = "what changed since you last looked, 2025-11-13T02:15:00Z";
    let floor = "GitHub: checked under 60 s ago, again in 60 s".to_string();
    assert_eq!(a.changes_shown(), shown(head, &[format!("{} three", short(&three)), floor]));
}

/// What a mark of a list carries: rows named, `at` = [`T1`], an old record at [`T0`].
#[test]
fn mark_at_names_the_time_a_mark_carries() {
    let seen = Some(Seen {
        head: "a".repeat(40),
        at: T0,
    });
    let list = |seen: Option<Seen>, head: bool, notes: Vec<Note>| Changes {
        key: Some("0123456789abcdef".into()),
        head: head.then(|| "b".repeat(40)),
        seen,
        notes,
        ..Default::default()
    };
    let rows = [
        ("clean", list(seen.clone(), true, vec![]), Some(T1)),
        ("git timed out", list(seen.clone(), true, vec![Note::GitTimedOut]), None),
        ("gh failed, a record", list(seen.clone(), true, vec![Note::GhFailed]), Some(T0)),
        ("gh timed out, none", list(None, true, vec![Note::GhTimedOut]), Some(T1)),
        ("floor", list(seen.clone(), true, vec![Note::Floor { again_in: 5 }]), Some(T0)),
        ("no gh", list(seen.clone(), true, vec![Note::NoGh]), Some(T1)),
        ("not logged in", list(seen.clone(), true, vec![Note::NotLoggedIn]), Some(T1)),
        ("no head", list(seen.clone(), false, vec![]), None),
    ];
    for (row, c, want) in rows {
        assert_eq!(mark_at(&c, T1), want, "row {row}");
    }
}

#[test]
fn a_gh_failure_moves_the_head_and_keeps_the_time() {
    let r = Rig::new("ghfail");
    let two = commit(&r.repo, "two");
    std::fs::write(r.bin.join("gh.exit"), "1").unwrap();
    let mut a = r.app();
    explain(&mut a, '1');
    settle(&mut a);
    answered(&mut a, &r.claude(ANSWERS));
    a.frame(1);
    assert_eq!(r.read(), Some(Seen { head: two, at: T0 }));
}

#[test]
fn a_pane_on_another_machine_collects_nothing() {
    let r = Rig::new("ssh");
    commit(&r.repo, "two");
    // a collect started would hold in gh, so the row cannot pass by a race
    hold(&r);
    let mut a = r.app_with("ssh", Duration::from_secs(5));
    explain(&mut a, '1');
    let waiting = a.changes.as_ref().unwrap().waiting();
    release(&r);
    assert!(!waiting, "a collect started");
    assert_eq!((a.changes_shown(), r.starts()), (None, 0));
}

/// Outside any repo, or in one with no commit yet, the list can never be
/// marked seen: the answer opens at its bottom as before, with no list (#1269).
#[test]
fn a_pane_with_nothing_to_mark_shows_no_list() {
    for row in ["not a repo", "no commit yet"] {
        let r = Rig::new(&row.replace(' ', ""));
        let dir = r.s.join("plain");
        std::fs::create_dir_all(&dir).unwrap();
        if row == "no commit yet" {
            git(&dir, &["init", "-q", "-b", "main"]);
        }
        let cap = Capture {
            name: "w".into(),
            cmd: "bash".into(),
            path: dir.display().to_string(),
            work: WorkDir {
                dir: dir.display().to_string(),
                ..Default::default()
            },
            screen: "$ ".into(),
            ..Default::default()
        };
        let mut a = App::new("%3", cap);
        a.changes = Some(Looks::new(r.collector(Duration::from_secs(20))));
        explain(&mut a, '1');
        settle(&mut a);
        answered(&mut a, &r.claude(ANSWERS));
        let at = View::Answer {
            scroll: usize::MAX,
            armed: false,
        };
        assert_eq!((a.view.clone(), a.changes_shown()), (at, None), "row {row}");
        assert!(!frame(&a, 80, 20).contains("first look"), "row {row}");
    }
}

/// The child half of the row below, in a process whose PATH starts with a git
/// that sleeps on `rev-parse --verify`, so HEAD times out and the repo key does not.
#[test]
fn git_timeout_child() {
    if std::env::var_os("WIRED_GIT_TIMEOUT_CHILD").is_none() {
        return;
    }
    let r = Rig::new("gitslow");
    let pr = r#"[{"number":7,"title":"seven","updatedAt":"2025-11-14T00:00:00Z"}]"#;
    std::fs::write(r.bin.join("gh.pr"), pr).unwrap();
    let mut l = Looks::new(r.collector(Duration::from_secs(20)));
    l.ahead(r.repo.to_str().unwrap());
    for _ in 0..6000 {
        l.poll();
        if !l.waiting() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let head = format!("what changed since you last looked, {SINCE}");
    let lines = ["PR #7 seven".to_string(), "git: took too long, commits left out".into()];
    assert_eq!((l.shown(), l.unseen()), (shown(&head, &lines), true));
    eprintln!("the-git-timeout-list-was-shown");
}

/// A HEAD that git took too long to read is no "no commit yet": the list keeps
/// its GitHub items and the timeout note (#1269).
#[test]
fn a_git_timeout_keeps_the_list_and_says_so() {
    let name = format!("cockpit-wired-shim-{}", std::process::id());
    let s = Scratch::fresh(std::env::temp_dir().join(name));
    let path = std::env::var_os("PATH").unwrap_or_default();
    let real = std::env::split_paths(&path).map(|d| d.join("git")).find(|g| g.is_file());
    let real = real.expect("a git on PATH");
    let body = format!(
        "#!/bin/sh\nfor a; do [ \"$a\" = --verify ] && exec sleep 3; done\nexec '{}' \"$@\"\n",
        real.display()
    );
    write_exec(&s.join("git"), &body);
    let first = std::iter::once(s.to_path_buf()).chain(std::env::split_paths(&path));
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "git_timeout_child", "--nocapture"])
        .env("PATH", std::env::join_paths(first).unwrap())
        .env("WIRED_GIT_TIMEOUT_CHILD", "1")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}");
    assert!(said.lines().any(|l| l == "the-git-timeout-list-was-shown"), "{said}");
}

/// After `git checkout HEAD~1` the old tip is still held by its branch: no
/// commit and no rewrite, and the checkout detached HEAD.
#[test]
fn the_head_moved_back_lists_nothing_new() {
    let r = Rig::new("back");
    r.seen(&commit(&r.repo, "two"), T0);
    git(&r.repo, &["checkout", "-q", "HEAD~1"]);
    let mut a = r.app();
    explain(&mut a, '1');
    settle(&mut a);
    let head = format!("nothing new listed since you last looked, {SINCE}");
    let detached = ["GitHub: HEAD is detached, CI left out".to_string()];
    assert_eq!(a.changes_shown(), shown(&head, &detached));
}
