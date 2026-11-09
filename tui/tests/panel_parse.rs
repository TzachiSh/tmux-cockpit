//! Reply parsing: one literal reply per behaviour, pure functions only.
use cockpit_tui::panel::envelope::OutType;
use cockpit_tui::panel::parse::{Opt, Parsed, Reply, Step, cut_off, explain, files_read, fix, tool_calls, write};

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

/// #283: the wrapper is highlight's fence, so a tag with a digit, a comma or a
/// space before it, and a longer fence, all wrap a reply as the view reads them.
#[test]
fn a_wrapper_fence_is_the_one_fence_rule() {
    let rows = [
        ("digit in the tag", "```python3\nprint(1)\n```"),
        ("attribute after a comma", "```text,ignore\nprint(1)\n```"),
        ("space before the tag", "``` text\nprint(1)\n```"),
        ("four backticks", "````\nprint(1)\n````"),
    ];
    for (name, out) in rows {
        assert_eq!(fix(out), Reply::Text("print(1)".into()), "row {name}");
    }
}

/// An outer fence around markdown that holds its own tagged block of the same
/// length still wraps the whole reply: the inner opener is paired with the
/// first bare closer, and the last line closes the outer one.
#[test]
fn a_wrapper_holding_a_paired_inner_fence_is_still_one() {
    let rows = [
        ("nested_same", "```\n# Title\n```sh\nls\n```\n```", "# Title\n```sh\nls\n```"),
        (
            "nested_md",
            "```markdown\nSee:\n```bash\nmake\n```\ndone\n```",
            "See:\n```bash\nmake\n```\ndone",
        ),
        ("nested_long", "````\n```sh\nls\n```\n````", "```sh\nls\n```"),
    ];
    for (name, out, want) in rows {
        assert_eq!(fix(out), Reply::Text(want.into()), "row {name}");
    }
}

/// A fence with nothing inside wraps nothing.
#[test]
fn an_empty_fence_is_not_a_wrapper() {
    assert_eq!(fix("```\n```"), Reply::Text("```\n```".into()));
}

/// Explain strips a bare wrapper too: it only keeps a fence that names a language.
#[test]
fn explain_strips_a_bare_wrapper() {
    let Reply::Answer { text, .. } = explain("```\nplain words\n```") else {
        panic!()
    };
    assert_eq!(text, "plain words");
}

/// A fence left open wraps nothing: the reply is read as it came.
#[test]
fn an_unclosed_fence_is_not_a_wrapper() {
    assert_eq!(fix("```py\nx = 1"), Reply::Text("```py\nx = 1".into()));
}

/// Two fenced blocks with prose between are not one block around the prose.
#[test]
fn a_reply_of_two_blocks_is_not_unwrapped() {
    let out = "```\na\n```\nmid\n```\nb\n```";
    assert_eq!(fix(out), Reply::Text(out.into()));
    let Reply::Answer { text, .. } = explain(out) else { panic!() };
    assert_eq!(text, out);
}

#[test]
fn replies_are_unescaped() {
    assert_eq!(reply("<prompt>close <\\/div></prompt>"), Reply::Prompt("close </div>".into()));
    assert_eq!(fix("fix <\\/b> here"), Reply::Text("fix </b> here".into()));
}

#[test]
fn a_reply_quoting_an_escaped_closer_gives_back_the_users_text() {
    // the user wrote `<\/div>`; the envelope showed the model `<\\/div>`
    assert_eq!(fix("keep <\\\\/div> as is"), Reply::Text("keep <\\/div> as is".into()));
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

// ---------------------------------------------------------------- files read (#416)

/// One assistant turn with these tool calls, `(id, name, path)`.
fn uses(calls: &[(&str, &str, &str)]) -> String {
    let c: Vec<String> = calls
        .iter()
        .map(|(id, name, p)| format!(r#"{{"type":"tool_use","id":"{id}","name":"{name}","input":{{"file_path":"{p}","pattern":"x"}}}}"#))
        .collect();
    format!(r#"{{"type":"assistant","message":{{"content":[{}]}}}}"#, c.join(","))
}

/// The tool results for these ids, `(id, is_error)`.
fn results(rs: &[(&str, bool)]) -> String {
    let c: Vec<String> = rs
        .iter()
        .map(|(id, e)| format!(r#"{{"type":"tool_result","tool_use_id":"{id}","is_error":{e},"content":"…"}}"#))
        .collect();
    format!(r#"{{"type":"user","message":{{"content":[{}]}}}}"#, c.join(","))
}

const RESULT: &str = r#"{"type":"result","subtype":"success","result":"<prompt>p</prompt>"}"#;

#[test]
fn files_read_lists_each_read_once_in_order() {
    let out = format!(
        "[{},{},{},{},{RESULT}]",
        uses(&[("a", "Read", "/r/b.rs"), ("b", "Grep", "/r")]),
        results(&[("a", false), ("b", false)]),
        uses(&[("c", "Read", "/r/a.rs"), ("d", "Read", "/r/b.rs"), ("e", "Glob", "/r")]),
        results(&[("c", false), ("d", false), ("e", false)]),
    );
    assert_eq!(files_read(&out), Some(vec!["/r/b.rs".to_string(), "/r/a.rs".to_string()]));
}

#[test]
fn files_read_leaves_out_a_refused_read() {
    let out = format!(
        "[{},{},{RESULT}]",
        uses(&[("a", "Read", "/r/.env"), ("b", "Read", "/r/main.rs")]),
        results(&[("a", true), ("b", false)])
    );
    assert_eq!(files_read(&out), Some(vec!["/r/main.rs".to_string()]));
}

#[test]
fn files_read_of_a_call_that_read_nothing_is_empty() {
    assert_eq!(files_read(&format!("[{RESULT}]")), Some(vec![]));
    assert_eq!(files_read("[]"), Some(vec![]));
}

#[test]
fn files_read_is_none_for_output_that_is_not_the_verbose_json() {
    for out in ["", "<prompt>p</prompt>", RESULT, "{", "[1, \"x\"", "null"] {
        assert_eq!(files_read(out), None, "{out:?}");
    }
}

#[test]
fn files_read_skips_malformed_entries() {
    let bad = r#"[{"type":"assistant","message":{"content":"text"}},
        {"type":"assistant","message":{"content":[{"type":"tool_use","id":"a","name":"Read","input":{"file_path":7}}]}},
        {"type":"assistant"}, 3]"#;
    assert_eq!(files_read(bad), Some(vec![]));
}

// ---------------------------------------------------------------- tool calls (#628)

/// One assistant turn with one tool call of this name and json input.
fn call(id: &str, name: &str, input: &str) -> String {
    let u = format!(r#"{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}"#);
    format!(r#"{{"type":"assistant","message":{{"content":[{u}]}}}}"#)
}

#[test]
fn tool_calls_name_each_read_and_search_with_its_arguments() {
    let out = format!(
        "[{},{},{},{RESULT}]",
        call("a", "Read", r#"{"file_path":"/r/a.rs"}"#),
        call("b", "Grep", r#"{"pattern":"fn main","path":"/r","output_mode":"content"}"#),
        call("c", "Glob", r#"{"pattern":"**/*.toml"}"#),
    );
    let want = ["Read /r/a.rs", "Grep fn main /r", "Glob **/*.toml"];
    assert_eq!(tool_calls(&out), Some(want.map(String::from).to_vec()));
}

#[test]
fn tool_calls_leave_out_a_refused_call_and_a_repeat() {
    let out = format!(
        "[{},{},{},{RESULT}]",
        uses(&[("a", "Read", "/r/.env"), ("b", "Read", "/r/main.rs")]),
        results(&[("a", true), ("b", false)]),
        uses(&[("c", "Read", "/r/main.rs")]),
    );
    assert_eq!(tool_calls(&out), Some(vec!["Read /r/main.rs x".to_string()]));
}

#[test]
fn tool_calls_of_a_call_that_ran_none_is_empty_and_of_other_output_none() {
    assert_eq!(tool_calls(&format!("[{RESULT}]")), Some(vec![]));
    for out in ["", "<prompt>p</prompt>", RESULT, "{", "null"] {
        assert_eq!(tool_calls(out), None, "{out:?}");
    }
}

#[test]
fn tool_calls_keep_a_call_with_no_string_argument_by_its_name() {
    let out = format!("[{}]", call("a", "Glob", r#"{"pattern":7}"#));
    assert_eq!(tool_calls(&out), Some(vec!["Glob".to_string()]));
}
