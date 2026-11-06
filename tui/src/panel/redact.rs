//! Secrets blanked before a screen leaves the box. Hand-matched so the crate
//! takes no regex dependency; each matcher mirrors one of type-panel's patterns.
use super::{boundary_at, boundary_before, ci, skip_ws};

pub const HIDDEN: &str = "[hidden]";

/// A match at `i`: the text in `i..keep` survives, `keep..end` becomes [`HIDDEN`].
type Matcher = fn(&str, usize) -> Option<(usize, usize)>;

/// One pass per pattern, in order, like successive `re.subn` calls: a later
/// pattern sees the earlier markers, which is why `sk-ant-` goes before `sk-`.
const PASSES: [Matcher; 9] = [pem, sk_ant, sk, github, akia, jwt, slack, coder, key_value];

/// The text with every secret replaced, and how many were.
pub fn redact(text: &str) -> (String, usize) {
    let mut n = 0;
    let mut s = text.to_string();
    for m in PASSES {
        s = pass(&s, &mut n, m);
    }
    (s, n)
}

fn pass(s: &str, n: &mut usize, m: Matcher) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(c) = s[i..].chars().next() {
        if let Some((keep, end)) = m(s, i) {
            out.push_str(&s[i..keep]);
            out.push_str(HIDDEN);
            *n += 1;
            i = end;
        } else {
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn run(s: &str, i: usize, f: impl Fn(u8) -> bool) -> usize {
    s.as_bytes()[i..].iter().take_while(|b| f(**b)).count()
}

fn tok(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// `\bPREFIX[class]{min,}`
fn prefixed(s: &str, i: usize, prefix: &str, class: fn(u8) -> bool, min: usize) -> Option<(usize, usize)> {
    if !s[i..].starts_with(prefix) || !boundary_before(s, i) {
        return None;
    }
    let j = i + prefix.len();
    let r = run(s, j, class);
    (r >= min).then_some((i, j + r))
}

/// `-----BEGIN [A-Z ]*PRIVATE KEY-----` up to its END line, or to the end of
/// the text when the block is cut off: half a key is still a key.
fn pem(s: &str, i: usize) -> Option<(usize, usize)> {
    let armour = |at: usize, word: &str| -> Option<usize> {
        let j = at + 5 + word.len() + 1;
        if !s[at..].starts_with("-----") || !s[at + 5..].starts_with(word) || !s[j - 1..].starts_with(' ') {
            return None;
        }
        let r = run(s, j, |b| b.is_ascii_uppercase() || b == b' ');
        (s[j..j + r].ends_with("PRIVATE KEY") && s[j + r..].starts_with("-----")).then_some(j + r + 5)
    };
    let body = armour(i, "BEGIN")?;
    let end = s[body..]
        .match_indices("-----END ")
        .find_map(|(k, _)| armour(body + k, "END"))
        .unwrap_or(s.len());
    Some((i, end))
}

fn sk_ant(s: &str, i: usize) -> Option<(usize, usize)> {
    prefixed(s, i, "sk-ant-", tok, 10)
}

fn sk(s: &str, i: usize) -> Option<(usize, usize)> {
    prefixed(s, i, "sk-", tok, 20)
}

fn github(s: &str, i: usize) -> Option<(usize, usize)> {
    ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
        .iter()
        .find_map(|p| prefixed(s, i, p, |b| b.is_ascii_alphanumeric(), 20))
        .or_else(|| prefixed(s, i, "github_pat_", |b| b.is_ascii_alphanumeric() || b == b'_', 20))
}

/// `\bAKIA[0-9A-Z]{16}\b`: exactly 16, so a longer run is not a key id.
fn akia(s: &str, i: usize) -> Option<(usize, usize)> {
    prefixed(s, i, "AKIA", |b| b.is_ascii_digit() || b.is_ascii_uppercase(), 16)?;
    let end = i + 4 + 16;
    boundary_at(s, end).then_some((i, end))
}

fn jwt(s: &str, i: usize) -> Option<(usize, usize)> {
    let (_, mut j) = prefixed(s, i, "eyJ", tok, 8)?;
    for min in [8, 4] {
        if !s[j..].starts_with('.') {
            return None;
        }
        let r = run(s, j + 1, tok);
        if r < min {
            return None;
        }
        j += 1 + r;
    }
    Some((i, j))
}

fn slack(s: &str, i: usize) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    if !s[i..].starts_with("xox") || !b.get(i + 3).is_some_and(|c| b"abprs".contains(c)) || b.get(i + 4) != Some(&b'-') {
        return None;
    }
    prefixed(s, i, &s[i..i + 5], |b| b.is_ascii_alphanumeric() || b == b'-', 10)
}

/// A Coder session token: `\b[A-Za-z0-9]{10}-[A-Za-z0-9]{22}\b`.
fn coder(s: &str, i: usize) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    let alnum = |from: usize, n: usize| b.get(from..from + n).is_some_and(|w| w.iter().all(u8::is_ascii_alphanumeric));
    let end = i + 33;
    (boundary_before(s, i) && alnum(i, 10) && b.get(i + 10) == Some(&b'-') && alnum(i + 11, 22) && boundary_at(s, end)).then_some((i, end))
}

const KEYS: [&str; 9] = [
    "password",
    "passwd",
    "pwd",
    "token",
    "secret",
    "api?key",
    "access?key",
    "private?key",
    "session?token",
];

/// A key word joined to a prefix by `_`: `DB_PASSWORD`, `MY_API_KEY` (#468). `_` is
/// a word character, so the word boundary missed it; `-` and `.` are not, so
/// `x-api-key` and `db.password` were already caught.
fn joined(s: &str, i: usize) -> bool {
    s[..i].ends_with('_')
}

/// `bearer x` and `token = x` style pairs: the name stays, the value goes. A key
/// word starts at a word boundary or after a `_` that joins it to a prefix;
/// either way `=` or `:` must follow it, so `password_hint` stays.
fn key_value(s: &str, i: usize) -> Option<(usize, usize)> {
    let at_word = boundary_before(s, i);
    if !at_word && !joined(s, i) {
        return None;
    }
    let keep = if let Some(j) = ci(s, i, "bearer").filter(|_| at_word) {
        let k = skip_ws(s, j);
        (k > j).then_some(k)?
    } else {
        let j = KEYS.iter().find_map(|k| ci(s, i, k))?;
        let j = skip_ws(s, j);
        if !s[j..].starts_with(['=', ':']) {
            return None;
        }
        skip_ws(s, j + 1)
    };
    let (chars, len) = s[keep..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .fold((0, 0), |(n, l), c| (n + 1, l + c.len_utf8()));
    (chars >= 4).then_some((keep, keep + len))
}
