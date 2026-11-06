#!/usr/bin/env bash
# Sourced, never run. The prompt basket's side in the shell (#365 child 5).

# Unsets <pane>'s @cockpit_basket when it still holds the text in <sent-file>,
# the basket a send or insert carried (`.basket` beside the action, D7): a ref a
# viewer added since stays. One tmux command list, which the server runs with no
# other client's command in between (#582): the sent text goes into a second
# option, set without -F so it stays literal; if-shell -F compares the two
# options, so nothing is escaped (a `##[` in a format stays `##[`, review-510);
# the second option goes again whether or not the basket was cleared. The
# newline basket.rs ends the value with is put back, as $(...) drops it.
# 0: cleared. 1: kept, as the basket changed. 2: <pane> is not %<digits>, and no
# tmux runs.
basket_clear() {              # <pane> <sent-file>
    is_pane "${1-}" || return 2
    local sent
    sent=$(cat "$2")
    tmux set-option -p -t "$1" @cockpit_basket_sent "$sent"$'\n' \; \
        if-shell -F -t "$1" '#{==:#{@cockpit_basket},#{@cockpit_basket_sent}}' \
            "set-option -pu -t $1 @cockpit_basket" \; \
        set-option -pu -t "$1" @cockpit_basket_sent 2>/dev/null
    [ -z "$(tmux show-options -pqv -t "$1" @cockpit_basket 2>/dev/null)" ]
}
