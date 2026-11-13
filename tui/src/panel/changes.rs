//! What changed in a repo since the user last looked (#1000, task 2 of #935):
//! commits from git, PRs, issues and the branch's latest CI run from `gh`, as
//! a list built in plain code, so the model that explains it cannot make an
//! item up. Unwired: nothing in the panel calls it yet.
//!
//! - **Last seen.** Per repo, the HEAD sha and a time in seconds, in
//!   `$XDG_CACHE_HOME/tmux-cockpit/panel/seen/<key>`, mode 600 in a mode 700
//!   folder. `<key>` is [`repo_key`], a hash of the repo's absolute top-level
//!   path; the file holds no path, subject or title. A file that is missing,
//!   unreadable or malformed is nothing seen. Only [`mark_seen`] writes it;
//!   [`Collector::collect`] writes nothing.
//! - **Citations.** Every [`Item`] cites a 12-digit short sha, `#N` or a run
//!   id; one that does not is dropped, never shown.
//! - **Outside text.** Commit subjects, titles and workflow names go through
//!   [`redact_clean`] before they reach an item.
//! - **Quiet.** A failed git or gh call leaves its items out and adds one
//!   [`Note`]; nothing is printed and nothing panics.
use super::capture::{self, Ran};
use super::redact::redact_clean;
use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::fs::{DirBuilder, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Items shown per kind, newest first; the rest are counted as "and K more".
/// The owner may change it.
pub const CAP: usize = 20;
/// The shortest time between two gh refreshes of one repo, in seconds.
pub const REFRESH_SECS: u64 = 60;
/// GitHub search calls allowed in any 60 seconds, across repos.
pub const SEARCHES_PER_MINUTE: usize = 30;
/// A refresh searches twice: PRs and issues. The CI run is no search.
const SEARCHES_PER_REFRESH: usize = 2;
/// PRs or issues asked of gh at once; this many back counts as "K or more".
pub const GH_LIMIT: usize = 100;
/// Every git call of one collect, together.
pub const GIT_BUDGET: Duration = Duration::from_secs(1);
/// The gh calls of one refresh, run at once, together.
pub const GH_BUDGET: Duration = Duration::from_secs(5);
/// An item's text past this many chars is cut, after redaction.
const TEXT_MAX: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Commit,
    /// The last-seen sha is no longer behind HEAD: one item in place of commits.
    Rewritten,
    Pr,
    Issue,
    Ci,
}

const KINDS: [Kind; 5] = [Kind::Commit, Kind::Rewritten, Kind::Pr, Kind::Issue, Kind::Ci];

/// One cited fact. `cite` is 12 hex digits for a commit or a rewrite, `#N`
/// for a PR or an issue, a run id's digits for CI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    pub cite: String,
    pub text: String,
}

/// How many of a kind were left out past [`CAP`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum More {
    Exactly(usize),
    /// gh returned [`GH_LIMIT`]: there may be more than this.
    AtLeast(usize),
}

/// Why part of the list is missing: one quiet line each, never an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    NotARepo,
    GitTimedOut,
    /// origin is not a github.com repo, so gh is never started.
    NoGithubRemote,
    NoGh,
    /// gh exited 4, its "authentication required".
    NotLoggedIn,
    GhTimedOut,
    GhSilent,
    /// Not JSON, or JSON of the wrong shape.
    GhUnreadable,
    GhFailed,
    /// This repo was refreshed under [`REFRESH_SECS`] ago with another last-seen time.
    Floor {
        again_in: u64,
    },
    /// [`SEARCHES_PER_MINUTE`] spent.
    SearchLimit {
        again_in: u64,
    },
}

impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let gh = |f: &mut std::fmt::Formatter, why: &str| write!(f, "GitHub: {why}, left out");
        match self {
            Note::NotARepo => f.write_str("changes: not in a git work tree"),
            Note::GitTimedOut => f.write_str("git: took too long, commits left out"),
            Note::NoGithubRemote => gh(f, "origin is not a github.com repo"),
            Note::NoGh => gh(f, "no gh on PATH"),
            Note::NotLoggedIn => gh(f, "gh is not logged in"),
            Note::GhTimedOut => gh(f, "gh took too long"),
            Note::GhSilent => gh(f, "gh printed nothing"),
            Note::GhUnreadable => gh(f, "gh printed something unreadable"),
            Note::GhFailed => gh(f, "gh failed"),
            Note::Floor { again_in } => {
                write!(f, "GitHub: checked under {REFRESH_SECS} s ago, again in {again_in} s")
            }
            Note::SearchLimit { again_in } => {
                let used = SEARCHES_PER_MINUTE;
                write!(f, "GitHub: {used} searches a minute used, again in {again_in} s")
            }
        }
    }
}

/// What the last-seen file holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub head: String,
    /// Seconds since the epoch.
    pub at: u64,
}

/// One collect's answer. `key` and `head` are what [`mark_seen`] takes once
/// the user has seen the list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    pub key: Option<String>,
    pub head: Option<String>,
    pub seen: Option<Seen>,
    pub items: Vec<Item>,
    pub more: Vec<(Kind, More)>,
    pub notes: Vec<Note>,
}

impl Changes {
    /// One line per item, each kind's "and K more" after its items, then the notes.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![];
        for k in KINDS {
            for i in self.items.iter().filter(|i| i.kind == k) {
                out.push(match k {
                    Kind::Commit | Kind::Rewritten => format!("{} {}", i.cite, i.text),
                    Kind::Pr => format!("PR {} {}", i.cite, i.text),
                    Kind::Issue => format!("issue {} {}", i.cite, i.text),
                    Kind::Ci => format!("CI run {} {}", i.cite, i.text),
                });
            }
            for (_, m) in self.more.iter().filter(|(kind, _)| *kind == k) {
                out.push(match m {
                    More::Exactly(n) => format!("and {n} more"),
                    More::AtLeast(n) => format!("and {n} or more"),
                });
            }
        }
        out.extend(self.notes.iter().map(Note::to_string));
        out
    }

    /// `item` if it carries its kind's citation; dropped otherwise.
    fn push(&mut self, item: Item) {
        if cited(item.kind, &item.cite) {
            self.items.push(item);
        }
    }
}

/// A short sha, `#N` or a run id, as `kind` needs.
fn cited(kind: Kind, cite: &str) -> bool {
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let digits = |s: &str, max: usize| (1..=max).contains(&s.len()) && all_digits(s);
    match kind {
        Kind::Commit | Kind::Rewritten => cite.len() == 12 && is_hex(cite),
        Kind::Pr | Kind::Issue => {
            let n = cite.strip_prefix('#').unwrap_or_default();
            digits(n, 10) && !n.starts_with('0')
        }
        Kind::Ci => digits(cite, 20) && !cite.starts_with('0'),
    }
}

fn is_hex(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A full object name, SHA-1 or SHA-256, lowercase.
fn is_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && is_hex(s)
}

/// Outside text as an item shows it: redacted, then one line, then cut.
/// Redaction comes first, so a cut never splits a key into a run too short to match.
fn clean(s: &str) -> String {
    let (r, _) = redact_clean(s);
    let one: String = r.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let one = one.trim();
    match one.char_indices().nth(TEXT_MAX) {
        Some((i, _)) => format!("{}…", &one[..i]),
        None => one.to_string(),
    }
}

/// FNV-1a, 64 bits, of the canonical top-level path, as 16 hex digits: the
/// last-seen file's name. Stable across builds, which std's hasher is not.
pub fn repo_key(top: &str) -> String {
    let path = std::fs::canonicalize(top)
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|_| top.into());
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.as_encoded_bytes() {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// `$XDG_CACHE_HOME/tmux-cockpit/panel/seen`, or `~/.cache/...` without it.
pub fn seen_dir() -> PathBuf {
    super::xdg("XDG_CACHE_HOME", ".cache").join("tmux-cockpit/panel/seen")
}

fn is_key(key: &str) -> bool {
    key.len() == 16 && is_hex(key)
}

/// The last-seen record for `key` under `store`; `None` for a missing,
/// unreadable or malformed file. Reads at most 200 bytes.
pub fn read_seen(store: &Path, key: &str) -> Option<Seen> {
    if !is_key(key) {
        return None;
    }
    let mut b = Vec::new();
    std::fs::File::open(store.join(key)).ok()?.take(200).read_to_end(&mut b).ok()?;
    let s = std::str::from_utf8(&b).ok()?.strip_suffix('\n')?;
    let (head, at) = s.split_once(' ')?;
    let at = at.bytes().all(|c| c.is_ascii_digit()).then(|| at.parse().ok()).flatten()?;
    is_sha(head).then(|| Seen { head: head.into(), at })
}

/// Records that the user saw the repo `key` at `head`, at `at` seconds: the
/// one write this module makes. Mode 600, replaced whole by a rename.
pub fn mark_seen(store: &Path, key: &str, head: &str, at: u64) -> std::io::Result<()> {
    let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, what.to_string());
    if !is_key(key) {
        return Err(bad("key must be 16 lowercase hex digits"));
    }
    if !is_sha(head) {
        return Err(bad("head must be a full lowercase sha"));
    }
    DirBuilder::new().recursive(true).mode(0o700).create(store)?;
    // Threads of one process share the pid, so the counter keeps their temp names apart.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = store.join(format!(".{key}.{}.{seq}", std::process::id()));
    let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    let done = f
        .write_all(format!("{head} {at}\n").as_bytes())
        .and_then(|()| std::fs::rename(&tmp, store.join(key)));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// `owner/repo` of a github.com remote URL: https, ssh or scp-like, with or
/// without `.git`. `None` for any other host or shape.
pub fn github_repo(url: &str) -> Option<String> {
    let rest = ["https://github.com/", "ssh://git@github.com/", "git@github.com:"]
        .iter()
        .find_map(|p| url.trim().strip_prefix(p))?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, repo) = rest.split_once('/')?;
    let byte = |b: u8| b.is_ascii_alphanumeric() || b"-_.".contains(&b);
    let part = |s: &str| !s.is_empty() && !s.starts_with(['.', '-']) && s.bytes().all(byte);
    (part(owner) && part(repo)).then(|| format!("{owner}/{repo}"))
}

/// `secs` as `2026-11-13T10:00:00Z`, the form a GitHub search takes.
pub fn utc(secs: u64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = (secs / 86_400) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let t = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t % 3600 / 60, t % 60)
}

/// How gh is started. The panel's is `gh` found by PATH; a test's is its own
/// stand-in by full path, with the PATH the spawn sees set to the rig's.
#[derive(Clone, Debug)]
pub struct Gh {
    pub program: OsString,
    /// The PATH the spawn sees; `None` keeps the panel's.
    pub path: Option<OsString>,
    pub budget: Duration,
}

impl Gh {
    pub fn from_env() -> Gh {
        Gh {
            program: "gh".into(),
            path: None,
            budget: GH_BUDGET,
        }
    }

    /// `gh <args> -R github.com/<repo>`, from `/`, so it never runs git in the
    /// user's repo, whose config could run code. No prompt, no update check.
    fn spawn(&self, args: &[&str], repo: &str) -> std::io::Result<capture::Git> {
        let mut c = Command::new(&self.program);
        c.args(args)
            .arg("-R")
            .arg(format!("github.com/{repo}"))
            .current_dir("/")
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env("NO_COLOR", "1")
            .env_remove("GH_REPO");
        if let Some(p) = &self.path {
            c.env("PATH", p);
        }
        capture::bounded(c)
    }
}

/// A refresh's GitHub part, kept for [`REFRESH_SECS`].
#[derive(Clone)]
struct Fetched {
    at: u64,
    since: Option<u64>,
    items: Vec<Item>,
    more: Vec<(Kind, More)>,
    note: Option<Note>,
}

/// Keeps the refresh limits between calls: the panel holds one for its life.
/// The limits are this process's; a new panel starts with none spent.
pub struct Collector {
    pub gh: Gh,
    pub git_budget: Duration,
    /// Where the last-seen files are: [`seen_dir`] in the panel.
    pub store: PathBuf,
    /// Seconds since the epoch; a test's own clock.
    clock: Box<dyn Fn() -> u64 + Send>,
    fetched: HashMap<String, Fetched>,
    searches: VecDeque<u64>,
}

impl Collector {
    pub fn new(gh: Gh, store: PathBuf, clock: Box<dyn Fn() -> u64 + Send>) -> Collector {
        Collector {
            gh,
            git_budget: GIT_BUDGET,
            store,
            clock,
            fetched: HashMap::new(),
            searches: VecDeque::new(),
        }
    }

    pub fn from_env() -> Collector {
        let now = || {
            let since = SystemTime::now().duration_since(UNIX_EPOCH);
            since.map(|d| d.as_secs()).unwrap_or(0)
        };
        Collector::new(Gh::from_env(), seen_dir(), Box::new(now))
    }

    /// The list for the repo `dir` is in; `dir` must be absolute, as `git -C ""`
    /// would read the cwd's repo. Blocks for at most the git budget plus the gh
    /// budget, so the panel calls it off its drawing thread.
    pub fn collect(&mut self, dir: &str) -> Changes {
        let mut c = Changes::default();
        let end = Instant::now() + self.git_budget;
        let top = Path::new(dir)
            .is_absolute()
            .then(|| capture::toplevel(dir, self.git_budget))
            .flatten();
        let Some(top) = top else {
            c.notes.push(Note::NotARepo);
            return c;
        };
        let key = repo_key(&top);
        c.seen = read_seen(&self.store, &key);
        let head = capture::spawn_git(&top, &[], &["rev-parse", "--verify", "-q", "HEAD"]);
        let branch = capture::spawn_git(&top, &[], &["symbolic-ref", "-q", "--short", "HEAD"]);
        let url = capture::spawn_git(&top, &[], &["config", "--get", "remote.origin.url"]);
        let wait = |g| capture::wait(g, end);
        let (head, branch, url) = (wait(head), wait(branch), wait(url));
        match head {
            Ran::TimedOut => c.notes.push(Note::GitTimedOut),
            Ran::Ok(h) if is_sha(h.trim()) => {
                c.head = Some(h.trim().to_string());
                commits(&mut c, &top, end);
            }
            _ => {} // no commits yet
        }
        c.key = Some(key.clone());
        let Some(repo) = url.ok().as_deref().and_then(github_repo) else {
            c.notes.push(Note::NoGithubRemote);
            return c;
        };
        let branch = branch.ok().map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
        let f = self.github(&key, &repo, branch.as_deref(), c.seen.as_ref().map(|s| s.at));
        c.items.extend(f.items);
        c.more.extend(f.more);
        c.notes.extend(f.note);
        c
    }

    /// The GitHub part: the one kept from under [`REFRESH_SECS`] ago when its
    /// `since` matches, else a refresh if the floor and the search limit allow.
    fn github(&mut self, key: &str, repo: &str, br: Option<&str>, since: Option<u64>) -> Fetched {
        let now = (self.clock)();
        let mut f = Fetched {
            at: now,
            since,
            items: vec![],
            more: vec![],
            note: None,
        };
        if let Some(old) = self.fetched.get(key) {
            let age = now.saturating_sub(old.at);
            if age < REFRESH_SECS {
                if old.since == since {
                    return old.clone();
                }
                f.note = Some(Note::Floor {
                    again_in: REFRESH_SECS - age,
                });
                return f;
            }
        }
        // a stamp ahead of a clock that went back stays: the limit errs toward fewer calls
        self.searches.retain(|&t| now.saturating_sub(t) < 60);
        if self.searches.len() + SEARCHES_PER_REFRESH > SEARCHES_PER_MINUTE {
            let oldest = self.searches.front().copied().unwrap_or(now);
            f.note = Some(Note::SearchLimit {
                again_in: 60 - now.saturating_sub(oldest),
            });
            return f;
        }
        self.searches.extend([now; SEARCHES_PER_REFRESH]);
        refresh(&self.gh, &mut f, repo, br, since);
        self.fetched.insert(key.to_string(), f.clone());
        f
    }
}

/// The commits since the last-seen sha, or one [`Kind::Rewritten`] item when
/// that sha is no longer behind HEAD.
fn commits(c: &mut Changes, top: &str, end: Instant) {
    let head = c.head.clone().unwrap_or_default();
    let range = match &c.seen {
        Some(s) if s.head == head => return,
        Some(s) => {
            let args = ["merge-base", "--is-ancestor", &s.head, &head];
            let anc = capture::spawn_git(top, &[], &args);
            match capture::wait(anc, end) {
                Ran::Ok(_) => format!("{}..{head}", s.head),
                Ran::TimedOut => return c.notes.push(Note::GitTimedOut),
                Ran::Failed(_) => {
                    let text = format!("history rewritten since {}", utc(s.at));
                    let cite = s.head[..12].to_string();
                    return c.push(Item {
                        kind: Kind::Rewritten,
                        cite,
                        text,
                    });
                }
            }
        }
        None => head,
    };
    let n = format!("--max-count={}", CAP + 1);
    let args = ["log", "-z", "--no-decorate", "--format=%H%x1f%s", &n, &range];
    let log = capture::spawn_git(top, &[], &args);
    let count = capture::spawn_git(top, &[], &["rev-list", "--count", &range]);
    let (log, count) = (capture::wait(log, end), capture::wait(count, end));
    let log = match log {
        Ran::Ok(s) => s,
        Ran::TimedOut => return c.notes.push(Note::GitTimedOut),
        Ran::Failed(_) => return,
    };
    let all: Vec<Item> = log
        .split('\0')
        .filter_map(|r| r.trim_start_matches('\n').split_once('\x1f'))
        .map(|(sha, subject)| Item {
            kind: Kind::Commit,
            cite: sha.get(..12).unwrap_or_default().to_string(),
            text: clean(subject),
        })
        .collect();
    let listed = all.len();
    for i in all.into_iter().take(CAP) {
        c.push(i);
    }
    let total = count.ok().and_then(|n| n.trim().parse::<usize>().ok());
    match total {
        Some(t) if t > CAP => c.more.push((Kind::Commit, More::Exactly(t - CAP))),
        None if listed > CAP => c.more.push((Kind::Commit, More::AtLeast(1))),
        _ => {}
    }
}

/// The three gh calls, started at once under one deadline; the first failure
/// in order is the one note.
fn refresh(gh: &Gh, f: &mut Fetched, repo: &str, branch: Option<&str>, since: Option<u64>) {
    let query = match since {
        Some(t) => format!("updated:>={} sort:updated-desc", utc(t)),
        None => "sort:updated-desc".to_string(),
    };
    let limit = GH_LIMIT.to_string();
    let list = |what: &str| {
        let head = [what, "list", "--state", "all", "--limit", &limit];
        let args = [&head[..], &["--search", &query, "--json", "number,title,updatedAt"]].concat();
        gh.spawn(&args, repo)
    };
    let (prs, issues) = (list("pr"), list("issue"));
    let fields = "databaseId,workflowName,status,conclusion";
    let run_args = |b| ["run", "list", "--branch", b, "--limit", "1", "--json", fields];
    let run = branch.map(|b| gh.spawn(&run_args(b), repo));
    let end = Instant::now() + gh.budget;
    let mut notes = vec![];
    let mut read = |g| match outcome(g, end) {
        Ok(v) => Some(v),
        Err(n) => {
            notes.push(n);
            None
        }
    };
    let (prs, issues) = (read(prs), read(issues));
    let run = run.and_then(&mut read);
    let mut c = Changes::default();
    numbered(&mut c, Kind::Pr, prs);
    numbered(&mut c, Kind::Issue, issues);
    if let Some(r) = run.as_ref().and_then(|v| v.as_array()).and_then(|a| a.first()) {
        let s = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or("");
        let state = if s("conclusion").is_empty() { s("status") } else { s("conclusion") };
        let cite = r
            .get("databaseId")
            .and_then(|v| v.as_u64())
            .map(|n| n.to_string())
            .unwrap_or_default();
        c.push(Item {
            kind: Kind::Ci,
            cite,
            text: clean(&format!("{}: {state}", s("workflowName"))),
        });
    }
    f.items = c.items;
    f.more = c.more;
    let first = |n: &Note| notes.contains(n).then(|| n.clone());
    f.note = first(&Note::NoGh).or_else(|| notes.first().cloned());
}

/// What one gh call gave: a JSON array, or the note saying why not.
fn outcome(g: std::io::Result<capture::Git>, end: Instant) -> Result<serde_json::Value, Note> {
    let g = g.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Note::NoGh
        } else {
            Note::GhFailed
        }
    })?;
    match capture::wait(Some(g), end) {
        Ran::TimedOut => Err(Note::GhTimedOut),
        Ran::Failed(Some(4)) => Err(Note::NotLoggedIn),
        Ran::Failed(_) => Err(Note::GhFailed),
        Ran::Ok(s) if s.trim().is_empty() => Err(Note::GhSilent),
        Ran::Ok(s) => serde_json::from_str::<serde_json::Value>(&s)
            .ok()
            .filter(|v| v.is_array())
            .ok_or(Note::GhUnreadable),
    }
}

/// PRs or issues from gh's array, newest update first, at most [`CAP`].
fn numbered(c: &mut Changes, kind: Kind, v: Option<serde_json::Value>) {
    let Some(serde_json::Value::Array(rows)) = v else {
        return;
    };
    let mut all: Vec<(String, Item)> = rows
        .iter()
        .map(|r| {
            let s = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let cite = r
                .get("number")
                .and_then(|v| v.as_u64())
                .map(|n| format!("#{n}"))
                .unwrap_or_default();
            (
                s("updatedAt"),
                Item {
                    kind,
                    cite,
                    text: clean(&s("title")),
                },
            )
        })
        .filter(|(_, i)| cited(kind, &i.cite))
        .collect();
    all.sort_by(|a, b| b.0.cmp(&a.0));
    let total = all.len();
    for (_, i) in all.into_iter().take(CAP) {
        c.push(i);
    }
    if rows.len() >= GH_LIMIT {
        c.more.push((kind, More::AtLeast(total.saturating_sub(CAP))));
    } else if total > CAP {
        c.more.push((kind, More::Exactly(total - CAP)));
    }
}
