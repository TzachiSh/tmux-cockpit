# tmux-cockpit: why `make check` takes ~3 minutes, and how to make it ~40 s

Measured on the 12-core box, tmux 3.7c / mawk / fzf 0.44.1, working copy
`/tmp/.../scratchpad/speed` (a copy of `cockpit-wt/phase3-loop`).

**Caveat on every wall clock below:** load average was 4.9–6.0 the whole time
(the other agents in this session). Absolute seconds are pessimistic by maybe
10–20 %; the *ratios* and the pass/fail results are what to trust. Every timing
was taken at least twice.

## Headline

| | wall clock | verdict |
|---|---|---|
| `make check` today (lint + test) | **162.4 s** | green |
| ├ `make lint` | **17.2 s** | 14.1 s of it is shellcheck on `tests/run.sh` alone |
| └ `tests/run.sh` | **145.2 s** | 618 passed, 0 failed |
| `tests/freeze-rig/run.sh gate` today | **69.6 s** | FAIL, 696 visits (5.8 walks) |
| **`make -j2 check-par`** (lint beside a 12-way sharded suite) | **40.5 / 40.6 s** | **green ×2, 4.0×** |
| ├ `make test-par` (J=12) | 38.0 / 38.8 / 39.1 s | 618 passed, 0 failed, ×3 |
| `tests/run.sh` after 10 sleep→poll conversions | 130.2 s | 618 passed |
| …plus halving the poll tick | 124.2 / 124.3 / 125.1 s | 618 passed, ×3 |
| gate at `REPS=3 SETTLE=0.5` | **30.0 / 30.1 / 30.2 s** | identical numbers |
| `make gate`, content unchanged since last green (§6a) | **0.03 s** | stamp hit |
| `make gate`, content moved → full lint + suite + stamp | 41.7 / 42.3 s | 618 passed, ×2 |
| `make test-changed` after a one-line script edit (§6b) | **0.4 – 19 s** | caught an injected break in 8.2 s |

**`make check` + gate: 232 s → 70 s.** The two changes that do it are a new
135-line `tests/par.sh`, fourteen `# group:` comment lines, and two default
values in `gate.sh`.

One correction to the brief's premise worth stating up front: **lint is not
free.** `make check` is lint + test and shellcheck costs 17.2 s, 14.1 s of which
is the single 5,900-line `tests/run.sh`. It cannot be split further (I measured
`xargs -P 12` over the file list: 15.2 s, no real gain — one file dominates), but
it *can* run beside the tests. `make -j2 check-par` hides it completely.

---

## 1. The profile

The profile at `scratchpad/prof/profile.txt` is unusable — every per-block field
reads `0.0` (only its `TOTAL 146.4` is real). I re-profiled by injecting a
`date +%s.%N` mark before each of the 116 `^echo "== "` lines: results in
`scratchpad/myprof.txt`, run wall clock 145.2 s, sum of blocks 144.2 s.

The distribution is top-heavy but with a long tail: top-5 = 39.6 s, top-15 =
87.7 s (61 %), top-30 = 120.5 s, remaining 86 blocks = 24 s.

### Top 15 blocks

`sleep` = seconds of fixed `sleep` textually inside the block (some do not
execute; see §3).

| s | sleep | line | block | mechanism |
|---:|---:|---:|---|---|
| 8.58 | 0.0 | 4380 | `--deliver: a pane on a permission prompt is refused` | **polled round-trips**, not sleeps: several `rundeliver` calls, each a `send-keys` of the real `tmux-review --deliver` into a live agent pane then a 40×0.1 s poll for `RC=<digit>` |
| 8.45 | 8.3 | 1286 | `the branch picker must not hijack ctrl-n` | **all fixed sleeps**: `2` for fzf to start, then `1`+`1`+`1` around typing `zzz` / `C-n` / `Escape`, `1.2` + 2×`0.5` for the alt-enter create failure |
| 8.11 | 10.0 | 2416 | `prefix J jumps to the pane that needs you` | **4× `sleep 2`**, each waiting for a nested `tmux-jump` client to fork, connect and move the window. Comment says "sleep 2, not 1 … under concurrent load" |
| 7.40 | 7.3 | 2088 | `tm: typing filters, ^X kills, the attached session survives` | fixed sleeps around an fzf picker: `1.2` spawn, `2` settle, `0.8` typing, 2×`1.2` after `C-x`, `0.4`, `0.5` |
| 7.07 | 7.0 | 1770 | `picking a folder browses into it` | `2.2` popup settle + `0.4`+`0.4` Tab + `2.5` after Enter |
| 6.43 | 6.0 | 1135 | `a directory name can no longer run code through the status job` | **nested-client attach ×2**: `sleep 2` for `tmux -L INNER attach` inside an outer server's pane, then `sleep 1` after `refresh-client` for the status render. Done twice (hostile cwd, then normal cwd) |
| 5.95 | 2.4 | 3594 | `--loop: alt-enter pins, ctrl-s delivers, esc discards` | mostly **fzf startup + polled** `until_cap` on the `review>` prompt; 2.4 s of it is fixed sleeps |
| 5.59 | 5.5 | 1728 | `tab marks several files and opens them all` | `2.2` popup + 2×`0.4` Tab + `2.5` Enter |
| 5.58 | 5.3 | 1620 | `clicking the directory segment browses the working dir` | `0.3` run-shell + `2.2` popup + `0.8` typing + `2` Enter |
| 4.65 | 4.6 | 1874 | `^O opens a folder whole; the list names the line` | `2.2` popup + `0.4` Down + `2` after `C-o` |
| 4.53 | 0.2 | 4329 | `--deliver: the composed review lands typed-but-unsent` | polled `rundeliver` round-trips, same as row 1 |
| 4.05 | 0.2 | 5815 | `--review: moving between projects leaves every review where it was` | polled `until_cap` on a real fzf session across two project dirs |
| 3.84 | 3.5 | 1487 | `the picker fires even where the app owns the mouse` | `0.5` + 2×`1.5` around a synthesised double-click, waiting for the popup |
| 3.76 | 0.0 | 2791 | `--lines: a giant diff caps to one notice` | **real work + `timeout` perf gates**: builds a 6 M-row tree, a 4.6 MB lockfile and a 5 MB-line fixture, then five `timeout 1.5` and one `timeout 10` budget assertions |
| 3.73 | 3.5 | 1373 | `the path picker fires in a window with a split` | `0.4` + `1.5` + `1.5` double-click → popup |

**Mechanism summary:** 11 of the top 15 are dominated by *fixed sleeps waiting
for an event that could be polled*. Two (`--deliver` ×2, `moving between
projects`) are already polled and are bounded by 0.1 s tick granularity. One
(`giant diff`) is genuine CPU work under a wall-clock budget.

### Fixed sleeps that actually execute

I shimmed `sleep` onto `PATH` and logged every call (`scratchpad/sleeps.txt`,
471 calls). Excluding pane commands (`sleep 300` ×36, `sleep 3600` ×1,
`sleep 30` ×1) and the two backgrounded FIFO holders (`( exec 9>$FIFO; sleep 8 ) &`
at lines 891 and 2557, which do not block):

| duration | count | seconds |
|---:|---:|---:|
| 2.5 | 1 | 2.5 |
| 2.2 | 4 | 8.8 |
| 2 | 13 | 26.0 |
| 1.5 | 4 | 6.0 |
| 1.2 | 4 | 4.8 |
| 1 | 15 | 15.0 |
| 0.8 | 4 | 3.2 |
| 0.6 | 3 | 1.8 |
| 0.5 | 9 | 4.5 |
| 0.4 | 6 | 2.4 |
| 0.3 | 16 | 4.8 |
| 0.25 | 2 | 0.5 |
| 0.2 | 3 | 0.6 |
| 0.05 | 2 | 0.1 |
| **blocking fixed sleeps** | **86** | **81.0 s** |
| poll ticks (`sleep 0.1` in `until_cap`/`until_file`/`rundeliver`) | 345 | 34.5 s |

**81 s of the 145 s run is a fixed sleep, and another 34.5 s is poll ticks.**
The suite spends ~80 % of its wall clock waiting.

(The census run itself reports 26 failures — an artefact of the instrumentation:
`exec -a claude sleep 3600` resolves `sleep` to the shim, which re-execs
`/bin/sleep` and loses the `claude` argv[0], so the agent glob stops matching.
The durations are unaffected.)

---

## 2. The sharded parallel runner

### `tests/par.sh` (135 lines with §6b's PAR_BLOCKS filter, shellcheck-clean; copy at `scratchpad/par.sh`)

It reuses `tests/only.sh`'s extraction awk verbatim — prelude, every top-level
function, the chosen blocks — with one change: **blocks are selected by ordinal,
not by title pattern**, so a title carrying regex metacharacters (there are
several: `^X`, `$EDITOR`, `--`, `?`) cannot pick up the wrong block.

```
tests/run.sh ──cp──> snapshot ──awk group pass──> N group lists
                                       │
                                       └─> one self-contained shard script per group,
                                           written to tests/.par.$$.NNNN, bash -n'd
                                       │
              serial phase (J=1) ──────┤   # group: serial shards, alone
              parallel phase (xargs -P J) │   everything else
                                       │
                        merge logs in shard order, sum counts, exit 1 if any red
```

Five things it gets right, each of which I hit as a bug first:

1. **`$DIR` comes from `BASH_SOURCE`.** My first version wrote shards into a
   `mktemp -d` and 440 of 618 assertions failed, because the prelude's
   `DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"` then pointed at a
   temp dir instead of the repo. Shards must sit **flat in `tests/`** —
   `only.sh` already does this and says why in a comment. Not a subdirectory:
   `dirname`/.. has to land on the repo root exactly.
2. **Isolation is already there and is free.** Everything the prelude allocates
   is keyed on `$$` — `SUITE_SOCK=cockpit-suite-$$`, `cockpit-rv-$$`,
   `cockpit-rd-$$`, `STATE=$(mktemp -d)`, `XDG_RUNTIME_DIR=$STATE`,
   `SHIM=$(mktemp -d)`, `TMPS=$(mktemp)`, and `SOCKS`/`kill_socks` operating only
   on its own names. N shards are N processes with N distinct PIDs, so each gets
   its own tmux server, its own `$PANE` on that server, its own state dir and its
   own cleanup registry. **No sharing to break, and nothing in par.sh to
   maintain for it.** The `SUITE_SOCK` server and `$PANE` are per-shard, not
   shared — the concern in the brief does not materialise.
3. **The snapshot.** `cp tests/run.sh $WORK/run.snap` before generating anything,
   so a concurrent editor cannot hand half a file to a shard mid-generation
   (the known "fake syntax error" failure mode).
4. **Exit code propagates.** Each shard's own `[ "$fail" -eq 0 ] || exit 1`
   drops a `.fail` marker; the merger sets `red=1` and exits 1. Verified: red
   runs returned 1, green runs 0. `run.sh`'s own trailing summary/`exit 1` is
   stripped from whichever shard holds the last block, so a red shard still
   reports its counts instead of dying before them.
5. **A shard that dies before counting is named**, not silently subtracted from
   the total — that is the signature of a missing `# group:`.

`make test` is untouched (`@tests/run.sh`, one process, source order). New
target:

```make
test-par:
	@tests/par.sh          # J=8 make test-par to override; default nproc
```

### Blocks that cannot be sharded apart

Found two ways: statically (an awk pass for variables assigned in one block and
read in a later one) and empirically (generate all 116 single-block shards and
`bash -n` each; then run them). Both agree.

**Group `deliver` — blocks 95–100, lines 4200–4563.** Two independent reasons:

- *Variables.* Block 95 creates the server and both panes; 96–100 read them:
  `RDS` (the socket) → 96,97,98,99,100; `AP` (the agent pane) → 96,97,98,99;
  `NP` (the plain pane) → 96,98,99; `RDSTORE` → 96,97. Under `set -u` a shard
  missing these dies rather than failing an assertion.
- *Syntax.* `if need_tmux "tmux-review --deliver"; then :; else` opens at line
  4201 and its `fi` is at 4563 — **the `if` straddles six block titles**, so
  blocks 95 and 99 do not even parse standalone. These were the only two blocks
  of 116 that failed `bash -n` in isolation.

**Group `etmux` — block 25 (`tmux-open-path: the editor command survives odd
filenames`, line 695) + block 58 (`@cockpit_open_single skips the popup for
machine output`, line 2059).** Block 25 sets
`ETMUX_TEMPLATE=$(mktmpf); cp "$ETMUX/tmux" "$ETMUX_TEMPLATE"`; block 58 does
`cp "$ETMUX_TEMPLATE" "$OST/tmux"`. **Non-contiguous, 33 blocks apart** — this
is the case that rules out "just split into contiguous ranges". (Alternative
fix, if you would rather not couple them: give block 58 its own three-line
`tmux` stub, since it only needs a stub, not *that* stub.)

`RVS` is **not** a cross-block dependency despite `only.sh`'s header saying so:
it is assigned at 3596 and its last use is at 3727, inside the same block (the
next title is at 3731). The header comment is stale.

Everything else that my static pass flagged is a false positive: loop variables
(`i`, `c`, `d`, `p`, `v`, `w`) and env-prefix assignments (`HOME=`, `PATH=`,
`TMUX=`, `TMUX_PANE=`).

**How the sharder keeps them together:** a `# group: NAME` comment on the line
above `echo "== …"`. Blocks sharing a NAME land in one shard in source order;
an unmarked block is its own group. 14 marker lines total, no other source
change. A missing marker is loud, never silent — either `bash -n` refuses the
shard by name, or the shard dies on `set -u` and the merger reports "shard NNNN
died without reporting".

### Measurements

All runs green = 618 passed / 0 failed / 0 skipped, the same count the serial
suite produces (so the extraction is faithful, not silently dropping blocks).

| J | wall | green? | note |
|---:|---:|---|---|
| serial `tests/run.sh` | 145.2 s | yes | baseline |
| 1 | 173.4 s | yes | +28 s = 109 preludes × 0.26 s |
| 4 | 52.4 / 56.6 s | yes | |
| 6 | 50.7 s | yes | |
| 8 | 42.1 / 42.7 s | yes | |
| **12** | **38.6 / 39.5 s** | **yes ×2** | markers only |
| 12 | 36.3 / 37.2 / 38.0 / 38.3 s | yes ×4 | with §3 conversions, 3 serial blocks |
| **12** | **39.1 / 38.0 / 38.8 s** | **yes ×3** | **final: 6 serial blocks, via `make test-par`** |

**J=12: 145.2 → ~38.5 s, 3.8×.** J=1 costs 173 s — 28 s more than the serial
suite, exactly the 109 preludes (measured at 0.26 s each via
`time tests/only.sh '^syntax$'`), which is the price of the design and is repaid
at any J ≥ 2.

Phase breakdown at J=12 (instrumented copy): generation of 109 shards **1.0 s**,
serial phase **9.8 s**, parallel phase **30.8 s**. Generation is not the problem.

**The parallel phase is CPU-saturated, not sleep-bound:** 8 workers give 30 s of
parallel phase, 12 workers give 30.8 s — 50 % more workers buys nothing. Twelve
tmux servers, twelve `fzf`s and a lot of `git` on 12 cores (already at load 5
from other work) is the ceiling. That is why `nproc` is a fine default and why
tuning J further is not worth it.

### The `timeout` perf gates under contention

This is the one real hazard, and it took more than one run to find all of it.
Green at J=4. Red at J=8 and J=12, in **six** blocks:

| block | line | gate | fails at |
|---|---:|---|---|
| `--lines: a giant diff caps to one notice` | 2791 | `timeout 1.5` ×5, `timeout 10` | J=8 **and** J=12, every run |
| `--fleet: dedups panes … 20 panes under a second` | 5222 | `timeout 1`, `timeout 15` | J=12 (once at J=8) |
| `--fleet: from a non-agent pane, the queue lists…` | 5038 | `timeout 1` | J=12 |
| `--preview-diff: the second render of a file is a cache hit` | 4082 | **`timeout 0.05`** | intermittent — did not appear until my 8th J=12 run |
| `--lines: builds rows without a fork or a quadratic append per line` | 3169 | `timeout 0.2` | same failure mode, added pre-emptively |
| `--rows: re-marking after a comment reads the cache, not git` | 3311 | `timeout 0.15` | same failure mode, added pre-emptively |

The failing assertions are wall-clock budgets ("a 6,000,030-row tree answers in
under 1.5 s", "20 panes scan in under 1 s", "a repeat render returns in under
50 ms") — they measure the box's spare CPU as much as the code.

**I nearly got this wrong.** After five green J=12 runs I had concluded three
blocks were enough. The `timeout 0.05` cache-hit gate then flaked on a later run
(it surfaced during the exit-code test, alongside the failure I had injected
deliberately). Five consecutive green runs is not proof for a gate this tight —
so the two other sub-second cache gates go in the serial group too, even though
I never caught them failing. They cost ~1 s each.

**Fix: `# group: serial`.** Those six run first, in one shard, alone, before the
parallel phase takes the machine. Costs ~12 s and buys a green J=12: verified
green ×3 after the expansion (39.1 / 38.0 / 38.8 s).

**These gates are not purely a sharding problem.** One of my three serial
`make test` runs (no sharding at all, one process, source order) exited nonzero
while the box was at load 7.2 from the other agents; the two either side of it
were green at 127.9 s and 128.8 s. I did not capture which assertion — output
was going to `/dev/null` — so I cannot name it, but a serial run failing under
load is consistent with the same wall-clock budgets, and it means `make test` on
a busy machine is already exposed to this today. Sharding raises the odds; it
did not create them.

*Alternative, if ~12 s matters more than gate tightness:* raise those three
budgets (e.g. `timeout 1.5` → `3`) and let them run in the parallel phase. That
returns ~8 s but loosens a real performance assertion — the owner's call, not
mine. I did not do it.

---

## 3. sleep → poll

The suite already has the right primitives in the prelude — `until_cap <sock>
<pane> <glob>` and `until_file <file> <glob>`, both 40 ticks of 0.1 s = a 4 s
cap. Nothing new is needed except **one helper** and the discipline of naming
the event.

### The recipe that does most of the work

For "send-keys a command, then sleep N": append a sentinel and poll for it.

```bash
# before
send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'" Enter; sleep 2
# after
send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'; echo J''DONE1" Enter
until_cap "$JJ" "$JP1" '*JDONE1*'
```

The `''` is load-bearing and cost me a debugging round: `capture-pane` sees the
*typed* command line too, so a plain `echo JDONE1` matches instantly and the
poll proves nothing. Splitting the literal makes the typed text (`J''DONE1`)
and the printed text (`JDONE1`) different. The same trap applies to the
existing `echo JRC=\$?` sites — poll for `*JRC=0*`, never `*JRC=*`.

### Measured conversions (prototyped, all green)

| block | before | after | saved |
|---|---:|---:|---:|
| `prefix J jumps to the pane that needs you` (4× `sleep 2` → sentinel poll) | 8.32 s | 1.06 s | **7.3 s** |
| `a directory name can no longer run code…` (2× attach, 1 render) | 6.69 s | 1.79 s | **4.9 s** |
| `…neither can a NEWLINE or TAB…` (1× attach) | 3.45 s | 1.45 s | **2.0 s** |
| `tmux-branch: the popup resolves the pane…` (2× attach) | 2.52 s | 0.49 s | **2.0 s** |
| **10 sleep sites, 4 blocks** | 20.98 s | 4.79 s | **16.2 s** |

Whole suite: **145.2 s → 130.2 s** (−15.0 s, matching the block sum), 618
passed. Ten sites out of ~40.

### The nested-client attaches

Lines 1037/1042, 1156, 1178, 1215 (and 5329/5331/5334) all do:

```bash
tmux -L "$OUTER" new-session -d -s o "$TMUX_REAL -L $INNER attach -t s"
sleep 2                                  # for the attach to land
tmux -L "$INNER" refresh-client
sleep 1                                  # for the status line to render
```

The attach **is** a pollable event — the inner server's client count:

```bash
until_clients() {             # <inner socket> <how many>
    local i
    for i in $(seq 1 80); do
        [ "$("$TMUX_REAL" -L "$1" list-clients -F x 2>/dev/null | wc -l)" -ge "$2" ] && return 0
        sleep 0.05
    done
    return 1
}
```

Note **the inner socket, not the outer one** — I got this wrong first and the
block went from 2.5 s to 7.1 s, because the outer server is `new-session -d` and
never has a client at all. The `tmux-branch: the popup resolves` block needs
`until_clients "$INNER" 2`: the bug it reproduces ("resolves into the busiest
client") requires both nested clients up, so the count matters, not just
presence.

### Which `sleep ≥ 1` sites convert, and which must not

| line(s) | what | verdict | est. |
|---|---|---|---:|
| 1039, 1042 | nested attach ×2 | ✅ done → `until_clients` | 2.0 s ✔ |
| 1156, 1178, 1215 | `sleep 2` nested attach | ✅ done → `until_clients` | 5.5 s ✔ |
| 1180 | `sleep 1` render, **positive** ("a normal cwd still renders its path") | ✅ done → `until_cap "*$D2*"` | 0.9 s ✔ |
| 2437, 2442, 2452, 2459 | `sleep 2` after `tmux-jump` | ✅ done → sentinel poll | 7.3 s ✔ |
| 1306 | `sleep 2`, fzf branch list appears | → `until_cap '*● main*'` | 1.8 s |
| 1319 | `sleep 1` after typing `zzz` | → `until_cap '*zzz*'` (the query echoes) | 0.9 s |
| 1323 | `sleep 1` after `Escape` | → `until_cap` for the shell prompt returning, or `#{pane_current_command}` back to `bash` | 0.9 s |
| 1348 | `sleep 1.2`, create fails and pauses | → `until_cap` for the failure message | 1.0 s |
| 1397, 1403 | `sleep 1.5` ×2, double-click → popup | → `until_file` on `$TMUX_LOG` (the stub writes `POPUP …`) | 2.6 s |
| 1509, 1512 | `sleep 1.5` ×2, same pattern | → `until_file` | 2.6 s |
| 1646, 1653 | `2.2` popup + `2` Enter | → `until_cap` prompt, `until_file` log | 3.5 s |
| 1744, 1761 | `2.2` popup + `2.5` Enter opens files | → `until_cap`, `until_file` | 4.0 s |
| 1784, 1786, 1793 | `2.2` + `2` + `2` | → `until_cap` / `until_file` | 5.2 s |
| 1885, 1894 | `2.2` popup + `2` after `C-o` | → `until_cap` / `until_file` | 3.7 s |
| 2101, 2104, 2118, 2122 | `1.2`+`2`+`1.2`+`1.2`, `tm` picker | → `until_cap` on the picker/session list | 5.0 s |
| 3673 | `sleep 1` after Enter in `--loop` | → `until_cap` (block already uses it) | 0.9 s |
| 5331, 5334 | `sleep 1` ×2 nested attach | → `until_clients` | 1.8 s |
| 5470 | `sleep 1` after `prefix R` reload | → poll the option the reload sets | 0.9 s |
| **1158, 1217** | `sleep 1` render, then `[ -e "$SP/OWNED" ]` — **"the hostile `$(…)` never ran"** | ❌ **keep**: a negative assertion, the bounded wait *is* the test | — |
| **1321** | `sleep 1` after `C-n`, then "creates no branch" | ❌ **keep** (a proxy exists — the fzf pointer moves — but it is weaker than the wait) | — |
| **1280** | `sleep 1` inside `for i in 1..8; printf; sleep 1` | ❌ **keep**: this is the *fixture* — a slow drip feeding the `notify` deadline test | — |
| **2342** | `[ "$a" = "-s" ] && sleep 1` | ❌ **keep**: inside a stub script, it is the stub's simulated behaviour | — |
| **891, 2557** | `( exec 9>"$FIFO"; sleep 8 ) &` | ❌ **keep**: backgrounded FIFO holders, they never block the suite | — |

**Estimated total: ~50 s of the 81 s removable** (16.2 s already realised,
~35 s remaining), leaving ~31 s of genuinely-negative or fixture sleeps.

### The poll tick

345 poll ticks × 0.1 s = 34.5 s per run. Halving the tick and doubling the
bound (`sleep 0.1` → `0.05`, `seq 1 40` → `seq 1 80`, 38 + 18 sites, same 4 s
ceiling) measured **130.2 s → 124.2 / 124.3 s, −6.0 s**, green ×2. Less than the
naive 17 s because most polls already succeed on their first tick. Free, but
minor; it is a `sed`, and it makes the already-polled `--deliver` blocks (the
two slowest in the suite) cheaper.

### tmux `wait-for`

Available (`wait-for -S <chan>` / `wait-for <chan>`) and it *would* be a precise
primitive, but it needs a signaller. Nothing here signals: the suite is waiting
on fzf's startup, on a nested client's connect, and on tmux's own render — none
of which call `wait-for -S`. It could be made to work for the attaches with
`set-hook -g client-attached 'run-shell "tmux wait-for -S attached"'`, but that
is more machinery and more tmux state than `until_clients`, for the same result.
**Not worth it** — the polling helpers the suite already has cover every case.

---

## 4. The freeze rig

`bash tests/freeze-rig/run.sh gate` → **69.6 s**, verdict FAIL:

```
pane visits per status redraw: 696   (120 panes, budget 120 = 1.0x a full walk)
FAIL (redraw and move over budget): redraw 5.8 full server walks on 120 panes
```

(5.8 walks — consistent with the recorded 5.8/5.9 for this tree.)

### Where the 69.6 s goes

| | seconds |
|---|---:|
| build 120 panes (90 `split-window` + 120 `respawn-pane`) + attach a client | ~11.5 |
| `sleep 1` after respawning agents, `sleep 1.5` after the `script -qfc` attach, `sleep 1` after the alpha/bravo respawn | 3.5 |
| redraw loop: `REPS` × 2 × `SETTLE` | 10.0 |
| `move_cost` ×4: `4 × REPS × 2 × SETTLE` | 40.0 |
| | **≈ 65 + build** |

**`move_cost` is 4/7 of the whole rig** — four move measurements each paying the
full `REPS × 2 × SETTLE`.

### Load-bearing vs not

- `SETTLE` **is** load-bearing in kind: the count is taken as a *difference*
  across a forced redraw, and the settle is what guarantees the redraw's
  `pane loop:` lines are all flushed to the `-vv` log before the second `grep -c`.
  Too short and you undercount. But 1.0 s is far more than it needs.
- `REPS` takes the **minimum** over reps. More reps means more chances to catch
  an *undercount*, so raising REPS makes the number lower, not more reliable.
  This showed up live: the one anomalous run of the whole exercise was at the
  **defaults** (`REPS=5 SETTLE=1.0`), which reported `project→project 529,
  agent→terminal 332` instead of the stable `706 / 712`. Every reduced setting,
  and the two repeat runs at defaults, gave 706/712.
- The `sleep 1` / `sleep 1.5` build settles are one-shot and cheap; the `1.5`
  after the attach could be an `until_file`-style poll for
  `$LOGDIR/tmux-server-*.log` appearing (the script already checks for it and
  exits 2 if absent), saving ~1.3 s. Marginal.

### Can REPS/SETTLE drop? Yes — measured

Every row below produced the **identical** verdict and the identical
`redraw = 696`:

| REPS | SETTLE | wall | redraw | proj | term | pane | ctl |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 5 | 1.0 (default) | 69.6 | 696 | 529 | 332 | 706 | 706 |
| 5 | 1.0 (repeat) | 67.2 | 696 | 706 | 712 | 706 | 706 |
| 5 | 1.0 (repeat) | 67.4 | 696 | 706 | 712 | 706 | 706 |
| 5 | 0.5 | 42.3 | 696 | 706 | 712 | 706 | 706 |
| 3 | 1.0 | 45.3 | 696 | 706 | 712 | 706 | 706 |
| **3** | **0.5** | **30.2 / 30.0 / 30.1** | **696** | **706** | **712** | **706** | **706** |
| 3 | 0.3 | 24.1 | 696 | 706 | 712 | 706 | 706 |
| 3 | 0.2 | 21.0 | 696 | 706 | 712 | 706 | 706 |
| 2 | 0.5 | 24.0 | 696 | 706 | 712 | 706 | 706 |
| 2 | 0.4 | 21.9 | 696 | 706 | 712 | 706 | 706 |

**Recommend `REPS=3 SETTLE=0.5`: 69.6 s → 30.0 s (−57 %), same numbers, run
three times.** `SETTLE=0.3` and `0.2` also held on this box, but with the load
already at 5 that is closer to the edge than I would ship — 0.5 keeps a 2×
margin over the point where anything moved (nothing did, down to 0.2).

Two lines in `gate.sh`:

```bash
REPS=${REPS:-3}
SETTLE=${SETTLE:-0.5}
```

The remaining ~11.5 s is building the 120-pane shape: 210 sequential tmux
round-trips. Batching them into chained `\;` commands would cut it, but that is
a real rewrite of the rig's setup for ~8 s — **not recommended**.

---

## 4b. A finding the brief did not ask about: lint

`make check` is `lint + test`, and I had assumed lint was noise. It is not:

| | seconds |
|---|---:|
| `shellcheck tests/run.sh` (5,887 lines) | **14.1** |
| `shellcheck scripts/tmux-review` | 0.8 |
| everything else in `SOURCES` (~22 files) | ~2.3 |
| **`make lint`** | **17.2** |

That is 11 % of today's `make check` and it is **not** parallelisable by file —
one file is 82 % of it (`xargs -P 12` over the file list: 15.2 s, a 2 s gain).
But it does not need to be: it is independent of the tests, so it can simply run
beside them.

```make
check-par: lint test-par        # make -j2 check-par
```

Measured: **40.5 / 40.6 s green ×2**, against 38.0–39.1 s for `make test-par`
alone — the entire 17.2 s of lint disappears into the tests' wall clock. Both
recipes still run and both exit codes still gate the target.

(`make -j2 check` on the *existing* target works the same way and needs no
Makefile change at all, if you would rather not add a target.)

---

## 5. Ranked recommendations

Smallest change first. "Saved" is measured unless marked *est.*

| # | change | diff | saved | risk |
|---|---|---|---:|---|
| **1** | **`gate.sh`: `REPS=${REPS:-3}`, `SETTLE=${SETTLE:-0.5}`** | 2 lines | **39.6 s** (69.6→30.0) | **None measured.** Identical numbers ×3; the only anomaly seen was at the old defaults. |
| **2** | **`tests/par.sh` + 14 `# group:` markers + `make test-par`** | 1 new 135-line file (reuses `only.sh`'s awk), 14 comment lines, 6 Makefile lines | **~107 s** (145→38.5) | Low. Green ×3 at J=12 in the final configuration (×9 overall). `make test` untouched. Isolation is pre-existing (`$$`), not new. A missing marker fails loudly (`bash -n` by name, or "died without reporting"), never silently. |
| **2b** | **`check-par: lint test-par`, run as `make -j2 check-par`** | 2 Makefile lines | **17.2 s** (the whole lint, hidden behind the tests) | None. Both recipes still run and both exit codes still gate; verified green ×2 at 40.5 / 40.6 s. |
| **2c** | **green stamp — `tests/green.sh` + `make gate`** (§6a) | 1 new 57-line file, 4 Makefile lines | **the entire gate, 42 s → 0.03 s**, on any commit that changes nothing the tests read | Very low. The key is content, so it cannot go stale in the unsafe direction — a wrong key only costs a re-run. Tool versions are in it. Verified mtime-only touch stays green, one byte invalidates, green→dirty→green ×4. |
| **2d** | **`make test-changed` — `tests/changed.sh` + `PAR_BLOCKS`** (§6b) | 1 new 85-line file, 12 lines in `par.sh`, 2 Makefile lines | **38 s → 0.4–19 s** per edit (7.7 s for a `tmux-where` change) | Low **for a developer loop, which is all it is.** Prints `ALL` for anything unrecognised, for a prelude/helper change, and with no stamp. Caught a real injected break in 8.2 s. Five blocks want a `# tests:` marker (§6b). Merge still runs the full gate. |
| **3** | **sentinel-poll the ~14 remaining `send-keys … ; sleep N` sites** | ~30 lines | **~25 s** *est.* (of which 7.3 s already proven on `prefix J`) | Low, one trap: split the sentinel literal (`J''DONE1`), `capture-pane` sees the typed line. Each site is independently testable with `make test-only`. |
| **4** | **`until_clients` + the 7 nested-attach sites** (5 done: 1039, 1042, 1156, 1178, 1215; left: 5331, 5334) | 11-line helper + 7 sites | **8.9 s** measured over 3 blocks, ~1.8 s *est.* left | Low. Poll the **inner** socket. |
| **5** | **`until_file` the 6 double-click → popup sites** (1397/1403/1509/1512) | ~8 lines | **~5 s** *est.* | Low; the tmux stub already writes `POPUP` to `$TMUX_LOG`. |
| **6** | **poll tick `0.1`→`0.05`, bound `40`→`80`** | one `sed`, 56 sites | **6.0 s** | Very low; same 4 s ceiling, ×2 the `capture-pane` forks. |
| **7** | *(optional, owner's call)* raise the three `serial` blocks' `timeout`s so they can shard | 3 numbers | ~8 s | **Medium — loosens a real perf assertion.** I did not do this. |
| **8** | *(not recommended)* batch the rig's 210 setup round-trips | rewrite of gate.sh setup | ~8 s | Medium; rewrites the part of the rig that makes the number comparable to `RESULTS.txt`. |

**Doing 1 + 2 + 2b alone — 2 lines, one new file, 14 comments, one Makefile
target — takes `make check` + gate from 232 s to 70 s, measured.** Adding 2c
takes the *repeat* cost of an unchanged tree to 0.03 s, and 2d takes the
edit-one-script loop to 0.4–19 s. Items 3–6 then take the single-process
`make test` from 145 s to ~95 s *est.* (124 s already measured with 4 and 6
done), which matters for `make test-only` iteration even though the sharded path
is CPU-bound and barely moves.

### What I did not do

- No new dependency. `xargs -P` is the scheduler; awk, `bash -n` and `mktemp` do
  the rest.
- No cost table / bin-packing. Prelude overhead is 0.26 s, so per-group shards
  with `xargs -P` pulling work dynamically self-balance and cannot go stale.
- No framework, no test-runner abstraction, no per-shard config.
- Did not touch `make test`, `tests/only.sh`, or any assertion.

---

## 6. Incremental gating: don't pay the full gate on every commit

Two independent mechanisms, both prototyped and measured in the working copy.
**Both are developer-loop tools. The full parallel gate still runs at merge** —
`make gate` runs it automatically whenever the content key does not match, so
skipping is never a decision anyone has to remember to un-make.

### (a) The green stamp — `tests/green.sh`

```
make gate      # already green -> prints and exits 0 in 0.03 s
               # otherwise     -> make -j2 check-par, then stamps on success
```

Measured: **0.03 s when green**, 41.7 s when not (a full `lint` + 12-way suite,
then the stamp). Verified green→dirty→green across four cycles.

**The key is a manifest, not a git tree.** One `sha256` per file the tests read,
plus `tmux -V`, `fzf --version`, `git --version`:

| option | cost | why not |
|---|---:|---|
| `sha256sum <files>` | **3 ms** | — chosen |
| `git rev-parse HEAD^{tree}` + `git diff HEAD \| sha256sum` + `git ls-files -o` | 31 ms | 10× slower, three forks, needs git, and two extra steps to cover unstaged and untracked files |

The manifest also hashes the bytes the tests actually read rather than a
commit's idea of them, which is the property that matters: a dirty worktree is
the normal state during development. Per-file rather than one digest over the
lot, because then **(b)'s "what changed" falls out of a plain `diff`** — no
second mechanism to build or keep in step. `sha256sum <files>` and not
`cat … | sha256sum`, so a rename, an addition or a deletion moves the key too.

Verified behaviours:

- `touch -d 2020-01-01 scripts/agent-state` (mtime only, content identical) →
  **stays green**. A timestamp-keyed stamp would have re-run the suite here.
- one appended newline in `scripts/agent-state` → **invalidates**.
- tool versions are in the key, so a tmux/fzf/git upgrade invalidates every
  stamp without a byte of the repo changing.

**Per-worktree, outside git:** `$(git rev-parse --git-dir)/cockpit-green`. In a
linked worktree that resolves to its own `.git/worktrees/<name>`, so two
checkouts never share a stamp and no `.gitignore` entry is needed. Falls back to
`tests/.cache/green` outside a repo. One ceiling, marked with a `ponytail:`
comment in the file: a plain `cp -r` of a worktree inherits its `.git` file and
therefore its stamp path. That is safe — a stale key only costs a re-run, it can
never produce a false green, because the key is content — but two such copies
would ping-pong the stamp.

### (b) `make test-changed` — only the blocks a change could have broken

```
make test-changed     # PAR_BLOCKS="$(tests/changed.sh)" tests/par.sh
```

Measured, each after a one-line edit to the named file:

| change | blocks | shards | assertions | wall |
|---|---:|---:|---:|---:|
| nothing (stamp matches) | 1 | 1 | 11 | **0.4 s** |
| `README.md` | 1 | 1 | 11 | **0.4 s** |
| `scripts/tmux-where` | 6 | 7 | 47 | **7.7 s** |
| `cockpit.tmux` | 17 | 17 | 102 | **19.0 s** |
| `scripts/agent-state` | 21 | 21 | 142 | **18.2 s** |
| `scripts/tmux-review` | 44 | — | — | — |
| `scripts/lib/review-store.sh` | 44 | — | — | — |
| *(full suite, for comparison)* | 115 | 104 | 618 | 38 s |

**It catches real breakage.** I changed `tmux-where`'s path branch to emit
`" XX%s"`; `make test-changed` selected 11 shards and went red in **8.2 s** on
exactly the right three assertions — `local path is ~-shortened`, `a normal pane
still shows its path`, `HOME unset does not crash`.

*(A first attempt at that test appended the break after the script's trailing
`exit 0` and "passed" — the break never executed. Worth recording: a green from
an incremental runner is only as good as the break you tested it with.)*

**The mapping is derived, not declared.** `tests/changed.sh` greps each block's
body for the scripts it runs — and resolves top-level helpers first, which is
what makes it work: `st() { "$DIR/scripts/agent-state" "$@"; }` means nine of
the state-machine blocks reach `agent-state` without ever naming it. Two passes
over `run.sh`, ~25 lines of awk.

**Why not map by title, as the brief proposed.** I measured both:

| approach | blocks needing a `# tests:` marker |
|---|---:|
| block title names its script | **59 of 115** |
| body-derived, helpers resolved | **5 of 115** |

Title mapping fails on whole families — every `prefix …` binding block (they
test `cockpit.tmux` and `tmux-jump`), every state-machine block (`state
transitions`, `done survives scanning`, `safety`, … all `agent-state`), `the
fixes review found` (four scripts at once), `worktree in the branch label`
(`tmux-where`). Deriving costs no markers and does not rot when a title is
reworded.

The **five** blocks that need an explicit `# tests: <file>…` marker — they drive
tmux formats or raw tmux commands and genuinely never name a script:

| # | block | really tests |
|---:|---|---|
| 12 | `the shipped config against a real agent pane` | `examples/tmux.conf`, `cockpit.tmux` |
| 15 | `stale state is disbelieved` | the formats from `cockpit.tmux` |
| 21 | `spaced window names survive the formats` | the formats from `cockpit.tmux` |
| 29 | `tmux-branch: local and remote branches` | `scripts/tmux-branch` (it goes through a `brs` helper and plain `git`) |
| 109 | `--deliver: the paste is gated atomically…` | `scripts/tmux-review`'s `if-shell` gate, reproduced with raw tmux |

`changed.sh` already honours a `# tests: <path>…` line inside a block, so those
five are a five-line follow-up. Until then they are covered by the fallbacks
below, not silently skipped.

**Fail-safe by construction.** It prints `ALL` — run everything — for any path
it does not recognise, for any change above the first block in `tests/run.sh`
(the prelude and the shared helpers), and when there is no stamp to diff
against. A change *inside* one block's body selects that block. `PAR_BLOCKS`
selects whole **groups**, never bare blocks, so picking block 96 cannot drop
block 95's server and panes.

**What it is not.** It is a fast loop for `while I edit this one script`, not a
merge gate. Selection is only as good as the derivation: an indirect dependency
nobody greps for — a script invoked through a tmux option value, a format
string assembled at runtime — would be missed. That is exactly why `make gate`
exists and why the stamp is keyed on content: the full suite runs on anything
the stamp has not already seen go green.


---

## Files

- `scratchpad/suite-speed-report.md` — this report
- `scratchpad/speed/Makefile` — `test-par` and `check-par` targets, `par.sh` added to `SOURCES`
- `scratchpad/par.sh` — the sharded runner (also at `scratchpad/speed/tests/par.sh`)
- `scratchpad/speed/tests/green.sh` — the green stamp (§6a)
- `scratchpad/speed/tests/changed.sh` — changed-block selection (§6b)
- `scratchpad/blockdeps.tsv` — the derived block → files table (115 rows)
- `scratchpad/speed/` — working copy with everything applied and green
- `scratchpad/myprof.txt` — per-block profile (116 rows, seconds)
- `scratchpad/sleeps.txt` — the 471-call sleep census
- `scratchpad/run.sh.prototype.diff` — the `run.sh` prototype diff *(contaminated:
  the source worktree was committed to by another agent while I worked, so this
  diff includes their changes; my edits are the 14 `# group:` lines, the
  `until_clients` helper, 10 converted sleep sites, and the 56 tick/bound sites)*
