//! What the info screen shows about one pane (#252): the facts, read once. The
//! git root and the review base are ports of tmux-review's `git_root` and
//! `review_base`, so the screen and `prefix r` name the same tree and base.
use super::gitsafe::git;
use std::path::PathBuf;

/// One transcript row: `doing`, `said` or `asked`, its text, and its age in seconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doing {
    pub kind: String,
    pub text: String,
    pub age: Option<u64>,
}

/// The review base: the rev, the label and the mode that held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Base {
    pub rev: String,
    pub label: String,
    pub mode: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub id: String,
    pub wher: String,
    pub cmd: String,
    pub cwd: String,
    /// `@cockpit_dir` when set
    pub dir: Option<String>,
    pub dir_missing: bool,
    pub root: String,
    pub state: String,
    pub msg: String,
    /// an agent pane: it has the `/btw` pointer, and a transcript to read
    pub agent: bool,
    pub doing: Vec<Doing>,
    pub review: String,
    pub spool: String,
    /// `None` when HEAD does not resolve (no commit yet)
    pub base: Option<Base>,
    pub sid: String,
    pub title: String,
    pub tui: String,
    /// the review dir, its pinned count and its delivered count; `None` when absent
    pub rev_dir: Option<(String, usize, usize)>,
}

/// Control characters out: a transcript, a title and a path are not trusted
/// with the terminal.
pub fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// tmux-review's `git_root`: the worktree's top, else the dir itself.
pub fn git_root(dir: &str) -> String {
    let out = git(dir, &[], None, &["rev-parse", "--show-toplevel"]);
    let out = out.trim_end_matches('\n');
    if out.is_empty() { dir.to_string() } else { out.to_string() }
}

/// POSIX `cksum`: CRC-32 over the bytes, then the length, complemented.
pub fn cksum(data: &[u8]) -> u32 {
    let step = |mut crc: u32, b: u8| {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
        crc
    };
    let mut crc = data.iter().fold(0u32, |c, b| step(c, *b));
    let mut n = data.len();
    while n > 0 {
        crc = step(crc, (n & 0xff) as u8);
        n >>= 8;
    }
    !crc
}

/// tmux-review's `session_dir`: keyed on the pane id and the root.
pub fn session_dir(id: &str, root: &str) -> PathBuf {
    let state = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}/.local/state", std::env::var("HOME").unwrap_or_default()));
    let n = cksum(root.as_bytes());
    PathBuf::from(format!("{state}/tmux-cockpit/review/{}-{n}", id.trim_start_matches('%')))
}

/// review-diff.sh's `review_base`, for `dir`: the first case that holds among
/// since-review, branch and uncommitted, starting at `mode`. `br` and `head`
/// are the branch and its sha when the caller already has them.
///
/// The upstream, else origin/HEAD's target, else main, else master: the shell
/// and this agree, so the screen names the base `prefix r` diffs against.
pub fn review_base(dir: &str, mode: &str, br: &str, head: &str) -> Base {
    let mode = if mode.is_empty() { "auto" } else { mode };
    let mut br = br.to_string();
    if br.is_empty() {
        br = git(dir, &[], None, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim_end_matches('\n')
            .to_string();
    }
    let mut head = head.to_string();
    let rname = format!("refs/cockpit/reviewed/{}", if br.is_empty() { "HEAD" } else { &br });
    let bref = format!("refs/heads/{br}");
    let rows = git(
        dir,
        &[],
        None,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)%09%(upstream:short)%09%(upstream:track)%09%(symref:short)",
            "--",
            &rname,
            &bref,
            "refs/remotes/origin/HEAD",
            "refs/heads/main",
            "refs/heads/master",
        ],
    );
    let (mut ckpt, mut bups, mut btrack) = (String::new(), String::new(), String::new());
    let (mut mainsha, mut mastersha) = (String::new(), String::new());
    let (mut ohead, mut osha) = (String::new(), String::new());
    for l in rows.lines() {
        let mut f = l.split('\t');
        let (name, sha) = (f.next().unwrap_or(""), f.next().unwrap_or("").to_string());
        let (ups, track) = (f.next().unwrap_or("").to_string(), f.next().unwrap_or("").to_string());
        let sym = f.next().unwrap_or("").to_string();
        if name == rname {
            ckpt = sha;
        } else if name == bref {
            if head.is_empty() {
                head = sha;
            }
            (bups, btrack) = (ups, track);
        } else if name == "refs/remotes/origin/HEAD" {
            (ohead, osha) = (sym, sha);
        } else if name == "refs/heads/main" {
            mainsha = sha;
        } else if name == "refs/heads/master" {
            mastersha = sha;
        }
    }
    let held = |rev: &str, label: &str, mode: &str| Base {
        rev: rev.into(),
        label: label.into(),
        mode: mode.into(),
    };
    if matches!(mode, "auto" | "since-review") && !ckpt.is_empty() {
        return held(&ckpt, "since last review", "since-review");
    }
    if matches!(mode, "auto" | "since-review" | "branch") {
        let (up, upsha) = if !bups.is_empty() && btrack != "[gone]" {
            let s = if btrack.contains("ahead") { String::new() } else { head.clone() };
            (bups.clone(), s)
        } else if !ohead.is_empty() {
            (ohead, osha)
        } else if !mainsha.is_empty() {
            ("main".to_string(), mainsha)
        } else if !mastersha.is_empty() {
            ("master".to_string(), mastersha)
        } else {
            (String::new(), String::new())
        };
        if !up.is_empty() && (head.is_empty() || upsha != head) {
            let mb = git(dir, &[], None, &["merge-base", &up, "HEAD"]).trim_end_matches('\n').to_string();
            if head.is_empty() {
                head = git(dir, &[], None, &["rev-parse", "HEAD"]).trim_end_matches('\n').to_string();
            }
            if !mb.is_empty() && mb != head {
                return held(&mb, &format!("vs {up}"), "branch");
            }
        }
    }
    held("HEAD", "uncommitted", "uncommitted")
}

/// `1.2.3`, the installed binary's version and path, and the version
/// `@cockpit_tui_version` asks for when it differs (prefix U updates it).
fn tui_row(want: &str) -> String {
    let have = env!("CARGO_PKG_VERSION");
    let bin = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    let want = clean(want);
    let want = want.trim();
    let want = want.strip_prefix("cockpit-tui-").unwrap_or(want);
    let want = want.strip_prefix('v').unwrap_or(want);
    if !want.is_empty() && want != have {
        format!("{have}  {bin}  (want {want} -- prefix U updates it)")
    } else {
        format!("{have}  {bin}")
    }
}

fn count(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path).map_or(0, |s| s.lines().filter(|l| l.starts_with("## ")).count())
}

const SEP: char = '\u{1f}';

/// Every fact for `pane`, in one tmux read. `Err` names a pane that is not
/// there: display-message exits 0 and expands to nothing for a missing id, so
/// the resolved id is the test, not the status.
pub fn gather(pane: &str) -> Result<Facts, String> {
    // A US byte inside a title, a message or a window name would shift every
    // later field: free-form values are blanked in the format, as data.rs FIELDS does.
    let q = |f: &str| format!("#{{s/{SEP}/ /:{f}}}");
    let fmt = [
        "#{pane_id}".to_string(),
        format!("{}:#{{window_index}} {}", q("#{session_name}"), q("#{window_name}")),
        q("#{pane_current_command}"),
        q("#{pane_current_path}"),
        q("#{@cockpit_dir}"),
        q("#{@panestate}"),
        q("#{@panestate_msg}"),
        q("#{@cockpit_review}"),
        q("#{@cockpit_review_spool}"),
        q("#{@cockpit_session}"),
        q("#{@cockpit_title}"),
        "#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},1,0}".to_string(),
        q("#{@cockpit_tui_version}"),
    ]
    .join(&SEP.to_string());
    let out = super::tmux(&["display-message", "-p", "-t", pane, &fmt]).unwrap_or_default();
    let f: Vec<&str> = out.trim_end_matches('\n').split(SEP).collect();
    if f.len() < 13 || f[0].is_empty() {
        return Err(format!("no such pane {pane}"));
    }
    let mut x = Facts {
        id: f[0].into(),
        wher: clean(f[1]),
        cmd: clean(f[2]),
        cwd: clean(f[3]),
        state: clean(f[5]),
        msg: clean(f[6]),
        review: clean(f[7]),
        spool: clean(f[8]),
        sid: clean(f[9]),
        title: clean(f[10]),
        agent: f[11] == "1",
        tui: tui_row(f[12]),
        ..Default::default()
    };
    let mut path = f[3].to_string();
    if !f[4].is_empty() {
        x.dir = Some(clean(f[4]));
        if std::path::Path::new(f[4]).is_dir() {
            path = f[4].to_string();
        } else {
            x.dir_missing = true;
        }
    }
    // No cwd at all: git_root("") would describe the caller's repo, not this pane's.
    let root = if path.is_empty() { String::new() } else { git_root(&path) };
    x.root = clean(&root);
    if x.agent {
        x.doing = doing_rows(&x.id);
    }
    let sess = session_dir(&x.id, &root);
    if !root.is_empty() && !git(&root, &[], None, &["rev-parse", "-q", "--verify", "HEAD"]).trim().is_empty() {
        let chosen = std::fs::read_to_string(sess.join("comments.md.base")).unwrap_or_default();
        x.base = Some(review_base(&root, chosen.trim_end_matches('\n'), "", ""));
    }
    if sess.is_dir() {
        let n = (count(&sess.join("comments.md")), count(&sess.join("delivered.md")));
        x.rev_dir = Some((clean(&sess.display().to_string()), n.0, n.1));
    }
    Ok(x)
}

/// The transcript's doing / said / asked, left out quietly when unreadable.
fn doing_rows(pane: &str) -> Vec<Doing> {
    let Some(text) = super::transcript::file(pane).and_then(|p| super::transcript::doing(&p)) else {
        return vec![];
    };
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (kind, a) = (f.next()?, clean(f.next()?));
            let age = f.next().and_then(|s| s.parse().ok());
            matches!(kind, "doing" | "said" | "asked").then(|| Doing {
                kind: kind.into(),
                text: a,
                age: if kind == "said" { None } else { age },
            })
        })
        .collect()
}
