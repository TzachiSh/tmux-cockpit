# Judge trust and small-N statistics for the panel eval

tmux-cockpit-dd [8f35e6], 2026-11-27. For #1405, and for #628 and #409. Read-only research. No
model was called, and no file under `tui/evals/panel/` was opened.

## Question

Are the numbers the panel eval prints sound for the sizes we run, and what should the Explain
eval (#1405) copy or change?

## What we do today (master cb881f50)

- **One rate.** `pass^k` is the share of cases whose every run passed, with a Wilson 95% interval
  over cases (`tui/examples/panel_eval.rs:2719`, `docs/tui.md:317-320`).
- **Two prompts.** `--compare` pairs rows by case id, prints each side's count per case and marks
  a case "up" or "down" (`panel_eval.rs:2205`, `:2256`). It prints each side's
  unpaired `pass^all` with a Wilson interval, then both sides' `pass^all` over the cases both
  graded in full, with no interval (`paired_pass_all`, `panel_eval.rs:2318`). Nothing covers the
  difference between the sides.
- **The judge.** A `--judge` run prints the judge's raw pass rate with a Wilson interval and its
  agreement with the keyword checks (`judge_line`, near `panel_eval.rs:1803`).
- **Calibration.** `--calibrate` prints TPR, TNR and kappa with a 95% interval against the owner's
  labels, per round and per split (`tui/examples/panel_eval/judge.rs:561`). #628 counts 118 labels.

## Sources

Each quote was read in the paper's HTML on arxiv.org. I read the abstract, the recommendations
and the passages quoted here, not the proofs or the full experiments.

1. Bowyer, Aitchison, Ivanova, "Position: Don't Use the CLT in LLM Evals With Fewer Than a Few
   Hundred Datapoints", arXiv 2503.01747v3.
   - Measured: the coverage of each interval method against its nominal level.
   - "only the Bayesian credible interval and Wilson confidence intervals achieve this for small
     N" (this = the nominal coverage).
   - "both CLT-based and bootstrap CIs show poor calibration".
   - For two models: "With small data, we recommend Bayesian intervals for the difference or odds
     ratio."
2. Miller, "Adding Error Bars to Evals", arXiv 2411.00640v1. It derives formulas and measures
   nothing new. Its recommendations include:
   - "When two models are being compared, conducting statistical inference on the question-level
     paired differences, rather than the population-level summary statistics".
   - "Reducing variance by resampling answers".
   - "Using power analysis to determine whether an eval (or a random subsample) is capable of
     testing a hypothesis of interest".
3. Lee and others, "How to Correctly Report LLM-as-a-Judge Evaluations", arXiv 2511.21140v4.
   - Measured: Monte Carlo simulations and judge runs on the Chatbot Arena benchmark. The
     correction itself is derived.
   - "Unless the LLM is perfectly accurate (i.e., q0 = q1 = 1), the naive estimator p in (3) is
     generally a biased estimator of theta." Here q1 is the judge's sensitivity (our TPR), q0 its
     specificity (our TNR), p the judge's raw pass rate and theta the true pass rate.
   - Its equation 7 is the corrected rate: theta = (p + q0 - 1) / (q0 + q1 - 1), with q0 and q1
     estimated from the calibration labels.
   - Its interval "incorporates variance contributions from both the test and calibration
     datasets".
   - It also proposes "an adaptive strategy for allocating calibration samples". I did not read
     that section.
   - Code: https://github.com/UW-Madison-Lee-Lab/LLM-judge-reporting (not run).
4. Arawjo, "How to Run Statistics over LLM Judges and Trust the Results", arXiv
   2609.35815v1.
   - Measured: Monte Carlo simulations of interval coverage and Type-I error at N<100.
   - "We strongly advise against bootstrap CIs at N<100."
   - "Bonett and Price (2012)'s method outperforms the Bayesian paired method of Bowyer et al.
     (2025) on worst-case coverage and Type-I error control while running much faster" (for
     intervals on paired binary data).
   - "for many numeric IRR metrics, false-positive risk peaks around 'almost perfect' agreement".
     That finding is about numeric scores. Our judge is pass or fail, so I do not know that it
     carries over.

Abstract only: BACON (arXiv 2607.16239) and "Measuring all the noises of LLM Evals" (arXiv
2512.21326). The second says paired prediction noise usually exceeds paired data noise, so
averaging over runs raises power.

## Findings

| # | Our practice | What the sources say | Verdict |
|---|---|---|---|
| 1 | Wilson interval on `pass^k`, cases as trials | Sources 1 and 4 back Wilson at small N and reject CLT and bootstrap | Adopt, already in place |
| 2 | `--compare` shows moved cases, no interval on the difference | Sources 2 and 4: infer on paired per-case differences. Source 4 names Bonett and Price for paired binary data | Adapt |
| 3 | The judge's raw pass rate, Wilson interval | Source 3: the raw rate is biased unless TPR = TNR = 1. Equation 7 corrects it from the TPR and TNR we already print | Adapt |
| 4 | kappa as the judge's headline figure | Source 3 works from TPR and TNR, not kappa. Source 4 warns about agreement metrics, for numeric scores | Ignore for now: the warning is about numeric scores. No cost |
| 5 | Case counts chosen by hand | Source 2: a power analysis says whether N cases can show the change we care about | Adapt for #1405 |

## Recommendation

For the owner, through the orchestrator. Each item is its own PR with rows. None needs a model
call.

1. **#1405 picks its Explain case count by a power calculation**, before any case is written.
   Cost: arithmetic in the plan. It tells us whether a run can show the change we want to see.
2. **`--compare` prints an interval on the paired difference in `pass^k`.** Cost: one function and
   its tests in `panel_eval.rs`, reusing the pairing of `paired_pass_all`, which returns only
   per-side totals today. The Bonett and Price formula has to be taken from their 2012 paper,
   which I have not read.
3. **A `--judge` run prints the corrected pass rate and its interval beside the raw one**, from the
   TPR and TNR of the last calibration. Cost: one function, plus a way to hand the calibration
   counts to the run. If the label set has few owner fails, the interval will be wide. That width
   is the measure of whether more owner labels (#628) are worth their cost.

## Not known

- How large the judge's bias is on our data. That needs the calibration counts, which I did not
  open.
- Whether equation 7's interval behaves well at our label count. Source 3 derives it with the
  delta method, which is a large-sample argument.
- Whether Bonett and Price suits `pass^k`, where a case passes only if all k runs pass.
