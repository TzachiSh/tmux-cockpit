# tmux-panestate

Shows what every AI coding agent in your tmux session is doing, without you
having to switch windows to find out.

```text
 claude-main ●    claude-2 ▲    coder ○
─ NEEDS YOU 2:claude-2 ────────────────────────────────────────
```

| state | marker | colour | means |
|-------|--------|--------|-------|
| blocked | `▲` | red | waiting on **you** — a permission prompt |
| error | `✕` | orange | a tool failed |
| running | `●` | yellow | working |
| done | `✓` | green | finished, you have not picked it back up |
| idle | `○` | grey | no agent, or nothing happening |

Shape carries the state as well as colour, so it survives a greyscale
screenshot and a red/green colourblind reader.

## Where it shows up

- **Window tabs** — one marker per window. Worst state in the window wins, so a
  blocked pane is never hidden behind a busy one.
- **Pane border** — the whole top border takes the state colour. Colour is
  pre-attentive: you see which pane needs you without reading anything.
- **Active pane's border** — names any *other* window that is blocked, since
  you cannot see its border from here.

Nothing is drawn when nothing is happening.

## How it works

Claude Code hooks call `scripts/agent-state`, which writes a per-pane tmux
option:

```sh
tmux set-option -p -t "$TMUX_PANE" @panestate running
```

Everything you see is then plain tmux format syntax in `~/.tmux.conf` reading
that option. **A status redraw runs no shell at all.** The option dies with its
pane, so there is no stale state and nothing to garbage collect.

### State ranking

`needs-input` > `error` > `running` > `done`

A lower state may not overwrite a higher one. Only work actually resuming
(`running`) or an explicit `off` clears them. This exists because `Stop` fires
when a *blocked* turn ends — with the permission prompt still on screen — and
letting that write `done` turned a red "answer me" into a green "finished" on a
timer.

## Install

```sh
git clone <this repo> ~/.tmux/plugins/tmux-panestate
```

Add to `~/.tmux.conf` (see `examples/tmux.conf` for the full block):

```tmux
run-shell ~/.tmux/plugins/tmux-panestate/panestate.tmux
```

Wire the hooks in `~/.claude/settings.json`:

| hook | state |
|------|-------|
| `SessionStart` | `off` — a resumed pane must not inherit old state |
| `UserPromptSubmit` | `running` |
| `PermissionRequest` | `needs-input` |
| `PostToolUse` | `running` |
| `PostToolUseFailure` | `error` |
| `SubagentStart` / `SubagentStop` | `running` |
| `TaskCompleted` / `Stop` | `done` |
| `Notification` | `notify` (parses the JSON on stdin) |
| `SessionEnd` | `off` |

## Requirements

- **tmux >= 3.5** — per-pane `pane-border-style` accepting a format. On 3.4 the
  border colour silently does nothing; the window markers still work.
- Claude Code (or any agent that can run a command on lifecycle events).
- A Nerd Font is *not* required. The markers are plain Unicode geometry.

## Also included

`scripts/tmux-where` — `pane_current_path` is tmux's **local** view, so a pane
sitting in ssh reports this machine's path and git branch. This prints the ssh
destination instead, and blanks the branch. It exits early on
`#{pane_current_command}` so the common case costs nothing.

## Tests

```sh
tests/run.sh
```

47 cases. They skip rather than fail when no tmux server is running.

## Known limits

- A session started **before** a hook was added never fires it — Claude Code
  snapshots hooks per session id. Such a pane stays grey until it restarts.
- A long "thinking" phase fires no hook, so a working pane can read as idle.
  CPU is not a usable substitute: an agent waiting on the API is
  indistinguishable from an idle one.
