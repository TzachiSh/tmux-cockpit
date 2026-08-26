# cockpit-tui: the engineering rulebook

For issue #61, tmux-cockpit's Rust/Ratatui binary. Every rule carries its evidence.
Paths are relative to the two research roots: `54/` = `~/.cache/cockpit-wt/issue-54-research/docs/superpowers/research/2026-08-26-ask-busy-agent/`,
`58/` = `~/.cache/cockpit-wt/issue-58-research/docs/superpowers/research/2026-08-26-ui-audit/`; bare paths are the plugin repo.

## 1. DO

**Crate, deps, toolchain**

1. One crate, flat modules in `src/`, a `lib.rs` that only re-exports.
   Nothing below 15k LOC in the eight apps read uses a workspace; the demo is 2,019 lines in 9 files. (54/ratatui.md §"Event loop", 54/spike/cockpit-tui-demo/src/lib.rs)
2. Five dependencies, no sixth without a measurement: `ratatui 0.30.2`, `crossterm "=0.29.0"`, `ansi-to-tui 8`, `serde_json`, `libc`.
   The demo ships every screen on exactly these; each added crate is build time and a supply-chain surface. (54/spike/cockpit-tui-demo/Cargo.toml.txt)
3. Pin crossterm exactly (`=0.29.0`), never a caret range.
   crossterm #1104 (open, 2026-08): an unrecognised `CSI ? … $y` report wedges the parser and swallows all later input. (54/ratatui.md §"Event loop and terminal")
4. Edition 2024, and gate the source-build fallback on `rustc >= 1.88`.
   ratatui 0.30.2's MSRV is 1.88.0; Debian 13 ships 1.85 and Ubuntu 24.04 ships 1.75, so a bare `cargo build` fallback dumps a compiler error on stock toolchains. (54/ratatui.md §"Corrections" 1)
5. Release profile `lto = true, strip = true, codegen-units = 1, opt-level = "s", panic = "abort"`, paired with `ratatui::init()`.
   785 KB glibc / 889 KB musl at 2,019 lines; init's panic hook runs *before* the abort and restores the terminal. (54/ratatui.md §"Corrections" 5, §"Packaging")

**Event loop and push**

6. Sync loop: a `needs_redraw` flag consumed at the top, `event::poll(100 ms)`, drain the whole input burst before one frame.
   One input source, one timer, ~4 forks a tick; the sidebar's 77-line loop is the proven shape. (54/ratatui.md §"Event loop", 54/spike/cockpit-tui-demo/src/fleet.rs:339-451)
7. Hash rows + previews and skip `terminal.draw()` when the hash is unchanged.
   17/17 consecutive no-redraws on a static server, 0 bytes/tick idle against fzf's unconditional 2,450 B/s. (54/competition.md table, 54/spike/cockpit-tui-demo/src/fleet.rs:385-389)
8. Keep the 1 s tick as the floor; a push *pulls the next tick forward*, never replaces it.
   A missed signal then costs one second and never a hang. (54/spike/cockpit-tui-demo/src/fleet.rs:342-344)
9. SIGUSR1 handler: set the flag **and** `libc::raise(libc::SIGWINCH)`.
   crossterm retries `poll` on EINTR, so a bare flag waits out the poll — 68 ms median vs 25 ms; SIGWINCH is the one signal crossterm turns into an event. (54/ratatui.md §"Push channel", 54/spike/cockpit-tui-demo/src/push.rs:11-17)
10. Treat `Event::Resize` as a refetch, not only a relayout.
    A real terminal resize then refreshes data too; verified a state change with no signal became visible on a 108×22 → 90×18 resize. (54/ratatui.md §"Push channel")
11. Publish the pid to `@cockpit_tui_pid` at start and unpublish it on **every** exit path: `q`, SIGHUP, SIGTERM, Drop.
    `display-popup -E` tears the child down by signal; the C twin lost its cleanup and left a dead pid published. (54/spike/c-demo/fleet.c:260-282)
12. The hook confirms the pid is the TUI before signalling: `[ "$(cat /proc/$pid/comm 2>/dev/null)" = cockpit-tui ] && kill -USR1 "$pid"`.
    A published pid outlives SIGKILL and SIGUSR1's default action on a recycled pid is *terminate*. (54/ratatui.md §"Push channel", rule 2 of the C twin)
13. The hook sends USR1 only; let SIGWINCH arrive from the terminal.
    ncurses surfaces WINCH only when the size actually changes, and a resize repaints the whole screen (1,109 bytes vs 21 for a glyph flip). (54/ratatui.md §"Push channel", rule 1 of the C twin)
14. Honour `COCKPIT_READONLY`: skip every `set-option`, including the pid publish.
    Without it, screenshotting the live fleet mutates it — a probe run already left a dead `@cockpit_tui_pid` on the owner's live server. (54/competition.md anomalies 6-7, 54/spike/c-demo/fleet.c:261)

**Data sources and caches**

15. One `list-panes -a -F` per tick, the format from `scripts/tmux-review:1706-1723` copied verbatim, built into **one** `-F` argument.
    Continuation lines each starting a fresh quoted string become separate argv words and tmux refuses them ("too many arguments"), reproduced live. (scripts/tmux-review:1701-1705)
16. Sentinel every optional field — `#{?#{@panestate},#{@panestate},-}` — and unwrap it after reading.
    Tab is IFS whitespace, so an unset field emits `\t\t`, collapses to one delimiter and shifts every field left; a pane silently vanished from the queue. (scripts/tmux-review:1708-1716)
17. Cache `claude agents --json` for 60 s, and only ever call it for panes with no `@cockpit_session`.
    380 ms per call (three runs, 0.37-0.38 s); uncached it made a tick 0.40-0.47 s instead of 0.00. The single most important implementation constraint. (54/ratatui.md §"Corrections" 3, 54/spike/cockpit-tui-demo/src/data.rs:190-213)
18. Resolve the session id for such panes with one `ps -eo pid=,ppid=` BFS from `pane_pid`.
    One fork for the whole fleet instead of a `ps`/`lsof` per pid. (54/ratatui.md §"Data model", 54/spike/cockpit-tui-demo/src/data.rs:229-256)
19. Read the last 64 KB of a transcript, whole lines only, and skip the parse when `fs::metadata().modified()` is unchanged.
    0.1 s on a 1.2 MB live file, and the mtime gate makes an idle session free. (54/claude-code.md §"Transcripts", 54/ratatui.md §"Data model" — the demo does the tail, not yet the gate)
20. `capture-pane -ep` per **visible** tile only, staggering unfocused tiles past ~8.
    ~4.5 ms each; 30,000 calls against 10 scrolling panes left server RSS flat (3,860 → 3,964 KB). (54/ratatui.md §"Data model")
21. Measure tmux server RSS over an hour before any 1 Hz view ships.
    tmux-agent-sidebar #85: a 1 Hz `list-panes -a` grew the server ~2 GB/day, only `kill-server` freed it. (54/rust.md §2, 58/README.md §"What was measured")
22. Take the transcript fields from the documented set: `tool_use` `name` + `input.description // .prompt // .command // .file_path`, the last `text` block, the last string `message.content`.
    Those are the four that exist on 2.1.246; permission waits never appear (the `tool_use` is written only after approval). (54/claude-code.md §"Transcripts", §"/btw")

**Actions, output, theme, rendering**

23. Every mutating action stays a shell script; the binary prints `<verb> <pane-id>` and a wrapper runs it.
    Keeps the bash path the source of truth and lets the 1000-assertion suite keep running with no binary. (54/tui.md §6, 54/spike/cockpit-tui-demo/wrap.sh)
24. Return the action through a `COCKPIT_ACTION` file, not stdout.
    ratatui draws to stdout: `act=$(cockpit-tui fleet)` swallowed the whole UI and the popup rendered blank. (54/ratatui.md §"Rendering hazards", 54/candidates.md §"Popup output channels", 54/spike/cockpit-tui-demo/src/main.rs:38-47)
25. Any action that must outlive the popup (`tmux-review --deliver`) goes through `run-shell -b`.
    A popup kills its own children the moment it closes. (scripts/tmux-review:~1905, 54/ratatui.md §"Data model")
26. Resolve tmux as bare `Command::new("tmux")` through PATH, with `COCKPIT_TMUX` as the only override.
    `tests/run.sh` puts a PATH shim at `$SHIM/tmux`, so the bash suite drives the binary with zero harness changes; with no override the binary also follows `$TMUX` to a private `-L` socket unconfigured. (tests/run.sh:26-49, 54/ratatui.md §"Testing")
27. Load the theme once at startup: one `display-message -p` for `#{client_termfeatures}` plus one `show-options -g` for the `@thm_*` roles; RGB when it says RGB, else the 256 column.
    Do not trust crossterm's `COLORTERM`/`TERM` detection inside a popup (`TERM=tmux-256color`); tmux downgrades RGB itself unless `terminal-features` declares it. (54/ratatui.md §"Rendering hazards", 54/spike/cockpit-tui-demo/src/theme.rs:47-60)
28. Keep the 2-column glyph gutter and a `COCKPIT_GLYPHS=ascii` tier.
    ▲ ● ✓ ◆ are East-Asian Ambiguous: unicode-width and TestBackend say 1, an emoji-preferring fallback draws 2 and shifts the row; the gutter lets a double-wide glyph eat its own padding. (54/screens.md §0 and §"Graceful degradation", 58/pickers.md §9)
29. Never enable `Wrap` on a preview.
    ratatui #2679 (open) panics on double-width graphemes in a wrapped `Paragraph`, and wrap folds 200-column pane lines into `↳` fragments. (54/ratatui.md §"Rendering hazards", 58/README.md §"What was measured")
30. Send no terminal queries from inside the popup, and keep `q` as an exit that does not depend on Escape parsing.
    tmux itself emits `CSI ? 2031 h` / `CSI ? 996 n` into a popup; that family crashed tmux next-3.7 (#4942) and wedges crossterm (#1104). (54/ratatui.md §"Event loop", 54/rust.md §3)
31. Truncate with `…`, counting characters; paths truncate left; the right rail is exactly 5 columns of elapsed duration.
    A 6-column duration is clipped by the rail and loses its leading digit — a live "10m05s" rendered as "0m05s". (54/screens.md §0, 54/ratatui.md §"The demo, as built")
32. Refuse to draw below 40 usable columns: print "terminal too narrow — prefix i on the pane instead".
    Better than a row that lies about which agent it names. (54/screens.md §"Graceful degradation", 54/spike/cockpit-tui-demo/src/fleet.rs:24)
33. Log to `${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/tui.log` and `2>>` it in the binding, on by default.
    `display-popup -E` closes on exit, so a crash vanishes with no trace. (54/ratatui.md §"Popup vs window", 54/tui.md §5)

**Popup, packaging, tests, process**

34. `display-popup -E` for the board; `@cockpit_board_window on` for a parked window, mirroring `@cockpit_review_window`.
    Blocking, one per client, correct for glance-then-jump; non-blocking popups (#4379) were closed in favour of floating panes, which are a branch. (54/ratatui.md §"Popup vs window")
35. Require tmux ≥ 3.7 for the binary's bindings.
    3.7 fixed "popup window gets overwritten by background updates" (#4920); a hook-driven cockpit repaints constantly, so below 3.7 the popup tears. (54/tui.md §5, 54/ratatui.md §"Popup vs window")
36. Ship with cargo-dist, three targets (x86_64 musl, x86_64 + aarch64 darwin), checksums on.
    `aarch64-unknown-linux-musl` needs hand-wired cross linkers (cargo-dist #1581); darwin cannot be built from Linux, and macOS runners are free. (54/ratatui.md §"Corrections" 6, §"Packaging")
37. The `.tmux` fetch copies davychhouk/tmux-cmdline: uname → asset → `.sha256` → verify → `cargo build` gated on rustc → `display-message` warning, keeping whatever binary exists.
    The one clean pattern among twelve `.tmux` files that shell out to cargo; verify-then-install is the only control that caught the Codecov uploader tamper. (54/rust.md §5, 58/install.md §"What not to do")
38. Binary lives in the plugin dir's `bin/`, gitignored.
    `install.sh` only manages its own symlinks in `~/.local/bin` and a test asserts it. (54/ratatui.md §"Packaging")
39. `@cockpit_tui_version` pins the tag, and `prefix U` (which re-runs every `*.tmux`) is the only fetch point, via `run-shell -b`.
    tpm's own precedent: install and update are gestures the human makes, never a config-load network call. (54/ratatui.md §"Packaging", 58/install.md §"Patterns worth copying")
40. Missing binary: the binding prints one message and exits 0.
    The binary is additive; `prefix r` (fzf) remains the fleet view either way. (54/ratatui.md §"What the binary would own", 54/tui.md §6)
41. Tests are TestBackend goldens with a `COCKPIT_BLESS=1` rewrite path, plus a `== cockpit-tui ==` block in `tests/run.sh` asserting `--json` fields with the existing `is()` helper.
    12 tests in 0.02 s; the bash block needs no harness changes and `skip_` does not count as a pass. (54/spike/cockpit-tui-demo/tests/screens.rs, tests/run.sh:76, 54/ratatui.md §"Testing")
42. Keep property tests for the contracts a golden cannot hold: collapse order is always a prefix of [loc, ±, where]; every `fmt_dur` ≤ 5 columns; every profile fits its width up to the last column.
    A golden pins one width; these pin the rule across all of them. (54/spike/cockpit-tui-demo/tests/screens.rs:156-212)
43. `make tui` stays separate from `make check`, and `make check` must stay green with no cargo installed.
    Everything else in the repo is shell; the binary must not become a build prerequisite. (54/ratatui.md §"Testing", Makefile header)
44. Pin a count or poll a condition in every test; never a clock, never a `sleep`.
    Five issues (#18/#32/#36/#44/#47) were wall-clock gates measuring the box under peer load; the prelude already has `until_cap` and `until_file`. (memory: fleet-timing-gate-branch)
45. Open a GitHub issue before the fix; build in a worktree under `~/.cache/cockpit-wt/<branch>`; the main checkout stays on `master`.
    Concurrent sessions share the main checkout, so a `git checkout` there swaps files under another session mid-run. (memory: issue-before-fix, branch-work-in-cockpit-wt-worktrees)
46. Never write to the live tmux server — private `-L` sockets for every experiment, read-only (`list-keys`, `show-options`, `capture-pane`) on the live one.
    Committed as repo policy in `AGENTS.md`. (AGENTS.md, memory: never-experiment-on-live-cockpit-server)
47. Comments: 2-3 lines maximum, one fact the code cannot say (a measurement, a constraint, a trap).
    The repo's paragraph-per-block habit is not a license; matching it drew two review comments in one sitting. (memory: comments-why-only-short)
48. Every phase plan opens with a ponytail audit table: what the research asked for, keep/skip/replace, and why.
    The owner wants the lazy version built and the gap written down, not silently dropped or silently built. (memory: ponytail-governs-plans)

## 2. DO NOT

1. **No tokio.** One input source, one timer, four forks a tick; recon declares tokio and never uses it. (54/ratatui.md §"Event loop", 54/rust.md §2)
2. **No `notify`/inotify.** One watch per file and directory against `max_user_watches`, plus a thread, for a screen that redraws once a second. (54/ratatui.md §"Data model")
3. **No control mode.** `refresh-client -B` reports "at most once a second" (measured 0.54 s, and five writes 30 ms apart collapsed to one notification); `%output` is raw pane bytes, so previews would mean writing a terminal emulator. (54/ratatui.md §"Corrections" 2)
4. **No unix socket for push.** It measured 32 ms against a signal's 31 ms, cost 106 lines, and carried no payload the receiver could not refetch. (54/ratatui.md §"Push channel", 54/competition.md table)
5. **No mouse in phase 1.** Enabling SGR capture inside a popup takes the wheel and text selection away. (54/ratatui.md §"Event loop", 54/tui.md §5)
6. **Never rebuild the fleet list as a Table.** 1,756 lines of Rust bought j/k/enter; fzf gives fuzzy filter, multi-select and the wheel for free, and the demo's Table has no filter input at all. (54/competition.md §"Verdict", 54/ratatui.md §"What the binary would own")
7. **No workspace or crates split below 15k LOC.** Nobody among eight apps does it; the `event-driven` template is 272 lines total. (54/ratatui.md §"Event loop")
8. **No clap, anyhow, color-eyre, chrono, git2.** Five subcommands parse from `args[0..2]`; claude-tmux's git2 → libgit2 → libssh2 → openssl chain fails to build on a clean box and ships "libgit2.so.1.9 not found" issues. (54/rust.md §2, 54/spike/cockpit-tui-demo/src/main.rs)
9. **Never pair `panic = "abort"` with a Drop-based terminal guard.** No unwinding means Drop never runs. Pick `init()` + abort, or a Drop guard without abort. (54/ratatui.md §"Corrections" 5)
10. **Never `$(cockpit-tui …)`.** ratatui paints on stdout; command substitution swallows the UI and the popup is blank. (54/ratatui.md §"Rendering hazards")
11. **No `#{q:}` in any format.** It is the prime suspect in the tmux-agent-sidebar #85 server leak, and the fleet format already avoids it. (54/rust.md §2 and §6)
12. **Never call `claude agents --json` per tick, and never per pane.** 380 ms a call. (54/ratatui.md §"Corrections" 3)
13. **Never `--resume` a live session, forked or not.** Two processes resuming one id interleave into one transcript; the fork spike also cost $0.27 and left a stray transcript. (54/claude-code.md §"the fork spike")
14. **No download from `run-shell cockpit.tmux`.** A network call on the tmux startup path. (58/install.md §"(c) Stays out")
15. **No sudo, anywhere, ever, including the optional tier.** (58/install.md §"What not to do")
16. **Never install the binary into `~/.local/bin`.** That directory is `install.sh`'s symlink space and a test asserts it. (54/ratatui.md §"Packaging")
17. **Never require a Nerd Font.** README.md:255 promises it is not required; plain BMP glyphs stay the default and nerd is opt-in. (58/pickers.md §9)
18. **Never redraw on a timer.** Hash and skip; fzf's unconditional reload writes 2,450 B/s forever, which is the row a compiled board exists to win. (54/competition.md anomaly 2)
19. **Never leave the USR1 handler as a bare `AtomicBool`.** 68 ms median instead of 25 ms, because crossterm retries `poll` on EINTR. (54/ratatui.md §"Push channel")
20. **Never make Escape the only exit.** Escape is tmux's own dismiss key and the delay is tmux's `escape-time`; `q` must always work. (54/tui.md §5, 54/ratatui.md §"Event loop")
21. **Never enable `Wrap`** — see DO 29. (ratatui #2679, open)
22. **Never reuse `version_at_least` (cockpit.tmux:175-180) for a three-component version.** `${v#*.}` of `0.74.3` is `74.3`, fails its own digit test and reads as 0. Use `vnum`. (58/install.md §5, 58/pickers.md §5)
23. **Never `curl | sh`, and never install on an unfetchable checksums file.** "Could not verify" is not "verified". (58/install.md §"What not to do")
24. **Never write to the live tmux server from a spike, a screenshot run, or a benchmark.** It has already happened once. (54/competition.md anomalies 6-7)
25. **Do not add insta, or a snapshot framework, until eyeballing 7 goldens stops working.** (54/ratatui.md §"Event loop")
26. **Do not send keys into an agent pane** (`send-keys -l`, `/btw` injection, anything). The review-flow rule forbids stealing focus or typing into a composer; the observer path is read-only. (54/README.md §"Recommendation" 3 and 6, 54/ux.md §3)
27. **Do not write a C/ncurses version.** truecolour quantises to 256 on the owner's terminal, ~18 % of the file is hand-rolled unicode column arithmetic, and libncursesw/libtinfo ABI must match on every target. (54/competition.md §"Verdict")
28. **Do not build phase 2's reply as a second `display-popup`.** A popup is one per client and blocks the hook that raised it. (54/ratatui.md §"Popup vs window", 54/README.md §"Recommendation" 3)

## 3. Gates

Each is a command. A phase is done when all of its gates pass on a private `-L` socket, and the owner has seen one manual popup run recorded on the issue.

**Phase 0 — no new screen (~120 Rust, ~90 bash, ~40 CI)**

- `cargo test && cargo clippy -- -D warnings && cargo fmt --check` — green, 0 warnings.
- `PATH=$(printf %s "$PATH" | tr : '\n' | grep -v cargo | paste -sd:) make check` — green with no cargo on PATH.
- `tests/only.sh 'cockpit-tui'` with the binary present → `fail=0`; with `bin/cockpit-tui` moved aside → `fail=0` and `skipped>0` (never PASS).
- `strace -f -e execve -- cockpit-tui --rows` over 30 ticks on a 4-pane fixture with `@cockpit_session` set: exactly **0** `claude agents` execs after the first, and no tick in `$COCKPIT_LOG` above 100 ms.
- `dist plan` lists three artifacts, each with a `.sha256`, plus `sha256.sum` and `dist-manifest.json`.
- A fake `rustc` on PATH printing `1.85.0` makes the `.tmux` print "needs Rust ≥ 1.88" and exit 0, leaving any existing binary in place.
- 3,600 ticks against a 20-pane private server: tmux server RSS delta < 5 % (baseline 3,872 KB).

**Phase 1 — the board (~140 Rust, ~25 bash)**

- Goldens at 80 / 120 / 160 render 1 / 2 / 3 columns; a needs-input tile is in `needs` red and shows its question.
- Property test over widths 1..40 for `trunc`/`trunc_left`/`wrap`; `fmt_dur` ≤ 5 columns for the whole boundary set.
- `cockpit-tui --json` on a fixture of 3 agent panes + 2 shell panes names exactly the 3.
- `! grep -rn 'Wrap' src/` and `! grep -rn 'supports_keyboard_enhancement\|window_size\|SetCursorStyle' src/` — no wrap, no terminal queries.
- `COCKPIT_READONLY=1 cockpit-tui --rows` against a private server leaves `tmux show-option -gqv @cockpit_tui_pid` empty.
- Start the binary, `kill -TERM`, then `tmux show-option -gqv @cockpit_tui_pid` is empty; repeat by closing the popup with `q`.
- Push latency, 10 samples: flip `@panestate`, `kill -USR1`, poll `capture-pane` every 5 ms until the glyph changes → median < 60 ms; the same flip with no signal → > 400 ms.
- A tmux < 3.7 fake `tmux -V` makes the binding print a message and exit 0; `bin/cockpit-tui` absent does the same and `prefix r` still opens.
- `capture-pane -ep` of the popup on a real 3.7 server shows the owner's `@thm_green` in RGB and a 256 index on a client-less server.

**Phase 2 — inline reply (~150 Rust)**

- A fake `tmux-review` first on PATH records an argv containing `--deliver` and the target pane id, exactly once per ↵.
- A golden of the one-line input overlay over a `needs-input` tile; Esc leaves the tile unchanged.
- Pressing the reply key on a non-needs-input tile records **no** argv.
- `! grep -rn 'send-keys' src/` — the reply never types into the pane.
- needs-input tiles sort first in the `--json` output, before running, error, done, idle.

## 4. Read this first

1. This binary owns **one** screen: the board (N tiles, each a live `capture-pane` preview). The fleet list stays fzf, `prefix i` and `prefix f` stay bash.
2. It is optional and additive. Missing binary → one message, exit 0, `prefix r` still works.
3. It renders and decides; it never acts. It prints `<verb> <pane-id>` to a `COCKPIT_ACTION` file and a shell wrapper runs the existing script.
4. The three things that bite: the 380 ms `claude agents --json` fork (cache 60 s), the 1.88 MSRV against distro toolchains (fail the fallback loudly), and crossterm #1104 (send no queries, pin `=0.29.0`, keep `q`).
5. Push is signals: `kill -USR1` after a `/proc/<pid>/comm` check; the handler also `raise`s SIGWINCH or the wakeup waits out the poll. No socket, no control mode.
6. Publish `@cockpit_tui_pid` on start, unpublish on *every* exit path. Honour `COCKPIT_READONLY`.
7. One `list-panes -a -F` per tick, format copied verbatim from `scripts/tmux-review:1706-1723`, sentinels intact, one `-F` argument, no `#{q:}`.
8. Hash rows + previews, skip the draw when unchanged. 1 s tick floor; a push pulls it forward.
9. No tokio, no notify, no mouse in phase 1, no `Wrap`, no workspace, no clap/anyhow/git2.
10. Tests: TestBackend goldens (`COCKPIT_BLESS=1` to rewrite) plus a `== cockpit-tui ==` block in `tests/run.sh` that skips cleanly when the binary is absent. Pin a count, never a clock.
11. `make check` must stay green with no cargo installed. `make tui` is separate.
12. Ship with cargo-dist, three targets, checksums; fetch on `prefix U` only, never at tmux load; binary in `bin/`, gitignored.
13. Require tmux ≥ 3.7. Log to `~/.local/state/tmux-cockpit/tui.log` — `-E` hides crashes.
14. Process: issue first, worktree under `~/.cache/cockpit-wt/<branch>`, never touch the live server, comments 2-3 lines of *why*, ponytail audit table at the top of every phase plan.
15. The bash board is the honest counter-offer: 40 lines, 47 ms/tick, works today. The binary earns its distribution tax only when tile count, mouse, or per-cell styling makes bash's cost visible.

## Additions from the repo analyses (2026-08-27)

49. Probe the crossterm event reader with `event::poll(Duration::ZERO)` before `ratatui::init()` and before publishing the pid; on error print one line and exit 1. crossterm builds the reader lazily and caches a failed first attempt forever, which inside a popup means a board that draws and ignores `q`. (references.md, rust-code `sgr-agent-tui/src/terminal.rs:23-41`)
50. Wrap `terminal.draw` in a consecutive-failure budget (~30) and treat `ErrorKind::Interrupted | WouldBlock` from `poll`/`read` as retries; a bare `?` turns a `/dev/tty` blink into an exit with a stale errno, and our own SIGUSR1 delivers `Interrupted`. (references.md, rust-code `rc-cli/src/app.rs:1011-1029, 5186-5217`)
51. Unpublish the pid and restore the terminal from the signal handler with a pre-formatted fork+exec argv, never `Command::output()` in the handler, so `pkill` still cleans up when the loop is wedged. (references.md, rust-code `app.rs:981-999`)
52. The board's popup is `-w 90%` (or `@cockpit_board_window on`), not the 70 % the pickers use: a 70 % popup on a 120-column terminal is an 84-column board, one tile column. (ux.md §"geometry")
53. Sort tiles by `(state rank, pane id)`; never a clock in the sort key, or tiles reorder while the human reads them; page, never scroll; the grid geometry comes from the page size, not this page's occupancy. (ux.md §1.4-1.5)
54. Focus is border weight (`Thick`), state is border colour, tmux's active pane is a `▎` marker; they compose and all survive NO_COLOR, which the binary honours itself (no surveyed Rust app does). (ux.md §1.2, §5.3)
55. The `?` help overlay is generated from the dispatch table; there is no second key list. (ux.md §3.4)
56. Width budgets subtract `ellipsis.width()`, not 1: the ascii tier's `...` is three cells, and the demo's `trunc` counts `chars()`. (ux.md §5.4)
57. Wrap the Crossterm backend with zeroclaw's `WideCellCleanupBackend` (or an equivalent) before the first preview ships: ratatui 0.30 mis-positions the cell after a wide grapheme, and previews are arbitrary agent panes. Test at the escape-byte level. (references.md, zeroclaw `apps/zerocode/src/terminal_backend.rs:1-193, 269-384`)
58. Fetch the checksum manifest before the tarball; a missing manifest entry is "no binary for this platform", a mismatch is fatal, a missing checksum tool falls back to source. (references.md, zeroclaw `install.sh:326-364`)
59. Read the Rust floor from `Cargo.toml`'s `rust-version` in both the `.tmux` gate and CI; never hardcode "1.88" twice. (references.md, zeroclaw `ci.yml:440-460`)
60. Enforce the terminal-query and `Wrap` bans with `clippy.toml` `disallowed-methods` / `disallowed-types` (each with a `reason`) under `-D warnings`, and mirror the grep gates as `#[test]`s in `tests/gates.rs` so `make tui` runs them; keep the shell greps as a belt. (references.md, zeroclaw `clippy.toml`, `tests/architecture/`)
61. One `cleanup()` (unpublish the pid, restore the terminal) reached from the panic hook, the signal arm and the normal return, guarded by a `TERMINAL_ACTIVE` atomic. (references.md, zeroclaw `main.rs:59-61, 189-205, 425-432`)
62. Tests that set `COCKPIT_*` env vars take a process-global lock and restore on Drop. (references.md, zeroclaw `test_support.rs:9-33`)
63. Measure once whether 256-indexed colours survive the owner's tmux/terminal before shipping the 256 rung; zeroclaw's tests claim a screen terminfo collapses them while truecolor passes. (references.md, zeroclaw `color_depth.rs:207-264`)
