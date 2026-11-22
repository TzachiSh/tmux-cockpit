//! #1408: `cockpit-tui pane <name>` on a real tmux, a private `-L` server, never the user's
//! (AGENTS.md): the one pane whose own `@cockpit_role` is the name. A window's value and a
//! server's are not a pane's own, a value matches only whole, and more than one pane holding
//! the name is an error that names them all.
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
        let sock = format!("cockpit-role-{tag}-{}", std::process::id());
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
            c.args(["new-session", "-d", "-c", &d, "sleep 300"]);
        });
        started.then_some(r)
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// A new pane, in a window of its own.
    fn pane(&self) -> String {
        let d = self.dir.display().to_string();
        self.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, "sleep 300"])
    }

    /// `cockpit-tui pane <args...>` against the server on `sock`: its exit code, stdout and stderr.
    fn run_on(&self, sock: &str, args: &[&str]) -> (Option<i32>, String, String) {
        let s = self.dir.display();
        let o = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
            .arg("pane")
            .args(args)
            .env("COCKPIT_TMUX", format!("tmux -L {sock}"))
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

    fn run(&self, args: &[&str]) -> (Option<i32>, String, String) {
        self.run_on(&self.sock, args)
    }

    /// `cockpit-tui pane <name>`: its exit code and stdout.
    fn code_out(&self, name: &str) -> (Option<i32>, String) {
        let (code, out, _) = self.run(&[name]);
        (code, out)
    }
}

#[test]
fn one_pane_with_the_name_prints_its_id() {
    let Some(r) = Rig::new("one") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let id = r.pane();
    r.tmux(&["set-option", "-p", "-t", &id, "@cockpit_role", "orchestra/worker-67"]);
    assert_eq!(r.code_out("orchestra/worker-67"), (Some(0), format!("{id}\n")));
}

#[test]
fn no_pane_with_the_name_is_gone_exit_1() {
    let Some(r) = Rig::new("gone") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    r.pane();
    let (code, out, err) = r.run(&["orchestra/worker-67"]);
    assert_eq!((code, out.as_str()), (Some(1), ""), "{err}");
    assert!(err.contains("no pane is named orchestra/worker-67"), "{err}");
}

#[test]
fn two_panes_with_the_name_are_an_error_never_the_first() {
    let Some(r) = Rig::new("two") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let (a, b) = (r.pane(), r.pane());
    for p in [&a, &b] {
        r.tmux(&["set-option", "-p", "-t", p, "@cockpit_role", "s1/claude"]);
    }
    let (code, out, err) = r.run(&["s1/claude"]);
    assert_eq!((code, out.as_str()), (Some(3), ""), "{err}");
    assert!(
        err.contains("2 panes are named s1/claude: ") && err.contains(&a) && err.contains(&b),
        "{err}"
    );
}

/// A window's value shows on every pane of it in a format. It is no pane's own, so it names
/// none of them, and the pane beside it that holds its own name is still found.
#[test]
fn a_window_value_names_no_pane() {
    let Some(r) = Rig::new("win") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let a = r.pane();
    let d = r.dir.display().to_string();
    let b = r.tmux(&["split-window", "-d", "-P", "-F", "#{pane_id}", "-t", &a, "-c", &d, "sleep 300"]);
    r.tmux(&["set-option", "-w", "-t", &a, "@cockpit_role", "x"]);
    r.tmux(&["set-option", "-p", "-t", &b, "@cockpit_role", "y"]);
    assert_eq!(r.code_out("x"), (Some(1), String::new()), "the window's value");
    assert_eq!(r.code_out("y"), (Some(0), format!("{b}\n")), "the pane's own, beside it");
}

/// A format reads a server's value ahead of the pane's own. The pane's own still answers.
#[test]
fn a_server_value_hides_no_pane_value() {
    let Some(r) = Rig::new("srv") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let a = r.pane();
    r.tmux(&["set-option", "-s", "@cockpit_role", "y"]);
    r.tmux(&["set-option", "-p", "-t", &a, "@cockpit_role", "x"]);
    assert_eq!(r.code_out("x"), (Some(0), format!("{a}\n")));
}

/// A value over two lines is not either of its lines: each pane's answer is read as one record.
#[test]
fn a_value_over_two_lines_matches_no_line() {
    let Some(r) = Rig::new("lines") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let a = r.pane();
    r.tmux(&["set-option", "-p", "-t", &a, "@cockpit_role", "x\ny"]);
    for n in ["x", "y"] {
        assert_eq!(r.code_out(n), (Some(1), String::new()), "{n}");
    }
}

#[test]
fn the_name_is_one_argument_with_no_newline_or_unit_separator() {
    let Some(r) = Rig::new("usage") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let rows: [&[&str]; 5] = [&[], &[""], &["a", "b"], &["a\nb"], &["a\u{1f}b"]];
    for bad in rows {
        let (code, out, err) = r.run(bad);
        assert_eq!((code, out.as_str()), (Some(2), ""), "{bad:?}: {err}");
        assert!(err.contains("cockpit-tui: pane needs one name"), "{bad:?}: {err}");
    }
}

#[test]
fn with_no_tmux_server_it_says_so_and_exits_1() {
    let Some(r) = Rig::new("none") else {
        eprintln!("SKIP: no tmux");
        return;
    };
    let (code, out, err) = r.run_on(&format!("{}-none", r.sock), &["x"]);
    assert_eq!((code, out.as_str()), (Some(1), ""), "{err}");
    assert!(err.contains("cockpit-tui: tmux is not reachable"), "{err}");
}
