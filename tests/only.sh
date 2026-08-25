#!/usr/bin/env bash
# Run only the tests/run.sh blocks whose "== title ==" matches PATTERN (an
# ERE), in seconds instead of the suite's two minutes. Builds a script from
# the prelude (helpers, counters, the state dir), every top-level function
# defined anywhere in run.sh, the matching blocks, and the summary tail.
# Blocks that read variables another block set (AP/NP from the deliver
# block, RVS from the loop block) need that block matched too -- widen the
# pattern.  Usage: tests/only.sh 'tmux-review --attach|--deliver'
set -u
[ $# -eq 1 ] || { echo "usage: tests/only.sh PATTERN" >&2; exit 2; }
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN="$DIR/tests/run.sh"
OUT=$(mktemp "$DIR/tests/.only.XXXXXX") || exit 1   # under tests/: the prelude derives DIR from BASH_SOURCE
trap 'rm -f -- "$OUT"' EXIT
awk -v pat="$1" '
    /^echo "== / && !seen_block { seen_block = 1 }
    !seen_block { print; next }                       # the prelude, verbatim
    /^[A-Za-z_][A-Za-z_0-9]*\(\) *\{/ { infn = 1; depth = 0 }   # a top-level function
    infn {                                            # runs until its braces balance
        print; o = $0; c = $0
        depth += gsub(/\{/, "", o) - gsub(/\}/, "", c)
        if (depth <= 0) infn = 0
        next
    }
    /^echo "== / { title = $0; sub(/^echo "== /, "", title); sub(/ =="$/, "", title)
                   want = (title ~ pat); nblk += want }
    want { print }
    END { if (!nblk) { print "echo \"tests/only.sh: no block matches\" >&2; exit 2" }
          print "printf '"'"'  %d passed, %d failed, %d skipped\\n'"'"' \"$pass\" \"$fail\" \"$skipped\""
          print "[ \"$fail\" -eq 0 ] || exit 1" }
' "$RUN" > "$OUT"
bash -n "$OUT" || exit 2
cd "$DIR" && bash "$OUT"
