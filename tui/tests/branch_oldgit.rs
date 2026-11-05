//! git before 2.36 rejects `worktree list -z` (#303). A `git` on PATH that does the
//! same stands in; the list must still name the worktree, and a path with a newline,
//! which the plain porcelain cuts in two, must be left out rather than guessed at.
//! Its own file: it sets PATH, and nothing else in this process may race it.
mod fake;

use cockpit_tui::data::branches;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
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

#[test]
fn without_z_the_plain_porcelain_names_the_worktree_and_skips_a_newline_path() {
    let root = std::env::temp_dir().join(format!("cockpit-oldgit-{}", std::process::id()));
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main", "."]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "i"]);
    git(
        &repo,
        &["worktree", "add", "-q", "-b", "plain", root.join("w plain").to_str().unwrap()],
    );
    git(
        &repo,
        &["worktree", "add", "-q", "-b", "odd", root.join("w\nodd").to_str().unwrap()],
    );
    let real = String::from_utf8(Command::new("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap();
    let shim = root.join("shim");
    std::fs::create_dir_all(&shim).unwrap();
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in *\" worktree list \"*\" -z \"*) echo 'error: unknown switch z' >&2; exit 129 ;; esac\nexec {} \"$@\"\n",
        real.trim()
    );
    fake::write_exec(&shim.join("git"), &script);
    let path = format!("{}:{}", shim.display(), std::env::var("PATH").unwrap());
    // SAFETY: this file holds one test, so no other thread reads the environment.
    unsafe { std::env::set_var("PATH", path) };
    let bs = branches::list(repo.to_str().unwrap());
    let held = |n: &str| bs.iter().find(|b| b.name == n).unwrap().elsewhere.clone();
    let want = std::fs::canonicalize(root.join("w plain")).unwrap().to_str().unwrap().to_string();
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(held("plain"), Some(want));
    assert_eq!(held("odd"), None, "a cut path is skipped");
}
