#!/usr/bin/env bash
# tmux-review's diff engine: the base, the diff, the --lines rows, ^A's checkpoint.
# Sourced by tmux-review, never run. Bash 4+.

# Files git would have to hash in full to diff, and the knee in what that
# costs: 100 ms at 32 MB, 680 ms at 256 MB, 26 s at 1 GB, measured. NOT the
# review's 4 MB byte cap -- a 4.6 MB lockfile bump is two rows and must stay.
BIGCAP=$(( 32 * 1024 * 1024 ))
# ponytail: one number at the measured knee, no option; add @cockpit_review_max_file when someone reviews a 40 MB fixture on purpose.

# Untracked and stat-modified files over BIGCAP, "<bytes>\t<path>". An
# untracked file, and a tracked one whose SIZE changed, cost a stat here and
# nothing more; a touched tracked file of unchanged size git still hashes to
# tell -- once, here. ':(top)' because ls-files alone stops at the cwd, while
# git diff never does. ONE stat for the whole list: a fork per file made an
# 800-file tree a fork walk.
# ponytail: that leaves one 0.68 s hash on a touched 256 MB tracked file whose
# size did not change; to drop it, walk `ls-files -z` and stat every tracked
# file over the cap, `cat-file -s` its index blob for a size change -- equal
# sizes still need the hash.
big_files() {                 # <dir>
    local dir=$1
    local -a sz=(-c $'%s\t%n')          # GNU; BSD spells the same two fields -f '%z\t%N'
    stat -c %s -- "$dir" >/dev/null 2>&1 || sz=(-f $'%z\t%N')
    git -C "$dir" --no-optional-locks ls-files -o -m --exclude-standard -z -- ':(top)' 2>/dev/null \
    | ( cd "$dir" 2>/dev/null || exit; xargs -0 -r stat "${sz[@]}" -- 2>/dev/null ) \
    | awk -F'\t' -v cap="$BIGCAP" 'NF == 2 && $1 + 0 > cap'
}

# --- the base every review diffs against --------------------------------------
# ONE diff, whatever the base: staged, unstaged AND untracked changes come out
# of a single `git diff <base>`, because the diff runs against a COPY of the
# index that `git add -N -A` has extended with an intent-to-add entry for
# every untracked file. -N records the path only (no content is hashed), and
# a plain `git diff` then shows such a file as a new file against the working
# tree -- the documented use of intent-to-add. Measured on the eight goldens:
# byte-identical to the ls-files/awk synthesis this replaces, 12/12 runs.
#
# The copy is not optional: an `add` against the real index would rewrite the
# agent's own index under it. `cp -p`, not `cp`: git's racy-timestamp guard
# compares each entry's mtime with the INDEX FILE's mtime, and a fresh copy
# with a fresh mtime made git trust a same-size, same-second edit as clean --
# reproduced 1 run in 3 on the `spaces` fixture, 0 in 12 with -p. An unborn
# repo has no index to copy; the rm lets git start one from nothing.
#
# It also makes the approved checkpoint (^A, a tree that INCLUDES untracked
# files) diff exactly: against the real index `git diff <tree>` reports a
# file present in the tree but untracked here as DELETED (reproduced); with
# the -N entry it compares the file's content instead.
#
# --no-optional-locks on every call, the same discipline fleet_scan_root()
# uses: even a read can take index.lock, and this runs against a repo an
# AGENT is working in.
tree_diff() {
    local dir=$1; shift
    local idx gd head br rp
    # One rev-parse for the three facts every diff needs -- the git dir (whose
    # index to copy), HEAD, and the branch review_base keys on. rev-parse
    # carries on past an argument it cannot resolve, so an unborn HEAD leaves
    # two lines (git dir, "HEAD") and a born one three. This and review_base
    # were 10 forks per root; the 20-pane fleet fork count caught it.
    mapfile -t rp < <(git -C "$dir" --no-optional-locks rev-parse --absolute-git-dir HEAD --abbrev-ref HEAD 2>/dev/null)
    gd=${rp[0]-}; head=${rp[1]-}; br=${rp[2]-}
    [ -n "$gd" ] || return 1
    # A caller that scans many roots at once (fleet_rows) lends the one temp
    # dir it already owns, and the copy is named by PID and dies with that
    # dir: mktemp + rm per root were 40 of the fleet's forks.
    if [ -n "${TREE_DIFF_TMP:-}" ]; then idx=$TREE_DIFF_TMP/idx.$BASHPID
    else idx=$(mktemp) || return 1
        trap 'rm -f -- "$idx"' EXIT; trap 'trap "" HUP INT TERM; exit 143' HUP INT TERM; fi
    cp -p -- "$gd/index" "$idx" 2>/dev/null || rm -f -- "$idx"
    GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks add -N -A 2>/dev/null
    # Keep the big files away from git rather than filter them afterwards: the
    # cost is the hashing, and only a pathspec stops that. Exclusions with no
    # positive pathspec beside them still leave the diff whole-worktree wide.
    local -a ex=() bigp=()
    case " $* " in *' -- '*) ;; *)
        local bp
        while IFS=$'\t' read -r _ bp; do
            [ -n "$bp" ] || continue
            bigp+=("$bp"); ex+=(":(exclude,literal)$bp")
        done < <(big_files "$dir")
        [ "${#ex[@]}" -gt 0 ] && ex=(-- "${ex[@]}") ;;
    esac
    if [ -n "$br" ]; then
        local base
        base=$(review_base "$dir" "${COCKPIT_REVIEW_BASE:-auto}" "$br" "$head"); base=${base%%$'\t'*}
        GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks -c core.quotePath=false \
            diff --no-color --no-ext-diff "$base" "$@" "${ex[@]+"${ex[@]}"}" 2>/dev/null
    else
        local out
        out=$(GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks -c core.quotePath=false diff --no-color --no-ext-diff "$@" "${ex[@]+"${ex[@]}"}" 2>/dev/null)
        [ -n "$out" ] || out=$(GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks -c core.quotePath=false diff --no-color --no-ext-diff --cached "$@" "${ex[@]+"${ex[@]}"}" 2>/dev/null)
        printf '%s\n' "$out"
    fi
    # The row git printed for these itself, before we kept it from reading
    # them: a binary file's numstat is "-\t-\tpath". Without it a tree whose
    # only change is a big file reads CLEAN to --delta and to the fleet scan.
    [ "${#bigp[@]}" -eq 0 ] || case " $* " in *' --numstat '*) printf -- '-\t-\t%s\n' "${bigp[@]}" ;; esac
    [ -n "${TREE_DIFF_TMP:-}" ] || { rm -f -- "$idx"; trap - EXIT HUP INT TERM; }
}

# --- which base ---------------------------------------------------------------
# Four cases, the first that holds (when-how lens §2.3; revdiff's auto-detect):
#   since-review  refs/cockpit/reviewed/<branch> exists (^A wrote it)  -> only what moved since
#   branch        merge-base of the upstream (else origin/HEAD, else main/master) and HEAD,
#                 when that is not HEAD itself                          -> the branch + uncommitted
#   uncommitted   HEAD                                                  -> today's view
# MODE forces one; a forced mode that does not hold falls through to the next,
# so the printed mode is the one that HELD -- next_base relies on that.
# Prints "<rev>\t<label>\t<mode>". Never called on an unborn HEAD.
review_base() {               # <dir> [auto|since-review|branch|uncommitted] [branch] [head-sha]
    local dir=$1 mode=${2:-auto} br=${3:-} head=${4:-} ref up mb
    local name sha ups track sym ckpt="" bups="" btrack="" ohead="" osha="" mainsha="" mastersha="" upsha=""
    [ -n "$br" ] || br=$(git -C "$dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
    ref="refs/cockpit/reviewed/${br:-HEAD}"
    # Every ref the cases below ask about, from ONE for-each-ref rather than a
    # rev-parse per candidate (seven, before): the checkpoint; the branch's
    # upstream, whose :track is "[gone]" exactly when `--verify @{u}` would
    # refuse it and "" when it sits at HEAD; origin/HEAD's target; main;
    # master. Rows come back sorted by name, not by our preference, so they
    # are collected first and chosen after. A pattern also matches refs BELOW
    # it (refs/heads/main/x when no main exists), hence the exact names.
    while IFS=$'\t' read -r name sha ups track sym; do
        case "$name" in
            "$ref")                   ckpt=$sha ;;
            "refs/heads/$br")         [ -n "$head" ] || head=$sha; bups=$ups; btrack=$track ;;
            refs/remotes/origin/HEAD) ohead=$sym; osha=$sha ;;
            refs/heads/main)          mainsha=$sha ;;
            refs/heads/master)        mastersha=$sha ;;
        esac
    done < <(git -C "$dir" --no-optional-locks for-each-ref \
        --format='%(refname)%09%(objectname)%09%(upstream:short)%09%(upstream:track)%09%(symref:short)' \
        -- "$ref" "refs/heads/$br" refs/remotes/origin/HEAD refs/heads/main refs/heads/master 2>/dev/null)
    case "$mode" in auto|since-review)
        [ -z "$ckpt" ] || { printf '%s\tsince last review\tsince-review\n' "$ckpt"; return 0; } ;;
    esac
    case "$mode" in auto|since-review|branch)
        # The upstream, else origin/HEAD, else main, else master -- and, where
        # the rows already say the merge-base IS HEAD, no merge-base fork: an
        # upstream that is not ahead of us, or a fallback whose sha is HEAD's.
        if [ -n "$bups" ] && [ "$btrack" != '[gone]' ]; then
            up=$bups; case "$btrack" in *ahead*) upsha="" ;; *) upsha=$head ;; esac
        elif [ -n "$ohead" ];     then up=$ohead;  upsha=$osha
        elif [ -n "$mainsha" ];   then up=main;    upsha=$mainsha
        elif [ -n "$mastersha" ]; then up=master;  upsha=$mastersha
        else up=""
        fi
        if [ -n "$up" ] && { [ -z "$head" ] || [ "$upsha" != "$head" ]; }; then
            mb=$(git -C "$dir" --no-optional-locks merge-base "$up" HEAD 2>/dev/null)
            [ -n "$head" ] || head=$(git -C "$dir" --no-optional-locks rev-parse HEAD 2>/dev/null)
            if [ -n "$mb" ] && [ "$mb" != "$head" ]; then
                printf '%s\tvs %s\tbranch\n' "$mb" "$up"; return 0
            fi
        fi ;;
    esac
    printf 'HEAD\tuncommitted\tuncommitted\n'
}

# The mode after CUR among those that hold, wrapping -- ^B's cycle.
next_base() {                 # <dir> <current-mode>
    local dir=$1 cur=$2 m first="" prev="" out
    for m in since-review branch uncommitted; do
        out=$(review_base "$dir" "$m"); [ "${out##*$'\t'}" = "$m" ] || continue
        [ -n "$first" ] || first=$m
        [ -n "$prev" ] && [ "$prev" = "$cur" ] && { printf '%s\n' "$m"; return 0; }
        prev=$m
    done
    printf '%s\n' "$first"
}

# Exit 0 when PANE's tree differs from its review base -- the one question the
# Stop hook asks. Reads the mode the pane's own review chose with ^B, if any.
delta_mode() {                # <pane>
    local pane=$1 dir root sess mode
    dir=$(pane_dir "$pane")
    [ -n "$dir" ] || return 1
    root=$(git_root "$dir")
    git -C "$root" --no-optional-locks rev-parse --is-inside-work-tree >/dev/null 2>&1 || return 1
    sess=$(session_dir "$pane" "$root")
    mode=$(cat -- "$sess/comments.md.base" 2>/dev/null)
    export COCKPIT_REVIEW_BASE=${mode:-auto}
    [ -n "$(tree_diff "$root" --numstat)" ]
}

# --- --lines -------------------------------------------------------------
# Every +/- hunk line of the working tree's diff, one tab-separated row per
# line: path, line, fragment, display. `line` anchors a `+` (or context-side)
# row to the NEW file; a deletion has no line in the new file, so its row
# carries the OLD line number instead, prefixed "-" -- the same (+)/(-) split
# the compose grammar uses for its headers.
#
# Everything uncommitted is ONE set -- staged, unstaged and brand-new files --
# because that is what "review what I have not committed" means. See tree_diff
# below for the base.

# ${#s} counts CHARACTERS in a UTF-8 locale and BYTES in C. The cap below is a
# byte cap and `head -c` is a byte bound, so the length that decides it has to
# be measured in C. In a UTF-8 locale a non-ASCII diff measures up to 4x short,
# and that does not merely miss the cap: it reads a capture that WAS truncated
# at the budget as one that fit, and hands the truncated diff to the parse.
# Same idiom as pad_field() in review-fleet.sh.
bytelen() { local LC_ALL=C; printf '%s\n' "${#1}"; }

lines_mode() {
    local dir=$1
    [ -n "$dir" ] || return 0

    # core.quotePath=false: with git's default (true) a path holding any
    # non-ASCII byte comes out C-quoted and octal-escaped -- `--- "a/caf\303\251.txt"`
    # -- and the header match below (/^--- a\//, /^\+\+\+ b\//) needs a bare
    # `a/`/`b/`, so a quoted header matched neither. The file's every row was
    # then attributed to the PREVIOUS file (or dropped, if it was first): the
    # agent got told to edit café's lines in alpha.txt. Turning quoting off
    # makes the path literal in both the diff headers and numstat, so the two
    # stay keyed on the same name. (A path with a literal tab/quote is still
    # git-quoted even here -- the residual narrow case; non-ASCII is the common
    # one and is fixed.)
    #
    # numstat FIRST and the full diff only after the cap below has let it
    # through: COUNTING is cheap, CAPTURING is what costs. `git diff
    # --numstat` carries every number the cap needs, and on a tree the cap
    # exists to refuse (30 regenerated lockfiles, 6,000,030 changed rows,
    # 38 MB on disk) it is 0.75s and 32 MB against the full capture's 3.25s
    # and 222 MB -- which this used to pay first, to fill a variable it then
    # threw away for a one-row notice.
    local diff numstat
    numstat=$(tree_diff "$dir" --numstat)

    # Untracked files are rows of the same diff: see tree_diff's intent-to-add copy.

    # A giant single-file diff -- a regenerated lockfile, a vendored bundle, a
    # minified asset -- would feed the per-row parse below tens of thousands of
    # rows. The review popup is MODAL and runs lines_mode at startup and on
    # every reload, so that parse locked the one client for minutes to hours
    # (measured: a 100k-row single-file diff did not finish in 5 minutes, while
    # the git diff and the awk themselves cost ~0.03s -- the shell row-build is
    # the whole cost). numstat's own add+del columns already total the exact
    # number of +/- rows this function emits, at the one cheap git call made
    # just above -- so count first and, past a cap, emit a single notice
    # instead of parsing at all. The popup then always appears at once and esc
    # still cancels it. 5000 is well past any diff a human reads line-by-line
    # yet far below where the parse is felt.
    #
    # Rows are not the only way a diff gets too big to hand a reviewer. 4000
    # rows of a 1500-column minified line sit comfortably under the row cap
    # and are still 5.7 MB to capture, parse into shell rows, and render as an
    # fzf list of 1500-column entries. So the same pass caps on BYTES too --
    # 4 MB -- and it MEASURES those bytes rather than estimating them.
    #
    # Two estimators were tried here first and both broke, in opposite
    # directions. `size * (add+del) / lines`, a per-file average, is dominated
    # by the short lines while the diff is dominated by the long one: 100,000
    # one-byte lines beside one 5 MB line estimated 103 bytes and captured
    # 10 MB (0.91s, 82 MB RSS, in the modal popup, for a two-row review).
    # Flooring it with `maxlen * (add+del)` catches that and inverts the
    # error: it charges the widest line in a file to EVERY changed row of it,
    # whatever the change was, and since the cap is a per-review TOTAL one
    # such file takes every other file down with it -- a 744-byte, 26-row
    # review refused as "(6.8 MB)", an unrelated one-line Go change lost along
    # with it. That estimate also needed `wc -L`, and on a wc without it the
    # sized map came back empty and the byte cap vanished in silence (4000
    # rows parsed, 1.02s, where the cap had fired before).
    #
    # There is no estimate that is both safe and fair, because the thing being
    # guessed at -- how many bytes these particular rows are -- is exactly
    # what a diff already knows. So take the real number, and BOUND the taking
    # of it: capture once through `head -c cap+1` and let git die on SIGPIPE
    # past that. A 400 MB diff then costs the 4 MB read that proves it too
    # big, not the 2.06s/177MB a full capture measured -- and when it fits,
    # what came back IS the diff the parse below reads, so nothing is diffed
    # twice and the "capture only after the cap decides" property is kept.
    #
    # numstat still runs first, for the ROW cap: rows can exceed 5000 while
    # the bytes stay under 4 MB (12001 rows of "1\n" is 24 KB), and counting
    # them is the cheap call already made above.
    local cap=5000 bytecap=$(( 4 * 1024 * 1024 )) total=0 bytes=0
    local nadd ndel nfile reason=
    while IFS=$'\t' read -r nadd ndel nfile; do
        [ -n "$nfile" ] || continue
        # A binary file's numstat counts are "-\t-". It contributes no +/- rows
        # to the parse below -- without --binary git prints one "Binary files
        # a/x and b/x differ" line and nothing else, whatever the file weighs
        # -- so it adds nothing to the row total, and its one line is already
        # in the capture like every other byte. This is what keeps a two-line
        # code change beside a rebuilt 5 MB asset reviewable.
        [ "$nadd" = "-" ] && continue
        case "$nadd" in *[!0-9]*) nadd=0 ;; esac
        case "$ndel" in *[!0-9]*) ndel=0 ;; esac
        total=$(( total + nadd + ndel ))
    done <<<"$numstat"
    if [ "$total" -gt "$cap" ]; then
        reason="$total changed lines"
    else
        if [ "$bytes" -gt "$bytecap" ]; then
            reason='> 4 MB'
        else
            # One byte past what is left of the budget: enough to prove the cap
            # is exceeded, never more than the budget itself in memory.
            #
            # The trailing X is what makes the length EXACT. Command
            # substitution strips trailing newlines, so without it a capture
            # that happens to end on one reads a byte short of the budget --
            # and a diff truncated at the budget would then be parsed as a
            # complete one, silently dropping everything past 4 MB and
            # mangling the row it was cut in half on.
            diff=$(tree_diff "$dir" | head -c $(( bytecap - bytes + 1 )); printf X)
            diff=${diff%X}
            if [ "$(bytelen "$diff")" -gt $(( bytecap - bytes )) ]; then
                reason='> 4 MB'
                diff=
            else
                # Back to the shape the old `diff=$(tree_diff ...)` produced,
                # so the parse below sees exactly what it always did.
                diff=${diff%$'\n'}
            fi
        fi
    fi
    if [ -n "$reason" ]; then
        local big="diff too large to anchor line-by-line ($reason) — review it directly"
        # Offset 1 (the preview's first line) so the 5-field split in rows_mode
        # never meets a short row -- this notice has no file to scroll anyway.
        printf '%s\t%s\t%s\t%s\t%s\n' "$CAP_SENTINEL" 0 "$big" "$big" 1
        return 0
    fi

    # The files tree_diff kept away from git, as one notice row on the
    # sentinel every reader already refuses (attach, ^Y, mark_rows).
    local skipped
    skipped=$(big_files "$dir" | awk -F'\t' '{ printf "%s%s (%d MB)", (n++ ? ", " : ""), $2, $1 / 1048576 }')
    if [ -n "$skipped" ]; then
        local note="$skipped skipped — over $(( BIGCAP / 1048576 )) MB, review it directly"
        printf '%s\t%s\t%s\t%s\t%s\n' "$CAP_SENTINEL" 0 "$note" "$note" 1
    fi

    [ -n "$diff" ] || return 0

    # One pass: track the file a hunk belongs to (from the +++/--- header
    # pair -- a whole-file deletion has "+++ /dev/null", so the path has to
    # come from the "--- a/" side instead), the hunk's starting old/new line
    # from "@@ -o,c +o,c @@" (not a capture-group regex -- portable to
    # non-GNU awk), and a running counter per side. Context lines advance both
    # counters and emit nothing; a +/- line emits a row and advances its own
    # side.
    #
    # The -o,c and +o,c tokens are POSITIONAL -- always fields 2 and 3 of the
    # header -- and have to be taken that way, not found by scanning every
    # token for a leading -/+. git appends a funcname context after the
    # closing "@@" ("@@ -2,7 +2,7 @@ fn compute() -> i32 {"), and a scan
    # keeps clobbering oldtok/newtok on any later token that starts with -
    # or + too -- "-> i32" matched and silently became the anchor.
    # Reproduced: a deletion at old line 5 was anchored to -3 instead.
    #
    # Header lines ("--- a/x", "+++ b/x", "@@ ...") exist only BETWEEN hunks,
    # never inside one -- but a body line whose own text starts "-- " or
    # "++ " becomes, once diff prepends its own -/+ marker, "--- ..." or
    # "+++ ..." and a plain /^--- /-style match can't tell the two apart.
    # "@@" needs no such guard: a body line can only ever start "@@" if diff
    # gave it a bare literal first, which it never does. So --- / +++ are
    # only trusted as headers while the hunk's own line budget (oldrem/newrem,
    # from the "@@ -o,c +o,c @@" counts) is exhausted; while budget remains,
    # every line is content, dispatched on its leading character alone, same
    # as any other +/-/context row. Reproduced: an unstaged "-- a sql
    # comment" deletion vanished and every anchor after it drifted; an added
    # "++ ..." line reset the counters and threw every later anchor off too.
    local raw
    raw=$(printf '%s\n' "$diff" | awk '
        function inhunk() { return (oldrem > 0 || newrem > 0) }
        {
            if (inhunk()) {
                # poff is where the row lands in the PREVIEW, not in the
                # file: --preview-diff prints one stat line and then this same
                # diff from the first @@ of that file. So every line inside a
                # hunk -- context, +, -, and the "\ No newline" note alike --
                # occupies one preview line and is counted before it is
                # dispatched, and the +1 at the emit is that stat line.
                poff++
                c = substr($0, 1, 1)
                if (c == "+") {
                    if (file != "") {
                        printf "%s\t%d\t+\t%d\t%s\n", file, newl, poff + 1, substr($0, 2)
                        newl++
                    }
                    newrem--
                } else if (c == "-") {
                    if (file != "") {
                        printf "%s\t%d\t-\t%d\t%s\n", file, oldl, poff + 1, substr($0, 2)
                        oldl++
                    }
                    oldrem--
                } else if (c == " ") {
                    oldl++; newl++; oldrem--; newrem--
                }
                # anything else ("\ No newline at end of file") stays ignored
                next
            }
            if ($0 ~ /^--- /) {
                # git TERMINATES a header path with a tab when the path holds a
                # space ("--- a/has space.txt<TAB>"), and substr keeps it. That
                # tab then rode into every row the file emitted, where -F"\t"
                # read it as an extra empty field: the line number came out
                # blank, the preview offset was glued to the front of the
                # fragment, and the sign column landed in the offset -- so a
                # DELETION on such a path was emitted as an addition. It was
                # invisible while the rows were built by a tab-IFS `read`,
                # which treats tab as IFS whitespace and collapses the doubled
                # one; awk -F"\t" does not. A header path can never
                # legitimately end in a raw tab (git C-quotes those), so
                # stripping one is unambiguous, and doing it here fixes both
                # sides at the source instead of in every consumer.
                if ($0 ~ /^--- a\//) { oldfile = substr($0, 7); sub(/\t$/, "", oldfile) }
                next
            }
            if ($0 ~ /^\+\+\+ /) {
                if ($0 ~ /^\+\+\+ b\//) { file = substr($0, 7); sub(/\t$/, "", file) } else file = oldfile
                # A new file is a new preview, so its offset restarts too.
                oldl = 0; newl = 0; poff = 0; next
            }
            if ($0 ~ /^@@/) {
                split($0, parts, " ")
                oldtok = parts[2]; newtok = parts[3]
                sub(/^-/, "", oldtok); split(oldtok, oa, ","); oldl = oa[1] + 0
                oldrem = (oa[2] == "" ? 1 : oa[2] + 0)
                sub(/^\+/, "", newtok); split(newtok, na, ","); newl = na[1] + 0
                newrem = (na[2] == "" ? 1 : na[2] + 0)
                # The @@ line is where --preview-diff starts printing, so it is
                # preview line 1 of the diff -- count it like any other.
                poff++
                next
            }
        }
    ')

    [ -n "$raw" ] || return 0

    # Churn, the rows, the test-first partition and the ordering: ONE awk over
    # the two streams this function already has, then `sort | cut`. What stood
    # here was four bash passes over the same data -- a `read` loop building a
    # churn map, a second `read` loop building the rows, a loop partitioning
    # the paths, and two emit loops indexing back into a rows array by each
    # file's recorded [start,end] range -- and bash pays an interpreter step
    # per row for every one of them. Measured on a 4900-row single-file diff
    # (the largest shape that stays under the cap), ten runs each: the whole
    # function cost 195-227ms, of which the git diff and the parsing awk --
    # everything up to `raw` -- are 43-52ms. The bash row build alone was the
    # other ~165ms. The same rows out of awk cost ~19ms, 59-69ms end to end.
    # The row build was never the parse; it was bash.
    #
    # Stream 1 is numstat (add, del, path) -- churn is additions + deletions,
    # the risk proxy that orders everything but the test partition. A binary
    # file's counts are "-\t-", not numbers, so they weigh nothing; a path
    # numstat never mentions weighs nothing either (awk reads an unset element
    # as 0, exactly what `${churn[$path]:-0}` did).
    #
    # Stream 2 is the raw rows. `NR == FNR` is safe as the stream test here
    # only because `printf '%s\n' "$numstat"` always emits at least one line,
    # so FNR restarts at 1 for stream 2 while NR does not -- the empty-first-
    # stream trap the estimate awk above has to sidestep with ARGV[1].
    #
    # The text is taken as "everything past the fourth tab", not as $5: a
    # fragment may itself contain a tab and $5 would drop everything after it,
    # where the bash `read -r ... rest` this replaces kept it. (Both then lose
    # it to the control-char strip -- which is the point: the two agree.)
    #
    # LC_ALL=C on the awk, not just on the sort: the 160 cut is a BYTE cut, and
    # `substr` counts bytes in mawk but CHARACTERS in gawk under a UTF-8
    # locale -- the same diff cut at 160 characters on one box and 160 bytes on
    # the next, and `length($1)`-style field arithmetic drifted with it. C
    # makes every awk here byte-oriented, which is also what the shell it
    # replaces counted once the fragment reached printf.
    #
    # Ordering is done by `sort`, not in awk (no `asorti` in POSIX awk, and an
    # in-awk insertion sort is the O(n^2) this is here to delete). Three
    # prepended key columns and one ascending sort do it: the partition (0 for
    # a test file, 1 otherwise), then `999999999 - churn` so ONE ascending
    # sort reads as churn-DESCENDING, then NR to break ties. NR ascending is
    # first-seen order, and git diff never interleaves two files, so every row
    # of a file shares its file's two leading keys and its NRs are contiguous
    # -- the file's group stays whole and internally line-ascending. LC_ALL=C
    # for a byte order that does not move with the locale; `cut` drops the
    # three keys again.
    LC_ALL=C awk -F'\t' '
        NR == FNR {
            if ($1 ~ /^[0-9]+$/) a = $1; else a = 0
            if ($2 ~ /^[0-9]+$/) d = $2; else d = 0
            churn[$3] = a + d
            next
        }
        {
            p = $1
            # tests/*, *_test.*, *.test.* are all substrings of plain "test",
            # so one test covers the whole family -- the *test* glob the shell
            # partition used, ported exactly.
            if (!(p in part)) part[p] = (index(p, "test") > 0 ? 0 : 1)
            # Control-char strip + 160 cap, the agent-state:82-86 discipline:
            # this text reaches an fzf display column and a comment-store
            # header next.
            frag = substr($0, length($1) + length($2) + length($3) + length($4) + 5)
            gsub(/[\001-\037\177]/, "", frag)
            # 160 BYTES, backed off to a character boundary. A blind byte cut
            # ends a fragment mid-sequence -- 200 "e-acute" after one ASCII
            # byte put a lead byte C3 at 160 and its continuation at 161, so
            # the fzf column and the comment-store quote header got an
            # undecodable trailing byte. The byte AFTER the cut says whether
            # the cut split anything: only a continuation byte there means it
            # landed inside a sequence, and then the whole partial sequence at
            # the end goes. A cut that already fell on a boundary is left
            # alone -- trimming it would drop a whole valid character (200
            # "e-acute" with no prefix is exactly 80 characters at 160 bytes).
            # Invalid input cannot loop or corrupt further: with no lead byte
            # to match, the sub does nothing and the 160-byte cut stands.
            if (length(frag) > 160) {
                tail = substr(frag, 161, 1)
                frag = substr(frag, 1, 160)
                if (tail ~ /^[\200-\277]$/) sub(/[\300-\367][\200-\277]*$/, "", frag)
            }
            # A deletion has no line in the new file, so its row carries the
            # OLD number prefixed "-".
            ml = ($3 == "-" ? "-" $2 : $2)
            printf "%d\t%09d\t%09d\t%s\t%s\t%s\t%s:%s (%s)  %s\t%s\n", \
                   part[p], 999999999 - churn[p], NR, \
                   p, ml, frag, p, $2, $3, frag, $4
        }
    ' <(printf '%s\n' "$numstat") <(printf '%s\n' "$raw") \
        | LC_ALL=C sort -t "$(printf '\t')" -k1,1n -k2,2 -k3,3 \
        | cut -f4-
}

# The snapshot: lines_mode's rows, written ONCE per popup, re-marked after.
#
# And on any tree with something to review, that ONE parse is the only one
# this popup pays: it lands in "<store>.rows" and every rows_mode below --
# fzf's own reload() after each pinned comment, and the two rebuilds on the
# keep-reviewing paths -- reads it back and only re-marks it. (A clean tree
# writes an empty snapshot, which rows_mode declines, so that one case
# still parses twice before the "no uncommitted changes" exit below. It has
# nothing to parse.) The snapshot is deliberately NOT refreshed afterwards:
# the rows a reviewer comments on must stay the rows the reviewer SAW, even
# if the agent writes to the tree while the popup is up. (That is the
# phase-0 mitigation for reviewing a live tree; it is not a diff cache that
# outlives anything -- it dies with the store below.)
#
# Guarded on a non-empty store because "$store.rows" with no store is
# ".rows", a real path in whatever directory this was run from.
#
# `umask 077` + `set -C` in the same subshell that does the writing, the
# discipline deliver() already uses for the oversized-review file -- and
# this file needs it MORE, because its name is DERIVABLE. session_dir()
# builds the store's path from the pane id and the repo root's cksum, and
# "<store>.rows" is one suffix away from that -- inside a 0700 dir under
# $HOME, but O_EXCL still earns its keep against a link planted by anyone
# who can write there. The file holds the entire uncommitted diff. A plain
# `>` was wrong twice over: it created the
# snapshot at the caller's umask (0664 here -- the whole diff
# world-readable, while the store beside it is 0600), and it FOLLOWED a
# symlink already sitting at that name, so the diff could be written into
# a file of someone else's choosing and their file truncated to make room.
# noclobber makes the open O_CREAT|O_EXCL, which refuses an existing path
# -- a symlink included, dangling or not -- and umask makes what it does
# create born 0600, with no create-then-chmod window.
#
# The `rm -f` first is for a resumed session's stale snapshot, which set -C
# would otherwise refuse to replace -- but it is guarded, not unconditional:
# cleared only when "$2.rows" is a plain file, so a planted symlink is left
# standing here and still hits the noclobber refusal below, which sweeps it
# up (unlinked, never followed) the same as always.
#
# A refused write is not an error worth stopping for: the `rm -f` unlinks
# whatever was squatting the name (unlink never follows a symlink, so the
# victim is untouched) and leaves no snapshot, and rows_mode simply
# derives the rows from the tree instead. Slower, correct, no popup lost.
snapshot() {                  # <dir> <store>
    [ -n "${2:-}" ] || return 0
    [ -L "$2.rows" ] || rm -f -- "$2.rows"
    (umask 077; set -C; lines_mode "$1" > "$2.rows") 2>/dev/null || rm -f -- "$2.rows"
}

# Comments whose "## path:line (sign)" header matches no row in the
# snapshot: the agent moved the line since the comment was pinned (dev lens
# §2.2). They still deliver -- the quoted fragment is what the agent anchors
# on -- so this only NAMES them. Streams told apart by FILENAME, as in
# mark_rows: an empty store makes NR == FNR lie.
stale_keys() {                # <store> <rows-file>  -> stale headers, one per line
    # A refused snapshot write means "unknown", not "everything moved".
    [ -s "${2:-}" ] || return 0
    local st=${1:-/dev/null} rows=${2:-/dev/null}
    [ -f "$st" ] || st=/dev/null
    [ -f "$rows" ] || rows=/dev/null
    LC_ALL=C awk -F'\t' '
        FILENAME == ARGV[1] { if (substr($0, 1, 3) == "## ") h[++n] = $0; next }
        NF == 0 { next }
        {
            sign = (substr($2, 1, 1) == "-") ? "-" : "+"
            num = $2; sub(/^-/, "", num)
            k["## " $1 ":" num " (" sign ")"] = 1
        }
        # A context ( ) record has no diff row to judge it by: never reported.
        END {
            for (i = 1; i <= n; i++)
                if (h[i] !~ / \( \)$/ && !(h[i] in k)) print h[i]
        }
    ' "$st" "$rows"
}

# --- approve -------------------------------------------------------------------
# refs/cockpit/reviewed/<branch> = the tree the human just looked at, untracked
# files included (a real add -A into a temp index, then write-tree; content IS
# hashed here, once per approve). A ref to a tree object is what makes the
# since-review base a plain `git diff <ref>`; git update-ref accepts it.
# It does NOT die with the branch -- `git branch -D` and `worktree remove`
# leave it standing (reproduced), so a deleted-and-recreated branch of the
# same name inherits a stale checkpoint, and a name that collides on the
# refs/ hierarchy (a branch "feat" after a checkpoint at "feat/x") blocks a
# later approve outright. Purge by hand:
#   git for-each-ref --format='%(refname)' refs/cockpit/reviewed | xargs -n1 git update-ref -d
# The pane's state is the caller's to clear -- this function knows the repo,
# not the pane.
approve() {                   # <dir>
    local dir=$1 idx gd br tree
    idx=$(mktemp) || return 1
    trap 'rm -f -- "$idx"' EXIT; trap 'trap "" HUP INT TERM; exit 143' HUP INT TERM
    gd=$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null) || { rm -f -- "$idx"; trap - EXIT HUP INT TERM; return 1; }
    cp -p -- "$gd/index" "$idx" 2>/dev/null || rm -f -- "$idx"
    br=$(git -C "$dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
    tree=$(GIT_INDEX_FILE=$idx git -C "$dir" add -A 2>/dev/null \
           && GIT_INDEX_FILE=$idx git -C "$dir" write-tree 2>/dev/null)
    rm -f -- "$idx"; trap - EXIT HUP INT TERM
    [ -n "$tree" ] && git -C "$dir" update-ref "refs/cockpit/reviewed/${br:-HEAD}" "$tree" 2>/dev/null
}
