//! The transcript reader: `file`, `doing` and `tail`. Its output is byte-identical to the jq
//! reader it replaced (Freeze 1, #54). Every failure is `None`.
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

/// Claude Code's projects directory: `$CLAUDE_CONFIG_DIR/projects`, else `~/.claude/projects`.
pub(crate) fn projects_dir() -> PathBuf {
    match std::env::var("CLAUDE_CONFIG_DIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d).join("projects"),
        _ => PathBuf::from(format!("{}/.claude/projects", std::env::var("HOME").unwrap_or_default())),
    }
}

/// `path` as a regular file inside `root`, symlinks resolved, or None. The hooks check the
/// same rule before writing @cockpit_transcript; the reader does not trust the option.
pub(crate) fn under(root: &Path, path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.split('/').any(|c| c == "..") {
        return None;
    }
    let p = std::fs::canonicalize(path).ok()?;
    let root = std::fs::canonicalize(root).ok()?;
    (p.starts_with(&root) && p.is_file()).then_some(p)
}

/// The transcript of a pane's session: @cockpit_transcript (the hook's `transcript_path`),
/// else @cockpit_session, else `claude agents --json` joined on the pane's process tree;
/// then [`locate`], the lookup the board shares. `cwd` is the pane's pane_current_path.
pub fn file(pane: &str, cwd: &str) -> Option<PathBuf> {
    if pane.is_empty() {
        return None;
    }
    let one = |args: &[&str]| super::tmux(args).unwrap_or_default().trim_end_matches('\n').to_string();
    let opt = one(&["show-option", "-pqv", "-t", pane, "@cockpit_transcript"]);
    if let Some(p) = under(&projects_dir(), &opt) {
        return Some(p);
    }
    let mut sid = one(&["show-option", "-pqv", "-t", pane, "@cockpit_session"]);
    if sid.is_empty() {
        let pp: i64 = one(&["display-message", "-p", "-t", pane, "#{pane_pid}"]).parse().ok()?;
        sid = agents().into_iter().find(|(pid, _)| descends(*pid, pp)).map(|(_, s)| s)?;
    }
    // one look, so no memory of a miss: a session that just started is found
    find(&projects_dir(), &opt, &sid, cwd, None)
}

thread_local! {
    /// [`locate`]'s third step by session id: a found file until it is gone, a miss
    /// for [`GLOB_MISS_TTL`], so the board's refresh does not list every project
    /// dir each tick for a pane whose transcript is nowhere.
    static GLOBBED: std::cell::RefCell<std::collections::HashMap<String, (std::time::Instant, Option<PathBuf>)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

const GLOB_MISS_TTL: std::time::Duration = std::time::Duration::from_secs(10);

/// `<projects>/<cwd's slug>/<sid>.jsonl`, where Claude Code writes a session started in
/// `cwd`; the slug maps every byte outside `[A-Za-z0-9-]` to `-`. None for no cwd.
pub(crate) fn slug_path(sid: &str, cwd: &str) -> Option<PathBuf> {
    slug_in(&projects_dir(), sid, cwd)
}

fn slug_in(dir: &Path, sid: &str, cwd: &str) -> Option<PathBuf> {
    // Per byte, not per char: a two-byte glyph in the cwd becomes two dashes.
    let slug: String = cwd
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' { b as char } else { '-' })
        .collect();
    (!cwd.is_empty()).then(|| dir.join(slug).join(format!("{sid}.jsonl")))
}

/// #500: the one transcript lookup the board, `workdir` and the info screen share,
/// so they read the same file. In order: `opt` (@cockpit_transcript) when it resolves
/// to a regular file under [`projects_dir`] ([`under`]: no `..`, no symlink out, since
/// an agent can set it); else `<projects>/<cwd's slug>/<sid>.jsonl`; else the first
/// `<projects>/*/<sid>.jsonl`, sorted, dot dirs left out. None for a session id that
/// could name a path or a glob. The board polls, so its third step keeps what it found
/// until the file is gone and a miss for [`GLOB_MISS_TTL`].
pub(crate) fn locate(opt: &str, sid: &str, cwd: &str) -> Option<PathBuf> {
    find(&projects_dir(), opt, sid, cwd, Some(std::time::Instant::now()))
}

/// [`locate`]'s steps under the projects dir `dir`. With `now`, the third step uses and
/// fills its memory, a miss counting as fresh until `now` is [`GLOB_MISS_TTL`] past it;
/// the clock is a parameter so a test can step it.
fn find(dir: &Path, opt: &str, sid: &str, cwd: &str, now: Option<std::time::Instant>) -> Option<PathBuf> {
    if let Some(p) = under(dir, opt) {
        return Some(p);
    }
    if sid.is_empty() || !sid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return None;
    }
    if let Some(p) = slug_in(dir, sid, cwd).filter(|p| p.is_file()) {
        return Some(p);
    }
    let seen = now.and_then(|_| GLOBBED.with(|c| c.borrow().get(sid).cloned()));
    match (seen, now) {
        (Some((_, Some(p))), _) if p.is_file() => return Some(p),
        (Some((at, None)), Some(now)) if now.saturating_duration_since(at) < GLOB_MISS_TTL => return None,
        _ => {}
    }
    // The shell's glob: sorted, dot directories left out.
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    names.retain(|n| !n.as_bytes().starts_with(b"."));
    names.sort();
    let found = names
        .into_iter()
        .map(|n| dir.join(n).join(format!("{sid}.jsonl")))
        .find(|p| p.is_file());
    if let Some(now) = now {
        GLOBBED.with(|c| c.borrow_mut().insert(sid.to_string(), (now, found.clone())));
    }
    found
}

/// `claude agents --json` takes about 0.4 s and could hang: bounded to 2 s when
/// `timeout` exists. Rows are (pid, sessionId), in the list's order.
fn agents() -> Vec<(i64, String)> {
    let run = |argv: &[&str]| {
        Command::new(argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
    };
    let out = match run(&["timeout", "2", "claude", "agents", "--json"]) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => run(&["claude", "agents", "--json"]),
        r => r,
    };
    let v: Value = out.ok().and_then(|o| serde_json::from_slice(&o.stdout).ok()).unwrap_or(Value::Null);
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r["pid"].as_i64()?, r["sessionId"].as_str()?.to_string())))
        .collect()
}

/// True when `pid` is `anc` or a descendant of it. Bounded: a ppid chain read
/// across a pid reuse could in principle loop.
fn descends(mut pid: i64, anc: i64) -> bool {
    for _ in 0..1024 {
        if pid <= 1 {
            return false;
        }
        if pid == anc {
            return true;
        }
        match ppid(pid) {
            Some(p) => pid = p,
            None => return false,
        }
    }
    false
}

/// /proc first; `ps` only where there is no /proc at all (macOS).
fn ppid(pid: i64) -> Option<i64> {
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // past the command name, which may hold spaces: state, then the parent pid
        return stat.rsplit_once(") ")?.1.split_whitespace().nth(1)?.parse().ok();
    }
    if Path::new("/proc/self").is_dir() {
        return None;
    }
    let o = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&o.stdout).trim().parse().ok()
}

/// `tail -n 300`: a final line with no newline still counts as a line.
fn last_lines(path: &Path) -> Option<String> {
    const N: usize = 300;
    if !path.is_file() {
        return None;
    }
    let mut f = std::fs::File::open(path).ok()?;
    // The file's last byte never opens a line, newline or not.
    let (mut pos, mut seen, mut start) = (f.metadata().ok()?.len().saturating_sub(1), 0, 0);
    let mut chunk = vec![0; 64 * 1024];
    'scan: while pos > 0 {
        let n = pos.min(chunk.len() as u64) as usize;
        pos -= n as u64;
        f.seek(SeekFrom::Start(pos)).ok()?;
        f.read_exact(&mut chunk[..n]).ok()?;
        for (i, _) in chunk[..n].iter().enumerate().rev().filter(|(_, b)| **b == b'\n') {
            seen += 1;
            if seen == N {
                start = pos + i as u64 + 1;
                break 'scan;
            }
        }
    }
    let mut buf = vec![];
    f.seek(SeekFrom::Start(start)).ok()?;
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// jq's `clean`: the first line holding a non-blank char, TAB to a space, C0, DEL
/// and C1 dropped, leading blanks trimmed, 160 chars. Bidi controls are dropped
/// too (#453), which jq's never did: tool details, `said` and `asked` are model-
/// or human-written and reach the info screen.
fn clean(s: &str) -> String {
    let line = s.split('\n').find(|l| l.chars().any(|c| !c.is_whitespace())).unwrap_or("");
    let kept: String = line
        .chars()
        .map(|c| if c == '\t' { ' ' } else { c })
        .filter(|&c| c >= ' ' && !('\u{7f}'..='\u{9f}').contains(&c) && !crate::pick::is_bidi(c))
        .collect();
    kept.trim_start().chars().take(160).collect()
}

/// What the harness injected is "", never a prompt. The bash-* tags are #193's,
/// added after the jq reader was frozen.
const INJECTED: [&str; 7] = [
    "<task-notification",
    "<system-reminder",
    "<local-command-",
    "<command-",
    "<bash-input",
    "<bash-stdout",
    "<bash-stderr",
];

fn prompt(s: &str) -> String {
    let t = s.trim_start();
    if t.starts_with("<pasted_content") {
        "[pasted text]".into()
    } else if INJECTED.iter().any(|p| t.starts_with(p)) {
        String::new()
    } else {
        clean(s)
    }
}

/// C's isspace: unlike `is_ascii_whitespace` it takes the vertical tab.
fn c_space(c: &u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// glibc's strptime `get_number`: leading spaces, at most `n` digits, and it stops
/// early once another digit would pass `to`.
fn number(b: &[u8], i: &mut usize, from: i64, to: i64, n: usize) -> Option<i64> {
    while b.get(*i).is_some_and(c_space) {
        *i += 1;
    }
    let mut val = 0;
    for k in 0..n {
        match b.get(*i) {
            Some(c) if c.is_ascii_digit() && (k == 0 || val * 10 <= to) => {
                val = val * 10 + i64::from(c - b'0');
                *i += 1;
            }
            _ if k == 0 => return None,
            _ => break,
        }
    }
    (from..=to).contains(&val).then_some(val)
}

/// jq's `fromdateiso8601` after `sub("\\.[0-9]+"; "")`: strptime("%Y-%m-%dT%H:%M:%SZ")
/// then timegm, which jq reports as an error when it returns -1.
fn epoch(ts: &str) -> Option<i64> {
    let b = ts.as_bytes();
    let cut = (0..b.len()).find(|&i| b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit));
    let s: Vec<u8> = match cut {
        Some(i) => {
            let end = i + 1 + b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            [&b[..i], &b[end..]].concat()
        }
        None => b.to_vec(),
    };
    let mut i = 0;
    let lit = |i: &mut usize, c: u8| (s.get(*i) == Some(&c)).then(|| *i += 1);
    let y = number(&s, &mut i, 0, 9999, 4)?;
    lit(&mut i, b'-')?;
    let m = number(&s, &mut i, 1, 12, 2)?;
    lit(&mut i, b'-')?;
    let d = number(&s, &mut i, 1, 31, 2)?;
    lit(&mut i, b'T')?;
    let hh = number(&s, &mut i, 0, 23, 2)?;
    lit(&mut i, b':')?;
    let mm = number(&s, &mut i, 0, 59, 2)?;
    lit(&mut i, b':')?;
    let ss = number(&s, &mut i, 0, 61, 2)?;
    lit(&mut i, b'Z')?;
    if !s.get(i).is_none_or(c_space) {
        return None;
    }
    // Days from civil (Howard Hinnant); day 31 of a short month rolls over like timegm.
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + (153 * m2 + 2) / 5;
    let days = era * 146097 + doe - 719468 + d - 1;
    let t = days * 86400 + hh * 3600 + mm * 60 + ss;
    (t != -1).then_some(t)
}

/// Whole seconds since the row's timestamp, never negative; "" when it does not parse.
fn secs(v: &Value, now: i64) -> String {
    v["timestamp"]
        .as_str()
        .and_then(epoch)
        .map(|t| (now - t).max(0).to_string())
        .unwrap_or_default()
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The leading numeric x.y.z of a version; "2.1" is 2.1.0.
fn version(s: &str) -> Option<[f64; 3]> {
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let n = digits(s);
    let mut v = [s[..n].parse().ok()?, 0.0, 0.0];
    let mut rest = &s[n..];
    for slot in &mut v[1..] {
        let Some(r) = rest.strip_prefix('.').filter(|r| digits(r) > 0) else {
            break;
        };
        *slot = r[..digits(r)].parse().ok()?;
        rest = &r[digits(r)..];
    }
    Some(v)
}

fn items(v: &Value) -> impl Iterator<Item = &Value> {
    v["message"]["content"].as_array().into_iter().flatten()
}

fn kind(v: &Value) -> &str {
    v["type"].as_str().unwrap_or("")
}

/// A string prompt from the human: not isMeta, content a string.
fn human(v: &Value) -> Option<&str> {
    let meta = !matches!(v.get("isMeta"), None | Some(Value::Null) | Some(Value::Bool(false)));
    (kind(v) == "user" && !meta).then(|| v["message"]["content"].as_str()).flatten()
}

/// Which input field says what a tool is doing, in preference order: the
/// concrete argument first, the prose last. Shared with the board (data.rs).
const DETAIL: [&str; 12] = [
    "command",
    "file_path",
    "path",
    "pattern",
    "description",
    "summary",
    "subject",
    "prompt",
    "query",
    "url",
    "skill",
    "task_id",
];

/// The raw detail of a tool_use as (prefix, text): the first DETAIL field holding
/// a non-blank string; else SendMessage's `to` as "to <to>"; else an
/// AskUserQuestion's first `header` (#453); else, for an `mcp__*` tool, the first
/// string-valued input field by key order. Other nested objects and arrays never
/// count. None gives the bare tool name.
pub(crate) fn pick<'a>(name: &str, input: &'a Value) -> Option<(&'static str, &'a str)> {
    let ok = |s: &&str| s.chars().any(|c| !c.is_whitespace());
    DETAIL
        .iter()
        .find_map(|k| input[k].as_str().filter(ok).map(|s| ("", s)))
        .or_else(|| input["to"].as_str().filter(ok).map(|s| ("to ", s)))
        .or_else(|| {
            let h = input["questions"][0]["header"].as_str();
            h.filter(|_| name == "AskUserQuestion").filter(ok).map(|s| ("", s))
        })
        .or_else(|| {
            if !name.starts_with("mcp__") {
                return None;
            }
            let mut kv: Vec<_> = input.as_object()?.iter().filter_map(|(k, v)| Some((k, v.as_str()?))).collect();
            kv.sort_by_key(|(k, _)| *k);
            kv.into_iter().map(|(_, s)| s).find(ok).map(|s| ("", s))
        })
}

/// An AskUserQuestion's row text (#453): how many questions follow the first,
/// then the first non-blank line of the first, made safe to draw. The count
/// leads because the board cuts a long line's end. `None` when the input names
/// no question.
pub(crate) fn question(input: &Value) -> Option<String> {
    let qs = input["questions"].as_array()?;
    let line = qs.first()?["question"]
        .as_str()?
        .lines()
        .map(|l| crate::pick::clean(l).trim().to_string())
        .find(|l| !l.is_empty())?;
    Some(match qs.len() - 1 {
        0 => line,
        n => format!("(+{n}) {line}"),
    })
}

fn tlabel(it: &Value) -> String {
    let n = it["name"].as_str().unwrap_or("");
    let d = pick(n, &it["input"])
        .map(|(p, s)| (p, clean(s)))
        .filter(|(_, d)| !d.is_empty())
        .map(|(p, d)| format!("{p}{d}"));
    clean(&match d {
        Some(d) => format!("{n}: {d}"),
        None => n.to_string(),
    })
}

/// jq reads a lone low-surrogate escape as U+FFFD where serde_json refuses the
/// whole line; a lone high one fails in both.
fn lone_low(l: &str) -> String {
    let b = l.as_bytes();
    let esc = |i: usize| {
        let e = b.get(i..i + 6).filter(|e| e.starts_with(b"\\u"))?;
        u32::from_str_radix(std::str::from_utf8(&e[2..]).ok()?, 16).ok()
    };
    let (mut out, mut i, mut last) = (String::with_capacity(l.len()), 0, 0);
    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }
        match esc(i) {
            Some(0xD800..=0xDBFF) if matches!(esc(i + 6), Some(0xDC00..=0xDFFF)) => i += 12,
            Some(0xDC00..=0xDFFF) => {
                out += &l[last..i];
                out += "\\ufffd";
                i += 6;
                last = i;
            }
            Some(_) => i += 6,
            None => i += 2,
        }
    }
    out + &l[last..]
}

/// The user and assistant rows of the last 300 lines, or None below the version
/// gate. A row whose message is not an object is skipped (#193), where jq fails.
fn turns(path: &Path) -> Option<Vec<Value>> {
    let all: Vec<Value> = last_lines(path)?
        .split('\n')
        .filter_map(|l| {
            serde_json::from_str::<Value>(l)
                .or_else(|_| serde_json::from_str(&lone_low(l)))
                .ok()
        })
        .filter(Value::is_object)
        .collect();
    let v = all.iter().filter_map(|r| r["version"].as_str().and_then(version)).next_back();
    if v.is_some_and(|v| v < [2.0, 1.0, 246.0]) {
        return None;
    }
    let ok = |r: &Value| matches!(kind(r), "user" | "assistant") && matches!(r.get("message"), None | Some(Value::Null | Value::Object(_)));
    Some(all.into_iter().filter(ok).collect())
}

/// jq's `==`: numbers by value, so an id of 1 is closed by a tool_use_id of 1.0.
fn jq_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| jq_eq(x, y)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| jq_eq(v, w))),
        _ => a == b,
    }
}

/// The newest main-thread row's `cwd` in the last 300 lines (#452): where the
/// session works now, which a `cd` into a worktree moves and the pane's cwd does
/// not. A sidechain's never counts. The board's reader keeps the same rule.
pub fn cwd(path: &Path) -> Option<String> {
    last_lines(path)?
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|r| r["isSidechain"].as_bool() != Some(true) && r["agentId"].is_null())
        .find_map(|r| r["cwd"].as_str().filter(|c| !c.is_empty()).map(String::from))
}

/// `doing\t<label>\t<secs>`, `said\t<text>`, `asked\t<prompt>\t<secs>`, each left
/// out when unknown. Only the last tool_use counts, open parallel ones before it do not.
/// Then `ask\t<question>` for the newest AskUserQuestion still open (#453).
pub fn doing(path: &Path) -> Option<String> {
    let t = turns(path)?;
    let now = now();
    let closed: Vec<&Value> = t
        .iter()
        .filter(|r| kind(r) == "user")
        .flat_map(items)
        .filter(|i| kind(i) == "tool_result")
        .map(|i| i.get("tool_use_id").unwrap_or(&Value::Null))
        .collect();
    let said_of = |r: &Value| {
        items(r)
            .filter(|i| kind(i) == "text")
            .filter_map(|i| i["text"].as_str().map(clean))
            .filter(|s| !s.is_empty())
            .last()
    };
    let assistant = || t.iter().filter(|r| kind(r) == "assistant");
    let tu = assistant()
        .flat_map(|r| items(r).filter(|i| kind(i) == "tool_use").map(move |i| (i, r)))
        .last();
    let said = assistant().filter_map(said_of).next_back();
    let asked = t
        .iter()
        .filter_map(|r| human(r).map(|c| (prompt(c), r)))
        .rfind(|(p, _)| !p.is_empty());
    let mut out = String::new();
    if let Some((i, r)) = tu
        && !closed.iter().any(|c| jq_eq(c, i.get("id").unwrap_or(&Value::Null)))
    {
        out += &format!("doing\t{}\t{}\n", tlabel(i), secs(r, now));
    }
    if let Some(s) = said {
        out += &format!("said\t{s}\n");
    }
    if let Some((p, r)) = asked {
        out += &format!("asked\t{p}\t{}\n", secs(r, now));
    }
    // Only the newest tool_use's ask is what the agent waits on, and a prompt
    // typed after it means the human moved on from it.
    let typed_after = |r: &Value| {
        let at = t.iter().position(|x| std::ptr::eq(x, r));
        t.iter()
            .enumerate()
            .any(|(k, x)| Some(k) > at && human(x).is_some_and(|c| !prompt(c).is_empty()))
    };
    if let Some((i, r)) = tu
        && i["name"] == "AskUserQuestion"
        && !closed.iter().any(|c| jq_eq(c, i.get("id").unwrap_or(&Value::Null)))
        && !typed_after(r)
        && let Some(q) = question(&i["input"])
    {
        out += &format!("ask\t{q}\n");
    }
    (!out.is_empty()).then_some(out)
}

/// `USER: ` / `AGENT: ` / `TOOL <label>` lines, oldest first, dropped from the top
/// while the bytes (newlines counted) pass `max`.
pub fn tail(path: &Path, max: i64) -> Option<String> {
    let mut lines: Vec<String> = vec![];
    for r in turns(path)? {
        if kind(&r) == "user" {
            lines.extend(human(&r).map(prompt).filter(|p| !p.is_empty()).map(|p| format!("USER: {p}")));
            continue;
        }
        for i in items(&r) {
            match kind(i) {
                "text" => lines.extend(
                    i["text"]
                        .as_str()
                        .map(clean)
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("AGENT: {s}")),
                ),
                "tool_use" => lines.push(format!("TOOL {}", tlabel(i))),
                _ => {}
            }
        }
    }
    let mut used = 0;
    let keep = lines
        .iter()
        .rev()
        .take_while(|l| {
            used += l.len() as i64 + 1;
            used <= max
        })
        .count();
    (keep > 0).then(|| lines[lines.len() - keep..].iter().map(|l| format!("{l}\n")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One case per line: [tool name, input, expected label]. tests/run.sh feeds the
    /// same file to the jq reader, so the two must agree.
    const CASES: &str = include_str!("../../../tests/fixtures/tool-labels/cases.jsonl");

    #[test]
    fn tlabel_names_what_each_tool_is_doing() {
        let mut n = 0;
        for l in CASES.lines() {
            let c: Value = serde_json::from_str(l).unwrap();
            let it = serde_json::json!({"type": "tool_use", "name": c[0], "input": c[1]});
            assert_eq!(tlabel(&it), c[2].as_str().unwrap(), "{l}");
            n += 1;
        }
        assert_eq!(n, 19);
    }

    /// Removes the tree when dropped, so a failing assert leaves nothing in /tmp.
    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tree() -> (PathBuf, Tmp) {
        let t = std::env::temp_dir().join(format!("tx-under-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("projects/-p")).unwrap();
        std::fs::write(t.join("projects/-p/a.jsonl"), "{}").unwrap();
        std::fs::write(t.join("out.jsonl"), "{}").unwrap();
        (t.join("projects"), Tmp(t))
    }

    #[test]
    fn under_accepts_a_file_inside_the_tree() {
        let (root, _t) = tree();
        let p = format!("{}/-p/a.jsonl", root.display());
        assert_eq!(under(&root, &p), std::fs::canonicalize(&p).ok());
    }

    #[test]
    fn under_refuses_outside_dotdot_symlink_directory_and_empty() {
        let (root, t) = tree();
        let t = &t.0;
        let r = root.display();
        std::os::unix::fs::symlink(t.join("out.jsonl"), root.join("-p/link.jsonl")).unwrap();
        for bad in [
            format!("{}/out.jsonl", t.display()),
            format!("{r}/-p/../../out.jsonl"),
            format!("{r}/-p/../-p/a.jsonl"),
            format!("{r}/-p/link.jsonl"),
            format!("{r}/-p"),
            String::new(),
        ] {
            assert_eq!(under(&root, &bad), None, "{bad:?}");
        }
    }

    /// #453: an AskUserQuestion is labelled by its first header; another tool's
    /// `questions` is not a field the label reads.
    #[test]
    fn an_ask_is_labelled_by_its_first_header_and_only_an_ask() {
        let qs = serde_json::json!({"questions": [{"question": "Merge?", "header": "Merge"}]});
        let label = |n: &str| tlabel(&serde_json::json!({"name": n, "input": qs}));
        assert_eq!(
            (label("AskUserQuestion"), label("Other")),
            ("AskUserQuestion: Merge".into(), "Other".into())
        );
    }

    /// #453: rows are [input, question]; `None` where the input names no question.
    #[test]
    fn question_reads_the_first_question_and_counts_the_rest() {
        let rows = [
            (serde_json::json!({"questions": [{"question": "a?"}]}), Some("a?")),
            (serde_json::json!({"questions": [{"question": "a?"}, {}]}), Some("(+1) a?")),
            (
                serde_json::json!({"questions": [{"question": " \n\u{2066}b\u{7}?\nc"}]}),
                Some("b?"),
            ),
            (serde_json::json!({"questions": []}), None),
            (serde_json::json!({"questions": [{"question": ["a?"]}]}), None),
            (serde_json::json!({"question": "a?"}), None),
        ];
        for (input, want) in rows {
            assert_eq!(question(&input).as_deref(), want, "{input}");
        }
    }

    /// #500: `<root>/<project>/<sid>.jsonl`, written; its path.
    fn put(root: &Path, project: &str, sid: &str) -> PathBuf {
        std::fs::create_dir_all(root.join(project)).unwrap();
        let p = root.join(project).join(format!("{sid}.jsonl"));
        std::fs::write(&p, "{}").unwrap();
        std::fs::canonicalize(&p).unwrap()
    }

    /// #500: the option outranks the cwd's slug, which outranks the glob; an option
    /// outside the tree never counts; a session id that is a path finds nothing.
    #[test]
    fn find_takes_the_option_then_the_slug_then_the_glob() {
        let (root, t) = tree();
        let opt = put(&root, "-x", "s1");
        let slugged = put(&root, "-w", "s1");
        put(&root, "-a", "s1");
        let o = opt.display().to_string();
        assert_eq!(find(&root, &o, "s1", "/w", None), Some(opt), "the option");
        assert_eq!(find(&root, "", "s1", "/w", None).as_deref(), Some(slugged.as_path()), "the slug");
        let out = t.0.join("out.jsonl").display().to_string();
        assert_eq!(find(&root, &out, "s1", "/w", None).as_deref(), Some(slugged.as_path()), "outside");
        let first = std::fs::canonicalize(root.join("-a/s1.jsonl")).unwrap();
        let found = find(&root, "", "s1", "/elsewhere", None).map(|p| std::fs::canonicalize(p).unwrap());
        assert_eq!(found, Some(first), "the glob, sorted: -a before -w and -x");
        // `../s1` would climb out of a project dir to this file
        std::fs::write(root.join("s1.jsonl"), "{}").unwrap();
        assert_eq!(find(&root, "", "../s1", "/w", None), None, "a path for an id");
    }

    /// #500: the board's memory of the glob: a found file is kept until it is gone,
    /// then the glob runs again; a miss is kept until GLOB_MISS_TTL has passed on the
    /// clock the test steps, never longer.
    #[test]
    fn the_glob_keeps_a_find_until_it_is_gone_and_a_miss_for_its_ttl() {
        let (root, _t) = tree();
        let t0 = std::time::Instant::now();
        let at = |s: u64| Some(t0 + std::time::Duration::from_secs(s));
        let real = |p: Option<PathBuf>| p.map(|p| std::fs::canonicalize(p).unwrap());
        let a = put(&root, "-a", "s2");
        assert_eq!(real(find(&root, "", "s2", "/x", at(0))), Some(a.clone()));
        std::fs::remove_file(&a).unwrap();
        let b = put(&root, "-b", "s2");
        assert_eq!(real(find(&root, "", "s2", "/x", at(0))), Some(b), "a gone find is looked for again");
        assert_eq!(find(&root, "", "s3", "/x", at(0)), None);
        put(&root, "-a", "s3");
        assert_eq!(find(&root, "", "s3", "/x", at(9)), None, "the miss is kept inside its ttl");
        assert!(find(&root, "", "s3", "/x", at(10)).is_some(), "and looked for again after it");
        assert_eq!(find(&root, "", "s4", "/x", None), None);
        put(&root, "-a", "s4");
        assert!(find(&root, "", "s4", "/x", None).is_some(), "an uncached look keeps no miss");
    }

    #[test]
    fn a_label_never_ends_in_a_colon() {
        for l in CASES.lines() {
            let c: Value = serde_json::from_str(l).unwrap();
            let it = serde_json::json!({"name": c[0], "input": c[1]});
            assert!(!tlabel(&it).trim_end().ends_with(':'), "{l}");
        }
    }
}
