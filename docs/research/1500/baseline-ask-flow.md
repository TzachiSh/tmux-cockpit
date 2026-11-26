Written by ask-baseline for tmux-cockpit-2a [fa458f] on 2026-11-26. It read master a11b1776.

# Baseline: how an AskUserQuestion reaches the cockpit

ask-baseline, for tmux-cockpit-2a [fa458f]. Read-only, master a11b1776. No tmux, cargo, tests or claude were run.

The cockpit never reads an AskUserQuestion's options anywhere. The board shows only the first line of question 1, the pane border shows only the tool name, and the user answers in Claude Code's own dialog. Claude Code can answer the question from a hook, so auto-answer is possible.

## 1. How a question reaches the cockpit today

- **Hooks** (`examples/claude-hooks.json`):
  - PreToolUse writes `running` (:58-60), PermissionRequest writes `needs-input` (:55-57), Notification runs `notify` (:79-81), PostToolUse writes `running` (:61-64).
  - No hook returns a decision.
- **PermissionRequest path** (`scripts/agent-state:283-303`):
  - It reads 4 KB of the payload.
  - `@panestate_msg` gets the tool name only, so it reads `AskUserQuestion`. `@panestate_ask` gets `<agent_id>/AskUserQuestion`.
  - Both go through `safe_word`: only `[A-Za-z0-9._-]`, at most 64 bytes (:204-220).
  - The ask ends only on a later call from the same agent (:236-245, :500-502). Running, off and interrupted clear both options (:579-582).
- **Notification path:** `permission_prompt` and `elicitation_dialog` mean needs-input (:347). The message loses control characters and is cut to 120 characters (:377-378). A permission_prompt keeps the text PermissionRequest wrote (:369-373).
- **Background jobs** get only `"<name> needs you"` (`scripts/agent-poll:346`), capped at 120 (`agent-state:532`).
- **Free-text rule:** `last_assistant_message` is never stored (`agent-state:392`; `docs/states-and-hooks.md:669-670`, the #289 hazard). The event schema refuses answer, prompt and tool-input fields (`docs/events.md:31-35`).
- **What is shown:**
  - The pane border shows ▲ plus `@panestate_msg`, cut to the free width (`scripts/lib/statusbar.conf:172,187`). *Inferred:* it reads `AskUserQuestion`, assuming PermissionRequest fires for that tool. The docs imply it does (section 4), but I did not run it. Before #1330 it read "Claude needs your permission" (#453 body).
  - The fleet bar shows only the ▲N count (`agent-state:80`).
  - The board swaps in the transcript's question (`tui/src/data.rs:562-565`). `question()` keeps the first non-blank line of question 1, prefixed `(+N)` for the rest (`tui/src/data/transcript.rs:495-510`). "doing" shows `AskUserQuestion: <header 1>` (:479-484).
  - That text is cut to the row width in the list (`tui/src/board.rs:2314`), folded in tiles (`board.rs:586`), and repeated on the info screen (`board.rs:2886`; `tui/src/data/info.rs:333-337`).
  - The tiles (`v`) show a live capture of the pane (`docs/board-and-panel.md:8`), so the dialog is visible there, cut to the tile.
- **What the user must do:** `a` on a ▲ agent only jumps: "there is no inline reply yet" (`board.rs:4603-4607`). The reply overlay ("phase 2", `docs/tui.md:176`) is not built. The user answers in Claude Code's own dialog in that pane.
- **What is read:**
  - Hook payload: never the question. Only `tool_name` and `agent_id` from the first 4 KB.
  - Transcript: only question 1's first line, the count, and header 1. `["header"]` at `transcript.rs:482` is the only place that indexes into `questions`, so options, descriptions, preview and multiSelect are never read.
  - Pane capture: the panel's Explain only, 200 lines (`tui/src/panel/capture.rs:67-68`).

## 2. The ✦ panel

- **Shows today:** Write (`^S`), Fix (`^F`), Explain and Ask-about-code (`docs/board-and-panel.md:40-71`). The ✦ name comes from the Termux key "✦ Panel" (`examples/termux.properties:1,7`) and epic #409's title.
  - Explain sees the dialog only as screen text, classed `waiting_choice` (`capture.rs:686-727`). The prompt tells it to name the option to pick, with no `<prompt>` (`tui/prompts/explain.txt:13-15`, example :30-35).
  - "Recommended" exists only on the panel's own questions (`explain.txt:19-23`, `write.txt:86-91`), drawn as a star (`tui/src/panel/view.rs:687`). There is no why, no easiest or lowest-risk option, and no visual.
  - The flow renderer is "not wired into Explain yet" (`docs/tui.md:1482-1483`). Batches hold up to 3 questions, shown as `question k of N` (`docs/tui.md:1436-1478`; `tui/src/panel/parse.rs:21`).
- **Envelope and prompts:**
  - `<screen>` is redacted. `<session state>` and `<agent>` carry doing, asked and the work folder, 300 characters each (`tui/src/panel/envelope.rs:296-330`). Then `<conversation>` and `<questions_left>` (`explain.txt:4`).
  - Rounds: 3 for Write, 6 for Explain (`tui/src/panel/app.rs:60-61`). Calls use `claude -p --tools ""` and load no settings (`docs/board-and-panel.md:43-45,130-135`).
- **Measurement:** `panel_eval` sends Write calls only (`tui/examples/panel_eval.rs:1-3`). It scores ask-vs-write FPR/FNR, keyword `must`/`must_not`, pass^k and cost, plus a `--judge` rubric calibrated on owner labels (`docs/tui.md:307-344`). Explain is unmeasured (#1405; `docs/tui.md:340-341`).
- **Issues and owners**, from bodies and comments:
  - #409, the epic: filed by the orchestrator on an owner request, progress by tmux-cockpit-27.
  - #935, the Explain redesign: tmux-cockpit-84 [0f80bb], "the panel files owner". Its X2 says "no automatic actions". On 2026-11-23 the owner ruled through 2a that #1405 is its first task when the redesign starts. It has not started.
  - #628: filed by 27, narrowed by db to the owner-label step.
  - #718: parked by the owner on 2026-11-10 (via f2). #731: parked by f2.
  - #927: filed by 84.
  - #515, #516, #517, #788: unsigned, no comments.
  - #789: research; f2's follow-up plan is "not started".
  - #1405: filed by 2a, deferred into #935.
  - #442: owner request; 2a commented on 2026-11-25.
  - From shared memory, not the issues: the current panel session is [f7b145]. I did not check whether it is alive.

## 3. Auto-answer or auto-approve today

Nothing exists. `git grep` for auto-answer, auto-approve, `permissionDecision`, `updatedInput` and policy finds no product code. The only hook stdout is `additionalContext` (`scripts/review-gate:193`, `examples/claude-hooks.json:49`).

- `review-gate` flags an unreviewed diff, hands over a queued review and caps `/goal` rewakes (`review-gate:1-4`).
- `agent-poll` is the state source for background jobs (`agent-poll:1-18`).
- The #789 router decides the panel's own ask-or-write choice (`docs/research/789/README.md:1-3`; `tui/src/panel/route.rs:1-2`).
- No past answers are stored. `tui/src/data/fold.rs:89-93` only closes an ask when its `tool_result` arrives.

## 4. Claude Code mechanisms an auto-answer could use

- **PreToolUse:** `permissionDecision: "allow"` plus `updatedInput` that echoes `questions` and adds `answers: {"<question text>": "<label>"}` answers the question. "allow" alone is not enough. The tool-input table says to supply `answers` "via `updatedInput` to answer programmatically". Source: https://code.claude.com/docs/en/hooks, section "Tools that require user interaction" (read through a fetch agent).
- **PermissionRequest:** the decision is `{behavior: allow, updatedInput, updatedPermissions}` or `{behavior: deny, message, interrupt}`. The docs say "`AskUserQuestion` and `ExitPlanMode` need this field". The same schema shows as text in the 2.1.332 binary (`~/.local/share/claude/versions/2.1.332`, read, not run). `defer` works only under `-p`.
- **So a hook can answer, not only allow or deny.**
- **Tool schema:** 1-4 questions, a header of at most 12 characters, 2-4 options each with label, description and an optional preview, `multiSelect`, `answers` and `annotations` (`mods/cockpit-events/.claude-plugin/types/claude-code-tools/index.d.ts:179-205`; local, gitignored).
- **Mod API:**
  - A `tool.call` hook that returns `{ result }` answers in place of the call (`mods/cockpit-events/.claude-plugin/types/claude-code/index.d.ts:4972`).
  - `ui.render` on `AskUserQuestion` can rewrite the questions as drawn, within the tool's schema (:12991-13013). That is a place to show a recommendation inside the dialog itself.
  - `$.ui.ask` opens the same dialog from a mod (:2844-2858).
  - The catch: the mod hooks no tool event, because a `tool.call` hook turns off backgrounding and live output for that tool (`docs/mod.md:65-69`). *Inferred:* a matcher on AskUserQuestion alone limits that cost to AskUserQuestion.
- **Constraints:** hooks fail open and never block the agent (`agent-state:620`), and the event schema forbids question text (`docs/events.md:31-35`). An auto-answer needs an owner ruling on both.

## 5. Likely "hard to follow" points

- The border names the tool, not the question (inferred, section 1).
- The board shows one line of question 1. Questions 2-4 appear only as `(+N)`, and options never appear (`transcript.rs:495-510`).
- Order is lost: there is a count but no "k of N", and nothing says which question is on screen. *Inferred:* Claude Code shows one question per tab.
- With two asks waiting, the key is the newer one's. Answering the newer one first clears the ▲ while the older one still waits (`docs/states-and-hooks.md:41-43`).
- Subagent and teammate transcript rows are skipped (`fold.rs:75-77`), so their asks show the tool name only. A forwarded teammate prompt shares the lead's ▲ (`docs/mod.md:52-55`).
- The ask shows only while it is the newest `tool_use` (`fold.rs:193`; `transcript.rs:640-645`). *Inferred:* a parallel call made after the ask hides it.
- Over 256 KB written after an open ask, the transcript window no longer reaches it (`data.rs:497-498`).
- A background job's ask is only `"<name> needs you"` (`agent-poll:346`).
- `a` only jumps (`board.rs:4603-4607`). Explain is unmeasured and never reads the ask as data. No answer history exists to learn standing choices from.
