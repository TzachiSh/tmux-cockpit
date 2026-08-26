# Phase 3 (the loop) — whole-branch review, c94302f..ea18ef9

Reviewed in passes, as asked: scripts first (`scripts/review-gate` in full, `scripts/tmux-review` end to end — `tree_diff`, `review_base`, `next_base`, `delta_mode`, `approve`, `done_store`, `run_loop`, `compose`, `deliver`, `fleet_rows`, `fleet_scan_root`, `review_pane`, `request`, `info_mode`, `fleet_queue`, `main_entry`; the `agent-state`, `tmux-jump`, `tmux-open-path`, `cockpit.tmux`, `install.sh`, `examples/` diffs), then `tests/run.sh`'s diff plus `tests/par.sh`, `tests/green.sh`, `tests/changed.sh` and the Makefile, then the README and SKILL.md. Read-only on the worktree; the two run checks ran on a private `-L cockpit-final-148517` server with `XDG_STATE_HOME`/`XDG_RUNTIME_DIR` under the scratchpad, killed and cleaned afterwards. `make lint`: `shellcheck: clean`. I did not run the suite (par.sh writes shard files into `tests/`; the gate is Task 12 step 2, the controller's).

### Strengths

- **The drain cannot loop.** `review-gate:42` disarms `@cockpit_review_spool` before a byte is printed, `rm -f`s the spool after, and the second Stop on the same pane prints nothing (run-verified below). The escaper's order (`\\` → `"` → `\n\r\t` → strip the rest, `review-gate:53-61`) is pinned by a test that would catch either inversion.
- **The read stops at the two fields it wants** (`review-gate:79-87`), not at an EOF Claude Code never sends: 0.109 s on the flag path, 0.025 s on the drain, with a writer holding the pipe open. The 2 s deadline is a backstop the test also fires.
- **One temp-index discipline everywhere a diff runs.** `tree_diff` (`tmux-review:113-143`) copies the index with `cp -p`, `add -N -A`s the copy, `--no-optional-locks` on every call; `approve` uses the same copy for the real `add -A` + `write-tree`; `fleet_rows` lends its one temp dir so the fleet costs no mktemp/rm per root. The index-mtime test (Task 1) is the tooth.
- **`review_base` is one `for-each-ref`** for five candidate refs with the `[gone]`/`ahead` shortcut that skips `merge-base` when the rows already say it is HEAD (`tmux-review:154-200`). 69404fb's "four git forks per root" holds: rev-parse, cp, add -N, for-each-ref, diff (+ merge-base only when needed).
- **The paste gate is airtight in the direction that matters:** the spool option is armed *before* the `if-shell -F`, the busy/idle choice happens inside one tmux command-queue item, and the idle arm is the only thing that disarms it (`tmux-review:1411-1417`). No `send-keys` anywhere in `scripts/` or `cockpit.tmux`; needs-input is never pasted into; `--request` opens exactly one popup and every in-popup picker is fzf, not `display-popup`.
- **Tests:** every added `sleep` is inside a bounded poll (`for i in $(seq 1 N)…`) except the one documented spawn settle; all 8 new servers are `-L cockpit-*-$$` and on `SOCKS`; the install block overrides `HOME` on every call and says why (`tests/run.sh:1605-1614`); the FIFO-holder shape measures the hook's own read, not a pipeline.
- **`par.sh`** snapshots `run.sh` before slicing, `bash -n`s every shard, runs the wall-clock (`serial`) groups alone first, and reddens the run on a shard that dies before it counts. **`green.sh`** keys the stamp on tool versions, not only file bytes.
- **`install.sh`'s `link_one`** keeps the three guards (not-a-symlink, foreign symlink, failed `ln`) for the skill link and skips it when `~/.claude` is absent.
- The README is honest where it costs something to be: the ref outlives the branch and the purge command is printed; a queued review without the hook installed evaporates; `Stop` does not fire on Esc.

### Run-verified checks

Script: `/tmp/claude-1000/-home-tzachi--tmux-plugins-tmux-cockpit/a3bc609c-6457-43ad-809e-32027f4c86ba/scratchpad/runcheck.sh`. Fixture: a one-commit repo with `f` dirtied (`a` → `a\nb`), a session whose pane runs `exec -a claude bash`, `@cockpit_agents_glob` set, `TMUX` pointed at the private socket, `TMUX_PANE=%0`. The FIFO writer sends the one-line Stop payload (`session_id`, `transcript_path` to a two-record transcript, `cwd`, `hook_event_name`, `stop_hook_active`) and then `sleep 8`s with the pipe open.

**Check 1 — held-open stdin, dirty tree, no spool (the flag path), under `timeout 5`:**

```
timeout 5 scripts/review-gate < gate.fifo
rc=0 elapsed=0.109s
stdout=<>
stderr=<>
@cockpit_review=pending  @cockpit_title=Final review title  @cockpit_session=sid-final
@cockpit_fleet=#[fg=#{@thm_yellow}]◆1#[default]
```

**Check 2 — `--deliver` into a `running` pane, then `review-gate` twice:**

```
tmux set-option -p -t %0 @panestate running
scripts/tmux-review --deliver %0 <state>/tmux-cockpit/review/0-2856438149/comments.md <repo>   # rc=0
scripts/tmux-review --done  <same store>                                                       # rc=0
@cockpit_review_spool=<state>/tmux-cockpit/review/0-2856438149/comments.md.deliver
session dir: comments.md.deliver (0600, 1029 B)  delivered.md (0664, 53 B)      # store archived, spool present
capture-pane | grep -c FINAL-CHECK-MARKER -> 0                                   # nothing pasted

--- review-gate run 1
rc=0 elapsed=0.025s
stdout=<{"hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"Code review of your changes, from the human, batched on purpose.\n\nScope: …\n## f:2 (+)\n> b\nFINAL-CHECK-MARKER: is b right here?"}}>
stderr=<>
spool option=<>  spool file=absent  @panestate=running  @cockpit_review=<pending>

--- review-gate run 2
rc=0 elapsed=0.068s
stdout=<>
stderr=<>
spool option=<>  @cockpit_review=<pending>
session dir now: delivered.md only
```

Handed over exactly once, JSON on stdout, exit 0, no stderr; spool file and option gone after the first run. Two things the listing exposed and the issues below pick up: `delivered.md` is born 0664 where every sibling is 0600, and `@cockpit_review` stays `pending` across a queued delivery.

Also run: `./install.sh --hooks --dry-run` from the worktree (writes nothing) shows **one** new `Stop` group (`review-gate`, `"timeout": 5`) — and a second hunk under `SessionStart` adding `agent-state off`, which the examples file has carried since before this branch (`git show c94302f:examples/claude-hooks.json` has it) and the live settings.json lacks. Live-config drift, not a branch defect; the controller should expect two hunks at Task 12 step 5.

### Issues

#### Critical

None found. No hook path blocks or loops (verified), no paste into a dialog, no test touches the real `$HOME`, live server or state dir.

#### Important

**I-1. `^A` destroys a queued review — and `delivered.md` with it.** `scripts/tmux-review:1706-1708` (`review_pane`, rc 21) and `:1760-1763` (`request`, rc 21) `rm -rf -- "$sess"`. A review delivered to a busy pane lives in `$sess/comments.md.deliver` with `@cockpit_review_spool` armed; `^A` is allowed then because the store is empty (the guard at `:1102` only sees pinned comments), so one keystroke removes the spool, leaves the option dangling at a path that no longer exists (`review-gate:37` then silently takes the flag path), and removes `delivered.md` too. The inputs file's "text survives in delivered.md" is wrong — `delivered.md` is `${store%/*}/delivered.md`, the same directory. README:673-674 documents the loss, which contradicts README:585's own rationale for the pinned-comments guard ("approving a batch nobody sent would throw it away"). *Fix (four lines):* in `deliver()` after the `if-shell`, drop the spool the idle arm did not consume — `[ -n "$(tmux show-option -pqv -t "$pane" @cockpit_review_spool)" ] || rm -f -- "$spool"` (this also closes Task 6 deferred (1)); then in both rc-21 arms replace `rm -rf` with `rm -f -- "$sess"/comments.md "$sess"/comments.md.rows "$sess"/comments.md.base "$sess"/delivered.md "$sess"/rc; rmdir -- "$sess" 2>/dev/null` — an armed `.deliver` keeps the dir, `review-gate:65`'s `rmdir` finishes it after the drain. (The alternative, refusing `^A` while the spool is armed, is the same size; keeping the spool is friendlier because approving what you read while the fix is in flight is the normal case.) Then rewrite README:673-674.

**I-2. README:669-671 claims something the code does not do.** "A review queued for an agent you interrupted therefore lands at its next turn end instead, or `prefix + r` pastes it now that the pane is idle." Nothing outside `review-gate` reads `@cockpit_review_spool` or `.deliver` (grep: `tmux-review:1237,1411,1415,1785` are the write, the arm and the info line). `prefix r` opens a fresh review of the same rows; the queued text is handed over only at the *end of the next turn*, i.e. after the agent has already done whatever the next prompt asked. *Fix:* say that; an idle-pane drain is a phase-4 item if wanted.

**I-3. The two stale README paragraphs Task 11's review said must be fixed before merge are still at head.** README:126-133 says `#{agent_fleet}` "walks every pane of every session, twice, on each expansion … 1.7 ms per expansion on a 40-pane server" — since the freeze precompute `cockpit.tmux:61,80` substitutes the `@cockpit_fleet` option and a redraw walks nothing (README:533 says so, three paragraphs of the same file apart). README:817 says "309 cases"; the suite reports 738. Same class: `cockpit.tmux:14` still says `#{agent_fleet}` is "▲N blocked and ✕N errored" (Task 11 deferred (4)). Three edits.

**I-4. `tests/changed.sh` misses the five blocks that carry a `# tests:` marker.** The awk at `tests/changed.sh:43-44` handles `/^# tests: /` *inside* the current block (the report says "inside a block", suite-speed-report.md:659), but all five markers in `tests/run.sh` (`:222, :305, :381, :728, :5074`) sit on the line *before* their block's title — so each is credited to the preceding block, and `delete f` on the title line wipes it. Computed the table with the script's own awk: blocks 4, 7, 13, 23 and 107 ("the paste is gated atomically") have **empty** dependencies, i.e. `make test-changed` never runs them for a `cockpit.tmux`, `examples/`, `tmux-branch` or `tmux-review` change — exactly the blocks the marker exists for. *Fix (awk):* buffer marker files in `pend[]` when they precede a title and merge them into `f` on the `echo "== "` line (or move the five markers below their titles; the awk-side fix is safer because the placement reads naturally). Related, same file: `runsh_blocks()` uses `git diff -U0 -- tests/run.sh` (unstaged only). A `run.sh` change that is staged or already committed since the stamp lists in `green.sh changed` but yields no hunks, so `want` stays `1` and only the syntax block runs — silently. When `tests/run.sh` is in the changed list and no hunk is found, print `ALL`.

**I-5. `tests/green.sh`'s manifest does not cover what the tests read under `tests/fixtures/`.** `green.sh:28-30` hashes `tests/*` (files only; the two directories produce "Is a directory" lines) — `tests/fixtures/review/mkfix.sh` and the eight `.tsv` goldens the `--lines` blocks compare against are not in it. Edit a golden or `mkfix.sh` and `make gate` prints "already green … nothing changed since the last full gate" and exits 0. *Fix:* add `tests/fixtures/review/*` to the `printf` list.

#### Minor

- **M-1.** The busy arm of `deliver()` (`tmux-review:1418`) leaves `@cockpit_review` as it was, so a queued delivery keeps the pane `◆` (run check 2: `pending` before and after), while `request()`'s rc-20 arm (`:1751`) sets `sent`. The next `running` transition clears it either way (`agent-state:215`), so this is a few seconds of "◆ running" in the queue for a pane whose review is already in flight; set `sent` in the busy arm for symmetry, or leave it.
- **M-2.** The two Stop hooks run in parallel, so `review-gate:48`'s `agent-state running` races the sibling's `done`: if `done` lands second the pane reads done (and keeps `◆`, since the early exit at `agent-state:200` skips the clear) while it works the review. The comment at `review-gate:43-47` owns this; the next PreToolUse fixes it. Worth one README sentence, not code.
- **M-3.** `tmux-review:1411-1417`: the spool is armed a few ms before the `if-shell`. A Stop landing in that window is drained by `review-gate` *and* then pasted by the idle arm — the same review twice. Negligible window; noting it because the design comment says the window is closed (it is, for the paste-into-a-non-agent case; not for this one).
- **M-4.** File discipline inside the 0700 dir is uneven: `comments.md`, `.rows`, `.deliver` are born 0600 behind `[ ! -L ]`; `delivered.md` is appended with `>>` at the caller's umask (0664 in run check 2) and no symlink check (`:673`); `.base` is written with `>` and no check (`:1074`); `rc` via `echo … >` in the popup's `sh` (`:1746`). The dir's mode covers them; one `umask 077` in `done_store` and a `[ ! -L ]` on `.base` would make it uniform.
- **M-5.** `agent-state off` (`:195-196`) unsets `@cockpit_review_spool` but leaves the `.deliver` file, and `delivered.md` already says the batch was delivered. A review queued while the agent is running whose session ends before its next Stop (`/exit`, a crash) is dropped with no trace in the UI; `prefix i` no longer shows `queued:`. README:897-901 describes the clear as intentional; say what it costs.
- **M-6.** `.gitignore` has `tests/.only.*` but not `tests/.par.*` or `tests/.cache/` (`green.sh:29`'s fallback stamp dir). During a `make test-par` a concurrent `git status` in the worktree shows ~120 untracked shard files, and `green.sh changed`'s git fallback would list them.
- **M-7.** `tests/changed.sh:69` treats `tests/par.sh` and `tests/green.sh` as "nothing an assertion reads", but block 123 copies and runs both. A `par.sh` edit does not select its own self-test.
- **M-8.** `review-gate` exits at `:66` on a drain without stashing `@cockpit_title`/`@cockpit_session`; the next Stop does it. Fine, just unpinned.
- **M-9.** A detached HEAD checkpoints as `refs/cockpit/reviewed/HEAD` (`review_base:157`, `approve:660`), shared by every detached worktree of the repo. Mention under the ref's known limits or key on the short sha.

### Triage of deferred minors

| item | verdict | one line |
|---|---|---|
| T1 dead `bytes` branch in `lines_mode` | DEFER | plan-mandated shape; drop when the block is next touched |
| T2 `run_loop` lets `.base` override `COCKPIT_REVIEW_BASE` | DEFER | store wins by design; `usage()` never advertised the env knob |
| T2 positional `+1 -0` fleet assertion | DEFER | fine until a second fixture prints it |
| T3 `tmp_index()` helper | DEFER | two callers; the third earns it |
| T5 `--delta` non-zero clears an existing ◆ | DEFER | self-heals at the next Stop |
| T5 `tac \|\| tail -r` forks tail on GNU | DEFER | one fork on a path that runs once per turn |
| T5 empty spool delivered as `""` | DEFER | `deliver()` refuses an empty store before it writes a spool |
| T5 NUL in a spool warns on stderr | DEFER | a human-written review never carries one |
| T5 title truncation / change-only writes unpinned | DEFER | behaviour verified by hand in check 1 |
| T5 `host_short` wrapper hides `@cockpit_title` on a never-titled pane | DEFER | brief-mandated |
| T5 `recount` not muted like `running` | DEFER | it prints nothing (check 1: stdout empty through a recount) |
| T6 (1) paste leaves `comments.md.deliver` behind | **MUST FIX** | one line in `deliver()`, and I-1's dir clean-up depends on it |
| T6 (2) oversized pointer mktemp on the queued path | DEFER | one 0600 file under `$XDG_RUNTIME_DIR`, OS-GC'd |
| T6 (3) second `--deliver` overwrites the spool / `^A` rm -rf's it | **MUST FIX** (the `^A` half) | I-1: `delivered.md` does not survive either; overwrite stays documented |
| T6 (4) no hook installed → queued review evaporates | DEFER | README:657-660 says so |
| T6 (5) ENOSPC truncates an armed spool | DEFER | rare; the store is already archived |
| T6 (6) two unguarded expansions in the converted block | DEFER | test code |
| T8 editor exit status on the `^S` edit discarded | DEFER | the file is re-read either way |
| T8 `n` unguarded when the store vanishes mid-session | DEFER | `grep -c` prints 0 on a missing file |
| T8 `REVIEW_PANE_ID` via env | DEFER | one channel, both callers set it |
| T9 a popup that cannot open exits 0 with nothing on stdout | DEFER | `session_attached` pre-check covers the common cause; SKILL.md's rc-0 text is compatible |
| T9 no-`~/.claude` branch of `install.sh` unpinned | DEFER | `tests/run.sh:1609-1614` exercises it incidentally |
| T10 (1) `main_entry`'s `is_agent` display-message | DEFER | it is not dead — it decides whether to preselect |
| T10 (2) `sleep 0.3` after `run-shell cockpit.tmux` | DEFER | suite idiom, five sibling sites |
| T10 (3) `w_state` cap 12 on `◆ needs-input` | DEFER | `pad_field` overflows, it never truncates |
| T10 (4) `psz` comment says 95 cols | DEFER | comment |
| T10 (5) `tmux-jump` sent-before-pending unpinned | DEFER | three-line change, easy to pin later |
| T10 (6) clean-tree pane with a mark in J, not the queue | DEFER | filter unchanged by design |
| T10 (7) queue's `while` has no non-interactive exit | DEFER | `--filter` in a config option is the owner's foot |
| T10 (8) fzf without `start:pos` closes the queue | DEFER | declare the fzf floor in Requirements when next touched |
| T11 (1) `review dir` label misaligns | DEFER | polish |
| T11 (2) `queued: <path>` line untested | DEFER | check 2 shows the option it reads |
| T11 (3) `@cockpit_info_key` custom/off untested | DEFER | same shape as the review-key test |
| T11 (4) `cockpit.tmux:14` header says two glyphs | **MUST FIX** | one word, same wave as I-3 |
| T11 (5) `--info -p` flag order load-bearing | DEFER | `usage()` shows the working order |
| T11 pre-existing README: `#{agent_fleet}` cost paragraph, "309 cases" | **MUST FIX** | I-3; Task 11's reviewer already required it |

Four MUST FIX lines: T6 (1), T6 (3)'s `^A` half, T11 (4), T11's README pair. Everything else defers.

### Rulings: agree/disagree

- **Ruling 1 (T10 converts every `-p <agent pane>` test to the queue prelude):** AGREE — §8 #3 is the spec; the blocks now wait on `fleet>` then `review>`.
- **T1 churn = add + del, `??` for the suffix only:** AGREE; 69404fb then moved the suffix onto `--summary`'s `create mode` lines, which is the right count under a checkpoint base.
- **T2 fix 1–3 now (`COCKPIT_REVIEW_BASE_REV` in the preview's cache key, `--no-index` untracked preview with a ponytail note):** AGREE — `tmux-open-path`'s key change is exactly the stale-render hazard.
- **T5 handoff via `additionalContext` on stdout, exit 0, exit-2 fallback documented:** AGREE — run-verified: JSON on stdout, empty stderr, exit 0, once.
- **T5 break once both fields are in hand:** AGREE — 25–110 ms measured against a writer that holds the pipe.
- **T3 ref lifecycle text corrected everywhere; `tmp_index` deferred:** AGREE — README:588-594 prints the purge.
- **T3b runs on this branch; 088f25b and 69404fb reviewed here instead of retroactively:** AGREE — and this review is where their findings surface: I-4, I-5, M-6, M-7 are 088f25b's; 69404fb is clean.
- **T6 the refused-delivery block converts to queued semantics:** AGREE — no refusal remains for that path.
- **T6 "defer all; (3) and (4) go into the README as documented behaviour":** **DISAGREE on (3).** The premise ("text survives in delivered.md") is false — `delivered.md` lives in the directory `^A` removes — so documenting the loss (README:673-674) made a bug into a contract. Fix per I-1; (4) stays documented.
- **T9 loop exit code recorded in `<sess>/rc` decides `--request`'s outcome:** AGREE — `leave` keeps a non-empty store, so "non-empty = deliver" would have been wrong.
- **T9 `allowed-tools: Bash(*tmux-review*--request*)`:** AGREE — the docs quoted in allowed-tools-check.md permit a leading `*`; one prompt if not.
- **T8 `until_cap '*review>*'` over a fixed sleep:** AGREE.

### Recommendations

1. One fix wave before merge, in this order: I-1 (four lines in `tmux-review`, then README:673-674), I-2 (two README lines), I-3 (README:126-133, README:817, `cockpit.tmux:14`), I-4 and I-5 (one awk buffer and one `printf` operand in `tests/`). Re-run the gate unpiped after; the `review-gate` and queued-delivery blocks already cover the drain, and a two-line addition to the `^A` block (arm a spool, press `^A`, assert the spool survives) would pin I-1.
2. Update `final-review-inputs.md`'s T6 (3) line so the ledger stops saying `delivered.md` survives.
3. After merge, the live `settings.json` merge will add the `SessionStart` `agent-state off` group as well as the `Stop` group — expected, see the dry-run note above.
4. Phase 4 candidates that fell out of this read: an idle-pane drain for a spool left by an interrupted agent (I-2 is the honest doc until then); `sent` on the busy arm (M-1); the detached-HEAD checkpoint key (M-9).

### Assessment

**Ready to merge?** With fixes. The product code does what the plan and the spec's phase-3 row say, the hook is safe and fast by measurement, and the constraints hold across every path I traced; what blocks the merge is one real loss path behind `^A` (I-1), three README statements that are false today (I-2, I-3, the second already flagged by Task 11's reviewer), and two silent holes in the fast gate a developer will trust (I-4, I-5) — all small, all local, none architectural.
