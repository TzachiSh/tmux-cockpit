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
/// When the main checkout's `.git` is a file (`--separate-git-dir`, a submodule), git
/// prints the git dir as the main record's path. That is no tree and holds no `.git`,
/// so the record is dropped (#537); the fallback's [`ours`] drops it too. A first
/// record with a `.git`, a linked tree's file included, is a tree and kept (#560).
fn worktrees(dir: &str, top: &str) -> Vec<(String, Option<String>)> {
    let z = git(dir, &[], None, &["worktree", "list", "--porcelain", "-z"]);
    if !z.is_empty() {
        let mut v = records(z.split('\0'), true);
        if v.first().is_some_and(|(p, _)| !Path::new(p).join(".git").exists()) {
            v.remove(0);
        }
        return v;
    }
    // Asked from the top: before 2.13 the answer was the cwd's prefix plus a path from the top.
    let top = if top.is_empty() { dir } else { top };
    let common = git(top, &[], None, &["rev-parse", "--git-common-dir"]);
    let common = common.trim_end_matches('\n');
    if common.is_empty() {
        return Vec::new();
    }
    let common = Path::new(top).join(common);
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
    let mut cur: Option<(String, Option<String>, bool)> = None;
    let mut flush = |cur: &mut Option<(String, Option<String>, bool)>| {
        // An empty path names nothing; tmux-branch drops it too.
        if let Some((p, b, true)) = cur.take()
            && !p.is_empty()
        {
            out.push((p, b));
        }
    };
    for f in fields {
        if let Some(p) = f.strip_prefix("worktree ") {
            flush(&mut cur);
            cur = Some((p.to_string(), None, true));
        } else if let Some((_, b, ok)) = cur.as_mut() {
            if let Some(r) = f.strip_prefix("branch refs/heads/") {
                *b = Some(r.to_string());
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
    use super::{records, same};

    fn plain(s: &str) -> Vec<(String, Option<String>)> {
        records(s.split('\n'), false)
    }

    #[test]
    fn a_whole_bare_or_detached_line_is_a_label_that_keeps_the_record() {
        for l in ["bare", "detached"] {
            let got = plain(&format!("worktree /none/d\nHEAD 0\n{l}\n"));
            assert_eq!(got, [("/none/d".to_string(), None)], "{l}");
        }
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
