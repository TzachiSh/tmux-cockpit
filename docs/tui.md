# The board (cockpit-tui)

`prefix w` opens a grid of live agent panes, one tile each, blocked ones first.
Each tile carries the pane's state glyph, what the agent is doing, how long it
has been doing it, and a live `capture-pane` of the pane itself. `↵` jumps to
the tile you are on and closes the board.

It is a small Rust binary, `bin/cockpit-tui`, and it is **optional**. Nothing
else in the plugin needs it, nothing breaks without it, and every key it offers
already exists as a bash path. If it is not installed the binding prints one
message and exits 0.

## What it is not

- **Not the fleet list.** `prefix r` stays fzf. fzf gives fuzzy filter,
  multi-select and the wheel for free; a table in Rust would buy `j`/`k`/`↵` and
  lose the filter. The board is a grid of previews, which is the thing fzf
  cannot draw.
- **Not an actor.** The binary renders and decides. It writes one line,
  `<verb> <pane-id>`, to the file named by `$COCKPIT_ACTION`, and the binding
  runs the existing `tmux-jump` / `tmux-review` script. Every mutating path
  stays shell, so the suite keeps testing it with no binary present.
- **Not a replacement for `prefix i` or `prefix f`.** The board reaches them; it
  does not reimplement them.

## Install

The binary ships as a release asset. With `@cockpit_tui_fetch on`, `prefix U`
(tpm's update key) fetches it; otherwise run `scripts/tui-fetch` yourself.
Either way: uname picks the asset, the `.sha256` is fetched first and verified,
and only a verified file is installed. A checksum mismatch, an unfetchable manifest or a
platform with no asset all leave whatever binary is already there and say so
once. The repository is private, so the fetch rides your own `gh` login when
`gh auth token` finds one (the plugin never carries a token); without one the
release URL 404s and the fetch skips. `COCKPIT_TUI_REPO` overrides the
`owner/repo` gh downloads from.

Nothing is fetched when tmux loads its config. The load path stays offline;
installing and updating stay gestures you make, the way tpm's own are.

To build from source instead, from the plugin directory:

```sh
make tui
```

That needs **Rust >= 1.88** (ratatui 0.30's floor; Debian 13 ships 1.85 and
Ubuntu 24.04 ships 1.75). On an older toolchain the fetch path refuses by name
rather than dumping a compiler error, and keeps any binary you already have.

`make check`, the repo's own gate, stays green with no cargo installed at all.
`make tui` is a separate target on purpose: the board must never become a build
prerequisite for a plugin that is otherwise shell.

The binary lives in the plugin's own `bin/` and is gitignored. `install.sh`
does not touch it; that script manages only its symlinks in `~/.local/bin`.

`@cockpit_board_window on` parks the board in a window instead of the popup.

## Keys

Inside the board:

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
| `r` | review its diff (`tmux-review`) |
| `f` | the panel for it (`cockpit-panel`) |
| `i` | info for the selected pane |
| `a` | ask this agent. Phase 1 jumps; phase 2 opens the reply overlay |
| `?` | help overlay, generated from the key table itself |
| `^L` | refetch and redraw now |
| `q` | close. `esc` closes an open overlay, else closes the board |

`j`/`k` are vertical because the board is a grid; `⇥` is the linear one. The
help overlay is generated from the same table the dispatch reads, so a renamed
key cannot go stale in one and not the other.

**No mouse**, in phase 1 or phase 2. Enabling SGR capture inside a popup takes
the wheel and text selection away from you for as long as the board is open,
and the board is a screen you hold for seconds.

## Options

Set these before the plugin's `run-shell` line, like every other cockpit option.

| option | default | does |
|--------|---------|------|
| `@cockpit_board_key` | `w` | rebinds the board. `off` leaves stock tmux alone |
| `@cockpit_board_window` | unset | `on` parks the board in a window instead of a popup, mirroring `@cockpit_review_window` |
| `@cockpit_tui_fetch` | `off` | `on` lets `prefix U` run `scripts/tui-fetch`. Off, nothing is ever fetched from the config load path, and `scripts/tui-fetch` by hand is the way in |
| `@cockpit_tui_version` | unset | pins the release tag `prefix U` fetches: `cockpit-tui-v<ver>`, e.g. `cockpit-tui-v0.1.0`. The pre-#79 `tui-v` form is accepted for comparison only — the fetch URL uses the tag as written, so a stale `tui-v` pin compares as current against a release that does not exist. |
| `@cockpit_tui_pid` | (state) | read-only: the pid of the running board. Written at start, unset on `q`, SIGHUP and SIGTERM |
| `@cockpit_panel_key` | `f` | binds the panel (`cockpit-tui panel %N` through `scripts/cockpit-panel`) to a prefix key. `off`, or an unset key with the old `@cockpit_fixer_key off`, leaves stock `find-window` on `f` |
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
| `COCKPIT_GLYPHS` | `ascii` swaps the glyphs *and* the borders for the ambiguous-width fallback tier |
| `COCKPIT_CLAUDE` | the panel's `claude` binary. Unset is `claude` on `PATH` |
| `COCKPIT_PANEL_TIMEOUT` | seconds before the panel kills its `claude -p` call. Default 120 |
| `COCKPIT_PANEL_MODEL` | forces the panel's model, over the rules and the model picked on screen |
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

120 columns, 5 agents, 2 columns, full density. The width is the board's own
area, not the terminal's.

```
 board · 5 agents · 1 needs you
┌ 1 ▲  henosis:1 · Openvpn setup ──────────────────────────┐┏▎2 ●◆ henosis:3 · Implement with test review and rig ━━━━━┓
│permission: Bash(sudo openvpn --config                4m12│┃Bash: shellcheck on cockpit.tmux and tests/run.sh      32s┃
│/etc/openvpn/work.conf --daemon --log /var/log/ovpn.log)  │┃While the suite and rig run: shellcheck on the two script…┃
│Bash command                                              │┃$ make check                                              ┃
│sudo openvpn --config /etc/openvpn/work.conf --daemon     │┃1001 assertions, 0 failures                               ┃
│Do you want to proceed?                                   │┃$ shellcheck cockpit.tmux tests/run.sh                    ┃
│❯ 1. Yes   2. Yes, don't ask again   3. No                │┃                                                          ┃
│                                                          │┃                                                          ┃
│                                                          │┃                                                          ┃
└─ ↵ answer ───────────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ 3 ●  work:2 · Fleet gate review ─────────────────────────┐┌ 4 ✓↵ work:4 · Left click issue ──────────────────────────┐
│Agent: whole-branch review of phase 1                 4m12││idle · handover pending                                12m│
│Dispatched the review agent; waiting on its report before…││Pushed PR #28 and left the branch on master.              │
│⏺ Agent (fleet-gate-review)                               ││  ✓ pushed to origin/fix/left-click                       │
│  Read tests/fleet.sh, 214 lines                          ││  PR #28 opened                                           │
│  Read scripts/agent-state, 412 lines                     ││❯                                                         │
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
 ↵ jump   a ask   r review   f panel   i info   ? help   q close
```

Tile 1 is blocked: red border, the question in place of the doing line, and
` ↵ answer ` on the bottom border. Tile 2 has focus (thick border) and is
tmux's active pane (the `▎`). The `◆` after its state glyph is a pending
review. The right rail is exactly 5 columns of elapsed duration; a 6-column
duration would be clipped by the rail and lose its leading digit.

Tiles sort by `(state rank, pane id)`: needs-input, error, running, done, idle.
Nothing in the sort key is a clock, so tiles do not reorder while you read them.
Page 1 is the triage page by construction.

The grid is `clamp(width / 48, 1, 3)` columns, then `min(cols, n)`. Height picks
a density: 2 header rows at `h >= 7`, 1 at `h >= 5` (the `said` line drops),
and below that the grid scrolls by whole tile rows. A tile is never half cut and
never resizes as you scroll; the header's `1-4 of 5   ▼1` is the indicator, not
a scrollbar.

### The help overlay

```
╭ keys ──────────────────────────────────────────────────────────────────╮
│ move                          act                                      │
│ h l  ← →   column             ↵    jump to this pane, close the board  │
│ j k  ↑ ↓   row                a    answer / ask this agent             │
│ ⇥ ⇧⇥       next / previous    r    review its diff                     │
│ 1-9        tile by number     f    panel for this pane                 │
│ g G        first / last       i    info for this pane                  │
│ u          next blocked                                                │
│ [ ]        page               view                                     │
│                               ?    this help                           │
│                               ^L   refresh now                         │
│                               q    close   ·   esc close overlay       │
╰ any key closes ────────────────────────────────────────────────────────╯
```

### The triage overlay

Phase 2, not shipped. It answers a blocked agent in place rather than attaching
to its pane, and reserves the action line `deliver %N`. Its design is on
[#61](https://github.com/TzachiSh/tmux-cockpit/issues/61), in the research
lane's `ux.md` §2.

## States

Exact text. All lower case, no exclamation marks. The `cockpit:` prefix appears
only on `display-message` output, never inside the board, where the frame
already says who is talking.

**No agent panes:**

```
╭ board ─────────────────────────────────────────────────────╮
│                                                            │
│ no agent panes                                             │
│                                                            │
│ cockpit tracks a pane once an agent runs in it.            │
│ start one, or press q and use prefix r for the full fleet. │
│                                                            │
╰ q close ───────────────────────────────────────────────────╯
```

**tmux not answering**, in two stages, never an instant blank screen. Until 30 s
the last-known tiles stay and the header carries `· stale 34s`. After 30 s or
three consecutive failures the grid is replaced:

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

**Terminal below 40 usable columns:** the board refuses to draw and prints
`terminal too narrow — prefix i on the pane instead`. Better than a row that
lies about which agent it names.

**`a` on a tile that is not waiting on you:** one line in the footer row for
2 s, replacing the key hints.

```
 work:2 is not waiting on you — ↵ jumps there, /btw asks it while it works
```

**No binary, or a non-zero exit:** one `display-message`, exit 0, never a broken
popup. `prefix r` still opens the fleet list.

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
jump %N · review %N · panel %N · info %N · deliver %N   (deliver reserved for phase 2)
paste %N · send %N                                      (the panel)
```

The verb is `[a-z]+`, the argument is a tmux pane id. The binary executes
nothing. `paste` and `send` carry text, which goes to `$COCKPIT_ACTION.text`,
mode 0600, control characters stripped; with `COCKPIT_ACTION` unset the panel
refuses them and exits 1.

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

`ok` marks that step `✓ sent` and selects the next one with a command;
`failed:<why>` shows why as an error note and marks nothing. The loop ends when
the binary exits with no action, or with one taken anywhere but Steps. The
binary reads the file back strictly: no symlink, a regular file of at most
64 KiB, version 1, at most 64 steps, every line in its place, and the pane must
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

**CLI:** `cockpit-tui board | panel %N [--resume <state> --last <how>] |
transcript ... | --rows | --json | --version | -h | --help`. There is no `fleet`, `info` or `ask` subcommand: the
board reaches those through the action file and the shell scripts.

**Helper subcommands (#199):** `cockpit-tui transcript file %N | doing <jsonl> |
tail <jsonl> [max]` is the transcript reader of `scripts/lib/transcript.sh`,
byte-identical to the jq version (Freeze 1 on #54) with two #193 changes: a row
whose `message` is not an object is skipped where jq failed the whole read, and
`<bash-input>`, `<bash-stdout>` and `<bash-stderr>` prompts are dropped like the
other harness tags. It also departs from jq in three declared places:
- a content item that would raise a jq runtime error is skipped, not fatal;
- JSON only jq accepts (NaN, leading zeros, nesting deeper than 128) skips the line;
- `file` sorts project directories in byte order, not the locale's.

Its exit-code rule overrides the one below: every failure, a usage error
included, is no output, status 1 and nothing on stderr.

**Log format:** one line per event, `<unix_micros> <text>`, append-only, only
when `COCKPIT_LOG` is set. No rotation.

**Exit codes:** `0` normal (an action was written, or none), `1` runtime failure
(tmux unreachable, terminal not a tty), `2` usage error (unknown subcommand, bad
pane id). The binding treats a missing binary and a non-zero exit the same way.

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
- **No mouse in phase 1 or 2.**
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
| `backend` | the VS16 wide-glyph shim over the Crossterm backend, and `init()` / `restore()` with the panel's bracketed paste |
| `data` | every tmux and transcript read, `Pane`, the truncation helpers |
| `board` | the tile grid and its event loop |
| `fleet` | the headless `--rows` / `--json` formatters and the list widths |
| `theme` | the `@thm_*` colour roles and the glyph tiers |
| `push` | `@cockpit_tui_pid`, SIGUSR1 wakeups, the exit-path cleanup |
| `panel` | the type-panel port: capture, redaction, prompt envelope, the `claude -p` request, reply parsing |

Every mutating action stays a shell script: the binary prints
`<verb> <pane-id>` and a wrapper runs it, so the bash suite works with no
binary present.

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
and exits when it comes; the action file is left unwritten.

**A crash left nothing behind.** `display-popup -E` closes the popup the moment
the command exits, so a panic disappears with it. The log is at
`${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/tui.log`, on by default, and
the binding appends stderr to it.

**The glyphs shift the columns.** `▲ ● ✓ ◆` are East-Asian Ambiguous: a terminal
with an emoji-preferring fallback draws them two cells wide. The 2-column gutter
absorbs that. If it still looks wrong, `COCKPIT_GLYPHS=ascii` swaps the glyphs
and the borders for a tier that is byte-stable across fonts.

**Screenshots and benchmarks.** `COCKPIT_READONLY=1` makes the binary skip every
`set-option`, so watching the fleet does not mutate it.

**A stale `@cockpit_tui_pid`.** The board unpublishes the pid on `q`, SIGHUP and
SIGTERM. A pid left behind after a `SIGKILL` is harmless: the hook reads
`/proc/$pid/comm` before it signals anything.
