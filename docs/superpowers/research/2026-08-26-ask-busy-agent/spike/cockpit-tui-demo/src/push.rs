//! Push wakeups, so a hook can say "look now" instead of the screen finding out
//! on its next second. Signals only: `kill -USR1 $(tmux show-option -gqv
//! @cockpit_tui_pid)`. SIGWINCH does the same job without any code here --
//! crossterm already delivers it as Event::Resize, which fleet's loop treats as
//! a refetch. Neither carries a payload, and neither needs to: the screen reads
//! the truth from tmux anyway, it only needs to be told when.
use crate::data::tmux;
use std::sync::atomic::{AtomicBool, Ordering};

static SIGNALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_usr1(_: libc::c_int) {
    SIGNALLED.store(true, Ordering::Relaxed);
    // crossterm retries its poll on EINTR, so a bare flag is not noticed until
    // the poll times out -- measured 68 ms median against SIGWINCH's 25 ms.
    // SIGWINCH is the one signal it turns into an event, which ends the wait.
    unsafe { libc::raise(libc::SIGWINCH) };
}

pub struct Push;

/// The C twin's guard (`spike/c-demo/fleet.c:261`): never set-option on a server
/// we only read. Without it a screenshot run against the live socket leaves a
/// dead `@cockpit_tui_pid` behind, which is how competition.md:33 happened.
fn publish(on: bool) {
    if std::env::var_os("COCKPIT_READONLY").is_some() {
        return;
    }
    if on {
        tmux(&["set-option", "-g", "@cockpit_tui_pid", &std::process::id().to_string()]);
    } else {
        tmux(&["set-option", "-gu", "@cockpit_tui_pid"]);
    }
}

impl Push {
    pub fn start() -> Self {
        unsafe { libc::signal(libc::SIGUSR1, on_usr1 as *const () as libc::sighandler_t) };
        publish(true);
        Self
    }

    /// True when something asked for a refresh. A burst collapses to one
    /// refetch, because the flag is a bool and not a queue.
    pub fn pending(&self) -> bool {
        SIGNALLED.swap(false, Ordering::Relaxed)
    }
}

impl Drop for Push {
    fn drop(&mut self) {
        publish(false);
    }
}
