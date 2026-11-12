//! #964: the panel counts the screen's hidden secrets with the call the
//! envelope makes (`redact::redact_clean`, #954), so Home and Busy name a key
//! split by a direction control as a screen secret, never one "more in the
//! conversation". Busy takes its split from the envelope it sent. Every value
//! here is made up.
use cockpit_tui::panel::app::{App, SentHidden, Tile, View};
use cockpit_tui::panel::ask;
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType, build_parts};
use cockpit_tui::panel::view::render;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;
use std::sync::Once;

/// A made-up key cut in two by U+202E: too short to match on either side.
const SPLIT: &str = "$ ls\nsk-ant-MADEUP\u{202E}FAKEFAKE00";
/// A made-up key behind U+2066, the only boundary before it.
const GLUED: &str = "$ ls\nw\u{2066}sk-ant-MADEUPFAKEFAKE00";
/// A made-up key with nothing around it, as the panel always counted it.
const PLAIN: &str = "$ ls\nsk-abcdefghijklmnopqrst1";

/// HOME and XDG_CONFIG_HOME at a folder that is never made, before the first
/// `App`: a prompt override under the developer's config changes a write.
fn fixed_env() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        // SAFETY: set once, before any thread here reads the environment.
        unsafe {
            std::env::set_var("HOME", "/sys/absent-964");
            std::env::set_var("XDG_CONFIG_HOME", "/sys/absent-964");
            std::env::remove_var("COCKPIT_PANEL_MODEL");
        }
    });
}

fn cap(screen: &str, name: &str, path: &str) -> Capture {
    Capture {
        name: name.into(),
        cmd: "zsh".into(),
        path: path.into(),
        screen: screen.into(),
        ..Default::default()
    }
}

fn app(c: Capture) -> App {
    fixed_env();
    App::new("%3", c)
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

// ---------------------------------------------------------------- an Ask

/// A scratch dir under the temp dir, removed on drop.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Pinned: an Ask's Busy line is #965's to fix, and this change leaves it as
/// it was: the screen counted as Home counts it, the rest as conversation.
#[test]
fn an_asks_busy_line_is_left_as_it_was() {
    let d = Dir(std::env::temp_dir().join(format!("cockpit-panel-counts-{}", std::process::id())));
    std::fs::create_dir_all(d.0.join(".git")).unwrap();
    let f = d.0.join("x.rs");
    std::fs::write(&f, "a\nb\nc\n").unwrap();
    let mut a = app(cap(PLAIN, "w", "/h"));
    a.ask_open(ask::load(&f, 1, 2).unwrap());
    key(&mut a, KeyCode::Char('y'));
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, View::Busy);
    assert!(frame(&a).contains("sent: screen 2 lines, 1 hidden · ask about"), "{}", frame(&a));
}
