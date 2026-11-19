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
| anything else: an answer, a refusal, a subagent's turn, any other event | nothing |

Settings `Stop` does not fire on an interrupted turn, so before the mod the
pane stayed `running` until the next prompt. The mod never sends outcome
`answer`: a normal turn still ends through `Stop`, which knows about background
subagents (the #310 hold).

Each line is one agent-event v1 object with `agent` `claude`, the pane from
`TMUX_PANE`, the session id with its dashes taken out, and the time in UTC. It
carries no answer, prompt or tool text. `map.js` reads only `reason` and
`agentId` from the event, and the schema refuses any other key.

## Known v1 gap: Esc on a permission prompt

Esc on a permission prompt ends the turn with reason `answer`, not `aborted`,
and no `Stop` fires, so the pane stays on `needs-input` until the next prompt.
The mod leaves that as it is. To the mod, a refused ask looks the same as an ask
the person approved whose command then failed: both end the call with
`isError`, and the model can answer after either. A rule that read the first as
an interrupt also marked the second, a normal turn, as aborted (#1337 review).

Telling them apart needs a fact the mod does not have yet. #1330 T6 measures
whether Esc on a prompt and an approved, failing call differ in the order of
`turn.step` events. A rule for this case comes back only on that measured fact.

What the tests do not cover: the `.catch(() => {})` around the push. The test
kit skips a hook that rejects, so no test can pin that catch. The test "the
hook does not wait for the push" does pin that the run is never awaited.

## How it runs

- `register.js` has one hook, on `turn.complete`, and keeps no state between
  events. It hooks no tool event: Claude Code's API types say a hook on
  `tool.call` or `tool.run` turns off moving that tool to the background and
  its live output.
- The pane is read with `$.env.get("TMUX_PANE")` at each `turn.complete`, since
  no `session.start` follows `/clear`. With no pane (Claude Code outside tmux)
  the mod runs nothing.
- The line goes on stdin to `scripts/cockpit-event`, found at
  `$.plugin.root/../../scripts/cockpit-event`. That ties the mod to a checkout
  of this repo: it must stay at `mods/cockpit-events` inside it.
- `scripts/cockpit-event` arrives with #1330 T3. Until it lands, every push is
  rejected, the catch drops it, and loading the mod changes nothing.
- The run is fire and forget: `void $.process.run(...).catch(() => {})` with a
  1500 ms bound. The hook never waits for it and returns `next(e)`'s result, so
  a missing core or a dead tmux server cannot slow or fail a turn. The hook
  waits only for two local lookups before passing on: the pane and the session
  id.

## Loading it

Start Claude Code with the folder as a plugin directory:

```sh
claude --plugin-dir ~/.tmux/plugins/tmux-cockpit/mods/cockpit-events
```

To turn it off, start Claude Code without the flag. The settings hooks work
the same with or without it.

Tested with Claude Code 2.1.327. The floor, 2.1.299, is inferred from the
CHANGELOG and not tested.

## Testing it

```sh
claude plugin test mods/cockpit-events       # the contract tests, no model call
claude plugin validate mods/cockpit-events   # what the module hooks, calls and reads
```

`tests/register.test.ts` runs the mod against stub engine hooks and reads the
lines it hands to a stub `process.run`. `tests/map.test.ts` checks `map.js`
alone. CI does not run these, since it has no Claude Code. They run by hand,
and after a Claude Code upgrade.
