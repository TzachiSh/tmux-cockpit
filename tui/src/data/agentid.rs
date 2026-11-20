//! Whether a pane runs Claude Code (#1359): one rule over facts its caller already
//! holds, so the basket, the panel and the board cannot answer differently for the
//! same pane. It makes no tmux call of its own.

use super::{cmd_base, version_shaped};

/// A command that is Claude Code's: `claude` in any case, from any path, or its
/// version-shaped process title (`2.1.318`), as data's agentish reads them.
pub fn claude_cmd(comm: &str) -> bool {
    let b = cmd_base(comm);
    b == "claude" || version_shaped(&b)
}

/// The pane runs Claude Code: the board calls it an agent's (`agent`, #519's
/// `data::is_agent`) and its command, tmux's `pane_current_command`, is
/// [`claude_cmd`]. @panestate alone is not enough, as it outlives the claude that
/// set it, and an agent list that leaves claude out makes it no agent's. The
/// window name is no evidence (#1359): a user names a window anything.
pub fn is_claude(cmd: &str, agent: bool) -> bool {
    agent && claude_cmd(cmd)
}

#[cfg(test)]
mod tests {
    use super::is_claude;

    /// #1359's table: the pane's command, whether the board calls it an agent's,
    /// and the answer. The window name is not an input, so its rows read as the
    /// same command elsewhere.
    #[test]
    fn one_answer_per_pane() {
        let rows = [
            ("1 claude, an agent", "claude", true, true),
            ("2 the version title", "2.1.318", true, true),
            ("3 a path to claude", "/usr/local/bin/claude", true, true),
            ("3 claude in upper case", "CLAUDE", true, true),
            ("4 a shell in a window named claude, no agent", "zsh", false, false),
            ("5 a stale @panestate on a shell", "fish", true, false),
            ("6 node in a window named claude, an agent", "node", true, false),
            ("7 node", "node", false, false),
            ("8 claude, off the agent list", "claude", false, false),
            ("9 claude, no agent list", "claude", false, false),
            ("10 claude off the list, marked", "claude", true, true),
            ("11 another agent", "codex", true, false),
            ("12 a name past comm's 15 bytes", "claude-code-nightly", true, false),
            ("13 a wrapper: bash, marked", "bash", true, false),
            ("an empty command", "", true, false),
        ];
        for (row, cmd, agent, want) in rows {
            assert_eq!(is_claude(cmd, agent), want, "{row}");
        }
    }
}
