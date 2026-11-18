#!/usr/bin/env bash
# The line that loads the plugin from a tmux config, for a plugin folder at any path.
# Written to bash 3.2's syntax, as bootstrap.sh is. Under bash 3.2.57 (#1060) this file,
# bootstrap.sh and install.sh pass bash -n, and cockpit_conf_line and cockpit_bare_carries
# print what bash 5.3 prints for 39 folder shapes, four of them holding a '. The two
# scripts were parsed under 3.2 and never run under it, and macOS's own bash was not used.
# Sourced by bootstrap.sh, which appends it to a config, and install.sh, which prints it.
#
# tmux hands a run-shell argument to sh, so the path must reach sh as one quoted word:
# bare, sh splits it at a blank and runs a $( ) in it (#1043, #1046). The word then sits in
# a tmux double-quoted string, where \ " and $ are escaped. run-shell expands tmux formats
# in its argument, and doubling a # does not hide it there: tmux copies a ## in front of a [
# through as it is. So each # leaves sh's quotes as \#, which puts a ' after every # for
# tmux, and #' is no format. A backtick is escaped too: tmux does not read one, but a
# line someone pastes into a shell would run it. A newline goes in as \n.
cockpit_conf_line() {         # <plugin folder> -> run-shell "'<folder>/cockpit.tmux'"
    local p="$1/cockpit.tmux" q="'\\''"
    p="'${p//\'/$q}'"
    p=${p//\\/\\\\}; p=${p//\"/\\\"}; p=${p//\$/\\\$}
    p=${p//\`/\\\`}; p=${p//$'\n'/\\n}; p=${p//\#/\'\\\\#\'}
    printf 'run-shell "%s"' "$p"
}

# Does a bare line, run-shell <folder>/cockpit.tmux, load the plugin from this folder?
# A yes is sure. A no means "might not": a * or a ? with no other folder to match, an ESC
# and a few more do load, and the callers word what they print that way (#1060).
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
            \[*) ;;
            [\{,DFHIPSTWh]*) return 1 ;;
            *) [ -z "$h" ] || return 1 ;;
        esac
    done
}

# The argument of a config line that runs a shell command: run-shell, or run, an optional
# -b, then the rest of the line, blanks trimmed. Set in cockpit_arg, with no subshell per
# line. Returns 1, cockpit_arg empty, for any other line: one starting with # among them.
cockpit_conf_run_arg() {      # <config line> -> cockpit_arg
    local l=$1
    cockpit_arg=''
    l=${l#"${l%%[![:space:]]*}"}; l=${l%"${l##*[![:space:]]}"}
    case "$l" in
        run-shell[[:space:]]*) l=${l#run-shell} ;;
        run[[:space:]]*) l=${l#run} ;;
        *) return 1 ;;
    esac
    l=${l#"${l%%[![:space:]]*}"}
    case "$l" in -b[[:space:]]*) l=${l#-b}; l=${l#"${l%%[![:space:]]*}"} ;; esac
    cockpit_arg=$l
}

# The number of the first line in a config that loads this folder's plugin bare:
# run-shell, or run, an optional -b, then the path with no quote around it, written out
# or with ~ for HOME. Nothing is printed, and 1 returned, when no line has that shape.
# It takes no quoted form, run-shell "<path>" included, where cockpit_conf_wired_line
# does: this one finds the line that might not load (#1046), that one any line (#1067).
cockpit_conf_bare_line() {    # <config file> <plugin folder> -> line number
    local f=$1 p="$2/cockpit.tmux" t='' l n=0 h=${HOME:-}
    h=${h%/}
    [ -z "$h" ] || case "$2" in "$h"/*) t="~${p#"$h"}" ;; esac
    while IFS= read -r l || [ -n "$l" ]; do
        n=$((n + 1))
        cockpit_conf_run_arg "$l" || continue
        l=$cockpit_arg
        if [ "$l" = "$p" ] || { [ -n "$t" ] && [ "$l" = "$t" ]; }; then
            printf '%s\n' "$n"; return 0
        fi
    done < "$f"
    return 1
}

# The number of the first line in a config that runs this folder's cockpit.tmux: run-shell
# or run, an optional -b, then the path in one of the shapes docs/install.md gives: bare
# or '<path>', each also with ~ for HOME, "<path>", '"$HOME/<rest>"' or '"${HOME}/<rest>"',
# and the argument cockpit_conf_line prints, "'<path>'" for a plain path. A comment, a bind
# naming the file and another folder's path are no load (#1067). It takes run-shell
# "<path>", where cockpit_conf_bare_line does not: that one finds only the unquoted line
# (#1046).
# The folder is a physical path (cd -P), and a config names it through any symlink, as
# ~/.tmux/plugins/tmux-cockpit linked to a checkout. So an argument no spelling matches is
# taken as the file it names (cockpit_conf_arg_path) and is a load when that is this
# folder's cockpit.tmux, -ef: the same file. Another folder's copy is another file.
# An if-shell or a source-file that reaches a line is not followed.
cockpit_conf_wired_line() {   # <config file> <plugin folder> -> line number
    local f=$1 p="$2/cockpit.tmux" r='' l n=0 h=${HOME:-} c
    local -a forms
    h=${h%/}
    c=$(cockpit_conf_line "$2"); forms=("$p" "${c#run-shell }")
    [ -z "$h" ] || case "$2" in "$h"/*) r=${p#"$h"} ;; esac
    [ -z "$r" ] || forms+=("~$r")
    case "$p" in *\'*) ;; *) forms+=("'$p'"); [ -z "$r" ] || forms+=("'~$r'") ;; esac
    case "$p" in *[\"\\\$]*) ;; *) forms+=("\"$p\"") ;; esac
    case "$r" in
        ''|*[\'\"\\\$\`]*) ;;
        *) forms+=("'\"\$HOME$r\"'" "'\"\${HOME}$r\"'") ;;
    esac
    while IFS= read -r l || [ -n "$l" ]; do
        n=$((n + 1))
        cockpit_conf_run_arg "$l" || continue
        for c in "${forms[@]}"; do
            [ "$cockpit_arg" = "$c" ] && { printf '%s\n' "$n"; return 0; }
        done
        cockpit_conf_arg_path "$cockpit_arg" || continue
        case $cockpit_path in /*) ;; *) continue ;; esac
        [ "$cockpit_path" -ef "$p" ] && { printf '%s\n' "$n"; return 0; }
    done < "$f"
    return 1
}

# The file a run argument names once tmux and sh have taken their quotes off, with HOME put
# in for a leading ~/, $HOME/ or ${HOME}/ where one of them expands it. tmux expands both ~
# and $ outside quotes and inside double quotes (tmux(1), PARSING SYNTAX), and sh expands
# both in an unquoted word. So bare, "<s>" and '<s>' (sh then sees <s> unquoted) take all
# three. '"<s>"' takes $HOME and ${HOME}, as sh's double quotes leave a ~ alone. "'<s>'"
# expands nothing, as sh's single quotes keep <s> as written. Any other shape, a quote
# inside one of these, a \ or a trailing comment, is no path. Set in cockpit_path (#1067).
cockpit_conf_arg_path() {     # <run argument> -> cockpit_path
    local a=$1 h=${HOME:-} t=all i
    h=${h%/}; cockpit_path=''
    case $a in
        \'*\')
            i=${a#\'}; i=${i%\'}
            case $i in
                *\'*) return 1 ;;
                \"*\") i=${i#\"}; i=${i%\"}; t=var
                       case $i in *[\"\\]*) return 1 ;; esac ;;
            esac ;;
        \"*\")
            i=${a#\"}; i=${i%\"}
            case $i in
                *[\"\\]*) return 1 ;;
                \'*\') i=${i#\'}; i=${i%\'}; t=none
                       case $i in *\'*) return 1 ;; esac ;;
            esac ;;
        *[\'\"\\]*) return 1 ;;
        *) i=$a ;;
    esac
    if [ -n "$h" ] && [ "$t" != none ]; then
        case $i in
            '~/'*) [ "$t" = var ] || i="$h/${i#\~/}" ;;
            '$HOME/'*) i="$h/${i#\$HOME/}" ;;
            '${HOME}/'*) i="$h/${i#\$\{HOME\}/}" ;;
        esac
    fi
    cockpit_path=$i
}

# Does tpm load this folder's plugin? Read in tpm at commit 99469c4a, its
# scripts/helpers/plugin_functions.sh: _get_user_tmux_conf (line 21) picks ONE user config,
# the XDG tmux.conf when it is a file, else ~/.tmux.conf, and _tmux_conf_contents (line 37)
# with tpm_plugins_list_helper (line 77) reads @plugin lines from that file alone. That awk
# takes a line matching ^[ \t]*set(-option)? +-g +@plugin (blanks only between the words,
# @plugin a prefix, so @plugins counts too), removes the quotes and takes field 4. tpm runs
# every *.tmux in <plugin path>/<basename of field 4, less .git and a #branch>. The plugin
# path is ~/.tmux/plugins, or the XDG tmux folder's plugins when an XDG tmux.conf is there
# (tpm's tpm, set_default_tpm_path, line 26). tpm itself starts from a run line in any
# config tmux loads.
# A line names this plugin when its name is this folder's name or <plugin path>/<name> is
# this folder, -ef, through a symlink too. Prints the config that names it, tpm's own
# first. 0: it loads, the line in tpm's config, <plugin path>/<name> this folder and tpm's
# run line there. 2: a line names it but one of those three fails (a
# TMUX_PLUGIN_MANAGER_PATH this cannot read among them). 1: no @plugin line names it (#1067).
cockpit_conf_tpm() {          # <plugin folder> <config>... -> the config naming it
    local d=$1 x="${XDG_CONFIG_HOME:-${HOME:-}/.config}/tmux" pd tc l c spec arg nm hm
    local f='' t='' run=0 hit=0 re='^[[:blank:]]*set(-option)? +-g +@plugin'
    shift
    pd="${HOME:-}/.tmux/plugins"; tc="${HOME:-}/.tmux.conf"
    [ ! -f "$x/tmux.conf" ] || { pd="$x/plugins"; tc="$x/tmux.conf"; }
    for c in "$@"; do
        nm=0; hm=0
        while IFS= read -r l || [ -n "$l" ]; do
            if cockpit_conf_run_arg "$l"; then
                arg=${cockpit_arg//\'/}; arg=${arg//\"/}
                case "$arg" in */tpm/tpm) run=1 ;; esac
                continue
            fi
            [[ $l =~ $re ]] || continue
            l=${l//\'/}; l=${l//\"/}
            read -r _ _ _ spec _ <<< "$l"
            spec=${spec%%#*}; spec=${spec%.git}; spec=${spec##*/}
            [ "$spec" != "${d##*/}" ] || nm=1
            # What tpm runs is <plugin path>/<spec>: that folder, through any symlink.
            [ -z "$spec" ] || [ ! "$pd/$spec" -ef "$d" ] || { nm=1; hm=1; }
        done < "$c"
        [ "$nm" -eq 1 ] || continue
        [ -n "$f" ] || f=$c
        if [ -z "$t" ] && [ "$c" -ef "$tc" ]; then t=$c; hit=$hm; fi
    done
    [ -n "$f" ] || return 1
    if [ -n "$t" ] && [ "$hit" -eq 1 ] && [ "$run" -eq 1 ]; then
        printf '%s\n' "$t"; return 0
    fi
    printf '%s\n' "${t:-$f}"
    return 2
}

# Which of the configs tmux loads runs this folder's plugin: the first with a wired line,
# else the one tpm reads it from. Sets cockpit_wired to that config. 0: it loads. 2: a tpm
# @plugin line names it, not confirmed, and cockpit_wired is that config. 1: neither. Read
# by install.sh --check, install.sh's closing note and bootstrap.sh alike (#1067).
cockpit_conf_wired() {        # <plugin folder> <readable config>... -> cockpit_wired
    local d=$1 c
    shift; cockpit_wired=''
    for c in "$@"; do
        cockpit_conf_wired_line "$c" "$d" >/dev/null && { cockpit_wired=$c; return 0; }
    done
    [ "$#" -gt 0 ] || return 1
    # shellcheck disable=SC2034 # read by the scripts that source this file
    cockpit_wired=$(cockpit_conf_tpm "$d" "$@")
}
