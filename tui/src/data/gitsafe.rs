//! The one list of flags that keeps a repo's own config from running as the
//! user, and the diff count built on it (#199).
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
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

/// stdout, and whether git exited 0: a failed diff must not read as clean.
fn run(dir: &str, extra: &[String], index: Option<&Path>, args: &[&str]) -> (bool, String) {
    let (ok, out) = run_raw(dir, extra, index, args);
    (ok, String::from_utf8_lossy(&out).into_owned())
}

/// [`run`] with stdout as git wrote it: a path need not be UTF-8.
fn run_raw<S: AsRef<OsStr>>(dir: &str, extra: &[String], index: Option<&Path>, args: &[S]) -> (bool, Vec<u8>) {
    let mut c = Command::new("git");
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

/// Lines added, deleted, files new against the base, and the board's `±` string.
pub type Stat = (u64, u64, u64, String);

/// tmux-review's `tree_diff`, so `prefix r` and the board agree. The index copy
/// keeps its mtime, or git's racy check trusts a same-second edit as clean.
/// `base` None is HEAD; None back means git failed (a bad base, not a repo).
pub fn tree_stat(root: &str, gitdir: &str, base: Option<&str>) -> Option<Stat> {
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
    let n = IDX_SEQ.fetch_add(1, Ordering::Relaxed);
    let idx = std::env::temp_dir().join(format!("cockpit-tui-idx.{}.{n}", std::process::id()));
    let src = Path::new(gitdir).join("index");
    let copied = std::fs::copy(&src, &idx).is_ok()
        && std::fs::metadata(&src)
            .and_then(|m| m.modified())
            .and_then(|t| File::options().write(true).open(&idx)?.set_modified(t))
            .is_ok();
    if !copied {
        let _ = std::fs::remove_file(&idx);
    }
    let mut intent: Vec<OsString> = ["add", "-N", "-A"].map(OsString::from).into();
    let ps = std::env::temp_dir().join(format!("cockpit-tui-ps.{}.{n}", std::process::id()));
    if !excludes.is_empty() {
        // create_new: a name planted in the shared temp dir is never followed
        let _ = std::fs::remove_file(&ps);
        let wrote = File::options()
            .write(true)
            .create_new(true)
            .open(&ps)
            .and_then(|mut f| f.write_all(&excludes));
        if wrote.is_err() {
            let _ = std::fs::remove_file(&ps);
            let _ = std::fs::remove_file(&idx);
            return None; // never an add without its excludes
        }
        let mut from = OsString::from("--pathspec-from-file=");
        from.push(&ps);
        intent.extend([from, "--pathspec-file-nul".into()]);
    }
    run_raw(root, &filters, Some(&idx), &intent);
    let _ = std::fs::remove_file(&ps);
    let ex: Vec<String> = big.iter().map(|b| format!(":(exclude,literal){b}")).collect();
    let diff = |rev: Option<&str>| {
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
            "--numstat",
            "--summary",
        ];
        args.extend(rev);
        if !ex.is_empty() {
            args.push("--");
            args.extend(ex.iter().map(String::as_str));
        }
        run(root, &filters, Some(&idx), &args)
    };
    let (mut ok, mut out) = diff(Some(base.unwrap_or("HEAD")));
    let unborn = || !run(root, &[], None, &["rev-parse", "--verify", "-q", "HEAD"]).0;
    if !ok && base.is_none() && unborn() {
        // tree_diff's no-branch path: the worktree, then the index.
        (ok, out) = diff(None);
        if ok && out.trim().is_empty() {
            (ok, out) = diff(Some("--cached"));
        }
    }
    let _ = std::fs::remove_file(&idx);
    if !ok {
        return None;
    }
    let (mut add, mut del, mut new) = (0u64, 0u64, 0u64);
    for l in out.lines() {
        if l.starts_with(" create mode ") {
            new += 1;
            continue;
        }
        let mut f = l.split('\t');
        add += f.next().and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
        del += f.next().and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
    }
    if out.trim().is_empty() && big.is_empty() {
        return Some((0, 0, 0, "clean".into()));
    }
    let mut pm = format!("+{add} -{del}");
    if new > 0 {
        pm.push_str(&format!("  ?{new} new"));
    }
    Some((add, del, new, pm))
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
