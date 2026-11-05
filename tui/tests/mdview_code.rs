//! The viewer on a code file (#365 child 2): the language from the name or the
//! shebang, every line numbered and highlighted, no markdown read into it, and
//! the keys the markdown viewer has. No tmux and no tty. COCKPIT_BLESS=1
//! rewrites the goldens.
use cockpit_tui::highlight::{Lang, Role};
use cockpit_tui::mdview::code;
use cockpit_tui::mdview::render::{Ink, Kind, Opts, Row};
use cockpit_tui::mdview::{self, Mode, Out, Screen, mode};
use cockpit_tui::theme::{Glyphs, Theme};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn golden(name: &str, got: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    let path = format!("{dir}/{name}.txt");
    if std::env::var("COCKPIT_BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path}; run COCKPIT_BLESS=1 cargo test"));
    assert_eq!(want, got, "\n{name} changed:\n--- want ---\n{want}\n--- got ---\n{got}");
}

fn draw(w: u16, h: u16, th: &Theme, s: &mut Screen) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| mdview::render(f, f.area(), th, s)).unwrap();
    t.backend().to_string()
}

const AREA: Rect = Rect::new(0, 0, 60, 13);

const RUST: &str = "// a comment\nfn main() {\n\tlet s = \"# not a heading\";\n    println!(\"*{s}*\");\n}\n";

fn code(name: &str, src: &str) -> Screen {
    let m = mode(name, src);
    let mut s = Screen::new(name, Ok(src.into()), Opts::default()).with_mode(m);
    s.layout(mdview::body_width(AREA));
    s
}

fn key(s: &mut Screen, c: KeyCode) -> Out {
    s.key(KeyEvent::new(c, KeyModifiers::NONE), AREA)
}

/// Each row as drawn: its gutter, then its cells.
fn texts(rows: &[Row]) -> Vec<String> {
    (0..rows.len())
        .map(|i| code::gutter(rows, i).unwrap_or_default() + &rows[i].text())
        .collect()
}

// ---------------------------------------------------------------- the language

#[test]
fn the_extension_picks_markdown_or_a_language() {
    for n in ["a.md", "A.MARKDOWN", "b.mdown", "c.mkd"] {
        assert_eq!(mode(n, "fn x"), Mode::Markdown, "{n}");
    }
    let cases = [
        ("main.rs", Some(Lang::Rust)),
        ("x.PY", Some(Lang::Python)),
        ("a.ts", Some(Lang::Js)),
        ("a.tsx", Some(Lang::Js)),
        ("p.json", Some(Lang::Json)),
        ("run.sh", Some(Lang::Shell)),
        ("main.go", None),
        ("notes.txt", None),
    ];
    for (n, want) in cases {
        assert_eq!(mode(n, ""), Mode::Code(want), "{n}");
    }
}

/// With no extension it knows, the first line's `#!` names the language.
#[test]
fn a_shebang_names_the_language() {
    let cases = [
        ("#!/usr/bin/env python3\nprint(1)\n", Some(Lang::Python)),
        ("#!/usr/bin/env -S python3.11 -u\n", Some(Lang::Python)),
        ("#!/bin/bash\n", Some(Lang::Shell)),
        ("#! /bin/sh -e\n", Some(Lang::Shell)),
        ("#!/usr/bin/env node\n", Some(Lang::Js)),
        ("#!/usr/bin/perl\n", None),
        ("no shebang\n", None),
        ("", None),
    ];
    for (src, want) in cases {
        assert_eq!(mode("tool", src), Mode::Code(want), "{src:?}");
    }
    assert_eq!(mode("t.md", "#!/bin/sh\n"), Mode::Markdown, "an extension wins");
}

// ---------------------------------------------------------------- the rows

#[test]
fn each_source_line_is_one_numbered_row() {
    let rows = code::render(RUST, Some(Lang::Rust), 60);
    assert_eq!(
        rows.iter().map(|r| r.line).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5],
        "no row for the last newline"
    );
    assert_eq!(texts(&rows)[0], "1 // a comment");
    assert_eq!(texts(&rows)[2], "3     let s = \"# not a heading\";", "a tab to the next stop");
    assert_eq!(texts(&rows)[3], "4     println!(\"*{s}*\");", "no markdown is read");
    assert!(rows.iter().all(|r| r.lead == 2), "the gutter's columns");
    assert_eq!(rows[0].text(), "// a comment", "the gutter is no cell");
}

#[test]
fn the_gutter_is_as_wide_as_the_last_number() {
    let src: String = (1..=12).map(|i| format!("l{i}\n")).collect();
    let rows = code::render(&src, None, 40);
    assert_eq!(texts(&rows)[0], " 1 l1");
    assert_eq!(texts(&rows)[11], "12 l12");
}

/// A line wider than the body wraps; its rows keep its number once and its line.
#[test]
fn a_long_line_wraps_under_a_blank_gutter() {
    let src = format!("short\n{}\n", "x".repeat(30));
    let rows = code::render(&src, None, 12);
    assert_eq!(texts(&rows), ["1 short", "2 xxxxxxxxxx", "  xxxxxxxxxx", "  xxxxxxxxxx"]);
    assert_eq!(rows.iter().map(|r| r.line).collect::<Vec<_>>(), [1, 2, 2, 2]);
    assert!(rows.iter().all(|r| r.lead + r.width() <= 12));
}

#[test]
fn crlf_and_an_empty_file() {
    assert_eq!(texts(&code::render("a\r\nb\r\n", None, 20)), ["1 a", "2 b"]);
    assert!(code::render("", None, 20).is_empty());
    assert_eq!(texts(&code::render("\n", None, 20)), ["1 "], "one empty line");
}

#[test]
fn the_lexer_labels_the_cells() {
    let rows = code::render(RUST, Some(Lang::Rust), 60);
    let kinds: Vec<(String, Kind)> = rows[1].cells.iter().map(|(t, i)| (t.clone(), i.kind)).collect();
    assert_eq!(kinds[0], ("fn".into(), Kind::Code(Role::Keyword)), "{kinds:?}");
    let plain = code::render(RUST, None, 60);
    assert!(
        plain
            .iter()
            .flat_map(|r| &r.cells)
            .all(|(_, i)| *i == Ink::of(Kind::Code(Role::Plain))),
        "an unknown language is plain"
    );
}

// ---------------------------------------------------------------- the screen

/// A line number is never a match, and a match lands on the code's own bytes.
#[test]
fn find_skips_the_gutter() {
    let mut s = code("x.txt", "alpha\nbeta\ngamma\n");
    key(&mut s, KeyCode::Char('/'));
    key(&mut s, KeyCode::Char('2'));
    assert!(s.hits.is_empty(), "{:?}", s.hits);
    key(&mut s, KeyCode::Backspace);
    key(&mut s, KeyCode::Char('m'));
    key(&mut s, KeyCode::Enter);
    let at: Vec<(usize, usize)> = s.hits.iter().map(|h| (h.row, h.from)).collect();
    assert_eq!(at, [(2, 2), (2, 3)], "g-a-m-m-a");
}

/// `e` opens the editor at the top row's source line, a wrapped line's own.
#[test]
fn e_edits_at_the_top_source_line() {
    let tail: String = (3..30).map(|i| format!("l{i}\n")).collect();
    let src = format!("a\n{}\n{tail}", "x".repeat(200));
    let mut s = code("x.txt", &src);
    for _ in 0..3 {
        key(&mut s, KeyCode::Down);
    }
    assert_eq!(s.rows()[s.top].line, 2, "inside the wrapped line");
    assert_eq!(key(&mut s, KeyCode::Char('e')), Out::Edit(2));
}

#[test]
fn a_resize_keeps_the_top_source_line() {
    let src: String = (1..=40).map(|i| format!("line {i} {}\n", "w".repeat(i))).collect();
    let mut s = code("x.txt", &src);
    s.top = s.rows().iter().position(|r| r.line == 20).unwrap();
    s.layout(30);
    assert_eq!(s.rows()[s.top].line, 20);
    assert_eq!(s.line(), 20);
}

#[test]
fn opening_at_a_line_puts_it_on_top() {
    let src: String = (1..=50).map(|i| format!("l{i}\n")).collect();
    let mut s = Screen::new("x.rs", Ok(src), Opts::default())
        .with_mode(Mode::Code(Some(Lang::Rust)))
        .at_line(30);
    s.layout(40);
    assert_eq!(s.line(), 30);
}

/// A file the loader refused is its note, as in markdown.
#[test]
fn a_refused_file_is_a_note() {
    let mut s = Screen::new("blob.bin", Err("a binary file".into()), Opts::default()).with_mode(Mode::Code(None));
    let out = draw(60, 10, &Theme::fixed(), &mut s);
    assert!(out.contains("a binary file: e opens it in the editor"), "{out}");
}

// ---------------------------------------------------------------- goldens

fn sample() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/view-sample.rs")).unwrap()
}

#[test]
fn code_goldens() {
    let th = Theme::fixed();
    let mut s = Screen::new("view-sample.rs", Ok(sample()), Opts::default()).with_mode(mode("view-sample.rs", ""));
    golden("view-code-80", &draw(80, 24, &th, &mut s));
    let mut s = Screen::new("view-sample.rs", Ok(sample()), Opts::default()).with_mode(mode("view-sample.rs", ""));
    golden("view-code-40", &draw(40, 16, &th, &mut s));
    let ascii = Theme {
        g: Glyphs::ascii(),
        ..Theme::fixed()
    };
    let mut s = Screen::new("notes.txt", Ok(sample()), Opts::default()).with_mode(mode("notes.txt", ""));
    golden("view-code-plain-60", &draw(60, 16, &ascii, &mut s));
}

// ---------------------------------------------------------------- review-418

/// The lexer's state runs from one line to the next: a block comment's second
/// line is still comment.
#[test]
fn a_block_comment_spans_its_lines() {
    let rows = code::render("/* a\nb */ fn x", Some(Lang::Rust), 40);
    assert_eq!(rows[1].cells[0], ("b */".to_string(), Ink::of(Kind::Code(Role::Comment))));
    assert!(rows[1].cells.contains(&("fn".to_string(), Ink::of(Kind::Code(Role::Keyword)))));
}

#[test]
fn a_known_extension_beats_a_shebang() {
    assert_eq!(mode("x.rs", "#!/bin/sh\n"), Mode::Code(Some(Lang::Rust)));
    assert_eq!(
        mode("x.go", "#!/bin/sh\n"),
        Mode::Code(Some(Lang::Shell)),
        "an unknown one does not"
    );
}

/// `dash` is a shell, `env`'s `-u NAME` and `VAR=1` are not the program, and
/// `ts-node` runs TypeScript.
#[test]
fn more_shebangs() {
    let cases = [
        ("#!/bin/dash\n", Some(Lang::Shell)),
        ("#!/bin/ash\n", Some(Lang::Shell)),
        ("#!/bin/mksh\n", Some(Lang::Shell)),
        ("#!/usr/bin/env -u FOO python3\n", Some(Lang::Python)),
        ("#!/usr/bin/env --unset=FOO python3\n", Some(Lang::Python)),
        ("#!/usr/bin/env -C /tmp bash\n", Some(Lang::Shell)),
        ("#!/usr/bin/env VAR=1 bash\n", Some(Lang::Shell)),
        ("#!/usr/bin/env ts-node\n", Some(Lang::Js)),
        ("#!/usr/bin/env VAR=1\n", None),
    ];
    for (src, want) in cases {
        assert_eq!(mode("tool", src), Mode::Code(want), "{src:?}");
    }
}

/// tmux-open-path's `*.md` matches a file named just `.md`: so does the viewer.
#[test]
fn a_file_named_dot_md_is_markdown() {
    assert_eq!(mode(".md", ""), Mode::Markdown);
    assert_eq!(mode(".MARKDOWN", ""), Mode::Markdown);
    assert_eq!(mode("md", ""), Mode::Code(None), "no dot, no match");
}

/// Code has no links: `u` does nothing there and the help leaves it out.
#[test]
fn u_is_markdown_only() {
    let mut s = code("x.rs", RUST);
    let opts = s.opts;
    key(&mut s, KeyCode::Char('u'));
    assert_eq!(s.opts, opts, "no toggle");
    assert!(!mdview::help_for(false, Mode::Code(None)).iter().any(|(k, _)| k == "u"));
    assert!(mdview::help_for(false, Mode::Markdown).iter().any(|(k, _)| k == "u"));
    assert_eq!(mdview::help_for(false, Mode::Markdown), mdview::help_rows(false));
    let mut s = code("x.rs", RUST);
    key(&mut s, KeyCode::Char('h'));
    let out = draw(60, 30, &Theme::fixed(), &mut s);
    assert!(!out.contains("link addresses"), "{out}");
}
