#!/usr/bin/env bash
# tmux-cockpit bootstrap -- a working tmux on a machine that has nothing.
#
#   ./bootstrap.sh             ask before each step
#   ./bootstrap.sh --yes       take every default, ask nothing
#   ./bootstrap.sh --full      also fetch the pinned fzf, gum and bat into $BIN
#   ./bootstrap.sh --full --latest   the same, at each project's newest tag
#   ./bootstrap.sh --check     run install.sh --check (the doctor) and exit
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

TMUX_FLOOR_MAJ=3 TMUX_FLOOR_MIN=8 TMUX_BUILD=3.8

USAGE='usage: bootstrap.sh [--yes] [--dry-run] [--full [--latest]] [--check]'
assume_yes=0 dry=0 full=0 latest=0 check=0
for a in "$@"; do case "$a" in
    "") ;;
    --yes|-y)  assume_yes=1 ;;
    --full)    full=1 ;;
    --latest)  latest=1 ;;
    --check)   check=1 ;;
    --dry-run) dry=1 ;;
    --help|-h) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    # A typo must not install a different set of things than the user asked for.
    *) printf 'unknown option: %s\n\n%s\n' "$a" "$USAGE" >&2; exit 2 ;;
esac; done
# --latest is an opt-in on top of --full, never instead of it: alone it would
# resolve three tags for a fetch that never runs.
[ "$latest" -eq 1 ] && [ "$full" -eq 0 ] && {
    printf -- '--latest needs --full\n\n%s\n' "$USAGE" >&2; exit 2; }
# One implementation of the doctor, in install.sh; this is only the name people
# reach for first, and it reports rather than doing any of the rest.
[ "$check" -eq 1 ] && [ $((full + latest + dry + assume_yes)) -gt 0 ] && {
    printf -- '--check runs alone\n\n%s\n' "$USAGE" >&2; exit 2; }
[ "$check" -eq 1 ] && exec "$DIR/install.sh" --check

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
sha_c() { if have sha256sum; then sha256sum -c -; else shasum -a 256 -c -; fi; }

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
# tmux ships only a source tarball, so a distro below the floor builds it.
build_tmux() {
    local deps tmp
    case "$PKG" in
        apt)    deps="build-essential pkg-config bison libevent-dev libncurses-dev curl" ;;
        dnf)    deps="gcc make pkgconf-pkg-config bison libevent-devel ncurses-devel curl" ;;
        pacman) deps="base-devel libevent ncurses curl" ;;
        zypper) deps="gcc make pkg-config bison libevent-devel ncurses-devel curl" ;;
        *)      deps="" ;;
    esac
    [ -n "$deps" ] &&{ run "install the tmux build deps" bash -c "$PKG_INSTALL $deps" || return 1; }
    [ "$dry" -eq 1 ] && { printf '  would: build tmux %s into ~/.local\n' "$TMUX_BUILD"; return 0; }
    tmp=$(mktemp -d) || return 1
    trap 'rm -rf "$tmp"' RETURN
    printf '  note:  tmux publishes no checksums; TLS only\n'
    run "download tmux $TMUX_BUILD" curl -fsSL --retry 2 --max-time 120 -o "$tmp/t.tgz" \
        "${COCKPIT_GITHUB:-https://github.com}/tmux/tmux/releases/download/$TMUX_BUILD/tmux-$TMUX_BUILD.tar.gz" || return 1
    run "build tmux $TMUX_BUILD into ~/.local" bash -c "cd '$tmp' && tar xzf t.tgz && cd tmux-$TMUX_BUILD \
        && ./configure --prefix='$HOME/.local' && make -j\$(nproc 2>/dev/null || echo 2) && make install"
}

step "tmux version"
v=$(tmux -V 2>/dev/null)
note "found  ${v:-no tmux}"
if [ -n "$v" ] && ver_ge "$v" "$TMUX_FLOOR_MAJ" "$TMUX_FLOOR_MIN"; then :
elif [ "$PKG" = brew ]; then
    run "upgrade tmux" brew upgrade tmux
elif ask "tmux-cockpit needs tmux $TMUX_BUILD; build it into ~/.local/bin?" y && build_tmux; then
    case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) note "add ~/.local/bin to PATH, before /usr/bin" ;; esac
    hash -r
else
    failed+=("tmux $TMUX_BUILD")
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
# Both candidates, not the first readable one: tmux loads both, so the
# run-shell line in either wires the box (#96). install.sh --check agrees.
CONF=''; wired=''
for c in "${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf" "$HOME/.tmux.conf"; do
    [ -r "$c" ] || continue
    [ -n "$CONF" ] || CONF=$c
    grep -q 'cockpit\.tmux' "$c" 2>/dev/null && { wired=$c; break; }
done
[ -n "$CONF" ] || CONF="$HOME/.tmux.conf"

if [ -n "$wired" ]; then
    note "have   $wired already loads cockpit.tmux"
    skipped+=("config: already wired")
elif [ ! -e "$CONF" ]; then
    if ask "no tmux config; install the full example to $CONF?" y; then
        run "write $CONF" cp -- "$DIR/examples/full.tmux.conf" "$CONF"
    else
        skipped+=("config")
    fi
else
    # Never rewrite a config someone has been editing for years. Appending the
    # one run-shell line is the smallest change that makes cockpit work, and it
    # is a line they can delete. The full example stays available to copy from.
    note "$CONF exists and does not load cockpit"
    if ask "append the run-shell line to it? (a backup is kept)" y; then
        if [ "$dry" -eq 1 ]; then
            printf '  would: append run-shell to %s\n' "$CONF"
        else
            # Keep the FIRST backup: this step runs again whenever the
            # run-shell line was removed, and the second copy would already
            # carry cockpit's own edit. install.sh guards its .bak the same way.
            bak_ok=1; bak_note=" (backup: $CONF.bak)"
            if [ -e "$CONF.bak" ]; then
                bak_note=' (existing backup kept, not overwritten)'
            elif ! cp -- "$CONF" "$CONF.bak"; then
                bak_ok=0
            fi
            if [ "$bak_ok" -eq 1 ] &&
               printf '\n# tmux-cockpit\nrun-shell %s/cockpit.tmux\n' "$DIR" >> "$CONF"; then
                did+=("append run-shell to $CONF$bak_note")
                printf '  ok:    appended to %s%s\n' "$CONF" "$bak_note"
                note "the full example config is at $DIR/examples/full.tmux.conf"
            else
                failed+=("append to $CONF")
            fi
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

# ----------------------------------------------------------- latest tools ----
# Opt-in and pinned, the way fzf's own installer pins. Debian ships fzf 0.44.1,
# below the 0.63 floor the styled pickers need, and shadowing it in $BIN is the
# only clean way up: never over a distro binary, never with sudo.
FZF=0.74.3 GUM=2.0.0 BAT=0.26.1

# The releases redirect, not the API: no token and no 60-per-hour quota.
latest_tag() {                # latest_tag <owner/repo> <pin>
    local u v
    u=$(curl -fsSLI -o /dev/null -w '%{url_effective}' --max-time 30 \
        "$G/$1/releases/latest" 2>/dev/null)
    v=${u##*/tag/}; v=${v#v}
    case "$v" in
        ''|*/*) note "could not resolve the latest $1; keeping the pin $2" >&2
                printf '%s\n' "$2" ;;
        *) printf '%s\n' "$v" ;;
    esac
}

# Checksums first, then the asset: "could not verify" is not "verified", so an
# unreachable sums file fails closed while an unreachable asset is only a skip.
# Same order as scripts/tui-fetch.
fetch_tool() {                # <name> <ver> <have> <asset-url> <sums-url|-> <path-in-tar>
    local name=$1 ver=$2 have=$3 asset=$4 sums=$5 path=$6 tmp a depth
    if [ "$dry" -eq 1 ]; then printf '  would: fetch %s %s\n' "$name" "$ver"; return 0; fi
    if [ "$have" = "$ver" ]; then
        printf '  ok:    %s %s already current\n' "$name" "$ver"; return 0
    fi
    tmp=$(mktemp -d) || return 1
    trap 'rm -rf "$tmp"' RETURN
    a=${asset##*/}
    if [ "$sums" = - ]; then
        printf '  note:  %s publishes no checksums; TLS only\n' "$name"
    elif ! curl -fsSL --max-time 30 -o "$tmp/sums" "$sums"; then
        printf '  FAIL:  %s checksums unreachable\n' "$name" >&2; return 1
    fi
    if ! curl -fsSL --retry 2 --max-time 120 -o "$tmp/a.tgz" "$asset"; then
        printf '  skip:  %s (download failed -- offline?)\n' "$name" >&2; return 0
    fi
    # Rewrite the published name onto the temp file, so sha256sum -c checks the
    # bytes that arrived rather than a name that is not on disk.
    if [ "$sums" != - ] && ! ( cd "$tmp" && grep -E " \*?${a//./\\.}\$" sums |
            sed -E "s| \*?${a//./\\.}\$| a.tgz|" | sha_c ) >/dev/null 2>&1; then
        printf '  FAIL:  %s checksum mismatch -- NOT installed\n' "$name" >&2; return 1
    fi
    depth=$(( $(printf '%s' "$path" | tr -cd / | wc -c) ))
    if ! tar -xzf "$tmp/a.tgz" -C "$tmp" --strip-components="$depth" "$path" 2>/dev/null; then
        printf '  FAIL:  %s could not extract %s\n' "$name" "$path" >&2; return 1
    fi
    if ! ( mkdir -p "$BIN" && install -m 755 "$tmp/${path##*/}" "$BIN/$name" ); then
        printf '  FAIL:  %s could not install into %s\n' "$name" "$BIN" >&2; return 1
    fi
    printf '  ok:    %s -> %s %s\n' "$name" "$BIN/$name" "$ver"
}

if [ "$full" -eq 1 ]; then
    step "latest tools"
    BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
    G="${COCKPIT_GITHUB:-https://github.com}"
    fz='' gu='' ba=''
    case "$(uname -s)/$(uname -m)" in
        Linux/x86_64|Linux/amd64) fz=linux_amd64; gu=Linux_x86_64
                                  ba=x86_64-unknown-linux-musl ;;
        Linux/aarch64|Linux/arm64) fz=linux_arm64; gu=Linux_arm64
                                   ba=aarch64-unknown-linux-musl ;;
        Darwin/x86_64) fz=darwin_amd64; gu=Darwin_x86_64; ba=x86_64-apple-darwin ;;
        Darwin/arm64|Darwin/aarch64) fz=darwin_arm64; gu=Darwin_arm64
                                     ba=aarch64-apple-darwin ;;
    esac
    if [ -z "$fz" ]; then
        note "no prebuilt binaries for $(uname -s)/$(uname -m), use your package manager"
        skipped+=("latest tools: no prebuilt binaries")
    elif [ "$(uname -s)" = Darwin ] && have brew; then
        # brew already owns these on macOS; a fetched copy would shadow the one
        # it upgrades, and only one of the two would ever move again.
        if ask "install fzf, gum and bat with brew?" y; then
            run "brew install fzf bat gum" brew install fzf bat gum
        else
            skipped+=("latest tools")
        fi
    elif ! have curl; then
        note "curl is missing; nothing to fetch with"
        skipped+=("latest tools: no curl")
    elif ask "fetch fzf $FZF, gum $GUM and bat $BAT into $BIN?" y; then
        # Not in a dry run: a preview opens no connection, and the redirect is one.
        if [ "$latest" -eq 1 ] && [ "$dry" -eq 0 ]; then
            FZF=$(latest_tag junegunn/fzf "$FZF")
            GUM=$(latest_tag charmbracelet/gum "$GUM")
            BAT=$(latest_tag sharkdp/bat "$BAT")
        elif [ "$latest" -eq 1 ]; then
            note "would: resolve the latest tags first; the pins are shown"
        fi
        # The probe is "$BIN/<tool>", never PATH: with a distro fzf ahead of
        # $BIN, a PATH probe re-downloads everything on every run.
        fetch_tool fzf "$FZF" "$("$BIN/fzf" --version 2>/dev/null | awk '{print $1}')" \
            "$G/junegunn/fzf/releases/download/v$FZF/fzf-$FZF-$fz.tar.gz" \
            "$G/junegunn/fzf/releases/download/v$FZF/fzf_${FZF}_checksums.txt" \
            fzf || failed+=("fetch fzf")
        fetch_tool gum "$GUM" "$("$BIN/gum" --version 2>/dev/null | awk '{print $3}' | tr -d v)" \
            "$G/charmbracelet/gum/releases/download/v$GUM/gum_${GUM}_$gu.tar.gz" \
            "$G/charmbracelet/gum/releases/download/v$GUM/checksums.txt" \
            "gum_${GUM}_$gu/gum" || failed+=("fetch gum")
        fetch_tool bat "$BAT" "$("$BIN/bat" --version 2>/dev/null | awk '{print $2}')" \
            "$G/sharkdp/bat/releases/download/v$BAT/bat-v$BAT-$ba.tar.gz" - \
            "bat-v$BAT-$ba/bat" || failed+=("fetch bat")
    else
        skipped+=("latest tools")
    fi
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

# --full ends by showing the state it left behind. Advisory, like brew doctor:
# its exit grades the box, not this run, so it never becomes ours.
if [ "$full" -eq 1 ] && [ "$dry" -eq 0 ]; then
    printf '\n'
    "$DIR/install.sh" --check || :
fi

if [ "${#failed[@]}" -gt 0 ] && [ -n "${failed[0]:-}" ]; then
    printf '\nSome steps failed. Fix them and re-run: this script is idempotent.\n' >&2
    exit 1
fi

printf '\nNext: start tmux, then press prefix + I to let tpm fetch anything new.\n'
printf 'Icons are drawn by YOUR terminal font -- if you see boxes, install a\n'
printf 'Nerd Font and select it in the terminal, not on this machine.\n'
