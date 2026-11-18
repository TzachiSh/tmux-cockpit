# Agent events

An agent tells the cockpit what it is doing through an adapter. The adapter
turns the agent's own signals into **agent-event** lines and hands them to the
core, which turns each into one `scripts/agent-state` request. The core never
asks which agent sent a line: a new agent needs a new adapter and nothing else
(#1330).

The schema is `contracts/agent-event.schema.json`. Its examples, and the check
CI runs on them, are described in `contracts/README.md`.

## The line, agent-event v1

One JSON object per line, with these fields and no other:

| field | required | shape |
|-------|----------|-------|
| `schema_version` | yes | the integer `1` |
| `agent` | yes | `^[a-z][a-z0-9-]{1,23}$`, the adapter's fixed name |
| `kind` | yes | `turn.start`, `turn.end`, `tool.start`, `tool.end`, `input.needed` or `session.end` |
| `pane` | yes | the tmux pane id, `^%[0-9]{1,10}$` |
| `session` | yes | the agent's session id, 16 to 64 lowercase hex |
| `at` | yes | UTC, `2026-11-18T09:31:05Z`, an optional fraction |
| `outcome` | on `turn.end` only | `answer`, `aborted`, `refusal` or `error` |
| `failed` | optional, on `tool.end` only | `true` or `false` |

- `outcome` is required on `turn.end` and refused on every other kind.
- `failed` is refused on every kind but `tool.end`. A `tool.end` without it
  counts as `failed: false`.
- Any other key is refused (`additionalProperties: false`), so a line cannot
  carry an answer, a prompt, a tool's input or a path. `agent` is the one field
  that holds a word, at most 24 characters: the adapter's fixed name, a constant
  in that adapter's code, never derived from a session, project, path or user
  input.
- Ids are opaque lowercase hex. An adapter strips separators: a UUID such as
  Claude's session id becomes its 32 hex digits.
- Kinds are `noun.verb`, lower case, one dot.
- A change to any field bumps `schema_version`, by the rule in
  `contracts/README.md`.

Subagent kinds are left out of v1: a subagent's start can be missing while its
end arrives, and the subagent log already comes from the settings hooks. A
`session.start` is left out too: it fires once per load, never after `/clear`.

## What the core does with each kind

The core runs one `agent-state` request per line. Each request keeps the guard
it has today; the line numbers are `scripts/agent-state`.

| kind | request | guard today |
|------|---------|-------------|
| `turn.start` | `wake` | Lifts `done` or an unset state to `running` (398, 434), and keeps the review mark: only a human prompt clears it, which stays UserPromptSubmit's job (436-439). |
| `tool.start` | `running` | Refused when already `running` (394). Entering `running` clears the review mark (438). |
| `tool.end`, `failed: false` | `running` | The same as `tool.start`. |
| `tool.end`, `failed: true` | `error` | Refused on `error` or `needs-input` (396). |
| `input.needed` | `needs-input` | Refused when already `needs-input` (395). |
| `turn.end`, `answer` | `done` | Refused on `done`, `needs-input` or `error` (397). The core runs it with stdin from `/dev/null`, so the Stop-payload read (383) finds no subagents and does not hold the pane. |
| `turn.end`, `refusal` | `done` | The same as `answer`. |
| `turn.end`, `error` | `error` | The same as `tool.end` with `failed: true` (396). |
| `turn.end`, `aborted` | `interrupted` | New, added by #1330 T3, not yet in `agent-state`. Allowed from any state, no colour, clears `needs-input`, and keeps the review spool and `@cockpit_subs`, as the idle body does (430). |
| `session.end` | `off` | The ended-session body: clears the state, the review mark, the review spool and `@cockpit_subs` (400, 430). |

In v1 the Claude mod emits only `turn.end` with `aborted` and with `error`. The
settings hooks keep the rest, until #1330 T7 decides whether to move them.

## Adding a CLI agent

1. Add `adapters/<agent>/`, or `mods/<agent>-cockpit/` if the agent has an
   in-process plugin API. It turns the agent's own signals into agent-event v1
   lines with `agent: "<agent>"`. `agent` is the adapter's fixed name, a
   constant in that adapter's code, never derived from a session, project, path
   or user input.
2. The adapter provides:
   - the pane id and a session id. Ids are opaque lowercase hex; strip
     separators.
   - `turn.start`, and `turn.end` with an `outcome`
   - `tool.start` and `tool.end` if the agent has tools
   - `input.needed` if it can block on the user
   - `session.end` when it exits
3. It never puts agent text in a line. It hands each line to
   `scripts/cockpit-event` on stdin.
4. Add the agent's process name to the default agent list (`cockpit.tmux:63`)
   if it is missing.
5. Add contract rows: the native signal in, the expected agent-event lines out,
   and each line valid against the schema. Add one end-to-end row on a private
   socket.
6. Change nothing in `scripts/cockpit-event`, `scripts/agent-state` or the
   schema. A new kind or field is a schema v2 request to the core's owner.

## Who owns what

| path | job | owner |
|------|-----|-------|
| `contracts/agent-event.schema.json`, `contracts/examples/agent-event/` | the schema and its examples | the core |
| `scripts/cockpit-event` | reads, checks and maps each line to a request | the core |
| `scripts/agent-state` | the requests, `interrupted` included | the core |
| `tests/adapters/fake/` | a test-only second adapter | the core |
| `mods/claude-cockpit/` | the Claude mod adapter and its contract tests | the Claude adapter |
| `docs/events.md` | this page | the core |

An adapter codes against the merged schema and never edits it. A change goes to
the core's owner as a request.
