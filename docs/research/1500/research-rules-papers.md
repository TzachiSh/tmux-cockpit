# What recent papers say about the auto-answer rules

tmux-cockpit-dd [8f35e6], 2026-11-27. For #1500 (R2, R3, R6, R7, R8, R10, D5, D6), part of #1521. Read-only
research that adds papers to `research-user-rules.md`, which covered how other tools do it.

## Question

Do studies of permission rules and approval habits back the frozen design, and does any of them
argue for a change before #1508 to #1510 are built?

## Sources

Each quote was read in the paper's HTML on arxiv.org. I read the abstract, the limits section
and the passages quoted here, not every experiment.

1. Salerno, Gao, Gay, Serebrenik, Leitner, "Let the Agent Do It? How Software Practitioners
   Understand and Make Permission Decisions in Agentic AI Assistants", arXiv 2610.06047v1.
   Interviews with 18 practitioners, then a survey of 115.
   - Permission systems should "avoid treating repeated approvals as stable preferences".
   - "repeated approval may reflect fatigue rather than preference".
   - They should "make reversibility clearer" and "distinguish rejecting a single action from
     rejecting an entire approach".
2. Yan, "Do User-Authored Permission Policies Improve Protection Against AI Agent Overreach?",
   arXiv 2608.27443v2. 113 people without a software background, a simulated day of 18 actions
   with 7 overreach actions, three conditions.
   - "POLICY blocked less overreach than HITL (-20.1 percentage points, 95% CI [-32.1, -8.1])".
   - "participants chose 'ask' for 114 of 140 POLICY rules, returning most overreach actions to
     runtime".
   - "Of the 148 overreach actions executed in POLICY, 133 followed human approval and 15 ran
     automatically under 'allow' rules."
   - "Ask is not a neutral compromise." Interfaces "could preview representative actions that
     would return as prompts".
3. Turan, "Oversight Has a Capacity: Calibrating Agent Guards to a Subjective, Fatiguing Human",
   arXiv 2606.08919v1.
   - "more human oversight can make a system less safe" once the reviewer tires.
   - Reviewers "only moderately agree on what is risky (Fleiss' kappa = 0.52)".
   - Its own limits: "The inverted-U is simulated, not measured", the reviewers are "LLM
     personas, a proxy for human annotators", and the set is 125 actions.
4. Wu, Chen, Sun, Shi, Lyu, Lo, "'Shall We Approve?' Consequence-Aware Approval for Autonomous
   Coding Agents", arXiv 2610.40753v1.
   - Existing approvers "base their decisions on the text of the request and command. Yet a
     command's behavior depends on the code it invokes and the state of the workspace".
5. Ou, "Progressive Autonomy as Preference Learning", arXiv 2605.19151v1. It learns an
   allow, block or ask region from approve and deny feedback. Its evaluation is a simulation
   with synthetic annotators.

## Findings

| # | Our design | What the sources say | Verdict |
|---|---|---|---|
| 1 | R6: rules match facts the cockpit reads, never question text. Facts are re-read when acting | Source 4: approvers that read only the request text miss what the action does | Keep |
| 2 | R8: a built-in never-list. R2: risk and undo tags in the popup | Source 1 asks for clearer reversibility. Source 2: most overreach ran after a person approved it, not under a rule | Keep. The popup matters as much as the rules |
| 3 | R7 and D5: promotion is offered after 5 matching shadow answers in a row | Source 1: systems should not treat repeated approval as a stable preference, since it may reflect fatigue | Adapt, see below |
| 4 | D6 and R10: 3 answers per rule per hour, a notification for each | Source 3: attention is finite, and more prompts can lower safety. That result is simulated | Keep. Watch the notification count once rules go live |
| 5 | R3: the asking agent writes the risk tag | Source 3: raters agree only moderately on "risky" | Adapt: the popup shows the risk tag as the agent's claim (#1506) |
| 6 | R6: a fixed action set, no learned model | Source 5 offers a learned gate, tested only in simulation | Ignore |

### Finding 3 in numbers

This is my own arithmetic, not from a paper. A Wilson 95% interval on k matches of k gives this
lower bound on the true agreement rate:

| Matches in a row | Lower bound |
|---|---|
| 5 | 57% |
| 10 | 72% |
| 20 | 84% |
| 35 | 90% |

The bound treats each match as an independent draw. A rule that checks facts is more regular
than that, so read the table as a rough guide. It still says that 5 in a row is thin evidence
when taken alone.

## Recommendation

For the owner, through the orchestrator. Nothing here blocks #1508 to #1510.

1. **D5: the promotion offer shows its evidence, not only the count.** For each matching shadow
   answer it lists the facts the rule read and the owner's answer, from the log R10 already
   keeps. Cost: part of #1510's recent list, no new store.
2. **D5: the owner picks N again with the table above.** A merge can't be undone in this repo, so
   the merge rule may deserve a higher N than cleanup-lane or continue-queue. Cost: one number
   per action in the rules file.
3. **Shadow answers given in a burst count once.** Source 1 warns that repeated approval may
   reflect fatigue. Counting a burst once is my reading of that, not a tested design. One way is
   to count at most one match per rule per hour, which reuses D6's window. Cost: a rule in
   #1510's counter.

## Not known

- Whether any of this holds for one expert owner. Source 2 studied people without a software
  background, and source 1 is self-report.
- Whether fatigue shows up at our volume. No one has counted the owner's asks per hour.
