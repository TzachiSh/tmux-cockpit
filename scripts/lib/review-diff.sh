#!/usr/bin/env bash
# tmux-review's diff engine: the base, the diff, the approve checkpoint.
# Sourced by tmux-review, never run. Bash 4+. Source it by a path that names
# its directory: it finds its sibling git-env.sh through BASH_SOURCE.

# tmux-review sources git-env.sh at its top for every mode. The engine does it
# again for a caller that sources it on its own: its git is about the dir it
# is given, never a repo an inherited GIT_DIR names. The pathspec modes go too
# (#673): with GIT_LITERAL_PATHSPECS set, ':(top)' names a file called that, so
# no gitlink is found to keep out of the add and its repo's own clean filter
# runs. ICASE widens each ':(exclude,literal)'.
# shellcheck source=git-env.sh
. "${BASH_SOURCE[0]%/*}/git-env.sh" || return 1

# Files git would have to hash in full: 100 ms at 32 MB, 26 s at 1 GB, measured.
BIGCAP=$(( 32 * 1024 * 1024 ))

# Every list below is NUL-ended records, end to end (#716): a big path may hold
# a newline, and a split one is two wrong excludes that leave git its blob.

# Untracked and stat-modified files over BIGCAP, "w<path>" from DIR. ':(top)':
# ls-files stops at the cwd, git diff does not. One stat for the whole list.
# GNU stat ends each record with a NUL, and the tr swap makes the awk's lines
# of them, with a NUL where a name had a newline. The path is all after the
# size's tab, tabs included. The probe is the flag the call uses: BusyBox has
# -c but no --printf, and a failed stat lists nothing. BusyBox's -c and BSD's
# -f end records with a newline only, so there a big name holding one is still
# split.
big_files() {                 # <dir>
    local dir=$1
    local -a sz=(--printf $'%s\t%n\\0') swap=(tr '\n\0' '\0\n')
    if ! stat --printf %s -- "$dir" >/dev/null 2>&1; then
        swap=(cat); sz=(-c $'%s\t%n')
        stat -c %s -- "$dir" >/dev/null 2>&1 || sz=(-f $'%z\t%N')
    fi
    git -C "$dir" --no-optional-locks ls-files -o -m --exclude-standard -z -- ':(top)' 2>/dev/null \
    | ( cd "$dir" 2>/dev/null || exit; xargs -0 -r stat "${sz[@]}" -- 2>/dev/null ) \
    | "${swap[@]}" \
    | awk -F'\t' -v cap="$BIGCAP" \
          'NF >= 2 && $1 + 0 > cap { print "w" substr($0, index($0, "\t") + 1) }' \
    | tr '\n\0' '\0\n'
}

# Tracked paths whose diff can read a blob over BIGCAP, "t<path>" from the
# root. Three sets, sized together: what differs between REV and HEAD with a
# big blob on either side (#708: a branch or since-review base also diffs what
# the branch committed or deleted), what differs between HEAD and the worktree
# with a big HEAD blob (#716: deleted or shrunk in the worktree reads it), and
# what differs between HEAD and the index with a big blob on either side
# (revdiff's uncommitted view reads the index blob with `git show :path`, a
# staged copy the worktree then shrank or deleted too). big_files sees none of
# them: it sizes worktree files. diff-index on a stat-dirty file prints no sha
# and hashes nothing, so the index needs --cached. The sizes are cat-file's
# object headers: no blob is read. No renames: pairing them reads both blobs. A
# gitlink's sha names no blob, and =all keeps diff-index out of its repo: a
# status there runs its filters.
big_tree_paths() {            # <dir> <rev>
    local m p om nm os ns i
    local -a olds=() news=() paths=()
    local -A blobsz=()
    while IFS= read -r -d '' m && IFS= read -r -d '' p; do
        read -r om nm os ns _ <<<"$m"                  # ":<mode> <mode> <sha> <sha> <status>"
        [ "$om" != :160000 ] && [ "$nm" != 160000 ] || continue
        olds+=("$os"); news+=("$ns"); paths+=("$p")
    done < <({ [ "$2" = HEAD ] \
               || git -C "$1" --no-optional-locks diff-tree -r -z --no-renames "$2" HEAD --
               git -C "$1" --no-optional-locks diff-index -z --no-renames \
                   --ignore-submodules=all HEAD --
               git -C "$1" --no-optional-locks diff-index -z --no-renames --cached \
                   --ignore-submodules=all HEAD --; } 2>/dev/null)
    [ "${#paths[@]}" -gt 0 ] || return 0
    while read -r m p; do
        case $p in ''|*[!0-9]*) ;; *) blobsz[$m]=$p ;; esac       # "<sha> missing" for 000...
    done < <(printf '%s\n' "${olds[@]}" "${news[@]}" | sort -u |
             git -C "$1" --no-optional-locks cat-file \
                 --batch-check='%(objectname) %(objectsize)' 2>/dev/null)
    for i in "${!paths[@]}"; do
        if [ "${blobsz[${olds[i]}]:-0}" -gt "$BIGCAP" ] \
            || [ "${blobsz[${news[i]}]:-0}" -gt "$BIGCAP" ]; then
            printf 't%s\0' "${paths[i]}"
        fi
    done
}

# The one list every reader takes: big_files' records, then with REV
# big_tree_paths', a path once. "w" paths are from DIR, "t" ones from the root,
# and each is printed as it came: the key that finds a twin is from the root,
# or a subdir's untracked "a.bin" hides the root's deleted one.
big_list() {                  # <dir> [rev]
    local r k pre up=-
    local -A seen=()
    while IFS= read -r -d '' r; do
        k=${r#?}
        [ -n "$k" ] || continue
        case $r in w*)                       # DIR's prefix, asked once
            [ "$up" != - ] \
                || up=$(git -C "$1" --no-optional-locks rev-parse --show-prefix 2>/dev/null)
            pre=$up
            while case $k in ../*) ;; *) false ;; esac; do      # ls-files' ../ from DIR
                k=${k#../}; pre=${pre%/}
                case $pre in */*) pre=${pre%/*}/ ;; *) pre="" ;; esac
            done
            k=$pre$k ;;
        esac
        [ -z "${seen[$k]+x}" ] || continue
        seen[$k]=1; printf '%s\0' "$r"
    done < <(big_files "$1"; [ -z "${2:-}" ] || big_tree_paths "$1" "$2")
}

# Sets the caller's rcx to the exclude entries of revdiff's own config file
# (#706): $REVDIFF_CONFIG, else ~/.config/revdiff/config, no XDG lookup
# (revdiff v1.13.0, app/config.go). The format is go-flags' INI: each line
# trimmed, ; and # comments, [section] headers, `key = value` with both sides
# trimmed, one line per entry. The key is `exclude` in any case, or `X`, in no
# section or [Application Options], a name go-flags matches lowercased. Read as
# text, never sourced. Status 0 when the file names an exclude, even an empty or
# skipped one: revdiff then ignores REVDIFF_EXCLUDE. A line revdiff cannot parse
# makes it skip the whole file: status 1 and no entries. A quoted value is
# unquoted when it holds no backslash; one that does is skipped, since Go's
# escapes are not undone here, and so is one past PATH_MAX (4096 bytes): it
# matches no path, and longer ones fill the exec's argv (16 of 128 KiB fail it,
# E2BIG), but still names an exclude. awk parses: a bash trim of a
# long line is seconds per MB. It prints N per exclude key, E<value> per entry,
# R to reject.
revdiff_config_excludes() {
    local f=${REVDIFF_CONFIG:-} r out named=1
    local -a rows=()
    rcx=()
    [ -n "$f" ] || [ -z "${HOME:-}" ] || f=$HOME/.config/revdiff/config
    [ -n "$f" ] && [ -f "$f" ] && [ -r "$f" ] || return 1
    # Captured whole, then split: a read from a pipe takes a byte a syscall.
    { out=$(LC_ALL=C awk '
        function trim(s) { sub(/^[[:space:]]+/, "", s); sub(/[[:space:]]+$/, "", s); return s }
        {
            line = trim($0)
            if (line == "" || line ~ /^[;#]/) next
            if (substr(line, 1, 1) == "[") {          # no closing ]: revdiff errors
                if (substr(line, length(line)) != "]") { print "R"; exit }
                sec = tolower(trim(substr(line, 2, length(line) - 2)))
                if (sec == "") { print "R"; exit }
                next
            }
            i = index(line, "=")
            if (!i) { print "R"; exit }
            k = trim(substr(line, 1, i - 1)); v = trim(substr(line, i + 1)); esc = 0
            if (substr(v, 1, 1) == "\"") {
                if (v ~ /^"[^"\\]*"$/) v = substr(v, 2, length(v) - 2)
                else if (!index(v, "\\")) { print "R"; exit }   # Go Unquote fails too
                else esc = 1
            }
            if (sec != "" && sec != "application options") next
            if (tolower(k) != "exclude" && k != "X") next
            print "N"                                  # PATH_MAX: no longer path to match
            if (!esc && v != "" && length(v) <= 4096) print "E" v
        }' "$f"); } 2>/dev/null
    mapfile -t rows <<<"$out"
    for r in "${rows[@]}"; do
        case $r in
            R) rcx=(); return 1 ;;
            N) named=0 ;;
            E*) rcx+=("${r#E}") ;;
        esac
    done
    return "$named"
}

# revdiff runs its own git (#693): `add -N` on every untracked file it lists,
# then `diff -U1000000` on the one it shows. A 400 MB log hung it at 5.4 GB.
# Its -X drops a path from that list before either runs. Sets the caller's rdx
# to revdiff's file options: --untracked, an --exclude=<path> per big file (the
# = form, so a leading dash is no option), and the user's own excludes, which a
# command-line -X replaces: the config file's, else REVDIFF_EXCLUDE (#706). REV
# is the base revdiff diffs against, HEAD when empty: big_tree_paths' files
# count too. -X matches a whole path, a newline in it included (revdiff lists
# files with -z and trims only the ends), once revdiff has trimmed
# whitespace off its ends, so a big path with whitespace at an end cannot be
# named. Untracked, it is shown only under --untracked: that is left out, and
# the status is 1. Tracked or committed, revdiff may diff it in full whatever
# the options say (at startup if it is first, or when selected): the status is
# 2, and the caller must not start revdiff. DIR is the root:
# revdiff's paths are from it.
revdiff_files() {             # <dir> [rev]
    local r bp e cut=0
    local -a env=() rcx=()
    rdx=()
    while IFS= read -r -d '' r; do
        bp=${r#?}
        case $r in
            t[[:space:]]*|t*[[:space:]]) cut=2 ;;
            w[[:space:]]*|w*[[:space:]])
                [ "$cut" -eq 2 ] || cut=1        # one tracked is enough: no more ls-files
                [ "$cut" -eq 2 ] || [ -z "$(git -C "$1" --no-optional-locks ls-files \
                    -- ":(literal)$bp" 2>/dev/null)" ] || cut=2 ;;
        esac
        rdx+=("--exclude=$bp")
    done < <(big_list "$1" "${2:-HEAD}")
    if [ "${#rdx[@]}" -gt 0 ]; then
        if revdiff_config_excludes; then
            for e in ${rcx[@]+"${rcx[@]}"}; do rdx+=("--exclude=$e"); done
        elif [ -n "${REVDIFF_EXCLUDE:-}" ]; then
            IFS=, read -r -a env <<<"$REVDIFF_EXCLUDE"
            for e in "${env[@]}"; do [ -z "$e" ] || rdx+=("--exclude=$e"); done
        fi
    fi
    [ "$cut" -eq 0 ] || return "$cut"
    rdx+=(--untracked)
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
    local base=""
    if [ -n "$br" ]; then
        base=$(review_base "$dir" "${COCKPIT_REVIEW_BASE:-auto}" "$br" "$head")
        base=${base%%$'\t'*}
    fi
    # Only a pathspec stops git hashing the big files. Exclusions alone still
    # leave the diff whole-worktree wide. Big blobs too (#708, #716): their
    # paths are from the root, so :(top).
    local -a ex=() bigp=()
    case " $* " in *' -- '*) ;; *)
        local r
        while IFS= read -r -d '' r; do
            bigp+=("${r#?}")
            case $r in
                w*) ex+=(":(exclude,literal)${r#?}") ;;
                *)  ex+=(":(top,exclude,literal)${r#?}") ;;
            esac
        done < <(big_list "$dir" "$base")
        [ "${#ex[@]}" -gt 0 ] && ex=(-- "${ex[@]}") ;;
    esac
    if [ -n "$br" ]; then
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
    case $tmp in /*) ;; ?*) tmp=$PWD/$tmp ;; esac   # git reads GIT_INDEX_FILE from -C's dir
    # rm -rf is armed only on an empty dir of the user's own (#943): an empty answer once
    # became $PWD/, and its trap removed the caller's cwd. So: no symlink, the user's, empty
    # as mktemp -d makes it, and neither / nor the cwd. Each test fails closed: an ls that
    # cannot run or read the dir is a refusal, not an empty listing. No trap is set unless all
    # hold.
    local real ents
    real=$(cd -- "$tmp" 2>/dev/null && pwd -P) || real=""
    if [ -z "$real" ] || [ "$real" = / ] || [ "$real" = "$(pwd -P)" ] || [ -L "$tmp" ] \
        || [ ! -O "$tmp" ] || ! ents=$(ls -A -- "$tmp" 2>/dev/null) || [ -n "$ents" ]; then
        tmp=""; return 1
    fi
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
#   since-review  refs/cockpit/reviewed/<branch> exists (a wrote it)   -> only what moved since
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
    dir=$(pane_workdir "$pane")
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
    # Big files stay out of the add as they stay out of tree_diff's (#693): add
    # -A would hash each in full and write it as a blob. One pathspec file, as
    # --pathspec-from-file takes no pathspec beside it. Excludes alone leave the
    # add tree-wide, from a subdir too: git applies them to a pathspec-less add.
    # A failed cat stops the add: it would run without the gitlink excludes.
    # The worktree's big files only: add -A reads no HEAD blob, and excluding a
    # deleted one would keep it in the checkpoint.
    local r nbig=0 lost=0
    {
        [ ! -s "$tmp/pathspec" ] || cat -- "$tmp/pathspec" || lost=1
        while IFS= read -r -d '' r; do
            printf ':(exclude,literal)%s\0' "${r#w}"; nbig=$((nbig + 1))
        done < <(big_list "$dir")
    } > "$tmp/addspec" 2>/dev/null && [ "$lost" -eq 0 ] || { scratch_done; return 1; }
    [ "$nbig" -eq 0 ] || from=(--pathspec-from-file="$tmp/addspec" --pathspec-file-nul)
    br=$(git -C "$dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
    tree=$(GIT_INDEX_FILE=$idx git -C "$dir" add -A ${from[@]+"${from[@]}"} 2>/dev/null \
           && { [ "${#links[@]}" -eq 0 ] \
                || GIT_INDEX_FILE=$idx git "${links[@]}" < "$tmp/gitlinks" 2>/dev/null; } \
           && GIT_INDEX_FILE=$idx git -C "$dir" write-tree 2>/dev/null)
    scratch_done
    [ -n "$tree" ] && git -C "$dir" update-ref "refs/cockpit/reviewed/${br:-HEAD}" "$tree" 2>/dev/null
}
