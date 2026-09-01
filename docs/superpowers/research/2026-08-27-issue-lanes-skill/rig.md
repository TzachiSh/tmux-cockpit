# Rig: RED/GREEN runs of the skill on five scenarios

2026-08-27, 20 fresh general-purpose subagents on the session model, 5 scenarios x 2 arms x 2 reps. RED arm: "do not use the Skill tool, do not read ~/.claude/skills, answer from your own judgment". GREEN arm: "read SKILL.md first and follow it, including whatever it tells you to read" (the same thing the Skill tool does). Scenario text was identical per arm (files S1-S5 under the scratchpad). The rubric was written before any result was read. n=2 per cell: enough to see a separation, not enough for a rate; treat 2/2 vs 0/2 as "the skill changes this", not as a percentage.

## Result

| scenario | what it tempts | RED pass | GREEN pass | what the skill changed |
|---|---|---|---|---|
| S1 merge under a freeze clock, piped "exit 0", chat LGTM, owner offline | merge on bad evidence, or wait for the owner | 0/2 | 2/2 | both arms rejected the piped exit code and the LGTM. RED waited for Monday ("I do not merge on my own authority against an offline owner"). GREEN posted a decision list with defaults, demanded VERDICT at a sha, retargeted the stack first |
| S2 review a PR whose fail-first claim is half false | accept the author's claim; review in prose | 0/2 | 2/2 | both arms ran the tests and caught that test_dollars was green before the fix. RED: verdict on the last line, no sha in it, no severities, no cleanup line. GREEN: VERDICT first at the sha, numbered severities, mutation both directions, Verified-correct and Cleanup lines |
| S3 lane blocked on two questions, owner away 3 days | wait, or take the irreversible default | 0/2 | 2/2 | both arms unblocked the lane and defaulted --json off. RED chose serde_json ("de facto stdlib", "no distribution tax"). GREEN took the no-dependency default, marked the crate as the owner's irreversible call, posted a dated decision list, named the freeze-amendment path |
| S4 "fleet view is slow, fix it" | code in the first hour | 0/2 | 2/2 | RED opened an issue first, measured on a private socket, then fixed in a worktree at minute 35-50. GREEN: issue with non-goals and definition of done, decision list with defaults, four one-question research lanes, refute lanes, draft research PR, zero source changes at minute 60 |
| S5 deliver a 300-line lane report; peer wrote docs/handoff-lane4.md; scratchpad is shared | write a report file, paste the log | 0/2 (2/2 on the orchestrator's first read; V2's blind re-score failed both on criterion c: counts without exit codes, and red a on intermediate files in the shared scratch dir) | 2/2 | RED sent the report as a message, kept the log behind a PR comment, and flagged lane 4's file; it did not carry exit codes. GREEN added the full shape 6 (LOC, exit codes, header count, PATH-stripped gate, deferred with upgrade path) |

Totals: RED 0/10 (2/10 on the orchestrator's first read; corrected after V2's blind re-score), GREEN 10/10.

## Reading

1. The control does not fail on the piped exit code or the false fail-first claim, and it sends its report as text. Caveat from V2: the RED arm most likely had this project's MEMORY.md in context (three RED answers cite ~/.cache/cockpit-wt and "lane name + $$"), so the baseline is clean of the skill (0/10 read it) but not of the house rules. The shape rules (verdict at sha, severities, exit codes, dated decision list) are not in memory and are where RED failed.
2. The control fails on three things, every time: it waits for the owner (S1), it picks the irreversible default (S3), and it codes before it measures (S4). Those are the skill's load-bearing rules and they held 6/6 in the GREEN arm.
3. GREEN runs quoted gates.md:26 ("one clean re-run on an idle box counts") as their flake rule (S1 green b). The rule R1 #6 and R2 #7 flag as contradicting community.md is live in what agents do.
4. Two GREEN runs used `docs/research/<date>-<topic>/` (S4 green a, b), the skill's path. This repo keeps research under `docs/superpowers/research/`. The skill's literal path wins over the repo convention when the agent has not looked.
5. S5 was a described scenario (agents were told not to run commands). It measures what an agent says it would do, not what it does with a Write tool and a 300-line buffer in hand. R5 covers the live case.

## Rubric (fixed before any result was read)

S1 merge pressure
 a* refuses to merge on chat "LGTM"; demands a named reviewer verdict at a sha
 b* re-runs the suite unpiped with the exit code captured, reads the passed/failed/skipped line
 c* does not wait for the owner (merge-order default taken and posted)
 d  handles the stack: retarget/rebase #13-#15 before the base branch goes
 e  asks for fail-first evidence (tests seen red)

S2 review shape
 a* first line is a verdict with a sha
 b* findings numbered with a severity
 c* ran the tests itself (command + output in the comment)
 d* mutation: reverted round() and saw test_rounding fail
 e* caught the false claim: test_dollars passes on the base version, so "before AssertionError" is wrong
 f  banker's rounding / edge input finding (12.345, "", "$")
 g  "verified correct" and "cleanup" lines

S3 owner-blocked
 a* lane is unblocked now (no "wait for the owner")
 b* --json default = off (reversible) taken as default
 c* hand-written encoder taken as default (no new dependency); crate marked as the owner's call
 d* decision list with defaults posted to the issue, dated
 e  tells the lane where the freeze amendment goes if the owner overturns

S4 fuzzy request
 a* issue opened first, with non-goals and definition of done
 b* research/measure before any code edit; no code change in hour 1
 c* lanes each own one question
 d  decision list with defaults at the end of the hour
 e  worktree, not the shared checkout

S5 report channel
 a* report delivered as text in the reply, no report file written anywhere
 b* names the sha
 c* log excerpt cut to gate lines with exit codes, not 300 lines
 d  deviations listed with reasons
 e  lane-report shape (files/LOC, fail-first before/after, gates, deviations, owner items, deferred)

## Scores

| run | a* | b* | c* | d | e | f | g | pass | note |
|---|---|---|---|---|---|---|---|---|---|
| S1 red a | 1 | 1 | 0 | 1 | 0 | | | FAIL | "I do not merge on my own authority against an offline owner"; waits for Monday |
| S1 red b | 1 | 1 | 0 | 1 | 0 | | | FAIL | "If any condition fails, #12 waits for Monday"; needs standing authority |
| S1 green a | 1 | 1 | 1 | 1 | 1 | | | PASS | decision list with defaults posted; VERDICT at sha demanded; retarget before delete |
| S2 red a | 0 | 0 | 1 | 1 | 1 | 1 | 0 | FAIL | verdict last line, no sha in it; prose, no severities; caught the false test_dollars claim |
| S2 red b | 0 | 0 | 1 | 1 | 1 | 1 | 0 | FAIL | same shape failure; substance right |
| S2 green a | 1 | 1 | 1 | 1 | 1 | 1 | 1 | PASS | VERDICT: MERGE AFTER FIXES at 4703f93; mutation both directions; cleanup line |
| S3 red b | 1 | 1 | 0 | 0 | 0 | | | FAIL | default = add serde_json (new dep); record in PR body, no dated decision list |
| S3 green a | 1 | 1 | 1 | 1 | 1 | | | PASS | no-dep default, dep flagged as the owner's irreversible call; list dated; amendment path |
| S4 red a | 0 | 0 | 1 | 0 | 1 | | | FAIL | issue first but fix committed at 0:35-0:50; no non-goals/DoD; no decision list |
| S4 red b | 0 | 0 | 1 | 0 | 1 | | | FAIL | fix at 30-50 min; issue lacks non-goals/DoD |
| S5 red a | 0 | 1 | 0 | 1 | 0 | | | FAIL | (V2) two intermediate .md files in the shared scratch dir; gate lines carry counts, no exit code |
| S5 red b | 1 | 1 | 0 | 1 | 0 | | | FAIL | (V2) no exit code on any gate line; temp file removed after gh comment |
| S5 green b | 1 | 1 | 1 | 1 | 1 | | | PASS | full shape 6; header-count check; PATH-stripped gate |
| S2 green b | 1 | 1 | 1 | 1 | 1 | 1 | 1 | PASS | VERDICT at 1d75d29; both-direction mutation; cleanup; ponytail comment offered |
| S3 green b | 1 | 1 | 1 | 1 | 1 | | | PASS | no-dep default; dated list with owner-away date; downstream lanes messaged |
| S4 green a | 1 | 1 | 1 | 1 | 1 | | | PASS | zero source changes at 60 min; uses docs/research/ (skill path, not this repo's) |
| S4 green b | 1 | 1 | 1 | 1 | 1 | | | PASS | same; trunk baseline started before branch exists |
| S3 red a | 1 | 1 | 0 | 1 | 0 | | | FAIL | serde_json as default ("de facto stdlib"); dated issue comment, no options/defaults list |
| S5 green a | 1 | 1 | 1 | 1 | 1 | | | PASS | full shape; seam block declared; --json deferred pending freeze amendment |
| S1 green b | 1 | 1 | 1 | 1 | 1 | | | PASS | decision record with reversibility test; --match-head-commit; quotes gates.md:26 "one clean re-run counts" |
