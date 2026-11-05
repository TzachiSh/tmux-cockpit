//! The board's TestBackend goldens and the contracts a golden cannot hold.
//! No tmux, no clock, no env: every fixture is a literal and every theme is
//! built by hand, so a failure means the layout moved and nothing else.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
use cockpit_tui::board::{self, Act, EXTRAS, Focus, Grid, KEYS, Nav, Row, Sel, View};
use cockpit_tui::data::Pane;
use cockpit_tui::theme::{Glyphs, Theme};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use std::collections::{BTreeSet, HashSet};

fn golden(name: &str, got: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    let path = format!("{dir}/{name}.txt");
    if std::env::var("COCKPIT_BLESS").as_deref() == Ok("1") {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path}; run COCKPIT_BLESS=1 cargo test"));
    assert_eq!(want, got, "\n{name} changed:\n--- want ---\n{want}\n--- got ---\n{got}");
}

fn draw(w: u16, h: u16, f: impl FnOnce(&mut ratatui::Frame)) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(f).unwrap();
    t.backend().to_string()
}

/// TestBackend's Display quotes every row, which is two columns a width check
/// must not count.
fn rows(screen: &str) -> Vec<&str> {
    screen
        .lines()
        .map(|l| l.strip_prefix('"').and_then(|l| l.strip_suffix('"')).unwrap_or(l))
        .collect()
}

/// The same render, kept as cells, so a test can assert a colour or a modifier.
fn render_to_buffer(w: u16, h: u16, f: impl FnOnce(&mut ratatui::Frame)) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(f).unwrap();
    t.backend().buffer().clone()
}

fn pane(id: &str, wher: &str, title: &str, state: &str) -> Pane {
    Pane {
        id: id.into(),
        wher: wher.into(),
        title: title.into(),
        state: state.into(),
        cmd: "claude".into(),
        ..Default::default()
    }
}

/// ux.md §1.7-1.9's five agents, in tmux's own (unsorted) order.
fn fixture() -> Vec<Pane> {
    let mut a = pane("%9", "henosis:1", "Openvpn setup", "needs-input");
    a.msg = "permission: Bash(sudo openvpn --config /etc/openvpn/work.conf --daemon --log /var/log/ovpn.log)".into();
    a.since = Some(252);

    let mut b = pane("%14", "henosis:3", "Implement with test review and rig", "running");
    b.doing = "Bash: shellcheck on cockpit.tmux and tests/run.sh".into();
    b.said = "While the suite and rig run: shellcheck on the two scripts, and the independent review pass on the diff.".into();
    b.since = Some(32);
    b.review = "pending".into();

    let mut c = pane("%21", "work:2", "Fleet gate review", "running");
    c.doing = "Agent: whole-branch review of phase 1".into();
    c.said = "Dispatched the review agent; waiting on its report before the merge.".into();
    c.since = Some(252);

    let mut d = pane("%30", "work:4", "Left click issue", "done");
    d.doing = "idle · handover pending".into();
    d.said = "Pushed PR #28 and left the branch on master.".into();
    d.since = Some(720);
    d.review = "sent".into();

    let e = pane("%31", "work:5", "", "");

    vec![a, b, c, d, e]
}

fn previews() -> Vec<Option<String>> {
    [
        "Bash command\nsudo openvpn --config /etc/openvpn/work.conf --daemon\nDo you want to proceed?\n❯ 1. Yes   2. Yes, don't ask again   3. No",
        "$ make check\n1001 assertions, 0 failures\n$ shellcheck cockpit.tmux tests/run.sh",
        "⏺ Agent (fleet-gate-review)\n  Read tests/fleet.sh, 214 lines\n  Read scripts/agent-state, 412 lines",
        "  ✓ pushed to origin/fix/left-click\n  PR #28 opened\n❯",
        "❯",
    ]
    .iter()
    .map(|s| Some((*s).to_string()))
    .collect()
}

fn view<'a>(ps: &'a [Pane], pvs: &'a [Option<String>], th: &'a Theme, sel: usize, top: usize) -> View<'a> {
    View {
        panes: ps,
        pvs,
        th,
        sel,
        top,
        active: "%14",
        note: None,
        help: None,
        stale: None,
        lost: false,
        never: false,
        list: None,
        confirm: None,
    }
}

/// The widths step 1's goldens pin: a phone in landscape, the common popup,
/// a wide monitor.
const SIZES: [(u16, u16); 3] = [(60, 24), (100, 30), (180, 40)];

fn board_at(w: u16, h: u16, top: usize, th: &Theme) -> String {
    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, th, 1, top);
    draw(w, h, |f| board::render(f, f.area(), &v))
}

// ---------------------------------------------------------------- the grid

#[test]
fn board_80x24_one_column() {
    golden("board-80", &board_at(80, 24, 0, &Theme::fixed()));
}

/// Scrolled to the last row: the tile keeps its size and the header flips to
/// `5-5 of 5  ▲4` (ux.md §1.7's second mockup).
#[test]
fn board_80x24_scrolled_to_last_row() {
    golden("board-80-scrolled", &board_at(80, 24, 4, &Theme::fixed()));
}

#[test]
fn board_120x30_two_columns() {
    golden("board-120", &board_at(120, 30, 0, &Theme::fixed()));
}

#[test]
fn board_160x24_three_columns() {
    golden("board-160", &board_at(160, 24, 0, &Theme::fixed()));
}

/// ux.md §1.5's ladder, at the widths its own mockups were generated at.
#[test]
fn grid_ladder() {
    let g = |w: u16, h: u16, n: usize| board::grid(ratatui::layout::Rect::new(0, 0, w, h), n);
    assert_eq!(
        g(80, 24, 5),
        Grid {
            cols: 1,
            rows: 5,
            rows_shown: 4,
            full: false
        }
    );
    assert_eq!(
        g(120, 30, 5),
        Grid {
            cols: 2,
            rows: 3,
            rows_shown: 3,
            full: true
        }
    );
    assert_eq!(
        g(160, 24, 5),
        Grid {
            cols: 3,
            rows: 2,
            rows_shown: 2,
            full: true
        }
    );
    assert_eq!(
        g(160, 24, 9),
        Grid {
            cols: 3,
            rows: 3,
            rows_shown: 3,
            full: true
        }
    );
    assert_eq!(
        g(160, 24, 16),
        Grid {
            cols: 3,
            rows: 6,
            rows_shown: 4,
            full: false
        }
    );
    // never leave a whole empty column: two agents on a 160 board get two tiles
    assert_eq!(g(160, 24, 2).cols, 2);
    assert_eq!(g(160, 24, 1).cols, 1);
    // the cap only bites above 192 columns
    assert_eq!(g(400, 24, 9).cols, 3);
}

/// (state rank, pane id), and the id sorts numerically: %9 before %14.
#[test]
fn sorts_needs_first_then_by_pane_id() {
    let mut ps = fixture();
    ps.reverse();
    let ord: Vec<&str> = board::order(&ps).iter().map(|&i| ps[i].id.as_str()).collect();
    assert_eq!(ord, ["%9", "%14", "%21", "%30", "%31"]);

    let mut err = pane("%3", "w:1", "boom", "error");
    err.doing = "x".into();
    ps.push(err);
    let ord: Vec<&str> = board::order(&ps).iter().map(|&i| ps[i].id.as_str()).collect();
    assert_eq!(ord, ["%9", "%3", "%14", "%21", "%30", "%31"]);
}

// ------------------------------------------------------------- needs-input

#[test]
fn needs_input_tile() {
    let ps = fixture();
    let pvs = previews();
    let th = Theme::fixed();
    let one = [ps[0].clone()];
    let pv = [pvs[0].clone()];
    let v = view(&one, &pv, &th, 0, 0);
    golden("board-needs", &draw(72, 10, |f| board::render(f, f.area(), &v)));
}

/// Red border, the question in the header, and no key hint on the frame: the
/// footer says `a answer` for a ▲ selection (D14, D19).
#[test]
fn needs_input_border_is_red_and_carries_the_verb() {
    let th = Theme::fixed();
    let ps = fixture();
    let pvs = previews();
    let one = [ps[0].clone()];
    let pv = [pvs[0].clone()];
    let v = view(&one, &pv, &th, 0, 0);
    let b = render_to_buffer(72, 10, |f| board::render(f, f.area(), &v));

    // the tile starts on row 1 (row 0 is the header); its top-left corner cell
    assert_eq!(b[(0, 1)].fg, th.needs, "needs-input tile border must be needs red");
    // row 0 is the board header, row 1 the tile's top border, row 2 its first
    // header line: the question belongs there, not merely somewhere on screen
    let row2: String = (0..72).map(|x| b[(x, 2)].symbol()).collect();
    assert!(
        row2.contains("permission: Bash(sudo openvpn"),
        "the question must be on the tile's first header row: {row2:?}"
    );
    assert_eq!(b[(1, 2)].fg, th.needs, "the question is in needs red");
    let bottom: String = (0..72).map(|x| b[(x, 8)].symbol()).collect();
    assert!(bottom.starts_with('┗') && !bottom.contains("answer"), "{bottom:?}");
    let foot: String = (0..72).map(|x| b[(x, 9)].symbol()).collect();
    assert!(foot.contains("a answer"), "{foot:?}");
}

/// Focus is border weight, not colour: it survives NO_COLOR (ux.md §1.2).
#[test]
fn focus_is_border_weight() {
    let th = Theme::no_color();
    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, &th, 1, 0);
    let b = render_to_buffer(120, 30, |f| board::render(f, f.area(), &v));
    let row: String = (0..120).map(|x| b[(x, 1)].symbol()).collect();
    // tile 1 (needs-input, not focused) is plain; tile 2 (focused) is thick
    assert!(row.starts_with('┌'), "unfocused tile must be Plain: {row}");
    assert!(row.contains('┏'), "focused tile must be Thick: {row}");
}

/// Done and idle tiles dim their preview and grey their title (ux.md §5.1).
#[test]
fn done_and_idle_tiles_are_dimmed() {
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, &th, 1, 0);
    let b = render_to_buffer(120, 30, |f| board::render(f, f.area(), &v));
    // located by each tile's own title, so a neighbouring tile in the other
    // column cannot satisfy the assertion meant for this one
    let at = |marker: &str| -> (u16, u16) {
        let y = (0..b.area.height)
            .find(|&y| (0..b.area.width).map(|x| b[(x, y)].symbol()).collect::<String>().contains(marker))
            .unwrap_or_else(|| panic!("no row carries {marker:?}"));
        let row: Vec<String> = (0..b.area.width).map(|x| b[(x, y)].symbol().to_string()).collect();
        let x = (0..b.area.width as usize).find(|&x| row[x..].concat().starts_with(marker)).unwrap() as u16;
        (x, y)
    };
    // expressed twice: the title greys, and the preview dims, so a terminal that
    // ignores DIM still shows the state and a reader who ignores colour still does
    for (marker, who) in [("Left click issue", "done"), ("(no title)", "idle")] {
        let (x, y) = at(marker);
        assert_eq!(b[(x, y)].fg, th.meta, "the {who} tile's title must be meta, not value");
        assert!(
            b[(x, y + 3)].modifier.contains(Modifier::DIM),
            "the {who} tile's preview must carry DIM"
        );
    }
    // and a running tile is left alone, so the dimming still discriminates
    let (x, y) = at("Fleet gate review");
    assert_ne!(b[(x, y)].fg, th.meta, "a running tile's title must not be dimmed");
    assert!(
        !b[(x, y + 3)].modifier.contains(Modifier::DIM),
        "a running tile's preview must not carry DIM"
    );
}

// ------------------------------------------------------------ colour tiers

/// D17: the selection is ▎ plus reverse, so it still shows with no colour.
#[test]
fn no_color_keeps_the_selection_visible() {
    let th = Theme::no_color();
    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, &th, 2, 0);
    golden("board-nocolor-100", &draw(100, 30, |f| board::render(f, f.area(), &v)));
    let b = render_to_buffer(100, 30, |f| board::render(f, f.area(), &v));
    let (x, y) = (0..b.area.height)
        .flat_map(|y| (0..b.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| b[(x, y)].symbol() == "▎")
        .expect("the selection carries ▎");
    assert_eq!(b[(x + 1, y)].symbol(), "3", "on the selected tile");
    assert!(b[(x + 1, y)].modifier.contains(Modifier::REVERSED), "and its number is reversed");
    let marks = b.content.iter().filter(|c| c.symbol() == "▎").count();
    assert_eq!(marks, 1, "one selection");
    // tmux's active pane is a different mark, at its tile's end
    let text: String = b.content.iter().map(|c| c.symbol()).collect();
    assert_eq!(text.matches('◂').count(), 1);
}

/// D17's two rules: focus never shares a hue with a state, and what a reader
/// must read (a footer verb, the note, a help line, a way out) is never `meta`.
#[test]
fn focus_has_its_own_hue_and_must_read_text_is_not_meta() {
    let th = Theme::fixed();
    for st in [th.needs, th.error, th.running, th.done, th.sky] {
        assert_ne!(th.accent, st, "the focus hue is a state's");
    }
    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, &th, 1, 0);
    let b = render_to_buffer(120, 30, |f| board::render(f, f.area(), &v));
    let at = |y: u16| (0..b.area.width).map(|x| &b[(x, y)]).collect::<Vec<_>>();
    assert_eq!(
        at(1).iter().find(|c| c.symbol() == "┏").unwrap().fg,
        th.accent,
        "a focused running tile"
    );
    let words = |cells: Vec<&ratatui::buffer::Cell>| {
        cells
            .into_iter()
            .filter(|c| c.symbol().chars().all(char::is_alphabetic) && c.symbol() != " ")
            .map(|c| c.fg)
            .collect::<Vec<_>>()
    };
    assert!(!words(at(29)).contains(&th.meta), "the footer");

    let mut v = view(&ps, &pvs, &th, 3, 0);
    v.note = Some("work:2 is not waiting on you");
    let b = render_to_buffer(120, 30, |f| board::render(f, f.area(), &v));
    assert!(
        !(0..120).any(|x| b[(x, 29)].symbol() != " " && b[(x, 29)].fg == th.meta),
        "the note"
    );

    let mut v = view(&ps, &pvs, &th, 1, 0);
    v.help = Some(0);
    let b = render_to_buffer(100, 40, |f| board::render(f, f.area(), &v));
    let text: Vec<String> = (0..40).map(|y| (0..100).map(|x| b[(x, y)].symbol()).collect()).collect();
    let y = text.iter().position(|l| l.contains("review its diff")).unwrap() as u16;
    let x = text[y as usize].find("review").unwrap();
    let x = text[y as usize][..x].chars().count() as u16;
    assert_ne!(b[(x, y)].fg, th.meta, "a help line");

    let b = render_to_buffer(30, 12, |f| board::render(f, f.area(), &v));
    let text: Vec<String> = (0..12).map(|y| (0..30).map(|x| b[(x, y)].symbol()).collect()).collect();
    let y = text.iter().position(|l| l.contains("esc or q")).unwrap() as u16;
    let x = text[y as usize].find("esc").unwrap();
    let x = text[y as usize][..x].chars().count() as u16;
    assert_ne!(b[(x, y)].fg, th.meta, "the floor's way out");
}

/// NO_COLOR: every role is Reset and nothing else moves. Shape still carries
/// state, which is why this is the tier the goldens can pin byte-for-byte.
#[test]
fn no_color_tier() {
    let th = Theme::no_color();
    golden("board-nocolor", &board_at(120, 30, 0, &th));
    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, &th, 1, 0);
    let b = render_to_buffer(120, 30, |f| board::render(f, f.area(), &v));
    for c in &b.content {
        assert_eq!(c.fg, Color::Reset, "a cell kept a foreground colour under NO_COLOR");
        assert_eq!(c.bg, Color::Reset, "a cell kept a background colour under NO_COLOR");
    }
}

/// The ascii tier swaps glyphs AND border sets, and every line keeps the width
/// it had in the unicode tier (ux.md §5.4).
#[test]
fn ascii_tier_keeps_every_column_width() {
    use unicode_width::UnicodeWidthStr;
    // ascii pane content too: the tier swaps chrome, and unicode left in the
    // preview would let a chrome glyph hide behind it
    let pvs: Vec<Option<String>> = previews()
        .iter()
        .map(|p| p.as_ref().map(|t| t.replace(['❯', '⏺', '✓'], ">")))
        .collect();
    // the dot in `idle · handover pending` is pane data, not chrome, so it is
    // sanitised here too and is_ascii() can stay the strongest assertion
    let mut ps = fixture();
    for p in &mut ps {
        p.doing = p.doing.replace('·', ".");
    }
    let at = |g| {
        let mut th = Theme::fixed();
        th.g = g;
        let v = view(&ps, &pvs, &th, 1, 0);
        draw(80, 24, |f| board::render(f, f.area(), &v))
    };
    let ascii = at(Glyphs::ascii());
    golden("board-ascii", &ascii);
    assert!(ascii.is_ascii(), "the ascii tier emitted a non-ascii byte:\n{ascii}");

    let uni = at(Glyphs::unicode());
    for (i, (a, u)) in rows(&ascii).iter().zip(rows(&uni)).enumerate() {
        assert_eq!(a.width(), u.width(), "line {i} changed display width in the ascii tier");
    }
    for glyph in [
        "┌", "│", "─", "┏", "┃", "━", "▲", "▼", "●", "○", "✓", "✕", "◆", "↵", "▎", "◂", "…", "·", "—",
    ] {
        assert!(!ascii.contains(glyph), "the ascii tier still draws {glyph}:\n{ascii}");
    }
}

// --------------------------------------------------------------- the keymap

/// One KEYS table feeds the dispatch and the help overlay; a key bound but
/// undocumented, or documented but unbound, fails here (ux.md §3.4).
#[test]
fn help_lists_every_key_in_the_dispatch_table() {
    for list in [false, true] {
        dispatch_matches_help(list);
    }
}

fn dispatch_matches_help(list: bool) {
    let norm = |a: Act| if matches!(a, Act::Tile(_)) { Act::Tile(0) } else { a };
    let documented: HashSet<Act> = KEYS
        .iter()
        .filter(|k| k.on.shown(list))
        .flat_map(|k| k.acts.iter().copied())
        .chain(EXTRAS.iter().map(|e| Act::Extra(e.key)))
        .map(norm)
        .collect();

    let mut codes: Vec<KeyEvent> = ('a'..='z')
        .chain('A'..='Z')
        .chain('0'..='9')
        .chain("[]? -=/".chars())
        .map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
        .collect();
    codes.extend(
        [
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::PageUp,
            KeyCode::PageDown,
        ]
        .map(|c| KeyEvent::new(c, KeyModifiers::NONE)),
    );
    codes.push(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
    codes.push(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    let bound: HashSet<Act> = codes.iter().filter_map(|k| board::action(*k, list)).map(norm).collect();

    assert_eq!(bound, documented, "the dispatch table and the help overlay disagree (list: {list})");
}

/// One column over the whole board, the glyph legend at the end (D14), for
/// the layout on screen (D2): the list's by default, the tiles' behind `v`.
#[test]
fn help_goldens() {
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    let nav = Nav::new(Sel::Pane("%9".into()), Default::default());
    for (w, h) in SIZES {
        let mut v = view(&ps, &pvs, &th, 1, 0);
        v.help = Some(0);
        v.list = Some(&nav);
        golden(&format!("board-help-{w}"), &draw(w, h, |f| board::render(f, f.area(), &v)));
    }
    let mut v = view(&ps, &pvs, &th, 1, 0);
    v.help = Some(0);
    let out = draw(100, 40, |f| board::render(f, f.area(), &v));
    golden("board-help-tiles-100", &out);
    assert!(
        out.contains("tile by number") && out.contains("column") && !out.contains("fold"),
        "{out}"
    );
}

/// `↑↓ j k PgUp PgDn` scroll a help taller than the screen; anything else closes.
#[test]
fn a_short_help_scrolls_to_its_legend() {
    use cockpit_tui::chrome::scroll;
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    let n = board::help_rows(&th.g, false).len();
    let rows = 10usize;
    let bottom = n - rows;
    let mut v = view(&ps, &pvs, &th, 1, 0);
    v.help = Some(0);
    let first = draw(80, rows as u16 + 2, |f| board::render(f, f.area(), &v));
    assert!(
        first.contains(&format!("1-{rows} of {n}")) && first.contains("any other key closes"),
        "{first}"
    );
    assert!(!first.contains("tmux's active pane"), "{first}");
    let mut t = 0;
    for k in [
        KeyCode::Down,
        KeyCode::Char('j'),
        KeyCode::PageDown,
        KeyCode::PageDown,
        KeyCode::PageDown,
    ] {
        t = scroll(k, t, bottom, rows).expect("a scroll key keeps it open");
    }
    assert_eq!(t, bottom);
    v.help = Some(t);
    let last = draw(80, rows as u16 + 2, |f| board::render(f, f.area(), &v));
    assert!(last.contains("tmux's active pane") && last.contains("the selection"), "{last}");
    assert_eq!(scroll(KeyCode::Char('k'), t, bottom, rows), Some(bottom - 1));
    assert_eq!(scroll(KeyCode::PageUp, 1, bottom, rows), Some(0));
    assert_eq!(scroll(KeyCode::Char('q'), t, bottom, rows), None, "any other key closes");
}

// -------------------------------------------------------------- truncation

/// Never panics, never overruns, and the budget subtracts the ellipsis's own
/// width — three cells in the ascii tier, not one (ux.md §5.4).
#[test]
fn truncation_never_overruns() {
    use unicode_width::UnicodeWidthStr;
    let samples = [
        "",
        "a",
        "Bash: shellcheck on cockpit.tmux and tests/run.sh",
        "/home/u/.cache/cockpit-wt/lowercase-keys/scripts/agent-state",
        "中文中文中文中文中文",
        "a中b中c中d中e中f",
        "café　naïve",
        "⌨️ keyboard ⌨️ sequence",
    ];
    for g in [Glyphs::unicode(), Glyphs::ascii()] {
        for s in samples {
            for w in 0..40usize {
                for got in [g.trunc(s, w), g.trunc_left(s, w), g.trunc_middle(s, w)] {
                    assert!(got.width() <= w, "{:?} -> {got:?} is {} wide, over {w}", s, got.width());
                }
            }
            // a width at or above the source keeps it verbatim
            let w = s.width();
            assert_eq!(g.trunc(s, w), s);
            assert_eq!(g.trunc_left(s, w), s);
        }
    }
    let u = Glyphs::unicode();
    assert_eq!(u.trunc("abcdef", 4), "abc…");
    assert_eq!(u.trunc_left("/a/b/c/d", 5), "…/c/d");
    assert_eq!(u.trunc_middle("abcdefghij", 7), "abc…hij");
    let a = Glyphs::ascii();
    assert_eq!(a.trunc("abcdef", 4), "a...");
    assert_eq!(a.trunc_left("/a/b/c/d", 5), ".../d");
}

// ------------------------------------------------------------- the bans

/// ratatui #2679 panics on a double-width grapheme inside a wrapped Paragraph,
/// and crossterm #1104 wedges on a terminal query inside a popup.
#[test]
fn no_wrapping_and_no_terminal_queries() {
    let backend = include_str!("../src/backend.rs");
    for (name, src) in [
        ("board.rs", include_str!("../src/board.rs")),
        ("chrome.rs", include_str!("../src/chrome.rs")),
        ("theme.rs", include_str!("../src/theme.rs")),
        ("backend.rs", backend),
    ] {
        assert!(!src.contains("Wrap"), "{name} builds a Paragraph with Wrap");
        for q in ["supports_keyboard_enhancement", "SetCursorStyle", "EnableMouseCapture"] {
            assert!(!src.contains(q), "{name} sends a terminal query ({q})");
        }
    }
    // window_size is a required Backend method, so backend.rs names it exactly
    // twice: the signature and the delegation. Nothing else may call it.
    let calls = backend.lines().filter(|l| l.contains("window_size")).count();
    assert_eq!(
        calls, 2,
        "backend.rs must only declare and delegate window_size, found {calls} lines"
    );
    assert!(!include_str!("../src/board.rs").contains("window_size"));
    // every colour literal lives in theme.rs
    for (name, src) in [
        ("board.rs", include_str!("../src/board.rs")),
        ("chrome.rs", include_str!("../src/chrome.rs")),
    ] {
        assert!(!src.contains("Color::"), "{name} names a colour directly");
    }
}

/// Hints drop whole, lowest priority first, and `?` and `q` stay: "half a
/// label is not a smaller button, it is an unreadable one" (ux.md §1.6).
#[test]
fn footer_drops_whole_hints_in_priority_order() {
    use cockpit_tui::chrome::fit;
    let g = Glyphs::unicode();
    let keys = |w: usize| -> Vec<String> {
        fit(board::hints(&g, false, Focus::Agent(false)), w, 3)
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    };
    for (w, want) in [
        (200, vec!["↵", "a", "r", "f", "i", "x", "?", "q"]),
        (74, vec!["↵", "a", "r", "f", "i", "x", "?", "q"]),
        (73, vec!["↵", "a", "r", "f", "i", "?", "q"]),
        (65, vec!["↵", "a", "r", "f", "i", "?", "q"]),
        (64, vec!["↵", "a", "r", "f", "?", "q"]),
        (56, vec!["↵", "a", "r", "f", "?", "q"]),
        (55, vec!["↵", "a", "r", "?", "q"]),
        (46, vec!["↵", "a", "r", "?", "q"]),
        (45, vec!["↵", "a", "?", "q"]),
        (35, vec!["↵", "a", "?", "q"]),
        (34, vec!["↵", "?", "q"]),
        (26, vec!["?", "q"]),
        (17, vec!["q"]),
        (1, vec!["q"]),
    ] {
        assert_eq!(keys(w), want, "at w={w}");
    }
    // the label follows the selection: answer on a ▲, ask on anything else
    let verb = |needs| {
        board::hints(&g, false, Focus::Agent(needs))
            .into_iter()
            .find(|(k, _)| k == "a")
            .unwrap()
            .1
    };
    assert_eq!((verb(true).as_str(), verb(false).as_str()), ("answer", "ask"));
    // the panel's spacing: its 40-column footers are pinned by its goldens
    let ps: Vec<(String, String)> = [("i", "insert"), ("s", "send"), ("?", "keys"), ("esc", "draft")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert_eq!(fit(ps.clone(), 37, 2).len(), 4);
    assert_eq!(fit(ps, 36, 2).into_iter().map(|(k, _)| k).collect::<Vec<_>>(), ["i", "?", "esc"]);
}

/// Every key the footer names is one the dispatch takes there, for what the
/// hint says (D19): on a tile, on a list agent, on a list header.
#[test]
fn the_footer_lists_only_keys_the_dispatch_accepts() {
    let cases = [
        (false, Focus::Agent(false)),
        (false, Focus::Agent(true)),
        (true, Focus::Agent(false)),
        (true, Focus::Agent(true)),
        (true, Focus::Head(false)),
        (true, Focus::Head(true)),
    ];
    for (list, focus) in cases {
        for (k, verb) in board::hints(&Glyphs::unicode(), list, focus) {
            let code = match k.as_str() {
                "↵" => KeyCode::Enter,
                "space" => KeyCode::Char(' '),
                "n p" => KeyCode::Char('n'),
                c if c.chars().count() == 1 => KeyCode::Char(c.chars().next().unwrap()),
                other => panic!("the footer names {other:?}, which no key sends"),
            };
            let a = board::action(KeyEvent::new(code, KeyModifiers::NONE), list).unwrap_or_else(|| panic!("{k} {verb} is not bound"));
            if let Act::Extra(c) = a {
                let e = board::extra(c).unwrap();
                assert!(matches!(focus, Focus::Agent(_)) && e.label == verb, "{k} {verb} runs {a:?}");
                continue;
            }
            let row = KEYS.iter().find(|r| r.acts.contains(&a) && r.on.shown(list)).unwrap();
            let named = if matches!(focus, Focus::Head(_)) { row.head } else { row.hint };
            assert!(!named.is_empty() && row.keys.starts_with(k.as_str()), "{k} {verb} runs {a:?}");
        }
    }
    // a header never offers the act keys, which need an agent
    let head: Vec<String> = board::hints(&Glyphs::unicode(), true, Focus::Head(false))
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    for k in ["↵", "a", "r", "f", "i", "x"] {
        assert!(!head.contains(&k.to_string()), "a header's footer offers {k}");
    }
}

#[test]
fn board_goldens() {
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    for (w, h) in SIZES {
        // 60 selects the ▲: `a answer`, and ▎ and ◂ on different tiles
        let v = view(&ps, &pvs, &th, if w == 60 { 0 } else { 1 }, 0);
        golden(&format!("board-{w}"), &draw(w, h, |f| board::render(f, f.area(), &v)));
    }
}

#[test]
fn empty_fleet_says_so() {
    let th = Theme::fixed();
    for (w, h) in SIZES {
        let v = view(&[], &[], &th, 0, 0);
        let out = draw(w, h, |f| board::render(f, f.area(), &v));
        golden(&format!("board-empty-{w}"), &out);
        assert!(out.contains("no agent panes"), "{out}");
        assert!(
            out.contains("start an agent in any pane and it shows up here."),
            "wrapped, never cut:\n{out}"
        );
        assert!(!out.contains("prefix r"), "{out}");
        assert!(!out.contains('…'), "{out}");
    }
    // centred: as many rows above the card as under it, give or take one
    let v = view(&[], &[], &th, 0, 0);
    let out = draw(100, 30, |f| board::render(f, f.area(), &v));
    let r = rows(&out);
    let top = r.iter().position(|l| l.contains('╭')).unwrap();
    let bot = r.iter().position(|l| l.contains('╰')).unwrap();
    assert!((top as i32 - 1 - (r.len() - 1 - bot) as i32).abs() <= 1, "{out}");
    // and wrapped at 44 columns, not cut
    let out = draw(44, 24, |f| board::render(f, f.area(), &v));
    assert!(
        out.contains("start an agent in any pane and it shows") && out.contains("│ up here."),
        "{out}"
    );
}

/// D16: the header says how stale the tiles are; the card comes after 30 s,
/// three failed reads, or at once when no read ever worked.
#[test]
fn a_silent_tmux_goes_stale_then_says_so() {
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    let mut v = view(&ps, &pvs, &th, 1, 0);
    v.stale = Some(12);
    let out = draw(100, 30, |f| board::render(f, f.area(), &v));
    golden("board-stale-100", &out);
    assert!(out.lines().next().unwrap().contains("· stale 12s"), "{out}");
    assert!(!out.contains("tmux is not answering"), "{out}");

    v.stale = Some(34);
    v.lost = true;
    let out = draw(100, 30, |f| board::render(f, f.area(), &v));
    for s in [
        "board · stale 34s",
        "tmux is not answering",
        "last good read 34s ago · the server may have exited.",
        "^L retry",
        "q close",
    ] {
        assert!(out.contains(s), "{s}:\n{out}");
    }
}

/// Each legend glyph means one thing, in both tiers: the ascii pane mark once
/// shared `<` with review-sent.
#[test]
fn the_legend_never_lists_a_glyph_twice() {
    for g in [Glyphs::unicode(), Glyphs::ascii()] {
        let rows = board::help_rows(&g, false);
        let at = rows.iter().position(|(k, v)| k.is_empty() && v == "on a tile").unwrap();
        let marks: Vec<&str> = rows[at + 1..].iter().map(|(k, _)| k.as_str()).collect();
        let set: HashSet<&str> = marks.iter().copied().collect();
        assert_eq!(set.len(), marks.len(), "a glyph twice in the legend: {marks:?}");
    }
}

/// The stale rules over a passed-in clock: a failed read, or a worker silent
/// for 3 s (a hung tmux fails nothing), makes the header; 30 s, three failures,
/// or no read ever makes the card.
#[test]
fn stale_counts_from_the_last_good_read_and_a_hang_counts_too() {
    use board::Reads;
    use std::time::{Duration, Instant};
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut r = Reads::new(t0);
    r.got(true, at(1));
    assert_eq!((r.stale(at(2)), r.lost(at(2))), (None, false));
    // hung: no snapshot at all after the good read at 1 s
    assert_eq!(r.stale(at(3)), None, "two quiet seconds are a slow read");
    assert_eq!((r.stale(at(4)), r.lost(at(4))), (Some(3), false));
    assert_eq!((r.stale(at(31)), r.lost(at(31))), (Some(30), true));
    // failing: the header at once, the card at the third failure
    let mut r = Reads::new(t0);
    r.got(true, at(1));
    r.got(false, at(2));
    assert_eq!((r.stale(at(2)), r.lost(at(2))), (Some(1), false));
    r.got(false, at(3));
    r.got(false, at(4));
    assert!(r.lost(at(4)));
    r.got(true, at(5));
    assert_eq!((r.stale(at(5)), r.lost(at(5))), (None, false), "a good read clears it");
    // no read has ever worked: the card at once, failed or hung
    let mut r = Reads::new(t0);
    assert_eq!((r.stale(at(2)), r.lost(at(2))), (None, false));
    assert!(r.lost(at(3)), "a first read that hangs");
    r.got(false, at(1));
    assert!(r.lost(at(1)), "a first read that fails");
}

/// With no read ever, the card claims no age and no tiles, and nothing peeks
/// out from under it.
#[test]
fn the_never_card_says_no_read_has_worked() {
    let th = Theme::fixed();
    let mut v = view(&[], &[], &th, 0, 0);
    (v.stale, v.lost, v.never) = (Some(4), true, true);
    let out = draw(100, 30, |f| board::render(f, f.area(), &v));
    golden("board-never-100", &out);
    assert!(out.contains("tmux is not answering · no read has worked yet."), "{out}");
    assert!(out.contains("^L retry") && out.contains("q close"), "{out}");
    for s in ["last good read", "tiles below", "stale", "no agent panes"] {
        assert!(!out.contains(s), "{s}:\n{out}");
    }
    assert_eq!(out.matches('╰').count(), 1, "one card:\n{out}");
}

/// Better than a grid of tiles too narrow to name their agent (rules.md 32):
/// the shared floor card, with its way out.
#[test]
fn refuses_below_forty_columns() {
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, &th, 0, 0);
    let out = draw(30, 12, |f| board::render(f, f.area(), &v));
    assert!(out.contains("make the window larger") && out.contains("rotate"), "{out}");
    assert!(out.contains("esc or q closes"), "{out}");
    assert!(!out.contains("too short"), "{out}");
    // and 40 still draws the board
    let v = view(&ps, &pvs, &th, 0, 0);
    assert!(draw(40, 12, |f| board::render(f, f.area(), &v)).contains("henosis:1"));
}

/// `]` jumps a screenful and stops on the last row, so the final page can hold
/// one tile at its normal size rather than one stretched to fill (ux.md §1.5).
#[test]
fn paging_reaches_the_last_row_and_back() {
    let ps = fixture();
    let ord = board::order(&ps);
    let g = board::grid(ratatui::layout::Rect::new(0, 0, 80, 24), ps.len());
    let (sel, top) = board::moved(Act::PageDown, 0, ps.len(), &g, 0, &ps, &ord);
    assert_eq!((sel, top), (4, 4), "one screenful down lands on the last row");
    let (sel, top) = board::moved(Act::PageUp, sel, ps.len(), &g, top, &ps, &ord);
    assert_eq!((sel, top), (0, 0), "and back to the top");
    // G pulls the viewport only as far as it must
    let (sel, top) = board::moved(Act::Last, 0, ps.len(), &g, 0, &ps, &ord);
    assert_eq!((sel, top), (4, 1), "G scrolls the minimum that shows the last tile");
    // u walks the blocked tiles, which sort first
    assert_eq!(board::moved(Act::NextNeeds, 2, ps.len(), &g, 0, &ps, &ord).0, 0);
}

/// h and l move a column and stop at the row edge; tab is what wraps in reading
/// order. Written as `idx ± 1` they would be the same two keys (ux.md §3.1).
#[test]
fn column_movement_stops_at_the_row_edge() {
    let ps = fixture();
    let ord = board::order(&ps);
    let g = board::grid(ratatui::layout::Rect::new(0, 0, 120, 30), ps.len());
    assert_eq!(g.cols, 2);
    let m = |a, sel| board::moved(a, sel, ps.len(), &g, 0, &ps, &ord).0;
    assert_eq!(m(Act::Right, 0), 1);
    assert_eq!(m(Act::Right, 1), 1, "l must not fall into the next row");
    assert_eq!(m(Act::Left, 2), 2, "h must not climb into the previous row");
    assert_eq!(m(Act::Next, 1), 2, "tab is the one that crosses rows");
    assert_eq!(m(Act::Next, 4), 0, "and wraps");
}

/// Neither moved() nor the tick may leave the selection off-screen: focus is
/// border weight, so an off-screen selection has nothing pointing at it.
#[test]
fn the_viewport_always_holds_the_selection() {
    let ps = fixture();
    let ord = board::order(&ps);
    let acts = [
        Act::Left,
        Act::Right,
        Act::Up,
        Act::Down,
        Act::Next,
        Act::Prev,
        Act::First,
        Act::Last,
        Act::PageUp,
        Act::PageDown,
        Act::NextNeeds,
        Act::Tile(1),
        Act::Tile(5),
        Act::Tile(9),
    ];
    for (w, h) in [(80u16, 24u16), (120, 30), (160, 24), (48, 12)] {
        let g = board::grid(ratatui::layout::Rect::new(0, 0, w, h), ps.len());
        for a in acts {
            for sel in 0..ps.len() {
                for top in 0..g.rows {
                    let (s2, t2) = board::moved(a, sel, ps.len(), &g, top, &ps, &ord);
                    assert!(
                        s2 < ps.len() && t2 < g.rows,
                        "{a:?} from sel={sel} top={top} at {w}x{h} produced sel={s2} top={t2}"
                    );
                    let row = s2 / g.cols;
                    assert!(
                        row >= t2 && row < t2 + g.rows_shown,
                        "{a:?} from sel={sel} top={top} at {w}x{h} left sel={s2} off-screen (top={t2})"
                    );
                }
            }
        }
    }
    // and it never panics on an empty fleet
    let g = board::grid(ratatui::layout::Rect::new(0, 0, 80, 24), 0);
    assert_eq!(board::moved(Act::Tile(0), 0, 0, &g, 0, &[], &[]), (0, 0));
}

/// ux.md §1.5's table: sixteen agents at 160x24 is compact, twelve visible, and
/// the tile numbers reach two digits.
#[test]
fn sixteen_agents_page_and_number_correctly() {
    let mut ps = fixture();
    for i in 40..51 {
        let mut p = pane(&format!("%{i}"), &format!("work:{i}"), &format!("Task {i}"), "running");
        p.doing = format!("Bash: step {i}");
        p.since = Some(60);
        ps.push(p);
    }
    assert_eq!(ps.len(), 16);
    let pvs = vec![Some(String::new()); 16];
    let th = Theme::fixed();
    let v = view(&ps, &pvs, &th, 0, 0);
    let out = draw(160, 24, |f| board::render(f, f.area(), &v));
    let head = out.lines().next().unwrap();
    assert!(head.contains("16 agents"), "{head}");
    assert!(head.contains("1-12 of 16"), "{head}");
    assert!(head.contains("▼4"), "{head}");
    assert!(out.contains(" 12 ●"), "the twelfth tile keeps its two-digit number:\n{out}");

    // and on page two the number printed in a tile is still the number that
    // selects it: `5` reaches the tile labelled 5 wherever the viewport is
    let ord = board::order(&ps);
    let g = board::grid(ratatui::layout::Rect::new(0, 0, 160, 24), ps.len());
    let (sel, top) = board::moved(Act::PageDown, 0, ps.len(), &g, 0, &ps, &ord);
    let v = view(&ps, &pvs, &th, sel, top);
    let out = draw(160, 24, |f| board::render(f, f.area(), &v));
    assert!(out.lines().next().unwrap().contains("13-16 of 16"), "{out}");
    assert!(out.contains("▎13 ●"), "page two labels its first tile 13:\n{out}");
    assert_eq!(
        board::moved(Act::Tile(5), sel, ps.len(), &g, top, &ps, &ord).0,
        4,
        "5 selects the tile printed as 5, from any page"
    );
}

/// A token wider than a header line must be marked truncated, never silently
/// clipped: it used to push the 5-column age rail off the tile entirely.
#[test]
fn an_over_wide_token_never_eats_the_rail() {
    use unicode_width::UnicodeWidthStr;
    let th = Theme::fixed();
    let mut ps = fixture();
    ps[0].msg = format!("{} tail", "x".repeat(200));
    ps[1].doing = "y".repeat(200);

    // each pane alone, so the assertion can only be satisfied by its own tile:
    // %21 also renders 4m12s, and any tile can supply the ellipsis
    for (i, rail) in [(0usize, "4m12s"), (1, "32s")] {
        let one = [ps[i].clone()];
        let pv = [Some(String::new())];
        for (w, h) in [(80u16, 24u16), (60, 10), (160, 12)] {
            let v = view(&one, &pv, &th, 0, 0);
            let out = draw(w, h, |f| board::render(f, f.area(), &v));
            assert!(out.contains(rail), "tile {i} lost its rail at {w}x{h}:\n{out}");
            assert!(out.contains('…'), "tile {i} must mark the token truncated at {w}x{h}:\n{out}");
            for (n, line) in rows(&out).iter().enumerate() {
                assert!(
                    line.width() <= w as usize,
                    "at {w}x{h} row {n} is {} columns, over {w}: {line:?}",
                    line.width()
                );
            }
        }
    }
}

/// No fixture, width, tier or scroll position may produce a row wider than the
/// area. A golden pins one size; this pins the rule across all of them.
#[test]
fn no_rendered_row_ever_exceeds_the_area() {
    use unicode_width::UnicodeWidthStr;
    let mut ps = fixture();
    ps.push(pane(
        "%77",
        "a-very-long-session-name:12",
        "and a title that keeps going and going and going",
        "error",
    ));
    let pvs = previews();
    let ascii = {
        let mut t = Theme::fixed();
        t.g = Glyphs::ascii();
        t
    };
    for th in [Theme::fixed(), Theme::no_color(), ascii] {
        for w in [39u16, 40, 48, 60, 80, 96, 120, 144, 160, 200] {
            for h in [4u16, 5, 8, 12, 24, 30, 40] {
                for top in [0usize, 1, 3] {
                    let v = view(&ps, &pvs, &th, 0, top);
                    let out = draw(w, h, |f| board::render(f, f.area(), &v));
                    for (i, line) in rows(&out).iter().enumerate() {
                        assert!(
                            line.width() <= w as usize,
                            "at {w}x{h} top={top} row {i} is {} columns, over {w}: {line:?}",
                            line.width()
                        );
                    }
                }
            }
        }
    }
}

/// Under 8 rows the width is fine, so the card says `too short`, not narrow.
#[test]
fn refuses_below_the_height_floor_too() {
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, &th, 0, 0);
    let out = draw(100, 6, |f| board::render(f, f.area(), &v));
    golden("board-short-100", &out);
    assert!(out.contains("too short") && out.contains("esc or q closes"), "{out}");
    assert!(draw(80, 4, |f| board::render(f, f.area(), &v)).contains("too short"));
    assert!(draw(80, 7, |f| board::render(f, f.area(), &v)).contains("too short"));
    assert!(draw(80, 8, |f| board::render(f, f.area(), &v)).contains("henosis:1"));
    assert!(board::too_small(ratatui::layout::Rect::new(0, 0, 80, 7)));
    assert!(!board::too_small(ratatui::layout::Rect::new(0, 0, 40, 8)));
}

/// capture-pane exit 1 with empty stdout means the pane is gone, not that the
/// pane is quiet. A blank body would make those two look identical.
#[test]
fn a_failed_capture_says_so_instead_of_going_blank() {
    let th = Theme::fixed();
    let ps = fixture();
    let mut pvs = previews();
    pvs[1] = None;
    let v = view(&ps, &pvs, &th, 1, 0);
    let out = draw(120, 30, |f| board::render(f, f.area(), &v));
    assert!(out.contains("capture failed"), "a failed capture must say so:\n{out}");
    // and only that tile: the others still show their pane
    assert!(out.contains("Do you want to proceed?"), "{out}");
    assert_eq!(out.matches("capture failed").count(), 1, "{out}");

    // before the first fetch there is no entry at all, and that draws nothing
    let none: Vec<Option<String>> = Vec::new();
    let v = view(&ps, &none, &th, 1, 0);
    let out = draw(120, 30, |f| board::render(f, f.area(), &v));
    assert!(!out.contains("capture failed"), "an unfetched tile is not a failed one:\n{out}");
    assert!(out.contains("henosis:1"), "the tiles still draw:\n{out}");
}

/// F8: the ages must not reach the redraw gate, or an idle server repaints once
/// a second forever. No clock and no sleep -- a tick is simulated by advancing
/// `since`, which is what a clock would have done anyway.
#[test]
fn the_gate_ignores_the_ages_but_the_rail_still_redraws() {
    use cockpit_tui::data::hash_rows;
    // the same expression run() feeds the gate, not a local re-implementation
    let extra = board::gate_extra;

    let a = fixture();
    let pvs = previews();
    let mut older = fixture();
    for p in &mut older {
        p.since = p.since.map(|s| s + 1);
        p.idle_since = Some(99);
        p.asked_since = Some(7);
    }
    assert_eq!(
        hash_rows(&a, ""),
        hash_rows(&older, ""),
        "a second of ageing must not dirty the gate"
    );

    // but the gate hashes the RENDERED durations, so a rail the reader can see
    // change still redraws -- and one they cannot does not
    assert_ne!(extra(&a, &pvs), extra(&older, &pvs), "32s -> 33s is visible");
    assert_ne!(hash_rows(&a, &extra(&a, &pvs)), hash_rows(&older, &extra(&older, &pvs)));

    let mut invisible = fixture();
    for p in &mut invisible {
        if p.since == Some(720) {
            p.since = Some(721);
        }
    }
    assert_eq!(extra(&a, &pvs), extra(&invisible, &pvs), "12m0s and 12m1s both render 12m");
    assert_eq!(hash_rows(&a, &extra(&a, &pvs)), hash_rows(&invisible, &extra(&invisible, &pvs)));

    // and a changed preview always dirties it
    let mut moved = pvs.clone();
    moved[0] = Some("something else".into());
    assert_ne!(extra(&a, &pvs), extra(&a, &moved));
}

/// Every SGR parameter crossterm emitted, in order.
fn sgr_params(bytes: &[u8]) -> Vec<u16> {
    let s = String::from_utf8_lossy(bytes).into_owned();
    let mut out = Vec::new();
    let mut rest = s.as_str();
    while let Some(i) = rest.find("\u{1b}[") {
        rest = &rest[i + 2..];
        let Some(end) = rest.find(|c: char| c.is_ascii_alphabetic()) else {
            break;
        };
        let (params, tail) = rest.split_at(end);
        if tail.starts_with('m') {
            out.extend(params.split(';').filter_map(|p| p.parse::<u16>().ok()));
        }
        rest = &tail[1..];
    }
    out
}

/// The pin `Theme::load` could not have, because it is only reached from
/// `board::run`: under NO_COLOR=1 nothing colour-bearing reaches the wire, and
/// bold and dim survive -- which is the whole reason NO_COLOR is honoured here
/// instead of being left to crossterm, whose empty SGR would drop all three.
#[test]
fn no_color_puts_no_colour_byte_on_the_wire() {
    use cockpit_tui::backend::WideCellBackend;
    use ratatui::backend::Backend;
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    let th = {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: every test that touches the environment takes ENV first, and
        // Theme::load returns before any tmux call when NO_COLOR is set
        unsafe { std::env::set_var("NO_COLOR", "1") };
        let th = Theme::load();
        unsafe { std::env::remove_var("NO_COLOR") };
        th
    };

    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, &th, 1, 0);
    let b = render_to_buffer(120, 30, |f| board::render(f, f.area(), &v));

    let mut sink: Vec<u8> = Vec::new();
    {
        let mut backend = WideCellBackend::new(&mut sink);
        let mut cells = Vec::new();
        for y in 0..b.area.height {
            for x in 0..b.area.width {
                cells.push((x, y, &b[(x, y)]));
            }
        }
        backend.draw(cells.into_iter()).unwrap();
        Backend::flush(&mut backend).unwrap();
    }
    assert!(!sink.is_empty(), "nothing reached the wire");

    let params = sgr_params(&sink);
    for p in &params {
        assert!(
            !matches!(p, 30..=38 | 40..=48 | 90..=97 | 100..=107),
            "NO_COLOR still emitted SGR colour parameter {p}"
        );
    }
    assert!(params.contains(&1), "bold must survive NO_COLOR");
    assert!(params.contains(&2), "dim must survive NO_COLOR");
}

/// The popup must not be a blank rectangle while the first fetch runs
/// (ux.md §4); `claude agents --json` alone measures 380 ms.
#[test]
fn a_loading_frame_is_drawn_before_the_first_fetch() {
    let th = Theme::fixed();
    let out = draw(80, 24, |f| board::loading(f, f.area(), &th));
    assert!(out.contains("reading the fleet"), "{out}");
    // and it is not the empty-fleet card, which would be a lie before a read
    assert!(!out.contains("no agent panes"), "{out}");
}

// ---------------------------------------------------------------- the return trip

#[test]
fn a_success_note_fades_after_five_seconds_and_a_failure_stays() {
    let t0 = std::time::Instant::now();
    let s = |n: u64| t0 + std::time::Duration::from_millis(n);
    let ok = board::Note::returned("cockpit: sent to %3".into(), t0);
    assert!(!ok.gone(s(4_999)), "under 5 s");
    assert!(ok.gone(s(5_000)), "at 5 s");
    let bad = board::Note::returned("✕ not sent: server asks something new".into(), t0);
    assert!(!bad.gone(s(3_600_000)), "a ✕ note outlives any clock");
}

#[test]
fn a_failure_note_goes_on_the_next_key_which_still_runs_except_esc() {
    let t0 = std::time::Instant::now();
    let bad = board::Note::returned("✕ cockpit: could not paste into %3; nothing sent".into(), t0);
    assert_eq!(board::note_key(&bad, KeyCode::Esc), (true, false), "esc only dismisses");
    for k in [
        KeyCode::Char('r'),
        KeyCode::Char('j'),
        KeyCode::Enter,
        KeyCode::Char('q'),
        KeyCode::Char('?'),
    ] {
        assert_eq!(board::note_key(&bad, k), (true, true), "{k:?} dismisses and acts");
    }
    // a success note is the clock's: a key neither takes it nor is taken by it
    let ok = board::Note::returned("cockpit: sent to %3".into(), t0);
    assert_eq!(board::note_key(&ok, KeyCode::Esc), (false, true));
    assert_eq!(board::note_key(&ok, KeyCode::Char('r')), (false, true));
}

#[test]
fn board_flags_parse_strictly() {
    let a = |v: &[&str]| board::opts(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(a(&[]).unwrap(), board::Opts::default());
    let o = a(&["--select", "%14", "--note", "/x/n"]).unwrap();
    assert_eq!(
        (o.select.as_deref(), o.note.as_deref()),
        (Some("%14"), Some(std::path::Path::new("/x/n")))
    );
    for bad in [
        &["--select"][..],
        &["--select", "14"],
        &["--select", "%1x"],
        &["--select", "%1", "--select", "%2"],
        &["--note", ""],
        &["--resume", "x"],
    ] {
        assert!(a(bad).is_err(), "{bad:?}");
    }
}

/// A private scratch dir per test: cargo runs tests on threads.
fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cockpit-board-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn board_state_round_trips_and_a_missing_file_is_a_fresh_board() {
    let d = scratch("state-rt");
    let p = d.join("tmux-cockpit/board.state");
    assert_eq!(board::read_state(&p), Ok(None));
    let s = board::Saved {
        sel: Some("%14".into()),
        ..Default::default()
    };
    board::write_state(&p, &s).unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "cockpit-board-state 1\nsel %14\n");
    assert_eq!(board::read_state(&p), Ok(Some(s)));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn board_state_is_read_strictly() {
    let d = scratch("state-strict");
    let p = d.join("board.state");
    for (body, why) in [
        ("cockpit-board-state 2\nsel %1\n", "a newer head"),
        ("cockpit-panel-state 1\nsel %1\n", "another file's head"),
        ("cockpit-board-state 1\nsel %1", "cut short"),
        ("cockpit-board-state 1\nsel 1\n", "not a pane id"),
        ("cockpit-board-state 1\nsel %1\nsel %2\n", "two selections"),
        ("cockpit-board-state 1\nwho knows\n", "an unknown line"),
        ("", "empty"),
    ] {
        std::fs::write(&p, body).unwrap();
        assert!(board::read_state(&p).is_err(), "{why}");
    }
    // oversize, even with a good head
    let mut big = String::from("cockpit-board-state 1\n");
    big.push_str(&"x".repeat(board::STATE_MAX as usize));
    std::fs::write(&p, big).unwrap();
    assert!(board::read_state(&p).is_err(), "over 64 KiB");
    // a symlink to a good file is still refused
    let real = d.join("real");
    std::fs::write(&real, "cockpit-board-state 1\nsel %1\n").unwrap();
    std::fs::remove_file(&p).unwrap();
    std::os::unix::fs::symlink(&real, &p).unwrap();
    assert!(board::read_state(&p).is_err(), "a symlink");
    // and a write replaces the link, never the file it points at
    board::write_state(
        &p,
        &board::Saved {
            sel: Some("%2".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "cockpit-board-state 1\nsel %1\n");
    assert!(!std::fs::symlink_metadata(&p).unwrap().file_type().is_symlink());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_unknown_pane_falls_back_to_d5() {
    let ps = fixture();
    let ord = board::order(&ps);
    let at = |id: &str| ord.iter().position(|&i| ps[i].id == id).unwrap();
    assert_eq!(board::open_sel(&ps, &ord, Some("%21"), "%14"), at("%21"), "the pane asked for");
    assert_eq!(board::open_sel(&ps, &ord, Some("%99"), "%14"), at("%9"), "gone: the first ▲");
    let calm: Vec<Pane> = ps
        .iter()
        .filter(|p| p.state != cockpit_tui::data::State::NeedsInput)
        .cloned()
        .collect();
    let ord = board::order(&calm);
    let at = |id: &str| ord.iter().position(|&i| calm[i].id == id).unwrap();
    assert_eq!(
        board::open_sel(&calm, &ord, Some("%99"), "%30"),
        at("%30"),
        "no ▲: the client's pane"
    );
    assert_eq!(board::open_sel(&calm, &ord, None, "%99"), 0, "else the first row");
}

#[test]
fn the_note_file_is_one_line_under_the_same_rules() {
    let d = scratch("note");
    let p = d.join("note");
    std::fs::write(&p, "✕ cockpit: could not paste into %3\u{1b}[2J; nothing sent\n").unwrap();
    assert_eq!(
        board::read_note(&p),
        Ok(Some("✕ cockpit: could not paste into %3[2J; nothing sent".into()))
    );
    std::fs::write(&p, "").unwrap();
    assert_eq!(board::read_note(&p), Ok(None), "empty is no note");
    std::fs::write(&p, "one\ntwo\n").unwrap();
    assert!(board::read_note(&p).is_err(), "two lines");
    std::fs::write(&p, "x".repeat(board::NOTE_MAX as usize + 1)).unwrap();
    assert!(board::read_note(&p).is_err(), "oversize");
    let real = d.join("real");
    std::fs::write(&real, "cockpit: sent to %3\n").unwrap();
    std::fs::remove_file(&p).unwrap();
    std::os::unix::fs::symlink(&real, &p).unwrap();
    assert!(board::read_note(&p).is_err(), "a symlink");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn note_goldens() {
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    let mut v = view(&ps, &pvs, &th, 2, 0);
    v.note = Some("cockpit: sent to %21");
    golden("board-note-100", &draw(100, 30, |f| board::render(f, f.area(), &v)));
    v.note = Some("✕ cockpit: the pane now runs bash, not claude; nothing sent to %21");
    let out = draw(100, 30, |f| board::render(f, f.area(), &v));
    golden("board-note-fail-100", &out);
    // with no tiles there is no key bar, but the note that takes esc still shows
    let out = draw(100, 30, |f| board::render(f, f.area(), &board::View { panes: &[], pvs: &[], ..v }));
    assert!(out.contains("no agent panes"), "{out}");
    assert!(rows(&out)[29].contains("nothing sent to %21 · esc dismiss"), "{out}");
    // the way out survives a cut at 60 columns
    let out = draw(60, 24, |f| board::render(f, f.area(), &v));
    assert!(rows(&out)[23].trim_end().ends_with("· esc dismiss"), "{out}");
}

// ------------------------------------------------------- the list (step 3)

/// 6.1's fleet: four sessions, thirteen agents, in tmux's own order.
fn fleet() -> Vec<Pane> {
    let a = |id: &str, wher: &str, title: &str, state: &str, line: &str, since: Option<u64>| {
        let mut p = pane(id, wher, title, state);
        if matches!(state, "needs-input" | "error") {
            p.msg = line.into();
        } else {
            p.doing = line.into();
        }
        p.since = since;
        p
    };
    let mut ps = vec![
        a("%3", "api:0", "server", "needs-input", "Do you want to run npm test?", Some(252)),
        a("%4", "api:1", "tests", "running", "Bash: cargo test --workspace", Some(63)),
        a("%5", "api:3", "migrate", "running", "Edit src/db/migrate.rs", Some(22)),
        a("%6", "api:4", "auth", "done", "Pushed PR #231, waiting on review", Some(720)),
        a("%7", "web:2", "docs", "needs-input", "plan ready: approve?", Some(38)),
        a("%8", "web:0", "ui", "running", "Bash: pnpm test Board", Some(220)),
        a("%9", "web:1", "checkout", "done", "Opened PR #88: card form a11y", Some(1500)),
        a("%10", "web:5", "a11y", "", "", None),
        a("%11", "infra:0", "terraform", "error", "terraform plan: state lock held", Some(362)),
        a("%12", "infra:1", "dns", "running", "Bash: dig +short api.internal", Some(9)),
        a("%13", "infra:2", "k8s", "", "", None),
        a("%14", "ml:0", "train", "running", "Bash: python train.py --epochs 40", Some(2460)),
        a("%15", "ml:1", "eval", "done", "Wrote eval/test_report.md", Some(7500)),
    ];
    ps[0].review = "pending".into();
    ps[3].review = "pending".into();
    ps[6].review = "pending".into();
    // what data::fleet fills in on the fetch thread: repo⑂branch and the stat
    for (id, loc, pm) in [
        ("%3", "api⑂feat/rate-limit", "+42 -7"),
        ("%4", "api⑂feat/rate-limit", "+42 -7"),
        ("%5", "api⑂feat/rate-limit", "+42 -7"),
        ("%6", "api⑂main", "clean"),
        ("%7", "web⑂docs-plan", "+3 -1"),
        ("%8", "web⑂main", "+120 -33  ?2 new"),
        ("%9", "web⑂checkout-a11y", "+0 -0  ?1 new"),
        ("%10", "web⑂main", "clean"),
        ("%11", "infra⑂main", "+9 -2"),
        ("%12", "infra⑂main", "clean"),
        ("%14", "ml⑂exp-40", "+15 -4"),
        ("%15", "ml⑂exp-40", "+15 -4"),
    ] {
        let p = ps.iter_mut().find(|p| p.id == id).unwrap();
        (p.loc, p.pm) = (loc.into(), pm.into());
    }
    ps.reverse();
    ps
}

fn folds(ss: &[&str]) -> BTreeSet<String> {
    ss.iter().map(|s| s.to_string()).collect()
}

/// The list as text: `[api]` for a header, the pane id for an agent.
fn shown(ps: &[Pane], f: &BTreeSet<String>) -> Vec<String> {
    board::rows(ps, f)
        .into_iter()
        .map(|r| match r {
            Row::Head(s) => format!("[{s}]"),
            Row::Agent(i) => ps[i].id.clone(),
            r => format!("{r:?}"),
        })
        .collect()
}

fn names(ps: &[Pane]) -> Vec<&str> {
    board::groups(ps).into_iter().map(|(n, _)| n).collect()
}

fn set(ps: &mut [Pane], id: &str, state: &str) {
    let p = ps.iter_mut().find(|p| p.id == id).unwrap();
    p.state = state.into();
}

fn list_view<'a>(ps: &'a [Pane], th: &'a Theme, nav: &'a Nav) -> View<'a> {
    View {
        active: "%8",
        list: Some(nav),
        ..view(ps, &[], th, 0, 0)
    }
}

fn nav_on(ps: &[Pane], sel: Sel, f: &[&str], room: usize) -> Nav {
    let mut n = Nav::new(sel, folds(f));
    n.settle(ps, room);
    n
}

/// D3: the hot band (a ▲ or ✕) first, then the rest, each by session name;
/// inside, (state rank, pane id), the id numerically.
#[test]
fn groups_sort_in_two_bands_by_name() {
    let ps = fleet();
    assert_eq!(names(&ps), ["api", "infra", "web", "ml"]);
    assert_eq!(
        shown(&ps, &folds(&[])),
        [
            "[api]", "%3", "%4", "%5", "%6", "[infra]", "%11", "%12", "%13", "[web]", "%7", "%8", "%9", "%10", "[ml]", "%14", "%15"
        ]
    );
}

/// No clock in the sort: ages move nothing, and a state change moves a group
/// only when it crosses a band.
#[test]
fn a_group_moves_only_across_bands() {
    let mut ps = fleet();
    let before = shown(&ps, &folds(&[]));
    for (i, p) in ps.iter_mut().enumerate() {
        p.since = Some(99_999 - i as u64 * 1000);
        p.idle_since = Some(i as u64);
    }
    assert_eq!(shown(&ps, &folds(&[])), before, "ages reorder nothing");

    // inside a band: running to done reorders the group's rows, not the groups
    set(&mut ps, "%14", "done");
    assert_eq!(names(&ps), ["api", "infra", "web", "ml"]);
    set(&mut ps, "%4", "done");
    assert_eq!(names(&ps), ["api", "infra", "web", "ml"], "api still holds a ▲");
    // across: ml gains a ▲ and joins the hot band, by name
    set(&mut ps, "%15", "needs-input");
    assert_eq!(names(&ps), ["api", "infra", "ml", "web"]);
    // and api's ▲ answered drops it to the calm band
    set(&mut ps, "%3", "running");
    assert_eq!(names(&ps), ["infra", "ml", "web", "api"]);
}

/// D4: a folded group shows its header and every ▲ and ✕ row, and nothing
/// unfolds by itself.
#[test]
fn a_fold_never_hides_an_urgent_row_and_never_opens_by_itself() {
    let mut ps = fleet();
    let f = folds(&["api", "infra", "web", "ml"]);
    assert_eq!(shown(&ps, &f), ["[api]", "%3", "[infra]", "%11", "[web]", "%7", "[ml]"]);
    // a folded session that gains a ▲ stays folded and shows the row
    let mut nav = nav_on(&ps, Sel::Head("ml".into()), &["api", "infra", "web", "ml"], 20);
    set(&mut ps, "%15", "needs-input");
    nav.settle(&ps, 20);
    assert!(nav.folds.contains("ml"), "nothing unfolds by itself");
    assert_eq!(
        shown(&ps, &nav.folds),
        ["[api]", "%3", "[infra]", "%11", "[ml]", "%15", "[web]", "%7"]
    );
    assert_eq!(nav.sel, Sel::Head("ml".into()), "the selection stays on its header");
    // a new session appears unfolded
    ps.push(pane("%20", "new:0", "fresh", "running"));
    nav.settle(&ps, 20);
    assert!(shown(&ps, &nav.folds).ends_with(&["[new]".to_string(), "%20".to_string()]));
}

/// D4: a digit or `u` that lands in a folded group unfolds it.
#[test]
fn a_digit_or_u_landing_in_a_folded_group_unfolds_it() {
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Head("api".into()), &["api", "infra", "web", "ml"], 20);
    // rows: [api] %3 [infra] %11 [web] %7 [ml]; agent 2 on screen is %11
    nav.act(Act::Tile(2), &ps, 20);
    assert_eq!(nav.sel, Sel::Pane("%11".into()));
    assert!(!nav.folds.contains("infra"), "the digit unfolded infra");
    assert!(nav.folds.contains("api") && nav.folds.contains("web"), "and nothing else");

    let mut nav = nav_on(&ps, Sel::Pane("%3".into()), &["web"], 20);
    nav.act(Act::NextNeeds, &ps, 20);
    assert_eq!(nav.sel, Sel::Pane("%7".into()));
    assert!(!nav.folds.contains("web"), "u unfolded web");
    nav.act(Act::NextNeeds, &ps, 20);
    assert_eq!(nav.sel, Sel::Pane("%3".into()), "u wraps");
}

/// D5: the selection is an identity. A refresh that moves its group keeps it,
/// on the screen row it had, so the viewport moves and not the cursor.
#[test]
fn the_selection_survives_a_refresh_that_moves_it() {
    let mut ps = fleet();
    let room = 6;
    let mut nav = nav_on(&ps, Sel::Pane("%14".into()), &[], room);
    // rows 0..17; %14 is row 15, so the viewport ends on it
    let screen = |n: &Nav, ps: &[Pane]| {
        let rows = board::rows(ps, &n.folds);
        n.find(&rows, ps) - n.top
    };
    let held = screen(&nav, &ps);
    // ml gains a ▲: it jumps into the hot band, above web
    set(&mut ps, "%15", "needs-input");
    nav.settle(&ps, room);
    assert_eq!(nav.sel, Sel::Pane("%14".into()));
    assert_eq!(screen(&nav, &ps), held, "the cursor holds its screen row");
    let rows = board::rows(&ps, &nav.folds);
    assert_eq!(
        rows[nav.find(&rows, &ps)],
        Row::Agent(ps.iter().position(|p| p.id == "%14").unwrap())
    );
}

/// D5's fallbacks for a pane that is gone: the row now at its place in the
/// same session, else that session's header, else the first row.
#[test]
fn a_gone_pane_gives_way_to_its_neighbour_then_its_header_then_the_top() {
    let mut ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%4".into()), &[], 30);
    ps.retain(|p| p.id != "%4");
    nav.settle(&ps, 30);
    assert_eq!(nav.sel, Sel::Pane("%5".into()), "the row now at its place");
    // the last of its group gone: that place is past the group's end
    let mut nav = nav_on(&ps, Sel::Pane("%6".into()), &[], 30);
    ps.retain(|p| p.id != "%6");
    nav.settle(&ps, 30);
    assert_eq!(nav.sel, Sel::Head("api".into()), "its header");
    // the whole session gone
    let mut nav = nav_on(&ps, Sel::Pane("%15".into()), &[], 30);
    ps.retain(|p| !p.wher.starts_with("ml:"));
    nav.settle(&ps, 30);
    assert_eq!(nav.sel, Sel::Head("api".into()), "the first row");
}

/// D5: folding the selected agent's group moves the selection to the header,
/// unless the agent is ▲ or ✕ and stays on screen.
#[test]
fn folding_moves_the_selection_to_the_header_unless_it_stays_visible() {
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%4".into()), &[], 30);
    nav.act(Act::Fold, &ps, 30);
    assert_eq!(nav.sel, Sel::Head("api".into()));
    assert!(nav.folds.contains("api"));
    nav.act(Act::Toggle, &ps, 30);
    assert!(!nav.folds.contains("api"), "space unfolds");
    assert_eq!(nav.sel, Sel::Head("api".into()), "and stays on the header");

    let mut nav = nav_on(&ps, Sel::Pane("%3".into()), &[], 30);
    nav.act(Act::Toggle, &ps, 30);
    assert_eq!(nav.sel, Sel::Pane("%3".into()), "a ▲ stays selected");
    nav.act(Act::FoldAll, &ps, 30);
    assert_eq!(nav.folds, folds(&["api", "infra", "ml", "web"]));
    nav.act(Act::UnfoldAll, &ps, 30);
    assert!(nav.folds.is_empty());

    // `l` on a folded header unfolds; on an open one it enters the group
    let mut nav = nav_on(&ps, Sel::Head("web".into()), &["web"], 30);
    nav.act(Act::Unfold, &ps, 30);
    assert!(nav.folds.is_empty());
    assert_eq!(nav.sel, Sel::Head("web".into()));
    nav.act(Act::Unfold, &ps, 30);
    assert_eq!(nav.sel, Sel::Pane("%7".into()));
}

/// D6: the digits are the first nine agent rows on screen, renumbered as the
/// list scrolls; `[` `]` scroll a screenful and the selection only clamps.
#[test]
fn digits_follow_the_screen_and_a_page_only_clamps_the_selection() {
    let ps = fleet();
    let room = 6;
    let mut nav = nav_on(&ps, Sel::Pane("%4".into()), &[], room);
    assert_eq!(nav.top, 0);
    nav.act(Act::Tile(1), &ps, room);
    assert_eq!(nav.sel, Sel::Pane("%3".into()));
    nav.act(Act::PageDown, &ps, room);
    // rows 6..12 on screen: %11 %12 %13 [web] %7 %8
    assert_eq!(nav.top, 6);
    assert_eq!(nav.sel, Sel::Pane("%11".into()), "clamped to the first row on screen");
    nav.act(Act::Tile(4), &ps, room);
    assert_eq!(nav.sel, Sel::Pane("%7".into()), "the 4th agent row on screen, past the header");
    nav.act(Act::Tile(9), &ps, room);
    assert_eq!(nav.sel, Sel::Pane("%7".into()), "no ninth row on screen: nothing");
    nav.act(Act::PageDown, &ps, room);
    assert_eq!(nav.top, 11, "stops on the last full screen");
    assert_eq!(nav.sel, Sel::Pane("%8".into()), "clamped: %7 scrolled off the top");
    nav.act(Act::Down, &ps, room);
    nav.act(Act::PageDown, &ps, room);
    assert_eq!(nav.top, 11);
    assert_eq!(nav.sel, Sel::Pane("%9".into()), "still on screen, so it stays");
    nav.act(Act::PageUp, &ps, room);
    assert_eq!(nav.top, 5);
    assert_eq!(nav.sel, Sel::Pane("%7".into()), "clamped to the last row on screen");
    nav.act(Act::PageUp, &ps, room);
    assert_eq!(nav.top, 0);
    assert_eq!(nav.sel, Sel::Head("infra".into()), "a header row is a row too");

    // and the rendered digits are those same rows
    let th = Theme::fixed();
    let mut nav = nav_on(&ps, Sel::Pane("%4".into()), &[], 6);
    nav.act(Act::PageDown, &ps, 6);
    let v = list_view(&ps, &th, &nav);
    let out = draw(60, 10, |f| board::render(f, f.area(), &v));
    let r = rows(&out);
    assert!(r[2].contains(" 1 ✕") && r[2].contains("terraform"), "{out}");
    assert!(r[6].contains(" 4 ▲") && r[6].contains("docs"), "{out}");
    assert!(r[1].contains("7-12 of 17"), "N-M of T when the list does not fit:\n{out}");
}

/// `n` `p` step through the headers, wrapping; `⇥` skips them; `g` `G` ends.
#[test]
fn n_and_p_walk_the_headers_and_tab_skips_them() {
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%4".into()), &[], 30);
    let mut seen = vec![];
    for _ in 0..5 {
        nav.act(Act::NextSess, &ps, 30);
        seen.push(nav.sel.clone());
    }
    let h = |s: &str| Sel::Head(s.into());
    assert_eq!(seen, [h("infra"), h("web"), h("ml"), h("api"), h("infra")]);
    nav.act(Act::PrevSess, &ps, 30);
    assert_eq!(nav.sel, h("api"));
    nav.act(Act::PrevSess, &ps, 30);
    assert_eq!(nav.sel, h("ml"), "p wraps");
    nav.act(Act::Next, &ps, 30);
    assert_eq!(nav.sel, Sel::Pane("%14".into()));
    nav.act(Act::Next, &ps, 30);
    nav.act(Act::Next, &ps, 30);
    assert_eq!(nav.sel, Sel::Pane("%3".into()), "⇥ skips [api] as it wraps");
    nav.act(Act::Prev, &ps, 30);
    assert_eq!(nav.sel, Sel::Pane("%15".into()));
    nav.act(Act::First, &ps, 30);
    assert_eq!(nav.sel, h("api"));
    nav.act(Act::Up, &ps, 30);
    assert_eq!(nav.sel, h("api"), "j k stop at the ends");
    nav.act(Act::Last, &ps, 30);
    assert_eq!(nav.sel, Sel::Pane("%15".into()));
}

/// D19: the footer follows the focus and lists only what works there.
#[test]
fn the_footer_follows_the_focus() {
    let g = Glyphs::unicode();
    let f = |focus| -> Vec<String> { board::hints(&g, true, focus).into_iter().map(|(k, v)| format!("{k} {v}")).collect() };
    assert_eq!(
        f(Focus::Agent(true)),
        [
            "↵ jump",
            "a answer",
            "r review",
            "f panel",
            "i info",
            "space fold",
            "/ find",
            "x kill",
            "? keys",
            "q close"
        ]
    );
    assert_eq!(
        f(Focus::Head(false)),
        [
            "space fold",
            "l open",
            "- fold all",
            "= open all",
            "n p sessions",
            "/ find",
            "? keys",
            "q close"
        ]
    );
    assert_eq!(f(Focus::Head(true))[0], "space unfold");
    // the tiles keep today's footer, and #228's actions
    let t: Vec<String> = board::hints(&g, false, Focus::Agent(false))
        .into_iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect();
    assert_eq!(
        t,
        ["↵ jump", "a ask", "r review", "f panel", "i info", "x kill", "? keys", "q close"]
    );

    // and the drawn footer is the one for the selection
    let ps = fleet();
    let th = Theme::fixed();
    let nav = nav_on(&ps, Sel::Head("web".into()), &["web"], 26);
    let out = draw(100, 30, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
    assert!(rows(&out)[29].starts_with(" space unfold   l open   - fold all"), "{out}");
    assert_eq!(nav.focus(&ps), Focus::Head(true));
}

/// D17: the selected row is ▎ plus the `sel` band, its text in fg; under
/// NO_COLOR the band is reverse video, which reads with no colour at all.
#[test]
fn the_selected_row_is_a_bar_and_a_band() {
    for th in [Theme::fixed(), Theme::no_color()] {
        let ps = fleet();
        let nav = nav_on(&ps, Sel::Pane("%6".into()), &[], 26);
        let b = render_to_buffer(100, 30, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
        let marks: Vec<(u16, u16)> = (0..30)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .filter(|&(x, y)| b[(x, y)].symbol() == "▎")
            .collect();
        assert_eq!(marks.len(), 1, "one selection");
        let (x, y) = marks[0];
        let line: String = (0..95).map(|x| b[(x, y)].symbol()).collect();
        assert!(line.contains("auth"), "{line}");
        let k = line.find("Pushed").unwrap();
        let k = line[..k].chars().count() as u16;
        if th.sel == Color::Reset {
            assert!(b[(k, y)].modifier.contains(Modifier::REVERSED), "NO_COLOR keeps reverse video");
        } else {
            assert!(!b[(k, y)].modifier.contains(Modifier::REVERSED), "the band is a background");
            assert_eq!(b[(k, y)].bg, th.sel);
        }
        assert_eq!(b[(k, y)].fg, th.value, "never the dim role");
        assert!(!b[(x, y)].modifier.contains(Modifier::REVERSED), "the bar itself is not reversed");
    }
}

#[test]
fn list_goldens() {
    let th = Theme::fixed();
    let ps = fleet();
    for (w, h) in SIZES {
        let room = h as usize - 4;
        let nav = nav_on(&ps, Sel::Pane("%3".into()), &[], room);
        let out = draw(w, h, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
        golden(&format!("board-list-{w}"), &out);
        assert!(out.contains("13 agents · 4 sessions"), "{out}");
        assert!(out.contains("idle"), "an idle row says idle:\n{out}");

        let nav = nav_on(&ps, Sel::Head("infra".into()), &["web", "infra", "ml"], room);
        let out = draw(w, h, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
        golden(&format!("board-folded-{w}"), &out);
        assert!(out.contains("2 folded"), "{out}");
    }
    // the same fleet behind `v`
    let ord = board::order(&ps);
    let v = View {
        active: "%8",
        ..view(&ps, &[], &th, ord.iter().position(|&i| ps[i].id == "%3").unwrap(), 0)
    };
    golden("board-tiles-100", &draw(100, 30, |f| board::render(f, f.area(), &v)));
}

/// The extra cells at the four widths the issue names: the stat, the review
/// mark and the branch, dropped from the right as the row narrows.
#[test]
fn list_extra_goldens() {
    let th = Theme::fixed();
    let ps = fleet();
    for w in [40u16, 60, 80, 120] {
        let nav = nav_on(&ps, Sel::Pane("%3".into()), &[], 20);
        let out = draw(w, 24, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
        golden(&format!("board-extra-{w}"), &out);
        assert!(w < 60 || out.contains("+42 -7"), "the stat shows at {w}:\n{out}");
    }
}

/// A row with a branch and a row without end their age in the same column: the
/// board and the buffer measure ⑂ with the same unicode-width.
#[test]
fn rows_with_and_without_a_branch_end_at_the_same_column() {
    use unicode_width::UnicodeWidthStr;
    let th = Theme::fixed();
    let mut ps = fleet();
    // %12 reads "no repo" and shows no ⑂; %4 shows the branch
    ps.iter_mut().find(|p| p.id == "%12").unwrap().loc.clear();
    let nav = nav_on(&ps, Sel::Pane("%3".into()), &[], 20);
    let out = draw(80, 24, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
    let rs = rows(&out);
    let row = |name: &str| rs.iter().find(|r| r.contains(name)).unwrap_or_else(|| panic!("{name}:\n{out}"));
    let (with, without) = (row(" dns "), row(" tests "));
    assert!(with.contains("no repo") && !with.contains('⑂'), "{with}");
    assert!(without.contains('⑂') || without.contains("+42 -7"), "{without}");
    assert_eq!(with.width(), without.width());
    // the age is the last text before the active-mark column and the border
    let end = |r: &str| r.trim_end_matches('│').trim_end_matches(['◂', ' ']).width();
    assert_eq!(end(with), end(without), "{with}\n{without}");
}

/// At 40 columns the cells may get no room at all; the message and the age stay.
#[test]
fn at_40_columns_the_cells_never_starve_the_message_or_the_age() {
    let th = Theme::fixed();
    let ps = fleet();
    let nav = nav_on(&ps, Sel::Pane("%3".into()), &[], 20);
    let out = draw(40, 24, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
    let r = rows(&out);
    let server = r.iter().find(|l| l.contains(" server ")).unwrap();
    assert!(server.contains("Do you") && server.contains("4m12s"), "{server}");
    let ui = r.iter().find(|l| l.contains(" ui ")).unwrap();
    assert!(ui.contains("Bash") && ui.contains("3m40s"), "{ui}");
}

/// No row the list draws runs past the area, at any width the board accepts.
#[test]
fn no_list_row_exceeds_the_area() {
    use unicode_width::UnicodeWidthStr;
    let ps = fleet();
    for th in [Theme::fixed(), {
        let mut t = Theme::fixed();
        t.g = Glyphs::ascii();
        t
    }] {
        for w in [40u16, 41, 47, 60, 80, 100, 133, 180, 240] {
            for f in [&[][..], &["api", "web"][..]] {
                let nav = nav_on(&ps, Sel::Pane("%7".into()), f, 20);
                let out = draw(w, 24, |fr| board::render(fr, fr.area(), &list_view(&ps, &th, &nav)));
                for (i, r) in rows(&out).iter().enumerate() {
                    assert_eq!(r.width(), w as usize, "row {i} at {w}:\n{out}");
                }
                // a header's counts are never clipped by the frame
                assert!(out.contains("▲1 ●2 ✓1  │") || out.contains("!1 *2 +1  |"), "{out}");
            }
        }
    }
}

/// `fold <session>` lines round-trip, after the selection, and are read strictly.
#[test]
fn board_state_keeps_the_folds() {
    let d = scratch("state-folds");
    let p = d.join("board.state");
    let s = board::Saved {
        sel: Some("%7".into()),
        folds: folds(&["web", "my session"]),
        filter: None,
    };
    board::write_state(&p, &s).unwrap();
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "cockpit-board-state 1\nsel %7\nfold my session\nfold web\n"
    );
    assert_eq!(board::read_state(&p), Ok(Some(s)));
    for (body, why) in [
        ("cockpit-board-state 1\nfold web\nfold web\n", "a fold twice"),
        ("cockpit-board-state 1\nfold \n", "an empty name"),
        ("cockpit-board-state 1\nfold web\nsel %1\n", "the selection after a fold"),
        ("cockpit-board-state 1\nfold a\tb\n", "a control character"),
    ] {
        std::fs::write(&p, body).unwrap();
        assert!(board::read_state(&p).is_err(), "{why}");
    }
    std::fs::write(&p, "cockpit-board-state 1\nfold web\n").unwrap();
    assert_eq!(
        board::read_state(&p),
        Ok(Some(board::Saved {
            sel: None,
            folds: folds(&["web"]),
            filter: None,
        })),
        "folds with no selection"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// D5 on open reads the first ▲ in the list's order, not the tiles'.
#[test]
fn the_list_opens_on_its_first_blocked_agent() {
    let ps = fleet();
    let lo = board::list_order(&ps);
    assert_eq!(ps[lo[board::open_sel(&ps, &lo, None, "%8")]].id, "%3");
    assert_eq!(ps[lo[board::open_sel(&ps, &lo, Some("%13"), "%8")]].id, "%13");
}

/// The keys mean what the layout on screen says (D2, 9.1).
#[test]
fn h_l_and_space_fold_in_the_list_and_move_in_the_tiles() {
    let k = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
    assert_eq!(board::action(k('h'), true), Some(Act::Fold));
    assert_eq!(board::action(k('l'), true), Some(Act::Unfold));
    assert_eq!(board::action(k('h'), false), Some(Act::Left));
    assert_eq!(board::action(k(' '), false), None);
    assert_eq!(board::action(k('n'), false), None);
    assert_eq!(board::action(k('v'), false), Some(Act::Layout));
    assert_eq!(board::action(k('v'), true), Some(Act::Layout));
    // esc is never the only way out (D22)
    assert_eq!(board::action(k('q'), true), Some(Act::Quit));
}

// ------------------------------------------- extra actions and the confirm (#228)

/// The fleet with each window named as its title, which a kill names.
fn named() -> Vec<Pane> {
    let mut ps = fleet();
    for p in &mut ps {
        p.name = p.title.clone();
        p.win = format!("@{}", p.id.trim_start_matches('%'));
    }
    ps
}

fn id(ps: &[Pane], id: &str) -> Pane {
    ps.iter().find(|p| p.id == id).unwrap().clone()
}

fn key(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}

fn kill() -> &'static board::Extra {
    board::extra('x').unwrap()
}

/// Each entry's key reaches it in both layouts, so no table row takes it
/// first; and it is none of D6's, D7's `/`, D8's `a` or the confirm's own.
#[test]
fn every_extra_has_a_key_of_its_own() {
    let mut seen = HashSet::new();
    for e in EXTRAS {
        assert!(seen.insert(e.key), "{} is registered twice", e.key);
        assert!(!"/a0123456789nyu[]".contains(e.key), "{} is a key D6-D8 take", e.key);
        assert!(!e.verb.is_empty() && e.verb.bytes().all(|b| b.is_ascii_lowercase()), "{}", e.verb);
        for list in [false, true] {
            assert_eq!(board::action(key(KeyCode::Char(e.key)), list), Some(Act::Extra(e.key)));
        }
    }
    assert_eq!(
        EXTRAS.iter().map(|e| (e.key, e.verb)).collect::<Vec<_>>(),
        [('x', "kill"), ('o', "resume")]
    );
}

/// On an agent the footer and `?` list every entry; a header lists none.
#[test]
fn the_footer_and_the_help_list_the_extras() {
    let g = Glyphs::unicode();
    for list in [false, true] {
        // an ended agent with a session id is offered every entry (#301)
        let ks = board::hints_on(&g, list, Focus::Agent(false), &ended("%3", SID));
        let help = board::help_rows(&g, list);
        for e in EXTRAS {
            assert!(ks.contains(&(e.key.to_string(), e.label.to_string())), "{ks:?}");
            assert!(help.contains(&(e.key.to_string(), e.help.to_string())), "{help:?}");
        }
        // after the act keys, before `?`: the cut drops them first
        let at = |k: &str| ks.iter().position(|(x, _)| x == k).unwrap();
        assert!(at("i") < at("x") && at("x") < at("?"), "{ks:?}");
    }
    let head = board::hints(&g, true, Focus::Head(false));
    assert!(
        EXTRAS.iter().all(|e| !head.iter().any(|(k, _)| *k == e.key.to_string())),
        "{head:?}"
    );
}

/// D18: `y n esc ? q ^C` and nothing else; `↵` never sends.
#[test]
fn the_confirm_owns_the_keys() {
    use board::Answer;
    assert_eq!(board::confirm_key(key(KeyCode::Char('y'))), Some(Answer::Yes));
    assert_eq!(board::confirm_key(key(KeyCode::Char('n'))), Some(Answer::Back));
    assert_eq!(board::confirm_key(key(KeyCode::Esc)), Some(Answer::Back));
    assert_eq!(board::confirm_key(key(KeyCode::Char('?'))), Some(Answer::Help));
    assert_eq!(board::confirm_key(key(KeyCode::Char('q'))), Some(Answer::Close));
    assert_eq!(
        board::confirm_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Some(Answer::Close)
    );
    let mut dead: Vec<KeyEvent> = ('a'..='z')
        .chain('A'..='Z')
        .chain('0'..='9')
        .chain("[]/ -=".chars())
        .filter(|c| !"ynq".contains(*c))
        .map(|c| key(KeyCode::Char(c)))
        .collect();
    dead.extend(
        [
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
        ]
        .map(key),
    );
    dead.push(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
    dead.push(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    for k in dead {
        assert_eq!(board::confirm_key(k), None, "{k:?} does something on the confirm");
    }
    // the footer is those keys, in that order (D19)
    let c = board::Confirm::open(kill(), &id(&named(), "%3")).unwrap();
    let ks: Vec<String> = board::confirm_hints(&c).into_iter().map(|(k, v)| format!("{k} {v}")).collect();
    assert_eq!(ks, ["y kill", "n back", "esc back", "? keys", "q close"]);
    let help: Vec<String> = board::confirm_help_rows().into_iter().map(|(k, _)| k).collect();
    assert!(
        ["y", "n esc", "?", "q ^C"].iter().all(|k| help.contains(&k.to_string())),
        "{help:?}"
    );
}

/// `x` asks, naming the pane that dies (#301), and `y` exits with `kill %N`
/// and `%N @W`, the pane and its window's id, as `.expect`; an entry with no
/// confirm exits at once.
#[test]
fn y_writes_the_verb_and_the_expected_name() {
    let ps = named();
    let c = board::Confirm::open(kill(), &id(&ps, "%3")).unwrap();
    assert_eq!(c.ask, "kill pane %3 (api:0 server)?");
    assert_eq!(
        c.exit(),
        board::Exit {
            line: "kill %3".into(),
            expect: Some("%3 @3".into())
        }
    );
    // #288: `R` is gone; an entry with no confirm still exits at once
    assert!(board::extra('R').is_none());
    let r = board::Extra {
        key: 'Z',
        label: "zap",
        help: "",
        verb: "zap",
        confirm: None,
        expect: None,
        offered: |_| true,
        not_here: "",
    };
    assert!(board::Confirm::open(&r, &id(&ps, "%3")).is_none());
    assert_eq!(
        board::at_once(&r, &id(&ps, "%3")),
        board::Exit {
            line: "zap %3".into(),
            expect: None
        }
    );
    // the expected value lands beside the action file, as one line
    let d = scratch("expect-y");
    let f = d.join("act");
    cockpit_tui::panel::action::write_expect(&f, c.expect.as_deref().unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(d.join("act.expect")).unwrap(), "%3 @3\n");
}

/// `.expect` follows `.text`'s rules: control characters out (here `\t` and
/// `\n` too, as it is one line), a size cap that refuses rather than cuts,
/// created fresh, never through a symlink, mode 0600.
#[test]
fn the_expect_file_rules() {
    use cockpit_tui::panel::action::{EXPECT_MAX, expect_body, write_expect};
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(expect_body("se\x1b[31mrv\ter\n\x7fx").as_deref(), Some("se[31mrverx\n"));
    // a C1 character is not an ASCII control: kept, as the shell reader keeps it
    assert_eq!(expect_body("a\u{9b}b\u{85}").as_deref(), Some("a\u{9b}b\u{85}\n"));
    // an empty value would check nothing: refused, as is one that is all control
    assert_eq!(expect_body(""), None);
    assert_eq!(expect_body("\t\n\x1b"), None);
    assert_eq!(expect_body(&"w".repeat(EXPECT_MAX)).map(|b| b.len()), Some(EXPECT_MAX + 1));
    assert_eq!(expect_body(&"w".repeat(EXPECT_MAX + 1)), None);
    let d = scratch("expect-rules");
    let f = d.join("act");
    assert!(write_expect(&f, &"w".repeat(EXPECT_MAX + 1)).is_err());
    assert!(!d.join("act.expect").exists(), "a refused value leaves no file");
    write_expect(&f, "api").unwrap();
    let m = std::fs::metadata(d.join("act.expect")).unwrap();
    assert_eq!(m.permissions().mode() & 0o777, 0o600);
    // a stale file is never overwritten, and a symlink is never followed
    assert!(write_expect(&f, "api").is_err());
    let g = d.join("g");
    std::os::unix::fs::symlink(d.join("victim"), d.join("g.expect")).unwrap();
    assert!(write_expect(&g, "api").is_err());
    assert!(!d.join("victim").exists());
}

/// D18: a refresh while the confirm is open updates the ages, but no row
/// moves, folds or goes, even when states, sessions and panes change under it.
#[test]
fn a_refresh_while_asked_moves_no_row() {
    let old = named();
    let c = board::Confirm::open(kill(), &id(&old, "%4")).unwrap();
    let mut new = named();
    set(&mut new, "%4", "needs-input");
    set(&mut new, "%3", "done");
    new[7].wher = "zzz:9".into();
    new.retain(|p| p.id != "%12");
    new.push(pane("%40", "aaa:0", "newcomer", "needs-input"));
    for p in &mut new {
        p.since = p.since.map(|s| s + 7);
    }
    let Ok(held) = board::held(&c, &old, &new) else {
        panic!("a pane that is there closed the confirm")
    };
    let f = folds(&["ml"]);
    assert_eq!(shown(&held, &f), shown(&old, &f));
    assert_eq!(board::rows(&held, &f), board::rows(&old, &f));
    assert_eq!(held.iter().find(|p| p.id == "%4").unwrap().since, Some(70));
}

/// D18: the target's pane goes while the confirm is open: it closes itself
/// with a `✕` note, which stays until a key, and never moves to another agent.
#[test]
fn a_gone_target_closes_the_confirm_with_a_note() {
    let old = named();
    let c = board::Confirm::open(kill(), &id(&old, "%3")).unwrap();
    let new: Vec<Pane> = named().into_iter().filter(|p| p.id != "%3").collect();
    let Err(msg) = board::held(&c, &old, &new) else {
        panic!("the confirm stayed open on a gone pane")
    };
    assert_eq!(msg, "✕ nothing done: api:0 is gone, kill not sent");
    assert!(board::failure(&msg));
}

fn confirm_view<'a>(ps: &'a [Pane], th: &'a Theme, nav: &'a Nav, c: &'a board::Confirm) -> View<'a> {
    View {
        confirm: Some(c),
        ..list_view(ps, th, nav)
    }
}

#[test]
fn confirm_goldens() {
    let th = Theme::fixed();
    let ps = named();
    let nav = nav_on(&ps, Sel::Pane("%3".into()), &[], 20);
    let c = board::Confirm::open(kill(), &id(&ps, "%3")).unwrap();
    for (w, h) in [(60u16, 24u16), (100, 30)] {
        let v = confirm_view(&ps, &th, &nav, &c);
        let out = draw(w, h, |f| board::render(f, f.area(), &v));
        golden(&format!("board-confirm-{w}"), &out);
        let r = rows(&out);
        assert!(
            out.contains("kill pane %3 (api:0 server)?") && out.contains("sessions · held"),
            "{out}"
        );
        assert!(r[h as usize - 1].starts_with(" y kill"), "{out}");
    }
    // the tiles draw the same confirm
    let ord = board::order(&ps);
    let v = View {
        confirm: Some(&c),
        ..view(&ps, &[], &th, ord.iter().position(|&i| ps[i].id == "%3").unwrap(), 0)
    };
    let out = draw(100, 30, |f| board::render(f, f.area(), &v));
    assert!(
        out.contains("kill pane %3 (api:0 server)?") && rows(&out)[29].starts_with(" y kill"),
        "{out}"
    );
    // its `?` is its own keys
    let mut v = confirm_view(&ps, &th, &nav, &c);
    v.help = Some(0);
    let out = draw(100, 30, |f| board::render(f, f.area(), &v));
    assert!(out.contains("on the confirm") && !out.contains("fold all"), "{out}");
}

/// Agent windows share a name (`claude`): the kill's `.expect` carries the
/// window id, so a pane moved into a namesake window fails the wrapper's check.
#[test]
fn two_windows_with_one_name_expect_different_ids() {
    let mut a = pane("%8", "api:1", "one", "running");
    let mut b = pane("%9", "api:2", "two", "running");
    (a.name, a.win) = ("claude".into(), "@8".into());
    (b.name, b.win) = ("claude".into(), "@9".into());
    let (ca, cb) = (board::Confirm::open(kill(), &a).unwrap(), board::Confirm::open(kill(), &b).unwrap());
    assert_eq!(
        (ca.ask.as_str(), cb.ask.as_str()),
        ("kill pane %8 (api:1 claude)?", "kill pane %9 (api:2 claude)?")
    );
    assert_eq!((ca.expect.as_deref(), cb.expect.as_deref()), (Some("%8 @8"), Some("%9 @9")));
}

/// A pane with no window id, or a malformed one, is never offered a kill:
/// the confirm does not open and a `✕` note says why.
#[test]
fn no_window_id_refuses_the_kill() {
    for w in ["", "claude", "@", "@9x", "%9"] {
        let mut p = pane("%8", "api:1", "one", "running");
        p.win = w.into();
        let msg = board::refused(kill(), &p).unwrap_or_else(|| panic!("{w:?} was offered a kill"));
        assert_eq!(msg, "✕ nothing done: api:1 has no window id to check, kill not sent");
        assert!(board::failure(&msg));
    }
    let mut p = pane("%8", "api:1", "one", "running");
    p.win = "@12".into();
    assert_eq!(board::refused(kill(), &p), None);
}

/// The list's top line drops whole parts, never a word: the session count
/// first, then the agent count; the needs and error counts stay longest.
#[test]
fn the_top_line_drops_whole_parts_in_priority_order() {
    let th = Theme::fixed();
    let ps = fleet();
    let nav = nav_on(&ps, Sel::Pane("%3".into()), &[], 20);
    for (w, want) in [
        (60u16, " board · 13 agents · 4 sessions · 2 needs you · 1 error"),
        (55, " board · 13 agents · 4 sessions · 2 needs you · 1 error"),
        (54, " board · 13 agents · 2 needs you · 1 error"),
        (42, " board · 13 agents · 2 needs you · 1 error"),
        (41, " board · 2 needs you · 1 error"),
        (40, " board · 2 needs you · 1 error"),
    ] {
        let out = draw(w, 24, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
        assert_eq!(rows(&out)[0].trim_end(), want, "at {w}");
    }
}

/// A fold is saved while its tmux session exists, agents or not, and dropped
/// once the session is gone.
#[test]
fn a_fold_outlives_its_agents_but_not_its_session() {
    let ss = folds(&["api", "web", "scratch"]);
    assert_eq!(
        board::kept_folds(&folds(&["web", "gone", "scratch"]), &ss),
        folds(&["scratch", "web"])
    );
    assert_eq!(board::kept_folds(&folds(&["web"]), &folds(&[])), folds(&[]));
}

// ------------------------------------------------------------- find (D7, D21)

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// The list as text under a filter, the field open or not.
fn found(nav: &Nav, ps: &[Pane]) -> Vec<String> {
    nav.rows(ps)
        .into_iter()
        .map(|r| match r {
            Row::Head(s) => format!("[{s}]"),
            Row::Agent(i) => ps[i].id.clone(),
            r => format!("{r:?}"),
        })
        .collect()
}

/// Type `s` into an open field, one key at a time, as the board does.
fn typed(nav: &mut Nav, ps: &[Pane], s: &str) {
    for c in s.chars() {
        let k = board::find_key(key(KeyCode::Char(c))).unwrap();
        assert_eq!(k, board::FindKey::Type(c));
        nav.find_act(k, ps, 26);
    }
}

/// D7: every term must match, each in any field, ignoring case; the state
/// reads as its word.
#[test]
fn find_terms_all_match_over_every_field() {
    let ps = fleet();
    let hits = |f: &str| -> Vec<&str> {
        let mut v: Vec<&str> = ps.iter().filter(|p| board::find_match(p, f)).map(|p| p.id.as_str()).collect();
        v.sort_by_key(|id| id[1..].parse::<u32>().unwrap());
        v
    };
    assert_eq!(hits("test"), ["%3", "%4", "%8", "%15"], "title, message and doing line");
    assert_eq!(hits("test api"), ["%3", "%4"], "the session narrows it");
    assert_eq!(hits("TEST Running"), ["%4", "%8"], "case and the state word");
    assert_eq!(hits("needs"), ["%3", "%7"]);
    assert_eq!(hits("error"), ["%11"]);
    assert_eq!(hits("done"), ["%6", "%9", "%15"]);
    assert_eq!(hits("idle"), ["%10", "%13"]);
    assert_eq!(hits("infra 2"), ["%13"], "the window index");
    assert_eq!(hits("test nothing-here"), Vec::<&str>::new(), "every term, not any");
    assert_eq!(hits("  ").len(), ps.len(), "no term matches everything");
    // a term never spans two fields
    assert_eq!(hits("api\nserver"), ["%3"], "whitespace splits terms, newline included");
    assert!(hits("0server").is_empty());
}

/// A group with no match hides; one with a match shows unfolded while the
/// field is open, and its fold is kept for after.
#[test]
fn find_hides_empty_groups_and_opens_folded_ones_for_the_field() {
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%3".into()), &["api", "ml", "web"], 26);
    nav.open_find(&ps, 26);
    typed(&mut nav, &ps, "test");
    assert_eq!(found(&nav, &ps), ["[api]", "%3", "%4", "[web]", "%8", "[ml]", "%15"]);
    assert_eq!(nav.folds, folds(&["api", "ml", "web"]), "the folds are kept");
    // kept with ↵, the folds apply again, and the pick's group opens (D4)
    nav.find_act(board::FindKey::Keep, &ps, 26);
    assert!(!nav.field);
    assert_eq!(nav.sel, Sel::Pane("%3".into()));
    assert_eq!(nav.folds, folds(&["ml", "web"]));
    assert_eq!(found(&nav, &ps), ["[api]", "%3", "%4", "[web]", "[ml]"]);
    // and clearing it brings every group back, folds as they were
    nav.clear_find(&ps, 26);
    assert_eq!(found(&nav, &ps), shown(&ps, &folds(&["ml", "web"])));
}

/// ↵ with nothing picked (an empty field, or no match) opens no folded group.
#[test]
fn find_keep_with_no_pick_opens_no_fold() {
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%3".into()), &["api", "ml", "web"], 26);
    nav.open_find(&ps, 26);
    nav.find_act(board::FindKey::Keep, &ps, 26);
    assert_eq!(nav.folds, folds(&["api", "ml", "web"]), "an empty field picks nothing");
    nav.open_find(&ps, 26);
    typed(&mut nav, &ps, "zzzz-no-such-agent");
    nav.find_act(board::FindKey::Keep, &ps, 26);
    assert_eq!(nav.folds, folds(&["api", "ml", "web"]), "no match picks nothing");
}

/// Narrowing keeps the selection while it matches, else takes the first
/// match; clearing leaves it on the agent it was on.
#[test]
fn find_selection_narrows_and_clears() {
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%8".into()), &[], 26);
    nav.open_find(&ps, 26);
    typed(&mut nav, &ps, "te");
    assert_eq!(nav.sel, Sel::Pane("%8".into()), "ui still matches");
    typed(&mut nav, &ps, "st api");
    assert_eq!(nav.sel, Sel::Pane("%3".into()), "ui is gone: the first match");
    nav.find_act(board::FindKey::Move(Act::Down), &ps, 26);
    assert_eq!(nav.sel, Sel::Pane("%4".into()), "↓ moves in the field");
    nav.find_act(board::find_key(ctrl('p')).unwrap(), &ps, 26);
    assert_eq!(nav.sel, Sel::Pane("%3".into()), "^P moves too");
    nav.find_act(board::find_key(key(KeyCode::Tab)).unwrap(), &ps, 26);
    assert_eq!(nav.sel, Sel::Pane("%4".into()), "⇥ is the next agent");
    nav.find_act(board::find_key(key(KeyCode::Backspace)).unwrap(), &ps, 26);
    assert_eq!(nav.filter, "test ap");
    nav.find_act(board::find_key(ctrl('u')).unwrap(), &ps, 26);
    assert_eq!((nav.filter.as_str(), nav.field), ("", true), "^U clears the text, the field stays");
    assert_eq!(nav.sel, Sel::Pane("%4".into()), "and the selection stays put");
    assert_eq!(found(&nav, &ps), shown(&ps, &folds(&[])));
}

/// D14: in the field every printable key is text: `q`, the act keys and the
/// registry's keys type, and never act.
#[test]
fn in_the_field_q_and_the_registry_keys_type() {
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%3".into()), &[], 26);
    nav.open_find(&ps, 26);
    let mut word = String::from("qarfi?vjk-= /1");
    word.extend(EXTRAS.iter().map(|e| e.key));
    typed(&mut nav, &ps, &word);
    assert_eq!(nav.filter, word);
    assert!(nav.field, "no key closed the field");
    // the keys that are not text
    assert_eq!(board::find_key(ctrl('c')), Some(board::FindKey::Close));
    assert_eq!(board::find_key(key(KeyCode::Esc)), Some(board::FindKey::Cancel));
    assert_eq!(board::find_key(key(KeyCode::Enter)), Some(board::FindKey::Keep));
    assert_eq!(board::find_key(key(KeyCode::F(1))), Some(board::FindKey::Help));
    assert_eq!(board::find_key(ctrl('n')), Some(board::FindKey::Move(Act::Down)));
    assert_eq!(board::find_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT)), None);
    // on the list ^C closes the board too, and `/` opens the field
    assert_eq!(board::action(ctrl('c'), true), Some(Act::Quit));
    assert_eq!(board::action(key(KeyCode::Char('/')), true), Some(Act::Find));
}

/// 7.1: esc dismisses a failure note, else closes the overlay, else the
/// field, else the kept filter, else the board.
#[test]
fn the_esc_ladder() {
    use board::EscStep::*;
    assert_eq!(board::esc_step(true, true, true, true), Note);
    assert_eq!(board::esc_step(false, true, true, true), Overlay);
    assert_eq!(board::esc_step(false, false, true, true), Field);
    assert_eq!(board::esc_step(false, false, false, true), Filter);
    assert_eq!(board::esc_step(false, false, false, false), Board);
    // the field's esc clears the filter and closes the field, in one step
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%3".into()), &[], 26);
    nav.open_find(&ps, 26);
    typed(&mut nav, &ps, "dns");
    assert_eq!(nav.sel, Sel::Pane("%12".into()));
    nav.find_act(board::FindKey::Cancel, &ps, 26);
    assert_eq!((nav.filter.as_str(), nav.field), ("", false));
    assert_eq!(nav.sel, Sel::Pane("%12".into()), "the selection stays where find took it");
}

/// The field and its rule take two rows from the list while it is open.
#[test]
fn the_open_field_takes_two_rows() {
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%15".into()), &[], 10);
    assert_eq!(nav.fit(10), 10);
    nav.open_find(&ps, 10);
    assert_eq!(nav.fit(10), 8);
    let out = draw(60, 14, |f| board::render(f, f.area(), &list_view(&ps, &th_fixed(), &nav)));
    assert!(out.contains("eval"), "the selection is still on screen:\n{out}");
}

fn th_fixed() -> Theme {
    Theme::fixed()
}

fn find_view<'a>(ps: &'a [Pane], th: &'a Theme, nav: &'a Nav) -> View<'a> {
    list_view(ps, th, nav)
}

/// 6.3: the field, the match count, the narrowed list renumbered, and the
/// field's own key bar; then no match (D21).
#[test]
fn find_goldens() {
    let th = Theme::fixed();
    let ps = fleet();
    for (w, h) in SIZES {
        let mut nav = nav_on(&ps, Sel::Pane("%4".into()), &[], h as usize - 4);
        nav.open_find(&ps, h as usize - 4);
        typed(&mut nav, &ps, "test");
        let out = draw(w, h, |f| board::render(f, f.area(), &find_view(&ps, &th, &nav)));
        golden(&format!("board-find-{w}"), &out);
        assert!(out.contains(" / test▏") && out.contains(" 4 of 13 "), "{out}");
        assert!(!out.contains("infra"), "a group with no match hides:\n{out}");
        let foot = rows(&out)[h as usize - 1];
        assert!(foot.contains("↵ keep") && foot.contains("esc clear"), "{foot}");
        if w >= 100 {
            assert!(
                foot.starts_with(" type to filter   ↑↓ move   ⌫ edit   ^U clear   ↵ keep   esc clear"),
                "{foot}"
            );
        }
    }
    for (w, h) in [(60u16, 24u16), (100, 30)] {
        let mut nav = nav_on(&ps, Sel::Pane("%4".into()), &[], h as usize - 4);
        nav.open_find(&ps, h as usize - 4);
        typed(&mut nav, &ps, "terrafrom");
        let out = draw(w, h, |f| board::render(f, f.area(), &find_view(&ps, &th, &nav)));
        golden(&format!("board-find-none-{w}"), &out);
        assert!(out.contains("no agent matches \"terrafrom\"") && out.contains(" 0 of 13 "), "{out}");
        assert!(out.contains("^U clears") && !out.contains("move"), "{out}");
    }
}

/// A kept filter shows in the list's title, and esc's hint says it clears it.
#[test]
fn a_kept_filter_is_named_in_the_title() {
    let th = Theme::fixed();
    let ps = fleet();
    let mut nav = nav_on(&ps, Sel::Pane("%4".into()), &[], 26);
    nav.open_find(&ps, 26);
    typed(&mut nav, &ps, "test");
    nav.find_act(board::FindKey::Keep, &ps, 26);
    let out = draw(100, 30, |f| board::render(f, f.area(), &find_view(&ps, &th, &nav)));
    assert!(out.contains(" / test · 4 of 13 ┐"), "{out}");
    assert!(!out.contains("▏"), "the field is closed:\n{out}");
    assert!(rows(&out)[29].contains("esc clear filter"), "{out}");
    // kept with no match: the list says so, and / edits it
    nav.open_find(&ps, 26);
    typed(&mut nav, &ps, "zz");
    nav.find_act(board::FindKey::Keep, &ps, 26);
    let out = draw(100, 30, |f| board::render(f, f.area(), &find_view(&ps, &th, &nav)));
    assert!(out.contains("no agent matches \"testzz\"") && out.contains("0 of 13"), "{out}");
    assert!(rows(&out)[29].starts_with(" / edit   esc clear"), "{out}");
}

/// board.state's `filter` line: appended to version 1, after the folds, read
/// strictly, and an older file without one still reads.
#[test]
fn board_state_keeps_the_filter() {
    let d = scratch("state-filter");
    let p = d.join("board.state");
    let s = board::Saved {
        sel: Some("%7".into()),
        folds: folds(&["web"]),
        filter: Some("test api".into()),
    };
    board::write_state(&p, &s).unwrap();
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "cockpit-board-state 1\nsel %7\nfold web\nfilter test api\n"
    );
    assert_eq!(board::read_state(&p), Ok(Some(s)));
    for (body, why) in [
        ("cockpit-board-state 1\nfilter a\nfilter b\n", "a filter twice"),
        ("cockpit-board-state 1\nfilter   \n", "a filter of spaces"),
        ("cockpit-board-state 1\nfilter \n", "an empty filter"),
        ("cockpit-board-state 1\nfilter a\nfold web\n", "a fold after the filter"),
        ("cockpit-board-state 1\nfilter a\nsel %1\n", "the selection after the filter"),
        ("cockpit-board-state 1\nfilter a\tb\n", "a control character"),
        ("cockpit-board-state 2\nfilter a\n", "another version"),
    ] {
        std::fs::write(&p, body).unwrap();
        assert!(board::read_state(&p).is_err(), "{why}");
    }
    std::fs::write(&p, format!("cockpit-board-state 1\nfilter {}\n", "x".repeat(201))).unwrap();
    assert!(board::read_state(&p).is_err(), "longer than the field takes");
    std::fs::write(&p, "cockpit-board-state 1\nfilter needs\n").unwrap();
    assert_eq!(
        board::read_state(&p),
        Ok(Some(board::Saved {
            filter: Some("needs".into()),
            ..Default::default()
        })),
        "a filter alone"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The field, its rule and the no-match text never run past the area, in
/// either glyph tier, however long the filter.
#[test]
fn no_find_row_exceeds_the_area() {
    use unicode_width::UnicodeWidthStr;
    let ps = fleet();
    for th in [Theme::fixed(), {
        let mut t = Theme::fixed();
        t.g = Glyphs::ascii();
        t
    }] {
        for w in [40u16, 41, 60, 100, 180] {
            for text in ["test", "terrafrom", &"long words ".repeat(18)] {
                let mut nav = nav_on(&ps, Sel::Pane("%7".into()), &["api"], 20);
                nav.open_find(&ps, 20);
                typed(&mut nav, &ps, text);
                let out = draw(w, 24, |fr| board::render(fr, fr.area(), &find_view(&ps, &th, &nav)));
                for (i, r) in rows(&out).iter().enumerate() {
                    assert_eq!(r.width(), w as usize, "row {i} at {w}:\n{out}");
                }
                assert!(out.contains(th.g.cursor), "the cursor stays on screen:\n{out}");
            }
        }
    }
}

/// The extra cells as text: a worktree row's branch cell (#312).
fn extra_in(th: &Theme, loc: &str, room: u16) -> String {
    let p = Pane {
        loc: loc.into(),
        pm: "clean".into(),
        ..Pane::default()
    };
    board::row_extra(&p, th, room, &Default::default())
        .iter()
        .map(|s| s.content.to_string())
        .collect()
}

fn extra(loc: &str, room: u16) -> String {
    extra_in(&Theme::fixed(), loc, room)
}

/// With room, a worktree row names the repo, the branch and the worktree;
/// a main checkout's row keeps its `⑂branch` alone.
#[test]
fn a_roomy_worktree_row_names_repo_branch_and_worktree() {
    assert_eq!(extra("m⑂feat · wt w1", 40), " clean m⑂feat · wt w1");
    assert_eq!(extra("api⑂main", 40), " clean ⑂main");
}

/// As the room shrinks: the repo goes first, then the worktree name is cut,
/// then dropped, and the branch goes last. `" clean"` takes 6 of the room.
#[test]
fn a_narrowing_worktree_row_drops_repo_then_worktree_then_branch() {
    let t = |room| extra("m⑂feat · wt fix-312-row", room);
    assert_eq!(t(30), " clean m⑂feat · wt fix-312-row", "everything");
    assert_eq!(t(29), " clean ⑂feat · wt fix-312-row", "repo dropped");
    assert_eq!(t(24), " clean ⑂feat · wt fix-3…", "worktree cut");
    assert_eq!(t(20), " clean ⑂feat · wt f…", "one real char survives");
    assert_eq!(t(19), " clean ⑂feat", "a lone … is no name: dropped");
    assert_eq!(t(8), " clean", "branch dropped last");
    // a wide first character that cannot fit beside the ellipsis is no cut either
    let w = extra("m⑂feat · wt 日本語の木の名前", 20);
    assert_eq!(w, " clean ⑂feat", "a lone … after a wide char: dropped");
}

/// In ascii the branch mark is `@`, so a detached HEAD's `@sha` takes none.
#[test]
fn an_ascii_detached_head_shows_one_at() {
    let mut th = Theme::fixed();
    th.g = Glyphs::ascii();
    assert_eq!(extra_in(&th, "api⑂@1b94824", 40), " clean @1b94824");
    assert_eq!(extra_in(&th, "api⑂main", 40), " clean @main");
    assert_eq!(extra("api⑂@1b94824", 40), " clean ⑂@1b94824");
}

// ------------------------------------------- ended agents, kill a pane, resume (#301)

const SID: &str = "0b5e7c1a-3f2d-4e8b-9a6c-1d2e3f4a5b6c";

/// An agent whose process is gone, left on `needs-input`: the worst state it
/// could have died on, so every ▲ rule has something to get wrong.
fn ended(id: &str, sess: &str) -> Pane {
    let mut p = pane(id, "api:0", "server", "needs-input");
    (p.msg, p.sess, p.win, p.ended) = ("Do you want to run npm test?".into(), sess.into(), "@3".into(), true);
    p
}

/// The fleet with `%3` (api's ▲) ended.
fn fleet_ended() -> Vec<Pane> {
    let mut ps = fleet();
    ps.iter_mut().find(|p| p.id == "%3").unwrap().ended = true;
    ps
}

/// At 95 columns, the widest list with no right pane (#284) to name the title too.
fn list_line(ps: &[Pane], sel: &str, id: &str) -> (Buffer, u16) {
    let (th, nav) = (Theme::fixed(), nav_on(ps, Sel::Pane(sel.into()), &[], 26));
    let b = render_to_buffer(95, 30, |f| board::render(f, f.area(), &list_view(ps, &th, &nav)));
    let p = ps.iter().find(|p| p.id == id).unwrap();
    let y = (0..30).find(|&y| (0..95).map(|x| b[(x, y)].symbol()).collect::<String>().contains(p.title.as_str()));
    (b.clone(), y.unwrap())
}

#[test]
fn an_ended_row_says_ended_with_its_own_glyph_and_no_needs_mark() {
    let ps = fleet_ended();
    let (b, y) = list_line(&ps, "%4", "%3");
    let line: String = (0..95).map(|x| b[(x, y)].symbol()).collect();
    assert!(line.contains("◌") && line.contains("ended") && !line.contains("▲"), "{line}");
    assert!(!line.contains("npm test"), "the message it died on is not shown: {line}");
}

#[test]
fn an_ended_row_is_dimmed_name_and_text_off_the_band() {
    let ps = fleet_ended();
    let th = Theme::fixed();
    let (b, y) = list_line(&ps, "%4", "%3");
    let line: String = (0..95).map(|x| b[(x, y)].symbol()).collect();
    for word in ["server", "ended"] {
        let x = line[..line.find(word).unwrap()].chars().count() as u16;
        assert!(b[(x, y)].modifier.contains(Modifier::DIM), "{word} is DIM");
        assert_eq!(b[(x, y)].fg, th.quote, "{word} is in the dim role");
    }
}

#[test]
fn a_selected_ended_row_keeps_fg_and_no_dim() {
    let ps = fleet_ended();
    let (b, y) = list_line(&ps, "%3", "%3");
    let line: String = (0..95).map(|x| b[(x, y)].symbol()).collect();
    let x = line[..line.find("server").unwrap()].chars().count() as u16;
    assert!(!b[(x, y)].modifier.contains(Modifier::DIM), "D17: the band is never dim");
    assert_eq!(b[(x, y)].fg, Theme::fixed().value);
}

/// "needs you", the hot band, the fold's keep and `u` all skip an ended ▲.
#[test]
fn an_ended_agent_is_never_needs_or_urgent() {
    let ps = fleet_ended();
    let th = Theme::fixed();
    let nav = nav_on(&ps, Sel::Pane("%4".into()), &[], 26);
    let out = draw(120, 30, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
    assert!(rows(&out)[0].contains(" 1 needs you"), "only web's ▲ is counted: {}", rows(&out)[0]);
    // api held only the ▲ that ended: it drops out of the hot band
    assert_eq!(names(&ps), ["infra", "web", "api", "ml"]);
    // a folded api hides its ended row, which a live ▲ never is
    assert!(!shown(&ps, &folds(&["api"])).contains(&"%3".to_string()));
}

#[test]
fn u_and_the_open_selection_skip_an_ended_needs_row() {
    let ps = fleet_ended();
    let mut nav = nav_on(&ps, Sel::Pane("%4".into()), &[], 26);
    nav.act(Act::NextNeeds, &ps, 26);
    assert_eq!(nav.sel, Sel::Pane("%7".into()), "u lands on web's live ▲");
    let lo = board::list_order(&ps);
    assert_eq!(ps[lo[board::open_sel(&ps, &lo, None, "")]].id, "%7");
    let nav = nav_on(&ps, Sel::Pane("%3".into()), &[], 26);
    assert_eq!(nav.focus(&ps), Focus::Agent(false), "a answers nothing on it");
}

#[test]
fn an_ended_agent_sorts_after_the_live_ones() {
    let ps = vec![ended("%1", SID), pane("%2", "api:1", "b", "done"), pane("%3", "api:2", "c", "")];
    let ids: Vec<&str> = board::order(&ps).into_iter().map(|i| ps[i].id.as_str()).collect();
    assert_eq!(ids, ["%2", "%3", "%1"]);
}

#[test]
fn an_ended_tile_has_no_needs_border_and_says_ended() {
    let ps = vec![ended("%1", SID)];
    let th = Theme::fixed();
    let pvs = vec![Some("$".to_string())];
    let b = render_to_buffer(60, 24, |f| board::render(f, f.area(), &view(&ps, &pvs, &th, 0, 0)));
    let out: String = (0..24)
        .map(|y| (0..60).map(|x| b[(x, y)].symbol()).collect::<String>() + "\n")
        .collect();
    assert!(out.contains("ended") && !out.contains("▲"), "{out}");
    assert_ne!(b[(0, 1)].fg, th.needs, "the border is not the needs colour");
}

#[test]
fn the_header_counts_ended_agents_under_their_own_glyph() {
    let ps = fleet_ended();
    let th = Theme::fixed();
    let nav = nav_on(&ps, Sel::Pane("%4".into()), &[], 26);
    let out = draw(100, 30, |f| board::render(f, f.area(), &list_view(&ps, &th, &nav)));
    let api = rows(&out).into_iter().find(|r| r.contains(" api ")).unwrap().to_string();
    assert!(api.contains("◌1") && !api.contains("▲"), "{api}");
}

#[test]
fn find_reads_ended_as_the_state_word() {
    let ps = fleet_ended();
    let p = ps.iter().find(|p| p.id == "%3").unwrap();
    assert!(board::find_match(p, "ended") && !board::find_match(p, "needs"));
}

/// D18 holds `ended` as it holds the state: a row must not move under a confirm.
#[test]
fn a_refresh_while_asked_holds_ended() {
    let old = named();
    let c = board::Confirm::open(kill(), &id(&old, "%4")).unwrap();
    let mut new = named();
    new.iter_mut().find(|p| p.id == "%4").unwrap().ended = true;
    let held = board::held(&c, &old, &new).unwrap();
    assert!(!held.iter().find(|p| p.id == "%4").unwrap().ended);
}

/// `o` is in the footer only on an ended agent whose session is a UUID.
#[test]
fn resume_is_offered_only_on_an_ended_agent_with_a_session_id() {
    let g = Glyphs::unicode();
    let has_o = |p: &Pane| {
        board::hints_on(&g, true, Focus::Agent(false), p)
            .iter()
            .any(|(k, v)| k == "o" && v == "resume")
    };
    assert!(has_o(&ended("%3", SID)));
    let mut live = ended("%3", SID);
    live.ended = false;
    assert!(!has_o(&live), "a live agent");
    for bad in [
        "",
        "-",
        "abc",
        &SID.to_uppercase(),
        &format!("{SID};id"),
        &format!("$(id){}", &SID[5..]),
    ] {
        assert!(!has_o(&ended("%3", bad)), "{bad:?} offered a resume");
    }
}

#[test]
fn a_session_id_is_strictly_a_lowercase_uuid() {
    let ok = [SID, "00000000-0000-0000-0000-000000000000"];
    let no = [
        "",
        "0b5e7c1a3f2d4e8b9a6c1d2e3f4a5b6c",
        "0b5e7c1a-3f2d-4e8b-9a6c-1d2e3f4a5b6",
        "0b5e7c1a-3f2d-4e8b-9a6c-1d2e3f4a5b6c0",
        "0b5e7c1a-3f2d-4e8b-9a6c1-d2e3f4a5b6c",
        "0B5E7C1A-3F2D-4E8B-9A6C-1D2E3F4A5B6C",
        "0b5e7c1g-3f2d-4e8b-9a6c-1d2e3f4a5b6c",
        "0b5e7c1a-3f2d-4e8b-9a6c-1d2e3f4a5b6c\n",
        "#(id)c1a-3f2d-4e8b-9a6c-1d2e3f4a5b6c",
        "０b5e7c1a-3f2d-4e8b-9a6c-1d2e3f4a5b6",
    ];
    assert!(ok.iter().all(|s| board::is_session_id(s)));
    for s in no {
        assert!(!board::is_session_id(s), "{s:?}");
    }
}

/// `o` writes `resume %N` at once, the session id as `.expect`; on a live
/// agent the dispatch leaves it to `offered`, which says no.
#[test]
fn o_writes_resume_and_the_session_id() {
    let o = board::extra('o').unwrap();
    let p = ended("%3", SID);
    assert!(board::Confirm::open(o, &p).is_none(), "resume asks nothing");
    assert_eq!(
        board::at_once(o, &p),
        board::Exit {
            line: "resume %3".into(),
            expect: Some(SID.into())
        }
    );
    let mut live = p.clone();
    live.ended = false;
    assert!(!(o.offered)(&live));
}

/// `x` is offered on an ended agent too, and closes its pane like any other.
#[test]
fn x_is_offered_on_an_ended_agent() {
    let p = ended("%3", SID);
    assert!((kill().offered)(&p) && board::refused(kill(), &p).is_none());
    assert_eq!(board::Confirm::open(kill(), &p).unwrap().expect.as_deref(), Some("%3 @3"));
}

/// #460: a tile's title names the bound job, `→ <job>`, in place of its title.
#[test]
fn a_bound_viewer_tile_names_its_job() {
    let th = Theme::fixed();
    let (mut ps, pvs) = (fixture(), previews());
    ps[0].job = "sweep".into();
    let v = view(&ps, &pvs, &th, 1, 0);
    let screen = draw(120, 30, |f| board::render(f, f.area(), &v));
    let label = ps[0].label().to_string();
    assert!(screen.contains("→ sweep"), "{screen}");
    assert!(!screen.contains(&label), "the title {label:?} gave way: {screen}");
}

/// #460 review: find reads the job a bound row draws in place of its title.
#[test]
fn find_matches_the_bound_job_a_row_draws() {
    let mut p = fixture().remove(0);
    p.job = "nightlysweep".into();
    assert!(
        board::find_match(&p, "nightlysweep"),
        "the row reads -> nightlysweep, find misses it"
    );
}
