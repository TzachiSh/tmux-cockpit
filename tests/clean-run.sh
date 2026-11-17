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
# The exit trap removes both dirs and the list, on every path. HUP, INT and TERM are trapped
# too, and both traps ignore the three first, as par.sh's do (#1127): dash runs no EXIT trap
# when a signal it does not trap ends the script, and a second signal during the removal would
# cut it short (#1172). A trap runs once the command in the foreground is done, so a signal to
# this script alone waits for the command, and never removes its dirs under it.
t='' x='' l=''
cr_clean() {
    trap '' HUP INT TERM
    # a test that made a dir unreadable must not keep the rm from taking it
    for d in "$t" "$x"; do
        [ -z "$d" ] || { chmod -R u+rwx "$d" 2>/dev/null; rm -rf -- "$d"; }
    done
    [ -z "$l" ] || rm -f -- "$l"
}
cr_sig() { trap '' HUP INT TERM; exit $((128 + $1)); }
trap cr_clean EXIT
trap 'cr_sig 1' HUP; trap 'cr_sig 2' INT; trap 'cr_sig 15' TERM
t=$(mktemp -d) || exit 1
x=$(mktemp -d) || exit 1
l=$(mktemp) || exit 1
TMPDIR=$t TMUX_TMPDIR=$x COCKPIT_TEST_CLEAN_RUN_CT_LIST=$l "$@"
rc=$?
# only this run's own folders, by its list: another gate's are never named
ct=$(while IFS= read -r f; do [ ! -e "$f" ] || echo "$f"; done < "$l")
rmdir "$x/tmux-$(id -u)" 2>/dev/null
# a test that made a dir unreadable must not hide it from the scan
chmod -R u+rwx "$t" "$x" 2>/dev/null
# files and empty dirs: a dir that holds something is named by what it holds
leaves() { (cd "$1" && find . -mindepth 1 \( ! -type d -o -empty \) | sed "s|^\\.|$2|"); }
left=$( (leaves "$t" TMPDIR; leaves "$x" TMUX_TMPDIR; [ -z "$ct" ] || echo "$ct") | sort | tr '\n' ' ')
[ -z "$left" ] || printf 'clean-run: left behind: %s\n' "${left% }" >&2
[ "$rc" -ne 0 ] && exit "$rc"
[ -z "$left" ]
