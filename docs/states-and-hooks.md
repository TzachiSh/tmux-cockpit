# States and hooks

## How it works

Claude Code hooks call `scripts/agent-state`, which writes a per-pane tmux
option:

```sh
tmux set-option -p -t "$TMUX_PANE" @panestate running
```

Everything you see is then plain tmux format syntax in `~/.tmux.conf` reading
that option. **A status redraw runs no shell at all.** The option dies with its
pane, so there is no stale state and nothing to garbage collect.

When a pane goes `needs-input`, a second option, `@panestate_msg`, carries the
first 120 characters of the ask itself — Claude's `Notification` hook payload,
not a guess. It clears whenever the pane resumes work or goes idle, and is
left alone by `done` since a done pane has no border alert to read it on.

### State ranking

`needs-input` > `error` > `running` > `done`

A lower state may not overwrite a higher one. Only work actually resuming
(`running`) or an explicit `off` clears them. This exists because `Stop` fires
when a *blocked* turn ends — with the permission prompt still on screen — and
letting that write `done` turned a red "answer me" into a green "finished" on a
timer.

## Hooks

| hook | state |
|------|-------|
| `SessionStart` | `off` — a resumed pane must not inherit old state |
| `SessionStart` (2nd) | injects `skills/unslop/SKILL.md` as context (through `jq`): the writing rules every agent on the box follows, with no per-machine paste |
| `SessionStart` (3rd) | `session` — saves the payload's `session_id` as `@cockpit_session` and `transcript_path` as `@cockpit_transcript` (see [Session identity](#session-identity)) |
| `UserPromptSubmit` | `running` |
| `UserPromptSubmit` (2nd) | `session` — the same save, so `/clear` and `--resume` (a new `session_id`) are followed from the first prompt |
| `PermissionRequest` | `needs-input` |
| `PostToolUse` | `running` |
| `PostToolUseFailure` | `error` |
| `SubagentStart` | `running` |
| `TeammateIdle` | `wake` — a teammate idling wakes its lead with no hook of its own; `done` → `running`, sticky states and a pending `◆` kept |
| `PreToolUse` | `running` — marks the pane busy as a tool begins |
| `Stop` | `done` |
| `Stop` (2nd) | `review-gate` — flags `◆` when the finished diff is unreviewed; hands a queued review to the agent at this turn boundary (`additionalContext` on stdout, once); caps a `/goal` rewake chain at `@cockpit_rewake_cap` and honours `review-gate --concede` (see [A goal that will not be met](#a-goal-that-will-not-be-met)) |
| `StopFailure` | `error` — a turn killed by an API error fires this, **not** `Stop`; unwired, it left a bright "working" marker forever |
| `Notification` | `notify` (parses the JSON on stdin) |
| `WorktreeCreate` | `worktree-create` — `claude --worktree <name>` and `EnterWorktree` put the tree under `~/.cache/cockpit-wt/<name>` on branch `<name>` (branched from the launch checkout's HEAD; an existing branch is checked out), so the session's cwd is the tree it works in and every cockpit read describes that tree. One namespace for every repo on the machine; subagents with `isolation: worktree` land there too, with their slug branches. Not with `--worktree --tmux` together, which skips the hook |
| `WorktreeRemove` | `worktree-remove` — when claude means to remove a tree the hook above made (remove on `ExitWorktree`, verified on 2.1.246; a subagent finishing clean, per the docs), a clean tree on a merged branch goes, tree and branch; a dirty tree or unmerged commits stay, which is claude's own rule for the trees it makes itself. `claude -p` never cleans up worktrees, so a subagent run under `-p` leaves its tree and slug branch behind; `git worktree remove` plus `git branch -d` |
| `SessionEnd` | `off` |

A `Stop` hook does not fire on a user interrupt (Esc), so a review queued for an
interrupted agent waits for the end of that agent's next turn — nothing hands it
over earlier, `prefix r` included.

## Context fill, optional

`scripts/agent-context` is a Claude Code `statusLine` shim: fed the statusline
JSON on stdin, it records the pane's context-window fill as a percent in
`@panestate_ctx` and prints a minimal `<model> · ctx NN%` line. It never blocks
or errors at the agent — an unparseable payload, a missing `jq`, or a writer
that stalls is a silent no-op within a bounded read, the same contract
`agent-state` holds for hooks.

A `statusLine` fully replaces any other one you have configured, so it is not
wired by default — see the commented `_statusline_comment` stanza in
`examples/claude-hooks.json` for the setting to add. Once wired,
both example configs show a `◔NN%` warning on a window's tab once its active
pane crosses 80% fill, ahead of the compaction that would otherwise land
unannounced.

## Which commands count as an agent

One option, read by every format:

```tmux
set -g @cockpit_agents "^(claude|codex|aider|cursor|opencode|gemini|crush)$"
```

`cockpit.tmux` sets that default if you have not, so there is nothing to do
unless you run a CLI it does not know — then set your own **before** the
`run-shell` line and everything follows.

Under the hood the formats match a derived **glob list**
(`@cockpit_agents_glob`, `|claude|codex|…|`), not the regex: tmux recompiles a
`#{m/r:}` regex on every evaluation — measured at 32–36 µs per test, 25 tests
per config — and switching to glob matching cut the whole status redraw by
2–3×, with byte-identical rendering. A simple `^(a|b|c)$` value converts
automatically; anything fancier gets a message asking you to set
`@cockpit_agents_glob` yourself rather than being silently ignored. The one
semantic difference: a `pane_current_command` containing a literal `*` would
false-match under glob — no real process name does.

Do not set either option to the empty string. tmux treats an empty pattern as
matching every command, which would paint every pane in the session as an
agent.

## Stale state

Hooks cannot fire if an agent is killed or crashes, so `@panestate` can outlive
the agent it describes. The formats therefore only believe it while the pane is
actually running one:

```tmux
#{m/r:^(claude|codex|aider|cursor|opencode|gemini|crush)$,#{pane_current_command}}
```

Other projects solve this by polling the process tree from a background daemon.
tmux already knows the foreground command, so this costs nothing.

The review state is per-pane for the same reason. `@cockpit_review`
(`pending` or `sent`), `@cockpit_review_spool`, `@cockpit_title` and
`@cockpit_session` are all pane options, so a killed pane takes them with it
and no `◆` can outlive the agent it counted. An agent that merely exits leaves
its pane behind, so the `SessionEnd` hook clears the two review ones itself:
a queued review nobody can drain, or a pending mark on a pane with no agent in
it, would otherwise keep counting toward `◆N` forever.

## Deliberately unwired

`SubagentStop`, `TaskCompleted`, `PreCompact`.

`SubagentStop -> running` looked harmless but cleared a failed subagent's `error`,
so a run that failed finished **green**. The next `PostToolUse` restores `running`
anyway, so the hook only ever destroyed information. `TaskCompleted` fires
mid-turn per task-list item and flashed green while the agent kept working;
`Stop` already covers every turn end.

`TeammateIdle` IS wired, to `agent-state wake`. A teammate going idle posts an
idle notification to its lead, and that message wakes an idle lead with no
hook of its own, so the lead's pane sat on green `done` (and `◆ done` in the
fleet) until its first tool call, 6 s on a live team. `wake` flips `done` (or
no state) to `running` and leaves a blocked or errored lead exactly as it is.
Measured with in-process teammates (the default); in split-pane team mode the
hook lands on the teammate's own pane, so an idle teammate may read `running`
until its next turn.

## A goal that will not be met

Claude Code's `/goal` is a Stop hook: after every turn an evaluator reads the
transcript and, unless it judges the condition met, sends the agent back in.
There is no cap, and the agent has no way to give up; the only exits are the
evaluator's own "impossible" verdict, which it is told to grant only after
confirming that itself, and your `/goal clear`. An agent that hit a permission
it will not route around ends its turn, is told the goal is unmet, ends it
again, and the pane sits on `running` until you notice (issue #52).

`review-gate` takes two things off that loop, per turn. Each Stop the goal hook
causes carries `stop_hook_active:true`; the hook counts those per pane and at
`@cockpit_rewake_cap` (`set -g @cockpit_rewake_cap 10` is the default, `0`
disables) ends the turn with `continue:false`, which Claude Code honours ahead
of the goal hook's block (2.1.246: the Stop handler returns on a hook's
`preventContinuation` before it looks at any blocking error). And the agent
can concede:

    review-gate --concede "push denied by the permission classifier; needs you"

ends its next turn with that line as the stop reason, whatever the count. Both
the count and the concession are keyed on the pane *and* its tmux server, so a
private `tmux -L` socket cannot fire either on the live server's same-numbered
pane; files under the older pane-only names are ignored, and the
`rm -rf ~/.local/state/tmux-cockpit/review` purge above clears them. Re-run
`install.sh` after upgrading: the `cockpit-concede` skill is a symlink, and a box
installed before it has none. That skill tells the agent when to concede. The
goal itself stays set either way, since only Claude Code can clear it: your
next prompt resumes it, `/goal clear` ends it.

## Known limits

- A session started **before** a hook was added never fires it — Claude Code
  snapshots hooks per session id. Such a pane stays grey until it restarts.
- A long "thinking" phase fires no hook, so a working pane can read as idle.
  CPU is not a usable substitute: an agent waiting on the API is
  indistinguishable from an idle one.

## Session identity

`agent-state session` runs at `SessionStart` (`startup|resume|clear`, not `compact`, which
keeps its `session_id`) and at `UserPromptSubmit`. It changes no state. It reads two fields
of the hook payload and writes them with `set-option -p`, which does not expand formats:

| option | from | saved when |
|--------|------|------------|
| `@cockpit_session` | `session_id` | it is a UUID |
| `@cockpit_transcript` | `transcript_path` | it is an absolute path with no `..` and no `#`, and resolves (`realpath`, symlinks followed) to a regular file under `$CLAUDE_CONFIG_DIR/projects`, else `~/.claude/projects` |

A new `session_id` drops the old `@cockpit_transcript` first, so a pane never points at the
previous session's file. A missing or refused field leaves the option as it was. The reader
(`cockpit-tui transcript file`) takes `@cockpit_transcript` if it still resolves under the
projects tree, then `@cockpit_session`, then `claude agents --json`. `last_assistant_message`
is never stored: free text in an option is the #289 hazard. `Stop` still refreshes
`@cockpit_session` through `review-gate`.
