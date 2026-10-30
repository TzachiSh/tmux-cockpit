#!/usr/bin/env bash
# tmux-review's picker: the --loop fzf session and the verbs its binds re-run.
# Sourced by tmux-review, never run. Bash 4+.

# --- --rows / --attach (internal: the picker's own reentry, tmux-open-path's
# --preview/--pick convention) --------------------------------------------
# --lines rows, marked: a leading "●" on any row whose path:line already has
# a comment block in the store, a space on every other row -- same column
# width either way, so nothing shifts. This is the marker-column trick at
# tmux-open-path:176-183, applied to a display FIELD instead of a preview
# line: prepend, never repaint. Its own subcommand (not inline in run_loop)
# because fzf's reload() re-execs a fresh command; there is nothing else to
# call back into.
rows_mode() {
    local dir=$1 store=$2
    [ -n "$dir" ] || return 0

    # The rows the popup shows are a SNAPSHOT, not a live read. run_loop
    # writes lines_mode's output to "<store>.rows" once when the popup opens
    # and every reload re-marks that file instead of asking git again: the
    # picker is modal, so the tree cannot move under it, and pinning a comment
    # changes only which rows carry a marker. Without the snapshot every
    # pinned comment re-ran the whole git diff + row build -- measured on a
    # 4900-row diff with 500 comments pinned, 306ms per reload against 15ms
    # now, and the list visibly stalled between comments.
    #
    # This is also the phase-0 "review the live tree" mitigation: the rows the
    # reviewer comments on stay the rows the reviewer SAW, even if the agent
    # writes to the tree mid-review. It is not a diff cache -- there is no
    # invalidation and no staleness window past the popup, because the file
    # dies with the store that names it.
    #
    # No snapshot (a bare --rows call, or a run whose store was never opened
    # through the loop) still derives the rows from the tree, and the two
    # produce the same bytes for the same tree.
    #
    # `! -L` beside the `-s`: the write above refuses to follow a planted
    # symlink, and the read refuses to trust one independently of it. Without
    # this, someone who wins the sliver of a race between the failed write and
    # its `rm -f` gets to choose the rows the picker shows -- and every comment
    # the human then pins is anchored to a path THEY named.
    local hdr=/dev/null dlv=/dev/null
    [ -n "$store" ] && [ -f "$store" ] && hdr=$store
    [ -n "$store" ] && [ -f "${store%/*}/delivered.md" ] && dlv=${store%/*}/delivered.md
    if [ -n "$store" ] && [ ! -L "$store.rows" ] && [ -s "$store.rows" ]; then
        mark_rows "$hdr" "$dlv" "$store.rows"
    else
        mark_rows "$hdr" "$dlv" <(lines_mode "$dir")
    fi
}

# The marking itself: ONE awk over the store and the rows, no shell per row.
# What stood here read the store's headers into an associative array and then
# ran a bash loop over every row -- a by-hand tab split, an array lookup, two
# ${var/from/to} substitutions and a printf, an interpreter step apiece. On
# the 4900-row/500-comment shape that loop was ~240ms of the 306ms; the same
# work in awk is ~4ms (68ms end to end against lines_mode's own 64ms floor).
# The bash marking was never the store lookup; it was bash, the same finding
# the row build in lines_mode already made.
#
# Streams are told apart by FILENAME == ARGV[1], not by `NR == FNR`: the store
# can be EMPTY (the first time the popup opens) or non-empty (a resumed
# session), and with an empty first file `NR == FNR` is true for the SECOND
# file's first record too
# -- every row would have been read as a store header and the list would have
# come out blank. Same trap, same sidestep, as the estimate awk in lines_mode.
#
# LC_ALL=C for the same reason lines_mode sets it: every substr/length here is
# meant to be bytes, and gawk under a UTF-8 locale counts characters instead.
#
# Two properties the bash loop had to work for, kept here for free. -F"\t"
# does NOT collapse a run of tabs the way `IFS=$'\t' read` does, so an empty
# fragment (a blank added line) stays an empty field instead of shifting the
# display one column left -- reproduced once as 2 of 18 rows rendering blank.
# And a marker key is an exact string, matching what the `grep -qxF` before it
# matched: "## " is the store's own grammar, written by attach() and nothing
# else, so a substr test on the first three bytes cannot pick up a body line.
#
# The display column is rebuilt here from fields 1-3, not patched out of the
# one lines_mode wrote, so --lines and its goldens never change.
# ponytail: length() is bytes under LC_ALL=C; a non-ASCII path pads a column
# or two short.
mark_rows() {                 # <store-or-/dev/null> <delivered-or-/dev/null> <rows-file>
    # The popup's own width. run_loop exports it, because this function runs
    # again on every fzf reload -- once per pinned comment -- and a tput fork
    # there measured +5ms a call on a 4000-row diff. The fork is the fallback
    # for a bare --rows outside the loop; ncurses still reads the tty there
    # even with stdout on a pipe.
    local cols=${COCKPIT_REVIEW_COLS:-}
    [ -n "$cols" ] || cols=$(tput cols 2>/dev/null || echo 80)
    case $cols in ''|*[!0-9]*) cols=80 ;; esac
    LC_ALL=C awk -F'\t' -v OFS='\t' -v cap="$CAP_SENTINEL" -v cols="$cols" '
        FILENAME == ARGV[1] { if (substr($0, 1, 3) == "## ") c[$0]++; next }
        FILENAME == ARGV[2] { if (substr($0, 1, 3) == "## ") d[$0]++; next }
        NF == 0 { next }
        {
            # A deletion carries the OLD line number prefixed "-", so the
            # marker key has to split the sign back off -- a (+) and a (-) row
            # at the same line number are two different anchors.
            sign = (substr($2, 1, 1) == "-") ? "-" : "+"
            num = $2; sub(/^-/, "", num)
            # One column wide whatever the count: ● for one, the digit past
            # that, + for anything more, ↺ for a row commented in an EARLIER
            # round (delivered, nothing pinned on it now) -- "you said this
            # last time, it is still here". A wrong digit lies, "+" does not.
            key = "## " $1 ":" num " (" sign ")"
            n = c[key] + 0
            m = (n == 0) ? ((key in d) ? "↺" : " ") : (n == 1) ? "●" : (n > 9) ? "+" : n ""
            r++; f1[r] = $1; f2[r] = $2; f3[r] = $3; f5[r] = $5; mk[r] = m; sg[r] = sign
            loc[r] = $1 ":" num; if (length(loc[r]) > w) w = length(loc[r])
        }
        END {
            # Padded to the widest path:line so the signs make one column;
            # a wider one keeps its tail (the file name). The cap is a SHARE
            # of the screen, not the constant 48 it was: at 80 columns that
            # left the diff text 13 to 21 columns and the metadata won the
            # row (#105 defect 6). 35% keeps 40+ columns of code at 80.
            pcap = int(cols * 35 / 100)
            if (pcap > 48) pcap = 48
            if (pcap < 18) pcap = 18
            if (w > pcap) w = pcap
            for (i = 1; i <= r; i++) {
                # The cap row is a notice, not an anchor: rebuilding it would
                # invent a "(diff too large):0" file nothing can be pinned to.
                if (f1[i] == cap) { print f1[i], f2[i], f3[i], f3[i], f5[i]; continue }
                l = loc[i]; if (length(l) > w) l = "…" substr(l, length(l) - w + 4)
                col = (sg[i] == "+") ? "\033[32m+\033[0m" : "\033[31m-\033[0m"
                printf "%s\t%s\t%s\t%s %-" w "s  %s %s\t%s\n", f1[i], f2[i], f3[i], mk[i], l, col, f3[i], f5[i]
            }
        }
    ' "$1" "$2" "$3"
}

# One block, appended -- never rewrite the store, only grow it, so a crash or
# a killed popup mid-review loses at most the comment in flight, not the ones
# already pinned. Grammar is revdiff's, verbatim: "## path:line (+|-)", a
# quoted fragment, the note. A body line starting "## " gets a leading space
# so the parser (compose is a plain cat, but any future reader) can't mistake
# it for a header; "??" needs no handling at all, it passes straight through
# -- the composed header is what tells the agent it is a question.
# The line as the tree (+) or HEAD (-) has it, sign-prefixed line as the rows
# carry it. attach() quotes it; alt-c copies it.
read_line() {                 # <dir> <path> <line-with-sign>
    case "${3#-}" in ''|*[!0-9]*) return 0 ;; esac   # anything else is a sed script
    case "$3" in
        -*) git -C "$1" show "HEAD:$2" 2>/dev/null | sed -n "${3#-}p" ;;
        *)  sed -n "${3}p" -- "$1/$2" 2>/dev/null ;;
    esac
}

attach() {
    local store=$1 path=$2 line=$3 fragment=$4 comment=$5 dir=${6:-}
    # No row under the cursor (an empty list, or the query cleared the match)
    # hands {1} an empty string -- without this guard that still writes
    # "## : (+)" to the store and it delivers, a comment with nothing to
    # anchor to.
    [ -n "$path" ] || return 0
    [ -n "$comment" ] || return 0
    # The cap's own "(diff too large)" row (lines_mode's cap branch) has no
    # real file or line behind it -- alt-enter on it used to write "##
    # (diff too large): (+)" to the store anyway, a comment anchored to
    # nothing that delivered as a bogus instruction. Refuse it here, same as
    # the no-row-under-cursor guard above.
    [ "$path" != "$CAP_SENTINEL" ] || return 0
    local sign num bline
    case "$line" in
        -*) sign="-"; num=${line#-} ;;
        *)  sign="+"; num=$line ;;
    esac
    # The display fragment is cut at 160 bytes for the list; the STORE gets the
    # whole line so the `>` quote matches something in the file -- a truncated
    # quote defeats the anchoring it exists for. Read it back from the tree
    # (+) or HEAD (-); fall back to the fragment on any miss.
    local full="" ft gt
    if [ -n "$dir" ]; then
        full=$(read_line "$dir" "$path" "$line")
        full=${full//[$'\001'-$'\037'$'\177']/}
        full=${full:0:1000}
        # The guard: the line read back from the tree must still START with the
        # fragment fzf had under the cursor, or the file moved under the review
        # and the quote would anchor to the wrong text.
        #
        # Both sides are compared with their LEADING WHITESPACE STRIPPED, because
        # fzf strips it from a {n} field replacement before it ever reaches this
        # script: for `    return 42` the guard used to compare the indented file
        # line against the de-indented fragment, the prefix test failed, full was
        # cleared, and the store fell back to the trimmed 160-byte display
        # fragment. Every line inside a function body is indented, so the whole
        # whole-line quote was inert for real code -- measured: a 224-char
        # indented line stored a 156-char quote with its indentation gone.
        # $full itself stays UNTRIMMED: the indentation is part of the line the
        # agent has to find.
        ft=${full#"${full%%[![:space:]]*}"}
        gt=${fragment#"${fragment%%[![:space:]]*}"}
        [ -n "$full" ] && [ "${ft#"$gt"}" != "$ft" ] || full=""
    fi
    [ -n "$full" ] && fragment=$full
    [ ! -L "$store" ] || { tmux display-message "cockpit: ${store##*/} is a symlink, refusing to write" 2>/dev/null; return 1; }
    {
        printf '## %s:%s (%s)\n' "$path" "$num" "$sign"
        printf '> %s\n' "$fragment"
        while IFS= read -r bline; do
            case "$bline" in
                '## '*) printf ' %s\n' "$bline" ;;
                *)      printf '%s\n' "$bline" ;;
            esac
        done <<<"$comment"
        printf '\n'
    } >> "$store"
}

# The inverse of attach() for one anchor: every record at path:line (sign)
# goes, the rest are written back byte-identical (parse is the declared
# inverse of format -- the round-trip test in tests/run.sh pins it). This
# and nothing else in the script rewrites the store; attach() appends.
# parse_store sets a store that does not parse aside as .bak and says so on
# stderr only; ^X and ^E run under execute-silent, which eats stderr, so the
# marks vanished without a word (#22). The lib stays tmux-free; this says it.
load_store() {                # <store>
    parse_store "$1"
    [ -z "$STORE_BAK" ] \
        || tmux display-message "cockpit: ${1##*/} does not parse; set aside as ${STORE_BAK##*/}" 2>/dev/null || true
}

detach() {                    # <store> <path> <line-with-sign>
    local store=$1 path=$2 line=$3 sign=+ num i j=0 k
    [ -n "$path" ] && [ -n "$line" ] || return 0
    [ -s "$store" ] || { tmux display-message "cockpit: nothing pinned on this row" 2>/dev/null; return 0; }
    case "$line" in -*) sign=-; num=${line#-} ;; *) num=$line ;; esac
    # shellcheck source=lib/review-store.sh
    . "$LIBDIR/review-store.sh" || { tmux display-message "cockpit: cannot find $LIBDIR/review-store.sh" 2>/dev/null; return 1; }
    declare -A STORE=() OUT=(); STORE_N=0
    load_store "$store"
    # A store that fails to parse is moved aside to .bak and STORE_N comes
    # back 0 -- the [ -s "$store" ] guard above already sent a genuinely
    # empty file home, so 0 here can only mean the parse was rejected.
    # Falling through would format_store 0 records over the ORIGINAL path,
    # silently erasing it a second time (past the .bak) under execute-silent.
    [ "$STORE_N" -gt 0 ] || return 0
    for ((i = 0; i < STORE_N; i++)); do
        [ "${STORE[$i.path]}" = "$path" ] && [ "${STORE[$i.line]:-}" = "$num" ] \
            && [ "${STORE[$i.side]}" = "$sign" ] && continue
        for k in path line end side frag body; do
            [ -n "${STORE[$i.$k]+x}" ] && OUT[$j.$k]=${STORE[$i.$k]}
        done
        j=$((j + 1))
    done
    [ "$j" -lt "$STORE_N" ] || { tmux display-message "cockpit: nothing pinned on this row" 2>/dev/null; return 0; }
    STORE=(); for k in "${!OUT[@]}"; do STORE[$k]=${OUT[$k]}; done; STORE_N=$j
    format_store "$store"
}

# Runs inside fzf's execute(): the terminal is the editor's until it exits.
# The file starts as whatever was typed so far, so ^E continues a comment
# rather than restarting it; an editor that leaves it empty pins nothing
# (attach's own rule). Under $TMPDIR, not beside the store: a killed popup
# leaves the editor's file behind, and a leftover inside the session dir
# would block its rmdir for good.
#
# The one editor chain, shared with ^S's "edit the batch": the path picker's
# @cockpit_editor first, then the environment, then vi.
# On a Remote-SSH/Coder host there is no code(1) and usually no $EDITOR, and vi
# in the popup is the wrong answer next to the VS Code window the diff is being
# read in. The shim takes --wait, so the buffer comes back like vi's does.
editor_cmd() {
    local ed
    ed=$(tmux show-options -gqv @cockpit_editor 2>/dev/null)
    ed=${ed:-${VISUAL:-${EDITOR:-}}}
    if [ -z "$ed" ] && vscode_shim >/dev/null; then ed="$SELF --vscode --wait"; fi
    printf '%s\n' "${ed:-vi}"
}

# Both callers of editor_cmd run its answer here. A user's EDITOR is a STRING
# and must word-split (EDITOR="code --wait"), but the shim arm is a path this
# script built out of its own $SELF -- and an install under "~/my dotfiles"
# split that into two words and ran a command that does not exist.
run_editor() {                # <editor-command> <file>
    if [ "$1" = "$SELF --vscode --wait" ]; then
        "$SELF" --vscode --wait "$2"
        return
    fi
    # Word-splitting on purpose: EDITOR="code --wait".
    # shellcheck disable=SC2086
    $1 "$2"
}

# A pane id means nothing to the person reading the confirm; the agent's own
# title (claude sets it) or the window name does, with the id kept for prefix i.
pane_label() {                # <pane-id>
    local t
    t=$(tmux display-message -p -t "$1" "$AGENT_TITLE_FMT" 2>/dev/null)
    printf '%s\n' "${t:+$t }($1)"
}

# Every body pinned on one anchor, so ^E can open what is there. Several
# comments on one row come back as one, blank-line separated. With `frag`,
# the first record's > quote instead: the snapshot the edit must keep.
seed_comment() {              # <store> <path> <line-with-sign> [body|frag]
    local store=$1 path=$2 line=$3 key=${4:-body} sign=+ num i sep=""
    case "$line" in -*) sign=-; num=${line#-} ;; *) num=$line ;; esac
    [ -s "$store" ] || return 0
    # shellcheck source=lib/review-store.sh
    . "$LIBDIR/review-store.sh" || return 0
    declare -A STORE=(); STORE_N=0
    load_store "$store"
    for ((i = 0; i < STORE_N; i++)); do
        [ "${STORE[$i.path]}" = "$path" ] && [ "${STORE[$i.line]:-}" = "$num" ] \
            && [ "${STORE[$i.side]}" = "$sign" ] || continue
        printf '%s%s\n' "$sep" "${STORE[$i.$key]:-}"; sep=$'\n'
        [ "$key" = body ] || break
    done
}

edit_comment() {              # <store> <path> <line> <fragment> <query> <dir>
    local store=$1 path=$2 line=$3 fragment=$4 query=$5 dir=$6 tmp ed
    [ -n "$path" ] && [ -n "$store" ] || return 0
    # ^E on the cap row would otherwise open the editor and then discard the paragraph.
    [ "$path" != "$CAP_SENTINEL" ] || return 0
    # fzf's execute() lends stdin the tty but leaves stdout on the pipe
    # run_loop captures (resnap's rule); vi refused that pipe and ate keys.
    [ -t 0 ] && [ ! -t 1 ] && exec >/dev/tty 2>&1
    tmp=$(umask 077; mktemp "${TMPDIR:-/tmp}/cockpit-review-edit.XXXXXX") || return 0
    local seed="" inplace=0
    if [ -n "$query" ]; then printf '%s\n' "$query" > "$tmp"
    else
        seed=$(seed_comment "$store" "$path" "$line")
        [ -n "$seed" ] && { printf '%s\n' "$seed" > "$tmp"; inplace=1; }
    fi
    ed=$(editor_cmd)
    run_editor "$ed" "$tmp"
    # A buffer that comes back untouched is a cancel (:q) and must not reach the
    # rewrite below: two comments on one row seed ONE buffer, so a ^E someone
    # only looked at would merge them into a single record, ●2 down to ●.
    [ "$inplace" -eq 1 ] && [ "$(cat -- "$tmp")" = "$seed" ] && { rm -f -- "$tmp"; return 0; }
    # Edit in place = the old blocks go, the file comes back as one; an emptied file deletes.
    # The stored quote, not the row's fragment: attach re-reads the live line
    # only while it still starts with what is passed here, so a line the agent
    # moved since the snapshot keeps its old quote instead of taking the new text.
    if [ "$inplace" -eq 1 ]; then
        seed=$(seed_comment "$store" "$path" "$line" frag); [ -n "$seed" ] && fragment=$seed
        detach "$store" "$path" "$line"
    fi
    attach "$store" "$path" "$line" "$fragment" "$(cat -- "$tmp")" "$dir"
    rm -f -- "$tmp"
}

# The store and the row snapshot beside it go together: the snapshot is scratch
# that belongs to ONE popup, and the session dir must not accumulate a
# 4900-row file per review. So the invariant is simply that NO
# snapshot outlives the popup that wrote it: every path that drops the store
# drops both, and the two paths that deliberately KEEP a store -- a refused
# delivery (review_pane, below) and the "could not ask about the pinned
# comments" exit -- still shed the snapshot on its own. A kept store is a
# review someone can re-deliver; a kept snapshot is a diff-sized file nothing
# downstream will ever read again, because --deliver does not know it exists.
#
# Empty store: nothing to remove. `rm -f -- "" ".rows"` would otherwise reach
# for a file actually named ".rows" in the current directory.
drop_rows() {
    [ -n "${1:-}" ] || return 0
    rm -f -- "$1.rows"
}

drop_store() {
    [ -n "${1:-}" ] || return 0
    rm -f -- "$1" "$1.rows" "$1.base"
}

# What another agent needs to discuss a change: the file as it stands, then
# the diff that got it there. Both to stdout; the bind pipes it to the clipboard.
copy_mode() {                 # <dir> [path]
    local dir=$1 path=${2:-}
    [ -n "$dir" ] || return 0
    if [ -z "$path" ]; then tree_diff "$dir"; return 0; fi
    [ "$path" != "$CAP_SENTINEL" ] || return 0
    if [ -f "$dir/$path" ]; then
        printf '### %s\n' "$path"; cat -- "$dir/$path"; printf '\n### diff %s\n' "$path"
    fi
    tree_diff "$dir" -- "$path"
}

# alt-c: fzf hands {+1} then {+2}, every marked row's path then every line, so
# the first half of the arguments are paths and the second half their lines.
# The list row's fragment is cut at 160 bytes and stripped of control
# characters (tabs included); the file has the real line.
copy_lines() {                # <dir> <path>... <line-with-sign>...
    local dir=${1:-} n i; shift
    [ -n "$dir" ] && [ $(( $# % 2 )) -eq 0 ] || return 0
    n=$(( $# / 2 ))
    for ((i = 1; i <= n; i++)); do
        [ "${!i}" != "$CAP_SENTINEL" ] || continue
        read_line "$dir" "${!i}" "${@:i+n:1}"
    done
}

# --- --loop ----------------------------------------------------------------
# tmux-branch's --print-query/--expect shape (tmux-branch:213-243) for the
# keys that LEAVE the picker (ctrl-s delivers, esc/^C discard); tm's
# execute-silent(...)+reload(...) (tm:187) for enter, which must NOT leave
# it -- the list mutates in place, in the same fzf session, so commenting
# five rows in a row costs one popup, not five. Enter pins a comment; it
# never approves -- there is still no accept-all, on purpose (a review whose
# only exit is one Enter on everything is the rubber stamp this exists to
# replace). With an empty query attach() itself is the no-op (it returns 0
# without writing), so no separate dead-key binding is needed. alt-enter
# stays bound to the same action, an alias for terminals that eat plain
# enter's alt chord.
run_loop() {
    local dir=$1 store=$2
    [ -n "$dir" ] || { usage; exit 2; }
    command -v fzf >/dev/null 2>&1 || {
        # display-message, not stderr: inside display-popup -E stderr of a
        # command that exits at once is rendered to nobody.
        tmux display-message "cockpit: the review loop needs fzf" 2>/dev/null; exit 1;
    }
    load_theme || exit 1
    # Read once, here, and inherited by every --rows re-exec through fzf, the
    # same reason COCKPIT_REVIEW_BASE below is exported. The popup is modal, so
    # its width cannot move under the review.
    COCKPIT_REVIEW_COLS=$(tput cols 2>/dev/null || echo 80); export COCKPIT_REVIEW_COLS
    # One argv for every cockpit picker: --ansi (nothing in a row can be dimmed
    # without it), --tabstop=1, the pointer, marker and ellipsis, and the
    # user's own @thm_ colours. The per-picker flags below come after, and the
    # user's options after those -- fzf takes the last occurrence of a flag.
    local -a THEME=() CTHEME=()
    mapfile -t THEME  < <(cockpit_fzf_args highlight-line header-border=bottom)
    mapfile -t CTHEME < <(cockpit_fzf_args highlight-line no-input)
    # One order for every cockpit picker: ↵, alt-↵, ⇥, ^-verbs, ^/, esc. Each
    # line is inside 48 columns, which is what a 70% popup draws at 80.
    local keys
    keys=$'↵ pin · ⇥ mark · ^E edit · ^X delete · ^L list\n^Y copy · alt-c lines · alt-y all · ^R refresh\n^S deliver · ^A approve · ^/ preview · esc leave'
    # Appended, not asked of cockpit_fzf_args: that prints one argument per
    # line for mapfile, so a footer carrying newlines would arrive as three
    # array elements and the last two would reach fzf as stray arguments.
    THEME+=("--footer=$keys")

    # The base this review diffs against: ^B's choice, remembered beside the
    # store so a resumed review keeps it; exported because --rows and --resnap
    # re-exec through fzf and inherit this environment. COCKPIT_REVIEW_BASE_REV
    # (review_base's field 1 -- the actual rev, not the mode word) rides along
    # so the preview subprocess (a separate tmux-open-path invocation, below)
    # diffs against the same base this review does, instead of always HEAD.
    local base_line label mode nxt
    COCKPIT_REVIEW_BASE=$(cat -- "$store.base" 2>/dev/null); export COCKPIT_REVIEW_BASE
    set_base_line() {
        if git -C "$dir" --no-optional-locks rev-parse -q --verify HEAD >/dev/null 2>&1; then
            base_line=$(review_base "$dir" "${COCKPIT_REVIEW_BASE:-auto}")
            label=$(cut -f2 <<<"$base_line"); mode=$(cut -f3 <<<"$base_line")
            COCKPIT_REVIEW_BASE_REV=${base_line%%$'\t'*}
            # next_base wraps, so with only one base holding it hands back the
            # mode we are already on -- a "^B → uncommitted" that goes nowhere.
            nxt=$(next_base "$dir" "$mode"); [ "$nxt" = "$mode" ] && nxt=""
        else
            label=uncommitted; mode=uncommitted
            COCKPIT_REVIEW_BASE_REV=HEAD
            nxt=""
        fi
        export COCKPIT_REVIEW_BASE_REV
    }
    set_base_line

    local rows_cmd attach_cmd preview_cmd resnap_cmd list_cmd detach_cmd edit_cmd copy_cmd copy_all_cmd rows_out nrows
    rows_cmd="$(shq "$SELF") --rows $(shq "$dir") $(shq "$store")"
    attach_cmd="$(shq "$SELF") --attach $(shq "$store") {1} {2} {3} {q} $(shq "$dir")"
    preview_cmd="cd $(shq "$dir") && $(shq "$OPENPATH") --preview-diff {1} {5}"
    resnap_cmd="$(shq "$SELF") --resnap $(shq "$dir") $(shq "$store")"
    list_cmd="$(shq "$SELF") --list $(shq "$store")"
    detach_cmd="$(shq "$SELF") --detach $(shq "$store") {1} {2}"
    edit_cmd="$(shq "$SELF") --edit $(shq "$store") {1} {2} {3} {q} $(shq "$dir")"
    # The cap row copies nothing, and `printf '' | tmux load-buffer -w -` exits 0
    # while LEAVING the old buffer -- so an ungated bind flashes "copied" over
    # whatever was already there. Gate the load and the message on real output.
    copy_cmd="out=\$($(shq "$SELF") --copy $(shq "$dir") {1}); [ -n \"\$out\" ] && printf '%s\n' \"\$out\" | tmux load-buffer -w - && tmux display-message 'cockpit: file + diff copied'"
    copy_all_cmd="$(shq "$SELF") --copy $(shq "$dir") | tmux load-buffer -w - && tmux display-message 'cockpit: the whole diff copied'"
    # {+1} {+2}: the path and line of every ⇥-marked row, or of the cursor row
    # when none is marked. fzf placeholders: they must reach --bind literally,
    # unexpanded. Same output gate as ^Y: the cap row yields nothing.
    copy_lines_cmd="out=\$($(shq "$SELF") --copy-lines $(shq "$dir") {+1} {+2}); [ -n \"\$out\" ] && printf '%s\n' \"\$out\" | tmux load-buffer -w - && tmux display-message 'cockpit: line(s) copied'"

    # Build the initial list ONCE and take the row count from it, rather than
    # running lines_mode a second time just to `wc -l` it: the parse (git diff
    # + the whole row build) used to run twice on every popup open. At startup
    # the store is empty, so rows_mode's marks are all blank and its count is
    # exactly what lines_mode would report. The count is only a presence test
    # now -- the split below is a percentage, so nothing here has to measure
    # the terminal.
    snapshot "$dir" "$store"
    # A resumed session (Task 2's dir found comments already pinned) says so,
    # with the count that matters: how many of them the agent has moved.
    if [ -n "$store" ] && [ -s "$store" ]; then
        local nres nstale
        nres=$(grep -c '^## ' -- "$store"); nstale=$(stale_keys "$store" "$store.rows" | grep -c '^## ')
        tmux display-message "cockpit: resumed $nres pinned comment(s)$([ "$nstale" -gt 0 ] && printf ' · %s no longer anchor, ^R lists' "$nstale")" 2>/dev/null
    fi
    rows_out=$(rows_mode "$dir" "$store")
    nrows=$(printf '%s\n' "$rows_out" | grep -c .)
    if [ "$nrows" -eq 0 ]; then
        tmux display-message "cockpit: no uncommitted changes in $dir" 2>/dev/null
        drop_store "$store"
        exit 0
    fi

    # --disabled: the typed text here is a comment, not a filter. Without it
    # fzf fuzzy-matches whatever you type against the displayed field, and a
    # comment that does not happen to match its own row's text empties the
    # match list -- at which point {1}/{2}/{3} have no row to resolve
    # against and execute-silent gets three empty strings. Found by tracing
    # exactly that: the row under the cursor vanished the moment the typed
    # comment stopped being a substring of the diff line.
    # Not tmux-branch's own ^U/^D for preview scroll here: this list's typed
    # text is a comment, and readline's own ^U/^D already mean something
    # inside it (kill-line, delete-char) -- so scroll moves to alt-u/alt-d
    # instead. ^/ still toggles the preview, same as everywhere else.
    # The picker runs in a LOOP now: a cancel with comments pinned asks first,
    # and "keep reviewing" has to land back in the same list rather than in
    # the caller.
    local out rc key n choice crc ndlv abr ctx
    while :; do
        ndlv=""
        [ -n "$store" ] && [ -f "${store%/*}/delivered.md" ] && ndlv=$(grep -c '^## ' "${store%/*}/delivered.md")
        # The legend that explains column 1 comes FIRST and the root last:
        # fzf cuts a header on the right, and of these the root is the one
        # part a reader can reconstruct from the pane they are sitting in.
        # The delivered count rides the ↺ it belongs to instead of a clause
        # of its own (#105).
        ctx="● pinned · ↺${ndlv:+ $ndlv} sent · base $label${nxt:+ (^B → $nxt)} · ${dir/#"$HOME/"/\~/}"
        out=$(printf '%s\n' "$rows_out" \
            | fzf "${THEME[@]}" --print-query --expect=ctrl-s,ctrl-b,ctrl-a --disabled --multi \
                  --delimiter=$'\t' --with-nth=4 \
                  --reverse --info=inline-right --cycle --scroll-off=2 \
                  --bind "enter:execute-silent(${attach_cmd})+reload(${rows_cmd})+clear-query" \
                  --bind "alt-enter:execute-silent(${attach_cmd})+reload(${rows_cmd})+clear-query" \
                  --bind "ctrl-r:execute(${resnap_cmd})+reload(${rows_cmd})" \
                  --bind "ctrl-x:execute-silent(${detach_cmd})+reload(${rows_cmd})" \
                  --bind "ctrl-l:execute(${list_cmd})" \
                  --bind "ctrl-e:execute(${edit_cmd})+reload(${rows_cmd})+clear-query" \
                  --bind "ctrl-y:execute-silent(${copy_cmd})" \
                  --bind "alt-c:execute-silent(${copy_lines_cmd})" \
                  --bind "alt-y:execute-silent(${copy_all_cmd})" \
                  --bind 'ctrl-/:toggle-preview,alt-u:preview-half-page-up,alt-d:preview-half-page-down' \
                  --bind 'alt-up:preview-up,alt-down:preview-down' \
                  --preview "$preview_cmd" \
                  --preview-window="down,55%,border-top,+{5}-/2" \
                  --prompt='review> ' \
                  --header-first \
                  --header="$ctx")
        rc=$?

        # fzf's own exit codes (tmux-branch:228-231): 0 = an --expect key
        # fired, 1 = no match, 130 = esc/^C. Anything outside {0,1} is a
        # cancel, and a cancel must not leave a stale store some later run
        # reuses -- but it must not silently eat a pinned batch either.
        case "$rc" in
            0|1) ;;
            *)
                # A cancel with pinned comments asks; losing a twenty-comment
                # review to a reflexive esc is the cliff revdiff avoids with a
                # separate discard key. An empty store has nothing to protect,
                # so it still exits straight out.
                if [ -s "$store" ]; then
                    n=$(grep -c '^## ' "$store")
                    choice=$(printf 'leave — %d comment(s) stay pinned\nkeep reviewing\ndiscard %d comment(s)\n' "$n" "$n" \
                        | fzf "${CTHEME[@]}" --reverse --no-info --prompt='esc? ' \
                              --header="$n comment(s) are pinned and not delivered")
                    crc=$?
                    # The confirm picker's OWN exit code decides, not just what
                    # it printed. An fzf that cannot start (no tty -- the popup
                    # torn down under us, a --loop run whose terminal went away)
                    # prints nothing and exits 2, and reading only $choice sent
                    # that through the `*)` arm below, back into `while :`, to
                    # fail again: measured rc=124 under `timeout 5` and 172
                    # "inappropriate ioctl for device" lines in five seconds,
                    # one fork per turn. There is no human here to ask, so the
                    # safe answer is the one every refusal already gives --
                    # keep the store, say where it is, exit 0.
                    case "$crc" in
                        0) ;;              # a row was chosen; $choice decides
                        130) rows_out=$(rows_mode "$dir" "$store"); continue ;;   # same snapshot, re-marked
                        *)  # The popup is over, so its snapshot is scratch --
                            # but the store is a review nobody has seen the
                            # end of, so it stays and gets named.
                            drop_rows "$store"
                            tmux display-message \
                                "cockpit: could not ask about the pinned comments$(keep_hint "" "$store")" \
                                2>/dev/null
                            exit 0 ;;
                    esac
                    case "$choice" in
                        leave*)   drop_rows "$store"; exit 0 ;;     # the dir keeps the store; prefix r resumes it
                        discard*) drop_store "$store"; exit 0 ;;
                        *)  # Back to the list, store kept. Re-mark the rows:
                            # the marks in $rows_out are from before the
                            # comments were pinned, and fzf's own reload only
                            # ever touched the list inside the session that
                            # just exited. This re-marks the SNAPSHOT taken at
                            # open, not the tree -- "keep reviewing" is the
                            # same review, so it has to be the same rows.
                            rows_out=$(rows_mode "$dir" "$store")
                            continue ;;
                    esac
                fi
                drop_store "$store"; exit 0 ;;
        esac

        # Line 1 is the typed query (--print-query); unused here because
        # --disabled already routed it into the comment via {q} inside the
        # alt-enter bind -- the outer loop only needs which --expect key fired.
        key=$(sed -n 2p <<<"$out")
        if [ "$key" = ctrl-b ]; then
            mode=$(next_base "$dir" "$mode")
            printf '%s\n' "$mode" > "$store.base"
            COCKPIT_REVIEW_BASE=$mode; export COCKPIT_REVIEW_BASE
            set_base_line
            # snapshot(), not resnap(): the base just changed, so EVERY row is
            # a fresh anchor against the new diff, not a re-mark of stale ones
            # -- and resnap()'s `read -n 1` would block here, inside the loop,
            # waiting on a key nothing prompted for. Any pinned comment that
            # no longer anchors still shows as stale the next time this store
            # is opened (run_loop's own "resumed" message), same as any other
            # base switch would leave it.
            snapshot "$dir" "$store"
            rows_out=$(rows_mode "$dir" "$store")
            [ "$(printf '%s\n' "$rows_out" | grep -c .)" -gt 0 ] || \
                tmux display-message "cockpit: nothing to review against $label" 2>/dev/null
            continue
        fi
        if [ "$key" = ctrl-a ]; then
            if [ -s "$store" ]; then
                # Not dead work: fzf's own reload() (enter/alt-enter, inside
                # THIS session) marks its internal list live but never touches
                # $rows_out -- a comment pinned since the last computation
                # needs this refresh or the next fzf launch shows a stale,
                # unmarked list. The other two exits below genuinely change
                # nothing about the rows, so they skip it.
                tmux display-message "cockpit: deliver or discard the $(grep -c '^## ' "$store") pinned comment(s) first" 2>/dev/null
                rows_out=$(rows_mode "$dir" "$store"); continue
            fi
            if ! git -C "$dir" --no-optional-locks rev-parse -q --verify HEAD >/dev/null 2>&1; then
                tmux display-message "cockpit: nothing to approve against before the first commit" 2>/dev/null
                continue
            fi
            if approve "$dir"; then exit 21; fi
            abr=$(git -C "$dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null); abr=${abr:-HEAD}
            tmux display-message "cockpit: could not mark $abr reviewed · a stale ref may block it" 2>/dev/null
            continue
        fi
        if [ "$key" = ctrl-s ]; then
            # Once, in the same picker shape esc uses: the consent gesture
            # shows what it is about to send and to whom, and offers the whole
            # batch to $EDITOR first (a comment edited as text -- the grammar
            # is forgiving, compose is cat).
            n=$(grep -c '^## ' "$store")
            [ "$n" -gt 0 ] || { tmux display-message "cockpit: nothing pinned" 2>/dev/null; continue; }
            local to ed e
            to="the agent"; [ -n "$REVIEW_PANE_ID" ] && to=$(pane_label "$REVIEW_PANE_ID")
            ed=$(editor_cmd); e=${ed%% *}; e=${e##*/}
            # The shim arm's first word is this script's own path; "tmux-review"
            # is not the name of the thing the batch would open in.
            case "$ed" in "$SELF"*) e="VS Code" ;; esac
            choice=$(printf 'deliver %d comment(s) to %s\nedit the batch in %s first\nkeep reviewing\n' "$n" "$to" "$e" \
                | fzf "${CTHEME[@]}" --reverse --no-info --prompt='^S? ' --header="deliver $n comment(s)?")
            case $? in 0) ;; *) rows_out=$(rows_mode "$dir" "$store"); continue ;; esac
            case "$choice" in
                deliver*) exit 20 ;;
                edit*)    run_editor "$ed" "$store" </dev/tty >/dev/tty 2>&1
                          rows_out=$(rows_mode "$dir" "$store"); continue ;;
                *)        rows_out=$(rows_mode "$dir" "$store"); continue ;;
            esac
        fi
        # No --expect key matched -- enter/alt-enter stay inside the picker via
        # execute-silent, so this should not happen, but an unmatched exit is a
        # discard, never a silent deliver.
        drop_store "$store"
        exit 0
    done
}

# --- --resnap ----------------------------------------------------------------
# Runs inside fzf's execute(): the terminal is ours until we return, and
# reload() re-marks the new snapshot after. Nothing stale: say nothing and
# hand the terminal straight back. Something stale: name it and wait for a
# key, because reload() will redraw over it the moment we return.
#
# fzf's execute() gives the command the real terminal on stdin (fd0 opens
# /dev/tty) but leaves stdout exactly as fzf's OWN stdout -- and run_loop
# captures fzf's whole stdout into `out=$(... | fzf ...)`, so a plain print
# to stdout here would sit in that capture, invisible until the picker
# exits. `[ -t 0 ]` tells the two calling shapes apart: fzf's execute()
# (stdin is the real tty) prints to /dev/tty instead; a direct CLI call
# (this function's other caller, always with stdin redirected -- a test's
# `</dev/null` included) keeps printing to stdout, which is what pipes it.
resnap() {                    # <dir> <store>
    local dir=$1 store=$2 stale n
    [ -n "$dir" ] && [ -n "$store" ] || { usage; exit 2; }
    snapshot "$dir" "$store"
    stale=$(stale_keys "$store" "$store.rows")
    n=$(printf '%s\n' "$stale" | grep -c '^## ')
    [ "$n" -gt 0 ] || return 0
    if [ -t 0 ]; then
        printf 'refreshed — %d comment(s) no longer anchor to a changed line (they still deliver, quoted):\n%s\n\npress any key\n' "$n" "$stale" > /dev/tty
    else
        printf 'refreshed — %d comment(s) no longer anchor to a changed line (they still deliver, quoted):\n%s\n\npress any key\n' "$n" "$stale"
    fi
    read -r -n 1 -s
    return 0
}

# --- --list ------------------------------------------------------------------
# Every pinned comment, in the terminal, until a key: the store IS the list.
# Not the preview window -- a preview(...) bind needs the window's scroll
# offset reset and put back, and the focus bind that put it back un-hid the
# preview after ^/ (measured on fzf 0.44.1). Same tty rule as resnap().
list_store() {                # <store>
    [ -t 0 ] && exec >/dev/tty
    cat -- "$1" 2>/dev/null
    printf '\n(press any key)\n'
    read -r -n 1 -s
    return 0
}
