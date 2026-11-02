# Board and panel: UI/UX audit

Read-only audit of `cockpit-tui board` (`prefix w`) and `cockpit-tui panel %N`
(`prefix f`, or `f` on a board tile), master `d95607c`. Binary:
`~/.cache/cockpit-wt/design-build/release/cockpit-tui`. Every live capture below
came from a private `tmux -L aud<pid>` server, `COCKPIT_READONLY=1`, killed
afterwards. Agents are `bash feed.sh <kind>`, which prints a canned screen and
then runs `exec -a claude sleep`, with real `@panestate` / `@panestate_msg`
values. No agent has a transcript, so `doing` is always `—` and the elapsed rail
is always blank. Real use fills both in.

The fixture `q` (a Claude Code permission box) has one line that is 2 cells
short (`don't ask again              │`). That is my fixture's quoting, not a
rendering bug.

File:line references are to master.

---

## Part 1: the board

### 1.1 Tile grid at each width

**What it shows.** `grid()` (`tui/src/board.rs:61-72`) gives
`clamp(width / 48, 1, 3)` columns, so 40-95 columns get 1 column, 96-143 get 2,
and 144 and up get 3. Rows share the height equally (`Constraint::Ratio`,
`board.rs:182`). A row needs at least 5 rows of height, or the grid scrolls. At
7 or more rows per tile the tile is "full" and gets 2 header lines.

100x40, 6 agents with real previews:

```
 board · 6 agents · 2 needs you
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━┓┌ 2 ▲  web:0 · docs ─────────────────────────────┐
┃Bash: npm test -- --watch=false. Do you         ┃│plan ready: approve?                            │
┃want to proceed?                                ┃│                                                │
┃╭──────────────────────────────────────────────╮┃│⏺ Done. Pushed branch fix/login and opened PR #4│
┃│ Bash command                                 │┃│                                                │
┃│   npm test -- --watch=false                  │┃│>                                               │
┃│ Do you want to proceed?                      │┃│                                                │
┃│ ❯ 1. Yes                                     │┃│                                                │
┃│   2. Yes, and don't ask again              │  ┃│                                                │
┃│   3. No, and tell Claude what to do          │┃│                                                │
┃╰──────────────────────────────────────────────╯┃│                                                │
┃                                                ┃│                                                │
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└ ↵ answer ──────────────────────────────────────┘
┌ 3 ✕  infra:0 · terraform ──────────────────────┐┌ 4 ●  api:1 · tests ────────────────────────────┐
│—                                               ││—                                               │
│                                                ││                                                │
│error: terraform plan failed                    ││✓ test 24: checkout flow step 24 passes in 168ms│
│  │ Error: Invalid provider configuration       ││✓ test 25: checkout flow step 25 passes in 175ms│
│  │ on main.tf line 12                          ││✓ test 26: checkout flow step 26 passes in 182ms│
│                                                ││✓ test 27: checkout flow step 27 passes in 189ms│
│                                                ││✓ test 28: checkout flow step 28 passes in 196ms│
│                                                ││✓ test 29: checkout flow step 29 passes in 203ms│
│                                                ││✓ test 30: checkout flow step 30 passes in 210ms│
│                                                ││⏺ Running 30 more...                            │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 5 ●  web:1 · ui ───────────────────────────────┐┌ 6 ✓  web:2 · checkout ─────────────────────────┐
│—                                               ││—                                               │
│                                                ││                                                │
│a very long single line that goes on and on: xxx││⏺ Done. Pushed branch fix/login and opened PR #4│
│> short                                         ││                                                │
│                                                ││>                                               │
│                                                ││                                                │
│                                                ││                                                │
│                                                ││                                                │
│                                                ││                                                │
│                                                ││                                                │
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f panel   i info   ? help   q close
```

180x45, same fleet:

```
 board · 6 agents · 2 needs you
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓┌ 2 ▲  web:0 · docs ───────────────────────────────────────┐┌ 3 ✕  infra:0 · terraform ────────────────────────────────┐
┃Bash: npm test -- --watch=false. Do you want to           ┃│plan ready: approve?                                      ││—                                                         │
┃proceed?                                                  ┃│                                                          ││                                                          │
┃⏺ Bash(npm test -- --watch=false)                         ┃│⏺ Done. Pushed branch fix/login and opened PR #41.        ││error: terraform plan failed                              │
┃  ⎿  Running...                                           ┃│                                                          ││  │ Error: Invalid provider configuration                 │
┃                                                          ┃│>                                                         ││  │ on main.tf line 12                                    │
┃╭──────────────────────────────────────────────╮          ┃│                                                          ││                                                          │
┃│ Bash command                                 │          ┃│                                                          ││                                                          │
┃│   npm test -- --watch=false                  │          ┃│                                                          ││                                                          │
┃│ Do you want to proceed?                      │          ┃│                                                          ││                                                          │
┃│ ❯ 1. Yes                                     │          ┃│                                                          ││                                                          │
┃│   2. Yes, and don't ask again              │            ┃│                                                          ││                                                          │
┃│   3. No, and tell Claude what to do          │          ┃│                                                          ││                                                          │
┃╰──────────────────────────────────────────────╯          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└ ↵ answer ────────────────────────────────────────────────┘└──────────────────────────────────────────────────────────┘
┌ 4 ●  api:1 · tests ──────────────────────────────────────┐┌ 5 ●  web:1 · ui ─────────────────────────────────────────┐┌ 6 ✓  web:2 · checkout ───────────────────────────────────┐
│—                                                         ││—                                                         ││—                                                         │
│                                                          ││                                                          ││                                                          │
│✓ test 15: checkout flow step 15 passes in 105ms  (src/che││a very long single line that goes on and on: xxxxxxxxxxxxx││⏺ Done. Pushed branch fix/login and opened PR #41.        │
│✓ test 16: checkout flow step 16 passes in 112ms  (src/che││> short                                                   ││                                                          │
│✓ test 17: checkout flow step 17 passes in 119ms  (src/che││                                                          ││>                                                         │
│✓ test 18: checkout flow step 18 passes in 126ms  (src/che││                                                          ││                                                          │
│✓ test 19: checkout flow step 19 passes in 133ms  (src/che││                                                          ││                                                          │
│✓ test 20: checkout flow step 20 passes in 140ms  (src/che││                                                          ││                                                          │
│✓ test 21: checkout flow step 21 passes in 147ms  (src/che││                                                          ││                                                          │
│✓ test 22: checkout flow step 22 passes in 154ms  (src/che││                                                          ││                                                          │
│✓ test 23: checkout flow step 23 passes in 161ms  (src/che││                                                          ││                                                          │
│✓ test 24: checkout flow step 24 passes in 168ms  (src/che││                                                          ││                                                          │
│✓ test 25: checkout flow step 25 passes in 175ms  (src/che││                                                          ││                                                          │
│✓ test 26: checkout flow step 26 passes in 182ms  (src/che││                                                          ││                                                          │
│✓ test 27: checkout flow step 27 passes in 189ms  (src/che││                                                          ││                                                          │
│✓ test 28: checkout flow step 28 passes in 196ms  (src/che││                                                          ││                                                          │
│✓ test 29: checkout flow step 29 passes in 203ms  (src/che││                                                          ││                                                          │
│✓ test 30: checkout flow step 30 passes in 210ms  (src/che││                                                          ││                                                          │
│⏺ Running 30 more...                                      ││                                                          ││                                                          │
└──────────────────────────────────────────────────────────┘└──────────────────────────────────────────────────────────┘└──────────────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f panel   i info   ? help   q close
```

**Keep.**
- Whole tiles only. A tile is never cut in half, and a short last row keeps
  normal-sized tiles (`board.rs:59-60`, `board.rs:699-702`).
- The 48-column `MIN_TILE_W` (`board.rs:19`). It keeps the title readable.
- Blocked tiles stand out without help: a red thick border, `▲`, the question,
  and `↵ answer` on the bottom border.

**Missing or awkward.**
- **(c) Three equal columns at 180.** Each tile at 180 is 60 columns wide. The
  test-runner preview clips at `(src/che`, while the 21 empty rows below tile 2
  and tile 3 go unused. Tiles are equal by construction (`Ratio(1, cols)`,
  `board.rs:184`). There is no notion of "the blocked tile deserves more room"
  and no wider-column option. At 180x45 with 6 agents, about 60% of the body
  cells are blank.
- **Height is shared equally even when content is short.** Each of 2 rows gets
  19 rows at 180x45. A done agent with 3 lines of output gets 19 rows, and so
  does a blocked agent whose question box needs 12.
- **Columns jump from 1 to 3** with no in-between that adapts to the agent
  count. 2 agents at 180 get 2 columns 90 wide, which is fine. 4 agents at 180
  get 3+1, a lone tile on row 2 with two empty thirds.
- **The worker under-fetches the preview by one row** when the ratio split
  rounds up. The worker computes `tile_h = avail / rows_shown` with integer
  division (`board.rs:763-764`), but `Layout` gives the remainder row to some
  tiles. At 100x15, tile 1 shows a blank last body row (see the 100x15 capture).

### 1.2 Header and counts

**What it shows.** ` board · N agents · K needs you` on the left (`board.rs:108-118`).
When rows are hidden, `F-L of N   ▲a ▼b` sits on the right (`board.rs:120-132`).

**Keep.** The `needs you` count in the needs colour. The `N-M of T ▲ ▼` form is
the same one the panel uses (`panel/view.rs:1046`, whose comment cites the
board's header).

**Missing or awkward.**
- **No per-state or per-session breakdown.** It says "17 agents · 3 needs you".
  It does not say errors (1), running (8) or done (5), or how many sessions.
- **The right indicator overlaps the left text when narrow.** Both are drawn
  into the same `Rect` with no width check. At 40x20:
  ` board · 6 agents · 2 need1-3 of 6   ▼3` (the 40x20 help capture below). The
  panel's `titles()` (`panel/view.rs:752-776`) solves this: the title gives way
  first, then the arrows, and the count is dropped whole. The board has no
  equivalent.
- The doc's `· stale 34s` header and the "tmux is not answering" card
  (`docs/tui.md:483-495`) **are not implemented**. `board.rs` has no `stale`.
  The worker silently keeps the last list (`board.rs:757-761`). The tiles can be
  stale forever with no sign.

### 1.3 Tile chrome

**What it shows** (`board.rs:212-292`):
- top border: `▎` (when it is tmux's active pane), the number, the state glyph,
  the review mark, `sess:idx`, ` · `, then the title (`@cockpit_title`, else the
  window name);
- header row 1: `doing` (or `msg` folded when blocked), plus a 5-column elapsed
  rail;
- header row 2 (full density only): `said`;
- bottom border: ` ↵ answer ` on blocked tiles only.

**Keep.** The fixed `(state, pane id)` order, the 5-column rail and its
escalation colours (`board.rs:300-305`), `trunc_middle` on `doing`, and the
double encoding of dimmed (DIM plus the meta colour).

**Missing or awkward.**
- **The second header row is reserved even when `said` is empty.** `hdr` is 2
  whenever `full` (`board.rs:276`), but the line is pushed only
  `if !p.said.is_empty()` (`board.rs:334`). Every non-blocked tile in every
  capture has a `—` row and then a **blank row** before the preview. That is 2
  dead rows per tile, and 28 dead rows at 100x40 with 17 agents.
- **`—` for doing is a whole row that says nothing.** With no transcript, row 1
  is `—` and the rail is blank. One line could hold the state word ("running",
  "error", "done") or the last preview line.
- **The question appears twice on blocked tiles.** The header folds `msg` (`Bash:
  npm test … Do you want to proceed?`), and the preview tail shows the same
  permission box. At small sizes the preview tail is the *bottom* of the box
  (`3. No, and tell Claude…`, `╰────╯`), which is the least useful part. The
  options (`❯ 1. Yes`) are cut off (100x40 with 17 agents, tiles 1 and 3).
- The `↵ answer` bottom title is the only per-tile action hint. A non-blocked
  tile has no hint at all. An error tile, the second most urgent, has no
  `r review` / `f fix` hint.
- **`▎` means two things.** On the board it marks *tmux's active pane*
  (`board.rs:242`). In the panel the same glyph (`th.g.active`) marks *the
  selection* (`panel/view.rs:179`). The board's own selection mark is the thick
  border (`board.rs:262`).
- Tile numbers grow to 2 digits (`10 ●`) and push the title right by a column.
  This is fine.
- The window *name* is shown, but not the session's working directory or
  branch, which `Pane` carries (`root`, `path`, `loc`, `pm`). `pm`, the diff
  `±`, would tell "this one has changes to review" at a glance.

### 1.4 Preview content

**What it shows.** `capture-pane -ep -J -S -n`, trailing blanks trimmed, the
last `n` lines, top-aligned, ANSI kept, no wrap (`data.rs:810-819`,
`board.rs:280-291`).

**Keep.** ANSI colour, the no-`Wrap` rule (`docs/tui.md` hard rules), the
64 KB cap, and `capture failed` versus an empty pane.

**Missing or awkward.**
- **Top-aligned with a trimmed tail.** A pane whose content is short leaves the
  bottom of the tile empty (tile 6 at 180x45: 3 lines, then 16 blank rows).
  Bottom-aligned would read like a terminal and would put the prompt line where
  the eye expects it.
- **Long lines clip silently** (`a very long single line … xxx`, no `…`
  marker). This is acceptable under the no-wrap rule, but a 1-cell `›` at the
  edge would say "cut".
- **No horizontal fit.** A 120-column agent screen in a 58-column tile shows
  its left half only. The Claude Code permission box is 48 wide and fits. Wider
  TUIs (diffs, tables) do not.
- **Previews are fetched only for visible tiles** (`board.rs:767`), which is
  correct. A tile scrolled into view shows its last cached capture.

### 1.5 Footer hints

**What it shows.** One row. The candidates are tried whole, widest first
(`board.rs:159-171`):
1. `↵ jump   a ask   r review   f panel   i info   ? help   q close` (63 cols)
2. `↵ jump   ? help   q close`
3. `? help`

**Keep.** Hints drop whole and are never cut (the same rule as the panel).

**Missing or awkward.**
- **(e) At 60 columns every action except jump disappears.** The step from
  candidate 1 to 2 drops `a r f i` together. A 60-column popup (a 66-column
  terminal, or a phone) never shows that the board can review, open the panel
  or show info. The panel drops one hint at a time, lowest priority first
  (`panel/view.rs:727-748`).
- **The footer is not state-aware.** It always says `a ask`, even on a blocked
  tile where `a` is a plain jump (`board.rs:972-975`), and on a running tile
  where `a` only prints a note (`board.rs:976-982`).
- **Its separators differ from the panel's.** The board uses three spaces in a
  standalone row (`board.rs:146-148`). The panel puts the hints *in the bottom
  border* with two spaces (`panel/view.rs:1064`). Its verbs differ too: the
  board says `? help`, the panel says `? keys`; the board says `q close`, the
  panel says `esc close`.

### 1.6 The `?` overlay

```
 board · 6 agents · 2 needs you
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━┓┌ 2 ▲  web:0 · docs ─────────────────────────────┐
┃Bash: npm test -- --watch=false. Do you         ┃│plan ready: approve?                            │
┃want to proceed?                                ┃│                                                │
┃╭──────────────────────────────────────────────╮┃│⏺ Done. Pushed branch fix/login and opened PR #4│
┃│ Bash command                                 │┃│                                                │
┃│   npm test -- --watch=false                  │┃│>                                               │
┃│ Do you want to proceed?                      │┃│                                                │
┃│ ❯ 1. Yes                                     │┃│                                                │
┃│   2. Yes, and don't ask again              │  ┃│                                                │
┃│   3. No, and tell Claude what to do          │┃│                                                │
┃╰──────────────────────────────────────────────╯┃│                                                │
┃                                                ┃│                                                │
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└ ↵ answer ──────────────────────────────────────┘
┌ 3 ✕  infra:0 ·╭ keys ─────────────────────────────────────────────────────────────╮──────────────┐
│—              │ move                       act                                    │              │
│               │ h l  ← →  column           ↵   jump to this pane, close the board │              │
│error: terrafor│ j k  ↑ ↓  row              a   answer / ask this agent            │asses in 168ms│
│  │ Error: Inva│ ⇥ ⇧⇥      next / previous  r   review its diff                    │asses in 175ms│
│  │ on main.tf │ 1-9       tile by number   f   panel for this pane                │asses in 182ms│
│               │ g G       first / last     i   info for this pane                 │asses in 189ms│
│               │ u         next blocked                                            │asses in 196ms│
│               │ [ ]       page             view                                   │asses in 203ms│
│               │                            ?   this help                          │asses in 210ms│
│               │                            ^L  refresh now                        │              │
└───────────────│                            q   close   ·   esc close overlay      │──────────────┘
┌ 5 ●  web:1 · u╰ any key closes ───────────────────────────────────────────────────╯──────────────┐
│—                                               ││—                                               │
│                                                ││                                                │
│a very long single line that goes on and on: xxx││⏺ Done. Pushed branch fix/login and opened PR #4│
│> short                                         ││                                                │
│                                                ││>                                               │
│                                                ││                                                │
│                                                ││                                                │
│                                                ││                                                │
│                                                ││                                                │
│                                                ││                                                │
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f panel   i info   ? help   q close
```

40x20, overlay and header both clipped:

```
 board · 6 agents · 2 need1-3 of 6   ▼3
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━┓
┃Bash: npm test -- --watch=false…      ┃
┃│   2. Yes, and don't ask again       ┃
╭ keys ────────────────────────────────╮
│ move                       act       │
│ h l  ← →  column           ↵   jump  │
│ j k  ↑ ↓  row              a   answe │
│ ⇥ ⇧⇥      next / previous  r   revie │
│ 1-9       tile by number   f   panel │
│ g G       first / last     i   info  │
│ u         next blocked               │
│ [ ]       page             view      │
│                            ?   this  │
│                            ^L  refre │
│                            q   close │
╰ any key closes ──────────────────────╯
│  │ on main.tf line 12                │
└──────────────────────────────────────┘
 ↵ jump   ? help   q close
```

**What it shows.** Two columns (move | act + view), generated from the same
`KEYS` table that `action()` reads (`board.rs:465-510`, `board.rs:551-642`).

**Keep.** One table for both dispatch and help. The grouping. The rule that any
key closes it (`board.rs:934-938`).

**Missing or awkward.**
- **No narrow layout and no scroll.** At 40 columns the right column is cut
  mid-word (`↵   jump  `, `a   answe`) with no ellipsis, and below about 14 rows
  the bottom rows are lost. The panel's overlay is one column, scrolls with
  `↑↓`, and shows `N-M of T` (`panel/view.rs:943-985`).
- **`q close · esc close overlay`** is misleading: any key closes the overlay,
  and `esc` *outside* the overlay closes the board (`board.rs:668`).
- **It is a centred card over a live grid.** The panel's overlay covers the
  whole popup, because "a card inside it would clip the very rows it exists to
  show" (`panel/view.rs:938-939`). That reasoning applies to the board at 40x20.
- **It does not say what each tile state means** (`▲ ✕ ● ✓ ○`, `◆` review) or
  what `▎` and the thick border mean. This is the only place a legend could
  live.

### 1.7 The empty state

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

```






╭ board ───────────────────────────────╮
│                                      │
│ no agent panes                       │
│                                      │
│ cockpit tracks a pane once an agent …│
│ start one, or press q and use prefix…│
│                                      │
╰ q close ─────────────────────────────╯
```

**What it shows.** A `card()` (`board.rs:418-457`).

**Missing or awkward.**
- **The card sits at the top of the popup, not in its centre**, despite
  `area.centered(…)` (`board.rs:421`). I saw this in both captures. I did not
  find the cause in the code. It needs a check before anyone relies on
  `centered`.
- At 40 columns both lines of advice are cut (`agent …`, `prefix…`). The card
  truncates and does not wrap (`board.rs:437`). The panel's `many()` wraps.
- The advice is "press q and use prefix r". It never says which pane types
  count as agents, or that `@panestate` comes from the hooks.

### 1.8 Narrow and short fallbacks

`MIN_BOARD_W = 40`, `MIN_BOARD_H = 5` (`board.rs:76-78`). Below either one,
`narrow()` runs (`board.rs:396-415`).

38x30:

```
╭ board ─────────────────────────────╮
│terminal too narrow                 │
│prefix i on the pane instead        │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
│                                    │
╰────────────────────────────────────╯
```

100x4 (too *short*, still says "too narrow"):

```
╭ board ───────────────────────────────────────────────────────────────────────────────────────────╮
│terminal too narrow                                                                               │
│prefix i on the pane instead                                                                      │
╰──────────────────────────────────────────────────────────────────────────────────────────────────╯
```

**Missing or awkward.**
- **The message is wrong for a short screen.** The card says "terminal too
  narrow" at 100x4, but the width is fine. The panel says "make the window
  larger or rotate" for both cases (`panel/view.rs:779-795`).
- **The text differs from the doc.** The doc gives
  `terminal too narrow — prefix i on the pane instead` as one line
  (`docs/tui.md:520-522`). The code draws two lines, without the dash.
- **The board has no remedy key.** The panel card says `esc goes back`. The
  board card names no key (`q` works).
- **A floor of 5 rows is not a usable floor.** At 100x15 the grid still draws:
  2 rows of tiles, 4 visible out of 6. But at 100x7 one tile with a single
  preview row is "valid". The panel measured its floors per view (28x7 pc,
  28x12 tablet). The board's floor is a guard against blank screens, not a
  measured usability floor.

100x15:

```
 board · 6 agents · 2 needs you                                                       1-4 of 6   ▼2
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━┓┌ 2 ▲  web:0 · docs ─────────────────────────────┐
┃Bash: npm test -- --watch=false. Do you w…      ┃│plan ready: approve?                            │
┃│   2. Yes, and don't ask again              │  ┃│⏺ Done. Pushed branch fix/login and opened PR #4│
┃│   3. No, and tell Claude what to do          │┃│                                                │
┃╰──────────────────────────────────────────────╯┃│>                                               │
┃                                                ┃│                                                │
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└ ↵ answer ──────────────────────────────────────┘
┌ 3 ✕  infra:0 · terraform ──────────────────────┐┌ 4 ●  api:1 · tests ────────────────────────────┐
│—                                               ││—                                               │
│error: terraform plan failed                    ││✓ test 29: checkout flow step 29 passes in 203ms│
│  │ Error: Invalid provider configuration       ││✓ test 30: checkout flow step 30 passes in 210ms│
│  │ on main.tf line 12                          ││⏺ Running 30 more...                            │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f panel   i info   ? help   q close
```

### 1.9 Narrow portrait (d)

40x40 and 60x50, 6 agents:

```
 board · 6 agents · 2 needs you
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━┓
┃Bash: npm test -- --watch=false…      ┃
┃│   2. Yes, and don't ask again       ┃
┃│   3. No, and tell Claude what to do ┃
┃╰─────────────────────────────────────┃
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ 2 ▲  web:0 · docs ───────────────────┐
│plan ready: approve?                  │
│⏺ Done. Pushed branch fix/login and op│
│                                      │
│>                                     │
│                                      │
└ ↵ answer ────────────────────────────┘
┌ 3 ✕  infra:0 · terraform ────────────┐
│—                                     │
│error: terraform plan failed          │
│  │ Error: Invalid provider configurat│
│  │ on main.tf line 12                │
└──────────────────────────────────────┘
┌ 4 ●  api:1 · tests ──────────────────┐
│—                                     │
│✓ test 29: checkout flow step 29 passe│
│✓ test 30: checkout flow step 30 passe│
│⏺ Running 30 more...                  │
└──────────────────────────────────────┘
┌ 5 ●  web:1 · ui ─────────────────────┐
│—                                     │
│a very long single line that goes on a│
│> short                               │
│                                      │
│                                      │
└──────────────────────────────────────┘
┌ 6 ✓  web:2 · checkout ───────────────┐
│—                                     │
│⏺ Done. Pushed branch fix/login and op│
│                                      │
│>                                     │
└──────────────────────────────────────┘
 ↵ jump   ? help   q close
```

```
 board · 6 agents · 2 needs you
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃Bash: npm test -- --watch=false. Do you want to           ┃
┃proceed?                                                  ┃
┃│ ❯ 1. Yes                                     │          ┃
┃│   2. Yes, and don't ask again              │            ┃
┃│   3. No, and tell Claude what to do          │          ┃
┃╰──────────────────────────────────────────────╯          ┃
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ 2 ▲  web:0 · docs ───────────────────────────────────────┐
│plan ready: approve?                                      │
│                                                          │
│⏺ Done. Pushed branch fix/login and opened PR #41.        │
│                                                          │
│>                                                         │
│                                                          │
└ ↵ answer ────────────────────────────────────────────────┘
┌ 3 ✕  infra:0 · terraform ────────────────────────────────┐
│—                                                         │
│                                                          │
│error: terraform plan failed                              │
│  │ Error: Invalid provider configuration                 │
│  │ on main.tf line 12                                    │
│                                                          │
└──────────────────────────────────────────────────────────┘
┌ 4 ●  api:1 · tests ──────────────────────────────────────┐
│—                                                         │
│                                                          │
│✓ test 28: checkout flow step 28 passes in 196ms  (src/che│
│✓ test 29: checkout flow step 29 passes in 203ms  (src/che│
│✓ test 30: checkout flow step 30 passes in 210ms  (src/che│
│⏺ Running 30 more...                                      │
└──────────────────────────────────────────────────────────┘
┌ 5 ●  web:1 · ui ─────────────────────────────────────────┐
│—                                                         │
│                                                          │
│a very long single line that goes on and on: xxxxxxxxxxxxx│
│> short                                                   │
│                                                          │
│                                                          │
└──────────────────────────────────────────────────────────┘
┌ 6 ✓  web:2 · checkout ───────────────────────────────────┐
│—                                                         │
│                                                          │
│⏺ Done. Pushed branch fix/login and opened PR #41.        │
│                                                          │
│>                                                         │
│                                                          │
└──────────────────────────────────────────────────────────┘
 ↵ jump   ? help   q close
```

60x50, 17 agents:

```
 board · 17 agents · 3 needs you             1-9 of 17   ▼8
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃Bash: npm test. Do you want to proceed?                   ┃
┃│   3. No, and tell Claude what to do          │          ┃
┃╰──────────────────────────────────────────────╯          ┃
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ 2 ▲  web:1 · docs ───────────────────────────────────────┐
│plan ready: approve?                                      │
│                                                          │
│>                                                         │
│                                                          │
└ ↵ answer ────────────────────────────────────────────────┘
┌ 3 ▲  infra:3 · vpn ──────────────────────────────────────┐
│permission: Bash(sudo openvpn)                            │
││   3. No, and tell Claude what to do          │          │
│╰──────────────────────────────────────────────╯          │
└ ↵ answer ────────────────────────────────────────────────┘
┌ 4 ✕  infra:0 · terraform ────────────────────────────────┐
│—                                                         │
│  │ Error: Invalid provider configuration                 │
│  │ on main.tf line 12                                    │
└──────────────────────────────────────────────────────────┘
┌ 5 ●  api:1 · tests ──────────────────────────────────────┐
│—                                                         │
│✓ test 30: checkout flow step 30 passes in 210ms  (src/che│
│⏺ Running 30 more...                                      │
│                                                          │
└──────────────────────────────────────────────────────────┘
┌ 6 ●  api:2 · migrate ────────────────────────────────────┐
│—                                                         │
│✓ test 30: checkout flow step 30 passes in 210ms  (src/che│
│⏺ Running 30 more...                                      │
└──────────────────────────────────────────────────────────┘
┌ 7 ●  web:0 · ui ─────────────────────────────────────────┐
│—                                                         │
│a very long single line that goes on and on: xxxxxxxxxxxxx│
│> short                                                   │
└──────────────────────────────────────────────────────────┘
┌ 8 ●  web:3 · search ─────────────────────────────────────┐
│—                                                         │
│✓ test 30: checkout flow step 30 passes in 210ms  (src/che│
│⏺ Running 30 more...                                      │
│                                                          │
└──────────────────────────────────────────────────────────┘
┌ 9 ●  infra:1 · k8s ──────────────────────────────────────┐
│—                                                         │
│✓ test 30: checkout flow step 30 passes in 210ms  (src/che│
│⏺ Running 30 more...                                      │
└──────────────────────────────────────────────────────────┘
 ↵ jump   ? help   q close
```

**What works.** One column. Each tile keeps its identity line, and blocked tiles
stay first.

**Missing or awkward.**
- **One agent per 5-8 rows.** A phone in portrait (40x40) sees 6 agents and no
  more. With 17 agents it sees 9 out of 17, and paging is the only way on.
- **At 40 columns the header uses 1 row for `—`**, which leaves 3-4 preview
  rows of 38 columns. That is barely enough to recognise the agent, and not
  enough to read it.
- **The fold of a blocked question loses its end at `hdr = 1`.** At 40x40,
  `Bash: npm test -- --watch=false…`: the question itself, "Do you want to
  proceed?", is cut. When not `full`, the needs header gets 1 row
  (`board.rs:276`, `fold` with `rows = 1`).
- **No compact list mode.** At this size, one line per agent (glyph, `sess:idx`,
  title, age) would show 30+ agents. The fleet profile widths in `fleet.rs:22-54`
  already define such a row (profile A, under 68 columns).
- **No touch.** The board has no mouse at all (`docs/tui.md:86-89`,
  `backend::init(false, false)` at `board.rs:806`). The panel has a tablet mode
  next door. A phone user can open the board but must find `↵`, `j`, `k` and
  `]` on Termux's extra-keys row.

### 1.10 Scrolling and paging

17 agents across 4 sessions, 100x40, first page:

```
 board · 17 agents · 3 needs you                                                    1-14 of 17   ▼3
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━┓┌ 2 ▲  web:1 · docs ─────────────────────────────┐
┃Bash: npm test. Do you want to proceed?         ┃│plan ready: approve?                            │
┃│   3. No, and tell Claude what to do          │┃│                                                │
┃╰──────────────────────────────────────────────╯┃│>                                               │
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└ ↵ answer ──────────────────────────────────────┘
┌ 3 ▲  infra:3 · vpn ────────────────────────────┐┌ 4 ✕  infra:0 · terraform ──────────────────────┐
│permission: Bash(sudo openvpn)                  ││—                                               │
││   3. No, and tell Claude what to do          │││  │ Error: Invalid provider configuration       │
│╰──────────────────────────────────────────────╯││  │ on main.tf line 12                          │
│                                                ││                                                │
└ ↵ answer ──────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 5 ●  api:1 · tests ────────────────────────────┐┌ 6 ●  api:2 · migrate ──────────────────────────┐
│—                                               ││—                                               │
│✓ test 30: checkout flow step 30 passes in 210ms││✓ test 30: checkout flow step 30 passes in 210ms│
│⏺ Running 30 more...                            ││⏺ Running 30 more...                            │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 7 ●  web:0 · ui ───────────────────────────────┐┌ 8 ●  web:3 · search ───────────────────────────┐
│—                                               ││—                                               │
│a very long single line that goes on and on: xxx││✓ test 30: checkout flow step 30 passes in 210ms│
│> short                                         ││⏺ Running 30 more...                            │
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 9 ●  infra:1 · k8s ────────────────────────────┐┌ 10 ●  ml:0 · train ────────────────────────────┐
│—                                               ││—                                               │
│✓ test 30: checkout flow step 30 passes in 210ms││✓ test 30: checkout flow step 30 passes in 210ms│
│⏺ Running 30 more...                            ││⏺ Running 30 more...                            │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 11 ●  ml:1 · eval ─────────────────────────────┐┌ 12 ●  ml:4 · export ───────────────────────────┐
│—                                               ││—                                               │
│✓ test 30: checkout flow step 30 passes in 210ms││✓ test 30: checkout flow step 30 passes in 210ms│
│⏺ Running 30 more...                            ││⏺ Running 30 more...                            │
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 13 ✓  api:3 · auth ────────────────────────────┐┌ 14 ✓  web:2 · checkout ────────────────────────┐
│—                                               ││—                                               │
│                                                ││                                                │
│>                                               ││>                                               │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f panel   i info   ? help   q close
```

The same, after `G` and after `]`:

```
 board · 17 agents · 3 needs you                                                    5-17 of 17   ▲4
┌ 5 ●  api:1 · tests ────────────────────────────┐┌ 6 ●  api:2 · migrate ──────────────────────────┐
│—                                               ││—                                               │
│✓ test 30: checkout flow step 30 passes in 210ms││✓ test 30: checkout flow step 30 passes in 210ms│
│⏺ Running 30 more...                            ││⏺ Running 30 more...                            │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 7 ●  web:0 · ui ───────────────────────────────┐┌ 8 ●  web:3 · search ───────────────────────────┐
│—                                               ││—                                               │
│a very long single line that goes on and on: xxx││✓ test 30: checkout flow step 30 passes in 210ms│
│> short                                         ││⏺ Running 30 more...                            │
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 9 ●  infra:1 · k8s ────────────────────────────┐┌ 10 ●  ml:0 · train ────────────────────────────┐
│—                                               ││—                                               │
│✓ test 30: checkout flow step 30 passes in 210ms││✓ test 30: checkout flow step 30 passes in 210ms│
│⏺ Running 30 more...                            ││⏺ Running 30 more...                            │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 11 ●  ml:1 · eval ─────────────────────────────┐┌ 12 ●  ml:4 · export ───────────────────────────┐
│—                                               ││—                                               │
│✓ test 30: checkout flow step 30 passes in 210ms││✓ test 30: checkout flow step 30 passes in 210ms│
│⏺ Running 30 more...                            ││⏺ Running 30 more...                            │
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 13 ✓  api:3 · auth ────────────────────────────┐┌ 14 ✓  web:2 · checkout ────────────────────────┐
│—                                               ││—                                               │
│                                                ││                                                │
│>                                               ││>                                               │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┌ 15 ✓  infra:2 · dns ───────────────────────────┐┌ 16 ✓  ml:2 · data ─────────────────────────────┐
│—                                               ││—                                               │
│                                                ││                                                │
│>                                               ││>                                               │
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
┏ 17 ✓  ml:3 · notebook ━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃—                                               ┃
┃                                                ┃
┃>                                               ┃
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ↵ jump   a ask   r review   f panel   i info   ? help   q close
```

```
 board · 17 agents · 3 needs you                                                  15-17 of 17   ▲14
┏ 15 ✓  infra:2 · dns ━━━━━━━━━━━━━━━━━━━━━━━━━━━┓┌ 16 ✓  ml:2 · data ─────────────────────────────┐
┃—                                               ┃│—                                               │
┃                                                ┃│                                                │
┃>                                               ┃│>                                               │
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└────────────────────────────────────────────────┘
┌ 17 ✓  ml:3 · notebook ─────────────────────────┐
│—                                               │
│                                                │
│>                                               │
│                                                │
└────────────────────────────────────────────────┘



























 ↵ jump   a ask   r review   f panel   i info   ? help   q close
```

**What works.** `follow()` keeps the selection visible (`board.rs:710-719`). The
header indicator is accurate.

**Missing or awkward.**
- **`]` moves the selection by a whole page too** (`board.rs:694-695`): sel 1
  becomes sel 15. The panel's PgDn scrolls without moving the selection off
  what you were reading.
- **The last page is mostly blank.** `]` stops on the last row and not the last
  full page (`board.rs:699-702`, which is deliberate). The result at 100x40 is
  3 tiles in the top 11 rows and 26 blank rows. A page that ends at the last
  full screenful would show 14 tiles instead of 3.
- **No scrollbar or position glyph in the grid itself.** The header count is
  the only sign.
- Scrolling is by tile row. That is fine.

### 1.11 Every key, and what it does

From `action()` (`board.rs:644-671`), `moved()` (`board.rs:675-706`) and the run
loop (`board.rs:951-991`):

| key | act | what it actually does | notes |
|---|---|---|---|
| `h` `←` | Left | sel-1 within the row; stops at column 0 | |
| `l` `→` | Right | sel+1 within the row | |
| `k` `↑` | Up | sel − cols | |
| `j` `↓` | Down | sel + cols, clamped to last | |
| `⇥` / `⇧⇥` | Next/Prev | reading order, wraps | |
| `1`-`9` | Tile(n) | selects **absolute** tile n | doc says "on this page" (`docs/tui.md:71`); tiles 10+ have no digit |
| `g` / `G` | First/Last | | `g` is also the panel's "best guess" in Question |
| `u` | NextNeeds | next blocked, wraps; no-op if none | silent when none |
| `[` `PgUp` / `]` `PgDn` | Page | moves sel and top by a screen | |
| `↵` | Jump | writes `jump %N`, exits | |
| `a` | Ask | blocked: `jump %N`. Otherwise: a 2 s footer note | the help says "answer / ask", but it only ever jumps |
| `r` | Review | writes `review %N`, exits; the wrapper runs `tmux-review` in the same popup | the board does not come back |
| `f` | Panel | writes `panel %N`, exits; the wrapper runs `cockpit-panel` | the board does not come back |
| `i` | Info | writes `info %N`; `tmux-review --info; read -r _` | waits for an unlabelled Enter (`scripts/cockpit-board:71`) |
| `?` | Help | overlay; the next key closes it and is swallowed | |
| `^L` | Refresh | asks the worker; no visual ack | |
| `q` `esc` | Quit | closes | |
| (none) | `/` filter, `s` session, `x` kill, `z` zoom | **absent** | |

**(b) Finding a specific agent among many: there is no search or filter.** No
`/` and no session jump. Digits stop at 9. With 17 agents the only routes to
"the ml:3 notebook agent" are `G` plus arrow keys, or reading every tile. The
"What it is not" section (`docs/tui.md:15-18`) sends filtering to `prefix r`
(fzf), which is a different screen with no previews.

**(a) No grouping by session.** `order()` sorts by `(state, pane id)`
(`board.rs:45-49`). `api:0`, `web:1`, `infra:3` and `ml:0` are interleaved
(see the 17-agent capture). `Pane.sess` exists (`data.rs:113`), and `wher`
starts with the session name. Nothing on screen groups, labels or counts
sessions. `list_panes` also sorts by state (`data.rs:800`), and the board then
re-sorts. The design rule "no clock in the sort" (`board.rs:43-44`) is worth
keeping in any grouped design: group, then rank inside the group, never age.

### 1.12 What "act on an agent" offers (e)

| verb | key | discoverable where | caveats |
|---|---|---|---|
| jump | `↵` | footer (always), `↵ answer` on blocked tiles | |
| answer | `a` / `↵` | blocked tile bottom title | **no inline answer**; it jumps. `deliver` is reserved but stubbed (`scripts/cockpit-board:72`) |
| ask (/btw) | `a` | footer, help | only a note telling you to jump and type `/btw` yourself |
| review | `r` | footer at 63+ columns, help | leaves the board for good |
| panel | `f` | footer at 63+ columns, help | leaves the board for good; see §3 |
| info | `i` | footer at 63+ columns, help | leaves the board; ends on a bare `read` |

`a` on a running tile (100x40, the last 3 rows):

```
│                                                ││                                                │
└────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
 api:1 is not waiting on you · ↵ jumps there, /btw asks it while it works
```

Every action except `a` on a non-blocked tile **ends the board**. There is no
menu of actions on a tile, no per-state default (an error tile's `↵` could be
`r` or `f`), and no "return to the board" after review, panel or info.

---

## Part 2: the panel

Captured live at 60, 100 and 180 columns, pc and tablet, against `%0` (the
blocked `api:0 · server` agent). States that need a `claude -p` answer
(Question, Result, Steps, Answer, Confirm) come from the goldens in
`tui/tests/golden/` (40/60/80/120 wide). The live Busy and Error used a fake
`COCKPIT_CLAUDE` (one that sleeps, one that exits 1 with "not logged in").

### 2.1 Home

pc 100x30:

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────╮
│ ▎1 ★ Write a prompt    a task for Claude Code from your rough words                              │
│  2   Compose           type once: fix spelling, write, or insert                                 │
│  3   Explain           what is on this screen, and what next                                     │
│  4   Tablet mode       big tiles, tap buttons, the mouse                                         │
│  5   Close                                                                                       │
│                                                                                                  │
│ server · claude · /tmp/claude-1000/-home-tzachi--tmux-plugins-tmux-cockpit/9a68607b-74d7-4d99-b… │
│ will send: screen 11 lines, 0 hidden                                                             │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
╰ 1-5 pick  ? keys  esc close ─────────────────────────────────────────────────────────────────────╯
```

pc 60x24 and pc 180x20:

```
╭ panel · %0 · server ─────────────────────────────────────╮
│ ▎1 ★ Write a prompt    a task for Claude Code from your… │
│  2   Compose           type once: fix spelling, write, … │
│  3   Explain           what is on this screen, and what… │
│  4   Tablet mode       big tiles, tap buttons, the mouse │
│  5   Close                                               │
│                                                          │
│ server · claude · /tmp/claude-1000/-home-tzachi--tmux-p… │
│ will send: screen 11 lines, 0 hidden                     │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
╰ 1-5 pick  ? keys  esc close ─────────────────────────────╯
```

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ ▎1 ★ Write a prompt    a task for Claude Code from your rough words                                                                                                              │
│  2   Compose           type once: fix spelling, write, or insert                                                                                                                 │
│  3   Explain           what is on this screen, and what next                                                                                                                     │
│  4   Tablet mode       big tiles, tap buttons, the mouse                                                                                                                         │
│  5   Close                                                                                                                                                                       │
│                                                                                                                                                                                  │
│ server · claude · /tmp/claude-1000/-home-tzachi--tmux-plugins-tmux-cockpit/9a68607b-74d7-4d99-bcb5-222b8df31b47/scratchpad/design/aud                                            │
│ will send: screen 11 lines, 0 hidden                                                                                                                                             │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
╰ 1-5 pick  ? keys  esc close ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

tablet 60x30 and tablet 100x30:

```
╭ panel · %0 · server ─────────────────────────────────────╮
│ ╭──────────────────────────────────────────────────────╮ │
│ │ ▎1 ★ Write a prompt    a task for Claude Code from … │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  2   Compose           type once: fix spelling, wri… │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  3   Explain           what is on this screen, and … │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  4   PC mode           keys only: the terminal sele… │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  5   Close                                           │ │
│ ╰──────────────────────────────────────────────────────╯ │
│                                                          │
│ server · claude · /tmp/claude-1000/-home-tzachi--tmux-p… │
│ will send: screen 11 lines, 0 hidden                     │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ keys ] [ close ]                                       │
╰──────────────────────────────────────────────────────────╯
```

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────╮
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │ ▎1 ★ Write a prompt    a task for Claude Code from your rough words                          │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │  2   Compose           type once: fix spelling, write, or insert                             │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │  3   Explain           what is on this screen, and what next                                 │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │  4   PC mode           keys only: the terminal selects text again                            │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │  5   Close                                                                                   │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────╯ │
│                                                                                                  │
│ server · claude · /tmp/claude-1000/-home-tzachi--tmux-plugins-tmux-cockpit/9a68607b-74d7-4d99-b… │
│ will send: screen 11 lines, 0 hidden                                                             │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│ [ keys ] [ close ]                                                                               │
╰──────────────────────────────────────────────────────────────────────────────────────────────────╯
```

**Keep.** The numbered tiles with descriptions, `★` on the recommended one, the
`will send:` line (it tells you what leaves the machine), and the tablet boxed
tiles that fall back to 1 row when they do not fit.

**Missing or awkward.**
- **The panel does not know the agent is blocked.** `%0` is `needs-input` with
  "Do you want to proceed?", yet Home shows a generic menu and its context line
  `server · claude · /tmp/…`. The board just showed this agent with a red border
  and `↵ answer`. The panel has no state glyph, no question and no "answer
  this" tile.
- **The identity differs from the board's.** The title is `panel · %0 · server`
  (pane id and window name). The board said `1 ▲ api:0 · server`. Neither the
  session (`api`) nor the tile number carries over.
- **At 180 columns everything is in the left 70 columns**, and 13 of 18 rows
  are blank. Descriptions are cut at 60 (`from your…`).
- The `cwd` line is the longest thing on the screen and is cut mid-path. It
  could be `~`-abbreviated or middle-truncated (the board uses `trunc_middle`).

### 2.2 Compose (empty, typing, cursor)

Golden empty (80); live typing with the cursor moved back 3 cells,
pc 100; tablet 180:

```
╭ panel · %3 · w ──────────────────────────────────────────────────────────────╮
│ type: auto   model: auto                                                     │
│ ▏type what you want…                                                         │
│                                                                              │
│ → haiku · short request - the fast model is enough                           │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
╰ ^S write  ^F fix spelling  ^T insert as typed  ⇥ type  F1 keys  esc home ────╯
```

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────╮
│ type: auto   model: auto                                                                         │
│ fix the flaky api tset then re▏run                                                               │
│                                                                                                  │
│ → haiku · short request - the fast model is enough                                               │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
╰ ^S write  ^F fix spelling  ^T insert as typed  ⇥ type  ^N model  F1 keys  esc home ──────────────╯
```

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ type: auto   model: auto                                                                                                                                                         │
│ make the board group by session▏                                                                                                                                                 │
│                                                                                                                                                                                  │
│ → haiku · short request - the fast model is enough                                                                                                                               │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│ [ write ] [ fix spelling ] [ insert as typed ] [ type ] [ model ] [ keys ] [ home ]                                                                                              │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

**Keep.** The live model route (`→ haiku · short request…`), the one-cell `▏`
cursor, and the footer that drops `^N model` at 80 but keeps it at 100.

**Missing or awkward.**
- **`?` is text here, so help is `F1`.** Every other view uses `?`, and the
  board only uses `?`. This is correct, but it is a mode switch the footer has
  to teach (it does: `F1 keys`).
- The text box has no visible frame. In pc mode at 100 or 180, the input is a
  single line among the others, easy to miss as "the place to type".
- The tablet buttons wrap to a second row with `[ home ]` alone at 80, while at
  180 they fit on one row.

### 2.3 Busy and Error

Live, pc 100 and tablet 60:

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────╮
│ ⠴ reading the screen · haiku · 1s                                                                │
│                                                                                                  │
│ sent: screen 11 lines, 0 hidden · explain                                                        │
│ model haiku: quick screen explanation                                                            │
│ times out at 120 s                                                                               │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
╰ ? keys  esc cancel ──────────────────────────────────────────────────────────────────────────────╯
```

```
╭ panel · %0 · server ─────────────────────────────────────╮
│ ⠦ reading the screen · haiku · 1s                        │
│                                                          │
│ sent: screen 11 lines, 0 hidden · explain                │
│ model haiku: quick screen explanation                    │
│ times out at 120 s                                       │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ keys ] [ cancel ]                                      │
╰──────────────────────────────────────────────────────────╯
```

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────╮
│ claude failed: Error: not logged in                                                              │
│                                                                                                  │
│ log in: run claude in a terminal and use /login, then r retries                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
╰ r retry  ? keys  esc back ───────────────────────────────────────────────────────────────────────╯
```

**Keep.** The spinner, verb, model, elapsed time and "times out at" line; the
"still working…" line after 30 s (golden `panel-busy-slow-80`); and an Error
that names the fix ("run claude … /login, then r retries").

**Awkward.** The Error footer `esc back` does not say back *to where*, though
the `?` overlay does. While Busy, the agent may have moved on, and the panel
shows nothing of the pane.

### 2.4 Question, Result, Steps, Answer, Confirm (goldens)

```
╭ panel · %3 · work ───────────────────────────────────────────────────────────╮
│ Which file should I change?                                                  │
│                                                                              │
│ ▎1 ★ ~/bin/net-proxy                                                         │
│  2   ~/bin/clip-helper                                                       │
│  3   Both files                                                              │
│                                                                              │
│ question 1 of at most 3                                                      │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
╰ 1-3 pick  o other  g best guess  ⌫ back  ? keys  esc draft ──────────────────╯
```

```
╭ panel · %3 · w ──────────────────────────────────────────╮
│ ○ step 1 inserted                                        │
│ Steps (bash)                                             │
│                                                          │
│  1 update  ✓ inserted                                    │
│      $ apt update                                        │
│  2 tap Allow in the app                                  │
│ ▎3 install                                               │
│      $ apt install jq                                    │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
╰ i insert  s send  ↑↓ pick  r retry  ? keys  esc home ────╯
```

```
╭ panel · %3 · w ─────────────────────────────────────────── 6-20 of 40 ▲5 ▼20 ╮
│ send this to %3 (bash) and press Enter?                                      │
│                                                                              │
│   echo step 6                                                                │
│   echo step 7                                                                │
│   echo step 8                                                                │
│   echo step 9                                                                │
│   echo step 10                                                               │
│   echo step 11                                                               │
│   echo step 12                                                               │
│   echo step 13                                                               │
│   echo step 14                                                               │
│   echo step 15                                                               │
│   echo step 16                                                               │
│   echo step 17                                                               │
│   echo step 18                                                               │
│   echo step 19                                                               │
│   echo step 20                                                               │
│ … 20 more lines                                                              │
╰ y confirm  ↑↓ scroll  ↵ is off: risky or multi-line  ? keys  esc n cancel ───╯
```

```
╭ panel · %3 · work ───────────────────────────────────────────────────────────╮
│ ╭───────────────────────────────────╮ ╭────────────────────────────────────╮ │
│ │             yes, send             │ │                 no                 │ │
│ ╰───────────────────────────────────╯ ╰────────────────────────────────────╯ │
│ ⚠ risky: rm                                                                  │
│ send this to %3 (bash) and press Enter?                                      │
│                                                                              │
│   rm -rf target                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│ [ keys ]                                                                     │
╰──────────────────────────────────────────────────────────────────────────────╯
```

**Keep.**
- `N-M of T ▲a ▼b` in the top border.
- Footers that drop whole and always keep `esc` and `?`, and whose `esc` names
  its destination.
- Confirm's `↵ is off: risky or multi-line`.
- Steps' `✓ inserted` after a resume.
- The tablet yes/no pair at the top, away from the bottom buttons that opened
  it.

**Awkward.**
- Result/Answer/Steps at 120+ columns leave most of the width blank.
- The `○ step 1 inserted` note on a resumed Steps view uses `○`, which is the
  board's glyph for *idle*.
- An armed Answer (`esc` once) is described in docs/tui.md:375-378 but has no
  golden of its own in pc mode.

### 2.5 The `?` overlay and the below-floor card

```
╭ keys ────────────────────────────────────────────────────╮
│ ↑↓ j k    move                                           │
│ ↵         open the selected tile                         │
│ 1-5       open a tile by number                          │
│ ? F1      this help                                      │
│ esc q ^C  close the panel                                │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
╰ any key closes ──────────────────────────────────────────╯
```

```
╭ panel ─────────────────╮
│ make the window larger │
│ or rotate              │
│ esc goes back          │
│                        │
╰────────────────────────╯
```

Tablet, 40x11 (under 28x12):

```
╭ panel ───────────────────────────────╮
│ make the window larger or rotate     │
│ esc goes back                        │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
│                                      │
╰──────────────────────────────────────╯
```

**Keep.** The full-popup overlay, one column, scrollable, with `esc` rows that
name their destination. The single "larger or rotate" card.

**Awkward.** On Home, 11 of 14 overlay rows are blank. The card at 40x11 is
plain text in a big empty box, and the tablet card has no tappable way out (this
is by design: `docs/tui.md:358-364`).

---

## Part 3: the board to panel to board handoff

From `board.rs:963-966`, `scripts/cockpit-board:61-70` and `cockpit.tmux:191-205`:

1. **`f` on a tile.** The board writes `panel %N` and exits. `cockpit-board`
   then runs `scripts/cockpit-panel -p %N` *inside the board's popup*. tmux
   allows one popup per client.
2. **What the user sees.** The popup frame still has the board's tmux title
   (`-T ' board · #{session_name} '`, `cockpit.tmux:205`), and the session named
   there is the *client's* session, not the agent's. The size is the board's
   (`-w 90% -h $PH`), not the panel's own (`-w 94% -h 90%`). Inside, the panel
   draws its own `panel · %N · <window>` frame. The tile number, the session,
   the state and the question are gone (§2.1).
3. **Tablet mode is lost.** The panel's own binding passes
   `-e COCKPIT_PANEL_MODE=<mode>` (`cockpit.tmux:191`). The board's popup does
   not. A user who set `@cockpit_panel_mode tablet` on a non-Termux machine gets
   a pc-mode panel when it is opened from the board, and a tablet panel when it
   is opened with `prefix f`. Under Termux, `auto` still detects it.
4. **Getting back.** There is no way back. `esc`/`q` on the panel's Home exits
   the panel, `cockpit-board` exits, and the popup closes onto the pane you were
   in *before* the board. The same holds after `r` review and `i` info. To
   return, you press `prefix w` again: the selection resets to tile 1, and any
   paging is lost.
5. **The keys differ between the two screens:**

| | board | panel |
|---|---|---|
| close | `q` `esc` | `esc` (Home), `q` `^C` |
| help label | `? help` | `? keys` / `F1 keys` |
| help shape | 2-column card, no scroll | full-popup list, scrolls |
| footer | own row, 3-space gaps, 3 fixed candidates | in the bottom border, drops 1 hint at a time |
| selection | thick border | `▎` bar |
| `▎` means | tmux's active pane | selection |
| digits | absolute tile 1-9 | option 1-N |
| `g` | first tile | best guess (Question) |
| `r` | review the diff | retry the call |
| `i` | info | insert |
| `f` | open the panel | follow up (Answer) |
| floor | 40x5, text "too narrow" | 28x7 pc / 28x12 tablet, "larger or rotate" |
| touch | none | tablet mode, taps, wheel |
| scroll indicator | header right, overlaps at 40 cols | top border right, gives way properly |

`r`, `i` and `f` mean different things one keypress apart. A user who presses
`f` on the board, then `r` out of habit in the panel's Result view, retries a
paid call instead of opening a review.

---

## Part 4: Top 10 improvements, ranked

1. **Return to the board after `f`, `r` and `i`.** Run the action in the popup,
   then re-run `cockpit-tui board` with the selection restored (the panel's
   `--resume` pattern already exists). Today every action is one-way, which
   makes the board a launcher and not a cockpit.
2. **Search and filter, plus session grouping.** Add `/` for an incremental
   filter on `sess:idx`, title and doing, and `s` to group tiles under session
   headers (state rank inside each group, never a clock). Show session counts in
   the header. This covers (a) and (b), and it fixes digits stopping at 9:
   numbers can restart per page, or `/` can replace them.
3. **Carry the agent's context into the panel.** Pass the state, `msg`,
   `sess:idx` and title in (env or argv). On a blocked agent, put an
   "Answer: Do you want to proceed?" tile first. Give the title the board's
   `▲ api:0 · server`. Pass `COCKPIT_PANEL_MODE` from the board's popup too, and
   retitle or resize the popup.
4. **Reclaim dead rows in the tiles.** Drop the reserved `said` row when it is
   empty (`board.rs:276/334`). Merge `—` with the state word. Bottom-align
   previews. For blocked tiles, show the question box's *options* instead of its
   tail. This gives back 2-3 rows per tile.
5. **Make the narrow and portrait layout a list.** Under about 68 columns, or
   when tiles would get 3 or fewer preview rows, switch to one line per agent
   (fleet profile A) with the selected agent's preview expanded below it. A
   phone could then see 30+ agents and not 6.
6. **Use the space at 180+ columns.** Size the grid by content, not equal thirds:
   the blocked or selected tile gets a wider and taller cell, and done or idle
   tiles collapse to 3-4 rows. Or add a 2-pane "list + big preview" layout at
   ≥160 columns.
7. **Adopt the panel's chrome on the board.** Use one `footer()` that drops 1
   hint at a time and keeps `?` and `q` (so `r f i` survive at 60 columns), and
   make it state-aware (a blocked tile shows `↵ answer`, an error shows
   `r review  f fix`). Use one `titles()`-style header that never overlaps. Make
   the `?` overlay full-popup, one column and scrollable. Pick one meaning for
   `▎`.
8. **Give the board a tablet mode** that mirrors the panel's: a tap selects and
   a second tap (or an action chip on the tile) acts, with a wheel to scroll and
   `[ jump ] [ answer ] [ panel ] [ review ]` buttons. Gate it on
   `@cockpit_panel_mode` / Termux the same way, so the touch user who opens
   `prefix w` and `f` sees one convention.
9. **A per-tile action menu, and a real inline answer.** `↵` on a tile could
   open a small chooser (jump / answer / ask / review / panel / info) with the
   state-specific default first. Then wire `deliver` so a permission prompt can
   be answered with `1` / `2` / `3` from the board without leaving it (the
   reserved verb at `scripts/cockpit-board:72`).
10. **Fix the state screens and the doc drift.** Use a short-screen message that
    is not "too narrow". Implement the `stale` header and the card the doc
    promises (`docs/tui.md:483-495`), or remove them from the doc. Centre the
    empty card and wrap its text. Fix `1-9` "on this page" in docs/tui.md:71 or
    in code. Make `]` scroll without moving the selection a full page. Fix the
    worker's off-by-one preview row count (`board.rs:763`). Unify the key
    meanings that clash between the board and the panel (`r`, `i`, `f`, `g`),
    or show the screen name in every footer so the context switch is visible.
