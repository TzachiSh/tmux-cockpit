//! The action, text, cmd and state files the panel hands its wrapper, and the
//! `--resume` state read back: the Steps after an action (version 1), and a
//! hidden panel (version 2, #278).
use super::app::{Action, App, InputKind, MAX_DRAFT, MODELS, OUT_CYCLE, Resume, Run, Shown, View, assumed};
use super::envelope::{Flow, OutType, Role, Turn};
use super::parse::{Opt, Query, Step};
use serde_json::{Map, Value, json};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const STATE_HEAD: &str = "cockpit-panel-state 1";
/// Far over any real reply: write.txt asks for a handful of steps.
const STATE_MAX: u64 = 64 * 1024;
const STATE_STEPS: usize = 64;
const HIDE_HEAD: &str = "cockpit-panel-state 2";
/// A hidden panel carries the draft, the reply and the conversation: an 8000
/// character draft is 32 KiB at most, so this is far over a real one.
pub const HIDE_MAX: u64 = 1024 * 1024;
/// A hidden panel older than this is dropped, not brought back: its screen and
/// its draft are a day stale.
pub const HIDE_AGE: u64 = 24 * 60 * 60;

/// Control characters out, `\t` and `\n` kept, `\r\n` folded: a stray ESC or
/// C1 byte pasted into a pane is a keystroke, not text. Bidi controls and the
/// line and paragraph separators (U+2028, U+2029) go too (review-510): they
/// reorder or break what the agent reads, and nothing shows them.
pub fn sanitize(s: &str) -> String {
    let kept = |c: char| c == '\t' || c == '\n' || !c.is_control();
    let shown = |c: char| !crate::pick::is_bidi(c) && !matches!(c, '\u{2028}' | '\u{2029}');
    s.replace("\r\n", "\n").chars().filter(|&c| kept(c) && shown(c)).collect()
}

/// `<path>.text` first, then the action line, so the wrapper never reads a
/// line whose text is not there yet. Both are created fresh, never followed.
/// `<path>.cmd` is the command the pane ran when it was captured, for the
/// wrapper to check it still runs before the paste lands.
pub fn write_action(path: &Path, pane: &str, cmd: &str, a: &Action) -> std::io::Result<()> {
    fresh(Path::new(&format!("{}.text", path.display())), &sanitize(&a.text))?;
    fresh(
        Path::new(&format!("{}.cmd", path.display())),
        &format!("{}\n", sanitize(cmd).replace('\n', " ")),
    )?;
    fresh(path, &format!("{} {pane}\n", a.verb))
}

/// `<path>.basket` (#365 child 5): the pane's basket as the send carried it, for
/// the wrapper to clear once the paste lands and only while the option still
/// holds this. Written fresh, never followed, mode 0600, as `.text` is.
pub fn write_basket(path: &Path, body: &str) -> std::io::Result<()> {
    fresh(Path::new(&format!("{}.basket", path.display())), body)
}

/// The longest `.expect` written: far over a tmux name, and the wrapper reads one line.
pub const EXPECT_MAX: usize = 1024;

/// `.expect`'s text: one line, every ASCII control character out, `\t` and
/// `\n` too, and nothing else, so its bytes are what a shell reader sees.
/// `None` when empty, which would check nothing, or over [`EXPECT_MAX`] bytes,
/// refused rather than cut, as a cut value could never match.
pub fn expect_body(s: &str) -> Option<String> {
    let line: String = s.chars().filter(|c| !c.is_ascii_control()).collect();
    (!line.is_empty() && line.len() <= EXPECT_MAX).then(|| format!("{line}\n"))
}

/// `<path>.expect`, the value the board showed for the target, for the wrapper
/// to check against tmux before it acts. Written before the action line and
/// under `.text`'s rules: created fresh, never followed, mode 0600.
pub fn write_expect(path: &Path, value: &str) -> std::io::Result<()> {
    let b = expect_body(value).ok_or(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "the expected value is empty or too long",
    ))?;
    fresh(Path::new(&format!("{}.expect", path.display())), &b)
}

fn fresh(p: &Path, body: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(p)?;
    f.write_all(body.as_bytes())
}

fn state_esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn state_unesc(s: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            _ => return Err("a bad escape".into()),
        }
    }
    Ok(out)
}

/// `<path>.state`, for the wrapper to hand back with `--resume`: written before
/// the action line, created fresh and never followed, like the other two.
pub fn write_state(path: &Path, pane: &str, cmd: &str, r: &Resume) -> std::io::Result<()> {
    let b = state_body(pane, cmd, r).ok_or(std::io::Error::new(std::io::ErrorKind::InvalidInput, "steps too long to keep"))?;
    fresh(Path::new(&format!("{}.state", path.display())), &b)
}

/// The state file's text, or `None` when [`read_state`] would refuse it.
pub fn state_body(pane: &str, cmd: &str, r: &Resume) -> Option<String> {
    if r.items.len() > STATE_STEPS {
        return None;
    }
    let mut b = format!(
        "{STATE_HEAD}\npane {pane}\ncmd {}\nlabel {}\nsel {}\nverb {}\n",
        state_esc(cmd),
        state_esc(&r.label),
        r.sel,
        r.verb
    );
    for (s, d) in r.items.iter().zip(&r.done) {
        let f = [&s.lang, &s.note, &s.cmd].map(|x| state_esc(x));
        b.push_str(&format!("step {}\t{}\t{}\t{}\n", u8::from(*d), f[0], f[1], f[2]));
    }
    (b.len() as u64 <= STATE_MAX).then_some(b)
}

/// A small regular file's text: no symlink, no FIFO, at most `max` bytes.
fn read_small(path: &Path, max: u64) -> Result<String, String> {
    use std::io::Read;
    let small = |m: std::fs::Metadata| m.is_file() && m.len() <= max;
    // checked before the open, which a FIFO would block, and again on the fd itself
    if !std::fs::symlink_metadata(path).is_ok_and(small) {
        return Err("not a small regular file".into());
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !f.metadata().is_ok_and(small) {
        return Err("not a small regular file".into());
    }
    let mut body = String::new();
    f.take(max).read_to_string(&mut body).map_err(|e| e.to_string())?;
    Ok(body)
}

/// The command and the steps a [`write_state`] file holds, refused unless it is
/// a small regular file of this version, for this very pane.
pub fn read_state(path: &Path, pane: &str) -> Result<(String, Resume), String> {
    let body = read_small(path, STATE_MAX)?;
    let Some(body) = body.strip_suffix('\n') else {
        return Err("cut short".into());
    };
    let mut lines = body.split('\n');
    if lines.next() != Some(STATE_HEAD) {
        return Err("not a version 1 state file".into());
    }
    let mut field = |key: &str| -> Result<String, String> {
        let l = lines.next().ok_or(format!("no {key} line"))?;
        let v = l
            .strip_prefix(key)
            .and_then(|v| v.strip_prefix(' '))
            .ok_or(format!("{key} expected"))?;
        state_unesc(v)
    };
    if field("pane")? != pane {
        return Err(format!("it is for another pane, not {pane}"));
    }
    let cmd = field("cmd")?;
    let label = field("label")?;
    let sel: usize = field("sel")?.parse().map_err(|_| "a bad sel".to_string())?;
    let verb = match field("verb")?.as_str() {
        "paste" => "paste",
        "send" => "send",
        _ => return Err("a bad verb".into()),
    };
    let (mut items, mut done) = (vec![], vec![]);
    for l in lines {
        let rest = l.strip_prefix("step ").ok_or("a line that is not a step")?;
        let f: Vec<&str> = rest.split('\t').collect();
        let [d, lang, note, cmd] = f[..] else {
            return Err("a step needs four fields".into());
        };
        done.push(match d {
            "0" => false,
            "1" => true,
            _ => return Err("a bad done mark".into()),
        });
        items.push(Step {
            lang: state_unesc(lang)?,
            note: state_unesc(note)?,
            cmd: state_unesc(cmd)?,
        });
    }
    if items.is_empty() || items.len() > STATE_STEPS || sel >= items.len() {
        return Err("no step to come back to".into());
    }
    Ok((
        cmd,
        Resume {
            label,
            items,
            sel,
            done,
            verb,
        },
    ))
}

/// A hidden panel, as [`read_hidden`] reads it back: what [`App::unhide`] restores.
pub struct Hidden {
    /// The captured command, kept for the wrapper's pane guard, as in a steps resume.
    pub cmd: String,
    pub view: View,
    pub busy_back: Option<View>,
    pub draft: String,
    pub cur: usize,
    pub out: OutType,
    pub model: usize,
    /// Explore first (#416); [`App::unhide`] drops it for a pane that is no longer local.
    pub explore: bool,
    pub run: Option<Run>,
}

/// The text of a hidden panel's state file, saved at `now` (unix seconds), or
/// `None` when it cannot be kept: a call is out (Busy), or it tops [`HIDE_MAX`].
/// A Confirm is kept as the view it was opened from: a resumed panel never
/// opens on a yes.
pub fn hide_body(a: &App, now: u64) -> Option<String> {
    let busy_back = match &a.busy_back {
        Some(v) => view_out(v)?,
        None => Value::Null,
    };
    let j = json!({
        "pane": a.pane,
        "saved": now,
        "cmd": a.cap.cmd,
        "draft": a.draft,
        "cur": a.cur,
        "out": a.out.as_str(),
        "model": a.model,
        "explore": a.explore,
        "view": view_out(&a.view)?,
        "busy_back": busy_back,
        "run": a.run.as_ref().map(run_out),
    });
    let b = format!("{HIDE_HEAD}\n{j}\n");
    (b.len() as u64 <= HIDE_MAX).then_some(b)
}

/// `<path>.state` for a hidden panel, under [`write_state`]'s rules; the
/// wrapper moves it to the pane's own path.
pub fn write_hidden(path: &Path, body: &str) -> std::io::Result<()> {
    fresh(Path::new(&format!("{}.state", path.display())), body)
}

fn view_out(v: &View) -> Option<Value> {
    Some(match v {
        View::Busy => return None,
        View::Confirm { back, .. } => return view_out(back),
        // the basket is the pane's, so a hidden panel comes back on the draft
        View::Basket { .. } => json!({"v": "compose"}),
        View::Home { sel } => json!({"v": "home", "sel": sel}),
        View::Compose => json!({"v": "compose"}),
        View::Question { sel, .. } => json!({"v": "question", "sel": sel}),
        View::Input { kind, text, cur, back } => {
            let name = match kind {
                InputKind::Other => "other",
                InputKind::Explain => "explain",
                InputKind::FollowUp => "followup",
                InputKind::Correct(_) => "correct",
            };
            let mut j = json!({"v": "input", "kind": name, "text": text, "cur": cur, "back": view_out(back)?});
            if let InputKind::Correct(i) = kind {
                j["which"] = json!(i);
            }
            j
        }
        View::Result { label, text, scroll } => json!({"v": "result", "label": label, "text": text, "scroll": scroll}),
        View::Steps { label, items, sel, done } => {
            let steps: Vec<Value> = items
                .iter()
                .zip(done)
                .map(|(s, d)| json!({"lang": s.lang, "note": s.note, "cmd": s.cmd, "done": d}))
                .collect();
            json!({"v": "steps", "label": label, "sel": sel, "steps": steps})
        }
        View::ExplainAsk { sel } => json!({"v": "explainask", "sel": sel}),
        View::Answer { scroll, .. } => json!({"v": "answer", "scroll": scroll}),
        View::Error { line, hint } => json!({"v": "error", "line": line, "hint": hint}),
    })
}

fn turns_out(ts: &[Turn]) -> Value {
    let role = |r: Role| if r == Role::User { "user" } else { "assistant" };
    ts.iter().map(|t| json!([role(t.role), t.text])).collect()
}

fn opts_out(os: &[Opt]) -> Value {
    os.iter().map(|o| json!([o.label, o.recommended])).collect()
}

fn ask_out(a: &super::ask::AskCode) -> String {
    json!({
        "at": a.at,
        "path": a.path,
        "repo": a.repo.to_string_lossy(),
        "code": a.code,
        "hidden": a.hidden,
        "cut": a.cut,
        "asked": [a.asked.0, a.asked.1],
        "shown": [a.shown.0, a.shown.1],
    })
    .to_string()
}

fn ask_in(s: &str) -> Result<super::ask::AskCode, String> {
    let v: Value = serde_json::from_str(s).map_err(|_| "a bad ask".to_string())?;
    let mut o = Obj::of(v)?;
    let pair = |v: Value| -> Result<(u32, u32), String> {
        let [a, b] = tuple(v)?;
        let n = |x: Value| x.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("a bad line");
        Ok((n(a)?, n(b)?))
    };
    let a = super::ask::AskCode {
        at: o.text("at")?,
        path: o.text("path")?,
        repo: o.text("repo")?.into(),
        code: o.text("code")?,
        hidden: o
            .take("hidden")?
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("a bad count")?,
        cut: o.take("cut")?.as_bool().ok_or("a bad cut")?,
        asked: pair(o.take("asked")?)?,
        shown: pair(o.take("shown")?)?,
    };
    o.done()?;
    // ask::load_in refuses such a name, so only a forged state file holds one (#936)
    if crate::pick::clean(&a.path) != a.path {
        return Err("a control or bidi character in the ask's path".into());
    }
    Ok(a)
}

fn run_out(r: &Run) -> Value {
    let (flow, draft, out) = match &r.flow {
        Flow::Write { draft, out } => ("write", Some(draft.clone()), Some(out.as_str())),
        Flow::Fix { draft } => ("fix", Some(draft.clone()), None),
        Flow::Explain => ("explain", None, None),
        // an ask's code and ref ride in `draft`, one JSON string (#365)
        Flow::Ask(a) => ("ask", Some(ask_out(a)), None),
    };
    let qstack: Vec<Value> = r
        .qstack
        .iter()
        .map(|s| json!([s.text, opts_out(&s.options), turns_out(&s.turns), s.at]))
        .collect();
    let ahead: Vec<Value> = r.ahead.iter().map(|q| json!([q.text, opts_out(&q.options)])).collect();
    let log: Vec<Value> = r.log.iter().map(|(you, t)| json!([you, t])).collect();
    json!({
        "flow": flow,
        "draft": draft,
        "out": out,
        "turns": turns_out(&r.turns),
        "rounds": r.rounds,
        "max": r.max,
        "qstack": qstack,
        "ahead": ahead,
        "question": r.question,
        "options": opts_out(&r.options),
        "log": log,
        "ready": r.ready,
        "model": r.model,
        "why": r.why,
        "pending_turn": r.pending_turn,
        "guessed_from": r.guessed_from,
    })
}

/// One JSON object read strictly: each key taken once, a missing one or one
/// left over refused, so a file this version did not write is never half read.
struct Obj(Map<String, Value>);

impl Obj {
    fn of(v: Value) -> Result<Obj, String> {
        match v {
            Value::Object(m) => Ok(Obj(m)),
            _ => Err("an object expected".into()),
        }
    }
    fn take(&mut self, k: &str) -> Result<Value, String> {
        self.0.remove(k).ok_or(format!("no {k}"))
    }
    fn text(&mut self, k: &str) -> Result<String, String> {
        text(self.take(k)?).map_err(|e| format!("{k}: {e}"))
    }
    fn opt_text(&mut self, k: &str) -> Result<Option<String>, String> {
        match self.take(k)? {
            Value::Null => Ok(None),
            v => text(v).map(Some).map_err(|e| format!("{k}: {e}")),
        }
    }
    fn num(&mut self, k: &str) -> Result<usize, String> {
        num(self.take(k)?).map_err(|e| format!("{k}: {e}"))
    }
    fn opt_num(&mut self, k: &str) -> Result<Option<usize>, String> {
        match self.take(k)? {
            Value::Null => Ok(None),
            v => num(v).map(Some).map_err(|e| format!("{k}: {e}")),
        }
    }
    fn flag(&mut self, k: &str) -> Result<bool, String> {
        self.take(k)?.as_bool().ok_or(format!("{k}: not true or false"))
    }
    /// A key a file from before it was added does not have: absent is false.
    fn opt_flag(&mut self, k: &str) -> Result<bool, String> {
        match self.0.remove(k) {
            None => Ok(false),
            Some(v) => v.as_bool().ok_or(format!("{k}: not true or false")),
        }
    }
    fn list(&mut self, k: &str) -> Result<Vec<Value>, String> {
        list(self.take(k)?).map_err(|e| format!("{k}: {e}"))
    }
    /// A list a file from before it was added does not have: absent is empty.
    fn opt_list(&mut self, k: &str) -> Result<Vec<Value>, String> {
        match self.0.remove(k) {
            None => Ok(vec![]),
            Some(v) => list(v).map_err(|e| format!("{k}: {e}")),
        }
    }
    fn done(self) -> Result<(), String> {
        match self.0.keys().next() {
            Some(k) => Err(format!("an unknown key {k}")),
            None => Ok(()),
        }
    }
}

fn text(v: Value) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s),
        _ => Err("not text".into()),
    }
}

fn num(v: Value) -> Result<usize, String> {
    v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("not a count".into())
}

fn list(v: Value) -> Result<Vec<Value>, String> {
    match v {
        Value::Array(a) => Ok(a),
        _ => Err("not a list".into()),
    }
}

/// A `[a, b]` or `[a, b, c]` list, exactly `N` long.
fn tuple<const N: usize>(v: Value) -> Result<[Value; N], String> {
    list(v)?.try_into().map_err(|_| format!("a list of {N} expected"))
}

/// A cursor on a char boundary of a box that holds at most MAX_DRAFT characters.
fn boxed(text: &str, cur: usize) -> Result<(), String> {
    if text.chars().count() > MAX_DRAFT {
        return Err("a box over its cap".into());
    }
    if !text.is_char_boundary(cur) {
        return Err("a cursor off the text".into());
    }
    Ok(())
}

fn out_in(s: &str) -> Result<OutType, String> {
    OUT_CYCLE
        .into_iter()
        .find(|o| o.as_str() == s)
        .ok_or(format!("an unknown type {s}"))
}

fn turns_in(v: Value) -> Result<Vec<Turn>, String> {
    list(v)?
        .into_iter()
        .map(|t| {
            let [r, t] = tuple(t)?;
            let role = match text(r)?.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                _ => return Err("an unknown role".into()),
            };
            Ok(Turn { role, text: text(t)? })
        })
        .collect()
}

fn opts_in(v: Value) -> Result<Vec<Opt>, String> {
    list(v)?
        .into_iter()
        .map(|o| {
            let [l, r] = tuple(o)?;
            let recommended = r.as_bool().ok_or("a bad recommended mark")?;
            Ok(Opt {
                label: text(l)?,
                recommended,
            })
        })
        .collect()
}

fn view_in(v: Value, nested: bool) -> Result<View, String> {
    let mut o = Obj::of(v)?;
    let v = match o.text("v")?.as_str() {
        "home" => View::Home { sel: o.num("sel")? },
        "compose" => View::Compose,
        "question" => View::Question {
            sel: o.num("sel")?,
            scroll: None,
        },
        // a box opens from a question, the asks or an answer: never from a box
        "input" if !nested => {
            let kind = match o.text("kind")?.as_str() {
                "other" => InputKind::Other,
                "explain" => InputKind::Explain,
                "followup" => InputKind::FollowUp,
                "correct" => InputKind::Correct(o.num("which")?),
                _ => return Err("an unknown box".into()),
            };
            let (text, cur) = (o.text("text")?, o.num("cur")?);
            boxed(&text, cur)?;
            let back = Box::new(view_in(o.take("back")?, true)?);
            // a correction is to one of the result's assumptions
            if let InputKind::Correct(i) = kind
                && !matches!(&*back, View::Result { text, .. } if i < assumed(text).len())
            {
                return Err("a correction with no assumption to correct".into());
            }
            View::Input { kind, text, cur, back }
        }
        "result" => View::Result {
            label: o.text("label")?,
            text: o.text("text")?,
            scroll: o.num("scroll")?,
        },
        "steps" => {
            let (label, sel) = (o.text("label")?, o.num("sel")?);
            let (mut items, mut done) = (vec![], vec![]);
            for s in o.list("steps")? {
                let mut s = Obj::of(s)?;
                done.push(s.flag("done")?);
                items.push(Step {
                    lang: s.text("lang")?,
                    note: s.text("note")?,
                    cmd: s.text("cmd")?,
                });
                s.done()?;
            }
            if items.is_empty() || items.len() > STATE_STEPS || sel >= items.len() {
                return Err("no step to come back to".into());
            }
            View::Steps { label, items, sel, done }
        }
        "explainask" => View::ExplainAsk { sel: o.num("sel")? },
        "answer" => View::Answer {
            scroll: o.num("scroll")?,
            armed: false,
        },
        "error" => View::Error {
            line: o.text("line")?,
            hint: o.opt_text("hint")?,
        },
        x => return Err(format!("an unknown view {x}")),
    };
    o.done()?;
    Ok(v)
}

fn run_in(v: Value) -> Result<Run, String> {
    let mut o = Obj::of(v)?;
    let flow = match (o.text("flow")?.as_str(), o.opt_text("draft")?, o.opt_text("out")?) {
        ("write", Some(draft), Some(out)) => Flow::Write { draft, out: out_in(&out)? },
        ("fix", Some(draft), None) => Flow::Fix { draft },
        ("explain", None, None) => Flow::Explain,
        ("ask", Some(a), None) => Flow::Ask(ask_in(&a)?),
        _ => return Err("a bad flow".into()),
    };
    let turns = turns_in(o.take("turns")?)?;
    let (rounds, max) = (o.num("rounds")?, o.num("max")?);
    let mut qstack: Vec<Shown> = vec![];
    for q in o.list("qstack")? {
        // a file from before batches has no index: each question was its own round
        let mut q = list(q)?;
        let at = if q.len() == 4 { num(q.pop().unwrap_or_default())? } else { 0 };
        let [q, opts, ts] = tuple(Value::Array(q))?;
        // a batch's later question follows the one before it
        if at > 0 && qstack.last().is_none_or(|s| s.at + 1 != at) {
            return Err("a question out of its batch".into());
        }
        let (text, options, turns) = (text(q)?, opts_in(opts)?, turns_in(ts)?);
        qstack.push(Shown { text, options, turns, at });
    }
    let mut ahead = vec![];
    for q in o.opt_list("ahead")? {
        let [q, opts] = tuple(q)?;
        ahead.push(Query {
            text: text(q)?,
            options: opts_in(opts)?,
        });
    }
    if !ahead.is_empty() && qstack.is_empty() {
        return Err("questions to come with none on screen".into());
    }
    let question = o.text("question")?;
    let options = opts_in(o.take("options")?)?;
    let mut log = vec![];
    for l in o.list("log")? {
        let [you, t] = tuple(l)?;
        log.push((you.as_bool().ok_or("a bad log mark")?, text(t)?));
    }
    let r = Run {
        flow,
        turns,
        rounds,
        max,
        qstack,
        ahead,
        question,
        options,
        log,
        ready: o.opt_text("ready")?,
        model: o.text("model")?,
        why: o.text("why")?,
        pending_turn: o.flag("pending_turn")?,
        guessed_from: o.opt_num("guessed_from")?,
    };
    o.done()?;
    if r.rounds > r.max {
        return Err("more rounds than the run allows".into());
    }
    Ok(r)
}

/// A [`hide_body`] file read back at `now`, refused unless it is a small regular
/// file of version 2 for this very pane, at most [`HIDE_AGE`] old, every key in
/// it known and every value in range.
pub fn read_hidden(path: &Path, pane: &str, now: u64) -> Result<Hidden, String> {
    let body = read_small(path, HIDE_MAX)?;
    let Some(body) = body.strip_suffix('\n') else {
        return Err("cut short".into());
    };
    let Some((HIDE_HEAD, j)) = body.split_once('\n') else {
        return Err("not a version 2 state file".into());
    };
    if j.contains('\n') {
        return Err("more than one body line".into());
    }
    let mut o = Obj::of(serde_json::from_str(j).map_err(|e| format!("not JSON: {e}"))?)?;
    if o.text("pane")? != pane {
        return Err(format!("it is for another pane, not {pane}"));
    }
    let saved = o.num("saved")? as u64;
    // a minute of slack for a clock set back
    if saved > now.saturating_add(60) {
        return Err("it is dated in the future".into());
    }
    if now.saturating_sub(saved) > HIDE_AGE {
        return Err(format!("it is older than {} h", HIDE_AGE / 3600));
    }
    let (cmd, draft, cur) = (o.text("cmd")?, o.text("draft")?, o.num("cur")?);
    boxed(&draft, cur)?;
    let out = out_in(&o.text("out")?)?;
    let model = o.num("model")?;
    if model >= MODELS.len() {
        return Err("an unknown model".into());
    }
    // a panel hidden before #416 kept no toggle: it comes back off
    let explore = o.opt_flag("explore")?;
    let view = view_in(o.take("view")?, false)?;
    let busy_back = match o.take("busy_back")? {
        Value::Null => None,
        v => Some(view_in(v, false)?),
    };
    let run = match o.take("run")? {
        Value::Null => None,
        v => Some(run_in(v)?),
    };
    o.done()?;
    // the question and the answer are drawn from the run
    match (&view, &run) {
        (View::Question { sel, .. }, Some(r)) if *sel < r.options.len().max(1) => {}
        (View::Answer { .. }, Some(_)) => {}
        (View::Question { .. } | View::Answer { .. }, _) => return Err("a view with no run to show".into()),
        // a correction is one more round of a write
        (
            View::Input {
                kind: InputKind::Correct(_),
                ..
            },
            r,
        ) if !r.as_ref().is_some_and(|r| matches!(r.flow, Flow::Write { .. })) => {
            return Err("a correction on a run that is not a write".into());
        }
        _ => {}
    }
    Ok(Hidden {
        cmd,
        view,
        busy_back,
        draft,
        cur,
        out,
        model,
        explore,
        run,
    })
}
