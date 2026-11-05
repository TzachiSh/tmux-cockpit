#!/usr/bin/env bash
# tmux-review's diff engine: the base, the diff, ^A's checkpoint.
# Sourced by tmux-review, never run. Bash 4+.

# Files git would have to hash in full: 100 ms at 32 MB, 26 s at 1 GB, measured.
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

# One `<prefix><path>` NUL record per gitlink in STAGED (the `ls-files -s -z
# --full-name` output) written to OUT, the path byte for byte. PREFIX defaults
# to `:(top,exclude,literal)`, the pathspec that keeps an add out of it.
# A read takes one record, never the list: a shell variable cannot hold a NUL.
# Status 1 when a write fails, or when there was a gitlink and OUT is not a
# file holding its records: the caller must then run no add at all.
gitlink_excludes() {          # <staged> <out> [prefix]
    local r n=0 pre=${3-':(top,exclude,literal)'}
    while IFS= read -r -d '' r; do
        case $r in 160000\ *)
            printf '%s%s\0' "$pre" "${r#*$'\t'}" || return 1; n=$((n + 1)) ;;
        esac
    done < "$1" > "$2" || return 1
    [ "$n" -eq 0 ] || { [ -f "$2" ] && [ -s "$2" ]; }
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
# fresh name), made absolute. The shared temp dir is the agent's to write too,
# and cp writes through a symlink planted there (#435); noclobber refuses a
# symlink to a file raced in at any of the three names.
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
    local -a from=()
    scratch_index "$dir" "$gd" || return 1
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
        # core.quotePath=false, as tui/src/data/gitsafe.rs's diff: literal non-ASCII
        # paths. The nonascii and untracked rows in tests/run.sh pin it.
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
    scratch_done
}

# tree_diff's and approve's way out, on every path: the temp dir gone, the
# traps cleared. Reads the caller's $tmp.
scratch_done() { rm -rf -- "$tmp"; trap - EXIT HUP INT TERM; }

# The private dir and what goes in it, for a caller that adds to a copy of
# the index. Sets the caller's tmp (the dir), idx (the copy, $tmp/index, absent
# when there is no index yet), and from (the add's pathspec options, empty when
# no gitlink is staged), with the traps scratch_done clears. $tmp/staged
# holds the copy's staged entries: read from the copy the add runs on, never
# from the agent's index, which can drop a gitlink in between and with it
# its exclude. Status 1 when the list or the excludes do not land: the dir is
# gone, and the caller must run no add.
scratch_index() {             # <dir> <gitdir>
    local dir=$1 gd=$2
    tmp=$(mktemp -d) || return 1
    case $tmp in /*) ;; *) tmp=$PWD/$tmp ;; esac   # git reads GIT_INDEX_FILE from -C's dir
    trap 'rm -rf -- "$tmp"' EXIT; trap 'trap "" HUP INT TERM; exit 143' HUP INT TERM
    idx=$tmp/index
    { (set -C; cat -- "$gd/index" > "$idx") && touch -r "$gd/index" -- "$idx"; } 2>/dev/null \
        || rm -f -- "$idx"
    (set -C; GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks ls-files -s -z --full-name \
        -- ':(top)' > "$tmp/staged") 2>/dev/null || { scratch_done; return 1; }
    # grep's 1: no record holds "160000 ", so there is no gitlink and no loop
    # to run. An error (2) runs the loop all the same.
    grep -q -a -F -e '160000 ' -- "$tmp/staged" 2>/dev/null
    if [ $? -ne 1 ]; then
        (set -C; gitlink_excludes "$tmp/staged" "$tmp/pathspec") 2>/dev/null \
            || { scratch_done; return 1; }
        [ ! -s "$tmp/pathspec" ] || from=(--pathspec-from-file="$tmp/pathspec" --pathspec-file-nul)
    fi
}

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

# --- approve -------------------------------------------------------------------
# refs/cockpit/reviewed/<branch> = the tree the human just looked at, untracked
# files included, so since-review is a plain `git diff <ref>`. The ref outlives
# `git branch -D`, and a branch "feat" after a checkpoint at "feat/x" collides
# in refs/ and blocks a later approve. Purge by hand:
#   git for-each-ref --format='%(refname)' refs/cockpit/reviewed | xargs -n1 git update-ref -d
# The add runs on a private copy of the index (scratch_index, shared with
# tree_diff) and stays out of every gitlink, whose repo would run its own
# filters (#441). update-index then records each gitlink as add -A did: its
# HEAD, read from its refs with no status run, or the entry dropped when the
# dir is gone. A skip-worktree gitlink stays as it is, as add -A leaves it.
# An empty DIR is refused: git -C "" is the cwd.
approve() {                   # <dir>
    local dir=$1 tmp idx gd top br tree rp
    local -a from=() links=()
    [ -n "$dir" ] || return 1
    mapfile -t rp < <(git -C "$dir" rev-parse --absolute-git-dir --show-toplevel 2>/dev/null)
    gd=${rp[0]-}; top=${rp[1]-}
    [ -n "$top" ] || return 1
    scratch_index "$dir" "$gd" || return 1
    if [ "${#from[@]}" -gt 0 ]; then
        (set -C; gitlink_excludes "$tmp/staged" "$tmp/gitlinks" '') 2>/dev/null \
            || { scratch_done; return 1; }
        links=(-C "$top" update-index -z --remove --ignore-skip-worktree-entries --stdin)
    fi
    br=$(git -C "$dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
    tree=$(GIT_INDEX_FILE=$idx git -C "$dir" add -A ${from[@]+"${from[@]}"} 2>/dev/null \
           && { [ "${#links[@]}" -eq 0 ] \
                || GIT_INDEX_FILE=$idx git "${links[@]}" < "$tmp/gitlinks" 2>/dev/null; } \
           && GIT_INDEX_FILE=$idx git -C "$dir" write-tree 2>/dev/null)
    scratch_done
    [ -n "$tree" ] && git -C "$dir" update-ref "refs/cockpit/reviewed/${br:-HEAD}" "$tree" 2>/dev/null
}
