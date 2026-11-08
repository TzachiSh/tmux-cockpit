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
/// `काााा` is one grapheme, 5 cells wide. (name, text, width, rows)
const OVER_WIDE: [(&str, &str, usize, &[&str]); 4] = [
    ("alone", "काााा", 4, &["काााा"]),
    ("after a short word", "ab काााा", 4, &["ab", "काााा"]),
    ("with a grapheme after it", "कााााx", 4, &["काााा", "x"]),
    ("twice", "काााा काााा", 4, &["काााा", "काााा"]),
];

/// The child half: each OVER_WIDE row wrapped, in a process whose address space
/// is capped, so a loop that pushes rows forever (the wrap before #656) runs out
/// of its own memory rather than the box's. One `ROW\t<name>\t<rows>` line each.
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
    // a line of its own: libtest's "test hang_child ... " ends without one
    println!();
    for (name, t, w, _) in OVER_WIDE {
        println!("ROW\t{name}\t{:?}", wrap(t, w));
    }
    std::process::exit(0);
}

/// The OVER_WIDE rows as the capped child wrapped them, by name. A child that has
/// not returned in 10 s is killed and fails the test: no wrap runs in this process.
fn over_wide_in_a_child() -> Vec<(String, String)> {
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
    let row = |l: &str| {
        l.strip_prefix("ROW\t")?
            .split_once('\t')
            .map(|(n, r)| (n.to_string(), r.to_string()))
    };
    out.lines().filter_map(row).collect()
}

#[test]
fn an_over_wide_first_grapheme_gets_no_empty_row() {
    let got = over_wide_in_a_child();
    for (name, _, _, want) in OVER_WIDE {
        let rows = got.iter().find(|(n, _)| n == name).map(|(_, r)| r.as_str());
        assert_eq!(rows, Some(format!("{want:?}").as_str()), "{name}");
    }
}

/// Before #656 the wrap looped forever on an over-wide grapheme (#658): it cut
/// at the grapheme itself, an empty row, and never moved on. In the capped child
/// every row comes back, within the deadline.
#[test]
fn the_over_wide_grapheme_that_hung_the_old_wrap_returns() {
    assert_eq!(over_wide_in_a_child().len(), OVER_WIDE.len(), "every row came back");
}
