//! The info screen (#252): the rows, the keys, the goldens, and the two ports
//! the shell used to own -- git root and review base. Fixtures are literals and
//! scratch repos; no tmux. COCKPIT_BLESS=1 cargo test rewrites the goldens.
use cockpit_tui::data::info::{Base, Doing, Facts, cksum, clean, git_root, review_base};
use cockpit_tui::info::{self, Out, Screen};
use cockpit_tui::theme::Theme;
use crossterm::event::KeyCode;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn draw(w: u16, h: u16, s: &Screen) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| info::render(f, f.area(), &Theme::fixed(), s)).unwrap();
    t.backend().to_string()
}

fn facts() -> Facts {
    Facts {
        id: "%7".into(),
        wher: "work:2 api".into(),
        cmd: "claude".into(),
        cwd: "/home/u/src/api/server".into(),
        dir: Some("/home/u/src/api-wt".into()),
        root: "/home/u/src/api-wt".into(),
        state: "needs-input".into(),
        msg: "run make?".into(),
        agent: true,
        doing: vec![
            Doing {
                kind: "doing".into(),
                text: "Bash: make test".into(),
                age: Some(252),
            },
            Doing {
                kind: "said".into(),
                text: "Running the suite now.".into(),
                age: None,
            },
            Doing {
                kind: "asked".into(),
                text: "fix the parser".into(),
                age: Some(3720),
            },
        ],
        review: "pending".into(),
        spool: "2".into(),
        base: Some(Base {
            rev: "HEAD".into(),
            label: "uncommitted".into(),
            mode: "uncommitted".into(),
        }),
        sid: "sid-9".into(),
        title: "Fix the parser".into(),
        tui: "0.2.0  /usr/bin/cockpit-tui".into(),
        rev_dir: Some(("/home/u/.local/state/tmux-cockpit/review/7-123".into(), 1, 2)),
        ..Default::default()
    }
}

#[test]
fn golden_widths() {
    let s = Screen::of(Ok(facts()));
    for w in [40, 60, 80, 120] {
        golden(&format!("info-{w}"), &draw(w, 24, &s));
    }
}

#[test]
fn golden_floor() {
    golden("info-floor", &draw(30, 6, &Screen::of(Ok(facts()))));
}

#[test]
fn golden_missing_pane() {
    golden("info-missing", &draw(60, 12, &Screen::of(Err("no such pane %999".into()))));
}

#[test]
fn golden_scrolled_with_range() {
    let mut s = Screen::of(Ok(facts()));
    s.top = 4;
    golden("info-80-short", &draw(80, 10, &s));
}

fn area() -> Rect {
    Rect::new(0, 0, 80, 10)
}

#[test]
fn esc_and_q_close_it() {
    for c in [KeyCode::Esc, KeyCode::Char('q')] {
        let mut s = Screen::of(Ok(facts()));
        assert!(matches!(info::key(&mut s, &Theme::fixed(), c, area()), Out::Close), "{c:?}");
    }
}

#[test]
fn arrows_scroll_and_clamp() {
    let th = Theme::fixed();
    let mut s = Screen::of(Ok(facts()));
    info::key(&mut s, &th, KeyCode::Up, area());
    assert_eq!(s.top, 0, "up at the top stays");
    info::key(&mut s, &th, KeyCode::Down, area());
    assert_eq!(s.top, 1);
    for _ in 0..100 {
        info::key(&mut s, &th, KeyCode::Down, area());
    }
    let rows = info::rows(&facts(), &th.g, 76).len();
    assert_eq!(s.top, rows - 7, "down stops where the last row is the last drawn");
}

#[test]
fn help_opens_and_any_other_key_closes_only_it() {
    let th = Theme::fixed();
    let mut s = Screen::of(Ok(facts()));
    info::key(&mut s, &th, KeyCode::Char('?'), area());
    assert_eq!(s.help, Some(0));
    assert!(matches!(info::key(&mut s, &th, KeyCode::Char('x'), area()), Out::Stay));
    assert_eq!(s.help, None, "x closed the help, not the screen");
}

#[test]
fn rows_follow_the_shell_and_a_non_agent_has_no_btw() {
    let th = Theme::fixed();
    let r = info::rows(&facts(), &th.g, 76);
    let get = |k: &str| r.iter().find(|(l, _)| l == k).map(|(_, v)| v.as_str());
    assert_eq!(get("pane"), Some("%7  work:2 api  (claude)"));
    assert_eq!(get("state"), Some("needs-input  · run make?"));
    assert_eq!(get("doing"), Some("Bash: make test (since 4m12s)"));
    assert_eq!(get("asked"), Some("fix the parser (1h02m)"));
    assert_eq!(get("review"), Some("pending  · queued: 2"));
    assert_eq!(get("base"), Some("uncommitted  (HEAD)"));
    assert!(get("ask it").is_some());
    let plain = Facts {
        agent: false,
        doing: vec![],
        dir: None,
        rev_dir: None,
        base: None,
        ..facts()
    };
    let r = info::rows(&plain, &th.g, 76);
    assert!(
        r.iter()
            .all(|(l, _)| !matches!(l.as_str(), "ask it" | "doing" | "dir" | "base" | "review dir"))
    );
}

#[test]
fn a_missing_cockpit_dir_says_so() {
    let f = Facts {
        dir_missing: true,
        ..facts()
    };
    let r = info::rows(&f, &Theme::fixed().g, 76);
    assert!(r.iter().any(|(l, v)| l == "dir" && v.contains("missing -- using cwd")));
}

#[test]
fn control_characters_are_stripped() {
    assert_eq!(clean("a\x1b[31mb\x07\tc\r\nd"), "a[31mbcd");
}

#[test]
fn cksum_is_posix() {
    assert_eq!(cksum(b"123456789"), 930766865);
    assert_eq!(cksum(b""), 4294967295);
    // the length goes in too: a 256-byte input folds a second length byte
    let long = vec![b'x'; 300];
    let want = Command::new("sh")
        .arg("-c")
        .arg("head -c 300 /dev/zero | tr '\\0' x | cksum | cut -d' ' -f1")
        .output()
        .unwrap();
    assert_eq!(cksum(&long).to_string(), String::from_utf8_lossy(&want.stdout).trim());
}

fn sh(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .output()
        .unwrap();
    assert!(st.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&st.stderr));
}

/// Removes the scratch dir when the test ends, passed or not.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn repo(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cockpit-info-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    sh(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("f"), "a\n").unwrap();
    sh(&d, &["add", "-A"]);
    sh(&d, &["commit", "-q", "-m", "init"]);
    d
}

fn rev(d: &Path, r: &str) -> String {
    let o = Command::new("git").arg("-C").arg(d).args(["rev-parse", r]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// On main with nothing ahead: the uncommitted view. A feature branch with a
/// commit past main: the branch base. A checkpoint ref beats both until a mode
/// is forced past it; a forced mode that does not hold falls through.
#[test]
fn review_base_three_modes() {
    let d = repo("modes");
    let _g = Scratch(d.clone());
    let ds = d.to_str().unwrap();
    let main = rev(&d, "HEAD");
    assert_eq!(
        review_base(ds, "auto", "", ""),
        Base {
            rev: "HEAD".into(),
            label: "uncommitted".into(),
            mode: "uncommitted".into()
        }
    );
    sh(&d, &["checkout", "-q", "-b", "feat"]);
    std::fs::write(d.join("f"), "a\nb\n").unwrap();
    sh(&d, &["commit", "-q", "-am", "two"]);
    let b = review_base(ds, "", "", "");
    assert_eq!(
        (b.rev.as_str(), b.label.as_str(), b.mode.as_str()),
        (main.as_str(), "vs main", "branch")
    );
    let head = rev(&d, "HEAD");
    sh(&d, &["update-ref", "refs/cockpit/reviewed/feat", &head]);
    let b = review_base(ds, "auto", "", "");
    assert_eq!(
        (b.rev.as_str(), b.label.as_str(), b.mode.as_str()),
        (head.as_str(), "since last review", "since-review")
    );
    assert_eq!(
        review_base(ds, "branch", "", "").mode,
        "branch",
        "a forced mode skips the checkpoint"
    );
    assert_eq!(review_base(ds, "uncommitted", "", "").mode, "uncommitted");
    // a checkpoint on another branch's name does not hold here
    sh(&d, &["update-ref", "-d", "refs/cockpit/reviewed/feat"]);
    assert_eq!(review_base(ds, "since-review", "", "").mode, "branch", "since-review falls through");
}

/// The second case of the class: a detached HEAD keys its checkpoint on HEAD,
/// and a branch with an upstream that is not ahead has nothing to diff against.
#[test]
fn review_base_detached_and_upstream() {
    let d = repo("det");
    let _g = Scratch(d.clone());
    let ds = d.to_str().unwrap();
    let head = rev(&d, "HEAD");
    sh(&d, &["checkout", "-q", "--detach"]);
    sh(&d, &["update-ref", "refs/cockpit/reviewed/HEAD", &head]);
    assert_eq!(review_base(ds, "auto", "", "").mode, "since-review");
}

#[test]
fn git_root_is_the_top_or_the_dir_itself() {
    let d = repo("root");
    let _g = Scratch(d.clone());
    std::fs::create_dir_all(d.join("a/b")).unwrap();
    let want = std::fs::canonicalize(&d).unwrap();
    assert_eq!(git_root(d.join("a/b").to_str().unwrap()), want.to_str().unwrap());
    let plain = std::env::temp_dir().join(format!("cockpit-info-plain-{}", std::process::id()));
    std::fs::create_dir_all(&plain).unwrap();
    let _p = Scratch(plain.clone());
    assert_eq!(git_root(plain.to_str().unwrap()), plain.to_str().unwrap(), "no repo: the dir");
    let _ = std::fs::remove_dir_all(&d);
}

/// A repo whose branch `feat` tracks a bare remote, for the upstream case of
/// review_base. `feat` is one commit past main; the remote has it or not.
fn tracked(tag: &str) -> (PathBuf, PathBuf) {
    let d = repo(tag);
    let remote = PathBuf::from(format!("{}-remote", d.display()));
    let _ = std::fs::remove_dir_all(&remote);
    std::fs::create_dir_all(&remote).unwrap();
    sh(&remote, &["init", "-q", "--bare", "-b", "main"]);
    sh(&d, &["remote", "add", "origin", remote.to_str().unwrap()]);
    sh(&d, &["checkout", "-q", "-b", "feat"]);
    std::fs::write(d.join("f"), "a\nb\n").unwrap();
    sh(&d, &["commit", "-q", "-am", "two"]);
    sh(&d, &["push", "-q", "-u", "origin", "feat"]);
    (d, remote)
}

/// The upstream, when it is not ahead of HEAD, leaves nothing to diff against.
#[test]
fn review_base_upstream_at_head_is_uncommitted() {
    let (d, r) = tracked("upsame");
    let (_g, _h) = (Scratch(d.clone()), Scratch(r));
    let b = review_base(d.to_str().unwrap(), "auto", "", "");
    assert_eq!((b.label.as_str(), b.mode.as_str()), ("uncommitted", "uncommitted"));
}

/// Ahead of its upstream, the branch is diffed against the merge-base with it,
/// which is the upstream itself.
#[test]
fn review_base_ahead_of_upstream_is_vs_upstream() {
    let (d, r) = tracked("upahead");
    let (_g, _h) = (Scratch(d.clone()), Scratch(r));
    let pushed = rev(&d, "HEAD");
    std::fs::write(d.join("f"), "a\nb\nc\n").unwrap();
    sh(&d, &["commit", "-q", "-am", "three"]);
    let b = review_base(d.to_str().unwrap(), "auto", "", "");
    assert_eq!(
        (b.rev.as_str(), b.label.as_str(), b.mode.as_str()),
        (pushed.as_str(), "vs origin/feat", "branch")
    );
}

/// An upstream that was deleted reads [gone]: the fallback to main applies.
#[test]
fn review_base_gone_upstream_falls_back_to_main() {
    let (d, r) = tracked("upgone");
    let (_g, _h) = (Scratch(d.clone()), Scratch(r));
    let main = rev(&d, "main");
    sh(&d, &["push", "-q", "origin", "--delete", "feat"]);
    let b = review_base(d.to_str().unwrap(), "auto", "", "");
    assert_eq!(
        (b.rev.as_str(), b.label.as_str(), b.mode.as_str()),
        (main.as_str(), "vs main", "branch")
    );
}

/// origin/HEAD is the fallback base when the repo has no local main or master:
/// a clone whose default branch is `develop`, on a topic branch one commit past
/// it. The shell's tab IFS used to fold the row and lose the target.
#[test]
fn review_base_origin_head_when_no_main() {
    let d = repo("ohead");
    let remote = PathBuf::from(format!("{}-remote", d.display()));
    let _ = std::fs::remove_dir_all(&remote);
    std::fs::create_dir_all(&remote).unwrap();
    let _g = (Scratch(d.clone()), Scratch(remote.clone()));
    sh(&remote, &["init", "-q", "--bare", "-b", "develop"]);
    sh(&d, &["branch", "-m", "develop"]);
    sh(&d, &["remote", "add", "origin", remote.to_str().unwrap()]);
    sh(&d, &["push", "-q", "origin", "develop"]);
    sh(&d, &["remote", "set-head", "origin", "develop"]);
    let base = rev(&d, "HEAD");
    sh(&d, &["checkout", "-q", "-b", "topic"]);
    std::fs::write(d.join("f"), "a\nb\n").unwrap();
    sh(&d, &["commit", "-q", "-am", "topic"]);
    let b = review_base(d.to_str().unwrap(), "auto", "", "");
    assert_eq!(
        (b.rev.as_str(), b.label.as_str(), b.mode.as_str()),
        (base.as_str(), "vs origin/develop", "branch")
    );
}
