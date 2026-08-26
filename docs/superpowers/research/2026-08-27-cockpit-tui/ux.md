# cockpit-tui: the board, the triage, the keymap

UI/UX lane, 2026-08-27, read-only. Designs phase 1 (the board) and phase 2 (needs-you
triage) of the Rust/Ratatui binary against screens.md §0's grid, colour roles and glyph
tiers, chrome.md §1's urgency ramp, and ratatui.md's measured constraints. Every mockup
below is generated at an exact column count (`scratchpad/mock.py`, `over.py`), not
hand-drawn; all lines verified at 80/120/160.

Mockup widths are the **board's own area** (`f.area()`), not the terminal. A 70 % popup
on a 120-column terminal gives an 84-column board, which is one tile column. That is the
single most consequential geometry fact here: **the board wants `-w 90%` or
`@cockpit_board_window on`, not the 70 % the other popups use**, or it will be a
one-column list on every terminal the owner actually uses.

---

## 1. The board

### 1.1 Tile anatomy

```
┌ 3 ●◆ henosis:3 · Implement with test review… ────────────┐
│Bash: shellcheck on cockpit.tmux and tests/run.sh      32s│
│While the suite and rig run: shellcheck on the two scrip… │
│$ make check                                              │
│1001 assertions, 0 failures                               │
└──────────────────────────────────────────────────────────┘
  │ │ ││  │          │                              │    │
  │ │ ││  │          └ ai-title, right-truncated     │    └ 5-col rail: elapsed
  │ │ ││  └ where (session:window), meta             └ doing, middle-truncated
  │ │ │└ review mark ◆/↵, blank when none
  │ │ └ state glyph ▲ ✕ ● ✓ ○
  │ └ tile number, 1-9, addressable
  └ ▎ when this pane is tmux's active pane, blank otherwise
```

Five parts, in the order the eye needs them:

| part | rows | content | colour role |
|---|---|---|---|
| top title | in the border | `▎N ●◆ where · title` | see §5 |
| doing + rail | 1 | tool line, `32s` right-flush in 5 cols | value / meta |
| said | 1 (full density only) | the agent's last sentence | quote |
| preview | rest | `capture-pane -ep`, raw ANSI, clipped | its own |
| bottom title | in the border | only on a needs-input tile: ` ↵ answer ` | needs |

The header is exactly `Length(2)` at full density and `Length(1)` at compact — the demo's
shape (board.rs:79, `Layout::vertical([Length(2), Min(0)])`) reused verbatim. **No rule,
no blank line, no inner Block between the header and the preview**: the header's label
colours and the right-flush rail already read as chrome, and a separator costs a preview
row at exactly the density where rows are scarce.

The 2-cell glyph gutter (`●◆`, `▲ `) is screens.md §0's, and it is load-bearing for a
different reason here: `▲ ● ✓ ◆` are East-Asian Ambiguous and a terminal with an
emoji-preferring fallback draws them 2 cells wide (ratatui.md "Rendering hazards"). Two
reserved cells mean a double-wide glyph eats its own padding and the title does not shift.

**Preview**: capture the pane's *visible screen*, drop trailing blank lines, show the
**last** `h` lines. The demo shows the head (board.rs:99); the tail is right — for a busy
agent it is the newest output, for a blocked one it is the permission prompt, for an idle
one it is the composer, and all three are what you would see if you jumped there. It also
fixes competition.md anomaly 3 (fzf's `tail` looked blank because the fake panes were
padded with blanks; dropping trailing blanks is the fix, not switching to head).

Preview lines are **clipped, not truncated and not wrapped** — no `…`, no `Wrap`. A
`Paragraph` with no `Wrap` clips at the widget edge for free, and `Wrap` on arbitrary
terminal output hits ratatui #2679 (panic on double-width graphemes).

### 1.2 Focus, active pane, and the ring

Two independent signals, because they are two different facts (the pattern is
tmux-agent-sidebar `src/ui/panes/row.rs:38-64`, which separates "the cursor is here" from
"tmux's active pane is here"):

- **focus** = border **weight**: `BorderType::Thick` focused, `BorderType::Plain`
  otherwise. Weight, not colour, because it survives NO_COLOR, greyscale and
  red/green colourblindness — the same argument README.md:29-30 already makes for the
  glyphs, and the one agent-manager writes down explicitly
  (`internal/ui/styles.go:135-138`, "shape carries the state; colour reinforces it").
- **tmux-active pane** = a `▎` in the first title cell.

Border **colour** carries state and never focus: `needs` red on a blocked tile, `error`
peach on an errored one, `accent` lavender on the focused tile, `meta` otherwise. A
focused blocked tile is thick **and** red — the two signals compose instead of competing.
That is the one thing to get right; every alternative (double border for focus, reverse
video, a `▶` pointer stealing a column) either collides with the needs treatment or costs
grid columns.

Unfocused tiles are **dimmed rather than the focused one brightened** (gitui
`src/ui/style.rs:46-62`) — cheap, and it scales to nine tiles without nine loud borders.

### 1.3 Needs-input treatment

Three changes, all visible without reading:

1. Border red, and the tile **sorts first**.
2. The `doing` line is replaced by the question, in `needs` red, wrapped to the header's
   2 lines (our own text, so wrapping is safe — the `Wrap` ban is about the preview).
3. The bottom border carries ` ↵ answer `. Not "needs you" — the ▲, the red border and
   the red question already say that; the bottom title is where the eye looks for *what
   to do*, so it gets the verb.

`title_bottom` on a `Block` (the demo already does this, board.rs:69-74) costs zero rows.

### 1.4 Sort order

`(state rank, pane id)`, ranks: needs-input 0, error 1, running 2, done 3, idle 4.

Deliberately **not** `(rank, elapsed, id)` — workmux sorts that way
(`src/command/dashboard/app/agents.rs:126-149`) and it means tiles reorder on a clock
while you are looking at them. Rank changes are event-driven and rare; ages tick every
second. Ties broken by pane id, which is stable and monotonic.

The payoff of sorting plus paging: **page 1 is always the triage page.** Everything that
needs you is on it, by construction.

### 1.5 Grid, density, paging

```
cols  = clamp(width / 48, 1, 3)  then min(cols, n)   # 48 = narrowest useful tile
rows  = ceil(n / cols)
h     = (height - 2) / rows                          # 2 = header + footer
h >= 7 -> full    (2 header rows + >=3 preview)
h >= 5 -> compact (1 header row  + >=2 preview, `said` dropped)
h <  5 -> compact + paging: rows_shown = (height-2)/5, page = cols * rows_shown
```

Two rules that are not obvious and matter:

- **`cols = min(cols, n)`** — never leave a whole empty column; two agents on a
  160-column board get two 80-wide tiles, not two 53-wide ones with a third of the screen
  blank.
- **Grid geometry is fixed by the page size, not by this page's occupancy.** A last page
  holding one agent renders one normal-sized tile and blank space below, not one tile
  22 rows tall. Otherwise `]` visibly resizes everything.

Within a row, the last cell absorbs the remainder so the right edge is flush; a row with
fewer tiles than columns leaves the trailing cells **empty**. An empty cell is cheaper
than a tile that changes width when a sixth agent appears.

**Paging, not scrolling.** A half-cut tile is worse than a page boundary, and with
needs-first sorting the first page is the one that matters. Precedent: recon paginates its
rooms (`src/view_ui.rs:423-447`). No `Scrollbar` widget anywhere in the binary.

| n | 80×24 (1 col) | 120×30 (2 cols) | 160×24 (3 cols) |
|---|---|---|---|
| 1 | 1 tile, full height — a live mirror of that pane | same | same |
| 2 | 2 tiles stacked, h=11, full | 2 tiles side by side, full | 2 tiles, 80 wide each (`min(cols, n)`) |
| 5 | compact, 4 per page, 2 pages | full, 3 rows (last row half empty) | full, 2 rows (last row one gap) |
| 9 | compact, 4 per page, 3 pages | compact, 6 per page, 2 pages | **full, 3×3, one page** |
| 16 | compact, 4 per page, 4 pages | compact, 6 per page, 3 pages | compact, 12 per page, 2 pages |

At 1 agent the tile is as tall as the board and the preview asks `capture-pane` for
`h-4` lines, so a tall tile shows *more of the pane*, not more blank. The board at n=1 is
a legitimate "watch this one" view, not a degenerate case.

### 1.6 Header and footer

Header, one line, no border:

```
 board · 5 agents · 1 needs you                             1-4 of 5   page 1/2
```

Left: counts, reusing `fleet.rs:83-95`'s `counts()` (agents · N needs you · N review
pending). Right: the range and page, **printed only when `pages > 1`**. No spinner: the
tick is 1 Hz and costs 13 ms, so an animation there would be decoration pretending to be
feedback. What does go on the right when it applies is a **staleness marker**,
`· stale 34s` in `error`, when the last successful tick is older than 3 s.

Footer, one line, no border, degrading in exactly two stages (television
`screen/status_bar.rs:118-128` — measure the built string, drop to the help-only form,
then to nothing, rather than letting a `Paragraph` clip mid-word):

```
 ↵ jump   a ask   r review   f fix   i info   ? help   q close     (fits >= 62)
 ↵ jump   ? help   q close                                         (fits >= 27)
 ? help                                                            (last resort)
```

Keys in `accent`, verbs in `meta`, three spaces between pairs — screens.md §0's key-hint
convention, and pickers.md:161's ordering rule (leftmost = least skippable).

### 1.7 The board at 80 × 24, 5 agents — compact, page 1 of 2

```
 board · 5 agents · 1 needs you                             1-4 of 5   page 1/2
┌ 1 ▲  henosis:1 · Openvpn setup ──────────────────────────────────────────────┐
│permission: Bash(sudo openvpn --config /etc/openvpn/work.conf --daemon…   4m12│
│Bash command                                                                  │
│sudo openvpn --config /etc/openvpn/work.conf --daemon                         │
│Do you want to proceed?                                                       │
└─ ↵ answer ───────────────────────────────────────────────────────────────────┘
┏▎2 ●◆ henosis:3 · Implement with test review and rig ━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃Bash: shellcheck on cockpit.tmux and tests/run.sh                          32s┃
┃$ make check                                                                  ┃
┃1001 assertions, 0 failures                                                   ┃
┃$ shellcheck cockpit.tmux tests/run.sh                                        ┃
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ 3 ●  work:2 · Fleet gate review ─────────────────────────────────────────────┐
│Agent: whole-branch review of phase 1                                     4m12│
│⏺ Agent (fleet-gate-review)                                                   │
│  Read tests/fleet.sh, 214 lines                                              │
└──────────────────────────────────────────────────────────────────────────────┘
┌ 4 ✓↵ work:4 · Left click issue ──────────────────────────────────────────────┐
│idle · handover pending                                                    12m│
│  ✓ pushed to origin/fix/left-click                                           │
│  PR #28 opened                                                               │
└──────────────────────────────────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f fix   i info   ? help   q close
```

Page 2 keeps the tile size:

```
 board · 5 agents · 1 needs you                             5-5 of 5   page 2/2
┌ 5 ○  work:5 · (no title) ────────────────────────────────────────────────────┐
│—                                                                             │
│❯                                                                             │
│                                                                              │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f fix   i info   ? help   q close
```

### 1.8 The board at 120 × 30, 5 agents — 2 columns, full density

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
 ↵ jump   a ask   r review   f fix   i info   ? help   q close
```

### 1.9 The board at 160 × 24 — 3 columns, 5 and 9 agents

```
 board · 5 agents · 1 needs you
┌ 1 ▲  henosis:1 · Openvpn setup ───────────────────┐┏▎2 ●◆ henosis:3 · Implement with test review and … ┓┌ 3 ●  work:2 · Fleet gate review ───────────────────┐
│permission: Bash(sudo openvpn --config         4m12│┃Bash: shellcheck on cockpit.tmux and tests/…    32s┃│Agent: whole-branch review of phase 1           4m12│
│/etc/openvpn/work.conf --daemon --log…             │┃While the suite and rig run: shellcheck on the two…┃│Dispatched the review agent; waiting on its report …│
│Bash command                                       │┃$ make check                                       ┃│⏺ Agent (fleet-gate-review)                         │
│sudo openvpn --config /etc/openvpn/work.conf --daem│┃1001 assertions, 0 failures                        ┃│  Read tests/fleet.sh, 214 lines                    │
│Do you want to proceed?                            │┃$ shellcheck cockpit.tmux tests/run.sh             ┃│  Read scripts/agent-state, 412 lines               │
│❯ 1. Yes   2. Yes, don't ask again   3. No         │┃                                                   ┃│                                                    │
│                                                   │┃                                                   ┃│                                                    │
│                                                   │┃                                                   ┃│                                                    │
│                                                   │┃                                                   ┃│                                                    │
└─ ↵ answer ────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└────────────────────────────────────────────────────┘
┌ 4 ✓↵ work:4 · Left click issue ───────────────────┐┌ 5 ○  work:5 · (no title) ─────────────────────────┐
│idle · handover pending                         12m││—                                                  │
│Pushed PR #28 and left the branch on master.       ││                                                   │
│  ✓ pushed to origin/fix/left-click                ││❯                                                  │
│  PR #28 opened                                    ││                                                   │
│❯                                                  ││                                                   │
│                                                   ││                                                   │
│                                                   ││                                                   │
│                                                   ││                                                   │
│                                                   ││                                                   │
└───────────────────────────────────────────────────┘└───────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f fix   i info   ? help   q close
```

Nine agents still fit on one page at 160×24 (h = 7, exactly full density) — the density
ladder buys the whole fleet in one screen at the width the owner's wide terminal has:

```
 board · 9 agents · 1 needs you
┌ 1 ▲  henosis:1 · Openvpn setup ───────────────────┐┏▎2 ●◆ henosis:3 · Implement with test review and … ┓┌ 3 ●  work:2 · Fleet gate review ───────────────────┐
│permission: Bash(sudo openvpn --config         4m12│┃Bash: shellcheck on cockpit.tmux and tests/…    32s┃│Agent: whole-branch review of phase 1           4m12│
│/etc/openvpn/work.conf --daemon --log…             │┃While the suite and rig run: shellcheck on the two…┃│Dispatched the review agent; waiting on its report …│
│Bash command                                       │┃$ make check                                       ┃│⏺ Agent (fleet-gate-review)                         │
│sudo openvpn --config /etc/openvpn/work.conf --daem│┃1001 assertions, 0 failures                        ┃│  Read tests/fleet.sh, 214 lines                    │
│Do you want to proceed?                            │┃$ shellcheck cockpit.tmux tests/run.sh             ┃│  Read scripts/agent-state, 412 lines               │
│❯ 1. Yes   2. Yes, don't ask again   3. No         │┃                                                   ┃│                                                    │
└─ ↵ answer ────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└────────────────────────────────────────────────────┘
┌ 4 ✓↵ work:4 · Left click issue ───────────────────┐┌ 5 ○  work:5 · (no title) ─────────────────────────┐┌ 6 ●  work:6 · Task 6 ──────────────────────────────┐
│idle · handover pending                         12m││—                                                  ││Bash: step 6                                      6m│
│Pushed PR #28 and left the branch on master.       ││                                                   ││said line                                           │
│  ✓ pushed to origin/fix/left-click                ││❯                                                  ││$ ...                                               │
│  PR #28 opened                                    ││                                                   ││out                                                 │
│❯                                                  ││                                                   ││                                                    │
└───────────────────────────────────────────────────┘└───────────────────────────────────────────────────┘└────────────────────────────────────────────────────┘
┌ 7 ●  work:7 · Task 7 ─────────────────────────────┐┌ 8 ●  work:8 · Task 8 ─────────────────────────────┐┌ 9 ●  work:9 · Task 9 ──────────────────────────────┐
│Bash: step 7                                     7m││Bash: step 8                                     8m││Bash: step 9                                      9m│
│said line                                          ││said line                                          ││said line                                           │
│$ ...                                              ││$ ...                                              ││$ ...                                               │
│out                                                ││out                                                ││out                                                 │
│                                                   ││                                                   ││                                                    │
└───────────────────────────────────────────────────┘└───────────────────────────────────────────────────┘└────────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f fix   i info   ? help   q close
```

Sixteen agents at 160×24: compact, 12 per page, `1-12 of 16   page 1/2` on the header
right, tile numbers 10-12 in the last row (the number field is 1-2 characters, budgeted).

---

## 2. Needs-you triage (phase 2)

One overlay, three shapes. `Clear` then a rounded `Block`, centred, 76 columns wide
(clamped to `area.width - 8`), height from the content. Rounded to match the proposed
`popup-border-lines rounded` (chrome.md §2); colour is `needs` red, which is what
separates it from the help overlay.

Fourteen of the surveyed dashboards answer "reply" with "attach to the real pane". Only
workmux has a genuine inline reply (`app/agents.rs:307-320`). This overlay is that, and
it is the whole reason phase 2 exists.

### 2.1 A permission prompt

```
╭ ▲ needs you · henosis:1 · Openvpn setup ─────────────────────────────────╮
│ Claude needs your permission to use Bash                                 │
│                                                                          │
│ sudo openvpn --config /etc/openvpn/work.conf --daemon                    │
│   --log /var/log/ovpn.log                                                │
│                                                                          │
│ blocked 4m12s · henosis:1 · ~/work/vpn                                   │
│                                                                          │
│ reply> yes, but log to /tmp/ovpn.log instead                             │
╰ ↵ send   y allow   n deny   ⇥ next blocked   esc back        1 of 1 ─────╯
```

### 2.2 An AskUserQuestion

```
╭ ▲ question · work:2 · Fleet gate review ─────────────────────────────────╮
│ Which base branch should the fix worktree track?                         │
│                                                                          │
│ 1  master              the shipped default                               │
│ 2  feat/lowercase-keys where the failing test lives                      │
│ 3  let me type one                                                       │
│                                                                          │
│ blocked 47s · work:2 · Fleet gate review                                 │
│                                                                          │
│ reply>                                                                   │
╰ 1-3 pick   ↵ send   ⇥ next blocked   esc back              1 of 2 ───────╯
```

The two differ in exactly three places, and nowhere else:

| | permission | AskUserQuestion |
|---|---|---|
| title word | `needs you` | `question` |
| body | the command, verbatim, indented 0/2 for its wrapped tail | the question, then its options as `N  label   hint` |
| footer verbs | `y allow   n deny` | `1-3 pick` |

A third shape, **plain waiting** (blocked with no structured prompt): title word
`waiting`, body = the agent's last sentence, footer has neither the y/n pair nor the digit
range — only `↵ send   ⇥ next blocked   esc back`. Do not invent a y/n affordance for a
prompt that has no yes and no no; agent-deck writes down why (`internal/agents/view.go:59-66`:
naming the wrong state "sends the reader looking for a prompt that does not exist").

### 2.3 Key semantics inside the overlay

- `↵` with text in the input **sends** (spawn `tmux-review --deliver` through
  `run-shell -b`, because a popup kills its children — ratatui.md "Data model"). `↵` with
  an **empty** input **jumps** to the pane instead, and the footer changes to say so:
  `↵ jump` when empty, `↵ send` when not. One condition, no ambiguity.
- `esc` closes the overlay and returns to the board — never the whole binary. Esc closes
  the innermost thing only (bottom `src/app.rs:228-292` layers this explicitly).
- `y` / `n` and `1-9` are **only live while the input is empty.** A text input swallows
  letter keys; a shortcut that silently eats the `y` of "yes, but…" is the classic bug
  here. When the input is non-empty those keys are literal characters, and the footer
  drops them.
- `⇥` moves to the next blocked agent without leaving the overlay (recon's
  `Command::Next`, `src/main.rs:95-105`, is the same idea one level out). `1 of 3` on the
  footer right says how many are queued.
- `q` does **not** close the overlay — it would be a typo away from a lost reply. Only
  `esc`.

### 2.4 The countdown

There is no countdown in the board or the triage overlay. A modal the human is typing into
must not close itself. The countdown belongs to the passive needs-you toast
(screens.md §3), which stays in bash.

If it is ever ported: drive it from an `Instant` deadline, not from the poll loop, so the
digit changes exactly once a second regardless of how often `poll` returns; render it as a
fixed-width `%2d` inside the footer `Line`. Flicker is a non-issue in ratatui — the
double-buffer diff writes the three changed cells and nothing else (measured in
ratatui.md: 21 bytes for a glyph flip). The bash `\r` trick has no analogue and needs
none. The rule that does carry over: **the countdown must not enter the dirty-hash**, or
every second becomes a full redraw of every preview.

---

## 3. Keyboard model

### 3.1 Phase 1

| key | does |
|---|---|
| `h` `l` `←` `→` | focus one tile left / right |
| `j` `k` `↑` `↓` | focus one tile up / down |
| `⇥` `⇧⇥` | next / previous tile in reading order, wrapping across pages |
| `1`-`9` | select tile N **on this page** |
| `g` `G` | first / last tile |
| `u` | select the next needs-input tile (mirrors `prefix u`, the repo's jump key) |
| `[` `]` `PgUp` `PgDn` | previous / next page (only when paged) |
| `↵` | jump to the selected pane, close the board |
| `r` | review its diff (`tmux-review`) |
| `f` | side fix from it (`tmux-fixer`) |
| `i` | info overlay for the selected pane |
| `a` | phase 1: jump. phase 2: the triage overlay |
| `?` | help overlay |
| `^L` | refetch and redraw now |
| `q` | close. `esc` closes an overlay if one is open, else closes |

The demo maps `j` to "next" and `k` to "previous" linearly (board.rs:143-152); that is
wrong once there is a grid. `j/k` are vertical, `⇥` is linear. bottom keeps lowercase
`hjkl` for movement inside a widget and uppercase `HJKL` between widgets
(`src/app.rs:1173-1176`); the board needs no inner scroll in phase 1, so lowercase is free
for grid movement and the uppercase set stays unassigned.

Numbers **select**, they do not jump. `3` then `↵` is two keys; `3` alone teleporting you
out of the board on a mistyped digit is not worth saving one. workmux binds `1`-`9` to
`JumpToIndex` = select (`keymap.rs:34-71`); recon uses `1`-`9` to zoom a room. Numbers
appear in both exactly when the UI became directly addressable — which is what putting the
number in each tile title is for.

### 3.2 Phase 2 additions

| key | does |
|---|---|
| `a` | triage overlay on the selected tile; on a non-blocked tile it refuses with a message (§4) rather than opening an input that cannot be sent |
| `A` | triage overlay on the **first** blocked agent, from anywhere |
| in overlay | see §2.3 |

### 3.3 Not in phase 1 or 2

- **No mouse.** Enabling SGR capture inside a `display-popup` takes the wheel and text
  selection away from the user for the whole time the board is open (ratatui.md "Event
  loop"). The board is a glance-and-jump screen open for seconds; trading tmux's own
  copy-mode for click-to-focus is a bad trade. Revisit only with drag-to-move-window,
  which is the one feature that cannot be done without it.
- **No `:` command palette.** Twelve keys do not need one. workmux has `:` because it has
  forty.
- **No `/` filter.** Filtering is why the fleet list stays fzf. The board is bounded by
  what fits on a page, and the page is sorted so the important tiles are on page 1.
- **No kitty keyboard protocol** (absent under tmux, #4196), and **no terminal queries** —
  crossterm #1104 wedges the parser on an unrecognised `CSI ? … $y`, which is exactly what
  tmux emits around a popup. Pin `crossterm = "=0.29.0"`; keep `q` as an exit that does not
  route through Escape parsing.

### 3.4 The `?` overlay

Two columns, key on the left of each pair, grouped `move` / `act` / `view`. Any key
closes it — a help screen you have to learn a key to leave is a joke at its own expense.

```
╭ keys ──────────────────────────────────────────────────────────────────╮
│ move                          act                                      │
│ h l  ← →   column             ↵    jump to this pane, close the board  │
│ j k  ↑ ↓   row                a    answer / ask this agent             │
│ ⇥ ⇧⇥       next / previous    r    review its diff                     │
│ 1-9        tile by number     f    side fix from it                    │
│ g G        first / last       i    info for this pane                  │
│ u          next blocked                                                │
│ [ ]        page               view                                     │
│                               ?    this help                           │
│                               ^L   refresh now                         │
│                               q    close   ·   esc close overlay       │
╰ any key closes ────────────────────────────────────────────────────────╯
```

**Generate it from the keymap table, not from a second list.** One
`const KEYS: [(Key, &str, Group); N]` feeds both the dispatch `match` and this overlay.
gitui derives its help from the same `CommandInfo` registry that drives the command bar
(`src/popups/help.rs:187-195`); television derives its help rows from the live keybinding
config (`screen/help_panel.rs:228-258`). bottom hardcodes a `[&str; 24]`
(`src/constants.rs:29-53`) and that is the one to avoid — it is how a help screen ends up
describing a key that was renamed two releases ago. Align the description column at the
max key width across all rows (television `help_panel.rs:314-353`), not per group.

---

## 4. States

Exact text. All lower case, no exclamation marks, `cockpit:` prefix only on
`display-message` output (chrome.md §5), never inside the TUI where the frame already says
who is talking.

**Empty fleet** — centred, 62 columns:

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

**tmux server gone / not answering** — two stages, never an instant blank screen. Until
30 s: keep the last-known tiles and mark the header `· stale 34s` in `error`. After 30 s
or three consecutive failures, replace the grid:

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

**Run outside tmux** (`$TMUX` unset and no `COCKPIT_TMUX`) — **no TUI at all**. Print to
stderr and exit 1:

```
cockpit-tui: no tmux server — run this from inside tmux, or set COCKPIT_TMUX
```

A usage error must not raise a full-screen app the user then has to work out how to leave.
(And note ratatui.md's trap: ratatui draws to **stdout**, so a caller doing
`act=$(cockpit-tui board)` swallows the UI. The action comes back through the
`COCKPIT_ACTION` file, as the demo already does.)

**No transcript for a pane** — the tile still renders; the preview is the fallback and is
usually enough. `doing` reads `—` in `meta`; `said` is omitted (not printed empty). Never
the word "unknown".

**Permission denied on a transcript** — `doing` reads `transcript unreadable` in `meta`,
once, for that tile only. Log the errno to
`${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/tui.log`. One unreadable file must never
take the screen.

**Loading** — draw a frame **before** the first fetch, so the popup is never a black
rectangle while `claude agents --json` takes its measured 380 ms:

```
╭ board ─────────────────────────────────────────────────────╮
│                                                            │
│ reading the fleet …                                        │
│                                                            │
╰ q close ───────────────────────────────────────────────────╯
```

A braille spinner `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` at 10 Hz appears **only after 1 s of waiting**, prefixed to
that line. 380 ms does not need a spinner; 4 s does. This is the only animation in the
binary.

**Non-blocked tile, `a` pressed** — a one-line message in the footer row, in `meta`, for
2 s, replacing the key hints:

```
 work:2 is not waiting on you — ↵ jumps there, /btw asks it while it works
```

---

## 5. Colour and glyph discipline

### 5.1 Roles applied

screens.md §0's nine roles cover the board with nothing added.

| element | role | notes |
|---|---|---|
| tile border, plain | meta | Overlay0 |
| tile border, focused | accent | plus `BorderType::Thick` |
| tile border, needs-input | needs | red wins over accent |
| tile border, error | error | peach |
| `▎` active marker | accent | |
| tile number | accent when focused, meta otherwise | |
| state glyph | per state: needs / error / running / done / meta(idle) | chrome.md §1's ramp |
| review mark ◆ | **sky**, not yellow | chrome.md defect C1: ◆ and ● are the same yellow today; "working" and "finished, unread" must not share a colour |
| review mark ↵ (pasted) | meta | |
| `where` | meta | |
| title | value; **meta** when state is done or idle | the dimming signal |
| `doing` | value; **needs** when blocked | |
| age rail | meta | |
| `said` | quote | Subtext0 |
| preview | raw ANSI, untouched | plus `DIM` on the base style for done/idle tiles |
| header counts | label; the `N needs you` clause in needs | |
| stale marker | error | |
| page indicator | meta | |
| footer keys / verbs | accent / meta | |
| overlay border | needs (triage), meta (help, info) | |
| `reply>` | accent | matches fzf's `fleet> ` and bash's `ask> ` |

Two additions to chrome.md's ramp that the board needs and the status line does not:
`▲` is **bold** on a blocked tile (closing defect C2, which is that `▲` is bold in the
fleet and not in the tabs), and done/idle tiles get `Modifier::DIM` on the preview's base
style. DIM composes with the ANSI foregrounds underneath it; a terminal that ignores DIM
still gets the meta-coloured title, which is why the dimming is expressed twice.

### 5.2 Detection ladder

One `display-message -p` at startup packing every `@thm_*` plus `#{client_termfeatures}`
(theme.rs:57-72 already does two calls; make it one). **Never query the terminal** — a
query inside `display-popup` can wedge crossterm's parser.

```
client_termfeatures contains RGB  -> Color::Rgb from @thm_*      (theme.rs today)
else tput colors >= 256           -> Color::Indexed(N)           (theme.rs today)
else                              -> the 16 rung                 (missing)
```

The 16 rung, from screens.md §0's last column:

| role | 16-colour |
|---|---|
| label | `DarkGray` (2m) |
| value | `Reset` |
| quote | `Reset` |
| meta | `DarkGray` (90m) |
| accent | `Blue` (34m) |
| needs | `Red` (31m) |
| error | `Yellow` (33m) |
| running | `Yellow` (33m) |
| done | `Green` (32m) |

Peach folds into yellow and lavender into blue, so **error and running are the same colour
at 16 and accent and the review mark collide**. That is exactly why every state also
carries a glyph and why focus is a border weight. The 16 rung is where the shape-first
discipline pays for itself.

### 5.3 NO_COLOR

**Honour it in-app.** None of the eight surveyed Rust apps do (bottom sets it only in its
test harness, `tests/integration/util.rs:72`); all of them delegate to the terminal and
get it wrong under tmux, where the outer terminal's env is not what the popup sees.

```rust
if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) { /* every role = Reset */ }
```

Then: focus = thick border, state = glyph, blocked = the word in the bottom title,
done/idle = `DIM`. Nothing in the board becomes unreadable, and this is the case the
golden tests should pin, because it is byte-stable.

### 5.4 Glyphs

Default is the shipping BMP set. `COCKPIT_GLYPHS=ascii` (theme.rs:113-131 already reads
it) swaps glyphs **and borders**, because `BorderType::Plain` is box-drawing:

| meaning | unicode | ascii |
|---|---|---|
| needs-input | ▲ | `!` |
| error | ✕ | `x` |
| running | ● | `*` |
| done | ✓ | `+` |
| idle | ○ | `.` |
| review pending / sent | ◆ / ↵ | `#` / `<` |
| active pane | ▎ | `>` |
| ellipsis | … | `...` |
| border, plain | `┌─│┘` | `+-|+` |
| border, focused | `┏━┃┛` | `#=#|` |

The ellipsis change is not cosmetic: the truncation reserve goes from 1 cell to 3 and the
budget must subtract `ellipsis.width()`, not 1. The demo's `trunc` (data.rs) counts
`chars()` and assumes one; both are bugs to fix before the ascii tier ships.

The same board, `COCKPIT_GLYPHS=ascii`, 80 × 24, byte-identical column widths:

```
 board . 5 agents . 1 needs you                             1-4 of 5   page 1/2
+ 1 !  henosis:1 . Openvpn setup ----------------------------------------------+
|permission: Bash(sudo openvpn --config /etc/openvpn/work.conf --daem...   4m12|
|Bash command                                                                  |
|sudo openvpn --config /etc/openvpn/work.conf --daemon                         |
|Do you want to proceed?                                                       |
+- < answer -------------------------------------------------------------------+
#>2 *# henosis:3 . Implement with test review and rig =========================#
#Bash: shellcheck on cockpit.tmux and tests/run.sh                          32s#
#$ make check                                                                  #
#1001 assertions, 0 failures                                                   #
#$ shellcheck cockpit.tmux tests/run.sh                                        #
#==============================================================================#
+ 3 *  work:2 . Fleet gate review ---------------------------------------------+
|Agent: whole-branch review of phase 1                                     4m12|
|o Agent (fleet-gate-review)                                                   |
|  Read tests/fleet.sh, 214 lines                                              |
+------------------------------------------------------------------------------+
+ 4 +< work:4 . Left click issue ----------------------------------------------+
|idle . handover pending                                                    12m|
|  + pushed to origin/fix/left-click                                           |
|  PR #28 opened                                                               |
+------------------------------------------------------------------------------+
 < jump   a ask   r review   f fix   i info   ? help   q close
```

Focus still reads without colour (`#` vs `+`), and blocked still reads (`!` plus
`< answer`). This is the golden the fail-first tests should pin — it is the only tier that
is byte-stable across fonts.

---

## 6. Style guide for the crate

**Widgets, and only these:**

| need | widget |
|---|---|
| tile frame | `Block::bordered()` with `title` / `title_bottom` |
| header, footer | `Line` of `Span`s, rendered directly |
| tile header lines | `Paragraph`, **no `Wrap`** |
| preview | `Paragraph` of `ansi_to_tui`-parsed `Text`, **no `Wrap`** |
| overlays | `Clear` then `Block::bordered()` |
| reply input | a `String` + a cursor index, drawn as a `Line`; `frame.set_cursor_position` |

Not used, anywhere: `Table` (the fleet list stays fzf — that is the whole contract),
`List`/`ListState` (the grid is a `Layout`), `Tabs`, `Gauge`, `Sparkline`, `Chart`,
`Scrollbar` (paging instead), `Wrap` (banned, see below).

**Borders.** `BorderType::Plain` normal, `Thick` focused, `Rounded` for overlays (matching
`popup-border-lines rounded`). Never `Double` — it reads as an error frame and collides
with the ascii tier. Use `ratatui::symbols::border::Set` for the ascii tier rather than a
second widget path.

**Layout.** `Layout::vertical(...).areas()` destructuring, as the demo does
(board.rs:26, fleet.rs:167). `Constraint::Ratio(1, n)` for the grid, so the remainder
lands in the last cell. `Rect::centered(w, h)` for overlays — it is a built-in method now
(`ratatui-core/src/layout/rect.rs:556-563`); do not hand-roll the `centered_rect` helper
that gitui and television still carry.

**Padding.** None inside tiles — `Block::inner` is the padding. One leading and one
trailing space in every title so it never touches a corner. `Padding::horizontal(1)` on
overlays only.

**Truncation.** `…` (U+2026), `...` in ascii. Width-aware via
`unicode_width::UnicodeWidthStr::width`, never `chars().count()`. Three directions,
chosen by content:

- **right** for titles, `said`, header text (the head identifies it)
- **left** for paths and `repo⑂branch` (the tail identifies it) — gitui
  `src/strings.rs:426-439`
- **middle** for the `doing` tool line — `Bash: shellcheck on cockpit.tmux and
  tests/run.sh` loses its meaning from either end alone. atuin's `fit_middle`
  (`crates/atuin-ai/src/tui/view/trunc.rs:25-28`) is the model, including collapsing
  whitespace first and subtracting the fixed prefix's width before ellipsizing.

**Theme.** One `Theme` struct, `Copy`, loaded once (theme.rs today), plus `Theme::fixed()`
returning fixed `Indexed` values so goldens never move with the user's palette. No style
constants outside it. No per-widget colour literals — grep for `Color::` should hit
`theme.rs` and nothing else.

**Event loop.** The sidebar's (`src/app.rs:28-128`): a `needs_redraw` flag consumed at the
top; `poll(min(deadlines, 100ms))`; drain the whole input burst with `while poll(0)`
before one `draw`; hash rows + previews and skip the draw when unchanged. SIGUSR1 raises
SIGWINCH to end the poll (push.rs). No tokio, no `notify`, no control mode.

**Do not:**

1. No `Wrap` on the preview, ever (ratatui #2679 panics on double-width graphemes; and it
   folds 200-column pane lines into `↳` fragments — the #58 audit found the same in fzf).
2. No nested `Block` inside an overlay's border. One frame, then text.
3. No animation except the loading spinner (after 1 s) and, if ported, the countdown.
   recon repaints a pulsing border every tick to alternate a colour (`src/view_ui.rs:474-478`)
   — that is CPU and flicker for decoration.
4. No mouse capture in phase 1 or 2.
5. No terminal queries; pin `crossterm = "=0.29.0"`.
6. No `Table` for the fleet. The demo's Table version has no filter input at all, which is
   precisely what fzf gives for free.
7. No sort key that contains a clock. Tiles must not reorder while you read them.
8. No tile geometry that depends on the current page's occupancy.
9. No `printf` to stdout outside `draw` — ratatui owns stdout; the action leaves through
   `COCKPIT_ACTION`.
10. No `chars().count()` for a width budget.
11. No second copy of the key list. The help overlay reads the dispatch table.
12. No new dependency beyond ratatui, crossterm, unicode-width, unicode-segmentation,
    ansi-to-tui, serde_json, libc. No clap, no anyhow, no color-eyre, no tokio.

---

## 7. Three apps to copy, with lines

**1. bottom — the grid and its focus movement.** The only surveyed app with real spatial
navigation between non-uniform tiles. `src/app/layout_manager.rs:39-90`
(`get_movement_mappings`) precomputes, once, each widget's left/right/up/down neighbour
from a `BTreeMap<LineSegment, …>` of percentage spans, using `is_intersecting`/
`get_distance` (:42-57) — proper nearest-neighbour, not "next in a Vec".
`src/app.rs:1288-1333` (`move_widget_selection`) looks the neighbour up and skips hidden
widgets. Copy this for `h/j/k/l` the moment the grid stops being uniform (a last row with
one tile already is). Focused-border construction:
`src/canvas/components/data_table/draw.rs:74-92` picks `highlighted_border_style` vs
`border_style`; `src/canvas/drawing_utils.rs:33-51` (`widget_block`) shows the
constrained-space variant that drops the border entirely when unselected — the fallback if
three columns of borders ever look noisy. What **not** to copy: the hardcoded
`GENERAL_HELP_TEXT: [&str; 24]` at `src/constants.rs:29-53`.

**2. gitui — the footer that measures itself, and truncation.** `src/cmdbar.rs:63-77`
(`is_multiline`) sums `UnicodeWidthStr::width` per command against the bar width;
`:79-114` (`refresh_list`) inserts line breaks and reserves 9 columns for a `more [.]`
indicator; `:139-198` bails out entirely below a minimum width instead of drawing a lie.
That is the model for §1.6's two-stage footer (television's
`screen/status_bar.rs:118-128` is the shorter version of the same idea, and the one I would
actually write: measure, drop to help-only, drop to nothing). Help derived from the command
registry: `src/popups/help.rs:187-195`. Popup: `:36,41-51` — a fixed 65×24
`centered_rect_absolute` + `Clear` + `BorderType::Thick`; confirm dialogs use Enter/Esc,
not y/n (`src/popups/confirm.rs:28-48`). Focus by dimming the unfocused rather than
brightening the focused: `src/ui/style.rs:46-62`. Ellipsis constant:
`src/strings.rs:49`, `ELLIPSIS: char = '\u{2026}'`; front-truncation for paths at
`:426-439`.

**3. tmux-agent-sidebar — everything tmux-shaped.** The closest analogue in existence.
Event loop `src/app.rs:28-128` (dirty flag, `poll(min(deadlines, 16ms))`, SIGUSR1
`AtomicBool`, a `window_inactive_count` backoff that halves the tmux reload rate when the
window is not focused — worth copying verbatim for a board left open all day). Two focus
signals, the single best UI finding of the survey: `src/ui/panes/row.rs:38-64` renders a
full-row background for "the cursor is here" and a separate `▎` accent marker for "tmux's
active pane is here", and keeps the marker off the deeper rows. tmux queries:
`src/tmux/panes.rs:28-48`, `src/cli/capture/tmux_probe.rs:43-84` (`capture-pane -p -e -t`).
ANSI → styled cells with the `vte` crate, not a hand-rolled SGR parser:
`src/cli/capture/ansi.rs:1-60` — relevant if `ansi_to_tui` ever proves lossy. Content-sized
popups that **abort rather than draw broken** below a floor:
`src/ui/notices/popup.rs:145-173`. Theme from tmux options as `Color::Indexed`:
`src/ui/colors.rs:5-49`.

Honourable mentions: **television** `screen/help_panel.rs:228-258` + `:314-353` (help rows
derived from the live keymap, description column aligned at the global max key width) and
`screen/layout.rs:488-509` (`preview_fits` — auto-collapse the preview below
`MIN_PREVIEW_HEIGHT`/`WIDTH`, which is exactly §1.5's density ladder). **workmux**
`src/command/dashboard/app/agents.rs:307-320` — the only real inline reply among fourteen
agent dashboards.

What no one has: **none of the fourteen agent dashboards renders a live pane preview
inside a tiled grid.** ntm has adaptive cards but fills them with metadata badges
(`internal/tui/dashboard/dashboard.go:4031`, `4233-4238`); recon has a true 2×2 nested
`Layout` grid but fills the rooms with pixel-art sprites (`src/view_ui.rs:423-447`,
`462-537`). Both chose structured content over pane text at card width. That is the design
risk to plan around, and the density ladder plus the tail-of-visible-screen preview rule
are the answer: at 48 columns a preview line is a fragment, which is why the *header* two
lines carry the meaning and the preview only carries the texture.
