//! The board's diff count (`tree_stat`, `cockpit-tui diffstat`) against a repo
//! whose gitlinks carry their own config (#428), and the real index it must
//! never rewrite. Each test makes its own temp repos.
use cockpit_tui::data::gitsafe::tree_stat;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
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

    fn git<S: AsRef<OsStr> + std::fmt::Debug>(&self, args: &[S]) -> String {
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

    fn write(&self, name: impl AsRef<Path>, body: &str) {
        let p = self.0.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// An hour ahead: same bytes, a stat the index no longer matches.
    fn stat_dirty(&self, name: impl AsRef<Path>) {
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
    fn plant(&self, gd: &Path, file: &Path) {
        let cfg = self.0.join(gd).join("config");
        let cmd = format!("touch {}; cat", self.marker().display());
        let set = [
            "config".as_ref(),
            "-f".as_ref(),
            cfg.as_os_str(),
            "filter.mark.clean".as_ref(),
            cmd.as_ref(),
        ];
        self.git(&set);
        self.write(gd.join("info/attributes"), "* filter=mark\n");
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
    r.plant(Path::new(".git/modules/sub"), Path::new("sub/f.txt"));
    assert_eq!(r.stat(), Some((0, 0, 0, "clean".into())));
    assert!(!r.marker().exists(), "the submodule's clean filter ran");
}

/// A repo `git add`ed at `name` (a gitlink with no .gitmodules entry), its own
/// clean filter planted: tree_stat never runs it.
fn embedded_filter_never_runs(tag: &str, name: &OsStr) {
    let r = Repo::new(tag);
    r.commit("a.txt", "a\n", "first");
    let emb = Repo(r.0.join(name));
    std::fs::create_dir(&emb.0).unwrap();
    emb.git(&["init", "-q", "-b", "main"]);
    emb.commit("e.txt", "e\n", "emb first");
    r.git(&["add".as_ref(), name]);
    r.git(&["commit", "-q", "-m", "add emb"]);
    r.plant(&Path::new(name).join(".git"), &Path::new(name).join("e.txt"));
    assert_eq!(r.stat(), Some((0, 0, 0, "clean".into())));
    assert!(!r.marker().exists(), "the embedded repo's clean filter ran");
}

#[test]
fn an_added_embedded_repos_own_clean_filter_never_runs() {
    embedded_filter_never_runs("embedded", OsStr::new("emb"));
}

/// Read as lossy UTF-8, `\xff` would turn into U+FFFD and the exclude miss it.
#[test]
fn a_gitlink_at_a_non_utf8_path_never_runs_its_clean_filter() {
    embedded_filter_never_runs("nonutf8", OsStr::from_bytes(b"bad\xffname"));
}

/// prefix r counts a moved submodule pointer as `1 1 sub`: so does the board.
#[test]
fn a_submodule_pointer_bump_is_counted() {
    let (r, _sub) = with_submodule("bump");
    r.write("sub/g.txt", "y\n");
    r.git(&["-C", "sub", "add", "g.txt"]);
    r.git(&["-C", "sub", "commit", "-q", "-m", "bump"]);
    assert_eq!(r.stat(), Some((1, 1, 0, "+1 -1".into())));
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

/// The excludes reach add only through their file: with no temp dir to write
/// it in, diffstat fails rather than run the add bare.
#[test]
fn no_room_for_the_excludes_is_a_failure_and_runs_nothing() {
    let (r, _sub) = with_submodule("nofile");
    r.plant(Path::new(".git/modules/sub"), Path::new("sub/f.txt"));
    let o = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["diffstat", &r.dir()])
        .env("TMPDIR", r.0.join("no-such-dir"))
        .output()
        .unwrap();
    assert_eq!((o.stdout.is_empty(), o.status.code()), (true, Some(1)));
    assert!(!r.marker().exists(), "the add ran without its excludes");
}
