//! #1248: on tmux next-3.9 display-popup makes a modal floating pane, so the branch
//! popup's goto (#228) would list cockpit's own popup as a pane of the tree. Tag 3.8 makes
//! the same kind of pane with `new-pane -O`, so this runs there: a modal pane whose
//! command starts with cockpit's mark is no pane to goto, and a user's own modal pane
//! still is. Its own binary: `branch::goto` reads the env.
mod fake;
use cockpit_tui::branch::{self, Origin};
use std::path::{Path, PathBuf};
use std::process::Command;

struct Server {
    sock: String,
    dir: PathBuf,
    /// killed with its socket before the folder goes
    _server: fake::Server,
}

impl Server {
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// A modal pane in `cwd` running `cmd`, over a new window started outside every tree,
    /// marked as the hooks mark an agent's, so it would be goto's first pick: its id.
    fn modal(&self, cmd: &str, cwd: &str) -> String {
        let d = self.dir.display().to_string();
        let id = ["-P", "-F", "#{pane_id}"];
        let w = self.tmux(&[&["new-window", "-d", "-c", &d][..], &id, &["exec sleep 60"]].concat());
        let p = self.tmux(&[&["new-pane", "-O", "-t", &w, "-c", cwd][..], &id, &[cmd]].concat());
        self.tmux(&["set-option", "-p", "-t", &p, "@panestate", "running"]);
        p
    }

    /// The session's windows, one `<index> <pane path>` line each, polled a bounded count
    /// until `n` of them name a path: a new pane's path reads empty until tmux finds it.
    fn windows(&self, n: usize) -> Vec<String> {
        let mut got = Vec::new();
        for _ in 0..200 {
            let out = self.tmux(&["list-windows", "-t", "t:", "-F", "#{window_index} #{pane_current_path}"]);
            got = out.lines().map(String::from).collect();
            if got.len() == n && got.iter().all(|l| l.split_once(' ').is_some_and(|(_, p)| !p.is_empty())) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        got
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self._server.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(args);
    let c = c.env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@e");
    let c = c.env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@e");
    let c = c.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1");
    assert!(c.status().unwrap().success(), "git {args:?}");
}

/// Rows, in order on one server: with only cockpit's popup working in the tree, goto opens
/// a window there; then with a user's modal pane there too, goto selects that pane.
#[test]
fn a_cockpit_popup_is_no_pane_to_goto_and_a_users_modal_pane_is() {
    // first, before the rig's folder: the runner unset the caller's
    // log, agents cache and state folder, which a lookup uses (#1066)
    fake::checked_state();
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("no tmux on PATH -- skipping");
        return;
    }
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cockpit-popup-goto-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // git lists real paths
    let dir = std::fs::canonicalize(&dir).unwrap();
    let sock = format!("cockpit-popup-goto-{pid}");
    let (d, r, w) = (dir.display().to_string(), dir.join("r"), dir.join("w"));
    // built first, so a start that panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir: dir.clone(),
    };
    git(&dir, &["init", "-q", "-b", "main", "r"]);
    git(&r, &["commit", "-q", "--allow-empty", "-m", "i"]);
    git(&r, &["worktree", "add", "-q", "-b", "held", w.to_str().unwrap()]);
    let (r, w) = (r.display().to_string(), w.display().to_string());
    s._server.start(|c| {
        c.args(["new-session", "-d", "-s", "t", "-c", &d, "exec sleep 60"]);
    });
    let popup = s.modal("COCKPIT_POPUP=1 exec sleep 60", &w);
    assert_eq!(s.tmux(&["display-message", "-p", "-t", &popup, "#{pane_modal_flag}"]), "1");
    let path = fake::rig_path(&dir.join("bin"));
    let held = fake::env_lock();
    // SAFETY: the one test of this binary that writes the env. The others read it only
    // in their spawns, which std orders with these writes (fake::run_by_hand).
    unsafe {
        fake::set_var(&held, "PATH", path);
        fake::set_var(&held, "COCKPIT_TMUX", format!("tmux -L {}", s.sock));
        fake::set_var(&held, "HOME", &d);
        fake::remove_var(&held, "CLAUDE_CONFIG_DIR");
        // goto writes, on this rig's own server only
        fake::remove_var(&held, "COCKPIT_READONLY");
    }
    drop(held);
    fake::assert_claude_is_the_rigs(&dir);
    assert_eq!(branch::goto(&r, &w, &Origin::default()), Ok(String::new()), "popup only");
    // windows 0 and 1 are the session's and the popup's host; a bare new-window takes 2
    let win = s.windows(3);
    assert_eq!((win.len(), win.last()), (3, Some(&format!("2 {w}"))), "{win:?}");
    let mine = s.modal("exec sleep 60", &w);
    assert_eq!(branch::goto(&r, &w, &Origin::default()), Ok(String::new()), "with a user's");
    let active = s.tmux(&["display-message", "-p", "-t", "t:", "#{pane_id}"]);
    assert_eq!(
        active, mine,
        "goto selected {active}, not the user's modal pane (the popup is {popup})"
    );
}

/// #1061, #1066: this binary's rig row, started again with TERM and HUP ignored and blocked, as a
/// suite under nohup or with them blocked hands them down: the rig's server, told to
/// go by a TERM it sends itself, is gone afterwards with its panes.
#[test]
#[cfg(target_os = "linux")]
fn the_rig_rows_leave_no_server_handed_a_blocked_term_and_hup() {
    let rows = ["a_cockpit_popup_is_no_pane_to_goto_and_a_users_modal_pane_is"];
    let pid = fake::rows_pass_handed(&rows, &[libc::SIGTERM, libc::SIGHUP]);
    fake::assert_no_server_left(pid);
}
