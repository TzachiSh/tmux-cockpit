#!/usr/bin/env bash
# Sourced, never run. The required cockpit-tui (#199): where it is, how old it
# may be. Builtins only, like vnum.sh: install.sh's fail rows use it.

TUI_MIN=0.2.0
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
