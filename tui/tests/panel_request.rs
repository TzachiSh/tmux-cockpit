//! The `claude -p` call against fake claude scripts. Each test builds its own
//! `Call`, so nothing here touches the process environment.
use cockpit_tui::panel::request::{Call, Fail, Profile, Request, prompt_file};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// A fork made while another test holds a fake open for writing carries that fd
/// to its exec, and the fake then fails ETXTBSY: writes and spawns take turns.
static FORK: Mutex<()> = Mutex::new(());

fn spawn(call: &Call, system: &str, prompt: &str, model: &str) -> Request {
    let _g = FORK.lock().unwrap_or_else(|e| e.into_inner());
    Request::spawn(call, system, prompt, model)
}

fn spawn_with(call: &Call, p: &Profile, prompt: &str) -> Request {
    let _g = FORK.lock().unwrap_or_else(|e| e.into_inner());
    Request::spawn_with(call, p, "S", prompt, "default")
}

/// Polled, never slept for: gives up after 10 s.
fn until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..1000 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Reaped, or a zombie its new parent has yet to reap: either way, not running.
fn gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat.rsplit_once(") ").is_some_and(|(_, r)| r.starts_with('Z')),
        Err(_) if Path::new("/proc/self").exists() => true,
        // SAFETY: signal 0 delivers nothing; it only asks whether the pid exists.
        Err(_) => (unsafe { libc::kill(pid as libc::pid_t, 0) }) != 0,
    }
}

/// Fully reaped: the worker waited on it, so not even a zombie is left.
fn reaped(pid: u32) -> bool {
    if Path::new("/proc/self").exists() {
        !Path::new(&format!("/proc/{pid}")).exists()
    } else {
        gone(pid)
    }
}

struct Rig {
    dir: PathBuf,
}

impl Rig {
    fn new(tag: &str, script: &str) -> Rig {
        let dir = std::env::temp_dir().join(format!("cockpit-panel-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("claude");
        let body = script.replace("$D", &dir.display().to_string());
        let _g = FORK.lock().unwrap_or_else(|e| e.into_inner());
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        Rig { dir }
    }

    fn call(&self, timeout: u64) -> Call {
        Call {
            claude: self.dir.join("claude").display().to_string(),
            timeout: Duration::from_secs(timeout),
            dir: self.dir.join("cache"),
        }
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }

    fn pid_file(&self, name: &str) -> Option<u32> {
        self.read(name).trim().parse().ok()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn answer(req: &mut Request) -> Result<String, Fail> {
    let mut got = None;
    assert!(
        until(|| {
            got = req.poll();
            got.is_some()
        }),
        "no answer in 10 s"
    );
    got.unwrap()
}

#[test]
fn the_argv_stdin_and_cwd_follow_the_contract() {
    let rig = Rig::new(
        "argv",
        "printf '%s\\n' \"$@\" > $D/argv\ncat > $D/stdin\npwd > $D/cwd\necho 'the reply'",
    );
    let mut req = spawn(&rig.call(30), "SYS", "hello </x>\n", "sonnet");
    assert_eq!(answer(&mut req), Ok("the reply\n".into()));
    assert_eq!(req.poll(), None, "the answer comes once");

    let argv = rig.read("argv");
    let args: Vec<&str> = argv.lines().collect();
    let file = rig.call(30).dir.join("prompts");
    assert_eq!(args.len(), 10, "{argv}");
    assert_eq!(args[..4], ["-p", "--model", "sonnet", "--system-prompt-file"]);
    assert!(args[4].starts_with(&file.display().to_string()), "{}", args[4]);
    assert_eq!(
        args[5..],
        [
            "--tools",
            "",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--disable-slash-commands"
        ]
    );
    assert_eq!(std::fs::read_to_string(args[4]).unwrap(), "SYS");
    assert_eq!(rig.read("stdin"), "hello </x>\n");
    let cwd = std::fs::canonicalize(rig.call(30).dir).unwrap();
    assert_eq!(rig.read("cwd").trim(), cwd.display().to_string());
}

#[test]
fn the_default_model_sends_no_model_flag() {
    let rig = Rig::new("default", "printf '%s\\n' \"$@\" > $D/argv\necho ok");
    let mut req = spawn(&rig.call(30), "S", "p", "default");
    assert_eq!(answer(&mut req), Ok("ok\n".into()));
    assert_eq!(rig.read("argv").lines().take(2).collect::<Vec<_>>(), ["-p", "--system-prompt-file"]);
}

#[test]
fn a_failed_call_reports_the_first_stderr_line() {
    let rig = Rig::new("stderr", "echo partial\necho 'boom: bad model' >&2\necho more >&2\nexit 3");
    let mut req = spawn(&rig.call(30), "S", "p", "haiku");
    assert_eq!(answer(&mut req), Err(Fail::Failed("boom: bad model".into())));
}

#[test]
fn a_failed_call_with_only_stdout_reports_stdout() {
    let rig = Rig::new("stdout", "echo\necho 'Error: not logged in'\nexit 1");
    let mut req = spawn(&rig.call(30), "S", "p", "haiku");
    assert_eq!(answer(&mut req), Err(Fail::Failed("Error: not logged in".into())));
}

#[test]
fn an_empty_answer_is_a_failure_even_on_exit_0() {
    let rig = Rig::new("empty", "cat >/dev/null\necho '  '");
    let mut req = spawn(&rig.call(30), "S", "p", "haiku");
    assert_eq!(answer(&mut req), Err(Fail::Failed("no answer".into())));
    let rig = Rig::new("empty-warn", "echo 'warn: quota' >&2");
    let mut req = spawn(&rig.call(30), "S", "p", "haiku");
    assert_eq!(answer(&mut req), Err(Fail::Failed("warn: quota".into())));
}

#[test]
fn a_missing_binary_fails_through_poll() {
    let call = Call {
        claude: "/nonexistent/claude".into(),
        timeout: Duration::from_secs(5),
        dir: std::env::temp_dir().join("cockpit-panel-missing"),
    };
    let mut req = spawn(&call, "S", "p", "haiku");
    assert_eq!(req.pid(), None);
    match answer(&mut req) {
        Err(Fail::Failed(s)) => assert!(s.starts_with("/nonexistent/claude: "), "{s}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_timeout_kills_the_whole_process_group() {
    let rig = Rig::new("timeout", "sleep 1000 &\necho $! > $D/kid\nexec sleep 1000");
    let mut req = spawn(&rig.call(1), "S", "p", "haiku");
    let pid = req.pid().unwrap();
    assert!(until(|| rig.pid_file("kid").is_some()), "the fake never started");
    let kid = rig.pid_file("kid").unwrap();
    assert_eq!(answer(&mut req), Err(Fail::Timeout));
    assert_eq!(Fail::Timeout.to_string(), "took too long");
    assert!(until(|| reaped(pid)), "the leader {pid} was not reaped");
    assert!(until(|| gone(kid)), "the background child {kid} outlived its group");
}

#[test]
fn cancel_kills_and_reaps_and_silences_poll() {
    let rig = Rig::new("cancel", "echo $$ > $D/pid\nexec sleep 1000");
    let mut req = spawn(&rig.call(120), "S", "p", "haiku");
    assert!(until(|| rig.pid_file("pid").is_some()), "the fake never started");
    let pid = req.pid().unwrap();
    assert_eq!(rig.pid_file("pid"), Some(pid), "the spawned pid is the group leader");
    req.cancel();
    assert!(until(|| reaped(pid)), "{pid} survived cancel, or was never reaped");
    assert_eq!(req.poll(), None);
}

#[test]
fn dropping_a_request_kills_its_claude() {
    let rig = Rig::new("drop", "echo $$ > $D/pid\nexec sleep 1000");
    let req = spawn(&rig.call(120), "S", "p", "haiku");
    assert!(until(|| rig.pid_file("pid").is_some()), "the fake never started");
    let pid = req.pid().unwrap();
    drop(req);
    assert!(until(|| reaped(pid)), "{pid} survived the drop");
}

#[test]
fn the_system_prompt_is_written_once_per_text_and_private() {
    let rig = Rig::new("prompt", "true");
    let dir = rig.dir.join("cache");
    let a = prompt_file(&dir, "one").unwrap();
    assert_eq!(prompt_file(&dir, "one").unwrap(), a);
    let b = prompt_file(&dir, "two").unwrap();
    assert_ne!(a, b);
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "two");
    assert_eq!(std::fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(dir.join("prompts")).unwrap().permissions().mode() & 0o777, 0o700);
    let left: Vec<_> = std::fs::read_dir(dir.join("prompts"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(left.is_empty(), "no temp file is left behind");
}

#[test]
fn threads_writing_one_prompt_do_not_collide() {
    let rig = Rig::new("prompt-race", "true");
    let dir = rig.dir.join("cache");
    let paths: Vec<PathBuf> = (0..16)
        .map(|_| {
            let dir = dir.clone();
            std::thread::spawn(move || prompt_file(&dir, "same text").unwrap())
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect();
    assert!(paths.iter().all(|p| *p == paths[0]));
    assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), "same text");
    assert_eq!(
        std::fs::read_dir(dir.join("prompts")).unwrap().count(),
        1,
        "one file, no temp left behind"
    );
}

#[test]
fn a_bad_byte_does_not_lose_the_answer() {
    let rig = Rig::new("bytes", "printf 'caf\\351 ok\\n'");
    let mut req = spawn(&rig.call(30), "S", "p", "haiku");
    assert_eq!(answer(&mut req), Ok("caf\u{fffd} ok\n".into()));
    assert_eq!(req.pid(), None, "a reaped child has no pid to hand out");
}

// ---------------------------------------------------------------- profiles (#365)

/// The ask's cap in these tests: long enough never to fire.
const CAP: Duration = Duration::from_secs(300);

const ECHO_ARGV: &str = "printf '%s\\n' \"$@\" > $D/argv\npwd > $D/cwd\necho ok";

/// The argv after `-p` and the system prompt file, one per line.
fn argv_after_prompt(rig: &Rig) -> Vec<String> {
    let argv = rig.read("argv");
    let args: Vec<String> = argv.lines().map(String::from).collect();
    let at = args.iter().position(|a| a == "--system-prompt-file").expect("a system prompt file");
    args[at + 2..].to_vec()
}

/// The text profile is today's call, byte for byte: `spawn` and `spawn_with(TEXT)` agree.
#[test]
fn the_text_profile_is_todays_argv() {
    let rig = Rig::new("text", ECHO_ARGV);
    let mut req = spawn_with(&rig.call(30), &Profile::TEXT, "p");
    assert_eq!(answer(&mut req), Ok("ok\n".into()));
    assert_eq!(
        argv_after_prompt(&rig),
        [
            "--tools",
            "",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--disable-slash-commands"
        ]
    );
    let cwd = std::fs::canonicalize(rig.call(30).dir).unwrap();
    assert_eq!(rig.read("cwd").trim(), cwd.display().to_string());
}

/// Read-only: three tools, each pre-approved, the writers denied, no prompt to wait on,
/// slash commands off, and the cwd the profile's.
#[test]
fn the_read_only_profile_reads_and_never_writes() {
    let rig = Rig::new("ro", ECHO_ARGV);
    let repo = rig.dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut req = spawn_with(&rig.call(30), &Profile::read_only(repo.clone(), false, 0.5, CAP), "p");
    assert_eq!(answer(&mut req), Ok("ok\n".into()));
    let repo_s = repo.display().to_string();
    assert_eq!(
        argv_after_prompt(&rig),
        [
            "--tools",
            "Read,Grep,Glob",
            "--allowedTools",
            "Read,Grep,Glob",
            "--disallowedTools",
            "Bash,Edit,Write,NotebookEdit,Agent",
            "--permission-prompts",
            "none",
            "--settings",
            "{\"disableAllHooks\":true}",
            "--add-dir",
            &repo_s,
            "--max-budget-usd",
            "0.5",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--disable-slash-commands"
        ]
    );
    // review-432's blocker: a cwd in the repo loads its .claude/settings.json,
    // whose hooks, env and apiKeyHelper run in -p. The repo goes in by --add-dir.
    let cwd = rig.read("cwd").trim().to_string();
    assert_eq!(cwd, std::fs::canonicalize(rig.call(30).dir).unwrap().display().to_string());
    assert_ne!(cwd, std::fs::canonicalize(&repo).unwrap().display().to_string());
}

/// Research adds the web and skills, and so drops `--disable-slash-commands`.
#[test]
fn the_research_profile_adds_the_web_and_skills() {
    let rig = Rig::new("research", ECHO_ARGV);
    let p = Profile::read_only(rig.dir.clone(), true, 0.5, CAP);
    let mut req = spawn_with(&rig.call(30), &p, "p");
    assert_eq!(answer(&mut req), Ok("ok\n".into()));
    let tools = "Read,Grep,Glob,WebSearch,WebFetch,Skill";
    let dir = rig.dir.display().to_string();
    assert_eq!(
        argv_after_prompt(&rig),
        [
            "--tools",
            tools,
            "--allowedTools",
            tools,
            "--disallowedTools",
            "Bash,Edit,Write,NotebookEdit,Agent",
            "--permission-prompts",
            "none",
            "--settings",
            "{\"disableAllHooks\":true}",
            "--add-dir",
            &dir,
            "--max-budget-usd",
            "0.5",
            "--no-session-persistence",
            "--strict-mcp-config"
        ]
    );
}

/// No profile reaches a writer: every read-only one denies all five.
#[test]
fn every_read_only_profile_denies_the_writers() {
    for research in [false, true] {
        let p = Profile::read_only(PathBuf::from("/"), research, 0.5, CAP);
        for t in ["Bash", "Edit", "Write", "NotebookEdit", "Agent"] {
            assert!(
                p.deny.contains(&t) && !p.tools.contains(&t) && !p.allow.contains(&t),
                "{t}, research {research}"
            );
        }
    }
}

/// A profile's own timeout wins over the call's: an ask may run longer than a write.
#[test]
fn a_profile_timeout_overrides_the_calls() {
    let rig = Rig::new("ptimeout", "echo $$ > $D/pid\nexec sleep 30");
    let p = Profile {
        timeout: Some(Duration::from_millis(300)),
        ..Profile::TEXT
    };
    let mut req = spawn_with(&rig.call(60), &p, "p");
    assert_eq!(answer(&mut req), Err(Fail::Timeout));
}

/// claude's own budget stop is a failure the panel shows in its words.
#[test]
fn a_budget_stop_is_shown_as_claude_said_it() {
    let rig = Rig::new("budget", "echo 'Error: Exceeded USD budget (0.01)'\nexit 1");
    let mut req = spawn_with(&rig.call(30), &Profile::TEXT, "p");
    assert_eq!(answer(&mut req), Err(Fail::Failed("Error: Exceeded USD budget (0.01)".into())));
}

/// A skill's text can ask for a shell or git (#428: git in a folder can rewrite its
/// index, and a submodule's filter driver runs). The argv itself holds the line: no
/// read-only profile offers Bash or approves it, and every one denies it by rule.
#[test]
fn no_read_only_argv_offers_bash_and_every_one_denies_it() {
    for research in [false, true] {
        let rig = Rig::new(&format!("nobash{research}"), ECHO_ARGV);
        let mut req = spawn_with(&rig.call(30), &Profile::read_only(rig.dir.clone(), research, 0.5, CAP), "p");
        assert_eq!(answer(&mut req), Ok("ok\n".into()));
        let args = argv_after_prompt(&rig);
        let value = |flag: &str| {
            let at = args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("no {flag}: {args:?}"));
            args[at + 1].split(',').map(String::from).collect::<Vec<_>>()
        };
        for flag in ["--tools", "--allowedTools"] {
            assert!(
                !value(flag).iter().any(|t| t.starts_with("Bash")),
                "{flag} offers Bash, research {research}"
            );
        }
        assert!(value("--disallowedTools").contains(&"Bash".to_string()), "research {research}");
    }
}

/// D11: a read-only call runs no hook, the user's or a plugin's: a hook is a
/// command, and oh-my-claudecode's wrote `.omc/` into the call's cwd. The panel's
/// text calls keep theirs, as the owner left them.
#[test]
fn read_only_turns_the_hooks_off_and_text_keeps_them() {
    for research in [false, true] {
        let p = Profile::read_only(PathBuf::from("/"), research, 0.5, CAP);
        assert!(!p.hooks, "research {research}");
    }
    let text = Profile::TEXT;
    assert!(text.hooks, "the panel's own calls keep their hooks");
    let rig = Rig::new("hooks", ECHO_ARGV);
    let mut req = spawn_with(&rig.call(30), &Profile::read_only(rig.dir.clone(), true, 0.5, CAP), "p");
    assert_eq!(answer(&mut req), Ok("ok\n".into()));
    let args = argv_after_prompt(&rig);
    let at = args.iter().position(|a| a == "--settings").expect("--settings");
    assert_eq!(args[at + 1], r#"{"disableAllHooks":true}"#);
}

/// No profile can give a call a folder as its cwd: there is no such field, and a
/// read-only call starts in the panel's cache dir with the repo added.
#[test]
fn a_read_only_call_starts_outside_the_repo() {
    let repo = PathBuf::from("/some/repo");
    let p = Profile::read_only(repo.clone(), true, 0.5, CAP);
    assert_eq!(p.read_dirs, [repo]);
    assert!(Profile::TEXT.read_dirs.is_empty());
}

/// review-432: a caller cannot forget the caps, as `read_only` takes them.
#[test]
fn read_only_carries_its_budget_and_timeout() {
    let p = Profile::read_only(PathBuf::from("/r"), false, 0.25, Duration::from_secs(7));
    assert_eq!((p.budget_usd, p.timeout), (Some(0.25), Some(Duration::from_secs(7))));
}
