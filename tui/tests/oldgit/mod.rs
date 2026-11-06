//! A git before 2.36, which rejects `worktree list -z`: a shim on PATH that does
//! the same. Each test file that uses it holds one test, since it sets PATH, and
//! declares `mod fake;`, which writes the shim.
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// A fresh repo on `main` with one commit, in a canonical root unique to `tag`.
pub fn repo(tag: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("cockpit-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(root.join("repo")).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let repo = root.join("repo");
    git(&repo, &["init", "-q", "-b", "main", "."]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "i"]);
    (root, repo)
}

pub fn add(repo: &Path, branch: &str, path: &Path) {
    git(repo, &["worktree", "add", "-q", "-b", branch, path.to_str().unwrap()]);
}

/// Puts the shim first on PATH. The caller's file must hold no other test.
#[allow(dead_code)] // the files that run on the installed git, which takes -z, use no shim
pub fn shim(root: &Path) {
    let out = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    let real = String::from_utf8(out.stdout).unwrap();
    let dir = root.join("shim");
    std::fs::create_dir_all(&dir).unwrap();
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in *\" worktree list \"*\" -z \"*) exit 129 ;; esac\n\
         exec {} \"$@\"\n",
        real.trim()
    );
    crate::fake::write_exec(&dir.join("git"), &script);
    let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
    // SAFETY: the caller's file holds one test, so no other thread reads the environment.
    unsafe { std::env::set_var("PATH", path) };
}

/// [`shim`], and `rev-parse --git-common-dir` answers as git 2.7 to 2.12 did: a
/// relative common dir comes back as the subdirectory prefix plus the path from the
/// top, so from `repo/sub` it says `sub/.git`. Callers end their args with it.
#[allow(dead_code)] // one test file uses it; the others share this module
pub fn shim_212(root: &Path) {
    shim(root);
    let dir = root.join("shim");
    let real = std::fs::read_to_string(dir.join("git")).unwrap();
    let last = real.lines().last().unwrap();
    let real = last.trim_start_matches("exec ").trim_end_matches(" \"$@\"");
    let script = format!(
        "#!/usr/bin/env bash\ncase \" $* \" in *\" worktree list \"*\" -z \"*) exit 129 ;; esac\n\
         case \" $* \" in *\" rev-parse --git-common-dir \") ;; *) exec {real} \"$@\" ;; esac\n\
         pre=(\"${{@:1:$#-2}}\")\n\
         top=$({real} \"${{pre[@]}}\" rev-parse --show-toplevel) || exit\n\
         px=$({real} \"${{pre[@]}}\" rev-parse --show-prefix)\n\
         c=$({real} \"${{pre[@]}}\" -C \"$top\" rev-parse --git-common-dir) || exit\n\
         case $c in /*) echo \"$c\" ;; *) echo \"$px$c\" ;; esac\n"
    );
    crate::fake::write_exec(&dir.join("git"), &script);
}

/// A git whose `worktree list -z` prints git's own list with `first`'s record moved to
/// the front; every other call is the installed git's. Puts it first on PATH, as [`shim`].
#[allow(dead_code)] // one test file uses it; the others share this module
pub fn shim_first(root: &Path, repo: &Path, first: &Path) {
    let args = ["-C", repo.to_str().unwrap(), "worktree", "list", "--porcelain", "-z"];
    let z = String::from_utf8(Command::new("git").args(args).output().unwrap().stdout).unwrap();
    let head = format!("worktree {}\0", first.display());
    let mut recs: Vec<&str> = z.split("\0\0").filter(|r| !r.is_empty()).collect();
    recs.sort_by_key(|r| !r.starts_with(&head));
    assert!(recs[0].starts_with(&head), "{} is listed: {recs:?}", first.display());
    let list = root.join("list.z");
    std::fs::write(&list, recs.join("\0\0") + "\0\0").unwrap();
    shim(root);
    let git = root.join("shim").join("git");
    let real = std::fs::read_to_string(&git).unwrap();
    let real = real.lines().last().unwrap();
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in *\" worktree list \"*\" -z \"*) exec cat '{}' ;; esac\n\
         {real}\n",
        list.display()
    );
    crate::fake::write_exec(&git, &script);
}

pub fn held(bs: &[cockpit_tui::data::branches::Branch], name: &str) -> Option<String> {
    bs.iter().find(|b| b.name == name).unwrap().elsewhere.clone()
}
