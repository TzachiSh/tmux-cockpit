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

## How it shows in the popup

The queue is fzf over padded rows (`fleet_queue` :1896). Four visible columns,
state (cap 12) · title (cap 40) · repo⑂branch (cap 32) · ±, a two-line header,
list height capped at 8 rows, the diff preview below. A child row reuses the
same four columns, nothing new to lay out:

```
fleet>
which agent needs you   [◆ review pending · ↵ pasted, unsent]  state · title · repo⑂branch · ±
↵ review   ^/ preview   ^X kill window   esc cancel
> ◆ done       cockpit:1 tmux-cockpit                cockpit⑂master          +12 -3
  running      cockpit:2 fix/teammate-idle           cockpit⑂fix/teammate    +80 -9
    running    └ code-research · Codebase: how fleet shows agents
    running    └ community-research · Community: subagent visib…
  needs-input  api:0 claude                          api⑂main                +0 -0
```

- Indent lives in the state column (`  running`, 9 chars, under the 12 cap).
  Title is `└ name · description`, name falls back to `type`
  (`general-purpose`), description cut so the whole title fits the 40 cap.
  repo⑂branch and ± are blank on a child: they belong to the pane, and
  repeating them under every child is noise.
- Field 1 (hidden) is the parent's pane id on every child row, so ↵ reviews
  the parent, `^/` previews the parent's diff, and `^X` prompts to kill the
  parent's window with the same y/N naming the window. No new key.
- Children are inserted in `fleet_queue` after the rank/churn sort, directly
  into the padded `disp` array, not into `rows`: the width loop keeps
  measuring parents only, and an empty repo⑂branch never hits the
  tab-collapse trap because the child never passes through `read`.
- The preselect scan (`pos`) breaks on the first row carrying the pane id,
  which is the parent, so `prefix r` still lands on the parent row.
- Sort order is untouched: children follow their parent wherever rank put
  it. A blocked pane still comes first.
- Completed subagents show `completed` for the 30 s Claude Code keeps the
  row, then drop, same as the built-in panel. Beyond 8 subagents the shim
  keeps the first 8 and the last row reads `└ +N more`.
- The pane reads `@cockpit_subs` in `fleet_queue` with one
  `list-panes -a -F '#{pane_id}<TAB>#{?#{@cockpit_subs},#{@cockpit_subs},-}'`
  rather than a sixth `fleet_rows` field, the same seam the preview uses so
  `--fleet` stays a five-field interface.

Not a tree widget. One level, plain indent, a fixed `└`. fzf renders lines,
so a deeper tree would be more indent, not a different mechanism.

## Risks

| risk | why it matters | mitigation |
|---|---|---|
| The queue lists only dirty or blocked panes (`fleet_rows` :1567). A clean `running` lead with live subagents is not in the list at all. | The feature is invisible in the most common "agent is busy" case. | Decision for the owner. Keep the rule (the queue is "who needs you", a busy clean agent does not) or add `[ -n "$subs" ]` to the keep test, one line, rank 3. Recommend the one-liner: seeing what a lead is doing is the point of the ask. |
| fzf filters lines independently. Typing a query can hide a parent and leave its children as orphans. | Confusing rows with no context. | Accept. Orphans keep the `└`, ↵ still opens the right pane. Add the parent's title to the child's hidden field 6 and `--nth` it only if it bites. |
| Children eat the 8-row list height. Two leads with four subagents each shows two parents and a scrollbar. | Parents below the fold. | Count parents only toward `lrows`, cap the list at 12 when children exist. Or leave it, `--cycle` scrolls. |
| Stale `@cockpit_subs` after the agent exits without a final empty tick. | Ghost rows under a pane whose agent is gone. | `SessionEnd -> agent-state off` unsets the option inside its existing batch (fixed option name, closed set). Verify in the rig that the final tick sends `tasks: []`. |
| Each shim tick is a bash+jq spawn, each changed value one `set-option`, and every `set-option` forces a server-wide redraw. | A redraw storm if the value churns per tick. | The value excludes `tokenCount` and `startTime`, so it changes only on start, stop and status flips. Compare-then-set, the `agent-context` :46 guard. Measure spawns and writes per minute in the rig. |
| `subagentStatusLine` is a top-level settings key, not under `hooks`. `install.sh --hooks` does not merge it. | Opt-in, most users never see the feature. | Have `--hooks` add the key when absent (unlike `statusLine` it does not replace a HUD; the OMC HUD leaves it null). Document. |
| Teammates in split panes are also tasks in the lead's panel, so they appear as `└` rows under the lead and as their own pane rows. | Double listing. | Accept for v1. Filter by `type` once the rig shows what the field carries for a teammate. |
| Untrusted workspace or `disableAllHooks` silently skips the shim (binary strings: "workspace trust not accepted"). | No rows, no error. | Fail-open by design. Note in README. |
| Minimum Claude Code version for `subagentStatusLine` unknown; the doc marks `model` v2.1.205+, `effort` v2.1.214+. | An older CC ignores the key or logs a schema error. | Rig it on the installed version, state the floor in the README. |
| `status` values undocumented (`running`, `completed`, `failed`, `pending`?). | Wrong column text. | Take them verbatim from the fixture the rig captures. |

## Test plan

Three layers, in the repo's existing shapes.

1. Live rig, once, by hand (the `tests/fixtures/statusline.json` capture
   pattern). A real `claude` in a tmux pane with the shim wired, spawning two
   background subagents and one teammate. The shim `tee`s its stdin to a
   file. Outputs: the fixture `tests/fixtures/subagent-statusline.json`, the
   observed `status` values, whether `TMUX_PANE` is set, ticks per minute
   with rows visible, whether a final `tasks: []` tick arrives after the 30 s
   hide, and the `type` value of a teammate task. Every unknown above is
   answered by this one session.
2. `tests/run.sh` blocks, each on a private `tmux -L` server, the pattern at
   :420 and :5620.
   - shim: fixture in, `@cockpit_subs` out; garbage in, no write; tabs and
     newlines in a description stripped; 20 tasks capped at 8 with `+12
     more`; unchanged input costs exactly 1 tmux invocation and 0 writes
     (the PATH shim counter from :426); no `TMUX_PANE`, exit 0 and no tmux
     call.
   - popup: a fake agent pane in a dirty repo with `@cockpit_subs` set by
     hand; drive the popup with `send-keys`, `capture-pane -p -J`, assert the
     `└` row directly under the parent, the parent still first; ↵ on the
     child row opens the parent's review (the :1693 path); ^X on the child
     row prompts with the parent's window name; `--fleet` still prints
     exactly the parent rows (`grep -c '^%'` unchanged at :5688); a 40-char
     title column never widens for a child.
   - `SessionEnd -> off` unsets `@cockpit_subs`.
3. Freeze rig, `tests/freeze-rig/redraw_cost.sh`, one run to confirm zero
   pane visits added per redraw (no format reads the option) and to count
   writes per minute under 8 churning subagents. Not a gate, a number for
   the commit message.

`make test-changed` picks the two new blocks; `make check-par` before the
merge.

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
