//! The risky-command matcher, after type-panel's.
use super::{boundary_at, boundary_before, ci};

fn ws(s: &str, i: usize, min: usize) -> Option<usize> {
    let n: usize = s[i..]
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .map(char::len_utf8)
        .sum();
    (s[i..i + n].chars().count() >= min).then_some(i + n)
}

/// `word` with a word boundary after it.
fn word(s: &str, i: usize, lit: &str) -> Option<usize> {
    ci(s, i, lit).filter(|&e| boundary_at(s, e))
}

/// `a ws+ b ws+ c`, the shape of `git push`, `chmod -R` and kin.
fn seq(s: &str, i: usize, parts: &[&str]) -> Option<usize> {
    let mut j = i;
    for (k, p) in parts.iter().enumerate() {
        if k > 0 {
            j = ws(s, j, 1)?;
        }
        j = ci(s, j, p)?;
    }
    Some(j)
}

fn line_end(s: &str, i: usize) -> usize {
    s[i..].find('\n').map_or(s.len(), |n| i + n)
}

fn risky_at(s: &str, i: usize) -> Option<usize> {
    if let Some(e) = ci(s, i, ">").and_then(|j| ws(s, j, 0)).and_then(|j| ci(s, j, "/dev/sd")) {
        return Some(e);
    }
    if !boundary_before(s, i) {
        return None;
    }
    let eol = line_end(s, i);
    let simple = [
        ci(s, i, "rm").and_then(|j| ws(s, j, 1).filter(|&e| e > j)),
        word(s, i, "sudo"),
        ci(s, i, "dd").and_then(|j| ws(s, j, 1)),
        ci(s, i, "mkfs"),
        word(s, i, "shred"),
        seq(s, i, &["chmod", "-r"]),
        seq(s, i, &["chown", "-r"]),
        seq(s, i, &["kill", "-9"]),
        word(s, i, "pkill"),
        word(s, i, "killall"),
        word(s, i, "reboot"),
        seq(s, i, &["git", "reset", "--hard"]),
        seq(s, i, &["git", "clean", "-"]),
        seq(s, i, &["drop", "table"]),
    ];
    if let Some(e) = simple.into_iter().flatten().next() {
        return Some(e);
    }
    if let Some(j) = seq(s, i, &["git", "push"]).and_then(|j| ws(s, j, 1)) {
        let rest = &s[j..eol];
        let force = rest.match_indices("--force").map(|(k, m)| j + k + m.len()).chain(
            rest.match_indices("-f")
                .map(|(k, m)| j + k + m.len())
                .filter(|&e| boundary_at(s, e)),
        );
        if let Some(e) = force.max() {
            return Some(e);
        }
    }
    let fetch = word(s, i, "curl").or_else(|| word(s, i, "wget"))?;
    s[fetch..eol].match_indices('|').rev().find_map(|(k, _)| {
        let j = ws(s, fetch + k + 1, 0)?;
        let j = ci(s, j, "ba").or_else(|| ci(s, j, "z")).unwrap_or(j);
        word(s, j, "sh")
    })
}

/// The first risky fragment of a command, as type-panel flags them.
pub fn risky(cmd: &str) -> Option<String> {
    let mut i = 0;
    while let Some(c) = cmd[i..].chars().next() {
        if let Some(e) = risky_at(cmd, i) {
            return Some(cmd[i..e].trim().to_string());
        }
        i += c.len_utf8();
    }
    None
}
