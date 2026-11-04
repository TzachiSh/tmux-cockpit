//! A process that does nothing but hold a real `Push` open, so a test can wake
//! it through tmux and send it real signals. The board is the production
//! caller; this is the same `Push::start()` with no terminal to claim, which is
//! what lets the test run headless under `cargo test`.
fn main() {
    // A board's start drops the entries of pids that are not cockpit-tui, so a
    // holder must wear the board's name for a second holder to keep its entry.
    #[cfg(target_os = "linux")]
    // SAFETY: PR_SET_NAME reads a NUL-terminated string of at most 16 bytes.
    unsafe {
        libc::prctl(libc::PR_SET_NAME, c"cockpit-tui".as_ptr());
    }
    let push = cockpit_tui::push::Push::start();
    loop {
        // pending() is what logs the wake, so the loop only has to ask.
        let _ = push.pending();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
