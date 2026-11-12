//! `cockpit-tui files <dir> --select <name>`: the browser reopened after the
//! viewer closes starts on the file that was viewed. Scratch folders only; no
//! tmux, no tty.
#[allow(dead_code)] // for its Scratch guard only, not write_exec
mod fake;

use cockpit_tui::files::{self, Files};
use cockpit_tui::pick::Outcome;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fake::Scratch;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

impl Scratch {
    fn new(files: &[&str]) -> Self {
        let p = std::env::temp_dir().join(format!(
            "cockpit-files-select-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        let s = Self(std::fs::canonicalize(&p).unwrap());
        for f in files {
            std::fs::write(s.0.join(f), b"x").unwrap();
        }
        s
    }
}

/// What Enter prints: the selected row, as the browser would hand it on.
fn enter(f: &mut Files) -> Outcome {
    f.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), Rect::new(0, 0, 80, 24))
}

fn view(p: &Path) -> Outcome {
    Outcome::Choose([b"view ", p.as_os_str().as_bytes()].concat())
}

fn opened(s: &Scratch, name: &[u8]) -> Files {
    let mut f = Files::new(&s.0, &Theme::fixed().g).unwrap();
    f.select(name);
    f
}

#[test]
fn select_starts_on_the_named_file() {
    let s = Scratch::new(&["a.txt", "b.txt", "c.txt"]);
    let mut f = opened(&s, b"c.txt");
    assert_eq!(enter(&mut f), view(&s.0.join("c.txt")));
}

#[test]
fn a_name_not_in_the_folder_starts_on_the_first_row() {
    let s = Scratch::new(&["a.txt", "b.txt"]);
    let mut f = opened(&s, b"gone.txt");
    assert_eq!(enter(&mut f), view(&s.0.join("a.txt")));
}

#[test]
fn a_dotfile_turns_the_dotfiles_on_to_select_it() {
    let s = Scratch::new(&["a.txt", ".env"]);
    let mut f = opened(&s, b".env");
    assert!(f.hidden);
    assert_eq!(enter(&mut f), view(&s.0.join(".env")));
}

#[test]
fn an_ignored_file_turns_the_ignored_on_to_select_it() {
    let s = Scratch::new(&["a.txt", "run.log"]);
    let git = std::process::Command::new("git").args(["init", "-q"]).arg(&s.0).status();
    assert!(git.unwrap().success());
    std::fs::write(s.0.join(".gitignore"), "*.log\n").unwrap();
    let mut f = opened(&s, b"run.log");
    assert!(f.show_ignored);
    assert_eq!(enter(&mut f), view(&s.0.join("run.log")));
}

#[test]
fn a_plain_name_leaves_the_dotfiles_and_the_ignored_off() {
    let s = Scratch::new(&["a.txt", "b.txt"]);
    let f = opened(&s, b"b.txt");
    assert_eq!((f.hidden, f.show_ignored), (false, false));
}

/// What `parse_args` gives for a row: the folder and the name, or a refusal.
type Parsed = Result<(String, Option<String>), ()>;

/// The folder and the name `parse_args` took, the parts this table is about.
fn dir_and_name(a: files::Args) -> (String, Option<String>) {
    (a.dir, a.select)
}

/// The arguments `main` takes, each row named: the old one-folder form still
/// parses, and `--select` takes one entry name, never a path or a control.
#[test]
fn the_arguments_table() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let ok = |d: &str, n: Option<&str>| Ok((d.to_string(), n.map(str::to_string)));
    let rows: &[(&str, Vec<String>, Parsed)] = &[
        ("one folder, the old form", a(&["/d"]), ok("/d", None)),
        ("a folder and a name", a(&["/d", "--select", "x.md"]), ok("/d", Some("x.md"))),
        ("a dotfile name", a(&["/d", "--select", ".env"]), ok("/d", Some(".env"))),
        ("no argument", a(&[]), Err(())),
        ("--select without a name", a(&["/d", "--select"]), Err(())),
        ("an empty name", a(&["/d", "--select", ""]), Err(())),
        ("a slash in the name", a(&["/d", "--select", "a/b"]), Err(())),
        ("a bare slash", a(&["/d", "--select", "/"]), Err(())),
        ("an escape in the name", a(&["/d", "--select", "a\x1bb"]), Err(())),
        ("a newline in the name", a(&["/d", "--select", "a\nb"]), Err(())),
        ("a C1 control in the name", a(&["/d", "--select", "a\u{85}b"]), Err(())),
        ("another flag", a(&["/d", "--pick", "x"]), Err(())),
        ("two folders", a(&["/d", "/e"]), Err(())),
        ("a name and more", a(&["/d", "--select", "x", "y"]), Err(())),
    ];
    for (name, args, want) in rows {
        let got = files::parse_args(args).map(dir_and_name).map_err(|_| ());
        assert_eq!(&got, want, "{name}");
    }
}

/// A refused name is named in the error: `--select` and what it takes. (`main`
/// exits 2 on it, as it does for no terminal, so its code alone proves nothing.)
#[test]
fn a_refused_name_names_the_argument() {
    let e = files::parse_args(&["/d".into(), "--select".into(), "a/b".into()]).unwrap_err();
    assert!(e.contains("--select") && e.contains("one name"), "{e}");
}

/// The rows the first frame draws for a screen `w` by `h`, laid out as the
/// run loop does it: the header first, then the scroll.
fn first_frame(f: &mut Files, w: u16, h: u16) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    f.frame(&mut t, &Theme::fixed()).unwrap();
    rows_of(&t, w, h)
}

fn rows_of(t: &Terminal<TestBackend>, w: u16, h: u16) -> Vec<String> {
    let b = t.backend().buffer();
    (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol()).collect()).collect()
}

/// Whether `line` is the list row of `name`, selected or not: the marker column, a space, the
/// name, then padding. The whole line must match: the header above the list shows the temp
/// path, which can hold any name (#908).
fn shows(line: &str, name: &str) -> bool {
    [" ", "▎"].iter().any(|mark| line.trim_end() == format!("{mark} {name}"))
}

fn names(r: std::ops::Range<u32>) -> Vec<String> {
    r.map(|i| format!("f{i}.txt")).collect()
}

/// Back on a file inside the first screen, the list keeps its natural scroll:
/// the folder's first row is drawn, and so is the file.
#[test]
fn a_selection_on_the_first_screen_keeps_the_rows_above_it() {
    let n = names(10..40);
    let s = Scratch::new(&n.iter().map(String::as_str).collect::<Vec<_>>());
    let mut f = opened(&s, b"f12.txt");
    let rows = first_frame(&mut f, 80, 24);
    for name in ["f10.txt", "f12.txt"] {
        assert!(rows.iter().any(|r| shows(r, name)), "no list row shows {name:?}: {rows:#?}");
    }
    assert_eq!(enter(&mut f), view(&s.0.join("f12.txt")));
}

/// A file past the first screen scrolls the list only as far as it takes to
/// show it: the first frame draws it, on the last list row, not the top.
#[test]
fn a_selection_past_the_first_screen_is_drawn_on_the_first_frame() {
    let n = names(10..90);
    let s = Scratch::new(&n.iter().map(String::as_str).collect::<Vec<_>>());
    let mut f = opened(&s, b"f60.txt");
    let rows = first_frame(&mut f, 80, 24);
    let at = rows.iter().position(|r| r.trim_end() == "▎ f60.txt");
    let last = rows.iter().rposition(|r| n.iter().any(|name| shows(r, name)));
    assert!(at.is_some() && at == last, "f60 at {at:?}, last row {last:?}: {rows:#?}");
    assert!(!rows.iter().any(|r| shows(r, "f10.txt")), "scrolled: {rows:#?}");
}

/// A key leaves the screen laid out for its frame (#601): the header first,
/// then the scroll, so a draw straight after it still shows the selection.
#[test]
fn a_key_leaves_the_selection_on_the_screen_it_drew() {
    let n = names(10..90);
    let s = Scratch::new(&n.iter().map(String::as_str).collect::<Vec<_>>());
    let mut f = opened(&s, b"f60.txt");
    f.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), Rect::new(0, 0, 80, 24));
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
    t.draw(|fr| files::draw(fr, fr.area(), &f, &Theme::fixed())).unwrap();
    let rows = rows_of(&t, 80, 24);
    assert!(rows.iter().any(|r| r.trim_end() == "▎ f61.txt"), "{rows:#?}");
}
