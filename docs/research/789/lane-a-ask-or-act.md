# Lane A: how assistants choose between asking and acting

Question: how do published assistants, coding agents and research systems choose between asking and acting, and what measured evidence shows when asking helps or hurts?

Grades in the lane files are the first-pass grades with corrections folded in; `verified.md` has the refute-pass grade for each claim.

Read-only lane. Every source below was fetched on 2026-11-10. Grades: CONFIRMED (fetched, quoted), PLAUSIBLE (search summary only, page not opened).

## Findings

| # | Finding | Source | Grade |
|---|---|---|---|
| 1 | Under-asking is the common failure. 6,120 dialogues, 10 LLMs, 5 ambiguity dimensions, 6 simulated user personas. Quote: "consistent under-clarification bias". Also "answer prematurely, and performance degrades as dialogue depth increases". No numeric gain in the abstract | https://arxiv.org/abs/2512.21120 (ClarifyMT-Bench) | CONFIRMED, abstract only |
| 2 | Models see the ambiguity and still answer. Quote: "overwhelmingly default to direct answers". Retrieved context makes it worse. n not in the abstract | https://arxiv.org/abs/2605.25284 | CONFIRMED, abstract only |
| 3 | Agents cannot reliably tell clear from unclear tasks. Quote: "models struggle to distinguish between well-specified and underspecified instructions". Interaction improved performance by "up to 74%", a maximum; the metric and task count are not in the abstract (refute pass: the exact quote came back only as a paraphrase, the meaning holds) | https://arxiv.org/abs/2502.13069 (Ambig-SWE, ICLR 2026) | TIGHTENED, abstract only (see verified.md) |
| 4 | A multi-agent scaffold that separates "underspecification detection from code execution" reports a 69.40% task resolve rate on an underspecified SWE-bench Verified variant, across proprietary and open-weight LLMs (aggregation not stated), and beats a single-agent setup by an unquantified margin; it conserves queries on simple tasks. n not on the page. The 61.20% baseline is unverified and not reported here | https://arxiv.org/abs/2603.26233 | TIGHTENED, abstract only (see verified.md) |
| 5 | Vendor guidance gives both dials as system-prompt text: a `default_to_action` block ("infer the most useful likely action and proceed"), a conservative block ("default to providing information, doing research, and providing recommendations"), a reversibility rule ("hard to reverse... ask the user before proceeding"), and, inside the parallel-tool-calls block only, "Never use placeholders or guess missing parameters in tool calls" (a tool-call rule, not a general ask-or-act rule). The reversibility sentence also covers shared systems and destructive actions. On overtrigger the page says Opus 4.5 and 4.6 "might now overtrigger" on prompts built to reduce undertriggering on tools or skills, and "Claude 4.6 models are more proactive and might overtrigger"; both are about tool use, not about asking | https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/claude-prompting-best-practices | CONFIRMED |
| 6 | Claude Code's AskUserQuestion is meant for clarifying requirements before a plan is final; a user reports the plan agent sometimes skips it; the issue was opened 2025-10-31 and closed as resolved 2026-01-10 per a search summary, and "meant for clarifying requirements" is the reporter's reading, not an official statement | https://claudeissues.com/issue/10685-bug-plan-agent-doesnt-call-askuserquestion-despite-documentation-and-tool-access (direct fetch failed, ENOTFOUND) | PLAUSIBLE, search summary |
| 7 | Over-asking is named as a cost (AskBench, RegretBench arXiv 2607.21143, ClarifyMT; Dialogue SWE-Bench, CLAMBER, Noisy-ToolBench "Ask-when-Needed") but no number was fetched | search summaries | PLAUSIBLE, unopened |

## Inference for the panel (speculation, the lane's own; refute pass trimmed the claims)

- Split detect from write: one step labels "blocked by a missing fact" yes or no, a second step asks or writes. Finding 4 (one study, aggregation and baseline not stated) suggests it; it does not show a single prompt is the weak point.
- Ask only when a missing fact cannot be found by looking and a wrong guess is costly or hard to undo (finding 5, vendor prompt text, not measured evidence). "Use tools to discover missing details instead of guessing" is on the page; "state the assumption in the prompt" is the lane's own addition.
- Models lean toward answering (findings 1, 2). That each wording tweak shifts the bias both ways is the lane's own reasoning, matching the two #718 backfires; no finding here measures it, and the overtrigger quotes concern tool use.
- Test the decision on a fixed labelled set, scoring both errors, not by anecdote.

## Not found

- A measured over-asking cost with n and conditions.
- Cursor, Copilot, Aider or Codex plan-mode documents on when to ask.
- An official Claude Code docs page for AskUserQuestion.
- Full-paper n for findings 2 to 4.
- Deep-research products' clarify-first behaviour (not searched).
