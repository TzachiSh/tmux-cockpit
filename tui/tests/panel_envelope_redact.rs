//! #937: the window's name and the pane's path are redacted wherever the
//! envelope writes them: the `window` attribute of every `<screen>` and the
//! Write flow's `<window>` line. The command is a program's name only
//! (`#{pane_current_command}`), so it is cleaned and never redacted. A ref and
//! an Ask's path are files the user chose, and stay as written. Every value
//! here is made up.
use cockpit_tui::panel::app::{App, Tile, View};
use cockpit_tui::panel::ask::AskCode;
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType, RefItem, build, build_parts, build_refs};
use cockpit_tui::panel::view::render;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::sync::Once;

/// A made-up key glued to a word by a `-`, as `rename-window` could set it.
const NAME: &str = "deploy-sk-ant-MADEUPFAKEFAKE00";
const NAME_SHOWN: &str = "deploy-[hidden]";
/// A made-up token as a folder's name.
const PATH: &str = "/home/u/ghp_MADEUPFAKEFAKEFAKEFAKE00/src";
const PATH_SHOWN: &str = "/home/u/[hidden]/src";

fn cap(name: &str, cmd: &str, path: &str) -> Capture {
    Capture {
        name: name.into(),
        cmd: cmd.into(),
        path: path.into(),
        screen: "$ ls".into(),
        ..Default::default()
    }
}

fn write() -> Flow {
    Flow::Write {
        draft: "x".into(),
        out: OutType::Auto,
    }
}

/// The three flows that send a screen, named.
fn screen_flows() -> [(&'static str, Flow); 3] {
    [
        ("write", write()),
        ("fix", Flow::Fix { draft: "x".into() }),
        ("explain", Flow::Explain),
    ]
}

/// The `<screen ...>` opening line of the envelope.
fn screen_tag(flow: &Flow, c: &Capture) -> Option<String> {
    build(flow, c, &[], 3).lines().find(|l| l.starts_with("<screen ")).map(String::from)
}

/// The Write flow's `<window>` line.
fn window_line(c: &Capture) -> Option<String> {
    build(&write(), c, &[], 3)
        .lines()
        .find(|l| l.starts_with("<window>"))
        .map(String::from)
}

// ------------------------------------------------------- the window's name

#[test]
fn a_key_in_the_window_name_is_hidden_in_each_screen_tag() {
    let c = cap(NAME, "zsh", "/home/u");
    let want = format!("<screen window=\"{NAME_SHOWN} · zsh\">");
    for (flow_name, flow) in screen_flows() {
        assert_eq!(screen_tag(&flow, &c), Some(want.clone()), "{flow_name}");
    }
}

#[test]
fn a_key_in_the_window_name_is_hidden_in_the_window_line() {
    let want = format!("<window>{NAME_SHOWN} · zsh · /home/u</window>");
    assert_eq!(window_line(&cap(NAME, "zsh", "/home/u")), Some(want));
}

// ------------------------------------------------------- the pane's path

#[test]
fn a_key_in_the_path_is_hidden_in_the_window_line() {
    let want = format!("<window>w · zsh · {PATH_SHOWN}</window>");
    assert_eq!(window_line(&cap("w", "zsh", PATH)), Some(want));
}

#[test]
fn the_path_is_written_nowhere_else_in_a_write() {
    let got = build(&write(), &cap("w", "zsh", PATH), &[], 3);
    assert!(!got.contains("MADEUPFAKE"), "{got}");
}

// ------------------------------------------- ordinary values and the command

#[test]
fn an_ordinary_name_and_path_are_written_as_they_are() {
    let c = cap("build", "zsh", "/home/u/src/app-2");
    assert_eq!(screen_tag(&write(), &c).as_deref(), Some("<screen window=\"build · zsh\">"));
    let want = "<window>build · zsh · /home/u/src/app-2</window>";
    assert_eq!(window_line(&c).as_deref(), Some(want));
}

#[test]
fn the_command_is_not_redacted() {
    let cmd = "sk-ant-MADEUPFAKEFAKE00";
    let c = cap("w", cmd, "/home/u");
    assert_eq!(screen_tag(&write(), &c), Some(format!("<screen window=\"w · {cmd}\">")));
    assert_eq!(window_line(&c), Some(format!("<window>w · {cmd} · /home/u</window>")));
}

#[test]
fn an_empty_name_and_path_hide_nothing() {
    let c = cap("", "zsh", "");
    assert_eq!(window_line(&c).as_deref(), Some("<window> · zsh · </window>"));
    assert_eq!(build_parts(&write(), &c, &[], 3).hidden(), 0);
}

// ---------------------------------- keys glued or split by direction controls

/// A key the control glues to the word before it, and one it splits inside:
/// redacting only the raw text misses the split one, only the cleaned text the
/// glued one (`redact::redact_clean`).
const CONTROLLED: [(&str, &str, &str); 4] = [
    ("glued name", "w\u{2066}sk-ant-MADEUPFAKEFAKE00", "w[hidden]"),
    ("split name", "w-sk-ant-MADEUP\u{202E}FAKEFAKE00", "w-[hidden]"),
    ("glued path", "/u/a\u{200F}ghp_MADEUPFAKEFAKEFAKEFAKE00", "/u/a[hidden]"),
    ("split path", "/u/ghp_MADEUPFAKE\u{202E}FAKEFAKEFAKE00", "/u/[hidden]"),
];

#[test]
fn a_key_glued_or_split_by_a_direction_control_is_hidden() {
    for (row, value, shown) in CONTROLLED {
        let c = if row.ends_with("name") {
            cap(value, "zsh", "/h")
        } else {
            cap("w", "zsh", value)
        };
        let line = window_line(&c).unwrap_or_default();
        assert!(line.contains(shown) && !line.contains("FAKE"), "{row}: {line}");
    }
}

#[test]
fn a_key_glued_or_split_in_the_name_is_hidden_in_the_screen_tag() {
    for (row, value, shown) in CONTROLLED.iter().filter(|r| r.0.ends_with("name")) {
        let tag = screen_tag(&Flow::Explain, &cap(value, "zsh", "/h")).unwrap_or_default();
        assert_eq!(tag, format!("<screen window=\"{shown} · zsh\">"), "{row}");
    }
}

// ------------------------------------------------------------------ counts

/// The window belongs to the screen: both go in `screen_hidden`. The name is
/// one secret though a write writes it twice; the path counts only in a write,
/// the one flow that sends it.
#[test]
fn the_name_and_path_count_with_the_screen() {
    let c = cap(NAME, "zsh", PATH);
    let rows = [
        ("write", write(), 2),
        ("fix", Flow::Fix { draft: "x".into() }, 1),
        ("explain", Flow::Explain, 1),
    ];
    for (row, flow, want) in rows {
        let e = build_parts(&flow, &c, &[], 3);
        assert_eq!((e.screen_hidden, e.repo_hidden, e.turns_hidden), (want, 0, 0), "{row}");
    }
}

#[test]
fn a_split_key_in_the_name_counts_once() {
    let c = cap("w-sk-ant-MADEUP\u{202E}FAKEFAKE00", "zsh", "/h");
    assert_eq!(build_parts(&Flow::Explain, &c, &[], 3).screen_hidden, 1);
}

// -------------------------------------------- what stays as written (owner)

#[test]
fn an_asks_path_is_not_redacted() {
    let path = "src/ghp_MADEUPFAKEFAKEFAKEFAKE00.rs";
    let ask = Flow::Ask(AskCode {
        at: format!("{path}:1-1"),
        path: path.into(),
        repo: "/r".into(),
        code: "1 x".into(),
        hidden: 0,
        asked: (1, 1),
        shown: (1, 1),
        cut: false,
    });
    let got = build(&ask, &Capture::default(), &[], 3);
    assert!(got.contains(&format!("<code path=\"{path}\" lines=\"1-1\">")), "{got}");
}

#[test]
fn a_ref_is_not_redacted() {
    let text = "@/home/u/ghp_MADEUPFAKEFAKEFAKEFAKE00/a.rs";
    let r = RefItem {
        text: text.into(),
        mention: true,
        whole: true,
        outside: false,
        denied: false,
    };
    let got = build_refs(&write(), &cap("w", "zsh", "/home/u"), &[], 3, &[r]).text;
    assert!(
        got.contains(&format!("<ref form=\"mention\" note=\"whole file\">{text}</ref>")),
        "{got}"
    );
}

// --------------------------------------------------- what the panel shows

/// HOME and XDG_CONFIG_HOME at a folder that is never made, before the first
/// `App`: a prompt override under the developer's config changes a write.
fn fixed_env() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        // SAFETY: set once, before any thread here reads the environment.
        unsafe {
            std::env::set_var("HOME", "/sys/absent-937");
            std::env::set_var("XDG_CONFIG_HOME", "/sys/absent-937");
            std::env::remove_var("COCKPIT_PANEL_MODEL");
        }
    });
}

fn app(name: &str, path: &str) -> App {
    fixed_env();
    App::new("%3", cap(name, "zsh", path))
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

/// Compose, a draft, then ^S: busy with a Write.
fn writing(a: &mut App) {
    let i = a.tiles().iter().position(|t| t.0 == Tile::Compose).unwrap();
    key(a, KeyCode::Char(char::from(b'1' + i as u8)));
    key(a, KeyCode::Char('x'));
    a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert_eq!(a.view, View::Busy);
}

#[test]
fn home_counts_a_key_in_the_window_name_with_the_screen() {
    let a = app(NAME, "/home/u");
    assert_eq!(a.screen_hidden, 1);
}

#[test]
fn busy_counts_a_written_path_with_the_screen_not_the_conversation() {
    let mut a = app("w", PATH);
    writing(&mut a);
    let f = frame(&a);
    assert!(f.contains("sent: screen 1 lines, 1 hidden · write (auto)"), "{f}");
}
