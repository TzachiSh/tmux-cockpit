//! The panel builds its envelope off the UI thread while the repo block's git still
//! runs (#1354, #1342 amendment 10): `busy()` returns before git does. A fake git goes
//! first on PATH, so every row holds fake's environment lock from its setup to its end.
mod fake;

use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::capture::{Capture, Repo};
use cockpit_tui::panel::request::{Call, Effort};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fake::Scratch;
use std::path::Path;
use std::time::Duration;

/// The fake git's config call waits for `go` this many 10 ms rounds, then writes
/// `done`: a `busy()` that waits for git returns only after `done` exists.
const ROUNDS: u32 = 3000;

/// How many loop passes a row gives the call to go out and come back.
const PASSES: usize = 3000;

/// PATH with the scratch dir first, put back on drop under the same lock.
struct OnPath {
    old: Option<String>,
    held: fake::EnvHeld,
}

impl OnPath {
    fn new(dir: &Path) -> OnPath {
        fake::fixed_env();
        let held = fake::env_lock();
        let old = std::env::var("PATH").ok();
        let path = format!("{}:{}", dir.display(), old.clone().unwrap_or_default());
        // SAFETY: every row here takes the lock first and holds it to its end.
        unsafe { fake::set_var(&held, "PATH", path) };
        OnPath { old, held }
    }
}

impl Drop for OnPath {
    fn drop(&mut self) {
        // SAFETY: as above.
        unsafe {
            match &self.old {
                Some(v) => fake::set_var(&self.held, "PATH", v),
                None => fake::remove_var(&self.held, "PATH"),
            }
        }
    }
}

/// A git whose config read waits for `<d>/go`, then answers as a clean repo on main.
fn hung_git(d: &Path) {
    let q = d.display().to_string();
    let body = format!(
        "#!/bin/sh\nfor a; do case $a in config|rev-parse|status|log|diff) s=$a; break ;; esac; done\n\
         case $s in\n\
         config) n=0; until [ -e '{q}/go' ] || [ $n -ge {ROUNDS} ]; do sleep 0.01; n=$((n+1)); done\n\
         : > '{q}/done'; exit 1 ;;\n\
         rev-parse) echo '{q}' ;;\nstatus) echo '## main' ;;\nlog) echo 'abc1234 first' ;;\nesac\n"
    );
    fake::write_exec(&d.join("git"), &body);
}

/// An app on a shell pane in `d`, its repo block started now, in Compose with a draft.
fn pending_app(d: &Path) -> App {
    let dir = d.display().to_string();
    let cap = Capture {
        name: "work".into(),
        cmd: "bash".into(),
        path: dir.clone(),
        screen: "$ ".into(),
        repo: Repo::start(&dir, "", Duration::from_secs(3600)),
        ..Default::default()
    };
    let mut a = App::new("%3", cap);
    a.view = View::Compose;
    a.draft = "list the files".into();
    a
}

fn ctrl_s(a: &mut App) {
    a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
}

/// A claude that keeps what it was sent in `<d>/stdin`, then fails.
fn call(d: &Path) -> Call {
    let bin = d.join("claude-stub");
    let q = d.display().to_string();
    fake::write_exec(
        &bin,
        &format!("#!/bin/sh\ncat > '{q}/stdin.tmp'\nmv '{q}/stdin.tmp' '{q}/stdin'\nexit 3\n"),
    );
    std::fs::create_dir_all(d.join("cache")).unwrap();
    Call {
        claude: bin.display().to_string(),
        timeout: Duration::from_secs(60),
        dir: d.join("cache"),
        effort: Effort::Medium,
    }
}

/// The loop's two calls until the answer lands: a count of passes, never a clock.
fn answered(a: &mut App, call: &Call) -> bool {
    (0..PASSES).any(|_| {
        a.launch(call);
        let got = a.poll();
        if !got {
            std::thread::sleep(Duration::from_millis(10));
        }
        got
    })
}

fn scratch(tag: &str) -> Scratch {
    Scratch::fresh(std::env::temp_dir().join(format!("cockpit-offthread-{tag}-{}", std::process::id())))
}

#[test]
fn busy_returns_while_the_repo_blocks_git_hangs() {
    let d = scratch("hang");
    let _p = OnPath::new(&d);
    hung_git(&d);
    let mut a = pending_app(&d);
    ctrl_s(&mut a);
    let waited = d.join("done").exists();
    std::fs::write(d.join("go"), "").unwrap();
    a.cap.repo.get();
    assert!(!waited, "busy() returned only after the fake git's config call ended");
    assert_eq!(a.view, View::Busy);
}

#[test]
fn the_envelope_built_off_the_thread_carries_the_repo_block() {
    let d = scratch("carries");
    let _p = OnPath::new(&d);
    hung_git(&d);
    let mut a = pending_app(&d);
    let call = call(&d);
    ctrl_s(&mut a);
    std::fs::write(d.join("go"), "").unwrap();
    assert!(answered(&mut a, &call), "no answer in {PASSES} passes");
    let sent = std::fs::read_to_string(d.join("stdin")).unwrap_or_default();
    assert!(sent.contains("<repo>\nbranch: main\n"), "{sent}");
}

#[test]
fn no_call_goes_out_while_the_envelope_is_built() {
    let d = scratch("waits");
    let _p = OnPath::new(&d);
    hung_git(&d);
    let mut a = pending_app(&d);
    let call = call(&d);
    ctrl_s(&mut a);
    a.launch(&call);
    let early = a.req.is_some();
    std::fs::write(d.join("go"), "").unwrap();
    assert!(answered(&mut a, &call), "no answer in {PASSES} passes");
    assert!(!early, "a call went out before its envelope was built");
}

#[test]
fn a_cancel_while_building_sends_nothing() {
    let d = scratch("cancel");
    let _p = OnPath::new(&d);
    hung_git(&d);
    let mut a = pending_app(&d);
    let call = call(&d);
    ctrl_s(&mut a);
    a.key(KeyEvent::from(KeyCode::Esc));
    std::fs::write(d.join("go"), "").unwrap();
    a.cap.repo.get();
    for _ in 0..50 {
        a.launch(&call);
        a.poll();
    }
    assert!(a.req.is_none() && !a.pending, "a cancelled write still went out");
    assert_eq!(a.view, View::Compose);
}

#[test]
fn a_cancel_while_building_leaves_the_loop_idle() {
    let d = scratch("idle");
    let _p = OnPath::new(&d);
    hung_git(&d);
    let mut a = pending_app(&d);
    ctrl_s(&mut a);
    a.key(KeyEvent::from(KeyCode::Esc));
    let ticking = a.ticks();
    std::fs::write(d.join("go"), "").unwrap();
    a.cap.repo.get();
    assert!(!ticking, "the loop still wakes for an envelope nobody will send");
}

#[test]
fn the_loop_ticks_while_the_envelope_is_built() {
    let d = scratch("ticks");
    let _p = OnPath::new(&d);
    hung_git(&d);
    let mut a = pending_app(&d);
    ctrl_s(&mut a);
    let ticking = a.ticks();
    std::fs::write(d.join("go"), "").unwrap();
    a.cap.repo.get();
    assert!(ticking, "the loop would sleep on a key while the envelope is built");
}

#[test]
fn the_hidden_counts_land_with_the_envelope() {
    let d = scratch("counts");
    let _p = OnPath::new(&d);
    hung_git(&d);
    let mut a = pending_app(&d);
    // panel_counts pins this screen at one secret
    a.cap.screen = "$ ls\nsk-abcdefghijklmnopqrst1".into();
    let call = call(&d);
    ctrl_s(&mut a);
    std::fs::write(d.join("go"), "").unwrap();
    assert!(answered(&mut a, &call), "no answer in {PASSES} passes");
    assert_eq!(a.sent_hidden().screen, 1, "{:?}", a.sent_hidden());
}
