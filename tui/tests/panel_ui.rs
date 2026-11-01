//! The panel's screens and state machine: TestBackend goldens, key sequences,
//! and the action file. COCKPIT_BLESS=1 cargo test rewrites the goldens.
use cockpit_tui::panel::app::{
    Action, App, InputKind, Level, Resume, Tile, View, bracketed_ok, read_state, risky, sanitize, write_action, write_state,
};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::OutType;
use cockpit_tui::panel::parse::Step;
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
    framed(app, w, h, &Theme::fixed(), tick).0
}

fn framed(app: &App, w: u16, h: u16, th: &Theme, tick: u64) -> (String, (usize, usize)) {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut page = (0, 0);
    t.draw(|f| page = render(f, f.area(), app, th, tick)).unwrap();
    (t.backend().to_string(), page)
}

/// A frame as the event loop draws it: the offsets are fitted after it.
fn shown(a: &mut App, w: u16, h: u16) -> String {
    let (f, (total, room)) = framed(a, w, h, &Theme::fixed(), 0);
    a.fit(total, room);
    f
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
fn busy_echoes_what_was_sent_and_its_limit() {
    let mut a = app("work", "bash", "$ ls\nsk-abcdefghijklmnopqrst1 ghp_abcdefghijABCDEFGHIJ12");
    assert_eq!(a.screen_hidden, 2);
    assert!(frame(&a, 80, 20, 0).contains("will send: screen 2 lines, 2 hidden"));
    writing(&mut a, "x");
    assert_eq!(a.hidden, 2, "the envelope's own count");
    let f = frame(&a, 80, 20, 299);
    assert!(f.contains("sent: screen 2 lines, 2 hidden · write (auto)"), "{f}");
    assert!(f.contains("model haiku: short request"), "{f}");
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
        matches!(&a.view, View::Error { line, hint: Some(_) } if line == "no questions left, and Claude still asks: Q4?"),
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
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.run.as_ref().unwrap().rounds, 1, "a cancelled best guess gives the rounds back");
    ch(&mut a, 'g');
    a.reply(Ok(q(3)));
    assert!(
        matches!(a.view, View::Error { .. }),
        "a question after the best guess is an error, never a pasteable result"
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
fn a_risky_or_multi_line_send_needs_y_not_enter() {
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<steps><step><note>n</note><cmd>sudo rm -rf /tmp/x</cmd></step></steps>".into()));
    ch(&mut a, 's');
    assert!(shown(&mut a, 80, 20).contains("y confirm  (↵ is off: risky or multi-line)"));
    key(&mut a, KeyCode::Enter);
    assert!(a.action.is_none() && !a.quit, "Enter does not fire a risky send");
    assert!(matches!(a.view, View::Confirm { .. }) && note(&a).unwrap_or("").contains("press y"));
    ch(&mut a, 'y');
    assert_eq!(a.action.as_ref().map(|x| x.verb), Some("send"));

    // bash takes the paste whole, so no line warning: it is the send that runs both
    let mut a = app("w", "bash", "$ ");
    writing(&mut a, "x");
    a.reply(Ok("<text>make\nmake install</text>".into()));
    ch(&mut a, 's');
    key(&mut a, KeyCode::Enter);
    assert!(a.action.is_none(), "Enter does not fire a multi-line send");
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
    for (w, h) in [(80u16, 20u16), (120, 24)] {
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
    assert!(frame(&a, 80, 20, 0).contains("esc or q again discards it"));
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
    assert!(frame(&b, 80, 20, 0).contains("update  ✓ sent"));

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
