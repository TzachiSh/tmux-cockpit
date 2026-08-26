# Review Redesign — Phase 3 (the loop) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** close the review loop — the fleet says *which agent finished with unreviewed changes* (`◆N`), a review written while the agent is busy reaches it at its next turn boundary with zero keystrokes, `^A` approves so the next review shows only the delta, and an agent can ask for a review itself (`/cockpit-review`).

**Architecture:** one new one-shot script, `scripts/review-gate`, is a second `Stop` hook beside `agent-state done`: it hands over a queued review (exit 2 + stderr, the documented way a Stop hook makes Claude continue), flags `@cockpit_review pending` when the pane's diff against its review base is non-empty, and stashes the conversation title/session id the hook payload already carries. The *review base* becomes a function of four cases (`refs/cockpit/reviewed/<branch>` → merge-base with upstream/default → `HEAD`), and every diff — the picker's rows, the fleet's churn, the hook's flag — runs against a **temp copy of the index with `git add -N -A`** so staged, unstaged and untracked changes are one set for any base and an approved checkpoint tree diffs exactly (measured: a real-index `git diff <ref>` reports an approved-but-untracked file as *deleted*; the temp index does not — and the eight `--lines` goldens stay byte-identical, 12/12 runs). `deliver()` gains one arm: a busy pane gets the composed review spooled beside the store and `@cockpit_review_spool` armed; the paste path is unchanged for an idle pane. `◆N` rides in the existing `@cockpit_fleet` string — no new global option, no new format token, the freeze rig sees nothing new.

**Tech Stack:** bash 4+, tmux ≥ 3.3, fzf 0.44.1 (`--expect`, `--sync` + `start:pos(N)`, `execute`), git ≥ 2.20 (`add -N`, `update-ref` on a tree object, `rev-parse --absolute-git-dir`), POSIX awk (mawk here), `tac` (GNU; `tail -r` fallback). Claude Code hooks: `Stop` exit 2 → "prevents Claude from stopping, shows stderr to Claude" (hooks reference, exit-code table); every payload carries `session_id` and `transcript_path`. No jq, no new dependency.

**Spec:** `docs/superpowers/plans/2026-08-24-review-redesign-goal.md` §1 phase 3 row, §2 rig, §3 (never `send-keys Enter`, never paste into `needs-input`, no fifth `@panestate`, no pane walk on a redraw), §8 answers in `2026-08-25-review-section8-decisions.md` (#1 dir+ref yes, #2 popup default + `@cockpit_review_window`, #3 `prefix r` = always the queue, #6 plan-mode gated, #7 peter-fork *pieces*); `docs/superpowers/research/2026-08-24-review-redesign.md` §7.2–7.4; lens reports `when-how.md` §1–§4, §6 and `ai.md` §7.A, §7.C, §7.F; `tcsm.md` §3.1 (ai-title); `tmuxai-ux.md` §8 (confirm). The ponytail audit below records where this plan deliberately ships less than those asked for.

**Branch:** `feat/phase3-loop`, worktree `~/.cache/cockpit-wt/phase3-loop`, off master `c94302f` (phase 2 merged as `9d93f11`). All work happens there. Gate on master at branch time: `make check` 614 passed / 0 failed; freeze rig 712 visits on 121 panes (5.9 walks — the pre-existing `tmux.conf` marker walk, project memory `freeze-precompute-pattern`).

## Global Constraints

- No shell on a status redraw; hooks never block the agent (`review-gate`: bounded `read -t`, `"timeout": 5` in the hook entry, fail-open, no `jq`); every counter is precomputed at transition time with compare-then-set (`scripts/agent-state`'s batch; project memory `freeze-precompute-pattern`). Phase 3 adds **no** global option and **no** new format token: `◆N` is a third glyph in `@cockpit_fleet`; `@cockpit_title` is a per-pane option a window format may read for its own active pane (no walk).
- State = per-pane tmux options that die with the pane (`@cockpit_review` ∈ {`pending`,`sent`}, `@cockpit_review_spool`, `@cockpit_title`, `@cockpit_session`), **plus** the two owner-approved relaxations: the session dir under `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/review/<pane>-<cksum root>/` (0700; now lives until `^A` approve or an explicit discard) and one `refs/cockpit/reviewed/<branch>` per approved review (a tree object, written by `git update-ref`; it outlives the branch — purge with `git for-each-ref --format="%(refname)" refs/cockpit/reviewed | xargs -n1 git update-ref -d`, and Task 11's README says so).
- One-shot scripts; no daemon; no server; no port.
- Never `send-keys Enter` into an agent pane. Never paste into `needs-input` — phase 3 *spools* for it instead of refusing. Never steal focus.
- Batch delivery only; `^S` is the consent gesture (it now asks once). `↵` never approves anything. `^A` approves only when nothing is pinned.
- Every `tmux` command string quotes its paths (`shq` in scripts, `sq` in `cockpit.tmux`); post-popup work goes through `tmux run-shell -b`. **No nested `display-popup`** (`--request` opens exactly one).
- `git` reads on an agent's live tree take `--no-optional-locks` and **never touch the real index**: every diff runs with `GIT_INDEX_FILE` pointed at a `cp -p` copy (Task 1 pins this with a test on the real index's mtime).
- Pure module stays pure: `scripts/lib/review-store.sh` is unchanged by this phase.
- `bash -n` + `shellcheck -x` clean (`make lint` covers `scripts/review-gate` automatically — `SOURCES` globs `scripts/*`); `make check` is the gate, run **unpiped** (project memory `gate-pipelines-mask-failures`); `make test-only PAT='<block title ERE>'` for the TDD cycle.
- Tests run only on private `-L` sockets; every server a test starts is added to `SOCKS`; every mktemp goes through `mktmpd`/`mktmpf`; **no new fixed `sleep`** — waits use `until_cap` / `until_file` / an `RC=` marker poll; the two spawn settles the suite already keeps are the only exception and are commented as such.
- Commits: lowercase `feat:`/`fix:`/`test:`/`docs:` one-liners in the repo's voice (measured, past tense), ending with the session trailer.
- Work only in the worktree. Never edit `~/.tmux/plugins/tmux-cockpit` directly — another session shares it (project memory `concurrent-sessions-shared-worktree`). Fail-first runs that need a pristine copy use a temp copy, not `git stash` (project memory `worktrees-share-git-stash-stack`).
- **Ponytail governs the build** (the `ponytail` skill, level full): read the whole flow first, then the shortest diff that works; no rename for its own sake, no abstraction with one caller, deletion over addition; a deliberate ceiling gets a `# ponytail:` comment naming the upgrade path. A worker who finds a shorter correct diff takes it and says so in the summary.

## Ponytail audit — what the lenses asked for and this plan does not build

| asked for | verdict | why |
|---|---|---|
| `@cockpit_reviews` global option + `#{agent_reviews}` token (when-how §6.2) | **replace** | `◆N` is appended to the `@cockpit_fleet` string agent-state already precomputes; one more `#{n:…}` in the same batched `display-message`, zero new options, zero new tokens, the rig's inventory unchanged. |
| `@cockpit_review_n` (changed-line count per pane) | **skip** | no reader in this phase; the queue already shows `±`. |
| `@cockpit_review_gate on` (a Stop that blocks with *no* review to hand over) | **skip** | the drain already blocks exactly when there is a review; a Stop that blocks on nothing is the fleet-hostile mode the lens itself called opt-in-and-documented-against. Add when someone asks for it. |
| `@cockpit_review_base` global (auto/worktree/branch) | **replace** with `^B` | the base is a property of *one review*: `^B` cycles the modes that hold and `<store>.base` remembers the choice for that session dir; no global to explain. |
| `.git/cockpit/` as the spool | **replace** | the session dir exists, is 0700 and already holds the store; the spool is `<store>.deliver` beside it. |
| drop the 2048-byte pointer fork | **keep the fork** | on the *paste* path the text must fit a prompt and the file must outlive the popup and its dir (the human presses Enter later), so the `$XDG_RUNTIME_DIR` mktemp stays; on the *Stop* path stderr carries the whole text and no pointer exists. |
| `transcript_path`/`session_id` stashed on SessionStart/UserPromptSubmit (tcsm §3.1) | **replace** | `running` fires per tool call — nothing reads stdin there. `review-gate` reads its own Stop payload (bounded) and stashes `@cockpit_session` and the newest `ai-title` as `@cockpit_title` — once per turn, off the hot path. |
| sort comments by path/line before compose (ai §7.A) | **skip** | `compose` is `cat`; the close-out line keys on `path:line` whatever the order. |
| `↵` drills into the hunk (when-how §6.3) | **skip** | phase 4's hunk-level rows; `↵` keeps its contract (pin). |
| `^X` = discard the batch with confirm | **replace** with a third row in esc's picker | esc now means *leave, keep the batch*; the same picker offers `discard N comment(s)` explicitly. One picker, three rows. |
| plan-mode review (`PreToolUse` on `ExitPlanMode`, §8 #6 "yes, gated") | **defer to phase 4** | three mechanisms for one opt-in: JSON-unescaping `tool_input.plan` (jq on a hook path), a non-diff row mode (a plan is a document, not a diff), and detecting `revdiff-planning` with no documented API for it. Phase 4's revdiff backend brings `revdiff-planning`, which *is* that hook — `@cockpit_reviewer=revdiff` gets plan review for free without a colliding matcher. |
| `PreToolUse Bash(git commit *)` gate, `--seed`, AI first pass | **phase 4** | per the master plan. |
| pre-warm the queue's git numbers at the `done` transition (roadmap) | **skip** | the queue scans 20 roots in parallel under a second already; a precomputed number is another option with one reader. |
| `--purge` | **skip** | `rm -rf ~/.local/state/tmux-cockpit/review` is the purge and the README says so; `prefix i` shows the dir. |
| the session key is pane-id + root (two servers with the same `%N` share a dir) | **skip, noted** | two tmux servers reviewing the same worktree from the same pane number is a test rig, not a workflow; `--info` names the dir. |
| confirm-before-deliver `[y/N/e]` (tmuxai-ux §8) | **keep, as a picker** | the loop already owns one picker for esc; `^S` gets the same shape: `deliver N → pane` / `edit the batch first` / `keep reviewing`. |
| `prefix i` info popup | **keep** | `--info` is ~25 lines of `printf`; it is also where the session dir, the base and the queued spool become visible. |
| `@cockpit_review_window on` | **keep** | six lines in `cockpit.tmux`: `new-window` instead of `display-popup` at the one binding site. |

---

### Task 0: Worktree (done) and the branch

**Files:** none.

- [ ] **Step 1: Confirm the worktree**

Run: `cd ~/.cache/cockpit-wt/phase3-loop && git status --short --branch && git log --oneline -1`
Expected: `## feat/phase3-loop` and `c94302f`. If the worktree is missing: `git -C ~/.tmux/plugins/tmux-cockpit worktree add ~/.cache/cockpit-wt/phase3-loop -b feat/phase3-loop master`.

- [ ] **Step 2: Commit this plan**

```bash
git add docs/superpowers/plans/2026-08-25-review-phase3.md
git commit -m "docs: phase 3 plan — the loop: review-gate on Stop, the temp-index base, ^A/^B, the queue as prefix r, /cockpit-review"
```

---

### Task 1: `tree_diff` on a temp index — one diff for staged, unstaged and untracked, against any base

**Files:**
- Modify: `scripts/tmux-review` — `tree_diff()` (the `# --- the base every uncommitted view diffs against` block), `lines_mode()` (delete the untracked synthesis: the `ulist=` pipeline, the numstat append, the `wc -c` byte charge, `[ -n "$diff$ulist" ]`), `ulist_operands()` and `awk_over()` (delete — no caller left).
- Test: `tests/run.sh` — a new block after `== tmux-review --lines: output is byte-identical to the committed goldens ==`.

**Interfaces:**
- Produces: `tree_diff <dir> [git-diff args...]` — unchanged signature; now runs `git diff <base> "$@"` with `GIT_INDEX_FILE` set to a `cp -p` copy of the real index after `git add -N -A`, so untracked files are `+` rows of the same diff. `<base>` is `HEAD` in this task; Task 2 makes it `review_base`. The unborn-HEAD fallback (`diff`, else `diff --cached`) keeps its shape.

- [ ] **Step 1: Write the failing test — the real index is never touched**

Append to `tests/run.sh`, directly after the goldens block's `done` (before `echo "== tmux-open-path --preview-diff: a nested file's diff is not lost to path-doubling =="`):

```bash
echo "== tmux-review --lines: the diff never writes the agent's index =="
# Every diff now runs against a COPY of the index (git add -N -A on the copy
# folds untracked files into the same diff). The copy is the whole point:
# an `add -N` against the real index would rewrite the agent's own index
# under it mid-commit. Pinned by the index file's mtime and size.
RI=$(mktmpd); "$DIR/tests/fixtures/review/mkfix.sh" untracked "$RI" >/dev/null 2>&1
ri_stamp() { stat -c '%Y %s' "$RI/.git/index" 2>/dev/null || stat -f '%m %z' "$RI/.git/index"; }
touch -d '2020-01-01 00:00:00' "$RI/.git/index" 2>/dev/null || touch -t 202001010000 "$RI/.git/index"
ri_before=$(ri_stamp)
is "untracked rows still come out of one diff" "2" \
   "$("$DIR/scripts/tmux-review" --lines "$RI" | grep -c $'^new.txt\t')"
is "and the real index is byte- and mtime-identical afterwards" "$ri_before" "$(ri_stamp)"
is "no intent-to-add entry leaked into the real index" "" \
   "$(git -C "$RI" ls-files -s -- new.txt)"
unset -f ri_stamp
rm -rf "$RI"
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd ~/.cache/cockpit-wt/phase3-loop && make test-only PAT='never writes the agent'`
Expected: `untracked rows still come out of one diff` PASS (the old synthesis emits them), the other two PASS too — this block is **not** fail-first on the current code; it is the tooth that bites if a worker ever drops the copy. Proceed; Step 4 is the change that the goldens gate.

- [ ] **Step 3: Replace `tree_diff` and delete the synthesis**

Replace the whole `tree_diff()` function (keep the comment block above it, then rewrite its last paragraph as below):

```bash
# --- the base every review diffs against --------------------------------------
# ONE diff, whatever the base: staged, unstaged AND untracked changes come out
# of a single `git diff <base>`, because the diff runs against a COPY of the
# index that `git add -N -A` has extended with an intent-to-add entry for
# every untracked file. -N records the path only (no content is hashed), and
# a plain `git diff` then shows such a file as a new file against the working
# tree -- the documented use of intent-to-add. Measured on the eight goldens:
# byte-identical to the ls-files/awk synthesis this replaces, 12/12 runs.
#
# The copy is not optional: an `add` against the real index would rewrite the
# agent's own index under it. `cp -p`, not `cp`: git's racy-timestamp guard
# compares each entry's mtime with the INDEX FILE's mtime, and a fresh copy
# with a fresh mtime made git trust a same-size, same-second edit as clean --
# reproduced 1 run in 3 on the `spaces` fixture, 0 in 12 with -p. An unborn
# repo has no index to copy; the rm lets git start one from nothing.
#
# It also makes the approved checkpoint (^A, a tree that INCLUDES untracked
# files) diff exactly: against the real index `git diff <tree>` reports a
# file present in the tree but untracked here as DELETED (reproduced); with
# the -N entry it compares the file's content instead.
#
# --no-optional-locks on every call, the same discipline fleet_scan_root()
# uses: even a read can take index.lock, and this runs against a repo an
# AGENT is working in.
tree_diff() {
    local dir=$1; shift
    local idx gd
    idx=$(mktemp) || return 1
    gd=$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null) || { rm -f -- "$idx"; return 1; }
    cp -p -- "$gd/index" "$idx" 2>/dev/null || rm -f -- "$idx"
    GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks add -N -A 2>/dev/null
    if git -C "$dir" --no-optional-locks rev-parse -q --verify HEAD >/dev/null 2>&1; then
        GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks -c core.quotePath=false \
            diff --no-color --no-ext-diff HEAD "$@" 2>/dev/null
    else
        local out
        out=$(GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks -c core.quotePath=false diff --no-color --no-ext-diff "$@" 2>/dev/null)
        [ -n "$out" ] || out=$(GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks -c core.quotePath=false diff --no-color --no-ext-diff --cached "$@" 2>/dev/null)
        printf '%s\n' "$out"
    fi
    rm -f -- "$idx"
}
```

Then in `lines_mode()`:
1. Delete from `# Untracked files carry no diff at all` through the `fi` that closes `if [ -n "$ulist" ]; then numstat=...` (the whole `ulist=` block and its comment).
2. In the byte-cap branch, delete the `if [ -n "$ulist" ]; then bytes=$(ulist_operands ... ) ... fi` block and its comment ("Untracked files are charged first and exactly…"); `bytes` stays initialised to 0 so `head -c $(( bytecap - bytes + 1 ))` is unchanged in shape.
3. Change `[ -n "$diff$ulist" ] || return 0` to `[ -n "$diff" ] || return 0`.
4. Delete the "second awk over untracked files" block — the one that appends rows for `$ulist` after the diff parse (search for `awk_over "$dir" "$ulist"` after the `raw=$(printf '%s\n' "$diff" | awk '` program; every use of `$ulist` goes).
5. Delete `ulist_operands()` and `awk_over()` and their comment blocks (`grep -n 'ulist\|awk_over' scripts/tmux-review` must print nothing when done).
6. Rewrite the `lines_mode` comment paragraph that begins `# Untracked files carry no diff at all` … `0.008s.` to one sentence: `# Untracked files are rows of the same diff: see tree_diff's intent-to-add copy.`

- [ ] **Step 4: Run the goldens, the new block, and every `--lines`/`--rows`/`preview-diff` block**

Run: `make test-only PAT='tmux-review --lines|tmux-review --rows|never writes|preview-diff'`
Expected: every line PASS, including all eight `golden <name>` lines. Run the goldens block five times (`for i in 1 2 3 4 5; do make test-only PAT='byte-identical'; done`) — every run identical.

- [ ] **Step 5: Timing has not moved**

Run: `make test-only PAT='giant diff caps|builds rows without a fork|20 panes under a second'`
Expected: PASS; note the measured numbers from the block output in the commit message if the blocks print them.

- [ ] **Step 6: Commit**

```bash
git add scripts/tmux-review tests/run.sh
git commit -m "feat: every review diff runs on an intent-to-add copy of the index — untracked files are rows of the one diff, the agent's index is never written; goldens byte-identical"
```

---

### Task 2: The review base — `review_base`, `--delta`, `^B`, and a committing agent stays in the queue

**Files:**
- Modify: `scripts/tmux-review` — add `review_base()`, `next_base()`, `delta_mode()` below `tree_diff()`; `tree_diff()` uses `review_base`; `run_loop()` gains `ctrl-b` and a base line in the header; `fleet_scan_root()` derives `dirty` from the numstat; `usage()`; the dispatcher gains `--delta`.
- Test: `tests/run.sh` — one new block `== tmux-review: the review base is since-last-review, else the branch, else uncommitted ==` after Task 1's block; the fleet block `== tmux-review --fleet: from a non-agent pane, the queue lists exactly the dirty agent panes ==` gains one fixture.

**Interfaces:**
- Produces: `review_base <dir> [auto|since-review|branch|uncommitted]` → prints `<rev>\t<label>\t<mode-that-held>`; never called on an unborn HEAD. `next_base <dir> <mode>` → the next mode that holds, cycling. `tree_diff` reads `$COCKPIT_REVIEW_BASE` (a mode, default `auto`). `tmux-review --delta <pane>` exits 0 when the pane's diff against its review base is non-empty, 1 otherwise (Task 5's hook calls it). `<store>.base` holds the mode a review chose with `^B` (Task 3's `--info` and Task 5's `--delta` read it).

- [ ] **Step 1: Write the failing tests**

Append after Task 1's block:

```bash
echo "== tmux-review: the review base is since-last-review, else the branch, else uncommitted =="
# revdiff's auto-detect, one function: a checkpoint ref wins, then the
# merge-base with the upstream/default branch when it is not HEAD itself,
# then HEAD (today's view). ^B cycles the modes that hold. Exercised through
# --delta and --lines so the tests see what a reviewer sees.
RB=$(mktmpd)
(
  cd "$RB" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\n' > f; git add -A; git commit -q -m init
  git checkout -q -b feat
  printf 'a\nCOMMITTED-ON-FEAT\n' > f; git commit -q -am feat
) >/dev/null 2>&1
is "a clean feature branch reviews its commits against main" "1" \
   "$("$DIR/scripts/tmux-review" --lines "$RB" | grep -c 'COMMITTED-ON-FEAT')"
printf 'a\nCOMMITTED-ON-FEAT\nUNCOMMITTED\n' > "$RB/f"
is "branch + uncommitted are one set" "2" \
   "$("$DIR/scripts/tmux-review" --lines "$RB" | grep -c 'COMMITTED-ON-FEAT\|UNCOMMITTED')"
is "COCKPIT_REVIEW_BASE=uncommitted narrows it to HEAD" "1" \
   "$(COCKPIT_REVIEW_BASE=uncommitted "$DIR/scripts/tmux-review" --lines "$RB" | grep -c 'COMMITTED-ON-FEAT\|UNCOMMITTED')"
# A checkpoint: the tree as it stands now, under the ref ^A will write.
(
  cd "$RB" || exit
  I=$(mktemp); cp -p .git/index "$I"; GIT_INDEX_FILE=$I git add -A
  git update-ref refs/cockpit/reviewed/feat "$(GIT_INDEX_FILE=$I git write-tree)"; rm -f "$I"
) >/dev/null 2>&1
is "with a checkpoint at the current tree there is nothing to review" "" \
   "$("$DIR/scripts/tmux-review" --lines "$RB")"
printf 'a\nCOMMITTED-ON-FEAT\nUNCOMMITTED\nSINCE\n' > "$RB/f"
is "and after one more edit only that edit is a row" "SINCE" \
   "$("$DIR/scripts/tmux-review" --lines "$RB" | cut -f3)"
is "COCKPIT_REVIEW_BASE=branch still shows the whole branch" "3" \
   "$(COCKPIT_REVIEW_BASE=branch "$DIR/scripts/tmux-review" --lines "$RB" | grep -c .)"
rm -rf "$RB"
```

And in the fleet block, after `FQ_NONAGENT=$(mktmpd); mk_git_dir "$FQ_NONAGENT" 1   # dirty, but not an agent`, add:

```bash
FQ_AHEAD=$(mktmpd)                                   # agent, clean tree, commits ahead of main -- wanted
(
  cd "$FQ_AHEAD" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init
  git checkout -q -b feat; printf 'b\n' >> f; git commit -q -am feat
) >/dev/null 2>&1
```

after the `NONAGENT_PANE=` line:

```bash
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_AHEAD" 'exec -a claude bash' >/dev/null 2>&1
AHEAD_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:4 -F '#{pane_id}')
```

and after the existing `the dirty agent pane is in the queue` assertion:

```bash
case "$fqout" in
    *"$AHEAD_PANE"*"+1 -0"*) ok "an agent that committed on its branch is still in the queue, with its branch churn" ;;
    *) bad "an agent that committed on its branch is still in the queue, with its branch churn" "$AHEAD_PANE with +1 -0" "$fqout" ;;
esac
```

- [ ] **Step 2: Run them to see them fail**

Run: `make test-only PAT='review base is since|queue lists exactly'`
Expected: `a clean feature branch reviews its commits against main` FAIL (got 0), `with a checkpoint…` FAIL, `an agent that committed…` FAIL; the rest may pass by accident.

- [ ] **Step 3: Implement**

Below `tree_diff()` add:

```bash
# --- which base ---------------------------------------------------------------
# Four cases, the first that holds (when-how lens §2.3; revdiff's auto-detect):
#   since-review  refs/cockpit/reviewed/<branch> exists (^A wrote it)  -> only what moved since
#   branch        merge-base of the upstream (else origin/HEAD, else main/master) and HEAD,
#                 when that is not HEAD itself                          -> the branch + uncommitted
#   uncommitted   HEAD                                                  -> today's view
# MODE forces one; a forced mode that does not hold falls through to the next,
# so the printed mode is the one that HELD -- next_base relies on that.
# Prints "<rev>\t<label>\t<mode>". Never called on an unborn HEAD.
review_base() {               # <dir> [auto|since-review|branch|uncommitted]
    local dir=$1 mode=${2:-auto} br ref up mb head
    br=$(git -C "$dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
    ref="refs/cockpit/reviewed/${br:-HEAD}"
    case "$mode" in auto|since-review)
        if git -C "$dir" --no-optional-locks rev-parse -q --verify "$ref" >/dev/null 2>&1; then
            printf '%s\tsince last review\tsince-review\n' "$ref"; return 0
        fi ;;
    esac
    case "$mode" in auto|since-review|branch)
        up=$(git -C "$dir" --no-optional-locks rev-parse -q --verify --abbrev-ref '@{u}' 2>/dev/null) \
        || up=$(git -C "$dir" --no-optional-locks symbolic-ref -q --short refs/remotes/origin/HEAD 2>/dev/null) \
        || { for up in main master; do
                 git -C "$dir" --no-optional-locks rev-parse -q --verify "$up" >/dev/null 2>&1 && break
                 up=
             done; }
        if [ -n "$up" ]; then
            mb=$(git -C "$dir" --no-optional-locks merge-base "$up" HEAD 2>/dev/null)
            head=$(git -C "$dir" --no-optional-locks rev-parse HEAD 2>/dev/null)
            if [ -n "$mb" ] && [ "$mb" != "$head" ]; then
                printf '%s\tvs %s\tbranch\n' "$mb" "$up"; return 0
            fi
        fi ;;
    esac
    printf 'HEAD\tuncommitted\tuncommitted\n'
}

# The mode after CUR among those that hold, wrapping -- ^B's cycle.
next_base() {                 # <dir> <current-mode>
    local dir=$1 cur=$2 m first="" prev="" out
    for m in since-review branch uncommitted; do
        out=$(review_base "$dir" "$m"); [ "${out##*$'\t'}" = "$m" ] || continue
        [ -n "$first" ] || first=$m
        [ -n "$prev" ] && [ "$prev" = "$cur" ] && { printf '%s\n' "$m"; return 0; }
        prev=$m
    done
    printf '%s\n' "$first"
}

# Exit 0 when PANE's tree differs from its review base -- the one question the
# Stop hook asks. Reads the mode the pane's own review chose with ^B, if any.
delta_mode() {                # <pane>
    local pane=$1 dir root sess mode
    dir=$(tmux display-message -p -t "$pane" '#{pane_current_path}' 2>/dev/null)
    [ -n "$dir" ] || return 1
    root=$(git_root "$dir")
    git -C "$root" --no-optional-locks rev-parse --is-inside-work-tree >/dev/null 2>&1 || return 1
    sess=$(session_dir "$pane" "$root")
    mode=$(cat -- "$sess/comments.md.base" 2>/dev/null)
    export COCKPIT_REVIEW_BASE=${mode:-auto}
    [ -n "$(tree_diff "$root" --numstat)" ]
}
```

In `tree_diff()`, replace the `if git … rev-parse -q --verify HEAD` branch's diff call so the base is resolved:

```bash
    if git -C "$dir" --no-optional-locks rev-parse -q --verify HEAD >/dev/null 2>&1; then
        local base
        base=$(review_base "$dir" "${COCKPIT_REVIEW_BASE:-auto}"); base=${base%%$'\t'*}
        GIT_INDEX_FILE=$idx git -C "$dir" --no-optional-locks -c core.quotePath=false \
            diff --no-color --no-ext-diff "$base" "$@" 2>/dev/null
```

In `fleet_scan_root()`, replace the `st=$(git … status --porcelain)` / `if [ -n "$st" ]; then dirty=1; numstat=…` shape so dirty derives from the diff (the branch case has a clean `status`):

```bash
    st=$(git -C "$root" --no-optional-locks status --porcelain 2>/dev/null)
    numstat=$(tree_diff "$root" --numstat)
    if [ -n "$numstat" ]; then
        dirty=1
        add=0; del=0
```

(everything after `add=0; del=0` — the numstat loop, the `??` count off `$st`, `stat`, `churn` — is unchanged; the `else` arm too.)

In `run_loop()`:
1. After `local dir=$1 store=$2` … `command -v fzf` guard, add:

```bash
    # The base this review diffs against: ^B's choice, remembered beside the
    # store so a resumed review keeps it; exported because --rows and --resnap
    # re-exec through fzf and inherit this environment.
    local base_line label mode
    COCKPIT_REVIEW_BASE=$(cat -- "$store.base" 2>/dev/null); export COCKPIT_REVIEW_BASE
    set_base_line() {
        if git -C "$dir" --no-optional-locks rev-parse -q --verify HEAD >/dev/null 2>&1; then
            base_line=$(review_base "$dir" "${COCKPIT_REVIEW_BASE:-auto}")
            label=$(cut -f2 <<<"$base_line"); mode=$(cut -f3 <<<"$base_line")
        else
            label=uncommitted; mode=uncommitted
        fi
    }
    set_base_line
```

2. Change `--expect=ctrl-s` to `--expect=ctrl-s,ctrl-b` and the header to:

```bash
                  --header=$'↵ pin the typed comment   ^S deliver   esc discard (asks first)\n^E editor  ^X delete  ^L list  ^R refresh  ^B base  ^/ preview  alt-u/alt-d scroll\n'"base: $label"
```

(the header is now built per iteration, so `--header=` must be inside the `while :` loop — it already is.)

3. After `key=$(sed -n 2p <<<"$out")`, before the `ctrl-s` test:

```bash
        if [ "$key" = ctrl-b ]; then
            mode=$(next_base "$dir" "$mode")
            printf '%s\n' "$mode" > "$store.base"
            COCKPIT_REVIEW_BASE=$mode; export COCKPIT_REVIEW_BASE
            set_base_line
            snapshot "$dir" "$store"
            rows_out=$(rows_mode "$dir" "$store")
            [ "$(printf '%s\n' "$rows_out" | grep -c .)" -gt 0 ] || \
                tmux display-message "tmux-review: nothing to review against $label" 2>/dev/null
            continue
        fi
```

Add to `usage()` after the `--list` line: `  tmux-review --delta <pane>            exit 0 when <pane>'s tree differs from its review base`, and `^B base` to the key line. Dispatcher: `--delta) shift; delta_mode "${1:-}" ;;`.

- [ ] **Step 4: Run the tests**

Run: `make test-only PAT='review base is since|queue lists exactly|tmux-review --lines|tmux-review --loop'`
Expected: every line PASS (the goldens included — the fixtures have no upstream and sit on `main`, so `review_base` falls to `HEAD` for them).

- [ ] **Step 5: `^B` live**

Extend the new block, before `rm -rf "$RB"`: drive the picker in a private server and press `^B`:

```bash
if need_tmux "review base ^B"; then :; else
RBS="cockpit-rbs-$$"; SOCKS="$SOCKS $RBS"; RBSTATE=$(mktmpd)
"$TMUX_REAL" -L "$RBS" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RB" 'exec bash' >/dev/null 2>&1
RBP=$("$TMUX_REAL" -L "$RBS" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RBS" send-keys -t "$RBP" \
    "XDG_STATE_HOME=$(printf '%q' "$RBSTATE") '$DIR/scripts/tmux-review' --loop $(printf '%q' "$RB") $(printf '%q' "$RBSTATE/c.md")" Enter
until_cap "$RBS" "$RBP" '*base: since last review*' && ok "the header names the base" || bad "the header names the base" "base: since last review" "$("$TMUX_REAL" -L "$RBS" capture-pane -p -t "$RBP")"
"$TMUX_REAL" -L "$RBS" send-keys -t "$RBP" C-b
until_cap "$RBS" "$RBP" '*base: vs main*' && ok "^B cycles to the branch base and re-snapshots" || bad "^B cycles to the branch base and re-snapshots" "base: vs main" "$("$TMUX_REAL" -L "$RBS" capture-pane -p -t "$RBP")"
is "the choice is remembered beside the store" "branch" "$(cat "$RBSTATE/c.md.base")"
"$TMUX_REAL" -L "$RBS" send-keys -t "$RBP" Escape
"$TMUX_REAL" -L "$RBS" kill-server >/dev/null 2>&1
fi
```

Run: `make test-only PAT='review base is since'` — every line PASS.

- [ ] **Step 6: Commit**

```bash
git add scripts/tmux-review tests/run.sh
git commit -m "feat: the review base is the checkpoint, else the branch, else HEAD — a committing agent stays in the queue; ^B cycles it, --delta answers the hook"
```

---

### Task 3: `^A` approve, the delivered archive, and `↺` on re-review

**Files:**
- Modify: `scripts/tmux-review` — add `approve()` and `done_store()`; `mark_rows()` takes the delivered file; `rows_mode()` passes it; `run_loop()` gains `ctrl-a` and the `↺` count in the header; `review_pane()` handles rc 21 and uses `--done`; `keep_hint()`'s re-deliver command; `usage()`; dispatcher gains `--done`.
- Test: `tests/run.sh` — new block `== tmux-review --loop: ^A approves — the ref is the tree, the dir is gone, the next review is the delta ==`; the phase-2 block `== tmux-review: a delivered review's comment store does not outlive the popup that made it ==` changes its last assertion (the dir now keeps `delivered.md`).

**Interfaces:**
- Produces: `approve <dir>` writes `refs/cockpit/reviewed/<branch>` to a tree of the working tree (tracked + untracked, real `add -A` on a temp index). `done_store <store>` (`--done`) appends the store to `<dir>/delivered.md`, removes the store and `.rows`, `rmdir`s the dir if empty. `mark_rows <store> <delivered> <rows-file>` — third file; a row whose key is only in `<delivered>` renders `↺`. `run_loop` exits **21** on approve; `review_pane` on 21 unsets `@cockpit_review`, runs `agent-state recount` (Task 4 — until then the call is a harmless unknown keyword, agent-state exits 0) and removes the session dir.

- [ ] **Step 1: Write the failing test**

Append after Task 2's block:

```bash
echo "== tmux-review --loop: ^A approves — the ref is the tree, the dir is gone, the next review is the delta =="
if need_tmux "review ^A"; then :; else
RA="cockpit-ra-$$"; SOCKS="$SOCKS $RA"; RASTATE=$(mktmpd); RADIR=$(mktmpd)
(
  cd "$RADIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go; printf 'brand new\n' > new.txt
) >/dev/null 2>&1
"$TMUX_REAL" -L "$RA" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RADIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RA" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
RAAP=$("$TMUX_REAL" -L "$RA" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RA" set-option -p -t "$RAAP" @cockpit_review pending >/dev/null 2>&1
ra_shell() { local p i; p=$("$TMUX_REAL" -L "$RA" split-window -t t -P -F '#{pane_id}' 'exec bash'); for i in $(seq 1 40); do [ "$("$TMUX_REAL" -L "$RA" display-message -p -t "$p" '#{pane_current_command}')" = bash ] && break; sleep 0.1; done; printf '%s' "$p"; }
RACTL=$(ra_shell)
RASESS="$RASTATE/tmux-cockpit/review/${RAAP#%}-$(printf '%s' "$RADIR" | cksum | cut -d' ' -f1)"
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" "XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $RAAP" Enter
until_cap "$RA" "$RACTL" '*review>*'
# One pinned comment: ^A must refuse -- approve means "nothing more to say".
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" "not yet"; until_cap "$RA" "$RACTL" '*not yet*'
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" Enter; until_file "$RASESS/comments.md" '*not yet*'
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" C-a
until_cap "$RA" "$RACTL" '*deliver or discard the 1 pinned*' && ok "^A with a pinned comment refuses and says why" || bad "^A with a pinned comment refuses and says why" "deliver or discard the 1 pinned comment(s) first" "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RACTL")"
is "and nothing was approved" "" "$(git -C "$RADIR" rev-parse -q --verify refs/cockpit/reviewed/main 2>/dev/null)"
# Deliver it (the pane is idle: a paste), then re-open and approve.
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" C-s
until_cap "$RA" "$RACTL" '*deliver 1 comment*'; "$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" Enter
until_cap "$RA" "$RAAP" '*not yet*' && ok "the batch lands in the agent pane" || bad "the batch lands in the agent pane" "not yet" "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RAAP")"
until_file "$RASESS/delivered.md" '*not yet*' && ok "a delivered batch is archived as delivered.md" || bad "a delivered batch is archived as delivered.md" "not yet in delivered.md" "$(ls "$RASESS" 2>/dev/null)"
is "the store itself is gone" "absent" "$([ -e "$RASESS/comments.md" ] && echo present || echo absent)"
RACTL2=$(ra_shell)
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL2" "XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $RAAP" Enter
until_cap "$RA" "$RACTL2" '*↺src.go:3 (+)*' && ok "on re-open the delivered comment's row reads ↺" || bad "on re-open the delivered comment's row reads ↺" "↺src.go:3 (+)" "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RACTL2")"
until_cap "$RA" "$RACTL2" '*1 delivered earlier*' && ok "and the header counts it" || bad "and the header counts it" "↺ 1 delivered earlier" "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RACTL2")"
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL2" C-a
for i in $(seq 1 40); do [ -e "$RASESS" ] || break; sleep 0.1; done
is "^A removes the session dir" "gone" "$([ -e "$RASESS" ] && echo present || echo gone)"
is "^A clears the pane's review state" "" "$("$TMUX_REAL" -L "$RA" show-option -pqv -t "$RAAP" @cockpit_review)"
RAREF=$(git -C "$RADIR" rev-parse -q --verify refs/cockpit/reviewed/main 2>/dev/null)
is "^A wrote the checkpoint ref to a tree" "tree" "$(git -C "$RADIR" cat-file -t "$RAREF" 2>/dev/null)"
is "the tree holds the untracked file too" "1" "$(git -C "$RADIR" ls-tree -r "$RAREF" --name-only 2>/dev/null | grep -c '^new.txt$')"
is "nothing is left to review" "" "$("$DIR/scripts/tmux-review" --lines "$RADIR")"
# --delta asks tmux about the pane, so it runs on THIS server (run-shell), not the suite's.
is "--delta says so (exit 1)" "D=1" "$("$TMUX_REAL" -L "$RA" run-shell "XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' --delta $RAAP; echo D=\$?" 2>/dev/null | tail -1)"
printf 'brand new\nEDITED\n' > "$RADIR/new.txt"
is "an edit to the approved untracked file is the only row" "EDITED" "$("$DIR/scripts/tmux-review" --lines "$RADIR" | cut -f3)"
unset -f ra_shell
"$TMUX_REAL" -L "$RA" kill-server >/dev/null 2>&1
fi
```

Note the `deliver 1 comment` wait: Task 8 adds the `^S` confirm; **until Task 8 lands, replace that line with nothing** — `^S` delivers straight away. Task 8's Step 1 re-inserts it. (The block is written once, here, with the final shape, and Task 8 flips it on.)

- [ ] **Step 2: Run it to see it fail**

Run: `make test-only PAT='\^A approves'`
Expected: `^A with a pinned comment refuses` FAIL (no such key; the picker ignores it), and everything after fails or times out.

- [ ] **Step 3: Implement**

Add after `stale_keys()`:

```bash
# --- approve -------------------------------------------------------------------
# refs/cockpit/reviewed/<branch> = the tree the human just looked at, untracked
# files included (a real add -A into a temp index, then write-tree; content IS
# hashed here, once per approve). A ref to a tree object is what makes the
# since-review base a plain `git diff <ref>`; git update-ref accepts it and
# `git branch -D` / `worktree remove` are its GC. The pane's state is the
# caller's to clear -- this function knows the repo, not the pane.
approve() {                   # <dir>
    local dir=$1 idx gd br tree
    idx=$(mktemp) || return 1
    gd=$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null) || { rm -f -- "$idx"; return 1; }
    cp -p -- "$gd/index" "$idx" 2>/dev/null || rm -f -- "$idx"
    br=$(git -C "$dir" --no-optional-locks rev-parse --abbrev-ref HEAD 2>/dev/null)
    tree=$(GIT_INDEX_FILE=$idx git -C "$dir" add -A 2>/dev/null \
           && GIT_INDEX_FILE=$idx git -C "$dir" write-tree 2>/dev/null)
    rm -f -- "$idx"
    [ -n "$tree" ] && git -C "$dir" update-ref "refs/cockpit/reviewed/${br:-HEAD}" "$tree" 2>/dev/null
}

# After a delivery: the batch joins delivered.md beside the store (the ↺ marks
# of the next round read it), the store and its snapshot go, the dir stays
# while anything is in it -- ^A is what removes it. Public as --done so the
# by-hand re-delivery keep_hint() prints can finish the same way.
done_store() {                # <store>
    local st=$1 d=${1%/*}
    [ -f "$st" ] && [ ! -L "$st" ] && [ -s "$st" ] && cat -- "$st" >> "$d/delivered.md"
    rm -f -- "$st" "$st.rows"
    rmdir -- "$d" 2>/dev/null
    return 0
}
```

`mark_rows` becomes three-file; replace its signature line and the awk's first two rules:

```bash
mark_rows() {                 # <store-or-/dev/null> <delivered-or-/dev/null> <rows-file>
    LC_ALL=C awk -F'\t' -v OFS='\t' '
        FILENAME == ARGV[1] { if (substr($0, 1, 3) == "## ") c[$0]++; next }
        FILENAME == ARGV[2] { if (substr($0, 1, 3) == "## ") d[$0]++; next }
        NF == 0 { next }
```

and the marker line:

```bash
            # One column wide whatever the count: ● for one, the digit past
            # that, + for anything more, ↺ for a row commented in an EARLIER
            # round (delivered, nothing pinned on it now) -- "you said this
            # last time, it is still here". A wrong digit lies, "+" does not.
            key = "## " $1 ":" num " (" sign ")"
            n = c[key] + 0
            m = (n == 0) ? ((key in d) ? "↺" : " ") : (n == 1) ? "●" : (n > 9) ? "+" : n ""
```

(delete the old `n = c["## " $1 ...]` line.) The two callers in `rows_mode()` pass the delivered file:

```bash
    local hdr=/dev/null dlv=/dev/null
    [ -n "$store" ] && [ -f "$store" ] && hdr=$store
    [ -n "$store" ] && [ -f "${store%/*}/delivered.md" ] && dlv=${store%/*}/delivered.md
    if [ -n "$store" ] && [ ! -L "$store.rows" ] && [ -s "$store.rows" ]; then
        mark_rows "$hdr" "$dlv" "$store.rows"
    else
        mark_rows "$hdr" "$dlv" <(lines_mode "$dir")
    fi
```

`run_loop()`: `--expect=ctrl-s,ctrl-b,ctrl-a`; header line 1 becomes `↵ pin the typed comment   ^S deliver   ^A approve   esc discard (asks first)`; line 3 becomes `"base: $label$ndlv"` where, computed each iteration just before the fzf call:

```bash
        local ndlv=""
        [ -f "${store%/*}/delivered.md" ] && ndlv="   ↺ $(grep -c '^## ' "${store%/*}/delivered.md") delivered earlier"
```

and after the `ctrl-b` branch:

```bash
        if [ "$key" = ctrl-a ]; then
            if [ -s "$store" ]; then
                tmux display-message "tmux-review: deliver or discard the $(grep -c '^## ' "$store") pinned comment(s) first" 2>/dev/null
                rows_out=$(rows_mode "$dir" "$store"); continue
            fi
            if approve "$dir"; then exit 21; fi
            tmux display-message "tmux-review: could not write the checkpoint ref" 2>/dev/null
            rows_out=$(rows_mode "$dir" "$store"); continue
        fi
```

`review_pane()`: the `if [ "$rc" -eq 20 ]` block's cleanup string becomes

```bash
        cleanup="rc=\$?; if [ \$rc -eq 0 ]; then $(shq "$SELF") --done $(shq "$store"); else rm -f -- $(shq "$store.rows"); fi"
```

and gains a branch before the `else`:

```bash
    elif [ "$rc" -eq 21 ]; then
        # Approved: the ref is written (run_loop did it, in the popup); the
        # pane's attention state and the review dir end here.
        tmux set-option -pu -t "$pane" @cockpit_review 2>/dev/null
        "$(dirname "$SELF")/agent-state" recount "$pane"
        [ -d "$sess" ] && [ ! -L "$sess" ] && rm -rf -- "$sess"
        tmux display-message "review approved — the next one shows only what changes from here" 2>/dev/null
```

`keep_hint()`'s printf gains the archive: `'; comments kept at %s — deliver later with: %s --deliver %s %s && %s --done %s'` with `"$(shq "$SELF")" "$(shq "$store")"` appended to its argument list. `usage()`: `  tmux-review --done <store>            after a by-hand --deliver: archive the batch, drop the snapshot`; `^A approve` on the key line. Dispatcher: `--done) shift; done_store "${1:-}" ;;`.

In the phase-2 block `a delivered review's comment store does not outlive the popup that made it`, the assertion that the session **dir** is gone after delivery becomes: the store is gone, `delivered.md` holds the comment, the dir is present (grep the block for its `is "` lines about the dir and rewrite the expectation; the test's name stays true — the *store* does not outlive the popup).

- [ ] **Step 4: Run**

Run: `make test-only PAT='\^A approves|does not outlive|tmux-review --rows|ctrl-s delivers|survives the client'`
Expected: every line PASS.

- [ ] **Step 5: Commit**

```bash
git add scripts/tmux-review tests/run.sh
git commit -m "feat: ^A approves — the checkpoint ref is the tree the human saw, the dir goes, and a delivered batch stays as ↺ on the next round"
```

---

### Task 4: `agent-state recount`, `◆N` in the fleet string, `off` clears the review options

**Files:**
- Modify: `scripts/agent-state` — the state validation `case`, the two sticky-guard allow lists, the write block, the `off` arm, `fmt_nr`, the batched read, the fleet string.
- Test: `tests/run.sh` — new block `== agent-state recount: ◆N counts panes with a pending review, off clears them ==` after the block `== agent-state: a hook transition costs at most four tmux round-trips ==`.

**Interfaces:**
- Produces: `agent-state recount <pane>` — writes no state; re-derives the window/fleet counters from live options (compare-then-set). `@cockpit_fleet` may now end with `#[fg=#{@thm_yellow}]◆N#[default] `. `off` unsets `@cockpit_review` and `@cockpit_review_spool` on the pane.

- [ ] **Step 1: Write the failing test**

```bash
echo "== agent-state recount: ◆N counts panes with a pending review, off clears them =="
if need_tmux "agent-state recount"; then :; else
tmux set-option -p -t "$PANE" @cockpit_review pending >/dev/null 2>&1
st recount "$PANE"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'◆1'*) ok "recount adds ◆1 to the fleet string" ;;
    *) bad "recount adds ◆1 to the fleet string" "…◆1…" "$(tmux show-option -gqv @cockpit_fleet)" ;;
esac
tmux set-option -p -t "$PANE" @cockpit_review sent >/dev/null 2>&1
st recount "$PANE"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'◆'*) bad "a sent review is not a pending one" "no ◆" "$(tmux show-option -gqv @cockpit_fleet)" ;;
    *) ok "a sent review is not a pending one" ;;
esac
tmux set-option -p -t "$PANE" @cockpit_review pending >/dev/null 2>&1
tmux set-option -p -t "$PANE" @cockpit_review_spool /nonexistent >/dev/null 2>&1
tmux set-option -p -t "$PANE" @panestate needs-input >/dev/null 2>&1
st recount "$PANE"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'▲'*'◆1'*) ok "recount runs under a sticky needs-input too, ▲ before ◆" ;;
    *) bad "recount runs under a sticky needs-input too, ▲ before ◆" "▲N … ◆1" "$(tmux show-option -gqv @cockpit_fleet)" ;;
esac
st off "$PANE"
is "off clears the review state" "" "$(tmux show-option -pqv -t "$PANE" @cockpit_review)"
is "off clears a queued spool" "" "$(tmux show-option -pqv -t "$PANE" @cockpit_review_spool)"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'◆'*) bad "and the ◆ is gone" "no ◆" "$(tmux show-option -gqv @cockpit_fleet)" ;;
    *) ok "and the ◆ is gone" ;;
esac
fi
```

- [ ] **Step 2: Run it to see it fail**

Run: `make test-only PAT='agent-state recount'`
Expected: `recount adds ◆1` FAIL (unknown keyword exits 0 before writing).

- [ ] **Step 3: Implement**

In `scripts/agent-state`:

1. The validation case: `running|needs-input|error|done|off|recount) ;;` and a sentence in its comment: `recount writes no state -- it re-derives the counters below from what the options already say (review-gate and tmux-review call it after touching @cockpit_review).`
2. Both sticky guards: `needs-input) case "$state" in running|off|recount) ;; *) exit 0 ;; esac ;;` and `error) case "$state" in running|off|needs-input|recount) ;; *) exit 0 ;; esac ;;`.
3. The write block: 

```bash
if [ "$state" = off ]; then
    [ -n "$current" ] && tq set-option -pu -t "$pane" @panestate
    # A session that ends takes its review attention with it: a pending mark
    # or a queued spool on a pane whose agent is gone would count forever.
    tq set-option -pu -t "$pane" @cockpit_review
    tq set-option -pu -t "$pane" @cockpit_review_spool
elif [ "$state" = recount ]; then
    :                                       # nothing to write; derive below
else
```

4. After `fmt_ne=…` add:

```bash
# ◆N: agent panes whose review-gate flagged a finished, unreviewed diff
# (@cockpit_review = pending). Same walk, same guard, same batch -- one more
# #{n:} in the read below, one more glyph in the string. Ranked last: an agent
# waiting on you (▲) or dead (✕) outranks one waiting to be read.
fmt_nr='#{n:#{S:#{W:#{P:#{?#{==:#{@cockpit_review},pending},#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},x,},}}}}}'
```

5. The batched `display-message` format gains `"$sep$fmt_nr"` right after `"$sep$fmt_ne"`, and the split gains `nr=${pre%%"$sep"*};            pre=${pre#*"$sep"}` after the `ne=` line (so `fleet_cur=$pre` still reads the last field).
6. After the `✕$ne` case: `case "$nr" in ''|0|*[!0-9]*) ;; *) fleet="$fleet#[fg=#{@thm_yellow}]◆$nr#[default] " ;; esac`.
7. Update the header comment's usage line: `#   usage: agent-state <running|needs-input|error|done|off|notify|recount> [pane-id]`.

- [ ] **Step 4: Run**

Run: `make test-only PAT='agent-state'`
Expected: every line PASS, including `a hook transition costs at most four tmux round-trips` (recount is two processes).

- [ ] **Step 5: Commit**

```bash
git add scripts/agent-state tests/run.sh
git commit -m "feat: agent-state recount re-derives the counters, ◆N joins the fleet string, off clears a pane's review state"
```

---

### Task 5: `scripts/review-gate` — the second Stop hook: drain, flag, title

**Files:**
- Create: `scripts/review-gate`
- Modify: `examples/claude-hooks.json` (a second `Stop` matcher group), `README.md` hooks table (one row), `examples/tmux.conf` and `examples/full.tmux.conf` (the tab title reads `@cockpit_title` first).
- Test: `tests/run.sh` — new block `== review-gate: hands a queued review over once, flags ◆ on a finished unreviewed diff, never blocks ==` after Task 4's block; the block `== install.sh --hooks merges additively ==` gains one assertion.

**Interfaces:**
- Consumes: `tmux-review --delta <pane>` (Task 2), `agent-state recount|running` (Task 4), `@cockpit_review_spool` (armed by Task 6; this task tests it with a hand-set option).
- Produces: `scripts/review-gate` (no arguments; `$TMUX_PANE`; JSON payload on stdin). Exit 2 + the spool's text on stderr when a spool is armed; exit 0 otherwise. Sets/unsets `@cockpit_review pending`; sets `@cockpit_title`, `@cockpit_session` when they change.

- [ ] **Step 1: Write the failing test**

```bash
echo "== review-gate: hands a queued review over once, flags ◆ on a finished unreviewed diff, never blocks =="
if need_tmux "review-gate"; then :; else
RG="cockpit-rg-$$"; SOCKS="$SOCKS $RG"; RGSTATE=$(mktmpd); RGDIR=$(mktmpd)
(
  cd "$RGDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init
) >/dev/null 2>&1
"$TMUX_REAL" -L "$RG" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RGDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RG" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
RGAP=$("$TMUX_REAL" -L "$RG" list-panes -t t: -F '#{pane_id}')
RGCTL=$("$TMUX_REAL" -L "$RG" split-window -t t -P -F '#{pane_id}' 'exec bash')
until_cap "$RG" "$RGCTL" '*$*'
rg_n() { "$TMUX_REAL" -L "$RG" capture-pane -p -J -t "$RGCTL" | grep -c 'GRC=[0-9]'; }
rg_wait() {                   # <count-before>  until one more finished GRC= marker is on screen (rundeliver's shape)
    local i; for i in $(seq 1 60); do [ "$(rg_n)" -gt "$1" ] && return 0; sleep 0.1; done; return 1
}
rg_run() {                    # <stdin-file>  runs the hook inside the session as the pane's own hook would, RC on the control pane
    local n0; n0=$(rg_n)
    "$TMUX_REAL" -L "$RG" send-keys -t "$RGCTL" \
        "TMUX_PANE=$RGAP XDG_STATE_HOME=$(printf '%q' "$RGSTATE") '$DIR/scripts/review-gate' < $(printf '%q' "$1") 2> $(printf '%q' "$RGSTATE/err"); echo GRC=\$?" Enter
    rg_wait "$n0"
}
rg_rc() { "$TMUX_REAL" -L "$RG" capture-pane -p -J -t "$RGCTL" | grep -o 'GRC=[0-9]*' | tail -1; }
RGPAY=$(mktmpf); printf '{"session_id":"sid-123","transcript_path":"%s/tx.jsonl","cwd":"%s","hook_event_name":"Stop","stop_hook_active":false}\n' "$RGSTATE" "$RGDIR" > "$RGPAY"
printf '{"type":"ai-title","aiTitle":"First topic"}\n{"type":"user"}\n{"type":"ai-title","aiTitle":"Fix the parser"}\n{"type":"assistant"}\n' > "$RGSTATE/tx.jsonl"
# Clean tree: nothing to flag; the payload's fields are stashed.
rg_run "$RGPAY"; is "a clean tree exits 0" "GRC=0" "$(rg_rc)"
is "no review is pending on a clean tree" "" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_review)"
is "the newest ai-title becomes the pane's title" "Fix the parser" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_title)"
is "the session id is stashed for prefix i" "sid-123" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_session)"
# Dirty tree: ◆.
printf 'a\nb\n' > "$RGDIR/f"
rg_run "$RGPAY"; is "a finished, unreviewed diff exits 0" "GRC=0" "$(rg_rc)"
is "and flags the pane pending" "pending" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_review)"
case "$("$TMUX_REAL" -L "$RG" show-option -gqv @cockpit_fleet)" in *'◆1'*) ok "the fleet string says ◆1" ;; *) bad "the fleet string says ◆1" "◆1" "$("$TMUX_REAL" -L "$RG" show-option -gqv @cockpit_fleet)" ;; esac
# A queued review: handed over on stderr, exit 2, exactly once.
printf 'Code review from the human\n\n## f:2 (+)\n> b\nDRAIN-MARKER\n\n' > "$RGSTATE/spool.md"
"$TMUX_REAL" -L "$RG" set-option -p -t "$RGAP" @cockpit_review_spool "$RGSTATE/spool.md" >/dev/null 2>&1
rg_run "$RGPAY"; is "a queued review makes the hook exit 2" "GRC=2" "$(rg_rc)"
case "$(cat "$RGSTATE/err")" in *DRAIN-MARKER*) ok "and the review is on stderr, where a Stop hook's exit 2 hands it to Claude" ;; *) bad "and the review is on stderr" "DRAIN-MARKER" "$(cat "$RGSTATE/err")" ;; esac
is "the spool option is cleared before speaking" "" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_review_spool)"
is "the spool file is consumed" "absent" "$([ -e "$RGSTATE/spool.md" ] && echo present || echo absent)"
is "the pane reads running again (the agent is about to work)" "running" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @panestate)"
rg_run "$RGPAY"; is "the same Stop fired again delivers nothing (no rewake loop)" "GRC=0" "$(rg_rc)"
is "and stderr is empty the second time" "" "$(cat "$RGSTATE/err")"
# Never blocks: a writer that holds stdin open without sending is the hang.
rg_t0=$SECONDS; rg_n0=$(rg_n)
"$TMUX_REAL" -L "$RG" send-keys -t "$RGCTL" "sleep 20 | TMUX_PANE=$RGAP '$DIR/scripts/review-gate'; echo GRC=\$?" Enter
rg_wait "$rg_n0"
[ $(( SECONDS - rg_t0 )) -le 4 ] && ok "a silent stdin does not hang the hook ($(( SECONDS - rg_t0 ))s)" || bad "a silent stdin does not hang the hook" "under 4s" "$(( SECONDS - rg_t0 ))s"
unset -f rg_n rg_wait rg_run rg_rc
"$TMUX_REAL" -L "$RG" kill-server >/dev/null 2>&1
fi
```

In `== install.sh --hooks merges additively ==`, directly after `is "a second run changes nothing (idempotent)" …`, add — the fixture there starts with one foreign `Stop` group, so after the merge `Stop` holds foreign + `agent-state done` + `review-gate`, three groups, and the merge dedupes per group:

```bash
is "the shipped Stop hooks are two groups beside the foreign one, agent-state done once" "3 1 1" \
   "$(jq -r '[(.hooks.Stop | length),
             ([.hooks.Stop[].hooks[].command | select(test("agent-state done"))] | length),
             ([.hooks.Stop[].hooks[] | select(.command | test("review-gate")) | .timeout] | length)] | join(" ")' \
       "$HK/.claude/settings.json")"
```

- [ ] **Step 2: Run it to see it fail**

Run: `make test-only PAT='review-gate|merges additively'`
Expected: `a clean tree exits 0` FAIL (no such script → `GRC=127`), the hooks assertion FAIL (1 group).

- [ ] **Step 3: Write `scripts/review-gate`**

```bash
#!/bin/bash
# review-gate -- the second Stop hook (examples/claude-hooks.json), beside
# `agent-state done`. Two jobs at the one moment the plugin knows an agent has
# reached a turn boundary:
#
#   1. hand over a review that was queued while the agent was busy. tmux-review
#      --deliver arms @cockpit_review_spool with the composed review's path when
#      the pane is running or on a prompt; here the file's text goes to stderr
#      and the hook exits 2 -- for a Stop hook that "prevents Claude from
#      stopping" and "shows stderr to Claude" (hooks reference, exit-code
#      table). Zero keystrokes, no paste, no Enter, no focus change, and the
#      turn boundary is by construction: this hook IS the boundary.
#   2. flag the pane ◆ (@cockpit_review pending) when its diff against the
#      review base is non-empty, so the fleet says "finished, unreviewed"
#      instead of only "finished" -- and stash the conversation title and the
#      session id the payload carries, for the tab and for prefix i.
#
# agent-state's discipline, verbatim: pane id shape guard, bounded stdin read
# with one deadline, one JSON field pulled by parameter expansion (no jq),
# silent no-op on anything unparseable, fail-open everywhere. A hook must never
# block the agent that fired it; the hooks entry carries "timeout": 5 as the
# outer bound and this script is built to finish in well under one second.
set -u
pane=${TMUX_PANE:-}
case "$pane" in %[0-9]*) ;; *) exit 0 ;; esac
here=$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)

# --- 1. the drain -------------------------------------------------------------
spool=$(tmux show-option -pqv -t "$pane" @cockpit_review_spool 2>/dev/null)
if [ -n "$spool" ] && [ -f "$spool" ] && [ ! -L "$spool" ]; then
    # Disarm BEFORE speaking. A Stop hook that re-emits its own reason on the
    # Stop it caused is a rewake loop (codex-plugin-cc#248); Claude Code also
    # caps consecutive blocks, but the cap is not the design. A crash past this
    # line loses one review; it never repeats one.
    tmux set-option -pu -t "$pane" @cockpit_review_spool 2>/dev/null
    # The sibling hook wrote `done` for this same Stop; exit 2 means the agent
    # works again now, so say so -- the border would otherwise read green while
    # the pane grinds through the review (the next PreToolUse corrects it too;
    # this only closes the gap).
    "$here/agent-state" running "$pane"
    cat -- "$spool" >&2
    rm -f -- "$spool"
    rmdir -- "${spool%/*}" 2>/dev/null
    exit 2
fi

# --- 2. the flag, and the two payload fields worth keeping --------------------
# Bounded exactly as agent-state's notify arm: one deadline for the whole read,
# read -t so a writer that holds the pipe open without sending cannot wedge
# the hook, the partial line kept.
payload="" chunk="" deadline=$(( SECONDS + 2 ))
while left=$(( deadline - SECONDS )); [ "$left" -gt 0 ]; do
    IFS= read -r -t "$left" chunk || { payload="$payload$chunk"; break; }
    payload="$payload$chunk"; chunk=""
done
# "name":"value" -> value, control chars stripped, length capped. Not a JSON
# parser: an escaped quote inside the value truncates it, which is fine for
# a path and a UUID and merely shortens a title.
field() {
    local v
    case "$payload" in
        *"\"$1\""*) v=${payload#*\"$1\"}; v=${v#*:}; v=${v#*\"}; v=${v%%\"*} ;;
        *) v="" ;;
    esac
    v=${v//[$'\001'-$'\037']/}
    printf '%s' "${v:0:200}"
}
tx=$(field transcript_path)
sid=$(field session_id)
title=""
if [ -n "$tx" ] && [ -f "$tx" ]; then
    # The newest ai-title record wins; read from the end (9 ms on a 64 MB
    # project dir, measured). tac is GNU, tail -r is BSD; neither means no title.
    rec=$( { tac -- "$tx" 2>/dev/null || tail -r -- "$tx" 2>/dev/null; } | grep -m1 '"ai-title"')
    case "$rec" in
        *'"aiTitle"'*) title=${rec#*\"aiTitle\"}; title=${title#*:}; title=${title#*\"}; title=${title%%\"*} ;;
    esac
    title=${title//[$'\001'-$'\037']/}
    title=${title:0:120}
fi

# One read for everything this decides against; one write per value that
# actually changed -- every set-option is a redraw.
sep=$'\037'
cur=$(tmux display-message -p -t "$pane" \
    '#{@cockpit_review}'"$sep"'#{@cockpit_title}'"$sep"'#{@cockpit_session}' 2>/dev/null)
review=${cur%%"$sep"*};  cur=${cur#*"$sep"}
title_cur=${cur%%"$sep"*}; sid_cur=${cur#*"$sep"}

if "$here/tmux-review" --delta "$pane"; then want=pending; else want=""; fi
if [ "$review" != "$want" ]; then
    if [ -n "$want" ]; then tmux set-option -p -t "$pane" @cockpit_review pending 2>/dev/null
    else                    tmux set-option -pu -t "$pane" @cockpit_review 2>/dev/null; fi
    "$here/agent-state" recount "$pane"
fi
# Untrusted strings each get a call of their own (agent-state's rule: a bare
# `;` argument splits a tmux command list).
[ -n "$title" ] && [ "$title" != "$title_cur" ] && tmux set-option -p -t "$pane" @cockpit_title "$title" 2>/dev/null
[ -n "$sid" ]   && [ "$sid"   != "$sid_cur"   ] && tmux set-option -p -t "$pane" @cockpit_session "$sid" 2>/dev/null
exit 0
```

`chmod +x scripts/review-gate`.

In `examples/claude-hooks.json`, the `Stop` array becomes two groups (the merge dedupes per group, so an upgraded settings.json gains only the new one):

```json
    "Stop": [
      { "matcher": "", "hooks": [ { "type": "command", "command": "\"${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}\"/scripts/agent-state done" } ] },
      { "matcher": "", "hooks": [ { "type": "command", "command": "\"${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}\"/scripts/review-gate", "timeout": 5 } ] }
    ],
```

and a line in `_comment`: `"Stop runs a second command, review-gate: it flags a finished pane whose diff is unreviewed (◆) and hands a queued review to the agent (exit 2). Its own timeout is 5 s; it never blocks a turn otherwise."`

README hooks table, after the `Stop` row: ``| `Stop` (2nd) | `review-gate` — flags `◆` when the finished diff is unreviewed; hands a queued review to the agent at this turn boundary (exit 2 + stderr), once |``.

Tab title: in `examples/tmux.conf` lines 42–43 and every matching site in `examples/full.tmux.conf` (`grep -n 's/^✳ //:#{pane_title}' examples/*.conf`), replace `#{s/^✳ //:#{pane_title}}` with `#{?#{@cockpit_title},#{@cockpit_title},#{s/^✳ //:#{pane_title}}}` — the transcript's own topic when review-gate has seen one, the OSC title otherwise. A per-pane option read on the window's active pane; no walk.

- [ ] **Step 4: Run**

Run: `make test-only PAT='review-gate|merges additively|agent-state'` then `make lint`
Expected: every line PASS; `shellcheck: clean` (`scripts/review-gate` is in `SOURCES` by the glob).

- [ ] **Step 5: The rig**

Run: `bash tests/freeze-rig/run.sh gate` twice.
Expected: `PASS`/`FAIL` line unchanged from master (712 visits on 121 panes; the same FAIL verdict master has for the pre-existing marker walk). A higher number is a regression from the tab-title edit — revert it and report.

- [ ] **Step 6: Commit**

```bash
git add scripts/review-gate examples/claude-hooks.json README.md examples/tmux.conf examples/full.tmux.conf tests/run.sh
git commit -m "feat: review-gate on Stop — hands a queued review to the agent once (exit 2), flags ◆ on a finished unreviewed diff, stashes the transcript title; never blocks (2 s read deadline, 5 s hook timeout)"
```

---

### Task 6: `deliver()` — a busy pane gets the review queued, not refused

**Files:**
- Modify: `scripts/tmux-review` — `deliver()` (the spool, the `if-shell`), its comment block (guard 3 rewritten), `usage()`.
- Test: `tests/run.sh` — rewrite the block `== tmux-review --deliver: a pane on a permission prompt is refused, not pasted into ==` as `== tmux-review --deliver: a busy pane gets the review queued for its next turn boundary ==`; the atomic-gate block `== tmux-review --deliver: the paste is gated atomically… ==` keeps its assertions (an idle agent still pastes; a pane that stopped being an agent still never gets it).

**Interfaces:**
- Produces: `deliver()` writes the composed review to `<store>.deliver` (0600) and arms `@cockpit_review_spool=<that path>`; the `if-shell -F` idle arm disarms it and pastes as before; the busy arm (`@panestate` is `running` or `needs-input`) keeps it armed, drops the buffer and reports `review queued`. Exit 0 in both arms.

- [ ] **Step 1: Rewrite the test block**

Replace the whole `a pane on a permission prompt is refused, not pasted into` block with:

```bash
echo "== tmux-review --deliver: a busy pane gets the review queued for its next turn boundary =="
"$TMUX_REAL" -L "$RDS" send-keys -t "$AP" C-u >/dev/null 2>&1      # clear the earlier paste
for busy in needs-input running; do
    "$TMUX_REAL" -L "$RDS" set-option -p -t "$AP" @panestate "$busy" >/dev/null 2>&1
    printf '## src.go:1 (+)\n> foo\nQUEUE-MARKER-%s fix\n\n' "$busy" > "$RDSTORE"
    rundeliver "$AP" "$RDSTORE"
    is "delivery into a $busy pane exits 0 (queued, not refused)" "RC=0" "$(lastrc)"
    case "$(apcap)" in
        *"QUEUE-MARKER-$busy"*) bad "nothing is pasted into a $busy pane" "no marker on the agent pane" "$(apcap)" ;;
        *) ok "nothing is pasted into a $busy pane" ;;
    esac
    is "the spool is armed on the pane" "$RDSTORE.deliver" \
       "$("$TMUX_REAL" -L "$RDS" show-option -pqv -t "$AP" @cockpit_review_spool)"
    case "$(cat "$RDSTORE.deliver" 2>/dev/null)" in
        "Code review of your changes"*"QUEUE-MARKER-$busy"*) ok "and the spool holds the composed review" ;;
        *) bad "and the spool holds the composed review" "composed header + marker" "$(cat "$RDSTORE.deliver" 2>/dev/null)" ;;
    esac
    is "the spool is born 0600" "600" "$(stat -c '%a' "$RDSTORE.deliver" 2>/dev/null || stat -f '%Lp' "$RDSTORE.deliver")"
    is "no buffer is left behind" "" "$("$TMUX_REAL" -L "$RDS" list-buffers -F '#{buffer_name}' 2>/dev/null | grep 'cockpit-review')"
    "$TMUX_REAL" -L "$RDS" set-option -pu -t "$AP" @cockpit_review_spool >/dev/null 2>&1
    rm -f "$RDSTORE.deliver"
done
"$TMUX_REAL" -L "$RDS" set-option -p -t "$AP" @panestate done >/dev/null 2>&1
rundeliver "$AP" "$RDSTORE"
case "$(apcap)" in
    *"QUEUE-MARKER-running"*) ok "the same review lands as a paste once the pane is done" ;;
    *) bad "the same review lands as a paste once the pane is done" "marker on the agent pane" "$(apcap)" ;;
esac
is "an idle delivery leaves no spool armed" "" "$("$TMUX_REAL" -L "$RDS" show-option -pqv -t "$AP" @cockpit_review_spool)"
"$TMUX_REAL" -L "$RDS" set-option -pu -t "$AP" @panestate >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" set-option -pu -t "$AP" @cockpit_review >/dev/null 2>&1
```

(`Code review of your changes` is Task 7's header; until Task 7 lands the case pattern is `"Code review of your uncommitted changes"*`. Task 7 flips it.)

- [ ] **Step 2: Run it to see it fail**

Run: `make test-only PAT='busy pane gets the review queued'`
Expected: `delivery into a needs-input pane exits 0` FAIL (`RC=1`), spool assertions FAIL.

- [ ] **Step 3: Implement**

In `deliver()`:

1. Delete guard 3 (the `pstate`/`needs-input` refusal) and rewrite its comment paragraph in the block above the function: `#   3. (no longer a refusal) a pane that is running or on a permission prompt gets the review QUEUED: the composed text is spooled beside the store and @cockpit_review_spool names it; review-gate hands it over at the agent's next Stop. Nothing is ever pasted into a dialog.`
2. After `payload=$("$SELF" --compose "$store" "$root")`, before `bytes=`:

```bash
    # The spool: the composed review as a file, born 0600 beside the store. On
    # the paste path it is the Stop hook's copy if the pane turns out busy at
    # paste time; on the hook path it is the whole delivery (stderr carries
    # text of any length, so the 2048-byte pointer below is paste-only).
    local spool="$store.deliver"
    if ! (umask 077; [ ! -L "$spool" ] && printf '%s' "$payload" > "$spool"); then
        tmux display-message \
            "tmux-review: could not write the review spool, refusing to deliver$(keep_hint "$pane" "$store")" \
            2>/dev/null
        return 1
    fi
```

3. Directly before the `tmux if-shell -F -t "$pane"` call: `tmux set-option -p -t "$pane" @cockpit_review_spool "$spool" 2>/dev/null` with the comment:

```bash
    # Armed FIRST, disarmed by the idle arm below: the busy/idle choice is made
    # by tmux inside one command-queue item, and arming ahead of it means the
    # spool is never armed on a pane that just took the paste, and never
    # missing on one that did not. A pane that stopped being an agent inside
    # the ~50ms window keeps a harmless armed option that dies with it --
    # review-gate only ever runs from an agent's own hook.
```

4. The `if-shell`:

```bash
    tmux if-shell -F -t "$pane" \
        "#{&&:#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{&&:#{!=:#{@panestate},running},#{!=:#{@panestate},needs-input}}}" \
        "set-option -pu -t $pane @cockpit_review_spool ; set-option -p -t $pane @cockpit_review sent ; paste-buffer -p -r -d -b $bufname -t $pane ; display-message -d 0 'review pasted into $where ($pane) — press Enter there to send; prefix J walks there'" \
        "delete-buffer -b $bufname ; display-message -d 0 'review queued for $where ($pane) — it lands when the agent next stops'"
```

The post-`if-shell` buffer check stays as it is (both arms consume the buffer; a surviving buffer still means "tmux never ran either arm" and returns 1).

5. `usage()`: `  tmux-review --deliver <pane> <store>   paste it typed-but-unsent into <pane>; a busy pane gets it queued for its next Stop`.

- [ ] **Step 4: Run**

Run: `make test-only PAT='tmux-review --deliver|does not outlive|REFUSED delivery|review-gate'`
Expected: every line PASS.

- [ ] **Step 5: Commit**

```bash
git add scripts/tmux-review tests/run.sh
git commit -m "feat: a review delivered to a running or blocked pane is queued beside its store and armed for review-gate; only an idle pane is pasted into"
```

---

### Task 7: The compose header and the close-out line

**Files:**
- Modify: `scripts/tmux-review` — `compose()`; `deliver()`'s pointer sentence.
- Test: `tests/run.sh` — the two goldens in `== tmux-review --compose: header + blocks verbatim, ?? passes through untouched ==`; Task 6's case pattern.

**Interfaces:**
- Produces: the header text below, byte-exact; the pointer sentence `Read and address every comment in <file> — a code review from the human; ?? comments are questions.`

- [ ] **Step 1: Update the goldens (fail-first: the old header fails them)**

Replace the header lines of both `rv3golden` and `rv3rootgolden` (everything before the first `## scripts/tm:43 (+)`) with:

```text
Code review of your changes, from the human, batched on purpose.

Scope: change only what these comments ask for. Do not refactor, rename,
tidy or commit anything else — a fix that grows the diff defeats the review.
Questions first: a comment containing ?? is a question. Answer every question
in chat before you edit anything, then make the edits in one pass.
Disagree out loud: if a comment is wrong or would break something, say so and
do not make the change. Silent compliance with a wrong comment is the worse
failure.
Anchors: comments are path:line with the original line quoted. The numbers are
from the diff as it was when the review was written and drift after your first
edit — trust the quoted fragment, not the number.
When done, end your reply with one line per comment:
  <path>:<line> — done | skipped: <reason>

```

and for `rv3rootgolden`, the line `Paths are relative to /home/agent/project.` goes directly after the `edit — trust the quoted fragment, not the number.` line, before `When done`. Flip Task 6's `"Code review of your uncommitted changes"*` pattern to `"Code review of your changes"*`.

- [ ] **Step 2: Run to see it fail**

Run: `make test-only PAT='tmux-review --compose'` — both goldens FAIL.

- [ ] **Step 3: Implement**

```bash
compose() {
    local store=${1:-} root=${2:-}
    printf 'Code review of your changes, from the human, batched on purpose.\n\n'
    printf 'Scope: change only what these comments ask for. Do not refactor, rename,\ntidy or commit anything else — a fix that grows the diff defeats the review.\n'
    printf 'Questions first: a comment containing ?? is a question. Answer every question\nin chat before you edit anything, then make the edits in one pass.\n'
    printf 'Disagree out loud: if a comment is wrong or would break something, say so and\ndo not make the change. Silent compliance with a wrong comment is the worse\nfailure.\n'
    printf 'Anchors: comments are path:line with the original line quoted. The numbers are\nfrom the diff as it was when the review was written and drift after your first\nedit — trust the quoted fragment, not the number.\n'
    [ -n "$root" ] && printf 'Paths are relative to %s.\n' "$root"
    printf 'When done, end your reply with one line per comment:\n  <path>:<line> — done | skipped: <reason>\n\n'
    [ -n "$store" ] && [ -f "$store" ] && cat -- "$store"
    return 0
}
```

Rewrite the function's comment: `# The header (ai lens §7.F: scope fence, questions first, dissent licence, trust the fragment, a close-out line the next review can key on), then the store verbatim -- cat, nothing clever.` In `deliver()`, the pointer: `payload="Read and address every comment in $file — a code review from the human; ?? comments are questions."`.

- [ ] **Step 4: Run**

Run: `make test-only PAT='tmux-review --compose|tmux-review --deliver|oversized'` — every line PASS (the oversized block asserts on the pointer sentence; update its expected text if it quotes the old one).

- [ ] **Step 5: Commit**

```bash
git add scripts/tmux-review tests/run.sh
git commit -m "feat: the composed header fences scope, puts questions first, licenses dissent, trusts the fragment and asks for a close-out line per comment"
```

---

### Task 8: `^S` asks once (deliver / edit / keep); `esc` leaves and keeps the batch

**Files:**
- Modify: `scripts/tmux-review` — `run_loop()` (the `ctrl-s` branch, the esc picker), `review_pane()` (`REVIEW_PANE_ID` in the loop's env), `usage()`.
- Test: `tests/run.sh` — Task 3's block gets its `deliver 1 comment` wait back; the phase-0 block `== tmux-review --loop: alt-enter pins a comment, ctrl-s delivers, esc discards ==` and the phase-2 blocks that press `Down Enter` on esc's picker (`survives the client`, `moving between projects`, `clean-tree direct pane` if it does) change to the new row order; one new block `== tmux-review --loop: ^S asks first — deliver, edit the batch, or keep reviewing; esc leaves the batch pinned ==`.

**Interfaces:**
- Produces: on `^S` a picker `deliver N comment(s) to <pane>` / `edit the batch in $EDITOR first` / `keep reviewing`; on esc with comments pinned: `leave — N comment(s) stay pinned` / `keep reviewing` / `discard N comment(s)`. `run_loop` reads `$REVIEW_PANE_ID` for the label; `review_pane()` and `--request` export it.

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-review --loop: ^S asks first — deliver, edit the batch, or keep reviewing; esc leaves the batch pinned =="
if need_tmux "review ^S confirm"; then :; else
CS="cockpit-cs-$$"; SOCKS="$SOCKS $CS"; CSSTATE=$(mktmpd); CSDIR=$(mktmpd)
(
  cd "$CSDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\n' > f
) >/dev/null 2>&1
CSED=$(mktmpf); printf '#!/bin/sh\nprintf "EDITED-BY-EDITOR\\n" >> "$1"\n' > "$CSED"; chmod +x "$CSED"
"$TMUX_REAL" -L "$CS" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$CSDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$CS" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
CSAP=$("$TMUX_REAL" -L "$CS" list-panes -t t: -F '#{pane_id}')
CSCTL=$("$TMUX_REAL" -L "$CS" split-window -t t -P -F '#{pane_id}' 'exec bash'); until_cap "$CS" "$CSCTL" '*$*'
CSSESS="$CSSTATE/tmux-cockpit/review/${CSAP#%}-$(printf '%s' "$CSDIR" | cksum | cut -d' ' -f1)"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" "EDITOR=$(printf '%q' "$CSED") XDG_STATE_HOME=$(printf '%q' "$CSSTATE") '$DIR/scripts/tmux-review' -p $CSAP" Enter
until_cap "$CS" "$CSCTL" '*review>*'
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" "one"; until_cap "$CS" "$CSCTL" '*one*'; "$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Enter; until_file "$CSSESS/comments.md" '*one*'
# esc: leave, batch kept, dir kept.
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Escape
until_cap "$CS" "$CSCTL" '*leave — 1 comment*' && ok "esc offers to leave with the batch pinned" || bad "esc offers to leave with the batch pinned" "leave — 1 comment(s) stay pinned" "$("$TMUX_REAL" -L "$CS" capture-pane -p -t "$CSCTL")"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Enter
until_cap "$CS" "$CSCTL" '*$*'
is "leaving keeps the store" "1" "$(grep -c '^## ' "$CSSESS/comments.md" 2>/dev/null)"
# Back in: ^S asks; edit runs $EDITOR on the whole store; then deliver.
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" "EDITOR=$(printf '%q' "$CSED") XDG_STATE_HOME=$(printf '%q' "$CSSTATE") '$DIR/scripts/tmux-review' -p $CSAP" Enter
until_cap "$CS" "$CSCTL" '*resumed 1 pinned*'
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" C-s
until_cap "$CS" "$CSCTL" "*deliver 1 comment(s) to $CSAP*" && ok "^S asks before delivering, naming the pane" || bad "^S asks before delivering, naming the pane" "deliver 1 comment(s) to $CSAP" "$("$TMUX_REAL" -L "$CS" capture-pane -p -t "$CSCTL")"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Down Enter      # edit the batch in $EDITOR first
until_file "$CSSESS/comments.md" '*EDITED-BY-EDITOR*' && ok "edit hands the whole store to \$EDITOR" || bad "edit hands the whole store to \$EDITOR" "EDITED-BY-EDITOR in the store" "$(cat "$CSSESS/comments.md")"
until_cap "$CS" "$CSCTL" '*review>*' && ok "and returns to the list" || bad "and returns to the list" "review>" "$("$TMUX_REAL" -L "$CS" capture-pane -p -t "$CSCTL")"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" C-s; until_cap "$CS" "$CSCTL" '*deliver 1 comment*'; "$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Enter
until_cap "$CS" "$CSAP" '*EDITED-BY-EDITOR*' && ok "deliver pastes the edited batch" || bad "deliver pastes the edited batch" "EDITED-BY-EDITOR in the agent pane" "$("$TMUX_REAL" -L "$CS" capture-pane -p -t "$CSAP")"
"$TMUX_REAL" -L "$CS" kill-server >/dev/null 2>&1
fi
```

Re-insert in Task 3's block the line `until_cap "$RA" "$RACTL" '*deliver 1 comment*'; "$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" Enter` after its `C-s`. In every block that presses `Down Enter` to reach `discard N comment(s)` (grep `discard 1 comment` / `discard [0-9]* comment` in `tests/run.sh`), the discard row is now third: `Down Down Enter`. In the phase-0 block titled `alt-enter pins a comment, ctrl-s delivers, esc discards`, add the confirm step after its `C-s` the same way and read its esc assertions against the new rows.

- [ ] **Step 2: Run to see it fail**

Run: `make test-only PAT='\^S asks first'` — `esc offers to leave` FAIL.

- [ ] **Step 3: Implement**

In `run_loop()` the esc picker's rows and dispatch:

```bash
                    choice=$(printf 'leave — %d comment(s) stay pinned\nkeep reviewing\ndiscard %d comment(s)\n' "$n" "$n" \
                        | fzf --reverse --no-info --prompt='esc? ' \
                              --header="$n comment(s) are pinned and not delivered")
```

```bash
                    case "$choice" in
                        leave*)   drop_rows "$store"; exit 0 ;;     # the dir keeps the store; prefix r resumes it
                        discard*) drop_store "$store"; exit 0 ;;
                        *)        rows_out=$(rows_mode "$dir" "$store"); continue ;;
                    esac
```

The `ctrl-s` branch:

```bash
        if [ "$key" = ctrl-s ]; then
            # Once, in the same picker shape esc uses: the consent gesture
            # shows what it is about to send and to whom, and offers the whole
            # batch to $EDITOR first (a comment edited as text -- the grammar
            # is forgiving, compose is cat).
            n=$(grep -c '^## ' "$store")
            [ "$n" -gt 0 ] || { tmux display-message "tmux-review: nothing pinned" 2>/dev/null; continue; }
            choice=$(printf 'deliver %d comment(s) to %s\nedit the batch in $EDITOR first\nkeep reviewing\n' "$n" "${REVIEW_PANE_ID:-the agent}" \
                | fzf --reverse --no-info --prompt='^S? ' --header="deliver $n comment(s)?")
            case $? in 0) ;; *) rows_out=$(rows_mode "$dir" "$store"); continue ;; esac
            case "$choice" in
                deliver*) exit 20 ;;
                edit*)    ${EDITOR:-vi} "$store" </dev/tty >/dev/tty 2>&1
                          rows_out=$(rows_mode "$dir" "$store"); continue ;;
                *)        rows_out=$(rows_mode "$dir" "$store"); continue ;;
            esac
        fi
```

`review_pane()`: the loop call becomes `REVIEW_PANE_ID=$pane "$SELF" --loop "$dir" "$store"`. Header line 1: `↵ pin the typed comment   ^S deliver (asks first)   ^A approve   esc leave (asks first)`. `usage()`'s `--loop` line: `ctrl-s asks, then exits 20 (deliver); esc leaves the batch pinned (asks first when any are)`.

- [ ] **Step 4: Run**

Run: `make test-only PAT='tmux-review --loop|\^A approves|survives the client|moving between projects|clean-tree'` — every line PASS.

- [ ] **Step 5: Commit**

```bash
git add scripts/tmux-review tests/run.sh
git commit -m "feat: ^S asks once — deliver, edit the batch, or keep reviewing; esc leaves the batch pinned and prefix r resumes it"
```

---

### Task 9: `--request` and the `/cockpit-review` skill

**Files:**
- Create: `skills/cockpit-review/SKILL.md`
- Modify: `scripts/tmux-review` — add `request()`; `usage()`; dispatcher. `install.sh` — links `~/.claude/skills/cockpit-review` (and removes it on `--uninstall`). `README.md` — a short "the agent asks" paragraph.
- Test: `tests/run.sh` — new block `== tmux-review --request: the agent asks — exit 10 with the review on stdout, 0 on nothing, 1 with no client ==`; the block `== install.sh only ever touches its own links ==` gains the skill link.

**Interfaces:**
- Produces: `tmux-review --request <pane>` — opens the review popup on that pane (one `display-popup -E`), waits; when the store is non-empty afterwards prints `compose` to stdout, archives with `--done`, exits **10**; exits 0 when nothing was pinned; exits 1 (stderr says so) when the session has no attached client. `^A` inside a requested review is handled the same as `review_pane` (state cleared, dir removed, exit 0). The skill runs it via `${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}/scripts/tmux-review --request "$TMUX_PANE"`.

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-review --request: the agent asks — exit 10 with the review on stdout, 0 on nothing, 1 with no client =="
if need_tmux "tmux-review --request"; then :; else
RQ="cockpit-rq-$$"; SOCKS="$SOCKS $RQ"; RQSTATE=$(mktmpd); RQDIR=$(mktmpd)
(
  cd "$RQDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\n' > f
) >/dev/null 2>&1
"$TMUX_REAL" -L "$RQ" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RQDIR" 'exec -a claude bash' >/dev/null 2>&1
RQAP=$("$TMUX_REAL" -L "$RQ" list-panes -t t: -F '#{pane_id}')
# A detached server has no client to draw a popup on: a launcher error, not a
# "discarded" -- the skill tells them apart by the exit code.
XDG_STATE_HOME=$RQSTATE TMUX= "$TMUX_REAL" -L "$RQ" run-shell "XDG_STATE_HOME=$(printf '%q' "$RQSTATE") '$DIR/scripts/tmux-review' --request $RQAP; echo RQRC=\$?" >"$RQSTATE/out" 2>&1
case "$(cat "$RQSTATE/out")" in *RQRC=1*) ok "no attached client: exit 1" ;; *) bad "no attached client: exit 1" "RQRC=1" "$(cat "$RQSTATE/out")" ;; esac
# The popup itself needs a client and a human; stand a tmux shim in front of
# it (the suite's own shim technique) that answers `display-popup` by doing
# nothing -- the human's whole review is then whatever the store already
# holds when the popup "closes".
RQSHIM=$(mktmpd)
cat > "$RQSHIM/tmux" <<SHIMEOF
#!/bin/sh
case "\$1" in
    display-popup) exit 0 ;;
    display-message) for a; do :; done; case "\$a" in *session_attached*) echo 1; exit 0 ;; esac ;;
esac
exec "$TMUX_REAL" -L "$RQ" "\$@"
SHIMEOF
chmod +x "$RQSHIM/tmux"
RQSESS="$RQSTATE/tmux-cockpit/review/${RQAP#%}-$(printf '%s' "$RQDIR" | cksum | cut -d' ' -f1)"
mkdir -p "$RQSESS"; printf '## f:2 (+)\n> b\nREQUEST-MARKER\n\n' > "$RQSESS/comments.md"
rq_out=$(PATH="$RQSHIM:$PATH" XDG_STATE_HOME=$RQSTATE "$DIR/scripts/tmux-review" --request "$RQAP"); rq_rc=$?
is "comments pinned: exit 10" "10" "$rq_rc"
case "$rq_out" in "Code review of your changes"*"REQUEST-MARKER"*) ok "and the composed review is on stdout" ;; *) bad "and the composed review is on stdout" "header + REQUEST-MARKER" "$rq_out" ;; esac
is "the batch is archived, the store consumed" "1" "$(grep -c REQUEST-MARKER "$RQSESS/delivered.md" 2>/dev/null)"
rq_out=$(PATH="$RQSHIM:$PATH" XDG_STATE_HOME=$RQSTATE "$DIR/scripts/tmux-review" --request "$RQAP"); rq_rc=$?
is "nothing pinned: exit 0, nothing printed" "0 " "$rq_rc $rq_out"
"$TMUX_REAL" -L "$RQ" kill-server >/dev/null 2>&1
fi
```

In `== install.sh only ever touches its own links ==`: the `HOME="$IB"` prefix and the two skill-link assertions described in Step 3 (write them now; they fail until Step 3 lands).

- [ ] **Step 2: Run to see it fail**

Run: `make test-only PAT='tmux-review --request|only ever touches'` — `no attached client` FAIL (`unknown option`), the link assertions FAIL.

- [ ] **Step 3: Implement**

`scripts/tmux-review`, after `review_pane()`:

```bash
# --- --request: the agent asks ----------------------------------------------
# The blocking path (ai lens §7.C, revdiff's contract): run from the agent's
# own Bash tool by the cockpit-review skill, this opens ONE popup on the pane,
# waits for the human, and hands the batch back as stdout + exit 10 -- the
# tool result -- instead of a paste. Nothing else changes: the same session
# dir, the same loop, ^A and esc mean what they always mean. A dropped
# launcher (the tool timed out) leaves the popup and the store alone; running
# it again resumes the pinned comments, so the recovery is "run it again".
request() {                   # <pane>
    local pane=$1 dir root sess store pw ph
    case "$pane" in %[0-9]*) ;; *) usage; exit 2 ;; esac
    if [ "$(tmux display-message -p -t "$pane" '#{session_attached}' 2>/dev/null)" = 0 ]; then
        printf 'tmux-review: no client is attached to %s'"'"'s session — nowhere to open the review\n' "$pane" >&2
        exit 1
    fi
    dir=$(tmux display-message -p -t "$pane" '#{pane_current_path}' 2>/dev/null)
    [ -n "$dir" ] || { printf 'tmux-review: pane %s is gone\n' "$pane" >&2; exit 1; }
    root=$(git_root "$dir"); sess=$(session_dir "$pane" "$root"); store="$sess/comments.md"
    (umask 077; mkdir -p -- "$sess" && [ ! -L "$store" ] && : >> "$store") || {
        printf 'tmux-review: could not create %s\n' "$sess" >&2; exit 1; }
    pw=$(tmux show-option -gqv @cockpit_popup_width);  pw=${pw:-70%}
    ph=$(tmux show-option -gqv @cockpit_popup_height); ph=${ph:-70%}
    # -E: this returns when the loop exits; the exit status is not trusted
    # (tmux's own, not the loop's) -- the store decides.
    tmux display-popup -E -t "$pane" -w "$pw" -h "$ph" -T ' review · requested ' \
        "REVIEW_PANE_ID=$(shq "$pane") $(shq "$SELF") --loop $(shq "$root") $(shq "$store")" 2>/dev/null
    if [ -s "$store" ]; then
        compose "$store" "$root"
        done_store "$store"
        exit 10
    fi
    # A ^A inside the popup wrote the ref and exited 21 -- an exit code the
    # popup does not return, so approve is detected the way it is defined:
    # the ref exists and the delta is empty. run_loop has no pane in hand,
    # so the pane's state is cleared here as review_pane does it.
    if git -C "$root" --no-optional-locks rev-parse -q --verify \
           "refs/cockpit/reviewed/$(git -C "$root" rev-parse --abbrev-ref HEAD 2>/dev/null)" >/dev/null 2>&1 \
       && ! "$SELF" --delta "$pane"; then
        tmux set-option -pu -t "$pane" @cockpit_review 2>/dev/null
        "$(dirname "$SELF")/agent-state" recount "$pane"
        [ -d "$sess" ] && [ ! -L "$sess" ] && rm -rf -- "$sess"
    fi
    exit 0
}
```

`usage()`: `  tmux-review --request <pane>          the agent asks: one popup on <pane>; the batch on stdout, exit 10`. Dispatcher: `--request) shift; request "${1:-}" ;;`.

`skills/cockpit-review/SKILL.md`:

```markdown
---
name: cockpit-review
description: Ask the human for a code review of this pane's changes through the tmux-cockpit review popup, then act on the comments. Use when you finished a piece of work and want it looked at before going on, or when the user says "review this" / "get a review".
allowed-tools: Bash(*/scripts/tmux-review --request *)
---

Run this, with a long timeout (the human is reading and annotating; do not
poll, do not retry, do not run anything else meanwhile):

    "${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}/scripts/tmux-review" --request "$TMUX_PANE"

Read the exit code:

- **10** — the review is on stdout. Follow its header: answer every `??`
  question in chat first, then make the edits in one pass, disagree out loud
  where a comment is wrong, and end with one close-out line per comment
  (`<path>:<line> — done | skipped: <reason>`).
- **0** — the human pinned nothing (left, discarded, or approved). Say so in
  one line and continue with what you were doing.
- **1** — the popup could not open (no attached client, the pane is gone).
  Report stderr verbatim. Do not retry.

If the command timed out, the popup is still open and nothing is lost: wait
for the human to tell you the review is done, then run the same command
again — it resumes the pinned comments, and their `^S` hands them to you.
```

`install.sh`: the body of `for c in "${COMMANDS[@]}"; do … done` (from `if [ "$uninstall" -eq 1 ]; then` to the `fi` that closes the `ln -sfn` branch) becomes a function, defined above the loop, with `link` and `target` as its two parameters and `rc` still the script-global:

```bash
link_one() {                  # <link> <target>  create, or on --uninstall remove, one link that is ours
    local link=$1 target=$2
    …the loop body, verbatim…
}
```

the loop becomes `for c in "${COMMANDS[@]}"; do link_one "$BIN/$c" "$DIR/scripts/$c"; done`, and after it:

```bash
# The cockpit-review skill, for Claude Code -- only where Claude Code is
# installed (~/.claude exists), so a plain shell box gets no stray dir.
if [ -d "${HOME:-}/.claude" ]; then
    [ "$uninstall" -eq 0 ] && mkdir -p "$HOME/.claude/skills" 2>/dev/null
    link_one "$HOME/.claude/skills/cockpit-review" "$DIR/skills/cockpit-review"
fi
```

`--uninstall` runs the same function's remove branch. The printed lines keep their format. In the test block `== install.sh only ever touches its own links ==`, every `install.sh` invocation gets `HOME="$IB"` in front (it already sets `XDG_BIN_HOME`; without a `HOME` override the skill link would land in the real `~/.claude/skills`), `mkdir -p "$IB/.claude"` runs once at the top, and two assertions are added after the existing install/uninstall pair: `is "the skill is linked into ~/.claude/skills" "$DIR/skills/cockpit-review" "$(readlink "$IB/.claude/skills/cockpit-review")"` after the install, `is "and unlinked on --uninstall" "absent" "$([ -e "$IB/.claude/skills/cockpit-review" ] && echo present || echo absent)"` after the uninstall (the `3` / `0` link counts on `$IBIN` are unchanged — the skill link is not in `$IBIN`).

README, at the end of the "Review the agent's changes" section, before the tests section:

```markdown
**The agent can ask.** `install.sh` links `skills/cockpit-review` into
`~/.claude/skills`, so `/cockpit-review` (or "get this reviewed") runs
`tmux-review --request` from the agent's own pane: one popup, the same
loop, and the batch comes back as the tool's result (exit 10) instead of a
paste — revdiff's contract. `^A` there approves as anywhere else.
```

- [ ] **Step 4: Run**

Run: `make test-only PAT='tmux-review --request|only ever touches'` and `make lint` — every line PASS; shellcheck clean.

- [ ] **Step 5: Commit**

```bash
git add scripts/tmux-review skills/cockpit-review/SKILL.md install.sh README.md tests/run.sh
git commit -m "feat: --request lets the agent ask — one popup, the batch on stdout with exit 10; the cockpit-review skill is linked by install.sh"
```

---

### Task 10: `prefix r` is always the queue (preselected, looping back), `@cockpit_review_window`, review glyphs in the queue and in `prefix J`

**Files:**
- Modify: `scripts/tmux-review` — `main_entry()`, `fleet_queue()` (preselect, loop), `fleet_rows()` (the review field → glyph); `scripts/tmux-jump` (pending after sent); `cockpit.tmux` (the binding site); `usage()`.
- Test: `tests/run.sh` — the block `== tmux-review: a clean-tree direct pane says so instead of opening an empty picker ==` becomes `== tmux-review: prefix r from an agent pane opens the queue with that pane preselected, and returns to it after a review ==`; `== prefix r reviews, prefix R reloads; off unbinds each ==` gains the window variant; `== prefix J jumps to the pane that needs you ==` gains a pending pane; the fleet block asserts the glyph.

**Interfaces:**
- Produces: `fleet_queue [preselect-pane]`; rows' state field reads `◆ done` / `↵ done` when `@cockpit_review` is `pending` / `sent`; `tmux-jump` cycle order is blocked → sent → pending; `@cockpit_review_window on` binds `prefix r` to `new-window -n review`.

- [ ] **Step 1: Write the failing tests**

Replace the clean-tree block with:

```bash
echo "== tmux-review: prefix r from an agent pane opens the queue with that pane preselected, and returns to it after a review =="
if need_tmux "prefix r is the queue"; then :; else
PQ="cockpit-pq-$$"; SOCKS="$SOCKS $PQ"; PQSTATE=$(mktmpd); PQA=$(mktmpd); PQB=$(mktmpd)
for d in "$PQA" "$PQB"; do (
  cd "$d" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\n' > f
) >/dev/null 2>&1; done
"$TMUX_REAL" -L "$PQ" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$PQA" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PQ" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
"$TMUX_REAL" -L "$PQ" new-window -t t: -c "$PQB" 'exec -a claude bash' >/dev/null 2>&1
PQAP=$("$TMUX_REAL" -L "$PQ" list-panes -t t:0 -F '#{pane_id}'); PQBP=$("$TMUX_REAL" -L "$PQ" list-panes -t t:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$PQ" set-option -p -t "$PQAP" @panestate done >/dev/null 2>&1
"$TMUX_REAL" -L "$PQ" set-option -p -t "$PQBP" @panestate needs-input >/dev/null 2>&1   # ranks FIRST in the queue
"$TMUX_REAL" -L "$PQ" set-option -p -t "$PQAP" @cockpit_review pending >/dev/null 2>&1
PQCTL=$("$TMUX_REAL" -L "$PQ" split-window -t t:0 -P -F '#{pane_id}' 'exec bash'); until_cap "$PQ" "$PQCTL" '*$*'
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" "XDG_STATE_HOME=$(printf '%q' "$PQSTATE") '$DIR/scripts/tmux-review' -p $PQAP" Enter
until_cap "$PQ" "$PQCTL" '*fleet>*' && ok "prefix r from an agent pane opens the queue" || bad "prefix r from an agent pane opens the queue" "fleet>" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")"
# The pointer sits on the row of the pane the key was pressed in, not on the
# needs-input pane that sorts first.
case "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")" in
    *"> ◆ done"*) ok "with that pane preselected (its row shows ◆ for the pending review)" ;;
    *) bad "with that pane preselected (its row shows ◆ for the pending review)" "> ◆ done …" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")" ;;
esac
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" Enter
until_cap "$PQ" "$PQCTL" '*review>*' && ok "↵ reviews it" || bad "↵ reviews it" "review>" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")"
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" Escape
until_cap "$PQ" "$PQCTL" '*fleet>*' && ok "esc from the review returns to the queue" || bad "esc from the review returns to the queue" "fleet>" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")"
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" Escape
until_cap "$PQ" "$PQCTL" '*$*'
"$TMUX_REAL" -L "$PQ" kill-server >/dev/null 2>&1
fi
```

In the `prefix r reviews` block, after the `off` assertions, add a third private server with `set -g @cockpit_review_window on` before `run-shell cockpit.tmux` and assert `list-keys -T prefix | awk '$4=="r"'` matches `*new-window*tmux-review*`. In the `prefix J` block, before the "Nothing blocked" step: set `@cockpit_review pending` on `JP2` (state running) and nothing blocked, press from `JP1`, assert the window is `1` (`prefix J walks to a pane with a pending review when nothing is blocked`). In the fleet block: `"$TMUX_REAL" -L "$FQ" set-option -p -t "$DIRTY_PANE" @cockpit_review sent` before the `--fleet` call and assert the dirty pane's row contains `↵ `.

- [ ] **Step 2: Run to see them fail**

Run: `make test-only PAT='prefix r|prefix J|queue lists exactly'` — the queue-from-agent-pane line FAIL (`review>` comes up instead), the glyph, window and jump lines FAIL.

- [ ] **Step 3: Implement**

`fleet_rows()`: the format gains `#{?#{@cockpit_review},#{@cockpit_review},-}` as a field after the state sentinel; the read becomes `read -r isagent id state review title path`; store `pane_review[n]=$review` (with `[ "$review" = - ] && review=`); in phase 3 the state column is built as

```bash
        case "${pane_review[i]}" in pending) glyph='◆ ' ;; sent) glyph='↵ ' ;; *) glyph='' ;; esac
        rows+=("$(printf '%s\t%s\t%s\t%s\t%s\t%s\t%s' \
            "$rank" "$churn" "$id" "$glyph${state:-—}" "$title" "${repo:-?}⑂${branch:-?}" "$stat")")
```

(the header comment's row description: `state` becomes `[◆|↵ ]state`.)

`fleet_queue()` takes `pre=${1:-}` and wraps its body from `rows=$(fleet_rows)` to the `review_pane` call in `while :; do … done`; the fzf call gains the preselect:

```bash
    local pos=0 i
    for i in "${!disp[@]}"; do [ "${disp[$i]%%$'\t'*}" = "$pre" ] && { pos=$(( i + 1 )); break; }; done
    ...
        | fzf --delimiter=$'\t' --with-nth=2,3,4,5 \
              ${pos:+--sync --bind "start:pos($pos)"} \
```

(`${pos:+…}` with `pos=0` still expands — use `[ "$pos" -gt 0 ] && presel=(--sync --bind "start:pos($pos)")` and `${presel[@]+"${presel[@]}"}`, the same idiom as `extra`.) After `review_pane "$pane" "$(git_root "$dir")"` the loop sets `pre=$pane` and continues; `[ -n "$out" ] || return 0` is the exit. The header's first line becomes `which agent needs you   [◆ review pending · ↵ pasted, unsent]  state · title · repo⑂branch · ±`.

`main_entry()`: the `if [ "$is_agent" = 1 ] … else fleet_queue fi` becomes:

```bash
    # Always the queue (§8 #3): the other blocked agents stay visible, and the
    # pane the key was pressed in is preselected so one ↵ reviews it.
    if [ "$is_agent" = 1 ]; then fleet_queue "$pane"; else fleet_queue; fi
```

`scripts/tmux-jump`: a third array `review_pending=()`, filled by `elif [ "$review" = pending ]`, appended after `pending` (the sent ones) with the same `${arr[@]+"${arr[@]}"}` idiom; the header comment gains one line.

`cockpit.tmux`, the review binding:

```bash
# @cockpit_review_window on: a real window instead of the modal popup, so the
# fleet's borders and tabs stay visible while you read (§8 #2: popup default,
# window opt-in, decided again after this phase). Same script, same -p.
if [ "$(tmux show-option -gqv @cockpit_review_window)" = on ]; then
    R_OPEN=(new-window -n review)
else
    R_OPEN=(display-popup -E -w "$PW" -h "$PH" -T ' review · #{b:pane_current_path} ')
fi
RK=$(tmux show-option -gqv @cockpit_review_key)
case "$RK" in
    off) ;;
    "")  tmux bind-key r "${R_OPEN[@]}" "$Q_REVIEW -p '#{pane_id}'" ;;
    *)   tmux bind-key "$RK" "${R_OPEN[@]}" "$Q_REVIEW -p '#{pane_id}'" ;;
esac
```

README: the `prefix + r` row of the key table and the "From an **agent pane** it reviews that pane" paragraph change to "always the queue, the current pane preselected"; a line for `@cockpit_review_window on`.

- [ ] **Step 4: Run**

Run: `make test-only PAT='prefix r|prefix J|tmux-review --fleet|tmux-review: '` — every line PASS.

- [ ] **Step 5: Commit**

```bash
git add scripts/tmux-review scripts/tmux-jump cockpit.tmux README.md tests/run.sh
git commit -m "feat: prefix r is always the queue with the current pane preselected and returns to it after a review; ◆/↵ in the rows and in prefix J; @cockpit_review_window opens a window instead"
```

---

### Task 11: `prefix i` — the info popup, and the docs

**Files:**
- Modify: `scripts/tmux-review` — `info_mode()`; dispatcher; `usage()`. `cockpit.tmux` — the `i` binding with `@cockpit_info_key`. `README.md` — the loop (◆, queued delivery, `^A`, `^B`, `↺`), the options block, the key table, `prefix i`, the state dir's lifetime and purge.
- Test: `tests/run.sh` — new block `== tmux-review --info / prefix i: one screen says what the pane is, waits on, and holds ==`.

**Interfaces:**
- Produces: `tmux-review -p <pane> --info` prints the screen below; `prefix i` (`@cockpit_info_key`, `off` restores stock) opens it in the popup and waits for `↵`.

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-review --info / prefix i: one screen says what the pane is, waits on, and holds =="
if need_tmux "tmux-review --info"; then :; else
IN="cockpit-in-$$"; SOCKS="$SOCKS $IN"; INSTATE=$(mktmpd); INDIR=$(mktmpd)
( cd "$INDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\n' > f ) >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$INDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
INAP=$("$TMUX_REAL" -L "$IN" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @panestate_msg 'run make?' >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @cockpit_review pending >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @cockpit_session sid-9 >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @cockpit_title 'Fix the parser' >/dev/null 2>&1
INSESS="$INSTATE/tmux-cockpit/review/${INAP#%}-$(printf '%s' "$INDIR" | cksum | cut -d' ' -f1)"
mkdir -p "$INSESS"; printf '## f:2 (+)\n> b\nx\n\n' > "$INSESS/comments.md"; printf '## f:1 (+)\n> a\ny\n\n## f:1 (+)\n> a\nz\n\n' > "$INSESS/delivered.md"
inout=$("$TMUX_REAL" -L "$IN" run-shell "XDG_STATE_HOME=$(printf '%q' "$INSTATE") '$DIR/scripts/tmux-review' -p $INAP --info" 2>&1)
for want in "pane      $INAP" "state     needs-input  · run make?" "review    pending" "base      uncommitted" "session   sid-9" "title     Fix the parser" "1 pinned · 2 delivered"; do
    case "$inout" in *"$want"*) ok "info shows: $want" ;; *) bad "info shows: $want" "$want" "$inout" ;; esac
done
"$TMUX_REAL" -L "$IN" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$IN" list-keys -T prefix 2>/dev/null | awk '$4=="i"')" in
    *tmux-review*--info*) ok "prefix i is bound to the info popup" ;;
    *) bad "prefix i is bound to the info popup" "a display-popup running tmux-review --info" "$("$TMUX_REAL" -L "$IN" list-keys -T prefix | awk '$4=="i"')" ;;
esac
"$TMUX_REAL" -L "$IN" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 2: Run to see it fail**

Run: `make test-only PAT='tmux-review --info'` — every line FAIL.

- [ ] **Step 3: Implement**

```bash
# --- --info: one screen for prefix i -------------------------------------------
# What the plugin knows about one pane, in the order you ask it: who, what it
# waits on, the review state, the base, the session, the review dir. Read in
# ONE display-message, split on a US byte (agent-state's idiom -- tab would
# collapse the empty fields that mean "unset").
info_mode() {                 # <pane>
    local pane=$1 sep=$'\037' f id where cmd path state msg review spool sid title root sess b n1 n2
    f=$(tmux display-message -p -t "$pane" \
        '#{pane_id}'"$sep"'#{session_name}:#{window_index} #{window_name}'"$sep"'#{pane_current_command}'"$sep"'#{pane_current_path}'"$sep"'#{@panestate}'"$sep"'#{@panestate_msg}'"$sep"'#{@cockpit_review}'"$sep"'#{@cockpit_review_spool}'"$sep"'#{@cockpit_session}'"$sep"'#{@cockpit_title}' 2>/dev/null) \
        || { printf 'tmux-review: no such pane %s\n' "$pane" >&2; return 1; }
    id=${f%%"$sep"*};     f=${f#*"$sep"}
    where=${f%%"$sep"*};  f=${f#*"$sep"}
    cmd=${f%%"$sep"*};    f=${f#*"$sep"}
    path=${f%%"$sep"*};   f=${f#*"$sep"}
    state=${f%%"$sep"*};  f=${f#*"$sep"}
    msg=${f%%"$sep"*};    f=${f#*"$sep"}
    review=${f%%"$sep"*}; f=${f#*"$sep"}
    spool=${f%%"$sep"*};  f=${f#*"$sep"}
    sid=${f%%"$sep"*};    title=${f#*"$sep"}
    printf 'pane      %s  %s  (%s)\n' "$id" "$where" "$cmd"
    printf 'cwd       %s\n' "$path"
    printf 'state     %s%s\n' "${state:-—}" "${msg:+  · $msg}"
    printf 'review    %s%s\n' "${review:-—}" "${spool:+  · queued: $spool}"
    root=$(git_root "$path"); sess=$(session_dir "$id" "$root")
    if git -C "$root" --no-optional-locks rev-parse -q --verify HEAD >/dev/null 2>&1; then
        COCKPIT_REVIEW_BASE=$(cat -- "$sess/comments.md.base" 2>/dev/null); export COCKPIT_REVIEW_BASE
        b=$(review_base "$root")
        printf 'base      %s  (%s)\n' "$(cut -f2 <<<"$b")" "$(cut -f1 <<<"$b")"
    fi
    printf 'session   %s\n' "${sid:-—}"
    printf 'title     %s\n' "${title:-—}"
    if [ -d "$sess" ]; then
        n1=$(grep -c '^## ' -- "$sess/comments.md" 2>/dev/null); n2=$(grep -c '^## ' -- "$sess/delivered.md" 2>/dev/null)
        printf 'review dir %s\n           %s pinned · %s delivered · ^A removes it\n' "$sess" "${n1:-0}" "${n2:-0}"
    fi
    printf '\n↵ to close\n'
}
```

Dispatcher: `--info) info_mode "${REVIEW_PANE_ID:-}" ;;` (it takes `-p`). `usage()`: `  tmux-review -p <pane> --info          one screen: state, review, base, session, review dir (prefix i)`.

`cockpit.tmux`, after the review binding:

```bash
# prefix i: what this pane is, waits on, and holds -- state, review, base,
# session id, review dir. Replaces stock i (window info) because it IS the
# window info you want on an agent pane; @cockpit_info_key rebinds, off keeps
# stock. `read` is POSIX, so the popup's sh can wait for ↵ itself.
IK=$(tmux show-option -gqv @cockpit_info_key)
case "$IK" in
    off) ;;
    "")  tmux bind-key i display-popup -E -w "$PW" -h "$PH" -T ' info · #{pane_id} ' "$Q_REVIEW -p '#{pane_id}' --info; read -r _" ;;
    *)   tmux bind-key "$IK" display-popup -E -w "$PW" -h "$PH" -T ' info · #{pane_id} ' "$Q_REVIEW -p '#{pane_id}' --info; read -r _" ;;
esac
```

(and the `# Keys it binds` header comment at the top of `cockpit.tmux` lists `prefix + i`.)

README — one pass over "Review the agent's changes":
- the loop paragraph: `◆N` in the fleet counter and what sets it (`review-gate` on Stop, cleared when the agent works again or on `^A`); delivery to a busy pane is queued and lands at its next Stop with zero keystrokes; `^A` approves (the ref, the delta next time, `↺` on rows commented earlier); `^B` cycles the base and the header names it.
- the key table: `^A`, `^B`; `^S` "asks first"; `esc` "leave — the batch stays pinned (asks first)"; `prefix + i`.
- the options block (tcsm-ux §11 #4 — one aligned copy-pasteable block):

```sh
set -g @cockpit_review_key     r       # off restores stock refresh-client
set -g @cockpit_review_window  off     # on: a window instead of the popup
set -g @cockpit_info_key       i       # off restores stock window info
set -g @cockpit_jump_key       J
set -g @cockpit_popup_width    70%
set -g @cockpit_popup_height   70%
set -g @cockpit_fleet_fzf_options ""
```

- the state dir: lives under `~/.local/state/tmux-cockpit/review/` until `^A` or a discard; `rm -rf` of it is the purge; `prefix i` names the dir.
- the "Stale state" section: `@cockpit_review`, `@cockpit_title`, `@cockpit_session` die with the pane; `SessionEnd` clears the review ones.

- [ ] **Step 4: Run**

Run: `make test-only PAT='tmux-review --info|prefix r'` — every line PASS.

- [ ] **Step 5: Commit**

```bash
git add scripts/tmux-review cockpit.tmux README.md tests/run.sh
git commit -m "feat: prefix i shows what a pane is, waits on and holds; the README tells the loop — ◆, queued delivery, ^A, ^B, ↺ — and ships one options block"
```

---

### Task 12: Gate and merge

**Files:** none new.

- [ ] **Step 1: Whole-branch review**

Dispatch a fresh `oh-my-claudecode:code-reviewer` (or `superpowers:requesting-code-review`) with `git diff master...feat/phase3-loop`, this plan, and the Global Constraints; name the report file. Findings go to a fresh worker; nothing here is self-approved. Two things the reviewer must check by running, not reading: `review-gate` under `timeout 5` with a held-open stdin returns; a `--deliver` into a `running` pane followed by `review-gate` hands the text over exactly once.

- [ ] **Step 2: The gate, twice, unpiped**

Run: `cd ~/.cache/cockpit-wt/phase3-loop && make check` — read the last line yourself.
Expected: `shellcheck: clean` and `  N passed, 0 failed, K skipped` with N ≥ 614 + the new assertions (~90). Run it again; same result. Then once from outside the repo: `(cd / && make -C ~/.cache/cockpit-wt/phase3-loop check)`.

- [ ] **Step 3: Leak sweep**

Run: `ls /tmp/tmux-1000 | grep -c cockpit-; pgrep -fa 'hunk|lens'; ss -ltnp | grep 47657; ls ~/.local/state/tmux-cockpit/review 2>/dev/null; git -C ~/.tmux/plugins/tmux-cockpit for-each-ref refs/cockpit/`
Expected: the socket count is what it was before the run; the rest prints nothing (the suite's `XDG_STATE_HOME` redirects keep the real state dir untouched; no test ever wrote a ref into the real repo).

- [ ] **Step 4: Freeze rig**

Run: `bash tests/freeze-rig/run.sh gate` twice.
Expected: 712 visits on 121 panes, worst move unchanged — same verdict line as master. Record under a dated heading in `tests/freeze-rig/RESULTS.txt` ("phase 3: `@cockpit_title` in the tab, `◆N` in the fleet string — no new walk").

- [ ] **Step 5: Live check on the owner's server (read-only)**

With the hooks **not** yet merged into the live settings.json: `install.sh --hooks --dry-run` from the worktree shows exactly one new `Stop` group. Do not apply it from the worktree — the live `TMUX_COCKPIT_DIR` points at the main checkout, and the merge lands with the merge below.

- [ ] **Step 6: Merge**

```bash
cd ~/.tmux/plugins/tmux-cockpit && git merge --no-ff feat/phase3-loop -m "Merge branch 'feat/phase3-loop' — phase 3: review-gate on Stop, the temp-index base, ^A/^B/↺, the queue as prefix r, queued delivery, /cockpit-review, prefix i"
make check
```

Expected: the merged tree's gate green, unpiped. Then `install.sh --hooks` on the live settings (the owner's session picks it up on its next start — hooks are snapshotted per session), update the memory file `review-redesign-research.md`'s status line and the §8 memo with the merge hash, and remove the worktree (`git worktree remove ~/.cache/cockpit-wt/phase3-loop`).

---

## Self-review

**Spec coverage** (master plan §1 phase 3 row): `scripts/review-gate` on `Stop` → `@cockpit_review pending` ✔ Task 5 · precomputed `◆N` ✔ Task 4 (in `@cockpit_fleet`, audit) · Stop-hook drain, paste when idle ✔ Tasks 5–6 · rewritten compose header + close-out line ✔ Task 7 · base resolution + `^B` ✔ Task 2 · `^A` approve → checkpoint ✔ Task 3 · re-review delta `↺` ✔ Tasks 2–3 · `prefix r` always the queue, loops back ✔ Task 10 · `@cockpit_review_window` ✔ Task 10 · `/cockpit-review` skill + `--request` (exit 10) + SKILL.md ✔ Task 9 · `transcript_path`/`session_id` in pane options, `ai-title` labels ✔ Task 5 (`@cockpit_title`, `@cockpit_session`; audit: at Stop, not per prompt) · confirm-before-deliver ✔ Task 8 · `prefix i` ✔ Task 11 · plan-mode review ✘ deferred to phase 4 (audit; §8 #6 allows it) · gate: hook never blocks ✔ Task 5's silent-stdin assertion + `"timeout": 5`; rewake-loop test (fires twice, delivers once) ✔ Task 5; pane-visits gate unchanged ✔ Tasks 5 and 12; every new option precomputed ✔ (`@cockpit_review`, `@cockpit_title`, `@cockpit_session`, `@cockpit_review_spool` are written at transitions/keypresses and only read by formats per pane; the one counter rides the existing batch). Carried inputs from phase 2: a hand re-delivery now ends with `--done` ✔ Task 3; GC = `^A` + the README purge line ✔ Task 11 (audit); the pane+root key stays ✔ (audit).

**Placeholder scan:** no "TBD", no "similar to Task N", no "add error handling"; every step carries its code or the exact command; the two cross-task flips (Task 3's `deliver 1 comment` wait, Task 6's header pattern) are stated in both places.

**Type consistency:** `review_base <dir> [mode]` → `rev\tlabel\tmode` is consumed by `tree_diff` (field 1), `run_loop`'s `set_base_line` (fields 2–3), `next_base` (field 3), `info_mode` (fields 1–2). `tree_diff <dir> [args]` keeps its four call sites (`lines_mode` ×2, `fleet_scan_root`, `delta_mode`). `delta_mode <pane>` = `--delta <pane>` (review-gate, `request`, Task 3's test). `mark_rows <store> <delivered> <rows>` matches both `rows_mode` calls. `done_store <store>` = `--done` (review_pane's cleanup, `keep_hint`, `request`). Exit codes: `--loop` 20 deliver / 21 approve (review_pane handles both; `request` handles 21 by the ref + empty delta); `--request` 10/0/1; `review-gate` 2/0. Options: `@cockpit_review` (`pending`|`sent`), `@cockpit_review_spool` (path), `@cockpit_title`, `@cockpit_session` — the same names in agent-state, review-gate, tmux-review, tmux-jump, the examples and the tests. `agent-state recount <pane>` is what review_pane, request and review-gate call.
