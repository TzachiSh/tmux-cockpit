#!/usr/bin/env bash
# Run only the tests/run.sh blocks whose "== title ==" matches PATTERN (an
# ERE), in seconds instead of the suite's two minutes. Builds a script from
# the prelude (helpers, counters, the state dir), every top-level function
# defined anywhere in run.sh, the matching blocks, and the summary tail.
# Blocks that read variables another block set (AP/NP from the deliver
# block) need that block matched too -- widen the pattern.
# A "\(" in PATTERN is a literal paren.  No match: status 2; invalid ERE: status 4.
# ONLY_RUN=FILE reads blocks from FILE instead of tests/run.sh (the tests).
# Usage: tests/only.sh 'tmux-review --attach|--deliver'
set -u
[ $# -eq 1 ] || { echo "usage: tests/only.sh PATTERN" >&2; exit 2; }
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN="${ONLY_RUN:-$DIR/tests/run.sh}"
OUT=$(mktemp "$DIR/tests/.only.XXXXXX") || exit 1   # under tests/: the prelude derives DIR from BASH_SOURCE
# HUP, INT and TERM are trapped here, and the exit trap ignores all three first, as par.sh
# does (#1127, #1171). With no trap of its own, bash ends through its own handler, which kills
# the shell when a second such signal lands while the first is handled, and the .only file
# stays in tests/. The cost: a signal sent to this pid alone waits for the block run below.
only_clean() { trap '' HUP INT TERM; rm -f -- "$OUT"; }
only_sig() { trap '' HUP INT TERM; exit $((128 + $1)); }
trap only_clean EXIT
trap 'only_sig 1' HUP; trap 'only_sig 2' INT; trap 'only_sig 15' TERM
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
    /^flop_left$/ { next }                            # the tail below calls it once
    /^echo "== / { title = $0; sub(/^echo "== /, "", title); sub(/ =="$/, "", title)
                   want = (title ~ ENVIRON["PAT"]); nblk += want }
    want { print }
    END { if (!nblk) exit 3
          print "flop_left"
          print "printf '"'"'  %d passed, %d failed, %d skipped\\n'"'"' \"$pass\" \"$fail\" \"$skipped\""
          print "[ \"$fail\" -eq 0 ] || exit 1" }
' "$RUN" > "$OUT"
case $? in   # awk: 3 is our no-match; any other failure is awk dying on the ERE
    0) ;;
    3) echo "tests/only.sh: no block matches $1" >&2; exit 2 ;;
    *) echo "tests/only.sh: bad pattern $1" >&2; exit 4 ;;
esac
bash -n "$OUT" || exit 2
cd "$DIR" && bash "$OUT"
