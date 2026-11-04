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
| `SessionStart` (2nd) | `session` — saves the payload's `session_id` as `@cockpit_session` and `transcript_path` as `@cockpit_transcript` (see [Session identity](#session-identity)) |
| `SessionStart` (3rd) | injects `skills/unslop/SKILL.md` as context (through `jq`): the writing rules every agent on the box follows, with no per-machine paste |
| `UserPromptSubmit` | `running` |
| `UserPromptSubmit` (2nd) | `session` — the same save, so `/clear` and `--resume` (a new `session_id`) are followed from the first prompt |
| `CwdChanged` | `session` — saves the payload's `new_cwd` as `@cockpit_dir`, so the board shows the repo the agent `cd`'d into (see [Working tree](#working-tree)) |
| `PermissionRequest` | `needs-input` |
| `PostToolUse` | `running` |
| `PostToolUseFailure` | `error` |
| `SubagentStart` | `running` |
| `SubagentStart` (2nd) | `sub-start` — appends the event to the session's subagent log (see [Subagent event log](#subagent-event-log)); no pane state |
| `SubagentStop` | `sub-stop` — the same log; no pane state, so a failed subagent's `error` is not cleared (see [Deliberately unwired](#deliberately-unwired)) |
| `TeammateIdle` | `wake` — a teammate idling wakes its lead with no hook of its own; `done` → `running`, sticky states and a pending `◆` kept |
| `TeammateIdle` (2nd) | `sub-idle` — the subagent log |
| `PreToolUse` | `running` — marks the pane busy as a tool begins |
| `Stop` | `done`, unless the payload's `background_tasks` lists a subagent: then no state changes (see [Background subagents](#background-subagents)). Either way `@cockpit_subs` takes the count (see [Subagent count](#subagent-count)) |
| `Stop` (2nd) | `review-gate` — flags `◆` when the finished diff is unreviewed; hands a queued review to the agent at this turn boundary (`additionalContext` on stdout, once); caps a `/goal` rewake chain at `@cockpit_rewake_cap` and honours `review-gate --concede` (see [A goal that will not be met](#a-goal-that-will-not-be-met)) |
| `StopFailure` | `error` — a turn killed by an API error fires this, **not** `Stop`; unwired, it left a bright "working" marker forever |
| `Notification` | `notify` (parses the JSON on stdin) |
| `WorktreeCreate` | `worktree-create` — `claude --worktree <name>` and `EnterWorktree` put the tree under `~/.cache/cockpit-wt/<name>` on branch `<name>` (branched from the launch checkout's HEAD; an existing branch is checked out), so the session's cwd is the tree it works in and every cockpit read describes that tree. One namespace for every repo on the machine; subagents with `isolation: worktree` land there too, with their slug branches. Not with `--worktree --tmux` together, which skips the hook |
| `WorktreeRemove` | `worktree-remove` — when claude means to remove a tree the hook above made (remove on `ExitWorktree`, verified on 2.1.246; a subagent finishing clean, per the docs), a clean tree on a merged branch goes, tree and branch; a dirty tree or unmerged commits stay, which is claude's own rule for the trees it makes itself. `claude -p` never cleans up worktrees, so a subagent run under `-p` leaves its tree and slug branch behind; `git worktree remove` plus `git branch -d` |
| `SessionEnd` | `off` |
| `SessionEnd` (2nd) | `sub-end` — drops the session's subagent log, and sweeps logs unwritten for 7 days |

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

`TaskCompleted`, `PreCompact`, and `SubagentStop` as a *state* hook.

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

## Background subagents

A main turn can end while background subagents still work ("Waiting for 3
background agents to finish"). That `Stop` is a pause: Claude Code wakes the
session when each subagent reports back, which fires `UserPromptSubmit`, and
the `Stop` that ends the last report-back turn writes `done`. Claude Code
puts the in-flight work in the `Stop` payload's `background_tasks`, so
`agent-state done` reads it and leaves the pane as it is while any entry has
type `subagent`. `needs-input` and `error` stay sticky, since nothing is written.

Only subagents hold the pane. An idle teammate stays `running` in Claude Code's
task registry for the life of the team, and a background shell or monitor can
run forever; holding on either would leave a pane "working" that nothing will
wake. A `workflow`, `cloud session` or `MCP task` does not hold it either: the
hooks reference names only a background subagent's report-back as a wake of
the main session, and a cloud session runs on another machine for as long as
it likes. Holding on work with no sure wake risks a pane stuck "working"; not
holding only shows `done` early. A subagent that never stops is listed by
every `Stop` until the session ends, and `SessionEnd` clears the pane.

There is no running subagent counter, so there is nothing to drift, lock or
reset. Counting `SubagentStart` against `SubagentStop` would drift:
`SubagentStart` also fires on a resumed subagent and on every message an
in-process teammate handles, and `SubagentStop` also fires for Claude Code's
internal agents (prompt suggestions, `/btw`), which never fired a start. The
count below is read off each `Stop`'s list whole, so a wrong one lasts one turn.

## Subagent count

At every `Stop`, `agent-state done` writes `@cockpit_subs`, the number of
`background_tasks` entries of type `subagent`, with the same structural match
as the hold above. At 0 the option is unset. It is written whether or not the
pane's state changes, so a sticky `needs-input` pane still takes it.

- **Cleared** by `off` at `SessionStart` (`startup|resume|clear`; a new process
  runs no subagent) and at `SessionEnd`.
- **Not cleared** at `UserPromptSubmit`. A background subagent reporting back
  fires one while the others still run, and so does a prompt you type while
  they run. It is not cleared at the idle notification either, because the lead
  idles while its subagents work.
- `compact` keeps it: compaction does not stop a subagent, and the next `Stop`
  rewrites the count.

The bar's fleet summary, `#{E:@cockpit_fleet}`, ends with `✦N`, the sum over
every agent pane, in the running colour. It is empty at 0 and hidden in touch
mode. tmux has no sum over panes, so the format repeats a mark N times per pane
(`#{R:x,N}`) and counts the marks (`#{n:}`). Touch mode belongs to a client, so
its test is stored escaped and runs when the bar draws. The count is refreshed
beside every write of `@cockpit_subs`, since a `Stop` that holds the pane or
meets a sticky state runs no sums of its own. The sweep drops a killed pane's
count. The mark is `✦` in every glyph tier, like `▲ ✕ ◆`.

The board's subagent tree reads Claude Code's own files, not the hooks:
`<session>/subagents/agent-<id>.meta.json` and `agent-<id>.jsonl`, beside
`@cockpit_transcript` (`<sid>.jsonl` -> `<sid>/subagents/`, used when it still
resolves to a file in the projects tree), else beside the transcript the board
reads for `@cockpit_session`. `data::subagents` caches each file on (mtime,
length), and a pane with no such dir costs one failed `opendir`.

| field | from |
|-------|------|
| `ty`, `desc` | `agentType`, `description` (control characters dropped) |
| `kind` | `isFork: true` is a fork; a `teamName` is a teammate; else a subagent |
| `state` | `stoppedByUser: true`, or `finished: true` with an API error as the newest assistant row, is failed; `finished: true` is done; else running |
| `started` | the first transcript row's `timestamp` |
| `tokens` | the newest usage row's input, output and cache tokens summed, the figure Claude Code reports as `totalTokens`, not a sum over turns |

Claude Code 2.1.317 sets `finished` when a task leaves `running`, for success and
failure alike, so a failed run that left no API error row reads as done.

## Subagent event log

`SubagentStart`, `SubagentStop`, `TeammateIdle` and `SessionEnd` also run
`agent-state sub-start|sub-stop|sub-idle|sub-end` (#361). They set no pane state and call no
tmux: each appends one line to
`${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/subs/<session_id>.log`, `<epoch>\t<event>\t<agent_id>`
with the event `start`, `stop` or `idle`. `sub-end` deletes the session's log and any log
unwritten for 7 days.

- **Append-only.** One write under `PIPE_BUF` is atomic, so hooks firing side by side lose
  nothing, and a reader takes the newest line per `agent_id`. It is not a counter: `SubagentStart`
  also fires for a resumed subagent and for every message an in-process teammate handles, and
  `SubagentStop` for every turn a teammate ends (#317).
- **What an event says.** The newest event is what the agent did last. A teammate's `stop` ends
  a turn, not the agent, and no documented hook fires when TaskStop or a shutdown ends one, so
  the log cannot tell an idle teammate from an ended one.
- **Safe to run anywhere.** Only a plain id (`[A-Za-z0-9_-]`, 1 to 64 bytes) is written; a
  malformed or missing id drops the event. The dir is 0700 and the log 0600 whatever the umask, a
  symlink in the way is never followed, the payload is read for 2 s at most, and the hook always
  exits 0.
- **Install.** The four entries come with `install.sh --hooks`; an update needs it rerun, and
  `install.sh --check` warns (`subhooks`) when they are missing. The board does not read the log
  yet; it still reads `meta.json` and the transcript as above.

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

`agent-state session` runs at `SessionStart` (`startup|resume|clear|fork`: each can start a
new `session_id`; not `compact`, which keeps it), at `UserPromptSubmit` and at `CwdChanged`. It changes no
state. It reads these fields of the hook payload and writes them with `set-option -p`, which does
not expand formats:

| option | from | saved when |
|--------|------|------------|
| `@cockpit_session` | `session_id` | it is a UUID |
| `@cockpit_transcript` | `transcript_path` | it is an absolute path with no `..` and no `#`, and resolves (`realpath`, symlinks followed) to a regular file under `$CLAUDE_CONFIG_DIR/projects`, else `~/.claude/projects` |
| `@cockpit_dir` | `new_cwd`, else `cwd` | it is a whole JSON string naming an existing directory by an absolute path with no control character or backslash, and the agent moved or a new session began (see [Working tree](#working-tree)) |

A new `session_id` drops the old `@cockpit_transcript` first, so a pane never points at the
previous session's file. A missing or refused field leaves the option as it was. The reader
(`data::transcript::file`) takes `@cockpit_transcript` if it still resolves under the
projects tree, then `@cockpit_session`, then `claude agents --json`. `last_assistant_message`
is never stored: free text in an option is the #289 hazard. `Stop` still refreshes
`@cockpit_session` through `review-gate`.

The read is capped at 4 KB: `UserPromptSubmit` carries the whole prompt and bash reads a pipe
a byte at a time. Claude sends the ids first; one past the cap, or cut by it, is skipped and the
old value stays. The hook checks the path against the projects tree of the `CLAUDE_CONFIG_DIR`
claude runs with; the reader uses its own environment's (usually the tmux server's). Export
`CLAUDE_CONFIG_DIR` where tmux starts, not only in a shell, or the reader refuses the stored path
and falls back to `@cockpit_session`.

## Working tree

tmux reports a pane's process cwd, and a Claude Code session never moves it: a `claude`
started in `~` that `cd`s into a repo read `no repo` on the board. Every hook payload carries
the session's own `cwd`, so `agent-state session` saves it as `@cockpit_dir`, the option the
board, `prefix + i` and `tmux-review` already read ahead of the pane's cwd. It runs at
`CwdChanged` (a `cd` in the main conversation; subagents fire none), with `SessionStart` and
`UserPromptSubmit` as backstops for a missed event and for installs that predate the
`CwdChanged` entry.

`@cockpit_cwd` keeps the cwd the hooks saw last. `@cockpit_dir` is written only when the
payload's cwd differs from it, or when a new `session_id` arrives. An agent that drives a
hand-made worktree with `git -C` sets `@cockpit_dir` itself ([review.md](review.md)), and a
prompt in the same cwd leaves that value alone; its next `cd` replaces it.

A value is refused, and the old option kept, when it is not a whole JSON string (a payload cut
by the 4 KB cap, or by a writer that died), not absolute, holds a control character or a
backslash, or names no directory. `#` is allowed: a directory may be named `x#y`, and
`set-option -p` stores `#(...)` as written. tmux reads an argument's trailing `;` as a command
separator, so a directory ending in `;` is passed as `\;`.
