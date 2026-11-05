# Board and panel

## The board (cockpit-tui)

`prefix + w` opens every agent pane in a list grouped by tmux session: the
state glyph, what the agent is doing and how long it has been doing it. The
sessions holding a blocked agent sort first, and a folded session still shows
its blocked agents. `v` swaps to the tiles, one live `capture-pane` per agent.
`↵` jumps to the agent you are on and closes the board. `/` narrows the list
as you type, over the session, window, title, state word and last line; `↵`
keeps the filter and `esc` clears it.

See [the board screen](tui.md#the-board) for what it looks like.

It is the plugin's required binary, `bin/cockpit-tui`, fetched by `install.sh`
(or by `scripts/tui-fetch` by hand, or built with `make tui`, Rust >= 1.88).
Without it the binding prints one message and exits 0.
The board is the fleet view: every agent pane, in the list or the tiles, and
the one place to act on them, one agent at a time.

The binary renders and decides; it never acts. It writes one line,
`<verb> <pane-id>`, and the binding runs the same `tmux-jump` / `tmux-review`
script the keys already use. After `r` review or `f` panel the board comes
back on the same pane, with the outcome as a note in its footer; a failure
(`✕`) stays until a key. `i` never leaves it: the pane's info draws over the
board. `@cockpit_board_return off` makes `r` and `f` one-way. The last
selection, the folds and a kept filter are kept in
`${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/board.state`, so the next
`prefix + w` opens the same way.

Keys, options, the screens, the states and the engineering rules:
[docs/tui.md](tui.md).

## The panel (cockpit-tui)

The panel asks a text-only `claude -p` about the pane you are in and pastes
the answer back into it. It is a port of type-panel. Claude runs with
`--tools ""`, so it sees only the screen text it is sent and can neither edit a
file nor run a command. The one exception is opt-in: with explore on (`^R`), a
write may also read files in the pane's repo (or its folder outside a repo) with
Read, Grep and Glob, and still cannot write. Its reads stop at that folder, the result notes the files it
read, and what it reads is neither redacted nor trusted: read the result before
you send it. The panel itself writes its system prompt to a cache
file and the answer to a private temp file for the paste.

- **Compose**: type rough words. `^S` writes a prompt, command, steps,
  text or hint from them, `^F` fixes the spelling, `^T` inserts them as
  typed. `⇥` picks the output type and `^N` the model. `^R` turns on
  explore first: the write may read a few files in the pane's repo, so
  the prompt names real files and functions (local windows only, never ssh,
  `/` or your home folder; up to USD 0.15 and 90 s per call). The box has a
  cursor: `←` `→`, `^←` `^→` by word, `Home` `End` (`^A` `^E`) and `Del`;
  `⌫`, `^W` and `^U` delete before it.
- **Explain**: what the pane is doing, what should happen next, or your
  own question. When the next step is a message to a Claude Code agent,
  the answer ends with that message, ready to insert or send.
- Claude may ask up to 3 multiple-choice questions first (6 for Explain):
  `1`-`4` picks, `o` types your own answer, `g` tells it to guess. A write
  that still asks with no questions left is told once to write it anyway,
  with what it assumed as `Assumed:` lines; the error shows only if it asks
  again.
- On a result, `i` inserts it into the pane without Enter, and `s` sends it
  with Enter after a confirm. The paste is bracketed only when the program
  asked for that, so a multi-line insert into a program that may run each
  line (python3, sh, psql) asks first too. A multi-line send takes `y`,
  not Enter. `n`, `q` or `esc` cancels it.
- When a written result ends with `Assumed:` lines, `a` corrects one: `⇥`
  picks which, and Enter writes it again with your correction. That takes
  one of the 3 questions, but a correction is still sent when none are left.
- On steps, `1`-`9` or `↑↓` pick one, `i` inserts it and `s` sends it. The
  panel comes back on the steps with that one marked `✓ inserted` or `✓ sent`
  (`+` with `COCKPIT_GLYPHS=ascii`), so you can go on to the next.
- `↑↓` and `PgUp` `PgDn` scroll a long result, answer or confirm. `esc` or `q`
  on an Explain answer asks once before it drops the conversation.
- `?` lists every key of the screen you are on (`F1` inside a text box, where
  `?` is text). `esc` steps back one screen, and the footer names where to:
  Home, the draft, the answer. On Home it closes the panel.
- `^Z` hides the panel, so you can switch windows while it is open: the
  popup closes and keeps where you were (the draft and its cursor, the
  question, the result, the steps, the answer). The next `prefix + f` on the
  same pane brings it back as it was; nothing is inserted or sent until you
  press the key. Another pane's panel starts fresh and the hidden one stays.
  Not while Claude is working: `esc` cancels the call first. From the board,
  `^Z` goes back to the board. A hidden panel is dropped after 24 hours, or
  once its pane is gone.
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
in place of the `^S` `^F` `^T` `⇥` `^N` `^R` keys, and a big yes and no on the
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
