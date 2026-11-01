//! The type-panel port: capture, redaction, the prompt envelope, the `claude -p`
//! request and reply parsing. No view here; the UI lands in the next phase.
pub mod capture;
pub mod envelope;
pub mod parse;
pub mod prompts;
pub mod redact;
pub mod route;

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
