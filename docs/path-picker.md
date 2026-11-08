# Path picker

## Ctrl+double-click a line to pick a path off it

**Ctrl+double-click** any line of pane output. Every whitespace-separated token
on that line that resolves to a real file becomes a candidate.

**One candidate opens at once**, a folder included: there is nothing to choose,
so no popup appears. **Several** open a `cockpit-tui pick` popup with the list:
`↵` opens the highlighted row, `esc` cancels, and typing filters. When the line
yields nothing, the status line says so and no popup appears.

It reads `file:12:3`, `file:12`, `file(12,3)` (tsc), `a/file` (git diff),
`~/file` and absolute paths, and strips the quotes, brackets and trailing commas
terminals wrap paths in. It also unwraps the `Name(path)` shape agents print
their tool calls in — `Write(docs/spec.md)`, `Read(src/app.ts:31)` — where the
path is real but the token around it is not; `file(12,3)` stays a *location*,
not a wrapper. Existence on disk is the filter, so a word that merely looks
like a path is dropped.

**`alt-c` copies the highlighted row's path** instead of opening it: the
absolute path goes to the tmux buffer and, as OSC 52, to the clipboard, for
pasting into an agent's prompt. It reaches the system clipboard only if your
terminal accepts clipboard writes. The footer names the key.

To copy something no key already copies, hold **shift** and drag. A tmux popup
has no copy mode, so in any of these popups the terminal's own selection is the
one that works.

**The folder keys browse the pane's folder.** The menu's `o` row (Browse folder),
the cockpit table's `o` and a tap on the path on the pane border open `cockpit-tui files` in a
popup on the pane's current folder (#363). Type to filter; `↵` on a folder goes
into it, `←` (or `⌫` on an empty filter) goes up, and `.` on an empty filter shows
or hides dotfiles, `alt-i` shows or hides what `.gitignore`,
`.ignore` and git's global excludes hide (shown entries are drawn dim; `.git` is
never listed). The footer names both keys and their state. Folders sort first. `↵` on a file shows it in the viewer, `alt-e` opens it in the editor, `alt-c` copies its
absolute path (a folder's too), `esc` leaves. The header shows the folder, cut on
the left when it is long, and browsing above the repo root is allowed.

`↵` sends any regular file to `cockpit-tui view` (a popup of its own, drawn in place from the
browser's popup), on a touch client too; `@cockpit_md_view off` or a missing binary falls back to the editor
routing below. `alt-e` opens a file the way a clicked path does (VS Code, or `@cockpit_editor`), except
on a touch client (`@cockpit_is_touch`, see [status-bar.md](status-bar.md)): VS
Code would open on the PC's screen and show a phone nothing, so the file opens in
a terminal editor window next to the pane. The editor is `@cockpit_editor`, else
`$VISUAL`, else `$EDITOR`, else `nvim`, else `vim`; the window is named for the
file and starts in its folder. With none of them found the status line says so.

`q` or `esc` in a viewer opened from the browser goes back to the browser, in the
viewed file's folder with that file selected; its footer says `q back`. `esc` in
the browser then closes the popup. `e` in the viewer ends the popup as before,
with the editor open. `?` opens the ask panel and then the viewer again, whose
`q` goes back to the browser too. When the file's folder is gone by then, the
browser comes back in the folder it started in. A `cockpit-tui` without
`--select` still reopens the folder, on its first row.

The browser prints one line and `tmux-open-path --folder` checks it before acting:
one line, the verb `view`, `open` or `copy`, an absolute path that exists (a regular file for `view`). A file name
with a newline in it cannot be sent, so the browser says so and prints nothing.
Names are drawn without control and bidi characters; the printed path is exact.

**A markdown file opens in the viewer.** When the one path to open is a
`.md`, `.markdown`, `.mdown` or `.mkd` file (any case), it opens rendered and
read-only in `cockpit-tui view`, in a popup the size of the pane, or in the
picker's own popup when it was chosen there (tmux has one popup per client).
`e` closes the viewer and opens the file in the editor, at the source line of
the top row it showed; a clicked `README.md:40` opens the viewer at line 40.
`set -g @cockpit_md_view off` sends markdown to the editor directly, as before, and
the file browser's `↵` stops viewing too: every file goes to the editor. On a touch client the
viewer's `e` opens the terminal editor (see above), at the line when the editor takes `+N`
(vi, vim, nvim, nano, emacs, micro), instead of VS Code on the PC.
The viewer's keys are in [docs/tui.md](tui.md#the-markdown-viewer-283).

Rows keep their line numbers — a clicked `src/app.ts:31` reads as
`src/app.ts:31` in the list, not a bare filename.

A `cockpit-tui` older than the plugin does not know `alt-c`'s flag: the click says
so, naming `make tui` and `scripts/tui-fetch`, instead of failing quietly.

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
also pairs with the branch popup, which is `Ctrl`+click on the status bar.

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

Keep it a **terminal** editor: it runs inside that new tmux window. A touch
client (`@cockpit_is_touch`) never gets VS Code: a click there opens
`@cockpit_editor`, else `$VISUAL` or `$EDITOR`, else nvim or vim, in a window of
its own.

`@cockpit_editor` wins. After it comes VS Code, either as `code` on the `PATH`
or as the remote shim Remote-SSH and Coder install under `~/.vscode-server`,
and only where neither of those exists does the click fall back to `$VISUAL` or
`$EDITOR`. On a Remote-SSH or Coder host that is a change: with `EDITOR=nvim`
and no `@cockpit_editor` set, a click now lands in the VS Code window you are
already looking at, where it used to open nvim in a tmux window. Set
`@cockpit_editor nvim` to keep nvim. With no editor anywhere it says so plainly
rather than reporting a success that never happened.
