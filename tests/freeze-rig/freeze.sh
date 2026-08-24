#!/usr/bin/env bash
# tmux-cockpit freeze rig -- end-to-end responsiveness.
#
#   bash freeze.sh matrix                  config element x shape x load
#   bash freeze.sh one <S> <W> <P> <load> <n> [variants...]
#   bash freeze.sh scale [variant]         latency vs total panes
#   bash freeze.sh visible [variant]       latency vs visible panes
#   bash freeze.sh rate [variant]          latency vs title rate
#
# load: none | spam (plain lines) | title (OSC 2 only) | agent (both)
#
# Everything runs on a private tmux server. Nothing touches the user's server.
set -u
source "$(dirname "$0")/lib.sh"

DUR=${DUR:-12}
SAMPLE_MS=${SAMPLE_MS:-150}

# Round-trip latency of a trivial tmux command, sampled while the server works.
# This is the number the user feels: the tmux server is single-threaded, so
# time spent expanding formats is time a keystroke waits in the queue.
sample() {          # -> "p50 p95 max n_over_1s cpu%"
    local spid; spid=$(server_pid)
    local t_end=$(( $(date +%s%N) + DUR*1000000000 ))
    local c0 c1 w0 w1 vals=() t0 t1
    c0=$(cpu_ticks "$spid"); w0=$(date +%s%N)
    while [ "$(date +%s%N)" -lt "$t_end" ]; do
        t0=$(date +%s%N)
        timeout 30 tmux -L "$SOCK" display-message -p ok >/dev/null 2>&1
        t1=$(date +%s%N)
        vals+=( $(( (t1-t0)/1000000 )) )
        read -r -t "0.$(printf '%03d' "$SAMPLE_MS")" -u 9 x 2>/dev/null
    done 9<> <(:)
    c1=$(cpu_ticks "$spid"); w1=$(date +%s%N)
    printf '%s\n' "${vals[@]}" | sort -n | awk \
        -v hz="$(getconf CLK_TCK)" -v dc="$((c1-c0))" -v dw="$(( (w1-w0)/1000000 ))" '
        {v[NR]=$1} END{
          p50=v[int(NR*0.5)?int(NR*0.5):1]; p95=v[int(NR*0.95)?int(NR*0.95):1];
          over=0; for(i=1;i<=NR;i++) if(v[i]>1000) over++;
          printf "%s %s %s %s %.0f", p50, p95, v[NR], over, (dc/hz)*100000/dw }'
}

sweep() {           # sweep <S> <W> <P> <load> <n_load_panes> <variants...>
    local S=$1 W=$2 P=$3 LOAD=$4 NP=$5; shift 5
    rig_start "fzr-$$"
    rig_shape "$S" "$W" "$P" >/dev/null 2>&1
    rig_states 4 >/dev/null 2>&1
    rig_attach >/dev/null 2>&1
    rig_load "$LOAD" "$NP" >/dev/null 2>&1
    local panes vis V r
    panes=$(srv list-panes -a -F x | wc -l)
    vis=$(srv list-panes -t s1 -F x | wc -l)
    for V in "$@"; do
        reset_variant; apply_variant "$V"
        srv refresh-client -S 2>/dev/null; sleep 0.5
        r=$(sample)
        printf '%-4s %-26s %-8s %-5s %-4s %-6s %-3s %s\n' \
            "$V" "$(variant_desc "$V")" "${S}x${W}x${P}" "$panes" "$vis" \
            "$LOAD" "$NP" "$r"
    done
    rig_cleanup
}

hdr() { printf '\n%-4s %-26s %-8s %-5s %-4s %-6s %-3s %-5s %-5s %-5s %-4s %s\n' \
        VAR ELEMENT SxWxP PANES VIS LOAD 'N' p50 p95 max '>1s' 'cpu%'; }

ALL='V0 V1 V2 V3 V4 V5 V6 V7'

case "${1:-matrix}" in
matrix) hdr
    for shape in "1 6 2" "3 10 4" "5 20 4"; do
        for load in none spam agent; do sweep $shape "$load" 4 $ALL; done
    done ;;
one)    shift; hdr; S=$1 W=$2 P=$3 L=$4 N=$5; shift 5
        sweep "$S" "$W" "$P" "$L" "$N" "${@:-$ALL}" ;;
scale)  hdr; V=${2:-V3}
    for shape in "1 4 4" "1 12 4" "2 12 4" "3 15 4" "4 20 4" "5 20 4"; do
        sweep $shape agent 4 V0 "$V" V7
    done ;;
visible) hdr; V=${2:-V3}
    for shape in "3 40 2" "3 20 4" "3 13 6" "3 10 8" "3 5 16"; do
        sweep $shape agent 4 V0 "$V" V7
    done ;;
rate)   hdr; V=${2:-V7}
    for n in 1 2 4 8; do
        OSC_N=$n sweep 3 10 4 agent 4 V0 "$V"
    done ;;
esac
