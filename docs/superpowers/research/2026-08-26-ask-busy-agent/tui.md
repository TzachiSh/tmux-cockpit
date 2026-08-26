# Go + Bubble Tea, or something like it: the research

The owner asked "why not Go Bubble Tea?" after ux.md §4 said no in a table. This is the answer with numbers: two spikes built and measured against the live fleet, the source of the five Bubble Tea supervisors, the framework state in 2026, and how tmux plugins actually ship binaries. Spike code in [spike/](spike/).

## 1. Two spikes, same screen, same fleet

Both list cockpit's agent panes from the live server (read-only), with the state glyph, pane id, window, title, the `doing` line from the transcript tail with its age, the agent's last sentence, and a `capture-pane` preview of the selected row; `j`/`k`, `↵` jumps, `q` closes; refresh every second.

| | Bubble Tea ([fleet-bubbletea.go](spike/fleet-bubbletea.go)) | bash + fzf ([fleet-fzf.sh](spike/fleet-fzf.sh)) |
|---|---|---|
| code | 383 lines of Go (≈120 of them the transcript tail and the `claude agents` join, which the bash side gets from `tmux-review`) | 8 lines, on top of a `--fleet` that prints the `doing` column |
| dependencies | Go toolchain to build (not installed on this machine; 70 MB download), bubbletea v1.3 + lipgloss, 43 go.sum lines | fzf ≥ 0.36 for `--listen` (0.44.1 installed), curl |
| build | 5.5 s module fetch + 9 s compile | none |
| binary | 4.2 MB static (`-s -w`) | n/a |
| tick, 3 agents | 10–23 ms (list-panes + 3 transcript tails + capture-pane) | 12 ms rows + fzf's own reload |
| RSS / CPU at 1 s tick | 12 MB / 3.5 % | 10 MB / 1.1 % |
| inside `display-popup -E` | works: 70×17, `TERM=tmux-256color`, ticks logged | works, it is what cockpit already does |
| first render bug | yes: the preview's long lines wrapped and pushed the header off-screen; fixed with `lipgloss.MaxWidth` and a row count | none; fzf owns the layout |
| live refresh | `tea.Tick` | a loop `curl -XPOST localhost:$port -d 'reload(rows)'`; the ages tick 22m9s → 22m12s on screen |
| preview | own box, own clipping | `--preview 'tmux capture-pane -ep -t {2}'` |
| filter / multi-select / mouse wheel | none written | free |
| what the human sees | the same rows | the same rows |

The Bubble Tea one took one build to run and one fix to render right. It is not hard. It is also not different on screen.

## 2. What the five Bubble Tea supervisors cost their authors

Read from source (non-test LOC, `wc -l`; details in the bt-source lane):

| tool | total Go | TUI layer | Msg types / Update arms | release tarball | ships how |
|---|---|---|---|---|---|
| claude-squad | 7.3k | 2.3k (+1.7k lipgloss) | 9 / 13 | 1.9 MB | goreleaser, brew core, curl script |
| agent-manager | 33k | 18.5k (55 %) | 32 | 7.9 MB | goreleaser, brew tap, curl + checksum |
| agent-deck | 198k | 44k + 30k cmd | 52 / 59 | 13.8 MB | goreleaser, brew tap, `go install` |
| ntm | 413k | 28k | 54 / 55 | ~18 MB | goreleaser, brew tap, curl `--easy-mode` |
| Gas Town `gt feed` | 249k total | 3.7k feed + 0.6k | event chan, 100 ms tail | 13.9 MB | goreleaser, brew core; README: "avoid `go install` on macOS, Gatekeeper kills the unsigned binary" |

None is on Bubble Tea v2 yet; agent-manager has an open migration issue. ntm vendors bubbletea to delete one thing: the startup background-colour probe that "adds multi-second startup latency on terminals that ignore OSC queries".

What each one had to write that cockpit gets from its hooks: state detection from pane text (claude-squad `strings.Contains("Yes, allow once")`; agent-manager a 500-line regex rules engine plus its own hook installer; agent-deck hooks + content hash + a flicker detector in an 11k-line `instance.go`; ntm a spinner classifier; Gas Town has an open issue to "replace TUI status-bar scraping with a structural busy/idle signal"). Attach: claude-squad and agent-deck run `tmux attach` in their own PTY and sniff for Ctrl-Q, with a comment about nuking "control sequences like `?[?62c0;95;0c`" that arrive in the first 50 ms; agent-manager and Gas Town use `tea.ExecProcess`. Preview: agent-manager and agent-deck hold a `tmux -C` control-mode client open per selected session to avoid a fork per tick, with 2–4 MB read buffers because "capture-pane of a colored 200-column pane produces lines far past bufio's 64KB default".

Issue titles from their trackers, the tmux-specific ones: "NTM UI occasionally becomes corrupted after terminal resize" (fixed by `tea.ClearScreen` on every `WindowSizeMsg`), "Renderer broken (flickers) since ≥ 0.18" (WSL2 + tmux), "full-screen MaxWidth pass makes View() 51 % of CPU, laggy with ~55 sessions", "terminal-features grows unbounded on the tmux server, 5,018 entries after weeks of uptime" (`set -asq` on every spawn), "Ctrl+Q detaches only from one session per socket, falls through to the app and exits Claude Code", "First keypress after an attached session exits is silently discarded", "Session status misread as idle when tmux client has no UTF-8 locale", "No way out of a full-screen attach when the user's prefix is C-\\", "Light themes render light chrome on a dark canvas when OSC 11 cannot be delivered", "Bubble Tea v1.3.10 does not parse Kitty keyboard sequences, uppercase shortcuts silently dropped on Wayland" (agent-deck wrote a 593-line CSI-u reader).

Only ntm runs inside `display-popup`. It needed: stderr redirected to a file because `-E` tears the popup down the instant the process exits and "any error is painted and erased in the same frame", `ClearScreen` on resize, and a `popupMode` branch ("Escape closes the popup; zoom doesn't re-attach").

## 3. The framework in 2026

Bubble Tea v2.0.0 shipped 2026-02-24 (`charm.land/bubbletea/v2`), v2.0.9 on 2026-08-19: `View()` returns a struct, key/mouse messages split, alt-screen and mouse are declarative fields, a cell-diffing renderer, synchronized output (tmux ≥ 3.5 supports DECSET 2026, 3.6 answers the query), Kitty keyboard. v2 fixed the two things that used to bite in tmux, flicker and wide chars. Open in tmux still: startup terminal queries (the "altscreen takes a second to kick in" class; a query sent while alt-screen is active never completes, #1383; run as a subcommand it times out after ~5 s, #823), and colour detection under `TERM=tmux-256color` unless `COLORTERM` is exported (#825; lipgloss v2 dropped auto-detection, you choose the profile). bubblezone (mouse hit-testing) may not work with the lipgloss v2 compositor. Companions: bubbles v2.2.1, lipgloss v2.0.6, glamour v2.0.1.

Alternatives with the same shape: Ratatui (Rust, immediate mode, 1–4 MB, cargo present on this machine; more code per screen), tview (Go on tcell, `QueueUpdateDraw`, less ceremony for "a table that updates"; powers k9s), Textual (Python venv, 0.5–1 s start; wrong runtime for a tmux plugin), Ink (Node; same), Brick (Haskell; no). Shell-first: gum v2.0.0 (2026-08-20, 5 MB; Bubbles as a binary for scripts, no live mode), fzf (`--listen`, `reload`, `--tmux` popup since 0.53, preview `follow`).

## 4. How tmux plugins ship binaries

tpm does `git clone` and runs `*.tmux`. Every compiled plugin surveyed solves the rest itself:

| plugin | language | on load, when the binary is missing | offline without the toolchain |
|---|---|---|---|
| tmux-thumbs | Rust | first keypress opens a split running an interactive `select`: Compile (`cargo build --release`) or Download (curl the releases API; Darwin_x86_64 / Linux_x86_64 only, else "❌ Unknown platform") | the binding is dead; last release 2023-03 |
| tmux-fingers | Crystal | `run-shell -b install-wizard.sh`: releases API download or `shards build`; version check against `shard.yml` on every load | wizard pane with an error; no binding |
| tmux-popup-control | Go, Bubble Tea v2 in `display-popup` | `go mod vendor; make build`, else "make build doesn't seem to have done anything, bailing early!"; README: extract the tarball into the plugin dir yourself | nothing works |
| tmux-auto-name | Go | reuses a PATH binary or downloads a checksum-verified release, falls back to `go build` | no path |
| sesh | Go | not a tpm plugin: brew/`go install`/AUR/nix, user binds it | n/a |
| tmux-sessionx, tmux-fzf-url, tmux-floax, tmux-menus, tmux-tilit, tmux-nerd-font-window-name | bash / POSIX sh | nothing to build; fzf assumed | works |

Pattern: a first-run wizard that curls the GitHub releases API, matches `uname`, falls back to the toolchain, plus a version-drift check so `prefix U` redeploys the binary. All dead offline. Nobody commits binaries to git. tmux-popup-control's tarball is 8.4 MB compressed, 15–25 MB on disk; ours would be 4 MB per platform × Darwin/Linux × amd64/arm64, a goreleaser pipeline, and macOS Gatekeeper on unsigned `go install` builds.

## 5. Popup facts that decide the design (tmux 3.7c man page, issues)

- `display-popup` is a blocking client-side overlay, one per client. Non-blocking popups (PR #4379) were closed 2026-05 "in favour of floating panes", which are a branch, not 3.7. A long-lived dashboard lives in a window/pane, or in a popup running a nested `tmux new-session -A` with `TMUX` unset.
- `-E` closes on exit; a crashing TUI just vanishes. Log to a file.
- Escape and C-c are tmux's own dismiss keys; a TUI that wants Escape must eat it first.
- `TERM` inside is `default-terminal`; truecolor needs `terminal-features` on the outer terminal and `COLORTERM` inside.
- Popups are not panes: `pane-focus-in` never fires for the popup (#2903), and since 3.4 the underlying pane gets focus-out/in when a popup opens/closes (#3991). A cockpit that keys anything off focus hooks sees the agent pane "lose focus" whenever the fleet popup opens.
- Mouse: tmux forwards events to the popup when `mouse on`; `MouseDown1Pane` bindings do not fire inside; the app speaks SGR itself.
- 3.7 fixed "popup window gets overwritten by background updates" (#4920). A hook-driven cockpit repaints panes constantly, so a popup TUI needs 3.7 or it tears.
- Resize: popups adjust to fit since 3.2, the pty gets SIGWINCH.

## 6. When a compiled TUI is the right call

When the screen needs state a fresh process cannot rebuild per keystroke: incremental scrolling of a large table with per-cell styling, in-place editing, drag interactions, several regions updating at different rates, or a control-mode client held open so updates are pushed rather than polled (what agent-manager and agent-deck do for the preview). tmux-popup-control proves Bubble Tea v2 runs fine inside `display-popup` on 3.7. If cockpit goes there, the way that survives is: Go or Ratatui with prebuilt tarballs and a build fallback, tmux ≥ 3.7 required, every action the TUI performs stays a shell script it shells out to (so the bash path stays the source of truth and the 1000-assertion suite keeps running without the binary), and the binary is optional, with the fzf view as the fallback when it is missing.

## 7. Verdict, revised

Not "no". "Not yet, and not for this screen."

The fleet view is rows + preview + a handful of actions. Both spikes render the same rows; the fzf one is 8 lines on a dependency every cockpit user already has, refreshes live through `--listen`, and inherits filtering, multi-select and wheel for free. The Bubble Tea one is 383 lines, a toolchain the owner's own machine lacks, a release matrix, and the popup/attach/colour issue class above, to gain nothing visible today. The distribution tax lands on every user and every release; the gain lands on one screen.

What would flip it: a screen that is not a list. A side-by-side board of N agents each with its own scrolling preview and its own refresh rate; drag-to-reorder or drag-to-move-window; inline reply to a blocked agent's question inside the dashboard (agent view's peek); a control-mode subscription so previews update on `%output` without a fork per second. If the owner wants one of those, the spike is the starting point and the hybrid rule in §6 is the contract. Until then: `tmux-review --fleet` grows the `doing` column, `prefix r` gets `--listen` reload, and `prefix a` gets a `claude agents` row for the Haiku-summary dashboard Anthropic maintains.

Owner decision 6 in the README is now this choice, with the numbers.
