#!/usr/bin/env bash
# THE CI PROXY: how many panes does tmux visit to draw ONE status redraw?
#
# This is a COUNT, not a timing. It is identical on a fast laptop and a slow CI
# box, it does not care about load from other jobs, and it is exactly the
# quantity the freeze is made of: every visit is a format_create +
# format_defaults + format_free for one pane (format.c format_loop_panes).
#
# It works by running the rig's tmux server with -vv. format.c logs one
# "pane loop: %N" line per pane visited by a #{P:} walk (and "session loop:" /
# "window loop:" for #{S:} / #{W:}), gated on log_get_level() != 0. We zero the
# log, force exactly one redraw, and count.
#
# status-interval is 0 and every pane is idle, so the forced redraw is the only
# redraw in the window -- that is what makes the count reproducible.
set -u
source "$(dirname "$0")/lib.sh"

REPS=${REPS:-5}
LOGDIR="${TMPDIR:-/tmp}/fzcount-$$"

start_logged() {        # start_logged <sockname>
    SOCK="$1"
    tmux -L "$SOCK" kill-server 2>/dev/null
    mkdir -p "$LOGDIR"; rm -f "$LOGDIR"/tmux-server-*.log
    ( cd "$LOGDIR" && tmux -L "$SOCK" -vv -f /dev/null \
        new-session -d -s s1 -x 200 -y 62 \; \
        set -g status-interval 0 \; \
        set -g history-limit 2000 \; \
        set -g mouse on \; \
        set -g @cockpit_agents_glob "$GLOB" \; \
        set -g @thm_red '#ff6b6b' \; set -g @thm_peach '#ff9838' \; \
        set -g @thm_yellow '#ffcc33' \; set -g @thm_green '#3dea7d' \; \
        set -g @thm_overlay_0 '#5b6272' \; set -g @cockpit_blocked_bg '#52303a' \; \
        set -g status-left '' \; set -g status-right '' \; \
        set -g window-status-format '#W' \; set -g window-status-current-format '#W' )
}

logfile() { ls "$LOGDIR"/tmux-server-*.log 2>/dev/null | head -1; }

# pane visits caused by exactly one status redraw
visits1() {
    local lf before after; lf=$(logfile)
    [ -n "$lf" ] || { echo NOLOG; return; }
    srv refresh-client 2>/dev/null; sleep 1.0
    # Count by DIFFERENCE, never by truncating: tmux holds the log open and
    # writes continuously, so truncation races its writes and under-counts.
    before=$(grep -c 'pane loop: %' "$lf" 2>/dev/null); before=${before:-0}
    srv refresh-client 2>/dev/null
    sleep 1.0
    after=$(grep -c 'pane loop: %' "$lf" 2>/dev/null); after=${after:-0}
    printf '%s' "$((after - before))"
}

# The MINIMUM over several samples. Occasionally a second redraw lands inside
# the sampling window and doubles the count; the floor is the true cost of one
# redraw, and it is what stays identical run to run and machine to machine.
visits() {
    local i v min=""
    for ((i=0; i<REPS; i++)); do
        v=$(visits1)
        case "$v" in NOLOG) echo NOLOG; return ;; esac
        if [ -z "$min" ] || [ "$v" -lt "$min" ]; then min=$v; fi
    done
    printf '%s' "$min"
}

printf '\n%-4s %-30s %-8s %-6s %-7s %s\n' \
    VAR ELEMENT SxWxP PANES VISITS 'visits/pane (server walks per redraw)'
for shape in "2 8 3"; do
  set -- $shape
  for V in V0 V2 V3 V4 V5 V7; do
    start_logged "fzcnt-$$"
    rig_shape "$1" "$2" "$3" >/dev/null 2>&1
    rig_states 4 >/dev/null 2>&1
    rig_attach >/dev/null 2>&1
    reset_variant; apply_variant "$V"
    panes=$(srv list-panes -a -F x | wc -l)
    n=$(visits)
    printf '%-4s %-30s %-8s %-6s %-7s %s\n' \
        "$V" "$(variant_desc "$V")" "$1x$2x$3" "$panes" "$n" \
        "$(awk -v n="$n" -v p="$panes" 'BEGIN{printf "%.2f", p?n/p:0}')"
    rig_cleanup
  done
done
rm -rf "$LOGDIR"
