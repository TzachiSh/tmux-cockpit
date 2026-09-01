# r1-75 — Inventory of time-sensitive assertions in tests/run.sh

Read-only sweep of `/home/tzachi/.tmux/plugins/tmux-cockpit/tests/run.sh` (9094 lines) at master `a77f882`, against issue #75's body and its 11 comments.

Note on line numbers: #75 quotes numbers from older trees (`5372` for "the store itself is gone", `1814` for the `sleep 2`, `1890-1919` for the ctrl+double-click block). The file has grown since. Every row below carries the number on current master, with the issue's number in the last column where it differs.

## Constructs found

Five classes, defined by how the clock reaches the verdict:

- **wall-budget (timeout verdict)**: `timeout N cmd`; exit 124 is itself the `bad`. The budget is the assertion.
- **wall-budget (measured)**: elapsed time from `SECONDS`, `date +%s%N`, or `EPOCHREALTIME`, compared to a constant.
- **timeout-kill cascade**: `timeout N` is a hang guard, its stdout feeds one or more content assertions; a kill empties the output and reds all of them.
- **sleep-then-assert**: fixed `sleep`, then `capture-pane` / `show-option` / file read feeding an assertion, with no poll.
- **mouse-timing**: two `send-keys -H` mouse sequences separated by `sleep 0.05`, relying on tmux accumulating a double-click.

## Table

| file:line | assertion | construct | threshold | red in #75? |
|---|---|---|---|---|
| run.sh:1219 | "a 4000-token clicked line is bounded, not just slow" | wall-budget (timeout verdict) | `timeout 0.6` | yes, issue body run 5 |
| run.sh:1655 | "5000 branches build their rows well under 2s" | wall-budget (timeout verdict) | `timeout 2` | yes, L2 lane comment and PR #68 verifier comment |
| run.sh:2888 | "a 400000-line single-file diff renders well under a 2s budget" | wall-budget (timeout verdict) | `timeout 2` | yes, PR #73 reviewer |
| run.sh:2895 | "and still leads with a real stat summary" | timeout-kill cascade from 2885 | same `timeout 2` | yes, PR #73 reviewer |
| run.sh:3843 | "800 untracked files list in well under 1s" | wall-budget (timeout verdict) | `timeout 1` | yes, PR #68 and PR #73 reviewers |
| run.sh:3952 | "a 12001-row diff returns fast, not after a 10s+ parse" | wall-budget (timeout verdict) | `timeout 10` | no |
| run.sh:4030 | "a 6,000,030-row tree answers with a notice" | wall-budget (timeout verdict) | `timeout 10` | no |
| run.sh:4116-4126 (4 asserts) | "the bump fixture is over the 4 MB cap…", "…while its diff is two rows", "a one-line bump in a 4.6 MB lockfile is still reviewable" | timeout-kill cascade | `timeout 10` at 4120 | no |
| run.sh:4176-4194 (4 asserts) | "…and its per-file AVERAGE estimate is 103 bytes…", "one 5 MB line among 100k short ones caps on bytes…", "the skewed notice is one row…", "…and with only a SHORT line changed…" | timeout-kill cascade | `timeout 10` at 4178, 4196 | no |
| run.sh:4233-4244 (4 asserts) | "the cross-file fixture is 26 changed rows…", "…and its whole real diff is well under 4 MB", "every row is there, both files", "and the unrelated Go file is not lost with the skewed one" | timeout-kill cascade | `timeout 10` at 4237 | no |
| run.sh:4264 block | "the untracked fixture is 6 MB on disk, and tracked-clean" and siblings | timeout-kill cascade | `timeout 10` | no |
| run.sh:4372 | "a 5000-row under-cap diff parses in well under 3s" | wall-budget (timeout verdict) | `timeout 3` | no |
| run.sh:4411 | "4900 rows build in under 0.2s (measured 210ms bash, 65ms awk)" | wall-budget (timeout verdict) | `timeout 0.2` | yes, four times: PR #69 verifier, #80 (dup), PR #87 verifier run 1, plus a master run |
| run.sh:4459 | "--rows re-marks a 4000-row diff in well under 5s" | wall-budget (timeout verdict) | `timeout 5` | no |
| run.sh:4480 | "a 500-comment re-mark over 4900 rows takes under 0.15s" | wall-budget (timeout verdict) | `timeout 0.15` | yes, L2 lane |
| run.sh:4481 onward (3 asserts) | the marking assertions reading `$STATE/rows.cached` | timeout-kill cascade from 4479 | same `timeout 0.15` | yes, L2 lane, cascaded from the kill |
| run.sh:5029 | "--loop gives up when fzf cannot start, instead of spinning" | wall-budget (timeout verdict) | `timeout 5` | no |
| run.sh:5407 | "and the list comes back in under 1.5 s" | wall-budget (measured, `date +%s%N`) | `< 1500 ms` | no |
| run.sh:5417 | "and it answers without hashing the file" | wall-budget (measured, `date +%s%N`) | `< 500 ms` | no |
| run.sh:5744 | "a repeat render exits 0" | wall-budget (timeout verdict) | `timeout 5` | no; its `timeout 0.05` ancestor was #36 |
| run.sh:7053 | "the fleet queue gathers 50 panes' git state well under the old serial time" | measured `date +%s%N` plus `timeout 15` guard | `< 5000 ms` | no |
| run.sh:915 | "and the read returns as soon as it has both fields" | measured, `SECONDS` | `< 2s` | no |
| run.sh:924 | "and it gives up inside its own deadline" | measured, `SECONDS` | `<= 4s` | no |
| run.sh:1964 | "a slow drip does not hold the hook open" | measured, `SECONDS` | `<= 4s` | no |
| run.sh:3657 | "a stalling writer is abandoned in time" | measured, `SECONDS` | `<= 3s` | no |
| run.sh:8223 | "no marker: one stat, under 200 ms with no tools on PATH" | measured, `EPOCHREALTIME` | `< 0.2s` | no |
| run.sh:8272 | "no fixer: no added latency" | measured, `SECONDS` | `<= 3s` | no |
| run.sh:8281 | "the hook returns within 4 s of the fixer finishing" | poll (`rh_wait`, 100 x 0.1s) | 10s cap | no |
| run.sh:8296 | "a dead fixer pane does not hold" | measured, `SECONDS` | `<= 4s` | no |
| run.sh:8301 | "an expired deadline does not hold" | measured, `SECONDS` | `<= 4s` | no |
| run.sh:8306 | "stop_hook_active true never holds" | measured, `SECONDS` | `<= 4s` | no |
| run.sh:1566 | "notify gives up rather than wedging the hook" | timeout verdict, rc compared | `timeout 6` | no |
| run.sh:1115 | "tm from a popup moves the real client to the session" | timeout-kill cascade plus `sleep 1` | `timeout 3` | no |
| run.sh:2236 | "tmux-branch survives HOME unset" | timeout-kill cascade | `timeout 5` | no |
| run.sh:7253 | "↵ on a clean row leaves the queue instead of re-listing" | timeout verdict, rc compared to 0 | `timeout 5` | no |
| run.sh:2088 / 2092 | "ctrl+double-click reaches the picker with 2 panes" | mouse-timing, `sleep 0.05` between clicks, `sleep 1.5` around | 0.05s inter-click | yes, PR #76 reviewer (1 of 3, long cwd) and PR #87 verifier run 2; issue quoted 1890-1919 |
| run.sh:2297 | "ctrl+double-click fires in a mouse-mode pane" | mouse-timing, `sleep 0.05` at 2294 | 0.05s inter-click | yes, PR #87 verifier run 2 |
| run.sh:1992 | "the current branch is marked in the list" | sleep-then-assert, `sleep 2` at 1988 then capture-pane | 2s | yes, L1 lane and PR #68 verifier; issue quoted 1814 |
| run.sh:2814 | "enter on a folder opens the browser, not the editor" | sleep-then-assert, `sleep 2.2` at 2810, `sleep 2` at 2812 | 2.2s | yes, L5 lane |
| run.sh:2820 | "and a file inside it opens" | sleep-then-assert, `sleep 0.8` / `sleep 2` at 2818-2819 | 2s | yes, L5 lane |
| run.sh:3521 | "jump lands on the blocked window" | sleep-then-assert, `sleep 2` at 3520 | 2s | yes, L5 lane |
| run.sh:3588 | "two blocked and one errored, across sessions" | no clock: `agent-state` writes at 3583-3585, read immediately | none | yes, L5 lane, with `can't find window: 1` |
| run.sh:5546 | "the store itself is gone" | poll on a different file (`until_file delivered.md` at 5545), then bare `[ -e comments.md ]` | 4s poll cap | yes, issue body run 4; issue quoted 5372 |
| run.sh:2787 | "two marked files both open" | sleep-then-assert | 2.5s | no |
| run.sh:2030-2042 | branch-picker create/pause assertions | sleep-then-assert | 1.2s | no |
| run.sh:3143-3152 | ctrl-x / ctrl-u branch-key assertions | sleep-then-assert | 1.2s | no |
| run.sh:3497 | "prefix u from the first blocked pane cycles to the second (#45)" | sleep-then-assert | 1.5s | no |
| run.sh:4931 | "Enter with nothing typed pins nothing (no accept-all)" | sleep-then-assert, deliberate per comment at 4929-4930 | 0.3s | no |
| run.sh:4942 | "plain enter does nothing: still in the picker" | sleep-then-assert | 1s | no |
| run.sh:7338 | `C-b r` review binding assertions | sleep-then-assert | 1.5s | no |
| 110 sites total | remaining green sleep-then-assert sites | sleep-then-assert | 0.1s to 2.5s | no; clustered at 255-284, 390, 470, 1713-1716, 2001-2005, 2021-2042, 2074-2085, 2285-2295, 2399-2520, 2548, 2591-2678, 2722-2819, 2911-2920, 3128-3152, 3420, 3493-3556, 5868-6798, 7191-7452 |

## Counts

748 assertion calls; 29 `timeout N` uses; 240 fixed `sleep` calls; 198 poll loops. By class: 14 wall-budget with the timeout as the verdict, 13 wall-budget measured against `SECONDS` / `date +%s%N` / `EPOCHREALTIME`, 8 timeout-kill cascade blocks covering roughly 20 downstream assertions, 2 mouse-timing, 110 sleep-then-assert sites.

Distinct assertions reported red across the 11 comments: 19. Of those, 8 are timeout-verdict wall budgets, 3 are the `rows.cached` cascade, 2 are mouse-timing, 5 are sleep-then-assert, 1 fits no timing class.

## Fits no class

`run.sh:3588` "two blocked and one errored, across sessions" has no sleep, no timeout, no clock: three `agent-state` calls at 3583-3585, then `@cockpit_fleet` read through `display-message`. L5 reports it red with `can't find window: 1`, a tmux server error, not a budget overrun. Sibling at 3607 adds a window after that read. Server-state race against peer tmux servers.

## Budgets that are not assertions

Excluded from the table: `run.sh:7122` (`timeout 5` inside a `send-keys` string; the 80 x 0.1s poll deliberately outlasts it, comment at 7124) and `run.sh:7044` (`timeout 15` guarding the measured gather at 7053).
