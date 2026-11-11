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
# since the comm in field 2 may hold spaces or parens. macOS: `ps -o lstart=`, to the second.
# Nothing when the process is gone or cannot be read.
bound_start() {               # <pid> -> its start time
    local s f
    if [ -r "/proc/$1/stat" ]; then
        read -r s 2>/dev/null < "/proc/$1/stat" || return 0
        read -r -a f <<< "${s##*) }"
        printf '%s' "${f[19]-}"            # field 22: the 20th after the comm
    elif [ ! -d /proc/self ]; then
        ps -o lstart= -p "$1" 2>/dev/null
    fi
}

# TERM to <pid> only while it is still the process started at <start time>: a gone or
# reused pid, or one whose start time cannot be read, is signalled nothing.
bound_kill() {                # <pid> <start time>
    [ -n "$2" ] && [ "$(bound_start "$1")" = "$2" ] && kill -TERM "$1" 2>/dev/null
}

bound() {                     # <secs> <cmd> [args...] -> cmd's status (143 when ended)
    local secs=$1 p tp s ts rc; shift
    if [ -n "$BOUND_TO" ]; then "$BOUND_TO" "$secs" "$@"; return; fi
    # An async command reads /dev/null unless told otherwise: hand it this stdin.
    "$@" <&0 & p=$!
    tp=$(bound_start "$p")
    # The timer: a sleep whose pid and start time it prints first, so this shell can end it
    # when the command ends early, then at the deadline the command is ended, if it is still
    # the one started above. No `wait -n` in bash 3.2; no spinner.
    exec 9< <(sleep "$secs" </dev/null >/dev/null 2>&1 & s=$!
              echo "$s $(bound_start "$s")"
              wait "$s" && bound_kill "$p" "$tp") 2>/dev/null
    read -r s ts <&9
    wait "$p"; rc=$?
    bound_kill "$s" "$ts"
    read -r _ <&9                          # EOF: the timer has finished
    exec 9<&-
    return "$rc"
}
