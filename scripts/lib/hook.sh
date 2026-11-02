#!/usr/bin/env bash
# The hooks' stdin payload. Sourced, never run; no forks, and bash 3.2 safe:
# agent-state has no bash-4 floor.

# Bounded read, not `cat`: Claude Code can hold the pipe open past the payload.
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

# <pane> shows an agent view (#174): its turn runs in a background worker that reports no
# state of its own, so scripts/agent-poll owns the pane's @panestate from here on. Marks the
# pane and makes sure one poller runs on this server. Costs one tmux read and, with the
# poller alive, one /proc read; the start is detached (run-shell -b) so the hook never waits.
mark_agentview() {            # <pane> <scripts dir>
    local info view pid every q
    info=$(tmux display-message -p -t "$1" \
        '#{@cockpit_agentview}|#{@cockpit_poll_pid}|#{@cockpit_bg_poll}' 2>/dev/null) || return 0
    view=${info%%|*}; info=${info#*|}; pid=${info%%|*}; every=${info#*|}
    [ "$every" = off ] && return 0
    [ "$view" = 1 ] || tmux set-option -p -t "$1" @cockpit_agentview 1 2>/dev/null
    poll_alive "$pid" && return 0
    printf -v q '%q' "$2/agent-poll"
    tmux run-shell -b "$q" 2>/dev/null
    return 0
}
