# R2: internal consistency of the four skill files

Lane R2, read-only, 2026-08-27. SKILL.md mtime Aug 27 10:03 (templates.md 10:00, gates.md 10:02, community.md 10:00). Line numbers refer to those versions.

| # | sev | where | finding | action |
|---|---|---|---|---|
| 1 | BLOCKER | SKILL.md:10 vs :63 | "nothing waits on them" vs "the one irreversible go/no-go waits for the owner" | improve: "nothing reversible waits; the irreversible go/no-go does, with a named wait window" |
| 2 | BLOCKER | SKILL.md:59 vs :28, templates.md:23 | "No report files" while research/verify lanes deliver `docs/research/.../<lane>.md`, `verified.md`; nobody says who commits the lane file | improve: define report (message) vs record (committed doc); orchestrator writes records from lane text |
| 3 | BLOCKER | SKILL.md:29 vs :32, :56 | 2-3 verify lanes write one `verified.md`, but "no two lanes own one file" | improve: one verified-<lane>.md each, orchestrator merges |
| 4 | MAJOR | gates.md:14 vs SKILL.md:32, :56 | gates define seam files lanes append to; SKILL forbids shared files outright, never defines "seam" | improve: ownership rule gets "except seam files listed in the freeze" |
| 5 | MAJOR | SKILL.md:45-49, templates.md:82, :24 | three grading vocabularies (severity+status, GREEN/YELLOW/RED, CONFIRMED/TIGHTENED/REFUTED) with no mapping; review template carries severity only; "verified" is a fourth status word | improve: one mapping table; add status slot to templates 7 |
| 6 | MAJOR | community.md:22 vs SKILL.md:45 | community "Take" says two axes, "not a five-rung ladder"; SKILL keeps a four-rung ladder; the number is wrong and the take was not taken | improve: "Considered, kept ours because", or adopt (blocking)/(non-blocking) |
| 7 | MAJOR | gates.md:26 vs community.md:47, :78 | gates: re-run once on an idle box and merge on the clean run; community: never prove a flake by one green run | improve: "run the block alone N times (name N)" |
| 8 | MAJOR | templates.md:27 vs :25-26 | README immutable after acceptance, yet carries a dated implementation history written after acceptance; rules.md edited in place | improve: name the append-only sections as the exception |
| 9 | MAJOR | SKILL.md:31 vs :32 | lane 0 lands the freeze in state 5 but lanes are planned in state 6; lane 0 never defined | improve: define lane 0, or move Freeze after Plan |
| 10 | MAJOR | gates.md:18 vs templates.md:66-76 | gate demands size and dep count "in the PR body"; lane-report template has no slot | add: "Size / deps: <before -> after> (or n/a)" to template 6 |
| 11 | MAJOR | gates.md:7, :10, :17, :18 | four gates (toolchain off PATH, rig branch AND trunk, leak sweep, compiled-artifact checks) reachable from no lifecycle row, mistake row, or template slot | improve: kickoff brief Gates slot says "every gates.md row that applies, each named" |
| 12 | MAJOR | SKILL.md:32, :36, :37, :38 | Plan, "seam merges diffed and stated", Manual run, Release have no template; templates 5 and 10 are not in the lifecycle table | add: a plan comment shape; fold manual-run and release into the recap |
| 13 | MINOR | SKILL.md:49 vs gates.md:22-27 | RED rule never points at "Grading a red run" | improve: add the pointer |
| 14 | MINOR | gates.md:24, SKILL.md:76, templates.md:57 | "trunk" undefined; used as a literal git ref that fails on this repo (master) | improve: "trunk = the default branch" once; `<trunk>` in the command |
| 15 | MINOR | SKILL.md:63 | "Ask once" lists five things across states 4, 10, 12 and mid-flight; no wait window before defaults take effect | improve: one ask per state; defaults take effect after <window> |
| 16 | MINOR | SKILL.md:10, :33, :73, templates.md:56 | three issue kinds; nothing says decisions and amendments go only on the parent | improve: one sentence |
| 17 | MINOR | templates.md:57 vs SKILL.md:36, :76 | lane and orchestrator both own retargeting; lanes are gone at merge time | improve: orchestrator retargets; drop from the brief |
| 18 | MINOR | SKILL.md:35-36 | rebasing a leaf changes its sha, so row 9's MERGE no longer names the merged sha; the repeat re-verify is implied, never stated | improve: "every rebase repeats row 9" |
| 19 | MINOR | SKILL.md:30 vs templates.md:21, :27 | README with decisions is inside the research PR (state 2) but decisions arrive in state 4; "acceptance" undefined | improve: research PR merges after Decide; acceptance = that merge |
| 20 | MINOR | many | undefined terms: lane 0, skeleton, seam, integration lane, packaging lane, trunk, collection issue, mirror, block (two meanings), the rig, keep/skip/replace audit, shared stash, stream discipline | add: a ten-line glossary |
| 21 | MINOR | SKILL.md:25-39 | "Done when" judgment-only in rows 2, 3, 5, 9; no command named in rows 1, 6, 10 (second half), 12 | improve: proxies ("verified.md lists every row of every lane file", "an Output: line per mutation") |
| 22 | MINOR | SKILL.md:28, community.md:49, gates.md:10 | perf evidence n: "n and conditions" vs "10+ runs per side" vs no n at all | improve: gates:10 adds "n runs per side, n named" |
| 23 | MINOR | templates.md:63 vs :93 | brief says report "in your reply"; only the re-verify request says "as a message, not only as a final line" | improve: copy the clause into the brief |
| 24 | NIT | templates.md:24 vs :25 | verified.md must list "rules corrected" but rules.md is optional | improve: "(none if no rules.md)" |
| 25 | NIT | community.md:29, :40, :41, :54, :65 | "Take" items with no rule behind them | improve: relabel "Considered" or add the rule |
| 26 | NIT | SKILL.md:39 | "agent memory dir" and "sync mirrors" undefined | improve: name them |

What an agent following SKILL.md alone would do wrong (instruction lives only in a supporting file): use `git stash` (templates:62); mutate in the lane's worktree instead of a scratch copy (templates:89, gates:8); append to a shared file without the unique header or the syntax check (gates:14); edit the README after acceptance (templates:27); file a flake without block/load/isolated result (templates:108); write "Freeze amendment N" without the "(from lane L / PR #P)" and "additive" clauses (templates:51); take defaults with no "Asked <date>" line (templates:37). SKILL:12's "read both files" is the only mitigation.

Did: read all four files in full, grepped every term across them, checked both directions of the lifecycle/template and mistake/gate maps.
Did not: verify against the cockpit repo, git history, or the community URLs; edited nothing.
Could do next: draft the glossary and the grading map as a patch; check which gates.md rows the cockpit Makefile implements.
