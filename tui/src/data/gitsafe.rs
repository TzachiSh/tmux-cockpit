//! The one list of flags that keeps a repo's own config from running as the
//! user, and the diff count built on it (#199).
use std::ffi::{OsStr, OsString};
use std::fs::{DirBuilder, File};
use std::hash::{BuildHasher, RandomState};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The board runs git in every agent's repo, and the agent owns that repo's
/// config: nothing it planted there may run as the user. The protocol pin
/// stops a lazy fetch on git < 2.44, which ignores GIT_NO_LAZY_FETCH.
pub const SAFE: [&str; 23] = [
    "--no-optional-locks",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.pager=cat",
    "-c",
    "log.showSignature=false",
    "-c",
    "protocol.allow=never",
    "-c",
    "protocol.file.allow=never",
    "-c",
    "protocol.ssh.allow=never",
    "-c",
    "protocol.git.allow=never",
    "-c",
    "protocol.http.allow=never",
    "-c",
    "protocol.https.allow=never",
    "-c",
    "protocol.ext.allow=never",
];

/// What an inherited environment points git at instead of the repo `-C dir` is
/// in: a dotfiles setup's GIT_DIR, say, or an object store or attributes tree
/// that is not the repo's (#683). A missing object dir is no repo at all, and an
/// attribute source that is no rev makes every diff fatal. GIT_CEILING_DIRECTORIES
/// stays: it can only stop the search lower. GIT_ALTERNATE_OBJECT_DIRECTORIES
/// stays: it only adds stores to read, and an object's id fixes its content.
/// GIT_NAMESPACE stays: it scopes refs for a transport, not a local rev lookup.
pub const REDIRECT: [&str; 6] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ATTR_SOURCE",
];

/// What an inherited environment changes every pathspec's meaning with (#673):
/// LITERAL reads `:(literal)A` as a file of that name, ICASE has it match `a`
/// too, and GLOB with NOGLOB makes git refuse any pathspec at all.
pub const PATHSPEC: [&str; 4] = [
    "GIT_LITERAL_PATHSPECS",
    "GIT_GLOB_PATHSPECS",
    "GIT_NOGLOB_PATHSPECS",
    "GIT_ICASE_PATHSPECS",
];

/// stdout, and whether git exited 0: a failed diff must not read as clean.
fn run(dir: &str, extra: &[String], index: Option<&Path>, args: &[&str]) -> (bool, String) {
    let (ok, out) = run_raw(dir, extra, index, args);
    (ok, String::from_utf8_lossy(&out).into_owned())
}

/// [`run`] with stdout as git wrote it: a path need not be UTF-8.
fn run_raw<S: AsRef<OsStr>>(dir: &str, extra: &[String], index: Option<&Path>, args: &[S]) -> (bool, Vec<u8>) {
    let mut c = Command::new("git");
    for v in REDIRECT.iter().chain(&PATHSPEC) {
        c.env_remove(v);
    }
    c.arg("-C")
        .arg(dir)
        .args(SAFE)
        .args(extra)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        // a partial clone's missing blob would run the repo's remote.*.uploadpack
        .env("GIT_NO_LAZY_FETCH", "1")
        // and on git < 2.44, which ignores that: no transport at all, a remote helper included
        .env("GIT_ALLOW_PROTOCOL", "");
    if let Some(i) = index {
        c.env("GIT_INDEX_FILE", i);
    }
    // a unit test compares with git run on no user or system config: the
    // runner's core.excludesFile must not hide a file from one side only
    #[cfg(test)]
    c.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1");
    c.output().map(|o| (o.status.success(), o.stdout)).unwrap_or_default()
}

pub(crate) fn git(dir: &str, extra: &[String], index: Option<&Path>, args: &[&str]) -> String {
    run(dir, extra, index, args).1
}

/// Filter drivers are named in config, so each one the repo defines is blanked
/// by name; an empty driver is no filter, and `required=false` keeps it legal.
/// None when a name holds `=`: git splits `-c` at the first one, so
/// `filter.a=b.clean=` would set `filter.a` and leave the planted driver live.
pub fn no_filters(root: &str) -> Option<Vec<String>> {
    filter_flags(&git(root, &[], None, &FILTER_CONFIG))
}

/// The config read [`filter_flags`] parses, for a caller that runs git itself.
pub const FILTER_CONFIG: [&str; 4] = ["config", "-z", "--get-regexp", r"^filter\."];

/// [`no_filters`] on the output of `git config` run with [`FILTER_CONFIG`].
pub fn filter_flags(out: &str) -> Option<Vec<String>> {
    let mut names: Vec<&str> = out
        .split('\0')
        .filter_map(|r| r.split('\n').next()?.strip_prefix("filter.")?.rsplit_once('.').map(|(n, _)| n))
        .collect();
    if names.iter().any(|n| n.contains('=')) {
        return None;
    }
    names.sort_unstable();
    names.dedup();
    Some(
        names
            .iter()
            .flat_map(|n| ["clean=", "smudge=", "process=", "required=false"].map(|k| ["-c".to_string(), format!("filter.{n}.{k}")]))
            .flatten()
            .collect(),
    )
}

/// tmux-review's BIGCAP: past this a file is never hashed, only reported.
const BIGCAP: u64 = 32 * 1024 * 1024;

static IDX_SEQ: AtomicUsize = AtomicUsize::new(0);
static SWEEP: Once = Once::new();

/// Every scratch dir's name starts so, a leaked one too: the sweep reads it.
const SCRATCH: &str = "cockpit-tui-idx.";
/// The excludes file's name before it moved into a scratch dir (#433).
const OLD_PS: &str = "cockpit-tui-ps.";

/// One scan's private dir for its index copy and excludes file (#435). A
/// shared temp dir is the agent's to write too: a fixed name there was a
/// symlink the copy wrote through. This one is mkdir'd 0700 under a name no
/// one can guess, never reused, and Drop removes it on every way out, a
/// panic's unwind included; what a SIGKILL or an abort leaves, [`sweep_stale`] takes.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Option<Scratch> {
        // git resolves GIT_INDEX_FILE and --pathspec-from-file against `-C root`
        let tmp = std::path::absolute(std::env::temp_dir()).ok()?;
        SWEEP.call_once(|| sweep_stale(&tmp));
        let pid = std::process::id();
        let key = RandomState::new().hash_one((pid, IDX_SEQ.fetch_add(1, Ordering::Relaxed)));
        let dir = tmp.join(format!("{SCRATCH}{pid}.{key:016x}"));
        DirBuilder::new().mode(0o700).create(&dir).ok()?; // EEXIST on anything there
        Some(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `src` copied to a new file `dst`, with `src`'s mtime. Public for a test
/// that plants a symlink at `dst`.
#[doc(hidden)]
pub fn copy_index(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut from = File::open(src)?;
    let mtime = from.metadata()?.modified()?;
    let mut to = File::options().write(true).create_new(true).mode(0o600).open(dst)?;
    std::io::copy(&mut from, &mut to)?;
    to.set_modified(mtime)
}

/// Past this a scratch entry is a killed scan's: a scan takes well under a second.
const STALE: std::time::Duration = std::time::Duration::from_secs(3600);

/// Removes what a killed scan left in `tmp`: a scratch dir, or a bare index
/// copy or excludes file from before them, last changed over [`STALE`] ago.
/// Age alone says stale: a pid is never read, since a live board may run
/// under another name or in another pid namespace. Only those exact names,
/// only the user's own, and a symlink is never followed or removed.
pub fn sweep_stale(tmp: &Path) {
    let Ok(entries) = std::fs::read_dir(tmp) else { return };
    let uid = unsafe { libc::geteuid() };
    let now = std::time::SystemTime::now();
    for e in entries.flatten() {
        if !e.file_name().to_str().is_some_and(scratch_name) {
            continue;
        }
        let p = e.path();
        let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
        let old = m.modified().is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > STALE));
        if m.uid() != uid || !old {
            continue;
        }
        let gone = if m.is_dir() {
            std::fs::remove_dir_all(&p)
        } else if m.is_file() {
            std::fs::remove_file(&p)
        } else {
            continue;
        };
        if gone.is_ok() {
            eprintln!("cockpit-tui: level=info msg=\"swept a stale scratch entry\" path={p:?}");
        }
    }
}

/// `cockpit-tui-idx.<digits>.<hex>` or `cockpit-tui-ps.<digits>.<hex>`, exactly.
fn scratch_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix(SCRATCH).or_else(|| name.strip_prefix(OLD_PS)) else {
        return false;
    };
    let all = |s: &str, f: fn(&u8) -> bool| !s.is_empty() && s.bytes().all(|b| f(&b));
    rest.split_once('.')
        .is_some_and(|(pid, tail)| all(pid, u8::is_ascii_digit) && all(tail, u8::is_ascii_hexdigit))
}

/// Lines added, deleted, files new against the base, and the board's `±` string.
pub type Stat = (u64, u64, u64, String);

/// tmux-review's `tree_diff`, so `prefix r` and the board agree. The index copy
/// keeps its mtime, or git's racy check trusts a same-second edit as clean.
/// `base` None is HEAD; None back means git failed (a bad base, not a repo).
pub fn tree_stat(root: &str, gitdir: &str, base: Option<&str>) -> Option<Stat> {
    tree_stat_with(root, gitdir, base, &write_new)
}

/// A file at `p` holding `bytes`, never one that was already there.
fn write_new(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    File::options().write(true).create_new(true).mode(0o600).open(p)?.write_all(bytes)
}

/// [`tree_stat`] with the excludes file written by `write_excludes`: a test
/// makes that one write fail, or panic, and nothing else.
#[doc(hidden)]
pub fn tree_stat_with(
    root: &str,
    gitdir: &str,
    base: Option<&str>,
    write_excludes: &dyn Fn(&Path, &[u8]) -> std::io::Result<()>,
) -> Option<Stat> {
    if base.is_some_and(|b| b.starts_with('-')) {
        return None; // git would read it as an option: `--output=<file>` writes
    }
    let filters = no_filters(root)?;
    // -m re-hashes a touched tracked file, and hashing runs its clean filter
    let listed = git(root, &filters, None, &["ls-files", "-o", "-m", "--exclude-standard", "-z"]);
    let big: Vec<&str> = listed
        .split('\0')
        .filter(|f| !f.is_empty())
        .filter(|f| std::fs::metadata(Path::new(root).join(f)).is_ok_and(|m| m.is_file() && m.len() > BIGCAP))
        .collect();
    // A gitlink's repo has its own config, filters included (#428), and `add -A`
    // checks every gitlink for dirt by running status inside it. add has no
    // --ignore-submodules and overrides diff.ignoreSubmodules, so only a
    // pathspec keeps it out, read from a file: a path need not be UTF-8, and
    // thousands of them would pass ARG_MAX.
    let (ok, staged) = run_raw(root, &[], None, &["ls-files", "-s", "-z"]);
    if !ok {
        return None;
    }
    let excludes = gitlink_excludes(&staged);
    let scratch = Scratch::new()?;
    let idx = scratch.0.join("index");
    if copy_index(&Path::new(gitdir).join("index"), &idx).is_err() {
        let _ = std::fs::remove_file(&idx); // no index yet: git starts an empty one
    }
    let mut intent: Vec<OsString> = ["add", "-N", "-A"].map(OsString::from).into();
    if !excludes.is_empty() {
        let ps = scratch.0.join("pathspec");
        if write_excludes(&ps, &excludes).is_err() {
            return None; // never an add without its excludes
        }
        let mut from = OsString::from("--pathspec-from-file=");
        from.push(&ps);
        intent.extend([from, "--pathspec-file-nul".into()]);
    }
    run_raw(root, &filters, Some(&idx), &intent);
    let ex: Vec<String> = big.iter().map(|b| format!(":(exclude,literal){b}")).collect();
    // -z: paths raw, as ls-files gives them, so a name with a tab, a quote or a
    // non-UTF-8 byte compares as itself (#657 review)
    let diff = |rev: &[&str]| {
        let mut args = vec![
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            // a gitlink's commit still counts (`1 1 sub`, as prefix r has it);
            // its dirt, which takes a status inside it, does not
            "--ignore-submodules=dirty",
            // renames on whatever diff.renames says: a staged `git mv` is a rename
            "-M",
            "--numstat",
            "-z",
        ];
        args.extend(rev);
        if !ex.is_empty() {
            args.push("--");
            args.extend(ex.iter().map(String::as_str));
        }
        run_raw(root, &filters, Some(&idx), &args)
    };
    let unborn = base.is_none() && !run(root, &[], None, &["rev-parse", "--verify", "-q", "HEAD"]).0;
    // An unborn tree has no commit to diff: the staged lines (the index against
    // nothing) and the worktree's change to them, both, every time (#657 review).
    let outs: Vec<(bool, Vec<u8>)> = if unborn {
        vec![diff(&[]), diff(&["--cached"])]
    } else {
        vec![diff(&[base.unwrap_or("HEAD")])]
    };
    if outs.iter().any(|(ok, _)| !ok) {
        return None;
    }
    // #454, the owner's pick: `+a -d` counts tracked changes only. A file the real
    // index does not hold is untracked: the intent-to-add above only lets the diff
    // see it, and it counts once, in `?N new`, its lines nowhere. A staged new
    // file is tracked and counts.
    let (ok, others) = run_raw(root, &filters, None, &["ls-files", "-o", "--exclude-standard", "-z"]);
    if !ok {
        return None;
    }
    let untracked: std::collections::HashSet<&[u8]> = others.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
    let num = |x: &[u8]| std::str::from_utf8(x).ok().and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
    let (mut add, mut del, mut rows) = (0u64, 0u64, false);
    // a tracked file whose lines left the tree for an untracked one: a plain `mv`
    // (rename detection pairs it with the new name's intent-to-add copy), or
    // `git rm --cached` (the copy re-adds it unchanged, so no row shows at all)
    let mut gone: Vec<&[u8]> = vec![];
    // `git rm --cached`: deleted from the real index against the base, and in the
    // tree still, untracked; the intent-to-add copy hides it from the diff
    let (ok, dropped) = if unborn {
        (true, vec![])
    } else {
        let args = [
            "diff",
            "--cached",
            "--name-only",
            "-z",
            "--no-renames",
            "--diff-filter=D",
            base.unwrap_or("HEAD"),
        ];
        run_raw(root, &filters, None, &args)
    };
    if !ok {
        return None;
    }
    gone.extend(dropped.split(|b| *b == 0).filter(|p| untracked.contains(p)));
    for (_, out) in &outs {
        let mut t = out.split(|b| *b == 0);
        while let Some(rec) = t.next() {
            let mut f = rec.splitn(3, |b| *b == b'\t');
            let (Some(a), Some(d), Some(path)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            rows = true;
            if path.is_empty() {
                // a rename: its source and destination follow, NUL-ended
                let (Some(src), Some(dst)) = (t.next(), t.next()) else { break };
                if untracked.contains(dst) {
                    gone.push(src);
                    continue;
                }
            } else if untracked.contains(path) {
                continue;
            }
            add += num(a);
            del += num(d);
        }
    }
    del += base_lines(root, &filters, base, unborn, &gone)?;
    drop(scratch);
    let new = untracked.len() as u64;
    if !rows && new == 0 && del == 0 && big.is_empty() {
        return Some((0, 0, 0, "clean".into()));
    }
    let mut pm = format!("+{add} -{del}");
    if new > 0 {
        pm.push_str(&format!("  ?{new} new"));
    }
    Some((add, del, new, pm))
}

/// The lines `paths` held at `base` (HEAD when None): the empty tree diffed
/// against it, so each path's lines are its additions. 0 in an unborn tree or
/// with no paths, which is the usual case: these are only the files a plain `mv`
/// or `git rm --cached` took out of the tracked tree.
fn base_lines(root: &str, filters: &[String], base: Option<&str>, unborn: bool, paths: &[&[u8]]) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    if unborn || paths.is_empty() {
        return Some(0);
    }
    let (ok, empty) = run(root, &[], None, &["hash-object", "-t", "tree", "/dev/null"]);
    let empty = empty.trim();
    if !ok || empty.is_empty() {
        return None;
    }
    let mut args: Vec<OsString> = [
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--numstat",
        "-z",
    ]
    .map(OsString::from)
    .into();
    args.extend([OsString::from(empty), OsString::from(base.unwrap_or("HEAD")), OsString::from("--")]);
    // one pathspec matches a path once: sorted and deduped, no path sits in two batches
    let mut paths = paths.to_vec();
    paths.sort_unstable();
    paths.dedup();
    let specs: Vec<OsString> = paths
        .iter()
        .map(|p| {
            let mut spec = OsString::from(":(literal)");
            spec.push(std::ffi::OsStr::from_bytes(p));
            spec
        })
        .collect();
    let num = |x: &[u8]| std::str::from_utf8(x).ok().and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
    let mut lines = 0;
    for batch in batches(&specs, ARGS_BUDGET) {
        let (ok, out) = run_raw(root, filters, None, &[&args[..], batch].concat());
        if !ok {
            return None;
        }
        lines += out
            .split(|b| *b == 0)
            .filter_map(|r| r.split(|b| *b == b'\t').next())
            .map(num)
            .sum::<u64>();
    }
    Some(lines)
}

/// Pathspec bytes on one [`base_lines`] diff (#665). A plain `mv` of a big tree
/// drops thousands of paths, and all of them on one command line passed ARG_MAX:
/// the spawn failed and the row read `?`. Linux allows at least 128 KiB of
/// arguments and environment together; half of that leaves the rest to the
/// environment and the fixed flags. A test lowers it to cross it cheaply.
#[cfg(not(test))]
const ARGS_BUDGET: usize = 64 * 1024;
#[cfg(test)]
const ARGS_BUDGET: usize = 256;

/// `specs` cut in order into runs costing at most `budget` bytes each, as
/// execve counts an argument: its bytes, a NUL and a pointer. A spec over the
/// budget goes alone, so every spec is in exactly one run.
fn batches(specs: &[OsString], budget: usize) -> Vec<&[OsString]> {
    let mut out = vec![];
    let (mut start, mut used) = (0, 0);
    for (i, s) in specs.iter().enumerate() {
        let cost = s.len() + 1 + std::mem::size_of::<usize>();
        if i > start && used + cost > budget {
            out.push(&specs[start..i]);
            (start, used) = (i, 0);
        }
        used += cost;
    }
    if start < specs.len() {
        out.push(&specs[start..]);
    }
    out
}

/// A NUL-separated exclusion pathspec for each gitlink in `git ls-files -s -z`
/// output, its path byte for byte.
fn gitlink_excludes(staged: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in staged.split(|b| *b == 0) {
        let Some(rest) = r.strip_prefix(b"160000 ") else { continue };
        let Some(tab) = rest.iter().position(|b| *b == b'\t') else {
            continue;
        };
        out.extend_from_slice(b":(exclude,literal)");
        out.extend_from_slice(&rest[tab + 1..]);
        out.push(0);
    }
    out
}

/// `cockpit-tui git-flags [--filters <dir>]`: one word per line, so a shell
/// caller can `mapfile` it straight into an argv.
pub fn git_flags(args: &[String]) -> i32 {
    let words: Vec<String> = match args {
        [] => SAFE.iter().map(|s| s.to_string()).collect(),
        [f, dir] if f == "--filters" => match no_filters(dir) {
            Some(w) => w,
            None => {
                eprintln!("cockpit-tui: refusing: filter driver name contains '='");
                return 1;
            }
        },
        _ => {
            eprintln!("usage: cockpit-tui git-flags [--filters <dir>]");
            return 2;
        }
    };
    for w in words {
        println!("{w}");
    }
    0
}

/// `cockpit-tui diffstat <root> [--base <rev>]`: `<add>\t<del>\t<new>\t<pm>`.
/// The git dir comes off `<root>/.git` itself (a dir, or a `gitdir:` file): no fork.
pub fn diffstat(args: &[String]) -> i32 {
    let (root, base) = match args {
        [r] => (r, None),
        [r, b, rev] if b == "--base" => (r, Some(rev.as_str())),
        _ => {
            eprintln!("usage: cockpit-tui diffstat <root> [--base <rev>]");
            return 2;
        }
    };
    let dotgit = Path::new(root).join(".git");
    let gitdir = if dotgit.is_dir() {
        dotgit
    } else {
        let Some(gd) = std::fs::read_to_string(&dotgit)
            .ok()
            .and_then(|s| s.lines().next()?.strip_prefix("gitdir: ").map(str::to_string))
        else {
            return 1;
        };
        Path::new(root).join(gd) // an absolute gitdir replaces root
    };
    match tree_stat(root, &gitdir.to_string_lossy(), base) {
        Some((a, d, n, pm)) => {
            println!("{a}\t{d}\t{n}\t{pm}");
            0
        }
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs(lens: &[usize]) -> Vec<OsString> {
        lens.iter().map(|n| OsString::from("p".repeat(*n))).collect()
    }

    fn sizes(b: &[&[OsString]]) -> Vec<Vec<usize>> {
        b.iter().map(|r| r.iter().map(|s| s.len()).collect()).collect()
    }

    /// A table row: its name, the spec lengths, the run lengths wanted.
    type Row = (&'static str, &'static [usize], &'static [&'static [usize]]);

    #[test]
    fn batches_cut_in_order_under_the_budget() {
        // each spec costs its length + 1 + 8 on a 64-bit target: 7 bytes cost 16
        let rows: [Row; 5] = [
            ("no specs, no runs", &[], &[]),
            ("all fit in one", &[7, 7], &[&[7, 7]]),
            ("exactly the budget", &[7, 7, 7, 7], &[&[7, 7, 7, 7]]),
            ("one past it starts a run", &[7, 7, 7, 7, 7], &[&[7, 7, 7, 7], &[7]]),
            ("an oversize spec goes alone", &[7, 100, 7], &[&[7], &[100], &[7]]),
        ];
        for (name, lens, want) in rows {
            let want: Vec<Vec<usize>> = want.iter().map(|r| r.to_vec()).collect();
            assert_eq!(sizes(&batches(&specs(lens), 64)), want, "{name}");
        }
    }

    /// A throwaway repo under the temp dir, removed on drop.
    struct Repo(PathBuf);

    impl Repo {
        fn new(tag: &str) -> Repo {
            let name = format!("cockpit-gitsafe-unit-{tag}-{}", std::process::id());
            let d = std::env::temp_dir().join(name);
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            let r = Repo(d);
            r.git(&["init", "-q", "-b", "main"]);
            r
        }

        fn git(&self, args: &[&str]) -> Vec<u8> {
            let o = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
            o.stdout
        }

        /// `n` files under `top/`, file i holding i lines, committed.
        fn committed(tag: &str, n: usize) -> Repo {
            let r = Repo::new(tag);
            std::fs::create_dir_all(r.0.join("top")).unwrap();
            for i in 1..=n {
                std::fs::write(r.0.join(format!("top/file-{i:03}")), "x\n".repeat(i)).unwrap();
            }
            r.git(&["add", "-A"]);
            r.git(&["commit", "-q", "-m", "one"]);
            r
        }

        fn stat(&self) -> Option<Stat> {
            let root = self.0.display().to_string();
            tree_stat(&root, &self.0.join(".git").display().to_string(), None)
        }

        /// What plain git says: deleted lines off `diff HEAD`, files off `ls-files -o`.
        fn reference(&self) -> (u64, u64, u64) {
            let numstat = String::from_utf8(self.git(&["diff", "HEAD", "--numstat"])).unwrap();
            let field = |l: &str, k: usize| l.split('\t').nth(k).unwrap().parse::<u64>().unwrap();
            let col = |k: usize| numstat.lines().map(|l| field(l, k)).sum();
            let others = self.git(&["ls-files", "-o", "--exclude-standard", "-z"]);
            (col(0), col(1), others.split(|b| *b == 0).filter(|p| !p.is_empty()).count() as u64)
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // 40 specs of 22 bytes, 31 as execve counts them, against a 256-byte budget: five diffs
    const FILES: usize = 40;

    #[test]
    fn the_fixture_crosses_the_lowered_budget() {
        let spec = OsString::from(":(literal)top/file-001");
        assert!(FILES * (spec.len() + 9) > 4 * ARGS_BUDGET, "too few files to batch");
    }

    #[test]
    fn a_plain_mv_past_the_budget_counts_what_git_does() {
        let r = Repo::committed("mv", FILES);
        std::fs::rename(r.0.join("top"), r.0.join("moved")).unwrap();
        let (add, del, new, _) = r.stat().expect("a stat");
        assert_eq!((add, del, new), r.reference());
        assert_eq!(del, (FILES * (FILES + 1) / 2) as u64, "every file's lines, each once");
    }

    #[test]
    fn a_path_named_twice_counts_once_as_on_one_command_line() {
        let r = Repo::committed("dup", FILES);
        // file-008 twice: sorted, the pair straddles the end of the first
        // batch of eight, so only the dedup keeps it to one diff
        let names: Vec<_> = (1..=FILES).chain([8]).map(|i| format!("top/file-{i:03}")).collect();
        let paths: Vec<&[u8]> = names.iter().map(|s| s.as_bytes()).collect();
        let root = r.0.display().to_string();
        let want = (FILES * (FILES + 1) / 2) as u64;
        assert_eq!(base_lines(&root, &[], None, false, &paths), Some(want));
    }

    #[test]
    fn a_rm_cached_past_the_budget_counts_what_git_does() {
        let r = Repo::committed("rmc", FILES);
        r.git(&["rm", "-rq", "--cached", "top"]);
        let (add, del, new, _) = r.stat().expect("a stat");
        assert_eq!((add, del, new), r.reference());
        assert_eq!(del, (FILES * (FILES + 1) / 2) as u64, "every file's lines, each once");
    }
}
