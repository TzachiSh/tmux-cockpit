//! #452: the info screen, `info --json` and the board name one work dir. Its own
//! binary: `gather` reads COCKPIT_TMUX, CLAUDE_CONFIG_DIR and HOME, process-global,
//! so every test here holds fake's environment lock.
mod fake;

use cockpit_tui::data::info::{Facts, WorkFrom, gather, git_root};
use cockpit_tui::data::list_panes;
use fake::EnvHeld;
use std::path::PathBuf;
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
        // `set` never puts PATH back, so each rig adds an entry that outlives its folder
        ("PATH", fake::rig_path(&d.join("bin"))),
        ("CLAUDE_CONFIG_DIR", home.join(".claude").display().to_string()),
        ("HOME", home.display().to_string()),
        ("COCKPIT_READONLY", "1".into()),
    ]
}

fn set(g: &EnvHeld, t: &Tmp, env: &[(&'static str, String)]) {
    // the runner unset the caller's cache and state, which a lookup uses (#1066)
    fake::checked_state_held(g);
    for (k, v) in env {
        // SAFETY: every test in this binary holds the lock while it touches the environment.
        unsafe { fake::set_var(g, k, v) };
    }
    fake::assert_claude_is_the_rigs(&t.0);
}

fn facts(g: &EnvHeld, t: &Tmp, cwd: &str, opt: &str, said: &str) -> Facts {
    set(g, t, &rig(t, cwd, opt, said));
    gather("%1").unwrap()
}

/// `cockpit-tui info --json <pane>` under `env`: (exit code, stdout).
fn info_json(g: &EnvHeld, env: &[(&'static str, String)], pane: &str) -> (Option<i32>, Vec<u8>) {
    // the built binary starts here before any `set`: the same check first (#1066)
    fake::checked_state_held(g);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["info", "--json", pane])
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .output()
        .unwrap();
    (out.status.code(), out.stdout)
}

/// Rows: the transcript's cwd over the pane's; `@cockpit_dir` over both; a
/// dangling transcript cwd falls to the pane's. `root` follows the work dir (#500).
#[test]
fn the_work_dir_follows_the_board_order_and_root_follows_it() {
    let g = fake::env_lock();
    let t = Tmp::new("order");
    let (wt, opt, cwd) = (t.sub("wt"), t.sub("opt"), t.sub("cwd"));
    // wt its own repo, so a root off the cwd would be the checkout's, not wt
    let git = std::process::Command::new("git")
        .args(["init", "-q", &wt])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status();
    assert!(git.unwrap().success());
    let x = facts(&g, &t, &cwd, "", &wt);
    assert_eq!((x.work.as_str(), x.work_from), (wt.as_str(), WorkFrom::Transcript));
    // #500: root follows the work dir, as `prefix r` does once it reads `workdir`
    assert_eq!(x.root, git_root(&wt), "root follows the work dir");
    assert_ne!(x.root, git_root(&cwd));
    let x = facts(&g, &t, &cwd, &opt, &wt);
    assert_eq!((x.work.as_str(), x.work_from), (opt.as_str(), WorkFrom::Opt));
    let x = facts(&g, &t, &cwd, "", "/no/such/wt");
    assert_eq!((x.work.as_str(), x.work_from), (cwd.as_str(), WorkFrom::Cwd));
}

/// The orchestrator's tmux-review will read `info --json %N`'s `dir`: the dir the
/// board's stat used for the same pane, for each of the three sources.
#[test]
fn info_json_dir_is_the_boards_work_dir() {
    let g = fake::env_lock();
    let t = Tmp::new("json");
    let (wt, opt, cwd) = (t.sub("wt"), t.sub("opt"), t.sub("cwd"));
    for (o, s, want) in [("", &wt, &wt), (opt.as_str(), &wt, &opt), ("", &"/no/such/wt".to_string(), &cwd)] {
        let env = rig(&t, &cwd, o, s);
        let (code, stdout) = info_json(&g, &env, "%1");
        let v: serde_json::Value = serde_json::from_slice(&stdout).unwrap_or_default();
        set(&g, &t, &env);
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
    let g = fake::env_lock();
    let t = Tmp::new("gone");
    let env = rig(&t, &t.sub("cwd"), "", "");
    std::fs::write(t.0.join("display"), "").unwrap();
    assert_eq!(info_json(&g, &env, "%9"), (Some(1), vec![]));
}

/// #460: gather reads `@cockpit_job_name` and `@cockpit_job`, the name cleaned
/// (a pane title); with neither, no job.
#[test]
fn gather_reads_the_bound_job_cleaned() {
    let g = fake::env_lock();
    let t = Tmp::new("job");
    let cwd = t.sub("cwd");
    set(&g, &t, &rig(&t, &cwd, "", ""));
    let plain = std::fs::read_to_string(t.0.join("display")).unwrap();
    std::fs::write(t.0.join("display"), format!("{plain}\u{1f}bg_7\u{1f}swe\u{202e}ep\u{7}")).unwrap();
    assert_eq!(gather("%1").unwrap().job, Some(("sweep".to_string(), "bg_7".to_string())));
    std::fs::write(t.0.join("display"), format!("{plain}\u{1f}\u{1f}")).unwrap();
    assert_eq!(gather("%1").unwrap().job, None);
    // a name with no id is no bind (agent-poll sets both at once): never `(@cockpit_job )`
    std::fs::write(t.0.join("display"), format!("{plain}\u{1f}\u{1f}lone")).unwrap();
    assert_eq!(gather("%1").unwrap().job, None);
}

/// #1066: started by hand with a caller's XDG_CACHE_HOME kept, this binary's rig
/// refuses before a lookup can read or write the agents cache there.
#[test]
fn a_kept_agents_cache_is_refused() {
    let kept = [("XDG_CACHE_HOME", "/canary-1066/cache")];
    let (code, said) = fake::run_by_hand("the_work_dir_follows_the_board_order_and_root_follows_it", &kept);
    let refused = said.contains("the caller's state refused (#1066): XDG_CACHE_HOME set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}

/// #1066: started by hand with a caller's XDG_STATE_HOME kept, this binary's rig
/// refuses before it can read the review folder there.
#[test]
fn a_kept_state_folder_is_refused() {
    let kept = [("XDG_STATE_HOME", "/canary-1066/state")];
    let (code, said) = fake::run_by_hand("the_work_dir_follows_the_board_order_and_root_follows_it", &kept);
    let refused = said.contains("the caller's state refused (#1066): XDG_STATE_HOME set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}

/// #1066: `info_json` starts the built binary with the rig's HOME before any `set`, so it
/// checks too: started by hand with a caller's XDG_CACHE_HOME kept, that row is refused.
#[test]
fn a_kept_cache_is_refused_before_the_built_binary_starts() {
    let kept = [("XDG_CACHE_HOME", "/canary-1066/cache")];
    let (code, said) = fake::run_by_hand("info_json_for_no_such_pane_is_exit_1_and_silent", &kept);
    let refused = said.contains("the caller's state refused (#1066): XDG_CACHE_HOME set. ");
    assert_eq!((code, refused), (Some(101), true), "{said}");
}
