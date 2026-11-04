# tmux-cockpit

Shows the state of every AI coding agent in your tmux session in the window
tabs, pane borders and status bar, so you do not have to switch windows to find
out which one needs you. Keys and clicks then act on what you see: jump to a
blocked agent, review its diff, switch branch by clicking the branch in the
status bar (a tap on a phone), or Ctrl+double-click a line of output to open
the file it names, at that line.

```text
── ≡ ─ cockpit  ▲ elsewhere ───~/src/api ⎇ feat/jwt   [z]  ┬── ▲ Allow Bash: rm -rf .next? ───
⋮
 1 Rotate the JWT signing key ●  2 docs  3 Terraform plan ✕  4 Weekly notes ✓ ◆  5 logs
```

The bottom line is the window tabs, each with its window's worst state. The
top border holds the menu (`≡`), the session name, `▲ elsewhere` while an agent
in another session waits, and the active pane's path, branch and zoom button.
A waiting pane's border names what it is asking. Every one of them is a click,
or a tap on a phone.

| state | marker | colour | means |
|-------|--------|--------|-------|
| blocked | `▲` | red | waiting on **you**: a permission prompt |
| error | `✕` | orange | a tool failed |
| running | `●` | yellow | working |
| done | `✓` | green | finished, you have not picked it back up |
| idle | `○` | grey | an agent with nothing happening |
| unread review | `◆` | cyan | finished work nobody has read yet |

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
- jq, for `install.sh --hooks` and the review gate.
- Claude Code, or any agent that can run a command on lifecycle events.
- The `cockpit-tui` binary, which draws the board and the panel. `install.sh`
  fetches it with your `gh` login, else builds it (Rust 1.88 or newer), and
  stops with a message when it can do neither.
- [revdiff](https://github.com/umputun/revdiff), for the `prefix + r` review
  (tested with v1.13.0). Without it `prefix + r` says "revdiff not found".
  Install details: [docs/install.md](docs/install.md#requirements).
- A Nerd Font is not needed.

### With tpm

```tmux
set -g @plugin 'TzachiSh/tmux-cockpit'
run '~/.tmux/plugins/tpm/tpm'             # last
```

tpm runs plugins after every `set -g` in your config, so the cockpit options
you set are in place when the plugin loads. No theme plugin is needed: the
plugin draws the bar itself.

### Manual

```sh
git clone https://github.com/TzachiSh/tmux-cockpit ~/.tmux/plugins/tmux-cockpit
~/.tmux/plugins/tmux-cockpit/install.sh    # links tm and the tmux-* commands into ~/.local/bin
```

Then add this line to `~/.tmux.conf`, after the cockpit options you set and
before tpm:

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
| `prefix + a` | the menu: every action, one letter each; also `≡` on the border |
| `prefix + u` | jump to the pane that needs you; again for the next |
| `prefix + w` | the board: every agent pane, grouped by session, blocked first; `/` filters it as you type, `v` for live tiles, `x` kills an agent's pane, `o` resumes an ended one (needs `cockpit-tui`) |
| `prefix + f` | the panel: ask `claude -p` about this pane, paste the answer in (needs `cockpit-tui`); `^Z` in it hides it, and `prefix + f` on the pane brings it back |
| `prefix + r` | the board again, as the review queue: `r` on a row opens that agent's uncommitted diff in revdiff (needs `cockpit-tui`) |
| `prefix + i` | info about this pane: cwd, git root, state, review (`cockpit-tui info`; `i` in the board shows the same) |
| `prefix + s` | the sessions screen, `cockpit-tui sessions` (needs `cockpit-tui`) |
| `prefix + b` | switch branch (`cockpit-tui branch`; needs `cockpit-tui`) |
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
| `@cockpit_md_view` | `on` | `off`: a clicked markdown file opens in the editor, not the viewer; the file browser's `↵` then opens every file in the editor too |
| `@cockpit_review_window` | `off` | `on`: `prefix + r` opens the board in a window, not a popup |
| `@cockpit_board_window` | `off` | `on`: the board opens in a window, not a popup |
| `@cockpit_board_return` | `on` | `off`: `r` and `f` leave the board instead of coming back to it |
| `@cockpit_panel_mode` | `auto` | `pc` or `tablet` forces the panel's mode |
| `@cockpit_statusbar` | on | `off` keeps your own status line and borders |
| `@cockpit_touch` | `auto` | the touch layout (bar and sessions screen) below 100 columns |
| `@cockpit_icons` | unset | `ascii` or `nerd` for the menu and branch icons |
| `@cockpit_glyphs` | unset | `ascii` for plain glyphs in the board, the panel, info, the branch picker, `tm` and sessions; `nerd` draws the default there, but for the board's subagent badge |
| `@thm_<role>`, `@thm_<role>_light` | the plugin's palette | the colours; see [docs/status-bar.md](docs/status-bar.md#colours) |
| `@cockpit_rewake_cap` | `10` | cap on `/goal` rewakes per pane; `0` disables |
| `@cockpit_bg_poll` | `10` | seconds between polls of agent views' background state; `off` disables; an empty, invalid or `0` value falls back to 10 |

For a bar of your own, `@cockpit_statusbar off` and the
`#{E:@cockpit_where_path}`, `#{E:@cockpit_where_branch}` and
`#{E:@cockpit_fleet}` options: [docs/status-bar.md](docs/status-bar.md#a-bar-of-your-own).
The old `#{agent_*}` tokens are gone; the same page has the migration table.

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

Actions (jump, review, the board, the path picker) are shell scripts bound to keys.
The required `cockpit-tui` binary draws the board and the panel; it writes a
verb and a pane id, and the scripts act on it.

Details: [docs/states-and-hooks.md](docs/states-and-hooks.md).

## Troubleshooting

- **The status bar shows `#{agent_fleet}` literally.** The `#{agent_*}` tokens
  were removed. Use the options in the
  [migration table](docs/status-bar.md#moving-from-the-agent_-tokens);
  `install.sh --check` lists the lines that still carry one.
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
- [docs/status-bar.md](docs/status-bar.md): the bar, borders and tabs, colours, a bar of your own, `tmux-where`
- [docs/path-picker.md](docs/path-picker.md): Ctrl+double-click a path
- [docs/vscode-remote.md](docs/vscode-remote.md): VS Code over Remote-SSH and Coder, security
- [docs/board-and-panel.md](docs/board-and-panel.md) and [docs/tui.md](docs/tui.md): the `cockpit-tui` board and panel
- [docs/menus.md](docs/menus.md): the menu and moving windows
- [docs/phone-and-touch.md](docs/phone-and-touch.md): using it from a phone
- [docs/tm.md](docs/tm.md): the `tm` session launcher (the command stays; its picker is `cockpit-tui sessions`)
- [docs/review.md](docs/review.md): the review `r` opens from the board
- [docs/branch-picker.md](docs/branch-picker.md): the branch popup and the folder browser
- [docs/testing.md](docs/testing.md): running the tests

## License

MIT. See [LICENSE](LICENSE).
