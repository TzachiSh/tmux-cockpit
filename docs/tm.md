# tm

The `tm` command stays; its picker is `cockpit-tui sessions` (#245, #228), not fzf. See "Sessions" in [tui.md](tui.md) for the keys and the action contract.

## tm — session launcher

`prefix + s` opens it in a popup sized to its sessions: no empty half-screen.

```sh
tm <name>     attach to <name>, creating it if missing
tm            pick a session; a name nothing matches + ↵ creates it
tm --needs    the picker, only the sessions waiting on you or in error
tm -k [name]  kill sessions (no name = pick one: ^X twice)
tm -l         list sessions
```

Each row reads `1 ▲ api-gateway  2 agents  2 wins  ◉  5m12s`: a jump number, the
worst agent state in the session (`▲` needs you, `✕` error, `●` running, `✓`
done, `○` idle, blank for no agent), the name, counts, `◉` when another client
is on it, and when you last used it. Sessions that need you come first, the
rest by last use, and your own session last with `(here)`, so `prefix + s ↵`
goes back to the session you just left. The order holds still while the
picker is open; the glyphs refresh every 2 s.

In the picker: typing **filters**. `↵` switches, or creates the typed name when
nothing matches; `alt-↵` creates it even when something does. `^X` arms a kill
and names what you would lose (agents, windows, attached elsewhere); `^X` again
kills, any other key cancels. Your own session refuses (`tm -k <name>` still
kills anything, by name). `F2` renames. `1`-`9` jump while the filter is empty.
The preview, when the popup is 60 columns or wider, shows the session's windows
with their states and the question a waiting agent asks. After a kill or a rename
the picker comes back. `tm` uses `switch-client` inside tmux and `attach-session`
outside, and checks `session_attached` rather than `$TMUX` to tell the
difference — anything launched from a pane inherits `$TMUX`, so a terminal
opened inside tmux still carries it after the session is detached.

`tm` acts on what the picker wrote only after checking it again: the target must
be a session id, and the name the picker showed must still be the session's live
name, so a kill never lands on a session renamed or replaced since the screen was
drawn. A refusal says why with a `✕`.

**Touch.** A click selects a row and a click on the selected row switches; there
is no separate touch layout. Below 100 client columns the popup is full width
(`set -g @cockpit_touch auto|on|off`, default `auto`). `set -g @cockpit_glyphs
ascii` swaps the glyphs for `! x * + .` where a terminal draws them at the wrong
width.

A client under 14 lines high has no room for a popup and its border, so `prefix + s` opens `tm` in a window of the client's session instead; the window closes when `tm` is done. Under 13 lines even that is too short and the screen asks for a larger terminal.

Without a terminal, or without `cockpit-tui` (see `make tui`), `tm` lists the
sessions and reads a name.

A `cockpit-tui` built before the plugin gained a screen exits 2 with `unknown
screen sessions` (#276). `tm` then says "cockpit-tui is older than the plugin"
and names `make tui` and `scripts/tui-fetch`; `prefix U` fixes it only with
`@cockpit_tui_fetch on`. Exit 2 without that line keeps the plain "exited 2".
