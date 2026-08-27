# R5: can a skill force agents onto one channel, and how to test it

Lane R5, 2026-08-27. SKILL.md mtime Aug 27 10:03. Owner's question: can the skill force lanes, reviewers and subagents to talk only over the message channel and never another way?

Short answer: no skill can force a single channel; prose only shapes it. Claude Code 2.1.247 already blocks one narrow pattern for subagents (built in, not a hook). A 17-line PreToolUse hook closes the rest of the Write/Edit path and can tell subagent from main via `agent_id`. Bash heredocs, MCP memory tools, tmux buffers/send-keys and git stay open; only per-agent-type tool allowlists close those, and lane-to-lane SendMessage is invisible to the orchestrator. Tested live in a throwaway dir: hook fired on 3/3 subagent writes, the subagent put the content in its reply.

## 1. Channel inventory

| channel | who reads | orchestrator notices | verdict |
|---|---|---|---|
| Agent tool final reply | parent only | yes | the wanted channel |
| SendMessage | named recipient; lane-to-lane goes past the lead | only as recipient | wanted lane-to-lead; lane-to-lane is an unlogged side channel |
| Write/Edit to repo files | every lane on the branch, reviewers | via git diff/PR | fine for owned source and research PRs; bad for report/handoff .md |
| Bash heredoc/redirect | same | no (Write/Edit matchers do not see Bash) | the bypass the memory records; hook (d) misses it |
| shared scratchpad | every teammate | never | the handoff file the rule forbids |
| gh issue/PR comments | everyone incl. owner | if it reads the issue | the record the skill wants |
| OMC MCP shared_memory_* (`.omc/state/shared-memory/{ns}/{key}.json`), notepad_*, state_*, project_memory_*, wiki_* | any session in the project dir | no; .omc is gitignored | bad as handoff; shared_memory exists precisely to share between agents |
| memory dir, session_search | every future and concurrent session | no | a handoff to future sessions |
| tmux send-keys | target pane's agent, as typed input | no | injection, never legitimate |
| tmux buffers (tmux-open-path:734,792; tmux-review:1199-1204, :1663) | any pane | no | human copy flow; readable by an agent |
| tmux options (agent-state:141 `@panestate_msg`, :209, :372; tmux-review:1659 `@cockpit_review_spool` + review-gate Stop hook injects the spool as additionalContext) | every pane | no | closed-set keywords except the spool; covered by the live-server prohibition |
| git shared stash, commits on a shared base | all worktrees | git log only | covered by "never the shared checkout" |

## 2. What the rule says now

SKILL.md:59 "Reports are text over the message channel. No report files." templates.md:63 "Report as text in your reply ... Do not write a report file." templates.md:93 "Send the reply as a message, not only as a final line." Allows research .md as a PR, PR body/comment for reports, issue comments for decisions. Leaves open: files as handoff (scratchpad, .omc/), gh comments as lane-to-lane chat, MCP shared memory/notepad, lane-to-lane SendMessage, tmux buffers. "Message channel" is never defined.

## 3. What blocked subagent report writes on 2026-08-26

A built-in check inside Claude Code's Write tool, not a hook, permission, agent type or sandbox. From the 2.1.247 binary (offset 198244868): `if(t.agentId&&/^(REPORT|SUMMARY|FINDINGS|ANALYSIS).*\.md$/i.test(basename))` logs `tengu_subagent_md_report_blocked` and returns "Subagents should return findings as text, not write report files. Include this content in your final response instead." Fires only under an agentId, only for Write, only for those basenames. Control run: REPORT.md and summary-of-work.md blocked, handoff.md written (6 bytes). ~/.claude/settings.json has no Write/Edit deny hook; no project .claude/ in the cockpit repo; OMC pre-tool-enforcer.mjs does not deny by path.

## 4. Enforcement ladder, priced

(a) prose as now: 0 cost; the 2026-08-26 memory shows 2 of 3 lanes idled with nothing delivered and one bypassed via heredoc.
(b) structural slot: templates 5 and 6 already carry it (line 63). Add one line naming the channels: "reply/SendMessage to <lead>, PR body, PR/issue comment. Not: files, scratchpad, .omc/, shared_memory, notepad, lane-to-lane messages." One edit, no runtime. Recommended first.
(c) read-only agent types: Explore and Plan lack Edit/Write; OMC analyst, architect, code-reviewer, critic, document-specialist, explore, scientist, security-reviewer, verifier lack Write/Edit. All keep Bash and MCP. Cannot cover implementation lanes.
(d) PreToolUse deny hook: 17 lines, jq only, tested. Covers Write and Edit by path, optional subagent-only via `agent_id`. Misses Bash heredocs and MCP writes. Scopes: ~/.claude/settings.json, .claude/settings.json (needs trust), .claude/settings.local.json, `--settings file`, plugin hooks.json. Deny shape: JSON `hookSpecificOutput.permissionDecision: "deny"` on exit 0; exit 2 also blocks. Not installed anywhere.

```bash
#!/bin/bash
# PreToolUse (Write|Edit): deny report/handoff files; SUBAGENT_ONLY=1 limits it to subagents.
in=$(cat)
path=$(printf '%s' "$in" | jq -r '.tool_input.file_path // empty')
agent=$(printf '%s' "$in" | jq -r '.agent_id // empty')
[ -n "$path" ] || exit 0
[ "${SUBAGENT_ONLY:-0}" = 1 ] && [ -z "$agent" ] && exit 0
shopt -s nocasematch
case "${path##*/}" in
  *report*.md|*handoff*.md|*findings*.md|*summary*.md) ;;
  *) case "$path" in */scratchpad/*.md) ;; *) exit 0 ;; esac ;;
esac
jq -n --arg r "Reports and handoffs go over the message channel (reply or SendMessage), not into ${path##*/}." \
  '{hookSpecificOutput:{hookEventName:"PreToolUse",permissionDecision:"deny",permissionDecisionReason:$r}}'
```
settings entry: `{"hooks":{"PreToolUse":[{"matcher":"Write|Edit","hooks":[{"type":"command","command":"<path>/deny-report-files.sh","timeout":5}]}]}}`

## 5. Test run

Safety: agent-state:13-15 and review-gate:37 both exit 0 unless `$TMUX_PANE` matches `%[0-9]*`, so `env -u TMUX -u TMUX_PANE` makes every cockpit hook a no-op before any tmux call. OMC hooks write only `.omc/` under the cwd.

Run 1 (hook via `--settings`, dir scratchpad/rig-78/R5-3539125/): `env -u TMUX -u TMUX_PANE claude -p --settings hook-settings.json --permission-mode acceptEdits --allowedTools Agent Write Edit Read --max-turns 8`, asking a general-purpose subagent to Write lane-report.md, handoff.md, notes.md then reply. hook.log: 3 lines, all `agent_id=a1a7... agent_type=general-purpose`; all three denied (notes.md via the scratchpad clause); no files created; the subagent quoted the deny reason and put the content in its reply; the parent relayed it. 3 turns, 28 s, $0.90.
Run 2 (control, no hook): REPORT.md and summary-of-work.md blocked by the built-in; handoff.md created. 2 turns, 26 s, $0.83.
Cleanup: no leftover processes; removed both .omc/ dirs and builtin/handoff.md; kept deny-report-files.sh, hook-settings.json, hook.log, run.jsonl, run2.jsonl as evidence. Live tmux server untouched.

## Findings

1. MAJOR confirmed SKILL.md:59 - "message channel" undefined; lane-to-lane SendMessage, shared_memory_*, notepad and scratchpad files all pass a literal reading. Improve: name allowed channels (reply/SendMessage to the lead, PR body, issue/PR comment) and forbidden ones in one line.
2. MAJOR confirmed named-subagent-reports-via-files.md - the block is a built-in regex on REPORT/SUMMARY/FINDINGS/ANALYSIS*.md for subagents, not "the harness refuses report files". Improve: memory names the pattern so briefs stop relying on it.
3. MINOR confirmed templates.md:63 - the slot does not name the recipient. Improve: "reply to <lead>; nothing to other lanes except via the lead".
4. MINOR plausible hook (d) - misses Bash heredocs and MCP writes; pair with `--disallowedTools` for MCP write tools on research lanes.
5. NIT confirmed tmux-review:1659-1663 - the review spool is an agent-to-agent text channel if an agent runs `tmux-review --deliver`; covered by the live-server prohibition. Keep.
6. NIT confirmed - the shared scratchpad is the cheapest illegal handoff; the `*/scratchpad/*.md` clause catches it. Keep if (d) ships.

Did: channel inventory from scripts and the OMC dist; found the built-in block in the binary; hook docs (agent_id/agent_type, deny JSON, scopes); wrote and ran the hook in two safe `claude -p` runs; cleanup.
Did not: install anything; touch ~/.claude, the repo, worktrees, or the live server; test the heredoc case under the hook.
Could do next: rung (b) one-liners in SKILL.md:59 and templates.md:63; if the owner wants (d), ship the script under the repo's examples with a settings.local.json snippet and a fail-first test feeding the three payloads.
