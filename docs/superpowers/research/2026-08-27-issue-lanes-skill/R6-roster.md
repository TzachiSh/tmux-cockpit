# R6: who works on what; does the skill have a roster slot

Lane R6, read-only, 2026-08-27. SKILL.md mtime Aug 27 10:03 (gates.md 10:15). Owner's question: the issue should say who works on what, each lane's name and title.

## 1. Does the skill have a roster slot? No.

SKILL.md:25 has a "Who" column of roles, never names. SKILL.md:32 row 6 Plan names the lane DAG's content ("owner, files, order, start condition") and is the only lifecycle row with no location under "Artifact, where". templates.md:56 shape 5 (kickoff) carries lane + scope + files, sent to one agent, never posted. Shape 6 (lane report) opens "SHA <head>, branch, base, PR #<n> (state)" with no lane field; on #61 every comment is authored by TzachiSh, so the lane prefix in the text is the only attribution and shape 6 does not require it. Shape 9 lists PRs and shas, "who" only for decisions. Shape 4's "Freeze amendment N (from lane L / PR #P)" is the one place lane name + PR are mandated together. "roster" appears nowhere.

## 2. Was a roster ever posted on #61? No.

Closest: 04:14:25Z "Lanes in flight: L0 (#63, feat/tui-skeleton), L2 board (feat/tui-board), L4 packaging (feat/tui-dist), L5 docs (docs/tui) ... L1, L3, L6 start when L0 pushes. Merge order L0, then L1-L5, then L6" (prose, only L0 has a sub-issue number, never edited); 04:51:49Z "L1, L3, L6 dispatched; L0 #69, L2, L4 #68, L5 #67 done or pending retarget"; 06:05:16Z "#69 -> #67 -> #73 -> #74 -> #68 -> #77 -> #76" (PR numbers, no lane names). Nothing on #61 names a reviewer, a MERGE sha, or a merge sha per lane. Research lanes on PR #62 were never named on the issue; file names stood in.

The #78 roster (07:22Z) is good on one row per lane, agent names matching ListAgents, V-to-R pairing, records location. A cold reader cannot: find a lane's output (no "where" column, "research PR follows" has no number); tell which sha (SKILL.md mtime) a finding was made against; date the status cells; trust "running" cells unless the comment is edited later.

## 3. How a lane is named in practice

| item | lane in title | lane in branch | first body line |
|---|---|---|---|
| #63, #70, #71 | yes ("cockpit-tui L0: ...") | n/a | "Part of #61." |
| PR #69, #73, #68, #67 | yes | no (feat/tui-skeleton, feat/tui-data, feat/tui-dist, docs/tui) | "Part of #61. Closes #N." |

Lane name in 7/7 titles, 0/7 branches, 4/4 bodies say "Part of". Titles drift between sub-issue and PR; the L-number is the stable key.

## 4. Smallest change

One issue comment, posted by the orchestrator at dispatch, edited in place at dispatch, lane report (PR + sha), reviewer verdict (reviewer + MERGE sha), merge (merge sha). GitHub keeps the edit history. The recap copies the final table. A body slot is worse: body edits do not show in the timeline.

Shape 11, "Roster (issue comment, orchestrator edits in place)":

| lane | scope | where | reviewer | status at sha |
|---|---|---|---|---|
| L0 | crate in tui/, make tui, CI | #63, PR #69, feat/tui-skeleton | L0-review | merged f1e198c |
| R1-form | skill form audit | docs/.../R1-form.md | V1-refute-form | reported 2026-08-27 07:14Z at SKILL.md mtime 10:03 |

"where": sub-issue, PR and branch for a code lane; file path for a research lane. "status at sha": dispatched <date> / reported <sha> / MERGE at <sha> by <reviewer> / merged <sha> / re-dispatched / dropped <why>. "none" where a cell does not apply.

SKILL.md row 6 artifact cell becomes: "keep/skip/replace audit; roster comment on the issue (shape 11: lane, scope, where, reviewer, status at sha), edited in place at dispatch, report, MERGE and merge". Optional: shape 6 opens with "<lane> (#sub, PR #p):".

## Findings

1. MAJOR confirmed SKILL.md:32 - row 6 has content but no location. Improve: name the roster comment.
2. MAJOR confirmed templates.md - no shape carries lane + scope + output + status. Add: shape 11.
3. MAJOR confirmed #61 - no roster ever posted; DAG in prose across three unedited comments; no reviewer or sha per lane on the issue. Add: the #61 recap carries the final roster.
4. MINOR confirmed templates.md:69 - shape 6 has no lane field; all comments share one author login. Improve: open with "<lane> (#sub, PR #p):".
5. MINOR confirmed #78 roster - no where column, no sha, undated times. Improve: add them and edit the comment as lanes report.
6. MINOR confirmed - lane name in titles, not branches. Keep: the roster's where column maps them; no branch-naming rule.
7. NIT confirmed SKILL.md:25 - the Who column of roles invites reading it as the roster. Keep; names go in the roster comment.
8. NIT plausible #61 research - research lanes never named on the issue. Improve: the roster's where cell makes file-equals-lane explicit.

Did: read the three files; grepped; read #61 body and 15 comments with timestamps; #78 body and roster; titles, branches, states, merge shas, first body lines for #63-#66, #70, #71, PRs #67-#69, #73, #74, #76, #77.
Did not: edit anything; read PR review comments (reviewer names per lane live there).
Could do next: fill the #61 roster from the PR reviews for its recap; draft shape 11 and the row 6 cell as a patch after the owner reads the table.
