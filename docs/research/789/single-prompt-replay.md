# Single-prompt replay: master's prompt on the same 80 tune cases

Run on 2026-11-10 as the like-for-like comparison that the review of `router-replay.md` asked for. Ran by the panel session as `probe789c`: research only, nothing committed, a detached worktree. The raw jsonl, the logs and the analysis script stay in `~/.cache/probe789c-tmp/`, with the full 80-id table in `analysis.txt`.

## What was run

| Item | Value |
|---|---|
| Prompt | `origin/master` `tui/prompts/write.txt` plus `tui/prompts/write.examples.txt`, files hash `0c183bbab74a1feb` |
| Cases | the same 80 tune ids as the router replay (24 expect ask, 48 expect write, 8 expect either), `--check` header "0 holdout" |
| Runs | 3 per case, 240 calls, same model and flags as the router replay, hermetic, no tools |
| Spend | $1.2141 (`cost.tsv`: $0.5230 for run 1, $0.6911 for runs 2 and 3). The estimate was about $1.25, the cap $2 |
| Limit | master's prompt was tuned on these cases through #701 and #702 (the #718 wordings were tried on them and not merged). The router prompt carries no case text and no case ids. Its sources include lane D, which read the tune cases, and the owner's purpose rule from the master tuning |

The orchestrator read the counts back from `analysis.txt` and re-added the spend.

## Results (calls)

| Expect | master asks | master writes | router asks | router writes |
|---|---|---|---|---|
| ask (72 calls) | 72 | 0 | 41 | 31 |
| write (144 calls) | 2 | 142 | 12 | 132 |
| either (24 calls) | 11 | 13 | 12 | 12 |

Master matched the label on 214 of the 216 ask and write calls, the router on 173 of 216. The 24 either calls are not counted. Master's 0 errors and 0 parse failures match the router's.

Master's two misses: `big-refactor-plan` (asked on run 1) and `derived-cards-parser` (asked on run 1), both labelled write. Runs disagreed on 4 of 80 cases for master (those two plus `h-search-answered` and `two-services-answered`, both either) and on 1 of 80 for the router.

Per-call cost and latency, router alone against a full master reply. Master: mean $0.00506, median latency 3,919 ms on run 1 and 2,961 ms on runs 2 and 3 (the eval's own p50 reads 3,966 ms on run 1, the upper middle value). Router: mean about $0.0026 (the total $0.6311 over 240 calls), median latency 2,562 ms. These are not a saving. The router is a step in front of the writer, so a routed request pays the router call and then a writer call. No end-to-end cost or latency was measured. The two runs are on the same machine on different minutes and box load was not recorded.

## Where the four system hashes come from

The 240 master rows carry four system hashes, because one prompt file pair is rendered per case with its context (output type, window, bare refs), and the example selection varies. Every case has exactly one hash across its 3 runs. That is master as shipped, so the comparison is not unfair. The router has one fixed text (system hash `337ce79b67cd67b2`). Its `prompt` field reads `0c183bbab74a1feb` only because that field hashes master's files. Four of the ten ask cases the router misses and master asks (`yaml-service`, `ci-workflow-spec`, `nightly-job-yaml`, `h-api-run-config`) are `format` cases, which add examples the router never sees, so part of the gap may be example selection and not only in-sample tuning.

## Where they differ (calls asked out of 3, master against router)

| Label | Cases |
|---|---|
| ask, master 3/3 and router 0/3 | `invoice-off`, `yaml-service`, `split-module`, `review-own-diff`, `review-my-changes`, `review-branch-focus`, `refactor-plan-goal`, `ci-workflow-spec`, `nightly-job-yaml`, `h-api-run-config` |
| ask, master 3/3 and router 2/3 | `dates-wrong` |
| write, master 0/3 and router 3/3 | `standup-moves`, `deploy-cancelled-given`, `one-on-one-cancelled`, `pnpm-switch` |
| write, master 1/3 and router 0/3 | `big-refactor-plan`, `derived-cards-parser` |
| either | `two-services-answered` (master 1/3, router 3/3), `h-search-answered` (master 1/3, router 0/3) |

## What it shows

1. On these 80 tune cases the single prompt is nearly at the ceiling: 214 of 216 calls. The router's 173 of 216 is 41 calls worse. Graded CONFIRMED.
2. The comparison is not like for like. Master's prompt was tuned on these cases. The router prompt carries no case text, though its sources include lane D, which read the tune cases. Master has an in-sample advantage of unknown size. Graded CONFIRMED that the advantage exists, and PLAUSIBLE that it explains much of the gap. A fair comparison needs cases that neither prompt was fitted to.
3. The cases where the router asks and master writes (`standup-moves`, `deploy-cancelled-given`, `one-on-one-cancelled`, `pnpm-switch`) are the router's four false asks. The 11 ask cases that the router writes and master asks are the router's misses. These two sets do not overlap with master's two misses, so the two prompts fail on different cases.
4. Trying a second router version on these tune cases would fit it to cases where the single prompt already scores 214 of 216. It could not show an improvement over master on them. Graded PLAUSIBLE, a judgment about the setup and not a measurement.
5. No like-for-like out-of-sample measure of both prompts exists in this folder. The one place it could be measured is the new blind set, which is not yet committed.

## Not tested

- Either prompt on any case it was not fitted to.
- A second router version.
- The router with tools.
- Run-to-run spread of master across more than 3 runs per case on all 80 cases.
