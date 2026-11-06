//! Flow A's "send to agent" (#365 child 5, PR 4): `c` on an ask's answer opens
//! Compose prefilled (owner's D8), the ask's ref in the basket; esc goes back to
//! the answer (D3), and a write from there ends the thread. No tmux: the basket
//! write is refused by the test guard, and the ref is kept on the App.
use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::ask;
use cockpit_tui::panel::capture::Capture;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn key(a: &mut App, code: KeyCode) {
    a.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

/// An ask about src/x.rs:3-5, asked "why a fn?", answered with `reply`.
fn answered(tag: &str, reply: &str) -> (Dir, App) {
    let d = std::env::temp_dir().join(format!("cockpit-panel-prefill-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join(".git")).unwrap();
    std::fs::create_dir_all(d.join("src")).unwrap();
    let f = d.join("src/x.rs");
    std::fs::write(&f, (1..=10).map(|i| format!("l{i}\n")).collect::<String>()).unwrap();
    let d = Dir(d.canonicalize().unwrap());
    let cap = Capture {
        path: d.0.display().to_string(),
        ..Default::default()
    };
    let mut a = App::new("%3", cap);
    a.ask_open(ask::load(&f, 3, 5).unwrap());
    for c in "why a fn?".chars() {
        key(&mut a, KeyCode::Char(c));
    }
    key(&mut a, KeyCode::Enter);
    a.reply(Ok(reply.into()));
    assert!(matches!(a.view, View::Answer { .. }), "{:?}", a.view);
    (d, a)
}

#[test]
fn c_with_a_prompt_prefills_the_prompt() {
    let (_d, mut a) = answered("prompt", "Split it.\n<prompt>Make x a method of Counter.</prompt>\n");
    key(&mut a, KeyCode::Char('c'));
    assert!(matches!(a.view, View::Compose), "{:?}", a.view);
    assert_eq!(a.draft, "Make x a method of Counter.");
}

#[test]
fn c_without_a_prompt_prefills_the_template() {
    let (_d, mut a) = answered("template", "It is a function: src/x.rs:3.\n\nMore below.\n");
    key(&mut a, KeyCode::Char('c'));
    assert_eq!(a.draft, "About src/x.rs:3-5: why a fn?\nIt is a function: src/x.rs:3.");
}

/// The prefill is cut to 2000 characters, at a space, with a visible ellipsis.
#[test]
fn a_long_prefill_is_cut_at_2000_characters() {
    let long = "word ".repeat(1000);
    let (_d, mut a) = answered("cap", &format!("{long}\n"));
    key(&mut a, KeyCode::Char('c'));
    let n = a.draft.chars().count();
    assert!(n <= 2000 && a.draft.ends_with('…'), "{n}: {}", &a.draft[a.draft.len() - 20..]);
    assert!(!a.draft.trim_end_matches('…').ends_with("wor"), "cut between words");
}

/// The ask's lines go in the basket, so the send carries them as a ref.
#[test]
fn c_puts_the_asks_ref_in_the_basket() {
    let (d, mut a) = answered("ref", "It is.\n");
    key(&mut a, KeyCode::Char('c'));
    let want = cockpit_tui::refs::Ref::new(d.0.join("src/x.rs"), Some((3, 5)), None).unwrap();
    assert_eq!(a.basket, [want]);
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Char('c'));
    assert_eq!(a.basket.len(), 1, "a second c adds it once");
}

/// D3: the thread is kept, and esc in that Compose is back at the answer.
#[test]
fn esc_in_that_compose_goes_back_to_the_answer() {
    let (_d, mut a) = answered("esc", "It is.\n");
    key(&mut a, KeyCode::Char('c'));
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Answer { .. }), "{:?}", a.view);
    assert!(a.run.as_ref().is_some_and(|r| r.log.len() == 2), "the thread is kept");
}

/// The owner's rule: ^S from that Compose ends the thread, so esc after the
/// write goes to the board, never back to the answer.
#[test]
fn a_write_from_that_compose_ends_the_thread() {
    let (_d, mut a) = answered("write", "It is.\n");
    key(&mut a, KeyCode::Char('c'));
    ctrl(&mut a, 's');
    assert!(
        a.run
            .as_ref()
            .is_some_and(|r| matches!(r.flow, cockpit_tui::panel::envelope::Flow::Write { .. })),
        "the write is a run of its own"
    );
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Home { .. }), "{:?}", a.view);
}

/// Flow A only: an Explain's answer has no file and lines to send.
#[test]
fn c_on_an_explain_answer_does_nothing() {
    let mut a = App::new("%3", Capture::default());
    a.view = View::ExplainAsk { sel: 0 };
    key(&mut a, KeyCode::Enter);
    a.reply(Ok("an answer\n".into()));
    assert!(matches!(a.view, View::Answer { .. }), "the explain answered: {:?}", a.view);
    key(&mut a, KeyCode::Char('c'));
    assert!(matches!(a.view, View::Answer { .. }), "{:?}", a.view);
}

/// An ask's answer offers `c` on its footer and in its help.
#[test]
fn an_asks_answer_offers_c() {
    let (_d, a) = answered("offer", "It is.\n");
    let g = cockpit_tui::theme::Theme::fixed().g;
    assert!(cockpit_tui::panel::view::keys(&a, &g).iter().any(|(k, _)| k == "c"));
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
    t.draw(|f| {
        cockpit_tui::panel::view::render(f, f.area(), &a, &cockpit_tui::theme::Theme::fixed(), 0);
    })
    .unwrap();
    assert!(t.backend().to_string().contains("c compose"), "{}", t.backend());
}

// ------------------------------------------------- review-510 on #575

/// The answer's text is the model's: an ESC sequence, an OSC 52 and a bidi
/// control never reach the draft, which a send would paste.
#[test]
fn the_prefill_is_cleaned() {
    let dirty = "<prompt>a\u{1b}[31mb \u{1b}]52;c;aGk=\u{7} c\u{202e}d</prompt>";
    let (_d, mut a) = answered("dirty", &format!("x\n{dirty}\n"));
    key(&mut a, KeyCode::Char('c'));
    for (row, c) in [("ESC", '\u{1b}'), ("BEL", '\u{7}'), ("U+202E", '\u{202e}')] {
        assert!(!a.draft.contains(c), "{row} survived: {:?}", a.draft);
    }
    // the whole sequence goes, not only its ESC and BEL (review-510's NIT)
    for body in ["[31m", "]52;c;aGk="] {
        assert!(!a.draft.contains(body), "{body} survived: {:?}", a.draft);
    }
    assert_eq!(a.draft, "ab  cd");
}

/// A long answer with no space near the cap (CJK) is cut at the cap, never
/// backed off to its head line.
#[test]
fn a_long_answer_with_no_spaces_keeps_its_length() {
    let (_d, mut a) = answered("cjk", &format!("{}\n", "漢".repeat(3000)));
    key(&mut a, KeyCode::Char('c'));
    let n = a.draft.chars().count();
    assert!((1950..=2000).contains(&n) && a.draft.ends_with('…'), "{n}");
}

/// The cut is on whole graphemes: an e and its combining acute stay together.
#[test]
fn the_cut_keeps_whole_graphemes() {
    use unicode_segmentation::UnicodeSegmentation;
    let (_d, mut a) = answered("graph", &format!("{}\n", "e\u{301}".repeat(2500)));
    key(&mut a, KeyCode::Char('c'));
    let head = a.draft.trim_end_matches('…');
    assert!(head.ends_with('\u{301}'), "a lone e at the cut");
    assert!(a.draft.graphemes(true).count() <= 2000);
}

/// ^F from that Compose ends the thread as ^S does: esc after it is the board.
#[test]
fn a_fix_from_that_compose_ends_the_thread() {
    let (_d, mut a) = answered("fix", "It is.\n");
    key(&mut a, KeyCode::Char('c'));
    ctrl(&mut a, 'f');
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Home { .. }), "{:?}", a.view);
}

/// review-510: the cap is 2000 characters, on whole graphemes. A family emoji is
/// seven chars and one grapheme; 2100 of them would be an 8000-char draft that
/// meets MAX_DRAFT and loses its ellipsis.
#[test]
fn the_cap_counts_chars_and_keeps_whole_emoji() {
    use unicode_segmentation::UnicodeSegmentation;
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}";
    let (_d, mut a) = answered("emoji", &format!("<prompt>{}</prompt>\n", family.repeat(2100)));
    key(&mut a, KeyCode::Char('c'));
    assert!(
        a.draft.chars().count() <= 2000 && a.draft.ends_with('…'),
        "{}",
        a.draft.chars().count()
    );
    let head = a.draft.trim_end_matches('…');
    assert!(head.graphemes(true).all(|g| g == family), "a family cut in two");
}

/// #588: an OSC that is never terminated ends at the next newline, so the text
/// after it is kept, not swallowed to the end.
#[test]
fn an_unterminated_osc_ends_at_the_newline() {
    let (_d, mut a) = answered("osc", "<prompt>keep this \u{1b}]0;a title\nand the rest</prompt>\n");
    key(&mut a, KeyCode::Char('c'));
    assert_eq!(a.draft, "keep this \nand the rest");
}
