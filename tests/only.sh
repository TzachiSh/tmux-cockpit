#!/usr/bin/env bash
# Run only the tests/run.sh blocks whose "== title ==" matches PATTERN (an
# ERE), in seconds instead of the suite's two minutes. Builds a script from
# the prelude (helpers, counters, the state dir), every top-level function
# defined anywhere in run.sh, the matching blocks, and the summary tail.
# Blocks that read variables another block set (AP/NP from the deliver
# block, RVS from the loop block) need that block matched too -- widen the
# pattern.  A "\(" in PATTERN is a literal paren.  No match: status 2.
# ONLY_RUN=FILE reads blocks from FILE instead of tests/run.sh (the tests).
# Usage: tests/only.sh 'tmux-review --attach|--deliver'
set -u
[ $# -eq 1 ] || { echo "usage: tests/only.sh PATTERN" >&2; exit 2; }
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN="${ONLY_RUN:-$DIR/tests/run.sh}"
OUT=$(mktemp "$DIR/tests/.only.XXXXXX") || exit 1   # under tests/: the prelude derives DIR from BASH_SOURCE
trap 'rm -f -- "$OUT"' EXIT
# The pattern goes through the environment: awk -v would unescape \( to (.
PAT="$1" awk '
    /^echo "== / && !seen_block { seen_block = 1 }
    !seen_block { print; next }                       # the prelude, verbatim
    # ponytail: brace depth counts every { and } in the line, strings included;
    # a helper whose body prints a lone brace would end early -- none does today.
    # Upgrade: skip quoted regions, or mark helper ends with a "# }" sentinel.
    /^[A-Za-z_][A-Za-z_0-9]*\(\) *\{/ { infn = 1; depth = 0 }   # a top-level function
    infn {                                            # runs until its braces balance
        print; o = $0; c = $0
        depth += gsub(/\{/, "", o) - gsub(/\}/, "", c)
        if (depth <= 0) infn = 0
        next
    }
    /^echo "== / { title = $0; sub(/^echo "== /, "", title); sub(/ =="$/, "", title)
                   want = (title ~ ENVIRON["PAT"]); nblk += want }
    want { print }
    END { if (!nblk) exit 3
          print "printf '"'"'  %d passed, %d failed, %d skipped\\n'"'"' \"$pass\" \"$fail\" \"$skipped\""
          print "[ \"$fail\" -eq 0 ] || exit 1" }
' "$RUN" > "$OUT" || { echo "tests/only.sh: no block matches $1" >&2; exit 2; }
bash -n "$OUT" || exit 2
cd "$DIR" && bash "$OUT"
