# Review Redesign — Goal Plan

> **For agentic workers:** this is the MASTER plan: phases, gates, rig, what-not-to-do, and
> the agent/team protocol. Each phase executes through its own bite-sized plan
> (`superpowers:subagent-driven-development` or `superpowers:executing-plans`). Phase 0–1's
> plan is `2026-08-24-review-phase0-1.md`, ready now. Phases 2–4 get their plans written
> **after** the owner answers the decisions in §8 — two of them relax invariants and must not
> be decided by an agent.

**Goal:** turn `prefix r` into a review the owner can actually use across a fleet of agents —
best-practice reading, delivery that lands without a pane switch, a review *moment*, a base,
a loop that closes — without leaving bash+tmux+fzf, with every step gated by a fail-first test.

**Architecture:** five independently shippable phases. 0 fixes the fourteen reproduced defects.
1 takes the diff pipeline off the per-keystroke path (awk-only rows, cached re-mark,
numstat-first cap, parallel fleet). 2 puts one artifact in the middle (a session dir with an
immutable `diff.tsv` snapshot and a `comments.md` whose `parse()` is the inverse of `format()`).
3 gives the review a moment (`@cockpit_review` set at `Stop`), a base (checkpoint ref → branch
→ HEAD), a loop (`^A` approve, spool survives, re-review shows the delta) and a delivery that
lands (paste when idle, Stop-hook drain when busy). 4 delegates diff reading to revdiff (first),
hunk (opt-in), tuicr, and adds pre-seeding, the commit gate, the AI first pass. 5 is a decision
gate, not a task — and the HN check argues the answer is "no".

**Tech Stack:** bash 4+, tmux ≥ 3.3 (popups; `display-message -d` ≥ 3.2), fzf 0.44.1 (the
installed floor — `reload-sync` yes, `transform`/`--tmux`/`--wrap` **no**), git, awk (POSIX +
`SUBSEP` only — mawk here, gawk/BSD awk elsewhere), bat optional. No Node, no Go, no daemon.

**Spec:** `docs/superpowers/research/2026-08-24-review-redesign.md` (the synthesis) and the
lens reports + prototypes under `docs/superpowers/research/2026-08-24-review-lenses/`.
Shareable one-page version of the synthesis and this plan (private artifact; updated in place):
https://claude.ai/code/artifact/8682a108-927b-4fa3-aeb2-a2f6e100a84c

## Global Constraints

- No shell on a status redraw; hooks never block the agent (bounded `read -t`, fail-open,
  no `jq` on the hot path); precomputed counters use compare-then-set at transition time
  (`scripts/agent-state:149-213` is the pattern; project memory `freeze-precompute-pattern`).
- State = per-pane tmux options that die with the pane. The two relaxations (a bounded
  session dir under `$XDG_STATE_HOME/tmux-cockpit/review/`; one `refs/cockpit/reviewed/<branch-slug>`
  per approved review) exist only if the owner approves §8 #1; until then, the purist
  fallbacks (`@cockpit_reviewed_sha` per pane; spool in the mktemp dir) are what gets built.
- One-shot scripts; no daemon; no server; no port. hunk's daemon is the documented exception,
  behind an explicit opt-in, and cockpit reaps it by binary path, never by `pkill -f` words.
- Never `send-keys Enter` into an agent pane. Never paste into `needs-input`. Never steal
  focus (`-t "$pane"`, no `select-*` as a side effect).
- Batch delivery only; `^S` is the consent gesture. `↵` never approves anything.
- Every `tmux` command string quotes its paths (`printf %q` in scripts, single-quote wrap in
  `cockpit.tmux`); post-popup work goes through `tmux run-shell -b` (the popup kills its
  process group on close).
- `bash -n` + `shellcheck -x` clean; `make check` is the gate and is run **unpiped** — `make
  check | tail` reports exit 0 on a red suite (project memory `gate-pipelines-mask-failures`).
- Tests run only on private `-L` sockets via the suite's PATH shim; every server a test starts
  is registered in `SOCKS` and killed; every mktemp goes through `mktmpd`/`mktmpf`.
- Commits: lowercase `fix:`/`feat:`/`perf:`/`test:` one-liners in the repo's voice (measured
  failures, past tense, numbers), ending with the session trailer.
- Work only in a worktree under `~/.cache/cockpit-wt/<lane>` on a `feat/<lane>` branch off
  **current** `master` (it moved during this research: `abed845` is the tip as of writing;
  rebase before opening a lane). Never edit the main checkout — another session shares it
  (project memory `concurrent-sessions-shared-worktree`).
- **Ponytail governs the build** (the `ponytail` skill, level full, added 2026-08-25): the
  ladder runs *after* the reading, never instead of it — trace the whole flow, then ship the
  shortest diff that works. No rename for its own sake, no abstraction with one caller, no
  file split with one consumer, no column without a reader; deletion over addition; a
  deliberate ceiling gets a `# ponytail:` comment naming the upgrade path. Each phase plan
  carries a **ponytail audit** table saying what the lens's scope lost to it and why — phase
  2's is the first (`2026-08-25-review-phase2.md`); the lens reports stay the research, the
  audit is the build.

---

## 1. Phases, gates, and what each ships

| Phase | Ships | Gate to merge | Plan |
|---|---|---|---|
| **0 — bugs** | `@panestate` delivery guard · `git diff HEAD` union + untracked rows · non-collapsing row parse (`\x1f`-safe) · `↵` attaches · esc confirms · colour + two-line header · full-line fragment in the store · persistent `REVIEW ↵` state + `prefix J` cycles it · fleet rows with state/repo/branch, state-first sort, blocked-but-clean panes listed · `core.quotePath=false` in the preview · preview follows the cursor | 11 fail-first tests green; `make check` green twice; leak sweep clean | `2026-08-24-review-phase0-1.md` Tasks 1–11 |
| **1 — perf** | numstat-first byte+row cap · awk-only row emission (byte-identical goldens) · cached rows + awk re-mark on reload · memoised preview · `xargs -P` fleet with `--no-optional-locks` · batched `agent-state` tmux calls · truncate-before-strip | timing gates with `timeout` teeth: 4900-row open < 200 ms, one alt-Enter < 60 ms, lockfile "too large" < 1.5 s, 50-pane fleet < 500 ms; goldens byte-identical | same file, Tasks 12–18 |
| **2 — artifact** | session dir under `$XDG_STATE_HOME/tmux-cockpit/review/<pane>-<root>/` (0700; resume on reopen; gone on discard/delivery, kept on refusal) · `lib/review-store.sh` with `parse` = inverse of `format`, full grammar (ranges, file-level) — **landed** · `snapshot()` owns the `<store>.rows` artifact · `^R` re-snapshot with stale report (and the same report on resume) · `^X` delete under cursor via the parser · one-column count marker · `^L` store-in-preview · `^E` `$EDITOR` with `display-popup -e` pass-through · `make test-only` · 9 event-waits polled — *ponytail-audited: no noun-verb rename, no awk split, no `diff.tsv`/`meta`, no `@` picker (a printable key in a comment box), 12 negative/settle sleeps kept* | round-trip property test (landed); `^R` mutation test fail-first; a review survives `kill-pane` of its client; `--detach` through a symlinked install; boundary test lists the one lib; `make check` green twice unpiped; freeze rig unchanged | `2026-08-25-review-phase2.md` |
| **3 — the loop** | `scripts/review-gate` on `Stop` → `@cockpit_review pending` + precomputed `◆N` · Stop-hook drain delivery, paste when idle · rewritten compose header + close-out line · base resolution + `^B` · `^A` approve → checkpoint · re-review delta `↺` · `prefix r` always the queue, loops back after delivery · `@cockpit_review_window` · `/cockpit-review` skill + `--request` (exit 10) · SKILL.md · `transcript_path`/`session_id` in pane options, `ai-title` labels · confirm-before-deliver `[y/N/e]` · `prefix i` info popup · plan-mode review (intercept `ExitPlanMode`) if §8 #6 says yes | hook never blocks (5 s timeout, `read -t`); rewake-loop test (hook fires twice, delivers once); pane-visits-per-redraw gate unchanged (freeze rig); every new option precomputed | to write after §8 #1–#3, #6 |
| **4 — delegate** | `@cockpit_reviewer = fzf\|revdiff\|hunk\|tuicr` — revdiff first, stdout → `parse` → `deliver` unchanged · hunk as opt-in with poller + reaper · `--seed` agent comments (`◆`) · opt-in `PreToolUse Bash(git commit *)` gate · opt-in AI first pass, five-nit cap, never blocks the popup · `delta`/`difftastic` in the preview · hunk-level rows with line drill-in | backend contract test: a fake reviewer binary that prints a fixed grammar and exits 10 lands in `deliver` byte-identically; hunk test asserts port 47657 is free after the popup closes | to write after §8 #4 and the coexistence report |
| **5 — gate** | nothing — a decision: fund a first-party Go TUI? | — | the HN check (§6c) says no; revisit only with new evidence |

Phases 0 and 1 touch only `scripts/tmux-review`, `scripts/tmux-open-path`, `scripts/tmux-jump`,
`scripts/agent-state` (one clear), `cockpit.tmux` (header/keys), `README.md`, `tests/run.sh`.
They need no decision from the owner and are safe to start now.

## 2. The rig

The existing suite is the rig; extend it, don't fork it. Conventions to reuse verbatim:

- **Private server per block:** `RDS="cockpit-rd-$$"; SOCKS="$SOCKS $RDS"; "$TMUX_REAL" -L "$RDS" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash'` — `exec -a claude` is what makes a pane count as an agent under `@cockpit_agents_glob`, which the block sets explicitly because the private socket has no `cockpit.tmux` run-shell (`tests/run.sh:3016-3030`).
- **Fixture repos:** `mktmpd`, `git init -q -b main`, deterministic author env, commit, then mutate (`tests/run.sh:2432-2447`). Add the five fixtures every lens said were missing: **mixed staged+unstaged**, **untracked-only**, **blank added line**, **200-char line**, **non-ASCII path with a blank line** — and the perf fixtures generated by a script (`tests/fixtures/mkdiff.sh <rows> <files> <width>`, ported from the perf lens's `mkrepo.sh`).
- **Driving the picker:** `send-keys -t t:` into the pane running `tmux-review --loop`, then poll `capture-pane -p` in a `for i in 1..20; case "$(cap)" in *expected*) break;; esac; sleep 0.1` loop — the pattern already at `tests/run.sh:2484-2489` and `:3120-3128`. **No new fixed `sleep`s**; phase 2 converts the 25 existing ones.
- **Delivery assertions:** `rundeliver` + `lastrc` + `apcap`/`npcap` (`tests/run.sh:3040-3062`) — the `RC=` marker is the assertion of record, never the absence of text.
- **Perf teeth:** `timeout N cmd` where N is ~3× the expected time on this machine, exactly as `tests/run.sh:590,882,1738` do; a red gate means the regression came back. Record the measured number in the test's comment.
- **Golden files:** commit the current `--lines` output for every parser fixture as `tests/fixtures/review/*.tsv` **before** touching the parser (phase 1 Task 12); `cmp` against them after.
- **Freeze rig:** `tests/freeze-rig/run.sh` stays the gate for any new tmux option or format — pane visits per redraw must not exceed pane count.
- **Leak sweep after every lane:** `ls /tmp/tmux-1000 | grep -c cockpit-` unchanged; `pgrep -fa 'hunk|lens'` empty; `ss -ltnp | grep 47657` empty.
- **Hermetic tmux stubs (from tmuxai's test pattern):** where a function only *formats* tmux calls, route them through one `_tmux()` indirection the test can override — phase 2's `review-store.sh` and `review-diff.awk` need no tmux at all and must stay that way (boundary test).

## 3. What NOT to do

Each of these was either reproduced by a lens or shipped by a tool we studied.

1. **Do not reload the world after a comment.** `reload(rows_cmd)` re-running `git diff` + awk + bash on every alt-Enter is the 1 s stall. Re-mark cached rows in awk.
2. **Do not capture the diff before capping it.** Numstat first, cap on bytes *and* rows, then capture. A regenerated lockfile is 5.7 s / 254 MB today.
3. **Do not review the live tree.** Snapshot once per session; the agent keeps editing under you and the `●` marks drift onto lines you never touched.
4. **Do not paste into `needs-input`.** The human's Enter becomes a blind approval of whatever tool was being asked about.
5. **Do not `send-keys Enter`, ever.** Not even "gated on done" — the race lands in a dialog.
6. **Do not shred the store on esc / non-zero exit / delivery.** Confirm, keep, or spool.
7. **Do not add a fifth `@panestate` value.** Its ranking is load-bearing for the sticky guard and `@cockpit_win_needs`; review state is a separate option.
8. **Do not walk panes on a redraw.** Every new counter is precomputed at transition time, compare-then-set.
9. **Do not make alt-chords the only way to do something.** They are silently dead on many terminals (hunk #843). Plain key first, alt as alias.
10. **Do not build a Go/Rust core for speed.** Startup differs by 0.17 ms; awk closes the only slow path 8×. Go only for a TUI, only at the phase-5 gate, only on product merits — and the humans who built reviewers this year say the terminal's ceiling is volume, not rendering.
11. **Do not run hunk without an opt-in, a poller, and a reaper.** Its daemon outlives the TUI; its comments do not. `pkill -f 'hunk daemon'` killed the lens agent's own shell — match the binary path.
12. **Do not adopt tmuxai or TCSM.** Take the screens (confirm `[y/N/e]`, totals footer, options block, info popup, state-first sort, `ai-title` via hooks); leave the mechanisms (YAML config, `send-keys`+Enter, md5 sessions, popup-race polls, 0.7 s CLI on any path a human waits on).
13. **Do not re-litigate the grammar.** revdiff's `## path:line (+)` is the wire format; keep the `> fragment` line (our one advantage); add ranges and file-level *within* it; never put an id in the header (it parses as a filename).
14. **Do not let `^S` mean anything but "deliver the batch".** hunk's `^S` = save-one-note would send half-written reviews through muscle memory.
15. **Do not pipe the gate.** `make check | tail` is green on a red suite.
16. **Do not edit the main checkout.** Another session is merging Wave D into it right now.
17. **Do not write "similar to task N", "add error handling", or a fixed `sleep` into a plan or a test.** Placeholders are plan failures; sleeps are the flakiness the suite already has too much of.
18. **Do not open a popup from inside a popup.** The second `display-popup -E` exits 0 with empty output, which every consumer reads as "clean review" (reproduced). revdiff's own launcher is a popup — exec the binary, not the launcher, from cockpit's popup.
19. **Do not let two tools deliver into the agent, and do not add an `ExitPlanMode` hook without checking for `revdiff-planning`.** Two `PreToolUse` hooks on one matcher both fire; either can deny; the second popup no-ops.
20. **Do not trust an empty result from `hunk session list`** (#527 — daemon-down and no-sessions are byte-identical, exit 0 both), **do not select hunk sessions by `--repo`** (ambiguous with two TUIs on one repo — use `paneId`), **do not call `comment list` without `--type user`** (the default hides every human note).
21. **Do not use revdiff's default history dir as a recovery path** — pass `--history-dir` (#331: keyed by repo basename, cross-reads same-named repos).

## 4. Agents and team — how to run this

**Shape:** one *lead* session (keeps the plan, dispatches, integrates, runs the gate) plus
short-lived *worker* subagents, one per task, fresh context each, via
`superpowers:subagent-driven-development`. Workers get: the task text verbatim (they see only
their task, so its **Interfaces** block is how they learn neighbours' names), the worktree path,
the global constraints, and the instruction to return a summary + the exact test output.

**Lanes (what can run in parallel):**

| Lane | Files | Parallel with |
|---|---|---|
| A `tmux-review` | `scripts/tmux-review`, its `tests/run.sh` blocks | nothing else that touches `tmux-review` — phase 0 Tasks 1,3,4,5,6,7,9,11 and phase 1 Tasks 12–16 **serialize** on this lane |
| B `open-path` | `scripts/tmux-open-path` (quotePath, preview memo, `HEAD` alignment) | A, C, D |
| C `state` | `scripts/agent-state` (clear review badge; batched tmux calls), `scripts/tmux-jump` | A, B, D |
| D `docs/config` | `cockpit.tmux` (header/keys/options block), `README.md` | A, B, C |

Two workers on lane A at once will produce a merge conflict every time — the dev lens counted
eight functions sharing one file. If the owner wants more parallelism, phase 2's split into
`tmux-review` + `lib/review-diff.awk` + `lib/review-store.sh` is what buys it.

**Review is a separate pass, never self-approval.** After each task: a fresh
`oh-my-claudecode:code-reviewer` (or `superpowers:requesting-code-review`) agent reads the diff
against the task's spec and the constraints; findings go back to a fresh worker. The lead does
not merge its own unreviewed work. Before a phase merges: one whole-branch review by a
`critic`/`architect`-class agent against the synthesis, then `make check` green **twice**
consecutively on the lane, then the leak sweep, then merge to master with a green gate.

**Model routing:** workers on lane A tasks that touch the awk parser or `deliver()` → opus;
config/README/simple guard tasks → sonnet; lookups → haiku. Research is done — no more lens
agents unless a task hits something the reports did not measure.

**Use `oh-my-claudecode:team` (or `/team`) only for phases 2–4**, where the file split makes
three lanes genuinely independent; for phases 0–1 the serialized lane A makes a team slower
than one worker at a time plus lanes B–D in parallel.

**What every worker returns:** the commands run and their real output (the `is` lines, the
`timeout` result), the commit hash, what was *not* done and why. "Tests pass" without the
output is not evidence.

**What the lead does between tasks:** reads the summary, runs the specific test block itself
(`tests/run.sh` has no per-block runner — grep the `== title ==` and run the whole suite; it is
~90 s), checks `git diff --stat` matches the task's Files list, and only then dispatches the
next task.

## 5. Order of work (recommended)

1. Open lane A worktree: `git worktree add ~/.cache/cockpit-wt/review-p0 -b feat/review-p0 master`.
2. Phase 0 Tasks 1–11 in order (Task 2's untracked rows are a prerequisite for Task 9's fleet
   rows; Task 11's offset field is a prerequisite for the preview follow).
3. Merge phase 0 (gate + leak sweep). Ship it — the owner can use the reviewer from here.
4. Phase 1 Tasks 12–18, opening lanes B and C in parallel for the preview memo and the
   batched `agent-state` calls.
5. Merge phase 1. Measure again with the perf lens's `bench.sh` and record the numbers in
   the commit messages.
6. Owner answers §8 — **done 2026-08-25, all seven on the memo's recommendations.** Phase 2's plan is `2026-08-25-review-phase2.md` (dev lens §5 as spec, ponytail-audited); 3 and 4 follow the same shape.

## 6. Measures of done

- A 4900-row review opens in < 200 ms and a comment lands in < 60 ms (today 450–753 ms / ~1 s).
- A regenerated lockfile shows "too large" in < 1.5 s and < 110 MB (today 5.7 s / 254 MB).
- The fleet queue lists every agent that needs you, blocked-but-clean first, in < 500 ms at
  50 panes (today: dirty-only, churn-sorted, 2.5 s).
- A review written while the agent is busy reaches it at its next turn boundary with zero
  keystrokes; written while it is idle, it lands typed-but-unsent with a persistent badge.
- Re-opening a review shows only what changed since `^A`.
- `esc` never loses a comment; a dropped SSH client never loses a review.
- `@cockpit_reviewer=revdiff` produces the same delivered payload as the fzf path for the
  same comments (contract test).
- Every one of the fourteen reproduced defects has a fail-first test that was red on
  `55de827` and is green on the merged branch.

## 7. Coexistence of hunk and revdiff

Verdict from `hunk-revdiff-together.md` (live lab, both installed on one tmux + Claude Code
box): **yes as a documented pairing, no as a product feature, and emphatically no as "two
reviewers".** Nobody in the community runs both as reviewers; the herdr plugins that wrap
each are separate. The three costed configurations:

- **A — revdiff reviews, hunk watches.** `hunk diff --watch` in a side pane is a read-only
  live mirror of the tree (proven: re-anchors notes across an fs-event reload); cockpit never
  reads its comments. Buys the side-by-side reader fzf can never be, at the cost of a
  session-long daemon. **Offer as README documentation, not a feature** (no
  `@cockpit_watcher` option — owning a hunk lifecycle means owning a daemon). Document that a
  `c` typed in the watcher never reaches the agent.
- **B — hunk as the backend.** As §6d: poller, `--type user`, fragment re-derived, daemon
  reaped by resolved binary path. New blocker: hunk #527 — `session list` returns the same
  "No active Hunk sessions" + exit 0 whether the daemon died or there were none, so a poller
  that loses the daemon harvests zero comments and reports success. Capture the sessionId at
  launch and probe the port; never trust an empty result.
- **C — one store, two renderers.** revdiff `--annotations=comments.md` in → `-o comments.md`
  out is **byte-identical, fragment line preserved** — the backend seam becomes a lossless
  two-way channel and `--seed` is free on the revdiff path. hunk renders cockpit's rows via
  `--agent-context notes.json` (read-only). **Ship C for revdiff; hunk stays one-way.**

Community check (`hunk-revdiff-together-appendix.md`): seven setups run both; the one written
rationale (fanout) matches this split — hunk as resident bidirectional reader, revdiff as the
zero-dependency exit-code contract; afontaine's config binds them to two keys with two roles;
the one "kept one" write-up cites two overlapping skills competing for the same request; the
one conflict on record is two `ExitPlanMode` hooks. Model the switch on herdr-pickr: show only
the reviewers that are installed.

Phase 4's plan therefore wires: `@cockpit_reviewer` with exactly one value, one harvest, one
delivery; revdiff round-trip via `--annotations`/`-o` with an explicit `--history-dir` under
cockpit's session dir (revdiff #331: the default history is keyed by repo *basename* and
cross-reads between same-named repos); `$EDITOR` re-injected into the popup env (it holds
only `TMUX`, `TMUX_PANE`, `TERM`); hunk as opt-in with the #527 guard. Phase 3's plan-mode
item (§8 #6) must detect an installed `revdiff-planning` before adding its own `ExitPlanMode`
hook — two `PreToolUse` hooks on one matcher both fire and the second popup silently no-ops.

## 8. Decisions the owner must make before phases 2–4

1. Accept the two narrow invariant relaxations (session dir; one git ref per approved review)?
2. Non-modal review by default (server-owned window) or popup default with the window opt-in?
3. `prefix r` = always the fleet queue with the current pane preselected?
4. Backend order revdiff → hunk (opt-in) → tuicr — confirm?
5. Fund phase 5 at all? (Recommendation: no.)
6. Add plan-mode review to phase 3? (If yes: it must detect `revdiff-planning` and step aside —
   two `ExitPlanMode` interceptors collide.)
7. Bring the `peter-fork` TCSM work into cockpit's fleet queue?
