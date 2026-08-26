#!/usr/bin/env bash
# Latency from "the state changed" to "the screen shows it", three channels.
# Flips running<->done rather than needs-input: the needs-you overlay redraws
# once a second on its countdown and would pollute every sample after the first.
set -uo pipefail
D=$(cd "$(dirname "$0")" && pwd)
BIN=$D/target/release/cockpit-tui
DEMO="tmux -L demo -f /dev/null"
LOG=$D/bench.log
N=${N:-10}

$DEMO has-session -t demo 2>/dev/null || { echo "run ./demo.sh first"; exit 1; }
rm -f "$LOG"
tmux -L capture -f /dev/null kill-session -t bench 2>/dev/null
tmux -L capture -f /dev/null new-session -d -s bench -x 108 -y 22 -c "$D" \
  -e "COCKPIT_TMUX=$DEMO" -e "COCKPIT_LOG=$LOG" "$BIN fleet --sock; sleep 300"
sleep 2
PID=$($DEMO show-option -gqv @cockpit_tui_pid)
SOCK=$($DEMO show-option -gqv @cockpit_tui_sock)
echo "listener pid=$PID sock=$SOCK mode=$(stat -c %a "$SOCK" 2>/dev/null)"

sample() {                       # <mode>
  local mode=$1 i state
  for i in $(seq "$N"); do
    [ $((i % 2)) -eq 0 ] && state=running || state=done
    $DEMO set-option -p -t %2 @panestate "$state"
    printf '%s flip %s\n' "$(date +%s%6N)" "$mode" >> "$LOG"
    case $mode in
      sock) COCKPIT_TMUX="$DEMO" "$BIN" notify "state %2 $state" ;;
      usr1) kill -USR1 "$PID" ;;
      poll) : ;;
    esac
    sleep 1.6
  done
}

for m in sock usr1 poll; do sample "$m"; done
tmux -L capture -f /dev/null kill-session -t bench 2>/dev/null

# mawk has no 2-D arrays; one pass per mode is shorter than working around it
printf '%-6s %5s %8s %8s %8s\n' mode n "min ms" "med ms" "max ms"
for m in sock usr1 poll; do
  v=$(awk -v M="$m" '$2=="flip"{mode=$3;t=$1;want=1;next} want&&$2=="draw"{if(mode==M) printf "%.1f\n",($1-t)/1000; want=0}' "$LOG" | sort -n)
  c=$(printf '%s\n' "$v" | grep -c .)
  printf '%-6s %5d %8s %8s %8s\n' "$m" "$c" \
    "$(printf '%s\n' "$v" | head -1)" \
    "$(printf '%s\n' "$v" | sed -n "$(( (c+1)/2 ))p")" \
    "$(printf '%s\n' "$v" | tail -1)"
done
