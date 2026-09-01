# r2-75 — Prior art and suite machinery for timing flakes (issue #75)

## 1. The rule this project already established

**Rule 44**, `docs/superpowers/research/2026-08-27-cockpit-tui/rules.md:108-109`, quoted verbatim:

> 44. Pin a count or poll a condition in every test; never a clock, never a `sleep`.
>     Five issues (#18/#32/#36/#44/#47) were wall-clock gates measuring the box under peer load; the prelude already has `until_cap` and `until_file`. (memory: fleet-timing-gate-branch)

`rules.md:108` never contains the literal string "rule 44" (it reads `44. Pin a count or poll a condition...`), so `grep -rn "rule 44\|rule-44\|Rule 44" .` returns exactly one hit outside `.git`: `docs/superpowers/research/2026-08-27-cockpit-tui/references.md:11` ("a predicate, never a sleep (our rule 44 in Rust)"). The rule text lives only in that research rulebook and is not in `AGENTS.md` or `README.md`; the *principle* is stated in prose inside the suite at `tests/run.sh:8385`.

Issue #80 (closed 2026-08-27 as a duplicate of #75, https://github.com/TzachiSh/tmux-cockpit/issues/80) is where the rule was used as an argument: "**A wall clock, which rule 44 forbids** — 'pin a count or poll a condition; never a clock, never a sleep'. It measures the box, not the code, and the box is shared." #80 also states the grading procedure for a red run (in the diff? passes alone? is it a clock? then flake class) which the owner had already written into the lane rules.

## 2. What the five prior issues decided, and what each PR converted

The owner's decisions, from the issue comments:

- **#18 → PR #31** (merged 2026-08-26, files `tests/run.sh`, `tests/par.sh`, `scripts/tmux-review`, `README.md`). 20-pane fleet dedup under `timeout 1`. Decision comment (https://github.com/TzachiSh/tmux-cockpit/issues/18#issuecomment-5428474294): "the timing gate is dropped: the fork-count check (`20 + 19*5`) on the next line is what pins the dedup and does not depend on the clock. `timeout 1` becomes `timeout 5` as a hang guard... the 'under 1s' assertion is now 'exits 0'." Also records that a killed command emptying a capture file drags the *next* assertion red — the cascade shape common to all five.
- **#32 → PR #39** (merged; PR #34 was the same commits, closed when its base was deleted). The 6-million-row `--lines` cap. Owner "took the third option": a PATH shim logging `git` argv, asserting one `git diff --numstat` and zero bare `git diff` — "counted, never captured". Fail-first preserved without a clock by moving the capture back ahead of numstat. `timeout 10` kept as a hang guard.
- **#36 → PR #37** (merged). Preview-diff cache hit under `timeout 0.05`. The block already planted a sentinel row in a hermetic cache dir that only a cache read returns; that sentinel is the proof, so the timeout became `timeout 5` and the assertion became "exits 0".
- **#44 and #47 → PR #46** (merged, `tests/run.sh` only, +11/-2). The two deleted lines are `sleep 0.8` and `exec -a claude bash`. #44: the fixed `sleep 0.8` before the capture became `until_cap` waits on the marker the next assertion reads. #47: 50 `new-window 'exec -a claude bash'` became `exec -a claude sleep 3600` (no bashrc load, the load spike itself) plus a poll until all 50 report `pane_current_command` = claude before scanning.

Three conversion shapes, in the owner's own order of preference: (a) delete the clock because a count beside it already pins the property; (b) pin a different observable property with a shim and a count; (c) replace a sleep with a poll on the exact thing the next assertion reads.

The second decision recorded on #18 matters for #75: keeping a block `# group: serial` "covers the suite's own load, not a peer session's suite or an ssh sync". Serialization was already tried and is known insufficient.

## 3. Suite machinery, with semantics

`tests/run.sh` is 9,094 lines, one file, ~115 blocks. Prelude ends at the first `echo "== `.

- **`until_cap` — `tests/run.sh:105-113`.** `until_cap <socket> <pane> <case-pattern>`: captures the pane 40 times, `sleep 0.1` between, returns 0 on the first match. Ceiling 4 s. The pattern is a case-glob with no embedded quotes; a quote inside an expanded pattern is a literal character, so patterns carry none.
- **`until_file` — `tests/run.sh:119-127`.** Same 40 × 0.1 s loop against `cat <file>`. 4 s ceiling.
- **`review_enter` — `tests/run.sh:114-118`.** Composed poll: wait `fleet>`, send Enter, wait `review>`. Only higher-level poll helper.
- **Counting — `tests/run.sh:11-14`.** `ok`/`bad`/`is` bump plain `pass`/`fail` counters. `skip_`/`need_tmux` (`:135-137`) count skips separately, deliberately not as passes. Totals + `exit 1` on nonzero fail at `tests/run.sh:9093-9094`.
- **Grouping — `tests/par.sh:37-44`.** `# group: NAME` directly above a block title shards group-mates together in source order. Live markers: 6 `serial` (`run.sh:3924, 4323, 4466, 5735, 6748, 7065`), 5 `deliver` (`:5856, 5940, 5992, 6029, 6076`), 2 `etmux` (`:1225, 3085`).
- **Serial perf lane — `tests/par.sh:102-117`.** Comment: "run against a clock or a 50 MB fixture... Measured: green at J=4, red at J=8 and J=12." Serial shards run at `-P 1` first, rest at `-P J`.
- **`tests/par.sh` also:** snapshots run.sh (`:32`) so a peer editor cannot hand a shard half a file, `bash -n` per shard (`:87`), sums `$PAR_COUNT` files (`:119-133`), flags a shard that died without reporting.
- **`tests/only.sh:15-36`.** Rebuilds prelude + all top-level functions + ERE-matched blocks. The isolation re-run every #75 comment cites.
- **`tests/changed.sh`.** Derives block ordinals a changed file could break; prints `ALL` when unsure; feeds `PAR_BLOCKS`.
- **`tests/green.sh`.** Content-keyed manifest stamp (sha256 per file read, plus tmux/fzf/git versions) gating `make gate`.
- **Makefile:** `test`, `test-par`, `test-only PAT=`, `check`=lint+test, `check-par`=lint+test-par, `gate`=green check → check-par → stamp, `test-changed`; `tui`/`tui-test` never prerequisites of `check`.
- **Freeze rig — `tests/freeze-rig/countvisits.sh:2-8`**, the principle stated: "This is a COUNT, not a timing. It is identical on a fast laptop and a slow CI box, it does not care about load from other jobs."

## 4. Box load handling: none

`grep -n "loadavg\|getloadavg\|uptime\|nproc\|retry\|RETRY\|flaky\|CI=" tests/run.sh` → 3 hits, none load-related (`:971` an ssh assertion whose payload is the word `uptime`; `:22` and `:1387` comments about past flakiness). No retry, no skip-under-load, no load sampling, no `CI` branch. `tests/par.sh:18` reads `nproc` only to pick J.

## 5. Current surface

`tests/run.sh` holds 255 `sleep` tokens; excluding 79 long-lived pane fixtures (`sleep 300`/`3600`/`30` fixtures, `sleep 8` fd holders) leaves 176 timing sleeps, which still includes the two poll helpers' own `sleep 0.1`. The "33" figure is the textual count of `timeout` occurrences, not a separate measurement: 7 of the 33 are comment prose, 26 execute. Timeout budgets: `0.05, 0.15, 0.2, 0.6, 1×2, 1.5, 2×2, 3×2, 5×11, 6×3, 10×7, 15`.

Load-bearing for #75: some of the reported clocks are **not** serial, and serialization has visibly failed on the ones that are. `5000 branches ... well under 2s` (`run.sh:1655`), `400000-line single-file diff ... under a 2s budget` (`:2888`), `800 untracked files ... well under 1s` (`:3843`) are unmarked parallel-pool blocks. Two red budgets are serial, not one: `4900 rows build in under 0.2s` (`:4411`) sits under the marker at `:4323`, and the `timeout 0.15` re-mark budget at `:4479-4481` sits under the marker at `:4466`, which heads the block titled at `:4467`. Both still went red, which is the same finding #18 already recorded: a serial group covers the suite's own load, not a peer's. Drift caveat: the `sleep 2` cited as `run.sh:1814` on 2026-08-27 is now comment text in the RCE-via-directory-name block.
