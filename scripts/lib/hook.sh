#!/usr/bin/env bash
# The hooks' stdin payload. Sourced, never run; no forks, and bash 3.2 safe:
# agent-state has no bash-4 floor.

# Bounded read, not `cat`: a hook must never wait on its writer. Claude Code 2.1.317 ends
# the payload with a newline and closes stdin; nothing promises either.
# Newlines are put back so two lines cannot fuse into a match never sent.
read_payload() {              # <stop-cmd...>: ends the read early once it succeeds
    payload="" chunk=""
    local deadline=$(( SECONDS + 2 )) left
    while left=$(( deadline - SECONDS )); [ "$left" -gt 0 ]; do
        IFS= read -r -t "$left" chunk || { payload="$payload$chunk"; break; }
        payload="$payload$chunk
"; chunk=""
        "$@" && break
    done
}

# "key":"value" in $payload -> REPLY, raw. Not a JSON parser: an escaped quote
# inside the value truncates it.
json_str() {                  # <key>
    case "$payload" in
        *"\"$1\""*) REPLY=${payload#*\""$1"\"}; REPLY=${REPLY#*:}; REPLY=${REPLY#*\"}; REPLY=${REPLY%%\"*} ;;
        *) REPLY="" ;;
    esac
}

# Is <pid> a live agent-poll? A pid published in a tmux option outlives its process and the
# kernel reuses the number, so the name is checked before anything trusts it (AGENTS.md).
# -r first: the stale pid is the one absent from /proc, so `read || ps` would fork every time.
# agent-poll is run by its own path (#!/bin/bash), so the kernel names the process after it.
poll_alive() {                # <pid>
    local comm=""
    case "$1" in ''|*[!0-9]*) return 1 ;; esac
    if [ -r "/proc/$1/comm" ]; then
        read -r comm 2>/dev/null < "/proc/$1/comm"
    elif [ ! -d /proc/self ]; then
        # macOS: comm is the interpreter for a script, so ask for the command line.
        comm=$(ps -o args= -p "$1" 2>/dev/null)
        case "$comm" in *agent-poll|*agent-poll\ *) comm=agent-poll ;; esac
    fi
    [ "${comm##*/}" = agent-poll ]
}

# Start a poller, detached (run-shell -b), so the caller never waits for it. Two starts at once
# are fine: the slot is a compare-and-set in the server and the loser exits.
start_poller() {              # <scripts dir>
    local q
    printf -v q '%q' "$1/agent-poll"
    # run-shell expands formats: a # in the install path must stay a #.
    # shellcheck source=fmt.sh
    . "${BASH_SOURCE[0]%/*}/fmt.sh" 2>/dev/null || return 0
    tmux run-shell -b "$(fesc "$q")" 2>/dev/null
    return 0
}

# <pane> shows an agent view (#174): its turn runs in a background worker that reports no
# state of its own, so scripts/agent-poll owns the pane's @panestate from here on. Marks the
# pane and makes sure one poller runs on this server. Costs one tmux read and, with the
# poller alive, one /proc read; the start is detached (run-shell -b) so the hook never waits.
mark_agentview() {            # <pane> <scripts dir>
    local info view pid every
    info=$(tmux display-message -p -t "$1" \
        '#{@cockpit_agentview}|#{@cockpit_poll_pid}|#{@cockpit_bg_poll}' 2>/dev/null) || return 0
    view=${info%%|*}; info=${info#*|}; pid=${info%%|*}; every=${info#*|}
    [ "$every" = off ] && return 0
    [ "$view" = 1 ] || tmux set-option -p -t "$1" @cockpit_agentview 1 2>/dev/null
    poll_alive "$pid" && return 0
    start_poller "$2"
}
