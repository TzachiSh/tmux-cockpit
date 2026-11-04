//! The target pane as the panel sees it, and what a Claude Code session on it
//! is doing, read off its own UI text.
use crate::data::gitsafe::{FILTER_CONFIG, SAFE, filter_flags};
use crate::data::tmux;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capture {
    pub name: String,
    pub cmd: String,
    pub path: String,
    /// Raw, not redacted: the envelope redacts it on the way out.
    pub screen: String,
    /// [`repo_state`] of `path`, raw like the screen; "" outside a repo.
    pub repo: String,
}

pub fn is_claude(name: &str, cmd: &str) -> bool {
    cmd == "claude" || name.eq_ignore_ascii_case("claude")
}

/// `None` when tmux cannot answer for the pane.
pub fn capture(pane: &str) -> Option<Capture> {
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
    // An ssh window's path is the local shell's, not the remote one it shows.
    let repo = if path.is_empty() || SSH_LIKE.contains(&cmd.as_str()) {
        String::new()
    } else {
        repo_state(&path, &screen)
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
const STATUS_CAP: usize = 15;
const STAT_CAP: usize = 10;
const PATHS_CAP: usize = 10;

struct Git {
    child: Child,
    out: Receiver<Vec<u8>>,
}

/// gitsafe's flags, no lock taken under the user's own git, no prompt, no
/// transport; its own process group so a timeout kills what it forked.
fn spawn_git(dir: &str, extra: &[String], args: &[&str]) -> Option<Git> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(SAFE)
        .args(extra)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", "")
        .process_group(0)
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, out) = channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        let _ = tx.send(b);
    });
    Some(Git { child, out })
}

/// Its stdout and whether it exited 0, if it finished by `end`; killed otherwise.
fn finish(mut g: Git, end: Instant) -> Option<(bool, String)> {
    let out = g.out.recv_timeout(end.saturating_duration_since(Instant::now())).ok();
    // stdout closed is not exited: wait out what is left of the budget
    while out.is_some() {
        match g.child.try_wait() {
            Ok(Some(st)) => return out.map(|b| (st.success(), String::from_utf8_lossy(&b).into())),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(1)),
            _ => break,
        }
    }
    // Not reaped yet, so the pid and its group are still this child's.
    // SAFETY: killpg takes a plain integer and touches no memory.
    unsafe { libc::killpg(g.child.id() as libc::pid_t, libc::SIGKILL) };
    let _ = g.child.wait();
    None
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

/// The git state of `dir` for the write call's `<repo>` block: branch, short
/// status, the last five commits, the diff stat against HEAD, and which paths
/// on `screen` exist. "" when `dir` is not in a work tree. Plain git under
/// [`REPO_BUDGET`]; a part that runs out of time is left out.
pub fn repo_state(dir: &str, screen: &str) -> String {
    repo_state_within(dir, screen, REPO_BUDGET)
}

pub fn repo_state_within(dir: &str, screen: &str, budget: Duration) -> String {
    if dir.is_empty() {
        return String::new();
    }
    let end = Instant::now() + budget;
    // `git config` exits 1 when nothing matches, so only a timeout fails here.
    let Some((_, cfg)) = spawn_git(dir, &[], &FILTER_CONFIG).and_then(|g| finish(g, end)) else {
        return String::new();
    };
    let Some(filters) = filter_flags(&cfg) else {
        return String::new();
    };
    let q = ["-c", "core.quotePath=false"];
    let status = spawn_git(dir, &filters, &[&q[..], &["status", "--short", "--branch"]].concat());
    let log = spawn_git(dir, &filters, &["log", "-5", "--oneline", "--no-decorate", "--no-color"]);
    let diff_args = ["diff", "--stat", "--no-color", "--no-ext-diff", "--no-textconv", "HEAD"];
    let diff = spawn_git(dir, &filters, &[&q[..], &diff_args].concat());
    let done = |g: Option<Git>| g.and_then(|g| finish(g, end)).and_then(|(ok, o)| ok.then_some(o));
    let (status, log, diff) = (done(status), done(log), done(diff));
    let Some(status) = status else {
        return String::new();
    };
    let mut lines = status.lines();
    let head = lines.next().and_then(|l| l.strip_prefix("## ")).unwrap_or("");
    let sha = log.as_deref().and_then(|l| l.split_whitespace().next());
    let branch = match (head, sha) {
        ("HEAD (no branch)", Some(sha)) => format!("detached at {sha}"),
        ("HEAD (no branch)", None) => "detached".into(),
        (h, _) => match h.strip_prefix("No commits yet on ") {
            Some(b) => format!("{b} (no commits yet)"),
            None => h.into(),
        },
    };
    let mut b = format!("branch: {branch}\n");
    let changed: Vec<&str> = lines.collect();
    if changed.is_empty() {
        b.push_str("status: clean\n");
    } else {
        section(&mut b, "status:", &changed, STATUS_CAP);
    }
    if let Some(log) = log.filter(|l| !l.trim().is_empty()) {
        section(&mut b, "recent commits:", &log.lines().collect::<Vec<_>>(), 5);
    }
    if let Some(diff) = diff.filter(|d| !d.trim().is_empty()) {
        let all: Vec<&str> = diff.lines().collect();
        if let Some((summary, files)) = all.split_last() {
            section(&mut b, "diff --stat:", files, STAT_CAP);
            b.push_str(&format!("  {summary}\n"));
        }
    }
    let paths = screen_paths(screen);
    if !paths.is_empty() {
        let home = std::env::var("HOME").unwrap_or_default();
        let seen: Vec<String> = paths
            .iter()
            .map(|t| {
                let p = match t.strip_prefix('@').unwrap_or(t) {
                    p if p.starts_with('/') => p.to_string(),
                    p => match p.strip_prefix("~/") {
                        Some(r) => format!("{home}/{r}"),
                        None => Path::new(dir).join(p).display().to_string(),
                    },
                };
                let there = if Path::new(&p).exists() { "exists" } else { "missing" };
                format!("{there}: {t}")
            })
            .collect();
        section(
            &mut b,
            "paths on screen:",
            &seen.iter().map(String::as_str).collect::<Vec<_>>(),
            PATHS_CAP,
        );
    }
    b.trim_end().to_string()
}

/// `name.ext`, an extension of one to eight letters or digits.
fn has_ext(file: &str) -> bool {
    file.rsplit_once('.').is_some_and(|(n, e)| {
        let alnum = e.bytes().all(|c| c.is_ascii_alphanumeric());
        !n.is_empty() && (1..=8).contains(&e.len()) && alnum
    })
}

/// Tokens on `screen` that look like files: `@/abs`, `~/x`, `./x`, and any
/// `dir/name.ext`; quotes, brackets and a `:line:col` tail stripped, URLs not
/// taken. The first [`PATHS_CAP`] distinct ones, in screen order.
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
        let pathy = t.chars().all(|c| c.is_alphanumeric() || "._/-+@~".contains(c));
        let prefixed = ["@/", "~/", "./"].iter().any(|p| t.len() > p.len() && t.starts_with(p));
        let named = t.rsplit_once('/').is_some_and(|(_, f)| has_ext(f));
        // `:` is not pathy, so a URL never gets here
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
