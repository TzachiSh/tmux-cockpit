# Router tuning and the final prompt

Run on 2026-11-10 and 2026-11-11, after README decision 2 (prototype the router), which the owner answered A. After the like-for-like replay the owner chose, in a separate question recorded on #789 (comment 6770734166), to keep tuning the router on Sonnet for accuracy only (that question's option B, not the README's decision 2 option B). The owner then chose v2 as the final prompt on 2026-11-11. Nothing here changes the shipped prompt or any code. The router is research only and is not wired into the product. The raw rows stay under `~/.cache/probe789e-*`, and the held-out rows were read only through counting scripts.

## The prompts

| Version | File | File sha256 (first 16) | Rig hash | What it is |
|---|---|---|---|---|
| v1 | `router-prompt.txt` | `bd1cd72d38e625d1` | `337ce79b67cd67b2` | The frozen first prompt, written from lanes A and C and the owner's purpose rule |
| v2 | `router-prompt-v2.txt` | `8369e136c3fb0a97` | `9b188bdfeaf99ab2` | The final prompt. v1 plus rule 2 widened to a plan or refactor of a whole module, and new rules 4 to 7 (a generated file with no named target, a bug report with no evidence, a review with no criterion, a message that commits the user to a fact only they know). Written by a lane that saw only the develop half |
| v3 | `router-prompt-v3.txt` | `681afadfe9999ccd` | `0e163380931b992f` | v2 with rules 4 and 6 widened, aimed at the two remaining develop misses |
| v4a | `router-prompt-v4a.txt` | `f09f50ce13e4e6ec` | `9a4d0804e9622eb5` | v2 with rules 2 and 6 rewritten as a missing goal or criterion, with the reason attached |
| v4b | `router-prompt-v4b.txt` | `013c8d1d9fdc4306` | `82fa622e1314e3d4` | v2 plus four example lines. Dropped after pass A |

## Method

- **Cases:** the 80 tune cases (24 ask, 48 write, 8 either), 3 runs each, `--no-explore`, model sonnet, hermetic. A case counts as right only when all 3 runs match its label. The either cases are counted apart.
- **Split:** the first alternating split put sibling cases of one family on both halves, so it was retired before any tuning. The family split links cases that share one or more rare phrases (in at most 3 of the 80 cases) or a case-id stem. It gave 59 families, develop 40 and confirm 40 with ask 12, write 24 and either 4 on each side, and 0 rare shared phrases across the halves. `develop.txt` sha256 `7e399264…`, `confirm.txt` sha256 `fa827f3d…`. The first rule failed two checks and the orchestrator amended it, which `RULE.txt` records.
- **Writer lane:** a separate lane wrote v2, v3 and v4a, and saw only the develop cases, the misses and the sourced criteria. The confirm half was run blind, as counts only, at most twice (v2 and v3). After the owner asked for a last round, the two straggler cases were opened to the writer and the confirm half stopped being blind for v4a and v4b.
- **Held-out set:** the 41 older held-out cases (6 ask, 30 write, 5 either). They are checked in at `tui/evals/panel/cases.txt` with `holdout: yes`, so "unseen" means no writer lane received any of their text. A check of every held-out body line of 30 or more characters found none in any writer artifact, in the pass files or in this record. The only hits are 5 lines of an old writer copy that also occur verbatim in tune cases. Run as counts only through the closed-set `label` field that PR #829 added. The label is "ask", "write" or "other", never reply text.

## Results

Ask and write cases only, 36 cases and 108 calls per set. Develop is in-sample for v2, v3, v4a and v4b, since each was written or edited after seeing develop misses.

| Prompt | develop (in-sample) | confirm | held-out (unseen by every writer lane) |
|---|---|---|---|
| v1 | 28/36 cases, 84/108 calls | 29/36, 89/108 | 34/36, 104/108 |
| v2 | 34/36, 104/108 | 34/36, 102/108 | 35/36, 105/108 |
| v3 | 36/36, 108/108 | 34/36, 102/108 | 35/36, 105/108 |
| v4a | 35/36, 106/108 | 36/36, 108/108 (partly seen) | 34/36, 105/108 |
| v4b | 34/36, 102/108 | 33/36, 101/108 | not run |

All either cases in every set were a one-word ask or write, with 0 errors and 0 unparseable replies. Confirm was unseen when v1, v2 and v3 were run, and partly seen for v4a and v4b, because their writer saw both straggler cases (the v4b confirm cell is partly seen too). v2's confirm counts were already known when v3 was written, but counts only, no case text. v1's develop and confirm numbers come from the first 80-case replay rows ($0.6312, in `router-replay.md`) filtered by the family split, not from pass 1.

Pairwise counts of cases at 3 of 3 (ask and write):
- Confirm: v2 and v3 pass the same 34 cases, 0 each way. v2 beats v1 by 5 cases (one-sided Fisher exact, v2 greater than v1: p = 0.076 by cases, which is not significant at 0.05, and p = 0.0048 by calls, which are not independent).
- Held-out: v4a right where v2 is not, 0, and the reverse 1. v4a against v3 the same. v2 right where v1 is not, 1, and the reverse 0.
- v1 already passes 34 of 36 on the held-out set, so that set is near ceiling and every difference there is one case.

## The two stragglers

`review-own-diff` and `split-module`, both labelled ask, were answered write on all 3 runs by v1, v2 and v3. Both requests end with a restrictive phrase ("just report problems", "just the plan") that states the output wanted. The owner's label says the focus of the review and the purpose of the split are still the user's call. Both prompts already had a rule that should fire, so the miss is how the rule is read: the stated output form is read as a complete specification.

Research on fixing that kind of miss found no published name for it and no measured prompt-level fix. Sources fetched on 2026-11-11 (all CONFIRMED on the page):
- Ambig-SWE (arxiv 2502.13069v2): underspecification is "missing information that would prevent an expert from being able to create a successful solution", and "prompt engineering alone proves insufficient for reliable underspecificity detection".
- LHAW (arxiv 2602.10525): removed information falls along Goals, Constraints, Inputs and Context.
- Anthropic's prompting guide: give the reason behind an instruction, and use 3 to 5 diverse examples.
- Over-asking risk: in Ambig-SWE the false-ask rate on fully specified inputs rose with stronger asking prompts for some models (Sonnet 3.5: 0.00, 0.24, 0.36 across neutral, moderate and strong) and did not rise for Sonnet 4 (0.08, 0.10, 0.03).

v4a followed the missing-slot idea and fixed both stragglers, but only on cases its writer had seen. It showed no gain on unseen data, and it labelled more held-out cases ask than the others (ask labels 28 against 24 for v2 and v3, and 23 for v1, with the same 105 of 108 calls right).

## The decision

The owner chose v2 on 2026-11-11. Reasons: it is the simplest prompt that carries the whole out-of-sample gain over v1, and v3's and v4a's extra rules are supported only by cases their writers had seen.

## Cost

| Step | Cost |
|---|---|
| Pass 1 (v1 on the retired alternating develop half, a superseded re-run), pass 2 (v2, develop), pass 3 (v3, develop) | $0.3156, $0.2620, $0.2564 |
| Confirm runs (v2, v3) | $0.2472, $0.2348 |
| Pass A (v4a and v4b on develop and confirm) | $0.9997 |
| Pass B (v1, v2, v3, v4a on the held-out set) | $1.1223 |
| Total tuning | $3.4380 of the $4 tuning cap the orchestrator set for this round inside the owner's $80 eval budget. The total excludes the earlier $0.6312 and $1.2141 replays. Costs are the `cost.tsv` figures, so pass A reads $0.9997 where its rows sum to $0.9995 and pass B reads $1.1223 where its rows sum to $1.1224 |

## Limits

- The confirm half is spent. The held-out set was used for this round. Any further edit would have only the four new blind cases, which is too few to conclude.
- v2 still misses two develop cases, `ci-workflow-spec` and `review-my-changes`.
- The router adds a call in front of the writer: for v2, a median latency of 2.2 s (p90 2.7 s) and a median cost of $0.0015 per call (mean $0.0021, first cold call about $0.003 to $0.005), over 363 calls through `claude -p` in the panel's own CLI path. Start-up dominates the latency. No end-to-end cost or latency was measured. A routed request pays the router call and then the writer call.
- Master's single prompt scored 214 of 216 calls on the same 80 tune cases, but it was tuned on them. Neither side has been compared on unseen cases other than the four blind cases still to be committed.

## Not tested

- Any router edit on unseen cases after the held-out set was used.
- The router with tools, or integrated into the product.
- The thinking and effort follow-up (approved plan, not started).
