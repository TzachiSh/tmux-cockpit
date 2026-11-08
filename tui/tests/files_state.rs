//! `cockpit-tui files` keeps its state across a view and back (#574): it says
//! its dotfiles toggle, its ignored toggle and its filter beside the line it
//! prints, and takes them back as `--hidden`, `--ignored` and `--filter`.
//! Scratch folders only; no tmux, no tty.
#[allow(dead_code)] // for its Scratch guard only, not write_exec
mod fake;

use cockpit_tui::files::{self, Args, Files};
use cockpit_tui::pick::Outcome;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fake::Scratch;
use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

impl Scratch {
    fn new(files: &[&str]) -> Self {
        let p = std::env::temp_dir().join(format!(
            "cockpit-files-state-{}-{}",
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

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The browser as `main` starts it on `argv` (the folder first).
fn started(s: &Scratch, flags: &[&str]) -> Files {
    let mut v = vec![s.0.to_str().unwrap()];
    v.extend_from_slice(flags);
    let a = files::parse_args(&args(&v)).unwrap();
    let mut f = Files::new(&s.0, &Theme::fixed().g).unwrap();
    f.restore(&a);
    f
}

fn tap(f: &mut Files, c: KeyCode, m: KeyModifiers) -> Outcome {
    f.key(KeyEvent::new(c, m), ratatui::layout::Rect::new(0, 0, 80, 24))
}

fn shown(f: &Files) -> Vec<String> {
    f.pick.shown.iter().map(|&i| f.pick.rows[i].text.clone()).collect()
}

fn state(f: &Files) -> String {
    String::from_utf8(f.state_line()).unwrap()
}

#[test]
fn hidden_lists_the_dotfiles() {
    let s = Scratch::new(&["a.txt", ".env"]);
    let f = started(&s, &["--hidden"]);
    assert_eq!((f.hidden, shown(&f)), (true, vec![".env".to_string(), "a.txt".to_string()]));
}

#[test]
fn ignored_lists_what_the_ignore_files_hide() {
    let s = Scratch::new(&["a.txt", "run.log"]);
    let git = std::process::Command::new("git").args(["init", "-q"]).arg(&s.0).status();
    assert!(git.unwrap().success());
    std::fs::write(s.0.join(".gitignore"), "*.log\n").unwrap();
    let f = started(&s, &["--ignored"]);
    assert_eq!(
        (f.show_ignored, shown(&f)),
        (true, vec!["a.txt".to_string(), "run.log".to_string()])
    );
}

#[test]
fn filter_is_typed_back_in() {
    let s = Scratch::new(&["a1.txt", "b1.txt", "a2.txt"]);
    let f = started(&s, &["--filter", "a"]);
    assert_eq!(
        (f.pick.query.as_str(), shown(&f)),
        ("a", vec!["a1.txt".to_string(), "a2.txt".to_string()])
    );
}

#[test]
fn filter_and_select_start_on_the_file_in_the_filtered_rows() {
    let s = Scratch::new(&["a1.txt", "b1.txt", "a2.txt"]);
    let mut f = started(&s, &["--select", "a2.txt", "--filter", "a"]);
    let want = [b"view ", s.0.join("a2.txt").as_os_str().as_bytes()].concat();
    assert_eq!(tap(&mut f, KeyCode::Enter, KeyModifiers::NONE), Outcome::Choose(want));
}

#[test]
fn a_selected_name_the_filter_hides_starts_on_the_first_filtered_row() {
    let s = Scratch::new(&["a1.txt", "b1.txt", "a2.txt"]);
    let mut f = started(&s, &["--select", "b1.txt", "--filter", "a"]);
    let want = [b"view ", s.0.join("a1.txt").as_os_str().as_bytes()].concat();
    assert_eq!(tap(&mut f, KeyCode::Enter, KeyModifiers::NONE), Outcome::Choose(want));
}

/// The viewed file deleted meanwhile: the toggles still list what they show.
#[test]
fn a_selected_name_gone_from_the_folder_still_lists_the_dotfiles() {
    let s = Scratch::new(&["a.txt", ".env"]);
    let f = started(&s, &["--select", "gone.txt", "--hidden"]);
    assert_eq!((f.hidden, shown(&f)), (true, vec![".env".to_string(), "a.txt".to_string()]));
}

#[test]
fn no_flag_is_the_fresh_browser() {
    let s = Scratch::new(&["a.txt", ".env"]);
    let f = started(&s, &[]);
    assert_eq!((f.hidden, f.show_ignored, f.pick.query.as_str()), (false, false, ""));
}

#[test]
fn the_state_line_says_both_toggles_and_the_filter() {
    let s = Scratch::new(&["a.txt", "b c.txt"]);
    let mut f = started(&s, &[]);
    tap(&mut f, KeyCode::Char('.'), KeyModifiers::NONE);
    tap(&mut f, KeyCode::Char('i'), KeyModifiers::ALT);
    for c in "b c".chars() {
        tap(&mut f, KeyCode::Char(c), KeyModifiers::NONE);
    }
    assert_eq!(state(&f), "state 1 1 b c");
}

#[test]
fn a_fresh_browser_says_both_off_and_no_filter() {
    let s = Scratch::new(&["a.txt"]);
    assert_eq!(state(&started(&s, &[])), "state 0 0 ");
}

/// A filter the flag would refuse is never said: the line carries none.
#[test]
fn a_filter_that_cannot_ride_the_line_is_dropped_from_it() {
    let s = Scratch::new(&["a.txt"]);
    let long = "x".repeat(files::FILTER_MAX + 1);
    let rows: &[(&str, &str)] = &[
        ("an escape", "a\x1b[2Jb"),
        ("a bidi override", "a\u{202e}b"),
        ("a C1 control", "a\u{85}b"),
        ("over the cap", &long),
    ];
    for (name, q) in rows {
        let mut f = started(&s, &[]);
        f.pick.query = q.to_string();
        assert_eq!(state(&f), "state 0 0 ", "{name}");
    }
}

#[test]
fn the_state_rides_a_second_line_after_the_answer() {
    let s = Scratch::new(&["a.txt"]);
    let f = started(&s, &["--hidden"]);
    let got = files::with_state(Outcome::Choose(b"view /x".to_vec()), &f);
    assert_eq!(got, Outcome::Choose(b"view /x\nstate 1 0 ".to_vec()));
}

#[test]
fn a_cancel_carries_no_state() {
    let s = Scratch::new(&["a.txt"]);
    let f = started(&s, &["--hidden"]);
    assert_eq!(files::with_state(Outcome::Cancel, &f), Outcome::Cancel);
}

/// What `parse_args` gives for a row, or a refusal.
type Parsed = Result<Args, ()>;

/// The new flags, each row named: any order after the folder, each at most once,
/// and `--filter` takes one line of up to FILTER_MAX characters with no control
/// or bidi character. The old forms are the `files_select` table's rows.
#[test]
fn the_state_flags_table() {
    let at_cap = "x".repeat(files::FILTER_MAX);
    let over = "x".repeat(files::FILTER_MAX + 1);
    let ok = |sel: Option<&str>, hidden, ignored, filter: &str| {
        Ok(Args {
            dir: "/d".into(),
            select: sel.map(str::to_string),
            hidden,
            ignored,
            filter: filter.into(),
        })
    };
    let rows: &[(&str, Vec<String>, Parsed)] = &[
        ("hidden", args(&["/d", "--hidden"]), ok(None, true, false, "")),
        ("ignored", args(&["/d", "--ignored"]), ok(None, false, true, "")),
        ("a filter", args(&["/d", "--filter", "a b"]), ok(None, false, false, "a b")),
        ("an empty filter", args(&["/d", "--filter", ""]), ok(None, false, false, "")),
        (
            "a filter at the cap",
            args(&["/d", "--filter", &at_cap]),
            ok(None, false, false, &at_cap),
        ),
        (
            "a filter that looks like a flag",
            args(&["/d", "--filter", "--hidden"]),
            ok(None, false, false, "--hidden"),
        ),
        (
            "all of them, select first",
            args(&["/d", "--select", "x.md", "--hidden", "--ignored", "--filter", "x"]),
            ok(Some("x.md"), true, true, "x"),
        ),
        (
            "all of them, select last",
            args(&["/d", "--filter", "x", "--ignored", "--hidden", "--select", "x.md"]),
            ok(Some("x.md"), true, true, "x"),
        ),
        ("--filter without a value", args(&["/d", "--filter"]), Err(())),
        ("a filter over the cap", args(&["/d", "--filter", &over]), Err(())),
        ("an escape in the filter", args(&["/d", "--filter", "a\x1bb"]), Err(())),
        ("a newline in the filter", args(&["/d", "--filter", "a\nb"]), Err(())),
        ("a bidi override in the filter", args(&["/d", "--filter", "a\u{202e}b"]), Err(())),
        ("hidden twice", args(&["/d", "--hidden", "--hidden"]), Err(())),
        ("filter twice", args(&["/d", "--filter", "a", "--filter", "b"]), Err(())),
        ("a flag before the folder", args(&["--hidden", "/d"]), Err(())),
        ("a flag and no folder", args(&["--hidden"]), Err(())),
    ];
    for (name, a, want) in rows {
        let got = files::parse_args(a).map_err(|_| ());
        assert_eq!(&got, want, "{name}");
    }
}

#[test]
fn a_refused_filter_names_the_argument() {
    let e = files::parse_args(&args(&["/d", "--filter", "a\x1bb"])).unwrap_err();
    assert!(e.contains("--filter") && e.contains("control"), "{e}");
}
