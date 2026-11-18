//! #1248: on tmux next-3.9 display-popup makes a modal floating pane, so the board's
//! `list-panes -a` lists cockpit's own popup. Tag 3.8 makes the same kind of pane with
//! `new-pane -O`, so this runs there: a modal pane whose command starts with cockpit's
//! mark is no row, and a user's own modal pane still is. Its own binary: `list_panes`
//! reads the env.
mod fake;
use cockpit_tui::data::list_panes;
use std::path::PathBuf;
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

    /// A modal pane running `cmd` in a new window, marked as the hooks would mark an
    /// agent's: its id.
    fn modal(&self, cmd: &str, sess: &str) -> String {
        let d = self.dir.display().to_string();
        let id = ["-P", "-F", "#{pane_id}"];
        let w = self.tmux(&[&["new-window", "-d", "-c", &d][..], &id, &["exec sleep 60"]].concat());
        let modal = ["new-pane", "-O", "-t", &w, "-c", &d];
        let p = self.tmux(&[&modal[..], &id, &[cmd]].concat());
        for (k, v) in [("@panestate", "running"), ("@cockpit_session", sess)] {
            self.tmux(&["set-option", "-p", "-t", &p, k, v]);
        }
        p
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self._server.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_cockpit_popup_is_no_board_row_and_a_users_modal_pane_is() {
    // first, before the rig's folder: the runner unset the caller's
    // log, agents cache and state folder, which a lookup uses (#1066)
    fake::checked_state();
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("no tmux on PATH -- skipping");
        return;
    }
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cockpit-popup-board-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = format!("cockpit-popup-board-{pid}");
    let d = dir.display().to_string();
    // built first, so a start that panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir: dir.clone(),
    };
    s._server.start(|c| {
        c.args(["new-session", "-d", "-s", "t", "-c", &d, "exec sleep 60"]);
    });
    let popup = s.modal("COCKPIT_POPUP=1 exec sleep 60", "s1");
    let mine = s.modal("exec sleep 60", "s2");
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
        fake::set_var(&held, "COCKPIT_READONLY", "1");
    }
    drop(held);
    fake::assert_claude_is_the_rigs(&dir);
    let ids: Vec<String> = list_panes().unwrap().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, [mine], "the popup {popup} is listed, or the user's modal pane is not");
}

/// The board's filter is lib/fmt.sh's, character for character, so the shell scans and the
/// board skip the same panes (#1248).
#[test]
fn the_boards_popup_filter_is_the_shells() {
    let fmt = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/lib/fmt.sh");
    let read = ". \"$1\" && printf %s \"$COCKPIT_NOT_POPUP\"";
    let out = Command::new("bash").args(["-c", read, "_", fmt]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), cockpit_tui::data::NOT_POPUP);
}

/// #1061, #1066: this binary's rig row, started again with TERM and HUP ignored and blocked, as a
/// suite under nohup or with them blocked hands them down: the rig's server, told to
/// go by a TERM it sends itself, is gone afterwards with its panes.
#[test]
#[cfg(target_os = "linux")]
fn the_rig_rows_leave_no_server_handed_a_blocked_term_and_hup() {
    let rows = ["a_cockpit_popup_is_no_board_row_and_a_users_modal_pane_is"];
    let pid = fake::rows_pass_handed(&rows, &[libc::SIGTERM, libc::SIGHUP]);
    fake::assert_no_server_left(pid);
}
