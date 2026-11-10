# Probe results: which lines drive the two-services break

Run on 2026-11-10 for decision 1 of the README, which the owner answered yes. Ran by the panel session as `probe789`: tune cases only (`--check` reported 3 cases, 0 holdout), a detached worktree, nothing committed, 30 runs per case per arm, one-sided Fisher tests. The raw jsonl, per-arm logs and the analysis script stay in `~/.cache/probe789-tmp/`.

## Arms

| Arm | Prompt |
|---|---|
| master A, master B | `origin/master` `write.txt`, run twice for baseline spread |
| W2' | commit `59539018` with the caching example removed from line 34. Three edits against master remain: line 34 widening without the example, line 39 (plan after the interview), and line 80 (W1's edit) |
| W2'' | commit `59539018` with line 39 as on master. Two edits against master remain: line 34 widening with its caching example, and line 80 (W1's edit) |

Both W2 arms carry W1's line 80 edit, which makes a purpose harder to count as stated. The reviewer's `diff` of the arm prompts showed it.

## Counts (passes of 30; read back from the jsonl by the orchestrator)

| Case | master A | master B | W2' | W2'' |
|---|---|---|---|---|
| two-services | 30 | 30 | 22 (p = 0.0023 against master A or B) | 4 (p below 0.0001) |
| h-rate-limit | 23 | 25 | 30 (p = 0.0053 against master A, 0.0261 against B, 0.0051 against the pooled master) | 30 (same p values) |
| big-refactor-plan | 16 | 12 | 13 (p = 0.30 against A) | 18 (p = 0.40 against A, 0.098 against B) |

Master A against master B: two-services 30 and 30, h-rate-limit 23 and 25 (p = 0.37), big-refactor-plan 16 and 12 (p = 0.22). The earlier #718 master run of h-rate-limit was 19. So the baseline of h-rate-limit moved between 19 and 25 across three runs of the same prompt.

Spend: $2.447 (`cost.tsv`: $0.6004, $0.6820, $0.6199, $0.5447). The panel session's estimate before the run was about $2.30, and the cap I set was $3 (messages to and from the panel session).

## Miss kinds

| Case | What the misses were |
|---|---|
| two-services | every miss asked the purpose, mostly "What is the caching for?", never the subject. W2': 8 misses. W2'': 26. Master: 0 |
| h-rate-limit | every miss was a plan-first prompt with no interview. Master A 7, master B 5, both W2 arms 0 |
| big-refactor-plan | asked the purpose of the TOML move or what happens to the YAML files, on master and on both arms alike, except one W2' written prompt that lacked a re-run step |

## What it shows

1. Three edits (the line 34 widening without the caching example, the line 39 change, and W1's line 80 edit) drop `two-services` to 22/30 in W2', and every miss asks the purpose. So the caching example is not the only cause. How much is due to the line 34 widening alone is not isolated: W1's line 80 edit is in both W2 arms, and the only W1-alone data on this case is 3 of 3 in a short run, which cannot rule line 80 out. Graded CONFIRMED for the W2' result, PLAUSIBLE for the attribution to line 34.
2. The example and the line 39 change are confounded between W2' and W2''. They differ in two things. If the #718 W2 run (3/30, both factors present, measured earlier and possibly under a different baseline) is taken as the both-factors cell, removing the example moved 3 to 22 and removing the line 39 change moved 3 to 4. Graded PLAUSIBLE: it leans on a different run, and the cell with no example and master's line 39 was not run.
3. Both W2 arms lift h-rate-limit to 30/30. The lift is line 34 on top of W1's line 80 edit (W1 alone scored 11/30 in #718); line 39 is not needed for it, since W2'' keeps master's line 39. Graded CONFIRMED for this run, with master at 23 and 25.
4. `big-refactor-plan` sits at 40 to 60% on master and does not clearly move with either arm (W2'' 18/30 against master B 12/30 is p = 0.098). The purpose question fires on refactors already, so this is not a W2 effect.
5. The master baseline spread is wide: h-rate-limit 19, 23 and 25 across three runs. A difference of 4 to 6 passes at 30 runs is inside it. The W1 drop to 11/30 on h-rate-limit (the #718 W1 run) is below all three master runs.

## What this means for the router question

One reading, not isolated by these arms: making "add caching" a big feature makes the purpose question fire ahead of the subject question. A wording that fixes `h-rate-limit` through that same definition costs `two-services`. This matches the coupling in `lane-d-decision-map.md` and is evidence for a separate step that decides ask or write before the writer sees the lines, but it does not show a router would do better. Not tested.

## Not tested

- The cell with no caching example and master's line 39.
- W2' with only the line 34 edit, and W1's line 80 edit alone at 30 runs on `two-services`.
- A router.
- Any held-out case.
