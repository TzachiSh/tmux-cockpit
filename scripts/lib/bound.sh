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

# Its parent now: /proc/<pid>/stat field 4, after the last ")"; macOS `ps -o ppid=`.
bound_ppid() {                # <pid> -> its parent's pid, or nothing
    local s bound_fields
    if [ -r "/proc/$1/stat" ]; then
        read -r s 2>/dev/null < "/proc/$1/stat" || return 0
        read -r -a bound_fields <<< "${s##*) }"
        printf '%s' "${bound_fields[1]-}"
    elif [ ! -d /proc/self ]; then
        s=$(ps -o ppid= -p "$1" 2>/dev/null) || return 0
        printf '%s' "${s//[[:space:]]/}"
    fi
}

# TERM to <pid> and every process below it, only while <pid> is still the process started at
# <start time>: a gone or reused pid, or one whose start time cannot be read, is signalled
# nothing, and nor is anything below it. The tree is frozen first, top down: SIGSTOP the
# verified top, then each child of a stopped process, read from a fresh
# `ps -A -o pid= -o ppid=` (POSIX, so on macOS too). A stopped process can start no other,
# and none of them can run on between its child's end and its own (a wrapper would start
# its next command, or exit 0). Each child is stopped only while its parent is still the
# stopped one, read again just before (bound_ppid), and kept only if that still holds after:
# else it is let go (SIGCONT), a pid the kernel handed to someone else. Then all of them get
# TERM, then CONT, so each acts on it. A forking wrapper's child would otherwise hold the
# caller's $( ) open after the wrapper ends.
bound_kill() {                # <pid> <start time>
    [ -n "$2" ] && [ "$(bound_start "$1")" = "$2" ] || return 0
    kill -STOP "$1" 2>/dev/null || return 0
    local held=$1 todo=$1 c pid ppid
    while [ -n "$todo" ]; do
        c=${todo%% *}; todo=${todo#"$c"}; todo=${todo# }
        while read -r pid ppid; do
            [ "$ppid" = "$c" ] && [ "$(bound_ppid "$pid")" = "$c" ] || continue
            kill -STOP "$pid" 2>/dev/null || continue
            if [ "$(bound_ppid "$pid")" = "$c" ]; then
                held="$held $pid"; todo="$todo $pid"
            else
                kill -CONT "$pid" 2>/dev/null
            fi
        done <<EOF_BOUND
$(ps -A -o pid= -o ppid= 2>/dev/null)
EOF_BOUND
    done
    # shellcheck disable=SC2086  # a list of pids
    kill -TERM $held 2>/dev/null
    # shellcheck disable=SC2086
    kill -CONT $held 2>/dev/null
}

# The timer: a sleep with no children, so no tree, the same start-time check.
bound_end() {                 # <pid> <start time>
    [ -n "$2" ] && [ "$(bound_start "$1")" = "$2" ] && kill -TERM "$1" 2>/dev/null
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
        bound_end "$s" "$ts"
        read -r _ <&9                      # EOF: the timer has finished
        exit "$rc"
    )
}
