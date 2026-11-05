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
    /// the lines inlined, each behind its number, right aligned
    pub code: String,
    /// the lines asked about, the end held to the file's last
    pub asked: (u32, u32),
    /// the lines inlined: `asked`, or its start when the caps cut it
    pub shown: (u32, u32),
}

/// The work tree holding `file`: the nearest folder above it with a `.git` (a
/// dir, or a worktree's file). Found by looking, never by running git (#428).
pub fn repo_of(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("/"));
    dir.ancestors().find(|d| d.join(".git").exists()).unwrap_or(dir).to_path_buf()
}

/// Lines `a` to `b` of `file`, for a question.
pub fn load(file: &Path, a: u32, b: u32) -> Result<AskCode, String> {
    let r = Ref::new(file, Some((a, b)), None).map_err(|e| e.to_string())?;
    let text = crate::mdview::load(r.path(), crate::mdview::MAX_FILE)?;
    let all: Vec<&str> = text.lines().collect();
    let name = r.path().file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let n = all.len() as u32;
    if a > n {
        return Err(format!("{name} has {n} lines; line {a} is past its end"));
    }
    let b = b.min(n);
    let repo = repo_of(r.path());
    let at = refs::render(&[Ref::new(r.path(), Some((a, b)), None).map_err(|e| e.to_string())?], &repo);
    let path = refs::render(&[Ref::new(r.path(), None, None).map_err(|e| e.to_string())?], &repo);
    let w = b.to_string().len();
    let (mut code, mut last) = (String::new(), a - 1);
    for (i, l) in all[a as usize - 1..b as usize].iter().enumerate() {
        let num = a + i as u32;
        let row = format!("{num:>w$} {l}");
        let sep = usize::from(!code.is_empty());
        if i >= MAX_LINES || code.len() + sep + row.len() > MAX_BYTES {
            // one line past the cap on its own (minified code) goes in cut
            if code.is_empty() {
                let mut end = MAX_BYTES;
                while !row.is_char_boundary(end) {
                    end -= 1;
                }
                code.push_str(&row[..end]);
                last = num;
            }
            break;
        }
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
        asked: (a, b),
        shown: (a, last.max(a)),
    })
}
