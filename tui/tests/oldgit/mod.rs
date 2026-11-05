//! A git before 2.36, which rejects `worktree list -z`: a shim on PATH that does
//! the same. Each test file that uses it holds one test, since it sets PATH.
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
    std::fs::write(dir.join("git"), script).unwrap();
    let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
    std::fs::set_permissions(dir.join("git"), mode).unwrap();
    let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
    // SAFETY: the caller's file holds one test, so no other thread reads the environment.
    unsafe { std::env::set_var("PATH", path) };
}

pub fn held(bs: &[cockpit_tui::data::branches::Branch], name: &str) -> Option<String> {
    bs.iter().find(|b| b.name == name).unwrap().elsewhere.clone()
}
