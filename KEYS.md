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
| Name this pane (its `@cockpit_role`, found by `cockpit-tui pane <name>`) | `n` |
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
| tap the path on the border | browse the pane's working directory (`↵` views a file, `alt-e` edits it, `alt-a` adds it to the pane's prompt basket) |
| tap the branch on the border | the branch popup |
| tap `[z]` / `[u]` (split windows) | zoom / unzoom the pane |
| tap a window tab | that window |
| tap `‹` / `›` (touch) | previous / next window |
| `Ctrl`+double-click a line | path picker for the paths on that line |
| `Ctrl`+right-click a line | the same picker |
| `Alt`+click a line | the same picker, where the terminal forwards Alt |

A markdown file the picker opens shows in the viewer, and so does any file the
file browser's `↵` picks (`@cockpit_md_view off` sends both to the editor).
`cockpit-tui view` shows a markdown file rendered, and any other text file whole
as numbered code highlighted by its extension or `#!` line; the keys are the
same for both: `/` finds, `n` `N` move between matches, `e` opens the editor at
the selection's end, or at the top line when there is none. `v` selects lines (a
tap drops an anchor, a second tap or a drag ends it); the move keys and `n` `N`
move its end. `?` asks about the selection, or the top line: the launcher opens
the panel on the pane with the question box, and the viewer comes back after;
the answer's `c` makes it a draft for the pane's agent. `a` adds the selection,
or the whole file, to the pane's prompt basket, and `y` copies its reference
(`src/x.rs:40-72`). The panel's Compose shows the basket; `^T` sends the draft
with its refs after a confirm, and `^B` lists them. `esc` clears the selection,
then the matches, and `h` or `F1` lists the rest. See
[docs/tui.md](docs/tui.md#the-markdown-viewer-283) and [a code
file](docs/tui.md#a-code-file-365).

Wheel and drag are never touched, and neither is a single `Ctrl`+click: tmux
3.8 binds nothing to it. A plain click's release on a pane or a border selects
that pane and is handed on to it: when a popup ate an earlier release, tmux
turns the next press into a drag end and the click would select nothing (#21,
#53). A root `MouseUp1Pane` or `MouseUp1Border` of your own is replaced.

Only a plain left click is caught that way. The drag end and the release take
the next press's own button and modifiers (tmux 3.8 server-client.c, lines 926
to 928 and 1234 to 1265), so these presses after an eaten release are still
lost once. Each was measured on a private tmux 3.8 server (#1320):

- A middle click selects nothing and pastes nothing.
- A right click on a pane opens no menu. On an app with mouse mode on it
  reaches the app, but the pane is not selected.
- An Alt click or a `Ctrl`+right click picks no path and selects nothing.
- A press you then drag still drags and copies, but the pane is not selected.
- A click on a pane in copy mode selects the pane but ends copy mode.

No release binding fixes the first three. The release key comes after every
plain click too, and no tmux 3.8 format says whether the press was lost
(format.c, lines 3768 to 3807). Bound to its press's command, `MouseUp2Pane`
pasted twice on each middle click, and `M-MouseUp1Pane` and `C-MouseUp3Pane`
picked twice. `MouseUp3Pane` did work, since a plain right click's menu takes
its own release (server-client.c, line 1824). It stays unbound anyway. It needs
a copy of tmux's pane menu kept in step with tmux, and it would open that menu
for you if you unbound `MouseDown3Pane`.

A press you then drag gets no release key. Its release is a drag end, as any
drag's is (server-client.c, lines 1158 to 1180), and the plugin never touches
a drag. In copy mode the press runs copy mode's own `MouseDragEnd1Pane`,
`copy-pipe-and-cancel`, which always leaves copy mode (window-copy.c, lines
2697 to 2704). Only rebinding that key in the copy-mode tables kept copy mode,
and the same key ends a real drag-to-select, so those tables stay stock. A
`Ctrl` double click still picks, because its first press still counts toward
the double click.

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
A reload (`prefix R`) that turns a key off or moves it gives the old key back
to what tmux binds there with no config; a key you bound yourself since stays
yours. Each cockpit key carries a `cockpit: <name>` note in `prefix ?`.
The keys are global and read when the plugin loads: set them with `set -g`, and
reload after a change; a session's own value is not read.

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
