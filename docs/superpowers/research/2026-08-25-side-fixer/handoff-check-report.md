# Fact-check: uploaded "Mid-Work and End-of-Work Review Handoff" vs. our session research

Checked 2026-08-25. Every GitHub issue fetched through `gh api` (title + state + state_reason + body).
Every Anthropic doc fetched as raw markdown (`code.claude.com/docs/en/<page>.md`) and grepped, so
quotes below are verbatim from the live pages, not from a summarizer.

**Tally: 38 [E] verified · 4 [I] unverifiable/secondary-only · 5 [X] contradicted.**

---

## 0. Verdict up front

| Decision | Verdict | Strongest verified fact |
|---|---|---|
| **(1)** Running agent → review handed over at its next turn boundary via a `Stop` hook; no "paste now", no `!` | **SUPPORT** on the mechanism and the no-interrupt rule; **WEAKEN** on the words "turn boundary" | hooks.md, `Stop`: *"Runs when the main Claude Code agent has finished responding. **Does not run if the stoppage occurred due to a user interrupt.**"* — an Esc/interrupt path would silently skip the hook entirely, so "never interrupt" is not a preference here, it is what makes the delivery mechanism fire at all. |
| **(2)** No agent-side deferral convention; the human owns pending→sent→approved | **SUPPORT**, unambiguously | `#29224` verified `state: closed`, `state_reason: not_planned`. Its two siblings are closed the same way (`#29490` stop-word check-in, `#57263` per-message acknowledgment). The only `defer` primitive in the harness is `PreToolUse` `permissionDecision: "defer"`, and hooks.md scopes it out: *"Claude Code honors this value only in non-interactive mode with the `-p` flag. In interactive sessions it logs a warning and ignores the hook result."* |

Neither decision is overturned. One clause of (1) is factually wrong and one DO line in the uploaded
doc is stale — details in §3 and §5.

---

## 1. Claim-by-claim verification of the uploaded document

### 1A — Queueing semantics (the parent's special-attention item)

| # | Claim (uploaded doc) | Source cited | Verdict | Evidence |
|---|---|---|---|---|
| 1 | A typed message during a running turn is queued, not an interrupt | interactive-mode | **[E]** | Verbatim, interactive-mode.md L340: *"Type a message and press `Enter` while Claude is working. Claude Code queues the message instead of interrupting the turn, and lists the queued entries above the input box until it sends them."* |
| 2 | A queued message is delivered **mid-turn, at the next tool-call boundary**, not held to the next turn | interactive-mode | **[E]** | Verbatim L346: *"Messages: if you queue a message while Claude is running tool calls, Claude Code passes it to Claude as soon as those tool calls finish, within the same turn. When the turn ends, Claude Code sends the messages that are still queued as the next turn, each as a separate message"*. Independently confirmed by the maintainer closing `#64624` on 2026-08-17: *"a message you send while Claude is working shows as queued, but it's delivered to Claude at the next tool-call boundary within the current turn, not after the whole response finishes… The only time it waits until the end is when Claude is producing a text-only reply with no tool calls in between."* |
| 3 | Queued slash/shell commands are held to end-of-turn | interactive-mode | **[E]** | Verbatim L347. |
| 4 | Esc interrupts; queued items sent right away; work so far kept | interactive-mode | **[E]** | Verbatim L349 + keyboard table L36. |
| 5 | `#36326` — docs said Enter steers, it only queues; closed not planned | #36326 | **[E]** | Title, body quote and `closed/not_planned` all confirmed. |
| 6 | `tiann/hapi#888` — "flush at the next LLM pause… many users actually want an end-of-turn-only queue" | tiann/hapi#888 | **[I]** | Issue exists and is on-topic (*"feat(codex,claude): mid-turn steering — deliver queued messages at step boundaries, not only on full-turn completion"*, closed), but the quoted sentence is not verbatim in it. Third-party repo, no authority over Claude Code behavior; drop it. |
| 7 | `#69124` — queued message picked up only at the next step boundary, less immediate than Codex | #69124 | **[E]** | Verbatim in body. Doc omits that it is `closed/not_planned`. |
| 8 | `/btw` is a true side channel usable while Claude works | interactive-mode | **[E]** | Verbatim L522: *"**Available while Claude is working**: you can run `/btw` even while Claude is processing a response. The side question runs independently and doesn't interrupt the main turn."* **But**: L523 *"**No tool access**"* and the Q&A *"never enter the conversation history"* — so `/btw` is human→Claude read-only. It is **not** a review-delivery path. Rule it out explicitly. |

### 1B — Hook injection points

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 9 | Stop exit 2 → stderr delivered to Claude as the reason | **[E]** | hooks.md Stop decision control: *"A hook that blocks by exiting 2 routes the same way as `reason`: Claude receives the stderr message as the explanation for why it should continue."* (The doc cited DataCamp; the primary confirms it, so the secondary is unnecessary.) |
| 10 | `Stop`/`SubagentStop` `hookSpecificOutput.additionalContext` shipped in v2.1.163 | **[E]** | CHANGELOG.md L1611 verbatim: *"Hooks: Stop and SubagentStop hooks can now return `hookSpecificOutput.additionalContext` to give Claude feedback and keep the turn going without being labeled a hook error"*. hooks.md now documents it: *"unlike `decision: "block"` it is shown in the transcript as hook feedback rather than a hook error"* and *"the transcript labels it `Stop hook feedback` and no hook error notification is shown"*. |
| 11 | Docs-lag caveat: the live hooks page **still** says SubagentStop doesn't support `additionalContext` (#65495) | **[X]** | Stale. `#65495` is `closed/completed`, and hooks.md L2316 now reads: *"SubagentStop hooks use the same decision control format as Stop hooks, **including `hookSpecificOutput.additionalContext`** with `hookEventName` set to `"SubagentStop"`, for non-error feedback that keeps the subagent running."* No fallback path is needed for this. |
| 12 | `decision:"block"` + `reason` continues the turn "unless the response also sets `impossible: true`" | **[E, scope note]** | Quote is verbatim — but from hooks-guide.md L836, which is the **prompt-hook** (LLM-backed) response schema. hooks.md L3256 confirms: *"`impossible` … The model returns it with `ok: false`"*. A bash command hook has no `impossible` field; irrelevant to tmux-cockpit. |
| 13 | UserPromptSubmit `additionalContext` must nest under `hookSpecificOutput` | **[E]** | hooks.md L1318/L1330. |
| 14 | PostToolUse `additionalContext` is buggy/unreliable — "PostToolUse hooks cannot inject context visible to Claude" (#18427) | **[X]** | The issue exists and the quote is real, but it is `closed/not_planned` and dated 2026-01-15. hooks.md now documents PostToolUse decision control with `additionalContext`: *"String added to Claude's context alongside the tool result."* The uploaded doc's fallback recommendation (use `decision:"block"` because `additionalContext` is broken) rests on a stale bug. |
| 15 | PreToolUse deny-with-reason surfaces the reason to the model | **[E]** | hooks.md L1723 (*"For `"deny"`, shown to Claude"*) + *"A hook that blocks by exiting 2 routes the same way as `"deny"`"*. |
| 16 | `stop_hook_active` guards the rewake loop | **[E]** | hooks.md L2441 verbatim: *"The `stop_hook_active` field is `true` when Claude Code is already continuing as a result of a stop hook. Check this value or process the transcript to avoid blocking on a condition that will never resolve. Claude Code overrides the hook and ends the turn after 8 consecutive blocks."* |
| 17 | Block cap: `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP`, default 8, `0` disables, added v2.1.143 | **[E]** | All four parts verified. env-vars.md L358: *"Maximum number of consecutive times a Stop or SubagentStop hook may block the turn from ending before Claude Code overrides it and ends the turn anyway (default: 8). Set to `0` to disable the cap."* CHANGELOG L2046: *"Fixed stop hooks that block repeatedly looping forever — the turn now ends with a warning after 8 consecutive blocks (override via `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP`)"*. The uploaded doc sourced this from a third-party repo; the primary is env-vars.md + CHANGELOG. |
| 18 | `openai/codex-plugin-cc#248` — Stop gate blocking on infra errors → rewake loop | **[E]** | Issue exists, **open**: *"Stop review gate blocks Claude on infrastructure errors, causing rewake loops"*. |
| 19 | Hook timeouts: command/http/mcp_tool 10 min; UserPromptSubmit 30s; MessageDisplay 10s | **[E]** | hooks-guide.md L939 verbatim. |
| 20 | `#34600` — Stop exit-2 feedback mislabeled "Stop hook error" | **[E]** | Verbatim; `closed/not_planned`. The `additionalContext` path documented in #10 is the actual fix. |
| 21 | Plugin-vs-`.claude/hooks/` exit-2 continuation bug (#10412) → install via `.claude/hooks/` | **[X]** | Issue verified verbatim, but `state: closed`, `state_reason: **completed**` (2025-10, fixed; closing comment points at the confirmed fix in #10875). This DO line is stale advice about a fixed bug. |

### 1C — Interrupting mid-turn

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 22 | Esc stops a wrong-path response while preserving context | **[E]** | Primary confirms without the secondary: interactive-mode L36 *"Claude keeps the work done so far."* |
| 23 | wmedia two-key distinction (Esc = stop cold; Enter = nudge) | **[I]** | Third-party blog; consistent with the primary, adds nothing. |
| 24 | `#17505` — accidental ESC interrupts without confirmation, lost work | **[E]** | Verbatim; `closed/not_planned`. |
| 25 | `#3003` — interrupting during tool calls permanently corrupts the conversation | **[E, aged]** | Verbatim, but note: opened 2025-07-04, `closed/**duplicate**`, reported against a 2025 build. Real, but it is the weakest leg of the "interrupts are harmful" argument, not the strongest. |
| 26 | `#21451` — "DON'T interrupt during an in-flight edit — abandoned edits corrupt the codebase" | **[X on the inference]** | The quote is verbatim, but the issue is titled *"Claude aborts code edits mid-process **when tokens run out**"* — it is about token exhaustion, not user interruption. It does not support a claim about interrupts. `closed/duplicate`. |
| 27 | `#23350` — abort state persists, auto-interrupting later tool calls | **[E]** | Verbatim; `closed/not_planned`. |
| 28 | `#50246` — users prefer queueing over interrupting | **[E]** | Verbatim. Note: `closed/**completed**` — queueing is the shipped behavior, which is the point. |
| 29 | `#63190` — *"Mid-turn injections can confuse multi-step tasks, and there's no way to say 'this isn't urgent, just queue it.'"* | **[E]** | Verbatim at body line 64. Issue is **open**. |

### 1D — An urgent channel

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 30 | No shipped diff-review tool provides an urgent-vs-batch lane | **[E]** (doc marks it [I]) | I grepped the READMEs of `modem-dev/hunk`, `backnotprop/plannotator`, `yoshiko-pg/difit`, `kamranahmedse/diffity`, `BloopAI/vibe-kanban`, `smtg-ai/claude-squad`, `herdrdev/herdr`, `umputun/revdiff`, `hamelsmu/claude-review-loop` for `urgent|priority|interrupt|defer`. Zero hits for an urgent/priority review lane in any of them. Both reports are right, and this is now direct evidence rather than inference. |
| 31 | `#11945` — *"Priority queue: Option to mark urgent messages that should interrupt"* | **[E]** | Verbatim at body line 75; `closed/not_planned`. |
| 32 | A priority/urgent channel is "repeatedly REQUESTED but **UNSHIPPED**" | **[X]** | Both cited issues are `closed/**completed**`: `#30492` and `#64624`. `#41665` ("interrupt message on stdin") is also `closed/completed`. What shipped is not a *marked-urgent* lane — it is queue-delivery-at-tool-boundary (claim #2), which the maintainer's closing comment on #64624 states plainly. Correct framing: **the ask was answered by tool-boundary delivery, not by a priority marker.** "Unshipped" is wrong; "no marked-urgent lane exists" is right. |
| 33 | Esc is the only "urgent = now" mechanism, and it is destructive | **[E]** | Verbatim from #30492's body. |

### 1E — Agent-side deferral

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 34 | `#29224` proposed agent-side ack/defer and was closed as not planned; includes *"Progress acknowledgment — 'Got it, I'll handle that next' for real instructions"* | **[E]** | `state: closed`, `state_reason: not_planned`. Quote verbatim at body line 90. Caveat worth knowing: the issue's motivating use case is *frustrated user messages* ("wtf", "hurry up"), not review handoff — it is still the right precedent, just a looser fit than the doc implies. |
| 35 | `#63190` `/defer` is the nearest convention, and it is **user-side** | **[E]** | Verbatim: *"adding a clean 'I'll deal with this next' lane"*. Issue is **open** (doc doesn't say). |
| 36 | `#29490` stop-word check-in convention, unshipped | **[E]** | Verbatim; `closed/not_planned`. |
| 37 | `#57263` no per-message acknowledgment of queued messages | **[E]** | Verbatim; `closed/not_planned`. |
| 38 | claude-review-loop carries `task → addressing` in `.claude/review-loop.local.md` | **[E]** | README verbatim: *"If in `task` phase: writes a runner script and prompt file, transitions to `addressing`, blocks exit with instructions"*; *"State is tracked in `.claude/review-loop.local.md`"*; *"The stop hook timeout is set to 30 seconds… The hook itself is fast (it only writes files and returns a block decision)"*. |
| 39 | vibe-kanban carries pending/review/done as harness-owned columns | **[I]** | Only secondary blogs cited; the repo README has no matching text. Plausible, unverified. |

### 1F/1G/1H — Projects, fresh-vs-same, end-of-work

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 40 | Subagents get a fresh, isolated context | **[I]** | Only third-party sources cited (HackerNoon/Medium/Layer3Labs). Uncontroversial and almost certainly true, but not primary-sourced in the doc. |
| 41 | Anthropic Code Review dispatches a team of agents; 16% → substantive review coverage; depth over speed | **[E]** | Blog verbatim: *"Code Review dispatches a team of agents on every PR to catch the bugs that skims miss"*; *"Before, 16% of PRs got substantive review comments. Now 54% do."*; *"Code Review optimizes for depth and is more expensive than lighter-weight solutions."* (The doc's "built for depth, not speed" is a light paraphrase.) |
| 42 | Severity tiers Important / Nit / Pre-existing | **[E]** | code-review.md L44/L62. Primary, not the fast.io secondary. |
| 43 | Re-review convergence: after round one, suppress new nits, Important only | **[E]** | code-review.md L179 verbatim. |
| 44 | Auto-resolve threads on fix | **[E]** | code-review.md L53 (*"the next run resolves the thread when the issue is fixed"*) and L106 (*"auto-resolving threads when you fix flagged issues"*). |
| 45 | Fix + push, not replies; `@claude review` for a fresh pass | **[E]** | code-review.md L53 verbatim. |
| 46 | Verification bar (file:line evidence before posting a class of finding) | **[E]** | code-review.md L177 verbatim. |
| 47 | Neutral check run; gating is opt-in via the severity JSON | **[E]** | code-review.md L66 + L73 verbatim. |

Per-project WHEN/WHO/STATE claims spot-checked and confirmed from source READMEs:
`revdiff` (*"`revdiff_review` suspends pi, hands the terminal to revdiff, then returns captured
annotations to the agent"*, and the rolling-compare marker: *"The next round opens a rolling compare
(`<new> <old>`); reviewed snapshots are replaced, and a clean review removes the final snapshot"*),
`herdr` (*"every pane is marked working, blocked, or idle"*), `diffity` (*"Uses severity tags
(`[must-fix]`, `[suggestion]`, `[nit]`, `[question]`)"*). All **[E]**.

---

## 2. Where the two reports agree

Both are correct and mutually confirming on:

- Claude Code queues typed input; Enter never interrupts. Delivery is boundary-based, never mid-tool-call.
- Interrupts are the lossy path; do not build the handoff on Esc/Ctrl+C.
- `Stop` hook + `decision:block`/`reason` (or exit 2 + stderr) is the working, proven delivery
  mechanism, with `stop_hook_active` and the 8-block cap as the loop guards. `claude-review-loop`
  is the field-proven precedent for exactly this shape.
- No tool anywhere ships an urgent lane or an agent-side deferral state. The `!` marker has no prior art.
- End-of-work convergence: severity tiers, suppress nits after round one, auto-resolve on fix,
  fix-and-push rather than replies, advisory (never merge-blocking) by default, no full-diff re-read.
- Mid-work comments go back to the **same** agent; fresh context is for the heavy final pass.

---

## 3. Where they contradict each other, and who wins

| Topic | Uploaded doc | Our report | Who's right |
|---|---|---|---|
| **PreToolUse `additionalContext`** | Silent; only offers PreToolUse *deny-with-reason* | *"PreToolUse cannot carry `additionalContext` today (open feature requests #15345/#15664)"* | **Neither — ours is [X] wrong.** Both issues are `closed/**completed**`. CHANGELOG v2.1.9: *"Added support for `PreToolUse` hooks to return `additionalContext` to the model"*, and #15664's closing comment says so explicitly. hooks.md documents the field today. This is the single most consequential correction in this check. |
| **PostToolUse `additionalContext`** | *"buggy/unreliable"* (#18427) — recommends `decision:"block"` instead | *"the one documented event that can add context without blocking"* | **Ours.** hooks.md documents PostToolUse `additionalContext` as supported. #18427 is a closed-not-planned bug from January that the docs have since moved past. |
| **Priority channel status** | "requested but unshipped" (#11945, #30492) | "#30492/#64624 open and unimplemented" | **Neither.** Both #30492 and #64624 are `closed/completed`; the maintainer states tool-boundary delivery is shipped. Correct statement: *no marked-urgent lane exists; the ask was satisfied by tool-boundary queue delivery.* |
| **SubagentStop doc-lag** | "live docs still wrong, keep a fallback" | Same claim | **Neither — both stale.** #65495 is closed/completed and hooks.md is now correct. No fallback needed. |
| **Plugin exit-2 bug** | DO: install via `.claude/hooks/` to dodge #10412 | Silent | **Ours, by omission.** #10412 is `closed/completed`. |
| **`#21451` as interrupt evidence** | Cited as "abandoned edits corrupt the codebase" under DON'T-interrupt | Not cited | **Ours.** The issue is about token exhaustion, not interruption. |
| **`!` urgent marker** | Ship it as front-of-queue + ATTENTION-header salience, still at a boundary | Skip for v1; revisit only on measured evidence | **Tie on evidence — both agree the evidence is silent.** The owner's decision (skip) matches ours, and nothing verified here argues against it. The uploaded doc's version costs one sort key, so it is a cheap add later. |
| **Payload shape** | `hookSpecificOutput.additionalContext` over exit 2 + stderr | `decision:block` + `reason` (or the combined shape) | **Uploaded doc, verified.** See §4. |
| **Nit cap** | Absent | Present, [E] | **Ours** — code-review.md L171 confirms it, and the owner's plan already has it. |
| **Verification bar** | Present, [E] | Absent | **Uploaded doc** — code-review.md L177 confirms it. Real addition. |

---

## 4. Techniques the uploaded doc adds (verified), and what they'd cost here

**4.1 `hookSpecificOutput.additionalContext` instead of exit 2 + stderr — ADOPT.**
Verified [E] (claim #10). hooks.md: *"Use `additionalContext` when the hook is working as designed and
giving Claude guidance… It keeps the conversation going through the same loop protections as
`decision: "block"`, namely the `stop_hook_active` input and the 8-consecutive-continuation cap, but
the transcript labels it `Stop hook feedback` and no hook error notification is shown."*
Cost in bash: emit one JSON object on stdout instead of `printf >&2; exit 2`. Roughly three lines,
`jq -n` or a heredoc. Same guards, strictly better UI. There is no reason to keep the exit-2 shape.

**4.2 Verification bar for the review body — ADOPT (end-of-work).**
Verified [E] (claim #46). One line of instruction text in the review template; zero code.

## 4bis. Techniques NEITHER report has, found during this check

These came out of the live hooks reference and are the highest-value part of this report.

**A. `Stop` does not fire on a user interrupt.** hooks.md, Stop: *"Does not run if the stoppage
occurred due to a user interrupt."* This turns "never use Esc" from a preference into a hard
requirement: an interrupted turn produces **no Stop event**, so a queued review would sit undelivered
until the *next* turn ends. Cost: zero — it is a rule, and the plan already follows it.

**B. `PreToolUse` `additionalContext` is a shipped, non-destructive tool-boundary delivery path.**
Verified: CHANGELOG v2.1.9, hooks.md PreToolUse decision-control table. And it is field-proven for
exactly this use case — from `#30492`'s thread: *"a wildcard `PreToolUse` hook watches a note file and
re-emits its contents as `additionalContext`, clearing the file after read. It genuinely delivers a
note at the next tool-call boundary without interrupting the turn."*
Cost in a bash+tmux plugin: one extra hook entry with matcher `"*"`, a ~10-line drain script
(`[ -s "$q" ] || exit 0`; read; `mv` the file aside atomically; emit JSON). Ceiling: it runs before
every tool call, so it must stay a stat-and-exit in the common case.
**This does not change decision (1)** — it is not an interrupt and not a "paste now". It is the
strictly-better *timing* for the same non-destructive delivery, and the harness's own queue behaves
this way. Worth having as the follow-up to the Stop hook, not as a replacement.

**C. `asyncRewake` is the only documented way to reach an *idle* agent.** hooks.md L452: *"If `true`,
runs in the background and wakes Claude on exit code 2… The hook's stderr, or stdout if stderr is
empty, is shown to Claude as a system reminder"*, and L3449: *"an `asyncRewake` hook that exits with
code 2 wakes Claude immediately even when the session is idle."* Our report cites `asyncRewake` only
as a loop hazard; the uploaded doc never mentions it. A Stop hook cannot reach an agent that has
already stopped — this can. Cost: one config flag on an existing hook, plus the documented
infinite-loop discipline.

**D. Hook output is capped at 10,000 characters.** hooks.md L893: *"Hook output strings, including
`additionalContext`, `systemMessage`, and plain stdout, are capped at 10,000 characters. Output that
exceeds this limit is saved to a file and replaced with a preview and file path."* A multi-file review
batch will hit this. The harness's own convention — write the full text to the session dir, hand
Claude a path plus a preview — is the one to mirror, and tmux-cockpit already has a per-review session
directory to put it in. Cost: a `wc -c` guard and a path in the payload.

**E. `permissionDecision: "defer"` exists but is out of reach.** hooks.md: *"Claude Code honors this
value only in non-interactive mode with the `-p` flag. In interactive sessions it logs a warning and
ignores the hook result."* Also *"`"defer"` only works when Claude makes a single tool call in the
turn."* Worth recording so nobody rediscovers it as a candidate deferral primitive: it is a headless-
SDK feature, unusable in a tmux pane. This *reinforces* decision (2).

---

## 5. DO / DON'T delta

### Lines the uploaded doc adds that are VERIFIED — take these

- **DO** return `{"hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"…"}}` rather than
  exit 2 + stderr. Same loop protections, transcript reads `Stop hook feedback` instead of
  `Stop hook error`. [E, hooks.md + CHANGELOG v2.1.163]
- **DO** set a verification bar in the review template — evidence/`file:line` required before a class
  of finding is posted. [E, code-review.md L177]
- **DO** keep the review payload deliberately small; the harness caps hook output at 10k chars and
  spills to a file path. [E, hooks.md L893 — found in this check, in neither report]
- **DON'T** treat `#3003` (2025, closed-duplicate) as the load-bearing anti-interrupt fact. The
  load-bearing fact is that **Stop does not fire on a user interrupt at all**. [E, hooks.md]

### Lines in OUR report contradicted by verified evidence — fix these

- **[X]** *"PreToolUse cannot carry `additionalContext` today (open feature requests #15345/#15664)"* —
  both closed/completed; shipped in **v2.1.9** and documented. Delete this and replace it with §4bis-B.
- **[X]** *"`CLAUDE_CODE_STOP_HOOK_BLOCK_CAP` … asyncRewake hook docs"* framing — the cap is right, but
  cite env-vars.md + CHANGELOG v2.1.143, and stop treating `asyncRewake` as only a hazard: it is the
  idle-agent delivery mechanism.
- **[X]** *"#30492, #64624 open and unimplemented"* — both closed as completed. The shipped answer is
  tool-boundary queue delivery.
- **[X]** *"the reference page's SubagentStop section is still wrong as of this writing"* — fixed;
  #65495 closed/completed, hooks.md now documents SubagentStop `additionalContext`.
- Keep, unchanged and verified: nit cap, suppress-nits-after-round-one, auto-resolve on fix, advisory
  gate, summary-first tally, no automatic retry of a failed handoff, single `task → addressing`
  transition rather than N stacked batches.

### Lines in the UPLOADED doc contradicted by verified evidence — do not adopt

- **[X]** *"DO install the Stop hook via `.claude/hooks/` (not only via a plugin) to avoid the plugin
  exit-2 continuation bug (#10412)"* — that bug is **fixed** (closed/completed). tmux-cockpit is a
  plugin; this would be a real, unnecessary structural constraint on the design.
- **[X]** *"DON'T rely on `additionalContext` from PostToolUse — it has been dropped/unreliable
  (#18427)"* — closed/not-planned in January; the field is documented and supported today.
- **[X]** *"keep a fallback path since SubagentStop `additionalContext` support is under-documented
  (#65495)"* — resolved; no fallback needed.
- **[X]** *"DON'T interrupt during an in-flight edit — abandoned edits corrupt the codebase (#21451)"* —
  the conclusion may be sound, but #21451 is about token exhaustion and does not support it. Cite
  #1172 or #23350 instead.
- **[X]** the "priority channel is unshipped" framing — see claim #32.

---

## 6. Bottom line for the two decisions

**Decision (1) — Stop-hook handoff at the boundary, no paste-now, no `!`: SUPPORTED.**
The uploaded doc reaches the same conclusion by a different route, and the verified evidence is
stronger than either report states: an interrupt-based handoff would not merely be lossy, it would
*skip the delivery hook entirely*, because `Stop` does not run on a user interrupt. One clause needs
rewording — Claude Code's own queue delivers at the next **tool-call** boundary within the turn, not
at the turn boundary — and one mechanism should be added to the payload (`additionalContext` over
exit 2). A `PreToolUse` note-drain is a verified, shipped, non-destructive way to hit that tighter
boundary later if the Stop boundary measures too slow; it is an enhancement to this decision, not a
challenge to it.

**Decision (2) — no agent-side deferral convention: SUPPORTED, with a second independent leg.**
#29224 closed not-planned is confirmed, and so are its two siblings. The check also found the one
harness primitive actually named `defer` (`PreToolUse` `permissionDecision: "defer"`) and confirmed it
is headless-only and ignored in interactive sessions — which closes the last plausible objection to
the human owning pending→sent→approved.
