#!/bin/sh
# How many tui test binaries refuse where cargo's runner does not run (#1066):
# with COCKPIT_TEST_CLAUDE_GUARD=0, or on a target with no runner key (#1090).
#
# Builds the test binaries, the artifacts cargo built with the test profile,
# then starts each one directly, as cargo does where no runner applies, with
# no TMUX_TMPDIR and none of a tmux session's variables. They run in a user and
# mount namespace of their own with a tmpfs over /tmp, so no socket and no read
# reaches your tmux server. Prints each binary that refuses with
# "TMUX_TMPDIR not set", then the counts.
#
# Linux only: it needs `unshare -rm`. Where that fails (no unshare, or no user
# namespaces: macOS, a locked-down container) it says so in one line and exits
# 1 before it builds or starts anything. It never runs a binary outside the
# namespace, since there a plain tmux would reach /tmp/tmux-<uid>.
# Usage: tests/tui-refusers.sh
set -u
if [ "${1-}" != --inside ]; then
    if ! unshare -rm true 2>/dev/null; then
        echo "tui-refusers.sh: no user and mount namespace here (unshare -rm), so no test binary was started" >&2
        exit 1
    fi
    here=$(cd "$(dirname "$0")" && pwd) || exit 1
    list=$(mktemp) || exit 1
    (cd "$here/../tui" && cargo test --locked --no-run --message-format=json 2>/dev/null) |
        grep '"profile":{[^}]*"test":true' | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | sort -u >"$list"
    ns=$(readlink /proc/self/ns/user)/$(readlink /proc/self/ns/mnt)
    unshare -rm "$here/tui-refusers.sh" --inside "$list" "$ns"
    rc=$?
    rm -f "$list"
    exit "$rc"
fi

# --inside is for the line above alone, which hands it the user and mount
# namespaces it ran in. Called any other way, from a shell or from inside a
# container's own namespace (#1089), it has no others to show, and no binary
# may start there.
ns=$(readlink /proc/self/ns/user)/$(readlink /proc/self/ns/mnt)
case $ns in /* | */) ns=${3-} ;; esac
if [ -z "${3-}" ] || [ "$ns" = "$3" ]; then
    echo "tui-refusers.sh: --inside not in a namespace of its own, so no test binary was started" >&2
    exit 1
fi
# the list sits in TMPDIR, which may be /tmp: opened before the tmpfs hides it
exec 3<"$2"
mount -t tmpfs none /tmp || exit 1
n=0 r=0
while IFS= read -r b <&3; do
    n=$((n + 1))
    if env -u TMUX -u TMUX_PANE -u TMUX_COCKPIT_DIR -u TMUX_TMPDIR -u COCKPIT_LOG \
        -u XDG_CACHE_HOME -u XDG_STATE_HOME TMPDIR=/tmp "$b" </dev/null 2>&1 |
        grep -q 'TMUX_TMPDIR not set'; then
        r=$((r + 1))
        echo "refuses ${b##*/}"
    fi
done
echo "binaries=$n refusing=$r"
