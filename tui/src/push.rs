//! Push wakeups over `tmux wait-for`; contract in docs/tui.md, "Push wakeups".
//!
//! Each board waits on a channel of its own, `cockpit-board-<pid>`, and lists
//! it in `@cockpit_board_wake` as the command that wakes it. A hook runs that
//! option with `run-shell -C`, inside the tmux call it already makes. No pid is
//! ever signalled: a stale entry costs one `wait-for -S` on a channel nobody
//! waits on, and the next board to start drops it.
use crate::data::{log, readonly, tmux, tmux_argv, tmux_set};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

/// The option a hook runs to wake every board.
pub const OPTION: &str = "@cockpit_board_wake";

static SIGNALLED: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(true);
/// Set by `cleanup()` before it wakes the waiter itself, so the wake ends the
/// thread instead of re-arming it.
static STOPPING: AtomicBool = AtomicBool::new(false);
/// The `tmux wait-for` child: its pid while it runs, 0 between waits, -1 once
/// the board is going. Cleared before the child is reaped, so a pid read from
/// here names our own unreaped child and never a recycled one.
static WAITER: AtomicI32 = AtomicI32::new(0);
/// This board's channel, set once it is listed in the option.
static CHAN: OnceLock<String> = OnceLock::new();
/// The terminal as it was before ratatui took it, so the signal arm can hand it
/// back with tcsetattr -- one of the few calls that is async-signal-safe.
static COOKED: OnceLock<libc::termios> = OnceLock::new();

pub fn channel(pid: u32) -> String {
    format!("cockpit-board-{pid}")
}

/// One entry of the option. The trailing ` ; ` keeps the list a command list.
pub fn entry(chan: &str) -> String {
    format!("wait-for -S {chan} ; ")
}

/// A `set-option -gF` value that drops `chan`'s entry and keeps the rest. A
/// format modifier ends at `;`, so `.` stands in for it in the pattern.
pub fn unlist(chan: &str) -> String {
    format!("#{{s/wait-for -S {chan} . //:{OPTION}}}")
}

extern "C" fn on_exit_signal(_: libc::c_int) {
    stop_waiter();
    unsafe { restore_now() };
    // 0, not 128+sig: a torn-down popup wrote no action, which is exactly what
    // exit 0 with an empty $COCKPIT_ACTION means to the wrapper.
    unsafe { libc::_exit(0) };
}

/// Kills the waiter and keeps the thread from starting another. A swap and a
/// kill, so the signal arm may call it; the entry the signal arm leaves listed
/// is dropped by the next board's start.
fn stop_waiter() {
    let p = WAITER.swap(-1, Ordering::SeqCst);
    if p > 0 {
        unsafe { libc::kill(p, libc::SIGKILL) };
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

/// Whether `pid` is a running cockpit-tui, read from its name and never by
/// signalling it. A listed pid that is not one is a board that died unlisted.
fn is_board(pid: u32) -> bool {
    let comm = match std::fs::read_to_string(format!("/proc/{pid}/comm")) {
        Ok(c) => c,
        Err(_) if std::path::Path::new("/proc/self").is_dir() => return false,
        // macOS: no /proc, and ps prints a path
        Err(_) => Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default(),
    };
    comm.trim().rsplit('/').next() == Some("cockpit-tui")
}

/// The tmux argv that lists `chan`, after dropping the entries in `listed` of
/// boards that are gone. Each dropped channel gets one `-S`, which releases a
/// waiter that a SIGKILLed board left behind where no PDEATHSIG took it along.
pub fn register_args(listed: &str, chan: &str) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    for e in listed.split(" ; ") {
        let Some(old) = e.trim().strip_prefix("wait-for -S ") else {
            continue;
        };
        let Some(pid) = old.strip_prefix("cockpit-board-").and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if old == chan || is_board(pid) {
            continue;
        }
        a.extend(["set-option".into(), "-gF".into(), OPTION.into(), unlist(old), ";".into()]);
        a.extend(["wait-for".into(), "-S".into(), old.into(), ";".into()]);
    }
    a.extend(["set-option".into(), "-ga".into(), OPTION.into(), entry(chan)]);
    a
}

/// Blocks in `tmux wait-for` until a hook signals the channel, sets the flag,
/// and waits again. Returns once `stop_waiter()` has run.
fn wait_loop(argv: Vec<String>, chan: String) {
    let me = std::process::id();
    loop {
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .args(["wait-for", &chan])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: prctl, getppid and _exit are async-signal-safe. A SIGKILLed
            // board takes its waiter along; checked after the prctl, because the
            // board may be gone already by the time the child runs it.
            unsafe {
                cmd.pre_exec(move || {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                    if libc::getppid() as u32 != me {
                        libc::_exit(0);
                    }
                    Ok(())
                });
            }
        }
        let Ok(mut child) = cmd.spawn() else {
            return log("push: cannot run tmux wait-for; the 1 s tick only");
        };
        let pid = child.id() as i32;
        if WAITER.compare_exchange(0, pid, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        // Wait for the exit without reaping, so WAITER is cleared while the pid
        // is still ours: stop_waiter() racing this kills a zombie at worst.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        while unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT) } != 0 {
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                break;
            }
        }
        let stopped = WAITER.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst).is_err();
        let ok = child.wait().is_ok_and(|s| s.success());
        if stopped || STOPPING.load(Ordering::SeqCst) {
            return;
        }
        if ok {
            SIGNALLED.store(true, Ordering::Relaxed);
            // crossterm retries its poll on EINTR, so a bare flag waits out the
            // timeout; SIGWINCH is the one signal it turns into an event.
            unsafe { libc::raise(libc::SIGWINCH) };
        } else {
            // the server is gone or refused the wait: no tight respawn loop
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

pub struct Push;

impl Push {
    /// Call before `ratatui::init()`: the saved termios must be the cooked one,
    /// and init's panic hook then wraps this one rather than replacing it.
    pub fn start() -> Self {
        unsafe {
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
        // A blank COCKPIT_TMUX lists nothing, and argv[0] would be "wait-for".
        let argv = tmux_argv();
        if readonly() || argv.is_empty() {
            return Self;
        }
        let chan = channel(std::process::id());
        let listed = tmux(&["show-option", "-gqv", OPTION]).unwrap_or_default();
        let a = register_args(listed.trim_end_matches('\n'), &chan);
        tmux_set(&a.iter().map(String::as_str).collect::<Vec<_>>());
        let _ = CHAN.set(chan.clone());
        DONE.store(false, Ordering::SeqCst);
        STOPPING.store(false, Ordering::SeqCst);
        WAITER.store(0, Ordering::SeqCst);
        std::thread::spawn(move || wait_loop(argv, chan));
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

/// The one unlisting, from `q`, the panic hook or the hangup watch; first wins.
/// DONE starts true, so under COCKPIT_READONLY this is a no-op.
///
/// tmux keeps a channel while it has a waiter or one latched wake, and frees it
/// on the `-S` that finds neither. So the waiter is woken rather than killed: a
/// killed client is dropped from the channel whenever the server reads its EOF,
/// which can come after the next command. Three `-S` in one command then go
/// wake, latch, free, and the channel does not outlive the board. The kill
/// after is for a waiter the wake did not reach, a server gone among them.
pub fn cleanup() {
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    STOPPING.store(true, Ordering::SeqCst);
    if let Some(c) = CHAN.get() {
        let u = unlist(c);
        let s = ["wait-for", "-S", c];
        let mut a = vec!["set-option", "-gF", OPTION, &u];
        for _ in 0..3 {
            a.push(";");
            a.extend(s);
        }
        tmux_set(&a);
    }
    stop_waiter();
}

impl Drop for Push {
    fn drop(&mut self) {
        cleanup();
    }
}
