//! A live panel reads its pane again before each model call (#1354, #1342 amendment
//! 9), off the UI thread, so a question round sees the screen as it is now. A fixture
//! never does. A fake tmux and claude go first on PATH, so every row holds fake's
//! environment lock from its setup to its end.
mod fake;

use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::capture::{Capture, Repo};
use cockpit_tui::panel::request::{Call, Effort};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fake::Scratch;
use std::path::Path;
use std::time::Duration;

const PASSES: usize = 3000;
const OLD: &str = "the screen at open";
const NEW: &str = "the screen now";

/// COCKPIT_TMUX the bare name, found first on a PATH that starts with the scratch
/// dir; no TMUX, and TMUX_TMPDIR in the scratch dir, so a real tmux reached by
/// mistake finds no server. All four put back on drop under the same lock.
struct Rig {
    d: Scratch,
    old: Vec<(&'static str, Option<String>)>,
    held: fake::EnvHeld,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        fake::fixed_env();
        // the runner's own socket folder, checked before this rig replaces it (#1066)
        fake::checked_tmux();
        let held = fake::env_lock();
        let name = format!("cockpit-refresh-{tag}-{}", std::process::id());
        let d = Scratch::fresh(std::env::temp_dir().join(name));
        let q = d.display().to_string();
        let path = format!("{q}:{}", std::env::var("PATH").unwrap_or_default());
        let set: [(&'static str, Option<String>); 4] = [
            ("COCKPIT_TMUX", Some("tmux".into())),
            ("PATH", Some(path)),
            ("TMUX", None),
            ("TMUX_TMPDIR", Some(q.clone())),
        ];
        let old = set.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect();
        for (k, v) in &set {
            // SAFETY: every row here takes the lock first and holds it to its end.
            unsafe {
                match v {
                    Some(v) => fake::set_var(&held, k, v),
                    None => fake::remove_var(&held, k),
                }
            }
        }
        // each call logged; the pane is a bash in the scratch dir showing NEW
        let tmux = format!(
            "#!/bin/sh\necho \"$1\" >> '{q}/calls'\ncase \"$1 $*\" in\n\
             display*window_name*) printf 'w\\tbash\\t%s\\n' '{q}' ;;\n\
             capture-pane*) printf '%s\\n' '{NEW}' ;;\n*) exit 1 ;;\nesac\n"
        );
        fake::write_exec(&d.join("tmux"), &tmux);
        Rig { d, old, held }
    }

    /// How many times the panel read the pane's screen.
    fn captures(&self) -> usize {
        let calls = std::fs::read_to_string(self.d.join("calls")).unwrap_or_default();
        calls.lines().filter(|l| *l == "capture-pane").count()
    }

    /// A claude that keeps what it was sent in `stdin`, then fails.
    fn call(&self) -> Call {
        let q = self.d.display().to_string();
        let body = format!("#!/bin/sh\ncat > '{q}/stdin.tmp'\nmv '{q}/stdin.tmp' '{q}/stdin'\nexit 3\n");
        fake::write_exec(&self.d.join("claude-stub"), &body);
        std::fs::create_dir_all(self.d.join("cache")).unwrap();
        Call {
            claude: self.d.join("claude-stub").display().to_string(),
            timeout: Duration::from_secs(60),
            dir: Some(self.d.join("cache")),
            effort: Effort::Medium,
        }
    }

    fn sent(&self) -> String {
        std::fs::read_to_string(self.d.join("stdin")).unwrap_or_default()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        for (k, v) in &self.old {
            // SAFETY: as above.
            unsafe {
                match v {
                    Some(v) => fake::set_var(&self.held, k, v),
                    None => fake::remove_var(&self.held, k),
                }
            }
        }
    }
}

/// A panel opened on `%3` in `d` when its screen showed OLD, in Compose with a draft.
fn app(d: &Path, live: bool) -> App {
    let cap = Capture {
        name: "w".into(),
        cmd: "bash".into(),
        path: d.display().to_string(),
        screen: OLD.into(),
        repo: "".into(),
        ..Default::default()
    };
    let mut a = App::new("%3", cap);
    a.live = live;
    a.view = View::Compose;
    a.draft = "list the files".into();
    a
}

/// ^S, then the loop's two calls until the answer lands: a count of passes.
fn write_answered(a: &mut App, call: &Call) -> bool {
    a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    answered(a, call)
}

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

#[test]
fn a_live_panel_sends_the_screen_as_it_is_at_the_call() {
    let r = Rig::new("now");
    let mut a = app(&r.d, true);
    assert!(write_answered(&mut a, &r.call()), "no answer in {PASSES} passes");
    let sent = r.sent();
    assert!(sent.contains(NEW) && !sent.contains(OLD), "{sent}");
}

#[test]
fn every_call_of_a_live_panel_reads_the_screen_again() {
    let r = Rig::new("each");
    let call = r.call();
    let mut a = app(&r.d, true);
    assert!(write_answered(&mut a, &call), "no answer in {PASSES} passes");
    assert!(matches!(a.view, View::Error { .. }), "{:?}", a.view);
    a.key(KeyEvent::from(KeyCode::Char('r')));
    assert!(answered(&mut a, &call), "no answer to the retry in {PASSES} passes");
    assert_eq!(r.captures(), 2);
}

/// A live panel whose pane tmux can no longer read sends nothing (#1376). The call is
/// refused with an error that names the pane, never sent with the screen at open.
#[test]
fn a_live_panel_whose_pane_is_gone_sends_nothing() {
    let r = Rig::new("gone");
    let q = r.d.display().to_string();
    fake::write_exec(&r.d.join("tmux"), &format!("#!/bin/sh\necho \"$1\" >> '{q}/calls'\nexit 1\n"));
    let call = r.call();
    let mut a = app(&r.d, true);
    a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let landed = (0..PASSES).any(|_| {
        a.launch(&call);
        if a.pending {
            std::thread::sleep(Duration::from_millis(10));
        }
        !a.pending
    });
    assert!(landed, "the envelope never landed in {PASSES} passes");
    match &a.view {
        // the pane named with no id (#1407)
        View::Error { line, .. } => assert!(
            line.contains("tmux cannot read the pane: it may be gone, so nothing was sent") && !line.contains("%3"),
            "{line}"
        ),
        v => panic!("not refused: {v:?}"),
    }
    assert!(a.req.is_none(), "a call was forked");
    assert_eq!(r.sent(), "", "the stale screen was sent");
}

#[test]
fn a_fixture_panel_never_reads_its_pane_again() {
    let r = Rig::new("fixture");
    let mut a = app(&r.d, false);
    assert!(write_answered(&mut a, &r.call()), "no answer in {PASSES} passes");
    assert!(r.sent().contains(OLD), "{}", r.sent());
    assert_eq!(r.captures(), 0);
}

/// A fixture whose repo block is still in git's hands is built on the thread too: it
/// still never reads its pane. The fake git's config read waits for `go` at most this
/// many 10 ms rounds.
const ROUNDS: u32 = 3000;

#[test]
fn a_fixture_built_off_the_thread_never_reads_its_pane_again() {
    let r = Rig::new("fixture-thread");
    let q = r.d.display().to_string();
    let git = format!(
        "#!/bin/sh\nfor a; do case $a in config|rev-parse|status|log|diff) s=$a; break ;; esac; done\n\
         case $s in\n\
         config) n=0; until [ -e '{q}/go' ] || [ $n -ge {ROUNDS} ]; do sleep 0.01; n=$((n+1)); done\n\
         exit 1 ;;\nrev-parse) echo '{q}' ;;\nstatus) echo '## main' ;;\nesac\n"
    );
    fake::write_exec(&r.d.join("git"), &git);
    let call = r.call();
    let mut a = app(&r.d, false);
    a.cap.repo = Repo::start(&q, "", Duration::from_secs(3600));
    a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    std::fs::write(r.d.join("go"), "").unwrap();
    assert!(answered(&mut a, &call), "no answer in {PASSES} passes");
    assert_eq!(r.captures(), 0);
}
