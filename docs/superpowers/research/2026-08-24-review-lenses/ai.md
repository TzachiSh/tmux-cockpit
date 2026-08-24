# Lens: AI / agent integration — how the review reaches the agent

Report date: 2026-08-24. Scope: `scripts/tmux-review` compose/deliver, `scripts/agent-state`,
`cockpit.tmux`, `examples/claude-hooks.json`, measured against Claude Code's 2026 hook/MCP/skill
surface and the tools the community actually ships. Tags: **[E]** = verified evidence (URL, file:line,
or `gh api`), **[I]** = my inference.

---

## 0. The one-sentence finding

The plugin already knows, per pane, whether the agent is mid-turn, blocked, or idle
(`@panestate`, written only in `scripts/agent-state:125-137`) — and `deliver()` **never looks at
it** (`grep -n panestate scripts/tmux-review` → no match [E]). Every other weakness in this lens
is downstream of that: the review is delivered by the one mechanism that is *least* safe when the
agent is busy and *least* likely to arrive when the agent is idle, using a signal the repo
already computes for free.

---

## 1. What is built today, restated precisely

- `compose()` (`scripts/tmux-review:436-446` [E]) = a 3-line fixed header + the store verbatim.
  Header text, verbatim from `:438`:
  > `Code review of your uncommitted changes. Address each comment; a comment` /
  > `containing ?? is a question — answer it in chat, do not edit for it.` /
  > `Comments are anchored as path:line with the original line quoted.`
  plus optional `Paths are relative to <root>.`
- `deliver()` (`:471-561` [E]): pane-id shape guard → `pane_current_command` vs
  `@cockpit_agents_glob` → store non-empty → `load-buffer` → `tmux if-shell -F` re-check in the
  same command-queue item → `paste-buffer -p -r -d`. Typed, **unsent**.
- Over 2048 bytes (`:515-527` [E]) the payload becomes a 0600 mktemp file under
  `$XDG_RUNTIME_DIR` and only this sentence is pasted (`:525` [E]):
  > `Read and address every comment in <file> — review of your uncommitted changes; ?? comments are questions.`
- Comment store order = pin order (`attach()` `:319-345` appends [E]); no ids, no sort, no
  agent-authored entries, no back-channel.

The engineering *inside* those 90 lines is genuinely good — the `if-shell -F` atomic re-check at
`:558` closes a race the repo reproduced 6/6, the per-invocation buffer name avoids a
server-global collision, `-p -r -d` is the correct paste shape. The problem is not the paste's
quality. It is that paste is one of six mechanisms and the repo picked it for all cases.

---

## 2. What the Claude Code surface actually offers in 2026 (verified)

From <https://code.claude.com/docs/en/hooks> [E]:

| Event | Blockable | Structured output that reaches the model |
|---|---|---|
| `SessionStart` | no | **stdout → model** as context; `systemMessage` |
| `UserPromptSubmit` | yes | **stdout → model**; `hookSpecificOutput.additionalContext`; `decision:"block"`+`reason` |
| `PreToolUse` | yes | `additionalContext`, `updatedInput`, `decision`+`reason` (stdout NOT to model) |
| `PostToolUse` / `PostToolUseFailure` | no | `hookSpecificOutput.additionalContext` |
| **`PostToolBatch`** | **yes** | `decision:"block"`+`reason`; fires after a batch of parallel tool calls, *before the next model call* |
| `PermissionRequest` | no (exit 2 ignored) | `hookSpecificOutput.decision: allow\|deny\|ask` + `reason` |
| **`Stop`** | **yes** | `decision:"block"` + `reason` (**shown to Claude**, conversation continues); `stopReason` |
| `SubagentStop`, `TaskCompleted`, `TeammateIdle`, `PreCompact`, `ConfigChange` | yes | `decision`+`reason` |
| `Notification`, `StopFailure`, `MessageDisplay` | no | **only `terminalSequence` honored** |

Three facts that decide the architecture:

1. **`Stop` with `decision:"block"` + `reason` is a first-class "hand the agent text at a turn
   boundary and make it act" primitive.** [E] Exit code 2 is the equivalent, with stderr fed back
   to Claude. This is *exactly* the delivery cockpit needs and it costs one hook entry.
2. **stdout reaches the model on only three events**: `UserPromptSubmit`, `UserPromptExpansion`,
   `SessionStart` [E]. Everywhere else you must emit JSON (or exit 2). `Notification` — the event
   `agent-state notify` already uses — honors *nothing* but `terminalSequence` [E], so it can
   never be a delivery channel.
3. **Default `Stop` timeout is 600 s** [E], and Plannotator ships `"timeout": 345600` (4 days) on
   its hook [E, plannotator README]. So "hooks never block long" is a *cockpit* invariant, not a
   platform one — which matters when arguing about relaxation (§6).

**`--channels`** (research preview, <https://code.claude.com/docs/en/channels-reference>) is the
platform's own answer to "push into a live session" [E]:

```ts
// MCP server constructor
capabilities: { experimental: { 'claude/channel': {} } },
// push
await mcp.notification({
  method: 'notifications/claude/channel',
  params: { content: '<the review>', meta: { pane: '%7', severity: 'review' } },
})
```
arriving in context as `<channel source="cockpit-review" pane="%7">…</channel>`. And the
delivery semantics, verbatim from the reference [E]:

> *"Events queue into the session and are processed in order. If several notifications arrive
> while Claude is busy, they're delivered together on the next turn and Claude handles them as a
> group."*

That is cockpit's batching discipline and turn-boundary rule, ratified by the platform. But it is
gated behind: `--channels` **at launch** (cannot be retrofitted onto a running pane [E]), an
Anthropic-curated plugin allowlist during preview (`--dangerously-load-development-channels`
otherwise, which shows a full-screen warning dialog [E]), Bun/Node + the MCP SDK, Anthropic auth,
and an org policy switch. **Verdict: watch, do not build.** It is the north star, not the road.

**What does *not* work**, contrary to hope: `--resume` "creates a new process from a saved
transcript" and does not attach to a live session [E, cli-reference]; `claude -c -p "msg"` starts
a fresh session; `claude inject` does not exist (open FRs anthropics/claude-code
[#24947](https://github.com/anthropics/claude-code/issues/24947),
[#27441](https://github.com/anthropics/claude-code/issues/27441) [E]).
`--append-system-prompt` is startup-only, so it cannot carry a review [E].

---

## 3. What the community converged on (verified stars / last push, `gh api`, 2026-08-24)

| Project | ★ | Last push | Mechanism |
|---|---:|---|---|
| [modem-dev/hunk](https://github.com/modem-dev/hunk) | 8711 | 2026-08-24 | TUI + **loopback daemon** on `127.0.0.1:47657` + CLI (`hunk session comment add/list/apply`) + experimental MCP |
| [backnotprop/plannotator](https://github.com/backnotprop/plannotator) | 7985 | 2026-08-24 | **`PermissionRequest` hook on `ExitPlanMode`**, `timeout: 345600`; browser UI; deny → structured feedback into the session |
| [google/git-appraise](https://github.com/google/git-appraise) | 5309 | **2023-08-12** | git notes; dead |
| [yoshiko-pg/difit](https://github.com/yoshiko-pg/difit) | 3095 | 2026-08-09 | blocking CLI → stdout on SIGINT; `--background` + `GET /api/comments-output` |
| [umputun/revdiff](https://github.com/umputun/revdiff) | 807 | 2026-08-20 | **blocking popup + structured stdout, exit 10**; separate `revdiff-planning` plugin uses a **`Stop` hook** |
| maxto/agent-mux | 2 | 2026-06-21 | tmux paste + >2 KB file pointer (cockpit's model) |

Two things are striking here.

**(a) The paste-into-a-terminal mechanism has essentially no adoption.** The only project doing
it as its primary channel is agent-mux at **2 stars** [E] — which is precisely the design cockpit
copied (including the >2 KB threshold, `TMUX_AGENT_INLINE_THRESHOLD`). The 8 000-star projects
either block the agent (revdiff, difit) or intercept a hook (plannotator, revdiff-planning). That
is a real signal, not a fashion: paste is the only mechanism whose completion depends on a human
keystroke landing in a *different* pane from the one they are looking at.

**(b) The two winning shapes are hook-interception and blocking-stdout, and both are cheap.**
revdiff's skill contract, verbatim [E, `.claude-plugin/skills/revdiff/SKILL.md`]: launcher sets
`REVDIFF_EXIT_CODE_ON_ANNOTATIONS=10`; the agent treats exit 10 as success and reads stdout; on
timeout it does **not** retry but recovers from `${TMPDIR}/revdiff-output-*` or durable history.
revdiff-planning's `Stop` hook "extracts the complete `<proposed_plan>` … annotations are returned
as continuation feedback so the agent revises the plan and presents it for review again", and it
is **gated**: it only fires for a root `Stop` payload with `permission_mode` = `plan` [E]. That
gating is the whole craft of a Stop-hook gate.

The counter-example is equally instructive: openai/codex-plugin-cc
[#248](https://github.com/openai/codex-plugin-cc/issues/248) [E] — its Stop review gate
"conflates two distinct outcomes into the same `{ decision: "block" }`": a genuine block and an
*infrastructure failure*, and "Claude Code's rewake-on-block semantic creates a sustained rewake
loop … burning tokens and producing zero useful review." **A Stop-hook gate must be
fail-open and must clear its own trigger before it speaks.** Design rule, evidenced.

---

## 4. Comparison matrix of delivery mechanisms

Rows are mechanisms for getting a finished human review from the popup into the agent. "Mid-turn"
= what happens if the agent is running when the human presses ^S.

| # | Mechanism | Latency to *acted on* | Agent mid-turn | Works when agent never asked | Survives pane death | New deps | Auto-starts a turn |
|---|---|---|---|---|---|---|---|
| 1 | **Paste, unsent** (today, `:558`) | **unbounded** — waits on a human Enter in another pane; often *never* | Lands in whatever has focus: a permission dialog, or steers mid-task between tool calls [E, #71726/#73118] | **Yes** — its one real strength | No (buffer + prompt die) | none | No |
| 2 | Paste + guarded auto-Enter at `@panestate=done` | ~1 redraw (<1 s) | N/A — gated off | Yes | No | none | Yes (turn boundary) |
| 3 | **`Stop` hook drains a pending review** | next turn end (seconds for a busy agent; **never** for an already-idle one) | Perfect — that *is* the boundary | Yes | No (per-pane option) | none | Yes |
| 4 | `Stop` hook *flags* review-wanted (non-blocking, exit 0) | 0 (it is a signal, not a delivery) | Perfect | It is the agent asking | No | none | No |
| 5 | `UserPromptSubmit` `additionalContext` drain | next time the human types anything | Cannot fire mid-turn | Yes, but human must type | No | none | No (rides an existing turn) |
| 6 | `SessionStart` stdout | next session/resume/compact | n/a | Yes | Only if the store outlives the pane | none | No |
| 7 | `PostToolUse` / **`PostToolBatch`** `additionalContext` | **sub-turn**, at the next tool boundary | Best-in-class: designed for it | Yes | No | none | Rides the current turn |
| 8 | **Blocking skill/slash-command** (revdiff model, exit 10) | 0 — the agent is already waiting | n/a (agent asked) | **No** — needs the agent to have called it | Popup dies with client; revdiff persists to history as mitigation [E] | Bash-tool timeout ceiling | n/a |
| 9 | **stdio MCP server** (`get_review`, `list_comments`) — pull | one poll interval, or never (agent must think to call) | Agent chooses when | No — pull only | Server is a per-session subprocess | node/bun/python + MCP SDK, or fragile bash JSON-RPC | No |
| 10 | **MCP channel** `notifications/claude/channel` | seconds; queued & grouped on next turn [E] | Platform-handled, batched | Yes | No | Bun/Node + SDK + `--channels` **at launch** + preview allowlist + org policy | Yes |
| 11 | `claude -p --resume` out-of-band | n/a | n/a | n/a | n/a | none | **Does not reach the live pane at all** [E] |

Reading of the matrix: **no single row wins**. Row 1 is the only one that works when the agent is
idle and nobody asked; row 3 is the only one that is safe and prompt when the agent is busy. They
are complements, and the discriminator between them — `@panestate` — is already sitting in a tmux
option that costs zero shell to read.

---

## 5. Where the current design actually loses

### 5.1 The delivery ignores turn state [E, high severity]
`deliver()` guards on pane-id shape and `pane_current_command` only (`:471-561`). Consequences:

- **A pane blocked on a permission dialog gets the review pasted into the dialog.**
  `@panestate=needs-input` is exactly this case and is not consulted. `pane_current_command` is
  still `claude`, so guard 2 passes. What a bracketed paste does to Claude Code's select-list
  permission prompt is untested here — best case it is swallowed and the review is silently lost.
  [I, but trivially testable and the antipatterns doc already asked for this guard:
  `docs/superpowers/research/2026-08-24-review-antipatterns.md` "Guards: %N pane id, agents glob
  match, **@panestate not running**" — the third guard was specified and not implemented [E].]
- **A running pane gets steered mid-task.** In the CLI/TUI a message submitted while Claude works
  "is picked up between tool calls and injected into the agent loop mid-task"
  [E, anthropics/claude-code#71726]. So the human's Enter does not politely wait for the turn to
  end — it redirects the agent away from what it was doing, mid-edit, with a batch of comments
  about a diff the agent is *still writing*. (Gap 4 of the landscape doc, unaddressed.)
- **The fix is free and uses the mechanism already proven at `:558`.** tmux formats can read
  per-pane options, so the atomic `if-shell -F` guard can carry the turn-state test with no extra
  shell round-trip:

  ```sh
  tmux if-shell -F -t "$pane" \
    "#{&&:#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},\
        #{||:#{==:#{@panestate},done},#{==:#{@panestate},}}}" \
    "paste-buffer -p -r -d -b $bufname -t $pane ; display-message '…'" \
    "delete-buffer -b $bufname ; run-shell -b '<self> --arm $pane $file'"
  ```
  Idle → paste (today's behaviour). Busy/blocked → arm the pending-review option and let the Stop
  hook deliver it. One `if-shell`, still atomic, still no daemon.

### 5.2 Delivery has no completion event [E/I]
A pasted-but-unsent review is indistinguishable, from every other pane and from the status line,
from a pane with nothing pending. The plugin's whole premise is *knowing which panes need you*,
and it just created an attention state it does not render. The human who reviews pane %7 from
pane %2 must remember to walk to %7 and press Enter — and "never steal focus" (correctly) forbids
taking them there. [I] In a 6-pane fleet this is where reviews go to die.

### 5.3 There is no reverse channel at all [E]
Nothing in `compose()` asks the agent to report what it did. The store has no ids. So:
- The human cannot re-review "only what changed" — the second `prefix r` shows the whole diff again.
- The agent cannot say "comment 3 is wrong" in a way anything can consume.
- Comments arrive in **pin order**, not path order (`attach()` appends, `compose()` is `cat`),
  so an agent working the list top-to-bottom re-opens the same file several times. [E from code, [I] on cost.]

### 5.4 No pre-seeding [E]
Conductor's MCP tool, difit's repeatable `--comment '{"type":"thread",…}'` flag, and hunk's
`hunk session comment add --repo . --file X --new-line 103 --summary "…"` all exist for one
reason: the agent knows which three of its forty changed lines it was unsure about, and the human
does not. Cockpit opens an empty list every time. This is the highest value-per-line item in this
report and it needs **one new subcommand and one option**, not MCP (§7.4).

### 5.5 Prompt design of the payload — five concrete defects

Against the header at `:438`:

1. **No "don't over-reach" fence.** "Address each comment" with no scope limit. [I] An agent that
   receives 8 line comments and a dirty tree will frequently also "clean up while I'm here". The
   review then produces a *new* unreviewed diff, which is the exact failure the whole feature
   exists to prevent.
2. **No dissent licence.** Nothing invites the agent to push back. Self-preference/sycophancy is
   documented in the repo's own antipatterns doc (arxiv 2509.26600, 2510.12367 [E]) and it cuts
   both ways: an agent that complies with a wrong comment silently introduces a bug the human
   just asked for. One clause fixes it.
3. **`??` detection is under-specified relative to the source it copied.** revdiff classifies an
   annotation as a question if it contains `??` **or** starts with `explain|remind|describe|what
   is|what are|how does|how do|clarify` (case-insensitive) [E, SKILL.md]. Cockpit ships only the
   `??` half, in prose, in a header the model must generalise from. A comment reading
   `why is this a map?` (one `?`) is a question by any human reading and a directive by the
   header's literal rule. [I] Expect misclassification.
4. **No question/edit ordering rule.** revdiff explicitly *holds* code-change annotations until
   the explanation loop finishes [E]. Cockpit's header mixes both classes in one batch with no
   ordering, so a payload containing `??` and edits will typically produce edits first and a
   half-answered question buried after them. [I]
5. **Anchors are presented as authoritative but drift.** After the agent's first edit, every
   later `path:line` in the same file is off. The quoted fragment is in the payload and is the
   robust anchor (this is right, and matches AnchoredAI [E]), but the header never tells the agent
   to prefer it. One sentence.

Bonus, on the >2 KB path (`:515-527`): the threshold is **2048 bytes**, and a ten-comment review
clears it easily, so the *common* case is the pointer sentence, not the inline review [I]. The
file lives under `$XDG_RUNTIME_DIR` — outside the repo — so the agent's `Read` of it may trip an
outside-the-project-root permission prompt, which lands on a human who has already walked away
[I, testable]. Writing it under the repo's own `.git/cockpit-review-*` instead would keep it
inside the trusted root, keep it 0600-able, and make it die with the worktree. (It also stops the
tmpdir from being the only GC, which the comment at `:513` concedes.)

---

## 6. On the invariants

- **"No daemon / state dies with the pane"** — hold it. Everything recommended below uses
  per-pane tmux options (`@cockpit_review_pending`) exactly like `@panestate`
  (`agent-state:125-137`), so it dies with the pane and needs no GC. hunk's daemon (8711★) is the
  counter-example of what you would be signing up for: a loopback HTTP broker on a fixed port,
  session registration, and an MCP surface on top. Not for a bash plugin.
- **"Hooks never block long"** — hold it, and note it is *stricter than the platform*: `Stop`'s
  default timeout is 600 s and Plannotator ships 345600 [E]. The recommended Stop hook is
  ~3 tmux calls + one `cat`; nothing opens.
- **"Never auto-submit into the agent"** — **relax it, narrowly, with evidence.** The invariant
  conflates two different things: *sending Enter into a pane mid-turn* (genuinely destructive —
  claude-code #63190/#30492, and #71726 shows CLI messages are injected mid-task) and *handing
  the agent text at a turn boundary it itself just reached*. Every 2026 mechanism in §2 does the
  second: `Stop`+`decision:block` continues the conversation with your `reason` and no human
  keystroke; channels state outright that queued events are "delivered together on the next turn"
  [E]; Plannotator's denial "sends structured feedback to the agent" with no Enter [E]. The human
  consent gesture already exists — it is `^S`. Requiring a *second* consent gesture, in a
  different pane, is what makes reviews evaporate (§5.2). Recommendation: keep "never send Enter
  into a `running` or `needs-input` pane" as the invariant, and let the Stop-hook path deliver at
  `done`.
- **"Never steal focus"** — untouched by everything below; the Stop path removes the *need* to
  visit the pane, which strengthens it.

---

## 7. Recommended architecture, with sketches

A three-layer split. Layer A is the delivery fix and is small. Layer B is the agent-side loop.
Layer C is the optional blocking path for when the agent asks.

### 7.A Turn-state-aware delivery: paste when idle, Stop-hook when busy

**One new hook, one new script, one new per-pane option. No daemon, no jq, no JSON.**

`examples/claude-hooks.json` gains a second `Stop` entry (hooks with the same event run in
parallel [E], and `agent-state` prints nothing, so there is no stdout collision):

```json
"Stop": [
  { "matcher": "", "hooks": [
    { "type": "command", "command": "\"${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}\"/scripts/agent-state done" },
    { "type": "command", "command": "\"${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}\"/scripts/agent-review-drain", "timeout": 5 }
  ] }
]
```

```bash
#!/usr/bin/env bash
# scripts/agent-review-drain -- Stop hook. Hand the agent a review the human
# pinned while it was busy, at the turn boundary it just reached. Never opens
# anything, never waits: two tmux calls and a cat.
set -uo pipefail
pane="${TMUX_PANE:-}"
case "$pane" in %[0-9]*) ;; *) exit 0 ;; esac    # agent-state:13-16, same guard

file=$(tmux show-option -pqv -t "$pane" @cockpit_review_pending 2>/dev/null) || exit 0
[ -n "$file" ] && [ -r "$file" ] || exit 0

# Clear BEFORE speaking. A Stop gate that re-emits its own reason is how
# codex-plugin-cc#248 turned into a sustained rewake loop. Clearing first means
# a crash loses one review instead of repeating it forever. Fail-open always.
tmux set-option -pu -t "$pane" @cockpit_review_pending 2>/dev/null || true

# The sibling Stop hook already wrote `done`. Blocking the stop means the agent
# is about to work again, so correct the border before we return -- otherwise
# the fleet shows green while the pane grinds through the review.
"${0%/*}/agent-state" running "$pane"

# exit 2 on a blockable event feeds stderr straight back to Claude. Chosen over
# the JSON form ({"decision":"block","reason":...}) purely to keep jq out of the
# runtime path -- agent-state:66-70 makes that call explicitly for the same reason.
cat -- "$file" >&2
exit 2
```

`deliver()` changes only its terminal branch: idle → paste (unchanged); busy/blocked → write the
composed review to the same 0600 file and `set-option -p @cockpit_review_pending "$file"`. The
`if-shell -F` in §5.1 makes that choice atomically, in tmux, with no shell round-trip — the same
technique that already closed the race at `:558`.

Two consequences worth stating plainly:
- The **>2 KB pointer becomes unnecessary on the Stop path** — `reason` carries arbitrary text and
  costs the agent no tool call and no permission prompt. Keep the file only as the pending-review
  spool.
- An **already-idle pane never fires Stop again**, so the paste path is not redundant — it is the
  correct mechanism for exactly the idle case. That is why this is a hybrid, not a replacement.

**Verify before shipping** [I]: that `exit 2` on `Stop` really surfaces stderr to the model on
your Claude Code build (the docs are explicit that exit 2 blocks and that the JSON `reason` is
"shown to Claude"; the stderr routing for `Stop` specifically is the one link in the chain I did
not see stated for that event). If it does not, swap the last two lines for the JSON form and
accept the jq dependency, or hand-escape.

### 7.B "Needs review" as a first-class fleet state — the agent asks

Second, tiny Stop-hook behaviour (same script, exit 0 branch): if the tree is dirty and no review
is pending, set a **window-visible** flag rather than opening anything.

```bash
# no pending review: does this pane WANT one? (dirty tree at a turn boundary)
dir=$(tmux display-message -p -t "$pane" '#{pane_current_path}' 2>/dev/null)
if [ -n "$dir" ] && ! git -C "$dir" diff --quiet 2>/dev/null; then
    tmux set-option -p -t "$pane" @cockpit_review_wanted 1 2>/dev/null || true
fi
```

This is Plannotator's insight (auto-open at the decision point) with the popup removed — and the
popup **must** be removed here, because `display-popup` is *client*-owned and there is one client
for a whole fleet [E, `cockpit.tmux:249-250` binds a popup to the client]. Six panes ending turns
at once would fight over it. A flag instead: the fleet queue (`fleet_rows()`) sorts wanted-first,
the pane border shows it, and the human opens the review when they choose. Cost of the whole
signal: one `git diff --quiet` per turn end — nowhere near a status redraw, so invariant 1 holds.

### 7.C The blocking path, for when the agent *does* ask: `/cockpit-review`

A skill, not MCP. Concrete constraints found in the skills docs [E]:

- `` !`cmd` `` injection runs under the **Bash tool's 2-minute timeout**, and **any non-zero exit
  aborts the whole skill invocation** ("Shell command failed for pattern …"). So an
  exit-10-on-annotations contract *cannot* live in a `!` injection. It must be a Bash **tool
  call** in the skill body, with an explicit long `timeout`.
- `allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/tmux-review *)` pre-approves it so the human
  is not prompted [E].

```markdown
---
name: cockpit-review
description: Open the tmux-cockpit review popup on this pane's uncommitted diff and act on the comments.
disable-model-invocation: false
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/tmux-review *)
---
Run, with a long timeout (the human is annotating; do not poll, do not retry):

    ${CLAUDE_PLUGIN_ROOT}/scripts/tmux-review --request "$TMUX_PANE" || true

Exit 10 means comments were captured — read them from stdout. Exit 0 means the
human discarded the review; stop and say so. Any other code is a launcher error:
report its stderr verbatim, do not retry.

If the launcher timed out, the popup is still open and nothing is lost. Do not
relaunch. Say so, and wait for the human to tell you the review is finished;
then read the spool named in $XDG_RUNTIME_DIR/tmux-cockpit-review-*.
```

`--request` = today's `review_pane()` (`:629-645`) with the popup opened via `tmux display-popup
-E -t "$pane"` and the composed store printed to **stdout**, `exit 10`. That is revdiff's exact
contract, and the timeout-recovery clause is copied from revdiff's SKILL.md because that failure
is real and their answer is correct [E].

### 7.D Pre-seeding — the agent annotates its own diff first

One subcommand, no protocol:

```
tmux-review --seed [pane]    # reads revdiff-grammar blocks on stdin,
                             # appends to a per-pane seed store
```
stored in `@cockpit_review_seed` (path to a 0600 mktemp), merged by `rows_mode()` with a distinct
marker (`◆` vs the existing `●`) so the human sees at a glance *"the agent flagged this line
itself"*, and folded into the composed payload as `## path:line (+) [agent]`. The agent writes it
from the same skill at the end of a turn:

```
printf '## src/x.sh:42 (+)\n> foo=$(bar)\nI could not tell whether bar can be empty here.\n' \
  | tmux-review --seed
```

This is Conductor's MCP tool and difit's `--comment` flag [E] achieved with a pipe. It is the
single biggest usability delta available: opening a review already pointed at the three lines the
author was unsure about, instead of an empty 400-row list.

### 7.E The reverse channel — cheapest version that works

No new grammar, no ids, no parser. Change `compose()`'s header to require a structured close-out,
and have the *next* review key on it:

```
When you are done, end your reply with one line per comment, nothing else:
  <path>:<line> — done | skipped: <one-line reason>
```
`path:line` is already a unique key in the store, so nothing new is invented and revdiff's grammar
is untouched (an id in the header — `## [3] foo.go:12 (+)` — would parse as a *filename* under
revdiff's `^## (.+?)(?::(\d+)…)? \((…)\)$` regex [E], so do not put it there).

The re-review half: at deliver time record `git -C "$root" rev-parse HEAD` plus a
`git diff | cksum` in `@cockpit_review_base`. On the next `prefix r`, files whose diff changed
since that mark sort first and carry a `↺` marker. Comments the agent said it skipped keep their
`●`. That is "re-review only what changed" with no persistence beyond a pane option.

### 7.F Rewrite the header

```
Code review of your uncommitted changes, from the human, batched on purpose.

Scope: change only what these comments ask for. Do not refactor, rename, tidy,
or commit anything else — a fix that grows the diff defeats the review.
Questions first: a comment containing ?? or opening with explain / what / why /
how / clarify is a QUESTION. Answer all questions in chat before you edit
anything, then make the edits in one pass.
Disagree out loud: if a comment is wrong or would break something, say so and
do not make the change. Silent compliance with a wrong comment is the worse
failure.
Anchors: comments are path:line with the original line quoted. The line numbers
are from the diff as it was when the review was written; after your first edit
they drift. Trust the quoted fragment, not the number.
Paths are relative to <root>.
When done, end your reply with one line per comment:
  <path>:<line> — done | skipped: <reason>
```

Every clause maps to a defect in §5.5. Cost: it is a `printf`.

---

## 8. Two things deliberately NOT recommended

- **A stdio MCP server** (`get_review`, `list_comments`). It buys nothing the CLI does not
  already give: the agent has a Bash tool, `tmux-review --compose` is already a clean
  machine-readable subcommand, and MCP adds a per-session subprocess plus a runtime the repo does
  not have. hunk (8711★) reached for MCP only once it had a **daemon brokering many live TUI
  sessions** [E] — the invariant cockpit explicitly refuses. MCP earns its keep at the moment
  cockpit wants *push*, and at that moment the right MCP surface is a **channel** (§2), not a
  tool. Revisit when channels leave research preview.
- **An LLM first-pass over the diff.** Claude Code's own `/code-review` (effort levels,
  `--comment`, `--fix`) and subagents can pre-flag risky hunks [E, bundled skill], but: (i) the
  repo's own antipatterns doc already establishes agent self-review as biased (arxiv 2509.26600,
  2510.12367 [E]); (ii) it adds seconds-to-minutes of latency and API cost to a popup whose whole
  virtue is that it opens instantly; (iii) the cheap risk proxies are *local* — `lines_mode()`
  already sorts test files first and files by churn, and the two highest-signal additions are
  pure `git`/`awk`: **assertion-only churn in test files** (Osmani's named agent failure mode
  [E]) and **new-file vs edit**. Do those first; they cost milliseconds and no tokens. If an LLM
  pass ever lands, it belongs in §7.D as a *seed*, produced by the agent that already has the
  context, not as a second model spun up per review.

---

## 9. Ranked recommendations

| # | What | Why | Cost | Replaces |
|---|---|---|---|---|
| 1 | **Turn-state-aware delivery**: extend the `if-shell -F` guard with `@panestate`; idle → paste, busy/blocked → arm `@cockpit_review_pending` (§5.1, §7.A) | Today a review can be pasted into an open permission dialog or steer a mid-edit agent [E #71726]; the discriminating signal is already computed | **S** | the unconditional paste at `:558` |
| 2 | **`scripts/agent-review-drain` Stop hook**, exit 2 + stderr, clears its trigger first (§7.A) | The only mechanism that is prompt *and* safe when the agent is busy; the 8k★ projects all intercept a hook; codex-plugin-cc#248 gives the exact anti-pattern to avoid | **S/M** | nothing — it is the missing half of #1 |
| 3 | **Rewrite the compose header** (§7.F): scope fence, questions-first ordering, dissent licence, anchor-drift rule, close-out format | Five named defects at `:438`, each a one-clause fix; revdiff needed all of these and shipped them [E] | **S** | `:438` |
| 4 | **Pre-seeding: `tmux-review --seed`** + `◆` marker (§7.D) | Conductor/difit/hunk all ship this; opens the review pointed at what the author was unsure about instead of an empty 400-row list | **M** | the always-empty comment store |
| 5 | **`@cockpit_review_wanted` at turn end + fleet-queue sort** (§7.B) | Plannotator's auto-open insight without the popup, which cannot work per-client across a fleet; makes "needs review" another attention state the plugin already renders well | **M** | remembering to press `prefix r` |
| 6 | **Sort comments by path/line in `compose()`** | Pin order makes the agent re-open the same file repeatedly; `sort` on the store's blocks is mechanical | **S** | `cat` at `:446` |
| 7 | **Move the spool from `$XDG_RUNTIME_DIR` into `.git/`**, and raise/retire the 2048-byte threshold on the Stop path (§5.5 bonus) | Keeps the file inside the trusted root (no add-directory prompt), dies with the worktree, and `reason` needs no pointer at all | **S** | `:515-527` |
| 8 | **Local risk proxies**: assertion-only test churn, new-file flag, in `lines_mode()` ordering (§8) | Osmani's named agent failure mode; milliseconds and zero tokens, versus an LLM pass that is slower, costlier, and self-biased | **M** | an LLM first-pass |
| 9 | **`/cockpit-review` skill + `--request` (exit 10, stdout)** (§7.C) | Lossless round trip for the case where the agent asks; revdiff's contract verbatim, incl. its timeout-recovery clause | **M** | nothing (new surface) |
| 10 | **Re-review delta**: `@cockpit_review_base` + `↺` marker (§7.E) | Second `prefix r` currently re-shows the whole diff; the close-out line from #3 makes this cheap | **M** | a full re-read |
| 11 | *Watch, do not build*: MCP **channel** (`notifications/claude/channel`) | The platform's own push mechanism, and its "queued, delivered together on the next turn" semantics ratify this design — but preview-gated, Bun/SDK, `--channels` at launch only | **L**, deferred | — |

---

## 10. Sources

Docs: <https://code.claude.com/docs/en/hooks> · <https://code.claude.com/docs/en/channels> ·
<https://code.claude.com/docs/en/channels-reference> · <https://code.claude.com/docs/en/mcp> ·
<https://code.claude.com/docs/en/skills> · <https://code.claude.com/docs/en/cli-reference> ·
<https://code.claude.com/docs/en/code-review>

Projects (stars/last push via `gh api`, 2026-08-24):
[modem-dev/hunk](https://github.com/modem-dev/hunk) 8711 ·
[backnotprop/plannotator](https://github.com/backnotprop/plannotator) 7985 ·
[google/git-appraise](https://github.com/google/git-appraise) 5309 (dead 2023-08) ·
[yoshiko-pg/difit](https://github.com/yoshiko-pg/difit) 3095 ·
[umputun/revdiff](https://github.com/umputun/revdiff) 807 · maxto/agent-mux 2 ·
[hunk agent-workflows.md](https://github.com/modem-dev/hunk/blob/main/docs/agent-workflows.md) ·
[revdiff SKILL.md](https://github.com/umputun/revdiff/blob/master/.claude-plugin/skills/revdiff/SKILL.md) ·
[revdiff-planning README](https://github.com/umputun/revdiff/blob/master/plugins/revdiff-planning/README.md)

Issues: [claude-code#24947](https://github.com/anthropics/claude-code/issues/24947) ·
[#27441](https://github.com/anthropics/claude-code/issues/27441) ·
[#71726](https://github.com/anthropics/claude-code/issues/71726) ·
[#73118](https://github.com/anthropics/claude-code/issues/73118) ·
[codex-plugin-cc#248](https://github.com/openai/codex-plugin-cc/issues/248)

Repo: `scripts/tmux-review:319-345,436-446,471-561,558,515-527,629-645` ·
`scripts/agent-state:13-16,21,66-70,125-137` · `cockpit.tmux:113-125,246-250` ·
`examples/claude-hooks.json` · `docs/superpowers/research/2026-08-24-review-antipatterns.md` ·
`docs/superpowers/research/2026-08-24-review-tools-landscape.md`
