# Lane D: the current ask-or-write decision and its coupling

Question: where does `tui/prompts/write.txt` decide ask or write, and is the coupling the #718 backfires suggest real?

Read-only. No model calls. No held-out case body and no scratchpad file was read; the existing ho3 case was used by id and by its existing note only. Grades: C = shown by file:line or command output, P = plausible inference, S = speculation.

## Branch map (`tui/prompts/write.txt`, 91 lines)

| Line | Decides | Hands over to |
|---|---|---|
| 11 | `<output_type>` given: make that type, do not ask which | skips the type question at 67 onward |
| 12-18 | pick one output type: prompt, steps, command, text, format, hint; prompt is the default for work (13) | |
| 32 | large or unclear task: Claude Code looks first and returns a plan or questions | 33 |
| 33 | big task: baseline facts, plan, wait for the OK | 34 |
| 34 | big new feature: interview via AskUserQuestion, requirements, OK, then plan | the written prompt carries the interview |
| 35 | small addition: check the existing one, add, test both | |
| 39 | feature: what done looks like; if done-when or the kind is open, a plan-only branch with no interview | the written prompt carries the plan |
| 55 | never guess what only the user knows: ask. For what Claude Code can check, tell it to check | 67 |
| 67-68 | when to ask, and the ordered list "ask the first open one" | 69-76 |
| 69-76 | 1 purpose, 2 user-only fact, 3 type, 4 subject, 5 what matters, 6 scope, 7 approach, 8 reader and tone | |
| 80 | big new feature: look for its purpose in the request and screen; if neither says it, ask as point 1; if stated, ask a point-2 fact or write. Scope and kind go to the interview | 34 (write) or 69 (ask) |
| 81 | do not ask scope or approach (6, 7) when the prompt has Claude Code return a plan or options first | 39 |
| 82 | `<questions_left>` is 0: no question, write | |
| 83-89 | question format: exactly 2 options | |

Ask or write is decided in two stages: line 80 judges whether the purpose is stated, then line 34 versus 39 picks the write shape. Lines 80, 81, 39 and 34 point at each other. `docs/tui.md:285-296` is the rule for prompt changes (name targets, 30 runs per side, one-sided Fisher p < 0.05, guards 3 runs, at most 3 wordings); `docs/tui.md:412-418` is judge rule 9 (corrected by the refute pass from 413-416).

## The two #718 wordings against `origin/fix/718-purpose-ask`

- W1 (`2f94ab31`, one line): edits line 80 only. Replaces "(the problem it solves, or who uses it and what they get)" with "(the problem it solves, or the result the user wants from the whole feature)" and adds "even when a part names who will use it and what it shows (a screen for staff to see the orders)". C.
- W2 (`59539018`): keeps W1's line 80, adds to line 34 "or one capability added across a whole service, such as sign-in, caching or request limits for an API, even when the request names it in a few words", and to line 39 "(for a big new feature, after the interview and the requirements list above)". C. The line 34 text names caching, the exact request of the `two-services` case. Both wordings are reverted on that branch (tip `43881430` "Revert W2" over `a210adf4` "Revert W1"), so the branch's current `write.txt` shows neither; read them with `git show` on the two commits.

## Cases and the lines they depend on

| Case | Expect | Lines | Counts (C unless noted) |
|---|---|---|---|
| ho2-clinic-reminders | ask | 80 | master 13/30, W1 29/30, W2 30/30; mechanism P |
| ho2-sensor-anomalies | ask | none (no named user) | 30/30 on master, W1, W2 |
| h-rate-limit | either | 34 vs 39 | master 19/30 (11 misses), W1 11/30 (19 misses), W2 30/30. W1 vs master Fisher p = 0.035. Every miss on both sides is a plan-first prompt with no asking and `missing: requirements, AskUserQuestion\|interview`. W2 changed lines 34 and 39 together, so the 30/30 does not separate them |
| two-services | ask (`ask_any: gateway; billing`) | 4 subject question; W2 line 34 | master 30/30, W2 3/30 (p < 0.0001). All 27 W2 misses asked the purpose question ("What is the caching for?") and none named a service. Master's passes asked which service. Mechanism C: W2's caching example makes caching a big feature, line 80 then fires the purpose question as point 1, which outranks point 4 (subject) |
| ho3-tool-lending-parts | ask (gate) | 80 | W1 9/15, W2 7/15 from the jsonl; master 3/15 rests on the issue comments only (no master jsonl) |
| h-feature-done | either | | 3/3 in both full runs |

## Verdict after the refute pass

Split by pair, because the evidence differs:

- **W2 and `two-services`: coupling through the ask order, PROBABLE.** The failing rows show the path: line 34 classifies caching as a big feature, line 80 fires, and the point 1 purpose question outranks the point 4 subject question. The trigger is the caching example, but the damage travels through lines 34, 80 and 69-76, so it is more than a local wording collision. C for the path, 27 of 27 misses.
- **W1 and `h-rate-limit`: PLAUSIBLE.** The 19/30 to 11/30 drop is significant under the project's own rule (p = 0.035), but it is one comparison. `big-refactor-plan` 18/30 to 12/30 (p = 0.098) and `json-flag` 29/30 to 27/30 moved the same way. Master and W1 were measured in the same runs, so baseline drift does not explain the gap. The earlier ranking put drift third; the refute pass rates it the weakest.
- **A likelier general cause the first pass left out:** any wording that pushes the panel toward "big feature" or "ask the purpose" shifts other cases through lines 34, 39 and 80. W2's h-rate-limit gain (30/30) and `two-services` loss (3/30) look like one lever pulled in opposite directions. W2 got no 30-run guard on `big-refactor-plan` or `json-flag` (0/3 and 0/3 in its full run), so the statement that other guards moved less than p < 0.05 covers W1 only. Untested at 30 per side.
- **Label risk:** h-rate-limit is `expect: either` with a `must` on the interview, so a sensible plan-first prompt fails the keyword check. P.

Decisive probe, not run (costs model calls). Three arms at 30 per side on `two-services`, h-rate-limit and `big-refactor-plan`: W2' (line 34 without the caching example, line 39 kept), W2'' (line 34 as W2, line 39 unchanged), and master twice for baseline spread. W2' recovering `two-services` while h-rate-limit holds would show a collision; W2'' separates line 34 from line 39. The lane estimated a few cents under the existing ledger; the orchestrator's own estimate is at about $0.006 per case-run from `cost.tsv`, so about $1.60 for 3 arms x 3 cases x 30 runs.

Files read: `tui/prompts/write.txt`, `tui/prompts/write.examples.txt`, `tui/evals/panel/cases.txt` (tune cases), `docs/tui.md`, and the jsonl and `cost.tsv` files under `~/.cache/lane718-tmp/`.
