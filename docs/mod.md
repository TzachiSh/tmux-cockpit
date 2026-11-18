# The Claude mod

`mods/cockpit-events` is a Claude Code mod: a plugin of hooks that runs inside
Claude Code. It tells the cockpit about a turn the settings hooks never hear
end, an interrupted one. It is the Claude adapter of the agent-event schema
(docs/events.md), and it is a prototype of #1330: it is loaded by hand, and
`install.sh` does not install it.

Its plugin name and its folder are both `cockpit-events`. Claude Code reserves
plugin names that start with `claude-` for Anthropic's own plugins.

## What it reports

The mod reports only what `examples/claude-hooks.json` cannot. The settings
hooks keep every other state change.

| Claude Code event | line handed to the core |
|---|---|
| `turn.complete` on the main loop, reason `aborted` (Esc or Ctrl-C) | `turn.end`, outcome `aborted` |
| `turn.complete` on the main loop, reason `error` (an API error) | `turn.end`, outcome `error` |
| `turn.complete` on the main loop, reason `answer`, under the refused-ask rule below | `turn.end`, outcome `aborted` |
| anything else: an answer, a refusal, a subagent's turn, `turn.start`, a tool call | nothing |

Settings `Stop` does not fire on an interrupted turn, so before the mod the
pane stayed `running` until the next prompt. The mod never sends outcome
`answer`: a normal turn still ends through `Stop`, which knows about background
subagents (the #310 hold).

Each line is one agent-event v1 object with `agent` `claude`, the pane from
`TMUX_PANE`, the session id with its dashes taken out, and the time in UTC. It
carries no answer, prompt or tool text. `map.js` reads only `reason` and
`agentId` from the event, and the schema refuses any other key.

### The refused-ask rule

Esc on a permission prompt ends the turn with reason `answer`, not `aborted`,
and no `Stop` fires, so the pane stays on `needs-input`. The mod turns that
answer into outcome `aborted` only when all of these hold:

1. In this main turn, `tool.check` answered `ask` for a call X on the main loop
   (no `agentId`), with `approver` `person`.
2. Call X's `tool.call` ended with `isError`.
3. Every `ask` from any loop has had its `tool.call` end.
4. No agent turn is open: every `turn.start` with an `agentId` has had its
   `turn.complete`.
5. `$.agent.list()` shows no agent `pending`, `running` or `waiting`. The list
   misses workflow agents and the engine's own forks, which is why rule 4 is
   there too. A list that cannot be read counts as a running agent.

If any one is false, the mod sends nothing, and the pane is left as the
settings hooks leave it. The plan numbers these conditions 1, 2, 4, 5' and 6.

What the tests do not cover: an elicitation from a background loop the mod never
heard start, with an empty agent list. There the rule could read a refused
background prompt as the main turn's. Claude Code's events give no way to see
that loop, so this is a named gap, not a tested case. The test kit skips a hook
that rejects, so the `.catch(() => {})` around the push is kept as a defence no
test can pin, while the test "the hook does not wait for the push" pins that
the run is never awaited.

## How it runs

- `register.js` hooks `turn.start`, `tool.check`, `tool.call` and
  `turn.complete`. The first three only keep the state the refused-ask rule
  reads. Only `turn.complete` hands a line on.
- The pane is read with `$.env.get("TMUX_PANE")` at each `turn.complete`, since
  no `session.start` follows `/clear`. With no pane (Claude Code outside tmux)
  the mod runs nothing.
- The line goes on stdin to `scripts/cockpit-event`, found at
  `$.plugin.root/../../scripts/cockpit-event`. That ties the mod to a checkout
  of this repo: it must stay at `mods/cockpit-events` inside it.
- The run is fire and forget: `void $.process.run(...).catch(() => {})` with a
  1500 ms bound. The hook never waits for it and returns `next(e)`'s result, so
  a missing core or a dead tmux server cannot slow or fail a turn. The hook
  waits only for local lookups before passing on: the pane, the session id and,
  under the refused-ask rule, the agent list.
- Nothing runs from a tool hook. A tool hook's `$.process.run` is rejected as
  `aborted` when the tool is interrupted.

## Loading it

Start Claude Code with the folder as a plugin directory:

```sh
claude --plugin-dir ~/.tmux/plugins/tmux-cockpit/mods/cockpit-events
```

To turn it off, start Claude Code without the flag. The settings hooks work
the same with or without it.

Tested with Claude Code 2.1.327. The floor is 2.1.299, the release in which
`turn.start` began to fire for subagents.

## Testing it

```sh
claude plugin test mods/cockpit-events       # the contract tests, no model call
claude plugin validate mods/cockpit-events   # what the module hooks, calls and reads
```

`tests/register.test.ts` runs the mod against stub engine hooks and reads the
lines it hands to a stub `process.run`. `tests/map.test.ts` checks `map.js`
alone. CI does not run these, since it has no Claude Code. They run by hand,
and after a Claude Code upgrade.
