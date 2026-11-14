//! Which test binaries run a real tmux, and that each goes through the helpers'
//! check first (#1066). cargo's runner clears the caller's TMUX, TMUX_PANE and
//! TMUX_COCKPIT_DIR for a test binary and gives it a TMUX_TMPDIR of its own;
//! `fake::checked_tmux` refuses a binary started without them. A binary that
//! runs tmux and never reaches that check would run unrefused by hand from
//! inside a tmux session.
mod fake;

use std::path::{Path, PathBuf};

/// The text of every Rust string literal in `src`, `\n` read as a new line.
/// Plain and raw literals; a char literal `'"'` is skipped.
fn literals(src: &str) -> Vec<(usize, String)> {
    let b = src.as_bytes();
    let (mut out, mut i) = (vec![], 0);
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i = src[i..].find('\n').map_or(b.len(), |n| i + n);
            }
            b'\'' if b.get(i + 2) == Some(&b'\'') || b.get(i + 1) == Some(&b'\\') => {
                i = src[i + 1..].find('\'').map_or(b.len(), |n| i + n + 2);
            }
            b'r' if matches!(b.get(i + 1), Some(b'"' | b'#')) && !word(b, i) => {
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                let close = format!("\"{}", "#".repeat(hashes));
                let start = i + 2 + hashes;
                let end = src[start..].find(&close).map_or(b.len(), |n| start + n);
                out.push((start, src[start..end].to_string()));
                i = end + close.len();
            }
            b'"' => {
                let (start, mut j, mut text) = (i + 1, i + 1, String::new());
                while j < b.len() && b[j] != b'"' {
                    if b[j] == b'\\' && j + 1 < b.len() {
                        text.push(if b[j + 1] == b'n' { '\n' } else { b[j + 1] as char });
                        j += 2;
                    } else {
                        let c = src[j..].chars().next().unwrap();
                        text.push(c);
                        j += c.len_utf8();
                    }
                }
                out.push((start, text));
                i = j + 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// Whether the byte before `i` continues a word, as in `br"` or `for"`.
fn word(b: &[u8], i: usize) -> bool {
    i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')
}

/// Whether a literal at `at` in `src`, with text `t`, names a real tmux to run:
/// `tmux` alone as a program (not a path part given to `join`), an argv that
/// starts with it (a COCKPIT_TMUX value), or a line of a script that runs it,
/// `tmux` and a word after an optional `exec`.
fn runs_tmux(src: &str, at: usize, t: &str) -> bool {
    let before = src[..at - 1].trim_end();
    let program = t == "tmux" && !before.ends_with("join(");
    let line = |l: &str| {
        let l = l.trim_start();
        let l = l.strip_prefix("exec ").unwrap_or(l).trim_start();
        let arg = |c: char| c.is_ascii_lowercase() || c == '-' || c == '"';
        l.strip_prefix("tmux ").is_some_and(|rest| rest.starts_with(arg))
    };
    let shell = t.contains('\n') && t.lines().flat_map(|l| l.split([';', '|', '&'])).any(line);
    program || t.starts_with("tmux -") || shell
}

/// The files a test binary's root `root` is built from: itself, and each
/// module it loads by `mod <name>;`, with or without `#[path = ...]`.
fn crate_files(root: &Path) -> Vec<PathBuf> {
    let dir = root.parent().unwrap();
    let src = std::fs::read_to_string(root).unwrap();
    let mut files = vec![root.to_path_buf()];
    let mut path = None;
    for l in src.lines().map(str::trim) {
        if let Some(p) = l.strip_prefix("#[path = \"").and_then(|p| p.strip_suffix("\"]")) {
            path = Some(dir.join(p));
        } else if let Some(m) = l.strip_prefix("mod ").and_then(|m| m.strip_suffix(';')) {
            files.push(path.take().unwrap_or_else(|| dir.join(m).join("mod.rs")));
        }
    }
    files
}

/// `fake/mod.rs`, read apart: every tmux it runs goes through its own
/// `checked_tmux` first, so its text would name each binary that loads it.
const FAKE: &str = "fake/mod.rs";

/// What a binary's files say when they reach the check: a call to it, or the
/// rig server whose start makes it.
const CHECKS: [&str; 2] = ["fake::checked_tmux(", "fake::Server::new("];

/// Binaries the scan names that run no real tmux, each with why.
const ALLOWED: [(&str, &str); 1] = [(
    "transcript.rs",
    "writes a stub named tmux and runs it by its path, through COCKPIT_TMUX",
)];

/// #1066: every test binary that runs a real tmux reaches the helpers' check.
/// Read from `tui/tests`: a binary is named where one of its files holds a
/// string literal that [`runs_tmux`] and none of its files names a [`CHECKS`]
/// entry. What it cannot see: a binary that reaches tmux only through the
/// product, as the panel binaries and `files` do through `data::tmux` and
/// `Theme::load` (each has a row of its own that runs the check); tmux by a
/// path, as `Command::new("/usr/bin/tmux")`; a one-line shell string, as
/// `["-c", "tmux ls"]`; a tmux name built at run time or by a helper from
/// parts; a script read from a file; and a check named in a file but never
/// reached.
#[test]
fn every_binary_that_runs_tmux_reaches_the_check() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut missing = vec![];
    let mut roots: Vec<PathBuf> = std::fs::read_dir(&tests).unwrap().map(|e| e.unwrap().path()).collect();
    roots.retain(|p| p.extension().is_some_and(|e| e == "rs"));
    roots.sort();
    for root in &roots {
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        let files = crate_files(root);
        let text = |f: &PathBuf| std::fs::read_to_string(f).unwrap();
        let checks = files.iter().any(|f| CHECKS.iter().any(|c| text(f).contains(c)));
        let runs = files.iter().filter(|f| !f.ends_with(FAKE)).find_map(|f| {
            let src = text(f);
            let hit = literals(&src).into_iter().find(|(at, t)| runs_tmux(&src, *at, t))?;
            let line = src[..hit.0].lines().count();
            Some(format!("{}:{line}", f.strip_prefix(&tests).unwrap().display()))
        });
        let allowed = ALLOWED.iter().any(|(f, _)| *f == name);
        if let (Some(at), false, false) = (runs, checks, allowed) {
            missing.push(at);
        }
    }
    assert_eq!(missing, Vec::<String>::new(), "run a real tmux and never reach fake::checked_tmux");
}

/// The scan above, on sources with a known answer, so a reader that sees
/// nothing cannot pass it. The shapes it reads, not every way to run tmux:
/// [`every_binary_that_runs_tmux_reaches_the_check`] lists what it misses.
#[test]
fn the_scan_finds_the_shapes_it_reads_and_no_lookalike() {
    let rows = [
        ("a program", r#"Command::new("tmux").arg("-V")"#, true),
        ("an argv value", r#"format!("tmux -L {}", s)"#, true),
        ("a script line", r##""#!/bin/sh\ntmux -L x \"$@\"\n""##, true),
        ("after a ;", r##""#!/bin/sh\nset -e; tmux -S y ls\n""##, true),
        ("a command word", r##""#!/bin/sh\ntmux ls\n""##, true),
        ("after exec", r##""#!/bin/sh\nexec tmux new-session -d\n""##, true),
        ("a quoted command", r##""#!/bin/sh\nexec tmux \"$@\"\n""##, true),
        ("a path part", r#"dir.join("tmux")"#, false),
        ("a stub's path", r#"format!("{d}/bin/tmux")"#, false),
        ("words in a message", r#"assert!(x, "tmux is not answering")"#, false),
        ("a plugin folder", r#"".cache/tmux-cockpit/agents.json""#, false),
        ("a comment", "// Command::new(\"tmux\")\n", false),
    ];
    for (row, src, want) in rows {
        let got = literals(src).into_iter().any(|(at, t)| runs_tmux(src, at, &t));
        assert_eq!(got, want, "{row}: {src}");
    }
}

/// #1066: the folder the runner makes for this binary in /tmp holds tmux's own
/// `tmux-<uid>` folder and the sockets in it, and nothing else, read while a rig
/// server runs there.
#[test]
fn the_tmux_folder_holds_sockets_and_nothing_else() {
    use std::os::unix::fs::FileTypeExt;
    let dir = fake::checked_tmux();
    let mut s = fake::Server::new(&format!("cr-{}", std::process::id()));
    let cwd = std::env::temp_dir();
    if !s.start(|c| {
        c.args(["new-session", "-d", "-c"]).arg(&cwd).arg("sleep 30");
    }) {
        eprintln!("SKIP: no tmux on PATH");
        return;
    }
    let mut odd = vec![];
    for top in std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap()) {
        let name = top.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("tmux-") && top.file_type().unwrap().is_dir()) {
            odd.push(name);
            continue;
        }
        for e in std::fs::read_dir(top.path()).unwrap().map(|e| e.unwrap()) {
            if !e.file_type().unwrap().is_socket() {
                odd.push(format!("{name}/{}", e.file_name().to_string_lossy()));
            }
        }
    }
    assert_eq!(odd, Vec::<String>::new(), "in {}", dir.display());
}

/// #1066: what a part 4 row may kill is a process whose TMUX names a rig socket of the
/// child and which carries this binary's own TMUX_TMPDIR. Two `sleep`s of the row's own
/// with the same socket name in TMUX: the one with another TMUX_TMPDIR, as a server
/// another run leaked under a reused pid would have, is not taken.
#[test]
#[cfg(target_os = "linux")]
fn a_process_with_another_tmux_tmpdir_is_never_taken_for_a_rigs() {
    let own = fake::checked_tmux();
    let child = 999_999_999u32;
    let tmux = format!("{}/tmux-0/cockpit-decoy-{child},1,0", own.display());
    let start = |tmpdir: &Path| {
        let mut c = std::process::Command::new("/bin/sleep");
        c.arg("30").env("TMUX", &tmux).env("TMUX_TMPDIR", tmpdir).spawn().unwrap()
    };
    let (mut ours, mut decoy) = (start(&own), start(Path::new("/elsewhere-1066")));
    // the spawns have exec'd sleep once each reads as sleep in /proc: polled, 20 s at most
    let comm = |p: u32| std::fs::read_to_string(format!("/proc/{p}/comm")).unwrap_or_default();
    for tries in 0.. {
        if comm(ours.id()).trim() == "sleep" && comm(decoy.id()).trim() == "sleep" {
            break;
        }
        assert!(tries < 4000, "the two sleeps were not sleep after {tries} tries");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let left = fake::servers_left(child);
    for c in [&mut ours, &mut decoy] {
        c.kill().unwrap();
        c.wait().unwrap();
    }
    assert_eq!((left.contains(&ours.id()), left.contains(&decoy.id())), (true, false), "{left:?}");
}
