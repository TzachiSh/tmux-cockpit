# The board (cockpit-tui)

`prefix w` opens a list of every agent pane, grouped by tmux session, the
sessions that need you first. Each row carries the pane's state glyph, what the
agent is doing and how long it has been doing it; a group folds to one header
that still shows every agent waiting on you. `v` swaps to the tiles, one live
`capture-pane` per agent. `↵` jumps to the selected agent and closes the board.

It is a small Rust binary, `bin/cockpit-tui`. It is **required**: the board and
the panel run on it, and the follow-up #199 PRs move the shell scripts'
transcript, diff-count and git-safety readers onto its `transcript`, `diffstat`
and `git-flags`. Install fetches it with your own `gh` login or builds it from
source (Rust ≥ 1.88), and stops with a message when it can do neither.

## What it is not

- **Not a fuzzy filter.** The fzf fleet queue is gone (#253): `prefix r` opens
  the board, a grouped list of agents with their diff stat and branch.
- **Not an actor.** The binary renders and decides. It writes one line,
  `<verb> <pane-id>`, to the file named by `$COCKPIT_ACTION`, and the binding
  runs the existing `tmux-jump` / `tmux-review` script. Every mutating path
  stays shell.
- **Not a replacement for `prefix i` or `prefix f`.** The board reaches them; it
  does not reimplement them.

## Install

The binary ships as a release asset. `install.sh` fetches it. With
`@cockpit_tui_fetch on`, every config load also runs `scripts/tui-fetch` in the
background: tmux starting, `prefix R`, and tpm's update key `prefix U`, which
reloads the config after updating. Otherwise run `scripts/tui-fetch` yourself,
or build it with `make tui`.
Either way: uname picks the asset, the `.sha256` is fetched first and verified,
and only a verified file is installed. A checksum mismatch, an unfetchable manifest or a
platform with no asset all leave whatever binary is already there and say so
once. The repository is private, so the fetch rides your own `gh` login when
`gh auth token` finds one (the plugin never carries a token); without one the
release URL 404s and the fetch skips. `COCKPIT_TUI_REPO` overrides the
`owner/repo` gh downloads from.

By default (unless `@cockpit_tui_fetch on`) nothing is fetched when tmux loads its config. The load path stays offline;
installing and updating stay gestures you make, the way tpm's own are.

To build from source instead, from the plugin directory:

```sh
make tui
```

That needs **Rust >= 1.88** (ratatui 0.30's floor; Debian 13 ships 1.85 and
Ubuntu 24.04 ships 1.75). On an older toolchain the fetch path refuses by name
rather than dumping a compiler error, and keeps any binary you already have.

`make check` builds the binary first; the suite needs cargo.

The binary lives in the plugin's own `bin/` and is gitignored. `install.sh`
runs `scripts/tui-fetch --required` and exits 1 when no working binary is
left; `install.sh --check` fails when it is missing or older than `TUI_MIN`
(`scripts/lib/tui.sh`). `COCKPIT_TUI_BIN` points every script at another
binary. Loading the plugin runs one offline `--version` and says when the
binary is missing or too old.

`@cockpit_board_window on` parks the board in a window instead of the popup.

## Keys

Inside the board, the list (the default layout):

| key | does |
|-----|------|
| `j` `k` `↑` `↓` | next / previous row, headers included |
| `h` `←` | fold the selected group |
| `l` `→` | unfold the selected group; on an open header, enter it |
| `space` | fold / unfold the selected group |
| `-` `=` | fold all / unfold all |
| `⇥` `⇧⇥` | next / previous agent, skipping headers, wrapping |
| `n` `p` | next / previous session header, wrapping |
| `1`-`9` | the Nth agent row on screen. Selects, does not jump; a row in a folded group unfolds it |
| `g` `G` | first / last row |
| `u` | the next `▲` agent, wrapping; unfolds its group |
| `[` `]` `PgUp` `PgDn` | scroll a screenful; the selection only moves to stay on screen |
| `↵` | jump to the selected pane, close the board; on a header, fold / unfold |
| `a` `r` `f` `i` `x` | as in the tiles, on an agent; a header does not take them |
| `/` | find: open the field and filter as you type (see [Find](#find)) |
| `v` | the tiles |
| `?` `^L` `q` | as in the tiles |
| `esc` | the first that applies: dismiss a `✕` note, close the info overlay, close the find field (clearing it), clear a kept filter, close the board |
| `^C` | close the board from anywhere, the find field included |

In the find field every printable key is text, `q`, `?` and the act and
`EXTRAS` keys included:

| key | does |
|-----|------|
| letters, digits, space | type; the list narrows as you type |
| `↑` `↓` `^N` `^P` | next / previous row |
| `⇥` `⇧⇥` | next / previous agent |
| `⌫` | delete the last character |
| `^U` | clear the text; the field stays |
| `↵` | keep the filter and close the field |
| `esc` | clear the filter and close the field |
| `F1` | the keys |
| `^L` | refetch and redraw now |
| `^C` | close the board |

In the tiles (`v`):

| key | does |
|-----|------|
| `h` `l` `←` `→` | focus one tile left / right |
| `j` `k` `↑` `↓` | focus one tile up / down |
| `⇥` `⇧⇥` | next / previous tile in reading order |
| `1`-`9` | select tile N on this page. Selects, does not jump |
| `g` `G` | first / last tile |
| `u` | the next needs-input tile, mirroring `prefix u` |
| `[` `]` `PgUp` `PgDn` | scroll a screenful, only when rows are hidden |
| `↵` | jump to the selected pane, close the board |
| `r` | review its diff (`tmux-review`); the board comes back after |
| `f` | the panel for it (`cockpit-panel`); the board comes back after |
| `i` | info for the selected pane; any key comes back to the board |
| `a` | answer a `▲` agent, ask any other; the footer says which. Phase 1 jumps on a `▲` and notes it on the rest; phase 2 opens the reply overlay |
| `x` | kill the pane's window, after a confirm (`kill %N`, for #228's wrapper arm) |
| `?` | the keys: full-popup, one column, generated from the key table itself, with a legend of the tile glyphs. `↑↓` `j` `k` `PgUp` `PgDn` scroll it; any other key closes it |
| `v` | the list |
| `^L` | refetch and redraw now |
| `q` `esc` `^C` | close the board. With `?` open, any key only closes the help; with a `✕` note up, `esc` only dismisses it |

`h`/`l` fold in the list and move a column in the tiles: tree-view convention
in one, grid convention in the other. `?` lists the keys of the layout on
screen. The help and the footer are generated from the same table the dispatch
reads, so a renamed key cannot go stale in one and not the other.

**One key bar.** The footer row is the only place the board lists keys; no
frame carries a hint. It lists only keys that work on what is selected, in
priority order, and drops one whole hint at a time from the right as the width
shrinks; `?` and `q` always stay. On an agent: `↵ jump`, `a answer` or `a ask`,
`r review`, `f panel`, `i info`, then `space fold` and `/ find` in the list,
then `x kill`, which the cut drops first. On a list
header: `space fold` or `space unfold`, `l open`, `- fold all`, `= open all`,
`n p sessions`, `/ find`. With a kept filter, `esc clear filter` comes before
`? keys`. In the find field the bar is the field's:
`type to filter   ↑↓ move   ⌫ edit   ^U clear   ↵ keep   esc clear`, `↑↓ move`
left out when nothing matches; as it narrows, `type to filter` goes first, then
`⌫ edit`, `^U clear` and `↑↓ move`, never `↵ keep` or `esc clear`. The footer, the `?` help and the floor card are the panel's
own, from `chrome`.

**Extra actions.** `EXTRAS` in `board.rs`, the `// #228:` registry, holds the
actions beyond the key table: a key, the footer's label, the `?` help row, the
verb, and two optional functions of the selected `Pane`, `confirm` (the
question) and `expect` (the value for `.expect`). Each entry is in the footer on
an agent (never a header), in `?` under "act on the selected agent", and in the
dispatch; its key may not be one the table, D6, D7's `/` or D8's `a` takes.
`x` → `kill`, with a confirm and the window id as `.expect`, is #228's and the
only entry (#288 removed `R` → `request`). An entry with no `confirm` exits at once.

**The confirm.** An entry with a `confirm` opens one frame, the focus frame,
over the foot of the list or the tiles: the action and the target in its title,
`confirm` at its right, the question (`kill window api:2 · server?`) and what
the keys do, all in fg. It owns the keyboard (D18): `y` exits with the action,
`n` and `esc` go back with nothing written, `?` shows its keys, `q` and `^C`
close the board; every other key does nothing, `↵` included, so a held Enter
cannot send (D8). The footer lists those keys only (D19):
`y kill   n back   esc back   ? keys   q close`. While it is open a refresh
updates each row's age and text but no row moves, folds or goes; the list's
title reads `sessions · held`. If the target's pane goes, the confirm closes
itself with the note `✕ nothing done: api:2 is gone, kill not sent`. Step 6's
`deliver` is meant to open the same confirm.

**No mouse on the board.** Enabling SGR capture inside a popup takes the wheel
and text selection away from you for as long as the board is open, and the board
is a screen you hold for seconds. The panel captures the mouse only in tablet
mode: see [The panel in tablet mode](#the-panel-in-tablet-mode).

## Options

Set these before the plugin's `run-shell` line, like every other cockpit option.

| option | default | does |
|--------|---------|------|
| `@cockpit_board_key` | `w` | rebinds the board. `off` leaves stock tmux alone |
| `@cockpit_board_window` | unset | `on` parks the board in a window instead of a popup, mirroring `@cockpit_review_window` |
| `@cockpit_board_return` | `on` | `off` makes `r` and `f` one-way: the board closes and stays closed, as before the return trip. Read by `scripts/cockpit-board` after each action |
| `@cockpit_tui_fetch` | `off` | `on` runs `scripts/tui-fetch` in the background on every config load (start, `prefix R`, tpm's `prefix U`). Off, nothing is ever fetched from the config load path, and `scripts/tui-fetch` by hand is the way in |
| `@cockpit_tui_version` | unset | pins the release tag `scripts/tui-fetch` fetches: `cockpit-tui-v<ver>`, e.g. `cockpit-tui-v0.1.0`. The pre-#79 `tui-v` form is accepted for comparison only — the fetch URL uses the tag as written, so a stale `tui-v` pin compares as current against a release that does not exist. |
| `@cockpit_tui_pid` | (state) | read-only: the pid of the running board. Written at start, unset on `q`, SIGHUP and SIGTERM |
| `@cockpit_panel_key` | `f` | binds the panel (`cockpit-tui panel %N` through `scripts/cockpit-panel`) to a prefix key. `off`, or an unset key with the old `@cockpit_fixer_key off`, leaves stock `find-window` on `f` |
| `@cockpit_panel_mode` | `auto` | `pc`, `tablet` or `auto`: how the panel is shown. `auto` is tablet under Termux (`TERMUX_VERSION` set, or `PREFIX` under `com.termux`), pc elsewhere. Read at load and handed to the popup as `COCKPIT_PANEL_MODE`; any other word shows a message and binds `auto` |
| `@cockpit_panel_bound` | (state) | the key the last load bound the panel to, so a reload that turns it off or moves it unbinds the old one |

The popup is `-w 90%`, wider than the 70% the pickers use, because a 70% popup
on a 120-column terminal is an 84-column board, which is one tile column. The
height is whatever `@cockpit_popup_height` gives, like every other cockpit
popup.

`@cockpit_tui_pid` is how the state hooks wake the board: `agent-state` sends
`SIGUSR1` after a transition, and the board pulls its next tick forward. The
sender checks `/proc/$pid/comm` first, because a published pid outlives a
`SIGKILL` and `SIGUSR1`'s default action on a recycled pid is *terminate*.

Read-only, and owned by bash: `@cockpit_session`, `@cockpit_dir`, `@panestate`,
`@panestate_msg`, `@cockpit_review`, `@cockpit_review_spool`, `@cockpit_title`,
`@thm_*`.

## Environment

| variable | does |
|----------|------|
| `COCKPIT_ACTION` | file the one action line is written to. Unset sends it to stdout |
| `COCKPIT_TMUX` | overrides the tmux argv. Unset follows `$TMUX` to whatever socket the pane is on |
| `COCKPIT_LOG` | append target. The binding sets it to `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/tui.log` |
| `COCKPIT_READONLY` | any non-empty value: never write to a tmux server, the pid included. The panel wrapper then pastes and sends nothing and logs what it would have done |
| `COCKPIT_GLYPHS` | `ascii` swaps the glyphs *and* the borders for the ambiguous-width fallback tier; the panel then prints nothing but ASCII, its key names included (`enter`, `tab`, `bksp`, `left`, `up/dn`). The wrappers and the `prefix i` popup set it from `@cockpit_glyphs` (#293) |
| `COCKPIT_CLAUDE` | the panel's `claude` binary. Unset is `claude` on `PATH` |
| `COCKPIT_PANEL_TIMEOUT` | seconds before the panel kills its `claude -p` call. Default 120 |
| `COCKPIT_PANEL_MODEL` | forces the panel's model, over the rules and the model picked on screen |
| `COCKPIT_PANEL_MODE` | `auto`, `pc` or `tablet`, from `@cockpit_panel_mode`. Unset or empty is `auto`. Any other value: the panel exits 2 before it draws. The board's popup passes it too, so a panel opened from the board gets the same mode |
| `COCKPIT_PANEL_NOTE` | set by `scripts/cockpit-board` for `scripts/cockpit-panel`: the file the panel wrapper writes its last line to instead of a `display-message`, a failure starting with `✕`. Unset (`prefix f`), the message goes to the status line as before |
| `COCKPIT_PANEL_DROPPED` | set by `scripts/cockpit-panel` for the binary: the pane ids, space-separated, whose hidden panels it just dropped because the pane is gone. The binary shows them in an info note; anything but `%N` words is ignored |
| `COCKPIT_PANEL_CALL` | set to `1` in the panel's `claude -p` child, never read by the binary: a hook can skip the call by it |

The binding runs the binary under `env -u CLAUDECODE`, which is required inside
a popup.

`COCKPIT_READONLY=1` exists because a probe run once left a dead
`@cockpit_tui_pid` on the owner's live server. Use it for screenshots and
benchmarks.

**The panel's prompts.** A file at
`${XDG_CONFIG_HOME:-$HOME/.config}/tmux-cockpit/prompts/<name>.txt`, with
`<name>` one of `write`, `write.examples`, `explain` or `fix`, replaces the
shipped prompt of that name; the `{{platform}}`, `{{home}}`, `{{about_user}}`
and `{{examples}}` placeholders are filled in it the same way. A missing file
means the shipped prompt. A file that exists but cannot be read (permissions,
invalid UTF-8) also falls back to the shipped prompt, and the reason goes to
`tui.log`.

**The panel's cache.** `claude -p` reads its system prompt from a file, so each
distinct prompt is written once to
`${XDG_CACHE_HOME:-$HOME/.cache}/tmux-cockpit/panel/prompts/<hash>.txt`, mode
0600 in a 0700 directory. `tmux-cockpit/panel` is also the call's working
directory. Nothing prunes it; a new file appears only when a prompt's text
changes.

## The screens

### The board

The list, at 100 columns: 13 agents in 4 sessions, the first `▲` selected.

```
 board · 13 agents · 4 sessions · 2 needs you · 1 error
┌ sessions ────────────────────────────────────────────────────────────────────────────────────────┐
│  ▾ api ─────────────────────────────────────────────────────────────────────────────── ▲1 ●2 ✓1  │
│▎ 1 ▲◆ 0 server               Do you want to run npm test?                                  4m12s │
│  2 ●  1 tests                Bash: cargo test --workspace                                  1m03s │
│  3 ●  3 migrate              Edit src/db/migrate.rs                                          22s │
│  4 ✓◆ 4 auth                 Pushed PR #231, waiting on review                               12m │
│  ▾ infra ───────────────────────────────────────────────────────────────────────────── ✕1 ●1 ○1  │
│  5 ✕  0 terraform            terraform plan: state lock held                               6m02s │
│  6 ●  1 dns                  Bash: dig +short api.internal                                    9s │
│  7 ○  2 k8s                  idle                                                                │
│  ▾ web ──────────────────────────────────────────────────────────────────────────── ▲1 ●1 ✓1 ○1  │
│  8 ▲  2 docs                 plan ready: approve?                                            38s │
│  9 ●  0 ui                   Bash: pnpm test Board                                         3m40s◂│
│    ✓◆ 1 checkout             Opened PR #88: card form a11y                                   25m │
│    ○  5 a11y                 idle                                                                │
│  ▾ ml ─────────────────────────────────────────────────────────────────────────────────── ●1 ✓1  │
│    ●  0 train                Bash: python train.py --epochs 40                               41m │
│    ✓  1 eval                 Wrote eval/test_report.md                                     2h05m │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
 ↵ jump   a answer   r review   f panel   i info   space fold   ? keys   q close
```

The top line is the agent count, the session count, how many need you and
how many are in error, and `· stale` while reads fail. When it does not fit,
whole parts drop, never a cut word: the session count first, then the agent
count, then the error and needs counts.

A row is the selection mark, its digit, the state glyph and review mark, the
window index, the title, what the agent asks (`▲`, `✕`) or is doing, and its
age; an idle agent says `idle`. `◂` marks tmux's active pane. The selection is
`▎` plus a band in the `sel` role (`@thm_sel`, `@thm_sel_light` on a light
theme), its text in the value colour: dim on `sel` is only 3.4:1. Under
`NO_COLOR` the band is reverse video, so the selection still shows.

**Order.** One group per tmux session. Sessions holding a `▲` or `✕` come
first, then the rest, each band by session name; inside a group,
`(state rank, pane id)`, as in the tiles. No clock: a group moves only when it
starts or stops needing you, never when an agent goes from running to done.

**Folding.** `h`, `space`, `↵` on a header, or `-` fold a group to its header,
which counts what it hides and shows every `▲` and `✕` row it holds, so
folding never hides an agent waiting on you:

```
│  ▾ api ─────────────────────────────────────────────────────────────────────────────── ▲1 ●2 ✓1  │
│  1 ▲◆ 0 server               Do you want to run npm test?                                  4m12s │
│  …                                                                                               │
│▎ ▸ infra ────────────────────────────────────────────────────────────────── 2 folded · ✕1 ●1 ○1  │
│  5 ✕  0 terraform            terraform plan: state lock held                               6m02s │
│  ▸ web ───────────────────────────────────────────────────────────────── 3 folded · ▲1 ●1 ✓1 ○1  │
│  6 ▲  2 docs                 plan ready: approve?                                            38s │
│  ▸ ml ──────────────────────────────────────────────────────────────────────── 2 folded · ●1 ✓1  │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
 space unfold   l open   - fold all   = open all   n p sessions   ? keys   q close
```

Nothing unfolds by itself: a new session appears unfolded, and a folded
session that gains a `▲` stays folded and shows the row. A digit or `u` that
lands in a folded group unfolds it. Folds are kept in `board.state`.

**Selection.** The selection is a pane id, or a session name on a header,
never an index. A refresh finds it by id and keeps it on the screen row it had,
so the viewport moves and the cursor does not. A pane that is gone gives way to
the row now at its place in the same session, else that session's header, else
the first row. Folding the selected agent's group moves the selection to the
header, unless the agent is `▲` or `✕` and stays on screen.

**Digits and scrolling.** `1`-`9` are the first nine agent rows on screen,
renumbered as the list scrolls; a header takes no digit. `[` `]` scroll a
screenful and the selection only moves to stay on screen. When the list does
not fit, its frame's title says `N-M of T`, counted in rows.

**Find.** `/` opens a field at the top of the list, above a rule, and the list
narrows as you type. Find reads each agent's session, window index, window
name, title, state word (`needs`, `error`, `running`, `done`, `idle`) and its
message or doing line. Space-separated terms must all match, each anywhere in
those fields, ignoring case; it is a substring narrowing, with no fuzzy
ranking. A session with no match hides; one with a match shows unfolded while
the field is open, and its fold is kept for after. The matches are renumbered
`1`-`9` as any rows are, and the frame's title says `4 of 13`, the agents that
match out of all of them:

```
┌ sessions ─────────────────────────────────────────────────────────────────────────────── 4 of 13 ┐
│ / test▏                                                                                          │
├──────────────────────────────────────────────────────────────────────────────────────────────────┤
│  ▾ api ─────────────────────────────────────────────────────────────────────────────── ▲1 ●2 ✓1  │
│  1 ▲◆ 0 server         Do you want to run npm test?                +42 -7 ⑂feat/rate-limit 4m12s │
│▎ 2 ●  1 tests          Bash: cargo test --workspace                +42 -7 ⑂feat/rate-limit 1m03s │
│  ▾ web ──────────────────────────────────────────────────────────────────────────── ▲1 ●1 ✓1 ○1  │
│  3 ●  0 ui             Bash: pnpm test Board                         +120 -33 ?2 new ⑂main 3m40s◂│
│  ▾ ml ─────────────────────────────────────────────────────────────────────────────────── ●1 ✓1  │
│  4 ✓  1 eval             Wrote eval/test_report.md                          +15 -4 ⑂exp-40 2h05m │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
 type to filter   ↑↓ move   ⌫ edit   ^U clear   ↵ keep   esc clear
```

The selection stays on its agent while it matches; when the filter drops it,
it moves to the first match. `esc` clears the filter and closes the field, and
the selection stays on the agent find took it to. `↵` keeps the filter: the
field closes, the list stays narrowed and the frame's title names it,
`/ test · 4 of 13`. Kept, the folds apply again, except the group of the agent
you picked, which unfolds (as a digit's pick does). `/` edits a kept filter,
and `esc` on the list clears it before it would close the board. The kept
filter is written to `board.state`, so the next board opens narrowed.

With no match the field stays, and the list says so, what find reads, and the
way out; the title reads `0 of 13`, and `↵` still keeps the filter as typed:

```
│ / terrafrom▏                                                                                     │
├──────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                  │
│ no agent matches "terrafrom"                                                                     │
│                                                                                                  │
│ find reads the session, name, title, state word and last line.                                   │
│                                                                                                  │
│ ⌫ edits   ^U clears   esc clears                                                                 │
```

The tiles (`v`) take no filter: `/` is a list key, and a kept filter waits in
the list for `v` to come back.

The list has no preview: the right-hand preview pane is a later step, so the
list takes the board's whole width, and it captures no pane.

### The tiles (`v`)

120 columns, 5 agents, 2 columns, full density. The width is the board's own
area, not the terminal's.

```
 board · 5 agents · 1 needs you
┌ 1 ▲  henosis:1 · Openvpn setup ──────────────────────────┐┏▎2 ●◆ henosis:3 · Implement with test review and rig ━━ ◂ ┓
│permission: Bash(sudo openvpn --config               4m12s│┃Bash: shellcheck on cockpit.tmux and tests/run.sh      32s┃
│/etc/openvpn/work.conf --daemon --log /var/log/ovpn.log)  │┃While the suite and rig run: shellcheck on the two script…┃
│Bash command                                              │┃$ make check                                              ┃
│sudo openvpn --config /etc/openvpn/work.conf --daemon     │┃1001 assertions, 0 failures                               ┃
│Do you want to proceed?                                   │┃$ shellcheck cockpit.tmux tests/run.sh                    ┃
│❯ 1. Yes   2. Yes, don't ask again   3. No                │┃                                                          ┃
│                                                          │┃                                                          ┃
└──────────────────────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ 3 ●  work:2 · Fleet gate review ─────────────────────────┐┌ 4 ✓↵ work:4 · Left click issue ──────────────────────────┐
│Agent: whole-branch review of phase 1                4m12s││idle · handover pending                                12m│
│Dispatched the review agent; waiting on its report before…││Pushed PR #28 and left the branch on master.              │
│⏺ Agent (fleet-gate-review)                               ││  ✓ pushed to origin/fix/left-click                       │
│  Read tests/fleet.sh, 214 lines                          ││  PR #28 opened                                           │
│  Read scripts/agent-state, 412 lines                     ││❯                                                         │
│                                                          ││                                                          │
│                                                          ││                                                          │
│                                                          ││                                                          │
└──────────────────────────────────────────────────────────┘└──────────────────────────────────────────────────────────┘
┌ 5 ○  work:5 · (no title) ────────────────────────────────┐
│—                                                         │
│                                                          │
│❯                                                         │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
└──────────────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f panel   i info   ? keys   q close
```

Tile 1 is blocked: red border and the question in place of the doing line;
the footer says `a answer` while it is selected. Tile 2 is the selection: the
thick border, `▎` and its number in reverse video, so it still shows under
`NO_COLOR`. It is also tmux's active pane, the `◂` (`@` in the ascii tier) at
its top-right end. The `◆` after its state glyph is a pending review. The right
rail is exactly 5 columns of elapsed duration; a 6-column duration would be
clipped by the rail and lose its leading digit.

Tiles sort by `(state rank, pane id)`: needs-input, error, running, done, idle.
Nothing in the sort key is a clock, so tiles do not reorder while you read them.
Page 1 is the triage page by construction.

The grid is `clamp(width / 48, 1, 3)` columns, then `min(cols, n)`. Height picks
a density: 2 header rows at `h >= 7`, 1 at `h >= 5` (the `said` line drops),
and below that the grid scrolls by whole tile rows. A tile is never half cut and
never resizes as you scroll; the header's `1-4 of 5   ▼1` is the indicator, not
a scrollbar.

### The return trip

`scripts/cockpit-board` is a loop. After `review` and `panel` it runs
the board again in the same popup, as
`cockpit-tui board --select %N --note <file>`: the selection is back on the pane
just acted on, and the footer row shows the note. `jump`, `q`, `deliver` (still
reserved), a verb the plugin does not know (`request` from a binary older than
#288), a board that exits non-zero, and
`@cockpit_board_return off` end the loop.

The note is the action's last line. For `panel` it is what `cockpit-panel`
would have put in the status line (`cockpit: sent to %3`); `review` writes
none. `i` never leaves the board: it draws the info card in-process. A line that starts with
`✕` reports a failure or a refusal and stays until the next key, which also
does its job, except `esc`, which only dismisses it; the note ends with
`· esc dismiss`. Any other note fades after 5 s.

```
 ✕ cockpit: the pane now runs bash, not claude; nothing sent to %21 · esc dismiss
```

The note file is read like the panel's state file: no symlink, a regular file
of at most 4 KiB, one `\n`-terminated line, control characters dropped. A note
that fails those rules is logged and not shown.

The selection is also kept in
`${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/board.state`, so the next
plain `prefix w` opens on the same pane while it exists. Without one, or when
the pane is gone, the board opens on the first `▲`, else the client's pane,
else the first tile. The file is written on every exit, created new beside it
with `O_NOFOLLOW`, mode 0600, and renamed over it; `COCKPIT_READONLY` skips the
write. It is read back strictly: no symlink, a regular file of at most 64 KiB,
the head `cockpit-board-state 1`, then known `key value` lines: `sel %N` at
most once, then one `fold <session>` line per folded session, each name once,
with no control character, then at most one `filter <text>` line, the kept
find filter: at most 200 bytes, more than spaces, no control character. The
`filter` line is a line kind appended to version 1, so a file without one
still reads. Anything else is a fresh board and a 5 s note
`board.state ignored (<why>); starting fresh`. A header selection is saved as
its group's first agent, and a fold whose tmux session is gone is dropped on the
next write; a session that is still there keeps its fold even while it has no
agent pane. The layout is not kept: every board opens on the list.

```
cockpit-board-state 1
sel %7
fold infra
fold web
filter test
```

The design's first frame drawn from `board.state` before the first fetch is
not built: it would need the fleet's rows in the file, and the file keeps the
selection, the folds and the filter only. The reopened board shows the loading card for its first fetch.

### The help

`?` covers the whole board, one column, so nothing is cut at 60 columns, and
lists the keys of the layout on screen. Rows that do not fit scroll: the top
border says `1-28 of 41` and the bottom one `↑↓ scroll  any other key closes`.
The panel's `?` is the same code. The list's:

```
╭ keys ────────────────────────────────────────────────────────╮
│ move                                                         │
│ j k  ↑ ↓  row                                                │
│ ⇥ ⇧⇥      next / previous agent                              │
│ 1-9       agent by its number on screen                      │
│ g G       first / last                                       │
│ u         next blocked                                       │
│ [ ]       scroll a screenful                                 │
│ n p       next / previous session                            │
│                                                              │
│ groups                                                       │
│ space     fold / unfold the group                            │
│ h ←       fold the group                                     │
│ l →       unfold, or enter an open group                     │
│ -         fold all                                           │
│ =         unfold all                                         │
│                                                              │
│ act on the selected agent                                    │
│ ↵         jump to this pane; on a header, fold / unfold      │
│ a         answer / ask this agent                            │
│ r         review its diff                                    │
│ f         panel for this pane                                │
│ i         info for this pane                                 │
│                                                              │
│ view                                                         │
│ ?         this help                                          │
│ v         tiles / list                                       │
│ ^L        refresh now                                        │
│ q esc     close the board                                    │
│                                                              │
│ on a row                                                     │
│ ▲         needs you                                          │
│ ✕         error                                              │
│ ●         running                                            │
│ ✓         done                                               │
│ ○         idle                                               │
│ ◆         review pending                                     │
│ ↵         review sent                                        │
│ ▾         an open group                                      │
│ ▸         a folded group                                     │
│ ▎         the selection                                      │
│ ◂         tmux's active pane                                 │
╰ any key closes ──────────────────────────────────────────────╯
```

The tiles' help lists `h l ← →` as columns, `1-9` as tiles by number, `[ ]` as
pages, and no group keys.

### The panel's text boxes

Compose and the one-line boxes have a cursor, drawn as `▏` (`|` in the ascii
tier) in accent and reversed, so a typed `|` never passes for it. It sits
between two grapheme clusters, placed by display width: the wrap measures each
cluster as the buffer draws it, so `❤️` and `1️⃣` take 2 columns, not 1.

- `←` `→` move a character, `^←` `^→` a word, `Home` `End` (`^A` `^E`) to the
  line's start and end, `Del` deletes the character after it. `⌫` and `^W`
  delete the character and the word before it, and `^U` everything before it,
  as in readline. Typing and a paste go in at the cursor.
- Moves and deletes go by grapheme cluster: a ZWJ family, a skin tone or a
  flag is one symbol, and Backspace takes it whole. A letter typed before a
  stray combining mark takes the mark, and the cursor goes past both.
- The box keeps the cursor's line on screen; the route lines under it show
  whenever it leaves room. A note never scrolls away behind it: when the head
  has to scroll with the body, the note stays on top (cut with an ellipsis
  when only one row is left), so the cap note is always read.
- An empty Compose shows a dim `type what you want…`; it is drawn, never typed.
- Over 6000 characters the head counts `N/8000`, the cap; at the cap a typed
  or pasted character is turned away and a note says how many.

### The panel on a small screen

The panel is built down to 28x7, the floor measured over every view: under 28
columns the armed Answer's `esc` hint or the title's pane id is cut, and under 7
rows Explain's heading leaves no body row. Tablet mode has its own floor,
28x12: its buttons take rows of their own, and it is the smallest size where
every view keeps its heading, 2 or more body rows, its note and every button
uncut beside them. A Confirm that carries line warnings, and a view
with a long note, keep 1 body row there and scroll it. Under 12 rows, Result (heading, line warning, two rows of
buttons) and Confirm (the big pair) at 28 columns fall short; 40x12, a phone in
portrait, is at the floor. The Home tile refuses to flip into tablet mode
under 28x12 (a warn note says so), since the card would leave Home's only way
out, `esc`, closing the panel; back to pc is always allowed. A panel that
starts in tablet mode (`auto` on Termux, or the option) under 28x12 shows the
card like any other start, and `esc` closes it from Home. Below the floor the
panel draws one card,
`make the window larger or rotate` and `esc goes back`. `esc` and `q` do what
they do on the view under the card (an Answer still asks first, Compose keeps its
draft, only Home closes), `^C` quits. Any other key, `q` typed into a box and a
paste are dropped: nobody can see what they would do. An open `?` closes.

At 40x12 (a phone in portrait), Steps:

```
╭ panel · %3 · work ───────────────────╮
│ Steps (bash)                         │
│                                      │
│  1 Remove the old build              │
│      $ rm -rf target                 │
│ ▎2 Build again                       │
│      $ cargo build --release         │
│  3 Tap Allow in the app              │
│                                      │
│                                      │
│                                      │
╰ i insert  s send  ? keys  esc draft ─╯
```

- **Footers drop whole hints**, lowest priority first, never cut one. `esc` and
  the help key always stay, and `esc` names where it goes: `home`, `draft`,
  `answer`, `back`, `cancel` or `close`.
- **`?` lists every key of the view on screen**, `^N`, `⇥` and `F2` included,
  and each `esc` row says where it leads, so the way back to Home reads off the
  chain. In the Compose box and the one-line boxes `?` is text, so `F1` opens it
  there (and everywhere else). Any key closes it; `↑↓` `j` `k` `PgUp` `PgDn`
  scroll it when it does not fit.
- **Home, Question and Steps keep the selection on screen.** A long question
  scrolls with its options, so the options never fall off the bottom; its
  `N-M of T` shows what is cut, and `PgUp` reads the question from its start.
  `Enter` after `PgUp` first scrolls back to the selection, and the next one
  answers it; `PgDn` back to the bottom, or a resize that fits the whole
  question, drops the scroll. A
  question with no options shows no digit hint.
- **Tiles** drop their description under 50 columns of text and cut it with an
  ellipsis above; a long option or step note wraps under itself.
- **The top border** gives the title way first, with an ellipsis, then drops the
  indicator's arrows; the `N-M of T` count is never cut, only dropped whole.

### The panel in tablet mode

One state machine and one set of views, shown two ways. `pc` is the default off
Termux; `tablet` is the default on it; the Home tile `Tablet mode` (`PC mode`
in tablet mode) flips it until the panel closes. A step resume opens a new
process, so it comes back in the `@cockpit_panel_mode` mode.

Why two modes and not one layout: mouse capture inside a popup takes the
terminal's own text selection away (you can no longer drag over an answer to
copy it), and tall tiles cost rows a keyboard user has no use for. A touch
screen needs both.

In tablet mode:

- **Mouse capture** is `?1000h ?1006h`: presses, releases and the wheel, SGR
  encoded. Not `?1003h`: a report for every pointer move is a wakeup and a
  redraw each. Every exit path writes `?1006l ?1015l ?1003l ?1002l ?1000l`: `q`,
  `esc`, `^C`, an action, SIGHUP and SIGTERM (through the loop's restore), the
  panic hook (`restore()`), a failed `init()`, and the hangup watch before its
  `_exit`. These are set-mode sequences, not queries: the terminal answers
  nothing, so crossterm #1104 (a wedged parser on an unknown report) cannot
  trigger. tmux 3.8 asks the outer terminal for the mouse whenever the popup
  does, with `mouse` on or off, and hands the popup popup-relative coordinates.
- **A tap is a bare left press.** The release, drags, the right button, motion,
  and a press with Shift, Ctrl or Alt held (a click meant for the terminal) do
  nothing, so a press and its release are one action. The wheel is
  `↑`/`↓`: it scrolls Result, Answer, Confirm and `?`, and moves the selection
  on Home, Question, Explain and Steps. In Compose and the one-line boxes it does
  nothing.
- **Hit map.** Every frame `render()` records a rect per tappable element: the
  tiles, the options, the steps, each button, Confirm's yes and no. A tap runs
  the key the element stands for, through the same `App::key()`; a tile or an
  option is the selection plus Enter, a step is the selection only. With `?` open
  a tap anywhere is `esc`, which closes it. Under the floor a tap does nothing:
  the card hides the view, and what `esc` does there differs by view (it arms
  and then discards an Answer, cancels a paid call), so two taps on a card the
  user cannot read must not lose anything. The `esc` key still goes back, and
  the card's remedy, a larger window or a rotation, needs no tap.
- **Tiles are boxed, 3 rows tall,** on Home, Question and Explain, when the
  whole view still fits; otherwise they are 1 row, as in pc mode. Measured, not
  a fixed height: Home on a Claude Code pane (5 tiles) needs 21 rows, so a
  80x24 popup gets tall tiles and a 40x20 phone in portrait does not.
- **Buttons replace the footer.** Each footer hint that names one key becomes a
  `[ verb ]` button on the bottom rows (Compose: write, fix spelling, insert as
  typed, type, model, keys, home), wrapped to more rows when narrow. Ranges and
  arrows (`1-5`, `↑↓`) become no button. Keys keep working in both modes.
- **Confirm** gets a big `yes` and `no` pair at the top, 3 rows tall while the
  body keeps half the rows. `yes` is `y`, never Enter, so a multi-line send
  still takes an explicit yes. An armed Answer gets a big `discard it`
  (`esc`) and `keep it` (`n`) at the top, and its `[ home ]` button becomes
  `[ keep it ]` in place; its note says "tap discard it, or esc again".
- **A double tap never confirms.** Two defences, as in type-panel, whose tap
  guard was there for the same reason:
  - Layout: every button that opens a Confirm or arms a discard is a bottom
    button, and what confirms is drawn at the top, so the second tap of a
    double tap lands on no confirming cell. A property test checks every size
    from the tablet floor, 28x12, to 120x40 for each such button (Steps, Result and Answer send and
    insert, Answer home, Compose insert as typed).
  - Guard: a tap within 300 ms of a frame that shows a new view (another view,
    the Answer armed, `?` opened or closed) is dropped. Keys are never dropped.
    The selection moving is not a new view. The loop passes its clock in, like
    `tick`, so the tests set the time and read no clock. The first frame starts
    the guard too, and after a steps resume it lasts 800 ms: the close, the
    paste and the reopen fit inside a slow double tap, and the reopened Steps
    put `[ insert ]` back under the same finger. The state file is unchanged.

At 40x20, a phone in portrait, a send:

```
╭ panel · %3 · work ───────────────────╮
│ ╭───────────────╮ ╭────────────────╮ │
│ │   yes, send   │ │       no       │ │
│ ╰───────────────╯ ╰────────────────╯ │
│ send this to %3 (bash) and press     │
│ Enter?                               │
│                                      │
│   rm -rf target                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│ [ keys ]                             │
╰──────────────────────────────────────╯
```

**Copy, in both modes.** `x` on a Result copies it, on Steps the selected
step's command (its note when it has none), on an Answer the prompt for the
agent (the last answer when there is none). `y` was not free: it is Confirm's
yes. The text is cleaned like a paste (control characters out, tabs and
newlines kept): an ESC copied out would be a keystroke wherever it is pasted.
The panel writes OSC 52 (`ESC ] 52 ; c ; <base64> BEL`) to its own tty; nothing
is run, no termux-* binary is needed, and a failed write is a warn note, never
an exit. The terminal never answers an OSC 52, so the note says only what is
known: `sent N chars to the clipboard (needs tmux set-clipboard on)`. Inside
tmux it needs:

```sh
set -g set-clipboard on    # the default, external, drops an app's OSC 52
```

Measured on tmux 3.8 with a private server: from a popup, with `set-clipboard
on`, the text lands in a tmux buffer and is passed on to the outer terminal;
with `external` (the default) or `off`, nowhere. `allow-passthrough` is not
needed, since the sequence is not wrapped. The outer terminal must accept OSC 52
too (Termux does).

**Size.** tmux 3.8 drops an OSC string longer than `input-buffer-size`
(default 1048576) without a word: measured, 786,000 bytes of text (1,048,000 of
base64) landed and 786,432 did not. Over 768,000 bytes the panel copies nothing
and says `too long to copy: N chars`. Termux's emulator is said to cut OSC
strings near 8 KB; that is unconfirmed, so under Termux a copy over 6,000 bytes
is still sent, with a warn note that it may be cut.

**Termux.** `examples/termux.properties` has an extra-keys row with a key that
sends `prefix + f`.

### Hiding the panel (#278)

The panel is a modal popup: while it is open tmux gives it every key, so no
other window can be reached. `^Z` hides it instead of closing it. The popup
closes and its whole working state is kept for that pane; the next `prefix f`
on the same pane opens it again where it was. Picked because it is free in
every view (no box types a control character, and no view binds it), reads as
"suspend" to a terminal user, and is a `^` key, so tablet mode makes it a
`[ hide ]` button like `^S` and `^T`. `^H` was not: many terminals send it for
Backspace.

- **Kept:** the view (Home, Compose, Question, a one-line box, Result, Steps,
  Explain's questions, Answer, Error) with its selection and scroll, the draft
  and its cursor, the output type, the model, and the run: the flow, the
  turns, the question rounds and their stack, the options, Explain's log, the
  prompt for the agent, the model the call went to and why. A Confirm is kept
  as the view it was opened from, so a resumed panel never opens on a yes. An
  armed Answer comes back unarmed. The `?` overlay is not kept.
- **Not while Busy.** A running `claude -p` dies with the process, so `^Z`
  then says `a call is out: esc cancels it, then ^Z hides` and changes
  nothing. Saving the request and offering to run it again on the way back
  was the other choice; refusing is the safer one: a re-run is a second paid
  call, its answer can differ, and it would go out on a screen captured
  later than the one the question was about.
- **Nothing is sent on the way back.** The panel opens on the kept view with
  an info note, `back where you left it`; the draft, a result or a step waits
  for its key. The first view gets the 800 ms tap guard of a steps resume,
  and the keys typed during the reopen are dropped, as there.
- **The captured command is kept**, as in a steps resume, so the wrapper's
  pane guard compares against what the draft or the reply was written for.
  When the pane runs something else by now the note says so, warn level:
  `...; the pane now runs vim, not bash, so a paste is refused`.
- **One state per pane.** `prefix f` on another pane opens a fresh panel and
  the hidden one stays. The wrapper says `cockpit: hidden · prefix f on %N
  brings it back` (the bound key, from `@cockpit_panel_bound`); opened from
  the board, that is the board's note and the board comes back, as after `esc`.
- **Stale state is dropped, with a note.** A file older than 24 h
  (`action::HIDE_AGE`) is refused (`the hidden panel was dropped: it is older
  than 24 h`), and the wrapper also deletes any older than a day. A hidden
  panel whose pane is gone is deleted the next time the panel opens on any
  pane of that server, which says `dropped the panel hidden on %N: the pane is
  gone`. A corrupt file is refused whole, with the reason in the same note.
- **Too much to keep** (over 1 MiB): `^Z` says so and the panel stays open.

**The state file.** The binary writes `$COCKPIT_ACTION.state` and no action
line; the wrapper takes that pair as a hide and moves the file to

```
${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/panel/<server pid>-<N>.state
```

in a 0700 directory, `<N>` the pane id without its `%`. The server pid is in
the name because pane ids start again at `%0` with a new server and every `-L`
server shares the directory. The next run on the pane moves the file into its
private directory before the binary starts (a second client's `prefix f` finds
nothing) and passes it back:

```
cockpit-tui panel %N --resume <state> --last hidden
```

It is version 2 of the panel's state file: a `cockpit-panel-state 2` line,
then one line of JSON, an object with `pane`, `saved` (unix seconds), `cmd`,
`draft`, `cur`, `out`, `model`, `view`, `busy_back` and `run`. Version 1, the
steps resume, is unchanged and still read by `--last ok|failed:<why>`; each
reader refuses the other's version. The read has version 1's rules (no
symlink, a regular file, checked before the non-blocking open and again on
it, created new with `O_NOFOLLOW`, mode 0600) with a 1 MiB cap, and is as
strict: one body line, the pane must be the one opened on, at most 24 h old
and not more than a minute in the future, every key present and none unknown,
every cursor on a character boundary of its text, the model and the type
known, steps 1 to 64 with the selection among them, and a Question or Answer
only with the run it is drawn from. Any failure opens a fresh panel with the
reason in a warn note.

### Contract changes for tablet mode (for #61)

- New option `@cockpit_panel_mode auto|pc|tablet`, read at load; the binding
  passes it as `display-popup -e COCKPIT_PANEL_MODE=<mode>`.
- New variable `COCKPIT_PANEL_MODE`; a value other than `auto`, `pc`, `tablet`
  or empty is exit 2, a usage error.
- `view::render()` returns `(total, room, Vec<Hit>)`: the hit map is new.
- `view::too_small(area, tablet)` takes the mode: tablet mode's floor is
  `TABLET_MIN_W` x `TABLET_MIN_H` (28x12), pc mode's stays 28x7.
- `backend::init(paste, mouse)` takes a second flag; `backend::mouse()`,
  `osc52()` and `copy()` are new. `restore()` turns the mouse off.
- In tablet mode the panel writes `?1000h ?1006h` and reads mouse events; in
  both modes `x` writes OSC 52 to the tty. The board is unchanged: no mouse.
- Home has one more tile, `Tablet mode` / `PC mode`, before `Close`, so Close's
  digit moves up by one (`4` to `5` on a Claude Code or shell pane).
- `x` is a new key on Result, Steps and Answer.
- `App::tap(t, now)` and `App::frame(now)` take the loop's clock in ms; a tap
  within 300 ms of a new view is dropped. `view::mouse()` takes `now` too.

### Contract changes for the return trip (for #61)

- The CLI line: `cockpit-tui board [--select %N] [--note <file>]`, each at most
  once; anything else is exit 2.
- New state file `board.state`, its path, head and read rules (see
  [The return trip](#the-return-trip)). Steps 3 and 4 append `fold` and
  `filter` lines to version 1.
- New option `@cockpit_board_return` (`on`); new variable `COCKPIT_PANEL_NOTE`,
  between the two wrappers only.
- The board's popup passes `-e COCKPIT_PANEL_MODE=<mode>` (the parked window
  too) and its title is ` board `, without `#{session_name}`.
- `scripts/cockpit-panel` from the board writes its outcome to the note file
  instead of `display-message`; `prefix f` is unchanged.
- `info` from the board no longer waits on a bare `read -r _` (a line): it
  names its key, `any key: back to the board`, and takes one.
- The board's note: 5 s for success, until a key for a `✕` line (D20); the 2 s
  note on `a` is unchanged.
- The board opens on D5's choice: `--select`, else `board.state`, else the
  first `▲`, else the client's pane, else the first tile (it was always the
  first tile).
- `board::run` takes `&Opts`; `panel::xdg` is `pub(crate)`.

### Contract changes for the list (for #61)

- The default layout is the grouped list; the tiles move behind `v`, which
  toggles them in both layouts. The tiles keep every other key.
- In the list, `h` `l` `←` `→` fold and unfold instead of moving a column,
  `1`-`9` are the agent rows on screen instead of absolute tiles, and `[` `]`
  scroll without moving the selection a page. New keys: `space`, `-`, `=`,
  `n`, `p`, `v`; `↵` on a header toggles its fold.
- `board.state` version 1 gains `fold <session>` lines after `sel`.
- The list's header line adds the session count and the error count; the
  tiles' header is unchanged.
- The list captures no pane: the worker skips `capture-pane` until `v`.
- The `?` help is generated per layout; `board-help-{60,100,180}` are the
  list's, and the tiles' is `board-help-tiles-100`.
- Two marked seams for #228 in `board.rs`: `row_extra(&Pane, &Theme) ->
  Vec<Span>`, extra cells on a list row (none today), and `info(&str) ->
  Option<String>`, where `i` opens a view (today `info %N`).

### Contract changes for find (for #61)

- New list key `/` (`Act::Find`) opens the find field; it is in the footer as
  `/ find` after `space fold` on an agent and after `n p sessions` on a header,
  and in `?`, which gains an "in the find field" section.
- `^C` closes the board from anywhere (`action` maps it to `Act::Quit`); the
  `?` row reads `q esc ^C`.
- `esc` on the list is a ladder (7.1): a `✕` note, the info overlay, the find
  field, a kept filter, then the board. `board::{esc_step, EscStep}` hold it.
- In the field every printable key is text, `q`, `?` and the `EXTRAS` keys
  included; `esc` and `^C` replace `q`, `F1` replaces `?` (D14).
  `board::{find_key, FindKey}` map the keys.
- `board.state` version 1 gains an optional last line, `filter <text>`, read
  strictly; `board::Saved` gains `filter: Option<String>`.
- `board::Nav` gains `filter`, `field` and the methods `rows`, `fit`,
  `filtering`, `open_find`, `clear_find`, `find_act`; `board::{find_match,
  rows_with, state_word, find_hints}` are new. The open field takes two list
  rows.
- The tiles take no filter: `/` is list-only.
- Goldens: new `board-find-{60,100,180}` and `board-find-none-{60,100}`. The
  footer gains `/ find` in `board-list-{100,180}`, `board-folded-{100,180}`
  and `board-extra-120`; the help's new `/` row, the find section and
  `q esc ^C` move `board-help-{60,100,180}`, and `q esc ^C` moves
  `board-help-tiles-100`.
- Not built here: the 100/180 mockups' right pane (step 7), the tablet chips
  and `[ keep ]` `[ clear ]` (step 8), and the filter in the top line's right
  end; the list's frame title carries it.

### Contract changes for the board's extra actions (for #61)

- New board keys `x` (`kill %N`, after a confirm) and `R` (`request %N`), from
  the `EXTRAS` registry; new verbs `kill` and `request` on the action line.
- New sidecar `$COCKPIT_ACTION.expect` (rules under [the action
  file](#the-freeze)); `panel::action::{write_expect, expect_body, EXPECT_MAX}`
  are new. For `kill` it holds the target's window id (`@N`), never the name.
- The fleet read appends a 13th `list-panes -F` field, `#{window_id}`, kept in
  `data::Pane::win`; a row with fewer than 13 fields is skipped. `--json` and
  `--rows` do not carry it. `board::{is_window_id, refused}` are new: a pane
  with no `@N` id gets a `✕` note on `x` instead of a confirm.
- `board::run` returns `Option<board::Exit>` (`line`, `expect`), not
  `Option<String>`; `board::View` gains `confirm: Option<&Confirm>`;
  `Act::Extra(char)` is new.
- The confirm: `y n esc ? q ^C` only, `↵` off; a refresh holds the rows; a gone
  target closes it with a `✕` note. The footer while it is open lists those
  keys; on an agent the footer gains `x kill` and `R request` before `?`.
- `scripts/cockpit-board` removes `$f.expect` before each run of the board.
- Goldens: new `board-confirm-{60,100}`; the footer or the help moved in
  `board-{80,80-scrolled,100,120,160,180}`, `board-ascii`, `board-nocolor`,
  `board-nocolor-100`, `board-stale-100`, `board-list-{100,180}`,
  `board-tiles-100` and `board-help-{60,100,180,tiles-100}`.

### Contract changes for hide and resume (for #61)

- New key `^Z` in every panel view: hide (see [Hiding the panel](#hiding-the-panel-278)).
  Every footer but Busy, Confirm and an armed Answer gains `^Z hide` before
  the help key, so a narrow footer drops it first; in tablet mode that is a
  `[ hide ]` button. Every `?` list gains a `^Z` row.
- `--last hidden` is new on `cockpit-tui panel %N --resume <state>`: it reads
  a version 2 state file. `--last ok|failed:<why>` and version 1 are unchanged.
- `panel::action::{Hidden, hide_body, write_hidden, read_hidden, HIDE_MAX,
  HIDE_AGE}` and `App::{hide, unhide}` are new; `read_state` shares its file
  checks with `read_hidden`.
- `scripts/cockpit-panel` treats a `.state` with no action line as a hide,
  keeps it under `tmux-cockpit/panel/` in the state directory, passes
  `COCKPIT_PANEL_DROPPED`, and asks tmux for `#{pid}` once per run.
- Goldens: the footer gained `^Z hide` in `panel-{home,error,error-long,input}-{40,60,80,120}`,
  `panel-explain-ask-{60,80,120}`, `panel-{answer,question,result,steps,steps-resumed}-{80,120}`,
  `panel-compose{,-count,-cursor,-empty}-120`, `panel-ascii-{home-40,home-80,answer-80,question-80,steps-80}`;
  the button in `panel-tablet-{home,compose,steps}-{40,80}`; the key count in
  `panel-help-{compose,steps}-{40,60,80,120}`. Home's `[ close ]` button moved
  right by the new one; the suite's tap-close exit taps it at its new column.

### Contract changes for removing request (for #61)

- #288: the board's `R` key and the `request` verb are gone. `EXTRAS` holds `x`
  (`kill`) alone; the footer on an agent ends `x kill   ? keys   q close`, and
  `?` loses the `R` row. `prefix R` (reload) and the board's `r` are unchanged.
- `scripts/cockpit-board` drops its `request)` arm: a `request %N` from an older
  binary is logged as an unknown verb and ends the board. `tmux-review
  --request`, the agent's own path (the cockpit-review skill), stays.
- Goldens: the footer moved in `board-{100,120,160,180}`, `board-extra-120`,
  `board-list-180`, `board-nocolor`, `board-nocolor-100`, `board-stale-100` and
  `board-tiles-100`; the help in `board-help-{60,100,180,tiles-100}`.

### The triage overlay

Phase 2, not shipped. It answers a blocked agent in place rather than attaching
to its pane, and reserves the action line `deliver %N`. Its design is on
[#61](https://github.com/TzachiSh/tmux-cockpit/issues/61), in the research
lane's `ux.md` §2.

### The picker

`cockpit-tui pick` is the one list-and-filter component (#228, decision 6). It
replaces fzf wherever cockpit asked a person to choose a row.

```
printf 'main\tmain\nfeat/x\tfeat/x  Add x\n' | cockpit-tui pick --prompt branch --header 'switch to' --preview notes.txt
```

Rows come on stdin as `<key>TAB<display>`; a line with no tab is its own key and
display, and a line with no key is skipped. The chosen key is the only thing on
stdout. The screen is drawn on `/dev/tty` and keys are read from it, since stdin
is the rows and stdout is the answer.

| exit | meaning |
|------|---------|
| 0 | a row was chosen; its key is on stdout |
| 1 | `esc` or ctrl-c, or no rows; stdout is empty |
| 2 | a bad argument, an unreadable `--preview` file, no terminal, or a stdout that cannot be written |

`--preview <file>` is static text shown right of the list when the screen is 60
columns or wider (the first 64 KiB, no scrolling). Nothing mutates: the caller
acts on the key. Row, header and preview text have control characters stripped
before they are drawn, and every text is drawn in `value` (D17). Under 28x12 the
screen is the floor card.

The filter is a case-insensitive substring over the display text, never the key.
The picker is a text-entry screen: every printable key, `j` `k` `q` `g` `G` `?`
included, is a letter of the query, so the first letter of a name is never a
command. Ctrl and Alt chords are not letters.

| key | does |
|-----|------|
| letters | add to the filter |
| `⌫` `^U` | delete a letter / clear the filter |
| `↑` `↓` `^P` `^N` `PgUp` `PgDn` | move the selection (`chrome::scroll`) |
| `Home` `End` | first / last row |
| `↵` | print the selected row's key, exit 0 |
| click | select the row under the pointer; `↵` chooses |
| wheel | scroll the window three rows, the selection stays inside it |
| `F1` | the keys; any key but a scroll key closes it |
| `esc` `^C` | cancel |

The key is bytes: rows are split on `\n` and the first tab as bytes and the key is
printed exactly as sent (a path need not be UTF-8); only the display is decoded.
Input is read up to 8 MiB and cut at a line boundary; the header then ends with
`(first N rows)`. Bidi and zero-width controls are stripped with the other
controls, so a row cannot show one name and print another. If stdout cannot be
written the exit is 2. At the narrowest popup the footer keeps `↵ choose` and
`esc cancel`.

Mouse capture is on for the picker, unlike the board: a list you click is the
point of it.

**stderr.** A line with a tab but nothing before it has no key and is skipped.
When that happens, `cockpit-tui pick` says so once on stderr, whatever else
happens: `cockpit-tui: pick: skipped 3 rows with no key` (`1 row` when it is
one). If every row was skipped there is nothing to choose, so no screen opens,
stdout is empty and the exit is 1.

**Reusing it.** `sessions` (and the branch picker) build on the same `Pick`:
`Pick::set_rows` swaps the rows of an open list and keeps the query and the
selection on the row with the same key; `Look` swaps the floor card's title, the
footer's hints, the `F1` help, each row's line and the empty-list text without
redrawing the screen; `Pick::entry` borrows the prompt line to ask for something
else; `Pick::list_pct` moves the divider when a preview sits beside the list;
`open_screen` and `screen` are the tty setup both pickers share.

## States

Exact text. All lower case, no exclamation marks. The `cockpit:` prefix appears
only on `display-message` output, never inside the board, where the frame
already says who is talking.

**No agent panes:** the header says ` board · 0 agents`, and under it this card
sits centred; on a narrow board it wraps, never cuts.

```
╭ board ─────────────────────────────────────────────────────╮
│                                                            │
│ no agent panes                                             │
│                                                            │
│ cockpit tracks a pane once an agent runs in it.            │
│ start an agent in any pane and it shows up here.           │
│                                                            │
╰ q close ───────────────────────────────────────────────────╯
```

**tmux not answering**, in two stages, never an instant blank screen. A read
that fails, or none arriving for 3 s (a hung tmux fails nothing, it blocks),
keeps the last-known tiles and the header carries `· stale 34s`, counted from
the last good read. After 30 s or three consecutive failures this card covers
the grid; `^L` asks again:

```
╭ board · stale 34s ─────────────────────────────────────────╮
│                                                            │
│ tmux is not answering                                      │
│                                                            │
│ last good read 34s ago · the server may have exited.       │
│ the tiles below are from that read.                        │
│                                                            │
╰ ^L retry   q close ────────────────────────────────────────╯
```

When no read has worked since the board opened, there is no age and no tile
to vouch for. The card comes as soon as the first read fails, or after 3 s if
it hangs, and says only that:

```
╭ board ─────────────────────────────────────────────────────╮
│                                                            │
│ tmux is not answering · no read has worked yet.            │
│                                                            │
╰ ^L retry   q close ────────────────────────────────────────╯
```

**Run outside tmux** (`$TMUX` unset and no `COCKPIT_TMUX`): no TUI at all, one
line on stderr, exit 1. A usage error must not raise a full-screen app you then
have to work out how to leave.

```
cockpit-tui: no tmux server — run this from inside tmux, or set COCKPIT_TMUX
```

**Loading:** a frame is drawn *before* the first fetch, so the popup is never a
black rectangle while `claude agents --json` takes its measured 380 ms.

```
╭ board ─────────────────────────────────────────────────────╮
│                                                            │
│ reading the fleet …                                        │
│                                                            │
╰ q close ───────────────────────────────────────────────────╯
```

**No transcript for a pane:** the tile still renders, the preview is the
fallback. `doing` reads `—`, `said` is omitted rather than printed empty. Never
the word "unknown".

**A transcript that cannot be read:** `doing` reads `transcript unreadable`,
once, for that tile only, and the errno goes to the log. One unreadable file
must never take the screen.

**Under the floor:** below 40 usable columns the board refuses to draw the
tiles. Better than a row that lies about which agent it names. It draws the
panel's floor card instead:

```
╭ board ─────────────────────╮
│ make the window larger or  │
│ rotate                     │
│ esc or q closes            │
╰────────────────────────────╯
```

Under 8 rows, where the width is fine, the card says `too short · make the
window larger or rotate`. Under the card only `q`, `esc` and `^L` act; any
other key is dropped, since nobody can see what it would do.

**`a` on a tile that is not waiting on you:** one line in the footer row for
2 s, replacing the key hints.

```
 work:2 is not waiting on you — ↵ jumps there, /btw asks it while it works
```

**No binary, or a non-zero exit:** one `display-message`, exit 0, never a broken
popup. `prefix r` opens the same board.

## Rules

Every lane of the board reads this section before its first commit. The freeze
is agreed; changing any of it takes a comment on
[#61](https://github.com/TzachiSh/tmux-cockpit/issues/61), not a commit.

### The freeze

**`data::Pane`**, field for field:

```
id, wher, name, cmd, state, title, msg, sess, path, root, spool, pinned,
delivered, pid, review, doing, said, asked, loc, pm,
since: Option<u64>, asked_since: Option<u64>, idle_since: Option<u64>
```

Derives `Default, Clone, Hash, PartialEq`. The durations are `u64` and not
`Duration` because of `Hash`.

**`--json`**: an array of objects with exactly these 14 keys, in this order:

```
id, where, state, title, doing, said, asked, msg, since, loc, diff, review,
session, path
```

`where` is `sess:idx`, and nothing else: the window name is `data::Pane`'s
`name` field, which `--json` does not carry. `title` falls back to the pane
name, then `(no title)`. `since` is seconds or `null`. `diff` is the `±`
string. `state` is one of `needs-input | running | done | error | ""`.
Pretty-printed, one object per agent pane, never a non-agent pane.

**`--rows`**: tab-separated, 8 fields, no header:

```
glyph \t id \t where \t title \t (msg if needs-input else doing) \t since \t loc \t diff
```

**The action file.** Exactly one line, `\n`-terminated, written to
`$COCKPIT_ACTION` on exit. Nothing else is ever written there:

```
jump %N · review %N · panel %N · deliver %N            (deliver reserved for phase 2)
kill %N                                                 (the board's EXTRAS, #228's arm)
paste %N · send %N                                      (the panel)
```

The verb is `[a-z]+`, the argument is a tmux pane id. The binary executes
nothing. `paste` and `send` carry text, which goes to `$COCKPIT_ACTION.text`,
mode 0600, control characters stripped; with `COCKPIT_ACTION` unset the panel
refuses them and exits 1.

An action whose `EXTRAS` entry has an `expect` also writes
`$COCKPIT_ACTION.expect` before the action line, under `.text`'s rules: created
new with `O_NOFOLLOW`, mode 0600, never over a file that is there. It is one
`\n`-terminated line, every control character dropped (`\t` and `\n` too), at
most 1 KiB (`EXPECT_MAX`); a longer value is refused, not cut, and the board
exits 1 with nothing written, as is an empty one. For `kill` it is the target
pane's `#{window_id}` (`@9`) when the confirm opened, not its name: agent
windows tend to share one (`claude`), and a pane moved into a namesake would
pass a name check. The wrapper reads the pane's `#{window_id}` again and
refuses with a `✕` note when it differs. A pane with no `@N` window id is
not offered the kill: `x` shows `✕ nothing done: <sess:idx> has no window id
to check, kill not sent` and opens no confirm. Only ASCII control characters
are dropped (`is_ascii_control`), so the bytes match what a shell `read` sees;
a C1 character such as U+009B stays. With `COCKPIT_ACTION` unset such an action is refused,
exit 1. `scripts/cockpit-board` removes `act.expect` before each run of the
board, so one trip's value never stands in for the next.

Beside it, `$COCKPIT_ACTION.cmd` is one line: the pane's
`#{pane_current_command}` when the panel captured it. Both files are written
before the action line, each created new with `O_NOFOLLOW`. `scripts/cockpit-panel`
reads the pane's current command again just before the paste; when the two
differ (a Claude Code that exited would take the answer as shell input), it
pastes nothing, logs it, and says
`cockpit: the pane now runs <now>, not <then>; nothing sent`.

**Steps stay open.** An action taken from the Steps view also leaves
`$COCKPIT_ACTION.state`, written before the action line, created new with
`O_NOFOLLOW`, mode 0600. It holds what the view needs to come back: a
`cockpit-panel-state 1` head, the pane, the captured command, the label, the
step the action was for, the verb, then one `step <done 0|1>\t<lang>\t<note>\t<cmd>`
line per step (`\\`, `\n`, `\r`, `\t` escaped). After running the action the
wrapper moves it aside and runs the binary again in the same popup:

```
cockpit-tui panel %N --resume <state> --last ok|failed:<why>
```

(`--last hidden` is the other resume: [Hiding the panel](#hiding-the-panel-278).)

`ok` marks that step `✓ inserted` or `✓ sent`, by the verb (earlier rounds' steps `✓ done`:
the state file keeps no verb per step), and selects the next one with a command;
`failed:<why>` shows why as an error note and marks nothing. The loop ends when
the binary exits with no action, or with one taken anywhere but Steps. The
binary reads the file back strictly: no symlink, a regular file of at most
64 KiB (checked before the open, which is non-blocking, and again on the open
file), version 1, at most 64 steps, every line in its place, and the pane must
be the one it was opened on, or it opens on Home with an error note. The
command written back is the captured one, so the wrapper's pane guard still
compares against what the steps were written for. The binary still executes
nothing: every paste and Enter is the wrapper's.

The wrapper ends every action in one `display-message` on the pane:
`cockpit: inserted N chars into %N` for `paste`, `cockpit: sent to %N` for
`send`, `cockpit: could not load the paste buffer; nothing sent to %N` or
`cockpit: could not paste into %N; nothing sent` when tmux refuses (tmux's
error goes to the log). Under `COCKPIT_READONLY` it runs no tmux command at
all and only logs what it would have sent.

**Signatures.** Frozen; each lane changes bodies only.

```
tmux(&[&str])           -> Option<String>   None = unreachable, or a non-zero exit
preview(&str, usize)    -> Option<String>   None = capture-pane failed
```

Neither returns an empty string for a failure. A `None` from `preview` renders
as one line, `capture failed`, never a blank tile: an empty preview and a dead
pane are different facts and a blank tile tells you which one it is.

**CLI:** `cockpit-tui board [--select %N] [--note <file>] | panel %N [--resume <state> --last ok|failed:<why>|hidden] |
transcript ... | --rows | --json | --version | -h | --help`. There is no `fleet`, `info` or `ask` subcommand: the
board reaches those through the action file and the shell scripts.

**Helper subcommands (#199):** `cockpit-tui transcript file %N | doing <jsonl> |
tail <jsonl> [max]` is the transcript reader; `scripts/lib/transcript.sh` is a
shim over it. It is byte-identical to the jq reader it replaced (Freeze 1 on #54)
with two #193 changes: a row whose `message` is not an object is skipped where jq
failed the whole read, and
`<bash-input>`, `<bash-stdout>` and `<bash-stderr>` prompts are dropped like the
other harness tags. It also departs from jq in three declared places:
- a content item that would raise a jq runtime error is skipped, not fatal;
- JSON only jq accepts (NaN, leading zeros, nesting deeper than 128) skips the line;
- `file` sorts project directories in byte order, not the locale's.

Its exit-code rule overrides the one below: every failure, a usage error
included, is no output, status 1 and nothing on stderr.

The git helpers keep the exit-code rule below:

- `cockpit-tui git-flags` prints the fixed git safety flags, one word per line,
  without starting git. `git-flags --filters <dir>` prints the `-c` overrides
  that blank every filter driver `<dir>`'s config names.
- `cockpit-tui diffstat <root> [--base <rev>]` prints
  `<add>\t<del>\t<new>\t<pm>`, the board's diff count against `<rev>` (default
  HEAD; an unborn HEAD counts the tree as new). Exit 1 when `<root>` is not a
  repo, the diff fails or the base starts with `-`; 2 on a usage error.

**Log format:** one line per event, `<unix_micros> <text>`, append-only, only
when `COCKPIT_LOG` is set. No rotation.

**Exit codes:** `0` normal (an action was written, or none), `1` runtime failure
(tmux unreachable, terminal not a tty), `2` usage error (unknown subcommand, bad
pane id, a bad `COCKPIT_PANEL_MODE`). The binding treats a missing binary and a non-zero exit the same way.

`--rows` and `--json` exit 1 when the `list-panes` call fails, so a wrapper can
tell the two quiet answers apart: `[]` with exit 0 is "no agents", exit 1 is
"no tmux".

### The hard rules

- **Never enable `Wrap`** on a preview. ratatui #2679 (open) panics on
  double-width graphemes in a wrapped `Paragraph`, and wrapping folds
  200-column pane lines into `↳` fragments. A `Paragraph` with no `Wrap` clips
  at the widget edge for free.
- **Send no terminal queries**, and pin `crossterm = "=0.29.0"` exactly, never a
  caret range. crossterm #1104 (open): an unrecognised `CSI ? … $y` report
  wedges the parser and swallows all later input, and tmux emits exactly that
  family around a popup. Keep `q` as an exit that does not route through Escape
  parsing.
- **No mouse on the board; on the panel only in tablet mode,** which turns it
  off on every exit path. Capture takes the terminal's text selection away.
- **No tokio, no `notify`, no clap, no anyhow, no color-eyre, no chrono, no
  git2.** One input source, one timer, about four forks a tick. Five
  subcommands parse from `args[0..2]`.
- **`ratatui::init()` paired with `panic = "abort"`**, never a Drop-based
  terminal guard with abort: no unwinding means Drop never runs. `init()`'s
  panic hook runs before the abort and restores the terminal.
- **Emit truecolor unconditionally** from `@thm_*`. Measured on 3.7c: tmux
  downgrades per attached client, to nearest-256 without RGB and to 16 on a
  16-colour client, and the inner `TERM` has no effect. There is no 16-colour
  tier to write, and `#{client_termfeatures}` is at most a shortcut.
- **Honour `NO_COLOR` in-app**, every role set to `Reset`, and leave crossterm's
  own `NO_COLOR` handling off: it emits an empty SGR, which also drops bold, dim
  and reverse, the three attributes the colourless fallback is built on.
- **Actions are printed, never executed.** The panel's `claude -p` fork is a
  read only because it runs with `--tools ""`: adding a tool breaks this rule.
  Pasting into a pane stays a printed action that a wrapper runs.
- **Every tmux write goes through one function** that `COCKPIT_READONLY`
  disables, the pid publish included.
- **Tests run on private `-L` sockets**, registered in `SOCKS`, killed after.
  Never a binding, hook or option on the live server.
- **Pin a count or poll a condition. Never a clock, never a `sleep`.** Five
  issues (#18, #32, #36, #44, #47) were wall-clock gates measuring this box
  under peer load; `tests/run.sh` already has `until_cap` and `until_file`.
- **Comments are at most 2 lines** of why the code cannot say: a constraint or
  a trap. History and measurements go in the commit message.

## Contracts

The data contract (`Pane`, `--json`, `--rows`, the action file, exit codes) is
[the freeze](#the-freeze) above; the options and variables are in
[Options](#options) and [Environment](#environment).

### Modules

The module list is frozen with the rest: no lane adds, removes or renames a
module without a comment on #61. `lib.rs` re-exports and nothing else; no
workspace, no crate split.

| module | owns |
|--------|------|
| `backend` | the VS16 wide-glyph shim over the Crossterm backend, `init()` / `restore()` with the panel's bracketed paste and tablet-mode mouse capture, the OSC 52 copy, and `exit_on_hangup()`, the tty hangup watch |
| `data` | every tmux and transcript read, `Pane`, the truncation helpers |
| `board` | the tile grid and its event loop |
| `chrome` | what the board and the panel share: the footer's hint fit, the full-popup `?` help and its scroll, the floor card |
| `fleet` | the headless `--rows` / `--json` formatters and the list widths |
| `theme` | the `@thm_*` colour roles and the glyph tiers |
| `push` | `@cockpit_tui_pid`, SIGUSR1 wakeups, the exit-path cleanup |
| `info` | the pane info screen (`cockpit-tui info %N`, the board's `i` overlay) and `data::info`, its facts: git root, review base, the transcript rows |
| `pick` | the `cockpit-tui pick` list: row parsing, the case-insensitive filter, selection, click and wheel, and its screen; draws on /dev/tty |
| `sessions` | the `cockpit-tui sessions` view (#245) on `pick`'s screen; `data::sessions` the one `list-panes -a` read, the order and the freeze |
| `panel` | the type-panel port: capture, redaction, prompt envelope, the `claude -p` request, reply parsing; `panel::action` the action, text, cmd and state files |

Every mutating action stays a shell script: the binary prints
`<verb> <pane-id>` and a wrapper runs it.

### Push wakeups

A hook says "look now" with `kill -USR1 $(tmux show-option -gqv
@cockpit_tui_pid)` instead of the board finding out on its next tick. The
signals carry no payload and need none: the board reads the truth from tmux, it
only needs to be told when.

The hook must confirm the pid is ours before signalling (`scripts/agent-state`
reads `/proc/$pid/comm`, or `ps -o comm=` where there is no `/proc`): a
published pid outlives SIGKILL, and SIGUSR1's default action on a recycled pid
is *terminate*. The board unpublishes on every exit path, signal handlers
included.

### The VS16 backend shim

The Crossterm backend in ratatui-crossterm 0.1.2 emits an absolute `MoveTo`
only when the next changed cell is not `prev.x + 1`
(`ratatui-crossterm-0.1.2/src/lib.rs:241-247`); it tracks position, never
width. For an ordinary wide grapheme `Buffer::diff` skips the covered column,
so the next cell lands at `x + 2` and a `MoveTo` follows: CJK and
emoji-modifier sequences are safe. For an emoji presentation sequence (U+FE0F)
the diff deliberately re-emits the covered trailing column at `x + 1`
(`ratatui-core-0.1.2/src/buffer/diff.rs:158-165`). That is contiguous, so no
`MoveTo` is emitted, and on a terminal that draws the glyph two cells wide
everything after it shifts one column. Agent panes carry emoji, so this fires in
practice.

The shim fabricates nothing: when a changed cell lands on a column the
preceding VS16 glyph covers, the batch is flushed and that cell starts a fresh
delegated `draw`, so the backend emits an absolute `MoveTo` while the cell keeps
its real symbol and style.

The upstream fix is ratatui#2721. Delete the shim (`WideCellBackend`) once a
ratatui-crossterm newer than 0.1.2 ships; `init()` and `restore()` stay, since
they also own the panel's bracketed paste. Do not add a CJK `suppress_until` branch: it is unreachable
given the diff above, and would silently drop a real cell if it ever were
reached.

## Troubleshooting

**The popup is blank.** Something captured stdout. ratatui paints on stdout, so
`act=$(cockpit-tui board)` swallows the entire UI and the popup renders empty.
The action comes back through `$COCKPIT_ACTION`; never use command
substitution on the binary.

**The board draws but ignores every key.** It has no controlling terminal.
crossterm builds its event reader lazily on the first `poll` and caches a
failure forever, so a daemonized or hook-spawned invocation paints a TUI that
answers nothing. The binary probes with `event::poll(Duration::ZERO)` before it
initialises the terminal and prints one line instead. Reproduced with `setsid`,
with `</dev/null`, and with a non-tty stdin; a real `display-popup -E` has a pty
and is fine.

**A panel outlives its tmux server.** crossterm 0.29 rereads a hung-up tty
forever, so a loop blocked in `event::read` never sees its signal flag again.
The board and the panel each run a thread that `poll`s the tty for `POLLHUP`
or `POLLERR` and exits when it comes; the action file is left unwritten. A
`POLLNVAL` (Darwin's `poll` takes no devices) leaves the process unwatched.

**A crash left nothing behind.** `display-popup -E` closes the popup the moment
the command exits, so a panic disappears with it. The log is at
`${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/tui.log`, on by default, and
the binding appends stderr to it. Past 1 MiB the binary moves it to `tui.log.1`
and starts a new one (#291); the board logs a tick only when the fetch took over
500 ms or the read started or stopped failing.

**The glyphs shift the columns.** `▲ ● ✓ ◆` are East-Asian Ambiguous: a terminal
with an emoji-preferring fallback draws them two cells wide. The 2-column gutter
absorbs that. If it still looks wrong, `COCKPIT_GLYPHS=ascii` swaps the glyphs
and the borders for a tier that is byte-stable across fonts.

**Screenshots and benchmarks.** `COCKPIT_READONLY=1` makes the binary skip every
`set-option`, so watching the fleet does not mutate it.

**A stale `@cockpit_tui_pid`.** The board unpublishes the pid on `q`, SIGHUP and
SIGTERM. A pid left behind after a `SIGKILL` is harmless: the hook reads
`/proc/$pid/comm` before it signals anything.

## `cockpit-tui branch` (#246)

The branch popup behind `prefix b` and the status-bar branch click, built on the
shared picker (`pick.rs`). `scripts/tmux-branch` runs `cockpit-tui branch -C <dir>`
and acts on the one line it prints; the binary never mutates.

| key | does |
|---|---|
| type | filters the list (case-insensitive substring) |
| `↵` | prints `switch <branch>`, or `track <remote>/<branch>` on a remote-only row |
| `alt-↵` | prints `create <name>` from the filter text, after `git check-ref-format --branch` accepts it |
| `esc` `^C` | cancels, prints nothing, exit 1 |

Rows are local branches by most recent commit, then remote-only branches. `●` marks
the current branch. `⑂` marks a branch checked out in another worktree: git refuses
to switch to it, so `↵` gives a `✕` note and prints nothing. The preview is
`git log --graph --oneline -30` of the selection, read on a worker thread and cached
per branch; it shows `loading…` until it arrives, and below 60 columns it is hidden.

Every git read uses the `gitsafe` flags. A branch name is untrusted text: control and
bidi characters are dropped from what is drawn, and the line printed carries the exact
ref. The script treats that line as untrusted too: a verb from `switch track create`, one
line, a name `git check-ref-format --branch` accepts, and for `switch`/`track` a ref that
exists under `refs/heads` or `refs/remotes`. Git's own refusal (a dirty tree) is shown
verbatim with `[any key]`, then the picker opens again.

A branch name cannot contain a space, so the filter text `my branch` is refused with a
note on `alt-↵`.

## The info screen (#252)

`cockpit-tui info %N` is what `prefix i` opens, in a popup (`@cockpit_info_key`
still names the key; `off` unbinds it). In the board, `i` draws the same screen
as an overlay over the selected pane and `esc` returns to the board; the board
no longer exits with `info %N` for the wrapper, and `cockpit-board` has no
`info` arm: an `info` line from an old binary is an unknown verb. The module is `info` (the
screen) and `data::info` (the facts); both are in the module table.

It shows what `tmux-review --info` showed, in the same order: `pane`, `cwd`,
`dir` (`@cockpit_dir`, and `missing` when the directory is gone), `root`,
`state` with `@panestate_msg`, the `doing` / `said` / `asked` rows with their
ages and the `/btw` pointer for an agent pane, `review` with the queued spool,
`base`, `session`, `title`, `tui` and the review dir with its pinned and
delivered counts. The git root and the review base are Rust, on the `gitsafe`
flags: the base is `review_base` from `review-diff.sh` (since-review, branch,
uncommitted), so the screen names what `prefix r` diffs against. One quirk is
kept on purpose: `review_base` reads its ref rows with a tab `IFS`, which
folds empty fields, so `origin/HEAD` never serves as the upstream there; it
does not here either.

Text from outside (a transcript, a title, a path) is stripped of control
characters before it is drawn. It is not a text-entry screen: `esc` and `q`
close it, `↑↓` `j` `k` `PgUp` `PgDn` scroll, `?` lists the keys.
`tmux-review --info` stays until #228's cleanup removes it.

## The board as the review queue (#253)

`prefix r` opens the board, the same command `prefix w` runs, and
`@cockpit_review_window on` opens it in a window. The fzf fleet queue is gone;
`tmux-review --fleet` went with it. `r` on a row opens the review loop as before.

**Row cells.** The list draws, between the message and the age: the diff stat
(`+a -d`, `clean`, `no repo`, `?N new` for untracked files) and the branch
(`⑂name`). The review mark stays in the row's leading column. A cell that does
not fit is dropped with every cell to its right; the cells take at most a third
of the width the name and message share, and leave the name 6 cells and the
message 12, so a 40-column row may show none.

**Where the facts come from.** `data/fleet.rs`, on the fetch thread. One diff
scan per distinct repo root however many panes share it, at most 8 git processes
at once, `@cockpit_dir` over the pane's cwd (the tree the agent works in), and
the gitsafe flags. The list still makes no pane capture.

**Verbs.** `cockpit-board` gains one line, `kill %N` (#288 removed `request %N`).
`kill %N` kills the pane's window. The board writes the `#{window_id}` it
showed (`@9`, not the name: automatic-rename gives agents the same name) to
`$COCKPIT_ACTION.expect` first. The wrapper compares it with the window the pane
is in now and kills the window that id names; it refuses with a `✕` note if the
pane moved to another window, if the file is missing, empty, not an `@<digits>`
id or a symlink, or if the pane is gone. The `✓` note and the log carry the id
and `session:index` of what died.
The board key waits on the board's verb hook (not shipped).

## Sessions

<!-- sessions view (#245) -->

`cockpit-tui sessions [--needs] [--current $N]` is `tm`'s picker (#228, decision
6), built on `pick`. One `list-panes -a` read gives every session with its
windows; a fetch thread repeats it about every 2 s, so the glyphs move while the
list is open.

```
 session> ▏                                                                   1/5
 5 sessions · ▲1 ✕1 ●1
▎ 1 ▲ api                  1a 2w       45s  │ api · 2 windows
  2 ✕ ops                  1a 1w        5s  │ ▎0 ▲ claude needs you
  3 ● web                  1a 1w   ◉ 1m05s  │       Run rm -rf build/ before the
  4   docs                 - 2w      4m12s  │       tests?
  5 ✓ tmux-cockpit (here)  1a 1w            │  1   shell no agent
 ↵ switch   ^X kill   F2 rename   F1 keys   esc cancel
```

Each row is a jump digit, the worst agent state over the session's panes (`▲` needs
you, `✕` error, `●` running, `✓` done, `○` idle, blank for no agent), the name,
agent and window counts (`1 agent  2 wins` at 47 columns of list or more, `1a 2w`
down to 32, none below), `◉` when another client is attached, and the time since
the session was last used, in `data::fmt_dur`'s format (`45s`, `1m05s`, `4m12s`,
`1h03m`). A pane is an agent when its command is in `@cockpit_agents_glob`, the test
`agent-state` uses, so a stale `@panestate` on a shell is not believed.

**Order.** Waiting first, then failed, then the most recently used; the session
`--current` names goes last and is marked `(here)`. The order is read once and
frozen: a refresh updates states and ages, keeps every session where it was, drops
the ones that went and appends new ones, so a digit and the cursor keep their
session. `--needs` shows only the waiting and the failed, falling back to every
session (`nothing needs you`) when there are none; it takes no new sessions on
refresh. `--current` must be `^[$][0-9]+$`.

The preview (when the screen is 60 columns or wider) lists the windows with their
state, and wraps the question a waiting window asks. Narrower screens have no
preview.

| key | does |
|-----|------|
| letters, `⌫` `^U` | filter by name (a substring, case-insensitive) |
| `1`-`9` | switch to that row, only while the filter is empty |
| `↵` | switch to the selected session; with nothing matching, create the typed name |
| `alt-↵` | create the typed name even when a row matches |
| `^X` | arm a kill: the header names what dies (`kill api? 1 agent (1 waiting) · 2 windows`, with `attached elsewhere` first when it is). A second `^X` confirms; any other key, `esc` and a click keep the session and are spent. Your own session refuses |
| `F2` | rename: the prompt becomes `rename>` with the old name; `↵` confirms, `esc` goes back |
| click | select; click the selected row again to switch (two taps, so a stray one never switches) |
| `F1`, `esc` `^C` | the keys, cancel |

**The action contract.** The binary mutates nothing. It writes one line,
`<verb> <target>`, to `$COCKPIT_ACTION`, the target a tmux id (`$N`, checked
against `^[$][0-9]+$` by the caller, never a name) or `-`:

| line | side files | caller does |
|------|-----------|-------------|
| `switch $N` | none | `switch-client` / `attach-session` |
| `new-session -` | `.text`: the new name, already cleaned | create it in the active pane's directory, then switch |
| `kill-session $N` | `.expect`: the name that was shown | kill `$N` if its live name is still `.expect` |
| `rename-session $N` | `.text`: the new name; `.expect`: the old name | rename `$N` if its live name is still `.expect` |

Free-form text never rides on the action line. `.text` and `.expect` are one line,
mode 0600, created new with `O_NOFOLLOW` and written before the action line, like
the panel's files. With `COCKPIT_ACTION` unset only `switch` has anywhere to go
(its line is printed on stdout) and `^X`, `F2` and creating say
`✕ ... needs COCKPIT_ACTION` instead of doing anything: one contract, so a name
never has to be split out of a stdout line. Exit 0 with an action written, 1 for a
cancel (nothing written), 2 for a bad argument, no terminal or tmux, 3 when the
action could not be written.

`scripts/tm` re-checks all of it before it acts: the id against the pattern, the
name against `tmux display-message -p -t $N '#{session_name}'`, and refuses with a
`✕` note (on stderr, and on the client's status line when in a popup) if the
session is gone, was renamed between the print and the act (`session $3 is now
called 'x', not 'y'; nothing changed`), or is the one `tm` runs in. A kill or a
rename that goes through puts the picker back, in the same popup. `@cockpit_glyphs
ascii` reaches the picker as `COCKPIT_GLYPHS=ascii`; `nerd` is not a tier of the
Rust screens and draws the default.
