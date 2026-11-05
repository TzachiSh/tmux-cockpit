//! `claude agents --json` off the fetch path (#492 fix 5), and its cache file.
//!
//! The call takes 170-210 ms, so the fetch never waits on it: [`resolve`] only
//! reads what an earlier call left, [`flush`] starts the next call on a thread
//! of its own, one at a time. The answer lands in memory and in
//! `${XDG_CACHE_HOME:-~/.cache}/tmux-cockpit/agents.json` (docs/tui.md "The
//! agents cache"), which `cockpit-tui workdir` can read through [`cached`].
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A reader ignores a file older than this many seconds.
pub const FILE_TTL_S: u64 = 90;
/// A negative expires after this, and the file is rewritten this often.
const NEG_TTL: Duration = Duration::from_secs(60);
/// #294: under the board's 3 s stale threshold; then the group is killed.
const AGENTS_DEADLINE: Duration = Duration::from_millis(2500);

/// What `claude agents --json` knows of one session. `cwd` is the session's
/// LAUNCH dir, not where it works now (#448).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub session: String,
    pub cwd: String,
    pub kind: String,
}

/// A file that passed every guard. Keys are pids; the start time already matched.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub by_pid: HashMap<String, Entry>,
    pub none: HashSet<String>,
}

/// Pid of the `claude agents` child while it runs, else 0.
static CHILD: AtomicU32 = AtomicU32::new(0);

/// Whether `pid` is the lookup's own child. The board sits in a pane, so that
/// child is in the pane's process tree: a walk would find a `claude` there and
/// show the board's own pane as an agent for as long as the call runs.
pub fn is_lookup_child(pid: &str) -> bool {
    let c = CHILD.load(Ordering::Relaxed);
    c != 0 && pid.parse::<u32>() == Ok(c)
}

pub fn proc_root() -> PathBuf {
    std::env::var_os("COCKPIT_PROC_DIR").map_or_else(|| "/proc".into(), PathBuf::from)
}

pub fn cache_path() -> PathBuf {
    crate::panel::xdg("XDG_CACHE_HOME", ".cache").join("tmux-cockpit/agents.json")
}

fn epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Field 22 of `<root>/<pid>/stat`, in clock ticks since boot. The comm in
/// field 2 may hold spaces and parens, so the fields count from the last `)`.
/// `None` without a proc root (macOS) or for a pid that is gone.
pub fn start_of(root: &Path, pid: &str) -> Option<String> {
    if !all_digits(pid) {
        return None;
    }
    let s = std::fs::read_to_string(root.join(pid).join("stat")).ok()?;
    let rest = s.rsplit_once(')')?.1;
    // field 3 is the first after the comm
    rest.split_whitespace().nth(19).filter(|t| all_digits(t)).map(String::from)
}

/// `pid:start` to its two halves, both non-empty digits.
fn split_key(k: &str) -> Option<(&str, &str)> {
    let (p, s) = k.split_once(':')?;
    (all_digits(p) && all_digits(s)).then_some((p, s))
}

/// The file at `path`, or `None` when it is absent, unparseable, `v` is not 1
/// or `at` is more than [`FILE_TTL_S`] before `now`. A key whose pid's start
/// time in `root` differs, or cannot be read, is dropped: a reused pid never
/// matches, and on macOS nothing does.
pub fn read_file(path: &Path, root: &Path, now: u64) -> Option<Snapshot> {
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(1 << 20).read_to_string(&mut text).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    if v["v"].as_u64() != Some(1) || now.saturating_sub(v["at"].as_u64()?) > FILE_TTL_S {
        return None;
    }
    let live = |k: &str| {
        split_key(k)
            .filter(|(p, s)| start_of(root, p).as_deref() == Some(*s))
            .map(|(p, _)| p.to_string())
    };
    let mut snap = Snapshot::default();
    for (k, e) in v["by_pid"].as_object()? {
        if let (Some(pid), Some(session)) = (live(k), e["session"].as_str()) {
            let s = |f: &str| e[f].as_str().unwrap_or_default().to_string();
            snap.by_pid.insert(
                pid,
                Entry {
                    session: session.into(),
                    cwd: s("cwd"),
                    kind: s("kind"),
                },
            );
        }
    }
    for k in v["none"].as_array()? {
        snap.none.extend(k.as_str().and_then(live));
    }
    Some(snap)
}

/// The cache entry for `pid`, for a reader that wants the session without
/// asking `claude` (#504). Read only; `None` is "not known", not "no session".
pub fn cached(pid: &str) -> Option<Entry> {
    read_file(&cache_path(), &proc_root(), epoch())?.by_pid.remove(pid)
}

/// Temp file in the same directory, mode 600, then a rename over `path`.
fn write_file(path: &Path, doc: &Value) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".agents.json.{}.{n}", std::process::id()));
    let res = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(doc.to_string().as_bytes()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

struct Pos {
    start: Option<String>,
    entry: Entry,
    seen: Instant,
}

struct State {
    path: PathBuf,
    /// which state a thread belongs to; a thread of an older one drops its answer
    gen_id: u64,
    running: bool,
    /// agent pids the next call should ask about
    pending: HashSet<String>,
    pos: HashMap<String, Pos>,
    neg: HashMap<String, (Option<String>, Instant)>,
    wrote: Option<Instant>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static GEN: AtomicU64 = AtomicU64::new(0);

/// An entry is good while the pid's start time is the one stored. Without a
/// start time on either side (no proc root) it is good for [`NEG_TTL`] only.
fn same_proc(stored: &Option<String>, seen: Instant, root: &Path, pid: &str) -> bool {
    match (stored, start_of(root, pid)) {
        (Some(a), Some(b)) => *a == b,
        (Some(_), None) => false,
        (None, _) => seen.elapsed() < NEG_TTL,
    }
}

impl State {
    fn new(path: PathBuf, root: &Path) -> State {
        let mut s = State {
            gen_id: GEN.fetch_add(1, Ordering::Relaxed),
            path,
            running: false,
            pending: HashSet::new(),
            pos: HashMap::new(),
            neg: HashMap::new(),
            wrote: None,
        };
        // another board's answer is as good as ours
        if let Some(snap) = read_file(&s.path, root, epoch()) {
            let now = Instant::now();
            for (pid, entry) in snap.by_pid {
                let start = start_of(root, &pid);
                s.pos.insert(pid, Pos { start, entry, seen: now });
            }
            for pid in snap.none {
                let start = start_of(root, &pid);
                s.neg.insert(pid, (start, now));
            }
        }
        s
    }

    fn pos_of(&self, root: &Path, pid: &str) -> Option<&Entry> {
        self.pos
            .get(pid)
            .filter(|p| same_proc(&p.start, p.seen, root, pid))
            .map(|p| &p.entry)
    }

    fn is_neg(&self, root: &Path, pid: &str) -> bool {
        self.neg
            .get(pid)
            .is_some_and(|(s, t)| t.elapsed() < NEG_TTL && same_proc(s, *t, root, pid))
    }

    /// Drop what no longer holds, then the document to write. Entries with no
    /// start time cannot be keyed, so they stay in memory only.
    fn doc(&mut self, root: &Path) -> Value {
        self.pos.retain(|k, p| same_proc(&p.start, p.seen, root, k));
        self.neg.retain(|k, (s, t)| t.elapsed() < NEG_TTL && same_proc(s, *t, root, k));
        let by_pid: serde_json::Map<String, Value> = self
            .pos
            .iter()
            .filter_map(|(k, p)| {
                let e = &p.entry;
                Some((
                    format!("{k}:{}", p.start.as_ref()?),
                    json!({"session": e.session, "cwd": e.cwd, "kind": e.kind}),
                ))
            })
            .collect();
        let none: Vec<String> = self
            .neg
            .iter()
            .filter_map(|(k, (s, _))| Some(format!("{k}:{}", s.as_ref()?)))
            .collect();
        self.wrote = Some(Instant::now());
        json!({"v": 1, "at": epoch(), "by_pid": by_pid, "none": none})
    }
}

fn with_state<R>(root: &Path, f: impl FnOnce(&mut State) -> R) -> R {
    let path = cache_path();
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_none_or(|s| s.path != path) {
        *g = Some(State::new(path, root));
    }
    f(g.as_mut().expect("set above"))
}

/// The session the last answer gave for one of `lineage`'s pids, or empty. Never
/// waits: a pid the cache does not know goes on the list [`flush`] asks about.
/// `lineage` is `(pid, comm)`; the agent pids are the ones whose comm is an
/// agent's, else the first (the pane's own).
pub fn resolve(lineage: &[(String, String)]) -> String {
    let root = proc_root();
    with_state(&root, |s| {
        if let Some(e) = lineage.iter().find_map(|(p, _)| s.pos_of(&root, p)) {
            return e.session.clone();
        }
        let agent = |(_, c): &&(String, String)| super::agentish(c);
        let mut ids: Vec<&String> = lineage.iter().filter(agent).map(|(p, _)| p).collect();
        if ids.is_empty() {
            ids.extend(lineage.first().map(|(p, _)| p));
        }
        for p in ids {
            if !s.is_neg(&root, p) {
                s.pending.insert(p.clone());
            }
        }
        String::new()
    })
}

/// End of a fetch: start the call for what [`resolve`] left unknown, unless one
/// is in flight. With nothing unknown, an answer a minute old is stamped again
/// (its entries still hold) so a reader sees a file under [`FILE_TTL_S`].
pub fn flush() {
    let root = proc_root();
    let (spawn, write) = with_state(&root, |s| {
        if !s.pending.is_empty() && !s.running {
            s.running = true;
            return (Some((s.gen_id, s.path.clone(), std::mem::take(&mut s.pending))), None);
        }
        let due = s.wrote.is_none_or(|t| t.elapsed() >= NEG_TTL);
        if s.pending.is_empty() && !s.pos.is_empty() && due {
            return (None, Some((s.path.clone(), s.doc(&root))));
        }
        (None, None)
    });
    if let Some((path, doc)) = write {
        let _ = write_file(&path, &doc);
    }
    if let Some((gen_id, path, asked)) = spawn {
        std::thread::spawn(move || ask(gen_id, &path, asked));
    }
}

/// `(pid, entry)` rows of the `--json` array; a row with no pid or session is skipped.
fn parse(out: &[u8]) -> Vec<(String, Entry)> {
    let v: Value = serde_json::from_slice(out).unwrap_or(Value::Null);
    let s = |r: &Value, f: &str| r[f].as_str().unwrap_or_default().to_string();
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let e = Entry {
                        session: r["sessionId"].as_str()?.into(),
                        cwd: s(r, "cwd"),
                        kind: s(r, "kind"),
                    };
                    Some((r["pid"].as_i64()?.to_string(), e))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn ask(gen_id: u64, path: &Path, asked: HashSet<String>) {
    let out = claude_agents_json(AGENTS_DEADLINE);
    let root = proc_root();
    let doc = {
        let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = g.as_mut().filter(|s| s.gen_id == gen_id) else {
            return;
        };
        s.running = false;
        // Pids added while the call ran may predate its answer; the next fetch
        // adds a pane's pid again if the answer still leaves it unknown.
        s.pending.clear();
        let now = Instant::now();
        let rows = parse(out.as_deref().unwrap_or_default());
        // a spawn failure or a deadline leaves what is known; the asked pids rest for NEG_TTL
        if out.is_some() {
            s.pos = rows
                .iter()
                .map(|(p, e)| {
                    (
                        p.clone(),
                        Pos {
                            start: start_of(&root, p),
                            entry: e.clone(),
                            seen: now,
                        },
                    )
                })
                .collect();
        }
        for p in asked.into_iter().filter(|p| !s.pos.contains_key(p)) {
            let start = start_of(&root, &p);
            s.neg.insert(p, (start, now));
        }
        s.doc(&root)
    };
    let _ = write_file(path, &doc);
}

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
    CHILD.store(child.id(), Ordering::Relaxed);
    let out = lookup_output(&mut child, deadline);
    CHILD.store(0, Ordering::Relaxed);
    out
}

fn lookup_output(child: &mut std::process::Child, deadline: Duration) -> Option<Vec<u8>> {
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
