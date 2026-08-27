//! The data layer's caches, classifier and pane reads, driven through PATH
//! shims: no real tmux, no real ps, no clock assertion and no sleep. Every
//! fixture is a literal, so a failure names one behaviour and not the box.
use cockpit_tui::data::{Pane, list_panes, preview};
use std::fs::{FileTimes, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// PATH, HOME and COCKPIT_* are process-global while cargo runs tests on
/// threads, so every rig holds this for its lifetime and puts the vars back.
static ENV: Mutex<()> = Mutex::new(());

struct Env(Vec<(&'static str, Option<String>)>);

impl Env {
    fn set(vars: &[(&'static str, String)]) -> Self {
        let old = vars.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect();
        for (k, v) in vars {
            // SAFETY: the ENV mutex is held for the whole lifetime of this guard.
            unsafe { std::env::set_var(k, v) };
        }
        Env(old)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for (k, v) in &self.0 {
            // SAFETY: as above.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}

fn write_exec(p: &Path, body: &str) {
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

pub struct Rig {
    dir: PathBuf,
    _env: Env,
    _lock: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("cockpit-tui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let d = dir.display().to_string();
        // capture-pane with no fixture is the dead-pane case: exit 1, no stdout.
        write_exec(
            &dir.join("bin/tmux"),
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n\
                 list-panes) cat {d}/rows ;;\n\
                 show-option) printf '|claude|codex|aider|cursor|opencode|gemini|crush|' ;;\n\
                 capture-pane) [ -f {d}/cap ] || exit 1; cat {d}/cap ;;\n\
                 esac\n"
            ),
        );
        write_exec(&dir.join("bin/ps"), &format!("#!/bin/sh\ncat {d}/ps\n"));
        write_exec(&dir.join("bin/claude"), &format!("#!/bin/sh\necho 1 >> {d}/forks\nprintf '[]'\n"));
        // git fails: loc and diff are another lane's business, and a real repo
        // under $TMPDIR would make these rows depend on the box.
        write_exec(&dir.join("bin/git"), "#!/bin/sh\nexit 1\n");
        std::fs::write(dir.join("rows"), "").unwrap();
        std::fs::write(dir.join("ps"), "").unwrap();
        let env = Env::set(&[
            ("PATH", format!("{d}/bin:{}", std::env::var("PATH").unwrap_or_default())),
            ("COCKPIT_TMUX", format!("{d}/bin/tmux")),
            ("HOME", d),
            ("COCKPIT_READONLY", "1".into()),
        ]);
        Rig {
            dir,
            _env: env,
            _lock: lock,
        }
    }

    fn rows(&self, rows: &[String]) {
        std::fs::write(self.dir.join("rows"), rows.join("\n") + "\n").unwrap();
    }

    fn ps(&self, table: &str) {
        std::fs::write(self.dir.join("ps"), table).unwrap();
    }

    /// `$HOME/.claude/projects/<slug>/<sess>.jsonl`; the slug of `/w` is `-w`.
    fn transcript(&self, sess: &str, body: &str) -> PathBuf {
        self.transcript_in("-w", sess, body)
    }

    fn transcript_in(&self, slug: &str, sess: &str, body: &str) -> PathBuf {
        let dir = self.dir.join(format!(".claude/projects/{slug}"));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{sess}.jsonl"));
        std::fs::write(&p, body).unwrap();
        p
    }

    fn capture(&self, body: &str) {
        std::fs::write(self.dir.join("cap"), body).unwrap();
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// One `list-panes -F` row: 13 `\037` fields, `-` where the option is unset.
fn row_at(id: &str, wher: &str, cmd: &str, state: &str, sess: &str, pid: &str, path: &str) -> String {
    [id, wher, "win", cmd, state, "-", "-", sess, path, pid, "-", "-", "-"].join("\u{1f}")
}

fn row(id: &str, cmd: &str, state: &str, sess: &str, pid: &str) -> String {
    row_at(id, "w:0", cmd, state, sess, pid, "/w")
}

fn get<'a>(ps: &'a [Pane], id: &str) -> &'a Pane {
    ps.iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| panic!("{id} missing from {:?}", ps.iter().map(|p| &p.id).collect::<Vec<_>>()))
}

fn ids(ps: &[Pane]) -> Vec<&str> {
    ps.iter().map(|p| p.id.as_str()).collect()
}

/// A transcript re-read after an untouched `(len, mtime)` is the 0.1 s-per-tick
/// bill the gate exists to avoid. The second line is rewritten in place at the
/// same byte length with the mtime restored, so only a re-read can see it.
#[test]
fn an_unchanged_transcript_is_not_reread() {
    let r = Rig::new("mtime");
    let p = r.transcript(
        "s1",
        "{\"type\":\"user\",\"timestamp\":\"2026-08-27T00:00:00.000Z\",\"message\":{\"content\":\"go\"}}\n\
         {\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:00:01.000Z\",\"message\":{\"stop_reason\":\"tool_use\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{\"command\":\"echo one\"}}]}}\n",
    );
    r.rows(&[row("%1", "claude", "running", "s1", "100")]);

    assert_eq!(get(&list_panes().unwrap(), "%1").doing, "Bash: echo one");

    let md = std::fs::metadata(&p).unwrap();
    let before = std::fs::read_to_string(&p).unwrap();
    let poisoned = before.replace("echo one", "echo two");
    assert_eq!(poisoned.len(), before.len(), "the poison must not move the length");
    std::fs::write(&p, &poisoned).unwrap();
    OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_times(FileTimes::new().set_modified(md.modified().unwrap()))
        .unwrap();

    assert_eq!(
        get(&list_panes().unwrap(), "%1").doing,
        "Bash: echo one",
        "the tail was parsed again although (len, mtime) never moved"
    );
}

/// Rule 72: the classifier is the process tree. Claude renames itself to its
/// version, so `pane_current_command` reads `node` or a version string on a live
/// agent and a plain shell on one that has exited.
#[test]
fn agent_panes_come_from_the_process_tree_and_not_from_a_name() {
    let r = Rig::new("tree");
    // 200/201: a shell whose child renamed itself to its version. 202: the
    // agent exited, only the shell is left. 203: the shell of a session named
    // for claude, which minted a bogus row under the string heuristic.
    r.ps("200 1 -bash\n201 200 2_1_246\n202 1 zsh\n203 1 zsh\n");
    r.rows(&[
        row("%1", "node", "-", "s1", "200"),
        row("%2", "zsh", "-", "s2", "202"),
        row_at("%3", "henosis-claude-resume:0", "zsh", "-", "s3", "203", "/w"),
    ]);

    assert_eq!(ids(&list_panes().unwrap()), ["%1"]);
}

/// A `ps` row can name itself as its own parent (pid 1, a reparented zombie);
/// the walk queued it forever and the tick never returned.
#[test]
fn a_self_parenting_ps_row_does_not_wedge_the_walk() {
    let r = Rig::new("cycle");
    r.ps("300 300 claude\n301 300 sh\n");
    r.rows(&[row("%1", "claude", "running", "-", "300")]);

    assert_eq!(ids(&list_panes().unwrap()), ["%1"]);
}

/// Rule 73: `capture-pane` exit 1 with empty stdout is a dead pane, and a blank
/// preview would draw it as a live, idle one.
#[test]
fn a_dead_pane_is_an_error_and_not_a_blank_preview() {
    let r = Rig::new("gone");
    assert_eq!(preview("%9", 5), None);

    r.capture("one\ntwo\nthree\n");
    assert_eq!(preview("%9", 2).as_deref(), Some("two\nthree"));
}

/// Rule 73's other half: the preview cap is a byte offset into text that is
/// valid UTF-8 but not ASCII, and slicing mid-glyph panics. Every char here is
/// three bytes, so the cut lands inside one.
#[test]
fn the_preview_cap_snaps_to_a_char_boundary() {
    let r = Rig::new("snap");
    r.capture(&"€".repeat(30_000));

    let p = preview("%9", 200).unwrap();
    assert!(p.len() <= 64 * 1024, "preview not capped: {} bytes", p.len());
    assert!(p.chars().all(|c| c == '€'), "the cut split a glyph");
    assert_eq!(p.len() % 3, 0);
}

/// Rules 22, 66 and 67: what the tail says when the hooks said nothing. `doing`
/// prefers the command over the description; an AskUserQuestion is open until a
/// `tool_result` closes it; an interrupt ends the turn without closing one.
#[test]
fn the_transcript_lifecycle_drives_state_when_the_hooks_are_silent() {
    let r = Rig::new("life");
    r.ps("100 1 claude\n");
    let user =
        |t: &str, c: &str| format!("{{\"type\":\"user\",\"timestamp\":\"2026-08-27T00:00:{t}.000Z\",\"message\":{{\"content\":{c}}}}}\n");
    let ask = |t: &str, id: &str| {
        format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:00:{t}.000Z\",\"message\":{{\"stop_reason\":\"tool_use\",\
             \"content\":[{{\"type\":\"tool_use\",\"id\":\"{id}\",\"name\":\"AskUserQuestion\",\"input\":{{\"question\":\"which?\"}}}}]}}}}\n"
        )
    };
    // The real interrupt row is the array form: 36 of the 38 on this box, the
    // two string ones being /compact continuations that quote the phrase.
    let interrupt = user(
        "30",
        "[{\"type\":\"text\",\"text\":\"[Request interrupted by user for tool use]\"}]",
    );
    let interrupt_str = user("30", "\"[Request interrupted by user]\"");

    r.transcript("ask", &(user("00", "\"go\"") + &ask("01", "tu1")));
    r.transcript(
        "answered",
        &(user("00", "\"go\"") + &ask("01", "tu1") + &user("09", "[{\"type\":\"tool_result\",\"tool_use_id\":\"tu1\",\"content\":\"A\"}]")),
    );
    // The `end_turn` row is not the last row in a live file: attachment and
    // custom-title rows land after it and must not reopen the turn.
    r.transcript(
        "done",
        "{\"type\":\"assistant\",\"timestamp\":\"2020-01-01T00:00:00.000Z\",\"message\":{\"stop_reason\":\"end_turn\",\"content\":[{\"type\":\"text\",\"text\":\"all set\"}]}}\n\
         {\"type\":\"attachment\",\"timestamp\":\"2026-08-27T00:00:00.000Z\"}\n\
         {\"type\":\"custom-title\",\"timestamp\":\"2026-08-27T00:00:01.000Z\"}\n",
    );
    r.transcript(
        "stopped",
        &(user("00", "\"go\"")
            + "{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:00:01.000Z\",\"message\":{\"stop_reason\":\"tool_use\",\
               \"content\":[{\"type\":\"tool_use\",\"id\":\"t9\",\"name\":\"Bash\",\"input\":{\"description\":\"nap\",\"command\":\"sleep 100\"}}]}}\n"
            + &interrupt),
    );
    r.transcript("stopped-asking", &(user("00", "\"go\"") + &ask("01", "tu5") + &interrupt));
    r.transcript("stopped-str", &(user("00", "\"go\"") + &ask("01", "tu7") + &interrupt_str));
    // <task-notification and friends are written by the harness, not typed:
    // 48 of 278 string user rows on this box, after the turn already ended.
    r.transcript(
        "injected",
        &("{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:00:01.000Z\",\"message\":{\"stop_reason\":\"end_turn\",\
           \"content\":[{\"type\":\"text\",\"text\":\"all set\"}]}}\n"
            .to_string()
            + &user("30", "\"<task-notification>a background task finished</task-notification>\"")),
    );
    r.rows(&[
        row("%1", "node", "-", "ask", "100"),
        row("%2", "node", "-", "answered", "100"),
        row("%3", "node", "-", "done", "100"),
        row("%4", "node", "-", "stopped", "100"),
        row("%5", "node", "-", "stopped-asking", "100"),
        row("%6", "node", "running", "done", "100"),
        row("%7", "node", "-", "injected", "100"),
        row("%8", "node", "-", "stopped-str", "100"),
    ]);
    let ps = list_panes().unwrap();

    assert_eq!(get(&ps, "%1").state, "needs-input", "an unanswered AskUserQuestion");
    assert_eq!(get(&ps, "%2").state, "running", "a tool_result closed the question");
    assert_eq!(get(&ps, "%3").state, "done", "end_turn, with later non-assistant rows");
    assert_eq!(get(&ps, "%4").state, "done", "the interrupt ended the turn");
    assert_eq!(get(&ps, "%5").state, "needs-input", "the interrupt left the question open");
    assert_eq!(get(&ps, "%6").state, "running", "@panestate outranks the transcript");

    let done = get(&ps, "%3");
    assert_eq!(done.said, "all set");
    assert!(
        done.since > done.idle_since,
        "since must come off the assistant row ({:?}), not the newest row ({:?})",
        done.since,
        done.idle_since
    );

    assert_eq!(
        get(&ps, "%8").state,
        "needs-input",
        "the string interrupt form still terminates a turn"
    );

    let injected = get(&ps, "%7");
    assert_eq!(injected.state, "done", "a machine-injected row is not a new human turn");
    assert_eq!(injected.asked, "", "and it is not what the human asked");

    let stopped = get(&ps, "%4");
    assert_eq!(stopped.doing, "Bash: sleep 100", "command outranks description");
    assert_eq!(stopped.asked, "go", "the interrupt row is not a human turn");
}

/// Rule 70: the project slug maps every non-`[A-Za-z0-9-]` **byte** to `-`, not
/// every char, so a two-byte glyph in the cwd becomes two dashes. Per char the
/// path is one dash short and the session reads as having no transcript at all.
#[test]
fn a_non_ascii_cwd_finds_its_transcript() {
    let r = Rig::new("slug");
    r.transcript_in("-w--", "s1", BASH_ROW);
    r.rows(&[row_at("%1", "w:0", "claude", "running", "s1", "100", "/w\u{e9}")]);

    assert_eq!(get(&list_panes().unwrap(), "%1").doing, "Bash: echo one");
}

/// Rule 69: the window is >= 256 KB. At 64 KB a transcript busy enough to fill
/// it hides the open question behind its own padding, and the escalation never
/// fires because the small window did find *a* row -- just not that one.
#[test]
fn an_open_question_further_back_than_64k_is_still_found() {
    let r = Rig::new("window");
    r.ps("100 1 claude\n");
    let pad: String = (0..300)
        .map(|i| {
            format!(
                "{{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:01:00.000Z\",\"message\":{{\"stop_reason\":\"tool_use\",\
                 \"content\":[{{\"type\":\"tool_use\",\"id\":\"p{i}\",\"name\":\"Bash\",\"input\":{{\"command\":\"{}\"}}}}]}}}}\n",
                "x".repeat(300)
            )
        })
        .collect();
    assert!(pad.len() > 64 * 1024 && pad.len() < 256 * 1024, "padding is {} bytes", pad.len());
    let ask = "{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:00:01.000Z\",\"message\":{\"stop_reason\":\"tool_use\",\
               \"content\":[{\"type\":\"tool_use\",\"id\":\"tu1\",\"name\":\"AskUserQuestion\",\"input\":{\"question\":\"which?\"}}]}}\n";
    r.transcript("s1", &(ask.to_string() + &pad));
    r.rows(&[row("%1", "node", "-", "s1", "100")]);

    assert_eq!(get(&list_panes().unwrap(), "%1").state, "needs-input");
}

const BASH_ROW: &str = "{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:00:01.000Z\",\"message\":{\"stop_reason\":\"tool_use\",\
     \"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{\"command\":\"echo one\"}}]}}\n";
