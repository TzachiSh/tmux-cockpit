//! Claude's reply, read back into what the panel shows. A hand-written tag
//! scanner: the reply regexes it replaces need no lookaround.
use super::envelope::{OutType, unesc};
use super::{boundary_at, skip_ws};
use crate::highlight::{lang, wrapper};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opt {
    pub label: String,
    pub recommended: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub note: String,
    /// Empty for an action outside the terminal, such as a tap in an app.
    pub cmd: String,
    pub lang: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Question {
        text: String,
        options: Vec<Opt>,
    },
    Prompt(String),
    Text(String),
    Format {
        lang: String,
        body: String,
    },
    /// `command` is set when it came from a `<command>` tag.
    Steps {
        steps: Vec<Step>,
        command: bool,
    },
    Hint(String),
    Untagged(String),
    /// Explain's answer, with its trailing `<prompt>` for the agent split out.
    Answer {
        text: String,
        ready: Option<String>,
    },
    /// `edit` or `need_file`: file edits are not ported yet.
    Unsupported(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub reply: Reply,
    /// The tag the reply opened first and never closed.
    pub cut_off: Option<&'static str>,
}

/// An opening tag: `<name>`, or `<name key="v">` when `key` is given.
struct Tag {
    start: usize,
    end: usize,
    attr: Option<String>,
}

/// `<name(\s+key="v")?\s*/?>`; `need` makes the attribute required and non-empty.
fn open_at(s: &str, i: usize, name: &str, key: &str, need: bool, slash: bool) -> Option<Tag> {
    let j = i + 1 + name.len();
    if !s[i..].starts_with('<') || !s[i + 1..].starts_with(name) {
        return None;
    }
    let ws = skip_ws(s, j);
    let mut k = ws;
    let mut attr = None;
    if !key.is_empty() && ws > j && s[ws..].starts_with(key) && s[ws + key.len()..].starts_with("=\"") {
        let v = ws + key.len() + 2;
        let q = s[v..].find('"')?;
        attr = Some(s[v..v + q].to_string());
        k = skip_ws(s, v + q + 1);
    }
    if need && attr.as_deref().is_none_or(str::is_empty) {
        return None;
    }
    if slash && s[k..].starts_with('/') {
        k += 1;
    }
    s[k..].starts_with('>').then_some(Tag {
        start: i,
        end: k + 1,
        attr,
    })
}

fn find_open(s: &str, from: usize, name: &str, key: &str, need: bool, slash: bool) -> Option<Tag> {
    let pat = format!("<{name}");
    s[from..]
        .match_indices(&pat)
        .find_map(|(i, _)| open_at(s, from + i, name, key, need, slash))
}

fn close(s: &str, from: usize, name: &str) -> Option<usize> {
    s[from..].find(&format!("</{name}>")).map(|i| from + i)
}

/// A tag's body up to its close, or to the end when `open_ended` and there is none.
/// Returns the body and the index just past the match.
fn body<'a>(s: &'a str, t: &Tag, name: &str, open_ended: bool) -> Option<(&'a str, usize)> {
    match close(s, t.end, name) {
        Some(c) => Some((&s[t.end..c], c + name.len() + 3)),
        None if open_ended => Some((&s[t.end..], s.len())),
        None => None,
    }
}

/// `>\n?(.*?)\n?\s*</x>`: one leading newline goes, trailing whitespace goes.
fn block(b: &str) -> &str {
    b.strip_prefix('\n').unwrap_or(b).trim_end()
}

fn norm(s: &str) -> String {
    unesc(&s.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The fence a reply is wrapped in whole ([`wrapper`]): its tag and body.
fn wrapped(o: &str) -> Option<(&str, &str)> {
    let lines: Vec<&str> = o.split('\n').collect();
    let f = wrapper(&lines)?;
    let last = lines.len() - 1;
    // past the opener's newline, up to the closer's
    let (from, to) = (lines[0].len() + 1, o.len() - lines[last].len() - 1);
    Some((f.tag, o.get(from..to).unwrap_or("")))
}

/// A reply wrapped whole in a fence is read from inside it.
fn unfence(o: &str) -> &str {
    wrapped(o).map_or(o, |(_, body)| body.trim())
}

/// An answer is only shown, never typed, so a reply that is one block in a
/// language the lexer knows keeps its fence and is coloured (#282). A bare or
/// `xml` fence is a wrapper and still goes.
fn code_fence(raw: &str) -> bool {
    wrapped(raw).is_some_and(|(tag, _)| lang(tag).is_some())
}

/// "Other" and its kin: the panel adds that option itself.
fn is_other(label: &str) -> bool {
    let low = label.to_lowercase();
    ["other", "something else", "none of these", "none of the above"]
        .iter()
        .any(|p| low.starts_with(p) && boundary_at(&low, p.len()))
}

fn options(o: &str) -> Vec<Opt> {
    let mut out = Vec::new();
    let mut used = false;
    let mut at = 0;
    while let Some(t) = find_open(o, at, "option", "recommended", false, false) {
        if t.attr.as_deref().is_some_and(|v| v != "true") {
            at = t.start + 1;
            continue;
        }
        let Some((b, next)) = body(o, &t, "option", false) else { break };
        at = next;
        let label = norm(b);
        if !label.is_empty() && !is_other(&label) {
            let rec = t.attr.is_some();
            out.push(Opt {
                label,
                recommended: rec && !used,
            });
            used |= rec;
        }
    }
    out.truncate(4);
    out
}

/// A closed `<question>`, and where its tag starts.
fn question_at(o: &str) -> Option<(usize, Reply)> {
    let t = find_open(o, 0, "question", "", false, false)?;
    let (b, _) = body(o, &t, "question", false)?;
    let q = Reply::Question {
        text: norm(b),
        options: options(o),
    };
    Some((t.start, q))
}

fn question(o: &str) -> Option<Reply> {
    question_at(o).map(|(_, q)| q)
}

/// `- x`, `* x`, `• x`, `1. x`, `2) x` -> `x`.
fn unbullet(l: &str) -> Option<&str> {
    let t = l.trim_start();
    let d = t.bytes().take_while(u8::is_ascii_digit).count();
    let r = if d > 0 {
        t[d..].strip_prefix(['.', ')'])
    } else {
        t.strip_prefix(['-', '*', '•'])
    }?;
    r.starts_with(char::is_whitespace).then(|| r.trim())
}

/// An older reply shape: a `QUESTION:` line, maybe after one line of preamble.
fn legacy_question(o: &str) -> Option<Reply> {
    let lines: Vec<&str> = o.lines().filter(|l| !l.trim().is_empty()).collect();
    let i = lines
        .iter()
        .take(2)
        .position(|l| l.trim().get(..9).is_some_and(|p| p.eq_ignore_ascii_case("question:")))?;
    let options = lines[i + 1..]
        .iter()
        .filter_map(|l| unbullet(l))
        .filter(|l| !is_other(l))
        .take(4)
        .map(|l| Opt {
            label: unesc(l),
            recommended: false,
        })
        .collect();
    Some(Reply::Question {
        text: unesc(lines[i].trim()[9..].trim()),
        options,
    })
}

/// The `QUESTION`-or-final reading Fix and Explain share. `None` is the final text.
fn reply_question(o: &str) -> Option<Reply> {
    question(o).or_else(|| legacy_question(o))
}

/// Fix: a question, or the corrected text to type.
pub fn fix(out: &str) -> Reply {
    let o = unfence(out.trim());
    reply_question(o).unwrap_or_else(|| Reply::Text(unesc(o)))
}

/// Explain: a question, or the answer with any `<prompt>` for the agent split out.
pub fn explain(out: &str) -> Reply {
    let o = unfence(out.trim());
    if let Some(q) = reply_question(o) {
        return q;
    }
    let Some(t) = find_open(o, 0, "prompt", "", false, false) else {
        return Reply::Answer {
            text: unesc(if code_fence(out.trim()) { out.trim() } else { o }),
            ready: None,
        };
    };
    let (b, end) = body(o, &t, "prompt", true).unwrap_or_default();
    let text = format!("{}{}", &o[..t.start], &o[end..]);
    let ready = Some(unesc(b.trim())).filter(|r| !r.is_empty());
    Reply::Answer {
        text: unesc(text.trim()),
        ready,
    }
}

/// The tags whose cut-off reply is worth a warning: they carry the result.
const CLOSERS: [&str; 7] = ["prompt", "text", "format", "edit", "steps", "command", "hint"];

/// The first tag in the reply, when it is one of [`CLOSERS`] and never closes.
pub fn cut_off(out: &str) -> Option<&'static str> {
    let o = out.trim();
    let r = &o[o.find('<')? + 1..];
    let n: usize = r.chars().take_while(|c| super::is_word(*c)).map(char::len_utf8).sum();
    if n == 0 || !r[n..].starts_with(|c: char| c == '>' || c.is_whitespace()) {
        return None;
    }
    let k = CLOSERS.iter().find(|k| **k == &r[..n])?;
    (!o.contains(&format!("</{k}>"))).then_some(*k)
}

const SHELL_LANGS: [&str; 7] = ["bash", "sh", "shell", "zsh", "fish", "console", "termux"];

/// Write: the earliest tag wins. `want` is the output type the user picked.
pub fn write(out: &str, want: OutType) -> Parsed {
    let raw = out.trim();
    let o = unfence(raw);
    let reply = write_reply(o, raw, want);
    Parsed {
        reply,
        cut_off: cut_off(out),
    }
}

fn write_reply(o: &str, raw: &str, want: OutType) -> Reply {
    let found = [
        find_open(o, 0, "prompt", "", false, false).map(|t| (t, "prompt")),
        find_open(o, 0, "text", "", false, false).map(|t| (t, "text")),
        find_open(o, 0, "format", "lang", false, false).map(|t| (t, "format")),
        find_open(o, 0, "edit", "path", true, false).map(|t| (t, "edit")),
        find_open(o, 0, "need_file", "path", true, true).map(|t| (t, "need_file")),
        find_open(o, 0, "steps", "", false, false).map(|t| (t, "steps")),
        find_open(o, 0, "command", "lang", false, false).map(|t| (t, "command")),
        find_open(o, 0, "hint", "", false, false).map(|t| (t, "hint")),
    ];
    let first = found.into_iter().flatten().min_by_key(|(t, _)| t.start);
    // A question counts only once closed, and only when no other tag comes first.
    let (t, kind) = match (first, question_at(o)) {
        (Some((t, _)), Some((at, q))) if at < t.start => return q,
        (None, Some((_, q))) => return q,
        (Some(f), _) => f,
        (None, None) => {
            return match legacy_question(o) {
                Some(q) => q,
                None if want == OutType::Hint && !o.is_empty() => Reply::Hint(unesc(o)),
                None => Reply::Untagged(unesc(o)),
            };
        }
    };
    let (b, _) = body(o, &t, kind, true).unwrap_or_default();
    let lang = || t.attr.as_deref().unwrap_or("").to_lowercase();
    match kind {
        "edit" => Reply::Unsupported("edit"),
        "need_file" => Reply::Unsupported("need_file"),
        "prompt" => Reply::Prompt(unesc(b.trim())),
        "text" if want == OutType::Hint && !b.trim().is_empty() => Reply::Hint(unesc(b.trim())),
        "text" => Reply::Text(unesc(b.trim())),
        "hint" => Reply::Hint(unesc(b.trim())),
        "format" => {
            let (lang, body) = (lang(), unesc(block(b)));
            if SHELL_LANGS.contains(&lang.as_str()) && !body.trim().is_empty() {
                return steps_or_raw(split_shell(&body, &lang), raw);
            }
            Reply::Format { lang, body }
        }
        "command" => {
            let lang = Some(lang()).filter(|l| !l.is_empty()).unwrap_or_else(|| "bash".into());
            let cmd = unesc(block(b)).trim_matches('\n').to_string();
            Reply::Steps {
                steps: vec![Step {
                    note: String::new(),
                    cmd,
                    lang,
                }],
                command: true,
            }
        }
        _ => steps_or_raw(parse_steps(b), raw),
    }
}

/// No usable step in a steps reply: show what came back rather than nothing.
fn steps_or_raw(steps: Vec<Step>, raw: &str) -> Reply {
    if steps.is_empty() {
        Reply::Untagged(raw.to_string())
    } else {
        Reply::Steps { steps, command: false }
    }
}

/// `Step 1.`, `2)` and the like: the panel numbers steps itself.
fn unnumber(s: &str) -> String {
    let t = s.trim_start();
    let t2 = match t.get(..4) {
        Some(p) if p.eq_ignore_ascii_case("step") => t[4..].trim_start(),
        _ => t,
    };
    let d = t2.bytes().take_while(u8::is_ascii_digit).count();
    match t2[d..].trim_start().strip_prefix(['.', ')', ':', '-']) {
        Some(r) if d > 0 => r.trim_start().to_string(),
        _ => s.to_string(),
    }
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        match rest[i + 1..].find('>') {
            Some(g) if g > 0 => rest = &rest[i + 2 + g..],
            _ => {
                out.push('<');
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `<step><note/><cmd lang/></step>`; a step may also end where the next opens.
pub fn parse_steps(b: &str) -> Vec<Step> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = b[at..].find("<step>").map(|i| at + i) {
        let from = i + 6;
        let closed = b[from..].find("</step>").map(|k| from + k);
        let next = b[from..].find("<step>").map(|k| from + k);
        let end = [closed, next].into_iter().flatten().min().unwrap_or(b.len());
        let inner = &b[from..end];
        at = if closed == Some(end) { end + 7 } else { end };

        let note = find_open(inner, 0, "note", "", false, false)
            .and_then(|t| body(inner, &t, "note", false))
            .map(|(n, _)| n.trim());
        let cmd = find_open(inner, 0, "cmd", "lang", false, false).and_then(|t| body(inner, &t, "cmd", false).map(|(c, _)| (t, c)));
        let note = match (note, &cmd) {
            (Some(n), _) => unesc(n),
            (None, Some(_)) => String::new(),
            (None, None) => unesc(&strip_tags(inner)).trim().to_string(),
        };
        if note.is_empty() && cmd.is_none() {
            continue;
        }
        let (cmd, lang) = match cmd {
            Some((t, c)) => {
                let lang = t.attr.filter(|l| !l.is_empty()).unwrap_or_else(|| "bash".into()).to_lowercase();
                (unesc(block(c)).trim_matches('\n').to_string(), lang)
            }
            None => (String::new(), String::new()),
        };
        out.push(Step {
            note: unnumber(&note),
            cmd,
            lang,
        });
    }
    out
}

/// Lines that only run as one unit: continuations, heredocs, blocks, functions.
fn compound(l: &str) -> bool {
    const KW: [&str; 13] = [
        "if", "for", "while", "until", "case", "function", "then", "do", "else", "elif", "fi", "done", "esac",
    ];
    let t = l.trim_start();
    let kw = KW.iter().any(|k| t.starts_with(k) && boundary_at(t, k.len()));
    let func = l.match_indices("()").any(|(i, _)| matches!(l[i + 2..].trim_start(), "" | "{"));
    l.ends_with('\\') || l.contains("<<") || kw || l.trim_end().ends_with(['{', '}']) || func
}

/// A shell block as steps: `#` comments note the next command, blank lines and
/// comments split steps, and a block with a compound line stays whole.
pub fn split_shell(body: &str, lang: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut note: Vec<String> = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    let flush = |block: &mut Vec<&str>, note: &mut Vec<String>, steps: &mut Vec<Step>| {
        if block.is_empty() {
            return;
        }
        let groups = if block.iter().any(|l| compound(l)) {
            vec![block.join("\n")]
        } else {
            block.iter().map(|l| l.to_string()).collect()
        };
        block.clear();
        for (k, g) in groups.into_iter().enumerate() {
            let n = if k == 0 { note.join(" ") } else { String::new() };
            steps.push(Step {
                note: n,
                cmd: g,
                lang: lang.to_string(),
            });
        }
        note.clear();
    };
    for line in body.split('\n') {
        let st = line.trim();
        if st.starts_with("#!") {
            continue;
        }
        if let Some(c) = st.strip_prefix('#') {
            flush(&mut block, &mut note, &mut steps);
            let c = c.trim_start_matches('#').trim();
            note.push(if note.is_empty() { unnumber(c) } else { c.to_string() });
        } else if st.is_empty() {
            flush(&mut block, &mut note, &mut steps);
        } else {
            block.push(line.trim_end());
        }
    }
    flush(&mut block, &mut note, &mut steps);
    if !note.is_empty() {
        steps.push(Step {
            note: note.join(" "),
            cmd: String::new(),
            lang: String::new(),
        });
    }
    steps
}

/// The files a `claude -p --output-format json --verbose` call read, in order and
/// each once; a Read the call refused is not one. `None` for any other output.
pub fn files_read(out: &str) -> Option<Vec<String>> {
    use serde_json::Value;
    let Ok(Value::Array(events)) = serde_json::from_str::<Value>(out.trim()) else {
        return None;
    };
    let content = |e: &Value, who: &str| {
        let c = (e["type"] == who).then(|| e["message"]["content"].as_array()).flatten();
        c.cloned().unwrap_or_default()
    };
    let refused: Vec<Value> = events
        .iter()
        .flat_map(|e| content(e, "user"))
        .filter(|r| r["type"] == "tool_result" && r["is_error"] == true)
        .map(|r| r["tool_use_id"].clone())
        .collect();
    let mut paths: Vec<String> = vec![];
    for u in events.iter().flat_map(|e| content(e, "assistant")) {
        if u["type"] != "tool_use" || u["name"] != "Read" || refused.contains(&u["id"]) {
            continue;
        }
        if let Some(p) = u["input"]["file_path"].as_str()
            && !paths.iter().any(|x| x == p)
        {
            paths.push(p.to_string());
        }
    }
    Some(paths)
}
