//! The LLM judge (#419 items 4-5): one `claude -p` call grades one reply PASS
//! or FAIL against a fixed rubric drawn from the owner's labels
//! (`evals/panel/judge-rubric.txt`), reasoning first, then a one-line critique.
//! It sits beside the keyword checks and never replaces them.
//!
//! `--calibrate FILE.jsonl` runs it on owner-labelled replies and prints TPR,
//! TNR and Cohen's kappa against the owner's verdicts.
use super::{claude_command, fnv, unwrap_json, wilson};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const RUBRIC: &str = include_str!("../../evals/panel/judge-rubric.txt");

/// The judge's default model: not sonnet, which writes the replies.
pub const DEFAULT_MODEL: &str = "haiku";

/// Each judge call's cap, `--max-budget-usd`: a haiku call costs about
/// $0.0003 and an opus one about $0.013, so only a runaway call reaches it.
pub const BUDGET_USD: &str = "0.05";

/// The rubric's hash, carried on every judged row so two runs on different
/// rubrics are never read as one.
pub fn rubric_hash() -> String {
    format!("{:016x}", fnv(RUBRIC))
}

/// What the judge sees of one reply: the case's input and the reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    pub window: String,
    pub out: String,
    pub screen: String,
    pub repo: String,
    pub request: String,
    pub reply: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub pass: bool,
    pub critique: String,
}

/// The user prompt of one judge call; the rubric is its system prompt.
pub fn prompt(i: &Input) -> String {
    let or_none = |s: &str| {
        if s.trim().is_empty() {
            "(none)".to_string()
        } else {
            s.trim_end().to_string()
        }
    };
    format!(
        "<window>{}</window>\n<output_type>{}</output_type>\n<screen>\n{}\n</screen>\n<repo>\n{}\n</repo>\n\
         <request>\n{}\n</request>\n<reply>\n{}\n</reply>\n\nGrade the reply.",
        i.window,
        i.out,
        or_none(&i.screen),
        or_none(&i.repo),
        i.request.trim_end(),
        i.reply.trim_end()
    )
}

/// The verdict in a judge's answer: exactly one `VERDICT: PASS|FAIL` line and
/// a non-empty `CRITIQUE:` line, in any case. Two verdict lines are an error,
/// not a pick: the judge changed its mind and either could be the one meant.
pub fn parse(text: &str) -> Result<Verdict, String> {
    let field = |name: &str| -> Vec<String> {
        text.lines()
            .filter_map(|l| {
                let l = l.trim().trim_start_matches(['*', '#', ' ']);
                let (k, v) = l.split_once(':')?;
                (k.trim().eq_ignore_ascii_case(name)).then(|| v.trim().trim_matches('*').trim().to_string())
            })
            .collect()
    };
    let verdicts = field("verdict");
    let pass = match verdicts.as_slice() {
        [v] if v.eq_ignore_ascii_case("pass") => true,
        [v] if v.eq_ignore_ascii_case("fail") => false,
        [v] => return Err(format!("verdict is {v:?}, want PASS or FAIL")),
        [] => return Err("no VERDICT line".into()),
        _ => return Err(format!("{} VERDICT lines, want one", verdicts.len())),
    };
    let critique = field("critique").into_iter().rev().find(|c| !c.is_empty());
    let critique = critique.ok_or("no CRITIQUE line")?;
    Ok(Verdict { pass, critique })
}

/// The argv of one judge call: no tools, no session, no MCP, no slash commands;
/// `claude_command` adds the json output and the hermetic flags.
pub fn argv(model: &str, system: &Path) -> Vec<OsString> {
    let s = [
        "-p",
        "--model",
        model,
        "--tools",
        "",
        "--no-session-persistence",
        "--strict-mcp-config",
    ];
    let mut a: Vec<OsString> = s.iter().map(OsString::from).collect();
    a.push("--disable-slash-commands".into());
    a.extend(["--max-budget-usd".into(), BUDGET_USD.into()]);
    a.push("--system-prompt-file".into());
    a.push(system.into());
    a
}

/// One judged reply: the verdict or why there is none, and what it cost.
#[derive(Clone, Debug)]
pub struct Judged {
    pub verdict: Result<Verdict, String>,
    pub cost_usd: Option<f64>,
}

impl Judged {
    /// The judge's verdict, `None` for a call that gave none, and the critique
    /// or the error.
    pub fn outcome(&self) -> (Option<bool>, String) {
        match &self.verdict {
            Ok(v) => (Some(v.pass), v.critique.clone()),
            Err(e) => (None, e.clone()),
        }
    }
}

/// Whether there were calls and every one of them failed: a run that judged
/// nothing must not exit as one that did.
pub fn every_failed(judged: &[Judged]) -> bool {
    !judged.is_empty() && judged.iter().all(|j| j.verdict.is_err())
}

/// Adds the judge's fields to a rig row: `judge` (null for a failed call),
/// `critique`, `judge_model`, `judge_cost_usd` and `rubric`. It never writes
/// `pass`, which stays the keyword checks' verdict.
pub fn annotate(row: &mut serde_json::Value, j: &Judged, model: &str) {
    let (pass, critique) = j.outcome();
    row["judge"] = pass.into();
    row["critique"] = critique.into();
    row["judge_model"] = model.into();
    row["judge_cost_usd"] = j.cost_usd.into();
    row["rubric"] = rubric_hash().into();
}

/// Grades `inputs` with `model`, `jobs` calls at a time, each hermetic in a
/// fresh folder under `root`; results in input order.
pub fn judge_all(claude: &str, model: &str, inputs: &[Input], jobs: usize, root: &Path) -> Vec<Judged> {
    let system = root.join("judge-rubric.txt");
    if let Err(e) = std::fs::write(&system, RUBRIC) {
        let why = format!("{}: {e}", system.display());
        return inputs
            .iter()
            .map(|_| Judged {
                verdict: Err(why.clone()),
                cost_usd: None,
            })
            .collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out = std::sync::Mutex::new(vec![None; inputs.len()]);
    std::thread::scope(|s| {
        for _ in 0..jobs.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(inp) = inputs.get(i) else { break };
                    let j = judge_one(claude, model, inp, &system, &root.join(format!("j{i}")));
                    out.lock().unwrap()[i] = Some(j);
                }
            });
        }
    });
    out.into_inner()
        .unwrap()
        .into_iter()
        .map(|j| j.expect("every input judged"))
        .collect()
}

fn judge_one(claude: &str, model: &str, i: &Input, system: &Path, cwd: &Path) -> Judged {
    use std::io::Write;
    let fail = |e: String| Judged {
        verdict: Err(e),
        cost_usd: None,
    };
    if let Err(e) = std::fs::create_dir(cwd) {
        return fail(format!("{}: {e}", cwd.display()));
    }
    let mut c = claude_command(claude.as_ref(), argv(model, system), true);
    c.current_dir(cwd).stdin(std::process::Stdio::piped());
    c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let child = c.spawn().and_then(|mut ch| {
        ch.stdin.take().expect("piped").write_all(prompt(i).as_bytes())?;
        ch.wait_with_output()
    });
    let _ = std::fs::remove_dir_all(cwd);
    let o = match child {
        Ok(o) => o,
        Err(e) => return fail(format!("{claude}: {e}")),
    };
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    match unwrap_json(&text) {
        Ok((answer, u)) if o.status.success() => Judged {
            verdict: parse(&answer),
            cost_usd: u.cost_usd,
        },
        Ok((_, u)) => Judged {
            verdict: Err(format!("exit {}: {}", o.status, String::from_utf8_lossy(&o.stderr).trim())),
            cost_usd: u.cost_usd,
        },
        Err(e) => fail(e),
    }
}

/// One owner-labelled reply, from a calibration file.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub key: String,
    /// The eval case the reply answers: the holdout keeps a case on one side.
    pub case: String,
    pub round: String,
    pub pass: bool,
    pub note: String,
    pub input: Input,
}

/// The labelled rows of a calibration file: one json object a line, with
/// `key`, `verdict` (`pass` or `fail`) and `reply`; `case` (the key when
/// absent), `round`, `note`, `window`, `out`, `screen`, `repo` and `request`
/// may be absent.
pub fn read_labels(name: &str, text: &str) -> Result<Vec<Label>, String> {
    let mut v = vec![];
    for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let at = format!("{name}:{}", n + 1);
        let j: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("{at}: {e}"))?;
        let s = |k: &str| j[k].as_str().unwrap_or("").to_string();
        let need = |k: &str| j[k].as_str().map(str::to_string).ok_or(format!("{at}: no string {k}"));
        let pass = match need("verdict")?.as_str() {
            "pass" => true,
            "fail" => false,
            other => return Err(format!("{at}: verdict {other:?}, want pass or fail")),
        };
        let input = Input {
            window: s("window"),
            out: s("out"),
            screen: s("screen"),
            repo: s("repo"),
            request: s("request"),
            reply: need("reply")?,
        };
        let key = need("key")?;
        v.push(Label {
            case: j["case"].as_str().unwrap_or(&key).to_string(),
            key,
            round: s("round"),
            pass,
            note: s("note"),
            input,
        });
    }
    Ok(v)
}

/// Which rows a calibration grades.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    All,
    /// Two thirds of each verdict: the rows the rubric may be tuned on.
    Tune,
    /// The other third, never looked at while tuning: the final numbers.
    Holdout,
}

impl Split {
    pub fn parse(s: &str) -> Result<Split, String> {
        match s {
            "all" => Ok(Split::All),
            "tune" => Ok(Split::Tune),
            "holdout" => Ok(Split::Holdout),
            _ => Err(format!("--split: {s:?}, want all, tune or holdout")),
        }
    }
}

/// Whether each label is in the holdout. Whole cases go to one side, so no
/// reply to a held-out case is tuned on. Cases with an owner fail and cases
/// without are two strata; from each, cases are taken in the order of their
/// name's hash until a third of the stratum's rows, rounded up, is held. A
/// new case shifts the cut by at most one case.
pub fn holdout(labels: &[Label]) -> Vec<bool> {
    let mut cases: Vec<&str> = labels.iter().map(|l| l.case.as_str()).collect();
    cases.sort_unstable();
    cases.dedup();
    let rows = |c: &str| labels.iter().filter(|l| l.case == c).count();
    let has_fail = |c: &str| labels.iter().any(|l| l.case == c && !l.pass);
    let mut held_cases: Vec<&str> = vec![];
    for fails in [false, true] {
        let mut stratum: Vec<&str> = cases.iter().copied().filter(|c| has_fail(c) == fails).collect();
        stratum.sort_by_key(|c| (fnv(c), *c));
        let want = stratum.iter().map(|c| rows(c)).sum::<usize>().div_ceil(3);
        let mut got = 0;
        for c in stratum {
            if got >= want {
                break;
            }
            got += rows(c);
            held_cases.push(c);
        }
    }
    labels.iter().map(|l| held_cases.contains(&l.case.as_str())).collect()
}

/// A calibration's graded rows, as indices into its labels with the judge's
/// verdict, and the calls that gave no verdict: those stay out of every table.
#[derive(Debug, Default, PartialEq)]
pub struct Scored {
    pub graded: Vec<(usize, bool)>,
    pub errors: usize,
}

pub fn score(judged: &[Judged]) -> Scored {
    let mut s = Scored::default();
    for (i, j) in judged.iter().enumerate() {
        match j.outcome().0 {
            Some(p) => s.graded.push((i, p)),
            None => s.errors += 1,
        }
    }
    s
}

/// Owner against judge, PASS as the positive class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Confusion {
    /// Both pass.
    pub tp: usize,
    /// Owner pass, judge fail.
    pub fn_: usize,
    /// Owner fail, judge pass.
    pub fp: usize,
    /// Both fail.
    pub tn: usize,
}

impl Confusion {
    pub fn of(pairs: impl IntoIterator<Item = (bool, bool)>) -> Confusion {
        let mut c = Confusion::default();
        for (owner, judge) in pairs {
            match (owner, judge) {
                (true, true) => c.tp += 1,
                (true, false) => c.fn_ += 1,
                (false, true) => c.fp += 1,
                (false, false) => c.tn += 1,
            }
        }
        c
    }

    pub fn n(&self) -> usize {
        self.tp + self.fn_ + self.fp + self.tn
    }

    /// Judge pass when the owner passed: (k, n).
    pub fn tpr(&self) -> (usize, usize) {
        (self.tp, self.tp + self.fn_)
    }

    /// Judge fail when the owner failed: (k, n).
    pub fn tnr(&self) -> (usize, usize) {
        (self.tn, self.tn + self.fp)
    }

    /// Cohen's kappa and its 95% interval from the large-sample standard error
    /// sqrt(po(1-po) / (n(1-pe)^2)) (Cohen 1960); `None` with no rows or when
    /// chance agreement is total (both raters gave one verdict to every row).
    pub fn kappa(&self) -> Option<(f64, f64, f64)> {
        let n = self.n() as f64;
        if n == 0.0 {
            return None;
        }
        let po = (self.tp + self.tn) as f64 / n;
        let owner_pass = (self.tp + self.fn_) as f64 / n;
        let judge_pass = (self.tp + self.fp) as f64 / n;
        let pe = owner_pass * judge_pass + (1.0 - owner_pass) * (1.0 - judge_pass);
        if pe >= 1.0 {
            return None;
        }
        let k = (po - pe) / (1.0 - pe);
        let se = (po * (1.0 - po) / (n * (1.0 - pe).powi(2))).sqrt();
        let z = 1.959_964;
        Some((k, (k - z * se).max(-1.0), (k + z * se).min(1.0)))
    }
}

/// `k/n` as a percent with its Wilson 95% interval.
pub fn rate((k, n): (usize, usize)) -> String {
    match wilson(k, n) {
        Some((lo, hi)) => format!(
            "{:.0}% ({k}/{n})  95% CI {:.0}%-{:.0}%",
            100.0 * k as f64 / n as f64,
            100.0 * lo,
            100.0 * hi
        ),
        None => "- (0/0)".into(),
    }
}

/// The metric lines of one table: TPR, TNR and kappa.
pub fn metrics(c: &Confusion) -> String {
    let kappa = match c.kappa() {
        Some((k, lo, hi)) => format!("{k:.2}  95% CI {lo:.2}-{hi:.2}"),
        None => "-".into(),
    };
    format!(
        "  n {}  (owner pass {}, fail {})\n  TPR    {}\n  TNR    {}\n  kappa  {kappa}",
        c.n(),
        c.tp + c.fn_,
        c.fp + c.tn,
        rate(c.tpr()),
        rate(c.tnr())
    )
}

/// Runs the judge on a labelled file and prints the metrics, overall, per
/// round and per split, then every disagreement with both reasons. Writes one
/// row per label to `out` when given.
pub fn calibrate(file: &Path, model: &str, split: Split, jobs: usize, out: Option<&Path>) -> Result<(), String> {
    let name = file.display().to_string();
    let text = std::fs::read_to_string(file).map_err(|e| format!("{name}: {e}"))?;
    let all = read_labels(&name, &text)?;
    let held = holdout(&all);
    let pick: Vec<(Label, bool)> = all
        .into_iter()
        .zip(held)
        .filter(|(_, h)| match split {
            Split::All => true,
            Split::Tune => !h,
            Split::Holdout => *h,
        })
        .collect();
    let claude = std::env::var("COCKPIT_CLAUDE")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or("claude".into());
    let root = Scratch::new()?;
    println!("judge {model} on {} rows of {name} ({split:?})", pick.len());
    let inputs: Vec<Input> = pick.iter().map(|(l, _)| l.input.clone()).collect();
    let judged = judge_all(&claude, model, &inputs, jobs, &root.0);
    let mut log = match out {
        Some(p) => Some(std::fs::File::create(p).map_err(|e| format!("{}: {e}", p.display()))?),
        None => None,
    };
    let mut cost = 0.0;
    for ((l, h), j) in pick.iter().zip(&judged) {
        cost += j.cost_usd.unwrap_or(0.0);
        let (judge, critique) = j.outcome();
        if judge.is_none() {
            println!("ERR  {:<36} {critique}", l.key);
        }
        if let Some(f) = log.as_mut() {
            use std::io::Write;
            let row = serde_json::json!({
                "key": l.key, "case": l.case, "round": l.round, "holdout": h, "owner": l.pass,
                "judge": judge, "critique": critique, "note": l.note, "model": model,
                "cost_usd": j.cost_usd, "rubric": rubric_hash(),
            });
            let _ = writeln!(f, "{row}");
        }
    }
    let Scored { graded, errors } = score(&judged);
    let graded: Vec<(&Label, bool, bool)> = graded.iter().map(|&(i, p)| (&pick[i].0, pick[i].1, p)).collect();
    let table =
        |keep: &dyn Fn(&Label, bool) -> bool| Confusion::of(graded.iter().filter(|(l, h, _)| keep(l, *h)).map(|(l, _, j)| (l.pass, *j)));
    println!("\nall graded\n{}", metrics(&table(&|_, _| true)));
    let mut rounds: Vec<&str> = graded.iter().map(|(l, _, _)| l.round.as_str()).collect();
    rounds.sort_unstable();
    rounds.dedup();
    for r in rounds {
        println!("round {r}\n{}", metrics(&table(&|l, _| l.round == r)));
    }
    if split == Split::All {
        println!("tune split\n{}", metrics(&table(&|_, h| !h)));
        println!("holdout split\n{}", metrics(&table(&|_, h| h)));
    }
    println!("\ndisagreements");
    for ((l, h), j) in pick.iter().zip(&judged) {
        if let Ok(v) = &j.verdict
            && v.pass != l.pass
        {
            let side = if *h { "holdout" } else { "tune" };
            let owner = if l.pass { "pass" } else { "fail" };
            let judge = if v.pass { "PASS" } else { "FAIL" };
            println!("  {} [{} {side}] owner {owner}, judge {judge}", l.key, l.round);
            println!("    owner: {}", if l.note.trim().is_empty() { "(no note)" } else { l.note.trim() });
            println!("    judge: {}", v.critique);
        }
    }
    println!("\ncost  ${cost:.4} over {} calls, {errors} failed", judged.len());
    if every_failed(&judged) {
        return Err(format!("every one of the {} judge calls failed", judged.len()));
    }
    Ok(())
}

/// A temp dir of the judge's own, removed on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Result<Scratch, String> {
        let d = std::env::temp_dir().join(format!("panel-eval-judge-{}", std::process::id()));
        std::fs::create_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        Ok(Scratch(d))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_a_pass_and_its_critique() {
        let v = parse("REASONING: it asks the kind first.\nCRITIQUE: asks the config kind.\nVERDICT: PASS").unwrap();
        assert_eq!(
            v,
            Verdict {
                pass: true,
                critique: "asks the config kind.".into()
            }
        );
    }

    #[test]
    fn parse_reads_a_fail_in_any_case_and_markdown() {
        let v = parse("Reasoning: guessed.\n**Critique:** has an Assumed: line\n**verdict:** fail").unwrap();
        assert!(!v.pass);
        assert_eq!(v.critique, "has an Assumed: line");
    }

    #[test]
    fn parse_rejects_a_missing_or_unknown_or_doubled_verdict() {
        let rows = [
            ("none", "CRITIQUE: x", "no VERDICT line"),
            ("unknown", "CRITIQUE: x\nVERDICT: MAYBE", "want PASS or FAIL"),
            ("doubled", "CRITIQUE: x\nVERDICT: PASS\nVERDICT: FAIL", "2 VERDICT lines"),
            ("empty", "", "no VERDICT line"),
        ];
        for (name, text, want) in rows {
            let e = parse(text).unwrap_err();
            assert!(e.contains(want), "{name}: {e}");
        }
    }

    #[test]
    fn parse_rejects_a_verdict_with_no_critique() {
        assert_eq!(parse("VERDICT: PASS").unwrap_err(), "no CRITIQUE line");
        assert_eq!(parse("CRITIQUE:   \nVERDICT: PASS").unwrap_err(), "no CRITIQUE line");
    }

    #[test]
    fn confusion_counts_pass_as_positive() {
        let c = Confusion::of([(true, true), (true, false), (false, true), (false, false), (false, false)]);
        assert_eq!(
            c,
            Confusion {
                tp: 1,
                fn_: 1,
                fp: 1,
                tn: 2
            }
        );
    }

    #[test]
    fn rates_and_kappa_on_a_fixed_table() {
        // po 0.85, pe 0.5*0.45 + 0.5*0.55 = 0.5, kappa 0.7
        let c = Confusion {
            tp: 40,
            fn_: 10,
            fp: 5,
            tn: 45,
        };
        assert_eq!(c.tpr(), (40, 50));
        assert_eq!(c.tnr(), (45, 50));
        let (k, lo, hi) = c.kappa().unwrap();
        assert!((k - 0.7).abs() < 1e-9, "{k}");
        // se = sqrt(0.85*0.15 / (100*0.25)) = 0.071414
        assert!((lo - (0.7 - 1.959_964 * 0.071_414)).abs() < 1e-5, "{lo}");
        assert!((hi - (0.7 + 1.959_964 * 0.071_414)).abs() < 1e-5, "{hi}");
    }

    #[test]
    fn kappa_is_none_with_no_rows_or_one_verdict_everywhere() {
        assert_eq!(Confusion::default().kappa(), None);
        assert_eq!(
            Confusion {
                tp: 7,
                ..Default::default()
            }
            .kappa(),
            None
        );
    }

    #[test]
    fn kappa_is_negative_when_the_judge_flips_every_row() {
        let (k, _, _) = Confusion {
            fn_: 5,
            fp: 5,
            ..Default::default()
        }
        .kappa()
        .unwrap();
        assert!((k + 1.0).abs() < 1e-9, "{k}");
    }

    fn label(key: &str, pass: bool) -> Label {
        let input = Input {
            reply: "r".into(),
            ..Default::default()
        };
        Label {
            key: key.into(),
            case: key.into(),
            round: "r1".into(),
            pass,
            note: String::new(),
            input,
        }
    }

    #[test]
    fn holdout_takes_a_third_of_each_verdict() {
        let mut ls: Vec<Label> = (0..98).map(|i| label(&format!("p{i}"), true)).collect();
        ls.extend((0..20).map(|i| label(&format!("f{i}"), false)));
        let h = holdout(&ls);
        let held = |pass: bool| ls.iter().zip(&h).filter(|(l, h)| l.pass == pass && **h).count();
        assert_eq!((held(true), held(false)), (33, 7));
    }

    #[test]
    fn holdout_keeps_a_row_where_it_was_when_another_is_added() {
        let ls: Vec<Label> = (0..30).map(|i| label(&format!("k{i}"), i % 4 == 0)).collect();
        let before = holdout(&ls);
        let mut more = ls.clone();
        more.push(label("new", true));
        let after = holdout(&more);
        let moved = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        assert!(moved <= 1, "{moved} rows moved");
    }

    #[test]
    fn read_labels_takes_the_owner_verdict_and_the_reply() {
        let t = r#"{"key":"a.r0","round":"r2","verdict":"fail","note":"n","request":"q","reply":"<text>x</text>"}"#;
        let l = &read_labels("f", t).unwrap()[0];
        assert_eq!((l.key.as_str(), l.round.as_str(), l.pass), ("a.r0", "r2", false));
        assert_eq!((l.input.request.as_str(), l.input.reply.as_str()), ("q", "<text>x</text>"));
    }

    #[test]
    fn read_labels_names_the_line_of_a_bad_row() {
        let rows = [
            ("verdict", r#"{"key":"a","verdict":"ok","reply":"r"}"#, "f:2: verdict \"ok\""),
            ("no reply", r#"{"key":"a","verdict":"pass"}"#, "f:2: no string reply"),
            ("no key", r#"{"verdict":"pass","reply":"r"}"#, "f:2: no string key"),
            ("not json", "{", "f:2: "),
        ];
        for (name, row, want) in rows {
            let t = format!("{}\n{row}", r#"{"key":"z","verdict":"pass","reply":"r"}"#);
            let e = read_labels("f", &t).unwrap_err();
            assert!(e.starts_with(want), "{name}: {e}");
        }
    }

    #[test]
    fn prompt_carries_the_case_and_marks_an_empty_screen() {
        let i = Input {
            request: "deploy it".into(),
            reply: "<question>q</question>".into(),
            ..Default::default()
        };
        let p = prompt(&i);
        assert!(p.contains("<request>\ndeploy it\n</request>"), "{p}");
        assert!(p.contains("<screen>\n(none)\n</screen>"), "{p}");
        assert!(p.contains("<reply>\n<question>q</question>\n</reply>"), "{p}");
    }

    #[test]
    fn argv_has_no_tools_and_the_rubric_as_system_prompt() {
        let a = argv("haiku", Path::new("/r/rubric.txt"));
        let at = |k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
        assert_eq!(at("--model"), Some("haiku".into()));
        assert_eq!(at("--tools"), Some("".into()));
        assert_eq!(at("--system-prompt-file"), Some("/r/rubric.txt".into()));
        assert_eq!(a[0], "-p");
        assert_eq!(at("--max-budget-usd"), Some(BUDGET_USD.into()));
    }

    fn judged(v: Result<bool, &str>) -> Judged {
        let verdict = v
            .map(|pass| Verdict {
                pass,
                critique: "c".into(),
            })
            .map_err(String::from);
        Judged {
            verdict,
            cost_usd: Some(0.001),
        }
    }

    #[test]
    fn annotate_never_writes_pass() {
        let mut row = serde_json::json!({"id": "a", "pass": false});
        annotate(&mut row, &judged(Ok(true)), "haiku");
        assert_eq!((row["pass"].as_bool(), row["judge"].as_bool()), (Some(false), Some(true)));
        assert_eq!(row["rubric"].as_str(), Some(rubric_hash().as_str()));
        assert_eq!(row["judge_model"].as_str(), Some("haiku"));
    }

    #[test]
    fn annotate_leaves_judge_null_for_a_failed_call() {
        let mut row = serde_json::json!({"id": "a", "pass": true});
        annotate(&mut row, &judged(Err("exit 1")), "haiku");
        assert!(row["judge"].is_null());
        assert_eq!(row["critique"].as_str(), Some("exit 1"));
        assert_eq!(row["pass"].as_bool(), Some(true));
    }

    #[test]
    fn score_keeps_a_failed_call_out_of_the_table_and_counts_it() {
        let s = score(&[judged(Ok(true)), judged(Err("no VERDICT line")), judged(Ok(false))]);
        assert_eq!(
            s,
            Scored {
                graded: vec![(0, true), (2, false)],
                errors: 1
            }
        );
    }

    #[test]
    fn every_failed_only_when_there_were_calls_and_none_gave_a_verdict() {
        assert!(!every_failed(&[]));
        assert!(every_failed(&[judged(Err("x")), judged(Err("y"))]));
        assert!(!every_failed(&[judged(Err("x")), judged(Ok(false))]));
    }

    #[test]
    fn holdout_keeps_each_case_on_one_side() {
        let mut ls = vec![];
        for c in 0..30 {
            for r in 0..3 {
                let mut l = label(&format!("c{c}.r{r}"), !(c % 5 == 0 && r == 0));
                l.case = format!("c{c}");
                ls.push(l);
            }
        }
        let h = holdout(&ls);
        for c in ls.chunks(3).zip(h.chunks(3)) {
            assert!(c.1.iter().all(|x| *x == c.1[0]), "case {} split", c.0[0].case);
        }
        let held_fails = ls.iter().zip(&h).filter(|(l, h)| !l.pass && **h).count();
        assert_eq!(held_fails, 2, "a third of the 6 fail cases");
    }

    #[test]
    fn read_labels_takes_the_case_or_falls_back_to_the_key() {
        let t = "{\"key\":\"a.r0\",\"case\":\"a\",\"verdict\":\"pass\",\"reply\":\"r\"}\n\
                 {\"key\":\"b.r0\",\"verdict\":\"pass\",\"reply\":\"r\"}";
        let l = read_labels("f", t).unwrap();
        assert_eq!((l[0].case.as_str(), l[1].case.as_str()), ("a", "b.r0"));
    }
}
