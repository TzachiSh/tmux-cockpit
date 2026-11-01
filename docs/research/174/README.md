# Issue #174: hooks of agent-view background sessions find no pane

Research, 2026-11-01, Claude Code 2.1.316 on Linux (WSL2). Three read-only lanes:
[hook-sites.md](hook-sites.md) (where each hook runs, with what env),
[attach-detect.md](attach-detect.md) (can a script tell which session a front end shows),
[state-sources.md](state-sources.md) (what `claude agents --json` reports). No code changed.

## What happened

Pane `%3` stayed on `needs-input` through 15+ tool calls after the question was answered.
The pane runs an interactive Claude front end in agent view. The conversation itself runs in a
background worker under a shared `claude daemon run` supervisor. Inferred, not observed on a
hook child: the front end relayed the worker's `agent_needs_input` notification with
`TMUX_PANE=%3`, so `needs-input` landed, and the hooks that would clear it run in the worker,
which has no `TMUX`/`TMUX_PANE`, so `scripts/agent-state` exits 0 with no pane.

## What was measured

| fact | evidence |
|---|---|
| `claude -p --max-turns 2` run directly in a pane (interactive `claude` not run): SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, Stop, SessionEnd all ran with `TMUX_PANE` set; `CLAUDE_CODE_SESSION_ID` = payload `session_id` | hook-sites.md, case (a), private server |
| background workers: `CLAUDE_CODE_SESSION_KIND=bg`, no `TMUX`/`TMUX_PANE` | hook-sites.md, `/proc/<pid>/environ` |
| supervisor's env carries the pane of the front end that started it; one supervisor serves every session | hook-sites.md, attach-detect.md E2 |
| `agent_needs_input` and `agent_completed` fire for background sessions "while agent view is open in a terminal"; no other relayed event is documented (`agent_needs_input` also covers an agent-team setup question and an auto-mode notice) | hooks.md, quoted in state-sources.md |
| no method found names the session a front end is attached to (one snapshot, one front end) | attach-detect.md, methods 1-10 |
| `claude agents --json --all`: documented as "the supported way to read session state from outside Claude Code"; `state` `blocked` "always means the session needs something from you" | state-sources.md |
| `status` is not enough: both blocked sessions read `status: idle` | state-sources.md |
| one `claude agents --json` call: median 376 ms wall, about 0.27 s CPU (n=5, 4 sessions) | state-sources.md |

## Decisions (owner, 2026-11-01)

1. An agent-view pane shows the worst state across all background sessions:
   blocked > failed > working > done.
2. One poller per tmux server runs `claude agents --json --all` every 10 s, only while some
   pane is an agent view. The interval is an option, and `off` disables it.
3. A hook inside a background worker exits 0 without touching tmux. PR #185 (parent walk)
   closed unmerged.

## Rejected

| idea | why |
|---|---|
| walk the parent chain for `TMUX_PANE` (PR #185) | the nearest ancestor with a pane is the shared supervisor, which carries the first front end's pane; every session lands there |
| guess the attached session from attach-journal `msgsLoaded` joined to roster `firedInteractiveMarks` | undocumented; would break on a switch or a version change (not observed) |
| record session_id -> pane when the front end relays a notification | wrong pane after switching sessions in agent view |
| poll every 2 s (offered option, not chosen) | derived about 13% of a core while running, for clearing a marker 8 s sooner |

## History

Times in UTC.

- 2026-10-31 21:44: #174 filed with the daemon-env cause.
- 2026-10-31 21:58: PR #185 (parent walk) opened.
- 2026-10-31 22:10-22:15: review DO NOT MERGE on #185; cause corrected on #174; this research
  round dispatched.
- 2026-11-01 05:57: decisions answered; #185 closed.
