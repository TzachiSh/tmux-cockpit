//! `refs` (#365): the shared code reference. Each test names one behaviour.
use cockpit_tui::refs::{Ref, RefError, parse, render};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// A temp tree (canonical root), removed when the test ends, pass or fail.
struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Tree {
        let tag = format!("cockpit-tui-refs-{name}-{}", std::process::id());
        let d = std::env::temp_dir().join(tag);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Tree(d.canonicalize().unwrap())
    }
    fn file(&self, rel: impl AsRef<Path>) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "x\n").unwrap();
        p.canonicalize().unwrap()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mk(p: &Path, lines: Option<(u32, u32)>, sym: Option<&str>) -> Ref {
    Ref::new(p, lines, sym.map(String::from)).unwrap()
}

#[test]
fn new_rejects_a_path_that_does_not_exist() {
    let t = Tree::new("missing");
    let e = Ref::new(t.0.join("nope"), None, None).unwrap_err();
    assert!(matches!(e, RefError::NotFound(_)), "{e:?}");
}

#[test]
fn new_rejects_the_empty_path() {
    assert!(matches!(Ref::new("", None, None), Err(RefError::NotFound(_))));
}

#[test]
fn new_rejects_line_zero_and_an_inverted_range() {
    let t = Tree::new("badlines");
    let f = t.file("a.rs");
    for (a, b) in [(0, 0), (0, 5), (5, 4)] {
        let e = Ref::new(&f, Some((a, b)), None).unwrap_err();
        assert!(matches!(e, RefError::Lines(x, y) if (x, y) == (a, b)), "{a}-{b}: {e:?}");
    }
}

#[test]
fn new_canonicalises_the_path() {
    let t = Tree::new("canon");
    let f = t.file("src/a.rs");
    let r = mk(&t.0.join("src/../src/./a.rs"), Some((1, 1)), None);
    assert_eq!(r.path, f);
}

#[test]
fn render_is_relative_inside_base_and_absolute_outside() {
    let t = Tree::new("relabs");
    let inside = t.file("proj/src/a.rs");
    let outside = t.file("other/b.rs");
    let base = t.0.join("proj");
    let out = render(&[mk(&inside, None, None), mk(&outside, None, None)], &base);
    assert_eq!(out, format!("src/a.rs\n{}", outside.display()));
}

#[test]
fn render_covers_single_line_range_none_and_symbol() {
    let t = Tree::new("forms");
    let f = t.file("src/server.go");
    let r = |l, s| mk(&f, l, s);
    let got = render(
        &[
            r(Some((40, 72)), Some("handleLogin")),
            r(Some((40, 40)), None),
            r(None, None),
            r(None, Some("main")),
        ],
        &t.0,
    );
    let want = "src/server.go:40-72 (handleLogin)\nsrc/server.go:40\n\
                src/server.go\nsrc/server.go (main)";
    assert_eq!(got, want);
}

#[test]
fn render_sanitises_a_symbol_with_a_paren_or_newline() {
    let t = Tree::new("sym");
    let f = t.file("a.rs");
    let one = |s| render(&[mk(&f, Some((1, 2)), Some(s))], &t.0);
    assert_eq!(one("f(x)"), "a.rs:1-2 (f(x])");
    assert_eq!(one("a\nb"), "a.rs:1-2 (a b)");
    assert_eq!(one("a\r\nb\u{1b}[31m"), "a.rs:1-2 (a b[31m)");
    assert_eq!(one("\n"), "a.rs:1-2");
}

#[test]
fn render_is_one_line_per_ref_whatever_the_symbol() {
    let t = Tree::new("oneline");
    let f = t.file("a.rs");
    let out = render(&[mk(&f, None, Some("x\ny\n\nz")), mk(&f, None, None)], &t.0);
    assert_eq!(out.lines().count(), 2, "{out:?}");
}

#[test]
fn render_drops_control_and_bidi_characters_from_the_text_only() {
    let t = Tree::new("ctl");
    let f = t.file("a\u{1b}[2J\u{202e}b.rs");
    let r = mk(&f, None, Some("s\u{2066}ym"));
    assert_eq!(render(std::slice::from_ref(&r), &t.0), "a[2Jb.rs (sym)");
    assert!(r.path.as_os_str().as_bytes().contains(&0x1b), "the Ref keeps its bytes");
}

#[test]
fn a_non_utf8_path_is_kept_byte_for_byte() {
    let t = Tree::new("bytes");
    let name = OsStr::from_bytes(b"caf\xe9.rs");
    let f = t.file(name);
    let r = mk(&f, Some((3, 3)), None);
    assert_eq!(r.path.file_name().unwrap().as_bytes(), b"caf\xe9.rs");
    assert!(render(&[r], &t.0).starts_with("caf"));
}

#[test]
fn parse_round_trips_render_in_both_forms() {
    let t = Tree::new("round");
    let a = t.file("proj/src/a.rs");
    let b = t.file("other/b.rs");
    let c = t.file("proj/c d.txt");
    let base = t.0.join("proj");
    let refs = vec![
        mk(&a, Some((40, 72)), Some("handleLogin")),
        mk(&a, Some((7, 7)), None),
        mk(&b, None, Some("main")),
        mk(&b, Some((1, 2)), None),
        mk(&c, None, None),
    ];
    assert_eq!(parse(&render(&refs, &base), &base), refs);
}

#[test]
fn parse_reads_a_path_that_looks_like_lines_or_a_symbol() {
    let t = Tree::new("tricky");
    let a = t.file("a:12");
    let b = t.file("b (x)");
    let c = t.file("c.rs");
    let refs = vec![
        mk(&a, None, None),
        mk(&a, Some((3, 4)), None),
        mk(&b, None, None),
        mk(&c, None, Some("f (g")),
    ];
    assert_eq!(parse(&render(&refs, &t.0), &t.0), refs);
}

#[test]
fn parse_skips_junk_missing_paths_and_bad_lines() {
    let t = Tree::new("junk");
    let f = t.file("a.rs");
    let input = format!(
        "\nnot a ref at all\nnope.rs:3\na.rs:0\na.rs:9-2\na.rs:x-y\n(sym)\n{}:5 (ok)\r\na.rs:2\n",
        f.display()
    );
    let want = vec![mk(&f, Some((5, 5)), Some("ok")), mk(&f, Some((2, 2)), None)];
    assert_eq!(parse(&input, &t.0), want);
}

#[test]
fn parse_of_empty_input_is_empty() {
    let t = Tree::new("empty");
    assert!(parse("", &t.0).is_empty());
}

#[test]
fn parse_prefers_lines_when_both_a_and_a_colon_12_exist() {
    let t = Tree::new("ambig");
    let a = t.file("a");
    t.file("a:12");
    assert_eq!(parse("a:12", &t.0), vec![mk(&a, Some((12, 12)), None)]);
}
