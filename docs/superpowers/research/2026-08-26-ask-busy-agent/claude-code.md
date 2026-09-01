# Claude Code 2.1.246: what it already gives a supervisor

Read from the docs on 2026-08-26 and checked against the installed binary where a command exists.

## /btw

`/btw <question>` answers from what is already in the conversation: the prompts, the replies, the tool results. It runs while Claude is working, does not interrupt the turn, and sees everything except the reply being written. No tools. One answer per ask, in an overlay; `f` forks a subagent with tools if the answer needs to go look. The question and answer never enter history; the newest 20 exchanges are replayed on each ask. Cheap while the prompt cache is warm. Terminal and VS Code only; there is no headless or cross-process `/btw`.

Doc: https://code.claude.com/docs/en/interactive-mode#side-questions-with-btw

## The queue

Typing and pressing Enter mid-turn queues the text above the prompt. A queued **message** is handed to Claude as soon as the running tool calls finish, inside the same turn; what is still queued when the turn ends becomes the next turn, one message each. A queued **command** or `!` shell line waits for the turn to end, except the ones that run at once (`/status`, and `/btw` by its own doc). `Esc` interrupts and sends the queue now; `Up` from the first input row takes the queue back into the box.

So the screenshot's question was not lost. It reached the agent after the 30 s shellcheck, as part of the work, with no separate answer.

Doc: https://code.claude.com/docs/en/interactive-mode#queue-messages-while-claude-works

## claude agents

`claude agents` is a full-screen view of every session on the machine grouped by state, one row each with a one-line summary written by a Haiku-class model (rewritten at turn end and every few minutes during a long turn; refreshed from output every 15 s between). `Space` peeks: a working session shows its status sentence, read-only; a waiting session shows the exact question with a reply box. Notification hook types `agent_needs_input` and `agent_completed` fire for it. Research preview.

`claude agents --json` here, 2.1.246, four live sessions:

```
{ "pid": 4134041, "cwd": "/home/tzachi/.tmux/plugins/tmux-cockpit", "kind": "interactive",
  "startedAt": 1787767465371, "sessionId": "098c323f-…", "name": "tmux-cockpit-ab", "status": "busy" }
```

`status` is `busy` or `idle` on this build; the doc also lists `waiting` with a `waitingFor` of `permission prompt | input needed | sandbox request | worker request | dialog open`. No hook is involved: the supervisor keeps `~/.claude/jobs/<id>/state.json` and the roster itself. `pid` is the claude process, a child of the pane's `pane_pid`.

Doc: https://code.claude.com/docs/en/agent-view

## Transcripts

`~/.claude/projects/<cwd with / → ->/<session-id>.jsonl`, one JSON object per line. Lines that matter for "what is it doing":

| line | fields |
|---|---|
| `type: user`, `message.content` a string | the human's prompt, `timestamp` |
| `type: assistant`, `content[].type == text` | what the agent said |
| `type: assistant`, `content[].type == tool_use` | `name`, `input.description` (Bash), `input.prompt` (Agent), `input.command`, `input.file_path` |
| `type: user`, `content[].type == tool_result` | the result, `sourceToolAssistantUUID` |
| `type: ai-title` | the title review-gate already greps |
| `type: attachment` (the most common non-message row), `last-prompt`, `custom-title`, `agent-name`, `permission-mode`, `system` (subtypes `stop_hook_summary`, `turn_duration`, …) | interleave after assistant rows; scan backwards past them |

Correction (2026-08-27, verified on 588 transcripts, PR #62 verified.md F): a DENIED permission is recorded, as the tool_use plus a `tool_result` "The user doesn't want to proceed with this tool use…"; a PENDING permission prompt has no marker at all and is byte-identical to a still-running tool. The earlier claim that the tool_use is written only after approval was wrong. Subagent transcripts live in the sibling directory `<session-uuid>/subagents/`, never inline.

Every hook payload carries `session_id` and `transcript_path` (SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, Stop, Notification, StatusLine, SessionEnd); cockpit's review-gate reads both at Stop and keeps only the id, as `@cockpit_session`.

The no-model "doing" read, measured 0.1 s on a 1.2 MB live file:

```sh
tail -n 300 "$tx" | jq -r 'select(.type=="assistant") | .message.content[]? | select(.type=="tool_use")
  | .name + ": " + ((.input.description // .input.prompt // .input.command // .input.file_path // "")|tostring|.[0:80])' | tail -1
tail -n 300 "$tx" | jq -r 'select(.type=="assistant") | .message.content[]? | select(.type=="text") | .text' | grep -v '^$' | tail -1
```

Output on the busy peer session: `Bash: Lint the changed scripts and show diff stat` and `While the suite and rig run: shellcheck on the two scripts, and the independent review pass on the diff.`

## Asking from outside: the fork spike

```
claude -p --resume ed6abf76-… --fork-session --max-turns 1 --model haiku --output-format json \
  "In two lines: what were you doing in this session, and what was the last thing you did?"
```

12 s wall, rc 0, correct two-line answer (the #21 drag-flag fix and the PR #33/#35 merges). A new transcript `f7247ad4-….jsonl` appeared; the original file did not change. Usage: 10 input, 18,307 cache read, 133,059 cache creation, 421 output, $0.27. The cache-creation line is the whole cost: a cold session re-reads its context. Against a session whose agent hit the API seconds ago the prefix is cached and the ask is cents. `--fork-session` is documented as leaving the original "unchanged on disk"; the locking is not documented, and this spike ran against an idle session. One more run against a busy one before anyone ships it.

Not tested: `--resume` without `--fork-session` on a live session. Do not: the sessions doc says two processes resuming one id interleave into one transcript.

## Asking from outside: the observer spike

The pattern `aside` (side chats per session, tool-less `claude -p` over the transcript tail) and `claude-code-ui` (a tool name while working, a model only at rest) use. Reduce the tail to what a reader needs, cap it, ask a tool-less haiku:

```sh
tail -n 400 "$tx" | jq -r '
  if .type=="user" and (.message.content|type)=="string" then "USER " + .timestamp[11:19] + ": " + .message.content[0:400]
  elif .type=="assistant" then (.message.content[]? | if .type=="text" then "AGENT: " + .text[0:400]
    elif .type=="tool_use" then "TOOL " + .name + ": " + ((.input.description // .input.prompt // .input.command // .input.file_path // "")|tostring|.[0:120])
    else empty end)
  else empty end' | tail -c 24000 > ctx
env -u CLAUDECODE claude -p --model haiku --tools '' --no-session-persistence --output-format json \
  --append-system-prompt "You are a read-only observer of another coding agent's session. Answer from the transcript excerpt only, two lines max." \
  "$(cat ctx)

---
Question: what is this agent doing right now, and is it waiting on me or stuck?"
```

On the busy peer session: 7 s, $0.05 (10 input, 24,866 cache creation, 306 output; the 4 KB excerpt is a fraction of that, the rest is the global CLAUDE.md and skills `claude -p` loads; `--setting-sources ''` or a bare `--system-prompt` would trim it), answer: "waiting on a code review report that was dispatched earlier. All tests passed (1014 assertions, 0 failures across two full suite runs) and the rig showed no delta vs master." The agent's session file was never opened for writing and no new session was left behind. `env -u CLAUDECODE` is required when launched from inside a Claude pane or hook, or the nested claude refuses.

## Notification hook

Types: `permission_prompt` (fires about 6 s into a prompt nobody answered), `idle_prompt` (60 s idle), `agent_needs_input`, `agent_completed`, `elicitation_dialog`, `auth_success`, `quota_auto_resume_*`. Cockpit's `agent-state notify` reads only `message` and substring-matches `needs your permission` and `waiting for your input`; the doc's `notification_type` field is the stable name (the tcsm.md note from the review-lenses research flagged this). AskUserQuestion has no type of its own in the doc; `agent_needs_input` is the likely carrier, unconfirmed.

Docs: https://code.claude.com/docs/en/hooks, https://code.claude.com/docs/en/sessions
