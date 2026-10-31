#!/usr/bin/env bash
# tmux-review's fleet queue (prefix r), --request and --info (prefix i).
# Sourced by tmux-review, never run. Bash 4+.

# --- --fleet / entry-point resolution --------------------------------------
# Which of the fleet needs your eyes: every agent pane, every session, that is
# either blocked or dirty -- landscape-gap #1, nothing else answers this. Rows
# are pane\t[◆|↵ ]state\ttitle\trepo⑂branch\t±stat.
#
# The order is the whole point, and it is NOT churn. A pane parked on a
# permission prompt is the one that actually needs a human, and it usually has
# nothing at all to diff -- churn-first ordering listed it last, or (with a
# clean tree) not at all, and answered "who typed most" instead of "who needs
# me". So @panestate ranks first -- needs-input, error, running, done, then
# unset -- and churn only breaks ties inside a rank. A needs-input pane is
# listed even with a clean tree; its ± column reads —, not "+0 -0", because
# there is nothing to review there, only someone to unblock.
#
# The is-agent test is tmux's own #{m:*|cmd|*,glob} conditional (deliver()'s
# guard, cockpit.tmux's window formats) run once per pane inside list-panes,
# not a second shell per pane to re-ask; the title falls back from pane_title
# to the window name the same way @catppuccin_window_text already does
# (examples/tmux.conf:42) -- #{host_short} is what an UNSET pane_title reads
# as, not empty.
fleet_rows() {
    # One -F argument, built in a variable: continuation lines each starting
    # a fresh quoted string are separate WORDS to tmux, not one concatenated
    # format -- list-panes then sees four positional arguments and refuses
    # them ("too many arguments"). Reproduced live before this fix.
    #
    # US (\037), not a tab, between the fields: pane_current_path and
    # @cockpit_dir are the two values that can legally carry a tab, and one
    # in a cwd truncated the path field and shifted @cockpit_dir into it
    # (#105 defect 15). pane_dir() already reads its own pair this way.
    local fmt raw sep=$'\037'
    # Both path fields are sanitised on the wire with LITERAL characters, the
    # only spelling #{s/…} understands: `\n` there matches the letter n and a
    # POSIX class ends the substitution at its first `]` (both measured on
    # 3.7c). The newline arm closes the record-framing hole; the other two
    # keep a path out of the delimiter's business.
    local safe="#{s/$sep/ /:#{s/"$'\t'"/ /:#{s/"$'\n'"/ /:"
    fmt='#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},1,0}'"$sep"
    fmt="${fmt}"'#{pane_id}'"$sep"
    # NOT a bare #{@panestate}: this was tab-separated, and tab is an IFS
    # *whitespace* character, so `read` collapsed a RUN of them into one
    # delimiter -- an unset @panestate emitted two, the field vanished, and
    # every field after it shifted left (the pane was dropped from the queue
    # entirely). US is not IFS whitespace and no longer collapses, but the
    # sentinel stays: `-` is what tells "unset" from a value further down.
    fmt="${fmt}"'#{?#{@panestate},#{@panestate},-}'"$sep"
    # Same sentinel, same reason: @cockpit_review is unset on most panes.
    fmt="${fmt}"'#{?#{@cockpit_review},#{@cockpit_review},-}'"$sep"
    fmt="${fmt}"'#{session_name}:#{window_index} '"$AGENT_TITLE_FMT""$sep"
    # Sentinelled too now that it is not the last field (an unset cwd shifted
    # @cockpit_dir into it). The option: pane_dir()'s rule off this one call.
    fmt="${fmt}"'#{?#{pane_current_path},'"$safe"'#{pane_current_path}}}},-}'"$sep"
    fmt="${fmt}"'#{?#{@cockpit_dir},'"$safe"'#{@cockpit_dir}}}},-}'
    raw=$(tmux list-panes -a -F "$fmt" 2>/dev/null)
    [ -n "$raw" ] || return 0

    # Every pane's git status+numstat used to run SERIALLY, one pane fully
    # finished before the next started -- ~2 git calls/pane. On a cold
    # filesystem (150ms/git, the hunt's measured figure) 100 panes blanked the
    # popup for ~32s, on every prefix+r from a non-agent pane. The panes are
    # independent of each other, so run their git work concurrently instead,
    # capped at $cap in flight at once -- unbounded would fork hundreds of git
    # processes at once on a big fleet, trading one kind of slowness for
    # another.
    local tmpdir
    tmpdir=$(mktemp -d) || return 0
    # Killed far more often than it finishes, and the rm at the end never runs
    # then. The handler exits rather than returning, since a returning handler
    # drops back into the interrupted wait, and it ignores its own signals first
    # so the EXIT trap's rm inherits SIG_IGN and survives tmux's second hangup.
    trap 'rm -rf -- "$tmpdir"' EXIT
    trap 'trap "" HUP INT TERM; exit 143' HUP INT TERM
    # shellcheck disable=SC2034  # read by tree_diff, in review-diff.sh
    local TREE_DIFF_TMP=$tmpdir      # tree_diff's index copies live here too

    # --- Phase 1: one pane list, one root each -------------------------------
    # Two agent panes cd'd into the same worktree (a subdirectory, or the
    # exact same path) are still two rows, but they have no business costing
    # two status+numstat scans -- so the repo root is resolved for every pane
    # FIRST, in its own background rev-parse (parallel, same cap as the scan
    # below), before any status call runs. rev-parse --show-toplevel takes no
    # lock; it is the status/diff calls after it that do, and those are what
    # phase 2 below dedups.
    local isagent id state review title path wt n=0 cap=${COCKPIT_FLEET_PARALLEL:-8}
    local -a pids=() pane_id=() pane_state=() pane_review=() pane_title=() pane_root=()
    while IFS=$sep read -r isagent id state review title path wt; do
        [ "$isagent" = 1 ] || continue
        [ "$path" != - ] || continue
        [ "$wt" != - ] && [ -d "$wt" ] && path=$wt
        [ "$state" = - ] && state=   # the wire sentinel above, back to unset
        [ "$review" = - ] && review=
        n=$((n + 1))
        pane_id[n]=$id; pane_state[n]=$state; pane_review[n]=$review; pane_title[n]=$title
        git -C "$path" --no-optional-locks rev-parse --show-toplevel \
            > "$tmpdir/root-$n" 2>/dev/null &
        pids+=("$!")
        if [ ${#pids[@]} -ge "$cap" ]; then
            wait "${pids[0]}" 2>/dev/null
            pids=("${pids[@]:1}")
        fi
    done <<<"$raw"
    wait 2>/dev/null
    [ "$n" -gt 0 ] || { rm -rf "$tmpdir"; trap - EXIT HUP INT TERM; return 0; }

    # Group panes by their resolved root. A pane whose cwd was not inside a
    # git repo at all resolves to an empty root; every such pane's git calls
    # fail the exact same way regardless of WHICH non-repo path it sits in
    # (nothing to diff against), so they all share one sentinel group instead
    # of each earning its own doomed scan.
    local -A root_idx=()
    local -a uniq_roots=()
    local i root key ri=0
    for ((i = 1; i <= n; i++)); do
        root=$(cat "$tmpdir/root-$i" 2>/dev/null)
        pane_root[i]=$root
        key=${root:-@@none@@}
        if [ -z "${root_idx[$key]+x}" ]; then
            ri=$((ri + 1))
            root_idx[$key]=$ri
            uniq_roots[ri]=$root
        fi
    done

    # --- Phase 2: one status+numstat scan per UNIQUE root --------------------
    pids=()
    for ((i = 1; i <= ri; i++)); do
        fleet_scan_root "${uniq_roots[i]}" > "$tmpdir/scan-$i" &
        pids+=("$!")
        if [ ${#pids[@]} -ge "$cap" ]; then
            wait "${pids[0]}" 2>/dev/null
            pids=("${pids[@]:1}")
        fi
    done
    wait 2>/dev/null

    # --- Phase 3: fan each pane's row back out of its group's scan -----------
    # No git call left here -- just the per-pane state/title/rank the shared
    # scan does not know, joined against the one result its group produced.
    local -a rows=()
    local _dirty repo branch stat churn rank line glyph
    for ((i = 1; i <= n; i++)); do
        id=${pane_id[i]}; state=${pane_state[i]}; title=${pane_title[i]}
        key=${pane_root[i]:-@@none@@}
        IFS=$'\t' read -r _dirty repo branch stat churn < "$tmpdir/scan-${root_idx[$key]}"
        case "$state" in
            needs-input) rank=0 ;;
            error)       rank=1 ;;
            running)     rank=2 ;;
            done)        rank=3 ;;
            *)           rank=4 ;;
        esac
        # The review mark rides the state column: ◆ the gate flagged a finished
        # diff nobody has read, ↵ a review is sitting pasted-but-unsent in the
        # pane. Without it the queue sends you back to a pane you just reviewed.
        case "${pane_review[i]}" in pending) glyph='◆ ' ;; sent) glyph='↵ ' ;; *) glyph='' ;; esac
        rows+=("$(printf '%s\t%s\t%s\t%s\t%s\t%s\t%s' \
            "$rank" "$churn" "$id" "$glyph${state:-—}" "$title" "${repo:-?}⑂${branch:-?}" "$stat")")
    done
    rm -rf "$tmpdir"; trap - EXIT HUP INT TERM
    [ ${#rows[@]} -gt 0 ] || return 0

    # Rank ascending, then churn descending inside the rank: who needs me
    # first, and among equals the one with the most to look at.
    for line in "${rows[@]}"; do printf '%s\n' "$line"; done \
        | sort -t "$(printf '\t')" -k1,1n -k2,2nr | cut -f3-
}

# One repo root's git state -- factored out of fleet_rows() so it can run in
# its own background subshell, ONCE per unique root (see the parallel phase-2
# dispatch above), not once per pane. Churn comes from tree_diff, the one base
# --lines reviews from, so a fully staged change still gets a real churn
# number instead of reading as 0. Prints dirty\trepo\tbranch\tstat\tchurn; an
# empty root (no pane in this group sits inside a git repo) short-circuits
# with no git call at all -- the result is path-independent already, so there
# is nothing to look up.
fleet_scan_root() {
    local root=$1
    local numstat add del untracked churn stat dirty=0 repo branch a d gd

    if [ -z "$root" ]; then
        # "?" on the wire, never an empty field: phase 3 joins this row back
        # with `IFS=$'\t' read`, tab is an IFS *whitespace* character, and a
        # RUN of tabs collapses into one delimiter there. Emitting repo and
        # branch empty shifted every field left, so a blocked agent outside a
        # repo rendered "—⑂" + churn with a blank ± column instead of "?⑂?"
        # and "—". Same trap, same sentinel, as the #{@panestate} guard in
        # fleet_rows' pane format above.
        printf '0\t?\t?\tno repo\t0\n'
        return 0
    fi
    # Two agents on two worktrees of the same repo are indistinguishable from
    # "s:w claude" alone; the row has to say WHICH tree it is. $root IS the
    # rev-parse --show-toplevel result already (fleet_rows resolved it to do
    # the grouping), so naming the repo costs no second call.
    repo=${root##*/}
    # The branch straight off the HEAD ref file, the way `git branch
    # --show-current` reads it, instead of a rev-parse fork per root: "ref:
    # refs/heads/<name>", or a bare sha when detached. A linked worktree's
    # .git is a file naming the dir that holds its own HEAD.
    gd=$root/.git
    if [ ! -d "$gd" ]; then
        read -r gd < "$gd" 2>/dev/null || gd=
        gd=${gd#gitdir: }; case "$gd" in ''|/*) ;; *) gd=$root/$gd ;; esac
    fi
    read -r branch < "$gd/HEAD" 2>/dev/null || branch=
    case "$branch" in 'ref: refs/heads/'*) branch=${branch#ref: refs/heads/} ;; *) branch=HEAD ;; esac

    # One diff per root, and that is every git this row needs beyond the
    # copy: `git status` here was a second walk of the tree for the "?N new"
    # count alone, and its `??` was the wrong count anyway once the base can
    # be a checkpoint -- a file untracked at ^A and untouched since is not
    # new to this review. --summary's "create mode" lines are the files new
    # AGAINST THE BASE, which is what the rest of the stat already measures.
    # tree_diff's --no-optional-locks is why reading a live worktree is safe.
    numstat=$(tree_diff "$root" --numstat --summary)
    if [ -n "$numstat" ]; then
        dirty=1
        add=0; del=0; untracked=0
        while IFS=$'\t' read -r a d _f; do
            case "$a" in ' create mode '*) untracked=$((untracked + 1)); continue ;; esac
            [ -n "$a" ] || continue
            case "$a" in *[!0-9]*) a=0 ;; esac
            case "$d" in *[!0-9]*) d=0 ;; esac
            add=$(( add + a )); del=$(( del + d ))
        done <<<"$numstat"
        stat="+$add -$del"
        [ "$untracked" -gt 0 ] && stat="$stat  ?$untracked new"
        churn=$(( add + del ))
    else
        # Nothing to review, only someone to unblock. "+0 -0" would read as a
        # measurement; the word is what the queue's ↵ keys its jump on.
        stat=clean; churn=0
    fi

    printf '%s\t%s\t%s\t%s\t%s\n' "$dirty" "${repo:-?}" "${branch:-?}" "$stat" "$churn"
}

# --- entry-point resolution -------------------------------------------------
# review_pane() drives the interactive loop against one pane: a session-dir
# store (session_dir(), below), the interactive --loop re-exec'd (not called
# as a function -- it ends in a script `exit`, and this needs its own exit
# code back rather than the whole process dying), ctrl-s (rc 20) handed to
# --deliver via `tmux run-shell -b` -- a popup kills its own children the
# moment it closes (tmux-open-path:381-386), so delivery has to outlive that
# teardown, not run inside it.

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
        tmux display-message "cockpit: could not create $sess" 2>/dev/null
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
        cleanup="rc=\$?; if [ \$rc -eq 0 ]; then $(shq "$SELF") --done $(shq "$store"); else rm -f -- $(shq "$store.rows"); fi"
        tmux run-shell -b "$(shq "$SELF") --deliver $(shq "$pane") $(shq "$store") $(shq "$dir"); $cleanup"
    elif [ "$rc" -eq 21 ]; then
        # Approved: the ref is written (run_loop did it, in the popup); the
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
        tmux display-message "cockpit: review approved · the next one shows only what changed" 2>/dev/null
    else
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
# and non-empty, and only ^S (rc 20) is a delivery. display-popup does not
# return the command's status -- it returns tmux's own -- so the popup command
# records it in <sess>/rc and this reads it back. A dropped launcher (the tool
# timed out) leaves the popup, the store and the rc file alone; running it
# again resumes the pinned comments, so the recovery is "run it again".
request() {                   # <pane>
    local pane=$1 dir root sess store pw ph rc t
    is_pane "$pane" || { usage; exit 2; }
    if [ "$(tmux display-message -p -t "$pane" '#{session_attached}' 2>/dev/null)" = 0 ]; then
        printf 'tmux-review: no client is attached to %s'"'"'s session — nowhere to open the review\n' "$pane" >&2
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
    # -T is a tmux format: a # in the path would be expanded, so double it.
    t=${root/#"$HOME/"/\~/}; t=${t//\#/\#\#}
    tmux display-popup -E -t "$pane" -w "$pw" -h "$ph" -T " review · requested · $t " \
        "REVIEW_PANE_ID=$(shq "$pane") $(shq "$SELF") --loop $(shq "$root") $(shq "$store"); echo \$? > $(shq "$sess/rc")" 2>/dev/null
    rc=$(cat -- "$sess/rc" 2>/dev/null); rm -f -- "$sess/rc"
    case "$rc" in
        20) [ -s "$store" ] || exit 0          # deliver with nothing pinned cannot happen; belt and braces
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

# The optional cockpit-tui binary, for the info screen: whether it is there,
# which version, and whether @cockpit_tui_version asks for a different one.
# Absent is a normal state, not a fault -- prefix r is the fleet view with or
# without it -- so the row names the gesture that installs it and stops.
# Which credentials that fetch would ride (#84): the repo is private, so an
# unauthenticated prefix U 404s, and the fix is `gh auth login`, not retrying.
tui_auth() { "$(dirname "$LIBDIR")/tui-fetch" --auth 2>/dev/null || printf 'unauthenticated'; }
tui_row() {
    local bin have want
    bin="$(dirname "$(dirname "$LIBDIR")")/bin/cockpit-tui"
    # Absent is the common case: test the file before paying a fork for it.
    [ -x "$bin" ] || { printf 'not installed  (prefix U fetches it, %s)' "$(tui_auth)"; return 0; }
    have=$("$bin" --version 2>/dev/null); have=${have##* }; have=${have#v}
    [ -n "$have" ] || { printf 'not installed  (prefix U fetches it, %s)' "$(tui_auth)"; return 0; }
    want=$(tmux show-option -gqv @cockpit_tui_version 2>/dev/null)
    want=${want#cockpit-}; want=${want#tui-}; want=${want#v}
    if [ -n "$want" ] && [ "$want" != "$have" ]; then
        printf '%s  %s  (want %s -- prefix U updates it)' "$have" "$bin" "$want"
    else
        printf '%s  %s' "$have" "$bin"
    fi
}

# Seconds as 45s, 4m12s or 1h3m; nothing for a non-number.
age_short() {                 # <seconds>
    case $1 in ''|*[!0-9]*|??????????*) return 1 ;; esac
    local n=$((10#$1))        # 0100 is a hundred, not an octal error
    if [ "$n" -ge 3600 ]; then printf '%sh%sm' $(( n / 3600 )) $(( n % 3600 / 60 ))
    elif [ "$n" -ge 60 ]; then printf '%sm%ss' $(( n / 60 )) $(( n % 60 ))
    else printf '%ss' "$n"; fi
}

# The doing / said / asked rows and the /btw pointer, for an agent pane. The
# transcript reader fails quietly (status 1, no output): then no rows, no error.
info_doing_rows() {           # <pane>
    local pane=$1 is_agent file kind a b age
    is_agent=$(tmux display-message -p -t "$pane" \
        '#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},1,0}' 2>/dev/null)
    [ "$is_agent" = 1 ] || return 0
    if file=$(transcript_file "$pane" 2>/dev/null) && [ -n "$file" ]; then
        while IFS=$'\t' read -r kind a b; do
            case $kind in
                doing) age=$(age_short "$b") && age=" (since $age)"
                       printf 'doing     %s%s\n' "$a" "$age" ;;
                said)  printf 'said      %s\n' "$a" ;;
                asked) age=$(age_short "$b") && age=" ($age)"
                       printf 'asked     %s%s\n' "$a" "$age" ;;
            esac
        done < <(transcript_doing "$file" 2>/dev/null)
    fi
    printf 'ask it    /btw <question> in the pane (answers while it works)\n'
}

# --- --info: one screen for prefix i -------------------------------------------
# What the plugin knows about one pane, in the order you ask it: who, what it
# waits on, the review state, whether a side fix holds it, the base, the
# session, the review dir. Read in ONE display-message, split on a US byte
# (agent-state's idiom -- tab would collapse the empty fields that mean
# "unset").
info_mode() {                 # <pane>
    local pane=$1 sep=$'\037' f id where cmd path wt state msg review spool sid title hold root sess b n1 n2
    f=$(tmux display-message -p -t "$pane" \
        '#{pane_id}'"$sep"'#{session_name}:#{window_index} #{window_name}'"$sep"'#{pane_current_command}'"$sep"'#{pane_current_path}'"$sep"'#{@cockpit_dir}'"$sep"'#{@panestate}'"$sep"'#{@panestate_msg}'"$sep"'#{@cockpit_review}'"$sep"'#{@cockpit_review_spool}'"$sep"'#{@cockpit_session}'"$sep"'#{@cockpit_title}'"$sep"'#{@cockpit_hold}' 2>/dev/null)
    id=${f%%"$sep"*};     f=${f#*"$sep"}
    # display-message does NOT fail on a pane id that does not exist -- it
    # expands every format to the empty string and exits 0 (measured on 3.7c),
    # and without this the screen below described the CALLER's own cwd
    # (git_root "" is the caller's repo) as if it were that pane's. The
    # resolved id, not the exit code, is what says the pane was found.
    [ -n "$id" ] || { printf 'tmux-review: no such pane %s\n' "$pane" >&2; return 1; }
    where=${f%%"$sep"*};  f=${f#*"$sep"}
    cmd=${f%%"$sep"*};    f=${f#*"$sep"}
    path=${f%%"$sep"*};   f=${f#*"$sep"}
    wt=${f%%"$sep"*};     f=${f#*"$sep"}
    state=${f%%"$sep"*};  f=${f#*"$sep"}
    msg=${f%%"$sep"*};    f=${f#*"$sep"}
    review=${f%%"$sep"*}; f=${f#*"$sep"}
    spool=${f%%"$sep"*};  f=${f#*"$sep"}
    sid=${f%%"$sep"*};    f=${f#*"$sep"}
    title=${f%%"$sep"*};  hold=${f#*"$sep"}
    printf 'pane      %s  %s  (%s)\n' "$id" "$where" "$cmd"
    printf 'cwd       %s\n' "$path"
    # The root is the tree prefix r diffs; an agent editing another tree
    # without setting @cockpit_dir shows the wrong one, and this is how the
    # human notices.
    if [ -n "$wt" ]; then
        if [ -d "$wt" ]; then path=$wt; printf 'dir       %s  (@cockpit_dir)\n' "$wt"
        else printf 'dir       %s  (@cockpit_dir, missing -- using cwd)\n' "$wt"; fi
    fi
    root=$(git_root "$path")
    printf 'root      %s\n' "$root"
    printf 'state     %s%s\n' "${state:-—}" "${msg:+  · $msg}"
    info_doing_rows "$id"
    printf 'review    %s%s\n' "${review:-—}" "${spool:+  · queued: $spool}"
    printf 'hold      %s\n' "${hold:-—}"
    sess=$(session_dir "$id" "$root")
    if git -C "$root" --no-optional-locks rev-parse -q --verify HEAD >/dev/null 2>&1; then
        # The mode ^B chose for THIS review, as run_loop reads it -- passed as
        # the argument review_base actually keys on, not exported (nothing this
        # function calls reads COCKPIT_REVIEW_BASE from the environment).
        b=$(review_base "$root" "$(cat -- "$sess/comments.md.base" 2>/dev/null)")
        printf 'base      %s  (%s)\n' "$(cut -f2 <<<"$b")" "$(cut -f1 <<<"$b")"
    fi
    printf 'session   %s\n' "${sid:-—}"
    printf 'title     %s\n' "${title:-—}"
    printf 'tui       %s\n' "$(tui_row)"
    if [ -d "$sess/fixer" ]; then
        # tmux-fixer/review-gate's own record, read the same way they read it
        # -- no fixer_alive() call here, this reports the raw facts (reason,
        # pane, deadline, guard) rather than the pass/fail liveness verdict.
        local freason fpane fdl fguard falive fleft
        freason=$(cat -- "$sess/fixer/reason" 2>/dev/null)
        fpane=$(cat -- "$sess/fixer/pane" 2>/dev/null)
        fdl=$(cat -- "$sess/fixer/deadline" 2>/dev/null)
        fguard=$(cat -- "$sess/fixer/guard" 2>/dev/null)
        if { [ "$fpane" = pending ] || is_pane "$fpane"; } && fixer_pane_alive "$fpane"; then
            falive=alive
        else
            falive=gone
        fi
        case "$fdl" in ''|*[!0-9]*) fleft=0 ;; *) fleft=$(( fdl - $(date +%s) )); [ "$fleft" -lt 0 ] && fleft=0 ;; esac
        printf 'side fix  %s  pane %s (%s)  deadline in %ss  guard /%s/\n' "$freason" "$fpane" "$falive" "$fleft" "$fguard"
    fi
    if [ -d "$sess" ]; then
        n1=$(grep -c '^## ' -- "$sess/comments.md" 2>/dev/null); n2=$(grep -c '^## ' -- "$sess/delivered.md" 2>/dev/null)
        printf 'review dir %s\n           %s pinned · %s delivered · ^A removes it\n' "$sess" "${n1:-0}" "${n2:-0}"
    fi
    printf '\n↵ to close\n'
}

# Pad one field to a fixed COLUMN width. bash's printf pads %-*s by BYTES,
# while ${#s} counts characters -- so a field holding ⑂ or — (3 bytes each)
# came out short and dragged every column to its right with it. Measuring the
# characters first, then widening the printf field by the byte overhead, makes
# printf's own padding land where the eye expects it.
pad_field() {                 # <width> <string>
    local w=$1 c=${#2}
    local LC_ALL=C            # from here ${#2} is bytes, which is what printf counts
    printf '%-*s' "$(( w + ${#2} - c ))" "$2"
}

# Cut a field TO a column width; pad_field only ever pads, so an over-cap
# field used to overflow and drag every column right of it (#105 defect 5).
# head=1 keeps the tail: in repo⑂branch the branch is the half you do not
# already know.
fit_field() {                 # <width> <string> [head]
    local w=$1 s=$2 mb='…' i
    [ "${#s}" -le "$w" ] && { printf '%s' "$s"; return; }
    [ "$w" -lt 2 ] && { printf '%s' "${s:0:$w}"; return; }
    # ${s:i:n} counts BYTES unless the locale is UTF-8, so under C the cut
    # lands inside a multibyte character and leaves an orphan the terminal
    # draws as a replacement glyph -- measured on the owner's own
    # repo⑂branch, where the head cut fell through the ⑂. ${#mb} is 1 in a
    # UTF-8 locale and 3 in a byte one, which is the only way to ask from
    # inside bash. pad_field below carries the same trap in its own form.
    if [ -n "${3:-}" ]; then
        s=${s: -$(( w - 1 ))}
        # A leading continuation byte is the tail of a character we cut
        # through; dropping it can never remove a whole one, since no
        # character starts with one.
        [ "${#mb}" -eq 1 ] || for i in 1 2 3; do
            case $s in [$'\200'-$'\277']*) s=${s#?} ;; *) break ;; esac
        done
        printf '…%s' "$s"
    else
        s=${s:0:$(( w - 1 ))}
        # The tail is the mirror, and a byte locale cannot tell a complete
        # character at the end from half of one, so drop the last character
        # whole. One column narrower than it could be, in a field that
        # already says … , and never a broken edge.
        [ "${#mb}" -eq 1 ] || { for i in 1 2 3; do
            case $s in *[$'\200'-$'\277']) s=${s%?} ;; *) break ;; esac
        done; case $s in *[$'\302'-$'\364']) s=${s%?} ;; esac; }
        printf '%s…' "$s"
    fi
}

kill_window() {               # <pane>  the row's window, after a y/N in the popup
    local pane=$1 where nwin npane ans
    is_pane "$pane" || return 0
    # #{?pane_id,...}: a pane gone since the list was built reads ": ", not empty.
    where=$(tmux display-message -p -t "$pane" '#{?pane_id,#{session_windows} #{window_panes} #{session_name}:#{window_index} #{window_name},}' 2>/dev/null)
    [ -n "$where" ] || { tmux display-message "cockpit: pane $pane is gone" 2>/dev/null; return 0; }
    nwin=${where%% *}; where=${where#* }; npane=${where%% *}; where=${where#* }
    if [ "$nwin" = 1 ]; then printf 'kill session %s (last window' "${where%%:*}"
    else printf 'kill window %s' "$where"; fi
    [ "$npane" -gt 1 ] 2>/dev/null && printf ', %s panes' "$npane"
    if [ "$nwin" = 1 ]; then printf ')? [y/N] '; else printf '? [y/N] '; fi
    IFS= read -r -n1 ans || ans=
    printf '\n'
    case "$ans" in y|Y) tmux kill-window -t "$pane" 2>/dev/null ;; esac
}


# The fzf queue over fleet_rows(): every agent pane, clean ones too (#49; a
# dirty-only filter hid an agent whose work sat in a tree its cwd did not
# name, and with it the only list of which agents exist).
# Unlike the review loop this filters as you
# type -- it is a pane list, not comment entry, so --disabled has no place
# here. Rows carry pane, state, title, repo⑂branch and stat; the preview
# re-asks tmux for the pane's cwd by id rather than smuggling a sixth hidden
# field, so `--fleet`'s output stays exactly that interface.
#
# The four visible fields are padded to their own column widths before fzf
# sees them: ragged single-space columns made the ± numbers impossible to
# compare down the list, which is most of what this list is FOR.
#
# The queue LOOPS (§8 #3): finishing one review re-lists the fleet with the
# pane you just left under the pointer, so reviewing four agents is four ↵
# instead of four prefix+r. esc on the list is the only way out -- a review's
# own exit code (deliver, approve, discard, leave) says nothing about whether
# you are done with the fleet.
fleet_queue() {               # [preselect-pane]
    command -v fzf >/dev/null 2>&1 || {
        tmux display-message "cockpit: the fleet queue needs fzf" 2>/dev/null
        return 1
    }
    load_theme || return 1
    # One order for every cockpit picker: ↵, alt-↵, ⇥, ^-verbs, ^/, esc. Both
    # lines stay inside 48 columns, which is what a 70% popup draws at 80: the
    # old legend reached column 94, so fzf's right-hand cut took the column
    # names the line exists to give (#105).
    local keys='↵ review · ^X kill window · ^/ preview · esc'
    local ctx='◆ pending · ↵ unsent · state · title · where · ±'
    local -a THEME=()
    mapfile -t THEME < <(cockpit_fzf_args highlight-line header-border=bottom "footer=$keys")
    local pre=${1:-}
    while :; do
        local rows
        rows=$(fleet_rows)
        if [ -z "$rows" ]; then
            tmux display-message "cockpit: no pane runs an agent named in @cockpit_agents_glob" 2>/dev/null
            return 0
        fi

        # Column widths, capped: one 200-character pane title should not push
        # repo⑂branch and the ± count off the right edge for every other row.
        local f_pane f_state f_title f_loc f_stat
        local w_state=0 w_title=0 w_loc=0 w_stat=0
        while IFS=$'\t' read -r f_pane f_state f_title f_loc f_stat; do
            [ -n "$f_pane" ] || continue
            [ "${#f_state}" -gt "$w_state" ] && w_state=${#f_state}
            [ "${#f_title}" -gt "$w_title" ] && w_title=${#f_title}
            [ "${#f_loc}"   -gt "$w_loc"   ] && w_loc=${#f_loc}
            [ "${#f_stat}"  -gt "$w_stat"  ] && w_stat=${#f_stat}
        done <<<"$rows"
        [ "$w_state" -gt 13 ] && w_state=13     # "◆ needs-input", the widest state there is
        [ "$w_title" -gt 40 ] && w_title=40
        [ "$w_loc"   -gt 40 ] && w_loc=40

        # Then shed until the row fits the popup, in one order every time:
        # repo⑂branch gives way first (down to 12), then the title (down to
        # 16). The ± column is what the queue exists to compare down the
        # list, so it never gives way -- and it was the first thing cut on
        # the old 8-column tabstop grid, on any terminal under ~150 columns
        # (#105 defect 5). fzf keeps 2 columns for its pointer, and each
        # delimiter is one column under --tabstop=1.
        local cols cut shed
        cols=$(tput cols 2>/dev/null || echo 80)
        case $cols in ''|*[!0-9]*) cols=80 ;; esac
        cut=$(( w_state + w_title + w_loc + w_stat + 3 - (cols - 2) ))
        if [ "$cut" -gt 0 ]; then
            shed=$(( w_loc - 12 )); [ "$shed" -lt 0 ] && shed=0
            [ "$shed" -gt "$cut" ] && shed=$cut
            w_loc=$(( w_loc - shed )); cut=$(( cut - shed ))
        fi
        if [ "$cut" -gt 0 ]; then
            shed=$(( w_title - 16 )); [ "$shed" -lt 0 ] && shed=0
            [ "$shed" -gt "$cut" ] && shed=$cut
            w_title=$(( w_title - shed ))
        fi

        local -a disp=()
        while IFS=$'\t' read -r f_pane f_state f_title f_loc f_stat; do
            [ -n "$f_pane" ] || continue
            disp+=("$f_pane"$'\t'\
"$(pad_field "$w_state" "$(fit_field "$w_state" "$f_state")")"$'\t'\
"$(pad_field "$w_title" "$(fit_field "$w_title" "$f_title")")"$'\t'\
"$(pad_field "$w_loc"   "$(fit_field "$w_loc"   "$f_loc" head)")"$'\t'\
"$f_stat")
        done <<<"$rows"
        [ ${#disp[@]} -gt 0 ] || return 0

        local nrows avail lrows psz preview_cmd out pane dir
        nrows=${#disp[@]}
        avail=$(tput lines 2>/dev/null || echo 24)
        lrows=$nrows; [ "$lrows" -gt 8 ] && lrows=8
        # 6, not 5: the picker spends two rows on chrome either way -- context
        # and keys as two header lines below the fzf floor, context in the
        # header and keys in the footer above it. Reserve the second row rather
        # than taking it out of the list.
        psz=$(( avail - lrows - 6 ))
        [ "$psz" -lt 5 ] && psz=5

        # The pointer starts on $pre's row -- the pane prefix+r was pressed in,
        # then whichever pane the last review was of. Without it the pointer
        # sits on whatever the SORT put first, and the one pane you actually
        # asked for is one you have to hunt for. --sync is what makes the bind
        # land: without it fzf runs `start` before the list is loaded and the
        # move is silently lost. pos() is 1-based over the input order.
        # A pane that is not in the queue at all (nothing to review there)
        # leaves pos at 0 and the list unmoved.
        local pos=0 i
        local -a presel=()
        for i in "${!disp[@]}"; do [ "${disp[$i]%%$'\t'*}" = "$pre" ] && { pos=$(( i + 1 )); break; }; done
        [ "$pos" -gt 0 ] && presel=(--sync --bind "start:pos($pos)")

        # The row carries no path: ask --dir (pane_dir's rule, dangling option
        # included) and resolve to the root, which --preview-diff expects.
        preview_cmd="pdir=\$($(shq "$SELF") --dir {1} 2>/dev/null); "
        preview_cmd="${preview_cmd}proot=\$(git -C \"\$pdir\" rev-parse --show-toplevel 2>/dev/null); "
        preview_cmd="${preview_cmd}$(shq "$OPENPATH") --preview-diff \"\${proot:-\$pdir}\""

        # @cockpit_fleet_fzf_options is a GLOBAL tmux option -- it can only have
        # been written by the owner's own tmux.conf, so `eval`ing it is eval of
        # their config, not of anything an agent pane can reach. It is confined to
        # this navigation picker on purpose: in the review loop ↵ has a contract
        # (pin a comment, never approve) and ^S is the consent gesture, and one
        # stray --bind from a config would quietly break either of them.
        local -a extra=()
        local opt; opt=$(tmux show-option -gqv @cockpit_fleet_fzf_options 2>/dev/null)
        [ -n "$opt" ] && eval "extra=($opt)"

        out=$(printf '%s\n' "${disp[@]}" \
            | fzf "${THEME[@]}" --delimiter=$'\t' --with-nth=2,3,4,5 \
                  --reverse --info=inline-right --cycle --scroll-off=2 \
                  --bind 'ctrl-/:toggle-preview,ctrl-u:preview-half-page-up,ctrl-d:preview-half-page-down' \
                  --expect=ctrl-x \
                  --preview "$preview_cmd" \
                  --preview-window="down,$psz,border-top" \
                  --prompt='fleet> ' \
                  --header="$ctx" \
                  ${presel[@]+"${presel[@]}"} \
                  ${extra[@]+"${extra[@]}"})
        [ -n "$out" ] || return 0

        # --expect: the key line first (empty for ↵), then the row -- or no
        # row at all when the key was pressed on an empty filter.
        local key=${out%%$'\n'*} sel=${out#*$'\n'}
        [ "$sel" != "$out" ] || continue
        pane=${sel%%$'\t'*}
        [ -n "$pane" ] || return 0
        if [ "$key" = ctrl-x ]; then
            kill_window "$pane"
            pre=$pane
            continue
        fi
        # Nothing to review on a clean row: ↵ goes to the pane instead of a
        # review that closes itself (§8 #3). -c pins the opener's client: bare
        # switch-client in a popup takes the busiest client (main_entry's note).
        case ${sel##*$'\t'} in clean|'no repo')
            local cl
            cl=$(tmux list-clients -t "$(tmux display-message -p -t "${REVIEW_PANE_ID:-$pane}" '#{session_name}' 2>/dev/null)" \
                    -F '#{client_name}' 2>/dev/null | head -n1)
            [ -z "$cl" ] || tmux switch-client -c "$cl" -t "$pane" 2>/dev/null
            tmux select-window -t "$pane" 2>/dev/null
            tmux select-pane -t "$pane" 2>/dev/null
            return 0 ;;
        esac
        dir=$(pane_dir "$pane")
        if [ -z "$dir" ]; then
            tmux display-message "cockpit: pane $pane is gone" 2>/dev/null
            continue
        fi
        review_pane "$pane" "$(git_root "$dir")"
        pre=$pane
    done
}

# No args, bound to prefix R: ask tmux which pane the CLIENT is actually on --
# not $TMUX_PANE, which inside this popup names the popup's own pseudo-pane,
# not the pane you pressed the key from. Bare `display-message -p` (no -t)
# has no client of its own to resolve from inside a popup -- it falls back to
# whichever attached client was most recently active, which can be a
# DIFFERENT session than the one the popup was opened from (reproduced with
# two real attached clients: the busier one wins, not the opener). REVIEW_PANE_ID
# -- this script's own -p, parsed below, mirroring tmux-branch's -p
# (tmux-branch:38-53) -- pins the query to the actual pane the popup came
# from when the caller has it; without it this falls back to the old
# best-effort client-recency resolution.
main_entry() {
    local pane is_agent
    if [ -n "$REVIEW_PANE_ID" ]; then
        pane=$REVIEW_PANE_ID
    else
        pane=$(tmux display-message -p '#{pane_id}' 2>/dev/null)
    fi
    if [ -z "$pane" ]; then
        printf 'tmux-review: no tmux client to resolve a pane from\n' >&2
        exit 1
    fi
    is_agent=$(tmux display-message -p -t "$pane" \
        '#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},1,0}' 2>/dev/null)
    # Always the queue (§8 #3): the other blocked agents stay visible, and the
    # pane the key was pressed in is preselected so one ↵ reviews it.
    if [ "$is_agent" = 1 ]; then fleet_queue "$pane"; else fleet_queue; fi
}
