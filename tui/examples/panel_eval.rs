//! The write prompt's evaluation (#410): labelled cases through the panel's own
//! prompt, envelope, model route, `claude -p` call and reply parser, scored on
//! whether it asks when it should and carries the facts it must.
//!
//! It calls `claude` once per case and run, so it needs the network and costs
//! money: run it by hand, never from `make test`.
//!
//!     cargo run --example panel_eval -- [--model M] [--runs N] [--jobs J]
//!         [--only TEXT] [--cases FILE] [--out FILE.jsonl] [--check] [--no-explore]
//!         [--no-hermetic] [--judge] [--judge-model M] [--ledger FILE] [--show-holdout]
//!         [--router-prompt FILE]
//!     cargo run --example panel_eval -- --compare A.jsonl B.jsonl [--allow-mixed]
//!     cargo run --example panel_eval -- --calibrate LABELS.jsonl
//!         [--judge-model M] [--split all|tune|holdout] [--jobs J] [--out FILE.jsonl]
//!     cargo run --example panel_eval -- --overlap [--base REV | --from FILE --to FILE]
//!         [--judge-model M] [--only TEXT] [--jobs J] [--ledger FILE]
//!
//! `--check` only reads and validates the cases. To compare a prompt edit, put
//! it at `$XDG_CONFIG_HOME/tmux-cockpit/prompts/write.txt` (the panel's own
//! override) and run twice with `--out`, then `--compare` the two files: it
//! reads them, makes no calls, and pairs the runs by case.
//!
//! `--router-prompt FILE` sends FILE's text as every call's system prompt in
//! place of the write prompt (#789). Each row's `prompt` and `system` then hash
//! FILE's text, so `--compare` tells its rows from a write run's. `--compare`,
//! `--calibrate`, `--purpose-calibrate` and `--overlap` send no panel prompt
//! and refuse the flag. `--check` reads and validates FILE too: it is the
//! flag's pre-flight.
//!
//! The cases are evals/panel/cases.txt and, beside it, the held-out file
//! evals/panel/holdout-blind.txt (#783), which tuning sessions do not open:
//! every case in it says `holdout: yes`, none shares an id with cases.txt, and
//! it may hold none. `--check` prints each file with its count; `--cases FILE`
//! reads only FILE.
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
use cockpit_tui::panel::prompts::{self, fnv};
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
#[path = "panel_eval/overlap.rs"]
mod overlap;
#[path = "panel_eval/purpose.rs"]
mod purpose;

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
/// `--- request` bodies. `#` lines outside a body are comments. At least one case.
fn parse_cases(text: &str) -> Result<Vec<Case>, String> {
    let cases = parse_some(text, false)?;
    if cases.is_empty() {
        return Err("no cases".into());
    }
    Ok(cases)
}

/// [`parse_cases`] for a file that may hold none: the held-out file ships empty.
/// A `quiet` error (#783) gives the line and the rule it broke, never the
/// line's text: a held-out case's text stays out of test and build logs. A
/// case whose keys mark it held out is quiet in any file (#797). A
/// rule of the whole case names the case's `===` line. Case ids stay in: a
/// quiet file's ids must be names (lowercase letters, digits and `-`), so a
/// body line that opens with `=== ` cannot pass case text off as one.
fn parse_some(text: &str, quiet: bool) -> Result<Vec<Case>, String> {
    // a mistyped `=== id` line would join the case before it as text (#783):
    // in the held-out file only, where a lost case changes no count anyone reads;
    // elsewhere a pytest banner is a real screen line
    let rule = "a line that starts with = is a case's === line";
    let bad = text.lines().position(|l| l.starts_with('=') && !l.starts_with("=== "));
    if let Some(i) = bad.filter(|_| quiet) {
        return Err(format!("line {}: {rule}", i + 1));
    }
    let mut cases = Vec::new();
    let mut next = 1;
    for (n, block) in text.split("\n=== ").enumerate() {
        // the block's first line in the file
        let line0 = next;
        next += block.matches('\n').count() + 1;
        let block = if n == 0 {
            match block.strip_prefix("=== ") {
                Some(b) => b,
                None => {
                    preamble(block, quiet)?;
                    continue;
                }
            }
        } else {
            block
        };
        let (id, rest) = block.split_once('\n').unwrap_or((block, ""));
        let id = id.trim().to_string();
        // a body line opening with `=== ` reads as a case: in the held-out file
        // its "id" may be case text, so one that is not a name goes unshown
        let name = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
        if quiet && (id.is_empty() || !id.chars().all(name)) {
            return Err(format!("line {line0}: an id is lowercase letters, digits and -"));
        }
        let rest = format!("\n{rest}");
        let (head, bodies) = rest.split_once("\n--- ").unwrap_or((&rest, ""));
        // a held-out case in a loud file is quiet too (#797), though its
        // `holdout:` line may come after the bad one
        let quiet = quiet || held_head(head);
        let err = |m: &str| format!("case {id}: {m}");
        // `loud` holds the file's text; a quiet error says only where and why
        let say = |at: usize, rule: &str, loud: String| {
            let short = format!("line {}: {rule}", line0 + at);
            err(if quiet { &short } else { &loud })
        };
        // a rule whose message holds no text of the file: quiet adds only the line
        let broke = |at: usize, rule: &str| say(at, rule, rule.to_string());
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
        for (at, line) in head.lines().enumerate().map(|(at, l)| (at, l.trim())) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = line
                .split_once(':')
                .ok_or_else(|| say(at, "not key: value", format!("not key: value: {line}")))?;
            let v = v.trim();
            if seen.iter().any(|s| s == k.trim()) {
                return Err(broke(at, &format!("{} given twice", k.trim())));
            }
            seen.push(k.trim().into());
            match k.trim() {
                "kind" => c.kind = v.into(),
                "window" => {
                    let f: Vec<&str> = v.split(" · ").collect();
                    let [name, cmd, path] = f[..] else {
                        return Err(broke(at, "window is name · cmd · path"));
                    };
                    (c.name, c.cmd, c.path) = (name.into(), cmd.into(), path.into());
                }
                "out" => {
                    let bad = || say(at, "unknown out", format!("unknown out {v}"));
                    c.out = out_type(v).ok_or_else(bad)?
                }
                "out_is" => {
                    let t = out_type(v).filter(|t| *t != OutType::Auto);
                    let bad = || say(at, "unknown out_is", format!("unknown out_is {v}"));
                    c.out_is = Some(t.ok_or_else(bad)?)
                }
                "expect" => {
                    expect = Some(match v {
                        "ask" => Expect::Ask,
                        "write" => Expect::Write,
                        "either" => Expect::Either,
                        _ => return Err(broke(at, "expect is ask, write or either")),
                    })
                }
                "ask_any" => c.ask_any = list(v),
                "must" => c.must = list(v),
                "must_not" => c.must_not = list(v),
                "holdout" => {
                    c.holdout = match v {
                        "yes" => true,
                        "no" => false,
                        _ => return Err(broke(at, "holdout is yes or no")),
                    }
                }
                "explore" => {
                    // one folder name: a path would copy something outside fixtures/
                    if v.is_empty() || v.starts_with('.') || v.contains('/') {
                        return Err(broke(at, "explore is a folder name under fixtures/"));
                    }
                    c.explore = Some(v.into())
                }
                "note" => {}
                k => return Err(say(at, "unknown key", format!("unknown key {k}"))),
            }
        }
        c.expect = expect.ok_or_else(|| broke(0, "no expect"))?;
        // each body with its `--- name` line's index in `rest`
        let mut at = head.matches('\n').count() + 1;
        let mut parts: Vec<(usize, &str)> = vec![];
        for b in bodies.split("\n--- ") {
            if !b.is_empty() {
                parts.push((at, b));
            }
            at += b.matches('\n').count() + 1;
        }
        for (i, &(at, body)) in parts.iter().enumerate() {
            let (name, text) = body.split_once('\n').unwrap_or((body, ""));
            let mut lines: Vec<&str> = text.lines().collect();
            if i + 1 == parts.len() {
                strip_trailing_comments(&mut lines);
            }
            let text = lines.join("\n");
            let dup = |set: bool| {
                if set {
                    Err(broke(at, &format!("{} given twice", name.trim())))
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
                    let rule = "conversation is user: and assistant: lines";
                    c.turns = turns(&text).map_err(|m| say(at, rule, m))?
                }
                "refs" => {
                    dup(!c.refs.is_empty())?;
                    let rule = "refs are path, path:a or path:a-b lines";
                    c.refs = case_refs(&text).map_err(|m| say(at, rule, m))?
                }
                x => return Err(say(at, "unknown section", format!("unknown section {x}"))),
            }
        }
        if c.request.is_empty() {
            return Err(broke(0, "no request"));
        }
        if c.kind.is_empty() || c.cmd.is_empty() {
            return Err(broke(0, "needs kind and window"));
        }
        if c.expect == Expect::Ask && !c.must.is_empty() {
            return Err(broke(0, "must is for write cases"));
        }
        if c.expect == Expect::Ask && c.out_is.is_some() {
            return Err(broke(0, "out_is is for write or either cases"));
        }
        if c.expect == Expect::Write && !c.ask_any.is_empty() {
            return Err(broke(0, "ask_any is for ask cases"));
        }
        if cases.iter().any(|o: &Case| o.id == c.id) {
            return Err(broke(0, "duplicate id"));
        }
        cases.push(c);
    }
    Ok(cases)
}

/// Whether a case's key lines, `head`, hold a `holdout` key whose value is not
/// `no` (#797). A value that is neither yes nor no counts as held: it is an
/// error anyway, and a quoted line from a case meant to be held out leaks.
fn held_head(head: &str) -> bool {
    let mut keys = head.lines().filter_map(|l| l.trim().split_once(':'));
    keys.any(|(k, v)| k.trim() == "holdout" && v.trim() != "no")
}

/// The lines before a file's first case are comments or blank (#783): a case
/// whose `===` line is mistyped is an error, never a case dropped unseen. A
/// `quiet` error leaves the line's text out.
fn preamble(block: &str, quiet: bool) -> Result<(), String> {
    let rule = "before the first case, a line is a comment or blank";
    let bad = block
        .lines()
        .enumerate()
        .find(|(_, l)| !l.trim().is_empty() && !l.trim().starts_with('#'));
    match bad {
        None => Ok(()),
        Some((i, _)) if quiet => Err(format!("line {}: {rule}", i + 1)),
        Some((i, l)) => Err(format!("line {}: {rule}: {l}", i + 1)),
    }
}

/// The cases a run read, and how many came from each file, for `--check`.
struct Loaded {
    cases: Vec<Case>,
    from: Vec<(PathBuf, usize)>,
}

/// Reads the cases file, then the held-out file when there is one (#783). That
/// file may hold no case, each of its cases says `holdout: yes`, and none has
/// an id the cases file has. An error starts with the file it is in.
fn load(cases: &Path, blind: Option<&Path>) -> Result<Loaded, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let mut all = parse_cases(&read(cases)?).map_err(|e| format!("{}: {e}", cases.display()))?;
    let mut from = vec![(cases.to_path_buf(), all.len())];
    if let Some(b) = blind {
        let at = |e: String| format!("{}: {e}", b.display());
        let held = parse_some(&read(b)?, true).map_err(at)?;
        if let Some(c) = held.iter().find(|c| !c.holdout) {
            return Err(at(format!("case {}: every case in this file needs holdout: yes", c.id)));
        }
        if let Some(c) = held.iter().find(|c| all.iter().any(|a| a.id == c.id)) {
            return Err(at(format!("case {}: already in {}", c.id, cases.display())));
        }
        from.push((b.to_path_buf(), held.len()));
        all.extend(held);
    }
    Ok(Loaded { cases: all, from })
}

/// Keeps the cases a run takes: those `--only` names.
fn pick(cases: &mut Vec<Case>, o: &Opts) {
    if let Some(w) = &o.only {
        cases.retain(|c| c.id.contains(w.as_str()) || c.kind == *w);
    }
}

/// The cases a run, `--check` and `--overlap` take: the files `o` names,
/// loaded, then those `--only` names.
fn run_cases(o: &Opts) -> Result<Loaded, String> {
    let mut l = load(&o.cases, o.blind.as_deref())?;
    pick(&mut l.cases, o);
    Ok(l)
}

/// The line a run prints first and `--check` prints alone: the cases taken,
/// and each file read with how many cases it held.
fn check_line(cases: &[Case], from: &[(PathBuf, usize)]) -> String {
    let count = |e: Expect| cases.iter().filter(|c| c.expect == e).count();
    let held = cases.iter().filter(|c| c.holdout).count();
    let files: Vec<String> = from.iter().map(|(p, n)| format!("{} ({n})", p.display())).collect();
    format!(
        "{} cases ({} ask, {} write, {} either; {held} holdout) from {}",
        cases.len(),
        count(Expect::Ask),
        count(Expect::Write),
        count(Expect::Either),
        files.join(", ")
    )
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
/// `/code-review`) needs no boundary there. A needle opening with `. ` is a
/// sentence start, and the start of the text is one too (#684): `. invert` is in
/// "Invert the check" but not in "We invert the sign". That rule is this
/// function's, so a `must` or an `ask_any` term gets it; [`has_unnegated`], which
/// reads a `must_not`, does not.
fn has(hay: &str, needle: &str) -> bool {
    let hay = format!(". {}", hay.to_lowercase());
    alternatives(needle).any(|n| !starts(&hay, &n).is_empty())
}

/// An `ask_any` term against a question: `@purpose` is [`asks_purpose`], any
/// other term is [`has`].
fn ask_term(question: &str, term: &str) -> bool {
    match term.trim() {
        "@purpose" => asks_purpose(question),
        t => has(question, t),
    }
}

/// Whether a question asks what the work is for (#702 H1). Its patterns, built on
/// the dev half of `evals/panel/purpose-questions.tsv` only, read the question
/// before its ` [options]`:
/// - "what" or "who" then a verb ([`WH_VERBS`]) and a "for" that ends a clause
///   ("What is it mainly for?"); "What platform is it for?" asks which target;
/// - the same opening and an "about" that ends a clause ("What should it tell
///   users about?");
/// - "purpose" anywhere, or "goal" as a purpose: "main goal", "goal of", "goal
///   for", "the goal" ("Which goal in the Makefile?" is not one);
/// - "problem" with "solve" ("What problem should it solve?");
/// - "what should ... achieve" or "what should ... catch".
///
/// Known limit: a scope question in the same form still matches ("What is the
/// backfill for: all history or the last 30 days?").
fn asks_purpose(question: &str) -> bool {
    let stem = question.split(" [").next().unwrap_or(question).to_lowercase();
    let mut words = stem.split_whitespace();
    let wh_verb = match (words.next(), words.next()) {
        (Some("what" | "who"), Some(v)) => WH_VERBS.contains(&v),
        (Some(w), _) => matches!(w, "what's" | "who's"),
        _ => false,
    };
    (wh_verb && has(&stem, "for?|about?"))
        || has(&stem, "purpose|main goal|goal of|goal for|the goal")
        || (has(&stem, "problem") && has(&stem, "solve"))
        || (stem.starts_with("what should") && has(&stem, "achieve|catch"))
}

/// The verbs that may follow "what" or "who" in a purpose question.
const WH_VERBS: [&str; 8] = ["is", "are", "was", "will", "would", "should", "does", "do"];

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
/// A term ending in `?` ("for?") is its word at the end of a clause: followed by
/// `?,;:.`, a closing `)` or `]`, or the end of the text, never by another word.
fn starts(hay: &str, n: &str) -> Vec<usize> {
    if let Some(w) = n.strip_suffix('?').filter(|w| !w.is_empty()) {
        return starts(hay, w)
            .into_iter()
            .filter(|i| hay[*i + w.len()..].chars().next().is_none_or(|c| "?,;:.)]".contains(c)))
            .collect();
    }
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
        g.on_target = c.ask_any.is_empty() || c.ask_any.iter().any(|w| ask_term(&g.question, w));
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
    /// The held-out file loaded beside `cases` (#783); `--cases` drops it.
    blind: Option<PathBuf>,
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
    /// Print a held-out case's failing reply or error, under a warning, for the
    /// owner or a listed reader rerunning one case (#792). Never into `--out`.
    show_holdout: bool,
    /// `--router-prompt`: the file and its text, sent as every call's system
    /// prompt in place of the write prompt (#789).
    router: Option<(PathBuf, String)>,
    judge_model: String,
    /// A labelled file: run the judge on it and print its agreement with the owner.
    calibrate: Option<PathBuf>,
    /// A purpose-question fixture: calibrate the `@purpose` judge on it (#702 H2).
    purpose_calibrate: Option<PathBuf>,
    split: judge::Split,
    /// Ask the judge whether each changed prompt line paraphrases a held-out
    /// case's request (#710).
    overlap: bool,
    /// `--overlap`'s lines: those `git diff BASE -- tui/prompts` adds.
    base: String,
    /// `--overlap`'s lines: those of the second file the first lacks.
    from_to: Option<(PathBuf, PathBuf)>,
}

/// `--router-prompt`'s file and its text, read once at start-up. A file that
/// cannot be read, is not UTF-8 or holds only blanks is an error naming the flag.
fn router_prompt(p: PathBuf) -> Result<(PathBuf, String), String> {
    let err = |why: String| format!("--router-prompt {}: {why}", p.display());
    let text = std::fs::read_to_string(&p).map_err(|e| err(e.to_string()))?;
    if text.trim().is_empty() {
        return Err(err("empty".into()));
    }
    Ok((p, text))
}

/// The held-out file (#783), from the crate's root.
const BLIND: &str = "evals/panel/holdout-blind.txt";

fn opts(args: impl IntoIterator<Item = String>) -> Result<Opts, String> {
    let mut o = Opts {
        cases: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/cases.txt"),
        blind: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(BLIND)),
        model: "auto".into(),
        // One run is not enough to judge a change: a case can pass 3/3, then 2/3.
        runs: 3,
        jobs: 4,
        only: None,
        out: None,
        check: false,
        purpose_calibrate: None,
        compare: None,
        allow_mixed: false,
        ledger: None,
        no_explore: false,
        hermetic: true,
        judge: false,
        show_holdout: false,
        router: None,
        judge_model: judge::DEFAULT_MODEL.into(),
        calibrate: None,
        split: judge::Split::All,
        overlap: false,
        base: "HEAD".into(),
        from_to: None,
    };
    let (mut from, mut to, mut base) = (None, None, None);
    let mut runs_given = false;
    let mut a = args.into_iter();
    while let Some(k) = a.next() {
        let mut val = || a.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--cases" => (o.cases, o.blind) = (val()?.into(), None),
            "--model" => o.model = val()?,
            "--runs" => {
                o.runs = val()?.parse().map_err(|e| format!("--runs: {e}"))?;
                runs_given = true
            }
            "--jobs" => o.jobs = val()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--only" => o.only = Some(val()?),
            "--out" => o.out = Some(val()?.into()),
            "--check" => o.check = true,
            "--no-explore" => o.no_explore = true,
            "--no-hermetic" => o.hermetic = false,
            "--allow-mixed" => o.allow_mixed = true,
            "--ledger" => o.ledger = Some(val()?.into()),
            "--judge" => o.judge = true,
            "--show-holdout" => o.show_holdout = true,
            "--router-prompt" => o.router = Some(router_prompt(val()?.into())?),
            "--judge-model" => o.judge_model = val()?,
            "--calibrate" => o.calibrate = Some(val()?.into()),
            "--purpose-calibrate" => o.purpose_calibrate = Some(val()?.into()),
            "--split" => o.split = judge::Split::parse(&val()?)?,
            "--overlap" => o.overlap = true,
            "--base" => base = Some(val()?),
            "--from" => from = Some(PathBuf::from(val()?)),
            "--to" => to = Some(PathBuf::from(val()?)),
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
    // the flag is for rerunning one case: a sweep with it prints every held-out failure
    if o.show_holdout && o.only.is_none() {
        return Err("--show-holdout needs --only: it reruns a held-out case, never a sweep".into());
    }
    // one paid call on one case, as the stop rule in docs/tui.md says
    if o.show_holdout && o.judge {
        return Err("--show-holdout makes one call: --judge adds another".into());
    }
    if o.show_holdout && !(runs_given && o.runs == 1) {
        return Err("--show-holdout makes one call: add --runs 1".into());
    }
    o.runs = o.runs.max(1);
    if !o.overlap && (from.is_some() || to.is_some() || base.is_some()) {
        return Err("--from, --to and --base are for --overlap".into());
    }
    o.from_to = match (from, to) {
        (Some(f), Some(t)) => Some((f, t)),
        (None, None) => None,
        _ => return Err("--from and --to go together".into()),
    };
    if let Some(b) = base {
        if o.from_to.is_some() {
            return Err("--base is for git diff, not --from and --to".into());
        }
        if b.starts_with('-') {
            return Err(format!("--base: {b} is not a revision"));
        }
        o.base = b;
    }
    // a mode that sends no panel prompt would run with the flag silently ignored
    let offline = [
        ("--compare", o.compare.is_some()),
        ("--calibrate", o.calibrate.is_some()),
        ("--purpose-calibrate", o.purpose_calibrate.is_some()),
        ("--overlap", o.overlap),
    ];
    if o.router.is_some()
        && let Some((mode, _)) = offline.iter().find(|(_, on)| *on)
    {
        return Err(format!("--router-prompt replaces the panel's prompt: {mode} sends none"));
    }
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
    /// The hash of the prompt files the job's call read (`prompts::files_hash`).
    prompt: String,
    req: Request,
    started: Instant,
}

/// One finished call of a case: what a run prints of it and writes to `--out`.
struct Ended<'a> {
    case: &'a Case,
    run: usize,
    model: &'a str,
    /// The system prompt's hash.
    system: u64,
    prompt: &'a str,
    ms: u128,
    /// Why the call failed; `None` when it answered.
    err: Option<String>,
    /// What kind of failure `err` is ([`fail_class`]): all a held-out case shows of it.
    class: &'static str,
    /// `None` for a failed call.
    grade: Option<Grade>,
    usage: Usage,
    reply: &'a str,
    explore: bool,
    /// The files an exploring call read; `None` for a text call.
    read: Option<Vec<String>>,
    /// What an exploring call read and searched ([`panel_reads`]).
    calls: Option<Vec<String>>,
}

/// The one place that decides what a run shows of a case (#792). A tune case
/// (`held` false) shows what `full` builds. A held-out case shows what `blind`
/// builds: its id, pass or fail and counts, never its text. That is no must,
/// must_not or ask_any phrase, kind, question, reply, reads, judge critique,
/// or error text, which may echo its input.
fn show_case<T>(held: bool, full: impl FnOnce() -> T, blind: impl FnOnce() -> T) -> T {
    if held { blind() } else { full() }
}

/// The `--out` fields a held-out case's row keeps (#792): none holds its
/// phrases. Every other field, one added later included, is null on that row:
/// the reply holds a passing case's must phrases, and the reads its searches.
const HELD_FIELDS: [&str; 25] = [
    "id",
    "expect",
    "run",
    "model",
    "system",
    "prompt",
    "ms",
    "error",
    "asked",
    "pass",
    "wrong_type",
    "cost_usd",
    "in_tokens",
    "out_tokens",
    "refused",
    "holdout",
    "explore",
    "env",
    "missing_n",
    "forbidden_n",
    "files_read_n",
    "judge",
    "judge_model",
    "judge_cost_usd",
    "rubric",
];

/// Why a `--show-holdout` run refuses to start, and the fix.
const SHOW_ONE: &str = "--show-holdout needs --only <exact id> matching one case";

/// The line a `--show-holdout` run prints first, before any case (#792).
const SHOW_BANNER: &str = "warning: --show-holdout is on: this run prints held-out case text; \
    a tuning session must not read it";

/// What a failed call was, in words of the rig's own: a timeout, a budget
/// stop, or any other failure.
fn fail_class(answer: &Result<String, Fail>) -> &'static str {
    match answer {
        Err(Fail::Timeout) => "took too long",
        Err(Fail::Budget { .. }) => "stopped at its budget",
        _ => "call failed",
    }
}

/// A held-out case's `row`: `missing_n`, `forbidden_n` and `files_read_n`
/// count its lists, and each field outside [`HELD_FIELDS`] is null.
fn blind_row(mut row: serde_json::Value) -> serde_json::Value {
    let Some(m) = row.as_object_mut() else {
        return row;
    };
    let lists = [
        ("missing", "missing_n"),
        ("forbidden", "forbidden_n"),
        ("files_read", "files_read_n"),
    ];
    for (list, n) in lists {
        if let Some(l) = m.get(list).and_then(|l| l.as_array()) {
            let len = l.len();
            m.insert(n.into(), len.into());
        }
    }
    for (k, v) in m.iter_mut() {
        if !HELD_FIELDS.contains(&k.as_str()) {
            *v = serde_json::Value::Null;
        }
    }
    row
}

/// Whether an `--out` row is a held-out case's; a row that does not say is.
fn held_row(row: &serde_json::Value) -> bool {
    row["holdout"].as_bool() != Some(false)
}

impl Ended<'_> {
    /// The run's line for this call: mark, id, kind, want, ms, then what went
    /// wrong. A held-out case's has no kind, and only counts or the failure's
    /// class for what.
    fn line(&self) -> String {
        let c = self.case;
        let mark = match (&self.err, &self.grade) {
            (Some(_), _) => "ERR ",
            (_, Some(g)) if g.pass => "ok  ",
            _ => "FAIL",
        };
        let full = || (c.kind.as_str(), self.what());
        let (kind, what) = show_case(c.holdout, full, || ("", self.held_detail()));
        let want = c.expect.as_str();
        let what = match &self.read {
            Some(r) => format!("[read {}] {what}", r.len()),
            None => what,
        };
        format!("{mark} {:<24} {:<13} want {want:<6} {:>6} ms  {what}", c.id, kind, self.ms)
    }

    /// What a run prints for this call: its [`line`](Self::line), and with
    /// `--show-holdout` (`show`) a held-out case's failing reply or error.
    fn printed(&self, show: bool) -> String {
        match self.unveiled().filter(|_| show) {
            Some(text) => format!("{}\n{text}", self.line()),
            None => self.line(),
        }
    }

    /// A held-out case's failing reply or full error under a warning line;
    /// `None` for a tune case or a pass.
    fn unveiled(&self) -> Option<String> {
        let id = &self.case.id;
        let text = match (&self.err, &self.grade) {
            _ if !self.case.holdout => return None,
            (Some(e), _) => format!("error: {e}"),
            (None, Some(g)) if !g.pass => format!("reply: {}", self.reply.trim_end()),
            _ => return None,
        };
        let warn = "a tuning session must not read it";
        Some(format!("warning: --show-holdout prints held-out case {id}; {warn}\n{text}"))
    }

    /// What a held-out case's line says went wrong: counts, or the class of a
    /// failed call.
    fn held_detail(&self) -> String {
        let g = match (&self.err, &self.grade) {
            (None, Some(g)) => g,
            _ => return format!("held out: {}", self.class),
        };
        let n = [("has", g.forbidden.len()), ("missing", g.missing.len())];
        let n = n.iter().filter(|(_, k)| *k > 0).map(|(w, k)| format!("{w} {k}"));
        let n = n.collect::<Vec<_>>().join(", ");
        match n.is_empty() {
            true => "held out".into(),
            false => format!("held out: {n}"),
        }
    }

    /// What a tune case's line says went wrong, its phrases included.
    fn what(&self) -> String {
        let (c, g) = (self.case, &self.grade);
        match (&self.err, g) {
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
        }
    }

    /// The `--out` row for this call, with the run's environment `env`. A
    /// held-out case's is [`blind_row`], its error only the failure's class.
    fn row(&self, env: serde_json::Value) -> serde_json::Value {
        let blind = || {
            let mut r = self.full_row(env.clone());
            if self.err.is_some() {
                r["error"] = self.class.into();
            }
            blind_row(r)
        };
        show_case(self.case.holdout, || self.full_row(env.clone()), blind)
    }

    fn full_row(&self, env: serde_json::Value) -> serde_json::Value {
        let (c, g, u) = (self.case, self.grade.as_ref(), &self.usage);
        serde_json::json!({
            "id": c.id, "kind": c.kind, "expect": c.expect.as_str(), "run": self.run, "model": self.model,
            "system": format!("{:016x}", self.system), "prompt": self.prompt,
            "ms": self.ms as u64, "error": self.err,
            "asked": g.map(|g| g.asked), "pass": g.map(|g| g.pass),
            "question": g.map(|g| g.question.clone()),
            "missing": g.map(|g| g.missing.clone()),
            "forbidden": g.map(|g| g.forbidden.clone()),
            "wrong_type": g.and_then(|g| g.wrong_type),
            "cost_usd": u.cost_usd,
            "in_tokens": u.in_tokens, "out_tokens": u.out_tokens,
            "refused": g.map(|g| g.refused), "holdout": c.holdout, "reply": self.reply,
            "explore": self.explore, "files_read": self.read, "reads": self.calls, "env": env,
        })
    }
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
                if let Some(e) = mixed_error(&[(a, &ra), (b, &rb)]).filter(|_| !o.allow_mixed) {
                    eprintln!("panel_eval: {e}");
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
    if let Some(file) = &o.purpose_calibrate {
        if let Err(e) = purpose_calibrate(file, &o) {
            eprintln!("panel_eval: {e}");
            std::process::exit(2);
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
    if o.overlap {
        std::process::exit(overlap_main(&o));
    }
    run(&o, std::env::current_exe());
}

/// A run of the cases `o` names: each call, its line and row, the summary.
/// `me` is the rig's own binary, which wraps each call to read its cost.
fn run(o: &Opts, me: std::io::Result<PathBuf>) {
    let Loaded { cases, from } = run_cases(o).unwrap_or_else(|e| {
        eprintln!("panel_eval: {e}");
        std::process::exit(2);
    });
    // --show-holdout reruns one case: its whole id, never a part of others' or a kind
    let one = matches!(&cases[..], [c] if o.only.as_deref() == Some(c.id.as_str()));
    if o.show_holdout && !one {
        let n = cases.len();
        eprintln!("panel_eval: {SHOW_ONE}; {n} matched");
        std::process::exit(2);
    }
    println!("{}", check_line(&cases, &from));
    if o.check {
        return;
    }
    if o.show_holdout {
        println!("{SHOW_BANNER}");
    }
    let mut call = Call::from_env();
    let version = claude_version(&call.claude);
    let mut wrapped = false;
    // what the calls get, read back: false when the rig cannot wrap them
    let mut hermetic = false;
    match me {
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
    // each prompt hash the jobs saw, in order. A plain and an exploring write read
    // different files, so each has its own; more than that means an override changed mid-run.
    let mut prompt_sets: Vec<String> = vec![];
    while !todo.is_empty() || !live.is_empty() {
        while live.len() < o.jobs
            && let Some((ci, run)) = todo.pop()
        {
            let c = &cases[ci];
            let path = pane_path(c, staged.0.as_deref());
            let explore = c.explore.is_some() && !o.no_explore;
            // per job, from the call's own read: an override edited mid-run shows in the rows
            let (system, prompt, profile, prompt_files) = panel_call(c, &path, explore);
            let (system, prompt_files) = routed(o.router.as_ref(), system, prompt_files);
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
            let class = fail_class(&answer);
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
            let end = Ended {
                case: c,
                run: j.run,
                model: &j.model,
                system: j.system,
                prompt: &j.prompt,
                ms,
                err,
                class,
                grade: g,
                usage,
                reply: &raw,
                explore: j.explore,
                read,
                calls,
            };
            println!("{}", end.printed(o.show_holdout));
            if log.is_some() || o.judge {
                let row = end.row(env.json());
                // with --judge, written once the judge has graded the reply
                match log.as_mut() {
                    Some(f) if !o.judge => {
                        let _ = writeln!(f, "{row}");
                    }
                    _ => rows.push((row, judge_input(c, &raw, end.calls.clone()))),
                }
            }
            done.push((j.case, ms, end.grade));
            costs.push(usage.cost_usd);
            reads.extend(end.read.map(|r| r.len()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    summary(&cases, &done, &hashes, o);
    let mut judged_nothing = false;
    let mut judge_usd = None;
    if o.judge {
        let (none, usd) = judge_rows(&mut rows, o, &call.claude, wrapped, log.as_mut());
        (judged_nothing, judge_usd) = (none, Some(usd));
    }
    let ledger = ledger_path(o);
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

/// What the judge reads of case `c`'s reply `raw`: the case's input, its
/// earlier rounds, and what the panel read before it: `None` when unknown, and
/// for a case with refs, whose basket the judge does not see.
fn judge_input(c: &Case, raw: &str, reads: Option<Vec<String>>) -> judge::Input {
    judge::Input {
        window: format!("{} · {} · {}", c.name, c.cmd, c.path),
        out: out_name(c.out).into(),
        screen: c.screen.clone(),
        repo: c.repo.clone(),
        request: c.request.clone(),
        reply: raw.into(),
        // the judge does not see the basket: rule 7 can weigh the reads only
        // when it sees the rest of what the panel saw
        reads: reads.filter(|_| c.refs.is_empty()),
        conversation: c.turns.clone(),
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
        errors += usize::from(j.outcome().0.is_none());
        println!("{}", verdict_line(row, j));
        judge::annotate(row, j, model);
        // a held-out row loses the critique annotate wrote, or the judge's error
        let r = row.take();
        *row = show_case(held_row(&r), || r.clone(), || blind_row(r.clone()));
    }
    errors
}

/// The line `--judge` prints for row `row` judged `j`: mark, id, critique. A
/// held-out case's has no critique, and a failed call only its class: the
/// judge's stderr may echo the case it was sent.
fn verdict_line(row: &serde_json::Value, j: &judge::Judged) -> String {
    let (pass, critique) = j.outcome();
    let mark = match pass {
        Some(true) => "ok  ",
        Some(false) => "FAIL",
        None => "ERR ",
    };
    let id = row["id"].as_str().unwrap_or("");
    let blind = || match pass {
        Some(_) => format!("judge {mark} {id}"),
        None => format!("judge {mark} {id}  held out: call failed"),
    };
    show_case(held_row(row), || format!("judge {mark} {id:<24} {critique}"), blind)
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
fn panel_call(c: &Case, path: &str, explore: bool) -> (String, String, Profile, String) {
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
    let ((system, prompt, _), hash) = a.request_hashed().expect("^S on a draft starts a write");
    let profile = a.run.as_ref().map_or(Profile::TEXT, |r| a.profile_for(&r.flow));
    (system, prompt, profile, hash.expect("a write's call carries its prompt hash"))
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
            // a held-out row keeps only the count (#792)
            files_read: v["files_read"]
                .as_array()
                .map(Vec::len)
                .or(v["files_read_n"].as_u64().map(|n| n as usize)),
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

/// The call's system prompt and prompt hash: with `--router-prompt`, its text
/// and [`files_hash`](prompts::files_hash) of it in place of the write prompt's (#789).
fn routed(router: Option<&(PathBuf, String)>, system: String, files: String) -> (String, String) {
    match router {
        Some((_, text)) => (text.clone(), prompts::files_hash(&[text])),
        None => (system, files),
    }
}

/// Where a run's system prompt came from: the `--router-prompt` file, or [`prompt_source`].
fn prompt_from(o: &Opts) -> String {
    match &o.router {
        Some((p, _)) => format!("{} (--router-prompt)", p.display()),
        None => prompt_source(&config_home()),
    }
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

/// `--compare`'s refusal: each case whose rows on one side carry two prompt
/// hashes, with its side's file and those hashes (`none` for a row without one).
fn mixed_error(sides: &[(&PathBuf, &Vec<Row>)]) -> Option<String> {
    let named = |p: &PathBuf, rows: &[Row], id: String| {
        let mut hs: Vec<&str> = vec![];
        for r in rows.iter().filter(|r| r.id == id) {
            let h = if r.prompt.is_empty() { "none" } else { r.prompt.as_str() };
            if !hs.contains(&h) {
                hs.push(h);
            }
        }
        format!("{id} in {} ({})", p.display(), hs.join(", "))
    };
    let mixed: Vec<String> = sides
        .iter()
        .flat_map(|(p, rows)| mixed_prompts(rows).into_iter().map(|id| named(p, rows, id)))
        .collect();
    let all = mixed.join(", ");
    let why = "--allow-mixed pairs them anyway";
    (!mixed.is_empty()).then(|| format!("rows with two prompt hashes for one case: {all}; {why}"))
}

/// Calibrates the `@purpose` judge on a fixture: each row's verdict from the
/// cache next to the ledger or one call, then agreement, false accepts and
/// misses, and whether the fallback may be used. The spend goes to the ledger.
fn purpose_calibrate(file: &Path, o: &Opts) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let rows = purpose::read_fixture(&text)?;
    let claude = std::env::var("COCKPIT_CLAUDE")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or("claude".into());
    let ledger = ledger_path(o);
    let mut cache = purpose::Cache::open(&ledger.with_file_name("purpose-cache.tsv"));
    let qs: Vec<String> = rows.iter().map(|(q, _)| q.clone()).collect();
    let (verdicts, usd) = purpose::judge_all(&claude, &o.judge_model, &qs, o.jobs, &mut cache);
    let c = purpose::tally(&rows, &verdicts);
    println!(
        "purpose judge {} on {} rows, rubric {}",
        o.judge_model,
        rows.len(),
        purpose::rubric_hash()
    );
    println!("agreement       {}/{} judged, {} failed calls", c.agree, c.judged, c.failed);
    println!("false accepts   {}/{} 0-rows", c.false_accepts.0, c.false_accepts.1);
    println!("misses          {}/{} 1-rows", c.misses.0, c.misses.1);
    println!("fallback        {}", if purpose::usable(&c) { "usable" } else { "not usable" });
    println!("cost            ${usd:.4}");
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    ledger_append(&ledger, &ledger_row(secs, "purpose-calibrate", rows.len(), 1, usd, &o.judge_model))
        .map_err(|e| format!("ledger {}: {e}", ledger.display()))?;
    for ((q, want), got) in rows.iter().zip(&verdicts) {
        if got.is_some_and(|g| g != *want) {
            println!("DISAGREE want {} judge {}  {q}", u8::from(*want), u8::from(got.unwrap_or(false)));
        }
    }
    Ok(())
}

/// `--overlap` (#710): the exit code of [`overlap_check`], or 2 when it could
/// not run.
fn overlap_main(o: &Opts) -> i32 {
    overlap_check(o).unwrap_or_else(|e| {
        eprintln!("panel_eval: --overlap: {e}");
        2
    })
}

/// Asks the judge whether each changed prompt line paraphrases a held-out
/// case's request. Prints a `FLAG` record per flagged pair, with the case id
/// and the line, never the case's text, then an `overlap` record of counts.
/// Returns 1 when a pair is flagged or left unchecked by a failed call.
fn overlap_check(o: &Opts) -> Result<i32, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let mut cases = run_cases(o)?.cases;
    cases.retain(|c| c.holdout);
    let lines = match &o.from_to {
        Some((from, to)) => overlap::new_lines(&read(from)?, &read(to)?),
        None => overlap::added_lines(&prompt_diff(&o.base)?),
    };
    let claude = std::env::var("COCKPIT_CLAUDE")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or("claude".into());
    let ledger = ledger_path(o);
    if cases.is_empty() && !lines.is_empty() {
        return Err(format!("{} changed lines and no held-out case to check them against", lines.len()));
    }
    let mut cache = overlap::Cache::open(&ledger.with_file_name("overlap-cache.tsv"), overlap::QUESTION);
    let pairs: Vec<(&str, &str)> = lines
        .iter()
        .flat_map(|l| cases.iter().map(move |c| (l.as_str(), c.request.as_str())))
        .collect();
    let r = overlap::judge_all(&claude, &o.judge_model, &pairs, o.jobs, &mut cache);
    let mut flagged = 0;
    for (i, v) in r.verdicts.iter().enumerate() {
        if *v == Some(true) {
            flagged += 1;
            println!("FLAG\t{}\t{}", cases[i % cases.len()].id, lines[i / cases.len()]);
        }
    }
    let failed = r.failed.len();
    println!(
        "overlap\tflagged={flagged}\tpairs={}\tlines={}\tcases={}\tcached={}\tfailed={failed}\tusd={:.4}\tmodel={}\tquestion={}",
        pairs.len(),
        lines.len(),
        cases.len(),
        r.cached,
        r.usd,
        o.judge_model,
        overlap::question_hash()
    );
    if r.cached < pairs.len() {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let row = ledger_row(secs, "overlap", cases.len(), 1, r.usd, &o.judge_model);
        if let Err(e) = ledger_append(&ledger, &row) {
            eprintln!("panel_eval: ledger {}: {e}", ledger.display());
        }
    }
    if failed > 0 {
        let mut why = r.failed.clone();
        why.sort();
        why.dedup();
        eprintln!("panel_eval: --overlap: {failed} pairs unchecked: {}", why.join("; "));
    }
    Ok(i32::from(flagged + failed > 0))
}

/// What `git diff BASE` changes in `tui/prompts`, the working tree against
/// `base`, with no context lines.
fn prompt_diff(base: &str) -> Result<String, String> {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .args(["diff", "--no-color", "--no-ext-diff", "--no-textconv", "-U0", base, "--", "prompts"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !o.status.success() {
        return Err(format!("git diff {base}: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
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

fn summary(cases: &[Case], done: &[(usize, u128, Option<Grade>)], hashes: &[u64], o: &Opts) {
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
    println!("system prompt   {} ({})", hs.join(" "), prompt_from(o));
    println!("passed          {}", pct(t.passed));
    // The share of cases that passed all their runs: a flaky case counts as a failure.
    println!("{:<16}{}", format!("pass^{}", o.runs), pct_ci(pass_all(&t.per_case)));
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
    fn a_question_term_matches_its_word_at_the_end_of_a_clause() {
        assert!(has("What is the anomaly detection for, mainly?", "for?"));
        assert!(has("What are the reminders for: who gets them", "for?"));
        assert!(has("Who is it for?", "for?"));
        assert!(has("(and what is it for)", "for?"));
        assert!(has("what is it for", "for?"), "the end of the text ends the clause");
        assert!(!has("Is this for the dashboard?", "for?"));
        assert!(!has("Is this format right?", "for?"));
    }

    #[test]
    fn purpose_terms_match_no_question_two_labellers_marked_not_about_purpose() {
        // Panel questions, each marked 1 (asks what the work is for) or 0 by two
        // blind labellers (#702 M2). The ask_any purpose terms of the cases.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals/panel/purpose-questions.tsv");
        let text = std::fs::read_to_string(path).unwrap();
        let terms = ["for?", "purpose", "goal", "about?"];
        let (mut hit, mut ones, mut false_hits) = (0, 0, vec![]);
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let (q, label) = line.split_once('\t').unwrap();
            let m = terms.iter().any(|t| has(q, t));
            match label {
                "1" => (ones, hit) = (ones + 1, hit + usize::from(m)),
                "0" if m => false_hits.push(q),
                "0" => {}
                l => panic!("label {l}"),
            }
        }
        assert_eq!(false_hits, Vec::<&str>::new());
        // recall: the misses ask the purpose in other words ("what problem
        // should it solve"), which the terms do not name; no label is changed
        assert_eq!((hit, ones), (213, 227));
    }

    #[test]
    fn an_at_purpose_ask_any_takes_a_question_about_what_the_work_is_for() {
        let mut c = parse_cases(ONE).unwrap().remove(0);
        c.expect = Expect::Ask;
        c.ask_any = vec!["@purpose".into()];
        let on = |q: &str| grade(&c, &format!("<question>{q}</question><option>a</option><option>b</option>")).on_target;
        assert!(on("What is the main problem these features should solve?"));
        assert!(on("What is the audit log mainly for?"));
        assert!(!on("What kind of notifications do you want?"));
        assert!(!on("Is this for the dashboard?"));
    }

    #[test]
    fn at_purpose_on_each_half_of_the_fixture_accepts_no_non_purpose_question() {
        // halves by the question's hash: the patterns were built on dev (even) only
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals/panel/purpose-questions.tsv");
        let text = std::fs::read_to_string(path).unwrap();
        let mut seen = [(0, 0, 0); 2]; // (hits on 1-rows, 1-rows, hits on 0-rows)
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let (q, label) = line.split_once('\t').unwrap();
            let h = &mut seen[(fnv(q) % 2) as usize];
            let m = asks_purpose(q);
            match label {
                "1" => (h.0, h.1) = (h.0 + usize::from(m), h.1 + 1),
                _ => h.2 += usize::from(m),
            }
        }
        assert_eq!(seen, [(100, 100, 0), (126, 127, 0)], "[dev, test]");
    }

    #[test]
    fn at_purpose_rejects_a_which_target_question() {
        for q in [
            "What platform is it for?",
            "What time zone is the reminder for?",
            "Which goal in the Makefile?",
        ] {
            assert!(!asks_purpose(q), "{q}");
        }
        assert!(!asks_purpose("Is this for the dashboard?"));
        assert!(!asks_purpose("What should the button say about errors?"));
    }

    #[test]
    fn at_purpose_known_limit_a_scope_question_with_a_verb_still_matches() {
        // "is" follows "What", so the for? rule takes it, though it asks the scope
        assert!(asks_purpose("What is the backfill for: all history or the last 30 days?"));
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

    // --- #789: --router-prompt, its options ---

    /// `opts` with `--router-prompt` on `p`.
    fn router_opts(p: &Path) -> Result<Opts, String> {
        opts(["--router-prompt".to_string(), p.display().to_string()])
    }

    #[test]
    fn router_prompt_reads_its_file_whole_at_start_up() {
        let d = scratch("router-opts");
        let p = d.0.join("router.txt");
        std::fs::write(&p, "route it  \n").unwrap();
        assert_eq!(router_opts(&p).unwrap().router, Some((p, "route it  \n".into())));
        assert_eq!(opts(args("")).unwrap().router, None);
    }

    /// Each way the file fails, by row name: the error names the flag and the
    /// file, and an empty or blank file says `empty`.
    #[test]
    fn router_prompt_refuses_a_bad_file_naming_the_flag() {
        let d = scratch("router-bad");
        let p = |f: &str| d.0.join(f);
        let made = [("empty.txt", &b""[..]), ("blank.txt", b" \n\t\n"), ("bin.txt", b"\xff\xfe")];
        made.iter().for_each(|(f, b)| std::fs::write(p(f), b).unwrap());
        std::fs::create_dir(p("dir")).unwrap();
        let rows = [("missing", "gone.txt"), ("empty", "empty.txt"), ("blank", "blank.txt")];
        let rows = [&rows[..], &[("not utf-8", "bin.txt"), ("a dir", "dir")]].concat();
        for (name, f) in rows {
            let e = router_opts(&p(f)).expect_err(name);
            let want = format!("--router-prompt {}: ", p(f).display());
            assert!(e.starts_with(&want) && e.len() > want.len(), "{name}: {e}");
            assert_eq!(e.ends_with(": empty"), ["empty", "blank"].contains(&name), "{name}: {e}");
        }
        assert_eq!(opts(args("--router-prompt")).unwrap_err(), "--router-prompt needs a value");
        // --check is the pre-flight: it refuses a bad file as a run does
        let check = [
            "--check".to_string(),
            "--router-prompt".into(),
            p("empty.txt").display().to_string(),
        ];
        assert!(opts(check).unwrap_err().ends_with(": empty"));
    }

    /// Each mode that sends no panel prompt, by row name, refuses the flag
    /// rather than run with it ignored.
    #[test]
    fn router_prompt_is_refused_in_each_mode_that_sends_no_panel_prompt() {
        let d = scratch("router-modes");
        let p = d.0.join("router.txt");
        std::fs::write(&p, "route it").unwrap();
        let rows = [("--compare", "--compare a b"), ("--calibrate", "--calibrate l.jsonl")];
        let rows = [
            &rows[..],
            &[("--purpose-calibrate", "--purpose-calibrate f"), ("--overlap", "--overlap")],
        ]
        .concat();
        for (mode, flags) in rows {
            let mut a = args(flags);
            a.extend(["--router-prompt".to_string(), p.display().to_string()]);
            let want = format!("--router-prompt replaces the panel's prompt: {mode} sends none");
            assert_eq!(opts(a).expect_err(mode), want, "{mode}");
        }
    }

    #[test]
    fn the_summary_names_the_router_file_as_the_prompts_source() {
        let d = scratch("router-src");
        let p = d.0.join("router.txt");
        std::fs::write(&p, "route it").unwrap();
        let want = format!("{} (--router-prompt)", p.display());
        assert_eq!(prompt_from(&router_opts(&p).unwrap()), want);
        assert_eq!(prompt_from(&opts(args("")).unwrap()), prompt_source(&config_home()));
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

    /// A row from an older file, without a hash, shows as `none` beside the hash.
    #[test]
    fn compare_names_a_row_without_a_prompt_hash_as_none() {
        let z = concat!(
            r#"{"id":"z","expect":"write","run":0,"pass":true}"#,
            "\n",
            r#"{"id":"z","expect":"write","run":1,"pass":true,"prompt":"aa"}"#,
        );
        let e = mixed_error(&[(&PathBuf::from("a"), &read_rows("a", z).unwrap())]).unwrap();
        let want = "rows with two prompt hashes for one case: z in a (none, aa); ";
        assert_eq!(e, format!("{want}--allow-mixed pairs them anyway"));
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
        let (system, prompt, profile, _) = panel_call(c, "/r", false);
        assert_eq!((system, profile), (prompts::write(&ctx), Profile::TEXT));
        assert_eq!(prompt, build(&flow, &cap, &[], 3));
        let (system, _, profile, _) = panel_call(c, "/r", true);
        assert_eq!(system, prompts::write_explore(&ctx));
        assert_eq!(profile, Profile::read_only("/r".into(), false, EXPLORE_BUDGET_USD, EXPLORE_TIMEOUT));
    }

    /// The row's prompt hash is the one the panel's call read (#711), not one
    /// from a second read of the override folder.
    #[test]
    fn the_rows_prompt_hash_is_the_calls_own() {
        use cockpit_tui::panel::capture::window_kind;
        use cockpit_tui::panel::prompts::{self, Ctx};
        let c = &parse_cases(ONE).unwrap()[0];
        let ctx = Ctx {
            out: c.out,
            window: window_kind(&c.name, &c.cmd),
            ..Ctx::default()
        };
        assert_eq!(panel_call(c, "/r", false).3, prompts::write_noted(&ctx).2);
        assert_eq!(panel_call(c, "/r", true).3, prompts::write_explore_noted(&ctx).2);
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
        let (_, prompt, ..) = panel_call(c, "/r", false);
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
        let (_, prompt, ..) = panel_call(&cases[0], &path, false);
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
        let (_, prompt, ..) = panel_call(&cases[0], &path, true);
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
        let dir = scratch("judge-reads");
        let root = &dir.0;
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
        let got = judge::judge_all(&fake.display().to_string(), "haiku", &[i], 1, root);
        let v = got[0].verdict.clone().unwrap();
        assert_eq!((v.pass, v.critique.as_str()), (false, "#871 invented"));
    }

    /// The judge does not see a case's basket, so it gets no reads there and
    /// rule 7 stays lenient (review645). It sees the earlier rounds (#628), so
    /// an answered case keeps its reads.
    #[test]
    fn judge_input_withholds_reads_only_for_a_case_with_refs() {
        let want = panel_reads(Some(EXPLORED), true);
        for (name, text, got) in [("refs", REFS, None), ("rounds", ANSWERED, want.clone())] {
            let c = &parse_cases(text).unwrap()[0];
            assert_eq!(judge_input(c, "r", want.clone()).reads, got, "{name}");
        }
    }

    #[test]
    fn judge_input_carries_the_earlier_rounds_into_the_prompt() {
        let c = &parse_cases(ANSWERED).unwrap()[0];
        let i = judge_input(c, "r", None);
        assert_eq!(i.conversation, c.turns);
        let want = "<conversation>\n<turn role=\"assistant\">Which API? Options: gateway | billing</turn>\n\
                    <turn role=\"user\">billing</turn>\n</conversation>\n";
        assert!(judge::prompt(&i).contains(want), "{}", judge::prompt(&i));
    }

    #[test]
    fn judge_input_for_a_first_round_has_no_conversation() {
        let c = &parse_cases(ONE).unwrap()[0];
        let i = judge_input(c, "r", None);
        assert!(i.conversation.is_empty());
        assert!(!judge::prompt(&i).contains("<conversation>"), "{}", judge::prompt(&i));
    }

    /// The round trip for the earlier rounds: a stand-in judge, given a rubric
    /// that points at the conversation, passes a prompt for the API the user
    /// picked in it and fails one for the other.
    #[test]
    fn a_stand_in_judge_reads_the_users_earlier_answer() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("judge-rounds");
        let root = &dir.0;
        let fake = root.join("fake-claude");
        let script = "#!/bin/sh\nwhile [ \"$1\" != --system-prompt-file ]; do shift; done\n\
            grep -q 'in the <conversation> section' \"$2\" || exit 3\n\
            p=$(cat); a=$(printf '%s' \"$p\" | sed -n 's/^<turn role=\"user\">\\(.*\\)<\\/turn>$/\\1/p')\n\
            [ -n \"$a\" ] || exit 4\n\
            case \"$p\" in *\"<reply>\"*\"$a\"*) echo 'CRITIQUE: uses the answer'; echo 'VERDICT: PASS';;\n\
            *) echo 'CRITIQUE: ignores the answer'; echo 'VERDICT: FAIL';; esac\n";
        std::fs::write(&fake, script).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let c = &parse_cases(ANSWERED).unwrap()[0];
        let inputs = ["<prompt>cache the billing API</prompt>", "<prompt>cache the gateway</prompt>"].map(|r| judge_input(c, r, None));
        let got = judge::judge_all(&fake.display().to_string(), "haiku", &inputs, 1, root);
        let got: Vec<(bool, String)> = got.into_iter().map(|j| j.verdict.map(|v| (v.pass, v.critique)).unwrap()).collect();
        assert_eq!(got, [(true, "uses the answer".into()), (false, "ignores the answer".into())]);
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

    /// Each field where a label's input is not its case's as `judge_input`
    /// builds it, as (key, field): window, out, screen, repo, request and
    /// conversation. An
    /// empty field is compared too, so a label that gained a screen or repo its
    /// case lacks is named. A label whose case is in neither list is not
    /// compared.
    fn label_drift(cases: &[Case], labels: &[judge::Label]) -> Vec<(String, &'static str)> {
        let mut out = vec![];
        for l in labels {
            let Some(c) = cases.iter().find(|c| c.id == l.case) else {
                continue;
            };
            let (want, got) = (judge_input(c, "", None), &l.input);
            for (f, w, g) in [
                ("window", &want.window, &got.window),
                ("out", &want.out, &got.out),
                ("screen", &want.screen, &got.screen),
                ("repo", &want.repo, &got.repo),
                ("request", &want.request, &got.request),
            ] {
                if w != g {
                    out.push((l.key.clone(), f));
                }
            }
            if want.conversation != got.conversation {
                out.push((l.key.clone(), "conversation"));
            }
        }
        out
    }

    /// One label row on `case` carrying ONE's crash case's input as the rig
    /// builds it (window "bash · bash · /home/u", out "auto", screen
    /// "$ net-proxy\nboom", request "fix this", no repo), each `set` field
    /// replaced.
    fn crash_label(case: &str, set: &[(&str, &str)]) -> judge::Label {
        let i = judge_input(&parse_cases(ONE).unwrap()[0], "r", None);
        let mut j = serde_json::json!({"key": "k", "case": case, "verdict": "pass", "reply": "r",
            "window": i.window, "out": i.out, "screen": i.screen, "repo": i.repo,
            "request": i.request});
        for (k, v) in set {
            j[*k] = (*v).into();
        }
        judge::read_labels("t", &j.to_string()).unwrap().remove(0)
    }

    #[test]
    fn label_drift_names_each_field_that_differs_from_the_case() {
        let cases = parse_cases(ONE).unwrap();
        let rows: [(&str, judge::Label, &[&str]); 8] = [
            ("same", crash_label("crash", &[]), &[]),
            ("lost a # line", crash_label("crash", &[("screen", "$ net-proxy")]), &["screen"]),
            ("request", crash_label("crash", &[("request", "fix it")]), &["request"]),
            ("repo the case lacks", crash_label("crash", &[("repo", "x")]), &["repo"]),
            ("window", crash_label("crash", &[("window", "zsh · zsh · /home/u")]), &["window"]),
            ("out", crash_label("crash", &[("out", "text")]), &["out"]),
            (
                "all empty",
                crash_label("crash", &[("window", ""), ("out", ""), ("screen", ""), ("request", "")]),
                &["window", "out", "screen", "request"],
            ),
            ("no such case", crash_label("gone", &[("screen", "")]), &[]),
        ];
        for (name, l, want) in rows {
            let got: Vec<&str> = label_drift(&cases, &[l]).into_iter().map(|(_, f)| f).collect();
            assert_eq!(got, want, "{name}");
        }
        assert_eq!(label_drift(&cases, &[]), vec![]);
    }

    /// A label row on case `c` with `c`'s input as the rig builds it and the
    /// turns in `conversation` (a json list, or null for none).
    fn label_on(c: &Case, conversation: serde_json::Value) -> judge::Label {
        let i = judge_input(c, "r", None);
        let j = serde_json::json!({"key": "k", "case": c.id, "verdict": "pass", "reply": "r",
            "window": i.window, "out": i.out, "screen": i.screen, "repo": i.repo,
            "request": i.request, "conversation": conversation});
        judge::read_labels("t", &j.to_string()).unwrap().remove(0)
    }

    #[test]
    fn label_drift_names_a_conversation_that_differs_from_the_case() {
        let mut cases = parse_cases(ONE).unwrap();
        cases.extend(parse_cases(ANSWERED).unwrap());
        let (crash, cache) = (&cases[0], &cases[1]);
        let turns = serde_json::json!([{"role": "assistant", "text": "Which API? Options: gateway | billing"},
            {"role": "user", "text": "billing"}]);
        let other = serde_json::json!([{"role": "assistant", "text": "Which API? Options: gateway | billing"},
            {"role": "user", "text": "gateway"}]);
        let rows: [(&str, judge::Label, &[&str]); 4] = [
            ("same", label_on(cache, turns.clone()), &[]),
            ("lost its rounds", label_on(cache, serde_json::Value::Null), &["conversation"]),
            ("another answer", label_on(cache, other), &["conversation"]),
            ("rounds the case lacks", label_on(crash, turns), &["conversation"]),
        ];
        for (name, l, want) in rows {
            let got: Vec<&str> = label_drift(&cases, &[l]).into_iter().map(|(_, f)| f).collect();
            assert_eq!(got, want, "{name}");
        }
    }

    /// The judge's calibration grades each owner label against the window,
    /// output type, screen, repo and request the row stores, so each must be
    /// its case's, read by the rig's own parser: a builder that took `#91 ...`
    /// for a comment once dropped review-which-pr's PR list.
    #[test]
    fn each_owner_label_carries_its_cases_input() {
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
        let dir = scratch("judge-trip");
        let root = &dir.0;
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
        let got = judge::judge_all(&fake.display().to_string(), "haiku", &[i], 1, root);
        let v = got[0].verdict.clone().unwrap();
        assert_eq!((v.pass, v.critique.as_str()), (false, "saw it"));
    }

    /// A fresh temp dir, removed when the guard drops, a panicking test included.
    fn scratch(name: &str) -> judge::Scratch {
        let d = std::env::temp_dir().join(format!("panel-eval-{name}.{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        judge::Scratch(d)
    }

    /// A test that panics holding a scratch dir still removes it (#757).
    #[test]
    fn a_scratch_dir_is_removed_when_its_test_panics() {
        let (mut dir, mut made) = (PathBuf::new(), false);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let s = scratch("panics");
            dir = s.0.clone();
            made = dir.is_dir(); // an assert here would be caught as the panic
            panic!("a failed assert");
        }));
        assert!(r.is_err(), "the closure panicked");
        assert!(made, "scratch made {}", dir.display());
        assert!(dir.ends_with(format!("panel-eval-panics.{}", std::process::id())));
        assert!(!dir.exists(), "{} left behind", dir.display());
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
        let dir = scratch("cwds");
        let root = &dir.0;
        let base = base_call(Path::new("/panel/cache"));
        let a = call_in(&base, Some(root), 0, 0).unwrap();
        let b = call_in(&base, Some(root), 0, 1).unwrap();
        assert_ne!(a.dir, b.dir);
        assert!(a.dir.starts_with(root) && b.dir.starts_with(root));
        assert_eq!(std::fs::read_dir(&a.dir).unwrap().count(), 0, "a fresh folder is empty");
        let again = call_in(&base, Some(root), 0, 0).unwrap_err();
        assert!(again.contains("case0-run0"), "never reused: {again}");
        assert_eq!(call_in(&base, None, 0, 0).unwrap().dir, base.dir);
    }

    /// The round trip: the panel's own Request, run on a hermetic call, starts
    /// claude in that call's folder (a stand-in claude that prints its cwd).
    #[test]
    fn the_panels_request_runs_claude_in_the_hermetic_folder() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("cwd-trip");
        let root = &dir.0;
        let fake = root.join("fake-claude");
        std::fs::write(&fake, "#!/bin/sh\ncat >/dev/null\npwd -P\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let base = Call {
            claude: fake.display().to_string(),
            ..base_call(root)
        };
        let call = call_in(&base, Some(root), 3, 1).unwrap();
        let mut req = Request::spawn_with(&call, &Profile::TEXT, "sys", "hi", "sonnet");
        let got = loop {
            match req.poll() {
                Some(a) => break a,
                None => std::thread::yield_now(),
            }
        };
        let want = std::fs::canonicalize(root.join("case3-run1")).unwrap();
        assert_eq!(got.unwrap().trim(), want.display().to_string());
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

    /// A made-up held-out case: ONE renamed, with `holdout: yes`.
    fn held(id: &str) -> String {
        let c = ONE.replace("# a comment\n", "").replace("=== crash", &format!("=== {id}"));
        c.replace("expect: write", "expect: write\nholdout: yes")
    }

    /// A scratch dir holding a cases file and a held-out file, with their paths.
    fn two_files(name: &str, cases: &str, blind: &str) -> (judge::Scratch, PathBuf, PathBuf) {
        let d = scratch(name);
        let (c, b) = (d.0.join("cases.txt"), d.0.join("holdout-blind.txt"));
        std::fs::write(&c, cases).unwrap();
        std::fs::write(&b, blind).unwrap();
        (d, c, b)
    }

    /// #783: the held-out file loads beside cases.txt; `--cases` loads only its path.
    #[test]
    fn the_held_out_file_loads_unless_cases_names_a_file() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel");
        let o = opts(args("")).unwrap();
        assert_eq!(o.cases, root.join("cases.txt"));
        assert_eq!(o.blind, Some(root.join("holdout-blind.txt")));
        let (_d, c, _) = two_files("explicit", ONE, &held("blind-a"));
        let o = opts(vec!["--cases".into(), c.display().to_string()]).unwrap();
        assert_eq!(o.blind, None);
        assert_eq!(run_cases(&o).unwrap().from, [(c, 1)]);
    }

    /// #783: a plain run, `--check` included, reads both shipped files.
    #[test]
    fn a_plain_run_reads_the_held_out_file() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel");
        let from = run_cases(&opts(args("--check")).unwrap()).unwrap().from;
        let paths: Vec<&PathBuf> = from.iter().map(|(p, _)| p).collect();
        assert_eq!(paths, [&root.join("cases.txt"), &root.join("holdout-blind.txt")]);
    }

    /// #783: `--overlap` keeps the held-out file; only `--cases` drops it.
    #[test]
    fn overlap_keeps_the_held_out_file() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel");
        let o = opts(args("--overlap --from a --to b")).unwrap();
        assert_eq!(o.blind, Some(root.join("holdout-blind.txt")));
    }

    /// #783: the `--check` line names each file read, with its count.
    #[test]
    fn the_check_line_names_each_file_with_its_count() {
        let (_d, c, b) = two_files("line", ONE, &format!("{}{}", held("b1"), held("b2")));
        let Loaded { cases, from } = load(&c, Some(&b)).unwrap();
        let want = format!("from {} (1), {} (2)", c.display(), b.display());
        assert!(check_line(&cases, &from).ends_with(&want), "{}", check_line(&cases, &from));
    }

    /// #783: `--check` counts each file: the cases read from each, in order.
    #[test]
    fn check_counts_both_files() {
        let blind = format!("# readers\n{}{}", held("b1"), held("b2"));
        let (_d, c, b) = two_files("counts", ONE, &blind);
        let got = load(&c, Some(&b)).unwrap();
        assert_eq!(got.from, [(c, 1), (b, 2)]);
        assert_eq!(got.cases.len(), 3);
    }

    /// #783: a plain run counts a case from the held-out file as held out.
    #[test]
    fn a_held_out_file_case_counts_as_held_out_in_a_plain_run() {
        let (_d, c, b) = two_files("grouped", ONE, &held("b1"));
        let cases = load(&c, Some(&b)).unwrap().cases;
        let pass = || {
            Some(Grade {
                pass: true,
                ..Grade::default()
            })
        };
        let at = |id: &str| cases.iter().position(|c| c.id == id).unwrap();
        let t = tally(&cases, &[(at("crash"), 1, pass()), (at("b1"), 1, pass())]);
        assert_eq!((t.passed, t.holdout), (Of(2, 2), Of(1, 1)));
    }

    /// #783: every case in the held-out file says `holdout: yes`.
    #[test]
    fn a_held_out_file_case_without_holdout_yes_fails() {
        let rows = [
            ("no key", held("b1").replace("holdout: yes\n", "")),
            ("no", held("b1").replace(": yes", ": no")),
        ];
        for (row, blind) in rows {
            let (_d, c, b) = two_files(&format!("nokey-{}", row.replace(' ', "-")), ONE, &blind);
            let e = load(&c, Some(&b)).err().unwrap_or_else(|| panic!("{row}: loaded"));
            assert!(
                e.contains("holdout-blind.txt: case b1: ") && e.contains("holdout: yes"),
                "{row}: {e}"
            );
        }
    }

    /// Each row: a parse error in the second of two made-up held-out cases, as
    /// the line it is on, the rule it breaks, and the text replaced to break it.
    fn quiet_rows() -> [(usize, &'static str, &'static str, &'static str); 8] {
        let req = "--- request\n";
        [
            (16, "not key: value", "kind: bug\n", "kind: bug\nMADE-UP stray line\n"),
            (18, "holdout is yes or no", "holdout: yes", "holdout: maybe"),
            (14, "no expect", "expect: write\n", ""),
            (16, "unknown key", "kind: bug\n", "kind: bug\nMADE-UP-KEY: x\n"),
            (16, "unknown out", "kind: bug\n", "kind: bug\nout: MADE-UP\n"),
            (23, "unknown section", req, "--- MADE-UP\n--- request\n"),
            (
                23,
                "conversation is user: and assistant: lines",
                req,
                "--- conversation\nMADE-UP\n--- request\n",
            ),
            (
                23,
                "refs are path, path:a or path:a-b lines",
                req,
                "--- refs\nMADE-UP:x-y\n--- request\n",
            ),
        ]
    }

    /// Two made-up held-out cases under a header, the second, b1, with `from`
    /// replaced by `to`.
    fn quiet_text(from: &str, to: &str) -> String {
        format!("# readers\n#\n{}{}", held("b0"), held("b1").replace(from, to))
    }

    /// #783: a held-out file's parse error gives the line and the rule broken,
    /// never the line's text; a rule of the whole case gives its `===` line.
    /// The bad line sits in the file's second case.
    #[test]
    fn a_held_out_file_error_never_shows_the_files_text() {
        for (i, (line, rule, from, to)) in quiet_rows().into_iter().enumerate() {
            let (_d, c, b) = two_files(&format!("quiet-{i}"), ONE, &quiet_text(from, to));
            let e = load(&c, Some(&b)).err().unwrap_or_else(|| panic!("{rule}: loaded"));
            assert_eq!(e, format!("{}: case b1: line {line}: {rule}", b.display()));
        }
    }

    /// #797: a parse error in a `holdout: yes` case in cases.txt gives the line
    /// and the rule broken, never the line's text, as the held-out file does.
    #[test]
    fn a_held_out_case_error_in_the_cases_file_never_shows_its_text() {
        for (i, (line, rule, from, to)) in quiet_rows().into_iter().enumerate() {
            let (_d, c, b) = two_files(&format!("quiet-cases-{i}"), &quiet_text(from, to), "");
            let e = load(&c, Some(&b)).err().unwrap_or_else(|| panic!("{rule}: loaded"));
            assert_eq!(e, format!("{}: case b1: line {line}: {rule}", c.display()));
        }
    }

    /// #797: a case's quietness is known before its keys are read, so a bad
    /// line above its `holdout: yes` line is unshown too.
    #[test]
    fn a_bad_line_above_holdout_yes_in_the_cases_file_is_unshown() {
        let bad = held("b1").replace("=== b1\n", "=== b1\nMADE-UP stray line\n");
        let (_d, c, b) = two_files("quiet-above", &bad, "");
        let e = load(&c, Some(&b)).err().expect("a bad line loaded");
        assert_eq!(e, format!("{}: case b1: line 2: not key: value", c.display()));
    }

    /// #797: a tune case in cases.txt, with no `holdout` key or `holdout: no`,
    /// still quotes its bad line.
    #[test]
    fn a_tune_case_error_in_the_cases_file_quotes_the_line() {
        let stray = "kind: bug\nMADE-UP stray line\n";
        for (row, mark) in [("no key", ""), ("no", "holdout: no\n")] {
            let tune = held("b1").replace("holdout: yes\n", mark).replace("kind: bug\n", stray);
            let (_d, c, b) = two_files(&format!("loud-{}", row.len()), &tune, "");
            let e = load(&c, Some(&b)).err().unwrap_or_else(|| panic!("{row}: loaded"));
            let want = format!("{}: case b1: not key: value: MADE-UP stray line", c.display());
            assert_eq!(e, want, "{row}");
        }
    }

    /// #797: each quiet row that hides text in a held-out case shows it in
    /// the same case without `holdout: yes`.
    #[test]
    fn a_tune_case_keeps_the_text_the_quiet_rows_hide() {
        let rows = quiet_rows().into_iter().filter(|r| r.3.contains("MADE-UP"));
        for (i, (_, rule, from, to)) in rows.enumerate() {
            let tune = quiet_text(from, to).replace("holdout: yes\n", "");
            let (_d, c, b) = two_files(&format!("loud-rows-{i}"), &tune, "");
            let e = load(&c, Some(&b)).err().unwrap_or_else(|| panic!("{rule}: loaded"));
            assert!(e.contains("MADE-UP"), "{rule}: {e}");
        }
    }

    /// #783: in the held-out file an id is a name, so a body line opening with
    /// `=== ` (Go's `=== RUN`) cannot carry the case's text into an error.
    #[test]
    fn a_held_out_file_id_that_is_not_a_name_is_refused_unshown() {
        let tail = "--- screen\n$ net-proxy\nboom\n--- request\nfix this\n";
        let moved = "--- request\nfix this\n--- screen\nboom\n=== RUN   MADE-UP-SECRET-TestLeak\n";
        let (_d, c, b) = two_files("bad-id", ONE, &held("b1").replace(tail, moved));
        let e = load(&c, Some(&b)).err().expect("a screen line made a case");
        assert_eq!(e, format!("{}: line 11: an id is lowercase letters, digits and -", b.display()));
    }

    /// #783: a line before the first case is a comment or blank, so a case
    /// whose `===` line is mistyped is an error, never a case quietly lost.
    /// The held-out file's error leaves the line out; cases.txt's shows it.
    #[test]
    fn a_line_before_the_first_case_is_an_error() {
        let rule = "line 2: before the first case, a line is a comment or blank";
        // a mistyped first `=== id` line is caught by its own, nearer rule
        let eq = "line 2: a line that starts with = is a case's === line";
        let typo = format!("# h\n{}", held("b1").replace("=== b1", "===b1"));
        let stray = format!("# h\nMADE-UP-PREAMBLE\n{}", held("b1"));
        for (row, blind, want) in [("no space", typo, eq), ("stray", stray.clone(), rule)] {
            let (_d, c, b) = two_files(&format!("pre-{}", row.len()), ONE, &blind);
            let e = load(&c, Some(&b)).err().unwrap_or_else(|| panic!("{row}: loaded"));
            assert_eq!(e, format!("{}: {want}", b.display()), "{row}");
        }
        let (_d, c, b) = two_files("pre-cases", &stray, "");
        let e = load(&c, Some(&b)).err().expect("cases.txt: loaded");
        assert_eq!(e, format!("{}: {rule}: MADE-UP-PREAMBLE", c.display()));
    }

    /// #783: in the held-out file a line opening with `=` is a case's `=== id`
    /// line, so a mistyped one is an error giving the line only, never text
    /// merged into the case before it.
    #[test]
    fn a_mistyped_case_line_is_an_error() {
        let rule = "a line that starts with = is a case's === line";
        let screen = "--- screen\n$ net-proxy\nboom\n";
        // silent before: b2 has no request, so b3's text and request join b2
        let b2 = held("b2").replace("--- request\nfix this\n", "");
        let merged = format!("{b2}{}", held("b3").replace("=== b3", "===b3").replace(screen, ""));
        // already an error before, but "request given twice" on a later line
        let twice = format!("{}{}", held("b1"), held("b2").replace("=== b2", "===b2"));
        let rows = [(10, merged), (10, held("b1").replace("boom\n", "boom\n== x\n")), (12, twice)];
        for (i, (line, blind)) in rows.into_iter().enumerate() {
            let (_d, c, b) = two_files(&format!("eq-{i}"), ONE, &blind);
            let e = load(&c, Some(&b)).err().unwrap_or_else(|| panic!("row {i}: loaded"));
            assert_eq!(e, format!("{}: line {line}: {rule}", b.display()), "row {i}");
        }
    }

    /// #783: in cases.txt a line opening with `=` is screen text: a pytest
    /// banner is a real screen line, and a dropped tune case shows in the counts.
    #[test]
    fn a_banner_line_stays_screen_text_in_the_cases_file() {
        let banner = "===== test session starts =====";
        let cases = ONE.replace("boom\n", &format!("boom\n{banner}\n"));
        let (_d, c, b) = two_files("eq-cases", &cases, "");
        let cases = load(&c, Some(&b)).unwrap_or_else(|e| panic!("{e}")).cases;
        assert_eq!(cases[0].screen, format!("$ net-proxy\nboom\n{banner}"));
    }

    /// #783: a parse error starts with the file it is in, cases.txt or the held-out file.
    /// `holdout: maybe` counts as held out (#797), so cases.txt's error gives the line too.
    #[test]
    fn a_parse_error_names_its_file() {
        let bad = held("zz").replace("holdout: yes", "holdout: maybe");
        let rows = [
            ("cases", format!("{ONE}{bad}"), String::new()),
            ("held-out", ONE.into(), bad.clone()),
        ];
        for (row, cases, blind) in rows {
            let (_d, c, b) = two_files(&format!("names-{row}"), &cases, &blind);
            let e = load(&c, Some(&b)).err().unwrap_or_else(|| panic!("{row}: loaded"));
            let want = match row {
                "cases" => format!("{}: case zz: line 16: holdout is yes or no", c.display()),
                _ => format!("{}: case zz: line 5: holdout is yes or no", b.display()),
            };
            assert_eq!(e, want, "{row}");
        }
    }

    /// #783: an id in both files is an error naming the id and the other file.
    #[test]
    fn an_id_in_both_files_fails() {
        let (_d, c, b) = two_files("clash", &format!("{ONE}{}", held("b1")), &held("b1"));
        let e = load(&c, Some(&b)).err().expect("an id in both files loaded");
        let other = format!("already in {}", c.display());
        assert!(e.contains("case b1: ") && e.contains(&other), "{e}");
    }

    /// #783: a held-out file with no case, a header or nothing at all, loads.
    #[test]
    fn a_held_out_file_with_no_case_loads() {
        for (row, blind) in [("header", "# who may read this\n#\n"), ("empty", "")] {
            let (_d, c, b) = two_files(&format!("empty-{row}"), ONE, blind);
            let got = load(&c, Some(&b)).unwrap_or_else(|e| panic!("{row}: {e}"));
            assert_eq!(got.from, [(c, 1), (b, 0)], "{row}");
        }
    }

    /// #783: a missing held-out file is an error, never a run with no held-out case.
    #[test]
    fn a_missing_held_out_file_fails() {
        let (_d, c, b) = two_files("missing", ONE, "");
        std::fs::remove_file(&b).unwrap();
        let e = load(&c, Some(&b)).err().expect("a missing file loaded");
        assert!(e.starts_with(&format!("{}: ", b.display())), "{e}");
    }

    /// #783: the shipped held-out file loads under its rules, and its header
    /// names who may read it. Only the header is read here, never a case.
    #[test]
    fn the_shipped_held_out_file_loads_and_names_its_readers() {
        let o = opts(args("")).unwrap();
        let blind = o.blind.clone().unwrap();
        load(&o.cases, Some(&blind)).unwrap();
        let text = std::fs::read_to_string(&blind).unwrap();
        let header: Vec<&str> = text.lines().take_while(|l| !l.starts_with("=== ")).collect();
        let header = header.join(" ").replace("# ", "").replace('#', "");
        for who in [
            "the owner",
            "the orchestrator",
            "re-verify",
            "blindness",
            "commits cases",
            "must not open",
        ] {
            assert!(header.contains(who), "the header does not say {who:?}");
        }
    }

    #[test]
    fn the_shipped_cases_are_valid() {
        // both files a run reads: cases.txt and the held-out file (#783), ids only in a failure
        let cases = run_cases(&opts(args("")).unwrap()).unwrap().cases;
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

    /// #684: the reply on `last-commit-test`, whose prompt fixes a parser test,
    /// with `then` appended. Each row of the tests below adds one sentence to it.
    fn fix_with(then: &str) -> String {
        let fix = "Fix parser::tests::parses_negative in src/parser.rs from 9f3c2a1, then run \
                   cargo test.";
        format!("<prompt>{fix} {then}</prompt>")
    }

    /// #684 item 2: each lead-in of the break term has a row of its own, worded so
    /// no other alternative of the term matches it.
    #[test]
    fn each_break_lead_in_passes_on_its_own() {
        let rows = [
            (
                "temporarily flip",
                "Next, temporarily flip the sign check, confirm the test fails, and put it back.",
            ),
            (
                "try flipping",
                "Next, try flipping the sign check: the test must fail; then restore it.",
            ),
            (
                "temporarily introduce a bug",
                "Next, temporarily introduce a bug, confirm the test fails, and revert it.",
            ),
            ("invert one", "Next, invert one condition, confirm the test fails, and restore it."),
            ("flip one", "Next, flip one condition, confirm the test fails, and put it back."),
            ("then flip", "Then flip the sign check, confirm the test fails, and put it back."),
            (
                "temporarily break the code",
                "Next, temporarily break the code, confirm the test fails, and undo it.",
            ),
            (
                "temporarily revert the fix",
                "Next, temporarily revert the fix, confirm the test fails, and reapply it.",
            ),
        ];
        let rows = rows.map(|(name, s)| (name, "last-commit-test", fix_with(s), true));
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "lead-ins graded the wrong way");
    }

    /// #684 item 1: a break step may open a sentence with no lead-in, and the
    /// start of the text opens one too; "We invert" and "Do not invert" stay out.
    #[test]
    fn a_break_step_may_open_a_sentence() {
        let first = "<prompt>Invert the sign check, confirm parser::tests::parses_negative fails \
                     and restore it. Fix it in src/parser.rs from 9f3c2a1.</prompt>";
        let rows = [
            ("invert opens", "Invert the sign check, confirm the test fails, then restore it."),
            ("flip opens", "Flip the sign check, confirm the test fails, and put it back."),
            ("introduce opens", "Introduce a bug in it, confirm the test fails, and revert it."),
        ];
        let mut rows = rows.map(|(n, s)| (n, "last-commit-test", fix_with(s), true)).to_vec();
        rows.push(("invert opens the text", "last-commit-test", first.to_string(), true));
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "sentence starts graded wrong");
    }

    /// #684 item 3: a mid-sentence "then flip" or "then invert" is a fix step,
    /// not a break; the same words opening a sentence still pass.
    #[test]
    fn a_fix_step_is_no_break_step() {
        let rows = [
            (
                "then invert to pass",
                "Find where the sign is lost, then invert the check so the test passes. \
                 Revert unrelated edits.",
                false,
            ),
            (
                "then flip to pass",
                "Read the diff, then flip the condition that is backwards, and revert \
                 unrelated edits.",
                false,
            ),
            (
                "Then invert opens",
                "Then invert the sign check, confirm the test fails, and put it back.",
                true,
            ),
        ];
        let rows = rows.map(|(n, s, want)| (n, "last-commit-test", fix_with(s), want));
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "fix steps graded the wrong way");
    }

    /// #684 known limit, pinned: a fix step that opens a sentence with "Invert" or
    /// "Flip" passes as a break step. The cost of `. invert` and `. flip`; a row
    /// that turns red here means the term changed and this limit should be rechecked.
    /// #757: an abbreviation's full stop ("i.e.", "e.g.") opens a sentence too.
    #[test]
    fn a_fix_step_opening_a_sentence_passes_a_known_limit() {
        let rows = [
            (
                "Invert opens a fix",
                "Invert the sign check so the test passes, and revert unrelated edits.",
            ),
            (
                "Flip opens a fix",
                "Flip the condition that is backwards, and revert unrelated edits.",
            ),
            (
                "i.e. flip is a fix",
                "Use the narrow fix, i.e. flip the condition that is backwards, and revert \
                 unrelated edits.",
            ),
            (
                "e.g. invert is a fix",
                "Check src/parser.rs, e.g. invert the check so the test passes, and revert \
                 unrelated edits.",
            ),
        ];
        let rows = rows.map(|(n, s)| (n, "last-commit-test", fix_with(s), true));
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "the known limit moved");
    }

    /// #757 row 2, a known limit: only a full stop or the text's start opens a
    /// sentence, so a break step after a colon, a bullet, a newline, a semicolon,
    /// an open paren or a quote fails. Replies do not use these today; a row that
    /// turns red means `has` learned that start, and its row goes.
    #[test]
    fn a_break_step_after_another_start_fails_a_known_limit() {
        let rows = [
            ("a full stop", "Next. Invert the sign check, see the test fail, restore it.", true),
            ("a colon", "Next: invert the sign check, see the test fail, restore it.", false),
            ("a bullet", "- Invert the sign check, see the test fail, restore it.", false),
            ("a newline", "\nInvert the sign check, see the test fail, restore it.", false),
            ("a semicolon", "Next; invert the sign check, see the test fail, restore it.", false),
            ("an open paren", "(Invert the sign check, see the test fail, restore it.)", false),
            ("a quote", "\"Invert the sign check, see the test fail, restore it.\"", false),
        ];
        let rows = rows.map(|(n, s, want)| (n, "last-commit-test", fix_with(s), want));
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "the known limit moved");
    }

    /// #757 row 3: `mutant` and `mutants` each carry the break step alone.
    #[test]
    fn a_mutant_step_passes_on_its_own() {
        let rows = [
            ("mutant", "Check that a mutant of the sign check fails the test, and undo it."),
            ("mutants", "Run the mutants of the sign check, see the test fail, and undo them."),
        ];
        let rows = rows.map(|(n, s)| (n, "last-commit-test", fix_with(s), true));
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "mutant rows graded wrong");
    }

    /// #757 row 3: the five cases with a break step carry one break term, the
    /// `must` group with the most alternatives, so a word dropped from one copy
    /// shows here.
    #[test]
    fn the_break_term_is_the_same_in_every_case() {
        let ids = [
            "last-commit-test",
            "test-the-changed-function",
            "h-pytest-new-helper",
            "h-go-header-test",
            "failing-test-keeps-break-it",
        ];
        let term = |id: &str| {
            let must = shipped(id).must;
            must.into_iter().max_by_key(|g| g.matches('|').count()).unwrap()
        };
        let first = term(ids[0]);
        let differ: Vec<&str> = ids.into_iter().filter(|id| term(id) != first).collect();
        assert_eq!(differ, Vec::<&str>::new(), "break terms differ from {}'s", ids[0]);
    }

    /// #684 item 4: "break the code" and "mutation" need the act, and reverting
    /// the fix is a break step once it opens a sentence.
    #[test]
    fn the_older_break_words_need_the_act() {
        let rows = [
            (
                "don't break the code",
                "Make sure you don't break the code elsewhere, and revert unrelated edits.",
                false,
            ),
            (
                "mutation of state",
                "Avoid any mutation of shared state, and revert unrelated edits.",
                false,
            ),
            (
                "Then break the code",
                "Then break the code, confirm the test fails, and undo it.",
                true,
            ),
            ("break the code opens", "Break the code, confirm the test fails, and undo it.", true),
            ("revert the fix", "Revert the fix, confirm the test fails, and reapply it.", true),
            (
                "Then revert the fix",
                "Then revert the fix, confirm the test fails, and reapply it.",
                true,
            ),
            ("if it breaks, revert the fix", "If other tests break, revert the fix.", false),
        ];
        let rows = rows.map(|(n, s, want)| (n, "last-commit-test", fix_with(s), want));
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "break words graded the wrong way");
    }

    /// #684 item 4: "mutation" passes only as the name of the check, each form on
    /// its own row.
    #[test]
    fn each_mutation_check_word_passes_on_its_own() {
        let rows = [
            ("mutation test", "Next, run a quick mutation test on the sign check and revert it."),
            ("mutation tests", "Next, run the mutation tests on the sign check and revert them."),
            (
                "mutation testing",
                "Next, do some mutation testing on the sign check and revert it.",
            ),
            ("mutation check", "Next, a mutation check: change the sign check and revert it."),
        ];
        let rows = rows.map(|(name, s)| (name, "last-commit-test", fix_with(s), true));
        assert_eq!(graded_wrong(&rows), Vec::<&str>::new(), "mutation words graded the wrong way");
    }

    /// The #678 review's 32 sentences, numbered from 1: 16 break steps, then 16
    /// with none. The review graded each alone against the break term.
    const REVIEW_678_SENTENCES: [(&str, bool); 32] = [
        (
            "Then introduce a bug in parse_negative, confirm the test fails, and revert it.",
            true,
        ),
        (
            "Then temporarily invert the check and confirm the test fails, then put it back.",
            true,
        ),
        ("Flip one condition on purpose, watch the test fail, and undo it.", true),
        ("Invert the check, confirm the test fails, then restore it.", true),
        ("Introduce a small bug in slugify, confirm the test fails, and revert it.", true),
        ("Then flip the comparison and confirm the test goes red, then revert.", true),
        (
            "After it passes, break the code on purpose and check the test fails, then undo.",
            true,
        ),
        ("Deliberately break the trim and see the test fail; revert it.", true),
        ("Try inverting the sign check: the test must fail; then restore it.", true),
        ("Mutate the parser to confirm the test catches it, then revert.", true),
        (
            "Next, introduce a bug into the parser and make sure the test fails, then revert it.",
            true,
        ),
        ("Then introduce a deliberate bug, confirm the test fails, and revert.", true),
        ("Temporarily flip the condition, check the test fails, and put it back.", true),
        ("Briefly invert the check to confirm the test fails, then restore it.", true),
        (
            "Finally, flip the sign in parse_negative, confirm the test fails, and revert.",
            true,
        ),
        ("Then introduce a bug, see the test fail, and revert it.", true),
        (
            "Make sure the fix does not introduce a bug and confirm the full suite passes.",
            false,
        ),
        ("The parser is flipping the sign of negative numbers.", false),
        ("We invert the sign twice in the parser.", false),
        ("Do not invert the logic.", false),
        ("Make sure the fix does not introduce a bug.", false),
        ("The bug makes the parser flip the sign.", false),
        ("slugify was inverting the trim check.", false),
        ("Keep it small, without introducing a bug.", false),
        ("Be careful not to introduce a bug, and confirm the suite passes.", false),
        ("Check that the fix does not flip the sign of positive numbers.", false),
        ("The old code would invert a boolean when the flag was missing.", false),
        ("Avoid any change that could introduce a small bug in the rounding.", false),
        ("Find where the sign is wrong, then invert it so the test passes.", false),
        (
            "Read the diff, then flip the condition that is backwards, and run the tests.",
            false,
        ),
        ("Do not mutate global state in the helper.", false),
        ("The regression came from a change that inverted the check.", false),
    ];

    /// #684: the break term gets 6 of the review's 32 sentences wrong (master: 9).
    /// Left: lead-ins it does not list (11, 14, 15), "a deliberate bug" (12), and
    /// bare `introduce a small bug` and `mutate` in a forbidding clause (28, 31).
    #[test]
    fn the_break_term_gets_six_review_sentences_wrong() {
        let must = shipped("last-commit-test").must;
        let term = must.iter().find(|m| m.contains("on purpose")).expect("a break term");
        let wrong: Vec<usize> = (1..=32)
            .filter(|i| has(REVIEW_678_SENTENCES[i - 1].0, term) != REVIEW_678_SENTENCES[i - 1].1)
            .collect();
        assert_eq!(wrong, [11, 12, 14, 15, 28, 31], "review sentences graded the wrong way");
    }

    // --- #710: `--overlap`, on a stand-in claude in a child process ---

    const OVERLAP_CASES: &str = "=== held-a\nkind: ops\nwindow: bash · bash · /home/u\nexpect: write\nholdout: yes\n\
        --- request\nSECRET-REQUEST-TEXT keep the staging box\n\
        === tuned-b\nkind: ops\nwindow: bash · bash · /home/u\nexpect: write\n--- request\nTUNED-REQUEST-TEXT other\n";

    /// A stand-in claude: refuses a call without the overlap question as its
    /// system prompt, logs each call, and says yes to input holding `leaky`.
    /// Three answers carry the request's text and no verdict: for `garble` a
    /// reply that is not yes or no, for `errjson` an `is_error` result, for
    /// `exitcode` a result with a non-zero exit.
    const OVERLAP_STUB: &str = r#"#!/bin/sh
while [ "$1" != --system-prompt-file ]; do shift; done
grep -q paraphrases "$2" || exit 3
in=$(cat)
req=$(printf '%s\n' "$in" | sed -n '/^<request>$/{n;p;}')
echo call >> "$OVERLAP_STUB_LOG"
case "$in" in
*garble*) printf 'Maybe: %s\n' "$in" ;;
*errjson*) printf '{"type":"result","is_error":true,"result":"%s"}\n' "$req" ;;
*exitcode*) printf '{"type":"result","result":"%s"}\n' "$req"; exit 1 ;;
*leaky*) echo '{"type":"result","result":"yes","total_cost_usd":0.001}' ;;
*) echo '{"type":"result","result":"no","total_cost_usd":0.001}' ;;
esac
"#;

    /// A scratch dir with the cases, the old prompt and the stand-in claude,
    /// removed when it drops, a failed assert included.
    fn overlap_dir(name: &str) -> judge::Scratch {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch(name);
        std::fs::write(d.0.join("cases.txt"), OVERLAP_CASES).unwrap();
        std::fs::write(d.0.join("from.txt"), "an old line\n").unwrap();
        std::fs::write(d.0.join("claude"), OVERLAP_STUB).unwrap();
        let exe = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(d.0.join("claude"), exe).unwrap();
        d
    }

    /// Runs `--overlap` on `to` as the edited prompt, in this test binary
    /// re-entered at [`overlap_child`], so its real stdout and stderr are caught.
    fn overlap_run(d: &Path, to: &str) -> (Option<i32>, String, String) {
        std::fs::write(d.join("to.txt"), to).unwrap();
        let p = |f: &str| d.join(f).display().to_string();
        let args = [
            "--overlap",
            "--cases",
            &p("cases.txt"),
            "--from",
            &p("from.txt"),
            "--to",
            &p("to.txt"),
        ];
        let args = [&args[..], &["--ledger", &p("cost.tsv"), "--jobs", "2"]].concat().join("\n");
        let mut c = std::process::Command::new(std::env::current_exe().unwrap());
        // `--cases` drops the held-out file: the child puts back the dir's own
        if d.join("holdout-blind.txt").exists() {
            c.env("PANEL_EVAL_OVERLAP_BLIND", p("holdout-blind.txt"));
        }
        let o = c
            .args(["--exact", "tests::overlap_child", "--nocapture", "--test-threads=1"])
            .env("PANEL_EVAL_OVERLAP_ARGS", args)
            .env("COCKPIT_CLAUDE", p("claude"))
            .env("OVERLAP_STUB_LOG", p("calls.log"))
            .output()
            .unwrap();
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        (o.status.code(), text(&o.stdout), text(&o.stderr))
    }

    /// Not a test of its own: the child [`overlap_run`] starts.
    #[test]
    fn overlap_child() {
        if let Ok(args) = std::env::var("PANEL_EVAL_OVERLAP_ARGS") {
            // libtest's `test ... ` has no newline yet: records start on their own line
            println!();
            let mut o = opts(args.split('\n').map(String::from)).unwrap();
            o.blind = std::env::var_os("PANEL_EVAL_OVERLAP_BLIND").map(PathBuf::from);
            std::process::exit(overlap_main(&o));
        }
    }

    /// The `FLAG` records: (case id, prompt line).
    fn flags(out: &str) -> Vec<(String, String)> {
        let rows = out.lines().filter_map(|l| l.strip_prefix("FLAG\t"));
        rows.filter_map(|r| r.split_once('\t')).map(|(a, b)| (a.into(), b.into())).collect()
    }

    /// A field of the `overlap` summary record.
    fn field(out: &str, k: &str) -> Option<String> {
        let rec = out.lines().find_map(|l| l.strip_prefix("overlap\t"))?;
        rec.split('\t').find_map(|f| f.strip_prefix(&format!("{k}="))).map(String::from)
    }

    fn calls(d: &Path) -> usize {
        std::fs::read_to_string(d.join("calls.log")).map_or(0, |t| t.lines().count())
    }

    #[test]
    fn overlap_flags_a_pair_by_case_id_and_line_never_the_case_text() {
        let d = overlap_dir("ov-flag");
        let (code, out, err) = overlap_run(&d.0, "an old line\na leaky line\n");
        assert_eq!(code, Some(1), "a flag fails the run: {out}{err}");
        assert_eq!(flags(&out), [("held-a".to_string(), "a leaky line".to_string())]);
        assert!(!out.contains("SECRET-REQUEST") && !err.contains("SECRET-REQUEST"), "{out}{err}");
    }

    #[test]
    fn overlap_with_no_flag_exits_0() {
        let d = overlap_dir("ov-clean");
        let (code, out, err) = overlap_run(&d.0, "a plain line\n");
        assert_eq!(code, Some(0), "{out}{err}");
        assert_eq!((flags(&out).len(), field(&out, "flagged").as_deref()), (0, Some("0")));
        assert_eq!(field(&out, "pairs").as_deref(), Some("1"));
    }

    #[test]
    fn overlap_judges_only_held_out_cases() {
        let d = overlap_dir("ov-held");
        overlap_run(&d.0, "a plain line\n");
        assert_eq!(calls(&d.0), 1, "one line, one held-out case");
    }

    #[test]
    fn overlap_rerun_reads_the_cache_which_holds_no_text() {
        let d = overlap_dir("ov-cache");
        overlap_run(&d.0, "a plain line\nanother line\n");
        let (_, out, _) = overlap_run(&d.0, "a plain line\nanother line\n");
        assert_eq!((calls(&d.0), field(&out, "cached").as_deref()), (2, Some("2")));
        let cache = std::fs::read_to_string(d.0.join("overlap-cache.tsv")).unwrap();
        assert!(!cache.contains("SECRET-REQUEST") && !cache.contains("plain"), "{cache}");
    }

    /// #757 row 10: the cache's question column is the summary's `question=`, so
    /// the hash a verdict is filed under and the one the run reports cannot drift.
    #[test]
    fn overlap_cache_question_column_is_the_summary_question() {
        let d = overlap_dir("ov-qcol");
        let (_, out, _) = overlap_run(&d.0, "a plain line\n");
        let cache = std::fs::read_to_string(d.0.join("overlap-cache.tsv")).unwrap();
        let cols: Vec<&str> = cache.lines().filter_map(|l| l.split('\t').nth(3)).collect();
        // one pair, one verdict: an empty cache or a missing field is no match
        assert_eq!(cols, [field(&out, "question").unwrap_or_default().as_str()], "{out}");
    }

    #[test]
    fn overlap_failed_call_never_prints_the_answer_and_fails_the_run() {
        let d = overlap_dir("ov-garble");
        // no verdict, an is_error result, a non-zero exit: each answer holds the request
        let (code, out, err) = overlap_run(&d.0, "garble this\nerrjson this\nexitcode this\n");
        assert_eq!((code, field(&out, "failed").as_deref()), (Some(1), Some("3")), "{out}{err}");
        assert!(!out.contains("SECRET-REQUEST") && !err.contains("SECRET-REQUEST"), "{out}{err}");
    }

    #[test]
    fn overlap_with_no_changed_line_calls_nothing() {
        let d = overlap_dir("ov-empty");
        let (code, out, _) = overlap_run(&d.0, "an old line\n\n");
        assert_eq!((code, calls(&d.0), field(&out, "lines").as_deref()), (Some(0), 0, Some("0")));
    }

    /// A typo in `--only` leaves nothing to check against: that is no pass.
    #[test]
    fn overlap_with_lines_but_no_held_out_case_exits_2() {
        let d = overlap_dir("ov-nocase");
        std::fs::write(d.0.join("cases.txt"), OVERLAP_CASES.replace("holdout: yes\n", "")).unwrap();
        let (code, _, err) = overlap_run(&d.0, "a plain line\n");
        assert_eq!((code, calls(&d.0)), (Some(2), 0), "{err}");
    }

    /// #783: the overlap check judges the held-out file's cases too.
    #[test]
    fn overlap_judges_the_held_out_files_cases() {
        let d = overlap_dir("ov-blind");
        let (held, tuned) = OVERLAP_CASES.split_at(OVERLAP_CASES.find("=== tuned-b").unwrap());
        std::fs::write(d.0.join("cases.txt"), tuned).unwrap();
        std::fs::write(d.0.join("holdout-blind.txt"), held).unwrap();
        let (code, out, err) = overlap_run(&d.0, "a plain line\n");
        assert_eq!((code, calls(&d.0)), (Some(0), 1), "{out}{err}");
    }

    #[test]
    fn overlap_spend_goes_in_the_ledger() {
        let d = overlap_dir("ov-ledger");
        overlap_run(&d.0, "a plain line\n");
        let ledger = std::fs::read_to_string(d.0.join("cost.tsv")).unwrap();
        let row: Vec<&str> = ledger.lines().last().unwrap().split('\t').collect();
        assert_eq!((row[1], row[4]), ("overlap", "0.0010"));
    }

    /// #792: a held-out case's text stays out of every line and row.
    mod quiet {
        use super::*;
        use serde_json::{Value, json};

        /// Made-up cases: one tune, two held out, each with phrases of its own.
        const QUIET: &str = "=== tune-q\nkind: bug\nwindow: bash · bash · /home/u\n\
            expect: write\nmust: al-beacon; ga-relay\nmust_not: ze-trap\n\
            --- request\nfix this\n\
            === held-q\nkind: co-kd\nwindow: bash · bash · /home/u\n\
            expect: write\nholdout: yes\nmust: qz-lamp; em-quill\n\
            must_not: vi-nail\n--- request\nfix that qz-echo\n\
            === held-ask\nkind: co-kd\nwindow: bash · bash · /home/u\n\
            expect: ask\nholdout: yes\nask_any: sf-gate\n--- request\ndeploy it\n";

        /// Every phrase of the held-out cases in [`QUIET`], kind and request included.
        const HELD: [&str; 6] = ["qz-lamp", "em-quill", "vi-nail", "sf-gate", "co-kd", "qz-echo"];

        /// A reply that holds a must_not and some must phrases of every case.
        const LEAKY: &str = "<prompt>Use ze-trap, vi-nail, al-beacon and qz-lamp.</prompt>";

        /// A question naming the held-out ask case's term: a passing reply.
        const ON_TARGET: &str = "<question>Which sf-gate?</question>";

        /// The error a failed call may carry: claude's stderr echoing its input.
        const ECHO: &str = "bad input: fix that qz-echo";

        /// A stand-in claude that echoes its request's phrase on stderr and fails.
        const ECHOER: &str = "#!/bin/sh\ngrep -o 'fix that qz-echo' | head -n 1 >&2\nexit 1\n";

        /// Case `id` of `cases` answered with `reply`, graded.
        fn ended<'a>(cases: &'a [Case], id: &str, reply: &'a str) -> Ended<'a> {
            let c = cases.iter().find(|c| c.id == id).unwrap();
            Ended {
                case: c,
                run: 0,
                model: "sonnet",
                system: 1,
                prompt: "p",
                ms: 7,
                err: None,
                class: "call failed",
                grade: Some(grade(c, reply)),
                usage: Usage::default(),
                reply,
                explore: false,
                read: None,
                calls: None,
            }
        }

        /// Case `id` of `cases` as a failed call whose error is `why`.
        fn failed<'a>(cases: &'a [Case], id: &str, why: &str) -> Ended<'a> {
            let mut e = ended(cases, id, "");
            (e.err, e.grade) = (Some(why.into()), None);
            e
        }

        /// A write case's line up to its detail, as [`ended`] makes it.
        fn head(mark: &str, id: &str, kind: &str) -> String {
            format!("{mark} {id:<24} {kind:<13} want write       7 ms")
        }

        /// The phrases of [`HELD`] that `text` holds.
        fn leaked(text: &str) -> Vec<&'static str> {
            HELD.into_iter().filter(|p| text.contains(p)).collect()
        }

        fn row_of(e: &Ended) -> Value {
            e.row(json!({}))
        }

        /// A stand-in claude at `dir`/fake-claude running `script`.
        fn fake(dir: &Path, script: &str) -> String {
            use std::os::unix::fs::PermissionsExt;
            let f = dir.join("fake-claude");
            std::fs::write(&f, script).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            f.display().to_string()
        }

        /// A judge verdict whose critique quotes the held-out case's phrases.
        fn quoting() -> judge::Judged {
            let critique = "says vi-nail, lacks em-quill".to_string();
            judge::Judged {
                verdict: Ok(judge::Verdict { pass: false, critique }),
                cost_usd: None,
            }
        }

        #[test]
        fn a_failing_held_out_line_shows_only_id_want_ms_and_counts() {
            let cases = parse_cases(QUIET).unwrap();
            let l = ended(&cases, "held-q", LEAKY).line();
            assert_eq!(l, format!("{}  held out: has 1, missing 1", head("FAIL", "held-q", "")));
            assert_eq!(leaked(&l), Vec::<&str>::new(), "{l}");
        }

        #[test]
        fn a_passing_held_out_question_stays_unshown() {
            let cases = parse_cases(QUIET).unwrap();
            let l = ended(&cases, "held-ask", ON_TARGET).line();
            assert!(l.starts_with("ok   held-ask ") && l.ends_with(" ms  held out"), "{l}");
            assert_eq!(leaked(&l), Vec::<&str>::new(), "{l}");
        }

        #[test]
        fn a_failing_tune_line_prints_as_before() {
            let cases = parse_cases(QUIET).unwrap();
            let want = format!("{}  has: ze-trap", head("FAIL", "tune-q", "bug"));
            assert_eq!(ended(&cases, "tune-q", LEAKY).line(), want);
            let l = ended(&cases, "tune-q", "<prompt>Use al-beacon.</prompt>").line();
            assert!(l.ends_with("  wrote, missing: ga-relay"), "{l}");
        }

        #[test]
        fn a_held_out_row_nulls_its_phrases_and_counts_them() {
            let cases = parse_cases(QUIET).unwrap();
            let row = row_of(&ended(&cases, "held-q", LEAKY));
            let text = ["kind", "question", "missing", "forbidden"].map(|k| &row[k]);
            assert_eq!(text, [&Value::Null; 4], "{row}");
            assert_eq!([&row["missing_n"], &row["forbidden_n"]], [&json!(1), &json!(1)]);
            assert_eq!(
                (&row["id"], &row["holdout"], &row["pass"]),
                (&json!("held-q"), &json!(true), &json!(false))
            );
        }

        #[test]
        fn a_tune_row_carries_its_phrases_as_before() {
            let cases = parse_cases(QUIET).unwrap();
            let row = row_of(&ended(&cases, "tune-q", LEAKY));
            let got = [&row["forbidden"], &row["missing"], &row["reply"]];
            assert_eq!(got, [&json!(["ze-trap"]), &json!(["ga-relay"]), &json!(LEAKY)]);
            assert!(row.get("missing_n").is_none() && row.get("forbidden_n").is_none(), "{row}");
        }

        /// The round trip: a stand-in claude, sent the held-out case's own
        /// request, echoes it on stderr and fails; the line and row show the class.
        #[test]
        fn a_held_out_failed_call_prints_only_its_class() {
            let dir = scratch("quiet-err");
            let cases = parse_cases(QUIET).unwrap();
            let c = cases.iter().find(|c| c.id == "held-q").unwrap();
            let (system, prompt, profile, _) = panel_call(c, &c.path, false);
            let call = Call {
                claude: fake(&dir.0, ECHOER),
                ..base_call(&dir.0)
            };
            let mut req = Request::spawn_with(&call, &profile, &system, &prompt, "sonnet");
            let answer = loop {
                match req.poll() {
                    Some(a) => break a,
                    None => std::thread::yield_now(),
                }
            };
            let class = fail_class(&answer);
            let err = call_output(answer, req.raw()).unwrap_err();
            assert_eq!(leaked(&err), ["qz-echo"], "the stand-in echoed the request: {err}");
            let mut e = failed(&cases, "held-q", &err);
            e.class = class;
            assert_eq!(e.line(), format!("{}  held out: call failed", head("ERR ", "held-q", "")));
            assert_eq!(row_of(&e)["error"], json!("call failed"));
            assert_eq!(leaked(&row_of(&e).to_string()), Vec::<&str>::new());
        }

        #[test]
        fn fail_class_names_a_timeout_a_budget_stop_and_any_other_failure() {
            let answers = [
                Err(Fail::Timeout),
                Err(Fail::Budget { limit_usd: 0.5 }),
                Err(Fail::Failed(ECHO.into())),
                Ok("{}".into()),
            ];
            let want = ["took too long", "stopped at its budget", "call failed", "call failed"];
            assert_eq!(answers.map(|a| fail_class(&a)), want);
        }

        #[test]
        fn a_tune_failed_call_keeps_its_error() {
            let cases = parse_cases(QUIET).unwrap();
            let e = failed(&cases, "tune-q", ECHO);
            assert_eq!(e.line(), format!("{}  {ECHO}", head("ERR ", "tune-q", "bug")));
            assert_eq!(row_of(&e)["error"], json!(ECHO));
        }

        #[test]
        fn a_held_out_explore_keeps_its_read_count() {
            let cases = parse_cases(QUIET).unwrap();
            let mut e = ended(&cases, "held-q", LEAKY);
            (e.explore, e.read) = (true, Some(vec!["/r/a.rs".into()]));
            let l = e.line();
            assert!(l.ends_with(" ms  [read 1] held out: has 1, missing 1"), "{l}");
        }

        #[test]
        fn a_held_out_row_nulls_a_field_it_does_not_know() {
            let row = json!({"id": "held-q", "pass": false, "later": "vi-nail", "holdout": true});
            let want = json!({"id": "held-q", "pass": false, "later": null, "holdout": true});
            assert_eq!(blind_row(row), want);
        }

        #[test]
        fn a_row_that_does_not_say_holdout_no_is_held() {
            let rows = [json!({"holdout": false}), json!({"holdout": true}), json!({})];
            assert_eq!(rows.map(|r| held_row(&r)), [false, true, true]);
        }

        #[test]
        fn a_held_out_verdict_line_and_row_carry_no_critique() {
            let cases = parse_cases(QUIET).unwrap();
            let row = row_of(&ended(&cases, "held-q", LEAKY));
            assert_eq!(verdict_line(&row, &quoting()), "judge FAIL held-q");
            let mut rows = vec![(row, judge::Input::default())];
            judge_apply(&mut rows, &[0], &[quoting()], "haiku");
            let r = &rows[0].0;
            assert_eq!((&r["critique"], &r["judge"]), (&Value::Null, &json!(false)));
        }

        /// The round trip: a stand-in judge, sent the held-out case's request,
        /// echoes it on stderr and fails; its line and row show none of it.
        #[test]
        fn a_held_out_judge_error_prints_only_its_class() {
            let dir = scratch("quiet-judge");
            let cases = parse_cases(QUIET).unwrap();
            let c = cases.iter().find(|c| c.id == "held-q").unwrap();
            let reply = "<prompt>x</prompt>";
            let input = [judge_input(c, reply, None)];
            let echoer = fake(&dir.0, ECHOER);
            let judged = judge::judge_all(&echoer, "haiku", &input, 1, &dir.0);
            let why = judged[0].verdict.clone().unwrap_err();
            assert_eq!(leaked(&why), ["qz-echo"], "the stand-in echoed the request: {why}");
            let row = row_of(&ended(&cases, "held-q", reply));
            assert_eq!(verdict_line(&row, &judged[0]), "judge ERR  held-q  held out: call failed");
            let mut rows = vec![(row, judge::Input::default())];
            judge_apply(&mut rows, &[0], &judged, "haiku");
            assert_eq!(leaked(&rows[0].0.to_string()), Vec::<&str>::new(), "{}", rows[0].0);
        }

        #[test]
        fn a_held_out_row_carries_no_phrase_in_any_field() {
            let cases = parse_cases(QUIET).unwrap();
            let mut e = ended(&cases, "held-q", LEAKY);
            let grep = vec!["Grep qz-lamp /r".to_string()];
            (e.explore, e.read, e.calls) = (true, Some(vec!["/r/vi-nail.rs".into()]), Some(grep));
            for row in [row_of(&e), row_of(&ended(&cases, "held-ask", ON_TARGET))] {
                assert_eq!(leaked(&row.to_string()), Vec::<&str>::new(), "{row}");
            }
            let row = row_of(&e);
            assert_eq!(["reply", "reads", "files_read"].map(|k| &row[k]), [&Value::Null; 3]);
        }

        #[test]
        fn by_default_a_held_out_failure_prints_no_reply_or_error() {
            let cases = parse_cases(QUIET).unwrap();
            for e in [ended(&cases, "held-q", LEAKY), failed(&cases, "held-q", ECHO)] {
                assert_eq!(e.printed(false), e.line());
                assert_eq!(leaked(&e.printed(false)), Vec::<&str>::new());
            }
        }

        #[test]
        fn show_holdout_prints_a_held_out_failing_reply_under_a_warning() {
            let cases = parse_cases(QUIET).unwrap();
            let e = ended(&cases, "held-q", LEAKY);
            let p = e.printed(true);
            let lines: Vec<&str> = p.lines().collect();
            assert_eq!(lines.len(), 3, "{p}");
            let warn = "warning: --show-holdout prints held-out case held-q";
            assert!(lines[1].starts_with(warn), "{p}");
            assert_eq!(lines[2], format!("reply: {LEAKY}"));
            assert_eq!(row_of(&e)["reply"], Value::Null, "the flag never reaches --out");
        }

        #[test]
        fn show_holdout_prints_a_held_out_full_error_and_the_row_keeps_the_class() {
            let cases = parse_cases(QUIET).unwrap();
            let e = failed(&cases, "held-q", ECHO);
            let p = e.printed(true);
            assert!(p.contains("\nwarning: --show-holdout "), "{p}");
            assert!(p.ends_with(&format!("\nerror: {ECHO}")), "{p}");
            assert_eq!(row_of(&e)["error"], json!("call failed"));
        }

        #[test]
        fn show_holdout_adds_nothing_for_a_pass_or_a_tune_case() {
            let cases = parse_cases(QUIET).unwrap();
            let tune = [ended(&cases, "tune-q", LEAKY), failed(&cases, "tune-q", ECHO)];
            for e in tune.into_iter().chain([ended(&cases, "held-ask", ON_TARGET)]) {
                assert_eq!(e.printed(true), e.line());
            }
        }

        /// [`QUIET`] and two more held-out cases, `hb-1` and `hb-10`, one id
        /// inside the other.
        fn with_hb() -> String {
            let at = QUIET.find("=== held-q\n").unwrap();
            let held = &QUIET[at..QUIET.find("=== held-ask").unwrap()];
            let hb = |id: &str| held.replace("=== held-q\n", &format!("=== {id}\n"));
            format!("{QUIET}{}{}", hb("hb-1"), hb("hb-10"))
        }

        #[test]
        fn show_holdout_refuses_unless_only_names_one_case_by_its_whole_id() {
            let rows = [
                ("many ids", "-", 5),
                ("a kind", "co-kd", 4),
                ("in another", "hb-1", 2),
                ("a part", "ld-as", 1),
            ];
            for (name, only, n) in rows {
                let flags = ["--show-holdout", "--runs", "1", "--only", only];
                let (code, out, err, _) = rig_run_on("quiet-one", &with_hb(), &flags);
                assert_eq!(code, Some(2), "{name}: {out}{err}");
                let silent = [" cases (", "held out", "warning"].iter().all(|w| !out.contains(w));
                assert!(silent, "{name}: {out}");
                let want = format!("{SHOW_ONE}; {n} matched");
                assert!(err.contains(&want), "{name}: {err}");
                assert!(!err.contains("hb-") && leaked(&err).is_empty(), "{name}: {err}");
            }
        }

        #[test]
        fn check_prints_no_show_holdout_banner() {
            let flags = ["--check", "--show-holdout", "--runs", "1", "--only", "held-q"];
            let (code, out, err, _) = rig_run("quiet-check", &flags);
            assert_eq!(code, Some(0), "{out}{err}");
            assert!(out.contains("1 cases (") && !out.contains(SHOW_BANNER), "{out}");
        }

        #[test]
        fn show_holdout_makes_one_paid_call() {
            let o = |s: &str| opts(args(s));
            assert_eq!(o("--show-holdout --only a --runs 1").unwrap().runs, 1);
            let flags = ["", "--runs 3", "--runs 1 --judge"];
            let refused = flags.map(|s| o(&format!("--show-holdout --only a {s}")));
            let want = [
                "--show-holdout makes one call: add --runs 1",
                "--show-holdout makes one call: add --runs 1",
                "--show-holdout makes one call: --judge adds another",
            ];
            assert_eq!(refused.map(|r| r.unwrap_err()), want);
        }

        #[test]
        fn show_holdout_is_off_by_default_and_needs_only() {
            assert!(!opts(args("")).unwrap().show_holdout);
            assert!(opts(args("--show-holdout --only held-q --runs 1")).unwrap().show_holdout);
            let e = opts(args("--show-holdout")).unwrap_err();
            assert_eq!(e, "--show-holdout needs --only: it reruns a held-out case, never a sweep");
        }

        #[test]
        fn a_show_holdout_run_warns_first_and_still_writes_no_text() {
            let flags = ["--show-holdout", "--only", "held-q", "--runs", "1"];
            let (code, out, err, rows) = rig_run("quiet-show", &flags);
            assert_eq!(code, Some(0), "{out}{err}");
            let mine: Vec<&str> = out.lines().skip_while(|l| !l.starts_with("1 cases")).collect();
            assert_eq!(mine[1], SHOW_BANNER, "{out}");
            let warn = "\nwarning: --show-holdout prints held-out case held-q;";
            assert!(out.contains(warn), "{out}");
            let reply = "\nreply: <prompt>Use ze-trap, vi-nail, al-beacon and qz-lamp.</prompt>\n";
            // the summary opens with one blank line; the reply adds none of its own
            assert!(out.contains(reply) && !out.contains(&format!("{reply}\n\n")), "{out}");
            assert_eq!(leaked(&rows), Vec::<&str>::new(), "{rows}");
        }

        /// A stand-in claude for a whole run: its version, a judge's verdict
        /// quoting phrases, and a reply holding phrases of every case.
        const RIG_CLAUDE: &str = "#!/bin/sh\n[ \"$1\" = --version ] && { echo 9.9.9; exit 0; }\n\
            case \"$(cat)\" in *'Grade the reply.'*)\n\
            printf 'VERDICT: FAIL\\nCRITIQUE: vi-nail, ze-trap\\n' ;;\n\
            *) printf '<prompt>Use ze-trap, vi-nail, al-beacon and qz-lamp.</prompt>\\n' ;; esac\n";

        /// A child run: its exit code, stdout, stderr and `--out` rows.
        type Ran = (Option<i32>, String, String, String);

        /// Runs [`QUIET`] with `extra` flags in this test binary, re-entered
        /// at [`rig_child`] on a stand-in claude.
        fn rig_run(name: &str, extra: &[&str]) -> Ran {
            rig_run_on(name, QUIET, extra)
        }

        /// [`rig_run`] on the cases `cases`.
        fn rig_run_on(name: &str, cases: &str, extra: &[&str]) -> Ran {
            rig_run_with(name, cases, RIG_CLAUDE, extra)
        }

        /// [`rig_run_on`] with `script` as the stand-in claude.
        fn rig_run_with(name: &str, cases: &str, script: &str, extra: &[&str]) -> Ran {
            let d = scratch(name);
            std::fs::write(d.0.join("cases.txt"), cases).unwrap();
            let claude = fake(&d.0, script);
            let p = |f: &str| d.0.join(f).display().to_string();
            let (cases, rows, ledger) = (p("cases.txt"), p("rows.jsonl"), p("cost.tsv"));
            let base = ["--cases", &cases, "--no-hermetic", "--runs", "1", "--jobs", "1"];
            let out = ["--out", &rows, "--ledger", &ledger];
            let args = [&base[..], &out, extra].concat().join("\n");
            let mut c = std::process::Command::new(std::env::current_exe().unwrap());
            c.args(["--exact", "tests::quiet::rig_child", "--nocapture", "--test-threads=1"]);
            for (k, v) in [("CACHE", "cache"), ("CONFIG", "config"), ("STATE", "state")] {
                c.env(format!("XDG_{k}_HOME"), p(v));
            }
            c.env("PANEL_EVAL_RIG_ARGS", args).env("COCKPIT_CLAUDE", claude);
            let o = c.output().unwrap();
            let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
            let rows = std::fs::read_to_string(d.0.join("rows.jsonl")).unwrap_or_default();
            (o.status.code(), text(&o.stdout), text(&o.stderr), rows)
        }

        /// Not a test of its own: the child [`rig_run`] starts.
        #[test]
        fn rig_child() {
            if let Ok(args) = std::env::var("PANEL_EVAL_RIG_ARGS") {
                println!();
                let o = opts(args.split('\n').map(String::from)).unwrap();
                run(&o, Err(std::io::Error::other("a test child has no rig binary")));
                std::process::exit(0);
            }
        }

        /// The lines of `text` that are not the tune case's own: its reply and
        /// critique hold phrases of every case.
        fn not_tune(text: &str) -> String {
            text.lines().filter(|l| !l.contains("tune-q")).collect::<Vec<_>>().join("\n")
        }

        #[test]
        fn a_run_prints_and_writes_no_held_out_phrase_and_every_tune_one() {
            for (name, extra) in [("quiet-run", &[][..]), ("quiet-judge", &["--judge"][..])] {
                let (code, out, err, rows) = rig_run(name, extra);
                assert_eq!(code, Some(0), "{name}: {out}{err}");
                let all = format!("{out}\n{err}\n{rows}");
                assert_eq!(leaked(&not_tune(&all)), Vec::<&str>::new(), "{name}: {all}");
                let tune = out.contains("has: ze-trap") && rows.contains("\"ze-trap\"");
                assert!(tune, "{name}: {all}");
                assert_eq!(rows.lines().count(), 3, "{name}: {rows}");
            }
        }

        #[test]
        fn compare_reads_a_held_out_rows_file_count_as_a_tune_rows_list() {
            let cases = parse_cases(QUIET).unwrap();
            let lines: Vec<String> = ["tune-q", "held-q"]
                .map(|id| {
                    let mut e = ended(&cases, id, LEAKY);
                    (e.explore, e.read) = (true, Some(vec!["/r/a.rs".into(), "/r/b.rs".into()]));
                    row_of(&e).to_string()
                })
                .into();
            let rows = read_rows("out", &lines.join("\n")).unwrap();
            assert_eq!(rows.iter().map(|r| r.files_read).collect::<Vec<_>>(), [Some(2), Some(2)]);
            assert_eq!(reads_line(&rows).unwrap(), "2.0 files mean, 2 at most, over 2 calls");
        }

        // --- #789: --router-prompt, on a stand-in claude ---

        /// A router prompt with blanks at its ends, which the call must keep.
        const ROUTER: &str = "ROUTER-789 send each request to one agent.\nNo trim here.  \n";

        /// A stand-in claude that copies each call's system prompt file into `cap`.
        fn router_claude(cap: &Path) -> String {
            let head = "#!/bin/sh\n[ \"$1\" = --version ] && { echo 9.9.9; exit 0; }\n\
                while [ $# -gt 1 ] && [ \"$1\" != --system-prompt-file ]; do shift; done\n\
                [ \"$1\" = --system-prompt-file ] || exit 3\n";
            let tail = "cat >/dev/null\nprintf '<prompt>al-beacon</prompt>\\n'\n";
            format!("{head}cp \"$2\" \"$(mktemp '{}/sys.XXXXXX')\"\n{tail}", cap.display())
        }

        /// The `field` of each `--out` row in `rows`.
        fn fields(rows: &str, field: &str) -> Vec<Value> {
            rows.lines()
                .map(|l| serde_json::from_str::<Value>(l).unwrap()[field].clone())
                .collect()
        }

        #[test]
        fn a_router_run_hands_every_call_exactly_the_router_text() {
            let cap = scratch("router-cap");
            let router = cap.0.join("router.txt");
            std::fs::write(&router, ROUTER).unwrap();
            let flags = ["--router-prompt", &router.display().to_string()];
            let claude = router_claude(&cap.0);
            let (code, out, err, _) = rig_run_with("router-sys", QUIET, &claude, &flags);
            assert_eq!(code, Some(0), "{out}{err}");
            assert_eq!(sent(&cap.0, &router), [ROUTER; 3]);
        }

        #[test]
        fn a_router_runs_rows_hash_the_router_file_not_write_txt() {
            let d = scratch("router-rows");
            let router = d.0.join("router.txt");
            std::fs::write(&router, ROUTER).unwrap();
            let flags = ["--router-prompt", &router.display().to_string()];
            let (code, out, err, rows) = rig_run("router-on", &flags);
            assert_eq!(code, Some(0), "{out}{err}");
            let hash = json!(cockpit_tui::panel::prompts::files_hash(&[ROUTER]));
            assert_eq!(fields(&rows, "prompt"), vec![hash.clone(); 3]);
            assert_eq!(fields(&rows, "system"), vec![json!(format!("{:016x}", fnv(ROUTER))); 3]);
            // without the flag the rows keep the shipped write prompt's hash, as `load` trims it
            let (_, _, _, plain) = rig_run("router-off", &[]);
            let ship = [prompts::WRITE, prompts::WRITE_EXAMPLES].map(|t| t.trim_end_matches('\n'));
            let ship = json!(prompts::files_hash(&ship));
            assert_eq!(fields(&plain, "prompt"), vec![ship.clone(); 3]);
            assert_ne!(ship, hash);
        }

        /// The system prompt files the stand-in claude copied into `cap`.
        fn sent(cap: &Path, router: &Path) -> Vec<String> {
            let files = std::fs::read_dir(cap).unwrap().map(|e| e.unwrap().path());
            files.filter(|p| p != router).map(|p| std::fs::read_to_string(p).unwrap()).collect()
        }

        /// An `explore:` case under `--no-explore` is a text call, sent the router
        /// text alone; exploring, the request appends its where-note to it.
        #[test]
        fn a_router_explore_case_gets_the_where_note_only_when_it_explores() {
            let cases = ONE.replace("expect: write", "expect: write\nexplore: invoicer");
            let rows = [("router-noex", &["--no-explore"][..], false), ("router-ex", &[], true)];
            for (name, extra, explores) in rows {
                let cap = scratch(&format!("{name}-cap"));
                let router = cap.0.join("router.txt");
                std::fs::write(&router, ROUTER).unwrap();
                let r = router.display().to_string();
                let flags = [&["--router-prompt", r.as_str()][..], extra].concat();
                let claude = router_claude(&cap.0);
                let (code, out, err, rows) = rig_run_with(name, &cases, &claude, &flags);
                assert_eq!(code, Some(0), "{name}: {out}{err}");
                assert_eq!(fields(&rows, "explore"), [json!(explores)], "{name}");
                let read = fields(&rows, "files_read");
                assert!(explores || read == [Value::Null], "{name}: {rows}");
                let s = sent(&cap.0, &router);
                assert_eq!((s.len(), s[0] == ROUTER), (1, !explores), "{name}: {s:?}");
                let noted = s[0].starts_with(&format!("{ROUTER}\n\nThe repository is at "));
                assert_eq!(noted, explores, "{name}: {s:?}");
            }
        }

        /// A side holding a write run's rows and a router run's for the same
        /// ids is refused, naming both hashes; one prompt per side pairs.
        #[test]
        fn compare_refuses_a_side_mixing_router_and_write_rows_naming_both() {
            let d = scratch("router-mix");
            let router = d.0.join("router.txt");
            std::fs::write(&router, ROUTER).unwrap();
            let flags = ["--router-prompt", &router.display().to_string()];
            let (_, _, _, routed) = rig_run("router-mix-on", &flags);
            let (_, _, _, plain) = rig_run("router-mix-off", &[]);
            let rows = |t: &str| read_rows("rows", t).unwrap();
            let (m, w) = (PathBuf::from("mixed.jsonl"), PathBuf::from("write.jsonl"));
            let both = rows(&format!("{plain}{routed}"));
            let e = mixed_error(&[(&m, &both), (&w, &rows(&plain))]).unwrap();
            let wh = fields(&plain, "prompt")[0].as_str().unwrap().to_string();
            let rh = cockpit_tui::panel::prompts::files_hash(&[ROUTER]);
            for id in ["tune-q", "held-q", "held-ask"] {
                let want = format!("{id} in mixed.jsonl ({wh}, {rh})");
                assert!(e.contains(&want), "{id}: {e}");
            }
            assert!(!e.contains("write.jsonl"), "{e}");
            assert_eq!(mixed_error(&[(&w, &rows(&plain)), (&m, &rows(&routed))]), None);
        }

        #[test]
        fn a_tune_verdict_line_and_row_keep_the_critique() {
            let cases = parse_cases(QUIET).unwrap();
            let row = row_of(&ended(&cases, "tune-q", LEAKY));
            let want = format!("judge FAIL {:<24} says vi-nail, lacks em-quill", "tune-q");
            assert_eq!(verdict_line(&row, &quoting()), want);
            let mut rows = vec![(row, judge::Input::default())];
            judge_apply(&mut rows, &[0], &[quoting()], "haiku");
            assert_eq!(rows[0].0["critique"], json!("says vi-nail, lacks em-quill"));
        }
    }

    #[test]
    fn overlap_options_refuse_a_lone_or_misplaced_file_and_a_flag_as_base() {
        let o = |a: &[&str]| opts(a.iter().map(|s| s.to_string())).unwrap_err();
        assert_eq!(o(&["--overlap", "--from", "a"]), "--from and --to go together");
        assert_eq!(o(&["--from", "a", "--to", "b"]), "--from, --to and --base are for --overlap");
        assert_eq!(o(&["--overlap", "--base", "--output=x"]), "--base: --output=x is not a revision");
        assert_eq!(
            o(&["--overlap", "--base", "v1", "--from", "a", "--to", "b"]),
            "--base is for git diff, not --from and --to"
        );
    }
}
