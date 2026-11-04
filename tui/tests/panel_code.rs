//! #282: code in the panel's Result, Steps and Answer views is coloured, fenced
//! blocks only, by the same rows the plain text wraps to.
//! COCKPIT_BLESS=1 cargo test rewrites the goldens.
use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::view::render;
use cockpit_tui::theme::{Glyphs, Theme};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

fn golden(name: &str, got: &str) {
    let path = format!("{}/tests/golden/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("COCKPIT_BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path}; run COCKPIT_BLESS=1 cargo test"));
    assert_eq!(want, got, "\n{name} changed:\n--- want ---\n{want}\n--- got ---\n{got}");
}

fn buffer(a: &App, w: u16, h: u16, th: &Theme) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| {
        render(f, f.area(), a, th, 0);
    })
    .unwrap();
    t.backend().buffer().clone()
}

fn text(b: &Buffer) -> String {
    let (w, h) = (b.area.width, b.area.height);
    (0..h)
        .map(|y| (0..w).map(|x| b[(x, y)].symbol()).collect::<String>() + "\n")
        .collect()
}

/// The frame as role letters: K keyword (accent), S string (done), C comment
/// or fence (meta), N number (running), `.` other text, blank for a space.
fn roles(b: &Buffer, th: &Theme) -> String {
    let (w, h) = (b.area.width, b.area.height);
    let letter = |x, y| {
        let c = &b[(x, y)];
        let fg = c.fg;
        match () {
            _ if c.symbol() == " " => ' ',
            _ if fg == th.accent => 'K',
            _ if fg == th.done => 'S',
            _ if fg == th.meta => 'C',
            _ if fg == th.running => 'N',
            _ => '.',
        }
    };
    (0..h).map(|y| (0..w).map(|x| letter(x, y)).collect::<String>() + "\n").collect()
}

/// The cell where `needle` starts, searched row by row.
fn at(b: &Buffer, needle: &str) -> (u16, u16) {
    for y in 0..b.area.height {
        let row: Vec<String> = (0..b.area.width).map(|x| b[(x, y)].symbol().to_string()).collect();
        if let Some(x) = (0..row.len()).find(|&x| row[x..].concat().starts_with(needle)) {
            return (x as u16, y);
        }
    }
    panic!("{needle:?} is not on screen:\n{}", text(b));
}

fn fg(b: &Buffer, needle: &str) -> Color {
    let (x, y) = at(b, needle);
    b[(x, y)].fg
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

fn ch(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

const ANSWER: &str = "If the file is there, the loop runs once:\n\
                      ```bash\n\
                      for f in *.log; do\n\
                      \tgrep -c \"error: $f\" \"$f\" # count them\n\
                      done\n\
                      ```\n\
                      Then 3 lines";

/// An explain answer whose reply carries a fenced block.
fn answered(reply: &str) -> App {
    let mut a = app("work", "bash", "$ ls\nbuild.log\n$ ");
    ch(&mut a, '3');
    ch(&mut a, '2');
    a.reply(Ok(reply.into()));
    assert!(matches!(a.view, View::Answer { .. }), "{:?}", a.view);
    a
}

#[test]
fn golden_code_answer() {
    let a = answered(ANSWER);
    let th = Theme::fixed();
    for (w, h) in [(40, 12), (60, 16), (80, 20), (120, 24)] {
        golden(&format!("panel-code-{w}"), &text(&buffer(&a, w, h, &th)));
    }
    golden("panel-code-roles-80", &roles(&buffer(&a, 80, 20, &th), &th));
}

#[test]
fn a_fenced_block_is_coloured_and_prose_is_not() {
    let th = Theme::fixed();
    let b = buffer(&answered(ANSWER), 80, 20, &th);
    assert_eq!(fg(&b, "for f"), th.accent, "a shell keyword");
    assert_eq!(fg(&b, "\"error"), th.done, "a string");
    assert_eq!(fg(&b, "# count"), th.meta, "a comment");
    assert_eq!(fg(&b, "```bash"), th.meta, "the fence line");
    assert_eq!(fg(&b, "If the"), th.value, "prose stays value");
    assert_eq!(fg(&b, "3 lines"), th.value, "a number in prose is not one");
}

#[test]
fn no_color_draws_the_same_text_with_no_colour() {
    let a = answered(ANSWER);
    let b = buffer(&a, 80, 20, &Theme::no_color());
    assert_eq!(text(&b), text(&buffer(&a, 80, 20, &Theme::fixed())));
    let (x, y) = at(&b, "for f");
    for x in x..b.area.width {
        assert_eq!(b[(x, y)].fg, Color::Reset, "column {x} of the code row");
    }
    assert_eq!(b[(x, y)].modifier, b[at(&b, "If the")].modifier, "no modifier stands in for colour");
}

#[test]
fn the_ascii_tier_colours_the_same_cells() {
    let a = answered(ANSWER);
    let th = Theme::fixed();
    let ascii = Theme {
        g: Glyphs::ascii(),
        ..Theme::fixed()
    };
    let (u, s) = (buffer(&a, 80, 20, &th), buffer(&a, 80, 20, &ascii));
    let row = |b: &Buffer, th: &Theme| roles(b, th).lines().nth(at(b, "for f").1 as usize).unwrap().to_string();
    assert_eq!(row(&u, &th), row(&s, &ascii));
}

#[test]
fn a_wrapped_string_keeps_its_colour_on_the_next_row() {
    let th = Theme::fixed();
    // prose first: a reply fenced whole is unfenced before the view sees it
    let a = answered("Try:\n```py\nx = \"one two three four five six seven eight nine\"\n```");
    let b = buffer(&a, 40, 12, &th);
    let (_, y) = at(&b, "x = ");
    let (x2, y2) = at(&b, "nine");
    assert!(y2 > y, "the string wraps:\n{}", text(&b));
    assert_eq!(b[(x2, y2)].fg, th.done, "the second row is still the string:\n{}", roles(&b, &th));
    assert_eq!(fg(&b, "x = "), th.value);
}

#[test]
fn a_result_colours_its_fence() {
    let mut a = app("py", "python3", ">>> ");
    let i = a
        .tiles()
        .iter()
        .position(|t| t.0 == cockpit_tui::panel::app::Tile::Compose)
        .unwrap();
    ch(&mut a, char::from(b'1' + i as u8));
    for c in "two lines".chars() {
        ch(&mut a, c);
    }
    a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    a.reply(Ok("<text>Like so:\n```rust\nlet n = 42;\n```</text>".into()));
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
    let th = Theme::fixed();
    let b = buffer(&a, 60, 16, &th);
    assert_eq!(fg(&b, "let n"), th.accent);
    assert_eq!(fg(&b, "42"), th.running);
    assert_eq!(fg(&b, "Like so"), th.value);
}

#[test]
fn a_step_command_is_lexed_by_its_lang() {
    let mut a = app("work", "bash", "$ ");
    let i = a
        .tiles()
        .iter()
        .position(|t| t.0 == cockpit_tui::panel::app::Tile::Compose)
        .unwrap();
    ch(&mut a, char::from(b'1' + i as u8));
    for c in "grep it".chars() {
        ch(&mut a, c);
    }
    a.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    a.reply(Ok(
        "<steps>\n<step><note>Count</note><cmd lang=\"bash\">grep -c \"x y\" f</cmd></step>\n\
                <step><note>Show</note><cmd lang=\"text\">cat \"a b\"</cmd></step>\n</steps>"
            .into(),
    ));
    assert!(matches!(a.view, View::Steps { .. }), "{:?}", a.view);
    let th = Theme::fixed();
    let b = buffer(&a, 80, 20, &th);
    assert_eq!(fg(&b, "\"x y\""), th.done, "a bash step's string");
    assert_eq!(fg(&b, "grep -c"), th.value, "a command word stays value");
    assert_eq!(fg(&b, "\"a b\""), th.value, "an unknown lang is plain");
}

// ------------------------------------------- an answer that is one fence

fn answer_text(out: &str) -> String {
    match cockpit_tui::panel::parse::explain(out) {
        cockpit_tui::panel::parse::Reply::Answer { text, .. } => text,
        r => panic!("not an answer: {r:?}"),
    }
}

/// The view is display-only, so a block in a language the lexer knows keeps
/// its fence and tag; a wrapper fence (bare, xml) still goes.
#[test]
fn an_answer_fenced_whole_in_a_language_keeps_its_fence() {
    assert_eq!(answer_text("```rust\nfn main() {}\n```"), "```rust\nfn main() {}\n```");
    assert_eq!(answer_text("```Py\nx = 1\n```"), "```Py\nx = 1\n```");
    assert_eq!(answer_text("```\nplain words\n```"), "plain words");
    assert_eq!(answer_text("```xml\nhello\n```"), "hello");
}

#[test]
fn a_fence_that_wraps_a_prompt_still_goes() {
    let r = cockpit_tui::panel::parse::explain("```bash\nRun it.\n<prompt>do it</prompt>\n```");
    let cockpit_tui::panel::parse::Reply::Answer { text, ready } = r else {
        panic!("{r:?}")
    };
    assert_eq!((text.as_str(), ready.as_deref()), ("Run it.", Some("do it")));
}

#[test]
fn an_answer_fenced_whole_is_coloured() {
    let th = Theme::fixed();
    let b = buffer(&answered("```rust\nlet n = 42;\n```"), 60, 16, &th);
    assert_eq!(fg(&b, "let n"), th.accent);
    assert_eq!(fg(&b, "```rust"), th.meta);
}
