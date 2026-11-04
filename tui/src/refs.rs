//! A code reference: a file, optionally a line range, optionally a symbol (#365).
//!
//! Contract:
//! - `Ref::new` canonicalises the path (it must exist) and keeps its exact bytes, so a
//!   non-UTF-8 name is fine. Lines are 1-based and inclusive, `1 <= a <= b`.
//! - `render` writes one line per ref: `path`, `path:40` (a == b), `path:40-72`, each with
//!   an optional ` (symbol)`. The path is relative to `base` when inside it, absolute
//!   otherwise. Control and bidi characters are dropped from the text only; a `)` in a
//!   symbol becomes `]` and a line break a space. The text is lossy for a non-UTF-8 path.
//! - `parse` is the reverse for text that `render` wrote (or a person typed): relative or
//!   absolute paths, and a line that is no reference, names a missing path or has bad
//!   lines is skipped. A path that itself ends in `:12` or contains ` (` still parses,
//!   since a split that names an existing file wins; if `a` and `a:12` both exist, `a:12`
//!   reads as line 12 of `a`.
use crate::pick::clean;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub path: PathBuf,
    pub lines: Option<(u32, u32)>,
    pub symbol: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RefError {
    /// The path cannot be canonicalised: it does not exist, or is empty.
    NotFound(PathBuf),
    /// Not `1 <= a <= b`.
    Lines(u32, u32),
}

impl fmt::Display for RefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefError::NotFound(p) => write!(f, "no such path: {}", p.display()),
            RefError::Lines(a, b) => write!(f, "bad line range {a}-{b}: need 1 <= a <= b"),
        }
    }
}

impl std::error::Error for RefError {}

impl Ref {
    // Wrapped by hand: the crate's 140-column rustfmt would join it past 100.
    #[rustfmt::skip]
    pub fn new(
        path: impl AsRef<Path>,
        lines: Option<(u32, u32)>,
        symbol: Option<String>,
    ) -> Result<Ref, RefError> {
        let path = path.as_ref();
        if let Some((a, b)) = lines
            && !(1 <= a && a <= b)
        {
            return Err(RefError::Lines(a, b));
        }
        let path = path.canonicalize().map_err(|_| RefError::NotFound(path.to_path_buf()))?;
        Ok(Ref { path, lines, symbol })
    }
}

fn symbol_text(s: &str) -> String {
    let one_line = s.lines().collect::<Vec<_>>().join(" ").replace(')', "]");
    clean(&one_line).trim().to_string()
}

/// One line per ref; see the module contract.
pub fn render(refs: &[Ref], base: &Path) -> String {
    let base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let lines: Vec<String> = refs
        .iter()
        .map(|r| {
            let shown = match r.path.strip_prefix(&base) {
                Ok(rel) if rel.as_os_str().is_empty() => Path::new("."),
                Ok(rel) => rel,
                Err(_) => &r.path,
            };
            let mut s = clean(&shown.to_string_lossy());
            match r.lines {
                Some((a, b)) if a == b => s.push_str(&format!(":{a}")),
                Some((a, b)) => s.push_str(&format!(":{a}-{b}")),
                None => {}
            }
            match r.symbol.as_deref().map(symbol_text) {
                Some(sym) if !sym.is_empty() => s.push_str(&format!(" ({sym})")),
                _ => {}
            }
            s
        })
        .collect();
    lines.join("\n")
}

fn parse_lines(s: &str) -> Option<(u32, u32)> {
    let num = |t: &str| t.bytes().all(|b| b.is_ascii_digit()).then(|| t.parse().ok())?;
    match s.split_once('-') {
        Some((a, b)) => Some((num(a)?, num(b)?)),
        None => num(s).map(|a| (a, a)),
    }
}

fn parse_line(line: &str, base: &Path) -> Option<Ref> {
    // Every place the symbol could start, then every place the lines could start; the
    // first split whose path exists wins.
    let mut ends: Vec<(&str, Option<&str>)> = vec![(line, None)];
    if line.ends_with(')') {
        for (i, _) in line.match_indices(" (") {
            ends.push((&line[..i], Some(&line[i + 2..line.len() - 1])));
        }
    }
    for (head, sym) in ends {
        let mut heads = vec![(head, None)];
        if let Some(i) = head.rfind(':')
            && let Some(l) = parse_lines(&head[i + 1..])
        {
            heads.push((&head[..i], Some(l)));
        }
        for (p, lines) in heads.into_iter().rev() {
            if p.is_empty() {
                continue;
            }
            let path = Path::new(p);
            let full = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };
            if let Ok(r) = Ref::new(full, lines, sym.map(String::from)) {
                return Some(r);
            }
        }
    }
    None
}

/// The reverse of `render`; see the module contract.
pub fn parse(s: &str, base: &Path) -> Vec<Ref> {
    s.lines().filter_map(|l| parse_line(l, base)).collect()
}
