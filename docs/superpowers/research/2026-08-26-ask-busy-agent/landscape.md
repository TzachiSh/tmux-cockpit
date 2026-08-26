# The landscape: how the community supervises agents in tmux

Web survey, 2026-08-26. Stars as shown that day. Three questions per tool: how it knows the state, what it shows for "what is it doing", how the human asks or gets told. [sources.md](sources.md) reads the code of the tmux-native ones.

## tmux-native managers and pickers

| tool | ★ | state from | shows | ask a busy agent | notify | stack |
|---|---|---|---|---|---|---|
| [claude-squad](https://github.com/smtg-ai/claude-squad) | 8.4k | pane preview only | tail + diff tab | attach | none | Go, Bubble Tea, session + worktree per instance |
| [agent-deck](https://github.com/asheshgoplani/agent-deck) | 794 | pane poll + hooks | ● ◐ ○ ✕, last message, cost | attach, or fork the session and ask the fork | status bar, Telegram/Slack | Go, Bubble Tea |
| [ccmanager](https://github.com/kbwo/ccmanager) | 1.2k | PTY output regex, no tmux | Waiting / Busy / Idle | no; a Haiku call auto-approves safe permission prompts | state-change hook runs a command | TS, Ink |
| [tmux-claude-session-manager](https://github.com/craftzdog/tmux-claude-session-manager) | 369 | `claude agents --json` (was hooks → `@claude_state`) | fzf row with a dot, preview = `capture-pane` | no | bell via `preferredNotifChannel: terminal_bell` | bash, fzf, jq, popup with a nested tmux so the agent survives the popup |
| [ccmux](https://github.com/epilande/ccmux) | 146 | hooks → pid/state files, then JSONL, then pane text | icons grouped by project | `ccmux handoff a b` types the last response into b's composer; busy targets queue until idle | desktop with Approve/Deny and inline Reply | Bun, Solid TUI, daemon :2269 |
| [recon](https://github.com/gavraz/recon) | 262 | Claude's own footer text: "esc to interrupt" working, "Esc to cancel" input, "(0 tokens)" new; pid → `~/.claude/sessions/<pid>.json` | table, or a Tamagotchi per agent | no | none | Rust, `display-popup -E recon` |
| [claude-tmux](https://github.com/nielsgroen/claude-tmux) | 203 | pane regex (`❯` + "ctrl+c to interrupt", `[y/n]`) | last lines in colour | no | none | Rust |
| [agent-manager](https://github.com/YoanWai/agent-manager) | 360 | pane scraping, 8 agents | live preview, project tree | no | desktop | Go, Bubble Tea |
| [dmux](https://github.com/formkit/dmux) | 1.8k | n/a | AI-named panes/branches | no | macOS notifications | Node |
| [ntm](https://github.com/Dicklesworthstone/ntm) | 431 | per-agent readiness regexes over `capture-pane` | dashboard | `ntm send` broadcasts via send-keys | n/a | Go |

## Status-bar plugins (hooks → status line)

| tool | ★ | mechanism |
|---|---|---|
| [tmux-agent-status](https://github.com/samleeney/tmux-agent-status) (samleeney) | 261 | hooks only; sidebar pane per session + a pulsing glyph per agent; sounds on done |
| [tmux-agent-indicator](https://github.com/accessd/tmux-agent-indicator) | 91 | UserPromptSubmit → running, PermissionRequest → needs-input, Stop → done; border colour, window title, status icon; `display-message` on transitions; custom command hook; OpenCode via its event bus |
| [tmux-agent-status](https://github.com/partner0/tmux-agent-status) (partner0) | 0 | window renamed with 🔴🟡🟢⚪; picker sorted 🟡 first |
| [tmux-notify](https://github.com/claude-contrib/claude-extensions/blob/main/plugins/tmux-notify/README.md) | n/a | Stop + Notification hooks: bell to `#{pane_tty}`, `display-message "Claude [win]: done/waiting"`, optional `select-pane`; skipped when the active pane is `$TMUX_PANE` |
| [cc-notifynub](https://github.com/konstantindenerz/cc-notifynub) | n/a | a pixel face in a `display-popup` pinned top-right; Stop → done eyes, 5 s; permission → 10 s; Enter jumps |

Cockpit's marker set (`▲ ✕ ● ✓ ○`, border colour, `NEEDS YOU` border text with the question, fleet counter across sessions) is already the richest of this row.

## Transcript watchers

| tool | ★ | mechanism |
|---|---|---|
| [agenthud](https://github.com/neochoon/agenthud) | 5 | tails JSONL: a pending `tool_use` at the tail = working, a yielded turn = waiting; shows tool + path stream, context gauge, subagent nesting; `agenthud summary` pipes a digest through `claude -p` |
| [claude-code-ui](https://github.com/KyleAMathews/claude-code-ui) | 413 | chokidar on `~/.claude/projects`, XState → kanban Working / Needs approval / Waiting / Idle; PermissionRequest hook for accuracy, else `tool_use` pending > 5 s |
| [claude-code-dashboard](https://github.com/Stargx/claude-code-dashboard) | 11 | same tail; active files, permission-mode badge |
| [claude-code-monitor](https://github.com/bruceyxli/claude-code-monitor) | 10 | PreToolUse hook POSTs and blocks until the dashboard's Allow/Deny answers; remote approval |
| [claude-code-monitor](https://github.com/onikan27/claude-code-monitor) | 304 | hooks → TUI + phone web UI; AppleScript focuses the right terminal tab by TTY; phone answers permission prompts |
| [Codeman](https://github.com/Ark0N/Codeman) | 720 | every session in tmux, rendered with xterm.js; hooks POST events; subagents drawn as floating windows; task queue; push over Tailscale |
| [Claude-Code-Agent-Monitor](https://github.com/hoangsonww/Claude-Code-Agent-Monitor) | 940 | hook-fed SQLite + kanban, Electron |
| [claude-dashboard](https://github.com/rdytech/claude-dashboard) | 0 | Textual list of sessions awaiting reply, built because "there's no ambient signal" |

## Desktop and web workspaces (not tmux)

Conductor (Mac, worktree per workspace, diff comments become composer attachments), Crystal (deprecated Feb 2026 → Nimbalyst), vibe-kanban (27.9k, sunsetting; kanban columns are the state), Happier (phone companion with three modes for a busy agent: Pending queue, Steer, Interrupt), cmux (Mac terminal with a tmux shim; sidebar per pane).

## Orchestrators (an agent supervises agents)

| tool | ★ | talking to a busy agent |
|---|---|---|
| [Gas Town](https://github.com/steveyegge/gastown) | 17.8k | `gt nudge`: `send-keys -l`, 500 ms, Enter separately, queued until idle, retry/backoff, DND unless `--force`; "never use raw tmux send-keys". `gt mail` async inbox; `gt escalate` → mail/email/SMS by severity. `gt feed` three-panel TUI with a "problems" view for stuck agents |
| [Tmux-Orchestrator](https://github.com/Jedward23/Tmux-Orchestrator) | 1.8k | `send-claude-message.sh`: send-keys, sleep, Enter; the orchestrator reads `capture-pane` |
| [cli-agent-orchestrator](https://github.com/awslabs/cli-agent-orchestrator) | 1.1k | cao-server, HTTP/PTY WebSocket/MCP, web UI |
| [claude-tmux-orchestration](https://github.com/primeline-ai/claude-tmux-orchestration) | 39 | workers write `workers/w1.json` status; blocking questions become files in `inbox/w1/`; prompts via `load-buffer` + `paste-buffer` + separate Enter; `.ready` handshake |
| [ralph-claude-code](https://github.com/frankbria/ralph-claude-code) | 9.6k | `--monitor` tmux layout: loop count, calls vs limit, log tail; `.ralph/status.json` |

## Claude Code itself (2.1.139+)

Agent view (`claude agents`): a Haiku-written one-line summary per session, `Space` peeks the exact question with a reply box when blocked, read-only status sentence when working. `claude agents --json` for scripts. Notification types `agent_needs_input`, `agent_completed`. Side-channel replies to queued messages were requested and closed not-planned ([#29224](https://github.com/anthropics/claude-code/issues/29224)); an external injection API closed duplicate ([#27441](https://github.com/anthropics/claude-code/issues/27441)). `/btw` is the in-pane side channel. Known bug: agent view shows "working" during a permission prompt ([#58965](https://github.com/anthropics/claude-code/issues/58965)).

## Techniques seen more than once

- Observer summariser: Stop hook (`async`, 30 s) → jq over the JSONL (last user/assistant text, tools dropped, 300 chars) → `env -u CLAUDECODE claude -p --model haiku` → a file → `status-format[1]` reads it every 5 s. `CLAUDECODE` must be unset or the nested claude refuses. https://quickchat.ai/post/tmux-session-summaries-for-parallel-ai-agents
- `capture-pane` observer: one agent reads another's pane and turns the noise into a status, a question, or a window name. https://mattorb.com/9-agent-wins-with-tmux-and-cmux/, https://gist.github.com/jimmc414/c45d5e54b540d5782560ec99e884142a
- Injection that survives: `send-keys -l` so `#` and `!` survive, 300–500 ms, then `send-keys Enter` on its own; multiline via `load-buffer` + `paste-buffer`. Gas Town, primeline, Tmux-Orchestrator agree.
- Suppress when the human is already looking: compare the active `#{pane_id}` with `$TMUX_PANE`; `display-message` only when same session, other window.
- Window name as status: rename to what the agent is doing instead of five windows named `claude`. https://hboon.com/using-tmux-with-claude-code/ (also `allow-passthrough on` so OSC notifications reach the terminal).
- Popup as workspace and as alert: `display-popup -E` running fzf/recon/ccmux; notifynub's popup doubles as the alert, Enter jumps.
- No project pops a gum/bubbletea prompt for the agent's question. The closest are ccmux's desktop notification with inline Reply, agent view's peek, and the phone d-pads.

## Patterns

1. Three states everywhere: working / needs-input / idle. Needs-input is the only colour that matters; sorting or counting it is the feature (partner0, ccmux, agent view's `← 2 agents` footer).
2. Hooks beat scraping; scraping is the fallback keyed on Claude's footer strings; the newest tools just call `claude agents --json`.
3. "What is it doing" is the transcript tail (fzf preview, JSONL tool+path stream) or a Haiku one-liner from the Stop hook or a timer. Anthropic adopted the one-liner natively.
4. No clean channel into a busy agent. Typing into the pane with a delayed Enter, queued until idle, is the universal hack; reading is done by an observer over the transcript or the pane; forking the session is the other workaround.
5. Attention = bell to `#{pane_tty}` + `display-message` + optional desktop/OSC, suppressed when the active pane is the agent's; Enter from the popup jumps.
