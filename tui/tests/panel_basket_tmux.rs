//! ^B's `d` and `D` on a real tmux (#365 child 5): each change is written to
//! the pane's basket before the list shows it. A private `-L` server, never the
//! user's; one test, as it sets COCKPIT_TMUX.
mod fake;

use cockpit_tui::basket;
use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::refs::Ref;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::process::Command;

struct Server {
    sock: String,
    dir: std::path::PathBuf,
    /// killed with its socket before the folder goes
    _server: fake::Server,
}

impl Server {
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self._server.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn key(a: &mut App, k: KeyCode, m: KeyModifiers) {
    a.key(KeyEvent::new(k, m));
}

#[test]
fn d_and_shift_d_write_the_basket_before_the_list_shows_it() {
    let sock = format!("cockpit-pbasket-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut s = Server {
        _server: fake::Server::new(&sock),
        sock,
        dir,
    };
    if !s._server.start(|c| {
        c.args(["new-session", "-d", "-c"]).arg(&s.dir).arg("sleep 300");
    }) {
        eprintln!("SKIP: no tmux");
        return;
    }
    // SAFETY: the one test of this binary that writes the env; the others read it only
    // in their spawns, which std orders with this write (fake::rows_pass_handed).
    unsafe { fake::set_var(&fake::env_lock(), "COCKPIT_TMUX", format!("tmux -L {}", s.sock)) };
    let pane = s.tmux(&["display-message", "-p", "#{pane_id}"]);
    let refs: Vec<Ref> = ["a.rs", "b.rs", "c.rs"]
        .iter()
        .map(|f| {
            std::fs::write(s.dir.join(f), "x\n").unwrap();
            Ref::new(s.dir.join(f), None, None).unwrap()
        })
        .collect();
    basket::write(&pane, &refs).unwrap();

    let cap = Capture {
        path: s.dir.display().to_string(),
        ..Default::default()
    };
    let mut a = App::new(&pane, cap);
    a.basket = basket::read(&pane).unwrap();
    a.view = View::Compose;
    key(&mut a, KeyCode::Char('b'), KeyModifiers::CONTROL);
    key(&mut a, KeyCode::Down, KeyModifiers::NONE);
    key(&mut a, KeyCode::Char('d'), KeyModifiers::NONE);
    let left = [refs[0].clone(), refs[2].clone()];
    assert_eq!(basket::read(&pane).unwrap(), left, "b.rs removed on the pane");
    assert_eq!((a.basket.as_slice(), &a.view), (&left[..], &View::Basket { sel: 1 }));
    key(&mut a, KeyCode::Char('D'), KeyModifiers::SHIFT);
    assert!(basket::read(&pane).unwrap().is_empty() && a.basket.is_empty(), "D empties it");
    assert_eq!(s.tmux(&["show-options", "-pqv", "-t", &pane, basket::OPTION]), "", "unset");
}

/// #1061, #1066: this binary's rig row, started again with TERM and HUP ignored and blocked, as a
/// suite under nohup or with them blocked hands them down: the rig's server, told to
/// go by a TERM it sends itself, is gone afterwards with its panes.
#[test]
#[cfg(target_os = "linux")]
fn the_rig_rows_leave_no_server_handed_a_blocked_term_and_hup() {
    let rows = ["d_and_shift_d_write_the_basket_before_the_list_shows_it"];
    let pid = fake::rows_pass_handed(&rows, &[libc::SIGTERM, libc::SIGHUP]);
    fake::assert_no_server_left(pid);
}
