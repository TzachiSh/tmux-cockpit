# Reviewing an AI agent's diff: landscape, mechanics, and what tmux-cockpit should steal

Research date: 2026-08-24

**Bottom line up front:** this problem has been solved three times in the last year, and the
best solution is terminal-native and already speaks tmux. **[revdiff](https://github.com/umputun/revdiff)**
(umputun, MIT, Go) is a TUI diff reviewer that launches inside a `tmux display-popup`, lets you
annotate any line, and prints structured `## file:line (+)` annotations to **stdout on quit** —
which the calling agent reads as its tool result. **[difit](https://github.com/yoshiko-pg/difit)**
(MIT, Node) does the same via a local web UI plus an HTTP endpoint. The round-trip problem is
solved; what nobody has solved is **review across a fleet of agent panes**, which is exactly
tmux-cockpit's territory.

## (a) Landscape

| Tool | What it is | Comment → agent mechanism | License | Terminal-friendly |
|---|---|---|---|---|
| **revdiff** | Go TUI, two-pane tree+diff, annotate any line | Structured markdown to **stdout on quit**; exit code 10 = annotations captured | MIT | **Yes — native tmux popup** |
| **difit** | Local web server, GitHub-style "Files changed" | `GET /api/comments-output` printed to stdout on SIGINT; also "Copy Prompt" button | MIT | Launches from CLI, reviews in browser |
| **hunk** (modem-dev) | OpenTUI "review-first terminal diff viewer for agent changesets" | `hunk session comment add/list/apply`; local server agents connect to | MIT | Yes |
| **Conductor** | macOS parallel-agent app | In-process **MCP server** (`createSdkMcpServer`) lets Claude comment on lines; human selects comments → "add to chat" | Free, closed | No |
| **Plannotator** | Local review server for plans *and* diffs | **Hook-based**: `ExitPlanMode` → `PermissionRequest` hook → browser UI → structured feedback into session | Apache-2.0/MIT | Hook-launched, browser UI |
| **Orca** | Web diff annotation | Comments pinned to lines, batched, "Send to agent" | Commercial | No |
| **VS Code + Copilot** | Native "Changes" view | Select range → "Add Feedback" → **"Submit Feedback"** (explicit batch) | Proprietary | No |
| **claude-code-feedback** (VS Code ext) | Diff comment threads | **`sendText` terminal injection** into the Claude Code pane | MIT | Injects *into* a terminal |
| **Cursor** | Agent review panel | Only "Add to Chat" on a selection; **batch line comments not shipped** | Proprietary | No |
| **Zed** | Agent diff review | Accept/reject only; PR-style comments **proposed, not shipped** (#59157) | GPL/proprietary mix | No |
| **Claude Code `/code-review`** | Built-in review subagent | `--fix` applies to working tree; `--comment` posts inline PR comments | Proprietary | Yes |
| **git-appraise** | Distributed review in git notes | Per-line JSON in `refs/notes/devtools/discuss` | Apache-2.0 | Yes, but **stale since 2023** |
| **lazygit / tig** | TUI git clients | No native comments; `customCommands` / `bind` can shell out | MIT / GPL-2.0 | Yes |

## (b) The four round-trip mechanisms that actually work

### 1. Blocking launch + structured stdout (the best one)

The agent runs the reviewer as a foreground command; it blocks; the human annotates; on quit the
annotations print to stdout and land in the agent's tool result. No polling, no IPC, no MCP
server. revdiff's grammar, from `app/annotation/parse.go`:

```
^## (.+?)(?::(\d+)(?:-(\d+))?)? \((file-level|\+|-| )\)$
```

producing records like:

```
## handler.go (file-level)
consider splitting this file into smaller modules

## handler.go:43 (+)
use errors.Is() instead of direct comparison

## handler.go:43-67 (+)
refactor this hunk to reduce nesting

## store.go:18 (-)
don't remove this validation
```

The parser is the exact inverse of the formatter, so the same file round-trips back in via
`--annotations=<file>` — an agent can *preload* its own findings and the human annotates on top.
Exit code `10` means "annotations captured" (vs `0` = clean/discarded, `1` = real error).

difit implements the same idea over HTTP: its SIGINT handler does
`fetch('http://localhost:PORT/api/comments-output')` and prints `path:L10 (old)` /
`path:L15-L25` blocks, with a `Total comments: N` footer.

### 2. Background server + HTTP harvest

`difit --background` detaches and prints `{"port":4966,"url":"...","pid":12345}` as JSON on
stdout. From then on anything can `curl localhost:4966/api/comments-output` to harvest comments,
or `POST /api/comment-imports` to inject them. This decouples "review is open" from "agent is
blocked" — the right shape when a review outlives a single agent turn.

### 3. Terminal text injection

The VS Code `claude-code-feedback` extension formats comments with `@file:line` refs plus the
diff hunk and types them into the Claude Code terminal via `sendText`. That is `tmux send-keys`
by another name, and it's the only mechanism that works when the agent is *already mid-session*
rather than blocked on a tool call.

agent-mux does the durable version: payloads over 2KB (`TMUX_AGENT_INLINE_THRESHOLD`) go to a
file under `$XDG_RUNTIME_DIR/threads/` and the pane receives only a compact ping
(`[tmux-agent v1 kind=thread thread=<id> seq=<N> from=<agent> pane=<%N> reply=<%N>]`) — the
agent reads the body on demand via `tmux-agent thread read <id>`, keeping the prompt small.

### 4. Hook interception

Plannotator intercepts `ExitPlanMode` via a `PermissionRequest` hook, opens a review UI, and
returns structured feedback as the hook's decision. revdiff's planning plugin does the same on a
`Stop` hook. This is how review becomes *automatic* rather than something the human must
remember to ask for.

## (c) What a tmux+fzf-native flow should borrow

### From revdiff — nearly everything, starting with the launcher

Its tmux path is eight lines you can lift directly (`scripts/launch-revdiff.sh`):

```bash
TMUX_ARGS=(tmux display-popup -E -w "$POPUP_W" -h "$POPUP_H")
# -T (title) requires tmux 3.3+
TMUX_ARGS+=(-T " $OVERLAY_TITLE ")   # "rd: dirname [ref]"
TMUX_ARGS+=(-d "$CWD" -- sh -c "$REVDIFF_CMD")
```

`display-popup -E` blocks until the command exits, which is precisely what makes mechanism #1
work. Three hard-won details in that script are worth copying outright:

- Popup children inherit a **server-era environment**, so `EDITOR`/`VISUAL` must be re-injected
  via an `/usr/bin/env KEY=VAL` prefix or multi-line annotation editing breaks.
- `-T` needs a tmux **≥3.3 version check** (regex on `tmux -V`).
- `REVDIFF_TMUX_WINDOW=1` swaps the popup for a **server-owned tmux window**, because a dropped
  SSH client tears down a client-owned popup and kills the review mid-flight. For a plugin whose
  whole premise is long-lived agent panes, that should probably be the default, not the opt-in.

Also: on signal termination (SIGHUP from a dropped client, or SIGTERM) revdiff saves annotations
to `~/.config/revdiff/history/<repo>/` — never to the `-o` output — so a cut-short review is
still recoverable.

### Adopt revdiff's annotation grammar verbatim

Don't invent one. It's plain markdown, human-editable, agent-parseable, round-trippable, and
already in the training distribution of every agent that has seen the repo. Two subtle touches
worth keeping:

- A body line starting with `## ` gets a **leading space** on output so parsers can't mistake it
  for a record header.
- An annotation containing the word "hunk" (case-insensitive, whole word) **auto-expands its
  header to the full hunk range** (`handler.go:43-67` instead of `handler.go:43`), so the agent
  gets range context for free.

### Steal the `??` convention

An annotation containing `??` anywhere (or opening with `explain`, `remind`, `describe`,
`what is`, `what are`, `how does`, `how do`, `clarify`) is a *question*, not a directive — the
agent explains instead of editing, and the answer comes back as a markdown doc reopened for
follow-up annotation. Code-change annotations from the same batch are held and applied after the
explanation loop finishes. One parse rule, roughly double the tool's usefulness, and `??` is
language-neutral.

### Steal difit's harvest endpoint shape for the fleet case

A background reviewer per pane on a known port, plus `curl .../api/comments-output`, is how you
express "review pane 3 while pane 5 keeps working" — something a blocking popup fundamentally
cannot.

### From Orca and Copilot, the batching discipline

Orca's docs state the failure mode plainly: *"Sending comments one at a time causes the agent to
swing back and forth. Batching keeps the feedback coherent: one round of thinking, one revision
pass, and a much higher hit rate."* Copilot enforces this structurally — nothing reaches the
agent until you click "Submit Feedback." So accumulate annotations and deliver once; don't send
on each keystroke.

### From Conductor, agent-authored comments as the starting state

Its MCP tool lets Claude annotate its own diff before the human opens it. difit exposes the same
thing without MCP, via repeatable:

```bash
difit . --comment '{"type":"thread","filePath":"src/x.ts","position":{"side":"new","line":10},"body":"..."}'
```

`position.line` also accepts `{"start":36,"end":39}` for ranges; `type` is `thread` or `reply`.
Opening a review pre-seeded with "here's what I was unsure about" is a large usability win over
an empty diff, and it costs one CLI flag.

### On fzf specifically

`forgit` and `fzf-git.sh` are the reference implementations for fzf+delta diff previews (note
`FORGIT_PREVIEW_PAGER`, and the known issue that delta's side-by-side mode renders squished
inside an fzf preview pane — wfxr/forgit#121, #471). But fzf is a *picker*, not an annotator —
the natural split is fzf to choose the pane/file/hunk, and a popup for the actual line
annotation.

If you want annotation without writing a TUI, **tig is the only TUI here with a native
line-number variable** exposed to shell binds:

```
bind blame X !sh -c 'myScript.sh %(file) %(lineno)'
bind main  e !vim +%(lineno) %(file)
```

lazygit's `customCommands` gives you free-text prompts but only file granularity:

```yaml
customCommands:
  - key: 'C'
    context: 'files'
    command: 'echo "{{.SelectedFile.Name}}: {{.Form.Comment}}" >> review.md'
    prompts:
      - type: 'input'
        title: 'Review comment'
        key: 'Comment'
```

`{{.SelectedFile.Name}}` is file-level; there is no `SelectedLine`. gitui has no arbitrary
shell-command hook at all.

### From the community wisdom, three design constraints

- Addy Osmani: *"A diff a human can actually read is now a design constraint, not a courtesy."*
  And his sharpest heuristic: *"Read the test changes more carefully than the code. This is the
  agent failure mode to watch. The agent changes behavior, then 'fixes' the test by rewriting
  the assertion to match the new, broken behavior."* A reviewer that surfaces test-file changes
  first would be genuinely differentiated.
- The Approval Fatigue pattern names the enemy: *"rejection has high friction... approval takes
  one keystroke."* Make "annotate" as cheap as "accept."
- A diff shows *what* changed but not *why*. revdiff's answer is `--description` /
  `--description-file` feeding an info popup (`i`), which for a pane-monitoring plugin could be
  auto-populated from the agent's own turn summary.

## (d) Gaps nobody has filled — the opportunity

Every tool above reviews **one diff, in one repo, for one agent**. tmux-cockpit already knows
about *many* Claude Code panes, and that axis is unclaimed:

1. **Fleet review triage.** No tool answers "which of my six agents produced a diff that needs
   my eyes?" A review queue keyed by pane, sorted by diff size or test-files-touched, is open
   territory. It composes naturally with the NEEDS-YOU signalling the plugin already has —
   "needs review" is just another attention state.
2. **Delivery to an already-running pane.** revdiff and difit both assume *the agent called the
   reviewer* and is blocked waiting. If a human spontaneously decides to review pane 3, there's
   no tool call to return into — you need `tmux send-keys`, and the durable version (write
   annotations to a file, send a short pointer) is what agent-mux does for handoffs but nobody
   does for review. **This is the single most tmux-native thing to build.**
3. **Review that survives the agent.** difit persists comments in browser localStorage; revdiff
   auto-saves to `~/.config/revdiff/history/`. Neither is addressable per-pane or per-worktree.
   Annotations in `refs/notes/` (git-appraise's design, sound even though the tool has been dead
   since 2023) would survive pane death, worktree removal, and agent restarts — and merge
   without conflicts via git's `cat_sort_uniq` notes strategy.
4. **Reviewing the working tree while the agent is still writing it.** Every tool assumes a
   quiescent diff. An agent mid-turn mutates files under the reviewer and line anchors drift.
   Orca claims line-tracking across edits; nobody in the terminal space does. Even detecting the
   race and warning would be new.
5. **Nobody has wired lazygit/tig to a Claude pane.** Despite both being trivially capable of it,
   no published config does this. Low-hanging fruit for a zero-dependency path alongside a popup
   reviewer.

**Recommendation:** don't write a diff TUI. Shell out to revdiff (with difit as the
browser-preferring alternative), adopt its annotation grammar as the wire format, and build the
two things only a tmux plugin can: the **fleet-wide review queue** and **durable delivery into a
live pane**. That's a week of work instead of a quarter, and it lands in the gap rather than the
crowd.

## (e) Sources

### Primary

- https://github.com/umputun/revdiff — README, `app/annotation/parse.go`,
  `.claude-plugin/skills/revdiff/SKILL.md`, `scripts/launch-revdiff.sh`, `references/usage.md`
- https://github.com/yoshiko-pg/difit — README, `src/cli/index.ts`, `src/cli/background.ts`,
  `src/server/server.ts`
- https://github.com/yoshiko-pg/difit/blob/main/skills/difit-review/SKILL.md
- https://www.hunk.dev/docs/agents/comments-and-annotations/
- https://github.com/google/git-appraise
- https://github.com/maxto/agent-mux
- https://github.com/iliasoroka1/rz
- https://code.claude.com/docs/en/code-review
- https://github.com/anthropics/claude-code-action
- https://github.com/jesseduffield/lazygit/blob/master/docs/Custom_Command_Keybindings.md
- https://jonas.github.io/tig/doc/tigrc.5.html
- https://github.com/backnotprop/plannotator
- https://conductor.build/blog/diff-tools
- https://github.com/zed-industries/zed/issues/59157
- https://forum.cursor.com/t/allow-commenting-diffs-when-reviewing-changes/163482
- https://code.visualstudio.com/docs/copilot/chat/review-code-edits
- https://github.com/wfxr/forgit
- https://github.com/junegunn/fzf-git.sh

### Discussion and wisdom

- https://news.ycombinator.com/item?id=47742437 — Show HN: Revdiff
- https://news.ycombinator.com/item?id=48166275 — Show HN: Codiff
- https://news.ycombinator.com/item?id=46619855 — Ask HN: Are diffs still useful for AI-assisted code changes?
- https://news.ycombinator.com/item?id=45427697 — Show HN: Sculptor
- https://addyosmani.com/blog/agentic-code-review/
- https://aipatternbook.com/approval-fatigue
- https://www.onorca.dev/docs/review/annotate-ai-diff
- https://simonwillison.net/2025/Oct/7/vibe-engineering/
- https://blog.vincentqiao.com/en/posts/claude-code-diff/
- https://codex.danielvaughan.com/2026/05/24/human-review-bottleneck-code-review-strategies-agent-output/
- https://lucumr.pocoo.org/2025/6/12/agentic-coding/
