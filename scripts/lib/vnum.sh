#!/usr/bin/env bash
# Sourced, never run. A builtin only: install.sh's fail rows must still be
# right on a PATH with nothing on it.

# 0.74.3 -> 0074003, 3.7c -> 3007000, "1.85.0 (abc)" -> 1085000.
vnum() {
    local v=${1%% *}; v=${v#v}; local IFS=.
    # shellcheck disable=SC2086  # split on the dots is the point
    set -- $v
    local a=${1:-0} b=${2:-0} c=${3:-0}
    a=${a%%[!0-9]*}; b=${b%%[!0-9]*}; c=${c%%[!0-9]*}
    printf '%d%03d%03d\n' "${a:-0}" "${b:-0}" "${c:-0}"
}
