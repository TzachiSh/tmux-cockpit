# verified.md — refute pass over the #75 research (PR #90)

Two fresh lanes graded the load-bearing claims. v2-75 covers the inventory and prior-art docs; v1-75 covers the remedies measurements.

## v1-75 — remedies.md

Box: 12 cores, tmux 3.7c, ambient load 2.4 to 5.0 from peer lanes (not the 2.0 the doc records). Fixture rebuilt from the block at run.sh:4386-4411 in a throwaway `git archive` tree. All tmux work on private `-L v1*` sockets; cleanup verified (no v1 sockets or servers left, default server untouched).

### 1. Wall inflates 3.9x, CPU 1.27x — SPLIT: wall CONFIRMED, CPU inflation REFUTED, idle CPU figure REFUTED

`TIMEFORMAT='%R %U %S'; { time scripts/tmux-review --lines $RV49 >/dev/null 2>&1; }`, n=7 per condition against N background `bash -c 'while :; do :; done'` spinners. Idle: wall 0.150 mean, CPU 0.111. 12 spinners: wall 0.427, CPU 0.103. 24 spinners: wall 0.653, CPU 0.112. Wall inflation 4.35x, so the 3.9x claim holds and is if anything conservative. CPU inflation is 1.01x, not 1.27x. CPU did not move at all across three load conditions, which makes the a1 remedy stronger than the doc argues.

The doc's absolute idle CPU of 168-197 ms does not belong to the site under test. That number reproduces only by timing the *pipeline* form, `--lines | grep -c .`: CPU 0.190-0.230 against wall 0.154-0.184. The command at run.sh:4411 is not a pipeline and costs 93-135 ms of CPU. This propagates: decision 2 sets the new budget at "3x idle CPU max, so 0.6 s against 197 ms measured", but 0.6 s is about 5x the real figure. Three times the measured CPU maximum is 0.40 s.

### 2. Headroom is 1.23-1.4x idle, wall 142-163 ms — CONFIRMED, range widens slightly

n=10 at load 2.87: wall 0.126 to 0.158, mean 0.142, headroom 1.27x to 1.59x against the 0.2 tooth, 0/10 red. The doc's number is right and its conclusion is right.

The doc understates how little load it takes to redden. Three spinners, a quarter of nproc, produced 1/10 red (wall mean 0.175). Six spinners, also 1/10 (mean 0.185). Twelve spinners, 10/10 red (0.383 to 0.519). The doc needed 24 spinners to make its point; a quarter of nproc already does it, and that is the number the remedy argument should carry.

### 3. Bash's `time` sums CPU of waited-for descendants — CONFIRMED

`{ time bash -c 'for i in 1 2 3 4; do (spin 1s) & done; wait'; }` reports wall 1.000, user 3.977, twice repeated. A depth-two grandchild reaped by the child counts (wall 0.999, user 0.995). A child disowned and orphaned does not (wall 0.003, user 0.000). The mechanism is exactly as described.

### 4. 280 ms registers, 350 ms does not; the merged form is reliable — CONFIRMED

Nested outer/inner tmux servers reproducing the run.sh:2081 rig, with `bind -n C-DoubleClick1Pane set -g @dbl yes` as the detector. Gap 0.28: 5/5. Gap 0.35: 0/5. Merged `send-keys -H $CDBL $CDBL`: 5/5 idle and 5/5 under 24 spinners at loadavg 5.4.

Caveat the doc should keep visible: the current two-call 0.05 s form also registered 5/5 under the same 24-spinner load. Neither form could be turned red. The merge is hardening against an unreproduced failure, not a demonstrated fix.

### 5. The a2 rejection — mechanism CONFIRMED, one site's stated reason TIGHTENED

Shim test: a `printf` shim first on PATH, logging one line per call. 200 iterations of `d="$(printf "%s" "$i")"` produced 0 hits. Controls confirm the shim works: `env printf` gave 5/5, and `enable -n printf` gave 5/5. A subshell fork of a builtin is invisible to a PATH shim.

Git history splits the two sites. For 5000 branches, confirmed: 50eb7ed removed `disps+=("$(printf '%s %-32s %-16s %s' ...)")`, and its message says the fork was 99% of a 16.9 s build. For 4900 rows the doc is wrong about the shape. The fix is 30ba10f, and the lines it removed are `$(( ))` arithmetic and bash loop passes; one removed comment states the code already used `printf -v` "rather than `disp=$(printf ...)`, which forked a subshell PER ROW". So the 4900-row regression involves no fork at all. The rejection stands a fortiori, but the site should be described as bash interpreter passes, not a builtin fork. The rejection itself is safe either way, since neither shape produces an execve.

### 6. d2, the inner session inherits the runner cwd and a long cwd truncates the clicked row — TIGHTENED: real mechanism, but conditional

Nested rig, `new-session -d -s t -x 80 -y 24` from a 63-character scratch cwd, `capture-pane -p` after `send-keys 'echo MARK README.md' Enter`.

With the prompt drawn first (0.5 s settle), row 0 is the truncated prompt and `$ echo MARK README.md` has wrapped to row 1: the clicked row carries no README.md. A 4-character cwd keeps it on row 0. But at the block's own timing, no settle, the tty echo wins and row 0 is the bare `echo MARK README.md` with README.md present, 5/5 with the same long cwd. The truncation only bites when the prompt wins the race: the doc presents the long cwd and the race as two facts, and they are one contingency; the cwd alone is harmless. Also the truncation point is `...flakes-7`, not the doc's `...flakes-75$ ech`. Remedy still correct: the real research worktree prompt already fills 75 of 80 columns, so a branch name six characters longer overflows.

### 7. run.sh:5546, non-atomic archive then unlink — CONFIRMED from the code

`done_store()`, scripts/tmux-review:736-748: line 743 `cat -- "$st" >> "$d/delivered.md" || return 1`, line 745 `rm -f -- "$st" "$st.rows" "$st.base"`. Two statements, nothing atomic between. The C-s path reaches it at scripts/tmux-review:1969-1970 via `tmux run-shell -b "... --deliver ...; rc=$?; if [ $rc -eq 0 ]; then ... --done <store>"`. Order is paste, append, unlink. `until_file` at run.sh:5545 can return inside the 743-745 gap, and the bare test at 5546 then sees comments.md.

### 8. run.sh:3588, the stderr came from the list-panes at 3579 — TIGHTENED on source, CONFIRMED on the no-op

Wording is tmux's own: on a session with only window 0, `list-panes -t b:1` prints exactly `can't find window: 1`, exit 1. But "the only unredirected stderr in the setup" is wrong. All three of FCP1 (`a:0`), FCP2 (`b:0`) and FCP3 (`b:1`) at run.sh:3577-3579 are unredirected. Only 3579 targets window 1, so the attribution survives; the word "only" does not, and a fix lane dropping `2>/dev/null` should expect two neighbours to speak too.

The no-op confirmed twice. Source: scripts/agent-state:14-17, `case "$pane" in %[0-9]*) ;; *) exit 0 ;; esac`. Empirically `agent-state error ""` printed nothing, exited 0, left `@panestate` empty; `agent-state error %0` set it to `error`.

## v2-75 — inventory.md and prior-art.md

Read-only against master `6c6424a` (run.sh 9094 lines). Note first: inventory.md says "9094 lines at master `a77f882`". `a77f882`'s run.sh is 8882 lines; 9094 is `6c6424a`. The line numbers are current, the base hash is not.

### 1. The count disagreement — TIGHTENED, with one REFUTED

Both lanes read the same tree. `sleep`/`timeout` counts are identical at `a77f882` and `6c6424a` (255 and 33), so drift is not the cause. Different patterns are. Mine, on `tests/run.sh`:

- `grep -oE 'sleep [0-9][0-9.]*'` → **255**
- `grep -c 'sleep '` → **259** (this is prior-art's 259: lines, not calls)
- `grep -cE '^[^#]*sleep [0-9]'` → **241** (inventory's 240 sits one off this)
- `grep -oE '\btimeout [0-9.]+'` → **33**, of which 7 are prose inside comments (3209, 4010, 5748, 5749, 7072, 7074, 7124), leaving **26 executed** `timeout N` invocations. Inventory's **29 is REFUTED**: no pattern I ran returns it.
- Prior-art's 33 timeout histogram (`0.05, 0.15, 0.2, 0.6, 1x2, 1.5, 2x2, 3x2, 5x11, 6x3, 10x7, 15`) reproduces exactly.

The load-bearing correction is that **all three sleep numbers are inflated**. 79 of the 255 are not waits at all: `sleep 300` x61, `sleep 3600` x7, `sleep 30` x7 are long-lived pane fixture commands (`exec -a claude sleep 300`), and `sleep 8` x4 are fd holders, one commented "a holder, not a wait" (run.sh:908). Another 56 of the 57 `sleep 0.1` are poll cadence inside bounded retry loops; the 57th (7038) is inside a slow-git shim heredoc. Net fixed waits that gate an assertion: **about 119**. Inventory's table body agrees (110 green sites plus its named red sleep rows); its headline 240 does not.

Prior-art's "176 outside poll helpers" is reproducible but mislabelled: 255 minus the 79 fixtures is exactly 176. It excluded fixture sleeps, not poll helpers. Its "33 wall-clock assertions" is the timeout count restated in the same sentence; my two honest patterns for assertion strings saying under/within N give 32 and 26.

### 2. Red-map completeness — REFUTED

I re-derived the list from `gh issue view 75 --comments` (body plus 11 comments). Eighteen assertions are named, and inventory maps all eighteen with correct current line numbers.

The nineteenth is missing. The **PR #77 comment is unmapped**: `grep -n '#77' inventory.md` returns nothing, and the red column cites only the issue body, L1, L2, L5, PR #68, #69, #73, #76, #87. That comment reports 1023/4 under load, 1026/1 idle, five distinct failures, and names one that is not a wall budget: "one fzf keystroke assertion in `tmux-review --loop`". No table row is marked red for a `--loop` keystroke assertion. Row 5029 ("--loop gives up when fzf cannot start") is marked "no" and is an exit-code check with no keystroke in it. A whole comment's worth of evidence, including one failure outside the wall-budget class, never reached the table.

### 3. Six spot-checked rows — five CONFIRMED, one TIGHTENED

- **4411** CONFIRMED. `timeout 0.2` at 4410, `ok` at 4411, string and threshold exact.
- **4479-4495** TIGHTENED. `timeout 0.15` at 4479, `ok` 4480, `bad` 4481, so the row labelled "run.sh:4481 onward (3 asserts)" actually starts on the previous assertion's `bad` branch. And only **two** downstream assertions read `$STATE/rows.cached`: 4496 (`cmp` cached against fresh) and 4497 (`grep -c ●`). The third, "a row that exists only in the snapshot is served from it" (4489-4494), re-runs `--rows` and greps stdout; it never touches `rows.cached` and cannot cascade from the kill. The doc's own criterion yields 2, not 3.
- **1988-1992** CONFIRMED. `sleep 2` at 1988, `capture-pane` case at 1991, `ok` 1992.
- **2081-2092** CONFIRMED. `sleep 0.05` inline at 2084 between the two `-H` sends, `sleep 1.5` at 2079 and 2085, `ok` 2088, else-branch `bad` 2092.
- **5545-5546** CONFIRMED. `until_file "$RASESS/delivered.md"` at 5545, bare `[ -e "$RASESS/comments.md" ]` at 5546.
- **3570-3588** CONFIRMED. `asFC` writes at 3583-3585, `is "two blocked and one errored, across sessions"` at 3588; no sleep, no timeout, no clock on that path. Separately, row `run.sh:5744` is off by one: the `ok "a repeat render exits 0"` is on line **5743**.

### 4. Rule 44's location — TIGHTENED

Wording at `rules.md:108-109` matches the quotation verbatim. No copy in `AGENTS.md`, `README.md`, `tests/`, or `scripts/`, confirmed by `grep -rn "never a clock\|never a sleep\|pin a count\|poll a condition"` over those paths.

The cited grep is wrong, though. `grep -rn "rule 44\|rule-44\|Rule 44" . --exclude-dir=.git` returns **one** hit (`references.md:11`), not two: rules.md:108 reads "44. Pin a count…" and never contains the string "rule 44". Also `tests/run.sh:8385` states the principle in prose ("a fork count, never a clock"), so "not in the suite itself" holds for the rule text, not the idea.

### 5. Prior-art's PR conversions — CONFIRMED, one detail TIGHTENED

- #18 comment `5428474294`, verbatim: fork-count `20 + 19*5`, `timeout 1` becomes `timeout 5` as a hang guard, "under 1s" becomes "exits 0". The serial sentence prior-art quotes is in that same comment.
- #36 → PR #37, MERGED onto master. Diff: `timeout 0.05` → `timeout 5`, "a repeat render returns in under 50ms" → "a repeat render exits 0". The sentinel plant lines are unchanged context, so it did pre-exist. Live today at run.sh:5743 with the #36 comment at 5746-5750.
- PR #46, MERGED 2026-08-26, `tests/run.sh` only, **+11/-2**, exactly the claimed shape: two `until_cap` waits, `exec -a claude sleep 3600`, plus a 40 x 0.1 s poll until 50 panes report `claude`. TIGHTENED on one detail: the two deleted lines are `sleep 0.8` and `exec -a claude bash`. There is **no `sleep 0.3`** in that diff.
- Stacking note confirmed: #34 CLOSED (head `fix/lines-timing-gate`, base `fix/fleet-timing-gate`); #39 MERGED, same head onto master.

### 6. The serial-group claim — TIGHTENED

Exactly 6 `# group: serial` at 3924, 4323, 4466, 5735, 6748, 7065, plus 5 `deliver` and 2 `etmux`, as claimed. 1655, 2888 and 3843 are unmarked (nearest preceding lines are plain block titles).

"Only `run.sh:4411` sits in a serial block" is wrong. The marker at 4466 heads the block titled at 4467, "tmux-review --rows: re-marking after a comment reads the cache, not git", which contains the L2-reported `timeout 0.15` budget at 4479-4481 and its cascade. **Two** red budgets are serial, not one. That strengthens the argument rather than weakening it: serialization has now visibly failed on both.

### 7. "No load handling" — CONFIRMED

Prior-art's exact grep returns 3 hits, none load-related: run.sh:971 (`sshdest ssh prod-box uptime`, a payload word), run.sh:22 and run.sh:1387 (comments about past flakiness). No retry, no skip-under-load, no load sampling, no `CI` branch. `tests/par.sh:18` is `J="${J:-$(nproc 2>/dev/null || echo 4)}"`, `nproc` for `J` only.
