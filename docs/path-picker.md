# Path picker

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
