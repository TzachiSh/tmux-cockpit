#!/usr/bin/env bash
# tmux-review's diff engine: the base, the diff, the --lines rows, ^A's checkpoint.
# Sourced by tmux-review, never run. Bash 4+.

# Files git would have to hash in full: 100 ms at 32 MB, 26 s at 1 GB, measured.
# Not the 4 MB byte cap below -- a 4.6 MB lockfile bump is two rows and must stay.
BIGCAP=$(( 32 * 1024 * 1024 ))

# Untracked and stat-modified files over BIGCAP, "<bytes>\t<path>". ':(top)':
# ls-files stops at the cwd, git diff does not. One stat for the whole list.
big_files() {                 # <dir>
    local dir=$1
    local -a sz=(-c $'%s\t%n')          # GNU; BSD spells the same two fields -f '%z\t%N'
    stat -c %s -- "$dir" >/dev/null 2>&1 || sz=(-f $'%z\t%N')
    git -C "$dir" --no-optional-locks ls-files -o -m --exclude-standard -z -- ':(top)' 2>/dev/null \
    | ( cd "$dir" 2>/dev/null || exit; xargs -0 -r stat "${sz[@]}" -- 2>/dev/null ) \
    | awk -F'\t' -v cap="$BIGCAP" 'NF == 2 && $1 + 0 > cap'
}

# One `:(top,exclude,literal)<path>` NUL record per gitlink in the
# `ls-files -s -z --full-name` output on stdin, the path byte for byte. A read
# takes one record, never the list: a shell variable cannot hold a NUL.
gitlink_excludes() {
    local r
    while IFS= read -r -d '' r; do
        case $r in 160000\ *) printf ':(top,exclude,literal)%s\0' "${r#*$'\t'}" ;; esac
    done
}

# --- the base every review diffs against --------------------------------------
# One `git diff <base>` covers staged, unstaged and untracked, against a COPY of
# the index that `add -N -A` extended with intent-to-add entries. The copy keeps
# the agent's own index untouched, and keeps its mtime: git's racy-timestamp
# guard compares entry mtimes with the index file's, and a fresh mtime hid
# same-second edits.
# The -N entries also make an approved tree (untracked files included) diff by
# content instead of reporting those files as deleted.
# --no-optional-locks everywhere: even a read can take index.lock in a repo an
# agent is working in.
# The copy and the gitlink excludes live in a private dir (mktemp -d: 0700, a
# fresh name). The shared temp dir is the agent's to write too, and cp writes
# through a symlink planted there (#435); noclobber refuses one raced in.
# A gitlink's repo has its own config, filters included (#434), and add and
# diff each run status inside it. add has no --ignore-submodules, so only a
# pathspec keeps it out; the diff ignores the dirt, never a moved pointer.
tree_diff() {
    local dir=$1; shift
    local tmp idx gd head br rp
    # rev-parse carries on past what it cannot resolve: an unborn HEAD gives two
    # lines (git dir, "HEAD"), a born one three.
    mapfile -t rp < <(git -C "$dir" --no-optional-locks rev-parse --absolute-git-dir HEAD --abbrev-ref HEAD 2>/dev/null)
    gd=${rp[0]-}; head=${rp[1]-}; br=${rp[2]-}
    [ -n "$gd" ] || return 1
    tmp=$(mktemp -d) || return 1
    trap 'rm -rf -- "$tmp"' EXIT; trap 'trap "" HUP INT TERM; exit 143' HUP INT TERM
    idx=$tmp/index
    { (set -C; cat -- "$gd/index" > "$idx") && touch -r "$gd/index" -- "$idx"; } 2>/dev/null || rm -f -- "$idx"
    local -a from=()
    git -C "$dir" --no-optional-locks ls-files -s -z --full-name -- ':(top)' > "$tmp/staged" 2>/dev/null \
        || { tree_diff_done; return 1; }
    # grep's 1: no record holds "160000 ", so there is no gitlink and no loop
    # to run. An error (2) runs the loop all the same.
    grep -q -a -F -e '160000 ' -- "$tmp/staged" 2>/dev/null
    if [ $? -ne 1 ]; then
        gitlink_excludes < "$tmp/staged" > "$tmp/pathspec" \
            || { tree_diff_done; return 1; }
        [ ! -s "$tmp/pathspec" ] || from=(--pathspec-from-file="$tmp/pathspec" --pathspec-file-nul)
    fi
    GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks add -N -A ${from[@]+"${from[@]}"} 2>/dev/null
    # Only a pathspec stops git hashing the big files. Exclusions alone still
    # leave the diff whole-worktree wide.
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
            diff --no-color --no-ext-diff --ignore-submodules=dirty "$base" "$@" "${ex[@]+"${ex[@]}"}" 2>/dev/null
    else
        local out
        out=$(GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks -c core.quotePath=false diff --no-color --no-ext-diff --ignore-submodules=dirty "$@" "${ex[@]+"${ex[@]}"}" 2>/dev/null)
        [ -n "$out" ] || out=$(GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks -c core.quotePath=false diff --no-color --no-ext-diff --ignore-submodules=dirty --cached "$@" "${ex[@]+"${ex[@]}"}" 2>/dev/null)
        printf '%s\n' "$out"
    fi
    # The binary-file numstat row git would have printed: without it a tree whose
    # only change is a big file reads clean to --delta.
    [ "${#bigp[@]}" -eq 0 ] || case " $* " in *' --numstat '*) printf -- '-\t-\t%s\n' "${bigp[@]}" ;; esac
    tree_diff_done
}

# tree_diff's way out, on every path: its temp dir gone, its traps cleared.
# Reads the caller's $tmp.
tree_diff_done() { rm -rf -- "$tmp"; trap - EXIT HUP INT TERM; }

# --- which base ---------------------------------------------------------------
# The first case that holds:
#   since-review  refs/cockpit/reviewed/<branch> exists (^A wrote it)  -> only what moved since
#   branch        merge-base of the upstream (else origin/HEAD, else main/master) and HEAD,
#                 when that is not HEAD itself                          -> the branch + uncommitted
#   uncommitted   HEAD                                                  -> today's view
# A forced MODE that does not hold falls through, so the printed mode is the one
# that held -- next_base relies on that. Prints "<rev>\t<label>\t<mode>".
# Never called on an unborn HEAD. tui/src/data/info.rs mirrors this.
review_base() {               # <dir> [auto|since-review|branch|uncommitted] [branch] [head-sha]
    local dir=$1 mode=${2:-auto} br=${3:-} head=${4:-} ref up mb
    local name sha ups track sym ckpt="" bups="" btrack="" ohead="" osha="" mainsha="" mastersha="" upsha=""
    [ -n "$br" ] || br=$(git -C "$dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
    ref="refs/cockpit/reviewed/${br:-HEAD}"
    # One for-each-ref for every candidate; exact names, since a pattern also
    # matches refs below it. :track is "[gone]" when `--verify @{u}` would fail.
    # US, not tab: tab is IFS whitespace and folds origin/HEAD's empty fields.
    while IFS=$'\037' read -r name sha ups track sym; do
        case "$name" in
            "$ref")                   ckpt=$sha ;;
            "refs/heads/$br")         [ -n "$head" ] || head=$sha; bups=$ups; btrack=$track ;;
            refs/remotes/origin/HEAD) ohead=$sym; osha=$sha ;;
            refs/heads/main)          mainsha=$sha ;;
            refs/heads/master)        mastersha=$sha ;;
        esac
    done < <(git -C "$dir" --no-optional-locks for-each-ref \
        --format='%(refname)%1f%(objectname)%1f%(upstream:short)%1f%(upstream:track)%1f%(symref:short)' \
        -- "$ref" "refs/heads/$br" refs/remotes/origin/HEAD refs/heads/main refs/heads/master 2>/dev/null)
    case "$mode" in auto|since-review)
        [ -z "$ckpt" ] || { printf '%s\tsince last review\tsince-review\n' "$ckpt"; return 0; } ;;
    esac
    case "$mode" in auto|since-review|branch)
        # No merge-base fork where the rows already say it is HEAD: an upstream
        # not ahead of us, or a fallback whose sha is HEAD's.
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

# The mode after CUR among those that hold, wrapping -- b's cycle at the prompt.
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

# Exit 0 when PANE's tree differs from its review base -- the Stop hook's
# question. Reads the mode the pane's own review chose with b, if any.
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
# One tab-separated row per +/- line of the diff: path, line, fragment, display.
# A deletion has no line in the new file, so it carries the old number as "-N".

# ${#s} counts characters in a UTF-8 locale; the cap is in bytes, as `head -c` is.
bytelen() { local LC_ALL=C; printf '%s\n' "${#1}"; }

lines_mode() {
    local dir=$1
    [ -n "$dir" ] || return 0
    # Rows only on stdout: the refusal goes to stderr, with a status a reader can test.
    if git_refused "$dir"; then printf '%s\n' "$GIT_REFUSAL" >&2; return 1; fi

    # core.quotePath=false (in tree_diff): a quoted non-ASCII header matches
    # neither /^--- a\// nor /^\+\+\+ b\//, and its rows went to the previous file.
    # A path with a literal tab or quote is still git-quoted.
    # numstat first: counting is cheap, capturing is what the caps below avoid.
    local diff numstat
    numstat=$(tree_diff "$dir" --numstat)

    # Two caps, so the modal popup always opens at once: 5000 rows (numstat's
    # add+del is the exact row count), and 4 MB of diff. The bytes are measured,
    # not estimated -- both estimators tried broke, in opposite directions --
    # by capturing through `head -c cap+1`, so a 400 MB diff costs a 4 MB read.
    local cap=5000 bytecap=$(( 4 * 1024 * 1024 )) total=0
    local nadd ndel nfile reason=
    while IFS=$'\t' read -r nadd ndel nfile; do
        [ -n "$nfile" ] || continue
        # Binary ("-\t-"): one "Binary files differ" line, no rows.
        [ "$nadd" = "-" ] && continue
        case "$nadd" in *[!0-9]*) nadd=0 ;; esac
        case "$ndel" in *[!0-9]*) ndel=0 ;; esac
        total=$(( total + nadd + ndel ))
    done <<<"$numstat"
    if [ "$total" -gt "$cap" ]; then
        reason="$total changed lines"
    else
        # The trailing X keeps the length exact: $(...) strips trailing
        # newlines, and a truncated capture would read as one that fit.
        diff=$(tree_diff "$dir" | head -c $(( bytecap + 1 )); printf X)
        diff=${diff%X}
        if [ "$(bytelen "$diff")" -gt "$bytecap" ]; then
            reason='> 4 MB'
            diff=
        else
            diff=${diff%$'\n'}
        fi
    fi
    if [ -n "$reason" ]; then
        local big="diff too large to anchor line-by-line ($reason) — review it directly"
        printf '%s\t%s\t%s\t%s\n' "$CAP_SENTINEL" 0 "$big" "$big"
        return 0
    fi

    # The files tree_diff kept away from git, as one row on the sentinel.
    local skipped
    skipped=$(big_files "$dir" | awk -F'\t' '{ printf "%s%s (%d MB)", (n++ ? ", " : ""), $2, $1 / 1048576 }')
    if [ -n "$skipped" ]; then
        local note="$skipped skipped — over $(( BIGCAP / 1048576 )) MB, review it directly"
        printf '%s\t%s\t%s\t%s\n' "$CAP_SENTINEL" 0 "$note" "$note"
    fi

    [ -n "$diff" ] || return 0

    # One pass over the diff. A whole-file deletion has "+++ /dev/null", so its
    # path comes from "--- a/". The @@ counts are fields 2 and 3, positionally:
    # a funcname after the closing @@ ("-> i32") once got scanned as the anchor.
    # --- and +++ are headers only once the hunk's line budget is spent: a body
    # line "-- x" or "++ x" reads "--- x" / "+++ x" with its diff marker.
    local raw
    raw=$(printf '%s\n' "$diff" | awk '
        function inhunk() { return (oldrem > 0 || newrem > 0) }
        {
            if (inhunk()) {
                c = substr($0, 1, 1)
                if (c == "+") {
                    if (file != "") {
                        printf "%s\t%d\t+\t%s\n", file, newl, substr($0, 2)
                        newl++
                    }
                    newrem--
                } else if (c == "-") {
                    if (file != "") {
                        printf "%s\t%d\t-\t%s\n", file, oldl, substr($0, 2)
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
                # git ends a header path holding a space with a tab; left in, it
                # shifted every field of its rows. A real tab is C-quoted.
                if ($0 ~ /^--- a\//) { oldfile = substr($0, 7); sub(/\t$/, "", oldfile) }
                next
            }
            if ($0 ~ /^\+\+\+ /) {
                if ($0 ~ /^\+\+\+ b\//) { file = substr($0, 7); sub(/\t$/, "", file) } else file = oldfile
                oldl = 0; newl = 0; next
            }
            if ($0 ~ /^@@/) {
                split($0, parts, " ")
                oldtok = parts[2]; newtok = parts[3]
                sub(/^-/, "", oldtok); split(oldtok, oa, ","); oldl = oa[1] + 0
                oldrem = (oa[2] == "" ? 1 : oa[2] + 0)
                sub(/^\+/, "", newtok); split(newtok, na, ","); newl = na[1] + 0
                newrem = (na[2] == "" ? 1 : na[2] + 0)
                next
            }
        }
    ')

    [ -n "$raw" ] || return 0

    # Churn, the rows, test files first, then by churn descending: one awk, then
    # `sort | cut` (bash per-row loops cost ~165 ms on 4900 rows; this ~19 ms).
    # Stream 1 is numstat, stream 2 the raw rows; `NR == FNR` is safe because
    # the first stream always has a line. LC_ALL=C: gawk's substr counts
    # characters under UTF-8, and the 160 cut is in bytes.
    LC_ALL=C awk -F'\t' '
        NR == FNR {
            if ($1 ~ /^[0-9]+$/) a = $1; else a = 0
            if ($2 ~ /^[0-9]+$/) d = $2; else d = 0
            churn[$3] = a + d
            next
        }
        {
            p = $1
            if (!(p in part)) part[p] = (index(p, "test") > 0 ? 0 : 1)
            # Past the third tab, not $4: a fragment may hold a tab.
            frag = substr($0, length($1) + length($2) + length($3) + 4)
            # Untrusted text bound for a display column and a store header: no escapes.
            gsub(/[\001-\037\177]/, "", frag)
            # 160 bytes, backed off to a character boundary when the cut split
            # a UTF-8 sequence (the byte after it is a continuation byte).
            if (length(frag) > 160) {
                tail = substr(frag, 161, 1)
                frag = substr(frag, 1, 160)
                if (tail ~ /^[\200-\277]$/) sub(/[\300-\367][\200-\277]*$/, "", frag)
            }
            ml = ($3 == "-" ? "-" $2 : $2)
            # Sort keys: test partition, 999999999 - churn (so ascending reads
            # descending), NR (first-seen order keeps each file whole).
            printf "%d\t%09d\t%09d\t%s\t%s\t%s\t%s:%s (%s)  %s\n", \
                   part[p], 999999999 - churn[p], NR, \
                   p, ml, frag, p, $2, $3, frag
        }
    ' <(printf '%s\n' "$numstat") <(printf '%s\n' "$raw") \
        | LC_ALL=C sort -t "$(printf '\t')" -k1,1n -k2,2 -k3,3 \
        | cut -f4-
}

# --- approve -------------------------------------------------------------------
# refs/cockpit/reviewed/<branch> = the tree the human just looked at, untracked
# files included, so since-review is a plain `git diff <ref>`. The ref outlives
# `git branch -D`, and a branch "feat" after a checkpoint at "feat/x" collides
# in refs/ and blocks a later approve. Purge by hand:
#   git for-each-ref --format='%(refname)' refs/cockpit/reviewed | xargs -n1 git update-ref -d
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
