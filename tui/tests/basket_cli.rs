//! `cockpit-tui basket add <pane> <file>` (#365 child 5): what tmux-open-path
//! runs for the browser's alt-a. The refusals here need no tmux.
use cockpit_tui::basket;

fn cli(a: &[&str]) -> (i32, String) {
    basket::cli(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

#[test]
fn anything_but_add_a_pane_and_a_file_is_usage() {
    for a in [&[][..], &["add"], &["add", "%1"], &["rm", "%1", "/x"], &["add", "%1", "/x", "y"]] {
        let (code, line) = cli(a);
        assert_eq!(code, 2, "{a:?}: {line}");
        assert!(line.contains("usage"), "{a:?}: {line}");
    }
}

#[test]
fn a_folder_or_a_missing_file_is_refused_before_tmux() {
    let d = std::env::temp_dir();
    let d = d.display().to_string();
    let (code, line) = cli(&["add", "%1", &d]);
    assert_eq!((code, line.contains("not a file")), (1, true), "{line}");
    let (code, line) = cli(&["add", "%1", "/no/such/file.rs"]);
    assert_eq!((code, line.contains("no such path")), (1, true), "{line}");
}

#[test]
fn a_bad_pane_is_refused() {
    let (code, line) = cli(&["add", "x", "/etc/hostname"]);
    assert_eq!((code, line.contains("pane")), (1, true), "{line}");
}
