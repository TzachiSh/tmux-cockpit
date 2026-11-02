# Menus

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
