//! A pane's subagents (#313): what each one is, off `agent-<id>.meta.json`, and when it
//! started and its tokens, off `agent-<id>.jsonl`, both in the session's `subagents/` dir.
//! Claude Code 2.1.317 writes the meta file (`persistAgentMetadata`) when the agent starts
//! and sets `finished: true` when its task leaves `running`, whatever the outcome.
//! Every file is cached on (mtime, len), so a quiet session costs a stat per file.
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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
    /// also an API error is Stopped, not Failed
    Stopped,
}

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
    /// mtime. Not hashed, like `started`.
    pub finished_at: Option<u64>,
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

type Stamp = (SystemTime, u64);

#[derive(Clone, Default)]
struct Meta {
    kind: SubKind,
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

thread_local! {
    static CACHE: RefCell<HashMap<PathBuf, HashMap<String, Cached>>> = RefCell::default();
    static READS: Cell<u64> = const { Cell::new(0) };
}

/// Files opened for reading on this thread so far: the cache's audit.
pub fn reads() -> u64 {
    READS.with(Cell::get)
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
    r.last = last.or(r.last);
    if let Some(t) = usage {
        r.tokens = t;
    }
    if let Some(e) = assistant {
        r.api_error = e;
    }
    r
}

/// The session's `subagents/` dir: beside `@cockpit_transcript` (`<sid>.jsonl` -> `<sid>/`)
/// when that still resolves to a file in the projects tree, as the hook checked it, else
/// beside the transcript the board reads for `@cockpit_session` and the pane's cwd. None for
/// a session id that could name another path.
pub fn dir_of(transcript: &str, sess: &str, cwd: &str) -> Option<PathBuf> {
    if let Some(t) = super::transcript::under(&super::transcript::projects_dir(), transcript) {
        return Some(t.with_extension("").join("subagents"));
    }
    if !plain(sess) {
        return None;
    }
    let t = PathBuf::from(super::transcript_path(sess, cwd));
    Some(t.with_extension("").join("subagents"))
}

/// The subagents in `dir`, oldest first (unstarted last), then by id. A missing dir is an
/// empty list and opens no file.
pub fn read(dir: &Path) -> Vec<Sub> {
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
        let mut now: HashMap<String, Cached> = HashMap::new();
        let mut subs = vec![];
        for id in ids {
            let mut e = old.remove(&id).unwrap_or_default();
            let mp = dir.join(format!("agent-{id}.meta.json"));
            let Some(ms) = stamp(&mp) else { continue };
            if e.meta.as_ref().is_none_or(|(s, _)| *s != ms) {
                e.meta = read_meta(&mp, ms.1).map(|m| (ms, m));
            }
            let Some((_, m)) = e.meta.clone() else {
                now.insert(id, e);
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
            let state = if m.stopped {
                SubState::Stopped
            } else if m.finished && r.api_error {
                SubState::Failed
            } else if m.finished {
                SubState::Done
            } else {
                SubState::Running
            };
            // the newest row says when it stopped; the mtime only when no tail row has a time
            let finished_at = (state != SubState::Running)
                .then(|| {
                    r.last.or_else(|| {
                        let t = e.rows.as_ref()?.0.0;
                        t.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs())
                    })
                })
                .flatten();
            subs.push(Sub {
                id: id.clone(),
                kind: m.kind,
                ty: m.ty,
                desc: m.desc,
                state,
                started: r.started,
                finished_at,
                tokens: r.tokens,
            });
            now.insert(id, e);
        }
        c.insert(dir.to_path_buf(), now);
        subs.sort_by_key(|s| (s.started.is_none(), s.started, s.id.clone()));
        subs
    })
}
