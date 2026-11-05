//! A tmux stand-in that serves fixed screens, so a capture is the same bytes
//! on every call: `capture-pane ... -t ID` prints `screens/ID`, a missing one
//! fails as a gone pane does, and `display-message -p F` prints F with `##`
//! as `#`, as tmux does. `;` separates commands, and the first failure stops
//! the list. A screen line `@MARK@` becomes the mark of the call it is in, so
//! a pane can hold the very text a batch cuts at; a call with no mark (one
//! pane's own capture) prints it as nothing.
#![allow(dead_code)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const SH: &str = r#"#!/bin/sh
D=$(dirname "$0")
echo call >> "$D/calls"
mark=""
for a; do case $a in *'##') mark=${a%#} ;; esac; done
run() {
    case $cmd in
        capture-pane)
            [ -f "$D/screens/$id" ] || { echo "can't find pane: $id" >&2; exit 1; }
            sed "s/^@MARK@\$/$mark/" "$D/screens/$id" ;;
        display-message) printf '%s\n' "$fmt" | sed 's/##/#/g' ;;
    esac
}
cmd= id= fmt= prev=
for a; do
    if [ "$a" = ";" ]; then run; cmd= id= fmt= prev=; continue; fi
    if [ -z "$cmd" ]; then cmd=$a; continue; fi
    [ "$prev" = -t ] && id=$a
    [ "$cmd" = display-message ] && [ "$a" != -p ] && fmt=$a
    prev=$a
done
run
"#;

/// The panes the stub serves, `(id, screen bytes)`: OSC 8 in the `ESC \`
/// form, trailing blank rows, more rows than a tile keeps, an all-blank and
/// an empty pane, a pane holding an old call's mark and the argv's spelling,
/// one line past the 64 KB preview cap, and last a pane holding this call's
/// own mark.
pub fn screens() -> Vec<(&'static str, Vec<u8>)> {
    let numbered: String = (1..=30).map(|k| format!("row {k}\n")).collect();
    vec![
        (
            "%1",
            b"\x1b[31mred\x1b[0m plain\n\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\\n##\n\n\n\n".to_vec(),
        ),
        ("%2", (numbered + "\n\n").into_bytes()),
        ("%3", b"\n\n\n\n\n".to_vec()),
        ("%4", Vec::new()),
        ("%5", b"x\ncockpit-0123456789abcdef0123456789abcdef#\ncockpit-0123##\ny\n".to_vec()),
        ("%6", [vec![b'w'; 70_000], b"\nlast\n".to_vec()].concat()),
        ("%7", b"a\n@MARK@\nb\n".to_vec()),
    ]
}

/// Writes the stub and its screens under a fresh dir; its argv.
pub fn stub(tag: &str) -> (PathBuf, Vec<String>) {
    let dir = std::env::temp_dir().join(format!("cockpit-tiles-stub-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("screens")).unwrap();
    for (id, s) in screens() {
        std::fs::write(dir.join("screens").join(id), s).unwrap();
    }
    let sh = dir.join("tmux");
    std::fs::write(&sh, SH).unwrap();
    std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let argv = vec![sh.display().to_string()];
    (dir, argv)
}

/// How many times the stub under `dir` ran.
pub fn calls(dir: &std::path::Path) -> usize {
    std::fs::read_to_string(dir.join("calls")).unwrap_or_default().lines().count()
}
