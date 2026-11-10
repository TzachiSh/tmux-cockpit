# Single-prompt replay: master's prompt on the same 80 tune cases

Run on 2026-11-10 as the like-for-like comparison that the review of `router-replay.md` asked for. Ran by the panel session as `probe789c`: research only, nothing committed, a detached worktree. The raw jsonl, the logs and the analysis script stay in `~/.cache/probe789c-tmp/`, with the full 80-id table in `analysis.txt`.

## What was run

| Item | Value |
|---|---|
| Prompt | `origin/master` `tui/prompts/write.txt`, prompt hash `0c183bbab74a1feb` |
| Cases | the same 80 tune ids as the router replay (24 expect ask, 48 expect write, 8 expect either), `--check` header "0 holdout" |
| Runs | 3 per case, 240 calls, same model and flags as the router replay, hermetic, no tools |
| Spend | $1.2141 (`cost.tsv`: $0.5230 for run 1, $0.6911 for runs 2 and 3). The estimate was about $1.25, the cap $2 |
| Limit | master's prompt was tuned on these cases through #702 and #718. The router prompt was written from the research alone and never saw them |

The orchestrator read the counts back from `analysis.txt` and re-added the spend.

## Results (calls)

| Expect | master asks | master writes | router asks | router writes |
|---|---|---|---|---|
| ask (72 calls) | 72 | 0 | 41 | 31 |
| write (144 calls) | 2 | 142 | 12 | 132 |
| either (24 calls) | 11 | 13 | 12 | 12 |

Master matched the label on 214 of 216 calls, the router on 173 of 216. Master's 0 errors and 0 parse failures match the router's.

Master's two misses: `big-refactor-plan` (asked on run 1) and `derived-cards-parser` (asked on run 1), both labelled write. Runs disagreed on 4 of 80 cases for master (those two plus `h-search-answered` and `two-services-answered`, both either) and on 1 of 80 for the router.

Cost and latency per call. Master: mean $0.00506, median latency 3,966 ms on run 1 and 2,961 ms on runs 2 and 3. Router: mean about $0.0026 (the total $0.6311 over 240 calls), median latency 2,562 ms. The router is cheaper and about 0.4 to 1.4 s faster per call at the median. The two runs are on the same machine on different minutes and box load was not recorded.

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
2. The comparison is not like for like. Master's prompt was tuned on these cases and the router never saw them, so master has an in-sample advantage of unknown size. Graded CONFIRMED that the advantage exists, and PLAUSIBLE that it explains much of the gap. A fair comparison needs cases that neither prompt was fitted to.
3. The cases where the router asks and master writes (`standup-moves`, `deploy-cancelled-given`, `one-on-one-cancelled`, `pnpm-switch`) are the router's four false asks. The 11 ask cases that the router writes and master asks are the router's misses. These two sets do not overlap with master's two misses, so the two prompts fail on different cases.
4. Trying a second router version on these tune cases would fit it to cases where the single prompt already scores 214 of 216. It could not show an improvement over master on them. Graded PLAUSIBLE, a judgment about the setup and not a measurement.
5. Neither prompt's behavior on cases they were not tuned on is measured by anything in this folder. The one place it could be measured is the new blind set, which is not yet committed.

## Not tested

- Either prompt on any case it was not fitted to.
- A second router version.
- The router with tools.
- Run-to-run spread of master across more than 3 runs per case on all 80 cases.
