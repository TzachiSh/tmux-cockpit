#!/usr/bin/env bash
# tmux-cockpit freeze rig -- what gate.sh shares. Sourced, never run.
# Everything runs on a PRIVATE tmux server; nothing touches the user's server.
set -u

RIG_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO=${REPO:-$(cd -P "$RIG_DIR/../.." && pwd)}

GLOB='|claude|codex|aider|cursor|opencode|gemini|crush|'

SOCK=""
# #195: an empty SOCK must never reach tmux. `-L ''` errors, but TMUX= with an empty
# path before its comma is the default socket, the user's live server.
srv() {
    [ -n "$SOCK" ] || {
        echo "freeze-rig: SOCK is empty; refusing to talk to tmux (#195)" >&2
        return 1
    }
    tmux -L "$SOCK" "$@"
}

# The precomputed options the bar reads, as agent-state writes them after the
# states the gate set by hand.
seed_sums() {
    local sp; sp=$(srv display-message -p '#{socket_path}')
    [ -n "$sp" ] || {
        echo "freeze-rig: no socket path for '$SOCK'; not running agent-state (#195)" >&2
        return 1
    }
    TMUX="$sp,0,0" "$REPO/scripts/agent-state" sweep
}

# The plugin's own bar (scripts/lib/statusbar.conf), what users run.
apply_bar() {
    srv set -g @cockpit_where_cmd "'$REPO/scripts/tmux-where'"
    srv source-file "$REPO/scripts/lib/statusbar.conf"; seed_sums
}
