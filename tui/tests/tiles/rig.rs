//! A private tmux server (AGENTS.md: never the user's) holding panes whose
//! screens are fixed, for the tiles' batched capture (#492).
#![allow(dead_code)]
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

pub struct Rig {
    pub sock: String,
    pub dir: PathBuf,
    pub ids: Vec<String>,
}

/// What each pane prints, then it sleeps. `END` marks a screen as drawn.
pub const SCREENS: [&str; 4] = [
    // colour, an OSC 8 link in the ESC \ form tmux emits, a line of `##`
    r"printf '\033[31mred\033[0m plain\n\033]8;;http://x\033\\link\033]8;;\033\\\n##\nEND\n'",
    // one line wider than the pane, which -J joins, then more rows than kept
    r"printf '%0300d\n' 7; seq 1 30; echo END",
    r"printf '\346\227\245\346\234\254 \342\234\246 wide\nEND\n'",
    // a pane that prints nothing but the mark
    "echo END",
];

impl Rig {
    pub fn tmux(&self, args: &[&str]) -> String {
        let mut c = Command::new("tmux");
        let o = c.args(["-L", &self.sock]).args(args).env_remove("TMUX").output();
        String::from_utf8_lossy(&o.map(|o| o.stdout).unwrap_or_default()).into_owned()
    }

    /// `tmux -L <sock>`, the argv the board would run.
    pub fn argv(&self) -> Vec<String> {
        vec!["tmux".into(), "-L".into(), self.sock.clone()]
    }

    /// A tmux stand-in that logs each call to `calls` and runs the real one;
    /// `extra` is shell run after it, with `$mark` set to the batch's mark.
    pub fn counting(&self, extra: &str) -> Vec<String> {
        let sh = self.dir.join("tmux-count");
        let body = format!(
            "#!/bin/sh\necho call >> '{d}/calls'\ntmux -L '{s}' \"$@\"; rc=$?\n\
             for a; do case $a in *'##') mark=${{a%#}};; esac; done\n{extra}\nexit $rc\n",
            d = self.dir.display(),
            s = self.sock
        );
        crate::fake::write_exec(&sh, &body);
        vec![sh.display().to_string()]
    }

    pub fn calls(&self) -> usize {
        std::fs::read_to_string(self.dir.join("calls")).unwrap_or_default().lines().count()
    }
}

/// A server with one window per [`SCREENS`] entry, each drawn; `None` without
/// tmux. Polled, never slept for: the cap only bounds a red run.
pub fn rig(tag: &str) -> Option<Rig> {
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("no tmux on PATH -- skipping");
        return None;
    }
    let sock = format!("cockpit-tiles-{tag}-{}", std::process::id());
    let dir = std::env::temp_dir().join(&sock);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.display().to_string();
    let mut r = Rig { sock, dir, ids: vec![] };
    r.tmux(&[
        "-f",
        "/dev/null",
        "new-session",
        "-d",
        "-c",
        &d,
        "-x",
        "80",
        "-y",
        "20",
        "sleep 300",
    ]);
    for s in SCREENS {
        let cmd = format!("{s}; exec sleep 300");
        let id = r.tmux(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-c", &d, &cmd]);
        r.ids.push(id.trim().to_string());
    }
    let t0 = Instant::now();
    let shows_end = |r: &Rig, id: &str| r.tmux(&["capture-pane", "-p", "-t", id]).contains("END");
    let drawn = |r: &Rig| r.ids.iter().all(|id| shows_end(r, id));
    while !drawn(&r) {
        assert!(t0.elapsed() < Duration::from_secs(30), "the panes never drew");
        std::thread::sleep(Duration::from_millis(20));
    }
    Some(r)
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.tmux(&["kill-server"]);
        let uid = unsafe { libc::getuid() };
        let tmp = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let sock = PathBuf::from(tmp).join(format!("tmux-{uid}")).join(&self.sock);
        let _ = std::fs::remove_file(sock);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
