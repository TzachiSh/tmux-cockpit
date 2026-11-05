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

`make check` builds the binary first; the suite needs cargo. `make tui-test`
runs cargo's tests, clippy and fmt on the `rust-version` toolchain through
rustup (CI's, so the lints match); `make tui-test TUI_TOOLCHAIN=` skips that.

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
| `s` | open or close the selected agent's subagent tree; on a header, every tree in that session (one shut opens them all, else all shut). See [Subagents](#the-board) |
| `⇥` `⇧⇥` | next / previous agent, skipping headers, wrapping |
| `n` `p` | next / previous session header, wrapping |
| `1`-`9` | the Nth agent row on screen. Selects, does not jump; a row in a folded group unfolds it |
| `g` `G` | first / last row |
| `u` | the next `▲` agent, wrapping; unfolds its group |
| `[` `]` `PgUp` `PgDn` | scroll a screenful; the selection only moves to stay on screen |
| `↵` | jump to the selected pane, close the board; on a header, fold / unfold |
| `a` `r` `f` `i` `x` `o` | as in the tiles, on an agent; a header does not take them |
| `/` | find: open the field and filter as you type (see [Find](#the-board)) |
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
| `x` | kill the agent's pane, after a confirm (`kill %N`); the window's other panes stay. On an ended agent it closes the pane left behind |
| `o` | on an ended agent whose `@cockpit_session` is a session id: `claude --resume <id>` in a new pane beside it, at the pane's cwd (`resume %N`). Not offered anywhere else |
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
then `x kill`, and `o resume` on an ended agent that has a session id; the
cut drops those first. On a list
header: `space fold` or `space unfold`, `l open`, `- fold all`, `= open all`,
`n p sessions`, `/ find`. With a kept filter, `esc clear filter` comes before
`? keys`. In the find field the bar is the field's:
`type to filter   ↑↓ move   ⌫ edit   ^U clear   ↵ keep   esc clear`, `↑↓ move`
left out when nothing matches; as it narrows, `type to filter` goes first, then
`⌫ edit`, `^U clear` and `↑↓ move`, never `↵ keep` or `esc clear`. The footer, the `?` help and the floor card are the panel's
own, from `chrome`.

**Extra actions.** `EXTRAS` in `board.rs`, the `// #228:` registry, holds the
actions beyond the key table: a key, the footer's label, the `?` help row, the
verb, two optional functions of the selected `Pane`, `confirm` (the question)
and `expect` (the value for `.expect`), and `offered`, whether the agent gets
the key at all, with `not_here`, the note the key leaves where it does not.
Each entry is in the footer on an agent it is offered on (never a header), in
`?` under "act on the selected agent", and in the dispatch; its key may not be
one the table, D6, D7's `/` or D8's `a` takes. `x` → `kill`, with a confirm and
`%N @W` (the pane and its window's id) as `.expect`, is #228's, scoped to the
pane by #301; `o` → `resume`, with no confirm and the session id as `.expect`,
is #301's (#288 removed `R` → `request`). An entry with no `confirm` exits at once.

**The confirm.** An entry with a `confirm` opens one frame, the focus frame,
over the foot of the list or the tiles: the action and the target in its title,
`confirm` at its right, the question (`kill pane %12 (api:2 server)?`) and what
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
| `@cockpit_board_wake` | (state) | read-only: one `wait-for -S cockpit-board-<pid> ; ` per running board, the command that wakes it. Appended at start, removed on `q`; an entry a signal or a crash leaves is dropped by the next board's start |
| `@cockpit_panel_key` | `f` | binds the panel (`cockpit-tui panel %N` through `scripts/cockpit-panel`) to a prefix key. `off`, or an unset key with the old `@cockpit_fixer_key off`, leaves stock `find-window` on `f` |
| `@cockpit_panel_mode` | `auto` | `pc`, `tablet` or `auto`: how the panel is shown. `auto` is tablet under Termux (`TERMUX_VERSION` set, or `PREFIX` under `com.termux`), pc elsewhere. Read at load and handed to the popup as `COCKPIT_PANEL_MODE`; any other word shows a message and binds `auto` |
| `@cockpit_md_view` | `on` | `cockpit-tui view` shows markdown rendered, and code and other text files numbered, and highlighted when the language is known (#365). With `off`, a clicked file and the file browser's `↵` go to the editor instead, for every file (the browser's with #408). Read by `scripts/tmux-open-path` at the click |
| `@cockpit_panel_bound` | (state) | the key the last load bound the panel to. Every cockpit key keeps one (`@cockpit_<name>_bound`, `<name>` as in its `@cockpit_<name>_key`), so a reload that turns a key off or moves it gives the old one back to stock |
| `@cockpit_board_client` | (state) | a pane option: the client that pressed the board or review key there in its popup form, so the board's jump moves that client and not the most recently active one. Read and cleared when the board starts; the window forms pass `-c` instead |
| `@cockpit_branch_client` | (state) | a pane option: the client that pressed `prefix b` there, so the branch popup's jump to another worktree moves that client and not the most recently active one. Read and cleared when `tmux-branch` starts |
| `@cockpit_giveback` | (state) | `<name>:<key>` words: old keys a reload could not give back to stock (tmux's own binding could not be read), retried on the next load |

The popup is `-w 90%`, wider than the 70% the pickers use, because a 70% popup
on a 120-column terminal is an 84-column board, which is one tile column. The
height is whatever `@cockpit_popup_height` gives, like every other cockpit
popup.

`@cockpit_board_wake` is how the state hooks wake the board: after a
transition `agent-state` runs it with `run-shell -C`, in the tmux call it
already makes, and each board pulls its next tick forward. See "Push wakeups".

Read-only, and owned by bash: `@cockpit_session`, `@cockpit_dir`, `@panestate`,
`@panestate_msg`, `@cockpit_review`, `@cockpit_review_spool`, `@cockpit_title`,
`@thm_*`.

## Environment

| variable | does |
|----------|------|
| `COCKPIT_ACTION` | file the one action line is written to. Unset sends it to stdout |
| `COCKPIT_TMUX` | overrides the tmux argv. Unset follows `$TMUX` to whatever socket the pane is on |
| `COCKPIT_LOG` | append target. The binding sets it to `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/tui.log` |
| `COCKPIT_PROC_DIR` | the proc root the pane-tree walk reads (default `/proc`); a test seam. A root without `self/task` or without `children` files sends the lookup to `ps` |
| `COCKPIT_READONLY` | any non-empty value: never write to a tmux server, the pid included. The panel wrapper then pastes and sends nothing and logs what it would have done |
| `COCKPIT_GLYPHS` | `ascii` swaps the glyphs *and* the borders for the ambiguous-width fallback tier; the panel then prints nothing but ASCII, its key names included (`enter`, `tab`, `bksp`, `left`, `up/dn`). `nerd` draws the default, but for the subagent badge's nf-oct-workflow (U+F52E) in place of `✦`. The wrappers and the `prefix i` popup set it from `@cockpit_glyphs` (#293) |
| `COCKPIT_CLAUDE` | the panel's `claude` binary. Unset is `claude` on `PATH` |
| `COCKPIT_PANEL_TIMEOUT` | seconds before the panel kills its `claude -p` call. Default 120 |
| `COCKPIT_PANEL_MODEL` | forces the panel's model, over the rules and the model picked on screen |
| `COCKPIT_PANEL_MODE` | `auto`, `pc` or `tablet`, from `@cockpit_panel_mode`. Unset or empty is `auto`. Any other value: the panel exits 2 before it draws. The board's popup passes it too, so a panel opened from the board gets the same mode |
| `COCKPIT_PANEL_NOTE` | set by `scripts/cockpit-board` for `scripts/cockpit-panel`: the file the panel wrapper writes its last line to instead of a `display-message`, a failure starting with `✕`. Unset (`prefix f`), the message goes to the status line as before |
| `COCKPIT_PANEL_DROPPED` | set by `scripts/cockpit-panel` for the binary: the pane ids, space-separated, whose hidden panels it just dropped because the pane is gone. The binary shows them in an info note; anything but `%N` words is ignored |
| `COCKPIT_PANEL_CALL` | set to `1` in the panel's `claude -p` child, never read by the binary: a hook can skip the call by it |

The binding runs the binary under `env -u CLAUDECODE`, which is required inside
a popup.

`COCKPIT_READONLY=1` exists because a probe run once left a dead board pid
(the wakeup before #224) on the owner's live server. Use it for screenshots and
benchmarks.

**The panel's prompts.** A file at
`${XDG_CONFIG_HOME:-$HOME/.config}/tmux-cockpit/prompts/<name>.txt`, with
`<name>` one of `write`, `write.examples`, `explain` or `fix`, replaces the
shipped prompt of that name; the `{{platform}}`, `{{home}}`, `{{about_user}}`
and `{{examples}}` placeholders are filled in it the same way. A missing file
means the shipped prompt. A file that exists but cannot be read (permissions,
invalid UTF-8) also falls back to the shipped prompt, and the reason goes to
`tui.log`.

**The write prompt's evaluation.** `cargo run --example panel_eval` (in `tui/`)
runs the labelled cases in `tui/evals/panel/cases.txt` through the panel's own
prompt, envelope, model route, `claude -p` call and reply parser. It prints, per
case and in total, whether the panel asked when a case needs a question, asked
when it did not, and carried the facts a case requires, plus the latency. It
calls `claude` once per case, so it is run by hand and never from `make test`;
`--check` only validates the cases, and `cargo test` runs that check too. To
compare a prompt edit, put it in the override file above and run again: the
summary prints a hash of the system prompt it sent, and a pass count per case
over the runs (3 by default: one run is not enough to judge a change). It also
prints asking as FPR (asked on a write case) and FNR (no question on an ask
case), with either cases in neither; `pass^k`, the share of cases that passed
all k runs, with its Wilson 95% interval, since cases and not runs are the
independent trials; and the cost of the calls, from `claude -p --output-format
json`. `--compare A.jsonl B.jsonl` reads two `--out` files and makes no calls:
per case each side's pass count, the cases whose pass rate moved, `pass^k` for
both sides over the cases both graded in full, and each side's own (unpaired)
`pass^k` and cost. The case format, with its `expect: ask|write|either`,
`must`, `must_not`, `out_is` and `holdout` keys, is described at the top of
the cases file; holdout cases are written after the prompt they run on, so a
gain there is not overfitting. `--model`, `--runs`, `--jobs`, `--only <id or
kind>` and `--out <file.jsonl>` are the knobs.

**The panel's cache.** `claude -p` reads its system prompt from a file, so each
distinct prompt is written once to
`${XDG_CACHE_HOME:-$HOME/.cache}/tmux-cockpit/panel/prompts/<hash>.txt`, mode
0600 in a 0700 directory. `tmux-cockpit/panel` is also the call's working
directory. Nothing prunes it; a new file appears only when a prompt's text
changes.

## The screens

### The board

The list alone, as a board under 96 columns draws it, shown here 100 columns
wide so every cell fits: 13 agents in 4 sessions, the first `▲` selected. At 96
columns and over the list shares the body with the [right pane](#the-right-pane).

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
ranking. Find also reads each agent's subagents' types and descriptions, so
`explore` finds the agents running an Explore subagent. A session with no match hides; one with a match shows unfolded while
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

**Background jobs (#448).** A pane that shows a background job (the agent view,
`claude attach`) takes its state from the job, not from hooks: `scripts/agent-poll`
binds it when the pane's title names exactly one job, and records the job on the pane
as `@cockpit_job` (id) and `@cockpit_job_name` (name), both cleared when the bind
ends. The board does not draw them yet (#460). A row's state is the bound job's, and the
rule, its residual false bind and how the poller starts are in
[states-and-hooks.md](states-and-hooks.md#agent-view-and-background-jobs).

**Subagents (#313).** An agent with subagents running or failed carries a
badge, the first of its row's extra cells, in two widths by the room the row
has: `✦2·1✕` (running, failed; the failed count only when there is one), then
their total, `✦3`, which takes 3 cells at 40 columns and leaves the name its 6
and the message its 12. Done subagents never count: an agent whose subagents
are all done shows no badge, and its tree still lists them. The badge is in
the error role while a subagent has failed, else the running role. `&` is the
badge in the ascii tier.

Running is the subagents whose `subagents/` files say they run. With no files
to read, it is the hook's `@cockpit_subs`, the background subagents in flight
at the agent's last Stop. An ended agent (#301) runs no subagent, whatever its
files say: its badge counts none running, shows only a failure, in the meta
role and dim, and its tree draws its running and idle subagents with `◌` and
no age.

A subagent's end is its `finished_at` (#338), the newest row of its transcript.
Without one, the board keeps its own: it stamps each subagent when a read first
sees it failed or done. A failure that ended before the agent's newest prompt
is an earlier turn's: it leaves the badge and the
tree at the same moment, so the two always agree. A background subagent that
started last turn and failed this one stays, while the board stays open: each
popup is a new process and starts with no memory (#338). The newest prompt is a human one;
a subagent's `<task-notification>` report-back does not count. Once the prompt
is out of the transcript's tail the read has none, and the board keeps the
newest it saw. On the first read of a pane, when the board opens, a subagent
already failed or done is stamped old unless it started after the newest
prompt.

`s` opens the selected agent's tree under its row; on a header, every tree in
the session. Trees never open on their own, and which are open is kept in
memory only, not in `board.state`, and a tree whose subagents all go (`/clear`)
shuts, so the next session's do not open it. The footer offers `s tree`, or
`s hide` when the tree is open, only where the tree gets a line: not in a
folded group, nor in a list too long to leave a row spare. There `s` opens
nothing and says `no room for its subagent tree` for 2 seconds:

```
│  ▾ api ────────────────────────────────────────────────────────────────────────────────── ▲1 ●2  │
│▎ 1 ▲  0 server             Do you want to run npm test?                            no repo 4m12s │
│  2 ●  1 tests           Bash: cargo test --workspace                    ✦2·1✕ +42 -7 ⑂main 1m03s │
│     ├ ✓ 2 done                                                                                   │
│     ├ ● Explore         find auth callers in src/auth                                12.4k 2m12s │
│     ├ ✓ Explore         map callers                                                     9k 1m35s │
│     ├ ✕ general-purpose fix lint in tui/src                                            842 1m08s │
│     └ ● Plan            draft the migration plan                                      3.1k   45s │
│  3 ●  3 migrate           Edit src/db/migrate.rs                                ✦1 no repo 1m03s │
```

A tree line is `├` (`|-` in ascii), the subagent's state glyph, its type, its
description, its tokens (the context of its last request: `842`, `12.4k`,
`3.1M`) and its age since it started. As the row narrows the tokens go first,
then the description shortens to 8 cells, then the type to 6; the age stays.
A done subagent folds into one `✓ N done` line, first in the tree, 2 minutes
after it ended. Each state has its mark (#361):

| state | mark (ascii) | role | age from | folds |
|-------|--------------|------|----------|-------|
| running | `●` (`*`) | running | start | never |
| failed | `✕` (`x`) | error | start | leaves at the next prompt |
| done | `✓` (`+`) | done | start | into `✓ N done`, 2 min after its end |
| stopped by the user | `∎` (`#`), dim | meta | start | as done |
| idle, a teammate between turns | `◦` (`o`) | meta | last sign of life | never: it is alive |
| end unknown | `◌` (`~`) | meta | last sign of life | at once, into `◌ N quiet` |

Only running and failed count in the badge. End unknown is 15 minutes with no
transcript write and no recorded end: a TaskStop mid-turn, or a live agent
waiting on a permission prompt or a long call. It is never red, and as it is
past the 2 minutes by its definition it folds at once, into a line of its own
whose age is the newest sign of life among them. One long session held 16 done,
51 end unknown and 3 idle subagents: `✓ 16 done`, `◌ 51 quiet` and the three
teammates. `∎` and `◦` are East Asian Width N, one cell everywhere; `■` is A.
A tree takes at most 5 lines, the last reading `+N more` when it has more.

Tree lines take only the rows the list leaves empty, and never more than half
of them, so they never push a row off screen and never make the list scroll:
`N-M of T` counts them, but a list that does not fit has no room for any. The
selected agent's tree gets rows first, then the blocked agents', then the rest
in board order; an agent that gets none keeps its badge. A folded group's trees
stay shut, only the badges show.

No key selects a tree line. `↑` `↓` step over a tree, `⇥`, the digits, `u`,
`n` `p`, `g` `G`, the page keys and find's moves land on headers and agents
only, so `↵` `a` `r` `f` `i` `x` `o` act on the agent the tree hangs from. The
top of the list is never a tree line: the viewport moves up to the agent, or
down past its tree when the selection would leave the screen.

### The right pane

At 96 columns and over (D1) the list takes the left of the body,
`clamp(w/3 + 8, 40, 64)` wide: 40 at 96, 41 at 100, 48 at 120 and 64 at 180,
which leaves the pane 116. The selected agent fills the right, at 100 columns:

```
 board · 6 agents · 3 sessions · 2 needs you · 1 error                                              
┌ sessions ─────────────────────────────┐┏ ▲◆ api:0 · server ━━━━━━━━━━━━━━━━━━━ needs you · 4m12s ┓
│  ▾ api ─────────────────────── ▲1 ●2  │┃ /src/api · ⑂feat/rate-limit · %3   ◆ review pending     ┃
│▎ 1 ▲◆ 0 server Do you want to … 4m12s │┃ asked  add a token-bucket rate limiter to the router    ┃
│  2 ●  1 tests  Bash: ca…rkspace 1m03s◂│┃ said   I added the token-bucket limiter and wired it i… ┃
│  3 ●  3 migra… Edit src…rate.rs   22s │┃ needs  Do you want to run npm test?                     ┃
│  ▾ infra ──────────────────────── ✕1  │┃                                                         ┃
│  4 ✕  0 terra… terraform plan:… 6m02s │┃ ── live · 13 lines ──────────────────────────────────── ┃
│  ▾ web ─────────────────────── ▲1 ◌1  │┃ ⏺ I added the token-bucket limiter and wired it into t› ┃
│  5 ▲  2 docs   plan ready: app…   38s │┃                                                         ┃
│  6 ◌  0 ui     ended                  │┃ ⏺ Bash(npm test -- --runInBand)                         ┃
│                                       │┃                                                         ┃
│                                       │┃  Bash command                                           ┃
│                                       │┃                                                         ┃
│                                       │┃    npm test -- --runInBand                              ┃
│                                       │┃    Run the API test suite serially                      ┃
│                                       │┃                                                         ┃
│                                       │┃  Do you want to proceed?                                ┃
│                                       │┃  ❯ 1. Yes                                               ┃
│                                       │┃    2. Yes, and don't ask again for npm test commands i› ┃
│                                       │┃    3. No, and tell Claude what to do differently (esc)  ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ↵ jump   a answer   r review   f panel   i info   space fold   / find   x kill   ? keys   q close  
```

The frame is thick, in the needs or error role on a `▲` or `✕` agent and the
accent otherwise. Its title is the row's glyph and review mark, `session:window`
and the title; its right end the state, `needs you` on a `▲`, `ended` on an
ended agent, and the age, which an idle agent does not have. Then five rows:

- where it runs: the path, `⑂branch` and the pane id, then `◆ review pending`
  or `↵ review sent`. The path is cut in its middle and goes first.
- `asked`, the newest human prompt; `said`, the agent's last text; `doing`, its
  tool. On a `▲` the third row is `needs` and the question, in the needs role;
  on a `✕`, `error` and the error; on an ended agent, `doing ended`. Nothing to
  show reads `—`.
- a blank row, then the rule `── live · N lines ──`.

The rest is the pane's screen: its visible screen (`capture-pane -p -e` with
no `-S`, so the blank rows below the cursor keep their places) and its cursor
(`#{cursor_flag} #{cursor_y}`), read by one tmux command, so one fork. ANSI is
kept, nothing wraps, and it is capped at 64 KB. The used part of the screen
runs to the cursor's row or the last row with text, whichever is lower; rows
past it are never content, and a row of colour only counts as blank. A used
part that fits the pane is drawn whole from the top, as the pane itself looks
(a cleared terminal, a short shell). A taller one:

- with the cursor shown (vim, less, a shell): tmux's third-down rule, clamped
  to the used rows. tmux's `choose-tree` preview (`screen_write_preview` in
  `screen-write.c`) puts the view's top at `cursor_y - rows / 3`, so the cursor
  sits a third of the way down, and clamps it to the screen's height; here it
  is clamped to `[0, used - rows]`. tmux's horizontal pan is not taken: a long
  line is clipped instead.
- with the cursor hidden (Claude Code, htop): the view ends at the last row
  with text, which keeps Claude Code's input box and the newest output in view.

When the 64 KB cap takes rows off a view's top, they come back blank, so the
newest row stays where it was.

A tab moves to the next stop of 8 columns, as in the terminal; any other
control character takes no cell. OSC sequences (titles, hyperlinks) are
dropped before the colours are read, as the parser loses the rest of a line
after a link that ends in `ESC \`; and when the 64 KB cap cuts inside a line,
the screen starts at the next one, so no escape is cut in half (one line
longer than the cap keeps the cut). Both apply to the tiles' previews too.
A line cut at the pane's edge ends in `›` (`>` in ascii), measured per
grapheme as it is drawn; blanks a TUI pads its lines with are not a cut. A
wide TUI still shows its left part only, marked.
A done, idle or ended agent's screen is dim, as its tile is. A capture that
failed says `capture failed` in the error role, which is a gone pane; a blank
screen is `live · blank`; and before the first capture of a newly selected
agent, `reading the screen …`.

On a header, or under a find with no match, the pane is a plain frame titled
`no agent selected` that reads `the agent you pick shows here.`

Only the selection is captured: one `capture-pane` per fetch, on the fetch
thread, never at 95 columns or under and never on a header. A new selection
asks the worker at once rather than waiting for the next second.

The list column draws what a board of its width would draw: a subagent tree
(#313), an ended row (#301) and a worktree cell (#312) narrow by the list's
width, not the board's. Under 96 columns the board is the list alone, byte for
byte what it was.

Not built here: the panel docked in the right pane at 164 columns and over
(D10); `f` still opens it full-area. The tiles' previews keep their own rules:
top-aligned, and a cut line unmarked.

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

### Code in the panel (#282)

Code in the Result, Steps and Answer views is coloured by a small built-in
lexer (`highlight`, a top-level module since #283), not a grammar crate: syntect, two-face and
tree-sitter were turned down for the binary's size.

- **Where.** A fenced block (three or more backticks, an optional language
  tag read up to a `,` or a space, so `rust,ignore` and `{.python}` both work)
  in a Result's text or an answer; a step's command, by its `lang`. Prose
  outside a fence is never lexed, so an `if` in a sentence stays text. A fence
  left open runs to the end of the text; a shorter fence inside is content.
- **An answer that is one block.** A reply fenced whole is read from inside
  its fence: the first line opens it, the last line is a bare closer of at
  least as many backticks, and a tagged block inside is paired with its own
  closer, so a wrapped markdown answer may hold code blocks. An Answer is
  only shown, never typed: when the fence names a
  language the lexer knows, the answer keeps it and is coloured. A bare or
  `xml` fence is a wrapper and still goes, as does any fence in Fix and Write,
  whose text is typed into the pane.
- **Languages.** Shell (`sh` `bash` `zsh` `console` and kin), JSON, Rust,
  Python, JS/TS. Any other tag, or none, is plain, except a bare fence whose
  lines all start with `$ `, which is read as shell.
- **Colours.** Existing roles only, no new `@thm_*` option: keyword `accent`,
  string `done`, number `running`, comment and the fence line `meta`.
  Everything else keeps the view's `value`.
- **NO_COLOR and ascii.** Every role is `Reset`, so the block draws exactly as
  plain text, with no modifier standing in. The lexer prints no glyphs, so the
  ascii tier draws the same characters.
- **Text and widths never move.** The lexer labels slices of each line, and the
  view lays the colours onto the rows the plain wrap already made, so a row,
  a width and a scroll count are what they were without colour.
- **Line-based and bounded.** One forward pass per line, linear in its length;
  what spans lines (a block comment, Rust's nested, a multi-line string, a
  heredoc body) is carried to the next. No input panics it: it is fuzzed with
  random bytes and random syntax mixes.

### Explore first (#416)

`^R` on Compose lets the next write read files in the pane's repo before it
writes, so the prompt can name the real files and functions. It is off when the
panel opens, stays as set until the panel closes, and `^Z` keeps it. The head
says `explore: on` (on a line of its own when narrow), Busy says `reading the
folder, then writing`, and tablet mode has an `[ explore ]` button.

- **The call.** `App::profile_for` is the one place a call's profile is
  picked: `Profile::TEXT`, or for an exploring write
  `Profile::read_only(<folder>, false, 0.15, 90 s)`. That is Read, Grep
  and Glob only, the folder by `--add-dir` and the cwd the cache dir, reads
  stopped at the folder (`dontAsk`, #465), no hooks, `--strict-mcp-config`,
  and json output. Fix and Explain never read.
- **The folder is the repo.** It is the git toplevel of the pane's folder
  (`capture::toplevel`: `rev-parse --show-toplevel` under gitsafe's flags, in
  the repo block's 300 ms), so a pane in `repo/src` explores `repo`. Outside a
  repo, or past that time, it is the pane's own folder. A repo's own config
  can name any folder as its tree (`core.worktree`, relative or absolute), and
  an inherited `GIT_DIR` or `GIT_WORK_TREE` can point git at another repo, so
  the root is checked: `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR` and
  `GIT_INDEX_FILE` are taken out of that git call's environment (a ceiling
  stays: it only stops the search lower). Then the root is kept only if it
  holds the pane's folder and its own `.git`, a folder or a `gitdir:` file, is
  the repo's git dir. Otherwise it is the pane's folder. A linked worktree, a
  submodule and a separate git dir pass. It is worked out once per panel, and
  the `explore on` note names it.
- **Only a local window.** `^R` is refused, with a warn note, when the window
  may be a shell on another machine: its command is not in `LOCAL_CMDS` and not
  an agent name (`capture::is_local`, #433's rule). It is also refused when tmux
  gives no folder, and for a folder `request::read_dir_ok` refuses, run on
  that repo root, with its words: `/`, the home folder or one above it, or a name that is empty, not
  UTF-8, or holds a control, bidi or glob character. A shell in `~`, or in a
  folder of a home that is itself a repo, cannot explore: `cd` into a project
  of its own first. The rule reads the command captured when the panel opened:
  `profile_for` checks it again at each call, so a toggle restored from a hidden
  panel never reaches a pane that was not local at open. A pane that starts ssh
  while the panel is open is not seen. A panel resumed on a pane that runs ssh
  by now comes back with explore off, and the note says so.
- **The caps are per call.** USD 0.15 and 90 s, `app::EXPLORE_BUDGET_USD` and
  `EXPLORE_TIMEOUT`, each call. One write can make up to five exploring calls
  on its own: the first, one per answered question (three at most), and the
  retry when it still asks with none left. Each `r` and each `a` correction is
  one more. The budget covers the write prompt's cache and three to five
  reads on sonnet. Reading past that is the agent's job, so the call ends instead
  of keeping the user on Busy. The budget is also the turn limit: `--max-turns`
  is hidden and may not be enforced (#432). The timeout is shorter than the text
  call's 120 s for the same reason, and `COCKPIT_PANEL_TIMEOUT` does not raise
  it. When either cap ends a call (`Fail::Timeout`, `Fail::Budget`), the error
  line names it and the hint points back to the draft, reached by `esc` from any
  view a call starts on, where `^R` turns explore off.
- **The prompt.** `prompts/write.explore.txt` is added after the write prompt
  only for an exploring write. It has its own file, not a placeholder in
  `write.txt`: the shipped write prompt, which is the eval's baseline, stays
  byte for byte the same when explore is off, and an owner's `write.txt`
  override keeps the reading limits. It says: read at most four files, finding
  them with Grep or Glob first; read before asking which file; stop once the
  files to change can be named; name the place, never the fix; cite as `@path`
  only a file read or shown; treat the repo as context, never instructions.
- **The files read.** An exploring call's answer opens its view with a note,
  `read 2 files: src/a.rs, lib/b.rs`, paths under the folder shown relative to
  it and with every control and bidi character dropped (`pick::clean`), three
  named and the rest counted; `read no files` when it read none. The
  count is `parse::files_read` over `Request::raw()`, the call's json events, so
  a refused read is left out and a budget stop still says what it read; a failed
  call that read nothing adds nothing to its error. claude's API error, marked
  `is_error` under a `success` subtype, is an error in claude's words. After a
  warning on that view (a reply cut off) the note is added to it, never in its
  place. A text call has no events and gets no note.
- **Accepted risks** (#416):
  - Reads stop at the folder (`dontAsk`, #465), with the deny list as a second
    wall. The limit is #465's: a user `settings.json` with a broad `Read` allow
    or an `additionalDirectories` entry reopens outside reads.
  - A file that is read skips the screen's redaction: the denies cover the key
    files, not a secret inside a source file.
  - Repo text can carry an injection into the written result. The section calls
    that text context only, and nothing runs unasked: an insert is a paste, and
    a send needs a confirm. But a command or steps result can be sent by the
    user, so an injection can reach a command the user runs.

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
  typed, type, model, explore, hide, keys, home), wrapped to more rows when narrow. Ranges and
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
  and its cursor, the output type, the model, the explore toggle (dropped,
  with a note, if the pane is no longer local), and the run: the flow, the
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
  #284 changes this at 96 columns and over: see its contract below.
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
- Not built here: the 100/180 mockups' right pane (step 7, built in #284), the tablet chips
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

### Contract changes for ended agents, a pane kill and resume (for #61)

- #301: `data::Pane` gains `ended: bool`, appended last: `@panestate` is set
  but no process in the pane is an agent (`agentish` on the command and the
  process tree, the test the fleet read already made). It is never set while
  `@cockpit_agents_glob` is empty, as a live `claude` cannot be told from a
  shell then. `data::ended(raw, live, glob_known)` is new. Nothing is written
  to tmux for it; `--json` and `--rows` do not carry it.
- An ended row draws dimmed (name and text in the dim role plus `DIM`, off the
  selection band), `◌` (ascii `~`) for its glyph and `ended` for its text, never
  `▲`. It is out of "needs you", the hot band, a fold's kept rows, `u`, the
  open selection and `a answer`; it sorts after the live rows of its group,
  and a header counts it under `◌`. Find reads `ended` as its state word. The
  tiles draw it the same way. `tm --needs` already skipped it: the sessions
  view believes `@panestate` only on a pane whose command is an agent.
- The fleet read evaluates `agentish` for every row now, not only for rows
  with no `@panestate`, so the first fetch on a thread also reads
  `@cockpit_agents_glob` (one `show-option`, once per thread).
- A pane with `@panestate` whose command is no agent is looked up in the
  process tree, which is one `ps -axww` for the fetch. Every ended row is such a
  pane, so the verdict is kept per (pane id, `#{pane_pid}`, command) for
  `GIT_TTL` (10 s): an ended row costs one `ps` per 10 s, not one per tick. A
  new command or process in the pane asks again at once; an agent that exits
  under an unchanged foreground (an editor) reads ended up to 10 s late. A pane
  with no `@panestate` keeps the uncached tree test it had.
- When a pane's `ended` turns, `tui.log` gets one line,
  `fleet ended-flip pane=%N ended=true|false live_by=command|tree|none cmd=… pid=…`,
  naming what found the agent, so a false "ended" can be traced.
- `x` kills the pane, not the window: the confirm reads `kill pane %N
  (<sess:idx> <window name>)?`, `.expect` is `%N @W`, and the wrapper runs
  `kill-pane` inside the `if-shell -F` that checks the window id. Its `✓` note
  is `killed pane %N (<sess:idx> <name>)`. An older binary's bare `@W` is
  refused, and an older wrapper refuses this binary's `%N @W`, so neither skew
  kills more or less than its confirm said.
- New key `o` → verb `resume`, no confirm, `.expect` the session id; offered
  only on an ended agent whose `@cockpit_session` is a lowercase UUID
  (`board::{resumable, is_session_id}`). Pressed elsewhere it leaves a 2 s note.
  `Extra` gains `offered` and `not_here`; `board::hints_on` lists the extras
  one agent is offered, and `board::hints` is it on a live agent.
- `scripts/cockpit-board` gains `resume)`: it splits the target's window with
  `claude --resume '<id>'` (quoted with `sq`) at the target's
  `#{pane_current_path}`, never `@cockpit_dir`: Claude Code never moves its
  process's cwd (#312), so the pane's cwd is the project the session started
  in, where `--resume` looks it up. The directory goes through `fesc`, as `-c`
  expands formats; then the new pane is selected and the board ends. It
  refuses with a `✕` note a pane that is gone, an id that is not
  a lowercase UUID, an id the pane no longer holds, or no directory; and a
  second copy, `✕ cockpit: <first 8 of the id> is already running in %N`, when
  any pane, the target included, holds the id and runs an agent (its command
  in `@cockpit_agents_glob`, or version-shaped), with or without `@panestate`
  (SessionStart and idle clear it), read with one `list-panes -a` at the key
  so a stale board cannot skip it. An ended
  pane holding it does not block. The wrapper reads the command only, not the
  process tree the board reads: a live agent under a foreground editor is not
  seen there.
- The `?` legend gains `◌ ended: its agent is gone`.
- Goldens: the confirm's question in `board-confirm-{60,100}`; the `x` and `o`
  rows and the legend in `board-help-{60,100,180,tiles-100}`. No list or tile
  golden moved: their fixtures hold no ended agent, and a live agent's footer
  is unchanged.

### Contract changes for the right pane (#284, for #61)

- Design step 7's right pane is built; its docked panel (D10) is not.
- At 96 columns and over, the list draws [the right pane](#the-right-pane);
  under 96, nothing changes on screen.
- The list captures again: the selected agent's pane only, one capture per
  fetch, `board::pane_rows(area)` rows (the board's height less ten), through
  `data::preview` and its 64 KB cap. `board::Want` gains `pick: Option<String>`
  and is public, with `board::captures(&[Pane], ok, &Want)`, the worker's
  choice of what to capture. A pane that went, a header, or a board under 96
  columns captures nothing.
- `board::{SPLIT_W, list_w, pane_rows, captures, pick_of, repick, shown,
  forget, read_gate}` are new (`read_gate`: the redraw gate knows whether the
  pick's first capture has come, so a blank one still replaces `reading`): the last four are the key loop's pick, asked for once per
  new selection, the screens of other agents forgotten on a new pick.
- `data::preview` drops OSC sequences and starts a capped capture at a line.
- The pane's capture is `data::screen(id, rows)`: `capture-pane -p -e` with no
  `-S` and no `-J`, then `display-message` for the cursor, in one tmux call.
  `data::{view, split_cursor}` are its pure halves. The tiles keep `preview`. `View` is unchanged:
  a `pvs` with no entry for the selection is "not read yet", as in the tiles.
- Goldens: new `board-preview-{100,180}`, from the 100/180 mockups. Moved, the
  right pane now beside the list: `board-list-{100,180}`,
  `board-folded-{100,180}`, `board-find-{100,180}`, `board-find-none-100`,
  `board-confirm-100`, `board-extra-120` and `board-subs-100`. No golden under
  96 columns moved, and no tiles golden.
- Six tests that read a full-width list row at 100 columns read it at 95: the
  three that find the ended row through `list_line` in `board.rs`, and in
  `board_subs.rs` the badge (now `the_badge_on_the_row_at_40_60_and_95`), the
  narrowing tree line and the token check in `subagent_goldens`.

### Contract changes for the shared highlighter (for #61)

- #283: `panel::highlight` moves to a top-level `highlight` module, unchanged,
  so the markdown viewer can colour code without reaching into the panel. The
  module table gains its row; `panel` loses the mention.
- `highlight::{split, Part, Fence}` are new: lines in, prose ranges and fenced
  blocks out (tag, opener line, body range, closer line or `None` for a fence
  left open). `highlight::fenced` is built on it, and so are the reply
  parser's `unfence` and `code_fence` go through `highlight::wrapper`, on the
  same opener and closer lines: a reply is wrapped when its first line opens
  a fence and its last line closes it, every tagged opener between them
  paired with a bare closer first.
- That moves three edges of the wrapper rule, which used to want a tag of
  letters only, exactly three backticks and a reply that merely starts and
  ends with them: a tag such as `python3`, `text,ignore` or ` text` and a
  four-backtick fence now wrap a reply, as do an indented, a CRLF and a
  longer closing fence; and two blocks with prose between them are no longer
  read as one block around the prose. A wrapper holding a paired inner block
  of the same length is unwrapped as before, and a fence with nothing inside
  still wraps nothing.
- `highlight::block(tag, body)` is new: a block body lexed by its tag, with
  the bare-fence `$ ` rule. `highlight::tag(info)` is public: the tag in an
  opener's info string.

### Contract changes for the markdown viewer (for #61)

- #283: a new screen, `cockpit-tui view [--line N] [--] <file>`, and a new
  module, `mdview` (with `mdview::render`). It exits 0, printing `edit <line>`
  on stdout after `e` and nothing after a close, and 2 for a bad argument or
  no terminal. A binary from before it says `unknown screen view`, which
  `tui_stale` already reads as stale.
- A new dependency, `pulldown-cmark =0.13.4` with default features off, which
  brings `unicase` 2.10.0; `bitflags` and `memchr` were in the tree. The
  release binary grows from 1,727,536 to 2,100,968 bytes, about 320 KB of it
  the parser.
- New option `@cockpit_md_view` (`on`). `scripts/tmux-open-path` gains
  `md_view` and its call at the top of `open_one`, and a `--view <pane>
  <file> <line>` re-entry for the viewer's popup. The popup's `-T` title is
  the name with control and bidi characters dropped, then `fesc`. With no
  client on the pane's session the editor opens instead, decided before the
  popup: once open, its status is its command's, and a detach is 129.
- `panel::view::role_style` is `pub(crate)`: the viewer colours code with the
  panel's roles.
- Goldens: new `view-{40,80,120}`, `view-ascii-80`, `view-nocolor-80`,
  `view-find-80`, `view-note-60`, `view-help-60`, `view-empty-60` and
  `view-floor`. No other golden moved.

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
| 10 | `alt-c` on a row, only with `--copy`; its key is on stdout |

`--copy` (open-at-line, #228) adds `alt-c` and its footer hint; without it `alt-c`
does nothing. A `cockpit-tui` that predates the flag exits 2 with `pick: unknown
argument --copy`, which `tui_stale` (`scripts/lib/tui.sh`) reads as a stale binary.

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
id, wher, name, cmd, state, title, msg, sess, path, dir, root, spool, pinned,
delivered, pid, review, doing, said, asked, loc, pm,
since: Option<u64>, asked_since: Option<u64>, idle_since: Option<u64>
```

Derives `Default, Clone, Hash, PartialEq`. The durations are `u64` and not
`Duration` because of `Hash`.

**`asked` and `asked_since` (#337):** the newest human prompt of the session and its
age. The reader finds it in the last 256 KB of the transcript. A turn that writes
more pushes the prompt out of that window, so the reader keeps the prompt it last
saw per transcript path, for as long as its thread lives, and gives it back when
the window has none. It drops the memory when the file is shorter than at the last
read or is another file (device and inode), and a prompt in the window replaces
it. After a gap it reads back to the last read, up to 2 MB, so a prompt written and
buried between two reads is not missed; a gap over 2 MB drops the memory, since a
newer prompt may sit in it. A prompt that carries an image is an array row, and
its text block counts as the prompt. The board's worker polls, so it keeps the
anchor for the whole turn. A one-shot `--json` or `--rows` reads each transcript
once: a prompt already past the window reads `""` and `null`, as before.

**`dir` (#452):** the tree the git facts came from: `@cockpit_dir`, else the
session transcript's newest main-thread `cwd`, else `path`. The first that is an
absolute path naming a directory wins; `path` stays the pane's launch cwd. The
reader keeps the last cwd it saw per transcript path, for the same file only
(device and inode, and not shorter than at the last read), so a stretch of rows
without one does not lose it.

**`--json`**: an array of objects with exactly these 15 keys, in this order:

```
id, where, state, title, doing, said, asked, msg, since, loc, diff, review,
session, path, dir
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
kill %N · resume %N                                     (the board's EXTRAS, #228's arm, #301)
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
pane's id and `#{window_id}` (`%12 @9`) when the confirm opened, not its name:
agent windows tend to share one (`claude`), and a pane moved into a namesake
would pass a name check. The wrapper reads the pane's `#{window_id}` again and
refuses with a `✕` note when it differs. For `resume` it is the session id the
row showed; the wrapper resumes only a lowercase UUID (`8-4-4-4-12` hex) that
the pane's `@cockpit_session` still holds. A pane with no `@N` window id is
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
--rows | --json | --version | -h | --help`. There is no `fleet`, `info` or `ask` subcommand: the
board reaches those through the action file and the shell scripts.

**The transcript reader (#199):** `data::transcript` (`file`, `doing`, `tail`) is a library
module the board and the info screen read in-process; there is no `transcript`
subcommand and no shell shim. It is byte-identical to the jq reader it replaced
(Freeze 1 on #54) with two #193 changes: a row whose `message` is not an object is
skipped where jq failed the whole read, and
`<bash-input>`, `<bash-stdout>` and `<bash-stderr>` prompts are dropped like the
other harness tags. It also departs from jq in three declared places:
- a content item that would raise a jq runtime error is skipped, not fatal;
- JSON only jq accepts (NaN, leading zeros, nesting deeper than 128) skips the line;
- `file` sorts project directories in byte order, not the locale's.

Every failure is `None`.

**Helper subcommands (#199):** the git helpers keep the exit-code rule below:

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
  read only because of its `request::Profile`: the panel's calls run with
  `--tools ""` (an explore-first write, #416, with the read-only profile), and a
  read-only profile (#365) has only Read, Grep and Glob (and,
  for research, WebSearch, WebFetch and skills), each pre-approved, with Bash,
  Edit, Write, NotebookEdit and Agent denied by rule as well and every permission
  prompt refused. A tool that writes or runs a command breaks this rule, and so
  does a read-only fork that runs with a pane's folder as its cwd or setting
  source: the repo's settings, hooks and env would load (a hook runs a command,
  `env` can send the request and the key elsewhere, `apiKeyHelper` runs). The
  folder goes in by `--add-dir`, which loads its skills and nothing else, and
  the cwd stays the panel's cache dir. A read-only fork also runs no hook of the
  user's or a plugin's (`disableAllHooks`). Its reads stop at that folder: the
  permission mode is pinned to `dontAsk`, under which the `--add-dir` folder is
  readable and every other read is refused, a symlink that leads out included,
  so a user's `defaultMode` cannot approve the rest. No Read, Grep or Glob allow
  rule is passed (a bare Grep or Glob one approved outside reads). One check,
  `request::read_dir_ok`, refuses a read dir that is `/`, home or above it, or
  whose name is empty, not UTF-8, or holds a control, bidi or glob character;
  a caller runs it to refuse up front. The secret denies stay as a second wall.
  The limit: rules merge, so a user `settings.json` with a broad `Read` allow
  or an `additionalDirectories` entry reopens outside reads.
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
| `push` | `@cockpit_board_wake`, the `tmux wait-for` waiter, the exit-path cleanup |
| `info` | the pane info screen (`cockpit-tui info %N`, the board's `i` overlay) and `data::info`, its facts: git root, review base, the transcript rows |
| `mdview` | the `cockpit-tui view` viewer (#283, #365): loading with its cap, the find, the keys and its screen on `pick`'s /dev/tty; `mdview::render` lays markdown out as rows, `mdview::code` any other text file as numbered highlighted rows, both pure |
| `pick` | the `cockpit-tui pick` list: row parsing, the case-insensitive filter, selection, click and wheel, and its screen; draws on /dev/tty |
| `files` | the `cockpit-tui files <dir>` browser (#363) on `pick`'s list and screen: one folder, folders first, `↵` in or view, `alt-e` edit, `←` up, `.` hidden, `alt-i` ignored, `alt-c` copy; prints `view <path>`, `open <path>` or `copy <path>` |
| `sessions` | the `cockpit-tui sessions` view (#245) on `pick`'s screen; `data::sessions` the one `list-panes -a` read, the order and the freeze |
| `panel` | the type-panel port: capture, redaction, prompt envelope, the `claude -p` request, reply parsing; `panel::action` the action, text, cmd and state files |
| `highlight` | the code lexer (#282) and `split`, the one fence rule the panel's view and its reply parsing share (#283) |

Every mutating action stays a shell script: the binary prints
`<verb> <pane-id>` and a wrapper runs it.

### Push wakeups

A hook says "look now" with `tmux run-shell -C '#{@cockpit_board_wake}'`
instead of the board finding out on its next tick. The wake carries no payload
and needs none: the board reads the truth from tmux, it only needs to be told
when. No pid is signalled, so nothing has to be verified first (#224).

Each board waits in a `tmux wait-for cockpit-board-<pid>` child and lists
`wait-for -S cockpit-board-<pid> ; ` in the option. A channel of its own,
because tmux latches a `wait-for -S` only when nobody waits: on a shared
channel the waiter a SIGKILLed board left behind would take a wake that lands
while a live board is between two waits, and that board would never see it.

When the child returns the board refetches and starts another. Between the two
nobody waits, and tmux keeps one bit per channel: the first `-S` in that window
latches it, so the next wait returns at once, and a second `-S` in the same
window frees it again. That refresh then waits for the 1 s tick.

The waiter goes with the board on every exit path:

- `q`, a panic and the tty hangup watch (`backend::exit_on_hangup`) remove the
  entry and wake the waiter with three `-S`: wake, latch, free, and tmux then
  holds nothing for the channel. A cleanup that lands between two waits, with
  no wake pending, goes latch, free, latch instead, and the latched bit stays
  for the server's life. Nothing reads it: the entry is gone and no board
  waits on that name.
- SIGHUP and SIGTERM kill it from the handler, which runs nothing else, and
  leave the entry.
- On Linux, PDEATHSIG kills it when the board is SIGKILLed.

A left entry costs a hook one `wait-for -S` inside tmux, toggling one latched
bit on a channel nobody waits on. The next board's start drops every entry
whose pid is not a running cockpit-tui (read from `/proc/<pid>/comm`, or
`ps -o comm=`), and sends that channel one last `-S`, which releases a waiter
left where there is no PDEATHSIG. Only entries of the exact form
`cockpit-board-<digits>` are read. The start also unsets `@cockpit_tui_pid`,
which a board from before #224 may have left.

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

**A stale `@cockpit_board_wake` entry.** A board ended by a signal or a crash
leaves its entry. It is harmless: the hook only signals a channel, and the next
board to start drops it.

## The markdown viewer (#283)

`cockpit-tui view <file>` shows a markdown file rendered and read-only, and
any other text file as numbered code (#365). The
path picker opens it for a lone `.md`, `.markdown`, `.mdown` or `.mkd` file
(see [the path picker](path-picker.md)); `@cockpit_md_view off` turns that off.
It draws on /dev/tty, as `pick` does, so its one line of output, `edit <line>`
or `ask <a> <b>` (#365), goes to stdout.

| key | does |
|---|---|
| `↑↓` `j` `k` | scroll a row |
| `PgUp` `PgDn` `space` | a page |
| `g` `G` `Home` `End` | the top, the end |
| the wheel | three rows |
| `/` | find: every key but `^C` is text until `↵` keeps the matches or `esc` drops them and goes back |
| `n` `N` | the next and the previous match, round the end |
| `v` | select lines from the top row's line; the move keys, `g` `G` and `n` `N` then move its end, and `v` again ends it |
| a tap, a second tap | the first drops a selection's anchor, the second ends it there; a drag selects, a shift-click extends |
| `?` | ask about the selection, or the top line: close with `ask <a> <b>` (#365) |
| `e` | close, and open the file in the editor at the selection's end, else the top row's source line |
| `u` | show or hide link addresses (markdown) |
| `h` `F1` | the keys |
| `q` `esc` `^C` | close; `esc` first clears the selection, then the matches, and `^C` closes from the find field too |

- **What it draws.** Headings with their `#`s, paragraphs, emphasis, strong,
  strikethrough, inline code, lists nested to any depth (`•` `◦` `▪`, numbers
  from the list's start, `[ ]` and `[✓]` task boxes), block quotes with a bar
  per level, tables with their alignment, code blocks, rules, images as
  `[image: alt]` and HTML as written. CommonMark with GitHub's tables, task
  lists and strikethrough, read by `pulldown-cmark`.
- **Code.** A code block is coloured by the panel's lexer, by the tag in its
  info string (`highlight::tag`, so `rust,ignore` is Rust) with the same roles
  as the panel's code; a bare block of `$ ` lines is shell. Tabs go out to
  4-column stops, and a long line wraps hard, its spaces kept.
- **Links are text.** A link is its text, underlined, then its address dimmed
  in brackets, unless it is an autolink or an `#anchor`. `u` hides the
  addresses. Nothing is followed, and no HTML is read.
- **Widths.** A row is never wider than the body, measured per grapheme as
  the buffer draws it, so CJK and emoji wrap where they end. Nesting gives up
  its indent past half the width before the text gives up its room. A table
  too wide shares the width out and wraps its cells; one with no room for 3
  columns per cell is drawn as `header: value` lines.
- **Find.** Case-insensitive unless the text has a capital, as `less` and vim's
  smartcase read it. Matches are reversed and the current one bold too, so
  both read without colour. A match never spans two rows.
- **Lines.** Each row carries the source line of its block (a code row, its
  own line), which `e` and `--line` use. A resize keeps the top row's line on
  top.
- **What it refuses.** A file over 1 MB (a `- a` list of that size peaks at
  about 105 MB resident, measured), one with a NUL byte, invalid UTF-8, or anything but
  a regular file (opened non-blocking, so a FIFO never hangs it) is a note in
  the frame. `e` still opens it in the editor. Control and bidi characters are
  dropped from everything drawn, the title included, as `pick` drops them.
- **NO_COLOR and ascii.** Without colour, inline code keeps its backticks and
  the other kinds lean on bold, italics, underline and the marks. The ascii
  tier draws `-` `*` `+` bullets, `[x]`, `|` bars and `-+-` table rules.

### A code file (#365)

Only a name ending in `.md`, `.markdown`, `.mdown` or `.mkd` (a bare `.md`
included, as `tmux-open-path`'s `*.md` glob reads it) is markdown. Any other
file is shown whole as code, with no markdown read into it, so a `# comment`
or a `*ptr` is drawn as written:

```
╭ view-sample.rs ──────────────────────────────────────────────────────────────╮
│  5 /// Counts each word, ignoring case. "# not a heading", *not emphasis*.   │
│  6 pub fn count(text: &str) -> HashMap<String, usize> {                      │
│  7     let mut out = HashMap::new();                                         │
```

- **The language.** The extension names it for the lexer the panel uses
  (`highlight`): Rust, Python, JavaScript and TypeScript, JSON and shell. With
  no extension it knows, a first line's `#!` does (`#!/usr/bin/env python3`,
  `#!/bin/bash`, `dash`, `node`, `ts-node`); `env`'s flags, its `-u NAME`,
  `NAME=value` words and a version (`python3.11`) are skipped. A known
  extension wins over the `#!`. Anything else, Go or a `.txt` included, is
  plain text, numbered.
- **Rows.** Each source line is one row behind its line number, right aligned
  to the widest; a line wider than the body wraps hard, its later rows under a
  blank gutter. Tabs go out to 4-column stops. A row keeps its source line, so
  `e`, `--line` and a resize work as in markdown, and the selection of #365's
  next step sits on these rows.
- **Find** reads the code, never the line numbers: `/2` does not match line 2.
  A word a hard wrap splits is not found, as in markdown.
- **`u`** does nothing in a code file, which has no links, and its help leaves
  it out.
- **Memory.** A row stores its cells and its line, and the gutter is drawn
  from the line: 1 MB of empty lines, the most rows the cap allows, peaks at
  about 77 MB resident (release, measured), under markdown's worst.
- **What it refuses** is the markdown viewer's: over 1 MB, a NUL byte, invalid
  UTF-8, not a regular file. The note says so, and `e` still opens the editor.

### Selecting and asking (#365)

`v`, or a tap on a line, starts a line selection; the move keys (or a second
tap, or a drag) set its other end. The selected rows carry `▎` in the frame's
padding column, and the footer says which lines an ask would take
(`6-12 · 7 lines`); the hints there are taps too. `?` closes the viewer with
`ask <a> <b>`, the selection's lines (or the top line's), widened to the end of
the last markdown block it touches with blank lines off the end. Only a launcher
that routes that line sets `COCKPIT_VIEW_ASK=1`, and only then is `? ask`
offered: without it `?` leaves a note and the viewer stays, so a launcher that
reads only `edit` never loses the screen to it. `tmux-open-path` routes it to
the panel's ask in a later change.
`esc` clears the selection before the matches, and the help moved from `?` to
`h` and `F1`. A tap and a release, then a press, a drag and a release, select
from the first line to the drag's end. Under the floor card or the help a tap
does nothing, and `?` on an empty file, or one the viewer refused, is a note.

## `cockpit-tui branch` (#246)

The branch popup behind `prefix b` and the status-bar branch click, built on the
shared picker (`pick.rs`). `scripts/tmux-branch` runs `cockpit-tui branch -C <dir>`
and acts on the one line it prints; the binary never mutates.

| key | does |
|---|---|
| type | filters the list (case-insensitive substring) |
| `↵` | prints `switch <branch>`, `track <remote>/<branch>` on a remote-only row, or `goto <worktree path>` on a branch another worktree holds (#303) |
| `alt-↵` | prints `create <name>` from the filter text, after `git check-ref-format --branch` accepts it |
| `esc` `^C` | cancels, prints nothing, exit 1 |

Rows are local branches by most recent commit, then remote-only branches. `●` marks
the current branch. `⑂` marks a branch checked out in another worktree: git refuses
to switch to it, so `↵` prints `goto <worktree path>` and the script selects the pane
working in that tree (an agent's first), or opens a window there; a path with a newline
in it gives a `✕` note and prints nothing. The preview is
`git log --graph --oneline -30` of the selection, read on a worker thread and cached
per branch; it shows `loading…` until it arrives, and below 60 columns it is hidden.

Every git read uses the `gitsafe` flags. A branch name is untrusted text: control and
bidi characters are dropped from what is drawn, and the line printed carries the exact
ref. The script treats that line as untrusted too: a verb from `switch track create goto`, one
line, a name `git check-ref-format --branch` accepts (for `goto`, a path that `git worktree
list` names and that is still a directory), and for `switch`/`track` a ref that
exists under `refs/heads` or `refs/remotes`. Git's own refusal (a dirty tree) is shown
verbatim with `[any key]`, then the picker opens again.

A branch name cannot contain a space, so the filter text `my branch` is refused with a
note on `alt-↵`.

## `cockpit-tui files` (#363)

The file browser behind the cockpit table's `o`, the menu's Browse folder row and a tap on the
pane-border path, built on the shared picker (`pick.rs`) and `files.rs`.
`scripts/tmux-open-path --folder` opens it in a popup on the pane's folder and
acts on the one line it prints; the binary never mutates.

| key | does |
|---|---|
| type | filters the folder (case-insensitive substring) |
| `↵` | on a folder, goes into it and clears the filter; on a file, prints `view <path>` (the viewer) |
| `alt-e` | on a file, prints `open <path>` (the editor); on a folder, nothing |
| `←`, `⌫` on an empty filter | goes up, landing on the folder it left; stops at `/` |
| `.` on an empty filter | shows or hides dotfiles; elsewhere it is a letter of the filter |
| `alt-i` | shows or hides what `.gitignore`, `.ignore` and git's global excludes hide (the `ignore` crate, one level); shown entries are dim; `.git` is never listed |
| `alt-c` | prints `copy <path>` for the row, a folder included |
| `esc` `^C` | cancels, prints nothing, exit 1 |

Folders come first, then names without regard to case; a symlink to a folder is a
folder, a broken one a file. A folder over 20000 entries lists the first 20000 and
says so in the header. The path printed is the folder as `realpath` sees it joined to
the row's own bytes, so a name that is not UTF-8 still prints exactly. A name with a
newline cannot ride one line: `↵` and `alt-c` give a `✕` note and print nothing.
Exit 2 is a bad argument, a folder that cannot be read or no terminal.

On a touch client the script opens a file in a terminal editor window instead of
VS Code; see [path-picker.md](path-picker.md).

## The info screen (#252)

`cockpit-tui info %N` is what `prefix i` opens, in a popup (`@cockpit_info_key`
still names the key; `off` unbinds it). In the board, `i` draws the same screen
as an overlay over the selected pane and `esc` returns to the board; the board
no longer exits with `info %N` for the wrapper, and `cockpit-board` has no
`info` arm: an `info` line from an old binary is an unknown verb. The module is `info` (the
screen) and `data::info` (the facts); both are in the module table.

It shows, in this order: `pane`, `cwd`,
`dir` (`@cockpit_dir`, and `missing` when the directory is gone), a second
`dir` row `<dir>  (transcript cwd)` when the work dir (#452) is the transcript's
cwd, `root`,
`state` with `@panestate_msg`, the `doing` / `said` / `asked` rows with their
ages and the `/btw` pointer for an agent pane, `review` with the queued spool,
`base`, `session`, `title`, `tui` and the review dir with its pinned and
delivered counts. The git root and the review base are Rust, on the `gitsafe`
flags: the base is `review_base` from `review-diff.sh` (since-review, branch,
uncommitted), so the screen names what `prefix r` diffs against. Both read
their ref rows split on the US byte, so an empty upstream field keeps its
place and `origin/HEAD` serves as the base when the branch has no upstream,
ahead of a local `main` or `master`.

Text from outside (a transcript, a title, a path) is stripped of control
characters before it is drawn. It is not a text-entry screen: `esc` and `q`
close it, `↑↓` `j` `k` `PgUp` `PgDn` scroll, `?` lists the keys.
`tmux-review --info` is gone.

**`cockpit-tui info --json %N`** (#452) prints one line, `{"dir": "<path>"}`: the
work dir by the board's rule (`dir` in `--json` above), for scripts. Root, base and
the review dir take `@cockpit_dir`, else the cwd; `prefix r` asks `workdir` for its
tree (#470) instead. The path is decoded as lossy UTF-8, as
every tmux read is: a byte that is not UTF-8 becomes U+FFFD. Exit 0 with the
line; exit 1 and nothing on stdout when the pane is not there; exit 2 for a
missing or malformed pane id.

**`cockpit-tui workdir %N`** (#500) prints the board's `dir` for that pane, the
path alone on one line, for tmux-review and goto. It reads the pane with one
`display-message` of the board's own format and runs the fetch's per-row code
(`row_pane`, `work_candidates`, `fleet::work_dirs`), so its agent test and its
order are the board's. Unlike the fetch it never runs `claude agents`: a pane
with no `@cockpit_session` has no transcript to read, so its dir is
`@cockpit_dir`, else its launch dir. Lossy UTF-8, as above. Exit 0 with the
line; exit 1 and nothing on stdout when the pane is not there, is not an
agent's, or has no dir; exit 2 for a missing or malformed pane id.

## The board as the review queue (#253)

`prefix r` opens the board, the same command `prefix w` runs, and
`@cockpit_review_window on` opens it in a window. The fzf fleet queue is gone;
`tmux-review --fleet` went with it. `r` on a row opens the review loop as before.

**Row cells.** The list draws, between the message and the age: the diff stat
(`+a -d`, `clean`, `no repo`, `?N new` for untracked files) and the branch
(`⑂name`, or `⑂@<short sha>` on a detached HEAD). The review mark stays in the
row's leading column. A cell that does not fit is dropped with every cell to its
right; the cells take at most a third of the width the name and message share,
and leave the name 6 cells and the message 12, so a 40-column row may show none.

A pane in a linked worktree (#312) gets `repo⑂branch · wt <name>`, the repo
being the main checkout's. As the row narrows the repo goes first, then the
worktree name is cut (to at least one character) and then dropped, and the
branch goes last. `--rows` and `--json` carry the same string as `loc`:
`repo⑂branch`, plus ` · wt <name>` in a linked worktree, `@<short sha>` for
the branch when HEAD is detached.

**Where the facts come from.** `data/fleet.rs`, on the fetch thread. One diff
scan per distinct repo root however many panes share it, at most 8 git processes
at once, and the gitsafe flags. The tree the agent works in is `@cockpit_dir`,
else the session transcript's newest cwd, else the pane's cwd (#452); a relative
path never counts and gets no facts. The list still makes no pane capture.

**Verbs.** `cockpit-board` gains one line, `kill %N` (#288 removed `request %N`).
`kill %N` kills the pane, not its window (#301). The board writes the pane id
and the `#{window_id}` it showed (`%12 @9`, not the name: automatic-rename gives
agents the same name) to `$COCKPIT_ACTION.expect` first. The wrapper checks the
pane id is the action's, compares the window id with the window the pane is in
now, and kills the pane in the same tmux call; it refuses with a `✕` note if
the pane moved to another window, if the file is missing, empty, names another
pane, is a bare `@<digits>` (an older binary's, whose confirm promised the
window) or a symlink, or if the pane is gone. The `✓` note and the log carry
the pane and the `session:index` and name of its window.
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
ascii` reaches the picker as `COCKPIT_GLYPHS=ascii`; `nerd` draws the default
there, its one glyph of its own being the board's subagent badge.

## tui-fetch: pins, tags and `COCKPIT_TUI_BIN` (#223, #243)

These notes replace older wording above where the two disagree.

- `@cockpit_tui_version` is read as written. No release was ever tagged in the
  pre-#79 `tui-v` form, so tui-fetch no longer reads `tui-v0.1.0` as 0.1.0: such
  a pin names a tag that does not exist, and tui-fetch installs nothing.
- `COCKPIT_TUI_BIN` must be an absolute path for tui-fetch; anything else
  installs to the plugin's own `bin/cockpit-tui`.
- The same holds for every script, not only tui-fetch: `scripts/lib/tui.sh`
  replaces a `COCKPIT_TUI_BIN` that is not absolute with the plugin's own
  `bin/cockpit-tui` before anything runs it.
