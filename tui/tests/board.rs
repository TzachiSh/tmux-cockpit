//! The board's TestBackend goldens and the contracts a golden cannot hold.
//! No tmux, no clock, no env: every fixture is a literal and every theme is
//! built by hand, so a failure means the layout moved and nothing else.
//!
//! COCKPIT_BLESS=1 cargo test  rewrites the goldens.
use cockpit_tui::board::{self, Act, Grid, KEYS, View};
use cockpit_tui::data::Pane;
use cockpit_tui::theme::{Glyphs, Theme};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use std::collections::HashSet;

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

fn previews() -> Vec<String> {
    vec![
        "Bash command\nsudo openvpn --config /etc/openvpn/work.conf --daemon\nDo you want to proceed?\n❯ 1. Yes   2. Yes, don't ask again   3. No".into(),
        "$ make check\n1001 assertions, 0 failures\n$ shellcheck cockpit.tmux tests/run.sh".into(),
        "⏺ Agent (fleet-gate-review)\n  Read tests/fleet.sh, 214 lines\n  Read scripts/agent-state, 412 lines".into(),
        "  ✓ pushed to origin/fix/left-click\n  PR #28 opened\n❯".into(),
        "❯".into(),
    ]
}

fn view<'a>(ps: &'a [Pane], pvs: &'a [String], th: &'a Theme, sel: usize, top: usize) -> View<'a> {
    View {
        panes: ps,
        pvs,
        th,
        sel,
        top,
        active: "%14",
        note: None,
        help: false,
    }
}

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

/// Red border, the question in the header, ` ↵ answer ` on the bottom border.
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
    let text = b.content.iter().map(|c| c.symbol()).collect::<String>();
    assert!(text.contains("permission: Bash(sudo openvpn"), "the question must be on the tile");
    assert!(text.contains("↵ answer"), "the bottom title must carry the verb");
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
    let dim = (0..b.area.height).any(|y| {
        let row: String = (0..b.area.width).map(|x| b[(x, y)].symbol()).collect();
        row.contains("PR #28 opened") && (0..b.area.width).any(|x| b[(x, y)].modifier.contains(Modifier::DIM))
    });
    assert!(dim, "the done tile's preview must carry DIM");
}

// ------------------------------------------------------------ colour tiers

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
    let mut th = Theme::fixed();
    th.g = Glyphs::ascii();
    let ascii = board_at(80, 24, 0, &th);
    golden("board-ascii", &ascii);

    let uni = board_at(80, 24, 0, &Theme::fixed());
    for (i, (a, u)) in ascii.lines().zip(uni.lines()).enumerate() {
        assert_eq!(a.chars().count(), u.chars().count(), "line {i} changed width in the ascii tier");
    }
    // the preview stays raw pane bytes by design, so only the chrome is checked
    for glyph in [
        "┌", "┐", "└", "┘", "│", "─", "┏", "┓", "┗", "┛", "┃", "━", "▲", "▼", "●", "○", "✕", "◆", "↵", "▎", "…",
    ] {
        assert!(!ascii.contains(glyph), "the ascii tier still draws {glyph}:\n{ascii}");
    }
}

// --------------------------------------------------------------- the keymap

/// One KEYS table feeds the dispatch and the help overlay; a key bound but
/// undocumented, or documented but unbound, fails here (ux.md §3.4).
#[test]
fn help_lists_every_key_in_the_dispatch_table() {
    let norm = |a: Act| if matches!(a, Act::Tile(_)) { Act::Tile(0) } else { a };
    let documented: HashSet<Act> = KEYS.iter().flat_map(|k| k.acts.iter().copied()).map(norm).collect();

    let mut codes: Vec<KeyEvent> = ('a'..='z')
        .chain('A'..='Z')
        .chain('0'..='9')
        .chain("[]?".chars())
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
    let bound: HashSet<Act> = codes.iter().filter_map(|k| board::action(*k)).map(norm).collect();

    assert_eq!(bound, documented, "the dispatch table and the help overlay disagree");
}

#[test]
fn help_overlay() {
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    let mut v = view(&ps, &pvs, &th, 1, 0);
    v.help = true;
    golden("board-help", &draw(100, 24, |f| board::render(f, f.area(), &v)));
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
        ("theme.rs", include_str!("../src/theme.rs")),
        ("backend.rs", backend),
    ] {
        assert!(!src.contains("Wrap"), "{name} builds a Paragraph with Wrap");
        for q in ["supports_keyboard_enhancement", "SetCursorStyle", "EnableMouseCapture"] {
            assert!(!src.contains(q), "{name} sends a terminal query ({q})");
        }
    }
    // window_size is a required Backend method, so backend.rs may name it once
    // to delegate; nothing else in the crate may call it.
    assert_eq!(
        backend.matches("window_size").count(),
        2,
        "backend.rs must only delegate window_size"
    );
    assert!(!include_str!("../src/board.rs").contains("window_size"));
    // every colour literal lives in theme.rs
    assert!(
        !include_str!("../src/board.rs").contains("Color::"),
        "board.rs names a colour directly"
    );
}

/// Footer pills drop whole, never truncated: "half a label is not a smaller
/// button, it is an unreadable one" (ux.md §1.6).
#[test]
fn footer_drops_whole_pills() {
    let g = Glyphs::unicode();
    let full = "↵ jump   a ask   r review   f fix   i info   ? help   q close";
    let mid = "↵ jump   ? help   q close";
    let last = "? help";
    for w in 1..90usize {
        let got = board::footer_text(&g, w);
        assert!(
            got == full || got == mid || got == last || got.is_empty(),
            "at w={w} the footer is a fragment: {got:?}"
        );
    }
    assert_eq!(board::footer_text(&g, 80), full);
    assert_eq!(board::footer_text(&g, 30), mid);
    assert_eq!(board::footer_text(&g, 10), last);
}

#[test]
fn empty_fleet_says_so() {
    let th = Theme::fixed();
    let v = view(&[], &[], &th, 0, 0);
    let out = draw(80, 24, |f| board::render(f, f.area(), &v));
    assert!(out.contains("no agent panes"), "{out}");
    assert!(out.contains("prefix r"), "{out}");
}

/// Better than a grid of tiles too narrow to name their agent (rules.md 32).
#[test]
fn refuses_below_forty_columns() {
    let th = Theme::fixed();
    let (ps, pvs) = (fixture(), previews());
    let v = view(&ps, &pvs, &th, 0, 0);
    let out = draw(30, 12, |f| board::render(f, f.area(), &v));
    assert!(out.contains("terminal too narrow"), "{out}");
    assert!(out.contains("prefix i on the pane instead"), "{out}");
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
    let pvs = vec![String::new(); 16];
    let th = Theme::fixed();
    let v = view(&ps, &pvs, &th, 0, 0);
    let out = draw(160, 24, |f| board::render(f, f.area(), &v));
    let head = out.lines().next().unwrap();
    assert!(head.contains("16 agents"), "{head}");
    assert!(head.contains("1-12 of 16"), "{head}");
    assert!(head.contains("▼4"), "{head}");
    assert!(out.contains(" 12 ●"), "the twelfth tile keeps its two-digit number:\n{out}");
}
