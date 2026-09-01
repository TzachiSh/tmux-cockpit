# r3-75: remedy options per flake class, with prices

Lane r3-75, issue #75. Line numbers below are master at a77f882; the numbers in the issue comments have drifted.

## Measurements (this box: 12 cores, tmux 3.7c, load 2.0 at rest)

| what | idle | under load |
|---|---|---|
| `tmux-review --lines` on 4900 rows (run.sh:4410, budget 0.2 s) | wall 142-163 ms, n=7 | wall 428-632 ms, n=7, 24 spinners |
| same, CPU (user+sys) | 168-197 ms | 225-249 ms |
| `tmux-branch` 5000 branches (run.sh:1653, budget 2 s) | wall 346-618 ms, n=5 | not sampled |
| `--lines` 800 untracked (run.sh:3841, budget 1 s) | wall 228-272 ms, n=5 | not sampled |
| `--preview-diff` 400k lines (run.sh:2884, budget 2 s) | wall 0.55 s, maxRSS 75.7 MB, n=1 cold | not sampled |

Two facts drive everything. First, **the 4900-row budget has 1.23-1.4x headroom, not the 3x its own comment claims** (run.sh:4386 says "the rig's convention, ~3x the expected time"). The comment's 59-74 ms was the inner row build; `timeout` sees the whole script. That is why this assertion is the most-reported one in #75. Second, **wall time inflated 3.9x under load while CPU time inflated 1.27x.** CPU time is the instrument that survives peers.

## Class (a): wall-clock perf budgets

**a1. CPU time instead of wall time.** `TIMEFORMAT='%U %S'; { time cmd; } 2>&1 | awk '{print ($1+$2 < B)}'`. Bash's `time` sums waited-for descendants, confirmed above (CPU 0.168 exceeded wall 0.143 on a pipeline). Diff: about 4 lines per site, 4 sites. Still catches the regression: yes for every one of these, because each pre-fix build burned CPU (fork per row, fork per branch, O(n^2) append, per-file fork walk), not wall time waiting. At load 15+: the 1.27x inflation I measured means a 3x margin holds. Pristine box: unchanged. Caveat: CPU is blind to a regression that adds *waiting*; none of these four are that shape.

**a2. Pin the count, not the clock.** The repo already does this three times: run.sh:8385 ("Counting curl, not timing it... a fork count, never a clock"), run.sh:7154 ("one fork per pane and five per dirty root"), run.sh:508 (PATH shim logging one line per call). Diff: 10-20 lines per site. Fits `800 untracked files` cleanly (the regression is four `git` forks per file, and a PATH shim over `git` counts them). **Rejected for 5000 branches and 4900 rows**: the regression there was `$(printf ...)`, a subshell fork of a bash *builtin*. No execve happens, so no PATH shim can see it and there is no portable fork counter. This is the strongest rule-honoring candidate but it only reaches one of the four sites.

**a3. Raise the budget.** Diff: one digit. Still catches: 0.2 already "is not a proof" by its own comment (run.sh:4394, rejects the bash build 2 runs in 3); doubling it makes it decorative. At load 15+: still red, because the inflation is unbounded, only less likely. Reject as a solution, accept only as an interim.

**a4. maxRSS instead of time, for the 400k-diff site only.** `/usr/bin/time -f %M`, assert under about 120000 kB. Measured 75.7 MB now against the 191 MB the pre-fix build used (run.sh:2871). Load-independent, 2.5x separation. Diff: 3 lines. Cost: GNU `time` is not POSIX, so it needs a `skip_` fallback like the suite's `need_tmux` blocks.

**Rejected: scaling by /proc/loadavg or nproc.** It reintroduces the clock and adds a second thing to get wrong; a budget that moves is a budget nobody can reason about, and loadavg lags by a minute so it under-reports a spike that just started.

**Rejected: a separate serial gate with a loadavg floor.** `# group: serial` already exists (par.sh:102-113) and already runs clock blocks alone at J=1. It does nothing for *peer* load, which is #75. A refusal floor turns a flake into a skip, so the gate stops gating exactly when the box is busy, which is when people push. Worth noting separately: **`5000 branches`, `800 untracked` and the 400k-diff block carry no `# group:` marker at all**, so each runs in a parallel shard today and contends with the suite itself. Adding `# group: serial` above run.sh:1622, run.sh:2865 and run.sh:3822 is one line each and is worth doing whichever remedy wins.

## Class: the 0.15 s timeout that cascades

Site run.sh:4479, `timeout 0.15 ... > "$STATE/rows.cached"`. On timeout the file is empty and run.sh:4494 (`cmp` cached against fresh) and run.sh:4495 (`500 rows are marked`) fail from it. Three reds, one cause.

**c1. Demote to a hang guard, keep the sentinel.** `timeout 10`, drop the timing claim. Precedent is already in the file: run.sh:5748 records `timeout 0.05` becoming `timeout 5` plus a planted cache sentinel, same failure mode, issue #36. This block already has its sentinel at run.sh:4486, and its own comment (run.sh:4483) admits 0.15 "no longer separates cached from uncached, only awk marking from bash marking". Diff: one digit plus 3 comment lines. Catches the regression: yes, the sentinel is the real proof. Load 15+: green. Pristine: unchanged. **Take it.**

**c2. Guard dependents on the exit code so they skip.** About 6 lines. Turns three reds into one red plus two skips. Honest, but leaves the false red. Good hygiene rule, not the fix.

**Rejected: retry once.** Halves the false failures and the true detections together, and hides the cascade instead of removing it.

## Class: fixed sleep then capture-pane

Site run.sh:1989, `sleep 2` then `capture-pane` asserting `● main`.

**b1. `until_cap`,** which exists at run.sh:105: `until_cap "$SOCK5" "$KP" '*● main*' || bad ...`. Diff: 2 lines, minus the sleep. Catches the regression: yes, an unmarked list never matches inside 4 s. Load 15+: green. Pristine: faster than today. Sweep every `sleep N; capture-pane` pair, not just this one.

**b2. Longer sleep.** One digit, still a clock, still red at some load, slower on a pristine box. Reject.

## Class: mouse double-click plus geometry

Site run.sh:2081-2089. Two independent causes, both need fixing.

**d1. One `send-keys`, not two.** Measured: tmux 3.7c registers a double click at a 280 ms gap 3/3 and at 350 ms 0/3. The current two-call form spans 77-106 ms at load 8 (n=10, 10/10 registered), so about 3x margin, same order as the wall budgets; I did not reproduce it red. `send-keys -t "$OP" -H $CDBL $CDBL` writes all four events at once, gap in microseconds, load-independent. Verified 5/5 idle and 5/5 at load 6.4. Diff: one line merged, one sleep deleted.

**d2. Pin the pane content, drop the shell.** The #87 failures were at load 2-3, so this is the likelier cause. The inner session inherits the runner's cwd and a real prompt; in 80 columns from a long cwd the prompt line truncates at `...timing-flakes-75$ ech`, so README.md is not on the clicked row, and row 1's content is a race between the tty echo and bash's first prompt. Replace with `new-session -d -s t -x 80 -y 24 'printf "MARK README.md\n"; exec sleep 300'` plus `until_cap` before the click. Verified row 1 is exactly `MARK README.md`. Diff: about 4 lines, deletes `sleep 0.4` and `sleep 1.5`.

## Decision list

1. **CPU budgets (a1) or counts and sentinels?** Default: CPU at all four sites; counts reach only one. Needs an explicit rule amendment that CPU time is not a clock.
2. **What margin?** Default 3x idle CPU max, so 0.6 s at run.sh:4410 against 197 ms measured.
3. **maxRSS for the 400k-diff site?** Default yes, with `skip_` when `/usr/bin/time` is absent: 191 MB to 76 MB, load-independent.
4. **0.15 s becomes a hang guard?** Default yes, per run.sh:5748.
5. **Sweep or point fix for the sleeps?** Default sweep, plus the three missing `# group: serial` markers.

## Addendum: the two sites the issue title names

**1. run.sh:5546 `the store itself is gone`.** Line 5545 polls `delivered.md` with `until_file`, then 5546 bare-tests `$RASESS/comments.md`. Deliver writes the archive and unlinks the store; nothing makes those atomic, so the poll can return between them and the store is still present.

Remedy: poll the store path instead of testing it once. The idiom already exists ten lines down at run.sh:5556, `for i in $(seq 1 40); do [ -e "$RASESS" ] || break; sleep 0.1; done`, so copy it, no new helper. Price: 1 line added. Catches the regression: yes, a store that is never removed is still absent after 4 s and still fails. Load 15+: green. Pristine: no measurable cost.

**2. run.sh:3588 `two blocked and one errored, across sessions`.** Diagnosed as far as read-only allows, and the location is pinned. Every setup call at run.sh:3570-3576 sends stderr to `/dev/null`; run.sh:3579, `FCP3=$(... list-panes -t b:1 ...)`, does not. So `can't find window: 1` came from 3579, and it means session `b` existed while window `b:1` did not. The `new-window -t b` at 3572 therefore failed or its window was gone by 3579, and its error was swallowed. `FCP3` is then empty, `asFC error ""` at 3585 does nothing, and the count reads short.

Why the `new-window` failed I cannot pin read-only, and I did not reproduce it. Cheapest instrumentation for a fix lane: drop `2>/dev/null` from 3570-3576 and let the setup speak.

Remedy, which is also the fix: pin the setup as a count before driving state. After 3579, `is "three agent panes exist" "3" "$(list-panes -a -F '#{pane_id}' | grep -c .)"`. Price: about 4 lines with the redirects removed. Catches the regression: yes, run.sh:3588 is untouched. Load 15+: a silent setup failure becomes an honest red at its own line instead of a wrong fleet count plus a cascade. Pristine: unchanged. This is the rule already written at run.sh:8385, count the thing, never time it.
