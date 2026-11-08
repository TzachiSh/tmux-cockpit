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
