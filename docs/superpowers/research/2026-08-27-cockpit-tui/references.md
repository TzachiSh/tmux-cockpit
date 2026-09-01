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

## gitui (gitui-org/gitui)

MIT, 48,516 lines in a 6-crate workspace (src/popups 10,610; asyncgit 3,032), 33 direct deps, ratatui 0.30 / crossterm 0.29 / rustc 1.88; 31 popups and 5 tabs wired into two hand-maintained lists. Good for two or three small files and two patterns, bad for architecture.

DO:
- `centered_rect_absolute` (`src/ui/mod.rs:113-124`, 12 lines) and `rect_inside(min, max, r)` (`:89-111`, tests `:156-221`), the clamp rule 32 needs.
- The overlay idiom: `render_widget(Clear, area)` right before the bordered block, every time (`msg.rs:87`, `confirm.rs:39`, `help.rs:44`, `goto_line.rs:159`); message-box auto-width: longest line + 2, clamped to [60, frame width] (`msg.rs:41-56`).
- Model the reply line on `popups/goto_line.rs` (167 lines: a `String`, push/pop, Esc clears, Enter emits, drawn as a `Paragraph` in a bordered block into a centred rect), not `textinput.rs`; paint the cursor as a reversed cell, gitui hides the real cursor globally (`main.rs:345`) and never calls `set_cursor`.
- Key hints built from the binding table at call time: `format!("Help [{}]", key_config.get_hint(keys.open_help))` (`strings.rs:603-612`, `keys/key_config.rs:76-119`); for cockpit a `const KEYS` rendered by both `?` and the footer.
- `command_pump` stops at the first `CommandBlocking::Blocking` so an open overlay's key list replaces the board's (`components/mod.rs:158-170, 202-210`).
- Semantic theme accessors: 21 colour fields, no call site names one (`ui/style.rs:46-109`); keep `@thm_*` roles behind the same shape so the RGB-vs-256 choice lives in one function.
- `Queue` (`src/queue.rs:169-193`, 25 lines): `Rc<RefCell<VecDeque>>`, drained after every event with `NeedsUpdate` bitflags; single-threaded, no `&mut` threaded through draw.
- Its default key symbols are the Ambiguous-width set (`keys/symbols.rs:28-51`) with a whole override file; cockpit needs the table, not the file.

DO NOT:
- ratatui-textarea: `textinput.rs` is 890 lines wrapping the crate, 353 of them remapping crossterm to readline verbs (`:262-615`) plus a shift-selection machine and a OnceCell dance.
- The Component / DrawableComponent / CommandInfo framework: the popup list is written twice by hand (`app.rs:495-534, 537-570`) with macros to paper over it.
- A popup "stack": there is none; all 31 draw into the same rect in list order, "top" means earliest in the list; `popup_stack.rs` is back-navigation history. For cockpit: `enum Overlay { None, Triage, Help }`.
- Its theme policy: zero capability detection, defaults are ANSI-16 names, RGB only from `theme.ron`; wrong inside a popup on `tmux-256color`.
- Config errors swallowed into `log::error!` and defaults (`key_list.rs:240-251`, `style.rs:307-327`); invisible under `-E`.
- asyncjob: a coalescing one-slot queue over three locks, rayon and crossbeam, inside asyncgit (git2 + gix). Our expensive call is already cached.
- The main loop: full redraw after every event (`gitui.rs:141`), watcher XOR a 5 s ticker. The one trick: `Spinner::draw` writes a single cell straight to the backend (`spinner.rs:39-63`), optional.
- As a base: git types leak into the UI layer (`ui/style.rs:3`, `popups/help.rs:11`, `queue.rs:9-15`).

Verdict: MIT, copy ~60 lines and two patterns; imitate `goto_line.rs` for phase 2; nothing else.

## agents-in-a-box (stevengonsalvez/agents-in-a-box)

"Context engineering for agentic coding": manages Claude Code / Codex / Gemini / Copilot with worktree isolation plus 94 skills. MIT, 21 stars, commit 2026-08-26; 639,529 lines in a 34-crate workspace, 775 packages (tokio full, git2 vendored-openssl, rusqlite, bollard, axum, portable-pty, vt100, sqlx, reqwest, notify, ratatui 0.30 and 0.26 in one lockfile). The architecture is the anti-pattern; one crate, `ainb-fleet-core` (9,671 lines), solves exactly phase 1+2's discovery/classification/tailing problem and is dense with measured traps.

DO:
- Classify agent panes by the process tree, never by `pane_current_command`: a running Claude renames itself to its version (`2.1.220`), an exited one's pane reads `zsh` (`discover/tmux.rs:80-84`, tests `:267, :279`: a session named `…-claude-resume` minted a bogus row under the string heuristic).
- `ps -axww` (not `-eo`): macOS truncates to terminal width even into a pipe and `comm` is the last column; lowercase the basename so `/Applications/Claude.app/…/Claude` is not the CLI; tokenise only the two leading numeric columns; BFS with `MAX_DEPTH = 8` and a seen set (`process_tree.rs:34-42, 102-121, 168`). ~40 lines.
- `capture-pane` failure is exit 1 with empty stdout; never return `Ok("")` (`read/tmux_pane.rs:51-55`): the difference between "blank pane" and "pane is gone".
- Snap byte offsets to char boundaries before slicing pane text; a live panic, twice (`read/errors.rs:54-64`, `read/tmux_pane.rs:70-75`, regression test `:88-94`).
- **needs-input is a lifecycle**: an `AskUserQuestion` is open iff no later `user` row carries a `tool_result` with its `tool_use_id`; an answer, a 60 s AFK timeout and an interrupt all emit the pair (`jsonl_tail.rs:267-288`; test `needs.rs:599`). The previous version pinned a session to ASK forever.
- **`stop_reason == "end_turn"` never fires on Claude ≥ 2.1.x**: the visible reply rides a `stop_reason: null` row and the only `end_turn` is a later metadata row with no text (`jsonl_tail.rs:79-96`); gate on `matches!(stop_reason, None | Some("end_turn"))` plus "has text" plus "no user follow-up" plus age (`needs.rs:203-221`). Our rule 22 lists the fields but not this gate.
- Classifier priority ASK > ERR > WAIT > IDLE, one signal per tile; pane-capture regex primary, transcript fallback; a specific 7-pattern error set ("better to miss an error than to spam") (`needs.rs:1-6, 145-225`, `errors.rs:17-48`).
- Test isolation: clear `TMUX`, `TMUX_PANE` and `TMUX_TMPDIR` (a `-L` flag on our calls does not reach children; `TMUX_TMPDIR` is ignored while `TMUX` is set), and keep an assertion that panes live on the private server (`tests/tmux_send_integrity.rs:36-73`; a real CI failure where one test's teardown killed a sibling's server).
- EINTR guard on `poll`/`read` (`ainb-core/src/main.rs:499-505`): our rule 9 raises SIGWINCH on every push, so this path is hit by design.
- `terminal.clear()` on `Event::Resize` (`main.rs:981-985`).
- `unsupported platform → print the cargo command and exit 0` (`install.sh:130-136`); the `KNOWN_OLD_*` byte-match before offering to upgrade a shipped conf (`cli/tmux_install.rs:36-43`).
- Field preference for the `doing` line: `command → file_path → path → pattern → description → query → subject` (`jsonl_tail.rs:752-768`); ours lacks path/pattern/query/subject and ranks `description` too high; keep `prompt` (they lack it, so a Task renders bare).
- The cheapest phase-2 trigger: a `PreToolUse` hook matched on `AskUserQuestion` carries the question at the moment the picker opens (`jsonl_tail.rs:398-405`); it writes the pane option and SIGUSR1 pulls the tick forward. No transcript polling for needs-input at all.

DO NOT:
- The architecture (34 crates, a fake dependency as a transitive version cap because `time 0.3.48` breaks against ratatui-core's blanket impls, "the cause of every red CI run since 2026-06-11", `ainb-tui/Cargo.toml:139-146`).
- A `send-keys` path: `send/tmux.rs` is 1,234 lines plus ~3,000 lines of tests to deliver one string; a macOS PTY reads at most 1,022 bytes, Claude classifies a ≥ 801-char read as a paste, the trailing Enter fuses into the payload's final read on a busy target, retried Enters arrived as `\r\r\r`, "there is no safe fixed delay; the wait must be OBSERVED"; an idle composer renders a dim ghost of the previous prompt (`send/tmux.rs:19-56`, `read/tmux_pane.rs:12-21`). The receipt for our DO NOT 26.
- A 64 KB transcript tail: rows are whole tool results, live transcripts reach 130 MB+, one `Read` result can exceed 64 KB, and the window then holds one fragment and every probe goes blind (`jsonl_tail.rs:566-585, 632-645`); they use 320 rows / 4 MiB with a grow-and-retry. Also: check whether the window opened on a row boundary instead of unconditionally dropping the first line (`:612-621`), and decode with `from_utf8_lossy` over the whole buffer (`:623-627`).
- mtime alone as the cache key (resolution can be 1 s); and any cached offset is invalid after `/compact` or `--resume`, which rotate to a new `<uuid>.jsonl` and re-emit old turns (`fleet/bridge/transport.rs:17-28`).
- Picking the newest-mtime `.jsonl` in the project dir: two agents in one cwd would classify from whichever was touched last (`needs.rs:130-141`); the hook's `transcript_path` into a pane option is the source, the cwd scan a fallback. The project slug maps every non-`[A-Za-z0-9-]` char to `-` (so `.` and `_` too) after canonicalising the path (`jsonl_tail.rs:15-48`); symlinked worktrees are our normal case.
- Its install path: `curl | bash`, sudo, `~/.local/bin`, checksum verification that soft-fails twice (`install.sh:65, 90-97, 156, 175-207`); a launchd/systemd timer for background release checks (`update.rs:688-777`).
- Five hand-rolled terminal enable/disable pairs with no shared guard, one asymmetric; a panic hook that replaces rather than chains the default.
- Their gaps: nothing handles `isSidechain` / `parentUuid` / `isCompactSummary` (a subagent's text can become the preview line; filter `isSidechain != true`); the API-error regex runs on the raw JSONL line, so a reply that says "API Error" classifies the session as errored.

Verdict: reference only; steal eight techniques, zero files.

## tmux-claude-session-manager (craftzdog), asked as "can we drop Rust and do the same"

385 lines of bash across seven files; tmux ≥ 3.2, fzf, jq, Claude Code ≥ 2.1.139. One fzf list, one row per running Claude process, status dot from `claude agents --json`, identity by pid joined pid→tty→pane in one awk over three tagged streams (three subprocesses regardless of fleet size), age from the transcript's mtime, sort rank then age; ONE preview (`capture-pane -ept {2}`, `--preview-window up,70%,follow`); Enter attaches through a nested tmux in a popup (closing the popup detaches, the agent survives; `list.sh:31-61` documents the teardown race); needs-input is bell forwarding only, and only if Claude is configured to ring; no reply, no timer, a 600 s cache. It cannot show N sessions at once, so it cannot meet "manage multiple sessions from one place"; cockpit's `prefix r` already is this shape with more columns, a filter and a review loop. Worth lifting: ~90 lines (the pid→tty→pane join so two Claudes in one cwd are two rows; bell forwarding).

The lane then built the bash board to ux.md's design (`scratchpad/board2/fleet-board2.sh`, 160 lines: needs-first, `clamp(W/48,1,3)` grid, density ladder, whole-row scroll, h/l/j/k, ↵/r/f actions, inline reply via a `tmux-review --deliver` store file, push over a fifo because bash restarts `read -t` after a trap so SIGUSR1 never wakes it) and measured it against the Rust board on a private server at 160×40, 1 Hz, 20 ticks:

| | bash board | Rust board |
|---|---|---|
| CPU/tick @3 / 6 / 9 tiles | 44 / 62 / 77 ms | 21 / 23 / 28 ms |
| bytes/tick @3 / 6 / 9 | 1,764 / 2,394 / 3,066 | 0 / 0 / 0 |
| execs **per second** @3 / 6 / 9 (both at the 1 Hz tick this ran at) | 5 / 10 / 14 | ~36, but measured under a 200 ms focused-tile capture — five refreshes a second, not the flat 1 s the board shipped with |
| RSS | 4.5 MB | 3.2 MB |
| push | fifo 41 ms (36–47) | SIGUSR1 31 ms |
| poll | 824 ms median | 712 ms |
| in `display-popup -E` | yes, keys and ↵ verified | yes |
| LOC | 160 (213 with harness) | 1,968 (base.md §"size": `src` 1,756 + `tests/screens.rs` 212) |

The 47 ms in candidates.md was three fork bugs, not a floor (`$(cup)` per preview line, `tput` per frame, awk per line); fixed, a tile costs two execs. Where bash breaks, measured: **wide glyphs bleed across tiles** (a CJK line in a 52-column tile is 104 display columns and wiped the first preview row of both tiles to its right; bash has no wcwidth); the fifo forwarder holds the tty raw and must be torn down while the human types a reply, so no push arrives during a reply; `${s:0:n}` on `-e` output cuts mid-escape (the awk clip fixes it at no cost); unconditional full repaint.

Verdict (the lane's, and the owner chose Rust on it): craftzdog's project cannot meet the need and cockpit already contains it; the bash board can meet it at 160 lines and no toolchain; Rust buys correct unicode width (the one correctness bug with no cheap bash fix), 0 bytes on an unchanged frame, 2–3 % of a core instead of 4–8 %, and headroom (bash extrapolates to ~130 ms at 16 tiles). If Rust had been dropped, ux.md and the tmux findings would have transferred verbatim; base/awesome/L4 would not.
