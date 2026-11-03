//! The board's per-agent git facts (#253): which repo each pane works in, its
//! branch and its diff stat. Runs on the fetch thread, never on the render path.
//! One diff scan per distinct repo root however many panes share it, at most
//! `PARALLEL` git processes in flight (the old `COCKPIT_FLEET_PARALLEL=8`).
use super::gitsafe::{git, tree_stat};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const PARALLEL: usize = 8;

/// `repo⑂branch`, the `±` string and the repo root; all empty outside a repo.
pub type Facts = (String, String, String);

/// What one scan found, and how many diff scans it took to find it.
pub struct Scan {
    pub facts: HashMap<String, Facts>,
    pub stats: usize,
}

/// How many threads `n` items get: never more than `PARALLEL`, never idle ones.
fn workers(n: usize) -> usize {
    n.min(PARALLEL)
}

/// `f` over `items` on up to `PARALLEL` threads, results in item order.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<R>>> = Mutex::new(items.iter().map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..workers(items.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(it) = items.get(i) else { break };
                    let r = f(it);
                    out.lock().unwrap()[i] = Some(r);
                }
            });
        }
    });
    out.into_inner().unwrap().into_iter().flatten().collect()
}

/// The tree a pane works in: `@cockpit_dir` when it names a directory (a
/// worktree made by hand keeps the process in the main checkout), else the
/// cwd. tmux-review's `pane_dir` rule. Each pane is `(cwd, @cockpit_dir)`, both
/// off the `list-panes` the fetch already made, so this forks nothing; a
/// dangling option (the tree was removed) falls back to the cwd.
pub fn work_dirs(panes: &[(String, String)]) -> Vec<String> {
    panes
        .iter()
        .map(|(cwd, dir)| {
            if !dir.is_empty() && std::path::Path::new(dir).is_dir() {
                dir.clone()
            } else {
                cwd.clone()
            }
        })
        .collect()
}

/// Facts for every distinct path in `paths`.
pub fn scan(paths: &[String]) -> Scan {
    let mut uniq: Vec<&String> = paths.iter().collect();
    uniq.sort();
    uniq.dedup();
    // root, git dir and branch: one fork per path
    let probes = par_map(&uniq, |p| {
        let out = git(
            p,
            &[],
            None,
            &["rev-parse", "--show-toplevel", "--absolute-git-dir", "--abbrev-ref", "HEAD"],
        );
        let mut it = out.lines();
        Some((it.next()?.to_string(), it.next()?.to_string(), it.next()?.to_string()))
    });
    let mut roots: Vec<(&str, &str)> = probes.iter().flatten().map(|(r, g, _)| (r.as_str(), g.as_str())).collect();
    roots.sort();
    roots.dedup_by_key(|r| r.0);
    let stats = par_map(&roots, |(r, g)| tree_stat(r, g, None).map(|s| s.3).unwrap_or_default());
    let by_root: HashMap<&str, &String> = roots.iter().map(|r| r.0).zip(&stats).collect();
    let facts = uniq
        .into_iter()
        .zip(&probes)
        .map(|(p, probe)| {
            let f = match probe {
                Some((root, _, branch)) => {
                    let repo = root.rsplit('/').next().unwrap_or(root);
                    (format!("{repo}⑂{branch}"), by_root[root.as_str()].clone(), root.clone())
                }
                None => Facts::default(),
            };
            (p.clone(), f)
        })
        .collect();
    Scan { facts, stats: roots.len() }
}

/// The diff stat as the row shows it: `+a -d`, `clean`, `no repo`, or
/// `?N new` when untracked files are all there is. `pm` is `Pane.pm`.
pub fn stat_text(loc: &str, pm: &str) -> String {
    if loc.is_empty() {
        return "no repo".into();
    }
    if pm.is_empty() || pm == "clean" {
        return if pm.is_empty() { "?".into() } else { pm.into() };
    }
    let (counts, new) = pm.split_once("  ").unwrap_or((pm, ""));
    let new = new.trim();
    if counts == "+0 -0" && !new.is_empty() {
        return new.into();
    }
    if new.is_empty() { counts.into() } else { format!("{counts} {new}") }
}

/// The branch half of `repo⑂branch`, split at the last mark; empty outside a repo.
pub fn branch(loc: &str) -> &str {
    loc.rsplit_once('⑂').map_or("", |(_, b)| b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stat_reads_like_prefix_r_did() {
        let t = |loc, pm| stat_text(loc, pm);
        assert_eq!(t("", ""), "no repo");
        assert_eq!(t("r⑂main", "clean"), "clean");
        assert_eq!(t("r⑂main", "+4 -1"), "+4 -1");
        assert_eq!(t("r⑂main", "+0 -0  ?2 new"), "?2 new");
        assert_eq!(t("r⑂main", "+4 -0  ?3 new"), "+4 -0 ?3 new");
        assert_eq!(t("r⑂main", ""), "?");
    }

    #[test]
    fn the_branch_is_what_follows_the_mark() {
        assert_eq!(branch("api⑂feat/x"), "feat/x");
        assert_eq!(branch(""), "");
        // a repo directory named with the mark: the branch is what follows the last one
        assert_eq!(branch("a⑂b⑂main"), "main");
    }

    #[test]
    fn par_map_keeps_order_and_never_runs_more_than_the_cap() {
        let (now, peak) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let items: Vec<usize> = (0..40).collect();
        let out = par_map(&items, |&i| {
            let n = now.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(n, Ordering::SeqCst);
            for _ in 0..200 {
                std::thread::yield_now();
            }
            now.fetch_sub(1, Ordering::SeqCst);
            i * 2
        });
        assert_eq!(out, items.iter().map(|i| i * 2).collect::<Vec<_>>());
        assert!(peak.load(Ordering::SeqCst) <= PARALLEL);
    }

    #[test]
    fn a_big_fleet_gets_eight_workers_and_a_small_one_gets_one_each() {
        assert_eq!((workers(0), workers(3), workers(8), workers(50)), (0, 3, 8, 8));
    }

    #[test]
    fn par_map_of_nothing_is_nothing() {
        assert!(par_map::<u8, u8>(&[], |&x| x).is_empty());
    }
}
