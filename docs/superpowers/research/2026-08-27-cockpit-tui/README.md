# Issue #61: cockpit-tui, the Rust/Ratatui board

Research, 2026-08-27, after the owner chose Rust (PR #55 carries the measurements and the demo). Lanes: [rules.md](rules.md) (56 DOs, 28 DO NOTs, per-phase gates, "read this first"), [workflow.md](workflow.md) (lanes L0–L6 with a DAG, the interface freeze, gates, kickoff messages), [base.md](base.md) (fork candidates incl. the six agent dashboards from awesome-ratatui; verdict: promote the demo, fork nothing, vendor ~250 lines; folder structure `tui/` in-repo), [ux.md](ux.md) (tile anatomy, focus, needs-input, sort, grid/density/paging, triage overlay, keymap, states, colour/glyph discipline, style guide, three reference apps), [awesome.md](awesome.md) (87 apps and 64 crates from awesome-ratatui, verdict: zero new crates), [references.md](references.md) (the repos the owner named: kwaak, rust-code, and more as they land). No code changed.

## Decisions the owner has to make

1. **Fetch timing.** rules.md 39 / DO NOT 14: fetch the binary only on `prefix U`, never at plugin load. base.md §4's `.tmux` sketch fetches once per version at load, bounded by a `.tried` marker and `curl --max-time 30`. Pick one. Recommendation: `prefix U` only; the load path stays offline.
2. **tmux floor for the board.** rules.md 35 says ≥ 3.7 (#4920, popup overwritten by background repaints); base.md §4 says the plugin's existing 3.3 floor is enough because the board is `display-popup -E -T`. Recommendation: 3.7, because a hook-driven cockpit repaints panes constantly and the tear is exactly the board's failure mode; gate it in the binding with a message.
3. **The list-panes format's quoting.** rules.md 15/16 and DO NOT 11: the repo's format verbatim, sentinels, no `#{q:}` (the #85 suspect; cockpit.tmux:54-59 rejects it because it passes `\n`/`\t` through). base.md §2 and awesome.md both say the demo's raw `\t` split is the first bug to fix and propose the sidebar's `#{q:}` + unescaper; the 6,000-call leak test on 3.7c showed no growth with `#{q:}`. Recommendation: a separator that cannot occur (tmux ≥ 3.4 octal-escapes control characters in format output, so `\x1f` arrives as literal `\037`) or `#{q:}` with the unescaper; decide once.
4. **tui-input for the phase-2 reply line.** base.md says adopt (wide-char cursor handling, ~150 lines otherwise); awesome.md says the demo's 24-line editor already works and a sixth dependency needs a trigger (kill/yank, history, multi-line). Recommendation: hand-rolled until the trigger.
5. **Board popup width.** ux.md: `-w 90%` (a 70 % popup on a 120-column terminal is one tile column) or `@cockpit_board_window on`. Recommendation: 90 % default for the board key only.
6. **The board key.** workflow.md proposes `prefix w` (the board is a better `choose-window`), rebindable via `@cockpit_board_key`, `off` keeps stock; base.md's sketch used `U`, which collides with tpm's update key. Recommendation: `w`.
7. **Phase-2 permission visibility.** iris's PreToolUse hook bridge (request/decision files, heartbeat, arm marker, timeout below the GC window) is the only known way to see and answer a permission prompt without `send-keys`, and it puts cockpit on the agent's critical path. Adopt as the phase-2 design, or keep phase 2 to `tmux-review --deliver` on `AskUserQuestion`-style waits only.
8. **Where the interface freeze lives.** workflow.md §3 (Pane struct, 14-key `--json`, options `@cockpit_tui_pid/_version`, `@cockpit_board_window/_key`, env `COCKPIT_ACTION/_TMUX/_LOG/_READONLY/_GLYPHS`, the one-line action file, CLI, exit codes). Sign it on #61 before L0 forks; changes only by a comment there.

## What changed since PR #55

- Two startup hazards added to the rules (49–50) from rust-code: crossterm's lazily built event reader caches a failed first `poll` forever (probe with `poll(Duration::ZERO)` before `init()`), and `Terminal::draw` can exit on a stale errno when `/dev/tty` blinks (a consecutive-failure budget; `Interrupted` from our own SIGUSR1 is a retry).
- The pid unpublish moves into the signal handler with a pre-formatted fork+exec argv (rule 51).
- Cheap adoptions from awesome.md: `(mtime, len)` as the transcript cache key; no background painted under the preview; a 12-line bottom-region bound if pane text is ever scraped; `#{pane_title}` and `#{pane_current_command}` as free fallback signals for panes with no `@panestate`; the panic message appended to `$COCKPIT_LOG`.
- bosun's pure state detectors and hysteresis smoother, iris's hook bridge, crmux's fixture library and agent-console's grid math are the ~1,900 lines worth copying (base.md §A); none of the six is a base.
- Measured: `capture-pane -e` emits SGR only (no cursor motion), so a vt100 emulator would re-emulate an already-rendered screen; control mode is a doorbell (bosun parses `%output` and throws it away, never reconnects).

## Fail-first tests

The per-phase gates in rules.md §3, plus per lane in workflow.md §1. Before L0: the demo's `--json` on a fixture with a tab in a window name still names every pane (decision 3); a fake `tmux -V` at 3.6 makes the board binding refuse (decision 2); the ascii-tier golden is byte-stable (rule 56).
