#!/usr/bin/env bash
# The green stamp: don't re-run a gate that already passed on exactly this
# content.
#
#   tests/green.sh check    exit 0 if nothing has changed since the last stamp
#   tests/green.sh stamp    record the current content as green
#   tests/green.sh changed  list files that changed since the stamp (else vs HEAD)
#   tests/green.sh files    list the files the stamp covers
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
gd=$(git rev-parse --git-dir 2>/dev/null)
STAMP="$gd/cockpit-green"
[ -n "$gd" ] && [ -d "$gd" ] || STAMP="$DIR/tests/.cache/green"

# The files the tests run against, one per line: the manifest's key set.
tracked() {
    # tests/fixtures is in the list because the tests READ it, review/ too:
    # mkfix.sh builds the repos the tree_diff and diffstat blocks run on.
    # Without it, editing a fixture left the stamp valid and `make gate`
    # answered "already green ... nothing changed since the last full gate"
    # on a suite that would now fail.
    #
    # Regular files only. tests/* globs the fixture DIRECTORIES too, and
    # sha256sum's "Is a directory" went into the manifest through the 2>&1 --
    # noise, and it also meant an unmatched glob would have entered as a
    # literal. The `case` keeps the old grep's job: a stamp must not depend on
    # tests/.par.* or tests/.cache, which only exist mid-run.
    local f
    # tui/ too, since the suite runs the binary built from it (#199). And every
    # other file a block reads by path: docs/tui.md, the Makefile, all of
    # tests/fixtures. The #672 block in run.sh fails on one missing here.
    { for f in cockpit.tmux install.sh bootstrap.sh Makefile docs/tui.md \
               scripts/* scripts/lib/* tests/* examples/*; do
          case $f in tests/.*) continue ;; esac
          [ -f "$f" ] && printf '%s\n' "$f"
      done
      find tests/fixtures tui -path tui/target -prune -o -type f -print; } | sort
}

manifest() {
    # shellcheck disable=SC2046  # deliberate word splitting: one path per line
    sha256sum $(tracked) 2>&1
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
        # No {64}: mawk has no interval expressions.
        manifest | diff - "$STAMP" | awk '/^[<>] [0-9a-f]+ /{print $3}' | sort -u
    else
        { git diff --name-only HEAD 2>/dev/null
          git ls-files -o --exclude-standard 2>/dev/null; } | sort -u
    fi
    ;;
files)
    tracked
    ;;
*)  echo "usage: tests/green.sh [check|stamp|changed|files]" >&2; exit 2 ;;
esac
