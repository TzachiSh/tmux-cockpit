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
    assert_eq!(r.path(), f);
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
    assert!(r.path().as_os_str().as_bytes().contains(&0x1b), "the Ref keeps its bytes");
}

#[test]
fn a_non_utf8_path_is_kept_byte_for_byte() {
    let t = Tree::new("bytes");
    let name = OsStr::from_bytes(b"caf\xe9.rs");
    let f = t.file(name);
    let r = mk(&f, Some((3, 3)), None);
    assert_eq!(r.path().file_name().unwrap().as_bytes(), b"caf\xe9.rs");
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

#[test]
fn parse_does_not_read_a_dir_with_a_paren_in_its_name_as_a_symbol() {
    let t = Tree::new("dirparen");
    t.file("report");
    let f = t.file("report (1)/f.txt");
    let r = mk(&f, None, Some("y"));
    let text = render(std::slice::from_ref(&r), &t.0);
    assert_eq!(text, "report (1)/f.txt (y)");
    assert_eq!(parse(&text, &t.0), vec![r]);
}

#[test]
fn parse_prefers_a_symbol_when_both_a_and_a_paren_x_exist() {
    let t = Tree::new("ambigsym");
    let a = t.file("a");
    t.file("a (x)");
    assert_eq!(parse("a (x)", &t.0), vec![mk(&a, None, Some("x"))]);
}

#[test]
fn new_separates_an_io_error_from_a_missing_path() {
    let t = Tree::new("io");
    let f = t.file("f");
    let e = Ref::new(f.join("x"), None, None).unwrap_err();
    assert!(matches!(e, RefError::Io(_, std::io::ErrorKind::NotADirectory)), "{e:?}");
    assert_eq!(parse(&format!("{}/x", f.display()), &t.0), vec![]);
}

#[test]
fn render_makes_a_path_relative_to_a_base_given_through_a_symlink() {
    let t = Tree::new("baselink");
    let f = t.file("real/src/a.rs");
    std::os::unix::fs::symlink(t.0.join("real"), t.0.join("link")).unwrap();
    let out = render(&[mk(&f, None, None)], &t.0.join("link"));
    assert_eq!(out, "src/a.rs");
}

#[test]
fn a_symlinked_file_resolves_to_its_target() {
    let t = Tree::new("filelink");
    let f = t.file("real.rs");
    std::os::unix::fs::symlink(&f, t.0.join("alias.rs")).unwrap();
    let r = mk(&t.0.join("alias.rs"), None, None);
    assert_eq!(r.path(), f);
    assert_eq!(parse("alias.rs:3", &t.0), vec![mk(&f, Some((3, 3)), None)]);
}

#[test]
fn render_trims_the_symbol_and_drops_unicode_line_separators() {
    let t = Tree::new("symtrim");
    let f = t.file("a.rs");
    let one = |s| render(&[mk(&f, None, Some(s))], &t.0);
    assert_eq!(one("  x  "), "a.rs (x)");
    assert_eq!(one("a\u{2028}b\u{2029}c"), "a.rs (a b c)");
}

#[test]
fn a_ref_to_base_itself_renders_dot_and_parses_back() {
    let t = Tree::new("basedot");
    let r = mk(&t.0, None, None);
    assert_eq!(render(std::slice::from_ref(&r), &t.0), ".");
    assert_eq!(parse(".", &t.0), vec![r]);
}

#[test]
fn parse_accepts_paths_outside_base() {
    let t = Tree::new("outside");
    let f = t.file("other/b.rs");
    let base = t.0.join("proj");
    std::fs::create_dir_all(&base).unwrap();
    assert_eq!(parse("../other/b.rs:2", &base), vec![mk(&f, Some((2, 2)), None)]);
    assert_eq!(parse(&f.display().to_string(), &base), vec![mk(&f, None, None)]);
}

#[test]
fn accessors_read_the_fields() {
    let t = Tree::new("acc");
    let f = t.file("a.rs");
    let r = mk(&f, Some((1, 2)), Some("s"));
    assert_eq!((r.path(), r.lines(), r.symbol()), (f.as_path(), Some((1, 2)), Some("s")));
}

// ---------------------------------------------------------------- #381's NITs

/// An all-whitespace symbol is no symbol.
#[test]
fn a_blank_symbol_is_none() {
    let t = Tree::new("blanksym");
    let f = t.file("a.rs");
    for s in ["", " ", "\t \n"] {
        assert_eq!(mk(&f, None, Some(s)).symbol(), None, "{s:?}");
    }
    assert_eq!(mk(&f, None, Some(" x ")).symbol(), Some(" x "));
}

/// A Ref can go in a set (#365 child 5's basket).
#[test]
fn a_ref_hashes() {
    let t = Tree::new("hash");
    let f = t.file("a.rs");
    let (whole, line) = (mk(&f, None, None), mk(&f, Some((1, 1)), None));
    let set: std::collections::HashSet<Ref> = [whole.clone(), whole, line].into();
    assert_eq!(set.len(), 2);
}
