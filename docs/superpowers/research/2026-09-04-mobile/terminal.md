# terminal-mechanism: which sequence the phone mishandles

Lane terminal-mechanism, issue #97, 2026-09-04. Read-only research; probes on private `-L` sockets, tmux 3.7c.

## Question

Which sequence in tmux's output did the phone terminal mishandle, and what tmux setting avoids it?

The sequence was the write into the bottom-right cell of the status line. tmux drove the cursor to the last row with `\e[100d` and then wrote a full-width status, ending at column 131 of a 131-column screen. That last write is safe only on a terminal that defers the wrap (the `xenl` pending-wrap state). On a terminal that wraps eagerly, writing that cell on the last row scrolls the whole screen by one line. Every status repaint then left the previous status one row higher, and every later absolute write landed one row below the content it belonged to.

The tmux-side setting is `set -as terminal-overrides ',xterm-256color:am@'`. It makes tmux skip that cell (`tty.c:2257`). It is scoped to the client's TERM, so it hits the phone and not the ghostty client.

## Sequences

Every distinct control sequence in the owner's 6,242-byte capture (`artifacts/tmux-stream-131x100.bin`), decoded with a CSI/OSC/DCS tokenizer. All CONFIRMED.

| Sequence | Count | Role |
|---|---|---|
| `\r` then `\n` | 299 each | full-redraw walk, three walks of 98, 98, 95 `\r\n\e[K` |
| `\e[K` | 294 | erase to end of line during the walk |
| `\e[Nm` SGR | 142 | truecolor status theme |
| `\e[NH` CUP | 13 | absolute addressing, includes `\e[1;1H`, `\e[2;1H`, `\e[4;46H` |
| `\e[?12l` `\e[?25h` | 8 each | cursor style, cursor show |
| `\e[?1000l/h` `\e[?1002l/h` `\e[?1003l` `\e[?1006l/h` | 7/3 each | mouse mode teardown and setup |
| `\e[NX` ECH | 4 | `\e[92X` and `\e[66X`, erases the status gap |
| `\e[NC` CUF | 4 | `\e[92C` and `\e[66C`, skips the gap |
| `\e[1;100r` DECSTBM | 3 | scroll region for the full 100 rows |
| `\e[?1049h` | 1 | alternate screen |
| `\e[22;0;0t` | 1 | push window title |
| `\e[18t` `\e[14t` | 1 each | size in characters, size in pixels |
| `\e[100d` VPA | 1 | status-only repaint, row 100 |
| `\e[?2031h` `\e[?996n` | 1 each | color-scheme change notification and query |
| `\e[c` `\e[>c` `\e[>q` `\e]10;?` `\e]11;?` | 1 each | DA1, DA2, XTVERSION, fg and bg color queries |
| `\e[?2004h` `\e[?1004h` `\e[?7727h` `\e[>4;2m` | 1 each | bracketed paste, focus events, modifyOtherKeys |

Measured column positions, CONFIRMED by replaying the bytes through a cursor model:

| Stream | Status row ends at column |
|---|---|
| owner's capture, 131 cols | 131 |
| probe, 131x100, same config | 131 |
| probe, 131x58, same config | 131 |
| probe, 131x100, `am@` override | 130 |
| probe, `status off`, screen filled to the bottom row | 131 (corrected 2026-09-04, refute pass) |
| probe, `status off`, idle shell | no write to the last row at all |

Column 131 means the cell at index 130, the bottom-right cell, was written. The character written there was a space.

## Hypotheses

The decisive run used tmux at 131x58 and a 131x58 terminal model, so tmux and the terminal agreed on the size and the row mismatch was removed as a confound. Six real status repaints were captured from tmux at `status-interval 1`, not spliced by hand.

| position | wrap | scrolls over 6 repaints | status rows left on screen |
|---|---|---|---|
| bottom | deferred | 0 | 1 |
| bottom | EAGER | 6 | 9 |
| top | deferred | 0 | 2 |
| top | EAGER | 0 | 2 |

The 2 rows in the top cases were identical across both wrap modes, so they were not wrap-induced.

Two settings avoid it. `set -as terminal-overrides ',xterm-256color:am@'` is the root-cause fix and is scoped to the phone's TERM. `set -g status-position top` also drives the scrolls to zero, by moving the full-width write off the last row.

| # | Hypothesis | For | Against | Verdict |
|---|---|---|---|---|
| 1 | Bottom-right cell wrap: the terminal wraps eagerly instead of deferring | At a size both sides agreed on, eager wrap alone gave 6 scrolls and 9 stacked status rows; deferred gave 0 and 1. `tty.c:2257` skips that cell only under `TERM_NOAM`, and xterm-256color has `am` | No Termius source names eager wrap | CONFIRMED sufficient, and sufficient alone |
| 2 | Reported rows exceeded rendered rows | Client reported 100, screenshot showed about 58. Row pitch 19.8 px at 1080x2400; the keyboard-free area is about 1,970 px, near 100 rows. Consistent with Termius not resizing the pty when the keyboard opened | Not needed for the ghosts: hypothesis 1 reproduced them at a matched size. Deferred wrap on a short screen gave 0 scrolls per repaint | PLAUSIBLE, second and separate defect |
| 3 | The `\r\n` walk writes the bottom status with no absolute address, so a short screen ghosts on every redraw (lane phone-practice) | Their byte reading is correct. In the full redraw the bottom status follows a bare `\r\n` while the top status gets `\e[H`. The top-position probe contained 5 `\e[H` and zero VPA | The walk carries `\e[K` on every step, so it erases as it descends and wipes the previous status rather than stacking copies. The mechanism also requires the size lie, and the 131x58 run produced 9 ghosts with no size lie at all. Steady-state repaints do use an absolute address: `\e[58;1H` at 58 rows, `\e[100d` at 100 | Reading CONFIRMED, mechanism RULED OUT |
| 4 | VPA `\e[Nd` unsupported | tmux emitted `\e[100d` for the status repaint | An ignored VPA would leave the status at the pane cursor, mid-screen. Both screenshots show it at the bottom | RULED OUT |
| 5 | ECH `\e[NX` unsupported | 4 ECH in the capture, `\e[92X` and `\e[66X` | An ignored ECH leaves stale glyphs in the status gap. The gap was clean in both screenshots | RULED OUT |
| 6 | Scroll region `\e[1;100r` mishandled | Emitted 3 times, bottom exceeds the visible rows | Out-of-range DECSTBM is clamped or ignored by every common terminal. Neither outcome stacks status rows | Unlikely |
| 7 | OSC/DCS queries echoed as text | 5 unanswered-query candidates: `\e[?996n`, `\e]10;?`, `\e]11;?`, `\e[18t`, `\e[14t` | Neither screenshot showed stray `rgb:` or `;t` text | RULED OUT |

## status-position top

It survives, and it earns a place in the recommendation. Under eager wrap it took the scrolls from 6 to 0.

The reason is not the absolute address phone-practice proposed, because the bottom repaints also use one (`\e[58;1H`). It is that the full-width write moves to row 0, where a wrap steps to row 1 instead of scrolling. That also makes it narrower than the `am@` fix: with the status at the top, tmux's last row becomes a pane row, and a full-width pane line still hits the bottom-right cell.

## Why the bottom status had no absolute address, in tty.c terms

It is the `tty_cursor` shortcut rather than anything in the redraw loop.

`tty_cursor` at `tty.c:2487` takes a "Zero on the next line" branch when the target is column 0 of the row below and that row is not `tty->rlower`, emitting `\r` then `\n` instead of CUP. During the redraw walk the cursor is already at row 98 when status row 99 comes up, so the shortcut fires and the status is written wherever the `\r\n` lands.

At the top position the target is (0,0), which hits the HOME branch at `tty.c:2482` and emits `\e[H`.

This applies only to the full redraw. In the steady state the cursor sits inside the pane, the shortcut does not apply, and tmux emits an absolute address: `\e[58;1H` in the 58-row capture and `\e[100d` VPA in the owner's 100-row one, because `tty.c:2570` prefers VPA when the row change exceeds the target row.

## How am@ reaches the skip

Four hops, each read in source:

1. `set -as terminal-overrides ',xterm-256color:am@'` removes the `am` capability for clients whose TERM is `xterm-256color`.
2. `am` is a real overridable capability: `tty-term.c:61` declares `[TTYC_AM] = { TTYCODE_FLAG, "am" }`.
3. `tty-term.c:511` sets `term->flags |= TERM_NOAM` when `tty_term_flag(term, TTYC_AM)` is false.
4. `tty.c:2257` in `tty_cell` returns early when `TERM_NOAM` is set and `tty->cy == tty->sy - 1` and `tty->cx == tty->sx - 1`.

`tty.c:703` then emits an explicit CUP after any wrap under `TERM_NOAM`, which makes the override safe rather than merely quieter. `tty_cell` is the single choke point every last-row write routes through, status and pane alike, so one override covers both.

## Sources, tmux

Shallow clone at `~/.cache/tmux-src`, commit `db24c029`. All CONFIRMED by reading.

| Location | Establishes |
|---|---|
| `tty.c:2256` | the comment "Skip last character if terminal is stupid" |
| `tty.c:2257` | the skip is gated on `TERM_NOAM` and `tty->cy == tty->sy - 1` and `tty->cx == tty->sx - 1`. The single choke point every last-row write routes through |
| `tty-term.c:61` | `[TTYC_AM] = { TTYCODE_FLAG, "am" }`, so `am` is a real capability and is overridable |
| `tty-term.c:496` | "Terminals without am (auto right margin) wrap at at $COLUMNS - 1" |
| `tty-term.c:499` | "Terminals without xenl (eat newline glitch) ignore a newline beyond the right edge of the terminal, but tmux doesn't care about this" |
| `tty-term.c:511` | `term->flags |= TERM_NOAM` set only when `am` is absent. xterm-256color has `am`, so `TERM_NOAM` was clear and tmux wrote the cell |
| `tty.c:677` | `tty_putc` drops a printable on the last row and last column only under `TERM_NOAM` |
| `tty.c:713` | `tty_putn` truncates a run on the last row only under `TERM_NOAM` |
| `tty.c:703` | under `TERM_NOAM` tmux emits an explicit CUP after a wrap, which is why the override is safe |
| `tty.c:2482` | HOME branch, source of the top status's `\e[H` |
| `tty.c:2487` | "Zero on the next line" comment; the condition at 2488-2489 carries a third clause, `(!tty_use_margin(tty) || tty->rleft == 0)`. Source of the bottom status's bare `\r\n` |
| `tty.c:2570` | closing of the comment; the condition at 2571-2573 prefers VPA when the row change exceeds the target row or the move would cross the scroll region |
| `tty.c:1231` | ECH used for a mid-line erase when terminfo has `ech` |
| `tty.c:2110`, `tty.c:2431` | under `TERM_NOAM` tmux stops relying on the terminal to wrap a full-width pane line and redraws or re-addresses instead. Measured cost of the override on a wrapping stream: 35,192 bytes against 34,324, about +2.5%, identical output (refute pass, 2026-09-04) |
| `cmd-refresh-client.c:269` | `refresh-client -C` jumps to `not_control_client` unless `CLIENT_CONTROL` is set; `:285` emits "not a control client". The server cannot correct a normal client's size |
| `cmd-refresh-client.c:276` | `-S` sets `CLIENT_STATUSFORCE` and calls `server_status_client`, a status-only redraw |
| `resize.c:161`, `resize.c:184` | `window-size latest` applies only when several clients share a window. The phone and the desktop are on different sessions, so it never fired |

## Sources, web

- https://github.com/kovidgoyal/kitty/issues/4310, the same symptom on another terminal: "the status window is scrolled up and then the new bar is drawn on the line above the terminals last line", "The very last line of the terminal display is left blank and never contains any content", "Lines that scroll off the top of the screen on redraws are lost", "The cursor remains where it was originally being drawn". No diagnosis was posted.
- https://docs.termius.com/changelog/android, 7.2.2 (2025-08-07): "Fixed incorrect TTY behavior with tmux." 7.5.5 (2026-05-28): "Fixed the issues with screen scrolling in TUI apps. Fixed an issue where opening a terminal editor could show content overlapping the previous shell output." This class of bug was live in Termius and patched at least twice.
- https://github.com/erikw/tmux-powerline/issues/351: "the screen start scrolling randomly" and "the status bar end up duplicated", tmux 3.2a under WSL2. No diagnosis.
- https://github.com/tmux/tmux/issues/1040: duplicated lines that persist until an app repaints, on Alacritty, Kitty and Terminology but not xterm, urxvt or st. No maintainer reply.
- https://github.com/microsoft/WSL/issues/2251 "Wrong line wrapping for applications with statuslines (vim, tmux, etc.)", same family, no diagnosis.
- No authoritative source names Termius Android's rendering engine. Treat it as unknown.

## How the VT model was built

Replay the owner's real bytes twice, changing exactly one thing: whether the terminal defers the wrap.

About 60 lines of Python, one class: a grid of `cols` by `rows`, a cursor, a `pend` flag, a scroll counter. It handles only what tmux emitted: CUP `H`, VPA `d`, CUF `C`, ECH `X`, EL `K`, ED `J`, DECSTBM `r`, CR, LF, printables. Private-parameter sequences, OSC and DCS strings, and the two-character charset designators `\e(B` and friends are consumed and ignored; none of them move the cursor. The designators matter: tmux emits one after every status paint, and a model that consumed only two of their three bytes would print the trailing `B`, which lands exactly on the pending-wrap cell and fabricates a scroll (refute pass, 2026-09-04).

```python
def put(s, ch):
    if s.pend: s.x = 0; s.nl(); s.pend = False
    s.g[s.y][s.x] = ch
    if s.x == s.c - 1:
        if s.deferred: s.pend = True      # xterm, ghostty, xterm.js
        else: s.x = 0; s.nl()             # eager wrap, the phone
    else: s.x += 1
```

`nl()` scrolls when `s.y == s.bot`, incrementing the counter. Skipped: SGR tracking, scrollback, wide characters, reflow. None affect whether a scroll happens.

## Exact commands

Capture, at a size tmux and the model agree on:

```sh
# conf-bottom / conf-top = owner's config plus:
#   set -g status-position bottom|top
#   set -g status-interval 1
#   set -g status-right "#(date +%T)"
python3 q.py qbot conf-bottom     # 131x58 pty, 3s attach then 6s of repaints
python3 q.py qtop  conf-top
```

`q.py` forks a pty at 131x58 with `TERM=xterm-256color`, execs `tmux -L <sock> -f <conf> new-session -s tests`, writes the attach phase to `<sock>-attach.bin`, then six seconds of steady-state output to `<sock>-repaints.bin`, then kills the server.

```python
for pos, tag in (("bottom","qbot"), ("top","qtop")):
    for d in (True, False):
        t = Term(131, 58, d).feed(open(tag+'-attach.bin').read())
        base = t.scrolls
        t.feed(open(tag+'-repaints.bin').read())
        rows = [n for n,r in enumerate(t.g) if 'bash' in ''.join(r)]
        print(pos, 'deferred' if d else 'EAGER', t.scrolls-base, len(rows), rows)
```

```
bottom    deferred  0        1        [57]
bottom    EAGER     6        9        [48, 49, 50, 51, 52, 53, 54, 55, 56]
top       deferred  0        2        [2, 55]
top       EAGER     0        2        [2, 55]
```

Column measurements used a separate 20-line cursor tracker returning the ending column of the status write; the table is under Sequences. The glyph in the bottom-right cell was a space, which is why the fix costs nothing visible.

## Experiment for the owner

Probe one needs no tmux and separates hypothesis 1 from 2 by itself. Run it in the phone session.

```sh
cat > /tmp/phoneprobe.sh <<'SH'
#!/bin/sh
R=$(tput lines); C=$(tput cols)
printf '\033[2J\033[H'
i=1; while [ $i -le $R ]; do printf 'row %03d of %s\n' "$i" "$R"; i=$((i+1)); done
printf '\033[1;1H>>> ROW 1 <<<'
printf '\033[%d;1H' "$R"
printf '%*s' "$C" '' | tr ' ' '#'
printf '\033[3;1H'
SH
sh /tmp/phoneprobe.sh
```

| Observation | Reading |
|---|---|
| `#` row sits on the last line, one screen of scrollback reaches `row 001` | deferred wrap, hypothesis 1 wrong |
| Everything shifted up one and a blank line appeared under the `#` row | eager wrap, hypothesis 1 CONFIRMED |
| Visible rows far fewer than the reported `of NNN` | hypothesis 2 CONFIRMED, read independently of the first line |

The script emits `\e[58;1H` followed by exactly 131 `#` under a 131x58 pty, so it does write the bottom-right cell. CONFIRMED.

Probe two, on a private socket, covers all three levers:

```sh
tmux -L probe -f /dev/null new-session -d -s p
tmux -L probe set -g status-interval 1
tmux -L probe set -g status-right '#(date +%T)'
tmux -L probe attach -t p        # watch one minute, expect ghosts
# detach, then each fix on its own:
tmux -L probe set -as terminal-overrides ',xterm-256color:am@'
tmux -L probe attach -t p
# detach, undo, then:
tmux -L probe set -g status-position top
tmux -L probe attach -t p
tmux -L probe kill-server
```

Ghosts before and none after either change confirms hypothesis 1 on the device.

## Recommendation

```
set -as terminal-overrides ',xterm-256color:am@'
```

| Property | Value |
|---|---|
| Status write | ends at column 130 instead of 131, CONFIRMED |
| Lost glyph | one space, the theme's trailing pad, CONFIRMED |
| Scope | matched on client TERM. With `xterm-256color` the widest row write stopped at 130, with `xterm-ghostty` it reached 131, CONFIRMED by running the same config under both |
| Desktop client | unaffected, it reports `xterm-ghostty` |
| Coverage | every last-row write, status and pane alike, because `tty.c:2257` is the one choke point. With the status at the bottom the skipped cell is the status pad, a space. With `status off` or `status-position top` the last row is a pane row and a real glyph is dropped: a filled screen ends the last row at column 131 with `#` present, at 130 with the cell blank under the override. Copy mode inherits the same skip (refute pass, 2026-09-04) |

The scope row is a per-client lever: the two clients already differ in TERM, so a TERM-keyed override discriminates them without a per-client option.

`set -g status-position top` is a real second option. Under eager wrap it took the scrolls from 6 to 0. It is narrower: with the status at the top, tmux's last row becomes a pane row, and a full-width pane line still hits the bottom-right cell. Ship `am@`, keep `status-position top` as a diagnostic and a fallback, but not both at once: with the status at the top the last row becomes a pane row, and `am@` then drops a visible character from it instead of a status pad (refute pass, 2026-09-04).

| Alternative | Why not |
|---|---|
| `status off` | removes the per-minute repaint, not the wrap. Once pane output reaches the bottom row a full-width pane line still writes column 131, measured with the glyph `#` present; on an idle shell nothing is written to the last row, so the alternative only looks safe while the screen is empty (refute pass, 2026-09-04) |
| shorter `status-right-length` | tmux pads the status to full width. Every probe ended at the screen width |
| `window-size` or `aggressive-resize` changes | apply only when clients share a window. These are separate sessions |
| `refresh-client -C` | "not a control client" for a normal SSH client, `cmd-refresh-client.c:269` |

Hypothesis 2 stays open and needs its own fix. Since the server cannot correct the client's size, the lever is `set -g window-size manual` plus `tmux resize-window -t tests -y <visible rows>`, which sizes the window independently of the client and letterboxes the rest. A workaround for a peer-app bug; separate issue.

## Tools

Harness tools: Bash, Read (two screenshots, `pty-probe.py`), WebSearch (4), WebFetch (4), ToolSearch, SendMessage. No skills. No repo file edited.

| Shell step | Reason |
|---|---|
| `gh issue view 97` | read the brief |
| `git clone --depth 1 https://github.com/tmux/tmux ~/.cache/tmux-src` | read tty.c at source |
| python tokenizer over the capture | count every distinct sequence |
| python cursor tracker | measure the status row's end column |
| `grep`/`sed` over `tty.c`, `tty-term.c`, `resize.c`, `cmd-refresh-client.c` | cite decision points |
| probes at 131x100, 131x58, mid-session resize, `am@`, `status off`, `status-position top`, `status-interval 1`, two TERM values | reproduce and price each lever |
| 60-line VT model, deferred and eager | replay real bytes, count scrolls and ghosts |
| `gh api repos/kovidgoyal/kitty/issues/4310/comments` | read comments WebFetch could not see |
| `tmux list-clients` | read-only check of the live server, three times, unchanged |

## Leak sweep

```
$ for s in mechA mechB mechC mechD mechE t256 tgh u256 ugh top qbot qtop; do tmux -L $s ls; done
error connecting to /tmp/tmux-1000/<each> (No such file or directory)   # all twelve
$ ls -d /tmp/tm-mech* /tmp/tm4.txt
ls: cannot access: No such file or directory
$ tmux list-clients -F '#{client_name} #{client_width}x#{client_height} #{client_termname} #{client_session}'
/dev/pts/10 191x45 xterm-ghostty henosis
/dev/pts/11 131x100 xterm-256color tests
```

`/tmp/mobile-probe-1938635` is the orchestrator's capture directory, left in place. `riv*` sockets under `/tmp/tmux-1000/` belonged to a peer lane and were left alone.

## Did / Did not / Could do next

Did: decoded the capture; measured the status write reaching the bottom-right cell; traced it to `tty.c:2257`; reproduced at 131x100, 131x58, across a mid-session resize, with `status off`, `am@` and `status-position top`; built a VT model and at a matched size showed eager wrap alone produces 6 scrolls and 9 stacked rows while deferred produces 0 and 1; confirmed phone-practice's byte reading, ruled out their mechanism, kept their `status-position top` finding with a corrected explanation; wrote and verified a device probe that needs no tmux.

Did not: touch the phone, so at report time hypothesis 1 was confirmed as a sufficient mechanism, not as the phone's observed behaviour. Later the same day the owner applied the override on the live server and the ghost rows stopped on the phone (README, Cause). Identify Termius's rendering engine. Confirm whether Termius answers `\e[18t`, so the 100-row figure is inferred from the pty winsize and pixel arithmetic. Test the `resize-window` mitigation on a device.

Could do next: run the two probes on the phone, about two minutes, closing both hypotheses. If hypothesis 1 confirms, file the `am@` line as an implementation issue and the row mismatch separately. Check the phone's Termius version against 7.2.2 and 7.5.5 from the changelog before treating it as unfixable upstream.
