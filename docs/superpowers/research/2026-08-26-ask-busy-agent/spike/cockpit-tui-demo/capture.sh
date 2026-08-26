#!/usr/bin/env bash
# Run a screen in a pane of size WxH on a third private server and capture it.
set -uo pipefail
D=$(cd "$(dirname "$0")" && pwd)
W=$1 H=$2; shift 2
S=cap$$
export COCKPIT_TMUX=${COCKPIT_TMUX:-tmux -L demo -f /dev/null}
export COCKPIT_LOG=$D/tui.log
tmux -L capture -f /dev/null new-session -d -s "$S" -x "$W" -y "$H" -c "$D" \
  -e "COCKPIT_TMUX=$COCKPIT_TMUX" -e "COCKPIT_LOG=$COCKPIT_LOG" \
  "$D/target/release/cockpit-tui $*; sleep 60"
sleep 3
tmux -L capture -f /dev/null capture-pane -p -t "$S"
tmux -L capture -f /dev/null kill-session -t "$S" 2>/dev/null
