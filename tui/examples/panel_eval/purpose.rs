//! The `@purpose` judge fallback (#702 H2): when [`super::asks_purpose`] misses
//! on an asked question, a judge model answers yes or no to [`QUESTION`].
//! Verdicts are cached by (question hash, model, rubric hash), so a regrade
//! reads them back for free.

use super::{claude_command, fnv, unwrap_json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The judge's one question, its whole rubric.
pub const QUESTION: &str = "Does the question below ask what the feature or work is for: its purpose, \
goal, the problem it solves, or what counts as success? Answer with one word: yes or no.";

/// The hash of [`QUESTION`], part of each cache key.
pub fn rubric_hash() -> String {
    format!("{:016x}", fnv(QUESTION))
}

/// A cache line's key: the question's hash, the model and the rubric hash.
fn key(question: &str, model: &str) -> String {
    format!("{:016x}\t{model}\t{}", fnv(question), rubric_hash())
}

/// Judge verdicts on disk: one `qhash TAB model TAB rubric TAB yes|no` line each.
pub struct Cache {
    path: PathBuf,
    map: HashMap<String, bool>,
}

impl Cache {
    /// The cache at `path`; a missing file is an empty cache.
    pub fn open(path: &Path) -> Cache {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let map = text
            .lines()
            .filter_map(|l| l.rsplit_once('\t'))
            .filter_map(|(k, v)| parse_answer(v).ok().map(|v| (k.to_string(), v)))
            .collect();
        Cache { path: path.into(), map }
    }

    pub fn get(&self, question: &str, model: &str) -> Option<bool> {
        self.map.get(&key(question, model)).copied()
    }

    /// Records a verdict, in memory and appended to the file.
    pub fn put(&mut self, question: &str, model: &str, yes: bool) -> std::io::Result<()> {
        let k = key(question, model);
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(f, "{k}\t{}", if yes { "yes" } else { "no" })?;
        self.map.insert(k, yes);
        Ok(())
    }
}

/// The judge's answer: its first word, yes or no, in any case.
pub fn parse_answer(text: &str) -> Result<bool, String> {
    let w: String = text.trim().chars().take_while(|c| c.is_alphabetic()).collect();
    match w.to_lowercase().as_str() {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err(format!("want yes or no, got {:?}", text.trim())),
    }
}

/// The verdict for each question, from the cache or one hermetic call each,
/// `jobs` at a time; new verdicts go to the cache. Returns the verdicts (None
/// for a failed call) and the dollars spent.
pub fn judge_all(claude: &str, model: &str, questions: &[String], jobs: usize, cache: &mut Cache) -> (Vec<Option<bool>>, f64) {
    let todo: Vec<usize> = (0..questions.len())
        .filter(|&i| cache.get(&questions[i], model).is_none())
        .collect();
    let root = match super::judge::Scratch::new() {
        Ok(r) => r,
        Err(_) => return (questions.iter().map(|q| cache.get(q, model)).collect(), 0.0),
    };
    let system = root.0.join("purpose-rubric.txt");
    let fresh: Vec<(Result<bool, String>, Option<f64>)> = if std::fs::write(&system, QUESTION).is_err() {
        todo.iter().map(|_| (Err("no rubric file".into()), None)).collect()
    } else {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let out = std::sync::Mutex::new(vec![None; todo.len()]);
        std::thread::scope(|s| {
            for _ in 0..jobs.max(1) {
                s.spawn(|| {
                    loop {
                        let n = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&i) = todo.get(n) else { break };
                        let r = ask(claude, model, &questions[i], &system, &root.0.join(format!("p{i}")));
                        out.lock().unwrap()[n] = Some(r);
                    }
                });
            }
        });
        out.into_inner()
            .unwrap()
            .into_iter()
            .map(|r| r.expect("every question asked"))
            .collect()
    };
    let mut cost = 0.0;
    for (&i, (v, c)) in todo.iter().zip(&fresh) {
        cost += c.unwrap_or(0.0);
        if let Ok(yes) = v {
            let _ = cache.put(&questions[i], model, *yes);
        }
    }
    (questions.iter().map(|q| cache.get(q, model)).collect(), cost)
}

fn ask(claude: &str, model: &str, question: &str, system: &Path, cwd: &Path) -> (Result<bool, String>, Option<f64>) {
    if let Err(e) = std::fs::create_dir(cwd) {
        return (Err(format!("{}: {e}", cwd.display())), None);
    }
    let mut c = claude_command(claude.as_ref(), super::judge::argv(model, system), true);
    c.current_dir(cwd).stdin(std::process::Stdio::piped());
    c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let child = c.spawn().and_then(|mut ch| {
        ch.stdin.take().expect("piped").write_all(question.as_bytes())?;
        ch.wait_with_output()
    });
    let _ = std::fs::remove_dir_all(cwd);
    let o = match child {
        Ok(o) => o,
        Err(e) => return (Err(format!("{claude}: {e}")), None),
    };
    match unwrap_json(&String::from_utf8_lossy(&o.stdout)) {
        Ok((answer, u)) if o.status.success() => (parse_answer(&answer), u.cost_usd),
        Ok((_, u)) => (Err(format!("exit {}", o.status)), u.cost_usd),
        Err(e) => (Err(e), None),
    }
}

/// Agreement of the judge with the fixture's labels, over every row.
#[derive(Debug, Default, PartialEq)]
pub struct Calibration {
    pub agree: usize,
    pub judged: usize,
    /// 0-rows the judge said yes to, of all 0-rows.
    pub false_accepts: (usize, usize),
    /// 1-rows the judge said no to, of all 1-rows.
    pub misses: (usize, usize),
    pub failed: usize,
}

/// Tallies verdicts against `question TAB 1|0` rows.
pub fn tally(rows: &[(String, bool)], verdicts: &[Option<bool>]) -> Calibration {
    let mut c = Calibration::default();
    for ((_, want), got) in rows.iter().zip(verdicts) {
        if *want {
            c.misses.1 += 1
        } else {
            c.false_accepts.1 += 1
        }
        let Some(got) = got else {
            c.failed += 1;
            continue;
        };
        c.judged += 1;
        c.agree += usize::from(got == want);
        match (want, got) {
            (false, true) => c.false_accepts.0 += 1,
            (true, false) => c.misses.0 += 1,
            _ => {}
        }
    }
    c
}

/// The fixture's rows, `#` lines skipped.
pub fn read_fixture(text: &str) -> Result<Vec<(String, bool)>, String> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| match l.split_once('\t') {
            Some((q, "1")) => Ok((q.to_string(), true)),
            Some((q, "0")) => Ok((q.to_string(), false)),
            _ => Err(format!("want question TAB 1|0: {l}")),
        })
        .collect()
}

/// Whether the fallback may be used: at most 1 false accept and at least 95%
/// agreement over the judged rows (#702 H2).
pub fn usable(c: &Calibration) -> bool {
    c.false_accepts.0 <= 1 && c.judged > 0 && c.agree * 100 >= c.judged * 95 && c.failed == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_its_first_word_yes_or_no() {
        assert_eq!(parse_answer("Yes."), Ok(true));
        assert_eq!(parse_answer(" no\n"), Ok(false));
        assert!(parse_answer("Maybe").is_err());
    }

    #[test]
    fn the_cache_reads_back_what_it_wrote_under_model_and_rubric() {
        let d = std::env::temp_dir().join(format!("panel-eval-purpose-{}", std::process::id()));
        let p = d.join("cache.tsv");
        let mut c = Cache::open(&p);
        c.put("What is it for?", "haiku", true).unwrap();
        let c = Cache::open(&p);
        let got = (c.get("What is it for?", "haiku"), c.get("What is it for?", "sonnet"));
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(got, (Some(true), None));
    }

    #[test]
    fn calibration_counts_false_accepts_misses_and_the_gate() {
        let rows = vec![("a".to_string(), true), ("b".to_string(), false), ("c".to_string(), true)];
        let c = tally(&rows, &[Some(true), Some(true), Some(false)]);
        assert_eq!((c.agree, c.judged, c.false_accepts, c.misses), (1, 3, (1, 1), (1, 2)));
        assert!(!usable(&c));
    }
}
