# Review Redesign — the seven §8 decisions (handoff after phases 0–1)

> Written by the lead session that executed `2026-08-24-review-phase0-1.md`. Phases 0 and 1 are
> merged (or merging) to master; phases 2–4 have no plan yet **by design** — the master plan says
> two of these decisions relax invariants and must not be made by an agent. This memo gives the
> owner what phases 0–1 learned and one recommendation per item. Answer them and the phase 2 plan
> gets written (the dev lens's §5 is its spec).

## Answered 2026-08-25

**Phase 3 merged 2026-08-26 as `bca3c4e`** (29 commits, `feat/phase3-loop`): `make check` 745 passed / 0 failed, unpiped, three times on the branch and once on the merged tree; freeze rig 712 visits on 121 panes, identical to master (`tests/freeze-rig/RESULTS.txt`). Decisions #1 (dir + ref), #2 (popup default, `@cockpit_review_window`), #3 (`prefix r` is always the queue) and #7 (peter-fork pieces) are implemented; #6 (plan-mode review) stays deferred to phase 4. Two rulings changed the plan on the way: review-gate hands a queued review over as `hookSpecificOutput.additionalContext` on stdout (exit 0), and `--request` decides by the loop's exit code, since esc→leave keeps a non-empty store. The execution ledger, the final whole-branch review and its fix-wave re-review are under `docs/superpowers/research/2026-08-26-phase3-execution/`; the phase-4 candidates they surfaced are listed at the end of the final review.

**Phase 2 merged 2026-08-25 as `9d93f11`** (27 commits, `feat/phase2-store`): `make check` on the merged tree 614 passed / 0 failed, unpiped; freeze rig unchanged at master's baseline (696 visits / 5.8 walks) with the new move numbers recorded in `tests/freeze-rig/RESULTS.txt`. The plan's rulings, deferred minors and the three inputs for phase 3 (the refused-delivery dir that resumes, no GC for the session dir, the pane-id + root key) are in the phase 2 plan and its merge commit.

All seven on the recommendations below: **#1 yes, both** · **#2 popup default, `@cockpit_review_window` opt-in** · **#3 yes, always the queue** · **#4 confirmed** · **#5 no** · **#6 yes, gated on `revdiff-planning` detection** · **#7 the pieces, not the fork.** Phase 2's plan: `2026-08-25-review-phase2.md` (branch `feat/phase2-store`, where the store lib had already landed as the decision-free part). One addition the owner made the same day: the ponytail skill governs every phase's build — the goal plan's Global Constraints carry it, and each phase plan opens with an audit of what the lens scope lost to it.

## What phases 0–1 delivered (measured on this box)

| Master plan §6 target | Before | After phase 1 |
|---|---|---|
| 4900-row review opens | 450–753 ms | **62 ms** (< 200 ms) |
| one pinned comment re-marks | ~1 s | **18 ms** (< 60 ms) |
| regenerated lockfile says "too large" | 5.7 s / 254 MB | **100 ms / 58 MB** (< 1.5 s / < 110 MB) |
| 50-pane fleet queue | 2.5 s, dirty-only, churn-sorted | **350 ms**, blocked-but-clean first, repo⑂branch on the row |
| the fourteen §3 defects | — | all fixed fail-first; `make check` 544 passed / 0 failed on the merged tree, three consecutive runs |

Every step went through a per-task review plus a whole-branch review; every controller ruling is in
the SDD ledger (`.superpowers/sdd/2026-08-24-review-phase0-1/progress.md`, worktree-local) and in
the merge commits. The freeze rig is unchanged (696 visits / 5.8 walks — the pre-existing
`tmux.conf` marker walk, not this work).

Two lessons that bear on the decisions below:

- **Everything that hurt in phase 0/1 lived in shared `/tmp`.** The refused-delivery spool, the
  `.rows` snapshot (born world-readable, followed a planted symlink until fixed with `umask 077` +
  `O_EXCL`), the preview cache (attacker-owned pre-created dir until the owner/mode check). The
  purist "spool in the mktemp dir" fallback is where the review's state ends up *anyway*; it is just
  unnamed, unbounded and unprotected.
- **The queue now creates a dead end it cannot close.** Task 9 lists a blocked-but-clean agent
  first (correct), but picking it lands on "no uncommitted changes" (deferred minor) — the loop-back
  to a jump is phase 3's, and phase 3 depends on #2/#3 below.

## The decisions

### 1. Accept the two narrow invariant relaxations (a bounded session dir; one `refs/cockpit/reviewed/<branch-slug>` per approved review)?

**Recommendation: yes, both, with the bounds the master plan names.** A session dir under
`$XDG_STATE_HOME/tmux-cockpit/review/` created 0700 replaces three ad-hoc `/tmp` files with one
place that has an owner, a mode and a GC policy; the ref is the only honest base for "re-review
shows the delta since `^A`" (a per-pane `@cockpit_reviewed_sha` dies with the pane, and the pane
is exactly what dies when an SSH client drops). Cost if wrong: one directory and one ref namespace
to document and clean up (`--purge`).

### 2. Non-modal review by default (server-owned window) or popup default with the window opt-in?

**Recommendation: popup default, `@cockpit_review_window` opt-in, decided again after phase 3.**
Phase 3's Stop-hook drain changes what "modal" costs (a busy agent no longer needs you to sit in
the popup), so the non-modal default is best judged once that lands. The fleet queue's loop-back
works identically in both. Cost if wrong: one option flip.

### 3. `prefix r` = always the fleet queue with the current pane preselected?

**Recommendation: yes.** After Task 9 the queue *is* the state-first list the owner asked for;
opening the current pane directly hides the other blocked agents, and "preselected" keeps the
one-keystroke path. This also gives the blocked-but-clean dead end its natural exit (↵ on such a
row = jump, not review). Cost if wrong: one extra ↵ for the single-agent user.

### 4. Backend order revdiff → hunk (opt-in) → tuicr — confirm?

**Recommendation: confirm.** Unchanged by phases 0–1: revdiff's `--annotations`/`-o` round-trips
cockpit's store byte-identically (a lossless two-way channel, `--seed` for free); hunk's daemon
outlives its TUI and drops comments (opt-in with the #527 guard, reaped by binary path); tuicr last.

### 5. Fund phase 5 (a first-party Go TUI) at all?

**Recommendation: no — and phase 1 removed the last argument for it.** Every §6 perf target is met
with ~3× margin on bash+awk (goldens byte-identical under mawk and busybox awk on this box; a Task 14 reviewer also ran them under gawk 5.2.1 extracted from a `.deb` into a PATH shim — gawk is not installed here, so re-check with `which gawk` before relying on it). What remains
is product, not speed, and the practitioners who built this year's reviewers say the terminal's
ceiling is volume, not rendering. Revisit only with new evidence.

### 6. Add plan-mode review to phase 3?

**Recommendation: yes, gated on the `revdiff-planning` detection the master plan requires** — two
`PreToolUse` hooks on `ExitPlanMode` both fire and the second popup silently no-ops. The HN check
under-weighted plan-mode review; it is the cheapest place to catch a wrong direction. If the
detection cannot be made reliable in phase 3's budget, defer it to phase 4 rather than ship a
colliding hook. Cost if wrong: one hook to remove.

### 7. Bring the `peter-fork` TCSM work into cockpit's fleet queue?

**Recommendation: bring the *pieces*, not the fork.** `ai-title` / `transcript_path` / `session_id`
via the hooks cockpit already runs (9 ms, precomputed at transition time per the freeze discipline),
the state-first sort (already shipped in Task 9), the confirm-before-deliver `[y/N/e]` and the info
popup are all in phase 3's list. The fork's mechanisms (YAML config, `send-keys`+Enter, md5
sessions, popup-race polls) stay out, as the master plan's §3 #12 says. Cost if wrong: re-reading
one closed PR.

## What is NOT decided here

Phase 2's plan (the artifact: session dir, `diff.tsv` snapshot, `lib/review-store.sh` with
`parse` = inverse of `format`, `lib/review-diff.awk`) is unwritten and waits on #1. Phase 3 waits on
#1–#3 and #6. Phase 4 waits on #4 and the coexistence report (already written). The deferred
minors from both phases are listed in the SDD ledger for the phase 2 plan to absorb — the notable
ones: renamed-file churn is 0; a deleted/renamed file's preview disagrees with the list;
`--lines <subdir>` mixes cwd- and root-relative untracked paths; `keep_hint()`'s paths are not
`##`-escaped; the fake-agent test panes start in the worktree cwd (a pasted `> foo` once created a
file there); the `tmux-branch` relative-date flake.
