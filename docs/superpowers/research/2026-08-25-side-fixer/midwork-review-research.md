# Mid-work review delivery: evidence survey

## 1. When should a mid-work review reach the agent?

**Claude Code's own message queueing (primary source, exact quotes):**

From https://code.claude.com/docs/en/interactive-mode ("Queue messages while Claude works"):

> "Type a message and press `Enter` while Claude is working. Claude Code queues the message instead of interrupting the turn, and lists the queued entries above the input box until it sends them."

> "Messages: if you queue a message while Claude is running tool calls, Claude Code passes it to Claude as soon as those tool calls finish, **within the same turn**. When the turn ends, Claude Code sends the messages that are still queued as **the next turn**, each as a separate message"

> "Commands and shell commands: Claude Code holds them until the turn ends, then runs them one at a time"

> "Press `Esc` to interrupt the turn instead. Claude Code keeps what you queued and sends it right away."

So Claude Code itself has *two* injection points already, both used today: (a) a **tool-call-boundary** delivery — a queued message reaches the model as soon as the in-flight tool calls finish, *before* the turn ends, and (b) a **turn-boundary** delivery for anything still queued when the turn ends. [E]

Esc from https://code.claude.com/docs/en/interactive-mode (keyboard shortcuts table):

> "`Esc` — Interrupt Claude, or close a dialog — Stop the current response or tool call mid-turn so you can redirect. Claude keeps the work done so far. If you have messages queued, Claude Code sends them next."

Docs-vs-behavior gap (bug, not feature): [anthropics/claude-code#36326](https://github.com/anthropics/claude-code/issues/36326) — "The documentation says pressing Enter while Claude Code is working should interrupt and steer it. In practice, Enter only queues the message... To actually interrupt and steer, the user must press Ctrl+C first, then Claude Code reads the queued message." [E] This confirms Enter = queue (not interrupt) is the actual, current, intended behavior — the interactive-mode doc excerpt above is the corrected/current version of that behavior; the older "how-claude-code-works" overview page the bug references is stale marketing copy, not the reference doc.

**Hooks docs, exact quotes**, from https://code.claude.com/docs/en/hooks:

- Stop: "Exit code 2: Prevents Claude from stopping, continues the conversation." Decision control: `decision: "block"` + required `reason`. `stop_hook_active` field: "indicating whether this is the first Stop hook invocation in the current turn... helps prevent infinite loops when hooks block stops repeatedly." [E]
- **Stop/SubagentStop `additionalContext` is newly supported** (v2.1.163+) but the reference page's SubagentStop section is *still wrong* as of this writing — [anthropics/claude-code#65495](https://github.com/anthropics/claude-code/issues/65495) quotes the doc text claiming "They do not support `additionalContext`" and quotes the changelog entry: "Hooks: Stop and SubagentStop hooks can now return `hookSpecificOutput.additionalContext` to give Claude feedback and keep the turn going without being labeled a hook error." Suggested combined shape: `{"decision":"block","reason":"...","hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"..."}}`. [E] — This is exactly tmux-cockpit's planned Stop-hook shape; confirm current CLI version supports the combined form before relying on it.
- PostToolUse: cannot block ("the tool already ran"); its `additionalContext` "is injected into Claude's next turn" per one summary, but a second source frames it as injected into the *same* ongoing tool loop rather than a full new "turn" — this is a naming ambiguity in third-party writeups, not something I could pin down with an exact primary-source sentence. What's firmly attested: PostToolUse fires only on tool *success*, and PreToolUse cannot carry `additionalContext` at all today ([anthropics/claude-code#15345](https://github.com/anthropics/claude-code/issues/15345), [#15664](https://github.com/anthropics/claude-code/issues/15664) are both open feature requests for that). [E]
- PreToolUse: `permissionDecision: "deny"` + `permissionDecisionReason` is the documented deny-with-reason shape. [E]
- UserPromptSubmit: exit code 2 "Blocks prompt processing and erases the prompt"; supports `additionalContext`; timeout lowered to 30s. [E]
- Rewake-loop warnings: `asyncRewake` hook docs: "Use this feature carefully to avoid infinite loops where the hook repeatedly wakes Claude with failures." `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP` caps Stop-hook blocks at 8 in a row by default, raiseable via the env var — explicitly "so a broken hook can't trap a session forever." [E]

**Anthropic's own real-time-steering feature requests** (open, unimplemented) argue FOR a boundary tighter than full-turn, but the concrete proposal Anthropic issue-authors converge on is *tool-call boundary*, not arbitrary mid-tool-call interrupt:

[anthropics/claude-code#30492](https://github.com/anthropics/claude-code/issues/30492) — full proposal: "Between tool calls (Read, Edit, Bash, etc.), Claude Code already runs PreToolUse hooks. A native priority message system could use this same interstitial window to deliver queued user messages as high-priority context, tagged so Claude recognizes them as steering input." Proposes tagging as `[steering]` vs "conversational." [E, still open/unimplemented as a distinct feature — the shipped "deliver queued message once in-flight tool calls finish" behavior above is Anthropic's actual answer to this request, already covering the core ask].

## 2. Interrupting mid-turn: harmful or useful? Evidence.

Evidence leans **harmful / lossy**, not useful, for a destructive interrupt:

- [anthropics/claude-code#1172](https://github.com/anthropics/claude-code/issues/1172): "When using Claude Code with tools that involve code modifications, there's a critical issue where user interruption leads to complete loss of context about what was attempted and which files were being modified... Tool returns no output at all when interrupted... Context is completely lost, requiring the conversation to restart from scratch." Proposed fix (unshipped): return partial results (files being analyzed/modified, current step) even on interrupt. [E]
- [anthropics/claude-code#53674](https://github.com/anthropics/claude-code/issues/53674) (found via search, not fetched in full): ESC interrupt removes the user's submitted prompt from chat history, so a follow-up has nothing to attach to — another lossiness report. [E, title/summary only]
- [anthropics/claude-code#30492](https://github.com/anthropics/claude-code/issues/30492) itself frames Esc as "destructive: it discards in-progress work and forces a restart," motivating a non-destructive alternative. [E]
- Countervailing: the current interactive-mode doc says Esc "keeps the work done so far" and immediately delivers anything queued — so Anthropic's shipped behavior already tries to minimize loss (partial work kept, queued input delivered right after). Whether that fully addresses the GitHub reports of *total* context loss for in-flight tool calls specifically is unresolved in the docs; the reports above are about tool-call-level interruption, which is a narrower and messier case than turn-level Esc.
- No evidence found of anyone in the ecosystem shipping a separate "urgent" interrupt channel distinct from the harness's own Esc/Ctrl+C. The `revdiff`, `hunk`, `claude-review-loop`, `plannotator` tools surveyed below all either (a) suspend the agent entirely during human review (revdiff), (b) wait for a natural Stop/PermissionRequest boundary (claude-review-loop, plannotator), or (c) let the agent poll on its own schedule (hunk) — none add a mid-tool-call "drop everything now" review-injection path. The only "urgent, mid-execution, non-destructive" proposal in the wild is Anthropic's own open #30492/#64624 issues, which are feature requests, not shipped or evidenced-effective behavior. **This is silence, not a verdict against it** — nobody has built and shipped the `!` marker idea to observe it failing or succeeding.

## 3. Separate agent / team / queue patterns surveyed

| Tool | WHEN review lands | Who addresses it | State visible to human |
|---|---|---|---|
| **Claude Code `/code-review`** (local) | On explicit command (`/code-review`), runs as a **background forked subagent** so it doesn't fill the main conversation; findings "arrive in your conversation when the review completes" — i.e., delivered as a message, subject to the same queue/turn-boundary rules as any other input. [E, https://code.claude.com/docs/en/code-review] | The **same session** that requested it (not a separate teammate) reads the findings and can fix them; re-running later shows each prior finding "marked... as fixed, skipped, or no change needed." [E] | Findings rendered as text or via `ReportFindings` tool (desktop app); re-review updates a persistent findings list with per-item status. |
| **Anthropic's hosted Code Review (GitHub PR bot)** | On PR open / every push / manual `@claude review`, per repo config. [E] | A **fresh multi-agent fleet on Anthropic infra** per review — "multiple agents analyze the diff... in parallel," explicitly *not* the agent that wrote the code (this is a GitHub bot, stateless per review). Push-triggered mode **auto-resolves threads when the flagged issue is fixed** on the next push. [E] | Inline PR comments with severity (🔴 Important / 🟡 Nit / 🟣 Pre-existing), a check-run summary table, and a machine-readable `bughunter-severity` JSON your CI can parse. Check run is always "neutral" — never blocks merge by itself. [E] |
| **claude-review-loop** ([hamelsmu/claude-review-loop](https://github.com/hamelsmu/claude-review-loop)) | **Stop-hook boundary**: when Claude tries to stop after finishing the task, the Stop hook intercepts, writes a runner + prompt, and **blocks the stop** with instructions to run Codex as the reviewer. [E, via fetch] | A **different tool (Codex)** does the reviewing; the **same Claude session** then reads `reviews/review-<id>.md` and addresses agreed items before it's allowed to stop. `stop_hook_active` prevents the rewake loop once addressing-phase exit is allowed. | State machine literally named `task` → `addressing`, tracked in `.claude/review-loop.local.md`; user sees Claude implement, then sees Codex output stream, then sees Claude address findings — largely unattended. |
| **revdiff / revdiff-planning** ([umputun/revdiff](https://github.com/umputun/revdiff)) | Two modes: (a) **skill pull** — agent or user runs `/revdiff`, which "suspends [the agent], hands the terminal to revdiff, then returns captured annotations to the agent" (fully synchronous, agent frozen during human review); (b) **plan-mode hook push** — `revdiff-planning` hooks `ExitPlanMode`, opens revdiff automatically on the plan text, and **returns annotations as the PreToolUse-style deny reason**, forcing a plan revision before any code is written. [E] | Same agent, immediately, in a loop: "the agent plans code changes, applies them, then reruns revdiff_review with the same args until no annotations are captured." | Purely sequential handoff — human TUI is the only UI, no agent execution overlaps it. |
| **plannotator** ([backnotprop/plannotator](https://github.com/backnotprop/plannotator)) | **PermissionRequest hook on `ExitPlanMode`** — browser UI opens the instant the agent tries to exit plan mode, i.e. a designed decision boundary the agent already stops at, not an interrupt. Deny returns "structured feedback" back to the agent. [E] | Same agent, next turn. | Browser-based annotation UI; approve/deny gates the tool call itself. |
| **hunk** ([modem-dev/hunk](https://github.com/modem-dev/hunk)) | Agent-initiated **poll via a local daemon** — "coding agents can connect to [a local server], leaving automated review comments directly in the diff view," and a `hunk-review` skill lets the agent "control live Hunk diff-review sessions... while the developer operates the interactive terminal UI" concurrently. Docs did not specify a push/notify mechanism for *human→agent* comments — only agent→hunk (adding annotations) was documented in the page fetched; the reverse direction (agent discovering new human comments) is unspecified in the docs I could reach, so this is a gap, not a confirmed design. | Not confirmed from docs (see gap above). | Sidecar notes rendered inline in the diff view, tagged by author (agent vs. human). |
| **difit** ([yoshiko-pg/difit](https://github.com/yoshiko-pg/difit)) | Fully offline/manual: human reviews in a local web diff viewer, then **exports comments as structured prompts** the human pastes to the agent. No hook/automatic delivery at all. [E] | Human decides when to hand off; no automated boundary. | Comment list in the local web UI; no agent-visible state. |
| **claude-squad / Conductor / vibe-kanban / herdr** (multi-agent managers) | All are **orchestration/visibility layers, not review-injection mechanisms** — they show agent state (working/blocked/done) and let a human diff-review and merge, but none of the fetched material describes a boundary-aware mid-turn review-injection primitive of their own; they lean on the underlying CLI's own queueing/Stop hooks. vibe-kanban's Kanban states are To Do → In Progress → **Review** → Done, and its human-in-the-loop nodes "persist the run as pending and show a static amber card; answering resumes the chain automatically." [E for vibe-kanban states/HITL] | Same agent (each Kanban card = one worktree = one agent). | Kanban column / amber "pending" card is the closest documented analogue to your "pending/addressing/done" ask. |

**Fresh vs. same-context reviewer** — the one clear, sourced data point is Anthropic's own hosted Code Review product, which is explicitly a **fresh fleet with no memory of the authoring session** (it's a stateless GitHub-bot review keyed off the PR diff, not a continuation of whatever session wrote the code) — this is the strongest "fresh context reviews better" signal available, though the docs justify it operationally (parallel specialized agents, verification step to cut false positives) rather than with an explicit "fresh eyes catch more" claim. [E, inferred framing — I] By contrast, Claude Code's local `/code-review` explicitly runs "as a background subagent with its own context window" even when invoked from inside a session that wrote the code — so even the *local* tool deliberately forks to a fresh context rather than reviewing in the authoring session's own context, and self-review agents (claude-code-reviewer subagent example in the sub-agents docs) are written to **rediscover context via `git diff` rather than rely on conversation history** — a second, independent signal toward fresh-context review being the intended pattern. [E] I found no primary source making an explicit for/against causal claim ("fresh review catches N% more bugs") — this is architectural convention, not measured evidence.

## 4. Agent deferring a review ("not now, after this step")

No Anthropic-documented convention found for the agent explicitly acknowledging-and-deferring a mid-work review with a visible state. The closest analogues:

- The Stop-hook `decision:block` + `reason` shape *is* effectively a forced, harness-level defer: the agent is told "you're not done, do X" and must continue — but this is imposed on the agent by the harness, not a choice the agent signals back to the human ("got it, will handle after this step").
- vibe-kanban's HITL "pending, amber card" is the closest UI convention for "acknowledged, waiting," but it's the *human's* input being deferred-for by the agent (agent blocked on human), the inverse of what you're asking (human's review deferred by agent). [E]
- General code-review-agent convention (found via generic search, not a primary Anthropic source): "deferring with a TODO comment for real concerns that is out of scope for the current PR, used sparingly" — this is a written artifact (a TODO/comment) as the defer signal, not a live status line. [I, weak — generic web summary, not verified against a fetched primary source]

**This is a genuine gap in the ecosystem** — no tool surveyed has a first-class "acknowledged, deferred to next boundary" status distinct from either fully blocking (Stop hook) or fully ignoring until polled (hunk-style).

## 5. End-of-work review conventions

From Anthropic's own Code Review docs (https://code.claude.com/docs/en/code-review), the `REVIEW.md` **"Re-review convergence"** tuning point, quoted directly:

> "**Re-review convergence**: tell Claude how to behave when a PR has already been reviewed. A rule like 'after the first review, suppress new nits and post Important findings only' stops a one-line fix from reaching round seven on style on style alone."

Other directly-relevant, sourced conventions from the same doc:

- **Nit caps**: "cap how many 🟡 Nit comments a single review posts... A cap like 'report at most five nits, mention the rest as a count in the summary' keeps reviews actionable." [E]
- **Auto-resolve on fix**: push-triggered review mode "auto-resolving threads when you fix flagged issues" — no separate accept/dismiss action needed by the human. [E]
- **Summary-first shape**: "ask for the review body to open with a one-line tally such as `2 factual, 4 style`, and to lead with 'no factual issues' when that's the case." [E]
- **Never blocks merge by itself**: "Findings are tagged by severity and don't approve or block your PR, so existing review workflows stay intact... The check run always completes with a neutral conclusion so it never blocks merging." [E] — explicit design choice against an accept-all/approval gate living inside the review tool.
- **Reactions (👍/👎) don't trigger anything**: "Reactions do not trigger a re-review or change anything on the PR" — explicitly NOT a re-read-the-whole-diff or accept-all mechanism. [E]
- **Retrigger, don't loop**: failed/timed-out review runs are "best-effort... never blocks your PR, but also doesn't retry on its own" — you comment `@claude review` to get one fresh attempt, not an automatic unbounded retry loop. [E]

What NOT to do at the end, distilled directly from the above (all sourced): don't let nits multiply unboundedly (cap them + suppress after round 1), don't require an accept-all UI action (auto-resolve on fix instead), don't gate merge inside the tool (severity is advisory, check run stays neutral), don't retry failed reviews automatically (comment-triggered, best-effort only).

## 6. DO / DON'T lists and recommendations for tmux-cockpit

### DO (mid-work)
- Deliver at the **tool-call boundary within the current turn if the agent is mid-tool-call, else at Stop** — this is exactly what Claude Code's own queued-message delivery already does ("Claude Code passes it to Claude as soon as those tool calls finish, within the same turn") [E, code.claude.com/docs/en/interactive-mode]. Your Stop-hook plan is the right fallback for the common case (agent between turns), and matches `claude-review-loop`'s proven pattern of intercepting Stop, writing review content to a file, and blocking with `decision:block`+`reason` (or the newer combined `reason`+`hookSpecificOutput.additionalContext` shape once you confirm your installed CLI version supports it — v2.1.163+, per issue #65495). [E]
- Clear the queue before blocking, exactly as planned — matches the documented purpose of `stop_hook_active` ("helps prevent infinite loops when hooks block stops repeatedly") and the `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP` safety valve (8 blocks default). [E]
- Prefer PostToolUse `additionalContext` only if you need review comments injected *before* the current turn naturally ends (i.e., genuinely mid-turn, not just mid-tool-call) — it's the one documented event that can add context without blocking a tool or the stop. Note PreToolUse cannot carry `additionalContext` today (open feature requests #15345/#15664), so "inject before the next tool call" isn't available as a deny-free mechanism. [E]

### DON'T (mid-work)
- Don't build a destructive "urgent" interrupt that cancels an in-flight tool call to force feedback through early — GitHub evidence (#1172, #53674) shows Claude Code's own interrupt path already loses context/tracking on tool-call-level interruption; forcing an equivalent through your own tooling inherits the same risk with none of the harness's own mitigations (queued-message auto-delivery, kept partial work). [E]
- Don't invent a distinct "urgent marker" channel speculatively — the evidence is **silent, not against it**: nobody in the surveyed ecosystem (revdiff, hunk, claude-review-loop, plannotator, claude-squad, vibe-kanban, Conductor, herdr) has shipped a separate urgent lane distinct from the harness's own interrupt/queue; the only "yes we want faster-than-turn-boundary steering" evidence is Anthropic's own open, unimplemented feature requests (#30492, #64624), and Claude Code's shipped answer to that ask is already the tool-call-boundary delivery described above — which your Stop-hook queue can't reach mid-turn, but doesn't need to for the common review case (agent between turns). Recommendation: **skip the `!` marker for v1**; it solves a problem (mid-tool-call urgency) none of the surveyed prior art has actually needed to solve, and the one prior-art shape that comes closest (a hard interrupt) has documented downsides. Revisit only if users report the Stop-hook boundary is measurably too slow in practice.
- Don't let the review queue grow unboundedly across multiple Stop-hook cycles — mirror `claude-review-loop`'s single task→addressing transition rather than accumulating N unaddressed review batches; if the human queues a second batch while the first is still being addressed, your gate should fold it into the still-open addressing phase rather than opening a third phase.

### DO (end)
- Cap nits and suppress repeats after round one — directly sourced from Anthropic's own `REVIEW.md` "re-review convergence" and "nit volume" guidance. [E]
- Auto-resolve/mark-done a review item once the diff shows it's fixed, rather than requiring an explicit human accept click — matches push-triggered Code Review's auto-resolve behavior. [E]
- Keep your gate advisory (never block completion by itself) unless the user explicitly wants a hard gate — matches "neutral conclusion... never blocks merging" being a deliberate design choice, not an oversight. [E]
- Summarize before detailing (tally line first) — matches the documented `REVIEW.md` summary-shape guidance. [E]

### DON'T (end)
- Don't require an "accept all" UI action as the only way to close out a review batch — Code Review's model is fix-triggers-auto-resolve, not accept-triggers-close.
- Don't re-read the whole diff on every re-review pass — Anthropic's guidance is explicitly to narrow scope after round one (Important-only), not to redo full-diff analysis each time.
- Don't let a failed/errored hand-off retry itself automatically — Code Review's own failed-run handling is "best-effort... doesn't retry on its own," requiring an explicit re-trigger.

## Sources
- [Hooks reference](https://code.claude.com/docs/en/hooks) — Stop/SubagentStop/PostToolUse/UserPromptSubmit/PreToolUse, stop_hook_active, CLAUDE_CODE_STOP_HOOK_BLOCK_CAP, asyncRewake warning [E]
- [Interactive mode](https://code.claude.com/docs/en/interactive-mode) — queued message delivery timing, Esc behavior [E]
- [Code Review](https://code.claude.com/docs/en/code-review) — re-review convergence, nit caps, auto-resolve, severity, neutral check run, local `/code-review` as background forked subagent [E]
- [anthropics/claude-code#65495](https://github.com/anthropics/claude-code/issues/65495) — Stop/SubagentStop additionalContext doc gap + changelog quote [E]
- [anthropics/claude-code#36326](https://github.com/anthropics/claude-code/issues/36326) — Enter-queues-not-interrupts, Ctrl+C needed to interrupt [E]
- [anthropics/claude-code#1172](https://github.com/anthropics/claude-code/issues/1172) — context/tracking loss on tool-call interruption [E]
- [anthropics/claude-code#30492](https://github.com/anthropics/claude-code/issues/30492) — priority/steering channel proposal, tool-call-boundary mechanism [E]
- [anthropics/claude-code#15345](https://github.com/anthropics/claude-code/issues/15345), [#15664](https://github.com/anthropics/claude-code/issues/15664) — PreToolUse additionalContext still unsupported (open requests) [E]
- [hamelsmu/claude-review-loop](https://github.com/hamelsmu/claude-review-loop) — task/addressing state machine, Stop hook block+reason [E]
- [umputun/revdiff docs](https://revdiff.com/docs) — skill-pull suspend/resume, plan-mode deny-reason push [E]
- [backnotprop/plannotator](https://github.com/backnotprop/plannotator) — PermissionRequest hook on ExitPlanMode [E]
- [modem-dev/hunk](https://github.com/modem-dev/hunk) agent-workflows.md — agent→hunk comment API; reverse direction undocumented [E for what's there; gap noted]
- [yoshiko-pg/difit](https://github.com/yoshiko-pg/difit) — manual export-as-prompt, no automated handoff [E]
- vibe-kanban (per web summaries, not a primary doc fetch) — Kanban states, HITL pending/amber card [I, secondary source]
- claude-squad, Conductor, herdr (per web summaries) — orchestration/visibility layers, no distinct review-injection primitive found [I, secondary source]
