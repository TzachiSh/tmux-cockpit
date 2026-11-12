//! `@cockpit_subs` and `@cockpit_transcript` ride on the one `list-panes` (#313): field 14 is
//! the hook's running count, field 15 names the session whose `subagents/` dir fills
//! `Pane.subs`. Its own test binary, as `forks_dirs.rs` is: `list_panes` keeps thread-local
//! caches and reads the env.
mod fake;

use cockpit_tui::data::{Pane, list_panes};
use fake::write_exec;
use std::path::Path;

fn sub(dir: &Path, id: &str, desc: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let m = format!(r#"{{"agentType":"Explore","description":"{desc}"}}"#);
    std::fs::write(dir.join(format!("agent-{id}.meta.json")), m).unwrap();
}

fn descs(p: &Pane) -> Vec<String> {
    p.subs.iter().map(|s| s.desc.clone()).collect()
}

/// Removes the fixture when the test ends, pass or fail.
struct Gone(std::path::PathBuf);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn field_14_is_the_count_and_the_subagents_dir_follows_the_transcript_then_the_session() {
    let dir = std::env::temp_dir().join(format!("cockpit-tui-subs-fields-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _gone = Gone(dir.clone());
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let d = dir.display().to_string();
    let pj = dir.join(".claude/projects");
    // %1 the transcript option; %2 an older 13-field row, by session; %3 a count that is no
    // number; %4 a transcript outside the tree, so by session; %5 a session id that climbs.
    sub(&pj.join("-p/s1/subagents"), "a", "by transcript");
    std::fs::write(pj.join("-p/s1.jsonl"), "").unwrap();
    sub(&pj.join("-w-two/s2/subagents"), "a", "two");
    sub(&pj.join("-w-two/s2/subagents"), "b", "two more");
    sub(&dir.join("out/s4/subagents"), "a", "outside");
    std::fs::write(dir.join("out/s4.jsonl"), "").unwrap();
    sub(&pj.join("-w-four/s4/subagents"), "a", "by session");
    std::fs::create_dir_all(pj.join("-w-five")).unwrap();
    sub(&pj.join("x/subagents"), "a", "climbed");
    let row = |id: &str, sess: &str, cwd: &str, tail: &[&str]| {
        let head = [id, "w:0", "win", "claude", "running", "-", "-", sess, cwd, "100", "-", "-"];
        let mut f = head.to_vec();
        f.push("@1");
        f.extend(tail);
        f.join("\u{1f}")
    };
    let t1 = pj.join("-p/s1.jsonl").display().to_string();
    let t4 = dir.join("out/s4.jsonl").display().to_string();
    let rows = [
        row("%1", "s1", "/w/one", &["", "3", &t1]),
        row("%2", "s2", "/w/two", &[]),
        row("%3", "s3", "/w/three", &["", "x"]),
        row("%4", "s4", "/w/four", &["", "1", &t4]),
        row("%5", "../x", "/w/five", &["", "", ""]),
    ]
    .join("\n");
    std::fs::write(dir.join("rows"), rows + "\n").unwrap();
    write_exec(
        &dir.join("bin/tmux"),
        &format!("#!/bin/sh\ncase \"$1\" in list-panes) cat {d}/rows ;; esac\n"),
    );
    write_exec(&dir.join("bin/git"), "#!/bin/sh\nexit 1\n");
    write_exec(&dir.join("bin/ps"), "#!/bin/sh\n");
    let path = fake::rig_path(&dir.join("bin"));
    // SAFETY: this test binary holds exactly one test, so nothing runs beside it.
    unsafe {
        std::env::set_var("PATH", path);
        std::env::set_var("COCKPIT_TMUX", format!("{d}/bin/tmux"));
        std::env::set_var("HOME", &d);
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::env::set_var("COCKPIT_READONLY", "1");
        // #505: no proc under it, so the walk asks the fake ps, not the live pid of the same number
        std::env::set_var("COCKPIT_PROC_DIR", format!("{d}/proc"));
    }
    fake::assert_claude_is_the_rigs(&dir);

    let mut ps = list_panes().unwrap();
    ps.sort_by(|a, b| a.id.cmp(&b.id));
    let of = |p: &Pane| (p.id.clone(), p.subs_n, descs(p));
    let got: Vec<(String, u32, Vec<String>)> = ps.iter().map(of).collect();
    let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let want = vec![
        ("%1".to_string(), 3, v(&["by transcript"])),
        ("%2".to_string(), 0, v(&["two", "two more"])),
        ("%3".to_string(), 0, v(&[])),
        ("%4".to_string(), 1, v(&["by session"])),
        ("%5".to_string(), 0, v(&[])),
    ];
    assert_eq!(got, want);
}
