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
