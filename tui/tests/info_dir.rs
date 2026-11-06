//! #452: the info screen, `info --json` and the board name one work dir. Its own
//! binary: `gather` reads COCKPIT_TMUX, CLAUDE_CONFIG_DIR and HOME, process-global,
//! so every test here holds `ENV`.
mod fake;

use cockpit_tui::data::info::{Facts, WorkFrom, gather, git_root};
use cockpit_tui::data::list_panes;
use std::path::PathBuf;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

/// A scratch tree under cargo's own scratch dir, removed on drop, so a failing
/// assert leaves nothing in the system temp dir.
struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Tmp {
        let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("info-dir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }
    fn sub(&self, name: &str) -> String {
        let d = self.0.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d.display().to_string()
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The board's slug: every byte outside `[A-Za-z0-9-]` is `-`.
fn slug(p: &str) -> String {
    p.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' { b as char } else { '-' })
        .collect()
}

/// Pane %1 runs claude in `cwd`, `@cockpit_dir` is `opt`, and its transcript, where
/// both the board and info look, says `said`. The shim answers list-panes for the
/// board and display-message and show-option for info. Returns the shim's env.
fn rig(t: &Tmp, cwd: &str, opt: &str, said: &str) -> Vec<(&'static str, String)> {
    let d = &t.0;
    let home = d.join("home");
    let proj = home.join(".claude/projects").join(slug(cwd));
    std::fs::create_dir_all(&proj).unwrap();
    let tr = proj.join("s1.jsonl");
    std::fs::write(
        &tr,
        format!("{}\n", serde_json::json!({"type": "user", "cwd": said, "message": {}})),
    )
    .unwrap();
    let info = ["%1", "w:0 win", "claude", cwd, opt, "running", "", "", "", "s1", "", "1", ""];
    std::fs::write(d.join("display"), info.join("\u{1f}")).unwrap();
    let board = [
        "%1", "w:0", "win", "claude", "running", "-", "-", "s1", cwd, "100", "-", "-", "@1", opt,
    ];
    std::fs::write(d.join("rows"), board.join("\u{1f}") + "\n").unwrap();
    let sh = format!(
        "#!/bin/sh\ncase \"$1\" in\nlist-panes) cat '{0}/rows' ;;\n\
         display-message) case \"$5\" in *cockpit_tui_version*) cat '{0}/display' ;; *) cat '{0}/rows' ;; esac ;;\n\
         show-option) case \"$*\" in *@cockpit_transcript*) echo '{1}' ;; *) echo '|claude|' ;; esac ;;\nesac\n",
        d.display(),
        tr.display()
    );
    fake::write_exec(&d.join("tmux"), &sh);
    vec![
        ("COCKPIT_TMUX", d.join("tmux").display().to_string()),
        ("CLAUDE_CONFIG_DIR", home.join(".claude").display().to_string()),
        ("HOME", home.display().to_string()),
        ("COCKPIT_READONLY", "1".into()),
    ]
}

fn set(env: &[(&'static str, String)]) {
    for (k, v) in env {
        // SAFETY: every test in this binary holds ENV while it touches the environment.
        unsafe { std::env::set_var(k, v) };
    }
}

fn facts(t: &Tmp, cwd: &str, opt: &str, said: &str) -> Facts {
    set(&rig(t, cwd, opt, said));
    gather("%1").unwrap()
}

/// `cockpit-tui info --json <pane>` under `env`: (exit code, stdout).
fn info_json(env: &[(&'static str, String)], pane: &str) -> (Option<i32>, Vec<u8>) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["info", "--json", pane])
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .output()
        .unwrap();
    (out.status.code(), out.stdout)
}

/// Rows: the transcript's cwd over the pane's; `@cockpit_dir` over both; a
/// dangling transcript cwd falls to the pane's. `root` stays tmux-review's.
#[test]
fn the_work_dir_follows_the_board_order_and_root_does_not() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let t = Tmp::new("order");
    let (wt, opt, cwd) = (t.sub("wt"), t.sub("opt"), t.sub("cwd"));
    // wt its own repo: had root followed the work dir, it would be wt itself
    let git = std::process::Command::new("git")
        .args(["init", "-q", &wt])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status();
    assert!(git.unwrap().success());
    let x = facts(&t, &cwd, "", &wt);
    assert_eq!((x.work.as_str(), x.work_from), (wt.as_str(), WorkFrom::Transcript));
    assert_eq!(x.root, git_root(&cwd), "root keeps tmux-review's rule, off the cwd");
    assert_ne!(x.root, wt);
    let x = facts(&t, &cwd, &opt, &wt);
    assert_eq!((x.work.as_str(), x.work_from), (opt.as_str(), WorkFrom::Opt));
    let x = facts(&t, &cwd, "", "/no/such/wt");
    assert_eq!((x.work.as_str(), x.work_from), (cwd.as_str(), WorkFrom::Cwd));
}

/// The orchestrator's tmux-review will read `info --json %N`'s `dir`: the dir the
/// board's stat used for the same pane, for each of the three sources.
#[test]
fn info_json_dir_is_the_boards_work_dir() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let t = Tmp::new("json");
    let (wt, opt, cwd) = (t.sub("wt"), t.sub("opt"), t.sub("cwd"));
    for (o, s, want) in [("", &wt, &wt), (opt.as_str(), &wt, &opt), ("", &"/no/such/wt".to_string(), &cwd)] {
        let env = rig(&t, &cwd, o, s);
        let (code, stdout) = info_json(&env, "%1");
        let v: serde_json::Value = serde_json::from_slice(&stdout).unwrap_or_default();
        set(&env);
        let board = list_panes().unwrap().into_iter().find(|p| p.id == "%1").unwrap().dir;
        assert_eq!(
            (code, v["dir"].as_str(), board.as_str()),
            (Some(0), Some(want.as_str()), want.as_str())
        );
    }
}

/// A pane that is not there: exit 1, nothing on stdout, as `--text` does.
#[test]
fn info_json_for_no_such_pane_is_exit_1_and_silent() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let t = Tmp::new("gone");
    let env = rig(&t, &t.sub("cwd"), "", "");
    std::fs::write(t.0.join("display"), "").unwrap();
    assert_eq!(info_json(&env, "%9"), (Some(1), vec![]));
}

/// #460: gather reads `@cockpit_job_name` and `@cockpit_job`, the name cleaned
/// (a pane title); with neither, no job.
#[test]
fn gather_reads_the_bound_job_cleaned() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let t = Tmp::new("job");
    let cwd = t.sub("cwd");
    set(&rig(&t, &cwd, "", ""));
    let plain = std::fs::read_to_string(t.0.join("display")).unwrap();
    std::fs::write(t.0.join("display"), format!("{plain}\u{1f}bg_7\u{1f}swe\u{202e}ep\u{7}")).unwrap();
    assert_eq!(gather("%1").unwrap().job, Some(("sweep".to_string(), "bg_7".to_string())));
    std::fs::write(t.0.join("display"), format!("{plain}\u{1f}\u{1f}")).unwrap();
    assert_eq!(gather("%1").unwrap().job, None);
    // a name with no id is no bind (agent-poll sets both at once): never `(@cockpit_job )`
    std::fs::write(t.0.join("display"), format!("{plain}\u{1f}\u{1f}lone")).unwrap();
    assert_eq!(gather("%1").unwrap().job, None);
}
