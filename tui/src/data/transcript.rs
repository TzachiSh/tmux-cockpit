//! `cockpit-tui transcript`, behind the scripts/lib/transcript.sh shim: its output is
//! byte-identical to the jq reader it replaced (Freeze 1, #54). Every failure is no output
//! and status 1, nothing on stderr.
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

/// `transcript file %N | doing <jsonl> | tail <jsonl> [max]`; returns the exit status.
pub fn main(args: &[String]) -> i32 {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = match a.as_slice() {
        ["file", pane, ..] => file(pane).map(|p| [p.as_os_str().as_bytes(), b"\n"].concat()),
        ["doing", path, ..] => doing(Path::new(path)).map(String::into_bytes),
        // `${2:-4096}`: an empty max is the default too.
        ["tail", path, rest @ ..] => match rest.first().copied().unwrap_or("") {
            "" => tail(Path::new(path), 4096),
            m if m.bytes().all(|b| b.is_ascii_digit()) => m.parse().ok().and_then(|m| tail(Path::new(path), m)),
            _ => None,
        }
        .map(String::into_bytes),
        _ => None,
    };
    match out {
        Some(b) => {
            // A closed pipe is the reader's business, never a panic on stderr.
            let _ = std::io::stdout().lock().write_all(&b);
            0
        }
        None => 1,
    }
}

/// The transcript of a pane's session: @cockpit_session, else `claude agents
/// --json` joined on the pane's process tree; the first `~/.claude/projects/*/<sid>.jsonl`.
pub fn file(pane: &str) -> Option<PathBuf> {
    if pane.is_empty() {
        return None;
    }
    let one = |args: &[&str]| super::tmux(args).unwrap_or_default().trim_end_matches('\n').to_string();
    let mut sid = one(&["show-option", "-pqv", "-t", pane, "@cockpit_session"]);
    if sid.is_empty() {
        let pp: i64 = one(&["display-message", "-p", "-t", pane, "#{pane_pid}"]).parse().ok()?;
        sid = agents().into_iter().find(|(pid, _)| descends(*pid, pp)).map(|(_, s)| s)?;
    }
    // A glob or a path never reaches the lookup below.
    if sid.is_empty() || !sid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return None;
    }
    let dir = PathBuf::from(format!("{}/.claude/projects", std::env::var("HOME").unwrap_or_default()));
    // The shell's glob: sorted, dot directories left out.
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    names.retain(|n| !n.as_bytes().starts_with(b"."));
    names.sort();
    names
        .into_iter()
        .map(|n| dir.join(n).join(format!("{sid}.jsonl")))
        .find(|p| p.is_file())
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
/// and C1 dropped, leading blanks trimmed, 160 chars.
fn clean(s: &str) -> String {
    let line = s.split('\n').find(|l| l.chars().any(|c| !c.is_whitespace())).unwrap_or("");
    let kept: String = line
        .chars()
        .map(|c| if c == '\t' { ' ' } else { c })
        .filter(|&c| c >= ' ' && !('\u{7f}'..='\u{9f}').contains(&c))
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
/// concrete argument first, the prose last. Shared with the board (data.rs) and
/// mirrored by `desc` in scripts/lib/transcript.sh; change all three together.
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
/// a non-blank string; else SendMessage's `to` as "to <to>"; else, for an
/// `mcp__*` tool, the first string-valued input field by key order. Nested
/// objects and arrays never count. None gives the bare tool name.
pub(crate) fn pick<'a>(name: &str, input: &'a Value) -> Option<(&'static str, &'a str)> {
    let ok = |s: &&str| s.chars().any(|c| !c.is_whitespace());
    DETAIL
        .iter()
        .find_map(|k| input[k].as_str().filter(ok).map(|s| ("", s)))
        .or_else(|| input["to"].as_str().filter(ok).map(|s| ("to ", s)))
        .or_else(|| {
            if !name.starts_with("mcp__") {
                return None;
            }
            let mut kv: Vec<_> = input.as_object()?.iter().filter_map(|(k, v)| Some((k, v.as_str()?))).collect();
            kv.sort_by_key(|(k, _)| *k);
            kv.into_iter().map(|(_, s)| s).find(ok).map(|s| ("", s))
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

/// `doing\t<label>\t<secs>`, `said\t<text>`, `asked\t<prompt>\t<secs>`, each left
/// out when unknown. Only the last tool_use counts, open parallel ones before it do not.
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

    #[test]
    fn a_label_never_ends_in_a_colon() {
        for l in CASES.lines() {
            let c: Value = serde_json::from_str(l).unwrap();
            let it = serde_json::json!({"name": c[0], "input": c[1]});
            assert!(!tlabel(&it).trim_end().ends_with(':'), "{l}");
        }
    }
}
