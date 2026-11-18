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
# A trailing comment comes off, at the first blank followed by a #, as docs/install.md's own
# tpm line has one. tmux starts a comment only at a # it reads between words, and keeps one
# inside quotes (cmd-parse.y, yylex() and yylex_token()). So a ' #' inside a quoted path
# ends the path here, where tmux keeps it: such a line runs nothing this file can name, and
# the callers say so loudly (#1302, #1304).
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
    l=${l%%[[:space:]]#*}; l=${l%"${l##*[![:space:]]}"}
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
# inside one of these or a \, is no path. Set in cockpit_path (#1067).
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

# The first line of a config that runs this folder's cockpit.tmux (cockpit_conf_wired_line)
# where sh reads the path as an unquoted word that cockpit_bare_carries does not carry: at a
# blank sh splits it, and nothing loads (docs/install.md). What sh reads, by the argument's
# shape (tmux(1), PARSING SYNTAX):
#   bare and "<s>": tmux puts HOME in for ~ and $HOME first, so the whole path.
#   '<s>': <s> as written. sh puts HOME in for a leading ~/ and does not split what it put
#   in, so only the rest counts there. A $HOME sh puts in is split: the whole path counts.
#   '"<s>"' and "'<s>'", the line cockpit_conf_line prints among them: sh's own quotes, none.
# The line as written is read, never this folder's path, so a link to the folder counts by
# the link's path. Sets cockpit_unquoted_n and cockpit_unquoted_form, bare or quoted; 1 when
# no line is one (#1303). cockpit_conf_bare_line finds the bare shape alone (#1046).
cockpit_conf_unquoted_line() {  # <config file> <plugin folder> -> cockpit_unquoted_*
    local f=$1 d=$2 l a w form n=0
    cockpit_unquoted_n=0; cockpit_unquoted_form=''
    while IFS= read -r l || [ -n "$l" ]; do
        n=$((n + 1))
        cockpit_conf_run_arg "$l" || continue
        a=$cockpit_arg
        case $a in \'\"*\"\'|\"\'*\'\") continue ;; esac
        cockpit_conf_arg_path "$a" || continue
        w=$cockpit_path; form=quoted
        case $a in
            "'~/"*\') w=${a#"'~/"}; w=${w%\'} ;;
            \'*\'|\"*\") ;;
            *) form=bare ;;
        esac
        ! cockpit_bare_carries "$w" || continue
        cockpit_conf_wired_line <(printf '%s\n' "$l") "$d" >/dev/null || continue
        # shellcheck disable=SC2034 # read by the scripts that source this file
        { cockpit_unquoted_n=$n; cockpit_unquoted_form=$form; }
        return 0
    done < "$f"
    return 1
}

# Where tpm looks, read in tpm at commit 99469c4a: its one user config, the XDG tmux.conf
# when that is a file, else ~/.tmux.conf (scripts/helpers/plugin_functions.sh,
# _get_user_tmux_conf, line 21), and its plugin folder, the XDG tmux folder's plugins when
# that tmux.conf is there, else ~/.tmux/plugins (tpm's tpm, set_default_tpm_path, line 26).
# Sets cockpit_tpm_conf and cockpit_tpm_dir.
cockpit_conf_tpm_where() {
    local x="${XDG_CONFIG_HOME:-${HOME:-}/.config}/tmux"
    cockpit_tpm_dir="${HOME:-}/.tmux/plugins"; cockpit_tpm_conf="${HOME:-}/.tmux.conf"
    [ ! -f "$x/tmux.conf" ] || { cockpit_tpm_dir="$x/plugins"; cockpit_tpm_conf="$x/tmux.conf"; }
}

# The files tpm also reads @plugin lines from: those a source or source-file line in its
# config names, one level down, read as tpm reads them. At tpm 99469c4a,
# scripts/helpers/plugin_functions.sh _sourced_files (line 47) runs
# sed -E 's/^[[:space:]]*source(-file)?[[:space:]]+(-q+[[:space:]]+)?['"]?([^'"]+)['"]?/\3/p'
# over that config, and _tmux_conf_contents (line 35) reads each word of what it prints,
# unquoted, so split at blanks and globbed, after _manual_expansion (line 5) makes a leading
# ~ HOME, then a leading $HOME. ${HOME} is left as it is. A word starting with -, a path
# not from / (tpm reads it from a folder this cannot know), and a file that is not there
# are skipped. Sets cockpit_tpm_src to the files, in order (#1304).
cockpit_conf_tpm_sourced() {  # <tpm's config> -> cockpit_tpm_src
    local l w g q="['\"]" nq="[^'\"]" re
    local -a ws gs
    # {0,1} for tpm's ?: a ? after a ) reads as a jq 1.7 postfix to the #481 scan.
    re="^[[:space:]]*source(-file){0,1}[[:space:]]+(-q+[[:space:]]+){0,1}$q?($nq+)$q?(.*)\$"
    cockpit_tpm_src=()
    [ -r "$1" ] || return 0
    while IFS= read -r l || [ -n "$l" ]; do
        [[ $l =~ $re ]] || continue
        read -ra ws <<< "${BASH_REMATCH[3]}${BASH_REMATCH[4]}"
        [ "${#ws[@]}" -gt 0 ] || continue
        for w in "${ws[@]}"; do
            case $w in -*) continue ;; esac
            w=${w/#\~/${HOME:-}}; w=${w/#\$HOME/${HOME:-}}
            case $w in /*) ;; *) continue ;; esac
            # shellcheck disable=SC2206 # unquoted on purpose: tpm globs the word
            gs=($w)
            for g in "${gs[@]}"; do
                [ ! -f "$g" ] || [ ! -r "$g" ] || cockpit_tpm_src+=("$g")
            done
        done
    done < "$1"
}

# The lines of each file in turn, with no external command: install.sh --check runs on a
# PATH that may hold no cat (#1304).
cockpit_conf_lines() {        # <file>... -> their lines
    local f l
    for f in "$@"; do
        while IFS= read -r l || [ -n "$l" ]; do printf '%s\n' "$l"; done < "$f"
    done
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
# config tmux loads: one whose path ends in /tpm/tpm, as a clone of tpm has it, or the path
# Debian's and Arch's packages install, /usr/share/tmux-plugin-manager/tpm. Any other path
# that ends in tpm is no run of tpm. A script of the user's named tpm would read as one,
# and then the plugin silently never loads, where rc 2 prints a fix line (#1302).
# cockpit_conf_run_arg takes a trailing comment off first. A ' #' inside a quoted path ends
# the path there, where tmux keeps it, and that line gives the loud rc 2, a "cannot tell".
# A line names this plugin when its name is this folder's name or <plugin path>/<name> is
# this folder, -ef, through a symlink too. Prints the config that names it, tpm's own
# first, and sets cockpit_tpm_file to it. 0: it loads, the line in tpm's config,
# <plugin path>/<name> this folder and tpm's run line there. 2: a line names it but one of
# those three fails (a TMUX_PLUGIN_MANAGER_PATH this cannot read among them). 1: no @plugin
# line names it (#1067). On 2, cockpit_tpm_why holds a word for each check that failed, in
# this order: conf (no line in tpm's own config, cockpit_tpm_conf, names it), run (no run
# line of tpm) and folder (tpm runs cockpit_tpm_runs, not this folder) (#1302).
cockpit_conf_tpm() {          # <plugin folder> <config>... -> the config naming it
    local d=$1 pd tc l c spec arg nm hm sp
    local -a fl
    local f='' t='' tn='' run=0 hit=0 re='^[[:blank:]]*set(-option)? +-g +@plugin'
    shift
    cockpit_tpm_file=''; cockpit_tpm_why=''; cockpit_tpm_runs=''
    cockpit_conf_tpm_where; pd=$cockpit_tpm_dir; tc=$cockpit_tpm_conf
    for c in "$@"; do
        nm=0; hm=0; sp=''; fl=("$c")
        # tpm's config with the files it sources, as tpm reads them (#1304).
        if [ "$c" -ef "$tc" ]; then
            cockpit_conf_tpm_sourced "$c"
            [ "${#cockpit_tpm_src[@]}" -eq 0 ] || fl+=("${cockpit_tpm_src[@]}")
        fi
        while IFS= read -r l || [ -n "$l" ]; do
            if cockpit_conf_run_arg "$l"; then
                arg=${cockpit_arg//\'/}; arg=${arg//\"/}
                case "$arg" in
                    */tpm/tpm|/usr/share/tmux-plugin-manager/tpm) run=1 ;;
                esac
                continue
            fi
            [[ $l =~ $re ]] || continue
            l=${l//\'/}; l=${l//\"/}
            read -r _ _ _ spec _ <<< "$l"
            spec=${spec%%#*}; spec=${spec%.git}; spec=${spec##*/}
            [ "$spec" != "${d##*/}" ] || { nm=1; [ -n "$sp" ] || sp=$spec; }
            # What tpm runs is <plugin path>/<spec>: that folder, through any symlink.
            [ -z "$spec" ] || [ ! "$pd/$spec" -ef "$d" ] || { nm=1; hm=1; }
        done < <(cockpit_conf_lines "${fl[@]}")
        [ "$nm" -eq 1 ] || continue
        [ -n "$f" ] || f=$c
        if [ -z "$t" ] && [ "$c" -ef "$tc" ]; then t=$c; hit=$hm; tn=$sp; fi
    done
    [ -n "$f" ] || return 1
    cockpit_tpm_file=${t:-$f}
    printf '%s\n' "$cockpit_tpm_file"
    [ -z "$t" ] || [ "$hit" -eq 0 ] || [ "$run" -eq 0 ] || return 0
    [ -n "$t" ] || cockpit_tpm_why=conf
    [ "$run" -eq 1 ] || cockpit_tpm_why="${cockpit_tpm_why:+$cockpit_tpm_why }run"
    if [ -n "$t" ] && [ "$hit" -eq 0 ]; then
        cockpit_tpm_why="${cockpit_tpm_why:+$cockpit_tpm_why }folder"
        cockpit_tpm_runs="$pd/$tn"
    fi
    return 2
}

# What failed, after a cockpit_conf_tpm that returned 2: one clause per word of
# cockpit_tpm_why, joined by "; ". install.sh --check prints it in its fix column and
# install.sh's closing note prints it too, so each names the check that failed (#1302).
cockpit_conf_tpm_text() {     # <plugin folder> -> the clauses
    local w s='' c
    for w in $cockpit_tpm_why; do
        case $w in
            conf)   c="tpm reads @plugin lines only from $cockpit_tpm_conf" ;;
            run)    c='no line runs tpm' ;;
            folder) c="tpm runs $cockpit_tpm_runs, not $1" ;;
            *)      continue ;;
        esac
        s="${s:+$s; }$c"
    done
    printf '%s' "$s"
}

# Which of the configs tmux loads runs this folder's plugin: the first with a wired line,
# else the one tpm reads it from. Sets cockpit_wired to that config. 0: it loads. 2: a tpm
# @plugin line names it, not confirmed, and cockpit_wired is that config. 1: neither. Read
# by install.sh --check, install.sh's closing note and bootstrap.sh alike (#1067).
cockpit_conf_wired() {        # <plugin folder> <readable config>... -> cockpit_wired
    local d=$1 c r
    shift; cockpit_wired=''
    for c in "$@"; do
        cockpit_conf_wired_line "$c" "$d" >/dev/null && { cockpit_wired=$c; return 0; }
    done
    [ "$#" -gt 0 ] || return 1
    # Not in a subshell: cockpit_tpm_why and the rest it sets are read by install.sh (#1302).
    cockpit_conf_tpm "$d" "$@" >/dev/null
    r=$?
    # shellcheck disable=SC2034 # read by the scripts that source this file
    cockpit_wired=$cockpit_tpm_file
    return "$r"
}

# The first line, outside a comment, that names cockpit.tmux, or an @plugin line (tpm's
# pattern) naming tmux-cockpit or this folder's name. bootstrap.sh appends no run line when
# one is there, even one the readers above call no load: that line may run a copy of the
# plugin by a route they do not follow, and two copies loading is worse than none (#1067).
# An @plugin line in a file tpm's config sources counts too, as tpm reads it there
# (cockpit_conf_tpm_sourced). A cockpit.tmux line there is not looked for: nothing here
# follows source-file the way tmux does (#1304).
# Sets cockpit_mention_file, cockpit_mention_n and cockpit_mention_line; 1 when none.
cockpit_conf_mention() {      # <plugin folder> <config>... -> cockpit_mention_*
    local d=$1 c l t n spec top=1 re='^[[:blank:]]*set(-option)? +-g +@plugin'
    local -a fs
    shift
    cockpit_mention_file=''; cockpit_mention_n=0; cockpit_mention_line=''
    cockpit_conf_tpm_where; cockpit_conf_tpm_sourced "$cockpit_tpm_conf"
    fs=("$@" '')              # the empty entry: the sourced files follow it
    [ "${#cockpit_tpm_src[@]}" -eq 0 ] || fs+=("${cockpit_tpm_src[@]}")
    for c in "${fs[@]}"; do
        [ -n "$c" ] || { top=0; continue; }
        n=0
        while IFS= read -r l || [ -n "$l" ]; do
            n=$((n + 1))
            t=${l#"${l%%[![:space:]]*}"}
            case $t in ''|'#'*) continue ;; esac
            case $t in
                *cockpit.tmux*) [ "$top" -eq 1 ] || continue ;;
                *)  [[ $l =~ $re ]] || continue
                    t=${l//\'/}; t=${t//\"/}
                    read -r _ _ _ spec _ <<< "$t"
                    spec=${spec%%#*}; spec=${spec%.git}; spec=${spec##*/}
                    [ "$spec" = tmux-cockpit ] || [ "$spec" = "${d##*/}" ] || continue ;;
            esac
            # shellcheck disable=SC2034 # read by the scripts that source this file
            { cockpit_mention_file=$c; cockpit_mention_n=$n; cockpit_mention_line=$l; }
            return 0
        done < "$c"
    done
    return 1
}
