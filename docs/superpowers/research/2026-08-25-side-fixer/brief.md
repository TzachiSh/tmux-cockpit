# Task brief: side-fixer panes + boundary handover (tmux-cockpit)

Paste this whole file as the opening prompt. Do **not** start coding until Phase 0 is done and its findings are written to the scratchpad.

---

## Goal

Today, when the human sees the main agent burning time on something orthogonal to its actual job (e.g. it's implementing an API but running a 2-minute Postman suite), the only move is to **interrupt it mid-turn** and say "wait, fix the tests first, then continue." That interrupt is the problem: it can leave a half-applied edit in the API code, discards in-flight work, and dumps the whole test-fixing effort into the main agent's context — none of which the main agent needs to remember afterwards.

Build the alternative:

1. The human spawns a **side-fixer pane** — a separate agent process, own context, own transcript — to do the orthogonal job.
2. The main agent is **never interrupted**. It keeps working.
3. The main agent is **held at its next Stop boundary** while the fixer is mid-flight, so it can't run or read the thing being repaired and get garbage results.
4. When the fixer finishes, the main agent resumes on that same turn with **one line** of handover — the *fact* of the change, not the fixer's reasoning.

Success = the human never presses Esc for this class of problem again.

### Non-goals

- Not a review channel. Diff review comments already have a path (queue → Stop hook → same agent). This is for *orthogonal work*, which is why a fresh agent is correct here and wrong there.
- Not a general orchestrator. No scheduler, no dependency graph, no agent-to-agent chat.
- Not a subagent. The fixer is a **peer process in its own pane**, not a Task-tool subagent (those block the parent turn and merge back into it — both things we're avoiding).

---

## Agent assignments

| Phase | Agent | Tools | Why |
|---|---|---|---|
| Phase 0 — Research | **Fresh research agent** (subagent or separate pane), read-only | Read, Grep, Glob, WebFetch/WebSearch. **No Edit/Write/Bash-mutations.** | Verifying hook schemas is context-heavy and throwaway. Keep it out of the implementing agent's window. Prompt must be self-contained — it has none of this conversation.<br>Deliverable: `scratchpad/hook-facts.md`, claims marked `[E]` (quoted from a fetched source + URL) or `[I]` (inference). |
| Phase 1–3 — Implementation | **Main implementing agent**, one, in-repo | Full | Holds the design intent across phases. Do not split implementation across parallel agents — they'd race on the same files. |
| Phase 4 — Review | **Fresh reviewer agent**, read-only | Read, Grep, Bash(read-only) | Reviews the finished diff unbiased by the reasoning that produced it. Findings go back to the *implementing* agent to fix. |

The research agent's prompt must state the installed Claude Code version explicitly and demand version-matched answers.

---

## Phase 0 — Research (blocking; no code yet)

Hook behavior has been moving fast and secondhand blog posts are stale. **Verify against the installed version before building on any of it.** Prefer, in order: the local install's own docs/schema → `code.claude.com/docs` → `anthropics/claude-code` releases & issues.

Answer each, with a URL + quote, marked `[E]`/`[I]`:

**A. Stop hook output schema (highest risk — the whole design rests on it)**
1. Does `Stop` accept `hookSpecificOutput.additionalContext` in *our installed version*, and does it continue the turn without rendering as a hook error? (Landed ~v2.1.163 — confirm ours.)
2. Exact JSON envelope: what nests under `hookSpecificOutput` vs. top level? (A top-level `additionalContext` is silently ignored on some events — confirm the shape that actually works.)
3. Fallback path if unsupported: `decision:"block"` + `reason`, or exit 2 + stderr. What does each *look like* in the UI?
4. `stop_hook_active` — when is it true, and does it survive a long hold?

**B. Holding vs. blocking**
5. Hook timeout for `command` hooks in our version (is it still ~10 min?). This bounds how long we can poll-hold.
6. Consecutive-block cap: `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP` — default value, is it per-turn or per-session, what happens on trip. *We intend to poll-hold inside one invocation rather than block repeatedly — confirm this doesn't count against the cap.*
7. Is the hook's stdout/stderr shown to the human, the model, or both?

**C. Spawning**
8. Confirm a hook is an arbitrary shell command that can `tmux split-window -d` and return immediately (detached spawn, hook exits fast). Any sandbox/env restriction on what a hook may spawn?
9. `claude -p` headless invocation: how to pass a prompt, set the working dir, and restrict tools (deny-list writes outside the fixer's scope).
10. **Confirm the negative:** there is no hook output field that spawns a subagent. If research finds one, stop and report — it changes the design.

**D. Prior art (short — don't rabbit-hole)**
11. Does any existing tool hold an agent at a boundary while a sibling process works? (claude-review-loop's Stop state machine is the closest known shape.) Note anything reusable; note plainly if the field is silent.

**Write findings to `scratchpad/hook-facts.md`. If A1 comes back negative, report before implementing — the payload changes, not the architecture.**

---

## Phase 1 — Flag protocol

The lock is a file, not a daemon. Per main-agent pane:

- `<state-dir>/<pane-id>/fixer.lock` — JSON: fixer pane id, PID, what it's touching, start time, human-readable reason.
- `<state-dir>/<pane-id>/handover` — one line the fixer writes on success. This is the *only* thing that reaches the main agent.

Rules:
- The fixer owns the lock; it creates on spawn and removes on exit (trap on EXIT so a crash still clears it).
- **Fail open, always.** Stale PID, expired deadline, malformed JSON, missing file → proceed as if unlocked. A dead fixer must never wedge the main agent.
- Hard deadline in the lock (default 5 min, well under the hook timeout). On expiry the hook proceeds and injects: *"a side task was running and did not finish; earlier results may be stale."*

---

## Phase 2 — Spawn path

Human triggers from the cockpit popup (keybind + one-line prompt for the fixer). The command:

1. Writes the lock **before** spawning (no race where the main agent slips past a boundary first).
2. `tmux split-window -d` a `claude` process, given: the fixer prompt, an explicit **scope fence** (which paths it may touch; deny the main agent's live edit surface), and the instruction to write `handover` then release the lock.
3. Returns immediately. Never blocks the caller.
4. Guard on lock existence — a second trigger on the same pane must refuse, not spawn a duplicate fixer.

The handover line the fixer writes should carry: what changed, the observable delta (`suite 118s → 8s`), and the staleness instruction (*"re-read any test file you had open; earlier timings are invalid"*). **Not** how it got there.

---

## Phase 3 — Stop hook hold

On the main agent's Stop:

1. No lock → return immediately. **Zero added latency in the common case.**
2. Lock present → poll (~1s) until it clears or the deadline passes. Log to stderr so the human sees *why* the pane is paused.
3. On release → return `additionalContext` containing the handover line + `"Continue with your task."`
4. On expiry → return the stale-warning line and proceed.
5. Whatever the outcome, **consume** `handover` (delete it) so the next boundary doesn't replay it.
6. Guard: if `stop_hook_active` is true, do not hold — return clean. Belt-and-braces against a rewake loop.

Interaction with the existing review queue: both deliver at the same boundary. Decide and document the order — recommended: **handover first, then queued review comments**, since the handover may invalidate what the review was based on.

---

## Explicit DON'Ts

- **Don't interrupt the main agent.** No Esc, no Ctrl+C, no synthetic keystrokes into a running turn. That's the failure mode this feature exists to remove.
- **Don't let the fixer's transcript reach the main agent.** One line, ~30 tokens. If the handover is a paragraph, it's wrong.
- **Don't block-and-rewake.** No repeated `decision:"block"` to stall the agent — it burns the block cap and wakes the model for nothing. One invocation, poll-hold inside it.
- **Don't hold without a deadline**, and don't hold when there's no lock.
- **Don't let the fixer write into the main agent's live edit surface.** Scope-fence it, or give it its own worktree. A stale-match Edit is the expensive failure, not tokens.
- **Don't spawn the fixer as a Task subagent.**
- **Don't build on any hook field Phase 0 didn't verify.**
- **Don't touch the existing review-comment queue path** beyond the documented ordering.

---

## Acceptance

- Main agent working, no fixer → Stop boundary latency unchanged.
- Fixer spawned mid-turn → main agent finishes its turn uninterrupted, then pauses at Stop with a visible reason.
- Fixer completes → main agent resumes on the same turn, context grew by one line.
- Fixer killed (`kill -9`) → main agent resumes within the deadline, with the stale warning.
- Two spawn attempts on one pane → second refuses.
- Long fixer (> deadline) → main agent proceeds, warned, not wedged.
- `scratchpad/hook-facts.md` exists and every hook field used in the code traces to an `[E]` claim in it.
