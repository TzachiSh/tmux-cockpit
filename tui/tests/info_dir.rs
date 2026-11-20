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

/// "YYYY-MM-DDTHH:MM:SS.000Z" `ago` seconds before now, civil-from-days (Hinnant).
fn stamp(ago: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let t = now.as_secs() as i64 - ago;
    let (days, rem) = (t.div_euclid(86400), t.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// #1355: `name` under the scratch tree, made a git repo, so a tool call into it names a tree.
fn repo(t: &Tmp, name: &str) -> String {
    let d = t.sub(name);
    let git = std::process::Command::new("git")
        .args(["init", "-q", &d])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status();
    assert!(git.unwrap().success());
    d
}

/// #1355: the rig's transcript gains a main-thread Bash call into `tree`, 30 s ago.
fn call_into(t: &Tmp, cwd: &str, tree: &str) {
    use std::io::Write;
    let tr = t.0.join("home/.claude/projects").join(slug(cwd)).join("s1.jsonl");
    let call = serde_json::json!({"type": "tool_use", "id": "u1", "name": "Bash",
        "input": {"command": format!("cd {tree} && make")}});
    let v = serde_json::json!({"type": "assistant", "timestamp": stamp(30), "cwd": cwd,
        "message": {"content": [call]}});
    let mut f = std::fs::OpenOptions::new().append(true).open(tr).unwrap();
    writeln!(f, "{v}").unwrap();
}

/// #1355: `@cockpit_cwd` is `hook` on the pane: the board's row 19, and info's own row 16
/// for a pane the board does not count. The fields between are empty.
fn hook_cwd(t: &Tmp, hook: &str) {
    for (file, seps) in [("rows", 6), ("display", 4)] {
        let p = t.0.join(file);
        let row = std::fs::read_to_string(&p).unwrap();
        let row = format!("{}{}{hook}\n", row.trim_end_matches('\n'), "\u{1f}".repeat(seps));
        std::fs::write(&p, row).unwrap();
    }
}

/// #1355: the board's row is a shell's, not an agent's: no @panestate, a shell command and
/// a pid that is no process, so the agent test finds no tree either.
fn shell(t: &Tmp) {
    let p = t.0.join("rows");
    let row = std::fs::read_to_string(&p).unwrap();
    let mut f: Vec<&str> = row.trim_end_matches('\n').split('\u{1f}').collect();
    (f[3], f[4], f[9]) = ("bash", "-", "999999999");
    std::fs::write(&p, f.join("\u{1f}") + "\n").unwrap();
}

/// #1355: a pane per source `workdir --source` names, on the rig: the rig's env.
fn case(t: &Tmp, src: &str) -> Vec<(&'static str, String)> {
    let (cwd, other) = (t.sub("cwd"), t.sub("other"));
    match src {
        "active-in" => {
            let env = rig(t, &cwd, "", &cwd);
            call_into(t, &cwd, &repo(t, "wt"));
            env
        }
        "session" => {
            let env = rig(t, &cwd, "", "/no/such/wt");
            hook_cwd(t, &other);
            env
        }
        "declared" => rig(t, &cwd, &other, &cwd),
        _ => rig(t, &cwd, "", "/no/such/wt"),
    }
}

/// `cockpit-tui workdir --source <pane>` under `env`: the dir and the source, split at the
/// last tab as docs/tui.md says.
fn source(g: &EnvHeld, env: &[(&'static str, String)], pane: &str) -> (String, String) {
    fake::checked_state_held(g);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["workdir", "--source", pane])
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string();
    let (d, src) = s.rsplit_once('\t').unwrap_or_default();
    (d.to_string(), src.to_string())
}

/// #1355: the agent's fresh `cd` into a tree outranks the transcript's cwd on the info
/// screen, as on the board.
#[test]
fn an_active_in_tree_is_the_info_work_dir() {
    let g = fake::env_lock();
    let t = Tmp::new("activein");
    let (cwd, wt) = (t.sub("cwd"), repo(&t, "wt"));
    let env = rig(&t, &cwd, "", &cwd);
    call_into(&t, &cwd, &wt);
    set(&g, &t, &env);
    assert_eq!(gather("%1").unwrap().work, wt);
}

/// #1355: with no `@cockpit_dir`, the hooks' `@cockpit_cwd` is the info work dir, before
/// the pane's launch dir.
#[test]
fn a_cockpit_cwd_alone_is_the_info_work_dir() {
    let g = fake::env_lock();
    let t = Tmp::new("hookcwd");
    let (cwd, hook) = (t.sub("cwd"), t.sub("hook"));
    let env = rig(&t, &cwd, "", "/no/such/wt");
    hook_cwd(&t, &hook);
    set(&g, &t, &env);
    assert_eq!(gather("%1").unwrap().work, hook);
}

/// #1355: a shell pane has no transcript, but its `@cockpit_cwd` still names its folder,
/// as goto reads it.
#[test]
fn a_shell_panes_cockpit_cwd_is_its_info_work_dir() {
    let g = fake::env_lock();
    let t = Tmp::new("shellcwd");
    let (cwd, hook) = (t.sub("cwd"), t.sub("hook"));
    let env = rig(&t, &cwd, "", "");
    hook_cwd(&t, &hook);
    shell(&t);
    set(&g, &t, &env);
    let x = gather("%1").unwrap();
    assert_eq!((x.agent, x.work), (false, hook));
}

/// #1355: the screen's work row is `<dir>  (<source>)`, the pair `workdir --source` prints
/// for the same pane. Rows: one per source.
#[test]
fn the_screens_dir_row_is_what_workdir_source_answers() {
    let g = fake::env_lock();
    for src in ["active-in", "session", "declared", "pane-cwd"] {
        let t = Tmp::new(&format!("row-{src}"));
        let env = case(&t, src);
        let (d, s) = source(&g, &env, "%1");
        set(&g, &t, &env);
        let glyphs = &cockpit_tui::theme::Theme::fixed().g;
        let r = cockpit_tui::info::rows(&gather("%1").unwrap(), glyphs, 400);
        let want = ("dir".to_string(), format!("{d}  ({s})"));
        assert_eq!((s.as_str(), r.contains(&want)), (src, true), "{src}: {r:?}");
    }
}

/// #1355: `info --json`'s `source` is the one `workdir --source` prints, next to the same
/// `dir`. Rows: one per source.
#[test]
fn info_json_source_is_what_workdir_source_answers() {
    let g = fake::env_lock();
    for src in ["active-in", "session", "declared", "pane-cwd"] {
        let t = Tmp::new(&format!("json-{src}"));
        let env = case(&t, src);
        let (d, s) = source(&g, &env, "%1");
        let (code, stdout) = info_json(&g, &env, "%1");
        let v: serde_json::Value = serde_json::from_slice(&stdout).unwrap_or_default();
        let got = (code, v["dir"].as_str(), v["source"].as_str());
        assert_eq!((s.as_str(), got), (src, (Some(0), Some(d.as_str()), Some(src))), "{src}");
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

/// #448: a viewer pane bound to a background job has no @cockpit_session or
/// @cockpit_transcript; info finds the job's transcript, under the job's own project
/// dir, by @cockpit_job_session, as the board does, and works in its cwd.
#[test]
fn a_bound_viewer_reads_its_jobs_transcript_by_the_job_session() {
    let g = fake::env_lock();
    let t = Tmp::new("jobsess");
    let (wt, cwd) = (t.sub("wt"), t.sub("cwd"));
    let env = rig(&t, &cwd, "", &wt);
    let projects = t.0.join("home/.claude/projects");
    std::fs::create_dir_all(projects.join("-job")).unwrap();
    let from = projects.join(slug(&cwd)).join("s1.jsonl");
    std::fs::rename(from, projects.join("-job/sjob.jsonl")).unwrap();
    let d = t.0.display();
    let sh = format!(
        "#!/bin/sh\ncase \"$1\" in\nlist-panes) cat '{d}/rows' ;;\n\
         display-message) case \"$5\" in *cockpit_tui_version*) cat '{d}/display' ;; \
         *) cat '{d}/rows' ;; esac ;;\n\
         show-option) case \"$*\" in *@cockpit_job_session*) echo sjob ;; \
         *@cockpit_agents_glob*) echo '|claude|' ;; esac ;;\nesac\n"
    );
    fake::write_exec(&t.0.join("tmux"), &sh);
    set(&g, &t, &env);
    let x = gather("%1").unwrap();
    assert_eq!((x.work.as_str(), x.work_from), (wt.as_str(), WorkFrom::Transcript));
}

/// #1267: the rig's fake tmux, answering @cockpit_session with `own` and
/// @cockpit_job_session with `job`, and no @cockpit_transcript.
fn session_shim(t: &Tmp, own: &str, job: &str) {
    let d = t.0.display();
    let sh = format!(
        "#!/bin/sh\ncase \"$1\" in\nlist-panes) cat '{d}/rows' ;;\n\
         display-message) case \"$5\" in *cockpit_tui_version*) cat '{d}/display' ;; \
         *) cat '{d}/rows' ;; esac ;;\n\
         show-option) case \"$*\" in *@cockpit_job_session*) echo '{job}' ;; \
         *@cockpit_session*) echo '{own}' ;; \
         *@cockpit_agents_glob*) echo '|claude|' ;; esac ;;\nesac\n"
    );
    fake::write_exec(&t.0.join("tmux"), &sh);
}

/// #1267: a pane sent to the background keeps its own @cockpit_session and gains
/// its job's; info reads the pane's own transcript first, as the board does.
#[test]
fn the_panes_own_session_outranks_the_job_session_on_the_info_screen() {
    let g = fake::env_lock();
    let t = Tmp::new("ownrank");
    let (own, job, cwd) = (t.sub("own"), t.sub("job"), t.sub("cwd"));
    let env = rig(&t, &cwd, "", &own);
    let projects = t.0.join("home/.claude/projects");
    std::fs::create_dir_all(projects.join("-job")).unwrap();
    let row = serde_json::json!({"type": "user", "cwd": job, "message": {}});
    std::fs::write(projects.join("-job/sjob.jsonl"), format!("{row}\n")).unwrap();
    session_shim(&t, "s1", "sjob");
    set(&g, &t, &env);
    assert_eq!(gather("%1").unwrap().work, own);
}

/// #1267: the info screen's session is the one its transcript came from, as
/// `--json`'s is: the pane's own, else its bound job's. Rows: own, job only.
#[test]
fn the_info_session_is_the_panes_own_else_its_jobs() {
    let g = fake::env_lock();
    let t = Tmp::new("infosid");
    let cwd = t.sub("cwd");
    set(&g, &t, &rig(&t, &cwd, "", ""));
    let plain = std::fs::read_to_string(t.0.join("display")).unwrap();
    let mut f: Vec<&str> = plain.split('\u{1f}').collect();
    f.extend(["bg_7", "alpha", "sjob"]);
    std::fs::write(t.0.join("display"), f.join("\u{1f}")).unwrap();
    let own = gather("%1").unwrap().sid;
    f[9] = "";
    std::fs::write(t.0.join("display"), f.join("\u{1f}")).unwrap();
    assert_eq!((own, gather("%1").unwrap().sid), ("s1".to_string(), "sjob".to_string()));
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
