# Lane B: a routing or classification step before generation

Question: what designs put a routing or classification step before generation, what do they cost in latency and money, how do they fail, and how do teams evaluate a router with few labelled cases?

Read-only lane. All sources accessed 2026-11-10.

## Findings

| # | Finding | Source | Grade |
|---|---|---|---|
| 1 | Routing and prompt chaining are standard workflow patterns. Quote: "Routing classifies an input and directs it to a specialized followup task." and "Routing works well for complex tasks where there are distinct categories". Chaining "trade[s] off latency for higher accuracy" | https://www.anthropic.com/engineering/building-effective-agents | CONFIRMED |
| 2 | The same page lists routing easy questions to smaller models: "Routing easy/common questions to smaller, cost-efficient models like Claude Haiku 4.5" | same | CONFIRMED |
| 3 | Learned cost routers exist (strong vs weak model). Quote: "significantly reduces costs-by over 2 times in certain cases". Abstract only, authors' own claim, and it is model selection, not scope classification | https://arxiv.org/abs/2406.18665 (RouteLLM, v4 Feb 2025) | CONFIRMED, does not apply directly |
| 4 | Misrouting is defined as a risk: "the event where the model set fails to cover any ground-truth LLM." The method allows abstaining | https://arxiv.org/html/2603.06616v1 (RACER) | CONFIRMED |
| 5a | UCCI: expected calibration error falls from 0.12 to 0.03, on 75,000 queries of a production NER workload, by isotonic calibration of token-margin uncertainty for cascade escalation (not scope classification) | https://arxiv.org/abs/2605.18796 | CONFIRMED, abstract (refute pass; the first pass had n unknown) |
| 5b | RouteNLP "500 calibration examples per tier and task" and Signed Rescue Routing "splits each task in half, split fixed before training" | arxiv 2604.23577, 2609.07786 | UNVERIFIED: the abstracts say neither. Dropped from the evidence. Signed Rescue Routing's abstract has "TBD" size placeholders and an odd fit, so the id may be mis-cited |
| 6 | "Calibration queries must resemble the real workload" | cited arxiv 2501.01818 | REFUTED as cited: that abstract is about "confounder gadgets", query-independent token sequences that attack routers. Dropped |
| 7 | TwinRouterBench: a static track of 970 router-visible prefixes from 520 instances across 5 benchmarks, each paired with an execution-verified target tier, plus a dynamic track on the 500-case SWE-bench Verified suite. It criticises earlier benchmarks for not testing whether a cheaper model "preserves downstream task success". It does not explicitly analyse error propagation across steps | https://arxiv.org/abs/2605.18859 | CONFIRMED, abstract; the first pass's "error propagation" wording overreached |

## Price and cost (fetched; token counts assumed)

- Price page https://platform.claude.com/docs/en/about-claude/pricing, fetched 2026-11-10: Claude Haiku 5.5 (prompts up to 100,000 tokens) "$0.10 / MTok" input, "$0.50 / MTok" output. Haiku 4.5 is $1 / $5.
- Assumed router call: about 1,500 input tokens and about 20 output tokens. That is about $0.00016 per call on Haiku 5.5, about $0.0016 on Haiku 4.5. The token counts are assumptions, not measurements.
- Money is negligible. Latency is the real cost, and no latency figure was fetched. Measure it on the owner's machine.

## Inference for a two-step panel (speculation, the lane's own)

Refute pass: three points go beyond the graded findings. (a) "no recovery" and "bias toward the cheaper wrong answer" lean on RACER, which abstains at the model-set level, not the label level. (b) "report counts, not percentages" and "fix the split before tuning" lost their only source (finding 5b). (c) "run each case several times" has no source. "Latency is the real cost" is an assumption nobody measured.

- The router outputs a closed label set (for example clear, ask-one-question) with a safe default on parse failure. Each side then carries a smaller prompt with fewer rules, so rewording one side cannot move the other's cases.
- Main risk is error propagation: a wrong label sends the case to the wrong writer with no recovery. Mitigation: make the writer able to ask too, and bias toward the cheaper wrong answer.
- Evaluate by replaying the labelled cases through the router alone. Report a confusion matrix per label and name the misrouted cases. The lane's own practice, no source: fix the split before tuning and keep a few cases held out, report counts rather than percentages at a small n, and run each case several times.

## Not found

- Measured latency of an added Claude CLI call, or of router-then-generate chains.
- A source on how small a labelled set can be before router evaluation becomes unreliable.
- Failure studies of prompt-chaining routers, as opposed to model-selection routers.
- RouteNLP and Signed Rescue Routing: the abstracts do not support the claims first made (finding 5b). UCCI and TwinRouterBench were confirmed from their abstracts only, not the full papers.
