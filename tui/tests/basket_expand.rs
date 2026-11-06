//! The send's ref expansion (#365 child 5, owner's D4): for a Claude Code pane,
//! `@path#La-b`, `@path#La`, `@path`, `@"path#La-b"` for a path with a space;
//! bare `path:a-b` for any other agent, and for a path holding `"`. Pure text.
use cockpit_tui::basket::{self, MAX_SEND};
use cockpit_tui::refs::Ref;
use std::path::PathBuf;

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("cockpit-expand-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d.canonicalize().unwrap())
    }

    fn file(&self, rel: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "x\n").unwrap();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn r(p: &PathBuf, l: Option<(u32, u32)>) -> Ref {
    Ref::new(p, l, None).unwrap()
}

/// One row per form: (the ref, for claude, for another agent).
#[test]
fn each_form_for_claude_and_for_another_agent() {
    let d = Dir::new("forms");
    let rows = [
        (
            "a range",
            r(&d.file("src/x.rs"), Some((40, 72))),
            "@src/x.rs#L40-72",
            "src/x.rs:40-72",
        ),
        ("one line", r(&d.file("src/x.rs"), Some((7, 7))), "@src/x.rs#L7", "src/x.rs:7"),
        ("a whole file", r(&d.file("README.md"), None), "@README.md", "README.md"),
        (
            "a space",
            r(&d.file("my dir/a b.rs"), Some((1, 2))),
            "@\"my dir/a b.rs#L1-2\"",
            "my dir/a b.rs:1-2",
        ),
        ("a quote", r(&d.file("q\"x.rs"), Some((3, 4))), "q\"x.rs:3-4", "q\"x.rs:3-4"),
        // a `#` could read as the range's start: bare, never a mention (review-534)
        ("a hash", r(&d.file("a#b.rs"), Some((1, 2))), "a#b.rs:1-2", "a#b.rs:1-2"),
    ];
    for (row, x, claude, other) in rows {
        assert_eq!(basket::ref_line(&x, &d.0, true), claude, "claude, {row}");
        assert_eq!(basket::ref_line(&x, &d.0, false), other, "another agent, {row}");
    }
}

/// A path outside the pane's folder goes absolute, in both forms.
#[test]
fn a_ref_outside_the_folder_is_absolute() {
    let (d, e) = (Dir::new("in"), Dir::new("out"));
    let x = r(&e.file("y.rs"), Some((1, 1)));
    let abs = e.0.join("y.rs").display().to_string();
    assert_eq!(basket::ref_line(&x, &d.0, true), format!("@{abs}#L1"));
    assert_eq!(basket::ref_line(&x, &d.0, false), format!("{abs}:1"));
}

/// The draft, a blank line, one ref per line; no refs leaves the draft as it is.
#[test]
fn the_text_is_the_draft_a_blank_line_then_the_refs() {
    let d = Dir::new("text");
    let refs = [r(&d.file("a.rs"), Some((1, 2))), r(&d.file("b.rs"), None)];
    assert_eq!(
        basket::expand("fix it\n", &refs, &d.0, true),
        Ok("fix it\n\n@a.rs#L1-2\n@b.rs".into())
    );
    assert_eq!(basket::expand("fix it", &[], &d.0, true), Ok("fix it".into()));
}

/// Over the cap the send is refused, never cut: a cut text would drop refs.
#[test]
fn a_text_over_the_cap_is_refused() {
    let d = Dir::new("cap");
    let draft = "w".repeat(MAX_SEND);
    let why = basket::expand(&draft, &[r(&d.file("a.rs"), None)], &d.0, true).unwrap_err();
    assert!(why.contains("too long"), "{why}");
    assert!(
        basket::expand(&"w".repeat(MAX_SEND), &[], &d.0, true).is_ok(),
        "exactly the cap passes"
    );
}
