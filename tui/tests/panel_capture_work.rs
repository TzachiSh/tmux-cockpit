//! #1343: the panel's capture carries the pane's work folder from the shared resolver,
//! on a real tmux, a private `-L` server, never the user's. One test, as it sets
//! COCKPIT_TMUX.
mod fake;

use cockpit_tui::data::workdir::Source;
use cockpit_tui::panel::capture::capture;
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
    fn at(&self, n: &str) -> String {
        self.dir.join(n).display().to_string()
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    fn pane(&self) -> String {
        self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &self.at("pane"), "sleep 300"])
    }
}

/// Rows: an agent pane with a dir set by hand, where the resolver's folder is not
/// pane_current_path; a shell pane the resolver does not count, where pane_current_path
/// is the fallback.
#[test]
fn the_capture_carries_the_resolvers_folder() {
    let sock = format!("cockpit-capwork-{}", std::process::id());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(&sock);
    let _ = std::fs::remove_dir_all(&dir);
    for n in ["pane", "hook", "hand"] {
        std::fs::create_dir_all(dir.join(n)).unwrap();
    }
    let dir = dir.canonicalize().unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut r = Rig {
        server: fake::Server::new(&sock),
        sock,
        dir,
    };
    let first = r.at("pane");
    if !r.server.start(|c| {
        c.args(["new-session", "-d", "-c", &first, "sleep 300"]);
    }) {
        eprintln!("SKIP: no tmux");
        return;
    }
    // SAFETY: the one test of this binary, so no other thread reads the env.
    unsafe { fake::set_var(&fake::env_lock(), "COCKPIT_TMUX", format!("tmux -L {}", r.sock)) };
    let agent = r.pane();
    for (k, v) in [
        ("@panestate", "running".to_string()),
        ("@cockpit_dir", r.at("hand")),
        ("@cockpit_cwd", r.at("hook")),
    ] {
        r.tmux(&["set-option", "-p", "-t", &agent, k, &v]);
    }
    let shell = r.pane();
    let rows = [
        ("an agent's declared dir", &agent, r.at("hand"), Source::Declared),
        ("a shell pane", &shell, r.at("pane"), Source::PaneCwd),
    ];
    for (name, id, want, source) in rows {
        let c = capture(id).unwrap_or_else(|| panic!("{name}: no capture"));
        let got = (c.work.dir.as_str(), c.work.source, c.work.cwd.as_str(), c.path.as_str());
        assert_eq!(got, (want.as_str(), source, r.at("pane").as_str(), r.at("pane").as_str()), "{name}");
    }
}
