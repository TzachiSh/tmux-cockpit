//! #1352: the `<repo>` block asks git nothing a git before 2.31 refuses. One test, as it
//! puts a stand-in git first on PATH.
mod fake;

use cockpit_tui::panel::capture::repo_state_within;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// The git a git older than 2.31 is, as far as the block can tell: the real one, but
/// `--path-format` (2.31) is refused with git's usage status. The block of a repo still
/// names its branch and its checkout.
#[test]
fn a_git_without_path_format_still_gives_the_block() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cockpit-oldgit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let (bin, repo) = (d.join("bin"), d.join("repo"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q", "-b", "main"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "first",
    ]);
    let which = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    let real = String::from_utf8_lossy(&which.stdout).trim().to_string();
    assert!(real.starts_with('/'), "no git on PATH: {real:?}");
    let body = format!("#!/bin/sh\nfor a; do case $a in --path-format*) exit 129 ;; esac; done\nexec '{real}' \"$@\"\n");
    fake::write_exec(&bin.join("git"), &body);
    let held = fake::env_lock();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    // SAFETY: the one test of this binary, holding the env lock.
    unsafe { fake::set_var(&held, "PATH", path) };

    let block = repo_state_within(&repo.display().to_string(), "", Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(&d);
    let got: Vec<&str> = block
        .lines()
        .filter(|l| l.starts_with("branch: ") || l.starts_with("checkout: "))
        .collect();
    assert_eq!(got, ["branch: main", "checkout: main"], "{block}");
}
