# Source reads: how the tmux-native tools do it

Shallow clones, 2026-08-26, read for four mechanisms: how a pane is joined to a session, what "doing" is built from, how the human is told, how (if ever) text goes into a busy pane. All MIT unless noted. None of the eight tmux-native ones calls a model for a summary; the ones that do are in the second half.

## Pane → session

| tool | join | notes |
|---|---|---|
| tmux-claude-session-manager | `claude agents --json` → `pid`; `ps -Ao pid=,tty=` → `list-panes -a -F '#{pane_tty} #{pane_id}'`, one awk (`scripts/agents.sh:20-44`) | no hooks; transcript found by glob `projects/*/<sid>.jsonl`, mtime only |
| recon | `list-panes -a -F '#{pane_pid}…'`, then `~/.claude/sessions/<pid>.json` gives `sessionId` (`session.rs:1194-1282`); resumed sessions via `ps -o args=` `--resume` and `lsof -p` on the largest open `.jsonl` (`:879-929`) | no hooks |
| ccmux | SessionStart hook writes `session-pids/claude-<sid>.json` with `pid:$PPID` and `tty` from `ps -p $PPID -o tty=` (`hook-scripts.ts:21-37`); pane by `tty == #{pane_tty}` (`session-pane-match.ts:128-147`) | cwd read from the transcript head `.cwd` (`parser.ts:200-207`) |
| tmux-agent-indicator, tmux-notify, cc-notifynub, tmux-agent-status | `$TMUX_PANE` in the hook env | what cockpit does |
| agent-deck | owns the session it spawns; `AGENTDECK_INSTANCE_ID` via `set-environment` (`instance.go:1461`) | hook payload incl. `transcript_path` written to `hooks/<id>.json` (`hook_handler.go:349-379`) |

For cockpit: `@cockpit_session` when set; otherwise craftzdog's join, one `claude agents --json`, one `ps`, one `list-panes`, no pid tree walk.

## "What is it doing"

| tool | source | what it reads |
|---|---|---|
| ccmux | transcript state machine (`status-machine.ts:153-458`) | `content[].tool_use.name`: `AskUserQuestion` → waiting:question, `ExitPlanMode` → waiting:plan, `Task` → subagent, first tool → `pendingTool`; `stop_reason` end_turn → idle; user string → working + `lastPrompt`; `system.subtype` turn_duration → idle. Display text = assistant text blocks joined, skipping `isMeta` and `<command-*>`/`<local-command-*>` prefixes (`transcript-readers/claude.ts:38-91`). **A permission-gated `tool_use` is not in the JSONL until approved** (`status-machine.ts:244-254`, verified on 2.1.214): the transcript never shows "waiting for permission"; only hooks or the pane do |
| marmonitor | transcript tail (`src/scanner/claude.ts`) | assistant `stop_reason: tool_use` with no following `tool_result` → waiting on a tool; no `stop_reason` → thinking; `end_turn` → done; `hook_progress` → tool. Hook-free |
| claude-code-ui | chokidar tail (`packages/daemon/src/summarizer.ts`) | while `working`: no model, `getWorkingSummary` = last `tool_use` name + input ("Editing src/foo.ts"); at rest: one sonnet call, 100 tokens, "5-10 words, mention files", cached on the hash of the last 5 timestamps |
| aside (`vignesh07/aside`) | tails `~/.claude/projects/**/*.jsonl` (`src/core/session-tailer.ts`), no hooks | roster line per session (project, branch, status, quiet-for, context, last activity) + recent transcript under 24,000 chars (`world-view.ts`); `AskUserQuestion` at the tail → `needs_input` with the question text (`claude-classifier.ts:189`) |
| agenthud | JSONL tail (`sessionLiveness.ts`) | pending `tool_use` at the tail = working, else waiting; tool + path stream |
| recon | pane scrape (`session.rs:1124-1171`) | last line "Esc to cancel" → Input; a line starting with `✻…⏺·` and containing "…" → Working; "❯" + digit → Input; else Idle. JSONL only for model and token counts |
| agent-deck | pane scrape (`detector.go:146-450`) + `#{pane_title}` spinner glyphs (`title_detection.go:244-310`) | busy on "esc to interrupt"; waiting on "Yes, allow once", "│ Do you want", "Esc to cancel", "❯ Try "; error on "API Error: 401"; background shells keep it running on `\d+ shells? still running` (`background_work.go:21-24`). The one JSONL read is a `===AGENTDECK_DONE===` sentinel in the last assistant text |
| dmux | `capture-pane -p -J -S -N` every 1 s (`PaneWorker.ts`) | regex `esc to (interrupt|cancel|stop|abort)` = working, no model; when the pane stops changing ≥ 5 s, `PaneAnalyzer.ts` sends the last 50 lines to a cheap model with a three-stage prompt: state `option_dialog / open_prompt / in_progress`, then the question and its keys, then `summary` + an `attention_body` under 140 chars that becomes the desktop notification |
| claude-tmux-titles | Stop hook → python over `transcript_path`, last 3 prompts (skips `isMeta`, `isSidechain`, `<system-reminder>`) → `env -u TMUX claude -p --model haiku "…2 or 3 lowercase words"` backgrounded → `rename-window` + `@auto_title`, 15 s throttle, manual rename respected | |
| tmux-ai-window-name | `after-select-window` hook → `capture-pane -p` every pane in the window, hashed → `claude -p --model haiku --no-session-persistence --tools '' --disable-slash-commands` → `rename-window` | |

For cockpit: the `doing` line is the claude-code-ui/agenthud read (last `tool_use` name + description, last text), plus marmonitor's `stop_reason` rule for "since". Permission waits stay on the hook path cockpit already has.

## The observer that answers questions

`aside` (`src/core/providers/claude-session.ts`, `claudeObserverArgs`): one long-lived `claude -p --input-format stream-json --output-format stream-json --verbose --safe-mode --tools '' --append-system-prompt <prompt> --model <m>` per side thread; each question goes in on stdin as `${transcript excerpt}\n\n---\n\nQuestion: ${q}`. System prompt (`side-chat-engine.ts:13`): "You are 'aside' — a read-only observer… They are asking about their agents WITHOUT interrupting or steering them… You are READ-ONLY. You have no tools… distinguish 'waiting for input' from 'stalled mid-task'". It never `--resume`s the agent's session. Threads persist in `~/.aside/threads.json`. This is the spike in claude-code.md, one-shot instead of long-lived.

The official docs' version is `claude -p --resume <id> --output-format json "summarize what we changed"` (sessions doc, "access conversations from scripts") with the caveat that two processes on one id interleave into one transcript, hence `--fork-session` for a live agent. `claude-mem` (91.9k) is the same observer shape fed by hooks through the Agent SDK, writing memories rather than answering.

## Telling the human

| tool | mechanism | suppressed when | jump |
|---|---|---|---|
| tmux-notify | bell `printf '\a' > #{pane_tty}` (`tmux-notify.sh:154`); opt-in `display-message -l "Claude [win]: done/waiting"` only when same session, other window (`:170-179`); opt-in `select-pane` after 250 ms, same window only | active `#{pane_id}` == `$TMUX_PANE` (`:206-208`) | none |
| tmux-agent-indicator | `display-message -d 5000 "[claude] needs-input (sess:win)"` (`agent-state.sh:301-321`), format option, optional command hook with `AGENT_*` env; border/window styles only for non-active panes (`:413-486`); styles reset on `pane-focus-in` hooks | styling yes, the message no | none |
| cc-notifynub | `display-popup -x R -y T -w 34 -h 7 -b rounded -e NOTIFY_STATE=… -E "bash notify.sh --render"` (`notify.sh:144-153`); `read -t` closes after 5 s (done) / 10 s (permission); Enter → `select-window -t :<idx>` | `#{window_active}` == 1, except permission prompts always show (`:135-138`) | Enter. **`display-popup -E` blocks the hook process until the popup closes** |
| tmux-claude-session-manager | `set-hook -g alert-bell "run-shell -b bell.sh"` re-emits BEL into the origin pane's tty so `window-status-bell-style` lights (`bell.sh:28-29`) | none | fzf Enter → `switch-client; select-window; select-pane` |
| tmux-agent-status (samleeney) | Notification hook → `paplay`/`afplay`/`\a` (`play-sound.sh:36-82`); no message, no popup | none | `prefix N` → next done project |
| ccmux | OS notifications: `osascript display notification`, `notify-send --app-name=ccmux`, D-Bus with Approve/Deny/Reply buttons, or OSC 777/9 to `#{pane_tty}` (`lib/notify.ts:398-502`); 1 s debounce, 60 s per-(session,kind) cooldown; stale banners retracted when viewed | active pane == session pane AND terminal frontmost, macOS only (`notifier.ts:468-483`) | click → `switch-client -c <client_tty> -t <pane>` (`notify-jump.ts:53-80`) |
| agent-deck | `cmux notify` / `notify-send` / `osascript`, waiting and error only, edge-triggered, no cooldown (`desknotify.go:60-150`) | none | none |
| recon | nothing | | `prefix i` → first Input session |
| dmux | macOS notification with the model-written `attention_body` | | |

For cockpit: tmux-notify's suppression rule (active pane == the asking pane) and indicator's `display-message -d` are the two lines worth taking. A popup must be raised with `run-shell -b` so the Notification hook returns; notifynub's Enter-jumps and `read -t` auto-close are the shape.

## Typing into a busy pane

Only ccmux, agent-deck and Gas Town do it, all with the same guards:

- ccmux `pane-io.ts:103-199`: `send-keys -t <pane> -l -- <text>`, 150 ms, `send-keys Enter`; multi-line via `load-buffer -b <name> -` + `paste-buffer -p -b <name> -d`, 150 ms, Enter. Refused unless `session.status === "idle"` (`server.ts:3433-3471`); busy targets are queued (`HandoffQueue`, 30 min TTL, 3 attempts, swept every 60 s) and delivered on idle. A leading `/` or `!` gets a space. Approve/deny from a notification sends key `1` / `Escape`, 30 ms between keys, then `capture-pane` must show the prompt gone (`notification-action.ts:71-79, 628-665`).
- agent-deck `send/ready.go:37-110`, `send/guard.go:165-240`, `tmux.go:5455-5700`: poll status every 200 ms until it left running and the composer prompt is visible, 300 ms; an existing draft is saved, `Ctrl+C`'d, re-typed after; `send-keys -l -- <chunk>`, 100 ms, Enter; arrival verified by `capture-pane`, 50 × 300 ms. A nudge into a busy pane is refused: "send-keys into a busy pane only queues the keystroke" (`inbox_nudge.go:11-41`).
- Gas Town `internal/tmux/tmux.go`: sanitise (strip ESC/CR/BS), `send-keys -l` in 512-byte chunks (the Linux tty canonical buffer is 4096), 500 ms + 25 ms per chunk, Escape first if the pane shows a rewind dialog, `sendEnterVerified` (snapshot 5 lines, Enter, re-send at 500/1000/2000 ms if unchanged), a flock per session. Default mode waits for idle (`capture-pane` last 5 lines: "esc to interrupt" absent, `❯ ` or `⏵⏵` present, two polls 200 ms apart), 15 s timeout, then a file queue drained by the UserPromptSubmit hook.

Cockpit's `--deliver` already does the `load-buffer` + `paste-buffer -p -r -d` half, with the idle check at paste time and the spool for busy panes handed over as Stop-hook `additionalContext` (review-gate:176-202). That is the same design as ccmux's queue and Gas Town's drain, minus the Enter, which is the rule. Nothing here argues for changing it.

## Also seen

ccbot (Telegram ↔ tmux, `session_monitor.py` tails `~/.claude/projects` via `sessions-index.json`, input back by `send_keys` literal + delay + Enter), hiroppy/tmux-agent-sidebar (Rust sidebar pane on 14 hook events, shows prompts, tool calls, wait reasons, subagent trees), MaxGhenis/tmux-claude-code (`cc detect`, `cc resume <keywords>` searches transcripts), tombelieber/claude-view (hook plugin dashboard), subinium/agf and five transcript browsers. Claude Code issues showing the demand: #48099 (read-only side chat from a selection), #29224 (side-channel replies to queued messages, closed not planned), #58965 (agent view says working during a permission prompt).

Clones: `scratchpad/src/{aside,ccmux,dmux,gastown,claude-tmux-titles,tmux-ai-window-name,claude-code-ui,agenthud,marmonitor,ccbot,ccmanager,claude-mem,tmux-agent-sidebar,claude-view,tmux-claude-session-manager,tmux-agent-indicator,tmux-agent-status,agent-deck,recon,claude-extensions,cc-notifynub}`, session-local, gone with the scratchpad.
