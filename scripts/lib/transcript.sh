#!/usr/bin/env bash
# What an agent is doing, read from its Claude Code transcript. Sourced, never run.
# The reader is `cockpit-tui transcript`; any failure is status 1 and no output.
# shellcheck source=tui.sh
. "${BASH_SOURCE[0]%/*}/tui.sh" || return 1

# The pane's transcript path: @cockpit_session, else `claude agents --json`.
transcript_file() {           # <pane-id>
    "$COCKPIT_TUI_BIN" transcript file "${1:-}" 2>/dev/null
}

# doing / said / asked rows, tab separated, for prefix i.
transcript_doing() {          # <jsonl-path>
    "$COCKPIT_TUI_BIN" transcript doing "${1:-}" 2>/dev/null
}

# USER: / AGENT: / TOOL lines, oldest first, capped at [max-bytes] (default 4096).
transcript_tail() {           # <jsonl-path> [max-bytes]
    "$COCKPIT_TUI_BIN" transcript tail "${1:-}" "${2:-}" 2>/dev/null
}
