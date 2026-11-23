# The Claude mod

`mods/cockpit-events` is a Claude Code mod: a plugin of hooks that runs inside
Claude Code. It tells the cockpit what the settings hooks never hear: a turn
that was interrupted, and an ask the person left with Esc. It is the Claude
adapter of the agent-event schema (docs/events.md). `install.sh --hooks`
installs it beside the settings hooks (see Installing it).

Its plugin name and its folder are both `cockpit-events`. Claude Code reserves
plugin names that start with `claude-` for Anthropic's own plugins.

## What it reports

The mod reports only what `examples/claude-hooks.json` cannot. The settings
hooks keep every other state change.

| Claude Code event | line handed to the core |
|---|---|
| `turn.complete` on the main loop, reason `aborted` (Esc or Ctrl-C) | `turn.end`, outcome `aborted` |
| `turn.complete` on the main loop, reason `error` (an API error) | `turn.end`, outcome `error` |
| `turn.complete` on the main loop, reason `answer` | `input.ended` |
| anything else: a refusal, a subagent's turn, any other event | nothing |

Settings `Stop` does not fire on an interrupted turn, so before the mod the
pane stayed `running` until the next prompt. The mod never sends `turn.end`
with outcome `answer`: a normal turn still ends through `Stop`, which knows
about background subagents (the #310 hold). `input.ended` writes no `done` and
leaves `@cockpit_subs` alone, so that hold stays `Stop`'s.

Each line is one agent-event object, `schema_version` 2, with `agent` `claude`,
the pane from `TMUX_PANE`, the session id with its dashes taken out, and the
time in UTC. It carries no answer, prompt or tool text. `map.js` reads only
`reason` and `agentId` from the event, and the schema refuses any other key.

## Esc on a permission prompt

Esc on a permission prompt ends the main turn with reason `answer`, not
`aborted`, and no `Stop` fires: 12 of 12 rounds on Claude Code 2.1.330 (#1332).
Before `input.ended` the pane stayed on `needs-input` until the next prompt.

The mod sends `input.ended` at every main-loop answer and keeps no state. It
does not tell a refused ask from one the person approved whose command then
failed: both end the call with `isError` (#1337 review). The core tells them
apart by the ask's key (#1403). The approved call's `PostToolUseFailure` has
already ended its ask (#1341), so `input.ended` finds none and changes nothing.
A refused ask fires no hook, so its key is still there, and `input.ended` ends
it. Only a main-thread key, `/<tool>`, is ended: a subagent's ask, an ask with
no key and every state but `needs-input` stay.

Known limits (#1332):

- The push has a 1500 ms bound, so a late `input.ended` could end the next
  turn's new ask, as a late `aborted` can today.
- While the lead waits on its own prompt, a teammate's prompt forwarded to the
  same pane shares its ▲, and Esc on the lead's prompt clears both (the
  two-asks limit of #1403).
- Not verified: if Claude Code ran a `PermissionRequest` in the lead's process
  for a forwarded teammate request with no `agent_id`, its key would read as the
  lead's, and the lead's turn end would clear that teammate's ▲.

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
  of this repo: it must stay at `mods/cockpit-events` inside it. It holds
  because the marketplace `install.sh` adds is that checkout, a local folder:
  Claude Code loads a plugin from such a marketplace "in place from its path
  inside the marketplace folder" (Claude Code's plugins/loading.md, "In-place
  and copied plugins"). So the mod and the core always come from one commit.
- The run is fire and forget: `void $.process.run(...).catch(() => {})` with a
  1500 ms bound. The hook never waits for it and returns `next(e)`'s result, so
  a missing core or a dead tmux server cannot slow or fail a turn. The hook
  waits only for two local lookups before passing on: the pane and the session
  id.

## Installing it

`install.sh --hooks` installs the mod. The repo root holds a marketplace,
`.claude-plugin/marketplace.json`, named `tmux-cockpit`, with one plugin,
`cockpit-events` from `./mods/cockpit-events`. `install.sh` asks claude's own
plugin CLI, at user scope:

1. `claude plugin marketplace list --json`, for a marketplace named
   `tmux-cockpit`.
2. With none, `claude plugin marketplace add <this checkout> --scope user`.
3. `claude plugin install cockpit-events@tmux-cockpit --scope user`.

A `tmux-cockpit` marketplace that is another folder or a repository is not
this checkout's: `install.sh` names it and installs nothing. To move the mod
to this checkout, run `install.sh --uninstall --hooks` from the other one, or
`claude plugin marketplace remove tmux-cockpit`, first. With no `claude` on
PATH, `install.sh` says so and skips the mod. A failed call prints a FAILED
line naming the command, and `install.sh` exits 1. `--dry-run` runs no
`claude` and prints the calls it would make. Each call runs without
`TMUX_PANE`: a `claude plugin` call can fire the SessionEnd hooks, and their
`agent-state off` would otherwise clear the pane `install.sh` runs in.

A new session loads the mod at start, and a session already open loads it
after `/reload-plugins`. Edits to the checkout, a `git pull` among them,
reach the next session or `/reload-plugins` with no reinstall.

`install.sh --uninstall --hooks` and `install.sh --hooks --remove` remove it
with `claude plugin marketplace remove tmux-cockpit --scope user`, which also
uninstalls the plugin. They run it only when the marketplace is this checkout,
and name any other one and leave it.

`install.sh --check` reports the mod on its `mod` row. It reads what the
plugin CLI wrote to the `settings.json` in Claude Code's config dir
(`$CLAUDE_CONFIG_DIR`, else `~/.claude`), with `jq`, and starts no
`claude`. The row is ok when the `tmux-cockpit` marketplace there is this
checkout's folder and `cockpit-events@tmux-cockpit` is enabled. It warns, with
the fix, when the mod is not installed (`install.sh --hooks`), is disabled
(`claude plugin enable cockpit-events@tmux-cockpit`), or comes from another
marketplace, which it names. Until the cockpit hooks are merged it skips, and
the hooks row says what to run. A `tmux-cockpit` marketplace known only to
`plugins/known_marketplaces.json` in that dir, and not to settings.json, reads as
not installed, and `install.sh --hooks` does not change that.

`install.sh` is the one supported install. A marketplace added from the
GitHub repository is copied into Claude Code's plugin cache, which keeps the
mod's folder alone, so the mod cannot reach `scripts/cockpit-event` and
reports nothing.

The settings hooks work the same with or without the mod.

Tested with Claude Code 2.1.330. The floor, 2.1.299, is inferred from the
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

`tsconfig.json` is tracked as Claude Code writes it, so a session leaves git clean (#1423).
