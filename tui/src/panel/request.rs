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

#[derive(Clone, Debug, PartialEq)]
pub enum Fail {
    Timeout,
    /// claude stopped at `--max-budget-usd`, in text or json: match this, not words.
    Budget {
        limit_usd: f64,
    },
    /// The first line claude printed, stderr first, or why it could not start.
    Failed(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Fail::Timeout => f.write_str("took too long"),
            Fail::Budget { limit_usd } => write!(f, "stopped at its budget of USD {limit_usd}"),
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
    /// `--effort`, from `COCKPIT_PANEL_EFFORT` (#623).
    pub effort: Effort,
}

/// `--effort`: one of the CLI's words, so the user's own text never reaches the
/// argv. The panel passes one on every call, as none of the user's settings
/// load, their `effortLevel` included (#623).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effort {
    Low,
    #[default]
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 5] = {
        use Effort::*;
        [Low, Medium, High, XHigh, Max]
    };

    /// The word as the CLI spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }

    /// `word` if it is one of the CLI's, exactly; anything else is `medium`.
    pub fn read(word: &str) -> Effort {
        Effort::ALL.into_iter().find(|e| e.as_str() == word).unwrap_or_default()
    }
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
            effort: Effort::read(&std::env::var("COCKPIT_PANEL_EFFORT").unwrap_or_default()),
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
    pub allow: Vec<String>,
    pub deny: &'static [&'static str],
    /// `--permission-mode`. Unset, the user's `defaultMode` decides what no rule
    /// covers, and `auto` approves a read outside the read dirs (probed).
    pub permission_mode: Option<&'static str>,
    pub output: Output,
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

/// `--output-format`: a text call's stdout is its answer; a json one's is its
/// events (`--verbose`), the answer their result, the rest kept for
/// [`Request::raw`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Text,
    Json,
}

const READ: &[&str] = &["Read", "Grep", "Glob"];
const RESEARCH: &[&str] = &["Read", "Grep", "Glob", "WebSearch", "WebFetch", "Skill"];
/// A read-only call's `--disallowedTools`. First the writers, never offered:
/// Agent too, as the CLI cannot keep it to the read-only Explore type (a rule
/// can deny an agent type, not allow only one). Then the secret paths, the
/// owner's call on review-432's finding 6: ssh, aws and gpg keys, Claude's own
/// credentials, and the token files of gh, git, netrc, docker, kube, npm, pypi
/// and gcloud: a Read rule covers Grep and Glob as
/// well and follows symlinks (probed). `~` is the home dir; `//` anchors at `/`,
/// as a bare `**/.env` is relative to the cwd (the cache dir) and would leave a
/// repo's `.env` readable. Every `.env` is denied, the repo's and `.env.example`
/// included: a code question has no need of them.
const DENY: &[&str] = &[
    "Bash",
    "Edit",
    "Write",
    "NotebookEdit",
    "Agent",
    "Read(~/.ssh/**)",
    "Read(~/.aws/**)",
    "Read(~/.gnupg/**)",
    "Read(~/.claude/.credentials.json)",
    "Read(~/.config/gh/**)",
    "Read(~/.git-credentials)",
    "Read(~/.netrc)",
    "Read(~/.docker/config.json)",
    "Read(~/.kube/**)",
    "Read(~/.npmrc)",
    "Read(~/.pypirc)",
    "Read(~/.config/gcloud/**)",
    "Read(//**/.env)",
    "Read(//**/.env.*)",
];

/// `path` matches one of the read-only profile's Read denies ([`DENY`]), so a
/// read-only call refuses to read it. `~` is `home`, `//` anchors at `/`, `**`
/// is any run of folders and `*` any run of characters in one name. A rule
/// relative to the cwd (none today) never matches: the cwd is the cache dir.
/// A rule that matches a folder on the path denies the file too: `x/.env/key`
/// is refused under `Read(//**/.env)` ("File is in a directory that is denied",
/// probed, #586), so each folder is held against the rule, the file last.
/// `path` must be canonical, as a ref's is: a `..` is dropped, not followed, so
/// `/r/.env/../key` would read as denied when it is not.
pub fn read_denied(path: &Path, home: Option<&Path>) -> bool {
    let names = |p: &Path| -> Vec<String> {
        let normal = |c: std::path::Component<'_>| match c {
            std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        };
        p.components().filter_map(normal).collect()
    };
    let path = names(path);
    let mut rules = DENY.iter().filter_map(|d| d.strip_prefix("Read(")?.strip_suffix(')'));
    rules.any(|rule| {
        let full = match (rule.strip_prefix("//"), rule.strip_prefix("~/"), home) {
            (Some(r), ..) => Path::new("/").join(r),
            (_, Some(r), Some(h)) => h.join(r),
            _ => return false,
        };
        let rule = names(&full);
        let rule: Vec<&str> = rule.iter().map(String::as_str).collect();
        let path: Vec<&str> = path.iter().map(String::as_str).collect();
        (1..=path.len()).any(|k| glob_names(&rule, &path[..k]))
    })
}

/// Name by name: `**` takes any run of names, none included.
fn glob_names(rule: &[&str], path: &[&str]) -> bool {
    match rule.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|i| glob_names(rest, &path[i..])),
        Some((r, rest)) => path
            .split_first()
            .is_some_and(|(n, more)| glob_name(r, n) && glob_names(rest, more)),
    }
}

/// One name: `*` takes any run of characters, none included.
fn glob_name(rule: &str, name: &str) -> bool {
    match rule.split_once('*') {
        None => rule == name,
        Some((head, tail)) => name.strip_prefix(head).is_some_and(|rest| {
            (0..=rest.len())
                .filter(|i| rest.is_char_boundary(*i))
                .any(|i| glob_name(tail, &rest[i..]))
        }),
    }
}

impl Profile {
    pub const TEXT: Profile = Profile {
        tools: &[],
        allow: Vec::new(),
        deny: &[],
        permission_mode: None,
        output: Output::Text,
        skills: false,
        no_prompts: false,
        budget_usd: None,
        timeout: None,
        hooks: true,
        read_dirs: Vec::new(),
    };

    /// Read the folder `repo` and nothing outside it, added beside the cwd; with
    /// `research`, search and fetch the web and load skills too. The budget and
    /// the timeout are the owner's caps (#365 D7), so the type asks for both.
    /// The mode is `dontAsk`: the read dirs are readable and any other read is
    /// refused, no allow rule needed (docs/tui.md, probed).
    pub fn read_only(repo: PathBuf, research: bool, budget_usd: f64, timeout: Duration) -> Profile {
        let tools = if research { RESEARCH } else { READ };
        // absolute: never read against the cache dir, never a `-` that passes for a flag
        let repo = std::path::absolute(&repo).unwrap_or(repo);
        // never Read, Grep or Glob: a bare Grep or Glob here approved reads outside (probed)
        let allow = if research {
            ["WebSearch", "WebFetch", "Skill"].map(String::from).to_vec()
        } else {
            vec![]
        };
        Profile {
            tools,
            allow,
            deny: DENY,
            permission_mode: Some("dontAsk"),
            output: Output::Json,
            skills: research,
            no_prompts: true,
            hooks: false,
            read_dirs: vec![repo],
            budget_usd: Some(budget_usd),
            timeout: Some(timeout),
        }
    }

    /// What the system prompt adds for a call with read dirs: its cwd is not the
    /// repo, so the model is told where the repo is and how the tools reach it.
    /// Each path is quoted; one with a control or bidi character never gets here
    /// (`start` refuses it), as it would put text in the prompt with its trust.
    pub fn where_note(&self) -> Option<String> {
        let dirs: Vec<String> = self.read_dirs.iter().map(|d| format!("{:?}", d.display().to_string())).collect();
        (!dirs.is_empty()).then(|| {
            format!(
                "The repository is at {}. Read, Grep and Glob take absolute paths under it, and anything outside it is refused; your working directory is not the repository.",
                dirs.join(" and ")
            )
        })
    }

    /// The flags after the system prompt file, at `effort`. Every profile loads
    /// no settings source (`--setting-sources ""`): no user, project or local
    /// settings, and with them no CLAUDE.md, hooks, plugins or `effortLevel`.
    /// Managed settings and `--settings` still apply (#623).
    pub fn argv(&self, effort: Effort) -> Vec<String> {
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
        if let Some(m) = self.permission_mode {
            a.extend(["--permission-mode".into(), m.into()]);
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
        if self.output == Output::Json {
            a.extend(["--output-format".into(), "json".into(), "--verbose".into()]);
        }
        a.extend(["--no-session-persistence".into(), "--strict-mcp-config".into()]);
        a.extend(["--setting-sources".into(), String::new()]);
        a.extend(["--effort".into(), effort.as_str().into()]);
        if !self.skills {
            a.push("--disable-slash-commands".into());
        }
        a
    }
}

type Answer = Result<String, Fail>;

/// An answer, and a json call's stdout.
type Done = (Answer, Option<String>);

pub struct Request {
    rx: Receiver<Done>,
    raw: Option<String>,
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
                let reading = Reading {
                    output: p.output,
                    budget_usd: p.budget_usd,
                };
                run(
                    child,
                    prompt.to_string(),
                    p.timeout.unwrap_or(call.timeout),
                    reading,
                    tx,
                    reaped.clone(),
                );
                Some(pid)
            }
            Err(e) => {
                let _ = tx.send((Err(Fail::Failed(e)), None));
                None
            }
        };
        Request {
            rx,
            raw: None,
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
        let (a, raw) = self.rx.try_recv().ok()?;
        self.done = true;
        self.raw = raw;
        Some(a)
    }

    /// A json call's stdout, its events, once [`Request::poll`] gave its answer,
    /// a failed one too: the input to `parse::files_read`. `None` for a text
    /// call, and for one that timed out or never started.
    pub fn raw(&self) -> Option<&str> {
        self.raw.as_deref()
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

/// The whole check on a read dir, the one [`Request::spawn_with`] runs: a caller
/// may run it first to refuse up front. The error is a line for the user.
///
/// A name goes into the system prompt, so a control or bidi character is refused,
/// never cleaned, as the call would read some other folder; so is an empty name
/// and one not UTF-8. `--add-dir` of `/`, home or a folder above home opens all of
/// home, a link to one included. A glob character is refused though no rule holds
/// the path today, in case one does again.
pub fn read_dir_ok(dir: &Path) -> Result<(), String> {
    let shown = format!("{:?}", dir.display().to_string());
    let Some(s) = dir.to_str() else {
        return Err(format!("{shown} is not UTF-8; refused"));
    };
    if s.is_empty() {
        return Err("the folder name is empty; refused".into());
    }
    if crate::pick::clean(s) != s {
        return Err(format!("{shown} holds a control or bidi character; refused"));
    }
    if s.contains(['*', '?', '[', ']', '{', '}', '\\']) {
        return Err(format!("{shown} holds a glob character; refused"));
    }
    let both = |p: &Path| [Some(lexical(p)), std::fs::canonicalize(p).ok()].into_iter().flatten();
    if both(dir).any(|d| d == Path::new("/")) {
        return Err("the root folder holds every file; refused".into());
    }
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    if let Some(h) = home
        && both(dir).any(|d| both(&h).any(|h| h.starts_with(&d)))
    {
        return Err(format!("{shown} is your home folder or holds it; refused"));
    }
    Ok(())
}

/// `p` absolute, `.` and `..` folded without touching the disk.
fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for c in std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()).components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Normal(n) => out.push(n),
            _ => {}
        }
    }
    out
}

fn start(call: &Call, p: &Profile, system: &str, model: &str) -> Result<std::process::Child, String> {
    for d in &p.read_dirs {
        read_dir_ok(d)?;
    }
    let system = match p.where_note() {
        Some(n) => format!("{system}\n\n{n}"),
        None => system.to_string(),
    };
    let file = prompt_file(&call.dir, &system).map_err(|e| format!("cannot write the system prompt: {e}"))?;
    let mut c = Command::new(&call.claude);
    c.arg("-p")
        .args(super::route::model_argv(model))
        .arg("--system-prompt-file")
        .arg(&file)
        .args(p.argv(call.effort))
        .current_dir(&call.dir)
        // The hooks key off TMUX_PANE: without it this call is no pane's agent.
        .env("COCKPIT_PANEL_CALL", "1")
        .env_remove("CLAUDECODE")
        // claude ranks it over --effort: the panel's own option decides (#623)
        .env_remove("CLAUDE_CODE_EFFORT_LEVEL")
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

/// What [`answer`] needs of the profile.
struct Reading {
    output: Output,
    budget_usd: Option<f64>,
}

fn run(mut child: std::process::Child, prompt: String, timeout: Duration, reading: Reading, tx: Sender<Done>, reaped: Arc<Mutex<bool>>) {
    let pid = child.id();
    let (done_tx, done_rx) = channel::<()>();
    {
        let (tx, reaped) = (tx.clone(), reaped.clone());
        std::thread::spawn(move || {
            if done_rx.recv_timeout(timeout) == Err(RecvTimeoutError::Timeout) {
                let g = lock(&reaped);
                if !*g {
                    // Sent before the kill, so the worker's own report of the death comes second.
                    let _ = tx.send((Err(Fail::Timeout), None));
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
        let ok = status.map(|s| s.success()).unwrap_or(false);
        let done = match reading.output {
            Output::Json => (answer_json(ok, &out, &err, reading.budget_usd), Some(out)),
            Output::Text => (answer(ok, out, &err).map_err(|f| budget(f, reading.budget_usd)), None),
        };
        let _ = tx.send(done);
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

/// A json call's answer: its result's text, or its error in claude's words. A
/// budget stop, `"Reached maximum budget ($0.5)"` (probed), is [`Fail::Budget`].
/// Output that is not claude's json (a stand-in claude) is read as text.
fn answer_json(ok: bool, out: &str, err: &str, budget_usd: Option<f64>) -> Answer {
    use serde_json::Value;
    let r = match serde_json::from_str::<Value>(out.trim()) {
        Ok(Value::Array(a)) => a.into_iter().rev().find(|e| e["type"] == "result"),
        Ok(v @ Value::Object(_)) if v["type"] == "result" => Some(v),
        _ => None,
    };
    let Some(r) = r else {
        return answer(ok, out.to_string(), err);
    };
    if r["subtype"] == "error_max_budget_usd" {
        let said = r["errors"][0].as_str().and_then(limit_in);
        if let Some(limit_usd) = said.or(budget_usd) {
            return Err(Fail::Budget { limit_usd });
        }
    }
    let text = r["result"].as_str().filter(|t| !t.trim().is_empty());
    match text {
        Some(t) if r["is_error"] != true => Ok(t.to_string()),
        _ if r["is_error"] != true => Err(Fail::Failed("no answer".into())),
        _ => {
            let first = r["errors"][0].as_str().or(text).or(r["subtype"].as_str());
            Err(Fail::Failed(first.and_then(first_line).unwrap_or_else(|| "no answer".into())))
        }
    }
}

/// A text call's budget stop, `Error: Exceeded USD budget (0.5)` (probed), as
/// [`Fail::Budget`]; any other failure as it is.
fn budget(f: Fail, budget_usd: Option<f64>) -> Fail {
    match &f {
        Fail::Failed(t) if t.to_ascii_lowercase().contains("exceeded usd budget") => {
            limit_in(t).or(budget_usd).map_or(f, |limit_usd| Fail::Budget { limit_usd })
        }
        _ => f,
    }
}

/// The number in the last `(...)` of claude's budget line, `$` or not.
fn limit_in(line: &str) -> Option<f64> {
    let (_, tail) = line.rsplit_once('(')?;
    tail.split_once(')')?.0.trim().trim_start_matches('$').parse().ok()
}
