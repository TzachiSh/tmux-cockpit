# Spec: RimZ-inspired improvements to tmux-cockpit

Source: analysis of https://github.com/rimio-ai/rimz (MIT) on 2026-08-24.
RimZ is a Rust control-room TUI for coding agents on tmux/Zellij. Five of its
ideas transfer to tmux-cockpit's lightweight model. Everything below must keep
cockpit's core invariant: **a status redraw runs no shell** — hooks and
keybindings write per-pane tmux options; formats only read them. State dies
with its pane. No daemon, no state files.

## F1 — Jump to the blocked pane (`prefix J`)

A key that drops you into the next pane whose agent needs you, cycling across
ALL sessions on repeated presses. RimZ: "one click drops you into the pane
that is waiting."

- New `scripts/tmux-jump`: list panes server-wide, filter
  `@panestate == needs-input`, jump to the one after the current pane
  (wrapping), `switch-client` + `select-window` + `select-pane`.
- No blocked pane → `display-message "no agent needs you"`, exit 0.
- Bound by cockpit.tmux to `prefix J`, key overridable via `@cockpit_jump_key`
  (literal `off` disables), same pattern as existing bindings.
- One-shot script: only runs on keypress, so the no-shell-redraw invariant is
  untouched.

## F2 — Fleet counter token `#{agent_fleet}`

A status-line token that reads the whole fleet at a glance (RimZ cockpit line
`? 2  ! 1`): count of `needs-input` panes and `error` panes across ALL
sessions, rendered like `▲2 ✕1`, empty when both are zero.

- Pure tmux format, zero shell: emit one `x` per matching pane via nested
  `#{S:#{W:#{P:…}}}`, measure with `#{n:…}`, wrap in colour conditionals.
- Added to cockpit.tmux's `interpolate()` like `#{agent_where_*}`; documented
  in README and examples/tmux.conf status-right.
- Counts only agent panes (the `@cockpit_agents_glob` guard, same as the
  existing banner formats).

## F3 — Show the ask itself

When a pane goes `needs-input`, surface WHAT it is asking (RimZ "Ask" column):
the Notification hook payload's message, stored per-pane, shown next to the
alert so you can triage without switching.

- `scripts/agent-state` `notify` arm already reads the payload; additionally
  extract the JSON `message` value and write it to `@panestate_msg`
  (truncated, control chars stripped). Every state transition that clears or
  overwrites the alert (`running`, `off`, `done` accepted) clears the message.
- Render: the pane-border needs-input arm appends
  `#{=/48/…:#{@panestate_msg}}`; format-only change in examples/tmux.conf and
  the theme-less fallback in cockpit.tmux.
- Hook budget unchanged: same single bounded read, one extra `tmux set` only
  on the needs-input transition.

## F4 — Context health from the Claude Code statusline

Claude Code's `statusLine` command receives JSON each turn (model, context
usage, cost). RimZ wraps it to power "context health". Cockpit ships an
optional shim writing `@panestate_ctx` (percent of context used) so tabs and
borders can warn before compaction.

- New `scripts/agent-context`: bounded stdin read (agent-state pattern),
  extract context fill from the captured real schema, write `@panestate_ctx`,
  print a minimal one-line statusline (pass-through of a user-configured
  original command is out of scope for v1).
- Schema is captured from a real `claude` run first; the parser is tested
  against that fixture. Unknown/absent fields → write nothing, print nothing
  extra, exit 0. Never block the agent.
- Render example in examples/tmux.conf (tab text gains a `◔NN%` suffix when
  `@panestate_ctx >= 80`). Documented as optional in README +
  examples/claude-hooks.json comment.

## F5 — Hook installer with dry-run diff

The hooks in examples/claude-hooks.json are mandatory for cockpit to show
anything, and today the README says "merge this yourself". RimZ installs hooks
additively with consent and an exact diff.

- `install.sh --hooks`: jq-merge cockpit's hook entries into
  `~/.claude/settings.json` — additive only (existing foreign hooks preserved,
  cockpit entries not duplicated on re-run), `settings.json.bak` written
  before any change.
- `install.sh --hooks --dry-run`: print a unified diff, write nothing.
- No jq → clear message and exit 1 (the manual path remains in README).
- `--uninstall` does NOT touch hooks (explicit `--hooks --uninstall` may come
  later; out of scope).

## Global constraints (all features)

- bash 4+ (`tm` precedent), tmux ≥ 3.1 already assumed by the repo; BSD/macOS
  quirks documented in-code where relevant (readlink -f, etc.).
- `make check` (shellcheck + tests/run.sh) green is the merge gate.
- Tests live in tests/run.sh as self-contained blocks on PRIVATE tmux sockets
  (`need_tmux` guard, `SOCKS="$SOCKS $X"` cleanup), suite style: `is/ok/bad`.
- Comment style: explain constraints and failure modes, not what lines do.
- No new runtime dependencies except jq for the OPTIONAL `--hooks` path.
- Redraw-cost invariant: no `#(command)` substitutions in any format cockpit
  sets or documents.
