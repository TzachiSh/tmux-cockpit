#!/usr/bin/env bash
# Builds one of eight deterministic git fixtures whose `tmux-review --lines`
# output is frozen in the sibling *.tsv goldens. Each case's body is the
# exact fixture-building subshell from the corresponding tests/run.sh
# `--lines` block (deterministic author env, `git init -q -b main`), so the
# fixture and the golden it produced stay reproducible byte for byte.
#
# Usage: mkfix.sh <basic|mixed|untracked|blank|nonascii|funcname|hunks|spaces> <dir>
# <dir> must already exist and be empty; it becomes the fixture's repo root.

set -euo pipefail

name=$1
dir=$2

export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e

cd "$dir"

case "$name" in
basic)
    git init -q -b main .
    mkdir -p tests
    printf 'a\nb\nc\n' > src.go
    printf 'x\ny\nz\n' > tests/foo_test.sh
    printf 'gone1\ngone2\n' > deleteme.txt
    git add -A
    git commit -q -m init
    printf 'a\nCHANGED\nc\n' > src.go
    printf 'x\nCHANGED\nz\n' > tests/foo_test.sh
    rm deleteme.txt
    ;;
mixed)
    git init -q -b main .
    printf 'a\n' > staged.txt
    printf 'a\n' > unstaged.txt
    git add -A
    git commit -q -m init
    printf 'a\nSTAGED-CHANGE\n' > staged.txt
    git add staged.txt
    printf 'a\nUNSTAGED-CHANGE\n' > unstaged.txt
    ;;
untracked)
    git init -q -b main .
    printf 'seed\n' > seed.txt
    git add -A
    git commit -q -m init
    printf 'first\nsecond\n' > new.txt
    printf 'NEW-LINE\n' > 'café-new.txt'
    ;;
blank)
    git init -q -b main .
    printf 'a\nb\n' > f.md
    git add -A
    git commit -q -m init
    printf 'a\n\nNEW\nb\n' > f.md
    ;;
nonascii)
    git init -q -b main .
    printf 'a\nb\n' > alpha.txt
    printf 'x\ny\n' > café.txt
    git add -A
    git commit -q -m init
    printf 'a\nAA\n' > alpha.txt
    # This fixture also exercises a blank added line, not just a non-ASCII
    # filename, so café.txt's own line 2 goes blank and CC lands as a new
    # line 3 -- one row proving both seams at once.
    printf 'x\n\nCC\n' > café.txt
    ;;
funcname)
    git init -q -b main .
    printf 'fn compute() -> i32 {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n    a + b\n}\n' > f.rs
    git add -A
    git commit -q -m init
    sed -i '5d' f.rs
    ;;
hunks)
    git init -q -b main .
    mkdir -p tests
    : > nums.txt
    for i in $(seq 1 40); do printf 'line%d\n' "$i" >> nums.txt; done
    printf 'seed\n' > tests/t.txt
    git add -A
    git commit -q -m init
    # Three edits spaced far enough apart (3, 20, 35) that diff's default
    # 3-line context keeps them as separate hunks -- the case no other
    # golden covers: the 5th-field preview offset must keep counting
    # correctly across a hunk boundary, not reset per hunk.
    #
    # Line 35's replacement is a 220-byte fragment with a literal \x01 (SOH)
    # at its centre -- the parser's control-char-strip-then-160-byte-cap
    # case (scripts/tmux-review:348). Do this edit BEFORE deleting line 20,
    # so "line 35" still names the pre-deletion row; the line-3 change is
    # addressing-safe either way since it's above both.
    long220=$(printf 'A%.0s' $(seq 1 109))$(printf '\001')$(printf 'B%.0s' $(seq 1 110))
    awk -v ln35="$long220" 'NR==35{print ln35; next} NR==20{next} {print}' nums.txt > nums.txt.new
    mv nums.txt.new nums.txt
    sed -i '3s/.*/line3-CHANGED/' nums.txt
    printf 'CHANGED\n' > tests/t.txt
    ;;
spaces)
    # git terminates a header path with a TAB when the path holds a space
    # ("--- a/has space.txt<TAB>"), which the parser used to carry into every
    # row of that file. Both sides are covered: a TRACKED path with a space,
    # which goes through the diff headers (one changed line and one deleted
    # line, so the deletion rows -- the ones that read as additions when the
    # tab rode along -- are pinned), and an UNTRACKED one, which has no diff
    # header at all and is walked straight off the filesystem.
    git init -q -b main .
    printf 'alpha\nbravo\ncharlie\n' > 'has space.txt'
    git add -A
    git commit -q -m init
    printf 'alpha\nBRAVO-CHANGED\n' > 'has space.txt'
    printf 'brand new\n' > 'new file.txt'
    ;;
*)
    echo "mkfix.sh: unknown fixture '$name'" >&2
    exit 1
    ;;
esac
