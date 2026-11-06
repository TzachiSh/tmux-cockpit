#!/usr/bin/env bash
# Sourced, never run. The prompt basket's side in the shell (#365 child 5).

# Unsets <pane>'s @cockpit_basket when it still holds the text in <sent-file>,
# the basket a send or insert carried (`.basket` beside the action, D7): a ref a
# viewer added since stays. The compare and the unset are one if-shell -F, which
# the server evaluates and runs with no other client's command in between, so no
# add can fall between them (#582). The sent text goes into the format literal:
# `#`, `,` and `}` escaped, and the newline basket.rs ends the value with put
# back, as $(...) drops it. 0: cleared. 1: kept, as the basket changed.
# 2: <pane> is not %<digits>, and no tmux runs.
basket_clear() {              # <pane> <sent-file>
    is_pane "${1-}" || return 2
    local sent
    sent=$(sed -e 's/#/##/g' -e 's/,/#,/g' -e 's/}/#}/g' "$2")
    tmux if-shell -F -t "$1" "#{==:#{@cockpit_basket},$sent"$'\n'"}" \
        "set-option -pu -t $1 @cockpit_basket" 2>/dev/null
    [ -z "$(tmux show-options -pqv -t "$1" @cockpit_basket 2>/dev/null)" ]
}
