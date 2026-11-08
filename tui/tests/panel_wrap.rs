//! #352: `view::wrap` measures each word once and cuts a long word in one pass.
//! Its rows must not change: every case is checked against the wrap it replaced,
//! kept here as the reference.
use cockpit_tui::panel::view::wrap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The wrap before #352, word for word: the rows the new one must give.
fn reference(text: &str, w: usize) -> Vec<String> {
    let w = w.max(4);
    let width = |s: &str| s.graphemes(true).map(|g| g.width()).sum::<usize>();
    let mut out = Vec::new();
    for para in text.split('\n') {
        let body = para.trim_start_matches(' ');
        let mut line = " ".repeat((para.len() - body.len()).min(w / 2));
        let mut fresh = true;
        for word in body.split(' ') {
            let cand = if fresh { format!("{line}{word}") } else { format!("{line} {word}") };
            if width(&cand) <= w {
                line = cand;
                fresh = false;
                continue;
            }
            if !fresh {
                out.push(std::mem::take(&mut line));
            }
            fresh = false;
            let mut rest = word.to_string();
            while width(&rest) > w {
                let mut used = 0;
                let cut = rest.grapheme_indices(true).find(|(_, g)| {
                    used += g.width();
                    used > w
                });
                let at = cut.map_or(rest.len(), |(i, _)| i);
                out.push(rest[..at].to_string());
                rest = rest[at..].to_string();
            }
            line = rest;
        }
        out.push(line);
    }
    out
}

/// Tokens that reach every branch: short and long words, wide and zero-width
/// graphemes, a ZWJ emoji, runs of spaces, indentation and empty lines.
const TOKENS: [&str; 14] = [
    "a",
    "word",
    "café",
    "日本語",
    "❤️",
    "👩‍💻",
    "e\u{301}",
    "",
    " ",
    "  ",
    "\n",
    "\n    ",
    "abcdefghijklmnopqrstuvwxyz0123456789",
    "長い長い長い長い長い長い長い長い長い長い",
];

/// A text of `n` tokens from a fixed sequence: the same every run.
fn text(seed: usize, n: usize) -> String {
    let mut x = seed.wrapping_mul(2654435761) | 1;
    let mut s = String::new();
    for _ in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.push_str(TOKENS[x % TOKENS.len()]);
        if !x.is_multiple_of(3) {
            s.push(' ');
        }
    }
    s
}

#[test]
fn every_row_is_the_rows_the_old_wrap_gave() {
    for seed in 0..300 {
        let t = text(seed, 1 + seed % 60);
        for w in [0, 3, 4, 5, 7, 10, 16, 33, 80] {
            assert_eq!(wrap(&t, w), reference(&t, w), "seed {seed} width {w}: {t:?}");
        }
    }
}

/// The cases a reader would name, each against the reference.
#[test]
fn the_named_cases_match_the_old_wrap() {
    let rows = [
        ("empty", ""),
        ("one long word", &"x".repeat(250)),
        ("a long word after a short one", &format!("ab {}", "y".repeat(90))),
        ("indent wider than half", &format!("{}deep words here", " ".repeat(40))),
        ("indent then a long word", &format!("    {}", "z".repeat(90))),
        ("wide graphemes at the edge", &"日本".repeat(30)),
        ("spaces only", "     "),
    ];
    for (name, t) in rows {
        for w in [4, 9, 20, 80] {
            assert_eq!(wrap(t, w), reference(t, w), "{name} at {w}");
        }
    }
}

/// #658: a word whose first grapheme is wider than the line starts its own row,
/// with no empty row before it. Literal rows: the old wrap above hangs on these.
/// `काााा` is one grapheme, 5 cells wide.
#[test]
fn an_over_wide_first_grapheme_gets_no_empty_row() {
    // (name, text, width, rows)
    let rows: [(&str, &str, usize, &[&str]); 4] = [
        ("alone", "काााा", 4, &["काााा"]),
        ("after a short word", "ab काााा", 4, &["ab", "काााा"]),
        ("with a grapheme after it", "कााााx", 4, &["काााा", "x"]),
        ("twice", "काााा काााा", 4, &["काााा", "काााा"]),
    ];
    for (name, t, w, want) in rows {
        assert_eq!(wrap(t, w), want, "{name}");
    }
}

/// The child half of the hang test: the wrap that hung before #656, in a process
/// whose address space is capped, so a loop that pushes rows forever runs out of
/// memory rather than out of the box's.
#[test]
fn hang_child() {
    if std::env::var_os("WRAP_HANG_CHILD").is_none() {
        return;
    }
    let cap = libc::rlimit {
        rlim_cur: 1 << 30,
        rlim_max: 1 << 30,
    };
    // SAFETY: setrlimit reads one struct; it lowers this child's own address space.
    unsafe { libc::setrlimit(libc::RLIMIT_AS, &cap) };
    print!("{:?}", wrap("काााा", 4));
    std::process::exit(0);
}

/// Before #656 the wrap looped forever on an over-wide grapheme (#658): it cut
/// at the grapheme itself, an empty row, and never moved on. The same input,
/// in a child bounded by a 10 s deadline and a memory cap, returns its one row.
#[test]
fn the_over_wide_grapheme_that_hung_the_old_wrap_returns() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "hang_child", "--test-threads=1", "--nocapture"])
        .env("WRAP_HANG_CHILD", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("wrap did not return within 10 s");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut out).unwrap();
    assert!(out.contains(r#"["काााा"]"#), "the child printed {out:?}");
}
