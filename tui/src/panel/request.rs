//! One `claude -p` call on a thread of its own, writing nothing to tmux. Its
//! [`Profile`] says what it may use: [`Profile::TEXT`], the panel's calls, runs
//! no tools; [`Profile::read_only`] reads the repo (and, for research, the web
//! and skills) and can never write, run a command or start an agent (#365).
use std::fs::{DirBuilder, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fail {
    Timeout,
    /// The first line claude printed, stderr first, or why it could not start.
    Failed(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Fail::Timeout => f.write_str("took too long"),
            Fail::Failed(s) => f.write_str(s),
        }
    }
}

/// Where and how long; [`Call::from_env`] is the real one, tests build their own.
#[derive(Clone, Debug)]
pub struct Call {
    pub claude: String,
    pub timeout: Duration,
    /// The child's cwd, and the parent of the system prompt files.
    pub dir: PathBuf,
}

impl Call {
    pub fn from_env() -> Call {
        let secs = std::env::var("COCKPIT_PANEL_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|s| *s > 0);
        Call {
            claude: std::env::var("COCKPIT_CLAUDE")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "claude".into()),
            timeout: Duration::from_secs(secs.unwrap_or(120)),
            dir: super::xdg("XDG_CACHE_HOME", ".cache").join("tmux-cockpit/panel"),
        }
    }
}

/// What a call may use, and its limits. Every field is a `claude` flag:
/// `--tools` (the built-in set it has), `--allowedTools` (approved up front, as a
/// `-p` call cannot be asked), `--disallowedTools` (denied by rule as well, in case
/// a tool reaches the set some other way), `--permission-prompts none` (anything
/// that would still ask is refused, never waited on), `--max-budget-usd`.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub tools: &'static [&'static str],
    pub allow: &'static [&'static str],
    pub deny: &'static [&'static str],
    /// skills and slash commands; off is `--disable-slash-commands`
    pub skills: bool,
    /// `--permission-prompts none`
    pub no_prompts: bool,
    pub budget_usd: Option<f64>,
    /// over the call's own, for a call that reads before it answers
    pub timeout: Option<Duration>,
    /// the user's and plugins' hooks; off is `--settings {"disableAllHooks":true}`.
    /// A hook is a command whatever the tools are, and a plugin's writes into the cwd
    pub hooks: bool,
    /// Folders the file tools may read, passed as `--add-dir`: readable, never
    /// the cwd. In its cwd `claude -p` loads a folder's `.claude/settings.json`
    /// with no trust prompt, and its hooks, `env` and `apiKeyHelper` run
    /// (review-432); through `--add-dir` only its skills load (probed). The cwd
    /// is always the call's own dir.
    pub read_dirs: Vec<PathBuf>,
}

const READ: &[&str] = &["Read", "Grep", "Glob"];
const RESEARCH: &[&str] = &["Read", "Grep", "Glob", "WebSearch", "WebFetch", "Skill"];
/// Never in a read-only call. Agent is here too: the CLI cannot keep it to the
/// read-only Explore type (a rule can deny an agent type, not allow only one).
const WRITERS: &[&str] = &["Bash", "Edit", "Write", "NotebookEdit", "Agent"];

impl Profile {
    pub const TEXT: Profile = Profile {
        tools: &[],
        allow: &[],
        deny: &[],
        skills: false,
        no_prompts: false,
        budget_usd: None,
        timeout: None,
        hooks: true,
        read_dirs: Vec::new(),
    };

    /// Read the folder `repo`, added beside the cwd; with `research`, search and
    /// fetch the web and load skills too. The budget and the timeout are the
    /// owner's caps (#365 D7), so the type asks for both.
    pub fn read_only(repo: PathBuf, research: bool, budget_usd: f64, timeout: Duration) -> Profile {
        let tools = if research { RESEARCH } else { READ };
        Profile {
            tools,
            allow: tools,
            deny: WRITERS,
            skills: research,
            no_prompts: true,
            hooks: false,
            read_dirs: vec![repo],
            budget_usd: Some(budget_usd),
            timeout: Some(timeout),
        }
    }

    /// The flags after the system prompt file. [`Profile::TEXT`]'s are the panel's
    /// argv from before profiles, byte for byte.
    pub fn argv(&self) -> Vec<String> {
        let mut a = vec!["--tools".to_string(), self.tools.join(",")];
        if !self.allow.is_empty() {
            a.extend(["--allowedTools".into(), self.allow.join(",")]);
        }
        if !self.deny.is_empty() {
            a.extend(["--disallowedTools".into(), self.deny.join(",")]);
        }
        if self.no_prompts {
            a.extend(["--permission-prompts".into(), "none".into()]);
        }
        if !self.hooks {
            a.extend(["--settings".into(), r#"{"disableAllHooks":true}"#.into()]);
        }
        for d in &self.read_dirs {
            a.extend(["--add-dir".into(), d.display().to_string()]);
        }
        if let Some(b) = self.budget_usd {
            a.extend(["--max-budget-usd".into(), b.to_string()]);
        }
        a.extend(["--no-session-persistence".into(), "--strict-mcp-config".into()]);
        if !self.skills {
            a.push("--disable-slash-commands".into());
        }
        a
    }
}

type Answer = Result<String, Fail>;

pub struct Request {
    rx: Receiver<Answer>,
    pid: Option<u32>,
    /// True once the child is reaped: from then on its pid may be someone else's.
    reaped: Arc<Mutex<bool>>,
    done: bool,
}

impl Request {
    /// Never fails here: a spawn error arrives through [`Request::poll`].
    /// Call it from the main thread: PR_SET_PDEATHSIG fires when the spawning
    /// thread exits, so a short-lived thread would kill claude with it.
    pub fn spawn(call: &Call, system: &str, prompt: &str, model: &str) -> Request {
        Request::spawn_with(call, &Profile::TEXT, system, prompt, model)
    }

    /// [`Request::spawn`] under profile `p`.
    pub fn spawn_with(call: &Call, p: &Profile, system: &str, prompt: &str, model: &str) -> Request {
        let (tx, rx) = channel();
        let reaped = Arc::new(Mutex::new(false));
        let pid = match start(call, p, system, model) {
            Ok(child) => {
                let pid = child.id();
                run(child, prompt.to_string(), p.timeout.unwrap_or(call.timeout), tx, reaped.clone());
                Some(pid)
            }
            Err(e) => {
                let _ = tx.send(Err(Fail::Failed(e)));
                None
            }
        };
        Request {
            rx,
            pid,
            reaped,
            done: false,
        }
    }

    /// The answer once, then `None`; also `None` after [`Request::cancel`].
    pub fn poll(&mut self) -> Option<Answer> {
        if self.done {
            return None;
        }
        let a = self.rx.try_recv().ok()?;
        self.done = true;
        Some(a)
    }

    /// `None` once reaped, when the number may already be someone else's.
    /// Never signal it; [`Request::cancel`] is the only safe kill.
    pub fn pid(&self) -> Option<u32> {
        self.pid.filter(|_| !*lock(&self.reaped))
    }

    /// Kills the whole process group; the worker thread reaps it.
    pub fn cancel(&mut self) {
        self.done = true;
        if let Some(pid) = self.pid {
            kill_group(&self.reaped, pid);
        }
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn lock(m: &Mutex<bool>) -> std::sync::MutexGuard<'_, bool> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Under the lock the worker reaps with, so the pid is still our child, alive
/// or a zombie, and cannot have been handed to another process.
fn kill_group(reaped: &Mutex<bool>, pid: u32) {
    let g = lock(reaped);
    if !*g {
        // SAFETY: plain syscall; the group is ours while the leader is unreaped.
        unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) };
    }
    drop(g);
}

fn start(call: &Call, p: &Profile, system: &str, model: &str) -> Result<std::process::Child, String> {
    let file = prompt_file(&call.dir, system).map_err(|e| format!("cannot write the system prompt: {e}"))?;
    let mut c = Command::new(&call.claude);
    c.arg("-p")
        .args(super::route::model_argv(model))
        .arg("--system-prompt-file")
        .arg(&file)
        .args(p.argv())
        .current_dir(&call.dir)
        // The hooks key off TMUX_PANE: without it this call is no pane's agent.
        .env("COCKPIT_PANEL_CALL", "1")
        .env_remove("CLAUDECODE")
        .env_remove("TMUX_PANE")
        .env_remove("TMUX")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // A panel killed past Drop (abort, a torn-down popup) must not leave claude
    // running. The signal follows the spawning thread, so spawn from the main one.
    #[cfg(target_os = "linux")]
    {
        let parent = std::process::id() as libc::pid_t;
        // SAFETY: the closure runs between fork and exec and calls only prctl,
        // getppid and _exit, all async-signal-safe; it allocates nothing.
        unsafe {
            c.pre_exec(move || {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong);
                // A parent that died before the prctl sends no signal: we were reparented.
                if libc::getppid() != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
    }
    c.spawn().map_err(|e| format!("{}: {e}", call.claude))
}

/// `<dir>/prompts/<hash>.txt`, mode 600, written once per distinct text so the
/// file always holds exactly what was sent.
pub fn prompt_file(dir: &Path, system: &str) -> std::io::Result<PathBuf> {
    let d = dir.join("prompts");
    DirBuilder::new().recursive(true).mode(0o700).create(&d)?;
    let mut h = DefaultHasher::new();
    system.hash(&mut h);
    let path = d.join(format!("{:016x}.txt", h.finish()));
    if !path.exists() {
        // Threads of one process share the pid, so the counter keeps their temp names apart.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let tmp = d.join(format!(".prompt.{}.{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        if let Err(e) = f.write_all(system.as_bytes()).and_then(|()| std::fs::rename(&tmp, &path)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    Ok(path)
}

fn run(mut child: std::process::Child, prompt: String, timeout: Duration, tx: Sender<Answer>, reaped: Arc<Mutex<bool>>) {
    let pid = child.id();
    let (done_tx, done_rx) = channel::<()>();
    {
        let (tx, reaped) = (tx.clone(), reaped.clone());
        std::thread::spawn(move || {
            if done_rx.recv_timeout(timeout) == Err(RecvTimeoutError::Timeout) {
                let g = lock(&reaped);
                if !*g {
                    // Sent before the kill, so the worker's own report of the death comes second.
                    let _ = tx.send(Err(Fail::Timeout));
                    // SAFETY: as in kill_group, under the same lock.
                    unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) };
                }
            }
        });
    }
    std::thread::spawn(move || {
        if let Some(mut stdin) = child.stdin.take() {
            // A claude that exits without reading gets EPIPE here, which is not ours to report.
            std::thread::spawn(move || {
                let _ = stdin.write_all(prompt.as_bytes());
            });
        }
        let mut err_pipe = child.stderr.take();
        // Bytes, then lossy: read_to_string drops everything at the first bad byte.
        let errs = std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(e) = err_pipe.as_mut() {
                let _ = e.read_to_end(&mut b);
            }
            String::from_utf8_lossy(&b).into_owned()
        });
        let mut b = Vec::new();
        if let Some(mut o) = child.stdout.take() {
            let _ = o.read_to_end(&mut b);
        }
        let out = String::from_utf8_lossy(&b).into_owned();
        let err = errs.join().unwrap_or_default();
        wait_unreaped(pid);
        let mut g = lock(&reaped);
        let status = child.wait();
        *g = true;
        let _ = tx.send(answer(status.map(|s| s.success()).unwrap_or(false), out, &err));
        drop(g);
        let _ = done_tx.send(());
    });
}

/// Blocks until the child exits but leaves it a zombie, so its pid and group
/// stay ours until the reap under the lock.
fn wait_unreaped(pid: u32) {
    loop {
        // SAFETY: an all-zero siginfo_t is valid, and waitid only writes into it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is a live, writable siginfo_t; WNOWAIT reaps nothing.
        let r = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT) };
        if r == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}

fn first_line(s: &str) -> Option<String> {
    s.trim().lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
}

fn answer(ok: bool, out: String, err: &str) -> Answer {
    if ok && !out.trim().is_empty() {
        return Ok(out);
    }
    let why = first_line(err).or_else(|| first_line(&out)).unwrap_or_else(|| "no answer".into());
    Err(Fail::Failed(why))
}
