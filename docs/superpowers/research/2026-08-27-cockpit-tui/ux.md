# cockpit-tui: the board, the triage, the keymap

UI/UX lane, 2026-08-27, read-only. Designs phase 1 (the board) and phase 2 (needs-you
triage) of the Rust/Ratatui binary against screens.md §0's grid, colour roles and glyph
tiers, chrome.md §1's urgency ramp, and ratatui.md's measured constraints. Every mockup
below is generated at an exact column count (`scratchpad/mock.py`, `over.py`), not
hand-drawn; all lines verified at 80/120/160. Both generators were session-local scratch
and are not committed anywhere, so **the mockups here cannot be regenerated** — they are
a record, and an edit to one is a hand edit whose column count nothing re-checks.

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

**Preview**: `capture-pane -p -e -J` on the pane's *visible screen* (no `-S`), drop
trailing blank lines, show the **last** `h` lines. `-J` matters: bosun documents it
(`src/tmux/client.rs:292-305`) as the flag that joins wrapped lines so an ANSI sequence is
not split across the wrap point and handed to the parser in halves. The demo shows the head (board.rs:99); the tail is right — for a busy
agent it is the newest output, for a blocked one it is the permission prompt, for an idle
one it is the composer, and all three are what you would see if you jumped there. It also
fixes competition.md anomaly 3 (fzf's `tail` looked blank because the fake panes were
padded with blanks; dropping trailing blanks is the fix, not switching to head).

Preview lines are **clipped, not truncated and not wrapped** — no `…`, no `Wrap`. A
`Paragraph` with no `Wrap` clips at the widget edge for free; `Wrap` on arbitrary terminal
output hits ratatui #2679 (panic on double-width graphemes); and bosun reached the same
rule from the other direction — "Lines wider than the area are clipped at the right edge…
Wrapping throws off the scroll math" (`src/ui/preview.rs:237-249`).

**Cadence: 1 Hz for every visible tile, 200 ms for the focused one only.** This is bosun's
(`src/actors/tmux_actor.rs:160,178`), and it is not a taste call — `:772-793` records why
they cut per-session fast capture from `1 + N` execs per tick to `1 + 1`: on macOS every
exec goes through Gatekeeper and pins `syspolicyd`. Capture **only visible tiles** (ilmari
gates its captures behind `AdapterRegistry::needs_output_tail`, `src/tmux.rs:399`, for the
same reason). The failure mode is live in this field: crmux spawns one `capture-pane` per
visible tile on every ~50 ms loop tick regardless of whether anything changed
(`src/app.rs:547,578,691`) — a 3×2 grid is six execs every 50 ms, forever.

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

Two other apps arrived at border weight independently: mprocs (`console/ui_term.rs:15-19`,
`BorderType::Thick` if active else `Plain`, plus a bold title) and anthropometer's
system-wide tile grid (`viz.rs:4595-4610`, Thick vs Plain plus colour and a bold title).
thurbox takes the opposite line and writes it down — "Focus is communicated by COLOUR,
never by a marker glyph or a heavier border" (`ui/plugins/20_agent.lua:183-189`). I am
against thurbox here, and its own code is the argument: the same repo shape-codes its state
glyphs `◐ ◆ ● ○ ✗ ⊘` explicitly for colourblind safety (`src/session/mod.rs:167-176`). If
colour cannot be trusted to carry state it cannot be trusted to carry focus either — and on
cockpit's 16-colour rung, accent collapses into blue alongside the review mark.

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
3. The age rail turns `error` past 60 s and `needs` past 300 s. claudectl escalates on the
   same two thresholds (`ui/table.rs:454-467`); "blocked 4m12s" in meta reads as a fact, in
   red it reads as a queue.
4. The bottom border carries ` ↵ answer `. Not "needs you" — the ▲, the red border and
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
h <  5 -> compact, rows_shown = (height-2)/5, and the grid SCROLLS BY WHOLE ROWS
```

The formula is not novel, and that is the point — two apps in this niche derived the same
shape independently. agent-console:
`let columns = usize::from((area.width / 36).clamp(1, 3)).min(item_count);`
(`src/main.rs:906`) is this expression with a different constant. crmux (`src/ui.rs:23-31`):

```rust
pub fn compute_grid(n: usize, available_width: u16, min_col_width: u16) -> (usize, usize) {
    let cols = (available_width / min_col_width).max(1) as usize;
    let cols = cols.min(n);
    let rows = n.div_ceil(cols);
    (cols, rows)
}
```

with `MIN_PANE_WIDTH: u16 = 80` (`src/ui.rs:17`) and no cap. Two deliberate divergences
from crmux: 48 rather than 80, because a cockpit tile's two header lines carry the meaning
and the preview only carries texture, so it survives half the width crmux needs; and a cap
of 3, because past three columns the preview is a fragment. The cap only bites above 192
columns.

Two rules that are not obvious and matter:

- **`cols = min(cols, n)`** — never leave a whole empty column; two agents on a
  160-column board get two 80-wide tiles, not two 53-wide ones with a third of the screen
  blank.
- **Tile height is `avail / rows_shown` from the density ladder, never fitted to the rows
  currently on screen.** Scrolling to a final row holding one agent must render one normal
  tile and blank space, not one tile 22 rows tall. (I hit this in the generator while it
  still paged; it is why row geometry is pinned rather than fitted.)

Within a row, the last cell absorbs the remainder so the right edge is flush; a row with
fewer tiles than columns leaves the trailing cells **empty**. An empty cell is cheaper
than a tile that changes width when a sixth agent appears.

**Scroll by whole rows; never shrink past the density floor, never cut a tile.** The field
has four answers to "what happens when the fleet outgrows the screen", worth laying side by
side:

| app | strategy | citation |
|---|---|---|
| crmux | shrink tiles forever, no scroll, no page | `src/ui.rs:23-31,477-501` |
| agent-console | fixed 7-row card, auto-scroll by row to keep the selection visible | `src/main.rs:900,908-910` |
| anthropometer | clamp tile height to 5..12, scroll whole tile-rows | `viz.rs:4500-4584` |
| iris-tui | collapse a group of ≥4 to one summary row `⚠12 ●3 ✓1 ○8` | `src/app.rs:109`, `src/ui.rs:543-591` |
| **cockpit** | **density ladder, then scroll whole rows** | this doc |

crmux's answer is the one to reject: at nine agents its tiles are a third of a screen each
and the preview is noise. The density ladder is the first two rungs of iris-tui's idea (drop
`said`, then drop rows) without needing a second row type. Scrolling by **whole** rows, not
pixels, is anthropometer's and agent-console's; it means `j` on the bottom row reveals the
next row instead of doing nothing, and it keeps tile height a constant
(`avail / rows_shown`), so nothing resizes as you scroll. `[` and `]` jump a screenful. No
`Scrollbar` widget — the header's `▲3 ▼4` counts are the indicator, which is thurbox's
(`ui/plugins/10_sessions.lua:732-736`).

Sorting needs-first and scrolling from the top means **the first screen is the triage
screen** with no special case.

| n | 80×24 (1 col) | 120×30 (2 cols) | 160×24 (3 cols) |
|---|---|---|---|
| 1 | 1 tile, full height — a live mirror of that pane | same | same |
| 2 | 2 tiles stacked, h=11, full | 2 tiles side by side, full | 2 tiles, 80 wide each (`min(cols, n)`) |
| 5 | compact, 4 visible, `▼1` | full, 3 rows (last row half empty) | full, 2 rows (one gap) |
| 9 | compact, 4 visible, `▼5` | compact, all 9 visible | **full, 3×3, all visible** |
| 16 | compact, 4 visible, `▼12` | compact, 10 visible, `▼6` | compact, 12 visible, `▼4` |

At 1 agent the tile is as tall as the board and the preview asks `capture-pane` for
`h-4` lines, so a tall tile shows *more of the pane*, not more blank. The board at n=1 is
a legitimate "watch this one" view, not a degenerate case.

### 1.6 Header and footer

Header, one line, no border:

```
 board · 5 agents · 1 needs you                                   1-4 of 5   ▼1 
```

Left: counts, reusing `fleet.rs:83-95`'s `counts()` (agents · N needs you · N review
pending). iris-tui does the same with a red-on-black `pending {n}` chip when nonzero
(`src/ui.rs:454-459`), agent-console with a full ramp
`● N working  ◐ N waiting  ○ N idle  × N failed` (`src/main.rs:769-808`). Right: the visible
range and `▲N ▼N` hidden-row counts, **printed only when something is off-screen**. No spinner: the
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

`a ask` is a key hint, not one of §3's action verbs — the same line carries `? help` and
`q close`, which are not verbs either. Until phase 2, `a` resolves to jump: on a blocked
tile it emits `jump %N`, and on any other tile it emits nothing and says so in a note
(§3.1; `tui/src/board.rs:919-926` on master, the footer itself at `:180`).

Keys in `accent`, verbs in `meta`, three spaces between pairs — screens.md §0's key-hint
convention, pickers.md:161's ordering rule (leftmost = least skippable). **Drop whole pairs,
never truncate a label**: thurbox states the rule better than I can — "half a label is not a
smaller button, it is an unreadable one" (`ui/bands.rs:287-291`). p2pmux has the
implementation shape: `NORMAL_FOOTER_TIERS`, candidate footers fullest to sparsest, pick the
widest that measures under the width (`tui/render/footer.rs:21-104,300-303`).

### 1.7 The board at 80 × 24, 5 agents — compact, one row hidden below

```
 board · 5 agents · 1 needs you                                   1-4 of 5   ▼1 
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

Scrolled to the last row — the tile keeps its size:

```
 board · 5 agents · 1 needs you                                   5-5 of 5   ▲4 
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

Sixteen agents at 160×24: compact, 12 visible, `1-12 of 16   ▼4` on the header right, tile numbers 10-12 in the last row (the number field is 1-2 characters, budgeted).

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
| `1`-`9` | select the tile whose printed number is N (absolute; the number you see is the key you press, tiles past 9 are not key-addressable) |
| `g` `G` | first / last tile |
| `u` | select the next needs-input tile (mirrors `prefix u`, the repo's jump key) |
| `[` `]` `PgUp` `PgDn` | scroll a screenful (only when rows are hidden) |
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

The field is split on `1`-`9`: crmux binds them to direct selection
(`src/event_handler.rs:214-223`, `MAX_NUMBER_KEYS: usize = 9`), bosun to *move this session
to section N* (`src/ui/modal/help.rs:329-446`). Take crmux's.

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
│ j k  ↑ ↓   row                a    answer / ask (phase 2; jump in 1)   │
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

**Corrected by measurement — there is no ladder.** verified.md tmux A graded this
REFUTED, backwards, and rule 27 was rewritten: emit `Color::Rgb` from `@thm_*`
**unconditionally** inside tmux. tmux does the downgrade itself, per attached client,
with a real nearest-colour fit — 256-indexed is what survives a 256-colour client and
truecolor is what gets downgraded without RGB, and the inner `TERM` has no effect at
all. So rung 1 is redundant, rung 3 is unimplementable (a 16-colour client reports
`bpaste,focus,title`, no colour feature to test against), and one
`#{client_termfeatures}` query picks one client when two of different capability are
attached. Keep the startup `display-message -p` for `@thm_*`; `client_termfeatures`
is at most an optimisation. `NO_COLOR` is still honoured in-app (§5.3).

~~The 16 rung~~, kept only as the record of what was dropped, from screens.md §0's
last column:

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
carries a glyph and why focus is a border weight — and that argument is untouched by the
correction above: tmux still collapses to 16 for a 16-colour client, it just does it
itself, downstream of us, so the binary carries no 16 rung of its own.

### 5.3 NO_COLOR

**Honour it in-app.** None of the eight surveyed Rust apps do (bottom sets it only in its
test harness, `tests/integration/util.rs:72`); all of them delegate to the terminal and
get it wrong under tmux, where the outer terminal's env is not what the popup sees.

```rust
if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) { /* every role = Reset */ }
```

Then: focus = thick border, state = glyph, blocked = the word in the bottom title,
done/idle = `DIM`. Nothing becomes unreadable, and this is the case the golden tests should
pin, because it is byte-stable.

**And it must be in-app for a second reason I had wrong.** p2pmux found that crossterm's own
`NO_COLOR` support emits an *empty* SGR, which resets bold, dim and reverse along with the
colours — so delegating to crossterm would silently destroy exactly the three attributes
this fallback is built on (`tui/terminal.rs:79-99`, `keep_attributes_through_no_color()`,
regression-tested at `:236-245`). Set the roles to `Reset` ourselves and keep crossterm's
handling off. p2pmux is the only app of the twenty surveyed that handles `NO_COLOR` at all,
and the only one that found this.

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
 board . 5 agents . 1 needs you                                   1-4 of 5   .1 
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
    (`unicode-width` is the one the owner ruled on afterwards, against awesome.md's
    rejection of it: accepted, and free — ratatui 0.30.2 already pulls 0.2.2.)
13. No `Modifier::SLOW_BLINK`. reeve puts its most important alert (health < 20) on it
    (`panels/left.rs:260-263`); many terminals ignore or mangle it, so the loudest signal is
    the least reliable one.
14. No LLM-generated tile content. agent-console's cards look like live previews and are a
    Haiku-class paraphrase behind a 90-second timeout (`src/summary.rs:23`) — one agent's
    second-hand account of another's output, with its own latency and failure mode. The
    preview must be the pane.
15. No re-deriving what the hooks already know. ilmari publishes a sticky attention bit into
    tmux as `@ilmari_attention` and reads it back in `list-panes -F` (`src/tmux.rs:30`,
    `src/model.rs:174-176`) — right design, and cockpit already has it as `@panestate`.

---

## 7. The reference set

Primary: the six Ratatui apps from awesome-ratatui that are in cockpit's exact niche, plus
two more from the same AI-agent section that turned out to matter more than four of the six
(`amtr`/anthropometer and `ilmari`). Secondary: the generic multi-panel apps.

### 7.1 The eight, at a glance

| app | main view | live preview | state from | needs-you | inline reply |
|---|---|---|---|---|---|
| **crmux** | **true tile grid** | `capture-pane -p -e` → ansi-to-tui | pane-text regex (3rd-party crate) | border tint + sine pulse; **no sort** | **yes**, `tmux send-keys` |
| **bosun** | list + one preview | `capture-pane -p -e -J`, and a real PTY via `vt100`+`tui-term` | `#{pane_title}` / `#{pane_current_command}` first, pane text last | glyph only; no sort, no count | attach (PTY), no composer |
| **agent-console** | **card grid** (1-3 cols) | **none** — LLM summaries | own PTYs + transcript; **not tmux** | `"NEEDS YOU"` label + header counts | attach |
| **claudectl** | flat `Table` | none | CPU% + JSONL heuristics | **sorts NeedsInput first, longest wait first**; colour escalates 60s/300s; OS notify | **yes**, `i` → `tmux send-keys` |
| **iris-tui** | list + detail | transcript tail | JSONL + an approval hook | sorts pending first; red `pending {n}` chip | modal allow/deny |
| **thurbox** | list + one live PTY | `vt100` + `tui-term`, full screen grid | **CLI hooks + OSC 2/9/777** | colour + OS notify; **sort is alphabetical only** | type into the PTY |
| **anthropometer** | memory map; **tile grid** in system-wide view | none (IPC from a parser) | engine-side | none | none |
| **ilmari** | list ("radar") | none | pane-text classifiers | **sticky attention published into tmux as `@ilmari_attention`** | none |

Two facts fall out that shaped this design more than anything else:

**Only crmux, agent-console and anthropometer draw a grid at all, and only crmux puts live
pane text in the cells.** Across twenty surveyed dashboards, cockpit's board would be the
second implementation of "a grid of tiles each showing a live agent pane", not the first —
but the only one that also sorts by who needs you.

**Not one of the eight combines a live grid with needs-first ordering.** crmux has the grid
and sorts by project name (`src/state.rs:557`). claudectl and iris-tui sort correctly and
have no grid. thurbox sorts alphabetically and says so. That gap is the board's whole
argument.

### 7.2 crmux — the closest thing that exists

The grid, reconstructed from its source constants at 160×24 (`compute_grid` `src/ui.rs:23-31`,
`MIN_PANE_WIDTH=80` `:17`, `Ratio(1,rows)`/`Ratio(1,items)` `:477-501`, `SELECTED_ICON "> "`
`:53`, footer `:250`, sort `state.rs:557`). This is a reconstruction, not a capture — I did
not build or run it:

```
┌> cockpit %14 Working ────────────────────────────────────────────────────────┐┌fleet %21 Working ────────────────────────────────────────────────────────────┐
│                                                                              ││                                                                              │
│                                                                              ││                                                                              │
│                                                                              ││                                                                              │
│                                                                              ││                                                                              │
│                                                                              ││                                                                              │
│$ make check                                                                  ││⏺ Agent (fleet-gate-review)                                                   │
│1001 assertions, 0 failures                                                   ││  Read tests/fleet.sh, 214 lines                                              │
│$ shellcheck cockpit.tmux                                                     ││                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘└──────────────────────────────────────────────────────────────────────────────┘
┌leftclick %30 Idle ───────────────────────────────────────────────────────────┐┌openvpn %9 WaitingForApproval ────────────────────────────────────────────────┐
│                                                                              ││                                                                              │
│                                                                              ││                                                                              │
│                                                                              ││                                                                              │
│                                                                              ││                                                                              │
│                                                                              ││                                                                              │
│  ✓ pushed to origin/fix/left-click                                           ││Do you want to proceed?                                                       │
│❯                                                                             ││❯ 1. Yes  2. Yes, don't ask again                                             │
│                                                                              ││  3. No                                                                       │
└──────────────────────────────────────────────────────────────────────────────┘└──────────────────────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ | hjkl/1-9/t:Nav C-u:ScrollUp s:Switch(leave crmux) Space:Mark v:Layout i:Input o:Claudeye ?:Help q:Quit                                                     │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
                                                                                                                                                                
```

Put beside §1.9's board at the same 160×24, five differences are visible without reading
any code, and each one is a decision in §1:

1. **Two columns where cockpit gets three** — `MIN_PANE_WIDTH = 80` against cockpit's 48.
   crmux needs 80 because the cell is only a pane tail; cockpit's two header lines carry the
   meaning, so the tile survives half the width.
2. **The blocked pane is bottom-right**, because the sort key is the project name. Cockpit
   sorts it first.
3. **Five blank rows at the top of every tile** — the tail is pinned to the bottom
   (`ui.rs:521-529`) and short output leaves the rest empty. Cockpit drops trailing blanks
   and fills from the top of the preview area.
4. **No state glyph and no counts** — the state is a word in the title and a border tint,
   so at a glance there is nothing to count and nothing that survives greyscale.
5. **Selection is a `> ` prefix**, which shifts the title text by two columns when you move.
   Border weight does not move anything.

Copy from crmux: `compute_grid` verbatim (§1.5), `ansi_to_tui::IntoText` for the cell
(`ui.rs:517-518`), and the inline reply path — `InputMode::Input`/`Broadcast`
(`state.rs:364-375`) with the mode banner `"-- INSERT -- | Keys sent to selected pane via
send-keys. Esc:Back"` (`ui.rs:252-258`), keys through `send_key_to_pane`
(`event_handler.rs:448-465`). Its `1`-`9` direct-select is `event_handler.rs:214-223`.

Do not copy: the sine-wave border pulse for waiting states (`ui.rs:64-89`) — animation on
the alert, banned in §6; the ~50 ms capture loop (§1.1); and `ClaudeState::Idle // no
Unknown state` (`claude_state.rs:111`), which silently reports detector confusion as "idle".

### 7.3 claudectl and iris-tui — the triage half

**claudectl** has the sort I want, written out: `sort_key()`
(`crates/claudectl-core/src/session.rs:32-41`) with `NeedsInput` first and longest-waiting
first inside the bucket, applied at `app/mod.rs:741-751`. The wait-time colour ramp at
60 s / 300 s is `ui/table.rs:454-467` (adopted, §1.3). The inline reply is `i` →
`status_bar.rs:46-57` → `terminals/tmux.rs:56-79`. OS notification with a 30 s cooldown:
`helpers.rs:105-118`. Do not copy: fixed columns summing to 112 with **no reference to
`area.width` anywhere in `ui/`** — the same bug screens.md §2 already fixed in the fleet row;
`q` and `Esc` both hard-quitting with no confirmation; and a blocking `ps` spawn on the draw
thread every 2 s (`process.rs:13`).

**iris-tui** has the comparator (`app.rs:558-566`: pending approval, then status rank, then
mtime) and the header chip (`ui.rs:454-459`, red-on-black `pending {n}`, adopted in §1.6).
Its approval modal (`draw_approval_popup`, `ui.rs:171-246`) is the allow/deny shape of §2.1
without the free-text line — which is exactly why cockpit needs the y/n-only-when-empty rule
in §2.3. Its `GROUP_AUTOCOLLAPSE = 4` (`app.rs:109`) rendering `⚠12 ●3 ✓1 ○8` is the density
answer past the point where tiles stop being worth drawing; worth keeping in the back pocket
for a 30-agent fleet. Do not copy: a fixed 20-second `mtime` cutoff deciding idle
(`session.rs:185-198`), which reports a session 21 s into a slow Bash call as idle — cockpit
has hooks and does not need to guess; and a 40 ms redraw with no dirty check.

### 7.4 bosun and thurbox — the engineering

**bosun** is the best-built of the six and the closest to cockpit's constraints. Take:
`capture-pane -p -e -J` with the documented reason for `-J` (`src/tmux/client.rs:292-305`);
the 1 Hz-all + 200 ms-focused cadence and the exec-budget note behind it
(`src/actors/tmux_actor.rs:160,178,772-793`); the no-wrap preview rule with its rationale
(`src/ui/preview.rs:237-249`); the two focus signals — a perimeter `draw_focus_border`
(`src/ui/mod.rs:153-166`) for "keys go here" against a `▌` row marker + selection background
(`src/ui/session_list.rs:244-265`) for "the cursor is here"; and `PREVIEW_MIN_WIDTH = 80`
(`src/ui/layout.rs:12`) as precedent for dropping a component rather than squeezing it.
State detection is the one to study: `src/tmux/detector/claude.rs:6-12` prefers
`#{pane_title}` and `#{pane_current_command}` over pane text, with hysteresis
(`src/util/hysteresis.rs`) so a spinner frame cannot chatter the sidebar. Do not copy: the
hand-maintained second key list in `src/ui/modal/help.rs:329-446`, and the absence of any
aggregate attention count — with a dozen sections one blocked session scrolled off-screen has
nothing pointing at it.

**thurbox** contributes three rules and one warning. The rules: footer pills drop whole, never
truncate — "half a label is not a smaller button, it is an unreadable one"
(`ui/bands.rs:287-291`); the help overlay is generated from the plugin keymap registry
(`src/kernel/modals/help.rs:76-90`) with a documented exception list, which is §3.4's rule
with the escape hatch I did not think to name; and one global 8 Hz animation clock driving the
braille spinner (`ui/lib/theme.lua:100,118-121`, `src/host/mod.rs:442`) rather than each
widget keeping its own. Its state glyphs `◐ ◆ ● ○ ✗ ⊘` are shape-coded for colourblind safety
with the rationale at `src/session/mod.rs:167-176`. The warning: it computes full cost and
token metrics every frame (`session/mod.rs:219-272`) and displays them nowhere, and its only
sort is alphabetical (`ui/lib/order.lua:170-177`) — a blocked agent can sit below the fold
behind a single missed OS notification. Also note `Tab` is deliberately unbound because agents
inside the panes need it (`src/kernel/modals/help.rs:39-44`); cockpit's board hosts no live
PTY, so `⇥` is free — but that stops being true the day a tile becomes interactive.

### 7.5 agent-console, anthropometer, ilmari — one idea each

**agent-console**: the column formula, `let columns = usize::from((area.width /
36).clamp(1, 3)).min(item_count);` (`src/main.rs:906`) — independently the same shape as
§1.5, cap and `min(n)` included; the fixed card height `SESSION_CARD_HEIGHT: u16 = 7`
(`:900`) and row-wise auto-scroll (`:908-910`); `session_priority()` returning
`("NEEDS YOU", question, Color::Yellow)` (`:933-949`), the same label §2 landed on; and the
header ramp `● N working  ◐ N waiting  ○ N idle  × N failed  ◆ N alert` (`:769-808`). Its
cards are LLM summaries behind a 90 s timeout (`src/summary.rs:23`) — banned in §6 item 14 —
and it owns PTYs rather than reading tmux, so it is not a model for anything below the UI.

**anthropometer**: the only app that does tile selection the way §1.2 proposes —
`BorderType::Thick` vs `Plain` plus colour and a bold title (`viz.rs:4595-4610`) — and the
only one whose grid growth strategy is the one adopted: `cols = (width/26).max(1).min(n)`,
tile height `clamp(height/grid_rows, 5, 12)`, scroll **whole tile-rows** via `row_off`
(`viz.rs:4500-4584`). It also has the best-argued negative result in the survey: reverse-video
selection on an already-solid `█` cell is a visual no-op (`viz.rs:792-801`), which is why a
background wash is not a general answer. Its `q` = global quit / `Esc` = local dismiss split
is the cleanest of the eight — and does not transfer to §2.3, because none of its overlays
takes free text and cockpit's reply line does. Its help browser (`viz.rs:4781-4792`,
searchable and sectioned, modelled on yazi and lazygit) is what §3.4 should grow into if the
keymap ever outgrows one card.

**ilmari**: publishes a sticky attention bit back into tmux as `@ilmari_attention` and reads
it in its own `list-panes -F` format (`src/tmux.rs:30`, `src/model.rs:174-176`), so attention
survives a restart and is independent of transient status. That is the right architecture and
cockpit already has it — `@panestate`, written by hooks. It also gates `capture-pane` behind
`AdapterRegistry::needs_output_tail` (`src/tmux.rs:399`) rather than capturing every pane.
One caution if anyone reads its sort: the doc comment at `app.rs:1604` says "waiting first,
then running" while `session_sort_bucket` returns `Running => 0, WaitingInput => 1`.

### 7.6 Generic patterns, from outside the niche

- **bottom** — spatial focus movement between non-uniform tiles: `src/app/layout_manager.rs:39-90`
  precomputes each widget's four neighbours from percentage spans with `is_intersecting`/
  `get_distance` (`:42-57`); `src/app.rs:1288-1333` walks them and skips hidden widgets. Copy
  for `h/j/k/l` the moment the grid stops being uniform — a last row with one tile already
  is. Focused-border selection: `src/canvas/components/data_table/draw.rs:74-92`,
  `src/canvas/drawing_utils.rs:33-51`. Do not copy `GENERAL_HELP_TEXT: [&str; 24]`
  (`src/constants.rs:29-53`).
- **gitui** — the footer that measures itself (`src/cmdbar.rs:63-77,79-114,139-198`, including
  bailing out entirely below a minimum width); help derived from the command registry
  (`src/popups/help.rs:187-195`); dim-the-unfocused rather than brighten-the-focused
  (`src/ui/style.rs:46-62`); `ELLIPSIS: char = '\u{2026}'` (`src/strings.rs:49`) and
  front-truncation for paths (`:426-439`).
- **tmux-agent-sidebar** — the event loop (`src/app.rs:28-128`: dirty flag, `poll(min(deadlines,
  16ms))`, SIGUSR1 `AtomicBool`, and a `window_inactive_count` backoff that halves the tmux
  reload rate when the window is not focused); two focus signals (`src/ui/panes/row.rs:38-64`);
  `capture-pane -p -e -t` (`src/cli/capture/tmux_probe.rs:72-84`); ANSI → cells with the `vte`
  crate (`src/cli/capture/ansi.rs:1-60`); popups that abort rather than draw broken below a
  floor (`src/ui/notices/popup.rs:145-173`).
- **p2pmux** — the `NO_COLOR` finding that corrects §5.3 (`tui/terminal.rs:79-99`, tested at
  `:236-245`); tiered footers, widest-that-fits (`tui/render/footer.rs:21-104,300-303`); and
  `vt100::Screen` cells drawn straight into a ratatui `Buffer` (`tui/render/vt.rs:20-38`), the
  template if a tile ever has to host real PTY output instead of a capture.
- **mprocs** — `BorderType::Thick` if active else `Plain` (`console/ui_term.rs:15-19`), the
  third independent arrival at §1.2; and `ScreenDiffer` (`term/screen_differ.rs:12-83`) as the
  shape of cheap live-panel updates. Its warning is that it hand-rolled a whole VT100 engine
  that the `vt100` crate provides.
- **oxker** — `should_redraw = rerender.swap_draw() || elapsed >= interval` (`src/ui/mod.rs:211-218`),
  a real dirty-flag plus throttle decoupled from the input poll; the braille spinner
  (`src/ui/gui_state.rs:158`) driven by a background task rather than the draw loop; `Clear` +
  centered `Block` popup helper (`draw_blocks/popup.rs:6-31`) with a table-driven key list.
- **television** — help rows derived from the live keymap with the description column aligned
  at the global max key width (`screen/help_panel.rs:228-258,314-353`); `preview_fits`
  (`screen/layout.rs:488-509`), which is §1.5's density ladder in miniature.
- **kmon** and **kwaak**, briefly, as counter-examples: kmon's `?` overwrites the current panel
  in place rather than overlaying (`src/app.rs:216-240`) and it full-redraws every 250 ms tick;
  kwaak re-clones and reformats the entire chat history every tick regardless of change
  (`chat_messages_widget.rs:14-24`), a cost that grows with transcript length.

### 7.7 The field's mistakes, collected

Worth reading once as a list, because four of them are one commit away in any board:

| mistake | who | cite |
|---|---|---|
| grid that never stops shrinking | crmux | `src/ui.rs:23-31` |
| one `capture-pane` exec per tile per 50 ms | crmux | `src/app.rs:547,578,691` |
| blocked agent sorted by name, not urgency | crmux, thurbox | `state.rs:557`, `ui/lib/order.lua:170-177` |
| tiles that look live but are LLM paraphrase | agent-console | `src/summary.rs:23` |
| fixed column widths, no `area.width` anywhere | claudectl | `crates/claudectl-tui/src/ui/` |
| idle decided by a fixed mtime cutoff | iris-tui | `session.rs:185-198` |
| the loudest alert on `SLOW_BLINK` | reeve | `panels/left.rs:260-263` |
| pulsing a border every tick for attention | crmux, recon | `ui.rs:64-89`, `view_ui.rs:474-478` |
| unknown state silently reported as idle | crmux | `claude_state.rs:111` |
| two glyph vocabularies, `●` meaning two things | anthropometer | `viz.rs:4767-4775` vs `:2899-2960` |
| help text hand-maintained beside the real keymap | bosun, claudectl, bottom | `modal/help.rs:329-446` |
| metrics computed every frame, displayed nowhere | thurbox | `session/mod.rs:219-272` |
