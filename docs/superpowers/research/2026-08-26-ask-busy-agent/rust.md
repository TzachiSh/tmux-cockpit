# Rust + Ratatui: the research

The owner asked "what about Rust?" after [tui.md](tui.md) measured Go. Same method: a Ratatui spike of the fleet view built and run against the live fleet, the source of the Rust tmux supervisors, the framework state, and how Rust tmux plugins ship under tpm. Spike in [spike/fleet-ratatui.rs](spike/fleet-ratatui.rs).

## 1. Three spikes, same screen, same fleet

| | Ratatui | Bubble Tea | bash + fzf `--listen` |
|---|---|---|---|
| code | 263 lines | 383 lines | 8 lines |
| toolchain | cargo 1.98, installed here | Go, not installed (70 MB SDK to scratch) | fzf 0.44, installed |
| deps | ratatui 0.30.2, ansi-to-tui 8, serde_json; 187 crates in the lockfile | bubbletea v1.3, lipgloss; ~20 modules | curl |
| clean release build | 32 s (lto, strip, opt-level z) | 5.5 s fetch + 9 s | none |
| binary | 0.64 MB, links libc + libgcc_s; musl target not installed (one `rustup target add` away) | 4.2 MB, fully static with `CGO_ENABLED=0` | n/a |
| headless `--rows`, 3 agents | 7 ms | 12 ms | 12 ms via the Go rows |
| tick inside the popup (70×17) | 7–8 ms | 10–23 ms | fzf reload |
| RSS / CPU at 1 s tick | 3.1 MB / 0.4 % | 12 MB / 3.5 % | 10 MB / 1.1 % |
| first render | right first time: `Layout::vertical([Length(n), Min(3)])` clipped the preview | preview wrapped and pushed the header off; one fix | right |
| ANSI preview | `ansi-to-tui` (ratatui org, 4.8M downloads) parses `capture-pane -e` into styled text | raw passthrough | fzf's preview |
| panic safety | `ratatui::init()` installs a hook that restores the terminal | none written | n/a |

The Rust one is the best-behaved of the three compiled numbers: smallest, lightest, fastest, correct on the first draw. It is still the same rows on screen.

## 2. The Rust supervisors, from source

| tool | ★ | LOC (TUI / data) | deps, crates | release | how it ships |
|---|---|---|---|---|---|
| recon | 262 | 4.8k (2.1k / 2.5k) | 8 deps, 132 crates; tokio declared and never used | none; `cargo install --path .` | `bind g display-popup -E -w 80% -h 60% recon` |
| claude-tmux | 203 | 5.5k (3.5k / 2k) | 7 deps, 139 crates; git2 → libgit2 → libssh2 → openssl | crates.io, 1,384 downloads | `cargo install claude-tmux`; `display-popup -E "~/.cargo/bin/claude-tmux"` |
| tmux-agent-sidebar | 498 | 40k (29k non-test; ui 8.5k, hooks 7k, pet 1k) | 8 pinned deps, 227 crates | 1.3–1.65 MB per platform, four platforms | tpm; `.tmux` looks in bin/, target/release/, PATH, else a `display-menu` wizard: download or build |
| tmux-thumbs | 1,095 | 1.8k | termion, 24 crates | 3.6 MB musl tar; x86_64 only, last release 2023 | tpm; first keypress opens a split running an interactive Compile-or-Download wizard |

Architecture: recon and claude-tmux are `poll(100–200 ms)` sync loops that shell out to `list-panes -a` and `capture-pane` per pane every 0.5–2 s and classify the pane text (recon: "Esc to cancel" → Input, a spinner glyph + "…" → Working; claude-tmux: content changed vs cache → Working, `[y/n]` → Waiting). claude-tmux renders the preview through `ansi-to-tui`, 15 lines. recon has no preview; its "view" mode draws sprites. Neither has a panic hook; a panic leaves the popup in raw mode, which `-E` hides by closing it. Neither matches `Event::Resize`; ratatui's autoresize on the next draw covers it.

tmux-agent-sidebar is cockpit's design twin in Rust: a `hook` subcommand reads the Claude hook JSON on stdin, resolves the pane from `$TMUX_PANE`, writes ~20 `@pane_*` options; the TUI is a persistent 15 % split per window (not a popup), redraws every second or on `SIGUSR1` from `after-select-pane` hooks, and reads state back with one `list-panes -a -F` of 25 `#{q:…}` fields plus a `ps` tree walk. It re-implemented in Rust what cockpit has in bash: hook parsing (2.5k LOC with tests), desktop notifications with dedupe stamps in pane options (555 LOC), a worktree flow, a git tab, a version check. Rust bought it the ps walk, the 1 s redraw and mouse. Open issue #85: the tmux **server** grows ~2 GB/day while the sidebar runs, new-window slows, only `kill-server` frees it; the pending PR blames `#{q:…}` in the once-a-second format. Any 1 Hz `list-panes -a` poller, the three spikes here included, has to check for that on 3.7 before shipping.

Build on this box (12 cores, cargo 1.98): tmux-thumbs 12 s, 24 crates; recon 31 s, 129 crates, 2.8 MB unstripped; claude-tmux fails at 17 s on `openssl-sys: Could not find directory of OpenSSL installation`, the dynamic-linking failure mode in the wild (its open issue #15 is "error while loading shared libraries: libgit2.so.1.9"). None builds offline from a cold registry.

## 3. Ratatui in 2026

0.30.2 (2026-06-19); the 0.29 → 0.30 gap was 14 months of alphas, patches now every few weeks. 0.30 split the crate into ratatui-core / ratatui-widgets / backends, MSRV 1.86, edition 2024, added `ratatui::init()`/`run()` (raw mode, alt screen, restoring panic hook), removed `Block::title()`. Backend: crossterm 0.29 (2025-04), selected by feature flag; termion and termwiz exist; the docs say "choose crossterm for most tasks". Widgets: Table with state, List, Paragraph, Scrollbar, Tabs, Sparkline, Gauge, Canvas. Event loop: sync `event::poll(tick)` + `read()` (the spike), or tokio + `EventStream` (the tutorial). Wide chars: unicode-width + segmentation; open issues on ambiguous-width clusters and emoji in lists, which matters for state glyphs. `ansi-to-tui` 8.0.1 is under the ratatui org and "ignores unknown or malformed escape sequences so you can feed it real terminal output", which is exactly `capture-pane -e`.

In tmux: colour detection reads `COLORTERM` then `TERM`, says 256 inside tmux, and does not matter because tmux downgrades RGB itself unless `terminal-features` says RGB (cockpit sets that today). Escape has no crossterm timeout; the delay users feel is tmux's `escape-time`, the same as with fzf. Kitty keyboard protocol is not in tmux (#4196 open), so `supports_keyboard_enhancement()` is a no-op there. Two real hazards: terminal queries from inside a popup crashed tmux next-3.7 (#4942, `CSI ? 12 $ p` from a crossterm app, "missing option cursor-style" because a popup has no pane; confirm the fix is in 3.7c), and crossterm #1104 (2026-08, open): an unrecognised `CSI ? … $y` report wedges the parser and swallows all later input, with tmux 3.6+ colour-scheme notifications named as a source. Rule: send no terminal queries from inside a popup, pin crossterm.

Measured by the ecosystem lane on a 60-line rows+preview app: 90 crates, 18 s clean release (0.3 s incremental), 602 KB with opt-level s, 697 KB static-pie on musl (13 s), init + draw + restore 0.16 ms.

## 4. Rust vs Go, for this binary

| | Rust | Go |
|---|---|---|
| binary | 0.6–0.7 MB | 4 MB |
| runtime cost, measured | 3 MB RSS, 0.4 % CPU | 12 MB, 3.5 % |
| clean build | 18–32 s, 90–190 crates | 9 s, ~20 modules |
| static Linux | musl target (`rustup target add`), musl malloc is slow under thread contention, irrelevant here | `CGO_ENABLED=0`, done |
| darwin from a Linux runner | no: cross cannot ship Darwin images, cargo-zigbuild breaks on SDK changes; cargo-dist uses native runners | yes, `GOOS=darwin GOARCH=arm64` |
| release tooling | cargo-dist 0.32 (2026-05), cargo-binstall; no macOS signing story (#1121 open since 2024) | goreleaser; same signing gap |
| ANSI preview | `ansi-to-tui`, first-party | passthrough, works by accident of lipgloss |
| panic in a popup | `ratatui::init` restores | hand-written |
| who writes it | cargo is on the owner's box | Go is not |

Go's one clear win is cross-compiling four targets from one Linux runner. Rust wins every runtime number and the first-draw correctness. Distribution is the same shape either way.

## 5. How Rust tmux plugins ship under tpm

Same pattern as Go, hand-rolled each time. The clean version is davychhouk/tmux-cmdline (2026-07): the `.tmux` maps `uname -s`/`-m` to a target triple, downloads `releases/download/$version/$asset` plus `$asset.sha256`, verifies, falls back to `cargo build --release`, else warns "no prebuilt binary for this platform; install Rust or select an available @tmux-cmdline-version". The version is a tmux option, so `prefix U` re-downloads. tmux-palette-rs commits the binaries into `dist/` so a plain tpm clone already has one. tmux-thumbs ships x86_64 only three years on, so every Apple Silicon user compiles. tmux-agent-sidebar's wizard is a `display-menu`: Download binary / Build from source. GitHub code search: zero `.tmux` files use cargo-binstall or cargo-dist; twelve run `cargo build --release`. All dead offline without the toolchain. Gatekeeper skips curl-fetched binaries (no quarantine xattr) today; Apple has been tightening.

Ratatui inside `display-popup -E`: claude-tmux, tmux-pilot, tmux-cmdline, tmux-palette-rs all do it, none has an open popup issue about Escape, colours, mouse or resize. tmux ≥ 3.3 resizes popups; 3.6 sends focus into them; the popup has its own default-shell and OSC 52.

## 6. Verdict

Rust is the better compiled option for cockpit, and by a margin: a 0.6 MB binary that draws right the first time, 3 MB resident, first-party ANSI parsing, a restoring panic hook, and cargo already on the owner's machine. If a compiled fleet view is ever built, build it in Rust, with the tmux-cmdline fetch pattern (uname → asset → sha256 → `cargo build` fallback → warn), musl for Linux, native macOS runners via cargo-dist, no terminal queries inside the popup, and the hybrid contract from tui.md §6: the binary optional, fzf the fallback, every action a shell script.

It still does not beat bash + fzf `--listen` for a rows + preview screen. What the compiled one adds is a real Table with aligned columns, a scrollbar, mouse on cells, and a 250 ms redraw without a subshell per tick. That is polish, not capability, bought with a release matrix, a fetcher in `cockpit.tmux`, and a new failure class (unsupported platform, offline first install, glibc drift, DECRQM-style query crashes). The trigger for building it is unchanged: a screen that is not a list.

One more thing the Rust lane surfaced that applies to every option, bash included: tmux-agent-sidebar's #85 server leak from a 1 Hz `list-panes -a` with `#{q:}` formats. The fleet `--fleet` already avoids `#{q:}`; keep it that way, and measure server RSS over an hour before any 1 Hz reload ships.
