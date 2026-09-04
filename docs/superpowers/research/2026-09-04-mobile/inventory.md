# repo-inventory: cockpit surfaces that assume a wide client

Lane repo-inventory, issue #97, 2026-09-04. Read-only research on private `-L` sockets.

## Question

Which cockpit surfaces break, truncate, overflow or become unreachable on a client of 60x30 and of 131x58, and where in the code does each come from?

Rig: tmux 3.7c at `~/.local/bin/tmux`, fzf 0.44.1, private `-L` sockets only, a Python pty at each size, frames reconstructed with pyte. Two configs: `conf-example` is `examples/tmux.conf` plus the test palette and the worktree `cockpit.tmux`; `conf-full` is `examples/full.tmux.conf` minus tpm, resurrect and continuum, the shape of the owner's live file. Artifacts under `/tmp/repo-inventory-1957678`.

examples/tmux.conf does not source catppuccin (only full.tmux.conf does, at :170 and :217), so conf-example renders the stock tmux status bar; its rows measure stock tmux, not the cockpit status line (refute pass, 2026-09-04).

One rig defect found and corrected mid-run: `/usr/bin/tmux` is 3.4, `~/.local/bin/tmux` is 3.7c. The first key dump used 3.4 and showed no control ranges. Every measurement below was re-taken on 3.7c.

## Inventory

### 1. Status line

| surface | file:line | at 60x30 | at 131x58 | verdict |
|---|---|---|---|---|
| whole status line, 1 window | `examples/full.tmux.conf:264,268,273,275` | `  tests  󰊢  research/mobile-97 ⑂research-mobil 󰃰  07:40` | `  tests  1  bash    ~/.cache/cockpit-wt/research-… 󰊢  research/mobile-97 ⑂research-mobil 󰃰  07:40` | CONFIRMED |
| window tab list | `full.tmux.conf:214-215`, `tmux.conf:52-53` | absent with 1, 2 or 3 windows | all three tabs drawn | CONFIRMED, breaks |
| directory segment, `range=control|2` | `full.tmux.conf:258,268`, live `~/.tmux.conf:233` | not drawn at all | drawn, cols 48-83 | CONFIRMED, breaks |
| branch segment, `range=control|1` | `full.tmux.conf:262,273`, live `~/.tmux.conf:238` | drawn, cut to `⑂research-mobil` | drawn, same cut | CONFIRMED, degraded at both |
| `status-right-length 140` | `full.tmux.conf:254`, live `:219` | never binds, the client is the limit | never binds | CONFIRMED, inert |
| `status-left-length 40` | `full.tmux.conf:253`, live `:218` | never binds, segment is 10 cells | never binds | CONFIRMED, inert |
| `#{agent_fleet}` | `cockpit.tmux:78`, `tmux.conf:74` commented | renders nowhere | renders nowhere | CONFIRMED, matches #58 defect 4 |
| `#{agent_where_path}` / `_branch` | `cockpit.tmux:60-61,141-145` | both run, path segment then dropped by truncation | both drawn | CONFIRMED |

The truncation direction is measurable. At 80 columns the line read `  tests ockpit-wt/research-… 󰊢 …`, so tmux cuts status-right from the left and slices the path mid-word with no ellipsis. Bisect with two windows, owner config:

| client width | window tabs |
|---|---|
| 80 | none |
| 96 | `<a` |
| 104 | window 2 only, `<` marker on the left, window 1 eaten (corrected 2026-09-04) |
| 112 | `<bash    2  bash`, window 1 partly eaten |
| 120 | both tabs whole |

The #58 finding that fleet columns get cut under 150 columns is confirmed at a new width and not re-derived. See `docs/superpowers/research/2026-08-26-ui-audit/README.md` and issue #58 defects 5 and 6.

### 2. Pane border

| surface | file:line | at 60x30 | at 131x58 | verdict |
|---|---|---|---|---|
| `pane-border-status top` | `full.tmux.conf:224`, `tmux.conf:31` | on | on | CONFIRMED |
| NEEDS YOU, owner config, no question text | `full.tmux.conf:235` | `─── NEEDS YOU ───…───[z][x]` fits | same, fits | CONFIRMED |
| NEEDS YOU with the 48-char question | `tmux.conf:34` | `─── NEEDS YOU Should I force-push the rebased branch to orig` and `[z]` and `[x]` are pushed off the line | `…origin…` plus `[z][x]` at the right edge | CONFIRMED, breaks |
| `[z]` `[x]` click targets | `tmux.conf:34`, `full.tmux.conf:235` | present when idle, gone when the pane is blocked | always present | CONFIRMED |

At 60 columns the two mouse-only pane buttons disappear exactly in the state where a phone user most needs them. A 60-column pane inside a wider client behaves the same way, since the format is per pane.

### 3. Popups and pickers

Popup interior cell sizes, measured by running `stty size` inside each popup on a private server.

| popup | file:line | 60x30 | 131x58 |
|---|---|---|---|
| pickers, -w 70% -h 70% | cockpit.tmux:209-210,229,235,247,261,294,302 | 40x19 | 89x38 |
| board, -w 90% | cockpit.tmux:331 | 52x19 | 115x38 |
| side fix, -h 6 | cockpit.tmux:310 | 40x4 | 89x4 |
| what is each agent doing, 78% 40% | full.tmux.conf:135 | 44x10 | 100x21 |
| sessions from the prefix m menu, 60% 50% | full.tmux.conf:138 | 34x13 | 76x27 |

The interior is two columns and two rows smaller than the outer box in every case.

What each picker renders. CONFIRMED, each run in a pty of the interior size above.

| picker | file:line | at 40 cols | at 89 cols | verdict |
|---|---|---|---|---|
| tm session picker | scripts/tm:184-190 | header, rows and preview all fit | fits, 16 blank rows | holds |
| branch picker | scripts/tmux-branch:191,233-241 | key hints cut mid-word, every row's age and worktree column cut | full hints, full rows | breaks |
| fleet queue | scripts/tmux-review:2219-2227,2272-2279 | only state and a cut title; repo-branch and the diff-stat column off-screen; both header lines cut | diff-stat column still cut | breaks at both |
| pane info card | scripts/tmux-review --info | every long value wraps to a second line; the pane row and the close hint scroll off 19 rows | 13 rows, fits | breaks |
| side fix prompt | scripts/tmux-fixer | prompt is 37 cells of 40, leaving 3 to type on the first line | 52 free | degraded |
| board, Rust | tui/src/board.rs:20,94,96,101 | 52 cols is above MIN_BOARD_W 40 but below 2 x MIN_TILE_W 48, so one tile column | 115 cols, two tile columns | PLAUSIBLE, bin/cockpit-tui not installed in this worktree, constants read from source |

Branch picker at 40x19:

```
switch>                             8/8
  ↵ switch   alt-↵ new branch from wh..
  on research/mobile-97 · ~/.cache/co..
> ● research/mobile-97               ..│
    master                           ..│
    origin/docs/tui                  ..│
```

The `^/ preview` and `esc cancel` hints are gone. The row format at scripts/tmux-branch:191 is `%s %-32s %-16s %s`, so the ref field alone takes 34 of the 40 columns and the age never fits. At 89 the rows read in full.

Fleet queue at 40x19:

```
fleet>                              3/3
  which agent needs you   [◆ review p..
  ↵ review, or jump to a clean pane  ..
> needs-input     henosis:0 claude ..
  done            node-dev:0 claude..
  running         henosis:1 api    ..
```

At 89x38 the last column is still cut:

```
> needs-input     henosis:0 claude        research-mobile-97⑂research/mobile-97   +55 ..
  done            node-dev:0 claude       ?⑂?                                     no r..
  running         henosis:1 api           tmux-cockpit⑂master                     +0 -..
```

The ± column is cut whenever the widest repo⑂branch field overflows its 32-character cap. With the owner's own `research-mobile-97⑂research/mobile-97` (37 characters) the queue needs 100 columns for the ± column; at 89 and at 95 it is cut. A fleet of short repo and branch names keeps the column at 89. The caps at scripts/tmux-review:2220-2221 are ceilings, and `pad_field` lets a longer field overflow rather than truncate, so the threshold is a property of the fleet, not of the popup (refute pass, 2026-09-04). Issue #58 defect 5 said the column gets cut under about 150 terminal columns; the cut is now measured.

Pane info card at 40x19:

```
cwd       /home/tzachi/.cache/cockpit-wt
/research-mobile-97
root      /home/tzachi/.cache/cockpit-wt
/research-mobile-97
state     needs-input
...
review dir /home/tzachi/.local/state/tmu
x-cockpit/review/0-2511474410
           0 pinned · 0 delivered · ^A r
emoves it
```

The card needs 16 rows at 40 columns against 19 available, and the first row naming the pane plus the closing hint scrolled off. At 89x38 it is 13 rows with both present.

Width blindness is the root cause under most of the above. `tput cols` appears nowhere in scripts/. Only `tput lines` at tmux-branch:221, tmux-open-path:769 and tmux-review:2233. Every layout sizes its preview from the height and never asks the width. The one exception is FZF_PREVIEW_COLUMNS handed to bat and delta at tmux-open-path:363,407,415.

Other popup sites: tmux-open-path:686-691 reuses the `@cockpit_popup_width/height` options at click time; tmux-review:2026 does the same for the review-request popup; scripts/cockpit-board:31-34 is the "not installed" path.

### 4. Keys and mouse

On tmux 3.7, `range=control|N` triggers the `ControlN` key, a location suffix, not the Ctrl modifier. `man tmux` line 3705 says so, and a bare SGR press and release written into the client pty with no modifier opened the branch picker at both 131x58 and 60x30. CONFIRMED. README.md:5,291 and KEYS.md:47-48 call this gesture "Ctrl+click the branch segment". That is wrong for 3.7 and understates what a phone does with a plain tap.

At 60 columns the status bar's touch targets do not sit under the text that names them. Measured by binding a logging run-shell for each MouseDown1*Status* key on a private server and pressing every column of the status row.

60x30, three windows:

| cols | key fired | what is drawn there |
|---|---|---|
| 1-10 | MouseDown1StatusLeft | `  tests ` |
| 11-33 | MouseDown1Status, window switch | the branch text (refute pass measured 11-33 with three windows named bash; the file's 11-36 held with a window named `bashXYZ`, so the Control1 left edge floats with the window-list width, corrected 2026-09-04) |
| 34-51 | MouseDown1Control1, branch picker | the rest of the branch text |
| 52-60 | MouseDown1StatusDefault | the clock |

60x30, one window:

| cols | key fired |
|---|---|
| 11 | MouseDown1Status |
| 12 | MouseDown1Control2, one column wide |
| 13-51 | MouseDown1Control1 |

131x58, three windows:

| cols | key fired | what is drawn there |
|---|---|---|
| 1-10 | MouseDown1StatusLeft | session segment |
| 11-45 | MouseDown1Status | the three tabs |
| 46-47 | MouseDown1StatusDefault | fill |
| 48-83 | MouseDown1Control2 | the directory text |
| 84-122 | MouseDown1Control1 | the branch text |
| 123-131 | MouseDown1StatusDefault | the clock |

At 131 every target sits under its own text. At 60 roughly half of what a phone user sees as the branch segment switches to a window whose name is not drawn anywhere, and the folder browser is one column wide with one window and absent with three. Not in issue #58.

Modifier inventory. The prefix is C-Space at full.tmux.conf:5 and live ~/.tmux.conf:4-5. The Termius extra key row supplies sticky Ctrl and Alt, Home, End, Pg Up, Pg Dn, Tab, shift-tab, Delete and arrows (phone-0710.png). There is no Shift+Enter key on that row, so the `extended-keys always` path at full.tmux.conf:27 has no producer on the phone.

| binding | file:line | phone reachability |
|---|---|---|
| prefix C-Space | full.tmux.conf:5 | sticky Ctrl then space, two taps |
| M-Left/Right/Up/Down pane moves | full.tmux.conf:62-65 | sticky Alt then an arrow, two taps |
| prefix M move window, prefix R reload | cockpit.tmux:440,388 | shift plus a letter, reachable, both have Shift-free rows per KEYS.md:26-28 |
| prefix a u r i f w b g | cockpit.tmux:471,269,294,302,310,331,247,261 | one modifier plus one letter, reachable |
| C-DoubleClick1Pane, C-MouseDown3Pane, M-MouseDown1Pane path picker | cockpit.tmux:533-557 | a chorded double-tap or a right-click, not a touch gesture, unreachable |
| MouseDown1Status window switch | tmux default | plain tap, but nothing is drawn to tap at 60 |
| MouseDown1Control1 branch picker | cockpit.tmux:229 | plain tap, CONFIRMED at both sizes |
| MouseDown1Control2 files browser | cockpit.tmux:235 | plain tap, but 1 column wide at 60 with one window, 0 with three |
| MouseDown1Control8/9, the [z] and [x] buttons | tmux.conf:34, full.tmux.conf:235 | plain tap |
| MouseDragEnd1Status, the #21 fix | cockpit.tmux:246 | plain tap on the status row only |

The lowercase-key rule from #43 pays off: every cockpit key is one modifier plus one letter, so nothing in the plugin's own key set is out of reach of the soft keyboard. The two gestures out of reach are both in the path picker, chorded by design per cockpit.tmux:484-511.

Stuck drag flag, #21 and #53. cockpit.tmux:246 binds MouseDragEnd1Status to `switch-client -t =`, which recovers a tap lost on the status row. Pane and border drag-ends are left unbound on purpose. On a phone every interaction is a tap into a popup or a menu that eats the release, so the uncovered pane and border cases get hit more often than on a desktop. Not reproduced directly; read from the bindings, PLAUSIBLE.

Measurement caveat: the first tap scan reused a client across taps without dismissing the popup each time, which made three columns read as "popup opened" when the popup was still up from the previous tap. The column-to-key tables above come from the logging-bind instrument, which has no such failure mode.

### 5. Hooks that redraw

| what | file:line |
|---|---|
| four refresh-client -S hooks | full.tmux.conf:249-252, live ~/.tmux.conf:214-217 |
| status-interval 2 | full.tmux.conf:247, live ~/.tmux.conf:212 |
| refresh-client inside the plugin | none. cockpit.tmux has zero. agent-state:395-398 documents why it stays off the transition path, at 19.6 ms per call |
| refresh-client in scripts | scripts/tmux-branch:72, once after a branch switch |
| window-size, aggressive-resize | set nowhere in the repo. tmux defaults are `latest` and `off`. The live server has `latest` and `aggressive-resize on` |

Redraw count. One 60-second window per row, n=1, idle client, no keystrokes, no agent output, 12 seconds of startup discarded.

| client | config | bytes to the client | cursor addressings | status repaints |
|---|---|---|---|---|
| 60x30 | conf-full | 441 | 2 | 1 |
| 131x58 | conf-full | 693 | 2 | 1 |
| 60x30 | conf-example | 101 | 2 | 1 |

An idle phone receives one status repaint per minute, at the minute rollover, because tmux diffs the screen and the clock is the only cell that changes. Zero VPA in every idle window. One ECH appears per idle minute wherever the status line has a gap to clear between the window list and status-right: `\e[26X` at 131x58 with conf-full (n=2) and `\e[17X` at 60x30 with conf-example. Only 60x30 with conf-full, whose status line is full edge to edge, emits none (refute pass, 2026-09-04).

The cost `status-interval 2` buys is on the server, not the wire. Swapping both status segments for counting jobs on a private server gave 60 shell spawns per 60 seconds idle, 30 status evaluations times two `#()` jobs, for one visible repaint. cockpit.tmux:63-78 already moved the fleet count off this path. The path and branch jobs at cockpit.tmux:60-61 are still on it. Same on a phone and a desktop.

### 6. Where a phone command fits

Located only, not designed.

scripts/tm is the only place in the repo that creates or attaches a session. The seam is `go()` at scripts/tm:77-90: `tmux new-session -d -s "$name" -c "$dir"` at :83, then `switch-client` at :86 or `attach-session` at :88 depending on `in_client()`. The argument case at scripts/tm:99-123 already carries -h, -l, --rows, --kill-one and -k, so a `--phone` arm is one more case arm plus one branch inside `go()`.

cockpit.tmux needs nothing new. It binds tm at cockpit.tmux:261 as prefix g through the same popup chrome.

The other new-session call site is cockpit.tmux:444, in the move-window menu. full.tmux.conf:138 is not a call site at all: it shells out to ~/.local/bin/tm and so goes through `go()` like everything else (refute pass, 2026-09-04; #58 defect 11 covers the hardcoded path).

window-size is unset everywhere in the repo, so tmux's `latest` applies, and a phone attaching to a shared session resizes every window to the phone's size for the desktop client too. cockpit.tmux:75 documents that a grouped session makes `#{agent_fleet}` read high because a format cannot de-duplicate pane ids, only ever high, never low.

Orchestrator's note, 2026-09-04: this lane's text said a grouped session "keeps its own size". practice.md measured the opposite and the refute pass confirmed it (verified.md claim 2a): grouped sessions share window size. The seam location stands; the phone arm must create an independent session, not a grouped one.

## Findings outside scope

1. README.md:5,291 and KEYS.md:47-48 document the branch and directory clicks as "Ctrl+click". On tmux 3.7 a plain click fires them, confirmed with a bare SGR press. A documentation defect that hides a gesture the phone already has.
2. `#{agent_fleet}` renders in no shipped config and not in the owner's live file: full.tmux.conf never mentions it, tmux.conf:74 has it commented, the live status-right at :233,238,240 has no fleet token. Same as #58 defect 4, but below 120 columns the window tabs are gone and the fleet string is the only remaining signal for a session that needs you.
3. The diff-stat column of the fleet queue is cut at 89 columns, the picker interior of a 131-column client, when a repo⑂branch field overflows its 32-character cap; the owner's own worktree names need 100 columns. #58 defect 5 predicted "under about 150"; now measured.
4. pane-focus-in at full.tmux.conf:252 fires on terminal focus. A phone that backgrounds and foregrounds the SSH app repeatedly pays two shell spawns each time. #58 defect 12 and the ui-audit README decision 6 already default to dropping it.
5. Rig defect for the next lane: /usr/bin/tmux on this box is 3.4 and ~/.local/bin/tmux is 3.7c. A probe that hardcodes /usr/bin/tmux silently loses every control range and reports the branch and directory clicks as unbound.

## Tools

Harness tools: Bash, Read, ToolSearch, SendMessage. No skills. Shell steps and reasons:

- `gh issue view 97`, `gh issue view 58`, AGENTS.md and the ui-audit README, to take the constraints and avoid redoing the ui-audit lane's work.
- cat, sed and grep over cockpit.tmux, both example configs, ~/.tmux.conf read-only, scripts/*, tui/src/board.rs, KEYS.md and README.md, for the inventory with file:line.
- `pip install --target` for pyte, so rendered frames were reconstructed from the pty byte stream instead of guessed. pyte's Screen needed two no-op overrides for report_device_status and write_process_input.
- hold.py, probe2.py, probe3.py fork a pty at a given size running `tmux -L <private> -f <conf> new-session`, for real frames at 60x30 and 131x58.
- `display-popup -E "stty size > file"` on a private server, to measure popup interiors.
- count.py drains 12 seconds of startup then 60 seconds of idle and counts bytes, VPA, CUP, EL and ECH.
- jobs.py swaps both status segments for counting jobs, to separate server-side evaluations from client-side repaints.
- inpty.py runs one script in a pty of a popup's interior size, to render tm, tmux-branch, the fleet queue, the info card and the side fix.
- tap.py, tap2.py, tapscan.py write raw SGR mouse press and release bytes into the client pty.
- rangescan.py binds a logging run-shell on a private server for each MouseDown1*Status* key and presses every column.
- `man tmux | grep range=`, to settle whether Control1 is a modifier or a location.

Two mistakes for the next lane. `command -v tmux` evaluated inside a PATH that already held the shim made the shim exec itself and hung one call for two minutes; resolve the real binary before prepending the shim. scripts/tmux-branch inherits $TMUX from the agent's own pane and calls refresh-client on it at tmux-branch:72, so every script probe ran under `env -u TMUX -u TMUX_PANE`.

## Leak sweep

```
$ ls /tmp | grep -i 1957678
repo-inventory-1957678
$ ls /tmp/tmux-1000 | grep -i "^riv"
(no match)
$ pgrep -af "exec -a claude sleep|sleep 900|sleep 600"
(none)
$ tmux -L default ls
henosis: 2 windows (created Wed Aug 26 07:37:43 2026) (attached)
tests: 1 windows (created Wed Aug 26 08:09:32 2026) (attached)
```

Thirty-one private sockets created, all killed, socket files removed by hand. /tmp/repo-inventory-1957678 kept as evidence. The live server was read with list-clients, show-options and ls only.

## Did / Did not / Could do next

Did: inventoried all six scope items with file:line. Measured status rendering at both sizes, the width at which window tabs disappear, five popup interiors, five pickers, the pane-border banner under both shipped border formats, the status column-to-key map, idle redraw bytes and shell spawns. Located the seam for a phone command.

Did not: name the corruption mechanism (terminal lane). Exercise the Rust board, bin/cockpit-tui not installed in the worktree. Test tmux-open-path's picker or the review comment loop. Test on Termius itself; every touch claim rests on the SGR bytes a tap produces.

Could do next: measure the redraw burst a real agent state transition sends to a 60-column client. Check whether the range-versus-drawing misalignment at 60 columns is tmux computing status ranges before clipping, which would make it an upstream report. Price a grouped session against `#{agent_fleet}` reading high.
