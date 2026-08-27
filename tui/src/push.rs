//! Push wakeups and the pid that makes them addressable. A hook says "look
//! now" with `kill -USR1 $(tmux show-option -gqv @cockpit_tui_pid)` instead of
//! the screen finding out on its next second. Signals only: neither USR1 nor
//! WINCH carries a payload, and neither needs to -- the screen reads the truth
//! from tmux anyway, it only needs to be told when.
//!
//! The hook must confirm the pid is ours before signalling
//! (`[ "$(cat /proc/$pid/comm)" = cockpit-tui ]`): a published pid outlives
//! SIGKILL, and SIGUSR1's default action on a recycled pid is *terminate*.
use crate::data::{readonly, tmux_argv, tmux_set};
use std::ffi::CString;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static SIGNALLED: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(true);

/// The unpublish command, resolved and NUL-terminated before any signal can
/// arrive, so the handler only forks and execs. `Command::output()` allocates
/// and locks, which is why `pkill` used to leave a dead pid behind on a wedged
/// loop.
struct Argv {
    ptrs: Vec<*const libc::c_char>,
    _own: Vec<CString>,
}
// Read-only after OnceLock::set, and the handler only reads it.
unsafe impl Sync for Argv {}
unsafe impl Send for Argv {}

static UNPUBLISH: OnceLock<Argv> = OnceLock::new();
/// The terminal as it was before ratatui took it, so the signal arm can hand it
/// back with tcsetattr -- one of the few calls that is async-signal-safe.
static COOKED: OnceLock<libc::termios> = OnceLock::new();

extern "C" fn on_usr1(_: libc::c_int) {
    SIGNALLED.store(true, Ordering::Relaxed);
    // crossterm retries its poll on EINTR, so a bare flag is not noticed until
    // the poll times out -- measured 68 ms median against SIGWINCH's 25 ms.
    // SIGWINCH is the one signal it turns into an event, which ends the wait.
    unsafe { libc::raise(libc::SIGWINCH) };
}

extern "C" fn on_exit_signal(_: libc::c_int) {
    if !DONE.swap(true, Ordering::SeqCst) {
        unsafe { unpublish_now() };
    }
    unsafe { restore_now() };
    // 0, not 128+sig: a torn-down popup wrote no action, which is exactly what
    // exit 0 with an empty $COCKPIT_ACTION means to the wrapper.
    unsafe { libc::_exit(0) };
}

/// fork + execvp with no allocation anywhere, because a signal can land inside
/// malloc. Reaped so the popup's shell never sees a zombie.
unsafe fn unpublish_now() {
    let Some(a) = UNPUBLISH.get() else { return };
    unsafe {
        match libc::fork() {
            0 => {
                libc::execvp(a.ptrs[0], a.ptrs.as_ptr());
                libc::_exit(127);
            }
            pid if pid > 0 => {
                let mut st: libc::c_int = 0;
                libc::waitpid(pid, &mut st, 0);
            }
            _ => {}
        }
    }
}

/// Leave the alternate screen and show the cursor, then hand the terminal back
/// in the mode it was found in. `write` and `tcsetattr` are async-signal-safe;
/// ratatui::restore() is not.
unsafe fn restore_now() {
    const OUT: &[u8] = b"\x1b[?1049l\x1b[?25h";
    unsafe {
        libc::write(libc::STDOUT_FILENO, OUT.as_ptr().cast(), OUT.len());
        if let Some(t) = COOKED.get() {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, t);
        }
    }
}

pub struct Push;

impl Push {
    /// Call before `ratatui::init()`: the saved termios must be the cooked one,
    /// and init's panic hook then wraps this one rather than replacing it.
    pub fn start() -> Self {
        unsafe {
            libc::signal(libc::SIGUSR1, on_usr1 as *const () as libc::sighandler_t);
            libc::signal(libc::SIGHUP, on_exit_signal as *const () as libc::sighandler_t);
            libc::signal(libc::SIGTERM, on_exit_signal as *const () as libc::sighandler_t);
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut t) == 0 {
                let _ = COOKED.set(t);
            }
        }
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |i| {
            cleanup();
            prev(i);
        }));
        if readonly() {
            return Self;
        }
        let mut own: Vec<CString> = tmux_argv().iter().filter_map(|s| CString::new(s.as_str()).ok()).collect();
        own.extend(
            ["set-option", "-gu", "@cockpit_tui_pid"]
                .iter()
                .filter_map(|s| CString::new(*s).ok()),
        );
        let mut ptrs: Vec<*const libc::c_char> = own.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        let _ = UNPUBLISH.set(Argv { ptrs, _own: own });
        DONE.store(false, Ordering::SeqCst);
        tmux_set(&["set-option", "-g", "@cockpit_tui_pid", &std::process::id().to_string()]);
        Self
    }

    /// True when something asked for a refresh. A burst collapses to one
    /// refetch, because the flag is a bool and not a queue.
    pub fn pending(&self) -> bool {
        SIGNALLED.swap(false, Ordering::Relaxed)
    }
}

/// The one unpublish, reached from `q`, from the panic hook and from the signal
/// arm; whichever gets there first wins. COCKPIT_READONLY never published a pid,
/// so DONE starts true and this is a no-op.
pub fn cleanup() {
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    tmux_set(&["set-option", "-gu", "@cockpit_tui_pid"]);
}

impl Drop for Push {
    fn drop(&mut self) {
        cleanup();
    }
}
