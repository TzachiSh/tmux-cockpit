//! Write's batched questions: a reply that asks up to 3 independent questions
//! at once, answered one by one and sent back in one round.
use cockpit_tui::panel::app::{App, InputKind, Tile, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Role, Turn};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn app() -> App {
    let cap = Capture {
        name: "w".into(),
        cmd: "zsh".into(),
        path: "/home/u/proj".into(),
        screen: "$ ".into(),
        ..Default::default()
    };
    App::new("%3", cap)
}

fn key(a: &mut App, code: KeyCode) {
    a.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ch(a: &mut App, c: char) {
    key(a, KeyCode::Char(c));
}

/// Compose, a draft, ^S, and `reply` back from the call; the call is marked
/// launched, so `pending` says whether a new one is due.
fn replied(reply: &str) -> App {
    let mut a = app();
    let i = a.tiles().iter().position(|t| t.0 == Tile::Compose).unwrap();
    ch(&mut a, char::from(b'1' + i as u8));
    for c in "do it".chars() {
        ch(&mut a, c);
    }
    a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert_eq!(a.view, View::Busy);
    a.pending = false;
    a.reply(Ok(reply.into()));
    a
}

fn q(n: usize) -> String {
    format!("<question>Q{n}?</question><option>a{n}</option><option>b{n}</option>")
}

fn batch(ns: &[usize]) -> String {
    ns.iter().map(|&n| q(n)).collect()
}

/// The question on screen and its place in the batch, from the run.
fn on(a: &App) -> (String, (usize, usize)) {
    let r = a.run.as_ref().unwrap();
    (r.question.clone(), r.place())
}

fn turn(role: Role, text: &str) -> Turn {
    Turn { role, text: text.into() }
}

fn tail(a: &App, n: usize) -> Vec<Turn> {
    let t = &a.run.as_ref().unwrap().turns;
    t[t.len().saturating_sub(n)..].to_vec()
}

#[test]
fn a_batch_walks_from_question_1_to_n() {
    let mut a = replied(&batch(&[1, 2, 3]));
    assert_eq!(on(&a), ("Q1?".into(), (1, 3)));
    ch(&mut a, '1');
    assert_eq!(on(&a), ("Q2?".into(), (2, 3)));
    ch(&mut a, '2');
    assert_eq!(on(&a), ("Q3?".into(), (3, 3)));
    assert_eq!(a.view, View::Question { sel: 0, scroll: None });
}

#[test]
fn a_single_question_is_question_1_of_1() {
    let a = replied(&q(1));
    assert_eq!(on(&a), ("Q1?".into(), (1, 1)));
}

#[test]
fn a_batch_goes_back_in_one_call_with_every_answer() {
    let mut a = replied(&batch(&[1, 2]));
    ch(&mut a, '1');
    assert!(!a.pending, "no call between the questions of a batch");
    ch(&mut a, '2');
    assert_eq!((a.view.clone(), a.pending), (View::Busy, true));
    let want = [
        turn(Role::Assistant, "Q1? Options: a1 | b1"),
        turn(Role::User, "a1"),
        turn(Role::Assistant, "Q2? Options: a2 | b2"),
        turn(Role::User, "b2"),
    ];
    assert_eq!(a.run.as_ref().unwrap().turns, want);
}

#[test]
fn a_batch_of_two_takes_two_from_the_questions_left() {
    let mut a = replied(&batch(&[1, 2]));
    ch(&mut a, '1');
    ch(&mut a, '1');
    let r = a.run.as_ref().unwrap();
    assert_eq!((r.rounds, r.max), (2, 3));
    let (_, prompt, _) = a.request_parts().unwrap();
    assert!(prompt.ends_with("<questions_left>1</questions_left>"), "{prompt}");
}

#[test]
fn a_batch_past_the_questions_left_keeps_its_first_ones() {
    let mut a = replied(&q(1));
    ch(&mut a, '1');
    a.reply(Ok(batch(&[2, 3, 4])));
    assert_eq!(on(&a), ("Q2?".into(), (1, 2)));
    assert_eq!(a.run.as_ref().unwrap().rounds, 3);
}

#[test]
fn esc_mid_batch_leaves_to_the_draft() {
    let mut a = replied(&batch(&[1, 2]));
    ch(&mut a, '1');
    key(&mut a, KeyCode::Esc);
    assert_eq!((a.view.clone(), a.draft.as_str()), (View::Compose, "do it"));
    assert!(a.run.is_none(), "the batch's answers go with the run");
}

#[test]
fn back_mid_batch_shows_the_previous_question_without_its_answer() {
    let mut a = replied(&batch(&[1, 2]));
    ch(&mut a, '2');
    key(&mut a, KeyCode::Backspace);
    assert_eq!(on(&a), ("Q1?".into(), (1, 2)));
    assert_eq!(tail(&a, 1), [turn(Role::Assistant, "Q1? Options: a1 | b1")]);
    assert_eq!(a.run.as_ref().unwrap().rounds, 2, "the batch is still asked");
}

#[test]
fn back_from_the_next_round_lands_on_the_batch_s_last_question() {
    let mut a = replied(&batch(&[1, 2]));
    ch(&mut a, '1');
    ch(&mut a, '1');
    a.reply(Ok(q(3)));
    key(&mut a, KeyCode::Backspace);
    assert_eq!(on(&a), ("Q2?".into(), (2, 2)));
    assert_eq!(tail(&a, 2), [turn(Role::User, "a1"), turn(Role::Assistant, "Q2? Options: a2 | b2")]);
    assert_eq!(a.run.as_ref().unwrap().rounds, 2);
}

#[test]
fn cancel_after_the_last_answer_returns_to_the_last_question() {
    let mut a = replied(&batch(&[1, 2]));
    ch(&mut a, '1');
    ch(&mut a, '1');
    key(&mut a, KeyCode::Esc);
    assert_eq!(on(&a), ("Q2?".into(), (2, 2)));
    assert_eq!(tail(&a, 1), [turn(Role::Assistant, "Q2? Options: a2 | b2")]);
}

#[test]
fn best_guess_mid_batch_answers_the_rest_in_the_same_call() {
    let mut a = replied(&batch(&[1, 2, 3]));
    ch(&mut a, '1');
    ch(&mut a, 'g');
    assert_eq!(a.view, View::Busy);
    let guessed: Vec<String> = tail(&a, 4).into_iter().map(|t| t.text).collect();
    assert!(
        guessed[1].contains("best guess") && guessed[3].contains("best guess"),
        "{guessed:?}"
    );
    assert_eq!(guessed[2], "Q3? Options: a3 | b3");
}

#[test]
fn a_cancelled_best_guess_mid_batch_gives_back_the_question_and_rounds() {
    let mut a = replied(&batch(&[1, 2]));
    ch(&mut a, 'g');
    assert_eq!(a.run.as_ref().unwrap().rounds, 3);
    key(&mut a, KeyCode::Esc);
    assert_eq!(on(&a), ("Q1?".into(), (1, 2)));
    assert_eq!(tail(&a, 1), [turn(Role::Assistant, "Q1? Options: a1 | b1")]);
    assert_eq!(a.run.as_ref().unwrap().rounds, 2);
}

#[test]
fn other_answers_one_question_of_the_batch() {
    let mut a = replied(&batch(&[1, 2]));
    ch(&mut a, 'o');
    assert!(
        matches!(
            a.view,
            View::Input {
                kind: InputKind::Other,
                ..
            }
        ),
        "{:?}",
        a.view
    );
    for c in "lab".chars() {
        ch(&mut a, c);
    }
    key(&mut a, KeyCode::Enter);
    assert_eq!(on(&a), ("Q2?".into(), (2, 2)));
    assert_eq!(tail(&a, 2)[0], turn(Role::User, "lab"));
}

#[test]
fn a_batch_with_no_questions_left_is_told_once_to_write() {
    let mut a = replied(&q(1));
    ch(&mut a, 'g');
    a.reply(Ok(batch(&[2, 3])));
    assert_eq!(a.view, View::Busy, "told once to write now");
    a.reply(Ok(batch(&[2, 3])));
    assert!(
        matches!(&a.view, View::Error { line, .. } if line == "no questions left, and Claude still asks: Q2?"),
        "{:?}",
        a.view
    );
}

#[test]
fn a_result_after_a_guess_leaves_no_batch_questions_queued() {
    let mut a = replied(&batch(&[1, 2, 3]));
    ch(&mut a, 'g');
    a.reply(Ok("<prompt>Do it.</prompt>".into()));
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
    assert_eq!(a.run.as_ref().unwrap().ahead, vec![], "the guess answered them");
}

/// The question on screen, its place and the rounds asked.
fn state(a: &App) -> (String, (usize, usize), usize) {
    let r = a.run.as_ref().unwrap();
    (r.question.clone(), r.place(), r.rounds)
}

#[test]
fn an_edit_reply_after_a_guess_keeps_the_batch_for_esc() {
    let mut a = replied(&batch(&[1, 2, 3]));
    ch(&mut a, 'g');
    a.reply(Ok("<edit path=\"p\">x</edit>".into()));
    assert!(matches!(a.view, View::Error { .. }), "{:?}", a.view);
    key(&mut a, KeyCode::Esc);
    assert_eq!(state(&a), ("Q1?".into(), (1, 3), 3));
}

#[test]
fn a_question_after_a_guess_keeps_the_batch_when_its_retry_is_cancelled() {
    let mut a = replied(&batch(&[1, 2, 3]));
    ch(&mut a, 'g');
    a.reply(Ok(q(4)));
    assert_eq!(a.view, View::Busy, "told once to write now");
    key(&mut a, KeyCode::Esc);
    assert_eq!(state(&a), ("Q1?".into(), (1, 3), 3));
    assert_eq!(tail(&a, 1), [turn(Role::Assistant, "Q1? Options: a1 | b1")]);
}
