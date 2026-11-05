//! #500: one transcript lookup. The board's `--json`, `workdir %N` and `info --json %N`
//! find the same transcript for a pane, whichever step finds it, so they name the same
//! dir. All three run as binaries under one shimmed tmux that answers each format.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Rig {
    d: PathBuf,
    env: Vec<(&'static str, String)>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.d);
    }
}

fn exec(p: &Path, body: &str) {
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn slug(p: &str) -> String {
    p.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' { b as char } else { '-' })
        .collect()
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("locate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("bin")).unwrap();
        std::fs::create_dir_all(d.join("opts")).unwrap();
        let s = d.display();
        // display-message: info's format (it names @cockpit_tui_version) gets the pane's
        // info line, the board's gets its FIELDS row; show-option -t P NAME reads opts/P.NAME
        exec(
            &d.join("bin/tmux"),
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n\
                 list-panes) cat '{s}/rows' ;;\n\
                 display-message) case \"$5\" in *cockpit_tui_version*) cat \"{s}/info$4\" ;;\n\
                   *) grep \"^$4$(printf '\\037')\" '{s}/rows' ;; esac ;;\n\
                 show-option) case \"$3\" in -t) cat \"{s}/opts/$4.$5\" 2>/dev/null ;;\n\
                   *) printf '|claude|' ;; esac ;;\n\
                 esac\n"
            ),
        );
        exec(
            &d.join("bin/claude"),
            &format!("#!/bin/sh\necho \"$*\" >> '{s}/claude-calls'\nprintf '[]'\n"),
        );
        exec(&d.join("bin/git"), "#!/bin/sh\nexit 1\n");
        exec(&d.join("bin/ps"), "#!/bin/sh\n");
        std::fs::write(d.join("rows"), "").unwrap();
        let env = vec![
            ("PATH", format!("{s}/bin:{}", std::env::var("PATH").unwrap_or_default())),
            ("COCKPIT_TMUX", format!("{s}/bin/tmux")),
            ("HOME", s.to_string()),
            ("COCKPIT_READONLY", "1".into()),
            ("XDG_STATE_HOME", format!("{s}/state")),
            // #505: the fake `ps` is the process table, so no live /proc pid leaks in
            ("COCKPIT_PROC_DIR", format!("{s}/proc")),
        ];
        Rig { d, env }
    }

    fn dir(&self, name: &str) -> String {
        let p = self.d.join(name);
        std::fs::create_dir_all(&p).unwrap();
        p.display().to_string()
    }

    /// A transcript at `<projects>/<project>/<sid>.jsonl` whose one row says `cwd`.
    fn transcript(&self, project: &str, sid: &str, cwd: &str) -> String {
        let p = self.d.join(".claude/projects").join(project);
        std::fs::create_dir_all(&p).unwrap();
        let v = serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:00.000Z", "cwd": cwd, "message": {"content": "go"}});
        let f = p.join(format!("{sid}.jsonl"));
        std::fs::write(&f, format!("{v}\n")).unwrap();
        f.display().to_string()
    }

    /// Pane `id` runs claude in `cwd` with session `sid`, @cockpit_transcript `opt`.
    fn pane(&self, id: &str, sid: &str, cwd: &str, opt: &str) {
        let row = [
            id, "w:0", "win", "claude", "running", "-", "-", sid, cwd, "100", "-", "-", "@1", "", "", opt,
        ];
        let mut rows = std::fs::read_to_string(self.d.join("rows")).unwrap();
        rows += &(row.join("\u{1f}") + "\n");
        std::fs::write(self.d.join("rows"), rows).unwrap();
        let info = [id, "w:0 win", "claude", cwd, "", "running", "", "", "", sid, "", "1", ""];
        std::fs::write(self.d.join(format!("info{id}")), info.join("\u{1f}")).unwrap();
        std::fs::write(self.d.join(format!("opts/{id}.@cockpit_transcript")), opt).unwrap();
        std::fs::write(self.d.join(format!("opts/{id}.@cockpit_session")), sid).unwrap();
    }

    fn run(&self, args: &[&str]) -> String {
        let o = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
            .args(args)
            .envs(self.env.iter().map(|(k, v)| (*k, v.as_str())))
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    /// (the board's `--json` dir, `workdir`, `info --json`'s dir) for `id`.
    fn three(&self, id: &str) -> (String, String, String) {
        let v: serde_json::Value = serde_json::from_str(&self.run(&["--json"])).unwrap();
        let board = v.as_array().unwrap().iter().find(|o| o["id"] == id).unwrap()["dir"]
            .as_str()
            .unwrap()
            .to_string();
        let work = self.run(&["workdir", id]).trim_end_matches('\n').to_string();
        let info: serde_json::Value = serde_json::from_str(&self.run(&["info", "--json", id])).unwrap();
        (board, work, info["dir"].as_str().unwrap().to_string())
    }
}

/// Rows: found by @cockpit_transcript in another project dir; by the cwd's slug; by
/// the glob over every project dir; and an @cockpit_transcript outside the projects
/// tree, which never counts, so the glob finds the real one.
#[test]
fn the_board_workdir_and_info_find_the_same_transcript() {
    let r = Rig::new("agree");
    let (launch, out) = (r.dir("main"), r.dir("out"));
    let (w1, w2, w3, w4, evil) = (r.dir("w1"), r.dir("w2"), r.dir("w3"), r.dir("w4"), r.dir("evil"));
    // two projects hold s1: the glob would take -a, the option names -z
    r.transcript("-a", "s1", &r.dir("decoy"));
    let t1 = r.transcript("-z", "s1", &w1);
    r.transcript(&slug(&launch), "s2", &w2);
    r.transcript("-other", "s3", &w3);
    r.transcript("-other", "s4", &w4);
    let v = serde_json::json!({"type": "user", "cwd": evil, "message": {}});
    std::fs::write(format!("{out}/s4.jsonl"), format!("{v}\n")).unwrap();
    r.pane("%1", "s1", &launch, &t1);
    r.pane("%2", "s2", &launch, "");
    r.pane("%3", "s3", &launch, "");
    r.pane("%4", "s4", &launch, &format!("{out}/s4.jsonl"));
    for (id, want) in [("%1", &w1), ("%2", &w2), ("%3", &w3), ("%4", &w4)] {
        assert_eq!(r.three(id), (want.clone(), want.clone(), want.clone()), "{id}");
    }
    assert_eq!(std::fs::read_to_string(r.d.join("claude-calls")).unwrap_or_default(), "");
}
