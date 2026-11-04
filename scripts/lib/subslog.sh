#!/usr/bin/env bash
# The per-session subagent event log (#361). Sourced, never run; no hook.sh needed.
#
#   ${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/subs/<session_id>.log
#   one line per event: <epoch>\t<start|stop|idle>\t<agent_id>
#   a TaskStop (#403):  <epoch>\tkill\t<agent_id>                  a plain agent
#                       <epoch>\tkill-tm\t<name>\t<description>    a teammate, mapped by the reader
# A description is the payload's JSON string body, escapes and all: no tab, newline or quote.
#
#   <session_id>.live, beside it (#423): one line, <epoch>\t<id>,<id>...\t<description>...,
#   the newest Stop's in-flight set. Its own file, replaced whole by a rename: only the newest
#   set is ever read, and as rows in the log a busy lead's sets pushed the older events out of
#   the reader's tail.
#
# The log is append-only, so hooks writing side by side lose nothing: one write under PIPE_BUF (4096
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
# whole, an entry of a type it does not know, or a set past the bound writes nothing: a short
# set would end live agents, and no set ends none.
subs_reconcile() {
    local LC_ALL=C sid arr re rt ob ty ids="" descs="" n=0
    subs_payload
    subs_id session_id; sid=$REPLY; [ -n "$sid" ] || return 0
    # The array as agent-state's subs_count matches it, whole strings or bytes other than `]`,
    # and no `[` either: a nested array would end the match early and drop the entries after it.
    re='"background_tasks": ?\[(("([^"\\]|\\.)*"|[^]["])*)\]'
    [[ $payload =~ $re ]] || return 0
    arr=${BASH_REMATCH[1]}
    # Bounded: each step below copies the rest of the array, so the loop is quadratic in its
    # bytes, and each entry costs a few matches, so it is bounded in entries too (#423).
    [ "${#arr}" -le 65536 ] || return 0
    # One flat object at a time, commas and spaces between: whole strings or bytes other than
    # braces and brackets, so a nested object or array is no match, and what is left at the
    # end is not all separators, which writes nothing.
    re='^[[:space:],]*\{(("([^"\\]|\\.)*"|[^][{}"])*)\}' rt='"type": ?"([a-z_]{1,32})"'
    while [[ $arr =~ $re ]]; do
        ob=${BASH_REMATCH[1]} arr=${arr:${#BASH_REMATCH[0]}}
        [ $((n += 1)) -le 256 ] || return 0
        [[ $ob =~ $rt ]] || return 0
        ty=${BASH_REMATCH[1]}
        # Closed (#423): a type renamed by a Claude Code update would otherwise drop its agents
        # from the set, and the set would end them all.
        case "$ty" in
        subagent)
            payload=$ob subs_id id; [ -n "$REPLY" ] || return 0
            ids=${ids:+$ids,}$REPLY ;;
        teammate)
            subs_desc description "$ob" || return 0
            descs=$descs$'\t'$REPLY ;;
        shell) ;;
        *) return 0 ;;
        esac
        [ $((${#ids} + ${#descs})) -le 3890 ] || return 0
    done
    [[ $arr =~ ^[[:space:],]*$ ]] || return 0
    subs_live "$sid" "$ids$descs"
}

# subs_live <session_id> <set>: "<epoch>\t<set>" as the session's .live file, written beside it
# and renamed into place, so a reader sees the old set or the new one, never half of one.
subs_live() {
    local sid=$1 now dir tmp
    subs_dir || return 0; dir=$REPLY
    umask 077
    [ -L "$dir" ] && return 0
    [ -d "$dir" ] || mkdir -p -- "$dir" 2>/dev/null || return 0
    now=${EPOCHSECONDS:-}; [ -n "$now" ] || now=$(date +%s 2>/dev/null) || return 0
    # noclobber: the temp name is created, never an existing file or a link opened. mv renames
    # over the .live name itself, a link there included, and follows nothing.
    tmp=$dir/.$sid.live.$$
    if (set -C; printf '%s\t%s\n' "$now" "$2" > "$tmp") 2>/dev/null; then
        mv -f -- "$tmp" "$dir/$sid.live" 2>/dev/null || rm -f -- "$tmp" 2>/dev/null
    fi
    return 0
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

# subs_end: SessionEnd. Drops the payload's session log and .live file, and sweeps the files
# nobody has written in 7 days (a session that never ended: a killed pane; a temp a killed
# hook left).
subs_end() {
    local sid dir
    subs_payload
    subs_dir || return 0; dir=$REPLY
    [ -d "$dir" ] && [ ! -L "$dir" ] || return 0
    subs_id session_id; sid=$REPLY
    if [ -n "$sid" ]; then
        rm -f -- "$dir/$sid.log" "$dir/$sid.live" 2>/dev/null
    fi
    # -mmin, not -mtime: -mtime +7 spares a log until it is 8 whole days old.
    find "$dir" -maxdepth 1 -type f \( -name '*.log' -o -name '*.live' -o -name '.*.live.*' \) \
        -mmin +10080 -exec rm -f -- {} + 2>/dev/null
    return 0
}
