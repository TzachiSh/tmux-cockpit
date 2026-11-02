# Configuration

## Keys and the options that rebind them

| key | does |
|-----|------|
| tap the branch on the border | branch picker |
| `prefix + b` | the same picker, without the mouse |
| `prefix + a` | the menu: every cockpit and window action, one letter each, no Shift; `≡` on the border opens it too — `@cockpit_menu_key` rebinds, `off` disables |
| `prefix + u` | jump to the pane that needs you (again: next one) — `@cockpit_jump_key` rebinds, `off` disables |
| `prefix + s` | the session picker, replacing stock `choose-tree` — `@cockpit_sessions_key` rebinds, `off` keeps stock |
| `prefix + r` | the board, the same one `prefix + w` opens; `r` on a row reviews that agent's uncommitted diff (replaces stock `refresh-client`) — `@cockpit_review_key` rebinds, `off` restores stock; `@cockpit_review_window on` opens a window instead of the popup |
| `prefix + i` | one screen for the pane you are on: cwd, the git root the review diffs, state, review mark, review base, session id and review dir; it replaces stock window info, `@cockpit_info_key` rebinds, `off` restores stock (being folded into the board, #228) |
| `prefix + f` | the panel: ask a text-only `claude -p` about this pane and paste the answer back into it — see [The panel](board-and-panel.md#the-panel-cockpit-tui) in board-and-panel.md; needs the `cockpit-tui` binary, replaces stock `find-window`, `@cockpit_panel_key` rebinds, `off` restores stock |
| `prefix + w` | the board: every agent pane as a live tile, blocked first, each previewing the pane itself; needs the `cockpit-tui` binary, `@cockpit_board_key` rebinds, `off` keeps stock; `@cockpit_board_window on` opens a window instead of the popup; after `r`, `f` or `i` the board comes back on the same pane with the outcome as a note, and `@cockpit_board_return off` makes them one-way |
| `prefix + R` | reload the tmux config — the file tmux actually loaded, XDG included — `@cockpit_reload_key` rebinds, `off` disables |
| `prefix + M` | move this window to another session, or a new one, picked from a menu; the client follows it, and moving your last window out will not detach you (replaces stock `select-pane -M`) — `@cockpit_move_key` rebinds, `off` restores stock; also `prefix + a M` |

Every key on one page, a line each: [KEYS.md](../KEYS.md).

## Review options

The options the review and its keys read, with their defaults, in one block.
None of them has to be set; as with the rest, set them before the `run-shell`
line.

```sh
set -g @cockpit_review_key     r       # the board; off restores stock refresh-client
set -g @cockpit_review_window  off     # on: the board in a window instead of the popup
set -g @cockpit_info_key       i       # off restores stock window info
set -g @cockpit_jump_key       u
set -g @cockpit_sessions_key   s       # off keeps stock choose-tree
set -g @cockpit_move_key       M       # off restores stock select-pane -M
set -g @cockpit_menu_key       a       # the menu; n if you never use next-window
set -g @cockpit_popup_width    70%
set -g @cockpit_popup_height   70%
```

## Popup size

One pair of options sizes both popups (default `70%` × `70%`). Set them
**before** the `run-shell` line, like `@cockpit_agents`:

```tmux
set -g @cockpit_popup_width  60%
set -g @cockpit_popup_height 50%
```

Anything `display-popup` accepts works, absolute columns/rows included. The
path picker re-reads them on every click, so changing them needs no reload.

The session picker (`prefix + s`) reads them as **ceilings**, not sizes: it
shrinks to fit its sessions, at most 70% of the client on a desktop, 90% wide
below 160 columns, and full width in touch mode.
