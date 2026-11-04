//! The panel's hide and resume (#278): ^Z keeps the whole state in a version 2
//! state file, and `--resume <file> --last hidden` brings it back as it was.
use cockpit_tui::panel::action::{HIDE_AGE, HIDE_MAX, read_hidden, read_state, write_hidden};
use cockpit_tui::panel::app::{App, InputKind, Level, Tap, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::OutType;
use cockpit_tui::panel::view::render;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const NOW: u64 = 1_800_000_000;

fn app(cmd: &str) -> App {
    let cap = Capture {
        name: "work".into(),
        cmd: cmd.into(),
        path: "/home/u/proj".into(),
        screen: "$ ".into(),
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
        key(a, KeyCode::Char(c));
    }
}

fn note(a: &App) -> Option<&str> {
    a.note.as_ref().map(|n| n.text.as_str())
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cockpit-panel-hide-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The body a hide at `NOW` writes, as `act.state` in a fresh directory.
fn saved(a: &App, tag: &str) -> PathBuf {
    let d = dir(tag);
    let body = cockpit_tui::panel::action::hide_body(a, NOW).expect("kept");
    write_hidden(&d.join("act"), &body).unwrap();
    d.join("act.state")
}

/// ^Z, the file, and a new panel on it: what the next `prefix f` shows.
fn round(a: &mut App, tag: &str) -> App {
    ctrl(a, 'z');
    assert!(a.hide && a.quit, "^Z closes the panel to hide it");
    assert!(a.action.is_none() && a.resume.is_none(), "and acts on nothing");
    let p = saved(a, tag);
    let mut b = app(&a.cap.cmd);
    b.unhide(read_hidden(&p, "%3", NOW + 5).unwrap());
    assert_eq!(b.view, a.view, "{tag}: the view comes back");
    assert_eq!(
        cockpit_tui::panel::action::hide_body(&b, NOW),
        cockpit_tui::panel::action::hide_body(a, NOW),
        "{tag}: the draft, the model, the run and the reply come back too"
    );
    assert!(
        b.action.is_none() && !b.pending && b.req.is_none() && !b.quit,
        "{tag}: nothing is sent, inserted or asked on the way back"
    );
    assert_eq!(note(&b), Some("back where you left it"));
    b
}

/// Compose, a draft, then ^S, and the reply the fake claude gave.
fn replied(draft: &str, reply: &str) -> App {
    let mut a = app("bash");
    key(&mut a, KeyCode::Char('2'));
    assert_eq!(a.view, View::Compose);
    typ(&mut a, draft);
    ctrl(&mut a, 's');
    assert_eq!(a.view, View::Busy);
    a.reply(Ok(reply.into()));
    a
}

#[test]
fn compose_comes_back_with_its_draft_cursor_type_and_model() {
    let mut a = app("bash");
    key(&mut a, KeyCode::Char('2'));
    typ(&mut a, "fix the tests");
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Tab);
    ctrl(&mut a, 'n');
    let b = round(&mut a, "compose");
    assert_eq!((b.draft.as_str(), b.cur), ("fix the tests", 11));
    assert_eq!((b.out, b.model), (OutType::Prompt, 1));
}

#[test]
fn home_and_the_asks_come_back_on_their_selection() {
    let mut a = app("bash");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    round(&mut a, "home");

    let mut a = app("bash");
    key(&mut a, KeyCode::Char('3'));
    key(&mut a, KeyCode::Down);
    assert_eq!(a.view, View::ExplainAsk { sel: 1 });
    round(&mut a, "ask");
}

#[test]
fn a_question_comes_back_with_its_rounds_and_back_works() {
    let mut a = replied("do it", "<question>Q1?</question><option>a1</option><option>b1</option>");
    key(&mut a, KeyCode::Char('1'));
    a.reply(Ok("<question>Q2?</question><option recommended=\"true\">a2</option>".into()));
    key(&mut a, KeyCode::Down);
    let mut b = round(&mut a, "question");
    let r = b.run.as_ref().unwrap();
    assert_eq!((r.rounds, r.question.as_str(), r.qstack.len()), (2, "Q2?", 2));
    assert!(r.options[0].recommended);
    key(&mut b, KeyCode::Backspace);
    assert_eq!(b.run.as_ref().unwrap().question, "Q1?", "the previous question is kept");
}

#[test]
fn a_result_comes_back_and_inserts_only_on_a_key() {
    let mut a = replied("say hi", "<text>hi\tthere \\ \"q\"</text>");
    assert!(matches!(a.view, View::Result { .. }));
    let mut b = round(&mut a, "result");
    key(&mut b, KeyCode::Char('i'));
    assert_eq!(
        b.action.as_ref().map(|x| (x.verb, x.text.as_str())),
        Some(("paste", "hi\tthere \\ \"q\""))
    );
}

#[test]
fn steps_come_back_with_their_marks() {
    let mut a = replied(
        "x",
        "<steps><step><note>one</note><cmd>ls</cmd></step><step><note>tap it</note></step></steps>",
    );
    key(&mut a, KeyCode::Down);
    let b = round(&mut a, "steps");
    assert!(matches!(&b.view, View::Steps { sel: 1, items, .. } if items.len() == 2));
}

#[test]
fn an_answer_and_a_follow_up_box_come_back() {
    let mut a = app("claude");
    key(&mut a, KeyCode::Char('3'));
    key(&mut a, KeyCode::Char('2'));
    a.reply(Ok("It waits.\n<prompt>go on</prompt>".into()));
    assert!(matches!(a.view, View::Answer { .. }));
    let b = round(&mut a, "answer");
    assert_eq!(b.run.as_ref().unwrap().ready.as_deref(), Some("go on"));

    key(&mut a, KeyCode::Char('f'));
    a.hide = false;
    a.quit = false;
    typ(&mut a, "and then?");
    assert!(matches!(
        &a.view,
        View::Input {
            kind: InputKind::FollowUp,
            ..
        }
    ));
    round(&mut a, "follow-up");
}

#[test]
fn an_error_comes_back_and_r_still_retries() {
    let mut a = replied("x", "<text>t</text>");
    key(&mut a, KeyCode::Char('r'));
    a.reply(Err(cockpit_tui::panel::request::Fail::Failed("boom".into())));
    assert!(matches!(a.view, View::Error { .. }));
    let mut b = round(&mut a, "error");
    key(&mut b, KeyCode::Char('r'));
    assert_eq!(b.view, View::Busy, "the retry is the user's key, never the resume");
}

#[test]
fn a_confirm_comes_back_as_the_view_it_was_opened_from() {
    let mut a = replied("say hi", "<text>hi</text>");
    key(&mut a, KeyCode::Char('s'));
    assert!(matches!(a.view, View::Confirm { .. }));
    ctrl(&mut a, 'z');
    let p = saved(&a, "confirm");
    let mut b = app("bash");
    b.unhide(read_hidden(&p, "%3", NOW).unwrap());
    assert!(matches!(b.view, View::Result { .. }), "never on a yes: {:?}", b.view);
}

#[test]
fn busy_refuses_to_hide_and_keeps_the_call() {
    let mut a = app("bash");
    key(&mut a, KeyCode::Char('2'));
    typ(&mut a, "x");
    ctrl(&mut a, 's');
    ctrl(&mut a, 'z');
    assert!(!a.hide && !a.quit && a.pending, "the call goes on");
    assert_eq!(a.view, View::Busy);
    assert_eq!(a.note.as_ref().map(|n| n.level), Some(Level::Warn));
    assert_eq!(note(&a), Some("a call is out: esc cancels it, then ^Z hides"));
    assert!(
        cockpit_tui::panel::action::hide_body(&a, NOW).is_none(),
        "nothing to write for Busy"
    );
    key(&mut a, KeyCode::Esc);
    ctrl(&mut a, 'z');
    assert!(a.hide, "after esc it hides");
}

#[test]
fn a_hide_keeps_a_changed_command_so_the_paste_guard_still_refuses() {
    let mut a = replied("say hi", "<text>hi</text>");
    ctrl(&mut a, 'z');
    let p = saved(&a, "cmd");
    let mut b = app("vim");
    b.unhide(read_hidden(&p, "%3", NOW).unwrap());
    assert_eq!(b.cap.cmd, "bash");
    assert_eq!(b.note.as_ref().map(|n| n.level), Some(Level::Warn));
    assert!(note(&b).unwrap().contains("now runs vim, not bash"), "{:?}", note(&b));
}

#[test]
fn the_state_is_per_pane_private_and_never_overwritten() {
    let mut a = app("bash");
    key(&mut a, KeyCode::Char('2'));
    ctrl(&mut a, 'z');
    let p = saved(&a, "pane");
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(read_hidden(&p, "%4", NOW).err().unwrap().contains("another pane"));
    let body = cockpit_tui::panel::action::hide_body(&a, NOW).unwrap();
    assert!(
        write_hidden(&p.with_file_name("act"), &body).is_err(),
        "created new, never over a file"
    );
}

#[test]
fn a_stale_state_is_dropped() {
    let mut a = app("bash");
    ctrl(&mut a, 'z');
    let p = saved(&a, "stale");
    assert!(read_hidden(&p, "%3", NOW + HIDE_AGE).is_ok(), "24 h is still in");
    let e = read_hidden(&p, "%3", NOW + HIDE_AGE + 1).err().unwrap();
    assert_eq!(e, "it is older than 24 h");
    assert!(read_hidden(&p, "%3", NOW - 120).err().unwrap().contains("future"));
}

fn refused(tag: &str, body: &str) -> String {
    let p = dir(tag).join("s");
    std::fs::write(&p, body).unwrap();
    read_hidden(&p, "%3", NOW).err().expect(tag)
}

#[test]
fn a_corrupt_state_is_refused_whole() {
    let mut a = app("bash");
    key(&mut a, KeyCode::Char('2'));
    typ(&mut a, "héllo");
    ctrl(&mut a, 'z');
    let good = cockpit_tui::panel::action::hide_body(&a, NOW).unwrap();
    assert!(refused("cut", good.trim_end()).contains("cut short"));
    assert!(refused("half", &good[..good.len() / 2]).contains("cut short"));
    assert!(refused("head", &good.replace("state 2", "state 3")).contains("version 2"));
    assert!(refused("json", &good.replace("{\"pane\"", "{pane")).contains("not JSON"));
    assert!(refused("extra", &good.replace("{\"pane\"", "{\"evil\":1,\"pane\"")).contains("unknown key evil"));
    assert!(refused("view", &good.replace("\"compose\"", "\"busy\"")).contains("unknown view"));
    assert!(refused("model", &good.replace("\"model\":0", "\"model\":9")).contains("model"));
    assert!(refused("cur", &good.replace("\"cur\":6", "\"cur\":2")).contains("cursor"));
    assert!(refused("lines", &good.replacen('\n', "\n\n", 1)).contains("one body line"));
    let v1 = "cockpit-panel-state 1\npane %3\ncmd zsh\nlabel L\nsel 0\nverb paste\nstep 0\tbash\tn\tls\n";
    assert!(refused("v1", v1).contains("version 2"), "a steps file is no hidden panel");
    let p = dir("v2").join("s");
    std::fs::write(&p, &good).unwrap();
    assert!(read_state(&p, "%3").is_err(), "and the steps reader refuses a hidden panel");

    let d = dir("odd");
    let link = d.join("link");
    std::os::unix::fs::symlink(&p, &link).unwrap();
    assert!(read_hidden(&link, "%3", NOW).is_err(), "never followed");
    let big = d.join("big");
    std::fs::write(&big, vec![b' '; HIDE_MAX as usize + 1]).unwrap();
    assert!(read_hidden(&big, "%3", NOW).err().unwrap().contains("small regular"));
    assert!(read_hidden(&d.join("none"), "%3", NOW).is_err());
}

#[test]
fn a_question_with_no_run_is_refused() {
    let mut a = replied("do it", "<question>Q?</question><option>a</option>");
    ctrl(&mut a, 'z');
    let good = cockpit_tui::panel::action::hide_body(&a, NOW).unwrap();
    let cut = good.find(",\"run\":").unwrap();
    assert!(refused("norun", &format!("{},\"run\":null}}\n", &good[..cut])).contains("no run"));
}

#[test]
fn the_help_footer_and_buttons_name_the_hide_key() {
    let mut a = app("bash");
    key(&mut a, KeyCode::Char('2'));
    let th = Theme::fixed();
    let keys = cockpit_tui::panel::view::keys(&a, &th.g);
    assert!(keys.iter().any(|(k, v)| k == "^Z" && v.contains("prefix f")), "{keys:?}");
    a.tablet = true;
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut hits = vec![];
    t.draw(|f| hits = render(f, f.area(), &a, &th, 0).2).unwrap();
    let z = Tap::Key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    let h = hits.iter().find(|h| h.tap == z).expect("a [ hide ] button");
    a.frame(0);
    a.tap(z, 10_000);
    assert!(a.hide && a.quit, "the button hides, at {:?}", h.at);
}

#[test]
fn a_long_reply_is_not_lost_to_the_cap() {
    let big = "x".repeat(HIDE_MAX as usize);
    let mut a = replied("x", &format!("<text>{big}</text>"));
    ctrl(&mut a, 'z');
    assert!(!a.hide && !a.quit, "too much to keep: the panel stays open");
    assert!(note(&a).unwrap().contains("too much to keep hidden"));
}

const ASSUMED: &str = "<prompt>Fix it.\nAssumed: a (b)\nAssumed: c (d)</prompt>";

#[test]
fn a_correction_box_comes_back_on_its_assumption_and_sends() {
    let mut a = replied("fix it", ASSUMED);
    key(&mut a, KeyCode::Char('a'));
    key(&mut a, KeyCode::Tab);
    typ(&mut a, "not c");
    assert!(matches!(
        a.view,
        View::Input {
            kind: InputKind::Correct(1),
            ..
        }
    ));
    let mut b = round(&mut a, "correct");
    key(&mut b, KeyCode::Enter);
    assert_eq!(b.view, View::Busy);
    let t = &b.run.as_ref().unwrap().turns.last().unwrap().text;
    assert!(t.starts_with("Correction to \"Assumed: c (d)\": not c"), "{t}");
}

#[test]
fn a_correction_box_past_its_assumptions_is_refused() {
    let mut a = replied("fix it", ASSUMED);
    key(&mut a, KeyCode::Char('a'));
    ctrl(&mut a, 'z');
    let good = cockpit_tui::panel::action::hide_body(&a, NOW).unwrap();
    assert!(good.contains("\"which\":0"), "{good}");
    let bad = good.replace("\"which\":0", "\"which\":2");
    assert!(refused("which", &bad).contains("no assumption"), "{bad}");
}

#[test]
fn a_hidden_correction_at_zero_questions_comes_back() {
    let mut a = replied("fix it", "<question>Q?</question><option>a</option>");
    key(&mut a, KeyCode::Char('g'));
    a.reply(Ok(ASSUMED.into()));
    key(&mut a, KeyCode::Char('a'));
    typ(&mut a, "x");
    key(&mut a, KeyCode::Enter);
    a.reply(Ok(ASSUMED.into()));
    let r = a.run.as_ref().unwrap();
    assert_eq!(r.rounds, r.max, "at the budget, not past it");
    round(&mut a, "zero");
}

#[test]
fn a_correction_box_on_a_fix_run_is_refused() {
    let mut a = replied("fix it", ASSUMED);
    key(&mut a, KeyCode::Char('a'));
    ctrl(&mut a, 'z');
    let good = cockpit_tui::panel::action::hide_body(&a, NOW).unwrap();
    let run = "\"flow\":\"write\",\"draft\":\"fix it\",\"out\":\"auto\"";
    assert!(good.contains(run), "{good}");
    let fix = good.replace(run, "\"flow\":\"fix\",\"draft\":\"fix it\",\"out\":null");
    assert!(refused("fixflow", &fix).contains("not a write"), "{fix}");
}
