//! The board's diff count (`tree_stat`, `cockpit-tui diffstat`) against a repo
//! whose gitlinks carry their own config (#428), and the real index it must
//! never rewrite. Each test makes its own temp repos.
use cockpit_tui::data::gitsafe::{Stat, copy_index, sweep_stale, tree_stat, tree_stat_with};
use std::ffi::OsStr;
use std::io::{self, ErrorKind};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
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

    /// A plain temp dir, no repo in it.
    fn plain(tag: &str) -> Repo {
        let name = format!("cockpit-gitsafe-sub-{tag}-{}", std::process::id());
        let d = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Repo(d)
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

/// What [`stat_writing`] saw: the result, the scratch dir, whether the filter ran.
type Seen = (Option<Stat>, PathBuf, bool);

/// tree_stat on a repo with a planted submodule filter, its excludes written
/// by `write`, which is handed a path inside the scan's scratch dir.
fn stat_writing(tag: &str, write: impl Fn(&Path, &[u8]) -> io::Result<()>) -> Seen {
    let (r, _sub) = with_submodule(tag);
    r.plant(Path::new(".git/modules/sub"), Path::new("sub/f.txt"));
    let seen = std::cell::RefCell::new(PathBuf::new());
    let rec = |p: &Path, b: &[u8]| {
        *seen.borrow_mut() = p.parent().unwrap().to_path_buf();
        write(p, b)
    };
    let gd = r.0.join(".git").display().to_string();
    let got = catch_unwind(AssertUnwindSafe(|| tree_stat_with(&r.dir(), &gd, None, &rec)));
    (got.unwrap_or(Some((9, 9, 9, "panicked".into()))), seen.take(), r.marker().exists())
}

fn real_write(p: &Path, b: &[u8]) -> io::Result<()> {
    std::fs::write(p, b)
}

/// A disk that fills mid-write: the file is there, empty.
fn no_space(p: &Path, _: &[u8]) -> io::Result<()> {
    std::fs::File::create(p)?;
    Err(ErrorKind::StorageFull.into())
}

/// The excludes reach add only through their file: when that one write fails
/// (the index copy beside it did not), tree_stat fails rather than run the add bare.
#[test]
fn no_room_for_the_excludes_is_a_failure_and_runs_nothing() {
    let (got, dir, ran) = stat_writing("nofile", no_space);
    assert_eq!((got, dir.as_os_str().is_empty(), ran), (None, false, false));
}

#[test]
fn the_scratch_dir_is_private_to_the_user() {
    let mode = std::cell::Cell::new(0);
    stat_writing("mode", |p, b| {
        mode.set(std::fs::metadata(p.parent().unwrap())?.mode() & 0o777);
        real_write(p, b)
    });
    assert_eq!(format!("{:o}", mode.get()), "700");
}

#[test]
fn the_scratch_dir_is_gone_after_a_normal_return() {
    let (got, dir, ran) = stat_writing("normal", real_write);
    assert_eq!((got, ran), (Some((0, 0, 0, "clean".into())), false));
    assert!(dir.is_absolute() && !dir.exists(), "{dir:?} left behind");
}

#[test]
fn the_scratch_dir_is_gone_after_an_early_return() {
    let (got, dir, _) = stat_writing("early", no_space);
    assert_eq!(got, None);
    assert!(dir.is_absolute() && !dir.exists(), "{dir:?} left behind");
}

#[test]
fn the_scratch_dir_is_gone_after_a_panic() {
    let (got, dir, _) = stat_writing("panic", |_, _| panic!("a planted panic"));
    assert_eq!(got, Some((9, 9, 9, "panicked".into())));
    assert!(dir.is_absolute() && !dir.exists(), "{dir:?} left behind");
}

/// `p`'s mtime, `secs` from now (negative is the past); a dir too.
fn age(p: &Path, secs: i64) {
    let t = if secs < 0 {
        SystemTime::now() - Duration::from_secs(secs.unsigned_abs())
    } else {
        SystemTime::now() + Duration::from_secs(secs as u64)
    };
    std::fs::File::open(p).unwrap().set_modified(t).unwrap();
}

/// Age alone says stale, past an hour: whatever pid a name holds, a fresh entry
/// stays and an old one goes. The rest, a symlink matching the pattern
/// included, stays as it was.
#[test]
fn the_sweep_takes_only_what_is_old() {
    let tmp = Repo::plain("sweep");
    let bin = Repo::plain("sweep-victim");
    let victim = bin.0.join("victim");
    std::fs::create_dir(&victim).unwrap();
    std::fs::write(victim.join("f"), "keep").unwrap();
    age(&victim, -7200);
    let (h2, now, ahead) = (-7200, 0, 7200);
    // (row, name, a dir or a file or a symlink to the old victim, mtime, swept)
    let rows = [
        ("a fresh scratch dir", "cockpit-tui-idx.1.00c0ffee00c0ffee", 'd', now, false),
        ("a fresh old-style index copy", "cockpit-tui-idx.99999.0", 'f', now, false),
        ("one dated ahead", "cockpit-tui-idx.7.ab", 'd', ahead, false),
        ("an old scratch dir", "cockpit-tui-idx.1.00c0ffee00c0fff0", 'd', h2, true),
        ("an old index copy", "cockpit-tui-idx.99999.3", 'f', h2, true),
        ("an old excludes file", "cockpit-tui-ps.99999.3", 'f', h2, true),
        ("an old one past u32", "cockpit-tui-idx.99999999999.1", 'f', h2, true),
        ("a symlink to an old dir", "cockpit-tui-idx.99999.4", 'l', now, false),
        ("a tail that is not hex", "cockpit-tui-idx.99999.zz", 'f', h2, false),
        ("no tail", "cockpit-tui-idx.99999.", 'f', h2, false),
        ("no pid", "cockpit-tui-idx..1", 'f', h2, false),
        ("another prefix", "cockpit-tui-idxx.99999.1", 'f', h2, false),
    ];
    for (_, name, kind, secs, _) in rows {
        let p = tmp.0.join(name);
        match kind {
            'd' => {
                std::fs::create_dir(&p).unwrap();
                std::fs::write(p.join("index"), "i").unwrap();
            }
            'f' => std::fs::write(&p, "i").unwrap(),
            _ => std::os::unix::fs::symlink(&victim, &p).unwrap(),
        }
        if kind != 'l' {
            age(&p, secs);
        }
    }
    sweep_stale(&tmp.0);
    let wrong: Vec<&str> = rows
        .iter()
        .filter(|(_, name, _, _, swept)| tmp.0.join(name).symlink_metadata().is_ok() == *swept)
        .map(|(row, ..)| *row)
        .collect();
    assert_eq!(wrong, Vec::<&str>::new(), "rows the sweep got wrong");
    assert_eq!(std::fs::read_to_string(victim.join("f")).unwrap(), "keep");
}

/// The sweep says what it took, one record on stderr with the path in a field.
#[test]
fn the_sweep_names_what_it_removed_on_stderr() {
    let r = Repo::new("sweeplog");
    r.commit("a.txt", "a\n", "first");
    let tmp = Repo::plain("sweeplog-tmp");
    let old = tmp.0.join("cockpit-tui-ps.12.0");
    std::fs::write(&old, "i").unwrap();
    age(&old, -7200);
    let o = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["diffstat", &r.dir()])
        .env("TMPDIR", &tmp.0)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    let paths: Vec<&str> = err.lines().filter_map(|l| l.split(" path=").nth(1)).collect();
    assert_eq!(paths, [format!("{old:?}")]);
}

/// The index copy is a new file: a symlink raced in at its name in the
/// scratch dir is refused, never written through.
#[test]
fn the_index_copy_never_writes_through_a_symlink() {
    let r = Repo::new("copyidx");
    r.commit("a.txt", "a\n", "first");
    let tmp = Repo::plain("copyidx-tmp");
    let victim = tmp.0.join("victim");
    std::fs::write(&victim, "keep\n").unwrap();
    let dst = tmp.0.join("index");
    std::os::unix::fs::symlink(&victim, &dst).unwrap();
    let copied = copy_index(&r.0.join(".git/index"), &dst);
    let after = std::fs::read(&victim).unwrap();
    assert!(
        after == b"keep\n",
        "written through a symlink: the victim holds {} bytes",
        after.len()
    );
    assert_eq!(copied.map_err(|e| e.kind()), Err(ErrorKind::AlreadyExists));
}

#[test]
fn the_sweep_of_a_missing_dir_does_nothing() {
    let none = format!("cockpit-gitsafe-sub-none-{}", std::process::id());
    sweep_stale(&std::env::temp_dir().join(none));
}

fn diffstat_in(r: &Repo, cwd: &Path, tmpdir: &Path) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["diffstat", &r.dir()])
        .current_dir(cwd)
        .env("TMPDIR", tmpdir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Before #435 the index copy and the excludes file sat at
/// `cockpit-tui-{idx,ps}.<pid>.<n>`. A symlink planted at each for the pid that
/// runs diffstat (exec keeps the shell's) is never written through.
#[test]
fn a_symlink_planted_at_a_predictable_name_is_never_written_through() {
    let (r, _sub) = with_submodule("plant");
    let tmp = Repo::plain("plant-tmp");
    let victim = tmp.0.join("victim");
    std::fs::write(&victim, "keep\n").unwrap();
    let plant = r#"for n in 0 1 2 3; do for k in idx ps; do
        ln -s "$1" "$TMPDIR/cockpit-tui-$k.$$.$n" || exit 9; done; done
        exec "$2" diffstat "$3""#;
    let o = Command::new("sh")
        .args(["-c", plant, "sh"])
        .arg(&victim)
        .args([env!("CARGO_BIN_EXE_cockpit-tui"), &r.dir()])
        .env("TMPDIR", &tmp.0)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let after = std::fs::read(&victim).unwrap();
    assert!(
        after == b"keep\n",
        "written through a symlink: the victim holds {} bytes",
        after.len()
    );
}

/// GIT_INDEX_FILE and --pathspec-from-file resolve against `-C root`, the
/// process's own writes against its cwd: a relative TMPDIR read `0 1 0 +0 -1`.
#[test]
fn a_relative_tmpdir_counts_what_an_absolute_one_does() {
    let r = Repo::new("reltmp");
    r.commit("a.txt", "a\n", "first");
    r.write("a.txt", "b\n");
    r.write("n.txt", "n\n");
    let cwd = Repo::plain("reltmp-cwd");
    std::fs::create_dir(cwd.0.join("t")).unwrap();
    let want = "2\t1\t1\t+2 -1  ?1 new\n";
    let got = [Path::new("t").to_path_buf(), cwd.0.join("t")].map(|t| diffstat_in(&r, &cwd.0, &t));
    assert_eq!(got, [want, want]);
}
