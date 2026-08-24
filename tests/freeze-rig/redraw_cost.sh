#!/usr/bin/env bash
# Server CPU burned per REDRAW, per config element, per shape.
#
# The tmux server is single-threaded: CPU it spends expanding formats during a
# redraw is time every keystroke and every command sits in the queue. So
# "ms of server CPU per redraw" IS the freeze metric, and unlike sampled
# round-trip latency it is not noisy -- we force a known number of redraws and
# read /proc/<server>/stat around them.
set -u
source "$(dirname "$0")/lib.sh"

R=${R:-60}          # forced redraws per sample

redraw_cost() {     # redraw_cost <refresh-client flags...> -> ms of CPU/redraw
    local spid c0 c1 i
    spid=$(server_pid)
    srv refresh-client "$@" 2>/dev/null; sleep 0.3          # warm
    c0=$(cpu_ticks "$spid")
    for ((i=0; i<R; i++)); do
        srv refresh-client "$@" 2>/dev/null
        read -r -t 0.02 -u 9 x 2>/dev/null                  # let it land
    done 9<> <(:)
    sleep 0.3
    c1=$(cpu_ticks "$spid")
    awk -v d="$((c1-c0))" -v hz="$(getconf CLK_TCK)" -v r="$R" \
        'BEGIN{printf "%.1f", (d/hz)*1000/r}'
}

row() {             # row <S> <W> <P> <variants...>
    local S=$1 W=$2 P=$3; shift 3
    rig_start "fzrc-$$"
    rig_shape "$S" "$W" "$P" >/dev/null 2>&1
    rig_states 4 >/dev/null 2>&1
    rig_attach >/dev/null 2>&1
    local panes vis V
    panes=$(srv list-panes -a -F x | wc -l)
    vis=$(srv list-panes -t s1 -F x | wc -l)
    for V in "$@"; do
        reset_variant; apply_variant "$V"
        printf '%-4s %-26s %-9s %-6s %-4s %-9s %s\n' \
            "$V" "$(variant_desc "$V")" "${S}x${W}x${P}" "$panes" "$vis" \
            "$(redraw_cost -S)" "$(redraw_cost)"
    done
    rig_cleanup
}

printf '\n%-4s %-26s %-9s %-6s %-4s %-9s %s\n' \
    VAR ELEMENT SxWxP PANES VIS 'ms/status' 'ms/full-redraw'
case "${1:-elements}" in
elements)
    for shape in "1 6 2" "3 10 4" "5 20 4"; do
        row $shape V0 V1 V2 V3 V4 V5 V6 V7
    done ;;
scale)      # cost vs TOTAL panes, visible held at 4
    for shape in "1 4 4" "1 12 4" "2 12 4" "3 15 4" "4 20 4" "5 20 4" "6 25 4"; do
        row $shape V0 V3
    done ;;
visible)    # cost vs VISIBLE panes, total held near 240
    for shape in "3 40 2" "3 20 4" "3 13 6" "3 10 8" "3 6 13" "3 5 16"; do
        row $shape V0 V3
    done ;;
one) shift; S=$1 W=$2 P=$3; shift 3; row "$S" "$W" "$P" "$@" ;;
esac
