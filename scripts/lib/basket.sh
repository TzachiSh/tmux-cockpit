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
# The test reads 1 only for a compare read whole (#736). tmux 3.7+ cuts a format
# that runs past 100 ms of wall time to "" from that point on, and a bare == read
# the two cut sides as equal and cleared a basket a viewer had added to. A cut
# now keeps the basket, so its refs go out again with the next send. A cut
# option name ends the list early, with the basket kept too (#1450).
# 0: cleared. 1: kept, as the basket changed or tmux cut the compare. 2: <pane>
# is not %<digits>, and no tmux runs.
basket_clear() {              # <pane> <sent-file>
    is_pane "${1-}" || return 2
    local sent
    sent=$(cat "$2")
    tmux set-option -p -t "$1" @cockpit_basket_sent "$sent"$'\n' \; \
        if-shell -F -t "$1" '#{?#{==:#{@cockpit_basket},#{@cockpit_basket_sent}},1,0}' \
            "set-option -pu -t $1 @cockpit_basket" \; \
        set-option -pu -t "$1" @cockpit_basket_sent 2>/dev/null
    [ -z "$(tmux show-options -pqv -t "$1" @cockpit_basket 2>/dev/null)" ]
}
