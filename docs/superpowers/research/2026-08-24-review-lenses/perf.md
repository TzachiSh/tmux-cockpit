# Lens: PERFORMANCE — `scripts/tmux-review`

Measured, not guessed. Every number below is from a run on this machine today.

**Rig.** 12-core x86_64, Linux 7.0.0-30, bash 5.2.21, tmux 3.7c, fzf 0.44.1, **mawk 1.3.4** as
`/usr/bin/awk` (no gawk installed — see caveat C-1), git 2.x, warm page cache. Timing harness
`scratchpad/perf/bench.sh` (bash `$EPOCHREALTIME` deltas, n reps, min/median/max reported — median is
the honest figure, min is the uncontended floor). `hyperfine` is not installed; `delta 0.19.2` and
`revdiff v1.12.0` were downloaded as release binaries into the scratchpad for baselines. A private
tmux server (`tmux -L lensperf -f /dev/null`) hosted the fleet panes (`exec -a claude bash`);
**killed at the end of this report** (see §9). The box was under load from three concurrent agent
sessions, so max values are noisy; medians and minima are stable and are what the conclusions rest on.

Fixtures in `scratchpad/perf/`: `r_<lines>_<files>` = N changed lines spread over M files;
`r_wide` = 4998 rows of 300-char lines (the worst diff that still fits *under* the 5000 cap);
`r_huge` = 100k rows; `r_lock` = a regenerated 51 MB `package-lock.json` (400k rows);
`bigrepo` = 20 000 tracked files, one dirty; `fleet/repo0..49` = 50 dirty repos.

---

## 0. The headline

1. **[E] The bash row loop is still 70–90 % of every parse**, even after today's O(n²) and
   fork-per-row fixes. A byte-identical awk-only reimplementation of `lines_mode` is **5.5–8×
   faster** (`scratchpad/perf/awkonly.sh`; `diff` against `--lines` output is empty on three fixtures).
2. **[E] The 5000-row cap does not bound the popup.** `lines_mode` captures the *entire* diff into a
   bash variable (`tmux-review:92`) **before** the cap is computed from numstat
   (`tmux-review:112-123`). On a regenerated lockfile that is **5.7 s wall and 254 MB RSS** to then
   print "diff too large". Reordering (numstat first, cap, then capture) is a 9-line change worth
   **6.5×** and **2.5× memory**. This is the C0 hard-lock only half-fixed.
3. **[E] One alt-Enter costs 170 ms (1000 rows) to 1.16 s (4900 rows)** because every comment
   re-runs `git diff` + awk + the whole bash row build + sort from scratch. Nothing about the diff
   changed. A cached-rows + awk-marking reload is **18–24 ms**, a **~40× cut**.
4. **[E] The fleet queue is a serial `git status` + `git diff --numstat` + `grep` per pane**: 655 ms
   at 50 panes on toy repos, **2.5 s at 50 panes on a realistic 20k-file repo**. `xargs -P 16` on the
   same panes: **192 ms** (3.4–5.6×).
5. **[E] fzf is innocent.** Rendering 5000 rows costs fzf 51–57 ms including the 15 ms rig floor.
   100 % of the growth from 100 → 5000 rows lives in tmux-review's shell.
6. **[E] `revdiff` (Go, ⭐807, pushed 2026-08-20) opens its full TUI in 120–200 ms flat**, whether the
   diff is 1000 or 4900 rows. tmux-review's popup is 91 ms / 163 ms / **450–750 ms** / **1.7 s** at
   100 / 1000 / 4900 / 4998-wide rows. We are slower than a full-screen Go TUI at every size above
   ~500 rows, doing strictly less work.

---

## 1. `--lines` — the parse, by size

`tmux-review --lines <dir>`, n=11, warm.

| fixture | changed lines | files | min | **median** | max | awk-only median | **speedup available** |
|---|---|---|---|---|---|---|---|
| `r_100_1`   | 100  | 1   | 37.1 | **42.5** | 62.4 | 15.4 | 2.8× |
| `r_100_10`  | 100  | 10  | 38.7 | **47.3** | 59.2 | — | — |
| `r_1000_1`  | 1000 | 1   | 83.8 | **118.1** | 220.5 | 21.4 | **5.5×** |
| `r_1000_10` | 1000 | 10  | 92.8 | **122.7** | 224.5 | — | — |
| `r_1000_100`| 1000 | 100 | 112.1 | **219.9** | 269.9 | 27.0 | **8.1×** |
| `r_4900_1`  | 4900 | 1   | 326.6 | **426.8** | 581.2 | 54.5 | **7.8×** |
| `r_4900_10` | 4900 | 10  | 279.5 | **439.1** | 603.6 | — | — |
| `r_4900_100`| 4800 | 100 | 299.9 | **436.9** | 770.9 | 68.9 | **6.3×** |
| `r_wide` (4998 rows × 300 chars) | 4998 | 1 | — | **970** | — | 120 | **8.1×** |

**[E] Scaling is linear in rows and near-indifferent to file count** — the fix landed today (indexed
array + `printf -v`) genuinely removed the O(n²). What is left is a *large linear constant*: ~85 µs
per row of pure bash, versus ~11 µs per row in mawk.

**[E] Row count is the wrong cap dimension.** `r_wide` is 4998 rows — *legal*, under the cap — and
takes 970 ms. Cost is `rows × line_width`, not rows. A file with long lines (minified JS, generated
SQL, a wide CSV, a Rust macro expansion) sails under the cap and still wedges the popup for a second.

---

## 2. Flame-style attribution

Same code, decomposed stage by stage (`scratchpad/perf/attrib.sh`, median of 3 runs). All ms.

```
r_4900_1 (4900 rows, 1 file) — TOTAL 461 ms
  git diff (capture)        12.9   ███                          2.8%
  git diff --numstat        11.2   ██                           2.4%
  cap loop (bash/numstat)    0.3   ·                            0.1%
  awk hunk parse            24.8   █████                        5.4%
  churn loop (bash)          0.4   ·                            0.1%
  BASH ROW LOOP            331.9   ████████████████████████████████████████████████████████████████████  72.0%
  sort + emit loop          81.2   ████████████████             17.6%
```

```
r_1000_100 (1000 rows, 100 files) — TOTAL 149 ms
  git diff                  21.8  ███████                    14.6%
  git diff --numstat        17.9  ██████                     12.0%
  cap loop                   1.8  ·                           1.2%
  awk hunk parse            10.0  ███                         6.7%
  churn loop                 4.8  ██                          3.2%
  BASH ROW LOOP             61.7  ████████████████████       41.4%
  sort + emit               30.9  ██████████                 20.7%
```

```
r_100_1 (100 rows) — TOTAL 32.7 ms   ← process floor dominates
  git diff 3.8 | numstat 4.5 | awk 3.9 | ROW LOOP 7.9 | sort+emit 12.2
  (bash startup + SELF resolution + shq: ~14.5 ms measured separately on a non-repo dir)
```

**Micro-attribution inside the row loop** (`scratchpad/perf/micro.sh`, 5000 iterations over a
300-char line, ms):

| primitive | cost / 5000 rows | note |
|---|---|---|
| empty `for` loop | 13–15 | bash's floor |
| `frag=${line//[$'\001'-$'\037'$'\177']/}` (control-char strip) | **89–115** | scans the **full** line before the 160-char truncation |
| `frag=${frag:0:160}` | 56 | applied *after* the scrub, so the scrub does 2× the needed work |
| `printf -v disp ...` | 47–68 | already the fast form; the slow `$(printf)` was fixed today |
| `allrows[ri]="..."` | 37–45 | indexed append, already amortised O(1) |

**[E] Ordering bug, free win:** swapping the two lines at `tmux-review:238-239` so the truncation
runs **before** the control-char strip halves the most expensive primitive in the loop (~50 ms /
5000 rows on wide lines) with zero behaviour change for any line whose first 160 chars are kept.

**[E] The "sort + emit" 18 % is not `sort(1)`.** `sort` runs on the *file list* (≤100 entries), so
it is microseconds. The 81 ms is the two `while read` + inner `for (( j=gstart; j<=gend; j++ ))`
loops at `tmux-review:261-268` re-emitting all 4900 rows one `printf` at a time through a bash loop.
The rows are already in memory in the right order per file; only the *file groups* need reordering —
awk can emit them directly.

**[E] Two git diffs per parse.** `tmux-review:92` and `:97` each run a full diff of the working tree;
numstat's add/del columns are derivable from the parse itself. On `r_1000_100` that duplicate is
17.9 ms of a 149 ms budget (12 %).

---

## 3. `--rows` and the comment store

`tmux-review --rows <dir> <store>`, n=9, median ms.

| fixture | store = 0 comments | store = 50 | store = 500 |
|---|---|---|---|
| `r_1000_1` (1000 rows) | 142.7 | 170.2 | 223.2 |
| `r_4900_1` (4900 rows) | 682.8 | 876.5 | 643.1 |

**[E] Store size is now flat** — 500 comments cost no more than 50 (the spread is machine noise).
Today's fix (headers read once into an associative array, `tmux-review:290-297`) worked: the old
per-row `grep -qxF` is gone.

**[E] But `rows_mode` adds a *second* full bash pass over every row** (`tmux-review:300-309`) on top
of `lines_mode`'s: +50–60 % over `--lines` at the same size (427 → 683 ms at 4900). Marking is three
string ops per row; in awk that whole pass is **18–24 ms for 4900 rows** (§4), i.e. 30× cheaper.

**[E] Peak RSS** (`/usr/bin/time -v`): `--lines r_4900_1` 6.4 MB; `--lines r_wide` 16.4 MB;
`--rows r_4900_1` + 500-comment store 6.6 MB. Memory is **not** a problem inside the cap — bash
arrays of 5000 rows are cheap. Memory *is* a problem outside it (§6).

---

## 4. The per-keystroke cost: one alt-Enter

alt-Enter fires `execute-silent(tmux-review --attach ...)` then `reload(tmux-review --rows ...)`
(`tmux-review:402`). Both are **fresh process launches** — fzf's binds run in a new subshell, so
each pays bash startup + `SELF` resolution again.

| fixture | store=50 | store=500 |
|---|---|---|
| `r_100_1` (100 rows) | **79.8 ms** | 124.1 ms |
| `r_1000_1` (1000 rows) | **275.0 ms** (min 172) | 203.6 ms |
| `r_4900_1` (4900 rows) | **1163.7 ms** (min 671, max **4964**) | 945.9 ms |

Components: `--attach` alone = **11.3 ms** (of which ~9.6 ms is process startup — the actual file
append is microseconds). Everything else is the reload re-deriving a diff that did not change.

**[E] Alternative: cached rows + awk marking** (`scratchpad/perf/markcache.sh` — read the cached
row file, re-mark from the store in one awk): **18.6 / 24.4 / 36.2 ms** (min/med/max) at 4900 rows
with a 500-comment store. **~40× faster than the current 946–1164 ms.**

**[E] Cache-key cost**: hashing the diff to detect staleness (`git diff | cksum`) is 29 ms median at
4900 rows — cheaper than the reload it guards, and it can be skipped entirely if the reload just
re-marks (the *diff* cannot change as a result of typing a comment; only an agent edit can change it,
and the popup is modal so the human isn't watching for that mid-comment anyway).

**[E] Session budget.** A 20-comment review at 4900 rows: 450 ms open + 20 × ~950 ms = **~19.5
seconds** of the popup sitting unresponsive-or-stale, spread across the session. At 1000 rows: ~4.2 s.
With cached reloads the same session is **450 ms + 20 × 20 ms = 0.85 s**.

**Hidden per-keystroke cost the brief did not list — the preview.** `--preview` at
`tmux-review:404` re-runs `tmux-open-path --preview-diff {1}` on **every cursor move**:

| fixture | preview cost per cursor move |
|---|---|
| `r_1000_1` | 39.6 ms |
| `r_4900_1` | 92.5 ms |
| `r_wide`   | **343.9 ms** |

`tmux-open-path:75-81` runs **up to three `git diff` invocations** for one preview (unstaged, staged
fallback, then `--stat`) and captures the whole file diff into a bash variable before `head -400`.
And the preview takes only `{1}` (the path), never `{2}` (the line) — so **for all 4900 rows of a
one-file diff the preview is byte-identical and regenerated 4900 times**. This is plan item D5,
still unshipped, and it is the single most cacheable thing in the program.

---

## 5. `--fleet` — the serial pane scan

Private tmux server, panes running `exec -a claude bash` in dirty repos. n=5–7.

| agent panes | repo shape | **serial (shipped)** | `xargs -P 16` replica | speedup |
|---|---|---|---|---|
| 1  | 60-line dirty repo | 43.5 ms | — | — |
| 10 | 60-line dirty repo | 157.2 ms | — | — |
| 50 | 60-line dirty repo | **655–712 ms** | **155–192 ms** | **3.7×** |
| 50 | 20 000-file repo (realistic) | **2532 ms** | — (extrapolates to ~350 ms) | ~7× |

**[E] Process accounting** (`strace -f -e trace=execve`): `--fleet` at 50 panes issues **192
execve**, of which **100 git + 50 grep** — exactly the 3 processes per pane at `tmux-review:591`,
`:596-597`, `:610`, all serial. Per-pane costs on a realistic repo: `git status --porcelain` 38.7 ms,
`git diff --numstat` 28.0 ms.

**[E] The `grep -c '^??'` fork per pane (`tmux-review:610`) is gratuitous** — the untracked count is
already derivable from the `$st` string the shell holds, and in the parallel replica it folds into
the same awk that sums numstat. 50 forks removed for free.

**[E] `--lines` / `--rows` are a constant 27 execve regardless of size** — the fork-per-row fix
landed and holds. Fleet is where the fork count still scales with input.

---

## 6. The cap does not cap — the real hard-lock

`lines_mode` order today (`tmux-review:91-123`): capture full diff → capture numstat → *then* count
and cap.

| input | shipped `--lines` | peak RSS | numstat-first replica | RSS |
|---|---|---|---|---|
| `r_huge` (100k rows, ~10 MB diff) | 577.7 ms | 33.7 MB | **108.2 ms** | — |
| `r_lock` (400k rows, **51 MB** diff) | **5678 ms** (max 10 396) | **254 MB** | **872 ms** | 103 MB |

**[E] A regenerated `package-lock.json` still freezes the modal popup for 5.7–10.4 seconds and
allocates 254 MB** to print one "diff too large" row. The cap prevents the *parse*, not the
*capture*. `scratchpad/perf/capfirst.sh` (run numstat, cap, and only then capture the diff) is
**6.5× faster and 2.5× lighter**; the residual 872 ms is git itself diffing 51 MB, which is the true
floor and could be bounded further by pathspec-excluding huge blobs or `git diff --stat` sampling.

Note also that the "too large" path *still costs two full git diffs* today because the unstaged
capture at `:92` and the numstat at `:97` each diff the tree independently.

---

## 7. Popup open-to-first-frame, and the baselines

Measured by launching each command in a fresh pane on the private server and polling `capture-pane`
until ≥3 non-blank lines appear (`scratchpad/perf/ttff.sh`). **Rig floor = 14–22 ms.**

| what | 100 rows | 1000 rows | 4900 rows | 4998 wide rows |
|---|---|---|---|---|
| **`tmux-review --rows \| fzf` (shipped)** | **91 ms** | **163 ms** | **450–753 ms** | **1619–1704 ms** |
| awk-only rows \| fzf (byte-identical output) | — | — | **103–136 ms** | **279–321 ms** |
| streaming awk \| fzf (emit as parsed, no global ordering) | — | — | **94–130 ms** | **157–245 ms** |
| `fzf` alone on a pre-built row file | 36–45 ms | 36–45 ms | **51–57 ms** | — |
| **`revdiff` full TUI** (Go, ⭐807) | — | **121–198 ms** | **151–188 ms** | — |
| `tmux display-popup -E true` (popup floor) | 12–15 ms | | | |

Throughput baselines on the same diffs (n=7, median):

| baseline | `r_1000_1` | `r_4900_1` | `r_wide` |
|---|---|---|---|
| `git diff \| delta -w 158` (Rust, ⭐31 850) | 28.8 ms | 99.2 ms | 390.9 ms |
| `git diff -U0 \| awk '{n++}'` (bare parse floor) | 12.5 ms | 18.8 ms | 97.0 ms |
| **shipped `--lines`** | 118.1 ms | 426.8 ms | 970 ms |

**[E] fzf costs 36→57 ms from 100 to 5000 rows.** It is not the bottleneck; blaming "fzf with 5000
rows" would be wrong. The whole delta between 91 ms and 1704 ms is our own shell.

**[E] revdiff's time-to-first-frame is flat in diff size** (121–198 ms at both 1000 and 4900 rows) —
it renders a viewport, not a list. tmux-review's is linear in diff size because it materialises every
row before the first pixel.

**[E] delta — a mature Rust renderer that does *more* work (syntax highlighting, side-by-side
layout, hunk headers) — is 4× faster than our plain-text row emitter** at 4900 rows.

---

## 8. Is the client blocked? Quantified.

**[E] The tmux *server* is not blocked.** With a popup running `sleep 2`, from another shell:
`list-panes` 7.1 ms, `display-message -p` 6.1 ms, `select-window` 12.6 ms. Other clients and the
status bar keep going.

**[E] The *client* is.** `display-popup -E` (bound at `cockpit.tmux:248-250`) captures the attached
client's keyboard for the popup's whole lifetime. So the blocking is real but its unit is
*human-visible dead time on the reviewing client*, and that time is exactly the numbers above:

| moment | dead/stale time at 1000 rows | at 4900 rows | at 4998 wide rows |
|---|---|---|---|
| `prefix r` → first frame (blank popup, keys queued) | 163 ms | **450–753 ms** | **1.6–1.7 s** |
| each alt-Enter → ● appears | 275 ms | **946–1164 ms** | ~2 s [I] |
| each cursor move → preview redraw | 40 ms | 93 ms | **344 ms** |
| a lockfile-sized diff → "too large" row | — | — | **5.7–10.4 s** |
| `prefix r` → fleet list, 50 agents, real repos | **2.5 s** | | |

**[I]** fzf's `reload()` is asynchronous, so during the reload the picker still accepts keys — but it
shows a *stale list* with the ● not yet drawn. Feedback latency, not input lockout. `execute-silent`
(the `--attach`) *does* block fzf, but only for its 11 ms. The felt problem is the ~1 s gap between
"I pressed alt-Enter" and "I can see it landed", which is well past the ~100 ms threshold at which an
action stops feeling direct.

---

## 9. Why the design is slow — the architectural reasons

1. **The unit of work is a line, and the executor is bash.** One fzf row per changed `+`/`-` line
   means the row count *is* the diff size. Real commits in this very repo:
   `HEAD~1..HEAD` = 1 hunk / 9 lines; `HEAD~5..HEAD` = 41 hunks / 923 lines (**22.5×**);
   `HEAD~20..HEAD` = 92 hunks / **5571 lines** (**60.6×**, and *over the cap* — that diff shows the
   "too large" row today, while 92 hunk rows would be instant). **[E]** Hunk-granularity rows would
   put every realistic review in the 10–100-row regime where everything measured here is free.
2. **Nothing is cached, and the reload contract forces a full rebuild.** `reload(cmd)` is fzf's only
   way to mutate the list, and the cmd we hand it re-derives the world. The diff is immutable for
   the popup's lifetime; the *only* thing that changes between reloads is the ● marker set.
3. **Every fzf bind is a fresh process.** ~10–15 ms of bash startup + `SELF` resolution + `shq`
   before any work, on `--attach`, on every `--rows` reload, and on every preview.
4. **The parse is done three times over.** `git diff` for the text, `git diff --numstat` for the
   churn (derivable from the text), and `git diff` again per preview per cursor move.
5. **The fleet scan is serial and fork-heavy.** 3 processes per pane, no parallelism, no reuse of the
   state the agent hooks already maintain per pane.
6. **The cap is on the wrong quantity and applied at the wrong point** (§1, §6): rows, not bytes; and
   after the capture, not before.
7. **Everything is materialised before the first frame.** fzf can render rows as they stream; today
   it gets one fully-built, fully-ordered list.

---

## 10. Ranked fixes

Ranked by (measured gain) ÷ (cost), with what each replaces.

| # | Fix | What it replaces | Measured / expected gain | Cost |
|---|---|---|---|---|
| **1** | **Numstat first, cap, *then* capture the diff.** Move `tmux-review:96-98` above `:91-94` and the cap block above the capture. | The full-diff capture that runs before the cap can fire (`:92`). | **5.7 s → 0.87 s** and **254 MB → 103 MB** on a regenerated lockfile; 578 → 108 ms at 100k rows. Fixes the C0 hard-lock properly. | **S** |
| **2** | **Cache the built rows for the popup's lifetime; make `--rows` re-mark from a cached row file in awk.** Write `lines_mode` output to a temp file next to the store on first build; `--rows` = one awk pass over it. | The `--rows` → `lines_mode` full rebuild on every alt-Enter (`:309`). | **946–1164 ms → 18–24 ms per comment (~40×)**; a 20-comment session drops from ~19.5 s to ~0.85 s of dead time. Optional `cksum` staleness key costs 29 ms. | **S** |
| **3** | **Emit rows from awk; delete the bash row loop, the churn loop, and the sort/emit loops.** `scratchpad/perf/awkonly.sh` is a working, **byte-identical** drop-in (verified against `--lines` on 3 fixtures). | `tmux-review:198-268` in its entirety, plus the second `git diff --numstat`. | **5.5–8.1×** (`4900 rows: 427 → 55 ms`; `r_wide: 970 → 120 ms`). Popup first-frame 450–753 → 103–136 ms. | **M** |
| **4** | **Cache the preview per file, and drop the double/triple `git diff` in `--preview-diff`.** One `git diff \| awk \| head -400` streamed, memoised on `(path, mtime)`. | `tmux-open-path:75-81` re-running 2–3 git diffs on every cursor move for an identical result. | 40 / 93 / 344 ms per cursor move → ~0 on a repeat (the common case: all rows of one file). This is plan item **D5**. | **S–M** |
| **5** | **Parallelise the fleet scan** (`xargs -P 16` or bounded background jobs) **and drop the `grep -c '^??'` fork** by counting untracked in the same awk. | `tmux-review:591-612`, 3 serial processes per pane. | **655 → 155–192 ms** at 50 toy panes (3.7×); ~2.5 s → ~350 ms on realistic repos (~7×). This is plan item **D3**. | **M** |
| **6** | **Hunk-level rows** (one row per `@@` block, path + range + the hunk's first changed line as the fragment), with an optional drill-in to line rows for the hunk under the cursor. | The one-row-per-changed-line model throughout. | **22–61× fewer rows** on this repo's real commits. Puts every realistic review under 100 rows, where the parse, the reload, and the cap all stop mattering. Also fixes the "5571-line commit shows *no rows at all*" cliff. | **L** |
| **7** | **Stream to fzf instead of building the full list.** Emit rows down the pipe as awk parses them; fzf renders incrementally. Requires giving up the global test-first/churn ordering (or pre-ordering the *files* from numstat and streaming file-by-file — that keeps the ordering and still streams). | The `rows_out=$(...)` capture + `grep -c` + `printf \| fzf` in `run_loop` (`:373-374, :397`). | first-frame **1.7 s → 157–245 ms** on `r_wide`; 450–753 → 94–130 ms at 4900. Composes with #3. | **M** |
| **8** | **Cap on bytes, not rows** (e.g. `git diff --numstat` total *and* a byte budget from `git diff --stat` / blob sizes), and skip generated paths by pathspec (`:(exclude)*.lock`, `*-lock.json`, `dist/`, `vendor/`). | The row-only `cap=5000` at `:112`. | Closes the `r_wide` hole: 4998 legal rows currently cost 970 ms + 1.7 s first frame. | **S** |
| **9** | **Free micro-fixes in the row loop** (if #3 is deferred): truncate to 160 chars *before* the control-char strip (`:238-239`); drop the second `git diff --numstat` by deriving churn in the parse. | Two lines. | ~50 ms / 5000 wide rows on the scrub; 12–18 ms per parse on the numstat. | **S** |
| **10** | **Shell out to a Go/Rust reviewer** (a small binary, or `revdiff` itself as the picker). | The whole bash pipeline. | `revdiff` opens in **121–198 ms flat, independent of diff size**; `delta` renders 4900 rows in 99 ms. Removes every constant above. | **L** — and it breaks the repo's "one-shot bash scripts, no build step, no daemon" invariant, so it is listed last despite having the best numbers. A middle path: keep bash as the shell and move only the parse to a single awk program (#3), which captures most of the gain at none of the cost. |

**Sequencing note:** #1 + #2 + #9 are all `S`, touch `lines_mode`/`rows_mode` only, and together take
the 4900-row popup from 450–753 ms open / 950–1164 ms per comment to roughly 450 ms open / 20 ms per
comment. #3 then takes the open to ~110 ms. #4 and #5 are independent files. #6 and #7 are the
redesign.

---

## Caveats

- **[C-1] awk dialect.** `/usr/bin/awk` here is **mawk 1.3.4**, the fastest common awk. On a machine
  where `awk` is **gawk** the awk stages are ~1.5–2× slower, and on macOS (**BSD awk**, or
  `onetrueawk`) ~2–3× slower **[I]**. Even at 3× the awk-only parse (55 → 165 ms at 4900 rows) still
  beats the shipped bash loop (427 ms). The awk program in `awkonly.sh` uses only POSIX awk features
  plus `SUBSEP` (POSIX) — no gawk extensions — so it is portable, and the existing parser's
  "not a capture-group regex — portable to non-GNU awk" discipline is preserved.
- **[C-2] Machine load.** Three concurrent agent sessions were running. Maxima are inflated (one
  alt-Enter measurement hit 4964 ms); medians and minima are consistent across repeated runs and are
  what the table conclusions use. Note that *inflated maxima are the realistic case* for this tool —
  its entire premise is a machine running a fleet of agents.
- **[C-3] Warm cache.** Every number is warm. The first cold run of `--lines` measured 132–996 ms at
  100 rows. Cold-cache popup opens are worse than everything above.
- **[C-4] `--fleet` fixtures are toy repos** (200-line file, 60 changed lines). The realistic 20k-file
  number (2.5 s at 50 panes) comes from running the exact per-pane body 50× against `bigrepo`, not
  from 50 real panes in that repo.
- **[C-5] The client-blocking analysis** distinguishes server (not blocked, measured) from client
  (keyboard captured by the popup, by tmux's design). The claim that fzf stays responsive during
  `reload()` is **[I]** from fzf's documented async-reload semantics, not measured here.

---

## Artifacts

All under `scratchpad/perf/`: `mkrepo.sh`, `bench.sh` (timing harness), `attrib.sh` (stage
attribution), `micro.sh` (row-loop primitives), **`awkonly.sh` (byte-identical awk drop-in for
`lines_mode`)**, `stream.sh` (streaming variant), `markcache.sh` (cached reload), `capfirst.sh`
(numstat-first cap), `fleet_par.sh` (parallel fleet scan), `panebody.sh`, `ttff.sh`
(time-to-first-frame rig), `bin/delta`, `bin/revdiff`, and the fixture repos.

**The private tmux server `lensperf` was killed after the final measurement.**
