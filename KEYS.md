# Keys

Every key tmux-cockpit binds, and the ones `examples/full.tmux.conf` adds,
one line each. `prefix` is `C-Space` in the example config, `C-b` in stock
tmux. All plugin keys are lowercase on purpose: tmux binds the character the
terminal sends, so a Shift-letter chord dies under Caps Lock or a non-Latin
layout.

## The plugin

| key | does |
|-----|------|
| `prefix a` | the menu below. Every row has its letter on screen. |
| `prefix u` | jump to the pane that needs you; again for the next one. |
| `prefix r` | the board: every agent, blocked ones first; `r` on a row reviews its diff. |
| `prefix i` | one screen about this pane: cwd, git root, state, review. |
| `prefix f` | the panel: Claude writes, fixes or explains for this pane and pastes the answer in. `^Z` in it hides it; `prefix f` on the same pane brings it back. Needs `cockpit-tui`. |
| `prefix e` | hide the status line and pane borders for this session; again restores them. |
| `prefix b` | the branch popup (`cockpit-tui branch`) for this pane's repo. |
| `prefix s` | the sessions screen (`cockpit-tui sessions`, what `tm` shows), with each session's agents previewed. Replaces stock `choose-tree`. |
| `prefix w` | the board: every agent pane in a list grouped by session, blocked first; `v` shows them as live tiles, `/` filters the list as you type (`↵` keeps the filter, `esc` clears it). `r` and `f` come back to it (`@cockpit_board_return off` makes them one-way); `i` shows the pane's info over it; `x` kills the agent's pane after a confirm. An agent whose process is gone shows dimmed as `ended`; `o` on it runs `claude --resume` on its session in a new pane. Needs the `cockpit-tui` binary. |
| `prefix M` | move this window to another session, or a new one. The client follows. |
| `prefix R` | reload every config file tmux loaded. |

`M` and `R` are the two Shift keys left: both are rare, and both have a
Shift-free path: the `Move window...` and `Reload config` rows of `prefix a`.

## The menu, `prefix a`

The same menu opens from `≡` at the top left of the border, and from a
right-click on `‹` or `›`. A row is there only when nothing on screen does its
job: Browse folder and Switch branch appear when the border has no room for
the path or the branch, and the two splits are left out on a touch client.

| row | letter |
|-----|--------|
| Board of every agent | `w` |
| Panel for this pane | `f` |
| What is each agent doing? (every pane and its state) | `t` |
| Browse folder | `o` |
| Switch branch | `b` |
| New window | `c` |
| Split right | `\` |
| Split down | `-` |
| Move window to a session... | `M` |
| Rename window | `,` |
| Kill pane… (Close window… on a single pane), with a confirm | `x` |
| Full screen (hide the bar) | `e` |
| Pane info | `i` |
| Rename session | `$` |
| Detach | `d` |
| Reload config | `R` |
| Kill session, with a confirm | `Q` |

The rows for plugin keys press the key they name, so rebinding a key moves its
row and `off` drops it.

## Mouse and touch

Taps on the bar act on release.

| gesture | does |
|---------|------|
| tap `≡` (top left of the border) | the menu |
| tap the session name, or `▲ elsewhere` beside it | sessions screen, waiting sessions first |
| tap the path on the border | browse the pane's working directory |
| tap the branch on the border | the branch popup |
| tap `[z]` / `[u]` (split windows) | zoom / unzoom the pane |
| tap a window tab | that window |
| tap `‹` / `›` (touch) | previous / next window |
| `Ctrl`+double-click a line | path picker for the paths on that line |
| `Ctrl`+right-click a line | the same picker |
| `Alt`+click a line | the same picker, where the terminal forwards Alt |

A markdown file the picker opens shows in the viewer (`@cockpit_md_view off`
sends it to the editor): `/` finds, `n` `N` move between matches, `e` opens the
editor at the top line, `?` lists the rest. See [docs/tui.md](docs/tui.md#the-markdown-viewer-283).

Plain clicks in a pane, wheel and drag are never touched, and neither is a
single `Ctrl`+click: tmux 3.8 binds nothing to it. `@cockpit_keep_swap_pane`
is gone with the swap-pane it kept (#223); `install.sh --check` warns while a
config still sets it.

## The example config, `examples/full.tmux.conf`

| key | does |
|-----|------|
| `prefix \|` or `prefix \` | split right, same directory |
| `prefix -` | split down, same directory |
| `prefix c` | new window, same directory |
| `prefix h j k l` | move between panes (repeat) |
| `prefix H J K L` | resize the pane by 5 (repeat) |
| `prefix v` | toggle stacked / side-by-side |
| `Alt`+arrows | move between panes, no prefix |
| copy mode `v` `y` `C-v` | select, copy, rectangle, like vim |

`H J K L` are the vim resize convention (tmux-pain-control does the same) and
stay Shift; the mouse drag does the same job.

## Changing a key

Set the option before the plugin's `run-shell` line. `off` disables the key
(and restores stock tmux where the plugin replaced one).

```sh
set -g @cockpit_menu_key      a    # n if you never use next-window
set -g @cockpit_jump_key      u
set -g @cockpit_sessions_key  s    # off keeps stock choose-tree
set -g @cockpit_board_key     w    # the board; off keeps stock tmux
set -g @cockpit_review_key    r
set -g @cockpit_info_key      i
set -g @cockpit_panel_key     f    # the panel; off restores stock find-window
set -g @cockpit_fullscreen_key e    # hide the chrome; off keeps stock tmux
set -g @cockpit_move_key      M
set -g @cockpit_reload_key    R
```
