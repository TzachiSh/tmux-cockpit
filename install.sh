#!/usr/bin/env bash
# tmux-cockpit installer.
#   ./install.sh            symlink the commands into ~/.local/bin
#   ./install.sh --uninstall  remove only the symlinks that point back here
#   ./install.sh --check      the doctor: one row per thing that can be wrong
#
# The tmux side needs no install: one run-shell line in ~/.tmux.conf loads
# cockpit.tmux, which binds its own keys. This script exists for the two scripts
# you also type by name, since a plugin directory is not on $PATH.
set -uo pipefail

# -P so DIR is the RESOLVED path. The uninstall guard below compares it against
# readlink -f of the link; with a logical path those never match when the plugin
# is reached through a symlink (a dotfiles checkout linked into ~/.tmux/plugins
# is the usual way), and --uninstall then removed nothing and said nothing.
DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# ${HOME:-}, not a bare $HOME: under `set -u` an unset HOME aborted here with
# a raw "HOME: unbound variable" instead of a message that says what's wrong.
BIN="${XDG_BIN_HOME:-${HOME:-}/.local/bin}"
if [ -z "${XDG_BIN_HOME:-}" ] && [ -z "${HOME:-}" ]; then
    printf 'HOME is not set and XDG_BIN_HOME is not set; cannot pick an install directory\n' >&2
    exit 1
fi
COMMANDS=(tm tmux-branch tmux-open-path)

USAGE='usage: install.sh [--check] [--uninstall] [--hooks [--dry-run]]'
uninstall=0; hooks=0; dry=0; check=0
for a in "$@"; do case "$a" in
    "") ;;
    -h|--help) printf '%s\n' "$USAGE"; exit 0 ;;
    --uninstall) uninstall=1 ;;
    --check)     check=1 ;;
    --hooks)     hooks=1 ;;
    --dry-run)   dry=1 ;;
    # A typo used to install silently. "./install.sh --uninstal" removing
    # nothing while reporting nothing is the worst possible answer.
    *) printf 'unknown option: %s\n\n%s\n' "$a" "$USAGE" >&2; exit 2 ;;
esac; done
# --check reports and changes nothing, so pairing it with a flag that writes
# would mean one of the two was silently ignored.
[ "$check" -eq 1 ] && [ $((uninstall + hooks + dry)) -gt 0 ] && {
    printf -- '--check runs alone\n\n%s\n' "$USAGE" >&2; exit 2; }
# --dry-run only means something as a preview of --hooks; alone it previews
# nothing and silently doing so would look like a no-op success.
[ "$dry" -eq 1 ] && [ "$hooks" -eq 0 ] && { printf -- '--dry-run needs --hooks\n' >&2; exit 2; }
# --uninstall does NOT touch hooks (see the usage comment up top). Letting
# --hooks ride along and silently merging anyway would be exactly the
# silent-wrong-answer this script refuses everywhere else -- a hard error is
# the honest answer here, not a quiet no-op.
[ "$uninstall" -eq 1 ] && [ "$hooks" -eq 1 ] && { printf -- '--uninstall does not touch hooks; drop --hooks or run it separately\n' >&2; exit 2; }


# ---------------------------------------------------------------- doctor ----
# vnum: 0.74.3 -> 0074003, 3.7c -> 3007000. The twin is in scripts/tui-fetch;
# neither script can source the other. Never cockpit.tmux's version_at_least,
# which reads ${v#*.} as the minor and grades 0.74.3 as 0.
vnum() {
    local v=${1%% *}; v=${v#v}; local IFS=.
    # shellcheck disable=SC2086  # split on the dots is the point
    set -- $v
    local a=${1:-0} b=${2:-0} c=${3:-0}
    a=${a%%[!0-9]*}; b=${b%%[!0-9]*}; c=${c%%[!0-9]*}
    printf '%d%03d%03d\n' "${a:-0}" "${b:-0}" "${c:-0}"
}

# Each tool names its version in its own field. A builtin, not awk: the fail
# rows still have to be right on a PATH with nothing on it.
fld() {                       # fld <field> <version line>
    local n=$1 i=1 w
    # shellcheck disable=SC2086  # ditto, on whitespace
    set -- $2
    for w in "$@"; do
        [ "$i" -eq "$n" ] && { printf '%s\n' "$w"; return 0; }
        i=$((i+1))
    done
}

# The exit is the worst row: 0 ok or skip, 1 warn, 2 fail. skip is 0 on
# purpose -- a bare shell cannot answer the last two rows, and a doctor that
# cried failure over that would be ignored the way brew doctor warns it can be.
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
    if [ -z "$v" ]; then row fail tmux 'not found' 'install tmux >= 3.3'
    elif [ "$(vnum "$v")" -lt "$(vnum 3.3)" ]; then row fail tmux "$v" 'install tmux >= 3.3'
    elif [ "$(vnum "$v")" -lt "$(vnum 3.7)" ]; then
        row warn tmux "$v" 'ctrl+click features off below 3.7'
    else row ok tmux "$v"
    fi

    # A warning, not a failure: the status bar and the pickers work without jq.
    v=$(jq --version 2>/dev/null)
    if [ -n "$v" ]; then row ok jq "$v"
    else row warn jq 'not found' 'needed by install.sh --hooks and the review gate'
    fi

    v=$(fld 1 "$(fzf --version 2>/dev/null)")
    if [ -z "$v" ]; then
        row warn fzf 'not found' 'bootstrap.sh --full (pickers fall back to a menu)'
    elif [ "$(vnum "$v")" -lt "$(vnum 0.63.0)" ]; then
        row warn fzf "$v" 'bootstrap.sh --full (styled pickers)'
    else row ok fzf "$v"
    fi

    # Debian names it batcat, and the path matters here: which one won is the
    # answer to "the preview is still plain text".
    bp=$(command -v bat 2>/dev/null) || bp=$(command -v batcat 2>/dev/null) || bp=''
    if [ -n "$bp" ]; then row ok bat "$(fld 2 "$("$bp" --version 2>/dev/null)") @ $bp"
    else row warn bat 'not found' 'optional; bootstrap.sh --full'
    fi

    v=$(fld 3 "$(gum --version 2>/dev/null)"); v=${v#v}
    if [ -z "$v" ] || [ "$(vnum "$v")" -lt "$(vnum 2.0.0)" ]; then
        row warn gum "${v:-not found}" 'optional; bootstrap.sh --full'
    else row ok gum "$v"
    fi

    # Order, not just presence: a fetched fzf in $BIN does nothing for anyone
    # if apt's copy still answers first.
    case ":$PATH:" in
        *":$BIN:"*)
            shadow=''; sdir=''
            for t in fzf bat gum; do
                [ -x "$BIN/$t" ] || continue
                w=$(command -v "$t" 2>/dev/null) || w=''
                [ -n "$w" ] && [ "$w" != "$BIN/$t" ] && { shadow=$t; sdir=${w%/*}; break; }
            done
            if [ -n "$shadow" ]; then
                row warn PATH "$BIN present" "put $BIN before $sdir in PATH"
            elif [ "${PATH%%:*}" = "$BIN" ]; then row ok PATH "$BIN first"
            else row ok PATH "$BIN present"
            fi ;;
        *) row fail PATH 'not on PATH' "add $BIN to PATH" ;;
    esac

    conf=''; wired=0
    # Both candidates, not the first readable one: tmux loads both, so the
    # run-shell line in either wires the box.
    for c in "${XDG_CONFIG_HOME:-${HOME:-}/.config}/tmux/tmux.conf" "${HOME:-}/.tmux.conf"; do
        [ -r "$c" ] || continue
        [ -n "$conf" ] || conf="$c"        # the first readable one names a fail row
        # read, not grep: this row still has to answer on a PATH with nothing on it.
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
# Not on --uninstall (nothing gets created there) and not on --dry-run (a
# preview must not create anything either -- see the symlink step below).
if [ "$uninstall" -eq 0 ] && [ "$dry" -eq 0 ]; then
    mkdir -p "$BIN" || { printf 'cannot create %s\n' "$BIN" >&2; exit 1; }
fi

# BSD readlink had no -f until macOS 12.3, where it returns empty and prints
# "illegal option". Empty then compares unequal to every target, so the guards
# below would refuse to touch our OWN links and uninstall would remove nothing
# -- silently, with status 0. Only one level of link is ever needed here: the
# question is just "did we make this".
link_target() { readlink "$1" 2>/dev/null; }

# --dry-run only ever previews --hooks (enforced above); it must not touch the
# symlinks at all. This used to run unconditionally, so "--hooks --dry-run" on
# a fresh $HOME still created three links and then reported "dry-run: nothing
# written" underneath them.
# One link, created or -- on --uninstall -- removed, but only ever one that is
# ours. A function, not the loop body it used to be, because the skill link
# below lives outside $BIN and outside the COMMANDS loop and needs exactly
# these guards. `return`, not the `continue` this read as inside the loop: a
# `continue` in a function is a no-op with a warning on stderr, in bash even
# when the caller is a loop.
link_one() {                  # <link> <target>
    local link=$1 target=$2

    if [ "$uninstall" -eq 1 ]; then
        # Only ever remove a link that points back into this plugin. Deleting
        # whatever happens to sit at that name would take someone else's tool
        # with it.
        if [ -L "$link" ] && [ "$(link_target "$link")" = "$target" ]; then
            if rm -- "$link"; then
                printf 'removed  %s\n' "$link"
            else
                # A read-only $BIN made rm fail silently before this: exit 0,
                # nothing printed, the link still there.
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
    # A symlink of the same name that points somewhere else belongs to another
    # tool -- "tm" is a name plenty of things want. Overwriting it silently is
    # bad enough; --uninstall would then delete it too, leaving neither.
    if [ -L "$link" ] && [ "$(link_target "$link")" != "$target" ]; then
        printf 'SKIP     %s is a symlink to %s, not ours\n' "$link" "$(link_target "$link")" >&2
        return 0
    fi
    # Reporting "linked" after ln failed is worse than failing: a read-only bin
    # directory produced three permission errors and three success lines, and
    # exited 0.
    if ln -sfn "$target" "$link" 2>/dev/null; then
        printf 'linked   %-24s -> %s\n' "$link" "$target"
    else
        printf 'FAILED   %s (cannot create the link)\n' "$link" >&2
        rc=1
    fi
}

if [ "$dry" -eq 0 ]; then
for c in "${COMMANDS[@]}"; do link_one "$BIN/$c" "$DIR/scripts/$c"; done
# Every skills/* dir, for Claude Code -- only where Claude Code is installed
# (~/.claude exists), so a plain shell box gets no stray dir.
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

if [ "$hooks" -eq 1 ]; then
    command -v jq >/dev/null 2>&1 || { printf 'jq is required for --hooks (the manual merge in README still works)\n' >&2; exit 1; }
    have_diff=1
    command -v diff >/dev/null 2>&1 || have_diff=0
    # Claude Code's settings.json has no XDG override, unlike $BIN above --
    # $HOME is required here, and must be checked before it is dereferenced.
    [ -n "${HOME:-}" ] || { printf 'HOME is not set; cannot locate ~/.claude/settings.json\n' >&2; exit 1; }
    settings="$HOME/.claude/settings.json"

    # readlink -f is GNU-only (the same BSD caveat documented above for
    # link_target), so resolve a symlink chain by hand, with the cd -P
    # discipline DIR itself uses at the top of this script.
    resolve_link() {
        local p="$1" d b
        while [ -L "$p" ]; do
            d=$(dirname "$p"); b=$(readlink "$p")
            case "$b" in /*) p="$b" ;; *) p="$d/$b" ;; esac
        done
        printf '%s/%s\n' "$(cd -P "$(dirname "$p")" && pwd)" "$(basename "$p")"
    }

    if [ -e "$settings" ]; then
        # A valid symlink (settings.json tracked by chezmoi, home-manager, or
        # by hand) must still be a symlink afterward -- writing AT the link
        # itself, which is the atomic-rename idiom's usual behavior (and
        # `sed -i`'s default, Debian #323850), would replace the tracked
        # dotfiles source with a plain file; the next chezmoi/home-manager
        # apply then restores the symlink and drops the merged hooks for
        # good. Resolve the chain and write at $real instead.
        real=$(resolve_link "$settings")
        orig=$(cat "$real") || { printf 'cannot read %s\n' "$settings" >&2; exit 1; }
    elif [ -L "$settings" ]; then
        # A dangling symlink passes neither "read what's there" (there's
        # nothing) nor "seed a fresh file" safely -- the seed write below
        # follows a symlink, so it would land wherever the link points,
        # outside ~/.claude entirely.
        printf 'refusing to follow the dangling symlink %s\n' "$settings" >&2
        exit 1
    else
        orig='{}'
        real="$settings"
        # A preview must not create ~/.claude or seed settings.json either --
        # the diff below is computed against $orig directly, without a file.
        if [ "$dry" -eq 0 ]; then
            mkdir -p "${settings%/*}" || exit 1
            printf '%s\n' "$orig" > "$settings"
        fi
    fi

    # Additive, never destructive: their hooks stay; ours are appended per
    # event, skipping any entry already present. index() does jq's generic
    # (order-insensitive, deep) equality, so reformatting the file -- jq -S,
    # or Claude Code rewriting its own settings -- no longer makes every
    # cockpit hook look new and get appended a second time.
    # norm papers over the one difference deep equality is too honest about:
    # "matcher" is optional and "" when absent, so a live entry carrying
    # matcher:"" and a shipped one without it are the same hook. Measured:
    # exactly that shape mismatch on the Stop event made every --hooks run
    # append one more duplicate, forever, while the other ten events held.
    # review-gate's own Stop entry changes shape across versions (timeout 5 ->
    # 330, the hold added) -- additive alone would keep the old one forever
    # beside the new. Drop any of THEIRS whose command ENDS in
    # /scripts/review-gate or /scripts/review-gate --pretool first (anchored
    # -- an unanchored substring test would also delete a foreign hook that
    # merely logs to a path containing that text), so an upgrade replaces
    # cockpit's own entries instead of piling a second one on.
    merged=$(jq -s '
        def norm: if type == "object" then . + {matcher: (.matcher // "")} else . end;
        .[0].hooks as $mine | .[1] as $theirs
        | ($theirs.hooks // {} | with_entries(.value |=
            (map(.hooks |= map(select(((.command // "") | test("/scripts/review-gate( --pretool)?\"?$")) | not)))
             | map(select((.hooks | length) > 0))))) as $h
        | $theirs + { hooks: (
            $h + ($mine | with_entries(
                .key as $k
                | .value = (($h[$k] // []) + [ .value[]
                    | select(. as $e | (($h[$k] // []) | map(norm) | index($e | norm)) == null) ])
              ))
          ) }' "$DIR/examples/claude-hooks.json" <(printf '%s' "$orig")) \
        || { printf 'merge failed; %s untouched\n' "$settings" >&2; exit 1; }

    # Compare only .hooks, structurally (jq -S -c, sorted and compact) --
    # comparing the whole file as text rewrote (and I2-clobbered) the backup
    # on every formatting-only difference, even when cockpit's hooks were
    # already fully present. Computed once, up front: a dry-run must predict
    # the same "already merged" outcome the real run would report, not just
    # an empty diff plus "nothing written".
    if [ "$(printf '%s' "$merged" | jq -S -c '.hooks // {}')" = "$(printf '%s' "$orig" | jq -S -c '.hooks // {}')" ]; then
        already_merged=1
    else
        already_merged=0
    fi

    if [ "$dry" -eq 1 ]; then
        if [ "$already_merged" -eq 1 ]; then
            printf 'hooks     already merged, nothing to do\n'
        elif [ "$have_diff" -eq 1 ]; then
            # -L names both sides with the real path instead of the process
            # substitution's /dev/fd/NN, which a plain diff -u would print.
            diff -u -L "$settings" -L "$settings (merged)" <(printf '%s\n' "$orig") <(printf '%s\n' "$merged")
            rc_diff=$?
            # 1 just means "they differ", which is the expected case here.
            # Anything higher is a real diff failure and must not silently
            # look like a clean preview.
            [ "$rc_diff" -le 1 ] || printf 'diff failed; merged result would be:\n%s\n' "$merged"
        else
            printf 'diff not found; merged result would be:\n%s\n' "$merged"
        fi
        printf 'dry-run: nothing written\n'
    elif [ "$already_merged" -eq 1 ]; then
        printf 'hooks     already merged, nothing to do\n'
    else
        # $orig was read once, up front (before the jq merge and the .hooks
        # diff above -- real wall time), and $merged was computed from that
        # one read. Claude Code rewrites its own settings.json, unlocked, on
        # every permission approval; measured window here is ~54ms, plenty
        # for a live session's write to land while this script is still
        # merging and then get silently overwritten by content computed from
        # the stale $orig -- reported as a successful merge. Re-read right
        # before touching anything and abort rather than clobber if it
        # moved: the concurrent write is real data nobody asked us to
        # discard, and it is never lost -- it is simply left as the current
        # content of $real, and a second --hooks run re-reads it fresh and
        # merges cleanly on top.
        now=$(cat "$real") || { printf 'cannot read %s\n' "$settings" >&2; exit 1; }
        if [ "$now" != "$orig" ]; then
            printf '%s changed since this run started (a concurrent writer raced the merge); nothing written -- re-run --hooks to merge the current content\n' "$settings" >&2
            exit 1
        fi
        # Preserve the FIRST backup only. Overwriting it on every modifying
        # run would replace a user's pre-existing settings.json.bak, or -- on
        # a second modifying run -- replace the pristine original with
        # content that already has our hooks merged in. And say so plainly:
        # reporting "(backup: ...)" when an existing .bak was kept, not
        # written, would point at a rollback that isn't actually one.
        if [ -e "$settings.bak" ]; then
            bak_note=' (existing backup kept, not overwritten)'
        else
            cp -- "$settings" "$settings.bak" || exit 1
            bak_note=" (backup: $settings.bak)"
        fi
        # `printf > "$real"` truncates before it writes a single byte, so a
        # SIGXFSZ or any other interrupted write (reproduced with
        # ulimit -f 1) leaves a truncated, invalid settings.json -- and the
        # rollback above never runs, because by then the file has already
        # been destroyed. Write to a sibling temp file instead and rename it
        # over the resolved target: rename() is atomic, so $real is either
        # the old content or the new content, never a half-written file
        # between. The temp file is created AT $real (not at $settings), so
        # a symlinked settings.json still resolves onto one filesystem and
        # the rename stays atomic; cp -p carries the original's mode onto it
        # first, since mktemp's own 0600 would otherwise silently tighten
        # settings.json's permissions on every merge.
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

# Both locations, and say it even when neither exists -- a user with no tmux
# config got no instruction at all, which is exactly the user who needs one.
# HOME may legitimately be unset here (only XDG_BIN_HOME was needed for the
# symlink step above), so this whole check degrades to the DIR-only message
# instead of dereferencing an unset $HOME.
if [ -z "${HOME:-}" ]; then
    printf '\nHOME is not set; add this to your tmux config by hand:\n  run-shell %s/cockpit.tmux\n' "$DIR" >&2
else
    conf=""
    for c in "${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf" "$HOME/.tmux.conf"; do
        [ -r "$c" ] && { conf="$c"; break; }
    done
    if [ -z "$conf" ]; then
        printf '\nNo tmux config found. Create %s with:\n  run-shell %s/cockpit.tmux\n' \
               "$HOME/.tmux.conf" "$DIR" >&2
    elif ! grep -q 'cockpit\.tmux' "$conf"; then
        printf '\nAdd this to %s, AFTER every option using an #{agent_*} token:\n  run-shell %s/cockpit.tmux\n' \
               "$conf" "$DIR" >&2
    fi
fi

exit "$rc"
