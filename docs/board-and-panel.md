# Board and panel

## The board (cockpit-tui)

`prefix + w` opens every agent pane as a live tile: the state glyph, what the
agent is doing, how long it has been doing it, and a `capture-pane` of the pane
itself. Blocked agents sort first, so page 1 is the triage page. `↵` jumps to
the tile you are on and closes the board.

See [the board screen](tui.md#the-board) for what it looks like.

It is the plugin's required binary, `bin/cockpit-tui`, fetched by `install.sh`
(or by `scripts/tui-fetch` by hand, or built with `make tui`, Rust >= 1.88).
Without it the binding prints one message and exits 0.
`prefix + r` remains the fleet list: fuzzy filter, multi-select and the wheel
come free from fzf, and the board is the grid of previews fzf cannot draw.

The binary renders and decides; it never acts. It writes one line,
`<verb> <pane-id>`, and the binding runs the same `tmux-jump` / `tmux-review`
script the keys already use.

Keys, options, the screens, the states and the engineering rules:
[docs/tui.md](tui.md).

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
variables and the action file are in [docs/tui.md](tui.md).

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
[`examples/termux.properties`](../examples/termux.properties) adds a Termux
extra-keys row with a key that opens the panel.
