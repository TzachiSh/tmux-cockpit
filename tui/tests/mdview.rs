//! The markdown viewer's screen (#283): loading, search, scrolling, the keys
//! and the goldens. No tmux and no tty: keys are literals, the theme is
//! `Theme::fixed()`, files are scratch files. COCKPIT_BLESS=1 rewrites goldens.
#[allow(dead_code)] // for its Scratch guard only, not write_exec
mod fake;

use cockpit_tui::mdview::render::{Ink, Opts, Row};
use cockpit_tui::mdview::{self, Hit, MAX_FILE, Out, Screen, find, load};
use cockpit_tui::theme::{Glyphs, Theme};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::path::PathBuf;

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

fn sample() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/view-sample.md")).unwrap()
}

fn draw(w: u16, h: u16, th: &Theme, s: &mut Screen) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| mdview::render(f, f.area(), th, s)).unwrap();
    t.backend().to_string()
}

const AREA: Rect = Rect::new(0, 0, 60, 13);

fn screen(src: &str) -> Screen {
    let mut s = Screen::new("doc.md", Ok(src.into()), Opts::default());
    s.layout(mdview::body_width(AREA));
    s
}

fn key(s: &mut Screen, c: KeyCode) -> Out {
    s.key(KeyEvent::new(c, KeyModifiers::NONE), AREA)
}

fn typed(s: &mut Screen, text: &str) {
    for c in text.chars() {
        key(s, KeyCode::Char(c));
    }
}

/// Paragraphs `p0` to `p<n-1>`, one row each and a blank row between.
fn paras(n: usize) -> String {
    (0..n).map(|i| format!("p{i}\n\n")).collect()
}

/// A scratch file under the crate's own target dir, and the shared guard on the
/// dir that holds it, removed on drop. The guard is held for its drop alone.
struct Doc(PathBuf, #[allow(dead_code)] fake::Scratch);

impl Doc {
    fn new(name: &str, body: &[u8]) -> Self {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let dir = tmp.join(format!("mdview-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        Doc(p, fake::Scratch(dir))
    }
}

#[test]
fn load_reads_text_and_strips_a_bom() {
    let f = Doc::new("a.md", "\u{feff}# hi\n".as_bytes());
    assert_eq!(load(&f.0, MAX_FILE), Ok("# hi\n".into()));
    let e = Doc::new("e.md", b"");
    assert_eq!(load(&e.0, MAX_FILE), Ok(String::new()));
}

#[test]
fn load_refuses_a_file_over_the_cap() {
    let f = Doc::new("big.md", &vec![b'a'; 2049]);
    assert_eq!(load(&f.0, 2048), Err("too large to show: over 2 KB".into()));
    assert!(load(&f.0, 2049).is_ok(), "the cap itself is allowed");
}

#[test]
fn load_refuses_binary_and_invalid_utf8() {
    let b = Doc::new("b.md", b"PK\x03\x04\0\0");
    assert_eq!(load(&b.0, MAX_FILE), Err("a binary file".into()));
    let u = Doc::new("u.md", b"caf\xe9");
    assert_eq!(load(&u.0, MAX_FILE), Err("not UTF-8 text".into()));
}

#[test]
fn load_refuses_what_is_not_a_regular_file_without_waiting() {
    let d = Doc::new("d.md", b"");
    let fifo = d.0.with_file_name("fifo.md");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: a fresh path in our own scratch dir.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert_eq!(load(&fifo, MAX_FILE), Err("not a regular file".into()));
    assert_eq!(load(d.0.parent().unwrap(), MAX_FILE), Err("not a regular file".into()));
    assert!(
        load(&d.0.with_file_name("gone.md"), MAX_FILE)
            .unwrap_err()
            .starts_with("cannot open it")
    );
}

/// One row a line, each its own text.
fn rows(t: &[&str]) -> Vec<Row> {
    let row = |s: &&str| Row {
        cells: vec![(s.to_string(), Ink::default())],
        ..Row::default()
    };
    t.iter().map(row).collect()
}

#[test]
fn find_ignores_case_unless_the_query_has_a_capital() {
    let t = rows(&["Rust rust RUST"]);
    assert_eq!(find(&t, "rust").len(), 3);
    assert_eq!(
        find(&t, "Rust"),
        vec![Hit {
            row: 0,
            from: 0,
            last: 0,
            to: 4
        }]
    );
    assert!(find(&t, "").is_empty());
    assert!(find(&t, "go").is_empty());
}

#[test]
fn find_maps_a_match_back_past_multibyte_text() {
    let t = ["日本 ÉCOLE école".to_string()];
    let hs = find(&rows(&[&t[0]]), "école");
    assert_eq!(hs.len(), 2);
    assert_eq!(&t[0][hs[0].from..hs[0].to], "ÉCOLE");
    assert_eq!(&t[0][hs[1].from..hs[1].to], "école");
}

/// A char that lowers to fewer bytes (the Kelvin sign's 3 to `k`'s 1) still maps
/// a match back onto the row's own bytes (#633 skips the map only when no char
/// changes length).
#[test]
fn find_maps_back_past_a_char_that_lowers_shorter() {
    let t = ["a \u{212A}elvin b".to_string()];
    let hs = find(&rows(&[&t[0]]), "kelvin");
    assert_eq!(hs.len(), 1);
    assert_eq!(&t[0][hs[0].from..hs[0].to], "\u{212A}elvin");
}

/// Lengths that change and cancel out: the Kelvin sign lowers 3 bytes to 1, each
/// Ⱥ 2 bytes to 3, so the folded text is as long as the row. The map is still
/// needed: equal totals are not equal chars (review-534 on #647).
#[test]
fn find_maps_back_when_changed_lengths_cancel_out() {
    let t = ["a \u{212A}ȺȺx b".to_string()];
    let hs = find(&rows(&[&t[0]]), "ⱥx");
    assert_eq!(hs.len(), 1);
    assert_eq!((hs[0].from, hs[0].to), (7, 10));
    assert_eq!(&t[0][hs[0].from..hs[0].to], "Ⱥx");
}

/// A match that starts on a soft wrap's put-back space starts on the next row
/// with text, past a row that is only its prefix (review-534 on #647, as #631).
#[test]
fn a_match_from_a_put_back_space_skips_a_prefix_only_row() {
    use cockpit_tui::mdview::render::Join;
    let mut t = rows(&["foo", "> ", "> bar"]);
    (t[1].join, t[1].body, t[2].join, t[2].body) = (Join::Space, 2, Join::Glued, 2);
    let h = find(&t, " bar");
    assert_eq!((h[0].row, h[0].from, h[0].last, h[0].to), (2, 2, 2, 5));
}

#[test]
fn search_jumps_to_the_first_match_and_n_cycles_round() {
    let mut s = screen(&format!("{}needle\n\n{}needle", paras(20), paras(20)));
    key(&mut s, KeyCode::Char('/'));
    typed(&mut s, "needle");
    key(&mut s, KeyCode::Enter);
    assert_eq!(s.hits.len(), 2);
    let room = mdview::body_rows(AREA);
    let on = |s: &Screen| {
        let r = s.hits[s.cur.unwrap()].row;
        s.top <= r && r < s.top + room
    };
    assert_eq!(s.cur, Some(0));
    assert!(on(&s));
    key(&mut s, KeyCode::Char('n'));
    assert_eq!(s.cur, Some(1));
    assert!(on(&s));
    key(&mut s, KeyCode::Char('n'));
    assert_eq!(s.cur, Some(0), "n past the last goes round");
    key(&mut s, KeyCode::Char('N'));
    assert_eq!(s.cur, Some(1), "N before the first goes round");
}

#[test]
fn the_find_field_takes_every_key_as_text() {
    let mut s = screen("q and n and e");
    key(&mut s, KeyCode::Char('/'));
    for c in ['q', 'e', '?', 'n'] {
        assert_eq!(key(&mut s, KeyCode::Char(c)), Out::Stay);
    }
    assert_eq!(s.field.as_ref().map(|f| f.0.as_str()), Some("qe?n"));
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(s.key(ctrl_c, AREA), Out::Close, "but ^C closes from the field too");
}

#[test]
fn esc_in_the_field_cancels_and_puts_the_view_back() {
    let mut s = screen(&format!("{}needle", paras(30)));
    key(&mut s, KeyCode::Char('/'));
    typed(&mut s, "needle");
    assert!(s.top > 0, "the match scrolled the view");
    key(&mut s, KeyCode::Esc);
    assert_eq!((s.top, s.field.is_none(), s.hits.len()), (0, true, 0));
}

#[test]
fn a_search_with_no_match_says_so() {
    let mut s = screen("abc");
    key(&mut s, KeyCode::Char('/'));
    typed(&mut s, "zzz");
    key(&mut s, KeyCode::Enter);
    assert_eq!(s.note.as_deref(), Some("no match for zzz"));
}

#[test]
fn esc_clears_the_matches_before_it_closes() {
    let mut s = screen("abc");
    key(&mut s, KeyCode::Char('/'));
    typed(&mut s, "b");
    key(&mut s, KeyCode::Enter);
    assert_eq!(key(&mut s, KeyCode::Esc), Out::Stay);
    assert!(s.hits.is_empty());
    assert_eq!(key(&mut s, KeyCode::Esc), Out::Close);
    assert_eq!(key(&mut screen("x"), KeyCode::Char('q')), Out::Close);
}

#[test]
fn scrolling_stops_at_both_ends() {
    let mut s = screen(&paras(30));
    let bottom = s.rows().len() - mdview::body_rows(AREA);
    key(&mut s, KeyCode::Up);
    assert_eq!(s.top, 0);
    key(&mut s, KeyCode::Char('G'));
    assert_eq!(s.top, bottom);
    for c in [KeyCode::Down, KeyCode::Char('j'), KeyCode::PageDown, KeyCode::Char(' ')] {
        key(&mut s, c);
        assert_eq!(s.top, bottom, "{c:?}");
    }
    key(&mut s, KeyCode::Char('g'));
    assert_eq!(s.top, 0);
    key(&mut s, KeyCode::PageDown);
    assert_eq!(s.top, mdview::body_rows(AREA) - 1);
}

#[test]
fn the_wheel_scrolls_three_rows_within_bounds() {
    let mut s = screen(&paras(30));
    let wheel = |s: &mut Screen, k| {
        s.mouse(
            MouseEvent {
                kind: k,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            AREA,
        )
    };
    wheel(&mut s, MouseEventKind::ScrollDown);
    assert_eq!(s.top, 3);
    for _ in 0..50 {
        wheel(&mut s, MouseEventKind::ScrollDown);
    }
    assert_eq!(s.top, s.rows().len() - mdview::body_rows(AREA));
    for _ in 0..50 {
        wheel(&mut s, MouseEventKind::ScrollUp);
    }
    assert_eq!(s.top, 0);
}

#[test]
fn a_short_file_never_scrolls() {
    let mut s = screen("one\n\ntwo");
    for c in [KeyCode::Down, KeyCode::PageDown, KeyCode::Char('G')] {
        key(&mut s, c);
        assert_eq!(s.top, 0, "{c:?}");
    }
}

#[test]
fn e_edits_at_the_top_rows_source_line() {
    let mut s = screen(&paras(30));
    assert_eq!(key(&mut s, KeyCode::Char('e')), Out::Edit(1));
    key(&mut s, KeyCode::Down);
    key(&mut s, KeyCode::Down);
    // row 2 is p1, on line 3
    assert_eq!(key(&mut s, KeyCode::Char('e')), Out::Edit(3));
}

#[test]
fn e_on_a_file_it_cannot_show_edits_at_line_one() {
    let mut s = Screen::new("x.md", Err("a binary file".into()), Opts::default());
    s.layout(40);
    assert_eq!(key(&mut s, KeyCode::Char('e')), Out::Edit(1));
}

#[test]
fn it_opens_at_the_block_holding_a_line() {
    let mut s = Screen::new("x.md", Ok(paras(30)), Opts::default()).at_line(9);
    s.layout(40);
    assert_eq!(s.rows()[s.top].text(), "p4");
}

/// A line inside a block opens at the block, not at the next one.
#[test]
fn a_line_inside_a_paragraph_opens_at_that_paragraph() {
    let src = "# t\n\nalpha one\nbeta two\ngamma three\n\nnext\n\n".to_string() + &paras(30);
    let mut s = Screen::new("x.md", Ok(src), Opts::default()).at_line(4);
    s.layout(40);
    assert_eq!(s.rows()[s.top].text(), "alpha one beta two gamma three");
}

#[test]
fn a_resize_keeps_the_top_rows_source_line() {
    let src: String = (0..30).map(|i| format!("{} {i}\n\n", "word ".repeat(20))).collect();
    let mut s = Screen::new("x.md", Ok(src), Opts::default());
    s.layout(60);
    s.top = 20;
    let line = s.line();
    s.layout(30);
    assert_eq!(s.line(), line);
    assert_ne!(s.top, 20, "the rows did move");
}

#[test]
fn u_hides_and_shows_link_addresses() {
    let mut s = screen("[a](http://x)");
    assert_eq!(s.rows()[0].text(), "a (http://x)");
    key(&mut s, KeyCode::Char('u'));
    assert_eq!(s.rows()[0].text(), "a");
    key(&mut s, KeyCode::Char('u'));
    assert_eq!(s.rows()[0].text(), "a (http://x)");
}

#[test]
fn the_help_opens_on_h_and_any_other_key_closes_it() {
    let mut s = screen("x");
    key(&mut s, KeyCode::Char('h'));
    assert_eq!(s.help, Some(0));
    assert_eq!(key(&mut s, KeyCode::Char('q')), Out::Stay, "q closes the help, not the viewer");
    assert_eq!(s.help, None);
}

#[test]
fn a_file_name_is_cleaned_before_it_is_drawn() {
    let s = Screen::new("a\x1b]0;x\x07.md", Ok(String::new()), Opts::default());
    assert_eq!(s.name, "a]0;x.md");
}

fn bless_set(name: &str, w: u16, h: u16, th: &Theme, o: Opts) {
    let mut s = Screen::new("view-sample.md", Ok(sample()), o);
    golden(name, &draw(w, h, th, &mut s));
}

#[test]
fn goldens_at_40_80_and_120() {
    for (w, h) in [(40, 80), (80, 64), (120, 60)] {
        bless_set(&format!("view-{w}"), w, h, &Theme::fixed(), Opts::default());
    }
}

#[test]
fn goldens_in_the_ascii_tier_and_without_colour() {
    let ascii = Theme {
        g: Glyphs::ascii(),
        ..Theme::fixed()
    };
    let o = Opts {
        ascii: true,
        ..Opts::default()
    };
    bless_set("view-ascii-80", 80, 64, &ascii, o);
    let plain = Opts {
        colour: false,
        ..Opts::default()
    };
    bless_set("view-nocolor-80", 80, 64, &Theme::no_color(), plain);
}

#[test]
fn goldens_of_the_scrolled_view_the_field_the_note_and_the_help() {
    let th = Theme::fixed();
    let mut s = Screen::new("view-sample.md", Ok(sample()), Opts::default());
    let a = Rect::new(0, 0, 80, 20);
    draw(80, 20, &th, &mut s);
    s.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), a);
    for c in "quote".chars() {
        s.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), a);
    }
    golden("view-find-80", &draw(80, 20, &th, &mut s));
    let mut n = Screen::new("bin.md", Err("a binary file".into()), Opts::default());
    golden("view-note-60", &draw(60, 10, &th, &mut n));
    let mut h = Screen::new("x.md", Ok(String::new()), Opts::default());
    h.help = Some(0);
    golden("view-help-60", &draw(60, 20, &th, &mut h));
    let mut e = Screen::new("empty.md", Ok(String::new()), Opts::default());
    golden("view-empty-60", &draw(60, 8, &th, &mut e));
}

#[test]
fn under_the_floor_it_shows_the_card_and_q_still_closes() {
    let th = Theme::fixed();
    let mut s = Screen::new("x.md", Ok("hi".into()), Opts::default());
    golden("view-floor", &draw(20, 5, &th, &mut s));
    let tiny = Rect::new(0, 0, 20, 5);
    assert_eq!(s.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), tiny), Out::Stay);
    assert_eq!(s.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE), tiny), Out::Close);
}

fn buffer(w: u16, h: u16, th: &Theme, s: &mut Screen) -> ratatui::buffer::Buffer {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| mdview::render(f, f.area(), th, s)).unwrap();
    t.backend().buffer().clone()
}

/// The modifiers on the first cell of `tok` in the body.
fn mods(b: &ratatui::buffer::Buffer, tok: &str) -> ratatui::style::Modifier {
    for y in 1..b.area.height - 2 {
        let row: String = (0..b.area.width).map(|x| b[(x, y)].symbol().to_string()).collect();
        if let Some(i) = row.find(tok) {
            let x = row[..i].chars().count() as u16;
            return b[(x, y)].modifier;
        }
    }
    panic!("{tok:?} is not drawn")
}

#[test]
fn matches_draw_reversed_and_the_current_one_bold_too() {
    use ratatui::style::Modifier;
    let th = Theme::fixed();
    let mut s = screen("alpha beta alpha");
    key(&mut s, KeyCode::Char('/'));
    typed(&mut s, "alpha");
    let b = buffer(60, 13, &th, &mut s);
    assert!(mods(&b, "alpha").contains(Modifier::REVERSED | Modifier::BOLD));
    assert!(!mods(&b, "beta").contains(Modifier::REVERSED));
    assert!(mods(&b, "a beta").contains(Modifier::REVERSED), "the first match's last letter");
}

#[test]
fn two_matches_on_one_row_are_both_reversed() {
    use ratatui::style::Modifier;
    let th = Theme::fixed();
    let mut s = screen("foo foo");
    key(&mut s, KeyCode::Char('/'));
    typed(&mut s, "foo");
    let b = buffer(60, 13, &th, &mut s);
    let row: Vec<Modifier> = (2..9).map(|x| b[(x, 1)].modifier).collect();
    let rev: Vec<bool> = row.iter().map(|m| m.contains(Modifier::REVERSED)).collect();
    assert_eq!(rev, [true, true, true, false, true, true, true], "{row:?}");
    assert!(
        row[0].contains(Modifier::BOLD) && !row[4].contains(Modifier::BOLD),
        "only the current one is bold"
    );
}

#[test]
fn without_colour_no_cell_has_one() {
    use ratatui::style::Color;
    let mut s = Screen::new(
        "x.md",
        Ok(sample()),
        Opts {
            colour: false,
            ..Opts::default()
        },
    );
    let b = buffer(80, 64, &Theme::no_color(), &mut s);
    assert!(b.content.iter().all(|c| c.fg == Color::Reset && c.bg == Color::Reset));
}

/// The name gives way to the row counter, however wide the counter is.
#[test]
fn a_long_name_leaves_room_for_a_wide_counter() {
    let mut s = Screen::new(&"n".repeat(80), Ok(paras(50_001)), Opts::default());
    let th = Theme::fixed();
    draw(60, 20, &th, &mut s);
    s.key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE), Rect::new(0, 0, 60, 20));
    let top = draw(60, 20, &th, &mut s).lines().next().unwrap().to_string();
    assert!(top.contains(" 99985-100001 of 100001 ╮"), "{top}");
    assert!(
        top.contains("…") && top.contains("n ─ 99985"),
        "the name is cut, a rule before the counter: {top}"
    );
}
