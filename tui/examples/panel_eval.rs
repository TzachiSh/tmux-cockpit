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
                        _ => return Err(err("expect is ask or write")),
                    })
                }
                "ask_any" => c.ask_any = list(v),
                "must" => c.must = list(v),
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
    g.pass = match c.expect {
        Expect::Ask => g.asked && g.on_target,
        Expect::Write => !g.asked && g.missing.is_empty(),
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
        runs: 1,
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
    let asks = cases.iter().filter(|c| c.expect == Expect::Ask).count();
    println!(
        "{} cases ({asks} ask, {} write) from {}",
        cases.len(),
        cases.len() - asks,
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
                (_, Some(g)) if g.asked => format!("asked: {}", g.question),
                (_, Some(g)) if !g.missing.is_empty() => format!("wrote, missing: {}", g.missing.join("; ")),
                _ => "wrote".into(),
            };
            let want = if c.expect == Expect::Ask { "ask" } else { "write" };
            println!("{mark} {:<24} {:<13} want {want:<5} {:>6} ms  {what}", c.id, c.kind, ms);
            if let Some(f) = log.as_mut() {
                let row = serde_json::json!({
                    "id": c.id, "kind": c.kind, "expect": want, "run": j.run, "model": j.model,
                    "ms": ms as u64, "error": err,
                    "asked": g.as_ref().map(|g| g.asked), "pass": g.as_ref().map(|g| g.pass),
                    "question": g.as_ref().map(|g| g.question.clone()),
                    "missing": g.as_ref().map(|g| g.missing.clone()), "reply": raw,
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
    println!("system prompt   {}", hs.join(" "));
    println!("passed          {}", pct(pass, done.len()));
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
    fn the_shipped_cases_are_valid() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals/panel/cases.txt");
        let cases = parse_cases(&std::fs::read_to_string(p).unwrap()).unwrap();
        assert!(cases.len() >= 25, "{}", cases.len());
        assert!(cases.iter().any(|c| c.expect == Expect::Ask));
        assert!(cases.iter().any(|c| c.expect == Expect::Write));
    }
}
