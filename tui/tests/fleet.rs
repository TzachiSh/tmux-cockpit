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

#[test]
fn cockpit_dir_wins_over_the_cwd_when_it_names_a_directory() {
    let wt = Repo::new("wt");
    let panes = [("/main/checkout".to_string(), wt.path()), ("/other".to_string(), String::new())];
    assert_eq!(work_dirs(&panes), vec![wt.path(), "/other".to_string()]);
}

#[test]
fn a_dangling_cockpit_dir_falls_back_to_the_cwd() {
    let panes = [("/main/checkout".to_string(), "/no/such/tree".to_string())];
    assert_eq!(work_dirs(&panes), vec!["/main/checkout".to_string()]);
    assert_eq!(work_dirs(&[]), Vec::<String>::new());
}
