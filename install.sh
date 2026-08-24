#!/usr/bin/env bash
# tmux-cockpit installer.
#   ./install.sh            symlink the commands into ~/.local/bin
#   ./install.sh --uninstall  remove only the symlinks that point back here
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
BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
COMMANDS=(tm tmux-branch tmux-open-path)

uninstall=0; hooks=0; dry=0
for a in "$@"; do case "$a" in
    "") ;;
    --uninstall) uninstall=1 ;;
    --hooks)     hooks=1 ;;
    --dry-run)   dry=1 ;;
    # A typo used to install silently. "./install.sh --uninstal" removing
    # nothing while reporting nothing is the worst possible answer.
    *) printf 'unknown option: %s\n\nusage: install.sh [--uninstall] [--hooks [--dry-run]]\n' "$a" >&2; exit 2 ;;
esac; done
# --dry-run only means something as a preview of --hooks; alone it previews
# nothing and silently doing so would look like a no-op success.
[ "$dry" -eq 1 ] && [ "$hooks" -eq 0 ] && { printf -- '--dry-run needs --hooks\n' >&2; exit 2; }
# --uninstall does NOT touch hooks (see the usage comment up top). Letting
# --hooks ride along and silently merging anyway would be exactly the
# silent-wrong-answer this script refuses everywhere else -- a hard error is
# the honest answer here, not a quiet no-op.
[ "$uninstall" -eq 1 ] && [ "$hooks" -eq 1 ] && { printf -- '--uninstall does not touch hooks; drop --hooks or run it separately\n' >&2; exit 2; }

rc=0
# Not on --dry-run: a preview must not create anything either -- see the
# symlink step below, which this pairs with.
if [ "$dry" -eq 0 ]; then
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
if [ "$dry" -eq 0 ]; then
for c in "${COMMANDS[@]}"; do
    link="$BIN/$c"
    target="$DIR/scripts/$c"

    if [ "$uninstall" -eq 1 ]; then
        # Only ever remove a link that points back into this plugin. Deleting
        # whatever happens to sit at that name would take someone else's tool
        # with it.
        if [ -L "$link" ] && [ "$(link_target "$link")" = "$target" ]; then
            rm -- "$link" && printf 'removed  %s\n' "$link"
        fi
        continue
    fi

    if [ -e "$link" ] && [ ! -L "$link" ]; then
        printf 'SKIP     %s already exists and is not a symlink\n' "$link" >&2
        continue
    fi
    # A symlink of the same name that points somewhere else belongs to another
    # tool -- "tm" is a name plenty of things want. Overwriting it silently is
    # bad enough; --uninstall would then delete it too, leaving neither.
    if [ -L "$link" ] && [ "$(link_target "$link")" != "$target" ]; then
        printf 'SKIP     %s is a symlink to %s, not ours\n' "$link" "$(link_target "$link")" >&2
        continue
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
done
else
    printf 'dry-run: skipping the symlink step\n'
fi

if [ "$hooks" -eq 1 ]; then
    command -v jq >/dev/null 2>&1 || { printf 'jq is required for --hooks (the manual merge in README still works)\n' >&2; exit 1; }
    have_diff=1
    command -v diff >/dev/null 2>&1 || have_diff=0
    settings="$HOME/.claude/settings.json"

    if [ -e "$settings" ]; then
        orig=$(cat "$settings") || { printf 'cannot read %s\n' "$settings" >&2; exit 1; }
    elif [ -L "$settings" ]; then
        # A dangling symlink passes neither "read what's there" (there's
        # nothing) nor "seed a fresh file" safely -- the seed write below
        # follows a symlink, so it would land wherever the link points,
        # outside ~/.claude entirely.
        printf 'refusing to follow the dangling symlink %s\n' "$settings" >&2
        exit 1
    else
        orig='{}'
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
    merged=$(jq -s '
        .[0].hooks as $mine | .[1] as $theirs
        | $theirs + { hooks: (
            ($theirs.hooks // {}) as $h
            | $h + ($mine | with_entries(
                .key as $k
                | .value = (($h[$k] // []) + [ .value[]
                    | select(. as $e | (($h[$k] // []) | index($e)) == null) ])
              ))
          ) }' "$DIR/examples/claude-hooks.json" <(printf '%s' "$orig")) \
        || { printf 'merge failed; %s untouched\n' "$settings" >&2; exit 1; }

    if [ "$dry" -eq 1 ]; then
        if [ "$have_diff" -eq 1 ]; then
            diff -u <(printf '%s\n' "$orig") <(printf '%s\n' "$merged")
            rc_diff=$?
            # 1 just means "they differ", which is the expected case here.
            # Anything higher is a real diff failure and must not silently
            # look like a clean preview.
            [ "$rc_diff" -le 1 ] || printf 'diff failed; merged result would be:\n%s\n' "$merged"
        else
            printf 'diff not found; merged result would be:\n%s\n' "$merged"
        fi
        printf 'dry-run: nothing written\n'
    # Compare only .hooks, structurally (jq -S -c, sorted and compact) --
    # comparing the whole file as text rewrote (and I2-clobbered) the backup
    # on every formatting-only difference, even when cockpit's hooks were
    # already fully present.
    elif [ "$(printf '%s' "$merged" | jq -S -c '.hooks // {}')" = "$(printf '%s' "$orig" | jq -S -c '.hooks // {}')" ]; then
        printf 'hooks     already merged, nothing to do\n'
    else
        # Preserve the FIRST backup only. Overwriting it on every modifying
        # run would replace a user's pre-existing settings.json.bak, or -- on
        # a second modifying run -- replace the pristine original with
        # content that already has our hooks merged in.
        [ -e "$settings.bak" ] || cp -- "$settings" "$settings.bak" || exit 1
        # `printf > "$settings"` truncates before it writes a single byte, so
        # a SIGXFSZ or any other interrupted write (reproduced with
        # ulimit -f 1) leaves a truncated, invalid settings.json -- and the
        # rollback above never runs, because by then $settings has already
        # been destroyed. Write to a sibling temp file instead and rename it
        # over the original: rename() is atomic, so $settings is either the
        # old content or the new content, never a half-written file between.
        tmp=$(mktemp "$settings.XXXXXX") && printf '%s\n' "$merged" > "$tmp" && mv -- "$tmp" "$settings" \
            || { rm -f -- "$tmp"; printf 'write failed; %s untouched\n' "$settings" >&2; exit 1; }
        printf 'merged    cockpit hooks into %s (backup: %s.bak)\n' "$settings" "$settings"
    fi
fi

[ "$uninstall" -eq 1 ] && exit "$rc"

case ":$PATH:" in
    *":$BIN:"*) ;;
    *) printf '\nNOTE: %s is not on your $PATH.\n' "$BIN" >&2 ;;
esac

# Both locations, and say it even when neither exists -- a user with no tmux
# config got no instruction at all, which is exactly the user who needs one.
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

exit "$rc"
