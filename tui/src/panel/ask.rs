//! The viewer's `?` (#365): one line range of one file, read for a question the
//! panel then asks a read-only `claude -p` about. The lines are read here, fresh
//! from disk, with the viewer's own limits: a file the viewer would refuse
//! (binary, over 1 MB, not UTF-8, not a regular file) is refused here too.
use crate::refs::{self, Ref};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// At most this many lines go inline (D8); the model Reads the rest itself.
pub const MAX_LINES: usize = 400;
/// And at most this many bytes of them.
pub const MAX_BYTES: usize = 32 * 1024;
/// The owner's caps for one ask (D7): what an ask may spend, and how long.
pub const BUDGET_USD: f64 = 0.50;
pub const TIMEOUT: Duration = Duration::from_secs(300);

/// What a question is asked about.
#[derive(Clone, Debug, PartialEq)]
pub struct AskCode {
    /// the reference as `refs::render` writes it, relative to `repo`: `src/x.rs:40-72`
    pub at: String,
    /// the same file without its lines: `src/x.rs`
    pub path: String,
    /// the folder the call may read: the git work tree holding the file, else its folder
    pub repo: PathBuf,
    /// the lines inlined, each behind its number, right aligned, secrets hidden
    pub code: String,
    /// how many secrets `code` had hidden
    pub hidden: usize,
    /// the lines asked about, the end held to the file's last
    pub asked: (u32, u32),
    /// the lines inlined: `asked`, or its start when the caps cut it
    pub shown: (u32, u32),
    /// the last line shown was cut short at [`MAX_BYTES`]
    pub cut: bool,
}

/// The work tree holding `file`: the nearest folder above it with a `.git` (a
/// dir, or a worktree's file). Found by looking, never by running git (#428).
/// Never home, a folder above it, or `/`: a `.git` there (dotfiles) is no
/// project, and `--add-dir` of one opens all of home. `None` when the file's own
/// folder is one of them.
pub fn repo_under(file: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new("/"));
    let floor = |d: &Path| d.parent().is_none() || home.is_some_and(|h| h.starts_with(d));
    if floor(dir) {
        return None;
    }
    let mut up = dir.ancestors().take_while(|d| !floor(d));
    Some(up.find(|d| d.join(".git").exists()).unwrap_or(dir).to_path_buf())
}

/// $HOME as the file paths are, canonical.
fn home() -> Option<PathBuf> {
    let h = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)?;
    Some(std::fs::canonicalize(&h).unwrap_or(h))
}

/// The file's name with a control or bidi character, or not UTF-8.
const BAD_NAME: &str = "a control or bidi character, or no UTF-8, in the file's name; refused";

/// Opens a key's armour, and its body lines until the END line close it.
fn pem_edge(l: &str, word: &str) -> bool {
    l.contains(&format!("-----{word} ")) && l.contains("PRIVATE KEY-----")
}

/// Lines `a` to `b` of `file`, for a question.
pub fn load(file: &Path, a: u32, b: u32) -> Result<AskCode, String> {
    load_in(file, a, b, home().as_deref())
}

/// [`load`], with `home` as the home folder. Each line is redacted before it is
/// numbered or cut, so a key at the cap is hidden whole and the ^Z state holds
/// no secret; a key's PEM body is hidden line by line, from the file's start.
pub fn load_in(file: &Path, a: u32, b: u32, home: Option<&Path>) -> Result<AskCode, String> {
    let r = Ref::new(file, Some((a, b)), None).map_err(|e| e.to_string())?;
    // the path goes out as the model's to Read: a cleaned one would name no file
    match r.path().to_str() {
        Some(p) if crate::pick::clean(p) == p => {}
        _ => return Err(BAD_NAME.into()),
    }
    let text = crate::mdview::load(r.path(), crate::mdview::MAX_FILE)?;
    let all: Vec<&str> = text.lines().collect();
    let name = r.path().file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let n = all.len() as u32;
    if a > n {
        return Err(format!("{name} has {n} lines; line {a} is past its end"));
    }
    let b = b.min(n);
    let no_project = || format!("{name} is not in a project folder");
    let repo = repo_under(r.path(), home).ok_or_else(no_project)?;
    super::request::read_dir_ok(&repo)?;
    let render = |lines| -> Result<String, String> {
        let r = Ref::new(r.path(), lines, None).map_err(|e| e.to_string())?;
        Ok(refs::render(&[r], &repo))
    };
    let (at, path) = (render(Some((a, b)))?, render(None)?);
    let w = b.to_string().len();
    let (mut code, mut last, mut hidden) = (String::new(), a - 1, 0);
    let (mut cut, mut pem) = (false, false);
    for (i, l) in all[..b as usize].iter().enumerate() {
        let num = i as u32 + 1;
        let open = pem || pem_edge(l, "BEGIN");
        pem = open && !pem_edge(l, "END");
        if num < a {
            continue;
        }
        let (l, n) = if open {
            (super::redact::HIDDEN.to_string(), 1)
        } else {
            super::redact::redact(l)
        };
        let row = format!("{num:>w$} {l}");
        let sep = usize::from(!code.is_empty());
        if (num - a) as usize >= MAX_LINES || code.len() + sep + row.len() > MAX_BYTES {
            // one line past the cap on its own (minified code) goes in cut
            if code.is_empty() {
                let mut end = MAX_BYTES;
                while !row.is_char_boundary(end) {
                    end -= 1;
                }
                code.push_str(&row[..end]);
                (last, cut) = (num, true);
                hidden += n;
            }
            break;
        }
        hidden += n;
        if sep == 1 {
            code.push('\n');
        }
        code.push_str(&row);
        last = num;
    }
    Ok(AskCode {
        at: at.trim_end().to_string(),
        path: path.trim_end().to_string(),
        repo,
        code,
        hidden,
        asked: (a, b),
        shown: (a, last.max(a)),
        cut,
    })
}
