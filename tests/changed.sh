#!/usr/bin/env bash
# Print the ordinals of the blocks a change could have broken, for PAR_BLOCKS.
# A DEVELOPER LOOP, not a gate -- the full parallel suite still runs at merge,
# and `make gate` runs it whenever the green stamp does not match.
#
# Which blocks exercise which script is DERIVED, not declared: each block's body
# is grepped for the scripts it runs, and top-level helpers are resolved first
# (a block that calls st() depends on whatever st()'s body names -- 9 of the 115
# blocks reach agent-state only that way). Measured: deriving needs a marker on
# 5 blocks; mapping by block title would need one on 59.
#
# Prints "ALL" when it cannot be sure. Every unknown path takes that branch.
set -u
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN="$DIR/tests/run.sh"
cd "$DIR" || exit 1
TBL=$(mktemp) || exit 1
trap 'rm -f -- "$TBL"' EXIT

SCRIPTS='agent-context agent-state tm tmux-branch tmux-jump tmux-open-path tmux-review tmux-where'

# ordinal <TAB> title <TAB> files-it-uses
awk -v scripts="$SCRIPTS" '
function note(arr, line,   i, n, S) {
    n = split(scripts, S, " ")
    # The trailing class matters: scripts/tm must not match scripts/tmux-where.
    for (i = 1; i <= n; i++) if (line ~ ("scripts/" S[i] "([^A-Za-z0-9_-]|$)")) arr["scripts/" S[i]] = 1
    if (line ~ /cockpit\.tmux/)  arr["cockpit.tmux"] = 1
    if (line ~ /install\.sh/)    arr["install.sh"] = 1
    if (line ~ /review-store/)   arr["scripts/lib/review-store.sh"] = 1
    if (line ~ /examples\//)     arr["examples"] = 1
}
FNR == NR {                                       # pass 1: the top-level helpers
    if ($0 ~ /^[A-Za-z_][A-Za-z_0-9]*\(\) *\{/) { fn = $0; sub(/\(\).*/, "", fn); infn = 1; d = 0 }
    if (infn) {
        o = $0; c = $0; d += gsub(/\{/, "", o) - gsub(/\}/, "", c)
        delete h; note(h, $0); for (k in h) H[fn] = H[fn] " " k
        if (d <= 0) infn = 0
    }
    next
}

# A marker credits the block it is WRITTEN FOR, and all five in run.sh sit on
# the line above their block title -- where it reads naturally, and where the
# old rule (add to the current block, then `delete f` on the title) fed every
# one of them to the PREVIOUS block and then threw it away: blocks 4, 7, 13, 23
# and 107 came out with no dependencies at all, so `make test-changed` never
# selected them for the very scripts the marker names. So the files are held in
# pend[] and merged by whichever line comes next -- the title (the marker
# announces the block below it) or any other line (the marker is inside a block
# after all, the shape the file has always supported).
/^# tests: / { for (i = 3; i <= NF; i++) pend[$i] = 1; next }
/^echo "== / { if (t != "") emit(); n++; t = $0; sub(/^echo "== /, "", t); sub(/ =="$/, "", t)
               delete f; for (k in pend) f[k] = 1; delete pend; next }
t == "" { next }
{ for (k in pend) f[k] = 1; delete pend
  note(f, $0)
  for (hn in H) if ($0 ~ ("(^|[^A-Za-z_0-9])" hn "([^A-Za-z_0-9]|$)")) {
      m = split(H[hn], P, " "); for (i = 1; i <= m; i++) if (P[i] != "") f[P[i]] = 1 } }
function emit(   k, s) { s = ""; for (k in f) s = s " " k; printf "%d\t%s\t%s\n", n, t, substr(s, 2) }
END { emit() }
' "$RUN" "$RUN" > "$TBL"

blocks_for() { awk -F'\t' -v f="$1" '{ n = split($3, D, " "); for (i = 1; i <= n; i++) if (D[i] == f) { print $1; break } }' "$TBL"; }

# tests/run.sh: a hunk above the first block is the prelude or a helper and can
# affect anything. A hunk inside a block runs that block.
runsh_blocks() {
    local first ln out
    first=$(grep -n '^echo "== ' "$RUN" | head -1 | cut -d: -f1)
    out=$(git diff -U0 -- tests/run.sh 2>/dev/null |
        awk '/^@@/ { split($3, a, ","); sub(/^\+/, "", a[1]); print a[1] }' |
        while read -r ln; do
            if [ "${ln:-0}" -lt "$first" ]; then echo ALL; return; fi
            awk -v l="$ln" '/^echo "== /{ n++ } NR == l { print n; exit }' "$RUN"
        done)
    # `git diff` is the WORKING TREE against the index, but green.sh compares
    # against the stamp -- so a run.sh edit that has since been staged or
    # committed is still "changed" here and yields no hunk at all. No hunks is
    # not "no blocks": it is "this diff cannot tell", which is the ALL branch.
    [ -n "$out" ] || out=ALL
    printf '%s\n' "$out"
}

want=1                                            # the syntax block, always
for f in $(bash "$DIR/tests/green.sh" changed); do
    case "$f" in
    tests/run.sh)
        b=$(runsh_blocks)
        case "$b" in *ALL*) echo ALL; exit 0 ;; esac
        want="$want $b" ;;
    tests/only.sh|tests/par.sh|tests/green.sh|tests/changed.sh|Makefile|README.md|LICENSE|.*|docs/*)
        ;;                                        # nothing an assertion reads
    scripts/lib/*)                                # reaches every script sourcing it
        for s in scripts/*; do
            [ -f "$s" ] && grep -q "${f##*/}" "$s" && want="$want $(blocks_for "$s")"
        done
        want="$want $(blocks_for "$f")" ;;
    examples/*)  want="$want $(blocks_for examples)" ;;
    cockpit.tmux|install.sh|scripts/*)
        want="$want $(blocks_for "$f")" ;;
    *)  echo ALL; exit 0 ;;                       # anything unrecognised
    esac
done
# shellcheck disable=SC2086  # deliberate word splitting: a list of ordinals
printf '%s\n' $want | sort -un | paste -sd,
