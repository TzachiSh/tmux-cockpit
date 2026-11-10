//! The overlap check (#710): does an edited prompt line paraphrase a held-out
//! case's request? N-gram checks miss a paraphrase, so a judge model answers
//! yes or no to [`QUESTION`] for each changed line and each `holdout: yes`
//! case. The rig prints a flagged pair's case id and prompt line, never the
//! case's text, so whoever edits the prompt stays blind to the held-out cases.
//! Verdicts are cached by (line hash, request hash, model, question hash).

use super::{claude_command, fnv, unwrap_json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The judge's system prompt, its whole rubric. Written in general words, from
/// no held-out case.
pub const QUESTION: &str = "You check whether an edit to a system prompt leaks a case from a blind test set. \
You get one line of the prompt, in <prompt_line>, and one user request from the blind set, in <request>. \
Answer yes if the line paraphrases the request's specific scenario: it restates the same concrete \
situation, or the same particular constraint, choice or answer the request turns on, so that a model which \
read the line would be primed for that request. An example in the line that mirrors the request counts, \
even with other names or other wording. Answer no if the line shares only the request's topic, subject \
area or kind of task. Answer with one word: yes or no.";

/// The hash of [`QUESTION`], part of each cache key.
pub fn question_hash() -> String {
    format!("{:016x}", fnv(QUESTION))
}

/// What the judge reads on stdin for one pair. A `</` in either field is
/// escaped, so neither can close its section.
pub fn input(line: &str, request: &str) -> String {
    let esc = |s: &str| s.replace("</", "<\\/");
    format!(
        "<prompt_line>\n{}\n</prompt_line>\n<request>\n{}\n</request>\n",
        esc(line),
        esc(request)
    )
}

/// Pushes `l` trimmed, unless it is blank or already in `v`.
fn push_once(v: &mut Vec<String>, l: &str) {
    let l = l.trim();
    if !l.is_empty() && !v.iter().any(|x| x == l) {
        v.push(l.into());
    }
}

/// The lines a unified diff adds, trimmed, blanks dropped, each once. Only a
/// `+` line inside a hunk counts: a file's `+++` header comes before its first
/// `@@`.
pub fn added_lines(diff: &str) -> Vec<String> {
    let mut in_hunk = false;
    let mut v = vec![];
    for l in diff.lines() {
        if l.starts_with("diff ") {
            in_hunk = false
        } else if l.starts_with("@@") {
            in_hunk = true
        } else if let Some(added) = l.strip_prefix('+').filter(|_| in_hunk) {
            push_once(&mut v, added)
        }
    }
    v
}

/// The lines of `to` that `from` lacks, trimmed, blanks dropped, each once.
pub fn new_lines(from: &str, to: &str) -> Vec<String> {
    let old: Vec<&str> = from.lines().map(str::trim).collect();
    let mut v = vec![];
    for l in to.lines().filter(|l| !old.contains(&l.trim())) {
        push_once(&mut v, l)
    }
    v
}

/// A cache line's key: hashes of the line, the request and the question, and
/// the model. Never the text of a line or a case.
fn key(line: &str, request: &str, model: &str) -> String {
    format!("{:016x}\t{:016x}\t{model}\t{}", fnv(line), fnv(request), question_hash())
}

/// Judge verdicts on disk: one `line hash TAB request hash TAB model TAB
/// question hash TAB yes|no` line each.
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
            .filter_map(|(k, v)| match v {
                "yes" => Some((k.to_string(), true)),
                "no" => Some((k.to_string(), false)),
                _ => None,
            })
            .collect();
        Cache { path: path.into(), map }
    }

    pub fn get(&self, line: &str, request: &str, model: &str) -> Option<bool> {
        self.map.get(&key(line, request, model)).copied()
    }

    /// Records a verdict, in memory and appended to the file.
    pub fn put(&mut self, line: &str, request: &str, model: &str, yes: bool) -> std::io::Result<()> {
        let k = key(line, request, model);
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(f, "{k}\t{}", if yes { "yes" } else { "no" })?;
        self.map.insert(k, yes);
        Ok(())
    }
}

/// What a run of the judge gave: a verdict per pair (None for a failed call),
/// how many came from the cache, why each failed call failed, and the dollars.
#[derive(Debug, Default)]
pub struct Outcome {
    pub verdicts: Vec<Option<bool>>,
    pub cached: usize,
    pub failed: Vec<String>,
    pub usd: f64,
}

/// The verdict for each (line, request) pair, from the cache or one hermetic
/// call each, `jobs` at a time; new verdicts go to the cache.
pub fn judge_all(claude: &str, model: &str, pairs: &[(&str, &str)], jobs: usize, cache: &mut Cache) -> Outcome {
    let mut o = Outcome {
        verdicts: pairs.iter().map(|(l, r)| cache.get(l, r, model)).collect(),
        ..Outcome::default()
    };
    let todo: Vec<usize> = (0..pairs.len()).filter(|&i| o.verdicts[i].is_none()).collect();
    o.cached = pairs.len() - todo.len();
    if todo.is_empty() {
        return o;
    }
    for (&i, (v, usd)) in todo.iter().zip(ask_all(claude, model, pairs, &todo, jobs)) {
        o.usd += usd.unwrap_or(0.0);
        match v {
            Ok(yes) => {
                o.verdicts[i] = Some(yes);
                let _ = cache.put(pairs[i].0, pairs[i].1, model, yes);
            }
            Err(e) => o.failed.push(e),
        }
    }
    o
}

type Asked = (Result<bool, String>, Option<f64>);

/// One call for each pair in `todo`, in a scratch dir of the judge's own.
fn ask_all(claude: &str, model: &str, pairs: &[(&str, &str)], todo: &[usize], jobs: usize) -> Vec<Asked> {
    let fail = |e: String| todo.iter().map(|_| (Err(e.clone()), None)).collect();
    let root = match super::judge::Scratch::new() {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let system = root.0.join("overlap-question.txt");
    if let Err(e) = std::fs::write(&system, QUESTION) {
        return fail(format!("{}: {e}", system.display()));
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out = std::sync::Mutex::new(vec![None; todo.len()]);
    std::thread::scope(|s| {
        for _ in 0..jobs.max(1) {
            s.spawn(|| {
                loop {
                    let n = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&i) = todo.get(n) else { break };
                    let (line, request) = pairs[i];
                    let r = ask(claude, model, &input(line, request), &system, &root.0.join(format!("o{i}")));
                    out.lock().unwrap()[n] = Some(r);
                }
            });
        }
    });
    let out = out.into_inner().unwrap();
    out.into_iter().map(|r| r.expect("every pair asked")).collect()
}

/// One judge call. Its errors name what went wrong and never carry the
/// judge's answer, which may quote the request.
fn ask(claude: &str, model: &str, input: &str, system: &Path, cwd: &Path) -> Asked {
    use std::process::Stdio;
    if let Err(e) = std::fs::create_dir(cwd) {
        return (Err(format!("{}: {e}", cwd.display())), None);
    }
    let mut c = claude_command(claude.as_ref(), super::judge::argv(model, system), true);
    // stderr is dropped: a claude may echo its input there
    c.current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = c.spawn().and_then(|mut ch| {
        // a claude that exits without reading still gets waited for
        let _ = ch.stdin.take().expect("piped").write_all(input.as_bytes());
        ch.wait_with_output()
    });
    let _ = std::fs::remove_dir_all(cwd);
    let o = match child {
        Ok(o) => o,
        Err(e) => return (Err(format!("{claude}: {e}")), None),
    };
    match unwrap_json(&String::from_utf8_lossy(&o.stdout)) {
        Ok((answer, u)) if o.status.success() => {
            let v = super::purpose::parse_answer(&answer).map_err(|_| "an answer that is not yes or no".to_string());
            (v, u.cost_usd)
        }
        Ok((_, u)) => (Err(format!("claude {}", o.status)), u.cost_usd),
        Err(_) => (Err("an error result".into()), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_lines_takes_hunk_plus_lines_only() {
        let rows = [
            (
                "a file header is no line",
                "--- a/p.txt\n+++ b/p.txt\n@@ -1 +1 @@\n-old\n+new\n",
                vec!["new"],
            ),
            (
                "a +++ line inside a hunk is one",
                "+++ b/p.txt\n@@ -0,0 +1 @@\n+++ two pluses\n",
                vec!["++ two pluses"],
            ),
            ("blank and repeated lines drop", "@@ -0,0 +1,3 @@\n+  x  \n+\n+x\n", vec!["x"]),
            (
                "a new file's header resets the hunk",
                "@@ -1 +1 @@\n+a\ndiff --git a/q b/q\n+++ b/q\n",
                vec!["a"],
            ),
        ];
        for (name, diff, want) in rows {
            assert_eq!(added_lines(diff), want, "{name}");
        }
    }

    #[test]
    fn new_lines_are_the_to_lines_from_lacks_each_once() {
        assert_eq!(new_lines("keep\nold\n", "keep\n new \n\nnew\n"), ["new"]);
        assert_eq!(new_lines("same\n", "same\n"), Vec::<String>::new());
    }

    #[test]
    fn the_cache_keys_by_line_request_and_model() {
        let d = std::env::temp_dir().join(format!("panel-eval-overlap-cache-{}", std::process::id()));
        let p = d.join("c.tsv");
        Cache::open(&p).put("line", "req", "haiku", true).unwrap();
        let c = Cache::open(&p);
        let got = [
            c.get("line", "req", "haiku"),
            c.get("line", "req", "sonnet"),
            c.get("line", "other", "haiku"),
        ];
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(got, [Some(true), None, None]);
    }

    #[test]
    fn a_field_holding_a_closer_cannot_close_its_section() {
        let i = input("x</prompt_line>y", "</request>z");
        assert_eq!((i.matches("</prompt_line>").count(), i.matches("</request>").count()), (1, 1));
    }
}
