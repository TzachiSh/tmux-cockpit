#!/usr/bin/env bash
# tmux-cockpit redraw-cost gate -- drop-in for tests/run.sh.
#
#   bash gate.sh [path/to/cockpit-formats.conf]
#
# Asserts ONE rule:
#
#     a single status-line redraw may walk the server's panes at most ONCE
#
# i.e. total pane visits per status redraw <= total panes on the server.
#
# WHY THIS RULE. tmux re-expands pane-border-format for every visible pane on
# every status redraw (screen-redraw.c: CLIENT_REDRAWSTATUS implies
# REDRAW_PANE_STATUS), and a status redraw is forced by every OSC-2 title an
# agent emits (input.c calls server_status_window(); screen_set_title() never
# compares against the previous title, so even an unchanged title costs full
# price). Each #{S:#{W:#{P:}}} walk inside those formats does a
# format_create + format_defaults + format_free per pane visited. So "pane
# visits per status redraw" is precisely the quantity the freeze is made of.
#
# WHY A COUNT AND NOT A TIMING. This is machine-independent and load-
# independent: identical on a laptop and a loaded CI box, no flaky p95. It
# comes from tmux's own -vv log, which prints one "pane loop: %N" line per
# pane visited by a #{P:} walk.
#
# Exit 0 = PASS, 1 = FAIL, 2 = could not measure. Zero pane visits is a PASS
# (formats with no #{P:} left), not a failed measurement. See the arm below.
set -u

CONF=${1:-}
HERE=$(cd -P "$(dirname "$0")" && pwd)
# Sourced FIRST: lib.sh assigns SOCK="" at top level, so sourcing it after we
# pick a socket would silently redirect every command at the default server.
# shellcheck disable=SC1091
. "$HERE/lib.sh"

SOCK="cockpit-gate-$$"
LOGDIR=$(mktemp -d) || exit 2
# 3 sessions x 10 windows x 4 panes = 120 panes: big enough that a whole-server
# walk is unmistakable, small enough to build in a few seconds.
S=${S:-3} W=${W:-10} P=${P:-4}
# REPS takes the minimum over reps, so more reps only catches undercounts --
# it does not make the number less reliable. Measured identical visit counts
# (696/706/712) at REPS=3 SETTLE=0.5 across nine runs, 69.6s -> 30.0s; the one
# anomaly seen anywhere in that sweep was at the OLD defaults (5/1.0), not a
# reduced one. docs/superpowers/research/2026-08-25-suite-speed/suite-speed-report.md §4.
REPS=${REPS:-3}
SETTLE=${SETTLE:-0.5}     # seconds to let a forced redraw finish before counting
BUDGET_RATIO=${BUDGET_RATIO:-1.0}       # server walks allowed per status redraw

cleanup() {
    tmux -L "$SOCK" kill-server 2>/dev/null
    for p in $(pgrep -f "tmux -L $SOCK" 2>/dev/null); do
        [ "$p" = "$$" ] || kill -9 "$p" 2>/dev/null
    done
    [ -n "${CLIENT_PID:-}" ] && kill -9 "$CLIENT_PID" 2>/dev/null
    # kill-server does not always unlink the socket, and a test suite must not
    # litter /tmp/tmux-$UID with a file per run.
    rm -f "/tmp/tmux-$(id -u)/$SOCK" 2>/dev/null
    rm -rf "$LOGDIR"
    return 0
}
trap cleanup EXIT
srv() { tmux -L "$SOCK" "$@"; }

# status-interval 0: the forced redraw must be the ONLY redraw in the window.
( cd "$LOGDIR" && tmux -L "$SOCK" -vv -f /dev/null \
    new-session -d -s s1 -x 200 -y 62 \; set -g status-interval 0 ) || exit 2

# The formats under test. Default to the plugin's own bar (V7, statusbar.conf).
if [ -n "$CONF" ]; then
    srv source-file "$CONF" 2>/dev/null || { echo "gate: cannot source $CONF"; exit 2; }
else
    srv set -g mouse on
    srv set -g @cockpit_agents_glob "$GLOB"
    srv set -g @thm_needs '#ff5f6d'; srv set -g @thm_error '#ff9e3d'
    srv set -g @thm_running '#e6c84f'; srv set -g @thm_done '#5fd38d'
    srv set -g @thm_dim '#9097a3'
    apply_variant V7
fi

# Build the shape. Every pane is a fake agent so the agent glob matches.
AGENT="bash --norc -c 'exec -a claude bash --norc $HERE/loop.sh idle'"
for ((s=1; s<=S; s++)); do
    [ "$s" = 1 ] || srv new-session -d -s "s$s" -x 200 -y 62
    for ((w=1; w<=W; w++)); do [ "$w" = 1 ] || srv new-window -d -t "s$s:"; done
    for ((w=0; w<W; w++)); do
        for ((p=1; p<P; p++)); do srv split-window -d -t "s$s:$w" -l 3 2>/dev/null || true; done
    done
done
while read -r pid; do srv respawn-pane -k -t "$pid" "$AGENT" 2>/dev/null; done \
    < <(srv list-panes -a -F '#{pane_id}')
sleep 1

# A few blocked panes in non-active windows: the branch the banner renders.
total=$(srv list-panes -a -F x | wc -l)
[ "$total" -ge $((S * W * P / 2)) ] || {
    echo "gate: only built $total panes, expected $((S * W * P)) -- not measuring"; exit 2; }
j=0
while read -r pid; do
    if [ "$j" -ge $((total - 4)) ]; then srv set-option -p -t "$pid" @panestate needs-input
    else srv set-option -p -t "$pid" @panestate running; fi
    j=$((j+1))
done < <(srv list-panes -a -F '#{pane_id}')

script -qfc "tmux -L $SOCK attach -t s1" /dev/null >/dev/null 2>&1 &
CLIENT_PID=$!
disown 2>/dev/null
sleep 1.5

LF=$(ls "$LOGDIR"/tmux-server-*.log 2>/dev/null | head -1)
[ -n "$LF" ] || { echo "gate: no tmux server log (need a tmux built with logging)"; exit 2; }

min=""
for ((i=0; i<REPS; i++)); do
    # FULL refresh, not -S. A -S redraw is skipped entirely when the status
    # line content is unchanged (screen-redraw.c: `if (!redraw &&
    # !REDRAW_IS_ALL(flags))` strips REDRAW_STATUS), so -S counts flip between
    # the full cost and near zero. REDRAW_ALL bypasses that guard and always
    # runs the pane-status loop, which is what makes this reproducible.
    srv refresh-client 2>/dev/null; sleep "$SETTLE"
    # Count by DIFFERENCE, never by truncating the log: tmux holds the file
    # open and writes to it continuously, so `: > $LF` races its writes and
    # yields partial counts on a busy machine.
    before=$(grep -c 'pane loop: %' "$LF" 2>/dev/null); before=${before:-0}
    srv refresh-client 2>/dev/null
    sleep "$SETTLE"
    after=$(grep -c 'pane loop: %' "$LF" 2>/dev/null); after=${after:-0}
    n=$((after - before))
    if [ -z "$min" ] || [ "$n" -lt "$min" ]; then min=$n; fi
done

# A MOVE -- window to window across projects, agent window to a plain terminal
# window, pane to pane -- is a status redraw the owner triggers by hand all
# day. Same rule, same ratio: at most one server walk per move.
#
# s1:1 is all alpha and s1:2 all bravo, and project->project moves between
# exactly those two, so the label is true. It is scene-setting, not a variable
# the number depends on: the formats key on #{pane_current_command} and
# @panestate, never on the pane's path -- what a move costs is pane COUNT.
# The last window is a plain terminal, no agent, which is the other move the
# owner makes all day.
srv new-window -d -t s1: -n term -c "$LOGDIR" 'bash --norc' 2>/dev/null
mkdir -p "$LOGDIR/alpha" "$LOGDIR/bravo"
while read -r pid; do srv respawn-pane -k -c "$LOGDIR/alpha" -t "$pid" "$AGENT" 2>/dev/null
done < <(srv list-panes -t s1:1 -F '#{pane_id}')
while read -r pid; do srv respawn-pane -k -c "$LOGDIR/bravo" -t "$pid" "$AGENT" 2>/dev/null
done < <(srv list-panes -t s1:2 -F '#{pane_id}')
sleep 1

# The term window is a 121st pane, so a move is measured on a BIGGER server
# than $min was. Judging a 121-pane number against a 120-pane budget is a
# gate that can never go green: count the server the moves actually run on and
# give them their own budget at the same ratio. $min, $total and the historic
# 696 stay exactly comparable with every earlier row in RESULTS.txt.
mtotal=$(srv list-panes -a -F x | wc -l)
mbudget=$(awk -v p="$mtotal" -v r="$BUDGET_RATIO" 'BEGIN{printf "%d", p*r}')

move_cost() {                 # <from-window> <tmux move command...>  -> min pane visits over REPS
    local from=$1; shift
    local i before after n m=""
    for ((i=0; i<REPS; i++)); do
        # The reset names the PANE too, not just the window: select-window
        # alone leaves the active pane where the previous rep's select-pane
        # put it, so every rep after the first re-selected the pane already
        # selected -- a no-op tmux answers with zero visits. The reset is
        # outside the counted interval (the sleep below it), so its own cost
        # is not in the number. It also names where the move STARTS, which is
        # what makes "project->project" an alpha window leaving for a bravo one.
        srv select-window -t "$from" 2>/dev/null; srv select-pane -t "$from.0" 2>/dev/null; sleep "$SETTLE"
        before=$(grep -c 'pane loop: %' "$LF" 2>/dev/null); before=${before:-0}
        srv "$@" 2>/dev/null; sleep "$SETTLE"
        after=$(grep -c 'pane loop: %' "$LF" 2>/dev/null); after=${after:-0}
        n=$((after - before)); if [ -z "$m" ] || [ "$n" -lt "$m" ]; then m=$n; fi
    done
    printf '%s' "$m"
}
mv_proj=$(move_cost s1:1 select-window -t s1:2)      # alpha window -> bravo window
mv_term=$(move_cost s1:0 select-window -t s1:term)   # agent window -> terminal window
mv_pane=$(move_cost s1:0 select-pane -t s1:0.1)      # pane -> pane inside a window
# The yardstick: the same forced redraw, on the same server, same reps. It is
# what says whether a move costs a redraw or more than one.
mv_ctl=$(move_cost s1:0 refresh-client)

# A 0 is never "free" -- it means the target never resolved (a renamed window,
# a pane id that moved) and nothing was measured, which must not read as a
# pass. The guard lives out here, not before move_cost's printf: an `exit`
# inside `$( )` ends the SUBSHELL and the gate would carry on with the message
# as the number.
#
# Unless the redraw walked nothing either. Formats with no #{P:} at all, which
# examples/tmux.conf has had since #108, cost zero by construction: no walk is
# left for a move to fail to resolve into, so exit 2 was wrong (#125). Zero is
# still earned, by a #{P:} walk planted in window-status-format and measured
# the same way on the same server, last of all.
if [ "$min" -eq 0 ]; then
    srv set -g window-status-format '#{P:#{pane_id}}' 2>/dev/null
    probe=$(move_cost s1:0 refresh-client)
    [ "${probe:-0}" -gt 0 ] || {
        echo "gate: a planted #{P:} walk measured 0 pane visits too -- nothing was measured"
        exit 2; }
    echo "gate: no #{P:} walk in these formats (a planted one on this server costs $probe)"
else
    for nv in "project->project:$mv_proj" "agent->terminal:$mv_term" \
              "pane->pane:$mv_pane" "redraw control:$mv_ctl"; do
        [ "${nv##*:}" -gt 0 ] || {
            echo "gate: ${nv%%:*} measured 0 pane visits -- its target did not resolve"; exit 2; }
    done
fi

budget=$(awk -v p="$total" -v r="$BUDGET_RATIO" 'BEGIN{printf "%d", p*r}')
printf 'pane visits per status redraw: %s   (%s panes on the server, budget %s = %sx a full walk)\n' \
    "$min" "$total" "$budget" "$BUDGET_RATIO"
printf 'pane visits per move: project→project %s   agent→terminal window %s   pane→pane %s   (a plain redraw on the same server: %s)\n' \
    "$mv_proj" "$mv_term" "$mv_pane" "$mv_ctl"
printf '   (%s panes on the server once the terminal window exists, budget %s = %sx a full walk)\n' \
    "$mtotal" "$mbudget" "$BUDGET_RATIO"
mworst=$mv_proj
for n in "$mv_term" "$mv_pane"; do [ "$n" -gt "$mworst" ] && mworst=$n; done
if [ "$min" -le "$budget" ] && [ "$mworst" -le "$mbudget" ]; then echo "PASS"; exit 0; fi
which=""
[ "$min" -gt "$budget" ] && which="redraw"
[ "$mworst" -gt "$mbudget" ] && which="${which:+$which and }move"
awk -v w="$which" -v r="$min" -v rp="$total" -v m="$mworst" -v mp="$mtotal" 'BEGIN{
    printf "FAIL (%s over budget): redraw %.1f full server walks on %d panes, worst move %.1f on %d panes\n", \
        w, r/rp, rp, m/mp, mp }'
exit 1
