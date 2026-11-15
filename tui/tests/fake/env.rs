//! The lock a test binary writes and reads its environment under (#1110), apart from the
//! rest of `fake` and on std alone, so that a row can build this file by itself and show
//! that an [`EnvHeld`] has no constructor outside it. `fake`'s checks read under it.
// a binary that loads `fake` and takes no lock leaves these unused
#![allow(dead_code)]

use std::cell::Cell;
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
/// sleep, as the rows that hold a sibling's write do, ends.
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
/// that call panics instead, naming the form that takes the holder's token.
pub(super) fn for_check(form: &str) -> MutexGuard<'static, ()> {
    if HOLDS.get() {
        let held = "on a thread that holds fake::env_lock(): call";
        panic!("fake::{form}() {held} fake::{form}_held(&held) (#1110)");
    }
    ENV.lock().unwrap_or_else(|e| e.into_inner())
}
