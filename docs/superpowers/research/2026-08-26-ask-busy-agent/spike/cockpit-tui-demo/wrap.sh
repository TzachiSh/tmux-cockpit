#!/usr/bin/env bash
# tui.md §6: the binary decides, the shell acts. The TUI owns stdout, so the
# action comes back through a file rather than a command substitution.
D=$(cd "$(dirname "$0")" && pwd)
export COCKPIT_TMUX=${COCKPIT_TMUX:-"tmux -L demo -f /dev/null"}
export COCKPIT_LOG=${COCKPIT_LOG:-$D/tui.log}
f=$(mktemp) || exit 1
trap 'rm -f "$f"' EXIT
COCKPIT_ACTION=$f "$D/target/release/cockpit-tui" "$@" || exit $?
read -r verb target < "$f" || exit 0
case "$verb" in
  jump)   $COCKPIT_TMUX switch-client -t "$target" ;;
  review) $COCKPIT_TMUX display-message "would run tmux-review on $target" ;;
  fix)    $COCKPIT_TMUX display-message "would run tmux-fixer on $target" ;;
esac
