#!/usr/bin/env bash
# The one part of the shipped config that spawns PROCESSES on redraw:
# status-right's #{agent_where_path} / #{agent_where_branch}, which
# cockpit.tmux rewrites into two #() calls to scripts/tmux-where.
#
# Reports round-trip latency, tmux server CPU, and the system-wide process
# creation rate (from /proc/stat's "processes" counter) so a fork storm shows
# up even though the forks are not charged to the tmux server's own CPU.
set -u
source "$(dirname "$0")/lib.sh"

DUR=${DUR:-12}

forks() { awk '/^processes /{print $2}' /proc/stat; }

sample() {
    local spid; spid=$(server_pid)
    local t_end=$(( $(date +%s%N) + DUR*1000000000 ))
    local c0 c1 f0 f1 w0 w1 vals=() t0 t1
    c0=$(cpu_ticks "$spid"); f0=$(forks); w0=$(date +%s%N)
    while [ "$(date +%s%N)" -lt "$t_end" ]; do
        t0=$(date +%s%N)
        timeout 30 tmux -L "$SOCK" display-message -p ok >/dev/null 2>&1
        t1=$(date +%s%N)
        vals+=( $(( (t1-t0)/1000000 )) )
        read -r -t 0.150 -u 9 x 2>/dev/null
    done 9<> <(:)
    c1=$(cpu_ticks "$spid"); f1=$(forks); w1=$(date +%s%N)
    printf '%s\n' "${vals[@]}" | sort -n | awk \
        -v hz="$(getconf CLK_TCK)" -v dc="$((c1-c0))" \
        -v df="$((f1-f0))" -v dw="$(( (w1-w0)/1000000 ))" '
        {v[NR]=$1} END{
          p50=v[int(NR*0.5)?int(NR*0.5):1]; p95=v[int(NR*0.95)?int(NR*0.95):1];
          printf "%s %s %s %.0f %.0f", p50, p95, v[NR], (dc/hz)*100000/dw, df*1000/dw }'
}

printf '\n%-4s %-30s %-6s %-6s %-6s %-6s %s\n' \
    VAR ELEMENT p50 p95 max 'cpu%' 'forks/s(system)'
for V in V0 V8 V7 V9; do
    rig_start "fzst-$$"
    rig_shape 3 10 4 >/dev/null 2>&1
    rig_states 4 >/dev/null 2>&1
    rig_attach >/dev/null 2>&1
    OSC_N=1 rig_load agent 4 >/dev/null 2>&1
    reset_variant; apply_variant "$V"
    srv refresh-client -S 2>/dev/null; sleep 0.5
    r=$(sample); set -- $r
    printf '%-4s %-30s %-6s %-6s %-6s %-6s %s\n' \
        "$V" "$(variant_desc "$V")" "$1" "$2" "$3" "$4" "$5"
    rig_cleanup
done
