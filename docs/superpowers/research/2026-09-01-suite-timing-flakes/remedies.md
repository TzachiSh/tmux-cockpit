# r3-75: remedy options per flake class, with prices

Lane r3-75, issue #75. Line numbers below are master at 6c6424a; the numbers in the issue comments have drifted.

## Measurements (this box: 12 cores, tmux 3.7c, load 2.0 at rest)

| what | idle | under load |
|---|---|---|
| `tmux-review --lines` on 4900 rows (run.sh:4410, budget 0.2 s) | wall 150 ms, n=7 | wall 427 ms at 12 spinners, 653 ms at 24 (4.35x) |
| same, CPU (user+sys), bare command | 111 ms (range 93-135) | 103 ms at 12 spinners, 112 ms at 24 (1.01x) |
| `tmux-branch` 5000 branches (run.sh:1653, budget 2 s) | wall 346-618 ms, n=5 | not sampled |
| `--lines` 800 untracked (run.sh:3841, budget 1 s) | wall 228-272 ms, n=5 | not sampled |
| `--preview-diff` 400k lines (run.sh:2885, budget 2 s) | wall 0.55 s, maxRSS 75.7 MB, n=1 cold | not sampled |
| `tmux-open-path --dry-run` on a 4000-token line (run.sh:1216, budget 0.6 s) | not measured | not sampled |

(Corrected after the refute pass: the first draft's 168-197 ms idle CPU was the `| grep -c .` pipeline form, not the bare command the suite times.)

Two facts drive everything. First, **the 4900-row budget has 1.23-1.4x headroom, not the 3x its own comment claims** (run.sh:4386 says "the rig's convention, ~3x the expected time"). The comment's 59-74 ms was the inner row build; `timeout` sees the whole script. That is why this assertion is the most-reported one in #75. Second, **wall time inflated 4.35x under load while CPU time did not move at all: 111 ms idle, 112 ms at 24 spinners, 1.01x.** CPU is not merely more stable than wall time here, it is flat, which is a stronger case for a1 than the first draft made. And the budget does not need a stress rig to redden: three spinners, a quarter of `nproc`, already fail it 1/10 with a 0.175 s wall mean; 12 spinners fail 10/10. A single peer build is enough.

## Class (a): wall-clock perf budgets

**a1. CPU time instead of wall time.** `TIMEFORMAT='%U %S'; { time cmd; } 2>&1 | awk '{print ($1+$2 < B)}'`. Bash's `time` sums waited-for descendants, confirmed above (CPU 0.168 exceeded wall 0.143 on a pipeline). Diff: about 4 lines per site, 5 sites. Still catches the regression: yes for every one of these, because each pre-fix build burned CPU (fork per row, fork per branch, O(n^2) append, per-file fork walk, uncapped per-token loop), not wall time waiting. At load 15+: CPU did not move at all, 111 ms idle against 112 ms at 24 spinners (1.01x), so a 3x margin over the idle CPU maximum holds under any load this box can reach. Pristine box: unchanged. Caveat: CPU is blind to a regression that adds *waiting*; none of these five are that shape.

The fifth site is **run.sh:1219**, "a 4000-token clicked line is bounded, not just slow" (`timeout 0.6` at 1216, red once in the issue body). It is a wall budget of exactly the same shape as the other four and joins a1 on the same terms; nothing new was measured for it here, so a fix lane must take its own idle CPU reading before picking its budget.

**a2. Pin the count, not the clock.** The repo already does this three times: run.sh:8385 ("Counting curl, not timing it... a fork count, never a clock"), run.sh:7154 ("one fork per pane and five per dirty root"), run.sh:508 (PATH shim logging one line per call). Diff: 10-20 lines per site. Fits `800 untracked files` cleanly (the regression is four `git` forks per file, and a PATH shim over `git` counts them). **Rejected for 5000 branches and 4900 rows**: for 5000 branches the regression was `$(printf ...)`, a subshell fork of a bash builtin (commit 50eb7ed). For 4900 rows the fix commit 30ba10f removed `$(( ))` arithmetic and bash loop passes over the rows; the code already used `printf -v`, so there was never a fork here at all, only interpreter passes. The rejection holds a fortiori: no execve either way, so no PATH shim can count anything, and there is no portable fork counter. This is the strongest rule-honoring candidate but it only reaches one of the five sites.

**a3. Raise the budget.** Diff: one digit. Still catches: 0.2 already "is not a proof" by its own comment (run.sh:4394, rejects the bash build 2 runs in 3); doubling it makes it decorative. At load 15+: still red, because the inflation is unbounded, only less likely. Reject as a solution, accept only as an interim.

**a4. maxRSS instead of time, for the 400k-diff site only.** `/usr/bin/time -f %M`, assert under about 120000 kB. Measured 75.7 MB now against the 191 MB the pre-fix build used (run.sh:2871). Load-independent, 2.5x separation. Diff: 3 lines. Cost: GNU `time` is not POSIX, so it needs a `skip_` fallback like the suite's `need_tmux` blocks.

**Rejected: scaling by /proc/loadavg or nproc.** It reintroduces the clock and adds a second thing to get wrong; a budget that moves is a budget nobody can reason about, and loadavg lags by a minute so it under-reports a spike that just started.

**Rejected: a separate serial gate with a loadavg floor.** `# group: serial` already exists (par.sh:102-113) and already runs clock blocks alone at J=1. It does nothing for *peer* load, which is #75. A refusal floor turns a flake into a skip, so the gate stops gating exactly when the box is busy, which is when people push. Worth noting separately: **`5000 branches`, `800 untracked` and the 400k-diff block carry no `# group:` marker at all**, so each runs in a parallel shard today and contends with the suite itself. Adding `# group: serial` above run.sh:1622, run.sh:2828 and run.sh:3822 is one line each and is worth doing whichever remedy wins. (A marker must sit directly above the block title `par.sh` splits on: 2828 is the title of the 400k-diff block, while 2865 is mid-block and would group the block after it and leave the 400k one bare.)

## Class: the 0.15 s timeout that cascades

Site run.sh:4479, `timeout 0.15 ... > "$STATE/rows.cached"`. On timeout the file is empty and run.sh:4496 (`cmp` cached against fresh) and run.sh:4497 (`500 rows are marked`) fail from it. Two reds, one cause. The sentinel assertion at 4489-4494 re-runs `--rows` and greps stdout, never reading `rows.cached`, so it cannot cascade.

**c1. Demote to a hang guard, keep the sentinel.** `timeout 10`, drop the timing claim. Precedent is already in the file: run.sh:5748 records `timeout 0.05` becoming `timeout 5` plus a planted cache sentinel, same failure mode, issue #36. This block already has its sentinel at run.sh:4488, and its own comment (run.sh:4482) admits 0.15 "no longer separates cached from uncached, only awk marking from bash marking". Diff: one digit plus 3 comment lines. Catches the regression: yes, the sentinel is the real proof. Load 15+: green. Pristine: unchanged. **Take it.**

**c2. Guard dependents on the exit code so they skip.** About 6 lines. Turns two reds into one red plus one skip. Honest, but leaves the false red. Good hygiene rule, not the fix.

**Rejected: retry once.** Halves the false failures and the true detections together, and hides the cascade instead of removing it.

## Class: fixed sleep then capture-pane

Site run.sh:1988, `sleep 2` then `capture-pane` asserting `● main`.

**b1. `until_cap`,** which exists at run.sh:105: `until_cap "$SOCK5" "$KP" '*● main*' || bad ...`. Diff: 2 lines, minus the sleep. Catches the regression: yes, an unmarked list never matches inside 4 s. Load 15+: green. Pristine: faster than today. Sweep every `sleep N; capture-pane` pair, not just this one.

**b2. Longer sleep.** One digit, still a clock, still red at some load, slower on a pristine box. Reject.

## Class: mouse double-click plus geometry

Site run.sh:2081-2089. Two independent causes, both need fixing.

**d1. One `send-keys`, not two.** Measured: tmux 3.7c registers a double click at a 280 ms gap 3/3 and at 350 ms 0/3. The current two-call form spans 77-106 ms at load 8 (n=10, 10/10 registered), so about 3x margin. The pass rates do not distinguish the forms: the single-call `send-keys -t "$OP" -H $CDBL $CDBL` ran 5/5 and the current two-call form also ran 5/5 under 24 spinners. Neither could be turned red, so merging the two calls is hardening against a failure never reproduced in the rig, not a demonstrated fix. Price is still one line merged, one sleep deleted; take it on the mechanism (gap in microseconds, load-independent), not on evidence.

**d2. Pin the pane content, drop the shell.** The #87 failures were at load 2-3, so this is the likelier cause. The long cwd and the echo/prompt race are one contingency, not two independent facts: the inner session inherits the runner's cwd and a real prompt, and when the tty echo wins the race, row 0 is a bare `echo MARK README.md` and the click succeeds 5/5 even from a long cwd. Truncation bites only when the prompt draws first, and it truncates at `...flakes-7` in 80 columns, pushing README.md off the clicked row. Replace with `new-session -d -s t -x 80 -y 24 'printf "MARK README.md\n"; exec sleep 300'` plus `until_cap` before the click, which removes the contingency instead of betting on which side of the race wins. Verified row 1 is exactly `MARK README.md`. Diff: about 4 lines, deletes `sleep 0.4` and `sleep 1.5`.

## Decision list

1. **CPU budgets (a1) or counts and sentinels?** Default: CPU at all five sites; counts reach only one. Needs an explicit rule amendment that CPU time is not a clock.
2. **What margin?** Default 3x idle CPU max, so 0.40 s at run.sh:4410 against 135 ms measured (the first draft said 0.6 s against 197 ms; that was the pipeline form's CPU).
3. **maxRSS for the 400k-diff site?** Default yes, with `skip_` when `/usr/bin/time` is absent: 191 MB to 76 MB, load-independent.
4. **0.15 s becomes a hang guard?** Default yes, per run.sh:5748.
5. **Sweep or point fix for the sleeps?** Default sweep, plus the three missing `# group: serial` markers.

## Addendum: the two red sites that are not budgets

Both prescriptions below stand up a tmux server. Run them only on a private `tmux -L <socket>` of your own; never against the attached server.

**1. run.sh:5546 `the store itself is gone`.** Line 5545 polls `delivered.md` with `until_file`, then 5546 bare-tests `$RASESS/comments.md`. Deliver writes the archive and unlinks the store; nothing makes those atomic, so the poll can return between them and the store is still present.

Remedy: poll the store path instead of testing it once. The idiom already exists ten lines down at run.sh:5556, `for i in $(seq 1 40); do [ -e "$RASESS" ] || break; sleep 0.1; done`, so copy it, no new helper. Price: 1 line added. Catches the regression: yes, a store that is never removed is still absent after 4 s and still fails. Load 15+: green. Pristine: no measurable cost.

**2. run.sh:3588 `two blocked and one errored, across sessions`.** Diagnosed as far as read-only allows, and the location is pinned. All three `FCP` reads at run.sh:3577-3579 are unredirected; only 3579, `FCP3=$(... list-panes -t b:1 ...)`, targets window 1, so `can't find window: 1` still attributes to it, and it means session `b` existed while window `b:1` did not. A fix lane dropping `2>/dev/null` from the setup should expect the two neighbours to start speaking too. The `new-window -t b` at 3572 therefore failed or its window was gone by 3579, and its error was swallowed. `FCP3` is then empty, `asFC error ""` at 3585 does nothing, and the count reads short.

Why the `new-window` failed I cannot pin read-only, and I did not reproduce it. Cheapest instrumentation for a fix lane: drop `2>/dev/null` from 3570-3576 and let the setup speak.

Remedy, which is also the fix: pin the setup as a count before driving state. After 3579, `is "three agent panes exist" "3" "$(list-panes -a -F '#{pane_id}' | grep -c .)"`. Price: about 4 lines with the redirects removed. Catches the regression: yes, run.sh:3588 is untouched. Load 15+: a silent setup failure becomes an honest red at its own line instead of a wrong fleet count plus a cascade. Pristine: unchanged. This is the rule already written at run.sh:8385, count the thing, never time it.
