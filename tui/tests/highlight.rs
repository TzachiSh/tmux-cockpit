//! The panel's code lexer (#282): per-language tables, fence detection, and the
//! never-panic, text-preserving property over random input.
use cockpit_tui::panel::highlight::{Lang, Role, State, fence, fenced, highlight, lang, line};

/// Every span of `src` (lines split on `\n`) lexed as `tag`, line breaks dropped.
fn spans<'a>(tag: &str, src: &'a str) -> Vec<(Role, &'a str)> {
    let lines: Vec<&str> = src.split('\n').collect();
    highlight(tag, &lines).into_iter().flatten().collect()
}

/// The role of `tok`'s first match in the one-line `src`: every byte of it has
/// that role, and a coloured token ends where its role does, on both sides.
fn role(tag: &str, src: &str, tok: &str) -> Role {
    assert!(!src.contains('\n'), "a table row is one line");
    let s = spans(tag, src);
    let per: Vec<Role> = s.iter().flat_map(|(r, t)| std::iter::repeat_n(*r, t.len())).collect();
    let at = src.find(tok).unwrap_or_else(|| panic!("{tok:?} is not in {src:?}"));
    let end = at + tok.len();
    let r = per[at];
    assert!(per[at..end].iter().all(|x| *x == r), "{tok:?} is split in {s:?}");
    let edge = |i: Option<usize>| i.and_then(|i| per.get(i)).is_some_and(|x| *x == r);
    assert!(
        r == Plain || !edge(at.checked_sub(1)) && !edge(Some(end)),
        "{tok:?} runs on in {s:?}"
    );
    r
}

/// (row name, tag, source, the span, its role)
type Row = (&'static str, &'static str, &'static str, &'static str, Role);

fn table(rows: &[Row]) {
    for (name, tag, src, tok, want) in rows {
        assert_eq!(role(tag, src, tok), *want, "row {name}: {tok:?} in {src:?}");
    }
}

use Role::{Comment, Keyword, Number, Plain, Str};

#[test]
fn tags_name_the_languages_case_insensitively() {
    let rows = [
        ("bash", Some(Lang::Shell)),
        ("SH", Some(Lang::Shell)),
        ("zsh", Some(Lang::Shell)),
        ("console", Some(Lang::Shell)),
        ("json", Some(Lang::Json)),
        ("rust", Some(Lang::Rust)),
        ("rs", Some(Lang::Rust)),
        ("Python", Some(Lang::Python)),
        ("py", Some(Lang::Python)),
        ("js", Some(Lang::Js)),
        ("typescript", Some(Lang::Js)),
        ("tsx", Some(Lang::Js)),
        ("", None),
        ("text", None),
        ("cobol", None),
    ];
    for (tag, want) in rows {
        assert_eq!(lang(tag), want, "tag {tag:?}");
    }
}

#[test]
fn empty_input_gives_no_spans() {
    assert!(highlight("bash", &[]).is_empty());
    assert_eq!(highlight("rust", &[""]), vec![Vec::<(Role, &str)>::new()]);
    assert!(line(Lang::Json, &mut State::default(), "").is_empty());
    assert_eq!(fenced(""), vec![Vec::<(Role, &str)>::new()]);
}

#[test]
fn an_unknown_tag_is_one_plain_span() {
    assert_eq!(spans("cobol", "if \"x\" # y"), vec![(Plain, "if \"x\" # y")]);
    assert_eq!(spans("", "fn main() {}"), vec![(Plain, "fn main() {}")]);
}

#[test]
fn shell_table() {
    table(&[
        ("keyword", "bash", "if true; then echo; fi", "if", Keyword),
        ("closing keyword", "bash", "if x; then y; fi", "fi", Keyword),
        ("command word", "bash", "rm -rf target", "rm", Plain),
        ("comment", "bash", "ls # list it", "# list it", Comment),
        ("whole-line comment", "bash", "# setup", "# setup", Comment),
        ("hash inside a word", "bash", "echo a#b", "a#b", Plain),
        ("hash after a metachar", "bash", "echo $# ${#a} args", "args", Plain),
        ("double quotes", "bash", "echo \"hi there\"", "\"hi there\"", Str),
        ("escaped quote", "bash", "echo \"a \\\" b\" x", "\"a \\\" b\"", Str),
        ("single quotes, no escape", "bash", "echo 'a\\' b", "'a\\'", Str),
        ("hash in a string", "bash", "echo \"# no\"", "\"# no\"", Str),
        ("number", "bash", "head -n 20 f", "20", Number),
        ("digits in a word", "bash", "ls file2", "file2", Plain),
    ]);
}

#[test]
fn a_shell_string_runs_across_lines() {
    let s = spans("bash", "echo \"one\ntwo\" done");
    assert_eq!(s[1..3], [(Str, "\"one"), (Str, "two\"")], "{s:?}");
    assert_eq!(s.last(), Some(&(Keyword, "done")), "{s:?}");
}

#[test]
fn a_heredoc_body_is_a_string_up_to_its_word() {
    let s = highlight("bash", &["cat <<'EOF'", "if it's here", "EOF", "fi"]);
    assert_eq!(s[1], vec![(Str, "if it's here")]);
    assert_eq!(s[2], vec![(Str, "EOF")]);
    assert_eq!(s[3], vec![(Keyword, "fi")]);
    let dash = highlight("sh", &["cat <<-END", "\tx", "\tEND", "done"]);
    assert_eq!(dash[3], vec![(Keyword, "done")], "<<- ends at a tab-indented word");
}

#[test]
fn json_table() {
    table(&[
        ("key", "json", "{\"name\": 1}", "\"name\"", Str),
        ("escapes", "json", "[\"a\\\"b\\\\\"]", "\"a\\\"b\\\\\"", Str),
        ("true", "json", "[true, 1]", "true", Keyword),
        ("null", "json", "{\"a\": null}", "null", Keyword),
        ("integer", "json", "[42]", "42", Number),
        ("exponent", "json", "[1.5e3]", "1.5e3", Number),
    ]);
}

#[test]
fn a_json_string_ends_at_the_line() {
    let s = highlight("json", &["\"open", "true"]);
    assert_eq!(s[0], vec![(Str, "\"open")]);
    assert_eq!(s[1], vec![(Keyword, "true")], "JSON strings never span lines");
}

#[test]
fn rust_table() {
    table(&[
        ("fn", "rust", "fn main() {}", "fn", Keyword),
        ("let mut", "rust", "let mut x = 1;", "mut", Keyword),
        ("ident", "rust", "let total = 1;", "total", Plain),
        ("line comment", "rust", "x; // note", "// note", Comment),
        ("doc comment", "rust", "/// docs", "/// docs", Comment),
        ("block comment", "rust", "a /* b */ c", "/* b */", Comment),
        ("string escape", "rust", "f(\"a\\\"b\")", "\"a\\\"b\"", Str),
        ("raw string", "rust", "r#\"say \"hi\"\"#;", "r#\"say \"hi\"\"#", Str),
        ("char", "rust", "let c = 'a';", "'a'", Str),
        ("escaped char", "rust", "let c = '\\n';", "'\\n'", Str),
        ("quote char", "rust", "let c = '\\'';", "'\\''", Str),
        ("hex", "rust", "let n = 0xff;", "0xff", Number),
        ("suffix", "rust", "let n = 1_000u32;", "1_000u32", Number),
        ("float", "rust", "let n = 1.5;", "1.5", Number),
    ]);
}

#[test]
fn a_rust_lifetime_is_not_a_char() {
    let s = spans("rust", "fn f<'a>(x: &'a str) {}");
    assert!(!s.iter().any(|(r, _)| *r == Str), "{s:?}");
}

#[test]
fn rust_block_comments_nest_across_lines() {
    let s = highlight("rust", &["/* a /* b */", "still */ fn"]);
    assert_eq!(s[0], vec![(Comment, "/* a /* b */")]);
    assert_eq!(s[1][0], (Comment, "still */"));
    assert_eq!(s[1].last(), Some(&(Keyword, "fn")));
}

#[test]
fn python_table() {
    table(&[
        ("def", "python", "def f(x):", "def", Keyword),
        ("None", "python", "x = None", "None", Keyword),
        ("comment", "python", "x = 1  # one", "# one", Comment),
        ("hash in string", "python", "s = \"# no\"", "\"# no\"", Str),
        ("nested quotes", "python", "s = \"it's\"", "\"it's\"", Str),
        ("single quotes", "python", "s = 'say \"hi\"'", "'say \"hi\"'", Str),
        ("escape", "python", "s = 'a\\'b'", "'a\\'b'", Str),
        ("number", "python", "x = 3.14", "3.14", Number),
    ]);
}

#[test]
fn a_python_triple_quote_spans_lines_and_a_single_does_not() {
    let s = highlight("py", &["x = '''one", "it's # two", "'''", "def"]);
    assert_eq!(s[1], vec![(Str, "it's # two")]);
    assert_eq!(s[2], vec![(Str, "'''")]);
    assert_eq!(s[3], vec![(Keyword, "def")]);
    let t = highlight("py", &["x = 'open", "def"]);
    assert_eq!(t[1], vec![(Keyword, "def")], "an unterminated ' ends at the line");
}

#[test]
fn js_table() {
    table(&[
        ("const", "js", "const x = 1;", "const", Keyword),
        ("interface", "ts", "interface P {}", "interface", Keyword),
        ("line comment", "js", "x(); // why", "// why", Comment),
        ("block comment", "js", "a /* b */ c", "/* b */", Comment),
        ("string", "js", "f('it\\'s')", "'it\\'s'", Str),
        ("template", "js", "f(`a ${b}`)", "`a ${b}`", Str),
        ("number", "js", "x = 10;", "10", Number),
    ]);
}

#[test]
fn a_js_template_spans_lines_and_comments_do_not_nest() {
    let s = highlight("js", &["x = `one", "two` + 1"]);
    assert_eq!(s[1][0], (Str, "two`"));
    let c = highlight("js", &["/* a /* b */ let"]);
    assert_eq!(c[0].last(), Some(&(Keyword, "let")), "{c:?}");
}

#[test]
fn an_unterminated_comment_runs_to_the_end() {
    let s = highlight("rust", &["/* open", "fn x", ""]);
    assert_eq!(s[1], vec![(Comment, "fn x")]);
}

#[test]
fn fence_openers_and_their_tags() {
    let rows = [
        ("```rust", Some("rust")),
        ("```", Some("")),
        ("  ```bash  ", Some("bash")),
        ("```` py", Some("py")),
        ("```js title=x", Some("js")),
        ("``", None),
        ("text ```rust", None),
        ("```a```", None),
        ("", None),
    ];
    for (s, want) in rows {
        assert_eq!(fence(s), want, "{s:?}");
    }
}

#[test]
fn only_the_inside_of_a_fence_is_lexed() {
    let s = fenced("if you\n```bash\nif x; then y; fi\n```\nif not");
    assert_eq!(s[0], vec![(Plain, "if you")]);
    assert_eq!(s[1], vec![(Role::Fence, "```bash")]);
    assert_eq!(s[2][0], (Keyword, "if"));
    assert_eq!(s[3], vec![(Role::Fence, "```")]);
    assert_eq!(s[4], vec![(Plain, "if not")]);
}

#[test]
fn a_fence_closes_only_on_its_own_length_or_more() {
    let s = fenced("````rust\n```\nfn\n````\nfn");
    assert_eq!(s[1], vec![(Plain, "```")], "a shorter fence is content");
    assert_eq!(s[2], vec![(Keyword, "fn")]);
    assert_eq!(s[4], vec![(Plain, "fn")], "closed by ````");
}

#[test]
fn an_unclosed_fence_runs_to_the_end() {
    let s = fenced("```py\ndef f():\n    return 1");
    assert_eq!(s[2][1], (Keyword, "return"));
}

#[test]
fn a_bare_fence_of_prompts_is_shell_and_other_bare_fences_plain() {
    let s = fenced("```\n$ if true; then ls; fi\n```");
    assert!(s[1].contains(&(Keyword, "if")), "{s:?}");
    let p = fenced("```\nif true; then ls; fi\n```");
    assert_eq!(p[1], vec![(Plain, "if true; then ls; fi")]);
}

// ------------------------------------------------------------- properties

/// xorshift64*: a fixed seed, so a failure names its input and replays.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

const TAGS: [&str; 7] = ["bash", "json", "rust", "python", "ts", "", "nope"];

/// Lexes `src` line by line in every language: no panic, no empty span, and the
/// spans of each line put back together are that line.
fn lossless(src: &str) {
    let lines: Vec<&str> = src.split('\n').collect();
    for tag in TAGS {
        for (got, want) in highlight(tag, &lines).iter().zip(&lines) {
            assert!(got.iter().all(|(_, t)| !t.is_empty()), "{tag}: empty span in {want:?}");
            assert_eq!(got.iter().map(|(_, t)| *t).collect::<String>(), *want, "{tag}");
        }
    }
    let joined: Vec<String> = fenced(src).iter().map(|l| l.iter().map(|(_, t)| *t).collect()).collect();
    assert_eq!(joined.join("\n"), src, "fenced");
}

#[test]
fn random_bytes_never_panic_and_keep_the_text() {
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..2000 {
        let n = (r.next() % 64) as usize;
        let bytes: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
        lossless(&String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn random_syntax_mixes_never_panic_and_keep_the_text() {
    let atoms = [
        "\"", "'", "`", "\\", "#", "//", "/*", "*/", "r#\"", "\"#", "'''", "\"\"\"", "<<EOF", "<<-'E'", "EOF", "\n", "\t", " ", "0x",
        "1.5e", "_", "fn", "if", "${", "}", "日本", "é", "👍🏽", "```rust", "```", "\u{fe0f}", "a",
    ];
    let mut r = Rng(42);
    for _ in 0..3000 {
        let n = (r.next() % 24) as usize;
        let src: String = (0..n).map(|_| r.pick(&atoms)).collect();
        lossless(&src);
    }
}

#[test]
fn a_very_long_line_is_lexed_whole() {
    let long = "\"\\".repeat(200_000) + "x # é 日本 /* 1.5";
    lossless(&long);
    let s = spans("rust", &long);
    assert_eq!(s.iter().map(|(_, t)| t.len()).sum::<usize>(), long.len());
}

// ------------------------------------------------------- review fixes

#[test]
fn a_shift_in_shell_arithmetic_opens_no_heredoc() {
    let s = highlight("bash", &["echo $((1<<2))", "fi"]);
    assert_eq!(s[1], vec![(Keyword, "fi")], "{s:?}");
}

/// A quoted word is never a shift: `cat <<'2'` is a heredoc that ends at `2`.
#[test]
fn a_quoted_digit_end_word_is_still_a_heredoc() {
    let s = highlight("bash", &["cat <<'2'", "x", "2", "fi"]);
    assert_eq!(s[1], vec![(Str, "x")], "{s:?}");
    assert_eq!(s[3], vec![(Keyword, "fi")], "{s:?}");
}

#[test]
fn a_crlf_heredoc_end_still_closes() {
    let s = highlight("bash", &["cat <<EOF\r", "body\r", "EOF\r", "fi"]);
    assert_eq!(s[2], vec![(Str, "EOF\r")]);
    assert_eq!(s[3], vec![(Keyword, "fi")], "{s:?}");
}

/// A string still open at the line's end is what the next line continues; a
/// heredoc on the same line waits behind it, and never swallows it.
#[test]
fn an_open_string_outranks_a_heredoc_on_its_line() {
    let s = highlight("bash", &["cat <<EOF \"abc", "def\" fi"]);
    assert_eq!(s[1][0], (Str, "def\""), "{s:?}");
    assert_eq!(s[1].last(), Some(&(Keyword, "fi")), "{s:?}");
}

#[test]
fn nothing_left_open_in_one_fence_reaches_the_next() {
    let rows = [
        ("comment", "```rust\n/* open\n```\n```rust\nfn\n```"),
        ("string", "```rust\n\"open\n```\n```rust\nfn\n```"),
        ("heredoc", "```bash\ncat <<EOF\n```\n```bash\nfi\n```"),
    ];
    for (name, text) in rows {
        let s = fenced(text);
        assert_eq!(s[4], vec![(Keyword, s[4][0].1)], "{name}: {s:?}");
    }
}

#[test]
fn fence_tags_with_attributes() {
    let rows = [
        ("```rust,ignore", Some("rust")),
        ("``` {.python}", Some("python")),
        ("```{.py .numberLines}", Some("py")),
        ("```{bash}", Some("bash")),
        ("``` ,x", Some("")),
    ];
    for (s, want) in rows {
        assert_eq!(fence(s), want, "{s:?}");
    }
    assert_eq!(fenced("```rust,ignore\nfn\n```")[1], vec![(Keyword, "fn")]);
}
