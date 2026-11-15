//! The lock a tui test binary writes and reads its environment under (#1110), and the
//! writers of that environment that take its token (#1132). `fake` re-exports all of it,
//! so a test says `fake::env_lock()` and `fake::set_var(&held, ..)`. A crate of its own,
//! outside the tui package, on std alone.

use std::cell::Cell;
use std::ffi::OsStr;
use std::sync::{Mutex, MutexGuard};

/// The one environment lock of a test binary. Private: [`env_lock`] is the only way in,
/// so every holder is known to its own thread.
static ENV: Mutex<()> = Mutex::new(());

thread_local! {
    /// Whether this thread holds [`ENV`], through an [`EnvHeld`].
    static HOLDS: Cell<bool> = const { Cell::new(false) };
}

/// Proof that this thread holds the binary's environment lock. Only [`env_lock`] makes
/// one, so a `_held` check handed it reads under that lock and no other: a guard of
/// another `Mutex<()>` is not one.
pub struct EnvHeld {
    _guard: MutexGuard<'static, ()>,
}

impl Drop for EnvHeld {
    fn drop(&mut self) {
        HOLDS.set(false);
    }
}

/// Takes the binary's environment lock for the life of the token. A lock left poisoned
/// by a holder that panicked is taken all the same: each holder puts its variables back
/// in a `Drop` that runs before the token's, and the next check is to give its own
/// refusal, not a poison panic. A thread that holds it must not join, or wait for the end
/// of, a thread that calls a guard-less check (`fake::checked_state`, `fake::checked_tmux`,
/// and through it `fake::fixed_env` and `fake::Server::start`): that check waits for this
/// lock and the join for the check, for ever. No test does. A wait for such a thread to
/// sleep, as the rows that hold a sibling's write do, ends. `fake::fixed_env` and
/// `fake::Server::start` check under this lock and let it go before the tmux or the write
/// that follows, so a binary that writes TMUX or TMUX_TMPDIR in its own process must not
/// call them: a sibling's value could reach that tmux. None does (panel_env, env_lock).
pub fn env_lock() -> EnvHeld {
    let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    HOLDS.set(true);
    EnvHeld { _guard: guard }
}

/// Whether a holder panicked since the binary started, for the rows that hold what a
/// refusing check leaves behind.
pub fn env_poisoned() -> bool {
    ENV.is_poisoned()
}

/// The lock for the read of a guard-less check named `form`, recovered as in
/// [`env_lock`]. On a thread that holds it already the take would wait for itself, so
/// that call panics instead, naming the form that takes the holder's token, and, for a
/// call that came through a helper, what the caller does instead. For `fake`'s checks
/// alone: the guard it returns writes nothing.
pub fn for_check(form: &str) -> MutexGuard<'static, ()> {
    if HOLDS.get() {
        let held = "on a thread that holds fake::env_lock(): call";
        let helper = "or, called through fake::fixed_env or fake::Server::start, call that \
                      helper before you take the lock";
        panic!("fake::{form}() {held} fake::{form}_held(&held), {helper} (#1110)");
    }
    ENV.lock().unwrap_or_else(|e| e.into_inner())
}

/// `std::env::set_var` under the binary's lock, which `_held` shows this thread holds.
///
/// # Safety
///
/// As for `std::env::set_var`: the lock keeps out every reader that takes it, but not a
/// read made by C code or by a thread that takes no lock.
pub unsafe fn set_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(_held: &EnvHeld, key: K, value: V) {
    unsafe { std::env::set_var(key, value) }
}

/// `std::env::remove_var` under the binary's lock, which `_held` shows this thread holds.
///
/// # Safety
///
/// As for [`set_var`].
pub unsafe fn remove_var<K: AsRef<OsStr>>(_held: &EnvHeld, key: K) {
    unsafe { std::env::remove_var(key) }
}

/// `std::env::set_var` with no lock, for the writes that took none before #1132, renamed
/// as they stood. Step 2 of #1132 moves each to [`set_var`] under [`env_lock`] and then
/// removes this; a row of tmux_reach.rs counts its calls, and the count only goes down.
///
/// # Safety
///
/// As for `std::env::set_var`, and no lock keeps a sibling's read out.
pub unsafe fn set_var_unlocked<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    unsafe { std::env::set_var(key, value) }
}

/// `std::env::remove_var` with no lock: as [`set_var_unlocked`], removed by step 2 of #1132.
///
/// # Safety
///
/// As for [`set_var_unlocked`].
pub unsafe fn remove_var_unlocked<K: AsRef<OsStr>>(key: K) {
    unsafe { std::env::remove_var(key) }
}
