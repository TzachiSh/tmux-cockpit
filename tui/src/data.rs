//! tmux and transcript reads. The frozen contract (#61) is "The freeze" in docs/tui.md.
pub mod fleet;
pub mod gitsafe;
pub mod info;
pub mod sessions;
// the subagent tree (#313)
pub mod subagents;
pub mod transcript;

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

pub fn tmux_argv() -> Vec<String> {
    std::env::var("COCKPIT_TMUX")
        .unwrap_or_else(|_| "tmux".into())
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// `None` on a spawn failure or non-zero exit: "no tmux" is not "no agents".
/// A blank COCKPIT_TMUX names no program, so it is a spawn failure too.
pub fn tmux(args: &[&str]) -> Option<String> {
    let c = tmux_argv();
    let out = Command::new(c.first()?).args(&c[1..]).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn readonly() -> bool {
    std::env::var_os("COCKPIT_READONLY").is_some_and(|v| !v.is_empty())
}

/// The only tmux write path, so `COCKPIT_READONLY` needs a single guard.
pub fn tmux_set(args: &[&str]) {
    if readonly() {
        return;
    }
    let _ = tmux(args); // best effort: a failed unpublish is not actionable
}

/// #291: past this, `tui.log` becomes `tui.log.1` (one generation kept).
pub const LOG_CAP: u64 = 1 << 20;

pub fn log(s: &str) {
    let Ok(p) = std::env::var("COCKPIT_LOG") else { return };
    let open = || OpenOptions::new().append(true).create(true).open(&p);
    let Ok(mut f) = open() else { return };
    // One fstat per line, on the fd already open. The inode check keeps two
    // writers that both see a full log from rotating twice: the second would
    // move the first one's fresh file over the full `.1`.
    if let Ok(m) = f.metadata()
        && m.len() > LOG_CAP
    {
        use std::os::unix::fs::MetadataExt;
        let rotated =
            std::fs::metadata(&p).is_ok_and(|n| n.ino() == m.ino() && n.dev() == m.dev()) && std::fs::rename(&p, format!("{p}.1")).is_ok();
        if rotated && let Ok(g) = open() {
            f = g;
        }
    }
    let us = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    // one write(2) on an O_APPEND fd: lines from several processes never interleave
    let _ = f.write_all(format!("{us} {s}\n").as_bytes());
}

/// Declared in rank order: the derived `Ord` is the fleet sort.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    NeedsInput,
    Error,
    Running,
    Done,
    #[default]
    Unknown,
}

impl State {
    /// The `--json` spelling; `Unknown` is `""`, never a word.
    pub fn as_str(self) -> &'static str {
        match self {
            State::NeedsInput => "needs-input",
            State::Error => "error",
            State::Running => "running",
            State::Done => "done",
            State::Unknown => "",
        }
    }
}

impl std::str::FromStr for State {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "needs-input" => State::NeedsInput,
            "error" => State::Error,
            "running" => State::Running,
            "done" => State::Done,
            _ => State::Unknown,
        })
    }
}

impl From<&str> for State {
    fn from(s: &str) -> Self {
        let Ok(st) = s.parse();
        st
    }
}

impl PartialEq<&str> for State {
    fn eq(&self, o: &&str) -> bool {
        self.as_str() == *o
    }
}

#[derive(Default, Clone, PartialEq)]
pub struct Pane {
    pub id: String,
    pub wher: String,
    pub name: String,
    pub cmd: String,
    pub state: State,
    pub title: String,
    pub msg: String,
    pub sess: String,
    pub path: String,
    /// the dir the git facts came from: `@cockpit_dir`, else the transcript's
    /// newest cwd, else `path` (#452); `fleet::work_dirs` picks it
    pub dir: String,
    pub root: String,
    pub spool: String,
    pub pinned: usize,
    pub delivered: usize,
    pub pid: String,
    pub review: String,
    pub doing: String,
    pub said: String,
    pub asked: String,
    pub loc: String,
    pub pm: String,
    /// seconds since the newest assistant row; not Duration, so the type can Hash
    pub since: Option<u64>,
    pub asked_since: Option<u64>,
    /// age of the newest transcript event of any kind
    pub idle_since: Option<u64>,
    /// `#{window_id}`, `@N`, as tmux gave it; the board checks it before use
    pub win: String,
    /// `@cockpit_subs` (#313): background subagents in flight at the agent's last Stop, the
    /// count of `type: "subagent"` in its `background_tasks`; 0 when unset or not a number
    pub subs_n: u32,
    /// every subagent in the session's `subagents/` dir, oldest first (data::subagents)
    pub subs: Vec<subagents::Sub>,
    /// #301: @panestate is set but no agent runs in the pane any more (a crash
    /// or a kill that let no hook run). The board draws it dimmed as ended.
    pub ended: bool,
}

/// Skips the three ages, which change every tick and would defeat the redraw
/// gate. Destructured so a new field fails to compile until it is placed.
impl Hash for Pane {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let Pane {
            id,
            wher,
            name,
            cmd,
            state,
            title,
            msg,
            sess,
            path,
            dir,
            root,
            spool,
            pinned,
            delivered,
            pid,
            review,
            doing,
            said,
            asked,
            loc,
            pm,
            since: _,
            asked_since: _,
            idle_since: _,
            win,
            subs_n,
            subs,
            ended,
        } = self;
        [
            id, wher, name, cmd, title, msg, sess, path, dir, root, spool, pid, review, doing, said, asked, loc, pm, win,
        ]
        .hash(h);
        (state, pinned, delivered, subs_n, ended).hash(h);
        // Sub hashes only what changes its drawing: never `started`, tokens by the thousand.
        subs.hash(h);
    }
}

impl Pane {
    pub fn label(&self) -> &str {
        if self.title.is_empty() {
            if self.name.is_empty() { "(no title)" } else { &self.name }
        } else {
            &self.title
        }
    }
}

pub fn hash_rows(ps: &[Pane], extra: &str) -> u64 {
    let mut h = DefaultHasher::new();
    ps.hash(&mut h);
    extra.hash(&mut h);
    h.finish()
}

/// `~/.claude/projects/<slug>/<uuid>.jsonl`; the slug maps every byte outside
/// `[A-Za-z0-9-]` to `-`. `<uuid>/subagents/` holds only sidechains.
fn transcript_path(sess: &str, path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    // Per byte, not per char: a two-byte glyph in the cwd becomes two dashes.
    let slug: String = path
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' { b as char } else { '-' })
        .collect();
    format!("{home}/.claude/projects/{slug}/{sess}.jsonl")
}

/// The last `cap` bytes as whole rows, decoded lossily: the window can open
/// mid-codepoint. Every line returned is a complete record.
fn tail(f: &mut File, len: u64, cap: u64) -> Vec<String> {
    let off = len.saturating_sub(cap);
    if f.seek(SeekFrom::Start(off.saturating_sub(1))).is_err() {
        return vec![];
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let mut on_boundary = true;
    if off > 0 && !buf.is_empty() {
        on_boundary = buf.remove(0) == b'\n';
    }
    let s = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = s.lines().map(String::from).collect();
    if !on_boundary && !lines.is_empty() {
        lines.remove(0);
    }
    lines
}

/// The one place doing/said/asked are built, so the one place to sanitise them:
/// a control character such as TAB would split a `--rows` record, and a bidi
/// control in model-written text would reorder the row (#453).
fn first_line(s: &str) -> String {
    let one = s.trim().lines().next().unwrap_or("");
    one.chars()
        .filter(|&c| !crate::pick::is_bidi(c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// What the tail says, held as timestamps rather than ages: the ages move every
/// second, these do not, so only these are worth caching.
#[derive(Clone, Default)]
struct Facts {
    tool: String,
    text: String,
    user: String,
    /// the newest assistant row; `newest` is the newest row of any type
    at: Option<SystemTime>,
    user_at: Option<SystemTime>,
    newest: Option<SystemTime>,
    /// `Unknown` when the tail said nothing
    state: State,
    /// the newest main-thread row's `cwd`, else the one the last read kept (#452)
    cwd: String,
    /// the newest open AskUserQuestion's question, `""` when none is open
    ask: String,
}

/// A read's (len, mtime), the file's (device, inode), and what it said.
type Tail = (u64, SystemTime, (u64, u64), Facts);

thread_local! {
    /// Keyed on (len, mtime). The path is re-resolved every tick because
    /// `/compact` and `--resume` rotate the file. The (device, inode) says
    /// whether a kept cwd is this file's (#452).
    static TAILS: std::cell::RefCell<HashMap<String, Tail>> =
        std::cell::RefCell::new(HashMap::new());
}

/// One row can be hundreds of KB, which a small window would read past.
const TAIL_FIRST: u64 = 256 * 1024;
const TAIL_MAX: u64 = 2 * 1024 * 1024;

/// The newest human prompt a transcript has shown (#337). A turn that writes more
/// than `TAIL_FIRST` pushes its own prompt out of the window, and `asked_since` is
/// the anchor "since the last prompt" is measured from, so it is kept here.
#[derive(Clone)]
struct Prompt {
    /// (device, inode): another file at the same path is another transcript
    file: (u64, u64),
    /// the length at the last read; a file below it was rewritten
    len: u64,
    user: String,
    user_at: Option<SystemTime>,
}

thread_local! {
    /// One per transcript path, held for the life of the thread. The board's
    /// worker thread polls for as long as it runs; a one-shot `--json` or `--rows`
    /// reads each file once and starts with nothing.
    static PROMPTS: std::cell::RefCell<HashMap<String, Prompt>> =
        std::cell::RefCell::new(HashMap::new());
}

fn read_facts(path: &str) -> Facts {
    let Ok(mut f) = File::open(path) else { return Facts::default() };
    let Ok(md) = f.metadata() else { return Facts::default() };
    let (len, mtime) = (md.len(), md.modified().unwrap_or(SystemTime::UNIX_EPOCH));
    let hit = TAILS.with(|c| {
        c.borrow()
            .get(path)
            .filter(|(l, m, _, _)| *l == len && *m == mtime)
            .map(|(_, _, _, v)| v.clone())
    });
    if let Some(fa) = hit {
        return fa;
    }
    let file = (md.dev(), md.ino());
    // #452: the cwd the last read saw, unless another file stands at the path now
    // (another inode, or this one rewritten shorter)
    let seen_cwd = TAILS.with(|c| {
        let c = c.borrow();
        let e = c.get(path).filter(|(l, _, id, _)| *id == file && *l <= len);
        e.map(|(_, _, _, f)| f.cwd.clone())
    });
    // A gap past what the window can scan may hold a newer prompt: nothing is kept then.
    let kept = PROMPTS.with(|c| {
        let c = c.borrow();
        c.get(path)
            .filter(|p| p.file == file && p.len <= len && len - p.len <= TAIL_MAX)
            .cloned()
    });
    // The window reaches back to the last read, so a prompt written and buried
    // between two reads is seen, not the older one that was kept.
    let first = kept.as_ref().map_or(TAIL_FIRST, |p| (len - p.len).clamp(TAIL_FIRST, TAIL_MAX));
    let mut fa = parse(tail(&mut f, len, first));
    // Known gap: over 256 KB written after an open ask still finds a row, so
    // the window never grows to reach the ask.
    if fa.tool.is_empty() && fa.text.is_empty() && len > first && first < TAIL_MAX {
        fa = parse(tail(&mut f, len, TAIL_MAX));
    }
    // A quiet stretch of rows with no cwd must not push it out of the window.
    if fa.cwd.is_empty() {
        fa.cwd = seen_cwd.unwrap_or_default();
    }
    if fa.user.is_empty()
        && fa.user_at.is_none()
        && let Some(p) = kept
    {
        (fa.user, fa.user_at) = (p.user, p.user_at);
    }
    PROMPTS.with(|c| {
        let mut c = c.borrow_mut();
        if fa.user.is_empty() && fa.user_at.is_none() {
            c.remove(path);
        } else {
            c.insert(
                path.to_string(),
                Prompt {
                    file,
                    len,
                    user: fa.user.clone(),
                    user_at: fa.user_at,
                },
            );
        }
    });
    TAILS.with(|c| c.borrow_mut().insert(path.to_string(), (len, mtime, file, fa.clone())));
    fa
}

/// Ends the turn without closing anything: neither a prompt nor an answer.
/// It arrives as a `text` block in the array form.
const INTERRUPT: &str = "[Request interrupted by user";

fn parse(lines: Vec<String>) -> Facts {
    let mut fa = Facts::default();
    // An AskUserQuestion is open until a user row carries its tool_result:
    // answers, dismissals and denials write one, an interrupt does not.
    let mut asks: Vec<String> = vec![];
    // #453: the newest tool_use's id and, for an AskUserQuestion, its question:
    // the row's message only while that ask is the one still open
    let mut newest: Option<(String, Option<String>)> = None;
    let (mut turn_over, mut saw) = (false, false);
    for raw in lines {
        let Ok(v) = serde_json::from_str::<Value>(&raw) else { continue };
        // In case a future version inlines subagent rows.
        if v["isSidechain"].as_bool() == Some(true) || !v["agentId"].is_null() {
            continue;
        }
        let ts = v["timestamp"].as_str().and_then(rfc3339);
        if ts.is_some() {
            fa.newest = ts;
        }
        if let Some(c) = v["cwd"].as_str().filter(|c| !c.is_empty()) {
            fa.cwd = c.to_string();
        }
        match v["type"].as_str() {
            Some("user") => {
                saw = true;
                let c = &v["message"]["content"];
                match c.as_str() {
                    // a tool_result arrives as an array, a human turn as a string
                    Some(t) if t.starts_with(INTERRUPT) => turn_over = true,
                    // <task-notification, <command-name, <local-command-*: the
                    // harness wrote these, so not a new turn and not `asked`.
                    Some(t) if t.starts_with('<') => {}
                    Some(t) => {
                        fa.user = first_line(t);
                        fa.user_at = ts;
                        turn_over = false;
                        // a typed prompt means the human moved on from an interrupted ask
                        asks.clear();
                    }
                    None => {
                        let (mut interrupted, mut result, mut typed) = (false, false, None);
                        for b in c.as_array().into_iter().flatten() {
                            if let Some(id) = b["tool_use_id"].as_str() {
                                asks.retain(|a| a != id);
                                result = true;
                            }
                            // safe beside the tool_results: they carry no `text`
                            if let Some(t) = b["text"].as_str() {
                                interrupted |= t.starts_with(INTERRUPT);
                                if typed.is_none() && !t.starts_with(INTERRUPT) && !t.starts_with('<') {
                                    typed = Some(t);
                                }
                            }
                        }
                        turn_over = interrupted;
                        // A prompt with an image is an array too: its text block is the
                        // human's, unless the row is a tool_result or one the harness wrote.
                        if let Some(t) = typed
                            && !result
                            && !interrupted
                            && v["isMeta"].as_bool() != Some(true)
                            && !first_line(t).is_empty()
                        {
                            fa.user = first_line(t);
                            fa.user_at = ts;
                            asks.clear();
                        }
                    }
                }
            }
            Some("assistant") => {
                saw = true;
                fa.at = ts.or(fa.at);
                // end_turn rides the text row; attachment and custom-title rows
                // land after it and must not reopen the turn.
                turn_over = v["message"]["stop_reason"].as_str() == Some("end_turn");
                let Some(blocks) = v["message"]["content"].as_array() else {
                    continue;
                };
                for b in blocks {
                    match b["type"].as_str() {
                        Some("tool_use") => {
                            let name = b["name"].as_str().unwrap_or("");
                            // The name alone when no field says more: never a bare `name:`.
                            fa.tool = match transcript::pick(name, &b["input"]).map(|(p, s)| (p, first_line(s))) {
                                Some((p, d)) if !d.is_empty() => format!("{name}: {p}{d}"),
                                _ => name.to_string(),
                            };
                            let id = b["id"].as_str().unwrap_or_default().to_string();
                            let ask = name == "AskUserQuestion";
                            if ask && !id.is_empty() {
                                asks.push(id.clone());
                            }
                            newest = Some((id, ask.then(|| transcript::question(&b["input"])).flatten()));
                        }
                        Some("text") => {
                            let t = first_line(b["text"].as_str().unwrap_or(""));
                            if !t.is_empty() {
                                fa.text = t;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    fa.ask = newest.filter(|(id, _)| asks.contains(id)).and_then(|(_, q)| q).unwrap_or_default();
    // An open question outranks a finished turn: the interrupt that ended the
    // turn left the picker's tool_use open, and the human still owes an answer.
    fa.state = match () {
        _ if !asks.is_empty() => State::NeedsInput,
        // an unknown state is unknown, never idle
        _ if !saw => State::Unknown,
        _ if turn_over => State::Done,
        _ => State::Running,
    };
    fa
}

/// doing / said / asked and their ages, from the tail of the session transcript.
/// Returns the transcript's cwd, `""` when it has none.
fn read_transcript(p: &mut Pane) -> String {
    if p.sess.is_empty() {
        return String::new();
    }
    let fa = read_facts(&transcript_path(&p.sess, &p.path));
    // @panestate wins; the transcript speaks only for panes the hooks never touched.
    if p.state == State::Unknown {
        p.state = fa.state;
    }
    // #453: the hook's text is generic; an open question says what is owed
    if p.state == State::NeedsInput && !fa.ask.is_empty() {
        p.msg = fa.ask;
    }
    p.doing = fa.tool;
    p.said = fa.text;
    p.asked = fa.user;
    p.since = fa.at.and_then(age);
    p.asked_since = fa.user_at.and_then(age);
    p.idle_since = fa.newest.and_then(age);
    fa.cwd
}

fn age(t: SystemTime) -> Option<u64> {
    SystemTime::now().duration_since(t).ok().map(|d| d.as_secs())
}

/// "2026-08-26T18:27:47.123Z" to SystemTime. Days-from-civil (Howard Hinnant),
/// so the crate needs no chrono for one timestamp format.
fn rfc3339(t: &str) -> Option<SystemTime> {
    let (date, time) = t.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, dd) = (d.next()??, d.next()??, d.next()??);
    let mut tt = time.trim_end_matches('Z').split(':');
    let (h, mi) = (tt.next()?.parse::<i64>().ok()?, tt.next()?.parse::<i64>().ok()?);
    let s = tt.next()?.parse::<f64>().ok()?;
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + dd - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400 + h * 3600 + mi * 60 + s as i64;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64))
}

thread_local! {
    /// `claude agents --json` dwarfs the rest of a tick and changes only when a
    /// session starts or ends; a new pane on this path waits up to the TTL.
    static AGENTS: std::cell::RefCell<Option<(std::time::Instant, HashMap<String, String>)>> = const { std::cell::RefCell::new(None) };
}

fn agent_sessions_cached() -> HashMap<String, String> {
    AGENTS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((t, m)) = c.as_ref()
            && t.elapsed() < Duration::from_secs(60)
        {
            return m.clone();
        }
        let m = agent_sessions();
        *c = Some((std::time::Instant::now(), m.clone()));
        m
    })
}

/// `claude agents --json` maps a pid to a session id, for panes whose
/// @cockpit_session the hooks never set.
///
/// #294: on the fetch thread, so a `claude` that hangs must not hang the board.
/// It gets `AGENTS_DEADLINE`, under the board's 3 s stale threshold (the call
/// takes about 0.2 s), then its process group is killed and the answer is empty.
fn agent_sessions() -> HashMap<String, String> {
    let out = claude_agents_json(AGENTS_DEADLINE).unwrap_or_default();
    let v: Value = serde_json::from_slice(&out).unwrap_or(Value::Null);
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| Some((r["pid"].as_i64()?.to_string(), r["sessionId"].as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

const AGENTS_DEADLINE: Duration = Duration::from_millis(2500);

/// `claude agents --json`'s stdout, or `None` on a spawn failure or when it
/// has not closed by the deadline.
fn claude_agents_json(deadline: Duration) -> Option<Vec<u8>> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let mut child = Command::new("claude")
        .args(["agents", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // its own group, so the kill takes anything it forked that holds stdout
        .process_group(0)
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        let _ = tx.send(b);
    });
    let end = Instant::now() + deadline;
    let out = rx.recv_timeout(deadline).ok();
    // stdout closed is not exited: wait out what is left of the deadline
    while out.is_some() && Instant::now() < end {
        if !matches!(child.try_wait(), Ok(None)) {
            return out;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Not reaped yet, so the pid and its group are still this child's.
    // SAFETY: killpg takes a plain integer and touches no memory.
    unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
    let _ = child.wait();
    out
}

/// One `ps` for the whole fleet, serving both the agent classifier and the
/// session lookup.
#[derive(Default)]
struct Procs {
    kids: HashMap<String, Vec<String>>,
    /// pid to the lowercased basename of its command
    comm: HashMap<String, String>,
}

/// `-axww`: macOS truncates to the terminal width even into a pipe. `comm` may
/// hold spaces, so only the two leading numeric fields are split.
fn procs() -> Procs {
    let out = Command::new("ps")
        .args(["-axww", "-o", "pid=,ppid=,comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut t = Procs::default();
    for l in out.lines() {
        let mut it = l.split_whitespace();
        let (Some(pid), Some(ppid)) = (it.next(), it.next()) else {
            continue;
        };
        if pid.parse::<u64>().is_err() || ppid.parse::<u64>().is_err() {
            continue;
        }
        let comm: String = it.collect::<Vec<_>>().join(" ");
        t.comm.insert(pid.into(), comm_norm(&comm));
        t.kids.entry(ppid.into()).or_default().push(pid.into());
    }
    t
}

/// Capped, with a seen set: `ps` can report a process as its own parent.
const MAX_DEPTH: usize = 8;

fn tree(pid: &str, t: &Procs) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    let mut q = vec![(pid.to_string(), 0usize)];
    while let Some((p, d)) = q.pop() {
        if !seen.insert(p.clone()) {
            continue;
        }
        if d < MAX_DEPTH
            && let Some(k) = t.kids.get(&p)
        {
            q.extend(k.iter().map(|c| (c.clone(), d + 1)));
        }
        out.push(p);
    }
    out
}

/// `ps comm=` and a `/proc/<pid>/comm` read both give the kernel's 15-char
/// name, so one normalisation serves both: basename, no login dash, lowercase.
fn comm_norm(comm: &str) -> String {
    let comm = comm.split_whitespace().collect::<Vec<_>>().join(" ");
    let base = comm.rsplit('/').next().unwrap_or(&comm);
    base.trim_start_matches('-').to_ascii_lowercase()
}

/// #492: the pids under `pid` (itself first off the stack, as [`tree`] orders
/// them) from `<root>/<pid>/task/<tid>/children`, reading only that subtree,
/// where `ps -axww` lists every process on the box. `None` when `root` is not a
/// proc with `children` (macOS, or a kernel without CONFIG_PROC_CHILDREN) or a
/// process vanished mid-walk: the caller then asks `ps`, which is always right.
fn proc_tree(root: &std::path::Path, pid: &str) -> Option<Vec<String>> {
    if !root.join("self/task").is_dir() {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    let mut q = vec![(pid.to_string(), 0usize)];
    while let Some((p, d)) = q.pop() {
        if !seen.insert(p.clone()) {
            continue;
        }
        if d < MAX_DEPTH {
            let tasks = root.join(&p).join("task");
            let mut kids = vec![];
            for t in std::fs::read_dir(&tasks).ok()? {
                let s = std::fs::read_to_string(t.ok()?.path().join("children")).ok()?;
                kids.extend(s.split_whitespace().map(str::to_string));
            }
            q.extend(kids.into_iter().map(|c| (c, d + 1)));
        }
        out.push(p);
    }
    Some(out)
}

/// `pid` and everything under it with each name, in [`tree`] order: from
/// `procs_t` when `ps` already ran this refresh, else a [`proc_tree`] walk,
/// else one `ps` kept in `procs_t` for the next ask.
fn lineage(pid: &str, procs_t: &mut Option<Procs>) -> Vec<(String, String)> {
    if procs_t.is_none()
        && let Some(ps) = proc_tree(std::path::Path::new("/proc"), pid)
    {
        return ps
            .into_iter()
            .map(|p| {
                let c = std::fs::read_to_string(format!("/proc/{p}/comm")).unwrap_or_default();
                (p, comm_norm(&c))
            })
            .collect();
    }
    let t = procs_t.get_or_insert_with(procs);
    tree(pid, t)
        .into_iter()
        .map(|p| (p.clone(), t.comm.get(&p).cloned().unwrap_or_default()))
        .collect()
}

/// A live Claude renames itself to its version, so `2.1.246` and `2_1_246` are
/// agent names too.
pub(crate) fn agentish(comm: &str) -> bool {
    let b = comm.rsplit('/').next().unwrap_or(comm).trim_start_matches('-').to_ascii_lowercase();
    is_agent_cmd(&b) || version_shaped(&b)
}

fn version_shaped(s: &str) -> bool {
    let seps = s.chars().filter(|c| *c == '.' || *c == '_').count();
    seps >= 2 && s.starts_with(|c: char| c.is_ascii_digit()) && s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
}

fn find_session(pid: &str, procs_t: &mut Option<Procs>, agents: &HashMap<String, String>) -> String {
    lineage(pid, procs_t)
        .iter()
        .find_map(|(p, _)| agents.get(p).cloned())
        .unwrap_or_default()
}

/// repo⑂branch, the `±` string and the repo root, for one path.
pub fn git_facts(path: &str) -> (String, String, String) {
    fleet::scan(&[path.to_string()]).facts.remove(path).unwrap_or_default()
}

const GIT_TTL: Duration = Duration::from_secs(10);

#[derive(Default)]
struct GitCache {
    /// by path
    facts: HashMap<String, (Instant, (String, String, String))>,
    /// each pane's state at the last fetch, by pane id
    seen: HashMap<String, State>,
}

thread_local! {
    /// Five git forks per repo are worth paying every 10 s, not every tick;
    /// a pane changing state is when its diff most likely moved, so it refetches.
    static GIT: std::cell::RefCell<GitCache> = std::cell::RefCell::default();
}

/// `dirs` is each pane's work dir candidates (`fleet::work_dirs`), in `ps` order.
fn fill_git(ps: &mut [Pane], dirs: &[[String; 3]]) {
    // the tree each pane works in: @cockpit_dir, the transcript's cwd, the cwd (#253, #452)
    let dirs = fleet::work_dirs(dirs);
    GIT.with(|c| {
        let GitCache { facts, seen } = &mut *c.borrow_mut();
        let stale: HashSet<&String> = ps
            .iter()
            .zip(&dirs)
            .filter(|(p, _)| seen.get(&p.id) != Some(&p.state))
            .map(|(_, d)| d)
            .collect();
        *seen = ps.iter().map(|p| (p.id.clone(), p.state)).collect();
        facts.retain(|_, (t, _)| t.elapsed() < GIT_TTL);
        for d in stale {
            facts.remove(d);
        }
        // one scan per distinct root, in parallel, however many dirs share it
        let missing: Vec<String> = dirs.iter().filter(|d| !facts.contains_key(*d)).cloned().collect();
        for (d, f) in fleet::scan(&missing).facts {
            facts.insert(d, (Instant::now(), f));
        }
        for (p, d) in ps.iter_mut().zip(&dirs) {
            let (_, (loc, pm, root)) = &facts[d];
            (p.loc, p.pm, p.root, p.dir) = (loc.clone(), pm.clone(), root.clone(), d.clone());
        }
    });
}

/// The review spool's two counts, straight off the directory.
fn spool_counts(dir: &str) -> (usize, usize) {
    if dir.is_empty() {
        return (0, 0);
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return (0, 0) };
    let (mut pinned, mut delivered) = (0, 0);
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.ends_with(".pin") {
            pinned += 1;
        } else if n.ends_with(".sent") {
            delivered += 1;
        }
    }
    (pinned, delivered)
}

/// One pane by id.
pub fn one_pane(id: &str) -> Option<Pane> {
    let found = list_panes()?.into_iter().find(|p| p.id == id);
    Some(found.unwrap_or(Pane {
        id: id.to_string(),
        ..Default::default()
    }))
}

thread_local! {
    /// `|claude|codex|…|` from cockpit.tmux, so the user's list is the only copy.
    /// Empty (plugin not loaded) leaves @panestate as the only test.
    static AGENT_GLOB: String = tmux(&["show-option", "-gqv", "@cockpit_agents_glob"]).unwrap_or_default().trim().to_string();
}

fn is_agent_cmd(cmd: &str) -> bool {
    AGENT_GLOB.with(|g| g.contains(&format!("|{cmd}|")))
}

/// #301: the hooks marked the pane (`raw`, its @panestate) but no process in
/// it is an agent. With no agent list (the plugin not loaded) a live `claude`
/// cannot be told from a shell, so nothing is called ended.
pub fn ended(raw: &str, live: bool, glob_known: bool) -> bool {
    !raw.is_empty() && !live && glob_known
}

const SEP: char = '\u{1f}';

/// Split on `\037`, not tab: a tab in a cwd shifts every later field. tmux never
/// escapes format output, so free-form fields strip NL, TAB and US in-format;
/// `#{q:}` would pass control characters raw.
const FIELDS: &str = concat!(
    "#{pane_id}\u{1f}#{session_name}:#{window_index}\u{1f}#{window_name}\u{1f}#{pane_current_command}",
    "\u{1f}#{?#{@panestate},#{@panestate},-}",
    "\u{1f}#{?#{@cockpit_title},#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{@cockpit_title}}}},-}",
    "\u{1f}#{?#{@panestate_msg},#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{@panestate_msg}}}},-}",
    "\u{1f}#{?#{@cockpit_session},#{@cockpit_session},-}",
    "\u{1f}#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{pane_current_path}}}}",
    "\u{1f}#{pane_pid}",
    "\u{1f}#{?#{@cockpit_review},#{@cockpit_review},-}",
    "\u{1f}#{?#{@cockpit_review_spool},#{@cockpit_review_spool},-}",
    "\u{1f}#{window_id}",
    "\u{1f}#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{@cockpit_dir}}}}",
    // #313: 14 the hook's subagent count, 15 the session's transcript, for its subagents/ dir
    "\u{1f}#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{@cockpit_subs}}}}",
    "\u{1f}#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{@cockpit_transcript}}}}",
);

/// `-` back to empty. A sentinelled option whose real value is a bare `-` would
/// read as unset; none of these ever holds one.
fn unsentinel(s: &str) -> String {
    if s == "-" { String::new() } else { s.to_string() }
}

/// (pane id, pane pid, command): what a tree verdict is good for
type TreeKey = (String, String, String);

thread_local! {
    /// #301: whether an agent runs anywhere in a marked pane's process tree, by
    /// (pane id, pane pid, command): a new command or process asks again, else
    /// one `ps` per pane per [`GIT_TTL`] rather than one per tick.
    static TREES: std::cell::RefCell<HashMap<TreeKey, (Instant, bool)>> = std::cell::RefCell::default();
    /// each pane's last `ended`, so a flip is logged once
    static ENDED: std::cell::RefCell<HashMap<String, bool>> = std::cell::RefCell::default();
}

fn tree_cached(key: (&str, &str, &str), look: impl FnOnce() -> bool) -> bool {
    let key = (key.0.to_string(), key.1.to_string(), key.2.to_string());
    TREES.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|_, (t, _)| t.elapsed() < GIT_TTL);
        if let Some((_, v)) = c.get(&key) {
            return *v;
        }
        let v = look();
        c.insert(key, (Instant::now(), v));
        v
    })
}

/// One line when a pane turns ended or back, with what found the agent
/// (`command`, `tree`) or `none`, so a false "ended" can be traced.
fn log_ended_flip(p: &Pane, live_by: &str) {
    let was = ENDED.with(|c| c.borrow_mut().insert(p.id.clone(), p.ended));
    if was.unwrap_or(false) != p.ended {
        log(&format!(
            "fleet ended-flip pane={} ended={} live_by={live_by} cmd={:?} pid={}",
            p.id, p.ended, p.cmd, p.pid
        ));
    }
}

/// `None` when the list-panes call itself failed: no tmux, not "no agents".
pub fn list_panes() -> Option<Vec<Pane>> {
    list_fleet().map(|(ps, _)| ps)
}

/// `list_panes`, plus every tmux session name off the same read, agents or
/// not: a session always holds a pane, so these are all of them.
pub fn list_fleet() -> Option<(Vec<Pane>, std::collections::BTreeSet<String>)> {
    let out = tmux(&["list-panes", "-a", "-F", FIELDS])?;
    let mut sessions = std::collections::BTreeSet::new();
    let (mut agents, mut procs_t) = (None, None);
    let (mut ps, mut dirs) = (vec![], vec![]);
    for l in out.lines() {
        let f: Vec<&str> = l.split(SEP).collect();
        // FIELDS has 16; the 13 before @cockpit_dir are the floor, so an older format reads
        if f.len() < 13 {
            continue;
        }
        // `session:window`; a session name never holds a `:` (tmux swaps it)
        sessions.insert(f[1].rsplit_once(':').map_or(f[1], |(s, _)| s).to_string());
        // A pane carrying @panestate counts whatever it runs: the hooks are the
        // source of truth, and a wrapper hides `claude` behind its own argv0.
        let raw = unsentinel(f[4]);
        // The name is a fast path that skips the `ps` fork; the tree decides.
        let by_cmd = agentish(f[3]);
        let mut in_tree = || lineage(f[9], &mut procs_t).iter().any(|(_, c)| agentish(c));
        // #301: a marked pane's verdict is cached, or every ended row forks `ps` each tick
        let by_tree = !by_cmd
            && if raw.is_empty() {
                in_tree()
            } else {
                tree_cached((f[0], f[9], f[3]), in_tree)
            };
        let live = by_cmd || by_tree;
        if raw.is_empty() && !live {
            continue;
        }
        let mut p = Pane {
            id: f[0].into(),
            wher: f[1].into(),
            name: f[2].into(),
            cmd: f[3].into(),
            state: raw.as_str().into(),
            title: unsentinel(f[5]),
            msg: unsentinel(f[6]),
            sess: unsentinel(f[7]),
            path: f[8].into(),
            pid: f[9].into(),
            review: unsentinel(f[10]),
            spool: unsentinel(f[11]),
            win: f[12].into(),
            ended: ended(&raw, live, AGENT_GLOB.with(|g| !g.is_empty())),
            ..Default::default()
        };
        log_ended_flip(
            &p,
            if by_cmd {
                "command"
            } else if by_tree {
                "tree"
            } else {
                "none"
            },
        );
        if p.sess.is_empty() {
            let agents = agents.get_or_insert_with(agent_sessions_cached);
            p.sess = find_session(&p.pid, &mut procs_t, agents);
        }
        let said_cwd = read_transcript(&mut p);
        (p.pinned, p.delivered) = spool_counts(&p.spool);
        // @cockpit_dir, @cockpit_subs and @cockpit_transcript come after the 13 a row from an
        // older format has, so such a row still reads: 13, 14 and 15 are optional
        // #452: the work dir's candidates, in the order they are tried
        let opt = f.get(13).copied().unwrap_or_default().to_string();
        dirs.push([opt, said_cwd, p.path.clone()]);
        p.subs_n = f.get(14).and_then(|n| n.parse().ok()).unwrap_or(0);
        if let Some(d) = subagents::dir_of(f.get(15).copied().unwrap_or_default(), &p.sess, &p.path) {
            p.subs = subagents::read(&d, sessions::now());
        }
        ps.push(p);
    }
    fill_git(&mut ps, &dirs);
    ps.sort_by_key(|p| p.state);
    Some((ps, sessions))
}

/// A pane can hold one multi-megabyte line, and the ANSI parser re-runs over the
/// whole preview each time it changes. Keep the newest 64 KB.
const PREVIEW_MAX: usize = 64 * 1024;

/// `None` when capture-pane fails: the pane is gone, which is not the same as
/// an empty preview and must never be drawn as a blank tile.
pub fn preview(id: &str, n: usize) -> Option<String> {
    if n == 0 {
        return Some(String::new());
    }
    // -J so a wrap point cannot split an ANSI sequence and hand the parser halves.
    let out = tmux(&["capture-pane", "-ep", "-J", "-t", id, "-S", &format!("-{n}")])?;
    // #284: ansi-to-tui drops the rest of a line after an OSC 8 link that ends
    // in `ESC \`, the form tmux emits; no OSC draws anything in a preview
    let out = strip_osc(&out);
    let lines: Vec<&str> = out.trim_end().lines().collect();
    Some(cap(&lines[lines.len().saturating_sub(n)..].join("\n")))
}

/// The newest [`PREVIEW_MAX`] bytes of `text`. A cut can land inside an escape,
/// whose tail would read as text: it starts at the next line. A single line
/// longer than the cap keeps the cut.
fn cap(text: &str) -> String {
    let cut = snap(text, text.len().saturating_sub(PREVIEW_MAX));
    let start = match text[..cut].ends_with('\n') || cut == 0 {
        true => cut,
        false => text[cut..].find('\n').map_or(cut, |k| cut + k + 1),
    };
    text[start..].to_string()
}

/// #284: the right pane's screen, `n` rows of the selected agent's visible
/// screen, as [`view`] picks them. One fork: the capture, then the cursor, in
/// one tmux command, so the two describe the same moment. `None` when it fails.
pub fn screen(id: &str, n: usize) -> Option<String> {
    if n == 0 {
        return Some(String::new());
    }
    // no -S: the visible screen, so the blank rows below the cursor keep their
    // places; no -J, which would merge rows and move the cursor's off its row
    let out = tmux(&[
        "capture-pane",
        "-p",
        "-e",
        "-t",
        id,
        ";",
        "display-message",
        "-p",
        "-t",
        id,
        "#{cursor_flag} #{cursor_y}",
    ])?;
    let out = strip_osc(&out);
    let (body, cursor) = split_cursor(&out)?;
    let v = view(body, cursor, n);
    // the rows the cap took come back blank, so the newest stays where it was
    let kept = cap(&v);
    let lost = v.matches('\n').count() - kept.matches('\n').count();
    Some("\n".repeat(lost) + &kept)
}

/// A capture's rows and its cursor row, `None` when the cursor is hidden, off
/// the `#{cursor_flag} #{cursor_y}` line that ends it. `None` without that line.
pub fn split_cursor(out: &str) -> Option<(&str, Option<usize>)> {
    let out = out.strip_suffix('\n').unwrap_or(out);
    let (body, tail) = out.rsplit_once('\n').unwrap_or(("", out));
    let (flag, y) = tail.split_once(' ')?;
    let y: usize = y.parse().ok()?;
    match flag {
        "1" => Some((body, Some(y))),
        "0" => Some((body, None)),
        _ => None,
    }
}

/// The `rows` rows of a visible `screen` the pane shows. Its used part runs to
/// the cursor's row or the last row with text, whichever is lower; rows past it
/// are never content. A used part that fits is all of it from row 0, as the pane
/// looks. A taller one:
/// - with the cursor shown (vim, less, a shell): tmux's third-down rule, clamped
///   to the used rows. tmux's choose-tree preview (screen-write.c,
///   `screen_write_preview`) puts the view's top at `cursor_y - rows / 3`, so the
///   cursor sits a third down, and clamps it to the screen's height; here it is
///   clamped to `[0, used - rows]`. tmux's horizontal pan is not taken: a long
///   line is clipped.
/// - with the cursor hidden (Claude Code, htop): the view ends at the last row
///   with text.
pub fn view(screen: &str, cursor: Option<usize>, rows: usize) -> String {
    let mut lines: Vec<&str> = screen.split('\n').collect();
    let text = lines.iter().rposition(|l| !blank_row(l));
    let Some(end) = text.max(cursor) else {
        return String::new();
    };
    let used = end + 1;
    if lines.len() < used {
        lines.resize(used, "");
    }
    let top = match cursor {
        _ if used <= rows => 0,
        Some(c) => c.saturating_sub(rows / 3).min(used - rows),
        None => used - rows,
    };
    lines[top..top + rows.min(used)].join("\n")
}

/// A row whose only cells are blanks, colours or not.
fn blank_row(l: &str) -> bool {
    let mut rest = l;
    while let Some(k) = rest.find('\x1b') {
        if !rest[..k].trim().is_empty() {
            return false;
        }
        // a CSI runs to its final byte, 0x40..=0x7e; any other escape is two bytes
        let esc = &rest[k + 1..];
        let len = match esc.strip_prefix('[') {
            Some(csi) => csi.find(|c: char| ('\x40'..='\x7e').contains(&c)).map_or(csi.len(), |e| e + 1) + 1,
            None => esc.chars().next().map_or(0, char::len_utf8),
        };
        rest = &esc[len.min(esc.len())..];
    }
    rest.trim().is_empty()
}

/// `s` with every OSC sequence (`ESC ]` up to a BEL or `ESC \`) taken out. One
/// with no end ends at its line's end, so it never eats the next line.
fn strip_osc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(k) = rest.find("\x1b]") {
        out.push_str(&rest[..k]);
        let body = &rest[k + 2..];
        let end = body
            .char_indices()
            .find_map(|(i, c)| match c {
                '\x07' => Some(i + 1),
                '\x1b' if body[i + 1..].starts_with('\\') => Some(i + 2),
                '\n' => Some(i),
                _ => None,
            })
            .unwrap_or(body.len());
        rest = &body[end..];
    }
    out.push_str(rest);
    out
}

/// Pane text is rarely ASCII, and slicing a byte offset inside a glyph panics.
fn snap(s: &str, mut at: usize) -> usize {
    while at < s.len() && !s.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// The 5-column right rail: 32s, 7m12s, 1h04m, 3d. Seconds drop past ten
/// minutes and minutes past ten hours so a clipped digit never lies.
pub fn fmt_dur(s: u64) -> String {
    match s {
        0..60 => format!("{s}s"),
        60..600 if s.is_multiple_of(60) => format!("{}m", s / 60),
        60..600 => format!("{}m{:02}s", s / 60, s % 60),
        600..3600 => format!("{}m", s / 60),
        3600..36000 => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
        36000..86400 => format!("{}h", s / 3600),
        // clamped: 10000d is six columns and the rail would eat its leading digit
        _ => format!("{}d", (s / 86400).min(9999)),
    }
}

/// Truncate right to `n` display columns with `…`. Counts chars, not bytes.
pub fn trunc(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() <= n {
        return s.into();
    }
    if n == 0 {
        return String::new();
    }
    c[..n - 1].iter().collect::<String>() + "…"
}

/// Paths truncate left: `…/tmux-cockpit/review/14-…`.
pub fn trunc_left(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() <= n {
        return s.into();
    }
    if n == 0 {
        return String::new();
    }
    "…".to_string() + &c[c.len() - (n - 1)..].iter().collect::<String>()
}

/// Word-wrap to at most `max` lines of `w` columns; the last line ends in `…`
/// if text remains.
pub fn wrap(s: &str, w: usize, max: usize) -> Vec<String> {
    if w == 0 || max == 0 {
        return vec![];
    }
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let need = if cur.is_empty() {
            word.chars().count()
        } else {
            cur.chars().count() + 1 + word.chars().count()
        };
        if need > w && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            if out.len() == max {
                let last = out.pop().unwrap();
                out.push(trunc(&format!("{last} {word}"), w));
                return out;
            }
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
        while cur.chars().count() > w {
            // one word longer than the line: hard-split it
            let head: String = cur.chars().take(w).collect();
            let rest: String = cur.chars().skip(w).collect();
            out.push(head);
            cur = rest;
            if out.len() == max {
                let last = out.pop().unwrap();
                out.push(trunc(&last, w));
                return out;
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// branch popup (#246)
pub mod branches;

#[cfg(test)]
mod tests {
    use super::*;

    /// The board's doing line for each tool shape must equal the history reader's
    /// label (tlabel), save for the 160-char cap and control handling: the cases
    /// are short and clean.
    #[test]
    fn read_facts_labels_each_tool_by_what_it_does() {
        for l in include_str!("../../tests/fixtures/tool-labels/cases.jsonl").lines() {
            let c: Value = serde_json::from_str(l).unwrap();
            let row = serde_json::json!({"type": "assistant", "message": {"content": [
                {"type": "tool_use", "id": "t1", "name": c[0], "input": c[1]}]}});
            assert_eq!(parse(vec![row.to_string()]).tool, c[2].as_str().unwrap(), "{l}");
        }
    }

    /// A fake `/proc`: `kids` maps pid to one `children` string per task.
    fn fake_proc(kids: &[(&str, &[&str])]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("cockpit-proc-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(root.join("self/task")).unwrap();
        for (pid, tasks) in kids {
            for (i, c) in tasks.iter().enumerate() {
                let d = root
                    .join(pid)
                    .join("task")
                    .join(format!("{}", pid.parse::<u32>().unwrap() + i as u32));
                std::fs::create_dir_all(&d).unwrap();
                std::fs::write(d.join("children"), c).unwrap();
            }
        }
        root
    }

    fn pids(root: &std::path::Path, pid: &str) -> Option<Vec<String>> {
        let r = proc_tree(root, pid);
        std::fs::remove_dir_all(root).unwrap();
        r
    }

    #[test]
    fn proc_tree_reaches_a_grandchild() {
        let r = fake_proc(&[("10", &["11 12 "]), ("11", &["13 "]), ("12", &[""]), ("13", &[""])]);
        let mut got = pids(&r, "10").unwrap();
        got.sort();
        assert_eq!(got, ["10", "11", "12", "13"]);
    }

    #[test]
    fn proc_tree_reaches_the_children_of_every_thread() {
        let r = fake_proc(&[("10", &["11 ", "12 "]), ("11", &[""]), ("12", &[""])]);
        let mut got = pids(&r, "10").unwrap();
        got.sort();
        assert_eq!(got, ["10", "11", "12"]);
    }

    #[test]
    fn proc_tree_orders_like_tree() {
        let r = fake_proc(&[
            ("10", &["11 12 "]),
            ("11", &["13 14 "]),
            ("12", &[""]),
            ("13", &[""]),
            ("14", &[""]),
        ]);
        let mut t = Procs::default();
        t.kids.insert("10".into(), vec!["11".into(), "12".into()]);
        t.kids.insert("11".into(), vec!["13".into(), "14".into()]);
        assert_eq!(pids(&r, "10").unwrap(), tree("10", &t));
    }

    #[test]
    fn proc_tree_survives_a_cycle() {
        let r = fake_proc(&[("10", &["11 "]), ("11", &["10 "])]);
        assert_eq!(pids(&r, "10").unwrap(), ["10", "11"]);
    }

    #[test]
    fn proc_tree_stops_at_the_depth_cap() {
        let chain: Vec<(String, String)> = (0..MAX_DEPTH + 3).map(|i| ((10 + i).to_string(), format!("{} ", 11 + i))).collect();
        let kids: Vec<(&str, Vec<&str>)> = chain.iter().map(|(p, c)| (p.as_str(), vec![c.as_str()])).collect();
        let kids: Vec<(&str, &[&str])> = kids.iter().map(|(p, c)| (*p, c.as_slice())).collect();
        let r = fake_proc(&kids);
        assert_eq!(pids(&r, "10").unwrap().len(), MAX_DEPTH + 1);
    }

    #[test]
    fn proc_tree_is_none_without_a_children_file() {
        let r = fake_proc(&[("10", &["11 "])]);
        std::fs::create_dir_all(r.join("11/task/11")).unwrap();
        assert_eq!(pids(&r, "10"), None);
    }

    #[test]
    fn proc_tree_is_none_when_the_pane_pid_is_gone() {
        assert_eq!(pids(&fake_proc(&[]), "10"), None);
    }

    #[test]
    fn proc_tree_is_none_without_a_proc() {
        assert_eq!(proc_tree(std::path::Path::new("/nonexistent-proc"), "1"), None);
    }

    #[test]
    fn comm_norm_matches_what_procs_stored() {
        for (raw, want) in [
            ("claude", "claude"),
            ("-Bash", "bash"),
            ("/usr/bin/Node", "node"),
            ("2.1.318", "2.1.318"),
            ("a  b", "a b"),
        ] {
            assert_eq!(comm_norm(raw), want, "{raw}");
        }
        // agentish("claude") reads the tmux agent list, so only the pure half is pinned here
        assert!(version_shaped(&comm_norm("2.1.318\n")) && !version_shaped(&comm_norm("bash\n")));
    }

    /// The live check: a shell, a grandchild with a name past 15 chars, and
    /// `ps` must agree on pids and on names, truncation included.
    #[cfg(target_os = "linux")]
    #[test]
    fn lineage_on_the_real_proc_matches_ps() {
        let dir = std::env::temp_dir().join(format!("cockpit-lin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let long = dir.join("claude-with-a-very-long-name");
        std::os::unix::fs::symlink(
            Command::new("sh")
                .args(["-c", "command -v python3"])
                .output()
                .map(|o| String::from_utf8(o.stdout).unwrap())
                .unwrap()
                .trim(),
            &long,
        )
        .unwrap();
        let mut sh = Command::new("sh")
            .arg("-c")
            .arg(format!("{} -c 'import time; time.sleep(30)' & wait", long.display()))
            .spawn()
            .unwrap();
        let pid = sh.id().to_string();
        let mut want = vec![];
        for _ in 0..2000 {
            let mut t = procs();
            want = tree(&pid, &t)
                .into_iter()
                .map(|p| (p.clone(), t.comm.remove(&p).unwrap_or_default()))
                .collect();
            if want.len() == 2 && proc_tree(std::path::Path::new("/proc"), &pid).is_some_and(|v| v.len() == 2) {
                break;
            }
            std::thread::yield_now();
        }
        let got = lineage(&pid, &mut None);
        for (p, _) in &got {
            // SAFETY: kill takes plain integers and touches no memory.
            unsafe { libc::kill(p.parse().unwrap(), libc::SIGKILL) };
        }
        let _ = sh.wait();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got.len(), 2);
        assert_eq!(got, want);
        assert_eq!(got[1].1.len(), 15);
    }
}
