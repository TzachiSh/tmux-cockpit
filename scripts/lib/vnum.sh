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

# Is version <have> at least <floor>? The tmux floor's one grade (#545): a build
# from master says next-X.Y, X.Y being the release it leads to, so it is graded
# as X.Y. A word with no number left (master, openbsd-7.5, empty) passes: no
# floor can grade it, and refusing to load over it would lock a user out.
vnum_ge() {                   # vnum_ge <have> <floor>
    case "${2-}" in [0-9]*) ;; *)
        printf "vnum_ge: <floor> is not a version: '%s'\n" "${2-}" >&2; return 2 ;;
    esac
    local v=${1-}; v=${v#next-}
    case "$v" in [0-9]*) ;; *) return 0 ;; esac
    [ "$(vnum "$v")" -ge "$(vnum "$2")" ]
}
