//! tmux and transcript reads. The frozen contract (#61) is "The freeze" in docs/tui.md.
pub mod fleet;
pub mod gitsafe;
pub mod info;
pub mod sessions;
pub mod transcript;

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
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
        } = self;
        [
            id, wher, name, cmd, title, msg, sess, path, root, spool, pid, review, doing, said, asked, loc, pm, win,
        ]
        .hash(h);
        (state, pinned, delivered).hash(h);
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
/// a control character such as TAB would split a `--rows` record.
fn first_line(s: &str) -> String {
    let one = s.trim().lines().next().unwrap_or("");
    one.chars()
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
}

thread_local! {
    /// Keyed on (len, mtime). The path is re-resolved every tick because
    /// `/compact` and `--resume` rotate the file.
    static TAILS: std::cell::RefCell<HashMap<String, (u64, SystemTime, Facts)>> =
        std::cell::RefCell::new(HashMap::new());
}

/// One row can be hundreds of KB, which a small window would read past.
const TAIL_FIRST: u64 = 256 * 1024;
const TAIL_MAX: u64 = 2 * 1024 * 1024;

fn read_facts(path: &str) -> Facts {
    let Ok(mut f) = File::open(path) else { return Facts::default() };
    let Ok(md) = f.metadata() else { return Facts::default() };
    let (len, mtime) = (md.len(), md.modified().unwrap_or(SystemTime::UNIX_EPOCH));
    let hit = TAILS.with(|c| {
        c.borrow()
            .get(path)
            .filter(|(l, m, _)| *l == len && *m == mtime)
            .map(|(_, _, v)| v.clone())
    });
    if let Some(fa) = hit {
        return fa;
    }
    let mut fa = parse(tail(&mut f, len, TAIL_FIRST));
    // Known gap: over 256 KB written after an open ask still finds a row, so
    // the window never grows to reach the ask.
    if fa.tool.is_empty() && fa.text.is_empty() && len > TAIL_FIRST {
        fa = parse(tail(&mut f, len, TAIL_MAX));
    }
    TAILS.with(|c| c.borrow_mut().insert(path.to_string(), (len, mtime, fa.clone())));
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
                    }
                    None => {
                        let mut interrupted = false;
                        for b in c.as_array().into_iter().flatten() {
                            if let Some(id) = b["tool_use_id"].as_str() {
                                asks.retain(|a| a != id);
                            }
                            // safe beside the tool_results: they carry no `text`
                            interrupted |= b["text"].as_str().is_some_and(|t| t.starts_with(INTERRUPT));
                        }
                        turn_over = interrupted;
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
                            if name == "AskUserQuestion"
                                && let Some(id) = b["id"].as_str()
                            {
                                asks.push(id.to_string());
                            }
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
fn read_transcript(p: &mut Pane) {
    if p.sess.is_empty() {
        return;
    }
    let fa = read_facts(&transcript_path(&p.sess, &p.path));
    // @panestate wins; the transcript speaks only for panes the hooks never touched.
    if p.state == State::Unknown {
        p.state = fa.state;
    }
    p.doing = fa.tool;
    p.said = fa.text;
    p.asked = fa.user;
    p.since = fa.at.and_then(age);
    p.asked_since = fa.user_at.and_then(age);
    p.idle_since = fa.newest.and_then(age);
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
fn agent_sessions() -> HashMap<String, String> {
    let out = Command::new("claude")
        .args(["agents", "--json"])
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    let v: Value = serde_json::from_slice(&out).unwrap_or(Value::Null);
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| Some((r["pid"].as_i64()?.to_string(), r["sessionId"].as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
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
        let base = comm.rsplit('/').next().unwrap_or(&comm);
        t.comm.insert(pid.into(), base.trim_start_matches('-').to_ascii_lowercase());
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

/// A live Claude renames itself to its version, so `2.1.246` and `2_1_246` are
/// agent names too.
fn agentish(comm: &str) -> bool {
    let b = comm.rsplit('/').next().unwrap_or(comm).trim_start_matches('-').to_ascii_lowercase();
    is_agent_cmd(&b) || version_shaped(&b)
}

fn version_shaped(s: &str) -> bool {
    let seps = s.chars().filter(|c| *c == '.' || *c == '_').count();
    seps >= 2 && s.starts_with(|c: char| c.is_ascii_digit()) && s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
}

fn find_session(pid: &str, t: &Procs, agents: &HashMap<String, String>) -> String {
    tree(pid, t).iter().find_map(|p| agents.get(p).cloned()).unwrap_or_default()
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

/// `dirs` is each pane's `(cwd, @cockpit_dir)`, in `ps` order.
fn fill_git(ps: &mut [Pane], dirs: &[(String, String)]) {
    // the tree each pane works in: @cockpit_dir over the cwd (#253)
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
            (p.loc, p.pm, p.root) = (loc.clone(), pm.clone(), root.clone());
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
);

/// `-` back to empty. A sentinelled option whose real value is a bare `-` would
/// read as unset; none of these ever holds one.
fn unsentinel(s: &str) -> String {
    if s == "-" { String::new() } else { s.to_string() }
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
        if f.len() < 13 {
            continue;
        }
        // `session:window`; a session name never holds a `:` (tmux swaps it)
        sessions.insert(f[1].rsplit_once(':').map_or(f[1], |(s, _)| s).to_string());
        // A pane carrying @panestate counts whatever it runs: the hooks are the
        // source of truth, and a wrapper hides `claude` behind its own argv0.
        let raw = unsentinel(f[4]);
        // The name is a fast path that skips the `ps` fork; the tree decides.
        let agent = !raw.is_empty() || agentish(f[3]) || {
            let t = procs_t.get_or_insert_with(procs);
            tree(f[9], t).iter().any(|p| t.comm.get(p).is_some_and(|c| agentish(c)))
        };
        if !agent {
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
            ..Default::default()
        };
        if p.sess.is_empty() {
            let agents = agents.get_or_insert_with(agent_sessions_cached);
            p.sess = find_session(&p.pid, procs_t.get_or_insert_with(procs), agents);
        }
        read_transcript(&mut p);
        (p.pinned, p.delivered) = spool_counts(&p.spool);
        // @cockpit_dir, last so a 13-field row from an older format still reads
        dirs.push((p.path.clone(), f.get(13).copied().unwrap_or_default().to_string()));
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
    let lines: Vec<&str> = out.trim_end().lines().collect();
    let text = lines[lines.len().saturating_sub(n)..].join("\n");
    Some(text[snap(&text, text.len().saturating_sub(PREVIEW_MAX))..].to_string())
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
}
