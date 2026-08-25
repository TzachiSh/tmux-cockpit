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
- **Status bar** — the `#{agent_fleet}` token counts blocked and errored agents
  across *every* session, not only the one you are attached to.

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
| `#{agent_fleet}` | `▲N` blocked and `✕N` errored agents, every session |

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

It is pure format syntax — a redraw runs no shell — but unlike the other two it
is not free: tmux walks every pane of every session, twice, on each expansion.
That costs about **1.7 ms per expansion on a 40-pane server** (1.2 ms measured
on an idle machine with every pane blocked). Against a default
`status-interval` of 15 s that is nothing, and it is small beside the 7.7 ms a
six-window redraw already costs. Panes in neither state pay only their
`@panestate` read; the dearer agent-glob test runs on the few that are.
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
`--uninstall` removes only links that point back into the plugin (and reports
anything it could not remove). `install.sh --help` prints usage and exits.

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

Wire the hooks into `~/.claude/settings.json`:

```sh
~/.tmux/plugins/tmux-cockpit/install.sh --hooks             # merge them in
~/.tmux/plugins/tmux-cockpit/install.sh --hooks --dry-run   # preview the diff first
```

It merges additively — your own hooks are never touched, cockpit's are appended
per event only where missing, and re-running after an upgrade only adds what's
new (a reformatted settings.json, e.g. from `jq -S` or Claude Code rewriting
its own file, is still recognized and never re-appended). A `settings.json.bak`
is written before the first change (an existing `.bak` is never overwritten).
`--hooks --dry-run` previews the merge without writing or creating anything,
even on a fresh install with no `~/.claude` yet. Requires `jq`; without it, or
without Claude Code's settings.json at that path, merge the `hooks` object
from `examples/claude-hooks.json` by hand:

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
| `prefix + J` | jump to the pane that needs you (again: next one) — `@cockpit_jump_key` rebinds, `off` disables |
| `prefix + r` | review an agent's uncommitted diff — from an agent pane: that pane; anywhere else: the fleet queue, blocked agents first, then dirty ones (replaces stock `refresh-client`) — `@cockpit_review_key` rebinds, `off` restores stock |
| `prefix + R` | reload the tmux config — the file tmux actually loaded, XDG included — `@cockpit_reload_key` rebinds, `off` disables |

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

Where neither `@cockpit_editor` nor `code` is available it falls back to `$VISUAL`
or `$EDITOR`, and says so plainly if there is no editor at all rather than
reporting a success that never happened.

## Review the agent's changes

`prefix + r` opens a popup review of an agent's uncommitted diff. Comments
attach to individual lines, and nothing reaches the agent until you say so —
there is deliberately no key that approves the whole thing at once.

From an **agent pane** it reviews that pane. From anywhere else — a shell, a
build pane, wherever `prefix + r` finds you — it opens the **fleet queue**
instead: every agent pane, on every session, that is blocked or has a dirty
tree, previewed the same way the path picker's folder preview shows a
directory's `git status`. Nothing else in this plugin answers "which of my
agents needs my eyes"; the queue exists because visiting each pane in turn to
find out does not scale past two or three of them.

The order is that question's answer, and it is deliberately **not** churn.
Agent state ranks first — `needs-input`, then `done`, `error`, `running`, then
panes with no state at all — and churn only breaks ties inside a rank. An
agent parked on a permission prompt is the one actually waiting on you, and it
usually has nothing at all to diff; sorting by churn listed it last, or (with a
clean tree) left it out entirely, and answered "who typed most" instead. A
blocked pane is listed even with a clean tree, and its `±` column reads `—`
rather than `+0 -0`, because there is nothing to review there — only someone to
unblock.

Each row is `state · title · repo⑂branch · ±`, in aligned columns. The
`repo⑂branch` field is what tells two worktrees of the same repo apart, which
`s:w claude` alone cannot:

```text
needs-input   main:2 claude    cockpit⑂feat/review-p0     —
running       side:0 codex     cockpit⑂main               +142 -30  ?2 new
—             main:4 claude    api⑂fix/token-refresh      +8 -8
```

`tmux-review --fleet` prints those rows tab-separated with no fzf, if you want
to drive the same list from a script.

The fleet queue — and only it — takes extra fzf options from a global tmux
option, so you can bend the picker to your own taste without patching the
script:

```sh
set -g @cockpit_fleet_fzf_options "--height=80% --pointer='▶'"
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
| `alt-u` / `alt-d` | — | scroll the preview |
| `^S` | — | deliver the whole batch |
| `esc` | cancel | discard the batch — asks first when comments are pinned |

Inside the review: `↵` pins whatever you typed as a comment on the diff line
under the cursor — `alt-↵` does the same thing, kept as an alias for
terminals where the plain chord doesn't land. The list reloads in place and
marks the row `●`, so five comments cost one popup, not five — the same
`execute-silent(...)+reload(...)` shape `tm`'s kill picker already uses.
`^S` delivers the whole batch; `esc` discards it — and with comments
pinned it asks first (`keep reviewing` / `discard N comment(s)`), because
one reflexive escape should not throw away a twenty-comment review.
**`↵` pins a comment — it never approves.** With nothing typed it does
nothing: there is no accept-all key on purpose, a review whose only exit is
one Enter on the whole diff is the rubber stamp this replaces.

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
review sits there unsent. `prefix J` walks to those panes after the ones
whose agent is blocked on you, so a review you pasted and then forgot is
still reachable by a keypress instead of by memory. The mark clears the
moment the agent starts working again — pressing Enter is what fires the
`running` hook — so it never outlives the thing it describes.

Delivery can also be **refused** — the target is on a permission prompt, is
not an agent pane, or is not a pane id at all. A refusal keeps the batch: the
comment store is left on disk, and the message names it along with the
`--deliver <pane> <store>` that re-sends it — spelled with the script's own
resolved path, not a bare `tmux-review`, so it pastes back even from a
plugin-only install where nothing is on `$PATH` yet. Every refusal you can see
keeps the store. One window does not: the paste is gated a second time inside
tmux, and if the pane stops being an idle agent between the shell guard and
that gate, nothing is pasted but the store is still deleted. It is a few
milliseconds wide and closes when the delete moves inside the gate itself.

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
  created `0700`, holding `comments.md` and its row snapshot. It exists so a
  dropped SSH client — which tears down a client-owned popup — cannot take a
  twenty-comment review with it: `prefix + r` on the same pane resumes it,
  marks and all. It is removed when the review is discarded or delivered and
  kept, and named in the message, when delivery is refused. There is no
  garbage collector; `rm -rf ~/.local/state/tmux-cockpit/review` is the purge.
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

309 cases. They run against a dedicated scratch pane, never a live one. They skip rather than fail when no tmux server is running.

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
