//! The markdown viewer's layout (#283): one element per test, asserted on the
//! rows' cells, and the property that no markdown panics it or draws a row
//! wider than its width. Pure: no tmux, no tty, no theme.
use cockpit_tui::highlight::Role;
use cockpit_tui::mdview::render::{Ink, Kind, Opts, Row, cols, render};

fn rows(src: &str, w: usize) -> Vec<Row> {
    render(src, w, Opts::default())
}

fn texts(src: &str, w: usize) -> Vec<String> {
    rows(src, w).iter().map(Row::text).collect()
}

/// The ink of `tok`'s first cell-aligned match in the rows.
fn ink_of(rs: &[Row], tok: &str) -> Ink {
    rs.iter()
        .flat_map(|r| r.cells.iter())
        .find(|(t, _)| t.contains(tok))
        .map(|(_, i)| *i)
        .unwrap_or_else(|| panic!("{tok:?} is in no cell of {rs:?}"))
}

#[test]
fn empty_and_blank_input_give_no_rows() {
    for src in ["", "\n\n", "   \n\t\n"] {
        assert!(rows(src, 40).is_empty(), "{src:?}");
    }
}

#[test]
fn headings_carry_their_level_and_their_marks() {
    for n in 1..=6u8 {
        let src = format!("{} Title", "#".repeat(n.into()));
        let rs = rows(&src, 40);
        assert_eq!(rs[0].text(), src, "level {n}");
        let t = ink_of(&rs, "Title");
        assert_eq!((t.kind, t.bold), (Kind::Heading(n), true), "level {n}");
        assert_eq!(ink_of(&rs, "#").kind, Kind::Meta, "level {n}");
    }
}

#[test]
fn emphasis_strong_and_strikethrough_set_their_flags() {
    let rs = rows("*i* **b** ~~s~~ ***ib***", 40);
    assert!(ink_of(&rs, "i").italic);
    assert!(ink_of(&rs, "b").bold);
    assert!(ink_of(&rs, "s").strike);
    let ib = ink_of(&rs, "ib");
    assert!(ib.bold && ib.italic);
}

#[test]
fn inline_code_is_its_own_kind_and_keeps_backticks_without_colour() {
    assert_eq!(ink_of(&rows("a `x y` b", 40), "x y").kind, Kind::Inline);
    let plain = Opts {
        colour: false,
        ..Opts::default()
    };
    assert_eq!(render("a `x y` b", 40, plain)[0].text(), "a `x y` b");
}

#[test]
fn a_link_is_its_text_then_its_dimmed_url() {
    let rs = rows("see [docs](https://e.com/d) now", 60);
    assert_eq!(rs[0].text(), "see docs (https://e.com/d) now");
    assert_eq!(ink_of(&rs, "docs").kind, Kind::Link);
    assert_eq!(ink_of(&rs, "https").kind, Kind::Url);
}

#[test]
fn a_url_is_left_out_when_hidden_for_an_autolink_and_for_an_anchor() {
    let off = Opts {
        urls: false,
        ..Opts::default()
    };
    assert_eq!(render("[docs](https://e.com)", 40, off)[0].text(), "docs");
    assert_eq!(texts("<https://e.com>", 40), ["https://e.com"]);
    assert_eq!(texts("[up](#top)", 40), ["up"]);
}

#[test]
fn an_image_is_its_label_and_address() {
    let rs = rows("![a cat](cat.png)", 40);
    assert_eq!(rs[0].text(), "[image: a cat] (cat.png)");
    assert_eq!(ink_of(&rs, "a cat").kind, Kind::Meta);
}

#[test]
fn bullets_change_with_depth_in_both_tiers() {
    let src = "- a\n  - b\n    - c\n      - d";
    assert_eq!(texts(src, 40), ["• a", "  ◦ b", "    ▪ c", "      • d"]);
    let ascii = Opts {
        ascii: true,
        ..Opts::default()
    };
    let got: Vec<String> = render(src, 40, ascii).iter().map(Row::text).collect();
    assert_eq!(got, ["- a", "  * b", "    + c", "      - d"]);
    assert_eq!(ink_of(&rows(src, 40), "•").kind, Kind::Bullet);
}

#[test]
fn an_ordered_list_counts_from_its_start() {
    assert_eq!(texts("3. a\n3. b\n3. c", 40), ["3. a", "4. b", "5. c"]);
}

#[test]
fn task_boxes_replace_the_bullet() {
    assert_eq!(texts("- [ ] todo\n- [x] done", 40), ["[ ] todo", "[✓] done"]);
    let ascii = Opts {
        ascii: true,
        ..Opts::default()
    };
    assert_eq!(render("- [x] done", 40, ascii)[0].text(), "[x] done");
}

#[test]
fn a_wrapped_item_hangs_under_its_text() {
    assert_eq!(texts("- aaaa bbbb cccc", 12), ["• aaaa bbbb", "  cccc"]);
}

#[test]
fn a_loose_list_has_a_blank_row_between_items_without_a_bullet() {
    assert_eq!(texts("- a\n\n- b", 40), ["• a", "", "• b"]);
}

#[test]
fn quotes_draw_a_bar_per_level_and_their_own_kind() {
    assert_eq!(texts("> a\n>\n> > b", 40), ["│ a", "│", "│ │ b"]);
    let rs = rows("> a", 40);
    assert_eq!(ink_of(&rs, "│").kind, Kind::Frame);
    assert_eq!(ink_of(&rs, "a").kind, Kind::Quote);
    let ascii = Opts {
        ascii: true,
        ..Opts::default()
    };
    assert_eq!(render("> a", 40, ascii)[0].text(), "| a");
}

#[test]
fn a_gap_before_a_quote_is_outside_it() {
    assert_eq!(texts("p\n\n> q", 40), ["p", "", "│ q"]);
}

#[test]
fn a_code_block_is_lexed_by_its_tag_and_indented() {
    let rs = rows("```rust\nfn x() {}\n```", 40);
    assert_eq!(rs[0].text(), "    fn x() {}");
    assert_eq!(ink_of(&rs, "fn").kind, Kind::Code(Role::Keyword));
}

#[test]
fn a_code_tab_goes_out_to_the_next_stop() {
    assert_eq!(texts("```\na\tb\n\tc\n```", 40), ["    a   b", "        c"]);
}

#[test]
fn code_rows_carry_their_own_source_lines() {
    let rs = rows("text\n\n```sh\none\ntwo\n```", 40);
    let lines: Vec<usize> = rs.iter().map(|r| r.line).collect();
    // the blank row between carries the line above it
    assert_eq!(lines, [1, 1, 4, 5]);
}

#[test]
fn a_long_code_line_wraps_hard_and_keeps_its_spaces() {
    assert_eq!(texts("```\nab  cd  ef\n```", 10), ["    ab  cd", "      ef"]);
}

/// A terminal escape in the file would reach the terminal: it is dropped,
/// in prose, in code and in a link's address.
#[test]
fn control_characters_are_dropped_everywhere() {
    let src = "a\x1b[2Jb\n\n```\nc\x1b]0;x\x07d\n```\n\n[l](u\x1bv)";
    for r in rows(src, 40) {
        assert!(!r.text().chars().any(char::is_control), "{:?}", r.text());
    }
}

#[test]
fn a_table_has_a_bold_head_a_rule_and_aligned_columns() {
    let src = "| a | b | c |\n|:--|:-:|--:|\n| x | y | z |\n| long | m | n |";
    assert_eq!(texts(src, 40), ["a    │ b │ c", "─────┼───┼──", "x    │ y │ z", "long │ m │ n"]);
    let rs = rows(src, 40);
    assert!(ink_of(&rs, "a").bold);
    assert!(!ink_of(&rs, "long").bold);
    assert_eq!(texts("| a | b |\n|--|--:|\n| x | yy |", 40)[2], "x │ yy");
    assert_eq!(texts("| a | bb |\n|--|--:|\n| xx | y |", 40)[2], "xx │  y");
}

#[test]
fn a_table_too_wide_shares_the_room_and_wraps_its_cells() {
    let src = "| h | text |\n|--|--|\n| a | one two three four |";
    let got = texts(src, 14);
    assert!(got.iter().all(|t| cols(t) <= 14), "{got:?}");
    assert_eq!(got[2], "a │ one two");
}

#[test]
fn a_table_with_no_room_for_its_columns_is_header_value_lines() {
    let src = "| a | b | c | d |\n|--|--|--|--|\n| 1 | 2 | 3 | 4 |";
    assert_eq!(texts(src, 20), ["a: 1", "b: 2", "c: 3", "d: 4"]);
}

#[test]
fn a_rule_spans_the_width() {
    let rs = rows("---", 20);
    assert_eq!(rs[0].text(), "─".repeat(20));
    assert_eq!(rs[0].cells[0].1.kind, Kind::Frame);
}

#[test]
fn a_hard_break_breaks_and_a_soft_one_is_a_space() {
    assert_eq!(texts("a  \nb\nc", 40), ["a", "b c"]);
}

#[test]
fn html_is_shown_as_written_never_read() {
    let rs = rows("<div>\n<b>x</b>\n</div>\n\nan <i>y</i>", 40);
    assert_eq!(rs[0].text(), "<div>");
    assert_eq!(ink_of(&rs, "<b>").kind, Kind::Meta);
    assert_eq!(ink_of(&rs, "<i>").kind, Kind::Meta);
}

#[test]
fn a_paragraph_carries_its_first_source_line() {
    let rs = rows("# t\n\none two three\nfour five\n\nsix", 8);
    let got: Vec<(String, usize)> = rs.iter().map(|r| (r.text(), r.line)).collect();
    assert_eq!(got[2], ("one two".into(), 3));
    assert_eq!(got.last().unwrap(), &("six".into(), 6));
}

#[test]
fn deep_nesting_gives_up_indent_before_the_text_gives_up_room() {
    let list: String = (0..40).map(|i| format!("{}- x\n", "  ".repeat(i))).collect();
    let rs = rows(&list, 20);
    assert!(rs.iter().all(|r| r.width() <= 20));
    assert_eq!(rs.iter().filter(|r| r.text().ends_with('x')).count(), 40);
    // forty bars clamp to half the width, five of them
    assert_eq!(texts(&format!("{} deep words", ">".repeat(40)), 20), ["│ │ │ │ │ deep words"]);
}

#[test]
fn wide_graphemes_wrap_by_their_drawn_width() {
    let got = texts("日本語日本語 👍🏽👍🏽👍🏽 ❤\u{fe0f}❤\u{fe0f}", 8);
    assert!(got.iter().all(|t| cols(t) <= 8), "{got:?}");
    assert_eq!(got[0], "日本語日");
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Random markdown from element atoms never panics, and at every width from
/// 20 to 200 no row is wider than the width.
#[test]
fn random_markdown_never_panics_or_overruns_the_width() {
    let atoms = [
        "# ",
        "## ",
        "\n",
        "\n\n",
        "- ",
        "  - ",
        "1. ",
        "> ",
        "> > ",
        "- [ ] ",
        "- [x] ",
        "*",
        "**",
        "~~",
        "`",
        "```rust\n",
        "```\n",
        "    ",
        "\t",
        "|",
        "| a | b |\n|--|--:|\n",
        "---\n",
        "[l](http://x)",
        "![i](p)",
        "<b>",
        "</b>",
        "<div>\n",
        "日本語",
        "👍🏽",
        "❤\u{fe0f}",
        "word ",
        "longwordlongwordlongword",
        "\\",
        "  \n",
        "\u{1b}[31m",
        "&amp;",
        "\r\n",
    ];
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for _ in 0..1500 {
        let n = (rng.next() % 40) as usize;
        let src: String = (0..n).map(|_| atoms[(rng.next() % atoms.len() as u64) as usize]).collect();
        let w = 20 + (rng.next() % 181) as usize;
        for o in [
            Opts::default(),
            Opts {
                ascii: true,
                urls: false,
                colour: false,
            },
        ] {
            for r in render(&src, w, o) {
                assert!(r.width() <= w, "{w}: {:?} from {src:?}", r.text());
            }
        }
    }
}

/// Random bytes, read as text, never panic either.
#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..1500 {
        let n = (rng.next() % 200) as usize;
        let b: Vec<u8> = (0..n).map(|_| (rng.next() % 128) as u8).collect();
        let src = String::from_utf8_lossy(&b);
        let w = 20 + (rng.next() % 181) as usize;
        for r in rows(&src, w) {
            assert!(r.width() <= w);
        }
    }
}
