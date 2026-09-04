# Phone client: screen corruption and a phone path (issue #97)

Research folder for #97, 2026-09-04. Three read-only lanes, then a refute pass per lane (verified.md). tmux 3.7c, every probe on a private `-L` socket, the live server read only.

| file | lane | question |
|---|---|---|
| terminal.md | terminal-mechanism | which sequence the phone mishandles, and the tmux-side setting that avoids it |
| practice.md | phone-practice | what tmux-from-a-phone users do, same config or a separate command |
| inventory.md | repo-inventory | which cockpit surfaces break at 60x30 and 131x58, with file:line |
| verified.md | refute lanes | each load-bearing claim re-run: CONFIRMED, TIGHTENED or REFUTED |
| artifacts/ | orchestrator | the owner's two screenshots, the 131x100 byte capture, the pty probe |

## What was observed

The owner's phone (Termius on Android, TERM `xterm-256color`, client size reported 131x100, about 58 rows visible) showed ghost copies of the status line stacking upward, one more per minute, and shell prompts vanishing while typed text stayed at the column the prompt had left it at.

## Measurements

| what | value | where |
|---|---|---|
| status write on the last row | ends at column 131 of 131, the bottom-right cell | terminal.md, Sequences |
| same, with `terminal-overrides ',xterm-256color:am@'` | ends at column 130 | terminal.md |
| same, client TERM `xterm-ghostty` with the override | still 131, so the override is TERM-scoped | terminal.md |
| eager-wrap terminal model, status bottom, 6 repaints | 6 scrolls, 9 status rows on screen | terminal.md, Hypotheses |
| deferred-wrap model, same bytes | 0 scrolls, 1 status row | terminal.md |
| eager-wrap model, status top | 0 scrolls | terminal.md |
| idle status repaints per minute | 1, at the minute rollover (n=1) | inventory.md, section 5 |
| idle server-side status evaluations per minute | 30, two `#()` jobs each, 60 shell spawns (not re-run in the refute pass) | inventory.md |
| phone attaches to the desktop's session, `window-size latest` | desktop window 191x44 → 60x29 | practice.md, Shape b |
| grouped session `new-session -t` | desktop still shrinks, 60x29 then 45x25 | practice.md, verified.md 2a |
| independent session `new-session -A -s phone` | desktop stays 191x44 through attach, rotate, detach | practice.md, verified.md 9 |
| `attach -f ignore-size` | desktop stays 191x44, phone pans a 191-wide window | practice.md, verified.md 3 |
| width at which window tabs vanish, two windows, owner config | absent at 80, whole at 120 | inventory.md, section 1 |
| picker popup interior at 60x30 / 131x58 | 40x19 / 89x38 | inventory.md, section 3 |
| fleet queue diff-stat column | cut at 89 columns (the 131-col popup interior) when a repo⑂branch field overflows its 32-char cap; the owner's names need 100 | inventory.md, verified.md 4 |
| status touch targets at 60 columns | half the branch text switches windows, folder browser 0 to 1 column wide | inventory.md, section 4 |
| `range=control|N` on 3.7c | a plain tap fires it, no Ctrl needed | inventory.md, section 4 |

## Cause

Two separate defects.

1. Ghost status rows. tmux writes the bottom-right cell of the last row (`tty.c:2257` skips it only when the terminal lacks `am`). A terminal that wraps eagerly at the last column, instead of holding a pending wrap the way xterm does, scrolls the screen one row on every full-width status repaint. The previous status stays one row up as a ghost, and every later absolute write lands one row below the content it belongs to, which is what erases the prompts. Confirmed as a sufficient mechanism by replaying the owner's own bytes through a 60-line terminal model, then observed on the device: on 2026-09-04 the owner applied the override on the live server and reattached the phone, and the ghost rows stopped.
2. The client reports 100 rows for about 58 visible. PLAUSIBLE; the visible-row count is inferred from pixel pitch, not measured, and a later attach reported 131x60. Consistent with Termius not resizing the pty when its keyboard opens. tmux cannot correct a client's size from the server (`refresh-client -C` is control-mode only). Separate defect, separate fix, peer-app bug.

The `\r\n` walk phone-practice noticed (the full redraw writes the bottom status after a bare `\r\n`, from the `tty_cursor` shortcut at `tty.c:2487`) is real but is not the ghost cause: each step carries `\e[K`, and the ghosts reproduce at a matched size with no row lie.

## Recommendation

Same config file. One override, one entry command, no width conditionals.

```
# examples/full.tmux.conf and the owner's ~/.tmux.conf
set -as terminal-overrides ',xterm-256color:am@'
```

```
# phone app's startup command
tmux new-session -A -s phone
```

The override costs one trailing space on the phone's status line and touches no other client. The entry command gives the phone its own session, so the desktop keeps its layout; a grouped session does not do that. Never link windows between the two sessions: a linked window shrinks the desktop at attach time, and `aggressive-resize` does not rescue it under `window-size latest`.

## Decisions for the owner

1. Ship the `am@` override in the example configs, or only document it? Options: A ship it, keyed to `xterm-256color` (the TERM every phone app sends; ghostty, kitty and alacritty send their own) / B document only. Recommended: A. Answered by the owner 2026-09-04: A.
2. Phone entry: document the one-line command, or add repo code? Options: A document the command in README under a "From a phone" heading / B add a `tm --phone` arm. Recommended: A. Answered by the owner 2026-09-04: both, document the command and add a full-screen toggle key (one prefix key on the phone session that hides the status line and pane borders and brings them back); a "bigger UI" was asked and withdrawn, tmux cannot change the font. Child issue #99.
3. Run the two device probes in terminal.md before the fix ships? Answered by the owner's run on 2026-09-04: the override stopped the ghosts on the phone, so hypothesis 1 holds on the device. The Termius version was not read against its 7.2.2 and 7.5.5 changelog fixes.
4. The 60-column surfaces (tabs degrade below 120 columns and are gone at 80, touch targets misaligned, pickers width-blind, `[z][x]` pushed off a blocked pane's border): fold into #58's width-aware rows, or a new issue? Options: A fold into #58, which already owns width-aware rows and the fleet column cut / B new issue. Recommended: A, one owner for width. Answered by the owner 2026-09-04: A.
5. `status-position top` as a fallback: keep documented only? Options: A documented as the diagnostic and fallback, never combined with `am@` / B ship it instead of `am@`. Recommended: A, `am@` covers pane rows too, top does not; together they drop a visible glyph from the bottom pane row (verified.md, terminal claim 1). Answered by the owner 2026-09-04: A.

Asked 2026-09-04, all five answered by the owner the same day (#97 comment).

## Rejected

| idea | why |
|---|---|
| width-conditional status (`#{e|>:#{client_width},120}`) | works, but the phone reports 131 columns and takes the wide branch; the defect is height and wrap, not width. Keep for a genuinely narrow client; use the `e|` form, `#{>:}` compares strings |
| grouped session | shares window size, desktop shrinks with the phone (measured) |
| `window-size largest` or `manual` globally | protects the desktop, phone reads a 191-wide window or every window freezes at default-size |
| `attach -f ignore-size` as the daily path | isolates size but the phone pans; keep as the safety net for an accidental attach to the desktop session |
| `status off` | the last-row write still reaches the bottom-right cell from pane content |
| shorter `status-right-length` | tmux pads the status to full width anyway (asserted by two lanes, not re-run in the refute pass) |
| `refresh-client -C` to fix the row count | control-mode clients only |
| a second tmux config for the phone | two files for one line |
| a phone TUI | non-goal |

## Fail-first tests

Not applicable to the research PR. For the implementation: a test in tests/run.sh that sources examples/full.tmux.conf on a private socket, attaches a 131x58 xterm-256color pty, and asserts the last-row write ends at column 130; remove the override and it must read 131.

## History

- 2026-09-04: three lanes dispatched and landed. Refute pass, verdicts per row in verified.md: practice.md one sub-claim REFUTED (aggressive-resize does work under `window-size smallest`); terminal.md two more TERM_NOAM consumers priced at +2.5% bytes, and `am@` drops a visible glyph when the last row is a pane row, so never pair it with `status-position top`; inventory.md the ECH-idle sub-claim REFUTED and the 60-column boundaries tightened. Corrections dated in place. Decisions posted on #97 the same day; the owner confirmed the override on the device.
