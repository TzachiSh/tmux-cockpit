#!/usr/bin/env bash
# tmux-cockpit installer.
#   ./install.sh            symlink the commands into ~/.local/bin
#   ./install.sh --uninstall  remove only the symlinks that point back here
#
# The tmux side needs no install: one run-shell line in ~/.tmux.conf loads
# cockpit.tmux, which binds its own keys. This script exists for the two scripts
# you also type by name, since a plugin directory is not on $PATH.
set -uo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
COMMANDS=(tm tmux-branch tmux-open-path)

uninstall=0
[ "${1:-}" = --uninstall ] && uninstall=1

mkdir -p "$BIN"

for c in "${COMMANDS[@]}"; do
    link="$BIN/$c"
    target="$DIR/scripts/$c"

    if [ "$uninstall" -eq 1 ]; then
        # Only ever remove a link that points back into this plugin. Deleting
        # whatever happens to sit at that name would take someone else's tool
        # with it.
        if [ -L "$link" ] && [ "$(readlink -f "$link")" = "$target" ]; then
            rm -- "$link" && printf 'removed  %s\n' "$link"
        fi
        continue
    fi

    if [ -e "$link" ] && [ ! -L "$link" ]; then
        printf 'SKIP     %s already exists and is not a symlink\n' "$link" >&2
        continue
    fi
    ln -sfn "$target" "$link"
    printf 'linked   %-24s -> %s\n' "$link" "$target"
done

[ "$uninstall" -eq 1 ] && exit 0

case ":$PATH:" in
    *":$BIN:"*) ;;
    *) printf '\nNOTE: %s is not on your $PATH.\n' "$BIN" >&2 ;;
esac

conf="$HOME/.tmux.conf"
if [ -r "$conf" ] && ! grep -q 'cockpit\.tmux' "$conf"; then
    printf '\nAdd this to %s, last line but one:\n  run-shell %s/cockpit.tmux\n' \
           "$conf" "$DIR" >&2
fi
