#!/usr/bin/env bash
# The line that loads the plugin from a tmux config, for a plugin folder at any path.
# Sourced by bootstrap.sh, which appends it to a config, and install.sh, which prints it.
#
# tmux hands a run-shell argument to sh, so the path must reach sh as one quoted word:
# bare, sh splits it at a blank and runs a $( ) in it (#1043, #1046). The word then sits in
# a tmux double-quoted string, where \ " and $ are escaped. run-shell expands tmux formats
# in its argument, and doubling a # does not hide it there: tmux copies a ## in front of a [
# through as it is. So each # leaves sh's quotes as \#, which puts a ' after every # for
# tmux, and #' is no format.
cockpit_conf_line() {         # <plugin folder> -> run-shell "'<folder>/cockpit.tmux'"
    local p="$1/cockpit.tmux" q="'\\''"
    p="'${p//\'/$q}'"
    p=${p//\\/\\\\}; p=${p//\"/\\\"}; p=${p//\$/\\\$}; p=${p//#/\'\\\\#\'}
    printf 'run-shell "%s"' "$p"
}

# Does a bare line, run-shell <folder>/cockpit.tmux, load the plugin from this folder?
# Measured on tmux 3.6 and 3.8, one character at a time (#1046): sh splits or reads the
# characters in bad, and run-shell expands tmux formats. A run of # in front of a [ arrives
# as it is. Any other ## becomes one #, and a # in front of { , or one of tmux's one-letter
# names (#S, #W…) is replaced. A # before anything else, and every other printing
# character, arrive whole.
cockpit_bare_carries() {      # <path> -> 0 when a bare line carries it
    local bad=$' \t\n"$&\'()*;<>?\\`|}' r=$1 h
    case "$r" in *["$bad"]*|*[[:cntrl:]]*) return 1 ;; esac
    while :; do
        case "$r" in *'#'*) ;; *) return 0 ;; esac
        r=${r#*'#'}; h=${r%%[!#]*}; r=${r#"$h"}       # h: the rest of this run of #
        case "$r" in
            '['*) ;;
            [{,DFHIPSTWh]*) return 1 ;;
            *) [ -z "$h" ] || return 1 ;;
        esac
    done
}

# The number of the first line in a config that loads this folder's plugin bare:
# run-shell, or run, an optional -b, then the path with no quote around it, written out
# or with ~ for HOME. Nothing is printed, and 1 returned, when no line has that shape.
cockpit_conf_bare_line() {    # <config file> <plugin folder> -> line number
    local f=$1 p="$2/cockpit.tmux" t='' l w n=0
    case "$2" in "${HOME:-/nonexistent}"/*) t="~${p#"$HOME"}" ;; esac
    while IFS= read -r l || [ -n "$l" ]; do
        n=$((n + 1))
        l=${l#"${l%%[![:space:]]*}"}; l=${l%"${l##*[![:space:]]}"}
        for w in 'run-shell -b ' 'run-shell ' 'run -b ' 'run '; do
            case "$l" in
                "$w"*)
                    l=${l#"$w"}; l=${l#"${l%%[![:space:]]*}"}
                    if [ "$l" = "$p" ] || { [ -n "$t" ] && [ "$l" = "$t" ]; }; then
                        printf '%s\n' "$n"; return 0
                    fi
                    break ;;
            esac
        done
    done < "$f"
    return 1
}
