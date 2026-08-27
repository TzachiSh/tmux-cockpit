# R1: skill form against the writing-skills rules

Lane R1, read-only, 2026-08-27. SKILL.md mtime Aug 27 10:03, 6883 bytes. Rules: superpowers writing-skills SKILL.md (WS), anthropic-best-practices.md (ABP), testing-skills-with-subagents.md (TS), plugin cache 6.3.0.

| # | sev | where | finding | action |
|---|---|---|---|---|
| 1 | MAJOR | SKILL.md:3 | description's second sentence names workflow outputs ("decision record, findings graded green/yellow/red, merge order"); WS:102 forbids summarizing the workflow. 340 chars, "Use when", third person: fine otherwise | improve: cut sentence two to triggers only |
| 2 | MAJOR | SKILL.md | 1211 words (1148 body) vs WS:220 target under 500; the authoring transcript said "1,105 words, I'll trim after the GREEN run" and the file grew 106 words instead | improve: drop what gates.md already holds (finding 4), about 230 words |
| 3 | MAJOR | templates.md:82 vs SKILL.md:45, :51 | grading rule demands severity+status and a did/did-not/could-next close, but no template has a slot for either; `grep -n 'GREEN\|did not' templates.md` is empty. WS:467: an omitted element needs a slot, not a reminder | add: status token after severity in shape 7; a Did/Did not/Could-next slot in shapes 6, 7, 9; delete SKILL.md:51 |
| 4 | MAJOR | SKILL.md:65-76, :53-59 | 6 of 8 Common-mistakes rows and 4 of 5 Prohibitions restate a gates.md row or a template slot; "retarget" stated 4 times, "freeze amendment" 4, "default on silence" 5 | remove: the restated rows and bullets; keep idle-lane and superseded-sha rows, or move them into gates.md |
| 5 | MAJOR | SKILL.md:53-59 | Prohibitions mix discipline failures (issue first, shared checkout, live system: WS row 1 wants prohibition + Excuse/Reality table + red flags; no Excuse/Reality table exists) with shape failures the templates already slot (no report files, file ownership, reviewer never edits) | improve: keep the three discipline prohibitions, add a 4-6 row Excuse/Reality table from the RED run, delete the shape ones |
| 6 | MAJOR | gates.md:26 vs community.md:47, :78 | "re-run the suite once on an idle box, and merge on the clean run" permits merge on one green re-run; community says never by one green run. WS:473 no nuance clauses | improve: "run only that block N times, N pinned in the brief; merge on N/N; otherwise RED" |
| 7 | MAJOR | all files | no baseline record: `grep -iE 'baseline\|scenario\|rationaliz'` hits only an unrelated Rust line. The authoring transcript has the RED run and "nine gaps"; the skill files record none of it, so WS:380 (edits need re-test) cannot be met | add: tests.md with the scenario prompt, the RED choices verbatim, the nine GREEN gaps, date |
| 8 | MINOR | community.md | background rationale, not a tool or 100+ line reference (WS:84-91); dated; "Struggled" paragraphs are narrative about other projects; SKILL.md never says when to read it; "three to put first" restates SKILL.md:10, :63 | remove: move to the repo's research folder, one pointer line in SKILL.md; or trim to Take bullets with URLs |
| 9 | MINOR | gates.md:5-20 | "Why it exists" column phrased as cockpit incidents ("eight false failures across six lanes"); right form, wrong content for a project-agnostic skill | improve: cut each to the mechanism, drop the counts |
| 10 | MINOR | SKILL.md:29, :35, :45, :47; templates.md:24, :84, :91 | "verify" has three meanings: state 3, state 9, and a status word | improve: rename state 3 "Refute" |
| 11 | MINOR | SKILL.md:3 | zero hits in the description for worktree, lane, freeze, reviewer, gate, subagent, team, multi-agent; body never says subagent/team either | improve: add "subagents", "agent team", "worktree", "stacked PRs" to the trigger sentence |
| 12 | MINOR | templates.md:3 vs :25 | "a slot that does not apply says none" vs "rules.md (optional)" | improve: pick one |
| 13 | NIT | SKILL.md:12 | backticked filenames, no links, no clause saying when to read community.md | improve: markdown links plus one when-clause each |
| 14 | NIT | SKILL.md:78-80 | red flags are a comma list with no closing action (WS:528-542 shape ends "All of these mean: ...") | improve: bullets plus "All of these mean: stop, post the default or the issue, then continue" |

Did: read the four skill files and the three writing-skills references in full; counted words, lines, description chars; grepped duplicates, keywords, test evidence in the skill dir, the memory file, the scratchpad roots and the two transcripts that mention the skill.
Did not: edit anything; run a scenario; check the nine GREEN gaps against the current text.
Could do next: pull the RED scenario and the nine gaps from transcript be834b45 into tests.md; re-run GREEN after findings 2-5 land.
