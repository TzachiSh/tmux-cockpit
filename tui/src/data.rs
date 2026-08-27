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
//! session, path`. `where` is `sess:idx name`; `title` falls back to the pane
//! name then `(no title)`; `since` is seconds or `null`; `diff` is the `±`
//! string; `state` ∈ `needs-input | running | done | error | ""`.
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
//! not a tty), `2` usage error.
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

pub fn tmux(args: &[&str]) -> String {
    let c = tmux_argv();
    Command::new(&c[0])
        .args(&c[1..])
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
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
    tmux(args);
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

#[derive(Default, Clone, Hash, PartialEq)]
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
    /// seconds since the last tool call; not Duration, so the type can Hash
    pub since: Option<u64>,
    pub asked_since: Option<u64>,
    /// age of the newest transcript event of any kind: "how long since this
    /// session did anything", the number the state row wants
    pub idle_since: Option<u64>,
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
    let slug: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
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

fn first_line(s: &str) -> String {
    s.trim().lines().next().unwrap_or("").trim().to_string()
}

/// What the tail says, held as timestamps rather than ages: the ages move every
/// second, these do not, so only these are worth caching.
#[derive(Clone, Default)]
struct Facts {
    tool: String,
    text: String,
    user: String,
    at: Option<SystemTime>,
    user_at: Option<SystemTime>,
    newest: Option<SystemTime>,
}

thread_local! {
    /// Keyed on (len, mtime), so an idle session costs one stat a tick instead
    /// of a parse. The path is resolved fresh every tick because `/compact` and
    /// `--resume` rotate the file out from under the key.
    static TAILS: std::cell::RefCell<HashMap<String, (u64, SystemTime, Facts)>> =
        std::cell::RefCell::new(HashMap::new());
}

/// 64 KB first (the p95 row is 4 KB), doubling while the window says nothing:
/// one row can be 750 KB, and 2 of 20 live transcripts hold no complete
/// assistant row in their last 64 KB.
const TAIL_FIRST: u64 = 64 * 1024;
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
    let mut cap = TAIL_FIRST;
    let mut fa;
    loop {
        fa = parse(tail(&mut f, len, cap));
        if !fa.tool.is_empty() || !fa.text.is_empty() || cap >= TAIL_MAX || cap >= len {
            break;
        }
        cap *= 2;
    }
    TAILS.with(|c| c.borrow_mut().insert(path.to_string(), (len, mtime, fa.clone())));
    fa
}

fn parse(lines: Vec<String>) -> Facts {
    let mut fa = Facts::default();
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
                if let Some(t) = v["message"]["content"].as_str() {
                    // a tool_result arrives as an array, a human turn as a string
                    fa.user = first_line(t);
                    fa.user_at = ts;
                }
            }
            Some("assistant") => {
                let Some(blocks) = v["message"]["content"].as_array() else {
                    continue;
                };
                for b in blocks {
                    match b["type"].as_str() {
                        Some("tool_use") => {
                            let i = &b["input"];
                            let d = ["description", "summary", "prompt", "command", "file_path"]
                                .iter()
                                .find_map(|k| i[k].as_str().filter(|s| !s.is_empty()))
                                .unwrap_or("");
                            fa.tool = format!("{}: {}", b["name"].as_str().unwrap_or(""), first_line(d));
                            fa.at = ts;
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
    fa
}

/// doing / said / asked and their ages, from the tail of the session transcript.
fn read_transcript(p: &mut Pane) {
    if p.sess.is_empty() {
        return;
    }
    let fa = read_facts(&transcript_path(&p.sess, &p.path));
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

fn descendants() -> HashMap<String, Vec<String>> {
    let out = Command::new("ps")
        .args(["-eo", "pid=,ppid="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    for l in out.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() == 2 {
            m.entry(f[1].into()).or_default().push(f[0].into());
        }
    }
    m
}

fn find_session(pid: &str, kids: &HashMap<String, Vec<String>>, agents: &HashMap<String, String>) -> String {
    let mut q = vec![pid.to_string()];
    while let Some(p) = q.pop() {
        if let Some(s) = agents.get(&p) {
            return s.clone();
        }
        if let Some(k) = kids.get(&p) {
            q.extend(k.iter().cloned());
        }
    }
    String::new()
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
pub fn one_pane(id: &str) -> Pane {
    list_panes().into_iter().find(|p| p.id == id).unwrap_or(Pane {
        id: id.to_string(),
        ..Default::default()
    })
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

pub fn list_panes() -> Vec<Pane> {
    let out = tmux(&["list-panes", "-a", "-F", FIELDS]);
    // The command list is cockpit's own @cockpit_agents default. A pane that
    // carries @panestate counts too: the hooks are the source of truth, and a
    // wrapper script hides `claude` behind its own argv0.
    let known = ["claude", "codex", "aider", "cursor", "opencode", "gemini", "crush"];
    let (mut agents, mut kids) = (None, None);
    let mut git: HashMap<String, (String, String, String)> = HashMap::new();
    let mut ps = vec![];
    for l in out.lines() {
        let f: Vec<&str> = l.split(SEP).collect();
        if f.len() < 13 {
            continue;
        }
        let state = unsentinel(f[4]);
        if !known.contains(&f[3]) && state.is_empty() {
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
            if agents.is_none() {
                agents = Some(agent_sessions_cached());
                kids = Some(descendants());
            }
            p.sess = find_session(&p.pid, kids.as_ref().unwrap(), agents.as_ref().unwrap());
        }
        read_transcript(&mut p);
        let (loc, pm, root) = git.entry(p.path.clone()).or_insert_with(|| git_facts(&p.path)).clone();
        p.loc = loc;
        p.root = root;
        (p.pinned, p.delivered) = spool_counts(&p.spool);
        p.pm = if p.state == "needs-input" { "—".into() } else { pm };
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
    ps
}

pub fn preview(id: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let out = tmux(&["capture-pane", "-ep", "-t", id, "-S", &format!("-{n}")]);
    let lines: Vec<&str> = out.trim_end().lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
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
        _ => format!("{}d", s / 86400),
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
