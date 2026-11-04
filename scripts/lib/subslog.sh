#!/usr/bin/env bash
# The per-session subagent event log (#361). Sourced, never run; no hook.sh needed.
#
#   ${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/subs/<session_id>.log
#   one line per event: <epoch>\t<start|stop|idle>\t<agent_id>
#
# Append-only, so hooks writing side by side lose nothing: one write under PIPE_BUF (4096
# bytes) is atomic, and no hook reads before it writes. The reader takes the newest line per
# agent_id. Not a counter: SubagentStart also fires for a resumed subagent and for every
# message an in-process teammate handles (#317), so only each agent's newest event counts.
# That event says what the agent did last: a teammate's SubagentStop ends a turn, not the
# agent, and no documented hook fires when TaskStop or a shutdown ends one.

subs_dir() {
    local base=${XDG_STATE_HOME:-}
    [ -n "$base" ] || { [ -n "${HOME:-}" ] || return 1; base=$HOME/.local/state; }
    REPLY=$base/tmux-cockpit/subs
}

# One id of the payload: "key": "value", the value only [A-Za-z0-9_-], 1 to 64 bytes. A
# quote inside a JSON string arrives as \", so no message text can match the key. REPLY empty
# on any miss. The first match wins, and the ids come before the final message.
subs_id() {                   # <key> -> REPLY
    local LC_ALL=C re="\"$1\": ?\"([A-Za-z0-9_-]{1,64})\""
    REPLY=""; [[ $payload =~ $re ]] && REPLY=${BASH_REMATCH[1]}
    return 0
}

# The payload: the first line, bounded by 2 s where `timeout` exists (#320, #322: a writer
# that sends neither a newline nor EOF must not hold the hook) and by 256 KB. Fails open to
# an empty payload.
subs_payload() {
    local to=""; command -v timeout >/dev/null 2>&1 && to="timeout 2"
    payload=$($to head -n 1 2>/dev/null) || payload=""
    payload=${payload:0:262144}
}

# subs_record <start|stop|idle>: one line for the payload's agent, or nothing at all.
subs_record() {
    local ev=$1 sid aid now dir f
    subs_payload
    subs_id session_id; sid=$REPLY; [ -n "$sid" ] || return 0
    subs_id agent_id; aid=$REPLY; [ -n "$aid" ] || return 0
    subs_dir || return 0; dir=$REPLY; f=$dir/$sid.log
    # 0700 dir, 0600 log, whatever the caller's umask; no link is ever followed.
    umask 077
    [ -L "$dir" ] && return 0
    [ -d "$dir" ] || mkdir -p -- "$dir" 2>/dev/null || return 0
    [ -L "$f" ] && return 0
    now=${EPOCHSECONDS:-}; [ -n "$now" ] || now=$(date +%s 2>/dev/null) || return 0
    printf '%s\t%s\t%s\n' "$now" "$ev" "$aid" >> "$f" 2>/dev/null
    return 0
}

# subs_end: SessionEnd. Drops the payload's session log, and sweeps the logs nobody has
# written in 7 days (a session that never ended: a killed pane).
subs_end() {
    local sid dir
    subs_payload
    subs_dir || return 0; dir=$REPLY
    [ -d "$dir" ] && [ ! -L "$dir" ] || return 0
    subs_id session_id; sid=$REPLY
    [ -n "$sid" ] && [ -f "$dir/$sid.log" ] && [ ! -L "$dir/$sid.log" ] && rm -f -- "$dir/$sid.log" 2>/dev/null
    find "$dir" -maxdepth 1 -type f -name '*.log' -mtime +7 -exec rm -f -- {} + 2>/dev/null
    return 0
}
