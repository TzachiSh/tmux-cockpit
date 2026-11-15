//! The canary of #1132: the gate's clippy must refuse each raw environment write below,
//! one per path in tui/clippy.toml. If one builds, the lint is off for it. Built only by
//! tests/tui-clippy-canary.sh, with the clippy-canary feature.
fn main() {
    // SAFETY: never run; clippy is to refuse every call before it builds.
    unsafe {
        std::env::set_var("COCKPIT_CANARY", "1");
        std::env::remove_var("COCKPIT_CANARY");
        libc::setenv(c"COCKPIT_CANARY".as_ptr(), c"1".as_ptr(), 1);
        libc::unsetenv(c"COCKPIT_CANARY".as_ptr());
        libc::putenv(std::ptr::null_mut());
        libc::clearenv();
    }
}
