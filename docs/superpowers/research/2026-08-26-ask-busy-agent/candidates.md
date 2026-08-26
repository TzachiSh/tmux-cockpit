# Every other candidate, spiked

Opus lane, 2026-08-27. The same screen as the fzf, Bubble Tea and Ratatui spikes (rows with glyph/id/window/title/doing/age, a `capture-pane -ep` preview of the selection, j/k, ↵ prints the pane id and exits, q, 1 s refresh), each fed by the Rust `--rows` binary (6 ms headless) so every spike is only the UI. Private tmux servers throughout; popup runs used a nested private server so a real client existed. Bytes/tick = what the app writes to its pty per second (`pipe-pane`, 10 s); execs/s from `strace -f -e execve`. Sources under the session scratch dir (`more/`), not committed.

## Table (3 agents, 160×40, app process only)

| | LOC | build/install | artifact | 1st frame | RSS | CPU/tick | bytes/tick | execs/s | in `display-popup -E` | j/k | preview |
|---|---|---|---|---|---|---|---|---|---|---|---|
| fzf `--listen` 0.74.3 | 11 | none | shared | 45 ms | 10.3 MB | 11 ms | 7.6 KB | 13.2 | yes; `--tmux` also returns stdout | via `--no-input --bind j:down,k:up` | free |
| Bubble Tea | 383 | 15 s | 4.2 MB static | – | 12 MB | 10–23 ms | – | – | yes | written | hand-rolled |
| Ratatui | 263 | 32 s, 187 crates | 0.64 MB | – | 3.1 MB | 7–8 ms | – | – | yes | written | ansi-to-tui |
| tview (Go, tcell) | 120 | 4.5 s + 2.3 s | 3.4 MB static | 42 ms | 10.1 MB | 8 ms | 7.5 KB | 5.1 | yes, clean | written | `TranslateANSI` |
| ncursesw (C) | 93 | 0.09 s gcc | 20 KB dynamic | 42 ms | 2.5 MB | <1 ms | **16 B** | 6.5 | if stdout is free | written | none: must write the translator |
| Textual (Python 8.2.8) | 58 | 11.4 s | 38 MB venv | 306 ms | 36 MB | 25 ms | 11 KB | 5.2 | only if stderr is free; sends 3 terminal queries | written | `Text.from_ansi` |
| Ink (Node 26, React) | 43 + bundler | 5.6 s + 1.8 s | 24–35 MB | 420 ms | 130–200 MB sawtooth | 26 ms | 8.1 KB | 6.3 | yes | written | passthrough |
| dialog 1.3 | 24 | installed | shared | 76 ms | ~0.1 MB | 1 ms | 8.9 KB | 5.5 | yes | no (letters are hotkeys) | impossible (no split layout) |
| whiptail | – | – | – | – | – | – | – | – | – | – | no `--timeout`: cannot refresh |
| bash + tput | 36 | none | 0 | 70 ms | 4.4 MB | 6 ms | 4.2 KB | 10.8 | if stdout is free | written | passthrough; `cut -c` counts escapes |
| `watch -t -n1` | 3 | none | 0 | – | 3.4 MB | 1 ms | – | – | wraps at 80 columns | none | static |
| `display-menu` rebuilt @1 Hz | – | none | 0 | – | 0 | 0 | – | – | yes | the loop can never read the cursor | none |
| bash board (3 live tiles) | 40 | none | 0 | – | 4.5 MB | 47 ms | 4.4 KB | 13.6 | yes | h/l | N tiles |

## Per candidate

- **tview**: the cheapest Go route; a third of Bubble Tea's lines and RSS because `QueueUpdateDraw` on a Table is the whole loop; `TranslateANSI` gave a preview first try. Loses filtering, multi-select and the wheel vs fzf for 120 lines and 10 MB.
- **ncursesw**: the efficiency outlier, 16 bytes/tick because ncurses diffs cells. Costs: the ✓/● glyphs came out as `�~\` until `setlocale(LC_ALL,"")` + `-lncursesw` + a wider byte budget (nothing warns); paints on stdout, so it cannot return a result from a popup on stdout; no ANSI preview until you write the translator. notcurses: not installed, maintenance mode.
- **Textual**: paints on stderr (a redirected stderr shows a blank popup); the **only spike that sends terminal queries from inside the popup** (`CSI ?2026$p`, `?2048$p`, `>25u`), the class behind tmux #4942 / crossterm #1104, and you cannot opt out because the framework sends them before your code runs; its own truecolor theme (the "different program" look).
- **Ink**: Node 26 cannot load `.jsx`, so esbuild is mandatory; 130–200 MB sawtooth heap (GC, not a leak); slowest first frame.
- **dialog**: `--timeout` is indistinguishable from Esc (both 255) unless `DIALOG_TIMEOUT` is exported; paints on stdout and returns on stderr; `j` selects an item tagged j; no split layout, so the preview has nowhere to go. Re-exec at 1 Hz showed 0 blank frames in 200 samples, so flicker is not the problem. **whiptail** has no `--timeout` at all (newt 0.52.24).
- **bash + tput**: `IFS= read -rsn1 -t 1` with rc > 128 is the whole loop; the one bug is `cut -c1-$cols` counting escape bytes (the same defect Diagon's generators have); fix with `capture-pane -p` or an escape-skipping awk.
- **`watch`**: no keys; inside a popup it wraps at 80 columns although the pty is 142 wide, `COLUMNS` does not fix it.
- **`display-menu` at 1 Hz**: re-issuing replaces the open menu and `-C` restores a highlight, but a menu reports only the chosen item, so the loop can never learn where the cursor moved; it resets forever.
- **bash board**: three live tiles with headers and h/l selection, 40 lines, works in a popup today. 47 ms CPU/tick vs 6 ms for the list, after replacing `$(tput cup)` per line (974 execs per 10 s) with `printf '\033[%d;%dH'`; the remaining cost is bash string slicing over tile area and scales with N × tile height.
- **fzf 0.74 `--tmux` vs `display-popup -E`**: `a=$(tmux display-popup -E 'echo X')` is empty (a popup's stdout is its own tty); `b=$(fzf --tmux …)` returns the selection like plain fzf, +25 ms, same geometry, and degrades to fullscreen fzf outside tmux with rc 0. Every result-file / tmux-option round-trip on a picker path is replaceable with a command substitution. `j` types into the filter unless `--no-input --bind 'j:down,k:up'`.

Popup output channels, since three spikes rendered blank on the first try for three different reasons: fzf and tview paint on the tty and return on stdout; ncurses and bash+tput paint on stdout and return nothing (use a file, an fd, or a tmux option); dialog paints on stdout and returns on stderr; Textual paints on stderr; ratatui paints on stdout (the demo returns through a `COCKPIT_ACTION` file).

## Survey only

tmuxwatch (Go, Bubble Tea, polls at 1 s, brew/go install, recommends its own session over a popup); workmux (Rust, cockpit's closest twin: a dashboard popup plus a per-window agent sidebar driven by hooks writing state files under `~/.local/state/workmux/agents/`, installers for Claude Code / Copilot / OpenCode / Antigravity; brew, cargo, prebuilt, Nix); tmux.expose (Mission-Control switcher with live previews); muxplex (browser dashboard); sesh `--watch`. ttyd/browser: a second daemon, a port and auth, a different product. Charm wish (SSH-serve a Bubble Tea app) and crush (an agent, not a library): irrelevant. None of the shipped live tmux dashboards is a tpm plugin, and the two closest to cockpit recommend a dedicated session rather than a popup.

## Verdict

Nothing displaces fzf for the list: every candidate is more code and more memory for a screen the human cannot tell apart, and two (Textual, dialog/whiptail) fail on behaviour, not cost. If cockpit ever compiles this screen, Rust stands. Two cheap things do change: prefer `fzf --tmux` over `display-popup -E` wherever the caller needs the answer; and the board is drawable in bash today, so the compiled trigger is "a board that must stay cheap at N agents" or mouse/per-cell styling, not "a board".
