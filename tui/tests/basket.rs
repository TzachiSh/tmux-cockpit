//! The prompt basket (#365 child 5): refs gathered for a pane's agent. The
//! pure half: the option's text, the caps, de-duplication. No tmux here.
use cockpit_tui::basket::{self, MAX_BYTES, MAX_REFS};
use cockpit_tui::refs::Ref;
use std::path::PathBuf;

/// A scratch dir under the temp dir, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("cockpit-basket-{tag}-{}", std::process::id()));
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

fn r(p: &PathBuf, lines: Option<(u32, u32)>) -> Ref {
    Ref::new(p, lines, None).unwrap()
}

#[test]
fn the_text_reads_back_as_the_refs_it_holds() {
    let d = Dir::new("trip");
    let refs = vec![r(&d.file("src/a.rs"), Some((3, 9))), r(&d.file("b.md"), None)];
    let text = basket::body(&refs).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(text.lines().all(|l| l.starts_with('/')), "each line an absolute path: {text}");
    assert_eq!(basket::from_body(&text), refs);
}

#[test]
fn an_empty_basket_is_no_text() {
    assert_eq!(basket::body(&[]), Ok(String::new()));
    assert!(basket::from_body("").is_empty());
}

/// The same file and lines twice is one ref; other lines of it are another.
#[test]
fn a_ref_already_there_is_not_added_twice() {
    let d = Dir::new("dup");
    let f = d.file("a.rs");
    let (one, added) = basket::with_ref(&[], r(&f, Some((1, 2)))).unwrap();
    assert!(added);
    let (same, added) = basket::with_ref(&one, r(&f, Some((1, 2)))).unwrap();
    assert_eq!((same.len(), added), (1, false));
    let (two, added) = basket::with_ref(&same, r(&f, Some((3, 4)))).unwrap();
    assert_eq!((two.len(), added), (2, true));
}

#[test]
fn past_the_ref_cap_an_add_is_refused() {
    let d = Dir::new("cap");
    let f = d.file("a.rs");
    let full: Vec<Ref> = (1..=MAX_REFS as u32).map(|i| r(&f, Some((i, i)))).collect();
    let why = basket::with_ref(&full, r(&f, Some((99, 99)))).unwrap_err();
    assert!(why.contains(&MAX_REFS.to_string()), "{why}");
}

/// tmux takes one command of about 16 KB (probed on 3.8): the text stays under 12 KiB.
#[test]
fn past_the_byte_cap_an_add_is_refused() {
    let d = Dir::new("bytes");
    let deep = ["d", "e", "f"].map(|c| c.repeat(250)).join("/");
    let refs: Vec<Ref> = (0..20).map(|i| r(&d.file(&format!("{deep}/f{i}.rs")), None)).collect();
    let mut kept = vec![];
    let mut refused = None;
    for x in refs {
        match basket::with_ref(&kept, x) {
            Ok((k, _)) => kept = k,
            Err(e) => {
                refused = Some(e);
                break;
            }
        }
    }
    let why = refused.expect("an add past the cap");
    assert!(why.contains("too long"), "{why}");
    assert!(basket::body(&kept).unwrap().len() <= MAX_BYTES);
}

/// A name the text cannot carry (a line break, a control character) is refused:
/// it would read back as some other path, or none.
#[test]
fn a_name_the_text_cannot_carry_is_refused() {
    let d = Dir::new("name");
    for name in ["a\nb.rs", "a\u{1b}b.rs"] {
        let why = basket::with_ref(&[], r(&d.file(name), None)).unwrap_err();
        assert!(why.contains("cannot"), "{name:?}: {why}");
    }
}

/// A file gone since it was added is dropped when the basket is read.
#[test]
fn a_gone_file_drops_out_on_read() {
    let d = Dir::new("gone");
    let (a, b) = (d.file("a.rs"), d.file("b.rs"));
    let text = basket::body(&[r(&a, None), r(&b, Some((1, 1)))]).unwrap();
    std::fs::remove_file(&a).unwrap();
    assert_eq!(basket::from_body(&text), vec![r(&b, Some((1, 1)))]);
}
