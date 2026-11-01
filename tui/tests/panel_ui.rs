//! The panel's screens and state machine: TestBackend goldens, key sequences,
//! and the action file. COCKPIT_BLESS=1 cargo test rewrites the goldens.
use cockpit_tui::panel::app::{Action, App, InputKind, Tile, View, risky, sanitize, write_action};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::OutType;
use cockpit_tui::panel::request::Call;
use cockpit_tui::panel::view::render;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
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
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| render(f, f.area(), app, &Theme::fixed(), tick)).unwrap();
    t.backend().to_string()
}

/// One golden per width, the two widths the popup meets most.
fn goldens(name: &str, app: &App, tick: u64) {
    golden(&format!("panel-{name}-80"), &frame(app, 80, 20, tick));
    golden(&format!("panel-{name}-120"), &frame(app, 120, 24, tick));
}

fn app(name: &str, cmd: &str, screen: &str) -> App {
    let cap = Capture {
        name: name.into(),
        cmd: cmd.into(),
        path: "/home/u/proj".into(),
        screen: screen.into(),
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

#[test]
fn golden_busy() {
    let mut a = app("work", "bash", "$ ls");
    writing(&mut a, "list big files");
    goldens("busy", &a, 37);
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
fn golden_steps_with_the_risky_flag() {
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
fn home_suggests_by_window_and_numbers_pick_tiles() {
    let mut a = app("w", "zsh", "$ ");
    assert_eq!(a.tiles()[0].1, "Write a command");
    ch(&mut a, '1');
    assert_eq!((a.view.clone(), a.out), (View::Compose, OutType::Command));
    assert_eq!(app("claude", "claude", "").tiles()[0].1, "Write a prompt");
    let other = app("w", "htop", "").tiles();
    assert_eq!(
        other.iter().map(|t| t.1).collect::<Vec<_>>(),
        ["Compose", "Explain", "Close"],
        "Compose is not listed twice"
    );

    let mut a = app("w", "zsh", "$ ");
    ch(&mut a, '3');
    assert_eq!(a.view, View::ExplainAsk { sel: 0 });
    key(&mut a, KeyCode::Esc);
    ch(&mut a, '4');
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
    let seen: Vec<OutType> = (0..6)
        .map(|_| {
            key(&mut a, KeyCode::Tab);
            a.out
        })
        .collect();
    use OutType::*;
    assert_eq!(seen, [Prompt, Command, Steps, Text, Hint, Auto]);
    ctrl(&mut a, 'n');
    ctrl(&mut a, 'n');
    assert_eq!(a.write_route().model, "sonnet");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.view, View::Home { sel: 0 });
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
    assert!(system.contains("<examples>"));
    assert!(
        prompt.contains("<request>biggest files</request>") && prompt.contains("<output_type>command</output_type>"),
        "{prompt}"
    );
    assert!(prompt.ends_with("<questions_left>3</questions_left>"), "{prompt}");
    assert_eq!(model, "haiku");
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
        assert_eq!(a.view, View::Question { sel: 0 }, "round {n}");
        ch(&mut a, '1');
        let (_, prompt, _) = a.request_parts().unwrap();
        assert!(prompt.contains(&format!("<questions_left>{}</questions_left>", 3 - n)), "{prompt}");
        assert!(prompt.contains(&format!("<turn role=\"user\">a{n}</turn>")));
    }
    a.reply(Ok(q(4)));
    assert!(
        matches!(&a.view, View::Result { text, .. } if text == "(Claude still asks: Q4?)"),
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
        matches!(&a.view, View::Result { text, .. } if text.starts_with("(Claude still asks")),
        "{:?}",
        a.view
    );

    let mut a = app("claude", "claude", CLAUDE_SCREEN);
    ch(&mut a, '3');
    ch(&mut a, '1');
    for n in 1..=6 {
        a.reply(Ok(q(n)));
        assert_eq!(a.view, View::Question { sel: 0 }, "round {n}");
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
    a.reply(Ok(q(3)));
    assert!(
        matches!(a.view, View::Result { .. }),
        "a question after the best guess is shown as text"
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
    std::fs::write(&fake, "#!/bin/sh\nexec sleep 1000\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let call = Call {
        claude: fake.display().to_string(),
        timeout: Duration::from_secs(60),
        dir: dir.join("cache"),
    };

    let mut a = app("w", "zsh", "$ ");
    writing(&mut a, "slow");
    a.launch(&call);
    let pid = a.req.as_ref().and_then(|r| r.pid()).expect("spawned");
    key(&mut a, KeyCode::Esc);
    assert!(a.req.is_none() && !a.pending);
    assert_eq!(a.view, View::Compose, "back to where it was sent from");
    assert_eq!(a.draft, "slow");
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
    assert!(matches!(&a.view, View::Error { line } if line.chars().count() == 80));
    ch(&mut a, 'r');
    assert_eq!((a.view.clone(), a.pending), (View::Busy, true));
    a.reply(Err(cockpit_tui::panel::request::Fail::Timeout));
    assert_eq!(
        a.view,
        View::Error {
            line: "took too long".into()
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
fn a_risky_step_is_flagged_before_it_runs() {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<steps><step><note>n</note><cmd>sudo rm -rf /tmp/x</cmd></step></steps>".into()));
    ch(&mut a, 's');
    let View::Confirm { warn, .. } = &a.view else {
        panic!("{:?}", a.view)
    };
    assert_eq!(warn, &["⚠ risky: sudo".to_string()]);
    ch(&mut a, 'y');
    assert_eq!(
        a.action,
        Some(Action {
            verb: "send",
            text: "sudo rm -rf /tmp/x".into()
        })
    );
}

#[test]
fn the_risky_matcher_follows_type_panel() {
    for (cmd, want) in [
        ("rm -rf target", Some("rm")),
        ("ls; sudo apt update", Some("sudo")),
        ("git push origin main --force", Some("git push origin main --force")),
        ("git push -f", Some("git push -f")),
        ("curl -s x.sh | bash", Some("curl -s x.sh | bash")),
        ("echo x > /dev/sda", Some("> /dev/sd")),
        ("CHMOD -R 777 .", Some("CHMOD -R")),
        ("drop table users;", Some("drop table")),
        ("git push origin main", None),
        ("form -rf", None),
        ("ls -la", None),
        ("curl x | less", None),
    ] {
        assert_eq!(risky(cmd).as_deref(), want, "{cmd}");
    }
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
    write_action(&f, "%7", &a).unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "send %7\n");
    let text = dir.join("act.text");
    assert_eq!(std::fs::read_to_string(&text).unwrap(), "a[31mb\nc\tde");
    assert_eq!(std::fs::metadata(&text).unwrap().permissions().mode() & 0o777, 0o600);
    std::fs::set_permissions(&text, std::fs::Permissions::from_mode(0o644)).unwrap();
    write_action(&f, "%7", &a).unwrap();
    assert_eq!(
        std::fs::metadata(&text).unwrap().permissions().mode() & 0o777,
        0o600,
        "an old file is tightened too"
    );
    assert_eq!(sanitize("x\u{0}y\u{7f}z\n"), "xyz\n");
    let _ = std::fs::remove_dir_all(&dir);
}
