# Review-Like-a-Boss Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `prefix R` opens a popup review of a Claude agent's uncommitted diff: pick diff lines, attach comments by typing them, and deliver the batch into the agent's pane as anchored `path:line` blocks — without a daemon, a web UI, or a new hard dependency.

**Architecture:** One new one-shot script (`scripts/tmux-review`) mirroring the `tmux-branch` pattern: an fzf picker over numbered diff lines whose preview reuses `tmux-open-path --preview-diff`; alt-enter attaches the typed query as a comment anchored to the highlighted row; ctrl-s composes all comments in the revdiff annotation grammar and pastes them into the agent's pane via `load-buffer`/`paste-buffer -p -r -d` — bracketed, LF-only, never auto-submitted. Comment state lives in a mktemp file whose lifetime is the popup; nothing persists, nothing polls.

**Tech Stack:** bash 4+, tmux ≥ 3.1 (`display-popup` path needs 3.2, guarded like the existing popups), fzf, git; bat optional (previews degrade as in tmux-open-path).

**Spec:** the two research reports this plan argues from:
- `docs/superpowers/research/2026-08-24-review-tools-landscape.md` (community landscape; the revdiff grammar; the gaps)
- `docs/superpowers/research/2026-08-24-review-antipatterns.md` (what NOT to do, with evidence; repo reuse inventory; invariants; delivery mechanics)

## Global Constraints

- Everything in the anti-patterns report's **invariants checklist** binds every task (no shell on redraw; hooks never block; state dies with the pane; one-shot scripts; bash 4 guard; `set -uo pipefail` + `H=${HOME:-/nonexistent}`; shellcheck + `make check` gate; path quoting; popup process-group death → post-popup work via `tmux run-shell -b`; popup cwd/$TMUX_PANE are not the source pane; no focus stealing; comment voice).
- **WHAT NOT TO DO** (each rule cites the research; violating one is a review-blocking defect):
  1. No whole-diff-one-Enter approval path — the unit of review is the line/hunk; there is deliberately NO "accept all" key.
  2. Never send Esc to the agent pane; never auto-submit; never paste into a pane whose `@panestate` is `running` without the explicit two-step (paste lands typed-but-unsent; the HUMAN presses Enter).
  3. No review daemon, server, database, or state file that outlives the popup (the >2KB payload file is the delivery vehicle, not a store — it lands in `$XDG_RUNTIME_DIR` and the agent consumes it).
  4. Comments always carry `path:line` AND the quoted source fragment — never a bare line number (fragments survive the agent re-editing).
  5. Batch delivery only: nothing reaches the pane until ctrl-s, one paste per review.
  6. No `select-pane`/`select-window`/pane creation as a side effect; target `-t "$pane"` and leave focus alone.
  7. No `send-keys` for body text — buffers only (`send-keys` is reserved for the never-bundled optional Enter).
  8. Test files sort FIRST in the file ordering, never last.
  9. Adopt the revdiff grammar verbatim (`## path:line (+)` headers, ` ## ` body-escape, `??` = question); do not invent a format.
- Commits: lowercase `feat:`/`fix:` one-liners ending with the session trailer (Co-Authored-By: Claude Fable 5 <noreply@anthropic.com> / Claude-Session: https://claude.ai/code/session_01LAJW8Xs7whjSgRzRHNFAER).
- Work in a worktree (`~/.cache/cockpit-wt/review-flow`, branch `feat/review-flow` off master); never edit the main checkout (concurrent sessions).

## File Structure

- Create: `scripts/tmux-review` — the whole feature: target resolution, diff-line list builder, fzf loop, comment store, payload composer, delivery. Subcommands for fzf binds and for tests: `--lines <dir>`, `--compose <store>`, `--deliver <pane> <store>`.
- Modify: `cockpit.tmux` — one binding block (`prefix R`, `@cockpit_review_key`, `off` disables) beside the `tmux-jump` binding.
- Modify: `README.md` — a "Review the agent's changes" section + key row; `examples/tmux.conf` — nothing (no formats change).
- Test: `tests/run.sh` — one new block (private socket, `exec -a claude bash` agent pane, fixture repo).

---

### Task 1: Diff-line list + picker skeleton

**Files:** Create `scripts/tmux-review`; Test `tests/run.sh` (new block)

**Interfaces:**
- Produces `tmux-review --lines <dir>`: prints tab-separated rows `path\tline\tfragment\tdisplay` for every +/- hunk line of `git -C <dir> diff` (unstaged, falling back to staged like `--preview-diff:66-67`), test files first, then by file churn descending. `line` is the NEW-file line number for `+`/context anchoring, the OLD-file number prefixed `-` for deletions (revdiff's `(-)` side).
- Produces the interactive default mode: resolves the target pane (from an agent pane: itself; else: Task 5's queue), then runs the fzf loop in a popup titled ` review · <pane title> `.

- [ ] **Step 1: failing test** — fixture repo with one modified source file, one modified test file, one deletion; assert `--lines` output rows: test file's rows first,每 row carries path, a numeric line, a non-empty fragment; deletion rows carry the `-` prefix. (Suite style: `is`/`case`, no tmux needed for `--lines`.)
- [ ] **Step 2: confirm FAIL** (unknown option / no script).
- [ ] **Step 3: implement `--lines`** — `git -C "$dir" diff --no-color --no-ext-diff` parsed in one awk pass tracking `+++ b/`, `@@ -o,+n` and counters; fragments are the line text minus the +/- sigil, control chars stripped (reuse the `agent-state:82-86` discipline), capped at 160 chars. Ordering: `git diff --stat` drives churn; a `*test*`/`tests/*`/`*_test.*`/`*.test.*` name match drives the tests-first partition.
- [ ] **Step 4: green; Step 5: shellcheck; Step 6: commit** `feat: tmux-review --lines, the numbered diff rows a review anchors to`

### Task 2: Comment attach + markers (the fzf loop)

**Files:** Modify `scripts/tmux-review`; Test `tests/run.sh`

**Interfaces:**
- The loop (tmux-branch's `--print-query`/`--expect` pattern, `tmux-branch:213-243`): fzf over Task 1 rows, `--with-nth` hiding the machine fields, `--preview` = `tmux-open-path --preview-diff {1}` sized per `tmux-branch:202-205`.
- `alt-enter` attaches the typed query as a comment on the highlighted row → appended to the mktemp store as one revdiff block; the list `reload`s with a `●` marker column on commented rows (the `tmux-open-path:176-183` marker trick — prepend, never repaint).
- `ctrl-s` exits 20 (deliver), `esc`/`ctrl-c` exits 0 (discard, store deleted), plain enter on a row does nothing destructive (re-enters the loop; there is deliberately no accept-all).
- Store format (grammar, verbatim from the landscape report):
  ```
  ## scripts/tm:43 (+)
  > the quoted fragment
  use errors.Is() instead of direct comparison
  ```
  Body lines beginning `## ` get a leading space on write. A comment containing `??` is left untouched — the composer's header explains the convention to the agent.

- [ ] **Step 1: failing tests** — drive the picker in a private tmux server (send-keys, the `-t t:` colon convention from the jump tests): type a comment, alt-enter, assert the store file holds the block with path:line and the quoted fragment; assert the reloaded list shows `●` on that row; esc → store gone.
- [ ] **Step 2: FAIL; Step 3: implement; Step 4: green; Step 5: commit** `feat: alt-enter pins the typed comment to the diff line under the cursor`

### Task 3: Composer + `??` questions

**Files:** Modify `scripts/tmux-review`; Test `tests/run.sh`

**Interfaces:** `tmux-review --compose <store>` prints the final payload: a fixed header, then the blocks verbatim. Header text (exact):
```
Code review of your uncommitted changes. Address each comment; a comment
containing ?? is a question — answer it in chat, do not edit for it.
Comments are anchored as path:line with the original line quoted.
```

- [ ] **Step 1: failing golden test** — seed a store with two blocks (one with `??`), assert `--compose` output equals the golden text byte-for-byte (`is` on the full string).
- [ ] **Step 2: FAIL; Step 3: implement (cat with the header, nothing clever); Step 4: green; Step 5: commit** `feat: the composed review reads as one batch, questions marked ??`

### Task 4: Delivery — the paste that cannot mangle

**Files:** Modify `scripts/tmux-review`; Test `tests/run.sh`

**Interfaces:** `tmux-review --deliver <pane> <store>`:
1. Guards, in order, each a silent-ish refusal with `display-message`: pane id matches `%[0-9]*` (`agent-state:13-16`); `pane_current_command` matches `@cockpit_agents_glob` (review text in a shell would EXECUTE); store non-empty.
2. Delivery (the anti-patterns report's derived sequence — verify live in Step 1):
   ```bash
   tmux-review --compose "$store" | tmux load-buffer -b cockpit-review -
   tmux paste-buffer -p -r -d -b cockpit-review -t "$pane"
   ```
   `-p` bracketed, `-r` LF-not-CR (CR submits, and CR is what csi-u re-encodes), `-d` drop the buffer, default vis(3) sanitizing kept.
3. Payloads over 2048 bytes: write the composed review to `$XDG_RUNTIME_DIR/tmux-cockpit-review-${pane#%}.md` (0600, `${XDG_RUNTIME_DIR:-/tmp}` fallback) and paste only `Read and address every comment in <path> — review of your uncommitted changes; ?? comments are questions.` The file is the delivery vehicle; the agent deletes nothing, the OS tmpdir does.
4. NEVER append Enter. After the paste: `display-message "review pasted into <pane> — press Enter there to send"`.
5. Called from the popup path via `tmux run-shell -b` (popup teardown kills children, `tmux-open-path:381-386`).

- [ ] **Step 1: live-verify the mechanics FIRST** (this is the plan's known unknown): private server, pane running `exec -a claude bash`, deliver a 3-line store; assert `capture-pane` shows all three lines at the prompt UNEXECUTED (bash echoes the bracketed paste; nothing ran); assert a payload with a trailing newline still does not execute. If `-r` proves wrong against a `extended-keys-format csi-u` server config, document what was measured and switch the composer to join with LF minus trailing — the TEST is the arbiter.
- [ ] **Step 2: failing tests** — the guard set (non-agent pane refused; bad pane id refused; big payload → file exists 0600 + short pointer pasted); **Step 3: implement; Step 4: green; Step 5: commit** `feat: the review lands typed-but-unsent in the agent pane, one bracketed paste`

### Task 5: Entry points, fleet queue, binding, docs

**Files:** Modify `scripts/tmux-review`, `cockpit.tmux`, `README.md`; Test `tests/run.sh`

**Interfaces:**
- No-arg resolution: if the CLIENT's active pane (ask tmux, `tmux-branch:34-40` — not `$TMUX_PANE`) matches the agents glob → review it. Otherwise: fzf queue of every agent pane with a dirty tree — rows `pane\tsession:win title\t±stat`, preview = `--preview-diff` of the pane's cwd, ordered dirty-first. Picking one enters Task 2's loop against it. (This is landscape-gap #1: nothing else answers "which of my agents needs my eyes".)
- Binding in `cockpit.tmux` beside the jump binding: `@cockpit_review_key`, default `R`, literal `off` disables; popup via the `$PW`/`$PH` convention, title ` review · #{b:pane_current_path} `.
- README: "Review the agent's changes" section — the flow, the grammar, the `??` convention, the two-step send, and the WHAT-NOT-TO-DO rationale in the repo's voice (each rule with its measured/community source, one line each). Key row beside `prefix J`'s.

- [ ] **Step 1: failing tests** — from a non-agent pane the queue lists exactly the dirty agent panes; `@cockpit_review_key off` leaves `R` unbound; binding present by default. **Step 2: FAIL; Step 3: implement; Step 4: green + full `make check`; Step 5: commit** `feat: prefix R reviews the agent's diff like a boss, fleet queue included`

### Task 6: Integration gate (session lead)

- [ ] `make check` green in the worktree across 2 consecutive runs; leak sweep (no `cockpit-*` sockets/processes left); merge `feat/review-flow` to master with a green gate; push (publishing to origin is standing-approved for this repo).

## Deferred by design (future, not now)
- revdiff/difit handoff (`@cockpit_reviewer` option): shell out to a real TUI reviewer and re-use our delivery path for its stdout annotations — the grammar already matches.
- `^R` secondary entry inside tmux-open-path's picker (research option C).
- Turn-boundary auto-nudge (needs-review as a `@panestate` value) — touches the hook path; separate spec.
- git-notes durable review store (landscape gap #3) — contradicts invariant 3; only with a written case.

## Self-review record
- Spec coverage: every WHAT-NOT-TO-DO rule maps to a constraint or a task step; both research reports' recommendations are either adopted (grammar, batching, paste mechanics, fleet queue, tests-first ordering) or explicitly deferred with the reason.
- Known unknown named in-plan: Task 4 Step 1 live-verifies the paste mechanics before anything builds on them.
- Type consistency: the store format, exit codes (20 = deliver), option names (`@cockpit_review_key`), and subcommand names are used identically across tasks.
