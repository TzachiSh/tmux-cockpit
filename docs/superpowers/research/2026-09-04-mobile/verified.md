# verified.md: refute pass on the #97 research

Fresh lanes graded the load-bearing claims by running them again. CONFIRMED, TIGHTENED (true with a narrower statement) or REFUTED, each with the command run and its output.

## practice.md (lane refute-practice, 2026-09-04)

Probes on private `-L` sockets named `rp*-2032807`, all `-f /dev/null`, all killed. The live server was never written to.

| # | Claim | Verdict | What ran | Output |
|---|---|---|---|---|
| 1 | `window-size` table: latest/smallest shrink to 60x29 on attach, largest keeps 191x44, manual keeps the session's `-x -y` | TIGHTENED | a real 191x45 pty client runs `new-session -s base -x 191 -y 45`, the option is set, size read, then a 60x30 pty client attaches | latest 191x44 → 60x29; smallest 191x44 → 60x29; largest 191x44 → 191x44; manual 191x44 → 191x44 when set after the desktop client is attached, 191x45 → 191x45 when set before any client sizes the window. The manual row is order-dependent. |
| 1b | `-x 191 -y 45` sets `default-size` for the session | CONFIRMED | `show-options -g default-size` vs `show-options -t base default-size` | global `80x24`, session `191x45` |
| 2a | A grouped session does NOT isolate, even with `window-size manual` | CONFIRMED | desktop 191x45 pty on `base`, `new-session -d -t base -s phone`, 60x30 pty attaches to `phone`, rotates to 45x26, then `set -g window-size manual` | G0 both 191x44. G1 both 60x29. G2 both 45x25. G3 with manual both 191x44 together (file said 191x45). |
| 2b | An independent session DOES isolate | CONFIRMED | same rig without `-t` | D0 base:0 191x44. D1 base:0 191x44, phone:0 60x30. D2 base:0 191x44, phone:0 60x29. D3 phone rotated, base:0 191x44, phone:0 45x25. D4 after detach, base:0 191x44. |
| 3 | `attach-session -f ignore-size` keeps base:0 at 191x44 and shows in `#{client_flags}` | CONFIRMED | 60x30 pty runs `attach -f ignore-size -t base`, rotates to 45x26 | base:0 191x44 throughout. Clients: `191x45 [attached,focused,UTF-8]`, `60x30 [attached,focused,ignore-size,UTF-8]` |
| 4a | `#{>:60,120}` true, `#{e|>:60,120}` false | CONFIRMED | `display -p` on each literal | `#{>:60,120}` → 1, `#{e|>:60,120}` → 0, `#{>:131,120}` → 1, `#{e|>:131,120}` → 1 |
| 4b | A width threshold renders differently on a 60 and a 191 column client at the same moment | CONFIRMED | both formats on one session, two live ptys, streams captured and stripped | desktop: `[base]  0:a-very-long-window-name-…FULL cpu 12% mem 4.1G net eth0 Tzachi-lep 08:02`; phone: `[bas]  0:a-very…SM 08:02` |
| 4c | At exactly 131x100 the `>120` branch takes the wide path | CONFIRMED | third pty at 131x100 on the same session | `131x100 ... FULL(wide)`, full desktop status rendered |
| 5 | `display-popup -w 90% -h 60%` on 60x30 yields near 52x16; `-w '#{client_width}'` rejected | CONFIRMED | popup running `stty size` | `16 52`; `-w '#{client_width}'` → `width invalid`. `#{popup_width}` inside the popup printed empty. |
| 6 | Bare SGR press and release on the status line of a 60x30 client changes the current window | CONFIRMED | three windows, current 2, then `\e[<0;9;30M` and `\e[<0;9;30m` to the pty | current `2:two` → `0:bash`. Acting binding is stock: `bind-key -T root MouseDown1Status switch-client -t =` |
| 7 | `refresh-client -C 131x58` on a normal client prints `not a control client` | CONFIRMED | with and without `-t` | `not a control client` both ways |
| 8 | `link-window` makes the shared window shrink from the phone regardless of `aggressive-resize` | TIGHTENED | linked window, phone on the phone session, 2x2 of `aggressive-resize` × `window-size` latest/smallest | Shrink confirmed and worse than stated: base:0 goes to 60x29 the moment the phone attaches, while still on phone:0, never having viewed the shared window. "Regardless of aggressive-resize" holds only under `window-size latest`. |
| 8b | The search-snippet `aggressive-resize` claim "did not reproduce; PLAUSIBLE at best" | REFUTED | the same 2x2 matrix | Under `window-size smallest` with `aggressive-resize on`, base:0 returns to 191x44 as soon as the phone switches away from the shared window. The lane's test ran under `latest`, the one setting that ignores `aggressive-resize`. |
| 9 | `tmux new-session -A -s phone` from the phone leaves the desktop untouched | CONFIRMED | server holds `base` with a 191x45 pty, a 60x30 pty runs the command, then resizes to 45x26, exits, re-runs | R0 base:0 191x44. R1 base:0 191x44, phone:0 60x29, no group. R2 base:0 191x44, phone:0 45x25. R3 reattached, base:0 191x44. |

### aggressive-resize matrix (new measurement)

Linked window `link-window -s base:0 -t phone:9`, desktop 191x45 on `base`, phone 60x30 on `phone`. Column is base:0, the same window object as phone:9.

| window-size | aggressive-resize | phone on phone:0 (away) | phone on phone:9 (shared) | phone switched away again |
|---|---|---|---|---|
| latest | on | 60x29 | 60x29 | 60x29 |
| latest | off | 60x29 | 60x29 | 60x29 |
| smallest | on | 60x29 | 60x29 | 191x44 |
| smallest | off | 60x29 | 60x29 | 60x29 |

The shared window shrinks on attach, before the phone ever selects it. `aggressive-resize` is inert under `latest` (the owner's setting), and under `smallest` recovers the window only on the next window switch.

Corrections applied to practice.md in place, dated 2026-09-04. The Recommendation and Rejected sections stood.

Leak sweep pasted by the lane: `ls /tmp/tmux-1000 | grep 2032807` empty; `/tmp/rp-2032807` kept as evidence; no `tmux -L rp` process; `tmux ls` shows only henosis and tests.

## terminal.md (lane refute-terminal, 2026-09-04)

Graded at 23314a9 (9e11e54 only added the am@ chain section). Own probes on private `-L refutterm-*` sockets, own cursor tracker and VT model, tmux 3.7c, source at db24c029.

| # | Claim | Verdict | What ran | Output |
|---|---|---|---|---|
| 1 | bottom-right cell written; `am@` stops at 130; ghostty still 131 | CONFIRMED | own pty at 131x58 and 131x100, TERM xterm-256color, `status-interval 1`, `status-right "#(date +%T)"`, replayed through an own cursor tracker | 131x58 base 131, `am@` 130; 131x100 base 131, `am@` 130; owner's capture 131 |
| 1b | scope is the client's TERM | CONFIRMED | same conf including the `am@` line under TERM xterm-ghostty | 131. Inert for a non-matching TERM |
| 1c | the override reaches TERM_NOAM | CONFIRMED | `tmux -vv` server log | `NOAM flag is 1`, `xterm-256color override: am@` |
| 2 | one scroll per full-width status repaint at bottom, zero at top | CONFIRMED | own 58-row VT model over own captures, n=8 repaints | bottom/EAGER 8 scrolls, 8 status rows at grid rows 50-57; bottom/deferred 0, 1; top/EAGER 0, 1; top/deferred 0, 1 |
| 3 | the `\r\n` walk is ruled out because each step carries `\e[K` | CONFIRMED, survives the size lie | a stream captured from a 100-row pty fed into a 58-row deferred model | deferred 1 status row (42 scrolls); same stream 58-row EAGER 8 status rows; 100-row model deferred 0 scrolls 1 row, EAGER 8, 8 |
| 4 | `status off` still writes column 131 on the last row | TIGHTENED | `status off`, idle vs a screen filled with 131-char lines | idle: zero writes on the last row, max column 42. Filled: last row reaches 131, glyph `#`. With `am@`: 130, cell blank |
| 5 | the cited source lines | CONFIRMED, two precision notes | read every cited line at db24c029 | all land where the file says; 2487 and 2570 are comment lines, conditions follow, applied in place |
| 6 | the device probe writes exactly `cols` chars and discriminates wrap modes | CONFIRMED | ran verbatim in a bare 131x58 pty, replayed in both wrap modes | `\e[58;1H` then exactly 131 `#`. Deferred: `#` row at index 57, marker intact. Eager: at 56, row 57 blank, marker destroyed |

Decisive bytes for claim 1, SGR stripped:

```
base:  \e[58;1H[tests] 0:bash*\e[108X\e[108C07:59:40\e(B     <- 8-char clock, ends col 131
am@:   \e[58;1H[tests] 0:bash*\e[108X\e[108C07:59:4\e(B      <- 7 chars, ends col 130
```

The clock is truncated by one character, as `tty_putn` at tty.c:713 computes: `len = sx - cx - 1`, cx 123, sx 131, gives 7. That probe's status-right has no trailing pad; the shipped theme's date module ends with a space (the catppuccin module invoked at examples/full.tmux.conf:275), so on the owner's config the dropped cell is that pad and the clock survives (review pass, 2026-09-04, measured: base ends at 131 with a space, am@ at 130 with the clock's last digit).

### What else `am@` changes

TERM_NOAM has six consumers at db24c029, not the four the file named: tty-term.c:511, tty.c:677, 703, 713, 2110, 2257, 2431. 2110 (`tty_cmd_cells`) and 2431 (`tty_cursor_pane_unless_wrap`) disable the "let the terminal wrap" shortcut for full-width pane lines. Priced: 260-column lines wrapping on 131 columns cost 35,192 bytes with `am@` against 34,324 without (+2.5%), identical rendering; a full-screen fill 9,051 against 8,679 (+4.3%).

| configuration | cost of the skipped cell |
|---|---|
| status at the bottom | the status pad, a space, nothing visible |
| `status off` or `status-position top` | the last row is a pane row; a real glyph is dropped while that content sits on the bottom row |
| copy mode | same choke point, 131 without, 130 with |
| wrapped long lines | no degradation, only the byte cost above |

So `am@` and `status-position top` are fine separately and worse together. Corrections applied to terminal.md and README decision 5 in place, 2026-09-04.

### Methodology warning

The refute lane's first pass falsely refuted claim 1: its tracker consumed two of the three bytes of `\e(B` (designate ASCII into G0, emitted after every status paint) and treated the trailing `B` as a printable, which landed on the pending-wrap cell and fabricated a scroll on the base run while hiding the truncation on the `am@` run. Treating `\e(`, `\e)`, `\e*`, `\e+` as three-byte sequences moved every number to agree with the file. A fixed-duration pty drain also cut mid-frame once and produced a spurious column-131 hit; drain until the stream goes quiet. The model sentence in terminal.md was corrected in place.

Leak sweep pasted by the lane: `ls /tmp/tmux-1000 | grep refutterm` and `ls /tmp | grep refutterm` both empty; live clients at that moment `/dev/pts/10 191x45 xterm-ghostty henosis` only, the phone client gone, the `tests` session still present and untouched.

## inventory.md (lane refute-inventory, 2026-09-04)

Graded at 6f2e27e. Private `-L refute-inventory-2063156-*` sockets under `env -u TMUX -u TMUX_PANE`, tmux 3.7c.

| # | Claim | Verdict | What ran | Output |
|---|---|---|---|---|
| 1 | bare SGR click fires MouseDown1Control1 | CONFIRMED | logging `bind -n MouseDown1<Key> run-shell -b` per key, bare `\e[<0;COL;30M`/`m` at every column; then unmodified conf-full at 60x30 (col 40) and 131x58 (col 100) | column 40 logged `Control1 x=39 y=0 range=control`; the branch picker opened at both sizes, frame identical to inventory.md's 40x19 render. Manual entry for `range=control|N` names no modifier. README.md:5,291 and KEYS.md:47-48 read as the file says and are wrong for 3.7 |
| 2a | no window tabs at 60x30, three windows | CONFIRMED | conf-full, three windows, rendered status row | `  tests  󰊢  research/mobile-97 ⑂research-mobil 󰃰  08:14`, identical. Note `research/mobile-97 ⑂research-mobil` is one value: scripts/tmux-where:159 prints branch then worktree name |
| 2b | column-to-key map at 60x30 | TIGHTENED | every column 1-60 pressed, three windows named bash; then window 3 renamed `bashXYZ` | 1-10 StatusLeft, 11-33 Status, 34-51 Control1, 52-60 StatusDefault. With the longer name 28-36 Status and 37-40 Control1, the file's boundary. Control1 always ends at 51; its left edge floats with the window-list width. The one-window table reproduced verbatim |
| 2c | tabs whole at 120, absent at 80, two windows | CONFIRMED, middle rows misdescribed | bisect at 80, 96, 104, 112, 120 | absent at 80, both whole at 120, `<a` at 96. At 104 and 112 the marker is `<` on the left and window 2 survives while window 1 is eaten, the reverse of the file's wording. status-right is cut from the left: `ockpit-wt` |
| 3 | popup interiors 40x19, 89x38, 52x19 | CONFIRMED | `stty size` inside each popup at both client sizes | 70%/70% 40x19 and 89x38; 90%/70% 52x19 and 115x38; 78%/40% 44x10 and 100x21; 60%/50% 34x13 and 76x27. The `-h 6` row was not re-run with `-w "$PW"`; with the default width it gave 28x4 and 63x4 |
| 4 | fleet queue loses diff-stat at 89 columns | TIGHTENED | the real scripts/tmux-review over a fake fleet (three `exec -a claude sleep 300` windows in scratch repos, @panestate set) in a real pane of 89x38, then with the owner's 37-character `research-mobile-97⑂research/mobile-97` loc field at 89, 95, 100, 110 | short fields: whole at 89. Owner's field: cut at 89 and 95, whole at 100. The caps at tmux-review:2220-2221 are ceilings; `pad_field` lets a longer field overflow. The threshold is a property of the fleet, not the popup |
| 5 | one status repaint/min, zero VPA and ECH idle | TIGHTENED, ECH part REFUTED | five idle minutes across both sizes and both configs, raw client bytes counted | bytes 441, 693, 101 reproduce exactly; one repaint and zero VPA per minute CONFIRMED. One ECH per idle minute wherever the status has a gap: `\e[26X` at 131x58 conf-full (n=2), `\e[17X` at 60x30 conf-example; none only at 60x30 conf-full where the line is full edge to edge. Caveat: examples/tmux.conf never sources catppuccin, so conf-example rows measure the stock tmux status bar |
| 6 | NEEDS YOU pushes `[z][x]` off at 60 columns | CONFIRMED verbatim | conf-example, two panes, 48-character question | 60: `─── NEEDS YOU Should I force-push the rebased branch to orig`, no buttons. 131: `…orig?x…───…───[z][x]` |
| 7 | tm `go()` is the only session-creating seam apart from cockpit.tmux:444 and full.tmux.conf:138 | TIGHTENED | grep for new-session and attach-session across scripts/, cockpit.tmux, examples/ | scripts/tm:83 and :88 are the only calls in scripts/; cockpit.tmux:444 is the other call site; full.tmux.conf:138 has no call, it shells out to ~/.local/bin/tm and goes through `go()`. Two call sites, not three |

Traps recorded by the lane: `set -g @thm_red #ff6b6b` unquoted is a comment, quote the palette; `display-popup -E` run synchronously from outside blocks the harness, bind it to a key and send the key bytes.

Leak sweep pasted: `ls /tmp/tmux-1000 | grep refute-inventory` empty; `/tmp/refute-inventory-2063156` kept as evidence; no `sleep 300` and no `tmux -L refute` process. Live server read-only; the `tests` session now shows without `(attached)`, the phone client having detached on its own.

Corrections applied to inventory.md in place, 2026-09-04.
