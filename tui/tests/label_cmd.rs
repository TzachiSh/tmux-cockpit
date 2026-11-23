//! #1407: `cockpit-tui label %N` on a real tmux, a private `-L` server, never the user's
//! (AGENTS.md): how a message names a pane, `session:window · title`, for the shell's
//! `label_ask`. Any pane, an agent's or not. A pane that is gone exits 1 with nothing printed,
//! and a usage error exits 2.
mod fake;

use std::path::PathBuf;
use std::process::Command;

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
    /// `None` where tmux is not there to run. Session `t`, its first window `w`.
    fn new(tag: &str) -> Option<Rig> {
        let sock = format!("cockpit-label-{tag}-{}", std::process::id());
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
        let d = r.dir.display().to_string();
        let started = r.server.start(|c| {
            c.args(["new-session", "-d", "-s", "t", "-n", "w", "-c", &d, "sleep 300"]);
        });
        started.then_some(r)
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// A new pane in a window of its own, named `name`: a name given turns the window's
    /// automatic-rename off, so it stays.
    fn pane(&self, name: &str) -> String {
        let d = self.dir.display().to_string();
        self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-n", name, "-c", &d, "sleep 300"])
    }

    /// `cockpit-tui label <args...>`: its exit code, stdout and stderr.
    fn run(&self, args: &[&str]) -> (Option<i32>, String, String) {
        let s = self.dir.display();
        let o = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
            .arg("label")
            .args(args)
            .env("COCKPIT_TMUX", format!("tmux -L {}", self.sock))
            .env("HOME", s.to_string())
            .env("CLAUDE_CONFIG_DIR", format!("{s}/.claude"))
            .env("COCKPIT_READONLY", "1")
            .env("XDG_STATE_HOME", format!("{s}/state"))
            .env_remove("COCKPIT_LOG")
            .env_remove("XDG_CACHE_HOME")
            .output()
            .unwrap();
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        (o.status.code(), text(&o.stdout), text(&o.stderr))
    }
}

#[test]
fn label_prints_session_window_and_title_for_any_pane() {
    let Some(r) = Rig::new("any") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    // a pane running sleep, no agent's: its window name stands in for the title
    let a = r.pane("win");
    let (code, out, err) = r.run(&[&a]);
    assert_eq!((code, out.as_str()), (Some(0), "t:1 · win\n"), "{err}");
    // @cockpit_title comes first, as on the board
    r.tmux(&["set-option", "-p", "-t", &a, "@cockpit_title", "fix the tests"]);
    let (code, out, err) = r.run(&[&a]);
    assert_eq!((code, out.as_str()), (Some(0), "t:1 · fix the tests\n"), "{err}");
    // the first window's pane, in another window: each pane by its own place
    let first = r.tmux(&["display-message", "-p", "-t", "t:0", "#{pane_id}"]);
    let (code, out, err) = r.run(&[&first]);
    assert_eq!((code, out.as_str()), (Some(0), "t:0 · w\n"), "{err}");
}

#[test]
fn label_exits_1_for_no_pane_and_2_for_a_bad_id() {
    let Some(r) = Rig::new("bad") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let (code, out, err) = r.run(&["%99999"]);
    assert_eq!((code, out.as_str()), (Some(1), ""), "a pane that is gone: {err}");
    let bad: [&[&str]; 6] = [&[], &[""], &["x"], &["%"], &["3"], &["%3", "%4"]];
    for args in bad {
        let (code, out, err) = r.run(args);
        assert_eq!((code, out.as_str()), (Some(2), ""), "{args:?}: {err}");
        assert!(err.contains("label needs one pane id like %3"), "{args:?}: {err}");
    }
}
