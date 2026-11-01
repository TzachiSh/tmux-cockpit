//! What the environment drives: the child's env, `Call::from_env`, overrides and
//! the capture. Env vars are process-global, so every test holds `ENV`.
use cockpit_tui::panel::capture::capture;
use cockpit_tui::panel::envelope::{Flow, OutType};
use cockpit_tui::panel::prompts::{self, Ctx};
use cockpit_tui::panel::request::{Call, Request};
use cockpit_tui::panel::route::route;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

static ENV: Mutex<()> = Mutex::new(());

struct Env {
    old: Vec<(&'static str, Option<String>)>,
    dir: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl Env {
    /// `None` unsets the variable for the test.
    fn new(tag: &str, vars: &[(&'static str, Option<String>)]) -> Env {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("cockpit-panel-env-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = vars.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect();
        for (k, v) in vars {
            // SAFETY: ENV is held for the guard's lifetime and every test here takes it.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v.replace("$D", &dir.display().to_string())),
                    None => std::env::remove_var(k),
                }
            }
        }
        Env { old, dir, _lock: lock }
    }

    fn exec(&self, name: &str, body: &str) -> String {
        let p = self.dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{}\n", body.replace("$D", &self.dir.display().to_string()))).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for (k, v) in &self.old {
            // SAFETY: as above.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..1000 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn the_call_is_no_panes_agent_and_marks_itself() {
    let e = Env::new(
        "child",
        &[
            ("CLAUDECODE", Some("1".into())),
            ("TMUX", Some("/tmp/tmux-1000/default,1,0".into())),
            ("TMUX_PANE", Some("%3".into())),
            ("COCKPIT_PANEL_CALL", None),
            ("COCKPIT_CLAUDE", Some("$D/claude".into())),
            ("COCKPIT_PANEL_TIMEOUT", Some("7".into())),
            ("XDG_CACHE_HOME", Some("$D/cache".into())),
        ],
    );
    e.exec("claude", "env > $D/env\necho ok");
    let call = Call::from_env();
    assert_eq!(call.claude, format!("{}/claude", e.dir.display()));
    assert_eq!(call.timeout, Duration::from_secs(7));
    assert_eq!(call.dir, e.dir.join("cache/tmux-cockpit/panel"));

    let mut req = Request::spawn(&call, "S", "p", "haiku");
    let mut got = None;
    assert!(until(|| {
        got = req.poll();
        got.is_some()
    }));
    assert_eq!(got.unwrap(), Ok("ok\n".into()));
    let env = std::fs::read_to_string(e.dir.join("env")).unwrap();
    let lines: Vec<&str> = env.lines().collect();
    assert!(lines.contains(&"COCKPIT_PANEL_CALL=1"), "{env}");
    for k in ["CLAUDECODE=", "TMUX=", "TMUX_PANE="] {
        assert!(!lines.iter().any(|l| l.starts_with(k)), "{k} leaked into the child:\n{env}");
    }
}

#[test]
fn the_call_defaults_without_its_variables() {
    let e = Env::new(
        "defaults",
        &[
            ("COCKPIT_CLAUDE", None),
            ("COCKPIT_PANEL_TIMEOUT", Some("soon".into())),
            ("XDG_CACHE_HOME", Some("relative/cache".into())),
            ("HOME", Some("$D".into())),
        ],
    );
    let call = Call::from_env();
    assert_eq!(call.claude, "claude");
    assert_eq!(call.timeout, Duration::from_secs(120), "an unparsable timeout is the default");
    assert_eq!(
        call.dir,
        e.dir.join(".cache/tmux-cockpit/panel"),
        "a relative XDG_CACHE_HOME is ignored"
    );
}

#[test]
fn cockpit_panel_model_beats_every_choice() {
    let _e = Env::new("model", &[("COCKPIT_PANEL_MODEL", Some("opus".into()))]);
    let r = route(
        &Flow::Write {
            draft: "x".into(),
            out: OutType::Steps,
        },
        "haiku",
    );
    assert_eq!(
        (r.model.as_str(), r.why.as_str(), r.suggest),
        ("opus", "COCKPIT_PANEL_MODEL is set", None)
    );
}

#[test]
fn the_shipped_prompts_render_without_a_placeholder_left() {
    let _e = Env::new(
        "shipped",
        &[("XDG_CONFIG_HOME", Some("$D/cfg".into())), ("HOME", Some("/home/u".into()))],
    );
    let platform = prompts::platform();
    let write = prompts::write(&Ctx {
        window: "shell",
        ..Ctx::default()
    });
    for (name, text) in [("write", &write), ("explain", &prompts::explain()), ("fix", &prompts::fix())] {
        assert!(!text.contains("{{"), "{name} left a placeholder");
        assert!(text.contains(&platform), "{name} has no platform");
        assert!(!text.ends_with('\n'), "{name} keeps its trailing newline");
    }
    assert!(
        write.contains("<examples>\n<example>") && write.contains("@/home/u/bin/net-proxy"),
        "examples carry {{home}}"
    );
}

#[test]
fn the_shipped_write_prompt_never_asks_for_a_file_edit() {
    let _e = Env::new("no-edit", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    for out in [OutType::Auto, OutType::Command, OutType::Format, OutType::Hint] {
        for window in ["claude", "shell", "ssh", "other"] {
            let w = prompts::write(&Ctx { out, guess: out, window });
            assert!(
                !w.contains("<edit") && !w.contains("<need_file") && !w.contains("<file"),
                "{out:?} {window}"
            );
        }
    }
}

#[test]
fn a_users_prompt_file_wins_over_the_shipped_one() {
    let e = Env::new("override", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    let d = e.dir.join("cfg/tmux-cockpit/prompts");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("fix.txt"), "Mine on {{platform}}\n\n").unwrap();
    std::fs::write(d.join("write.examples.txt"), "<example id=\"x\" core=\"1\">only mine</example>").unwrap();
    assert_eq!(prompts::fix(), format!("Mine on {}", prompts::platform()));
    assert!(
        prompts::explain().starts_with("You help a user understand"),
        "an absent override keeps the shipped text"
    );
    let write = prompts::write(&Ctx::default());
    assert!(write.ends_with("<examples>\n<example>only mine</example>\n</examples>"), "{write}");
}

#[test]
fn a_broken_prompt_file_falls_back_and_says_why() {
    let e = Env::new("broken", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    let d = e.dir.join("cfg/tmux-cockpit/prompts");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("fix.txt"), b"bad \xff byte").unwrap();
    let (fix, note) = prompts::fix_noted();
    assert!(
        fix.starts_with("You correct a draft"),
        "invalid UTF-8 keeps the shipped text: {fix}"
    );
    assert!(note.as_deref().is_some_and(|n| n.contains("fix.txt")), "{note:?}");

    let p = d.join("explain.txt");
    std::fs::write(&p, "Mine").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads a mode-000 file anyway; a directory fails for everyone.
    if std::fs::read(&p).is_ok() {
        std::fs::remove_file(&p).unwrap();
        std::fs::create_dir(&p).unwrap();
    }
    let (explain, note) = prompts::explain_noted();
    assert!(explain.starts_with("You help a user understand"), "{explain}");
    assert!(note.as_deref().is_some_and(|n| n.contains("explain.txt")), "{note:?}");
    assert_eq!(prompts::write_noted(&Ctx::default()).1, None, "an absent override is no note");
}

fn fake_tmux(e: &Env, display: &str) {
    let body = format!(
        "case \"$1\" in\ndisplay) printf '{display}' ;;\ncapture-pane) echo \"$@\" > $D/args; printf 'line1\\n\\nline2\\n  \\n\\n' ;;\n*) exit 1 ;;\nesac"
    );
    let t = e.exec("tmux", &body);
    // SAFETY: ENV is held by `e`.
    unsafe { std::env::set_var("COCKPIT_TMUX", t) };
}

#[test]
fn a_claude_pane_is_captured_deeper_and_trimmed() {
    let e = Env::new("capture", &[("COCKPIT_TMUX", None)]);
    fake_tmux(&e, "claude\\tclaude\\t/w\\n");
    let c = capture("%5").unwrap();
    assert_eq!((c.name.as_str(), c.cmd.as_str(), c.path.as_str()), ("claude", "claude", "/w"));
    assert_eq!(c.screen, "line1\n\nline2");
    assert_eq!(
        std::fs::read_to_string(e.dir.join("args")).unwrap().trim(),
        "capture-pane -p -J -t %5 -S -200"
    );
}

#[test]
fn a_shell_pane_with_no_window_name_keeps_its_fields() {
    let e = Env::new("capture-shell", &[("COCKPIT_TMUX", None)]);
    fake_tmux(&e, "\\tbash\\t/home/u\\n");
    let c = capture("%6").unwrap();
    assert_eq!((c.name.as_str(), c.cmd.as_str(), c.path.as_str()), ("", "bash", "/home/u"));
    assert!(std::fs::read_to_string(e.dir.join("args")).unwrap().contains("-S -80"));
}

#[test]
fn a_dead_pane_captures_nothing() {
    let e = Env::new("capture-dead", &[("COCKPIT_TMUX", None)]);
    let t = e.exec("tmux", "exit 1");
    // SAFETY: ENV is held by `e`.
    unsafe { std::env::set_var("COCKPIT_TMUX", t) };
    assert_eq!(capture("%7"), None);
    assert!(!Path::new(&e.dir.join("args")).exists());
}
