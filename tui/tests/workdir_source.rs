//! #1343: `cockpit-tui workdir [--source] %N` on a real tmux, a private `-L` server, never the
//! user's (AGENTS.md): the board's format reads `@cockpit_dir` and `@cockpit_cwd` off the pane,
//! and `--source` names which candidate answered.
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
    /// `None` where tmux is not there to run.
    fn new(tag: &str) -> Option<Rig> {
        let sock = format!("cockpit-wdsrc-{tag}-{}", std::process::id());
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
        let pane = r.at("pane");
        let started = r.server.start(|c| {
            c.args(["new-session", "-d", "-c", &pane, "sleep 300"]);
        });
        started.then_some(r)
    }

    fn at(&self, n: &str) -> String {
        self.dir.join(n).display().to_string()
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// A new pane in `pane`, marked as an agent's by the hooks, with `opts` set on it.
    fn agent(&self, opts: &[(&str, &str)]) -> String {
        let id = self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &self.at("pane"), "sleep 300"]);
        self.tmux(&["set-option", "-p", "-t", &id, "@panestate", "running"]);
        for (k, v) in opts {
            self.tmux(&["set-option", "-p", "-t", &id, k, v]);
        }
        id
    }

    fn run(&self, args: &[&str]) -> (Option<i32>, String) {
        let s = self.dir.display();
        let o = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
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
        (o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned())
    }
}

#[test]
fn source_names_the_candidate_that_answered() {
    let Some(r) = Rig::new("src") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let (pane, hook, hand) = (r.at("pane"), r.at("hook"), r.at("hand"));
    // rows: name, the options on the pane, then what --source prints
    let rows = [
        (
            "a dir set by hand",
            vec![("@cockpit_dir", hand.as_str()), ("@cockpit_cwd", hook.as_str())],
            format!("{hand}\tdeclared\n"),
        ),
        (
            "a dir the hooks wrote",
            vec![("@cockpit_dir", hook.as_str()), ("@cockpit_cwd", hook.as_str())],
            format!("{hook}\tsession\n"),
        ),
        ("no option at all", vec![], format!("{pane}\tpane-cwd\n")),
    ];
    for (name, opts, want) in rows {
        let id = r.agent(&opts);
        assert_eq!(r.run(&["workdir", "--source", &id]), (Some(0), want), "{name}");
    }
}

#[test]
fn without_the_flag_workdir_prints_the_folder_alone() {
    let Some(r) = Rig::new("plain") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let id = r.agent(&[("@cockpit_dir", &r.at("hand")), ("@cockpit_cwd", &r.at("hook"))]);
    assert_eq!(r.run(&["workdir", &id]), (Some(0), format!("{}\n", r.at("hand"))));
}

/// A hand-set dir since removed: the hooks' cwd answers before the pane's launch dir.
#[test]
fn a_dangling_declared_dir_falls_to_the_hooks_cwd() {
    let Some(r) = Rig::new("dangle") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let gone = r.at("gone");
    let id = r.agent(&[("@cockpit_dir", &gone), ("@cockpit_cwd", &r.at("hook"))]);
    assert_eq!(r.run(&["workdir", &id]), (Some(0), format!("{}\n", r.at("hook"))));
}

#[test]
fn source_with_no_pane_or_a_bad_id_exits_2() {
    let Some(r) = Rig::new("usage") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    for bad in [
        &["workdir", "--source"][..],
        &["workdir", "--source", "6"],
        &["workdir", "%1", "--source"],
    ] {
        assert_eq!(r.run(bad), (Some(2), String::new()), "{bad:?}");
    }
}
