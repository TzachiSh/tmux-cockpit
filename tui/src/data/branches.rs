//! What the branch popup reads from git (#246). Every call goes through
//! `gitsafe`'s flags, so a repo's own config runs nothing as the user.
use super::gitsafe::git;

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
/// is one with no `.git` in it (#348): a newline name can forge a whole
/// `worktree <dir>` line. A main checkout's `.git` is a dir, a linked one's a file.
fn worktrees(dir: &str) -> Vec<(String, Option<String>)> {
    let z = git(dir, &[], None, &["worktree", "list", "--porcelain", "-z"]);
    if !z.is_empty() {
        return records(z.split('\0'), true);
    }
    let plain = git(dir, &[], None, &["worktree", "list", "--porcelain"]);
    records(plain.split('\n'), false)
}

fn records<'a>(fields: impl Iterator<Item = &'a str>, nul: bool) -> Vec<(String, Option<String>)> {
    // As git's porcelain prints them: `bare` and `detached` are a whole line,
    // `locked` and `prunable` may carry a reason after a space.
    const LABELS: [&str; 2] = ["bare", "detached"];
    const PREFIXES: [&str; 4] = ["HEAD ", "branch ", "locked", "prunable"];
    let attr = |f: &str| LABELS.contains(&f) || PREFIXES.iter().any(|a| f.starts_with(a));
    let mut out = Vec::new();
    let mut cur: Option<(String, Option<String>, bool)> = None;
    let mut flush = |cur: &mut Option<(String, Option<String>, bool)>| {
        if let Some((p, b, ok)) = cur.take()
            && ok
            && (nul || std::path::Path::new(&p).join(".git").exists())
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
    worktrees(dir)
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
