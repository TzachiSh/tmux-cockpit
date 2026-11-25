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

# The same read, capped at <cap> bytes (#308, #315): bash reads a pipe a byte at a time, and
# a payload can carry a whole prompt or final message. A key past the cap is not seen.
# Bytes, not characters: a UTF-8 locale would count `read -n` in characters. The locale is
# put back on return; <stop-cmd> runs under C too. A cap that is no number reads nothing.
read_capped() {               # <cap> <stop-cmd...>
    payload="" chunk=""
    case "${1:-}" in ''|*[!0-9]*) return 1 ;; esac
    local LC_ALL=C cap=$1 deadline=$(( SECONDS + 2 )) left
    shift
    while left=$(( deadline - SECONDS )); [ "$left" -gt 0 ] && [ ${#payload} -lt "$cap" ]; do
        IFS= read -r -n $(( cap - ${#payload} )) -t "$left" chunk ||
            { payload="$payload$chunk"; break; }
        payload="$payload$chunk
"; chunk=""
        "$@" && break
    done
}

# The Stop payload's first line, whole, into payload (#310). head, not read_payload: bash reads
# a pipe a byte per syscall, about 2.4 s a MB, and a 2 s deadline then cut the array off a long
# final message. The payload is compact JSON, one line: claude writes it with a newline and
# closes stdin (2.1.317), so head returns. Bounded all the same, by <secs> (#320): a writer that
# sends neither must not hold the Stop. bound: timeout, else gtimeout; unbounded with neither
# (#746). 1 when the bound fires or the read cannot run, and payload is then empty: no part of
# a cut line is read.
read_stop() {                 # <secs>
    payload=""
    # shellcheck source=bound.sh
    . "${BASH_SOURCE[0]%/*}/bound.sh" 2>/dev/null || return 1
    payload=$(bound "$1" head -n 1 2>/dev/null) && return 0
    payload=""
    return 1
}

# How many background subagents the Stop payload lists in flight (#310, #313), in SUBS. Only
# subagents: an idle teammate stays "running" in claude's registry for the life of its team,
# and a shell or monitor task may never end. Matched structurally: inside a JSON string the
# quotes arrive escaped, so no message text can match. No array: 0.
subs_count() {
    local re arr m n
    SUBS=0
    # Regexes, linear in the payload: a `${payload#*key}` cut rescans it once per byte, and
    # the final message, any size, comes before the key. After `[`, whole strings or single
    # bytes other than `]`, so the match ends with the array (its entries are flat objects,
    # no nested array) and a `]` inside a string does not end it. Bytes, not characters: in a
    # UTF-8 locale glibc's multibyte matcher is quadratic on a long array, and a byte that is
    # not UTF-8 ends the match. Restored on return; nothing below forks. Still quadratic, even
    # under C: a payload repeating an unclosed `"background_tasks":[`, which valid JSON with
    # the key once cannot be.
    local LC_ALL=C
    re='"background_tasks": ?\[(("([^"\\]|\\.)*"|[^]"])*)'
    [[ $payload =~ $re ]] || return 0
    arr=${BASH_REMATCH[1]}
    # Counted by length, in one pass per spelling: deleting every marker shortens the array by
    # the marker's length per hit (a regex per hit was quadratic, 24 s at 3000). A marker
    # inside a string cannot match: its quotes arrive as \", so a backslash follows `type`.
    for m in '"type":"subagent"' '"type": "subagent"'; do
        n=${#arr} arr=${arr//"$m"/}
        SUBS=$((SUBS + (n - ${#arr}) / ${#m}))
    done
}

# A session id is saved only as a UUID (#308); a value cut by a cap fails the test. A new id
# drops the old transcript, which is not its transcript (/clear, --resume). 0 only when the
# id is new to the pane: <old> is the id read before, from a read known to be whole (#707).
# The same id writes nothing: every set-option redraws.
save_session() {              # <pane> <sid> <old>
    local uuid='^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
    [[ ${2:-} =~ $uuid ]] && [ "${3:-}" != "$2" ] || return 1
    tmux set-option -pu -t "$1" @cockpit_transcript 2>/dev/null
    tmux set-option -p -t "$1" @cockpit_session "$2" 2>/dev/null
    return 0
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
        # macOS: comm is the interpreter for a script, so ask for the command line, and take
        # only the shebang's form: bash, then the script as its first argument, named exactly
        # agent-poll. A path may hold a space; a space then / or - is the next argument.
        comm=$(ps -o args= -p "$1" 2>/dev/null)
        local interp=${comm%% *} rest
        rest=${comm#"$interp"}; rest=${rest# }; comm=""
        if [ "${interp##*/}" = bash ]; then
            case "$rest" in
                agent-poll|agent-poll\ *|*/agent-poll|*/agent-poll\ *)
                    case "${rest%%agent-poll*}" in
                        -*|*' /'*|*' -'*) ;;
                        *) comm=agent-poll ;;
                    esac ;;
            esac
        fi
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

# The config dir as a tmux argument to record on a pane, or 1 when it cannot be one. A control
# character would split the sweep's list-panes rows and PANE_FMT's tab-separated ones, so such
# a dir is not recorded (start_poller refuses it too): its pane is left on none, the default
# dir's, which the default poller serves (#685). tmux reads an argument that ends in ; as a
# separator: a trailing ; goes as \;, which tmux reads back as ; (#194).
cfg_arg() {                   # <dir>
    case "$1" in *[[:cntrl:]]*) return 1 ;; esac
    case "$1" in *\;) printf '%s\\;' "${1%;}" ;; *) printf '%s' "$1" ;; esac
}
CFG_BAD="a config dir with a control character is not used: its pane is polled on the default dir"

# Claude Code's subprocess env scrub takes CLAUDE_CONFIG_DIR and XDG_STATE_HOME out of its hooks'
# env (#1429, #1438), so the hook then reads its pane as the default dir's and keeps its state
# under ~/.local/state. On is what Claude Code 2.1.331 reads as on: 1, true, yes or on, in any
# case, spaces trimmed. Its own rule also turns the scrub on under GitHub Actions; that clause is
# left out, as our CI sets GITHUB_ACTIONS for every hook it runs.
scrub_on() {
    local v=${CLAUDE_CODE_SUBPROCESS_ENV_SCRUB:-}
    v=${v#"${v%%[![:space:]]*}"}; v=${v%"${v##*[![:space:]]}"}
    case $v in 1|[Tt][Rr][Uu][Ee]|[Yy][Ee][Ss]|[Oo][Nn]) return 0 ;; esac
    return 1
}
SCRUB_MSG="with CLAUDE_CODE_SUBPROCESS_ENV_SCRUB on the hooks see no CLAUDE_CONFIG_DIR and no"
SCRUB_MSG="$SCRUB_MSG XDG_STATE_HOME so their panes are polled on the default dir and their review"
SCRUB_MSG="$SCRUB_MSG and subagent state goes to the default state dir"

# What this hook does to @cockpit_poll_err for the scrub: SCRUB_IF, a format that reads 1 when
# the write is due, and SCRUB_DO, the write, for if-shell -F. The scrub on and no dir seen: the
# message, only into an empty slot, so it replaces no other. Else that message is taken back, and
# only that one. The slot is global, so a hook in any pane may take it back.
scrub_note() {
    if [ -z "${CLAUDE_CONFIG_DIR:-}" ] && scrub_on; then
        SCRUB_IF='#{?#{@cockpit_poll_err},0,1}'
        SCRUB_DO="set-option -g @cockpit_poll_err '$SCRUB_MSG'"
    else
        SCRUB_IF="#{==:#{@cockpit_poll_err},$SCRUB_MSG}"
        SCRUB_DO='set-option -gu @cockpit_poll_err'
    fi
}

# Start a poller, detached (run-shell -b), so the caller never waits for it. Two starts at once
# are fine: the slot is a compare-and-set in the server and the loser exits. With a dir, the
# poller runs with CLAUDE_CONFIG_DIR set to it; without, with none, so the server's own value
# does not choose the dir for panes whose claude used the default. Every automatic start comes
# here, the sweeps' and a relayed notification's, so COCKPIT_NO_AUTOSTART in the caller's
# environment stops each of them (#1111).
start_poller() {              # <scripts dir> [<config dir>]
    local cmd key have dir=${2:-}
    [ -z "${COCKPIT_NO_AUTOSTART:-}" ] || return 0
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
    # COCKPIT_NO_AUTOSTART: start_poller refuses it (#1111); this return also skips the reads.
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
    local info view fg pid every scrub cfg key arg="" bad="" dir=${CLAUDE_CONFIG_DIR:-}
    # A dir cfg_arg refuses is not used: the pane is the default dir's, and that poller starts.
    [ -z "$dir" ] || arg=$(cfg_arg "$dir") || { bad=1; dir=""; }
    key=$(poll_key "$dir") || return 0
    scrub_note
    info="#{@cockpit_agentview}|#{@cockpit_fg}|#{$(poll_slot "$key")}|#{@cockpit_bg_poll}"
    info=$(tmux display-message -p -t "$1" "$info|$SCRUB_IF|#{@cockpit_cfg}" 2>/dev/null) \
        || return 0
    view=${info%%|*}; info=${info#*|}; fg=${info%%|*}; info=${info#*|}
    pid=${info%%|*}; info=${info#*|}; every=${info%%|*}; info=${info#*|}
    scrub=${info%%|*}; cfg=${info#*|}
    [ "$every" = off ] && return 0
    [ "$view" = 1 ] || tmux set-option -p -t "$1" @cockpit_agentview 1 2>/dev/null
    # The agent view relays for this pane: it shows jobs now, whatever session ran in it before.
    [ -z "$fg" ] || tmux set-option -pu -t "$1" @cockpit_fg 2>/dev/null
    # The config dir this pane's claude uses, written only when it changed (#194). One that
    # cannot be recorded leaves the pane on none, and says so. The pane's dir goes last: a cut
    # option name ends the list there (#1450), and the next hook runs it again only while the
    # pane's dir differs.
    if [ -n "$bad" ]; then
        tmux set-option -pu -t "$1" @cockpit_cfg \; \
            set-option -g @cockpit_poll_err "$CFG_BAD" 2>/dev/null
    elif [ "$cfg" != "$dir" ]; then
        if [ -z "$dir" ]; then tmux set-option -pu -t "$1" @cockpit_cfg 2>/dev/null
        else
            tmux set-option -g @cockpit_cfg_any 1 \; set-option -p -t "$1" @cockpit_cfg "$arg" \
                2>/dev/null
        fi
    fi
    # The scrub's message (#1429): a call only when the read said the write is due, and the
    # server tests it again.
    [ "$scrub" != 1 ] || tmux if-shell -F -t "$1" "$SCRUB_IF" "$SCRUB_DO" 2>/dev/null
    poll_alive "$pid" && return 0
    start_poller "$2" "$dir"
}
