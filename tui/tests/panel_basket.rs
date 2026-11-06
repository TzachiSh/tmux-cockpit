//! The panel's side of the prompt basket (#365 child 5, PR 3): Compose shows the
//! refs, ^T with refs always confirms the exact text, `s` sends and `i` inserts,
//! ^B lists them. No tmux: the basket is set on the App as main would read it.
use cockpit_tui::basket::MAX_SEND;
use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::view::render;
use cockpit_tui::refs::Ref;
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A Compose on a pane whose folder holds a.rs and b.md, with both in the basket.
fn compose(tag: &str, claude: bool) -> (Dir, App) {
    let d = std::env::temp_dir().join(format!("cockpit-panel-basket-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let d = Dir(d.canonicalize().unwrap());
    for f in ["a.rs", "b.md"] {
        std::fs::write(d.0.join(f), "x\ny\n").unwrap();
    }
    let cap = Capture {
        name: "claude".into(),
        cmd: "claude".into(),
        path: d.0.display().to_string(),
        ..Default::default()
    };
    let mut a = App::new("%3", cap);
    a.basket = vec![
        Ref::new(d.0.join("a.rs"), Some((1, 2)), None).unwrap(),
        Ref::new(d.0.join("b.md"), None, None).unwrap(),
    ];
    a.claude = claude;
    a.view = View::Compose;
    for c in "fix it".chars() {
        key(&mut a, KeyCode::Char(c));
    }
    (d, a)
}

fn key(a: &mut App, code: KeyCode) {
    a.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn frame(a: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| {
        render(f, f.area(), a, &Theme::fixed(), 0);
    })
    .unwrap();
    t.backend().to_string()
}

fn confirmed(a: &App) -> String {
    match &a.view {
        View::Confirm { text, .. } => text.clone(),
        v => panic!("not a confirm: {v:?}"),
    }
}

#[test]
fn t_with_refs_confirms_the_exact_text_for_claude() {
    let (_d, mut a) = compose("claude", true);
    ctrl(&mut a, 't');
    assert_eq!(confirmed(&a), "fix it\n\n@a.rs#L1-2\n@b.md");
}

#[test]
fn t_with_refs_writes_bare_refs_for_another_agent() {
    let (_d, mut a) = compose("other", false);
    ctrl(&mut a, 't');
    assert_eq!(confirmed(&a), "fix it\n\na.rs:1-2\nb.md");
}

/// D2: in that confirm `s` sends (Enter after) and `i` inserts; either way the
/// basket as sent rides along, for cockpit-panel to clear once the paste lands.
#[test]
fn s_sends_and_i_inserts_and_the_basket_rides_along() {
    for (k, verb) in [('s', "send"), ('i', "paste")] {
        let (_d, mut a) = compose(&format!("verb{k}"), true);
        ctrl(&mut a, 't');
        key(&mut a, KeyCode::Char(k));
        let act = a.action.clone().unwrap_or_else(|| panic!("{k}: no action"));
        assert_eq!((act.verb, act.text.as_str()), (verb, "fix it\n\n@a.rs#L1-2\n@b.md"), "{k}");
        let sent = a.sent_basket.clone().unwrap_or_default();
        assert_eq!(sent.lines().count(), 2, "{k}: {sent}");
    }
}

#[test]
fn esc_on_the_confirm_goes_back_and_sends_nothing() {
    let (_d, mut a) = compose("esc", true);
    ctrl(&mut a, 't');
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Compose) && a.action.is_none() && a.sent_basket.is_none());
}

/// Over 16 KiB the send is refused, never cut: the note says so and Compose stays.
#[test]
fn a_send_over_the_cap_is_refused() {
    let (_d, mut a) = compose("cap", true);
    a.draft = "w".repeat(MAX_SEND);
    ctrl(&mut a, 't');
    assert!(matches!(a.view, View::Compose), "{:?}", a.view);
    let n = a.note.as_ref().map(|n| n.text.clone()).unwrap_or_default();
    assert!(n.contains("too long"), "{n}");
}

/// The strip: each ref from the pane's folder, a whole file marked, the count
/// on the head line; an empty basket draws none of it.
#[test]
fn compose_shows_the_refs_and_marks_a_whole_file() {
    let (_d, a) = compose("strip", true);
    let f = frame(&a, 80, 20);
    assert!(f.contains("refs: 2"), "{f}");
    assert!(f.contains("a.rs:1-2"), "{f}");
    assert!(f.contains("b.md") && f.contains("whole file"), "{f}");
    let (_d, mut e) = compose("nostrip", true);
    e.basket.clear();
    let f = frame(&e, 80, 20);
    assert!(!f.contains("refs:") && !f.contains("whole file"), "{f}");
}

/// A name with a bidi mark is drawn clean (pick::clean), as note_reads does.
#[test]
fn the_strip_draws_a_name_clean() {
    let (d, mut a) = compose("clean", true);
    let odd = d.0.join("x\u{202e}y.rs");
    std::fs::write(&odd, "x\n").unwrap();
    a.basket = vec![Ref::new(&odd, None, None).unwrap()];
    let f = frame(&a, 80, 20);
    assert!(!f.contains('\u{202e}') && f.contains("xy.rs"), "{f}");
}

#[test]
fn ctrl_b_opens_the_basket_and_esc_goes_back() {
    let (_d, mut a) = compose("open", true);
    ctrl(&mut a, 'b');
    assert!(matches!(a.view, View::Basket { sel: 0 }), "{:?}", a.view);
    let f = frame(&a, 80, 20);
    assert!(f.contains("a.rs:1-2") && f.contains("b.md"), "{f}");
    key(&mut a, KeyCode::Down);
    assert!(matches!(a.view, View::Basket { sel: 1 }), "{:?}", a.view);
    key(&mut a, KeyCode::Esc);
    assert!(matches!(a.view, View::Compose), "{:?}", a.view);
    assert_eq!(a.draft, "fix it", "the draft is kept");
}

/// `<action>.basket` holds the basket as sent, beside the action, mode 0600.
#[test]
fn the_sent_basket_is_written_beside_the_action() {
    use std::os::unix::fs::PermissionsExt;
    let (d, _) = compose("write", true);
    let f = d.0.join("act");
    cockpit_tui::panel::action::write_basket(&f, "/r/a.rs:1-2").unwrap();
    let b = d.0.join("act.basket");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "/r/a.rs:1-2");
    assert_eq!(std::fs::metadata(&b).unwrap().permissions().mode() & 0o777, 0o600);
}

/// The footer and the help name ^B only when there are refs to list, so a
/// Compose with an empty basket is drawn as before.
#[test]
fn b_is_offered_only_with_refs() {
    let g = Theme::fixed().g;
    let (_d, mut a) = compose("help", true);
    let has_b = |a: &App| cockpit_tui::panel::view::keys(a, &g).iter().any(|(k, _)| k == "^B");
    assert!(has_b(&a) && frame(&a, 120, 20).contains("^B refs"));
    a.basket.clear();
    assert!(!has_b(&a) && !frame(&a, 120, 20).contains("^B"));
}

// ------------------------------------- review-534 on #570: D2's s is on screen

/// The basket's confirm says what Enter does, and offers s and i on its footer.
#[test]
fn the_basket_confirm_offers_s_and_i_and_says_what_enter_does() {
    let (_d, mut a) = compose("offer", true);
    ctrl(&mut a, 't');
    let f = frame(&a, 120, 20);
    assert!(f.contains("inserts it, no Enter") && f.contains("s sends it with Enter"), "{f}");
    // the footer row itself, not the prompt line above it
    let foot = f.lines().rev().find(|l| l.contains("cancel")).unwrap_or("");
    assert!(foot.contains("s send") && foot.contains("insert"), "{foot}");
    let g = Theme::fixed().g;
    let keys = cockpit_tui::panel::view::keys(&a, &g);
    for k in ["s", "i"] {
        assert!(keys.iter().any(|(key, _)| key.split(' ').any(|w| w == k)), "F1 lacks {k}: {keys:?}");
    }
}

/// On a tablet the send is a button too, and a tap on it sends.
#[test]
fn a_tablet_can_tap_send() {
    let (_d, mut a) = compose("tablet", true);
    a.tablet = true;
    ctrl(&mut a, 't');
    let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut hits = vec![];
    t.draw(|f| hits = render(f, f.area(), &a, &Theme::fixed(), 0).2).unwrap();
    let s = KeyEvent::from(KeyCode::Char('s'));
    assert!(hits.iter().any(|h| h.tap == cockpit_tui::panel::app::Tap::Key(s)), "no send button");
    a.key(s);
    assert_eq!(a.action.as_ref().map(|x| x.verb), Some("send"));
}

/// review-534: the option can hold a ref whose file is gone, which the read
/// drops; `.basket` is the option's text as read, so the clear still matches
/// and a gone ref cannot pin the basket forever.
#[test]
fn a_gone_ref_does_not_pin_the_basket() {
    let (d, mut a) = compose("gone", true);
    let raw = format!("{}/a.rs:1-2\n{}/gone.rs", d.0.display(), d.0.display());
    a.basket = cockpit_tui::basket::from_body(&raw);
    a.basket_raw = raw.clone();
    assert_eq!(a.basket.len(), 1, "the gone ref is dropped on read");
    ctrl(&mut a, 't');
    key(&mut a, KeyCode::Char('s'));
    assert_eq!(a.sent_basket.as_deref(), Some(raw.as_str()));
}
