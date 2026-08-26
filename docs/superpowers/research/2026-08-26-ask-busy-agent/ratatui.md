# cockpit-tui in Ratatui: architecture, patterns, and a working demo

Three opus lanes, 2026-08-27, after the owner asked to go deep on Ratatui: an architecture with every claim re-measured on this box, a source read of eight Ratatui apps for the patterns to copy, and a demo built, deployed on a private server and tested. The demo crate is in [spike/cockpit-tui-demo/](spike/cockpit-tui-demo/) (2,019 lines, 12 tests, 7 goldens). The candidates that were not Ratatui are in [candidates.md](candidates.md).

## Corrections to the earlier research

1. ratatui 0.30.2's MSRV is **1.88.0** (verified in its Cargo.toml), not 1.86. Debian 13 stable ships rustc 1.85, Ubuntu 24.04 ships 1.75, so a `cargo build` fallback fails on stock distro toolchains unless rustup is installed. The fetch script must check `rustc --version` first and say "needs Rust ≥ 1.88, install rustup" instead of dumping a compiler error.
2. **Control mode is not push.** `refresh-client -B` subscriptions report "at most once a second" (man page), measured: one write → notification after 0.54 s; five writes 30 ms apart → one notification carrying only the last value. `%output` is raw pane bytes, so using it for previews means writing a terminal emulator. Drop control mode from the justification list; a 1 Hz `list-panes` has the same freshness.
3. **`claude agents --json` costs 380 ms** (three runs, 0.37–0.38 s). It is the fallback for panes with no `@cockpit_session`; in the first spike it ran every tick, making a tick 0.40–0.47 s instead of 0.00. Cache it (the demo: once per 60 s). The single most important implementation constraint.
4. fzf's latest is 0.74.3, not 0.74.4.
5. `panic = "abort"` is fine with `ratatui::init()` (its panic hook runs before the abort and restores the terminal) but wrong for a Drop-based guard (no unwinding, Drop never runs). The demo uses init + abort; the source-pattern lane's sidebar uses a Drop guard without abort. Pick one, not both halves of the wrong pair.
6. cargo-dist is active (0.32.0, 2026-05-22), but `aarch64-unknown-linux-musl` needs hand-wired cross linkers (#1581); ship three targets (x86_64 musl, x86_64 and aarch64 darwin) and add arm64 Linux when asked. cargo-dist cannot build darwin from Linux; it uses macOS runners, which are free.

## What the binary would own

| screen | shape | verdict |
|---|---|---|
| `prefix r` fleet | rows + one preview | stays fzf: filter, multi-select, wheel, `--listen` for 8 lines; the demo's Table version has no filter input at all, which is the biggest gap |
| **the board** | N tiles, each a live `capture-pane` preview, 3/2/1 columns | **the binary, and only this**: fzf has one preview window |
| `prefix i` info | printf + `read` | stays bash |
| `prefix f` side fix | two `read -p` | stays bash |
| `prefix ?` ask | prompt, spinner, answer | stays bash (8-line spinner in screens.md) |

The candidates lane then showed the board is drawable in 40 lines of bash at 47 ms CPU/tick (vs 6 ms for a list), scaling with tile area. So the trigger is "a board that must stay cheap at N agents", plus what no shell reaches: per-cell styling on a scrolled table, drag, mouse on cells. Minimum first release if built: the board, one binding, additive (missing binary → "board needs cockpit-tui; `prefix r` for the list"). Later: inline reply to a blocked agent through `tmux-review --deliver`, then drag-to-move-window (needs mouse capture, which takes wheel and selection away inside the popup). Never: the fleet list as a Table (fzf does it better for free).

## Data model and sources (measured)

- One `list-panes -a -F` per tick, the format from `tmux-review:1706-1723` verbatim with its two rules: sentinel every optional field (`#{?#{@panestate},#{@panestate},-}`, the `\t\t` collapse bug at :1708-1716), and one `-F` argument. 0.00 s for 3 panes; 6,000 calls on 20 panes left server RSS flat. No `#{q:}`.
- Session id: `@cockpit_session`, else the cached `claude agents --json` + one `ps -eo pid=,ppid=` BFS (the sidebar's ~70-line pattern, no per-pid `ps`/`lsof`).
- Transcript tails: last 64 KB, whole lines; skip the parse when `fs::metadata().modified()` is unchanged. No `notify`: one inotify watch per file and directory against `max_user_watches`, a thread, for a screen that redraws once a second.
- Previews: `capture-pane -ep` per visible tile; 30,000 calls against 10 scrolling panes left server RSS flat (3,860 → 3,964 KB), ~4.5 ms each. Stagger unfocused tiles if the count grows past ~8.
- Actions stay shell scripts: `tmux-jump`, `tmux-review --deliver` (already `run-shell -b` because a popup kills its children), `tmux-fixer`, `kill_window`. The binary is a renderer with a keymap.

## Event loop and terminal (patterns from the source read)

Copy the sidebar's loop (tmux-agent-sidebar app.rs:50-127, 77 lines): a `needs_redraw` flag consumed at the top; `poll(min(deadlines, 16 ms))`; drain the whole input burst with `while poll(0)` before one frame; `handle_event` returns a bool that re-arms the flag; refresh every 1 s or on SIGUSR1 (libc `sigaction` with `SA_RESTART`, handler stores an `AtomicBool`, the app publishes its pid in a pane option, a tmux hook `kill -USR1`s it; the sidebar's hook-dedup awk so `set-hook -ga` does not stack on reload). Hash the rows + previews and skip `terminal.draw()` on an unchanged tick (the demo: 17 consecutive no-redraws on a static server). No tokio: one input source, one timer, 4 forks per tick; recon declares tokio and never uses it. No resize handling: `Terminal::draw` autoresizes, popups have resized since 3.2. Escape delay is tmux's `escape-time`, same as fzf. **No mouse in phase 1**: enabling SGR capture inside a popup takes the wheel and text selection away. Kitty keyboard is absent under tmux (#4196); nobody among eight apps depends on it. **Send no terminal queries**: tmux itself emits `CSI ? 2031 h` and `CSI ? 996 n` to the outer terminal during a popup, the family behind crossterm #1104 (open, 2026-08: an unrecognised `CSI ? … $y` wedges the parser and swallows all later input) and tmux #4942; pin `crossterm = "=0.29.0"`, keep `q` as an exit that does not depend on Escape parsing. Terminal restore: `ratatui::init()` (hook runs before abort) or a 26-line RAII guard (sidebar main.rs:14-39) without `panic = "abort"`.

Project shape from the source read: one crate, flat modules (the `event-driven` template is 272 lines: main 13 / app 95 / event 125 / ui 39; recon is 4.8k flat; nobody below 15k LOC uses a workspace). Deps like the sidebar: ratatui, crossterm, unicode-width, serde_json, `libc` for the signal, nothing else (no clap, no anyhow, no color-eyre, no tokio). `ListState`/`TableState` + `render_stateful_widget`, `Layout::areas()`. gitui's grapheme-aware truncation (string_utils.rs:27-38) for any cell that can hold a branch name or emoji. Release profile: `strip = true, lto = true` (+ `codegen-units = 1, opt-level = "s"` per the ratatui recipe). Tests: TestBackend + golden strings (the demo's 10-line helper; insta only when the count outgrows eyeballing). The sidebar's 1,272 tests with mockable tmux calls (`#[cfg(test)] intercept_*`) is the model.

## Rendering hazards

- ▲ ● ✓ ◆ are East-Asian Ambiguous: unicode-width says 1, TestBackend lays them out as 1, a terminal with an emoji-preferring fallback draws 2 and shifts the row. Mitigation is the 2-column glyph gutter (screens.md) and an ascii tier; the demo implements the tier without a golden. Not testable with TestBackend.
- ratatui #2679 (open): `Paragraph` with `Wrap` panics on double-width graphemes. The preview is a Paragraph fed arbitrary terminal output. **Never enable `Wrap` on the preview** (the #58 audit also found wrap folds 200-column lines into `↳` fragments).
- Also open: #2357 emoji in List, #2526 Clear with straddling double-width glyphs, #1271, #925, #2730.
- Theme: one `display-message -p` at startup packing every `@thm_*` plus `#{client_termfeatures}`; RGB when it says RGB, else 256, else 16, else none. Do not trust crossterm's `COLORTERM`/`TERM` detection inside a popup (`TERM=tmux-256color`); tmux downgrades RGB itself unless `terminal-features` declares it, which cockpit sets. Verified: the demo emitted the owner's real `#3dea7d` green on the live server and `38;5;210` on the client-less demo server.
- **ratatui draws to stdout.** `act=$(cockpit-tui fleet)` swallowed the whole UI into the substitution and the popup was blank. The demo returns the action through a `COCKPIT_ACTION` file. (ncurses and bash+tput have the same trap; fzf and tview paint on the tty and return on stdout; dialog paints on stdout and returns on stderr; Textual paints on stderr.)

## Testing

Three layers. (a) `cargo test` with TestBackend goldens: the demo's 12 tests run in 0.02 s (0.83 s wall warm); fleet at 50/77/106, info at 54/82, the overlay, the ask answer, a sweep asserting the collapse order is always a prefix of [loc, ±, where]. (b) **The bash suite drives the binary with zero harness changes**: `tests/run.sh` puts a PATH shim at `$SHIM/tmux`, and the binary's `Command::new("tmux")` resolves through PATH; verified with two `exec -a claude sleep` panes and no `COCKPIT_TMUX` set, 3 real execs per run. A `== cockpit-tui ==` block asserts `--json` fields with the existing `is()` helper and skips cleanly when the binary is absent (`skip_` does not count as a pass). Also verified (2026-08-27): with no `COCKPIT_TMUX` override the binary follows `$TMUX` to a non-default `-L sock` server, both from a pane (`--rows` listed that server's two panes) and inside its `display-popup` (ticks 19–21 ms after the one 394 ms `claude agents` first tick), so a user on a private socket needs no configuration. (c) The popup path stays manual or becomes a freeze-rig case: pty + popup + keystroke tests are the flaky kind this repo avoids. CI: `cargo test` + clippy + fmt on ubuntu; `make tui` separate from `make check`, which must keep working with no cargo.

## Packaging

cargo-dist with three targets, checksums on by default (`.sha256` per archive + `sha256.sum`), `dist-manifest.json`; the `.tmux` fetch copies davychhouk/tmux-cmdline (uname → asset → `.sha256` → verify → `cargo build` gated on `rustc ≥ 1.88` → `display-message` warning, keep whatever binary exists). Binary lives in the plugin dir's `bin/` (gitignored), never `~/.local/bin` (`install.sh` there only manages its own symlinks, and a test asserts it). `@cockpit_tui_version` pins the tag; `prefix U` re-runs every `*.tmux` (verified in tpm's source), which is the hook point for "stale? fetch" via `run-shell -b`, never on plain load. Missing binary: the binding prints a message and exits 0. macOS: curl sets no quarantine xattr, so unsigned is fine for v1; cargo-dist has no notarization. cargo-binstall pulls metadata from crates.io and cannot install a GitHub-only binary. Measured sizes: 668 KB glibc / 761 KB static-pie musl for the 263-line spike; 783 KB / 889 KB for the 2,019-line demo; clean release 22–33 s, incremental 9–12 s, `cargo check` 0.3 s; musl needed only `rustup target add`.

## Popup vs window

`display-popup -E` for the board: blocking, one per client, correct for glance-then-jump; non-blocking popups (#4379) were closed for floating panes, which are a branch. Verified on 3.7c: 70×17, `TERM=tmux-256color`, exit on `q`, server RSS 3,676 → 3,996 KB. Require tmux ≥ 3.7 (#4920, "popup overwritten by background updates", matters for a hook-driven cockpit). `@cockpit_board_window on` for a parked window, mirroring `@cockpit_review_window`. `-E` makes a crash vanish: log to `${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/tui.log` and `2>>` it in the binding, on by default. Popups get focus-out/in on the pane under them since 3.4 (#3991), which touches the four `refresh-client -S` hooks (#58 decision 6).

## Phased plan (if built)

Phase 0, shippable with no new screen (~120 Rust, ~90 bash, ~40 CI): cache `claude agents --json`; mtime-gate transcript parses; split into `main.rs` + `tmux.rs` + `render.rs`; `--rows`/`--json`/`--version`; cargo-dist config and `release.yml`; the `.tmux` fetch; `make tui`; a doctor row. Gates: the bash block passes and skips cleanly without the binary; `make check` green with no cargo; `dist plan` lists three artifacts with checksums.

Phase 1, the board (~140 Rust, ~25 bash): grid, ANSI previews, needs-you border, ⇥/h/l/↵/q, dirty-flag redraw, theme at startup, 2-column gutter + ascii tier, no `Wrap`, no mouse; binding with log redirect, window mode, missing-binary message, tmux ≥ 3.7 gate. Gates: goldens at 80/120/160 showing 1/2/3 columns; needs-input tile red with its question; truncation property test over widths 1..40; `--json` names every agent pane and no other; the manual popup run recorded once on the issue.

Phase 2, inline reply (~150 Rust): a one-line input over a `needs-input` tile, ↵ spawns `tmux-review --deliver` via `run-shell -b`, Esc cancels; needs-input tiles sorted first. Gates: a fake `tmux-review` on PATH asserting the argv; an overlay golden; a non-needs-input tile refuses the input.

Not planned: drag/reorder, the fleet list as a Table, control mode, `notify`, tokio.

## The demo, as built

Files: data.rs 511 (tmux reads, transcript tail, git ±, row hash), fleet.rs 477 (profiles A/B/C, collapse order, question overlay, needs-you popup, loop, `--rows`/`--json`), ask.rs 228, board.rs 160, info.rs 140, theme.rs 127, main.rs 48, tests/screens.rs 212, demo.sh/capture.sh/wrap.sh. Deployed on `tmux -L demo` with four fake agents (needs-input with a sudo permission question, running + review pending, running, done + pasted) behind `prefix F/B/I/?`; `j`, `↵` in the popup moved the client from window 3 to window 1 through the wrapper. Run read-only against the live fleet with real transcripts and git facts. One real observer ask against pane %26: 4 s, $0.05, "Waiting on two parallel lanes … blocked on those reports, not on you" (accurate; `env -u CLAUDECODE` required inside the popup).

Deviations from screens.md, with reasons: no filter input; the needs-input question is an overlay over the row rect (Table has no row spans); the question wraps to 2 lines at every profile; the 5-column rail drops seconds past 10 min and minutes past 10 h (a live "10m05s" had rendered as "0m05s", clipped); info rail budget W−17 (W−15 left no gap); `state`'s rail is the age of the newest transcript event (nothing records when `@panestate` flipped); the needs-you popup is centred and fires only on a transition; keys ↵ jump / r review / f fix / q close; footer in the bottom border.

Measured: 2.9 MB RSS flat, 0.0–0.1 % CPU; ticks 10–18 ms live, 22–33 ms on the 4-pane demo after caching (one 380 ms first tick); redraw gate 17/17 on a static server, true every tick on the live one (previews change); `git diff --numstat HEAD` 4 ms warm; both theme rungs verified with `capture-pane -e`.

Still lacking: control-mode push (and, measured, not available anyway), mouse, scrollbar/paging past ~15 agents, filtering, packaging and the fallback path, the 16-colour rung, a golden for the ascii tier and any test for double-width drift, `asked` empty on busy transcripts (the last 64 KB often holds no string-content user line), inferred `@cockpit_review`/`_spool` semantics, no side-fix screen, `git diff` per path untested at 20 panes, silent errors.

## Verdict

Build it in Rust if a compiled screen is ever built, for the board alone, optional and additive, with actions in shell and the fzf list untouched. The demo shows it renders right, tests in a second, ships at under 1 MB static, and costs 3 MB resident. The three things that bite: the 380 ms `claude agents` fork (cache it), the 1.88 MSRV against distro toolchains (fail the source fallback loudly), and crossterm #1104 (no queries, pin 0.29.0, keep `q`). The bash board at 40 lines is the honest counter-offer: it works today at 47 ms/tick and needs no fetcher; the binary earns its tax only when the tile count, or mouse, or per-cell styling makes bash's cost visible. Owner decision 6 in the README.

## Push channel: unix socket vs SIGUSR1 vs poll (2026-08-27)

The owner asked for the demo to take push events over a socket. Added `src/push.rs` (106 lines: non-blocking `UnixListener` polled in the same loop as crossterm, a SIGUSR1 handler via `libc::signal`, a `notify <line>` client, Drop cleanup), `fleet --sock`, `bench.sh`; binary +10 KB; 12 tests still pass. The socket lives at `${XDG_RUNTIME_DIR:-/tmp}/cockpit-tui-<pid>.sock` (under 108 bytes, 0600), its path is published in `@cockpit_tui_sock` and the pid in `@cockpit_tui_pid`; a line only pulls the next tick forward, the 1 s poll stays underneath, and the line's content is logged and discarded (no payload is parsed; every action still comes from a keystroke and leaves as a printed line).

Latency from the trigger to the redraw showing the new state, 10 samples each, 4-agent demo server, the client's own exec included (what a hook pays):

| channel | min | median | max | wake | refetch + draw |
|---|---|---|---|---|---|
| unix socket | 28.2 ms | 34.0 ms | 37.1 ms | 8.5 ms | 25.3 ms |
| SIGUSR1 | 23.7 ms | 29.5 ms | 36.7 ms | 4.8 ms | 26.3 ms |
| poll (1 s) | 110.5 ms | 481.8 ms | 1013.2 ms | | |

The channel is not the cost: waking is 5–9 ms, the refetch (`list-panes` + `capture-pane`) is ~25 ms, and push only deletes the 0–1000 ms of waiting. Below ~30 ms the fix is not re-walking every pane per tick, which is a control-mode client pushing the changed pane, a different project. Idle cost with the listener open: RSS 2.98 MB (noise), CPU 0.3–0.9 % because the keyboard poll dropped to 10 ms to service `accept()`; the upgrade is `accept()` on a blocking thread that signals the loop, with the poll back at 100 ms (~8 lines, not done). Teardown: `q` and `display-popup -E` close both remove the file and unset the option; `kill -9` leaves a 0-byte file whose pid-named path never collides, startup connects first and unlinks on `ECONNREFUSED`, and `notify` against a stale option returns in 4 ms rc 0. End to end in a real popup: `set-option -p @panestate needs-input` + `cockpit-tui notify 'state %2 needs-input'` from another pane promoted the row and raised the needs-you overlay 0.4 s later. Security: 0600 in a 0700 runtime dir; if `XDG_RUNTIME_DIR` is unset the fallback is `/tmp`, where a pre-created file at the path is a pid guessing game; if this ships, refuse to bind under a world-writable dir. The hook must never block: connect-and-forget with a 50 ms write timeout.

Verdict: SIGUSR1 for cockpit (fastest, ~12 lines, the hook is `kill -USR1 "$(tmux show-option -gqv @cockpit_tui_pid)"`, nothing to unlink, no client binary, a dead pid fails harmlessly); the socket only if a message must ever carry a payload the refetch cannot discover; the 1 s poll stays as the floor. Publishing the pid or path is a write to the tmux server (`set-option -g`), exercised on the demo server only.
