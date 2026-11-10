# Router replay: one frozen router prompt on the tune cases

Run on 2026-11-10 for decision 2 of the README, option A, which the owner chose. Ran by the panel session as `probe789b`: research only, nothing committed, a detached worktree, one router version. The raw jsonl, the per-run logs, the analysis script and the scratch patch stay in `~/.cache/probe789b-tmp/`.

## What was run

| Item | Value |
|---|---|
| Router prompt | `router-prompt.txt` in this folder, sha256 `bd1cd72d38e625d1f535387eee3c096aa1d301e5762da4ac6bef01b2ae9615c9`, checked again after the runs |
| Written from | lane A, lane C and the decision map in this folder, plus the owner's purpose rule. Case requests were not read |
| Labels | `ask` or `write`, one word. An unparseable reply maps to `ask` |
| Cases | 80 tune cases (24 expect ask, 48 expect write, 8 expect either); `--check` reported 0 holdout, and 41 held-out blocks were dropped |
| Runs | 3 per case, 240 calls, through the eval rig's own envelope and CLI path, hermetic, model sonnet |
| Limit of the setup | the rig ran with `--no-explore`, so the router had no tools and could not look at files |
| Spend | $0.6311 as summed over the 240 calls in `analysis.txt` (`cost.tsv` rounded gives $0.6312: $0.3706 for run 1, $0.2606 for runs 2 and 3). The estimate was $1.20, worst case $2.70, hard stop $5 |

The orchestrator read the counts below back from `analysis.txt` and re-checked the hash and the spend.

## Results (calls)

| Expect | router ask | router write |
|---|---|---|
| ask (72 calls, 24 cases) | 41 | 31 |
| write (144 calls, 48 cases) | 12 | 132 |
| either (24 calls, 8 cases) | 12 | 12 |

173 of 216 non-either calls matched the label. Parse failures 0 of 240, API errors 0. Runs disagreed on 1 of 80 cases (`dates-wrong`), so the misses are systematic, not sampling noise.

Median latency 2,562 ms, max 6,076 ms (n = 240 on this machine, through the panel's own CLI path, with no tools, box load not recorded). Median cost per call $0.0015, max $0.0064, and median tokens 1,114.5 in and 3 out.

## Misrouted cases

Labels on runs 1, 2 and 3, where a is ask and w is write.

| Expect | Cases |
|---|---|
| ask, routed write (11 of 24 cases) | `ci-workflow-spec`, `h-api-run-config`, `invoice-off`, `nightly-job-yaml`, `refactor-plan-goal`, `review-branch-focus`, `review-my-changes`, `review-own-diff`, `split-module`, `yaml-service` (all www); `dates-wrong` (waa) |
| write, routed ask (4 of 48 cases) | `deploy-cancelled-given`, `one-on-one-cancelled`, `pnpm-switch`, `standup-moves` (all aaa) |

## What it shows

1. The router asked on 41 of 72 calls where the label is ask, and on 12 of 144 calls where the label is write (8.3%). Those 72 calls are 24 cases x 3 correlated runs. At case level, 13 of 24 ask cases were asked on every run. Graded CONFIRMED for this setup.
2. The misses are systematic: 14 of 15 misrouted cases were wrong on all three runs. Graded CONFIRMED. One edit of the router prompt might move several of them at once. Graded PLAUSIBLE, untested. A second version needs the orchestrator's approval before the tune cases are used to fit it.
3. The sourced criteria in the router prompt (a missing fact, an unstated purpose, a foundation, a new component, a one-way door) do not cover most of the missed ask cases. Their names (review, CI, YAML, refactor, split) point to scope and approach questions that the router judged Claude Code could settle by looking. Whether the router would ask with tools is not tested, since the rig ran without them. Graded PLAUSIBLE: it reads the case names and labels, not the case text.
4. The four false asks (cancel, move and switch requests) may be the one-way-door rule firing on requests that name a cancellation or a switch. Not tested. Graded PLAUSIBLE.
5. Money is not the cost: about $0.0015 a call on this CLI path. Latency is: a median of 2.6 s added in front of every panel reply. Whether that is acceptable is the owner's call.
6. A comparison with the single prompt exists only for six cases, from earlier runs, direction only. Master asked on 90 of 90 `two-services` runs (the router 3 of 3), 30 of 30 `ho2-sensor-anomalies` (3 of 3), 15 of 30 `ho2-clinic-reminders` (3 of 3), 44 of 90 `big-refactor-plan` (0 of 3, label write), 18 of 90 `h-rate-limit` (0 of 3, label either), and 0 of 30 `json-flag` (0 of 3). No full master run exists on all 80 cases, so no like-for-like comparison was made.

## What this means for the router question

Graded PLAUSIBLE. The router asked on none of 3 runs of `big-refactor-plan` (master asked on 44 of 90) and of `h-rate-limit` (master 18 of 90, label either). Three runs cannot carry that. The chance of 0 asks in 3 runs at master's own rate is about 13% for `big-refactor-plan` and about 51% for `h-rate-limit`, so the second is indistinguishable from chance and is not a miss under its label. The router also missed 11 of the 24 ask cases, 10 of them on every run. No master run of those 11 exists in the data, so the data does not show that master does better on them. Whether a router beats one prompt is not settled here. It moves some behavior into its own rules, and a second version was not tried.

## Not tested

- A second router version.
- The router with tools, which could look at files.
- A full single-prompt run on the same 80 cases for a like-for-like comparison.
- Any held-out case.
