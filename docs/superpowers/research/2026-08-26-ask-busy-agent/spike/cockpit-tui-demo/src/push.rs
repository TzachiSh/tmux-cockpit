//! Push wakeups, so a hook can say "look now" instead of the screen finding out
//! on its next second. Two channels, both optional, both fail-open: if either
//! is unavailable the 1 s poll still carries the screen.
use crate::data::{log, tmux};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};

static SIGNALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_usr1(_: libc::c_int) {
    SIGNALLED.store(true, Ordering::Relaxed);
}

pub struct Push {
    sock: Option<(UnixListener, String)>,
}

/// sun_path is 108 bytes including the NUL, and a truncated path binds to the
/// wrong name rather than failing.
const SUN_MAX: usize = 100;

pub fn sock_path() -> String {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    format!("{dir}/cockpit-tui-{}.sock", std::process::id())
}

impl Push {
    /// Binds the socket and installs the SIGUSR1 handler, then publishes both
    /// addresses as global tmux options so a hook can find them.
    pub fn start(enable_sock: bool) -> Self {
        let pid = std::process::id().to_string();
        unsafe { libc::signal(libc::SIGUSR1, on_usr1 as *const () as libc::sighandler_t) };
        tmux(&["set-option", "-g", "@cockpit_tui_pid", &pid]);

        let mut sock = None;
        if enable_sock {
            let path = sock_path();
            if path.len() > SUN_MAX {
                log(&format!("sock path too long ({} > {SUN_MAX}), poll only", path.len()));
            } else {
                // a socket left by a killed process is dead but still occupies
                // the name; a failed connect proves it and earns the unlink
                if UnixStream::connect(&path).is_err() {
                    let _ = std::fs::remove_file(&path);
                }
                match UnixListener::bind(&path) {
                    Ok(l) => {
                        let _ = l.set_nonblocking(true);
                        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                        tmux(&["set-option", "-g", "@cockpit_tui_sock", &path]);
                        log(&format!("sock listening {path}"));
                        sock = Some((l, path));
                    }
                    Err(e) => log(&format!("sock bind failed: {e}, poll only")),
                }
            }
        }
        Self { sock }
    }

    /// True when something asked for a refresh. Drains everything pending so a
    /// burst of hook calls costs one refetch, not one each.
    pub fn pending(&self) -> bool {
        let mut hit = SIGNALLED.swap(false, Ordering::Relaxed);
        if hit {
            log("recv usr1");
        }
        let Some((l, _)) = &self.sock else { return hit };
        while let Ok((s, _)) = l.accept() {
            // A line only ever means "refetch". It never names an action, so the
            // worst a writer can do is make the screen re-read tmux.
            for line in BufReader::new(s).lines().map_while(Result::ok).take(8) {
                log(&format!("recv sock {line}"));
                hit = true;
            }
        }
        hit
    }
}

impl Drop for Push {
    fn drop(&mut self) {
        if let Some((_, p)) = &self.sock {
            let _ = std::fs::remove_file(p);
            tmux(&["set-option", "-gu", "@cockpit_tui_sock"]);
        }
        tmux(&["set-option", "-gu", "@cockpit_tui_pid"]);
    }
}

/// `cockpit-tui notify "<line>"`: what agent-state's hook would call. Never
/// blocks the hook — no listener, or a listener that died mid-write, is a
/// silent no-op, because the screen's own poll will catch up anyway.
pub fn notify(line: &str) {
    use std::io::Write;
    let path = tmux(&["show-option", "-gqv", "@cockpit_tui_sock"]).trim().to_string();
    if path.is_empty() {
        return;
    }
    if let Ok(mut s) = UnixStream::connect(&path) {
        let _ = s.set_write_timeout(Some(std::time::Duration::from_millis(50)));
        let _ = writeln!(s, "{line}");
    }
}
