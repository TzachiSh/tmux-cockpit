//! The tree a pane works in, and where that answer came from (#1343): one rule for
//! the board, `cockpit-tui workdir` and the panel. The contract is the freeze on #1342.
use std::path::Path;

/// How old, in seconds, the newest tree the agent's commands reached may be and still
/// stand for the work folder.
pub const ACTIVE_MAX_AGE: u64 = 600;
/// How many recent trees a [`WorkDir`] keeps.
pub const RECENT_MAX: usize = 3;

/// Which candidate named the work folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Source {
    /// `@cockpit_dir`, set by hand
    Declared,
    /// a tree the agent's recent commands reached
    ActiveIn,
    /// the cwd the hooks or the session transcript saw last
    Session,
    /// pane_current_path, which always answers
    #[default]
    PaneCwd,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Declared => "declared",
            Source::ActiveIn => "active-in",
            Source::Session => "session",
            Source::PaneCwd => "pane-cwd",
        }
    }
}

/// A git toplevel the agent's commands reached, and the seconds since.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recent {
    pub dir: String,
    pub age: u64,
}

/// Every candidate a reader has for one pane. An empty string is no candidate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Candidates {
    /// `@cockpit_dir` when it differs from `@cockpit_cwd`, else empty
    pub declared: String,
    /// `@cockpit_cwd`, else `@cockpit_dir`
    pub hook_cwd: String,
    /// the session transcript's newest cwd
    pub transcript_cwd: String,
    pub active: Vec<Recent>,
    pub pane_cwd: String,
}

impl Candidates {
    /// `declared` and `hook_cwd` off the pane's `@cockpit_dir` and `@cockpit_cwd`, the rest
    /// empty. agent-state writes the two together from the hook's cwd, so a dir that differs
    /// from that cwd was set by hand (docs/review.md).
    pub fn from_options(dir: &str, cwd: &str) -> Candidates {
        Candidates {
            declared: if dir != cwd { dir.to_string() } else { String::new() },
            hook_cwd: if cwd.is_empty() { dir } else { cwd }.to_string(),
            ..Default::default()
        }
    }
}

/// The work folder, its source, the pane's own cwd and the recent trees.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkDir {
    pub dir: String,
    pub source: Source,
    /// pane_current_path, whoever won
    pub cwd: String,
    /// `active`, newest first, at most [`RECENT_MAX`]
    pub recent: Vec<Recent>,
}

/// Only an absolute path naming a directory counts: a relative one would be read
/// against the reader's own cwd, and a dangling one (the tree was removed) is no tree.
fn holds(d: &str) -> bool {
    let p = Path::new(d);
    p.is_absolute() && p.is_dir()
}

/// The first candidate that holds: `declared`, the newest `active` entry younger than
/// [`ACTIVE_MAX_AGE`], `hook_cwd`, `transcript_cwd`; else `pane_cwd`, whatever it is.
/// Forks nothing: every candidate comes off a read the caller already made.
pub fn choose(c: &Candidates) -> WorkDir {
    let mut recent = c.active.clone();
    // stable, so entries of one age keep the caller's order
    recent.sort_by_key(|r| r.age);
    let fresh = recent.first().filter(|r| r.age < ACTIVE_MAX_AGE).map_or("", |r| r.dir.as_str());
    let ranked = [
        (c.declared.as_str(), Source::Declared),
        (fresh, Source::ActiveIn),
        (c.hook_cwd.as_str(), Source::Session),
        (c.transcript_cwd.as_str(), Source::Session),
    ];
    let (dir, source) = ranked.into_iter().find(|(d, _)| holds(d)).unwrap_or((&c.pane_cwd, Source::PaneCwd));
    let dir = dir.to_string();
    recent.truncate(RECENT_MAX);
    WorkDir {
        dir,
        source,
        cwd: c.pane_cwd.clone(),
        recent,
    }
}
