#!/usr/bin/env bash
# tmux-cockpit installer.
#   ./install.sh            symlink the commands into ~/.local/bin, fetch cockpit-tui
#   ./install.sh --uninstall  remove only the symlinks that point back here
#   ./install.sh --check      the doctor: one row per thing that can be wrong
#   ./install.sh --hooks [--dry-run]  merge examples/claude-hooks.json into ~/.claude/settings.json
#   ./install.sh --uninstall --hooks [--dry-run]  also remove exactly the hooks --hooks added
#   ./install.sh --hooks --remove [--dry-run]     remove only those hooks, keep the links
set -uo pipefail

# Resolved (-P), so the uninstall guard matches links made through a symlinked checkout.
DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="${XDG_BIN_HOME:-${HOME:-}/.local/bin}"
if [ -z "${XDG_BIN_HOME:-}" ] && [ -z "${HOME:-}" ]; then
    printf 'HOME is not set and XDG_BIN_HOME is not set; cannot pick an install directory\n' >&2
    exit 1
fi
COMMANDS=(tm tmux-branch tmux-open-path)

USAGE='usage: install.sh [--check] [--uninstall] [--hooks [--dry-run]]
       install.sh --uninstall --hooks [--dry-run]   also drop cockpit'"'"'s hook entries from settings.json
       install.sh --hooks --remove [--dry-run]      drop only the hook entries, keep the links'
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
# shellcheck source=scripts/lib/tui.sh
. "$DIR/scripts/lib/tui.sh" || exit 1

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

if [ "$check" -eq 1 ]; then
    printf 'tmux-cockpit doctor\n\n'

    v=$(tmux -V 2>/dev/null); v=${v#tmux }
    if [ -z "$v" ]; then row fail tmux 'not found' 'bootstrap.sh builds tmux 3.8'
    elif [ "$(vnum "$v")" -lt "$(vnum 3.8)" ]; then row fail tmux "$v" 'bootstrap.sh builds tmux 3.8'
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

    conf=''; wired=0
    # tmux loads both, so the run-shell line in either wires the box.
    cxdg="${XDG_CONFIG_HOME:-${HOME:-}/.config}/tmux/tmux.conf"; chome="${HOME:-}/.tmux.conf"
    for c in "$cxdg" "$chome"; do
        [ -r "$c" ] || continue
        [ -n "$conf" ] || conf="$c"        # the first readable one names a fail row
        while read -r l || [ -n "$l" ]; do
            case "$l" in *cockpit.tmux*) wired=1; conf="$c"; break ;; esac
        done < "$c"
        [ "$wired" -eq 1 ] && break
    done
    wire_fix="run bootstrap.sh, or add run-shell $DIR/cockpit.tmux"
    if [ "$wired" -eq 1 ]; then row ok config "$conf loads cockpit"
    elif [ -n "$conf" ];    then row fail config "$conf" "$wire_fix"
    else row fail config 'no tmux config' "$wire_fix"
    fi

    # These formats re-expand per pane on every redraw, so a #{P:} walk in them freezes tmux.
    if [ "$wired" -eq 0 ]; then
        row skip formats 'no cockpit config to read'
        row skip tokens 'no cockpit config to read'
    else
        walk=0 tok=0 gone=''
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
                    # Retired options: nothing reads them any more (#223).
                    set*\ @cockpit_keep_swap_pane|set*\ @cockpit_keep_swap_pane[[:space:]]*)
                        gone=@cockpit_keep_swap_pane ;;
                esac
            done < "$c"
        done
        if [ "$walk" -eq 0 ]; then row ok formats 'no pane walk on redraw'
        else
            if [ "$walk" -eq 1 ]; then nf='format walks'; else nf='formats walk'; fi
            row warn formats "$walk $nf panes" \
                "delete them: the plugin draws the bar (see $DIR/examples/tmux.conf)"
        fi
        [ -z "$gone" ] || row warn options "$gone is retired" 'delete the line: tmux 3.8 binds no swap-pane on Ctrl+click'
        if [ "$tok" -eq 0 ]; then row ok tokens 'no #{agent_*} token'
        else
            row warn tokens "$tok lines use #{agent_*}" \
                "see the migration table in $DIR/docs/status-bar.md"
        fi
    fi

    if [ ! -d "${HOME:-}/.claude" ]; then row skip hooks 'no Claude Code'
    elif ! command -v jq >/dev/null 2>&1; then row skip hooks 'jq missing'
    else
        n=$(jq '[.hooks[]?[]?.hooks[]?.command? // ""
                 | select(test("tmux-cockpit|agent-state"))] | length' \
               "$HOME/.claude/settings.json" 2>/dev/null)
        case "${n:-0}" in
            0) row warn hooks '0 cockpit hooks merged' 'install.sh --hooks' ;;
            *) row ok hooks "$n cockpit hooks merged" ;;
        esac
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
if [ -d "${HOME:-}/.claude" ]; then
    [ "$uninstall" -eq 0 ] && mkdir -p "${HOME}/.claude/skills" 2>/dev/null
    for s in "$DIR"/skills/*/; do    # [ -d ]: an empty skills/ leaves the glob unexpanded
        [ -d "$s" ] || continue
        s=${s%/}; link_one "${HOME}/.claude/skills/${s##*/}" "$s"
    done
fi
else
    printf 'dry-run: skipping the symlink step\n'
fi

# Required (#199): a release through the user's gh login, else a source build.
if [ "$uninstall" -eq 0 ] && [ "$dry" -eq 0 ]; then
    "$DIR/scripts/tui-fetch" --required || rc=1
fi

# readlink -f is GNU-only, so the chain is resolved by hand.
resolve_link() {
    local p="$1" d b
    while [ -L "$p" ]; do
        d=$(dirname "$p"); b=$(readlink "$p")
        case "$b" in /*) p="$b" ;; *) p="$d/$b" ;; esac
    done
    printf '%s/%s\n' "$(cd -P "$(dirname "$p")" && pwd)" "$(basename "$p")"
}


# cockpit's own root in a command: the shipped ${TMUX_COCKPIT_DIR:-...} form (as a regex)
# or this checkout's path, so a foreign .../scripts/review-gate is never ours.
pre='\$\{TMUX_COCKPIT_DIR:-\$HOME/\.tmux/plugins/tmux-cockpit\}'

if [ "$hooks" -eq 1 ] && [ "$uninstall" -eq 1 ]; then
    command -v jq >/dev/null 2>&1 || { printf 'jq is required for --hooks (the manual removal in docs/install.md still works)\n' >&2; exit 1; }
    [ -n "${HOME:-}" ] || { printf 'HOME is not set; cannot locate ~/.claude/settings.json\n' >&2; exit 1; }
    settings="$HOME/.claude/settings.json"
    if [ ! -e "$settings" ]; then
        printf 'hooks     no %s, nothing to remove\n' "$settings"
        exit "$rc"
    fi
    real=$(resolve_link "$settings")
    orig=$(cat "$real") || { printf 'cannot read %s\n' "$settings" >&2; exit 1; }
    printf '%s' "$orig" | jq -e . >/dev/null 2>&1 \
        || { printf '%s is not valid JSON; refusing to touch it\n' "$settings" >&2; exit 1; }
    # A hook is cockpit's only when its command is one of this plugin's scripts at
    # the path the shipped examples use (or this checkout's), anchored at both ends.
    stripped=$(printf '%s' "$orig" | jq --arg dir "$DIR" --arg pre "$pre" '
        def esc: gsub("(?<c>[.\\[\\]()*+?{}|^$\\\\])"; "\\\(.c)");
        ($pre + "|" + ($dir | esc)) as $root
        | ("^\"?(" + $root + ")\"?/scripts/(agent-state( [a-z-]+)?|review-gate( --pretool)?|worktree-(create|remove))$") as $script
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
        tmp=$(mktemp "$real.XXXXXX") || { printf 'write failed; %s untouched\n' "$settings" >&2; exit 1; }
        trap 'rm -f -- "$tmp"' EXIT
        cp -p -- "$real" "$tmp" && printf '%s\n' "$stripped" > "$tmp" && mv -- "$tmp" "$real" \
            || { printf 'write failed; %s untouched\n' "$settings" >&2; exit 1; }
        trap - EXIT
        printf 'removed   cockpit hooks from %s (backup: %s)\n' "$settings" "$bak"
    fi
    exit "$rc"
fi

if [ "$hooks" -eq 1 ]; then
    command -v jq >/dev/null 2>&1 || { printf 'jq is required for --hooks (the manual merge in README still works)\n' >&2; exit 1; }
    have_diff=1
    command -v diff >/dev/null 2>&1 || have_diff=0
    # settings.json has no XDG override, so $HOME is required here.
    [ -n "${HOME:-}" ] || { printf 'HOME is not set; cannot locate ~/.claude/settings.json\n' >&2; exit 1; }
    settings="$HOME/.claude/settings.json"

    if [ -e "$settings" ]; then
        # Write at the resolved target: replacing a dotfiles-managed symlink with
        # a plain file loses the merge on the manager's next apply.
        real=$(resolve_link "$settings")
        orig=$(cat "$real") || { printf 'cannot read %s\n' "$settings" >&2; exit 1; }
    elif [ -L "$settings" ]; then
        # Seeding would follow the link to wherever it points, outside ~/.claude.
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
    # Cockpit entries that change shape across versions are replaced (anchored): review-gate,
    # an old `--pretool` (review-gate exits 0 on it), a SessionStart `agent-state off`.
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
             | map(.hooks |= map(select(ours("/scripts/review-gate( --pretool)?$") | not)))
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
        printf 'dry-run: nothing written\n'
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
        tmp=$(mktemp "$real.XXXXXX") || { printf 'write failed; %s untouched\n' "$settings" >&2; exit 1; }
        trap 'rm -f -- "$tmp"' EXIT
        cp -p -- "$real" "$tmp" && printf '%s\n' "$merged" > "$tmp" && mv -- "$tmp" "$real" \
            || { printf 'write failed; %s untouched\n' "$settings" >&2; exit 1; }
        trap - EXIT
        printf 'merged    cockpit hooks into %s%s\n' "$settings" "$bak_note"
    fi
fi

[ "$uninstall" -eq 1 ] && exit "$rc"

case ":$PATH:" in
    *":$BIN:"*) ;;
    *) printf '\nNOTE: %s is not on your $PATH.\n' "$BIN" >&2 ;;
esac

# HOME may be unset here, since only XDG_BIN_HOME was needed above.
if [ -z "${HOME:-}" ]; then
    printf '\nHOME is not set; add this to your tmux config by hand:\n  run-shell %s/cockpit.tmux\n' "$DIR" >&2
else
    # tmux loads both, so the run-shell line in either wires it, as --check reads it.
    conf=""; wired=0
    for c in "${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf" "$HOME/.tmux.conf"; do
        [ -r "$c" ] || continue
        [ -n "$conf" ] || conf="$c"
        grep -q 'cockpit\.tmux' "$c" && { wired=1; break; }
    done
    if [ -z "$conf" ]; then
        printf '\nNo tmux config found. Create %s with:\n  run-shell %s/cockpit.tmux\n' \
               "$HOME/.tmux.conf" "$DIR" >&2
    elif [ "$wired" -eq 0 ]; then
        printf '\nAdd this to %s, AFTER every option using an #{agent_*} token:\n  run-shell %s/cockpit.tmux\n' \
               "$conf" "$DIR" >&2
    fi
fi

exit "$rc"
