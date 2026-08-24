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
# Exit 0 = PASS, 1 = FAIL, 2 = could not measure.
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
REPS=${REPS:-5}
SETTLE=${SETTLE:-1.0}     # seconds to let a forced redraw finish before counting
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

# The formats under test. Default to the repo's shipped example config.
if [ -n "$CONF" ]; then
    srv source-file "$CONF" 2>/dev/null || { echo "gate: cannot source $CONF"; exit 2; }
else
    srv set -g mouse on
    srv set -g @cockpit_agents_glob "$GLOB"
    srv set -g @thm_red '#ff6b6b'; srv set -g @thm_peach '#ff9838'
    srv set -g @thm_yellow '#ffcc33'; srv set -g @thm_green '#3dea7d'
    srv set -g @thm_overlay_0 '#5b6272'; srv set -g @cockpit_blocked_bg '#52303a'
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

budget=$(awk -v p="$total" -v r="$BUDGET_RATIO" 'BEGIN{printf "%d", p*r}')
printf 'pane visits per status redraw: %s   (%s panes on the server, budget %s = %sx a full walk)\n' \
    "$min" "$total" "$budget" "$BUDGET_RATIO"
if [ "$min" -le "$budget" ]; then echo "PASS"; exit 0; fi
awk -v m="$min" -v p="$total" 'BEGIN{printf "FAIL: %.1f full server walks per redraw\n", m/p}'
exit 1
