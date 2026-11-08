//! The board's per-agent git facts (#253): which repo each pane works in, its
//! branch and its diff stat. Runs on the fetch thread, never on the render path.
//! One diff scan per distinct repo root however many panes share it, at most
//! `PARALLEL` git processes in flight (the old `COCKPIT_FLEET_PARALLEL=8`).
use super::gitsafe::{board_stat, git};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const PARALLEL: usize = 8;

/// `loc_of`'s `repo⑂branch[ · wt name]`, the `±` string and the repo root;
/// all empty outside a repo.
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

/// The tree a pane works in, from its candidates in order: `@cockpit_dir`, the
/// session transcript's newest cwd, pane_current_path (#452). The first that is
/// an absolute path naming a directory wins, else the last. A relative one would
/// be read against the board's own cwd, so it never counts. A worktree made by
/// hand keeps the process in the main checkout, so the cwd comes last; a dangling
/// option (the tree was removed) falls through. `cockpit-tui workdir %N` prints
/// this for one pane, for scripts (#500). Every candidate is off a read the fetch
/// already made, so this forks nothing.
pub fn work_dirs(panes: &[[String; 3]]) -> Vec<String> {
    panes
        .iter()
        .map(|[opt, said, cwd]| {
            let dir = [opt, said]
                .into_iter()
                .find(|d| Path::new(d).is_absolute() && Path::new(d).is_dir());
            dir.unwrap_or(cwd).clone()
        })
        .collect()
}

/// Facts for every distinct path in `paths`.
pub fn scan(paths: &[String]) -> Scan {
    let mut uniq: Vec<&String> = paths.iter().collect();
    uniq.sort();
    uniq.dedup();
    // root, git dir, common dir, HEAD and branch: one fork per path
    let probes = par_map(&uniq, |p| {
        // #452: a relative path, or none, would describe the board's own tree
        if !Path::new(p).is_absolute() {
            return None;
        }
        let out = git(
            p,
            &[],
            None,
            &[
                "rev-parse",
                "--show-toplevel",
                "--absolute-git-dir",
                "--git-common-dir",
                "HEAD",
                "--abbrev-ref",
                "HEAD",
            ],
        );
        let [root, gd, common, head, sha] = parse(&out)?;
        let (git_dir, common) = (real(p, &gd), real(p, &common));
        let loc = loc_of(&root, &git_dir, &common, &head, &sha);
        Some((root, gd, loc))
    });
    let mut roots: Vec<(&str, &str)> = probes.iter().flatten().map(|(r, g, _)| (r.as_str(), g.as_str())).collect();
    roots.sort();
    roots.dedup_by_key(|r| r.0);
    let stats = par_map(&roots, |(r, g)| board_stat(r, g).map(|s| s.3).unwrap_or_default());
    let by_root: HashMap<&str, &String> = roots.iter().map(|r| r.0).zip(&stats).collect();
    let facts = uniq
        .into_iter()
        .zip(&probes)
        .map(|(p, probe)| {
            let f = match probe {
                Some((root, _, loc)) => (loc.clone(), by_root[&root[..]].clone(), root.clone()),
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

/// What a linked worktree's `loc` carries after the branch.
const WT: &str = " · wt ";

/// The rev-parse lines as `[root, git dir, common dir, branch, sha]`. An unborn
/// HEAD stops rev-parse after it echoes the word: four lines, the last `HEAD`,
/// and no sha. Any other four (git < 2.25 can skip the toplevel) are no facts.
fn parse(out: &str) -> Option<[String; 5]> {
    let l: Vec<&str> = out.lines().collect();
    let (head, sha) = match l.len() {
        5.. => (l[4], l[3]),
        4 if l[3] == "HEAD" => (l[3], ""),
        _ => return None,
    };
    Some([l[0], l[1], l[2], head, sha].map(String::from))
}

/// `d` as an absolute path with no symlinks, read relative to `dir`: rev-parse
/// prints the common dir relative to the directory it ran in (`../.git` from a
/// subdirectory). Empty when it does not exist.
fn real(dir: &str, d: &str) -> String {
    let r = std::fs::canonicalize(Path::new(dir).join(d));
    r.map(|r| r.display().to_string()).unwrap_or_default()
}

fn base(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p)
}

/// The row's `loc` (#312): `repo⑂branch`, plus ` · wt name` in a linked
/// worktree, where repo is the main checkout's (or a bare repo's) name; a
/// detached HEAD is `@` and its short sha. `gitdir` and `common` are absolute.
/// A worktree of a `--separate-git-dir` repo is named after that storage dir.
pub fn loc_of(top: &str, gitdir: &str, common: &str, head: &str, sha: &str) -> String {
    if top.is_empty() {
        return String::new();
    }
    let at = match (head, sha.get(..7).unwrap_or(sha)) {
        ("HEAD", s) if !s.is_empty() => format!("@{s}"),
        (h, _) => h.to_string(),
    };
    if gitdir.is_empty() || common.is_empty() || gitdir == common {
        return format!("{}⑂{at}", base(top));
    }
    let repo = match common.trim_end_matches('/').strip_suffix("/.git") {
        Some(main) => base(main),
        None => {
            let b = base(common);
            b.strip_suffix(".git").filter(|r| !r.is_empty()).unwrap_or(b)
        }
    };
    format!("{repo}⑂{at}{WT}{}", base(top))
}

/// `loc` split at its worktree suffix. The suffix only counts when the part
/// before it has a mark, so a repo named with ` · wt ` alone still reads; one
/// whose name holds both ` · wt ` and `⑂` is ambiguous, and may split wrong.
fn head_of(loc: &str) -> (&str, &str) {
    match loc.rsplit_once(WT) {
        Some((h, w)) if h.contains('⑂') => (h, w),
        _ => (loc, ""),
    }
}

/// The branch half of `repo⑂branch`, split at the last mark before any
/// worktree suffix; empty outside a repo.
pub fn branch(loc: &str) -> &str {
    head_of(loc).0.rsplit_once('⑂').map_or("", |(_, b)| b)
}

/// The repo half of `repo⑂branch`, before the last mark ahead of any
/// worktree suffix; empty outside a repo.
pub fn repo(loc: &str) -> &str {
    head_of(loc).0.rsplit_once('⑂').map_or("", |(r, _)| r)
}

/// The linked worktree's name, empty in a main checkout or outside a repo.
pub fn worktree(loc: &str) -> &str {
    head_of(loc).1
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

    const SHA: &str = "1b94824e58760c4d8308732ec959978728b96452";

    #[test]
    fn a_main_checkout_reads_repo_and_branch() {
        assert_eq!(loc_of("/x/api", "/x/api/.git", "/x/api/.git", "main", SHA), "api⑂main");
    }

    #[test]
    fn a_linked_worktree_names_the_main_repo_and_the_worktree() {
        let l = loc_of("/x/w1", "/x/m/.git/worktrees/w1", "/x/m/.git", "feat", SHA);
        assert_eq!(l, "m⑂feat · wt w1");
    }

    #[test]
    fn a_worktree_of_a_bare_repo_drops_the_dot_git() {
        let l = loc_of("/x/bw", "/x/b.git/worktrees/bw", "/x/b.git", "main", SHA);
        assert_eq!(l, "b⑂main · wt bw");
    }

    #[test]
    fn a_detached_head_reads_as_its_short_sha() {
        assert_eq!(loc_of("/x/api", "/x/api/.git", "/x/api/.git", "HEAD", SHA), "api⑂@1b94824");
        let l = loc_of("/x/w1", "/x/m/.git/worktrees/w1", "/x/m/.git", "HEAD", SHA);
        assert_eq!(l, "m⑂@1b94824 · wt w1");
    }

    #[test]
    fn an_unborn_branch_has_no_sha_and_keeps_head() {
        assert_eq!(loc_of("/x/e", "/x/e/.git", "/x/e/.git", "HEAD", ""), "e⑂HEAD");
    }

    #[test]
    fn odd_shapes_never_panic() {
        assert_eq!(loc_of("", "", "", "", ""), "");
        assert_eq!(loc_of("/x/api", "/x/api/.git", "/x/api/.git", "HEAD", "1b9"), "api⑂@1b9");
    }

    #[test]
    fn rev_parse_output_reads_born_unborn_and_broken() {
        let born = "/x/m\n/x/m/.git\n.git\n1b94824\nmain\n";
        assert_eq!(
            parse(born),
            Some(["/x/m", "/x/m/.git", ".git", "main", "1b94824"].map(String::from))
        );
        // rev-parse dies at an unborn HEAD after echoing the word: four lines
        let unborn = "/x/e\n/x/e/.git\n.git\nHEAD\n";
        let want = ["/x/e", "/x/e/.git", ".git", "HEAD", ""];
        assert_eq!(parse(unborn), Some(want.map(String::from)));
        assert_eq!(parse("/x/m\n/x/m/.git\n"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn four_lines_are_unborn_only_when_the_last_is_head() {
        // git < 2.25 in a bare repo prints no toplevel line and carries on
        assert_eq!(parse("/x/b.git\n.\n1b94824\nmain\n"), None);
    }

    #[test]
    fn a_relative_dir_resolves_against_the_path_not_the_cwd() {
        let d = std::env::temp_dir().join(format!("cockpit-fleet-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let sub = d.join("sub").display().to_string();
        let want = std::fs::canonicalize(d.join(".git")).unwrap().display().to_string();
        let got = (real(&sub, "../.git"), real(&sub, &want), real(&sub, "../nope"));
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(got, (want.clone(), want, String::new()));
    }

    #[test]
    fn the_repo_is_what_precedes_the_branch() {
        assert_eq!(repo("m⑂feat · wt w1"), "m");
        assert_eq!(repo("a⑂b⑂main"), "a⑂b");
        assert_eq!(repo("api⑂main"), "api");
        assert_eq!(repo(""), "");
    }

    #[test]
    fn the_branch_leaves_the_worktree_suffix_behind() {
        assert_eq!(branch("m⑂feat · wt w1"), "feat");
        assert_eq!(branch("m⑂@1b94824 · wt w1"), "@1b94824");
        // the worktree's own name may carry the mark
        assert_eq!(branch("m⑂feat · wt a⑂b"), "feat");
        // a repo named like the suffix, outside any worktree
        assert_eq!(branch("a · wt b⑂main"), "main");
    }

    #[test]
    fn the_worktree_is_what_follows_the_suffix() {
        assert_eq!(worktree("m⑂feat · wt w1"), "w1");
        assert_eq!(worktree("m⑂feat · wt a⑂b"), "a⑂b");
        assert_eq!(worktree("api⑂main"), "");
        assert_eq!(worktree("a · wt b⑂main"), "");
        assert_eq!(worktree(""), "");
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
