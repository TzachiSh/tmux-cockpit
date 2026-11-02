# tm

The `tm` command stays; its fzf picker is being replaced by a cockpit-tui picker (#228).

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
