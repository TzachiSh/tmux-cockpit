//! #1352 (freeze amendment 10): `Repo::try_get` never waits. One test, as it puts a
//! fake git first on PATH.
mod fake;

use cockpit_tui::panel::capture::Repo;
use std::path::PathBuf;
use std::time::Duration;

/// Rows: a block known from the start; a default, with nothing pending; a block
/// whose git waits for a file the test writes, so the worker still runs, and then
/// the same block once that git has ended.
#[test]
fn try_get_answers_without_waiting() {
    assert_eq!(Repo::ready("branch: main").try_get(), Some("branch: main"), "known");
    assert_eq!(Repo::default().try_get(), Some(""), "nothing pending");

    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cockpit-tryget-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let go = d.join("go");
    // bounded by its own count as well as by the block's budget, which kills it
    let body = format!(
        "#!/bin/sh\ni=0\nuntil [ -e '{}' ] || [ $i -gt 3000 ]; do sleep 0.01; i=$((i+1)); done\nexit 1\n",
        go.display()
    );
    fake::write_exec(&d.join("git"), &body);
    let held = fake::env_lock();
    let path = format!("{}:{}", d.display(), std::env::var("PATH").unwrap_or_default());
    // SAFETY: the one test of this binary, holding the env lock.
    unsafe { fake::set_var(&held, "PATH", path) };

    let r = Repo::start(&d.display().to_string(), "", Duration::from_secs(20));
    assert_eq!(r.try_get(), None, "git still running");
    std::fs::write(&go, "").unwrap();
    // polled: the worker ends once its git sees the file and fails
    let mut got = None;
    for _ in 0..2000 {
        got = r.try_get();
        if got.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(got, Some(""), "a git that failed gives the empty block");
}
