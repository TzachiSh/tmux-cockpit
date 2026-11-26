# What recent papers say about supervising agents, for the Explain redesign

tmux-cockpit-dd [8f35e6], 2026-11-27. For #935 (R3, R4, R6, D2) and #1405, part of #1521.
Read-only research. No model was called, and no file under `tui/evals/panel/` was opened.

## Question

Explain will show a flow with a status per step, what changed since the last look, and one key
that turns the answer into a prompt. What do studies of people supervising coding agents say
about those three parts?

## Sources

Each quote was read in the paper's HTML on arxiv.org. I read the abstract, the findings and the
passages quoted here, not every section.

1. Khan, Kumar, "Soei: Structured Observability of Evidence and Intent for Multi-Agent AI
   Software Engineering", arXiv 2610.39468v1. A corpus of 45 real coding-agent sessions.
   - "57.8% of to-dos that the agent itself marked completed after editing had no verification
     between edit and completion. A dashboard that renders 'completed' to-dos as delivered work
     would show these as done."
   - "'reported complete' and 'observed verified' are different quantities in real use".
2. Yan, Lei, Shi, Wang, Cao, "From Watching to Steering: Enabling Human-Coding Agent
   Collaboration through Execution Traces", arXiv 2611.04466v1. Interviews with 12 experienced
   users of coding agents, then a tool and user studies.
   - Participants "questioned the reliability of agent-generated summaries. Existing traces often
     describe claimed actions without verifiable evidence".
   - They "preferred representations grounded in observable actions and execution states".
3. Long, Shi, Mozannar, Murad, Hosn, "ParallelPilot: Supporting Coordination and Monitoring in
   Parallel AI Coding", arXiv 2609.33113v1. A formative study of 14 and a within-subjects study
   of 16.
   - Throughput rose by 63% and tracking effort dropped, but "these gains did not extend to
     perceived control or participants' perceived success in redirecting the agent".
   - Tools "should pair high-level awareness with low-cost paths back to the implementation
     evidence developers need to judge and steer agent work".
   - Its own limit: the formative participants were "primarily researchers and research
     interns".
4. Sahni and six others, "Rasoi: Visual Diffs for Evaluating Structural Changes Made by Coding
   Agents", arXiv 2611.16293v1. A week-long study with 12 developers on their own code.
   - Developers "needed to know whether an agent-made change stayed within scope, whether it
     touched unexpected modules".
   - "10 out of 12 participants agreed that Rasoi made them feel more confident in their own
     understanding of the system, while only 2 agreed that it increased their trust in the coding
     agent itself".
5. Ye and others, "Project Progress Desk", arXiv 2611.13287v2. A demo paper with no evaluation.
   Its bar shows "progress backed by receipts" dark and "an earlier, dated estimate" light.

## Findings

| # | Our design | What the sources say | Verdict |
|---|---|---|---|
| 1 | D2: the what-changed list comes from a plain-code collector, and an item without a sha or #PR is dropped | Sources 2 and 3: people want summaries tied to evidence they can reach | Keep |
| 2 | The flow's step status is a line the model writes (`%% status: ✓`, decision 2 of 2026-11-12) | Source 1: over half of agent-marked completions had no check behind them. Source 2: claimed actions without evidence are hard to trust | Adapt, see below |
| 3 | R6: one key turns the answer into a prompt or review request | Source 3: a good overview alone did not make people feel able to redirect the agent | Keep. R6 is the part that study's tool lacked |
| 4 | R4: what changed is a list of commits, PRs, issues and CI | Source 4: people ask first whether a change stayed within scope | Adapt, small |
| 5 | #1405 has no pass rule for Explain yet | Sources 1 and 2 point at one that needs no judge: is each claim grounded | Adapt |

### Finding 2

As I read #935, the collector grounds the what-changed list, but nothing grounds a ✓ in the
flow. The model reads the screen and writes the mark, so the mark is the agent's claim passed on.
Source 1 measured how often such claims lack proof in its own corpus.

## Recommendation

For the owner, through the orchestrator. Nothing here changes the renderer children that are
running now.

1. **A done mark needs a citation, like a what-changed item.** A step shows ✓ only when its
   status line names a sha, a #PR or a CI run that the collector also saw. Otherwise it shows as
   claimed, with a separate mark from the glyph table. Cost: one more field in the status line
   and one glyph, decided before the wiring PR.
2. **#1405's pass rule includes a grounding check in plain code.** Every ✓ and every what-changed
   item in an Explain reply must cite something that exists in the case's fixture. It runs with
   no judge call, so it costs nothing per run.
3. **What-changed says which top-level folders the commits touched.** It is a cheap stand-in for
   source 4's scope question, from `git log --name-only` in the collector. This one is my idea,
   not a tested design.

## Not known

- Whether any of this carries to a text panel in a terminal. All five tools are graphical.
- How often our agents mark a step done without proof. Source 1's rate comes from 45 sessions of
  other people's work.
- Whether the studies' samples of 12 to 16 people generalise.
