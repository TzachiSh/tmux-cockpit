# Review popup follow-ups (#22) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development or superpowers:executing-plans, task by task. Steps use `- [ ]`.

**Goal:** close the eight questions in #22 the way the issue asks: research first (`docs/superpowers/research/2026-08-26-review-followups.md`), a decision per item, and only the items that survived built here. Four are code: `alt-c` copies the real lines (1, with 5 folded in), a corrupt store says so (4), every picker's border is visible (6), the gutter survives 10000-line files and ambiguous-width terminals (7), an in-place edit keeps the stored quote (8), the legend fits 80 columns (3). One is closed with its reason recorded (2).

**Architecture:** everything stays in the scripts that own it. `scripts/tmux-review` gains `read_line()` (the tree/HEAD read `attach()` already does) and a `--copy-lines` mode over `{+1} {+2}`; `seed_comment` gives `edit_comment` the stored quote; the legend literal shrinks. `scripts/lib/review-store.sh` adds one `tmux display-message` beside its stderr note. `scripts/tmux-open-path` widens the gutter to `%6` and changes the glyph. `tm`, `tmux-branch`, `tmux-open-path`, the fleet queue get the `border:8,scrollbar:8` token.

**Tech stack:** bash 4+, tmux ≥ 3.3, fzf 0.44.1, git, POSIX awk. No new dependency.

**Branch:** `fix/review-followups`, worktree `~/.cache/cockpit-wt/review-followups`, off master `41f98a6`.

## Global constraints

- Work only in the worktree; pristine copies for fail-first runs come from `git show master:path`, never `git stash`.
- `make check-par` unpiped is the gate; `make test-only PAT='<block ERE>'` for the cycle; `make lint` clean.
- Tests on private `-L` sockets registered in `SOCKS`; temp files via `mktmpd`/`mktmpf`; no new fixed `sleep`.
- `--lines` output stays byte-identical to the goldens; the row's field 5 keeps pointing at the same preview line (the gutter is a prefix).
- Comments: 2-3 lines, one fact the code cannot say. Commits: lowercase `fix:`/`test:`/`docs:` one-liners, past tense, with the session trailer.
- One reviewer pass per task (a fresh subagent, read-only), findings fixed before the next task.

## Ponytail audit — asked for, and what this plan does instead

| asked for (#22) | verdict | why |
|---|---|---|
| 1. `alt-c` reads the line from the file per marked row | **do** | `attach()` has the read; one function, two callers. |
| 2. `^E` picker or separator grammar for several comments on one row | **skip** | one agent reads the paragraphs under one anchor either way; only `●2` changes. README states the one-buffer rule. |
| 3. shorter legend plus a help key (`F1`/`alt-h`, `execute(less)`) | **replace** | reflow the three key lines to ≤ 80 cells; `--help` already holds the full map. No help screen. |
| 4. `tmux display-message` from `detach`/`seed_comment` when `STORE_N` is 0 on a non-empty file | **replace** | one `load_store` wrapper in `tmux-review` that both go through; the lib stays tmux-free (a test pins it). |
| 7 (research): a gutter sized to the file's largest line number | **skip** | needs a pass over the diff before the render; `%6` is one character and the ceiling is 99999. |
| 5. `CAP_SENTINEL` guard on `alt-c` | **do** | inside the new `--copy-lines`, same test `attach` and `^Y` use. |
| 6. the token in each picker, or one shared `FZF_COLOR` in `cockpit.tmux` | **do the first** | five fzf calls; an environment contract for one token is more than it saves. `popup-border-style` styles a different border. |
| 7. `%6` widths and `▸` or `>` | **do** | `%6`, and `▸` (U+25B8 is neutral width; U+25B6 is ambiguous). A highlighted line needs fzf 0.52. |
| 8. carry `STORE[$i.frag]` through `seed_comment`, `attach` takes an explicit fragment | **do** | `attach` already takes one; only the seed side changes. A moved-line `↺` state goes to #14. |
| test debt: the whole list | **partial** | only the three items adjacent to code touched here get a test; the rest stay listed on #22. |

---

### Task 0: Research and plan on the branch

- [ ] Commit `docs/superpowers/research/2026-08-26-review-followups.md` and this plan: `docs: research and plan for the #22 follow-ups`.

### Task 1: A corrupt store says so (4) and every picker's border is visible (6)

**Files:** `scripts/lib/review-store.sh`, `scripts/tm`, `scripts/tmux-branch`, `scripts/tmux-open-path`, `scripts/tmux-review`, `tests/run.sh`.

- [ ] Fail-first, item 4: in the `--detach` block, a store that does not parse under the fake `tmux` shim logs `set aside`; on master the log is empty.
- [ ] Fail-first, item 6: a static block greps each of the five picker fzf calls for `border:8`; master has one.
- [ ] `parse_store`: after the stderr `printf`, `tmux display-message "review-store: ${file##*/} does not parse; set aside as .bak" 2>/dev/null || true`.
- [ ] The four bare `--color=16` become `--color=16,border:8,scrollbar:8` (`tm:132` keeps its `prompt:red,...` list and appends).
- [ ] `make test-only` for both blocks green, `make lint` clean. Commit `fix: a store that does not parse says so on the status line; every picker's preview border is visible`.
- [ ] Review (subagent, read-only): the diff against the two items; fix findings.

### Task 2: `alt-c` copies the real lines (1, 5)

**Files:** `scripts/tmux-review`, `tests/run.sh`, `README.md`.

- [ ] Fail-first: `tmux-review --copy-lines <dir> f.txt 3 g.txt -2` prints the real tab-indented 300-byte line and HEAD's removed line, in argument order; the cap row prints nothing; on master the mode does not exist.
- [ ] `read_line <dir> <path> <line-with-sign>`: the `sed -n Np` / `git show HEAD:path` read from `attach`, control characters kept except NUL, no length cap (the store's 1000-byte cap stays in `attach`). `attach` calls it.
- [ ] `--copy-lines <dir> <path>... <line>...`: half the arguments are paths, half lines (fzf's `{+1}` then `{+2}`); prints each line; refuses `CAP_SENTINEL` rows.
- [ ] The `alt-c` bind runs `$SELF --copy-lines <dir> {+1} {+2} | tmux load-buffer -w -`, gated on output like `^Y`.
- [ ] README: the `alt-c` line says real lines, tabs kept.
- [ ] Green, lint, commit `fix: alt-c copies the marked rows' real lines, tabs and tails included, and skips the cap row`.
- [ ] Review; fix findings.

### Task 3: An in-place edit keeps the stored quote (8)

**Files:** `scripts/tmux-review`, `tests/run.sh`.

- [ ] Fail-first: a store whose `>` quote is a 300-byte line, the file rewritten so line 3 is different text, `--edit` with `EDITOR` = a script that appends a word; the store's `>` quote is unchanged on the branch, the 160-byte display fragment on master. Also: an editor that empties the buffer deletes the record (test debt).
- [ ] `seed_comment` takes a `frag` mode (prints `STORE[$i.frag]` of the first record) or a sibling `seed_frag`; `edit_comment` passes it to `attach` as the fragment when `inplace=1`.
- [ ] Green, lint, commit `fix: an in-place ^E keeps the stored quote instead of re-reading a line the agent may have moved`.
- [ ] Review; fix findings.

### Task 4: The gutter past 9999 lines and the cursor glyph (7)

**Files:** `scripts/tmux-open-path`, `tests/run.sh`.

- [ ] Fail-first: the six pinned strings become `%6`; a `\ No newline at end of file` line carries a blank gutter (test debt); the asked line starts with `▸`; a hunk at line 12345 shows `▸12345`.
- [ ] `%5` → `%6` in the five `printf`s and the `sed 's/^.\{11\}'` in the test; `▶` → `▸` in `mark_line` and its comment.
- [ ] Green, lint, commit `fix: the preview gutter holds 99999 lines and marks the row with a single-width glyph`.
- [ ] Review; fix findings.

### Task 5: The legend fits 80 columns (3)

**Files:** `scripts/tmux-review`, `tests/run.sh`, `README.md` (if the legend is quoted there).

- [ ] Fail-first: a static block extracts the three key lines of the `--header=$'...'` literal and asserts each ≤ 80 cells (`wc -m` on the ANSI-free text); master's first line is 88.
- [ ] Reflow: drop the words that only restate the key (`pin the typed comment` → `pin`, `copy file+diff` → `copy file`), keep every key; the fourth (dynamic) line is untouched.
- [ ] Green, lint, commit `fix: the review legend fits 80 columns`.
- [ ] Review; fix findings.

### Task 6: README, gate, PR

- [ ] README: `^E` "one buffer per row" sentence (item 2's rule), `alt-c` line (Task 2), nothing else.
- [ ] `make check-par` unpiped, green; `bash tests/freeze-rig/run.sh gate <abs>/tests/freeze-rig/fixed-example.conf` unchanged from master (120/125).
- [ ] Whole-branch review (subagent); fix findings; commit.
- [ ] Push, `gh pr create` with the decision table, the measurements, and "Closes #22"; note on #14 the moved-line state (item 8) as its follow-up.
