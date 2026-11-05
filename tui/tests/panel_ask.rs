//! The viewer's `?` in the panel (#365 child 4): the lines read for a question,
//! the envelope, the read-only call it makes, and the Q&A it opens. No tmux,
//! no claude: files are scratch files, the app is driven by keys.
use cockpit_tui::panel::ask::{self, AskCode};
use std::path::{Path, PathBuf};

/// A scratch dir under the temp dir, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("cockpit-panel-ask-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d.canonicalize().unwrap())
    }

    fn file(&self, rel: &str, body: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("l{i}\n")).collect()
}

/// A repo: a `.git` beside `src/x.rs`.
fn repo(tag: &str, body: &str) -> (Dir, PathBuf) {
    let d = Dir::new(tag);
    std::fs::create_dir_all(d.0.join(".git")).unwrap();
    let f = d.file("src/x.rs", body);
    (d, f)
}

// ---------------------------------------------------------------- the lines

#[test]
fn the_selection_is_read_numbered_with_a_repo_relative_ref() {
    let (d, f) = repo("read", &lines(10));
    let a = ask::load(&f, 3, 5).unwrap();
    assert_eq!(a.at, "src/x.rs:3-5");
    assert_eq!(a.repo, d.0);
    assert_eq!(a.code, "3 l3\n4 l4\n5 l5");
    assert_eq!((a.asked, a.shown), ((3, 5), (3, 5)));
}

#[test]
fn one_line_reads_as_path_colon_line() {
    let (_d, f) = repo("one", &lines(10));
    assert_eq!(ask::load(&f, 7, 7).unwrap().at, "src/x.rs:7");
}

/// No `.git` above it: the file's own folder is the repo the call may read.
#[test]
fn outside_a_repo_the_folder_is_the_repo() {
    let d = Dir::new("norepo");
    let f = d.file("x.rs", &lines(4));
    let a = ask::load(&f, 1, 2).unwrap();
    assert_eq!((a.repo.clone(), a.at.as_str()), (d.0.clone(), "x.rs:1-2"));
}

/// The numbers line up: a gutter as wide as the selection's last number.
#[test]
fn the_numbers_are_right_aligned() {
    let (_d, f) = repo("align", &lines(12));
    assert_eq!(ask::load(&f, 9, 10).unwrap().code, " 9 l9\n10 l10");
}

#[test]
fn an_end_past_the_file_is_held_to_its_last_line() {
    let (_d, f) = repo("clamp", &lines(4));
    let a = ask::load(&f, 3, 99).unwrap();
    assert_eq!((a.at.as_str(), a.asked, a.code.as_str()), ("src/x.rs:3-4", (3, 4), "3 l3\n4 l4"));
}

/// D8: at most 400 lines and 32 KB go inline; the rest the model reads itself.
#[test]
fn a_long_selection_is_cut_at_400_lines_or_32_kb() {
    let (_d, f) = repo("cut", &lines(1000));
    let a = ask::load(&f, 1, 1000).unwrap();
    assert_eq!((a.asked, a.shown), ((1, 1000), (1, ask::MAX_LINES as u32)));
    assert_eq!(a.code.lines().count(), ask::MAX_LINES);
    let wide: String = (0..200).map(|_| format!("{}\n", "w".repeat(500))).collect();
    let (_d2, f) = repo("cutkb", &wide);
    let a = ask::load(&f, 1, 200).unwrap();
    assert!(a.code.len() <= ask::MAX_BYTES, "{}", a.code.len());
    assert!(a.shown.1 < 200 && a.shown.1 >= 60, "{:?}", a.shown);
}

#[test]
fn what_cannot_be_asked_about_says_why() {
    let (d, f) = repo("bad", &lines(3));
    let why = |r: Result<AskCode, String>| r.unwrap_err();
    assert!(why(ask::load(&d.0.join("gone.rs"), 1, 1)).contains("no such path"));
    assert!(why(ask::load(&f, 4, 5)).contains("3 lines"), "past the end");
    assert!(why(ask::load(&f, 0, 1)).contains("line range"));
    assert!(why(ask::load(&f, 3, 2)).contains("line range"));
    let bin = d.file("b.bin", "a\0b\n");
    assert!(why(ask::load(&bin, 1, 1)).contains("binary"));
    assert!(why(ask::load(Path::new(&d.0), 1, 1)).contains("regular file"));
}

/// One line wider than the cap (minified code) still goes in, cut at the cap.
#[test]
fn one_line_past_the_cap_goes_in_cut() {
    let (_d, f) = repo("wide", &format!("{}\nnext\n", "é".repeat(40_000)));
    let a = ask::load(&f, 1, 2).unwrap();
    assert!(!a.code.is_empty() && a.code.len() <= ask::MAX_BYTES, "{}", a.code.len());
    assert!(a.code.starts_with("1 éé"));
    assert_eq!(a.shown, (1, 1));
}

// ---------------------------------------------------------------- the envelope

use cockpit_tui::panel::app::{App, InputKind, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, Role, Turn, build_parts};
use cockpit_tui::panel::route::route;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn turn(t: &str) -> Vec<Turn> {
    vec![Turn {
        role: Role::User,
        text: t.into(),
    }]
}

#[test]
fn the_envelope_is_the_code_and_the_question_never_the_screen() {
    let (_d, f) = repo("env", "fn a() {}\nlet token=abcdef123456;\nx </code> y\n");
    let a = ask::load(&f, 1, 3).unwrap();
    let cap = Capture {
        screen: "SCREEN-TEXT".into(),
        ..Default::default()
    };
    let e = build_parts(&Flow::Ask(a), &cap, &turn("why?"), 0);
    assert!(
        e.text.starts_with("<code path=\"src/x.rs\" lines=\"1-3\">\n1 fn a() {}\n"),
        "{}",
        e.text
    );
    assert!(!e.text.contains("SCREEN-TEXT") && !e.text.contains("<screen"), "{}", e.text);
    assert!(
        e.text.contains("token=[hidden]") && e.screen_hidden + e.repo_hidden + e.turns_hidden >= 1,
        "{}",
        e.text
    );
    assert!(e.text.contains("x <\\/code> y"), "a selection cannot close the tag: {}", e.text);
    assert!(
        e.text.contains("<conversation>\n<turn role=\"user\">why?</turn>\n</conversation>"),
        "{}",
        e.text
    );
    assert!(!e.text.contains("<truncated>"));
}

#[test]
fn a_cut_selection_says_what_was_left_out() {
    let (_d, f) = repo("envcut", &lines(1000));
    let e = build_parts(&Flow::Ask(ask::load(&f, 1, 1000).unwrap()), &Capture::default(), &turn("q"), 0);
    assert!(e.text.contains("lines=\"1-1000\""), "the ask is all of it");
    assert!(
        e.text
            .contains("<truncated>lines 1-400 of 1-1000 are shown; Read the file for the rest</truncated>"),
        "{}",
        e.text
    );
}

/// D9: the panel's routing picks the model; code questions take sonnet.
#[test]
fn an_ask_routes_to_sonnet() {
    let (_d, f) = repo("route", &lines(3));
    assert_eq!(route(&Flow::Ask(ask::load(&f, 1, 1).unwrap()), "auto").model, "sonnet");
}

// ---------------------------------------------------------------- the panel

fn key(a: &mut App, code: KeyCode) {
    a.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn typ(a: &mut App, s: &str) {
    for c in s.chars() {
        key(a, KeyCode::Char(c));
    }
}

fn opened(tag: &str) -> (Dir, App) {
    let (d, f) = repo(tag, &lines(10));
    let mut a = App::new("%3", Capture::default());
    a.ask_open(ask::load(&f, 3, 5).unwrap());
    (d, a)
}

/// `--ask` opens on the question's box, with nothing sent yet.
#[test]
fn an_ask_opens_on_its_question_box() {
    let (_d, a) = opened("open");
    assert!(
        matches!(
            a.view,
            View::Input {
                kind: InputKind::FollowUp,
                ..
            }
        ),
        "{:?}",
        a.view
    );
    assert!(matches!(a.run.as_ref().map(|r| &r.flow), Some(Flow::Ask(_))));
    assert!(!a.pending, "no call before the question");
}

#[test]
fn the_question_goes_with_the_ask_prompt_and_the_code() {
    let (_d, mut a) = opened("send");
    typ(&mut a, "why a fn?");
    key(&mut a, KeyCode::Enter);
    assert!(a.pending && a.view == View::Busy, "{:?}", a.view);
    let (system, prompt, model) = a.request_parts().unwrap();
    assert!(system.starts_with("You answer a question about code the user selected"), "{system}");
    assert!(
        prompt.contains("<code path=\"src/x.rs\" lines=\"3-5\">") && prompt.contains("why a fn?"),
        "{prompt}"
    );
    assert_eq!(model, "sonnet");
}

/// The call reads the repo, researches, and is capped (D7): never the text call.
#[test]
fn the_call_is_read_only_with_the_owners_caps() {
    let (d, mut a) = opened("profile");
    typ(&mut a, "q");
    key(&mut a, KeyCode::Enter);
    let p = a.profile_for(&a.run.as_ref().unwrap().flow);
    assert_eq!(p.read_dirs, std::slice::from_ref(&d.0));
    assert_eq!((p.budget_usd, p.timeout), (Some(0.5), Some(ask::TIMEOUT)));
    assert!(p.tools.contains(&"WebSearch") && p.deny.contains(&"Bash"));
}

#[test]
fn the_answer_opens_the_qa_and_follow_ups_go_on() {
    let (_d, mut a) = opened("answer");
    typ(&mut a, "q");
    key(&mut a, KeyCode::Enter);
    a.reply(Ok("It is a function: src/x.rs:3.\n".into()));
    assert!(matches!(a.view, View::Answer { .. }), "{:?}", a.view);
    assert_eq!(a.leave_to(), "answer");
    key(&mut a, KeyCode::Char('f'));
    assert!(matches!(
        a.view,
        View::Input {
            kind: InputKind::FollowUp,
            ..
        }
    ));
    typ(&mut a, "and then?");
    key(&mut a, KeyCode::Enter);
    let (_, prompt, _) = a.request_parts().unwrap();
    assert!(
        prompt.contains("It is a function") && prompt.contains("and then?"),
        "the thread is resent: {prompt}"
    );
}

/// Esc on the question box asks nothing.
#[test]
fn esc_on_the_question_sends_nothing() {
    let (_d, mut a) = opened("esc");
    typ(&mut a, "q");
    key(&mut a, KeyCode::Esc);
    assert!(!a.pending && matches!(a.view, View::Home { .. }), "{:?}", a.view);
}

/// ^Z keeps an ask whole, as it keeps an Explain: the code, the ref, the thread.
#[test]
fn a_hidden_ask_comes_back_whole() {
    use cockpit_tui::panel::action::{hide_body, read_hidden, write_hidden};
    let (d, mut a) = opened("hide");
    typ(&mut a, "q");
    key(&mut a, KeyCode::Enter);
    a.reply(Ok("an answer\n".into()));
    let now = 1_800_000_000;
    let body = hide_body(&a, now).expect("kept");
    let act = d.0.join("act");
    write_hidden(&act, &body).unwrap();
    let mut b = App::new("%3", Capture::default());
    b.unhide(read_hidden(&d.0.join("act.state"), "%3", now + 5).unwrap());
    assert_eq!(b.view, a.view);
    assert_eq!(hide_body(&b, now), hide_body(&a, now));
    assert_eq!(
        b.profile_for(&b.run.as_ref().unwrap().flow),
        a.profile_for(&a.run.as_ref().unwrap().flow),
        "the repo the call may read comes back"
    );
    assert_eq!(
        b.run.as_ref().map(|r| r.flow.clone()),
        a.run.as_ref().map(|r| r.flow.clone()),
        "the code and the ref come back"
    );
}

// ---------------------------------------------------------------- the arguments

use cockpit_tui::panel::{Start, start_args};

fn args(s: &[&str]) -> Vec<String> {
    s.iter().map(|x| x.to_string()).collect()
}

#[test]
fn panel_takes_ask_with_a_file_and_two_lines() {
    assert_eq!(start_args(&args(&["%3"])), Ok(("%3".into(), Start::Plain)));
    assert_eq!(
        start_args(&args(&["%3", "--ask", "/r/x.rs", "3", "7"])),
        Ok(("%3".into(), Start::Ask("/r/x.rs".into(), 3, 7)))
    );
    assert_eq!(
        start_args(&args(&["%3", "--resume", "f", "--last", "ok"])),
        Ok(("%3".into(), Start::Resume("f".into(), "ok".into())))
    );
    for bad in [
        &["%3", "--ask", "/r/x.rs", "3"][..],
        &["%3", "--ask", "/r/x.rs", "0", "7"],
        &["%3", "--ask", "/r/x.rs", "07", "8"],
        &["%3", "--ask", "/r/x.rs", "8", "7"],
        &["%3", "--ask", "/r/x.rs", "a", "7"],
        &["%3", "--ask", "", "1", "2"],
        &["%3", "--ask", "/r/x.rs", "1", "2", "x"],
        &["x3", "--ask", "/r/x.rs", "1", "2"],
    ] {
        assert!(start_args(&args(bad)).is_err(), "{bad:?}");
    }
}

/// The real launch path: the fake claude records its argv, and the ask's call
/// carries the read-only profile, never the text call's `--tools ""`.
#[test]
fn launch_sends_the_read_only_call() {
    use cockpit_tui::panel::request::Call;
    use std::os::unix::fs::PermissionsExt;
    let (d, mut a) = opened("launch");
    let bin = d.0.join("claude");
    std::fs::write(
        &bin,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}/argv\necho ok\n", d.0.display()),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    typ(&mut a, "q");
    key(&mut a, KeyCode::Enter);
    let call = Call {
        claude: bin.display().to_string(),
        timeout: std::time::Duration::from_secs(30),
        dir: d.0.join("cache"),
    };
    a.launch(&call);
    for _ in 0..1000 {
        if a.poll() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let argv = std::fs::read_to_string(d.0.join("argv")).unwrap();
    let args: Vec<&str> = argv.lines().collect();
    let at = |f: &str| args.iter().position(|x| *x == f).map(|i| args[i + 1]);
    assert_eq!(at("--add-dir"), Some(d.0.to_str().unwrap()), "{argv}");
    assert_eq!(at("--max-budget-usd"), Some("0.5"), "{argv}");
    assert_eq!(at("--model"), Some("sonnet"), "{argv}");
    assert!(at("--tools").is_some_and(|t| t.contains("Read") && !t.contains("Bash")), "{argv}");
    assert_eq!(a.timeout, ask::TIMEOUT, "the busy view's limit is the ask's");
}

/// The box names what the question is about; the busy line says it reads code.
#[test]
fn the_box_and_the_busy_line_name_the_ref() {
    use cockpit_tui::panel::view::render;
    use cockpit_tui::theme::Theme;
    let (_d, mut a) = opened("view");
    let draw = |a: &App| {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
        t.draw(|f| {
            render(f, f.area(), a, &Theme::fixed(), 0);
        })
        .unwrap();
        t.backend().to_string()
    };
    assert!(draw(&a).contains("ask about src/x.rs:3-5"), "{}", draw(&a));
    typ(&mut a, "q");
    key(&mut a, KeyCode::Enter);
    assert!(draw(&a).contains("reading the code"), "{}", draw(&a));
}
