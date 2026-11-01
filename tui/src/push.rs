//! SIGUSR1 wakeups and the published pid; contract in docs/tui.md, "Contracts".
//! A hook must verify the pid before signalling: SIGUSR1 kills a recycled pid.
use crate::data::{log, readonly, tmux_argv, tmux_set};
use std::ffi::CString;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static SIGNALLED: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(true);

/// The unpublish command, built before any signal can arrive so the handler only
/// forks and execs: `Command::output()` allocates and locks.
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
    // crossterm retries its poll on EINTR, so a bare flag waits out the timeout;
    // SIGWINCH is the one signal it turns into an event.
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

/// Hand the terminal back using only async-signal-safe calls (`write`,
/// `tcsetattr`); ratatui::restore() is not.
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
        // A blank COCKPIT_TMUX publishes nothing, and argv[0] would be "set-option".
        if readonly() || tmux_argv().is_empty() {
            return Self;
        }
        let mut own: Vec<CString> = tmux_argv().iter().filter_map(|s| CString::new(s.as_str()).ok()).collect();
        own.extend(unpublish_args().iter().filter_map(|s| CString::new(s.as_str()).ok()));
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
        let woke = SIGNALLED.swap(false, Ordering::Relaxed);
        // The only place a pushed refresh is distinguishable from the 1 s tick,
        // so log it for whoever debugs the channel.
        if woke {
            log("push wake");
        }
        woke
    }
}

/// The one unpublish, from `q`, the panic hook or the signal arm; first wins.
/// DONE starts true, so under COCKPIT_READONLY this is a no-op.
pub fn cleanup() {
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    let a = unpublish_args();
    tmux_set(&a.iter().map(String::as_str).collect::<Vec<_>>());
}

/// The option is global, so a second board may own it by now: unset it only
/// while it still names us, in one tmux command so nothing lands in between.
pub fn unpublish_args() -> [String; 4] {
    [
        "if-shell".into(),
        "-F".into(),
        format!("#{{==:#{{@cockpit_tui_pid}},{}}}", std::process::id()),
        "set-option -gu @cockpit_tui_pid".into(),
    ]
}

impl Drop for Push {
    fn drop(&mut self) {
        cleanup();
    }
}
