//! #1352: the capture's `<repo>` block is the work folder's, by the shared resolver
//! (#1343), on a real tmux, a private `-L` server, never the user's. One test, as it
//! sets COCKPIT_TMUX.
mod fake;

use cockpit_tui::panel::capture::capture_within;
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
    fn at(&self, n: &str) -> String {
        self.dir.join(n).display().to_string()
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// A window in `y` running `cmd`, with an agent's options naming `x` when `agent`.
    fn pane(&self, cmd: &str, agent: bool) -> String {
        let id = self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &self.at("y"), cmd]);
        if agent {
            for (k, v) in [("@panestate", "running".to_string()), ("@cockpit_dir", self.at("x"))] {
                self.tmux(&["set-option", "-p", "-t", &id, k, &v]);
            }
        }
        // polled: until its exec, the window's command is the shell that starts it
        let want = cmd.split(' ').next().unwrap();
        for _ in 0..500 {
            if self.tmux(&["display", "-p", "-t", &id, "#{pane_current_command}"]) == want {
                return id;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{id} never ran {want}");
    }

    fn repo(&self, n: &str, branch: &str) {
        let git = |args: &[&str]| {
            let o = Command::new("git")
                .arg("-C")
                .arg(self.dir.join(n))
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        };
        git(&["init", "-q", "-b", branch]);
        git(&["commit", "-q", "--allow-empty", "-m", "first"]);
    }
}

fn line<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    block.lines().find_map(|l| l.strip_prefix(key))
}

/// Rows: an agent pane in repo y whose dir names repo x; a shell pane in y, which the
/// resolver does not count, so pane_current_path is the fallback; an agent pane whose
/// command is not local, which gets no block (#429's rule, kept).
#[test]
fn the_repo_block_is_the_work_folders() {
    let sock = format!("cockpit-caprepo-{}", std::process::id());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(&sock);
    let _ = std::fs::remove_dir_all(&dir);
    for n in ["x", "y"] {
        std::fs::create_dir_all(dir.join(n)).unwrap();
    }
    let dir = dir.canonicalize().unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut r = Rig {
        server: fake::Server::new(&sock),
        sock,
        dir,
    };
    r.repo("x", "lane");
    r.repo("y", "main");
    let first = r.at("y");
    if !r.server.start(|c| {
        c.args(["new-session", "-d", "-c", &first, "sleep 300"]);
    }) {
        eprintln!("SKIP: no tmux");
        return;
    }
    // SAFETY: the one test of this binary, so no other thread reads the env.
    unsafe { fake::set_var(&fake::env_lock(), "COCKPIT_TMUX", format!("tmux -L {}", r.sock)) };
    let (x, y) = (r.at("x"), r.at("y"));
    let rows = [
        ("an agent's dir", r.pane("sh", true), Some((x.as_str(), "lane"))),
        ("a shell pane", r.pane("sh", false), Some((y.as_str(), "main"))),
        ("an agent's non-local command", r.pane("sleep 300", true), None),
    ];
    for (name, id, want) in rows {
        let c = capture_within(&id, Duration::from_secs(10)).unwrap_or_else(|| panic!("{name}: no capture"));
        let block = c.repo.get();
        let got = line(block, "root: ").zip(line(block, "branch: "));
        assert_eq!(got, want, "{name} (cmd {}):\n{block}", c.cmd);
    }
}
