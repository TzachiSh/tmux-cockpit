# Keys

Every key tmux-cockpit binds, and the ones `examples/full.tmux.conf` adds,
one line each. `prefix` is `C-Space` in the example config, `C-b` in stock
tmux. All plugin keys are lowercase on purpose: tmux binds the character the
terminal sends, so a Shift-letter chord dies under Caps Lock or a non-Latin
layout.

## The plugin

| key | does |
|-----|------|
| `prefix a` | the agents menu below. Every row has its letter on screen. |
| `prefix u` | jump to the pane that needs you; again for the next one. |
| `prefix r` | review an agent's uncommitted diff, blocked agents first. |
| `prefix i` | one screen about this pane: cwd, git root, state, review, hold. |
| `prefix f` | a side fix beside a busy agent: a fenced `claude -p` in its own pane. |
| `prefix b` | branch picker for this pane's repo. |
| `prefix g` | session picker (`tm`), with each session's agents previewed. |
| `prefix M` | move this window to another session, or a new one. The client follows. |
| `prefix R` | reload every config file tmux loaded. |

`M` and `R` are the two Shift keys left: both are rare, and both have a
Shift-free path (`prefix a m`, and the `Move window...` / `Reload config` rows
of the example's `prefix m` menu).

## The agents menu, `prefix a`

| row | letter |
|-----|--------|
| Jump to the pane that needs you | `u` |
| Review its diff | `r` |
| Side fix beside it | `f` |
| Pane info | `i` |
| Switch branch | `b` |
| Move window to a session... | `m` |
| Sessions... | `s` |

Each row presses the key it names, so rebinding a key moves its row and
`off` drops it. Needs tmux 3.4.

## Mouse

| gesture | does |
|---------|------|
| `Ctrl`+click the branch segment | branch picker (tmux 3.7) |
| `Ctrl`+click the directory segment | browse the pane's working directory (tmux 3.7) |
| `Ctrl`+double-click a line | path picker for the paths on that line |
| `Ctrl`+right-click a line | the same picker |
| `Alt`+click a line | the same picker, where the terminal forwards Alt |

Plain clicks, wheel and drag are never touched.

## The example config, `examples/full.tmux.conf`

| key | does |
|-----|------|
| `prefix m` | action menu: splits, zoom, layout, kill pane, new / rename / move / kill window, agents, branch, sessions, rename session, detach, reload, kill session |
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
set -g @cockpit_ai_key        a    # n if you never use next-window
set -g @cockpit_jump_key      u    # was J before #43
set -g @cockpit_sessions_key  g    # was S before #43
set -g @cockpit_review_key    r
set -g @cockpit_info_key      i
set -g @cockpit_fixer_key     f
set -g @cockpit_move_key      M
set -g @cockpit_reload_key    R
```
