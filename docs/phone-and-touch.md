# Phone and touch

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
