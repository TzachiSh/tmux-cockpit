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

### Context fill, optional

`scripts/agent-context` is a Claude Code `statusLine` shim: fed the statusline
JSON on stdin, it records the pane's context-window fill as a percent in
`@panestate_ctx` and prints a minimal `<model> · ctx NN%` line. It never blocks
or errors at the agent — an unparseable payload, a missing `jq`, or a writer
that stalls is a silent no-op within a bounded read, the same contract
`agent-state` holds for hooks.

A `statusLine` fully replaces any other one you have configured, so it is not
wired by default — see the commented `_statusline_comment` stanza in
`examples/claude-hooks.json` for the setting to add. Once wired,
`examples/tmux.conf` shows a `◔NN%` warning on a window's tab once its active
pane crosses 80% fill, ahead of the compaction that would otherwise land
unannounced.

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
| `StopFailure` | `error` — a turn killed by an API error fires this, **not** `Stop`; unwired, it left a bright "working" marker forever |
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
a remote-only branch becomes a local tracking branch. The list marks the branch
you are on with `●` and labels branches held by a linked worktree with `⑂name`
— git will refuse those, and now you can see it before pressing Enter. When git
does refuse — a dirty tree, a held branch — you read its message, press a key,
and you are **back in the picker with your query intact** instead of starting
over. It is `alt-↵` and not
`^N`, which is fzf's own move-down: as a create key it meant navigating with
emacs keys left the picker, and any filter you had typed became a branch name. When git refuses — a dirty
tree, or a branch already checked out in another worktree — you get git's own
message, naming the files or the worktree, and the popup waits for a key.

The plugin binds the key but **cannot mark the click target**: `status-right`
belongs to your config, not to the plugin. Wrap whatever shows the branch:

```tmux
set -ag status-right "#[range=control|1]#{E:@catppuccin_status_gitmux}#[norange]"
```

The **directory segment** works the same way: wrap it in `range=control|2` and
clicking it opens the **folder browser** at the pane's working dir — the same
popup the path picker descends into, entered from the top.

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

**Tab marks several** — a compiler error names a header and its source, a git
diff names two files: mark both, `↵` opens them all. Tab means the same thing in
`tm`'s kill picker.

**A folder browses, it does not open.** `↵` on a directory descends into it —
`..` climbs back up, `↵` on a file opens it. `^O` — in the picker and in the
browser — opens the selection **as-is**: on a folder that means the whole
folder lands in the editor (VS Code opens it as a workspace). In a
multi-selection `↵` skips directories, so marking three files and a folder
opens the files.

Rows keep their line numbers — a clicked `src/app.ts:31` reads as
`src/app.ts:31` in the list, not a bare filename.

The list takes only the rows it needs — with one or two candidates the preview
gets ~80% of the popup instead of leaving the top half empty around two
filenames.

### Instant open, if you want it

By default every click shows the picker. A lone candidate that carried an
explicit `:line` is machine output — a stack trace, a compiler error, a grep
hit — and can skip the popup:

```tmux
set -g @cockpit_open_single line     # instant-open a lone candidate with a line number
set -g @cockpit_open_single always   # instant-open any lone file candidate
```

Folders always go to the picker; unset keeps today's ask-first behaviour.

### Popup size

One pair of options sizes both popups (default `70%` × `70%`). Set them
**before** the `run-shell` line, like `@cockpit_agents`:

```tmux
set -g @cockpit_popup_width  60%
set -g @cockpit_popup_height 50%
```

Anything `display-popup` accepts works, absolute columns/rows included. The
path picker re-reads them on every click, so changing them needs no reload.

### The snippet

The preview pane is the reason to look before you leap, so it shows the thing
you are actually deciding about:

| what you picked | what you see |
|-----------------|--------------|
| a file, with a line number | the code around that line, the line highlighted |
| a file, no line number, **uncommitted changes** | its `git diff` — the changes are almost always why it's on that line |
| a file, no line number, clean | the file from the top |
| a directory | `git status --short` and a diff stat for it, or `ls -la` outside a repo |
| a binary | its type and size, never its bytes |

With [`bat`](https://github.com/sharkdp/bat) installed (`batcat` on Debian) the
snippet is syntax-highlighted and carries the **git gutter** — `+` and `~`
beside every line you have changed but not committed — so you see the code and
what moved in it at the same time. Without `bat` it falls back to numbered
lines with the target in reverse video. Nothing else is required.

`^G` switches the preview to the diff for any entry — in the folder browser
too — and `^F` back to the content. A clean file says "no uncommitted changes"
rather than showing an empty pane.

`tmux-open-path --preview <path> [line]` (or `--preview-diff <path>`) renders
exactly what the popup shows, which is the quickest way to see why a preview
looks wrong.

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

The picker fires even when the application in the pane has mouse reporting on —
which Claude Code and most agent TUIs always do. A guard that yielded to the
app was tried, and it made the gesture dead in every agent pane: the primary
target. Plain clicks, wheel and drag still reach the application untouched;
only the two chorded gestures belong to the picker. It also works in copy-mode,
where `#{mouse_line}` reads the scrolled-back content — scroll up and click a
path in old output.

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

`prefix + S` opens it in the same popup chrome as the other pickers. Each
session previews as its windows with the cockpit state beside every agent pane
— the one view the status bar cannot give you is *across* sessions.

```sh
tm <name>     attach to <name>, creating it if missing
tm            pick from a session list; if none exist, ask for a name
tm -k [name]  kill sessions (no name = pick them, Tab selects several)
```

In the picker: typing **filters**, like every other picker here — it used to be
a command mode where a bare `k` (the first key of filtering to "kanban") killed
the highlighted session instantly. Kill is `^X` now, the attached session is
labelled and refuses to die from the picker (`tm -k <name>` still kills
anything, deliberately, by name), and the kill picker wears red. The
new-session prompt cancels on empty instead of looping. It uses `switch-client` inside tmux and `attach-session` outside,
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

Its colour is `@cockpit_blocked_bg` (default `#52303a`). The previous
hard-coded `#241a1e` measured 1.03:1 against Catppuccin Mocha — invisible on
the very theme those variable names come from — and *darker* than most bases,
reading as disabled rather than alarmed.

The tint never fires at the pane you are **on**: `window-active-style` pins the
focused pane to the terminal default. An alert that colours the thing you are
already looking at — prompt on screen, border already red — is noise, not
signal. So the tint marks a blocked pane in a split you are *not* focused on,
and nothing else. The explicit `bg=default` is load-bearing; left unset, the
active pane inherits `window-style` and the tint comes back.

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
