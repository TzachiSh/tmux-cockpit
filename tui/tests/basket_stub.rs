//! The basket against a recording tmux stub, never a real server. One test in
//! this binary: it points `COCKPIT_TMUX` at the stub for the process.
use cockpit_tui::basket;
use cockpit_tui::refs::Ref;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// The scratch dir, removed however the test ends.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_basket_never_reaches_a_tmux_it_was_not_given() {
    let d = Dir(std::env::temp_dir().join(format!("cockpit-basket-stub-{}", std::process::id())));
    std::fs::create_dir_all(&d.0).unwrap();
    let calls = d.0.join("calls");
    // a stub named `tmux-stub`: it logs every call, answers %1's id, keeps nothing
    let stub = d.0.join("tmux-stub");
    let body = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\n[ \"$1\" = display-message ] && echo %1\nexit 0\n",
        calls.display()
    );
    std::fs::write(&stub, body).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let f = d.0.join("a.rs");
    std::fs::write(&f, "x\n").unwrap();
    let r = || Ref::new(&f, None, None).unwrap();
    let n = || std::fs::read_to_string(&calls).map_or(0, |c| c.lines().count());

    // SAFETY: the only test in this binary; nothing else reads the env meanwhile.
    unsafe { std::env::set_var("COCKPIT_TMUX", stub.display().to_string()) };
    // a pane that is no `%N` id is refused before tmux runs at all
    for p in ["", "3", "%", "%3;x", "-t"] {
        let why = basket::add(p, r()).unwrap_err();
        assert!(why.contains("pane"), "{p:?}: {why}");
    }
    assert_eq!(n(), 0, "a bad pane reached tmux");

    // a tmux that takes the set and keeps nothing: the read-back says so
    let why = basket::add("%1", r()).unwrap_err();
    assert!(why.contains("did not keep"), "{why}");
    assert!(n() > 0);

    // the guard: under cargo test, a tmux named `tmux` with no -L or -S (the
    // user's server) is refused. This one is the recording stub under that
    // name, so with the guard gone the call lands in its log, never on a server.
    let bin = d.0.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(&stub, bin.join("tmux")).unwrap();
    let before = n();
    unsafe { std::env::set_var("COCKPIT_TMUX", bin.join("tmux").display().to_string()) };
    assert!(
        std::env::var_os("COCKPIT_TEST_TMUX_GUARD").is_some(),
        "tui/.cargo/config.toml sets it"
    );
    let why = basket::read("%1").unwrap_err();
    assert!(why.contains("COCKPIT_TEST_TMUX_GUARD"), "{why}");
    assert_eq!(n(), before, "the guard ran tmux");
}
