#!/usr/bin/env bash
# The one module that knows what "## path:line (+)" means. Sourced, never
# run; pure -- no tmux, no git (tests/run.sh keeps it that way). Bash 4+.
#
#   ## <path>:<line>[-<end>] (+|-)     one record per comment
#   ## <path> (file-level)             a comment on the whole file
#   > <fragment>                       optional: the anchored line, verbatim
#   <body lines>                       a body line that starts "## " is
#                                      written " ## " so it cannot read as
#                                      a header; parse strips that space
#   <blank>                            record separator
#
# This is revdiff's grammar plus the fragment line, so a store round-trips
# through `revdiff --annotations` unchanged and pastes into an agent as-is.
#
# State lives in two globals the caller owns:
#   STORE    assoc array, keys "<i>.path" "<i>.line" "<i>.end" "<i>.side"
#            "<i>.frag" "<i>.body"; end and frag exist only when present
#   STORE_N  record count
# parse_store FILE  fills them. A file that breaks the grammar is moved to
#                   FILE.bak and the state is blank; it never fails (a hook
#                   or a popup must not die on a bad store). Missing = blank.
# format_store FILE writes them back -- to a 0600 temp beside FILE, then one
#                   mv, so a reader never sees half a store.
# Invariant the suite pins: format(parse(format(x))) is byte-identical.

parse_store() {
    local file=$1 line hdr loc i=-1 frag=0 body=0 bad=0
    STORE=(); STORE_N=0
    [ -f "$file" ] || return 0
    while IFS= read -r line || [ -n "$line" ]; do
        case "$line" in
            '## '*)
                hdr=${line#'## '}; i=$((i+1)); STORE[$i.body]=''; frag=0; body=0
                case "$hdr" in
                    *' (file-level)')
                        STORE[$i.path]=${hdr% (file-level)}; STORE[$i.side]='file-level'
                        [ -n "${STORE[$i.path]}" ] || bad=1 ;;
                    *' (+)'|*' (-)')
                        STORE[$i.side]=${hdr: -2:1}; hdr=${hdr% (?)}
                        # The LAST ':' splits path from location: a path may hold ':'.
                        loc=${hdr##*:}; STORE[$i.path]=${hdr%:*}
                        [ "$loc" != "$hdr" ] && [ -n "${STORE[$i.path]}" ] || bad=1
                        case "$loc" in
                            *-*) STORE[$i.line]=${loc%%-*}; STORE[$i.end]=${loc#*-} ;;
                            *)   STORE[$i.line]=$loc ;;
                        esac
                        [[ ${STORE[$i.line]:-} =~ ^[0-9]+$ ]] || bad=1
                        [[ ${STORE[$i.end]-0} =~ ^[0-9]+$ ]] || bad=1 ;;
                    *) bad=1 ;;
                esac ;;
            '> '*)
                # Only the line right after a header is the fragment; later
                # "> " lines are body (a comment may start with a quote).
                if [ $i -ge 0 ] && [ $frag = 0 ] && [ $body = 0 ]; then
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
        mv -f -- "$file" "$file.bak" 2>/dev/null
        printf 'review-store: %s does not parse; set aside as %s.bak\n' "$file" "$file" >&2
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
    tmp=$(umask 077; mktemp "$file.XXXXXX") || return 1
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
