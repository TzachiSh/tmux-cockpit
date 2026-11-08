//! #586: a Read deny that names a folder refuses everything under it. Probed
//! with the read_only profile: `Read(//**/.env)` refused `x/.env/key` with "File
//! is in a directory that is denied", and read a plain file beside it. So
//! `read_denied` holds a rule against every folder on the path, not only the
//! file's own name, and a ref there is noted as not read.
use cockpit_tui::panel::request::read_denied;
use std::path::Path;

/// Rows of (path, denied), read with home at /h/u.
fn wrong(rows: &[(&'static str, bool)]) -> Vec<&'static str> {
    let home = Some(Path::new("/h/u"));
    rows.iter()
        .filter(|(p, want)| read_denied(Path::new(p), home) != *want)
        .map(|r| r.0)
        .collect()
}

#[test]
fn a_file_under_a_denied_folder_is_denied() {
    let rows = [
        ("/r/.env/key", true),
        ("/r/a/.env/deep/key", true),
        ("/r/.env.local/key", true),
        ("/h/u/.claude/.credentials.json/x", true),
    ];
    assert_eq!(wrong(&rows), [] as [&str; 0]);
}

/// A folder whose name only starts like a denied one, or a denied name further
/// down than the path goes, denies nothing.
#[test]
fn a_folder_that_only_looks_denied_is_not() {
    let rows = [
        ("/r/.envrc/key", false),
        ("/r/env/key", false),
        ("/r/x.env/key", false),
        ("/h/u/.sshx/id", false),
        ("/h/u", false),
        ("/r", false),
    ];
    assert_eq!(wrong(&rows), [] as [&str; 0]);
}
