# The competition: bash + fzf vs Rust/Ratatui vs C/ncursesw, one harness

Opus lane, 2026-08-27. The three fleet demos measured by one method on one private `-L bench` server (4 fake agents, `@cockpit_session` set on every pane so `--rows` never forks `claude agents --json`), 10 samples each, plus screenshots of all three on the live fleet at 120×30 (read-only). Harness in [spike/bench/](spike/bench/); demos in [spike/cockpit-tui-demo/](spike/cockpit-tui-demo/) and [spike/c-demo/](spike/c-demo/). The C numbers come from a frozen copy of `fleet.c` compiled by the lane itself (the file was being edited during the run; md5 verified before and after each benchmark).

## Table

| | bash + fzf 0.74 | Rust / ratatui | C / ncursesw |
|---|---|---|---|
| LOC (excl. tests) | 15 (launcher; borrows the Rust `--rows`; a real bash row producer is ~75–105) | 1,756 (9 files) | 338 (1 file) |
| build | none (fzf prebuilt) | `cargo build --release`, 23.5 s cold (warm registry) | `gcc -O2 -Wall -Wextra`, 0.4–0.65 s |
| binary | 4.7 MB (fzf, static) | 785 KB, libc + libgcc_s | 27 KB (22 KB stripped), libncursesw + libtinfo + libc |
| idle RSS | 10.8 MB | 3.1 MB | 3.2 MB |
| idle CPU, 20 s | 0.5 % | 0.1 % | 0.0 % |
| tick cost, median | 14 ms (rows pipeline) | 13.4 ms (n=30, logged) | 17.7 ms (n=29, logged) |
| bytes/tick idle | 2,450 B/s (unconditional redraw) | 0 | 0 |
| push SIGUSR1, median | n/a (no signal path) | 31 ms (30–42) | 31 ms (29–40) |
| push SIGWINCH, median | n/a | 30 ms (29–42) | 31 ms (30–33) |
| poll, median | 705 ms (67–710) | 712 ms (72–718) | 715 ms (83–727) |
| in `display-popup -E` | yes | yes | yes |
| preview | `capture-pane` per selection change, tail of the pane | in-process `capture-pane`, head of the pane, bordered | in-process `capture-pane`, head of the pane, rule |
| filter / multi-select | both free | none | none |
| distribution under tpm | tpm clones the script; fzf is already a repo prerequisite | prebuilt per-platform binaries + a fetch on load, or cargo on the box; macOS unsigned | same, plus the libncursesw/libtinfo ABI must match on every target (musl/alpine, macOS's own ncurses) |

Screenshots: [fzf](spike/shots/fzf.png), [rust](spike/shots/rust.png), [c](spike/shots/c.png) (rendered from `capture-pane -ep` through the ANSI→HTML converter and headless Chrome).

## Anomalies, all explained

1. The C lane's first build had a dead SIGWINCH (a bare `continue` swallowed ncurses' EINTR): 1,031 ms median, the poll floor. The lane's KEY_RESIZE fix brought it to 31 ms; the table is from the fixed source.
2. Rust and C both write zero bytes on an unchanged tick (a real diff, verified: the same Rust process wrote 1,824 bytes for six genuine flips). fzf's `reload` redraws unconditionally, 2,450 B/s forever.
3. fzf's preview looked blank on the bench fixture because the fake panes are 50 rows with 4 lines at the top and the preview is `tail -n $FZF_PREVIEW_LINES`; on the live server it is full. Rust and C show the head.
4. The three poll medians (705/712/715 ms) are within 10 ms and none is ~500 ms: the harness sleeps 0.3 s between samples, so the phase over the 1 s tick is not uniform. Read the row as "poll latency is the tick interval, identical by construction".
5. Both push medians sit at the harness floor (`set-option` + a `capture-pane` fork every 5 ms + the app's own 13–18 ms tick). Rust and C are indistinguishable on push; the measurable difference in the whole exercise is 30 ms vs 710 ms, push vs poll, not Rust vs C. (The C lane's own finer harness put C at 15.7 ms with 13.4 of it in the shared `--rows` subprocess and the paint at 0.14 ms.)
6. **A write reached the live server.** The lane's first probe, `cockpit-tui fleet --help` with no `COCKPIT_TMUX`, defaulted to the live socket, published `@cockpit_tui_pid` (now a dead pid) and panicked before Drop could unset it. Inert (nothing reads it), left in place because removing it is a write; the owner's call: `tmux set-option -gu @cockpit_tui_pid`.
7. The Rust demo has no `COCKPIT_READONLY` guard; `Push::start()` runs `set-option -g` on whatever `COCKPIT_TMUX` points at, so screenshotting the live fleet mutates it. The C demo honours `COCKPIT_READONLY` (`fleet.c:262`). The live Rust screenshot was routed through a write-swallowing tmux wrapper. If Rust ships, it needs the guard.

## Verdict

The measurements do not support replacing fzf for the fleet screen, and they barely distinguish Rust from C. On the axis a compiled board was meant to win, push latency, Rust and C tie at 31 ms on the harness floor. The 23× win is push vs poll (30 ms vs 710 ms), a signalling decision, not a language one: a bash + fzf fleet can be woken by the agent-state hook POSTing `reload()` to fzf's listener instead of a 1 s timer, which collapses fzf's only losing row. What the compiled boards win is idle cost, 3 MB and 0 bytes/tick against 10.8 MB and 2,450 B/s, which matters for a screen left open all day; a battery argument, not a responsiveness one. Against that, fzf gives fuzzy filter and multi-select for free, and 1,756 lines of Rust or 338 of C currently buy j/k/enter and nothing else. Under the contract, list + preview = fzf.

For the non-list surfaces the lane calls C the better buy at this scale (338 lines, 27 KB, a 0.4 s build vs 1,756 lines, 785 KB, 23.5 s), with the runtime a wash, and the distribution tax the real price for either: per-platform artefacts or a fetch-on-load with checksums, offline and proxy failure paths, none of which cockpit pays today. C adds one tax Rust does not, the libncursesw/libtinfo ABI on every target, and the earlier lanes add two more the table does not show: ncurses cannot emit truecolour on the owner's RGB terminal (`38;2;80;200;120` quantises to `38;5;72`) and ~18 % of the C file is hand-rolled unicode column arithmetic that Rust's `unicode-width` gives for free. Net: fzf for the fleet; a compiled binary only when a surface is not a list; price the distribution work first, because it is larger than either implementation; and between the two, Rust unless the binary size itself is the constraint.
