# Side-fixer hook facts — Claude Code 2.1.245

Read-only research. Docs fetched live from code.claude.com/docs/en/*.md 2026-08-25.
Baseline already verified in scratchpad/handoff-check-report.md (§1B, §4bis) — cited here as "given", not redone.

## A. additionalContext / stop_hook_active

**A1 — [E, given].** `Stop`/`SubagentStop` `hookSpecificOutput.additionalContext` shipped in **v2.1.163**
(CHANGELOG.md L1611, quoted in handoff-check-report.md #10). hooks.md documents it today: *"unlike
`decision: "block"` it is shown in the transcript as hook feedback rather than a hook error"*, transcript
labels it `Stop hook feedback`. Confirmed still current on 2.1.245 (hooks.md fetched today, unchanged).

**A4 — [I], partially answered, no primary text found.** Third-party guides (converging, non-primary)
describe it correctly: *"the JSON payload... includes a `stop_hook_active` flag, which is true when the
current stop is already happening because a previous stop hook blocked... The first time Claude tries to
stop, the flag is false... this time the flag is true, because Claude Code knows this stop only exists as
the result of a previous block."* This is a **per-Stop-attempt** flag, not a per-invocation timer — it's
computed once when Claude Code decides to fire a new Stop event, before your hook runs, and handed to your
hook as input. It does **not** change mid-invocation while your hook holds/polls. Design implication: your
single long-lived invocation reads `stop_hook_active` once at start (it'll be `false` on the first
Stop — no prior block occurred), holds internally, and returns `additionalContext` once. That is one
Stop-cycle, so it counts as one unit toward the cap (see B6). **hooks.md itself never uses the string
"stop_hook_active" in its own reference prose** — the only documentation I could find it in is the
hooks-guide.md troubleshooting section (see B6 quote) and third-party guides. Flag this as the weakest
answer in the set: I could not find a primary sentence that explicitly states "computed once, fixed for
the invocation's duration" — I inferred it from the mechanism description (Claude Code sets it based on
whether *this* Stop event followed a block, and hands it to you as one input value). Nothing contradicts
the design; nothing proves the edge case (what if you're mid-hold when the user does something that would
otherwise trigger a second concurrent Stop — architecturally Stop events are serial per turn, so this
shouldn't arise).

## B. Holding

**B5 — spinner/cancel — [E] partial, [I] partial.**
hooks.md **[E]**, Common fields table: *"`statusMessage` (not required): Custom spinner message displayed
while the hook runs"* — confirms a command/http/mcp_tool hook shows a spinner, customizable. This directly
supports your design: set `statusMessage` to something like "waiting on side-fixer…" during the hold.
Whether Esc cancels a running hook and what happens to its children: **not found in hooks.md or
hooks-guide.md** (both fetched fresh, no "Esc"/"cancel" hits in the hook-execution context — hooks-guide's
only Esc mention is unrelated, closing the notification-test panel). Third-party guides ([I], not
verified) claim Esc can interrupt a stuck/spinning tool call generally, but I found no hook-specific
confirmation either way. **Treat as unconfirmed — do not assume Esc safely tears down your poll-hold; test
empirically before relying on it.**

**B6 — cap counts invocations, not duration — [E], given + reinforced.** From handoff-check-report.md
#16/#17 (hooks.md L2441, env-vars.md L358): *"Claude Code overrides the hook and ends the turn after 8
consecutive blocks"*; cap is `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP`, default 8. hooks-guide.md today, fetched
fresh, **[E]**: *"Claude Code overrides a Stop hook after it blocks eight times in a row without
progress... Parse the `stop_hook_active` field from the JSON input and exit early if it's `true`"* — the
guard pattern is check-once-per-invocation, confirming each **invocation** (regardless of how long it
ran) is one unit against the cap, not each second of duration. Also reconfirmed from
handoff-check-report.md #10 (hooks.md quoted): `additionalContext` *"keeps the conversation going through
the same loop protections as `decision: "block"`, namely the `stop_hook_active` input and the
8-consecutive-continuation cap"* — so **additionalContext counts too**. Your design (poll-hold inside ONE
invocation, return ONE additionalContext) = one continuation, one count. Nowhere near the cap of 8 for a
single handover.

**B7 — where output goes — [E], given + new detail.** From handoff-check-report.md #9/#16 + hooks-guide.md
fetched fresh, **[E]**: exit-0 stdout for most events "written to the debug log but not shown in the
transcript" and never reaches the model except for `UserPromptSubmit`/`UserPromptExpansion`/`SessionStart`.
For Stop specifically, structured JSON `additionalContext` is what the **model** sees (labeled `Stop hook
feedback` in transcript, visible to the human too via `Ctrl+O` transcript view — hooks-guide.md **[E]**:
*"Press `Ctrl+O` to open the transcript view to check the outcome of a hook run... Successful run: you see
nothing, unless the hook's JSON surfaces something, such as `systemMessage` or Stop hook feedback."*).
Full stdout/stderr for debugging: `claude --debug-file <path>` or `/debug` mid-session, per hooks-guide.md
**[E]**: *"Start Claude Code with `claude --debug-file /tmp/claude.log`... If you started without that
flag, run `/debug` mid-session to enable logging."* The `/hooks` menu (hooks-guide.md **[E]**) shows
*"event, matcher, type, source file, and the full command, prompt, or URL"* — configuration only, not
runtime output.

## C. Spawning

**C8 — sandbox/env for hook children — [E] + [I] gap flagged.**
- Env: hooks.md **[E]**, fetched fresh: *"Handlers run in the current directory with Claude Code's
  environment."* Plus exported placeholders `CLAUDE_PROJECT_DIR`, `CLAUDE_PLUGIN_ROOT`,
  `CLAUDE_PLUGIN_DATA`; `$CLAUDE_EFFORT`; `$CLAUDE_CODE_REMOTE` (not set locally); `$CLAUDE_CODE_BRIDGE_SESSION_ID`
  (v2.1.199+, only with an active Remote Control connection). No evidence `CLAUDE_*` vars are stripped —
  the opposite: it inherits "Claude Code's environment" wholesale, so a spawned `tmux split-window`'s
  child (a headless `claude -p`) would also see these vars unless you unset them.
- Session/TTY: hooks.md **[E]**, fetched fresh, new finding not in the prior report: *"On macOS and Linux,
  command hooks run in their own session without a controlling terminal. The hook process and any child
  processes can't open `/dev/tty` or send escape sequences directly to the Claude Code interface."* This
  confirms hooks are already `setsid`-style detached from the terminal — good news for spawning: your hook
  is not fighting for the TTY.
- **Process-group/timeout-kill behavior on the hook's own children: NOT documented anywhere in hooks.md or
  hooks-guide.md.** The doc only states, given: *"A `command`, `http`, or `mcp_tool` hook that reaches its
  `timeout` is canceled: Claude Code discards the hook's output, and the hook renders no decision."* This
  says the hook's *output* is discarded — it does not say whether the hook process itself, or any
  background children it spawned, are sent SIGTERM/SIGKILL. **[I], inferred from related but distinct GitHub
  issues** (none are hook-specific, so treat with caution):
  - `#45717` (Bash **tool** timeout, not a hook) — reports SIGTERM sent to the child's process group also
    killed the parent Claude Code process, because they shared a process group; a proposed fix is spawning
    in a new process group. This is about the **Bash tool's own command timeout**, a different code path
    from hook timeouts, but shows Anthropic's spawn code has had process-group bugs before.
  - `#43944` — "Background processes started by Bash tool are not cleaned up on session exit" — again the
    Bash tool, not hooks, but suggests detached background children are **not** aggressively reaped by
    default; a `disown`ed/backgrounded process tends to survive its parent.
  - `#41577` — "SessionEnd hooks are killed before completion when running async work" — this IS
    hook-specific and directly relevant: it shows at least one hook event (`SessionEnd`) has documented
    hard-killed hook processes doing async work; hooks-guide.md **[E]** independently confirms
    `SessionEnd` hooks get only *"a 1.5-second budget"* (or up to 60s if you raise its per-hook `timeout`).
    `Stop` hooks are a different, longer-lived event (10 min default per hooks-guide.md, given) and nothing
    suggests the same aggressive kill applies to Stop — but this is the one piece of negative evidence
    worth carrying forward: **don't assume a hook process's descendants outlive the hook's own exit/timeout
    by default.**
  - **Conclusion for your design: do NOT rely on nohup/disown from inside the hook process itself.** The
    one thing that is independently, separately confirmed safe is `tmux split-window -d`: this doesn't
    background a child of the hook process at all — it asks the **already-running tmux server** (a wholly
    separate, long-lived process tree the hook doesn't own and can't accidentally take down) to create a
    pane and run the fixer inside it. Once `tmux split-window -d` returns, the fixer pane's process is a
    child of the tmux server, not of the hook — so hook timeout/exit has no bearing on it. This is the
    correct design already implied by the brief ("must the spawn be detached via tmux, which is
    server-side, vs `nohup`") — **confirmed as the right call, not merely a hedge.**

**C9 — `claude -p` / headless flags, cli-reference.md, fetched fresh — [E] throughout unless marked:**
- Prompt: `claude -p "query"` (arg) or `cat file | claude -p "query"` (stdin, piped content combines with
  the arg prompt). Both documented.
- Working dir: no `--cwd` flag found; headless inherits the process's cwd (launch it via
  `tmux split-window -c <dir>` or `cd` in the pane command, not a `claude -p` flag).
- `--output-format`: `text`, `json`, `stream-json`. **Doc did not state explicitly whether `json` includes
  a final `result` field** — this needs a live smoke-test (`echo hi | claude -p "say hi" --output-format json`)
  rather than doc-inference; flagging as open, not fabricating an answer.
- `--allowedTools`/`--allowed-tools`, `--disallowedTools`/`--disallowed-tools`: use [permission rule
  syntax](https://code.claude.com/docs/en/permissions#permission-rule-syntax) — confirmed and now fully
  detailed from the primary permissions.md page (fetched fresh, long verbatim section, key excerpts):
  - `Edit(docs/**)` scopes Edit to a path glob; **`Write` path rules are silently never consulted** —
    *"If you write a path rule for `Write`... Claude Code accepts the rule but never consults it... Use
    `Edit(docs/**)` in place of `Write(docs/**)`"* (requires v2.1.210+). **This is a real gotcha for your
    scope fence: writing `Write(review/**)` does nothing — you must write `Edit(review/**)` to cover both
    Edit and Write.**
  - `Bash(git *)` as a deny: *"A rule such as `Bash(git commit *)` allows the command... `*` stands in for
    whatever text is in its place"*; put `*` after the subcommand: `Bash(git *)` allows/denies every git
    subcommand, `Bash(git log *)` only `git log`.
  - Path anchors: `//path` = absolute from filesystem root, `~/path` = home-relative, `/path` = relative to
    the settings source (project root when in `.claude/settings.json`), `path`/`./path` = relative to cwd.
  - Deny-rule directory patterns match at any depth (`Read(secrets/**)` catches nested `secrets/` dirs
    too); allow-rule directory patterns match only the exact anchored location.
  - Combine with `--settings '<inline JSON>'` to hand a headless fixer a self-contained
    `permissions.deny`/`permissions.allow` block without touching the repo's own settings files.
- `--add-dir <path>`: adds a working directory Claude may read/edit; grants file access, not full
  `.claude/` config discovery (most config types are NOT loaded from `--add-dir` dirs — skills/commands/
  subagents are exceptions, loaded; CLAUDE.md needs an explicit env var).
- `--permission-mode`: `default`, `acceptEdits`, `plan`, `auto`, `dontAsk`, `bypassPermissions` (also
  accepts `manual` as an alias for `default`). For an unattended scope-fenced fixer, `acceptEdits` +
  narrow `Edit()`/`Bash()` allow/deny rules, or `bypassPermissions` only inside an already-isolated
  worktree, are the realistic choices — `default`/`plan` would stall waiting on a human who isn't there.
- `--session-id <uuid>`: **confirmed, you can choose it** — *"Use a specific session ID for the
  conversation (must be a valid UUID)"*. This makes the transcript path knowable ahead of time
  (`~/.claude/projects/<slug>/<uuid>.jsonl`), so your lock file can point straight at it without scanning
  for "newest jsonl".
- `--output-format json` / `stream-json`: as above, `json` presumably has a final result but unconfirmed by
  docs text; `stream-json` is for a live event stream, consistent with a "live view" pane.
- `--max-turns`: caps agentic turns in print mode, errors out at the limit — good belt-and-suspenders
  bound on a runaway fixer.
- `--append-system-prompt`: appends to the default system prompt (doesn't replace it); there's also a
  separate `--system-prompt` that fully replaces it — useful if you want the fixer's system prompt to be
  entirely the scope-fence text rather than Claude Code's defaults.
- `--model`: alias (`sonnet`/`opus`/`haiku`/`fable`) or full model ID.
- Interactive `claude "<prompt>"` (no `-p`): **[E]** documented as "Start interactive session with initial
  prompt" — it runs the prompt but **stays interactive afterward**; there is no documented flag to
  auto-exit when done. So for a detached tmux pane you want the process to actually terminate on its own,
  `-p` (print/headless mode) is the only non-interactive shape — confirms your design should launch the
  fixer with `claude -p`, not a bare `claude "<prompt>"`, if you want the pane to close itself.

**C10 — CONFIRM THE NEGATIVE — [E], strong, from two independent primary pages fetched fresh today.**
No hook output field spawns a Claude session, subagent, or Claude-Code-managed process. hooks-guide.md
Limitations section, verbatim: *"Command hooks communicate through stdout, stderr, and exit codes only.
They can't trigger `/` commands or tool calls."* hooks.md (via careful full-page search): the only fields
that touch anything process-like are `systemMessage` (context text), `terminalSequence` (escape codes),
`additionalContext` (context text), and `updatedInput` (rewrites the *current* tool's input) — none spawn
anything. **One adjacent thing worth flagging, not a hook *output* field but a hook *type*:**
`"type": "agent"` hooks (hooks-guide.md, Agent-based hooks section, experimental) do make Claude Code
spawn a subagent — but this is a fixed, synchronous, Claude-Code-owned verification subagent (default
60s timeout, up to 50 tool-use turns, "experimental... behavior and configuration may change," explicitly
recommended against for production: *"For production workflows, prefer command hooks"*), invoked as the
hook itself rather than something a command hook's output can trigger. It cannot do what your design
needs (fire-and-forget a detached tmux pane that outlives the hook invocation) — it's Claude Code
synchronously calling Haiku/a subagent and blocking on its single verdict. **Confirmed: nothing in the
hook system spawns your side-fixer for you; you must do it yourself via `tmux split-window` inside a
`command` hook, exactly as designed.**

## D. Prior art

**D11 — [E]/[I], mostly given.** handoff-check-report.md #38 already has the load-bearing citation:
claude-review-loop's `task → addressing` Stop-hook state machine, verbatim from its README: *"If in `task`
phase: writes a runner script and prompt file, transitions to `addressing`, blocks exit with
instructions"*; state lives in `.claude/review-loop.local.md`; *"The stop hook timeout is set to 30
seconds… The hook itself is fast (it only writes files and returns a block decision)"* — note this prior
art's hook is fast/non-blocking (writes files, doesn't hold), unlike your poll-hold design; it relies on
the *next* Stop cycle to re-check state rather than blocking inside one invocation. Both are valid shapes;
yours trades "hold once, ≤5 min" for "block repeatedly across several Stop cycles until the cap." Given
the cap is 8 and each of claude-review-loop's checks is near-instant, their shape burns through the cap
fast if the human takes minutes to review; yours burns 1 unit of the cap for potentially several minutes.
No other tool in the READMEs grepped for the prior report (`hunk`, `plannotator`, `difit`, `diffity`,
`vibe-kanban`, `claude-squad`, `herdr`, `revdiff`) implements a sibling-process hold pattern — that search
already covered "urgent/priority" lanes, not this specific hold-for-a-worker pattern, so treat "no other
prior art found" as [I] (absence of evidence from a search aimed at a different question), not a
confirmed negative. I did not re-run a fresh targeted search for "hold agent while sibling process works"
prior art beyond what the given report covered — out of scope for read-only time budget; flag as a gap if
the team wants it closed.
Permission-rule syntax for scope-fencing a headless claude: fully answered above under C9 from
permissions.md — the syntax is `permissions.deny`/`permissions.allow` arrays of `Tool(specifier)` strings,
passable inline via `--settings '<json>'` on the `claude -p` invocation.

## E. Lock semantics — general tmux/POSIX facts, [I] (not tested live on this box per read-only constraint)

- `tmux split-window -d -P -F '#{pane_id}'`: `-d` keeps focus on the original pane (detached child pane,
  not "detached session"), `-P` prints info about the new pane, `-F` sets the format — `#{pane_id}` is
  tmux's standard unique pane identifier format variable (`%N`). This combination is the documented,
  standard way to create-and-capture a pane id (man tmux(1), `split-window` section; corroborated by
  multiple independent tmux guides in the search results). **[I]** — did not execute on this box per the
  read-only constraint; this is standard, stable tmux(1) behavior across the versions in wide use.
- `#{pane_dead}`: true once the pane's process has exited **and** `remain-on-exit` is on for that pane/
  window (otherwise the pane just closes and the pane_id stops existing — nothing to poll). Set
  `remain-on-exit on` (globally or with `-o` no — it's a `set-option`, e.g.
  `tmux set-option -p -t <pane_id> remain-on-exit on` or set it before the split via a window option) so a
  finished fixer's pane sticks around long enough for your hold-loop to observe `pane_dead` before you
  `kill-pane` it. Note tmux's own pane-exited **hook** only fires when `remain-on-exit` is *off* (per the
  tmux-users thread found), so if you want to poll a flag rather than register a tmux hook, keep
  `remain-on-exit on` and poll `#{pane_dead}`, or don't use `remain-on-exit` and instead rely on the pane
  disappearing from `tmux list-panes` output as your "it's gone" signal.
- `kill -0 <pid>`: standard POSIX idiom — sends no signal, just checks the process exists and you have
  permission to signal it; nonzero exit (with `$?`) means the process is gone or unreachable. Standard
  stale-lock/stale-PID check. **[I]**, universal POSIX behavior, not Claude-Code-specific.
- `mkdir` as an atomic lock primitive: **[I]**, standard shell idiom — `mkdir` is atomic on POSIX
  filesystems (a `mkdir` on an existing path fails with `EEXIST`, and there's no window where two
  concurrent `mkdir` calls on the same path can both "succeed" — the filesystem itself serializes it),
  which is exactly why it's the classic portable shell lock (`mkdir "$lockdir" 2>/dev/null || exit 1`).
  Preferable to `flock` here specifically because `flock(1)` is a Linux-only (util-linux) command not
  present on macOS by default (macOS has the `flock()` syscall via `/usr/bin/flock` only via extra
  install, or none at all depending on version) — `mkdir` needs no extra binary and works identically
  across the Linux box this session is on and macOS, which is presumably a portability goal.

## Summary for the design (per C10 instruction: flag anything that changes it)

Nothing here overturns the design. Three things sharpen it:
1. Use `Edit(<path-glob>)` in `--allowedTools`/`--settings`, **not** `Write(<path-glob>)` — Write path
   rules are silently ignored (v2.1.210+ behavior, confirmed current).
2. Spawn the fixer via `tmux split-window -d`, not `nohup`/`disown` inside the hook — hook children's
   survival past hook timeout/exit is undocumented and adjacent evidence (GitHub issues, not hook-specific)
   suggests process-group kill bugs have existed in Claude Code's own spawn code; tmux server-side spawn
   sidesteps the whole question because the fixer becomes tmux's child, not the hook's.
2b. Launch with `claude -p`, not bare `claude "<prompt>"` — the bare form stays interactive with no
   documented auto-exit flag, so the pane would hang open after the fixer finishes.
3. `--session-id <uuid>` lets you pick the fixer's session id up front, so the lock file can name the
   transcript path directly instead of discovering it.

Two genuine gaps, not filled by docs, worth a live smoke-test rather than more searching:
- Whether `--output-format json` includes a final `result` text field.
- Exact behavior of Esc pressed by the human while your Stop hook is mid-poll-hold (spinner customizable
  via `statusMessage`, but cancel-during-hook-execution semantics are undocumented).
