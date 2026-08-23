# tmux-cockpit

Shows what every AI coding agent in your tmux session is doing, without you
having to switch windows to find out — then lets you act on what you see without
leaving the pane. Ctrl+click the branch in the status bar to switch branch.
Ctrl+double-click any line of output to pick a path off it and open that file,
at the line it names.

```text
 claude-main ●    claude-2 ▲    coder ○
─ NEEDS YOU 2:claude-2 ────────────────────────────────────────
```

| state | marker | colour | means |
|-------|--------|--------|-------|
| blocked | `▲` | red | waiting on **you** — a permission prompt |
| error | `✕` | orange | a tool failed |
| running | `●` | yellow | working |
| done | `✓` | green | finished, you have not picked it back up |
| idle | `○` | grey | no agent, or nothing happening |

Shape carries the state as well as colour, so it survives a greyscale
screenshot and a red/green colourblind reader.

## Window labels

For an agent pane the tab shows the **conversation topic**, not the window name.
Claude Code writes it to `pane_title` (the same thing Ghostty shows in its tab),
so with several agents running you see what each is working on instead of three
tabs all reading `claude`:

```text
 Color scheme for code … ●     Openvpn setup with voi… ○     build ○
```

Non-agent panes fall back to the window name.

## Where it shows up

- **Window tabs** — one marker per window. Worst state in the window wins, so a
  blocked pane is never hidden behind a busy one.
- **Pane border** — the whole top border takes the state colour. Colour is
  pre-attentive: you see which pane needs you without reading anything.
- **Active pane's border** — names any *other* window that is blocked, since
  you cannot see its border from here.

Nothing is drawn when nothing is happening.

## How it works

Claude Code hooks call `scripts/agent-state`, which writes a per-pane tmux
option:

```sh
tmux set-option -p -t "$TMUX_PANE" @panestate running
```

Everything you see is then plain tmux format syntax in `~/.tmux.conf` reading
that option. **A status redraw runs no shell at all.** The option dies with its
pane, so there is no stale state and nothing to garbage collect.

### State ranking

`needs-input` > `error` > `running` > `done`

A lower state may not overwrite a higher one. Only work actually resuming
(`running`) or an explicit `off` clears them. This exists because `Stop` fires
when a *blocked* turn ends — with the permission prompt still on screen — and
letting that write `done` turned a red "answer me" into a green "finished" on a
timer.

## Install

```sh
git clone <this repo> ~/.tmux/plugins/tmux-cockpit
~/.tmux/plugins/tmux-cockpit/install.sh    # symlinks tm and the two tmux-* commands into ~/.local/bin
```

Add to `~/.tmux.conf` (see `examples/tmux.conf` for the full block):

```tmux
run-shell ~/.tmux/plugins/tmux-cockpit/cockpit.tmux
```

That one line also binds the keys below — there is nothing else to add for them.
It must come **after** every option using an `#{agent_*}` token, and before tpm.

If the path contains a space, quote it for the shell *inside* the tmux string —
tmux hands the line to `sh`, which would otherwise split it and fail with 127:

```tmux
run-shell "'~/my plugins/tmux-cockpit/cockpit.tmux'"
```

`install.sh` only ever creates symlinks, skips a name already taken by a real
file or by another tool's link, reports anything it could not create, and
`--uninstall` removes only links that point back into the plugin.

### Or with tpm

It is a normal tpm plugin — an executable `*.tmux` at the repo root — so:

```tmux
set -g @plugin 'catppuccin/tmux'          # if you use it: declare it FIRST
set -g @plugin '<owner>/tmux-cockpit'
run '~/.tmux/plugins/tpm/tpm'             # last
```

tpm is the better route, because it runs plugins from that last line — after
every `set -g` in your config, which is exactly the ordering the `#{agent_*}`
tokens need. The one failure mode of the `run-shell` line, loading it too early
and getting the raw token printed in your status bar, cannot happen under tpm.

Order matters against catppuccin specifically: cockpit rewrites
`@catppuccin_directory_text` and `@catppuccin_gitmux_text`, so catppuccin has to
be declared first or it resets them afterwards.

Wire the hooks in `~/.claude/settings.json`:

| hook | state |
|------|-------|
| `SessionStart` | `off` — a resumed pane must not inherit old state |
| `UserPromptSubmit` | `running` |
| `PermissionRequest` | `needs-input` |
| `PostToolUse` | `running` |
| `PostToolUseFailure` | `error` |
| `SubagentStart` | `running` |
| `PreToolUse` | `running` — marks the pane busy as a tool begins |
| `Stop` | `done` |
| `Notification` | `notify` (parses the JSON on stdin) |
| `SessionEnd` | `off` |

## Requirements

- **tmux >= 3.3** for everything except one gesture. Measured against real 3.4
  and 3.7c binaries:

  | version | what you get |
  |---------|--------------|
  | 3.3 | the floor — `display-popup -T`, which both pickers use |
  | 3.4 | per-pane border colours, the markers, both pickers, `prefix + b` |
  | 3.7 | adds `Ctrl`+click on the branch segment (`#[range=control\|N]`) |

  Below 3.7 the plugin says so once and skips that one binding; `prefix + b`
  opens the same picker. Below 3.3 it says so and stops.

  An earlier version of this file claimed 3.5 was the floor because per-pane
  border colour "silently does nothing" on 3.4. That was wrong — it works.
- Claude Code (or any agent that can run a command on lifecycle events).
- A Nerd Font is *not* required. The markers are plain Unicode geometry.

## Click the branch to switch branch

| key | does |
|-----|------|
| `Ctrl`+click the branch segment | branch picker |
| `prefix + b` | the same picker, without the mouse |

Both pickers share their keys: `↵` acts, `esc` cancels, `^/` toggles the
preview and `^U`/`^D` scroll it.

Most-recent branch first, `alt-↵` creates a branch named by what you typed, and
a remote-only branch becomes a local tracking branch. It is `alt-↵` and not
`^N`, which is fzf's own move-down: as a create key it meant navigating with
emacs keys left the picker, and any filter you had typed became a branch name. When git refuses — a dirty
tree, or a branch already checked out in another worktree — you get git's own
message, naming the files or the worktree, and the popup waits for a key.

The plugin binds the key but **cannot mark the click target**: `status-right`
belongs to your config, not to the plugin. Wrap whatever shows the branch:

```tmux
set -ag status-right "#[range=control|1]#{E:@catppuccin_status_gitmux}#[norange]"
```

`range=control|1` triggers `MouseDown1Control1`, a key of its own.
`range=user|X` would have taken over `MouseDown1Status` — the key that selects a
window when you click the window list.

## Ctrl+double-click a line to pick a path off it

**Ctrl+double-click** any line of pane output. Every whitespace-separated token
on that line that resolves to a real file becomes a candidate, and a picker
popup opens with the list and a preview of the spot — the same popup shape the
branch picker uses. Enter opens the highlighted one, Esc cancels.

The picker opens even when the line yields exactly **one** path. Seeing what the
line resolved to, with the file and line in front of you, is the point of the
gesture; opening on a single guess is how you end up in the wrong file. When the
line yields nothing, the status line says so and no popup appears.

It reads `file:12:3`, `file:12`, `file(12,3)` (tsc), `a/file` (git diff),
`~/file` and absolute paths, and strips the quotes, brackets and trailing commas
terminals wrap paths in. Existence on disk is the filter, so a word that merely
looks like a path is dropped.

### The snippet

The preview pane is the reason to look before you leap, so it shows the thing
you are actually deciding about:

| what you picked | what you see |
|-----------------|--------------|
| a file, with a line number | the code around that line, the line highlighted |
| a file, no line number | the file from the top |
| a directory | `git status --short` and a diff stat for it, or `ls -la` outside a repo |
| a binary | its type and size, never its bytes |

With [`bat`](https://github.com/sharkdp/bat) installed (`batcat` on Debian) the
snippet is syntax-highlighted and carries the **git gutter** — `+` and `~`
beside every line you have changed but not committed — so you see the code and
what moved in it at the same time. Without `bat` it falls back to numbered
lines with the target in reverse video. Nothing else is required.

`tmux-open-path --preview <path> [line]` renders exactly what the popup shows,
which is the quickest way to see why a preview looks wrong.

It reads the whole clicked **line**, not `#{mouse_word}`: tmux's
`word-separators` include `/ . : -`, so the word under `src/foo-bar.ts:31` is
`foo`.

### Why Ctrl+double-click

Because it is the gesture that actually arrives. Measured against Ghostty 1.3,
tmux receives plain clicks, `Ctrl`+click, double-clicks and `Ctrl`+double-clicks
— but **never** `Alt`+click, which the terminal strips to a plain click, and
never `Shift`+click, which it keeps for its own selection. An `Alt`-only binding
cannot fire there at all.

Of the gestures that do arrive, `Ctrl`+double-click is the only one tmux does
not already own: plain `Ctrl`+click is `swap-pane`, and plain double-click is
select-word-and-copy. Taking either would cost a feature people use daily. It
also pairs with the branch picker, which is `Ctrl`+click on the status bar.

`M-MouseDown1Pane` stays bound as well, for terminals that *do* forward
`Alt`+click (xterm, kitty, WezTerm, foot), where it is the lighter gesture.
Binding both costs nothing — whichever one your terminal delivers is the one
that fires.

If neither gesture does anything, check what your terminal actually sends:

```sh
tmux bind -n C-DoubleClick1Pane run-shell -b "echo fired >> /tmp/probe"
: > /tmp/probe   # then Ctrl+double-click a pane and look at /tmp/probe
```

Opens in VS Code at the line. For a terminal editor instead:

```tmux
set -g @cockpit_editor nvim   # new window next to the one you clicked, at +LINE
```

Where neither `@cockpit_editor` nor `code` is available it falls back to `$VISUAL`
or `$EDITOR`, and says so plainly if there is no editor at all rather than
reporting a success that never happened.

## tm — session launcher

```sh
tm <name>     attach to <name>, creating it if missing
tm            pick from a session list; if none exist, ask for a name
tm -k [name]  kill sessions (no name = pick them, Tab selects several)
```

In the picker: Enter attaches, `k` kills the highlighted session, `/` switches to
type-to-search. It uses `switch-client` inside tmux and `attach-session` outside,
and checks `session_attached` rather than `$TMUX` to tell the difference —
anything launched from a pane inherits `$TMUX`, so a terminal opened inside tmux
still carries it after the session is detached.

## Also included

`scripts/tmux-where` — `pane_current_path` is tmux's **local** view, so a pane
sitting in ssh reports this machine's path and git branch. This prints the ssh
destination instead, and blanks the branch. It exits early on
`#{pane_current_command}` so the common case costs nothing.

It also names a linked **git worktree**, since a branch alone is ambiguous when
you run several agents across worktrees of one repo:

```text
 main                    a normal checkout
 feat-api ⑂api-work      the same repo, linked worktree "api-work"
```

Branch and worktree come from reading `.git/HEAD` directly, with **no `git`
process at all** — a repo's HEAD is a one-line file, and this runs once per pane
on every status redraw. Measured on this repo, that took the branch segment from
6.2 ms to 2.8 ms a call, which is bash's own startup cost and nothing else. git
is still asked for one case it alone can answer: a detached HEAD, whose
short-hash width git decides from the size of the repo.

The one deliberate difference from `git rev-parse` is that this ignores
`safe.directory` — for a status bar, labelling the directory you are sitting in
is the wanted behaviour.

## Tests

```sh
tests/run.sh
```

144 cases. They run against a dedicated scratch pane, never a live one. They skip rather than fail when no tmux server is running.

The path-extraction cases drive `tmux-open-path --dry-run`, so they need neither
tmux nor an editor. The branch cases build a scratch repo with `git commit-tree`
rather than `git commit`: no user config, no hooks, no signing prompt.

## Whole-pane tint

A **blocked** pane also gets a faint warm tint across its entire background
(`window-style`, +2% luminance — noticeable, never harmful to readability).
It is the only state loud enough to earn more than a border.

## Which commands count as an agent

One option, read by every format:

```tmux
set -g @cockpit_agents "^(claude|codex|aider|cursor|opencode|gemini|crush)$"
```

`cockpit.tmux` sets that default if you have not, so there is nothing to do
unless you run a CLI it does not know — then set your own **before** the
`run-shell` line and everything follows. It is an extended regular expression
matched against `#{pane_current_command}`.

Do not set it to the empty string. tmux treats an empty pattern as matching
every command, which would paint every pane in the session as an agent.

## Stale state

Hooks cannot fire if an agent is killed or crashes, so `@panestate` can outlive
the agent it describes. The formats therefore only believe it while the pane is
actually running one:

```tmux
#{m/r:^(claude|codex|aider|cursor|opencode|gemini|crush)$,#{pane_current_command}}
```

Other projects solve this by polling the process tree from a background daemon.
tmux already knows the foreground command, so this costs nothing.

## Deliberately unwired

`SubagentStop`, `TaskCompleted`, `PreCompact`, `TeammateIdle`.

`SubagentStop -> running` looked harmless but cleared a failed subagent's `error`,
so a run that failed finished **green**. The next `PostToolUse` restores `running`
anyway, so the hook only ever destroyed information. `TaskCompleted` fires
mid-turn per task-list item and flashed green while the agent kept working;
`Stop` already covers every turn end.

## Known limits

- A session started **before** a hook was added never fires it — Claude Code
  snapshots hooks per session id. Such a pane stays grey until it restarts.
- A long "thinking" phase fires no hook, so a working pane can read as idle.
  CPU is not a usable substitute: an agent waiting on the API is
  indistinguishable from an idle one.
