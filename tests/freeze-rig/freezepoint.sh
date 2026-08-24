#!/usr/bin/env bash
# How hard do you have to push before a tmux command round-trip crosses 1s?
# Holds the server at 400 panes and raises the OSC-2 title pressure: more
# agent panes, more titles per cycle. Control vs full shipped config.
set -u
source "$(dirname "$0")/lib.sh"

DUR=${DUR:-12}
SAMPLE_MS=${SAMPLE_MS:-150}

sample() {
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

printf '\n%-4s %-6s %-7s %-6s %-6s %-6s %-5s %s\n' \
    VAR PANES 'AGENTS' 'OSC/s' p50 p95 '>1s' 'cpu%'
for np in 12 30; do
  for osc in 4; do
    rig_start "fzfp-$$"
    rig_shape 5 20 4 >/dev/null 2>&1
    rig_states 4 >/dev/null 2>&1
    rig_attach >/dev/null 2>&1
    OSC_N=$osc rig_load agent "$np" >/dev/null 2>&1
    panes=$(srv list-panes -a -F x | wc -l)
    rate=$(( osc * 20 * np ))          # 20 cycles/s per pane at DELAY=0.05
    for V in V0 V7; do
        reset_variant; apply_variant "$V"
        srv refresh-client -S 2>/dev/null; sleep 0.5
        r=$(sample)
        set -- $r
        printf '%-4s %-6s %-7s %-6s %-6s %-6s %-5s %s\n' \
            "$V" "$panes" "$np" "$rate" "$1" "$2" "$4" "$5"
    done
    rig_cleanup
  done
done
