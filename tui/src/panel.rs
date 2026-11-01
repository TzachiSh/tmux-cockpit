//! The type-panel port: capture, redaction, the prompt envelope, the `claude -p`
//! request, reply parsing, and the `cockpit-tui panel %N` screens.
pub mod app;
pub mod capture;
pub mod envelope;
pub mod parse;
pub mod prompts;
pub mod redact;
pub mod request;
pub mod route;
pub mod view;

use std::path::PathBuf;

/// XDG says a relative value is to be ignored, so only an absolute one wins.
fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var(var) {
        Ok(v) if v.starts_with('/') => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(fallback),
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
