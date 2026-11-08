//! The write call's `<repo>` block (#413): plain git in the pane's folder, and
//! which paths on the screen exist there. Each test makes its own temp repo.
use cockpit_tui::panel::capture::{repo_state_within, screen_paths};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime};

struct Repo(PathBuf);

impl Repo {
    fn new(tag: &str) -> Repo {
        let d = std::env::temp_dir().join(format!("cockpit-panel-repo-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let r = Repo(d);
        r.git(&["init", "-q", "-b", "main"]);
        r
    }

    fn dir(&self) -> String {
        self.0.display().to_string()
    }

    fn git(&self, args: &[&str]) -> String {
        let o = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn write(&self, name: &str, body: &str) {
        let p = self.0.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// An hour ahead: same bytes, a stat the index no longer matches.
    fn stat_dirty(&self, name: &str) {
        let f = std::fs::File::options().write(true).open(self.0.join(name)).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(3600)).unwrap();
    }

    fn commit(&self, name: &str, body: &str, msg: &str) {
        self.write(name, body);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A section's lines, trimmed: an unindented `head` line, then lines indented by two.
fn section(block: &str, head: &str) -> Vec<String> {
    let mut it = block.lines().skip_while(|l| *l != head);
    assert!(it.next().is_some(), "no {head} in:\n{block}");
    it.map_while(|l| l.strip_prefix("  ")).map(|l| l.trim().to_string()).collect()
}

fn line<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    block.lines().find_map(|l| l.strip_prefix(key))
}

/// What a test waits for the block. The product's 300 ms `REPO_BUDGET` leaves a
/// part out on a loaded box, and these tests check the parts (#596).
const BUDGET: Duration = Duration::from_secs(10);

/// The block under [`BUDGET`].
fn block(dir: &str, screen: &str) -> String {
    repo_state_within(dir, screen, BUDGET)
}

#[test]
fn a_folder_outside_any_repo_gives_nothing() {
    let d = std::env::temp_dir().join(format!("cockpit-panel-repo-none-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let got = block(&d.display().to_string(), "see ./a.rs");
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(got, "");
}

#[test]
fn an_empty_or_missing_folder_gives_nothing() {
    assert_eq!(block("", ""), "");
    assert_eq!(block("/nonexistent/cockpit-panel-repo", ""), "");
}

#[test]
fn the_branch_is_named() {
    let r = Repo::new("branch");
    r.commit("a.txt", "a\n", "first");
    r.git(&["checkout", "-q", "-b", "feat/x"]);
    assert_eq!(line(&block(&r.dir(), ""), "branch: "), Some("feat/x"));
}

#[test]
fn a_detached_head_is_named_by_its_sha() {
    let r = Repo::new("detached");
    r.commit("a.txt", "a\n", "first");
    r.commit("a.txt", "b\n", "second");
    let sha = r.git(&["rev-parse", "--short", "HEAD~1"]);
    r.git(&["checkout", "-q", "--detach", "HEAD~1"]);
    let want = format!("detached at {sha}");
    assert_eq!(line(&block(&r.dir(), ""), "branch: "), Some(want.as_str()));
}

#[test]
fn a_clean_tree_says_so() {
    let r = Repo::new("clean");
    r.commit("a.txt", "a\n", "first");
    assert_eq!(line(&block(&r.dir(), ""), "status: "), Some("clean"));
}

#[test]
fn status_is_capped_at_15_lines_with_the_rest_counted() {
    // rows: (untracked files, lines shown, the "and N more" line)
    for (n, shown, more) in [(15, 15, None), (16, 15, Some("and 1 more")), (20, 15, Some("and 5 more"))] {
        let r = Repo::new(&format!("cap{n}"));
        r.commit("a.txt", "a\n", "first");
        (0..n).for_each(|i| r.write(&format!("f{i:02}.txt"), "x"));
        let s = section(&block(&r.dir(), ""), "status:");
        let files = s.iter().filter(|l| l.starts_with("?? ")).count();
        assert_eq!(
            (files, s.iter().find(|l| l.starts_with("and ")).map(String::as_str)),
            (shown, more),
            "{n}"
        );
    }
}

#[test]
fn the_log_holds_the_last_five_commits() {
    let r = Repo::new("log");
    (1..=7).for_each(|i| r.commit("a.txt", &format!("{i}\n"), &format!("commit number {i}")));
    let s = section(&block(&r.dir(), ""), "recent commits:");
    let msgs: Vec<&str> = s.iter().map(|l| l.split_once(' ').unwrap().1).collect();
    assert_eq!(
        msgs,
        [
            "commit number 7",
            "commit number 6",
            "commit number 5",
            "commit number 4",
            "commit number 3"
        ]
    );
}

#[test]
fn the_diff_stat_names_the_changed_file_and_its_summary() {
    let r = Repo::new("diff");
    r.commit("src/lib.rs", "a\nb\n", "first");
    r.write("src/lib.rs", "a\nc\nd\n");
    let s = section(&block(&r.dir(), ""), "diff --stat:");
    assert!(s.iter().any(|l| l.starts_with("src/lib.rs |")), "{s:?}");
    assert_eq!(s.last().map(String::as_str), Some("1 file changed, 2 insertions(+), 1 deletion(-)"));
}

#[test]
fn a_staged_change_is_in_the_diff_stat() {
    let r = Repo::new("staged");
    r.commit("a.txt", "a\n", "first");
    r.write("a.txt", "b\n");
    r.git(&["add", "a.txt"]);
    let s = section(&block(&r.dir(), ""), "diff --stat:");
    assert!(s.iter().any(|l| l.starts_with("a.txt |")), "{s:?}");
}

#[test]
fn paths_on_the_screen_are_reported_as_there_or_missing() {
    let r = Repo::new("paths");
    r.commit("src/main.rs", "fn main() {}\n", "first");
    let abs = format!("{}/src/main.rs", r.dir());
    let screen = format!("error at src/main.rs:3:5\n  see ./docs/gone.md, lib/none.rs and @{abs}");
    let s = section(&block(&r.dir(), &screen), "paths on screen:");
    assert_eq!(
        s,
        [
            "exists: src/main.rs".to_string(),
            "missing: ./docs/gone.md".to_string(),
            format!("exists: @{abs}"),
        ]
    );
}

#[test]
fn a_screen_without_paths_has_no_paths_section() {
    let r = Repo::new("nopaths");
    r.commit("a.txt", "a\n", "first");
    assert!(!block(&r.dir(), "$ ls\nhello world").contains("paths on screen:"));
}

#[test]
fn path_tokens_are_the_four_shapes_and_nothing_else() {
    // rows: (screen text, the tokens taken)
    let rows: [(&str, &[&str]); 10] = [
        ("open @/etc/hosts now", &["@/etc/hosts"]),
        ("cat ~/notes.txt", &["~/notes.txt"]),
        ("run ./build.sh", &["./build.sh"]),
        ("(see tui/src/app.rs:120)", &["tui/src/app.rs"]),
        ("visit https://example.com/a/b.html", &[]),
        ("a/b and 1/2 and and/or", &[]),
        ("git log origin/master..HEAD", &[]),
        ("cloned github.com/foo/bar.git and https:x/y.rs", &[]),
        ("about 1/2.5 of it", &[]),
        ("`lib/x.py`, \"docs/y.md\".", &["lib/x.py", "docs/y.md"]),
    ];
    for (screen, want) in rows {
        assert_eq!(screen_paths(screen), want, "{screen}");
    }
}

#[test]
fn path_tokens_are_deduplicated_and_capped_at_10() {
    let screen: String = (0..14).map(|i| format!("d/f{i}.rs d/f{i}.rs\n")).collect();
    let got = screen_paths(&screen);
    assert_eq!(got.len(), 10);
    assert_eq!((got[0].as_str(), got[9].as_str()), ("d/f0.rs", "d/f9.rs"));
}

#[test]
fn a_stat_dirty_file_never_makes_git_rewrite_the_index() {
    let r = Repo::new("index");
    r.commit("a.txt", "a\n", "first");
    r.stat_dirty("a.txt");
    let idx = |r: &Repo| std::fs::metadata(r.0.join(".git/index")).map(|m| (m.ino(), m.mtime())).unwrap();
    let before = idx(&r);
    assert_eq!(line(&block(&r.dir(), ""), "status: "), Some("clean"));
    assert_eq!(idx(&r), before, "the index was rewritten");
}

#[test]
fn a_submodules_own_clean_filter_never_runs() {
    let sub = Repo::new("sub-inner");
    sub.commit("f.txt", "x\n", "sub first");
    let r = Repo::new("sub-outer");
    r.commit("a.txt", "a\n", "first");
    r.git(&["-c", "protocol.file.allow=always", "submodule", "add", "-q", &sub.dir(), "sub"]);
    r.git(&["commit", "-q", "-m", "add sub"]);
    let marker = r.0.join("ran");
    let cfg = r.0.join(".git/modules/sub/config").display().to_string();
    r.git(&[
        "config",
        "-f",
        &cfg,
        "filter.mark.clean",
        &format!("touch {}; cat", marker.display()),
    ]);
    r.write(".git/modules/sub/info/attributes", "* filter=mark\n");
    r.stat_dirty("sub/f.txt");
    let _ = block(&r.dir(), "");
    assert!(!marker.exists(), "the submodule's clean filter ran");
}

#[test]
fn color_config_in_the_repo_is_ignored() {
    let r = Repo::new("color");
    r.commit("a.txt", "a\n", "first");
    r.git(&["config", "color.ui", "always"]);
    r.write("a.txt", "b\n");
    r.write("new.txt", "n");
    let got = block(&r.dir(), "");
    assert!(got.contains("a.txt |") && !got.contains('\x1b'), "{got:?}");
}

#[test]
fn a_pane_in_a_subfolder_sees_repo_paths_and_the_root() {
    let r = Repo::new("subdir");
    r.commit("src/a.rs", "a\n", "first");
    r.write("src/a.rs", "b\n");
    let got = block(&format!("{}/src", r.dir()), "");
    let root = std::fs::canonicalize(&r.0).unwrap().display().to_string();
    assert_eq!(line(&got, "root: "), Some(root.as_str()));
    assert_eq!(section(&got, "status:"), ["M src/a.rs"]);
}
