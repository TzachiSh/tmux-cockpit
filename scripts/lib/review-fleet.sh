#!/usr/bin/env bash
# tmux-review's review entry (what r on a board row runs) and --request.
# Sourced by tmux-review, never run. Bash 4+.

# The loop is re-exec'd, not called: it ends in `exit`, and rc 20 is handed to
# --deliver via `run-shell -b` because a popup kills its children as it closes.
review_pane() {
    local pane=$1 dir=$2 store rc
    local sess cleanup
    sess=$(session_dir "$pane" "$dir"); store="$sess/comments.md"
    # umask in the subshell, so the parents and the store are born private.
    # [ ! -L ]: a store path is derivable, so a planted symlink is refused.
    (umask 077; mkdir -p -- "$sess" && [ ! -L "$store" ] && : >> "$store") || {
        tmux display-message "$(fesc "cockpit: could not create $sess")" 2>/dev/null
        return 1
    }
    REVIEW_PANE_ID=$pane "$SELF" --loop "$dir" "$store"
    rc=$?
    if [ "$rc" -eq 20 ]; then
        # The store goes only when --deliver succeeds: a refusal comes after the
        # popup is gone, and the store is the only copy. The snapshot goes either
        # way. run-shell hands this to `sh -c`, hence the escaped \$; every path
        # is shq-quoted and the whole string fesc'd.
        cleanup="rc=\$?; if [ \$rc -eq 0 ]; then $(shq "$SELF") --done $(shq "$store");"
        cleanup="$cleanup else rm -f -- $(shq "$store.rows"); fi"
        tmux run-shell -b "$(fesc "$(shq "$SELF") --deliver $(shq "$pane") $(shq "$store") \
$(shq "$dir"); $cleanup")"
    elif [ "$rc" -eq 21 ]; then
        # Approved (the loop wrote the ref). clear_sess, not rm -rf: a review
        # already queued for a busy pane must survive the approval.
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
# Run from the agent's own Bash tool (the cockpit-review skill): one popup on the
# pane, and the batch back on stdout with exit 10. The loop's exit code decides,
# not the store. display-popup returns tmux's status, not the command's, so the
# popup writes it to <sess>/rc. A timed-out launcher leaves the popup running;
# running it again resumes the pinned comments.
request() {                   # <pane>
    local pane=$1 dir root sess store pw ph rc t cmd
    is_pane "$pane" || { usage; exit 2; }
    if [ "$(tmux display-message -p -t "$pane" '#{session_attached}' 2>/dev/null)" = 0 ]; then
        printf 'tmux-review: no client is attached to %s'"'"'s session' "$pane" >&2
        printf ' — nowhere to open the review\n' >&2
        exit 1
    fi
    dir=$(pane_dir "$pane")
    [ -n "$dir" ] || { printf 'tmux-review: pane %s is gone\n' "$pane" >&2; exit 1; }
    root=$(git_root "$dir"); sess=$(session_dir "$pane" "$root"); store="$sess/comments.md"
    (umask 077; mkdir -p -- "$sess" && [ ! -L "$store" ] && : >> "$store") || {
        printf 'tmux-review: could not create %s\n' "$sess" >&2; exit 1; }
    pw=$(tmux show-option -gqv @cockpit_popup_width);  pw=${pw:-70%}
    ph=$(tmux show-option -gqv @cockpit_popup_height); ph=${ph:-70%}
    # A killed launcher's popup still writes rc when closed: clear it first, so
    # the rc read below is this popup's.
    rm -f -- "$sess/rc"
    # -T is a tmux format, so a # in the path would expand: fesc. The command is
    # not a format (measured on 3.8), so it is only shell-quoted.
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
        125) printf '%s\n' "$GIT_REFUSAL" >&2; rmdir -- "$sess" 2>/dev/null; exit 1 ;;
        127) printf 'tmux-review: revdiff not found: install it, see #233\n' >&2; exit 1 ;;
        20) [ -s "$store" ] || exit 0
            # The agent has the review the moment this returns: "sent", as the
            # paste path sets it.
            tmux set-option -p -t "$pane" @cockpit_review sent 2>/dev/null
            compose "$store" "$root"; done_store "$store"; exit 10 ;;
        21) # ^A: review_pane's clean-up.
            tmux set-option -pu -t "$pane" @cockpit_review 2>/dev/null
            "$(dirname "$SELF")/agent-state" recount "$pane"
            clear_sess "$sess"
            exit 0 ;;
        *)  # leave, discard or a dropped popup: the store stays as the loop left it.
            rmdir -- "$sess" 2>/dev/null
            exit 0 ;;
    esac
}

# The pane is REVIEW_PANE_ID (-p), else the client's: a bare `display-message -p`
# inside a popup falls back to the most recently active client.
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
