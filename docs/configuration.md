# Configuration

## Keys and the options that rebind them

| key | does |
|-----|------|
| tap the branch on the border | the branch popup, `cockpit-tui branch` |
| `prefix + b` | the same popup, without the mouse |
| `prefix + a` | the menu: every cockpit and window action, one letter each, no Shift; `≡` on the border opens it too — `@cockpit_menu_key` rebinds, `off` disables |
| `prefix + u` | jump to the pane that needs you (again: next one) — `@cockpit_jump_key` rebinds, `off` disables; like every key option it is global (`set -g`) and read at load, so a change needs a reload |
| `prefix + s` | the sessions screen (`cockpit-tui sessions`, what `tm` shows), replacing stock `choose-tree` — `@cockpit_sessions_key` rebinds, `off` keeps stock |
| `prefix + r` | the board, the same one `prefix + w` opens; `r` on a row reviews that agent's uncommitted diff (replaces stock `refresh-client`) — `@cockpit_review_key` rebinds, `off` restores stock; `@cockpit_review_window on` opens a window instead of the popup |
| `prefix + i` | one screen for the pane you are on: cwd, the git root the review diffs, state, review mark, review base, session id and review dir; it replaces stock window info, `@cockpit_info_key` rebinds, `off` restores stock; the board's `i` draws the same screen |
| `prefix + f` | the panel: ask a text-only `claude -p` about this pane and paste the answer back into it — see [The panel](board-and-panel.md#the-panel-cockpit-tui) in board-and-panel.md; needs the `cockpit-tui` binary, replaces stock `find-window`, `@cockpit_panel_key` rebinds, `off` restores stock |
| `prefix + w` | the board: every agent pane as a live tile, blocked first, each previewing the pane itself; needs the `cockpit-tui` binary, `@cockpit_board_key` rebinds, `off` keeps stock; `@cockpit_board_window on` opens a window instead of the popup; after `r`, `f` or `i` the board comes back on the same pane with the outcome as a note, and `@cockpit_board_return off` makes them one-way |
| `prefix + R` | reload the tmux config — the file tmux actually loaded, XDG included — `@cockpit_reload_key` rebinds, `off` disables |
| `prefix + M` | move this window to another session, or a new one, picked from a menu; the client follows it, and moving your last window out will not detach you (replaces stock `select-pane -M`) — `@cockpit_move_key` rebinds, `off` restores stock; also `prefix + a M` |

A `;` key is written `';'` or `'\;'` in tmux.conf; a bare `;` ends the `set`
command there, and tmux refuses it.

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

The sessions screen (`prefix + s`) reads them as **ceilings**, not sizes: it
shrinks to fit its sessions, at most 70% of the client on a desktop, 90% wide
below 160 columns, and full width in touch mode.

## Accounts

Claude Code keeps each login in its own config dir, the `CLAUDE_CONFIG_DIR` a `claude` runs
with ([authentication](https://code.claude.com/docs/en/authentication)). List yours, each
with a short name, and the board names the account on each agent's tile, before its
allowance:

```tmux
set -g @cockpit_accounts "p1=~/.claude p2=~/.claude-2 p3=~/.claude-3 work=~/.claude-work"
```

- A name is letters, digits, `-` and `_`. A dir is absolute, `~` or under `~/`, holds no space,
  and is on disk. One bad entry refuses the whole list, and the board's log (`COCKPIT_LOG`) says
  why.
- The board reads the option when it starts, so a change shows on the next board.
- The name is that of the dir the pane's hooks saw, `@cockpit_cfg`. A pane whose hooks saw none
  runs on `~/.claude`. With `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` on, the hooks see no dir at all,
  so the tile shows `?` and never guesses the default
  ([Agent view and background jobs](states-and-hooks.md#agent-view-and-background-jobs)). So
  it does for a dir with a control character in it, which the hooks cannot record.
- The board reads a transcript under the `projects` dir of any listed account, after its
  own, so a pane on another account keeps its doing line, model, context and subagents.

Log in once per dir, with `CLAUDE_CONFIG_DIR=~/.claude-2 claude` and so on, and start each pane's
`claude` with its account's `CLAUDE_CONFIG_DIR`. Keep one login per dir: two dirs on one login
drift apart when the server rotates the refresh token. Between the personal dirs, link
`projects/`, `history.jsonl` and `settings.json` to the first one's, so they share sessions, prompt
history and settings. Leave the work dir unlinked: it keeps its own history.

## Agent names

An entry in `@cockpit_agents` matches the pane's command in any case: `Cursor` marks a
`cursor` pane as an agent, and `claude` marks a `CLAUDE` one.

The name compared is the last part of the command's path, as tmux's `#{b:}` gives it:
`./claude`, `bin/claude` and `/opt/x/claude` are all `claude`, as on the board (#840).
So an entry that holds a `/`, such as `x/claude`, never matches.

Entries are command names, not patterns. When `@cockpit_agents_glob` is unset, an
entry with any character other than letters, digits, `_` and `-` (a `*`, `[0-9]` or
`.`) makes the plugin show a message and use the default list instead of yours. In
an `@cockpit_agents_glob` you set yourself, such characters are plain text, not
wildcards. Some places count a version-shaped command such as `2.1.318`, which
some installs of Claude Code show, as an agent on their own, among them the board
(and its resume key), the info screen, the panel, the background-job poller and the
work dir. Others, such as the status bar, jump, review delivery and the sessions
screen, follow only the names.

A process name that holds glob characters (`*`, `?`, `[...]`) is a pattern to the
shell places and plain text to the board's own list, so such names are unsupported.
A process titled `*` or `c*` counts as an agent at every shell place, among them the
status bar, jump, review delivery, the background-job poller, the work dir and the
board's resume key, but not in the board's own list (#880).
