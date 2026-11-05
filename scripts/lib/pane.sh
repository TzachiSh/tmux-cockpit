#!/usr/bin/env bash
# Sourced, never run. It runs before every popup the wrappers open, so it forks
# nothing as it loads, and is_pane is builtins only.

# %<digits> only: the value reaches a tmux target, a command string or a path.
is_pane() { case "${1-}" in %*) case "${1#%}" in ''|*[!0-9]*) return 1 ;; esac ;; *) return 1 ;; esac; }

# Prints a pane option and unsets it, in one tmux call. The popup keys pin the
# pressing client on the pane this way (#296, #360); taken once, a later press
# from another client sets a pin of its own instead of this run's (#480).
# 1: <pane> is not %<digits>, and no tmux runs. 2: <option> is not @<name>.
take_pane_opt() {             # <pane> <option>
    is_pane "${1-}" || return 1
    case ${2-} in
        @?*) ;;
        *) printf "take_pane_opt: <option> must be @<name>, got '%s'\n" "${2-}" >&2; return 2 ;;
    esac
    tmux show-option -pqv -t "$1" "$2" \; set-option -pu -t "$1" "$2" 2>/dev/null
}
