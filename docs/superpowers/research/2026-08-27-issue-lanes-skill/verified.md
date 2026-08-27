# verified.md: refute lanes over R1-R5 and the rig

Five fresh lanes, each told to refute one research lane and default to REFUTED when evidence is thin. Grades: CONFIRMED (holds as written), TIGHTENED (holds, narrower; corrected text given), REFUTED.

## V2 over the rig (blind re-score of 20 transcripts, rubric read first, scores.md read last)

| claim | grade | reproduction |
|---|---|---|
| GREEN 10/10 | CONFIRMED | 100 cells scored; every GREEN cell agrees |
| RED 2/10 | TIGHTENED to RED 0/10 | S5 red a and b fail criterion c (gate lines carry counts, no exit code) and red a writes two intermediate files to the shared scratch dir before feeding them to gh. Orchestrator accepts the stricter reading; rig.md totals corrected |
| S1 red b criterion a | TIGHTENED | refuses the LGTM but asks no reviewer for a verdict at a sha; only the CI URL request carries a sha. Orchestrator's 1 stands as borderline |
| "the control does not fail on the piped exit code, the false fail-first, or the report file" | TIGHTENED | true for the first two. For the report file, the control sends text but leaves intermediates in the shared scratch dir; and the RED arm most likely had this project's MEMORY.md in context (three RED answers cite ~/.cache/cockpit-wt and "lane name + $$", which are memory-index rules). The baseline is clean of the skill (0/10 read it, 0/10 used the Skill tool) but not of the house rules. Shape criteria (verdict at sha, severities, dated decision list, exit codes) are where RED failed and the memory does not carry those |
| GREEN read templates.md and gates.md | CONFIRMED (10/10 by content, 0/10 by the Read tool: all ten used Bash cat) | tool-call lists per transcript |

Rubric ambiguities that produced every disagreement: does a gh intermediate file count as a report file; do pass/fail counts without "exit N" satisfy "gate lines with exit codes". Both settled toward the letter of the rubric.

## V3 over R3 (nine rows re-fetched with curl, raw pages)

| row | R3 grade | V3 on R3 | correction |
|---|---|---|---|
| 20 OWNERS (line 39) | REFUTED | CONFIRMED, correction strengthened | owners.md:181 reviewers exclude the author; :191 approvers "including the individual who opened the PR"; kubernetes/kubernetes prow plugins.yaml sets `require_self_approval: false`, so an author in OWNERS approves implicitly. Line 39 should say so |
| 18 Graphite (line 59) | REFUTED | CONFIRMED | no sentence on two changes in one file; line 34's "merge order is manual coordination" is also not on the page (closest: "manually manage every dependency and rebase") |
| 19 arXiv 2607.20189 | TIGHTENED | TIGHTENED | abstract figure is 2.6x slower to merge (size-matched); 1.5x more revision rounds is in the html body; 29,580 Gerrit chains; bottom-up merging is Gerrit's rule, not the paper's finding |
| 6 Nygard | TIGHTENED | CONFIRMED | no date line; "keep the old one around, but mark it as superseded" |
| 14 SRE action items | TIGHTENED | CONFIRMED | table is Appendix D, columns Action Item / Type / Owner / Bug; ch. 15 has no shape list |
| 27 Chromium | TIGHTENED | CONFIRMED | "pinpoint" appears only as a verb; perf trybots and bisect bots |
| 26 benchstat | CONFIRMED | TIGHTENED (R3's quote) | "at least 10 times"; "ideally 20" is not on the page |
| 25 actions/runner#1212 | CONFIRMED | CONFIRMED | policy stated in #353 comment; add it beside #1212 |
| Go "about a week" | CONFIRMED | CONFIRMED | proposal README, Accepted and Declined sections |

Practice count: 32 bullets (27 Take, 5 Struggled) by `grep -c '^- '` and by section. The memory's 33 has no source in the file; it should read "32 sourced practices plus a three-item put-first list".

Declared deviation: V3's first curl batch dropped twelve download files into the shared main checkout (a backgrounded cd); V3 deleted them, git status clean before and after, nothing tracked touched. Yellow: declared, harmless, and exactly the "never the shared checkout" hazard the skill names.

## V1 over R1 and R2 (every blocker/major re-read at its line)

Artifact drift: gates.md mtime moved from 10:02 to 10:15 while the lanes were reading it. The skill's author session (be834b45, a live peer) appended a release-tag row ("run `dist plan --tag=<tag>` before pushing a tag; delete a failed tag before recutting"). Every gates.md cite at or below line 22 in R1/R2/R4 is now one line lower (their :26 is :27). Declared here; the row itself is the gates.md:18 release lesson R4 said was untested, landing from the real tui-v0.1.0 cut. The peer was asked to hold further edits until the decision table posts.

| finding | V1 grade | what changed |
|---|---|---|
| R1 #1 description summarises workflow | TIGHTENED, MINOR | 352 chars, not 340; sentence two is trigger form; only "graded green/yellow/red" and "merge order for stacked PRs" name outputs. Replace with "graded findings and a merge order". Note R1 #11 adds "stacked PRs" to the same sentence |
| R1 #2 1211 words | TIGHTENED | count holds; the transcript later says "1,181 ... over the 500-word target on purpose", so the overage is a recorded choice, not drift. The 230-word cut collides with R2's additions (conflict 1) |
| R1 #3 no status / close slot | CONFIRMED | shape 9 already carries Shipped/Deviations/Deferred; add the close slot to 6 and 7 only |
| R1 #4 restated rows | TIGHTENED | 7 of 8 mistake rows restate a gate (the superseded-sha row IS gates:15); 4 of 5 prohibitions restate; "issue before fix" is the unique one |
| R1 #5 no Excuse/Reality table | TIGHTENED | gap real, source wrong: the RED baseline run (be834b45/subagents/agent-askill-baseline-*) committed none of the discipline failures, so there are no excuses to harvest; the table has to come from gates.md's why-column and the memory files. "Delete the shape prohibitions" deletes :59, which R2 #2 rewrites (conflict 2) |
| R1 #6 / R2 #7 merge on one re-run | CONFIRMED | line is gates.md:27 now. WS:473 does not apply (it is a conditional on an observable predicate); the fix needs an N slot in template 5 |
| R1 #7 no baseline record | TIGHTENED | the skill dir records nothing: true. "WS:380 cannot be met": false; the scenario and both runs are in three subagent transcripts (skill-baseline, skill-green, skill-check) under be834b45/subagents/, so a re-test is possible today. tests.md is R1's choice, not a rule |
| R2 #1 :10 vs :63 | TIGHTENED | fix :10 to "nothing reversible waits"; drop the "named wait window" (Apache: vote on the irreversible, no window) |
| R2 #2 no report files vs lane .md | CONFIRMED | orchestrator writes records from lane text; matches this run |
| R2 #3 shared verified.md | TIGHTENED | subsumed by #2 once the orchestrator is the writer; not a separate blocker |
| R2 #4 seam vs ownership | TIGHTENED | no contradiction if seam files are unowned; the defect is "seam" undefined and no freeze slot listing seam files |
| R2 #5 three vocabularies | TIGHTENED | GREEN/YELLOW/RED already keys on the :45 status words; only CONFIRMED/TIGHTENED/REFUTED is unmapped; same fix as R1 #3 |
| R2 #6 five-rung ladder | TIGHTENED | "five-rung" is community.md's generic foil, not a count of SKILL's rungs; SKILL:45 is already two axes; what was not taken is the axis labels. "Considered, kept ours because" is right |
| R2 #8 immutable README vs history | CONFIRMED | community:12 vs :71 carry the same tension |
| R2 #9 lane 0 undefined | TIGHTENED | only "define lane 0" is safe; moving Freeze after Plan breaks row 6's done-when |
| R2 #10 size/deps slot | CONFIRMED | |
| R2 #11 unreachable gates | TIGHTENED | three not four: gates:7, :10, :18; the leak sweep has a Cleanup slot at templates:85 |
| R2 #12 missing templates | TIGHTENED | manual run is already in the recap (templates:100); template 10 is reached from SKILL:73; remaining gaps: plan shape, template 5 pointer, release |

Conflicting pairs the owner must settle: (1) R1 #2+#4 shrink SKILL.md by ~230 words while R2 #5, #12, #20 grow it (mapping table, plan shape, glossary); (2) R1 #5 deletes SKILL.md:59, R2 #2 rewrites it; (3) R1 #1 removes "stacked PRs" from the description, R1 #11 adds it. Compatible: R1 #6/R2 #7, R1 #3/R2 #5, R1 #4/R2 #17, R1 #8/R2 #6, R1 #10/R2 #5.


## V4 over R4 (every deviation re-checked against gh timelines, UTC)

| claim | grade | evidence |
|---|---|---|
| B5 #69 merged with no MERGE-at-sha on the PR | CONFIRMED (for the GitHub record) | #69 has one comment, 05:12:33 "merge after fixes ... Full text is with the lane owner"; commits 0894e07 05:25, 8f42393 05:33; merged 06:06:17; no MERGE next to 8f42393 on #69, #63 or #61. The message channel was not read, so "nowhere" is not shown |
| B6 #67 merged two commits past c75861a | TIGHTENED | the content change (52af942, docs/tui.md and tests/run.sh) was pre-announced on #61 06:05:16 as freeze amendment 2 "after #67 merges"; it landed before the merge instead, never stated or diffed on the PR, on a PR whose body says "No code" |
| B7 #73 auto-retargeted by GitHub; nobody retargeted before | REFUTED | #73 base_ref_changed 06:06:36 by TzachiSh, then #74 06:06:38, #68 06:06:41, #77 06:06:42, #76 06:06:44 (a loop, in the posted merge order); #69 head_ref_deleted 06:07:58, 82 s later. GitHub's auto-retarget fires only after deletion. The plan and gates.md:13 were followed; the tui-data-branch memory line "GitHub auto-retargeted #73" is wrong (fixed). R4 finding 6 (gates.md:13's why overstated) stands on the changelog alone |
| B11 two lanes owned theme.rs | CONFIRMED | #64 scope lists tui/src/theme.rs; #69 review put F2 (theme.rs) on L0; 8f42393 "shrink the theme.rs F2 edit to the three items L2 does not own"; #74 "one line in L0's file, with the lead's say-so". Both declared and resolved by an explicit call: declared deviations, not silent |
| B13 #76 author dispatched its own reviewer | TIGHTENED | 05:59:44 author-voice comment carries the subagent's summary and the author's fixes (4d99fc4, later rebased out); the reviewer never wrote to the PR. Who dispatched it is not proven by the wording |
| B1 decision list never on #61 in shape 3 | TIGHTENED | none of the 15 comments is a numbered list with defaults; the README on PR #62 is. SKILL.md:30 says "README ... summary comment on the issue", templates.md:31 says "issue comment". The run followed SKILL.md:30. The defect is the two files disagreeing, plus no "Asked <date>" |
| B12 #67/#68 opened against master, base unpushed | CONFIRMED | #67 body "polled for 5 minutes"; #68 body "not pushed yet"; kickoff 04:14, #69 pushed 04:43, a 29-minute window; #68 retargeted at 05:04, #67 never |
| finding 3, five wrong or vacuous mutations | CONFIRMED | two wrong knobs (cache filter, seen set), three vacuous tests (#74 F1, F2; #73 stopped-str arm), each quoted |

Contradiction check on R4's actions 1-5: (1) "PR comment is the record" contradicts SKILL.md:59 as written and agrees with :34 and templates:78; the skill is already split, so :59 needs the clause. (2) widening "seam merges" to "any commit after the verdict" contradicts nothing; #69 and #67 broke the rule that exists, so this is a compliance failure and more words alone do not fix it. (3) additive to gates.md:8. (4) "a cross-lane one-liner is a numbered amendment" contradicts :56's absolute "never" and needs :56 reworded "except through a numbered freeze amendment naming the edit". (5) contradicts the run's own DAG, which started disjoint leaves before lane 0 pushed on purpose: a trade of 29 minutes of parallelism for a clean stack, not a rule conflict.


## V5 over R5 (binary decoded, hook fed seven payloads, run 1 reproduced)

| item | grade | evidence |
|---|---|---|
| built-in block | CONFIRMED | claude 2.1.247, `grep -aob tengu_subagent_md_report_blocked` at 198244868 (strings) and 211011545 (code, inside Write's validateInput): `if(t.agentId&&/^(REPORT\|SUMMARY\|FINDINGS\|ANALYSIS).*\.md$/i.test(...))`; Write only (Edit destructures old_string); anchored, so lane-report.md and handoff.md pass it |
| safety guards | CONFIRMED | agent-state:13-16 and review-gate:36-38 both `case "$pane" in %[0-9]*) ;; *) exit 0 ;; esac` before any tmux call; every cockpit hook in settings.json exits 0 with TMUX_PANE unset |
| hook payloads | CONFIRMED, TIGHTENED | (i) deny (ii) allow (iii) allow (iv) deny (v) deny, case-insensitive. Extra: handoff.md with no agent_id and no flag is DENIED, so the default blocks the main agent too; Edit on FINDINGS.md denied. Deny JSON matches the hooks reference (code.claude.com/docs/en/hooks): hookSpecificOutput.hookEventName, permissionDecision, permissionDecisionReason |
| run 1 reproduced | CONFIRMED | cwd V5-3664961, same command, 24.0 s, hook.log 2 lines (agent_id, agent_type=general-purpose), 0 .md files created, parent relayed "Write lane-report.md: failed. Error: Reports and handoffs go over the message channel"; no process left; live tmux untouched |
| finding 1 (channel undefined) | CONFIRMED, TIGHTENED | the Agent return IS the "reply" of templates.md:63 and FAILS templates.md:93 ("as a message, not only as a final line"); SKILL.md:59 does not settle it. Three words (message channel, reply, message) for two channels |

Correction to carry: the rung (b) text must pick one word and one target ("reply to the lead; the Agent return or a SendMessage to the lead both count; nothing to other lanes"). If hook (d) ships: say it blocks the main agent too unless SUBAGENT_ONLY=1; payloads i, iii, iv are the fail-first test.


## R6 (roster) was not refuted by a separate lane

Its claims are gh lookups (titles, branches, comment timestamps) quoted verbatim; the orchestrator spot-checked #61's 04:14:25Z comment and PR #69's title. Grade: plausible, not verified by a fresh lane. Carried into the decision table as YELLOW.

