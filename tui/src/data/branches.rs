//! What the branch popup reads from git (#246). Every call goes through
//! `gitsafe`'s flags, so a repo's own config runs nothing as the user.
use super::gitsafe::git;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// The ref without its namespace: `main`, or `origin/main` for a remote-only one.
    pub name: String,
    pub remote: bool,
    pub age: String,
    pub current: bool,
    /// The full path of the other worktree that has it checked out.
    pub elsewhere: Option<String>,
}

impl Branch {
    /// The full ref, so a name that looks like an option or a path is neither.
    pub fn full_ref(&self) -> String {
        format!("refs/{}/{}", if self.remote { "remotes" } else { "heads" }, self.name)
    }
}

/// False when `dir` is not inside a git repository.
pub fn is_repo(dir: &str) -> bool {
    !git(dir, &[], None, &["rev-parse", "--git-dir"]).trim().is_empty()
}

fn refs(dir: &str, ns: &str) -> Vec<(String, String)> {
    git(
        dir,
        &[],
        None,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%09%(committerdate:relative)",
            ns,
        ],
    )
    .lines()
    .filter_map(|l| {
        let (r, age) = l.split_once('\t')?;
        Some((r.strip_prefix(ns)?.strip_prefix('/')?.to_string(), age.to_string()))
    })
    .collect()
}

/// Every worktree as `(path, branch)`. `-z` keeps a path with a newline in one
/// record, but needs git 2.36; older git rejects it and prints nothing, so the
/// plain porcelain is read instead, and a record whose path was cut by a newline
/// (a line that is no worktree attribute) is dropped rather than guessed at. So
/// is one that is not this repo's (#348, #532): a newline name can forge a whole
/// `worktree <dir>` line, and `<dir>` may be any checkout. See [`ours`].
/// git's main record is no tree when it is the common git dir (`--separate-git-dir`, a
/// submodule's, a bare repo's) or carries `bare` (a bare repo in `<proj>/.git`, listed
/// as `<proj>`, or `core.bare` on a normal repo). Either is dropped, whatever sits in it
/// (#537, #560): [`records`] drops a bare record, `-z` a common dir, and the fallback's
/// [`ours`] any git dir. A main checkout or a linked tree is neither, wherever listed.
fn worktrees(dir: &str, top: &str) -> Vec<(String, Option<String>)> {
    // Asked from the top: before 2.13 the answer was the cwd's prefix plus a path from the top.
    let top = if top.is_empty() { dir } else { top };
    let common = git(top, &[], None, &["rev-parse", "--git-common-dir"]);
    let common = common.trim_end_matches('\n');
    if common.is_empty() {
        return Vec::new();
    }
    let common = Path::new(top).join(common);
    let z = git(dir, &[], None, &["worktree", "list", "--porcelain", "-z"]);
    if !z.is_empty() {
        let mut v = records(z.split('\0'), true);
        v.retain(|(p, _)| !same(Path::new(p), &common));
        return v;
    }
    let plain = git(dir, &[], None, &["worktree", "list", "--porcelain"]);
    let mut v = records(plain.split('\n'), false);
    v.retain(|(p, b)| ours(Path::new(p), b.as_deref(), &common));
    v
}

/// True for a worktree of the repo whose common dir is `common`, as tmux-branch's
/// `wt_ours` decides it, and holding `branch` if it names one. A forged record can
/// pair a real tree's path with a cut tree's branch line; that tree's own HEAD names
/// its real branch. A path that is gone is kept only as git would call it prunable.
fn ours(p: &Path, branch: Option<&str>, common: &Path) -> bool {
    if !p.exists() {
        return prunable(p, common);
    }
    let Some(e) = entry(p, common) else { return false };
    branch.is_none_or(|b| heads(&e.join("HEAD"), b))
}

/// True when the HEAD file `head` names `refs/heads/<b>`: as a `ref:` line, or as a
/// symlink to that ref, which `core.preferSymlinkRefs` writes.
fn heads(head: &Path, b: &str) -> bool {
    let want = format!("refs/heads/{b}");
    match std::fs::read_link(head) {
        Ok(t) => t.as_os_str() == want.as_str(),
        Err(_) => first_line(head).is_some_and(|h| h.strip_prefix("ref: ") == Some(want.as_str())),
    }
}

/// True when some `<common>/worktrees/*/gitdir` names `<p>/.git`, as git decides that
/// a gone tree is prunable. The file is read whole, as git reads it, and a relative one
/// is taken from its entry. A text compare, since a missing path has no inode.
fn prunable(p: &Path, common: &Path) -> bool {
    use std::io::Read;
    let want = p.join(".git");
    let Ok(dir) = std::fs::read_dir(common.join("worktrees")) else {
        return false;
    };
    dir.flatten().any(|d| {
        let mut s = String::new();
        let f = std::fs::File::open(d.path().join("gitdir"));
        let read = f.and_then(|f| f.take(4096).read_to_string(&mut s)).is_ok();
        let b = s.trim_end_matches(['\n', '\r']);
        read && !b.is_empty() && d.path().join(b).as_os_str() == want.as_os_str()
    })
}

/// The git dir of the tree at `p` when the tree is this repo's. The main checkout's
/// `.git` is the common dir; a linked tree's `.git` file names a dir in its
/// `worktrees/` whose `gitdir` file names that `.git` back. Links are followed, as
/// git follows them.
fn entry(p: &Path, common: &Path) -> Option<PathBuf> {
    let dot = p.join(".git");
    if dot.is_dir() {
        return same(&dot, common).then_some(dot);
    }
    let g = p.join(first_line(&dot)?.strip_prefix("gitdir: ")?);
    let home = same(&g.join(".."), &common.join("worktrees"));
    let back = home && first_line(&g.join("gitdir")).is_some_and(|b| same(&g.join(b), &dot));
    back.then_some(g)
}

/// Both paths exist and are the same file (device and inode), as the shell's `-ef`.
fn same(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let id = |p: &Path| std::fs::metadata(p).map(|m| (m.dev(), m.ino()));
    matches!((id(a), id(b)), (Ok(x), Ok(y)) if x == y)
}

/// The first line of a regular file, read no further than 4 KiB.
fn first_line(f: &Path) -> Option<String> {
    use std::io::Read;
    if !f.is_file() {
        return None;
    }
    let mut s = String::new();
    std::fs::File::open(f).ok()?.take(4096).read_to_string(&mut s).ok()?;
    let l = s.lines().next()?;
    (!l.is_empty()).then(|| l.to_string())
}

fn records<'a>(fields: impl Iterator<Item = &'a str>, nul: bool) -> Vec<(String, Option<String>)> {
    // As git's porcelain prints them: each label is a whole line, and `locked`
    // and `prunable` may also carry a reason after a space.
    const LABELS: [&str; 4] = ["bare", "detached", "locked", "prunable"];
    const PREFIXES: [&str; 4] = ["HEAD ", "branch ", "locked ", "prunable "];
    let attr = |f: &str| LABELS.contains(&f) || PREFIXES.iter().any(|a| f.starts_with(a));
    let mut out = Vec::new();
    // (path, branch, not cut, carries bare)
    let mut cur: Option<(String, Option<String>, bool, bool)> = None;
    let mut flush = |cur: &mut Option<(String, Option<String>, bool, bool)>| {
        // An empty path names nothing, and a bare record is a git dir; tmux-branch drops both.
        if let Some((p, b, true, false)) = cur.take()
            && !p.is_empty()
        {
            out.push((p, b));
        }
    };
    for f in fields {
        if let Some(p) = f.strip_prefix("worktree ") {
            flush(&mut cur);
            cur = Some((p.to_string(), None, true, false));
        } else if let Some((_, b, ok, bare)) = cur.as_mut() {
            if let Some(r) = f.strip_prefix("branch refs/heads/") {
                *b = Some(r.to_string());
            } else if f == "bare" {
                *bare = true;
            } else if !nul && !f.is_empty() && !attr(f) {
                *ok = false;
            }
        }
    }
    flush(&mut cur);
    out
}

/// Branch name to the other worktree's full path, for every linked
/// worktree that is not `dir`'s own: git refuses to switch to those.
fn elsewhere(dir: &str) -> Vec<(String, String)> {
    let top = git(dir, &[], None, &["rev-parse", "--show-toplevel"]);
    let top = top.trim_end_matches('\n');
    worktrees(dir, top)
        .into_iter()
        .filter_map(|(p, b)| if p == top { None } else { Some((b?, p)) })
        .collect()
}

/// Local branches by most recent commit, then remote-only ones the same way.
pub fn list(dir: &str) -> Vec<Branch> {
    let cur = git(dir, &[], None, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let cur = cur.trim_end_matches('\n');
    let wt = elsewhere(dir);
    let local = refs(dir, "refs/heads");
    let mut out: Vec<Branch> = local
        .iter()
        .map(|(n, age)| Branch {
            name: n.clone(),
            remote: false,
            age: age.clone(),
            current: n == cur,
            elsewhere: wt.iter().find(|(b, _)| b == n).map(|(_, d)| d.clone()),
        })
        .collect();
    for (r, age) in refs(dir, "refs/remotes") {
        // origin/HEAD is a pointer, not a branch; and a local branch of that name wins
        let Some((_, short)) = r.split_once('/') else { continue };
        if short == "HEAD" || local.iter().any(|(n, _)| n == short) {
            continue;
        }
        out.push(Branch {
            name: r,
            remote: true,
            age,
            current: false,
            elsewhere: None,
        });
    }
    out
}

/// `git log --graph` of one branch, as plain text.
pub fn graph(dir: &str, b: &Branch) -> String {
    let r = b.full_ref();
    git(
        dir,
        &[],
        None,
        &["log", "--graph", "--oneline", "--decorate", "--no-color", "-30", &r, "--"],
    )
}

/// Git's own rule for a new branch name. `--branch` prints the name when it is
/// valid and nothing when it is not; it also expands `@{-1}`, so an echo that
/// differs from the input is refused too.
pub fn valid_name(dir: &str, name: &str) -> bool {
    !name.is_empty() && git(dir, &[], None, &["check-ref-format", "--branch", name]).trim_end_matches('\n') == name
}

#[cfg(test)]
mod tests {
    use super::{records, same, worktrees};
    use std::path::Path;
    use std::process::Command;

    fn plain(s: &str) -> Vec<(String, Option<String>)> {
        records(s.split('\n'), false)
    }

    #[test]
    fn a_whole_detached_line_is_a_label_that_keeps_the_record() {
        let got = plain("worktree /none/d\nHEAD 0\ndetached\n");
        assert_eq!(got, [("/none/d".to_string(), None)]);
    }

    #[test]
    fn a_record_carrying_bare_is_a_git_dir_and_dropped_with_z_and_without() {
        let z = "worktree /p\0bare\0\0worktree /p/wa\0HEAD 0\0branch refs/heads/wa\0\0";
        let want = [("/p/wa".to_string(), Some("wa".to_string()))];
        assert_eq!(records(z.split('\0'), true), want, "-z");
        assert_eq!(plain(&z.replace('\0', "\n")), want, "plain");
    }

    fn git(dir: &Path, args: &[&str]) {
        let mut c = Command::new("git");
        c.arg("-C").arg(dir).args(args);
        let c = c.env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@e");
        let c = c.env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@e");
        assert!(c.status().unwrap().success(), "git {args:?}");
    }

    #[test]
    fn a_bare_main_record_that_is_not_the_common_dir_is_dropped() {
        let d = std::env::temp_dir().join(format!("cockpit-bare560-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let d = std::fs::canonicalize(&d).unwrap();
        let s = |p: &str| d.join(p).to_str().unwrap().to_string();
        git(&d, &["init", "-q", "-b", "main", "src"]);
        git(&d.join("src"), &["commit", "-q", "--allow-empty", "-m", "i"]);
        git(&d, &["clone", "-q", "--bare", &s("src"), &s("proj/.git")]);
        git(&d.join("proj"), &["worktree", "add", "-q", "-b", "pw", &s("proj/wa")]);
        git(&d.join("src"), &["worktree", "add", "-q", "-b", "nw", &s("nb-wc")]);
        git(&d.join("src"), &["config", "core.bare", "true"]);
        let got = [s("proj/wa"), s("nb-wc")].map(|w| worktrees(&w, &w));
        let _ = std::fs::remove_dir_all(&d);
        let want = [s("proj/wa"), s("nb-wc")].map(|w| vec![w.clone()]);
        assert_eq!(got.map(|v| v.into_iter().map(|(p, _)| p).collect::<Vec<_>>()), want);
    }

    #[test]
    fn locked_and_prunable_are_labels_alone_or_before_a_space_and_a_reason() {
        for l in ["locked", "locked on usb", "prunable", "prunable gitdir gone"] {
            let got = plain(&format!("worktree /none/d\nHEAD 0\n{l}\n"));
            assert_eq!(got, [("/none/d".to_string(), None)], "{l}");
        }
    }

    #[test]
    fn a_line_that_only_starts_with_locked_or_prunable_cuts_the_record() {
        for l in ["lockedx", "prunablex", "locked\tx"] {
            let got = plain(&format!("worktree /none/d\n{l}\nHEAD 0\nbranch refs/heads/c\n"));
            assert_eq!(got, [], "{l}");
        }
    }

    #[test]
    fn same_is_one_inode_so_a_hard_link_is_the_same_file_as_the_shells_ef() {
        let d = std::env::temp_dir().join(format!("cockpit-same-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a"), "x").unwrap();
        std::fs::hard_link(d.join("a"), d.join("b")).unwrap();
        std::fs::write(d.join("c"), "x").unwrap();
        let (ab, ac) = (same(&d.join("a"), &d.join("b")), same(&d.join("a"), &d.join("c")));
        let _ = std::fs::remove_dir_all(&d);
        assert!(ab, "a hard link");
        assert!(!ac, "a copy");
    }

    #[test]
    fn an_empty_worktree_path_is_dropped() {
        let got = plain("worktree /none/x\nworktree \nHEAD 0\nbranch refs/heads/e\n");
        assert_eq!(got, [("/none/x".to_string(), None)]);
    }
}
