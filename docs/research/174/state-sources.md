# Claude Code's own state sources for background sessions (lane research-state-sources)

## Question

What do Claude Code's own state sources say about a background (agent-view) session's state,
so cockpit can show blocked, running or idle for it without a hook that knows its pane?

## Sources

| source | used for |
|---|---|
| `claude agents --help`, `claude agents --json` (run locally) | fields, values, timing |
| https://code.claude.com/docs/en/agent-view.md ("List sessions as JSON", "Read session state from a script", "The supervisor process") | meaning, stability |
| https://code.claude.com/docs/en/hooks.md (Notification matchers) | background notifications |
| https://code.claude.com/docs/en/statusline.md | status line |
| `tui/src/data.rs`, `scripts/agent-state` | cockpit's current paths |

## Fields

`--json` ran 5 times; identical listings, 4 background sessions, no interactive ones.

| field | values seen | meaning per docs |
|---|---|---|
| `kind` | `background` | `interactive` or `background` |
| `state` | `working` x2, `blocked` x2 | `working`, `blocked`, `done`, `failed`, `stopped`. "`blocked` always means the session needs something from you before it can continue." "A session that finished its turn and is waiting for your next instruction reads `done`, not `blocked`." |
| `status` | `idle` x3, `busy` x1 | `busy`, `waiting` or `idle`, while the process is alive |
| `waitingFor` | never seen (0 of 20 entries) | present when `status` is `waiting`: `permission prompt`, `input needed`, `sandbox request`, `worker request`, `dialog open` |
| `pid` | integer | process id, while alive |
| `id` | 8 hex chars | short id for `claude attach`/`logs`/`stop` |
| `sessionId` | uuid | for `claude --resume` |
| `name` | session name, or the id | display name |
| `cwd`, `startedAt` | path, Unix ms | |

- Docs: "`claude agents --json` is the supported way to read session state from outside Claude
  Code, for example from a status bar, a scheduler". "The files under `~/.claude/jobs/<id>/` are
  not a stable interface."
- Docs: "Agent view is in research preview. The interface and keyboard shortcuts may change as
  the feature evolves." No separate statement on the JSON schema.
- Docs recommend polling `claude agents --json --all`, which keeps listing exited sessions;
  without `--all`, completed sessions drop out.
- Both `blocked` sessions read `status: idle`, so `state`, not `status`, carries "needs you".

## Hooks and notifications for background sessions

| finding | quote | source |
|---|---|---|
| a background session starts waiting | `agent_needs_input`: "A background session starts waiting on your input while agent view is open in a terminal." | hooks.md |
| a background session finishes | `agent_completed`: "A background session finishes or fails. Fires only while agent view is open in a terminal." | hooks.md |
| agent view's own count | "The count refreshes about every ten seconds while the terminal is focused and immediately when focus returns" | agent-view.md |
| per-session hook or statusLine config for background sessions | not documented | absence in the three pages |

## Cost

| measurement | n | conditions | result |
|---|---|---|---|
| wall per `claude agents --json` | 5 | 4 sessions, WSL2, live box | 276, 364, 376, 381, 383 ms; median 376 ms |
| real/user/sys | 3 | same | real 0.29-0.32 s; user about 0.20 s; sys 0.07-0.09 s; user+sys about 0.27 s |
| poller at 2 s | derived | linear from CPU | about 13% of a core |
| poller at 10 s | derived | same | about 3% of a core |

Scaling with session count was not measured. A separate `CLAUDE_CONFIG_DIR` runs a separate
supervisor, so a poller needs the target sessions' environment.

## Cockpit paths

- `scripts/agent-state` writes `@panestate` per pane, and the window and fleet summaries, with
  compare-then-set. A poller writes through it.
- `tui/src/data.rs` caches `agents --json` for 60 s, for pid-to-sessionId only.
- `docs/tui.md` and the tui tests keep `agents --json` off the render path for its cost.

## Not established

- `waitingFor` values in practice; `kind: interactive` rows; cost scaling with session count.
- How an attached pane's pid relates to the session pid (`claude attach` not run).

## Implications

- There is a documented hook-free source, and `state: blocked` is exactly "needs the human".
- It cannot run on a hook's hot path; one poller per tmux server at 10 s costs about 3% of a core.
- Background notifications fire only while agent view is open, and nothing clears them.
- Unknown `state` values should be ignored, since the feature is a research preview.
