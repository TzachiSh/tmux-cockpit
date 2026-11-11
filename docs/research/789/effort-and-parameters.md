# Effort and call parameters for the panel's `claude -p` calls (#789)

Record of the follow-up approved on #789 (comment 6771952018): which effort level and which call settings the router and the writer should use, and whether anything makes a call faster or cheaper. Written by tmux-cockpit-f2 from the runner's own ledger and files (84 [0f80bb] ran every call). The router prompt is v2 (`router-prompt-v2.txt`). Nothing here is wired into the product.

All costs are notional API-rate figures on a Max subscription login. The share of plan limits was not measured.

## Rule fixed in advance

An arm is adopted when, on the out-of-sample half, its quality is no worse than the baseline by more than 1 case, no always-right case drops, and then either median latency is down at least 300 ms beyond the baseline spread or cost is down at least 20%. Anything inside the noise is reported as noise. Always-right means a case that both baseline arms got 3/3.

Labels: develop is in-sample. Confirm is in-sample for the writer and partly in-sample for the router (it picked v2 over v3). Held-out is fully unseen by every writer lane. Held-out results are counts only, scored through `label` (router) or `pass` (writer).

## Parameters that exist (official docs)

Sources: https://code.claude.com/docs/en/env-vars and https://code.claude.com/docs/en/data-usage.

- From the research lane's own sources, not from the two pages above (unconfirmed there): models haiku-5-5, sonnet-5-5, opus-5-5, fable-5-2. Haiku is $0.10 / $0.50 per MTok, Sonnet $2 / $10. All take five effort levels. Thinking cannot be turned off. There is no temperature or top_p flag. `--bare` needs an API key (this box is a subscription login). `--json-schema` exists.
- `DISABLE_TELEMETRY`: opts out of telemetry, "Also disables feature-flag fetching".
- `DISABLE_AUTOUPDATER`: disables automatic background updates. `DISABLE_UPDATES` blocks all updates. A separate update-check variable is not documented.
- `DISABLE_ERROR_REPORTING`: opts out of error reporting (stack traces sent to a third-party tracker, on for Pro, Max and Ultra sign-ins connecting directly).
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`: covers auto-updates, telemetry, error reporting, `/feedback`, release notes, status badge checks and availability checks, and feature-flag fetching. It does not cover the WebFetch preflight or the marketplace auto-install. It makes the three individual variables redundant.
- `CLAUDE_CODE_DISABLE_FEATURE_FLAGS`: feature-flag fetching only.

## Stage 1 and 1b: start-up latency of a trivial call

Stage 1 ran 220 trivial calls in all. Stage 1b ran 20 calls per variant, 6 variants, 120 calls, in a shuffled order (seed 789). Every reply was "ok". The figures below are stage 1b's, recomputed from its rows.

- Wall p50: both baselines 2492 and 2354 ms (spread 138 ms). `DISABLE_TELEMETRY=1` 1566 ms, the umbrella variable 1556 ms, all flags at once 1536 ms, feature flags alone 2049 ms.
- So telemetry and the umbrella take about 0.86 s off a trivial call, and feature-flag fetching alone takes about 0.37 s. Stage 1's start-up split (about 1.05 s down to about 0.3 s) is the runner's report and was not recomputed here.
- Every variant adds about 41 cache-write tokens per call (565 against 604 to 609).
- Cost: $0.4441 and $0.2900.

## Router: effort (R1 develop and confirm, R2 held-out)

Router v2, 3 runs per case, arms low, medium-a, medium-b, high, three interleaved rounds. Ask and write cases at 3/3 of 36, then calls of 108 (held-out: 6 ask, 30 write, 5 either).

| half | arm | 3/3 of 36 | calls of 108 | p50 ms | $/call |
|---|---|---|---|---|---|
| develop | low | 34 | 102 | 2514 | 0.00202 |
| develop | medium-a | 34 | 103 | 2508 | 0.00164 |
| develop | medium-b | 34 | 104 | 2357 | 0.00165 |
| develop | high | 35 | 105 | 2385 | 0.00192 |
| confirm | low | 34 | 102 | 2699 | 0.00197 |
| confirm | medium-a | 34 | 102 | 2617 | 0.00161 |
| confirm | medium-b | 34 | 102 | 2057 | 0.00165 |
| confirm | high | 33 | 101 | 2190 | 0.00194 |
| held-out | low | 35 | 106 | 2206 | 0.001754 |
| held-out | medium-a | 35 | 105 | 2057 | 0.001755 |
| held-out | medium-b | 35 | 105 | 2106 | 0.001776 |
| held-out | high | 34 | 103 | 2207 | 0.001885 |

Baseline p50 spreads: develop 152 ms, confirm 560 ms, held-out 49 ms. Low's higher cost on develop and confirm was run order: it ran first and paid the cache writes (round 1 $0.002786 per call, rounds 2 and 3 $0.001595). Warm, low is about -3% against medium.

Verdicts:
- low: quality within 1 on both out-of-sample halves, no always-right drop. Latency and cost are inside the noise. Not adopted.
- high: drops one always-right case on confirm (`undo-which`) and one on held-out, costs +18% (R1) and +6.7% (R2). Fails the rule. Not adopted.
- Medium stays. Cost: R1 $1.7276, R2 $0.8818.

## Router: model, schema, telemetry (R3)

- haiku at medium: develop 29/36 (94/108), 5 always-right drops. Confirm 25/36 (82/108), 9 drops. Held-out 35/36 (106/108), 0 drops (30 of 36 are write cases, which hides the ask misses). Its misses are mostly new cases, not medium's hard ones (shared misses 2 of 7, 2 of 11, 1 of 1). About 1/14 to 1/15 of the cost per call as measured. Quality fails. Not usable at this prompt.
- `--json-schema` (label enum): one probe call, $0.007626. The label came back both in `structured_output` and in `result`. The call took two turns (`num_turns` 2), 1778 cache-write tokens against about 1130, 51 output tokens against 3, so roughly four times the cost per call and no latency win possible. One-word replies had 0 parse failures in 1,812 router calls. The arm was not run. Parse rule written for it: `structured_output.label` if a string, else `result` parsed as JSON with a string `label`, else the usual no-answer failure.
- `DISABLE_TELEMETRY=1` at medium: quality unchanged on develop 34/36, confirm 34/36, held-out 35/36, no always-right drop, labels differing from the baseline majority 3/120, 0/120 and 1/123. p50 1705, 1505, 1252 ms against baselines 2432, 2337, 2082, so -727, -832 and -830 ms.
- Umbrella `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` (develop and confirm only, as ruled): 34/36 (103/108) and 34/36 (102/108), no always-right drop, 1/120 and 0/120 labels differing. p50 1204 and 1202 ms, so -1228 and -1135 ms, eight and two times the spreads. $/call equal to medium. Adopt-candidate for the router by the rule. Latency caveat: R3 ran later than the baselines, so time of day is not controlled. Stage 1b's shuffled start-up result points the same way.
- The first telemetry round cost 1.7 times more per call. That was a cold cache. The variable adds 41 tokens after a 736-token shared prefix, so the first call of the new prompt variant re-wrote about 370 tokens it had been reading. Per call, telemetry round 1 wrote 702 and read 755, rounds 2 and 3 wrote 337 and read 1121, the same as medium. Four direct calls confirm it: plain cold wrote 532 and read 736 ($0.0022356), umbrella cold 573 and 736 ($0.0023996), plain warm wrote 0 and read 1268 ($0.0001608), umbrella warm 0 and 1309 ($0.0001649). Steady-state cost of disabling is +41 tokens per call: +7% on a call that writes its cache, +2.6% on one that reads it.
- Cost: R3 $1.2155 (probe $0.0076, haiku $0.0418, telemetry $0.7694, umbrella $0.3917, cache check $0.0050).

## Writer (held-out only, 41 cases, 3 runs, `--no-explore`, scored by `pass`)

Five arms. Confirm was not run, as it would have passed the $10 cap.

| arm | 3/3 of 36 | calls of 108 | either 3/3 of 5 | p50 ms | p90 ms | $/call, rounds 2 and 3 | out tokens p50 |
|---|---|---|---|---|---|---|---|
| medium-a | 35 | 106 | 4 | 3508 | 5713 | 0.00508 | 230 |
| medium-b | 35 | 105 | 4 | 3509 | 5718 | 0.00522 | 232 |
| low | 33 | 105 | 4 | 3359 | 5315 | 0.00476 | 216 |
| high | 33 | 103 | 3 | 4811 | 6915 | 0.00642 | 394 |
| umbrella | 33 | 103 | 5 | 2757 | 4814 | 0.00506 | 249 |

A second held-out run added a third baseline (medium-c: 33/36, 102/108, p50 4712, $0.00514) and two runs of the umbrella (its third round did not run: the runner's per-leg headroom of $0.40 would have passed the $9.30 cap, a planning error of the runner). On runs 1 and 2, ask and write at 2/2 of 36: medium-a 35, medium-b 35, medium-c 33, umbrella run 1 33, umbrella run 2 31.

Literal verdict (rule as fixed, 3 runs): low, high and the umbrella each drop 2 always-right cases and fail. medium-c, a repeat of the baseline itself, also drops 2 and fails the same way. High is worse on every axis (p50 +1.3 s, +25% cost).

Noise reading, separately: on three runs medium-a and medium-b differ on 0 cases, medium-a and medium-c on 3, medium-b and medium-c on 3. Every umbrella "drop" is a single miss in three runs. The umbrella's run-1 and run-2 drops share 0 cases. The rule is too sharp at three runs. Writer quality under the umbrella is not distinguishable from noise at n=3, and it is not demonstrated equal. The box was loaded during the second run (medium-c p50 4712 to 4976 ms), so the writer's -0.75 s p50 is not confirmed by it. Cost: warm-ups $0.0455, held-out arms $3.3791, second run $1.0528.

## Ledger

Follow-up $9.0364 of the $10 cap: stage 1 $0.4441, stage 1b $0.2900, R1 $1.7276, R2 $0.8818, R3 $1.2155, writer warm-ups $0.0455, writer held-out arms $3.3791, writer second run $1.0528. The parts add up to $9.0364. Whole eval about $65.48 of $80: $53.01 to probe789d, $3.4378 router tuning (the passes sum to $3.4378, and the earlier record rounds it to $3.4380), $9.0364 follow-up. The $53.01 is carried from the runner's session record.

## What it means

- Effort: no effort level changes the router or the writer beyond noise, and high is worse. Keep medium.
- Model: Haiku is not usable for the router at v2.
- Structured output: not worth it. The call takes two turns.
- Telemetry and the other nonessential calls: the umbrella variable cuts wall time by about 0.75 s (writer, not confirmed on a loaded box) to 1.2 s (router) per call, changed 1 of 120 develop labels and 0 of 120 confirm labels against the baseline majority on the router (no held-out run backs the umbrella router), and costs +41 tokens per call. Writer quality is "not distinguishable from noise, not demonstrated equal". Setting it on the panel's own `claude -p` calls is a product change for the owner, because it turns off telemetry for those child calls.

## Where the evidence lives

Local only, under `~/.cache/probe789g` (stage1 and stage1b rows, `arms/<arm>/<half>/run*.jsonl`, `cost.tsv`, scripts `runner*.sh`, `writer*.sh`, `counts.py`, the scratch patches `r3-scratch*.diff`, never committed) and `~/.cache/probe789g-blind` (mode 700, held-out rows and logs, counts only, never opened). The scratch patches add `PROBE789G_EXTRA` to the claude argv for the three R3 arms and read `structured_output`. They change nothing when that variable is unset.
