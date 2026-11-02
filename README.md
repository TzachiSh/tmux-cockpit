# tmux-cockpit

Shows what every AI coding agent in your tmux session is doing, without you
having to switch windows to find out — then lets you act on what you see without
leaving the pane. Click the branch in the status bar to switch branch (a plain tap on a phone).
Ctrl+double-click any line of output to pick a path off it and open that file,
at the line it names.

```text
 claude-main ●    claude-2 ▲    coder ○
─ NEEDS YOU 2:claude-2 ────────────────────────────────────────
```

A blocked pane's own border, visible in a split but not the one you are
focused on, names the ask instead of just flagging one:

```text
─ NEEDS YOU Claude needs your permission to use Bash ───────────
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
Claude Code writes it to `pane_title`, so with several agents running you see
what each is working on instead of three tabs all reading `claude`:

```text
 Color scheme for code … ●     Openvpn setup with voi… ○     build ○
```

Non-agent panes fall back to the window name.

The tab reads `pane_title` **inside tmux**, so it works regardless of the outer
terminal. `examples/full.tmux.conf` ships with `set-titles off`: an agent
retitles constantly, and forwarding each change out as an OSC title sequence
(they do not coalesce like cell redraws — measured 609 titles, 27KB/3s under a
storm) stalls Windows Terminal's draw loop until its tmux client freezes. The
in-tmux tab above is unaffected. To let the outer window title track the topic
on a terminal that coalesces fine (Ghostty, kitty, iTerm2), uncomment the two
`set-titles` lines in that file.

## Where it shows up

- **Window tabs** — one marker per window. Worst state in the window wins, so a
  blocked pane is never hidden behind a busy one.
- **Pane border** — the whole top border takes the state colour. Colour is
  pre-attentive: you see which pane needs you without reading anything. A
  blocked pane's border also names what it is asking, so you can tell a
  one-key approval from a decision without switching to it.
- **Active pane's border** — names any *other* window that is blocked, since
  you cannot see its border from here.
- **Status bar** — the `#{agent_fleet}` token counts blocked, errored and
  unreviewed agents across *every* session, not only the one you are attached
  to.

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

### Status-bar tokens

Three tokens, usable in the six options `cockpit.tmux` rewrites when it loads:
`status-left`, `status-right`, `window-status-format`,
`window-status-current-format`, `@catppuccin_directory_text` and
`@catppuccin_gitmux_text`. Each must be set **before** the `run-shell` line.

That list is exhaustive. A token in an option of your own is never rewritten —
it reaches tmux as a name nothing defines and renders as nothing at all, and
pulling your option in with `#{E:}` does not change that.

| token | renders |
| --- | --- |
| `#{agent_where_path}` | `~/path` locally, the ssh destination when remote |
| `#{agent_where_branch}` | the git branch locally, blank when remote |
| `#{agent_fleet}` | `▲N` blocked, `✕N` errored and `◆N` finished-but-unreviewed agents, every session |

`#{agent_fleet}` is the one alert that is not tied to what you can see: window
dots and borders only speak for the session in front of you, so a *detached*
session can sit blocked for an hour without a pixel changing. It looks at the
whole server, and renders nothing at all while nothing is waiting.

It counts **window slots**, not distinct panes. A session made with
`tmux new-session -t` is a second *view* of one set of windows, and
`link-window` puts a single window in two sessions; either way a shared blocked
pane is counted once per view — three grouped views of one blocked agent read
`▲3`. tmux's own `list-panes -a` reports those panes the same way, and a format
has no means to de-duplicate them. So the count can read high on a grouped
setup. It never reads low, which is the direction that would matter: a fleet
rendering nothing is genuinely quiet.

It is pure format syntax — a redraw runs no shell — and it walks nothing
either: the token expands to one precomputed option, `#{E:@cockpit_fleet}`,
which `scripts/agent-state` rewrites at each state transition, off the redraw
path. The whole-server pane walk it replaced ran up to four times per
expansion, and an agent emitting an OSC-2 title forces an expansion on every
keystroke.
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
both example configs show a `◔NN%` warning on a window's tab once its active
pane crosses 80% fill, ahead of the compaction that would otherwise land
unannounced.

## Install

```sh
git clone <this repo> ~/.tmux/plugins/tmux-cockpit
~/.tmux/plugins/tmux-cockpit/install.sh    # symlinks tm and the two tmux-* commands into ~/.local/bin
```

Add to `~/.tmux.conf` (see `examples/tmux.conf` for the full block; its
`status-right` directory and branch segments read catppuccin's
`@catppuccin_status_*` options, so without that theme they render nothing):

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
`--uninstall` removes only links that point back into the plugin (and reports
anything it could not remove). `install.sh --help` prints usage and exits.

`install.sh --check` is the doctor. It writes nothing and fixes nothing: one
row per thing that can be wrong (tmux, jq, fzf, bat, gum, `$PATH` order, the
config, the hooks, truecolor, the popup border), the fix beside it, and the
exit is the worst row, 0 ok or skipped, 1 warnings, 2 something is broken.
Rows it cannot answer from where you ran it, such as truecolor outside tmux,
come out `skip` and count as 0. `bootstrap.sh --check` runs the same thing.

`bootstrap.sh --full` is the opt-in tier: it fetches pinned versions of fzf,
gum and bat into `${XDG_BIN_HOME:-~/.local/bin}`, sha256 verified where the
project publishes checksums, and shadows a distro copy rather than replacing
it. Without `--full` it still fetches fzf alone when the one it finds is below
0.73. No sudo, and nothing is downloaded when tmux loads the plugin or when you
press a key. Add `--latest` for each project's newest tag instead of the pins.
Offline it skips the tool and carries on; a checksum it cannot fetch or cannot
match installs nothing. On macOS with brew it runs `brew install fzf bat gum`.

The board (`prefix + w`) is the one optional extra: a small binary, fetched by
`prefix + U` or built with `make tui`. Everything else here works without it.
See [docs/tui.md](docs/tui.md).

### Or with tpm

It is a normal tpm plugin — an executable `*.tmux` at the repo root — so:

```tmux
set -g @plugin 'catppuccin/tmux'          # declare it FIRST; the example
                                          # block's status-right needs it
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

Wire the hooks into `~/.claude/settings.json`:

```sh
~/.tmux/plugins/tmux-cockpit/install.sh --hooks             # merge them in
~/.tmux/plugins/tmux-cockpit/install.sh --hooks --dry-run   # preview the diff first
```

It merges additively — your own hooks are never touched, cockpit's are appended
per event only where missing, and re-running after an upgrade only adds what's
new (a reformatted settings.json, e.g. from `jq -S` or Claude Code rewriting
its own file, is still recognized and never re-appended). Re-run
`install.sh --hooks` after every plugin update: it also removes hooks a newer
version no longer ships, such as the old PreToolUse `review-gate --pretool`. A `settings.json.bak`
is written before the first change (an existing `.bak` is never overwritten).
`--hooks --dry-run` previews the merge without writing or creating anything,
even on a fresh install with no `~/.claude` yet. Requires `jq`; without it, or
without Claude Code's settings.json at that path, merge the `hooks` object
from `examples/claude-hooks.json` by hand:

| hook | state |
|------|-------|
| `SessionStart` | `off` — a resumed pane must not inherit old state |
| `SessionStart` (2nd) | injects `skills/unslop/SKILL.md` as context (through `jq`): the writing rules every agent on the box follows, with no per-machine paste |
| `UserPromptSubmit` | `running` |
| `PermissionRequest` | `needs-input` |
| `PostToolUse` | `running` |
| `PostToolUseFailure` | `error` |
| `SubagentStart` | `running` |
| `TeammateIdle` | `wake` — a teammate idling wakes its lead with no hook of its own; `done` → `running`, sticky states and a pending `◆` kept |
| `PreToolUse` | `running` — marks the pane busy as a tool begins |
| `Stop` | `done` |
| `Stop` (2nd) | `review-gate` — flags `◆` when the finished diff is unreviewed; hands a queued review to the agent at this turn boundary (`additionalContext` on stdout, once); caps a `/goal` rewake chain at `@cockpit_rewake_cap` and honours `review-gate --concede` (see [A goal that will not be met](#a-goal-that-will-not-be-met)) |
| `StopFailure` | `error` — a turn killed by an API error fires this, **not** `Stop`; unwired, it left a bright "working" marker forever |
| `Notification` | `notify` (parses the JSON on stdin) |
| `WorktreeCreate` | `worktree-create` — `claude --worktree <name>` and `EnterWorktree` put the tree under `~/.cache/cockpit-wt/<name>` on branch `<name>` (branched from the launch checkout's HEAD; an existing branch is checked out), so the session's cwd is the tree it works in and every cockpit read describes that tree. One namespace for every repo on the machine; subagents with `isolation: worktree` land there too, with their slug branches. Not with `--worktree --tmux` together, which skips the hook |
| `WorktreeRemove` | `worktree-remove` — when claude means to remove a tree the hook above made (remove on `ExitWorktree`, verified on 2.1.246; a subagent finishing clean, per the docs), a clean tree on a merged branch goes, tree and branch; a dirty tree or unmerged commits stay, which is claude's own rule for the trees it makes itself. `claude -p` never cleans up worktrees, so a subagent run under `-p` leaves its tree and slug branch behind; `git worktree remove` plus `git branch -d` |
| `SessionEnd` | `off` |

A `Stop` hook does not fire on a user interrupt (Esc), so a review queued for an
interrupted agent waits for the end of that agent's next turn — nothing hands it
over earlier, `prefix r` included.

## Requirements

- **tmux >= 3.8**. Below it the plugin says so and binds nothing; `bootstrap.sh`
  builds 3.8 into `~/.local/bin` when the distro's tmux is older.
- **Rust >= 1.88** only to build the board with `make tui`;
  the fetched binary needs neither Rust nor cargo.
- **fzf >= 0.73** for the pickers. Debian 13 ships 0.60.3, Ubuntu 24.04
  0.44.1 and Ubuntu 26.04 0.67.0, all below it, so `bootstrap.sh` fetches
  0.74.3 into `~/.local/bin` when it finds an older one, and
  `install.sh --check` fails below 0.73. `bat` (`batcat` on Debian) and
  `gum` are optional; `jq` is needed by `install.sh --hooks` and the review
  gate.
- Claude Code (or any agent that can run a command on lifecycle events).
- A Nerd Font is *not* required. The markers are plain Unicode geometry.

## Click the branch to switch branch

| key | does |
|-----|------|
| click the branch segment | branch picker |
| `prefix + b` | the same picker, without the mouse |
| `prefix + a` | the agents menu: every row below, one letter each, no Shift — `@cockpit_ai_key` rebinds, `off` disables |
| `prefix + u` | jump to the pane that needs you (again: next one) — `@cockpit_jump_key` rebinds, `off` disables |
| `prefix + s` | the session picker, replacing stock `choose-tree` — `@cockpit_sessions_key` rebinds, `off` keeps stock |
| `prefix + r` | review an agent's uncommitted diff — always the fleet queue, blocked agents first, then dirty ones, with the current pane preselected (replaces stock `refresh-client`) — `@cockpit_review_key` rebinds, `off` restores stock; `@cockpit_review_window on` opens a window instead of the popup |
| `prefix + i` | one screen for the pane you are on: cwd, the git root the review diffs, state, review mark, review base, session id and review dir; it replaces stock window info, `@cockpit_info_key` rebinds, `off` restores stock |
| `prefix + f` | the panel: ask a text-only `claude -p` about this pane and paste the answer back into it — see [The panel](#the-panel-cockpit-tui) below; needs the optional `cockpit-tui` binary, replaces stock `find-window`, `@cockpit_panel_key` rebinds, `off` restores stock |
| `prefix + w` | the board: every agent pane as a live tile, blocked first, each previewing the pane itself; needs the optional `cockpit-tui` binary, `@cockpit_board_key` rebinds, `off` keeps stock; `@cockpit_board_window on` opens a window instead of the popup |
| `prefix + R` | reload the tmux config — the file tmux actually loaded, XDG included — `@cockpit_reload_key` rebinds, `off` disables |
| `prefix + M` | move this window to another session, or a new one, picked from a menu; the client follows it, and moving your last window out will not detach you (replaces stock `select-pane -M`) — `@cockpit_move_key` rebinds, `off` restores stock; also `prefix + a m`, and `prefix + m m` in the example config |

Every key on one page, a line each: [KEYS.md](KEYS.md).

The branch picker and the path picker share their keys: `↵` acts, `esc`
cancels, `^/` toggles the preview and `^U`/`^D` scroll it. The review popup's
two pickers are their own case — see below.

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
terminals wrap paths in. It also unwraps the `Name(path)` shape agents print
their tool calls in — `Write(docs/spec.md)`, `Read(src/app.ts:31)` — where the
path is real but the token around it is not; `file(12,3)` stays a *location*,
not a wrapper. Existence on disk is the filter, so a word that merely looks
like a path is dropped.

**Tab marks several** — a compiler error names a header and its source, a git
diff names two files: mark both, `↵` opens them all. Tab means the same thing in
`tm`'s kill picker.

**`alt-c` copies the paths instead of opening them.** The marked rows, or the
row under the cursor when nothing is marked, go to the tmux buffer and the
clipboard as absolute paths, one per line, for pasting into an agent's prompt.
It works in the folder browser too, where `⇥` marks rows for `alt-c` alone: `↵`
and `^O` there act on the first marked row. The copy goes out as OSC 52, so it
reaches the system clipboard only if your terminal accepts clipboard writes.

To copy something no key already copies, hold **shift** and drag. A tmux popup
has no copy mode, so in any of these popups the terminal's own selection is the
one that works.

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

The session picker (`prefix + s`) reads them as **ceilings**, not sizes: it
shrinks to fit its sessions, at most 70% of the client on a desktop, 90% wide
below 160 columns, and full width in touch mode.

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

### When the terminal opens the file itself

Claude Code wraps every path it prints — the one inside `Write(...)`,
`Read(...)`, `Edit(...)` — in an **OSC 8 hyperlink** pointing at
`file:///abs/path`. Ghostty (and any terminal that follows those) treats that
as a link of its own, and a modified click on it is handed to `xdg-open`: a
graphical editor or file dialog opens and the picker never runs.

Read out of Ghostty 1.3.1's `Surface.zig`, this is not a race we can win from
tmux — the link is handled *first* and the event is swallowed:

```zig
// Handle link clicking. We want to do this before we do mouse
// reporting or any other mouse handling because a successfully
// clicked link will swallow the event.
```

It fires on the **release of the first click**, with no click-count test, so it
takes the opening half of a `Ctrl`+double-click too — tmux never accumulates
the second and `C-DoubleClick1Pane` cannot fire at all.

What decides whether Ghostty claims the click is whether it is tracking links
at that moment, and with `set -g mouse on` tmux owns the mouse, so it only
tracks them while **shift** is also held. In practice:

| gesture, over an agent's `Write(path)` line | what happens |
|---|---|
| **`Ctrl`+right-click** | **only the picker** — link opening is a left-button feature, so nothing races |
| `Ctrl`+double-click | the picker opens, **and** Ghostty opens the link's file in a GUI — the link fires on the first click's release, before the event is forwarded |
| `Ctrl`+`Shift`+click | Ghostty only — `xdg-open`, a GUI window |

`Ctrl`+right-click is the recommended gesture on lines that carry hyperlinks
(everything an agent prints); the others still work everywhere else.

So: no shift. If you want Ghostty to stop claiming those clicks entirely, one
line in `~/.config/ghostty/config` does it —

```
mouse-shift-capture = always
```

— which sends shift-clicks to the application instead, at the cost of losing
shift-click-to-extend-selection while a mouse-tracking program is running.
`link-url = false` is *not* the option: it turns off Ghostty's regex URL
matcher, and OSC 8 hyperlinks are checked before that matcher and are not
affected by it. As of 1.3.1 nothing disables OSC 8 clicking on its own.

If neither gesture does anything, check what your terminal actually sends:

```sh
tmux bind -n C-DoubleClick1Pane run-shell -b "echo fired >> /tmp/probe"
: > /tmp/probe   # then Ctrl+double-click a pane and look at /tmp/probe
```

Opens in VS Code at the line. For a terminal editor instead:

```tmux
set -g @cockpit_editor nvim   # new window next to the one you clicked, at +LINE
```

Keep it a **terminal** editor: the review's `^E` reads the same option, and
opens it in the popup's own terminal. A GUI editor belongs in `VISUAL`
instead.

`@cockpit_editor` wins. After it comes VS Code, either as `code` on the `PATH`
or as the remote shim Remote-SSH and Coder install under `~/.vscode-server`,
and only where neither of those exists does the click fall back to `$VISUAL` or
`$EDITOR`. On a Remote-SSH or Coder host that is a change: with `EDITOR=nvim`
and no `@cockpit_editor` set, a click now lands in the VS Code window you are
already looking at, where it used to open nvim in a tmux window. Set
`@cockpit_editor nvim` to keep nvim. With no editor anywhere it says so plainly
rather than reporting a success that never happened.

## Review the agent's changes

`prefix + r` opens a popup review of an agent's uncommitted diff. Comments
attach to individual lines, and nothing reaches the agent until you say so —
there is deliberately no key that approves the whole thing at once.

It always opens the **fleet queue**: every agent pane, on every session,
blocked ones first, previewed the same way the path picker's folder preview
shows a directory's `git status`. A pane with nothing to review reads `clean`
(or `no repo`) in the ± column and `↵` on it jumps there instead of opening
an empty review. Nothing else in this plugin answers "which of my agents
needs my eyes"; the queue exists because visiting each pane in turn to find
out does not scale past two or three of them.

The tree a row diffs is the pane's cwd, unless the pane option `@cockpit_dir`
names another. An agent that made its worktree by hand (`git worktree add`
from the main checkout) keeps its process, and so its cwd, in the main
checkout and drives the tree with `git -C`; its row would read clean master.
One line from the agent fixes that, and every reader (the queue, `--delta`
for the Stop hook's ◆, `prefix + i`) follows it:

```sh
tmux set-option -p -t "$TMUX_PANE" @cockpit_dir ~/.cache/cockpit-wt/my-branch
```

`-p` is not optional: a window or global `@cockpit_dir` is inherited by every
pane, and then every row diffs that one tree.

`EnterWorktree` and `claude --worktree` need none of this; they move the
process itself, and the cwd is the tree.

From an **agent pane** the queue opens with that pane already under the
pointer, so `prefix + r` there is still one `↵` from its review — and the
other agents waiting on you stay on screen instead of being replaced by it.
Finishing a review returns to the queue, on the pane you just left, so
reviewing four agents is four `↵` rather than four `prefix + r`; `esc` on the
list is the way out.

The review diffs the **git root of the pane's cwd**, and the header names it
(`root: ~/.cache/cockpit-wt/api-work`). An agent that edits another worktree
by absolute path from a main-checkout pane is reviewed on the wrong tree, and
that line is how you notice. The fix is workflow, not the plugin: an agent that
works on a branch lives in that worktree, which is what the `WorktreeCreate`
hook above arranges for `claude --worktree` and `EnterWorktree`.

The review opens in a popup. `set -g @cockpit_review_window on` opens it as a
real window (`new-window -n review`) instead, if you would rather keep the
fleet's borders and window tabs visible while you read.

The order is that question's answer, and it is deliberately **not** churn.
Agent state ranks first — `needs-input`, then `error`, `running`, `done`, then
panes with no state at all — and churn only breaks ties inside a rank. An
agent parked on a permission prompt is the one actually waiting on you, and it
usually has nothing at all to diff; sorting by churn listed it last, or (with a
clean tree) left it out entirely, and answered "who typed most" instead. A
blocked pane is listed even with a clean tree, and its `±` column reads `—`
rather than `+0 -0`, because there is nothing to review there — only someone to
unblock.

Each row is `state · title · where · ±`, in aligned columns. The `where`
field is `repo⑂branch`, and it is what tells two worktrees of the same repo
apart, which `s:w claude` alone cannot:

```text
needs-input     main:2 claude    cockpit⑂feat/review-p0     —
◆ done          side:0 codex     cockpit⑂main               +142 -30  ?2 new
↵ running       main:4 claude    api⑂fix/token-refresh      +8 -8
```

The state column carries the review mark: `◆` a finished diff nobody has read
yet (the `Stop` gate flagged it), `↵` a review already pasted into that pane
and not yet sent. Without them the queue keeps sending you back to a pane you
have already been through.

`◆` is written by `review-gate`, the second `Stop` hook, at the turn boundary
where the agent finishes: it sets `@cockpit_review = pending` when the pane's
tree still differs from its review base. The mark clears when the agent starts
working again, when that diff goes empty at a later `Stop`, or when you approve
with `^A`. The same step recounts `◆N` in `#{agent_fleet}`, so the status bar
tells you how many finished agents nobody has read across every session,
attached or not. Nothing polls, and a redraw still runs no shell.

`tmux-review --fleet` prints those rows tab-separated with no fzf, if you want
to drive the same list from a script.

The fleet queue — and only it — takes extra fzf options from a global tmux
option, so you can bend the picker to your own taste without patching the
script:

```sh
set -g @cockpit_fleet_fzf_options "--height=80% --pointer='▸'"
```

It is confined to this picker on purpose. In the comment loop `↵` has a
contract (it pins a comment, it never approves) and `^S` is the consent
gesture; one stray `--bind` from a config file would quietly break either one.

The review popup's two pickers do not share one key set — the fleet queue is
a plain list, `↵` picks a pane; the comment loop's typed text *is* a
comment, so the keys that would collide with typing it move elsewhere:

| key | fleet queue | comment loop |
|-----|-------------|--------------|
| `↵` | review the selected pane | pin the typed comment (empty: nothing) |
| `alt-↵` | — | alias for `↵` |
| `^/` | toggle the preview | toggle the preview |
| `^U` / `^D` | scroll the preview | — (would edit the comment instead) |
| `alt-↑` / `alt-↓`, `alt-u` / `alt-d` | — | scroll the preview a line, half a page (the mouse wheel over it scrolls too) |
| `^R` | — | re-snapshot the diff; names any comment the agent moved (it still delivers, quoted) |
| `^X` | kill the window under the cursor, after a y/N that names it and its pane count — the session too, when it is that session's last window (a forgotten agent elsewhere) | delete the comment(s) on the row under the cursor |
| `^L` | — | show every pinned comment in the terminal |
| `^E` | — | open your editor on the typed text; with nothing typed on a commented row, on that comment (save empty to delete it) |
| `⇥` | — | mark the row (any number) |
| `^Y` | — | copy the file under the cursor, then its diff, to the tmux buffer and the clipboard |
| `alt-y` | — | copy the whole review's diff |
| `alt-c` | — | copy the marked rows' lines as the file has them, tabs and all (a `-` row: HEAD's line; or the row under the cursor), to the tmux buffer and the clipboard |
| `^B` | — | cycle the base the diff is taken against |
| `^A` | — | approve what you just read (only with nothing pinned) |
| `^S` | — | deliver the whole batch; asks first |
| `esc` | cancel | leave — the batch stays pinned (asks first) |

Inside the review: `↵` pins whatever you typed as a comment on the diff line
under the cursor — `alt-↵` does the same thing, kept as an alias for
terminals where the plain chord doesn't land. `^E` hands what you have typed
to your editor and pins the file that comes back, so a comment that needs three
lines costs no more than one — an empty file pins nothing, so deleting
everything and saving is the cancel. With nothing typed on a row that already
carries a comment, `^E` opens that comment instead and replaces it; saving an
empty file deletes it. Several comments on one row open as one buffer and
save back as one: the agent reads them as paragraphs under one anchor either
way. `^E` uses `@cockpit_editor` when set
(`set -g @cockpit_editor nano`; a **terminal** editor, because the path picker
opens the same option in a tmux window), else `$VISUAL`, else `$EDITOR`. With
none of the three set it tries VS Code's remote shim, and falls back to `vi`.
A GUI editor goes in `VISUAL`: `tmux set-environment -g VISUAL 'code --wait'`.
The popup's children see the tmux *server's* environment, so a shell `export`
made after the server started is not seen. That shim arm is for the Remote-SSH
or Coder host where there is no `code` on the `PATH` and usually no `$EDITOR`
either, and where `vi` in a popup is the wrong answer next to the VS Code
window the diff is being read in. It takes `--wait`, so the buffer comes back
the way `vi`'s does, and `^S`'s confirm offers
`edit the batch in VS Code first`. From a plain ssh session nothing can reach
the laptop's VS Code. The list reloads in place and marks the row `●` (the
digit instead, when more than one comment sits there), so five comments cost
one popup, not five — the same `execute-silent(...)+reload(...)` shape `tm`'s
kill picker already uses.
`^S` delivers the whole batch, and asks first: it names the count and the
target pane by its title, and offers the batch to your editor as text before it
goes (`deliver N comment(s) to fix the parser (%7)` /
`edit the batch in nano first` /
`keep reviewing`). `esc` asks the same way once anything is pinned
(`leave — N comment(s) stay pinned` / `keep reviewing` / `discard N
comment(s)`), because one reflexive escape should not throw away a
twenty-comment review. `leave` keeps them on disk: `prefix + r` on that pane
picks the review up where you left it.
**`↵` pins a comment — it never approves.** With nothing typed it does
nothing: there is no accept-all key on purpose, a review whose only exit is
one Enter on the whole diff is the rubber stamp this replaces.

The rows are a **snapshot** taken when the popup opens, so an agent that
keeps editing cannot move a `●` onto a line you never touched. When you do
want its newest lines, `^R` takes a fresh snapshot on purpose and names the
comments that no longer sit on a changed line — they still deliver, because
the quoted fragment is what the agent finds them by. A resumed review says the
same thing on open: `resumed 5 pinned comment(s), 2 no longer anchor`.

To copy a piece of the preview, hold **shift** and drag: a tmux popup has no
copy mode, so the terminal's own selection is the one that works there. `^Y`
copies the whole file plus its diff, `alt-y` every file's diff, for pasting
into another agent. `⇥` marks rows and `alt-c` copies their lines as the file has them, one per
row, when what you want is a handful of lines rather than a file. All three go
out as OSC 52, so they reach the system clipboard only if your terminal accepts
clipboard writes; some disable them by default.

**Big files are named, not diffed.** To tell whether a file changed at all git
hashes the whole of it, about 100 ms at 32 MB and 26 s at 1 GB, and the popup
is modal and rebuilds its rows on every reload. So a changed or untracked file
over **32 MB** is kept away from git and reported on one notice row at the top
of the list instead, though a tracked file whose size did not change is still
hashed once, so git can tell it changed:

```text
data/dump.bin (412 MB) skipped — over 32 MB, review it directly
```

The row carries no comment, no `^Y` and no mark glyph, exactly like the row a
diff too large to anchor line-by-line leaves. The file itself is untouched, and
the tree still counts as dirty because of it, so a pane whose only change is a
skipped file stays in the fleet queue and its review opens on that notice
rather than on an empty diff. The path picker's `^G` diff preview refuses any
file over 32 MB, naming the size. Put core dumps in `.gitignore`, as git's own
templates do, and the review never sees them at all.

**`^B` picks what the diff is taken against.** The base is the first of three
that holds: the checkpoint `refs/cockpit/reviewed/<branch>` when a review of
this branch was approved (only what moved since), the merge-base with the
upstream when that is not `HEAD` itself (the branch plus its uncommitted work),
else `HEAD` (today's uncommitted view). An agent that commits as it goes stays
in the queue that way instead of dropping out of it the moment it commits. `^B`
cycles the bases that hold, the picker's header names the one in force
(`base: since last review`), and the choice is remembered beside the comments,
so a resumed review reads the same diff it did before.

**`^A` approves what you just read.** It writes that tree, untracked files
included, to `refs/cockpit/reviewed/<branch>` and removes the review directory.
The next review of the branch diffs against it, so it shows only what the agent
changed after you looked. With comments pinned `^A` refuses and says to deliver
or discard them first: approving a batch nobody sent would throw it away.

That ref outlives the branch. `git branch -D` and `git worktree remove` both
leave it standing, so a branch deleted and recreated under the same name
inherits the old checkpoint. Nothing collects them; the purge is:

```sh
git for-each-ref --format='%(refname)' refs/cockpit/reviewed | xargs -n1 git update-ref -d
```

**`↺` marks a row you commented on in an earlier round** of the same review:
delivered already, with nothing pinned on it now. It reads "you said this last
time and the line is still here", which a fresh diff on its own cannot tell you.

A comment lands as:

```text
## scripts/tm:43 (+)
> the quoted fragment
use errors.Is() instead of direct comparison
```

anchored to the line's own **quoted text**, not a bare number, so it still
points at the right spot after the agent edits again — a bare line number
would not survive that. A comment containing `??` is a **question**: the
composed header tells the agent to answer it in chat, not to edit for it.

**Delivery is two steps, never one.** The finished batch lands **typed but
unsent** in the agent's pane — one bracketed paste, never an Enter — and a
human presses Enter there to actually send it. Live-verified on tmux: the
pane shows the pasted text sitting at the prompt, unexecuted, with and
without a trailing newline on the payload. Over 2048 bytes it lands as a
pointer to a `0600` file in `$XDG_RUNTIME_DIR` instead, so a long review is
not a wall of text typed straight into the agent's own prompt.

**A delivered review is a state, not a flash.** The notice naming the target
window stays up until the next message rather than blinking away in 750 ms,
and the pane itself is marked `@cockpit_review = sent` for as long as the
review sits there unsent. `prefix u` walks to those panes after the ones
whose agent is blocked on you, so a review you pasted and then forgot is
still reachable by a keypress instead of by memory. The mark clears the
moment the agent starts working again — pressing Enter is what fires the
`running` hook — so it never outlives the thing it describes.

**A delivery into a busy pane is queued, not refused.** When the target is
running or sitting on a permission prompt, nothing is pasted: the composed
review is written beside the comment store and the pane is armed with
`@cockpit_review_spool`. `review-gate` hands that text to the agent at its next
turn end as `hookSpecificOutput.additionalContext` on stdout, which the
transcript shows as Stop hook feedback rather than a hook error, and the spool
is disarmed before the hook speaks, so a queued review is handed over once and
never repeated. It costs no keystrokes: no paste, no Enter, no focus change,
and the turn boundary is the hook itself.

That drain is the hook's job and nothing else does it. Without `review-gate`
installed (`install.sh --hooks`), a delivery into a busy pane still archives
the batch to `delivered.md` and still writes the spool, but nothing ever reads
it; install the hook, or deliver again once the pane is idle.

A `Stop` hook does not fire on a user interrupt (Esc). A review queued for an
agent you interrupted therefore waits: nothing hands it over early, and it
lands at the end of the agent's *next* turn — after whatever you prompt next
has already run. `prefix + r` on that pane does not paste it; it opens a fresh
review of the same rows.

A second review delivered to the same pane before the agent stops replaces the
queued one: the spool is a single file, so the agent is handed the newer batch.
Both are in `delivered.md`. `^A` on a later round ends the review directory but
leaves a still-queued spool alone — the review you already sent is in flight,
and it still lands at the agent's next `Stop`.

Delivery can still be **refused**: the target is not an agent pane, or is not a
pane id at all. A busy one is not a refusal, it is the queue above. A refusal
keeps the batch: the comment store is left on disk, and the message names it
along with the `--deliver <pane> <store>` that re-sends it — spelled with the
script's own resolved path, not a bare `tmux-review`, so it pastes back even
from a plugin-only install where nothing is on `$PATH` yet.

The paste is gated a second time inside tmux, in the same command-queue item,
because the shell's own check is a ~50 ms-stale snapshot: a pane that stops
being an idle agent inside that window takes the queued arm rather than the
paste. A pane that is gone by then takes neither arm, and the store is kept
for the by-hand re-delivery.

What this deliberately does **not** do, and why:

- **No whole-diff, one-Enter approval.** The unit of review is the line or
  the hunk, never "the changes" — whole-diff review launders a change as
  reviewed without anyone actually reading it, and skimming has a measured
  ceiling of around 400 lines past which defect detection collapses.
- **Never sends Esc to the agent, never auto-submits.** Esc kills whatever
  the agent is mid-tool on, destroying in-flight work; typed text plus a
  human's own Enter lands at the next turn boundary instead
  (claude-code issues #63190, #30492).
- **No review daemon, server, or port.** What *does* outlive the popup is
  one directory per review under
  `${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/review/<pane>-<root>/`,
  created `0700`, holding `comments.md`, its row snapshot, the base `^B` chose,
  the batches already delivered and any queued spool. It exists so a
  dropped SSH client — which tears down a client-owned popup — cannot take a
  twenty-comment review with it: `prefix + r` on the same pane resumes it,
  marks and all. It lives until `^A` approves the review or you discard it; a
  delivered batch leaves `delivered.md` standing in it, which is what the next
  round's `↺` marks read, and a refused delivery keeps the store and names it
  in the message. `prefix + i` prints the directory for the pane you are on.
  There is no garbage collector; `rm -rf ~/.local/state/tmux-cockpit/review`
  is the purge.
  The spillover file an oversized review lands in is the delivery vehicle,
  not a store — the OS tmpdir is the only garbage collector it gets, the
  same as everything else this plugin writes.
- **Every comment carries `path:line` and the quoted source line**, never a
  line number alone. A bare number goes stale the instant the agent edits
  again — the same staleness that makes a GitHub review comment "outdated"
  the moment the diff it anchored to moves.
- **Delivery is a batch, never a comment at a time.** Nothing reaches the
  agent's pane until `^S` — one paste per review, not a stream of small
  interruptions while you are still deciding what you think.
- **Never selects, creates, or focuses a pane as a side effect.** Delivery
  targets `-t <pane>` and leaves focus alone; tmux focus-stealing has raced
  and corrupted concurrent pane input before (claude-code issue #49184).
- **Never `send-keys` for the review's own text.** The body goes through
  `load-buffer`/`paste-buffer` only — `send-keys` is reserved for the Enter a
  **human**, never this script, presses.
- **Test files sort first, never last** in the diff-line list — a change to
  the tests is read before the change it tests, not after.

**The agent can ask.** `install.sh` links every `skills/*` dir into
`~/.claude/skills`, so `/cockpit-review` (or "get this reviewed") runs
`tmux-review --request` from the agent's own pane: one popup, the same
loop, and the batch comes back as the tool's result (exit 10) instead of a
paste — revdiff's contract. `^A` there approves as anywhere else.

You can also leave a requested review for later: `esc → leave` keeps the
comments pinned and `--request` returns 0, so the agent carries on with nothing
in hand. Running `/cockpit-review` again resumes exactly those comments.

### VS Code over Remote-SSH

`VISUAL='code --wait'` is enough on a local machine, and only there. Over
Remote-SSH `code` is not on the `PATH` tmux was started with, and VS Code hands
the remote a fresh `VSCODE_IPC_HOOK_CLI` socket per connection that the tmux
server, started before that connection or outliving it, never has. Nothing to
install: click-to-open finds the shim itself, and `^E` does too when
`@cockpit_editor`, `$VISUAL` and `$EDITOR` are all empty. They take the newest
shim under `~/.vscode-server` (`cli/servers/*/server/bin/remote-cli/code`, or
`bin/*/bin/remote-cli/code` from an older server) and run it against each
`vscode-ipc-*.sock` in `$XDG_RUNTIME_DIR`, newest first, until a window takes
the file. The shim is its own probe, so no `nc` or `socat` is needed.

Two caveats. A dead socket refuses in about 150 ms, so the walk stops at the 8
newest rather than spending six seconds on a home full of stale ones, and a
window whose socket is older than those 8 is missed. And a closed window's
socket keeps listening through VS Code's reconnection grace, so answering is
not proof of a live window: `^E` can then sit on `--wait` until you press esc.

From a plain ssh session, with no VS Code on the far end, nothing can reach
the laptop's editor. Use a terminal editor there.

### Coder

A Coder workspace opened from VS Code is Remote-SSH underneath, and there is
nothing extra to do: the shim sits in the same place, and a workspace that has
no `/run/user/$UID` puts its sockets in `$TMPDIR` or `/tmp`, which is where the
walk looks when `$XDG_RUNTIME_DIR` is unset.

### Security

`VISUAL='code --wait'` and the shim the popup finds for itself run entirely as
you: they use the VS Code CLI socket in your own `$XDG_RUNTIME_DIR`, which no
other user can open, and give a same-user process nothing it did not already
have. `/tmp` is only the fallback for a workspace with neither
`$XDG_RUNTIME_DIR` nor `$TMPDIR` set (a Coder container); where either is set
the walk never looks in `/tmp`, where another user could plant a listening
socket. Never run the popup under `sudo`. VS Code's remote server listens on a
loopback TCP port by default, so on a shared host set
`"remote.SSH.remoteServerListenOnSocket": true`, or another local user can
attach to your editor session. Remote-SSH installs and auto-updates
`~/.vscode-server` on that host; some admins do not allow that, ask first.

### Review options

The options the review and its keys read, with their defaults, in one block.
None of them has to be set; as with the rest, set them before the `run-shell`
line.

```sh
set -g @cockpit_review_key     r       # off restores stock refresh-client
set -g @cockpit_review_window  off     # on: a window instead of the popup
set -g @cockpit_info_key       i       # off restores stock window info
set -g @cockpit_jump_key       u
set -g @cockpit_sessions_key   s       # off keeps stock choose-tree
set -g @cockpit_move_key       M       # off restores stock select-pane -M
set -g @cockpit_ai_key         a       # the agents menu; n if you never use next-window
set -g @cockpit_popup_width    70%
set -g @cockpit_popup_height   70%
set -g @cockpit_fleet_fzf_options ""
```

## The board (cockpit-tui)

`prefix + w` opens every agent pane as a live tile: the state glyph, what the
agent is doing, how long it has been doing it, and a `capture-pane` of the pane
itself. Blocked agents sort first, so page 1 is the triage page. `↵` jumps to
the tile you are on and closes the board.

```text
 board · 5 agents · 1 needs you                                   1-4 of 5   ▼1 
┌ 1 ▲  henosis:1 · Openvpn setup ──────────────────────────────────────────────┐
│permission: Bash(sudo openvpn --config /etc/openvpn/work.conf --daemon…   4m12│
│Bash command                                                                  │
│sudo openvpn --config /etc/openvpn/work.conf --daemon                         │
│Do you want to proceed?                                                       │
└─ ↵ answer ───────────────────────────────────────────────────────────────────┘
┏▎2 ●◆ henosis:3 · Implement with test review and rig ━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃Bash: shellcheck on cockpit.tmux and tests/run.sh                          32s┃
┃$ make check                                                                  ┃
┃1001 assertions, 0 failures                                                   ┃
┃$ shellcheck cockpit.tmux tests/run.sh                                        ┃
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ 3 ●  work:2 · Fleet gate review ─────────────────────────────────────────────┐
│Agent: whole-branch review of phase 1                                     4m12│
│⏺ Agent (fleet-gate-review)                                                   │
│  Read tests/fleet.sh, 214 lines                                              │
└──────────────────────────────────────────────────────────────────────────────┘
┌ 4 ✓↵ work:4 · Left click issue ──────────────────────────────────────────────┐
│idle · handover pending                                                    12m│
│  ✓ pushed to origin/fix/left-click                                           │
│  PR #28 opened                                                               │
└──────────────────────────────────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f fix   i info   ? help   q close
```

It is a separate optional binary, `bin/cockpit-tui`, fetched by `prefix + U` or
built with `make tui` (Rust >= 1.88). Without
it the binding prints one message and exits 0, and nothing else changes.
`prefix + r` remains the fleet list: fuzzy filter, multi-select and the wheel
come free from fzf, and the board is the grid of previews fzf cannot draw.

The binary renders and decides; it never acts. It writes one line,
`<verb> <pane-id>`, and the binding runs the same `tmux-jump` / `tmux-review`
script the keys already use.

Keys, options, the screens, the states and the engineering rules:
[docs/tui.md](docs/tui.md).

## The panel (cockpit-tui)

The panel asks a text-only `claude -p` about the pane you are in and pastes
the answer back into it. It is a port of type-panel. Claude runs with
`--tools ""`, so it sees only the screen text it is sent and can neither edit a
file nor run a command. The panel itself writes its system prompt to a cache
file and the answer to a private temp file for the paste.

- **Compose**: type rough words. `^S` writes a prompt, command, steps,
  text or hint from them, `^F` fixes the spelling, `^T` inserts them as
  typed. `⇥` picks the output type and `^N` the model. The box has a
  cursor: `←` `→`, `^←` `^→` by word, `Home` `End` (`^A` `^E`) and `Del`;
  `⌫`, `^W` and `^U` delete before it.
- **Explain**: what the pane is doing, what should happen next, or your
  own question. When the next step is a message to a Claude Code agent,
  the answer ends with that message, ready to insert or send.
- Claude may ask up to 3 multiple-choice questions first (6 for Explain):
  `1`-`4` picks, `o` types your own answer, `g` tells it to guess.
- On a result, `i` inserts it into the pane without Enter, and `s` sends it
  with Enter after a confirm. The paste is bracketed only when the program
  asked for that, so a multi-line insert into a program that may run each
  line (python3, sh, psql) asks first too. A multi-line send takes `y`,
  not Enter. `n`, `q` or `esc` cancels it.
- On steps, `1`-`9` or `↑↓` pick one, `i` inserts it and `s` sends it. The
  panel comes back on the steps with that one marked `✓ inserted` or `✓ sent`
  (`+` with `COCKPIT_GLYPHS=ascii`), so you can go on to the next.
- `↑↓` and `PgUp` `PgDn` scroll a long result, answer or confirm. `esc` or `q`
  on an Explain answer asks once before it drops the conversation.
- `?` lists every key of the screen you are on (`F1` inside a text box, where
  `?` is text). `esc` steps back one screen, and the footer names where to:
  Home, the draft, the answer. On Home it closes the panel.
- Under 28x7 (28x12 in tablet mode) the panel asks for a larger window. `esc` and `q` still act as on
  the screen underneath and `^C` quits; other keys are dropped.

Secrets on the screen are blanked before anything is sent. `prefix + f`
opens it on the pane you are in, and `f` on a board tile opens it on that
tile's pane. The key replaces stock `find-window`:

```sh
set -g @cockpit_panel_key f    # the default; off restores stock find-window
```

It needs the `cockpit-tui` binary, like the board. The binary never writes to
the pane: it leaves `paste %N` or `send %N` and the text in the action files,
and `scripts/cockpit-panel` does the paste and says what it did. Options,
variables and the action file are in [docs/tui.md](docs/tui.md).

`x` on a result, a step or an answer copies it to the clipboard with OSC 52
and says `sent N chars to the clipboard`. Inside tmux that needs
`set -g set-clipboard on`; the default, `external`, drops it.

### On a tablet

Under Termux the panel opens in tablet mode: big tiles, a row of tap buttons
in place of the `^S` `^F` `^T` `⇥` `^N` keys, and a big yes and no on the
confirm, drawn at the top, away from the button that opened it. A tap is a
press, so it acts once, and a tap in the first 300 ms of a new screen is
dropped, so a double tap never confirms; the wheel or a swipe scrolls. A
multi-line send still needs the yes button, which is `y`, never Enter. Keys
keep working.

```sh
set -g @cockpit_panel_mode auto    # the default; pc or tablet forces one
```

The Home tile `Tablet mode` / `PC mode` flips it until the panel closes. PC
mode never captures the mouse, so dragging over the popup still selects text.
[`examples/termux.properties`](examples/termux.properties) adds a Termux
extra-keys row with a key that opens the panel.

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

## prefix + M — move a window between sessions

One row per other session, plus `new session...` which asks for a name. Stock
`prefix + .` moves the window but leaves you behind in the old session, and if
that window was the session's last, tmux destroys the session and detaches
you: you are out of tmux. This binding switches the client first, then moves,
so the emptied session is destroyed with nobody in it and you land on the
window you moved. A new session gets exactly the moved window, no placeholder
shell beside it. `prefix + .` is untouched for the "renumber to index N" case.

The Shift is optional: `prefix + a m` opens the same menu from the agents
menu, and the example config's `prefix + m` menu has a `Move window...` row.
Both replay the key, so they follow `@cockpit_move_key`.

## prefix + a — the agents menu

Every cockpit action as a row with its letter on screen: jump `u`, review
`r`, panel `f`, pane info `i`, full screen `e`, branch `b`, move window
`m`, sessions `s`.
A row replays the key the action is bound to, so a rebound key moves its row
and an `off` key drops it. `@cockpit_ai_key` rebinds,
`off` disables. Stock `a` is unbound.

## From a phone

Give the phone app its own session, never the desktop's:

```sh
tmux new-session -A -s phone
```

A grouped session (`new-session -t`) shares its window size, so attaching from
the phone shrinks the desktop to phone width. An independent session leaves it
alone. Do not link windows between the two, for the same reason.

Both example configs carry the fix for what a phone terminal shows otherwise:
copies of the status line stacking upward, prompts erased as you type.

```sh
set -as terminal-overrides ",xterm-256color:am@"
```

tmux writes the bottom-right cell of the last row unless the terminal's
terminfo lacks `am`. A phone terminal wraps at that cell instead of holding the
wrap pending, so every full-width status repaint scrolls the screen one row and
leaves the old status behind (#97). `am@` costs one blank cell at the end of
the status line. It is keyed to `xterm-256color`, the TERM phone apps send, so
a desktop client still uses its last column. It also stops tmux letting the
terminal wrap long lines by itself, measured at +2.5% bytes on 260-column lines
wrapped into 131 and +4.3% on a full-screen fill, with identical output.

`prefix + e` hides the status line and the pane borders for this session and
window, and puts them back: a phone shows about 58 rows and the chrome is three
of them. `@cockpit_fullscreen_key` rebinds it, `off` keeps stock tmux.

Never combine the override with `status-position top`. Either one stops the
ghosting on its own; together they drop a visible glyph from the bottom pane
row. Full screen puts a pane row on the last line for the same reason, so the
bottom-right cell of the pane stays blank while the chrome is hidden. That is
the cost of the override, and it is the only one.

## tm — session launcher

`prefix + s` opens it in a popup sized to its sessions: no empty half-screen.

```sh
tm <name>     attach to <name>, creating it if missing
tm            pick a session; a name nothing matches + ↵ creates it
tm --needs    the picker, only the sessions waiting on you or in error
tm -k [name]  kill sessions (no name = pick them, Tab selects several)
```

Each row reads `1 ▲  api-gateway  2 agents  2 wins  ◉  5m`: a jump number, the
worst agent state in the session (`▲` needs you, `✕` error, `●` running, `✓`
done, `○` idle, blank for no agent), the name, counts, `◉` when another client
is on it, and when you last used it. Sessions that need you come first, the
rest by last use, and your own session last with `(here)`, so `prefix + s ↵`
goes back to the session you just left. The order holds still while the
picker is open; on a desktop the glyphs refresh every 2 s.

In the picker: typing **filters**. `↵` switches, or creates the typed name when
nothing matches; `alt-↵` creates it even when something does. `^X` arms a kill
and names what you would lose; `^X` again kills, any other key cancels. Your
own session refuses (`tm -k <name>` still kills anything, by name). `^/`
toggles the preview: the session's windows with their states, the waiting
agent's question, and the tail of its most urgent pane. `1`-`9` jump while the
query is empty. It uses `switch-client` inside tmux and `attach-session`
outside, and checks `session_attached` rather than `$TMUX` to tell the
difference — anything launched from a pane inherits `$TMUX`, so a terminal
opened inside tmux still carries it after the session is detached.

**Touch mode** turns on below 100 client columns (`set -g @cockpit_touch
auto|on|off`, default `auto`): rows are two lines tall, a waiting session shows
its question on the second, one tap switches, and the footer is a row of
buttons, `[ + new ] [ kill ] [ view ] [ close ]`. `set -g @cockpit_glyphs
ascii` swaps the glyphs for `! x * + .` where a terminal draws them at the
wrong width; `nerd` uses Nerd Font icons instead, opt-in because many Nerd
Font builds draw them two cells wide.

Without fzf, `tm` lists the sessions and reads a name.

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

745 cases. They run against a dedicated scratch pane, never a live one. They skip rather than fail when no tmux server is running.

The path-extraction cases drive `tmux-open-path --dry-run`, so they need neither
tmux nor an editor. The branch cases build a scratch repo with `git commit-tree`
rather than `git commit`: no user config, no hooks, no signing prompt.

Four faster ways to run them, each for a different point in the loop:

```sh
tests/only.sh PATTERN   # one block group by title, while iterating: seconds
make test-changed       # only the blocks a dirty file could have broken: sub-second to ~20s
make -j2 check-par      # lint beside a 12-way sharded suite, before a commit: ~40s
make gate               # check-par, skipped via a content-keyed stamp if nothing changed: 0.03s green, else ~40s
```

`make test` and `make check` are unchanged and stay the merge gate — one
process, source order. The six blocks marked `# group: serial` (a 50-pane
fleet scan under 5 s, a 5000-row `--lines` build under 3 s, 50 MB of
lockfile fixtures, …) run in
their own serial shard, alone, before the rest: they measure the box's spare
CPU as much as the code, and shard them apart and they flake under contention.

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

The review state is per-pane for the same reason. `@cockpit_review`
(`pending` or `sent`), `@cockpit_review_spool`, `@cockpit_title` and
`@cockpit_session` are all pane options, so a killed pane takes them with it
and no `◆` can outlive the agent it counted. An agent that merely exits leaves
its pane behind, so the `SessionEnd` hook clears the two review ones itself:
a queued review nobody can drain, or a pending mark on a pane with no agent in
it, would otherwise keep counting toward `◆N` forever.

## Deliberately unwired

`SubagentStop`, `TaskCompleted`, `PreCompact`.

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

## Known limits

- A session started **before** a hook was added never fires it — Claude Code
  snapshots hooks per session id. Such a pane stays grey until it restarts.
- A long "thinking" phase fires no hook, so a working pane can read as idle.
  CPU is not a usable substitute: an agent waiting on the API is
  indistinguishable from an idle one.
