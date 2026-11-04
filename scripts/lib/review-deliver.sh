#!/usr/bin/env bash
# tmux-review's hand-back: compose the batch, deliver it into the pane, archive it.
# Sourced by tmux-review, never run. Bash 4+.

# After a delivery: the batch joins delivered.md (the next round's ↺ marks read
# it), the store and its snapshot go. Public as --done for keep_hint's command.
done_store() {                # <store>
    local st=$1 d=${1%/*}
    [ "$d" = "$1" ] && d=.
    # The store goes only once it is in delivered.md: a failed append must not
    # lose the only copy of the review.
    if [ -f "$st" ] && [ ! -L "$st" ] && [ -s "$st" ]; then
        cat -- "$st" >> "$d/delivered.md" || return 1
    fi
    rm -rf -- "$d/history"
    rm -f -- "$st" "$st.rows" "$st.rdin" "$st.rdout" "$st.base"
    rmdir -- "$d" 2>/dev/null
    return 0
}

# What ^A ends: the session dir's files by name, not `rm -rf`. The dir may hold
# comments.md.deliver, a review queued for a busy pane's next Stop
# (@cockpit_review_spool); review-gate rmdirs the dir after it drains.
clear_sess() {                # <sess dir>
    local s=$1
    [ -d "$s" ] && [ ! -L "$s" ] || return 0
    rm -rf -- "$s/history"
    rm -f -- "$s/comments.md" "$s/comments.md.rows" "$s/comments.md.rdin" \
             "$s/comments.md.rdout" "$s/comments.md.base" "$s/delivered.md" "$s/rc"
    rmdir -- "$s" 2>/dev/null
    return 0
}

# --- --compose ---------------------------------------------------------------
# The header (scope fence, questions first, dissent licence, trust the fragment,
# a close-out line the next review can key on), then the store verbatim.
compose() {
    local store=${1:-} root=${2:-}
    printf 'Code review of your changes, from the human, batched on purpose.\n\n'
    printf 'Scope: change only what these comments ask for. Do not refactor, rename,\ntidy or commit anything else — a fix that grows the diff defeats the review.\n'
    printf 'Questions first: a comment containing ?? is a question. Answer every question\nin chat before you edit anything, then make the edits in one pass.\n'
    printf 'Disagree out loud: if a comment is wrong or would break something, say so and\ndo not make the change. Silent compliance with a wrong comment is the worse\nfailure.\n'
    printf 'Anchors: comments are path:line with the original line quoted. The numbers are\nfrom the diff as it was when the review was written and drift after your first\nedit — trust the quoted fragment, not the number.\n'
    [ -n "$root" ] && printf 'Paths are relative to %s.\n' "$root"
    printf 'When done, end your reply with one line per comment:\n  <path>:<line> — done | skipped: <reason>\n\n'
    [ -n "$store" ] && [ -f "$store" ] && cat -- "$store"
    return 0
}

# --- --deliver -------------------------------------------------------------
# The review lands typed-but-unsent in the agent's pane: one bracketed paste
# (`paste-buffer -p -r`, LF not CR), never an Enter. A pane that is not an agent
# is refused: a shell would execute the text. A running or prompting agent gets
# it queued (the spool) for review-gate to hand over at its next Stop.
# Every refusal names the store: after one, it is the only copy of the review.
keep_hint() {
    local pane=$1 store=$2
    [ -n "$store" ] && [ -s "$store" ] || return 0
    # A visible placeholder, not a missing argument in a command meant for pasting.
    [ -n "$pane" ] || pane='<pane>'
    # $SELF, not "tmux-review": that is on $PATH only after install.sh.
    printf '; comments kept at %s — deliver later with: %s --deliver %s %s && %s --done %s' \
        "$store" "$(shq "$SELF")" "$pane" "$(shq "$store")" "$(shq "$SELF")" "$(shq "$store")"
}

deliver() {
    local pane=$1 store=$2 root=$3

    # tmux prefix-matches -t against session names: a non-%N id could resolve
    # onto someone else's pane.
    is_pane "$pane" || {
        tmux display-message \
            "$(fesc "cockpit: not a pane id, refusing to deliver$(keep_hint '' "$store")")" \
            2>/dev/null; return 1; }

    local is_agent
    is_agent=$(tmux display-message -p -t "$pane" \
        '#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},1,0}' \
        2>/dev/null)
    [ "$is_agent" = 1 ] || {
        tmux display-message \
            "$(fesc "cockpit: $pane is not an agent pane, refusing to deliver$(keep_hint "$pane" "$store")")" \
            2>/dev/null
        return 1
    }

    [ -n "$store" ] && [ -s "$store" ] || {
        tmux display-message "cockpit: nothing to deliver" 2>/dev/null
        return 1
    }

    local payload bytes
    payload=$("$SELF" --compose "$store" "$root")

    # The spool, born 0600: the Stop hook's copy if the pane is busy at paste
    # time. The hook path hands over the whole text, so the 2048-byte pointer
    # below is paste-only (the harness files hook output past 10,000 chars).
    local spool="$store.deliver"
    if ! (umask 077; [ ! -L "$spool" ] && printf '%s' "$payload" > "$spool"); then
        tmux display-message \
            "$(fesc "cockpit: could not write the review spool, refusing to deliver$(keep_hint "$pane" "$store")")" \
            2>/dev/null
        return 1
    fi

    bytes=$(printf '%s' "$payload" | wc -c)

    # Over 2048 bytes the review becomes a file the agent reads. mktemp under
    # umask 077: a fixed name in a shared tmpdir (macOS has no XDG_RUNTIME_DIR)
    # is a symlink-disclosure vector. The X run ends the template: BSD mktemp
    # refuses anything after it.
    if [ "$bytes" -gt 2048 ]; then
        local runtime file
        runtime=${XDG_RUNTIME_DIR:-/tmp}
        file=$(umask 077 && mktemp "$runtime/tmux-cockpit-review-${pane#%}-XXXXXX") || {
            tmux display-message \
                "$(fesc "cockpit: could not create the review file, refusing to deliver$(keep_hint "$pane" "$store")")" \
                2>/dev/null
            return 1
        }
        printf '%s' "$payload" > "$file"
        # Nothing else deletes these. Keep this pane's two newest older ones: an
        # earlier pointer may still sit unsent in the prompt.
        local f i; local -a by=()
        for f in "$runtime/tmux-cockpit-review-${pane#%}-"??????; do
            [ "$f" != "$file" ] && [ -f "$f" ] && [ ! -L "$f" ] && [ -O "$f" ] || continue
            for ((i = ${#by[@]}; i > 0; i--)); do [ "$f" -nt "${by[i-1]}" ] || break; by[i]=${by[i-1]}; done
            by[i]=$f
        done
        [ "${#by[@]}" -le 2 ] || rm -f -- "${by[@]:2}"
        payload="Read and address every comment in $file — a code review from the human; ?? comments are questions."
    fi

    # Buffers are server-global: a fixed name races between two deliveries.
    local bufname
    bufname="cockpit-review-${pane#%}-$$"
    if ! printf '%s' "$payload" | tmux load-buffer -b "$bufname" -; then
        tmux display-message \
            "cockpit: could not stage the review, refusing to deliver" \
            2>/dev/null
        return 1
    fi
    # The is-agent test above is ~50 ms stale by now; a killed agent left its
    # shell to take the paste. So tmux re-tests it, and @panestate, in the same
    # command-queue item as the paste (if-shell -F). Both arms consume $bufname;
    # it survives only if tmux ran neither (the pane vanished), checked below.

    # Where it landed, read now against the target: the notice carries no -t, so
    # a #{window_name} left in it would name the reader's window. The name is
    # user data crossing two parsers: single quotes would close the if-shell
    # argument (stripped), and # is a format (doubled by fesc; on 3.7c
    # w##{@secret} rendered the option's value).
    local where
    where=$(tmux display-message -p -t "$pane" \
        '#{session_name}:#{window_index} #{window_name}' 2>/dev/null)
    where=${where//\'/}
    where=$(fesc "$where")
    [ -n "$where" ] || where="the agent pane"   # display-message failed; still read as a sentence

    # Armed first, disarmed by the idle arm: never armed on a pane that took the
    # paste, never missing on one that did not.
    tmux set-option -p -t "$pane" @cockpit_review_spool "$spool" 2>/dev/null

    # The jump key is read, never spelled (#105); `off` drops the clause. Both
    # notices fit the 72 columns an 80-column client draws.
    local jump='#{?#{==:#{@cockpit_jump_key},off},, · prefix #{?#{@cockpit_jump_key},#{@cockpit_jump_key},u} jumps}'
    local notice_paste="cockpit: review pasted into $where · ↵ sends it$jump"
    local notice_queue="cockpit: review queued for $where · it lands when the agent stops"
    tmux if-shell -F -t "$pane" \
        "#{&&:#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{&&:#{!=:#{@panestate},running},#{!=:#{@panestate},needs-input}}}" \
        "set-option -pu -t $pane @cockpit_review_spool ; set-option -p -t $pane @cockpit_review sent ; paste-buffer -p -r -d -b $bufname -t $pane ; display-message -d 0 '$notice_paste'" \
        "delete-buffer -b $bufname ; display-message -d 0 '$notice_queue'"

    # An empty option means the paste happened: nobody will read the spool.
    [ -n "$(tmux show-option -pqv -t "$pane" @cockpit_review_spool 2>/dev/null)" ] || rm -f -- "$spool"

    if tmux list-buffers -F '#{buffer_name}' 2>/dev/null | grep -qxF "$bufname"; then
        tmux delete-buffer -b "$bufname" 2>/dev/null
        return 1
    fi
    return 0
}
