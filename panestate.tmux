#!/usr/bin/env bash
# tmux-panestate -- ours, not a tpm plugin. Load with:
#
# Agent state is a per-pane tmux option (@panestate), written by Claude Code
# hooks via scripts/agent-state. The border colour, window dots and the
# cross-window alert are all plain tmux formats in ~/.tmux.conf, so a redraw
# costs no shell spawns. Only the ssh-aware path/branch still needs a script.
#   run-shell ~/.tmux/plugins/tmux-panestate/panestate.tmux
#
# Tokens it expands in status-left / status-right:
#   #{agent_where_path}   ~/path locally, ssh destination when remote
#   #{agent_where_branch} git branch locally, blank when remote
set -u

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

interpolate() {
    local s="$1"
    # Build replacements in variables first. Inlining them into ${s//pat/rep}
    # breaks: the replacement text contains "}" (from #{pane_pid}), and bash
    # ends the parameter expansion at that first "}", appending the remainder
    # as literal garbage.
    local r_path="#($DIR/scripts/tmux-where path #{pane_pid} '#{pane_current_path}' '#{pane_current_command}')"
    local r_brch="#($DIR/scripts/tmux-where branch #{pane_pid} '#{pane_current_path}' '#{pane_current_command}')"
    s="${s//'#{agent_where_path}'/$r_path}"
    s="${s//'#{agent_where_branch}'/$r_brch}"
    printf '%s' "$s"
}

# Rewrite an option in place, only if it still holds an un-interpolated token.
update_option() {
    local opt="$1" val
    val=$(tmux show-option -gqv "$opt")
    [ -n "$val" ] || return 0
    case "$val" in
        *'#{agent_'*) tmux set-option -gq "$opt" "$(interpolate "$val")" ;;
    esac
}

# Exported so the Claude Code hooks can find agent-state without a hardcoded path.
tmux set-environment -g TMUX_PANESTATE_DIR "$DIR"

for opt in \
           @catppuccin_directory_text @catppuccin_gitmux_text; do
    update_option "$opt"
done
