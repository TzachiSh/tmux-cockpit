#!/usr/bin/env bash
# tmux-cockpit installer.
#   ./install.sh            symlink the commands into ~/.local/bin, fetch cockpit-tui
#   ./install.sh --uninstall  remove only the symlinks that point back here
#   ./install.sh --check      the doctor: one row per thing that can be wrong
#   ./install.sh --hooks [--dry-run]  merge examples/claude-hooks.json into Claude Code's
#                                     settings.json ($CLAUDE_CONFIG_DIR, else ~/.claude)
#                                     and install the cockpit-events mod (docs/mod.md)
#   ./install.sh --uninstall --hooks [--dry-run]  also remove exactly what --hooks added
#   ./install.sh --hooks --remove [--dry-run]     remove only the hooks and the mod, keep the links
set -uo pipefail

# Resolved (-P), so the uninstall guard matches links made through a symlinked checkout.
# The plugin root holds the REAL file: `me` follows any chain of links, a relative
# target read from its own link's folder (#1290).
me=${BASH_SOURCE[0]}; case $me in (*/*) ;; (*) me=./$me ;; esac
[ -L "$me" ] && me=$(n=0
    while [ -L "$me" ] && [ $((n += 1)) -le 40 ] && t=$(readlink "$me"); do
        case $t in (/*) me=$t ;; (*) me=${me%/*}/$t ;; esac; done; printf %s "$me")
DIR="$(cd -P "$(dirname "$me")" && pwd)"
BIN="${XDG_BIN_HOME:-${HOME:-}/.local/bin}"
if [ -z "${XDG_BIN_HOME:-}" ] && [ -z "${HOME:-}" ]; then
    printf 'HOME is not set and XDG_BIN_HOME is not set; cannot pick an install directory\n' >&2
    exit 1
fi
COMMANDS=(tm tmux-branch tmux-open-path)

USAGE='usage: install.sh [--check] [--uninstall] [--hooks [--dry-run]]
       install.sh --uninstall --hooks [--dry-run]   also drop cockpit'"'"'s hooks and its mod
       install.sh --hooks --remove [--dry-run]      drop only the hooks and the mod, keep the links'
uninstall=0; hooks=0; dry=0; check=0; remove=0; links=1
for a in "$@"; do case "$a" in
    "") ;;
    -h|--help) printf '%s\n' "$USAGE"; exit 0 ;;
    --uninstall) uninstall=1 ;;
    --remove)    remove=1 ;;
    --check)     check=1 ;;
    --hooks)     hooks=1 ;;
    --dry-run)   dry=1 ;;
    *) printf 'unknown option: %s\n\n%s\n' "$a" "$USAGE" >&2; exit 2 ;;
esac; done
# Flag combinations that would silently ignore one half are refused.
[ "$check" -eq 1 ] && [ $((uninstall + hooks + dry)) -gt 0 ] && {
    printf -- '--check runs alone\n\n%s\n' "$USAGE" >&2; exit 2; }
[ "$remove" -eq 1 ] && [ "$hooks" -eq 0 ] && { printf -- '--remove needs --hooks\n' >&2; exit 2; }
[ "$remove" -eq 1 ] && [ "$uninstall" -eq 1 ] && { printf -- '--remove removes only the hooks; --uninstall --hooks removes the links too, use one\n' >&2; exit 2; }
# --hooks --remove: the hook removal below, with the links left alone.
[ "$remove" -eq 1 ] && { uninstall=1; links=0; }
[ "$dry" -eq 1 ] && [ "$hooks" -eq 0 ] && { printf -- '--dry-run needs --hooks\n' >&2; exit 2; }


# ---------------------------------------------------------------- doctor ----
# shellcheck source=scripts/lib/vnum.sh
. "$DIR/scripts/lib/vnum.sh" || exit 1
# shellcheck source=scripts/lib/confline.sh
. "$DIR/scripts/lib/confline.sh" || exit 1
# shellcheck source=scripts/lib/tui.sh
. "$DIR/scripts/lib/tui.sh" || exit 1
# shellcheck source=scripts/lib/eol.sh
. "$DIR/scripts/lib/eol.sh" || exit 1
# shellcheck source=scripts/lib/claudedir.sh
. "$DIR/scripts/lib/claudedir.sh" || exit 1
# shellcheck source=scripts/lib/tmpdir.sh
. "$DIR/scripts/lib/tmpdir.sh" || exit 1

# A builtin, not awk: the fail rows must still be right on an empty PATH.
fld() {                       # fld <field> <version line>
    local n=$1 i=1 w
    # shellcheck disable=SC2086  # splitting on whitespace is the point
    set -- $2
    for w in "$@"; do
        [ "$i" -eq "$n" ] && { printf '%s\n' "$w"; return 0; }
        i=$((i+1))
    done
}

# The exit is the worst row: 0 ok or skip, 1 warn, 2 fail. skip is 0 because a
# bare shell cannot answer some rows.
worst=0
row() {                       # row <ok|skip|warn|fail> <what> <found> [fix]
    local m
    case "$1" in
        ok)   m='  ok  ' ;;
        skip) m=' skip ' ;;
        warn) m=' warn '; [ "$worst" -lt 1 ] && worst=1 ;;
        *)    m=' fail '; worst=2 ;;
    esac
    printf '[%s] %-14s %-24s %s\n' "$m" "$2" "$3" "${4:-}"
}

# The cockpit-events mod (docs/mod.md), for --hooks and --check alike. mod_names sets mf, the
# checkout's marketplace file, and from it mk, the marketplace, and pl, the plugin; 1 when it
# cannot read them.
mod_names() {
    mf="$DIR/.claude-plugin/marketplace.json"
    mk=$(jq -er .name "$mf" 2>/dev/null) && pl=$(jq -er '.plugins[0].name' "$mf" 2>/dev/null)
}
# A jq def over a list of marketplaces in the shape `claude plugin marketplace list --json`
# prints: the one named $n is none, ours (a directory at $d, this checkout), or its source and
# where it is.
mod_where_jq='def mod_where($n; $d): [.[] | select(.name == $n)]
    | if length == 0 then "none"
      elif .[0].source == "directory" and .[0].path == $d then "ours"
      else "\(.[0].source) \(.[0].path // .[0].repo // .[0].url // "")" end;'

if [ "$check" -eq 1 ]; then
    printf 'tmux-cockpit doctor\n\n'

    v=$(tmux -V 2>/dev/null); v=${v#tmux }
    # Graded as cockpit.tmux grades it: next-3.9 is 3.9, a bare word (master) is ok.
    tmux_old=0
    vnum_ge "$v" 3.8 || tmux_old=1
    if [ -z "$v" ]; then row fail tmux 'not found' 'bootstrap.sh builds tmux 3.8'
    elif [ "$tmux_old" = 1 ]; then row fail tmux "$v" 'bootstrap.sh builds tmux 3.8'
    else row ok tmux "$v"
    fi

    # Required (#199): the board and the panel run on it.
    v=$(tui_check)
    case $? in
        0) row ok cockpit-tui "$v" ;;
        1) row fail cockpit-tui 'not found' 'install.sh fetches or builds it' ;;
        *) row fail cockpit-tui "$v" "needs $TUI_MIN+; install.sh updates it" ;;
    esac

    # A warning, not a failure: the status bar and the pickers work without jq.
    v=$(jq --version 2>/dev/null)
    if [ -n "$v" ]; then row ok jq "$v"
    else row warn jq 'not found' 'needed by install.sh --hooks and the review gate'
    fi

    # revdiff runs under the server's PATH, not this shell's: a distro copy can
    # answer there while a fetched one wins here.
    fp=$(tmux show-environment -g PATH 2>/dev/null); fp=${fp#PATH=}

    # Required: `r` on a board row spawns it. Same server-PATH rule. A go install
    # build prints "unknown": present, version unverifiable, so a warn.
    rv=$(PATH=${fp:-$PATH} command -v revdiff 2>/dev/null)
    v=''; [ -z "$rv" ] || v=$("$rv" --version 2>/dev/null)
    v=${v#version: }; v=${v%% *}
    revfix='bootstrap.sh, or brew install umputun/apps/revdiff, or'
    revfix="$revfix go install github.com/umputun/revdiff/app@v1.13.0 (rename app to revdiff);"
    revfix="$revfix see docs/install.md#requirements"
    if [ -z "$rv" ]; then row fail revdiff 'not found' "$revfix"
    elif [ -z "$v" ]; then                    # prints nothing: not a go install build
        row warn revdiff "no version @ $rv" "does not report a version; $revfix"
    elif [ "$(vnum "$v")" -eq 0 ]; then       # "unknown", "dev": no x.y.z to grade
        row warn revdiff "${v:-unknown} @ $rv" 'a go install build; tested with v1.13.0'
    elif [ "$(vnum "$v")" -lt "$(vnum 1.13.0)" ]; then
        row warn revdiff "$v @ $rv" "older than the tested v1.13.0; $revfix"
    else row ok revdiff "$v"
    fi

    # Order, not just presence: a fetched revdiff in $BIN is useless if a distro's answers first.
    case ":$PATH:" in
        *":$BIN:"*)
            shadow=''; sdir=''
            if [ -x "$BIN/revdiff" ]; then
                w=$(command -v revdiff 2>/dev/null) || w=''
                [ -n "$w" ] && [ "$w" != "$BIN/revdiff" ] && { shadow=revdiff; sdir=${w%/*}; }
            fi
            if [ -n "$shadow" ]; then
                row warn PATH "$BIN present" "put $BIN before $sdir in PATH"
            elif [ "${PATH%%:*}" = "$BIN" ]; then row ok PATH "$BIN first"
            else row ok PATH "$BIN present"
            fi ;;
        *) row fail PATH 'not on PATH' "add $BIN to PATH" ;;
    esac

    conf=''; wired=0; cfs=()
    # tmux loads both, so the run-shell line in either wires the box. A line that only
    # names cockpit.tmux, a comment or another folder's path, does not (#1067).
    cxdg="${XDG_CONFIG_HOME:-${HOME:-}/.config}/tmux/tmux.conf"; chome="${HOME:-}/.tmux.conf"
    for c in "$cxdg" "$chome"; do
        [ -r "$c" ] || continue
        [ -n "$conf" ] || conf="$c"        # the first readable one names a fail row
        cfs+=("$c")
    done
    wire_fix="run bootstrap.sh, or add $(cockpit_conf_line "$DIR")"
    if [ "${#cfs[@]}" -gt 0 ]; then
        cockpit_conf_wired "$DIR" "${cfs[@]}"
        case $? in
            0) wired=1; conf=$cockpit_wired ;;
            2) conf=$cockpit_wired      # the fix names each check that failed (#1302)
               wire_fix="a tpm @plugin line names cockpit, not confirmed:"
               wire_fix="$wire_fix $(cockpit_conf_tpm_text "$DIR")" ;;
            *) # A line that names the plugin and loads no copy from here: say which, and
               # which folder it runs, since bootstrap.sh then appends nothing (#1067).
               if cockpit_conf_mention "$DIR" "${cfs[@]}"; then
                   conf="$cockpit_mention_file line $cockpit_mention_n"
                   wire_fix="names cockpit but loads none from $DIR"; self=0
                   if cockpit_conf_run_arg "$cockpit_mention_line" &&
                      cockpit_conf_arg_path "$cockpit_arg"; then
                       case $cockpit_path in
                           /*) if [ "$cockpit_path" -ef "$DIR/cockpit.tmux" ]; then self=1
                               else wire_fix="runs the copy in ${cockpit_path%/*}, not $DIR"; fi ;;
                       esac
                   fi
                   # This folder's line in a file the config sources, which --check does not
                   # follow: it loads, and a line added beside it would load it twice (#1304).
                   if [ "$self" -eq 1 ]; then
                       wire_fix="loads this folder from $cockpit_mention_file, a file the"
                       wire_fix="$wire_fix config sources, which --check does not follow;"
                       wire_fix="$wire_fix nothing needs adding"
                   else
                       wire_fix="$wire_fix: keep one copy; this one's line is"
                       wire_fix="$wire_fix $(cockpit_conf_line "$DIR")"
                   fi
               fi ;;
        esac
    fi
    # A line whose path sh reads unquoted might not load from this folder: the bare line
    # (#1046, #1060), and '<path>' or "<path>", which sh reads unquoted too (#1303).
    if [ "$wired" -eq 1 ] && cockpit_conf_unquoted_line "$conf" "$DIR"; then
        n=$cockpit_unquoted_n
        case "$DIR" in
            *[[:cntrl:]]*) wire_fix='the folder path holds a control character: move the folder' ;;
            *) wire_fix="replace line $n with $(cockpit_conf_line "$DIR")" ;;
        esac
        why=unquoted
        [ "$cockpit_unquoted_form" = bare ] || why='sh reads its path unquoted'
        row warn config "$conf line $n" "$why, might not load from this folder; $wire_fix"
    elif [ "$wired" -eq 1 ]; then row ok config "$conf loads cockpit"
    elif [ -n "$conf" ];    then row fail config "$conf" "$wire_fix"
    else row fail config 'no tmux config' "$wire_fix"
    fi

    # Files git stores with LF and the tree holds with CRLF: tmux reads each CR as text and a
    # plugin's config stops loading (#188). The command that restores them is named, never
    # run: it overwrites edits to tracked files, so it is the user's step.
    if ! command -v git >/dev/null 2>&1; then row skip crlf 'git not found'
    else
        eol_dirs "$DIR"; eol_read=0; eol_bad=0
        for eol_d in "${eol_list[@]}"; do
            eol_crlf "$eol_d" || continue
            eol_read=$((eol_read + 1))
            [ "$eol_n" -gt 0 ] || continue
            eol_bad=1
            eol_fix="$(eol_fix_cmd "$eol_d") (overwrites edits to tracked files)"
            eol_found="$(eol_files "$eol_n") in ${eol_d##*/}"
            if [ "$eol_d" = "$DIR" ]; then row fail crlf "$eol_found" "$eol_fix"
            else row warn crlf "$eol_found" "$eol_fix"
            fi
        done
        if [ "$eol_bad" -eq 1 ]; then :
        elif [ "$eol_read" -eq 0 ]; then row skip crlf 'no git checkout to read'
        else row ok crlf "no CRLF in $eol_read checkouts"
        fi
    fi

    # These formats re-expand per pane on every redraw, so a #{P:} walk in them freezes tmux.
    if [ "$wired" -eq 0 ]; then
        row skip formats 'no cockpit config to read'
        row skip tokens 'no cockpit config to read'
    else
        walk=0 tok=0
        # A symlink of one onto the other must not count twice.
        [ "$cxdg" -ef "$chome" ] && chome=''
        for c in "$cxdg" "$chome"; do
            [ -r "$c" ] || continue
            acc=''
            while read -r l || [ -n "$l" ]; do
                # tmux joins a trailing backslash before expanding.
                case "$l" in *\\) acc="$acc${l%\\}"; continue ;; esac
                l="$acc$l"; acc=''
                # Nothing rewrites #{agent_*} any more: it prints as text.
                case "${l#"${l%%[![:space:]]*}"}" in '#'*) ;; *'#{agent_'*) tok=$((tok + 1)) ;; esac
                case "$l" in
                    set*\ pane-border-format\ *|set*\ window-status-format\ *|\
set*\ window-status-current-format\ *|set*\ @catppuccin_window_text\ *|\
set*\ @catppuccin_window_current_text\ *)
                        case "$l" in *'#{P:'*) walk=$((walk + 1)) ;; esac ;;
                esac
            done < "$c"
        done
        if [ "$walk" -eq 0 ]; then row ok formats 'no pane walk on redraw'
        else
            if [ "$walk" -eq 1 ]; then nf='format walks'; else nf='formats walk'; fi
            row warn formats "$walk $nf panes" \
                "delete them: the plugin draws the bar (see $DIR/examples/tmux.conf)"
        fi
        if [ "$tok" -eq 0 ]; then row ok tokens 'no #{agent_*} token'
        else
            row warn tokens "$tok lines use #{agent_*}" \
                "see the migration table in $DIR/docs/status-bar.md"
        fi
    fi

    if ! claude_cfg_dir || [ ! -d "$claude_cfg" ]; then
        row skip hooks 'no Claude Code'
        row skip mod 'no Claude Code'
    elif ! command -v jq >/dev/null 2>&1; then row skip hooks 'jq missing'
    else
        cset="$claude_cfg/settings.json"
        # jq exits 3 when it cannot compile a filter. Counted as 0, that read as "not
        # merged" (#481); it is cockpit's bug, so it gets a fail row of its own.
        jqv=$(jq --version 2>/dev/null)
        n=$(jq '[.hooks[]?[]?.hooks[]?.command? // ""
                 | select(test("tmux-cockpit|agent-state"))] | length' \
               "$cset" 2>/dev/null); nrc=$?
        case "$nrc:${n:-0}" in
            3:*) row fail hooks "$jqv rejected the filter" 'report it: tmux-cockpit issue' ;;
            *:0) row warn hooks '0 cockpit hooks merged' 'install.sh --hooks' ;;
            *) row ok hooks "$n cockpit hooks merged" ;;
        esac
        # `capture(...)? | .c`: jq 1.7.x (Ubuntu 24.04 ships 1.7.1) rejects any postfix
        # written straight after the `?` of a call; 1.8 accepts it (#481).
        # The board's subagent tree reads what these six append (#361); sub-taskstop and
        # sub-reconcile came later (#403), so a box set up before them has 4 of 6.
        subs=$(jq '[.hooks[]?[]?.hooks[]?.command? // ""
                    | capture("agent-state (?<c>sub-(start|stop|idle|end|taskstop|reconcile))$")? |
                    .c]
                   | unique | length' \
                 "$cset" 2>/dev/null); src=$?
        if [ "$src" = 3 ]; then
            row fail subhooks "$jqv rejected the filter" 'report it: tmux-cockpit issue'
        elif [ "$nrc" = 3 ]; then row skip subhooks 'hooks count unreadable'
        else
            case "$n:${subs:-0}" in
                0:*) row skip subhooks 'no cockpit hooks yet' ;;
                *:6) row ok subhooks 'all 6 subagent hooks merged' ;;
                *) row warn subhooks "${subs:-0} of 6 subagent hooks merged" \
                       're-run install.sh --hooks' ;;
            esac
        fi
        # The mod, from what claude's plugin CLI wrote to the same file: no claude starts. Like
        # subhooks, it waits for cockpit hooks, so the hooks row speaks first.
        if [ "$nrc" = 3 ]; then row skip mod 'hooks count unreadable'
        elif [ "${n:-0}" = 0 ]; then row skip mod 'no cockpit hooks yet'
        elif ! mod_names; then row fail mod "cannot read $mf" 'restore the checkout from git'
        else
            # <where>:<enabled>, the marketplaces put in the shape claude's list prints.
            mst=$(jq -r --arg n "$mk" --arg d "$DIR" --arg p "$pl@$mk" "$mod_where_jq"'
                    ([(.extraKnownMarketplaces | objects) // {} | to_entries[]
                      | {name: .key} + ((.value.source | objects) // {})] | mod_where($n; $d))
                    + ":" + ((.enabledPlugins | objects) // {} | .[$p] | tostring)' \
                  "$cset" 2>/dev/null); mrc=$?
            # A read that fails after the hooks count reads as nothing there, as the hooks row
            # reads a file it cannot read.
            case $mrc:$mst in
                3:*) row fail mod "$jqv rejected the filter" 'report it: tmux-cockpit issue' ;;
                0:ours:true) row ok mod "$pl from this checkout" ;;
                0:ours:false) row warn mod "$pl disabled" "claude plugin enable $pl@$mk" ;;
                0:ours:*|0:none:*|[!0]*) row warn mod "$pl not installed" 'install.sh --hooks' ;;
                *) mfix="not this checkout; claude plugin marketplace remove $mk,"
                   row warn mod "$mk is ${mst%:*}" "$mfix then install.sh --hooks" ;;
            esac
        fi
    fi

    if [ -z "${TMUX:-}" ]; then row skip truecolor 'run inside tmux to check'
    else
        tf=$(tmux display-message -p '#{client_termfeatures}' 2>/dev/null)
        case ",$tf," in
            *,RGB,*) row ok truecolor 'RGB advertised' ;;
            *) row warn truecolor 'no RGB' \
                   'set terminal-features ...:RGB, see examples/full.tmux.conf' ;;
        esac
    fi

    pb=$(tmux show -gv popup-border-lines 2>/dev/null) || pb=''
    if [ -z "$pb" ]; then row skip popup-border 'no tmux server'
    elif [ "$pb" = single ]; then
        row warn popup-border single 'set -g popup-border-lines rounded'
    else row ok popup-border "$pb"
    fi

    printf '\n'
    case "$worst" in
        0) printf 'ok\n' ;;
        1) printf 'usable, with warnings above\n' ;;
        *) printf 'not working: fix the fail rows\n' ;;
    esac
    exit "$worst"
fi

# The temp file a write below puts beside settings.json, removed however the script ends: both
# traps ignore HUP, INT and TERM first, so a signal cannot stop the rm (#1417, as #1411's). A
# signal sent to this pid alone waits for the command in the foreground: bash runs a trap only
# once that command ends. After the rm, the EXIT trap raises again the signal that stopped the
# script, so it dies by it: bootstrap.sh waits on it in the foreground, and bash goes on after a
# Ctrl-C unless its child died by SIGINT. own_tmpfile sets tmp only once it has taken mktemp's
# answer as a new, empty file of the user's own (#1322), so a mktemp that fails, or answers a file
# that exists, never hands the rm what it printed.
tmp='' sig=''
trap 'trap "" HUP INT TERM; rm -f -- ${tmp:+"$tmp"}
    [ -z "$sig" ] || { trap - "$sig"; kill -s "$sig" $$; }' EXIT
trap 'trap "" HUP INT TERM; sig=HUP; exit 129' HUP
trap 'trap "" HUP INT TERM; sig=INT; exit 130' INT
trap 'trap "" HUP INT TERM; sig=TERM; exit 143' TERM

rc=0
if [ "$uninstall" -eq 0 ] && [ "$dry" -eq 0 ]; then
    mkdir -p "$BIN" || { printf 'cannot create %s\n' "$BIN" >&2; exit 1; }
fi

# No -f: BSD readlink lacks it, and one level answers "did we make this".
link_target() { readlink "$1" 2>/dev/null; }

# Creates, or on --uninstall removes, one link, and only ever one that points here.
link_one() {                  # <link> <target>
    local link=$1 target=$2

    if [ "$uninstall" -eq 1 ]; then
        if [ -L "$link" ] && [ "$(link_target "$link")" = "$target" ]; then
            if rm -- "$link"; then
                printf 'removed  %s\n' "$link"
            else
                printf 'FAILED   %s (cannot remove the link)\n' "$link" >&2
                rc=1
            fi
        fi
        return 0
    fi

    if [ -e "$link" ] && [ ! -L "$link" ]; then
        printf 'SKIP     %s already exists and is not a symlink\n' "$link" >&2
        return 0
    fi
    # A same-named link pointing elsewhere belongs to another tool ("tm" is popular).
    if [ -L "$link" ] && [ "$(link_target "$link")" != "$target" ]; then
        printf 'SKIP     %s is a symlink to %s, not ours\n' "$link" "$(link_target "$link")" >&2
        return 0
    fi
    if ln -sfn "$target" "$link" 2>/dev/null; then
        printf 'linked   %-24s -> %s\n' "$link" "$target"
    else
        printf 'FAILED   %s (cannot create the link)\n' "$link" >&2
        rc=1
    fi
}

if [ "$links" -eq 0 ]; then
    :
elif [ "$dry" -eq 0 ]; then
for c in "${COMMANDS[@]}"; do link_one "$BIN/$c" "$DIR/scripts/$c"; done
# Skills only where Claude Code is installed, so a plain shell box gets no stray dir.
if claude_cfg_dir && [ -d "$claude_cfg" ]; then
    [ "$uninstall" -eq 0 ] && mkdir -p "$claude_cfg/skills" 2>/dev/null
    for s in "$DIR"/skills/*/; do    # [ -d ]: an empty skills/ leaves the glob unexpanded
        [ -d "$s" ] || continue
        s=${s%/}; link_one "$claude_cfg/skills/${s##*/}" "$s"
    done
fi
else
    printf 'dry-run: skipping the symlink step\n'
fi

# Required (#199): a release through the user's gh login, else a source build.
if [ "$uninstall" -eq 0 ] && [ "$dry" -eq 0 ]; then
    "$DIR/scripts/tui-fetch" --required || rc=1
fi

# readlink -f is GNU-only, so the chain is resolved by hand. At most 40 links, as the
# kernel follows: 1 and no output for a cycle or a longer chain (#1288).
resolve_link() {
    local p="$1" d b n=0
    while [ -L "$p" ]; do
        [ "$((n += 1))" -le 40 ] || return 1
        d=$(dirname "$p"); b=$(readlink "$p")
        case "$b" in /*) p="$b" ;; *) p="$d/$b" ;; esac
    done
    printf '%s/%s\n' "$(cd -P "$(dirname "$p")" && pwd)" "$(basename "$p")"
}
cycle_msg() {                 # <link>
    printf 'refusing: %s is a symlink cycle (or a chain of more than 40 links)\n' "$1" >&2
}

# The cockpit-events mod (docs/mod.md), through claude's own plugin CLI. The marketplace is this
# checkout, a local folder, so Claude Code loads the mod in place, beside the core it runs. A
# marketplace of that name is ours only when it is this folder: another one is named and left.
# No TMUX_PANE: a `claude plugin` call can fire the SessionEnd hooks, and agent-state off would
# clear the pane this runs in. With none, agent-state finds no pane and writes nothing.
mod_call() {                  # <claude args>...: its stdout in mod_out, else FAILED and rc=1
    local r
    mod_out=$(env -u TMUX_PANE claude "$@") && return 0
    r=$?
    printf 'FAILED   claude %s (exit %s)\n' "$*" "$r" >&2
    [ -z "$mod_out" ] || printf '%s\n' "$mod_out" >&2
    rc=1
    return 1
}
mod_step() {                  # install | remove
    local mf mk pl at
    mod_names || { printf 'FAILED   cannot read the mod marketplace %s\n' "$mf" >&2; rc=1; return; }
    if ! command -v claude >/dev/null 2>&1; then
        printf 'mod       no claude on PATH; %s@%s skipped (docs/mod.md)\n' "$pl" "$mk"
        return
    fi
    if [ "$dry" -eq 1 ] && [ "$1" = install ]; then
        printf 'dry-run: would run claude plugin marketplace add %s --scope user,' "$DIR"
        printf ' unless a %s marketplace is there\n' "$mk"
        printf 'dry-run: would run claude plugin install %s@%s --scope user\n' "$pl" "$mk"
        return
    elif [ "$dry" -eq 1 ]; then
        printf 'dry-run: would run claude plugin marketplace remove %s --scope user,' "$mk"
        printf ' if it is this checkout\n'
        return
    fi
    mod_call plugin marketplace list --json || return
    # none, ours, or the other marketplace's source and where it is
    at=$(printf '%s' "$mod_out" |
            jq -er --arg n "$mk" --arg d "$DIR" "$mod_where_jq"' mod_where($n; $d)' 2>/dev/null) ||
        { printf 'FAILED   claude plugin marketplace list --json printed no list\n' >&2
          rc=1; return; }
    case $1:$at in
        install:none) mod_call plugin marketplace add "$DIR" --scope user || return ;;
        install:ours) ;;
        install:*)
            printf 'SKIP     the %s marketplace is %s, not this checkout; %s@%s not installed\n' \
                "$mk" "$at" "$pl" "$mk" >&2
            return ;;
        remove:none) printf 'mod       no %s marketplace, nothing to remove\n' "$mk"; return ;;
        remove:ours)
            mod_call plugin marketplace remove "$mk" --scope user &&
                printf 'removed   the %s marketplace and %s@%s\n' "$mk" "$pl" "$mk"
            return ;;
        *)
            printf 'SKIP     the %s marketplace is %s, not this checkout; left alone\n' \
                "$mk" "$at" >&2
            return ;;
    esac
    mod_call plugin install "$pl@$mk" --scope user &&
        printf 'mod       %s@%s from %s (run /reload-plugins in a session already open)\n' \
            "$pl" "$mk" "$DIR"
}


# cockpit's own root in a command: the shipped ${TMUX_COCKPIT_DIR:-...} form (as a regex)
# or this checkout's path, so a foreign .../scripts/review-gate is never ours.
pre='\$\{TMUX_COCKPIT_DIR:-\$HOME/\.tmux/plugins/tmux-cockpit\}'

if [ "$hooks" -eq 1 ] && [ "$uninstall" -eq 1 ]; then
    command -v jq >/dev/null 2>&1 || { printf 'jq is required for --hooks (the manual removal in docs/install.md still works)\n' >&2; exit 1; }
    claude_cfg_dir || { printf 'neither CLAUDE_CONFIG_DIR nor HOME is set; cannot locate settings.json\n' >&2; exit 1; }
    settings="$claude_cfg/settings.json"
    if [ ! -e "$settings" ]; then
        # -e is false on a cycle too: that is no absent file, and it is said (#1288).
        if [ -L "$settings" ] && ! resolve_link "$settings" >/dev/null; then
            cycle_msg "$settings"; exit 1
        fi
        printf 'hooks     no %s, nothing to remove\n' "$settings"
        mod_step remove
        exit "$rc"
    fi
    real=$(resolve_link "$settings") || { cycle_msg "$settings"; exit 1; }
    orig=$(cat "$real") || { printf 'cannot read %s\n' "$settings" >&2; exit 1; }
    printf '%s' "$orig" | jq -e . >/dev/null 2>&1 \
        || { printf '%s is not valid JSON; refusing to touch it\n' "$settings" >&2; exit 1; }
    # A hook is cockpit's only when its command is one of this plugin's scripts at
    # the path the shipped examples use (or this checkout's), anchored at both ends.
    stripped=$(printf '%s' "$orig" | jq --arg dir "$DIR" --arg pre "$pre" '
        def esc: gsub("(?<c>[.\\[\\]()*+?{}|^$\\\\])"; "\\\(.c)");
        ($pre + "|" + ($dir | esc)) as $root
        | ("^\"?(" + $root + ")\"?/scripts/(agent-state( [a-z-]+)?|review-gate|worktree-(create|remove))$") as $script
        | ("^jq -Rs .* \"?(" + $root + ")\"?/skills/unslop/SKILL\\.md 2>/dev/null$") as $unslop
        | def mine: (.command // "") | (test($script) or test($unslop));
        if (.hooks | type) == "object" then
            .hooks |= (with_entries(
                .value |= (if type == "array" then
                    map(if (.hooks | type) == "array" then
                            (.hooks | length) as $n | .hooks |= map(select(mine | not))
                            | if $n > 0 and (.hooks | length) == 0 then empty else . end
                        else . end)
                  else . end)
                | select(((.value | type) != "array") or ((.value | length) > 0) or (($orig_hooks[.key] // []) | length) == 0)))
        else . end' --argjson orig_hooks "$(printf '%s' "$orig" | jq -c '.hooks // {} | if type == "object" then . else {} end')") \
        || { printf 'removal failed; %s untouched\n' "$settings" >&2; exit 1; }
    if [ "$(printf '%s' "$stripped" | jq -S -c .)" = "$(printf '%s' "$orig" | jq -S -c .)" ]; then
        printf 'hooks     no cockpit hooks in %s, nothing to remove\n' "$settings"
    elif [ "$dry" -eq 1 ]; then
        if command -v diff >/dev/null 2>&1; then
            diff -u -L "$settings" -L "$settings (cockpit hooks removed)" <(printf '%s\n' "$orig") <(printf '%s\n' "$stripped")
            [ $? -le 1 ] || printf 'diff failed; result would be:\n%s\n' "$stripped"
        else
            printf 'diff not found; result would be:\n%s\n' "$stripped"
        fi
        printf 'dry-run: nothing written\n'
    else
        now=$(cat "$real") || { printf 'cannot read %s\n' "$settings" >&2; exit 1; }
        if [ "$now" != "$orig" ]; then
            printf '%s changed since this run started (a concurrent writer raced the removal); nothing written -- re-run to remove from the current content\n' "$settings" >&2
            exit 1
        fi
        bak="$settings.bak-$(date +%Y%m%d-%H%M%S)"
        cp -p -- "$real" "$bak" || { printf 'cannot write the backup %s; %s untouched\n' "$bak" "$settings" >&2; exit 1; }
        own_tmpfile tmp "$real.XXXXXX" || {
            printf 'write failed; %s untouched (own_tmpfile: %s)\n' "$settings" \
                "$own_tmpfile_why" >&2
            exit 1; }
        cp -p -- "$real" "$tmp" && printf '%s\n' "$stripped" > "$tmp" && mv -- "$tmp" "$real" \
            || { printf 'write failed; %s untouched\n' "$settings" >&2; exit 1; }
        tmp=''
        printf 'removed   cockpit hooks from %s (backup: %s)\n' "$settings" "$bak"
    fi
    mod_step remove
    exit "$rc"
fi

if [ "$hooks" -eq 1 ]; then
    command -v jq >/dev/null 2>&1 || { printf 'jq is required for --hooks (the manual merge in README still works)\n' >&2; exit 1; }
    have_diff=1
    command -v diff >/dev/null 2>&1 || have_diff=0
    claude_cfg_dir || { printf 'neither CLAUDE_CONFIG_DIR nor HOME is set; cannot locate settings.json\n' >&2; exit 1; }
    settings="$claude_cfg/settings.json"

    if [ -e "$settings" ]; then
        # Write at the resolved target: replacing a dotfiles-managed symlink with
        # a plain file loses the merge on the manager's next apply.
        real=$(resolve_link "$settings") || { cycle_msg "$settings"; exit 1; }
        orig=$(cat "$real") || { printf 'cannot read %s\n' "$settings" >&2; exit 1; }
    elif [ -L "$settings" ]; then
        # -e is false on a cycle too: only a chain that ends is dangling (#1288).
        resolve_link "$settings" >/dev/null || { cycle_msg "$settings"; exit 1; }
        # Seeding would follow the link to wherever it points, outside the config dir.
        printf 'refusing to follow the dangling symlink %s\n' "$settings" >&2
        exit 1
    else
        orig='{}'
        real="$settings"
        if [ "$dry" -eq 0 ]; then
            mkdir -p "${settings%/*}" || exit 1
            printf '%s\n' "$orig" > "$settings"
        fi
    fi

    # Additive: their hooks stay, ours are appended unless a deep-equal entry
    # exists. norm makes an absent matcher equal "".
    # Cockpit entries that change shape across versions are replaced (anchored): review-gate
    # and a SessionStart `agent-state off`.
    # review-gate stays the last shipped Stop entry: --hooks strips and re-appends it, so a new Stop hook goes before it.
    merged=$(jq -s '
        def norm: if type == "object" then . + {matcher: (.matcher // "")} else . end;
        def runs($re): (.command // "") | test($re);
        def esc: gsub("(?<c>[.\\[\\]()*+?{}|^$\\\\])"; "\\\(.c)");
        ($pre + "|" + ($dir | esc)) as $root
        | def ours($tail): runs("^\"?(" + $root + ")\"?" + $tail);
        .[0].hooks as $mine | .[1] as $theirs
        | ([$mine.SessionStart[]? | select(any(.hooks[]?; ours("/scripts/agent-state off$"))) | .matcher // ""] | first) as $offm
        | ($theirs.hooks // {} | with_entries(.key as $k | .value |=
            (map(if $k == "SessionStart" and $offm != null and (.matcher // "") != $offm
                 then .hooks |= map(select(ours("/scripts/agent-state off$") | not)) else . end)
             | map(.hooks |= map(select(ours("/scripts/review-gate$") | not)))
             | map(select((.hooks | length) > 0))))) as $h
        | $theirs + { hooks: (
            $h + ($mine | with_entries(
                .key as $k
                | .value = (($h[$k] // []) + [ .value[]
                    | select(. as $e | (($h[$k] // []) | map(norm) | index($e | norm)) == null) ])
              ))
          ) }' --arg dir "$DIR" --arg pre "$pre" "$DIR/examples/claude-hooks.json" <(printf '%s' "$orig")) \
        || { printf 'merge failed; %s untouched\n' "$settings" >&2; exit 1; }

    # Only .hooks, structurally, so a formatting-only difference is not a change.
    if [ "$(printf '%s' "$merged" | jq -S -c '.hooks // {}')" = "$(printf '%s' "$orig" | jq -S -c '.hooks // {}')" ]; then
        already_merged=1
    else
        already_merged=0
    fi

    if [ "$dry" -eq 1 ]; then
        if [ "$already_merged" -eq 1 ]; then
            printf 'hooks     already merged, nothing to do\n'
        elif [ "$have_diff" -eq 1 ]; then
            diff -u -L "$settings" -L "$settings (merged)" <(printf '%s\n' "$orig") <(printf '%s\n' "$merged")
            rc_diff=$?
            # 1 means "they differ"; only above that is diff failing.
            [ "$rc_diff" -le 1 ] || printf 'diff failed; merged result would be:\n%s\n' "$merged"
        else
            printf 'diff not found; merged result would be:\n%s\n' "$merged"
        fi
    elif [ "$already_merged" -eq 1 ]; then
        printf 'hooks     already merged, nothing to do\n'
    else
        # Claude Code rewrites settings.json unlocked on every approval: abort
        # rather than clobber a write that landed while this was merging.
        now=$(cat "$real") || { printf 'cannot read %s\n' "$settings" >&2; exit 1; }
        if [ "$now" != "$orig" ]; then
            printf '%s changed since this run started (a concurrent writer raced the merge); nothing written -- re-run --hooks to merge the current content\n' "$settings" >&2
            exit 1
        fi
        # Only the first backup is kept: a later one would hold our hooks already.
        if [ -e "$settings.bak" ]; then
            bak_note=' (existing backup kept, not overwritten)'
        else
            cp -- "$settings" "$settings.bak" || exit 1
            bak_note=" (backup: $settings.bak)"
        fi
        # A sibling temp file renamed over $real, so an interrupted write never
        # truncates it; cp -p keeps the mode mktemp would tighten to 0600.
        own_tmpfile tmp "$real.XXXXXX" || {
            printf 'write failed; %s untouched (own_tmpfile: %s)\n' "$settings" \
                "$own_tmpfile_why" >&2
            exit 1; }
        cp -p -- "$real" "$tmp" && printf '%s\n' "$merged" > "$tmp" && mv -- "$tmp" "$real" \
            || { printf 'write failed; %s untouched\n' "$settings" >&2; exit 1; }
        tmp=''
        printf 'merged    cockpit hooks into %s%s\n' "$settings" "$bak_note"
    fi
    mod_step install
    # After the mod's would-run lines, so a preview still ends on this line.
    [ "$dry" -eq 0 ] || printf 'dry-run: nothing written\n'
fi

[ "$uninstall" -eq 1 ] && exit "$rc"

case ":$PATH:" in
    *":$BIN:"*) ;;
    *) printf '\nNOTE: %s is not on your $PATH.\n' "$BIN" >&2 ;;
esac

# HOME may be unset here, since only XDG_BIN_HOME was needed above.
if [ -z "${HOME:-}" ]; then
    printf '\nHOME is not set; add this to your tmux config by hand:\n  %s\n' \
        "$(cockpit_conf_line "$DIR")" >&2
else
    # tmux loads both, so the run-shell line in either wires it, as --check reads it.
    conf=""; wst=1; cfs=()
    for c in "${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf" "$HOME/.tmux.conf"; do
        [ -r "$c" ] || continue
        [ -n "$conf" ] || conf="$c"
        cfs+=("$c")
    done
    [ "${#cfs[@]}" -eq 0 ] || { cockpit_conf_wired "$DIR" "${cfs[@]}"; wst=$?; }
    if [ -z "$conf" ]; then
        printf '\nNo tmux config found. Create %s with:\n  %s\n' \
               "$HOME/.tmux.conf" "$(cockpit_conf_line "$DIR")" >&2
    elif [ "$wst" -eq 2 ]; then
        # --check fails this row; a run-shell line beside tpm's would load the plugin twice.
        printf '\ntpm seems to manage cockpit in %s, not confirmed: %s.\n%s\n' "$cockpit_wired" \
               "$(cockpit_conf_tpm_text "$DIR")" 'Run install.sh --check.' >&2
    elif [ "$wst" -ne 0 ] && cockpit_conf_mention "$DIR" "${cfs[@]}"; then
        # bootstrap.sh appends nothing here, and a line added by hand could load two copies.
        # One decision, one message: the line bootstrap.sh prints (#1067).
        printf '\n%s line %s names cockpit; run install.sh --check\n' \
               "$cockpit_mention_file" "$cockpit_mention_n" >&2
    elif [ "$wst" -ne 0 ]; then
        printf '\nAdd this to %s, AFTER every option using an #{agent_*} token:\n  %s\n' \
               "$conf" "$(cockpit_conf_line "$DIR")" >&2
    fi
fi

exit "$rc"
