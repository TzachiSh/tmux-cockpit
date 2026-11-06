//! `claude agents --json` off the fetch path (#492 fix 5), and its cache file.
//!
//! The call takes 170-210 ms, so the board's fetch never waits on it: [`resolve`]
//! only reads what an earlier call left, [`flush`] starts the next call on a
//! thread of its own, one at a time. A one-shot caller (`--json`, `--rows`) has
//! nothing to come back to, so without [`Background`] [`resolve`] asks and waits,
//! under the same deadline and kill. The answer lands in memory and in
//! `${XDG_CACHE_HOME:-~/.cache}/tmux-cockpit/agents.json` (docs/tui.md "The
//! agents cache"), which `cockpit-tui workdir` can read through [`cached`].
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
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
    /// epoch second of the answer
    pub at: u64,
    pub by_pid: HashMap<String, Entry>,
    pub none: HashSet<String>,
}

/// Pid of the `claude agents` child while it runs, else 0.
static CHILD: AtomicU32 = AtomicU32::new(0);

/// The lookup's child while one runs, once the board holds its pid, which is
/// what `kill_lookup` reads. The tests wait for it before they hang up.
pub fn lookup_pid() -> Option<u32> {
    Some(CHILD.load(Ordering::Relaxed)).filter(|&c| c != 0)
}

/// Where a lookup is, for a test that has to act inside one of its windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// forked, and its pid not yet in `CHILD`
    Spawned,
    /// exited, `CHILD` cleared, and not yet reaped
    Reaping,
}

/// A function run at each [`Stage`] with the child's pid.
pub type StageHook = fn(Stage, u32);

/// A test's hook. None in a board.
static STAGE_HOOK: Mutex<Option<StageHook>> = Mutex::new(None);

/// For a test: a function run at each [`Stage`] of every lookup, or none.
pub fn set_stage_hook(hook: Option<StageHook>) {
    *STAGE_HOOK.lock().unwrap_or_else(|e| e.into_inner()) = hook;
}

fn stage(at: Stage, pid: u32) {
    let hook = *STAGE_HOOK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(h) = hook {
        h(at, pid);
    }
}

/// Lookup threads started and not yet done, the cache write included.
static LOOKUPS: AtomicUsize = AtomicUsize::new(0);

/// How many lookup threads are still running, their cache write included: a
/// test waits for 0 before it removes the dir that write lands in (#608).
pub fn lookups() -> usize {
    LOOKUPS.load(Ordering::SeqCst)
}

/// One lookup thread counted: up when made, down when dropped, whichever way the
/// thread returns, and also when a spawn that panicked drops it unrun (#619).
struct Running;

impl Running {
    fn start() -> Running {
        LOOKUPS.fetch_add(1, Ordering::SeqCst);
        Running
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        LOOKUPS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The last child, and when it ended: a `ps` snapshot taken during the call can
/// be walked after it.
static RECENT: Mutex<Option<(u32, Instant)>> = Mutex::new(None);
const RECENT_FOR: Duration = Duration::from_secs(3);

/// The board runs the lookup on a thread; anything else waits for it.
static BACKGROUND: AtomicBool = AtomicBool::new(false);

/// For a test: the board's mode on or off without the guard.
pub fn set_background(on: bool) {
    BACKGROUND.store(on, Ordering::Relaxed);
}

/// Held by the board for its whole run: it turns the background lookup on, and
/// dropping it kills a lookup still running, as a quit mid-call must.
pub struct Background;

impl Background {
    pub fn start() -> Background {
        set_background(true);
        Background
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        kill_lookup();
    }
}

/// SIGKILL the lookup's process group, if one runs. Also the hangup path's.
pub fn kill_lookup() {
    let c = CHILD.load(Ordering::Relaxed);
    if c != 0 {
        // SAFETY: killpg takes a plain integer and touches no memory.
        unsafe { libc::killpg(c as libc::pid_t, libc::SIGKILL) };
    }
}

/// Whether `pid` is the lookup's own child. The board sits in a pane, so that
/// child is in the pane's process tree: a walk would find a `claude` there and
/// show the board's own pane as an agent for as long as the call runs.
pub fn is_lookup_child(pid: &str) -> bool {
    let Ok(pid) = pid.parse::<u32>() else { return false };
    if pid == CHILD.load(Ordering::Relaxed) && pid != 0 {
        return true;
    }
    let r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    r.is_some_and(|(c, t)| c == pid && t.elapsed() < RECENT_FOR)
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

/// A session id is used in a path, so it is dashes and alphanumerics, bounded.
fn ok_session(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The file's text when it is a regular file under 1 MiB. Not a FIFO or a
/// symlink, and opened non-blocking, so a planted node cannot hang the fetch.
fn read_capped(path: &Path) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;
    const CAP: u64 = 1 << 20;
    let m = std::fs::symlink_metadata(path).ok()?;
    if !m.is_file() || m.len() > CAP {
        return None;
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let mut text = String::new();
    Read::take(&mut f, CAP).read_to_string(&mut text).ok()?;
    Some(text)
}

/// The file at `path`, or `None` when it is absent, unparseable, `v` is not 1
/// or `at` is more than [`FILE_TTL_S`] before `now`. A key whose pid's start
/// time in `root` differs, or cannot be read, is dropped: a reused pid never
/// matches, and on macOS nothing does.
pub fn read_file(path: &Path, root: &Path, now: u64) -> Option<Snapshot> {
    let text = read_capped(path)?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let at = v["at"].as_u64()?;
    if v["v"].as_u64() != Some(1) || now.saturating_sub(at) > FILE_TTL_S {
        return None;
    }
    let live = |k: &str| {
        split_key(k)
            .filter(|(p, s)| start_of(root, p).as_deref() == Some(*s))
            .map(|(p, _)| p.to_string())
    };
    let mut snap = Snapshot { at, ..Snapshot::default() };
    for (k, e) in v["by_pid"].as_object()? {
        if let (Some(pid), Some(session)) = (live(k), e["session"].as_str().filter(|s| ok_session(s))) {
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
    /// when a call last answered, or failed; stale positives wait a NEG_TTL from it
    last_ask: Option<Instant>,
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
            last_ask: None,
        };
        // another board's answer is as good as ours
        if let Some(snap) = read_file(&s.path, root, epoch()) {
            let now = Instant::now();
            let seen = now.checked_sub(Duration::from_secs(epoch().saturating_sub(snap.at))).unwrap_or(now);
            // a one-shot asks for a pid the file lacks, whatever the file's age
            s.last_ask = BACKGROUND.load(Ordering::Relaxed).then_some(seen);
            for (pid, entry) in snap.by_pid {
                let start = start_of(root, &pid);
                s.pos.insert(pid, Pos { start, entry, seen });
            }
            for pid in snap.none {
                let start = start_of(root, &pid);
                s.neg.insert(pid, (start, seen));
            }
        }
        s
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

/// What one pane's lineage needs from the cache.
enum Look {
    Found(String),
    /// the agent pids no positive or fresh negative covers
    Unknown(Vec<String>),
}

impl State {
    /// A positive is served while its pid is the same process, however old; one
    /// older than [`NEG_TTL`] also puts its pid on `pending`, so a session id that
    /// changed under the pid is picked up within a minute, as master did.
    fn look(&mut self, root: &Path, lineage: &[(String, String)]) -> Look {
        let hit = lineage
            .iter()
            .find_map(|(p, _)| Some((p, self.pos.get(p).filter(|x| same_proc(&x.start, x.seen, root, p))?)));
        if let Some((p, x)) = hit {
            let stale = x.seen.elapsed() >= NEG_TTL && self.last_ask.is_none_or(|t| t.elapsed() >= NEG_TTL);
            let sess = x.entry.session.clone();
            if stale && BACKGROUND.load(Ordering::Relaxed) {
                self.pending.insert(p.clone());
            }
            return Look::Found(sess);
        }
        let agent = |(_, c): &&(String, String)| super::agentish(c);
        let mut ids: Vec<&String> = lineage.iter().filter(agent).map(|(p, _)| p).collect();
        if ids.is_empty() {
            ids.extend(lineage.first().map(|(p, _)| p));
        }
        Look::Unknown(ids.into_iter().filter(|p| !self.is_neg(root, p)).cloned().collect())
    }
}

/// The session the last answer gave for one of `lineage`'s pids, or empty. On the
/// board it never waits: a pid the cache does not know goes on the list [`flush`]
/// asks about. Elsewhere it asks now, once a minute at most, and waits.
/// `lineage` is `(pid, comm)`; the agent pids are the ones whose comm is an
/// agent's, else the first (the pane's own).
pub fn resolve(lineage: &[(String, String)]) -> String {
    let root = proc_root();
    let ask_now = with_state(&root, |s| match s.look(&root, lineage) {
        Look::Found(sess) => Err(sess),
        Look::Unknown(ids) if ids.is_empty() => Err(String::new()),
        Look::Unknown(ids) if BACKGROUND.load(Ordering::Relaxed) => {
            s.pending.extend(ids);
            Err(String::new())
        }
        Look::Unknown(ids) if s.last_ask.is_some_and(|t| t.elapsed() < NEG_TTL) => {
            // this run asked already; the answer did not name them
            let now = Instant::now();
            for p in ids {
                s.neg.insert(p.clone(), (start_of(&root, &p), now));
            }
            Err(String::new())
        }
        Look::Unknown(ids) => Ok((s.gen_id, s.path.clone(), ids.into_iter().collect::<HashSet<_>>())),
    });
    match ask_now {
        Err(sess) => sess,
        Ok((gen_id, path, asked)) => {
            ask(gen_id, &path, asked);
            with_state(&root, |s| match s.look(&root, lineage) {
                Look::Found(sess) => sess,
                Look::Unknown(_) => String::new(),
            })
        }
    }
}

/// End of a board fetch: start the call for what [`resolve`] left unknown or
/// stale, unless one is in flight.
pub fn flush() {
    let root = proc_root();
    let spawn = with_state(&root, |s| {
        let ids: HashSet<String> = s.pending.iter().filter(|p| !s.is_neg(&root, p)).cloned().collect();
        if ids.is_empty() || s.running {
            return None;
        }
        s.running = true;
        s.pending.clear();
        Some((s.gen_id, s.path.clone(), ids))
    });
    if let Some((gen_id, path, asked)) = spawn {
        // counted before the spawn, so no wait can see 0 while it starts; the
        // guard rides in the closure, so a spawn that fails drops it uncounted
        let running = Running::start();
        std::thread::spawn(move || {
            let _running = running;
            ask(gen_id, &path, asked)
        });
    }
}

/// `(pid, entry)` rows of the `--json` array; a row with no pid or session is skipped.
fn parse(out: &[u8]) -> Option<Vec<(String, Entry)>> {
    let v: Value = serde_json::from_slice(out).unwrap_or(Value::Null);
    let s = |r: &Value, f: &str| r[f].as_str().unwrap_or_default().to_string();
    v.as_array().map(|a| {
        a.iter()
            .filter_map(|r| {
                let e = Entry {
                    session: r["sessionId"].as_str().filter(|s| ok_session(s))?.into(),
                    cwd: s(r, "cwd"),
                    kind: s(r, "kind"),
                };
                Some((r["pid"].as_i64()?.to_string(), e))
            })
            .collect()
    })
}

fn ask(gen_id: u64, path: &Path, asked: HashSet<String>) {
    let t0 = Instant::now();
    let out = claude_agents_json(AGENTS_DEADLINE);
    let root = proc_root();
    let answered = out.as_deref().and_then(parse).is_some();
    let doc = {
        let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = g.as_mut().filter(|s| s.gen_id == gen_id) else {
            return;
        };
        s.running = false;
        s.last_ask = Some(Instant::now());
        super::log(&format!(
            "agents lookup ok={} asked={} ms={}",
            out.is_some(),
            asked.len(),
            t0.elapsed().as_millis()
        ));
        // Pids added while the call ran may predate its answer; the next fetch
        // adds a pane's pid again if the answer still leaves it unknown.
        s.pending.clear();
        let now = Instant::now();
        let rows = out.as_deref().and_then(parse);
        // a spawn failure, a deadline or no JSON array leaves what is known; the asked pids rest for NEG_TTL
        if let Some(rows) = rows {
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
    // `at` is the time of an answer: a failed call writes nothing
    if answered {
        let _ = write_file(path, &doc);
    }
}

/// `claude agents --json`'s stdout, or `None` on a spawn failure or when it
/// has not closed by the deadline.
fn claude_agents_json(deadline: Duration) -> Option<Vec<u8>> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let mut cmd = Command::new("claude");
    cmd.args(["agents", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // its own group, so the kill takes anything it forked that holds stdout
        .process_group(0);
    // From the fork to the store in CHILD a hang-up finds no child to kill (#607).
    // On Linux a board that dies there takes the child along. The check after the
    // prctl is for a board already gone by the time the child runs it. No test
    // can reach that check on purpose. PR_SET_PDEATHSIG is Linux-only: on macOS
    // the window stays open, and a child forked just before the board dies keeps
    // running until it answers or exits (#619).
    #[cfg(target_os = "linux")]
    {
        let me = std::process::id();
        // SAFETY: prctl, getppid and _exit are async-signal-safe.
        unsafe {
            cmd.pre_exec(move || {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                if libc::getppid() as u32 != me {
                    libc::_exit(0);
                }
                Ok(())
            });
        }
    }
    let mut child = cmd.spawn().ok()?;
    stage(Stage::Spawned, child.id());
    CHILD.store(child.id(), Ordering::Relaxed);
    lookup_output(&mut child, deadline)
}

/// Whether the child has exited, seen without reaping it: its pid stays ours.
fn exited(child: &std::process::Child) -> bool {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let flags = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
    // SAFETY: waitid writes only into `info`.
    let r = unsafe { libc::waitid(libc::P_PID, child.id() as libc::id_t, &mut info, flags) };
    // Under WNOHANG a child with nothing to report leaves si_signo 0, and SIGCHLD
    // once it has exited. POSIX says to zero it; Linux does, and xnu's waitid
    // leaves `info` untouched instead, which is why it is zeroed above. Darwin's
    // copyout sets si_signo to SIGCHLD whenever it reports an exit (#619).
    r != 0 || info.si_signo != 0
}

/// The end of a lookup: wait for the exit without reaping, clear `CHILD` while
/// the pid is still ours, and only then reap. A `kill_lookup` that read `CHILD`
/// just before mostly hits an unreaped child. Practically never a reused pid,
/// not never: its load and its killpg are two steps on the main thread, and a
/// preemption between them that outlasts this reap and a pid reuse would still
/// hit a stranger (#607, #619). push.rs's waiter has the same bound.
fn finish(child: &mut std::process::Child, out: Option<Vec<u8>>) -> Option<Vec<u8>> {
    let (pid, flags) = (child.id() as libc::id_t, libc::WEXITED | libc::WNOWAIT);
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: waitid writes only into `info`.
    while unsafe { libc::waitid(libc::P_PID, pid, &mut info, flags) } != 0 {
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            break;
        }
    }
    *RECENT.lock().unwrap_or_else(|e| e.into_inner()) = Some((child.id(), Instant::now()));
    CHILD.store(0, Ordering::Relaxed);
    stage(Stage::Reaping, child.id());
    let _ = child.wait();
    out
}

fn lookup_output(child: &mut std::process::Child, deadline: Duration) -> Option<Vec<u8>> {
    let Some(mut stdout) = child.stdout.take() else {
        // SAFETY: killpg takes a plain integer and touches no memory; not reaped yet.
        unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
        return finish(child, None);
    };
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
        if exited(child) {
            return finish(child, out);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Not reaped yet, so the pid and its group are still this child's.
    // SAFETY: killpg takes a plain integer and touches no memory.
    unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
    finish(child, out)
}
