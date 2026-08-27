//! A process that does nothing but hold a real `Push` open, so a test can send
//! it real signals. The board is the production caller; this is the same
//! `Push::start()` with no terminal to claim, which is what lets the test run
//! headless under `cargo test`.
fn main() {
    let push = cockpit_tui::push::Push::start();
    loop {
        // pending() is what logs the wake, so the loop only has to ask.
        let _ = push.pending();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
