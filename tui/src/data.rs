//! tmux and transcript reads.
//!
//! # The frozen data contract (issue #61, workflow.md §3)
//!
//! Nothing below changes without a comment on #61; other lanes change bodies only.
//!
//! **`Pane`** — `id, wher, name, cmd, state, title, msg, sess, path, root, spool,
//! pinned, delivered, pid, review, hold, doing, said, asked, loc, pm,
//! since: Option<u64>, asked_since: Option<u64>, idle_since: Option<u64>`.
//! Derives `Default, Clone, Hash, PartialEq`; the durations are `u64` and not
//! `Duration` so the struct can `Hash` (the redraw gate hashes rows).
//!
//! **`--json`** — one object per agent pane, never a non-agent pane, with
//! exactly these 14 keys **in this order**:
//! `id, where, state, title, doing, said, asked, msg, since, loc, diff, review,
//! session, path`. `where` is `sess:idx`, the window name is the separate
//! `name` field; `title` falls back to the pane name then `(no title)`;
//! `since` is seconds or `null`; `diff` is the `±` string for every state;
//! `state` ∈ `needs-input | running | done | error | ""`.
//!
//! **`--rows`** — tab-separated, 8 fields, no header:
//! `glyph \t id \t where \t title \t (msg if needs-input else doing) \t since \t loc \t diff`.
//!
//! **Options written:** `@cockpit_tui_pid` (published at start, unset on q,
//! SIGHUP, SIGTERM and the panic path), `@cockpit_tui_version` (written by the
//! fetch, read by the binding), `@cockpit_board_window`, `@cockpit_board_key`.
//! **Read only, owned by bash:** `@cockpit_session, @cockpit_dir, @panestate,
//! @panestate_msg, @cockpit_review, @cockpit_review_spool, @cockpit_title,
//! @cockpit_hold, @thm_*`.
//!
//! **Env:** `COCKPIT_ACTION` (file the action line is written to; unset means
//! stdout), `COCKPIT_TMUX` (override the tmux argv; unset follows `$TMUX`),
//! `COCKPIT_LOG` (append target), `COCKPIT_READONLY` (any non-empty value:
//! never write to a tmux server, the pid included), `COCKPIT_GLYPHS=ascii`.
//!
//! **Action lines** — exactly one `\n`-terminated line, nothing else ever
//! written to `$COCKPIT_ACTION`: `jump %N` · `review %N` · `fix %N` ·
//! `info %N` · `deliver %N` (reserved for L7). The verb is `[a-z]+`; the binary
//! executes nothing.
//!
//! **Exit codes:** `0` normal, `1` runtime failure (tmux unreachable, terminal
//! not a tty), `2` usage error. `[]` with exit 0 is "no agents"; exit 1 with no
//! stdout is "no tmux" -- a wrapper must be able to tell them apart.
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::Command;
use std::time::{Duration, SystemTime};

pub fn tmux_argv() -> Vec<String> {
    std::env::var("COCKPIT_TMUX")
        .unwrap_or_else(|_| "tmux".into())
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// `None` when tmux cannot be spawned or exits non-zero, so "no agents" and
/// "no tmux" stay distinguishable all the way out to the exit code.
pub fn tmux(args: &[&str]) -> Option<String> {
    let c = tmux_argv();
    let out = Command::new(&c[0]).args(&c[1..]).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn readonly() -> bool {
    std::env::var_os("COCKPIT_READONLY").is_some_and(|v| !v.is_empty())
}

/// The only write path to a tmux server, so `COCKPIT_READONLY` needs one guard
/// and not one per call site. A screenshot run without it already left a dead
/// `@cockpit_tui_pid` on the owner's live server.
pub fn tmux_set(args: &[&str]) {
    if readonly() {
        return;
    }
    let _ = tmux(args); // best effort: a failed unpublish is not actionable
}

pub fn log(s: &str) {
    if let Ok(p) = std::env::var("COCKPIT_LOG")
        && let Ok(mut f) = OpenOptions::new().append(true).create(true).open(p)
    {
        let us = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros();
        let _ = writeln!(f, "{us} {s}");
    }
}

#[derive(Default, Clone, PartialEq)]
pub struct Pane {
    pub id: String,
    pub wher: String,
    pub name: String,
    pub cmd: String,
    pub state: String,
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
    pub hold: String,
    pub doing: String,
    pub said: String,
    pub asked: String,
    pub loc: String,
    pub pm: String,
    /// seconds since the newest assistant row; not Duration, so the type can Hash
    pub since: Option<u64>,
    pub asked_since: Option<u64>,
    /// age of the newest transcript event of any kind: "how long since this
    /// session did anything", the number the state row wants
    pub idle_since: Option<u64>,
}

/// Everything except the three ages, which `age()` recomputes against `now`
/// every tick and would leave the redraw gate dirty on every tick. Destructured
/// on purpose: a new field is a compile error until it is placed on one side.
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
            hold,
            doing,
            said,
            asked,
            loc,
            pm,
            since: _,
            asked_since: _,
            idle_since: _,
        } = self;
        [
            id, wher, name, cmd, state, title, msg, sess, path, root, spool, pid, review, hold, doing, said, asked, loc, pm,
        ]
        .hash(h);
        (pinned, delivered).hash(h);
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

/// `~/.claude/projects/<slug>/<uuid>.jsonl`. The slug maps every byte outside
/// `[A-Za-z0-9-]` to `-` (reproduces 23/23 directories on this box). The
/// sibling `<uuid>/subagents/` is never opened: every row there is a sidechain.
fn transcript_path(sess: &str, path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    // Per BYTE, not per char: a two-byte glyph in the cwd becomes two dashes,
    // and a char-wise slug is one short of the directory that actually exists.
    let slug: String = path
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' { b as char } else { '-' })
        .collect();
    format!("{home}/.claude/projects/{slug}/{sess}.jsonl")
}

/// The last `cap` bytes as whole rows, decoded lossily -- the window can open
/// mid-codepoint, and `read_to_string` would drop the entire tail on that. The
/// leading partial row is dropped unless the window began just after a newline,
/// so every line returned is a complete record.
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
/// a Bash tool_use whose command held a TAB split `--rows` into 10 fields.
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
    /// the newest assistant row, which is what "how long has it been thinking"
    /// means; the newest row of any type is `newest`
    at: Option<SystemTime>,
    user_at: Option<SystemTime>,
    newest: Option<SystemTime>,
    /// needs-input / running / done, or empty when the tail said nothing
    state: String,
}

thread_local! {
    /// Keyed on (len, mtime), so an idle session costs one stat a tick instead
    /// of a parse. The path is resolved fresh every tick because `/compact` and
    /// `--resume` rotate the file out from under the key.
    static TAILS: std::cell::RefCell<HashMap<String, (u64, SystemTime, Facts)>> =
        std::cell::RefCell::new(HashMap::new());
}

/// A single row can be 750 KB, and at 64 KB a busy session buried its own open
/// question behind its padding.
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
    // ponytail: escalating on "no rows at all" misses a session that writes
    // >256 KB after an open ask -- it finds *a* row, so it never grows. Rule 69
    // accepts that; upgrade to "no open ask and no end_turn" if it ever bites.
    if fa.tool.is_empty() && fa.text.is_empty() && len > TAIL_FIRST {
        fa = parse(tail(&mut f, len, TAIL_MAX));
    }
    TAILS.with(|c| c.borrow_mut().insert(path.to_string(), (len, mtime, fa.clone())));
    fa
}

/// Rule 22's documented set, in preference order: the concrete argument first,
/// the prose last, so a Bash row reads as its command and a Task as its brief.
const DOING: [&str; 8] = [
    "command",
    "file_path",
    "path",
    "pattern",
    "description",
    "prompt",
    "query",
    "subject",
];

/// Ends the turn without closing anything, so it is neither a prompt nor an
/// answer. It arrives as a `text` block in the array form: 32 of 32 rows over
/// all 105 transcripts here, none as a string.
const INTERRUPT: &str = "[Request interrupted by user";

fn parse(lines: Vec<String>) -> Facts {
    let mut fa = Facts::default();
    // Rule 66: an AskUserQuestion is open until a later user row carries a
    // tool_result with its id -- an answer, a dismissal and a denial all write
    // one, an interrupt does not.
    let mut asks: Vec<String> = vec![];
    let (mut turn_over, mut saw) = (false, false);
    for raw in lines {
        let Ok(v) = serde_json::from_str::<Value>(&raw) else { continue };
        // A belt against a future version inlining subagent rows: today all of
        // them live under <uuid>/subagents/, which transcript_path never names.
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
                    // harness wrote these, nobody typed them (330 of 1831
                    // string rows here), so not a new turn and not `asked`.
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
                            // safe beside the tool_results: not one of the
                            // 7208 here carries a `text` key.
                            interrupted |= b["text"].as_str().is_some_and(|t| t.starts_with(INTERRUPT));
                        }
                        turn_over = interrupted;
                    }
                }
            }
            Some("assistant") => {
                saw = true;
                fa.at = ts.or(fa.at);
                // Rule 67: in a main transcript end_turn is reliable and rides
                // the text row; attachment and custom-title rows land after it
                // and must not reopen the turn, so only these two arms move it.
                turn_over = v["message"]["stop_reason"].as_str() == Some("end_turn");
                let Some(blocks) = v["message"]["content"].as_array() else {
                    continue;
                };
                for b in blocks {
                    match b["type"].as_str() {
                        Some("tool_use") => {
                            let i = &b["input"];
                            let d = DOING.iter().find_map(|k| i[k].as_str().filter(|s| !s.is_empty())).unwrap_or("");
                            let name = b["name"].as_str().unwrap_or("");
                            fa.tool = format!("{name}: {}", first_line(d));
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
        _ if !asks.is_empty() => "needs-input",
        // rule 78: an unknown state is unknown, never idle
        _ if !saw => "",
        _ if turn_over => "done",
        _ => "running",
    }
    .into();
    fa
}

/// doing / said / asked and their ages, from the tail of the session transcript.
fn read_transcript(p: &mut Pane) {
    if p.sess.is_empty() {
        return;
    }
    let fa = read_facts(&transcript_path(&p.sess, &p.path));
    // Rule 78: never re-derive what the hooks already wrote. @panestate wins;
    // the transcript only speaks for panes cockpit's hooks never touched.
    if p.state.is_empty() {
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
    /// `claude agents --json` costs ~380 ms, an order of magnitude more than
    /// the rest of a tick. It is only the fallback for panes whose
    /// @cockpit_session the hooks never set, and it changes only when a session
    /// starts or ends.
    /// ponytail: 60 s TTL, so a brand-new pane on the fallback path waits up to
    /// a minute for its transcript rows. Watch the sessions directory instead if
    /// that ever matters.
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

/// `-axww`, not `-eo`: macOS truncates to the terminal width even into a pipe,
/// and a truncated `comm` misses the agent. `comm` is the last column and may
/// hold spaces, so only the two leading numeric fields are tokenised.
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

/// Depth-first (`Vec::pop`): only membership matters, not order. Cap and seen
/// set are both load-bearing -- `ps` really does report a process as its own
/// parent, and the previous uncapped walk queued that row forever.
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
/// agent names too. The list stays @cockpit_agents_glob, one source, so the
/// user's edit reaches the tree walk as well as the pane name.
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

/// repo⑂branch and the working-tree diff, one pair of git calls per unique path.
fn git_facts(path: &str) -> (String, String, String) {
    let top = Command::new("git")
        .args([
            "-C",
            path,
            "--no-optional-locks",
            "rev-parse",
            "--show-toplevel",
            "--abbrev-ref",
            "HEAD",
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut it = top.lines();
    let (Some(root), Some(branch)) = (it.next(), it.next()) else {
        return (String::new(), String::new(), String::new());
    };
    let repo = root.rsplit('/').next().unwrap_or(root);
    let numstat = Command::new("git")
        .args(["-C", path, "--no-optional-locks", "diff", "--numstat", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let (mut add, mut del) = (0u64, 0u64);
    for l in numstat.lines() {
        let mut f = l.split('\t');
        add += f.next().and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
        del += f.next().and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
    }
    let pm = if add == 0 && del == 0 {
        "clean".to_string()
    } else {
        format!("+{add} -{del}")
    };
    (format!("{repo}⑂{branch}"), pm, root.to_string())
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
    /// `cockpit.tmux` already precomputes `|claude|codex|…|` into
    /// @cockpit_agents_glob from the public @cockpit_agents, so reading it keeps
    /// one list instead of a second copy here that drifts when the user edits
    /// theirs. Empty (plugin not loaded) leaves @panestate as the only test,
    /// which is the hooks' own answer anyway.
    static AGENT_GLOB: String = tmux(&["show-option", "-gqv", "@cockpit_agents_glob"]).unwrap_or_default().trim().to_string();
}

fn is_agent_cmd(cmd: &str) -> bool {
    AGENT_GLOB.with(|g| g.contains(&format!("|{cmd}|")))
}

const SEP: char = '\u{1f}';

/// One `-F` argument, one `list-panes -a` a tick. The separator is `\037`, not
/// a tab: a tab in a cwd shifts every later field, and `mkdir $'a\tb'`
/// reproduces it against today's rows. tmux protects neither -- format output
/// is never octal-escaped -- so the free-form fields strip NL, TAB and US
/// in-format, which also closes the newline-splits-a-record hole. `#{q:}` is
/// worse: it escapes spaces and passes every control character through raw.
/// The `-` sentinels keep the field list diffable against `scripts/tmux-review:1708`.
const FIELDS: &str = concat!(
    "#{pane_id}\u{1f}#{session_name}:#{window_index}\u{1f}#{window_name}\u{1f}#{pane_current_command}",
    "\u{1f}#{?#{@panestate},#{@panestate},-}",
    "\u{1f}#{?#{@cockpit_title},#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{@cockpit_title}}}},-}",
    "\u{1f}#{?#{@panestate_msg},#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{@panestate_msg}}}},-}",
    "\u{1f}#{?#{@cockpit_session},#{@cockpit_session},-}",
    "\u{1f}#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{pane_current_path}}}}",
    "\u{1f}#{pane_pid}",
    "\u{1f}#{?#{@cockpit_review},#{@cockpit_review},-}",
    "\u{1f}#{?#{@cockpit_hold},#{@cockpit_hold},-}",
    "\u{1f}#{?#{@cockpit_review_spool},#{@cockpit_review_spool},-}",
);

/// `-` back to empty. A sentinelled option whose real value is a bare `-` would
/// read as unset; none of these ever holds one.
fn unsentinel(s: &str) -> String {
    if s == "-" { String::new() } else { s.to_string() }
}

/// `None` when the list-panes call itself failed: no tmux, not "no agents".
pub fn list_panes() -> Option<Vec<Pane>> {
    let out = tmux(&["list-panes", "-a", "-F", FIELDS])?;
    let (mut agents, mut procs_t) = (None, None);
    let mut git: HashMap<String, (String, String, String)> = HashMap::new();
    let mut ps = vec![];
    for l in out.lines() {
        let f: Vec<&str> = l.split(SEP).collect();
        if f.len() < 13 {
            continue;
        }
        // A pane carrying @panestate counts whatever it runs: the hooks are the
        // source of truth, and a wrapper hides `claude` behind its own argv0.
        let state = unsentinel(f[4]);
        // Rule 72: @panestate is the hooks' own answer, the name is a fast path
        // that skips the `ps` fork when tmux already says agent, and the process
        // tree is the one that actually decides.
        let agent = !state.is_empty() || agentish(f[3]) || {
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
            state,
            title: unsentinel(f[5]),
            msg: unsentinel(f[6]),
            sess: unsentinel(f[7]),
            path: f[8].into(),
            pid: f[9].into(),
            review: unsentinel(f[10]),
            hold: unsentinel(f[11]),
            spool: unsentinel(f[12]),
            ..Default::default()
        };
        if p.sess.is_empty() {
            let agents = agents.get_or_insert_with(agent_sessions_cached);
            p.sess = find_session(&p.pid, procs_t.get_or_insert_with(procs), agents);
        }
        read_transcript(&mut p);
        let (loc, pm, root) = git.entry(p.path.clone()).or_insert_with(|| git_facts(&p.path)).clone();
        p.loc = loc;
        p.root = root;
        (p.pinned, p.delivered) = spool_counts(&p.spool);
        p.pm = pm;
        ps.push(p);
    }
    // needs-input first, then running, then the rest; the fleet exists to answer
    // "which one needs me", so rank is not a sort option.
    ps.sort_by_key(|p| match p.state.as_str() {
        "needs-input" => 0,
        "error" => 1,
        "running" => 2,
        "done" => 3,
        _ => 4,
    });
    Some(ps)
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

/// Pane text is valid UTF-8 but rarely ASCII, and slicing a byte offset that
/// lands inside a glyph panics; agents-in-a-box shipped that crash twice.
fn snap(s: &str, mut at: usize) -> usize {
    while at < s.len() && !s.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// The 5-column right rail of screens.md §0: 32s, 7m12s, 1h04m, 3d. Never more
/// than 5 columns, which is why seconds are dropped past ten minutes and
/// minutes past ten hours: "10m05s" and "23h59m" are six and would be clipped
/// by the rail, losing the leading digit and lying about the age.
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

/// Truncate right to `n` display columns, `…` per screens.md §0. Counts chars,
/// not bytes.
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
