//! The write prompt's evaluation (#410): labelled cases through the panel's own
//! prompt, envelope, model route, `claude -p` call and reply parser, scored on
//! whether it asks when it should and carries the facts it must.
//!
//! It calls `claude` once per case and run, so it needs the network and costs
//! money: run it by hand, never from `make test`.
//!
//!     cargo run --example panel_eval -- [--model M] [--runs N] [--jobs J]
//!         [--only TEXT] [--cases FILE] [--out FILE.jsonl] [--check] [--no-explore]
//!         [--no-hermetic] [--judge] [--judge-model M] [--ledger FILE]
//!     cargo run --example panel_eval -- --compare A.jsonl B.jsonl [--allow-mixed]
//!     cargo run --example panel_eval -- --calibrate LABELS.jsonl
//!         [--judge-model M] [--split all|tune|holdout] [--jobs J] [--out FILE.jsonl]
//!
//! `--check` only reads and validates the cases. To compare a prompt edit, put
//! it at `$XDG_CONFIG_HOME/tmux-cockpit/prompts/write.txt` (the panel's own
//! override) and run twice with `--out`, then `--compare` the two files: it
//! reads them, makes no calls, and pairs the runs by case.
//!
//! A case with `explore: <fixture>` (evals/panel/explore.txt, #416) runs on a
//! temp copy of `evals/panel/fixtures/<fixture>` as the pane's folder, through
//! the panel's explore-first call; `--no-explore` runs it as a text call instead.
//!
//! Each call is hermetic by default (#598): `claude --setting-sources ""`, so
//! no user, project or local settings load, and with them no CLAUDE.md, no
//! plugin hooks and no user skills; and a fresh temp dir per call as its cwd.
//! Without it, a call under `$HOME` loads `~/.claude/CLAUDE.md` (as the user's
//! memory and again as an ancestor's `.claude/CLAUDE.md`) and the user's
//! SessionStart hooks, which differ per machine. `--no-hermetic` runs the call
//! as the panel does, in its cache dir with the user's settings. The summary's
//! `environment` line says which, with the claude version and the HOME and XDG
//! variables.
use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType, Role, Turn};
use cockpit_tui::panel::parse::{self, Reply, files_read, tool_calls};
use cockpit_tui::panel::request::{Call, Effort, Fail, Profile, Request};
use cockpit_tui::panel::route::route;
use cockpit_tui::refs::Ref;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "panel_eval/judge.rs"]
mod judge;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    Ask,
    Write,
    /// A question on target, or a reply with every `must`: both serve the owner.
    Either,
}

impl Expect {
    fn as_str(self) -> &'static str {
        match self {
            Expect::Ask => "ask",
            Expect::Write => "write",
            Expect::Either => "either",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Case {
    id: String,
    kind: String,
    name: String,
    cmd: String,
    path: String,
    out: OutType,
    /// For a reply that is not a question: the type it must be, a prompt for a
    /// fact that needs a search, so a hint written from memory fails.
    out_is: Option<OutType>,
    expect: Expect,
    /// For `ask`: the question or an option names one of these.
    ask_any: Vec<String>,
    /// For `write`: every one of these is in the reply.
    must: Vec<String>,
    /// Never in the reply, a question included: a subagent for a typo, say.
    must_not: Vec<String>,
    /// Written after the prompt it is run on, so it shows overfitting.
    holdout: bool,
    screen: String,
    /// The `<repo>` block the panel would send; "" for a window outside a repo.
    repo: String,
    request: String,
    /// The panel's earlier rounds, oldest first: its questions and the answers.
    turns: Vec<Turn>,
    /// A folder under `fixtures/`: the pane's path, read by an explore-first write.
    explore: Option<String>,
    /// The prompt basket (#365 child 5): paths from the pane's folder and their
    /// lines, `None` for a whole file; a leading `../` is a file outside it.
    refs: Vec<CaseRef>,
}

/// One `--- refs` line: a path from the pane's folder, and its lines.
type CaseRef = (String, Option<(u32, u32)>);

fn out_type(s: &str) -> Option<OutType> {
    Some(match s {
        "auto" => OutType::Auto,
        "prompt" => OutType::Prompt,
        "steps" => OutType::Steps,
        "command" => OutType::Command,
        "text" => OutType::Text,
        "format" => OutType::Format,
        "hint" => OutType::Hint,
        _ => return None,
    })
}

fn list(v: &str) -> Vec<String> {
    v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// `=== id`, then `key: value` lines, then `--- screen`, `--- repo` and
/// `--- request` bodies. `#` lines outside a body are comments.
fn parse_cases(text: &str) -> Result<Vec<Case>, String> {
    let mut cases = Vec::new();
    for (n, block) in text.split("\n=== ").enumerate() {
        let block = if n == 0 {
            match block.strip_prefix("=== ") {
                Some(b) => b,
                None => continue,
            }
        } else {
            block
        };
        let (id, rest) = block.split_once('\n').unwrap_or((block, ""));
        let id = id.trim().to_string();
        let err = |m: &str| format!("case {id}: {m}");
        let rest = format!("\n{rest}");
        let (head, bodies) = rest.split_once("\n--- ").unwrap_or((&rest, ""));
        let mut c = Case {
            id: id.clone(),
            kind: String::new(),
            name: String::new(),
            cmd: String::new(),
            path: String::new(),
            out: OutType::Auto,
            out_is: None,
            expect: Expect::Write,
            ask_any: vec![],
            must: vec![],
            must_not: vec![],
            holdout: false,
            screen: String::new(),
            repo: String::new(),
            request: String::new(),
            turns: vec![],
            explore: None,
            refs: vec![],
        };
        let mut expect = None;
        let mut seen: Vec<String> = vec![];
        for line in head.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (k, v) = line.split_once(':').ok_or_else(|| err(&format!("not key: value: {line}")))?;
            let v = v.trim();
            if seen.iter().any(|s| s == k.trim()) {
                return Err(err(&format!("{} given twice", k.trim())));
            }
            seen.push(k.trim().into());
            match k.trim() {
                "kind" => c.kind = v.into(),
                "window" => {
                    let f: Vec<&str> = v.split(" · ").collect();
                    let [name, cmd, path] = f[..] else {
                        return Err(err("window is name · cmd · path"));
                    };
                    (c.name, c.cmd, c.path) = (name.into(), cmd.into(), path.into());
                }
                "out" => c.out = out_type(v).ok_or_else(|| err(&format!("unknown out {v}")))?,
                "out_is" => {
                    let t = out_type(v).filter(|t| *t != OutType::Auto);
                    c.out_is = Some(t.ok_or_else(|| err(&format!("unknown out_is {v}")))?)
                }
                "expect" => {
                    expect = Some(match v {
                        "ask" => Expect::Ask,
                        "write" => Expect::Write,
                        "either" => Expect::Either,
                        _ => return Err(err("expect is ask, write or either")),
                    })
                }
                "ask_any" => c.ask_any = list(v),
                "must" => c.must = list(v),
                "must_not" => c.must_not = list(v),
                "holdout" => {
                    c.holdout = match v {
                        "yes" => true,
                        "no" => false,
                        _ => return Err(err("holdout is yes or no")),
                    }
                }
                "explore" => {
                    // one folder name: a path would copy something outside fixtures/
                    if v.is_empty() || v.starts_with('.') || v.contains('/') {
                        return Err(err("explore is a folder name under fixtures/"));
                    }
                    c.explore = Some(v.into())
                }
                "note" => {}
                k => return Err(err(&format!("unknown key {k}"))),
            }
        }
        c.expect = expect.ok_or_else(|| err("no expect"))?;
        let parts: Vec<&str> = bodies.split("\n--- ").filter(|b| !b.is_empty()).collect();
        for (i, body) in parts.iter().enumerate() {
            let (name, text) = body.split_once('\n').unwrap_or((body, ""));
            let mut lines: Vec<&str> = text.lines().collect();
            if i + 1 == parts.len() {
                strip_trailing_comments(&mut lines);
            }
            let text = lines.join("\n");
            let dup = |set: bool| {
                if set {
                    Err(err(&format!("{} given twice", name.trim())))
                } else {
                    Ok(())
                }
            };
            match name.trim() {
                "screen" => {
                    dup(!c.screen.is_empty())?;
                    c.screen = text
                }
                "repo" => {
                    dup(!c.repo.is_empty())?;
                    c.repo = text
                }
                "request" => {
                    dup(!c.request.is_empty())?;
                    c.request = text.trim().into()
                }
                "conversation" => {
                    dup(!c.turns.is_empty())?;
                    c.turns = turns(&text).map_err(|m| err(&m))?
                }
                "refs" => {
                    dup(!c.refs.is_empty())?;
                    c.refs = case_refs(&text).map_err(|m| err(&m))?
                }
                x => return Err(err(&format!("unknown section {x}"))),
            }
        }
        if c.request.is_empty() {
            return Err(err("no request"));
        }
        if c.kind.is_empty() || c.cmd.is_empty() {
            return Err(err("needs kind and window"));
        }
        if c.expect == Expect::Ask && !c.must.is_empty() {
            return Err(err("must is for write cases"));
        }
        if c.expect == Expect::Ask && c.out_is.is_some() {
            return Err(err("out_is is for write or either cases"));
        }
        if c.expect == Expect::Write && !c.ask_any.is_empty() {
            return Err(err("ask_any is for ask cases"));
        }
        if cases.iter().any(|o: &Case| o.id == c.id) {
            return Err(err("duplicate id"));
        }
        cases.push(c);
    }
    if cases.is_empty() {
        return Err("no cases".into());
    }
    Ok(cases)
}

/// `assistant: ...` and `user: ...` lines, one turn each, as the panel keeps them.
fn turns(text: &str) -> Result<Vec<Turn>, String> {
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (role, t) = match line.split_once(": ") {
            Some(("user", t)) => (Role::User, t),
            Some(("assistant", t)) => (Role::Assistant, t),
            _ => return Err(format!("a conversation line must start with user: or assistant: (got {line})")),
        };
        out.push(Turn {
            role,
            text: t.trim().into(),
        });
    }
    if out.is_empty() {
        return Err("empty conversation".into());
    }
    Ok(out)
}

/// `path`, `path:a` or `path:a-b` lines, one ref each: a path from the pane's
/// folder, plain names only, or one `../` first for a file outside it.
fn case_refs(text: &str) -> Result<Vec<CaseRef>, String> {
    let mut out = vec![];
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let bad = || format!("a ref is path, path:a or path:a-b from the pane's folder (got {line})");
        let (path, lines) = match line.rsplit_once(':') {
            Some((p, r)) => {
                let n = |t: &str| t.parse::<u32>().ok().filter(|n| *n > 0);
                let (a, b) = r.split_once('-').unwrap_or((r, r));
                match (n(a), n(b)) {
                    (Some(a), Some(b)) if a <= b => (p, Some((a, b))),
                    _ => return Err(bad()),
                }
            }
            None => (line, None),
        };
        let rest = path.strip_prefix("../").unwrap_or(path);
        let plain = |c: Component| matches!(c, Component::Normal(_));
        if rest.is_empty() || !Path::new(rest).components().all(plain) {
            return Err(bad());
        }
        out.push((path.to_string(), lines));
    }
    if out.is_empty() {
        return Err("empty refs".into());
    }
    Ok(out)
}

/// The comments that head the next case trail the last body: a `#` block that a
/// blank line sets off. A `#` line right after text is the body's own (`#412`, a
/// root prompt), and so is a body of `#` lines only.
fn strip_trailing_comments(lines: &mut Vec<&str>) {
    loop {
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        let at = lines.len() - lines.iter().rev().take_while(|l| l.starts_with('#')).count();
        if at == lines.len() || at == 0 || !lines[at - 1].trim().is_empty() {
            return;
        }
        lines.truncate(at);
    }
}

/// `needle` as a whole word, any case: `plan` is not in "explanation" nor `36`
/// in "360". `a|b` is any of them. An end that is not a letter or digit (`--json`,
/// `/code-review`) needs no boundary there.
fn has(hay: &str, needle: &str) -> bool {
    let hay = hay.to_lowercase();
    alternatives(needle).any(|n| !starts(&hay, &n).is_empty())
}

/// [`has`], but a match with a negator shortly before it in its clause does not
/// count: "do not write the files by hand" names "by hand" only to forbid it. A
/// `must_not` reads its words this way (#701).
fn has_unnegated(hay: &str, needle: &str) -> bool {
    let hay = hay.to_lowercase();
    alternatives(needle).any(|n| starts(&hay, &n).into_iter().any(|i| !negated(&hay[..i])))
}

/// The `|` alternatives of `needle`, trimmed and lowercased.
fn alternatives(needle: &str) -> impl Iterator<Item = String> + '_ {
    needle.split('|').map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty())
}

/// Each byte offset where `n` starts in `hay` as a whole word, both lowercase.
fn starts(hay: &str, n: &str) -> Vec<usize> {
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let (first, last) = (n.chars().next(), n.chars().last());
    let starts = !word(first);
    let ends = !word(last);
    // Every start, overlapping ones too: match_indices skips "ab ab" at 4 in "xab ab ab".
    (0..hay.len())
        .filter(|i| hay.is_char_boundary(*i) && hay[*i..].starts_with(n))
        .filter(|i| {
            let (before, after) = (hay[..*i].chars().last(), hay[*i + n.len()..].chars().next());
            (starts || !word(before)) && (ends || !word(after))
        })
        .collect()
}

/// Words a negator may sit before the match it cancels: "do not write the
/// golden files by hand" puts "not" five words before "by hand".
const NEGATION_WINDOW: usize = 6;

/// Whether the text just before a match negates it: a negator among the last
/// [`NEGATION_WINDOW`] words of its clause (a clause ends at `.,;:!?` or a line):
/// "if there is no flag, write them by hand" negates nothing.
fn negated(before: &str) -> bool {
    let clause = before.rsplit(|c| ".,;:!?\n".contains(c)).next().unwrap_or("");
    let words: Vec<String> = clause
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '\u{2019}'))
        .filter(|w| !w.is_empty())
        .map(|w| w.replace('\u{2019}', "'"))
        .collect();
    let from = words.len().saturating_sub(NEGATION_WINDOW);
    let single = ["not", "never", "don't", "no", "without"];
    let pair = |a: &str, b: &str| matches!((a, b), ("rather", "than") | ("instead", "of"));
    words[from..].iter().any(|w| single.contains(&w.as_str())) || words[from.saturating_sub(1)..].windows(2).any(|p| pair(&p[0], &p[1]))
}

/// What the panel shows for a reply, the text a `must` is looked for in; `None`
/// for a reply the panel refuses (a file edit) or that was cut off.
fn shown(raw: &str, out: OutType) -> Option<String> {
    let p = parse::write(raw, out);
    if p.cut_off.is_some() {
        return None;
    }
    Some(match p.reply {
        Reply::Question { text, options } => {
            let o: Vec<String> = options.into_iter().map(|o| o.label).collect();
            format!("{text}\n{}", o.join("\n"))
        }
        Reply::Prompt(t) | Reply::Text(t) | Reply::Hint(t) | Reply::Untagged(t) => t,
        Reply::Format { body, .. } => body,
        Reply::Steps { steps, .. } => steps
            .iter()
            .map(|s| format!("{}\n{}", s.note, s.cmd))
            .collect::<Vec<_>>()
            .join("\n"),
        Reply::Answer { text, ready } => format!("{text}\n{}", ready.unwrap_or_default()),
        Reply::Unsupported(_) => return None,
    })
}

/// The output type a reply is, for `out_is`; `None` for a question.
fn reply_type(r: &Reply) -> Option<&'static str> {
    Some(match r {
        Reply::Question { .. } => return None,
        Reply::Prompt(_) => "prompt",
        Reply::Text(_) => "text",
        Reply::Format { .. } => "format",
        Reply::Steps { command: false, .. } => "steps",
        Reply::Steps { command: true, .. } => "command",
        Reply::Hint(_) => "hint",
        Reply::Untagged(_) => "untagged",
        Reply::Answer { .. } => "answer",
        Reply::Unsupported(k) => k,
    })
}

fn out_name(t: OutType) -> &'static str {
    match t {
        OutType::Auto => "auto",
        OutType::Prompt => "prompt",
        OutType::Steps => "steps",
        OutType::Command => "command",
        OutType::Text => "text",
        OutType::Format => "format",
        OutType::Hint => "hint",
    }
}

#[derive(Debug, Default, PartialEq)]
struct Grade {
    asked: bool,
    question: String,
    /// For `ask`: the question named one of `ask_any` (true when there is none).
    on_target: bool,
    /// For `write`: the `must` facts the reply lacks.
    missing: Vec<String>,
    /// A reply of another type than the case's `out_is`: the type it was.
    wrong_type: Option<&'static str>,
    /// The `must_not` words the reply has.
    forbidden: Vec<String>,
    /// A reply the panel refuses or that was cut off: it fails every case.
    refused: bool,
    pass: bool,
}

fn grade(c: &Case, raw: &str) -> Grade {
    let reply = parse::write(raw, c.out).reply;
    let mut g = Grade::default();
    let Some(text) = shown(raw, c.out) else {
        g.refused = true;
        return g;
    };
    if let Reply::Question { text, options } = &reply {
        g.asked = true;
        let opts: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        g.question = if opts.is_empty() {
            text.clone()
        } else {
            format!("{text} [{}]", opts.join(" | "))
        };
        g.on_target = c.ask_any.is_empty() || c.ask_any.iter().any(|w| has(&g.question, w));
    }
    if !g.asked {
        g.missing = c.must.iter().filter(|m| !has(&text, m)).cloned().collect();
    }
    g.wrong_type = c.out_is.and_then(|want| reply_type(&reply).filter(|t| *t != out_name(want)));
    g.forbidden = c.must_not.iter().filter(|m| has_unnegated(&text, m)).cloned().collect();
    let asked_well = g.asked && g.on_target;
    let wrote_well = !g.asked && g.missing.is_empty() && g.wrong_type.is_none();
    g.pass = g.forbidden.is_empty()
        && match c.expect {
            Expect::Ask => asked_well,
            Expect::Write => wrote_well,
            Expect::Either => asked_well || wrote_well,
        };
    g
}

#[derive(Debug)]
struct Opts {
    cases: PathBuf,
    model: String,
    runs: usize,
    jobs: usize,
    only: Option<String>,
    out: Option<PathBuf>,
    check: bool,
    /// Two `--out` files: compared offline, no calls.
    compare: Option<(PathBuf, PathBuf)>,
    /// `--compare` pairs a case whose rows on one side carry two prompt hashes.
    allow_mixed: bool,
    /// The cost ledger; default `cost.tsv` next to `--out`.
    ledger: Option<PathBuf>,
    /// `explore:` cases run as the text call, on the same folder.
    no_explore: bool,
    /// No user settings or CLAUDE.md, and a fresh cwd per call (#598).
    hermetic: bool,
    /// Also grade each reply with the LLM judge (#419), beside the keyword checks.
    judge: bool,
    judge_model: String,
    /// A labelled file: run the judge on it and print its agreement with the owner.
    calibrate: Option<PathBuf>,
    split: judge::Split,
}

fn opts(args: impl IntoIterator<Item = String>) -> Result<Opts, String> {
    let mut o = Opts {
        cases: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/cases.txt"),
        model: "auto".into(),
        // One run is not enough to judge a change: a case can pass 3/3, then 2/3.
        runs: 3,
        jobs: 4,
        only: None,
        out: None,
        check: false,
        compare: None,
        allow_mixed: false,
        ledger: None,
        no_explore: false,
        hermetic: true,
        judge: false,
        judge_model: judge::DEFAULT_MODEL.into(),
        calibrate: None,
        split: judge::Split::All,
    };
    let mut a = args.into_iter();
    while let Some(k) = a.next() {
        let mut val = || a.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--cases" => o.cases = val()?.into(),
            "--model" => o.model = val()?,
            "--runs" => o.runs = val()?.parse().map_err(|e| format!("--runs: {e}"))?,
            "--jobs" => o.jobs = val()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--only" => o.only = Some(val()?),
            "--out" => o.out = Some(val()?.into()),
            "--check" => o.check = true,
            "--no-explore" => o.no_explore = true,
            "--no-hermetic" => o.hermetic = false,
            "--allow-mixed" => o.allow_mixed = true,
            "--ledger" => o.ledger = Some(val()?.into()),
            "--judge" => o.judge = true,
            "--judge-model" => o.judge_model = val()?,
            "--calibrate" => o.calibrate = Some(val()?.into()),
            "--split" => o.split = judge::Split::parse(&val()?)?,
            "--compare" => {
                let two = (a.next(), a.next());
                let (Some(x), Some(y)) = two else {
                    return Err("--compare needs two files".into());
                };
                o.compare = Some((x.into(), y.into()))
            }
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    o.jobs = o.jobs.max(1);
    o.runs = o.runs.max(1);
    Ok(o)
}

/// Set in a call's environment to the real claude: the rig, run as that call,
/// execs it with `--output-format json --verbose` first, so the panel's own
/// Request runs the call unchanged and the reply carries its cost and its reads.
const AS_CLAUDE: &str = "PANEL_EVAL_AS_CLAUDE";

/// Set beside [`AS_CLAUDE`] to `1`: the wrapped call is hermetic.
const HERMETIC: &str = "PANEL_EVAL_HERMETIC";

/// What a hermetic call adds: no setting source at all. `user` alone is not
/// enough: under `$HOME`, `project` finds `~/.claude/CLAUDE.md` as an ancestor's
/// `.claude/CLAUDE.md` (probed with claude 2.1.319, #598).
const HERMETIC_FLAGS: [&str; 2] = ["--setting-sources", ""];

/// What the rig runs as the call's claude: the real one, json first, then the
/// call's own argv, with the variables gone so claude's children never see them.
/// A hermetic call loads no setting source.
fn claude_command(real: &OsStr, args: impl IntoIterator<Item = OsString>, hermetic: bool) -> std::process::Command {
    let args: Vec<OsString> = args.into_iter().collect();
    let mut c = std::process::Command::new(real);
    // a json profile (read_only) asks for it itself: never twice
    if !args.iter().any(|a| a == "--output-format") {
        c.args(["--output-format", "json", "--verbose"]);
    }
    if hermetic && !args.iter().any(|a| a == HERMETIC_FLAGS[0]) {
        c.args(HERMETIC_FLAGS);
    }
    c.args(args).env_remove(AS_CLAUDE).env_remove(HERMETIC);
    c
}

/// What a finished call printed, for [`unwrap_json`] and `files_read`: a json
/// call's events (its answer is only their result's text), else its answer.
fn call_output(answer: Result<String, Fail>, raw: Option<&str>) -> Result<String, String> {
    match (answer, raw) {
        (_, Some(events)) => Ok(events.to_string()),
        (a, None) => a.map_err(|f| f.to_string()),
    }
}

/// The real claude to exec as, when the rig is run as a call's claude. Only for
/// the argv Request sends, which starts with `-p`: a variable leaked into the
/// owner's shell must not turn `--check`, `--compare` or a plain run into a call.
fn as_claude(var: Option<OsString>, first: Option<&OsStr>) -> Option<OsString> {
    var.filter(|_| first == Some(OsStr::new("-p")))
}

/// Whether the wrapped call is hermetic, from [`HERMETIC`]: only `1` is.
fn hermetic_wanted(var: Option<OsString>) -> bool {
    var.is_some_and(|v| v == "1")
}

/// Sets what the wrapped call reads, [`AS_CLAUDE`] and [`HERMETIC`], and
/// returns whether a call will now run hermetic, read back from the
/// environment: what the calls get, which the fingerprint reports.
///
/// # Safety
/// No other thread may read or write the environment meanwhile.
unsafe fn hand_off(real: &str, hermetic: bool) -> bool {
    // SAFETY: the caller's
    unsafe {
        std::env::set_var(AS_CLAUDE, real);
        if hermetic {
            std::env::set_var(HERMETIC, "1");
        } else {
            std::env::remove_var(HERMETIC);
        }
    }
    hermetic_wanted(std::env::var_os(HERMETIC))
}

/// The call the wrapper runs for `args`, hermetic as `var` says.
fn wrapped_command(real: &OsStr, args: impl IntoIterator<Item = OsString>, var: Option<OsString>) -> std::process::Command {
    claude_command(real, args, hermetic_wanted(var))
}

/// The call the wrapper runs for `args`, hermetic as [`HERMETIC`] in this
/// process's environment says: what [`exec_claude`] execs.
fn from_env(real: &OsStr, args: impl IntoIterator<Item = OsString>) -> std::process::Command {
    wrapped_command(real, args, std::env::var_os(HERMETIC))
}

/// The rig as the call's claude: exec, so the pid, its process group and its
/// death signal stay the ones Request set up.
fn exec_claude(real: &OsStr) -> ! {
    use std::os::unix::process::CommandExt;
    let e = from_env(real, std::env::args_os().skip(1)).exec();
    eprintln!("panel_eval: {}: {e}", real.to_string_lossy());
    std::process::exit(127);
}

struct Job {
    case: usize,
    run: usize,
    explore: bool,
    model: String,
    /// The system prompt's hash.
    system: u64,
    /// [`prompt_hash`] when the job started.
    prompt: String,
    req: Request,
    started: Instant,
}

fn main() {
    if let Some(real) = as_claude(std::env::var_os(AS_CLAUDE), std::env::args_os().nth(1).as_deref()) {
        exec_claude(&real);
    }
    let o = match opts(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("panel_eval: {e}");
            std::process::exit(2);
        }
    };
    if let Some((a, b)) = &o.compare {
        let read = |p: &PathBuf| {
            let name = p.display().to_string();
            std::fs::read_to_string(p)
                .map_err(|e| format!("{name}: {e}"))
                .and_then(|t| read_rows(&name, &t))
        };
        match read(a).and_then(|ra| Ok((ra, read(b)?))) {
            Ok((ra, rb)) => {
                let mixed: Vec<String> = [(a, &ra), (b, &rb)]
                    .iter()
                    .flat_map(|(p, rows)| mixed_prompts(rows).into_iter().map(move |id| format!("{id} in {}", p.display())))
                    .collect();
                if !mixed.is_empty() && !o.allow_mixed {
                    eprintln!(
                        "panel_eval: rows with two prompt hashes for one case: {}; --allow-mixed pairs them anyway",
                        mixed.join(", ")
                    );
                    std::process::exit(2);
                }
                print_compare(&[(a, &ra), (b, &rb)])
            }
            Err(e) => {
                eprintln!("panel_eval: {e}");
                std::process::exit(2);
            }
        }
        return;
    }
    if let Some(file) = &o.calibrate {
        if let Err(e) = judge::calibrate(file, &o.judge_model, o.split, o.jobs, o.out.as_deref()) {
            eprintln!("panel_eval: {e}");
            std::process::exit(2);
        }
        return;
    }
    let text = std::fs::read_to_string(&o.cases).unwrap_or_else(|e| {
        eprintln!("panel_eval: {}: {e}", o.cases.display());
        std::process::exit(2);
    });
    let mut cases = parse_cases(&text).unwrap_or_else(|e| {
        eprintln!("panel_eval: {e}");
        std::process::exit(2);
    });
    if let Some(w) = &o.only {
        cases.retain(|c| c.id.contains(w.as_str()) || c.kind == *w);
    }
    let count = |e: Expect| cases.iter().filter(|c| c.expect == e).count();
    let held = cases.iter().filter(|c| c.holdout).count();
    println!(
        "{} cases ({} ask, {} write, {} either; {held} holdout) from {}",
        cases.len(),
        count(Expect::Ask),
        count(Expect::Write),
        count(Expect::Either),
        o.cases.display()
    );
    if o.check {
        return;
    }
    let mut call = Call::from_env();
    let version = claude_version(&call.claude);
    let mut wrapped = false;
    // what the calls get, read back: false when the rig cannot wrap them
    let mut hermetic = false;
    match std::env::current_exe() {
        Ok(me) => {
            wrapped = true;
            // SAFETY: no other thread runs yet: no call has been spawned.
            hermetic = unsafe { hand_off(&call.claude, o.hermetic) };
            call.claude = me.display().to_string();
        }
        Err(e) if o.hermetic => {
            eprintln!("panel_eval: a hermetic run needs the rig's own binary ({e}); --no-hermetic runs without");
            std::process::exit(2);
        }
        Err(e) => eprintln!("panel_eval: no cost recorded, cannot find the rig's own binary: {e}"),
    }
    let mut log = o.out.as_ref().map(|p| {
        std::fs::File::create(p).unwrap_or_else(|e| {
            eprintln!("panel_eval: {}: {e}", p.display());
            std::process::exit(2);
        })
    });
    // A Staged is removed when it drops, so after the last exit: `exit` skips
    // drops, and so does a panic in a release build (`panic = "abort"`).
    let staged = Staged(stage(&cases).unwrap_or_else(|e| {
        eprintln!("panel_eval: {e}");
        std::process::exit(2);
    }));
    let root = std::env::temp_dir().join(format!("panel-eval-cwd-{}", std::process::id()));
    let cwds = match o.hermetic.then(|| std::fs::create_dir(&root)) {
        Some(Err(e)) => {
            eprintln!("panel_eval: {}: {e}", root.display());
            drop(staged);
            std::process::exit(2);
        }
        Some(Ok(())) => Staged(Some(root)),
        None => Staged(None),
    };
    let env = fingerprint(&version, hermetic, cwds.0.as_deref(), &call.dir, call.effort, |k| {
        std::env::var(k).ok()
    });
    let mut hashes: Vec<u64> = vec![];
    let mut todo: Vec<(usize, usize)> = (0..o.runs).flat_map(|r| (0..cases.len()).map(move |c| (c, r))).collect();
    todo.reverse();
    let mut live: Vec<Job> = vec![];
    // (case, ms, grade); no grade for a call that failed (a timeout, an API error).
    let mut done: Vec<(usize, u128, Option<Grade>)> = vec![];
    let mut costs: Vec<Option<f64>> = vec![];
    let mut reads: Vec<usize> = vec![];
    // with --judge: each row, and what the judge reads of it
    let mut rows: Vec<(serde_json::Value, judge::Input)> = vec![];
    // each prompt hash the jobs saw, in order: two means an override changed mid-run
    let mut prompt_sets: Vec<String> = vec![];
    while !todo.is_empty() || !live.is_empty() {
        while live.len() < o.jobs
            && let Some((ci, run)) = todo.pop()
        {
            let c = &cases[ci];
            let path = pane_path(c, staged.0.as_deref());
            let explore = c.explore.is_some() && !o.no_explore;
            let (system, prompt, profile) = panel_call(c, &path, explore);
            // per job, next to the call: an override edited mid-run shows in the rows
            let prompt_files = prompt_hash();
            if !prompt_sets.contains(&prompt_files) {
                prompt_sets.push(prompt_files.clone());
            }
            let flow = Flow::Write {
                draft: c.request.clone(),
                out: c.out,
            };
            let h = fnv(&system);
            if !hashes.contains(&h) {
                hashes.push(h);
            }
            // --model takes any name, so the route is the rig's, not the App's four
            let model = route(&flow, &o.model).model;
            let call = call_in(&call, cwds.0.as_deref(), ci, run).unwrap_or_else(|e| {
                eprintln!("panel_eval: {e}");
                std::process::exit(2);
            });
            // Spawned here, on the main thread: the child's death signal follows it.
            let req = Request::spawn_with(&call, &profile, &system, &prompt, &model);
            live.push(Job {
                case: ci,
                run,
                explore,
                model,
                system: h,
                prompt: prompt_files,
                req,
                started: Instant::now(),
            });
        }
        let mut i = 0;
        while i < live.len() {
            let Some(answer) = live[i].req.poll() else {
                i += 1;
                continue;
            };
            let j = live.swap_remove(i);
            let c = &cases[j.case];
            let ms = j.started.elapsed().as_millis();
            let out = call_output(answer, j.req.raw());
            // a text call has no tools: its rows say null, not "read none"
            let read = out.as_ref().ok().and_then(|o| files_read(o)).filter(|_| j.explore);
            let calls = panel_reads(out.as_deref().ok(), j.explore);
            let (raw, usage, err) = match out {
                Ok(out) => match unwrap_json(&out) {
                    Ok((raw, u)) => (raw, u, None),
                    Err(e) => (String::new(), Usage::default(), Some(e)),
                },
                // A non-zero exit puts the json's one line in the error; its message is why.
                Err(f) => {
                    let why = unwrap_json(&f).err().unwrap_or(f);
                    (String::new(), Usage::default(), Some(why))
                }
            };
            let g = err.is_none().then(|| grade(c, &raw));
            let mark = match (&err, &g) {
                (Some(_), _) => "ERR ",
                (_, Some(g)) if g.pass => "ok  ",
                _ => "FAIL",
            };
            let what = match (&err, &g) {
                (Some(e), _) => e.clone(),
                (_, Some(g)) if g.refused => "refused by the panel (a file edit) or cut off".into(),
                (_, Some(g)) if !g.forbidden.is_empty() => format!("has: {}", g.forbidden.join("; ")),
                (_, Some(g)) if g.asked => format!("asked: {}", g.question),
                (_, Some(g)) if g.wrong_type.is_some() => {
                    let want = c.out_is.map_or("", out_name);
                    format!("wrote a {}, want {want}", g.wrong_type.unwrap_or(""))
                }
                (_, Some(g)) if !g.missing.is_empty() => format!("wrote, missing: {}", g.missing.join("; ")),
                _ => "wrote".into(),
            };
            let want = c.expect.as_str();
            let what = match &read {
                Some(r) => format!("[read {}] {what}", r.len()),
                None => what,
            };
            println!("{mark} {:<24} {:<13} want {want:<6} {:>6} ms  {what}", c.id, c.kind, ms);
            if log.is_some() || o.judge {
                let row = serde_json::json!({
                    "id": c.id, "kind": c.kind, "expect": want, "run": j.run, "model": j.model,
                    "system": format!("{:016x}", j.system), "prompt": j.prompt,
                    "ms": ms as u64, "error": err,
                    "asked": g.as_ref().map(|g| g.asked), "pass": g.as_ref().map(|g| g.pass),
                    "question": g.as_ref().map(|g| g.question.clone()),
                    "missing": g.as_ref().map(|g| g.missing.clone()),
                    "forbidden": g.as_ref().map(|g| g.forbidden.clone()),
                    "wrong_type": g.as_ref().and_then(|g| g.wrong_type),
                    "cost_usd": usage.cost_usd,
                    "in_tokens": usage.in_tokens, "out_tokens": usage.out_tokens,
                    "refused": g.as_ref().map(|g| g.refused), "holdout": c.holdout, "reply": raw,
                    "explore": j.explore, "files_read": read, "reads": calls, "env": env.json(),
                });
                // with --judge, written once the judge has graded the reply
                match log.as_mut() {
                    Some(f) if !o.judge => {
                        let _ = writeln!(f, "{row}");
                    }
                    _ => rows.push((row, judge_input(c, &raw, calls))),
                }
            }
            done.push((j.case, ms, g));
            costs.push(usage.cost_usd);
            reads.extend(read.map(|r| r.len()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    summary(&cases, &done, &hashes, o.runs);
    let mut judged_nothing = false;
    let mut judge_usd = None;
    if o.judge {
        let (none, usd) = judge_rows(&mut rows, &o, &call.claude, wrapped, log.as_mut());
        (judged_nothing, judge_usd) = (none, Some(usd));
    }
    let ledger = ledger_path(&o);
    let label = o
        .out
        .as_ref()
        .and_then(|p| p.file_name())
        .map_or("-".into(), |n| n.to_string_lossy().into_owned());
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let total: f64 = costs.iter().flatten().sum();
    let mut lines = vec![ledger_row(secs, &label, cases.len(), o.runs, total, &prompt_sets.join(","))];
    if let Some(usd) = judge_usd {
        lines.push(ledger_row(
            secs,
            &format!("{label} judge"),
            cases.len(),
            o.runs,
            usd,
            &o.judge_model,
        ));
    }
    for line in lines {
        if let Err(e) = ledger_append(&ledger, &line) {
            eprintln!("panel_eval: ledger {}: {e}", ledger.display());
        }
    }
    println!("environment     {env}");
    if let Some(l) = cost_line(&costs, wrapped) {
        println!("cost            {l}");
    }
    if !reads.is_empty() {
        let n: usize = reads.iter().sum();
        println!(
            "files read      {n} over {} exploring calls, at most {}",
            reads.len(),
            reads.iter().max().unwrap_or(&0)
        );
    }
    if judged_nothing {
        eprintln!("panel_eval: --judge: every judge call failed");
        // exit skips drops: remove the temp dirs first
        drop((staged, cwds));
        std::process::exit(1);
    }
}

/// What an exploring call read and searched, from its output `out`; `None` for
/// a text call, which has no tools, and for output that is not the json events.
fn panel_reads(out: Option<&str>, explore: bool) -> Option<Vec<String>> {
    out.and_then(tool_calls).filter(|_| explore)
}

/// What the judge reads of case `c`'s reply `raw`, and what the panel read
/// before it: `None` when unknown, and for a case with refs or earlier rounds,
/// which the judge does not see.
fn judge_input(c: &Case, raw: &str, reads: Option<Vec<String>>) -> judge::Input {
    judge::Input {
        window: format!("{} · {} · {}", c.name, c.cmd, c.path),
        out: out_name(c.out).into(),
        screen: c.screen.clone(),
        repo: c.repo.clone(),
        request: c.request.clone(),
        reply: raw.into(),
        // the judge sees neither the basket nor the earlier rounds: rule 7
        // can weigh the reads only when it sees the rest of what the panel saw
        reads: reads.filter(|_| c.refs.is_empty() && c.turns.is_empty()),
    }
}

/// `--judge`: grades every answered row, prints each verdict and the summary
/// line, and writes the rows with the judge's fields added. A failed call
/// (`error` set) is not judged: it has no reply. Returns whether every judge
/// call failed.
fn judge_rows(
    rows: &mut [(serde_json::Value, judge::Input)],
    o: &Opts,
    claude: &str,
    wrapped: bool,
    mut log: Option<&mut std::fs::File>,
) -> (bool, f64) {
    // the rig wraps calls by naming itself as claude: the judge calls the real one
    let real = match wrapped {
        true => std::env::var(AS_CLAUDE).unwrap_or_else(|_| claude.into()),
        false => claude.into(),
    };
    let todo: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].0["error"].is_null()).collect();
    let inputs: Vec<judge::Input> = todo.iter().map(|&i| rows[i].1.clone()).collect();
    let judged = match judge::Scratch::new() {
        Ok(root) => judge::judge_all(&real, &o.judge_model, &inputs, o.jobs, &root.0),
        Err(e) => inputs
            .iter()
            .map(|_| judge::Judged {
                verdict: Err(e.clone()),
                cost_usd: None,
            })
            .collect(),
    };
    println!();
    let errors = judge_apply(rows, &todo, &judged, &o.judge_model);
    let pairs: Vec<(Option<bool>, Option<bool>)> = rows.iter().map(|(r, _)| (r["pass"].as_bool(), r["judge"].as_bool())).collect();
    if let Some(l) = judge_line(&pairs) {
        println!("judge           {l}  ({})", o.judge_model);
    }
    if errors > 0 {
        println!("judge errors    {errors} of {} calls gave no verdict", judged.len());
    }
    let cost: Vec<Option<f64>> = judged.iter().map(|j| j.cost_usd).collect();
    if let Some(l) = cost_line(&cost, true) {
        println!("judge cost      {l}");
    }
    if let Some(f) = log.as_mut() {
        for (row, _) in rows.iter() {
            let _ = writeln!(f, "{row}");
        }
    }
    (judge::every_failed(&judged), cost.iter().flatten().sum())
}

/// Puts `judged[n]` on row `todo[n]` and prints its line; returns the calls
/// that gave no verdict. Those rows keep `judge` null, and `pass` stays the
/// keyword checks' on every row.
fn judge_apply(rows: &mut [(serde_json::Value, judge::Input)], todo: &[usize], judged: &[judge::Judged], model: &str) -> usize {
    let mut errors = 0;
    for (&i, j) in todo.iter().zip(judged) {
        let (row, _) = &mut rows[i];
        let (pass, critique) = j.outcome();
        let mark = match pass {
            Some(true) => "ok  ",
            Some(false) => "FAIL",
            None => "ERR ",
        };
        errors += usize::from(pass.is_none());
        println!("judge {mark} {:<24} {critique}", row["id"].as_str().unwrap_or(""));
        judge::annotate(row, j, model);
    }
    errors
}

/// The judge's summary over `(keyword pass, judge pass)` per row: its pass
/// rate with a Wilson interval, and how often it agrees with the keyword
/// checks; rows either side left ungraded count in neither.
fn judge_line(pairs: &[(Option<bool>, Option<bool>)]) -> Option<String> {
    let judged: Vec<bool> = pairs.iter().filter_map(|(_, j)| *j).collect();
    if judged.is_empty() {
        return None;
    }
    let pass = Of(judged.iter().filter(|p| **p).count(), judged.len());
    let both: Vec<(bool, bool)> = pairs.iter().filter_map(|(k, j)| Some(((*k)?, (*j)?))).collect();
    let agree = Of(both.iter().filter(|(k, j)| k == j).count(), both.len());
    Some(format!("pass {}, agrees with the keyword checks {}", pct_ci(pass), pct(agree)))
}

/// A temp dir of the rig's own, removed on drop: the fixture copy, or a
/// hermetic run's root of call folders.
struct Staged(Option<PathBuf>);

impl Drop for Staged {
    fn drop(&mut self) {
        if let Some(d) = &self.0 {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// The call for run `run` of case `ci`: under a hermetic run's `root`, a fresh
/// folder of its own as the cwd, which then holds only the prompt file; else
/// `base`, the panel's cache dir.
fn call_in(base: &Call, root: Option<&Path>, ci: usize, run: usize) -> Result<Call, String> {
    let Some(root) = root else {
        return Ok(base.clone());
    };
    let dir = root.join(format!("case{ci}-run{run}"));
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Call { dir, ..base.clone() })
}

/// `claude --version`'s first line, or why there is none.
fn claude_version(claude: &str) -> String {
    match std::process::Command::new(claude).arg("--version").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().into(),
        Ok(o) => format!("unknown ({})", o.status),
        Err(e) => format!("unknown ({e})"),
    }
}

/// What a call saw besides its prompt (#598): the same system prompt hash can
/// score apart on two machines when these differ.
#[derive(Debug, PartialEq)]
struct Fingerprint {
    claude: String,
    /// The calls' cwd: a hermetic run's root, each call a fresh folder under it.
    cwd: String,
    hermetic: bool,
    /// The setting sources the calls load: `None` is claude's default, all of them.
    setting_sources: Option<String>,
    /// The panel's `--effort`, from COCKPIT_PANEL_EFFORT (#623).
    effort: &'static str,
    /// HOME, XDG_CONFIG_HOME, XDG_CACHE_HOME and CLAUDE_CONFIG_DIR, `None` unset.
    vars: Vec<(&'static str, Option<String>)>,
}

const ENV_VARS: [&str; 4] = ["HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "CLAUDE_CONFIG_DIR"];

/// `hermetic` is what the calls get (see [`hand_off`]); `root` where they run.
/// A plain call loads what the panel's own argv names (#623).
fn fingerprint(
    claude: &str,
    hermetic: bool,
    root: Option<&Path>,
    dir: &Path,
    effort: Effort,
    var: impl Fn(&str) -> Option<String>,
) -> Fingerprint {
    let panel = Profile::TEXT.argv(effort);
    let at = panel.iter().position(|a| a == HERMETIC_FLAGS[0]);
    let panel_sources = at.and_then(|i| panel.get(i + 1));
    let cwd = match root {
        Some(r) => format!("{}/<case>-<run>", r.display()),
        None => dir.display().to_string(),
    };
    Fingerprint {
        claude: claude.into(),
        cwd,
        hermetic,
        setting_sources: if hermetic {
            Some(HERMETIC_FLAGS[1].into())
        } else {
            panel_sources.cloned()
        },
        effort: effort.as_str(),
        vars: ENV_VARS.iter().map(|k| (*k, var(k))).collect(),
    }
}

impl Fingerprint {
    fn json(&self) -> serde_json::Value {
        let vars: serde_json::Map<String, serde_json::Value> =
            self.vars.iter().map(|(k, v)| (k.to_string(), serde_json::json!(v))).collect();
        serde_json::json!({
            "claude": self.claude, "cwd": self.cwd, "hermetic": self.hermetic,
            "setting_sources": self.setting_sources, "effort": self.effort, "vars": vars,
        })
    }
}

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let memory = if self.setting_sources.as_deref() == Some("") {
            "user memory and settings off (--setting-sources \"\")"
        } else {
            "user memory and settings loaded"
        };
        write!(f, "claude {}; {memory}; effort {}; cwd {}", self.claude, self.effort, self.cwd)?;
        for (k, v) in &self.vars {
            write!(f, "; {k}={}", v.as_deref().unwrap_or("(unset)"))?;
        }
        Ok(())
    }
}

/// The system prompt, the user prompt and the profile the panel itself sends for
/// case `c` on `path`: an App in Compose, its ^S, then its own picks.
fn panel_call(c: &Case, path: &str, explore: bool) -> (String, String, Profile) {
    let cap = Capture {
        name: c.name.clone(),
        cmd: c.cmd.clone(),
        path: path.into(),
        screen: c.screen.clone(),
        repo: c.repo.as_str().into(),
    };
    let mut a = App::new("%0", cap);
    (a.view, a.out, a.explore) = (View::Compose, c.out, explore);
    // the basket as main reads it, the claude check by the window's command alone
    a.claude = cockpit_tui::basket::claude_cmd(&c.cmd);
    for (f, lines) in &c.refs {
        let r = Ref::new(Path::new(path).join(f), *lines, None);
        a.basket
            .push(r.unwrap_or_else(|e| panic!("case {}: ref {f}: {e} (not staged?)", c.id)));
    }
    (a.draft, a.cur) = (c.request.clone(), c.request.len());
    a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    if let Some(r) = a.run.as_mut() {
        // each question the panel asked used one of its rounds
        r.rounds = c.turns.iter().filter(|t| t.role == Role::Assistant).count();
        r.turns = c.turns.clone();
    }
    let (system, prompt, _) = a.request_parts().expect("^S on a draft starts a write");
    let profile = a.run.as_ref().map_or(Profile::TEXT, |r| a.profile_for(&r.flow));
    (system, prompt, profile)
}

/// The fixtures the `explore:` cases name, copied to a fresh temp dir of the
/// rig's own, so a model reading around finds nothing of this repo's; and the
/// files of each case's refs (#365 child 5), which the basket needs to exist.
fn stage(cases: &[Case]) -> Result<Option<PathBuf>, String> {
    let names: Vec<&str> = cases.iter().filter_map(|c| c.explore.as_deref()).collect();
    if names.is_empty() && cases.iter().all(|c| c.refs.is_empty()) {
        return Ok(None);
    }
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("panel-eval-fixtures-{}-{n}", std::process::id()));
    let from = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/fixtures");
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // removed on an error below, kept once all of it is staged
    let mut kept = Staged(Some(dir.clone()));
    for n in names {
        if !dir.join(n).exists() {
            copy_tree(&from.join(n), &dir.join(n)).map_err(|e| format!("fixture {n}: {e}"))?;
        }
    }
    for c in cases.iter().filter(|c| !c.refs.is_empty()) {
        stage_refs(c, Path::new(&pane_path(c, Some(&dir))))?;
    }
    Ok(kept.0.take())
}

/// A refs case's files under `pane`, a few lines each. A fixture is shared by
/// every case that names it, so a ref inside one is a file it has, never made.
fn stage_refs(c: &Case, pane: &Path) -> Result<(), String> {
    let err = |f: &str, e: std::io::Error| format!("case {}: ref {f}: {e}", c.id);
    std::fs::create_dir_all(pane).map_err(|e| err(".", e))?;
    for (f, lines) in &c.refs {
        let p = pane.join(f);
        if c.explore.is_some() && !f.starts_with("../") {
            if !p.is_file() {
                return Err(format!("case {}: ref {f} is not in its fixture", c.id));
            }
            continue;
        }
        let n = lines.map_or(3, |(_, b)| b);
        let body: String = (1..=n).map(|i| format!("// line {i}\n")).collect();
        std::fs::create_dir_all(p.parent().unwrap_or(pane)).map_err(|e| err(f, e))?;
        std::fs::write(&p, body).map_err(|e| err(f, e))?;
    }
    Ok(())
}

/// The pane's folder for case `c`: its fixture copy when it explores; for a
/// case with refs, a folder of its own under the stage, named as its window's
/// path ends, so the refs are files; else the window's path as written.
fn pane_path(c: &Case, staged: Option<&Path>) -> String {
    match (&c.explore, staged) {
        (Some(f), Some(d)) => d.join(f).display().to_string(),
        (None, Some(d)) if !c.refs.is_empty() => {
            let name = Path::new(&c.path)
                .file_name()
                .map_or("pane".into(), |n| n.to_string_lossy().into_owned());
            d.join(format!("case-{}", c.id)).join(name).display().to_string()
        }
        _ => c.path.clone(),
    }
}

/// Files and folders only: a fixture has no links.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let t = e.file_type()?;
        if t.is_dir() {
            copy_tree(&e.path(), &to.join(e.file_name()))?;
        } else if t.is_file() {
            std::fs::copy(e.path(), to.join(e.file_name()))?;
        }
    }
    Ok(())
}

/// The panel's config directory, as `panel::xdg` finds it.
fn config_home() -> PathBuf {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if v.starts_with('/') => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"),
    }
}

/// One run's row of an `--out` file: what `--compare` reads.
#[derive(Clone, Debug, Default, PartialEq)]
struct Row {
    id: String,
    expect: String,
    /// `None` for a failed call.
    pass: Option<bool>,
    /// The system prompt's hash; "" in a file written before it was recorded.
    system: String,
    /// The hash of write.txt and its examples as loaded; "" in an older file.
    prompt: String,
    cost_usd: Option<f64>,
    ms: Option<u64>,
    /// The files an exploring call read; `None` for a text call or an older file.
    files_read: Option<usize>,
    /// The row's `env` (#598) less its cwd, which names a pid; `None` in a file
    /// written before it was recorded.
    env: Option<serde_json::Value>,
}

/// The rows of an `--out` file; `name` heads each error.
fn read_rows(name: &str, text: &str) -> Result<Vec<Row>, String> {
    let mut rows = vec![];
    for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let err = |m: &str| format!("{name}:{}: {m}", n + 1);
        let v: serde_json::Value = serde_json::from_str(line).map_err(|e| err(&e.to_string()))?;
        let id = v["id"].as_str().ok_or_else(|| err("no id"))?;
        let pass = match &v["pass"] {
            serde_json::Value::Null => None,
            p => Some(p.as_bool().ok_or_else(|| err("pass is true, false or null"))?),
        };
        let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        rows.push(Row {
            id: id.into(),
            expect: text("expect"),
            pass,
            system: text("system"),
            prompt: text("prompt"),
            cost_usd: v["cost_usd"].as_f64(),
            ms: v["ms"].as_u64(),
            files_read: v["files_read"].as_array().map(Vec::len),
            env: v["env"].as_object().map(|e| {
                let mut e = e.clone();
                e.remove("cwd");
                serde_json::Value::Object(e)
            }),
        });
    }
    if rows.is_empty() {
        return Err(format!("{name}: no rows"));
    }
    Ok(rows)
}

/// One case side by side: (passed, graded, failed calls) in A and in B, `None`
/// where that file has no row for it.
#[derive(Debug, PartialEq)]
struct Cmp {
    id: String,
    expect: String,
    a: Option<(usize, usize, usize)>,
    b: Option<(usize, usize, usize)>,
}

impl Cmp {
    /// Up or down by pass rate over graded runs; `None` when it held, or a side has no graded run.
    fn moved(&self) -> Option<std::cmp::Ordering> {
        let ((ka, na, _), (kb, nb, _)) = (self.a?, self.b?);
        if na == 0 || nb == 0 {
            return None;
        }
        // kb/nb against ka/na, crosswise so 3/3 and 2/2 are equal.
        Some((kb * na).cmp(&(ka * nb))).filter(|o| o.is_ne())
    }
}

/// The distinct system prompt hashes of a file's rows, in first-seen order.
fn prompts_of(rows: &[Row]) -> Vec<&str> {
    let mut hs: Vec<&str> = vec![];
    for h in rows.iter().map(|r| r.system.as_str()).filter(|h| !h.is_empty()) {
        if !hs.contains(&h) {
            hs.push(h);
        }
    }
    hs
}

/// The distinct environments of a file's rows, in first-seen order; `None` for
/// rows that recorded none.
fn envs_of(rows: &[Row]) -> Vec<Option<&serde_json::Value>> {
    let mut es = vec![];
    for e in rows.iter().map(|r| r.env.as_ref()) {
        if !es.contains(&e) {
            es.push(e);
        }
    }
    es
}

/// The two sides ran in different environments: worth a warning, as cost and
/// pass rates may differ for that alone.
fn envs_differ(a: &[Row], b: &[Row]) -> bool {
    envs_of(a) != envs_of(b)
}

/// A side's (passed, graded, failed calls) for one case.
type Side = (usize, usize, usize);

/// pass^all of A and of B over the same cases: those both sides graded on every run.
fn paired_pass_all(cmp: &[Cmp]) -> (Of, Of) {
    let whole = |s: Option<(usize, usize, usize)>| s.filter(|(_, n, err)| *n > 0 && *err == 0);
    let both: Vec<_> = cmp.iter().filter_map(|c| Some((whole(c.a)?, whole(c.b)?))).collect();
    let side = |pick: fn(&(Side, Side)) -> Side| pass_all(&both.iter().map(pick).collect::<Vec<_>>());
    (side(|p| p.0), side(|p| p.1))
}

/// Per case, in A's order and then B's own.
fn compare(a: &[Row], b: &[Row]) -> Vec<Cmp> {
    let side = |rows: &[Row], id: &str| {
        let mine: Vec<&Row> = rows.iter().filter(|r| r.id == id).collect();
        let err = mine.iter().filter(|r| r.pass.is_none()).count();
        let ok = mine.iter().filter(|r| r.pass == Some(true)).count();
        (!mine.is_empty()).then_some((ok, mine.len() - err, err))
    };
    let mut out: Vec<Cmp> = vec![];
    for r in a.iter().chain(b) {
        if !out.iter().any(|c| c.id == r.id) {
            out.push(Cmp {
                id: r.id.clone(),
                expect: r.expect.clone(),
                a: side(a, &r.id),
                b: side(b, &r.id),
            });
        }
    }
    out
}

/// Two `--out` files side by side: per-case pass counts, the cases that moved,
/// and pass^all for each. Paired by case id, so a case's own flakiness shows.
fn print_compare(sides: &[(&PathBuf, &[Row]); 2]) {
    let cmp = compare(sides[0].1, sides[1].1);
    let cell = |s: Option<(usize, usize, usize)>| match s {
        None => "-".to_string(),
        Some((ok, n, 0)) => format!("{ok}/{n}"),
        Some((ok, n, err)) => format!("{ok}/{n} +{err}err"),
    };
    println!("{:<24} {:<6} {:<10} {:<10} moved", "case", "want", "A", "B");
    for c in &cmp {
        let m = match c.moved() {
            Some(std::cmp::Ordering::Greater) => "up",
            Some(_) => "down",
            None => "",
        };
        println!("{:<24} {:<6} {:<10} {:<10} {m}", c.id, c.expect, cell(c.a), cell(c.b));
    }
    println!();
    for (tag, (p, rows)) in ["A", "B"].iter().zip(sides) {
        let hs = prompts_of(rows);
        let per: Vec<_> = cmp.iter().filter_map(|c| if *tag == "A" { c.a } else { c.b }).collect();
        println!("{tag}  {} ({} rows; system prompt {})", p.display(), rows.len(), hs.join(" "));
        println!("   pass^all  {}  unpaired: this side's own cases", pct_ci(pass_all(&per)));
        let costs: Vec<Option<f64>> = rows.iter().map(|r| r.cost_usd).collect();
        if let Some(l) = cost_line(&costs, false) {
            println!("   cost      {l}");
        }
        if let Some(l) = latency_line(rows) {
            println!("   latency   {l}");
        }
        if let Some(l) = reads_line(rows) {
            println!("   read      {l}");
        }
        for e in envs_of(rows) {
            println!("   env       {}", e.map_or("not recorded (before #598)".into(), |e| e.to_string()));
        }
    }
    if envs_differ(sides[0].1, sides[1].1) {
        println!("warning: A and B ran in different environments (env above): cost and pass rates may differ for that alone");
    }
    let (pa, pb) = paired_pass_all(&cmp);
    println!(
        "paired    pass^all over the {} cases both graded in full: A {}  B {}",
        pa.1,
        pct(pa),
        pct(pb)
    );
    let names = |o: std::cmp::Ordering| {
        cmp.iter()
            .filter(|c| c.moved() == Some(o))
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>()
    };
    let (up, down) = (names(std::cmp::Ordering::Greater), names(std::cmp::Ordering::Less));
    println!("moved     {} up: {}", up.len(), up.join(", "));
    println!("          {} down: {}", down.len(), down.join(", "));
    let only = |f: &dyn Fn(&Cmp) -> bool| {
        let ids: Vec<&str> = cmp.iter().filter(|c| f(c)).map(|c| c.id.as_str()).collect();
        ids.join(", ")
    };
    let (oa, ob) = (only(&|c| c.b.is_none()), only(&|c| c.a.is_none()));
    if !oa.is_empty() || !ob.is_empty() {
        println!("only in A: {oa}; only in B: {ob}");
    }
}

/// What a call cost, from `claude -p --output-format json`; all `None` for a
/// reply that is not that json (a stand-in claude, an older one).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Usage {
    cost_usd: Option<f64>,
    /// Input tokens, cache writes and reads included.
    in_tokens: Option<u64>,
    out_tokens: Option<u64>,
}

/// The reply text inside `claude -p --output-format json`, and its usage; an
/// array (`--verbose`) gives its last result. Text that is not json is the reply
/// as it stands (a stand-in claude). Json with no string result, an empty one or
/// one marked `is_error` is a failed call: grading it would find tags in the json.
fn unwrap_json(out: &str) -> Result<(String, Usage), String> {
    use serde_json::Value;
    let v = match serde_json::from_str::<Value>(out.trim()) {
        Ok(Value::Array(a)) => a.into_iter().rev().find(|e| e["type"] == "result").unwrap_or_default(),
        Ok(v @ Value::Object(_)) => v,
        _ => return Ok((out.to_string(), Usage::default())),
    };
    let result = v["result"].as_str();
    let subtype = v["subtype"].as_str();
    if v["is_error"] == true {
        return Err(result.or(subtype).unwrap_or("an error result").into());
    }
    let text = match result {
        Some(t) if t.trim().is_empty() => return Err("no answer".into()),
        Some(t) => t,
        None if v["type"] == "result" => return Err(subtype.unwrap_or("not a claude json result").into()),
        None => return Err("not a claude json result".into()),
    };
    let u = &v["usage"];
    let ins = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"];
    let usage = Usage {
        cost_usd: v["total_cost_usd"].as_f64(),
        in_tokens: u["input_tokens"].as_u64().map(|_| ins.iter().filter_map(|k| u[k].as_u64()).sum()),
        out_tokens: u["output_tokens"].as_u64(),
    };
    Ok((text.to_string(), usage))
}

/// Mean and median call time over the rows that recorded one.
fn latency_line(rows: &[Row]) -> Option<String> {
    let mut ms: Vec<u64> = rows.iter().filter_map(|r| r.ms).collect();
    if ms.is_empty() {
        return None;
    }
    ms.sort_unstable();
    let mean = ms.iter().sum::<u64>() as f64 / ms.len() as f64 / 1000.0;
    let median = ms[ms.len() / 2] as f64 / 1000.0;
    Some(format!("{mean:.1} s mean, {median:.1} s median over {} calls", ms.len()))
}

/// Files read per exploring call, over the rows that recorded them.
fn reads_line(rows: &[Row]) -> Option<String> {
    let n: Vec<usize> = rows.iter().filter_map(|r| r.files_read).collect();
    let max = *n.iter().max()?;
    let mean = n.iter().sum::<usize>() as f64 / n.len() as f64;
    Some(format!("{mean:.1} files mean, {max} at most, over {} calls", n.len()))
}

/// The cost line: total and mean over the calls that reported one.
/// `wrapped`: the rig asked claude for json, so a run with no cost says so.
fn cost_line(costs: &[Option<f64>], wrapped: bool) -> Option<String> {
    let have: Vec<f64> = costs.iter().flatten().copied().collect();
    if have.is_empty() {
        let n = costs.len();
        return (wrapped && n > 0).then(|| format!("none reported by {n} calls: claude's json carried no total_cost_usd"));
    }
    let total: f64 = have.iter().sum();
    let mut line = format!(
        "${total:.4} total, ${:.4} mean over {} calls",
        total / have.len() as f64,
        have.len()
    );
    let none = costs.len() - have.len();
    if none > 0 {
        line.push_str(&format!(", {none} without a cost"));
    }
    Some(line)
}

/// Where the write prompt came from: each override file the panel reads, or the
/// binary. An override that cannot be read is named: the panel sends the shipped text.
fn prompt_source(base: &Path) -> String {
    let mut from = vec![];
    for name in ["write.txt", "write.examples.txt"] {
        let p = base.join("tmux-cockpit/prompts").join(name);
        match std::fs::read_to_string(&p) {
            Ok(_) => from.push(p.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => from.push(format!("{} unreadable ({e}), shipped sent", p.display())),
        }
    }
    if from.is_empty() { "shipped".into() } else { from.join(", ") }
}

/// FNV-1a: stable across Rust releases, unlike DefaultHasher, so hashes from
/// two toolchains compare.
fn fnv(s: &str) -> u64 {
    s.bytes()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The hash of the write prompt and its example pool as the panel loads them,
/// overrides included: one value per prompt set, whatever the case (#701).
fn prompt_hash() -> String {
    prompt_hash_in(&config_home())
}

/// [`prompt_hash`] over the override folder under `base`: each of write.txt, its
/// example pool and write.explore.txt as `prompts::load` reads it (the file, or
/// the shipped text when it is missing or unreadable, less trailing newlines).
fn prompt_hash_in(base: &Path) -> String {
    use cockpit_tui::panel::prompts;
    let load = |name: &str, shipped: &str| {
        let p = base.join("tmux-cockpit/prompts").join(format!("{name}.txt"));
        let t = std::fs::read_to_string(p).unwrap_or_else(|_| shipped.to_string());
        t.trim_end_matches('\n').to_string()
    };
    let w = load("write", prompts::WRITE);
    let e = load("write.examples", prompts::WRITE_EXAMPLES);
    let x = load("write.explore", prompts::WRITE_EXPLORE);
    format!("{:016x}", fnv(&format!("{w}\0{e}\0{x}")))
}

/// The ids whose rows carry more than one prompt hash. A row with none, from an
/// older file, counts as one more value when another row of its case has one.
fn mixed_prompts(rows: &[Row]) -> Vec<String> {
    let mut ids: Vec<String> = vec![];
    for r in rows {
        let two = rows.iter().any(|o| o.id == r.id && o.prompt != r.prompt);
        if two && !ids.contains(&r.id) {
            ids.push(r.id.clone());
        }
    }
    ids
}

/// Where a run's cost goes: `--ledger`, else `cost.tsv` next to `--out`, else
/// `panel-eval-cost.tsv` in `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit`.
fn ledger_path(o: &Opts) -> PathBuf {
    if let Some(l) = &o.ledger {
        return l.clone();
    }
    if let Some(out) = &o.out {
        return out.with_file_name("cost.tsv");
    }
    let state = match std::env::var("XDG_STATE_HOME") {
        Ok(v) if v.starts_with('/') => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"),
    };
    state.join("tmux-cockpit/panel-eval-cost.tsv")
}

/// One cost ledger line: unix time, the `--out` label, cases, runs, dollars, prompt hash.
fn ledger_row(secs: u64, label: &str, cases: usize, runs: usize, total: f64, prompt: &str) -> String {
    format!("{secs}\t{label}\t{cases}\t{runs}\t{total:.4}\t{prompt}")
}

/// Appends `line` to the ledger at `path`, with a header when the file is new.
fn ledger_append(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let new = !path.exists();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    if new {
        writeln!(f, "time\tout\tcases\truns\tusd\tprompt")?;
    }
    writeln!(f, "{line}")
}

/// `n` of `d`, and what a summary line counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Of(usize, usize);

#[derive(Debug, Default, PartialEq)]
struct Tally {
    passed: Of,
    holdout: Of,
    either_asked: Of,
    must_not_hit: Of,
    asked_when_due: Of,
    on_target: Of,
    /// Asking as a classifier, over ask and write cases (either is neither):
    /// asked on a write case, and did not ask on an ask case.
    fpr: Of,
    fnr: Of,
    facts: Of,
    refused: usize,
    errors: usize,
    /// Per case: passed, graded, failed calls.
    per_case: Vec<(usize, usize, usize)>,
}

/// Graded runs only, but for `errors` and the per-case failed calls: a call
/// that never answered says nothing about the prompt.
fn tally(cases: &[Case], done: &[(usize, u128, Option<Grade>)]) -> Tally {
    let graded: Vec<(&Case, &Grade)> = done.iter().filter_map(|(c, _, g)| g.as_ref().map(|g| (&cases[*c], g))).collect();
    let of = |f: &dyn Fn(&Case, &Grade) -> bool, keep: &dyn Fn(&Case, &Grade) -> bool| {
        let pick: Vec<_> = graded.iter().filter(|(c, g)| keep(c, g)).collect();
        Of(pick.iter().filter(|(c, g)| f(c, g)).count(), pick.len())
    };
    let all = |_: &Case, _: &Grade| true;
    let pass = |_: &Case, g: &Grade| g.pass;
    let asked = |_: &Case, g: &Grade| g.asked;
    let is = |e: Expect| move |c: &Case, _: &Grade| c.expect == e;
    let wrote: Vec<_> = graded
        .iter()
        .filter(|(c, g)| c.expect == Expect::Write && !g.asked && !g.refused)
        .collect();
    let need: usize = wrote.iter().map(|(c, _)| c.must.len()).sum();
    let lack: usize = wrote.iter().map(|(_, g)| g.missing.len()).sum();
    Tally {
        passed: of(&pass, &all),
        holdout: of(&pass, &|c, _| c.holdout),
        either_asked: of(&asked, &is(Expect::Either)),
        must_not_hit: of(&|_, g| !g.forbidden.is_empty(), &|c, _| !c.must_not.is_empty()),
        asked_when_due: of(&asked, &is(Expect::Ask)),
        on_target: of(&|_, g| g.asked && g.on_target, &is(Expect::Ask)),
        fpr: of(&asked, &is(Expect::Write)),
        fnr: of(&|_, g| !g.asked, &is(Expect::Ask)),
        facts: Of(need - lack, need),
        refused: graded.iter().filter(|(_, g)| g.refused).count(),
        errors: done.iter().filter(|(_, _, g)| g.is_none()).count(),
        per_case: (0..cases.len())
            .map(|i| {
                let mine: Vec<_> = done.iter().filter(|(c, _, _)| *c == i).collect();
                let ok = mine.iter().filter(|(_, _, g)| g.as_ref().is_some_and(|g| g.pass)).count();
                let err = mine.iter().filter(|(_, _, g)| g.is_none()).count();
                (ok, mine.len() - err, err)
            })
            .collect(),
    }
}

/// The Wilson score interval at 95% for `k` passes of `n`; `None` for no
/// trials or more passes than trials.
fn wilson(k: usize, n: usize) -> Option<(f64, f64)> {
    if n == 0 || k > n {
        return None;
    }
    let (z, n, p) = (1.959_964, n as f64, k as f64 / n as f64);
    let z2 = z * z;
    let mid = (p + z2 / (2.0 * n)) / (1.0 + z2 / n);
    let half = z / (1.0 + z2 / n) * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    Some(((mid - half).max(0.0), (mid + half).min(1.0)))
}

/// Cases whose every run passed, of the cases whose every run was graded: a
/// failed call leaves the case out rather than counting against the prompt.
fn pass_all(per_case: &[(usize, usize, usize)]) -> Of {
    let whole: Vec<_> = per_case.iter().filter(|(_, n, err)| *n > 0 && *err == 0).collect();
    Of(whole.iter().filter(|(ok, n, _)| ok == n).count(), whole.len())
}

fn pct(Of(n, d): Of) -> String {
    if d == 0 {
        "-".into()
    } else {
        format!("{:.0}% ({n}/{d})", 100.0 * n as f64 / d as f64)
    }
}

/// `pct`, then its Wilson 95% interval: cases are the independent trials, the
/// runs of one case are not.
fn pct_ci(o: Of) -> String {
    match wilson(o.0, o.1) {
        Some((lo, hi)) => format!("{}  95% CI {:.0}%-{:.0}%", pct(o), 100.0 * lo, 100.0 * hi),
        None => pct(o),
    }
}

fn summary(cases: &[Case], done: &[(usize, u128, Option<Grade>)], hashes: &[u64], runs: usize) {
    let t = tally(cases, done);
    println!();
    // Per case, over the runs: a flaky case shows as 2/3, not as noise in a total.
    println!("{:<24} {:<6} pass", "case", "want");
    for (c, (ok, n, err)) in cases.iter().zip(&t.per_case) {
        let tag = if c.holdout { "  holdout" } else { "" };
        let err = if *err > 0 {
            format!("  {err} failed call(s)")
        } else {
            String::new()
        };
        println!("{:<24} {:<6} {ok}/{n}{tag}{err}", c.id, c.expect.as_str());
    }
    let hs: Vec<String> = hashes.iter().map(|h| format!("{h:016x}")).collect();
    println!();
    println!("system prompt   {} ({})", hs.join(" "), prompt_source(&config_home()));
    println!("passed          {}", pct(t.passed));
    // The share of cases that passed all their runs: a flaky case counts as a failure.
    println!("{:<16}{}", format!("pass^{runs}"), pct_ci(pass_all(&t.per_case)));
    if t.holdout.1 > 0 {
        println!("  holdout       {}", pct(t.holdout));
    }
    if t.either_asked.1 > 0 {
        println!("either: asked   {}", pct(t.either_asked));
    }
    println!("must_not hit    {}", pct(t.must_not_hit));
    println!("asked when due  {}", pct(t.asked_when_due));
    println!("  on target     {}", pct(t.on_target));
    println!("asking          FPR {}  FNR {}", pct(t.fpr), pct(t.fnr));
    println!("facts present   {}", pct(t.facts));
    if t.refused > 0 {
        println!("refused/cut off {}", t.refused);
    }
    let mut ms: Vec<u128> = done.iter().filter(|(_, _, g)| g.is_some()).map(|(_, m, _)| *m).collect();
    ms.sort();
    if !ms.is_empty() {
        let mean = ms.iter().sum::<u128>() / ms.len() as u128;
        println!("latency ms      mean {mean}  p50 {}  max {}", ms[ms.len() / 2], ms[ms.len() - 1]);
    }
    if t.errors > 0 {
        println!("failed calls    {}", t.errors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = "# a comment\n=== crash\nkind: bug\nwindow: bash · bash · /home/u\nexpect: write\n\
                       must: net-proxy; line 88\n--- screen\n$ net-proxy\nboom\n--- request\nfix this\n";

    #[test]
    fn reads_a_case() {
        let c = &parse_cases(ONE).unwrap()[0];
        assert_eq!((c.id.as_str(), c.kind.as_str(), c.cmd.as_str()), ("crash", "bug", "bash"));
        assert_eq!(c.must, ["net-proxy", "line 88"]);
        assert_eq!(c.screen, "$ net-proxy\nboom");
        assert_eq!(c.request, "fix this");
        assert_eq!(c.expect, Expect::Write);
    }

    #[test]
    fn refuses_a_bad_case() {
        assert!(
            parse_cases("=== x\nkind: bug\nwindow: a · b · c\n--- request\nr\n")
                .unwrap_err()
                .contains("no expect")
        );
        let dup = format!("{ONE}{ONE}");
        assert!(parse_cases(&dup).unwrap_err().contains("duplicate"));
        let wrong = ONE.replace("expect: write", "expect: ask");
        assert!(parse_cases(&wrong).unwrap_err().contains("must is for write"));
    }

    #[test]
    fn grades_a_question_and_a_prompt() {
        let mut c = parse_cases(ONE).unwrap().remove(0);
        let g = grade(&c, "<prompt>Fix net-proxy at line 88.</prompt>");
        assert!(g.pass && !g.asked, "{g:?}");
        let g = grade(&c, "<prompt>Fix net-proxy.</prompt>");
        assert_eq!(g.missing, ["line 88"]);
        assert!(!g.pass);
        let q = "<question>What should happen?</question>\n<option>Skip the line</option>\n<option>Stop</option>";
        assert!(!grade(&c, q).pass);
        c.expect = Expect::Ask;
        c.must.clear();
        c.ask_any = vec!["should happen".into()];
        let g = grade(&c, q);
        assert!(g.pass && g.on_target, "{g:?}");
        c.ask_any = vec!["which file".into()];
        assert!(!grade(&c, q).pass);
    }

    #[test]
    fn must_not_skips_a_mention_a_negator_cancels() {
        let mut c = parse_cases(ONE).unwrap().remove(0);
        c.must_not = vec!["by hand".into()];
        for r in [
            "<prompt>Do not write the golden files by hand, and use the project's own flag.</prompt>",
            "<prompt>The test defines an update flag, so use it rather than writing the files by hand.</prompt>",
        ] {
            assert_eq!(grade(&c, r).forbidden, Vec::<String>::new(), "{r}");
        }
    }

    #[test]
    fn must_not_still_trips_on_a_plain_mention() {
        let mut c = parse_cases(ONE).unwrap().remove(0);
        c.must_not = vec!["by hand".into()];
        let g = grade(&c, "<prompt>Write them by hand. Do not run the generator.</prompt>");
        assert_eq!(g.forbidden, ["by hand"]);
    }

    #[test]
    fn a_negator_before_a_comma_does_not_cancel() {
        for r in [
            "If the tests do not pass, edit the golden files by hand",
            "If there is no flag, write the golden files by hand",
            "It is a no-op flag, so write them by hand",
            "Do it without the flag, regenerate them by hand",
        ] {
            assert!(has_unnegated(r, "by hand"), "{r}");
        }
    }

    #[test]
    fn a_negator_cancels_only_inside_its_clause_and_window() {
        assert!(has_unnegated("Do not edit. Write the files by hand.", "by hand"));
        assert!(has_unnegated("not a b c d e f write by hand", "by hand"));
        assert!(!has_unnegated("never write it by hand", "by hand"));
        assert!(!has_unnegated("Don\u{2019}t use a subagent", "subagent"));
        assert!(!has_unnegated("use the flag instead of editing by hand", "by hand"));
        assert!(has_unnegated("a subagent, not a skill, and a subagent", "subagent"));
    }

    #[test]
    fn either_takes_a_good_question_or_a_good_prompt_and_must_not_fails_both() {
        let mut c = parse_cases(ONE).unwrap().remove(0);
        c.expect = Expect::Either;
        c.ask_any = vec!["which line".into()];
        c.must_not = vec!["subagent".into()];
        assert!(grade(&c, "<prompt>Fix net-proxy at line 88.</prompt>").pass);
        assert!(grade(&c, "<question>Which line?</question>").pass);
        assert!(!grade(&c, "<question>What now?</question>").pass);
        assert!(!grade(&c, "<prompt>Fix net-proxy.</prompt>").pass);
        let g = grade(&c, "<prompt>Use a subagent to fix net-proxy at line 88.</prompt>");
        assert_eq!(g.forbidden, ["subagent"]);
        assert!(!g.pass);
    }

    #[test]
    fn reads_must_not_holdout_and_either() {
        let t = ONE.replace("expect: write", "expect: either\nmust_not: a; b\nholdout: yes");
        let c = &parse_cases(&t).unwrap()[0];
        assert_eq!((c.expect, c.holdout), (Expect::Either, true));
        assert_eq!(c.must_not, ["a", "b"]);
        assert!(parse_cases(&ONE.replace("expect: write", "expect: write\nholdout: maybe")).is_err());
    }

    #[test]
    fn a_comment_after_the_last_body_is_not_part_of_it() {
        let two = format!("{ONE}\n# next section\n\n{}", ONE.replace("=== crash", "=== crash2"));
        let cs = parse_cases(&two).unwrap();
        assert_eq!(cs[0].request, "fix this");
        let screen_last = "=== s\nkind: bug\nwindow: a · b · c\nexpect: write\n--- request\nr\n--- screen\n$ ls\n\n# a note\n";
        assert_eq!(parse_cases(screen_last).unwrap()[0].screen, "$ ls");
        let twice = ONE.replace("kind: bug", "kind: bug\nkind: plan");
        assert!(parse_cases(&twice).unwrap_err().contains("kind given twice"));
    }

    #[test]
    fn reads_a_repo_section_and_refuses_it_twice() {
        let t = ONE.replace("--- request", "--- repo\nbranch: main\n  M a.rs\n--- request");
        assert_eq!(parse_cases(&t).unwrap()[0].repo, "branch: main\n  M a.rs");
        assert_eq!(parse_cases(ONE).unwrap()[0].repo, "");
        let twice = t.replace("--- request", "--- repo\nx\n--- request");
        assert!(parse_cases(&twice).unwrap_err().contains("repo given twice"));
    }

    #[test]
    fn words_match_whole_and_any_of() {
        assert!(!has("Before editing, write an explanation", "plan"));
        assert!(has("Make a plan first.", "plan"));
        assert!(!has("the answer is 360", "36"));
        assert!(has("it is 36.", "36"));
        assert!(!has("a different file", "diff"));
        assert!(!has("install it", "all"));
        assert!(has("cite sources", "source|sources"));
        assert!(has("add --json to show", "--json"));
        assert!(has("run /code-review now", "/code-review"));
    }

    #[test]
    fn the_grade_reads_what_the_panel_shows() {
        let c = parse_cases(ONE).unwrap().remove(0);
        let edit = grade(&c, "<edit path=\"p\">net-proxy line 88</edit>");
        assert!(edit.refused && !edit.pass, "{edit:?}");
        let outside = grade(&c, "net-proxy, line 88.\n<prompt>Fix it.</prompt>");
        assert!(!outside.pass, "{outside:?}");
        let mut q = c.clone();
        q.expect = Expect::Ask;
        q.must.clear();
        q.must_not = vec!["subagent".into()];
        let g = grade(&q, "<question>Use a subagent?</question>");
        assert!(g.asked && !g.pass && g.forbidden == ["subagent"], "{g:?}");
    }

    #[test]
    fn the_tally_counts_per_kind_and_keeps_errors_apart() {
        let mk = |id: &str, e: Expect| {
            let mut c = parse_cases(ONE).unwrap().remove(0);
            c.id = id.into();
            c.expect = e;
            c
        };
        let cases = vec![mk("a", Expect::Ask), mk("w", Expect::Write)];
        let asked = Grade {
            asked: true,
            on_target: true,
            pass: true,
            ..Grade::default()
        };
        let wrote = Grade {
            pass: true,
            ..Grade::default()
        };
        let missed = Grade {
            missing: vec!["line 88".into()],
            ..Grade::default()
        };
        let done = vec![
            (0, 1, Some(asked)),
            (0, 1, None),
            (1, 1, Some(wrote)),
            (1, 1, Some(missed)),
            (
                1,
                1,
                Some(Grade {
                    asked: true,
                    ..Grade::default()
                }),
            ),
        ];
        let t = tally(&cases, &done);
        assert_eq!(t.passed, Of(2, 4));
        assert_eq!(t.asked_when_due, Of(1, 1));
        assert_eq!(t.fpr, Of(1, 3));
        assert_eq!(t.facts, Of(3, 4));
        assert_eq!(t.errors, 1);
        assert_eq!(t.per_case, [(1, 1, 1), (1, 3, 0)]);
    }

    #[test]
    fn a_cut_off_reply_is_refused() {
        let c = parse_cases(ONE).unwrap().remove(0);
        let g = grade(&c, "<prompt>Fix net-proxy at line 88.");
        assert!(g.refused && !g.pass, "{g:?}");
    }

    #[test]
    fn a_screen_or_request_given_twice_is_an_error() {
        let screen = ONE.replace("--- request", "--- screen\nx\n--- request");
        assert!(parse_cases(&screen).unwrap_err().contains("screen given twice"));
        let request = format!("{ONE}--- request\nagain\n");
        assert!(parse_cases(&request).unwrap_err().contains("request given twice"));
    }

    #[test]
    fn a_trailing_hash_line_is_kept_unless_a_blank_line_sets_it_off() {
        let issue = ONE.replace("fix this\n", "fix this\n#412\n");
        assert_eq!(parse_cases(&issue).unwrap()[0].request, "fix this\n#412");
        let root = "=== s\nkind: bug\nwindow: a · b · c\nexpect: write\n--- request\nr\n--- screen\n$ ls\n# id\n";
        assert_eq!(parse_cases(root).unwrap()[0].screen, "$ ls\n# id");
        let only = "=== s\nkind: bug\nwindow: a · b · c\nexpect: write\n--- request\n#412\n";
        assert_eq!(parse_cases(only).unwrap()[0].request, "#412");
        let comment = ONE.replace("fix this\n", "fix this\n#412\n\n# next\n# more\n\n");
        assert_eq!(parse_cases(&comment).unwrap()[0].request, "fix this\n#412");
    }

    #[test]
    fn words_match_where_matches_overlap() {
        assert!(has("xab ab ab", "ab ab"));
        assert!(has("nana na na", "na na"));
        assert!(!has("xab abx", "ab ab"));
    }

    /// A fresh `<scratch>/<name>.<pid>` holding `tmux-cockpit/prompts`.
    fn config(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panel-eval-{name}.{}", std::process::id()));
        if d.exists() {
            std::fs::remove_dir_all(&d).unwrap();
        }
        std::fs::create_dir_all(d.join("tmux-cockpit/prompts")).unwrap();
        d
    }

    #[test]
    fn the_prompt_source_names_each_override_and_an_unreadable_one() {
        let d = config("src");
        let p = d.join("tmux-cockpit/prompts");
        assert_eq!(prompt_source(&d), "shipped");
        std::fs::write(p.join("write.examples.txt"), "x").unwrap();
        let s = prompt_source(&d);
        assert!(s.contains("write.examples.txt") && !s.contains("unreadable"), "{s}");
        std::fs::create_dir(p.join("write.txt")).unwrap();
        assert!(prompt_source(&d).contains("unreadable"), "{}", prompt_source(&d));
        std::fs::remove_dir(p.join("write.txt")).unwrap();
        std::fs::write(p.join("write.txt"), [0xff, 0xfe]).unwrap();
        assert!(prompt_source(&d).contains("unreadable"), "{}", prompt_source(&d));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn out_is_fails_a_reply_of_another_type_but_not_a_question() {
        let mut c = parse_cases(&ONE.replace("expect: write", "expect: either\nout_is: prompt"))
            .unwrap()
            .remove(0);
        assert_eq!(c.out_is, Some(OutType::Prompt));
        let hint = grade(&c, "<hint>net-proxy, line 88</hint>");
        assert_eq!((hint.wrong_type, hint.pass), (Some("hint"), false));
        let text = grade(&c, "<text>net-proxy, line 88</text>");
        assert_eq!((text.wrong_type, text.pass), (Some("text"), false));
        assert!(grade(&c, "<prompt>Fix net-proxy at line 88.</prompt>").pass);
        c.ask_any = vec!["which line".into()];
        assert!(grade(&c, "<question>Which line?</question>").pass);
    }

    #[test]
    fn out_is_is_read_and_checked() {
        assert_eq!(parse_cases(ONE).unwrap()[0].out_is, None);
        let bad = ONE.replace("expect: write", "expect: write\nout_is: poem");
        assert!(parse_cases(&bad).unwrap_err().contains("unknown out_is poem"));
        let auto = ONE.replace("expect: write", "expect: write\nout_is: auto");
        assert!(parse_cases(&auto).unwrap_err().contains("unknown out_is auto"));
        let ask = ONE.replace("expect: write\nmust: net-proxy; line 88", "expect: ask\nout_is: prompt");
        assert!(parse_cases(&ask).unwrap_err().contains("out_is is for write"));
    }

    fn near(got: (f64, f64), want: (f64, f64)) -> bool {
        (got.0 - want.0).abs() < 0.005 && (got.1 - want.1).abs() < 0.005
    }

    #[test]
    fn wilson_matches_known_intervals() {
        assert!(near(wilson(36, 40).unwrap(), (0.77, 0.96)), "{:?}", wilson(36, 40));
        assert!(near(wilson(0, 10).unwrap(), (0.0, 0.2775)), "{:?}", wilson(0, 10));
        assert!(near(wilson(10, 10).unwrap(), (0.7225, 1.0)), "{:?}", wilson(10, 10));
        assert!(near(wilson(1, 2).unwrap(), (0.0945, 0.9055)), "{:?}", wilson(1, 2));
    }

    #[test]
    fn wilson_refuses_no_trials_and_more_passes_than_trials() {
        assert_eq!(wilson(0, 0), None);
        assert_eq!(wilson(5, 4), None);
    }

    #[test]
    fn pass_all_counts_cases_that_passed_every_run_and_skips_failed_calls() {
        assert_eq!(pass_all(&[(3, 3, 0), (2, 3, 0), (2, 2, 1), (0, 0, 3)]), Of(1, 2));
        assert_eq!(pass_all(&[]), Of(0, 0));
    }

    #[test]
    fn asking_rates_count_asks_on_write_cases_and_silence_on_ask_cases() {
        let mut ask = parse_cases(ONE).unwrap().remove(0);
        ask.expect = Expect::Ask;
        let mut either = ask.clone();
        either.expect = Expect::Either;
        let cases = vec![ask, parse_cases(ONE).unwrap().remove(0), either];
        let g = |asked| Some(Grade { asked, ..Grade::default() });
        let runs = [
            (0, g(true)),
            (0, g(false)),
            (0, g(false)),
            (1, g(true)),
            (1, g(false)),
            (2, g(true)),
            (1, None),
        ];
        let done: Vec<_> = runs.into_iter().map(|(c, g)| (c, 1, g)).collect();
        let t = tally(&cases, &done);
        assert_eq!((t.fpr, t.fnr), (Of(1, 2), Of(2, 3)));
    }

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn the_prompt_hash_follows_each_override_explore_included() {
        let d = std::env::temp_dir().join(format!("panel-eval-hash-{}", std::process::id()));
        let dir = d.join("tmux-cockpit/prompts");
        std::fs::create_dir_all(&dir).unwrap();
        let shipped = prompt_hash_in(&d);
        let mut seen = vec![shipped.clone()];
        for name in ["write", "write.examples", "write.explore"] {
            std::fs::write(dir.join(format!("{name}.txt")), format!("{name} edited")).unwrap();
            let h = prompt_hash_in(&d);
            assert!(!seen.contains(&h), "{name}.txt did not change the hash");
            seen.push(h);
        }
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(prompt_hash_in(&d), shipped);
    }

    #[test]
    fn compare_names_a_case_whose_rows_carry_two_prompt_hashes() {
        let rows = read_rows(
            "a",
            concat!(
                r#"{"id":"x","expect":"write","run":0,"pass":true,"prompt":"aa"}"#,
                "\n",
                r#"{"id":"x","expect":"write","run":1,"pass":true,"prompt":"bb"}"#,
                "\n",
                r#"{"id":"y","expect":"write","run":0,"pass":true,"prompt":"aa"}"#,
                "\n",
                r#"{"id":"z","expect":"write","run":0,"pass":true}"#,
                "\n",
                r#"{"id":"z","expect":"write","run":1,"pass":true,"prompt":"aa"}"#,
                "\n",
            ),
        )
        .unwrap();
        assert_eq!(mixed_prompts(&rows), ["x", "z"], "z: a row with no hash beside one with a hash");
        assert!(opts(args("--compare a b --allow-mixed")).unwrap().allow_mixed);
        assert!(!opts(args("--compare a b")).unwrap().allow_mixed);
    }

    #[test]
    fn the_ledger_goes_to_ledger_then_next_to_out_then_the_state_folder() {
        let o = opts(args("--ledger /l/c.tsv --out /o/a.jsonl")).unwrap();
        assert_eq!(ledger_path(&o), PathBuf::from("/l/c.tsv"));
        let o = opts(args("--out /o/a.jsonl")).unwrap();
        assert_eq!(ledger_path(&o), PathBuf::from("/o/cost.tsv"));
        let o = opts(args("")).unwrap();
        assert!(
            ledger_path(&o).ends_with("tmux-cockpit/panel-eval-cost.tsv"),
            "{:?}",
            ledger_path(&o)
        );
    }

    #[test]
    fn the_ledger_appends_one_row_a_run_under_a_header() {
        let d = std::env::temp_dir().join(format!("panel-eval-ledger-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("cost.tsv");
        ledger_append(&p, &ledger_row(7, "a.jsonl", 4, 5, 0.25, "ff")).unwrap();
        ledger_append(&p, &ledger_row(8, "b.jsonl", 1, 3, 0.1, "ee")).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(
            lines,
            [
                "time\tout\tcases\truns\tusd\tprompt",
                "7\ta.jsonl\t4\t5\t0.2500\tff",
                "8\tb.jsonl\t1\t3\t0.1000\tee"
            ]
        );
    }

    #[test]
    fn compare_takes_two_files() {
        let o = opts(args("--compare a.jsonl b.jsonl")).unwrap();
        assert_eq!(o.compare, Some(("a.jsonl".into(), "b.jsonl".into())));
        assert!(opts(args("--compare a.jsonl")).unwrap_err().contains("--compare needs two files"));
        assert!(opts(args("--compare")).unwrap_err().contains("--compare needs two files"));
    }

    const ROWS: &str = r#"{"id":"a","expect":"ask","run":0,"pass":true,"error":null,"system":"00ff"}
{"id":"a","expect":"ask","run":1,"pass":false,"error":null,"cost_usd":0.5}

{"id":"w","expect":"write","run":0,"pass":null,"error":"took too long"}
"#;

    #[test]
    fn reads_rows_and_takes_a_failed_call_as_no_pass() {
        let r = read_rows("A", ROWS).unwrap();
        assert_eq!(r.len(), 3);
        let want = Row {
            id: "a".into(),
            expect: "ask".into(),
            pass: Some(true),
            system: "00ff".into(),
            prompt: String::new(),
            cost_usd: None,
            ms: None,
            files_read: None,
            env: None,
        };
        assert_eq!(r[0], want);
        assert_eq!((r[1].cost_usd, r[2].pass), (Some(0.5), None));
    }

    #[test]
    fn refuses_bad_rows_naming_the_file_and_line() {
        assert!(read_rows("A", "").unwrap_err().contains("A: no rows"));
        assert!(read_rows("A", "{\"id\":\"a\"}\nnot json\n").unwrap_err().starts_with("A:2:"));
        assert!(
            read_rows("A", "{\"expect\":\"ask\",\"pass\":true}\n")
                .unwrap_err()
                .contains("A:1: no id")
        );
        assert!(
            read_rows("A", "{\"id\":\"a\",\"pass\":\"yes\"}\n")
                .unwrap_err()
                .contains("A:1: pass")
        );
    }

    #[test]
    fn compare_pairs_cases_and_names_the_ones_that_moved() {
        let row = |id: &str, pass: Option<bool>| Row {
            id: id.into(),
            expect: "ask".into(),
            pass,
            ..Row::default()
        };
        let a = [
            row("up", Some(false)),
            row("up", Some(true)),
            row("same", Some(true)),
            row("gone", Some(true)),
        ];
        let b = [
            row("same", Some(true)),
            row("up", Some(true)),
            row("up", Some(true)),
            row("new", None),
        ];
        let c = compare(&a, &b);
        let ids: Vec<&str> = c.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["up", "same", "gone", "new"]);
        assert_eq!((c[0].a, c[0].b), (Some((1, 2, 0)), Some((2, 2, 0))));
        assert_eq!(c[0].moved(), Some(std::cmp::Ordering::Greater));
        assert_eq!(c[1].moved(), None);
        assert_eq!((c[2].b, c[2].moved()), (None, None));
        assert_eq!((c[3].b, c[3].moved()), (Some((0, 0, 1)), None));
    }

    #[test]
    fn a_case_held_at_the_same_rate_did_not_move() {
        let row = |pass: Option<bool>| Row {
            id: "x".into(),
            expect: "ask".into(),
            pass,
            ..Row::default()
        };
        let a = [row(Some(true)), row(Some(true)), row(Some(true))];
        let b = [row(Some(true)), row(Some(true)), row(None)];
        assert_eq!(compare(&a, &b)[0].moved(), None);
        let down = [row(Some(true)), row(Some(false)), row(Some(true))];
        assert_eq!(compare(&a, &down)[0].moved(), Some(std::cmp::Ordering::Less));
    }

    #[test]
    fn unwraps_a_json_result_with_its_cost_and_tokens() {
        let j = r#"{"type":"result","is_error":false,"result":"<prompt>p</prompt>","total_cost_usd":0.0123,
            "usage":{"input_tokens":2,"cache_creation_input_tokens":100,"cache_read_input_tokens":30,"output_tokens":7}}"#;
        let (text, u) = unwrap_json(&j.replace('\n', "")).unwrap();
        assert_eq!(text, "<prompt>p</prompt>");
        assert_eq!(
            u,
            Usage {
                cost_usd: Some(0.0123),
                in_tokens: Some(132),
                out_tokens: Some(7)
            }
        );
    }

    #[test]
    fn text_that_is_not_a_json_result_is_the_reply_as_it_stands() {
        assert_eq!(
            unwrap_json("<prompt>p</prompt>\n").unwrap(),
            ("<prompt>p</prompt>\n".into(), Usage::default())
        );
        assert_eq!(unwrap_json("").unwrap(), (String::new(), Usage::default()));
    }

    #[test]
    fn a_json_error_result_is_a_failed_call() {
        let j = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"overloaded"}"#;
        assert_eq!(unwrap_json(j).unwrap_err(), "overloaded");
        let bare = r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#;
        assert_eq!(unwrap_json(bare).unwrap_err(), "error_max_turns");
    }

    #[test]
    fn a_verbose_json_array_yields_its_last_result() {
        let j = r#"[{"type":"assistant","text":"<prompt>no</prompt>"},{"type":"result","result":"x"},
            {"type":"result","is_error":false,"result":"<prompt>p</prompt>","total_cost_usd":0.5}]"#;
        let (text, u) = unwrap_json(j).unwrap();
        assert_eq!((text.as_str(), u.cost_usd), ("<prompt>p</prompt>", Some(0.5)));
        let none = r#"[{"type":"assistant","text":"<prompt>no</prompt>"}]"#;
        assert_eq!(unwrap_json(none).unwrap_err(), "not a claude json result");
    }

    #[test]
    fn json_without_a_string_result_is_a_failed_call() {
        let turns = r#"{"type":"result","subtype":"error_max_turns","is_error":false}"#;
        assert_eq!(unwrap_json(turns).unwrap_err(), "error_max_turns");
        let other = r#"{"a":"<prompt>p</prompt>"}"#;
        assert_eq!(unwrap_json(other).unwrap_err(), "not a claude json result");
        let number = r#"{"type":"result","result":7}"#;
        assert_eq!(unwrap_json(number).unwrap_err(), "not a claude json result");
    }

    #[test]
    fn an_empty_json_result_is_no_answer() {
        let j = r#"{"type":"result","is_error":false,"result":" \n"}"#;
        assert_eq!(unwrap_json(j).unwrap_err(), "no answer");
    }

    #[test]
    fn a_wrapped_run_with_no_cost_says_so() {
        assert_eq!(cost_line(&[None, None], false), None);
        let l = cost_line(&[None, None], true).unwrap();
        assert_eq!(l, "none reported by 2 calls: claude's json carried no total_cost_usd");
        assert_eq!(cost_line(&[], true), None);
    }

    #[test]
    fn the_cost_line_sums_the_calls_that_reported_one() {
        assert_eq!(cost_line(&[], false), None);
        assert_eq!(cost_line(&[None], false), None);
        let l = cost_line(&[Some(0.01), Some(0.03), None], false).unwrap();
        assert_eq!(l, "$0.0400 total, $0.0200 mean over 2 calls, 1 without a cost");
    }

    #[test]
    fn a_file_names_each_prompt_once_however_its_rows_interleave() {
        let row = |h: &str| Row {
            system: h.into(),
            ..Row::default()
        };
        let rows = [row("a"), row("b"), row("a"), row(""), row("c"), row("b")];
        assert_eq!(prompts_of(&rows), ["a", "b", "c"]);
        assert!(prompts_of(&[row("")]).is_empty());
    }

    #[test]
    fn an_explore_key_names_one_fixture_folder() {
        let c = ONE.replace("expect: write", "expect: write\nexplore: invoicer");
        assert_eq!(parse_cases(&c).unwrap()[0].explore.as_deref(), Some("invoicer"));
        assert_eq!(parse_cases(ONE).unwrap()[0].explore, None);
        for bad in ["../etc", "a/b", ".", ".hidden"] {
            let c = ONE.replace("expect: write", &format!("expect: write\nexplore: {bad}"));
            assert!(parse_cases(&c).unwrap_err().contains("folder name"), "{bad}");
        }
        let empty = ONE.replace("expect: write", "expect: write\nexplore:");
        assert!(parse_cases(&empty).unwrap_err().contains("folder name"));
    }

    /// Every explore case needs a read: each `must` is in the fixture and on no
    /// screen or request, so a text call cannot copy it.
    #[test]
    fn each_explore_case_names_what_only_its_fixture_holds() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel");
        let cases = parse_cases(&std::fs::read_to_string(root.join("explore.txt")).unwrap()).unwrap();
        assert_eq!(cases.len(), 4);
        for c in &cases {
            let fx = root.join("fixtures").join(c.explore.as_deref().expect("an explore key"));
            let mut all = String::new();
            grep_tree(&fx, &mut all);
            for m in &c.must {
                assert!(has(&all, m), "{}: {m} is in the fixture", c.id);
                assert!(!has(&c.screen, m) && !has(&c.request, m), "{}: {m} is not given", c.id);
            }
        }
    }

    /// Every file's path and text under `d`, appended to `out`.
    fn grep_tree(d: &Path, out: &mut String) {
        for e in std::fs::read_dir(d).unwrap().map(Result::unwrap) {
            out.push_str(&format!("{}\n", e.path().display()));
            if e.file_type().unwrap().is_dir() {
                grep_tree(&e.path(), out);
            } else {
                out.push_str(&std::fs::read_to_string(e.path()).unwrap());
            }
        }
    }

    /// The rig sends what the panel's own App picks: the prompts, the envelope
    /// with its three questions, and the profile, for a text and an exploring write.
    #[test]
    fn the_rig_sends_the_panels_own_call() {
        use cockpit_tui::panel::app::{EXPLORE_BUDGET_USD, EXPLORE_TIMEOUT};
        use cockpit_tui::panel::capture::window_kind;
        use cockpit_tui::panel::envelope::build;
        use cockpit_tui::panel::prompts::{self, Ctx};
        let c = &parse_cases(ONE).unwrap()[0];
        let ctx = Ctx {
            out: c.out,
            window: window_kind(&c.name, &c.cmd),
            ..Ctx::default()
        };
        let cap = Capture {
            name: c.name.clone(),
            cmd: c.cmd.clone(),
            path: "/r".into(),
            screen: c.screen.clone(),
            repo: c.repo.as_str().into(),
        };
        let flow = Flow::Write {
            draft: c.request.clone(),
            out: c.out,
        };
        let (system, prompt, profile) = panel_call(c, "/r", false);
        assert_eq!((system, profile), (prompts::write(&ctx), Profile::TEXT));
        assert_eq!(prompt, build(&flow, &cap, &[], 3));
        let (system, _, profile) = panel_call(c, "/r", true);
        assert_eq!(system, prompts::write_explore(&ctx));
        assert_eq!(profile, Profile::read_only("/r".into(), false, EXPLORE_BUDGET_USD, EXPLORE_TIMEOUT));
    }

    const ANSWERED: &str = "=== cache\nkind: feature\nwindow: bash · bash · /home/u\nexpect: write\n\
                            --- conversation\nassistant: Which API? Options: gateway | billing\n\
                            user: billing\n--- request\nadd caching\n";

    #[test]
    fn reads_a_conversation_as_turns_in_order() {
        let c = &parse_cases(ANSWERED).unwrap()[0];
        let roles: Vec<Role> = c.turns.iter().map(|t| t.role).collect();
        assert_eq!(roles, [Role::Assistant, Role::User]);
        assert_eq!(c.turns[1].text, "billing");
        assert_eq!(c.request, "add caching");
    }

    #[test]
    fn refuses_a_conversation_line_without_a_role() {
        let bad = ANSWERED.replace("user: billing", "billing");
        let e = parse_cases(&bad).unwrap_err();
        assert!(e.starts_with("case cache: ") && e.contains("user: or assistant:"), "{e}");
        let empty = ANSWERED.replace("assistant: Which API? Options: gateway | billing\nuser: billing\n", "");
        assert!(parse_cases(&empty).unwrap_err().contains("empty conversation"));
    }

    /// An answered case is sent as the panel sends its next round: the turns,
    /// and one question fewer for each one asked.
    #[test]
    fn the_rig_sends_a_conversation_with_the_questions_it_used() {
        use cockpit_tui::panel::envelope::build;
        let c = &parse_cases(ANSWERED).unwrap()[0];
        let cap = Capture {
            name: c.name.clone(),
            cmd: c.cmd.clone(),
            path: "/r".into(),
            screen: String::new(),
            repo: "".into(),
        };
        let flow = Flow::Write {
            draft: c.request.clone(),
            out: c.out,
        };
        let (_, prompt, _) = panel_call(c, "/r", false);
        assert_eq!(prompt, build(&flow, &cap, &c.turns, 2));
    }

    // ------------------------------------------- #365 child 5: `--- refs`

    const REFS: &str = "=== picked\nkind: bug\nwindow: claude · claude · /home/u/shop\nexpect: write\n\
                        --- refs\nsrc/cart.go:40-72\nREADME.md\n../shared/money.go:3\n--- request\nfix the total\n";

    /// A stage dir, removed on drop.
    struct Gone(PathBuf);

    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_a_refs_section_as_paths_and_lines() {
        let c = &parse_cases(REFS).unwrap()[0];
        let want = [
            ("src/cart.go", Some((40, 72))),
            ("README.md", None),
            ("../shared/money.go", Some((3, 3))),
        ];
        assert_eq!(c.refs, want.map(|(p, l)| (p.to_string(), l)));
        assert!(parse_cases(ONE).unwrap()[0].refs.is_empty());
    }

    #[test]
    fn refuses_a_ref_that_is_absolute_climbs_inside_or_has_a_bad_range() {
        for bad in ["/etc/hosts", "src/../x.go", "../../x.go", "a.go:9-3", "a.go:0", "a.go:", "a.go:1-x"] {
            let t = REFS.replace("README.md\n", &format!("{bad}\n"));
            let e = parse_cases(&t).map(|_| ()).expect_err(bad);
            assert!(e.starts_with("case picked: ") && e.contains("ref"), "{bad:?}: {e}");
        }
        let twice = REFS.replace("--- request", "--- refs\nb.go\n--- request");
        assert!(parse_cases(&twice).unwrap_err().contains("refs given twice"));
    }

    /// A refs case runs in a folder of its own holding the files, so the panel's
    /// own ^S renders them: mentions for claude, bare refs for another window.
    #[test]
    fn the_rig_sends_refs_as_the_panel_does() {
        let cases = parse_cases(REFS).unwrap();
        let d = Gone(stage(&cases).unwrap().expect("a stage"));
        let path = pane_path(&cases[0], Some(&d.0));
        assert!(Path::new(&path).join("src/cart.go").is_file() && path.ends_with("/shop"), "{path}");
        let (_, prompt, _) = panel_call(&cases[0], &path, false);
        let block = "<refs>\n<ref form=\"mention\">@src/cart.go#L40-72</ref>\n\
                     <ref form=\"mention\" note=\"whole file\">@README.md</ref>\n<ref form=\"mention\">@/";
        assert!(
            prompt.contains(block) && prompt.contains("/shared/money.go#L3</ref>\n</refs>"),
            "{prompt}"
        );
        let mut bash = cases[0].clone();
        (bash.name, bash.cmd) = ("bash".into(), "bash".into());
        assert!(
            panel_call(&bash, &path, false)
                .1
                .contains("<ref form=\"bare\">src/cart.go:40-72</ref>")
        );
    }

    const OUT: &str = "=== out\nkind: bug\nwindow: bash · bash · /home/u/inv\nexpect: write\nexplore: invoicer\n\
                       --- refs\ninvoicer/money.py:1-5\n../shared/rates.py\n--- request\nround like rates.py\n";

    #[test]
    fn an_exploring_refs_case_marks_a_ref_outside_its_fixture() {
        let cases = parse_cases(OUT).unwrap();
        let d = Gone(stage(&cases).unwrap().expect("a stage"));
        let path = pane_path(&cases[0], Some(&d.0));
        let (_, prompt, _) = panel_call(&cases[0], &path, true);
        assert!(prompt.contains("<ref form=\"bare\">invoicer/money.py:1-5</ref>"), "{prompt}");
        assert!(
            prompt.contains("outside the repo, not read\">/") && prompt.contains("shared/rates.py</ref>"),
            "{prompt}"
        );
    }

    /// A fixture is shared by its cases: a ref inside one must be a file it has.
    #[test]
    fn stage_refuses_a_ref_an_explore_fixture_lacks() {
        let bad = OUT.replace("invoicer/money.py:1-5", "invoicer/nope.py");
        let e = stage(&parse_cases(&bad).unwrap()).unwrap_err();
        assert!(e.contains("invoicer/nope.py"), "{e}");
    }

    #[test]
    fn stage_copies_each_named_fixture_once_and_nothing_without_one() {
        assert_eq!(stage(&parse_cases(ONE).unwrap()).unwrap(), None);
        let one = ONE.replace("expect: write", "expect: write\nexplore: invoicer");
        let two = format!("{one}{}", one.replace("=== crash", "=== crash2"));
        let d = stage(&parse_cases(&two).unwrap()).unwrap().expect("a temp dir");
        struct Gone(PathBuf);
        impl Drop for Gone {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let d = Gone(d);
        assert!(d.0.join("invoicer/invoicer/money.py").is_file());
        assert!(!d.0.starts_with(env!("CARGO_MANIFEST_DIR")), "outside this repo");
    }

    #[test]
    fn latency_and_reads_lines_count_only_the_rows_that_have_them() {
        let row = |ms: Option<u64>, files_read: Option<usize>| Row {
            ms,
            files_read,
            ..Row::default()
        };
        let rows = [
            row(Some(1000), Some(2)),
            row(Some(3000), None),
            row(Some(2000), Some(4)),
            row(None, None),
        ];
        assert_eq!(latency_line(&rows).unwrap(), "2.0 s mean, 2.0 s median over 3 calls");
        assert_eq!(reads_line(&rows).unwrap(), "3.0 files mean, 4 at most, over 2 calls");
        assert_eq!((latency_line(&[row(None, None)]), reads_line(&[])), (None, None));
    }

    #[test]
    fn a_json_call_is_never_given_the_output_flags_twice() {
        let call = ["-p", "--output-format", "json", "--verbose"].map(OsString::from);
        let c = claude_command(OsStr::new("/bin/claude"), call, false);
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(args, ["-p", "--output-format", "json", "--verbose"]);
    }

    /// A json call's row is read from its events: the cost and the reads are there,
    /// a failed call's too; a text call's from its answer.
    #[test]
    fn a_json_call_is_graded_from_its_events() {
        let events = r#"[{"type":"result","result":"<prompt>p</prompt>","total_cost_usd":0.02}]"#;
        let out = call_output(Ok("<prompt>p</prompt>".into()), Some(events)).unwrap();
        assert_eq!(unwrap_json(&out).unwrap().1.cost_usd, Some(0.02));
        let spent = call_output(Err(Fail::Budget { limit_usd: 0.15 }), Some(events));
        assert_eq!(spent.as_deref(), Ok(events), "a stop keeps its events");
        assert_eq!(call_output(Ok("text".into()), None).as_deref(), Ok("text"));
        // claude's API error says success in its subtype: is_error decides
        let api = r#"[{"type":"result","subtype":"success","is_error":true,"result":"API Error: 400 bad request"}]"#;
        let out = call_output(Err(Fail::Failed("API Error: 400 bad request".into())), Some(api)).unwrap();
        assert_eq!(unwrap_json(&out).unwrap_err(), "API Error: 400 bad request");
        assert_eq!(call_output(Err(Fail::Timeout), None), Err("took too long".into()));
    }

    #[test]
    fn the_wrapped_call_puts_json_first_and_drops_the_variable() {
        let call = ["-p", "--model", "sonnet"].map(OsString::from);
        let c = claude_command(OsStr::new("/bin/claude"), call, false);
        assert_eq!(c.get_program(), "/bin/claude");
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(args, ["--output-format", "json", "--verbose", "-p", "--model", "sonnet"]);
        let envs: Vec<_> = c.get_envs().collect();
        assert_eq!(envs, [(OsStr::new(AS_CLAUDE), None), (OsStr::new(HERMETIC), None)]);
    }

    /// #598: a hermetic call loads no setting source, so no CLAUDE.md and no hooks.
    #[test]
    fn a_hermetic_call_loads_no_setting_source() {
        let call = ["-p", "--model", "sonnet"].map(OsString::from);
        let c = claude_command(OsStr::new("/bin/claude"), call, true);
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(args[..3], ["--output-format", "json", "--verbose"]);
        assert_eq!(args[3..5], ["--setting-sources", ""], "an empty list: no source at all");
        assert_eq!(args[5..], ["-p", "--model", "sonnet"]);
        let envs: Vec<_> = c.get_envs().collect();
        assert_eq!(envs, [(OsStr::new(AS_CLAUDE), None), (OsStr::new(HERMETIC), None)]);
    }

    #[test]
    fn a_call_that_names_its_setting_sources_keeps_them() {
        let call = ["-p", "--setting-sources", "user"].map(OsString::from);
        let c = claude_command(OsStr::new("/bin/claude"), call, true);
        let n = c.get_args().filter(|a| *a == "--setting-sources").count();
        assert_eq!(n, 1);
    }

    #[test]
    fn hermetic_is_the_default_and_no_hermetic_turns_it_off() {
        assert!(opts(args("")).unwrap().hermetic);
        assert!(!opts(args("--no-hermetic")).unwrap().hermetic);
        assert!(opts(args("--no-hermetic --runs 2")).unwrap().runs == 2);
        assert!(opts(args("--hermetic=no")).unwrap_err().contains("unknown argument --hermetic=no"));
    }

    #[test]
    fn judge_is_off_by_default_and_judge_turns_it_on() {
        let o = opts(args("")).unwrap();
        assert!(!o.judge);
        assert_eq!(o.judge_model, judge::DEFAULT_MODEL);
        let o = opts(args("--judge --judge-model opus")).unwrap();
        assert!(o.judge);
        assert_eq!(o.judge_model, "opus");
        assert_eq!(opts(args("--judge-model")).unwrap_err(), "--judge-model needs a value");
    }

    #[test]
    fn calibrate_takes_a_file_and_a_split() {
        let o = opts(args("--calibrate l.jsonl --split holdout")).unwrap();
        assert_eq!(o.calibrate, Some(PathBuf::from("l.jsonl")));
        assert_eq!(o.split, judge::Split::Holdout);
        assert_eq!(opts(args("--calibrate l.jsonl")).unwrap().split, judge::Split::All);
        assert_eq!(opts(args("--calibrate")).unwrap_err(), "--calibrate needs a value");
        assert!(opts(args("--split half")).unwrap_err().contains("want all, tune or holdout"));
    }

    #[test]
    fn judge_line_counts_agreement_only_where_both_graded() {
        let rows = [
            (Some(true), Some(true)),
            (Some(false), Some(true)),
            (None, Some(false)),
            (Some(true), None),
        ];
        let l = judge_line(&rows).unwrap();
        assert!(l.starts_with("pass 67% (2/3)"), "{l}");
        assert!(l.ends_with("agrees with the keyword checks 50% (1/2)"), "{l}");
        assert_eq!(judge_line(&[(Some(true), None)]), None);
    }

    /// #419: the judge sits beside the keyword checks: an answer either way,
    /// or none, leaves `pass` as they set it; a call with no verdict is counted.
    #[test]
    fn judge_apply_keeps_the_keyword_pass_and_counts_failed_calls() {
        let row = |pass: bool| (serde_json::json!({"id": "a", "pass": pass}), judge::Input::default());
        let mut rows = vec![row(false), row(true), row(true)];
        let v = |pass: bool| {
            Ok(judge::Verdict {
                pass,
                critique: "c".into(),
            })
        };
        let judged = [v(true), Err("exit 1".to_string()), v(false)].map(|verdict| judge::Judged { verdict, cost_usd: None });
        assert_eq!(judge_apply(&mut rows, &[0, 1, 2], &judged, "haiku"), 1);
        let got: Vec<_> = rows.iter().map(|(r, _)| (r["pass"].as_bool(), r["judge"].as_bool())).collect();
        assert_eq!(got, [(Some(false), Some(true)), (Some(true), None), (Some(true), Some(false))]);
    }

    const EXPLORED: &str = r##"[{"type":"assistant","message":{"content":[
        {"type":"tool_use","id":"a","name":"Read","input":{"file_path":"/r/CHANGELOG.md"}},
        {"type":"tool_use","id":"b","name":"Grep","input":{"pattern":"#87","path":"/r"}}]}},
        {"type":"result","subtype":"success","result":"<prompt>x</prompt>"}]"##;

    #[test]
    fn panel_reads_lists_an_exploring_calls_reads_and_searches() {
        let want = ["Read /r/CHANGELOG.md", "Grep #87 /r"].map(String::from).to_vec();
        assert_eq!(panel_reads(Some(EXPLORED), true), Some(want));
    }

    #[test]
    fn panel_reads_is_none_for_a_text_call_a_failed_one_or_plain_output() {
        assert_eq!(panel_reads(Some(EXPLORED), false), None, "a text call");
        assert_eq!(panel_reads(None, true), None, "a failed call");
        assert_eq!(panel_reads(Some("<prompt>x</prompt>"), true), None, "not the json events");
    }

    #[test]
    fn judge_input_carries_the_panel_reads_into_the_prompt() {
        let c = &parse_cases(ONE).unwrap()[0];
        let i = judge_input(c, "r", panel_reads(Some(EXPLORED), true));
        let want = "<panel_reads>\nRead /r/CHANGELOG.md\nGrep #87 /r\n</panel_reads>";
        assert!(judge::prompt(&i).contains(want));
        assert!(!judge::prompt(&judge_input(c, "r", None)).contains("<panel_reads>"));
    }

    /// The checked-in labels carry no reads: their judge prompts stay as before #628.
    #[test]
    fn no_owner_label_carries_reads() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/owner-labels.jsonl");
        let l = judge::read_labels("owner-labels", &std::fs::read_to_string(p).unwrap()).unwrap();
        assert!(l.iter().all(|l| l.input.reads.is_none()));
        assert!(!l.iter().any(|l| judge::prompt(&l.input).contains("<panel_reads>")));
    }

    /// #628 round trip: a stand-in judge that fails an option naming a PR found
    /// in neither the screen nor the panel's reads, given a rubric whose rule 7
    /// weighs those reads. The real judge's grading is the calibration's job.
    #[test]
    fn a_stand_in_judge_fails_a_pr_named_in_neither_the_screen_nor_the_reads() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("judge-reads");
        let fake = root.join("fake-claude");
        let script = "#!/bin/sh\nwhile [ \"$1\" != --system-prompt-file ]; do shift; done\n\
            grep -q \"the panel's reads\" \"$2\" || exit 3\n\
            p=$(cat); r=$(printf '%s' \"$p\" | sed -n '/^<panel_reads>$/,/^<\\/panel_reads>$/p')\n\
            case \"$p\" in *'<reply>'*'#871'*) ;; *) exit 4;; esac\n\
            case \"$r\" in *Read*) ;; *) exit 5;; esac\n\
            case \"$r\" in *'#871'*) echo 'CRITIQUE: read it'; echo 'VERDICT: PASS';;\n\
            *) echo 'CRITIQUE: #871 invented'; echo 'VERDICT: FAIL';; esac\n";
        std::fs::write(&fake, script).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let c = &parse_cases(ONE).unwrap()[0];
        let reply = "<question>Which PR?</question>\n<option>#871</option>\n<option>#872</option>";
        let i = judge_input(c, reply, panel_reads(Some(EXPLORED), true));
        let got = judge::judge_all(&fake.display().to_string(), "haiku", &[i], 1, &root);
        let v = got[0].verdict.clone().unwrap();
        assert_eq!((v.pass, v.critique.as_str()), (false, "#871 invented"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The judge sees neither a case's basket nor its earlier rounds, so it
    /// gets no reads there and rule 7 stays lenient (review645).
    #[test]
    fn judge_input_withholds_reads_for_a_case_with_refs_or_earlier_rounds() {
        for (name, text) in [("refs", REFS), ("rounds", ANSWERED)] {
            let c = &parse_cases(text).unwrap()[0];
            let i = judge_input(c, "r", panel_reads(Some(EXPLORED), true));
            assert_eq!(i.reads, None, "{name}");
        }
    }

    #[test]
    fn judge_input_carries_the_case_window_and_output_type() {
        let c = &parse_cases(ONE).unwrap()[0];
        let i = judge_input(c, "<prompt>x</prompt>", None);
        assert_eq!(i.window, format!("{} · {} · {}", c.name, c.cmd, c.path));
        assert_eq!((i.out.as_str(), i.request.as_str()), (out_name(c.out), c.request.as_str()));
        assert_eq!(i.reply, "<prompt>x</prompt>");
    }

    #[test]
    fn the_checked_in_labels_read_as_118_rows_20_of_them_fails() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/owner-labels.jsonl");
        let l = judge::read_labels("owner-labels", &std::fs::read_to_string(p).unwrap()).unwrap();
        assert_eq!((l.len(), l.iter().filter(|l| !l.pass).count()), (118, 20));
    }

    /// Each field where a label's input is not its case's, as (key, field): the
    /// screen, repo and request, each only where the case has one. A label whose
    /// case is in neither list is not compared.
    fn label_drift(cases: &[Case], labels: &[judge::Label]) -> Vec<(String, &'static str)> {
        let mut out = vec![];
        for l in labels {
            let Some(c) = cases.iter().find(|c| c.id == l.case) else {
                continue;
            };
            let i = &l.input;
            for (f, want, got) in [
                ("screen", &c.screen, &i.screen),
                ("repo", &c.repo, &i.repo),
                ("request", &c.request, &i.request),
            ] {
                if !want.is_empty() && want != got {
                    out.push((l.key.clone(), f));
                }
            }
        }
        out
    }

    /// One label row on the case `crash` of ONE (screen "$ net-proxy\nboom",
    /// request "fix this", no repo).
    fn crash_label(case: &str, screen: &str, repo: &str, request: &str) -> judge::Label {
        let j = serde_json::json!({"key": "k", "case": case, "verdict": "pass", "reply": "r",
            "screen": screen, "repo": repo, "request": request});
        judge::read_labels("t", &j.to_string()).unwrap().remove(0)
    }

    #[test]
    fn label_drift_names_each_field_that_differs_from_a_field_the_case_has() {
        let cases = parse_cases(ONE).unwrap();
        let s = "$ net-proxy\nboom";
        let rows: [(&str, judge::Label, &[&str]); 6] = [
            ("same", crash_label("crash", s, "", "fix this"), &[]),
            ("lost a # line", crash_label("crash", "$ net-proxy", "", "fix this"), &["screen"]),
            ("request", crash_label("crash", s, "", "fix it"), &["request"]),
            ("repo the case lacks", crash_label("crash", s, "x", "fix this"), &[]),
            ("both", crash_label("crash", "", "", ""), &["screen", "request"]),
            ("no such case", crash_label("gone", "", "", ""), &[]),
        ];
        for (name, l, want) in rows {
            let got: Vec<&str> = label_drift(&cases, &[l]).into_iter().map(|(_, f)| f).collect();
            assert_eq!(got, want, "{name}");
        }
        assert_eq!(label_drift(&cases, &[]), vec![]);
    }

    /// The judge's calibration grades each owner label against the screen,
    /// repo and request the row stores, so each must be its case's, read by
    /// the rig's own parser: a builder that took `#91 ...` for a comment once
    /// dropped review-which-pr's PR list.
    #[test]
    fn each_owner_label_carries_its_cases_screen_repo_and_request() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel");
        let read = |f: &str| std::fs::read_to_string(root.join(f)).unwrap();
        let mut cases = parse_cases(&read("cases.txt")).unwrap();
        cases.extend(parse_cases(&read("explore.txt")).unwrap());
        let labels = judge::read_labels("owner-labels", &read("owner-labels.jsonl")).unwrap();
        let unmatched = labels.iter().filter(|l| !cases.iter().any(|c| c.id == l.case)).count();
        assert_eq!(unmatched, 0, "every label's case is in cases.txt or explore.txt");
        assert_eq!(label_drift(&cases, &labels), vec![]);
    }

    /// The round trip: judge_all hands a stand-in claude the rubric as its
    /// system prompt file and the case on stdin, and reads its verdict back.
    #[test]
    fn judge_all_reads_back_a_stand_in_verdict() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("judge-trip");
        let fake = root.join("fake-claude");
        let script = "#!/bin/sh\nwhile [ \"$1\" != --system-prompt-file ]; do shift; done\n\
            grep -q 'exactly 2 options' \"$2\" || exit 3\n\
            grep -q 'deploy it' && echo 'CRITIQUE: saw it' && echo 'VERDICT: FAIL'\n";
        std::fs::write(&fake, script).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let i = judge::Input {
            request: "deploy it".into(),
            reply: "r".into(),
            ..Default::default()
        };
        let got = judge::judge_all(&fake.display().to_string(), "haiku", &[i], 1, &root);
        let v = got[0].verdict.clone().unwrap();
        assert_eq!((v.pass, v.critique.as_str()), (false, "saw it"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panel-eval-{name}.{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn base_call(dir: &Path) -> Call {
        let timeout = Duration::from_secs(30);
        Call {
            claude: "claude".into(),
            timeout,
            dir: dir.into(),
            effort: Effort::Medium,
        }
    }

    /// #598: each hermetic call gets an empty folder of its own; a plain one the panel's.
    #[test]
    fn a_hermetic_call_runs_in_a_fresh_folder_of_its_own() {
        let root = scratch("cwds");
        let base = base_call(Path::new("/panel/cache"));
        let a = call_in(&base, Some(&root), 0, 0).unwrap();
        let b = call_in(&base, Some(&root), 0, 1).unwrap();
        assert_ne!(a.dir, b.dir);
        assert!(a.dir.starts_with(&root) && b.dir.starts_with(&root));
        assert_eq!(std::fs::read_dir(&a.dir).unwrap().count(), 0, "a fresh folder is empty");
        let again = call_in(&base, Some(&root), 0, 0).unwrap_err();
        assert!(again.contains("case0-run0"), "never reused: {again}");
        assert_eq!(call_in(&base, None, 0, 0).unwrap().dir, base.dir);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The round trip: the panel's own Request, run on a hermetic call, starts
    /// claude in that call's folder (a stand-in claude that prints its cwd).
    #[test]
    fn the_panels_request_runs_claude_in_the_hermetic_folder() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("cwd-trip");
        let fake = root.join("fake-claude");
        std::fs::write(&fake, "#!/bin/sh\ncat >/dev/null\npwd -P\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let base = Call {
            claude: fake.display().to_string(),
            ..base_call(&root)
        };
        let call = call_in(&base, Some(&root), 3, 1).unwrap();
        let mut req = Request::spawn_with(&call, &Profile::TEXT, "sys", "hi", "sonnet");
        let got = loop {
            match req.poll() {
                Some(a) => break a,
                None => std::thread::yield_now(),
            }
        };
        let want = std::fs::canonicalize(root.join("case3-run1")).unwrap();
        assert_eq!(got.unwrap().trim(), want.display().to_string());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn only_a_1_makes_the_wrapped_call_hermetic() {
        let rows = [
            ("set to 1", Some("1"), true),
            ("unset", None, false),
            ("set to 0", Some("0"), false),
            ("empty", Some(""), false),
        ];
        for (name, var, want) in rows {
            assert_eq!(hermetic_wanted(var.map(OsString::from)), want, "{name}");
        }
    }

    /// #598 review: what main sets is what the wrapped call reads. The only test
    /// that touches AS_CLAUDE and HERMETIC in this process's environment.
    #[test]
    fn the_hand_off_reaches_the_wrapped_call() {
        let flag = |c: &std::process::Command| c.get_args().any(|a| a == HERMETIC_FLAGS[0]);
        for (hermetic, want) in [(true, true), (false, false), (true, true)] {
            // SAFETY: no other test reads or writes these variables
            assert_eq!(unsafe { hand_off("/bin/claude", hermetic) }, want);
            let real = as_claude(std::env::var_os(AS_CLAUDE), Some(OsStr::new("-p"))).unwrap();
            assert_eq!(real, "/bin/claude");
            let c = from_env(&real, [OsString::from("-p")]);
            assert_eq!(flag(&c), want, "hermetic {hermetic}");
        }
    }

    /// #598 review: a baseline from before the env field, compared with a hermetic run.
    #[test]
    fn compare_reads_each_rows_env_without_its_cwd_and_tells_them_apart() {
        let row =
            |id: &str, cwd: &str| format!(r#"{{"id":"{id}","pass":true,"env":{{"claude":"2.1.319","cwd":"{cwd}","hermetic":true}}}}"#);
        let herm = [row("a", "/t/cwd-7/case0-run0"), row("b", "/t/cwd-7/case1-run0")].join("\n");
        let rows = read_rows("H", &herm).unwrap();
        assert_eq!(rows[0].env.as_ref().unwrap()["cwd"], serde_json::Value::Null, "the cwd names a pid");
        assert_eq!(envs_of(&rows).len(), 1, "one environment for the whole run");
        let old = read_rows("O", "{\"id\":\"a\",\"pass\":true}\n").unwrap();
        assert_eq!(envs_of(&old), [None]);
        assert!(envs_differ(&old, &rows) && !envs_differ(&rows, &rows));
    }

    #[test]
    fn the_fingerprint_names_the_version_the_cwd_the_sources_and_the_vars() {
        let var = |k: &str| (k == "HOME").then(|| "/home/u".to_string());
        let f = fingerprint(
            "2.1.319 (Claude Code)",
            true,
            Some(Path::new("/t/cwd")),
            Path::new("/c"),
            Effort::Low,
            var,
        );
        assert_eq!(
            (f.claude.as_str(), f.cwd.as_str()),
            ("2.1.319 (Claude Code)", "/t/cwd/<case>-<run>")
        );
        assert_eq!((f.hermetic, f.setting_sources.as_deref()), (true, Some("")));
        assert_eq!(f.vars[0], ("HOME", Some("/home/u".into())));
        assert_eq!(f.vars[3], ("CLAUDE_CONFIG_DIR", None));
        let plain = fingerprint("v", false, None, Path::new("/c"), Effort::Medium, var);
        assert_eq!((plain.hermetic, plain.cwd.as_str()), (false, "/c"));
        assert_eq!(f.json()["vars"]["XDG_CONFIG_HOME"], serde_json::Value::Null);
    }

    /// #623: the panel's own call loads no setting source, so a plain run does not either.
    #[test]
    fn a_plain_run_loads_the_setting_sources_the_panel_does() {
        let plain = fingerprint("v", false, None, Path::new("/c"), Effort::Medium, |_: &str| None);
        assert_eq!(plain.setting_sources.as_deref(), Some(""));
    }

    /// The effort the calls get, from COCKPIT_PANEL_EFFORT, is in the fingerprint.
    #[test]
    fn the_fingerprint_names_the_effort() {
        let f = fingerprint("v", true, None, Path::new("/c"), Effort::XHigh, |_: &str| None);
        assert_eq!(f.json()["effort"], "xhigh");
    }

    #[test]
    fn the_variable_is_honoured_only_for_the_call_argv() {
        let real = || Some(OsString::from("/bin/claude"));
        assert_eq!(as_claude(real(), Some(OsStr::new("-p"))), real());
        assert_eq!(as_claude(real(), Some(OsStr::new("--check"))), None);
        assert_eq!(as_claude(real(), Some(OsStr::new("--compare"))), None);
        assert_eq!(as_claude(real(), None), None);
        assert_eq!(as_claude(None, Some(OsStr::new("-p"))), None);
    }

    #[test]
    fn paired_pass_all_counts_only_cases_both_sides_graded_in_full() {
        let c = |id: &str, a, b| Cmp {
            id: id.into(),
            expect: "ask".into(),
            a,
            b,
        };
        let cmp = [
            c("x", Some((3, 3, 0)), Some((2, 3, 0))),
            c("w", Some((3, 3, 0)), Some((3, 3, 0))),
            c("only-a", Some((3, 3, 0)), None),
            c("err-b", Some((0, 3, 0)), Some((2, 2, 1))),
        ];
        assert_eq!(paired_pass_all(&cmp), (Of(2, 2), Of(1, 2)));
        assert_eq!(paired_pass_all(&[]), (Of(0, 0), Of(0, 0)));
    }

    #[test]
    fn the_shipped_cases_are_valid() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/cases.txt");
        let cases = parse_cases(&std::fs::read_to_string(p).unwrap()).unwrap();
        assert!(cases.len() >= 25, "{}", cases.len());
        assert!(cases.iter().any(|c| c.expect == Expect::Ask));
        assert!(cases.iter().any(|c| c.expect == Expect::Write));
        let leaked: Vec<&str> = cases.iter().filter(|c| c.request.contains("\n#")).map(|c| c.id.as_str()).collect();
        assert!(leaked.is_empty(), "a comment in a request: {leaked:?}");
        // Every case ends in its request, and the old strip dropped every trailing `#`
        // line of it: no request ends in one, so the old and new strips read this file alike.
        let tail: Vec<&str> = cases
            .iter()
            .filter(|c| c.request.lines().last().is_some_and(|l| l.starts_with('#')))
            .map(|c| c.id.as_str())
            .collect();
        assert!(tail.is_empty(), "a request ends in a # line: {tail:?}");
    }

    fn shipped(id: &str) -> Case {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/cases.txt");
        let cases = parse_cases(&std::fs::read_to_string(p).unwrap()).unwrap();
        cases.into_iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no case {id}"))
    }

    fn ask(q: &str, a: &str, b: &str) -> String {
        format!("<question>{q}</question>\n<option>{a}</option>\n<option>{b}</option>")
    }

    /// #558: a big feature's goal question is on target; a question on how to
    /// build it ("which store", "which library", "which channel") is not.
    #[test]
    fn a_goal_question_passes_the_big_feature_cases_and_an_approach_question_does_not() {
        let rows = [
            ("goal rate", "h-rate-limit", "What is the rate limiting for?", true),
            (
                "goal notify",
                "h-feature-done",
                "What should the notifications tell users about?",
                true,
            ),
            ("goal audit", "h-audit-log", "What is the audit log mainly for?", true),
            (
                "store",
                "h-rate-limit",
                "Which store should hold the counters for the limiter?",
                false,
            ),
            ("library", "h-audit-log", "Which library should we use for it?", false),
            ("channel", "h-feature-done", "Which channel should we use for them?", false),
        ];
        let wrong: Vec<&str> = rows
            .iter()
            .filter(|(_, id, q, want)| grade(&shipped(id), &ask(q, "Yes", "No")).pass != *want)
            .map(|(row, ..)| *row)
            .collect();
        assert_eq!(wrong, Vec::<&str>::new(), "rows graded the wrong way");
    }

    /// #514: h-node-lts takes a report that proposes the edits, never a prompt
    /// that makes them.
    #[test]
    fn h_node_lts_passes_a_report_that_proposes_edits_and_fails_one_that_edits() {
        let c = shipped("h-node-lts");
        let report = "<prompt>Find the current Node LTS, cite nodejs.org as the source, and list the \
                      lines to change in the Dockerfile. Do not edit any files.</prompt>";
        let edit = "<prompt>Find the current Node LTS, cite nodejs.org as the source, update the \
                    Dockerfile to it and list the release lines you checked.</prompt>";
        assert!(grade(&c, report).pass, "report-only");
        assert!(!grade(&c, edit).pass, "editing");
    }

    /// The reply of the checked-in owner label `key`.
    fn owner_reply(key: &str) -> String {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/owner-labels.jsonl");
        let l = judge::read_labels("owner-labels", &std::fs::read_to_string(p).unwrap()).unwrap();
        l.into_iter()
            .find(|l| l.key == key)
            .map(|l| l.input.reply)
            .unwrap_or_else(|| panic!("no label {key}"))
    }

    /// #626: a feature that comes with its test fails on a break-it step, in any
    /// of its wordings; the owner-passed replies still pass.
    #[test]
    fn a_feature_with_its_test_fails_on_a_break_it_step() {
        let brk = "\nMake one small change to the code on purpose so the new test fails, then undo the change.";
        let with = |r: &str| r.replacen("\n</prompt>", &format!("{brk}\n</prompt>"), 1);
        let (v2, v3) = (owner_reply("json-flag.r0.v2"), owner_reply("json-flag.r0.v3"));
        let csv = "<prompt>\nFirst check that the json export works, then add csv next to it and test both.\n</prompt>";
        let rows = [
            ("owner json-flag.r0.v2", "json-flag", v2, true),
            ("owner json-flag.r0.v3", "json-flag", v3.clone(), true),
            ("json-flag.r0.v3 plus a break", "json-flag", with(&v3), false),
            ("csv first", "small-add-format", csv.to_string(), true),
            ("csv first plus a break", "small-add-format", with(csv), false),
        ];
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "rows graded the wrong way");
    }

    /// Rows of (row name, case, reply, should pass) graded against the shipped
    /// cases: the names of the rows graded the other way.
    fn graded_wrong<'a>(rows: &[(&'a str, &str, String, bool)]) -> Vec<&'a str> {
        rows.iter()
            .filter(|(_, id, reply, want)| grade(&shipped(id), reply).pass != *want)
            .map(|(row, ..)| *row)
            .collect()
    }

    /// #587 (card 2): generated config asks its kind before any field.
    #[test]
    fn a_config_case_passes_a_kind_question_and_fails_a_field_question() {
        let fmt = "<format lang=\"yaml\">services:\n  web:\n    ports: [\"8080:8080\"]</format>";
        let rows = [
            (
                "format",
                "yaml-service",
                ask("Which YAML format do you need?", "Kubernetes", "Docker Compose"),
                true,
            ),
            (
                "platform",
                "yaml-service",
                ask("Where will it run?", "Kubernetes cluster", "Docker host"),
                true,
            ),
            (
                "image",
                "yaml-service",
                ask("Which image should the web service run?", "nginx", "node:20"),
                false,
            ),
            (
                "labels",
                "yaml-service",
                ask("Which labels should it carry?", "app: web", "tier: frontend"),
                false,
            ),
            (
                "spec",
                "yaml-service",
                ask("Which namespace and metadata should the Deployment spec use?", "default", "web"),
                false,
            ),
            ("written", "yaml-service", fmt.to_string(), false),
        ];
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "rows graded the wrong way");
    }

    /// #587 (card 10): a message asks for the fact its reader needs, never
    /// promises it later.
    #[test]
    fn a_slip_message_passes_a_date_question_and_fails_a_date_to_follow() {
        let later = "<text>Hi Dana, the release slips. I'll let you know the new date once it's set.</text>";
        let rows = [
            (
                "date",
                "release-slips",
                ask("Is there a new release date to put in the message?", "Yes", "Not yet"),
                true,
            ),
            ("later", "release-slips", later.to_string(), false),
            ("tone", "release-slips", ask("How formal should it be?", "Casual", "Formal"), false),
            (
                "send time",
                "release-slips",
                ask("When should I send it?", "Now", "Tomorrow"),
                false,
            ),
        ];
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "rows graded the wrong way");
    }

    /// #590: a baseline run on a rename passes both rename cases (the owner
    /// passed one), and rename-fn's mutation words miss "kept on purpose".
    #[test]
    fn a_rename_passes_with_a_baseline_and_a_kept_on_purpose() {
        let fn_rename = "Rename parse_cfg to parse_config everywhere and run the tests.";
        let bare = "In internal/db/pool.go:10-30, rename newPool to openPool and update its callers.";
        let baseline = " Run the tests first as a baseline and compare after.";
        let kept = " List any parse_cfg you kept on purpose and why.";
        let p = |s: String| format!("<prompt>{s}</prompt>");
        let rows = [
            ("fn plain", "rename-fn", p(fn_rename.into()), true),
            ("fn baseline", "rename-fn", p(format!("{fn_rename}{baseline}")), true),
            ("fn kept on purpose", "rename-fn", p(format!("{fn_rename}{kept}")), true),
            (
                "fn mutation step",
                "rename-fn",
                p(format!("{fn_rename} Break the code on purpose, see a test fail, undo it.")),
                false,
            ),
            ("bare plain", "refs-bare-rename", p(bare.into()), true),
            (
                "bare given an @",
                "refs-bare-rename",
                p(bare.replace("In internal", "In @internal")),
                false,
            ),
            ("bare baseline", "refs-bare-rename", p(format!("{bare}{baseline}")), true),
        ];
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "rows graded the wrong way");
    }

    /// #587 (cards 58, 59): a prompt that adds or fixes a test breaks the code on
    /// purpose, sees the test fail and undoes the break.
    #[test]
    fn a_test_prompt_passes_with_the_mutation_step_and_fails_without_it() {
        let add = "Add a test for slugify in src/text/slugify.ts that covers the trimmed title and run it.";
        let fix = "Fix parser::tests::parses_negative in src/parser.rs from 9f3c2a1, then run cargo test.";
        let step = " Then break the code on purpose, confirm the test fails, and undo the break.";
        let half = " Then break the code on purpose and confirm the test fails.";
        let flip = " Then flip one condition in the code, confirm the test fails, then undo that change.";
        let loose = " Make sure it does not break other tests, keep it a small change, and revert unrelated edits.";
        // #590: valid steps the #589 words missed
        let bug = " Then introduce a small bug in the code, confirm the test fails, and revert it.";
        let one_bug = " Then introduce a bug, see the test fail, and revert it.";
        let invert = " Then temporarily invert the trim check, confirm the test fails, and put it back.";
        let inverting = " Then try inverting the sign check: the test must fail; then restore it.";
        // #599: a bug described, or a bug the fix must not bring, is no break step
        let described = format!(" The parser is inverting the leading minus.{loose}");
        let was = " slugify was inverting the trim check; revert unrelated edits.";
        let do_not = " Make sure the fix does not introduce a bug, and revert unrelated edits.";
        let without = " Keep it small, without introducing a bug, and revert unrelated edits.";
        // #678 review: a do-not phrase or a description that runs into the act's words
        let do_not_and = " Make sure the fix does not introduce a bug and confirm the full suite \
                          passes, and revert unrelated edits.";
        let flipping = " The parser is flipping the leading minus; revert unrelated edits.";
        let flip_the = " The bug makes the parser flip the sign; revert unrelated edits.";
        let we_invert = " We invert the sign twice in the parser; revert unrelated edits.";
        let no_invert = " Do not invert the logic, and revert unrelated edits.";
        let bug_in = " Then introduce a bug in parse_negative, confirm the test fails, and revert it.";
        let p = |s: String| format!("<prompt>{s}</prompt>");
        let rows = [
            ("add", "test-the-changed-function", p(format!("{add}{step}")), true),
            ("add bare", "test-the-changed-function", p(add.into()), false),
            ("add no undo", "test-the-changed-function", p(format!("{add}{half}")), false),
            ("add flip", "test-the-changed-function", p(format!("{add}{flip}")), true),
            ("add loose", "test-the-changed-function", p(format!("{add}{loose}")), false),
            ("fix", "last-commit-test", p(format!("{fix}{step}")), true),
            ("fix bare", "last-commit-test", p(fix.into()), false),
            ("fix no undo", "last-commit-test", p(format!("{fix}{half}")), false),
            ("fix loose", "last-commit-test", p(format!("{fix}{loose}")), false),
            ("add small bug", "test-the-changed-function", p(format!("{add}{bug}")), true),
            ("add invert", "test-the-changed-function", p(format!("{add}{invert}")), true),
            ("fix a bug", "last-commit-test", p(format!("{fix}{one_bug}")), true),
            ("fix inverting", "last-commit-test", p(format!("{fix}{inverting}")), true),
            ("fix describes inverting", "last-commit-test", p(format!("{fix}{described}")), false),
            (
                "add describes inverting",
                "test-the-changed-function",
                p(format!("{add}{was}")),
                false,
            ),
            ("fix does not introduce", "last-commit-test", p(format!("{fix}{do_not}")), false),
            // a guard row: master's words fail it too
            (
                "add without introducing (guard)",
                "test-the-changed-function",
                p(format!("{add}{without}")),
                false,
            ),
            (
                "fix does not introduce and confirm",
                "last-commit-test",
                p(format!("{fix}{do_not_and}")),
                false,
            ),
            ("fix is flipping", "last-commit-test", p(format!("{fix}{flipping}")), false),
            ("fix flip the sign", "last-commit-test", p(format!("{fix}{flip_the}")), false),
            ("fix we invert the", "last-commit-test", p(format!("{fix}{we_invert}")), false),
            ("fix do not invert", "last-commit-test", p(format!("{fix}{no_invert}")), false),
            ("fix then a bug in", "last-commit-test", p(format!("{fix}{bug_in}")), true),
        ];
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "rows graded the wrong way");
    }

    /// #599: the holdouts that share the break words take a break step and
    /// refuse a described bug or a do-not phrase.
    #[test]
    fn a_holdout_test_prompt_takes_only_a_break_on_purpose() {
        let py = "Write tests for parse_price in shop/prices.py and run pytest -q.";
        let go = "Fix TestParseHeader in header/header_test.go and run go test ./header.";
        let invert = " Then invert the currency check, confirm a test fails, and put it back.";
        let bug = " Then introduce a small bug in it, confirm the test fails, and revert it.";
        let described = " The header code is inverting the key case; revert unrelated edits.";
        let do_not = " Make sure the fix does not introduce a bug, and revert unrelated edits.";
        let p = |s: String| format!("<prompt>{s}</prompt>");
        let rows = [
            ("py invert", "h-pytest-new-helper", p(format!("{py}{invert}")), true),
            ("py does not introduce", "h-pytest-new-helper", p(format!("{py}{do_not}")), false),
            ("go small bug", "h-go-header-test", p(format!("{go}{bug}")), true),
            ("go describes inverting", "h-go-header-test", p(format!("{go}{described}")), false),
        ];
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "rows graded the wrong way");
    }
}
