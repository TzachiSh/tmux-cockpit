# Menus

## prefix + M — move a window between sessions

One row per other session, plus `new session...` which asks for a name. Stock
`prefix + .` moves the window but leaves you behind in the old session, and if
that window was the session's last, tmux destroys the session and detaches
you: you are out of tmux. This binding switches the client first, then moves,
so the emptied session is destroyed with nobody in it and you land on the
window you moved. A new session gets exactly the moved window, no placeholder
shell beside it. `prefix + .` is untouched for the "renumber to index N" case.

The Shift is optional: the menu's `Move window to a session...` row (`prefix +
a M`) opens it too. The row replays the key, so it follows `@cockpit_move_key`.

## prefix + a — the menu

One menu for the cockpit and the window: `prefix + a` opens it centred, and
tapping `≡` at the top left of the border (or right-clicking `‹` or `›`)
opens it where you tapped. Every row has its letter on screen: the board `w`,
the panel `f`, what each agent is doing `t`, browse folder `o` (the pane's folder
in the editor), switch branch `b`, new window `c`, split right `\` and down `-`, move window
`M`, rename window `,`, kill pane (close window on a single pane) `x`, full
screen `e`, pane info `i`, rename session `$`, detach `d`, reload `R`, kill
session `Q`. Kill pane, close window and kill session ask first.

A row is there only when nothing on screen does its job: Browse folder and
Switch branch appear when the border has no room for the path or the branch
(or `prefix + e` hid it), and the splits are left out on a touch client.

A row for a plugin key replays that key, so a rebound key moves its row and an
`off` key drops it. `@cockpit_menu_key` rebinds the menu (the old
`@cockpit_ai_key` still works, with a notice), `off` disables it; moving it
gives the old key its stock binding back. Stock `a` is unbound.
