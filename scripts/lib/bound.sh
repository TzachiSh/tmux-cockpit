#!/usr/bin/env bash
# bound <secs> <cmd> [args...]: run cmd with this shell's stdin, stdout and stderr, ended
# after secs (#746). Sourced, never run. Where GNU timeout or Homebrew's gtimeout exists it is
# used, exactly as the callers did before; the watchdog below is reached only where neither
# does (stock macOS). bash 3.2 safe.
if command -v timeout >/dev/null 2>&1; then BOUND_TO=timeout
elif command -v gtimeout >/dev/null 2>&1; then BOUND_TO=gtimeout
else BOUND_TO=""
fi

# A pid alone names whatever the kernel handed that number to last. The pid plus its start
# time names one process: set at fork, kept across exec (an npm claude runs as node), and
# different for a reused pid. Linux: /proc/<pid>/stat field 22, read after the LAST ")"
# since the comm in field 2 may hold spaces or parens. macOS: `ps -o lstart=`, to the
# second, its padding trimmed (read strips it, and the two reads are compared). Nothing when
# the process is gone or cannot be read.
bound_start() {               # <pid> -> its start time
    local s bound_fields
    if [ -r "/proc/$1/stat" ]; then
        read -r s 2>/dev/null < "/proc/$1/stat" || return 0
        read -r -a bound_fields <<< "${s##*) }"
        printf '%s' "${bound_fields[19]-}"     # field 22: the 20th after the comm
    elif [ ! -d /proc/self ]; then
        bound_ps_start "$1"
    fi
}
bound_ps_start() {            # <pid> -> ps's lstart, trimmed
    local s
    s=$(ps -o lstart= -p "$1" 2>/dev/null) || return 0
    s=${s#"${s%%[![:space:]]*}"}; printf '%s' "${s%"${s##*[![:space:]]}"}"
}

# TERM to <pid>, and to the processes it started, only while <pid> is still the process
# started at <start time>: a gone or reused pid, or one whose start time cannot be read, is
# signalled nothing. Its children are read from the live parent link (pgrep -P) before it
# goes: a forking wrapper's child would hold the caller's $( ) open after it.
bound_kill() {                # <pid> <start time>
    [ -n "$2" ] && [ "$(bound_start "$1")" = "$2" ] || return 0
    local todo=$1 kids="" c k
    while [ -n "$todo" ]; do
        c=${todo%% *}; todo=${todo#"$c"}
        for k in $(pgrep -P "$c" 2>/dev/null); do kids="$kids $k"; todo="$todo $k"; done
        todo=${todo# }
    done
    # shellcheck disable=SC2086  # a list of pids
    kill -TERM "$1" $kids 2>/dev/null
}

bound() {                     # <secs> <cmd> [args...] -> cmd's status (143 when ended)
    if [ -n "$BOUND_TO" ]; then "$BOUND_TO" "$@"; return; fi
    # A subshell: the fd and the stderr redirect below stay in it, never the caller's.
    (
        secs=$1; shift
        # An async command reads /dev/null unless told otherwise: hand it this stdin.
        "$@" <&0 & p=$!
        tp=$(bound_start "$p")
        # The timer: a sleep whose pid and start time it prints first, so the command's end
        # can end it, and at the deadline it ends the command, if still the one started above.
        # No `wait -n` in bash 3.2; no spinner.
        exec 9< <(exec 2>/dev/null
                  sleep "$secs" >/dev/null & s=$!
                  echo "$s $(bound_start "$s")"
                  wait "$s" && bound_kill "$p" "$tp")
        read -r s ts <&9
        wait "$p"; rc=$?
        bound_kill "$s" "$ts"
        read -r _ <&9                      # EOF: the timer has finished
        exit "$rc"
    )
}
