#!/usr/bin/env bash
# Run tests/run.sh's blocks across J parallel shards.  make test-par [J=12]
#
#   tests/par.sh            every block, J shards at a time
#   tests/par.sh --each     the audit: every GROUP alone, one run apiece
#
# Reuses tests/only.sh's extraction -- prelude, every top-level function, the
# chosen blocks -- but selects blocks by ORDINAL instead of by title pattern,
# so a title carrying regex metacharacters cannot pick up the wrong block.
#
# Isolation comes free: the prelude already derives every socket, state dir
# and temp registry from $$ (SUITE_SOCK=cockpit-suite-$$, cockpit-rv-$$,
# STATE=$(mktemp -d), TMPS=$(mktemp)), so N processes each running the
# prelude are already N private tmux servers and N private state trees.
#
# Scheduling is xargs -P: one script per group, a worker pulls the next when
# it finishes.  No cost table to keep in sync and no static split to unbalance.
set -u

EACH=""
case "${1:-}" in
    "")     ;;
    --each) EACH=1 ;;
    *)      echo "usage: tests/par.sh [--each]" >&2; exit 2 ;;
esac
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
J="${J:-$(nproc 2>/dev/null || echo 4)}"
WORK=$(mktemp -d) || exit 1
# The shards themselves must sit directly under tests/, not in $WORK: the
# prelude computes DIR from its own BASH_SOURCE, so a shard anywhere else
# points $DIR at a temp dir and every assertion that reads the repo's config
# or scripts fails. tests/only.sh mktemps into tests/ for the same reason.
# Flat, not a subdirectory: dirname/.. has to land on the repo root exactly.
SH="$DIR/tests/.par.$$"
mkdir -p "$WORK/list" "$WORK/out" || exit 1
trap 'rm -rf -- "$WORK"; rm -f -- "$SH".*' EXIT

# Snapshot first: a concurrent editor saving tests/run.sh mid-run would
# otherwise hand half a file to a shard still being generated, which reads as
# a syntax error in code nobody touched.
cp "$DIR/tests/run.sh" "$WORK/run.snap" || exit 1

# Groups.  A block is its own group unless the line above its `echo "== "` is
# `# group: NAME`; blocks sharing a NAME run in one shard, in source order,
# because they read variables the first of them set.
awk '
    /^# group: / { g = $3; next }
    /^echo "== / { n++
                   if (g == "") g = "solo" n
                   if (!(g in first)) first[g] = n
                   printf "%04d\t%d\t%s\n", first[g], n, g
                   g = "" }
' "$WORK/run.snap" > "$WORK/groups.tsv"

# PAR_BLOCKS restricts the run to a comma-separated set of ordinals (see
# tests/changed.sh). Whole GROUPS survive the filter: selecting block 96 without
# block 95 would drop the server and panes it reads, so if any member is wanted
# the group runs entire.
if [ -n "${PAR_BLOCKS:-}" ] && [ "${PAR_BLOCKS}" != ALL ]; then
    awk -F'\t' -v want=",${PAR_BLOCKS}," '
        { if (index(want, "," $2 ",")) keep[$1] = 1; row[NR] = $0 }
        END { for (i = 1; i <= NR; i++) { split(row[i], c, "\t"); if (c[1] in keep) print row[i] } }
    ' "$WORK/groups.tsv" > "$WORK/groups.sel" && mv "$WORK/groups.sel" "$WORK/groups.tsv"
fi

while IFS=$'\t' read -r id n g; do
    printf '%s\n' "$n" >> "$WORK/list/$id"
    [ "$g" = serial ] && serial_ids=" $id${serial_ids:-}"
done < "$WORK/groups.tsv"

[ -s "$WORK/groups.tsv" ] || { echo "  nothing selected"; exit 0; }

# The audit. A block reading a variable a neighbour set passes under `make
# test-par`, whose shard carries that neighbour, and goes red alone (#120,
# #125). One run per GROUP: blocks sharing a `# group:` may share fixtures.
if [ -n "$EACH" ]; then
    each_red=""; each_n=0
    for f in "$WORK"/list/*; do
        sel=$(tr '\n' ',' < "$f"); sel=${sel%,}
        each_n=$((each_n + 1))
        PAR_BLOCKS="$sel" "$DIR/tests/par.sh" > "$WORK/each.log" 2>&1 && continue
        each_red="$each_red $sel"
        printf '== red alone: %s ==\n' "$sel"
        sed 's/^/  /' "$WORK/each.log"
    done
    printf '  %d groups run alone, red alone:%s\n' "$each_n" "${each_red:- none}"
    [ -z "$each_red" ] || exit 1
    exit 0
fi

for f in "$WORK"/list/*; do
    id=${f##*/}
    awk -v want=",$(tr '\n' ',' < "$f")" '
        /^echo "== / && !seen_block { seen_block = 1 }
        !seen_block { print; next }
        /^[A-Za-z_][A-Za-z_0-9]*\(\) *\{/ { infn = 1; depth = 0 }
        infn { print; o = $0; c = $0
               depth += gsub(/\{/, "", o) - gsub(/\}/, "", c)
               if (depth <= 0) infn = 0
               next }
        # run.sh prints its own totals and exits; the shard reports to the
        # merger instead, so drop them wherever the last block lands.
        /^printf .  %d passed/ { next }
        /^\[ "\$fail" -eq 0 \]/ { next }
        /^echo "== / { n++; keep = (index(want, "," n ",") > 0) }
        keep { print }
        END { print "printf %d,%d,%d\\\\n \"$pass\" \"$fail\" \"$skipped\" > \"$PAR_COUNT\""
              print "[ \"$fail\" -eq 0 ] || exit 1" }
    ' "$WORK/run.snap" > "$SH.$id"
    # A block whose `if`/`fi` straddles the next block's title does not stand
    # alone; name every one of them, not just the first, so one `# group:`
    # pass fixes them all. Not `bash -n | sed`: a pipeline reports sed's
    # status, so every shard would look valid.
    bash -n "$SH.$id" 2>"$WORK/nerr" && continue
    sed "s|^|par.sh: shard $id: |" "$WORK/nerr" >&2
    bad_shards="${bad_shards:-} $id"
done
[ -z "${bad_shards:-}" ] || { echo "par.sh: ungrouped shards:${bad_shards}" >&2; exit 2; }

run_shards() {                       # <parallelism> <shard file>...
    local p=$1; shift
    printf '%s\n' "$@" | xargs -P "$p" -I{} sh -c '
        id=${1##*.}
        PAR_COUNT="'"$WORK"'/out/$id.n" export PAR_COUNT
        bash "$1" > "'"$WORK"'/out/$id.log" 2>&1 || : > "'"$WORK"'/out/$id.fail"
    ' _ {}
}

# `# group: serial` blocks run against a clock or a large fixture, so they
# measure the box's spare CPU as much as the code. Measured on the ones there
# were: green at J=4, red at J=8 and J=12. They run first, alone, while nothing
# competes; the other blocks then take the machine. No block carries the mark
# today; the shard stays for the next one that needs it.
serial=""; rest=""
for f in "$SH".*; do
    case " $(printf '%s' "${serial_ids:-}" | tr ' ' '\n' | sort -u | tr '\n' ' ') " in
        *" ${f##*.} "*) serial="$serial $f" ;;
        *)              rest="$rest $f" ;;
    esac
done
# shellcheck disable=SC2086  # deliberate word splitting: a list of shard paths
[ -z "$serial" ] || run_shards 1 $serial
# shellcheck disable=SC2086
run_shards "$J" $rest

pass=0; fail=0; skip=0; red=0
for f in "$SH".*; do
    id=${f##*.}
    cat "$WORK/out/$id.log"
    if [ -s "$WORK/out/$id.n" ]; then
        IFS=, read -r p q s < "$WORK/out/$id.n"
        pass=$((pass + p)); fail=$((fail + q)); skip=$((skip + s))
    fi
    # A shard can die before it counts -- set -u on a variable a block in
    # another shard sets. That is a grouping bug, so say so, do not just
    # report a total that quietly does not add up.
    [ -e "$WORK/out/$id.fail" ] || continue
    red=1
    [ -s "$WORK/out/$id.n" ] || echo "par.sh: shard $id died without reporting" >&2
done
printf '  %d passed, %d failed, %d skipped  (J=%s, %s shards)\n' \
    "$pass" "$fail" "$skip" "$J" "$(find "$DIR/tests" -maxdepth 1 -name ".par.$$.*" | wc -l)"
[ "$red" -eq 0 ] || exit 1
