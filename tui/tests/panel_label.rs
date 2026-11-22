//! #1407: the panel's header and confirm name the pane by its label, `session:window ·
//! title`, never its id, on a real tmux, a private `-L` server, never the user's. One test,
//! as it sets COCKPIT_TMUX.
mod fake;

use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::capture::capture;
use cockpit_tui::panel::view::render;
use cockpit_tui::theme::Theme;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
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
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").args(["-L", &self.sock]).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
}

fn frame(a: &App) -> String {
    let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
    t.draw(|f| {
        render(f, f.area(), a, &Theme::fixed(), 0);
    })
    .unwrap();
    t.backend().to_string()
}

/// Rows: a shell the board does not count, titled by `select-pane -T`, opened in the panel:
/// its header and its send question name it `t:1 · fix the tests`, and neither holds its id.
#[test]
fn the_header_and_the_confirm_name_the_pane_by_its_label() {
    fake::fixed_env();
    let sock = format!("cockpit-panellabel-{}", std::process::id());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(&sock);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // built first, so a start that fails or panics still removes the folder
    let mut r = Rig {
        server: fake::Server::new(&sock),
        sock,
        dir: dir.canonicalize().unwrap(),
    };
    let first = r.dir.display().to_string();
    if !r.server.start(|c| {
        c.args(["new-session", "-d", "-s", "t", "-c", &first, "sleep 300"]);
    }) {
        eprintln!("SKIP: no tmux");
        return;
    }
    // SAFETY: the one test of this binary, so no other thread reads the env.
    unsafe { fake::set_var(&fake::env_lock(), "COCKPIT_TMUX", format!("tmux -L {}", r.sock)) };
    let pane = r.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &first, "sleep 300"]);
    r.tmux(&["select-pane", "-t", &pane, "-T", "fix the tests"]);
    let cap = capture(&pane).expect("the live pane read as gone");
    let mut a = App::new(&pane, cap);
    let top = frame(&a).lines().next().unwrap_or_default().to_string();
    assert!(top.contains(" panel · t:1 · fix the tests "), "{top}");
    assert!(!top.contains(&pane), "{top}");
    a.view = View::Confirm {
        text: "ls".into(),
        send: true,
        warn: vec![],
        back: Box::new(View::Compose),
        scroll: 0,
    };
    let f = frame(&a);
    assert!(f.contains("send this to t:1 · fix the tests ("), "{f}");
    assert!(!f.contains(&pane), "{f}");
}
