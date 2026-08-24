#!/usr/bin/env bash
# tmux-cockpit freeze rig -- entry point.
#
#   bash run.sh regress            PASS/FAIL gate for a candidate fix
#   bash run.sh elements           CPU per redraw, per config element, per shape
#   bash run.sh e2e                round-trip latency + CPU under an agent load
#   bash run.sh scale              scaling law: cost vs total panes
#   bash run.sh rate               cost vs OSC-title rate
#
# WHAT THIS MEASURES, AND WHY IT IS SHAPED THIS WAY
#
# tmux re-expands `pane-border-format` for every visible pane whenever the
# STATUS line is redrawn (screen-redraw.c: CLIENT_REDRAWSTATUS implies
# REDRAW_PANE_STATUS). Plain line output does NOT cause that. An OSC 2 title
# set DOES -- input.c calls server_status_window() on every OSC 0/2/7, and
# screen_set_title() never compares against the previous title, so even an
# unchanged title forces the redraw. Agent TUIs set their title constantly.
#
# So the load generator (loop.sh) has separate `spam` (plain lines) and
# `title` (OSC 2) modes. Measuring with plain lines only shows nothing; that
# is the trap this rig exists to avoid.
#
# Everything runs on a PRIVATE tmux server (socket fz*-$$). The user's server
# is never touched. Every run kills its server and reaps stray clients.
set -u
cd "$(dirname "$0")" || exit 1
D=$(pwd)

case "${1:-regress}" in
elements) exec bash "$D/redraw_cost.sh" elements ;;
scale)    exec bash "$D/redraw_cost.sh" scale ;;
visible)  exec bash "$D/redraw_cost.sh" visible ;;
e2e)      exec bash "$D/freeze.sh" matrix ;;
rate)     exec bash "$D/freeze.sh" rate ;;
regress)  ;;
*)        echo "usage: run.sh {regress|elements|scale|visible|e2e|rate}"; exit 2 ;;
esac

# ---------------------------------------------------------------- regression
# The gate. A medium-but-realistic server (3 sessions x 10 windows x 4 panes =
# 120 panes) with four agent panes emitting titles, which is the shape the
# freeze was reproduced on. Reports CPU per status redraw for the control and
# for the full shipped config; the fix has to bring the second one down.
source "$D/lib.sh"
R=${R:-60}

redraw_cost() {
    local spid c0 c1 i
    spid=$(server_pid)
    srv refresh-client -S 2>/dev/null; sleep 0.3
    c0=$(cpu_ticks "$spid")
    for ((i=0; i<R; i++)); do
        srv refresh-client -S 2>/dev/null
        read -r -t 0.02 -u 9 x 2>/dev/null
    done 9<> <(:)
    sleep 0.3
    c1=$(cpu_ticks "$spid")
    awk -v d="$((c1-c0))" -v hz="$(getconf CLK_TCK)" -v r="$R" \
        'BEGIN{printf "%.1f", (d/hz)*1000/r}'
}

BUDGET=${BUDGET:-6.0}       # ms of server CPU per status redraw, full config

trap 'rig_cleanup' EXIT
rig_start "fzreg-$$"
rig_shape 3 10 4 >/dev/null 2>&1
rig_states 4 >/dev/null 2>&1
rig_attach >/dev/null 2>&1

printf '\n%-28s %s\n' 'CONFIG' 'ms CPU per status redraw (120 panes, 4 visible)'
reset_variant; apply_variant V0; base=$(redraw_cost)
printf '%-28s %s\n' 'control (no cockpit)' "$base"
reset_variant; apply_variant V7; full=$(redraw_cost)
printf '%-28s %s\n' 'full cockpit config' "$full"

awk -v f="$full" -v b="$base" -v budget="$BUDGET" 'BEGIN{
    printf "\ndelta over control: %.1f ms   budget: %.1f ms\n", f-b, budget
    if (f <= budget) { print "PASS"; exit 0 } else { print "FAIL"; exit 1 }
}'
