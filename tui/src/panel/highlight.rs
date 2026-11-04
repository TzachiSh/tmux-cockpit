//! A small line-based lexer for code in the panel (#282): fenced blocks in the
//! Result and Answer text, and a step's command. It only labels slices of the
//! input, so the concatenated spans are the input byte for byte and a width
//! never moves. Built in, not syntect: the binary is the budget.
//!
//! Each line is one forward pass that advances at least a byte per step, so a
//! line costs time linear in its length, with no backtracking. What spans lines
//! (a block comment, a string, a heredoc) is carried in [`State`].

/// What a slice of code is. The panel maps each to an `@thm_*` colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
    /// a fence's own ``` line
    Fence,
}

/// The languages the lexer knows. Anything else is plain text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Shell,
    Json,
    Rust,
    Python,
    Js,
}

/// A fence tag or a step's `lang`, case-insensitive. `None` is plain text.
pub fn lang(tag: &str) -> Option<Lang> {
    Some(match tag.to_ascii_lowercase().as_str() {
        "sh" | "bash" | "shell" | "zsh" | "ksh" | "fish" | "console" | "shellsession" | "termux" => Lang::Shell,
        "json" | "jsonc" | "json5" => Lang::Json,
        "rust" | "rs" => Lang::Rust,
        "python" | "py" | "python3" => Lang::Python,
        "js" | "javascript" | "jsx" | "mjs" | "cjs" | "ts" | "typescript" | "tsx" => Lang::Js,
        _ => return None,
    })
}

/// What one line leaves open for the next.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    open: Open,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Open {
    #[default]
    Nothing,
    /// a block comment, `depth` deep: Rust's nest
    Comment {
        depth: u32,
    },
    Str(Quote),
    /// a shell heredoc's body, up to the line that is `word`
    Heredoc {
        word: String,
        tabs: bool,
    },
}

/// How a string ends: its quote byte, three of it, or the quote and `hashes`
/// `#`s of a Rust raw string. `esc`: a backslash takes the next byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Quote {
    q: u8,
    triple: bool,
    hashes: usize,
    esc: bool,
    /// the string goes on past the end of the line
    spans: bool,
}

const SHELL_KW: &[&str] = &[
    "if", "then", "else", "elif", "fi", "case", "esac", "for", "select", "while", "until", "do", "done", "in", "function", "time", "local",
    "export", "readonly", "declare", "return",
];
const JSON_KW: &[&str] = &["true", "false", "null"];
const RUST_KW: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl",
    "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
    "true", "type", "unsafe", "use", "where", "while",
];
const PY_KW: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for",
    "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
    "yield", "True", "False", "None",
];
const JS_KW: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "true",
    "false",
    "null",
    "undefined",
    "interface",
    "enum",
    "implements",
    "private",
    "public",
    "protected",
    "readonly",
    "declare",
    "namespace",
    "abstract",
];

fn keywords(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Shell => SHELL_KW,
        Lang::Json => JSON_KW,
        Lang::Rust => RUST_KW,
        Lang::Python => PY_KW,
        Lang::Js => JS_KW,
    }
}

/// A word byte. UTF-8's lead and continuation bytes count, so a cut never lands
/// inside a char. The shell's words run to its metacharacters: `a#b` is one.
fn word(lang: Lang, c: u8) -> bool {
    match lang {
        Lang::Shell => !c.is_ascii_whitespace() && !b";|&()<>\"'`$={}".contains(&c),
        Lang::Js => c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80,
        _ => c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80,
    }
}

/// Spans as byte ranges, adjacent ones of one role merged.
struct Out(Vec<(Role, usize, usize)>);

impl Out {
    fn push(&mut self, r: Role, a: usize, b: usize) {
        if a >= b {
            return;
        }
        match self.0.last_mut() {
            Some((lr, _, le)) if *lr == r && *le == a => *le = b,
            _ => self.0.push((r, a, b)),
        }
    }

    /// Every cut is at an ASCII byte or an end, so `get` never fails; were one
    /// to, the line goes out plain rather than panic.
    fn spans(self, s: &str) -> Vec<(Role, &str)> {
        let got: Option<Vec<_>> = self.0.iter().map(|&(r, a, b)| s.get(a..b).map(|t| (r, t))).collect();
        got.unwrap_or_else(|| plain(s))
    }
}

/// One line's spans, `st` carried in from the line above and out to the next.
pub fn line<'a>(lang: Lang, st: &mut State, s: &'a str) -> Vec<(Role, &'a str)> {
    let b = s.as_bytes();
    let mut out = Out(Vec::new());
    let mut i = 0;
    match std::mem::take(&mut st.open) {
        Open::Nothing => {}
        Open::Heredoc { word, tabs } => {
            // a CRLF text's `EOF\r` is its end word too
            let t = if tabs { s.trim_start_matches('\t') } else { s };
            if t.trim_end_matches('\r') != word {
                st.open = Open::Heredoc { word, tabs };
            }
            out.push(Role::Str, 0, b.len());
            return out.spans(s);
        }
        Open::Comment { depth } => {
            i = comment(lang, st, b, 0, depth);
            out.push(Role::Comment, 0, i);
        }
        Open::Str(q) => {
            i = string(st, b, 0, q);
            out.push(Role::Str, 0, i);
        }
    }
    let mut heredoc = None;
    while i < b.len() {
        let c = b[i];
        let at = |p: &[u8]| b[i..].starts_with(p);
        let line_comment = match lang {
            Lang::Shell => c == b'#' && (i == 0 || b" \t;|&(".contains(&b[i - 1])),
            Lang::Python => c == b'#',
            _ => at(b"//"),
        };
        if line_comment {
            out.push(Role::Comment, i, b.len());
            i = b.len();
        } else if lang != Lang::Shell && lang != Lang::Python && at(b"/*") {
            let j = comment(lang, st, b, i + 2, 1);
            out.push(Role::Comment, i, j);
            i = j;
        } else if let Some((q, from)) = quote(lang, b, i) {
            let j = string(st, b, from, q);
            out.push(Role::Str, i, j);
            i = j;
        } else if lang == Lang::Rust && c == b'\'' {
            let j = char_lit(b, i).unwrap_or(i + 1);
            out.push(if j > i + 1 { Role::Str } else { Role::Plain }, i, j);
            i = j;
        } else if lang == Lang::Shell && at(b"<<") && !at(b"<<<") {
            let (j, h) = heredoc_op(s, i + 2);
            heredoc = h.or(heredoc);
            out.push(Role::Plain, i, j);
            i = j;
        } else if word(lang, c) {
            let j = i + b[i..].iter().take_while(|&&c| word(lang, c)).count();
            let j = if c.is_ascii_digit() && lang != Lang::Shell {
                number(b, i)
            } else {
                j
            };
            out.push(s.get(i..j).map_or(Role::Plain, |w| classify(lang, w)), i, j);
            i = j;
        } else {
            out.push(Role::Plain, i, i + 1);
            i += 1;
        }
    }
    if let Open::Str(q) = st.open
        && !q.spans
    {
        st.open = Open::Nothing;
    }
    if st.open == Open::Nothing
        && let Some((word, tabs)) = heredoc
    {
        st.open = Open::Heredoc { word, tabs };
    }
    out.spans(s)
}

fn classify(lang: Lang, w: &str) -> Role {
    if keywords(lang).contains(&w) {
        Role::Keyword
    } else if w.as_bytes()[0].is_ascii_digit() && (lang != Lang::Shell || w.bytes().all(|c| c.is_ascii_digit())) {
        Role::Number
    } else {
        Role::Plain
    }
}

/// A number from `i`: digits, letters and `_` (`0xff`, `1_000u32`), a `.`
/// before a digit, and a sign after a decimal exponent (`1e-5`).
fn number(b: &[u8], i: usize) -> usize {
    let hex = b[i..].starts_with(b"0x") || b[i..].starts_with(b"0X");
    let mut j = i;
    while j < b.len() {
        let c = b[j];
        let digit_next = b.get(j + 1).is_some_and(u8::is_ascii_digit);
        let exp = !hex && j > i && matches!(b[j - 1], b'e' | b'E');
        if c.is_ascii_alphanumeric() || c == b'_' || (c == b'.' || (exp && matches!(c, b'+' | b'-'))) && digit_next {
            j += 1;
        } else {
            break;
        }
    }
    j
}

/// A string opener at `i`: its [`Quote`] and the index past the opener.
fn quote(lang: Lang, b: &[u8], i: usize) -> Option<(Quote, usize)> {
    let c = b[i];
    let q = |q, esc, spans| Quote {
        q,
        triple: false,
        hashes: 0,
        esc,
        spans,
    };
    match lang {
        Lang::Shell if c == b'\'' => Some((q(c, false, true), i + 1)),
        Lang::Shell if c == b'"' => Some((q(c, true, true), i + 1)),
        Lang::Json if c == b'"' => Some((q(c, true, false), i + 1)),
        Lang::Js if c == b'`' => Some((q(c, true, true), i + 1)),
        Lang::Js if c == b'"' || c == b'\'' => Some((q(c, true, false), i + 1)),
        Lang::Rust if c == b'"' => Some((q(c, true, true), i + 1)),
        Lang::Rust if c == b'r' && (i == 0 || !word(lang, b[i - 1])) => {
            let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
            (b.get(i + 1 + hashes) == Some(&b'"')).then_some((
                Quote {
                    hashes,
                    ..q(b'"', false, true)
                },
                i + 2 + hashes,
            ))
        }
        Lang::Python if c == b'"' || c == b'\'' => {
            let triple = b[i..].starts_with(&[c, c, c]);
            let quote = Quote {
                triple,
                ..q(c, true, triple)
            };
            Some((quote, i + if triple { 3 } else { 1 }))
        }
        _ => None,
    }
}

/// Scans a string body from `j`: the index past its close, or the line's end
/// with the string left open in `st`.
fn string(st: &mut State, b: &[u8], mut j: usize, q: Quote) -> usize {
    while j < b.len() {
        let c = b[j];
        if q.esc && c == b'\\' {
            j += 2;
            continue;
        }
        if c == q.q {
            let n = if q.triple { 3 } else { 1 };
            let close = b[j..].starts_with(&[c, c, c][..n]) && b.len() >= j + n + q.hashes;
            if close && b[j + n..j + n + q.hashes].iter().all(|&h| h == b'#') {
                return j + n + q.hashes;
            }
        }
        j += 1;
    }
    st.open = Open::Str(q);
    b.len()
}

/// Scans a block comment from `j`, `depth` deep: past its `*/`, or the line's
/// end with the comment left open in `st`.
fn comment(lang: Lang, st: &mut State, b: &[u8], mut j: usize, mut depth: u32) -> usize {
    while j < b.len() {
        if b[j..].starts_with(b"*/") {
            depth = depth.saturating_sub(1);
            j += 2;
            if depth == 0 {
                return j;
            }
        } else if lang == Lang::Rust && b[j..].starts_with(b"/*") {
            depth = depth.saturating_add(1);
            j += 2;
        } else {
            j += 1;
        }
    }
    st.open = Open::Comment { depth };
    b.len()
}

/// A Rust char literal at `i`: `'a'`, `'é'`, `'\n'`, `'\u{1F600}'`. A lifetime
/// (`'a`) is not one, and gives `None`.
fn char_lit(b: &[u8], i: usize) -> Option<usize> {
    let body = b.get(i + 1)?;
    if *body == b'\\' {
        // an escape is at most `\u{10FFFF}`: the close is near or absent
        let end = (i + 13).min(b.len());
        let k = (i + 3..end).find(|&k| b[k] == b'\'')?;
        return Some(k + 1);
    }
    let len = match body {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    };
    (*body != b'\'' && b.get(i + 1 + len) == Some(&b'\'')).then_some(i + 2 + len)
}

/// After a shell `<<`: the index past `-'WORD'`, and the word the body ends at.
fn heredoc_op(s: &str, mut j: usize) -> (usize, Option<(String, bool)>) {
    let b = s.as_bytes();
    let tabs = b.get(j) == Some(&b'-');
    j += usize::from(tabs);
    while b.get(j).is_some_and(|c| *c == b' ' || *c == b'\t') {
        j += 1;
    }
    let q = b.get(j).copied().filter(|c| *c == b'\'' || *c == b'"');
    j += usize::from(q.is_some());
    let w0 = j;
    while b.get(j).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
        j += 1;
    }
    let word = s[w0..j].to_string();
    if q.is_some() && b.get(j).copied() == q {
        j += 1;
    }
    // a word starting with a digit is a shift, as in `$((1<<2))`
    let shift = word.bytes().next().is_none_or(|c| c.is_ascii_digit());
    (j, (!shift).then_some((word, tabs)))
}

/// Lines of one language. An unknown `tag` gives each line one plain span.
pub fn highlight<'a>(tag: &str, lines: &[&'a str]) -> Vec<Vec<(Role, &'a str)>> {
    match lang(tag) {
        Some(l) => lex(l, lines),
        None => lines.iter().map(|s| plain(s)).collect(),
    }
}

fn lex<'a>(l: Lang, lines: &[&'a str]) -> Vec<Vec<(Role, &'a str)>> {
    let mut st = State::default();
    lines.iter().map(|s| line(l, &mut st, s)).collect()
}

/// A fence line's backtick count and the rest of the line; a backtick in the
/// rest makes it inline code, not a fence.
fn ticks(s: &str) -> Option<(usize, &str)> {
    let t = s.trim_start();
    let n = t.bytes().take_while(|&c| c == b'`').count();
    let rest = &t[n..];
    (n >= 3 && !rest.contains('`')).then_some((n, rest))
}

/// A fence opener's tag: `Some("")` for a bare fence, `None` for no fence.
pub fn fence(s: &str) -> Option<&str> {
    ticks(s).map(|(_, rest)| rest.split_whitespace().next().unwrap_or(""))
}

/// Markdown-ish text: the lines inside a fenced block are lexed by the fence's
/// tag, the fence lines are [`Role::Fence`], and every other line is plain. A
/// fence left open runs to the end. A bare fence whose lines all start `$ ` is
/// a shell session.
pub fn fenced(text: &str) -> Vec<Vec<(Role, &str)>> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let Some((n, rest)) = ticks(lines[i]) else {
            out.push(plain(lines[i]));
            i += 1;
            continue;
        };
        out.push(vec![(Role::Fence, lines[i])]);
        let body = &lines[i + 1..];
        let end = body
            .iter()
            .position(|l| ticks(l).is_some_and(|(m, r)| m >= n && r.trim().is_empty()));
        let body = &body[..end.unwrap_or(body.len())];
        let tag = rest.split_whitespace().next().unwrap_or("");
        let prompts = || {
            let mut full = body.iter().filter(|l| !l.trim().is_empty()).peekable();
            full.peek().is_some() && full.all(|l| l.trim_start().starts_with("$ "))
        };
        match lang(tag) {
            Some(l) => out.extend(lex(l, body)),
            None if tag.is_empty() && prompts() => out.extend(lex(Lang::Shell, body)),
            None => out.extend(body.iter().map(|s| plain(s))),
        }
        i += 1 + body.len();
        if let Some(close) = end.map(|_| lines[i]) {
            out.push(vec![(Role::Fence, close)]);
            i += 1;
        }
    }
    out
}

fn plain(s: &str) -> Vec<(Role, &str)> {
    if s.is_empty() { vec![] } else { vec![(Role::Plain, s)] }
}
