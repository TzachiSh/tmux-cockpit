#!/usr/bin/env bash
# Review store: revdiff's annotation grammar plus a fragment line. Sourced; pure.
#   "## <path>:<line>[-<end>] (+|-| )" (a space is a context line) or
#   "## <path> (file-level)", an optional "> <fragment>", body lines
#   (a body "## " is stored " ## "), a blank line.
# parse_store FILE fills STORE ("<i>.path|line|end|side|frag|body") and STORE_N;
# it never fails: a bad file is set aside as STORE_BAK, or left in place with STORE_BAK
# empty when no temp file is taken. format_store writes back.
# parse_store_revdiff FILE reads revdiff's own output: it has no fragment line,
# so a leading "> " body line stays body. parse_store keeps the fragment rule.

# Each temp file here is taken only when mktemp's answer is a new, empty file of the user's
# (#1322): the backup gets the bad store moved over it, and format_store's is renamed over
# the store.
# shellcheck source=tmpdir.sh
. "${BASH_SOURCE[0]%/*}/tmpdir.sh" || return 1

parse_store() { _parse_store "$1" 0; }
parse_store_revdiff() { _parse_store "$1" 1; }

_parse_store() {
    local file=$1 ingest=$2 line hdr loc i=-1 frag=0 body=0 bad=0
    STORE=(); STORE_N=0; STORE_BAK=
    [ -f "$file" ] || return 0
    while IFS= read -r line || [ -n "$line" ]; do
        case "$line" in
            '## '*)
                hdr=${line#'## '}; i=$((i+1)); STORE[$i.body]=''; frag=0; body=0
                case "$hdr" in
                    *' (file-level)')
                        STORE[$i.path]=${hdr% (file-level)}; STORE[$i.side]='file-level'
                        [ -n "${STORE[$i.path]}" ] || bad=1 ;;
                    *' (+)'|*' (-)'|*' ( )')
                        STORE[$i.side]=${hdr: -2:1}; hdr=${hdr% (?)}
                        # The LAST ':' splits path from location: a path may hold ':'.
                        loc=${hdr##*:}; STORE[$i.path]=${hdr%:*}
                        [ "$loc" != "$hdr" ] && [ -n "${STORE[$i.path]}" ] || bad=1
                        case "$loc" in
                            *-*) STORE[$i.line]=${loc%%-*}; STORE[$i.end]=${loc#*-} ;;
                            *)   STORE[$i.line]=$loc ;;
                        esac
                        # ASCII digits: in a UTF-8 locale [0-9] also matches ² and ١ (#842)
                        [[ ${STORE[$i.line]:-} =~ ^[0123456789]+$ ]] || bad=1
                        [[ ${STORE[$i.end]-0} =~ ^[0123456789]+$ ]] || bad=1 ;;
                    *) bad=1 ;;
                esac ;;
            '> '*)
                # Only the line right after a line header is the fragment; any
                # other "> " line is body (a comment may start with a quote).
                if [ $ingest = 0 ] && [ $i -ge 0 ] && [ $frag = 0 ] && [ $body = 0 ] \
                   && [ "${STORE[$i.side]}" != file-level ]; then
                    STORE[$i.frag]=${line#'> '}; frag=1
                else
                    [ $i -ge 0 ] || bad=1; STORE[$i.body]+="$line"$'\n'; body=1
                fi ;;
            '')
                [ $i -ge 0 ] && { STORE[$i.body]+=$'\n'; body=1; } ;;
            *)
                [ $i -ge 0 ] || { bad=1; continue; }
                [[ $line == ' ## '* ]] && line=${line# }
                STORE[$i.body]+="$line"$'\n'; body=1 ;;
        esac
    done < "$file"
    if [ $bad = 1 ]; then
        STORE=(); STORE_N=0
        # A fresh name: a second bad store must not overwrite the first one's backup.
        if own_tmpfile STORE_BAK "$file.bak.XXXXXX"; then
            mv -f -- "$file" "$STORE_BAK" 2>/dev/null
            printf 'review-store: %s does not parse; set aside as %s\n' "$file" "$STORE_BAK" >&2
        else
            printf 'review-store: %s does not parse; left in place (own_tmpfile: %s)\n' \
                "$file" "$own_tmpfile_why" >&2
        fi
        return 0
    fi
    STORE_N=$((i+1))
    # The separator and any trailing blank lines are not part of a body.
    for ((i=0; i<STORE_N; i++)); do
        while [[ ${STORE[$i.body]} == *$'\n' ]]; do STORE[$i.body]=${STORE[$i.body]%$'\n'}; done
    done
    return 0
}

format_store() {
    local file=$1 tmp i bline
    own_tmpfile tmp "$file.XXXXXX" || {
        printf 'review-store: %s left as it was (own_tmpfile: %s)\n' "$file" "$own_tmpfile_why" >&2
        return 1; }
    {
        for ((i=0; i<STORE_N; i++)); do
            if [ "${STORE[$i.side]:-}" = file-level ]; then
                printf '## %s (file-level)\n' "${STORE[$i.path]}"
            else
                printf '## %s:%s%s (%s)\n' "${STORE[$i.path]}" "${STORE[$i.line]}" \
                    "${STORE[$i.end]:+-${STORE[$i.end]}}" "${STORE[$i.side]}"
            fi
            [ -n "${STORE[$i.frag]+x}" ] && printf '> %s\n' "${STORE[$i.frag]}"
            if [ -n "${STORE[$i.body]:-}" ]; then
                while IFS= read -r bline; do
                    [[ $bline == '## '* ]] && bline=" $bline"
                    printf '%s\n' "$bline"
                done <<<"${STORE[$i.body]}"
            fi
            printf '\n'
        done
    } > "$tmp" && mv -f -- "$tmp" "$file"
}
