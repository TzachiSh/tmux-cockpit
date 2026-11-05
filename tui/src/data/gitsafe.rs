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
/// in: a dotfiles setup's GIT_DIR, say. GIT_CEILING_DIRECTORIES stays: it can
/// only stop the search lower.
pub const REDIRECT: [&str; 4] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE"];

/// stdout, and whether git exited 0: a failed diff must not read as clean.
fn run(dir: &str, extra: &[String], index: Option<&Path>, args: &[&str]) -> (bool, String) {
    let (ok, out) = run_raw(dir, extra, index, args);
    (ok, String::from_utf8_lossy(&out).into_owned())
}

/// [`run`] with stdout as git wrote it: a path need not be UTF-8.
fn run_raw<S: AsRef<OsStr>>(dir: &str, extra: &[String], index: Option<&Path>, args: &[S]) -> (bool, Vec<u8>) {
    let mut c = Command::new("git");
    for v in REDIRECT {
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
    drop(scratch);
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
