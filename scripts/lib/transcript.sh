#!/usr/bin/env bash
# What an agent is doing, read from its Claude Code transcript. Sourced, never run.
# The transcript format is internal to Claude Code, so every reader fails quietly:
# no output and status 1, never an error on stderr.
#
# Format gate: transcript lines carry a `version` (the Claude Code version). The
# fields read here were checked on 2.1.316; a version older than 2.1.246 is
# refused, a newer one is still tried and fails quietly if a field is gone.

# One jq program for both readers. $mode is "doing" or "tail". The input is the
# last lines of a transcript, slurped raw, so a line that is not JSON is skipped
# instead of failing the lot.
_TR_JQ='
def clean: split("\n") | map(select(test("\\S"))) | (.[0] // "")
    | explode | map(select(. >= 32 and . != 127)) | implode | sub("^\\s+"; "") | .[0:160];
def secs: try ((.timestamp | sub("\\.[0-9]+"; "") | fromdateiso8601) as $t
    | now - $t | floor | if . < 0 then 0 else . end) catch "";
def desc: first(.input.description?, .input.command?, .input.file_path?, .input.pattern?,
    .input.url? | select(type == "string" and test("\\S")) | clean) // "";
def tlabel: desc as $d | if $d == "" then .name else .name + ": " + $d end;
def items: .message.content | arrays | .[];

[split("\n")[] | fromjson? | select(type == "object")] as $all
| ([$all[] | .version? | strings] | last) as $v
| if $v != null and (($v | split(".") | map(tonumber? // 0)) < [2, 1, 246]) then empty else
  [$all[] | select(.type == "assistant" or .type == "user")] as $t
  | if $mode == "tail" then
      $t[] | if .type == "user" then
          (.message.content | strings | clean) | select(. != "") | "USER: " + .
        else items | if .type == "text" then
            (.text | strings | clean) | select(. != "") | "AGENT: " + .
          elif .type == "tool_use" then "TOOL " + tlabel
          else empty end end
    else
      ([$t[] | select(.type == "user") | items | select(.type == "tool_result")
          | .tool_use_id]) as $closed
      | ([$t[] | select(.type == "assistant") | .timestamp as $ts | items
          | select(.type == "tool_use") | . + {timestamp: $ts}] | last) as $tu
      | ([$t[] | select(.type == "assistant") | items | select(.type == "text")
          | .text | strings | clean | select(. != "")] | last) as $said
      | ([$t[] | select(.type == "user" and (.isMeta // false | not)
            and (.message.content | type) == "string")
          | {text: (.message.content | clean), timestamp}
          | select(.text != "")] | last) as $asked
      | (if $tu != null and ($closed | index($tu.id) | not)
          then ["doing", ($tu | tlabel), ($tu | secs | tostring)] | join("\t") else empty end),
        (if $said != null then ["said", $said] | join("\t") else empty end),
        (if $asked != null then ["asked", $asked.text, ($asked | secs | tostring)] | join("\t")
          else empty end)
    end end'

# Run the program over the last 300 lines of <jsonl>; status 1 when it prints nothing.
_tr_run() {                   # <mode> <jsonl-path>
    local out
    command -v jq >/dev/null 2>&1 || return 1
    [ -f "${2:-}" ] || return 1
    out=$(tail -n 300 -- "$2" 2>/dev/null \
          | jq -Rrs --arg mode "$1" "$_TR_JQ" 2>/dev/null) || return 1
    [ -n "$out" ] || return 1
    printf '%s\n' "$out"
}

# True when pid <$1> is <$2> or a descendant of it, walking /proc ppid links.
_tr_descends() {              # <pid> <ancestor-pid>
    local p=$1 stat
    while [ "${p:-0}" -gt 1 ] 2>/dev/null; do
        [ "$p" = "$2" ] && return 0
        stat=$(cat "/proc/$p/stat" 2>/dev/null) || return 1
        stat=${stat##*) }                      # past the command name, which may hold spaces
        read -r _ p _ <<<"$stat"               # state, then the parent pid
    done
    return 1
}

# `claude agents --json` takes about 0.4 s and could hang, so it is bounded to 2 s
# when timeout exists. Only reached when the pane has no @cockpit_session.
_tr_agents() {
    if command -v timeout >/dev/null 2>&1; then timeout 2 claude agents --json 2>/dev/null
    else claude agents --json 2>/dev/null; fi
}

# The transcript of a pane's session: @cockpit_session, else `claude agents --json`
# joined on the pane's process tree. Prints the .jsonl path, or nothing (status 1).
transcript_file() {           # <pane-id>
    local pane=${1:-} sid pp pid f
    command -v jq >/dev/null 2>&1 || return 1
    [ -n "$pane" ] || return 1
    sid=$(tmux show-option -pqv -t "$pane" @cockpit_session 2>/dev/null)
    if [ -z "$sid" ]; then
        pp=$(tmux display-message -p -t "$pane" '#{pane_pid}' 2>/dev/null)
        [ -n "$pp" ] || return 1
        while IFS=$'\t' read -r pid sid; do
            _tr_descends "$pid" "$pp" && break
            sid=
        done < <(_tr_agents \
                 | jq -r '.[] | select((.pid | type) == "number"
                                        and (.sessionId | type) == "string")
                          | [.pid, .sessionId] | join("\t")' 2>/dev/null)
    fi
    # A glob or a path never reaches the match below.
    case $sid in ''|*[!A-Za-z0-9_-]*) return 1 ;; esac
    for f in "$HOME"/.claude/projects/*/"$sid".jsonl; do
        [ -f "$f" ] && { printf '%s\n' "$f"; return 0; }
    done
    return 1
}

# Three rows for prefix i, tab separated, in this order, a row left out when its
# field is unknown:
#   doing<TAB><tool>: <description><TAB><seconds since the tool started, or empty>
#   said<TAB><last assistant text, first line, at most 160 chars>
#   asked<TAB><last string prompt, first line, at most 160 chars><TAB><seconds since>
transcript_doing() {          # <jsonl-path>
    _tr_run doing "${1:-}"
}

# The transcript tail reduced to lines another agent can read, oldest first:
#   USER: <prompt> | AGENT: <text> | TOOL <name>: <description>
# Capped at <max-bytes> (default 4096), cut at a line boundary from the top.
transcript_tail() {           # <jsonl-path> [max-bytes]
    local max=${2:-4096} out lines i keep=0 used=0 n
    case $max in ''|*[!0-9]*) return 1 ;; esac
    out=$(_tr_run tail "${1:-}") || return 1
    mapfile -t lines <<<"$out"
    for ((i = ${#lines[@]} - 1; i >= 0; i--)); do
        n=$(printf '%s\n' "${lines[i]}" | LC_ALL=C wc -c)
        used=$((used + n))
        [ "$used" -le "$max" ] || break
        keep=$((keep + 1))
    done
    [ "$keep" -gt 0 ] || return 1
    printf '%s\n' "${lines[@]: -$keep}"
}
