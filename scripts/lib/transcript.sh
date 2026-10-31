#!/usr/bin/env bash
# What an agent is doing, read from its Claude Code transcript. Sourced, never run.
# The transcript format is internal to Claude Code, so every reader fails quietly:
# no output and status 1, never an error on stderr.

# The transcript of a pane's session: @cockpit_session, else `claude agents --json`
# joined on the pane's process tree. Prints the .jsonl path, or nothing (status 1).
transcript_file() {           # <pane-id>
    return 1
}

# Three rows for prefix i, tab separated, in this order, a row left out when its
# field is unknown:
#   doing<TAB><tool>: <description><TAB><seconds since the tool started, or empty>
#   said<TAB><last assistant text, first line, at most 160 chars>
#   asked<TAB><last string prompt, first line, at most 160 chars><TAB><seconds since>
transcript_doing() {          # <jsonl-path>
    return 1
}

# The transcript tail reduced to lines another agent can read, oldest first:
#   USER: <prompt> | AGENT: <text> | TOOL <name>: <description>
# Capped at <max-bytes> (default 4096), cut at a line boundary from the top.
transcript_tail() {           # <jsonl-path> [max-bytes]
    return 1
}
