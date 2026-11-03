#!/usr/bin/env bash
# tmux-review's hand-back: compose the batch, deliver it into the pane, archive it.
# Sourced by tmux-review, never run. Bash 4+.

# After a delivery: the batch joins delivered.md beside the store (the ↺ marks
# of the next round read it), the store and its snapshot go, the dir stays
# while anything is in it -- ^A is what removes it. Public as --done so the
# by-hand re-delivery keep_hint() prints can finish the same way.
done_store() {                # <store>
    local st=$1 d=${1%/*}
    [ "$d" = "$1" ] && d=.
    # The store is removed only once its content is actually IN delivered.md --
    # an append that fails (a full disk, a permissions slip) must not lose the
    # only copy of the review.
    if [ -f "$st" ] && [ ! -L "$st" ] && [ -s "$st" ]; then
        cat -- "$st" >> "$d/delivered.md" || return 1
    fi
    rm -rf -- "$d/history"
    rm -f -- "$st" "$st.rows" "$st.rdin" "$st.rdout" "$st.base"
    rmdir -- "$d" 2>/dev/null
    return 0
}

# What ^A ends: the session dir's own files, by name. Not `rm -rf` -- the dir
# may also hold comments.md.deliver, a review already handed to a busy pane
# with @cockpit_review_spool armed at it, and that one belongs to the agent's
# next Stop, not to this approval. rmdir therefore succeeds on an approval with
# nothing in flight and fails (harmlessly) on one with a spool still waiting;
# review-gate rmdirs the leftover after it drains. Two callers, ^A in the
# picker and ^A in --request, and the list must not drift between them.
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
# The header (ai lens §7.F: scope fence, questions first, dissent licence,
# trust the fragment, a close-out line the next review can key on), then the
# store verbatim -- cat, nothing clever.
compose() {
    local store=${1:-} root=${2:-}
    printf 'Code review of your changes, from the human, batched on purpose.\n\n'
    printf 'Scope: change only what these comments ask for. Do not refactor, rename,\ntidy or commit anything else — a fix that grows the diff defeats the review.\n'
    printf 'Questions first: a comment containing ?? is a question. Answer every question\nin chat before you edit anything, then make the edits in one pass.\n'
    printf 'Disagree out loud: if a comment is wrong or would break something, say so and\ndo not make the change. Silent compliance with a wrong comment is the worse\nfailure.\n'
    printf 'Anchors: comments are path:line with the original line quoted. The numbers are\nfrom the diff as it was when the review was written and drift after your first\nedit — trust the quoted fragment, not the number.\n'
    # root is the repo root a comment's path is relative to -- optional
    # (omitted, --compose still reads standalone), named here because
    # deliver() has it and the agent otherwise has no way to tell.
    [ -n "$root" ] && printf 'Paths are relative to %s.\n' "$root"
    printf 'When done, end your reply with one line per comment:\n  <path>:<line> — done | skipped: <reason>\n\n'
    [ -n "$store" ] && [ -f "$store" ] && cat -- "$store"
    return 0
}

# --- --deliver -------------------------------------------------------------
# Land the composed review typed-but-unsent in the agent's own pane -- one
# bracketed paste, never an Enter. Live-verified on a private tmux 3.7c
# server (pane running `exec -a claude bash`): load-buffer + `paste-buffer -p
# -r -d` puts the whole multi-line payload into bash's readline buffer,
# UNEXECUTED -- visible across every capture-pane as the same still-open
# prompt line, no "command not found", no fresh prompt underneath. Tested
# with AND without `-r`, with AND without `extended-keys-format csi-u` (the
# #43169 mitigation this flag targets), and with AND without a trailing
# newline on the payload: all five combinations behaved identically and
# safely on this stack. -r ships anyway per the brief -- LF, never CR, is the
# documented-safe shape even where this local stack did not need it.
#
# Guards run in order, each a refusal via display-message (2>/dev/null: no
# attached client is not this call's problem to fail loudly over) rather
# than a paste:
#   1. pane id shape (agent-state:13-16) -- tmux prefix-matches -t against
#      session names too, so a stray non-%N argument could resolve onto
#      someone else's pane entirely.
#   2. pane_current_command against @cockpit_agents_glob (cockpit.tmux's own
#      #{m:*|cmd|*,glob} shape, reused verbatim from tmux-jump/tm) -- a shell
#      pane would EXECUTE review text instead of merely displaying it.
#   3. (no longer a refusal) a pane that is running or on a permission
#      prompt gets the review QUEUED: the composed text is spooled beside
#      the store and @cockpit_review_spool names it; review-gate hands it
#      over at the agent's next Stop. Nothing is ever pasted into a dialog.
#   4. store non-empty -- nothing pinned means nothing to deliver.
#
# Every refusal names the store. review_pane() deletes it only when delivery
# SUCCEEDS, so after a refusal the pinned batch is still on disk -- and it is
# the only copy of the review. A refusal that did not say so was
# indistinguishable from one that ate it.
keep_hint() {
    local pane=$1 store=$2
    [ -n "$store" ] && [ -s "$store" ] || return 0
    # the loop giving up has no pane in hand -- it was handed a dir and a store
    # and nothing else -- so the id becomes a visible placeholder rather than a
    # silently missing argument in a command the reader would paste as-is.
    [ -n "$pane" ] || pane='<pane>'

    # $SELF, not a bare "tmux-review": the name is only on $PATH after
    # install.sh symlinks it, and from a plugin-only install the suggested
    # command has to be copy-pasteable as printed.
    printf '; comments kept at %s — deliver later with: %s --deliver %s %s && %s --done %s' \
        "$store" "$(shq "$SELF")" "$pane" "$(shq "$store")" "$(shq "$SELF")" "$(shq "$store")"
}

deliver() {
    local pane=$1 store=$2 root=$3

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

    # The spool: the composed review as a file, born 0600 beside the store. On
    # the paste path it is the Stop hook's copy if the pane turns out busy at
    # paste time; on the hook path it is the whole delivery (review-gate hands the
    # file's text to the agent as Stop hook feedback, and the harness saves any
    # output past 10,000 characters to a file with a preview and the path, so the
    # 2048-byte pointer below is paste-only).
    local spool="$store.deliver"
    if ! (umask 077; [ ! -L "$spool" ] && printf '%s' "$payload" > "$spool"); then
        tmux display-message \
            "$(fesc "cockpit: could not write the review spool, refusing to deliver$(keep_hint "$pane" "$store")")" \
            2>/dev/null
        return 1
    fi

    bytes=$(printf '%s' "$payload" | wc -c)

    # Over 2048 bytes: the review becomes a file the agent reads, not a wall
    # of text typed into its prompt. mktemp, not a predictable
    # tmux-cockpit-review-<pane>.md name -- a fixed name under a shared
    # tmpdir (the COMMON case on macOS, which has no XDG_RUNTIME_DIR) is a
    # symlink-disclosure vector, and mktemp's own randomized suffix makes the
    # name unguessable rather than merely unlinked-and-recreated. `umask 077`
    # in the same subshell that runs mktemp means the file is BORN 0600 --
    # no create-then-chmod window for a racing reader. Template ends in the
    # X run with no trailing .md: BSD/macOS mktemp requires the X's to be
    # the LAST characters of the template and errors ("too few X's") on
    # anything after them, unlike GNU's more permissive "keep a literal
    # suffix after the trailing X run" -- the pointer sentence below already
    # names the exact path, so the extension carried no weight anyway. The
    # agent deletes nothing -- the file outlives the pane on purpose.
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

    # A per-invocation buffer name: tmux buffers are SERVER-GLOBAL, so a
    # fixed name races under concurrent deliveries -- a second delivery's
    # load-buffer can overwrite the first's payload before the first's
    # paste-buffer -d has run, landing the wrong review in the wrong pane (or
    # pasting nothing once the buffer was already deleted out from under
    # it). pane + $$ gives every invocation its own buffer.
    local bufname
    bufname="cockpit-review-${pane#%}-$$"
    if ! printf '%s' "$payload" | tmux load-buffer -b "$bufname" -; then
        tmux display-message \
            "cockpit: could not stage the review, refusing to deliver" \
            2>/dev/null
        return 1
    fi
    # The is-agent test the shell ran up top is a ~50ms-stale snapshot: a
    # compose re-exec, a wc, an mktemp, a spool write and this load-buffer all
    # sit between it and the paste. Kill the agent in that window -- a `claude`
    # foreground child exits and the pane's own shell is back -- and the old
    # unconditional `paste-buffer` landed the review in that shell (the hunt
    # reproduced it 6/6, pasting a live, one-Enter-from-running command line
    # the reviewer never targeted). So the check is redone HERE, by tmux
    # itself, in the same command-queue item as the paste: `if-shell -F -t
    # <pane>` re-evaluates the SAME #{m:...} test -- AND whether @panestate is
    # running or needs-input, which has the identical staleness window: a
    # permission prompt (or a tool starting to run) can come up in it -- against
    # the pane at paste time, and only pastes if it is still an idle agent;
    # otherwise it takes the queued branch below instead of the paste. The
    # window is closed -- there is no shell round-trip left between the test
    # and the paste. The early check up top stays as the cheap fast-fail for a
    # pane that is already not an agent.
    #
    # Both arms consume $bufname themselves -- the idle arm's `paste-buffer -d`,
    # the busy arm's `delete-buffer` -- because if-shell's nested command has
    # already run by the time this CLI call returns (same command-queue item,
    # no -b job behind it), so the buffer's survival is only possible if tmux
    # never ran either arm at all (the pane vanished between load-buffer and
    # if-shell). The post-if-shell check below exists for that gap, not to
    # distinguish paste from queue -- both arms are exit-0 successes and both
    # report themselves via their own display-message.

    # Where the review landed, named the way a human navigates: "work:2 api"
    # beats a bare %7 when the pane is three windows away. Read once, HERE,
    # against the TARGET pane: the notice's own display-message carries no -t
    # (it must not steal focus or fail when the pane is gone by then), so a
    # #{window_name} left in the message would resolve against whatever pane
    # the reader happens to be sitting in -- the wrong window's name.
    #
    # The window name is USER DATA and gets two escapes, because it crosses two
    # parsers on its way into the notice:
    #
    #   1. tmux's command-string parser. $where is interpolated into a
    #      single-quoted argument of the if-shell command string, so a window
    #      named don't closes that quote early and hands tmux the remainder as
    #      commands. Single quotes are stripped; nothing else breaks out.
    #   2. tmux's FORMAT parser. display-message expands its message as a
    #      format, so a window named w#{@some_option} renders that option's
    #      VALUE, not the name -- measured on 3.7c: a window renamed
    #      w##{@cockpit_secret} made the notice read "wLEAKED-SECRETz". ## is
    #      the format literal for #, so doubling every # is the whole fix, and
    #      it works on the 3.3 floor. (3.7c runs no #(shell-command) job from a
    #      display-message -- only status formats do -- but doubling covers
    #      that arm too, whatever a later tmux decides to run.)
    #
    # The READ above is safe on its own: a value substituted by #{window_name}
    # is not re-expanded, so only this second interpolation needs the escape.
    local where
    where=$(tmux display-message -p -t "$pane" \
        '#{session_name}:#{window_index} #{window_name}' 2>/dev/null)
    where=${where//\'/}
    where=$(fesc "$where")
    [ -n "$where" ] || where="the agent pane"   # display-message failed; still read as a sentence

    # Armed FIRST, disarmed by the idle arm below: the busy/idle choice is made
    # by tmux inside one command-queue item, and arming ahead of it means the
    # spool is never armed on a pane that just took the paste, and never
    # missing on one that did not. A pane that stopped being an agent inside
    # the ~50ms window keeps a harmless armed option that dies with it --
    # review-gate only ever runs from an agent's own hook.
    tmux set-option -p -t "$pane" @cockpit_review_spool "$spool" 2>/dev/null

    # The jump key is read, never spelled: it has been u since #43 and this
    # notice still said J (#105 defect 1). `off` unbinds the key, so that arm
    # drops the whole clause rather than pointing at nothing. Both notices are
    # inside the 72 columns an 80-column client draws, which is what cost the
    # pane id here -- $where already names the session, window and its name.
    local jump='#{?#{==:#{@cockpit_jump_key},off},, · prefix #{?#{@cockpit_jump_key},#{@cockpit_jump_key},u} jumps}'
    local notice_paste="cockpit: review pasted into $where · ↵ sends it$jump"
    local notice_queue="cockpit: review queued for $where · it lands when the agent stops"
    tmux if-shell -F -t "$pane" \
        "#{&&:#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{&&:#{!=:#{@panestate},running},#{!=:#{@panestate},needs-input}}}" \
        "set-option -pu -t $pane @cockpit_review_spool ; set-option -p -t $pane @cockpit_review sent ; paste-buffer -p -r -d -b $bufname -t $pane ; display-message -d 0 '$notice_paste'" \
        "delete-buffer -b $bufname ; display-message -d 0 '$notice_queue'"

    # The idle arm disarms the option as it pastes, so an empty option here
    # means the paste happened and nobody will ever read this spool -- the
    # queued arm is the only one that leaves it armed. Without this every
    # pasted review left a .deliver file behind in the session dir, and
    # done_store's rmdir then found the dir non-empty and left that too.
    [ -n "$(tmux show-option -pqv -t "$pane" @cockpit_review_spool 2>/dev/null)" ] || rm -f -- "$spool"

    if tmux list-buffers -F '#{buffer_name}' 2>/dev/null | grep -qxF "$bufname"; then
        tmux delete-buffer -b "$bufname" 2>/dev/null
        return 1
    fi
    return 0
}
