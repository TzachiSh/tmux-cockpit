#!/usr/bin/env bash
# Sourced, never run. Builtins only, bash 3.2 safe: agent-poll loads it and calls
# it once per pane on every tick, and workdir.sh once per pane on a goto walk.

# A command name as the agent tests read it: the last path part, without its
# leading dashes. Sets CMD_BASE, so a loop over panes forks nothing. The shell
# twin of cmd_base in tui/src/data.rs (#799). Case is left to each caller's
# pattern: version_cmd's holds no letter, and agent-poll's claude_cmd spells out
# both cases.
cmd_base() {
    CMD_BASE=${1##*/}
    CMD_BASE=${CMD_BASE#"${CMD_BASE%%[!-]*}"}
}

# Is this pane command the native install's version-shaped title (2.1.318)? After
# cmd_base: a leading digit, then only digits, dots and underscores, two or more
# of them dots or underscores. The shell twin of version_shaped in
# tui/src/data.rs (#761).
version_cmd() {
    cmd_base "$1"
    case $CMD_BASE in
        *[!0-9._]*) return 1 ;;
        [0-9]*[._]*[._]*) return 0 ;;
    esac
    return 1
}
