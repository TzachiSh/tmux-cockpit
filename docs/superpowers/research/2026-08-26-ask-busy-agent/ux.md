# The user flow, and whether a TUI helps

From the human's seat: four situations, what happens today, what the screenshot got instead, what the flow should be after #54. Then the TUI question, since the owner asked.

## 1. Four situations

| situation | today | after |
|---|---|---|
| I'm in the pane, it's busy, I want to ask it something | type, Enter, the line queues, gets folded into the work, no answer | `/btw <q>`: answer in an overlay within seconds, the turn never stops |
| I'm elsewhere, I want to know what a pane is doing | switch to it, read the scrollback | `prefix i` on it: a `doing` row; `prefix r` fleet: the same line per row; optional `prefix ?`: ask in words |
| It needs me | red `▲` in the tab, `NEEDS YOU` border with the question, fleet count; nothing moves or sounds | plus a status message naming the window and the question, and a bell, both skipped when I'm already on that pane; `prefix u` jumps as before |
| Which of my agents needs me | `prefix u` cycles, `prefix r` lists blocked first | unchanged |

The first row is the screenshot. The second is the one cockpit can build. The third is the "maybe a popup" ask. The fourth is done already.

## 2. The screens

### `prefix i`, with the doing block

```
 info · %14 ─────────────────────────────────────────────────
 pane      %14  henosis:3 Claude Code  (claude)
 cwd       ~/.cache/cockpit-wt/lowercase-keys
 root      ~/.cache/cockpit-wt/lowercase-keys
 state     running                                  since 7m12s
 doing     Bash: shellcheck on cockpit.tmux and tests/run.sh    32s
 said      While the suite and rig run: shellcheck on the two
           scripts, and the independent review pass on the diff.
 asked     "do it review it test rig and implaments"      41m ago
 review    pending
 session   098c323f-5ffe-4309-a3dc-996d24d6f00a
 title     Implement with test review and rig
 ask it    /btw <question> in the pane answers while it works
 review dir ~/.local/state/tmux-cockpit/review/14-3187720301
           0 pinned · 2 delivered · ^A removes it
 ↵ close
```

Four rows added: `since` on `state`, `doing`, `said`, `asked`, and the one-line `ask it` pointer. Everything else is the existing screen. `since` and the tool age come from the transcript timestamps; a `doing` row older than a minute with `state running` is the "is it stuck" answer without asking anyone, with one caveat: a pending permission prompt is byte-identical to a running tool in the transcript (claude-code.md:47), so the row cannot tell stuck from waiting-on-permission and `state` remains the one that can.

### `prefix r`, the fleet row

```
 ▲ %9   henosis:1  Openvpn setup      needs   permission: Bash(sudo openvpn …)
 ● %14  henosis:3  Implement with…    Bash: shellcheck on cockpit.tmux… 32s
 ● %21  work:2     Fleet gate review  Agent: Whole-branch review of phase 1  4m
 ✓ %30  work:4     Left click issue   done 12m ago · 3 files dirty
```

The `doing` line replaces nothing; it fills the column that today shows only churn. One transcript read per row, 0.1 s each, behind the same helper as `prefix i`. Owner decision 2.

### The needs-you message (default)

Status line, 4 s, only when the active pane is not the asking one:

```
 ▲ henosis:1 Openvpn setup needs you: Bash(sudo openvpn --config …)   prefix u
```

Plus BEL on the pane's tty so the terminal tab flags it. No focus change, no popup, no sound of its own.

### The needs-you popup (opt-in, `@cockpit_needs_popup on`)

```
 ┌ needs you ───────────────────────────────┐
 │ henosis:1  Openvpn setup                  │
 │ Bash(sudo openvpn --config /etc/…)        │
 │                                           │
 │ ↵ jump   esc dismiss   closes in 10s      │
 └───────────────────────────────────────────┘
```

Raised by `run-shell -b` from the hook so the hook returns; `read -t 10` closes it; `↵` runs `tmux-jump` to that pane. It takes the keyboard from whatever the human was typing into, which is why it is opt-in and why a permission prompt should not raise it while the human is mid-word in another agent's composer. The review-flow rule ("never steal focus") was written for text going into agents; it applies to the human's own typing just as much.

### `prefix f`, the side agent, with the main agent's steps

```
 side fix · %14 ─────────────────────────────────────────────
 main is on   Bash: run the full suite and the rig   4m12s
 side fix (one line, ↵ alone cancels): fix the flaky fleet timing test
 hold off commands matching (ERE, ↵ for none): make (check|test)
```

Same two lines as today plus the `main is on` line above them, so the human splitting off a task sees what they are splitting from. The side agent's prompt then opens with:

```
What the other agent has been doing (newest last):
USER 18:27: do it review it test rig and implaments
AGENT: Setting up the fix worktree and reading the exact sites…
TOOL Bash: Create fix worktree and read the pane-path call sites
TOOL Bash: Read the fleet test block, only.sh usage, Makefile targets
AGENT: While the suite and rig run: shellcheck on the two scripts…
TOOL Bash: run the full suite and the rig

Your job: fix the flaky fleet timing test
You are a SIDE TASK … (the existing fence and HANDOVER rules)
```

The flow: main agent is on a bug, its suite is taking minutes, the human presses `prefix f`, reads the `main is on` line, types the job, Enter, Enter. The side agent starts knowing the branch, the files in play and what was just tried, without the human explaining any of it. When it finishes, the main agent gets the HANDOVER line at its next Stop, as today.

### `prefix ?`, the observer ask (optional, owner decision 5)

```
 ask %14 · Implement with… ────────────────────────────────
 ask> is it stuck or waiting on something?
 
 Waiting on a code review report it dispatched earlier. The suite
 passed (1014/0, two runs) and the rig showed no delta vs master.
 
 7s · $0.05 · ↵ close · a ask again
```

The `7s · $0.05` in the footer is the one measured run (n=1, a second pending; claude-code.md:68 flags the same for the fork spike), not a typical figure, and decision 5 turns on it. One `read -r -p`, the transcript tail through jq, one tool-less haiku, print, `read`. Same shape as `prefix f`'s two-line popup and `prefix i`'s `read -r _`. If the pane is idle the popup should say so and point at typing into it instead.

## 3. The flow, end to end

```
see it     tab ▲/●/✓  →  border NEEDS YOU + question  →  status message + bell (new)
                                                          → popup (opt-in)
know more  prefix i: state · since · doing · said · asked (new rows)
           prefix r: the doing line per row (new column)
ask it     in the pane: /btw (documented, new)
           from anywhere: prefix ? observer (optional)
split it   prefix f: side agent, now told what main just did (new)
go there   prefix u, or ↵ in the popup / fleet row
answer it  in the pane, as today
```

No step sends keys into an agent. No step needs the human to leave what they were doing to find out whether they must.

## 4. TUI: what the community uses, and what fits here (the full research with two measured spikes is in [tui.md](tui.md))

| stack | who | what it buys | what it costs |
|---|---|---|---|
| bash + fzf in `display-popup` | cockpit, craftzdog, samleeney | preview pane, keys, zero build, ships in the plugin | one-shot screens; no live refresh without `--bind reload`; fzf is a hard dependency for pickers (cockpit already has a numbered fallback) |
| `display-menu` | cockpit (`prefix a`, `M`), tmux itself | native, keyboard letters on screen, no process | static rows, no text, tmux ≥ 3.4 |
| gum (Charm) | a few dotfiles, no supervisor found | `gum input`, `gum choose`, `gum spin`, `gum format` from bash, one binary | a new dependency for what `read -r -p` and fzf do; no live view either |
| Go + Bubble Tea | claude-squad, agent-deck, agent-manager, ntm, Gas Town's `gt feed` | a live dashboard: rows refresh, preview, key handling, styling | a second language, a build, a binary per platform, its own state; each one re-implements state detection cockpit already has in hooks |
| Ink (TS) | ccmanager, aside, agenthud | same as above in Node | Node runtime, same rewrite |
| Rust | recon, claude-tmux, tmux-agent-sidebar | fast live views | same rewrite, harder build |
| a browser | claude-code-ui, Codeman, claude-view | kanban, phone | leaves tmux |

What the live TUIs actually show that cockpit's status line does not: a one-line "doing" per agent, refreshing. That is a column, not a framework. Once `tmux-review --fleet` prints the `doing` line, `prefix r` is that view, and fzf's `--bind 'ctrl-r:reload(tmux-review --fleet)'` or a `change:reload` on a timer gives it a refresh if wanted, still in bash.

Verdict: no new TUI. The three screens above are the existing popup/fzf/`read` shapes with more rows. A Bubble Tea or Ink app would be a rewrite of the plugin's UI in a second language to gain a refresh loop that fzf's `reload` already offers, and every tool that went that way ended up re-deriving agent state from pane text because their TUI could not see the hooks. Cockpit's hooks are the asset; the popups are the view. gum is the only cheap addition and it buys nothing `read -r -p` does not.

If a live dashboard is ever wanted, the shortest path is `claude agents` itself in a popup: `display-popup -E -w 90% -h 80% claude agents`. One binding, Anthropic maintains it, it has the Haiku summary and the peek. It is machine-wide, not pane-aware, so it sits beside `prefix r`, not instead of it. Worth a row in the `prefix a` menu for the price of one line.

## 5. Why the screenshot's flow failed, in one line each

- The prompt box does not say that a queued line will be treated as work; the only hint is the tip that rotates under the spinner. Cockpit cannot change the box, so the pointer goes on `prefix i` and in KEYS.md.
- Nothing in tmux showed what the agent was doing beyond its scrollback; the `doing` row is that.
- Nothing said how long it had been on the current tool; `since` is that.
- Asking from outside the pane had no path at all; `prefix ?` is that, if the owner wants to pay for it.
