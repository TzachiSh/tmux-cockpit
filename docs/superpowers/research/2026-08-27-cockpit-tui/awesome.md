# awesome-ratatui, mined for cockpit-tui

Source: https://github.com/ratatui/awesome-ratatui (raw README, 563 lines, fetched 2026-08-27).
Metadata: one GitHub GraphQL batch over 87 repos (stars, default-branch tip date, SPDX licence,
root Cargo.toml). LOC = `wc -l` over `*.rs` outside `target/`, only where cloned into
scratchpad/src/. Library versions + dep graphs from the crates.io API.

Headline: 48 of 87 surveyed apps are on ratatui 0.30, 19 still on 0.29, 4 pin exactly (`=0.30.x`),
9 are workspaces. 57 of 87 committed in the last 30 days. The list is alive; the ecosystem has
already moved to 0.30, so the pinned-0.30.2 rule costs nothing in available prior art.

--------------------------------------------------------------------------------
## 1. APPS
--------------------------------------------------------------------------------

Format: name — URL | stars | last commit | licence | LOC | ratatui | what | verdict.
"read" = I read source. Everything else is metadata + README.

### 1a. The six agent ones (read source on all six)

**bosun** — https://github.com/yetidevworks/bosun | 43★ | 2026-08-24 | MIT | 29,322 | "0.30"
tmux-native TUI orchestrating Claude Code / Codex sessions: sidebar of sessions, live preview of
the focused one, per-session state glyphs.
STEAL — the most directly relevant repo in the entire list, and it has already run the experiment
cockpit is about to run.
- `src/ui/preview.rs:1-14` — deliberately paints NO background on the preview area. Claude Code
  emits `Color::Reset` cells; if you paint a panel colour underneath, reset cells show your colour
  and clash with the cells that carry a real background (Claude's status line). Leave the buffer at
  Reset and they fall through to the terminal default. Three lines of insight, one bug avoided.
- `src/tmux/detector/claude.rs:1-48` — state detection from free signals: `#{pane_title}` (Claude
  sets `<braille-spinner> <task title>` while running, `✳` when not) and `#{pane_current_command}`
  (version-shaped `2_1_220` when alive, `zsh` when the agent exited). Both ride the existing
  `list-panes -F` at zero extra fork cost. Pane scraping is the fallback and the ONLY source for
  confirmation prompts. Scraping is confined to `BOTTOM_REGION_LINES = 12` — the single biggest
  reliability win, it stops stale "Thinking…" in scrollback from pinning the state.
- `src/config.rs:169-176` — two-tick design: 1 Hz full refresh (list + detectors + statusbar) and a
  separate 200 ms fast tick that re-captures only the FOCUSED pane. Directly applicable to rule 20's
  "stagger unfocused tiles past ~8".
- `src/ui/embed_terminal.rs` (540 lines) — see the tui-term verdict in part 2. Read the header
  comment before anyone proposes a PTY preview.

**crmux** — https://github.com/maedana/crmux | 27★ | 2026-08-15 | MIT | 9,343 | 0.30 + `crossterm_0_28`
TUI viewer for multiple Claude Code sessions in tmux.
STEAL ONE THING, then note the trap.
- `src/app.rs:262-268` captures with `-p -e -S -` — the WHOLE scrollback, every refresh. The demo's
  `data.rs:423 preview()` uses `-S -{n}` for exactly n lines. crmux is the counter-example that
  proves the bound matters; do not copy it.
- `src/app.rs:275-300 query_tmux_cursor` — worth stealing if the board ever wants a cursor: tmux's
  `#{cursor_x}/#{cursor_y}` are relative to the VISIBLE pane, so against a `-S -`-style capture you
  must convert `content_row = content_lines - pane_height + cursor_y`. One extra `display-message`.
- `src/app.rs:691-701` — poll(50ms) then drain with `poll(Duration::ZERO)`. Same shape as rule 6,
  independent confirmation.
- Cargo.toml uses `ratatui = { version = "0.30", features = ["crossterm_0_28"] }`. Useful fact:
  ratatui 0.30 ships a compat feature for crossterm 0.28. Not needed here (we pin =0.29.0) but it
  means the `=0.29.0` pin will not fight ratatui.

**claudectl** — https://github.com/mercurialsolo/claudectl | 197★ | 2026-07-10 | MIT | 80,515 | "0.29"
Mission control for multiple Claude Code sessions: dashboard, cost tracking, budget enforcement.
SKIP. 80k LOC, a 3-crate workspace, rusqlite, clap+clap_complete+clap_mangen, and an optional MCP
server that pulls tokio. Its own CLAUDE.md declares a "no-async-runtime invariant" and then
relaxes it for the bus feature — the exact drift rule DO-NOT-1 exists to prevent. Nothing to steal
except as a cautionary size: this is what cockpit-tui looks like in two years if the five-dependency
rule lapses. One TestBackend file across 80k lines.

**iris** — https://github.com/itzenata/iris-tui | 6★ | 2026-07-08 | MIT | 3,967 | "0.29"
Live supervisor for active Claude Code sessions — status, tokens, cost, and one-pane approval of
tool calls.
STEAL, and this is the single best find in the list for phase 2.
- `src/bridge.rs:1-60` solves the problem the research flagged as unsolvable. 54/claude-code.md
  notes permission waits never appear in the transcript because the `tool_use` record is written
  only AFTER approval. iris gets them anyway via a supported `PreToolUse` hook where the hook IS
  the same binary (`iris hook`): Claude pipes the tool-call JSON to it, the hook writes a request
  file and block-polls for a decision file that the TUI writes when you press a key, then emits the
  `hookSpecificOutput` decision. No `send-keys`, no stolen focus — compatible with rule DO-NOT-26.
- The three safety valves are the part worth copying verbatim: a `heartbeat` file (stale > 8 s ⇒
  TUI is down ⇒ hook defers instantly to the native prompt rather than hanging the agent); a
  `gating` marker file (absent ⇒ passive, zero added latency on every tool call); and
  `REQUEST_STALE_SECS = 30` kept a hair above `POLL_TIMEOUT = 25 s` so a request whose hook already
  gave up gets GC'd instead of showing a phantom "needs approval" forever.
- `IRIS_BASE_DIR` env override so tests never touch `~/.claude/iris`. Same shape as `COCKPIT_TMUX`.
- Skip its ratatui usage (0.29, `ui.rs` is a plain 979-line list) and its `anthropic.rs` +
  chrono/ureq/dirs deps.

**thurbox** — https://github.com/Thurbeen/thurbox | 48★ | 2026-08-26 | MIT | 112,601 | "0.30"
TUI orchestrator running multiple AI coding agents in persistent tmux sessions.
SKIP the architecture, steal two facts. 112k LOC, mlua with vendored Lua 5.4, notify 8.2, tokio,
tui-term + vt100. It is the maximal version of this idea.
- Worth knowing: it uses `tui-term` + `vt100` for terminal emulation but ALSO keeps
  `capture-pane -p -J -t <target> -S <start>` (`src/agent/tmux.rs:1559`) — nobody replaces
  capture-pane wholesale, even with the full PTY stack in the tree.
- `src/agent/tmux.rs:155` documents a real trap: a session-name prefix collision (`tb-foo` vs
  `tb-foo-bar`) makes `send-keys`/`capture-pane` fail. Cockpit targets pane ids (`%N`), which is
  immune — good confirmation the pane-id choice was right.
- 22 files use TestBackend, the heaviest golden-test practice in the survey. Precedent for rule 41.

**agent-console** — https://github.com/buhuipao/agent-console | 16★ | 2026-08-19 | Apache-2.0 | 19,967 | "0.30"
Local dashboard for Codex and Claude Code.
STEAL ONE THING. It discovers sessions with NO `claude agents --json` fork at all — it reads the
transcript tree directly (`src/discovery.rs:18-24`): `MAX_PROVIDER_FILES = 60`, `HEAD_BYTES = 64K`,
`TAIL_BYTES = 128K`, `RECENT_SESSION_SECONDS = 7 days`. The cache key is
`FileFingerprint { modified_nanos: u128, len: u64 }` — mtime AND length. That beats rule 19's
mtime-only gate: two writes inside one mtime granule change `len` and are caught. One-word change,
strictly better. The head-bytes read is how it gets session metadata (cwd, name) from line 1 without
parsing the whole file.
SKIP its preview path: `src/pty.rs` is 9,506 lines of portable-pty + vt100 (`LocalTerminal::spawn`,
one PTY per session, reader thread each). It also spawns the agents itself, which cockpit does not.

### 1b. Other AI-agent / Claude / Codex / LLM apps

- **ilmari** — https://github.com/bnomei/ilmari | 17★ | 2026-08-08 | MIT | 23,671 | 0.30 (optional dep)
  "Minimal tmux popup radar to track your agents." Closest concept-match to cockpit-tui in the whole
  list. READ: it is not minimal — 23k Rust plus a 436-line `ilmari.tmux`. But its daemon writes
  per-pane and global tmux options (`@ilmari_state`, `@ilmari_badge`, `@ilmari_attention` per pane;
  `@ilmari_window_badges`, `@ilmari_running_count`, `@ilmari_waiting_count`, `@ilmari_attention_count`
  globally) and unsets them with `set-option -pqu` on cleanup. That is independent confirmation of
  cockpit's freeze/precompute pattern (`@cockpit_win_state`) and of rule 11's unpublish-on-every-exit.
  STEAL the option-shape confirmation. SKIP the 436-line entrypoint: it re-parses `$TMUX` into
  socket/pid/client, verifies server generation via `#{socket_path}` + `#{pid}`, and hand-rolls two
  levels of tmux command-list quoting. The one idea worth two lines is pinning every tmux call to
  the socket parsed from `$TMUX`; cockpit already gets that for free.
- **trex** — https://github.com/blackopsrepl/trex | 13★ | 2026-08-06 | ISC | 5,958 | "0.29"
  tmux session manager, fuzzy find, per-session stats, AI agent tracking. SKIP — it is the fzf
  fleet list rebuilt as a Table, i.e. exactly rule DO-NOT-6. Useful only as evidence for that rule.
- **p2pmux** — https://github.com/pelazas/p2pmux | 11★ | 2026-08-26 | MIT | 69,781 | 0.30 no-default
  P2P multiplexer, every pane a PTY on its owner's machine, with an inbox of agents across all of
  them. SKIP — 69k LOC and a network stack for a problem cockpit does not have. The "inbox of
  coding agents" framing matches phase 2's triage overlay; nothing else transfers.
- **Reeve** — https://github.com/Dancode-188/reeve | 7★ | 2026-08-25 | (no SPDX) | 29,551 | "0.29"
  "A terminal cockpit for AI agents: watch a run live, score it, step in when it goes sideways."
  Name collision, concept overlap. SKIP — 29k LOC, unlicensed (NOASSERTION means GitHub could not
  identify a licence: do not copy code from it), still on 0.29.
- **amtr / anthropometer** — https://github.com/arian-shamaei/anthropometer | 26★ | 2026-08-25 | MIT | 15,595
  btop-style render of a Claude Code session's live context window as a memory map, plus cache
  economics and a PDF report. SKIP for phase 1-2. Interesting only if cockpit ever wants a
  context-pressure glyph per tile; the transcript fields it reads would be the thing to look at then.
- **claudectl / iris / bosun / crmux / thurbox / agent-console** — see 1a.
- **agx** — https://github.com/brevity1swos/agx | 16★ | 2026-06-25 | no licence | 0.30 no-default
  Step-through debugger for AI agent execution traces. SKIP — unlicensed, and post-hoc trace
  replay is the opposite of a live board.
- **BitFun** — https://github.com/GCWing/BitFun | 1,817★ | 2026-08-26 | MIT | "0.29"
  An AI coding agent (not an observer). SKIP — wrong side of the boundary; cockpit renders and
  decides, never acts.
- **Martty** — https://github.com/openma-ai/Martty | 61★ | 2026-08-26 | MIT | "0.30.2"
  ratatui client for ACP-compatible coding agents, plugins, subagents, durable sessions. SKIP —
  it IS the agent client. Same 0.30.2 pin as the rules, minor precedent.
- **VT Code** — https://github.com/vinhnx/vtcode | 824★ | 2026-08-25 | Apache-2.0 | `=0.30.2`
  Semantic coding agent. SKIP as an app; NOTE the exact pin `=0.30.2` with `default-features = false`
  — precedent for rule 3's pinning style applied to ratatui itself.
- **Forge** — https://github.com/NorviaLabs/forge | 5★ | 2026-08-26 | MIT | `=0.30.2` no-default
  Agent + editor + shell in one keyboard-driven workspace. SKIP. Second exact-pin precedent.
- **Yardlet** — https://github.com/zzunkie/yardlet | 17★ | 2026-08-11 | MIT | "0.29"
  Local AI workbench driving installed Claude Code / Codex CLIs as interchangeable workers.
  SKIP — it drives agents; cockpit observes them.
- **opencrabs** — https://github.com/adolfousier/opencrabs | 910★ | 2026-08-26 | MIT | 0.30 `all-widgets`
  Orchestration layer for software development. SKIP — `features = ["all-widgets"]` is the
  anti-pattern (pull everything, use three).
- **SynapsCLI** — https://github.com/HaseebKhalid1507/SynapsCLI | 39★ | 2026-08-20 | Apache-2.0 | "0.30"
  "15MB, 2ms boot" agent harness. SKIP the app. The 15 MB claim is worth contrasting with the
  demo's 785 KB at `opt-level="s"` + lto + strip — the rules' release profile is 20× better and the
  README is the reason to keep it.
- **toktop** — https://github.com/htin1/toktop | 172★ | 2025-12-07 | MIT | "0.27"
  LLM usage monitor. SKIP — 8 months stale, ratatui 0.27.
- **opencode stats** — https://github.com/Cateds/opencode-stats | 62★ | 2026-08-04 | MIT | "0.30.0"
  Usage/cost dashboard for OpenCode. SKIP — cost accounting, not a board.
- **llmtrim** — https://github.com/fkiene/llmtrim | 219★ | 2026-08-17 | MPL-2.0 | workspace
  Local proxy compressing LLM requests, with a tabbed per-source cost dashboard. SKIP.
- **models** — https://github.com/arimxyer/models (now reyamira/models) | 500★ | 2026-08-26 | MIT | 0.30
  Browse AI models, benchmarks, coding agents. Uses `unstable-rendered-line-info`. SKIP.
- **Oatmeal** — https://github.com/dustinblackman/oatmeal | 771★ | 2024-06-01 | MIT | `=0.24.0`
  Chat with LLMs from the terminal. SKIP — unmaintained two years, ratatui 0.24.
- **tenere** — https://github.com/pythops/tenere | 680★ | 2026-05-10 | GPL-3.0 | "0.30"
  TUI for LLMs. SKIP — GPL, and it is a chat client.
- **commandOK** — https://github.com/64bit/commandOK | 44★ | 2026-06-16 | MIT | "0.30"
  Spotlight-like LLM command generator. SKIP as an app, but it is the nearest thing in the list to
  a "popup, one line of input, one answer" shape — i.e. the demo's `ask.rs`. Worth a glance only if
  `ask.rs` grows a history.
- **Stakpak** — https://github.com/stakpak/agent | 1,756★ | 2026-07-06 | Apache-2.0 | "0.29.0"
  AI DevOps agent. SKIP.
- **pixtuoid** — https://github.com/IvanWng97/pixtuoid | 455★ | 2026-08-26 | MIT | "0.30"
  Live pixel-art office for AI coding agents. SKIP (it is a toy), but it is proof the "one visual
  cell per agent, updated live" idea has an audience — 455★ in weeks.
- **gwm** — https://github.com/kbrdn1/gwm-cli | 133★ | 2026-08-15 | Apache-2.0 | "0.30"
  Git worktree manager, CLI+TUI in one binary, native libgit2, AI agent session tracking. SKIP —
  libgit2 is exactly the chain rule DO-NOT-8 names (claude-tmux's git2 → libssh2 → openssl).
  Relevant only as the worktree-management overlap with issue #49.
- **gmsg** — https://github.com/olorikendrick/gmsg | 12★ | 2026-07-27 | MIT | "0.30.0" — AI commit
  messages. SKIP.
- **nereid** — https://github.com/bnomei/nereid | 63★ | 2026-07-11 | (no SPDX) | 0.30 no-default —
  Mermaid diagrams with AI agents, TUI + MCP server. SKIP.
- **raymon** — https://github.com/bnomei/raymon | 18★ | 2026-07-01 | MIT | "0.30" — Ray logging TUI
  + MCP server. SKIP.
- **codemark** — https://github.com/DanielCardonaRojas/codemark | 122★ | 2026-08-19 | MIT | workspace
  Semantic code bookmarking for humans and agents. SKIP.
- **ygrep** — https://github.com/yetidevworks/ygrep | 57★ | 2026-07-30 | MIT | "0.29" — Tantivy-backed
  code search optimised for AI assistants. SKIP.

### 1c. tmux-related (non-agent)

- **mprocs / dekit** — https://github.com/pvolok/mprocs (repo renamed to pvolok/dekit) | 2,702★ |
  2026-08-25 | MIT | 27,345 | workspace
  Run multiple commands in parallel, output of each shown separately. The closest thing in the list
  to a multi-pane live viewer. SKIP the code (27k LOC, workspace, its own vt100 fork, phf,
  serde_yaml, flexi_logger). STEAL its release profile — `strip = "symbols"`, `opt-level = "z"`,
  `lto`, `panic = "abort"`, `codegen-units = 1`: identical to rule 5 except `z` vs `s`. Independent
  arrival at the same profile. Note the repo rename — any link in the plan should use the new name.
- **Gitside** — https://github.com/dev-bhaskar8/gitside | 20★ | 2026-08-03 | MIT | "0.29"
  Git TUI explicitly designed for "full terminals and narrow tmux panes". SKIP the app; the narrow-
  pane responsiveness framing is what rule 32 already covers (refuse below 40 columns).

### 1d. Process / system / container / k8s dashboards

Ordered by what they can teach, not by stars.

- **bottom** — https://github.com/ClementTsang/bottom | 13,941★ | 2026-08-26 | MIT | 38,936 | 0.30.2 no-default
  Cross-platform process/system monitor. The reference implementation of a configurable widget grid.
  READ `src/options/config/layout.rs:58-99`: rows carry a `ratio`, widgets inside a row carry a
  `ratio`, and it accumulates `total_height_ratio` / `total_col_ratio`. That is precisely what the
  demo's `board.rs:44-47` already does with `Constraint::Ratio(1, rows)` × `Constraint::Ratio(1, cols)`.
  VERDICT: already covered — bottom is confirmation the demo's 4-line grid is the right shape, and
  the reason not to reach for a tiles/grid crate. Also note: `default-features = false` with an
  explicit feature list, and 13.9k stars with zero TestBackend files (it uses insta instead).
- **oxker** — https://github.com/mrjackwills/oxker | 1,820★ | 2026-08-22 | MIT | 18,071 | "0.30"
  Docker container TUI. Uses both insta and TestBackend. Worth one look at its test layout if
  rule 41's goldens need a second precedent; otherwise SKIP.
- **ducker** — https://github.com/robertpsoane/ducker | 919★ | 2026-08-01 | MIT | 0.30.0 — k9s-inspired
  Docker TUI. SKIP.
- **stevedore** — https://github.com/takumiymd/stevedore | 11★ | 2026-07-31 | MIT | "0.29" — Docker
  and Compose TUI. SKIP.
- **kdash** — https://github.com/kdash-rs/kdash | 2,527★ | 2026-08-15 | MIT | 0.30 no-default — k8s
  dashboard. SKIP.
- **kubetui** — https://github.com/sarub0b0/kubetui | 393★ | 2026-08-22 | MIT | 0.30.0 — real-time
  k8s resource monitor. SKIP.
- **b4n** — https://github.com/fioletoven/b4n | 103★ | 2026-08-25 | MIT | 0.30 — k8s resource browser.
  SKIP.
- **kftui** — https://github.com/hcavarsan/kftray | 1,554★ | 2026-05-13 | GPL-3.0 | 0.30.0 with
  `unstable-widget-ref` — manages many `kubectl port-forward` processes. SKIP (GPL).
- **bpftop** — https://github.com/Netflix/bpftop (redirects to jfernandez/bpftop) | 2,702★ |
  2026-07-29 | Apache-2.0 | 0.30.0 no-default — live view of running eBPF programs. SKIP.
- **zenith** — https://github.com/bvaisvil/zenith | 3,044★ | 2026-08-25 | MIT | `0.29.*` — system
  monitor. SKIP (wildcard version requirement is the anti-pattern rule 3 forbids).
- **ytop** — https://github.com/cjbassi/ytop | 2,160★ | 2020-08-29 | MIT | pre-ratatui (tui-rs)
  SKIP — dead six years. Listed here only so nobody re-discovers it.
- **macmon** — https://github.com/vladkens/macmon | 1,834★ | 2026-08-04 | MIT | 0.30.2 optional dep
  Sudoless Apple Silicon monitoring. NOTE only: `ratatui = { ..., optional = true }` behind a feature
  so the CLI/JSON path builds without the TUI. That is the same separation rule 43 wants between
  `make check` and `make tui`, expressed in Cargo instead of Make. Not needed — cockpit's binary is
  optional at the plugin level, not the feature level.
- **pumas** — https://github.com/graelo/pumas | 219★ | 2026-08-20 | MIT | 5,517 | not in root Cargo.toml
  Apple Silicon power monitor. SKIP.
- **mxmon** — https://github.com/yusufmo1/mxmon | 4★ | 2026-07-24 | MIT | "0.30" — Apple Silicon
  monitor "with a JSON contract for scripts and agents". SKIP the app; the JSON-contract framing
  matches `cockpit-tui --json` (rules phase-1 gate). No code worth taking at 4 stars.
- **systemctl-tui** — https://github.com/rgwood/systemctl-tui | 2,039★ | 2026-07-27 | MIT | workspace
  SKIP.
- **systemd-manager-tui** — https://github.com/matheus-git/systemd-manager-tui | 1,573★ | 2026-08-03 |
  MIT | "0.29" — SKIP.
- **glim** — https://github.com/junkdog/glim | 234★ | 2025-09-07 | MIT | 0.29.0 `unstable-widget-ref`
  GitLab CI/CD pipeline monitor, by the tachyonfx author. SKIP — 11 months stale, and its whole
  differentiator is animation effects, which a 1 Hz hash-and-skip board deliberately does not have.
- **drydock** — https://github.com/yetidevworks/drydock | 244★ | 2026-08-26 | MIT | workspace
  Live dashboard for a fleet of git repos: uncommitted, unpushed, unreleased across all of them.
  CLOSEST NON-AGENT ANALOGUE to the board — N repos, N status tiles, one refresh. Same author as
  bosun. Not cloned; worth a second lane's read if the board's per-tile status line needs a second
  opinion. Otherwise SKIP.
- **tuistash** — https://github.com/edmocosta/tuistash | 109★ | 2026-04-10 | Apache-2.0 | 0.30.0 with
  `unstable-rendered-line-info` — Logstash monitor. SKIP.
- **pgmon** — https://github.com/nbari/pgmon | 14★ | 2026-07-01 | BSD-3-Clause | "0.30" — SKIP.
- **rrtop** — https://github.com/wojciech-zurek/rrtop | 43★ | 2021-08-05 | MIT — dead five years. SKIP.
- **oha** — https://github.com/hatoo/oha | 10,510★ | 2026-08-23 | MIT | 0.30.0 no-default — top-like
  HTTP load monitor. SKIP (it is a load generator with a live view).
- **erldash** — https://github.com/sile/erldash | 176★ | 2026-05-21 | (no SPDX) | "0.30" — SKIP.
- **mirador** — https://github.com/jchultarsky/mirador | 292★ | 2026-08-26 | MIT | "0.30.2" — personal
  dashboard: clocks, calendar, weather, tasks, notes, market, CPU/network graphs. SKIP — it is a
  widget zoo, the opposite of one screen done well.
- **qmassa** — https://github.com/ulissesf/qmassa | 107★ | 2026-07-25 | Apache-2.0 | workspace — GPU
  usage. SKIP.
- **tegratop** — https://github.com/pythops/tegratop | 85★ | 2025-12-14 | GPL-3.0 | "0.29" — SKIP.
- **unifly** — https://github.com/hyperb1iss/unifly | 248★ | 2026-08-07 | Apache-2.0 | 0.30 — UniFi
  controller, 8-screen dashboard. SKIP.
- **lazyslurm** — https://github.com/hill/lazyslurm | 77★ | 2026-08-09 | MIT | "0.28" — SKIP.
- **Slurmer** — https://github.com/wjwei-handsome/Slurmer | 110★ | 2025-07-14 | MIT | "0.29.0" — SKIP.
- **gentooplz** — https://github.com/JustRoccat/gentooplz | 10★ | 2026-08-03 | MIT | "0.30" — live
  dashboard of what Portage is building. SKIP.
- **winproc-tui** — https://github.com/TX230/winproc-tui | 110★ | 2026-08-26 | MIT | "0.30.2" — SKIP
  (Windows).

### 1e. Multi-pane live viewers

- **mprocs / dekit** — see 1c. The only serious one.
- **hwatch** — https://github.com/blacknon/hwatch | 1,073★ | 2026-06-20 | MIT | `>=0.29, <0.31`
  `watch(1)` with history and diffs. NOTE ONLY: the diff-between-successive-captures idea is the
  conceptual cousin of rule 7's hash-and-skip — hwatch diffs to SHOW change, cockpit hashes to
  SKIP drawing. Its version range `>=0.29, <0.31` is the pragmatic middle between a caret and an
  exact pin; the rules chose the exact pin for crossterm specifically (#1104), not for ratatui, so
  there is no conflict. SKIP the code.
- **logss** — https://github.com/todoesverso/logss | 297★ | 2025-10-15 | MIT | "0.29.0" — splits one
  log stream into containers. SKIP (10 months stale).
- **Livediff** — https://github.com/SoCkEt7/Livediff | 229★ | 2026-07-28 | Apache-2.0 | "0.30.2" —
  real-time file diff monitor. SKIP.
- **splashboard** — https://github.com/unhappychoice/splashboard | 214★ | 2026-08-24 | ISC | 0.30 with
  `widget-calendar` — a customisable splash rendered on shell startup / directory change. SKIP the
  app; the "render once into the terminal and exit" mode is a different shape from a popup loop.
- **traceview** — https://github.com/javaLux/traceview | 8★ | 2026-08-26 | MIT | "0.30.2" — SKIP.

### 1f. Popup-style pickers

The rules already settled this: fzf keeps the fleet list (DO-NOT-6). These are listed to close the
question, not to reopen it.

- **television** — https://github.com/alexpasmantier/television | 6,208★ | 2026-08-16 | MIT | 29,656 |
  0.30 `serde,macros`
  General-purpose fuzzy finder, the ratatui answer to fzf. VERDICT: the strongest possible argument
  FOR rule DO-NOT-6, not against it. 29,656 lines of Rust to reach parity with what fzf already
  gives cockpit for free — fuzzy filter, multi-select, wheel, preview scrolling
  (`television/action.rs:60-67`). If a compiled fuzzy list ever became a requirement, this is the
  crate to depend on rather than rebuild; it is not a requirement.
- **matchmaker** — https://github.com/Squirreljetpack/matchmaker | 238★ | 2026-08-24 | NOASSERTION |
  workspace — "Fuzzy picker (FZF reboot)". SKIP: unclear licence.
- **fsel** — https://github.com/Mjoyufull/fsel | 414★ | 2026-08-21 | BSD-2-Clause | 0.30 no-default —
  app launcher + fuzzy finder. SKIP.
- **fzf-make** — https://github.com/kyu08/fzf-make | 292★ | 2026-08-11 | MIT | "0.29.0" — picks a make
  target with a preview window. SKIP.
- **cargo-selector** — https://github.com/lusingander/cargo-selector | 73★ | 2026-07-12 | MIT | 0.30.2
  `serde` — smallest well-formed "list + pick + exit with a choice" binary in the list. NOTE: it
  returns its selection the same way cockpit must not (stdout). Cross-check against rule 24 if
  anyone proposes `$(cockpit-tui …)` again.
- **igrep** — https://github.com/konradsz/igrep | 842★ | 2026-02-01 | MIT | 0.26.2 — SKIP (stale).
- **get_blessed_rs** — https://github.com/josueBarretogit/get_blessed_rs | 50★ | 2024-06-29 | MIT |
  0.26.3 — SKIP (two years stale).
- **ssh-list** — https://github.com/akinoiro/ssh-list | 221★ | 2025-10-27 | GPL-3.0 | "0.29.0" — SKIP.
- **purple** — https://github.com/erickochen/purple | 656★ | 2026-08-26 | MIT | "0.30" — SSH config
  manager and launcher. SKIP.
- **scooter** — https://github.com/thomasschafer/scooter | 1,291★ | 2026-08-15 | MIT | workspace —
  interactive find and replace. SKIP.
- **repgrep** — https://github.com/acheronfail/repgrep | 525★ | 2025-05-06 | Apache-2.0 | 0.22.0 — SKIP.
- **serpl** — https://github.com/yassinebridi/serpl | 856★ | 2026-05-31 | MIT | 0.29.0 — SKIP.
- **ATAC** — https://github.com/Julien-cpsn/ATAC | 3,704★ | 2026-08-23 | MIT | `=0.30.0` `serde` —
  API client. NOTE ONLY: third exact-pin precedent, `=0.30.0`, on a 3.7k-star project.
- **slumber** — https://github.com/LucasPickering/slumber | 1,219★ | 2026-07-04 | MIT | workspace — SKIP.
- **rainfrog** — https://github.com/achristmascarl/rainfrog | 5,293★ | 2026-08-25 | MIT | 0.30.2
  `serde,macros` — Postgres TUI. SKIP; listed because it is the highest-starred project on the exact
  ratatui version the rules pin (0.30.2), i.e. that pin is well-trodden.

--------------------------------------------------------------------------------
## 2. LIBRARIES
--------------------------------------------------------------------------------

Compatibility rule of thumb: ratatui 0.30 re-exports `ratatui-core 0.1` and `ratatui-widgets 0.3`.
A crate depending on `ratatui-core ^0.1` / `ratatui-widgets ^0.3` IS 0.30-compatible. A crate
depending on `ratatui ^0.29` or lower is NOT, without a fork.

The five-dependency budget is currently spent: ratatui 0.30.2, crossterm =0.29.0, ansi-to-tui 8,
serde_json, libc. Every "adopt" below is a proposal to make it six. The demo is 1,982 lines across
9 files and ships every screen on the five. That is the number each verdict is measured against.

### The one the lead asked about first

**tui-term** — https://crates.io/crates/tui-term | 0.3.4 | 2026-04-07 | ratatui-core ^0.1 +
ratatui-widgets ^0.3 (compatible), **plus optional `vt100 ^0.16.2` and `portable-pty ^0.9`**
A pseudoterminal widget: `PseudoTerminal::new(screen)` where `screen` is a `&vt100::Screen`.

**Would it replace `capture-pane` + `ansi-to-tui`? No. VERDICT: SKIP, hard, and this is settled
by prior art rather than by opinion.**

What it actually is: a renderer for a vt100 screen. It does not talk to tmux and has no way to.
To point it at an existing tmux pane you must become a tmux client — `tmux attach` inside a PTY you
own — because a pane's PTY belongs to the tmux server, not to you. The costs, all evidenced:

1. Dependencies go 5 → 8 (tui-term + vt100 + portable-pty), plus their trees. That is not one
   measurement away from the budget; it is a different project.
2. One PTY, one reader thread, and one `tmux attach` client PER TILE. The board is N tiles.
   bosun — which shipped this — runs it for the FOCUSED session only and keeps the polled
   `capture-pane` path for every other preview, gated behind `Config::embed_enabled`
   (`src/config.rs:178-186`, `src/ui/embed_terminal.rs:1-9`). Nobody runs N of them.
3. It writes to the live tmux server. An attached client participates in window-size negotiation,
   so previewing resizes the user's session. bosun documents the full scar tissue at
   `src/ui/embed_terminal.rs:50-79`: it tried `-r` (read-only + ignore-size) plus a forced
   `resize-window`, which silently set `window-size=manual` and left the user's later full-screen
   attach clipped. This violates rule 46 and cannot honour `COCKPIT_READONLY` at all.
4. tmux 3.7 — the version rule 35 requires — made `send-keys` refuse a session whose resolved
   client is read-only, so the read-only flag that would have made this safe is unusable
   (`embed_terminal.rs:88-104`). bosun now attaches read-write and enforces read-only in
   application code. Cockpit's read-only guarantee would become a convention, not a mechanism.
5. It buys nothing for a preview. `ansi-to-tui` already turns `capture-pane -ep` output into
   styled `Text`. vt100 adds cursor position, scroll regions, and mouse modes — all meaningless in
   a 6-line snapshot, and mouse is banned in phase 1 anyway.
6. Cost comparison: `capture-pane -ep` is ~4.5 ms per visible tile (rule 20), and 30,000 calls left
   the tmux server RSS flat. A PTY per tile has no such measurement and adds N threads.

Independent confirmation: thurbox has the entire tui-term + vt100 stack in its tree and STILL calls
`capture-pane -p -J` for snapshots (`src/agent/tmux.rs:1559`). agent-console pays 9,506 lines in
`src/pty.rs` for the PTY path — but it SPAWNS the agents itself, so it owns the PTYs already.
Cockpit does not and never will.
Revisit only if phase 3 wants a single focused tile that accepts keystrokes. Then copy bosun's
design exactly: one embed for the focused tile, snapshots for the rest, behind an off-by-default flag.

### Widgets

| crate | latest | ratatui 0.30? | verdict |
|---|---|---|---|
| tui-popup | 0.7.6 (2026-06-14) | YES (core ^0.1, widgets ^0.3) | SKIP |
| tui-scrollview | 0.6.7 (2026-06-14) | YES | SKIP (phase 3 at the earliest) |
| tui-widget-list | 0.15.3 (2026-07-18) | YES | SKIP |
| tui-textarea | 0.7.0 (2024-10-22) | **NO** — ratatui ^0.29, crossterm ^0.28 | SKIP (incompatible + 22 months stale) |
| ratatui-textarea | 0.9.2 (2026-06-12) | YES (core ^0.1.1, widgets ^0.3.1) | SKIP (the maintained fork; still overkill) |
| throbber-widgets-tui | 0.11.1 (2026-06-19) | YES (ratatui ^0.30) | SKIP |
| tui-logger | 0.18.3 (2026-07-04) | YES (ratatui ^0.30) | SKIP |
| tui-prompts | 0.6.7 (2026-06-18) | YES | SKIP |
| rat-widget | 3.2.1 (2026-03-09) | YES | SKIP, emphatically |
| tui-tree-widget | 0.24.1 (2026-08-09) | YES | SKIP |
| edtui | 0.11.7 (2026-08-16) | YES | SKIP |
| tui-big-text | 0.8.9 (2026-08-24) | YES | SKIP |
| tui-menu | 0.3.1 (2025-12-30) | YES | SKIP |
| tui-overlay | 0.1.2 (2026-04-07) | YES | SKIP |
| tui-tabs | 0.1.1 (2026-03-23) | YES | SKIP |
| tui-skeleton | 0.3.0 (2026-03-31) | YES | SKIP |
| tui-dialog | 0.5.0 (2026-01-17) | YES | SKIP |
| tui-checkbox / tui-slider / tui-piechart / tui-shimmer / tui-globe / tui-rain / tui-nodes / tui-additions | — | mostly YES | SKIP (no use case) |
| ratatui-image / ratatui-splash-screen / ratatui-explorer / ratatui-code-editor / ratatui-markdown / malevich / hyperrat / ratatui-wireframe / ratatui-fretboard / ratatui-stacked-bar / ratatui-comfy-* / ratatui-toaster / ratatui-cheese / term-rustdoc | — | mixed | SKIP (no use case) |

The reasoning behind the four the lead named specifically:

**tui-popup** — https://crates.io/crates/tui-popup | 0.7.6 | compatible | 464k downloads.
What it does: centre a widget over the frame, `Clear` behind it, optional mouse drag to move.
Lines it would remove: roughly 6 — a centred `Rect` computation and a `Clear`. The demo does not
even have those yet; `ask.rs:80` renders a `Block::bordered()` into the popup's own full area,
because the popup IS the window (rule 34: `display-popup -E`). Its headline feature, drag-to-move,
needs mouse capture, which rule DO-NOT-5 forbids in phase 1 (SGR capture inside a popup steals the
wheel and text selection). Risk: a 6th dependency for 6 lines of code we have not written.
VERDICT: SKIP. Reconsider only if phase 2's reply overlay must float above the board rather than
inside the tile — and even then, `Clear` + one `Rect` helper is cheaper.

**tui-scrollview** — https://crates.io/crates/tui-scrollview | 0.6.7 | compatible | 429k downloads.
Scrolls a viewport over a larger buffer. Phase-1 previews do not scroll: `data.rs:423 preview()`
asks tmux for exactly `n` lines with `-S -{n}` and renders them, and rule 29 forbids `Wrap`, so
there is no overflow to scroll. VERDICT: SKIP now; the honest phase-3 candidate if a tile ever gets
"expand and scroll the last 200 lines". It would then remove maybe 40 lines of offset arithmetic.

**tui-widget-list** — https://crates.io/crates/tui-widget-list | 0.15.3 | compatible.
Selectable list of arbitrary widgets with per-item heights. This is the fleet list, and rule
DO-NOT-6 already retired that question: 1,770 lines of Rust bought j/k/enter, while fzf gives fuzzy
filter, multi-select and the wheel for free. VERDICT: SKIP. The board is a fixed grid of N tiles
with `Tab` cycling (`board.rs:139-152`), not a scrolling list — there is no list to hold.

**tui-input** — https://crates.io/crates/tui-input | 0.15.4 | 2026-08-10 | YES (ratatui ^0.30.2 and
crossterm ^0.29, both optional) | 1.9M downloads. The best-maintained option here.
Lines it would remove: **24** — `ask.rs:136-159 fn edit()`, which handles Char/Backspace/Delete/
Left/Right/Home/End with correct char→byte offset arithmetic for multibyte input. The demo's own
comment already reached this verdict: *"A line editor in twenty lines. tui-input 0.15 does support
ratatui 0.30, but one prompt does not earn a dependency."*
Risk it adds: a 6th dependency, plus two optional feature paths (crossterm 0.29 must be the same
crossterm we pin — it is), for 24 lines that are already written, already tested, and cannot
regress.
VERDICT: SKIP for phase 2 as specified (one inline reply line). It is the ONLY crate in this survey
with a real, countable line-removal case, so it is the one to name in the ponytail audit table as
"considered, rejected, here is the number". Adopt only if phase 2's reply line grows word-wise
kill/yank, history, or multi-line — at which point the 24 lines become 120 and the trade flips.

**rat-widget** — https://crates.io/crates/rat-widget | 3.2.1 | compatible | **17 non-optional
dependencies**. A whole framework: text/date/number input, textarea, checkbox, choice, radio,
slider, calendar, split, tabbed, multi-page, big-data table, file dialog, menubar, statusbar, plus
its own crossterm event-handling and focus system. VERDICT: SKIP. Adopting it is adopting an
application architecture; its focus/event model would fight the 77-line sync loop rule 6 specifies.

### Utilities

| crate | latest | ratatui 0.30? | verdict |
|---|---|---|---|
| **ansi-to-tui** | 8.0.1 (2026-01-10) | YES (ratatui-core ^0.1.0) | **ALREADY ADOPTED — keep** |
| ratatui-macros | 0.7.2 (2026-06-19) | YES (core ^0.1.2, widgets ^0.3.2) | SKIP |
| tachyonfx | 0.25.1 (2026-07-05) | YES (core ^0.1.2) | SKIP |
| coolor | 1.1.0 (2025-05-09) | n/a (0 non-optional deps) | SKIP |
| termprofile | 0.2.4 (2026-05-02) | YES (core ^0.1 optional) | SKIP — actively wrong here |
| color-to-tui | 0.3.0 (2023-04-03) | `ratatui 0.*` | SKIP (3 years stale, meaningless req) |
| opaline | 0.4.1 (2026-05-20) | YES (optional) | SKIP |
| terminput | 0.5.15 (2026-07-18) | n/a | SKIP |
| tui-syntax-highlight | 0.2.0 (2025-12-26) | YES | SKIP |
| ratatui-interact | 0.5.3 (2026-04-02) | YES | SKIP (mouse-focused; banned phase 1) |
| ratatui-garnish | 0.1.0 (2025-10-14) | **NO** (ratatui ^0.29.0) | SKIP |
| ratatui-input-manager / tui-pantry / ggsci-ratatui | — | YES | SKIP |

Notes on the two that look tempting:

**ratatui-macros** (0.7.2, 13.3M downloads, compatible) turns
`Layout::vertical([Constraint::Length(1), Constraint::Min(3)])` into `vertical![==1, *=1]`. The demo
has 4 such call sites (`board.rs:27,44,46,79`). Saves under 5 lines total across the codebase, in
exchange for macro indirection in the one file a reviewer reads first. SKIP.

**termprofile** (0.2.4, 0 non-optional deps, ratatui-core optional) detects terminal colour support
and downgrades ratatui colours to match. Superficially it is exactly rule 27. It is not: it detects
from `COLORTERM`/`TERM`, and rule 27 exists precisely because that lies inside a popup
(`TERM=tmux-256color` while tmux may or may not have RGB declared via `terminal-features`). The
demo's `theme.rs:47-60` asks tmux directly — one `display-message -p '#{client_termfeatures}'` plus
one `show-options -g` for the `@thm_*` roles — which is the correct source of truth and already
written in 127 lines. Adopting termprofile would replace a right answer with a wrong one. SKIP.

### Frameworks

| crate | latest | ratatui 0.30? | verdict |
|---|---|---|---|
| tui-realm (tuirealm) | 4.1.0 (2026-05-02) | YES (ratatui ^0.30) | SKIP |
| rat-salsa | 4.0.3 (2026-03-08) | YES | SKIP |
| ratatui-kit | 0.10.3 (2026-07-16) | YES | SKIP |
| tui-react | 0.24.0 (2026-01-08) | YES | SKIP |
| widgetui | 0.7.1 (2024-10-27) | **NO** (ratatui ^0.29, crossterm ^0.28) | SKIP |
| schemaui | 0.12.4 (2026-08-13) | YES (optional) | SKIP |
| ratzilla / egui-ratatui / mousefood / soft_ratatui / bevy_ratatui* / ratatui-uefi / crepuscularity | — | — | SKIP (alternate backends; irrelevant to a tmux popup) |

Every one of these is an Elm/React/component architecture over a 1,982-line program with one input
source and one timer. rat-salsa's pitch — "event-queue with tasks, timers, application events,
focus handling, dialog windows" — is a description of tokio-shaped complexity arriving through a
different door. SKIP all.

### tmux crates

**tmux_interface** — https://crates.io/crates/tmux_interface | 0.4.0 (2026-03-10) | 151k downloads |
0 non-optional dependencies. **Not in awesome-ratatui at all** (the list has no tmux section; I
checked it because the lead named it).
A typed builder API over the tmux CLI: `NewSession::new().detached().session_name(..)`.
VERDICT: SKIP, and the reasons are structural, not stylistic:
- Rule 15 requires the `list-panes -F` format copied VERBATIM from `scripts/tmux-review:1706-1723`,
  as one `-F` argument. A builder API's value is constructing that string for you; here that is the
  one thing that must not happen.
- Rule 26 requires `Command::new("tmux")` resolved through PATH so `tests/run.sh:26-49` can put a
  shim at `$SHIM/tmux` and drive the binary with zero harness changes. A wrapper crate that
  resolves tmux differently breaks the entire bash test strategy.
- The demo's whole tmux layer is `data.rs fn tmux()` — a `Command::new`, args, `output()`. There is
  nothing to remove.
**tmux-lib** — https://crates.io/crates/tmux-lib | 0.5.0 (2026-04-18) | 6.4k downloads | SKIP, same
reasons, less adoption.

Also checked: **unicode-width** 0.2.2 (759M downloads, 0 deps). Not a 6th dependency candidate —
rule 28 exists because unicode-width and TestBackend BOTH say `▲ ● ✓ ◆` are 1 column while an
emoji-preferring font draws 2. Adding unicode-width would formalise the wrong answer; the 2-column
glyph gutter is the fix. Noted because crmux, thurbox and agent-console all pull it and all still
have width bugs to work around.

--------------------------------------------------------------------------------
## 3. TEMPLATES / FRAMEWORKS / TUTORIALS
--------------------------------------------------------------------------------

Note on scope: the current awesome-ratatui README has no Templates or Tutorials section — only
Libraries (Frameworks / Widgets / Utilities / Bindings) and Apps. The templates below are from
https://github.com/ratatui/templates, cloned at scratchpad/src/templates; the frameworks are the
awesome list's Frameworks section, already covered in part 2.

- **hello-world** (20 lines, `template/src/main.rs`) — ALREADY COVERED. `ratatui::init()` /
  `ratatui::restore()`, which the demo uses and rule 5 depends on for the panic hook.
- **simple** (91 lines) — ALREADY COVERED. `while running { terminal.draw(...); handle_events(); }`.
  One thing to note against it: it draws unconditionally every loop. The demo's hash-and-skip
  (rule 7) is a deliberate departure, and the template is the thing being departed from.
- **simple-async** (93 lines) — teaches nothing the project may use. It is the tokio on-ramp
  DO-NOT-1 forbids.
- **event-driven** (272 lines across main/app/event/ui) — teaches ONE thing the demo lacks: a typed
  event enum with a channel, so timer ticks and key events arrive through one `Event` type instead
  of being interleaved by hand at the top of the loop (`board.rs:112-130` does the interleaving
  inline). VERDICT: not worth adopting. The demo has exactly two sources (a 1 Hz tick and
  `event::poll`), and rule 6 mandates the sync shape. Worth reading once for the enum, then closing.
  Also the number to quote: 272 lines is the ENTIRE template — cite it when someone proposes a
  workspace (DO-NOT-7).
- **event-driven-async** (272 lines) — same, on tokio. Skip.
- **component** (1,479 lines: config.rs 597, tui.rs 234, app.rs 179, components.rs 124, errors.rs 77,
  logging.rs 36) — teaches TWO things the demo lacks, one of them genuinely:
  1. `errors.rs` (77 lines): panic + error hooks that restore the terminal before printing, via
     `better-panic` + `human-panic` + `color-eyre`. GENUINELY MISSING — a crash inside
     `display-popup -E` currently vanishes (rule 33 mitigates with a log file, it does not restore
     the terminal). But the template's answer costs three dependencies that DO-NOT-8 forbids.
     `ratatui::init()` already installs a hook that restores the terminal before the abort
     (rule 5 / rules "Corrections" 5), so the gap is narrower than it looks: what is missing is only
     writing the panic message to `$COCKPIT_LOG` on the way out. That is ~5 lines of
     `std::panic::set_hook` wrapping the existing one. TAKE THE IDEA, NOT THE TEMPLATE.
  2. `components/fps.rs` (90 lines): a frame-rate counter. Not needed — the demo already logs per-tick
     timing (`board.rs:105`, `log("board tick {} tiles {:?} redraw={}")`), which is the measurement
     the phase-0 gate actually asserts (no tick above 100 ms).
  Everything else in the template — clap, config.rs's 597-line layered config, tokio, tokio-util,
  futures, strum, directories, signal-hook, action enums, a Component trait — is the shape
  DO-NOT-7/8 rule out. Its dependency list is 20 crates for a hello-world with an FPS counter.
- **tui-realm / rat-salsa / ratatui-kit / tui-react / widgetui** — see part 2. Each teaches
  "component architecture". None teaches anything a 1,982-line program with one screen needs.
- **The ratatui book chapters the lead named** — I could not verify these from the awesome list,
  which does not link the book. Their subject matter maps onto rules already carrying evidence:
  testing → rule 41 (TestBackend goldens + `COCKPIT_BLESS=1`, 12 tests in 0.02 s, and 22 files of
  precedent in thurbox); panic hooks → the `errors.rs` note above; popups → rule 34
  (`display-popup -E` makes ratatui's in-app popup pattern mostly moot); layouts → the demo's
  `Constraint::Ratio` grid, independently confirmed by bottom's
  `src/options/config/layout.rs:58-99`. VERDICT: already covered. If someone wants a second opinion
  on goldens, read oxker (insta + TestBackend in one repo) rather than the book.

--------------------------------------------------------------------------------
## 4. WHAT TO ADOPT
--------------------------------------------------------------------------------

Ordered by lines removed (or bugs avoided) per unit of risk added. The honest headline first:

**Zero new crates. The list's value is patterns, not dependencies.** Of ~60 widget and utility
crates surveyed, exactly one (tui-input) has a countable line-removal case, and it removes 24 lines
that already work. The five-dependency budget survives this survey intact.

1. **agent-console's file fingerprint** — change rule 19's transcript cache key from mtime to
   `(modified_nanos, len)`. One extra field, catches two writes inside one mtime granule.
   Cost: ~2 lines. Risk: none. (`src/discovery.rs:44-46`)
2. **bosun's `Color::Reset` preview rule** — never paint a background on the preview area; let
   Claude's reset cells fall through to the terminal default. Cost: 0 lines (it is a prohibition).
   Risk: none. Avoids a class of "the preview looks wrong but only in some themes" bug.
   (`src/ui/preview.rs:1-14`)
3. **bosun's bottom-region scraping bound** — if a state ever has to be inferred from pane text,
   look only at the last ~12 non-empty lines. Cost: 1 constant. Risk: none. Prevents stale
   "Thinking…" in scrollback from pinning a tile's state forever.
   (`src/tmux/detector/claude.rs:41-47`)
4. **bosun's free tmux signals** — `#{pane_title}` (Claude writes `<braille> <task title>` while
   running, `✳` idle) and `#{pane_current_command}` (version-shaped while alive). Both ride the
   existing single `list-panes -F` at zero extra fork cost, and give a fallback for panes where
   `@panestate` was never set. Cost: 2 format fields + ~10 lines. Risk: low, and it is additive to
   the hook-set option, never a replacement. (`src/tmux/detector/claude.rs:5-13`)
5. **The panic message reaches the log** — wrap `ratatui::init()`'s hook so the panic text is
   appended to `$COCKPIT_LOG` before the abort. Cost: ~5 lines, zero dependencies. Risk: none.
   Closes the one real gap the component template exposes. (templates/component/src/errors.rs, idea only)
6. **iris's PreToolUse hook bridge** — for phase 2, THE way to see and answer a pending permission
   prompt without `send-keys`: request file + decision file + heartbeat + a `gating` marker, with a
   hook timeout strictly below the request GC window. Cost: real (a `cockpit-tui hook` subcommand
   and two directories), but it is the only known mechanism for the thing the research called
   invisible. Risk: medium — it puts cockpit on the agent's critical path, which is exactly why the
   heartbeat and gating file are non-negotiable. Adopt as a phase-2 DESIGN, not as code.
   (`iris-tui/src/bridge.rs:1-60, 287-346`)
7. **bosun's two-tick split** — 1 Hz for the full refresh, a faster tick for the focused tile only.
   Cost: one extra `Instant`. Risk: low. This is the concrete shape of rule 20's "stagger unfocused
   tiles past ~8", if the board ever feels sluggish on the tile you are watching.
   (`src/config.rs:169-176`)
8. **crmux's cursor conversion** — only if a tile ever needs to show the cursor:
   `content_row = content_lines - pane_height + cursor_y` from one `display-message -p`. Cost: ~12
   lines. Risk: low. Bank it; do not build it. (`src/app.rs:275-300`)
9. **tui-input, if and only if phase 2's reply line grows up** — the pre-approved 6th dependency,
   already verified compatible (0.15.4, ratatui ^0.30.2, crossterm ^0.29, both optional). Trigger:
   word-wise kill/yank, history, or multi-line. Until then the 24-line `edit()` stands.
10. **tui-scrollview, if phase 3 adds an expanded tile** — pre-approved 7th, compatible (0.6.7).
    Trigger: a tile that shows more lines than it has rows. Not before.

### What NOT to adopt

- **tui-term + vt100 + portable-pty** — three dependencies, one PTY and one thread per tile, N real
  tmux clients, mandatory writes to the live server (kills `COCKPIT_READONLY`), and tmux 3.7's
  read-only `send-keys` refusal makes the safe attach mode unusable. bosun shipped it for ONE
  focused session and kept capture-pane for everything else; thurbox has the whole stack and still
  calls capture-pane. `capture-pane -ep` + ansi-to-tui already works at ~4.5 ms/tile with flat
  server RSS.
- **tui-widget-list / television-style compiled fuzzy lists** — rule DO-NOT-6, and television is
  29,656 lines of evidence for it. fzf already gives fuzzy filter, multi-select and the wheel.
- **rat-widget / rat-salsa / tui-realm / ratatui-kit / tui-react / widgetui** — component
  architectures for a 1,982-line program. rat-widget alone has 17 non-optional dependencies;
  widgetui and ratatui-garnish are not even 0.30-compatible.
- **termprofile / coolor / color-to-tui** — env-based colour detection is the exact thing rule 27
  forbids inside a popup. The verdict holds, the reason moved: verified.md tmux A refuted the
  premise this was written on, and corrected rule 27 emits RGB unconditionally, so there is no
  detection to delegate at all. `theme.rs`'s `#{client_termfeatures}` query — the answer named
  here — is itself now at most an optimisation, and the gate on it was deleted.
- **tui-popup** — 6 lines of centring and a `Clear`, for a popup that already IS the window; its
  differentiator is mouse drag, banned in phase 1.
- **throbber-widgets-tui / tui-logger / tui-prompts / tui-textarea / tui-tree-widget / tui-big-text /
  edtui / ratatui-macros** — each replaces between 2 and 24 lines that exist and work.
  tui-textarea 0.7.0 is additionally incompatible (ratatui ^0.29, crossterm ^0.28) and 22 months
  stale; use ratatui-textarea if this ever comes back.
- **tmux_interface / tmux-lib** — a builder API whose value is composing the format string that
  rule 15 requires be copied verbatim, and which would break the PATH-shim test strategy of
  rule 26. Neither is in awesome-ratatui.
- **unicode-width** — **overturned; the owner accepted it on #61.** The objection stands on its
  own terms (it reports 1 for the ambiguous-width glyphs that emoji-preferring fonts draw as 2,
  and the 2-column gutter of rule 28 remains that fix) but it is not a reason to hand-roll: it is
  already in the tree via ratatui 0.30.2, so it adds nothing to the build, and rule 2's
  "no sixth without a measurement" was satisfied — L2 measured `⌨`+U+FE0F as 1 by summing char
  widths against 2 at string level, a one-column overrun that `chars().count()` cannot see.
- **Anything from claudectl (80k LOC, workspace, MCP+tokio), thurbox (112k LOC, vendored Lua),
  p2pmux (69k LOC), Reeve (29k LOC, no identifiable licence)** — and note Reeve, matchmaker, agx,
  erldash and nereid all report NOASSERTION or no licence: do not copy code from them at all.
- **ytop, rrtop, get_blessed_rs, igrep, Oatmeal, toktop, logss, repgrep** — all 8+ months stale, most
  on ratatui 0.22–0.27. Listed so nobody rediscovers them as prior art.
