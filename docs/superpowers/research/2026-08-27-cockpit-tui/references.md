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
