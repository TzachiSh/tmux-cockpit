//! A pane's subagents (#313): what each one is, off `agent-<id>.meta.json`, and when it
//! started and its tokens, off `agent-<id>.jsonl`, both in the session's `subagents/` dir.
//! Claude Code 2.1.317 writes the meta file (`persistAgentMetadata`) when the agent starts
//! and sets `finished: true` when its task leaves `running`, whatever the outcome.
//! Every file is cached on (mtime, len), so a quiet session costs a stat per file.
//! The hooks' own record (#361), `<state>/tmux-cockpit/subs/<session_id>.log`, decides what the
//! files above cannot say: a teammate that is idle, an agent TaskStop ended (#403), or one
//! missing from the in-flight set at the parent's turn end, which the hooks keep apart in
//! `<session_id>.live` (#423).
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SubState {
    #[default]
    Running,
    /// `finished: true`, and the newest assistant row is no API error
    Done,
    /// finished, and the newest assistant row is an API error, unless the user stopped it
    Failed,
    /// `stoppedByUser: true`. The user's action wins: a stopped agent whose newest row is
    /// also an API error is Stopped, not Failed. Also a TaskStop the hooks recorded, and an
    /// agent the log had running that a later Stop's in-flight set no longer lists (#403)
    Stopped,
    /// a teammate between turns: its last hook event is a stop or an idle. It is not running,
    /// and nothing says whether it has ended
    Idle,
    /// its last hook event is a start, and its transcript has not been written for
    /// `END_UNKNOWN_AFTER` seconds, and no TaskStop or Stop reconcile says it ended: it was
    /// ended without a hook (a stop from the UI before the next Stop). Never Running, and not
    /// a failure
    EndUnknown,
}

/// Seconds without a transcript write after which an agent that never finished is `EndUnknown`.
/// A live agent writes at every tool call, but nothing bounds a quiet stretch: a permission
/// prompt waiting on the human, a long Bash or MCP call. Such an agent reads EndUnknown while
/// it lives. That is why the state is "end unknown", not "ended".
pub const END_UNKNOWN_AFTER: u64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SubKind {
    /// the Agent tool's subagent, the kind a Stop's `background_tasks` counts
    #[default]
    Subagent,
    /// `isFork: true`
    Fork,
    /// `teamName` set: an agent-team teammate, which stays running while its team lives
    Teammate,
}

/// One subagent. Hashed only on what changes its drawing: `id` and `started` never are.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sub {
    /// the agent id, `<id>` in `agent-<id>.meta.json`
    pub id: String,
    pub kind: SubKind,
    /// `agentType`: `Explore`, `general-purpose`, a custom agent's name
    pub ty: String,
    /// `description`, the Agent call's short task line
    pub desc: String,
    pub state: SubState,
    /// unix seconds of the transcript's first row; None before it has one
    pub started: Option<u64>,
    /// unix seconds the agent stopped working: None while it runs, and for a finished one
    /// with no transcript. The newest transcript row's `timestamp`, else the transcript's
    /// mtime. A stopped one takes the later of that and the meta file's mtime (the write
    /// that set `stoppedByUser`), and with no transcript the meta mtime alone. Not hashed,
    /// like `started`.
    pub finished_at: Option<u64>,
    /// unix seconds of the agent's last sign of life: the newest of its newest transcript row
    /// (else the transcript's mtime) and its last hook event. The age of an `EndUnknown` or
    /// `Idle` agent. Not hashed.
    pub active_at: Option<u64>,
    /// the newest usage row's input, output and cache tokens summed: the figure Claude Code
    /// reports as `totalTokens`, the context of its last request, not a sum over turns
    pub tokens: u64,
}

impl Hash for Sub {
    fn hash<H: Hasher>(&self, h: &mut H) {
        (self.kind, &self.ty, &self.desc, self.state, self.tokens / 1000).hash(h);
    }
}

/// A meta file is a few hundred bytes; anything past this is not one.
const META_MAX: u64 = 64 * 1024;
/// The first row holds the start. claude writes `timestamp` after `message`, so the whole
/// row is read: the prompt, rarely past a few KB.
const HEAD_MAX: u64 = 1024 * 1024;
/// The newest usage row is the last assistant row, which is small; a huge tool result after
/// it can push it out of this window, and the last figure read then stands.
const TAIL_MAX: u64 = 256 * 1024;
/// Cached dirs past this are dropped whole: a board left open for days meets many sessions.
const DIRS_MAX: usize = 64;
/// The hook log is read from its last 64 KB: a few hundred events, the newest of each agent.
const LOG_MAX: u64 = 64 * 1024;
/// The hook writes a set of at most 3890 bytes; a `.live` file past this is not one.
const LIVE_MAX: u64 = 8 * 1024;

type Stamp = (SystemTime, u64);
/// A hook file's stamp and inode.
type FileKey = (Stamp, u64);

#[derive(Clone, Default)]
struct Meta {
    kind: SubKind,
    /// `name`: a teammate's, what a TaskStop of it names
    name: String,
    ty: String,
    desc: String,
    finished: bool,
    stopped: bool,
}

#[derive(Clone, Default)]
struct Rows {
    /// the first row was read whole, so `started` is final
    head: bool,
    started: Option<u64>,
    /// the `timestamp` of the newest tail row that has one
    last: Option<u64>,
    tokens: u64,
    api_error: bool,
}

#[derive(Default)]
struct Cached {
    meta: Option<(Stamp, Meta)>,
    rows: Option<(Stamp, Rows)>,
}

/// One hook event.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ev {
    Start,
    Stop,
    Idle,
    /// ended: a TaskStop of it, or absent from a later Stop's in-flight set
    Kill,
}

/// One agent's newest event: its epoch and log line, and the epoch and line of its newest start.
#[derive(Clone, Copy)]
struct Rec {
    ev: Ev,
    t: u64,
    line: usize,
    start: Option<(u64, usize)>,
}

/// A teammate's TaskStop: its log line and epoch, the name the model gave, its description.
#[derive(Clone)]
struct TmKill {
    line: usize,
    t: u64,
    name: String,
    desc: String,
}

/// The newest Stop's in-flight set: its epoch, the subagents' ids, the teammates' descriptions
/// (a teammate's id there is a task id no file holds).
#[derive(Clone)]
struct Live {
    t: u64,
    ids: HashSet<String>,
    descs: HashSet<String>,
}

/// The log's tail, folded, and the `.live` file beside it.
#[derive(Clone, Default)]
struct Log {
    ids: HashMap<String, Rec>,
    kills: Vec<TmKill>,
    live: Option<Live>,
}

thread_local! {
    /// Both files of a session, each on its own stamp: a `.log` holds its fold, a `.live` a
    /// `Log` with only `live` set.
    static LOGS: RefCell<HashMap<PathBuf, (FileKey, Log)>> = RefCell::default();
    static CACHE: RefCell<HashMap<PathBuf, HashMap<String, Cached>>> = RefCell::default();
    static READS: Cell<u64> = const { Cell::new(0) };
}

/// Files opened for reading on this thread so far: the cache's audit.
pub fn reads() -> u64 {
    READS.with(Cell::get)
}

/// The hooks' log dir: `$XDG_STATE_HOME/tmux-cockpit/subs`, else `~/.local/state/...`, as
/// `scripts/lib/subslog.sh` has it. None with neither set.
pub fn state_dir(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let base = match (xdg, home) {
        (Some(x), _) if !x.is_empty() => PathBuf::from(x),
        (_, Some(h)) if !h.is_empty() => PathBuf::from(h).join(".local").join("state"),
        _ => return None,
    };
    Some(base.join("tmux-cockpit").join("subs"))
}

/// A description as `scripts/lib/subslog.sh` writes it, the JSON string's body with its
/// escapes, decoded and with control characters dropped as `text` drops them.
fn unjson(body: &str) -> Option<String> {
    let s: String = serde_json::from_str(&format!("\"{body}\"")).ok()?;
    Some(s.chars().filter(|c| !c.is_control()).collect())
}

/// The log's lines folded. Rows, tab-separated after `<epoch>\t<event>`:
/// - `start|stop|idle <agent_id>`: each agent's newest event. A `stop` or `idle` counts only
///   after a `start` of the same agent: Claude Code's internal agents send a stop with no start.
/// - `kill <agent_id>`: a TaskStop of a plain agent. Only a later `start` (a resume) undoes it;
///   the `stop` a teammate's TaskStop also fires does not.
/// - `kill-tm <name> <description>`: a TaskStop of a teammate, mapped by the reader.
///
/// Any other line, a `live` row as #405's hook wrote it among them, and a row with a field
/// that does not decode, is skipped. An id is not checked: only a plain agent id is ever looked
/// up, and the tail bounds the map.
fn fold_log(lines: &[String]) -> Log {
    let mut log = Log::default();
    for (line, l) in lines.iter().enumerate() {
        let f: Vec<&str> = l.split('\t').collect();
        let [t, e, rest @ ..] = f.as_slice() else { continue };
        let Ok(t) = t.parse::<u64>() else { continue };
        match (*e, rest) {
            ("start", [id]) => {
                let start = Some((t, line));
                log.ids.insert(
                    id.to_string(),
                    Rec {
                        ev: Ev::Start,
                        t,
                        line,
                        start,
                    },
                );
            }
            ("stop" | "idle", [id]) => {
                if let Some(r) = log.ids.get_mut(*id).filter(|r| r.ev != Ev::Kill) {
                    (r.ev, r.t, r.line) = (if *e == "stop" { Ev::Stop } else { Ev::Idle }, t, line);
                }
            }
            ("kill", [id]) => {
                let start = log.ids.get(*id).and_then(|r| r.start);
                log.ids.insert(
                    id.to_string(),
                    Rec {
                        ev: Ev::Kill,
                        t,
                        line,
                        start,
                    },
                );
            }
            ("kill-tm", [name, desc]) => {
                let Some(desc) = unjson(desc) else { continue };
                if !name.is_empty() || !desc.is_empty() {
                    log.kills.push(TmKill {
                        line,
                        t,
                        name: name.to_string(),
                        desc,
                    });
                }
            }
            _ => {}
        }
    }
    log
}

/// A `.live` file: one line, `<epoch>\t<id,id,...>\t<description>...`, the in-flight set at the
/// newest Stop. Anything else, a bad id or a description that does not decode, is no set.
fn fold_live(lines: &[String]) -> Log {
    let live = (|| {
        let [l] = lines else { return None };
        let f: Vec<&str> = l.split('\t').collect();
        let [t, ids, descs @ ..] = f.as_slice() else { return None };
        let t = t.parse::<u64>().ok()?;
        let ids: HashSet<String> = ids.split(',').filter(|i| !i.is_empty()).map(str::to_string).collect();
        let descs = descs.iter().map(|d| unjson(d)).collect::<Option<HashSet<String>>>()?;
        ids.iter().all(|i| plain(i)).then_some(Live { t, ids, descs })
    })();
    Log { live, ..Log::default() }
}

/// `<subs>/<sid>.log` and `<subs>/<sid>.live`, each cached on (mtime, len); a session id that
/// is not plain names no file. A missing or malformed `.live` is no set.
fn log_of(subs: &Path, sid: Option<&str>) -> Log {
    let Some(sid) = sid.filter(|s| plain(s)) else {
        return Log::default();
    };
    let mut log = cached(&subs.join(format!("{sid}.log")), LOG_MAX, fold_log);
    log.live = cached(&subs.join(format!("{sid}.live")), LIVE_MAX, fold_live).live;
    log
}

/// `p` folded by `fold` from its last `max` bytes, cached on its stamp and inode: the hook
/// renames each `.live` into place, so a set of the same length in the same mtime tick (a
/// coarse clock, WSL's) is a new inode all the same.
fn cached(p: &Path, max: u64, fold: fn(&[String]) -> Log) -> Log {
    use std::os::unix::fs::MetadataExt;
    let st = std::fs::metadata(p).ok().filter(|m| m.is_file());
    let Some(st) = st.map(|m| ((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()), m.ino())) else {
        LOGS.with(|c| c.borrow_mut().remove(p));
        return Log::default();
    };
    LOGS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((_, log)) = c.get(p).filter(|(s, _)| *s == st) {
            return log.clone();
        }
        if !c.contains_key(p) && c.len() >= 2 * DIRS_MAX {
            c.clear();
        }
        let log = open(p).map(|mut f| fold(&super::tail(&mut f, st.0.1, max))).unwrap_or_default();
        c.insert(p.to_path_buf(), (st, log.clone()));
        log
    })
}

fn stamp(p: &Path) -> Option<Stamp> {
    let md = std::fs::metadata(p).ok()?;
    md.is_file().then(|| (md.modified().unwrap_or(SystemTime::UNIX_EPOCH), md.len()))
}

fn open(p: &Path) -> Option<File> {
    let f = File::open(p).ok()?;
    READS.with(|r| r.set(r.get() + 1));
    Some(f)
}

/// ASCII letters, digits, `-` and `_`: a name that can only be itself in a path.
fn plain(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `agent-<id>.meta.json` with a plain id.
fn agent_id(name: &str) -> Option<&str> {
    let id = name.strip_prefix("agent-")?.strip_suffix(".meta.json")?;
    plain(id).then_some(id)
}

/// A string field, control characters dropped; empty when absent or not a string.
fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().chars().filter(|c| !c.is_control()).collect()
}

fn read_meta(p: &Path, len: u64) -> Option<Meta> {
    if len > META_MAX {
        return None;
    }
    let mut s = String::new();
    open(p)?.take(META_MAX).read_to_string(&mut s).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    v.as_object()?;
    let kind = if v["isFork"] == true {
        SubKind::Fork
    } else if v["teamName"].as_str().is_some_and(|t| !t.is_empty()) {
        SubKind::Teammate
    } else {
        SubKind::Subagent
    };
    Some(Meta {
        kind,
        name: text(&v["name"]),
        ty: text(&v["agentType"]),
        desc: text(&v["description"]),
        finished: v["finished"] == true,
        stopped: v["stoppedByUser"] == true,
    })
}

/// A row's `timestamp` as unix seconds.
fn row_secs(v: &Value) -> Option<u64> {
    let t = v["timestamp"].as_str().and_then(super::rfc3339)?;
    t.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

fn n(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// The start off the first row (kept from `old` once read whole: it never moves), and
/// the newest usage row and assistant row off the tail.
fn read_rows(p: &Path, len: u64, old: Option<&Rows>) -> Rows {
    let mut r = old.cloned().unwrap_or_default();
    let Some(mut f) = open(p) else { return r };
    if !r.head {
        let mut first = vec![];
        let _ = BufReader::new((&mut f).take(HEAD_MAX)).read_until(b'\n', &mut first);
        // Final only once whole: a row still being written is read again next time.
        r.head = first.ends_with(b"\n");
        r.started = serde_json::from_slice::<Value>(&first).ok().as_ref().and_then(row_secs);
    }
    let (mut usage, mut assistant, mut last) = (None, None, None);
    for l in super::tail(&mut f, len, TAIL_MAX).iter().rev() {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if last.is_none() {
            last = row_secs(&v);
        }
        if usage.is_none() && v["message"]["usage"].is_object() {
            let u = &v["message"]["usage"];
            usage = Some(
                [
                    "input_tokens",
                    "output_tokens",
                    "cache_creation_input_tokens",
                    "cache_read_input_tokens",
                ]
                .iter()
                .map(|k| n(&u[k]))
                .sum(),
            );
        }
        if assistant.is_none() && v["type"] == "assistant" {
            assistant = Some(v["isApiErrorMessage"] == true);
        }
        if usage.is_some() && assistant.is_some() && last.is_some() {
            break;
        }
    }
    // Not `last.or(r.last)`: a window with no timestamped row falls back to the mtime, and a
    // warm cache must say what a fresh read says.
    r.last = last;
    if let Some(t) = usage {
        r.tokens = t;
    }
    if let Some(e) = assistant {
        r.api_error = e;
    }
    r
}

/// The session's `subagents/` dir: `<sid>/` beside the transcript the board reads
/// (`transcript::locate`), else beside where the cwd's transcript would be. None for a
/// session id that could name another path.
pub fn dir_of(transcript: &str, sess: &str, cwd: &str) -> Option<PathBuf> {
    let t = super::transcript::locate(transcript, sess, cwd)
        .or_else(|| plain(sess).then(|| super::transcript::slug_path(sess, cwd)).flatten())?;
    Some(t.with_extension("").join("subagents"))
}

/// What the hook files say of one agent: its newest event and that event's epoch, after a
/// teammate's TaskStop, a `finished` meta newer than the start and the `.live` set are weighed.
/// None when they say nothing, and the meta decides. `meta_at` is the meta file's mtime,
/// `wrote` the agent's newest transcript row.
fn event_of(
    id: &str,
    m: &Meta,
    meta_at: Option<u64>,
    wrote: Option<u64>,
    log: &Log,
    names: &HashSet<String>,
    descs: &HashMap<String, usize>,
) -> Option<(Ev, u64)> {
    let rec = log.ids.get(id).copied();
    let mate = m.kind == SubKind::Teammate;
    // A teammate's TaskStop names it, or failing that (the model gave the task id) its
    // description when no other teammate has it; it counts after the teammate's newest
    // start, and not at all with no start in the tail.
    let killed = log.kills.iter().rfind(|k| {
        mate && rec.and_then(|r| r.start).is_some_and(|(_, s)| k.line > s)
            && if names.contains(&k.name) {
                k.name == m.name
            } else {
                !k.desc.is_empty() && k.desc == m.desc && descs.get(&m.desc) == Some(&1)
            }
    });
    if let Some(k) = killed {
        return Some((Ev::Kill, k.t));
    }
    // The hook log says what the meta file cannot. Without a record the meta decides, and
    // so does a `finished` written at or after the last `start`: a StopFailure sends no
    // SubagentStop, while a resume writes its `start` after the old `finished`.
    let r = rec.filter(|r| r.ev != Ev::Start || !(m.finished && meta_at.is_some_and(|w| w >= r.t)))?;
    // The `.live` set ends an agent the log has running (a start or a resume is its newest
    // event) that started in an earlier second, wrote nothing after it, and is not in it. It
    // cannot tell killed from finished; a finished meta decided above. Not an idle teammate:
    // no payload yet shows whether `background_tasks` lists one (#403).
    let gone = log.live.as_ref().is_some_and(|l| {
        r.start.is_some_and(|(t, _)| t < l.t)
            && wrote.is_none_or(|w| w <= l.t)
            && !l.ids.contains(id)
            && !(mate && l.descs.contains(&m.desc))
    });
    Some(if r.ev == Ev::Start && gone { (Ev::Kill, r.t) } else { (r.ev, r.t) })
}

/// The state from the meta, the transcript, the hook files' event, and whether the agent has
/// been quiet for `END_UNKNOWN_AFTER`. A user stop wins, then the event, then the meta.
fn state_of(m: &Meta, r: &Rows, ev: Option<Ev>, quiet: bool) -> SubState {
    match ev {
        _ if m.stopped => SubState::Stopped,
        Some(Ev::Kill) => SubState::Stopped,
        Some(Ev::Start) if quiet => SubState::EndUnknown,
        Some(Ev::Start) => SubState::Running,
        Some(Ev::Idle) => SubState::Idle,
        Some(Ev::Stop) if m.kind == SubKind::Teammate => SubState::Idle,
        Some(Ev::Stop) if r.api_error => SubState::Failed,
        Some(Ev::Stop) => SubState::Done,
        None if m.finished && r.api_error => SubState::Failed,
        None if m.finished => SubState::Done,
        // never `finished`, and no sign of life for a quarter hour: a teammate ended by
        // TaskStop or a shutdown, or one the log has no record of
        None if quiet => SubState::EndUnknown,
        None => SubState::Running,
    }
}

/// The subagents in `dir` at unix second `now`, oldest first (unstarted last), then by id,
/// with the hooks' log from the environment's state dir. A missing dir is an empty list and
/// opens no file.
pub fn read(dir: &Path, now: u64) -> Vec<Sub> {
    let subs = state_dir(
        std::env::var("XDG_STATE_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    );
    read_in(dir, now, &subs.unwrap_or_else(|| PathBuf::from("/nonexistent-cockpit-state")))
}

/// `read` with the hooks' log dir (`.../tmux-cockpit/subs`) given. The session id is `dir`'s
/// parent's name (`<sid>/subagents`).
pub fn read_in(dir: &Path, now: u64, subs: &Path) -> Vec<Sub> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        CACHE.with(|c| c.borrow_mut().remove(dir));
        return vec![];
    };
    let mut ids: Vec<String> = rd
        .flatten()
        .filter_map(|e| agent_id(&e.file_name().to_string_lossy()).map(str::to_string))
        .collect();
    ids.sort();
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if !c.contains_key(dir) && c.len() >= DIRS_MAX {
            c.clear();
        }
        let old = c.remove(dir).unwrap_or_default();
        let mut old = old;
        let log = log_of(subs, dir.parent().and_then(Path::file_name).and_then(|n| n.to_str()));
        let mut kept: HashMap<String, Cached> = HashMap::new();
        let mut out = vec![];
        // The metas first: a teammate's TaskStop maps by description only when its name is no
        // teammate's here, and that takes every name.
        let mut metas = vec![];
        for id in ids {
            let mut e = old.remove(&id).unwrap_or_default();
            let mp = dir.join(format!("agent-{id}.meta.json"));
            let Some(ms) = stamp(&mp) else { continue };
            if e.meta.as_ref().is_none_or(|(s, _)| *s != ms) {
                e.meta = read_meta(&mp, ms.1).map(|m| (ms, m));
            }
            metas.push((id, e));
        }
        let names: HashSet<String> = metas
            .iter()
            .filter_map(|(_, e)| e.meta.as_ref().map(|(_, m)| m))
            .filter(|m| m.kind == SubKind::Teammate && !m.name.is_empty())
            .map(|m| m.name.clone())
            .collect();
        // A description maps a TaskStop only when it is one teammate's: two alike stay unknown.
        let mut descs: HashMap<String, usize> = HashMap::new();
        for m in metas.iter().filter_map(|(_, e)| e.meta.as_ref().map(|(_, m)| m)) {
            if m.kind == SubKind::Teammate {
                *descs.entry(m.desc.clone()).or_default() += 1;
            }
        }
        for (id, mut e) in metas {
            let Some((ms, m)) = e.meta.clone() else {
                kept.insert(id, e);
                continue;
            };
            let rp = dir.join(format!("agent-{id}.jsonl"));
            match stamp(&rp) {
                // a shorter file was replaced, not appended to: its start is read again
                Some(rs) if e.rows.as_ref().is_none_or(|(s, _)| *s != rs) => {
                    let keep = e.rows.as_ref().filter(|(s, _)| s.1 <= rs.1).map(|(_, r)| r);
                    e.rows = Some((rs, read_rows(&rp, rs.1, keep)));
                }
                Some(_) => {}
                None => e.rows = None,
            }
            let r = e.rows.as_ref().map(|(_, r)| r.clone()).unwrap_or_default();
            let secs = |t: SystemTime| t.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs());
            // the newest row says when it last wrote; the mtime only when no tail row has a time
            let wrote = r.last.or_else(|| secs(e.rows.as_ref()?.0.0));
            let meta_at = secs(ms.0);
            let ev = event_of(&id, &m, meta_at, wrote, &log, &names, &descs);
            // the newest event heard, the one a finished meta overrode too
            let active_at = wrote.max(ev.map(|(_, t)| t).or(log.ids.get(&id).map(|r| r.t)));
            let quiet = active_at.or(meta_at).is_some_and(|a| now.saturating_sub(a) >= END_UNKNOWN_AFTER);
            let state = state_of(&m, &r, ev.map(|(ev, _)| ev), quiet);
            // A user stop is the write that set `stoppedByUser`, the meta file's mtime, which a
            // stop during a long tool call puts after the newest row.
            let finished_at = matches!(state, SubState::Done | SubState::Failed | SubState::Stopped)
                .then(|| if state == SubState::Stopped { wrote.max(meta_at) } else { wrote })
                .flatten();
            out.push(Sub {
                id: id.clone(),
                kind: m.kind,
                ty: m.ty,
                desc: m.desc,
                state,
                started: r.started,
                finished_at,
                active_at,
                tokens: r.tokens,
            });
            kept.insert(id, e);
        }
        c.insert(dir.to_path_buf(), kept);
        out.sort_by_key(|s| (s.started.is_none(), s.started, s.id.clone()));
        out
    })
}
