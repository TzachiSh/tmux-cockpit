#!/usr/bin/env bash
# bash+fzf live fleet: rows from the Go spike's --rows (stand-in for tmux-review --fleet + doing), reload every 1s via --listen
rows() { /tmp/claude-1000/-home-tzachi--tmux-plugins-tmux-cockpit/be834b45-4c9c-4a15-a41c-9dcf2d13c037/scratchpad/bt-spike/cockpit-fleet --rows | awk -F'\t' '{printf "%s %-4s %-12s %-24.24s %s  %s\n",$1,$2,$3,$4,$5,$6}'; }
export -f rows; export B
port=$(( 20000 + RANDOM % 10000 ))
( while :; do sleep 1; curl -s -XPOST "localhost:$port" -d 'reload(rows)' >/dev/null || exit; done ) &
rows | fzf --listen "$port" --reverse --no-sort --header 'fleet · live · ↵ jump · esc' --preview 'tmux capture-pane -ep -t {2} | tail -n $FZF_PREVIEW_LINES' --preview-window 'down,60%' --bind 'enter:become(echo jump {2})'
kill %1 2>/dev/null
