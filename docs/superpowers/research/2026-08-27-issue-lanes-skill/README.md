# The orchestrating-issue-lanes skill, reviewed by its own process

Issue #78. 2026-08-27. Status: research merged, decisions posted on #78 with defaults; edits to the skill happen in a follow-up lane after the owner answers or the defaults take effect.

The skill lives at `~/.claude/skills/orchestrating-issue-lanes/` (SKILL.md 1211 words, templates.md 922, gates.md 595, community.md 857; 30 URLs). It was written 2026-08-27 from the cockpit-tui run (#61) and had not been reviewed. This round ran the skill's own lifecycle on it: issue first, five read-only research lanes, one rig, five refute lanes, this README, decisions with defaults.

## What was measured

| lane | question | file | findings |
|---|---|---|---|
| R1 | does the skill meet the skill-authoring rules (writing-skills, Anthropic best practices)? | [R1-form.md](R1-form.md) | 14 (7 major) |
| R2 | do the four files agree with each other; what is undefined? | [R2-consistency.md](R2-consistency.md) | 26 (3 blocker, 9 major) |
| R3 | does every community.md claim hold against its source? | [R3-sources.md](R3-sources.md) | 31 URLs: 13 confirmed, 17 tightened, 1 refuted |
| R4 | did the #61 run's lessons land; where was the process skipped? | [R4-field.md](R4-field.md) | 47-row ledger, 18 deviations, 7 gates never exercised; 20 findings |
| R5 | can the skill force one channel; what would? | [R5-channel.md](R5-channel.md) | channel inventory, built-in block found, hook written and run; 6 findings |
| rig | 5 scenarios x red/green x 2 reps, fixed rubric | [rig.md](rig.md) | RED 2/10, GREEN 10/10 |
| V1-V5 | refute R1-R5 and the rig scoring | [verified.md](verified.md) | see file |

## Rig result in one table

| scenario | RED (no skill) | GREEN (skill) | what the skill changed |
|---|---|---|---|
| S1 merge under a freeze clock, piped exit 0, chat LGTM, owner offline | 0/2 | 2/2 | RED waited for Monday; GREEN posted defaults and demanded VERDICT at a sha |
| S2 review a PR with a half-false fail-first claim | 0/2 | 2/2 | both caught the false claim; only GREEN produced the review shape |
| S3 lane blocked, owner away 3 days | 0/2 | 2/2 | RED chose the new dependency; GREEN took the no-dep default |
| S4 "fleet view is slow, fix it" | 0/2 | 2/2 | RED coded at minute 35; GREEN had zero source changes at 60 |
| S5 deliver a 300-line report, peer wrote docs/handoff-lane4.md | 0/2 | 2/2 | both sent text; RED left intermediates in the shared scratch dir and no exit codes on gate lines (V2's blind re-score) |

The control never fails on the piped exit code or the false fail-first, and it sends text rather than a handoff file (with this project's memory rules likely in its context, per V2). It fails every time on waiting for the owner, the irreversible default, code before measurement, and every report shape. Those carry the skill.

## Recommendation

Keep the skill. Its three load-bearing rules held 6/6 and the control lost 6/6 on them. Fix the record, not the rules: three internal contradictions, one backwards source, a rule that lets one green re-run pass a flake, and the lessons #61 taught that are still only in memory files. Trim what gates.md already holds. Define "message channel" in one line and stop relying on a built-in regex nobody named.

## Decisions for the owner

See the table below and the comment on #78. Defaults in force until overturned there.

Every item has a default; work proceeds on the defaults. Reversible means one revert of the skill files undoes it.

| # | decision | options | default | why |
|---|---|---|---|---|
| 1 | SKILL.md grows or shrinks (V1 conflict 1) | A shrink: mistakes table and restated prohibitions move into gates.md; glossary and grading map go into templates.md/gates.md / B grow: keep everything, add mapping table, plan shape, glossary to SKILL.md | A | SKILL.md is what every lane loads; 1211 words, 7 of 8 mistake rows restate a gate. The instruction-only-in-a-supporting-file risk is met by the kickoff brief quoting gates verbatim (templates 5 already does) |
| 2 | SKILL.md:59 "No report files" (V1 conflict 2) | A rewrite as report (message to the lead) vs record (committed doc the orchestrator writes from lane text) / B delete, the template slots cover it | A | R2 #2 is a blocker; the rig showed the slot alone is not read (S5 RED left intermediates) |
| 3 | Channel enforcement (owner's question) | A prose one-liner naming the allowed channels now, hook shipped as an example script in this repo with its 3-payload test, not installed / B install the deny hook in ~/.claude/settings.json / C also restrict MCP write tools on research lanes | A | B is a write to the owner's live settings and blocks the main agent's own report files unless SUBAGENT_ONLY=1; it is the owner's call. A is one edit and reversible |
| 4 | Flake rule gates.md:27 "re-run once on an idle box" | A "run only that block N times, N pinned in the brief (3 if unset); merge on N/N; file it" / B keep | A | contradicts community.md:47 and :78; #61 already did "block alone, twice"; an idle box never existed |
| 5 | Grading vocabulary | A keep severity x status, add the status token to shape 7, map TIGHTENED to YELLOW, note Conventional Comments as considered / B adopt (blocking)/(non-blocking) labels | A | SKILL.md:45 is already two axes; only the labels differ |
| 6 | Roster (owner's addition) | A add shape 11 (lane, scope, where, reviewer, status at sha) as an issue comment the orchestrator edits in place; row 6 names it; shape 6 opens with the lane / B keep prose | A | no roster ever existed on #61; three prose comments had to be joined by hand |
| 7 | community.md | A keep in the skill dir; apply the 18 corrections (R3, V3); delete "three to put first" (restates SKILL.md); Struggled bullets stay / B move to this repo's research folder, one pointer line | A | the Take bullets are what the rules cite; the file is read only when someone asks why. Corrections are the point |
| 8 | Baseline record | A add tests.md to the skill dir: the five scenarios, the rubric, the expected RED failures, a pointer to this folder and to the be834b45 subagent transcripts / B none | A | writing-skills says edits need a re-test; today the record lives in transcripts nobody can find |
| 9 | Description | A drop "green/yellow/red" and "merge order for stacked PRs" as outputs, keep "stacked PRs" as a trigger word, add "subagents", "agent team", "worktree" / B leave | A | V1: MINOR, only two tokens name outputs; conflict 3 resolved by keeping the word as a trigger |
| 10 | State 3 name | A rename "Verify" to "Refute" / B leave | A | three meanings of verify today |
| 11 | Who edits and who re-tests | A one edit lane on the four files (no repo, so edits land directly in ~/.claude/skills with a dated backup copy); then one fresh lane re-runs the five GREEN scenarios (n=1) and one refute lane re-reads every edited line / B the orchestrator edits | A | the skill's own rule: never approve your own work |
| 12 | The ~/.claude/skills dir is not under git | A put it under git (a private repo or a dotfiles repo) so edits carry a sha / B keep the dated backup copy | A | every report in this round had to cite an mtime instead of a sha; the peer edited gates.md mid-review with no history. Reversible: `git init` |

Asked 2026-08-27. If unanswered, the defaults are taken and the edit lane starts; overturn any item with a comment on #78.

## Rejected ideas

- Delete community.md (R1 #8). Rejected: the file is where the rules' reasons live; corrected, it stays. Decision 7.
- Move Freeze after Plan to fix "lane 0 undefined" (R2 #9 alternative). Rejected by V1: row 6's done-when needs the freeze's ownership table. Define lane 0 instead.
- One verified-<lane>.md per verify lane (R2 #3). Rejected by V1: once the orchestrator writes records from lane text, no file is shared.
- A "named wait window" on the irreversible go/no-go (R2 #1 action). Rejected by V1: Apache and SKILL.md:63 both say the irreversible one waits; a window contradicts both.
- Excuse/Reality table harvested from the RED baseline (R1 #5 as written). Rejected by V1: the baseline committed none of the discipline failures; the table would come from gates.md's why-column, which already exists.
- A branch-naming rule carrying the lane name (R6 #6). Rejected: the roster's where column maps branches to lanes.
- Rerun the RED arm with memory disabled to get a pure baseline (V2). Deferred: the shape criteria, which are the skill's real effect, are not in memory; cost without a decision riding on it.
- Install the deny hook globally now. Deferred to the owner (decision 3): it is a live-settings write and blocks the main agent too.

## Fail-first tests the edit lane needs

The edit lane records these before editing, each red before and green after:
1. `grep -c 'nothing waits' SKILL.md` finds "nothing reversible waits"; the old sentence is gone (R2 #1).
2. `grep -n 'report' SKILL.md` shows the report-vs-record sentence and no bare "No report files" (R2 #2, decision 2).
3. `grep -c 're-run the suite once' gates.md` = 0 and the N-runs line exists (decision 4).
4. templates.md shape 7 line carries `<confirmed|plausible|refuted>` after the severity token; shapes 6 and 7 end with a Did/Did not/Could do next slot (R1 #3).
5. templates.md has a shape 11 roster table with the five columns; SKILL.md row 6 names it (decision 6).
6. community.md:39 says the reviewer is never the author and an OWNERS author approves implicitly; line 59 no longer cites Graphite for same-file conflicts; the arXiv line carries 2.6x/1.5x (R3, V3).
7. `wc -w SKILL.md` is below 900 (decision 1) and every gates.md row is reachable from the kickoff brief's Gates slot (R2 #11).
8. Description contains "subagents" and "worktree" and not "green/yellow/red" (decision 9).
9. gates.md has a row for "fail-first by copy-aside, never git stash" and one for "the mutation removes the mechanism the test names" (R4 #3, #15).
10. tests.md exists and names S1-S5 with the rubric (decision 8).
11. The five GREEN scenarios re-run at n=1 after the edits still pass every required criterion (writing-skills re-test).

## Implementation history

- 2026-08-27 07:00 issue #78 opened; R1-R5 and 20 rig runs dispatched at 07:10; V1-V5 dispatched 07:16-07:20.
- 2026-08-27 07:41 draft PR #82 opened; V4 reported 07:26Z (one R4 deviation refuted: #73 was retargeted by hand, not by GitHub).

## Outcome (written after the edit rounds, same day)

The decisions were taken on the defaults and the skill moved to its own repository, TzachiSh/issue-lanes-skill, as decision 12 said (baseline commit = the 10:15 files). Two edit rounds ran there under the skill's own process, each with a freeze, lanes, reviewers and an audit.

| round | issue | PRs | what changed | measure |
|---|---|---|---|---|
| apply the review | issue-lanes-skill#1 | 6 merged | the three contradictions, the backwards OWNERS claim, the flake rule, shapes 11 (roster) and 12 (plan), status token, channel definition, lane name and title on every post, project-agnostic lint | gate 0/48 red at baseline to 61/68 green; but total words 3646 to 6899 |
| KISS | issue-lanes-skill#14 | 6 merged | every file cut by deletion with a rule map per PR; community.md and sources.md deleted (no lane ever read a URL: 0 of 20 rig agents); dates and provenance banned; prose lint from a measured comparison against three human-written process docs | 6899 to 3079 words; 60/60 checks; rig re-run GREEN 10/10, RED 0/1 |

The final refute lane (V-final) inventoried 208 baseline rules: 165 present, 22 duplicate-deleted, 14 decided-out, 7 lost. The 7 came from two lanes each cutting the same rule as a duplicate of the other's file; they are restored under epic issue-lanes-skill#29. A demo run on a throwaway project (TzachiSh/tally-demo) feeds the same epic.

Lessons that changed the skill: a template names no project, path, issue, sha, date or URL; a budget is set after the floor is measured, not before (three budgets were raised); a rule cut as a duplicate lives in the file a lane loads first; the Cleanup line pastes ls, never asserts; an audit per round, not per project.
