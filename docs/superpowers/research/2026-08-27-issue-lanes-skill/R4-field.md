# R4: did the #61 run's lessons land, and where did the process get skipped

Lane R4, read-only, 2026-08-27. SKILL.md mtime Aug 27 10:03. Read: four skill files; 15 memory files; issues 61, 63, 70, 71, 75 (+64/65/66 bodies); PRs 62, 67, 68, 69, 73 (+74, 76, 77 comments, commit lists, timelines); README/verified.md/workflow.md on PR #62.

## A. Lesson ledger (23 rows below: 13 PARTIAL, 9 ABSENT, 1 present but ineffective; the 20 PRESENT rows live only in the transcript)

| lesson | source | skill |
|---|---|---|
| amendments found by lane 0's review land before lane 0 merges, or every lane rebases | cockpit-tui-lanes, #61 amendment 1 | PARTIAL: SKILL.md:31 has the path, not the timing |
| release before the integration lane's fetch path is verifiable | cockpit-tui-lanes | PARTIAL: SKILL.md:38 orders Release after Manual run |
| after merge, pull the shared checkout and tell the owner to reload | cockpit-tui-lanes | PARTIAL: "sync mirrors" |
| a mutation that leaves the test green means the wrong knob or a vacuous test; remove the mechanism the test names | tui-data-branch ("two mutations proved the wrong thing") | PARTIAL: gates.md:8 says both directions, nothing on choosing the mutation |
| a fixture whose earlier branch decides the outcome cannot pin a later arm | tui-data-branch, #73 | ABSENT |
| count rows with startswith; n and conditions | tui-data-branch | PARTIAL: templates.md:29 |
| kill stray suite runs by readlink /proc/PID/cwd | tui-data-branch, scratchpad-shared | ABSENT |
| lane name + $$ on every scratch file, not only logs | scratchpad-shared | PARTIAL: gates.md:6, templates.md:61 cover logs only |
| survey not final until the owner's resource list is in | cockpit-tui-decision | PARTIAL: SKILL.md:63 lists the ask, no rule |
| an owner's mid-flight "what about X" gets a measurement lane, not a stop | cockpit-tui-decision, #61 04:51 | PARTIAL: SKILL.md:63 covers overturn only |
| standing owner contract (screen classification, distribution tax) | tui-decision-contract | PARTIAL: Constraints slot only |
| foreground commands only inside a lane agent | named-subagent-reports | ABSENT |
| after a restart notice, check for a live old agent before re-dispatching | named-subagent-reports | ABSENT |
| all worktrees share one stash; fail-first by copy-aside | worktrees-share-stash | PARTIAL: templates.md:62 hazard line, no gate, no alternative |
| check `git worktree list` + ListAgents for a peer before claiming | check-peer-sessions (was workflow.md:177 on #62) | ABSENT |
| a test that only string-matches a binding proves nothing | popup-needs-pane-id | PARTIAL |
| a worktree test must load the worktree's code, not the installed copy | popup-needs-pane-id | ABSENT |
| cross-lane one-line edit needs a grant and a record | PR #74 | PARTIAL: SKILL.md:56 forbids, no exception path |
| a file the skeleton still fixes after leaves fork cannot be a leaf's file | PR #74, 8f42393 | PARTIAL |
| every integration line needs an owner in the DAG (board arm bounced L0>L2>L6) | #61 three comments | ABSENT |
| lane 0 pushes its branch at kickoff or leaves have no base | PR #67, #68 bodies | ABSENT |
| tests sensitive to cwd length | #75 | ABSENT (correctly; flake issue) |
| `make check \| tail` recurred despite the memory | #75 body | gates.md:5 present; memory alone did not prevent it |

## B. Process deviations in the run

1. Owner decision list never posted on the issue in shape 3; it lived in README.md on unmerged PR #62. "Defaults taken" came 04:14 next morning. No "Asked <date>" line.
2. No PR body uses shape 6. All open "Part of #61. Closes #N." and prose.
3. No review used shape 7 on the PR; full findings went over messages, PR got one paragraph ("Full text is with the lane owner", PR #69). No Ran/Output on any PR.
4. First-pass reviews for #67, #68, #69 name no sha. Verification passes do.
5. PR #69 has no MERGE-at-sha on the PR; two fix commits after "merge after fixes", merged at 8f42393 06:06:17.
6. PR #67 merged two commits past its MERGE sha (c75861a): 52af942 is a content change from freeze amendment 2, no statement. #68 and #73 did state their seam merges.
7. PR #73: nobody retargeted before the base went; GitHub auto-retargeted (#69 merged 06:06:17, base_ref_changed 06:06:36). gates.md:13 "deleting the base closes them" is true only for an unmerged base (#34). community.md:31 has it right.
8. Leaves merged trunk in; never rebased. SKILL.md:36 says rebased.
9. Two author-side wrong mutations (tui-data-branch) and three reviewer-side vacuous tests (#74 F1, F2; #73 "deleting the arm stays green"). gates.md:8 caught them; the rule for choosing the mutation is unwritten.
10. Freeze amendment numbering: first accepted amendment (04:42) unnumbered; "Freeze amendment 1" at 05:10.
11. Two lanes owned theme.rs (L2 scope, F2 assigned to L0); board arm one-liner in L0's file "with the lead's say-so".
12. Skeleton branch not pushed at kickoff; #67 polled 5 minutes then opened against master.
13. PR #76's first review was dispatched and acted on by the author in one context.
14. Posted merge order #69 > #67 > #73 > #74 > #68 > #77 > #76; executed #69, #67, #74, #68, #77, #73. Each waited for its own MERGE.
15. First manual run on a hand-wired tree, declared; repeated on the merged tree. Correctly yellow.
16. Report files: none. The prohibition held.
17. `make check | tail` recurred once (#75: "said 1017/0 while the real exit was 2").
18. Stages 12 and 13 not reached: no tag, no release, #61 open with no recap, AGENTS.md has no tui line.

## C. Gates never exercised

gates.md:13 (base merged, not deleted unmerged; the why is overstated); gates.md:18 release half (dist profile only on tag push, never run; no size before/after in any body); gates.md:17 (installer dry run not evidenced; leak sweep by one lane of seven); SKILL.md:38 stage 12 (verified against file:// and a 404); SKILL.md:39 stage 13; SKILL.md:41 nudge (no idle lane this run); gates.md:26 step 3 (the box was never idle; "block alone, twice" was the substitute every time).
Exercised and trustworthy: gates.md:5, 6, 7, 8 (three catches), 9, 10, 11, 12, 14, 15 (partly), 16, 19, 20.

## Findings

1. MAJOR confirmed templates.md:78-89 vs SKILL.md:59 - reviews never reached the PR in Ran/Output shape; the durable record holds one paragraph. Improve: say which channel carries the full review (PR comment is the record, the message is the notification).
2. MAJOR confirmed PR #69, #67 - merges without a MERGE-at-sha on the PR. Improve: SKILL.md:36 covers any commit after the verified sha, stated by the reviewer on the PR.
3. MAJOR confirmed gates.md:8 - no rule for choosing the mutation; five wrong or vacuous in one run. Add: "the mutation removes the mechanism the test names; green after a mutation is a finding, never a pass".
4. MAJOR confirmed SKILL.md:32/56 - ownership broke twice. Add: the DAG names an owner for every integration line; findings on another lane's file route via Belongs elsewhere; a cross-lane one-liner is a numbered amendment.
5. MAJOR confirmed templates.md:57 - leaves had no base. Add: lane 0 pushes its branch before any leaf is dispatched.
6. MINOR confirmed gates.md:13 - wrong for a merged base. Improve: use community.md:31's wording.
7. MINOR confirmed templates.md:31-38 - decision list never posted on the issue. Add one line to SKILL.md:30.
8. MINOR confirmed SKILL.md:34/57 - author dispatched its own reviewer (#76). Improve: "dispatched by the orchestrator, never by the author".
9. MINOR confirmed templates.md:51 - numbering starts on acceptance; say so.
10. MINOR confirmed SKILL.md:36 - "rebased" never done. Improve: "rebased or trunk merged in, post-verdict diff stated by the reviewer".
11. MINOR confirmed gates.md:18 - release half untested until a tag exists. Keep, flag.
12. MINOR confirmed gates.md:17 - installer dry run and leak sweep not visible. Improve: slots in shape 6.
13. MINOR plausible gates.md:26 - "idle box" never existed; name the substitute.
14. MINOR confirmed - peer check dropped from workflow.md:177. Add to SKILL.md:55.
15. MINOR confirmed templates.md:62 - stash hazard has no why and no alternative. Add a gates.md row.
16. NIT confirmed gates.md:6 / templates.md:61 - $$ on every scratch file; readlink /proc/PID/cwd for strays.
17. NIT confirmed gates.md:12 - worktree test can exercise the installed copy. Add one clause.
18. NIT confirmed SKILL.md:38 - Release may precede the integration lane's manual run.
19. NIT plausible SKILL.md:69 - "foreground only" and "check for a live old agent" absent.
20. NIT confirmed #75 - cwd-length test is not skill material. Keep out.

Did: read everything listed; cross-checked every deviation against timestamps and shas.
Did not: read review texts that went over messages; touch the live server or any worktree; write anything.
Could do next: diff workflow.md on PR #62 against SKILL.md line by line (the skill's real parent, still carries the peer check and copy-aside); re-check gates.md:18 after tui-v0.1.0; check the #61 recap against shape 9 when it closes.
