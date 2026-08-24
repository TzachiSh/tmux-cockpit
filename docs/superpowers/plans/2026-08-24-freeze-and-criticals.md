# Freeze + Criticals Remediation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use `- [ ]` checkboxes.

**Goal:** Fix the tmux freeze under agent load and the critical security/correctness/scale bugs found by four opus hunts, each with a reproduced regression test, keeping every repo invariant.

**Architecture:** Freeze fixes follow nicm's "precompute into an option" pattern — move the whole-server `#{S:#{W:#{P:}}}` walk out of `pane-border-format` into a global option that `agent-state` writes only on transitions; guard the unconditional write in `agent-context`. The O(n²) scale bugs are all the same shape (`$(printf)` fork per item + bash `+=` into one string) fixed with `printf -v` + indexed arrays. The RCE is fixed with tmux's own `#{q:}` shell-quoting.

**Tech Stack:** bash 4+, tmux 3.7c formats, awk, git, fzf, shellcheck. Regression rig from the freeze-rig agent (emits OSC-2 titles — plain line output shows nothing).

**Spec:** docs/superpowers/research/2026-08-24-critical-bugs.md (every finding, file:line, reproduction, fix direction) — the binding authority.

## Global Constraints
- Every invariant in the freeze research holds: NO shell on status redraw; a `set-option` forces a full server-wide redraw so writes must be transition-gated / no-op-guarded; hooks never block the agent (bounded reads); state dies with the pane; one-shot scripts; bash 4 guard; `set -uo pipefail`; shellcheck + `make check` gate; comment voice = measured failure + number, past tense.
- Every fix ships with a regression test that FAILS against the current code (verify by revert-and-rerun) — the culture rejects unreproduced claims and untested fixes.
- Freeze tests must emit OSC-2 titles, not plain output. Use a deterministic proxy (redraw count / server utime delta) the freeze-rig agent will specify, not flaky wall-clock p95, unless it certifies wall-clock is stable enough.
- Commits: lowercase `fix:`/`perf:` one-liners + the session trailer (Co-Authored-By: Claude Fable 5 <noreply@anthropic.com> / Claude-Session: https://claude.ai/code/session_01LAJW8Xs7whjSgRzRHNFAER).
- Worktrees off master; never edit the main checkout (concurrent sessions). Push after each wave's gate is green.

---

## WAVE A — security + the freeze (ship first, highest severity)

### Task A1: C1 — RCE via directory name (cockpit.tmux:46-47)
**Files:** Modify cockpit.tmux; Test tests/run.sh
**Fix:** replace the hand-quoted `'#{pane_current_path}'` / `'#{pane_current_command}'` inside the `#()` job with tmux's `#{q:pane_current_path}` / `#{q:pane_current_command}` (verified on 3.7c: `#{q:@x}` on `a'b$(x)c` → `a\'b\$\(x\)c`). Remove the now-redundant hand quotes.
- [ ] Failing test: a pane whose cwd is `x'$(touch $SP/OWNED)'y`, real config with `#{agent_where_path}` in status, an attached client (pty via `script -qfc` or a background `tmux attach`); assert `$SP/OWNED` is NOT created after a redraw. Confirm it FAILS on current code first.
- [ ] Fix both lines; re-run to green; also assert the path still renders correctly for a normal cwd (don't break the feature).
- [ ] `make check`; commit `fix: a directory name can no longer run code through the status job`.

### Task A2: F1 — agent-context unconditional write (agent-context:40)
**Files:** Modify scripts/agent-context; Test tests/run.sh
**Fix:** before the `tmux set-option -p @panestate_ctx`, read the current value (`tmux show-option -pqv` — measured 0 redraws) and early-exit when equal (the agent-state:130 pattern). Read is free; the write is the 19ms full redraw.
- [ ] Failing test: drive agent-context twice with the same fixture payload; assert the second invocation issues no `set-option` (shim `tmux` on PATH to count set-option calls, or assert via a redraw proxy). FAIL-first on current code.
- [ ] Fix; green; keep the existing agent-context tests passing.
- [ ] `make check`; commit `perf: the statusline shim skips the write when context fill is unchanged`.

### Task A3: F2 — move the NEEDS YOU banner walk out of pane-border-format
**Files:** Modify scripts/agent-state (write the precomputed banner on transitions), examples/tmux.conf + examples/full.tmux.conf (border format reads the option), cockpit.tmux if the token needs interpolation; Test tests/run.sh (with the freeze rig)
**Interfaces:** agent-state, on any state transition, computes the cross-window "NEEDS YOU <win>:<name>" string ONCE and writes it to a global option (e.g. `@cockpit_needs_you`); the pane-border-format's aggregate arm reads `#{@cockpit_needs_you}` instead of the nested `#{S:#{W:#{P:}}}` walk. The own-pane arm (a pane's own needs-input) stays a local read. Preserve the exact rendered output (banner text, colours, the focused-pane suppression rule).
- [ ] Failing test: the freeze rig (from freeze-rig agent) at its documented shape with OSC-2-emitting agents; assert the fix's redraw-cost proxy drops below the threshold the rig defines, AND that the rendered banner is byte-identical to the current output for a known blocked-pane arrangement (a correctness test, not just a perf one). FAIL-first: the perf assertion fails on current code.
- [ ] Implement: agent-state computes and writes the banner (this is a transition-time cost, off the redraw path); update both example configs; verify the focused-pane suppression and the multi-window banner still render exactly.
- [ ] `make check` + the rig; commit `perf: precompute the NEEDS YOU banner so a redraw stops walking every pane`.

### Task A4: agent_fleet double-walk (examples/tmux.conf status-right)
**Files:** Modify examples/tmux.conf, examples/full.tmux.conf, README if documented; Test tests/run.sh
**Fix:** each `#{agent_fleet}` counter is evaluated twice (the `#{?#{e|>:N,0}}` test + the output), = 2-4 whole-server walks. Collapse: compute each count once. Options: precompute into an option like the banner, OR restructure the format so the walk runs once per counter (a single `#{S:#{W:#{P:}}}` whose result is reused). Prefer precompute-into-option for consistency with A3 if the count is cheap to maintain on transitions; else the single-walk format.
- [ ] Failing/observing test: assert the rendered fleet string is unchanged for a known arrangement (correctness), and the walk-count proxy drops. Keep the existing fleet tests green.
- [ ] Implement; `make check`; commit `perf: the fleet counter walks the server once, not four times`.

---

## WAVE B — correctness criticals (parallel-safe with C after A lands; both touch tmux-review so serialize B and C within the same lane)

### Task B1: C2 — review anchors to the wrong file on quoted paths (tmux-review lines_mode)
**Fix:** run the diff with `git -c core.quotePath=false diff` (and unquote if any residual), and take the path from the `+++`/`diff --git` header only after quoting is off, so `café.txt`/tab/quote filenames anchor to themselves.
- [ ] Failing test: repo with modified `alpha.txt` and `café.txt`; `--lines` must emit café's rows attributed to café with café's line numbers (currently attributed to alpha or dropped). FAIL-first.
- [ ] Fix; green; add a first-file-in-diff variant (the drop case).
- [ ] `make check`; commit `fix: a non-ASCII filename no longer anchors its comments to the previous file`.

### Task B2: C3 — delivery TOCTOU (tmux-review deliver)
**Fix:** make the is-agent check atomic with the paste — gate paste-buffer on tmux's own conditional in one command-queue item: `tmux if-shell -F "#{m:*|#{pane_current_command}|*,$glob}" "paste-buffer -p -r -d -b $buf -t $pane" "display-message 'the pane is no longer an agent — not delivered'"`. Remove the ~50ms window between the shell-side guard and the paste.
- [ ] Failing test: reproduce the race (pane runs a shell that outlived an `exec -a claude` child; deliver, and between guard and paste the command is `bash` not `claude`) — assert the review text does NOT land in the shell. FAIL-first against the current shell-side guard (the hunt reproduced 6/6 landings at 30-55ms).
- [ ] Fix; green; keep the happy-path deliver tests + the guard-refusal tests green.
- [ ] `make check`; commit `fix: the review paste is gated atomically, so a pane that stopped being an agent never receives it`.

---

## WAVE C — scale criticals (same lane as B; the O(n²) shapes)

### Task C0 [ELEVATED — this is the HARD-LOCK] row cap + lazy startup, so a giant diff can't wedge the modal popup
**Files:** Modify scripts/tmux-review; Test tests/run.sh
The prefix+r popup is modal; lines_mode runs at popup STARTUP and every reload; on a lockfile-sized
single-file diff the O(n²) parse locks the client for minutes-to-hours (hunt-hardhang). Before/besides
the O(n²) fix, add a HARD CAP: if the diff exceeds N rows (e.g. 5000), --lines emits a single
"diff too large to anchor line-by-line (N rows) — review it directly" row (or a bounded head) instead
of parsing all of it, so the popup ALWAYS appears and stays esc-cancelable. Also move the startup
`nrows=$(lines_mode|wc -l)` off the critical path (the cap makes it bounded regardless).
- [ ] Failing test: `--lines` on a 50k-row single-file diff returns within a small bound (<1s) with the
  "too large" row, not after 30s+. FAIL-first (current: 30s+).
- [ ] Implement cap; keep normal diffs (< N rows) fully functional and byte-identical.
- [ ] `make check`; commit `fix: a giant diff can no longer wedge the review popup — it caps and stays cancelable`.

### Task C1: S1 — tmux-review --lines O(n²) + fork per row (:199-200)
**Fix:** `printf -v disp ...` (no command substitution); accumulate rows into an indexed array or a temp file, never `+=` into one associative-array element.
- [ ] Failing/perf test: `--lines` on an 8000-row single-file diff completes under a threshold (current: 55.9s). Assert linear-ish scaling (e.g. 8000 rows < K seconds, K set from the rig's warm baseline). FAIL-first.
- [ ] Fix; assert byte-identical output to the pre-fix parser on a small fixture (correctness), and the perf threshold.
- [ ] `make check`; commit `perf: --lines builds its rows without a fork or a quadratic append per line`.

### Task C2: S2 — re-parse + grep-per-row on every pinned comment (:245-247,310,338)
**Fix:** read the store's `## ` headers once into an associative array before the row loop (replace the per-row `grep -qxF`); pass the row count out of lines_mode instead of calling it twice.
- [ ] Failing/perf test: pin a comment on a 2000-row diff; assert the reload completes under a threshold (current: ~11.5s). FAIL-first.
- [ ] Fix; green; assert the ● markers still land on exactly the commented rows (correctness).
- [ ] `make check`; commit `perf: pinning a comment no longer re-greps the store once per diff row`.

### Task C3: S3 — tmux-where /proc walk, twice per redraw on ssh panes (:117,138)
**Fix:** build the ppid→children map with an indexed array (not string `+=`), OR — better — walk UP from the pane pid to find the ssh child instead of building a full downward map of every process.
- [ ] Failing/perf test: `tmux-where path <pid> /tmp ssh` on a 2000-process tree completes under a threshold (current: ~0.7s). FAIL-first.
- [ ] Fix; green; assert the ssh-destination output is unchanged on a known small process tree (correctness — this feeds the headline feature).
- [ ] `make check`; commit `perf: the ssh path walk stops rebuilding a whole-process-tree map on every redraw`.

---

## WAVE D — importants (mostly independent files; can parallelize across lanes)

- **D1 I1** cockpit.tmux reload binding (:270-277): source EVERY `#{config_files}` entry (split on comma) in one quoted string, not `cut -f1`; put the `display-message` in the same bound string. Test: multi-entry config_files (fixture NOT started with `-f /dev/null`) reloads the user's file, and a 2-config install sources both.
- **D2 I2** install.sh --hooks concurrent write (:140→239): O_EXCL lockfile around read→rename, or re-read+remerge if mtime/size changed before rename. Test: concurrent writer (slow-jq shim) mid-merge; the concurrent write survives.
- **D3 S4** fleet queue serial git (tmux-review:518,523-524,537): bounded `xargs -P` parallelism, or one `git status --porcelain=v1` pass per pane instead of status+numstat+grep. Test: 50-pane fleet with a slow-git shim completes under a threshold.
- **D4 S5** tmux-branch fork per branch (:173): `printf -v row`. Test: 5000-branch repo popup-open under a threshold (current 16.9s).
- **D5 S6** tmux-open-path preview guards (:170-171,75-82): measure `longest` over the render window [first,last] not lines 1-200; stream `git diff|awk|head-400` instead of capturing, drop the second full diff. Tests: a 3MB line at line 500 is guarded; a 400k-line diff preview is bounded in time/RSS.
- **D6 I4** popup client resolution (tmux-review:634, tmux-branch:38): resolve via the popup's own client or pass `#{pane_id}` into the binding string (click bindings already do). Test: two clients, popup opened on the less-recently-active one resolves ITS pane.
- **D7 I3 + S7** (low): agent-state stale-TMUX_PANE server-birth guard if cheap (else document); tmux-open-path clicked-line token cap like the preview.

---

## Sequencing
- Wave A first (security + freeze — what the user feels), each task its own review + fix loop; ship the wave when its gate is green.
- Waves B and C share scripts/tmux-review → ONE lane, sequential (B1,B2,C1,C2 on tmux-review; C3 on tmux-where can parallelize).
- Wave D: D2/D4/D5 (install.sh, tmux-branch, tmux-open-path) are independent files → parallel lanes; D1/D6 touch cockpit.tmux/tmux-review → serialize after their wave.
- Every wave: final whole-branch review before merge; the freeze rig is retained in tests/ as the permanent regression guard.

## Self-review
Spec coverage: every finding in 2026-08-24-critical-bugs.md maps to a task (C1→A1, F1→A2, F2→A3, fleet→A4, C2→B1, C3→B2, S1→C1, S2→C2, S3→C3, I1→D1, I2→D2, S4→D3, S5→D4, S6→D5, I4→D6, I3/S7→D7). Known dependency: A3/A4 and the freeze tests need the freeze-rig agent's rig + threshold — do not start A3 until that lands.
