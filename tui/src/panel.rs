//! The type-panel port: capture, redaction, the prompt envelope, the `claude -p`
//! request, reply parsing, and the `cockpit-tui panel %N` screens.
pub mod action;
pub mod app;
pub mod ask;
pub mod capture;
pub mod envelope;
pub mod parse;
pub mod prompts;
pub mod redact;
pub mod request;
pub mod route;
pub mod view;

use std::path::PathBuf;

/// How `cockpit-tui panel %N` starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    Plain,
    /// `--resume <state> --last <how>`
    Resume(String, String),
    /// `--ask <file> <a> <b>` (#365): a question about lines `a` to `b` of the file
    Ask(String, u32, u32),
}

const START_USAGE: &str = concat!(
    "panel takes a pane id, then only --resume <state> --last <how>",
    " or --ask <file> <a> <b>"
);

/// A pane id: `%` and digits.
pub fn is_pane(p: &str) -> bool {
    p.strip_prefix('%')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A line number as the viewer prints it: digits, no leading zero, from 1.
fn line_no(s: &str) -> Option<u32> {
    let digits = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (digits && !s.starts_with('0') && s.len() <= 9).then(|| s.parse().ok()).flatten()
}

/// The pane, then at most one way to start: `--resume <state> --last <how>`, or
/// `--ask <file> <a> <b>` with `1 <= a <= b`.
pub fn start_args(args: &[String]) -> Result<(String, Start), String> {
    let pane = args.first().filter(|p| is_pane(p)).ok_or("panel needs a pane id like %3")?;
    let rest: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    let start = match rest[..] {
        [] => Start::Plain,
        ["--resume", f, "--last", last] => Start::Resume(f.into(), last.into()),
        ["--ask", f, a, b] if !f.is_empty() => match (line_no(a), line_no(b)) {
            (Some(a), Some(b)) if a <= b => Start::Ask(f.into(), a, b),
            _ => {
                return Err(format!(
                    "--ask takes two line numbers, the first not past the second, not [{a} {b}]"
                ));
            }
        },
        _ => return Err(START_USAGE.into()),
    };
    Ok((pane.clone(), start))
}

/// XDG says a relative value is to be ignored, so only an absolute one wins.
pub(crate) fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var(var) {
        Ok(v) if v.starts_with('/') => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(fallback),
    }
}

/// Android under Termux: TERMUX_VERSION is set, or PREFIX is Termux's own tree.
pub fn is_termux() -> bool {
    std::env::var_os("TERMUX_VERSION").is_some_and(|v| !v.is_empty()) || std::env::var("PREFIX").is_ok_and(|p| p.contains("com.termux"))
}

/// `COCKPIT_PANEL_MODE` (from `@cockpit_panel_mode`) as tablet or not: `auto`
/// or unset is tablet under Termux. Any other word is refused, never guessed at.
pub fn tablet_mode(v: Option<&str>, termux: bool) -> Result<bool, String> {
    match v {
        None | Some("" | "auto") => Ok(termux),
        Some("pc") => Ok(false),
        Some("tablet") => Ok(true),
        Some(x) => Err(format!("COCKPIT_PANEL_MODE is {x:?}: use auto, pc or tablet")),
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Python's `\b` on the left: no word char right before `i`.
fn boundary_before(s: &str, i: usize) -> bool {
    !s[..i].chars().next_back().is_some_and(is_word)
}

fn boundary_at(s: &str, i: usize) -> bool {
    !s[i..].chars().next().is_some_and(is_word)
}

fn skip_ws(s: &str, i: usize) -> usize {
    i + s[i..].chars().take_while(|c| c.is_whitespace()).map(char::len_utf8).sum::<usize>()
}

/// ASCII case-insensitive `pat` at `i`, and the index past it; `?` in `pat`
/// stands for an optional `_` or `-`.
fn ci(s: &str, i: usize, pat: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut j = i;
    for p in pat.bytes() {
        if p == b'?' {
            if matches!(b.get(j), Some(b'_' | b'-')) {
                j += 1;
            }
        } else if b.get(j).is_some_and(|c| c.eq_ignore_ascii_case(&p)) {
            j += 1;
        } else {
            return None;
        }
    }
    Some(j)
}
