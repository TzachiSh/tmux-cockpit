# Repos the owner asked about, analysed for cockpit-tui

One opus lane per repo, 2026-08-27, read-only. Each: what it is, DO (steal, with file:line), DO NOT (with evidence), verdict. The rules they are measured against are in [rules.md](rules.md).

## kwaak (bosun-ai/kwaak)

A Ratatui TUI over its own agent runtime (swiftide RAG + LLM tool loop): each chat builds a Docker container, indexes the repo into DuckDB, runs a coding agent that opens PRs. 331 stars, MIT, v0.19.0, last commit 2026-01-02, README opens with "no longer actively maintained". 14,826 lines of Rust (frontend 2,516; `app.rs` 756). ratatui 0.29 with the `unstable-rendered-line-info` feature, crossterm 0.28, ansi-to-tui 7, tui-textarea, tui-markdown, tui-logger, throbber-widgets-tui, ratatui-splash-screen, tokio "full", swiftide, duckdb bundled, octocrab, reqwest, clap, anyhow, copypasta, vendored openssl; 811 lockfile packages; MSRV 1.89. Zero references to tmux. **Name collision, no relation**: bosun-ai is kwaak's org; the "bosun" in awesome-ratatui is yetidevworks/bosun (43 stars, MIT, pushed 2026-08-24, "tmux-native orchestrator for AI agent sessions, Rust + ratatui"), which is cockpit-tui's real prior art and is being read by the base lane.

DO:
- Whole-frame TestBackend golden harness: `src/test_utils/integration.rs:32-61` renders the real `ui` into `TestBackend::new(160, 40)` and pins every nondeterministic field (a fixed UUID) before rendering; snapshots are plain quoted screen rows. Same shape as our goldens; take the "pin the nondeterminism first" discipline, not insta.
- Condition-driven event pump for tests: `app.rs:449-463` `handle_events_until(stop_fn)` under `cfg(debug_assertions)`, used in `tests/auto_scrolling.rs:9-19`; a predicate, never a sleep (our rule 44 in Rust). Worth a tiny `pump_until` if phase 2 needs an event-order assertion.
- One pure state→glyph function: `src/frontend/chat_mode/chat_list_widget.rs:30-46`. Our tile states want the same shape, golden-testable and swappable for the ascii tier (their glyphs are Nerd Font private-use with no fallback; that half is a DO NOT).
- Convert captured ANSI once and cache the `Text` on the model: `src/frontend/actions/diff.rs:36` + `message_formatting.rs:41-43`. Same as our `capture-pane -ep` previews: convert on change, never per frame.
- Phase-2 input line, the honest report: `tui-textarea` in a `Block` (`input_bar_widget.rs:33-35`), a throbber while busy, a send key with an explicit non-empty guard (`on_key.rs:13-26`), arrows reaching the textarea only when non-empty (`:72-82`). But two comments call their line wrapping a "Hack to get linewrapping to work with tui_textarea" (`ui.rs:50-53`, `on_key.rs:84-92`). For a one-line reply, hand-roll the ~40-line editor; take the key handling and the empty guard, skip the crate.
- cargo-dist lessons from `dist-workspace.toml`: they ship gnu, not musl, and needed cross images plus a hand-listed apt block (`Cargo.toml:122-134`); our musl-only Linux target avoids it. `install-updater = true` and `install-path = "CARGO_HOME"` are wrong for a tpm plugin.

DO NOT:
- Unconditional redraw: `app.rs:314-326` draws at the top of every iteration and a poller sends a `Tick` every 250 ms; an idle kwaak repaints 4×/s (our DO NOT 18).
- tokio for one input and one timer; the app can only quit via `std::process::exit(0)` "as any dangling threads can now safely be dropped" (`main.rs:336-337`).
- `PushKeyboardEnhancementFlags(all())` on startup (`main.rs:358-360`): the kitty CSI family, forbidden inside a popup (crossterm #1104, tmux `CSI ? 2031 h`); it is also why two of their three send bindings do nothing on a plain terminal.
- `Wrap` on the transcript (`chat_messages_widget.rs:40-42`) plus `line_count` via the unstable feature and hand-rolled scroll clamping (ratatui #2679).
- A network version check inside the constructor (`app.rs:163-167`, update-informer).
- Dependency sprawl: `copypasta` drags X11 so CI must install libxcb before it can lint (`.github/workflows/tests.yml:37-39`); vendored openssl with a "quickfix, should be investigated" comment (`Cargo.toml:112-116`); a PNG splash rendered every frame during indexing (`splash.rs:19`, `app.rs:316-322`).
- A 756-line `App` with 20 public fields where the renderer writes layout state the key handler reads (`ui.rs:38,51-53`, `actions/scroll.rs:22-33`). Keep render pure.

Verdict: reference only. Read four files for pattern (the test harness, the event pump, the glyph table, the send-key guard); fork or vendor nothing.

## rust-code (fortunto2/rust-code)

An AI terminal coding agent in Rust, a Claude Code competitor, not a tmux tool. 41 stars, MIT, created 2026-03, pushed 2026-08-22. 58,579 lines across a 9-crate workspace; tokio "full", clap, anyhow, ratatui 0.29 (held back by tui-textarea's `^0.29`), crossterm 0.28, syntect, rmcp, arboard, image; 845 unit tests, zero TestBackend goldens. tmux is only a background-job runner (`rc-bg` session, `capture-pane` preview behind F7).

DO (two named, reproduced startup hazards our rules lacked):
- `claim_input_reader()`, `crates/sgr-agent-tui/src/terminal.rs:32-41`, first thing in `init_terminal()`: a `crossterm::event::poll(Duration::ZERO)` at startup whose error becomes a message on a clean terminal. Their comment (`:23-31`): crossterm builds its event reader lazily on the first `poll` (epoll/kqueue handle, the tty, a signal pipe); if that first attempt fails it caches `None` and every later poll returns "Failed to initialize input reader" forever. Inside `display-popup -E` that is a board that draws tiles and ignores `q`. Add the probe before `ratatui::init()` and before the pid publish; exit 1 with the message. 8 lines.
- `FailureBudget` around `terminal.draw`, `crates/rc-cli/src/app.rs:5192-5205`, consts `:5186-5189` (`DRAW_ERROR_BUDGET = 30`, `INPUT_ERROR_BUDGET = 5`), used `:1017-1029`; the bug (`:1011-1015`, their issue #6): `Terminal::draw` re-reads the terminal size every call, on unix by opening `/dev/tty`, and a momentary failure ended the session with whatever errno was lying around ("Bad file descriptor (os error 9)"). A popup is where `/dev/tty` blinks (client detach, resize, parent pane closing). Also `is_transient_io()` (`:5211-5217`): `Interrupted | WouldBlock` are retries, everything else fatal; with our SIGUSR1 firing into `poll`, `ErrorKind::Interrupted` is the case we will hit, and a bare `?` there turns every push into a crash. Their 12-line test (`:5271-5283`) pins "a success resets the run".
- Restore the terminal and unpublish from the signal handler, not through the loop (`app.rs:981-999`, "keeps `pkill` working even when the loop is the thing that is stuck"). For us the unpublish must be async-signal-safe: pre-format the `tmux set-option -u` argv at startup and fork+exec, never `Command::output()` in the handler.
- `redirect_stderr` via `libc::dup2` (`terminal.rs:97-105`) so the log works however the binary is invoked; their panic hook prints on stdout too because stderr is redirected (`:58-67`).
- CI steps that name the incident they prevent (`.github/workflows/ci.yml` packaging job: "v0.6.6 shipped with a failed publish job").

DO NOT:
- `list-windows -F "#{window_name}|…"` then `split('|')` + `parts.len() >= 3` (`app.rs:2744-2751`, again `tools/bash.rs:262-263`): a window named `a|b` mislabels the row silently. Our rule 16.
- A "done" status keyed on `#{pane_dead}` that never fires because the pane is held open by `read` (`bash.rs:186-207`, `app.rs:2755-2757`); `pane_dead` only turns 1 with `remain-on-exit`.
- `Color::Rgb` unconditionally with no capability probe (`preview.rs:40`; zero hits for `COLORTERM|termfeatures` in 58k lines).
- `Wrap { trim: false }` in three places (`app.rs:1424`, `:2044`, `content_viewer.rs:120`), sitting on ratatui #2679.
- `install.sh`: documented `curl | bash`, `releases/latest/download`, never verifies the `.sha256` its own release workflow publishes, then `sudo cp` to `/usr/local/bin` (`install.sh:5,41-57`, `Makefile:54-57`). Every one of our DO NOT 15/16/23 in one file.
- A `make audit-loc` that flags files over 800 lines while `app.rs` is 5,322 and clippy `-D warnings` skips the binary crate ("rc-cli has legacy warnings", `Makefile:26`).
- A committed agent transcript with the author's home paths (`crates/rc-cli/.rust-code/session_*.jsonl`); gitignore the state dir before the first run.
- 9 crates for one binary, with a five-line comment explaining why a vendored fork is `exclude`d.

Verdict: reference only; steal ~35 lines (the ZERO-poll probe, the failure budget with `is_transient_io`, and their comments) into phase 0 before the first popup run.

## zeroclaw (zeroclaw-labs/zeroclaw)

A self-hosted agent runtime: one Rust binary, ~20 LLM providers, 30+ chat channels, tools from shell to GPIO to WASM. Not a tmux tool (zero hits for tmux, pty, capture-pane). 32,661 stars, MIT OR Apache-2.0, created 2026-02, commit 2026-08-27; 868,644 lines in a 26-crate workspace, MSRV 1.96, release profile `opt-level="z", lto="fat", codegen-units=1, strip, panic="abort"`. The relevant part is `apps/zerocode`, a Ratatui config/chat TUI: 58,986 lines in ONE crate with 41 flat modules (`chat.rs` 13,251 lines), ratatui 0.30, crossterm 0.29 (caret), tokio, unicode-width/segmentation, inkjet with 30 tree-sitter grammars, fluent, pulldown-cmark, clap, anyhow, chrono, ~22 direct deps.

DO:
- `apps/zerocode/src/terminal_backend.rs:1-193`, `WideCellCleanupBackend`: a Backend wrapper fixing a documented ratatui 0.30 bug (`:1-36`): the Crossterm backend emits an absolute `MoveTo` only when the next cell is not `prev.x + 1`, and that contiguity assumption breaks after a wide grapheme. Two modes handled: a CJK glyph whose continuation cell would be printed over its second half (suppressed, `:137-142`), and a VS16 emoji sequence whose trailing cell must start a fresh batch so a `MoveTo` is emitted (`:111-126`). Fabricates nothing. Our previews are arbitrary agent panes, so CJK and emoji will arrive. ~110 lines plus three byte-level tests (`:269-384`) asserting exact escape bytes; copy the test style. Wire `cell_width_u16` (`:62`) to the width function we already have.
- `apps/zerocode/src/color_depth.rs:34-69`, `detect_from_env`: a pure function of five env strings, no terminal query. Its tests (`:207-264`, `unconfigured_ssh_tmux_emits_truecolor`) claim the opposite of our fallback: 256-indexed escapes are the ones a screen/tmux terminfo collapses onto 16 slots, while truecolor passes verbatim, so they default to truecolor under `TERM=screen`. Rule 27 (ask tmux for `#{client_termfeatures}`) is the safer source, but measure once on the owner's terminal before shipping the 256 column. Also `rgb_to_xterm256` (`:103-145`): considers both the 6×6×6 cube and the 24-step grey ramp, so neutral `@thm_*` greys land on 232–255; 40 lines, one test. Take whole.
- `install.sh:326-364`: fetch `SHA256SUMS` BEFORE the tarball; the manifest lists every asset, so a missing entry means "no binary for this platform", not a download failure; a missing checksum tool falls back, only a mismatch is fatal. Reorder our `.tmux` fetch to match.
- `.github/workflows/ci.yml:440-460`: the MSRV job reads `rust-version` out of `Cargo.toml` (they had hardcoded pins drift). Have the `.tmux` rustc gate and CI both read the floor from `Cargo.toml`.
- `clippy.toml`: `disallowed-methods` entries each with a `reason` naming the replacement; with `-D warnings` a ban is a compile error, no false hits in comments. Use it for the crossterm queries and `disallowed-types` for `ratatui::widgets::Wrap`; keep the greps as a belt. They needed `#![allow(clippy::disallowed_methods)]` in two files (`main.rs:7`, `lib.rs:8`), so plan one escape hatch.
- `apps/zerocode/src/main.rs:59-61, 189-205, 425-432, 440-460`: one `static TERMINAL_ACTIVE: AtomicBool`, one `force_restore_terminal()` shared by the panic hook and the signal path, the app wrapped so a SIGTERM returns normally and the ordinary restore runs. They pair `panic = "abort"` with a hook and no Drop guard, confirming DO NOT 9 is about Drop, not hooks. Map to one `cleanup()` that unpublishes the pid.
- `apps/zerocode/src/doctor.rs:1052-1062`, an 11-line `render_buffer_to_string`, and `todo_tracker.rs:426-441` keeping a `render_to_buffer` variant so a test can assert per-cell colour, which the phase-1 gate "a needs-input tile is red" needs.
- `apps/zerocode/src/zerocode_pane.rs:229-250` `truncate_to_width`: measure first, reserve the ellipsis, advance by grapheme cluster accumulating width.
- `apps/zerocode/src/test_support.rs:9-33`: a process-global `env_test_lock()` plus an `EnvVarGuard` that removes the var on Drop; our `COCKPIT_READONLY/_GLYPHS/_TMUX` tests run on cargo's thread pool in one process.
- `tests/architecture/*.rs` (the idea, not the 2,687 lines): plain `#[test]`s that read repo files and fail on a forbidden pattern, so the grep gates run under `cargo test`. One `tests/gates.rs`, ~20 lines.

DO NOT:
- `config_manager.rs:64-69` calls `supports_keyboard_enhancement()` at init, a `CSI ? u` query with a blocking read (crossterm #1104); `window_size()` exposed at `terminal_backend.rs:182`.
- `app.rs:58` `TICK = 200 ms`, loop `:471`, `term.draw` at `:494` with no dirty flag: five frames a second forever; they throttled the fetch (`dashboard.rs:230-237`) and not the draw.
- Blocking `event::poll(TICK)` inside an `async fn` on a multi-thread tokio runtime (`app.rs:692`), parking a worker up to 200 ms.
- `crossterm = "0.29"` caret range.
- Four incompatible truncation helpers in one crate (`widgets.rs:272` chars, `zerocode_pane.rs:229` graphemes, `dashboard.rs:2685` chars, `input_bar.rs:629` cells) despite their own AGENTS.md "single source of truth" rule.
- 43 `Wrap {` sites (ratatui #2679 exposures).
- `EnableMouseCapture` unconditionally at init (`config_manager.rs:56-62`) and the 241-line `mouse.rs` plus a drag-coalescing ring buffer it forced (`app.rs:73-94`).
- 22 direct deps for a config TUI (30 tree-sitter grammars, fluent with 2,624 lines of translations).
- Raw OSC 52 written to stdout mid-session (`mouse.rs:143-155`); in a popup that reaches tmux, not the terminal.
- README advertises `curl … | sh` while its install.sh does verify; `#![allow(dead_code)]` crate-wide in `theme.rs:4` against its own AGENTS.md.

Verdict: reference only, plus two files and ~90 lines: `terminal_backend.rs` (the wide-glyph fix, with its byte-level tests), `color_depth.rs`'s `rgb_to_xterm256` and pure `detect_from_env`, and the helpers in DO g/h/i; read `install.sh:326-364` and `ci.yml:440-460` before writing the `.tmux` fetch. One measurement it forces: 256-indexed vs truecolor passthrough on the owner's terminal.

## bottom (ClementTsang/bottom), the grid engine

htop/gotop replacement. 13,941 stars, MIT, v0.14.8, 37,872 lines in one crate (no workspace), ratatui 0.30.2 default-features off, crossterm 0.29 (caret), edition 2024, rust-version 1.95; clap, anyhow, serde, itertools, indexmap, unicode-ellipsis, sysinfo. Its model is a TOML-configured tree of rows/cols/widgets; the board's is N uniform tiles, which is the whole verdict.

DO (under 60 lines total):
- The nested `Layout::split` walk, `canvas.rs:432-455`, with the load-bearing comment at `:427-431`: "layouts are already cached in ratatui, so we don't need to do it manually"; a grid may re-split every frame. For a uniform board: `Layout::horizontal(vec![Constraint::Fill(1); ncols])` then `vertical(... nrows)`.
- `widget_block()`, `drawing_utils.rs:31-51`: the one place every widget gets its border, `(is_basic, is_selected, border_type, style) -> Block`; widen to idle / focused / needs-input.
- Focus is one `if`: `canvas.rs:57-63` `get_border_style` → highlighted or plain, applied at `data_table/draw.rs:74-89`.
- Non-fitting widget skipped: `canvas.rs:485` `if width >= 2 && height >= 2`; the inner-area guard `data_table/draw.rs:161-163`. Keep rule 32's refusal below 40 columns and add this `>= 2` per-tile check.
- The tier ladder: `cpu_basic.rs:99-110` splits first, measures the resulting cell, then picks a label limit applied to every cell; the pattern for what a tile shows at 3/2/1 columns.
- Expand-to-fullscreen is a bool plus a re-dispatch of the same draw fn (`app.rs:111, 1264-1285`, `canvas.rs:226-291`, Esc at `app.rs:287-291`, a right-aligned " Esc to go back " title at `data_table/draw.rs:128-132`); ~15 lines if tile zoom is ever wanted.
- Two titles on one block: `block.title_top(left)` + `block.title_top(right.right_aligned())` (`data_table/draw.rs:87-95`); ratatui places both, no rail arithmetic.
- Title degradation, not truncation: prepare long and short forms, pick by width (`data_table/draw.rs:118-125`).
- `truncate_to_text` (`utils/strings.rs:8-10`) wraps `unicode_ellipsis::truncate_str`, width-aware; the sixth-dependency question rule 31 implies (take the crate, or hand-roll and property-test).
- Default palette is the 16 ANSI names (`themes/default.rs:11-21`); RGB appears only when a user writes hex; the zero-line 16-colour fallback when a role is unset.

DO NOT:
- Precompute a neighbour graph: `layout_manager.rs:40-553`, 514 lines of `BTreeMap<LineSegment, …>` geometry for an arbitrary user tree; a uniform grid is `idx ± 1`, `idx ± ncols`, clamped, under 15 lines.
- Model focus as a state machine: `move_widget_selection_logic` `app.rs:1295-1639` (345 lines) plus expanded-movement handlers (66 more).
- Build the row/col/ratio config (`options/config/layout.rs`, 2,620 lines across options/args/config); cockpit derives ncols from width. Copy nothing from its CLI/config layer.
- `canvas_handled()` hiding widgets with `Constraint::Length(0)` and skipping through them in the movement code.
- Mouse bookkeeping (`top_left_corner`/`bottom_right_corner`, the reset loop `canvas.rs:114-128`, the off-by-one hack noted at `widget_carousel.rs:130-141`).
- Treat `--basic` as the narrow layout: an opt-in flag, a separate hand-written branch with per-platform `#[cfg]` arithmetic (`canvas.rs:293-424`), expand disabled, its own 155-line layout constructor. bottom has no responsive layout; the 3/2/1-by-width board is not in it.
- Its focus keys as-is: uppercase H/L/K/J because lowercase scrolls inside the widget; Tab is a per-widget action. The precedent: the moment a tile scrolls, focus keys must move.
- Expect rendering tests: `grep TestBackend` is empty; `tests/integration/layout_movement_tests.rs` is nine lines of commented TODOs; layout tests assert `children[0].children[0].down_neighbour == Some(3)`. No `NO_COLOR` handling anywhere.

Verdict: reference, narrow. Five small things (~60 lines) cover the board's grid, borders, focus and tiers; tile zoom is ~15 more; the other ~1,400 lines serve nesting the board does not have.
