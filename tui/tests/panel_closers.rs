//! A closer with blanks before or after its slash (`< /screen>`) cannot end an
//! envelope section (#671), and text that closes nothing of ours goes as it was.
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType, RefItem, Role, Turn, build, build_refs, esc, unesc};

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

const NAMES: [&str; 14] = [
    "screen",
    "window",
    "repo",
    "refs",
    "ref",
    "request",
    "output_type",
    "text",
    "code",
    "truncated",
    "session",
    "conversation",
    "turn",
    "questions_left",
];

#[test]
fn every_section_name_is_escaped_in_any_case() {
    for n in NAMES {
        let up = n.to_uppercase();
        assert_eq!(esc(&format!("< /{n}>")), format!("< \\/{n}>"), "{n}");
        assert_eq!(esc(&format!("< /{up}>")), format!("< \\/{up}>"), "{n}");
    }
}

#[test]
fn a_newline_or_tab_around_the_slash_is_still_a_closer() {
    for n in NAMES {
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
