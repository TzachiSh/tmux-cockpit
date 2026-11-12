//! #453: the info screen reads the open AskUserQuestion's question as the
//! state's message. Its own binary: `gather` reads COCKPIT_TMUX and
//! CLAUDE_CONFIG_DIR, process-global, so no other test shares them.
mod fake;

use cockpit_tui::data::info::gather;

/// A shimmed tmux whose pane %1 is in `state` with the hook's generic text, its
/// transcript `body`. Returns the gathered (state, msg).
fn facts(tag: &str, state: &str, body: &str) -> (String, String) {
    let d = std::env::temp_dir().join(format!("cockpit-info-ask-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("projects/-w")).unwrap();
    let t = d.join("projects/-w/s1.jsonl");
    std::fs::write(&t, body).unwrap();
    let us = '\u{1f}';
    let f = [
        "%1",
        "w:0 win",
        "claude",
        "/",
        "",
        state,
        "Claude needs your permission",
        "",
        "",
        "s1",
        "",
        "1",
        "",
    ];
    std::fs::write(d.join("display"), f.join(&us.to_string())).unwrap();
    // the board's FIELDS row, which the agent test (#519) reads: @panestate makes it one
    let board = ["%1", "w:0", "win", "claude", state, "-", "-", "s1", "/", "100", "-", "-", "@1"];
    std::fs::write(d.join("rows"), board.join(&us.to_string())).unwrap();
    let shim = d.join("tmux");
    let sh = format!(
        "#!/bin/sh\ncase \"$1\" in\ndisplay-message) case \"$5\" in *cockpit_tui_version*) cat {0}/display ;; \
         *) cat {0}/rows ;; esac ;;\nshow-option) echo {1} ;;\nesac\n",
        d.display(),
        t.display()
    );
    fake::write_exec(&shim, &sh);
    // each call adds its bin to PATH and the entry stays, dead, once `d` is removed
    let path = fake::rig_path(&d.join("bin"));
    // SAFETY: the one test in this binary, so no thread reads the environment meanwhile.
    unsafe {
        std::env::set_var("PATH", path);
        std::env::set_var("COCKPIT_TMUX", &shim);
        std::env::set_var("CLAUDE_CONFIG_DIR", &d);
    }
    fake::assert_claude_is_the_rigs(&d);
    let x = gather("%1").unwrap();
    let _ = std::fs::remove_dir_all(&d);
    (x.state, x.msg)
}

fn ask(id: &str, question: &str) -> String {
    let v = serde_json::json!({"type": "assistant", "version": "2.1.316", "message": {"content": [{"type": "tool_use",
        "id": id, "name": "AskUserQuestion", "input": {"questions": [{"question": question, "header": "H"}, {}]}}]}});
    format!("{v}\n")
}

/// Rows: an open question while the hooks say needs-input is the message; the
/// same while they say running leaves the hook's text; an answered one too.
#[test]
fn the_open_question_is_the_info_message_only_while_input_is_needed() {
    let answered = ask("q1", "Merge?")
        + &serde_json::json!({"type": "user", "version": "2.1.316",
            "message": {"content": [{"type": "tool_result", "tool_use_id": "q1"}]}})
        .to_string()
        + "\n";
    let generic = "Claude needs your permission";
    assert_eq!(facts("open", "needs-input", &ask("q1", "Merge\u{202e}?")).1, "(+1) Merge?");
    assert_eq!(facts("running", "running", &ask("q1", "Merge?")).1, generic);
    assert_eq!(facts("answered", "needs-input", &answered).1, generic);
}
