#!/usr/bin/env bash
# Builds one of six deterministic git fixtures whose `tmux-review --lines`
# output is frozen in the sibling *.tsv goldens. Each case's body is the
# exact fixture-building subshell from the corresponding tests/run.sh
# `--lines` block (deterministic author env, `git init -q -b main`), so the
# fixture and the golden it produced stay reproducible byte for byte.
#
# Usage: mkfix.sh <basic|mixed|untracked|blank|nonascii|funcname> <dir>
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
    # Controller ruling: this fixture also exercises a blank added line, not
    # just a non-ASCII filename, so café.txt's own line 2 goes blank and CC
    # lands as a new line 3 -- one row proving both seams at once.
    printf 'x\n\nCC\n' > café.txt
    ;;
funcname)
    git init -q -b main .
    printf 'fn compute() -> i32 {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n    a + b\n}\n' > f.rs
    git add -A
    git commit -q -m init
    sed -i '5d' f.rs
    ;;
*)
    echo "mkfix.sh: unknown fixture '$name'" >&2
    exit 1
    ;;
esac
