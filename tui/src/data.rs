//! tmux and transcript reads. The frozen contract (#61) is "The freeze" in docs/tui.md.
pub mod agents;
pub mod fleet;
// the board's transcript fold (#492)
mod fold;
pub mod gitsafe;
pub mod info;
pub mod sessions;
// the subagent tree (#313)
pub mod subagents;
pub mod transcript;

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

pub fn tmux_argv() -> Vec<String> {
    split_tmux_argv(std::env::var("COCKPIT_TMUX").ok().as_deref())
}

/// The argv a COCKPIT_TMUX value names; `None` (unset or not UTF-8) is plain `tmux`.
fn split_tmux_argv(value: Option<&str>) -> Vec<String> {
    value.unwrap_or("tmux").split_whitespace().map(String::from).collect()
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
    /// #460: `@cockpit_job_name`, the background job agent-poll bound this viewer
    /// pane to by its title; cleaned, since a title is untrusted. `""` when unbound.
    pub job: String,
    /// #454: the pane's own title (field 17), `✳ ` off, `""` when it is the host name;
    /// the label's fallback before the window name
    pub topic: String,
    /// #454: the last tool_use (`doing`) came back with `is_error`: a denial or a
    /// failed command. A done row then shows `said` instead.
    pub doing_failed: bool,
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
            job,
            topic,
            doing_failed,
        } = self;
        [
            id, wher, name, cmd, title, msg, sess, path, dir, root, spool, pid, review, doing, said, asked, loc, pm, win, job, topic,
        ]
        .hash(h);
        (state, pinned, delivered, subs_n, ended, doing_failed).hash(h);
        // Sub hashes only what changes its drawing: never `started`, tokens by the thousand.
        subs.hash(h);
    }
}

impl Pane {
    pub fn label(&self) -> &str {
        // @cockpit_title, else the pane's own title (#454), else the window name
        [&self.title, &self.topic, &self.name]
            .into_iter()
            .find(|s| !s.is_empty())
            .map_or("(no title)", |s| s.as_str())
    }
}

pub fn hash_rows(ps: &[Pane], extra: &str) -> u64 {
    let mut h = DefaultHasher::new();
    ps.hash(&mut h);
    extra.hash(&mut h);
    h.finish()
}

/// The last `cap` bytes as whole rows, decoded lossily: the window can open
/// mid-codepoint. Every line returned is a complete record. The board's own
/// transcript reads the delta instead (`fold::windowed`), which must equal this.
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
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// #454: the newest tool_use's result came back with `is_error`
    tool_failed: bool,
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
    // Known gap: over 256 KB written after an open ask still finds a row, so
    // the window never grows to reach the ask.
    let mut fa = fold::windowed(path, &mut f, &md, first, TAIL_MAX);
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

/// The facts of whole rows, oldest first.
#[cfg(test)]
fn parse(lines: Vec<String>) -> Facts {
    let mut f = fold::Fold::default();
    lines.iter().for_each(|l| f.step(0, l));
    f.view(0)
}

/// doing / said / asked and their ages, from the tail of the session transcript,
/// which `transcript::locate` finds from `opt` (@cockpit_transcript), the session id
/// and the cwd. Returns the transcript's cwd, `""` when it has none.
fn read_transcript(p: &mut Pane, opt: &str) -> String {
    let Some(t) = transcript::locate(opt, &p.sess, &p.path) else {
        return String::new();
    };
    let fa = read_facts(&t.to_string_lossy());
    // @panestate wins; the transcript speaks only for panes the hooks never touched.
    if p.state == State::Unknown {
        p.state = fa.state;
    }
    // #453: the hook's text is generic; an open question says what is owed
    if p.state == State::NeedsInput && !fa.ask.is_empty() {
        p.msg = fa.ask;
    }
    p.doing = fa.tool;
    p.doing_failed = fa.tool_failed;
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
        // the lookup's child and all under it are the board's, not the pane's agent
        if !seen.insert(p.clone()) || agents::is_lookup_child(&p) {
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
        // the lookup's child and all under it are the board's, not the pane's agent
        if !seen.insert(p.clone()) || agents::is_lookup_child(&p) {
            continue;
        }
        if d < MAX_DEPTH {
            let tasks = root.join(&p).join("task");
            let mut kids = vec![];
            for t in std::fs::read_dir(&tasks).ok()? {
                let s = std::fs::read_to_string(t.ok()?.path().join("children")).ok()?;
                kids.extend(s.split_whitespace().map(str::to_string));
            }
            // ps lists siblings by pid, the kernel by creation: find_session takes the first hit
            kids.sort_by_key(|k| k.parse::<u64>().unwrap_or(0));
            q.extend(kids.into_iter().map(|c| (c, d + 1)));
        }
        out.push(p);
    }
    Some(out)
}

/// `pid` and everything under it with each name, in [`tree`] order: from
/// `procs_t` when `ps` already ran this refresh, else a [`proc_tree`] walk,
/// else one `ps` kept in `procs_t` for the next ask. `COCKPIT_PROC_DIR` moves
/// the proc root, so a test can aim the walk at a fake one or at none.
fn lineage(pid: &str, procs_t: &mut Option<Procs>) -> Vec<(String, String)> {
    let root = std::env::var_os("COCKPIT_PROC_DIR").map_or_else(|| "/proc".into(), std::path::PathBuf::from);
    if procs_t.is_none()
        && let Some(ps) = proc_tree(&root, pid)
    {
        return ps
            .into_iter()
            .map(|p| {
                let c = std::fs::read_to_string(root.join(&p).join("comm")).unwrap_or_default();
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

/// Claude Code's version-shaped process title (`2.1.318`, `2_1_318`): a leading
/// digit, then only digits, dots and underscores, two or more of them dots or
/// underscores. The one copy of the rule in Rust: basket's claude_cmd calls it
/// (#776). scripts/agent-poll's version_cmd is its shell twin.
pub(crate) fn version_shaped(s: &str) -> bool {
    let seps = s.chars().filter(|c| *c == '.' || *c == '_').count();
    seps >= 2 && s.starts_with(|c: char| c.is_ascii_digit()) && s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
}

fn find_session(pid: &str, procs_t: &mut Option<Procs>) -> String {
    agents::resolve(&lineage(pid, procs_t))
}

/// repo⑂branch, the `±` string and the repo root, for one path.
pub fn git_facts(path: &str) -> (String, String, String) {
    fleet::scan(&[path.to_string()]).facts.remove(path).unwrap_or_default()
}

const GIT_TTL: Duration = Duration::from_secs(10);

/// #492: a pane changing state refetches its dir, but not twice inside this
/// gap. The flip waits for the gap to pass, so a burst of flips costs one fetch.
const GIT_FLIP_GAP: Duration = Duration::from_secs(3);

struct GitEntry {
    at: Instant,
    /// when the entry expires: [`GIT_TTL`] after a refetch, a spread of
    /// 2 s to [`GIT_TTL`] for a dir seen the first time (see [`first_life`])
    due: Instant,
    /// a pane here changed state since `at`
    flip: bool,
    facts: fleet::Facts,
}

#[derive(Default)]
struct GitCache {
    /// by path
    facts: HashMap<String, GitEntry>,
    /// each pane's state at the last fetch, by pane id
    seen: HashMap<String, State>,
    /// dirs met for the first time so far, the jitter's salt
    firsts: u64,
}

thread_local! {
    /// Five git forks per repo are worth paying every 10 s, not every tick;
    /// a pane changing state is when its diff most likely moved, so it refetches.
    static GIT: std::cell::RefCell<GitCache> = std::cell::RefCell::default();
}

/// #492: how long a dir met for the first time lives: 2, 4, 6, 8 or 10 s,
/// fixed by its path and arrival order. Fifty dirs met in one tick would
/// otherwise expire in one tick, every 10 s, and fork 300 git processes at once;
/// in five slots a refresh is about a fifth of them, still big enough to run
/// in parallel, and the period stays 10 s so forks per window do not grow.
fn first_life(path: &str, n: u64) -> Duration {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    (path, n).hash(&mut h);
    GIT_TTL / 5 * (1 + (h.finish() % 5) as u32)
}

/// `dirs` is each pane's work dir candidates (`fleet::work_dirs`), in `ps` order.
fn fill_git(ps: &mut [Pane], dirs: &[[String; 3]]) {
    // the tree each pane works in: @cockpit_dir, the transcript's cwd, the cwd (#253, #452)
    let dirs = fleet::work_dirs(dirs);
    GIT.with(|c| fill_git_at(&mut c.borrow_mut(), ps, &dirs, Instant::now(), &|m| fleet::scan(m).facts));
}

/// [`fill_git`] at time `now`, asking `scan` for the dirs that need facts. An
/// entry is refetched when it is past its `due`, or when a pane in it changed
/// state and the entry is at least [`GIT_FLIP_GAP`] old. Nothing else is
/// refetched, so no entry is ever older than [`GIT_TTL`] plus one tick: the
/// unstaged edits and untracked files of the diff stat show up within the
/// same bound as before the stagger.
fn fill_git_at(
    c: &mut GitCache,
    ps: &mut [Pane],
    dirs: &[String],
    now: Instant,
    scan: &dyn Fn(&[String]) -> HashMap<String, fleet::Facts>,
) {
    for (p, d) in ps.iter().zip(dirs) {
        if c.seen.get(&p.id) != Some(&p.state)
            && let Some(e) = c.facts.get_mut(d)
        {
            e.flip = true;
        }
    }
    c.seen = ps.iter().map(|p| (p.id.clone(), p.state)).collect();
    let mut missing: Vec<String> = dirs
        .iter()
        .filter(|d| {
            c.facts
                .get(*d)
                .is_none_or(|e| now >= e.due || (e.flip && now.duration_since(e.at) >= GIT_FLIP_GAP))
        })
        .cloned()
        .collect();
    missing.sort();
    missing.dedup();
    // one scan per distinct root, in parallel, however many dirs share it
    let mut found = scan(&missing);
    for d in missing {
        let life = if c.facts.contains_key(&d) {
            GIT_TTL
        } else {
            c.firsts += 1;
            first_life(&d, c.firsts)
        };
        let facts = found.remove(&d).unwrap_or_default();
        c.facts.insert(
            d,
            GitEntry {
                at: now,
                due: now + life,
                flip: false,
                facts,
            },
        );
    }
    let used: HashSet<&String> = dirs.iter().collect();
    c.facts.retain(|d, _| used.contains(d));
    for (p, d) in ps.iter_mut().zip(dirs) {
        let (loc, pm, root) = &c.facts[d].facts;
        (p.loc, p.pm, p.root, p.dir) = (loc.clone(), pm.clone(), root.clone(), d.clone());
    }
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
    AGENT_GLOB.with(|g| glob_has(g, cmd))
}

/// Whether `cmd` is an entry of `glob`, a `|a|b|` list, in any case (#508). The test
/// tmux's `#{m/i:*|cmd|*,glob}` makes: `|cmd|` inside the glob, both sides folded, so
/// an entry `Cursor` is the command `cursor` and a glob with no closing bar has no entry.
fn glob_has(glob: &str, cmd: &str) -> bool {
    !cmd.is_empty() && glob.to_lowercase().contains(&format!("|{}|", cmd.to_lowercase()))
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
    // #460: 16 the background job agent-poll bound a viewer pane to
    "\u{1f}#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{@cockpit_job_name}}}}",
    // #454: 17 the pane's own title, the topic Claude sets, as the status bar reads it:
    // `✳ ` off, and nothing when it is still tmux's default, the host name
    "\u{1f}#{?#{==:#{pane_title},#{host_short}},,#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{s/^✳ //:#{pane_title}}}}}}",
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
    let mut procs_t = None;
    let (mut ps, mut dirs) = (vec![], vec![]);
    for l in out.lines() {
        let f: Vec<&str> = l.split(SEP).collect();
        // FIELDS has 18; the 13 before @cockpit_dir are the floor, so an older format reads
        if f.len() < 13 {
            continue;
        }
        // `session:window`; a session name never holds a `:` (tmux swaps it)
        sessions.insert(f[1].rsplit_once(':').map_or(f[1], |(s, _)| s).to_string());
        let Some((mut p, live_by)) = row_pane(&f, &mut procs_t, true) else {
            continue;
        };
        log_ended_flip(&p, live_by);
        if p.sess.is_empty() {
            p.sess = find_session(&p.pid, &mut procs_t);
        }
        dirs.push(work_candidates(&mut p, &f));
        (p.pinned, p.delivered) = spool_counts(&p.spool);
        p.subs_n = f.get(14).and_then(|n| n.parse().ok()).unwrap_or(0);
        // a pane title, so untrusted: control and bidi characters go
        p.job = crate::pick::clean(f.get(16).copied().unwrap_or_default()).trim().to_string();
        // a pane title, so untrusted too
        p.topic = crate::pick::clean(f.get(17).copied().unwrap_or_default()).trim().to_string();
        if let Some(d) = subagents::dir_of(f.get(15).copied().unwrap_or_default(), &p.sess, &p.path) {
            p.subs = subagents::read(&d, sessions::now());
        }
        ps.push(p);
    }
    // #492: the answer comes later, from a thread of its own; this fetch never waits on it
    agents::flush();
    fill_git(&mut ps, &dirs);
    ps.sort_by_key(|p| p.state);
    Some((ps, sessions))
}

/// What [`row_pane`] returns: the Pane and what found the agent.
type RowPane = Option<(Pane, &'static str)>;

/// One FIELDS row to its Pane, or `None` when the row is not an agent's: the
/// board's agent test. Shared by the fetch and `workdir` (#500), so the two
/// cannot disagree on which panes are agents. The second value names what found
/// the agent, for the ended-flip log. `want_ended` false skips what only `ended`
/// needs (#507): a marked pane is an agent's without its process tree, so a
/// caller that never reads `ended` forks no `ps` for it, and gets `ended` false.
fn row_pane(f: &[&str], procs_t: &mut Option<Procs>, want_ended: bool) -> RowPane {
    // the floor list_fleet names
    if f.len() < 13 {
        return None;
    }
    // A pane carrying @panestate counts whatever it runs: the hooks are the
    // source of truth, and a wrapper hides `claude` behind its own argv0.
    let raw = unsentinel(f[4]);
    // The name is a fast path that skips the `ps` fork; the tree decides.
    let by_cmd = agentish(f[3]);
    let mut in_tree = || lineage(f[9], procs_t).iter().any(|(_, c)| agentish(c));
    // #301: a marked pane's verdict is cached, or every ended row forks `ps` each tick
    let by_tree = !by_cmd
        && if raw.is_empty() {
            in_tree()
        } else {
            want_ended && tree_cached((f[0], f[9], f[3]), in_tree)
        };
    let live = by_cmd || by_tree;
    if raw.is_empty() && !live {
        return None;
    }
    let p = Pane {
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
        ended: want_ended && ended(&raw, live, AGENT_GLOB.with(|g| !g.is_empty())),
        ..Default::default()
    };
    let by = if by_cmd {
        "command"
    } else if by_tree {
        "tree"
    } else {
        "none"
    };
    Some((p, by))
}

/// Reads `p`'s transcript into it and returns its work dir's candidates (#452), in
/// the order `fleet::work_dirs` tries them. @cockpit_dir, @cockpit_subs and
/// @cockpit_transcript come after the 13 a row from an older format has, so such a
/// row still reads: 13 to 17 are optional.
fn work_candidates(p: &mut Pane, f: &[&str]) -> [String; 3] {
    let said_cwd = read_transcript(p, f.get(15).copied().unwrap_or_default());
    let opt = f.get(13).copied().unwrap_or_default().to_string();
    [opt, said_cwd, p.path.clone()]
}

/// #500: the board's `Pane.dir` for one pane, off one `display-message` of the
/// board's own FIELDS and the same per-row code as the fetch. `None` for a pane
/// that is not there, is not an agent's, or whose dir is not on disk (#507). Unlike the fetch it never
/// runs `claude agents` (hundreds of ms): a pane with no @cockpit_session has no
/// transcript to read, so its dir is @cockpit_dir, else its launch dir.
pub fn workdir(pane: &str) -> Option<String> {
    let (mut p, f) = one_row(pane)?;
    let f: Vec<&str> = f.iter().map(String::as_str).collect();
    let cands = work_candidates(&mut p, &f);
    // #507: a dir that is not on disk (removed, or a name tmux's lossy decode
    // changed) is no dir: a script must not cd into it
    fleet::work_dirs(&[cands]).pop().filter(|d| std::path::Path::new(d).is_dir())
}

/// #519: the board's agent test for one pane (`row_pane`: @panestate, an agentish
/// command, a version-shaped name, the process tree), so the info screen cannot
/// decide differently from the board which panes have a transcript to read.
pub fn is_agent(pane: &str) -> bool {
    one_row(pane).is_some()
}

/// One pane's FIELDS row, read with one `display-message`, through the fetch's
/// `row_pane`: its Pane and the fields, or `None` when it is not there or not an
/// agent's.
fn one_row(pane: &str) -> Option<(Pane, Vec<String>)> {
    let out = tmux(&["display-message", "-p", "-t", pane, FIELDS])?;
    let f: Vec<&str> = out.trim_end_matches('\n').split(SEP).collect();
    // A missing pane exits 0 with every field empty: the resolved id is the test,
    // before the agent test can fork `ps` for a pid that is not there.
    if f[0] != pane {
        return None;
    }
    // workdir and is_agent never read `ended`
    let (p, _) = row_pane(&f, &mut None, false)?;
    Some((p, f.iter().map(|s| s.to_string()).collect()))
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
    let a = preview_args(id, n);
    let out = tmux(&a.iter().map(String::as_str).collect::<Vec<_>>())?;
    Some(shape_preview(&out, n))
}

/// The tmux argv of one preview's capture, the newest `n` rows of `id`. -J so
/// a wrap point cannot split an ANSI sequence and hand the parser halves.
pub fn preview_args(id: &str, n: usize) -> [String; 7] {
    ["capture-pane", "-ep", "-J", "-t", id, "-S", &format!("-{n}")].map(String::from)
}

/// A preview off the stdout of [`preview_args`]: its newest `n` lines, capped.
pub fn shape_preview(out: &str, n: usize) -> String {
    // #284: ansi-to-tui drops the rest of a line after an OSC 8 link that ends
    // in `ESC \`, the form tmux emits; no OSC draws anything in a preview
    let out = strip_osc(out);
    let lines: Vec<&str> = out.trim_end().lines().collect();
    cap(&lines[lines.len().saturating_sub(n)..].join("\n"))
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
    use serde_json::Value;

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

    fn pane(id: &str, state: State) -> Pane {
        Pane {
            id: id.into(),
            state,
            ..Default::default()
        }
    }

    /// A scan that answers each path with itself as its root and counts the paths asked.
    fn counted(n: &std::cell::Cell<usize>) -> impl Fn(&[String]) -> HashMap<String, fleet::Facts> + '_ {
        move |m| {
            n.set(n.get() + m.len());
            m.iter().map(|d| (d.clone(), ("loc".into(), "clean".into(), d.clone()))).collect()
        }
    }

    /// 50 dirs met in one tick must not all expire in one tick: over 30 s of
    /// 1 s ticks no tick fetches more than 20 of them, and no dir waits
    /// longer than the 10 s TTL (plus the tick) between two fetches.
    #[test]
    fn git_first_fetches_spread_and_stay_inside_the_ttl() {
        let dirs: Vec<String> = (0..50).map(|i| format!("/r/{i}")).collect();
        let mut ps: Vec<Pane> = (0..50).map(|i| pane(&format!("%{i}"), State::Running)).collect();
        let (mut c, t0) = (GitCache::default(), Instant::now());
        let (n, mut last) = (std::cell::Cell::new(0), HashMap::new());
        let (mut worst_tick, mut worst_gap) = (0, 0);
        for s in 0..=30u64 {
            let (before, now) = (n.get(), t0 + Duration::from_secs(s));
            fill_git_at(&mut c, &mut ps, &dirs, now, &counted(&n));
            if s > 0 {
                worst_tick = worst_tick.max(n.get() - before); // tick 0 is the cold fetch of all 50
            }
            for d in dirs.iter().filter(|d| c.facts[*d].at == now) {
                worst_gap = worst_gap.max(s - last.insert(d, s).unwrap_or(s));
            }
        }
        assert!(worst_tick <= 20, "a tick refetched {worst_tick} dirs");
        assert!(worst_gap <= 10, "a dir waited {worst_gap} s");
    }

    fn git_ok(dir: &std::path::Path, args: &[&str]) {
        let st = std::process::Command::new("git").arg("-C").arg(dir).args(args).status().unwrap();
        assert!(st.success(), "git {args:?}");
    }

    /// The diff stat counts unstaged edits, which no index or HEAD stamp shows:
    /// an edit made after a fetch is on the row within the TTL, tick by tick.
    #[test]
    fn git_unstaged_edit_shows_within_the_ttl() {
        let d = std::env::temp_dir().join(format!("cockpit-fillgit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        git_ok(&d, &["init", "-q", "-b", "main"]);
        std::fs::write(d.join("f"), "a\n").unwrap();
        git_ok(&d, &["add", "f"]);
        git_ok(&d, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x"]);
        let dir = d.display().to_string();
        let mut ps = vec![pane("%1", State::Running)];
        let (mut c, t0) = (GitCache::default(), Instant::now());
        let real = |m: &[String]| fleet::scan(m).facts;
        fill_git_at(&mut c, &mut ps, std::slice::from_ref(&dir), t0, &real);
        assert_eq!(ps[0].pm, "clean");
        // past the first (jittered) expiry, so the 10 s period is what is tested
        let at = |s| t0 + Duration::from_secs(s);
        (1..=10).for_each(|s| fill_git_at(&mut c, &mut ps, std::slice::from_ref(&dir), at(s), &real));
        std::fs::write(d.join("f"), "a\nb\n").unwrap(); // unstaged: index and HEAD stay put
        let seen = (11..=20)
            .find(|s| {
                fill_git_at(&mut c, &mut ps, std::slice::from_ref(&dir), at(*s), &real);
                ps[0].pm == "+1 -0"
            })
            .expect("the unstaged edit never reached the row")
            - 10;
        let _ = std::fs::remove_dir_all(&d);
        assert!(seen <= 10, "shown after {seen} s");
    }

    /// Five flips inside the gap cost one fetch, at the gap; a flip on an old
    /// entry still refetches at once.
    #[test]
    fn git_state_flips_debounce_to_one_fetch_per_dir() {
        let dirs = vec!["/r/a".to_string(), "/r/a".to_string()];
        let mut ps = vec![pane("%1", State::Running), pane("%2", State::Running)];
        let (mut c, t0, n) = (GitCache::default(), Instant::now(), std::cell::Cell::new(0));
        let ms = |m| t0 + Duration::from_millis(m);
        fill_git_at(&mut c, &mut ps, &dirs, ms(0), &counted(&n));
        assert_eq!(n.get(), 1, "two panes, one dir, one fetch");
        c.facts.get_mut("/r/a").unwrap().due = ms(60_000); // only flips can refetch now
        for (i, m) in [200, 400, 600, 800, 1000].into_iter().enumerate() {
            let st = if i % 2 == 0 { State::Done } else { State::Running };
            (ps[0].state, ps[1].state) = (st, st);
            fill_git_at(&mut c, &mut ps, &dirs, ms(m), &counted(&n));
        }
        assert_eq!(n.get(), 1, "flips inside the gap fetched");
        fill_git_at(&mut c, &mut ps, &dirs, ms(3000), &counted(&n));
        assert_eq!(n.get(), 2, "the debounced flip is fetched once, at the gap");
        fill_git_at(&mut c, &mut ps, &dirs, ms(3500), &counted(&n));
        assert_eq!(n.get(), 2, "and not again");
        ps[0].state = State::Error; // an entry older than the gap refetches at once
        fill_git_at(&mut c, &mut ps, &dirs, ms(7000), &counted(&n));
        assert_eq!(n.get(), 3);
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
    fn proc_tree_lists_siblings_by_pid_as_ps_does() {
        let r = fake_proc(&[("10", &["13 11 12 "]), ("11", &[""]), ("12", &[""]), ("13", &[""])]);
        let mut t = Procs::default();
        t.kids.insert("10".into(), vec!["11".into(), "12".into(), "13".into()]);
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

    /// #508: an `@cockpit_agents` entry matches the command in any case, both sides folded.
    #[test]
    fn glob_entries_match_in_any_case() {
        let g = "|Cursor|OpenCode|claude|";
        for (cmd, want) in [
            ("cursor", true),   // a leading capital in the entry
            ("opencode", true), // a mixed-case entry
            ("OPENCODE", true),
            ("CLAUDE", true), // an upper-case command, a lower-case entry
            ("open", false),  // a part of an entry is not one
            ("bash", false),
            ("", false),
        ] {
            assert_eq!(glob_has(g, cmd), want, "{cmd:?}");
        }
        assert!(!glob_has("", "claude"), "an empty glob holds nothing");
    }

    /// #508: a hand-set glob reads as tmux reads it, `#{m/i:*|cmd|*,glob}`. Each `want` is
    /// what a private tmux 3.8 printed for that pair under a UTF-8 locale.
    #[test]
    fn glob_matches_as_tmux_m_i_does() {
        let rows = [
            ("claude", "claude", false),      // no bars: no entry
            ("claude|codex", "codex", false), // the last entry needs its closing bar
            ("|claude", "claude", false),
            ("||claude|", "claude", true), // an empty entry is harmless
            ("|über|", "ÜBER", true),      // non-ASCII folds too
            ("|ǆ|", "ǅ", true),            // a title-case letter
        ];
        // every row that differs, so one red run names them all
        let wrong: Vec<_> = rows.iter().filter(|(g, c, w)| glob_has(g, c) != *w).collect();
        assert!(wrong.is_empty(), "differs from tmux: {wrong:?}");
    }

    /// Kills the child on drop, so a failed assert cannot leave it running.
    #[cfg(target_os = "linux")]
    struct Reap(std::process::Child);
    #[cfg(target_os = "linux")]
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = Command::new("pkill").args(["-KILL", "-P", &self.0.id().to_string()]).status();
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A child named past 15 chars: the walk and `ps` agree on pids and on the
    /// truncated name.
    #[cfg(target_os = "linux")]
    #[test]
    fn lineage_on_the_real_proc_matches_ps() {
        let dir = std::env::temp_dir().join(format!("cockpit-lin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let py = Command::new("sh").args(["-c", "command -v python3"]).output().unwrap().stdout;
        let long = dir.join("claude-with-a-very-long-name");
        std::os::unix::fs::symlink(String::from_utf8(py).unwrap().trim(), &long).unwrap();
        let cmd = format!("{} -c 'import time; time.sleep(30)' & wait", long.display());
        let sh = Reap(Command::new("sh").arg("-c").arg(cmd).spawn().unwrap());
        let pid = sh.0.id().to_string();
        let mut t = procs();
        while tree(&pid, &t).len() < 2 {
            t = procs();
        }
        let want: Vec<_> = tree(&pid, &t).into_iter().map(|p| (p.clone(), t.comm[&p].clone())).collect();
        assert_eq!(lineage(&pid, &mut None), want);
        assert_eq!(want[1].1.len(), 15);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_tmux_argv_unset_is_plain_tmux() {
        assert_eq!(split_tmux_argv(None), ["tmux"]);
    }

    #[test]
    fn split_tmux_argv_splits_a_socket_flag_into_words() {
        assert_eq!(split_tmux_argv(Some("tmux -L sock")), ["tmux", "-L", "sock"]);
    }

    /// Pins the limit docs/tui.md's COCKPIT_TMUX row states (#757 row 20).
    /// Delete or flip this test when a quoting syntax lands.
    #[test]
    fn known_limit_a_spaced_path_is_split_delete_when_quoting_lands() {
        assert_eq!(split_tmux_argv(Some("/opt/my tmux/tmux")), ["/opt/my", "tmux/tmux"]);
    }

    /// The same limit: quotes are not a syntax, so they stay in the words.
    #[test]
    fn known_limit_quotes_do_not_group_words_delete_when_quoting_lands() {
        let got = split_tmux_argv(Some("\"/opt/my tmux/tmux\""));
        assert_eq!(got, ["\"/opt/my", "tmux/tmux\""]);
    }
}
