#!/usr/bin/env bash
# Sourced, never run. The prompt basket's side in the shell (#365 child 5).

# Unsets <pane>'s @cockpit_basket when it still holds the text in <sent-file>,
# the basket a send or insert carried (`.basket` beside the action, D7): a ref a
# viewer added since stays. 0: cleared. 1: kept, as the basket changed.
# 2: <pane> is not %<digits>, and no tmux runs.
basket_clear() {              # <pane> <sent-file>
    is_pane "${1-}" || return 2
    local held sent
    held=$(tmux show-options -pqv -t "$1" @cockpit_basket 2>/dev/null)
    sent=$(cat "$2")
    if [ "$held" = "$sent" ]; then
        tmux set-option -pu -t "$1" @cockpit_basket 2>/dev/null
        return 0
    fi
    return 1
}
