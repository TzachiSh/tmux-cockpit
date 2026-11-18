//! #1248: on tmux next-3.9 display-popup makes a modal floating pane, so the session
//! list's `list-panes -a` counts cockpit's own popup as a window's agent. Tag 3.8 makes
//! the same kind of pane with `new-pane -O`, so this runs there: a modal pane whose command
//! starts with cockpit's mark is no agent, and a user's own modal pane still is. Its own binary:
//! `sessions::fetch` reads the env.
mod fake;
use cockpit_tui::data::sessions;
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

    /// A new window whose pane runs `cat`, and over it a modal pane running `cmd`, which
    /// ends in `sleep`, the agents glob's one name here: the modal pane's id.
    fn modal(&self, cmd: &str) -> String {
        let d = self.dir.display().to_string();
        let w = self.tmux(&["new-window", "-d", "-c", &d, "-P", "-F", "#{pane_id}", "cat"]);
        self.tmux(&["new-pane", "-O", "-t", &w, "-c", &d, "-P", "-F", "#{pane_id}", cmd])
    }

    /// Polls, a bounded count, until every pane runs its own command, not tmux's
    /// shell or the fork before the exec: the glob reads the command name.
    fn settle(&self) {
        for _ in 0..200 {
            let cmds = self.tmux(&["list-panes", "-a", "-F", "#{pane_current_command}"]);
            if cmds.lines().all(|c| c == "cat" || c == "sleep") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("the panes never ran their commands");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self._server.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_cockpit_popup_is_no_agent_of_its_window_and_a_users_modal_pane_is() {
    fake::checked_state();
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("no tmux on PATH -- skipping");
        return;
    }
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("cockpit-popup-sessions-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = format!("cockpit-popup-sessions-{pid}");
    let d = dir.display().to_string();
    // built first, so a start that panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir: dir.clone(),
    };
    s._server.start(|c| {
        c.args(["new-session", "-d", "-s", "t", "-c", &d, "cat"]);
    });
    s.tmux(&["set-option", "-g", "@cockpit_agents_glob", "|sleep|"]);
    let popup = s.modal("COCKPIT_POPUP=1 exec sleep 60");
    s.modal("exec sleep 60");
    s.settle();
    let held = fake::env_lock();
    // SAFETY: the one test of this binary that writes the env. The other reads it only
    // in its spawn, which std orders with these writes (fake::run_by_hand).
    unsafe {
        fake::set_var(&held, "COCKPIT_TMUX", format!("tmux -L {}", s.sock));
        fake::set_var(&held, "HOME", &d);
        fake::set_var(&held, "COCKPIT_READONLY", "1");
    }
    drop(held);
    let got = sessions::fetch().unwrap();
    let agents: Vec<usize> = got.iter().flat_map(|x| x.wins.iter().map(|w| w.agents)).collect();
    assert_eq!(agents, [0, 0, 1], "the popup {popup} counts, or the user's modal pane does not");
}

/// #1061, #1066: this binary's rig row, started again with TERM and HUP ignored and blocked, as a
/// suite under nohup or with them blocked hands them down: the rig's server, told to
/// go by a TERM it sends itself, is gone afterwards with its panes.
#[test]
#[cfg(target_os = "linux")]
fn the_rig_rows_leave_no_server_handed_a_blocked_term_and_hup() {
    let rows = ["a_cockpit_popup_is_no_agent_of_its_window_and_a_users_modal_pane_is"];
    let pid = fake::rows_pass_handed(&rows, &[libc::SIGTERM, libc::SIGHUP]);
    fake::assert_no_server_left(pid);
}
