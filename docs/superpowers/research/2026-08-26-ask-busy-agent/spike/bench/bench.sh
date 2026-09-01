#!/usr/bin/env bash
# bench.sh <label> <launch-cmd> <channel: usr1|winch|poll> [n]
# Flips @panestate on pane %1 of the -L bench server, fires the channel, and polls capture-pane
# every 5 ms until the row for %1 shows the new glyph. Prints min/median/max in ms.
S=$SCRATCH
label=$1 cmd=$2 chan=$3 n=${4:-10}; B=$S/demo-target/release/cockpit-tui; T="tmux -L bench -f /dev/null"
export COCKPIT_TMUX="tmux -L bench -f /dev/null"
tmux -L cap -f /dev/null kill-server 2>/dev/null
tmux -L cap -f /dev/null new-session -d -s v -x 120 -y 30 -e "COCKPIT_TMUX=$COCKPIT_TMUX" -e "COCKPIT_ACTION=/dev/null" -e "TERM=tmux-256color" -e "FZF_POLL=${FZF_POLL:-0}" "$cmd; sleep 60"
sleep 3
state=$($T show-option -pqv -t %1 @panestate); [ "$state" = running ] && glyph='●' || glyph='✓'
samples=()
for i in $(seq 1 $n); do
  if [ "$state" = running ]; then new=done; want='✓'; else new=running; want='●'; fi
  t0=$(date +%s%N)
  $T set-option -p -t %1 @panestate $new
  case $chan in
    usr1)      kill -USR1 "$($T show-option -gqv @cockpit_tui_pid)" ;;
    poll)      : ;;
  esac
  for j in $(seq 1 600); do
    if tmux -L cap capture-pane -p -t v | grep -q "$want.*Implement with test"; then break; fi
    sleep 0.005
  done
  t1=$(date +%s%N); samples+=($(( (t1 - t0) / 1000000 ))); state=$new
  sleep 0.3
done
sorted=$(printf '%s\n' "${samples[@]}" | sort -n); min=$(echo "$sorted" | head -1); max=$(echo "$sorted" | tail -1); med=$(echo "$sorted" | sed -n "$(( (n + 1) / 2 ))p")
printf '%-14s %-10s n=%d  min %4d  med %4d  max %4d ms   [%s]\n' "$label" "$chan" "$n" "$min" "$med" "$max" "$(echo "${samples[*]}")"
tmux -L cap send-keys -t v q 2>/dev/null; tmux -L cap send-keys -t v Escape 2>/dev/null; sleep 0.3; tmux -L cap kill-server 2>/dev/null
