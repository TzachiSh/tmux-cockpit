//! The panel's screens and state machine: TestBackend goldens, key sequences,
//! and the action file. COCKPIT_BLESS=1 cargo test rewrites the goldens.
mod fake;

use cockpit_tui::panel::action::{read_state, sanitize, write_action, write_state};
use cockpit_tui::panel::app::{Action, App, InputKind, Level, Resume, Tap, Tile, View, bracketed_ok};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::OutType;
use cockpit_tui::panel::parse::Step;
use cockpit_tui::panel::request::{Call, Effort};
use cockpit_tui::panel::view::{Hit, mouse, render};
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Once;
use std::time::Duration;

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

fn frame(app: &App, w: u16, h: u16, tick: u64) -> String {
    framed(app, w, h, &Theme::fixed(), tick).0
}

fn framed(app: &App, w: u16, h: u16, th: &Theme, tick: u64) -> (String, (usize, usize)) {
    let (f, page, _) = drawn(app, w, h, th, tick);
    (f, page)
}

/// A frame, its fit numbers, and its hit map.
fn drawn(app: &App, w: u16, h: u16, th: &Theme, tick: u64) -> (String, (usize, usize), Vec<Hit>) {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut page = (0, 0, vec![]);
    t.draw(|f| page = render(f, f.area(), app, th, tick)).unwrap();
    (t.backend().to_string(), (page.0, page.1), page.2)
}

/// A frame as the event loop draws it: the offsets are fitted after it.
fn shown(a: &mut App, w: u16, h: u16) -> String {
    let (f, (total, room)) = framed(a, w, h, &Theme::fixed(), 0);
    a.fit(total, room);
    f
}

/// The two widths the popup meets most, and two small screens: a phone in
/// portrait and a split pane.
fn goldens(name: &str, app: &App, tick: u64) {
    golden(&format!("panel-{name}-40"), &frame(app, 40, 12, tick));
    golden(&format!("panel-{name}-60"), &frame(app, 60, 16, tick));
    golden(&format!("panel-{name}-80"), &frame(app, 80, 20, tick));
    golden(&format!("panel-{name}-120"), &frame(app, 120, 24, tick));
}

/// HOME and XDG_CONFIG_HOME for every test here. Nothing is ever made at it:
/// sysfs takes no entry from a user, and a box without /sys has no such folder.
const NO_HOME: &str = "/sys/absent-948";

/// One environment for the whole test binary, set before the first `App` is
/// built and never restored. A prompt override under the developer's own config
/// folder and their HOME changed what a write sends (#948), and an exported
/// COCKPIT_PANEL_MODEL changed the model and the frames that name it (#953).
fn fixed_env() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        // SAFETY: std's getenv and setenv share one lock, and a spawn holds it
        // while the child takes the environment. Every read a test thread
        // makes here is a Rust read; the readers that are not Rust are child
        // processes, which get their copy at the spawn.
        unsafe {
            std::env::set_var("HOME", NO_HOME);
            std::env::set_var("XDG_CONFIG_HOME", NO_HOME);
            std::env::remove_var("COCKPIT_PANEL_MODEL");
        }
    });
}

fn app(name: &str, cmd: &str, screen: &str) -> App {
    fixed_env();
    let cap = Capture {
        name: name.into(),
        cmd: cmd.into(),
        path: "/home/u/proj".into(),
        screen: screen.into(),
        ..Default::default()
    };
    App::new("%3", cap)
}

fn key(a: &mut App, code: KeyCode) {
    a.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn typ(a: &mut App, s: &str) {
    for c in s.chars() {
        if c == '\n' {
            key(a, KeyCode::Enter);
        } else {
            key(a, KeyCode::Char(c));
        }
    }
}

fn note(a: &App) -> Option<&str> {
    a.note.as_ref().map(|n| n.text.as_str())
}

fn level(a: &App) -> Option<Level> {
    a.note.as_ref().map(|n| n.level)
}

fn ch(a: &mut App, c: char) {
    key(a, KeyCode::Char(c));
}

/// Compose, a draft, then ^S: the app is busy with a pending Write call.
fn writing(a: &mut App, draft: &str) {
    let i = a.tiles().iter().position(|t| t.0 == Tile::Compose).unwrap();
    ch(a, char::from(b'1' + i as u8));
    typ(a, draft);
    ctrl(a, 's');
    assert_eq!(a.view, View::Busy);
}

const CLAUDE_SCREEN: &str = "● I can either rewrite the parser or add a fallback.\n  Which do you prefer?\n\n❯ ";

// ---------------------------------------------------------------- goldens

#[test]
fn golden_home() {
    goldens("home", &app("claude", "claude", CLAUDE_SCREEN), 0);
}

#[test]
fn golden_compose() {
    let mut a = app("work", "bash", "$ make test\nFAIL tests/api.rs");
    ch(&mut a, '1');
    key(&mut a, KeyCode::Tab);
    ctrl(&mut a, 'n');
    typ(&mut a, "fix the flaky api test then rerun it");
    assert_eq!(a.out, OutType::Steps);
    goldens("compose", &a, 0);
}

/// ^R on (#416): the head says so.
#[test]
fn golden_compose_explore() {
    let mut a = app("work", "bash", "$ make test\nFAIL tests/api.rs");
    ch(&mut a, '1');
    ctrl(&mut a, 'r');
    typ(&mut a, "fix the flaky api test then rerun it");
    assert!(a.explore);
    goldens("compose-explore", &a, 0);
}

/// An exploring write's Busy names the read and its 90 s limit.
#[test]
fn golden_busy_explore() {
    let mut a = app("work", "bash", "$ ls");
    ch(&mut a, '1');
    ctrl(&mut a, 'r');
    typ(&mut a, "list big files");
    ctrl(&mut a, 's');
    assert_eq!(a.view, View::Busy);
    a.timeout = Duration::from_secs(90);
    goldens("busy-explore", &a, 37);
}

#[test]
fn golden_busy() {
    let mut a = app("work", "bash", "$ ls\nexport T=ghp_abcdefghijABCDEFGHIJ12");
    writing(&mut a, "list big files");
    a.timeout = Duration::from_secs(120);
    goldens("busy", &a, 37);
}

#[test]
fn golden_busy_still_working() {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    let i = a.tiles().iter().position(|t| t.0 == Tile::Explain).unwrap();
    ch(&mut a, char::from(b'1' + i as u8));
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, View::Busy);
    a.timeout = Duration::from_secs(90);
    goldens("busy-slow", &a, 312);
}

#[test]
fn busy_counts_repo_block_secrets_apart_from_the_conversation() {
    let mut a = app("work", "bash", "$ ls");
    a.cap.repo = "branch: main\nlog:\n  abc1234 add key sk-abcdefghijklmnopqrst1".into();
    writing(&mut a, "x");
    let f = frame(&a, 100, 20, 0);
    assert!(f.contains("sent: screen 1 lines, 0 hidden, 1 in the repo block"), "{f}");
    assert!(!f.contains("in the conversation"), "a repo secret is not a conversation one: {f}");
    let mut b = app("work", "bash", "$ ls");
    b.cap.repo = "branch: main".into();
    writing(&mut b, "x");
    let f = frame(&b, 100, 20, 0);
    assert!(f.contains("sent: screen 1 lines, 0 hidden"), "{f}");
    assert!(!f.contains("repo block"), "no repo note when it hid nothing: {f}");
    // explain never sends the block, so it never counts it
    let mut e = app("claude", "claude", CLAUDE_SCREEN);
    e.cap.repo = "log:\n  abc1234 sk-abcdefghijklmnopqrst1".into();
    ch(&mut e, '3');
    ch(&mut e, '2');
    let f = frame(&e, 100, 20, 0);
    assert!(f.contains("sent: screen") && !f.contains("repo block"), "{f}");
}

#[test]
fn busy_orders_screen_conversation_then_repo() {
    let mut a = app("work", "bash", "$ ls\nsk-abcdefghijklmnopqrst1");
    a.cap.repo = "log:\n  abc1234 ghp_abcdefghijABCDEFGHIJ12".into();
    writing(&mut a, "x");
    a.reply(Ok(
        "<question>Which?</question><option>sk-zyxwvutsrqponmlkjih9</option><option>b</option>".into(),
    ));
    ch(&mut a, '1');
    let f = frame(&a, 120, 20, 0);
    assert!(
        f.contains("sent: screen 2 lines, 1 hidden, 2 more in the conversation, 1 in the repo block"),
        "{f}"
    );
}

#[test]
fn busy_echoes_what_was_sent_and_its_limit() {
    let mut a = app("work", "bash", "$ ls\nsk-abcdefghijklmnopqrst1 ghp_abcdefghijABCDEFGHIJ12");
    assert_eq!(a.screen_hidden, 2);
    assert!(frame(&a, 80, 20, 0).contains("will send: screen 2 lines, 2 hidden"));
    writing(&mut a, "x");
    assert_eq!(a.hidden, 2, "the envelope's own count");
    let f = frame(&a, 80, 20, 299);
    assert!(f.contains("sent: screen 2 lines, 2 hidden · write (auto)"), "{f}");
    assert!(f.contains("model sonnet: asks and writes better"), "{f}");
    assert!(!f.contains("times out"), "no call has gone out, so no limit is known");
    assert!(!f.contains("still working"), "29.9 s is not yet slow");
    a.timeout = Duration::from_secs(45);
    let f = frame(&a, 80, 20, 300);
    assert!(f.contains("times out at 45 s") && f.contains("still working…"), "{f}");
}

#[test]
fn golden_question() {
    let mut a = app("work", "bash", "$ ls");
    writing(&mut a, "change the timeout there to 30");
    a.reply(Ok(
        "<question>Which file should I change?</question>\n<option recommended=\"true\">~/bin/net-proxy</option>\n\
                <option>~/bin/clip-helper</option>\n<option>Both files</option>"
            .into(),
    ));
    goldens("question", &a, 0);
}

#[test]
fn golden_result_with_the_multi_line_warning() {
    let mut a = app("py", "python3", ">>> ");
    writing(&mut a, "a dict of users");
    a.reply(Ok("<text>users = {\n    \"alice\": 1,\n    \"bob\": 2,\n}</text>".into()));
    goldens("result", &a, 0);
}

#[test]
fn golden_steps() {
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "clean the build and reinstall");
    a.reply(Ok(
        "<steps>\n<step><note>Remove the old build</note><cmd lang=\"bash\">rm -rf target</cmd></step>\n\
                <step><note>Build again</note><cmd lang=\"bash\">cargo build --release</cmd></step>\n\
                <step>Tap Allow in the app</step>\n</steps>"
            .into(),
    ));
    key(&mut a, KeyCode::Down);
    goldens("steps", &a, 0);
}

fn explained() -> App {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    ch(&mut a, '3');
    ch(&mut a, '2');
    a.reply(Ok("Claude Code asks which way to go. The fallback is the smaller change.\n\
                <prompt>Add the fallback for empty lines and keep the parser as it is.</prompt>"
        .into()));
    a
}

#[test]
fn golden_answer_with_ready() {
    goldens("answer", &explained(), 0);
}

#[test]
fn golden_error() {
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Err(cockpit_tui::panel::request::Fail::Failed(
        "Error: not logged in · run claude /login".into(),
    )));
    goldens("error", &a, 0);
}

// ---------------------------------------------------------------- home and compose

#[test]
fn tabs_are_expanded_before_the_wrap() {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<text>a\tb</text>".into()));
    let f = frame(&a, 40, 8, 0);
    assert!(f.contains("│ a    b"), "{f}");
}

#[test]
fn home_suggests_by_window_and_numbers_pick_tiles() {
    let mut a = app("w", "zsh", "$ ");
    assert_eq!(a.tiles()[0].1, "Write a command");
    ch(&mut a, '1');
    assert_eq!((a.view.clone(), a.out), (View::Compose, OutType::Command));
    assert_eq!(app("claude", "claude", "").tiles()[0].1, "Write a prompt");
    let other = app("w", "htop", "").tiles();
    assert_eq!(
        other.iter().map(|t| t.1).collect::<Vec<_>>(),
        ["Compose", "Explain", "Tablet mode", "Close"],
        "Compose is not listed twice"
    );

    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '3');
    assert_eq!(a.view, View::ExplainAsk { sel: 0 });
    key(&mut a, KeyCode::Esc);
    ch(&mut a, '5');
    assert!(a.quit && a.action.is_none(), "Close exits with no action");
    for k in [KeyCode::Esc, KeyCode::Char('q')] {
        let mut a = app("w", "zsh", "$ ");
        key(&mut a, k);
        assert!(a.quit && a.action.is_none(), "{k:?} on home");
    }
}

#[test]
fn the_compose_box_edits_and_q_is_text_there() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    typ(&mut a, "quit now\nplease");
    assert!(!a.quit, "q types inside the box");
    assert_eq!(a.draft, "quit now\nplease");
    ctrl(&mut a, 'w');
    assert_eq!(a.draft, "quit now\n");
    key(&mut a, KeyCode::Backspace);
    ctrl(&mut a, 'w');
    assert_eq!(a.draft, "quit ");
    ctrl(&mut a, 'u');
    assert_eq!(a.draft, "");
    typ(&mut a, &"x".repeat(8010));
    assert_eq!(a.draft.chars().count(), 8000, "the draft caps at 8000");
    let seen: Vec<OutType> = (0..7)
        .map(|_| {
            key(&mut a, KeyCode::Tab);
            a.out
        })
        .collect();
    use OutType::*;
    assert_eq!(seen, [Prompt, Command, Steps, Text, Format, Hint, Auto]);
    ctrl(&mut a, 'n');
    ctrl(&mut a, 'n');
    assert_eq!(a.write_route().model, "sonnet");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Home { sel: 0 });
}

#[test]
fn ctrl_w_steps_over_multi_byte_whitespace() {
    for (sep, name) in [('\u{a0}', "no-break space"), ('\u{3000}', "ideographic space")] {
        let mut a = app("w", "zsh", "$ ");
        ch(&mut a, '2');
        typ(&mut a, &format!("one{sep}two"));
        ctrl(&mut a, 'w');
        assert_eq!(a.draft, format!("one{sep}"), "{name}");
        ctrl(&mut a, 'w');
        assert_eq!(a.draft, "", "{name}");
    }
}

#[test]
fn an_empty_draft_sends_nothing() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    ctrl(&mut a, 's');
    ctrl(&mut a, 'f');
    ctrl(&mut a, 't');
    assert_eq!((a.view.clone(), a.pending, a.quit), (View::Compose, false, false));
    assert!(a.note.is_some());
}

#[test]
fn insert_as_typed_is_a_paste_of_the_draft() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    typ(&mut a, "ls -la");
    ctrl(&mut a, 't');
    assert_eq!(
        a.action,
        Some(Action {
            verb: "paste",
            text: "ls -la".into()
        })
    );
    assert!(a.quit);
}

#[test]
fn write_sends_the_draft_the_type_and_the_question_budget() {
    let mut a = app("w", "zsh", "$ ls");
    ch(&mut a, '1');
    writing_from_compose(&mut a, "biggest files");
    let (system, prompt, model) = a.request_parts().unwrap();
    // HOME sits inside an example's line, so a path can hold the tag but never be the line (#910)
    assert!(system.lines().any(|l| l == "<examples>"), "no line is the tag");
    assert!(
        prompt.contains("<request>biggest files</request>") && prompt.contains("<output_type>command</output_type>"),
        "{prompt}"
    );
    assert!(prompt.ends_with("<questions_left>3</questions_left>"), "{prompt}");
    assert_eq!(model, "sonnet");
}

fn writing_from_compose(a: &mut App, draft: &str) {
    typ(a, draft);
    ctrl(a, 's');
}

// ---------------------------------------------------------------- questions

fn q(n: usize) -> String {
    format!("<question>Q{n}?</question><option>a{n}</option><option>b{n}</option>")
}

#[test]
fn write_asks_at_most_three_rounds() {
    let mut a = app("w", "zsh", "$ ");
    writing(&mut a, "do it");
    for n in 1..=3 {
        a.reply(Ok(q(n)));
        assert_eq!(a.view, View::Question { sel: 0, scroll: None }, "round {n}");
        ch(&mut a, '1');
        let (_, prompt, _) = a.request_parts().unwrap();
        assert!(prompt.contains(&format!("<questions_left>{}</questions_left>", 3 - n)), "{prompt}");
        assert!(prompt.contains(&format!("<turn role=\"user\">a{n}</turn>")));
    }
    a.reply(Ok(q(4)));
    assert_eq!(a.view, View::Busy, "told once to write now");
    a.reply(Ok(q(5)));
    assert!(
        matches!(&a.view, View::Error { line, hint: Some(_) } if line == "no questions left, and Claude still asks: Q5?"),
        "{:?}",
        a.view
    );
}

#[test]
fn fix_never_asks_and_explain_asks_six_times() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    typ(&mut a, "teh");
    ctrl(&mut a, 'f');
    a.reply(Ok(q(1)));
    assert!(
        matches!(&a.view, View::Error { line, .. } if line.contains("Claude still asks: Q1?")),
        "{:?}",
        a.view
    );
    for c in ['i', 's', 'c'] {
        ch(&mut a, c);
    }
    assert!(
        a.action.is_none() && matches!(a.view, View::Error { .. }),
        "a leftover question is never inserted"
    );
    key(&mut a, KeyCode::Esc);
    assert_eq!((a.view.clone(), a.draft.as_str()), (View::Compose, "teh"));

    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    ch(&mut a, '3');
    ch(&mut a, '1');
    for n in 1..=6 {
        a.reply(Ok(q(n)));
        assert_eq!(a.view, View::Question { sel: 0, scroll: None }, "round {n}");
        ch(&mut a, '2');
    }
    a.reply(Ok(q(7)));
    assert!(matches!(a.view, View::Answer { .. }), "{:?}", a.view);
}

#[test]
fn best_guess_ends_the_rounds_and_back_pops_one() {
    let mut a = app("w", "zsh", "$ ");
    writing(&mut a, "do it");
    a.reply(Ok(q(1)));
    ch(&mut a, '1');
    a.reply(Ok(q(2)));
    key(&mut a, KeyCode::Backspace);
    assert_eq!(a.run.as_ref().unwrap().question, "Q1?", "back shows round 1 again");
    assert_eq!(a.run.as_ref().unwrap().rounds, 1);
    ch(&mut a, 'g');
    let r = a.run.as_ref().unwrap();
    assert_eq!(r.rounds, r.max);
    assert!(r.turns.last().unwrap().text.contains("best guess"));
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.run.as_ref().unwrap().rounds, 1, "a cancelled best guess gives the rounds back");
    ch(&mut a, 'g');
    a.reply(Ok(q(3)));
    a.reply(Ok(q(4)));
    assert!(
        matches!(a.view, View::Error { .. }),
        "a question after the best guess and its retry is an error, never a pasteable result"
    );
}

#[test]
fn other_opens_a_one_line_answer() {
    let mut a = app("w", "zsh", "$ ");
    writing(&mut a, "do it");
    a.reply(Ok("<question>Which host?</question>".into()));
    key(&mut a, KeyCode::Enter);
    assert!(
        matches!(
            a.view,
            View::Input {
                kind: InputKind::Other,
                ..
            }
        ),
        "no options: Enter types an answer"
    );
    typ(&mut a, "lab q");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, View::Busy);
    assert_eq!(a.run.as_ref().unwrap().turns.last().unwrap().text, "lab q");
}

// ---------------------------------------------------------------- busy, errors, results

#[test]
fn esc_cancels_a_busy_request_and_kills_it() {
    let dir = std::env::temp_dir().join(format!("cockpit-panel-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fake = dir.join("claude");
    fake::write_exec(&fake, "#!/bin/sh\nexec sleep 1000\n");
    let call = Call {
        claude: fake.display().to_string(),
        timeout: Duration::from_secs(60),
        dir: dir.join("cache"),
        effort: Effort::Medium,
    };

    let mut a = app("w", "zsh", "$ ");
    writing(&mut a, "slow");
    a.launch(&call);
    let pid = a.req.as_ref().and_then(|r| r.pid()).expect("spawned");
    key(&mut a, KeyCode::Esc);
    assert!(a.req.is_none() && !a.pending);
    assert_eq!(a.view, View::Compose, "back to where it was sent from");
    assert_eq!(a.draft, "slow");
    if !Path::new("/proc/self").exists() {
        eprintln!("skipped the reap check: no /proc here");
        return;
    }
    let reaped = (0..1000).any(|_| {
        let gone = !Path::new(&format!("/proc/{pid}")).exists();
        if !gone {
            std::thread::sleep(Duration::from_millis(10));
        }
        gone
    });
    assert!(reaped, "{pid} outlived the cancel");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failure_shows_one_line_and_retries_or_goes_back() {
    let mut a = app("w", "zsh", "$ ");
    writing(&mut a, "x");
    a.reply(Err(cockpit_tui::panel::request::Fail::Failed("e".repeat(200))));
    assert_eq!(
        a.view,
        View::Error {
            line: format!("claude failed: {}...", "e".repeat(160)),
            hint: None,
        }
    );
    ch(&mut a, 'r');
    assert_eq!((a.view.clone(), a.pending), (View::Busy, true));
    a.timeout = Duration::from_secs(120);
    a.reply(Err(cockpit_tui::panel::request::Fail::Timeout));
    assert_eq!(
        a.view,
        View::Error {
            line: "claude took longer than 120 s".into(),
            hint: Some("r retries; COCKPIT_PANEL_TIMEOUT=<seconds> gives it longer".into()),
        }
    );
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Compose);
}

#[test]
fn a_result_inserts_sends_after_a_confirm_or_goes_back_to_compose() {
    let mut a = app("py", "python3", ">>> ");
    writing(&mut a, "x");
    a.reply(Ok("<text>one\ntwo</text>".into()));
    ch(&mut a, 's');
    let View::Confirm { warn, send, .. } = &a.view else {
        panic!("{:?}", a.view)
    };
    assert!(*send);
    assert_eq!(warn, &["⚠ 2 lines – 'python3' may run each line".to_string()]);
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Result { .. }));
    ch(&mut a, 'c');
    assert_eq!((a.view.clone(), a.draft.as_str()), (View::Compose, "one\ntwo"));

    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<text>one\ntwo</text>".into()));
    assert_eq!(a.line_warning("one\ntwo"), None, "bash takes a bracketed paste whole");
    ch(&mut a, 'i');
    assert_eq!(
        a.action,
        Some(Action {
            verb: "paste",
            text: "one\ntwo".into()
        })
    );

    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<prompt>p</prompt>".into()));
    ch(&mut a, 's');
    key(&mut a, KeyCode::Enter);
    assert_eq!(
        a.action,
        Some(Action {
            verb: "send",
            text: "p".into()
        })
    );
}

#[test]
fn a_multi_line_insert_where_lines_may_run_asks_first() {
    let mut a = app("py", "python3", ">>> ");
    writing(&mut a, "x");
    a.reply(Ok("<text>one\ntwo</text>".into()));
    ch(&mut a, 'i');
    let View::Confirm { warn, send, .. } = &a.view else {
        panic!("{:?}", a.view)
    };
    assert!(!*send && warn[0].contains("may run each line"), "{warn:?}");
    key(&mut a, KeyCode::Enter);
    assert_eq!(
        a.action,
        Some(Action {
            verb: "paste",
            text: "one\ntwo".into()
        })
    );

    let mut a = app("py", "python3", ">>> ");
    writing(&mut a, "x");
    a.reply(Ok("<steps><step><note>n</note><cmd>a = 1\nb = 2</cmd></step></steps>".into()));
    ch(&mut a, 'i');
    assert!(matches!(&a.view, View::Confirm { send: false, .. }), "{:?}", a.view);
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Steps { .. }));

    let mut a = app("py", "python3", ">>> ");
    let i = a.tiles().iter().position(|t| t.0 == Tile::Compose).unwrap();
    ch(&mut a, char::from(b'1' + i as u8));
    typ(&mut a, "x = 1\ny = 2");
    ctrl(&mut a, 't');
    assert!(matches!(&a.view, View::Confirm { send: false, .. }), "{:?}", a.view);
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Compose);
    typ(&mut a, "");
    ctrl(&mut a, 'u');
    typ(&mut a, "one line");
    ctrl(&mut a, 't');
    assert_eq!(
        a.action,
        Some(Action {
            verb: "paste",
            text: "one line".into()
        }),
        "one line needs no confirm"
    );
}

#[test]
fn an_answer_with_a_ready_prompt_warns_on_an_open_menu() {
    let menu = "● Edit config.py?\nDo you want to make this edit to config.py?\n❯ 1. Yes\n  2. No";
    let mut a = app("claude", "claude", menu);
    ch(&mut a, '3');
    ch(&mut a, '2');
    a.reply(Ok("Pick 1.\n<prompt>go on</prompt>".into()));
    ch(&mut a, 'i');
    let View::Confirm { warn, send, .. } = &a.view else {
        panic!("{:?}", a.view)
    };
    assert!(!*send && warn[0].contains("menu"), "{warn:?}");
    key(&mut a, KeyCode::Enter);
    assert_eq!(
        a.action,
        Some(Action {
            verb: "paste",
            text: "go on".into()
        })
    );
}

#[test]
fn a_follow_up_keeps_the_conversation() {
    let mut a = explained();
    ch(&mut a, 'f');
    typ(&mut a, "why that one?");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, View::Busy);
    let (_, prompt, _) = a.request_parts().unwrap();
    assert!(prompt.contains("<turn role=\"assistant\">Claude Code asks") && prompt.contains("<turn role=\"user\">why that one?</turn>"));
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Answer { .. }));
    assert_eq!(
        a.run.as_ref().unwrap().log.len(),
        2,
        "the cancelled follow-up leaves the log as it was"
    );
}

// ---------------------------------------------------------------- the action file

#[test]
fn the_action_file_carries_the_verb_and_a_private_clean_text() {
    let dir = std::env::temp_dir().join(format!("cockpit-panel-act-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("act");
    let a = Action {
        verb: "send",
        text: "a\u{1b}[31mb\r\nc\td\u{9b}e\u{7}".into(),
    };
    write_action(&f, "%7", "zsh", &a).unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "send %7\n");
    assert_eq!(std::fs::read_to_string(dir.join("act.cmd")).unwrap(), "zsh\n");
    let text = dir.join("act.text");
    assert_eq!(std::fs::read_to_string(&text).unwrap(), "a[31mb\nc\tde");
    assert_eq!(std::fs::metadata(&text).unwrap().permissions().mode() & 0o777, 0o600);
    // A file that is already there, or a symlink planted in its place, is
    // somebody else's: the write fails rather than reuse it.
    let (f2, planted) = (dir.join("again"), dir.join("again.text"));
    std::os::unix::fs::symlink(dir.join("elsewhere"), &planted).unwrap();
    assert!(write_action(&f2, "%7", "zsh", &a).is_err());
    assert!(
        !dir.join("elsewhere").exists() && !f2.exists(),
        "nothing followed the link, no action line"
    );
    assert!(write_action(&f, "%7", "zsh", &a).is_err(), "an existing text file is refused");
    assert_eq!(sanitize("x\u{0}y\u{7f}z\n"), "xyz\n");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- scrolling

/// An Explain answer of `n` numbered lines, long enough to scroll.
fn long_answer(n: usize) -> App {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    ch(&mut a, '3');
    ch(&mut a, '1');
    let body: Vec<String> = (1..=n).map(|i| format!("line {i}")).collect();
    a.reply(Ok(body.join("\n")));
    a
}

fn scroll_of(a: &App) -> usize {
    match &a.view {
        View::Result { scroll, .. } | View::Answer { scroll, .. } | View::Confirm { scroll, .. } => *scroll,
        v => panic!("{v:?}"),
    }
}

#[test]
fn up_at_the_end_of_an_answer_moves_at_once() {
    let mut a = long_answer(40);
    shown(&mut a, 60, 12);
    let bottom = scroll_of(&a);
    assert!(bottom > 0 && bottom < 100, "the bottom is a real offset: {bottom}");
    key(&mut a, KeyCode::Up);
    assert_eq!(scroll_of(&a), bottom - 1, "one Up is one line");
    let f = shown(&mut a, 60, 12);
    assert!(f.contains(&format!(" {bottom}-42 of 43 ")), "{f}");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    assert_eq!(scroll_of(&a), bottom, "Down stops at the bottom");
}

#[test]
fn page_keys_move_an_answer_by_a_screen() {
    let mut a = long_answer(40);
    shown(&mut a, 60, 12);
    let (bottom, page) = (scroll_of(&a), a.room - 1);
    key(&mut a, KeyCode::PageUp);
    assert_eq!(scroll_of(&a), bottom - page);
    key(&mut a, KeyCode::PageDown);
    key(&mut a, KeyCode::PageDown);
    assert_eq!(scroll_of(&a), bottom);
    for _ in 0..20 {
        key(&mut a, KeyCode::PageUp);
    }
    assert_eq!(scroll_of(&a), 0);
}

#[test]
fn a_result_never_scrolls_past_its_end() {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    let body: Vec<String> = (1..=30).map(|i| format!("r{i}")).collect();
    a.reply(Ok(format!("<text>{}</text>", body.join("\n"))));
    shown(&mut a, 60, 12);
    for _ in 0..50 {
        key(&mut a, KeyCode::Down);
    }
    let bottom = scroll_of(&a);
    assert_eq!(bottom, a.bottom);
    key(&mut a, KeyCode::Up);
    assert_eq!(scroll_of(&a), bottom - 1);
}

#[test]
fn the_position_shows_where_it_can_move() {
    let mut a = long_answer(40);
    let f = shown(&mut a, 60, 12);
    let total = 40 + 3;
    let (top, room) = (a.bottom, a.room);
    let at_end = format!(" {}-{total} of {total} ▲{top} ", top + 1);
    assert!(f.contains(&at_end), "want {at_end:?}:\n{f}");
    assert!(!f.contains('▼'), "nothing below the end:\n{f}");
    for _ in 0..100 {
        key(&mut a, KeyCode::Up);
    }
    let f = shown(&mut a, 60, 12);
    let at_top = format!(" 1-{room} of {total} ▼{} ", total - room);
    assert!(f.contains(&at_top) && !f.contains('▲'), "want {at_top:?}:\n{f}");

    let th = Theme {
        g: cockpit_tui::theme::Glyphs::ascii(),
        ..Theme::fixed()
    };
    let f = framed(&a, 60, 12, &th, 0).0;
    assert!(
        f.contains(&format!(" 1-{room} of {total} v{} ", total - room)),
        "ascii arrows:\n{f}"
    );
    assert!(!frame(&explained(), 80, 20, 0).contains(" of "), "a body that fits shows none");
}

#[test]
fn steps_show_the_position_too() {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    let steps: String = (1..=12)
        .map(|i| format!("<step><note>step {i}</note><cmd>echo {i}</cmd></step>"))
        .collect();
    a.reply(Ok(format!("<steps>{steps}</steps>")));
    let f = shown(&mut a, 60, 12);
    assert!(f.contains("1-8 of 24 ▼16"), "{f}");
}

// ---------------------------------------------------------------- confirm

#[test]
fn a_multi_line_send_needs_y_not_enter() {
    // one line is sent on Enter, whatever it runs: guarding commands is not the panel's job
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<steps><step><note>n</note><cmd>sudo rm -rf /tmp/x</cmd></step></steps>".into()));
    ch(&mut a, 's');
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.action.as_ref().map(|x| x.verb), Some("send"));

    // bash takes the paste whole, so no line warning: it is the send that runs both
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<text>make\nmake install</text>".into()));
    ch(&mut a, 's');
    assert!(shown(&mut a, 80, 20).contains("y confirm  ↑↓ scroll  ↵ is off: multi-line"));
    key(&mut a, KeyCode::Enter);
    assert!(a.action.is_none() && !a.quit, "Enter does not fire a multi-line send");
    assert!(matches!(a.view, View::Confirm { .. }) && note(&a).unwrap_or("").contains("press y"));
    ch(&mut a, 'y');
    assert_eq!(
        a.action,
        Some(Action {
            verb: "send",
            text: "make\nmake install".into()
        })
    );

    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<text>ls -la</text>".into()));
    ch(&mut a, 's');
    assert!(shown(&mut a, 80, 20).contains("↵ y confirm"));
    key(&mut a, KeyCode::Enter);
    assert_eq!(
        a.action.as_ref().map(|x| x.verb),
        Some("send"),
        "a plain one-liner still takes Enter"
    );
}

#[test]
fn q_in_a_confirm_cancels_and_keeps_the_panel() {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<text>ls</text>".into()));
    ch(&mut a, 's');
    ch(&mut a, 'q');
    assert!(!a.quit && a.action.is_none());
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
    ch(&mut a, 'q');
    assert!(a.quit, "q outside a confirm still closes");
}

fn long_confirm() -> App {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    let body: Vec<String> = (1..=40).map(|i| format!("echo step {i}")).collect();
    a.reply(Ok(format!("<text>{}</text>", body.join("\n"))));
    ch(&mut a, 's');
    a
}

#[test]
fn a_long_confirm_scrolls_and_says_what_is_cut() {
    let mut a = long_confirm();
    let f = shown(&mut a, 60, 12);
    assert!(f.contains("echo step 1 ") && f.contains("… 33 more lines"), "{f}");
    for _ in 0..100 {
        key(&mut a, KeyCode::Down);
    }
    let f = shown(&mut a, 60, 12);
    assert!(f.contains("echo step 40") && !f.contains("more line"), "{f}");
    assert!(matches!(a.view, View::Confirm { .. }));

    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<text>ls</text>".into()));
    a.pane = format!("%3 {}", "x".repeat(70));
    ch(&mut a, 's');
    let f = shown(&mut a, 60, 12);
    assert!(f.contains("(bash) and press Enter?"), "the question wraps:\n{f}");
}

#[test]
fn golden_confirm_scrolled() {
    for (w, h) in [(40u16, 12u16), (60, 16), (80, 20), (120, 24)] {
        let mut a = long_confirm();
        shown(&mut a, w, h);
        for _ in 0..5 {
            key(&mut a, KeyCode::Down);
        }
        golden(&format!("panel-confirm-scroll-{w}"), &frame(&a, w, h, 0));
    }
}

// ---------------------------------------------------------------- esc on an answer

#[test]
fn esc_on_an_answer_asks_before_it_drops_the_conversation() {
    let mut a = explained();
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Answer { armed: true, .. }), "{:?}", a.view);
    assert!(a.run.is_some() && note(&a) == Some("esc or q again to discard this conversation"));
    assert!(frame(&a, 80, 20, 0).contains("esc q again discards it"));
    key(&mut a, KeyCode::Up);
    assert!(
        matches!(a.view, View::Answer { armed: false, .. }) && a.note.is_none(),
        "any other key keeps it"
    );
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Home { sel: 0 });
    assert!(a.run.is_none() && a.note.is_none() && !a.quit);
}

// ---------------------------------------------------------------- bracketed paste

#[test]
fn a_paste_into_compose_is_taken_whole() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    typ(&mut a, "> ");
    a.paste("a\tb\r\nc\rd\u{1b}[31m\u{7}e\nq");
    assert_eq!(a.draft, "> a\tb\nc\nd[31me\nq", "tabs and newlines kept, control characters out");
    assert!(!a.quit && a.view == View::Compose, "a pasted q is text");
    assert!(a.note.is_none());
}

#[test]
fn a_paste_into_a_one_line_box_joins_its_lines() {
    let mut a = explained();
    ch(&mut a, 'f');
    a.paste("why\r\nthat\none?\u{7}");
    let View::Input { text, .. } = &a.view else {
        panic!("{:?}", a.view)
    };
    assert_eq!(text, "why that one?", "a pasted newline does not send");

    let mut a = explained();
    a.paste("ignored");
    assert!(matches!(a.view, View::Answer { .. }) && a.run.as_ref().unwrap().log.len() == 2);
}

#[test]
fn the_draft_cap_says_what_it_left_out() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    a.paste(&"x".repeat(8010));
    assert_eq!(a.draft.chars().count(), 8000);
    assert_eq!(note(&a), Some("the box holds 8000 characters: 10 left out"));
    assert!(frame(&a, 80, 20, 0).contains("10 left out"));
    ch(&mut a, 'y');
    assert_eq!(
        note(&a),
        Some("the box holds 8000 characters: 1 left out"),
        "typing at the cap says so too"
    );
    key(&mut a, KeyCode::Backspace);
    assert!(a.note.is_none());
}

#[test]
fn q_on_an_answer_is_esc_and_never_quits() {
    let mut a = explained();
    ch(&mut a, 'q');
    assert!(matches!(a.view, View::Answer { armed: true, .. }) && !a.quit, "{:?}", a.view);
    ch(&mut a, 'q');
    assert_eq!(a.view, View::Home { sel: 0 });
    assert!(a.run.is_none() && !a.quit, "the second q goes home, not out");

    let mut a = explained();
    key(&mut a, KeyCode::Esc);
    ch(&mut a, 'q');
    assert_eq!(a.view, View::Home { sel: 0 }, "esc then q discards too");
    let mut a = explained();
    ctrl(&mut a, 'c');
    assert!(a.quit, "Ctrl-C stays a hard quit");
}

#[test]
fn no_body_rows_means_no_position_and_no_scroll() {
    let mut a = long_answer(40);
    let f = shown(&mut a, 60, 2);
    assert_eq!(a.room, 0);
    assert!(!f.contains(" of "), "{f}");
    let at = scroll_of(&a);
    for k in [KeyCode::Down, KeyCode::PageDown, KeyCode::Up] {
        key(&mut a, k);
        assert_eq!(scroll_of(&a), at, "{k:?}");
    }
}

#[test]
fn esc_out_of_a_one_line_box_clears_its_cap_note() {
    let mut a = explained();
    ch(&mut a, 'f');
    a.paste(&"x".repeat(8001));
    assert!(a.note.is_some());
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Answer { .. }) && a.note.is_none(), "{:?}", a.note);
}

#[test]
fn the_programs_that_take_a_paste_whole_include_every_ssh_like_one() {
    for (cmd, whole) in [
        ("bash", true),
        ("claude", true),
        ("nvim", true),
        ("ssh", true),
        ("mosh", true),
        ("mosh-client", true),
        ("autossh", true),
        ("coder", true),
        ("proot", true),
        ("python3", false),
        ("cat", false),
        ("", false),
    ] {
        let a = app("w", cmd, "");
        assert_eq!(a.line_warning("a\nb").is_none(), whole, "{cmd}");
        assert_eq!(bracketed_ok(cmd), whole, "{cmd}");
    }
    for cmd in cockpit_tui::panel::capture::SSH_LIKE {
        assert!(bracketed_ok(cmd), "{cmd} is ssh-like, so its remote shell takes the paste");
    }
}

/// Keys a view ignores leave it exactly as it was: the views are moved out
/// while a key is handled, so a branch that forgot to put one back would show.
#[test]
fn an_ignored_key_keeps_every_view_that_owns_text() {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<text>hello</text>".into()));
    let v = a.view.clone();
    ch(&mut a, 'x');
    assert_eq!(a.view, v, "result");

    a.reply(Ok(
        "<steps><step><note>n</note><cmd>ls</cmd></step><step><note>tap it</note></step></steps>".into(),
    ));
    let v = a.view.clone();
    ch(&mut a, 'x');
    assert_eq!(a.view, v, "steps");
    key(&mut a, KeyCode::Down);
    ch(&mut a, 'i');
    assert!(
        matches!(a.view, View::Steps { sel: 1, .. }),
        "a step with no command stays: {:?}",
        a.view
    );

    key(&mut a, KeyCode::Up);
    ch(&mut a, 's');
    let v = a.view.clone();
    assert!(matches!(v, View::Confirm { .. }));
    ch(&mut a, 'x');
    assert_eq!(a.view, v, "confirm");

    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<question>which?</question>".into()));
    ch(&mut a, 'o');
    let v = a.view.clone();
    assert!(matches!(v, View::Input { .. }));
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, v, "an empty answer is not sent and the box stays");

    let mut a = app("w", "bash", "$ ");
    a.view = View::Result {
        label: "Text".into(),
        text: "t".into(),
        scroll: 0,
    };
    ch(&mut a, 'r');
    assert!(matches!(a.view, View::Result { .. }), "r with nothing to retry keeps the result");
}

#[test]
fn notes_have_a_level_and_go_with_the_view_they_were_said_on() {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    key(&mut a, KeyCode::Esc);
    assert_eq!(
        (note(&a), level(&a)),
        (Some("cancelled"), Some(Level::Info)),
        "a cancel is not an error"
    );
    assert_eq!(a.view, View::Compose, "the note survives the move that said it");
    typ(&mut a, "y");
    assert!(a.note.is_none(), "compose clears it on the next key");

    ctrl(&mut a, 'u');
    ctrl(&mut a, 't');
    assert_eq!(level(&a), Some(Level::Warn));
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Home { sel: 0 });
    assert!(a.note.is_none(), "leaving the view drops its note");

    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok(
        "<steps><step><note>tap it</note></step><step><note>n</note><cmd>ls</cmd></step></steps>".into(),
    ));
    ch(&mut a, 'i');
    assert_eq!(level(&a), Some(Level::Info));
    key(&mut a, KeyCode::Down);
    assert!(a.note.is_some(), "moving within the same view keeps it");
    ch(&mut a, 's');
    assert!(matches!(a.view, View::Confirm { .. }) && a.note.is_none(), "{:?}", a.note);
}

#[test]
fn a_note_shows_its_glyph_in_both_tiers() {
    let mut a = app("w", "bash", "$ ");
    for (lv, uni, asc) in [
        (Level::Info, "○ hi", ". hi"),
        (Level::Warn, "▲ hi", "! hi"),
        (Level::Error, "✕ hi", "x hi"),
    ] {
        a.say(lv, "hi");
        assert!(frame(&a, 60, 10, 0).contains(uni), "{lv:?}");
        let mut th = Theme::fixed();
        th.g = cockpit_tui::theme::Glyphs::ascii();
        assert!(framed(&a, 60, 10, &th, 0).0.contains(asc), "{lv:?} ascii");
    }
}

#[test]
fn a_known_failure_says_what_to_do() {
    use cockpit_tui::panel::app::hint;
    use cockpit_tui::panel::request::Fail::{Failed, Timeout};
    for (f, want) in [
        (Failed("Error: not logged in · run claude /login".into()), Some("log in")),
        (Failed("Invalid API key · Please run /login".into()), Some("log in")),
        (Failed("API Error: 429 rate_limit_error".into()), Some("rate limit")),
        (Failed("Claude AI usage limit reached|1767225600".into()), Some("rate limit")),
        (Failed("API Error: 529 Overloaded".into()), Some("rate limit")),
        (
            Failed("claude: No such file or directory (os error 2)".into()),
            Some("not installed"),
        ),
        (
            Failed("cannot write the system prompt: No such file or directory (os error 2)".into()),
            None,
        ),
        (Timeout, Some("COCKPIT_PANEL_TIMEOUT")),
        (Failed("Error: something else".into()), None),
    ] {
        let got = hint(&f);
        match want {
            Some(w) => assert!(got.is_some_and(|g| g.contains(w)), "{f:?}: {got:?}"),
            None => assert_eq!(got, None, "{f:?}"),
        }
    }
}

#[test]
fn a_file_edit_is_unsupported_without_blaming_claude() {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<edit path=\"a\">x</edit>".into()));
    let View::Error { line, hint } = &a.view else {
        panic!("{:?}", a.view)
    };
    assert_eq!(line, "file edits are not supported in the panel");
    assert!(hint.is_some());
}

#[test]
fn golden_error_long_wraps() {
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Err(cockpit_tui::panel::request::Fail::Failed(format!(
        "API Error: 429 {{\"type\":\"error\",\"error\":{{\"type\":\"rate_limit_error\",\"message\":\"{}\"}}}}",
        "This request would exceed the rate limit for your organization. ".repeat(2)
    ))));
    goldens("error-long", &a, 0);
}

const MENU: &str = "● Edit config.py?\nDo you want to make this edit to config.py?\n❯ 1. Yes\n  2. No";

fn menu_warned(a: &App) -> bool {
    matches!(&a.view, View::Confirm { warn, .. } if warn.iter().any(|w| w.contains("menu choice")))
}

#[test]
fn an_open_menu_is_warned_on_every_insert_and_send() {
    let mut a = app("claude", "claude", MENU);
    writing(&mut a, "x");
    a.reply(Ok("<prompt>go on</prompt>".into()));
    ch(&mut a, 'i');
    assert!(menu_warned(&a), "result insert confirms: {:?}", a.view);
    key(&mut a, KeyCode::Esc);
    ch(&mut a, 's');
    assert!(menu_warned(&a), "result send");
    key(&mut a, KeyCode::Esc);

    a.reply(Ok("<steps><step><note>n</note><cmd>ls</cmd></step></steps>".into()));
    ch(&mut a, 'i');
    assert!(menu_warned(&a), "step insert");
    key(&mut a, KeyCode::Esc);
    ch(&mut a, 's');
    assert!(menu_warned(&a), "step run");

    let mut a = app("claude", "claude", MENU);
    let i = a.tiles().iter().position(|t| t.0 == Tile::Compose).unwrap();
    ch(&mut a, char::from(b'1' + i as u8));
    typ(&mut a, "one line");
    ctrl(&mut a, 't');
    assert!(menu_warned(&a), "compose ^T, one line or not");
    let f = frame(&a, 80, 20, 0);
    assert!(f.contains("waiting on a menu choice"), "the confirm shows it\n{f}");
}

#[test]
fn a_working_session_warns_a_send_and_lets_an_insert_through() {
    let busy = "● Reading files\n✻ Thinking… (esc to interrupt)";
    let mut a = app("claude", "claude", busy);
    assert_eq!(a.session, Some(cockpit_tui::panel::capture::Session::Working));
    writing(&mut a, "x");
    a.reply(Ok("<prompt>go on</prompt>".into()));
    ch(&mut a, 's');
    assert!(
        matches!(&a.view, View::Confirm { warn, .. } if warn.iter().any(|w| w.contains("still working"))),
        "{:?}",
        a.view
    );
    key(&mut a, KeyCode::Esc);
    ch(&mut a, 'i');
    assert_eq!(
        a.action.as_ref().map(|x| x.verb),
        Some("paste"),
        "an insert only waits in the input"
    );
}

const THREE_STEPS: &str = "<steps><step><note>update</note><cmd>apt update</cmd></step>\
<step><note>tap Allow in the app</note></step>\
<step><note>install</note><cmd>apt install jq</cmd></step></steps>";

fn stepped() -> App {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok(THREE_STEPS.into()));
    a
}

fn resume_of(a: &App) -> Resume {
    a.resume.clone().expect("a resume")
}

#[test]
fn an_action_from_steps_carries_the_steps_to_resume() {
    let mut a = stepped();
    ch(&mut a, 'i');
    assert_eq!(a.action.as_ref().map(|x| x.verb), Some("paste"));
    let r = resume_of(&a);
    assert_eq!((r.sel, r.verb, r.done.clone(), r.items.len()), (0, "paste", vec![false; 3], 3));
    assert_eq!(r.label, "Steps (bash)");

    let mut a = stepped();
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    ch(&mut a, 's');
    key(&mut a, KeyCode::Enter);
    let r = resume_of(&a);
    assert_eq!((r.sel, r.verb), (2, "send"), "through the confirm too");

    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<text>hi</text>".into()));
    ch(&mut a, 'i');
    assert!(a.action.is_some() && a.resume.is_none(), "a Result closes the panel as before");

    let mut a = stepped();
    ch(&mut a, 'i');
    a.stop();
    assert!(a.resume.is_none(), "a hangup leaves no way back");
}

#[test]
fn a_resume_marks_the_step_and_moves_to_the_next_command() {
    let mut a = stepped();
    ch(&mut a, 'i');
    let r = resume_of(&a);
    let mut b = app("w", "zsh", "$ apt update\n");
    b.resumed("bash".into(), r.clone(), "ok");
    assert_eq!(b.cap.cmd, "bash", "the guard compares against what the steps were written for");
    let View::Steps { sel, done, .. } = &b.view else {
        panic!("{:?}", b.view)
    };
    assert_eq!((*sel, done.clone()), (2, vec![true, false, false]), "the tap step is skipped");
    assert_eq!((note(&b), level(&b)), (Some("step 1 inserted"), Some(Level::Info)));
    assert!(
        frame(&b, 80, 20, 0).contains("update  ✓ inserted"),
        "the mark says what the note says"
    );

    let mut b = app("w", "bash", "$ ");
    b.resumed("bash".into(), r.clone(), "failed:the pane now runs vim, not bash\u{1b}[2J");
    let View::Steps { sel, done, .. } = &b.view else { panic!() };
    assert_eq!((*sel, done.clone()), (0, vec![false; 3]));
    assert_eq!(level(&b), Some(Level::Error));
    assert_eq!(note(&b), Some("step 1 was not sent: the pane now runs vim, not bash[2J"));

    let mut b = app("w", "bash", "$ ");
    b.resumed("bash".into(), r, "maybe");
    assert_eq!(level(&b), Some(Level::Error), "an unknown answer is not taken as ok");

    let mut last = stepped();
    last.view = View::Steps {
        label: "Steps".into(),
        items: vec![Step {
            note: "n".into(),
            cmd: "ls".into(),
            lang: "bash".into(),
        }],
        sel: 0,
        done: vec![false],
    };
    ch(&mut last, 's');
    key(&mut last, KeyCode::Enter);
    let mut b = app("w", "bash", "$ ");
    b.resumed("bash".into(), resume_of(&last), "ok");
    assert!(matches!(&b.view, View::Steps { sel: 0, .. }), "the last step stays selected");
    assert_eq!(note(&b), Some("step 1 sent"));
}

#[test]
fn golden_steps_resumed() {
    let mut a = stepped();
    ch(&mut a, 'i');
    let mut b = app("w", "bash", "$ apt update\n");
    b.resumed("bash".into(), resume_of(&a), "ok");
    goldens("steps-resumed", &b, 0);
}

fn state_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cockpit-panel-state-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_state_file_round_trips_and_is_private() {
    let dir = state_dir("rt");
    let r = Resume {
        label: "Steps (bash)".into(),
        items: vec![
            Step {
                note: "a\ttab and a \\ slash".into(),
                cmd: "printf 'x\\n'\nline two\r".into(),
                lang: "bash".into(),
            },
            Step {
                note: "tap it".into(),
                cmd: String::new(),
                lang: String::new(),
            },
        ],
        sel: 1,
        done: vec![true, false],
        verb: "send",
    };
    write_state(&dir.join("act"), "%3", "zsh", &r).unwrap();
    let p = dir.join("act.state");
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(read_state(&p, "%3"), Ok(("zsh".to_string(), r.clone())));
    assert!(
        write_state(&dir.join("act"), "%3", "zsh", &r).is_err(),
        "an existing state file is refused"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_state_file_is_refused_unless_it_is_exactly_ours() {
    let dir = state_dir("bad");
    let good = "cockpit-panel-state 1\npane %3\ncmd bash\nlabel L\nsel 0\nverb paste\nstep 0\tbash\tn\tls\n";
    let p = dir.join("s");
    let try_ = |body: &str, pane: &str| {
        std::fs::write(&p, body).unwrap();
        read_state(&p, pane)
    };
    assert!(try_(good, "%3").is_ok());
    for (body, why) in [
        (good.replace("state 1", "state 2"), "another version"),
        (good.trim_end().to_string(), "cut short"),
        (good.replace("sel 0", "sel 1"), "sel past the steps"),
        (good.replace("verb paste", "verb run"), "an unknown verb"),
        (good.replace("step 0\t", "step 2\t"), "a bad done mark"),
        (good.replace("\tls\n", "\tls\textra\n"), "a fifth field"),
        (good.replace("\tls\n", "\tl\\qs\n"), "a bad escape"),
        (good.replace("label L\n", ""), "a missing line"),
        (good.replace("step 0\tbash\tn\tls\n", ""), "no steps"),
        (format!("{good}junk\n"), "a stray line"),
        (format!("{good}{}", "step 0\tbash\tn\tls\n".repeat(64)), "too many steps"),
        (format!("{good}{}", "x".repeat(70 * 1024)), "too big"),
    ] {
        assert!(try_(&body, "%3").is_err(), "{why}");
    }
    assert!(try_(good, "%4").unwrap_err().contains("another pane"));
    let link = dir.join("link");
    std::os::unix::fs::symlink(&p, &link).unwrap();
    assert!(read_state(&link, "%3").is_err(), "never followed");
    assert!(read_state(&dir, "%3").is_err(), "a directory");
    let fifo = dir.join("fifo");
    let c = std::ffi::CString::new(fifo.display().to_string()).unwrap();
    // SAFETY: a valid C path; mkfifo only creates the node.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert!(read_state(&fifo, "%3").is_err(), "a FIFO is refused without blocking on its open");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn steps_too_long_to_keep_say_so_and_leave_no_state() {
    for reply in [
        format!("<steps>{}</steps>", "<step><note>n</note><cmd>ls</cmd></step>".repeat(65)),
        format!("<steps><step><note>n</note><cmd>{}</cmd></step></steps>", "x".repeat(70 * 1024)),
    ] {
        let mut a = app("w", "bash", "$ ");
        writing(&mut a, "x");
        a.reply(Ok(reply));
        assert!(matches!(a.view, View::Steps { .. }));
        assert_eq!(note(&a), Some("steps too long to keep open: the panel closes after one"));
        ch(&mut a, 'i');
        key(&mut a, KeyCode::Enter);
        assert!(a.action.is_some() && a.resume.is_none(), "the action goes, with no state behind it");
    }
    let a = stepped();
    assert!(a.note.is_none(), "three steps fit");
    let dir = state_dir("long");
    let r = Resume {
        label: "L".into(),
        items: vec![Step::default(); 65],
        sel: 0,
        done: vec![false; 65],
        verb: "paste",
    };
    assert!(write_state(&dir.join("act"), "%3", "bash", &r).is_err());
    assert!(!dir.join("act.state").exists(), "nothing written that a resume would refuse");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_answer_box_with_no_conversation_stays_put() {
    for kind in [InputKind::Other, InputKind::FollowUp] {
        let mut a = app("w", "bash", "$ ");
        a.view = View::Input {
            kind,
            text: "hi".into(),
            cur: 2,
            back: Box::new(View::Home { sel: 0 }),
        };
        let v = a.view.clone();
        key(&mut a, KeyCode::Enter);
        assert_eq!((a.view.clone(), a.pending), (v, false), "{kind:?}");
    }
}

#[test]
fn busy_counts_the_screen_apart_from_the_conversation() {
    let mut a = app("w", "bash", "$ ls\nghp_abcdefghijABCDEFGHIJ12");
    writing(&mut a, "x");
    a.reply(Ok("<question>which key?</question>".into()));
    ch(&mut a, 'o');
    typ(&mut a, "use sk-abcdefghijklmnopqrst1 and sk-abcdefghijklmnopqrst2");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, View::Busy);
    let f = frame(&a, 100, 20, 0);
    assert!(
        f.contains("sent: screen 2 lines, 1 hidden, 2 more in the conversation · write (auto)"),
        "{f}"
    );
}

// ---------------------------------------------------------------- small screens

fn asking() -> App {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    ch(&mut a, '3');
    a
}

#[test]
fn golden_explain_ask() {
    let mut a = asking();
    key(&mut a, KeyCode::Down);
    goldens("explain-ask", &a, 0);
}

#[test]
fn golden_input() {
    let mut a = asking();
    ch(&mut a, '5');
    typ(&mut a, "why is the build red");
    assert!(matches!(
        a.view,
        View::Input {
            kind: InputKind::Explain,
            ..
        }
    ));
    goldens("input", &a, 0);
}

/// Every view, with the text that is its heading and a line of its body.
fn every_view() -> Vec<(&'static str, App, &'static str, &'static str)> {
    let mut v = vec![("home", app("claude", "claude", CLAUDE_SCREEN), "panel", "Write a prompt")];
    let mut a = app("work", "bash", "$ ");
    ch(&mut a, '1');
    typ(&mut a, "fix the flaky test");
    v.push(("compose", a, "type: ", "flaky"));
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    v.push(("busy", a, "writing", "writing"));
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<question>Which file?</question><option>one</option><option>two</option>".into()));
    v.push(("question", a, "Which file?", "one"));
    let mut a = asking();
    ch(&mut a, '5');
    v.push(("input", a, "what do you", "▏"));
    let mut a = app("py", "python3", ">>> ");
    writing(&mut a, "x");
    a.reply(Ok("<text>users = {\n    \"alice\": 1,\n}</text>".into()));
    v.push(("result", a, "Text", "users"));
    v.push(("steps", stepped(), "Steps (bash)", "update"));
    let mut a = stepped();
    ch(&mut a, 'i');
    let mut b = app("w", "bash", "$ ");
    b.resumed("bash".into(), resume_of(&a), "ok");
    v.push(("steps-resumed", b, "Steps (bash)", "install"));
    v.push(("explain-ask", asking(), "what do you want", "What is it doing?"));
    v.push(("answer", explained(), "panel", "parser"));
    let mut a = explained();
    key(&mut a, KeyCode::Esc);
    v.push(("answer-armed", a, "discard", "parser"));
    v.push(("confirm", long_confirm(), "Enter?", "echo step"));
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Err(cockpit_tui::panel::request::Fail::Timeout));
    v.push(("error", a, "claude", "claude"));
    v
}

fn fits(f: &str, head: &str, body: &str) -> bool {
    let last = f.lines().last().unwrap_or("");
    f.contains(head) && f.contains(body) && last.contains("esc") && !f.contains("make the window larger")
}

#[test]
fn every_view_fits_at_the_floor() {
    use cockpit_tui::panel::view::{MIN_H, MIN_W};
    for (name, mut a, head, body) in every_view() {
        let f = shown(&mut a, MIN_W, MIN_H);
        assert!(fits(&f, head, body), "{name} at the floor:\n{f}");
        assert!(a.room >= 1, "{name}: a body row");
        for (w, h) in [(MIN_W - 1, 40), (200, MIN_H - 1)] {
            let f = shown(&mut a, w, h);
            assert!(f.contains("make the window larger"), "{name} at {w}x{h}:\n{f}");
        }
    }
}

#[test]
fn golden_small() {
    use cockpit_tui::panel::view::{MIN_H, MIN_W};
    let a = app("claude", "claude", CLAUDE_SCREEN);
    golden("panel-small", &frame(&a, MIN_W - 1, MIN_H, 0));
}

#[test]
fn under_the_floor_esc_and_q_act_as_on_the_view_underneath() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    typ(&mut a, "keep me");
    a.tiny = true;
    ch(&mut a, 'x');
    ctrl(&mut a, 's');
    assert_eq!(
        (a.view.clone(), a.draft.as_str()),
        (View::Compose, "keep me"),
        "unseen keys are dropped"
    );
    key(&mut a, KeyCode::Esc);
    assert_eq!(
        (a.view.clone(), a.draft.as_str(), a.quit),
        (View::Home { sel: 0 }, "keep me", false),
        "esc goes home"
    );
    key(&mut a, KeyCode::Esc);
    assert!(a.quit && a.action.is_none(), "only Home closes");

    let mut a = explained();
    a.tiny = true;
    key(&mut a, KeyCode::Esc);
    assert!(
        matches!(a.view, View::Answer { armed: true, .. }) && a.run.is_some(),
        "an answer arms first"
    );
    let mut a = explained();
    a.tiny = true;
    ch(&mut a, 'q');
    assert!(
        matches!(a.view, View::Answer { armed: true, .. }) && !a.quit,
        "q is esc on an answer"
    );
    ctrl(&mut a, 'c');
    assert!(a.quit, "^C is the hard quit");
}

#[test]
fn footers_drop_whole_hints_and_keep_esc_and_help() {
    for (name, mut a, ..) in every_view() {
        for w in [28u16, 30, 40, 60, 80, 120] {
            let f = shown(&mut a, w, 16);
            let last = f.lines().last().unwrap();
            assert!(last.contains("esc"), "{name} at {w}: {last}");
            if !matches!(a.view, View::Answer { armed: true, .. }) {
                assert!(last.contains("? keys") || last.contains("F1 keys"), "{name} at {w}: {last}");
            }
            // every hint shown is one the view's full footer has, never a cut one
            assert!(!last.contains('…'), "{name} at {w}: {last}");
        }
    }
    let mut a = app("work", "bash", "$ ");
    ch(&mut a, '1');
    let f = shown(&mut a, 40, 12);
    assert!(f.contains("╰ ^S write  F1 keys  esc home ─"), "{f}");
}

#[test]
fn help_lists_every_key_of_the_view_and_any_key_closes_it() {
    let mut a = stepped();
    ch(&mut a, '?');
    assert_eq!(a.help, Some(0));
    let f = shown(&mut a, 60, 16);
    for k in [
        "1-3",
        "pick a step by number",
        "send the step with Enter",
        "back to the draft",
        "any key closes",
    ] {
        assert!(f.contains(k), "{k}:\n{f}");
    }
    let view = a.view.clone();
    ch(&mut a, 's');
    assert_eq!((a.help, &a.view, a.quit), (None, &view, false), "the key only closes it");

    // ? is text in a box: F1 is the way in there, and the overlay names ^N, ^R, ⇥ and F2
    let mut a = app("work", "bash", "$ ");
    ch(&mut a, '1');
    ch(&mut a, '?');
    assert_eq!((a.help, a.draft.as_str()), (None, "?"));
    key(&mut a, KeyCode::F(1));
    let f = shown(&mut a, 60, 17);
    for k in ["^N", "next model", "^R", "explore first", "⇥", "^S F2", "back to home"] {
        assert!(f.contains(k), "{k}:\n{f}");
    }
    ctrl(&mut a, 'c');
    assert!(a.quit, "^C still closes the panel under the overlay");
}

#[test]
fn a_short_help_scrolls() {
    let mut a = app("work", "bash", "$ ");
    ch(&mut a, '1');
    key(&mut a, KeyCode::F(1));
    let f = shown(&mut a, 40, 9);
    assert!(
        f.contains("1-7 of 16") && f.contains("any other key closes") && !f.contains("close the panel"),
        "{f}"
    );
    for _ in 0..9 {
        key(&mut a, KeyCode::Down);
    }
    let f = shown(&mut a, 40, 9);
    assert!(f.contains("10-16 of 16") && f.contains("close the panel"), "{f}");
    key(&mut a, KeyCode::Char('x'));
    assert_eq!((a.help, a.draft.as_str()), (None, ""), "the closing key types nothing");
}

#[test]
fn an_overscrolled_help_moves_back_at_once() {
    let mut a = app("work", "bash", "$ ");
    ch(&mut a, '1');
    key(&mut a, KeyCode::F(1));
    shown(&mut a, 40, 9);
    for _ in 0..20 {
        key(&mut a, KeyCode::Down);
        shown(&mut a, 40, 9);
    }
    assert_eq!(a.help, Some(9), "16 keys in 7 rows: 9 is the last offset");
    key(&mut a, KeyCode::Up);
    assert!(shown(&mut a, 40, 9).contains("9-15 of 16"), "the first Up moves");
}

#[test]
fn golden_help() {
    let mut a = stepped();
    ch(&mut a, '?');
    goldens("help-steps", &a, 0);
    let mut a = app("work", "bash", "$ ");
    ch(&mut a, '1');
    key(&mut a, KeyCode::F(1));
    goldens("help-compose", &a, 0);
}

#[test]
fn home_tiles_drop_their_description_when_narrow() {
    let a = app("claude", "claude", CLAUDE_SCREEN);
    let f = frame(&a, 53, 12, 0);
    assert!(!f.contains("type once"), "under 50 columns of text:\n{f}");
    let f = frame(&a, 60, 12, 0);
    assert!(f.contains("type once: fix spelling, write, …"), "cut with an ellipsis:\n{f}");
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    a.cap.path = format!("/home/u/{}", "deep/".repeat(20));
    let f = frame(&a, 40, 12, 0);
    assert!(f.contains("/home/u/deep/deep… │"), "the cap line ends in an ellipsis:\n{f}");
}

#[test]
fn home_and_question_scroll_to_the_selection() {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    for _ in 0..4 {
        key(&mut a, KeyCode::Down);
    }
    let f = shown(&mut a, 40, 8);
    assert!(f.contains("▎5   Close"), "{f}");

    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    let long = "Which of these files should I change, given that both hold a timeout? ".repeat(4);
    a.reply(Ok(format!(
        "<question>{long}</question><option>first</option><option>second</option><option>third</option>"
    )));
    let f = shown(&mut a, 40, 12);
    assert!(f.contains("▎1   first"), "the options stay on screen:\n{f}");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    let f = shown(&mut a, 40, 12);
    assert!(f.contains("▎3   third"), "{f}");
}

#[test]
fn a_long_option_wraps_under_itself() {
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok(
        "<question>Which?</question><option>the configuration file in your home directory</option>".into(),
    ));
    let f = frame(&a, 40, 12, 0);
    assert!(f.contains("│ ▎1   the configuration file in your  │"), "{f}");
    assert!(f.contains("│      home directory"), "{f}");
}

#[test]
fn digits_select_steps() {
    let mut a = stepped();
    ch(&mut a, '3');
    assert!(matches!(a.view, View::Steps { sel: 2, .. }));
    ch(&mut a, '9');
    assert!(matches!(a.view, View::Steps { sel: 2, .. }), "no step 9: nothing moves");
    let f = shown(&mut a, 80, 20);
    assert!(f.contains("s send") && !f.contains("s run"), "{f}");
}

#[test]
fn the_title_gives_way_before_the_count() {
    let mut a = long_answer(60);
    a.cap.name = "a-long-window-name-for-a-narrow-popup".into();
    let top = |a: &mut App, w: u16| shown(a, w, 12).lines().next().unwrap().to_string();
    let t = top(&mut a, 80);
    assert!(t.contains("a-long-window-name") && t.contains(" of 63 "), "{t}");
    key(&mut a, KeyCode::Up);
    let t = top(&mut a, 40);
    assert!(
        t.contains("panel ·") && t.contains('…') && t.contains('▲') && t.contains('▼'),
        "title cut first: {t}"
    );
    let t = top(&mut a, 32);
    assert!(!t.contains('▲') && t.contains(" of "), "then the arrows go: {t}");
    let n = t.find(" of ").unwrap();
    assert!(
        t[..n].trim_end().chars().last().unwrap().is_ascii_digit(),
        "the count's start is whole: {t}"
    );
}

#[test]
fn arming_esc_at_the_bottom_keeps_the_last_line() {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    ch(&mut a, '3');
    ch(&mut a, '1');
    let body: Vec<String> = (1..=40).map(|i| format!("line {i}")).collect();
    a.reply(Ok(format!("{}\n<prompt>run the last step</prompt>", body.join("\n"))));
    assert!(shown(&mut a, 60, 16).contains("run the last step"));
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Answer { armed: true, .. }));
    let f = shown(&mut a, 60, 16);
    assert!(
        f.contains("run the last step"),
        "the note takes a row from the top, not the end:\n{f}"
    );
}

// ---------------------------------------------------------------- verifier round

#[test]
fn under_the_floor_q_in_a_box_is_dropped_not_typed() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    a.tiny = true;
    ch(&mut a, 'q');
    assert_eq!((a.draft.as_str(), a.quit), ("", false), "compose");
    let mut a = asking();
    ch(&mut a, '5');
    a.tiny = true;
    ch(&mut a, 'q');
    assert!(matches!(&a.view, View::Input { text, .. } if text.is_empty()) && !a.quit, "input");
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::ExplainAsk { .. }), "esc still goes back from a box");
}

#[test]
fn under_the_floor_a_paste_is_dropped() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    a.tiny = true;
    a.paste("rm -rf ~");
    assert_eq!(a.draft, "");
    a.tiny = false;
    a.paste("ls");
    assert_eq!(a.draft, "ls");
}

#[test]
fn going_under_the_floor_closes_the_overlay() {
    let mut a = explained();
    ch(&mut a, '?');
    assert!(a.help.is_some());
    a.shrunk(true);
    assert_eq!(a.help, None);
    key(&mut a, KeyCode::Esc);
    assert!(
        matches!(a.view, View::Answer { armed: true, .. }),
        "the card's esc goes back, it arms"
    );
}

fn long_question() -> App {
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    let long = (1..=12).map(|i| format!("part{i}")).collect::<Vec<_>>().join(" ");
    a.reply(Ok(format!(
        "<question>START {long}</question><option>first</option><option>second</option>"
    )));
    a
}

#[test]
fn a_long_question_shows_its_position_and_scrolls_to_its_start() {
    let mut a = long_question();
    let f = shown(&mut a, 30, 7);
    assert!(
        f.contains("▎1   first") && !f.contains("START") && f.contains(" of "),
        "options and a count:\n{f}"
    );
    key(&mut a, KeyCode::PageUp);
    let f = shown(&mut a, 30, 7);
    assert!(f.contains("START") && f.contains(" 1-"), "PgUp reads the start:\n{f}");
    key(&mut a, KeyCode::Down);
    let f = shown(&mut a, 30, 7);
    assert!(f.contains("▎2   second"), "a move follows the selection again:\n{f}");
}

#[test]
fn heads_and_the_overlay_footer_end_in_an_ellipsis_when_cut() {
    let mut a = app("work", "bash", "$ ");
    ch(&mut a, '1');
    let f = shown(&mut a, 28, 12);
    assert!(f.contains("│ type: command   model: … │"), "{f}");
    let a = asking();
    let f = frame(&a, 28, 12, 0);
    assert!(f.contains("│ what do you want to kno… │"), "{f}");
    let mut a = app("work", "bash", "$ ");
    ch(&mut a, '1');
    key(&mut a, KeyCode::F(1));
    let f = shown(&mut a, 28, 8);
    let last = f.lines().last().unwrap();
    assert!(last.contains("any other ke… ╯"), "{last}");
}

#[test]
fn a_cut_error_or_busy_says_how_much_is_left() {
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Err(cockpit_tui::panel::request::Fail::Failed("word ".repeat(80))));
    let f = shown(&mut a, 30, 8);
    assert!(f.contains("more lines"), "{f}");
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    a.timeout = Duration::from_secs(120);
    let f = frame(&a, 30, 8, 400);
    assert!(f.contains("more line"), "{f}");
}

#[test]
fn the_overlay_lists_backspace_and_real_digit_ranges() {
    let mut a = app("w", "htop", "$ ");
    ch(&mut a, '?');
    let f = shown(&mut a, 60, 16);
    assert!(f.contains("1-4  ") && !f.contains("1-9"), "four tiles:\n{f}");
    key(&mut a, KeyCode::Esc);
    ch(&mut a, '1');
    key(&mut a, KeyCode::F(1));
    assert!(shown(&mut a, 60, 16).contains("⌫ ^W ^U"), "compose");
    let mut a = asking();
    ch(&mut a, '5');
    key(&mut a, KeyCode::F(1));
    assert!(shown(&mut a, 60, 16).contains("⌫ ^W ^U"), "input");
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok(
        "<question>Which?</question><option>a</option><option>b</option><option>c</option>".into(),
    ));
    ch(&mut a, '?');
    let f = shown(&mut a, 60, 16);
    assert!(f.contains("1-3  ") && f.contains("answer by number"), "{f}");
}

#[test]
fn a_resumed_step_says_inserted_or_sent_in_both_tiers() {
    let mut a = stepped();
    ch(&mut a, 's');
    ch(&mut a, 'y');
    let r = resume_of(&a);
    assert_eq!(r.verb, "send");
    let mut b = app("w", "bash", "$ ");
    b.resumed("bash".into(), r, "ok");
    assert!(frame(&b, 80, 20, 0).contains("update  ✓ sent") && note(&b) == Some("step 1 sent"));
    let mut th = Theme::fixed();
    th.g = cockpit_tui::theme::Glyphs::ascii();
    assert!(framed(&b, 80, 20, &th, 0).0.contains(&format!("update  {} sent", th.g.done)));

    let mut a = stepped();
    ch(&mut a, 'i');
    let mut b = app("w", "bash", "$ ");
    b.resumed("bash".into(), resume_of(&a), "ok");
    assert!(framed(&b, 80, 20, &th, 0).0.contains(&format!("update  {} inserted", th.g.done)));
}

// ---------------------------------------------------------------- tablet mode

fn tablet(mut a: App) -> App {
    a.tablet = true;
    a
}

/// A tablet in landscape, and a phone in portrait.
fn tablet_goldens(name: &str, app: &App) {
    golden(&format!("panel-tablet-{name}-80"), &frame(app, 80, 24, 0));
    golden(&format!("panel-tablet-{name}-40"), &frame(app, 40, 20, 0));
}

#[test]
fn golden_tablet_home() {
    tablet_goldens("home", &tablet(app("claude", "claude", CLAUDE_SCREEN)));
}

#[test]
fn golden_tablet_compose() {
    let mut a = tablet(app("work", "bash", "$ make test\nFAIL tests/api.rs"));
    ch(&mut a, '1');
    typ(&mut a, "fix the flaky api test then rerun it");
    tablet_goldens("compose", &a);
}

/// The ^R button, tapped: explore on, and the head says so.
#[test]
fn golden_tablet_compose_explore() {
    let mut a = tablet(app("work", "bash", "$ "));
    ch(&mut a, '1');
    typ(&mut a, "fix it");
    let (f, _, hits) = drawn(&a, 40, 20, &Theme::fixed(), 0);
    let r = Tap::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(hits.iter().any(|h| h.tap == r), "an explore button:\n{f}");
    a.frame(0);
    a.tap(r, 10_000);
    assert!(a.explore, "the tap is ^R");
    tablet_goldens("compose-explore", &a);
}

fn tablet_steps() -> App {
    let mut a = tablet(app("work", "bash", "$ "));
    writing(&mut a, "clean the build and reinstall");
    a.reply(Ok(
        "<steps>\n<step><note>Remove the old build</note><cmd lang=\"bash\">rm -rf target</cmd></step>\n\
                <step><note>Build again</note><cmd lang=\"bash\">cargo build --release</cmd></step>\n\
                <step>Tap Allow in the app</step>\n</steps>"
            .into(),
    ));
    a
}

#[test]
fn golden_tablet_steps() {
    tablet_goldens("steps", &tablet_steps());
}

#[test]
fn golden_tablet_confirm() {
    let mut a = tablet_steps();
    ch(&mut a, 's');
    assert!(matches!(a.view, View::Confirm { .. }));
    tablet_goldens("confirm", &a);
}

/// Past any tap guard: no test frame stamps the clock unless it means to.
const LATER: u64 = 1_000_000;

fn at(a: &mut App, x: u16, y: u16, kind: MouseEventKind, hits: &[Hit]) {
    at_with(a, x, y, kind, hits, KeyModifiers::NONE, LATER);
}

fn at_with(a: &mut App, x: u16, y: u16, kind: MouseEventKind, hits: &[Hit], modifiers: KeyModifiers, now: u64) {
    mouse(
        a,
        hits,
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers,
        },
        now,
    );
}

/// Draws a frame as the loop does, then presses the left button on the first
/// cell of `label` as drawn: the hit map must match what is on screen.
fn tap_on(a: &mut App, w: u16, h: u16, label: &str) -> Vec<Hit> {
    let (f, (total, room), hits) = drawn(a, w, h, &Theme::fixed(), 0);
    a.fit(total, room);
    let (y, line) = f
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains(label))
        .unwrap_or_else(|| panic!("no {label:?} on screen:\n{f}"));
    // the TestBackend line is quoted: one char of the line before column 0
    let x = line[..line.find(label).unwrap()].chars().count() - 1;
    at(a, x as u16, y as u16, MouseEventKind::Down(MouseButton::Left), &hits);
    hits
}

fn tap(a: &mut App, label: &str) -> Vec<Hit> {
    tap_on(a, 80, 24, label)
}

fn tabbed(name: &str, cmd: &str, screen: &str) -> App {
    tablet(app(name, cmd, screen))
}

#[test]
fn the_mode_comes_from_the_option_and_auto_follows_termux() {
    use cockpit_tui::panel::tablet_mode;
    for (v, termux, want) in [
        (None, false, false),
        (None, true, true),
        (Some(""), true, true),
        (Some("auto"), false, false),
        (Some("auto"), true, true),
        (Some("pc"), true, false),
        (Some("tablet"), false, true),
    ] {
        assert_eq!(tablet_mode(v, termux), Ok(want), "{v:?} termux={termux}");
    }
    for bad in ["Tablet", "on", "touch", " pc"] {
        assert!(tablet_mode(Some(bad), true).is_err(), "{bad:?} is refused");
    }
}

#[test]
fn the_home_tile_flips_the_mode_both_ways() {
    let mut a = app("w", "zsh", "$ ");
    assert!(!a.tablet);
    let i = a.tiles().iter().position(|t| t.0 == Tile::Mode).unwrap();
    ch(&mut a, char::from(b'1' + i as u8));
    assert!(a.tablet && matches!(a.view, View::Home { .. }));
    assert_eq!(note(&a), Some("tablet mode until the panel closes"));
    assert_eq!(a.tiles()[i].1, "PC mode");
    tap(&mut a, "PC mode");
    assert!(!a.tablet, "a tap on the tile flips it back");
    assert_eq!(note(&a), Some("pc mode until the panel closes"));
}

#[test]
fn pc_mode_records_no_hits_and_ignores_the_mouse() {
    let mut a = app("w", "zsh", "$ ");
    let (_, _, hits) = drawn(&a, 80, 24, &Theme::fixed(), 0);
    assert!(hits.is_empty());
    at(&mut a, 5, 2, MouseEventKind::Down(MouseButton::Left), &hits);
    assert_eq!(a.view, View::Home { sel: 0 });
}

#[test]
fn a_tap_on_a_home_tile_opens_it_on_any_of_its_rows() {
    let mut a = tabbed("w", "zsh", "$ ");
    let (f, _, hits) = drawn(&a, 80, 24, &Theme::fixed(), 0);
    assert!(f.contains("╭────"), "tall tiles at 80x24:\n{f}");
    // row 4 is the second tile's top border
    at(&mut a, 40, 4, MouseEventKind::Down(MouseButton::Left), &hits);
    assert_eq!((a.view.clone(), a.out), (View::Compose, OutType::Auto));
    let mut a = tabbed("w", "zsh", "$ ");
    tap(&mut a, "Explain");
    assert_eq!(a.view, View::ExplainAsk { sel: 0 });
    let mut a = tabbed("w", "zsh", "$ ");
    tap(&mut a, "[ close ]");
    assert!(a.quit && a.action.is_none());
}

#[test]
fn one_tap_is_one_action_the_release_does_nothing() {
    let mut a = tabbed("w", "zsh", "$ ");
    let hits = tap(&mut a, "Tablet mode".replace("Tablet", "PC").as_str());
    assert!(!a.tablet);
    // the release lands on the same tile, now reading "Tablet mode": it must not flip it back
    let (_, _, now) = drawn(&a, 80, 24, &Theme::fixed(), 0);
    assert!(now.is_empty(), "pc mode again");
    let h = hits.iter().find(|h| h.tap == Tap::Pick(3)).unwrap();
    for kind in [
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Moved,
    ] {
        at(&mut a, h.at.x, h.at.y, kind, &hits);
        assert!(!a.tablet && a.view == View::Home { sel: 3 }, "{kind:?}");
    }
}

#[test]
fn a_tap_outside_every_hit_does_nothing() {
    let mut a = tabbed("w", "zsh", "$ ");
    let (_, _, hits) = drawn(&a, 80, 24, &Theme::fixed(), 0);
    for (x, y) in [(0, 0), (79, 23), (40, 18)] {
        at(&mut a, x, y, MouseEventKind::Down(MouseButton::Left), &hits);
    }
    assert!(a.view == View::Home { sel: 0 } && !a.quit);
}

#[test]
fn compose_has_a_button_for_every_ctrl_chord() {
    let mut a = tabbed("w", "zsh", "$ ");
    tap(&mut a, "Compose");
    typ(&mut a, "list files");
    tap(&mut a, "[ type ]");
    assert_eq!(a.out, OutType::Prompt);
    tap(&mut a, "[ model ]");
    assert_eq!(a.model, 1);
    tap(&mut a, "[ write ]");
    assert_eq!(a.view, View::Busy);
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Compose);
    tap(&mut a, "[ fix spelling ]");
    assert!(a.view == View::Busy && matches!(a.run.as_ref().unwrap().flow, cockpit_tui::panel::envelope::Flow::Fix { .. }));
    tap(&mut a, "[ cancel ]");
    assert_eq!(a.view, View::Compose);
    tap(&mut a, "[ keys ]");
    assert!(a.help.is_some());
    let (_, _, hits) = drawn(&a, 80, 24, &Theme::fixed(), 0);
    at(&mut a, 3, 3, MouseEventKind::Down(MouseButton::Left), &hits);
    assert_eq!(a.help, None, "a tap anywhere closes the overlay");
    assert_eq!(a.draft, "list files", "and types nothing");
    tap(&mut a, "[ insert as typed ]");
    assert_eq!(
        a.action,
        Some(Action {
            verb: "paste",
            text: "list files".into()
        })
    );
    let mut a = tabbed("w", "zsh", "$ ");
    tap(&mut a, "Compose");
    tap(&mut a, "[ home ]");
    assert_eq!(a.view, View::Home { sel: 0 });
}

#[test]
fn question_options_and_buttons_are_taps() {
    let ask = || {
        let mut a = tabbed("work", "bash", "$ ");
        writing(&mut a, "x");
        a.reply(Ok(
            "<question>Which?</question><option>first</option><option>second</option><option>third</option>".into(),
        ));
        a
    };
    let mut a = ask();
    tap(&mut a, "third");
    assert_eq!(a.view, View::Busy, "a tap picks: it is the Enter on that option");
    assert_eq!(a.run.as_ref().unwrap().turns.last().unwrap().text, "third");
    let mut a = ask();
    tap(&mut a, "[ other ]");
    assert!(matches!(
        a.view,
        View::Input {
            kind: InputKind::Other,
            ..
        }
    ));
    typ(&mut a, "mine");
    tap(&mut a, "[ send ]");
    assert_eq!(a.view, View::Busy);
    let mut a = ask();
    tap(&mut a, "[ best guess ]");
    assert_eq!(a.view, View::Busy);
    let mut a = ask();
    tap(&mut a, "[ back ]");
    assert_eq!(a.view, View::Compose);
}

#[test]
fn explain_questions_are_taps() {
    let mut a = tablet(asking());
    tap(&mut a, "Is something wrong?");
    assert_eq!(a.view, View::Busy);
    let mut a = tablet(asking());
    tap(&mut a, "In your own words");
    assert!(matches!(
        a.view,
        View::Input {
            kind: InputKind::Explain,
            ..
        }
    ));
}

#[test]
fn result_buttons_insert_send_copy_and_edit() {
    let res = || {
        let mut a = tabbed("w", "zsh", "$ ");
        writing(&mut a, "x");
        a.reply(Ok("<text>ls -la</text>".into()));
        a
    };
    let mut a = res();
    tap(&mut a, "[ insert ]");
    assert_eq!(a.action.as_ref().map(|x| x.verb), Some("paste"));
    let mut a = res();
    tap(&mut a, "[ copy ]");
    assert_eq!(
        (a.copy.as_deref(), note(&a)),
        (Some("ls -la"), Some("sent 6 chars to the clipboard (needs tmux set-clipboard on)"))
    );
    assert!(matches!(a.view, View::Result { .. }) && a.action.is_none());
    tap(&mut a, "[ send ]");
    assert!(matches!(a.view, View::Confirm { send: true, .. }));
    tap(&mut a, "no");
    assert!(matches!(a.view, View::Result { .. }));
    tap(&mut a, "[ edit ]");
    assert_eq!((a.view.clone(), a.draft.as_str()), (View::Compose, "ls -la"));
}

#[test]
fn a_tablet_send_is_confirmed_by_the_yes_button_which_is_y() {
    let mut a = tablet(stepped());
    let mut b = tablet_steps();
    tap(&mut b, "[ send ]");
    let f = frame(&b, 80, 24, 0);
    assert!(f.contains("yes, send"), "{f}");
    assert!(!f.contains("↵"), "no Enter hint where Enter does not confirm:\n{f}");
    tap(&mut b, "yes, send");
    assert_eq!(
        b.action,
        Some(Action {
            verb: "send",
            text: "rm -rf target".into()
        })
    );
    // the button is y: a tap never goes through the Enter that a multi-line send refuses
    let mut c = tablet_steps();
    ch(&mut c, 's');
    let (_, _, hits) = drawn(&c, 80, 24, &Theme::fixed(), 0);
    let yes = Tap::Key(KeyEvent::from(KeyCode::Char('y')));
    assert!(hits.iter().any(|h| h.tap == yes));
    assert!(!hits.iter().any(|h| h.tap == Tap::Key(KeyEvent::from(KeyCode::Enter))));
    tap(&mut a, "[ insert ]");
    assert!(a.action.is_some());
}

#[test]
fn a_tap_on_a_step_selects_it_and_copy_takes_its_command() {
    let mut a = tablet_steps();
    tap(&mut a, "cargo build --release");
    assert!(matches!(a.view, View::Steps { sel: 1, .. }), "a tap on any line of the step");
    assert!(a.action.is_none(), "a step tap only selects");
    tap(&mut a, "[ copy ]");
    assert_eq!(a.copy.as_deref(), Some("cargo build --release"));
    tap(&mut a, "Tap Allow in the app");
    tap(&mut a, "[ copy ]");
    assert_eq!(
        a.copy.as_deref(),
        Some("Tap Allow in the app"),
        "a step with no command copies its note"
    );
    tap(&mut a, "[ retry ]");
    assert_eq!(a.view, View::Busy);
}

#[test]
fn answer_buttons_follow_up_copy_and_discard_after_asking() {
    let mut a = tablet(explained());
    tap(&mut a, "[ copy ]");
    assert_eq!(
        a.copy.as_deref(),
        Some("Add the fallback for empty lines and keep the parser as it is.")
    );
    tap(&mut a, "[ home ]");
    assert!(matches!(a.view, View::Answer { armed: true, .. }));
    tap(&mut a, "[ keep it ]");
    assert!(matches!(a.view, View::Answer { armed: false, .. }) && a.run.is_some());
    tap(&mut a, "[ home ]");
    tap(&mut a, "discard it  ");
    assert!(a.view == View::Home { sel: 0 } && a.run.is_none());
    let mut a = tablet(explained());
    tap(&mut a, "[ follow up ]");
    assert!(matches!(
        a.view,
        View::Input {
            kind: InputKind::FollowUp,
            ..
        }
    ));
    let mut a = tablet(long_answer(3));
    tap(&mut a, "[ copy ]");
    assert_eq!(a.copy.as_deref(), Some("line 1\nline 2\nline 3"), "no prompt: the answer");
}

#[test]
fn error_and_busy_buttons() {
    let mut a = tabbed("w", "bash", "$ ");
    writing(&mut a, "x");
    tap(&mut a, "[ cancel ]");
    assert_eq!(a.view, View::Compose);
    tap(&mut a, "[ write ]");
    a.reply(Err(cockpit_tui::panel::request::Fail::Failed("boom".into())));
    tap(&mut a, "[ retry ]");
    assert_eq!(a.view, View::Busy);
    a.reply(Err(cockpit_tui::panel::request::Fail::Failed("boom".into())));
    tap(&mut a, "[ back ]");
    assert_eq!(a.view, View::Compose);
}

#[test]
fn under_the_floor_a_tap_does_nothing_and_esc_still_goes_back() {
    let mut a = tabbed("w", "zsh", "$ ");
    ch(&mut a, '2');
    let (_, _, hits) = drawn(&a, 20, 5, &Theme::fixed(), 0);
    assert!(hits.is_empty(), "the card records no hits");
    a.shrunk(true);
    a.tap(Tap::Key(KeyEvent::from(KeyCode::Esc)), LATER);
    assert_eq!(a.view, View::Compose, "nor does App take a tap there");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Home { sel: 0 }, "the esc key is the card's way back");
}

/// The re-verifier's D-new-1: under the floor, taps 500 and 800 ms apart on an Answer.
#[test]
fn under_the_floor_taps_never_arm_or_discard_an_answer() {
    let mut a = tablet(explained());
    a.frame(0);
    let (_, _, hits) = drawn(&a, 20, 5, &Theme::fixed(), 0);
    a.shrunk(true);
    a.frame(16);
    let esc = Tap::Key(KeyEvent::from(KeyCode::Esc));
    for now in [500, 800, 1_300, 5_000] {
        at_with(
            &mut a,
            3,
            2,
            MouseEventKind::Down(MouseButton::Left),
            &hits,
            KeyModifiers::NONE,
            now,
        );
        a.tap(esc, now);
        assert!(
            a.run.is_some() && matches!(a.view, View::Answer { armed: false, .. }),
            "a tap at {now} ms"
        );
    }
}

/// D-new-2: the second tap of a slow double tap on [ insert ] lands on the
/// resumed Steps, whose first view guards for longer.
#[test]
fn a_resumed_panel_guards_its_first_view_for_longer() {
    use cockpit_tui::panel::app::{RESUME_GUARD_MS, TAP_GUARD_MS};
    let mut a = tablet(stepped());
    ch(&mut a, 'i');
    let mut b = tablet(app("w", "bash", "$ "));
    b.resumed("bash".into(), resume_of(&a), "ok");
    b.frame(1_000);
    let insert = hits_of(&b, 40, 20).into_iter().find(|h| h.tap == k('i')).unwrap();
    for late in [TAP_GUARD_MS, 500, RESUME_GUARD_MS - 1] {
        b.tap(insert.tap, 1_000 + late);
        assert!(b.action.is_none(), "{late} ms after the resumed first frame");
    }
    b.tap(insert.tap, 1_000 + RESUME_GUARD_MS);
    assert!(b.action.is_some(), "past the longer guard a tap acts");

    // the next view of a resumed panel has the usual guard, and so has a fresh panel
    let mut b = tablet(app("w", "bash", "$ "));
    b.resumed("bash".into(), resume_of(&a), "ok");
    b.frame(0);
    b.tap(k('s'), RESUME_GUARD_MS);
    assert!(matches!(b.view, View::Confirm { .. }));
    b.frame(RESUME_GUARD_MS + 10);
    b.tap(k('n'), RESUME_GUARD_MS + 10 + TAP_GUARD_MS);
    assert!(matches!(b.view, View::Steps { .. }));
    let mut c = tablet_steps();
    c.frame(0);
    c.tap(k('s'), TAP_GUARD_MS);
    assert!(matches!(c.view, View::Confirm { .. }));
}

/// Only the resumed panel's first view guards for longer: its second view, a
/// Confirm, drops a tap under the usual 300 ms like any other new view.
#[test]
fn a_resumed_panels_second_view_drops_a_tap_under_300_ms() {
    use cockpit_tui::panel::app::{RESUME_GUARD_MS, TAP_GUARD_MS};
    let mut a = tablet(stepped());
    ch(&mut a, 'i');
    let mut b = tablet(app("w", "bash", "$ "));
    b.resumed("bash".into(), resume_of(&a), "ok");
    b.frame(0);
    b.tap(k('s'), RESUME_GUARD_MS);
    assert!(matches!(b.view, View::Confirm { .. }));
    let second = RESUME_GUARD_MS + 10;
    b.frame(second);
    b.tap(k('y'), second + TAP_GUARD_MS - 1);
    assert!(
        matches!(b.view, View::Confirm { .. }) && b.action.is_none(),
        "a tap under 300 ms is dropped"
    );
    b.tap(k('y'), second + TAP_GUARD_MS);
    assert!(b.action.is_some(), "at 300 ms it acts");
}

#[test]
fn the_wheel_scrolls_and_never_touches_a_box() {
    let mut a = tablet(long_answer(60));
    let _ = shown(&mut a, 80, 24);
    let bottom = scroll_of(&a);
    for _ in 0..3 {
        at(&mut a, 10, 10, MouseEventKind::ScrollUp, &[]);
    }
    assert_eq!(scroll_of(&a), bottom - 3);
    at(&mut a, 10, 10, MouseEventKind::ScrollDown, &[]);
    assert_eq!(scroll_of(&a), bottom - 2);
    let mut a = tabbed("w", "zsh", "$ ");
    at(&mut a, 10, 10, MouseEventKind::ScrollDown, &[]);
    assert_eq!(a.view, View::Home { sel: 1 }, "on a list the wheel moves the selection");
    tap(&mut a, "Compose");
    ctrl(&mut a, 't');
    assert_eq!(note(&a), Some("nothing to insert yet"));
    at(&mut a, 10, 10, MouseEventKind::ScrollUp, &[]);
    assert_eq!((a.view.clone(), note(&a)), (View::Compose, Some("nothing to insert yet")));
}

#[test]
fn tall_tiles_only_where_the_whole_view_fits() {
    let tall = |a: &App, w, h| frame(a, w, h, 0).contains("╭───");
    let a = tabbed("claude", "claude", CLAUDE_SCREEN);
    // 5 tiles of 3 rows, a blank, 2 capture lines and a row of buttons: 19 rows inside the border
    assert!(tall(&a, 80, 21) && !tall(&a, 80, 20));
    assert!(!tall(&a, 40, 20), "a phone in portrait keeps one row a tile");
    let a = tabbed("w", "htop", "");
    assert!(tall(&a, 40, 20), "four tiles fit there");
    assert!(!tall(&app("claude", "claude", CLAUDE_SCREEN), 80, 40), "pc mode never");
}

#[test]
fn every_tablet_view_fits_its_buttons_at_the_floor() {
    use cockpit_tui::panel::view::{TABLET_MIN_H, TABLET_MIN_W};
    for (name, a, ..) in every_view() {
        let a = tablet(a);
        let (f, _, hits) = drawn(&a, TABLET_MIN_W, TABLET_MIN_H, &Theme::fixed(), 0);
        assert!(!hits.is_empty(), "{name}: something to tap:\n{f}");
        for h in &hits {
            assert!(h.at.y < TABLET_MIN_H - 1 && h.at.y > 0, "{name}: a hit on the border {h:?}:\n{f}");
        }
    }
}

// ---------------------------------------------------------------- touch safety

fn hits_of(a: &App, w: u16, h: u16) -> Vec<Hit> {
    drawn(a, w, h, &Theme::fixed(), 0).2
}

fn k(c: char) -> Tap {
    Tap::Key(KeyEvent::from(KeyCode::Char(c)))
}

/// A button that opens a Confirm or arms a discard, and the tap that then
/// confirms: at no size from 20x4 to 120x40 may a second tap on the opener's
/// cells be that tap. One app for the whole grid, back to the opener with n.
fn never_under(name: &str, mut a: App, open: Tap, yes: Tap) {
    let mut checked = 0;
    use cockpit_tui::panel::view::{TABLET_MIN_H, TABLET_MIN_W};
    // from under the floor, where nothing may be tapped at all
    for w in 20..=120u16 {
        for h in 4..=40u16 {
            if w < TABLET_MIN_W || h < TABLET_MIN_H {
                assert!(hits_of(&a, w, h).is_empty(), "{name} at {w}x{h}: a hit under the floor");
                continue;
            }
            let Some(o) = hits_of(&a, w, h).into_iter().find(|x| x.tap == open) else {
                continue;
            };
            a.tap(open, LATER);
            assert!(
                matches!(a.view, View::Confirm { .. } | View::Answer { armed: true, .. }),
                "{name} at {w}x{h} opened {:?}",
                a.view
            );
            for c in hits_of(&a, w, h).into_iter().filter(|x| x.tap == yes) {
                assert!(
                    o.at.intersection(c.at).is_empty(),
                    "{name} at {w}x{h}: the opener {:?} lies under the confirm {:?}:\n{}",
                    o.at,
                    c.at,
                    frame(&a, w, h, 0)
                );
            }
            checked += 1;
            ch(&mut a, 'n');
            assert!(
                !matches!(a.view, View::Confirm { .. } | View::Answer { armed: true, .. }),
                "{name}: n goes back"
            );
        }
    }
    assert!(checked > 2_000, "{name}: only {checked} sizes had the opener on screen");
}

fn multi_result() -> App {
    let mut a = tabbed("py", "python3", ">>> ");
    writing(&mut a, "x");
    a.reply(Ok("<text>rm -rf target\nls</text>".into()));
    a
}

#[test]
fn no_double_tap_on_steps_send_confirms() {
    never_under("steps send", tablet_steps(), k('s'), k('y'));
}

#[test]
fn no_double_tap_on_result_send_or_insert_confirms() {
    never_under("result send", multi_result(), k('s'), k('y'));
    never_under("result insert", multi_result(), k('i'), k('y'));
}

#[test]
fn no_double_tap_on_answer_send_or_insert_confirms() {
    never_under("answer send", tablet(explained()), k('s'), k('y'));
    never_under("answer insert", tablet(explained()), k('i'), k('y'));
}

#[test]
fn no_double_tap_on_answer_home_discards() {
    let esc = Tap::Key(KeyEvent::from(KeyCode::Esc));
    never_under("answer home", tablet(explained()), esc, esc);
}

#[test]
fn no_double_tap_on_insert_as_typed_confirms() {
    let mut a = tabbed("py", "python3", ">>> ");
    tap(&mut a, "Compose");
    typ(&mut a, "one\ntwo");
    let ctrl_t = Tap::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    never_under("compose insert as typed", a, ctrl_t, k('y'));
}

#[test]
fn a_tap_within_the_guard_of_a_new_view_is_dropped_and_keys_never_are() {
    use cockpit_tui::panel::app::TAP_GUARD_MS;
    let mut a = tablet_steps();
    a.frame(1_000);
    let send = hits_of(&a, 40, 20).into_iter().find(|h| h.tap == k('s')).unwrap();
    a.tap(send.tap, 1_000 + TAP_GUARD_MS - 1);
    assert!(matches!(a.view, View::Steps { .. }), "the first frame starts the guard too");
    a.tap(send.tap, 1_000 + TAP_GUARD_MS);
    assert!(matches!(a.view, View::Confirm { .. }));
    a.frame(2_000);
    a.tap(k('y'), 2_120);
    assert!(
        a.action.is_none() && matches!(a.view, View::Confirm { .. }),
        "120 ms after the confirm opened"
    );
    ch(&mut a, 'y');
    assert!(a.action.is_some(), "a key is never guarded");

    // the selection moving is no new view: the guard does not restart
    let mut a = tablet_steps();
    a.frame(0);
    a.tap(Tap::Select(1), 500);
    a.frame(510);
    a.tap(Tap::Select(2), 520);
    assert!(matches!(a.view, View::Steps { sel: 2, .. }));
}

/// The verifier's D1 and D3: two presses 120 ms apart on one cell, at 40x20.
#[test]
fn a_double_tap_never_confirms_a_send_or_discards_an_answer() {
    let press = |a: &mut App, x: u16, y: u16, now: u64| {
        let hits = hits_of(a, 40, 20);
        at_with(a, x, y, MouseEventKind::Down(MouseButton::Left), &hits, KeyModifiers::NONE, now);
        a.frame(now + 16);
    };
    let mut a = tablet_steps();
    a.frame(0);
    let s = hits_of(&a, 40, 20).into_iter().find(|h| h.tap == k('s')).unwrap().at;
    for (i, now) in [1_000, 1_120, 1_500, 2_000].into_iter().enumerate() {
        press(&mut a, s.x, s.y, now);
        assert!(a.action.is_none(), "press {i} at {now} ms on [ send ]");
    }
    assert!(matches!(a.view, View::Confirm { .. } | View::Steps { .. }));

    let mut a = tablet(explained());
    a.frame(0);
    let home = hits_of(&a, 40, 20)
        .into_iter()
        .find(|h| h.tap == Tap::Key(KeyEvent::from(KeyCode::Esc)))
        .unwrap()
        .at;
    for now in [1_000, 1_120, 1_500, 2_000] {
        press(&mut a, home.x, home.y, now);
        assert!(a.run.is_some(), "the answer survives a press at {now} ms on [ home ]");
    }
}

#[test]
fn a_click_with_shift_ctrl_or_alt_is_no_tap() {
    for m in [KeyModifiers::SHIFT, KeyModifiers::CONTROL, KeyModifiers::ALT] {
        let mut a = tabbed("w", "zsh", "$ ");
        let hits = hits_of(&a, 80, 24);
        let close = hits.iter().find(|h| h.tap == Tap::Pick(4)).unwrap().at;
        at_with(&mut a, close.x, close.y, MouseEventKind::Down(MouseButton::Left), &hits, m, LATER);
        assert!(!a.quit && a.view == View::Home { sel: 0 }, "{m:?}");
    }
}

#[test]
fn a_copy_is_cleaned_like_a_paste_and_capped() {
    use cockpit_tui::panel::app::{COPY_MAX, COPY_TERMUX};
    let res = |t: &str| {
        let mut a = tabbed("w", "zsh", "$ ");
        writing(&mut a, "x");
        a.reply(Ok("<text>x</text>".into()));
        if let View::Result { text, .. } = &mut a.view {
            *text = t.into();
        }
        a
    };
    let mut a = res("a\tb\nc\x1b[201~d\x07e\u{9b}f");
    ch(&mut a, 'x');
    assert_eq!(
        a.copy.as_deref(),
        Some("a\tb\nc[201~def"),
        "tabs and newlines stay, control characters go"
    );
    assert_eq!(note(&a), Some("sent 13 chars to the clipboard (needs tmux set-clipboard on)"));

    let mut a = res(&"y".repeat(COPY_MAX + 1));
    ch(&mut a, 'x');
    assert_eq!((a.copy.as_deref(), level(&a)), (None, Some(Level::Warn)));
    assert_eq!(note(&a), Some(format!("too long to copy: {} chars", COPY_MAX + 1).as_str()));

    let mut a = res(&"y".repeat(COPY_TERMUX + 1));
    a.termux = true;
    ch(&mut a, 'x');
    assert!(a.copy.is_some() && level(&a) == Some(Level::Warn), "{:?}", note(&a));
    assert!(note(&a).unwrap().ends_with("Termux may cut it near 8 KB"));
    a.termux = false;
    ch(&mut a, 'x');
    assert_eq!(level(&a), Some(Level::Info), "only Termux is warned at that size");
}

#[test]
fn the_armed_note_says_tap_in_tablet_mode() {
    let mut a = tablet(explained());
    key(&mut a, KeyCode::Esc);
    assert_eq!(note(&a), Some("tap discard it, or esc again, to drop this conversation"));
    let f = frame(&a, 80, 24, 0);
    assert!(f.contains("discard it") && f.contains("[ keep it ]"), "{f}");
}

// ---------------------------------------------------------------- glyph tiers

fn composing(draft: &str) -> App {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '2');
    assert_eq!(a.view, View::Compose);
    typ(&mut a, draft);
    a
}

fn ascii_theme() -> Theme {
    let mut th = Theme::fixed();
    th.g = cockpit_tui::theme::Glyphs::ascii();
    th
}

/// Every view, the `?` overlay over each, and the views every_view leaves out.
fn more_views() -> Vec<(String, App)> {
    let mut v: Vec<(String, App)> = every_view().into_iter().map(|(n, a, ..)| (n.to_string(), a)).collect();
    for (name, mut a, ..) in every_view() {
        a.help = Some(0);
        v.push((format!("{name} ?"), a));
    }
    v.push(("compose-empty".into(), composing("")));
    let mut a = long_question();
    key(&mut a, KeyCode::PageUp);
    v.push(("question-scrolled".into(), a));
    let mut a = stepped();
    ch(&mut a, 's');
    v.push(("confirm-steps".into(), a));
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok(
        "<steps><step><note>wipe</note><cmd>sudo rm -rf /tmp/x\necho done</cmd></step></steps>".into(),
    ));
    v.push(("steps-lines".into(), a));
    let mut a = app("py", "python3", ">>> ");
    writing(&mut a, "x");
    a.reply(Ok("<text>one\ntwo</text>".into()));
    v.push(("result-lines".into(), a));
    let mut a = app("py", "python3", ">>> ");
    writing(&mut a, "x");
    a.reply(Ok("<text>one\ntwo</text>".into()));
    ch(&mut a, 'i');
    v.push(("confirm-lines".into(), a));
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<question>Which file?</question>".into()));
    v.push(("question-no-options".into(), a));
    for lv in [Level::Info, Level::Warn, Level::Error] {
        let mut a = composing("");
        a.say(lv, "hi");
        v.push((format!("note {lv:?}"), a));
    }
    v
}

/// [`more_views`] in pc mode and in tablet mode.
fn every_view_and_more() -> Vec<(String, App)> {
    let mut v = more_views();
    v.extend(more_views().into_iter().map(|(n, a)| (format!("{n} tablet"), tablet(a))));
    v
}

#[test]
fn the_ascii_tier_prints_nothing_but_ascii() {
    let th = ascii_theme();
    for (name, a) in every_view_and_more() {
        for (w, h) in [(80, 24), (40, 12)] {
            let (f, ..) = drawn(&a, w, h, &th, 37);
            let bad: String = f.chars().filter(|c| !c.is_ascii()).collect();
            assert!(bad.is_empty(), "{name} at {w}x{h}: {bad:?}\n{f}");
        }
    }
}

#[test]
fn the_unicode_tier_has_no_nerd_or_private_use_glyph() {
    let pua = |c: char| matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0x10FFFF);
    for (name, a) in every_view_and_more() {
        for (w, h) in [(80, 24), (40, 12)] {
            let f = frame(&a, w, h, 37);
            assert!(!f.chars().any(pua), "{name} at {w}x{h}:\n{f}");
        }
    }
}

#[test]
fn golden_ascii() {
    let th = ascii_theme();
    for (name, a, ..) in every_view() {
        if ["home", "compose", "question", "steps", "answer", "confirm"].contains(&name) {
            golden(&format!("panel-ascii-{name}-80"), &framed(&a, 80, 20, &th, 0).0);
            golden(&format!("panel-ascii-{name}-40"), &framed(&a, 40, 12, &th, 0).0);
        }
    }
}

// ---------------------------------------------------------------- editing

/// The draft with `|` where the cursor is.
fn at_cur(a: &App) -> String {
    format!("{}|{}", &a.draft[..a.cur], &a.draft[a.cur..])
}

fn press(a: &mut App, code: KeyCode, m: KeyModifiers) {
    a.key(KeyEvent::new(code, m));
}

#[test]
fn the_cursor_moves_by_char_word_and_line() {
    let c = KeyModifiers::CONTROL;
    let mut a = composing("one two  three");
    assert_eq!(at_cur(&a), "one two  three|");
    key(&mut a, KeyCode::Left);
    assert_eq!(at_cur(&a), "one two  thre|e");
    press(&mut a, KeyCode::Left, c);
    assert_eq!(at_cur(&a), "one two  |three");
    press(&mut a, KeyCode::Left, c);
    assert_eq!(at_cur(&a), "one |two  three");
    press(&mut a, KeyCode::Right, c);
    assert_eq!(at_cur(&a), "one two|  three");
    key(&mut a, KeyCode::Right);
    assert_eq!(at_cur(&a), "one two | three");
    key(&mut a, KeyCode::Home);
    assert_eq!(at_cur(&a), "|one two  three");
    key(&mut a, KeyCode::Left);
    assert_eq!(at_cur(&a), "|one two  three", "Left at the start stays");
    key(&mut a, KeyCode::End);
    assert_eq!(at_cur(&a), "one two  three|");
    key(&mut a, KeyCode::Right);
    assert_eq!(at_cur(&a), "one two  three|", "Right at the end stays");
    ctrl(&mut a, 'a');
    assert_eq!(a.cur, 0);
    ctrl(&mut a, 'e');
    assert_eq!(a.cur, a.draft.len());

    // Home and End are the line's, ^A and ^E too
    let mut a = composing("ab\ncd\nef");
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left);
    assert_eq!(at_cur(&a), "ab\ncd|\nef");
    key(&mut a, KeyCode::Home);
    assert_eq!(at_cur(&a), "ab\n|cd\nef");
    ctrl(&mut a, 'e');
    assert_eq!(at_cur(&a), "ab\ncd|\nef");
    ctrl(&mut a, 'a');
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::End);
    assert_eq!(at_cur(&a), "ab|\ncd\nef");
}

#[test]
fn typing_pasting_and_deleting_happen_at_the_cursor() {
    let c = KeyModifiers::CONTROL;
    let mut a = composing("one three");
    press(&mut a, KeyCode::Left, c);
    typ(&mut a, "two ");
    assert_eq!(at_cur(&a), "one two |three");
    a.paste("and\na half ");
    assert_eq!(at_cur(&a), "one two and\na half |three");
    ctrl(&mut a, 'w');
    assert_eq!(at_cur(&a), "one two and\na |three");
    key(&mut a, KeyCode::Backspace);
    assert_eq!(at_cur(&a), "one two and\na|three");
    key(&mut a, KeyCode::Delete);
    assert_eq!(at_cur(&a), "one two and\na|hree");
    key(&mut a, KeyCode::End);
    key(&mut a, KeyCode::Delete);
    assert_eq!(at_cur(&a), "one two and\nahree|", "Delete at the end does nothing");
    key(&mut a, KeyCode::Home);
    key(&mut a, KeyCode::Backspace);
    assert_eq!(at_cur(&a), "one two and|ahree", "Backspace at a line start joins the lines");
    ctrl(&mut a, 'u');
    assert_eq!(at_cur(&a), "|ahree", "^U takes everything before the cursor");
    key(&mut a, KeyCode::Enter);
    assert_eq!(at_cur(&a), "\n|ahree");

    // the cap counts the whole draft: a typed char mid-text is turned away too
    let mut a = composing("ab");
    typ(&mut a, &"x".repeat(7998));
    key(&mut a, KeyCode::Home);
    ch(&mut a, 'y');
    assert_eq!((a.draft.chars().count(), a.cur), (8000, 0));
    assert!(note(&a).is_some_and(|n| n.contains("1 left out")), "{:?}", a.note);
}

#[test]
fn the_cursor_steps_over_whole_utf8_chars_and_marks() {
    let c = KeyModifiers::CONTROL;
    // a combining acute, VS16 and an ideographic space: none may be split from its base
    let text = "e\u{301}x ❤\u{fe0f}日本\u{3000}語";
    let mut a = composing(text);
    let mut stops = vec![a.cur];
    for _ in 0..12 {
        key(&mut a, KeyCode::Left);
        stops.push(a.cur);
    }
    stops.dedup();
    let want: Vec<usize> = [
        "e\u{301}x ❤\u{fe0f}日本\u{3000}語",
        "e\u{301}x ❤\u{fe0f}日本\u{3000}",
        "e\u{301}x ❤\u{fe0f}日本",
        "e\u{301}x ❤\u{fe0f}日",
        "e\u{301}x ❤\u{fe0f}",
        "e\u{301}x ",
        "e\u{301}x",
        "e\u{301}",
        "",
    ]
    .iter()
    .map(|s| s.len())
    .collect();
    assert_eq!(stops, want);
    let mut back = vec![a.cur];
    for _ in 0..12 {
        key(&mut a, KeyCode::Right);
        back.push(a.cur);
    }
    back.dedup();
    back.reverse();
    assert_eq!(back, want, "Right stops where Left does");
    // every key from every stop: always on a char boundary, never a panic
    let keys = [
        (KeyCode::Backspace, KeyModifiers::NONE),
        (KeyCode::Delete, KeyModifiers::NONE),
        (KeyCode::Char('w'), c),
        (KeyCode::Char('u'), c),
        (KeyCode::Left, c),
        (KeyCode::Right, c),
        (KeyCode::Char('é'), KeyModifiers::NONE),
    ];
    for &stop in &want {
        for (code, m) in keys {
            let mut a = composing(text);
            a.cur = stop;
            press(&mut a, code, m);
            assert!(a.draft.is_char_boundary(a.cur) && a.cur <= a.draft.len(), "{code:?} at {stop}");
        }
    }
    key(&mut a, KeyCode::End);
    key(&mut a, KeyCode::Backspace);
    key(&mut a, KeyCode::Backspace);
    assert_eq!(a.draft, "e\u{301}x ❤\u{fe0f}日本");
    press(&mut a, KeyCode::Left, c);
    assert_eq!(at_cur(&a), "e\u{301}x |❤\u{fe0f}日本");
    ctrl(&mut a, 'w');
    assert_eq!(at_cur(&a), "|❤\u{fe0f}日本");
    key(&mut a, KeyCode::Delete);
    assert_eq!(at_cur(&a), "|日本", "Delete takes the heart and its VS16");
    // a stale offset off a char boundary is pulled back onto one
    a.cur = 1;
    ch(&mut a, 'a');
    assert_eq!(at_cur(&a), "a|日本");
}

#[test]
fn the_one_line_box_edits_at_its_cursor() {
    let c = KeyModifiers::CONTROL;
    let mut a = asking();
    ch(&mut a, '5');
    typ(&mut a, "why is it red");
    press(&mut a, KeyCode::Left, c);
    typ(&mut a, "so ");
    ctrl(&mut a, 'a');
    key(&mut a, KeyCode::Delete);
    a.paste("W\nh");
    key(&mut a, KeyCode::End);
    key(&mut a, KeyCode::Backspace);
    let View::Input { text, cur, .. } = &a.view else {
        panic!("{:?}", a.view)
    };
    assert_eq!((text.as_str(), *cur), ("W hhy is it so re", 17));
    ctrl(&mut a, 'a');
    ctrl(&mut a, 'u');
    let View::Input { text, .. } = &a.view else {
        panic!("{:?}", a.view)
    };
    assert_eq!(text, "W hhy is it so re", "^U at the start takes nothing");
    key(&mut a, KeyCode::Right);
    let f = frame(&a, 60, 12, 0);
    assert!(f.contains("W▏ hhy is it so re"), "the cursor is drawn where it is:\n{f}");
}

#[test]
fn a_wide_char_puts_the_cursor_by_its_width() {
    let mut a = composing("日本語 ok");
    for _ in 0..4 {
        key(&mut a, KeyCode::Left);
    }
    assert_eq!(at_cur(&a), "日本|語 ok");
    let f = frame(&a, 60, 12, 0);
    let line = f.lines().find(|l| l.contains('▏')).unwrap_or_else(|| panic!("no cursor:\n{f}"));
    // the backend writes a wide char's second cell as nothing: count columns by width
    use unicode_width::UnicodeWidthStr;
    let col = line[..line.find('▏').unwrap()].width();
    assert_eq!(col, "\"│ 日本".width(), "{line}");
}

#[test]
fn the_cursor_line_stays_on_screen() {
    let lines: Vec<String> = (1..=30).map(|i| format!("line{i}")).collect();
    let mut a = composing(&lines.join("\n"));
    let f = shown(&mut a, 40, 12);
    assert!(f.contains("line30▏") && f.contains("model"), "the end and the route:\n{f}");
    while a.cur > 0 {
        key(&mut a, KeyCode::Left);
    }
    let f = shown(&mut a, 40, 12);
    assert!(f.contains("▏line1") && !f.contains("line30"), "the cursor's line is shown:\n{f}");
}

#[test]
fn an_empty_compose_shows_a_placeholder_that_is_never_typed() {
    let mut a = composing("");
    let f = frame(&a, 60, 12, 0);
    assert!(f.contains("▏type what you want…"), "{f}");
    ctrl(&mut a, 't');
    assert_eq!((a.draft.as_str(), note(&a)), ("", Some("nothing to insert yet")));
    ctrl(&mut a, 's');
    assert!(a.action.is_none() && a.view == View::Compose);
    ch(&mut a, 'x');
    assert!(!frame(&a, 60, 12, 0).contains("type what you want"));
    key(&mut a, KeyCode::Backspace);
    assert!(frame(&a, 60, 12, 0).contains("type what you want"), "back when the draft empties");
    assert_eq!(a.draft, "");
}

#[test]
fn a_long_draft_counts_against_the_cap() {
    let mut a = composing("");
    a.paste(&"x".repeat(6000));
    assert!(!frame(&a, 80, 20, 0).contains("/8000"), "6000 is not long yet");
    ch(&mut a, 'y');
    let f = frame(&a, 80, 20, 0);
    assert!(f.contains("model: auto   6001/8000"), "{f}");
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Tab);
    let f = frame(&a, 40, 12, 0);
    let own = f.lines().any(|l| l.contains("6001/8000") && !l.contains("type:"));
    assert!(own && f.contains("type: command"), "on its own line when narrow:\n{f}");
}

#[test]
fn golden_compose_editing() {
    let mut a = composing("");
    goldens("compose-empty", &a, 0);
    typ(&mut a, "fix the flaky api test then rerun it");
    let c = KeyModifiers::CONTROL;
    for _ in 0..3 {
        press(&mut a, KeyCode::Left, c);
    }
    goldens("compose-cursor", &a, 0);
    let mut a = composing("");
    a.paste(&"word ".repeat(1300));
    goldens("compose-count", &a, 0);
}

// ---------------------------------------------------------------- item 3 leftovers

#[test]
fn a_question_with_no_options_shows_no_digit_pill() {
    let mut a = app("work", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<question>Which file?</question>".into()));
    let f = frame(&a, 80, 20, 0);
    assert!(f.contains("o other") && !f.contains(" 1 pick"), "{f}");
    let ks = cockpit_tui::panel::view::keys(&a, &Theme::fixed().g);
    assert!(!ks.iter().any(|(k, v)| k.is_empty() || v.contains("by number")), "{ks:?}");
    key(&mut a, KeyCode::Enter);
    assert!(matches!(a.view, View::Input { .. }), "Enter with no option types one");
}

#[test]
fn enter_after_pgup_shows_the_selection_before_it_answers() {
    let mut a = long_question();
    let _ = shown(&mut a, 30, 7);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::PageUp);
    let f = shown(&mut a, 30, 7);
    assert!(f.contains("START") && !f.contains("second"), "scrolled off the selection:\n{f}");
    key(&mut a, KeyCode::Enter);
    assert!(
        matches!(a.view, View::Question { sel: 1, scroll: None }),
        "the first Enter answers nothing"
    );
    let f = shown(&mut a, 30, 7);
    assert!(f.contains("▎2   second"), "it scrolled back to the selection:\n{f}");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, View::Busy, "the second answers it");
    assert_eq!(a.run.as_ref().unwrap().turns.last().unwrap().text, "second");
}

#[test]
fn a_pinned_question_scroll_is_dropped_when_it_all_fits() {
    let mut a = long_question();
    let _ = shown(&mut a, 30, 7);
    key(&mut a, KeyCode::PageUp);
    let _ = shown(&mut a, 30, 7);
    assert!(matches!(a.view, View::Question { scroll: Some(0), .. }));
    let _ = shown(&mut a, 120, 40);
    assert!(matches!(a.view, View::Question { scroll: None, .. }), "{:?}", a.view);
    let f = shown(&mut a, 30, 7);
    assert!(f.contains("▎1   first"), "back on a small screen, it follows the selection:\n{f}");
}

// ---------------------------------------------------------------- verifier round (item 5)

#[test]
fn the_cursor_and_the_deletes_move_by_grapheme_cluster() {
    let c = KeyModifiers::CONTROL;
    let family = "👨\u{200d}👩\u{200d}👧";
    let (tone, il, us) = ("👍\u{1f3fd}", "🇮🇱", "🇺🇸");
    let mut a = composing(&format!("a{family}{tone}{il}{us}"));
    key(&mut a, KeyCode::Left);
    assert_eq!(at_cur(&a), format!("a{family}{tone}{il}|{us}"), "one flag, not half of one");
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left);
    assert_eq!(at_cur(&a), format!("a{family}|{tone}{il}{us}"), "the skin tone stays on its hand");
    key(&mut a, KeyCode::Backspace);
    assert_eq!(at_cur(&a), format!("a|{tone}{il}{us}"), "Backspace takes the whole family");
    key(&mut a, KeyCode::Delete);
    assert_eq!(at_cur(&a), format!("a|{il}{us}"));
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    assert_eq!(at_cur(&a), format!("a{il}{us}|"));
    typ(&mut a, &format!(" {family}{family}"));
    ctrl(&mut a, 'w');
    assert_eq!(at_cur(&a), format!("a{il}{us} |"), "^W takes a word of emoji whole");
    typ(&mut a, family);
    press(&mut a, KeyCode::Left, c);
    assert_eq!(at_cur(&a), format!("a{il}{us} |{family}"));
    // a stale offset inside a cluster is pulled back to its start
    a.cur += 4;
    ch(&mut a, 'x');
    assert_eq!(at_cur(&a), format!("a{il}{us} x|{family}"));
}

#[test]
fn a_letter_typed_before_a_stray_mark_puts_the_cursor_past_the_mark() {
    let mut a = composing("");
    a.paste("\u{301}");
    key(&mut a, KeyCode::Home);
    ch(&mut a, 'a');
    assert_eq!(at_cur(&a), "a\u{301}|", "the mark joined the a: the cursor is after both");
    ch(&mut a, 'b');
    assert_eq!(a.draft, "a\u{301}b");
}

#[test]
fn wrap_and_the_cursor_measure_emoji_as_drawn() {
    use cockpit_tui::panel::view::wrap;
    use unicode_width::UnicodeWidthStr;
    let hearts = "❤\u{fe0f}".repeat(30);
    for l in wrap(&hearts, 36) {
        assert!(l.width() <= 36, "{} columns: {l}", l.width());
    }
    assert_eq!(wrap(&"1\u{fe0f}\u{20e3}".repeat(20), 36).len(), 2, "a keycap is 2 columns");
    let a = composing(&hearts);
    let f = frame(&a, 40, 12, 0);
    assert!(f.contains("❤\u{fe0f}▏"), "the cursor is on screen:\n{f}");
    // the backend notes the cells a wide symbol hides after the row: not drawn
    for l in f.lines().map(|l| l.split(" Hidden by").next().unwrap()) {
        assert_eq!(l.width(), 42, "no row drawn past the box:\n{f}");
    }
}

#[test]
fn at_the_floor_the_cap_note_stays_over_the_cursor_line() {
    use cockpit_tui::panel::view::{MIN_H, MIN_W, TABLET_MIN_H, TABLET_MIN_W};
    let capped = |tab: bool| {
        let mut a = composing("");
        a.tablet = tab;
        a.paste(&"word ".repeat(1600));
        assert!(note(&a).is_none());
        ch(&mut a, 'x');
        assert!(note(&a).is_some_and(|n| n.contains("left out")), "{:?}", a.note);
        a
    };
    for tab in [false, true] {
        let mut a = capped(tab);
        let f = if tab {
            shown(&mut a, TABLET_MIN_W, TABLET_MIN_H)
        } else {
            shown(&mut a, MIN_W, MIN_H)
        };
        assert!(f.contains("the box"), "tablet {tab}: the note shows:\n{f}");
        assert!(f.contains("▏"), "tablet {tab}: and the cursor:\n{f}");
    }
    // a note longer than the rows over the body: it keeps them, cut with an ellipsis
    let mut a = capped(false);
    a.say(
        Level::Warn,
        "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen",
    );
    let f = shown(&mut a, MIN_W, MIN_H);
    assert!(f.contains("▲ one two") && f.contains(" …") && f.contains("▏"), "{f}");
}

#[test]
fn pgdn_back_to_the_bottom_follows_the_selection_again() {
    let mut a = long_question();
    let _ = shown(&mut a, 30, 7);
    key(&mut a, KeyCode::PageUp);
    let _ = shown(&mut a, 30, 7);
    assert!(matches!(a.view, View::Question { scroll: Some(0), .. }));
    for _ in 0..10 {
        key(&mut a, KeyCode::PageDown);
        let _ = shown(&mut a, 30, 7);
    }
    assert!(matches!(a.view, View::Question { scroll: None, .. }), "{:?}", a.view);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.view, View::Busy, "one Enter answers");
}

#[test]
fn the_tier_flag_matches_the_tier() {
    use cockpit_tui::theme::Glyphs;
    assert!(Glyphs::ascii().ascii && !Glyphs::unicode().ascii);
}

/// The cell holding the cursor is accent and reversed; a typed `|` is not.
#[test]
fn the_cursor_has_a_style_of_its_own_in_both_tiers() {
    use ratatui::style::Modifier;
    for th in [Theme::fixed(), ascii_theme()] {
        let mut a = composing("a|b");
        key(&mut a, KeyCode::Left);
        let mut t = Terminal::new(TestBackend::new(40, 12)).unwrap();
        t.draw(|f| {
            render(f, f.area(), &a, &th, 0);
        })
        .unwrap();
        let buf = t.backend().buffer();
        let cells: Vec<_> = (0..40).map(|x| buf[(x, 2)].clone()).collect();
        let text: String = cells.iter().map(|c| c.symbol()).collect();
        let i = text[..text.find("a|").unwrap_or_else(|| panic!("{text:?}"))].chars().count();
        let (pipe, cur) = (&cells[i + 1], &cells[i + 2]);
        assert_eq!(cur.symbol(), th.g.cursor);
        assert!(cur.modifier.contains(Modifier::REVERSED) && cur.fg == th.accent, "{text:?}");
        assert!(!pipe.modifier.contains(Modifier::REVERSED) && pipe.fg != th.accent, "{text:?}");
    }
}

/// Measured like the pc floor: the smallest size where every tablet view shows
/// its heading, 2 body rows, every button uncut and its note. Under it, the card.
#[test]
fn every_tablet_view_keeps_two_body_rows() {
    use cockpit_tui::panel::view::{TABLET_MIN_H, TABLET_MIN_W};
    let keys = |hits: Vec<Hit>| hits.into_iter().filter(|x| matches!(x.tap, Tap::Key(_))).collect::<Vec<_>>();
    let (fw, fh) = (TABLET_MIN_W, TABLET_MIN_H);
    let mut sizes: Vec<(u16, u16)> = (fh..=40).map(|h| (fw, h)).collect();
    sizes.extend((fw..=120).step_by(4).map(|w| (w, fh)));
    for (w, h) in sizes {
        for (name, a, head, body) in every_view() {
            let mut a = tablet(a);
            let tall = keys(hits_of(&a, w, 60));
            let _ = shown(&mut a, w, h);
            let (f, (_, room), hits) = drawn(&a, w, h, &Theme::fixed(), 0);
            let at = format!("{name} at {w}x{h}:\n{f}");
            assert!(f.contains(head) && f.contains(body) && !f.contains("make the window"), "{at}");
            assert!(room >= 2, "{room} body rows, {at}");
            let got = keys(hits);
            let uncut = |t: &Hit| got.iter().any(|g| g.tap == t.tap && g.at.width == t.at.width);
            assert!(got.len() == tall.len() && tall.iter().all(uncut), "the buttons, {at}");
            if let Some(n) = &a.note {
                assert!(f.contains(&n.text.chars().take(10).collect::<String>()), "the note, {at}");
            }
        }
    }
    for (name, a, ..) in every_view() {
        let mut a = tablet(a);
        for (w, h) in [(fw - 1, 40), (200, fh - 1)] {
            let f = shown(&mut a, w, h);
            assert!(
                f.contains("make the window larger") && hits_of(&a, w, h).is_empty(),
                "{name} at {w}x{h}:\n{f}"
            );
        }
        a.tablet = false;
        assert!(!shown(&mut a, fw, fh - 1).contains("make the window"), "{name}: pc keeps 28x7");
    }
}

#[test]
fn the_tablet_tile_refuses_a_window_under_the_tablet_floor() {
    use cockpit_tui::panel::view::{TABLET_MIN_H, TABLET_MIN_W};
    let mode = |a: &App| a.tiles().iter().position(|t| t.0 == Tile::Mode).unwrap();
    let mut a = app("w", "bash", "$ ");
    for size in [(40, TABLET_MIN_H - 1), (TABLET_MIN_W - 1, 40)] {
        a.size = Some(size);
        a.view = View::Home { sel: mode(&a) };
        key(&mut a, KeyCode::Enter);
        assert!(!a.tablet, "{size:?}: no flip into a card");
        assert_eq!(
            (level(&a), note(&a)),
            (Some(Level::Warn), Some("the window is too small for tablet mode (28x12)"))
        );
        // 27 columns is under the pc floor too: only the short window still draws
        if size.0 >= cockpit_tui::panel::view::MIN_W {
            let f = shown(&mut a, size.0, size.1);
            assert!(!f.contains("make the window larger"), "{size:?}: pc keeps drawing");
        }
    }
    a.size = Some((TABLET_MIN_W, TABLET_MIN_H));
    key(&mut a, KeyCode::Enter);
    assert!(a.tablet, "at the floor it flips");
    // back to pc is always allowed, even under the floor
    a.size = Some((30, 8));
    a.view = View::Home { sel: mode(&a) };
    key(&mut a, KeyCode::Enter);
    assert!(!a.tablet && note(&a) == Some("pc mode until the panel closes"));
}

// ---------------------------------------------------------------- #279: Up and Down in Compose

/// Compose holding `draft` with the cursor at the first `|`, `cols` wide.
fn upd(draft: &str, cols: u16) -> App {
    let cur = draft.find('|').unwrap();
    let mut a = composing(&draft.replacen('|', "", 1));
    a.size = Some((cols, 20));
    a.cur = cur;
    a
}

fn up_down(a: &mut App, up: bool) -> String {
    key(a, if up { KeyCode::Up } else { KeyCode::Down });
    at_cur(a)
}

#[test]
fn up_and_down_keep_the_column_in_plain_ascii() {
    let mut a = upd("hello world\nfoo b|ar baz\nlast line", 80);
    assert_eq!(up_down(&mut a, true), "hello| world\nfoo bar baz\nlast line");
    assert_eq!(up_down(&mut a, false), "hello world\nfoo b|ar baz\nlast line");
    assert_eq!(up_down(&mut a, false), "hello world\nfoo bar baz\nlast |line");
}

#[test]
fn up_and_down_land_before_a_wide_char_they_split() {
    let mut a = upd("ab中文de\nxyz|wvu", 80);
    assert_eq!(up_down(&mut a, true), "ab|中文de\nxyzwvu", "column 3 is the right half of 中");
    let mut a = upd("ab中|文de\nxyzwvu", 80);
    assert_eq!(up_down(&mut a, false), "ab中文de\nxyzw|vu", "after 中 is column 4");
}

#[test]
fn up_and_down_take_a_zwj_family_whole() {
    let fam = "👨\u{200d}👩\u{200d}👧";
    let mut a = upd(&format!("a{fam}bc\nxy|z"), 80);
    assert_eq!(up_down(&mut a, true), format!("a|{fam}bc\nxyz"), "never inside the family");
    let mut a = upd(&format!("a{fam}|bc\nxyz"), 80);
    assert_eq!(up_down(&mut a, false), format!("a{fam}bc\nxyz|"), "the family is 2 columns");
}

#[test]
fn up_and_down_clamp_to_a_short_line_end() {
    let mut a = upd("a long line here|\nab\nanother long", 80);
    assert_eq!(up_down(&mut a, false), "a long line here\nab|\nanother long");
    assert_eq!(up_down(&mut a, false), "a long line here\nab\nan|other long");
    let mut a = upd("first\n\nla|st", 80);
    assert_eq!(up_down(&mut a, true), "first\n|\nlast", "an empty line takes the cursor");
}

#[test]
fn up_and_down_stay_put_on_the_first_and_last_line() {
    let mut a = upd("to|p\nbottom", 80);
    assert_eq!(up_down(&mut a, true), "to|p\nbottom");
    let mut a = upd("top\nbot|tom", 80);
    assert_eq!(up_down(&mut a, false), "top\nbot|tom");
    let mut a = upd("on|e line", 80);
    assert_eq!(
        (up_down(&mut a, true), up_down(&mut a, false)),
        ("on|e line".into(), "on|e line".into())
    );
}

#[test]
fn up_and_down_move_by_wrapped_row_as_drawn() {
    // 32 columns draw a 28-column box: the line wraps after "jumps"
    let draft = "the quick brown fox jumps over the lazy dog|";
    let mut a = upd(draft, 32);
    assert_eq!(up_down(&mut a, true), "the quick brown f|ox jumps over the lazy dog");
    let f = frame(&a, 32, 12, 0);
    assert!(f.contains("the quick brown f▏ox jumps"), "the cursor is where Up put it:\n{f}");
    assert_eq!(up_down(&mut a, true), "the quick brown f|ox jumps over the lazy dog", "the top row");
    assert_eq!(up_down(&mut a, false), draft, "back down: column 17 ends \"over the lazy dog\"");
    let mut a = upd(draft, 80);
    assert_eq!(up_down(&mut a, true), draft, "unwrapped, it is one row");
}

// ---------------------------------------------------------------- assumptions (#415)

const ASSUMED: &str = "<prompt>Fix the flaky test in @tests/run.sh.\n\
    Assumed: the test is the timeout one (the screen shows it)\n  \
    Assumed: no other file changes (the request names one)</prompt>";

/// A write whose prompt ends with two Assumed: lines, on the Result view.
fn assumed_result() -> App {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    writing(&mut a, "fix the flaky test");
    a.reply(Ok(ASSUMED.into()));
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
    // launch() clears it in the loop: here, so a test sees whether a call goes out
    a.pending = false;
    a
}

fn last_turn(a: &App) -> String {
    a.run.as_ref().unwrap().turns.last().unwrap().text.clone()
}

#[test]
fn assumed_finds_the_lines_indented_or_not_and_no_mention_mid_line() {
    let t = "Do x.\nNot Assumed: here\nAssumed: a (b)\n   Assumed: c (d)";
    assert_eq!(cockpit_tui::panel::app::assumed(t), ["Assumed: a (b)", "Assumed: c (d)"]);
    assert!(cockpit_tui::panel::app::assumed("").is_empty());
}

#[test]
fn assumed_takes_only_the_closing_run_not_a_line_mid_prompt() {
    let t = "Check the log:\n```\nAssumed: cached (old)\n```\nThen fix it.\nAssumed: a (b)\n\n";
    assert_eq!(cockpit_tui::panel::app::assumed(t), ["Assumed: a (b)"]);
    let mid = "Check the log:\n```\nAssumed: cached (old)\n```\nThen fix it.";
    assert!(cockpit_tui::panel::app::assumed(mid).is_empty(), "no closing run, none");
}

#[test]
fn a_mid_prompt_assumed_line_offers_no_correction() {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    writing(&mut a, "fix it");
    a.reply(Ok("<prompt>Look at:\n```\nAssumed: cached (old)\n```\nFix it.</prompt>".into()));
    let before = a.view.clone();
    ch(&mut a, 'a');
    assert_eq!(a.view, before);
}

#[test]
fn a_correction_goes_in_as_a_turn_and_takes_a_question() {
    let mut a = assumed_result();
    ch(&mut a, 'a');
    assert!(
        matches!(
            a.view,
            View::Input {
                kind: InputKind::Correct(0),
                ..
            }
        ),
        "{:?}",
        a.view
    );
    typ(&mut a, "it is the parser test\n");
    assert_eq!(a.view, View::Busy);
    let (_, prompt, _) = a.request_parts().unwrap();
    let line = "Assumed: the test is the timeout one (the screen shows it)";
    let want = format!("Correction to \"{line}\": it is the parser test");
    assert!(prompt.contains(&format!("<turn role=\"user\">{want}\n")), "{prompt}");
    assert!(prompt.contains("It was:\nFix the flaky test in @tests/run.sh."), "{prompt}");
    assert!(prompt.ends_with("<questions_left>2</questions_left>"), "{prompt}");
}

#[test]
fn tab_picks_the_next_assumption_round_to_the_first() {
    let mut a = assumed_result();
    ch(&mut a, 'a');
    key(&mut a, KeyCode::Tab);
    assert!(matches!(
        a.view,
        View::Input {
            kind: InputKind::Correct(1),
            ..
        }
    ));
    typ(&mut a, "two files\n");
    assert!(last_turn(&a).starts_with("Correction to \"Assumed: no other file changes"));
    let mut a = assumed_result();
    for _ in 0..2 {
        key(&mut a, KeyCode::Tab);
    }
    ch(&mut a, 'a');
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Tab);
    assert!(matches!(
        a.view,
        View::Input {
            kind: InputKind::Correct(0),
            ..
        }
    ));
}

#[test]
fn esc_backs_out_of_a_correction_with_nothing_sent() {
    let mut a = assumed_result();
    let before = a.view.clone();
    ch(&mut a, 'a');
    typ(&mut a, "half typed");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, before);
    assert!(!a.pending && a.run.as_ref().unwrap().turns.is_empty(), "no turn, no call");
}

#[test]
fn an_empty_correction_keeps_the_box() {
    let mut a = assumed_result();
    ch(&mut a, 'a');
    typ(&mut a, "  \n");
    assert!(matches!(
        a.view,
        View::Input {
            kind: InputKind::Correct(0),
            ..
        }
    ));
    assert!(!a.pending);
}

#[test]
fn a_prompt_with_no_assumed_line_offers_no_correction() {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    writing(&mut a, "fix it");
    a.reply(Ok("<prompt>Fix the test. It assumed: nothing</prompt>".into()));
    let f = frame(&a, 120, 24, 0);
    assert!(!f.contains("correct"), "{f}");
    let before = a.view.clone();
    ch(&mut a, 'a');
    assert_eq!(a.view, before, "a does nothing here");
    let keys = cockpit_tui::panel::view::keys(&a, &Theme::fixed().g);
    assert!(!keys.iter().any(|(k, _)| k == "a"), "{keys:?}");
}

#[test]
fn a_fixed_text_with_an_assumed_line_offers_no_correction() {
    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '1');
    typ(&mut a, "Assumed: teh");
    ctrl(&mut a, 'f');
    a.reply(Ok("Assumed: the".into()));
    let View::Result { text, .. } = &a.view else {
        panic!("{:?}", a.view)
    };
    assert_eq!(cockpit_tui::panel::app::assumed(text), ["Assumed: the"], "the line is there");
    ch(&mut a, 'a');
    assert!(matches!(a.view, View::Result { .. }), "a fix is not a write: {:?}", a.view);
}

#[test]
fn the_footer_and_the_help_name_the_correct_key() {
    let a = assumed_result();
    assert!(frame(&a, 120, 24, 0).contains("a correct"));
    let keys = cockpit_tui::panel::view::keys(&a, &Theme::fixed().g);
    assert!(keys.iter().any(|(k, v)| k == "a" && v == "correct an Assumed: line"), "{keys:?}");
}

#[test]
fn a_tablet_result_has_a_correct_button() {
    let mut a = assumed_result();
    a.tablet = true;
    let (f, _, hits) = drawn(&a, 80, 24, &Theme::fixed(), 0);
    assert!(f.contains("[ correct ]"), "{f}");
    let tap = Tap::Key(KeyEvent::from(KeyCode::Char('a')));
    assert!(hits.iter().any(|h| h.tap == tap), "the button taps a");
}

#[test]
fn a_correction_at_zero_questions_is_still_sent_and_stays_at_zero() {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    writing(&mut a, "fix the flaky test");
    a.reply(Ok(q(1)));
    ch(&mut a, 'g');
    a.reply(Ok(ASSUMED.into()));
    assert_eq!(a.run.as_ref().unwrap().rounds, 3);
    ch(&mut a, 'a');
    typ(&mut a, "the parser test\n");
    let (_, prompt, _) = a.request_parts().unwrap();
    assert!(prompt.ends_with("<questions_left>0</questions_left>"), "{prompt}");
    assert_eq!(a.run.as_ref().unwrap().rounds, 3, "never past the budget");
}

#[test]
fn a_cancelled_correction_gives_back_the_turn_and_the_question() {
    let mut a = assumed_result();
    let back = a.view.clone();
    ch(&mut a, 'a');
    typ(&mut a, "the parser test\n");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, back);
    let r = a.run.as_ref().unwrap();
    assert_eq!((r.rounds, r.turns.len()), (0, 0));
}

#[test]
fn a_write_asking_at_zero_questions_is_told_once_to_write_now() {
    let mut a = app("w", "zsh", "$ ");
    writing(&mut a, "do it");
    for n in 1..=3 {
        a.reply(Ok(q(n)));
        ch(&mut a, '1');
    }
    a.pending = false;
    a.reply(Ok(q(4)));
    assert_eq!(a.view, View::Busy, "no error yet: {:?}", a.view);
    assert!(a.pending, "the retry goes out");
    let (_, prompt, _) = a.request_parts().unwrap();
    let now = concat!(
        "(no questions left: write it now without guessing what only I know: have Claude Code ",
        "come back to me with the options for it first, or leave a placeholder for it)"
    );
    let want = format!("<turn role=\"user\">a3\n\n{now}");
    assert!(prompt.contains(&want), "{prompt}");
    a.reply(Ok("<prompt>Do it.</prompt>".into()));
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
}

#[test]
fn a_write_asking_again_after_the_retry_shows_the_error() {
    let mut a = app("w", "zsh", "$ ");
    writing(&mut a, "do it");
    for n in 1..=3 {
        a.reply(Ok(q(n)));
        ch(&mut a, '1');
    }
    a.reply(Ok(q(4)));
    a.reply(Ok(q(5)));
    let line = "no questions left, and Claude still asks: Q5?";
    assert!(matches!(&a.view, View::Error { line: l, .. } if l == line), "{:?}", a.view);
    let View::Error { hint: Some(h), .. } = &a.view else { panic!() };
    assert!(
        !h.contains("draft"),
        "esc goes to the view the call left, not always the draft: {h}"
    );
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Question { sel: 0, scroll: None });
    assert_eq!(
        a.run.as_ref().unwrap().turns.last().unwrap().text,
        "Q3? Options: a3 | b3",
        "the answer is dropped"
    );
}

#[test]
fn a_retry_from_a_result_at_zero_questions_adds_its_own_turn() {
    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    writing(&mut a, "fix the flaky test");
    a.reply(Ok(q(1)));
    ch(&mut a, '1');
    a.reply(Ok(ASSUMED.into()));
    a.run.as_mut().unwrap().rounds = 3;
    ch(&mut a, 'r');
    a.reply(Ok(q(2)));
    assert_eq!(a.view, View::Busy);
    let texts = |a: &App| a.run.as_ref().unwrap().turns.iter().map(|t| t.text.clone()).collect::<Vec<_>>();
    assert_eq!(texts(&a)[1], "a1", "the accepted answer is left alone");
    assert!(texts(&a)[2].starts_with("(no questions left"), "{:?}", texts(&a));
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
    assert_eq!(texts(&a), ["Q1? Options: a1 | b1", "a1"]);
}

#[test]
fn a_cancelled_retry_gives_back_the_answer() {
    let mut a = app("w", "zsh", "$ ");
    writing(&mut a, "do it");
    a.reply(Ok(q(1)));
    ch(&mut a, 'g');
    a.reply(Ok(q(2)));
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Question { sel: 0, scroll: None });
    let r = a.run.as_ref().unwrap();
    assert_eq!((r.rounds, r.turns.len()), (1, 1), "the guess and the retry are undone");
}

#[test]
fn golden_result_assumed() {
    goldens("result-assumed", &assumed_result(), 0);
    tablet_goldens("result-assumed", &tablet(assumed_result()));
}

#[test]
fn golden_input_correct() {
    let mut a = assumed_result();
    ch(&mut a, 'a');
    key(&mut a, KeyCode::Tab);
    typ(&mut a, "two files");
    goldens("input-correct", &a, 0);
}

// ---------------------------------------------------------------- batch questions

/// The second of a batch of two: which of the two it is, and the budget both took.
#[test]
fn golden_question_batch() {
    let mut a = app("work", "bash", "$ ls");
    writing(&mut a, "deploy it");
    a.reply(Ok("<question>Which host?</question>\n<option recommended=\"true\">lab</option>\n\
                <option>prod</option>\n<question>Which port?</question>\n<option>80</option>\n<option>443</option>"
        .into()));
    ch(&mut a, '1');
    golden("panel-question-batch-80", &frame(&a, 80, 20, 0));
    golden("panel-question-batch-40", &frame(&a, 40, 12, 0));
}
