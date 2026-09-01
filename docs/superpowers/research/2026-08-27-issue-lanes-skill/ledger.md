# Findings ledger: keep / improve / remove / add

Consolidated from R1-R6, the rig and V1-V5. Status: GREEN = confirmed by an independent refute lane, or by a command anyone can re-run; YELLOW = plausible or refute-lane tightened; RED = refuted or a defect with no fix yet. Line numbers are the 10:03 SKILL.md / 10:00 templates.md / 10:15 gates.md (V1 shift applied).

Not every finding became a row: MINOR and NIT findings that no decision or edit rides on were dropped at that threshold, so a finding with no row here was declined, not overlooked.

## Add (to do)

| # | what | where | sev | status | source |
|---|---|---|---|---|---|
| A1 | report vs record: a report is the message to the lead; a record is a committed doc the orchestrator writes from lane text | SKILL.md:59 | BLOCKER | GREEN | R2 #2, V1 |
| A2 | "nothing reversible waits on the owner; the irreversible go/no-go does" | SKILL.md:10 | BLOCKER | GREEN | R2 #1, V1 |
| A3 | one line naming the allowed channels: reply or SendMessage to the lead, PR body, issue/PR comment; not files, scratchpad, .omc/, shared_memory, notepad, lane-to-lane messages | SKILL.md:59, templates.md:63 | MAJOR | GREEN | R5 #1, V5 |
| A4 | roster shape 11 (lane, scope, where, reviewer, status at sha) as an issue comment edited in place; row 6 names it; shape 6 opens with the lane | templates.md, SKILL.md:32 | MAJOR | YELLOW | R6 #1-4 |
| A5 | the mutation removes the mechanism the test names; green after a mutation is a finding about the test, never a pass | gates.md:8 | MAJOR | GREEN | R4 #3 |
| A6 | the DAG names an owner for every integration line; a finding on another lane's file routes via Belongs elsewhere; a cross-lane one-liner is a numbered amendment | SKILL.md:32/56 | MAJOR | GREEN (V4: :56 must gain "except through a numbered freeze amendment") | R4 #4 |
| A7 | lane 0 pushes its branch before any leaf is dispatched | templates.md:57 | MAJOR | GREEN (V4: costs the 29 min of parallelism #61 chose) | R4 #5 |
| A8 | status token `<confirmed\|plausible\|refuted>` in shape 7; Did/Did not/Could do next slot in shapes 6 and 7 | templates.md:82, :76, :86 | MAJOR | GREEN | R1 #3, R2 #5, V1 |
| A9 | "Size / deps: <before -> after> (or n/a)" slot in shape 6 | templates.md:70 | MAJOR | GREEN | R2 #10, V1 |
| A10 | gates row: fail-first by copy-aside, never git stash (all worktrees share one stash stack) | gates.md | MINOR | YELLOW | R4 #15 |
| A11 | gates row: "N runs of the block alone, N pinned in the brief" replaces "re-run once on an idle box" | gates.md:27 | MAJOR | GREEN | R1 #6, R2 #7, V1 |
| A12 | define lane 0 (skeleton lane: module list, stubs, freeze doc) in state 5 | SKILL.md:31 | MAJOR | GREEN | R2 #9, V1 |
| A13 | "seam" defined; the freeze's file-ownership section lists seam files | SKILL.md:56, templates.md:48 | MAJOR | GREEN | R2 #4, V1 |
| A14 | tests.md: scenarios, rubric, expected RED failures, pointer to this folder and the be834b45 subagent transcripts | skill dir | MAJOR | GREEN | R1 #7, V1 |
| A15 | plan comment shape; template 5 pointed to from row 7; release checklist | templates.md, SKILL.md:33 | MAJOR | YELLOW | R2 #12, V1 (narrowed) |
| A16 | glossary: trunk, seam, skeleton, integration lane, packaging lane, collection issue, block, keep/skip/replace | templates.md or gates.md tail | MINOR | GREEN | R2 #14, #20 |
| A17 | peer check before claiming: `git worktree list` + ListAgents | SKILL.md:55 | MINOR | YELLOW | R4 #14 |
| A18 | decision list is posted on the issue even when the README carries it; "Asked <date>" line | SKILL.md:30 | MINOR | YELLOW (V4: SKILL.md:30 and templates.md:31 disagree on where shape 3 lives; the run followed SKILL.md) | R4 #7 |
| A19 | "a reviewer is dispatched by the orchestrator, never by the author" | SKILL.md:34 | MINOR | YELLOW (V4: the author's comment carried summary and fixes; who dispatched is unproven) | R4 #8 |
| A20 | "every rebase or trunk merge after the verdict repeats row 9; the reviewer states the post-verdict diff on the PR" | SKILL.md:36 | MAJOR | GREEN (V4: a compliance failure on an existing rule; wording widens it) | R4 #2, R2 #18 |
| A21 | which channel carries the full review: the PR comment is the record, the message is the notification | SKILL.md:59 | MAJOR | GREEN (V4: :59 needs the clause or it contradicts :34) | R4 #1 |
| A22 | $$ and lane name on every scratch file; kill strays by readlink /proc/PID/cwd; a worktree test must load the worktree's code | gates.md:6, :12 | NIT | YELLOW | R4 #16, #17 |
| A23 | memory: named-subagent-reports names the built-in regex (REPORT/SUMMARY/FINDINGS/ANALYSIS*.md, Write, subagents only) | memory dir | MAJOR | GREEN | R5 #2, V5 |

## Improve (fix the text)

| # | what | where | sev | status | source |
|---|---|---|---|---|---|
| I1 | description: drop "green/yellow/red" and "merge order" as outputs; add subagents, agent team, worktree; keep "stacked PRs" as a trigger | SKILL.md:3 | MINOR | GREEN | R1 #1, #11, V1 |
| I2 | community.md:39 OWNERS: reviewer never the author; an OWNERS author approves implicitly (require_self_approval false) | community.md:39 | MAJOR | GREEN | R3 #1, V3 |
| I3 | community.md:59 drop the Graphite attribution for same-file conflicts; :34 cite arXiv 2607.20189 as Gerrit relation chains, 2.6x slower merge, 1.5x revision rounds | community.md:34, :59 | MAJOR | GREEN | R3 #2, #3, V3 |
| I4 | 15 further community.md corrections (Nygard date, SRE columns, Chromium, Gerrit, RFC 3614, PEP 12 quote, PEP 207, Conventional Comments nine labels, seniority, Rust perf, Apache votes, KEP verdict marked as ours) | community.md | MINOR/NIT | GREEN | R3 #4-18, V3 |
| I5 | gates.md:13 "deleting the base closes them" only for an unmerged base; use community.md:31's wording | gates.md:13 | MINOR | YELLOW (V4 refuted the #73 story: a loop retargeted five PRs 82 s before deletion; the why-column stands on GitHub's changelog alone) | R4 #6 |
| I6 | gates.md why-column: mechanism, not cockpit incident counts | gates.md:5-20 | MINOR | YELLOW | R1 #9 |
| I7 | rename state 3 "Refute" | SKILL.md:29 | MINOR | GREEN | R1 #10 |
| I8 | red flags as bullets ending "All of these mean: stop, post the default or the issue, then continue" | SKILL.md:78-80 | NIT | GREEN | R1 #14 |
| I9 | README immutability: name the append-only sections (implementation history, rules corrections) | templates.md:27 | MAJOR | GREEN | R2 #8, V1 |
| I10 | "the research PR merges after Decide; acceptance = that merge" | SKILL.md:30 | MINOR | YELLOW | R2 #19 |
| I11 | one ask per state, each with defaults | SKILL.md:63 | MINOR | YELLOW | R2 #15 (window part rejected by V1) |
| I12 | orchestrator retargets; drop it from the brief | templates.md:57 | MINOR | YELLOW | R2 #17 |
| I13 | parent issue carries every decision and amendment; sub-issues carry only the lane's report | SKILL.md:33 | MINOR | YELLOW | R2 #16 |
| I14 | rows 2, 3, 9 done-when get a checkable proxy | SKILL.md:28-35 | MINOR | YELLOW | R2 #21 |
| I15 | gates:10 adds "n runs per side, n named" | gates.md:10 | MINOR | YELLOW | R2 #22 |
| I16 | brief says "reply to <lead>; the Agent return or a SendMessage to the lead both count" | templates.md:63 | MINOR | GREEN | R2 #23, R5 #3, V5 |
| I17 | "Considered, kept ours because" line for Conventional Comments and the four other Take items with no rule | community.md:22, :29, :40, :41, :54, :65 | NIT | GREEN | R2 #6, #25, V1 |
| I18 | gates.md:17 installer dry run and leak sweep become slots in shape 6 | templates.md | MINOR | YELLOW | R4 #12 |
| I19 | numbering of freeze amendments starts on acceptance | templates.md:51 | MINOR | YELLOW | R4 #9 |
| I20 | memory: 32 sourced practices (27 Take, 5 Struggled), not 33 | memory dir | NIT | GREEN | R3, V3 |
| I21 | skill research path: "the repo's research folder" instead of `docs/research/` (this repo uses docs/superpowers/research/; two GREEN runs used the skill's literal path) | SKILL.md:28, templates.md:21 | MINOR | GREEN | orchestrator, rig reading 4 |

## Remove (delete)

| # | what | where | sev | status | source |
|---|---|---|---|---|---|
| D1 | 7 of 8 Common-mistakes rows (restate gates.md); keep the idle-lane row or move it to gates.md | SKILL.md:65-76 | MAJOR | GREEN | R1 #4, V1 |
| D2 | the 4 restated prohibitions; keep "issue before fix" and the live-system rule | SKILL.md:53-59 | MAJOR | GREEN | R1 #4, #5, V1 (not :59, which A1 rewrites) |
| D3 | "The three to put first" (restates SKILL.md:10, :63) | community.md:75-79 | MINOR | GREEN | R1 #8 |
| D4 | "the number is wrong (five-rung)" claim: not a defect, community.md's foil | none | | REFUTED | R2 #6, V1 |
| D5 | SKILL.md:51 "Reports close with" once the slot exists | SKILL.md:51 | MINOR | GREEN | R1 #3 |

## Keep (do not do)

| # | what | why | source |
|---|---|---|---|
| K1 | the three load-bearing rules: defaults instead of waiting, reversible default, research before code | 6/6 in the rig, control 0/6 | rig |
| K2 | four-rung severity x status | already two axes | R2 #6, V1 |
| K3 | review shapes 6 and 7 as they are (plus the new slots) | the rig's whole S2 separation is the shape | rig |
| K4 | gates.md:18 release half | untested until a tag exists; the 10:15 row is the first real lesson | R4 #11 |
| K5 | lane name in titles, not branch names | roster maps them | R6 #6 |
| K6 | cwd-length test flake, transcript facts | not skill material | R4 #20, ledger |
| K7 | the built-in report regex as a safety net | it is narrow and undocumented; do not rely on it (A23) | R5, V5 |
