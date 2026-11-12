//! #936: every section of the panel's envelope that holds text from outside the
//! code goes through `action::sanitize`, as a ref already did. A direction
//! control (U+202E, U+2066, ...) or a line or paragraph separator is dropped;
//! newline, tab and right-to-left letters stay. Every value here is made up.
use cockpit_tui::panel::ask::{self, AskCode};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType, Role, Turn, build};
use std::path::PathBuf;

/// A capture with `screen` and `repo`, and the window's name, command and path.
fn cap(name: &str, cmd: &str, path: &str, screen: &str, repo: &str) -> Capture {
    Capture {
        name: name.into(),
        cmd: cmd.into(),
        path: path.into(),
        screen: screen.into(),
        repo: repo.into(),
    }
}

fn screen(s: &str) -> Capture {
    cap("w", "bash", "/home/u", s, "")
}

fn write(draft: &str) -> Flow {
    Flow::Write {
        draft: draft.into(),
        out: OutType::Auto,
    }
}

fn fix(draft: &str) -> Flow {
    Flow::Fix { draft: draft.into() }
}

fn turn(text: &str) -> [Turn; 1] {
    [Turn {
        role: Role::User,
        text: text.into(),
    }]
}

/// An Ask whose code is `code`, as a state file or `ask::load` hands it over.
fn ask_with(code: &str) -> Flow {
    Flow::Ask(AskCode {
        at: "src/x.rs:1-1".into(),
        path: "src/x.rs".into(),
        repo: "/r".into(),
        code: code.into(),
        hidden: 0,
        asked: (1, 1),
        shown: (1, 1),
        cut: false,
    })
}

/// The text from the end of the first tag opening with `open` to `close`, one
/// newline trimmed at each end, as the block forms write it.
fn body(env: &str, open: &str, close: &str) -> String {
    let at = env.find(open).unwrap_or_else(|| panic!("no {open} in {env}"));
    let from = at + env[at..].find('>').unwrap() + 1;
    let to = from + env[from..].find(close).unwrap_or_else(|| panic!("no {close} in {env}"));
    let b = &env[from..to];
    let b = b.strip_prefix('\n').unwrap_or(b);
    b.strip_suffix('\n').unwrap_or(b).to_string()
}

/// The text after the first `start` up to the next `end`.
fn between(env: &str, start: &str, end: &str) -> String {
    let from = env.find(start).unwrap_or_else(|| panic!("no {start} in {env}")) + start.len();
    env[from..from + env[from..].find(end).unwrap()].to_string()
}

// ------------------------------------------------------- one test per section

/// The screen in every flow that carries it, U+2029 included.
#[test]
fn the_screen_drops_a_direction_control() {
    let rows = [("write", write("x")), ("fix", fix("x")), ("explain", Flow::Explain)];
    for (name, flow) in rows {
        let s = "fix: update docs\u{202E}hidden-tail\u{2029}end";
        let env = build(&flow, &screen(s), &[], 3);
        assert_eq!(body(&env, "<screen", "</screen>"), "fix: update docshidden-tailend", "{name}");
    }
}

#[test]
fn the_window_attribute_drops_a_direction_control_from_name_and_command() {
    let rows = [
        ("name", cap("na\u{2066}me", "bash", "/p", "", "")),
        ("command", cap("name", "ba\u{2066}sh", "/p", "", "")),
    ];
    for (field, c) in rows {
        let env = build(&write("x"), &c, &[], 3);
        assert_eq!(between(&env, "<screen window=\"", "\""), "name · bash", "{field}");
    }
}

#[test]
fn the_window_line_drops_a_direction_control_from_name_command_and_path() {
    let rows = [
        ("name", cap("na\u{202E}me", "bash", "/home/u", "", "")),
        ("command", cap("name", "ba\u{200F}sh", "/home/u", "", "")),
        ("path", cap("name", "bash", "/home/\u{2066}u\u{2069}", "", "")),
    ];
    for (field, c) in rows {
        let env = build(&write("x"), &c, &[], 3);
        assert_eq!(between(&env, "<window>", "</window>"), "name · bash · /home/u", "{field}");
    }
}

#[test]
fn the_repo_block_drops_a_direction_control() {
    let c = cap("w", "bash", "/p", "", "commit 1 fix: docs\u{200F}tail");
    let env = build(&write("x"), &c, &[], 3);
    assert_eq!(body(&env, "<repo>", "</repo>"), "commit 1 fix: docstail");
}

#[test]
fn the_write_request_drops_a_direction_control() {
    let env = build(&write("make\u{2028}it"), &screen(""), &[], 3);
    assert_eq!(body(&env, "<request>", "</request>"), "makeit");
}

#[test]
fn the_fix_text_drops_a_direction_control() {
    let env = build(&fix("tpyo\u{202E}here"), &screen(""), &[], 3);
    assert_eq!(body(&env, "<text>", "</text>"), "tpyohere");
}

#[test]
fn a_turn_drops_a_direction_control() {
    let env = build(&Flow::Explain, &screen(""), &turn("why\u{202C} this"), 3);
    assert_eq!(body(&env, "<turn role=\"user\"", "</turn>"), "why this");
}

#[test]
fn an_asks_code_drops_a_direction_control() {
    let env = build(&ask_with("1 let a\u{2069} = 1;"), &Capture::default(), &[], 3);
    assert_eq!(body(&env, "<code", "</code>"), "1 let a = 1;");
}

/// A scratch repo under the temp dir holding `src/x.rs`, removed on drop.
struct Repo(PathBuf);

impl Repo {
    fn new(tag: &str, body: &str) -> (Repo, PathBuf) {
        let name = format!("cockpit-panel-envelope-sanitize-{tag}-{}", std::process::id());
        let d = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("src")).unwrap();
        let f = d.join("src/x.rs");
        std::fs::write(&f, body).unwrap();
        (Repo(d), f)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The code is cleaned as it is read, before the envelope: the ^Z state keeps it.
#[test]
fn an_asks_code_read_from_a_file_drops_a_direction_control() {
    let (_r, f) = Repo::new("read", "let a\u{202E} = 1;\n");
    let a = ask::load_in(&f, 1, 1, None).unwrap();
    assert_eq!(a.code, "1 let a = 1;");
}

// ------------------------------------- sanitize before redact (a split secret)

/// A made-up key with a direction control inside: redact alone stops at it and
/// sees a run too short to be a key, so the halves would go out joined.
const SPLIT: &str = "sk-ant-MADEUP\u{202E}FAKEFAKE00";

#[test]
fn a_secret_split_by_a_direction_control_is_hidden_in_every_redacted_section() {
    let rows = [
        ("screen", build(&write("x"), &screen(SPLIT), &[], 3)),
        ("repo", build(&write("x"), &cap("w", "b", "/p", "", SPLIT), &[], 3)),
        ("turn", build(&Flow::Explain, &screen(""), &turn(SPLIT), 3)),
    ];
    for (section, env) in rows {
        assert!(env.contains("[hidden]"), "{section}: {env}");
        assert!(!env.contains("FAKEFAKE00"), "{section}: {env}");
    }
}

#[test]
fn a_secret_split_by_a_direction_control_is_hidden_in_an_asks_code() {
    let (_r, f) = Repo::new("split", &format!("let k = \"{SPLIT}\";\n"));
    let a = ask::load_in(&f, 1, 1, None).unwrap();
    assert_eq!((a.code.as_str(), a.hidden), ("1 let k = \"[hidden]\";", 1));
}

// ------------------------------------------------------------ what stays

/// Each section's text as the envelope carries it, for the input `s`.
fn every_section(s: &str) -> Vec<(&'static str, String)> {
    let w = |c: Capture| build(&write("x"), &c, &[], 3);
    let turns = turn(s);
    vec![
        ("screen", body(&w(screen(s)), "<screen", "</screen>")),
        ("name", between(&w(cap(s, "b", "/p", "", "")), "<window>", " · b · /p")),
        ("command", between(&w(cap("n", s, "/p", "", "")), "<window>n · ", " · /p")),
        ("path", between(&w(cap("n", "b", s, "", "")), "<window>n · b · ", "</window>")),
        ("repo", body(&w(cap("n", "b", "/p", "", s)), "<repo>", "</repo>")),
        ("request", body(&build(&write(s), &screen(""), &[], 3), "<request>", "</request>")),
        ("text", body(&build(&fix(s), &screen(""), &[], 3), "<text>", "</text>")),
        (
            "turn",
            body(&build(&Flow::Explain, &screen(""), &turns, 3), "<turn role=\"user\"", "</turn>"),
        ),
        ("code", body(&build(&ask_with(s), &Capture::default(), &[], 3), "<code", "</code>")),
    ]
}

/// Newline and tab keep their meaning: `sanitize` keeps both, as they are.
#[test]
fn newline_and_tab_stay_in_every_section() {
    for (section, got) in every_section("a\tb\nc") {
        assert_eq!(got, "a\tb\nc", "{section}");
    }
}

/// A Windows line end is one newline; a lone carriage return goes.
#[test]
fn a_carriage_return_folds_into_the_newline_or_goes() {
    for (section, got) in every_section("a\r\nb\rc") {
        assert_eq!(got, "a\nbc", "{section}");
    }
}

/// Right-to-left letters are text, not controls: made-up Hebrew and Arabic stay.
#[test]
fn right_to_left_letters_stay_in_every_section() {
    for word in ["שלום עולם", "مرحبا بالعالم"] {
        for (section, got) in every_section(word) {
            assert_eq!(got, word, "{section}");
        }
    }
}

/// A screen of nothing but direction controls is an empty screen to a Write.
#[test]
fn a_screen_of_only_direction_controls_reads_as_empty() {
    let env = build(&write("x"), &screen("\u{202E}\u{2066}\u{2028}"), &[], 3);
    assert_eq!(body(&env, "<screen", "</screen>"), "(empty)");
}
