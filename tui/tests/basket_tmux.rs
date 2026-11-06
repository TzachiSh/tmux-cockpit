//! The basket on a real tmux, a private `-L` server, never the user's. One
//! test in this binary: it points `COCKPIT_TMUX` at that server for the process.
use cockpit_tui::basket;
use cockpit_tui::refs::Ref;
use std::process::Command;

struct Server {
    sock: String,
    dir: std::path::PathBuf,
    /// the socket file, which kill-server leaves behind
    path: String,
}

impl Server {
    fn start() -> Option<Server> {
        let sock = format!("cockpit-basket-{}", std::process::id());
        let dir = std::env::temp_dir().join(&sock);
        std::fs::create_dir_all(&dir).ok()?;
        let dir = dir.canonicalize().ok()?;
        let ok = Command::new("tmux")
            .args(["-L", &sock, "-f", "/dev/null", "new-session", "-d", "-c"])
            .arg(&dir)
            .arg("sleep 300")
            .status()
            .ok()?
            .success();
        let mut s = ok.then_some(Server {
            sock,
            dir,
            path: String::new(),
        })?;
        s.path = s.tmux(&["display-message", "-p", "#{socket_path}"]);
        Some(s)
    }

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

#[test]
fn the_basket_round_trips_through_a_pane_option() {
    let Some(s) = Server::start() else {
        eprintln!("SKIP: no tmux");
        return;
    };
    // SAFETY: the only test in this binary; nothing else reads the env meanwhile.
    unsafe { std::env::set_var("COCKPIT_TMUX", format!("tmux -L {}", s.sock)) };
    let pane = s.tmux(&["display-message", "-p", "#{pane_id}"]);
    let f = s.dir.join("a.rs");
    std::fs::write(&f, "x\n").unwrap();
    let one = Ref::new(&f, Some((1, 1)), None).unwrap();

    assert_eq!(basket::add(&pane, one.clone()), Ok((1, true)));
    assert_eq!(basket::add(&pane, one.clone()), Ok((1, false)), "the same ref once");
    assert_eq!(basket::add(&pane, Ref::new(&f, None, None).unwrap()), Ok((2, true)));
    assert_eq!(basket::read(&pane).unwrap().len(), 2);
    assert_eq!(basket::shown(Some(&pane), &one), "a.rs:1", "from the pane's folder");

    // the readonly guard: nothing is written, and the add says so
    unsafe { std::env::set_var("COCKPIT_READONLY", "1") };
    let why = basket::write(&pane, &[]).unwrap_err();
    unsafe { std::env::remove_var("COCKPIT_READONLY") };
    assert!(why.contains("COCKPIT_READONLY"), "{why}");
    assert_eq!(basket::read(&pane).unwrap().len(), 2, "unchanged under readonly");

    // review-534: tmux strips a trailing `;` off the last argument; the value
    // carries a closing newline, so a file named `x;` is kept as itself
    let semi = s.dir.join("x;");
    std::fs::write(&semi, "x\n").unwrap();
    let semi = Ref::new(&semi, None, None).unwrap();
    assert_eq!(basket::add(&pane, semi.clone()), Ok((3, true)));
    assert!(basket::read(&pane).unwrap().contains(&semi), "x; read back as itself");

    // review-534: a gone pane is said, not read as an empty basket
    let why = basket::read("%999").unwrap_err();
    assert!(why.contains("gone"), "{why}");

    // the viewer's run loop: `a` reaches this pane's basket, `y` sends the ref
    {
        use cockpit_tui::mdview::{self, Out, Screen, render::Opts};
        let g = s.dir.join("g.rs");
        std::fs::write(&g, "one\ntwo\n").unwrap();
        let mut v = Screen::new("g.rs", Ok("one\ntwo\n".into()), Opts::default());
        v.can_add = true;
        // review-534: a copier that records, never OSC 52 on the test's own stdout,
        // which reaches the tmux the suite runs under
        let sent = std::cell::RefCell::new(vec![]);
        let copy = |t: &str| -> std::io::Result<()> {
            sent.borrow_mut().push(t.to_string());
            Ok(())
        };
        assert_eq!(
            mdview::act(&mut v, Out::Add(Some((2, 2))), &g, Some(&pane), &copy),
            None,
            "it stays"
        );
        let note = v.note.clone().unwrap_or_default();
        assert_eq!(note, "added g.rs:2 (4 in the basket)", "{note}");
        let two = Ref::new(&g, Some((2, 2)), None).unwrap();
        assert!(basket::read(&pane).unwrap().contains(&two));
        assert_eq!(mdview::act(&mut v, Out::Copy(None), &g, Some(&pane), &copy), None);
        assert_eq!(*sent.borrow(), ["g.rs"], "y hands the ref to the copier");
        let note = v.note.clone().unwrap_or_default();
        assert!(note.starts_with("sent g.rs") && note.contains("set-clipboard on"), "{note}");
        assert_eq!(mdview::act(&mut v, Out::Close, &g, Some(&pane), &copy), Some(Out::Close));
    }

    basket::write(&pane, &[]).unwrap();
    assert_eq!(
        s.tmux(&["show-options", "-pqv", "-t", &pane, basket::OPTION]),
        "",
        "unset when empty"
    );
}
