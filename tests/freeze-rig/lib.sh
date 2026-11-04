#!/usr/bin/env bash
# tmux-cockpit freeze rig -- what gate.sh shares. Sourced, never run.
# Everything runs on a PRIVATE tmux server; nothing touches the user's server.
set -u

RIG_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO=${REPO:-$(cd -P "$RIG_DIR/../.." && pwd)}

GLOB='|claude|codex|aider|cursor|opencode|gemini|crush|'

SOCK=""
srv() { tmux -L "$SOCK" "$@"; }

# The precomputed options the bar reads, as agent-state writes them after the
# states the gate set by hand.
seed_sums() {
    TMUX="$(srv display-message -p '#{socket_path}'),0,0" "$REPO/scripts/agent-state" sweep
}

# The plugin's own bar (scripts/lib/statusbar.conf), what users run.
apply_bar() {
    srv set -g @cockpit_where_cmd "'$REPO/scripts/tmux-where'"
    srv source-file "$REPO/scripts/lib/statusbar.conf"; seed_sums
}
