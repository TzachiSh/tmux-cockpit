# The status line and borders

tmux-cockpit draws its own status line and pane borders (tmux 3.8). It uses
native formats only: on a redraw, no shell runs except for an ssh pane's path.
The formats live in `scripts/lib/statusbar.conf`, and `cockpit.tmux` sources
that file when the plugin loads.

## What is where

```
── ≡ ─ cockpit  ▲ elsewhere ─────~/src/api ⎇ feat/jwt   [z]  ┬── ▲ Allow Bash: rm -rf .next? ───
⋮
 1 Rotate the JWT signing key ▲  2 docs  3 Terraform plan ✕  4 Weekly notes ✓ ◆  5 logs
```

- **The bottom line holds only the window tabs.** Each tab shows the
  window's worst agent state: `▲` needs you, `✕` error, `●` running, `✓` done,
  `○` idle. A tab with an unread review also shows `◆`. The current tab's
  number is in the accent colour.
- **`◔NN%` on a tab** once the window's agent has filled 80% of its context
  (`@panestate_ctx`, written by `scripts/agent-context` as Claude Code's
  `statusLine`). Its cells come out of the tab's label; a compact tab shows the
  bare `◔`.
- **The top border, at the top-left pane (or the zoomed pane), shows the
  corner:**
  - `≡`, which opens the menu;
  - the session name, which opens the session picker;
  - `▲ elsewhere` while an agent in another session needs you. If none does
    but one has failed, it shows `✕ elsewhere`.
- **The active pane's border shows, on the right:**
  - the path;
  - the branch;
  - on a split window, `[z]` (`[u]` while zoomed).
- **A pane that waits shows `▲ <question>`.** The question comes first: the
  branch, and then the path, get only the room it leaves.
- **Other panes show their state glyph and title, dimmed.**
- **Kill pane and Close window are menu rows,** each with a confirm. They are
  no longer border buttons.

The layout is the same on every device. A touch client
(`@cockpit_touch on`, or unset and under 100 columns) adds:

- `‹` and `›` at the ends of the window line;
- tabs at least 5 cells wide;
- a padded corner on a pane at least 40 columns wide.

## Options

Set these before the plugin's `run-shell` line. `@cockpit_statusbar` and
`@cockpit_menu_key` are read when the plugin loads; `@cockpit_touch` and
`@cockpit_icons` are read on every redraw, so a change shows at once.

| option | values |
|---|---|
| `@cockpit_statusbar` | `off` leaves your status line and borders alone |
| `@cockpit_touch` | `on`, `off`, or unset (auto: under 100 columns) |
| `@cockpit_icons` | the menu and branch icons: unset `≡` `⎇`, `ascii` `=` `git:`, `nerd` U+F0C9 U+E0A0. The state glyphs are the same in every tier: the shape carries the state |
| `@cockpit_menu_key` | the menu's key, `a` by default; `off` binds none |

## Colours

The palette is a set of roles. Each role is a hex option with a dark and a
light value. Set any of them before the `run-shell` line to override it.

| role | dark | light | used for |
|---|---|---|---|
| `@thm_bg` | `#16181d` | `#f7f7f5` | the status background |
| `@thm_fg` | `#e6e4df` | `#1b1e24` | text |
| `@thm_surface` | `#2a2f3a` | `#e1e5ec` | the current tab, menus, popups |
| `@thm_dim` | `#9097a3` | `#5c626c` | other tabs, the path, idle |
| `@thm_accent` | `#6cb6ff` | `#0a5fc2` | the session name, the branch, the active border |
| `@thm_needs` | `#ff5f6d` | `#c4122f` | `▲` |
| `@thm_error` | `#ff9e3d` | `#9a4c00` | `✕` |
| `@thm_running` | `#e6c84f` | `#806200` | `●` |
| `@thm_done` | `#5fd38d` | `#1d7439` | `✓` |
| `@thm_review` | `#5fd3d3` | `#00737a` | `◆` |
| `@thm_sel` | `#3a4252` | `#c8d1df` | the selection band in cockpit-tui |

The light values are the same names with `_light` added, for example
`@thm_needs_light`.

tmux 3.8 has ten theme colour slots, and the plugin points each one at a role
(`dark-theme-red` reads `@thm_needs`, and so on). That gives each client its
own light or dark set:

- An unknown terminal background counts as dark. On a light terminal that tmux
  cannot detect, set `set -g theme light`.
- `set -g theme terminal` turns the slots off and uses the terminal's own
  colours.
- When the environment sets `NO_COLOR`, every slot is set to `default`.

## A bar of your own

With `@cockpit_statusbar off`, the plugin still publishes these options:

| format | gives |
|---|---|
| `#{E:@cockpit_where_path}` | the pane's path with `~` for home, or the ssh destination |
| `#{E:@cockpit_where_branch}` | the pane's branch, blank in an ssh pane |
| `#{E:@cockpit_fleet}` | `▲N ✕N ◆N`: agents waiting, failed, and unreviewed, across every session; empty when nothing needs you |

Some ranges you can mark with `#[range=control|N]…#[norange]`:

- `control|2` opens the folder browser;
- `control|3` opens the branch picker;
- `control|1` opens the menu;
- `control|0` opens the session picker.

`4` to `6` are free for you. tmux uses `7` to `9`.

```tmux
set -g @cockpit_statusbar off
set -g status-right "#{E:@cockpit_fleet}#[range=control|2]#{E:@cockpit_where_path}#[norange] #[range=control|3]#{E:@cockpit_where_branch}#[norange]"
run-shell ~/.tmux/plugins/tmux-cockpit/cockpit.tmux
```

The branch is a pane option. Hooks write it on a focus change, a new window
or split, and a shell prompt (OSC 133). So `status-interval` can stay at the
default.

### Moving from the `#{agent_*}` tokens

Nothing rewrites the old tokens any more. They now print as literal text.

| old | new |
|---|---|
| `#{agent_where_path}` | `#{E:@cockpit_where_path}` |
| `#{agent_where_branch}` | `#{E:@cockpit_where_branch}` |
| `#{agent_fleet}` | `#{E:@cockpit_fleet}` |
| `@cockpit_ai_key` | `@cockpit_menu_key` (the old name still works, with a notice) |

If you used the plugin's catppuccin setup, delete all of it:

- the `source -F …catppuccin*.conf` lines;
- every `@catppuccin_*` option;
- the `@thm_*` overrides (catppuccin names such as `@thm_red`, `@thm_peach`
  and `@thm_overlay_0` are not read any more);
- `status-left` and `status-right`;
- `status-interval 2` and the `refresh-client -S` hooks;
- the `pane-border-*` block;
- `window-style` and `@cockpit_blocked_bg`;
- the example's `bind m display-menu`. Stock `prefix m` (mark pane) is back,
  and `prefix a` is the menu.
