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

# A hook may run outside the pane's environment: Claude Code's daemon worker has no TMUX and
# no TMUX_PANE, though the daemon and the front end above it do (#174). Walks the parent
# chain from <pid> ($PPID by default), 12 hops at most and never pid 1, and exports the
# TMUX_PANE and TMUX of the first process that has a pane id. The pair comes from one
# process, so a stale TMUX of ours never points a recovered pane at the wrong server.
# Nothing found leaves both as they were. Callers test $TMUX_PANE first, so the fast path
# never reaches here. Linux reads /proc/<pid>/{stat,environ} with no fork; where /proc
# is absent (macOS) ps does both. COCKPIT_NO_PANE_WALK=1 turns it off; COCKPIT_PROC_DIR
# stands in for /proc (the tests' fake process table).
find_pane() {                 # [pid]
    [ -n "${TMUX_PANE:-}" ] && return 0
    [ -n "${COCKPIT_NO_PANE_WALK:-}" ] && return 0
    local proc=${COCKPIT_PROC_DIR:-/proc} pid=${1:-$PPID} hops=0
    local fp="" fs="" kv line penv
    while [ "$hops" -lt 12 ] && [ "$pid" -gt 1 ] 2>/dev/null; do
        hops=$((hops + 1)); fp="" fs=""
        if [ -d "$proc" ]; then
            # NUL-separated, which a bash variable cannot hold: read entry by entry.
            # 2>/dev/null first, or the open's failure is reported before it applies.
            while IFS= read -r -d '' kv || [ -n "$kv" ]; do
                case "$kv" in
                    TMUX_PANE=%*) fp=${kv#TMUX_PANE=} ;;
                    TMUX=*) fs=${kv#TMUX=} ;;
                esac
            done 2>/dev/null < "$proc/$pid/environ"
            line=""; read -r line 2>/dev/null < "$proc/$pid/stat" || [ -n "$line" ] || return 0
            # "pid (comm) S ppid ...": comm is free text, so cut at the last ") ".
            line=${line##*) }; line=${line#* }
            pid=${line%% *}
        else
            penv=$(ps eww -o command= -p "$pid" 2>/dev/null)
            case "$penv" in
                *" TMUX_PANE=%"*) fp=${penv#*" TMUX_PANE="}; fp=${fp%%[[:space:]]*} ;;
            esac
            case "$penv" in
                *" TMUX="*) fs=${penv#*" TMUX="}; fs=${fs%%[[:space:]]*} ;;
            esac
            pid=$(ps -o ppid= -p "$pid" 2>/dev/null) || return 0
            pid=${pid//[[:space:]]/}
        fi
        case "${fp#%}" in ''|*[!0-9]*) continue ;; esac
        export TMUX_PANE=$fp
        if [ -n "$fs" ]; then export TMUX=$fs; else unset TMUX; fi
        return 0
    done
    return 0
}
