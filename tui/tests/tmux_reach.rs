//! Which test binaries run a real tmux, and that each goes through the helpers'
//! check first (#1066). cargo's runner clears the caller's TMUX, TMUX_PANE and
//! TMUX_COCKPIT_DIR for a test binary and gives it a TMUX_TMPDIR of its own;
//! `fake::checked_tmux` refuses a binary started without them. A binary that
//! runs tmux and never reaches that check would run unrefused by hand from
//! inside a tmux session.
mod fake;

use std::path::{Path, PathBuf};

/// The text of every Rust string literal in `src`, `\n` read as a new line: plain, byte, C
/// and raw ones, as [`lit_at`] reads them. Comments and char literals are skipped.
fn literals(src: &str) -> Vec<(usize, String)> {
    let (mut out, mut i) = (vec![], 0);
    while i < src.len() {
        match lit_at(src, i) {
            Some((Lit::Str { raw }, start, end, next)) => {
                let t = &src[start..end];
                out.push((start, if raw { t.to_string() } else { unescape(t) }));
                i = next;
            }
            Some((_, _, _, next)) => i = next,
            None => i += src[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    out
}

/// A cooked literal's text with `\n` a new line and any other escape its next char.
fn unescape(t: &str) -> String {
    let (mut out, mut cs) = (String::new(), t.chars());
    while let Some(c) = cs.next() {
        match (c, c == '\\') {
            (_, true) => out.extend(cs.next().map(|e| if e == 'n' { '\n' } else { e })),
            (c, false) => out.push(c),
        }
    }
    out
}

/// What a comment or literal at byte `i` of `src` is.
enum Lit {
    Comment,
    Char,
    Str { raw: bool },
}

/// The comment or literal that starts at byte `i` of `src`, read as rustc reads it (#1110),
/// or `None` for code: its kind, where its text starts and ends, and where code resumes. A
/// `//` comment runs to its line's end and a `/* */` one nests. A char or byte-char literal
/// is one char or one escape between quotes, so a lifetime is not one. A raw string starts
/// at an `r`, `br` or `cr` that continues no word, and its hashes must end at a quote, so
/// `r#type` is an identifier. A plain, byte or C string ends at its first unescaped quote.
fn lit_at(src: &str, i: usize) -> Option<(Lit, usize, usize, usize)> {
    let b = src.as_bytes();
    let at = |j: usize| b.get(j).copied();
    match b[i] {
        b'/' if at(i + 1) == Some(b'/') => {
            let end = src[i..].find('\n').map_or(b.len(), |n| i + n);
            Some((Lit::Comment, i, end, end))
        }
        b'/' if at(i + 1) == Some(b'*') => {
            let (mut depth, mut j) = (0, i);
            while j < b.len() {
                let two = &b[j..b.len().min(j + 2)];
                depth += (two == b"/*") as i32 - (two == b"*/") as i32;
                j += if two == b"/*" || two == b"*/" { 2 } else { 1 };
                if depth == 0 {
                    break;
                }
            }
            Some((Lit::Comment, i, j, j))
        }
        b'\'' => {
            let c = src[i + 1..].chars().next()?;
            let after = i + 1 + c.len_utf8();
            let end = match c {
                '\\' => after + 1 + src.get(after + 1..)?.find('\'')?,
                _ if at(after) == Some(b'\'') => after,
                _ => return None,
            };
            Some((Lit::Char, i + 1, end, end + 1))
        }
        b'r' | b'b' | b'c' if !word(b, i) => {
            let r = if b[i] == b'r' { i } else { i + 1 };
            let hashes = b.get(r + 1..)?.iter().take_while(|&&c| c == b'#').count();
            if at(r) != Some(b'r') || at(r + 1 + hashes) != Some(b'"') {
                return None;
            }
            let close = format!("\"{}", "#".repeat(hashes));
            let start = r + 2 + hashes;
            let end = src[start..].find(&close).map_or(b.len(), |n| start + n);
            Some((Lit::Str { raw: true }, start, end, b.len().min(end + close.len())))
        }
        b'"' => {
            let mut j = i + 1;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = j.min(b.len());
            Some((Lit::Str { raw: false }, i + 1, end, b.len().min(end + 1)))
        }
        _ => None,
    }
}

/// Whether the byte before `i` continues a word, as in `br"` or `for"`.
fn word(b: &[u8], i: usize) -> bool {
    i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')
}

/// Whether a literal at `at` in `src`, with text `t`, names a real tmux to run:
/// `tmux` alone as a program (not a path part given to `join`), an argv that
/// starts with it (a COCKPIT_TMUX value), or a line of a script that runs it,
/// `tmux` and a word after an optional `exec`.
fn runs_tmux(src: &str, at: usize, t: &str) -> bool {
    let before = src[..at - 1].trim_end();
    let program = t == "tmux" && !before.ends_with("join(");
    let line = |l: &str| {
        let l = l.trim_start();
        let l = l.strip_prefix("exec ").unwrap_or(l).trim_start();
        let arg = |c: char| c.is_ascii_lowercase() || c == '-' || c == '"';
        l.strip_prefix("tmux ").is_some_and(|rest| rest.starts_with(arg))
    };
    let shell = t.contains('\n') && t.lines().flat_map(|l| l.split([';', '|', '&'])).any(line);
    program || t.starts_with("tmux -") || shell
}

/// The files a test binary's root `root` is built from: itself, and each
/// module it loads by `mod <name>;`, with or without `#[path = ...]`.
fn crate_files(root: &Path) -> Vec<PathBuf> {
    let dir = root.parent().unwrap();
    let src = std::fs::read_to_string(root).unwrap();
    let mut files = vec![root.to_path_buf()];
    let mut path = None;
    for l in src.lines().map(str::trim) {
        if let Some(p) = l.strip_prefix("#[path = \"").and_then(|p| p.strip_suffix("\"]")) {
            path = Some(dir.join(p));
        } else if let Some(m) = l.strip_prefix("mod ").and_then(|m| m.strip_suffix(';')) {
            files.push(path.take().unwrap_or_else(|| dir.join(m).join("mod.rs")));
        }
    }
    files
}

/// `fake/mod.rs`, read apart: every tmux it runs goes through its own
/// `checked_tmux` first, so its text would name each binary that loads it.
const FAKE: &str = "fake/mod.rs";

/// What a binary's files say when they reach the check: a call to it, or the
/// rig server whose start makes it.
const CHECKS: [&str; 2] = ["fake::checked_tmux(", "fake::Server::new("];

/// Binaries the scan names that run no real tmux, each with why.
const ALLOWED: [(&str, &str); 1] = [(
    "transcript.rs",
    "writes a stub named tmux and runs it by its path, through COCKPIT_TMUX",
)];

/// #1066: every test binary that runs a real tmux reaches the helpers' check.
/// Read from `tui/tests`: a binary is named where one of its files holds a
/// string literal that [`runs_tmux`] and none of its files names a [`CHECKS`]
/// entry. What it cannot see: a binary that reaches tmux only through the
/// product, as the panel binaries and `files` do through `data::tmux` and
/// `Theme::load` (`data::tmux` itself refuses a plain tmux that would reach the
/// caller's server, [`the_product_refuses_a_plain_tmux_bound_for_the_callers_server`]);
/// tmux by a path, as `Command::new("/usr/bin/tmux")`; a one-line shell string, as
/// `["-c", "tmux ls"]`; a tmux name built at run time or by a helper from
/// parts; a script read from a file; and a check named in a file but never
/// reached.
#[test]
fn every_binary_that_runs_tmux_reaches_the_check() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut missing = vec![];
    let mut roots: Vec<PathBuf> = std::fs::read_dir(&tests).unwrap().map(|e| e.unwrap().path()).collect();
    roots.retain(|p| p.extension().is_some_and(|e| e == "rs"));
    roots.sort();
    for root in &roots {
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        let files = crate_files(root);
        let text = |f: &PathBuf| std::fs::read_to_string(f).unwrap();
        let checks = files.iter().any(|f| CHECKS.iter().any(|c| text(f).contains(c)));
        let runs = files.iter().filter(|f| !f.ends_with(FAKE)).find_map(|f| {
            let src = text(f);
            let hit = literals(&src).into_iter().find(|(at, t)| runs_tmux(&src, *at, t))?;
            let line = src[..hit.0].lines().count();
            Some(format!("{}:{line}", f.strip_prefix(&tests).unwrap().display()))
        });
        let allowed = ALLOWED.iter().any(|(f, _)| *f == name);
        if let (Some(at), false, false) = (runs, checks, allowed) {
            missing.push(at);
        }
    }
    assert_eq!(missing, Vec::<String>::new(), "run a real tmux and never reach fake::checked_tmux");
}

/// The scan above, on sources with a known answer, so a reader that sees
/// nothing cannot pass it. The shapes it reads, not every way to run tmux:
/// [`every_binary_that_runs_tmux_reaches_the_check`] lists what it misses.
#[test]
fn the_scan_finds_the_shapes_it_reads_and_no_lookalike() {
    let rows = [
        ("a program", r#"Command::new("tmux").arg("-V")"#, true),
        ("an argv value", r#"format!("tmux -L {}", s)"#, true),
        ("a script line", r##""#!/bin/sh\ntmux -L x \"$@\"\n""##, true),
        ("after a ;", r##""#!/bin/sh\nset -e; tmux -S y ls\n""##, true),
        ("a command word", r##""#!/bin/sh\ntmux ls\n""##, true),
        ("after exec", r##""#!/bin/sh\nexec tmux new-session -d\n""##, true),
        ("a quoted command", r##""#!/bin/sh\nexec tmux \"$@\"\n""##, true),
        ("a path part", r#"dir.join("tmux")"#, false),
        ("a stub's path", r#"format!("{d}/bin/tmux")"#, false),
        ("words in a message", r#"assert!(x, "tmux is not answering")"#, false),
        ("a plugin folder", r#"".cache/tmux-cockpit/agents.json""#, false),
        ("a comment", "// Command::new(\"tmux\")\n", false),
    ];
    for (row, src, want) in rows {
        let got = literals(src).into_iter().any(|(at, t)| runs_tmux(src, at, &t));
        assert_eq!(got, want, "{row}: {src}");
    }
}

// --- #1110: one environment lock in a binary, fake's ---

/// `src` with its comments and the insides of its string and char literals blanked, as
/// [`lit_at`] reads them, so a scan of what the code declares and calls meets no word of a
/// message or a row's table.
fn code_of(src: &str) -> String {
    let (mut out, mut i) = (String::new(), 0);
    while i < src.len() {
        if let Some((_, _, _, next)) = lit_at(src, i) {
            out.push(' ');
            i = next;
        } else {
            let c = src[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// The locks named ENV a file may keep, each as its declaration in its file, never as the
/// file: push's, an `RwLock` because its readers, its rigs, run in parallel and its one
/// writer alone. That writer sets `COCKPIT_TMUX` and `COCKPIT_READONLY`, which no check
/// reads. The one every check reads under is the test-env crate's, outside `tests/` (#1132).
const KEPT: [(&str, &str); 1] = [("push.rs", "static ENV: RwLock<()> = RwLock::new(());")];

/// Whether any `static ENV` in the file `name` is a second lock, whatever its type and
/// whether or not the file writes: in a binary that writes a variable a check reads in its
/// own process (`COCKPIT_LOG`; `XDG_CACHE_HOME`; `TMUX`, `TMUX_PANE`, `TMUX_TMPDIR` and
/// `XDG_CACHE_HOME`), and in any file of `fake`, which owns the one.
fn by_name(name: &str) -> bool {
    ["data.rs", "agents.rs", "panel_env.rs"].contains(&name) || name.starts_with("fake/")
}

/// The words that mark a file as writing its own environment: std's and libc's, and the
/// interim writers that take no lock until step 2 of #1132 removes them.
const WRITES: [&str; 7] = [
    "set_var",
    "remove_var",
    "set_var_unlocked",
    "remove_var_unlocked",
    "setenv",
    "putenv",
    "unsetenv",
];

/// Whether `w` stands in `code` as a whole word.
fn has_word(code: &str, w: &str) -> bool {
    let id = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    code.match_indices(w)
        .any(|(at, _)| !id(code[..at].chars().last()) && !id(code[at + w.len()..].chars().next()))
}

/// Each `static` of `src`, the file `name` under `tui/tests`, that is a lock of its own
/// beside fake's (#1110), read on its code with spaces made one: one named `ENV` (`mut` or
/// not) that is a `Mutex` or `RwLock` in a file that writes ([`WRITES`]), or any `ENV`
/// where [`by_name`] holds; and a `Mutex<()>` or `RwLock<()>` of any name in a file that
/// writes. A [`KEPT`] declaration passes once, at the file's top level and the first
/// time only. Its declaration, up to its `;`.
fn own_env_locks(name: &str, src: &str) -> Vec<String> {
    let code = code_of(src).split_whitespace().collect::<Vec<_>>().join(" ");
    let writes = WRITES.iter().any(|w| has_word(&code, w));
    let (mut found, mut kept_once) = (vec![], false);
    for (at, _) in code.match_indices("static ") {
        // a word of its own, not a lifetime's `'static`
        if !matches!(code[..at].chars().last(), None | Some(' ' | '{' | '}' | ';')) {
            continue;
        }
        let rest = &code[at + "static ".len()..];
        let rest = rest.strip_prefix("mut ").unwrap_or(rest);
        let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
        let Some(ty) = rest[end..].trim_start().strip_prefix(':') else {
            continue;
        };
        let decl = code[at..].split(';').next().unwrap_or_default().to_string() + ";";
        let ty = ty.split('=').next().unwrap_or_default().trim();
        let unit_lock = matches!(ty.rsplit("::").next(), Some("Mutex<()>" | "RwLock<()>"));
        let env = &rest[..end] == "ENV";
        let lock = decl.contains("Mutex") || decl.contains("RwLock");
        let top = code[..at].matches('{').count() == code[..at].matches('}').count();
        if top && !kept_once && KEPT.contains(&(name, decl.as_str())) {
            kept_once = true;
        } else if (env && (by_name(name) || (lock && writes))) || (unit_lock && writes) {
            found.push(decl);
        }
    }
    found
}

/// Every `.rs` file under `dir`, named by its path under `top`, `/` between the parts.
fn rust_files(top: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    for e in std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()) {
        if e.is_dir() {
            rust_files(top, &e, out);
        } else if e.extension().is_some_and(|x| x == "rs") {
            let name = e.strip_prefix(top).unwrap().to_string_lossy().replace('\\', "/");
            out.push((name, e));
        }
    }
}

/// #1110: no test binary keeps an environment lock of its own beside fake's, which every
/// check reads under. A sibling's write under another lock would be read by a check as the
/// caller's. What it cannot see: a writer that takes no lock at all, a lock by another
/// name, and a write made through a helper of another file.
#[test]
fn no_binary_keeps_an_environment_lock_of_its_own() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files = vec![];
    rust_files(&tests, &tests, &mut files);
    assert!(files.iter().any(|(n, _)| n == "data.rs"), "the walk found no data.rs: {files:?}");
    let mut found: Vec<(String, String)> = vec![];
    for (name, f) in &files {
        let src = std::fs::read_to_string(f).unwrap();
        found.extend(own_env_locks(name, &src).into_iter().map(|d| (name.clone(), d)));
    }
    assert_eq!(found, Vec::<(String, String)>::new(), "a lock of its own: take fake::env_lock()");
}

/// The scan above, on sources with a known answer: what it flags and the lookalikes it
/// passes, push's lock among them only where it is push's.
#[test]
fn the_lock_scan_finds_the_shapes_it_reads_and_no_lookalike() {
    let mutex = "static ENV: Mutex<()> = Mutex::new(());";
    let rw = "static ENV: RwLock<()> = RwLock::new(());";
    let local = "static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());";
    let set = "fn f() { unsafe { std::env::set_var(\"A\", \"1\") } }\n";
    let unset = "fn f() { unsafe { std::env::remove_var(\"A\") } }\n";
    let rows: [(&str, &str, String, &[&str]); 22] = [
        ("a Mutex, a set_var", "x.rs", format!("{mutex}\n{set}"), &[mutex]),
        ("a Mutex, a remove_var", "x.rs", format!("{mutex}\n{unset}"), &[mutex]),
        ("an RwLock, a set_var", "x.rs", format!("{rw}\n{set}"), &[rw]),
        ("one inside a fn", "x.rs", format!("fn t() {{\n    {local}\n}}\n{set}"), &[local]),
        ("a pub one", "x.rs", format!("pub {mutex}\n{set}"), &[mutex]),
        ("push's", "push.rs", format!("{rw}\n{set}"), &[]),
        (
            "push's and a Mutex",
            "push.rs",
            format!("{rw}\nfn t() {{ {mutex} }}\n{set}"),
            &[mutex],
        ),
        ("push's in another file", "theme.rs", format!("{rw}\n{set}"), &[rw]),
        ("data's, no write", "data.rs", format!("{mutex}\n"), &[mutex]),
        ("agents', no write", "agents.rs", format!("{mutex}\n"), &[mutex]),
        ("panel_env's, no write", "panel_env.rs", format!("{mutex}\n"), &[mutex]),
        (
            "one in fake/env.rs, the crate's since #1132",
            "fake/env.rs",
            format!("{mutex}\n{set}"),
            &[mutex],
        ),
        ("one in fake/mod.rs, no write", "fake/mod.rs", format!("{mutex}\n"), &[mutex]),
        ("no write", "x.rs", format!("{mutex}\nfn f() {{ std::env::var(\"A\"); }}\n"), &[]),
        ("a write in a comment", "x.rs", format!("{mutex}\n// set_var(\"A\", \"1\")\n"), &[]),
        ("a lock in a comment", "x.rs", format!("// {mutex}\n{set}"), &[]),
        ("a lock in a string", "x.rs", format!("const S: &str = \"{mutex}\";\n{set}"), &[]),
        (
            "another name, no unit lock",
            "x.rs",
            format!("{}\n{set}", "static ENVS: Mutex<u8> = Mutex::new(0);"),
            &[],
        ),
        ("push's twice", "push.rs", format!("{rw}\nfn t() {{ {rw} }}\n{set}"), &[rw]),
        ("push's twice at the top", "push.rs", format!("{rw}\n{rw}\n{set}"), &[rw]),
        ("push's in a module", "push.rs", format!("mod m {{ {rw} }}\n{set}"), &[rw]),
        (
            "a lock in a raw string",
            "x.rs",
            format!("const S: &str = r#\"{mutex}\"#;\n{set}"),
            &[],
        ),
    ];
    for (row, name, src, want) in rows {
        assert_eq!(own_env_locks(name, &src), want, "{row}: {src}");
    }
}

/// The scan above past a lock's name, its spacing and the word a write is made with: a unit
/// lock by any name in a file that writes, `static mut`, a declaration spread over spaces
/// and lines, and std's or libc's writes by whatever name they are called.
#[test]
fn the_lock_scan_reads_past_names_spacing_and_the_writes_word() {
    let mutex = "static ENV: Mutex<()> = Mutex::new(());";
    let renamed = "static LOCK: Mutex<()> = Mutex::new(());";
    let rw = "static GATE: std::sync::RwLock<()> = std::sync::RwLock::new(());";
    let set = "fn f() { unsafe { std::env::set_var(\"A\", \"1\") } }\n";
    let libc = |w: &str| format!("{mutex}\nfn f() {{ unsafe {{ libc::{w}(p) }}; }}\n");
    let interim = |w: &str| format!("{renamed}\nfn f() {{ unsafe {{ fake::{w}(\"A\") }} }}\n");
    let rows: [(&str, String, &[&str]); 15] = [
        ("a renamed unit Mutex, a write", format!("{renamed}\n{set}"), &[renamed]),
        ("a renamed unit RwLock, a write", format!("{rw}\n{set}"), &[rw]),
        ("a renamed unit Mutex, no write", format!("{renamed}\n"), &[]),
        (
            "a lock of another type",
            format!("static HELD: Mutex<Option<u8>> = Mutex::new(None);\n{set}"),
            &[],
        ),
        (
            "static mut",
            format!("static mut ENV: Mutex<()> = Mutex::new(());\n{set}"),
            &["static mut ENV: Mutex<()> = Mutex::new(());"],
        ),
        (
            "spaces and a line break",
            format!("static  \n  ENV : Mutex<()> = Mutex::new(());\n{set}"),
            &["static ENV : Mutex<()> = Mutex::new(());"],
        ),
        (
            "a 'static lifetime",
            format!("fn g(s: &'static str) -> &'static str {{ s }}\n{set}"),
            &[],
        ),
        (
            "set_var by another name",
            format!("{mutex}\nuse std::env::set_var as sv;\n"),
            &[mutex],
        ),
        (
            "remove_var by another name",
            format!("{mutex}\nuse std::env::remove_var as rv;\n"),
            &[mutex],
        ),
        ("libc's setenv", libc("setenv"), &[mutex]),
        ("libc's putenv", libc("putenv"), &[mutex]),
        ("libc's unsetenv", libc("unsetenv"), &[mutex]),
        ("a word holding setenv", format!("{mutex}\nfn presetenv() {{}}\n"), &[]),
        ("an interim write, #1132", interim("set_var_unlocked"), &[renamed]),
        ("an interim remove, #1132", interim("remove_var_unlocked"), &[renamed]),
    ];
    for (row, src, want) in rows {
        assert_eq!(own_env_locks("x.rs", &src), want, "{row}: {src}");
    }
}

/// The lock scan's lexer ([`lit_at`] through [`code_of`]) on sources with a known answer:
/// each form of string, char and comment that can hold a quote or a `*/`, read as rustc
/// reads it. A lock and a write after the form are still named; inside a comment, not.
#[test]
fn the_lock_scan_reads_literals_and_comments_as_rustc_does() {
    let lock = "static ENV: Mutex<()> = Mutex::new(());";
    let set = "fn f() { unsafe { std::env::set_var(\"A\", \"1\") } }";
    let after: [(&str, &str); 13] = [
        ("a raw byte string ending in a backslash", r#"const P: &[u8] = br"C:\";"#),
        ("a raw string ending in a backslash", r#"const P: &str = r"C:\";"#),
        ("a raw C string ending in a backslash", r#"const P: &CStr = cr"C:\";"#),
        ("a raw string with hashes holding a quote", r##"const P: &str = r#"a "b" c"#;"##),
        ("a raw byte string with hashes", r##"const P: &[u8] = br#"a"b"#;"##),
        ("a byte string with an escaped quote", r#"const P: &[u8] = b"a\"b";"#),
        ("a char holding a quote", r#"const C: char = '"';"#),
        ("a byte char holding a quote", r#"const C: u8 = b'"';"#),
        ("an escaped quote char", r#"const C: char = '\''; const D: &str = "x";"#),
        ("an escaped quote, then a quote char", r#"const C: [char; 2] = ['\'','"'];"#),
        ("a char past ASCII", r#"const C: char = 'é'; const D: &str = "x";"#),
        ("a raw identifier", r#"fn f(r#type: u8) -> u8 { r#type }"#),
        ("a nested block comment", "/* a /* b */ c */"),
    ];
    for (row, form) in after {
        let src = format!("{form}\n{lock}\n{set}\n");
        assert_eq!(own_env_locks("x.rs", &src), [lock], "{row}: {src}");
    }
    let src = format!("/* a /* b */ {lock} */\n{set}\n");
    assert_eq!(own_env_locks("x.rs", &src), Vec::<String>::new(), "a lock inside a nested comment");
}

/// The tmux scan's lexer ([`lit_at`] through [`literals`]) on the same forms: the texts of the
/// string literals, as rustc reads them, and nothing from a comment or a char.
#[test]
fn the_tmux_scan_reads_literals_and_comments_as_rustc_does() {
    let texts = |src: &str| literals(src).into_iter().map(|(_, t)| t).collect::<Vec<_>>();
    let rows: [(&str, &str, &[&str]); 6] = [
        (
            "a raw byte string ending in a backslash",
            r#"br"C:\"; "tmux -V""#,
            &["C:\\", "tmux -V"],
        ),
        ("a raw string with hashes", r##"r#"a "tmux" b"#"##, &["a \"tmux\" b"]),
        ("a nested block comment", r#"/* "x" /* "y" */ "z" */ "w""#, &["w"]),
        ("a char holding a quote", r#"'"' "v""#, &["v"]),
        ("an escaped quote char", r#"'\'' "v""#, &["v"]),
        ("a raw identifier", r#"r#type = "v""#, &["v"]),
    ];
    for (row, src, want) in rows {
        assert_eq!(texts(src), want, "{row}: {src}");
    }
}

/// #1066: the folder the runner makes for this binary in /tmp holds tmux's own
/// `tmux-<uid>` folder and the sockets in it, and nothing else, read while a rig
/// server runs there.
#[test]
fn the_tmux_folder_holds_sockets_and_nothing_else() {
    use std::os::unix::fs::FileTypeExt;
    let dir = fake::checked_tmux();
    let mut s = fake::Server::new(&format!("cr-{}", std::process::id()));
    let cwd = std::env::temp_dir();
    if !s.start(|c| {
        c.args(["new-session", "-d", "-c"]).arg(&cwd).arg("sleep 30");
    }) {
        eprintln!("SKIP: no tmux on PATH");
        return;
    }
    let mut odd = vec![];
    for top in std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap()) {
        let name = top.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("tmux-") && top.file_type().unwrap().is_dir()) {
            odd.push(name);
            continue;
        }
        for e in std::fs::read_dir(top.path()).unwrap().map(|e| e.unwrap()) {
            if !e.file_type().unwrap().is_socket() {
                odd.push(format!("{name}/{}", e.file_name().to_string_lossy()));
            }
        }
    }
    assert_eq!(odd, Vec::<String>::new(), "in {}", dir.display());
}

/// #1066: what a part 4 row may kill is a process whose TMUX names a rig socket of the
/// child and which carries this binary's own TMUX_TMPDIR. Two `sleep`s of the row's own
/// with the same socket name in TMUX: the one with another TMUX_TMPDIR, as a server
/// another run leaked under a reused pid would have, is not taken.
#[test]
#[cfg(target_os = "linux")]
fn a_process_with_another_tmux_tmpdir_is_never_taken_for_a_rigs() {
    let own = fake::checked_tmux();
    let child = 999_999_999u32;
    let tmux = format!("{}/tmux-0/cockpit-decoy-{child},1,0", own.display());
    let start = |tmpdir: &Path| {
        let mut c = std::process::Command::new("/bin/sleep");
        c.arg("30").env("TMUX", &tmux).env("TMUX_TMPDIR", tmpdir).spawn().unwrap()
    };
    let (mut ours, mut decoy) = (start(&own), start(Path::new("/elsewhere-1066")));
    // the spawns have exec'd sleep once each reads as sleep in /proc: polled, 20 s at most
    let comm = |p: u32| std::fs::read_to_string(format!("/proc/{p}/comm")).unwrap_or_default();
    for tries in 0.. {
        if comm(ours.id()).trim() == "sleep" && comm(decoy.id()).trim() == "sleep" {
            break;
        }
        assert!(tries < 4000, "the two sleeps were not sleep after {tries} tries");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let left = fake::servers_left(child);
    for c in [&mut ours, &mut decoy] {
        c.kill().unwrap();
        c.wait().unwrap();
    }
    assert_eq!((left.contains(&ours.id()), left.contains(&decoy.id())), (true, false), "{left:?}");
}

/// Set in the child of [`the_product_refuses_a_plain_tmux_bound_for_the_callers_server`]:
/// that row then makes the product's read once and ends.
const PRODUCT_READ: &str = "COCKPIT_TEST_PRODUCT_READ";

/// How many calls the product's agent list read (`data::tmux`) made, in a child of row `me`
/// started by hand as a bare start is, no TMUX, TMUX_TMPDIR, COCKPIT_TMUX or
/// COCKPIT_TEST_TMUX_GUARD (so a debug build's own keying is on), then `set`, and
/// two stubs first on PATH that record each call and open no socket: `tmux`, and `stub`.
fn product_reads(s: &Path, me: &str, set: &[(&str, &str)]) -> usize {
    let path = format!("{}:{}", s.display(), std::env::var("PATH").unwrap_or_default());
    let mut c = std::process::Command::new(std::env::current_exe().unwrap());
    for v in ["TMUX", "TMUX_TMPDIR", "COCKPIT_TMUX", "COCKPIT_TEST_TMUX_GUARD"] {
        c.env_remove(v);
    }
    c.args(["--exact", me]).env("PATH", path.as_str());
    c.env(PRODUCT_READ, "1")
        .envs(set.iter().copied())
        .stdin(std::process::Stdio::null());
    let out = c.output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 passed"), "{out:?}");
    let mut calls = 0;
    for f in ["tmux.calls", "stub.calls"] {
        calls += std::fs::read_to_string(s.join(f)).unwrap_or_default().lines().count();
        let _ = std::fs::remove_file(s.join(f));
    }
    calls
}

/// A case of the row below: its name, what its child has set, and the calls it wants.
type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], usize);

/// #1089: under test the product's own tmux call refuses a plain `tmux` that would reach the
/// caller's server, by TMUX or as the default socket in /tmp, and reads it as no value. It
/// still runs the runner's case, a `-L` server, a stub, and any tmux with the guard lifted.
#[test]
fn the_product_refuses_a_plain_tmux_bound_for_the_callers_server() {
    if std::env::var_os(PRODUCT_READ).is_some() {
        let glob = cockpit_tui::data::tmux(&["show-option", "-gqv", "@cockpit_agents_glob"]);
        assert_eq!(glob, None);
        return;
    }
    let tag = format!("cockpit-product-read-{}", std::process::id());
    let s = fake::Scratch::fresh(std::env::temp_dir().join(tag));
    for name in ["tmux", "stub"] {
        fake::write_exec(&s.0.join(name), "#!/bin/sh\necho \"$*\" >> \"$0.calls\"\nexit 1\n");
    }
    let (dir, none) = (s.0.display().to_string(), s.0.join("none").display().to_string());
    let cases: [Case; 9] = [
        ("the runner's: TMUX unset, a TMUX_TMPDIR folder", &[("TMUX_TMPDIR", &dir)], 1),
        ("no TMUX_TMPDIR, a debug build with no variable", &[], 0),
        ("no TMUX_TMPDIR, COCKPIT_TEST_TMUX_GUARD=1", &[("COCKPIT_TEST_TMUX_GUARD", "1")], 0),
        ("a TMUX_TMPDIR that is no folder", &[("TMUX_TMPDIR", &none)], 0),
        ("TMUX set", &[("TMUX_TMPDIR", &dir), ("TMUX", "/elsewhere/default,1,0")], 0),
        ("a -L server, no TMUX_TMPDIR", &[("COCKPIT_TMUX", "tmux -L cockpit-read")], 1),
        ("a -S socket, no TMUX_TMPDIR", &[("COCKPIT_TMUX", "tmux -S /nowhere-1089/s")], 1),
        ("a stub by another name, no TMUX_TMPDIR", &[("COCKPIT_TMUX", "stub")], 1),
        ("the guard lifted, no TMUX_TMPDIR", &[("COCKPIT_TEST_TMUX_GUARD", "0")], 1),
    ];
    let me = "the_product_refuses_a_plain_tmux_bound_for_the_callers_server";
    let got: Vec<_> = cases.iter().map(|(n, set, _)| (*n, product_reads(&s.0, me, set))).collect();
    assert_eq!(got, cases.map(|(n, _, want)| (n, want)));
}

/// Set in the child of [`the_board_is_told_once_that_a_read_was_refused`] to what it
/// wants: `refused` or `ran`.
const PRODUCT_NOTE: &str = "COCKPIT_TEST_PRODUCT_NOTE";

/// The footer row of the board, `w` columns wide, showing `note` over no agents, as a
/// board whose reads were refused draws it (#1089).
fn footer_at(note: &str, w: u16) -> String {
    let th = cockpit_tui::theme::Theme::fixed();
    let v = cockpit_tui::board::View {
        panes: &[],
        pvs: &[],
        th: &th,
        sel: 0,
        top: 0,
        active: "",
        note: Some(note),
        help: None,
        stale: None,
        lost: false,
        never: false,
        list: None,
        confirm: None,
    };
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, 8)).unwrap();
    t.draw(|f| cockpit_tui::board::render(f, f.area(), &v)).unwrap();
    let b = t.backend().buffer();
    (0..w).map(|x| b[(x, 7)].symbol()).collect()
}

/// #1089: once `data::tmux` refused a read, the board's note names the guard and the way
/// out, once per run, and a 40-column footer still shows the way out; where nothing was
/// refused there is no note.
#[test]
fn the_board_is_told_once_that_a_read_was_refused() {
    if let Some(want) = std::env::var_os(PRODUCT_NOTE) {
        let read = || cockpit_tui::data::tmux(&["show-option", "-gqv", "@cockpit_agents_glob"]);
        let _ = (read(), read());
        let mut note = None;
        let shown = cockpit_tui::board::note_guard(&mut note, std::time::Instant::now());
        let said = [note.as_ref().map(|n| n.text.as_str()), cockpit_tui::data::guard_note()];
        assert_eq!((shown, note.as_ref().and_then(|n| n.ttl)), (want == "refused", None));
        // a board in a 40-column pane, the narrowest it draws, still shows the way out
        let foot = note.as_ref().map(|n| footer_at(&n.text, 40));
        let out = "COCKPIT_TEST_TMUX_GUARD=0 to lift";
        assert!(foot.as_ref().is_none_or(|f| f.contains(out)), "40 columns: {foot:?}");
        let text = "COCKPIT_TEST_TMUX_GUARD=0 to lift: no private tmux in COCKPIT_TMUX (-L or -S); \
                    reads refused";
        assert_eq!(said, if want == "refused" { [Some(text), None] } else { [None, None] });
        return;
    }
    let s = fake::Scratch::fresh(std::env::temp_dir().join(format!("cockpit-note-{}", std::process::id())));
    fake::write_exec(&s.0.join("tmux"), "#!/bin/sh\necho \"$*\" >> \"$0.calls\"\nexit 1\n");
    let me = "the_board_is_told_once_that_a_read_was_refused";
    let dir = s.0.display().to_string();
    assert_eq!(product_reads(&s.0, me, &[(PRODUCT_NOTE, "refused")]), 0);
    assert_eq!(product_reads(&s.0, me, &[(PRODUCT_NOTE, "ran"), ("TMUX_TMPDIR", &dir)]), 2);
}

/// Set in the child of [`push_and_the_tile_previews_send_the_callers_server_nothing`]: which
/// product path it starts.
const PRODUCT_PATH: &str = "COCKPIT_TEST_PRODUCT_PATH";

/// #1089: push's wait loop and the board's tile previews run tmux on the argv themselves.
/// Under the guard neither sends a caller's server anything: push registers no channel and
/// starts no `wait-for`, and the previews read as failed captures. Started with `TMUX` set.
#[test]
fn push_and_the_tile_previews_send_the_callers_server_nothing() {
    if let Some(path) = std::env::var_os(PRODUCT_PATH) {
        if path == "push" {
            // its wait loop runs on a thread of its own: up to 400 tries of 5 ms for the
            // stub, first on PATH, to record a call, so an ungated loop is seen
            let _push = cockpit_tui::push::Push::start();
            let path = std::env::var("PATH").unwrap_or_default();
            let calls = Path::new(path.split(':').next().unwrap_or_default()).join("tmux.calls");
            for _ in 0..400 {
                if calls.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        } else {
            let _ = cockpit_tui::board::previews(&["tmux".to_string()], &[("%1".to_string(), 5)]);
        }
        return;
    }
    let s = fake::Scratch::fresh(std::env::temp_dir().join(format!("cockpit-paths-{}", std::process::id())));
    fake::write_exec(&s.0.join("tmux"), "#!/bin/sh\necho \"$*\" >> \"$0.calls\"\nexit 1\n");
    let me = "push_and_the_tile_previews_send_the_callers_server_nothing";
    let tmux = ("TMUX", "/elsewhere/default,1,0");
    let push = product_reads(&s.0, me, &[(PRODUCT_PATH, "push"), tmux]);
    let previews = product_reads(&s.0, me, &[(PRODUCT_PATH, "previews"), tmux]);
    let dir = s.0.display().to_string();
    let ran = product_reads(&s.0, me, &[(PRODUCT_PATH, "previews"), ("TMUX_TMPDIR", &dir)]);
    assert_eq!((push, previews, ran), (0, 0, 1));
}

// --- #1132: the environment written only through the lock's token ---

/// How many times `w` stands in `code` as a whole word.
fn words(code: &str, w: &str) -> usize {
    let id = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    code.match_indices(w)
        .filter(|(at, _)| !id(code[..*at].chars().last()))
        .filter(|(at, _)| !id(code[at + w.len()..].chars().next()))
        .count()
}

/// The interim writers' names on the code of each `.rs` file under `tui/tests` and
/// `tui/examples`, comments and literals blanked, for each file that names one: the file
/// under `tui/`, and the count.
fn interim_names() -> Vec<(String, usize)> {
    let top = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![];
    for dir in ["tests", "examples"] {
        rust_files(top, &top.join(dir), &mut files);
    }
    files.sort();
    let named = files.into_iter().map(|(name, f)| {
        let code = code_of(&std::fs::read_to_string(f).unwrap());
        (name, words(&code, "set_var_unlocked") + words(&code, "remove_var_unlocked"))
    });
    named.filter(|(_, n)| *n > 0).collect()
}

/// #1132: the writes that take no lock, through `set_var_unlocked` and
/// `remove_var_unlocked`, only ever get fewer. 62 names at #1132's step 1: 60 calls in 22
/// files, and fake's re-export of the two names. A step-2 PR moves a binary's writes to
/// `fake::set_var(&held, ..)` and lowers its row and the total here; nothing may raise
/// them. Counted as words, so a call through a renamed import counts only its `use`.
#[test]
fn the_unlocked_writes_only_get_fewer() {
    let want: [(&str, usize); 22] = [
        ("examples/panel_eval.rs", 4),
        ("tests/basket_claude.rs", 1),
        ("tests/basket_claude_glob.rs", 1),
        ("tests/basket_stub.rs", 2),
        ("tests/basket_tmux.rs", 3),
        ("tests/branch_oldgit.rs", 1),
        ("tests/ended_glob.rs", 5),
        ("tests/fake/mod.rs", 5),
        ("tests/files_global.rs", 2),
        ("tests/forks.rs", 5),
        ("tests/forks_dirs.rs", 5),
        ("tests/forks_ended.rs", 5),
        ("tests/info_ask.rs", 3),
        ("tests/oldgit/mod.rs", 1),
        ("tests/panel_basket_tmux.rs", 1),
        ("tests/panel_explore_home.rs", 1),
        ("tests/preview_shape.rs", 1),
        ("tests/push.rs", 3),
        ("tests/subs_fields.rs", 6),
        ("tests/subs_tmux.rs", 5),
        ("tests/tiles_batch_same.rs", 1),
        ("tests/tiles_batch_stub.rs", 1),
    ];
    let got = interim_names();
    let total: usize = got.iter().map(|(_, n)| n).sum();
    let got: Vec<(&str, usize)> = got.iter().map(|(f, n)| (f.as_str(), *n)).collect();
    assert_eq!((total, got), (62, want.to_vec()), "lower the table in a step-2 PR, never raise it");
}

/// The word count above on sources with a known answer: a call counts, a longer
/// identifier, a comment and a string do not.
#[test]
fn the_unlocked_count_reads_words_on_code() {
    let rows: [(&str, &str, usize); 5] = [
        ("a call", "unsafe { fake::set_var_unlocked(\"A\", \"1\") }", 1),
        ("a longer name", "fn set_var_unlocked_too() {}", 0),
        ("in a comment", "// fake::set_var_unlocked(\"A\", \"1\")", 0),
        ("in a string", "const S: &str = \"set_var_unlocked\";", 0),
        ("two calls", "set_var_unlocked(a, b); set_var_unlocked(c, d);", 2),
    ];
    for (row, src, want) in rows {
        assert_eq!(words(&code_of(src), "set_var_unlocked"), want, "{row}");
    }
}

/// The lint level `tui/Cargo.toml` sets for `disallowed_methods`, from its own line of the
/// `[lints.clippy]` table, if the table holds one.
fn lint_level(manifest: &str) -> Option<&str> {
    let table = manifest.split("\n[").find_map(|t| t.strip_prefix("lints.clippy]\n"))?;
    table.lines().find_map(|l| l.strip_prefix("disallowed_methods = "))
}

/// #1132: the raw writers stay forbidden. clippy refuses a raw write only while
/// `tui/Cargo.toml` holds the lint at forbid and `tui/clippy.toml` names each writer, and
/// it says nothing when either is weakened, so this row does.
#[test]
fn the_raw_writers_stay_forbidden() {
    let top = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(top.join("Cargo.toml")).unwrap();
    assert_eq!(lint_level(&manifest), Some("\"forbid\""), "tui/Cargo.toml's [lints.clippy]");
    let conf = std::fs::read_to_string(top.join("clippy.toml")).unwrap();
    let named: Vec<&str> = conf
        .lines()
        .filter_map(|l| l.strip_prefix("path = \"")?.strip_suffix('"'))
        .collect();
    let want = [
        "std::env::set_var",
        "std::env::remove_var",
        "libc::setenv",
        "libc::unsetenv",
        "libc::putenv",
    ];
    assert_eq!(named, want, "tui/clippy.toml's disallowed-methods");
}

/// How many `extern` blocks `code` declares, of any ABI: an `extern` word whose next
/// token, past the ABI string that [`code_of`] blanks, is `{`.
fn extern_blocks(code: &str) -> usize {
    let id = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    code.match_indices("extern")
        .filter(|(at, _)| !id(code[..*at].chars().last()))
        .filter(|(at, _)| code[at + "extern".len()..].trim_start().starts_with('{'))
        .count()
}

/// #1132: no file under `tui/tests` or `tui/examples` declares an `extern` block. A
/// `setenv` declared in one is a function of its own that no `tui/clippy.toml` path
/// names, so the lint would let a raw write through it.
#[test]
fn no_test_declares_an_extern_block() {
    let top = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![];
    for dir in ["tests", "examples"] {
        rust_files(top, &top.join(dir), &mut files);
    }
    assert!(
        files.iter().any(|(n, _)| n == "tests/push.rs"),
        "the walk found no push.rs: {files:?}"
    );
    let found: Vec<String> = files
        .into_iter()
        .filter(|(_, f)| extern_blocks(&code_of(&std::fs::read_to_string(f).unwrap())) > 0)
        .map(|(n, _)| n)
        .collect();
    assert_eq!(found, Vec::<String>::new(), "an extern block: write through fake::set_var");
}

/// The block count above on sources with a known answer: a block of each ABI spelling
/// counts; a function defined with an ABI, a crate, a comment and a string do not.
#[test]
fn the_extern_scan_reads_blocks_on_code() {
    let rows: [(&str, &str, usize); 8] = [
        ("C", "unsafe extern \"C\" { fn setenv(); }", 1),
        ("no ABI", "extern { fn setenv(); }", 1),
        ("system", "extern \"system\" {\n    fn putenv();\n}", 1),
        ("a raw ABI", "unsafe extern r\"C\" { fn setenv(); }", 1),
        ("a fn with an ABI", "extern \"C\" fn f() {}", 0),
        ("a crate", "extern crate libc;", 0),
        ("in a comment", "// extern \"C\" { fn setenv(); }", 0),
        ("in a string", "const S: &str = \"extern {\";", 0),
    ];
    for (row, src, want) in rows {
        assert_eq!(extern_blocks(&code_of(src)), want, "{row}");
    }
}
