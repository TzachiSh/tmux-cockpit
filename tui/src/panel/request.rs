//! One `claude -p` call, text only, on a thread of its own. A read like data's
//! `claude agents` fork: it runs no tools and writes nothing to tmux.
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
    pub fn spawn(call: &Call, system: &str, prompt: &str, model: &str) -> Request {
        let (tx, rx) = channel();
        let reaped = Arc::new(Mutex::new(false));
        let pid = match start(call, system, model) {
            Ok(child) => {
                let pid = child.id();
                run(child, prompt.to_string(), call.timeout, tx, reaped.clone());
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

fn start(call: &Call, system: &str, model: &str) -> Result<std::process::Child, String> {
    let file = prompt_file(&call.dir, system).map_err(|e| format!("cannot write the system prompt: {e}"))?;
    let mut c = Command::new(&call.claude);
    c.arg("-p")
        .args(super::route::model_argv(model))
        .arg("--system-prompt-file")
        .arg(&file)
        .args([
            "--tools",
            "",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--disable-slash-commands",
        ])
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
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?
            .write_all(system.as_bytes())?;
        std::fs::rename(&tmp, &path)?;
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
