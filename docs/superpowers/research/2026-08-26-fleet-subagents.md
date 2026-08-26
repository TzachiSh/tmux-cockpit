# Fleet queue: show an agent's live subagents under its row

Research 2026-08-26, Claude Code 2.1.246. Two Opus agents (codebase, community)
plus hands-on reads of `~/.claude/projects/<proj>/<session>/subagents/`.

## What cockpit does today

- The fleet popup is `scripts/tmux-review` (`fleet_rows` :1468, `fleet_queue`
  :1896). One `tmux list-panes -a -F` builds the rows, one row per agent pane,
  five tab fields: pane_id, state, title, repo⑂branch, ±stat. fzf shows fields
  2-5, widths capped 12/40/32. No refresh loop, one-shot per keypress.
  `--fleet` prints the same rows and is a documented interface (README :538,
  tests/run.sh :5620 pins "exactly two rows").
- `SubagentStart -> agent-state running`. Nothing reads the payload. The
  subagent is invisible. `SubagentStop` is deliberately unwired (README :907):
  `-> running` cleared a failed subagent's `error`.
- Nothing in the repo reads `agent_id` / `agent_type`. `review-gate` :75-112
  already does a bounded stdin read and pulls fields without jq; that is the
  pattern for any hook payload parsing.
- Hooks fire inside subagents for their own tool calls (same `session_id`,
  `agent_id` present), so a subagent's PostToolUse already drives the lead
  pane to `running`. Since v2.1.198 subagents run in the background by
  default, so the lead's `Stop -> done` fires with children still alive.

## What Claude Code exposes

Verified against the 2.1.246 binary and https://code.claude.com/docs/en/hooks.

| source | fields | cost | ceiling |
|---|---|---|---|
| `SubagentStart` stdin | `agent_id`, `agent_type` (both required) | one bounded read per spawn | no description, no name field |
| `SubagentStop` stdin | + `agent_transcript_path`, `last_assistant_message`, `background_tasks` | one read per exit | may arrive without a Start; may never arrive on kill |
| `PreToolUse` Agent tool | `tool_input.description`, `subagent_type`, `model` | on the hot path (every tool call) | rejected: the hot path reads no stdin today |
| `subagentStatusLine` (settings key) | `tasks[]`: `id name type status description startTime model tokenCount cwd`, plus `columns` | one spawn per statusline tick while subagent rows are visible | idle rows hide after 30 s; >3 idle collapse into one row |
| `<session>/subagents/agent-<id>.meta.json` | `agentType description name model parentAgentId spawnDepth taskKind color` | one file read | undocumented layout |
| `Stop.background_tasks[]` | `{id type status description agent_type}` | free, already on Stop | snapshot at turn end, backgrounded work only |

`agent_id` encodes the name: `a<name>-<16 hex>` for named agents,
`a<16 hex>` otherwise (observed on disk). The doc's `agent-abc123` is a
placeholder.

`subagentStatusLine` contract (https://code.claude.com/docs/en/statusline):
stdin is one JSON object per tick with the base hook fields and `tasks`;
stdout is one `{"id","content"}` line per row, an empty stdout keeps the
default rendering. Same shape as cockpit's `statusLine` shim
`scripts/agent-context`, and a separate key, so it coexists with any HUD.

## Prior art

- oh-my-claudecode `subagent-tracker.mjs`: SubagentStart/Stop hooks, JSON
  ledger per session under a file lock, explicit `unmatched_stop` state.
- disler/claude-code-hooks-multi-agent-observability, simple10/agents-observe,
  hoangsonww/Claude-Code-Agent-Monitor: hook event stream to a local server
  and a web dashboard; parentage rebuilt from the stream.
- jimmyliao/agent-status-tmux, samleeney/tmux-agent-status: session-level
  busy/idle in the tmux status bar, no subagent breakdown.
- sirmalloc/ccstatusline: no subagent widget, does not use subagentStatusLine.
- anthropics/claude-code#34468 "Show Agent tool subagents in tmux split
  panes": closed, not planned. #87411 (open): asks for the prompt on
  SubagentStart. #22348 (closed): agent_id on PostToolUse, now shipped.

Nobody found uses `subagentStatusLine` or the `.meta.json` sidecars. No tmux
plugin shows per-agent subagent rows.

## Plan (the lazy one)

One shim, one pane option, one extra format field. No hook changes.

1. `scripts/agent-subs`, a `subagentStatusLine` command. Copy the
   `agent-context` skeleton: `%N` guard on `TMUX_PANE`, bounded stdin read,
   jq, fail-open. Emit `.tasks[] | "\(.status) \(.name // .type) \(.description)"`
   joined by `\036` (record separator), tabs and newlines stripped,
   description cut at 40, at most 8 rows. Compare-then-set `@cockpit_subs` on
   the pane (a set with the same value still forces a server redraw). Print
   nothing.
2. `tmux-review` `fleet_rows` :1487: add `#{?#{@cockpit_subs},#{@cockpit_subs},-}`
   as a sixth internal field (sentinel per the tab-collapse trap :1479).
   `fleet_queue` :1926: after each parent row emit one child row per entry,
   field 1 = the parent's pane id (so ↵ reviews the parent and ^X kills its
   window unchanged), state column = the task status, title column =
   `└ name · description`, loc column `-`. `--fleet` output untouched.
3. `examples/claude-hooks.json`: document the `subagentStatusLine` key next
   to the `statusLine` note. `install.sh --hooks` does not merge it (same as
   statusLine today).
4. Tests: one block feeding a captured `tests/fixtures/subagent-statusline.json`
   to `agent-subs` and asserting the option; one block setting `@cockpit_subs`
   by hand on a fake agent pane and asserting the popup shows the child row
   and `--fleet` still prints exactly the parent rows.

Costs. Hook path: zero. Redraw path: zero (no format reads `@cockpit_subs`).
Statusline tick: one bash+jq spawn while subagents are visible, one tmux
write when the list changed. Popup: one wider format string.

Ceiling, by design: rows vanish 30 s after a subagent idles and >3 idle
collapse. Fine for "what is running now".

## Skipped, and when to add

- Hook ledger (`SubagentStart`/`Stop` -> option keyed on `agent_id`): only if
  the statusline feed proves unreliable. It needs its own leak handling
  because `SubagentStop` can be missing and background children outlive
  `Stop`.
- Tree deeper than one level (`parentAgentId` from `.meta.json`): when
  someone runs teammates that spawn subagents and wants the nesting.
- `⑂N` in `#{agent_fleet}` status-right: a redraw-path change, needs a
  precomputed count and a freeze-rig run.
- Fixing green `done` while background subagents run: separate issue,
  `Stop.background_tasks` is the source.

## Unknowns to verify first

1. Which version introduced `subagentStatusLine`, and does it tick when the
   subagent panel is collapsed or the terminal is narrow.
2. Is `TMUX_PANE` in the shim's environment (it is for `statusLine`).
3. Tick rate while rows are visible, to size the compare-then-set win.
4. Does a `subagentStatusLine` that prints nothing keep the default panel
   rendering (the doc says yes).
