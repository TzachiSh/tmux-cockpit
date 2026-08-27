# R3: every community.md claim against its source

Lane R3, read-only, 2026-08-27. community.md mtime Aug 27 10:00, 79 lines, 31 URLs. All 31 fetched with WebFetch; raw text pulled with curl for 11 ambiguous ones. None unreachable.

Count: 31 claims, 13 CONFIRMED, 17 TIGHTENED, 1 REFUTED. Practices: 32 distinct bullets (27 Take, 5 Struggled); the memory's "33" counts a restated summary item.

| # | line | claim (short) | grade | what the source says |
|---|---|---|---|---|
| 1 | 8 | KEP template sections, dated Implementation History | CONFIRMED | headings match; history comment lists dated milestones incl. retired/superseded |
| 2 | 9 | PEP 12 rejected-ideas wording | TIGHTENED | exact text: "Why certain ideas that were brought while discussing this PEP were not ultimately pursued"; the line quotes a paraphrase |
| 3 | 9 | PEP 1 terminal statuses stay published | CONFIRMED | Rejected/Withdrawn/Superseded; Superseded-By / Replaces headers |
| 4 | 10 | Rust template splits unresolved questions in two | TIGHTENED | three buckets: before merge, during implementation, out of scope |
| 5 | 11 | Go: doc in repo, decision on issue, Active > Likely > Accept/Decline | CONFIRMED | |
| 6 | 12 | Nygard ADR: dated, immutable, supersede never edit | TIGHTENED | Nygard: one decision, Title/Context/Decision/Status/Consequences, reversed ones kept and marked superseded; no date field, nothing on never editing (date is MADR's) |
| 7 | 12 | MADR Considered Options with pros and cons | CONFIRMED | |
| 8 | 15 | RFC 3614: owned, time-boxed goals | TIGHTENED | "nothing good happens without an owner", six-month goals; motivation is unreviewed/misaligned RFCs, not "unowned proposals stall" |
| 9 | 15 | ncameron: unowned stall, feedback wave draining | TIGHTENED | 54 open RFCs over a year old, nobody closes them; comment volume overwhelms readers; neither "unowned" nor "draining" |
| 10 | 16 | KEPs gated on deadlines become paperwork | TIGHTENED | README has milestone dates and an exceptions process; the verdict is ours |
| 11 | 17 | python/peps#207 reclassifying long-Deferred PEPs | TIGHTENED | one issue about PEP 212 only |
| 12 | 22 | Conventional Comments kinds and decorations | TIGHTENED | nine labels (praise, nitpick, suggestion, issue, todo, question, thought, chore, note); the line lists six; decorations confirmed |
| 13 | 23 | facts outrank opinion; equal approaches, author's wins | CONFIRMED | verbatim |
| 14 | 24 | SRE postmortem: action items with owner and date | TIGHTENED | Owner, Type, Bug, status; no date column |
| 15 | 29 | issue forms required fields, PR template, default branch | CONFIRMED | |
| 16 | 30 | Gerrit: one concern per change; child after parent | TIGHTENED | page defines relation chains and says cherry-pick submitters "must remember to submit changes in the right order"; neither rule stated |
| 17 | 31 | GitHub retargets when base merged and deleted | CONFIRMED | "not a stale diff" half is our inference |
| 18 | 34 / 59 | Graphite: rebase tax, merge order manual / same-file failure | TIGHTENED / REFUTED | "manually rebase every single child branch"; nothing on merge order; nothing on two changes touching one file (line 59) |
| 19 | 34 | arXiv 2607.20189 | TIGHTENED | exists, 2026-07-22, Belhouchette et al., Gerrit relation chains in OpenStack/Wikimedia/ONAP; 1.5x more revision rounds, bottom-up merges; not GitHub stacked PRs |
| 20 | 39 | OWNERS: approver never the author | REFUTED | reviewers exclude the PR author; approvers "including the individual who opened the PR" |
| 21 | 40 | review order design > functionality > ... > docs | CONFIRMED | |
| 22 | 41 | one business day; optimise for code-health speed | TIGHTENED | max one business day; optimises for team throughput, code health is an impact |
| 23 | 42 | settled by data/principle, style guide, never seniority | TIGHTENED | seniority not mentioned |
| 24 | 47 | no automatic retry; prove by count/stress/race | CONFIRMED | zero-flake policy verbatim |
| 25 | 48 | pipefail only with explicit shell: bash | CONFIRMED | |
| 26 | 49 | benchstat 10+ runs, p-value | CONFIRMED | "at least 10, ideally 20"; Mann-Whitney U |
| 27 | 49 | Chromium Pinpoint until significant | TIGHTENED | page has no Pinpoint; perf trybots and bisect bots |
| 28 | 49 | Rust matched baseline build | TIGHTENED | compares two commit hashes on the perf bot; "matched" and "same machine" not on the page |
| 29 | 54 | CODEOWNERS | CONFIRMED | |
| 30 | 64 | Apache lazy consensus window | CONFIRMED | usually 72 hours |
| 31 | 64 | vote only on the irreversible | TIGHTENED | votes for legal/process approvals (releases, committers) |

Unlinked: line 64 Go "about a week, no consensus defaults to decline" CONFIRMED on the Go page. Line 65 "every open question gets a named owner and a time box (RFC 3614)" is a stretch: 3614 owns goals, not questions.

Findings (all confirmed):
1. MAJOR community.md:39 - approver rule is backwards. Improve: "the reviewer is never the author (the author may approve if listed)".
2. MAJOR community.md:59 - Graphite says nothing about two changes in one file. Improve: cite the arXiv paper or drop the attribution.
3. MINOR :34 - name the arXiv paper correctly (Gerrit relation chains, 1.5x revision rounds, bottom-up merges).
4. MINOR :12, :70 - date and "never edit" are not Nygard's; credit MADR and PEP headers.
5. MINOR :24 - action items: owner, type, bug; no date.
6. MINOR :30 - Gerrit states neither rule; quote the cherry-pick order line.
7. MINOR :49 - Chromium: perf trybots and bisect bots, or change the URL.
8. MINOR :41 - "optimise for the speed at which the team ships together".
9. MINOR :15 - "RFCs stall (54 open over a year old in 2022), comment volume overwhelms; the fix was owned six-month goals".
10. MINOR :65 - 3614 owns goals; "apply the same to open questions" is ours.
11. MINOR :16 - mark "release paperwork" as our read.
12. NIT :9 - use the source text.
13. NIT :10 - three buckets.
14. NIT :17 - one PEP (212).
15. NIT :22 - nine labels.
16. NIT :42 - seniority is our rule.
17. NIT :49 - Rust compares two commit hashes.
18. NIT :64 - votes for legal/process approvals.

Did: fetched all 31 URLs, raw text for 11, graded every row, counted practices.
Did not: edit anything; use WebSearch; verify unlinked claims beyond the two noted.
Could do next: apply the 18 corrections in a worktree lane; fix the memory count 33 to 32.
