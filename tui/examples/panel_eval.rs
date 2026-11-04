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
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The panel's write budget (`ROUNDS_WRITE`): the first call sees all of it.
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

/// `=== id`, then `key: value` lines, then `--- screen` and `--- request`
/// bodies. `#` lines outside a body are comments.
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
            request: String::new(),
        };
        let mut expect = None;
        for line in head.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (k, v) = line.split_once(':').ok_or_else(|| err(&format!("not key: value: {line}")))?;
            let v = v.trim();
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
        for body in bodies.split("\n--- ").filter(|b| !b.is_empty()) {
            let (name, text) = body.split_once('\n').unwrap_or((body, ""));
            let text = text.trim_end_matches('\n').to_string();
            match name.trim() {
                "screen" => c.screen = text,
                "request" => c.request = text.trim().into(),
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

fn has(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(&needle.to_lowercase())
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
    pass: bool,
}

fn grade(c: &Case, raw: &str) -> Grade {
    let reply = parse::write(raw, c.out).reply;
    let mut g = Grade::default();
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
        g.missing = c.must.iter().filter(|m| !has(raw, m)).cloned().collect();
    }
    g.forbidden = c.must_not.iter().filter(|m| has(raw, m)).cloned().collect();
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
    // (case, ms, grade) for answered calls; failures are counted apart.
    let mut done: Vec<(usize, u128, Grade)> = vec![];
    let mut failed = 0usize;
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
            };
            let flow = Flow::Write {
                draft: c.request.clone(),
                out: c.out,
            };
            let system = prompts::write(&Ctx {
                out: c.out,
                window: window_kind(&c.name, &c.cmd),
            });
            let mut h = DefaultHasher::new();
            system.hash(&mut h);
            let h = h.finish();
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
                    "forbidden": g.as_ref().map(|g| g.forbidden.clone()), "holdout": c.holdout, "reply": raw,
                });
                let _ = writeln!(f, "{row}");
            }
            match g {
                Some(g) => done.push((j.case, ms, g)),
                None => failed += 1,
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    summary(&cases, &done, failed, &hashes);
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "-".into()
    } else {
        format!("{:.0}% ({n}/{d})", 100.0 * n as f64 / d as f64)
    }
}

fn summary(cases: &[Case], done: &[(usize, u128, Grade)], failed: usize, hashes: &[u64]) {
    let of = |e: Expect| done.iter().filter(move |(c, _, _)| cases[*c].expect == e);
    let ask: Vec<_> = of(Expect::Ask).collect();
    let write: Vec<_> = of(Expect::Write).collect();
    let asked = ask.iter().filter(|(_, _, g)| g.asked).count();
    let target = ask.iter().filter(|(_, _, g)| g.asked && g.on_target).count();
    let false_ask = write.iter().filter(|(_, _, g)| g.asked).count();
    let wrote: Vec<_> = write.iter().filter(|(_, _, g)| !g.asked).collect();
    let facts: usize = wrote.iter().map(|(c, _, _)| cases[*c].must.len()).sum();
    let lacking: usize = wrote.iter().map(|(_, _, g)| g.missing.len()).sum();
    let mut ms: Vec<u128> = done.iter().map(|(_, m, _)| *m).collect();
    ms.sort();
    let pass = done.iter().filter(|(_, _, g)| g.pass).count();
    let hs: Vec<String> = hashes.iter().map(|h| format!("{h:016x}")).collect();
    println!();
    // Per case, over the runs: a flaky case shows as 2/3, not as noise in a total.
    println!("{:<24} {:<6} pass", "case", "want");
    for (i, c) in cases.iter().enumerate() {
        let mine: Vec<_> = done.iter().filter(|(ci, _, _)| *ci == i).collect();
        let p = mine.iter().filter(|(_, _, g)| g.pass).count();
        let tag = if c.holdout { "  holdout" } else { "" };
        println!("{:<24} {:<6} {p}/{}{tag}", c.id, c.expect.as_str(), mine.len());
    }
    println!();
    println!("system prompt   {}", hs.join(" "));
    println!("passed          {}", pct(pass, done.len()));
    let held: Vec<_> = done.iter().filter(|(c, _, _)| cases[*c].holdout).collect();
    if !held.is_empty() {
        let hp = held.iter().filter(|(_, _, g)| g.pass).count();
        println!("  holdout       {}", pct(hp, held.len()));
    }
    let either: Vec<_> = of(Expect::Either).collect();
    if !either.is_empty() {
        let ea = either.iter().filter(|(_, _, g)| g.asked).count();
        println!("either: asked   {}", pct(ea, either.len()));
    }
    let nots: usize = done.iter().filter(|(_, _, g)| !g.forbidden.is_empty()).count();
    let with_not = done.iter().filter(|(c, _, _)| !cases[*c].must_not.is_empty()).count();
    println!("must_not hit    {}", pct(nots, with_not));
    println!("asked when due  {}", pct(asked, ask.len()));
    println!("  on target     {}", pct(target, ask.len()));
    println!("false asks      {}", pct(false_ask, write.len()));
    println!("facts present   {}", pct(facts - lacking, facts));
    if !ms.is_empty() {
        let mean = ms.iter().sum::<u128>() / ms.len() as u128;
        println!("latency ms      mean {mean}  p50 {}  max {}", ms[ms.len() / 2], ms[ms.len() - 1]);
    }
    if failed > 0 {
        println!("failed calls    {failed}");
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
    fn the_shipped_cases_are_valid() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/cases.txt");
        let cases = parse_cases(&std::fs::read_to_string(p).unwrap()).unwrap();
        assert!(cases.len() >= 25, "{}", cases.len());
        assert!(cases.iter().any(|c| c.expect == Expect::Ask));
        assert!(cases.iter().any(|c| c.expect == Expect::Write));
    }
}
