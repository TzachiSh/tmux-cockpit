//! The canary of #1132 for the forbid: an inner allow of the lint must be refused (E0453).
//! Under deny it would let this raw write through. Built only by `make tui-clippy-canary`.
#[allow(clippy::disallowed_methods)]
fn main() {
    // SAFETY: never run; clippy is to refuse the allow above before it builds.
    unsafe { std::env::set_var("COCKPIT_CANARY", "1") };
}
