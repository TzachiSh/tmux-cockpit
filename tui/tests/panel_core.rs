//! The pure halves of the panel: redaction, the envelope, routing, examples and
//! the session sniff. Nothing here sets the environment; panel_env.rs does.
use cockpit_tui::panel::capture::{Capture, Session, session_state, window_kind};
use cockpit_tui::panel::envelope::{Flow, OutType, Role, Turn, build, build_counted, esc, unesc};
use cockpit_tui::panel::prompts::{Ctx, WRITE_EXAMPLES, examples, render};
use cockpit_tui::panel::redact::redact;
use cockpit_tui::panel::route::{Route, model_argv, route};

fn hid(s: &str) -> String {
    redact(s).0
}

#[test]
fn a_private_key_block_goes_whole_even_when_cut_off() {
    let pem = "a\n-----BEGIN RSA PRIVATE KEY-----\nMIIx\nabc\n-----END RSA PRIVATE KEY-----\nb";
    assert_eq!(redact(pem), ("a\n[hidden]\nb".into(), 1));
    assert_eq!(hid("x -----BEGIN PRIVATE KEY-----\nMIIx"), "x [hidden]");
    assert_eq!(hid("-----BEGIN PUBLIC KEY-----\nMIIx"), "-----BEGIN PUBLIC KEY-----\nMIIx");
}

#[test]
fn each_token_shape_is_hidden() {
    let cases = [
        ("k sk-ant-api03-abcdefghij end", "k [hidden] end"),
        ("sk-abcdefghijklmnopqrst1", "[hidden]"),
        ("ghp_abcdefghijABCDEFGHIJ12", "[hidden]"),
        ("github_pat_11AAAAAAA_bbbbbbbbbbbbbbbb", "[hidden]"),
        ("id AKIAABCDEFGHIJKLMNOP.", "id [hidden]."),
        ("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abcd", "[hidden]"),
        ("xoxb-1234567890-abc", "[hidden]"),
        ("abcdefghij-abcdefghijklmnopqrstuv", "[hidden]"),
    ];
    for (input, want) in cases {
        assert_eq!(redact(input), (want.to_string(), 1), "{input}");
    }
}

#[test]
fn near_misses_stay() {
    for s in [
        "sk-short",
        "AKIAABCDEFGHIJKLMNOPQ",
        "abcdefghij-abcdefghijklmnopqrstuvw",
        "pwd=abc",
        "mytoken=abcdef",
        "xsk-abcdefghijklmnopqrst1",
    ] {
        assert_eq!(redact(s), (s.to_string(), 0), "{s}");
    }
}

#[test]
fn a_named_secret_keeps_its_name() {
    assert_eq!(
        redact("password: hunter22 and Bearer abcdef"),
        ("password: [hidden] and Bearer [hidden]".into(), 2)
    );
    assert_eq!(hid("API-KEY = s3cr3t!"), "API-KEY = [hidden]");
    assert_eq!(hid("héllo ✓ token=ñandú42 ok"), "héllo ✓ token=[hidden] ok");
}

#[test]
fn esc_and_unesc_round_trip() {
    assert_eq!(esc("a </b> c"), "a <\\/b> c");
    assert_eq!(unesc(&esc("x </screen> y")), "x </screen> y");
}

fn cap(name: &str, cmd: &str, screen: &str) -> Capture {
    Capture {
        name: name.into(),
        cmd: cmd.into(),
        path: "/home/u".into(),
        screen: screen.into(),
    }
}

#[test]
fn the_write_envelope_escapes_redacts_and_tags_the_type() {
    let c = cap("bash", "bash", "$ echo </b>\nexport TOKEN=abcd1234");
    let turns = [
        Turn {
            role: Role::User,
            text: "a </b>".into(),
        },
        Turn {
            role: Role::Assistant,
            text: "q?".into(),
        },
    ];
    let flow = Flow::Write {
        draft: "close </tag>".into(),
        out: OutType::Command,
    };
    let want = "<screen window=\"bash · bash\">\n$ echo <\\/b>\nexport TOKEN=[hidden]\n</screen>\n\n\
                <window>bash · bash · /home/u</window>\n\n<request>close <\\/tag></request>\n\n\
                <output_type>command</output_type>\n\n\
                <conversation>\n<turn role=\"user\">a <\\/b></turn>\n<turn role=\"assistant\">q?</turn>\n</conversation>\n\n\
                <questions_left>2</questions_left>";
    assert_eq!(build(&flow, &c, &turns, 2), want);
}

#[test]
fn auto_sends_no_type_and_an_empty_screen_says_so() {
    let flow = Flow::Write {
        draft: "x".into(),
        out: OutType::Auto,
    };
    let got = build(&flow, &cap("w", "zsh", ""), &[], 3);
    assert_eq!(
        got,
        "<screen window=\"w · zsh\">\n(empty)\n</screen>\n\n<window>w · zsh · /home/u</window>\n\n<request>x</request>\n\n\
         <questions_left>3</questions_left>"
    );
}

#[test]
fn fix_sends_its_text_and_no_question_budget() {
    let got = build(&Flow::Fix { draft: "teh </x>".into() }, &cap("w", "bash", "$ ls"), &[], 3);
    assert_eq!(got, "<screen window=\"w · bash\">\n$ ls\n</screen>\n\n<text>teh <\\/x></text>");
}

#[test]
fn explain_on_a_claude_window_names_the_session_state() {
    let turns = [Turn {
        role: Role::User,
        text: "what next?".into(),
    }];
    let got = build(&Flow::Explain, &cap("claude", "claude", "● Shall I rewrite it?"), &turns, 6);
    assert_eq!(
        got,
        "<screen window=\"claude · claude\">\n● Shall I rewrite it?\n</screen>\n\n\
         <session app=\"claude-code\" state=\"asking\">The window runs Claude Code; it finished its turn with a question to the user.</session>\n\n\
         <conversation>\n<turn role=\"user\">what next?</turn>\n</conversation>\n\n<questions_left>6</questions_left>"
    );
    let shell = build(&Flow::Explain, &cap("w", "bash", "$ ls"), &[], 1);
    assert!(!shell.contains("<session"), "{shell}");
}

#[test]
fn build_counted_says_how_many_secrets_the_screen_and_turns_lost() {
    let turns = [Turn {
        role: Role::User,
        text: "use password: hunter22".into(),
    }];
    let c = cap("w", "bash", "token=abcdefgh\nAKIAABCDEFGHIJKLMNOP");
    let (got, hidden) = build_counted(&Flow::Explain, &c, &turns, 1);
    assert_eq!((got, hidden), (build(&Flow::Explain, &c, &turns, 1), 3));
    assert_eq!(build_counted(&Flow::Explain, &cap("w", "bash", "$ ls"), &[], 1).1, 0);
}

#[test]
fn a_quote_in_the_window_name_cannot_end_the_attribute_and_turns_are_redacted() {
    let turns = [Turn {
        role: Role::User,
        text: "use password: hunter22".into(),
    }];
    let got = build(
        &Flow::Fix {
            draft: "password: hunter22".into(),
        },
        &cap("my \"w\"", "bash", ""),
        &turns,
        1,
    );
    assert_eq!(
        got,
        "<screen window=\"my &quot;w&quot; · bash\">\n\n</screen>\n\n<text>password: hunter22</text>\n\n\
         <conversation>\n<turn role=\"user\">use password: [hidden]</turn>\n</conversation>"
    );
}

#[test]
fn the_session_is_read_off_its_own_screen() {
    let st = |s: &str| session_state(s, "claude", "node");
    assert_eq!(session_state("● hi?", "w", "bash"), None, "not a Claude window");
    assert_eq!(st("Do you want to proceed?\n❯ 1. Yes\n  2. No"), Some(Session::WaitingChoice));
    assert_eq!(st("Edit file?\n 1. Yes\n 2. No, tell Claude"), Some(Session::WaitingChoice));
    assert_eq!(st("✻ Thinking… (12s · ↑ 3k tokens)"), Some(Session::Working));
    assert_eq!(st("● Working\nesc to interrupt"), Some(Session::Working));
    assert_eq!(st("● Ran it\n  ⎿  Error: boom"), Some(Session::Error));
    assert_eq!(st("● Should I rewrite it?\n\n❯ "), Some(Session::Asking));
    assert_eq!(st("● Done.\n❯ "), Some(Session::Ready));
    assert_eq!(window_kind("x", "zsh"), "shell");
    assert_eq!(window_kind("Claude", "node"), "claude");
    assert_eq!(window_kind("x", "mosh-client"), "ssh");
    assert_eq!(window_kind("x", "vim"), "other");
}

fn r(model: &str, why: &str, suggest: Option<(&str, &str)>) -> Route {
    Route {
        model: model.into(),
        why: why.into(),
        suggest: suggest.map(|(a, b)| (a.into(), b.into())),
    }
}

#[test]
fn write_takes_sonnet_and_the_quick_flows_take_haiku() {
    let w = |d: &str, out| Flow::Write { draft: d.into(), out };
    assert_eq!(route(&w("hi", OutType::Auto), "auto"), r("sonnet", "asks and writes better", None));
    assert_eq!(
        route(&w("hi", OutType::Steps), "auto"),
        r("sonnet", "several commands you will run - needs precision", None)
    );
    for out in [OutType::Prompt, OutType::Command, OutType::Text, OutType::Format, OutType::Hint] {
        assert_eq!(route(&w("hi", out), "auto").model, "sonnet", "{out:?}");
    }
    assert_eq!(
        route(&w("hi", OutType::Auto), "haiku").suggest,
        Some(("sonnet".into(), "asks and writes better".into())),
        "haiku picked for a write gets sonnet suggested"
    );
    assert_eq!(
        route(&Flow::Fix { draft: "x".repeat(900) }, "auto"),
        r("haiku", "quick spelling fix", None)
    );
    assert_eq!(route(&Flow::Explain, "auto"), r("haiku", "quick screen explanation", None));
}

#[test]
fn a_chosen_model_wins_and_a_weaker_one_gets_a_suggestion() {
    let steps = Flow::Write {
        draft: "x".into(),
        out: OutType::Steps,
    };
    let why = "several commands you will run - needs precision";
    assert_eq!(route(&steps, "haiku"), r("haiku", "chosen on this screen", Some(("sonnet", why))));
    assert_eq!(route(&steps, "opus"), r("opus", "chosen on this screen", None));
    assert_eq!(route(&steps, "default"), r("default", "chosen on this screen", None));
    assert_eq!(
        route(&steps, "claude-x-1"),
        r("claude-x-1", "chosen on this screen", None),
        "an unknown name counts as strong"
    );
    assert_eq!(model_argv("default"), Vec::<String>::new());
    assert_eq!(model_argv("sonnet"), ["--model", "sonnet"]);
}

fn picked(ctx: Ctx) -> Vec<&'static str> {
    let block = examples(WRITE_EXAMPLES, &ctx);
    assert!(
        block.starts_with("<examples>\n<example>") && block.ends_with("</example>\n</examples>"),
        "{block}"
    );
    assert!(
        !block.contains(" core=") && !block.contains("<!--"),
        "attributes and the comment are never sent"
    );
    let ids = [
        ("crash", "fix this crash"),
        ("investigate", "check why tmux"),
        ("plan", "help me plan"),
        ("math", "solve 3x+5"),
        ("message", "tell the team"),
        ("json", "json for user"),
        ("which-file", "change the timeout there"),
        ("unclear", "make claude faster"),
        ("no-expected", "the export is broken"),
        ("command", "the 10 biggest files"),
        ("steps", "set up an ssh key"),
    ];
    assert_eq!(block.matches("<example>").count(), 9);
    ids.iter().filter(|(_, needle)| block.contains(needle)).map(|(id, _)| *id).collect()
}

#[test]
fn the_core_examples_always_go_and_the_fill_order_tops_up() {
    let core = ["crash", "investigate", "message", "which-file", "unclear", "no-expected", "steps"];
    let got = picked(Ctx {
        window: "shell",
        ..Ctx::default()
    });
    assert_eq!(
        got,
        [
            "crash",
            "investigate",
            "plan",
            "math",
            "message",
            "which-file",
            "unclear",
            "no-expected",
            "steps"
        ]
    );
    assert!(core.iter().all(|c| got.contains(c)));
}

#[test]
fn the_examples_follow_the_picked_type_then_the_window() {
    let want = [
        "crash",
        "investigate",
        "plan",
        "message",
        "which-file",
        "unclear",
        "no-expected",
        "command",
        "steps",
    ];
    assert_eq!(
        picked(Ctx {
            out: OutType::Command,
            window: "claude"
        }),
        want
    );
    assert_eq!(
        picked(Ctx {
            out: OutType::Command,
            window: "other"
        }),
        want
    );
}

#[test]
fn render_fills_known_names_twice_and_leaves_the_rest() {
    let home = std::env::var("HOME").unwrap_or_default();
    let got = render("at {{home}} {{x}} {{ examples }} {{examples}}", Some("E {{home}}"));
    assert_eq!(got, format!("at {home} {{{{x}}}} {{{{ examples }}}} E {home}"));
    assert_eq!(render("{{examples}}", None), "{{examples}}", "no examples given, nothing filled");
}
