//! The `claude -p` call against fake claude scripts. Each test builds its own
//! `Call`, so nothing here touches the process environment.
use cockpit_tui::panel::request::{Call, Fail, Request, prompt_file};
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
