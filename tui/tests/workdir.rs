//! #500: `cockpit-tui workdir %N` prints the board's `Pane.dir` for one pane, and
//! never asks `claude agents`. Both binaries run under one shimmed tmux, so the
//! board's `--json` and `workdir` read the same fixture.
mod fake;

use std::path::{Path, PathBuf};
use std::process::Command;

/// A scratch tree under cargo's own scratch dir, removed on drop.
struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn exec(p: &Path, body: &str) {
    fake::write_exec(p, body);
}

/// The board's FIELDS row: id, cmd, @panestate, @cockpit_session, cwd, pid, @cockpit_dir.
fn row(id: &str, cmd: &str, state: &str, sess: &str, cwd: &str, pid: &str, opt: &str) -> String {
    [id, "w:0", "win", cmd, state, "-", "-", sess, cwd, pid, "-", "-", "@1", opt, "", ""].join("\u{1f}")
}

struct Rig {
    t: Tmp,
    env: Vec<(&'static str, String)>,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("workdir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("bin")).unwrap();
        let s = d.display();
        // display-message answers for the -t pane only, as tmux does; nothing for a missing one
        exec(
            &d.join("bin/tmux"),
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n\
                 list-panes) cat '{s}/rows' ;;\n\
                 display-message) grep -a \"^$4$(printf '\\037')\" '{s}/rows' || printf '\\037%.0s' $(seq 15) ;;\n\
                 show-option) printf '|claude|codex|' ;;\n\
                 esac\n"
            ),
        );
        exec(&d.join("bin/ps"), &format!("#!/bin/sh\necho 1 >> '{s}/ps-calls'\ncat '{s}/ps'\n"));
        exec(
            &d.join("bin/claude"),
            &format!("#!/bin/sh\necho \"$*\" >> '{s}/claude-calls'\nprintf '[]'\n"),
        );
        exec(&d.join("bin/git"), "#!/bin/sh\nexit 1\n");
        std::fs::write(d.join("ps"), "").unwrap();
        let env = vec![
            ("PATH", format!("{s}/bin:{}", std::env::var("PATH").unwrap_or_default())),
            ("COCKPIT_TMUX", format!("{s}/bin/tmux")),
            ("HOME", s.to_string()),
            ("CLAUDE_CONFIG_DIR", format!("{s}/.claude")),
            ("COCKPIT_READONLY", "1".into()),
            // #505: no proc under it, so the walk asks the fake ps, not a live pid of the same number
            ("COCKPIT_PROC_DIR", format!("{s}/proc")),
            ("XDG_STATE_HOME", format!("{s}/state")),
        ];
        Rig { t: Tmp(d), env }
    }

    fn dir(&self, name: &str) -> String {
        let d = self.t.0.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d.display().to_string()
    }

    /// The session's transcript under the cwd's project slug, saying `cwd`.
    fn transcript(&self, launch: &str, sess: &str, cwd: &str) {
        let slug: String = launch
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b == b'-' { b as char } else { '-' })
            .collect();
        let p = self.t.0.join(".claude/projects").join(slug);
        std::fs::create_dir_all(&p).unwrap();
        let v = serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:00.000Z", "cwd": cwd, "message": {"content": "go"}});
        std::fs::write(p.join(format!("{sess}.jsonl")), format!("{v}\n")).unwrap();
    }

    fn run(&self, args: &[&str]) -> (Option<i32>, String) {
        let o = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
            .args(args)
            .envs(self.env.iter().map(|(k, v)| (*k, v.as_str())))
            .output()
            .unwrap();
        (o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned())
    }

    fn claude_calls(&self) -> String {
        std::fs::read_to_string(self.t.0.join("claude-calls")).unwrap_or_default()
    }
}

/// The board's `--json` `dir` for `id`.
fn board_dir(r: &Rig, id: &str) -> String {
    let (code, out) = r.run(&["--json"]);
    assert_eq!(code, Some(0));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let row = v
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == id)
        .unwrap_or_else(|| panic!("{id} not on the board"));
    row["dir"].as_str().unwrap().to_string()
}

/// Rows: a version-named claude whose transcript moved it, a pane with
/// `@cockpit_dir`, a transcript-only pane, one whose transcript is missing, and
/// one with no session at all. `workdir` prints what the board's `dir` says.
#[test]
fn workdir_is_the_boards_dir_for_every_kind_of_pane() {
    let r = Rig::new("agree");
    let (launch, wt, opt, gone) = (r.dir("main"), r.dir("wt"), r.dir("opt"), r.dir("gone-sess"));
    r.transcript(&launch, "s1", &wt);
    r.transcript(&launch, "s3", &wt);
    let rows = [
        row("%1", "2.1.246", "running", "s1", &launch, "101", ""),
        row("%2", "claude", "running", "s2", &launch, "102", &opt),
        row("%3", "claude", "-", "s3", &launch, "103", ""),
        row("%4", "claude", "running", "s4", &gone, "104", ""),
        row("%5", "claude", "running", "-", &launch, "105", ""),
    ];
    std::fs::write(r.t.0.join("rows"), rows.join("\n") + "\n").unwrap();
    let want = [("%1", &wt), ("%2", &opt), ("%3", &wt), ("%4", &gone), ("%5", &launch)];
    for (id, dir) in want {
        assert_eq!(board_dir(&r, id), *dir, "the board's {id}");
        assert_eq!(r.run(&["workdir", id]), (Some(0), format!("{dir}\n")), "workdir {id}");
    }
}

/// `claude agents --json` costs hundreds of ms: `workdir` never runs it, even
/// for a pane with no `@cockpit_session`, where the board does.
#[test]
fn workdir_never_asks_claude_agents() {
    let r = Rig::new("noclaude");
    let launch = r.dir("main");
    std::fs::write(r.t.0.join("rows"), row("%5", "claude", "running", "-", &launch, "105", "") + "\n").unwrap();
    assert_eq!(r.run(&["workdir", "%5"]), (Some(0), format!("{launch}\n")));
    assert_eq!(r.claude_calls(), "");
}

/// Exit 1 and nothing on stdout for a pane that is not there or is not an
/// agent's (the board's test: a shell with no @panestate); exit 2 for a bad id.
#[test]
fn workdir_exits_1_for_no_pane_or_no_agent_and_2_for_a_bad_id() {
    let r = Rig::new("exits");
    let launch = r.dir("main");
    std::fs::write(r.t.0.join("rows"), row("%6", "bash", "-", "-", &launch, "106", "") + "\n").unwrap();
    assert_eq!(r.run(&["workdir", "%6"]), (Some(1), String::new()), "a shell");
    let _ = std::fs::remove_file(r.t.0.join("ps-calls"));
    assert_eq!(r.run(&["workdir", "%9"]), (Some(1), String::new()), "no such pane");
    assert!(!r.t.0.join("ps-calls").exists(), "a missing pane forks no ps");
    for bad in [&["workdir"][..], &["workdir", "6"], &["workdir", "%"], &["workdir", "%6", "%7"]] {
        assert_eq!(r.run(bad), (Some(2), String::new()), "{bad:?}: the usage goes to stderr");
    }
}

/// #507: a wrapper pane the hooks marked (@panestate, a command that is not an
/// agent's) is an agent without its process tree, and the tree only decides
/// `ended`, which workdir never reads: workdir forks no `ps` for it. The board
/// still does (tests/forks_ended.rs).
#[test]
fn workdir_forks_no_ps_for_a_marked_wrapper_pane() {
    let r = Rig::new("wrapper");
    let launch = r.dir("main");
    std::fs::write(r.t.0.join("rows"), row("%7", "node", "running", "-", &launch, "107", "") + "\n").unwrap();
    std::fs::write(r.t.0.join("ps"), "107 1 node\n").unwrap();
    assert_eq!(r.run(&["workdir", "%7"]), (Some(0), format!("{launch}\n")));
    let forks = std::fs::read_to_string(r.t.0.join("ps-calls")).map_or(0, |s| s.lines().count());
    assert_eq!(forks, 0, "workdir forked ps for a pane it already knows is an agent's");
}

/// #507: a dir that is not on disk is no dir: exit 1 and nothing on stdout, not a
/// path a script would cd into. Rows: a launch dir since removed, and one whose
/// name is not UTF-8, which tmux's lossy decode turns into a path that is not there.
#[test]
fn workdir_exits_1_for_a_dir_that_is_not_on_disk() {
    use std::os::unix::ffi::OsStrExt;
    let r = Rig::new("gone");
    let gone = r.dir("gone");
    std::fs::remove_dir(&gone).unwrap();
    let mut raw = r.t.0.as_os_str().as_bytes().to_vec();
    raw.extend_from_slice(b"/bad\xffname");
    std::fs::create_dir(std::ffi::OsStr::from_bytes(&raw)).unwrap();
    let mut rows = (row("%1", "claude", "running", "-", &gone, "101", "") + "\n").into_bytes();
    let bad = row("%2", "claude", "running", "-", "@BAD@", "102", "") + "\n";
    let at = bad.find("@BAD@").unwrap();
    rows.extend_from_slice(&bad.as_bytes()[..at]);
    rows.extend_from_slice(&raw);
    rows.extend_from_slice(&bad.as_bytes()[at + 5..]);
    std::fs::write(r.t.0.join("rows"), rows).unwrap();
    assert_eq!(r.run(&["workdir", "%1"]), (Some(1), String::new()), "a removed dir");
    assert_eq!(r.run(&["workdir", "%2"]), (Some(1), String::new()), "a non-UTF-8 dir");
}
