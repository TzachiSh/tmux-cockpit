//! #422: find matches across the viewer's own wraps, the hard cut inside a word
//! too wide for a row and the soft break between words, and draws the match on
//! both rows; a real line break still ends a line.
use cockpit_tui::mdview::render::Opts;
use cockpit_tui::mdview::{self, Mode, Screen};
use cockpit_tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

/// 56 columns of body, as `mdview::body_width` gives.
const AREA: Rect = Rect::new(0, 0, 60, 13);

fn screen(src: &str, mode: Mode) -> Screen {
    let mut s = Screen::new("doc", Ok(src.into()), Opts::default()).with_mode(mode);
    s.layout(mdview::body_width(AREA));
    s
}

/// `q` typed into the find field and entered: the hits' (row, from).
fn search(s: &mut Screen, q: &str) -> Vec<(usize, usize)> {
    let mut key = |c| s.key(KeyEvent::new(c, KeyModifiers::NONE), AREA);
    key(KeyCode::Char('/'));
    q.chars().for_each(|c| _ = key(KeyCode::Char(c)));
    key(KeyCode::Enter);
    s.hits.iter().map(|h| (h.row, h.from)).collect()
}

fn buffer(s: &mut Screen) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(AREA.width, AREA.height)).unwrap();
    t.draw(|f| mdview::render(f, f.area(), &Theme::fixed(), s)).unwrap();
    t.backend().buffer().clone()
}

/// The body's rows as drawn, and whether each cell of `tok` on row `y` is reversed.
fn reversed(b: &Buffer, y: u16, tok: &str) -> bool {
    let row: String = (0..b.area.width).map(|x| b[(x, y)].symbol().to_string()).collect();
    let at = row.find(tok).unwrap_or_else(|| panic!("{tok:?} not on row {y}: {row:?}"));
    let x0 = row[..at].chars().count() as u16;
    (x0..x0 + tok.chars().count() as u16).all(|x| b[(x, y)].modifier.contains(Modifier::REVERSED))
}

/// The body starts on the buffer's second row.
const TOP: u16 = 1;

#[test]
fn a_word_cut_by_a_hard_wrap_is_found_once_in_markdown() {
    let mut s = screen(&format!("{}needle", "a".repeat(52)), Mode::Markdown);
    assert_eq!(search(&mut s, "needle"), [(0, 52)]);
    let b = buffer(&mut s);
    assert!(reversed(&b, TOP, "need") && reversed(&b, TOP + 1, "le"));
}

#[test]
fn a_word_cut_by_a_hard_wrap_is_found_once_in_code() {
    let mut s = screen(&format!("{}needle\n", "x".repeat(50)), Mode::Code(None));
    assert_eq!(search(&mut s, "needle"), [(0, 50)]);
    let b = buffer(&mut s);
    assert!(reversed(&b, TOP, "need") && reversed(&b, TOP + 1, "le"));
}

/// The space a soft wrap drops is put back: `find me` is one match.
#[test]
fn words_split_by_a_soft_wrap_are_found() {
    let mut s = screen(&format!("{} find me", "x".repeat(50)), Mode::Markdown);
    assert_eq!(search(&mut s, "find me"), [(0, 51)]);
    let b = buffer(&mut s);
    assert!(reversed(&b, TOP, "find") && reversed(&b, TOP + 1, "me"));
}

/// A wrap inside a quote: the bar the next row starts with is not in the match.
#[test]
fn a_quote_bar_on_the_next_row_is_skipped() {
    let mut s = screen(&format!("> {} find me", "x".repeat(48)), Mode::Markdown);
    assert_eq!(search(&mut s, "find me").len(), 1);
    let b = buffer(&mut s);
    assert!(reversed(&b, TOP + 1, "me") && !reversed(&b, TOP + 1, "│"));
}

/// Rows of two source lines are never joined, in code or in markdown.
#[test]
fn a_real_line_break_still_ends_the_line() {
    assert!(search(&mut screen("need\nle\n", Mode::Code(None)), "needle").is_empty());
    assert!(search(&mut screen("need\n\nle", Mode::Markdown), "needle").is_empty());
}

/// A match over three rows covers the middle row whole.
#[test]
fn a_match_over_three_rows_draws_on_each() {
    let mut s = screen(&"c".repeat(150), Mode::Markdown);
    assert_eq!(search(&mut s, &"c".repeat(120)), [(0, 0)]);
    assert_eq!((s.hits[0].last, s.hits[0].to), (2, 8));
    let b = buffer(&mut s);
    assert!(reversed(&b, TOP + 1, &"c".repeat(56)) && reversed(&b, TOP + 2, "cccccccc"));
}

/// The put-back space at a soft wrap: a match that ends on it stops at the end of
/// the row before, one that starts on it starts on the next row, and a search
/// for a space alone finds only the real ones (#633 maps them by row).
#[test]
fn a_match_at_the_put_back_space_lands_on_a_real_row() {
    let src = format!("{} find me", "x".repeat(50));
    let mut s = screen(&src, Mode::Markdown);
    assert_eq!(search(&mut s, "find "), [(0, 51)]);
    assert_eq!((s.hits[0].last, s.hits[0].to), (0, 55));
    assert_eq!(search(&mut screen(&src, Mode::Markdown), " me"), [(1, 0)]);
    assert_eq!(search(&mut screen(&src, Mode::Markdown), " "), [(0, 50)]);
}
