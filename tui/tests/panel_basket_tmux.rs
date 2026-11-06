//! ^B's `d` and `D` on a real tmux (#365 child 5): each change is written to
//! the pane's basket before the list shows it. A private `-L` server, never the
//! user's; one test, as it sets COCKPIT_TMUX.
use cockpit_tui::basket;
use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::refs::Ref;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::process::Command;

struct Server {
    sock: String,
    dir: std::path::PathBuf,
    path: String,
}

impl Server {
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = Command::new("tmux").args(["-L", &self.sock, "kill-server"]).status();
        if self.path.ends_with(&self.sock) {
            let _ = std::fs::remove_file(&self.path);
        }
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
    let mut new = Command::new("tmux");
    new.args(["-L", &sock, "-f", "/dev/null", "new-session", "-d", "-c"])
        .arg(&dir)
        .arg("sleep 300");
    if !new.status().is_ok_and(|s| s.success()) {
        eprintln!("SKIP: no tmux");
        return;
    }
    let mut s = Server {
        sock,
        dir,
        path: String::new(),
    };
    s.path = s.tmux(&["display-message", "-p", "#{socket_path}"]);
    // SAFETY: the only test in this binary; nothing else reads the env meanwhile.
    unsafe { std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", s.sock)) };
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
