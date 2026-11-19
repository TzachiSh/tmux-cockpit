#!/bin/bash
# A test-only second adapter (#1330): a made-up agent whose protocol is one word per line on
# stdin. Each word becomes agent-event v1 lines (docs/events.md) for scripts/cockpit-event, the
# worked example of "Adding a CLI agent". An unknown word writes nothing, and no word ever goes
# into a line.
#   usage: <words> | adapter.sh [path to cockpit-event]
set -u
[ -n "${TMUX_PANE-}" ] || exit 0    # outside tmux there is no pane to report

d=${BASH_SOURCE[0]}; case $d in (*/*) ;; (*) d=./$d ;; esac
[ -L "$d" ] && d=$(n=0
    while [ -L "$d" ] && [ $((n += 1)) -le 40 ] && t=$(readlink "$d"); do
        case $t in (/*) d=$t ;; (*) d=${d%/*}/$t ;; esac; done; printf %s "$d")
core=${1:-"${d%/*}/../../../scripts/cockpit-event"}

# One id for the whole run, from random bytes: 32 lowercase hex, no agent text in it.
sid=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
ev() {                        # <kind> [,"key":value]: one line
    printf '{"schema_version":1,"agent":"fake","kind":"%s","pane":"%s",' "$1" "$TMUX_PANE"
    printf '"session":"%s","at":"%s"%s}\n' "$sid" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "${2:-}"
}
while IFS= read -r w || [ -n "$w" ]; do
    case $w in
        begin) ev turn.start ;;
        tool) ev tool.start; ev tool.end ',"failed":false' ;;
        fail) ev tool.start; ev tool.end ',"failed":true' ;;
        end) ev turn.end ',"outcome":"answer"' ;;
        stop-by-user) ev turn.end ',"outcome":"aborted"' ;;
        quit) ev session.end ;;
        *) ;;
    esac
done | "$core"
