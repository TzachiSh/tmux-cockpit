//! The transcript reader against outputs frozen from the jq reader it
//! replaced, plus #193's cases. Seconds are normalised to N.
use cockpit_tui::data::transcript::{doing, file, tail};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

mod fake;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/transcript-reader");

/// The third field of doing and asked rows is a clock reading: "N" when numeric.
fn norm(s: &str) -> String {
    s.lines()
        .map(|l| {
            let mut f: Vec<&str> = l.split('\t').collect();
            if matches!(f[0], "doing" | "asked") && f.len() > 2 && !f[2].is_empty() && f[2].bytes().all(|b| b.is_ascii_digit()) {
                f[2] = "N";
            }
            f.join("\t") + "\n"
        })
        .collect()
}

#[test]
fn every_fixture_matches_the_frozen_jq_output() {
    let mut n = 0;
    for e in std::fs::read_dir(FIX).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "jsonl") {
            continue;
        }
        let want = |ext: &str| std::fs::read_to_string(p.with_extension(ext)).unwrap();
        assert_eq!(norm(&doing(&p).unwrap_or_default()), want("doing"), "doing {}", p.display());
        assert_eq!(tail(&p, 4096).unwrap_or_default(), want("tail"), "tail {}", p.display());
        n += 1;
    }
    assert!(n >= 9, "only {n} fixtures found under {FIX}");
}

/// A scratch file under the temp dir, removed on drop.
struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str, body: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!("cockpit-tr-{tag}-{}.jsonl", std::process::id()));
        std::fs::write(&p, body).unwrap();
        Tmp(p)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

const PROMPT: &str = r#"{"type":"user","version":"2.1.316","timestamp":"2026-01-01T10:00:00Z","message":{"content":"real prompt"}}"#;

#[test]
fn a_message_that_is_a_string_is_skipped_not_fatal() {
    let t = Tmp::new(
        "strmsg",
        &format!("{PROMPT}\n{}\n", r#"{"type":"assistant","version":"2.1.316","message":"str"}"#),
    );
    assert_eq!(norm(&doing(&t.0).unwrap()), "asked\treal prompt\tN\n");
    assert_eq!(tail(&t.0, 4096).unwrap(), "USER: real prompt\n");
}

#[test]
fn bash_mode_prompts_are_harness_injected() {
    for tag in [
        "<bash-input>ls</bash-input>",
        "<bash-stdout>x</bash-stdout>",
        "<bash-stderr>e</bash-stderr>",
    ] {
        let line = serde_json::json!({"type": "user", "version": "2.1.316", "message": {"content": tag}});
        let t = Tmp::new("bash", &format!("{PROMPT}\n{line}\n"));
        assert_eq!(norm(&doing(&t.0).unwrap()), "asked\treal prompt\tN\n", "{tag}");
        assert_eq!(tail(&t.0, 4096).unwrap(), "USER: real prompt\n", "{tag}");
    }
}

#[test]
fn only_the_last_parallel_tool_use_is_reported() {
    let open = Path::new(FIX).join("parallel-open.jsonl");
    assert!(doing(&open).unwrap().starts_with("doing\tBash: second parallel\t"));
    // The first is still open, but the last one closed: no doing row at all.
    let closed = Path::new(FIX).join("parallel-closed.jsonl");
    assert!(!doing(&closed).unwrap().contains("doing\t"));
}

#[test]
fn the_tail_cap_drops_whole_lines_from_the_top() {
    let open = Path::new(FIX).join("open.jsonl");
    assert_eq!(tail(&open, 40).unwrap(), "TOOL Bash: make test\n");
    assert_eq!(
        tail(&open, 60).unwrap(),
        "AGENT: Looking at the widget now.\nTOOL Bash: make test\n"
    );
    assert_eq!(tail(&open, 5), None);
}

#[test]
fn every_failure_is_none() {
    let open = format!("{FIX}/open.jsonl");
    let old = PathBuf::from(format!("{FIX}/old.jsonl"));
    assert_eq!(doing(&old), None);
    assert_eq!(tail(&old, 4096), None);
    assert_eq!(tail(Path::new(&open), 5), None);
    assert_eq!(file("", ""), None);
    let missing = Path::new("/nonexistent/t.jsonl");
    let empty = Tmp::new("empty", "");
    let junk = Tmp::new("junk", "this is not json\n{oops\n");
    for p in [missing, empty.0.as_path(), junk.0.as_path()] {
        assert_eq!(doing(p), None, "doing {}", p.display());
        assert_eq!(tail(p, 4096), None, "tail {}", p.display());
    }
}

/// HOME, PATH and COCKPIT_TMUX are process-global while tests run on threads.
static ENV: Mutex<()> = Mutex::new(());

/// A HOME with two project dirs and a tmux shim whose @cockpit_session is `sid`.
struct Rig {
    dir: PathBuf,
    old: Vec<(&'static str, Option<String>)>,
    _lock: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(tag: &str, sid: &str, pane_pid: u32) -> Rig {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("cockpit-tr-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for p in ["a-proj", "b-proj", "bin"] {
            std::fs::create_dir_all(dir.join(".claude/projects").join(p)).unwrap();
        }
        let bin = dir.join(".claude/projects/bin");
        let exec = |name: &str, body: String| fake::write_exec(&bin.join(name), &body);
        exec(
            "tmux",
            format!("#!/bin/sh\ncase \"$1\" in show-option) printf '%s\\n' '{sid}' ;; display-message) echo {pane_pid} ;; esac\n"),
        );
        exec(
            "claude",
            format!(
                "#!/bin/sh\necho '[{{\"pid\":1,\"sessionId\":\"sid-aaa\"}},{{\"pid\":{},\"sessionId\":\"sid-bbb\"}}]'\n",
                std::process::id()
            ),
        );
        let b = bin.display().to_string();
        let vars = [
            ("HOME", dir.display().to_string()),
            ("COCKPIT_TMUX", format!("{b}/tmux")),
            ("PATH", format!("{b}:{}", std::env::var("PATH").unwrap_or_default())),
        ];
        let old = vars.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect();
        for (k, v) in vars {
            // SAFETY: the ENV mutex is held for the whole lifetime of this rig.
            unsafe { std::env::set_var(k, v) };
        }
        Rig { dir, old, _lock: lock }
    }

    fn session(&self, proj: &str, sid: &str) -> PathBuf {
        let p = self.dir.join(".claude/projects").join(proj).join(format!("{sid}.jsonl"));
        std::fs::write(&p, "").unwrap();
        p
    }
}

impl Drop for Rig {
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

#[test]
fn file_takes_cockpit_session_and_the_first_project_holding_it() {
    let r = Rig::new("opt", "sid-aaa", 0);
    assert_eq!(file("%1", ""), None, "no transcript file yet");
    let want = r.session("a-proj", "sid-aaa");
    r.session("b-proj", "sid-aaa");
    assert_eq!(file("%1", ""), Some(want));
}

#[test]
fn file_refuses_a_session_id_that_is_a_glob_or_a_path() {
    for sid in ["sid-*", "../outside", "a b"] {
        let r = Rig::new("glob", sid, 0);
        r.session("a-proj", "sid-aaa");
        std::fs::write(r.dir.join(".claude/projects/outside.jsonl"), "").unwrap();
        assert_eq!(file("%1", ""), None, "{sid}");
    }
}

#[test]
fn file_joins_claude_agents_on_the_pane_process_tree() {
    // The pane's pid is this test's parent, so only our own pid descends from it.
    let r = Rig::new("agents", "", std::os::unix::process::parent_id());
    let want = r.session("b-proj", "sid-bbb");
    r.session("a-proj", "sid-aaa");
    assert_eq!(file("%1", ""), Some(want));
}

#[test]
fn a_blank_cockpit_tmux_is_none_not_a_panic() {
    let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let old = std::env::var_os("COCKPIT_TMUX");
    for v in ["", " "] {
        unsafe { std::env::set_var("COCKPIT_TMUX", v) };
        assert_eq!(file("%1", ""), None, "COCKPIT_TMUX={v:?}");
    }
    unsafe {
        match old {
            Some(o) => std::env::set_var("COCKPIT_TMUX", o),
            None => std::env::remove_var("COCKPIT_TMUX"),
        }
    }
}

/// #452: the newest main-thread row's cwd; a sidechain's, an empty or a
/// non-string one never counts, and a transcript with none is None.
#[test]
fn cwd_is_the_newest_main_thread_cwd() {
    let row = |cwd: serde_json::Value, side: bool| {
        format!(
            "{}\n",
            serde_json::json!({"type": "user", "cwd": cwd, "isSidechain": side, "message": {}})
        )
    };
    let s = |c: &str| serde_json::Value::from(c);
    let agent = format!(
        "{}\n",
        serde_json::json!({"type": "user", "cwd": "/agent", "agentId": "a1", "message": {}})
    );
    let body =
        row(s("/old"), false) + &row(s("/new"), false) + &row(s("/side"), true) + &agent + &row(s(""), false) + &row(7.into(), false);
    let t = Tmp::new("cwd", &body);
    assert_eq!(cockpit_tui::data::transcript::cwd(&t.0).as_deref(), Some("/new"));
    let none = Tmp::new("cwdnone", &row(s(""), false));
    assert_eq!(cockpit_tui::data::transcript::cwd(&none.0), None);
}

/// #453: an AskUserQuestion, `id`, asking `question` under `header`.
fn ask(id: &str, question: &str, header: &str) -> String {
    let v = serde_json::json!({"type": "assistant", "version": "2.1.316", "message": {"content": [{"type": "tool_use",
        "id": id, "name": "AskUserQuestion", "input": {"questions": [{"question": question, "header": header}]}}]}});
    format!("{v}\n")
}

fn answer(id: &str) -> String {
    let v = serde_json::json!({"type": "user", "version": "2.1.316",
        "message": {"content": [{"type": "tool_result", "tool_use_id": id}]}});
    format!("{v}\n")
}

/// #453: the info screen's rows end with the open question when it is the newest
/// tool_use, cleaned; an answered one writes no row.
#[test]
fn an_open_question_is_an_ask_row_and_an_answered_one_is_not() {
    let open = Tmp::new("askopen", &ask("q1", "Merge\u{202e} now?", "Merge"));
    let done = Tmp::new("askdone", &(ask("q1", "Merge?", "Merge") + &answer("q1")));
    assert!(doing(&open.0).unwrap().ends_with("ask\tMerge now?\n"));
    assert_eq!(doing(&done.0), None, "nothing open, said or asked");
}

/// #453 review: the info screen's ask row is the newest tool_use's only. After
/// an interrupted ask, a typed prompt and a Bash, there is none; the header is
/// model-written, so `doing` drops its bidi controls, as `said` and `asked` do.
#[test]
fn the_ask_row_is_the_newest_tool_uses_and_labels_drop_bidi() {
    let int = serde_json::json!({"type": "user", "version": "2.1.316",
        "message": {"content": [{"type": "text", "text": "[Request interrupted by user for tool use]"}]}});
    let typed = serde_json::json!({"type": "user", "version": "2.1.316", "message": {"content": "do\u{202e} it"}});
    let bash = r#"{"type":"assistant","version":"2.1.316","message":{"content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"rm -rf build"}}]}}"#;
    let stale = Tmp::new("askstale", &format!("{}{int}\n{typed}\n{bash}\n", ask("q1", "Old?", "Old")));
    let out = norm(&doing(&stale.0).unwrap());
    assert_eq!(out, "doing\tBash: rm -rf build\t\nasked\tdo it\t\n", "no clock: the ages are blank");
    let moved = Tmp::new("askmoved", &format!("{}{int}\n{typed}\n", ask("q1", "Old?", "Old")));
    assert!(!doing(&moved.0).unwrap().contains("ask\t"), "a prompt typed after the ask");
    let open = Tmp::new("askbidi", &ask("q1", "Merge?", "Mer\u{202e}ge"));
    assert!(doing(&open.0).unwrap().starts_with("doing\tAskUserQuestion: Merge\t"));
}
