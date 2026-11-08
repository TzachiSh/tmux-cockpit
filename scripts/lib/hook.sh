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
    # COCKPIT_PROC_DIR: the tests point it at an empty dir to run the macOS branch (#194).
    local comm="" proc=${COCKPIT_PROC_DIR:-/proc}
    case "$1" in ''|*[!0-9]*) return 1 ;; esac
    if [ -r "$proc/$1/comm" ]; then
        read -r comm 2>/dev/null < "$proc/$1/comm"
    elif [ ! -d "$proc/self" ]; then
        # macOS: comm is the interpreter for a script, so ask for the command line.
        comm=$(ps -o args= -p "$1" 2>/dev/null)
        case "$comm" in *agent-poll|*agent-poll\ *) comm=agent-poll ;; esac
    fi
    [ "${comm##*/}" = agent-poll ]
}

# One poller per Claude config dir (#194): a pane's hooks run with the CLAUDE_CONFIG_DIR its
# claude uses, so a pane on another dir is polled against that dir's agents. The dir as claude
# got it, never normalised: two spellings of one dir are two pollers, which is harmless.
# Unset is the default dir and the slot @cockpit_poll_pid, as before; a dir's slot is
# @cockpit_poll_pid_<cksum>, beside @cockpit_poll_dir_<cksum> naming the dir it is for.
poll_key() {                  # <dir> -> "" for the default dir, else its cksum
    [ -n "$1" ] || return 0
    local k; k=$(printf '%s' "$1" | cksum 2>/dev/null) || return 1
    printf '%s' "${k%% *}"
}
poll_slot() { printf '@cockpit_poll_pid%s' "${1:+_$1}"; }    # <key>

# A single-quoted word for /bin/sh, which run-shell runs: %q can print $'...', which sh does not
# read.
shq() { printf "'%s'" "${1//\'/\'\\\'\'}"; }

# Start a poller, detached (run-shell -b), so the caller never waits for it. Two starts at once
# are fine: the slot is a compare-and-set in the server and the loser exits. With a dir, the
# poller runs with CLAUDE_CONFIG_DIR set to it; without, with none, so the server's own value
# does not choose the dir for panes whose claude used the default.
start_poller() {              # <scripts dir> [<config dir>]
    local cmd key have dir=${2:-}
    # shellcheck source=fmt.sh
    . "${BASH_SOURCE[0]%/*}/fmt.sh" 2>/dev/null || return 0
    if [ -z "$dir" ]; then
        cmd="env -u CLAUDE_CONFIG_DIR $(shq "$1/agent-poll")"
    else
        case "$dir" in *[[:cntrl:]]*) return 0 ;; esac
        key=$(poll_key "$dir") && [ -n "$key" ] || return 0
        # Two dirs on one cksum share no slot: the second is refused, and said (#194).
        have=$(tmux show-option -gqv "@cockpit_poll_dir_$key" 2>/dev/null)
        if [ -n "$have" ] && [ "$have" != "$dir" ]; then
            tmux set-option -g @cockpit_poll_err "config dirs share a poller key: $key" 2>/dev/null
            return 0
        fi
        [ -n "$have" ] || tmux set-option -g "@cockpit_poll_dir_$key" "$dir" 2>/dev/null
        cmd="env CLAUDE_CONFIG_DIR=$(shq "$dir") COCKPIT_POLL_KEY=$key $(shq "$1/agent-poll")"
    fi
    # run-shell expands formats: a # in the install path or the dir must stay a #.
    tmux run-shell -b "$(fesc "$cmd")" 2>/dev/null
    return 0
}

# One poller runs on this server for each config dir in use (#448, #194). It binds a pane
# that shows a background job to that job, and a background job's hooks never touch tmux
# (#174), so nothing from a job can start it later: the sweeps (the load, and every window or
# pane change) do. Costs one tmux read and, with the poller alive, one /proc read; a server
# where a hook has seen a config dir (@cockpit_cfg_any) pays a list-panes for the dirs.
ensure_poller() {             # <scripts dir>
    local info pid any
    # COCKPIT_NO_AUTOSTART: the test suite's servers, which must not run the real claude.
    [ -z "${COCKPIT_NO_AUTOSTART:-}" ] || return 0
    info=$(tmux display-message -p '#{@cockpit_poll_pid}|#{@cockpit_bg_poll}|#{@cockpit_cfg_any}' \
        2>/dev/null) || return 0
    pid=${info%%|*}; info=${info#*|}; any=${info#*|}
    [ "${info%%|*}" = off ] && return 0
    poll_alive "$pid" || start_poller "$1"
    [ -n "$any" ] && ensure_dir_pollers "$1"
    return 0
}

# A poller for each config dir a pane's hook recorded, and the slot of a dir no pane holds
# any more taken back when its poller is gone: a compare-and-set, never a signal.
ensure_dir_pollers() {        # <scripts dir>
    local dirs d key pid keys=" " line k
    dirs=$(tmux list-panes -a -F '#{@cockpit_cfg}' 2>/dev/null | sort -u)
    while IFS= read -r d; do
        [ -n "$d" ] || continue
        key=$(poll_key "$d") && [ -n "$key" ] || continue
        keys="$keys$key "
        pid=$(tmux show-option -gqv "$(poll_slot "$key")" 2>/dev/null)
        poll_alive "$pid" || start_poller "$1" "$d"
    done <<EOT
$dirs
EOT
    tmux show-options -g 2>/dev/null | while read -r line; do
        case "$line" in @cockpit_poll_pid_*) ;; *) continue ;; esac
        k=${line%% *}; k=${k#@cockpit_poll_pid_}; pid=${line#* }
        case "$keys" in *" $k "*) continue ;; esac
        poll_alive "$pid" && continue
        tmux if-shell -F "#{==:#{@cockpit_poll_pid_$k},$pid}" \
            "set-option -gu @cockpit_poll_pid_$k ; set-option -gu @cockpit_poll_dir_$k" 2>/dev/null
    done
    return 0
}

# <pane> shows an agent view (#174): its turn runs in a background worker that reports no
# state of its own, so scripts/agent-poll owns the pane's @panestate from here on. Marks the
# pane and makes sure one poller runs on this server. Costs one tmux read and, with the
# poller alive, one /proc read; the start is detached (run-shell -b) so the hook never waits.
mark_agentview() {            # <pane> <scripts dir>
    local info view fg pid every cfg key dir=${CLAUDE_CONFIG_DIR:-}
    key=$(poll_key "$dir") || return 0
    info="#{@cockpit_agentview}|#{@cockpit_fg}|#{$(poll_slot "$key")}|#{@cockpit_bg_poll}"
    info=$(tmux display-message -p -t "$1" "$info|#{@cockpit_cfg}" 2>/dev/null) || return 0
    view=${info%%|*}; info=${info#*|}; fg=${info%%|*}; info=${info#*|}
    pid=${info%%|*}; info=${info#*|}; every=${info%%|*}; cfg=${info#*|}
    [ "$every" = off ] && return 0
    [ "$view" = 1 ] || tmux set-option -p -t "$1" @cockpit_agentview 1 2>/dev/null
    # The agent view relays for this pane: it shows jobs now, whatever session ran in it before.
    [ -z "$fg" ] || tmux set-option -pu -t "$1" @cockpit_fg 2>/dev/null
    # The config dir this pane's claude uses, written only when it changed (#194).
    if [ "$cfg" != "$dir" ]; then
        if [ -n "$dir" ]; then
            tmux set-option -p -t "$1" @cockpit_cfg "$dir" \; set-option -g @cockpit_cfg_any 1 \
                2>/dev/null
        else tmux set-option -pu -t "$1" @cockpit_cfg 2>/dev/null; fi
    fi
    poll_alive "$pid" && return 0
    start_poller "$2" "$dir"
}
