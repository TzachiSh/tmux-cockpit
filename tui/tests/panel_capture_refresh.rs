//! #1352 (freeze amendment 9): `Capture::refresh` reads the pane again, on a real
//! tmux, a private `-L` server, never the user's. One test, as it sets COCKPIT_TMUX.
mod fake;

use cockpit_tui::panel::capture::{Repo, capture};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

struct Rig {
    sock: String,
    dir: PathBuf,
    /// killed with its socket before the folder goes
    server: fake::Server,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.server.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Rig {
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// Polled until the pane's screen holds `mark`.
    fn shows(&self, id: &str, mark: &str) -> bool {
        (0..500).any(|_| {
            let seen = self.tmux(&["capture-pane", "-p", "-t", id]).contains(mark);
            if !seen {
                std::thread::sleep(Duration::from_millis(10));
            }
            seen
        })
    }
}

/// Rows: a pane whose screen changed since the capture, with name, cmd, path, work
/// and repo set to stand-ins first: the screen, path, work and repo are read again,
/// name and cmd stay. Then a pane that is gone: refresh leaves the capture as it was.
#[test]
fn refresh_reads_the_pane_again_and_keeps_name_and_cmd() {
    let sock = format!("cockpit-caprefresh-{}", std::process::id());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(&sock);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut r = Rig {
        server: fake::Server::new(&sock),
        sock,
        dir,
    };
    let cwd = r.dir.display().to_string();
    if !r.server.start(|c| {
        c.args(["new-session", "-d", "-c", &cwd, "sleep 300"]);
    }) {
        eprintln!("SKIP: no tmux");
        return;
    }
    // SAFETY: the one test of this binary, so no other thread reads the env.
    unsafe { fake::set_var(&fake::env_lock(), "COCKPIT_TMUX", format!("tmux -L {}", r.sock)) };
    let id = r.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &cwd, "sh"]);
    let mut c = capture(&id).expect("first capture");
    // the typed line holds `$((40+2))`; only the output holds the mark
    r.tmux(&["send-keys", "-t", &id, "echo mark-$((40+2))", "Enter"]);
    assert!(r.shows(&id, "mark-42"), "the pane never printed the mark");
    assert!(!c.screen.contains("mark-42"), "the first capture already had it");
    (c.name, c.cmd, c.path) = ("kept-name".into(), "kept-cmd".into(), "/stale".into());
    (c.work.dir, c.repo) = ("/stale".into(), Repo::ready("stale"));

    c.refresh(&id);
    let got = (c.name.as_str(), c.cmd.as_str(), c.path.as_str(), c.work.dir.as_str());
    assert_eq!(got, ("kept-name", "kept-cmd", cwd.as_str(), cwd.as_str()));
    assert!(c.screen.contains("mark-42"), "screen not read again:\n{}", c.screen);
    // the target dir may sit inside a checkout, so only the stand-in's loss is pinned
    assert_ne!(c.repo.get(), "stale", "repo block not read again");

    let before = c.clone();
    r.tmux(&["kill-window", "-t", &id]);
    c.refresh(&id);
    assert_eq!(c, before, "a gone pane changes nothing");
}
