//! tmux and transcript reads. Nothing here writes to a tmux server; the only
//! mutating action the binary can take is printed to stdout for a shell wrapper
//! to run, per tui.md §6 (actions stay shell scripts).
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

pub fn log(s: &str) {
    if let Ok(p) = std::env::var("COCKPIT_LOG")
        && let Ok(mut f) = OpenOptions::new().append(true).create(true).open(p)
    {
        let _ = writeln!(f, "{s}");
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

fn transcript_path(sess: &str, path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let slug = path.replace(['/', '.'], "-");
    format!("{home}/.claude/projects/{slug}/{sess}.jsonl")
}

/// Last `cap` bytes of a file, whole lines only.
fn tail(p: &str, cap: u64) -> Vec<String> {
    let Ok(mut f) = File::open(p) else { return vec![] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let off = len.saturating_sub(cap);
    let _ = f.seek(SeekFrom::Start(off));
    let mut s = String::new();
    let _ = f.read_to_string(&mut s);
    let mut lines: Vec<String> = s.lines().map(String::from).collect();
    if off > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines
}

fn first_line(s: &str) -> String {
    s.trim().lines().next().unwrap_or("").trim().to_string()
}

/// doing / said / asked and their ages, from the tail of the session transcript.
fn read_transcript(p: &mut Pane) {
    if p.sess.is_empty() {
        return;
    }
    let (mut tool, mut text, mut user) = (String::new(), String::new(), String::new());
    let (mut at, mut user_at, mut newest) = (None::<SystemTime>, None::<SystemTime>, None::<SystemTime>);
    for raw in tail(&transcript_path(&p.sess, &p.path), 65536) {
        let Ok(v) = serde_json::from_str::<Value>(&raw) else { continue };
        let ts = v["timestamp"].as_str().and_then(rfc3339);
        if ts.is_some() {
            newest = ts;
        }
        match v["type"].as_str() {
            Some("user") => {
                if let Some(t) = v["message"]["content"].as_str() {
                    // a tool_result arrives as an array, a human turn as a string
                    user = first_line(t);
                    user_at = ts;
                }
            }
            Some("assistant") => {
                let Some(blocks) = v["message"]["content"].as_array() else { continue };
                for b in blocks {
                    match b["type"].as_str() {
                        Some("tool_use") => {
                            let i = &b["input"];
                            let d = ["description", "summary", "prompt", "command", "file_path"]
                                .iter()
                                .find_map(|k| i[k].as_str().filter(|s| !s.is_empty()))
                                .unwrap_or("");
                            tool = format!("{}: {}", b["name"].as_str().unwrap_or(""), first_line(d));
                            at = ts;
                        }
                        Some("text") => {
                            let t = first_line(b["text"].as_str().unwrap_or(""));
                            if !t.is_empty() {
                                text = t;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    p.doing = tool;
    p.said = text;
    p.asked = user;
    p.since = at.and_then(age);
    p.asked_since = user_at.and_then(age);
    p.idle_since = newest.and_then(age);
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
    /// `claude agents --json` costs ~360 ms, an order of magnitude more than
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
    let out = Command::new("claude").args(["agents", "--json"]).output().map(|o| o.stdout).unwrap_or_default();
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
        .args(["-C", path, "--no-optional-locks", "rev-parse", "--show-toplevel", "--abbrev-ref", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut it = top.lines();
    let (Some(root), Some(branch)) = (it.next(), it.next()) else { return (String::new(), String::new(), String::new()) };
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
    let pm = if add == 0 && del == 0 { "clean".to_string() } else { format!("+{add} -{del}") };
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

/// ~ for $HOME, so a path fits the value budget without truncating.
pub fn home_rel(p: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

/// One pane by id, for the info and ask screens.
pub fn one_pane(id: &str) -> Pane {
    list_panes().into_iter().find(|p| p.id == id).unwrap_or(Pane { id: id.to_string(), ..Default::default() })
}

/// The observer's context: the same reduction claude-code.md's jq filter makes,
/// done in-process so the binary needs no jq.
pub fn excerpt(sess: &str, path: &str, cap: usize) -> String {
    let mut out: Vec<String> = vec![];
    for raw in tail(&transcript_path(sess, path), 400 * 1024) {
        let Ok(v) = serde_json::from_str::<Value>(&raw) else { continue };
        let ts = v["timestamp"].as_str().unwrap_or("");
        let hhmmss = if ts.len() >= 19 { &ts[11..19] } else { "" };
        match v["type"].as_str() {
            Some("user") => {
                if let Some(t) = v["message"]["content"].as_str() {
                    out.push(format!("USER {hhmmss}: {}", clip(t, 400)));
                }
            }
            Some("assistant") => {
                for b in v["message"]["content"].as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => out.push(format!("AGENT: {}", clip(b["text"].as_str().unwrap_or(""), 400))),
                        Some("tool_use") => {
                            let i = &b["input"];
                            let d = ["description", "summary", "prompt", "command", "file_path"]
                                .iter()
                                .find_map(|k| i[k].as_str().filter(|s| !s.is_empty()))
                                .unwrap_or("");
                            out.push(format!("TOOL {}: {}", b["name"].as_str().unwrap_or(""), clip(d, 120)));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let joined = out.join("\n");
    let b = joined.as_bytes();
    if b.len() <= cap {
        return joined;
    }
    // last cap bytes, on a char boundary
    let mut off = b.len() - cap;
    while off < b.len() && !joined.is_char_boundary(off) {
        off += 1;
    }
    joined[off..].to_string()
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>().replace('\n', " ")
}

const FIELDS: &str = "#{pane_id}\t#{session_name}:#{window_index}\t#{window_name}\t#{pane_current_command}\t#{@panestate}\t#{@cockpit_title}\t#{@panestate_msg}\t#{@cockpit_session}\t#{pane_current_path}\t#{pane_pid}\t#{@cockpit_review}\t#{@cockpit_hold}\t#{@cockpit_review_spool}";

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
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 13 || (!known.contains(&f[3]) && f[4].is_empty()) {
            continue;
        }
        let mut p = Pane {
            id: f[0].into(),
            wher: f[1].into(),
            name: f[2].into(),
            cmd: f[3].into(),
            state: f[4].into(),
            title: f[5].into(),
            msg: f[6].into(),
            sess: f[7].into(),
            path: f[8].into(),
            pid: f[9].into(),
            review: f[10].into(),
            hold: f[11].into(),
            spool: f[12].into(),
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
        let need = if cur.is_empty() { word.chars().count() } else { cur.chars().count() + 1 + word.chars().count() };
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
