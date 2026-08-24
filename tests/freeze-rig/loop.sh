#!/usr/bin/env bash
# The fake agent that fills every rig pane. Launched as
#   bash -c 'exec -a claude bash --norc loop.sh <mode> [args]'
# so pane_current_command reads "claude" and the agent glob matches, exactly
# like a real Claude Code pane. Sleeps are a read on a dangling fifo, never a
# `sleep` child: a child would change pane_current_command mid-loop and
# silently take the pane out of the measurement.
#
# Modes:
#   idle
#   spam  <burst> <delay>          plain lines only -- NO escape sequences
#   title <n> <delay>              n OSC 2 title sets per cycle, no line output
#   agent <burst> <delay> <n>      both: what a real agent pane emits
#
# The distinction matters more than anything else in this rig. Plain line
# output never re-expands pane-border-format; an OSC 2 title set does, for
# every visible pane, because tmux's input parser calls server_status_window().
set -u
f=$(mktemp -u); mkfifo "$f"; exec 3<>"$f"; rm -f "$f"
LINE='2026-08-24T16:04:11Z INFO  worker[7] processed batch id=8823 rows=1024 elapsed=41ms status=ok'
TITLES=('✳ Refactoring the parser' '✳ Running tests' '✳ Reading src/main.rs' '✳ Thinking')

nap() { [ "${1:-0}" = 0 ] || read -r -t "$1" -u 3 x; }
lines() { local n=$1 i; for ((i=0; i<n; i++)); do printf '%s\n' "$LINE"; done; }
titles() { local n=$1 i; for ((i=0; i<n; i++)); do printf '\033]2;%s\007' "${TITLES[i%4]}"; done; }

case "${1:-idle}" in
  idle)  read -r -u 3 x ;;                       # block forever
  spam)  while :; do lines "${2:-40}";              nap "${3:-0.05}"; done ;;
  title) while :; do titles "${2:-1}";              nap "${3:-0.05}"; done ;;
  agent) while :; do lines "${2:-40}"; titles "${4:-1}"; nap "${3:-0.05}"; done ;;
  # exactly N titles, then go quiet -- lets the rig divide server CPU by a
  # known number of events and get a rate-independent cost per title set
  burst) titles "${2:-500}"; read -r -u 3 x ;;
esac
