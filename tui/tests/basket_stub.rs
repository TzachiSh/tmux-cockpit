//! A tmux that takes the basket's `set-option` and keeps nothing: the write
//! reads it back and says so, rather than report an add that did not land.
//! One test in this binary: it points `COCKPIT_TMUX` at the stub.
use cockpit_tui::basket;
use cockpit_tui::refs::Ref;
use std::os::unix::fs::PermissionsExt;

#[test]
fn a_write_tmux_did_not_keep_is_an_error() {
    let d = std::env::temp_dir().join(format!("cockpit-basket-stub-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let stub = d.join("tmux");
    std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let f = d.join("a.rs");
    std::fs::write(&f, "x\n").unwrap();
    // SAFETY: the only test in this binary; nothing else reads the env meanwhile.
    unsafe { std::env::set_var("COCKPIT_TMUX", stub.display().to_string()) };
    let why = basket::add("%1", Ref::new(&f, None, None).unwrap()).unwrap_err();
    let _ = std::fs::remove_dir_all(&d);
    assert!(why.contains("did not keep"), "{why}");
}
