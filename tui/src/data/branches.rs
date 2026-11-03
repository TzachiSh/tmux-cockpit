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

/// Branch name to the other worktree's full path, for every linked
/// worktree that is not `dir`'s own: git refuses to switch to those.
fn elsewhere(dir: &str) -> Vec<(String, String)> {
    let top = git(dir, &[], None, &["rev-parse", "--show-toplevel"]);
    let top = top.trim_end_matches('\n');
    let (mut path, mut out) = (String::new(), Vec::new());
    for l in git(dir, &[], None, &["worktree", "list", "--porcelain"]).lines() {
        if let Some(p) = l.strip_prefix("worktree ") {
            path = p.to_string();
        } else if let Some(b) = l.strip_prefix("branch refs/heads/")
            && path != top
        {
            out.push((b.to_string(), path.clone()));
        }
    }
    out
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
