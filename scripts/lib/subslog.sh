#!/usr/bin/env bash
# The per-session subagent event log (#361). Sourced, never run; no hook.sh needed.
#
#   ${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/subs/<session_id>.log
#   one line per event: <epoch>\t<start|stop|idle>\t<agent_id>
#   a TaskStop (#403):  <epoch>\tkill\t<agent_id>                  a plain agent
#                       <epoch>\tkill-tm\t<name>\t<description>    a teammate, mapped by the reader
#   a Stop (#403):      <epoch>\tlive\t<id>,<id>...\t<description>...  the in-flight set
# A description is the payload's JSON string body, escapes and all: no tab, newline or quote.
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
# that sends neither a newline nor EOF must not hold the hook) and by 256 KB: the second
# head stops the read, the first gets SIGPIPE. Fails open to an empty payload.
subs_payload() {
    local to=""; command -v timeout >/dev/null 2>&1 && to="timeout 2"
    payload=$($to head -n 1 2>/dev/null | head -c 262144) || payload=""
}

# subs_desc <key> [text]: a "key": "string" of the text (default: the payload), as the JSON
# string's body, 0 to 512 bytes, no control byte. REPLY empty and status 1 on any miss.
subs_desc() {
    local LC_ALL=C re='"'"$1"'": ?"(([^"\\]|\\.)*)"' t=${2-$payload}
    REPLY=""
    [[ $t =~ $re ]] || return 1
    [ "${#BASH_REMATCH[1]}" -le 512 ] && [[ ${BASH_REMATCH[1]} != *[[:cntrl:]]* ]] || return 1
    REPLY=${BASH_REMATCH[1]}
}

# subs_record <start|stop|idle>: one line for the payload's agent, or nothing at all.
subs_record() {
    local sid aid
    subs_payload
    subs_id session_id; sid=$REPLY; [ -n "$sid" ] || return 0
    subs_id agent_id; aid=$REPLY; [ -n "$aid" ] || return 0
    subs_write "$sid" "$1"$'\t'"$aid"
}

# subs_taskstop: PostToolUse on TaskStop. A plain agent's tool_response.task_id is its agent
# id; any other task's is a task id no file holds, so the row carries what the reader maps a
# teammate by: tool_input.task_id (the name the model gave, when plain) and tool_response's
# command (the description). A failed TaskStop fires no hook.
subs_taskstop() {
    local LC_ALL=C sid in out ty name re
    subs_payload
    re='"tool_name": ?"TaskStop"'; [[ $payload =~ $re ]] || return 0
    subs_id session_id; sid=$REPLY; [ -n "$sid" ] || return 0
    # Each object's tail, from its key: the first id after it is the object's own. Keys inside
    # a string cannot match, their quotes arrive escaped.
    re='"tool_input": ?\{(.*)'; [[ $payload =~ $re ]] || return 0; in=${BASH_REMATCH[1]}
    re='"tool_response": ?\{(.*)'; [[ $payload =~ $re ]] || return 0; out=${BASH_REMATCH[1]}
    re='"task_type": ?"([a-z_]{1,32})"'; [[ $out =~ $re ]] || return 0; ty=${BASH_REMATCH[1]}
    if [ "$ty" = local_agent ]; then
        payload=$out subs_id task_id; [ -n "$REPLY" ] || return 0
        subs_write "$sid" "kill"$'\t'"$REPLY"
        return 0
    fi
    # Only a teammate: a shell's (local_bash) or any other task's description could name one.
    [ "$ty" = in_process_teammate ] || return 0
    payload=$in subs_id task_id; name=$REPLY
    subs_desc command "$out" || REPLY=""
    [ -n "$name$REPLY" ] || return 0
    subs_write "$sid" "kill-tm"$'\t'"$name"$'\t'"$REPLY"
}

# subs_reconcile: Stop. The parent's turn has ended; its background_tasks lists what is still in
# flight: a subagent by its agent id, a teammate by a task id, so by its description. Only
# Stop's: SubagentStop's lists the agent that is stopping as running. A list it cannot read
# whole, or a row past one atomic write, writes nothing: a short set would end live agents.
subs_reconcile() {
    local LC_ALL=C sid arr re rt ob ty ids="" descs=""
    subs_payload
    subs_id session_id; sid=$REPLY; [ -n "$sid" ] || return 0
    # The array as agent-state's subs_count matches it, whole strings or bytes other than `]`,
    # and no `[` either: a nested array would end the match early and drop the entries after it.
    re='"background_tasks": ?\[(("([^"\\]|\\.)*"|[^]["])*)\]'
    [[ $payload =~ $re ]] || return 0
    arr=${BASH_REMATCH[1]}
    # Bounded: each step below copies the rest of the array, so the loop is quadratic in it.
    [ "${#arr}" -le 65536 ] || return 0
    # One flat object at a time, commas and spaces between: whole strings or bytes other than
    # braces and brackets, so a nested object or array is no match, and what is left at the
    # end is not all separators, which writes nothing.
    re='^[[:space:],]*\{(("([^"\\]|\\.)*"|[^][{}"])*)\}' rt='"type": ?"([a-z_]{1,32})"'
    while [[ $arr =~ $re ]]; do
        ob=${BASH_REMATCH[1]} arr=${arr:${#BASH_REMATCH[0]}}
        [[ $ob =~ $rt ]] || return 0
        ty=${BASH_REMATCH[1]}
        case "$ty" in
        subagent)
            payload=$ob subs_id id; [ -n "$REPLY" ] || return 0
            ids=${ids:+$ids,}$REPLY ;;
        teammate)
            subs_desc description "$ob" || return 0
            descs=$descs$'\t'$REPLY ;;
        esac
        [ $((${#ids} + ${#descs})) -le 3890 ] || return 0
    done
    [[ $arr =~ ^[[:space:],]*$ ]] || return 0
    subs_write "$sid" "live"$'\t'"$ids$descs"
}

# subs_write <session_id> <row>: "<epoch>\t<row>" appended to the session's log.
subs_write() {
    local sid=$1 now dir f
    subs_dir || return 0; dir=$REPLY; f=$dir/$sid.log
    # 0700 dir, 0600 log, whatever the caller's umask; no link is ever followed.
    umask 077
    [ -L "$dir" ] && return 0
    [ -d "$dir" ] || mkdir -p -- "$dir" 2>/dev/null || return 0
    [ -L "$f" ] && return 0
    now=${EPOCHSECONDS:-}; [ -n "$now" ] || now=$(date +%s 2>/dev/null) || return 0
    printf '%s\t%s\n' "$now" "$2" >> "$f" 2>/dev/null
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
    if [ -n "$sid" ] && [ -f "$dir/$sid.log" ] && [ ! -L "$dir/$sid.log" ]; then
        rm -f -- "$dir/$sid.log" 2>/dev/null
    fi
    # -mmin, not -mtime: -mtime +7 spares a log until it is 8 whole days old.
    find "$dir" -maxdepth 1 -type f -name '*.log' -mmin +10080 -exec rm -f -- {} + 2>/dev/null
    return 0
}
