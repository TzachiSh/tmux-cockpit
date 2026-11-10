# Verified claims

Three fresh refute lanes re-fetched and re-counted every load-bearing claim on 2026-11-10. The fetch tool returns short excerpts, capped at about 125 characters per quote, so "exact" means the excerpt the tool returned. Wording in the lane files was corrected to match; the grade below is the grade of the claim as first written.

## Lane A (`lane-a-ask-or-act.md`)

| Claim | Grade | Reproduction and correction |
|---|---|---|
| A1 6,120 dialogues, 10 LLMs, 5 dimensions, 6 personas, "consistent under-clarification bias" | CONFIRMED | arxiv.org/abs/2512.21120 abstract. "Under-asking is the common failure" is a mild paraphrase |
| A2 "overwhelmingly default to direct answers" | CONFIRMED | arxiv.org/abs/2605.25284, titled "Knowing but Not Showing" |
| A3 ambiguity detection fails; "up to 74%" | TIGHTENED | arxiv.org/abs/2502.13069. 74% is a maximum improvement, metric not given. The ICLR 2026 acceptance is in the comments field. The exact "struggle to distinguish" quote came back only as a paraphrase |
| A4 69.40% with a separated detection step | TIGHTENED | arxiv.org/abs/2603.26233. Aggregation across models not stated, no baseline number, margin over single-agent unquantified |
| A5 vendor prompt text on acting and asking | CONFIRMED, one TIGHTENED | The "never guess missing parameters" line sits in the parallel-tool-calls block and covers tool-call parameters only. Overtrigger quotes name Opus 4.5 and 4.6 and concern tool use |
| A6 AskUserQuestion and plan mode | PLAUSIBLE | The page would not load; the issue is closed as resolved per a search summary |
| A7 over-asking cost | PLAUSIBLE | Not opened; no number |

## Lane B (`lane-b-routing-step.md`)

| Claim | Grade | Reproduction and correction |
|---|---|---|
| B1 routing is a named pattern; chaining trades latency for accuracy | CONFIRMED | anthropic.com/engineering/building-effective-agents. The latency quote is about chaining |
| B2 route easy questions to smaller models | CONFIRMED | same page |
| B3 RouteLLM "over 2 times" cost cut | CONFIRMED | arxiv.org/abs/2406.18665 v4. Model selection, not scope classification |
| B4 RACER misrouting and abstention | CONFIRMED | arxiv.org/html/2603.06616v1 sections 1 and 2.2 |
| B5a UCCI calibration error 0.12 to 0.03 | CONFIRMED, n corrected | 75,000 queries, a production NER workload |
| B5b RouteNLP 500 examples; Signed Rescue Routing split | REFUTED (not in the abstracts) | Dropped from the evidence |
| B6 calibration queries must resemble the workload | REFUTED as cited | arxiv 2501.01818 is about router-attack token sequences. Dropped |
| B7 TwinRouterBench | TIGHTENED | 970 prefixes from 520 instances; benchmarks downstream task success; no explicit error-propagation analysis |
| B8 Haiku 5.5 $0.10 / $0.50 per MTok, Haiku 4.5 $1 / $5 | CONFIRMED | platform.claude.com pricing table. The over-100,000-token tier ($0.50 / $2.50) is omitted; irrelevant at 1,500 tokens |
| B9 about $0.00016 and $0.0016 per call | CONFIRMED | recomputed: 1500 x 0.10/1e6 + 20 x 0.50/1e6 = $0.00016; 1500 x 1/1e6 + 20 x 5/1e6 = $0.0016. Token counts are assumed |

## Lane C (`lane-c-impact-criteria.md`)

| Claim | Grade | Reproduction and correction |
|---|---|---|
| C1 one-way and two-way doors | CONFIRMED | SEC exhibit 99.1; "or small groups" added |
| C2 blast radius and reversibility | CONFIRMED | AWS Well-Architected; an operations principle with no threshold |
| C3 Polar design-doc triggers | TIGHTENED | four triggers confirmed; "A pull request is enough" belongs to the endpoints exclusion only |
| C4 architectural significance | CONFIRMED | adr.github.io; the quote also says "and quality" |
| C5 risk-fitted deployment | CONFIRMED | sre.google release engineering; per-service, not per-change |
| C6 thresholds from Mage and Infisical | TIGHTENED | Mage says "longer than a day"; "more than one reasonable solution or team" appears in neither source. Infisical page returned 404, snippet only |
| C7 plan mode quote | CONFIRMED | code.claude.com common-workflows; best-practices page keyword search not re-checked |
| C8 checklist criteria e, f, and the multi-team part of d | NO SOURCE | disclosed as the lane's own synthesis |

## Lane D (`lane-d-decision-map.md`)

| Claim | Grade | Reproduction and correction |
|---|---|---|
| D1 line map of `write.txt`, 91 lines | CONFIRMED | `git show origin/master:tui/prompts/write.txt \| cat -n` |
| D2 W1 `2f94ab31` edits line 80; W2 `59539018` edits 34, 39, 80 | CONFIRMED | `git show -U0`. Both are reverted on the branch tip (`43881430` over `a210adf4`), now stated |
| D3 clinic 13, 29, 30 of 30; sensor 30 of 30 in all three | CONFIRMED | python tally of `pass` over `~/.cache/lane718-tmp/*.jsonl` |
| D4 h-rate-limit | TIGHTENED | master 19/30, W1 11/30 (Fisher p = 0.035), W2 30/30. All misses on both sides are plan-first prompts missing the interview terms. W2 changed lines 34 and 39 together |
| D5 two-services | TIGHTENED, mechanism now C | master 30/30, W2 3/30. All 27 W2 misses asked the purpose question ("What is the caching for?"); master asked which service |
| D6 ho3 3/15, 9/15, 7/15 | TIGHTENED | master 3/15 is from the issue comments only; no master jsonl exists |
| D7 big-refactor-plan 18 to 12 (p = 0.098); json-flag 29 to 27 | CONFIRMED | W2 has no 30-run guard on either (0/3 and 0/3 in its full run) |
| D8 `docs/tui.md` 285-296 decision rule | CONFIRMED | |
| D9 `docs/tui.md` rule 9 at 413-416 | TIGHTENED | rule 9 is lines 412-418 |
| D10 verdict PLAUSIBLE | TIGHTENED | PROBABLE for W2 and two-services (path shown in the rows); PLAUSIBLE for W1 and h-rate-limit; baseline drift rated weakest |
