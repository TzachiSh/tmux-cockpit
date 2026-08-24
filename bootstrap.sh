#!/usr/bin/env bash
# tmux-cockpit bootstrap -- a working tmux on a machine that has nothing.
#
#   ./bootstrap.sh             ask before each step
#   ./bootstrap.sh --yes       take every default, ask nothing
#   ./bootstrap.sh --dry-run   print what would happen, change nothing
#   ./bootstrap.sh --help
#
# install.sh is the small one: link the commands, merge the hooks, for a machine
# that already runs tmux. This is the large one -- packages, plugins, config,
# commands, hooks -- and it CALLS install.sh rather than copying it, so there is
# one implementation of the linking rules and one of the hook merge.
#
# Every step is skippable and every step is idempotent: running this twice must
# be as safe as running it once, because the second run is how you upgrade.
set -uo pipefail

DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGINS="${XDG_CONFIG_HOME:-$HOME}/.tmux/plugins"
[ -d "$HOME/.tmux/plugins" ] && PLUGINS="$HOME/.tmux/plugins"

# tmux 3.3 is the real floor (display-popup -T). 3.7 adds MouseDown1Control1 and
# #[range=control|N]; below it the Ctrl+click features are gated off and the
# rest works. Both numbers are measured, not guessed -- see cockpit.tmux.
TMUX_FLOOR_MAJ=3 TMUX_FLOOR_MIN=3
TMUX_CLICK_MAJ=3 TMUX_CLICK_MIN=7

assume_yes=0 dry=0
for a in "$@"; do case "$a" in
    "") ;;
    --yes|-y)  assume_yes=1 ;;
    --dry-run) dry=1 ;;
    --help|-h) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    # A typo must not install a different set of things than the user asked for.
    *) printf 'unknown option: %s\n\nusage: bootstrap.sh [--yes] [--dry-run]\n' "$a" >&2; exit 2 ;;
esac; done

did=() skipped=() failed=()
note()  { printf '  %s\n' "$*"; }
step()  { printf '\n== %s ==\n' "$*"; }
run() {  # run <description> <command...>
    local what=$1; shift
    if [ "$dry" -eq 1 ]; then printf '  would: %s\n' "$what"; return 0; fi
    if "$@" >/dev/null 2>&1; then did+=("$what"); printf '  ok:    %s\n' "$what"; return 0
    else failed+=("$what"); printf '  FAIL:  %s\n' "$what" >&2; return 1; fi
}

# Default is the answer taken by --yes and by a non-interactive shell (a pipe,
# a CI job). Asking a question nobody can answer and then hanging is the worst
# outcome, so absence of a tty means "take the default", never "wait".
ask() {  # ask <question> <default y|n>
    local q=$1 def=${2:-y} reply
    if [ "$assume_yes" -eq 1 ] || [ ! -t 0 ]; then [ "$def" = y ]; return; fi
    read -r -p "$q [$([ "$def" = y ] && echo 'Y/n' || echo 'y/N')] " reply || return 1
    case "${reply:-$def}" in [Yy]*) return 0 ;; *) return 1 ;; esac
}

have() { command -v "$1" >/dev/null 2>&1; }

ver_ge() {  # ver_ge <have> <maj> <min>
    local v=${1#tmux } maj min
    v=${v%%[!0-9.]*}; maj=${v%%.*}; min=${v#*.}; min=${min%%.*}
    case "$maj" in ''|*[!0-9]*) return 0 ;; esac      # unparseable: assume modern
    case "$min" in ''|*[!0-9]*) min=0 ;; esac
    [ "$maj" -gt "$2" ] || { [ "$maj" -eq "$2" ] && [ "$min" -ge "$3" ]; }
}

# ---------------------------------------------------------------- packages ---
# One installer per distro, resolved once. An unknown package manager is not an
# error: everything after this step still works, the user just installs the
# handful of tools by hand, so say which ones and carry on.
PKG="" PKG_INSTALL=""
if   have apt-get; then PKG=apt;    PKG_INSTALL="sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq"
elif have dnf;     then PKG=dnf;    PKG_INSTALL="sudo dnf install -y -q"
elif have pacman;  then PKG=pacman; PKG_INSTALL="sudo pacman -S --noconfirm --needed"
elif have zypper;  then PKG=zypper; PKG_INSTALL="sudo zypper -n install"
elif have brew;    then PKG=brew;   PKG_INSTALL="brew install"
fi

# name:command:why -- command is what we test for, because the package name and
# the binary disagree often enough (bat ships as batcat on Debian) that testing
# the package name would reinstall something already present on every run.
PACKAGES=(
    "git:git:cloning the plugins"
    "tmux:tmux:the whole point"
    "fzf:fzf:the tm session picker (falls back to a numbered menu)"
    "bat:batcat:syntax-coloured file previews (falls back to plain text)"
    "jq:jq:merging the Claude Code hooks"
)

step "packages"
missing=()
for p in "${PACKAGES[@]}"; do
    IFS=: read -r name cmd why <<<"$p"
    if have "$cmd"; then note "have   $name"; else missing+=("$name"); note "MISSING $name -- $why"; fi
done
if [ ${#missing[@]} -eq 0 ]; then
    skipped+=("packages: all present")
elif [ -z "$PKG" ]; then
    note "no known package manager; install by hand: ${missing[*]}"
    skipped+=("packages: no package manager")
elif ask "install ${missing[*]} with $PKG?" y; then
    # Deliberately one call, not a loop: apt resolves the set together, and a
    # per-package loop turns one password prompt into five.
    run "install ${missing[*]}" bash -c "$PKG_INSTALL ${missing[*]}"
else
    skipped+=("packages")
fi

# ------------------------------------------------------------------- tmux ----
step "tmux version"
if ! have tmux; then
    note "tmux is still not installed; the rest of this will not do much"
    failed+=("tmux missing")
else
    v=$(tmux -V 2>/dev/null)
    note "found  $v"
    if ! ver_ge "$v" "$TMUX_FLOOR_MAJ" "$TMUX_FLOOR_MIN"; then
        note "below the ${TMUX_FLOOR_MAJ}.${TMUX_FLOOR_MIN} floor -- display-popup -T is missing, so the popups will not work"
        failed+=("tmux older than ${TMUX_FLOOR_MAJ}.${TMUX_FLOOR_MIN}")
    elif ! ver_ge "$v" "$TMUX_CLICK_MAJ" "$TMUX_CLICK_MIN"; then
        # Not a failure. cockpit.tmux gates these; everything else works.
        note "below ${TMUX_CLICK_MAJ}.${TMUX_CLICK_MIN}: Ctrl+click on the branch and the click-to-open picker stay off"
        skipped+=("ctrl+click features (tmux < ${TMUX_CLICK_MAJ}.${TMUX_CLICK_MIN})")
    fi
fi

# ---------------------------------------------------------------- plugins ----
# dir:repo -- catppuccin lands in "tmux" because the example config sources
# ~/.tmux/plugins/tmux/catppuccin_*.conf by that path.
TPM_PLUGINS=(
    "tpm:https://github.com/tmux-plugins/tpm"
    "tmux-sensible:https://github.com/tmux-plugins/tmux-sensible"
    "tmux-yank:https://github.com/tmux-plugins/tmux-yank"
    "tmux-resurrect:https://github.com/tmux-plugins/tmux-resurrect"
    "tmux-continuum:https://github.com/tmux-plugins/tmux-continuum"
    "tmux:https://github.com/catppuccin/tmux"
)

step "tmux plugins"
if ! have git; then
    note "git is missing; cannot clone anything"
    skipped+=("plugins: no git")
elif ask "clone the ${#TPM_PLUGINS[@]} plugins into $PLUGINS?" y; then
    [ "$dry" -eq 1 ] || mkdir -p "$PLUGINS"
    for p in "${TPM_PLUGINS[@]}"; do
        d=${p%%:*}; url=${p#*:}
        # An existing checkout is left exactly as it is. Pulling would discard
        # a pinned revision the user chose, and this script has no business
        # deciding that a plugin should move.
        if [ -d "$PLUGINS/$d/.git" ]; then note "have   $d"; continue; fi
        run "clone $d" git clone -q --depth 1 "$url" "$PLUGINS/$d"
    done
else
    skipped+=("plugins")
fi

# ----------------------------------------------------------------- config ----
step "tmux config"
CONF="$HOME/.tmux.conf"
[ -r "${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf" ] && CONF="${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf"

if [ ! -e "$CONF" ]; then
    if ask "no tmux config; install the full example to $CONF?" y; then
        run "write $CONF" cp -- "$DIR/examples/full.tmux.conf" "$CONF"
    else
        skipped+=("config")
    fi
elif grep -q 'cockpit\.tmux' "$CONF" 2>/dev/null; then
    note "have   $CONF already loads cockpit.tmux"
    skipped+=("config: already wired")
else
    # Never rewrite a config someone has been editing for years. Appending the
    # one run-shell line is the smallest change that makes cockpit work, and it
    # is a line they can delete. The full example stays available to copy from.
    note "$CONF exists and does not load cockpit"
    if ask "append the run-shell line to it? (a backup is kept)" y; then
        if [ "$dry" -eq 1 ]; then
            printf '  would: append run-shell to %s\n' "$CONF"
        elif cp -- "$CONF" "$CONF.bak" &&
             printf '\n# tmux-cockpit\nrun-shell %s/cockpit.tmux\n' "$DIR" >> "$CONF"; then
            did+=("append run-shell to $CONF (backup $CONF.bak)")
            printf '  ok:    appended to %s (backup: %s.bak)\n' "$CONF" "$CONF"
            note "the full example config is at $DIR/examples/full.tmux.conf"
        else
            failed+=("append to $CONF")
        fi
    else
        skipped+=("config")
    fi
fi

# --------------------------------------------------------------- commands ----
step "commands"
if ask "link tm, tmux-branch and tmux-open-path into ~/.local/bin?" y; then
    if [ "$dry" -eq 1 ]; then printf '  would: run install.sh\n'
    else "$DIR/install.sh" && did+=("linked commands") || failed+=("install.sh"); fi
else
    skipped+=("commands")
fi

# ------------------------------------------------------------------ hooks ----
step "Claude Code hooks"
if [ ! -d "$HOME/.claude" ] && ! have claude; then
    note "no ~/.claude and no claude on PATH -- nothing to wire"
    skipped+=("hooks: no Claude Code")
elif ! have jq; then
    note "jq is required for the merge; the manual block is in the README"
    skipped+=("hooks: no jq")
elif ask "merge the agent-state hooks into ~/.claude/settings.json?" y; then
    # NOT install.sh --hooks --dry-run: that flag only previews the hook merge,
    # and the linking loop above it still runs and creates symlinks. A --dry-run
    # that writes to disk is worse than no --dry-run at all.
    if [ "$dry" -eq 1 ]; then printf '  would: merge hooks into ~/.claude/settings.json\n'
    else "$DIR/install.sh" --hooks && did+=("merged hooks") || failed+=("hook merge"); fi
else
    skipped+=("hooks")
fi

# ---------------------------------------------------------------- summary ----
step "summary"
[ "$dry" -eq 1 ] && note "dry run: nothing above was actually done"
for x in "${did[@]:-}";     do [ -n "$x" ] && printf '  done     %s\n' "$x"; done
for x in "${skipped[@]:-}"; do [ -n "$x" ] && printf '  skipped  %s\n' "$x"; done
for x in "${failed[@]:-}";  do [ -n "$x" ] && printf '  FAILED   %s\n' "$x" >&2; done

if [ "${#failed[@]}" -gt 0 ] && [ -n "${failed[0]:-}" ]; then
    printf '\nSome steps failed. Fix them and re-run: this script is idempotent.\n' >&2
    exit 1
fi

printf '\nNext: start tmux, then press prefix + I to let tpm fetch anything new.\n'
printf 'Icons are drawn by YOUR terminal font -- if you see boxes, install a\n'
printf 'Nerd Font and select it in the terminal, not on this machine.\n'
