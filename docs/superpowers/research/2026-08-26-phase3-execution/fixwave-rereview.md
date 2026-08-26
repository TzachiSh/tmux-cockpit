# Fix-wave re-review — phase 3, ea18ef9..de5c83d

Read: `final-review.md` (I-1..I-5 + the four MUST FIX triage rows), `fixwave-report.md`, the
review package diff. Verified against the worktree's files at head, read-only, no git run and
no suite re-run.

### Finding Verdicts

**1. I-1 + T6(1) + T6(3)-^A — `^A` must not destroy a queued review. ADDRESSED.**

- `scripts/tmux-review:684-691` `clear_sess()` removes exactly `comments.md`, `comments.md.rows`,
  `comments.md.base`, `delivered.md`, `rc` and then `rmdir`s. That list is complete: grepping every
  write into the session dir turns up those five plus `comments.md.deliver` (`:1311` `spool="$store.deliver"`),
  and `.deliver` is the one deliberately left. No other name is ever created there.
- Symlink guard: `:686` `[ -d "$s" ] && [ ! -L "$s" ] || return 0` is the *same* pair the old
  `rm -rf` arm carried, moved inside — discipline unchanged, and by-name `rm -f` is strictly safer
  than `rm -rf` since `rm` never follows a symlink it is unlinking.
- Both callers converted: `:1744` (`review_pane`, rc 21) and `:1798` (`request`, rc 21).
- `review-gate:64-65` still does `rm -f -- "$spool"; rmdir -- "${spool%/*}" 2>/dev/null` after the
  drain, so the dir `clear_sess` could not remove is finished off. Confirmed in the file.
- `deliver()`'s spool drop, `scripts/tmux-review:1434`, reads the option after the `if-shell`.
  Synchrony is not an assumption: the function's own comment at `:1380-1386` states it ("if-shell's
  nested command has already run by the time this CLI call returns — same command-queue item, no
  `-b` job behind it"), and it is the standing basis for the pre-existing post-`if-shell`
  `list-buffers` check three lines below. The new test `and no .deliver file behind` (`tests/run.sh:4881-4885`)
  is the empirical proof on the paste path.
- README:682-684 rewritten: the loss sentence is gone, replaced by "`^A` … ends the review directory
  but leaves a still-queued spool alone".
- Test `tests/run.sh:4481-4517`: bounded poll (`for i in $(seq 1 40) … sleep 0.1`) on the block's
  own private `-L "$RA"` socket, no fixed sleep, `review_enter` (which is itself two `until_cap`
  polls) for the picker, real `C-a` keystroke. `QSSESS` is built with the same
  `${pane#%}-$(cksum root)` shape `session_dir()` uses (`scripts/tmux-review:1688-1691`) and the
  same shape `RASESS` already uses at `:4389`. Six assertions, including that the checkpoint ref
  was still written and that the dir goes once the spool is drained.

**2. I-2 — `prefix + r` does not paste a queued review. ADDRESSED.** README:673-677 now says it waits,
that nothing hands it over early, that it lands at the end of the agent's *next* turn, and that
`prefix + r` opens a fresh review of the same rows. Matches the code: `@cockpit_review_spool` is read
only by `review-gate`.

**3. I-3 + T11(4) + the README pair. ADDRESSED.**
- README:128-133 — the walk/1.7 ms claim is gone; now "one precomputed option, `#{E:@cockpit_fleet}`
  … off the redraw path". Agrees with `cockpit.tmux:61,80` and README:533.
- README:820 — `745 cases`, which is this wave's own `make check` line.
- `cockpit.tmux:14-16` — three glyphs (`▲N blocked, ✕N errored and ◆N with a pending review`).
- Two extra copies the worker fixed beyond the brief and flagged: README:63-65 and
  `scripts/agent-state:291`. Both correct, and the wording is consistent with README:112's table row.

**4. I-4 — `tests/changed.sh`. ADDRESSED.**
- `tests/changed.sh:53-55`: the `/^# tests: /` rule now precedes the title rule and buffers into
  `pend[]`; the title rule does `delete f` then merges `pend`; the general rule merges and clears
  `pend` so a marker *inside* a block still credits that block. Checked the two shapes the brief
  named: a marker inside a block followed by a normal line lands on the current block (general rule,
  `:56`); two consecutive markers accumulate into `pend` (no `delete` between them) and both are
  credited.
- All five markers verified in place at `tests/run.sh:222, 305, 381, 728, 5115`, each on the line
  immediately above its `echo "== ` title — the placement the new rule is written for.
- `runsh_blocks()` (`:67-83`): `out=$(…)` then `[ -n "$out" ] || out=ALL`. On the normal unstaged
  case `out` holds the block numbers, so `ALL` does not fire — the report's case B, and the `return`
  inside the `while` is still in a pipeline subshell exactly as before, so the early-`ALL` path is
  unchanged. `printf '%s\n' "$out"` reproduces the old one-per-line output.

**5. I-5 — `tests/green.sh` manifest. ADDRESSED.** `tests/green.sh:28-50`: `tests/fixtures/review/*`
added, and the `printf | grep -v | sort` became a `for` loop keeping `[ -f "$f" ]` only, with the old
`tests/.` exclusion as a `case`. Nine fixture files exist and all are regular (8 `.tsv` + `mkfix.sh`).
The absent-dir case is safe: an unmatched glob stays literal and `[ -f ]` drops it. `set -u` only,
no `set -e`, so the loop body's `[ -f ] && printf` returning 1 is harmless. `changed`'s parser
(`:66`, `/^[<>] [0-9a-f]{64} /`) never matched the old "Is a directory" lines, so removing them
changes nothing there.

**6. M-6 — `.gitignore`. ADDRESSED.** `tests/.par.*` and `tests/.cache/` present, and both match the
real artifacts: `tests/par.sh:25` `SH="$DIR/tests/.par.$$"`, `tests/green.sh:26` fallback
`tests/.cache/green`.

### New Breakage in the Fix Diff

No Critical, no Important. Three Minor / nit:

- **Minor — `scripts/tmux-review:684-691` + `scripts/agent-state:195`: a session dir can now outlive
  everything that would collect it.** `^A` leaves the dir standing whenever `.deliver` is present, and
  the only collector is `review-gate`'s post-drain `rmdir`. A spool armed on a pane that never stops
  again (`/exit`, a crash, `agent-state off` — which unsets the option but does not remove the file)
  leaves the directory and its `.deliver` on disk forever, where the old `rm -rf` swept it. This is
  the right trade (a leaked 0700 directory beats a destroyed review) and the README already prints
  the `rm -rf ~/.local/state/tmux-cockpit/review` purge, but it widens deferred M-5 and is worth one
  sentence there.
- **Nit — `scripts/agent-state:293`:** the rewritten comment left one 106-column line
  (`# #{S:#{W:#{P:}}} walk, and the format named each count TWICE -- …`) un-rewrapped against the
  file's ~80-column prose.
- **Nit — `tests/changed.sh:53-55`:** a `# tests:` marker written on the *last* line of a block now
  credits the *next* block instead of the current one. Unreachable today (all five markers sit above
  titles) and the two placements are indistinguishable to a line-at-a-time parser, so this is the
  correct tie-break, not a defect — noting it only because the comment claims an inside-a-block
  marker "behaves as it always did", which holds everywhere except that one line.

### Out-of-Scope Observations

- README:133/134 — no blank line between the rewritten `#{agent_fleet}` paragraph and
  `### Context fill, optional`. Pre-existing (the hunk removes no blank context line) and CommonMark
  lets an ATX heading interrupt a paragraph, so it renders; the wave touched that exact paragraph and
  could have closed it.
- `tests/freeze-rig/*` is still outside `green.sh`'s manifest — the same class as I-5, but the rig is
  not part of `make check`, so the green stamp does not gate it. Not a hole in the fast gate.
- Recommendation 2 of the review (`final-review-inputs.md`'s T6 (3) line still says `delivered.md`
  survives) is still open; the fix report says so and it needs an owner.
- Every other Minor (M-1 through M-5, M-7 through M-9) is untouched, as instructed.
- The freeze rig's standing `redraw 5.8 / worst move 5.9` on 712 visits / 121 panes is bc5b03b's
  documented number; nothing in this wave touches a redraw path.

### Verdict

**All six findings ADDRESSED. No new Critical or Important breakage. Round passes — the wave clears
the merge blockers this review raised.** The `^A` loss path is closed at the shared helper with both
callers converted and pinned by a real-keystroke test; the two silent gate holes are closed and each
verified by the worker against a pristine copy; the four false README/header statements are now true.
