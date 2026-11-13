//! The target pane as the panel sees it, and what a Claude Code session on it
//! is doing, read off its own UI text.
use crate::data::gitsafe::{FILTER_CONFIG, REDIRECT, SAFE, filter_flags};
use crate::data::tmux;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capture {
    pub name: String,
    pub cmd: String,
    pub path: String,
    /// Raw, not redacted: the envelope redacts it on the way out.
    pub screen: String,
    /// [`repo_state_within`] of `path`, raw like the screen; "" outside a repo.
    pub repo: Repo,
}

/// The command in any case, or its version-shaped title, as the board reads it
/// ([`crate::basket::claude_cmd`], #760); or a window named claude.
pub fn is_claude(name: &str, cmd: &str) -> bool {
    crate::basket::claude_cmd(cmd) || name.eq_ignore_ascii_case("claude")
}

/// `None` when tmux cannot answer for the pane.
pub fn capture(pane: &str) -> Option<Capture> {
    capture_within(pane, REPO_BUDGET)
}

/// [`capture`] with the repo block under `budget`: a test checking the block's
/// parts waits longer than the panel would.
pub fn capture_within(pane: &str, budget: Duration) -> Option<Capture> {
    let info = tmux(&[
        "display",
        "-p",
        "-t",
        pane,
        "#{window_name}\t#{pane_current_command}\t#{pane_current_path}",
    ])?;
    // Only the newline goes: a trim would eat the tab of an empty window name.
    let mut f = info.trim_end_matches(['\n', '\r']).splitn(3, '\t').map(String::from);
    let (name, cmd, path) = (
        f.next().unwrap_or_default(),
        f.next().unwrap_or_default(),
        f.next().unwrap_or_default(),
    );
    // A session's last answer and plan sit further back than a shell's last command.
    let depth = if is_claude(&name, &cmd) { "-200" } else { "-80" };
    let out = tmux(&["capture-pane", "-p", "-J", "-t", pane, "-S", depth])?;
    let mut lines: Vec<&str> = out.trim_end_matches('\n').split('\n').collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let screen = lines.join("\n");
    let repo = if path.is_empty() || !is_local(&cmd) {
        Repo::default()
    } else {
        Repo::start(&path, &screen, budget)
    };
    Some(Capture {
        name,
        cmd,
        path,
        screen,
        repo,
    })
}

/// The whole repo block's time: a slow or huge repo costs the panel this much.
pub const REPO_BUDGET: Duration = Duration::from_millis(300);
/// What [`Repo::get`] waits past the budget for the worker's last send.
const REPO_SLACK: Duration = Duration::from_millis(50);
const STATUS_CAP: usize = 15;
const STAT_CAP: usize = 10;
const PATHS_CAP: usize = 10;

/// The repo block, filled by a thread [`capture`] starts, so the first frame
/// and the Explain and Fix calls never wait for git. [`Repo::get`] waits for
/// it, never past the budget and its slack.
#[derive(Clone, Default)]
pub struct Repo(Arc<RepoCell>);

#[derive(Default)]
struct RepoCell {
    pending: Mutex<Option<(Receiver<String>, Instant)>>,
    done: OnceLock<String>,
}

impl Repo {
    /// A block already known: a test's, or the eval's.
    pub fn ready(s: impl Into<String>) -> Repo {
        let c = RepoCell::default();
        let _ = c.done.set(s.into());
        Repo(Arc::new(c))
    }

    /// [`repo_state_within`] on its own thread.
    pub fn start(dir: &str, screen: &str, budget: Duration) -> Repo {
        let end = Instant::now() + budget;
        let (tx, rx) = channel();
        let (dir, screen) = (dir.to_string(), screen.to_string());
        std::thread::spawn(move || repo_work(&dir, &screen, end, &|b| drop(tx.send(b))));
        let c = RepoCell {
            pending: Mutex::new(Some((rx, end + REPO_SLACK))),
            done: OnceLock::new(),
        };
        Repo(Arc::new(c))
    }

    /// The block, or the last part of it the worker sent before the deadline.
    pub fn get(&self) -> &str {
        self.0.done.get_or_init(|| {
            let taken = self.0.pending.lock().unwrap_or_else(|e| e.into_inner()).take();
            let Some((rx, end)) = taken else {
                return String::new();
            };
            let mut last = String::new();
            while let Ok(b) = rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
                last = b;
            }
            last
        })
    }
}

impl From<&str> for Repo {
    fn from(s: &str) -> Repo {
        Repo::ready(s)
    }
}

impl From<String> for Repo {
    fn from(s: String) -> Repo {
        Repo::ready(s)
    }
}

impl PartialEq for Repo {
    fn eq(&self, o: &Repo) -> bool {
        self.get() == o.get()
    }
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Repo").field(&self.get()).finish()
    }
}

/// A child started by [`bounded`], and its stdout once that closes.
pub(crate) struct Git {
    child: Child,
    out: Receiver<Vec<u8>>,
}

/// Every call's flags past gitsafe's: no colour, whatever the user's or the
/// repo's config says.
const PLAIN: [&str; 6] = ["-c", "color.ui=false", "-c", "color.status=false", "-c", "color.diff=false"];

/// gitsafe's flags, no lock taken under the user's own git, no prompt, no
/// transport; started by [`bounded`]. The repo is the one `dir` is in,
/// whatever [`REDIRECT`] the panel inherited.
pub(crate) fn spawn_git(dir: &str, extra: &[String], args: &[&str]) -> Option<Git> {
    let mut c = Command::new("git");
    for v in REDIRECT {
        c.env_remove(v);
    }
    c.arg("-C")
        .arg(dir)
        .args(SAFE)
        .args(PLAIN)
        .args(extra)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", "");
    bounded(c).ok()
}

/// `c` started with no stdin and no stderr, in its own process group so a
/// timeout kills what it forked, its stdout read on a thread of its own.
pub(crate) fn bounded(mut c: Command) -> std::io::Result<Git> {
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).process_group(0);
    // The panel quitting mid-block takes its git calls with it.
    #[cfg(target_os = "linux")]
    {
        let parent = std::process::id() as libc::pid_t;
        // SAFETY: the closure runs between fork and exec and calls only prctl,
        // getppid and _exit, all async-signal-safe; it allocates nothing.
        unsafe {
            c.pre_exec(move || {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong);
                if libc::getppid() != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
    }
    let mut child = c.spawn()?;
    let mut stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no stdout pipe"))?;
    let (tx, out) = channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        let _ = tx.send(b);
    });
    Ok(Git { child, out })
}

/// A call dropped unfinished, on an early return say, is killed with its group.
impl Drop for Git {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            // Not reaped yet, so the pid and its group are still this child's.
            // SAFETY: killpg takes a plain integer and touches no memory.
            unsafe { libc::killpg(self.child.id() as libc::pid_t, libc::SIGKILL) };
            let _ = self.child.wait();
        }
    }
}

/// How a call ended.
pub(crate) enum Ran {
    Ok(String),
    Failed,
    TimedOut,
}

impl Ran {
    pub(crate) fn ok(self) -> Option<String> {
        match self {
            Ran::Ok(s) => Some(s),
            _ => None,
        }
    }
}

/// Its stdout if it exited 0 by `end`; killed with its group otherwise. Prints
/// nothing.
pub(crate) fn wait(g: Option<Git>, end: Instant) -> Ran {
    let Some(mut g) = g else {
        return Ran::Failed;
    };
    let out = g.out.recv_timeout(end.saturating_duration_since(Instant::now())).ok();
    // stdout closed is not exited: wait out what is left of the budget
    while let Some(b) = &out {
        match g.child.try_wait() {
            Ok(Some(st)) if st.success() => return Ran::Ok(String::from_utf8_lossy(b).into()),
            Ok(Some(_)) => return Ran::Failed,
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(1)),
            _ => break,
        }
    }
    drop(g); // the kill
    Ran::TimedOut
}

/// [`wait`], and a timeout logged with `part`, the panel's stderr being tui.log.
fn finish(g: Option<Git>, end: Instant, part: &str) -> Ran {
    let ran = wait(g, end);
    if matches!(ran, Ran::TimedOut) {
        eprintln!("cockpit-tui: repo block: git {part} timed out, left out");
    }
    ran
}

/// The work tree of the repo `dir` is in, under [`spawn_git`]'s flags, or `None`
/// outside a repo, past `budget`, or when it is not `dir`'s own (see [`own_tree`]).
pub fn toplevel(dir: &str, budget: Duration) -> Option<String> {
    let args = ["rev-parse", "--show-toplevel", "--absolute-git-dir"];
    let g = spawn_git(dir, &[], &args);
    let out = finish(g, Instant::now() + budget, "rev-parse --show-toplevel").ok()?;
    let mut lines = out.lines();
    let (top, gitdir) = (lines.next()?, lines.next()?);
    own_tree(Path::new(dir), Path::new(top), Path::new(gitdir)).then(|| top.to_string())
}

/// `top` holds `dir`, and its own `.git` (a folder, or a `gitdir:` file as in a
/// linked worktree or a submodule) is the repo's git dir: a repo's `core.worktree`
/// can name any folder as its tree, a parent repo or one outside, and is not trusted.
fn own_tree(dir: &Path, top: &Path, gitdir: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).ok();
    let (Some(dir), Some(top), Some(gitdir)) = (canon(dir), canon(top), canon(gitdir)) else {
        return false;
    };
    if !dir.starts_with(&top) {
        return false;
    }
    let dot = top.join(".git");
    let own = match std::fs::symlink_metadata(&dot) {
        Ok(m) if m.is_dir() => canon(&dot),
        Ok(m) if m.is_file() => {
            let body = std::fs::read_to_string(&dot).ok();
            let named = body.as_deref().and_then(|b| b.strip_prefix("gitdir:")).map(|p| top.join(p.trim()));
            named.and_then(|p| canon(&p))
        }
        _ => None,
    };
    own == Some(gitdir)
}

/// `lines` indented under `head`, the first `cap` of them and a count of the rest.
fn section(b: &mut String, head: &str, lines: &[&str], cap: usize) {
    b.push_str(head);
    b.push('\n');
    for l in lines.iter().take(cap) {
        b.push_str(&format!("  {l}\n"));
    }
    if lines.len() > cap {
        b.push_str(&format!("  and {} more\n", lines.len() - cap));
    }
}

/// The git state of `dir` for the write call's `<repo>` block: branch, the
/// work tree's root, short status, the last five commits, the diff stat
/// against HEAD, and the paths on `screen` that exist, plus the `@/`, `~/` and
/// `./` ones that do not. Plain git under `budget` (the panel's is
/// [`REPO_BUDGET`]).
///
/// "" when `dir` is not in a work tree, when the root or the filter config is
/// not read in time, when a filter name holds `=` ([`filter_flags`] gives None,
/// so no git that could run a filter is started), and when status fails.
/// A late status shows as `status: timed out`. A log or diff stat that is late
/// or fails (a repo with no commits yet has neither) is left out. A path scan
/// that runs out of time keeps the paths it checked by then, but one stat that
/// hangs past `budget` plus `REPO_SLACK` loses every path: the scan checks the
/// time only between paths, and the block holding them comes too late.
pub fn repo_state_within(dir: &str, screen: &str, budget: Duration) -> String {
    Repo::start(dir, screen, budget).get().to_string()
}

/// The block, sent once git is done and again with the screen's paths: a
/// stat that hangs (NFS, an automount) costs the paths, not the git part.
fn repo_work(dir: &str, screen: &str, end: Instant, send: &dyn Fn(String)) {
    if dir.is_empty() {
        return;
    }
    // Shows the root and that `dir` is in a work tree; it runs no filter.
    let top = spawn_git(dir, &[], &["rev-parse", "--show-toplevel"]);
    // `git config` exits 1 when nothing matches, so only a timeout fails here.
    let cfg = match finish(spawn_git(dir, &[], &FILTER_CONFIG), end, "config") {
        Ran::Ok(s) => s,
        Ran::Failed => String::new(),
        Ran::TimedOut => return,
    };
    let Some(mut filters) = filter_flags(&cfg) else {
        return;
    };
    // A diff refreshes the index and writes it back, under the lock, on a
    // stat-dirty file; a kill in mid-write would leave index.lock behind.
    let keep = ["core.quotePath=false", "status.relativePaths=false", "diff.autoRefreshIndex=false"];
    filters.extend(keep.iter().flat_map(|k| ["-c".to_string(), k.to_string()]));
    let sub = "--ignore-submodules=all"; // a submodule's own filters would run
    let status = spawn_git(dir, &filters, &["status", "--short", "--branch", sub]);
    let log = spawn_git(dir, &filters, &["log", "-5", "--oneline", "--no-decorate"]);
    let diff = spawn_git(dir, &filters, &["diff", "--stat", "--no-ext-diff", "--no-textconv", sub, "HEAD"]);
    let Some(top) = finish(top, end, "rev-parse").ok() else {
        return;
    };
    let (status, log, diff) = (finish(status, end, "status"), finish(log, end, "log"), finish(diff, end, "diff"));
    let log = log.ok().filter(|l| !l.trim().is_empty());
    let sha = log.as_deref().and_then(|l| l.split_whitespace().next());
    let mut b = String::new();
    match status {
        Ran::Ok(status) => {
            let mut lines = status.lines();
            let head = lines.next().and_then(|l| l.strip_prefix("## ")).unwrap_or("");
            let branch = match (head, sha) {
                ("HEAD (no branch)", Some(sha)) => format!("detached at {sha}"),
                ("HEAD (no branch)", None) => "detached".into(),
                (h, _) => match h.strip_prefix("No commits yet on ") {
                    Some(b) => format!("{b} (no commits yet)"),
                    None => h.into(),
                },
            };
            b.push_str(&format!("branch: {branch}\nroot: {}\n", top.trim()));
            let changed: Vec<&str> = lines.collect();
            if changed.is_empty() {
                b.push_str("status: clean\n");
            } else {
                section(&mut b, "status:", &changed, STATUS_CAP);
            }
        }
        Ran::TimedOut => b.push_str(&format!("branch: unknown\nroot: {}\nstatus: timed out\n", top.trim())),
        Ran::Failed => return,
    }
    if let Some(log) = log {
        section(&mut b, "recent commits:", &log.lines().collect::<Vec<_>>(), 5);
    }
    if let Some(diff) = diff.ok().filter(|d| !d.trim().is_empty()) {
        let all: Vec<&str> = diff.lines().collect();
        if let Some((summary, files)) = all.split_last() {
            section(&mut b, "diff --stat:", files, STAT_CAP);
            b.push_str(&format!("  {summary}\n"));
        }
    }
    send(b.trim_end().to_string());
    let paths = screen_paths(screen);
    let home = std::env::var("HOME").unwrap_or_default();
    let mut seen: Vec<String> = Vec::new();
    for t in &paths {
        if Instant::now() >= end {
            break;
        }
        let bare = t.strip_prefix('@').unwrap_or(t);
        let p = match bare.strip_prefix("~/") {
            Some(r) => format!("{home}/{r}"),
            None if bare.starts_with('/') => bare.to_string(),
            None => Path::new(dir).join(bare).display().to_string(),
        };
        if Path::new(&p).exists() {
            seen.push(format!("exists: {t}"));
        } else if ["@/", "~/", "./"].iter().any(|x| t.starts_with(x)) {
            // a bare `dir/name.ext` that is not there is most likely not a path
            seen.push(format!("missing: {t}"));
        }
    }
    if !seen.is_empty() {
        let seen: Vec<&str> = seen.iter().map(String::as_str).collect();
        section(&mut b, "paths on screen:", &seen, PATHS_CAP);
        send(b.trim_end().to_string());
    }
}

/// `name.ext`, an extension of one to eight letters or digits, not all digits.
fn has_ext(file: &str) -> bool {
    file.rsplit_once('.').is_some_and(|(n, e)| {
        let alnum = e.bytes().all(|c| c.is_ascii_alphanumeric());
        let number = n.bytes().chain(e.bytes()).all(|c| c.is_ascii_digit());
        !n.is_empty() && (1..=8).contains(&e.len()) && alnum && !number
    })
}

/// Tokens on `screen` that look like files: `@/abs`, `~/x`, `./x`, and any
/// `dir/name.ext`; quotes, brackets and a `:line:col` tail stripped. A range
/// (`a..b`), a host (`github.com/x`), a URL and a fraction are not taken. The
/// first [`PATHS_CAP`] distinct ones, in screen order.
pub fn screen_paths(screen: &str) -> Vec<String> {
    let mut got: Vec<String> = Vec::new();
    for w in screen.split_whitespace() {
        let mut t = w
            .trim_start_matches(|c| "\"'`([{<".contains(c))
            .trim_end_matches(|c| "\"'`)]}>,;.:".contains(c));
        while let Some((l, r)) = t.rsplit_once(':')
            && !r.is_empty()
            && r.bytes().all(|c| c.is_ascii_digit())
        {
            t = l;
        }
        // `:` is not pathy, so a URL never gets past this
        let pathy = t.chars().all(|c| c.is_alphanumeric() || "._/-+@~".contains(c)) && !t.contains("..");
        let prefixed = ["@/", "~/", "./"].iter().any(|p| t.len() > p.len() && t.starts_with(p));
        // a first segment with a dot is a host: github.com/foo/bar.git
        let host = t.split_once('/').is_some_and(|(h, _)| h.contains('.') && h != ".");
        let named = !host && t.rsplit_once('/').is_some_and(|(_, f)| has_ext(f));
        if pathy && (prefixed || named) && !got.iter().any(|g| g == t) {
            got.push(t.to_string());
            if got.len() == PATHS_CAP {
                break;
            }
        }
    }
    got
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    WaitingChoice,
    Working,
    Error,
    Asking,
    Ready,
}

impl Session {
    /// The `state` attribute the explain prompt documents.
    pub fn as_str(self) -> &'static str {
        match self {
            Session::WaitingChoice => "waiting_choice",
            Session::Working => "working",
            Session::Error => "error",
            Session::Asking => "asking",
            Session::Ready => "ready",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Session::WaitingChoice => "is waiting for the user to pick an option in a permission or choice menu",
            Session::Working => "is working on a task right now",
            Session::Error => "stopped with an error",
            Session::Asking => "finished its turn with a question to the user",
            Session::Ready => "is idle, waiting for the next prompt",
        }
    }
}

/// From the screen, not `@panestate`: the hooks cannot tell a menu from a
/// question, and the prompt's five states need that split.
pub fn session_state(screen: &str, name: &str, cmd: &str) -> Option<Session> {
    if !is_claude(name, cmd) {
        return None;
    }
    let all: Vec<&str> = screen.split('\n').filter(|l| !l.trim().is_empty()).collect();
    let tail = &all[all.len().saturating_sub(25)..];
    if waiting_choice(tail) {
        return Some(Session::WaitingChoice);
    }
    if working(tail) {
        return Some(Session::Working);
    }
    if errored(tail) {
        return Some(Session::Error);
    }
    let said = tail.iter().map(|l| l.trim()).rfind(|l| l.starts_with('●'));
    Some(if said.is_some_and(|l| l.ends_with('?')) {
        Session::Asking
    } else {
        Session::Ready
    })
}

/// `N.` then whitespace or the end of the line.
fn numbered(l: &str, n: char) -> Option<&str> {
    let r = l.trim_start().strip_prefix(n)?.strip_prefix('.')?;
    (r.is_empty() || r.starts_with(char::is_whitespace)).then_some(r)
}

fn waiting_choice(tail: &[&str]) -> bool {
    const ASKS: [&str; 5] = ["proceed", "make this edit", "create", "run", "allow"];
    let asked = tail.iter().any(|l| {
        l.match_indices("Do you want to ")
            .any(|(i, m)| ASKS.iter().any(|a| l[i + m.len()..].starts_with(a)))
    });
    let cursor = tail.iter().any(|l| {
        let t = l.trim_start();
        t.strip_prefix(['❯', '>']).is_some_and(|r| numbered(r, '1').is_some())
    });
    let yes_no = tail.windows(2).any(|w| {
        let yes = numbered(w[0], '1').is_some_and(|r| {
            let t = r.trim_start();
            t.len() < r.len() && t.starts_with("Yes") && super::boundary_at(t, 3)
        });
        yes && numbered(w[1], '2').is_some()
    });
    asked || cursor || yes_no
}

fn working(tail: &[&str]) -> bool {
    tail.iter().any(|l| {
        if l.contains("esc to interrupt") {
            return true;
        }
        // `(12s ·`, the elapsed counter of a running turn.
        let timer = l.match_indices('(').any(|(i, _)| {
            let r = &l[i + 1..];
            let d = r.bytes().take_while(u8::is_ascii_digit).count();
            d > 0 && r[d..].strip_prefix('s').is_some_and(|r| r.trim_start().starts_with('·'))
        });
        // `✻ Thinking…`, the spinner line.
        let t = l.trim_start();
        let spinner = t.strip_prefix(['✻', '✶', '✳', '✢', '✽', '·', '*']).is_some_and(|r| {
            let w = r.trim_start();
            let word: usize = w.chars().take_while(|c| super::is_word(*c)).map(char::len_utf8).sum();
            w.len() < r.len() && word > 0 && w[word..].starts_with('…')
        });
        timer || spinner
    })
}

fn errored(tail: &[&str]) -> bool {
    tail.iter().any(|l| {
        let low = l.to_lowercase();
        let bullet = l
            .trim_start()
            .strip_prefix('⎿')
            .is_some_and(|r| r.trim_start().to_lowercase().starts_with("error"));
        low.contains("api error") || low.contains("request failed") || low.contains("rate limit") || bullet
    })
}

/// The commands that make a window an ssh session: a remote shell on the far side.
pub const SSH_LIKE: [&str; 6] = ["ssh", "proot", "coder", "mosh", "mosh-client", "autossh"];

/// The commands that run in the folder tmux reports for them: with the board's
/// agent names (a Claude renamed to its version, say), the only ones
/// [`capture`] sends a repo block for (#429). Anything else may be the client
/// of a shell elsewhere: ssh, `docker exec`, `kubectl exec`, `incus exec`, and
/// whatever comes next. Such a pane loses the block rather than carry the wrong
/// machine's. `node` is Claude Code, Codex or Gemini installed from npm.
/// The limit: tmux names the foreground process, so a shell script that runs
/// ssh or `docker exec` as a child reads as its interpreter, `bash`, and local.
// rows by kind: shells, editors, agents. rustfmt would give each its own line.
#[rustfmt::skip]
pub const LOCAL_CMDS: [&str; 30] = [
    "bash", "zsh", "fish", "sh", "dash", "ksh", "mksh", "tcsh", "csh",
    "nu", "xonsh", "elvish", "pwsh",
    "nvim", "vim", "vi", "nano", "emacs", "hx", "kak", "micro",
    "claude", "codex", "gemini", "aider", "opencode", "goose", "crush", "cursor-agent", "node",
];

/// A window whose folder is the one tmux reports: [`LOCAL_CMDS`] or an agent
/// name. An ssh or container window's path is the local client's, not the
/// shell it shows, so it gets no repo block and no read of its folder.
pub fn is_local(cmd: &str) -> bool {
    LOCAL_CMDS.contains(&cmd) || crate::data::agentish(cmd)
}

/// The window kind the write examples are picked by.
pub fn window_kind(name: &str, cmd: &str) -> &'static str {
    if is_claude(name, cmd) {
        "claude"
    } else if ["bash", "zsh", "fish", "sh"].contains(&cmd) {
        "shell"
    } else if SSH_LIKE.contains(&cmd) {
        "ssh"
    } else {
        "other"
    }
}
