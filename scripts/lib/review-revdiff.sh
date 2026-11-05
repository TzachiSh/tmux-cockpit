#!/usr/bin/env bash
# tmux-review's review loop: revdiff reads and comments; this launches it,
# ingests what it wrote, and asks what next. Sourced by tmux-review. Bash 4+.
# Exit codes, the contract with review_pane and --request: 20 deliver,
# 21 approved, 125 refused by git.sh (#330), 126 revdiff failed (stderr in
# <session dir>/revdiff.err), 127 no revdiff, anything else leave.

# The line as the tree (+, context) or the base rev (-) has it, control
# characters out, 1000 bytes at most.
read_line() {                 # <dir> <path> <line> <sign> [base-rev]
    local line
    case "$3" in ''|*[!0-9]*) return 0 ;; esac   # anything else is a sed script
    if [ "$4" = "-" ]; then
        line=$(git -C "$1" show "${5:-HEAD}:$2" 2>/dev/null | sed -n "${3}p")
    else
        line=$(sed -n "${3}p" -- "$1/$2" 2>/dev/null)
    fi
    line=${line//[$'\001'-$'\037'$'\177']/}
    printf '%s\n' "${line:0:1000}"
}

# A pane id means nothing to the person reading the prompt; the agent's own
# title (claude sets it) or the window name does, with the id kept.
pane_label() {                # <pane-id>
    local t
    t=$(tmux display-message -p -t "$1" "$AGENT_TITLE_FMT" 2>/dev/null)
    printf '%s\n' "${t:+$t }($1)"
}

# The scratch beside a store, by name: the dir may also hold a queued delivery.
drop_rows() {
    [ -n "${1:-}" ] || return 0
    rm -rf -- "$1.rows" "$1.rdin" "$1.rdout" "${1%/*}/history"
}

drop_store() {
    [ -n "${1:-}" ] || return 0
    drop_rows "$1"
    rm -f -- "$1" "$1.base"
}

# revdiff's output into the store. Its records carry no fragment: each keeps the
# quote the store had for that anchor, else the line as it is now. A file that
# does not parse changes nothing.
ingest_revdiff() {            # <dir> <store> <revdiff-output> <base-rev>
    local dir=$1 store=$2 out=$3 rev=$4 i k frag
    declare -A STORE=() OLDF=(); STORE_N=0
    parse_store "$store"
    for ((i = 0; i < STORE_N; i++)); do
        [ -n "${STORE[$i.frag]+x}" ] || continue
        OLDF["${STORE[$i.path]}:${STORE[$i.line]:-}:${STORE[$i.side]}"]=${STORE[$i.frag]}
    done
    STORE=(); STORE_N=0
    parse_store_revdiff "$out"
    if [ -n "$STORE_BAK" ]; then
        rm -f -- "$STORE_BAK"
        tmux display-message \
            "cockpit: revdiff wrote something unreadable, the store is unchanged" 2>/dev/null
        return 1
    fi
    for ((i = 0; i < STORE_N; i++)); do
        [ "${STORE[$i.side]}" != file-level ] || continue
        k="${STORE[$i.path]}:${STORE[$i.line]}:${STORE[$i.side]}"
        if [ -n "${OLDF[$k]+x}" ]; then frag=${OLDF[$k]}
        else frag=$(read_line "$dir" "${STORE[$i.path]}" "${STORE[$i.line]}" \
                              "${STORE[$i.side]}" "$rev")
        fi
        STORE[$i.frag]=$frag
    done
    [ ! -L "$store" ] || {
        tmux display-message "cockpit: ${store##*/} is a symlink, refusing to write" 2>/dev/null
        return 1
    }
    format_store "$store"
}

# The store as revdiff's --annotations wants it: the same records, no quote
# lines (revdiff would keep them as comment text and hand them back).
write_preload() {             # <store> <preload-file>
    local i
    declare -A STORE=(); STORE_N=0
    parse_store "$1"
    for ((i = 0; i < STORE_N; i++)); do unset "STORE[$i.frag]"; done
    rm -f -- "$2"
    (umask 077; set -C; : > "$2") 2>/dev/null || return 1
    format_store "$2"
}

# One key from the human; EOF (no terminal left) reads as "l", leave.
ask_key() {
    local k
    IFS= read -rsn1 k || k=l
    printf '%s' "$k"
}

review_loop() {
    local dir=$1 store=$2
    [ -n "$dir" ] && [ -n "$store" ] || { usage; exit 2; }
    command -v revdiff >/dev/null 2>&1 || {
        # display-message too: under display-popup -E, stderr of a command that
        # exits at once reaches nobody.
        tmux display-message "cockpit: revdiff not found: install it, see #233" 2>/dev/null
        printf 'revdiff not found: install it, see #233\n' >&2
        exit 127
    }
    # shellcheck source=lib/review-store.sh
    . "$LIBDIR/review-store.sh" || exit 1

    # 125: refused, so --request can tell it from "reviewed, no comments" (0). A pinned batch stays.
    if git_refused "$dir"; then
        tmux display-message "$(fesc "$GIT_REFUSAL")" 2>/dev/null
        printf '%s\n' "$GIT_REFUSAL" >&2
        [ -s "$store" ] || drop_store "$store"
        exit 125
    fi

    # The base chosen with b, kept beside the store so a resumed review keeps it.
    local base_line label mode nxt rev
    COCKPIT_REVIEW_BASE=$(cat -- "$store.base" 2>/dev/null); export COCKPIT_REVIEW_BASE
    set_base_line() {
        if git -C "$dir" --no-optional-locks rev-parse -q --verify HEAD >/dev/null 2>&1; then
            base_line=$(review_base "$dir" "${COCKPIT_REVIEW_BASE:-auto}")
            label=$(cut -f2 <<<"$base_line"); mode=$(cut -f3 <<<"$base_line")
            rev=${base_line%%$'\t'*}
            # With one base holding, next_base wraps back to this mode.
            nxt=$(next_base "$dir" "$mode"); [ "$nxt" = "$mode" ] && nxt=""
        else
            label=uncommitted; mode=uncommitted; rev=HEAD; nxt=""
        fi
    }
    set_base_line

    if [ -z "$(tree_diff "$dir" --numstat)" ]; then
        tmux display-message "$(fesc "cockpit: no uncommitted changes in $dir")" 2>/dev/null
        drop_store "$store"
        exit 0
    fi
    if [ -s "$store" ]; then
        tmux display-message \
            "cockpit: resumed $(grep -c '^## ' -- "$store") pinned comment(s)" 2>/dev/null
    fi

    local rc n key choice to errf="${store%/*}/revdiff.err"
    local -a args
    while :; do
        # The default view is the uncommitted one; any other base is a ref.
        args=(--untracked --exit-code-on-annotations -o "$store.rdout"
              --history-dir "${store%/*}/history")
        write_preload "$store" "$store.rdin" && args+=(--annotations "$store.rdin")
        [ "$mode" = uncommitted ] || args+=("$rev")
        rm -f -- "$store.rdout"
        (cd "$dir" && revdiff "${args[@]}" 2>"$errf")
        rc=$?
        case "$rc" in
            10) if [ -f "$store.rdout" ] && [ ! -L "$store.rdout" ]; then
                    ingest_revdiff "$dir" "$store" "$store.rdout" "$rev"
                fi ;;
            0)  ;;                      # no comments, or Q: nothing changes
            *)  # Kept, for --request to print once the popup is gone.
                tmux display-message \
                    "$(fesc "cockpit: revdiff exited $rc: $(head -n 1 -- "$errf" 2>/dev/null)")" 2>/dev/null
                printf 'revdiff exited %s\n' "$rc" >&2; cat -- "$errf" >&2 2>/dev/null
                drop_rows "$store"; exit 126 ;;
        esac
        rm -f -- "$errf"
        rm -f -- "$store.rdout" "$store.rdin"

        while :; do
            n=0; [ -s "$store" ] && n=$(grep -c '^## ' -- "$store")
            to="the agent"; [ -n "${REVIEW_PANE_ID:-}" ] && to=$(pane_label "$REVIEW_PANE_ID")
            printf '\n review · base %s\n' "$label"
            if [ "$n" -gt 0 ]; then
                printf ' %d comment(s) pinned\n d deliver to %s · r review again' "$n" "$to"
                printf '%s · x discard · l leave\n' "${nxt:+ · b base → $nxt}"
            else
                printf ' no comments\n a approve · r review again'
                printf '%s · any other key leaves\n' "${nxt:+ · b base → $nxt}"
            fi
            key=$(ask_key)
            printf '\n'
            case "$key" in
                d) [ "$n" -gt 0 ] || continue
                   drop_rows "$store"; exit 20 ;;
                a) [ "$n" -eq 0 ] || continue
                   git -C "$dir" --no-optional-locks rev-parse -q --verify HEAD >/dev/null 2>&1 || {
                       tmux display-message \
                           "cockpit: nothing to approve before the first commit" 2>/dev/null
                       continue; }
                   if approve "$dir"; then drop_store "$store"; exit 21; fi
                   tmux display-message \
                       "cockpit: could not mark the branch reviewed" 2>/dev/null
                   continue ;;
                r) break ;;
                b) [ -n "$nxt" ] || continue
                   printf '%s\n' "$nxt" > "$store.base"
                   COCKPIT_REVIEW_BASE=$nxt; export COCKPIT_REVIEW_BASE
                   set_base_line
                   [ -n "$(tree_diff "$dir" --numstat)" ] || \
                       tmux display-message "$(fesc "cockpit: nothing to review against $label")" 2>/dev/null
                   break ;;
                x) [ "$n" -gt 0 ] || continue
                   printf ' discard %d comment(s)? y/N ' "$n"
                   choice=$(ask_key); printf '\n'
                   [ "$choice" = y ] || continue
                   drop_store "$store"; exit 0 ;;
                *) drop_rows "$store"; exit 0 ;;   # leave: the store stays; prefix r resumes it
            esac
        done
    done
}
