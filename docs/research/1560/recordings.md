Written by tmux-cockpit-dd [8f35e6] on 2026-11-27 for #1562. It read master 24379f96.

# Recordings: Stop payloads and the hooks of a background-task wake

Ten real sessions on Claude Code 2.1.332 recorded every hook payload around a background shell, a Monitor, a subagent with its own shell, a permission prompt and an API error. Claude Code's own cost estimate for all runs is $0.0218.

## Answers

| Question | Answer |
|---|---|
| Which hook fires when a finished background task wakes the session? | `UserPromptSubmit`. Its `prompt` is a `<task-notification>` block with the task id, the output file, a status and a summary. It came for a finished shell, for each Monitor event and for a subagent's report. |
| Does `idle_prompt` arrive while a background shell runs? | Yes, 60.0 s after the Stop, with the shell still running for another 49 s. |
| Is a plain `Bash` call with `run_in_background` the same as a shell started from Code Mode? | Yes. Both list `"type":"shell","status":"running"` with `id`, `description` and `command`, and both wake with `UserPromptSubmit`. |
| Does anything fire when a task is stopped from the tasks dialog? | No. No hook fired in the 27 s that followed, and the session was not woken. |
| Does a subagent that waits on its own shell stay in the parent's list? | No. The parent's next Stop lists only the shell, and the pane read `done` for 38 s while the subagent waited. |

## What follows for the epic

- A pane held on `running` after a Stop would drop to idle 60 s later: `agent-state` writes `off` for `idle_prompt` over `running` (`scripts/agent-state:350`, `:508`). A mutant that counts shells showed it in the test block: step 5 of the replay read idle.
- A hold needs a check of its own, since a stop from the tasks dialog fires nothing.
- The wake is reliable where a task ends by itself or its process is killed.

## How the rig ran

Each run was one interactive session with model `claude-haiku-5-5`, in a pane of a private tmux server started with `-f /dev/null`. A shell script drove it with `send-keys` and sampled `@panestate` every 0.5 s. The real login was used. `--setting-sources project,local` and `--strict-mcp-config` kept the owner's hooks, plugins and MCP servers out. `--settings` gave a recorder on every hook event, a status line that saved the cost, and the cockpit's own state hooks from `examples/claude-hooks.json` without review-gate. Permission mode was `auto`, and `default` for the permission case.

## Runs

Times are seconds after the prompt was sent.

| Run | What it did | Hooks in order | Pane state |
|---|---|---|---|
| p0 | a turn with no tool | UserPromptSubmit 0.0, Stop 1.1 with `[]`, Notification `idle_prompt` 61.2 | done from 1.6 |
| p2 | plain Bash, `run_in_background`, `sleep 110` | UserPromptSubmit 0.1, PreToolUse 1.5, PostToolUse 2.1, Stop 3.2 with one shell, `idle_prompt` 63.2, UserPromptSubmit 112.1, Stop 113.2 with `[]`, `idle_prompt` 173.3 | done 3.7 to 112.6, running, done 113.6 |
| c | Code Mode, a REPL shell that outlived its timeout, `sleep 60` | two PreToolUse and two PostToolUse (REPL and Bash), Stop 5.3 with one shell, UserPromptSubmit 62.3, Stop 63.5 with `[]`, `idle_prompt` 123.5 | done 6.5 to 63.6, running, done 63.8 |
| m | Monitor, two events 25 s apart | Stop 3.5 with one monitor, UserPromptSubmit 27.9 (the event), Stop 29.4 still with the monitor, UserPromptSubmit 52.7 (the end), Stop 53.8 with `[]` | done between each wake |
| x | `sleep 622` in the background, then its process killed from outside at about 24 s | Stop 3.9 with one shell, UserPromptSubmit 26.0, PreToolUse and PostToolUse, Stop 30.1 with `[]` | done 4.1 to 26.3 |
| k | `sleep 611` in the background, then stopped in the tasks dialog (Down, Enter, x) at 24 s | Stop 2.6 with one shell, then nothing until SessionEnd at 51.2 | done from 3.1 |
| s | a background subagent that starts its own background shell and waits | Stop 3.7 with the subagent, SubagentStop 6.9, UserPromptSubmit 9.6, Stop 10.8 with only the shell, SubagentStart 49.6, UserPromptSubmit 51.4 (an `<agent-message>` hand-back), SubagentStop 51.4, Stop 52.4 with `[]` | running with subs 1 until 12.1, done until 49.9 |
| n | Bash in default mode | PreToolUse 0.5, PermissionRequest 0.6, Notification `permission_prompt` 6.6, PostToolUse 19.7 after Enter, Stop 20.9 | needs-input 0.9 to 20.7 |
| e | a model name that does not exist | UserPromptSubmit 0.1, StopFailure 0.9 with `error` `model_not_found` and no `background_tasks` key | error from 1.1 |
| o (twice) | a background loop that prints a line every 5 s | as p2 | as p2 |

## What a payload says about a background shell

- `PostToolUse` for the Bash call carries `tool_response.backgroundTaskId`, the same id the Stop lists.
- A Stop entry has `id`, `type`, `status`, `description` and `command`. A subagent entry has `agent_type` and no `command`. Only `status` `running` was seen. A finished task is not listed.
- No payload carries a pid.
- The wake's `prompt` names the output file: `/tmp/claude-<uid>/<project>/<session>/tasks/<id>.output`. The same folder holds `scratchpad_dir`, which every payload carries. The file grew while the shell printed, 33 bytes then 55 bytes 10 s later. The path is not documented.
- The shell is a direct child of the claude process and leads its own session and process group.

## Not measured

- An artifact watcher's entry (anthropics/claude-code#98816), a `workflow` entry and a `teammate` entry.
- Any `status` other than `running`.
- `idle_prompt` while only a Monitor runs: its events came sooner than 60 s.
- A dev server beyond 3 minutes, and the time limit of a background shell.
- macOS, and any Claude Code version but 2.1.332.

## Fixtures

Seventeen of these payloads are in `tests/fixtures/hook-payloads/`, with the home and project paths replaced. The block "agent-state: recorded hook payloads walk the state by event table (#1566)" in `tests/run.sh` feeds each one from each of the five states.
