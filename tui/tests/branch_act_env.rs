//! The popup's own switch (#228) with a git variable inherited. A dotfiles setup can
//! export GIT_DIR into tmux's environment, and the popup inherits it: the switch must
//! land in the repo the picker listed, as `scripts/tmux-branch` made sure through
//! git-env.sh. The one test of this binary writes the environment, under the lock.
mod fake;

use cockpit_tui::{branch, data::branches};
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(args);
    for v in ["GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME"] {
        c.env(v, "t");
    }
    for v in ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"] {
        c.env(v, "t@e");
    }
    assert!(c.status().unwrap().success(), "git {args:?}");
}

/// `pane` and `other` in a fresh `root`, each on main with a branch conf whose `a`
/// differs from main's, and in `pane` a replace ref that swaps conf's `a` for another
/// blob (#730).
fn repos(root: &Path) -> (String, String) {
    for r in ["pane", "other"] {
        let d = root.join(r);
        git(root, &["init", "-q", "-b", "main", d.to_str().unwrap()]);
        std::fs::write(d.join("a"), format!("{r}\n")).unwrap();
        git(&d, &["add", "a"]);
        git(&d, &["commit", "-q", "-m", "main"]);
        git(&d, &["switch", "-q", "-c", "conf"]);
        std::fs::write(d.join("a"), "conf\n").unwrap();
        git(&d, &["commit", "-q", "-am", "conf"]);
        git(&d, &["switch", "-q", "main"]);
    }
    let p = root.join("pane");
    std::fs::write(root.join("swapped"), "swapped\n").unwrap();
    let blob = read(&p, &["hash-object", "-w", root.join("swapped").to_str().unwrap()]);
    let alt = format!("refs/alt/{}", read(&p, &["rev-parse", "conf:a"]));
    git(&p, &["update-ref", &alt, &blob]);
    let s = |d: &Path| d.to_str().unwrap().to_string();
    (s(&p), s(&root.join("other")))
}

fn read(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// What the two repos hold: each one's branch and dirty count, then `a` in the pane.
fn state(pane: &str, other: &str) -> String {
    let dirty = |d: &str| read(Path::new(d), &["status", "--porcelain"]).lines().count();
    let a = std::fs::read_to_string(Path::new(pane).join("a")).unwrap_or_default();
    let (cp, co) = (branches::current(pane), branches::current(other));
    format!("{cp} {} {co} {} {}", dirty(pane), dirty(other), a.trim())
}

#[test]
fn an_inherited_git_dir_or_its_kin_never_switches_another_repo() {
    let held = fake::env_lock();
    for (var, val) in [
        ("GIT_DIR", "@/.git"),
        ("GIT_WORK_TREE", "@"),
        ("GIT_COMMON_DIR", "@/.git"),
        ("GIT_INDEX_FILE", "@/.git/index"),
        ("GIT_OBJECT_DIRECTORY", "@/.git/objects"),
        ("GIT_ATTR_SOURCE", "no-such-rev"),
        ("GIT_REPLACE_REF_BASE", "refs/alt/"),
    ] {
        let tag = format!("cockpit-act-env-{}", std::process::id());
        let root = fake::Scratch::fresh(std::env::temp_dir().join(tag));
        let (pane, other) = repos(&root);
        // SAFETY: the one test of this binary; every write is under the lock it holds
        unsafe { fake::set_var(&held, var, val.replace('@', &other)) };
        let got = branch::act(&pane, b"switch conf");
        unsafe { fake::remove_var(&held, var) };
        assert!(got.is_ok(), "{var}: {got:?}");
        assert_eq!(state(&pane, &other), "conf 0 main 0 conf", "{var}");
    }
}
