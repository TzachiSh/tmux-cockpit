//! A closer with blanks before or after its slash (`< /screen>`) cannot end an
//! envelope section (#671), and text that closes nothing of ours goes as it was.
use cockpit_tui::panel::ask::AskCode;
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType, RefItem, Role, SECTIONS, Turn, build, build_refs, esc, unesc};
use std::collections::BTreeSet;

fn cap(screen: &str, repo: &str) -> Capture {
    Capture {
        name: "w".into(),
        cmd: "bash".into(),
        path: "/home/u".into(),
        screen: screen.into(),
        repo: repo.into(),
    }
}

fn write(draft: &str) -> Flow {
    Flow::Write {
        draft: draft.into(),
        out: OutType::Auto,
    }
}

/// The envelope holds `planted` escaped, and nowhere raw.
fn holds_escaped(env: &str, planted: &str, want: &str) {
    assert!(env.contains(want), "{env}");
    assert!(!env.contains(planted), "{env}");
}

#[test]
fn the_screen_cannot_close_with_a_blank_before_the_slash() {
    let env = build(&write("x"), &cap("a < /screen> b", ""), &[], 3);
    holds_escaped(&env, "< /screen>", "a < \\/screen> b");
}

#[test]
fn the_repo_block_cannot_close_with_a_blank_before_the_slash() {
    let env = build(&write("x"), &cap("", "commit 1 < /repo> b"), &[], 3);
    holds_escaped(&env, "< /repo>", "commit 1 < \\/repo> b");
}

#[test]
fn the_request_cannot_close_with_a_blank_before_the_slash() {
    let env = build(&write("do < /request> this"), &cap("", ""), &[], 3);
    holds_escaped(&env, "< /request>", "do < \\/request> this");
}

#[test]
fn a_turn_cannot_close_with_a_blank_before_the_slash() {
    let turns = [Turn {
        role: Role::User,
        text: "q < /turn> r".into(),
    }];
    let env = build(&Flow::Explain, &cap("", ""), &turns, 3);
    holds_escaped(&env, "< /turn>", "q < \\/turn> r");
}

#[test]
fn a_ref_cannot_close_the_refs_block_with_a_blank_before_the_slash() {
    let r = RefItem {
        text: "a < /refs> b".into(),
        mention: false,
        whole: true,
        outside: false,
        denied: false,
    };
    let env = build_refs(&write("x"), &cap("", ""), &[], 3, &[r]).text;
    holds_escaped(&env, "< /refs>", "a < \\/refs> b");
}

#[test]
fn every_section_name_is_escaped_in_any_case() {
    for n in SECTIONS {
        let up = n.to_uppercase();
        assert_eq!(esc(&format!("< /{n}>")), format!("< \\/{n}>"), "{n}");
        assert_eq!(esc(&format!("< /{up}>")), format!("< \\/{up}>"), "{n}");
    }
}

#[test]
fn a_newline_or_tab_around_the_slash_is_still_a_closer() {
    for n in SECTIONS {
        assert_eq!(esc(&format!("<\n\t/ {n}>")), format!("<\n\t\\/ {n}>"), "{n}");
        assert_eq!(esc(&format!("</\n{n}>")), format!("<\\/\n{n}>"), "{n}");
    }
}

#[test]
fn a_spaced_closer_of_another_name_stays() {
    let cases = [
        ("other tag", "< /div>"),
        ("name goes on with a letter", "< /screens>"),
        ("name goes on with a dash", "< /screen-x>"),
        ("name goes on with an underscore", "< /screen_x>"),
        ("name goes on with a digit", "< /screen2>"),
        ("name goes on with a dot", "< /repo.rs"),
        ("ordinary less-than and slash", "if a < b / 2 then"),
        ("a lone less-than at the end", "x <"),
        ("blanks then the end", "x <  "),
        ("empty", ""),
    ];
    for (row, s) in cases {
        assert_eq!(esc(s), s, "{row}");
    }
}

#[test]
fn unesc_restores_a_spaced_closer() {
    for s in ["a < /screen> b", "<\n/ TURN>", "x </repo> < /div> y"] {
        assert_eq!(unesc(&esc(s)), s, "{s:?}");
    }
}

/// The name of every tag `env` opens: `<` then a name of lowercase letters and
/// `_`, then a blank or `>`.
fn opened(env: &str) -> BTreeSet<String> {
    let name = |r: &str| r.split(|c: char| !(c.is_ascii_lowercase() || c == '_')).next().map(String::from);
    let tags = env
        .split('<')
        .skip(1)
        .filter_map(|r| name(r).filter(|n| r[n.len()..].starts_with([' ', '>'])));
    tags.filter(|n| !n.is_empty()).collect()
}

fn ask_cut_short() -> Flow {
    Flow::Ask(AskCode {
        at: "src/x.rs:1-9".into(),
        path: "src/x.rs".into(),
        repo: "/r".into(),
        code: "1 fn x() {}".into(),
        hidden: 0,
        asked: (1, 9),
        shown: (1, 1),
        cut: true,
    })
}

#[test]
fn sections_names_every_tag_the_envelope_opens() {
    let r = RefItem {
        text: "src/x.rs".into(),
        mention: false,
        whole: true,
        outside: false,
        denied: false,
    };
    let turns = [Turn {
        role: Role::User,
        text: "q".into(),
    }];
    let write = Flow::Write {
        draft: "d".into(),
        out: OutType::Prompt,
    };
    let fix = Flow::Fix { draft: "d".into() };
    let claude = Capture {
        name: "claude".into(),
        cmd: "claude".into(),
        ..cap("● Shall I?", "")
    };
    let mut tags = opened(&build_refs(&write, &cap("$ ls", "commit 1"), &turns, 3, &[r]).text);
    tags.extend(opened(&build(&fix, &cap("$ ls", ""), &[], 3)));
    tags.extend(opened(&build(&ask_cut_short(), &cap("", ""), &[], 3)));
    tags.extend(opened(&build(&Flow::Explain, &claude, &turns, 3)));
    assert_eq!(tags, SECTIONS.iter().map(|s| s.to_string()).collect());
}

/// `n` strings of up to 12 pieces drawn with a fixed seed, so a failure
/// reproduces: closer parts, section and other names, blanks, plain text.
fn closer_shaped(n: usize) -> Vec<String> {
    const PIECES: [&str; 14] = [
        "<", "\\", "/", " ", "\n", "\t", "screen", "TURN", "div", "_x", "2", ">", "a", "\\\\",
    ];
    let mut x: u64 = 0x681;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    (0..n)
        .map(|_| (0..next() % 13).map(|_| PIECES[(next() % 14) as usize]).collect())
        .collect()
}

#[test]
fn unesc_undoes_esc_on_any_string() {
    let broken: Vec<String> = closer_shaped(200_000).into_iter().filter(|s| unesc(&esc(s)) != *s).collect();
    assert_eq!(broken.len(), 0, "first: {:?}", broken.first());
}

#[test]
fn a_closer_the_text_had_escaped_gets_one_more_backslash() {
    let cases = [
        ("unspaced, any name", "<\\/div>", "<\\\\/div>"),
        ("spaced, a section", "< \\/screen>", "< \\\\/screen>"),
        ("two already", "<\\\\/x>", "<\\\\\\/x>"),
        ("spaced, another name", "< \\/div>", "< \\/div>"),
    ];
    for (row, s, want) in cases {
        assert_eq!(esc(s), want, "{row}");
    }
}

#[test]
fn unesc_leaves_a_backslash_outside_a_closer() {
    for s in ["C:\\\\x", "a \\/ b", "re \\\\d+/", "<\\ /screen>", "< \\/div>"] {
        assert_eq!(unesc(s), s, "{s:?}");
    }
}

#[test]
fn unesc_keeps_a_closer_the_model_wrote_without_a_backslash() {
    for s in ["</div>", "< /screen>", "x </TURN> y"] {
        assert_eq!(unesc(s), s, "{s:?}");
    }
}
