#!/usr/bin/env bash
# Sourced, never run. Builtins only: it runs before every popup the wrappers open.

# %<digits> only: the value reaches a tmux target, a command string or a path.
is_pane() { case "${1-}" in %*) case "${1#%}" in ''|*[!0-9]*) return 1 ;; esac ;; *) return 1 ;; esac; }
