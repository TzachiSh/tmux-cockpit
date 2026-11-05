//! `data::preview` reads the same bytes after #492 split it into
//! `preview_args` and `shape_preview`. The literals are what it read before.
//! Its own binary: `preview` reads COCKPIT_TMUX, which this sets.
#[path = "tiles/stub.rs"]
mod stub;

use cockpit_tui::data::preview;

#[test]
fn preview_reads_what_it_read_before_the_split() {
    let (dir, argv) = stub::stub("shape");
    // SAFETY: this test binary holds exactly one test, so nothing runs beside it.
    unsafe { std::env::set_var("COCKPIT_TMUX", &argv[0]) };
    let got: Vec<_> = stub::screens().iter().map(|(id, _)| preview(id, 5)).collect();
    let want = [
        "\u{1b}[31mred\u{1b}[0m plain\nlink\n##",
        "row 26\nrow 27\nrow 28\nrow 29\nrow 30",
        "",
        "",
        "x\ncockpit-0123456789abcdef0123456789abcdef#\ncockpit-0123##\ny",
        "last",
        "a\n\nb",
    ];
    assert_eq!(got, want.map(|s| Some(s.to_string())));
    assert_eq!(preview("%9", 5), None, "a gone pane");
    let _ = std::fs::remove_dir_all(dir);
}
