#!/usr/bin/env bash
# Sourced, never run. The required cockpit-tui (#199): where it is, how old it
# may be. Builtins only, like vnum.sh: install.sh's fail rows use it.

TUI_MIN=0.4.0
# shellcheck disable=SC2034  # read by tui-fetch: the release it installs is the floor
TUI_TAG=cockpit-tui-v$TUI_MIN
COCKPIT_TUI_BIN=${COCKPIT_TUI_BIN:-$(cd -P "${BASH_SOURCE[0]%/*}/../.." && pwd)/bin/cockpit-tui}
# shellcheck source=vnum.sh
. "${BASH_SOURCE[0]%/*}/vnum.sh" || return 1

# Prints the version it found. 1: missing or will not run; 2: below TUI_MIN.
tui_check() {
    local v
    v=$("$COCKPIT_TUI_BIN" --version 2>/dev/null); v=${v##* }; v=${v#v}
    [ -n "$v" ] || return 1
    printf '%s\n' "$v"
    [ "$(vnum "$v")" -ge "$(vnum "$TUI_MIN")" ] || return 2
}

# A cockpit-tui older than the plugin exits 2 and says "unknown screen <name>"
# on stderr (tui/src/main.rs), or "pick: unknown argument <flag>" when the screen
# is old but the flag is new (tui/src/pick.rs). Exit 2 alone is not enough: pick also exits 2
# when stdout is gone. Wrappers capture stderr to a file, replay it, then ask here.
# shellcheck disable=SC2034  # read by tm, tmux-branch and tmux-open-path
TUI_STALE_MSG="cockpit-tui is older than the plugin: run make tui, or scripts/tui-fetch (prefix U does it only with @cockpit_tui_fetch on)"
# tui_stale RC ERRFILE: 0 when RC is 2 and ERRFILE has the unknown-screen line.
tui_stale() {
    local l
    [ "${1:-}" = 2 ] && [ -r "${2:-}" ] || return 1
    while IFS= read -r l || [ -n "$l" ]; do
        case $l in
            "cockpit-tui: unknown screen "*|"cockpit-tui: pick: unknown argument "*) return 0 ;;
        esac
    done < "$2"
    return 1
}
