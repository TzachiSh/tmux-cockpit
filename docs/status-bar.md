# Status bar, borders and tabs

tmux-cockpit draws its own status line and pane borders (tmux 3.8). It uses
native formats only: on a redraw, no shell runs except for an ssh pane's path.
The formats live in `scripts/lib/statusbar.conf`, and `cockpit.tmux` sources
that file when the plugin loads. Nothing is drawn for a state when nothing is
happening.

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
  `statusLine`). It goes on top of the label, and the tab budget takes every
  gauge's cells off the room the tabs share; a compact tab shows the bare `◔`.
  A cut label never leaves a space before its `…`.
- **The top border, at the top-left pane (or the zoomed pane), shows the
  corner:**
  - `≡`, which opens the menu. While the tmux prefix is held it turns into a
    reversed accent block of the same width, so you can see the prefix is on;
  - the session name, which opens the sessions screen;
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
| `@thm_accent` | `#6cb6ff` | `#0a5fc2` | the session name, the branch, the active border, the held-prefix ≡ |
| `@thm_needs` | `#ff5f6d` | `#c4122f` | `▲` |
| `@thm_error` | `#ff9e3d` | `#9a4c00` | `✕` |
| `@thm_running` | `#e6c84f` | `#806200` | `●` |
| `@thm_done` | `#5fd38d` | `#1d7439` | `✓` |
| `@thm_review` | `#5fd3d3` | `#00737a` | `◆` |
| `@thm_sel` | `#3a4252` | `#c8d1df` | the selected row in the pickers, and the band on the selected row of cockpit-tui's board list |
| `@thm_faint` | `#717885` | `#7c828c` | cockpit-tui's frames and separators only, never text: 4.0 / 3.6 on bg, 3.0 / 3.1 on surface |

The light values are the same names with `_light` added, for example
`@thm_needs_light`.

tmux 3.8 has ten theme colour slots, and the plugin points each one at a role
(`dark-theme-red` reads `@thm_needs`, and so on). That gives each client its
own light or dark set:

- An unknown terminal background counts as dark.
- `set -g theme terminal` turns the slots off and uses the terminal's own
  colours.
- When the environment sets `NO_COLOR`, every slot is set to `default`.
- With `@cockpit_statusbar off` the plugin leaves the slots alone; tmux's own
  (or yours) stay.
- On a 16-colour client the slots fall back to ANSI: error is magenta, so it
  never shares yellow with running.
- A light terminal that tmux cannot detect needs `set -g theme light`. The
  board, the panel and the pickers run in popups, which cannot ask the terminal,
  so this option is also what switches them to the light roles.

## A bar of your own

With `@cockpit_statusbar off`, the plugin still publishes these options:

| format | gives |
|---|---|
| `#{E:@cockpit_where_path}` | the pane's path with `~` for home, or the ssh destination |
| `#{E:@cockpit_where_branch}` | the pane's branch, blank in an ssh pane |
| `#{E:@cockpit_fleet}` | `▲N ✕N ◆N ✦N`: agents waiting, failed, and unreviewed, and background subagents running (hidden in touch mode), across every session; empty when none |

Some ranges you can mark with `#[range=control|N]…#[norange]`:

- `control|2` opens the folder browser;
- `control|3` opens the branch popup;
- `control|1` opens the menu;
- `control|0` opens the sessions screen.

`4` to `6` are free for you. tmux uses `7` to `9`.

```tmux
set -g @cockpit_statusbar off
set -g status-right "#{E:@cockpit_fleet}#[range=control|2]#{E:@cockpit_where_path}#[norange] #[range=control|3]#{E:@cockpit_where_branch}#[norange]"
run-shell ~/.tmux/plugins/tmux-cockpit/cockpit.tmux
```

The branch is a pane option the hooks write (see below), so `status-interval`
can stay at the default.

`#{E:@cockpit_fleet}` counts **window slots**, not distinct panes. A session
made with `tmux new-session -t` is a second *view* of one set of windows, and
`link-window` puts a single window in two sessions; either way a shared blocked
pane is counted once per view. So the count can read high on a grouped setup,
never low.

The counts are written by `scripts/agent-state` at each state transition, and
by hooks when a window moves, a pane is killed or exits, or a session closes,
never on a redraw.

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

## Tab labels

For an agent pane the tab shows the **conversation topic**, not the window name.
Claude Code writes it to `pane_title`, so with several agents running you see
what each is working on instead of three tabs all reading `claude`:

```text
 Color scheme for code … ●     Openvpn setup with voi… ○     build ○
```

Non-agent panes fall back to the window name, and so does an agent that never
set a title (its `pane_title` is still the host name).

The tab reads `pane_title` **inside tmux**, so it works regardless of the outer
terminal. `examples/full.tmux.conf` ships with `set-titles off`: an agent
retitles constantly, and forwarding each change out as an OSC title sequence
(they do not coalesce like cell redraws — measured 609 titles, 27KB/3s under a
storm) stalls Windows Terminal's draw loop until its tmux client freezes. The
in-tmux tab above is unaffected. To let the outer window title track the topic
on a terminal that coalesces fine (Ghostty, kitty, iTerm2), uncomment the two
`set-titles` lines in that file.

## Path and branch: `tmux-where`

`scripts/tmux-where` — `pane_current_path` is tmux's **local** view, so a pane
sitting in ssh reports this machine's path and git branch. This prints the ssh
destination instead, and blanks the branch. It exits early on
`#{pane_current_command}` so the common case costs nothing.

It also names a linked **git worktree**, since a branch alone is ambiguous when
you run several agents across worktrees of one repo:

```text
 main                    a normal checkout
 feat-api ⑂api-work      the same repo, linked worktree "api-work"
```

Branch and worktree come from reading `.git/HEAD` directly, with **no `git`
process at all** — a repo's HEAD is a one-line file. The branch is not read on a
redraw: hooks run `tmux-where branch --set` on a focus change, a new window or
split, and a shell prompt (OSC 133), and it writes the pane's `@cockpit_branch`
only when the branch changed. git is still asked for one case it alone can
answer: a detached HEAD, whose short-hash width git decides from the size of
the repo. The path is a native format; only an ssh pane runs `tmux-where` on a
redraw, to name its destination.

The one deliberate difference from `git rev-parse` is that this ignores
`safe.directory` — for a status bar, labelling the directory you are sitting in
is the wanted behaviour.
