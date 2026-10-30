#!/usr/bin/env bash
# The side fixer's liveness, for tmux-fixer, review-gate and tmux-review.
# Sourced, never run.

# Alive iff recorded, not handed over, before its deadline, and its pane exists
# ("pending" until the split). No pid or lock: a dead process leaves no live pane.
fixer_alive() {               # <sess>
    local f="$1/fixer" p dl
    [ -d "$f" ] || return 1
    [ -f "$1/handover" ] && return 1
    dl=$(cat -- "$f/deadline" 2>/dev/null); case "$dl" in ''|*[!0-9]*) return 1 ;; esac
    [ "$(date +%s)" -lt "$dl" ] || return 1
    p=$(cat -- "$f/pane" 2>/dev/null)
    fixer_pane_alive "$p"
}
fixer_pane_alive() {          # <the pane id the record holds>
    [ "$1" = pending ] && return 0
    tmux list-panes -a -F '#{pane_id}' 2>/dev/null | grep -qx -- "$1"
}
