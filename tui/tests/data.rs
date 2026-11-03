//! The data layer's caches, classifier and pane reads, driven through PATH
//! shims: no real tmux, no real ps, no clock assertion and no sleep. Every
//! fixture is a literal, so a failure names one behaviour and not the box.
use cockpit_tui::data::gitsafe::{SAFE, no_filters, tree_stat};
use cockpit_tui::data::{Pane, git_facts, list_panes, preview};
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
    assert_eq!(git_facts(&r.path()).1, "+4 -0  ?3 new");
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
    assert_eq!(diffstat(&[&r.path()]).0, "5\t1\t1\t+5 -1  ?1 new\n");
    assert!(
        !marker.exists(),
        "a fetch ran planted config: {:?}",
        std::fs::read_to_string(&marker)
    );
    // f gains a line, g changes one, and the untracked .gitattributes is the one new file
    assert_eq!(pm, "+5 -1  ?1 new", "the hardened diff still measures the tree");
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
    assert_eq!(stat(&r, None), Some((3, 0, 2, "+3 -0  ?2 new".into())));
    assert_eq!(git_facts(&r.path()).1, "+3 -0  ?2 new");
    // staged and otherwise untouched: only the index has it, tree_diff's --cached leg
    r.git(&["add", "-A"]);
    assert_eq!(stat(&r, None), Some((3, 0, 2, "+3 -0  ?2 new".into())));
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
    assert_eq!(stat(&r, None), Some((0, 0, 0, "+0 -0".into())));
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
    assert_eq!(diffstat(&[&wt.display().to_string()]), ("1\t0\t1\t+1 -0  ?1 new\n".into(), 0));
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
    // the fake ran, so the deadline was what ended it
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
