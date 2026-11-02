# tmux-cockpit

Shows the state of every AI coding agent in your tmux session in the window
tabs, pane borders and status bar, so you do not have to switch windows to find
out which one needs you. Keys and clicks then act on what you see: jump to a
blocked agent, review its diff, switch branch by clicking the branch in the
status bar (a tap on a phone), or Ctrl+double-click a line of output to open
the file it names, at that line.

```text
 claude-main ●    claude-2 ▲    coder ○
─ NEEDS YOU 2:claude-2 ────────────────────────────────────────
```

A blocked pane's own border, visible in a split but not the one you are
focused on, names the ask instead of just flagging one:

```text
─ NEEDS YOU Claude needs your permission to use Bash ───────────
```

| state | marker | colour | means |
|-------|--------|--------|-------|
| blocked | `▲` | red | waiting on **you**: a permission prompt |
| error | `✕` | orange | a tool failed |
| running | `●` | yellow | working |
| done | `✓` | green | finished, you have not picked it back up |
| idle | `○` | grey | no agent, or nothing happening |

Shape carries the state as well as colour, so it survives a greyscale
screenshot and a red/green colourblind reader.

## Table of contents

- [Install](#install)
- [Quick start](#quick-start)
- [Usage](#usage)
- [Configuration](#configuration)
- [How it works](#how-it-works)
- [Troubleshooting](#troubleshooting)
- [Documentation](#documentation)
- [License](#license)

## Install

### Requirements

- tmux 3.8 or newer. Below it the plugin says so and binds nothing.
- fzf 0.73 or newer, for the pickers. `bootstrap.sh` fetches a newer one into
  `~/.local/bin` when the distro's is older.
- jq, for `install.sh --hooks` and the review gate.
- Claude Code, or any agent that can run a command on lifecycle events.
- Optional: the `cockpit-tui` binary for the board and the panel. Get it with
  `set -g @cockpit_tui_fetch on` (fetched when the config loads),
  `scripts/tui-fetch`, or `make tui` (Rust 1.88 or newer).
- Optional: `bat`. A Nerd Font is not needed.

### With tpm

```tmux
set -g @plugin 'catppuccin/tmux'          # declare it first
set -g @plugin 'TzachiSh/tmux-cockpit'
run '~/.tmux/plugins/tpm/tpm'             # last
```

tpm runs plugins after every `set -g` in your config, which is the order the
`#{agent_*}` tokens need.

### Manual

```sh
git clone https://github.com/TzachiSh/tmux-cockpit ~/.tmux/plugins/tmux-cockpit
~/.tmux/plugins/tmux-cockpit/install.sh    # links tm and the tmux-* commands into ~/.local/bin
```

Then add this line to `~/.tmux.conf`, after every option that uses an
`#{agent_*}` token and before tpm:

```tmux
run-shell ~/.tmux/plugins/tmux-cockpit/cockpit.tmux
```

`examples/tmux.conf` has the full block. `install.sh --check` reports what is
missing or misconfigured and writes nothing. More detail, including
`bootstrap.sh` and paths with spaces: [docs/install.md](docs/install.md).

## Quick start

1. Install as above and reload tmux (`tmux source ~/.tmux.conf`).
2. Wire the Claude Code hooks into `~/.claude/settings.json`:

   ```sh
   ~/.tmux/plugins/tmux-cockpit/install.sh --hooks --dry-run   # preview
   ~/.tmux/plugins/tmux-cockpit/install.sh --hooks             # merge
   ```

   The merge only adds cockpit's hooks; your own are left alone. Re-run it
   after every plugin update.
3. Start `claude` in a few panes. Tabs and borders take the state colours as
   the agents work.
4. Press `prefix + u` to jump to the pane that needs you.

Sessions that were already running before step 2 do not fire the new hooks
until they restart.

## Usage

| key | does |
|-----|------|
| `prefix + a` | the agents menu: every action below, one letter each |
| `prefix + u` | jump to the pane that needs you; again for the next |
| `prefix + w` | the board: every agent pane as a live tile (needs `cockpit-tui`) |
| `prefix + f` | the panel: ask `claude -p` about this pane, paste the answer in (needs `cockpit-tui`) |
| `prefix + r` | review an agent's uncommitted diff (being replaced, #228) |
| `prefix + i` | info about this pane: cwd, git root, state, review (being folded into the board, #228) |
| `prefix + s` | session picker (its fzf picker is being replaced, #228) |
| `prefix + M` | move this window to another session |
| `prefix + e` | hide the status line and borders (full screen) |
| Ctrl+double-click | pick a file path off a line of output and open it |

Every key, one line each: [KEYS.md](KEYS.md). What each key replaces and how
to rebind it: [docs/configuration.md](docs/configuration.md).

## Configuration

Set these before the `run-shell` line (tpm users: anywhere before `run`).
None is required.

| option | default | does |
|--------|---------|------|
| `@cockpit_agents` | `^(claude\|codex\|aider\|cursor\|opencode\|gemini\|crush)$` | which commands count as an agent |
| `@cockpit_<name>_key` | see [configuration](docs/configuration.md) | rebind a key; `off` restores stock tmux |
| `@cockpit_popup_width`, `@cockpit_popup_height` | `70%` | popup size |
| `@cockpit_editor` | unset | editor for click-to-open and the review's `^E`; a terminal editor |
| `@cockpit_open_single` | unset | `line` or `always`: open a lone path without the picker |
| `@cockpit_review_window` | `off` | `on`: the review opens in a window, not a popup |
| `@cockpit_board_window` | `off` | `on`: the board opens in a window, not a popup |
| `@cockpit_panel_mode` | `auto` | `pc` or `tablet` forces the panel's mode |
| `@cockpit_touch` | `auto` | touch layout for the session picker below 100 columns |
| `@cockpit_glyphs` | unset | `ascii` or `nerd` glyph sets |
| `@cockpit_blocked_bg` | `#52303a` | the tint behind a blocked pane |
| `@cockpit_rewake_cap` | `10` | cap on `/goal` rewakes per pane; `0` disables |

Status-bar tokens: `#{agent_fleet}`, `#{agent_where_path}` and
`#{agent_where_branch}`. They work only in the options listed in
[docs/status-bar.md](docs/status-bar.md#status-bar-tokens).

## How it works

Claude Code hooks call `scripts/agent-state`, which writes a per-pane tmux
option such as `@panestate running`. The tabs, borders and status bar are plain
tmux format strings that read that option, so a status redraw runs no shell.
The option dies with its pane, so there is nothing to clean up.

`needs-input` and `error` are sticky: a `Stop` or an idle event does not clear
them, and `error` does not overwrite `needs-input`. Only work resuming
(`running`) or an explicit `off` clears them.

The formats believe the state only while the pane is running a command that
matches `@cockpit_agents`, so a crashed agent does not leave a stale marker.

Actions (jump, review, the pickers, the board) are shell scripts bound to keys.
The optional `cockpit-tui` binary draws the board and the panel; it writes a
verb and a pane id, and the scripts act on it.

Details: [docs/states-and-hooks.md](docs/states-and-hooks.md).

## Troubleshooting

- **The status bar shows `#{agent_fleet}` literally.** The `run-shell` line is
  above the option that uses the token. Move it below, or use tpm.
- **A pane stays grey while its agent works.** The session started before the
  hooks were installed; restart it. A long thinking phase also fires no hook.
- **Ctrl+double-click opens a GUI editor instead of the picker.** The terminal
  is following an OSC 8 link. Use `Ctrl`+right-click on agent output; see
  [docs/path-picker.md](docs/path-picker.md#when-the-terminal-opens-the-file-itself).
- **A phone terminal stacks copies of the status line.** Use the
  `terminal-overrides` line from the example configs; see
  [docs/phone-and-touch.md](docs/phone-and-touch.md).
- **Something else.** Run `install.sh --check`.

## Documentation

- [KEYS.md](KEYS.md): every key
- [docs/install.md](docs/install.md): install, bootstrap, tpm, requirements
- [docs/configuration.md](docs/configuration.md): keys, options, popup size
- [docs/states-and-hooks.md](docs/states-and-hooks.md): the state model, the hooks, `/goal`, known limits
- [docs/status-bar.md](docs/status-bar.md): tabs, borders, tokens, the blocked tint, `tmux-where`
- [docs/path-picker.md](docs/path-picker.md): Ctrl+double-click a path
- [docs/vscode-remote.md](docs/vscode-remote.md): VS Code over Remote-SSH and Coder, security
- [docs/board-and-panel.md](docs/board-and-panel.md) and [docs/tui.md](docs/tui.md): the `cockpit-tui` board and panel
- [docs/menus.md](docs/menus.md): the agents menu and moving windows
- [docs/phone-and-touch.md](docs/phone-and-touch.md): using it from a phone
- [docs/tm.md](docs/tm.md): the `tm` session launcher (the command stays; its fzf picker is being replaced, #228)
- [docs/review.md](docs/review.md): the `prefix + r` review (being replaced by the board, #228)
- [docs/branch-picker.md](docs/branch-picker.md): the branch picker and folder browser (changing, #228)
- [docs/testing.md](docs/testing.md): running the tests

## License

MIT. See [LICENSE](LICENSE).
