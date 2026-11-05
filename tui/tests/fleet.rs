//! data/fleet.rs against scratch repos: the facts a list row draws, and the
//! rule that two panes on one root cost one diff scan.
use cockpit_tui::data::fleet::{scan, stat_text, work_dirs};
use std::path::PathBuf;
use std::process::Command;

struct Repo(PathBuf);

impl Repo {
    fn new(tag: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!("cockpit-fleet-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let r = Repo(dir);
        r.git(&["init", "-q", "-b", "main", "."]);
        r.write("f", "a\n");
        r.git(&["add", "-A"]);
        r.git(&["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "init"]);
        r
    }
    fn git(&self, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }
    fn write(&self, name: &str, body: &str) {
        std::fs::write(self.0.join(name), body).unwrap();
    }
    fn path(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn row(path: &str) -> String {
    let (loc, pm, _) = scan(&[path.to_string()]).facts.remove(path).unwrap();
    stat_text(&loc, &pm)
}

#[test]
fn a_clean_repo_reads_clean() {
    assert_eq!(row(&Repo::new("clean").path()), "clean");
}

#[test]
fn a_dirty_repo_reads_added_and_deleted() {
    let r = Repo::new("dirty");
    r.write("f", "a\nb\nc\n");
    assert_eq!(row(&r.path()), "+2 -0");
}

#[test]
fn untracked_files_count_in_the_stat_and_are_named_new() {
    let r = Repo::new("untracked");
    r.write("n1", "x\n");
    r.write("n2", "y\n");
    assert_eq!(row(&r.path()), "+2 -0 ?2 new");
}

#[test]
fn a_directory_outside_any_repo_reads_no_repo() {
    let d = std::env::temp_dir().join(format!("cockpit-fleet-norepo-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    // an ancestor repo would claim it; the scratch dir sits under $TMPDIR
    let p = d.display().to_string();
    let got = scan(std::slice::from_ref(&p)).facts.remove(&p).unwrap();
    std::fs::remove_dir_all(&d).unwrap();
    assert_eq!(stat_text(&got.0, &got.1), "no repo");
    assert_eq!(got.2, "");
}

#[test]
fn the_branch_and_root_come_back_with_the_stat() {
    let r = Repo::new("facts");
    let (loc, _, root) = scan(&[r.path()]).facts.remove(&r.path()).unwrap();
    assert!(loc.ends_with("⑂main"), "{loc}");
    assert_eq!(root, r.path());
}

fn loc(path: &str) -> String {
    scan(&[path.to_string()]).facts.remove(path).unwrap().0
}

fn name(r: &Repo) -> String {
    r.0.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn a_linked_worktree_reads_main_repo_branch_and_worktree_name() {
    let r = Repo::new("wtrow");
    let wt = r.0.with_file_name(format!("{}-wt", name(&r)));
    let _ = std::fs::remove_dir_all(&wt);
    r.git(&["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()]);
    let p = wt.display().to_string();
    let (l, _, root) = scan(std::slice::from_ref(&p)).facts.remove(&p).unwrap();
    let wt_name = wt.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::remove_dir_all(&wt).unwrap();
    assert_eq!(l, format!("{}⑂feat · wt {wt_name}", name(&r)));
    assert_eq!(root, p);
}

#[test]
fn a_subdirectory_of_the_main_checkout_is_not_a_worktree() {
    // rev-parse prints the common dir relative to the subdirectory: `../.git`
    let r = Repo::new("wtsub");
    std::fs::create_dir(r.0.join("sub")).unwrap();
    let sub = r.0.join("sub").display().to_string();
    assert_eq!(loc(&sub), format!("{}⑂main", name(&r)));
}

#[test]
fn a_detached_head_reads_as_its_short_sha() {
    let r = Repo::new("detached");
    r.git(&["checkout", "-q", "--detach"]);
    let sha = Command::new("git")
        .arg("-C")
        .arg(&r.0)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;
    let sha = String::from_utf8(sha).unwrap();
    assert_eq!(loc(&r.path()), format!("{}⑂@{}", name(&r), &sha[..7]));
}

#[test]
fn an_unborn_repo_keeps_its_row() {
    let d = std::env::temp_dir().join(format!("cockpit-fleet-unborn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let ok = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args(["init", "-q", "-b", "main"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    let got = loc(&d.display().to_string());
    std::fs::remove_dir_all(&d).unwrap();
    assert!(ok.success());
    assert_eq!(got, format!("cockpit-fleet-unborn-{}⑂HEAD", std::process::id()));
}

#[test]
fn two_panes_on_one_root_are_scanned_once() {
    let r = Repo::new("shared");
    std::fs::create_dir(r.0.join("sub")).unwrap();
    let sub = r.0.join("sub").display().to_string();
    r.write("f", "a\nb\n");
    let s = scan(&[r.path(), sub.clone(), r.path()]);
    assert_eq!(s.stats, 1, "one root, one diff scan");
    assert_eq!(s.facts.len(), 2, "both paths answered");
    assert_eq!(s.facts[&sub], s.facts[&r.path()]);
}

#[test]
fn two_roots_are_two_scans_and_nothing_is_one_scan_less() {
    let (a, b) = (Repo::new("two-a"), Repo::new("two-b"));
    assert_eq!(scan(&[a.path(), b.path()]).stats, 2);
    let none = scan(&[]);
    assert_eq!((none.stats, none.facts.len()), (0, 0));
}

/// `[@cockpit_dir, transcript cwd, pane_current_path]`, as `work_dirs` takes them.
fn cands(opt: &str, said: &str, cwd: &str) -> [String; 3] {
    [opt.into(), said.into(), cwd.into()]
}

#[test]
fn cockpit_dir_wins_over_the_cwd_when_it_names_a_directory() {
    let wt = Repo::new("wt");
    let panes = [cands(&wt.path(), "", "/main/checkout"), cands("", "", "/other")];
    assert_eq!(work_dirs(&panes), vec![wt.path(), "/other".to_string()]);
}

#[test]
fn a_dangling_cockpit_dir_falls_back_to_the_cwd() {
    let panes = [cands("/no/such/tree", "", "/main/checkout")];
    assert_eq!(work_dirs(&panes), vec!["/main/checkout".to_string()]);
    assert_eq!(work_dirs(&[]), Vec::<String>::new());
}

/// #452: rows: the option outranks the transcript's cwd; the transcript's cwd
/// outranks the pane's; a dangling transcript cwd falls through to the pane's.
#[test]
fn the_transcript_cwd_sits_between_the_option_and_the_pane_path() {
    let (opt, said) = (Repo::new("order-opt"), Repo::new("order-said"));
    let panes = [
        cands(&opt.path(), &said.path(), "/main"),
        cands("", &said.path(), "/main"),
        cands("/no/such/opt", "/no/such/said", "/main"),
    ];
    assert_eq!(work_dirs(&panes), vec![opt.path(), said.path(), "/main".to_string()]);
}

/// #452 review: a relative candidate would be read against the board's own cwd,
/// so it never counts; a relative path left over gets no git facts at all.
#[test]
fn a_relative_candidate_never_counts_and_gets_no_facts() {
    let panes = [cands(".", "src", "/main")];
    assert_eq!(work_dirs(&panes), vec!["/main".to_string()]);
    // the test runs inside a checkout, so "." would describe it
    let s = scan(&[".".to_string()]);
    assert_eq!((s.facts["."].clone(), s.stats), (Default::default(), 0));
}
