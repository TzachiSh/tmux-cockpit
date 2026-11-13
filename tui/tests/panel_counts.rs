//! #964: the panel counts the screen's hidden secrets with the call the
//! envelope makes (`redact::redact_clean`, #954), so Home and Busy name a key
//! split by a direction control as a screen secret, never one "more in the
//! conversation". Busy takes its split from the envelope it sent. Every value
//! here is made up.
mod fake;

use cockpit_tui::panel::app::{App, SentHidden, Tile, View};
use cockpit_tui::panel::ask;
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType, build_parts};
use cockpit_tui::panel::view::render;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// A made-up key cut in two by U+202E: too short to match on either side.
const SPLIT: &str = "$ ls\nsk-ant-MADEUP\u{202E}FAKEFAKE00";
/// A made-up key behind U+2066, the only boundary before it.
const GLUED: &str = "$ ls\nw\u{2066}sk-ant-MADEUPFAKEFAKE00";
/// A made-up key with nothing around it, as the panel always counted it.
const PLAIN: &str = "$ ls\nsk-abcdefghijklmnopqrst1";

fn cap(screen: &str, name: &str, path: &str) -> Capture {
    Capture {
        name: name.into(),
        cmd: "zsh".into(),
        path: path.into(),
        screen: screen.into(),
        ..Default::default()
    }
}

/// Every `App` here is built under the binary's one environment (#948, #953),
/// which [`fake::fixed_env`] sets.
fn app(c: Capture) -> App {
    fake::fixed_env();
    App::new("%3", c)
}

/// Once an `App` is built, the binary runs under the fixed environment. It
/// builds one itself, so it holds when run first or alone.
#[test]
fn an_app_is_built_under_the_fixed_environment() {
    let _ = app(cap("$ ", "w", "/h"));
    fake::assert_fixed_env();
}

/// The test above in a process started with all three variables set.
#[test]
fn the_fixed_environment_replaces_values_already_set() {
    fake::fixed_env_in_a_child("an_app_is_built_under_the_fixed_environment");
}

fn frame(a: &App) -> String {
    let mut t = Terminal::new(TestBackend::new(120, 20)).unwrap();
    t.draw(|f| {
        render(f, f.area(), a, &Theme::fixed(), 0);
    })
    .unwrap();
    t.backend().to_string()
}

fn key(a: &mut App, code: KeyCode) {
    a.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn tile(a: &mut App, t: Tile) {
    let i = a.tiles().iter().position(|x| x.0 == t).unwrap();
    key(a, KeyCode::Char(char::from(b'1' + i as u8)));
}

/// The three flows that send a screen, each started from Home by its keys.
fn start(a: &mut App, row: &str) {
    match row {
        "write" | "fix" => {
            tile(a, Tile::Compose);
            key(a, KeyCode::Char('x'));
            ctrl(a, if row == "write" { 's' } else { 'f' });
        }
        _ => {
            tile(a, Tile::Explain);
            key(a, KeyCode::Char('1'));
        }
    }
    assert_eq!(a.view, View::Busy, "{row}");
}

const FLOWS: [(&str, &str); 3] = [("write", "write (auto)"), ("fix", "fix spelling"), ("explain", "explain")];

fn flow(row: &str) -> Flow {
    match row {
        "write" => Flow::Write {
            draft: "x".into(),
            out: OutType::Auto,
        },
        "fix" => Flow::Fix { draft: "x".into() },
        _ => Flow::Explain,
    }
}

// --------------------------------------------------------------------- Home

#[test]
fn home_counts_a_split_key_on_the_screen() {
    assert_eq!(app(cap(SPLIT, "w", "/h")).screen_hidden, 1);
}

#[test]
fn home_counts_a_glued_key_on_the_screen() {
    assert_eq!(app(cap(GLUED, "w", "/h")).screen_hidden, 1);
}

/// Pinned: a plain key and a key in the name count as before (#963). Home
/// never counts the path: only a write sends it.
#[test]
fn home_counts_an_ordinary_screen_and_name_as_before() {
    let rows = [
        ("plain screen", cap(PLAIN, "w", "/h"), 1),
        ("no secret", cap("$ ls", "w", "/h"), 0),
        ("empty", cap("", "", ""), 0),
        ("name", cap(PLAIN, "deploy-sk-ant-MADEUPFAKEFAKE00", "/h"), 2),
        ("path", cap(PLAIN, "w", "/u/ghp_MADEUPFAKEFAKEFAKEFAKE00"), 1),
    ];
    for (row, c, want) in rows {
        assert_eq!(app(c).screen_hidden, want, "{row}");
    }
}

// --------------------------------------------------------------------- Busy

/// The symptom: a split key on the screen showed as "0 hidden, 1 more in the
/// conversation".
#[test]
fn busy_shows_a_split_key_on_the_screen_under_the_screen() {
    for (row, shown) in FLOWS {
        let mut a = app(cap(SPLIT, "w", "/h"));
        start(&mut a, row);
        let f = frame(&a);
        assert!(f.contains(&format!("sent: screen 2 lines, 1 hidden · {shown}")), "{row}: {f}");
    }
}

#[test]
fn busy_counts_a_split_key_on_the_screen_under_the_screen() {
    let want = SentHidden {
        screen: 1,
        repo: 0,
        talk: 0,
    };
    for (row, _) in FLOWS {
        let mut a = app(cap(SPLIT, "w", "/h"));
        start(&mut a, row);
        assert_eq!(a.sent_hidden(), want, "{row}");
    }
}

#[test]
fn busy_counts_a_glued_key_on_the_screen_under_the_screen() {
    let want = SentHidden {
        screen: 1,
        repo: 0,
        talk: 0,
    };
    for (row, _) in FLOWS {
        let mut a = app(cap(GLUED, "w", "/h"));
        start(&mut a, row);
        assert_eq!(a.sent_hidden(), want, "{row}");
    }
}

/// The repo block is redacted with the same call: a split key there is the
/// block's, not the conversation's.
#[test]
fn busy_shows_a_split_key_in_the_repo_block_under_the_repo_block() {
    let mut c = cap("$ ls", "w", "/h");
    c.repo = "log:\n  abc1234 sk-ant-MADEUP\u{202E}FAKEFAKE00".into();
    let mut a = app(c);
    start(&mut a, "write");
    let f = frame(&a);
    assert!(f.contains("sent: screen 1 lines, 0 hidden, 1 in the repo block · write"), "{f}");
}

/// Every source at once: the screen's plain, split and glued keys, a split
/// key in the name, a key in the path (a write's alone) and one in the repo
/// block (a write's alone). Each flow's count is the envelope's own split.
#[test]
fn busy_counts_match_the_envelopes_split_per_flow() {
    let screen = format!("{PLAIN}\n{SPLIT}\n{GLUED}");
    let mut c = cap(&screen, "w-sk-ant-MADEUP\u{202E}FAKEFAKE00", "/u/ghp_MADEUPFAKEFAKEFAKEFAKE00");
    c.repo = "log:\n  abc1234 sk-ant-MADEUP\u{202E}FAKEFAKE00".into();
    for (row, want) in [("write", (5, 1)), ("fix", (4, 0)), ("explain", (4, 0))] {
        let mut a = app(c.clone());
        start(&mut a, row);
        let r = a.run.as_ref().unwrap();
        let e = build_parts(&flow(row), &c, &r.turns, 0);
        let got = a.sent_hidden();
        assert_eq!((got.screen, got.repo), want, "{row}");
        assert_eq!(
            (got.screen, got.repo, got.talk),
            (e.screen_hidden, e.repo_hidden, e.turns_hidden),
            "{row}"
        );
    }
}

/// A key the user typed into an explain's question is the conversation's.
#[test]
fn busy_counts_a_key_in_the_question_under_the_conversation() {
    let mut a = app(cap(PLAIN, "w", "/h"));
    tile(&mut a, Tile::Explain);
    key(&mut a, KeyCode::Char('5'));
    for c in "why sk-abcdefghijklmnopqrst2".chars() {
        key(&mut a, KeyCode::Char(c));
    }
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, View::Busy);
    assert_eq!(
        a.sent_hidden(),
        SentHidden {
            screen: 1,
            repo: 0,
            talk: 1
        }
    );
}

/// Pinned: a plain key on the screen, in the name and in the path count as
/// #963 left them.
#[test]
fn busy_counts_an_ordinary_screen_name_and_path_as_before() {
    let c = cap(PLAIN, "deploy-sk-ant-MADEUPFAKEFAKE00", "/u/ghp_MADEUPFAKEFAKEFAKEFAKE00");
    for (row, want) in [("write", 3), ("fix", 2), ("explain", 2)] {
        let mut a = app(c.clone());
        start(&mut a, row);
        assert_eq!(
            a.sent_hidden(),
            SentHidden {
                screen: want,
                repo: 0,
                talk: 0
            },
            "{row}"
        );
    }
}

/// Busy's "sent:" line, out of its frame's box.
fn sent_line(f: &str) -> String {
    let row = f.lines().find(|l| l.contains("sent: ")).unwrap_or("");
    row.trim_matches(|c: char| c == '"' || c == '│' || c == ' ').to_string()
}

/// Pinned byte for byte as master printed them before #965: the flows that
/// send a screen keep their line.
#[test]
fn busy_lines_of_the_flows_that_send_a_screen_are_as_before() {
    let mut c = cap(
        &format!("{PLAIN}\n{SPLIT}"),
        "deploy-sk-ant-MADEUPFAKEFAKE00",
        "/u/ghp_MADEUPFAKEFAKEFAKEFAKE00",
    );
    c.repo = "log:\n  abc1234 sk-ant-MADEUP\u{202E}FAKEFAKE00".into();
    let rows = [
        ("write", "sent: screen 4 lines, 4 hidden, 1 in the repo block · write (auto)"),
        ("fix", "sent: screen 4 lines, 3 hidden · fix spelling"),
        ("explain", "sent: screen 4 lines, 3 hidden · explain"),
    ];
    for (row, want) in rows {
        let mut a = app(c.clone());
        start(&mut a, row);
        assert_eq!(sent_line(&frame(&a)), want, "{row}");
    }
}

// ---------------------------------------------------------------- an Ask

/// A made-up key cut in two by U+202E, as a line of an Ask's code.
const CODE_SPLIT: &str = "b sk-ant-MADEUP\u{202E}FAKEFAKE00";

/// An Ask about lines 1-2 of a file whose second line is `line`, its question
/// `q` sent from a pane showing `c`. The guard keeps the file until the test ends.
fn ask(c: Capture, tag: &str, line: &str, q: &str) -> (fake::Scratch, App) {
    ask_lines(c, tag, &format!("a\n{line}\nc\n"), (1, 2), q)
}

/// An Ask about lines `from`-`to` of a file holding `body`, as [`ask`].
fn ask_lines(c: Capture, tag: &str, body: &str, (from, to): (u32, u32), q: &str) -> (fake::Scratch, App) {
    let name = format!("cockpit-panel-counts-{tag}-{}", std::process::id());
    let d = fake::Scratch::fresh(std::env::temp_dir().join(name));
    std::fs::create_dir_all(d.join(".git")).unwrap();
    let f = d.join("x.rs");
    std::fs::write(&f, body).unwrap();
    let mut a = app(c);
    a.ask_open(ask::load(&f, from, to).unwrap());
    q.chars().for_each(|ch| key(&mut a, KeyCode::Char(ch)));
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, View::Busy);
    (d, a)
}

fn split(screen: usize, repo: usize, talk: usize) -> SentHidden {
    SentHidden { screen, repo, talk }
}

/// The envelope an Ask sends holds no screen and no window: a key in either is
/// none of its secrets (#965), whether plain, split, or in the window's name.
#[test]
fn an_asks_envelope_counts_no_secret_of_the_screen_or_window() {
    let rows = [
        ("plain screen", cap(PLAIN, "w", "/h")),
        ("split screen", cap(SPLIT, "w", "/h")),
        ("name", cap("$ ls", "deploy-sk-ant-MADEUPFAKEFAKE00", "/h")),
        ("path", cap("$ ls", "w", "/u/ghp_MADEUPFAKEFAKEFAKEFAKE00")),
    ];
    for (row, c) in rows {
        let (_d, a) = ask(c.clone(), "env", "b", "y");
        let e = build_parts(&a.run.as_ref().unwrap().flow, &c, &[], 0);
        assert_eq!(e.screen_hidden, 0, "{row}");
    }
}

/// An Ask's code is counted as its own, once, a split key as well as a plain one.
#[test]
fn an_asks_envelope_counts_its_codes_keys_as_the_codes() {
    for (row, line) in [("plain", "b sk-abcdefghijklmnopqrst3"), ("split", CODE_SPLIT)] {
        let (_d, a) = ask(cap("$ ls", "w", "/h"), "code", line, "y");
        let e = build_parts(&a.run.as_ref().unwrap().flow, &a.cap, &[], 0);
        assert_eq!((e.screen_hidden, e.repo_hidden, e.turns_hidden), (0, 1, 0), "{row}");
    }
}

/// Busy reads an Ask's split from the envelope it sent, as for every flow.
#[test]
fn busy_counts_an_ask_as_its_envelope_split_it() {
    let c = cap(&format!("{PLAIN}\n{SPLIT}"), "deploy-sk-ant-MADEUPFAKEFAKE00", "/h");
    let (_d, a) = ask(c.clone(), "match", CODE_SPLIT, "why sk-abcdefghijklmnopqrst2");
    let r = a.run.as_ref().unwrap();
    let e = build_parts(&r.flow, &c, &r.turns, 0);
    assert_eq!(a.sent_hidden(), split(e.screen_hidden, e.repo_hidden, e.turns_hidden));
    assert_eq!(a.sent_hidden(), split(0, 1, 1));
}

/// The code's key is named as the code's, never as "more in the conversation",
/// and the line names no screen.
#[test]
fn busy_names_an_asks_code_and_no_screen() {
    let (_d, a) = ask(cap(PLAIN, "w", "/h"), "line", "b sk-abcdefghijklmnopqrst3", "y");
    assert_eq!(sent_line(&frame(&a)), "sent: code 2 lines, 1 hidden · ask about x.rs:1-2");
}

/// A key split by U+202E on the screen changes nothing in an Ask's line.
#[test]
fn busy_line_of_an_ask_ignores_a_split_key_on_the_screen() {
    let (_d, a) = ask(cap(SPLIT, "w", "/h"), "scrsplit", "b", "y");
    assert_eq!(sent_line(&frame(&a)), "sent: code 2 lines, 0 hidden · ask about x.rs:1-2");
    assert_eq!(a.sent_hidden(), split(0, 0, 0));
}

/// A key in the window's name changes nothing in an Ask's line.
#[test]
fn busy_line_of_an_ask_ignores_a_key_in_the_window_name() {
    let (_d, a) = ask(cap("$ ls", "deploy-sk-ant-MADEUPFAKEFAKE00", "/h"), "name", "b", "y");
    assert_eq!(sent_line(&frame(&a)), "sent: code 2 lines, 0 hidden · ask about x.rs:1-2");
    assert_eq!(a.sent_hidden(), split(0, 0, 0));
}

/// A key split by U+202E in the Ask's code is the code's.
#[test]
fn busy_counts_a_split_key_in_an_asks_code_as_the_codes() {
    let (_d, a) = ask(cap("$ ls", "w", "/h"), "codesplit", CODE_SPLIT, "y");
    assert_eq!(sent_line(&frame(&a)), "sent: code 2 lines, 1 hidden · ask about x.rs:1-2");
    assert_eq!(a.sent_hidden(), split(0, 1, 0));
}

/// Pinned: a key in the Ask's question is the conversation's, as before.
#[test]
fn busy_counts_a_key_in_an_asks_question_under_the_conversation() {
    let (_d, a) = ask(cap("$ ls", "w", "/h"), "talk", "b", "why sk-abcdefghijklmnopqrst2");
    let want = "sent: code 2 lines, 0 hidden, 1 more in the conversation · ask about x.rs:1-2";
    assert_eq!(sent_line(&frame(&a)), want);
}

/// An Ask from a pane with nothing captured says the same.
#[test]
fn busy_line_of_an_ask_from_an_empty_capture() {
    let (_d, a) = ask(Capture::default(), "empty", "b", "y");
    assert_eq!(sent_line(&frame(&a)), "sent: code 2 lines, 0 hidden · ask about x.rs:1-2");
}

/// The line counts the code inlined, not the lines asked: the caps cut an Ask
/// past [`ask::MAX_LINES`], and a line over [`ask::MAX_BYTES`] after the first.
#[test]
fn busy_counts_an_asks_code_by_the_lines_shown() {
    let many: String = (1..=450).map(|i| format!("l{i}\n")).collect();
    let long = format!("a\n{}\nc\n", "x".repeat(ask::MAX_BYTES + 1));
    let rows = [
        (
            "past the line cap",
            many.as_str(),
            (1, 450),
            "code 400 lines, 0 hidden · ask about x.rs:1-450",
        ),
        (
            "a line past the byte cap",
            long.as_str(),
            (1, 3),
            "code 1 lines, 0 hidden · ask about x.rs:1-3",
        ),
    ];
    let line = |body, range| sent_line(&frame(&ask_lines(cap("$ ls", "w", "/h"), "shown", body, range, "y").1));
    let got: Vec<_> = rows.iter().map(|(row, body, range, _)| (*row, line(*body, *range))).collect();
    let want: Vec<_> = rows.iter().map(|(row, .., w)| (*row, format!("sent: {w}"))).collect();
    assert_eq!(got, want);
}
