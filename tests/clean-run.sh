#!/bin/sh
# Runs a command, by default `make -s tui-test`, with TMPDIR and TMUX_TMPDIR
# on private empty dirs, then fails naming whatever it left behind (#400).
# One thing may stay: the per-uid dir tmux makes in TMUX_TMPDIR, while it is
# empty; a socket in it is a server a test did not kill. A failing command
# keeps its own exit code. Both dirs are removed either way. The cargo runner
# makes each test binary's tmux folder in /tmp and adds its path to the list
# this hands it: one still there after the run is named too (#1066).
#   make tui-test-clean        tests/clean-run.sh [command...]
[ "$#" -gt 0 ] || set -- make -s tui-test
t=$(mktemp -d) || exit 1
x=$(mktemp -d) || { rmdir "$t"; exit 1; }
l=$(mktemp) || { rmdir "$t" "$x"; exit 1; }
TMPDIR=$t TMUX_TMPDIR=$x COCKPIT_TEST_CLEAN_RUN_CT_LIST=$l "$@"
rc=$?
# only this run's own folders, by its list: another gate's are never named
ct=$(while IFS= read -r f; do [ ! -e "$f" ] || echo "$f"; done < "$l")
rm -f "$l"
rmdir "$x/tmux-$(id -u)" 2>/dev/null
# a test that made a dir unreadable must not hide it from the scan or the rm
chmod -R u+rwx "$t" "$x" 2>/dev/null
# files and empty dirs: a dir that holds something is named by what it holds
leaves() { (cd "$1" && find . -mindepth 1 \( ! -type d -o -empty \) | sed "s|^\\.|$2|"); }
left=$( (leaves "$t" TMPDIR; leaves "$x" TMUX_TMPDIR; [ -z "$ct" ] || echo "$ct") | sort | tr '\n' ' ')
rm -rf "$t" "$x"
[ -z "$left" ] || printf 'clean-run: left behind: %s\n' "${left% }" >&2
[ "$rc" -ne 0 ] && exit "$rc"
[ -z "$left" ]
