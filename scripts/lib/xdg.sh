#!/usr/bin/env bash
# state_home: the state folder in REPLY, by the rule tui/src/store.rs's xdg_dir holds (#1508,
# #1588). XDG_STATE_HOME when it is an absolute path, else $HOME/.local/state when HOME is one.
# A relative or empty value is ignored, as the XDG spec says. With neither there is no state
# folder: 1, and each caller says NO_STATE_HOME. Sourced, never run. Builtins only, bash 3.2 safe.
# shellcheck disable=SC2034  # each caller says it
NO_STATE_HOME='no state folder: neither XDG_STATE_HOME nor HOME is an absolute path'
state_home() {                # -> REPLY
    REPLY=
    case ${XDG_STATE_HOME:-} in
        /*) REPLY=$XDG_STATE_HOME ;;
        *) case ${HOME:-} in /*) REPLY=$HOME/.local/state ;; *) return 1 ;; esac ;;
    esac
}
