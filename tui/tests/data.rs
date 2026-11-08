//! The data layer's caches, classifier and pane reads, driven through PATH
//! shims: no real tmux, no real ps, no clock assertion and no sleep. Every
//! fixture is a literal, so a failure names one behaviour and not the box.
mod fake;

use cockpit_tui::data::gitsafe::{SAFE, no_filters, tree_stat};
use cockpit_tui::data::{Pane, git_facts, list_panes, preview, screen, split_cursor, view};
use fake::write_exec;
use std::fs::{FileTimes, OpenOptions};
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
            // no proc under it: the walk falls back to the fake `ps`, not the box's pids
            ("COCKPIT_PROC_DIR", format!("{d}/proc")),
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

    /// A fake proc root: `(pid, comm, children)`; the walk reads it, not `ps`.
    fn proc(&self, procs: &[(&str, &str, &str)]) {
        let root = self.dir.join("proc");
        std::fs::create_dir_all(root.join("self/task")).unwrap();
        for (pid, comm, kids) in procs {
            let d = root.join(pid);
            std::fs::create_dir_all(d.join("task").join(pid)).unwrap();
            std::fs::write(d.join("task").join(pid).join("children"), kids).unwrap();
            std::fs::write(d.join("comm"), format!("{comm}\n")).unwrap();
        }
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
    [id, wher, "win", cmd, state, "-", "-", sess, path, pid, "-", "-", "@1"].join("\u{1f}")
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

/// #492: with a proc root, the tree comes from `children` files and `ps` is not
/// asked: the fake `ps` here lists only a shell, so a verdict of "agent" can
/// only have come from the grandchild under /proc.
#[test]
fn a_pane_tree_is_walked_under_proc_and_ps_is_not_asked() {
    let r = Rig::new("procwalk");
    r.ps("");
    r.proc(&[
        ("400", "bash", "401 "),
        ("401", "sh", "402 "),
        ("402", "2.1.318", ""),
        ("410", "zsh", ""),
    ]);
    r.rows(&[row("%1", "bash", "-", "s1", "400"), row("%2", "zsh", "-", "s2", "410")]);

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

/// #284: ansi-to-tui drops the rest of a line after an OSC 8 link ending in
/// `ESC \`, the form tmux emits, so every OSC goes, whatever ends it.
#[test]
fn the_preview_drops_osc_sequences_whatever_ends_them() {
    let r = Rig::new("osc");
    r.capture("pre \x1b]8;;https://ex.com\x1b\\LINK\x1b]8;;\x1b\\ after\nb \x1b]8;;u\x07L\x1b]8;;\x07 z\n\x1b]0;title\x07t\n");
    assert_eq!(preview("%9", 5).as_deref(), Some("pre LINK after\nb L z\nt"));
}

/// #284: the 64 KB cap can land inside an escape, whose tail then reads as
/// text; a cut starts at the next line instead.
#[test]
fn a_cap_inside_an_escape_starts_at_the_next_line() {
    let r = Rig::new("cut");
    // 116 bytes a line with its newline: the cut lands 3 bytes into an escape
    let line = format!("\x1b[38;5;196m{}\x1b[0m", "x".repeat(100));
    r.capture(&vec![line.as_str(); 1000].join("\n"));
    let p = preview("%9", 1000).unwrap();
    assert!(p.len() <= 64 * 1024, "{} bytes", p.len());
    assert!(p.split('\n').all(|l| l == line), "a cut line: {:?}", &p[..24]);
}

// ---------------------------------------------- the right pane's view (#284)

/// A visible screen of `h` rows: `body` from the top, blank rows below.
fn scr(body: &[&str], h: usize) -> String {
    let mut rows: Vec<String> = body.iter().map(|s| s.to_string()).collect();
    rows.resize(h, String::new());
    rows.join("\n")
}

#[test]
fn a_short_screen_is_drawn_from_the_top_as_the_pane_shows_it() {
    // a cleared terminal: the prompt on row 0, the cursor after it
    assert_eq!(view(&scr(&["$ "], 40), Some(0), 20), "$ ");
    // a short shell: three rows used, the cursor on the next
    assert_eq!(view(&scr(&["$ ls", "a b", "$ "], 40), Some(2), 20), "$ ls\na b\n$ ");
}

#[test]
fn a_taller_screen_with_the_cursor_at_the_bottom_shows_the_bottom() {
    let rows: Vec<String> = (0..40).map(|k| format!("r{k}")).collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let v = view(&scr(&refs, 40), Some(39), 5);
    assert_eq!(v, "r35\nr36\nr37\nr38\nr39");
}

#[test]
fn a_cursor_below_the_text_in_a_taller_screen_stays_in_view() {
    // text to row 25, the cursor on row 30 past blank rows: the view ends there
    let rows: Vec<String> = (0..=25).map(|k| format!("r{k}")).collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let v = view(&scr(&refs, 40), Some(30), 10);
    let got: Vec<&str> = v.split('\n').collect();
    assert_eq!(got.len(), 10);
    assert_eq!((got[0], got[4], got[9]), ("r21", "r25", ""), "rows 21-30, the cursor's row last");
}

#[test]
fn a_hidden_cursor_ends_the_view_at_the_last_row_with_text() {
    let rows: Vec<String> = (0..=25).map(|k| format!("r{k}")).collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    assert_eq!(view(&scr(&refs, 40), None, 3), "r23\nr24\nr25");
    assert_eq!(view(&scr(&[], 40), None, 3), "", "nothing on screen, no cursor: nothing");
}

#[test]
fn a_claude_code_screen_shows_its_input_box() {
    // output on top, the box near the bottom with the cursor in it, a status line under it
    let mut rows: Vec<String> = (0..30).map(|k| format!("output {k}")).collect();
    rows.extend(["╭────╮".into(), "│ > ▏│".into(), "╰────╯".into(), "  ? for shortcuts".into()]);
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    // Claude Code hides the terminal's cursor and draws its own
    let v = view(&scr(&refs, 40), None, 6);
    assert_eq!(v, "output 28\noutput 29\n╭────╮\n│ > ▏│\n╰────╯\n  ? for shortcuts");
}

/// 40 rows, every one with text, `r0`..`r39`: a full-screen TUI.
fn full() -> String {
    (0..40).map(|k| format!("r{k}")).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_vim_like_screen_with_the_cursor_near_the_top_shows_the_top() {
    // vim on line 2 of a full screen: tmux's py = 1 - 20/3, clamped to 0
    let v = view(&full(), Some(1), 20);
    let got: Vec<&str> = v.split('\n').collect();
    assert_eq!((got.len(), got[0], got[1]), (20, "r0", "r1"), "the cursor row shows");
}

#[test]
fn a_visible_cursor_mid_screen_sits_a_third_down_the_view() {
    // cursor on row 20, 21 rows shown: the view starts at 20 - 21/3 = 13
    let v = view(&full(), Some(20), 21);
    let got: Vec<&str> = v.split('\n').collect();
    assert_eq!((got[0], got[7], got.len()), ("r13", "r20", 21), "the cursor is row 7 of 21");
}

#[test]
fn a_hidden_cursor_on_a_full_screen_shows_its_last_rows() {
    // htop: no cursor, text to the last row
    assert_eq!(view(&full(), None, 3), "r37\nr38\nr39");
}

#[test]
fn a_row_of_colour_only_is_no_text() {
    let s = scr(&["text", "\x1b[48;5;236m      \x1b[0m"], 10);
    assert_eq!(view(&s, None, 5), "text", "an SGR-painted blank row is not content");
}

#[test]
fn the_cursor_line_ends_the_capture() {
    assert_eq!(split_cursor("a\nb\n\n1 2\n"), Some(("a\nb\n", Some(2))));
    assert_eq!(split_cursor("a\n0 7\n"), Some(("a", None)), "a hidden cursor");
    assert_eq!(split_cursor("a\nb\n"), None, "no cursor line: not a capture we asked for");
    assert_eq!(split_cursor(""), None);
}

/// #284: `screen` reads the capture and its cursor line from one tmux call,
/// drops OSC sequences, picks the view and caps it.
#[test]
fn screen_reads_the_view_off_one_capture() {
    let r = Rig::new("scr");
    r.capture("pre \x1b]8;;u\x1b\\LINK\x1b]8;;\x1b\\ after\nnext\n\n\n1 3\n");
    assert_eq!(
        screen("%9", 10).as_deref(),
        Some("pre LINK after\nnext\n\n"),
        "OSC out, the view to the cursor's row"
    );
    r.capture("a\nb\n");
    assert_eq!(screen("%9", 10), None, "no cursor line: not the capture asked for");
}

#[test]
fn a_screen_over_the_cap_keeps_its_newest_rows_where_they_were() {
    let r = Rig::new("scrcap");
    let line = format!("\x1b[38;5;196m{}\x1b[0m", "x".repeat(100));
    r.capture(&format!("{}\n0 0\n", vec![line.as_str(); 1000].join("\n")));
    let v = screen("%9", 1000).unwrap();
    let rows: Vec<&str> = v.split('\n').collect();
    assert!(v.len() <= 64 * 1024 + 1000, "{} bytes", v.len());
    assert_eq!(rows.len(), 1000, "the rows the cap took are blank, not gone");
    assert!(rows[0].is_empty() && rows[999] == line, "{:?} .. {:?}", rows[0], rows[999]);
    assert!(rows.iter().all(|l| l.is_empty() || *l == line), "no row cut inside an escape");
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
    r.transcript(
        "stopped-str",
        &(user("00", "\"go\"")
            + "{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:00:01.000Z\",\"message\":{\"stop_reason\":\"tool_use\",\
               \"content\":[{\"type\":\"tool_use\",\"id\":\"t8\",\"name\":\"Bash\",\"input\":{\"command\":\"sleep 1\"}}]}}\n"
            + &interrupt_str),
    );
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

    let stopped_str = get(&ps, "%8");
    assert_eq!(stopped_str.state, "done", "the string interrupt form still terminates a turn");
    assert_eq!(stopped_str.asked, "go", "the string interrupt row is not a human turn either");

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

/// A real repo under $TMPDIR, with the ENV lock held so no Rig's git shim is
/// on PATH, and the box's own git config kept out.
struct Repo {
    dir: PathBuf,
    _env: Env,
    _lock: MutexGuard<'static, ()>,
}

impl Repo {
    fn new(tag: &str) -> Repo {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let env = Env::set(&[("GIT_CONFIG_GLOBAL", "/dev/null".into()), ("GIT_CONFIG_NOSYSTEM", "1".into())]);
        let dir = std::env::temp_dir().join(format!("cockpit-tui-git-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let r = Repo {
            dir,
            _env: env,
            _lock: lock,
        };
        r.git(&["init", "-q", "-b", "main", "."]);
        r.write("f", "a\n");
        r.git(&["add", "-A"]);
        r.git(&["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "init"]);
        r
    }

    fn git(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .output()
            .unwrap()
    }

    fn write(&self, name: &str, body: &str) {
        std::fs::write(self.dir.join(name), body).unwrap();
    }

    fn path(&self) -> String {
        self.dir.display().to_string()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `prefix r`'s fleet test fixture (tests/run.sh, "+4 -0  ?3 new"): one tracked
/// line plus three one-line untracked files, the files counted once as new.
#[test]
fn the_diff_counts_untracked_files_like_prefix_r() {
    let r = Repo::new("stat");
    let name = r.dir.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(git_facts(&r.path()), (format!("{name}⑂main"), "clean".into(), r.path()));

    r.write("f", "a\nb\n");
    r.write("new1.txt", "x\n");
    r.write("new2.txt", "y\n");
    r.write("new3.txt", "z\n");
    // #454, the owner's pick: tracked lines only; prefix r's shell tree_diff still
    // counts the untracked lines, so the two differ here by design
    assert_eq!(git_facts(&r.path()).1, "+1 -0  ?3 new");
    let st = String::from_utf8_lossy(&r.git(&["status", "--porcelain"]).stdout).into_owned();
    assert!(st.contains("?? new1.txt"), "the stat touched the real index: {st}");
}

/// The agent owns its repo's config, and the board runs git in it: a planted
/// fsmonitor hook or filter driver must never run.
#[test]
fn planted_git_config_never_runs() {
    let r = Repo::new("plant");
    let marker = r.dir.join("../cockpit-tui-pwned-".to_string() + &std::process::id().to_string());
    let m = marker.display().to_string();
    let plant = |name: &str| {
        let p = r.dir.join(".git").join(name);
        write_exec(&p, &format!("#!/bin/sh\necho {name} >> '{m}'\ncat \"${{1:--}}\"\n"));
        p.display().to_string()
    };
    r.write("t", "a\n");
    r.write("g", "a\n");
    r.git(&["add", "t", "g"]);
    r.git(&["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "t"]);
    r.git(&["config", "core.fsmonitor", &plant("fsmonitor")]);
    r.git(&["config", "filter.evil.clean", &plant("filter")]);
    r.git(&["config", "filter.evil.required", "true"]);
    r.git(&["config", "filter.proc.process", &plant("process")]);
    r.git(&["config", "diff.evil.textconv", &plant("textconv")]);
    r.git(&["config", "diff.external", &plant("external")]);
    r.write(".gitattributes", "f filter=evil\nt filter=proc\ng diff=evil\n");
    r.write("f", "a\nb\n");
    r.write("g", "b\n");
    // a tracked file touched but not changed: only a re-hash can tell, and it runs t's filter
    let touch = |secs: u64| {
        let t = std::time::SystemTime::now() + std::time::Duration::from_secs(secs);
        std::fs::File::options()
            .write(true)
            .open(r.dir.join("t"))
            .unwrap()
            .set_modified(t)
            .unwrap();
    };
    touch(100);

    // the control: plain git runs every plant, so each one is live
    r.git(&["ls-files", "-m"]);
    r.git(&["diff", "--numstat", "HEAD"]);
    // g alone: a patch of f would die in its required filter before reaching g
    r.git(&["diff", "HEAD", "--", "g"]);
    r.git(&["diff", "--no-ext-diff", "HEAD", "--", "g"]);
    let ran = std::fs::read_to_string(&marker).unwrap_or_default();
    for p in ["fsmonitor", "filter", "process", "textconv", "external"] {
        assert!(ran.contains(p), "the {p} plant never fired: {ran:?}");
    }
    std::fs::remove_file(&marker).unwrap();
    touch(200); // the control's diff may have refreshed the real index

    let (_, pm, _) = git_facts(&r.path());
    // #454: tracked lines only; the untracked file is `?1 new`
    assert_eq!(diffstat(&[&r.path()]).0, "2\t1\t1\t+2 -1  ?1 new\n");
    assert!(
        !marker.exists(),
        "a fetch ran planted config: {:?}",
        std::fs::read_to_string(&marker)
    );
    // f gains a line, g changes one, and the untracked .gitattributes is the one new
    // file, its lines no longer counted (#454)
    assert_eq!(pm, "+2 -1  ?1 new", "the hardened diff still measures the tree");
}

/// The board may start with a dotfiles setup's GIT_DIR, or one of its kin, in
/// its environment: a row's name, branch, diff count and root stay the pane repo's.
#[test]
fn an_inherited_git_variable_never_moves_a_boards_row() {
    let r = Repo::new("redirect");
    // tracked past the cap: only the inherited index makes it look untracked, so big
    std::fs::File::create(r.dir.join("core"))
        .unwrap()
        .set_len(33 * 1024 * 1024)
        .unwrap();
    r.git(&["add", "core"]);
    r.git(&["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "core"]);
    let other = r.dir.join(".git/other"); // inside .git: never in the pane repo's tree
    let o = other.display().to_string();
    let git = |args: &[&str]| assert!(r.git(&[&["-C", o.as_str()][..], args].concat()).status.success());
    std::fs::create_dir_all(&other).unwrap();
    git(&["init", "-q", "-b", "elsewhere"]);
    std::fs::write(other.join("g"), "x\n").unwrap();
    git(&["add", "-A"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "other"]);
    let want = git_facts(&r.path());
    assert_eq!(want.1, "clean", "{want:?}");
    let rows = [
        ("GIT_DIR", format!("{o}/.git")),
        ("GIT_WORK_TREE", o.clone()),
        ("GIT_COMMON_DIR", format!("{o}/.git")),
        ("GIT_INDEX_FILE", format!("{o}/.git/index")),
    ];
    let moved: Vec<&str> = rows
        .into_iter()
        .filter(|(var, val)| {
            let _e = Env::set(&[(var, val.clone())]); // `r` holds ENV
            git_facts(&r.path()) != want
        })
        .map(|(var, _)| var)
        .collect();
    assert_eq!(moved, Vec::<&str>::new(), "these moved the row");
}

fn stat(r: &Repo, base: Option<&str>) -> Option<(u64, u64, u64, String)> {
    tree_stat(&r.path(), &r.dir.join(".git").display().to_string(), base)
}

fn diffstat(args: &[&str]) -> (String, i32) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .arg("diffstat")
        .args(args)
        .output()
        .unwrap();
    (String::from_utf8_lossy(&o.stdout).into_owned(), o.status.code().unwrap_or(-1))
}

/// `git diff HEAD` fails on an unborn HEAD, and that failure used to read as
/// "clean" for a tree full of new files.
#[test]
fn an_unborn_repo_with_new_files_is_not_clean() {
    let r = Repo::new("unborn");
    std::fs::remove_dir_all(r.dir.join(".git")).unwrap();
    r.git(&["init", "-q", "-b", "main", "."]);
    r.write("g", "x\ny\n");
    // #454: in an unborn repo every file is untracked: `?2 new`, no lines
    assert_eq!(stat(&r, None), Some((0, 0, 2, "+0 -0  ?2 new".into())));
    assert_eq!(git_facts(&r.path()).1, "+0 -0  ?2 new");
    // staged and otherwise untouched: only the index has it, tree_diff's --cached leg
    r.git(&["add", "-A"]);
    // #454: staged, they are tracked: their lines count, and nothing is untracked
    assert_eq!(stat(&r, None), Some((3, 0, 0, "+3 -0".into())));
}

/// Past BIGCAP a file is never hashed, but it is still a change.
#[test]
fn a_file_over_the_cap_counts_dirty() {
    let r = Repo::new("big");
    // sparse: the size is what the cap reads, and no 33 MB lands on disk
    std::fs::File::create(r.dir.join("core"))
        .unwrap()
        .set_len(33 * 1024 * 1024)
        .unwrap();
    // #657 review: untracked, it is `?1 new` whatever its size
    assert_eq!(stat(&r, None), Some((0, 0, 1, "+0 -0  ?1 new".into())));
}

#[test]
fn base_picks_what_the_diff_is_against() {
    let r = Repo::new("base");
    r.write("f", "a\nb\n");
    r.git(&["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-qam", "two"]);
    assert_eq!(stat(&r, None), Some((0, 0, 0, "clean".into())));
    assert_eq!(stat(&r, Some("HEAD~1")), Some((1, 0, 0, "+1 -0".into())));
    assert_eq!(stat(&r, Some("no-such-rev")), None, "a bad base is a failure, not clean");
    assert_eq!(diffstat(&[&r.path(), "--base", "HEAD~1"]), ("1\t0\t0\t+1 -0\n".into(), 0));
    assert_eq!(diffstat(&[&r.path(), "--base", "no-such-rev"]), (String::new(), 1));
    // a base git would parse as an option is refused before git sees it
    let out = r.dir.join("out");
    let opt = format!("--output={}", out.display());
    assert_eq!(diffstat(&[&r.path(), "--base", &opt]), (String::new(), 1));
    assert!(!out.exists(), "--base reached git as an option");
}

/// The git dir is read off `.git`, which in a linked worktree is a file.
#[test]
fn diffstat_reads_a_linked_worktree_and_refuses_a_non_repo() {
    let r = Repo::new("wt");
    let wt = r.dir.join("wt");
    r.git(&["worktree", "add", "-q", "-b", "side", &wt.display().to_string()]);
    std::fs::write(wt.join("n"), "x\n").unwrap();
    // #454: an untracked file is `?1 new`, its line no `+1`
    assert_eq!(diffstat(&[&wt.display().to_string()]), ("0\t0\t1\t+0 -0  ?1 new\n".into(), 0));
    assert_eq!(diffstat(&[&r.dir.join(".git").display().to_string()]), (String::new(), 1));
}

/// git.sh keeps a static copy of SAFE, pinned to `cockpit-tui git-flags` by tests/run.sh.
#[test]
fn the_flag_lists() {
    assert_eq!(
        SAFE.join(" "),
        concat!(
            "--no-optional-locks -c core.fsmonitor=false -c core.hooksPath=/dev/null -c core.pager=cat -c log.showSignature=false",
            " -c protocol.allow=never -c protocol.file.allow=never -c protocol.ssh.allow=never -c protocol.git.allow=never",
            " -c protocol.http.allow=never -c protocol.https.allow=never -c protocol.ext.allow=never"
        )
    );
    let r = Repo::new("flags");
    assert_eq!(no_filters(&r.path()), Some(Vec::new()));
    r.git(&["config", "filter.lfs.clean", "git-lfs clean -- %f"]);
    r.git(&["config", "filter.a.b.process", "x"]);
    assert_eq!(
        no_filters(&r.path()).unwrap().join(" "),
        "-c filter.a.b.clean= -c filter.a.b.smudge= -c filter.a.b.process= -c filter.a.b.required=false \
         -c filter.lfs.clean= -c filter.lfs.smudge= -c filter.lfs.process= -c filter.lfs.required=false"
    );
}

/// git splits `-c` at the first `=`, so no `-c` can blank a driver named `a=b`:
/// a repo that declares one is refused, and its planted clean never runs.
#[test]
fn a_filter_driver_named_with_an_equals_sign_is_refused() {
    let r = Repo::new("eqdrv");
    let pwned = r.dir.join("PWNED");
    r.write(".gitattributes", "x filter=a=b\n");
    r.write("x", "hi\n");
    r.git(&["config", "filter.a=b.clean", &format!("touch {}", pwned.display())]);
    // what blanking it would look like: git reads that as filter.a, and the plant runs
    r.git(&["-c", "filter.a=b.clean=", "add", "x"]);
    assert!(pwned.exists(), "the plant never fired under a blanked -c, so this proves nothing");
    std::fs::remove_file(&pwned).unwrap();

    assert_eq!(no_filters(&r.path()), None);
    assert_eq!(stat(&r, None), None, "a refused driver is a failure, not a count");
    assert_eq!(diffstat(&[&r.path()]), (String::new(), 1));
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["git-flags", "--filters", &r.path()])
        .output()
        .unwrap();
    assert_eq!((o.stdout.is_empty(), o.status.code()), (true, Some(1)));
    assert!(!pwned.exists(), "the planted clean ran");
}

/// A HEAD that resolves but cannot be diffed is a broken repo, not an unborn
/// one: the unborn fallback would count the index and call that the diff.
#[test]
fn a_broken_head_is_a_failure_not_unborn() {
    let r = Repo::new("broken");
    r.write("f", "a\nb\n");
    let tree = String::from_utf8_lossy(&r.git(&["rev-parse", "HEAD^{tree}"]).stdout)
        .trim()
        .to_string();
    std::fs::remove_file(r.dir.join(".git/objects").join(&tree[..2]).join(&tree[2..])).unwrap();
    assert_eq!(stat(&r, None), None);
}

/// A partial clone fetches a missing blob on demand, and the fetch runs the
/// repo's own remote.origin.uploadpack.
#[test]
fn a_missing_blob_never_lazy_fetches() {
    let r = Repo::new("lazy");
    let marker = r.dir.join("../cockpit-tui-lazy-".to_string() + &std::process::id().to_string());
    write_exec(
        &r.dir.join(".git/up"),
        &format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    );
    let blob = String::from_utf8_lossy(&r.git(&["rev-parse", "HEAD:f"]).stdout).trim().to_string();
    std::fs::remove_file(r.dir.join(".git/objects").join(&blob[..2]).join(&blob[2..])).unwrap();
    r.git(&["config", "core.repositoryformatversion", "1"]);
    r.git(&["config", "extensions.partialClone", "origin"]);
    r.git(&["config", "remote.origin.url", &r.dir.join("../nowhere").display().to_string()]);
    r.git(&["config", "remote.origin.uploadpack", &r.dir.join(".git/up").display().to_string()]);
    r.write("f", "b\n");

    r.git(&["diff", "--numstat", "HEAD"]); // the control
    assert!(marker.exists(), "the plant never fired");
    std::fs::remove_file(&marker).unwrap();

    assert_eq!(diffstat(&[&r.path()]), (String::new(), 1));
    assert!(!marker.exists(), "diffstat ran the planted uploadpack");

    // git < 2.44 ignores GIT_NO_LAZY_FETCH, so only the protocol pin is left,
    // and a planted protocol.file.allow=always must not reopen it.
    r.git(&["config", "protocol.file.allow", "always"]);
    r.git(&["diff", "--numstat", "HEAD"]);
    assert!(marker.exists(), "the plant never fired under protocol.file.allow");
    std::fs::remove_file(&marker).unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    let shim = r.dir.join(".git/oldgit");
    std::fs::create_dir_all(&shim).unwrap();
    write_exec(
        &shim.join("git"),
        &format!("#!/bin/sh\nunset GIT_NO_LAZY_FETCH\nPATH='{path}' exec git \"$@\"\n"),
    );
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["diffstat", &r.path()])
        .env("PATH", format!("{}:{path}", shim.display()))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(!marker.exists(), "diffstat ran the planted uploadpack on an old git");

    // A remote helper the repo names gets past the per-protocol pin.
    let helpers = r.dir.join(".git/helpers");
    std::fs::create_dir_all(&helpers).unwrap();
    write_exec(
        &helpers.join("git-remote-foo"),
        &format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    );
    r.git(&["config", "protocol.foo.allow", "always"]);
    r.git(&["config", "remote.origin.url", "foo::x"]);
    let hpath = format!("{}:{path}", helpers.display());
    std::process::Command::new("git")
        .args(["-C", &r.path(), "diff", "--numstat", "HEAD"])
        .env("PATH", &hpath)
        .output()
        .unwrap();
    assert!(marker.exists(), "the planted remote helper never fired");
    std::fs::remove_file(&marker).unwrap();
    write_exec(
        &shim.join("git"),
        &format!("#!/bin/sh\nunset GIT_NO_LAZY_FETCH\nPATH='{hpath}' exec git \"$@\"\n"),
    );
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(["diffstat", &r.path()])
        .env("PATH", format!("{}:{path}", shim.display()))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(!marker.exists(), "diffstat ran the planted remote helper on an old git");
}

/// The 13th field is `#{window_id}`, which the board's kill checks against;
/// a row with one field fewer is not this format, and is skipped whole.
#[test]
fn the_window_id_is_the_thirteenth_field() {
    let r = Rig::new("winid");
    r.ps("400 1 claude\n401 1 claude\n");
    let short = row("%2", "claude", "running", "-", "401");
    let short = short.rsplit_once('\u{1f}').unwrap().0.to_string();
    assert_eq!(short.split('\u{1f}').count(), 12);
    r.rows(&[row("%1", "claude", "running", "-", "400"), short]);
    let ps = list_panes().unwrap();
    assert_eq!(ids(&ps), ["%1"]);
    assert_eq!(ps[0].win, "@1");
}

/// #291: past `LOG_CAP` the log moves to `.1` and the line starts a new one;
/// under it the line is appended. One generation is kept: `.1` is replaced.
#[test]
fn the_log_rotates_past_its_cap() {
    let r = Rig::new("logrot");
    let log = r.dir.join("tui.log");
    let _env = Env::set(&[("COCKPIT_LOG", log.display().to_string())]);
    let rot = r.dir.join("tui.log.1");
    std::fs::write(&rot, "older\n").unwrap();
    let full = "x".repeat(cockpit_tui::data::LOG_CAP as usize + 1);
    std::fs::write(&log, &full).unwrap();
    cockpit_tui::data::log("after");
    assert_eq!(std::fs::read_to_string(&rot).unwrap(), full);
    let now = std::fs::read_to_string(&log).unwrap();
    assert!(now.ends_with(" after\n") && now.lines().count() == 1, "{now:?}");
    cockpit_tui::data::log("again");
    let now = std::fs::read_to_string(&log).unwrap();
    assert!(now.lines().count() == 2 && now.ends_with(" again\n"), "{now:?}");
    assert_eq!(std::fs::metadata(&rot).unwrap().len(), full.len() as u64);
}

/// #294: `claude agents --json` runs on the fetch thread, so one that hangs
/// (here a script whose child holds stdout open) is killed at the deadline
/// and the read goes on without it, instead of never returning.
#[test]
fn a_hanging_claude_agents_does_not_hang_the_read() {
    let r = Rig::new("agents-hang");
    let d = r.dir.display();
    write_exec(
        &r.dir.join("bin/claude"),
        &format!("#!/bin/sh\necho $$ > {d}/claude.pid\nsleep 60\nprintf '[]'\n"),
    );
    r.ps("400 1 claude\n");
    r.rows(&[row("%1", "claude", "running", "-", "400")]);
    let t = std::time::Instant::now();
    let ps = list_panes().unwrap();
    let took = t.elapsed();
    assert_eq!(ids(&ps), ["%1"]);
    assert_eq!(get(&ps, "%1").sess, "");
    assert!(took < std::time::Duration::from_secs(8), "the read took {took:?}");
    // the fake ran (on its own thread, #492), so the deadline was what ended it
    for _ in 0..2000 {
        if r.dir.join("claude.pid").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(r.dir.join("claude.pid").exists());
}

// --- ended agents (#301) -----------------------------------------------------

/// Ended: the hooks marked the pane, no agent runs in it, and the agent list is
/// known, without which a live `claude` cannot be told from a shell.
#[test]
fn ended_needs_a_mark_no_agent_and_a_known_agent_list() {
    use cockpit_tui::data::ended;
    for (raw, live, glob, want) in [
        ("needs-input", false, true, true),
        ("running", false, true, true),
        ("running", true, true, false),
        ("", false, true, false),
        ("running", false, false, false),
    ] {
        assert_eq!(ended(raw, live, glob), want, "{raw:?} live={live} glob={glob}");
    }
}

// --- the prompt time past the 256 KB tail (#337) -----------------------------

fn prompt(text: &str) -> String {
    format!("{{\"type\":\"user\",\"timestamp\":\"2026-08-27T00:00:00.000Z\",\"message\":{{\"content\":\"{text}\"}}}}\n")
}

/// Tool rows after a prompt: `kb` KB of them, each one a `tool_use`, so the
/// tail never comes up empty and the 2 MB retry stays asleep, as mid-turn.
fn turn_output(kb: usize) -> String {
    let row = format!(
        "{{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:01:00.000Z\",\"message\":{{\"stop_reason\":\"tool_use\",\
         \"content\":[{{\"type\":\"tool_use\",\"id\":\"p\",\"name\":\"Bash\",\"input\":{{\"command\":\"{}\"}}}}]}}}}\n",
        "x".repeat(900)
    );
    row.repeat(kb * 1024 / row.len() + 1)
}

fn append(p: &Path, body: &str) {
    use std::io::Write;
    OpenOptions::new().append(true).open(p).unwrap().write_all(body.as_bytes()).unwrap();
}

fn asked_of(r: &Rig, id: &str) -> (String, Option<u64>) {
    r.rows(&[row(id, "claude", "running", &format!("s{}", &id[1..]), &format!("10{}", &id[1..]))]);
    let ps = list_panes().unwrap();
    let p = get(&ps, id);
    (p.asked.clone(), p.asked_since)
}

/// The mechanism: the newest human prompt is looked up in the last 256 KB only,
/// so output written after it pushes it out, and `asked_since` goes to None.
#[test]
fn the_prompt_time_outlives_256k_of_later_output() {
    let r = Rig::new("askedkeep");
    let p = r.transcript("s1", &(prompt("fix the build") + &turn_output(1)));
    let (asked, since) = asked_of(&r, "%1");
    assert_eq!(asked, "fix the build");
    assert!(since.is_some(), "the prompt is in the window at first");

    append(&p, &turn_output(300));
    let (asked, since) = asked_of(&r, "%1");
    assert_eq!(asked, "fix the build", "the prompt text goes with its time");
    assert!(since.is_some(), "asked_since lost the prompt once it left the tail");
}

/// A second case of the class: the output arrives in many ticks, none of which
/// holds the prompt, and the answer is still the one prompt.
#[test]
fn the_prompt_time_survives_many_reads_inside_a_long_turn() {
    let r = Rig::new("askedticks");
    let p = r.transcript("s1", &(prompt("long one") + &turn_output(1)));
    asked_of(&r, "%1");
    for _ in 0..4 {
        append(&p, &turn_output(100));
        let (asked, since) = asked_of(&r, "%1");
        assert_eq!((asked.as_str(), since.is_some()), ("long one", true));
    }
}

/// One-shot callers (`--json`, `--rows`) read each transcript once in a fresh
/// process, so nothing is remembered: a prompt already past the tail is None.
#[test]
fn a_first_read_past_the_tail_has_no_prompt_time() {
    let r = Rig::new("askedcold");
    r.transcript("s1", &(prompt("buried") + &turn_output(300)));
    assert_eq!(asked_of(&r, "%1"), (String::new(), None));
}

/// The gap between two reads can hold a whole new prompt and more than 256 KB
/// behind it. The window grows to the gap, so the newer prompt wins.
#[test]
fn a_new_prompt_buried_between_two_reads_is_found() {
    let r = Rig::new("askedgap");
    let p = r.transcript("s1", &(prompt("one") + &turn_output(1)));
    asked_of(&r, "%1");
    append(&p, &(prompt("two") + &turn_output(300)));
    assert_eq!(asked_of(&r, "%1").0, "two");
}

/// A prompt in the tail replaces what was remembered.
#[test]
fn a_newer_prompt_replaces_the_remembered_one() {
    let r = Rig::new("askednew");
    let p = r.transcript("s1", &(prompt("one") + &turn_output(1)));
    asked_of(&r, "%1");
    append(&p, &(prompt("two") + &turn_output(1)));
    assert_eq!(asked_of(&r, "%1").0, "two");
}

/// A file that shrinks was rewritten: what was remembered of it is stale.
#[test]
fn a_shrunk_transcript_forgets_the_prompt() {
    let r = Rig::new("askedshrink");
    let p = r.transcript("s1", &(prompt("old") + &turn_output(40)));
    asked_of(&r, "%1");
    std::fs::write(&p, turn_output(5)).unwrap();
    assert_eq!(asked_of(&r, "%1"), (String::new(), None));
}

/// A replaced file (another inode) is another transcript even when it is longer.
#[test]
fn a_replaced_transcript_forgets_the_prompt() {
    let r = Rig::new("askedswap");
    let p = r.transcript("s1", &(prompt("old") + &turn_output(40)));
    asked_of(&r, "%1");
    let next = p.with_extension("new");
    std::fs::write(&next, turn_output(300)).unwrap();
    std::fs::rename(&next, &p).unwrap();
    assert_eq!(asked_of(&r, "%1"), (String::new(), None));
}

/// The memory is per transcript: a pane that never had a prompt gets none.
#[test]
fn the_remembered_prompt_belongs_to_its_own_transcript() {
    let r = Rig::new("askedown");
    let p = r.transcript("s1", &(prompt("mine") + &turn_output(1)));
    r.transcript("s2", &turn_output(1));
    asked_of(&r, "%1");
    append(&p, &turn_output(300));
    assert!(asked_of(&r, "%1").1.is_some());
    assert_eq!(asked_of(&r, "%2"), (String::new(), None));
    // %2's read must not have taken %1's slot: one shared slot would lose it here
    append(&p, &turn_output(300));
    assert_eq!(asked_of(&r, "%1").0, "mine");
}

/// A gap past what the window can scan may hold a newer prompt, so the older one
/// is no longer the answer: it reads as master did, with none.
#[test]
fn a_gap_past_the_scan_limit_forgets_the_prompt() {
    let r = Rig::new("askedhuge");
    let p = r.transcript("s1", &(prompt("one") + &turn_output(1)));
    asked_of(&r, "%1");
    append(&p, &(prompt("two") + &turn_output(2200)));
    assert_eq!(asked_of(&r, "%1"), (String::new(), None));
}

/// A prompt that carries an image is an array of blocks, not a string, and is
/// still the human's turn: it replaces the kept one.
#[test]
fn an_image_prompt_replaces_the_remembered_one() {
    let r = Rig::new("askedimage");
    let p = r.transcript("s1", &(prompt("one") + &turn_output(1)));
    asked_of(&r, "%1");
    append(
        &p,
        "{\"type\":\"user\",\"timestamp\":\"2026-08-27T00:02:00.000Z\",\"message\":{\"content\":[\
         {\"type\":\"image\",\"source\":{\"type\":\"base64\",\"data\":\"AA\"}},{\"type\":\"text\",\"text\":\"see this\"}]}}\n",
    );
    append(&p, &turn_output(300));
    assert_eq!(asked_of(&r, "%1").0, "see this");
}

/// Array rows that are not the human: a tool_result (also one with a text block
/// beside it), a meta row (a skill's text),
/// a harness tag and an interrupt. None of them moves `asked`.
#[test]
fn array_rows_that_are_not_a_prompt_leave_asked_alone() {
    let r = Rig::new("askedarray");
    let p = r.transcript("s1", &(prompt("one") + &turn_output(1)));
    for row in [
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"},{"type":"text","text":"beside a result"}]}}"#,
        r#"{"type":"user","isMeta":true,"message":{"content":[{"type":"text","text":"Base directory for this skill"}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"text","text":"<task-notification>x"}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
    ] {
        append(&p, &(row.to_string() + "\n"));
        assert_eq!(asked_of(&r, "%1").0, "one", "{row}");
    }
}

/// The window never shrinks below 256 KB for a small gap: `said` is an older row
/// that a window the size of the gap would not reach.
#[test]
fn a_small_gap_still_reads_the_256k_floor() {
    let r = Rig::new("askedfloor");
    let said = "{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:00:30.000Z\",\"message\":{\"stop_reason\":\"tool_use\",\
                \"content\":[{\"type\":\"text\",\"text\":\"working on it\"}]}}\n";
    let p = r.transcript("s1", &(prompt("go") + said + &turn_output(20)));
    asked_of(&r, "%1");
    append(&p, &turn_output(4));
    r.rows(&[row("%1", "claude", "running", "s1", "101")]);
    assert_eq!(get(&list_panes().unwrap(), "%1").said, "working on it");
}

/// "YYYY-MM-DDTHH:MM:SS.000Z" for `secs` from now (negative: the past).
fn stamp(secs: i64) -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + secs;
    let (days, rem) = (t.div_euclid(86400), t.rem_euclid(86400));
    // civil-from-days (Hinnant), the inverse of the reader's own
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

fn prompt_stamped(text: &str, secs: i64) -> String {
    format!(
        "{{\"type\":\"user\",\"timestamp\":\"{}\",\"message\":{{\"content\":\"{text}\"}}}}\n",
        stamp(secs)
    )
}

/// A slow read must not push the prompt later: the time is the row's, so a
/// prompt written 100 s ago is at least 100 s old however late it is read,
/// including from the remembered copy.
#[test]
fn the_prompt_age_is_counted_from_the_row_and_not_from_the_read() {
    let r = Rig::new("askedrow");
    let p = r.transcript("s1", &(prompt_stamped("go", -100) + &turn_output(1)));
    let first = asked_of(&r, "%1").1.expect("a prompt in the window has an age");
    assert!(first >= 100, "the age is {first}, under the 100 s the row says");
    append(&p, &turn_output(300));
    let kept = asked_of(&r, "%1").1.expect("the kept prompt has an age");
    assert!(kept >= 100, "the kept age is {kept}, under the 100 s the row says");
}

/// A wall clock stepped back leaves the prompt in the future. The reader says
/// nothing then, and never a kept 0 that hides a failure; the same on the kept path.
#[test]
fn a_prompt_dated_after_now_has_no_age_in_the_window_or_kept() {
    let r = Rig::new("askedfuture");
    let p = r.transcript("s1", &(prompt_stamped("go", 300) + &turn_output(1)));
    assert_eq!(asked_of(&r, "%1"), ("go".to_string(), None));
    append(&p, &turn_output(300));
    assert_eq!(asked_of(&r, "%1"), ("go".to_string(), None));
}

/// #452: a user row carrying `cwd`, main-thread or sidechain.
fn cwd_row(cwd: &str, side: bool) -> String {
    let v = serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:00.000Z", "cwd": cwd,
        "isSidechain": side, "message": {"content": [{"type": "tool_result", "tool_use_id": "x"}]}});
    format!("{v}\n")
}

/// #452: pane %1 at `/w`, its `@cockpit_dir` `opt`, transcript s1 `body`; its `dir`.
fn dir_of(r: &Rig, opt: &str, body: &str) -> String {
    r.transcript("s1", body);
    r.rows(&[format!("{}\u{1f}{opt}", row("%1", "claude", "running", "s1", "100"))]);
    get(&list_panes().unwrap(), "%1").dir.clone()
}

impl Rig {
    /// A directory under the rig, created, as a string.
    fn sub(&self, name: &str) -> String {
        let d = self.dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d.display().to_string()
    }
}

/// #452: with no `@cockpit_dir`, the transcript's newest main-thread cwd is the
/// work dir: an agent that moved into a worktree is shown there.
#[test]
fn the_transcripts_newest_cwd_is_the_work_dir() {
    let r = Rig::new("dircwd");
    let (old, new, side) = (r.sub("old"), r.sub("wt"), r.sub("side"));
    let body = cwd_row(&old, false) + &cwd_row(&new, false) + &cwd_row(&side, true);
    assert_eq!(dir_of(&r, "", &body), new);
}

/// #452: `@cockpit_dir` naming a directory outranks the transcript's cwd.
#[test]
fn cockpit_dir_outranks_the_transcript_cwd() {
    let r = Rig::new("diropt");
    let (opt, said) = (r.sub("opt"), r.sub("said"));
    assert_eq!(dir_of(&r, &opt, &cwd_row(&said, false)), opt);
}

/// #452: rows: a dangling `@cockpit_dir` and a dangling transcript cwd, an
/// empty cwd, no transcript at all. Each falls through to pane_current_path.
#[test]
fn a_work_dir_that_is_not_a_directory_falls_through_to_the_pane_path() {
    let r = Rig::new("dirgone");
    let said = r.sub("said");
    assert_eq!(dir_of(&r, "/no/such/opt", &cwd_row(&said, false)), said, "dangling option");
    assert_eq!(dir_of(&r, "/no/such/opt", &cwd_row("/no/such/said", false)), "/w", "both dangling");
    assert_eq!(dir_of(&r, "", &cwd_row("", false)), "/w", "an empty cwd");
    assert_eq!(dir_of(&r, "", ""), "/w", "nothing in the transcript");
}

/// #452: the cwd seen is kept for the transcript, so 300 KB of rows that carry
/// none, past the 256 KB window, do not take the work dir back to the pane path.
#[test]
fn the_transcript_cwd_outlives_a_window_of_rows_without_one() {
    let r = Rig::new("dirkeep");
    let wt = r.sub("wt");
    assert_eq!(dir_of(&r, "", &cwd_row(&wt, false)), wt);
    append(&r.dir.join(".claude/projects/-w/s1.jsonl"), &turn_output(300));
    r.rows(&[format!("{}\u{1f}", row("%1", "claude", "running", "s1", "100"))]);
    assert_eq!(get(&list_panes().unwrap(), "%1").dir, wt);
}

/// #452: a file shorter than the last read is another file: its cwd is not kept.
#[test]
fn a_rewritten_transcript_keeps_no_cwd_from_the_old_one() {
    let r = Rig::new("dirshrink");
    let wt = r.sub("wt");
    assert_eq!(dir_of(&r, "", &(cwd_row(&wt, false) + &turn_output(1))), wt);
    assert_eq!(dir_of(&r, "", BASH_ROW), "/w");
}

/// #452: `--json` keeps `path`, the launch cwd, and adds `dir`, the work dir.
#[test]
fn the_json_rows_carry_the_work_dir_beside_the_launch_path() {
    let r = Rig::new("dirjson");
    let wt = r.sub("wt");
    dir_of(&r, "", &cwd_row(&wt, false));
    let v: serde_json::Value = serde_json::from_str(&cockpit_tui::fleet::rows_json(&list_panes().unwrap())).unwrap();
    assert_eq!((v[0]["path"].as_str(), v[0]["dir"].as_str()), (Some("/w"), Some(wt.as_str())));
}

/// #452 review: another file replacing the transcript at the same path, even a
/// longer one, is another session's: the old file's cwd is not kept.
#[test]
fn a_replaced_transcript_keeps_no_cwd_from_the_old_file() {
    let r = Rig::new("dirreplace");
    let wt = r.sub("wt");
    assert_eq!(dir_of(&r, "", &cwd_row(&wt, false)), wt);
    // created beside it while the old file still exists, so its inode differs,
    // then renamed over it
    let p = r.dir.join(".claude/projects/-w/s1.jsonl");
    std::fs::write(p.with_extension("new"), turn_output(1)).unwrap();
    std::fs::rename(p.with_extension("new"), &p).unwrap();
    r.rows(&[format!("{}\u{1f}", row("%1", "claude", "running", "s1", "100"))]);
    assert_eq!(get(&list_panes().unwrap(), "%1").dir, "/w");
}

/// #453: an assistant row asking `questions` through AskUserQuestion, as id `id`.
fn ask_row(id: &str, questions: serde_json::Value) -> String {
    let v = serde_json::json!({"type": "assistant", "timestamp": "2026-08-27T00:00:01.000Z",
        "message": {"stop_reason": "tool_use", "content": [{"type": "tool_use", "id": id,
        "name": "AskUserQuestion", "input": {"questions": questions}}]}});
    format!("{v}\n")
}

fn answer_row(id: &str) -> String {
    let v = serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:09.000Z",
        "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": "A"}]}});
    format!("{v}\n")
}

/// A pane the Notification hook marked: state `state`, the hook's generic text.
fn hooked(id: &str, state: &str, sess: &str) -> String {
    let f = [
        id,
        "w:0",
        "win",
        "claude",
        state,
        "-",
        "Claude needs your permission",
        sess,
        "/w",
        "100",
        "-",
        "-",
        "@1",
    ];
    f.join("\u{1f}")
}

fn q(question: &str, header: &str) -> serde_json::Value {
    serde_json::json!({"question": question, "header": header, "options": [], "multiSelect": false})
}

/// #453: an open AskUserQuestion's first question is the row's message, not the
/// hook's generic text, and its header is what the agent is doing.
#[test]
fn an_open_question_is_the_message_and_its_header_the_doing() {
    let r = Rig::new("askmsg");
    r.transcript("s1", &ask_row("tu1", serde_json::json!([q("Merge #445 now?", "Merge")])));
    r.rows(&[hooked("%1", "needs-input", "s1")]);
    let ps = list_panes().unwrap();
    assert_eq!(get(&ps, "%1").msg, "Merge #445 now?");
    assert_eq!(get(&ps, "%1").doing, "AskUserQuestion: Merge");
}

/// #453: the count of the questions after the first leads the message: the board cuts its end.
#[test]
fn more_questions_than_one_are_counted_on_the_message() {
    let r = Rig::new("askmore");
    let qs = serde_json::json!([q("Which runner?", "Runner"), q("b?", "B"), q("c?", "C")]);
    r.transcript("s1", &ask_row("tu1", qs));
    r.transcript("s2", &ask_row("tu1", serde_json::json!([q("Only?", "One")])));
    r.rows(&[hooked("%1", "needs-input", "s1"), hooked("%2", "needs-input", "s2")]);
    let ps = list_panes().unwrap();
    assert_eq!(get(&ps, "%1").msg, "(+2) Which runner?");
    assert_eq!(get(&ps, "%2").msg, "Only?", "one question carries no count");
}

/// #453: model-written text reaches the terminal, so its control and bidi
/// characters go, and only its first non-blank line is a row.
#[test]
fn the_question_is_cleaned_and_cut_to_its_first_line() {
    let r = Rig::new("askclean");
    let raw = "\n \u{202e}\n\u{1b}[31mPick\u{202e}\u{2066} one\u{7}\tnow\nsecond line";
    r.transcript("s1", &ask_row("tu1", serde_json::json!([q(raw, "H\u{202e}")])));
    r.rows(&[hooked("%1", "needs-input", "s1")]);
    let p = list_panes().unwrap().into_iter().find(|p| p.id == "%1").unwrap();
    assert_eq!(p.msg, "[31mPick one now");
    assert_eq!(p.doing, "AskUserQuestion: H");
}

/// #453: an answered question says nothing: a later permission prompt keeps the hook's text.
#[test]
fn an_answered_question_leaves_the_hooks_message() {
    let r = Rig::new("askdone");
    r.transcript(
        "s1",
        &(ask_row("tu1", serde_json::json!([q("Merge?", "Merge")])) + &answer_row("tu1")),
    );
    r.rows(&[hooked("%1", "needs-input", "s1")]);
    assert_eq!(get(&list_panes().unwrap(), "%1").msg, "Claude needs your permission");
}

/// #453: only the newest tool_use's question shows: of two open asks the newer;
/// none when the newest was answered, though an older ask is still open.
#[test]
fn the_newest_open_question_is_the_one_shown() {
    let r = Rig::new("asknewest");
    let (old, new) = (
        ask_row("tu1", serde_json::json!([q("Old?", "O")])),
        ask_row("tu2", serde_json::json!([q("New?", "N")])),
    );
    r.transcript("s1", &(old.clone() + &new));
    r.transcript("s2", &(new + &old.replace("Old?", "Last?") + &answer_row("tu1")));
    r.rows(&[hooked("%1", "needs-input", "s1"), hooked("%2", "needs-input", "s2")]);
    let ps = list_panes().unwrap();
    let msgs = (get(&ps, "%1").msg.as_str(), get(&ps, "%2").msg.as_str());
    assert_eq!(msgs, ("New?", "Claude needs your permission"));
}

/// #453: the hooks said `running`, so nothing is owed: the message is left alone.
#[test]
fn a_running_pane_keeps_its_message_whatever_the_transcript_asks() {
    let r = Rig::new("askrun");
    r.transcript("s1", &ask_row("tu1", serde_json::json!([q("Merge?", "Merge")])));
    r.rows(&[hooked("%1", "running", "s1")]);
    assert_eq!(get(&list_panes().unwrap(), "%1").msg, "Claude needs your permission");
}

/// #453: a pane no hook touched has no message; the open question is its message.
#[test]
fn a_question_is_the_message_of_a_pane_the_hooks_never_marked() {
    let r = Rig::new("asknohook");
    r.ps("100 1 claude\n");
    r.transcript("s1", &ask_row("tu1", serde_json::json!([q("Merge?", "Merge")])));
    r.rows(&[row("%1", "node", "-", "s1", "100")]);
    let ps = list_panes().unwrap();
    assert_eq!(
        (get(&ps, "%1").state.as_str(), get(&ps, "%1").msg.as_str()),
        ("needs-input", "Merge?")
    );
}

/// #453: input of the wrong shape names no question, so the hook's text stays.
/// Rows: no `questions`, an empty list, a string, a number question, a blank one.
#[test]
fn a_question_of_the_wrong_shape_leaves_the_hooks_message() {
    let r = Rig::new("askshape");
    let shapes = [
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!("Merge?"),
        serde_json::json!([{"question": 7, "header": "Seven"}]),
        serde_json::json!([q(" \u{202e}\t", "Blank")]),
    ];
    let mut rows = vec![];
    for (i, s) in shapes.iter().enumerate() {
        r.transcript(&format!("s{i}"), &ask_row("tu1", s.clone()));
        rows.push(hooked(&format!("%{i}"), "needs-input", &format!("s{i}")));
    }
    r.rows(&rows);
    let ps = list_panes().unwrap();
    for (i, s) in shapes.iter().enumerate() {
        assert_eq!(get(&ps, &format!("%{i}")).msg, "Claude needs your permission", "row {i}: {s}");
    }
}

/// #453: `--json` carries the question as `msg` and the header in `doing`.
#[test]
fn the_json_rows_carry_the_question_and_its_header() {
    let r = Rig::new("askjson");
    r.transcript("s1", &ask_row("tu1", serde_json::json!([q("Merge?", "Merge")])));
    r.rows(&[hooked("%1", "needs-input", "s1")]);
    let v: serde_json::Value = serde_json::from_str(&cockpit_tui::fleet::rows_json(&list_panes().unwrap())).unwrap();
    assert_eq!(
        (v[0]["msg"].as_str(), v[0]["doing"].as_str()),
        (Some("Merge?"), Some("AskUserQuestion: Merge"))
    );
}

fn typed_row(text: &str) -> String {
    format!(
        "{}\n",
        serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:05.000Z", "message": {"content": text}})
    )
}

fn interrupt_row() -> String {
    let c = serde_json::json!([{"type": "text", "text": "[Request interrupted by user for tool use]"}]);
    format!(
        "{}\n",
        serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:04.000Z", "message": {"content": c}})
    )
}

fn bash_row(cmd: &str) -> String {
    let v = serde_json::json!({"type": "assistant", "timestamp": "2026-08-27T00:00:06.000Z",
        "message": {"stop_reason": "tool_use", "content": [{"type": "tool_use", "id": "b1", "name": "Bash",
        "input": {"command": cmd}}]}});
    format!("{v}\n")
}

/// #453 review: an interrupted ask, then a typed prompt and a Bash permission.
/// The pane waits on the Bash, so the hook's text stays and the old question never shows.
#[test]
fn a_question_the_agent_moved_on_from_is_not_the_message() {
    let r = Rig::new("askstale");
    let body = ask_row("tu1", serde_json::json!([q("Old question?", "Old")]))
        + &interrupt_row()
        + &typed_row("do something else")
        + &bash_row("rm -rf build");
    r.transcript("s1", &body);
    r.rows(&[hooked("%1", "needs-input", "s1")]);
    let p = list_panes().unwrap().into_iter().find(|p| p.id == "%1").unwrap();
    assert_eq!(
        (p.msg.as_str(), p.doing.as_str()),
        ("Claude needs your permission", "Bash: rm -rf build")
    );
}

/// #453 review: a typed prompt closes an interrupted ask, so a pane the hooks
/// never marked is running its new turn, not waiting on the old question.
#[test]
fn a_typed_prompt_closes_an_interrupted_question() {
    let r = Rig::new("askprompt");
    r.ps("100 1 claude\n");
    let body = ask_row("tu1", serde_json::json!([q("Old?", "Old")])) + &interrupt_row() + &typed_row("go on");
    r.transcript("s1", &body);
    r.rows(&[row("%1", "node", "-", "s1", "100")]);
    let p = list_panes().unwrap().into_iter().find(|p| p.id == "%1").unwrap();
    assert_eq!((p.state.as_str(), p.msg.as_str()), ("running", ""));
}

/// #453 review: the newest tool_use is an open ask with no readable question:
/// no older open question stands in for it.
#[test]
fn an_unreadable_newest_question_does_not_fall_back_to_an_older_one() {
    let r = Rig::new("asknofall");
    let body = ask_row("tu1", serde_json::json!([q("Old?", "O")])) + &ask_row("tu2", serde_json::json!([]));
    r.transcript("s1", &body);
    r.rows(&[hooked("%1", "needs-input", "s1")]);
    assert_eq!(get(&list_panes().unwrap(), "%1").msg, "Claude needs your permission");
}

/// #453 review: what the model said and the human asked are drawn too: their
/// bidi controls go, as the question's do.
#[test]
fn said_and_asked_drop_bidi_controls() {
    let r = Rig::new("askbidi");
    let said = serde_json::json!({"type": "assistant", "timestamp": "2026-08-27T00:00:06.000Z",
        "message": {"stop_reason": "end_turn", "content": [{"type": "text", "text": "do\u{202e}ne"}]}});
    r.transcript("s1", &(typed_row("fi\u{2066}x it") + &format!("{said}\n")));
    r.rows(&[hooked("%1", "done", "s1")]);
    let p = list_panes().unwrap().into_iter().find(|p| p.id == "%1").unwrap();
    assert_eq!((p.said.as_str(), p.asked.as_str()), ("done", "fix it"));
}

/// #453 review: the question shows only while its ask is the newest tool_use.
/// One turn asking and then running a Bash that waits on a permission: the hook's
/// text is about the Bash, so it stays.
#[test]
fn a_tool_use_after_the_open_question_keeps_the_hooks_message() {
    let r = Rig::new("askthenbash");
    let v = serde_json::json!({"type": "assistant", "timestamp": "2026-08-27T00:00:01.000Z",
        "message": {"stop_reason": "tool_use", "content": [
            {"type": "tool_use", "id": "tu1", "name": "AskUserQuestion", "input": {"questions": [q("Merge?", "M")]}},
            {"type": "tool_use", "id": "b1", "name": "Bash", "input": {"command": "rm -rf build"}}]}});
    r.transcript("s1", &format!("{v}\n"));
    r.rows(&[hooked("%1", "needs-input", "s1")]);
    assert_eq!(get(&list_panes().unwrap(), "%1").msg, "Claude needs your permission");
}

/// #460: `@cockpit_job_name`, field 16, is the bound job's name, cleaned: it comes
/// from a pane title. A row without it (an older format) has no job.
#[test]
fn the_bound_job_name_is_read_cleaned_and_absent_is_none() {
    let r = Rig::new("jobname");
    let base = row("%1", "claude", "running", "-", "100");
    let job = "night\u{202e}ly\u{1b}[31m sweep";
    r.rows(&[
        format!("{base}\u{1f}\u{1f}\u{1f}\u{1f}{job}"),
        row("%2", "claude", "running", "-", "101"),
    ]);
    let ps = list_panes().unwrap();
    assert_eq!(
        (get(&ps, "%1").job.as_str(), get(&ps, "%2").job.as_str()),
        ("nightly[31m sweep", "")
    );
    let v: serde_json::Value = serde_json::from_str(&cockpit_tui::fleet::rows_json(&ps)).unwrap();
    let job_of = |id: &str| v.as_array().unwrap().iter().find(|o| o["id"] == id).unwrap()["job"].clone();
    assert_eq!((job_of("%1"), job_of("%2")), ("nightly[31m sweep".into(), "".into()));
}

/// #477: the board's message and the info screen's ask row agree on whether a
/// row after an open question was the human moving on. Rows: the interrupt string,
/// which is not a prompt, so the question still shows on both; an image prompt,
/// which is one, so neither shows it.
#[test]
fn the_board_and_info_agree_on_what_closes_an_open_question() {
    let r = Rig::new("askagree");
    let image = serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:05.000Z", "message": {"content": [
        {"type": "image", "source": {"type": "base64", "data": "AA=="}}, {"type": "text", "text": "look at this"}]}});
    let cases = [
        ("s1", typed_row("[Request interrupted by user]"), true),
        ("s2", format!("{image}\n"), false),
    ];
    let mut rows = vec![];
    for (sess, after, _) in &cases {
        r.transcript(sess, &(ask_row("tu1", serde_json::json!([q("Merge?", "M")])) + after));
        rows.push(hooked(&format!("%{}", &sess[1..]), "needs-input", sess));
    }
    r.rows(&rows);
    let ps = list_panes().unwrap();
    for (sess, _, shown) in cases {
        let board = get(&ps, &format!("%{}", &sess[1..])).msg == "Merge?";
        let file = r.dir.join(format!(".claude/projects/-w/{sess}.jsonl"));
        let info = cockpit_tui::data::transcript::doing(&file)
            .unwrap_or_default()
            .contains("ask\tMerge?");
        assert_eq!((board, info), (shown, shown), "{sess}: (board shows it, info shows it)");
    }
}

/// #477 review: the board's `asked` and the info screen's `asked` row are the same
/// prompt after an open question: the interrupt string is not one, so both keep the
/// first prompt; an image prompt is one, so both name its text.
#[test]
fn the_board_and_info_agree_on_the_asked_prompt() {
    let r = Rig::new("askedagree");
    let image = serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:05.000Z", "message": {"content": [
        {"type": "image", "source": {"type": "base64", "data": "AA=="}}, {"type": "text", "text": "look at this"}]}});
    // #351: the summary Claude Code writes after /compact is no prompt either
    let text = "This session is being continued from a previous one.";
    let summary = serde_json::json!({"role": "user", "content": text});
    let compact = serde_json::json!({"type": "user", "isCompactSummary": true,
        "timestamp": "2026-08-27T00:00:09.000Z", "message": summary});
    let cases = [
        ("s1", typed_row("[Request interrupted by user]"), "first prompt"),
        ("s2", format!("{image}\n"), "look at this"),
        ("s3", format!("{compact}\n"), "first prompt"),
    ];
    let mut rows = vec![];
    for (sess, after, _) in &cases {
        let body = typed_row("first prompt") + &ask_row("tu1", serde_json::json!([q("Merge?", "M")])) + after;
        r.transcript(sess, &body);
        rows.push(hooked(&format!("%{}", &sess[1..]), "needs-input", sess));
    }
    r.rows(&rows);
    let ps = list_panes().unwrap();
    for (sess, _, want) in cases {
        let board = get(&ps, &format!("%{}", &sess[1..])).asked.clone();
        let file = r.dir.join(format!(".claude/projects/-w/{sess}.jsonl"));
        let doing = cockpit_tui::data::transcript::doing(&file).unwrap_or_default();
        let info = doing
            .lines()
            .find_map(|l| l.strip_prefix("asked\t"))
            .and_then(|l| l.split('\t').next());
        assert_eq!((board.as_str(), info), (want, Some(want)), "{sess}: (board, info)");
    }
}

/// #454 item 5: with no @cockpit_title the label is the pane's own title (field 17,
/// which the format already strips of `✳ ` and blanks when it is the host name),
/// else the window name. Rows: a topic, none, and @cockpit_title over a topic.
#[test]
fn the_label_falls_back_to_the_pane_title_before_the_window_name() {
    let r = Rig::new("topic");
    let base = |id: &str, title: &str| {
        let mut f: Vec<String> = row(id, "claude", "running", "-", "100").split('\u{1f}').map(String::from).collect();
        f[5] = title.into();
        f.extend(["", "", "", ""].map(String::from));
        f
    };
    let with = |mut f: Vec<String>, topic: &str| {
        f.push(topic.into());
        f.join("\u{1f}")
    };
    r.rows(&[
        with(base("%1", "-"), "migrate the runner"),
        with(base("%2", "-"), ""),
        with(base("%3", "fix it"), "a topic"),
    ]);
    let ps = list_panes().unwrap();
    let labels: Vec<&str> = ["%1", "%2", "%3"].iter().map(|id| get(&ps, id).label()).collect();
    assert_eq!(labels, ["migrate the runner", "win", "fix it"]);
}

/// #454 item 12, the owner's pick: a pane knows when its last tool_use came back
/// with `is_error` (a denial or a failed command alike), the flag only. Rows: an
/// ok result, an error result, an unanswered tool_use (an interrupt), and an
/// older failure before a newer tool that came back fine.
#[test]
fn the_last_tool_uses_error_flag_is_read() {
    let r = Rig::new("toolerr");
    let call = serde_json::json!({"type": "assistant", "timestamp": "2026-08-27T00:00:01.000Z", "message": {"content": [
        {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ssh nuc"}}]}});
    let result = |err: bool| {
        let v = serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:02.000Z", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "t1", "is_error": err, "content": "x"}]}});
        format!("{v}\n")
    };
    let end = "{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T00:00:03.000Z\",\"message\":{\"stop_reason\":\"end_turn\",\"content\":[{\"type\":\"text\",\"text\":\"The permission check blocked it.\"}]}}\n";
    r.transcript("s1", &(format!("{call}\n") + &result(false) + end));
    r.transcript("s2", &(format!("{call}\n") + &result(true) + end));
    r.transcript("s3", &(format!("{call}\n") + &typed_row("[Request interrupted by user]")));
    // an earlier failure, then a newer tool that came back fine
    let later = call.to_string().replace("\"t1\"", "\"t2\"");
    let ok2 = result(false).replace("\"t1\"", "\"t2\"");
    r.transcript("s4", &(format!("{call}\n") + &result(true) + &later + "\n" + &ok2 + end));
    let ids = ["%1", "%2", "%3", "%4"];
    r.rows(&ids.map(|id| hooked(id, "done", &format!("s{}", &id[1..]))));
    let ps = list_panes().unwrap();
    let got: Vec<bool> = ids.iter().map(|id| get(&ps, id).doing_failed).collect();
    assert_eq!(got, [false, true, false, false], "ok, is_error, unanswered, an older failure");
}

/// #657 review (review657g): the tracked-only stat over each shape a tree can take.
/// `g` is committed with six lines; each row is (name, setup, diffstat's line).
#[test]
fn the_tracked_only_stat_over_renames_staging_and_odd_names() {
    let six = "1\n2\n3\n4\n5\n6\n";
    type Setup = fn(&Repo);
    let rows: [(&str, Setup, &str); 10] = [
        (
            "a plain mv: g gone, h untracked",
            |r| {
                std::fs::rename(r.dir.join("g"), r.dir.join("h")).unwrap();
            },
            "0\t6\t1\t+0 -6  ?1 new\n",
        ),
        (
            "a plain mv and a new line",
            |r| {
                std::fs::rename(r.dir.join("g"), r.dir.join("h")).unwrap();
                r.write("h", "1\n2\n3\n4\n5\n6\n7\n");
            },
            "0\t6\t1\t+0 -6  ?1 new\n",
        ),
        (
            "a staged git mv is a rename",
            |r| {
                r.git(&["mv", "g", "h"]);
            },
            "0\t0\t0\t+0 -0\n",
        ),
        (
            "a git mv under diff.renames=false is still a rename",
            |r| {
                r.git(&["config", "diff.renames", "false"]);
                r.git(&["mv", "g", "h"]);
            },
            "0\t0\t0\t+0 -0\n",
        ),
        (
            "git rm --cached: g deleted, untracked again",
            |r| {
                r.git(&["rm", "-q", "--cached", "g"]);
            },
            "0\t6\t1\t+0 -6  ?1 new\n",
        ),
        (
            "a staged new file",
            |r| {
                r.write("n", "x\ny\n");
                r.git(&["add", "n"]);
            },
            "2\t0\t0\t+2 -0\n",
        ),
        (
            "a staged new file with a tab, a quote and a non-UTF-8 byte",
            |r| {
                use std::os::unix::ffi::OsStrExt;
                let name = std::ffi::OsStr::from_bytes(b"n\t\"\\\xff");
                std::fs::write(r.dir.join(name), "x\ny\n").unwrap();
                r.git(&["add", "-A"]);
            },
            "2\t0\t0\t+2 -0\n",
        ),
        ("a staged q\"x.txt", |r| stage_one(r, b"q\"x.txt"), "1\t0\t0\t+1 -0\n"),
        ("a staged t<TAB>b.txt", |r| stage_one(r, b"t\tb.txt"), "1\t0\t0\t+1 -0\n"),
        ("a staged b\\377n.txt", |r| stage_one(r, b"b\xffn.txt"), "1\t0\t0\t+1 -0\n"),
    ];
    let mut bad = Vec::new();
    for (name, setup, want) in rows {
        let r = Repo::new("shapes");
        r.write("g", six);
        r.git(&["add", "g"]);
        r.git(&["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "g"]);
        setup(&r);
        let got = diffstat(&[&r.path()]);
        if got != (want.to_string(), 0) {
            bad.push((name, got));
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

/// Writes one line to a file named by raw bytes and stages it.
fn stage_one(r: &Repo, name: &[u8]) {
    use std::os::unix::ffi::OsStrExt;
    std::fs::write(r.dir.join(std::ffi::OsStr::from_bytes(name)), "x\n").unwrap();
    r.git(&["add", "-A"]);
}

/// #657 review: an unborn repo with one staged file and one untracked one: the
/// staged lines count, the untracked file is `?1 new`.
#[test]
fn an_unborn_repo_counts_its_staged_lines_beside_an_untracked_file() {
    let r = Repo::new("unborn2");
    std::fs::remove_dir_all(r.dir.join(".git")).unwrap();
    std::fs::remove_file(r.dir.join("f")).unwrap();
    r.git(&["init", "-q", "-b", "main", "."]);
    r.write("s", "x\ny\n");
    r.git(&["add", "s"]);
    r.write("u", "z\n");
    assert_eq!(diffstat(&[&r.path()]), ("2\t0\t1\t+2 -0  ?1 new\n".into(), 0));
}
