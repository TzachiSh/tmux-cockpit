# Research 789: routing the ask or write choice

Date 2026-11-10. Owner decision: #718 parked, step back, research a dedicated routing step and an impact-based rubric (D2 of the owner's answers on epic #781). Read-only: no prompt, rubric, case or code changed, no paid run. No lane read the new held-out cases.

Lane files: `lane-a-ask-or-act.md`, `lane-b-routing-step.md`, `lane-c-impact-criteria.md`, `lane-d-decision-map.md`. Grades for each claim: `verified.md`.

## Measurements

| What | Number | Where |
|---|---|---|
| `write.txt` length | 91 lines; ask or write is decided across lines 34, 39, 80, 81 and the order at 67-76 | lane D |
| h-rate-limit | master 19/30, W1 11/30 (p = 0.035), W2 30/30 | lane D, jsonl |
| two-services | master 30/30, W2 3/30 (p < 0.0001); all 27 misses asked the purpose question | lane D, jsonl |
| ho2-clinic-reminders | master 13/30, W1 29/30, W2 30/30 | lane D, jsonl |
| Router call price | about $0.00016 on Haiku 5.5, $0.0016 on Haiku 4.5, on assumed 1,500 input and 20 output tokens | lane B, recomputed |
| Router latency | not measured, no source found | lane B |
| Proposed diagnostic probe | 4 arms (W2', W2'', master A, master B) x 3 cases x 30 runs = 360 case-runs at about $0.006 each is about $2.20 (per-run prices in `cost.tsv` range $0.0035 to $0.0081) | `cost.tsv`, recomputed by the reviewer |

## What the evidence says

- The ask-or-write decision in the shipped prompt is coupled through the ask order. For W2 and `two-services` the failing rows show the path: line 34 makes caching a big feature, line 80 asks the purpose first, and the purpose question outranks the subject question. Graded PROBABLE.
- The W1 drop on h-rate-limit is significant under the project's rule but is one comparison next to two weaker moves in guards. Graded PLAUSIBLE.
- Published work points the same way as the panel's experience on one point: models under-ask (A1, A2) and cannot reliably tell clear from unclear tasks (A3). One study separates detection from action and reports gains (A4), with its aggregation and baseline unstated. None of this measures the panel's own cases.
- A routing step is a standard pattern (B1) and costs almost nothing in money (B9). Its latency and its failure rate on a small labelled set have no source. Inference, not a finding: misrouting has no recovery unless the writer can still ask (lane B's refute note: RACER abstains at the model-set level, not the label level).
- A rubric by impact has sources for four criteria: touching a foundation or new component and the skip case (C3, all from one company's handbook), and a one-way door (C1, where "cannot be undone by reverting the commit" is the lane's own wording). The rest (more than one reasonable solution, stated purpose, crossing modules) have no source and stay the owner's own wording.

## Recommendation

1. Do not write a W3. Two wordings failed in opposite directions on the same lines.
2. Run the diagnostic probe first (decision 1). It separates line 34 from line 39 and measures baseline spread. It costs about $2.20 on tune cases only.
3. Prototype the router as a replay on the existing tune cases after the probe (decision 2), with a confusion matrix and a latency measurement. No change to the shipped prompt.
4. Draft the impact rubric as a document only (decision 3), using the four sourced criteria, until the router result is in.

## Decisions for the owner

1. Run the diagnostic probe? Options: A four arms (W2', W2'', master A, master B) on `two-services`, h-rate-limit and `big-refactor-plan`, 30 runs per case per arm, 360 case-runs, about $2.20, tune cases only. The arms are diagnostic, not candidate changes, and do not count toward the 3-wording cap in `docs/tui.md` / B skip it and go straight to the router prototype. Recommended: A because it is the cheapest way to learn whether the coupling is one line or the structure, and the router design depends on it.
2. Prototype the router? Options: A replay on the existing tune cases, cap $5 of the $80 eval budget, report a confusion matrix, misrouted cases and latency / B no router, keep one prompt and redesign its rules. Recommended: A after decision 1, because price is negligible and only measurement can show whether it beats one prompt.
3. Draft an impact rubric? Options: A a design document using criteria a, b, c and h with their sources, no judge change / B change rule 9 now / C nothing. Recommended: A because rule 9 stays as is until the routing question closes (#788 is parked for the same reason).
4. Keep #718 parked until decision 1 reports? Options: A yes / B reopen W3 now. Recommended: A because both wordings regressed.

Asked 2026-11-10, unanswered defaults apply: 1A, 2A after 1, 3A, 4A. The defaults spend only inside the eval budget the owner already set ($80, baseline cap $10); anything past it needs the owner's own word. Decision 1 was answered yes on 2026-11-10.

Key to finding ids: A, B, C and D are the lanes; the number is the row in `verified.md`.

## Rejected ideas

| Idea | Why |
|---|---|
| A third wording of lines 34, 39 or 80 | two tries moved other cases the other way; no probe separates the lines yet |
| Router without a safe default | inference: a wrong label has no recovery unless the writer can ask |
| Rubric from the checklist's unsourced criteria | no source for three of them; rule 9 stays |

## Fail-first tests

None. This folder changes no code.

## History

| Date | Event |
|---|---|
| 2026-11-10 | Four read-only lanes reported; three refute lanes graded every load-bearing claim; wording corrected |
| 2026-11-10 | Decision 1 answered yes by the owner. Probe ran, $2.447. Results in `probe-results.md`: three edits (line 34 widening without the caching example, line 39, line 80) drop `two-services` to 22/30, so the example is not the only cause and the share due to line 34 alone is not isolated; both W2 arms lift `h-rate-limit` to 30/30; master's own spread on h-rate-limit was 19, 23 and 25. Decisions 2 to 4 still open |
| 2026-11-10 | Decision 2 answered A by the owner. Router replay ran, $0.6312, one frozen prompt (`router-prompt.txt`), 80 tune cases x 3 runs. Results in `router-replay.md`: recall of 41/72 on ask calls, 12/144 false asks, median latency 2.6 s, 14 of 15 misrouted cases wrong on every run. Decisions 3 and 4 still open |
| 2026-11-10 | Like-for-like replay ran, $1.2141, master's prompt on the same 80 tune cases x 3 runs. Results in `single-prompt-replay.md`: master 214 of 216 calls against the router's 173 of 216, with an in-sample advantage for master. The prompts miss different cases. A second router version on tune cases was judged not worth running (PLAUSIBLE). Both prompts are untested on cases neither was fitted to |
| 2026-11-11 | Router tuning finished, $3.4380 of the $4 tuning cap. Results in `router-final.md`. v2 and v3 tie out-of-sample and both beat v1 by 5 cases on the confirm half and 1 on the fully unseen held-out set, v4a showed no gain on unseen data. The owner chose v2 as the final router prompt. The router is research only, not wired into the product, and adds about 2.1 to 2.6 seconds per request in front of the writer call |
