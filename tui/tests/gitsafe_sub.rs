//! The board's diff count (`tree_stat`, `cockpit-tui diffstat`) against a repo
//! whose gitlinks carry their own config (#428), and the real index it must
//! never rewrite. Each test makes its own temp repos.
use cockpit_tui::data::gitsafe::tree_stat;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime};

struct Repo(PathBuf);

impl Repo {
    fn new(tag: &str) -> Repo {
        let name = format!("cockpit-gitsafe-sub-{tag}-{}", std::process::id());
        let d = std::env::temp_dir().join(name);
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

    fn stat(&self) -> Option<(u64, u64, u64, String)> {
        tree_stat(&self.dir(), &self.0.join(".git").display().to_string(), None)
    }

    /// The marker a planted clean filter touches.
    fn marker(&self) -> PathBuf {
        self.0.join("ran")
    }

    /// A clean filter in the git dir `gd` (relative to the repo) for every path,
    /// touched stat-dirty under `file`, and proven live: plain status runs it.
    fn plant(&self, gd: &str, file: &str) {
        let cfg = format!("{}/{gd}/config", self.dir());
        let cmd = format!("touch {}; cat", self.marker().display());
        self.git(&["config", "-f", &cfg, "filter.mark.clean", &cmd]);
        self.write(&format!("{gd}/info/attributes"), "* filter=mark\n");
        self.stat_dirty(file);
        self.git(&["status", "--porcelain"]);
        assert!(self.marker().exists(), "the plant never fired, so this proves nothing");
        std::fs::remove_file(self.marker()).unwrap();
        self.stat_dirty(file);
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A superproject with `sub` registered as a submodule of a one-commit repo.
fn with_submodule(tag: &str) -> (Repo, Repo) {
    let sub = Repo::new(&format!("{tag}-inner"));
    sub.commit("f.txt", "x\n", "sub first");
    let r = Repo::new(tag);
    r.commit("a.txt", "a\n", "first");
    r.git(&["-c", "protocol.file.allow=always", "submodule", "add", "-q", &sub.dir(), "sub"]);
    r.git(&["commit", "-q", "-m", "add sub"]);
    (r, sub)
}

#[test]
fn a_submodules_own_clean_filter_never_runs() {
    let (r, _sub) = with_submodule("registered");
    r.plant(".git/modules/sub", "sub/f.txt");
    assert_eq!(r.stat(), Some((0, 0, 0, "clean".into())));
    assert!(!r.marker().exists(), "the submodule's clean filter ran");
}

#[test]
fn an_added_embedded_repos_own_clean_filter_never_runs() {
    let r = Repo::new("embedded");
    r.commit("a.txt", "a\n", "first");
    let emb = Repo(r.0.join("emb"));
    std::fs::create_dir(&emb.0).unwrap();
    emb.git(&["init", "-q", "-b", "main"]);
    emb.commit("e.txt", "e\n", "emb first");
    r.git(&["add", "emb"]); // a gitlink with no .gitmodules entry
    r.git(&["commit", "-q", "-m", "add emb"]);
    r.plant("emb/.git", "emb/e.txt");
    assert_eq!(r.stat(), Some((0, 0, 0, "clean".into())));
    assert!(!r.marker().exists(), "the embedded repo's clean filter ran");
}

#[test]
fn the_superprojects_own_change_is_still_counted_beside_a_submodule() {
    let (r, _sub) = with_submodule("counted");
    r.write("a.txt", "b\n");
    r.write("n.txt", "n\n");
    r.write("sub/f.txt", "y\n");
    assert_eq!(r.stat(), Some((2, 1, 1, "+2 -1  ?1 new".into())));
}

#[test]
fn a_stat_dirty_file_never_makes_tree_stat_rewrite_the_index() {
    let r = Repo::new("index");
    r.commit("a.txt", "a\n", "first");
    r.stat_dirty("a.txt");
    let idx = |r: &Repo| {
        let m = std::fs::metadata(r.0.join(".git/index")).unwrap();
        (m.ino(), m.mtime())
    };
    let before = idx(&r);
    assert_eq!(r.stat(), Some((0, 0, 0, "clean".into())));
    assert_eq!(idx(&r), before, "the index was rewritten");
}
