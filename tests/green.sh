#!/usr/bin/env bash
# The green stamp: don't re-run a gate that already passed on exactly this
# content.
#
#   tests/green.sh check    exit 0 if nothing has changed since the last stamp
#   tests/green.sh stamp    record the current content as green
#   tests/green.sh changed  list files that changed since the stamp (else vs HEAD)
#
# The stamp is a MANIFEST -- one sha256 per file the tests run against, plus the
# versions of the three tools the assertions observe. Not a git tree: computing
# it is 3 ms against 31 ms for `rev-parse HEAD^{tree}` + `diff HEAD | sha256sum`
# + `ls-files -o`; it needs no git at all; it covers untracked and unstaged
# files with no extra step; and it hashes the bytes the tests actually read
# rather than a commit's idea of them. Per-file rather than one digest over the
# lot, because `changed` then falls out of a plain diff -- no second mechanism.
set -u
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIR" || exit 1

# Per-worktree, outside the tree: two checkouts of this repo must not share a
# stamp, and the stamp must not need a .gitignore entry. A linked worktree's
# --git-dir is its own .git/worktrees/<name>.
# ponytail: a plain `cp -r` of a worktree inherits its .git file and so its
# stamp. Harmless -- a wrong key only costs a re-run -- but that is the ceiling.
STAMP=$(git rev-parse --git-dir 2>/dev/null)/cockpit-green
[ -d "${STAMP%/*}" ] || STAMP="$DIR/tests/.cache/green"

manifest() {
    # shellcheck disable=SC2046  # deliberate glob expansion into one sha256sum
    sha256sum $(printf '%s\n' cockpit.tmux install.sh bootstrap.sh \
                    scripts/* scripts/lib/* tests/* examples/* \
                | grep -v '^tests/\.' | sort) 2>&1
    # A tmux, fzf or git upgrade changes what the suite observes without
    # changing a byte of this repo, so a green stamp must not survive one.
    printf 'tool %s\n' "$(tmux -V 2>&1)" "$(fzf --version 2>&1)" "$(git --version 2>&1)"
}

case "${1:-check}" in
check)
    [ -r "$STAMP" ] || exit 1
    manifest | diff -q - "$STAMP" >/dev/null || exit 1
    echo "already green at $(sha256sum < "$STAMP" | cut -c1-12) - nothing changed since the last full gate"
    ;;
stamp)
    mkdir -p "$(dirname "$STAMP")" && manifest > "$STAMP" \
        && echo "stamped green at $(sha256sum < "$STAMP" | cut -c1-12)"
    ;;
changed)
    # Files whose content differs from the stamp. With no stamp, fall back to
    # git: everything modified or untracked relative to HEAD.
    if [ -r "$STAMP" ]; then
        manifest | diff - "$STAMP" | awk '/^[<>] [0-9a-f]{64} /{print $3}' | sort -u
    else
        { git diff --name-only HEAD 2>/dev/null
          git ls-files -o --exclude-standard 2>/dev/null; } | sort -u
    fi
    ;;
*)  echo "usage: tests/green.sh [check|stamp|changed]" >&2; exit 2 ;;
esac
