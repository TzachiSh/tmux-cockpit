//! Reply parsing: one literal reply per behaviour, pure functions only.
use cockpit_tui::panel::envelope::OutType;
use cockpit_tui::panel::parse::{Opt, Parsed, Reply, Step, cut_off, explain, fix, write};

fn opt(label: &str, recommended: bool) -> Opt {
    Opt {
        label: label.into(),
        recommended,
    }
}

fn step(note: &str, cmd: &str, lang: &str) -> Step {
    Step {
        note: note.into(),
        cmd: cmd.into(),
        lang: lang.into(),
    }
}

fn reply(out: &str) -> Reply {
    write(out, OutType::Auto).reply
}

#[test]
fn a_question_keeps_four_options_drops_other_and_counts_one_recommended() {
    let out = "<question>Which \n file?</question>\n<option recommended=\"true\">~/a</option>\n<option>Other</option>\n\
               <option recommended=\"true\">~/b</option>\n<option>c</option><option>d</option><option>e</option>";
    let want = Reply::Question {
        text: "Which file?".into(),
        options: vec![opt("~/a", true), opt("~/b", false), opt("c", false), opt("d", false)],
    };
    assert_eq!(reply(out), want);
    assert_eq!(fix(out), want, "Fix reads questions the same way");
    assert_eq!(explain(out), want, "and so does Explain");
}

#[test]
fn an_option_with_another_attribute_value_is_not_an_option() {
    let out = "<question>q</question><option recommended=\"false\">x</option><option>y</option>";
    assert_eq!(
        reply(out),
        Reply::Question {
            text: "q".into(),
            options: vec![opt("y", false)]
        }
    );
}

#[test]
fn other_like_labels_are_dropped_but_a_word_starting_with_other_is_not() {
    let out = "<question>q</question><option>Something else</option><option>None of the above</option>\
               <option>Otherwise fine</option>";
    assert_eq!(
        reply(out),
        Reply::Question {
            text: "q".into(),
            options: vec![opt("Otherwise fine", false)]
        }
    );
}

#[test]
fn a_legacy_question_line_after_a_preamble_is_a_question() {
    let out = "Sure.\nQUESTION: Which one?\n- first\n2) second\n- Other option\nnot a bullet";
    let want = Reply::Question {
        text: "Which one?".into(),
        options: vec![opt("first", false), opt("second", false)],
    };
    assert_eq!(reply(out), want);
    assert_eq!(fix(out), want);
}

#[test]
fn each_result_tag_has_its_kind() {
    assert_eq!(reply("<prompt>\n  Fix it  \n</prompt>"), Reply::Prompt("Fix it".into()));
    assert_eq!(reply("<text> Hi team </text>"), Reply::Text("Hi team".into()));
    assert_eq!(reply("<hint># Solution\n- x = 5</hint>"), Reply::Hint("# Solution\n- x = 5".into()));
    assert_eq!(
        reply("<format lang=\"JSON\">\n{\"a\": 1}\n  </format>"),
        Reply::Format {
            lang: "json".into(),
            body: "{\"a\": 1}".into()
        }
    );
    assert_eq!(
        reply("<format>k = 1</format>"),
        Reply::Format {
            lang: "".into(),
            body: "k = 1".into()
        }
    );
}

#[test]
fn a_command_is_a_one_step_list_and_bash_by_default() {
    assert_eq!(
        reply("<command lang=\"Bash\">\nls -la\n</command>"),
        Reply::Steps {
            steps: vec![step("", "ls -la", "bash")],
            command: true
        }
    );
    assert_eq!(
        reply("<command>du -sh .</command>"),
        Reply::Steps {
            steps: vec![step("", "du -sh .", "bash")],
            command: true
        }
    );
}

#[test]
fn steps_carry_notes_commands_and_bare_actions() {
    let out = "<steps>\n<step><note>1. Make key</note><cmd lang=\"bash\">ssh-keygen</cmd></step>\n\
               <step>Tap Allow in the app\n<step><note>Test</note><cmd>ssh lab <\\/dev/null</cmd></step>\n</steps>";
    let want = vec![
        step("Make key", "ssh-keygen", "bash"),
        step("Tap Allow in the app", "", ""),
        step("Test", "ssh lab </dev/null", "bash"),
    ];
    assert_eq!(
        reply(out),
        Reply::Steps {
            steps: want,
            command: false
        }
    );
}

#[test]
fn an_empty_steps_reply_is_shown_raw() {
    assert_eq!(reply("<steps></steps>"), Reply::Untagged("<steps></steps>".into()));
}

#[test]
fn a_shell_format_block_is_split_into_steps() {
    let out =
        "<format lang=\"bash\"># Step 1. update\napt update\napt upgrade\n\n# check\nif true; then\n  echo ok\nfi\n# done note\n</format>";
    let want = vec![
        step("update", "apt update", "bash"),
        step("", "apt upgrade", "bash"),
        step("check", "if true; then\n  echo ok\nfi", "bash"),
        step("done note", "", ""),
    ];
    assert_eq!(
        reply(out),
        Reply::Steps {
            steps: want,
            command: false
        }
    );
}

#[test]
fn the_earliest_tag_wins() {
    assert_eq!(reply("<text>hi</text> <prompt>p</prompt>"), Reply::Text("hi".into()));
    assert_eq!(
        reply("<prompt>use the <text> tag</prompt>"),
        Reply::Prompt("use the <text> tag".into())
    );
    assert_eq!(reply("<hint>h</hint><question>q</question>"), Reply::Hint("h".into()));
}

#[test]
fn an_unclosed_question_is_not_a_question() {
    assert_eq!(reply("<question>which?"), Reply::Untagged("<question>which?".into()));
}

#[test]
fn an_unclosed_result_tag_is_kept_and_flagged_cut_off() {
    assert_eq!(
        write("<prompt>Fix the thing and", OutType::Auto),
        Parsed {
            reply: Reply::Prompt("Fix the thing and".into()),
            cut_off: Some("prompt")
        }
    );
    assert_eq!(write("<prompt>done</prompt>", OutType::Auto).cut_off, None);
    assert_eq!(cut_off("intro <question>q"), None, "a question is not a result tag");
    assert_eq!(cut_off("<steps>\n<step>x"), Some("steps"));
    assert_eq!(cut_off("no tags"), None);
}

#[test]
fn file_edits_are_unsupported_for_now() {
    assert_eq!(
        reply("<edit path=\"/x\"><change><old>a</old><new>b</new></change></edit>"),
        Reply::Unsupported("edit")
    );
    assert_eq!(
        reply("I need it. <need_file path=\"/etc/hosts\"/>"),
        Reply::Unsupported("need_file")
    );
}

#[test]
fn plain_text_is_the_hint_when_a_hint_was_asked() {
    assert_eq!(write("x = 5", OutType::Hint).reply, Reply::Hint("x = 5".into()));
    assert_eq!(write("<text>t</text>", OutType::Hint).reply, Reply::Hint("t".into()));
    assert_eq!(write("x = 5", OutType::Auto).reply, Reply::Untagged("x = 5".into()));
}

#[test]
fn a_reply_fenced_whole_is_read_from_inside() {
    assert_eq!(reply("```xml\n<prompt>p</prompt>\n```"), Reply::Prompt("p".into()));
}

#[test]
fn replies_are_unescaped() {
    assert_eq!(reply("<prompt>close <\\/div></prompt>"), Reply::Prompt("close </div>".into()));
    assert_eq!(fix("fix <\\/b> here"), Reply::Text("fix </b> here".into()));
}

#[test]
fn explain_splits_the_prompt_for_the_agent_out() {
    assert_eq!(
        explain("Run the tests.\n<prompt>Fix <\\/div> in a.html</prompt>"),
        Reply::Answer {
            text: "Run the tests.".into(),
            ready: Some("Fix </div> in a.html".into())
        }
    );
    assert_eq!(
        explain("It is idle."),
        Reply::Answer {
            text: "It is idle.".into(),
            ready: None
        }
    );
    assert_eq!(
        explain("Tell it:\n<prompt>go on"),
        Reply::Answer {
            text: "Tell it:".into(),
            ready: Some("go on".into())
        },
        "a cut-off prompt still counts"
    );
    assert_eq!(
        explain("Nothing.<prompt> </prompt>"),
        Reply::Answer {
            text: "Nothing.".into(),
            ready: None
        }
    );
}
