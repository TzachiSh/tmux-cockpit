//! basket::claude_cmd, one row per command (#365 child 5, D4).
use cockpit_tui::basket::claude_cmd;

#[test]
fn claude_and_its_version_title_count_and_no_other_agent_does() {
    let rows = [
        ("claude", true),
        ("2.1.318", true),
        ("/usr/local/bin/claude", true),
        ("-claude", true),
        ("Claude", true),
        ("codex", false),
        ("gemini", false),
        ("zsh", false),
        ("claude-wrapper", false),
        ("1.2", false),
        ("", false),
    ];
    for (cmd, want) in rows {
        assert_eq!(claude_cmd(cmd), want, "{cmd:?}");
    }
}
