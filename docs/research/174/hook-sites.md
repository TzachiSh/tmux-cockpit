# Where hooks fire (lane research-hook-sites)

## Question

Which hook events fire in the front-end process and which in the background worker, with what
environment and payload, for (a) `claude` run directly in a tmux pane and (b) a background
session viewed through agent view? Claude Code 2.1.316.

## Sources

| tag | source |
|---|---|
| D-hooks | https://code.claude.com/docs/en/hooks.md |
| D-agent | https://code.claude.com/docs/en/agent-view.md |
| P | `/proc/<pid>/environ`, `ps -eo pid,ppid,args` (key names and TMUX/TMUX_PANE/CLAUDE_* values only) |
| R | `~/.claude/daemon/roster.json` |
| S | `~/.claude/sessions/<pid>.json` |
| E | experiment on a private tmux server (case a) |

## Process tree (P, R)

```
front end         claude                                  pane %3
supervisor        claude daemon run --origin transient --spawned-by {...pid:<front end>}
pty host          claude bg-pty-host ... pty/<short>.sock
worker            .../versions/2.1.316 --session-id <uuid> ...   (or claude bg-spare ...)
```

`roster.json` `workers.*.pid` is the pty-host pid. The process that runs the model and owns
`~/.claude/sessions/<pid>.json` is its child (roster `replPid`).

## Environment per process (P)

| process | TMUX | TMUX_PANE | CLAUDE_CODE_SESSION_KIND | other |
|---|---|---|---|---|
| front end | set | `%3` | unset | |
| supervisor | set | `%3` | unset | `CLAUDE_CODE_ENTRYPOINT=cli` |
| pty host | unset | unset | `bg` | `CLAUDE_BG_BACKEND=daemon`, `CLAUDE_JOB_DIR`, `CLAUDE_BG_RENDEZVOUS_SOCK` |
| worker | unset | unset | `bg` | same |
| worker's MCP child | unset | unset | unset | `CLAUDE_CODE_SESSION_ID=<worker's session>`, `CLAUDECODE=1` |

## Case (a): `claude` directly in a pane (E)

Private server `tmux -L r174hooks<pid> -f /dev/null`; `claude -p --model haiku --max-turns 2
--settings <temp>` started with `send-keys` in pane `%0`; prompt: run `echo hi`, reply "ok".
Each hook appended its env and payload to a log.

| event | fired | TMUX_PANE | TMUX | SESSION_KIND | payload session_id | transcript_path |
|---|---|---|---|---|---|---|
| SessionStart | yes | `%0` | set | unset | `27c1704e-...` | present |
| UserPromptSubmit | yes | `%0` | set | unset | same | present |
| PreToolUse | yes | `%0` | set | unset | same | present |
| PostToolUse | yes | `%0` | set | unset | same | present |
| Stop | yes | `%0` | set | unset | same | present |
| SessionEnd | yes | `%0` | set | unset | same | present |
| Notification | not triggered by a one-turn `-p` run | | | | | |

`CLAUDE_CODE_SESSION_ID` in the hook env equalled the payload `session_id` on every event.

## Case (b): background session in agent view

| event | runs in | TMUX_PANE | evidence |
|---|---|---|---|
| Notification `agent_needs_input` / `agent_completed` | the front end, "while agent view is open in a terminal" | inferred `%3` | D-hooks matcher table; the stuck write on #174 |
| SessionStart ... SessionEnd for the bg session | inferred: the worker | inferred unset | worker env (P); no bg hook fire observed |

D-agent: "The session takes over the terminal as a full interactive Claude Code session."
"While attached, the session behaves like any other Claude Code session." "An attached session
doesn't report its status to your terminal."

## Not established

| gap | why |
|---|---|
| the env of a hook child spawned by a bg worker | needs a live bg session; the daemon is shared with the user's live sessions, so none was started |
| whether an attached front end fires its own SessionStart, UserPromptSubmit, ... | docs silent; not tested for the same reason |
| payload shape of the relayed Notification | not captured |

## Implications

- In a plain pane, the pane comes straight from the hook env.
- A hook run from a worker has no pane (inferred from process env).
- An ancestor walk from a worker reaches the supervisor, which carries the first front end's
  pane for every session.
