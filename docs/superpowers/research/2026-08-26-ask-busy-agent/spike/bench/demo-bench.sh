#!/usr/bin/env bash
# A private tmux server with three fake agents, so nothing here can touch the
# live one. Every tmux call names -L demo explicitly.
set -euo pipefail
D=$(cd "$(dirname "$0")" && pwd)
BIN=$SCRATCH/demo-target/release/cockpit-tui
T=(tmux -L bench -f /dev/null)

"${T[@]}" kill-server 2>/dev/null || true
# real repo paths, so loc and ± are real git output rather than a fixture
R1=/home/tzachi/.tmux/plugins/tmux-cockpit
R2=/home/tzachi/.cache/cockpit-wt/issue-54-research
"${T[@]}" new-session -d -s demo -x 200 -y 50 -n openvpn -c "$R1" "printf 'sudo openvpn --config /etc/openvpn/work.conf\n\n> Do you want to proceed?\n  1. Yes\n  2. No\n'; sleep 100000"
"${T[@]}" new-window  -t demo -n build  -c "$R2" "printf '\033[32m$ make check\033[0m\n1001 assertions, 0 failures\nshellcheck cockpit.tmux ... ok\n'; sleep 100000"
"${T[@]}" new-window  -t demo -n review -c "$R1" "printf '\033[33m● reviewing\033[0m branch fleet-timing\n 12 files, +12 -4\n'; sleep 100000"
"${T[@]}" new-window  -t demo -n idle   -c "$R1" "printf 'nothing to do\n'; sleep 100000"

set_pane() { # <window> <state> <title> <msg> <review>
  local p; p=$("${T[@]}" list-panes -t "demo:$1" -F '#{pane_id}')
  "${T[@]}" set-option -p -t "$p" @panestate "$2"
  "${T[@]}" set-option -p -t "$p" @cockpit_title "$3"
  "${T[@]}" set-option -p -t "$p" @panestate_msg "$4"
  "${T[@]}" set-option -p -t "$p" @cockpit_review "$5"
  echo "$p"
}
P1=$(set_pane openvpn needs-input "Openvpn setup" "permission: Bash(sudo openvpn --config /etc/openvpn/work.conf --daemon --log /var/log/ovpn.log)" "")
P2=$(set_pane build   running     "Implement with test review and rig" "" pending)
P3=$(set_pane review  running     "Fleet gate review" "" "")
P4=$(set_pane idle    done        "Left click issue" "" sent)
echo "panes: $P1 $P2 $P3 $P4"

# the bindings a real install would ship; the wrapper runs the action the TUI prints
"${T[@]}" set-environment -g COCKPIT_TMUX "tmux -L bench -f /dev/null"
"${T[@]}" bind-key F display-popup -E -w 90% -h 80% "$D/wrap.sh fleet"
"${T[@]}" bind-key B display-popup -E -w 90% -h 80% "$D/wrap.sh board"
"${T[@]}" bind-key I display-popup -E -w 84  -h 20  "$D/wrap.sh info #{pane_id}"
"${T[@]}" bind-key '?' display-popup -E -w 84 -h 14 "$D/wrap.sh ask #{pane_id}"
echo "bound F/B/I/? on the demo server"
