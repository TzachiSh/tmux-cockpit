//! The write prompt's evaluation (#410): labelled cases through the panel's own
//! prompt, envelope, model route, `claude -p` call and reply parser, scored on
//! whether it asks when it should and carries the facts it must.
//!
//! It calls `claude` once per case and run, so it needs the network and costs
//! money: run it by hand, never from `make test`.
//!
//!     cargo run --example panel_eval -- [--model M] [--runs N] [--jobs J]
//!         [--only TEXT] [--cases FILE] [--out FILE.jsonl] [--check]
//!
//! `--check` only reads and validates the cases. To compare a prompt edit, put
//! it at `$XDG_CONFIG_HOME/tmux-cockpit/prompts/write.txt` (the panel's own
//! override) and run twice; the summary prints a hash of the system prompt.
use cockpit_tui::panel::capture::{Capture, window_kind};
use cockpit_tui::panel::envelope::{Flow, OutType, build};
use cockpit_tui::panel::parse::{self, Reply};
use cockpit_tui::panel::prompts::{self, Ctx};
use cockpit_tui::panel::request::{Call, Request};
use cockpit_tui::panel::route::route;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A copy of the panel's private `ROUNDS_WRITE` (app.rs): the first call sees
/// all of it. Change both together.
const QUESTIONS: usize = 3;

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
}

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
            expect: Expect::Write,
            ask_any: vec![],
            must: vec![],
            must_not: vec![],
            holdout: false,
            screen: String::new(),
            repo: String::new(),
            request: String::new(),
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
                "note" => {}
                k => return Err(err(&format!("unknown key {k}"))),
            }
        }
        c.expect = expect.ok_or_else(|| err("no expect"))?;
        let parts: Vec<&str> = bodies.split("\n--- ").filter(|b| !b.is_empty()).collect();
        for (i, body) in parts.iter().enumerate() {
            let (name, text) = body.split_once('\n').unwrap_or((body, ""));
            // The comments that head the next case trail the last body: they are not its text.
            let mut lines: Vec<&str> = text.lines().collect();
            if i + 1 == parts.len() {
                while lines.last().is_some_and(|l| l.trim().is_empty() || l.starts_with('#')) {
                    lines.pop();
                }
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

/// `needle` as a whole word, any case: `plan` is not in "explanation" nor `36`
/// in "360". `a|b` is any of them. An end that is not a letter or digit (`--json`,
/// `/code-review`) needs no boundary there.
fn has(hay: &str, needle: &str) -> bool {
    let hay = hay.to_lowercase();
    needle
        .split('|')
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty())
        .any(|n| {
            let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
            hay.match_indices(n.as_str()).any(|(i, m)| {
                let (first, last) = (m.chars().next(), m.chars().last());
                (!word(first) || !word(hay[..i].chars().last())) && (!word(last) || !word(hay[i + m.len()..].chars().next()))
            })
        })
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

#[derive(Debug, Default, PartialEq)]
struct Grade {
    asked: bool,
    question: String,
    /// For `ask`: the question named one of `ask_any` (true when there is none).
    on_target: bool,
    /// For `write`: the `must` facts the reply lacks.
    missing: Vec<String>,
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
    g.forbidden = c.must_not.iter().filter(|m| has(&text, m)).cloned().collect();
    let asked_well = g.asked && g.on_target;
    let wrote_well = !g.asked && g.missing.is_empty();
    g.pass = g.forbidden.is_empty()
        && match c.expect {
            Expect::Ask => asked_well,
            Expect::Write => wrote_well,
            Expect::Either => asked_well || wrote_well,
        };
    g
}

struct Opts {
    cases: PathBuf,
    model: String,
    runs: usize,
    jobs: usize,
    only: Option<String>,
    out: Option<PathBuf>,
    check: bool,
}

fn opts() -> Result<Opts, String> {
    let mut o = Opts {
        cases: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/cases.txt"),
        model: "auto".into(),
        // One run is not enough to judge a change: a case can pass 3/3, then 2/3.
        runs: 3,
        jobs: 4,
        only: None,
        out: None,
        check: false,
    };
    let mut a = std::env::args().skip(1);
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
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    o.jobs = o.jobs.max(1);
    o.runs = o.runs.max(1);
    Ok(o)
}

struct Job {
    case: usize,
    run: usize,
    model: String,
    req: Request,
    started: Instant,
}

fn main() {
    let o = match opts() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("panel_eval: {e}");
            std::process::exit(2);
        }
    };
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

    let call = Call::from_env();
    let mut log = o.out.as_ref().map(|p| {
        std::fs::File::create(p).unwrap_or_else(|e| {
            eprintln!("panel_eval: {}: {e}", p.display());
            std::process::exit(2);
        })
    });
    let mut hashes: Vec<u64> = vec![];
    let mut todo: Vec<(usize, usize)> = (0..o.runs).flat_map(|r| (0..cases.len()).map(move |c| (c, r))).collect();
    todo.reverse();
    let mut live: Vec<Job> = vec![];
    // (case, ms, grade); no grade for a call that failed (a timeout, an API error).
    let mut done: Vec<(usize, u128, Option<Grade>)> = vec![];
    while !todo.is_empty() || !live.is_empty() {
        while live.len() < o.jobs
            && let Some((ci, run)) = todo.pop()
        {
            let c = &cases[ci];
            let cap = Capture {
                name: c.name.clone(),
                cmd: c.cmd.clone(),
                path: c.path.clone(),
                screen: c.screen.clone(),
                repo: c.repo.as_str().into(),
            };
            let flow = Flow::Write {
                draft: c.request.clone(),
                out: c.out,
            };
            let system = prompts::write(&Ctx {
                out: c.out,
                window: window_kind(&c.name, &c.cmd),
            });
            let h = fnv(&system);
            if !hashes.contains(&h) {
                hashes.push(h);
            }
            let model = route(&flow, &o.model).model;
            let prompt = build(&flow, &cap, &[], QUESTIONS);
            // Spawned here, on the main thread: the child's death signal follows it.
            let req = Request::spawn(&call, &system, &prompt, &model);
            live.push(Job {
                case: ci,
                run,
                model,
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
            let (raw, err) = match answer {
                Ok(r) => (r, None),
                Err(f) => (String::new(), Some(f.to_string())),
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
                (_, Some(g)) if !g.missing.is_empty() => format!("wrote, missing: {}", g.missing.join("; ")),
                _ => "wrote".into(),
            };
            let want = c.expect.as_str();
            println!("{mark} {:<24} {:<13} want {want:<6} {:>6} ms  {what}", c.id, c.kind, ms);
            if let Some(f) = log.as_mut() {
                let row = serde_json::json!({
                    "id": c.id, "kind": c.kind, "expect": want, "run": j.run, "model": j.model,
                    "ms": ms as u64, "error": err,
                    "asked": g.as_ref().map(|g| g.asked), "pass": g.as_ref().map(|g| g.pass),
                    "question": g.as_ref().map(|g| g.question.clone()),
                    "missing": g.as_ref().map(|g| g.missing.clone()),
                    "forbidden": g.as_ref().map(|g| g.forbidden.clone()),
                    "refused": g.as_ref().map(|g| g.refused), "holdout": c.holdout, "reply": raw,
                });
                let _ = writeln!(f, "{row}");
            }
            done.push((j.case, ms, g));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    summary(&cases, &done, &hashes);
}

/// The panel's config directory, as `panel::xdg` finds it.
fn config_home() -> PathBuf {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if v.starts_with('/') => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"),
    }
}

/// Where the write prompt came from: the panel's override file, or the binary.
fn prompt_source(base: &Path) -> String {
    let p = base.join("tmux-cockpit/prompts/write.txt");
    if p.exists() { p.display().to_string() } else { "shipped".into() }
}

/// FNV-1a: stable across Rust releases, unlike DefaultHasher, so hashes from
/// two toolchains compare.
fn fnv(s: &str) -> u64 {
    s.bytes()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
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
    false_asks: Of,
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
        false_asks: of(&asked, &is(Expect::Write)),
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

fn pct(Of(n, d): Of) -> String {
    if d == 0 {
        "-".into()
    } else {
        format!("{:.0}% ({n}/{d})", 100.0 * n as f64 / d as f64)
    }
}

fn summary(cases: &[Case], done: &[(usize, u128, Option<Grade>)], hashes: &[u64]) {
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
    if t.holdout.1 > 0 {
        println!("  holdout       {}", pct(t.holdout));
    }
    if t.either_asked.1 > 0 {
        println!("either: asked   {}", pct(t.either_asked));
    }
    println!("must_not hit    {}", pct(t.must_not_hit));
    println!("asked when due  {}", pct(t.asked_when_due));
    println!("  on target     {}", pct(t.on_target));
    println!("false asks      {}", pct(t.false_asks));
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
        let screen_last = "=== s\nkind: bug\nwindow: a · b · c\nexpect: write\n--- request\nr\n--- screen\n$ ls\n# a note\n";
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
        assert_eq!(t.false_asks, Of(1, 3));
        assert_eq!(t.facts, Of(3, 4));
        assert_eq!(t.errors, 1);
        assert_eq!(t.per_case, [(1, 1, 1), (1, 3, 0)]);
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
    }
}
