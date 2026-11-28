# Accounts on the board: plan

Owner-approved 2026-11-28. Epic #1599. This page holds the plan. Each PR changes the user
docs it touches, and this page keeps only what no other page says.

## Goal

Show each agent's remaining 5-hour and weekly allowance on its tile, name the Claude Code
account a pane runs under, and move an idle pane to another of the owner's own accounts with
one key.

An account is a `CLAUDE_CONFIG_DIR`, the way Claude Code keeps several logins apart
(https://code.claude.com/docs/en/authentication). The usage figures come from the
`rate_limits` field Claude Code passes to the status line
(https://code.claude.com/docs/en/statusline). The cockpit holds directory paths and account
names only. It never reads, copies or logs a token, and never calls Anthropic's usage
endpoint.

The owner's setup, three personal accounts and one work account:

```
set -g @cockpit_accounts "p1=~/.claude p2=~/.claude-2 p3=~/.claude-3 work=~/.claude-work"
set -g @cockpit_pool "p1 p2 p3"
```

The work account is never in the pool. The switch is a key by default
(`@cockpit_switch key`), and a second key starts a new window under any listed account.

## The five PRs

1. **PR 1, #1600: allowance on the tile.** It also refuses a `resets_at` in the past or
   more than 7 days and 1 hour away, and shows how old the reading is.
2. **PR 2, #1601: account name on the tile.** The board accepts transcripts under every
   listed account, and setup docs cover one login per directory and linking `projects/`,
   `history.jsonl` and `settings.json` between the three personal directories only.
3. **PR 3, #1602: per-window warning lines and the weekly "ahead" mark.** The lines are 10%
   left on 5h and 3% left on weekly. The ahead mark follows the claude-swap rule: 15 points
   over the elapsed share of the week, hidden for the first 24 h after a reset, with no ETA
   in the UI.
4. **PR 4, #1603: a reading per account in a state file, with "closed" derived from it.**
   Closed means used ≥ 100% and now < `resets_at`, plus a small grace. PR 4 also adds the
   start and switch keys with fill-first order (the first account in `@cockpit_pool` order
   that is not closed). It switches only an idle or done pane, restarting and resuming in the
   same pane, and runs the resume spike with the linked-history setup first.
5. **PR 5, #1604: auto mode,** off by default.

## Not taken, and why

- **A router or proxy in front of the API.** A local test rig showed one changes the
  identity the client sends even with every option off. Phoebis and CLIProxyAPI change it on
  purpose: CLIProxyAPI signs the billing hash under its own policy
  (`internal/runtime/executor/claude_signing.go:274`) and fills a missing `account_uuid`
  with values "derived from seed"
  (`internal/runtime/executor/helps/claude_cli_identity_seed.go:79`). Its own tracker reports
  banned accounts (router-for-me/CLIProxyAPI issue 2211).
- **Polling Anthropic's usage endpoint.** It needs the account's token, and claude-swap
  measured its limit at about 28 to 30 requests an hour per identity
  (`src/claude_swap/poll_policy.py`). The
  status line already carries the same figures for free.
- **Swapping the live login file and refreshing tokens,** claude-swap's main mode. It
  handles tokens. One directory per login needs none of that.
- **Reading `<dir>/.claude.json` to catch a directory logged into another account.**
  That file can hold MCP secrets, and the panel already treats it as secret-bearing
  (`tui/src/panel/request.rs`). The setup docs say one login per directory instead. Two
  directories on one login can drift when the server rotates the refresh token (claude-swap
  `src/claude_swap/session.py`).
- **A backoff when no reset time is known** (CLIProxyAPI
  `sdk/cliproxy/auth/conductor_cooldown.go`). It is a second path when the first fails, which
  AGENTS.md check 3, No new fallback, rules out. Every reading carries `resets_at`, and a
  pane with no `rate_limits` has no reading to back off from.
- **A time-to-empty projection.** claude-swap keeps its own out of the UI for its wide error
  bars (`src/claude_swap/pace.py`).
- **"Most room first" order.** A 5-hour window starts at the first message, so spreading
  work over all accounts starts every clock at once. Filling one account before the next
  keeps the resets apart (CLIProxyAPI `sdk/cliproxy/auth/selector.go:92-95`).
- **Switching a busy agent.** Restarting a running or asking pane loses the turn in flight,
  so no mode does it.
- **Out of scope:** logging in for the owner, moving a conversation between the work account
  and the personal ones, the list view, the status bar and `--json`.

## The transcript gap PR 2 fixes

[Session identity](../states-and-hooks.md#session-identity) holds the rule: the hook checks a
transcript against the projects tree of the `CLAUDE_CONFIG_DIR` claude runs with, and the
board against its own environment's. What several accounts add, read from the code and not
yet shown by a test:

- The board is started once, under one account, so it refuses the transcript of a pane on
  any other unlinked account. That tile loses the doing line, the model, the context and the
  subagent tree. Linking `projects/` between the personal directories makes their paths
  resolve to one place, so they pass. The work account keeps its own history and still fails.
- With `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` on, the hooks see no `CLAUDE_CONFIG_DIR`
  ([Agent view and background jobs](../states-and-hooks.md#agent-view-and-background-jobs)).
  The hook then refuses an unlinked account's transcript too, and leaves `@cockpit_cfg`
  unset, which reads as `~/.claude`.
- PR 2 makes the board accept a transcript under the projects directory of any account in
  `@cockpit_accounts`. It must not name the default account for a pane whose hooks ran
  under the scrub, since the hook could not see that pane's directory. #1601 carries this
  case.

Third-party code is cited at CLIProxyAPI commit dbe641f2 and claude-swap commit 9bc83a5c.
