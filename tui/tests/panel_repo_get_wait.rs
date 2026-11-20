//! #1352 (freeze amendment 10): a `try_get` while another thread waits in `get()` never
//! settles the block before that wait ends. One test, as it puts a stand-in git first on
//! PATH.
mod fake;

use cockpit_tui::panel::capture::Repo;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// A stand-in git that waits for a file, then runs the real one on a committed repo.
/// A thread waits in `get()`. `try_get` meanwhile answers None, and once git runs, both
/// give the same block.
#[test]
fn try_get_during_a_get_waits_for_the_same_block() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cockpit-getwait-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let (bin, repo, go) = (d.join("bin"), d.join("repo"), d.join("go"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    for args in [&["init", "-q", "-b", "main"][..], &["commit", "-q", "--allow-empty", "-m", "first"]] {
        let o = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    }
    let which = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    let real = String::from_utf8_lossy(&which.stdout).trim().to_string();
    assert!(real.starts_with('/'), "no git on PATH: {real:?}");
    // bounded by its own count as well as by the block's budget, which kills it
    let body = format!(
        "#!/bin/sh\ni=0\nuntil [ -e '{}' ] || [ $i -gt 3000 ]; do sleep 0.01; i=$((i+1)); done\n\
         exec '{real}' \"$@\"\n",
        go.display()
    );
    fake::write_exec(&bin.join("git"), &body);
    let held = fake::env_lock();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    // SAFETY: the one test of this binary, holding the env lock.
    unsafe { fake::set_var(&held, "PATH", path) };

    let r = Repo::start(&repo.display().to_string(), "", Duration::from_secs(20));
    let waiter = r.clone();
    let get = std::thread::spawn(move || waiter.get().to_string());
    // A short sleep lets the thread reach get()'s wait. Should it not have, try_get sees
    // the worker still running and answers None too: the row cannot go red for it.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(r.try_get(), None, "try_get settled during get()'s wait");
    std::fs::write(&go, "").unwrap();
    let got = get.join().unwrap();
    let _ = std::fs::remove_dir_all(&d);
    assert!(got.starts_with("branch: main\n"), "{got}");
    assert_eq!(r.try_get(), Some(got.as_str()));
}
