//! The file browser (#363): what a directory lists, which key walks where, and
//! which line each leaf key prints. Scratch directories only; no tmux, no tty.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
#[allow(dead_code)] // for its Scratch guard only, not write_exec
mod fake;

use cockpit_tui::files::{self, Files};
use cockpit_tui::pick::{self, Outcome};
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fake::Scratch;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "cockpit-files-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(std::fs::canonicalize(&p).unwrap())
    }
    fn file(&self, rel: impl AsRef<OsStr>) -> PathBuf {
        let p = self.0.join(rel.as_ref());
        std::fs::write(&p, b"x").unwrap();
        p
    }
    fn dir(&self, rel: impl AsRef<OsStr>) -> PathBuf {
        let p = self.0.join(rel.as_ref());
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}

const BIG: Rect = Rect::new(0, 0, 80, 24);

fn browser(dir: &Path) -> Files {
    Files::new(dir, &Theme::fixed().g).unwrap()
}

fn press(f: &mut Files, code: KeyCode, m: KeyModifiers) -> Outcome {
    f.key(KeyEvent::new(code, m), BIG)
}

fn tap(f: &mut Files, code: KeyCode) -> Outcome {
    press(f, code, KeyModifiers::NONE)
}

fn type_str(f: &mut Files, s: &str) {
    for c in s.chars() {
        tap(f, KeyCode::Char(c));
    }
}

/// What the screen shows, in order.
fn shown(f: &Files) -> Vec<String> {
    f.pick.shown.iter().map(|&i| f.pick.rows[i].text.clone()).collect()
}

fn line(verb: &str, p: &Path) -> Outcome {
    Outcome::Choose([verb.as_bytes(), b" ", p.as_os_str().as_bytes()].concat())
}

// --- the list ----------------------------------------------------------------

#[test]
fn list_puts_directories_first_then_names_without_regard_to_case() {
    let s = Scratch::new();
    s.file("B.txt");
    s.dir("Zeta");
    s.file("a.txt");
    s.dir("alpha");
    let names: Vec<_> = files::list_dir(&s.0, false, false)
        .unwrap()
        .into_iter()
        .map(|e| e.is_dir)
        .zip(["alpha", "Zeta", "a.txt", "B.txt"])
        .collect();
    assert_eq!(names.iter().map(|(d, _)| *d).collect::<Vec<_>>(), [true, true, false, false]);
    assert_eq!(shown(&browser(&s.0)), ["alpha/", "Zeta/", "a.txt", "B.txt"]);
}

#[test]
fn list_hides_dotfiles_unless_asked() {
    let s = Scratch::new();
    s.file(".env");
    s.dir(".cache");
    s.file("main.rs");
    let plain = files::list_dir(&s.0, false, false).unwrap();
    assert_eq!(plain.len(), 1, "only main.rs");
    let all = files::list_dir(&s.0, true, false).unwrap();
    assert_eq!(all.len(), 3);
    assert!(all[0].is_dir, "the hidden directory still sorts first");
    s.dir(".git");
    assert_eq!(files::list_dir(&s.0, true, true).unwrap().len(), 3, ".git is never listed");
}

#[test]
fn a_symlink_to_a_directory_is_a_directory_and_a_broken_one_is_a_file() {
    let s = Scratch::new();
    let d = s.dir("real");
    std::os::unix::fs::symlink(&d, s.0.join("link")).unwrap();
    std::os::unix::fs::symlink(s.0.join("nowhere"), s.0.join("dangling")).unwrap();
    assert_eq!(shown(&browser(&s.0)), ["link/", "real/", "dangling"]);
}

#[test]
fn unreadable_or_missing_directory_is_an_error_naming_it() {
    let s = Scratch::new();
    let f = s.file("plain");
    for bad in [s.0.join("missing"), f] {
        let e = Files::new(&bad, &Theme::fixed().g).err().unwrap();
        assert!(e.contains(bad.to_str().unwrap()), "{e}");
    }
}

#[test]
fn an_empty_directory_lists_nothing_and_enter_does_nothing() {
    let s = Scratch::new();
    let mut f = browser(&s.0);
    assert!(shown(&f).is_empty());
    assert_eq!(tap(&mut f, KeyCode::Enter), Outcome::Continue);
}

// --- what the ignore files hide ----------------------------------------------

/// A repository whose .gitignore lists `target/` and `*.log`.
fn repo() -> Scratch {
    let s = Scratch::new();
    let ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(&s.0)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    std::fs::write(s.0.join(".gitignore"), "target/\n*.log\n").unwrap();
    s.dir("target");
    s.dir("src");
    s.file("run.log");
    s.file("main.rs");
    s
}

#[test]
fn ignored_entries_are_hidden_by_default_and_so_is_git() {
    let s = repo();
    assert_eq!(shown(&browser(&s.0)), ["src/", "main.rs"]);
    let mut f = browser(&s.0);
    tap(&mut f, KeyCode::Char('.'));
    assert_eq!(shown(&f), ["src/", ".gitignore", "main.rs"], "dotfiles show, .git never");
}

#[test]
fn alt_i_shows_ignored_entries_and_marks_them_and_a_second_one_hides_them() {
    let s = repo();
    let mut f = browser(&s.0);
    press(&mut f, KeyCode::Char('i'), KeyModifiers::ALT);
    assert_eq!(shown(&f), ["src/", "target/", "main.rs", "run.log"]);
    let mut ig: Vec<_> = f.ignored.iter().map(|k| String::from_utf8_lossy(k).into_owned()).collect();
    ig.sort();
    assert_eq!(ig, ["run.log", "target"]);
    press(&mut f, KeyCode::Char('i'), KeyModifiers::ALT);
    assert_eq!(shown(&f), ["src/", "main.rs"]);
    assert!(f.ignored.is_empty());
}

#[test]
fn a_dot_ignore_file_hides_too_and_a_bare_i_is_always_a_letter() {
    let s = repo();
    std::fs::write(s.0.join(".ignore"), "main.rs\n").unwrap();
    let mut f = browser(&s.0);
    assert_eq!(shown(&f), ["src/"]);
    type_str(&mut f, "i");
    assert_eq!((f.show_ignored, f.pick.query.as_str()), (false, "i"), "even on an empty filter");
}

#[test]
fn ignored_rows_are_drawn_dim_and_the_others_are_not() {
    let s = repo();
    let th = Theme::fixed();
    let mut f = browser(&s.0);
    press(&mut f, KeyCode::Char('i'), KeyModifiers::ALT);
    f.fit(60, &th.g);
    let mut t = Terminal::new(TestBackend::new(60, 16)).unwrap();
    t.draw(|fr| files::draw(fr, fr.area(), &f, &th)).unwrap();
    let buf = t.backend().buffer();
    let fg = |name: &str| {
        let y = (0..16)
            .find(|&y| (0..60).map(|x| buf[(x, y)].symbol()).collect::<String>().contains(name))
            .unwrap();
        let x = (0..60)
            .find(|&x| buf[(x, y)].symbol() == &name[..1] && buf[(x + 1, y)].symbol() == &name[1..2])
            .unwrap();
        buf[(x, y)].fg
    };
    assert_eq!(fg("target"), th.faint);
    assert_ne!(fg("main.rs"), th.faint);
    assert_ne!(fg("src"), th.faint);
}

#[test]
fn a_root_gitignore_applies_in_the_folders_below_it() {
    let s = repo();
    let src = s.0.join("src");
    std::fs::write(src.join("main.rs"), b"x").unwrap();
    std::fs::write(src.join("run.log"), b"x").unwrap();
    let names: Vec<_> = files::list_dir(&src, false, false).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, [b"main.rs".to_vec()]);
}

#[cfg(unix)]
#[test]
fn an_unreadable_grandchild_does_not_switch_the_ignore_rules_off() {
    use std::os::unix::fs::PermissionsExt;
    let s = repo();
    let locked = s.dir("a/locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let got: Vec<_> = files::list_dir(&s.0, false, false).unwrap().into_iter().map(|e| e.name).collect();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        got,
        [b"a".to_vec(), b"src".to_vec(), b"main.rs".to_vec()],
        "run.log and target stay hidden"
    );
}

#[test]
fn the_footer_names_the_key_and_its_state() {
    let g = Theme::fixed().g;
    let flat = |h: bool, i: bool| {
        files::hints(&g, 120, h, i)
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join("|")
    };
    assert!(flat(false, false).contains("alt-i ignored off"));
    assert!(flat(false, true).contains("alt-i ignored on"));
    assert!(flat(true, false).contains(". dotfiles on"));
    let wide = flat(false, false);
    assert!(wide.contains("view"), "{wide}");
    assert!(wide.contains("alt-e edit"), "{wide}");
    assert!(!wide.contains(" open"), "Enter no longer says open: {wide}");
}

#[test]
fn alt_e_stays_in_the_footer_at_popup_widths() {
    let g = Theme::fixed().g;
    for w in [60, 70, 84] {
        let h = files::hints(&g, w, false, false);
        assert!(h.iter().any(|(k, v)| k == "alt-e" && v == "edit"), "width {w}: {h:?}");
    }
}

// --- alt-a: the prompt basket (#365 child 5) ----------------------------------

#[test]
fn alt_a_on_a_file_prints_add_and_its_path() {
    let s = Scratch::new();
    let p = s.file("a.rs");
    let mut f = browser(&s.0);
    assert_eq!(press(&mut f, KeyCode::Char('a'), KeyModifiers::ALT), line("add", &p));
}

/// A folder is refused, with a note: the basket holds files and their lines.
#[test]
fn alt_a_on_a_folder_is_refused_with_a_note() {
    let s = Scratch::new();
    s.dir("src");
    let mut f = browser(&s.0);
    assert_eq!(press(&mut f, KeyCode::Char('a'), KeyModifiers::ALT), Outcome::Continue);
    assert!(
        f.pick.header.starts_with('✕') && f.pick.header.contains("folder"),
        "{}",
        f.pick.header
    );
}

#[test]
fn the_footer_and_the_help_offer_alt_a() {
    let g = Theme::fixed().g;
    let h = files::hints(&g, 120, false, false);
    assert!(h.iter().any(|(k, v)| k == "alt-a" && v == "add"), "{h:?}");
    let rows = files::help(&g);
    assert!(rows.iter().any(|(k, v)| k == "alt-a" && v.contains("basket")), "{rows:?}");
    // the help lists it where the footer does, right after alt-c
    let at = |k: &str| rows.iter().position(|(key, _)| key == k);
    assert_eq!(at("alt-a"), at("alt-c").map(|i| i + 1), "{rows:?}");
}

#[test]
fn the_narrow_footer_keeps_enter_as_view() {
    let g = Theme::fixed().g;
    let h = files::hints(&g, 8, false, false);
    assert_eq!(h[0], (g.key_enter.to_string(), "view".to_string()));
}

#[test]
fn help_names_enter_as_view_and_alt_e_as_open_in_the_editor() {
    let g = Theme::fixed().g;
    let rows = files::help(&g);
    let row = |k: &str| rows.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    assert!(row(g.key_enter).unwrap().starts_with("view"), "{rows:?}");
    assert!(row("alt-e").unwrap().contains("editor"), "{rows:?}");
}

// --- drawn names are text for a person ---------------------------------------

#[test]
fn drawn_names_drop_control_and_bidi_characters_but_the_key_keeps_them() {
    let s = Scratch::new();
    let name = "a\x1b[31mb\u{202E}c\u{2066}d\u{061C}\te.txt";
    s.file(name);
    let f = browser(&s.0);
    assert_eq!(shown(&f), ["a[31mbcd e.txt"]);
    assert_eq!(f.pick.rows[0].key, name.as_bytes(), "the key is the exact name");
}

#[test]
fn a_name_that_is_not_utf8_prints_its_exact_bytes() {
    let s = Scratch::new();
    let p = s.file(OsStr::from_bytes(b"caf\xe9.txt"));
    let mut f = browser(&s.0);
    assert_eq!(tap(&mut f, KeyCode::Enter), line("view", &p));
}

#[test]
fn a_name_with_a_newline_cannot_ride_one_line_so_it_prints_nothing() {
    let s = Scratch::new();
    s.file("two\nlines.txt");
    let mut f = browser(&s.0);
    assert_eq!(tap(&mut f, KeyCode::Enter), Outcome::Continue);
    assert!(f.pick.header.starts_with('✕'), "{}", f.pick.header);
    assert_eq!(
        press(&mut f, KeyCode::Char('c'), KeyModifiers::ALT),
        Outcome::Continue,
        "copy is a line too"
    );
    assert_eq!(press(&mut f, KeyCode::Char('a'), KeyModifiers::ALT), Outcome::Continue, "and add");
    // the next key clears the note
    tap(&mut f, KeyCode::Down);
    assert!(!f.pick.header.starts_with('✕'));
}

#[test]
fn a_name_with_a_control_or_bidi_character_is_refused_for_view_open_and_copy() {
    let names = [
        "a\u{1b}b.rs",
        "a\tb.rs",
        "a\u{7f}b.rs",
        "a\u{85}b.rs",
        "a\u{202e}b.rs",
        "a\u{2066}b.rs",
        "a\u{2069}b.rs",
        "a\u{61c}b.rs",
        "a\u{200e}b.rs",
        "a\u{200f}b.rs",
    ];
    for name in names {
        let s = Scratch::new();
        s.file(name);
        let mut f = browser(&s.0);
        for (label, k) in [
            ("view", (KeyCode::Enter, KeyModifiers::NONE)),
            ("open", (KeyCode::Char('e'), KeyModifiers::ALT)),
            ("copy", (KeyCode::Char('c'), KeyModifiers::ALT)),
            ("add", (KeyCode::Char('a'), KeyModifiers::ALT)),
        ] {
            assert_eq!(press(&mut f, k.0, k.1), Outcome::Continue, "{label} {name:?}");
            assert!(f.pick.header.starts_with('✕'), "{label} {name:?}: {}", f.pick.header);
        }
    }
}

#[test]
fn a_folder_with_a_control_name_can_still_be_entered_and_left() {
    let s = Scratch::new();
    let d = s.dir("d\u{1b}x");
    let mut f = browser(&s.0);
    assert_eq!(tap(&mut f, KeyCode::Enter), Outcome::Continue);
    assert_eq!(f.dir, d);
    assert!(!f.pick.header.starts_with('✕'), "{}", f.pick.header);
    tap(&mut f, KeyCode::Left);
    assert_eq!(f.dir, s.0);
}

#[test]
fn a_plain_neighbour_of_a_control_name_still_prints() {
    let s = Scratch::new();
    s.file("a\u{1b}b.rs");
    let p = s.file("ok.rs");
    let mut f = browser(&s.0);
    type_str(&mut f, "ok.rs");
    assert_eq!(tap(&mut f, KeyCode::Enter), line("view", &p));
}

// --- keys --------------------------------------------------------------------

#[test]
fn enter_on_a_file_prints_view_and_its_absolute_path() {
    let s = Scratch::new();
    s.dir("d");
    let p = s.file("notes.md");
    let mut f = browser(&s.0);
    tap(&mut f, KeyCode::Down);
    assert_eq!(tap(&mut f, KeyCode::Enter), line("view", &p));
}

#[test]
fn alt_e_on_a_file_prints_open_and_its_absolute_path() {
    let s = Scratch::new();
    s.dir("d");
    let p = s.file("main.rs");
    let mut f = browser(&s.0);
    tap(&mut f, KeyCode::Down);
    assert_eq!(press(&mut f, KeyCode::Char('e'), KeyModifiers::ALT), line("open", &p));
}

#[test]
fn alt_e_on_a_folder_and_a_plain_e_print_nothing() {
    let s = Scratch::new();
    s.dir("sub");
    let mut f = browser(&s.0);
    assert_eq!(press(&mut f, KeyCode::Char('e'), KeyModifiers::ALT), Outcome::Continue);
    assert_eq!(tap(&mut f, KeyCode::Char('e')), Outcome::Continue);
    assert_eq!(f.pick.query, "e", "a plain e is the filter");
}

#[test]
fn alt_e_on_a_newline_name_says_so_and_prints_nothing() {
    let s = Scratch::new();
    s.file("two\nlines.txt");
    let mut f = browser(&s.0);
    assert_eq!(press(&mut f, KeyCode::Char('e'), KeyModifiers::ALT), Outcome::Continue);
    assert!(f.pick.header.starts_with('✕'), "{}", f.pick.header);
}

#[test]
fn enter_on_a_directory_goes_into_it_and_clears_the_filter() {
    let s = Scratch::new();
    let d = s.dir("sub");
    s.file("other.txt");
    let inner = d.join("in.txt");
    std::fs::write(&inner, b"x").unwrap();
    let mut f = browser(&s.0);
    type_str(&mut f, "sub");
    assert_eq!(tap(&mut f, KeyCode::Enter), Outcome::Continue);
    assert_eq!(f.dir, d);
    assert_eq!(shown(&f), ["in.txt"]);
    assert!(f.pick.query.is_empty());
    assert_eq!(tap(&mut f, KeyCode::Enter), line("view", &inner));
}

#[test]
fn left_goes_up_and_lands_on_the_directory_it_left() {
    let s = Scratch::new();
    s.dir("a");
    let b = s.dir("b");
    s.dir("c");
    let mut f = browser(&b);
    assert_eq!(tap(&mut f, KeyCode::Left), Outcome::Continue);
    assert_eq!(f.dir, s.0);
    assert_eq!(shown(&f), ["a/", "b/", "c/"]);
    assert_eq!(f.pick.sel, 1, "back on b");
}

#[test]
fn backspace_on_an_empty_filter_goes_up_but_with_a_filter_it_edits_it() {
    let s = Scratch::new();
    let d = s.dir("d");
    s.file("d/xy.txt");
    let mut f = browser(&d);
    type_str(&mut f, "xy");
    tap(&mut f, KeyCode::Backspace);
    assert_eq!((f.dir.clone(), f.pick.query.as_str()), (d.clone(), "x"));
    tap(&mut f, KeyCode::Backspace);
    assert_eq!(f.dir, d, "that one emptied the filter");
    tap(&mut f, KeyCode::Backspace);
    assert_eq!(f.dir, s.0, "an empty filter: up");
}

#[test]
fn up_stops_at_the_filesystem_root() {
    let mut f = browser(Path::new("/"));
    tap(&mut f, KeyCode::Left);
    tap(&mut f, KeyCode::Backspace);
    assert_eq!(f.dir, Path::new("/"));
}

#[test]
fn dot_on_an_empty_filter_toggles_hidden_files_and_keeps_the_selection() {
    let s = Scratch::new();
    s.file(".a");
    s.file("m.txt");
    s.file("z.txt");
    let mut f = browser(&s.0);
    tap(&mut f, KeyCode::Down);
    assert_eq!(shown(&f), ["m.txt", "z.txt"]);
    tap(&mut f, KeyCode::Char('.'));
    assert_eq!(shown(&f), [".a", "m.txt", "z.txt"]);
    assert_eq!(f.pick.sel, 2, "still on z.txt");
    tap(&mut f, KeyCode::Char('.'));
    assert_eq!(shown(&f), ["m.txt", "z.txt"]);
}

#[test]
fn dot_inside_a_filter_is_a_letter_of_it() {
    let s = Scratch::new();
    s.file("a.txt");
    s.file("b.md");
    let mut f = browser(&s.0);
    type_str(&mut f, "b.m");
    assert_eq!((shown(&f), f.hidden), (vec!["b.md".to_string()], false));
}

#[test]
fn hidden_stays_on_when_walking_into_and_out_of_a_directory() {
    let s = Scratch::new();
    let d = s.dir("d");
    s.file("d/.secret");
    let mut f = browser(&s.0);
    tap(&mut f, KeyCode::Char('.'));
    tap(&mut f, KeyCode::Enter);
    assert_eq!(f.dir, d);
    assert_eq!(shown(&f), [".secret"]);
}

#[test]
fn alt_c_prints_copy_for_a_file_and_for_a_directory() {
    let s = Scratch::new();
    let d = s.dir("d");
    let p = s.file("f.txt");
    let mut f = browser(&s.0);
    assert_eq!(press(&mut f, KeyCode::Char('c'), KeyModifiers::ALT), line("copy", &d));
    tap(&mut f, KeyCode::Down);
    assert_eq!(press(&mut f, KeyCode::Char('c'), KeyModifiers::ALT), line("copy", &p));
}

#[test]
fn plain_c_is_a_letter_not_a_copy() {
    let s = Scratch::new();
    s.file("cat");
    let mut f = browser(&s.0);
    assert_eq!(tap(&mut f, KeyCode::Char('c')), Outcome::Continue);
    assert_eq!(f.pick.query, "c");
}

#[test]
fn esc_cancels_and_a_cancel_prints_nothing() {
    let s = Scratch::new();
    s.file("f");
    let mut f = browser(&s.0);
    let o = tap(&mut f, KeyCode::Esc);
    assert_eq!(o, Outcome::Cancel);
    let mut out = Vec::new();
    assert_eq!(pick::answer(&mut out, &o), 1);
    assert!(out.is_empty());
}

#[cfg(unix)]
#[test]
fn a_directory_that_cannot_be_read_stays_put_with_a_note() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new();
    let d = s.dir("locked");
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&d).is_ok() {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        return; // root reads everything
    }
    let mut f = browser(&s.0);
    assert_eq!(tap(&mut f, KeyCode::Enter), Outcome::Continue);
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(f.dir, s.0);
    assert!(f.pick.header.starts_with('✕'), "{}", f.pick.header);
}

// --- the screen --------------------------------------------------------------

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

fn draw(w: u16, h: u16, f: &mut Files) -> String {
    let th = Theme::fixed();
    f.label = "/work/demo".into();
    f.fit(w, &th.g);
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|fr| files::draw(fr, fr.area(), f, &th)).unwrap();
    t.backend().to_string()
}

fn demo() -> (Scratch, Files) {
    let s = Scratch::new();
    s.file(".hidden");
    s.file("README.md");
    s.dir("src");
    s.dir("docs");
    s.file("長い名前.txt");
    let f = browser(&s.0);
    (s, f)
}

#[test]
fn goldens() {
    for (w, h) in [(40, 14), (60, 16), (80, 24), (84, 24)] {
        let (_s, mut f) = demo();
        golden(&format!("files-{w}"), &draw(w, h, &mut f));
    }
    let (_s, mut f) = demo();
    tap(&mut f, KeyCode::Char('.'));
    golden("files-hidden", &draw(60, 16, &mut f));
    let s = Scratch::new();
    let mut e = browser(&s.0);
    golden("files-empty", &draw(60, 16, &mut e));
    let (_s, mut f) = demo();
    golden("files-floor", &draw(24, 8, &mut f));
}

#[test]
fn a_long_path_is_cut_on_the_left_so_the_folder_name_stays() {
    let (_s, mut f) = demo();
    let th = Theme::fixed();
    f.label = "/home/someone/projects/a/very/deep/tree/target-folder".into();
    f.fit(30, &th.g);
    assert!(f.pick.header.ends_with("target-folder"), "{}", f.pick.header);
    assert!(f.pick.header.starts_with('…'), "{}", f.pick.header);
}

#[test]
fn the_label_drops_control_and_bidi_characters() {
    let s = Scratch::new();
    let d = s.dir("x\x1b]0;pwn\x07\u{202E}y");
    let f = browser(&d);
    assert!(f.label.ends_with("/x]0;pwny"), "{}", f.label);
}

// --- the command line --------------------------------------------------------

fn run(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn the_command_wants_exactly_one_existing_directory() {
    let s = Scratch::new();
    let file = s.file("f");
    let nope = s.0.join("nope");
    let rows = [
        (vec!["files"], "needs exactly one folder"),
        (vec!["files", s.0.to_str().unwrap(), "extra"], "needs exactly one folder"),
        (vec!["files", nope.to_str().unwrap()], "cannot read"),
        (vec!["files", file.to_str().unwrap()], "cannot read"),
    ];
    for (args, why) in rows {
        let o = run(&args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(o.stdout.is_empty(), "{args:?}");
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(e.contains("cockpit-tui: files: ") && e.contains(why), "{args:?}: {e}");
    }
}

#[test]
fn usage_names_the_files_screen() {
    let o = run(&["--help"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("files <dir>"));
}
