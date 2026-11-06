//! action::sanitize (review-510 on #575): what reaches a pane drops control
//! characters, and now bidi controls and line separators too, on every send,
//! an answer's `<prompt>` included. One row per character.
use cockpit_tui::panel::action::sanitize;

#[test]
fn bidi_controls_and_line_separators_never_reach_the_pane() {
    let rows = [
        ("U+202E", '\u{202e}'),
        ("U+202A", '\u{202a}'),
        ("U+200E", '\u{200e}'),
        ("U+200F", '\u{200f}'),
        ("U+2066", '\u{2066}'),
        ("U+2069", '\u{2069}'),
        ("U+061C", '\u{61c}'),
        ("U+2028", '\u{2028}'),
        ("U+2029", '\u{2029}'),
    ];
    for (row, c) in rows {
        assert_eq!(sanitize(&format!("a{c}b\n")), "ab\n", "{row}");
    }
}

#[test]
fn tabs_newlines_and_plain_text_stay() {
    assert_eq!(sanitize("a\tb\r\nc é 漢\n"), "a\tb\nc é 漢\n");
}
