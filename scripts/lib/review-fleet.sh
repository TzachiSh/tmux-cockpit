#!/usr/bin/env bash
# tmux-review's review entry (what r on a board row runs) and --request.
# Sourced by tmux-review, never run. Bash 4+.

# --- entry-point resolution -------------------------------------------------
# review_pane() drives the interactive loop against one pane: a session-dir
# store (session_dir() in tmux-review), the interactive --loop re-exec'd (not
# called as a function -- it ends in a script `exit`, and this needs its own
# exit code back rather than the whole process dying), rc 20 (deliver) handed
# to --deliver via `tmux run-shell -b` -- a popup kills its own children the
# moment it closes, so delivery has to outlive that teardown, not run inside
# it.

review_pane() {
    local pane=$1 dir=$2 store rc
    local sess cleanup
    sess=$(session_dir "$pane" "$dir"); store="$sess/comments.md"
    # umask in the subshell: mkdir -p creates the parents at the caller's
    # umask otherwise, and the store is born 0600 the same way.
    # [ ! -L ]: the same discipline snapshot() uses on "$2.rows" -- a store
    # path is derivable (session_dir() is a pane id and a root's cksum), so a
    # planted symlink must be refused here too, not just followed.
    (umask 077; mkdir -p -- "$sess" && [ ! -L "$store" ] && : >> "$store") || {
        tmux display-message "$(fesc "cockpit: could not create $sess")" 2>/dev/null
        return 1
    }
    REVIEW_PANE_ID=$pane "$SELF" --loop "$dir" "$store"
    rc=$?
    if [ "$rc" -eq 20 ]; then
        # deliver() is a public subcommand the suite re-delivers the same
        # store through, so it must not rm its own store arg -- the cleanup
        # belongs to this caller, chained after delivery so the store still
        # outlives the popup's own teardown (comment above).
        #
        # The STORE is removed only on success: --deliver refuses (needs-input,
        # a non-agent pane, a bogus id) AFTER the popup is gone, and an
        # unconditional rm threw the whole pinned batch away on every one of
        # those -- the store is the only copy of the review, and the human
        # never even saw it happen. A refused store is left on disk on purpose;
        # deliver()'s message names it and the command that re-delivers it.
        #
        # The SNAPSHOT is removed either way -- it served the popup, and the
        # popup is already gone. The DIR follows the store: removed once it is
        # empty (a successful delivery), left standing -- named -- when the
        # store itself is kept.
        #
        # run-shell hands this to `sh -c`, so the exit code is stashed in $rc
        # and tested with a plain `if`; the `\$` keep both out of the reach of
        # the bash expansion that is building the string. Every path is still
        # shq-quoted.
        cleanup="rc=\$?; if [ \$rc -eq 0 ]; then $(shq "$SELF") --done $(shq "$store");"
        cleanup="$cleanup else rm -f -- $(shq "$store.rows"); fi"
        tmux run-shell -b "$(fesc "$(shq "$SELF") --deliver $(shq "$pane") $(shq "$store") \
$(shq "$dir"); $cleanup")"
    elif [ "$rc" -eq 21 ]; then
        # Approved: the ref is written (the loop did it, in the popup); the
        # pane's attention state and the review dir end here -- by NAME, not
        # rm -rf. A review already delivered to a busy pane is in flight in
        # comments.md.deliver with @cockpit_review_spool armed at it, and ^A is
        # allowed then (the pinned-comments guard sees an empty store), so the
        # recursive remove took the queued review, dangled the option at a path
        # that no longer existed, and took delivered.md with it. An armed spool
        # keeps the dir standing; review-gate's own rmdir finishes it after the
        # drain.
        tmux set-option -pu -t "$pane" @cockpit_review 2>/dev/null
        "$(dirname "$SELF")/agent-state" recount "$pane"
        clear_sess "$sess"
        tmux display-message \
            "cockpit: review approved · the next one shows only what changed" 2>/dev/null
    else
        rm -f -- "$sess/revdiff.err"
        rmdir -- "$sess" 2>/dev/null   # empty after a discard; a kept store keeps it
    fi
}

# --- --request: the agent asks ----------------------------------------------
# The blocking path (ai lens §7.C, revdiff's contract): run from the agent's
# own Bash tool by the cockpit-review skill, this opens ONE popup on the pane,
# waits for the human, and hands the batch back as stdout + exit 10 -- the
# tool result -- instead of a paste. Nothing else changes: the same session
# dir, the same loop, ^A and esc mean what they always mean.
#
# What comes back is decided by the LOOP's exit code, not by the store: esc
# now offers "leave" (the human said "not now"), which keeps the batch pinned
# and non-empty, and only d (rc 20) is a delivery. display-popup does not
# return the command's status -- it returns tmux's own -- so the popup command
# records it in <sess>/rc and this reads it back. A dropped launcher (the tool
# timed out) leaves the popup, the store and the rc file alone; running it
# again resumes the pinned comments, so the recovery is "run it again".
request() {                   # <pane>
    local pane=$1 dir root sess store pw ph rc t cmd
    is_pane "$pane" || { usage; exit 2; }
    if [ "$(tmux display-message -p -t "$pane" '#{session_attached}' 2>/dev/null)" = 0 ]; then
        printf 'tmux-review: no client is attached to %s'"'"'s session — nowhere to open the review\n' \
            "$pane" >&2
        exit 1
    fi
    dir=$(pane_dir "$pane")
    [ -n "$dir" ] || { printf 'tmux-review: pane %s is gone\n' "$pane" >&2; exit 1; }
    root=$(git_root "$dir"); sess=$(session_dir "$pane" "$root"); store="$sess/comments.md"
    (umask 077; mkdir -p -- "$sess" && [ ! -L "$store" ] && : >> "$store") || {
        printf 'tmux-review: could not create %s\n' "$sess" >&2; exit 1; }
    pw=$(tmux show-option -gqv @cockpit_popup_width);  pw=${pw:-70%}
    ph=$(tmux show-option -gqv @cockpit_popup_height); ph=${ph:-70%}
    # A killed launcher leaves its popup running, and that popup still writes
    # its rc when the human closes it -- so clear the file BEFORE opening this
    # one: an rc read here must belong to the popup this call just waited on.
    rm -f -- "$sess/rc"
    # -E: this returns when the loop exits. display-popup runs the string
    # through sh, so the escaped \$? is the LOOP's code, not tmux's.
    # -T is a tmux format: a # in the path would be expanded. The command
    # itself is not (measured on 3.8), so it is only shell-quoted.
    t=${root/#"$HOME/"/\~/}
    cmd="REVIEW_PANE_ID=$(shq "$pane") $(shq "$SELF") --loop $(shq "$root") $(shq "$store");"
    cmd="$cmd echo \$? > $(shq "$sess/rc")"
    tmux display-popup -E -t "$pane" -w "$pw" -h "$ph" \
        -T "$(fesc " review · requested · $t ")" "$cmd" 2>/dev/null
    rc=$(cat -- "$sess/rc" 2>/dev/null); rm -f -- "$sess/rc"
    case "$rc" in
        126) printf 'tmux-review: revdiff failed\n' >&2
             cat -- "$sess/revdiff.err" >&2 2>/dev/null; rm -f -- "$sess/revdiff.err"
             exit 1 ;;
        127) printf 'tmux-review: revdiff not found: install it, see #233\n' >&2; exit 1 ;;
        # deliver with nothing pinned cannot happen; belt and braces
        20) [ -s "$store" ] || exit 0
            # The agent has the review in hand the moment this returns, so the
            # pane carries the same "sent" the paste path sets on delivery.
            tmux set-option -p -t "$pane" @cockpit_review sent 2>/dev/null
            compose "$store" "$root"; done_store "$store"; exit 10 ;;
        21) # ^A: same clean-up review_pane does — the ref is written by the loop,
            # and clear_sess leaves an in-flight comments.md.deliver alone.
            tmux set-option -pu -t "$pane" @cockpit_review 2>/dev/null
            "$(dirname "$SELF")/agent-state" recount "$pane"
            clear_sess "$sess"
            exit 0 ;;
        *)  # leave / discard / a dropped popup: the store stays as the loop
            # left it. The rmdir is review_pane's own -- it takes the dir a
            # discard emptied and is a no-op while `leave` keeps the store.
            rmdir -- "$sess" 2>/dev/null
            exit 0 ;;
    esac
}

# No fleet queue any more (#253): prefix r opens the board, and r on a row
# there runs this. The pane is REVIEW_PANE_ID (-p) when the caller has it;
# otherwise the one the client is on, by a bare `display-message -p`, which
# inside a popup names no pane of its own and falls back to the most recently
# active client.
main_entry() {
    local pane dir
    pane=${REVIEW_PANE_ID:-$(tmux display-message -p '#{pane_id}' 2>/dev/null)}
    if [ -z "$pane" ]; then
        printf 'tmux-review: no tmux client to resolve a pane from\n' >&2
        exit 1
    fi
    dir=$(pane_dir "$pane")
    if [ -z "$dir" ]; then
        tmux display-message "$(fesc "cockpit: pane $pane is gone")" 2>/dev/null
        return 0
    fi
    review_pane "$pane" "$(git_root "$dir")"
}
