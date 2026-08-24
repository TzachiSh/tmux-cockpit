#!/usr/bin/env bash
# Server CPU burned per OSC-2 title set -- the rate-independent form of the law.
#
# One pane emits exactly K title sequences and then goes quiet. Server CPU
# divided by K is what each title update costs, for each config element and
# each server shape. Multiply by the agent's real title rate to get the load.
set -u
source "$(dirname "$0")/lib.sh"

K=${K:-400}

per_title() {
    local spid c0 c1 pid
    spid=$(server_pid)
    pid=$(srv list-panes -a -F '#{pane_id}' | head -1)
    sleep 0.5
    c0=$(cpu_ticks "$spid")
    srv respawn-pane -k -t "$pid" "$(agent_cmd burst "$K")" 2>/dev/null
    # the burst is synchronous in the pane; give tmux time to drain it
    sleep 4
    c1=$(cpu_ticks "$spid")
    awk -v d="$((c1-c0))" -v hz="$(getconf CLK_TCK)" -v k="$K" \
        'BEGIN{printf "%.3f", (d/hz)*1000/k}'
}

row() {             # row <S> <W> <P> <variants...>
    local S=$1 W=$2 P=$3; shift 3
    local V panes vis
    for V in "$@"; do
        rig_start "fzpt-$$"
        rig_shape "$S" "$W" "$P" >/dev/null 2>&1
        rig_states 4 >/dev/null 2>&1
        rig_attach >/dev/null 2>&1
        panes=$(srv list-panes -a -F x | wc -l)
        vis=$(srv list-panes -t s1 -F x | wc -l)
        reset_variant; apply_variant "$V"
        printf '%-4s %-26s %-8s %-6s %-4s %s\n' \
            "$V" "$(variant_desc "$V")" "${S}x${W}x${P}" "$panes" "$vis" "$(per_title)"
        rig_cleanup
    done
}

printf '\n%-4s %-26s %-8s %-6s %-4s %s\n' VAR ELEMENT SxWxP PANES VIS 'ms CPU per OSC-2 title'
for shape in "1 6 2" "3 10 4" "5 20 4"; do
    row $shape V0 V3 V5 V7
done
