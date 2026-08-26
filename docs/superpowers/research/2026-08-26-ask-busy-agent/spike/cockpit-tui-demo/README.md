# cockpit-tui

A Ratatui demo of tmux-cockpit's #54 screens, built to `screens.md` and to the
contract in `tui.md` §6: the binary reads tmux and the transcripts, decides, and
prints an action; a shell wrapper performs it. Nothing here writes to a tmux
server.

    cargo build --release
    ./demo.sh            # a private `tmux -L demo` server with four fake agents
    ./capture.sh 108 22 fleet

    cockpit-tui fleet        # the list, ↵ jump / r review / f fix / q close
    cockpit-tui board        # one tile per agent, live preview
    cockpit-tui info %14     # the info screen
    cockpit-tui ask %14      # the observer ask
    cockpit-tui --rows       # headless, tab-separated
    cockpit-tui --json       # headless, JSON

| env | |
|---|---|
| `COCKPIT_TMUX` | the tmux command, e.g. `tmux -L demo -f /dev/null` |
| `COCKPIT_ACTION` | file the chosen action is written to; without it, stdout |
| `COCKPIT_LOG` | append tick timings here |
| `COCKPIT_ASK_STUB=1` | stub the observer instead of calling claude |
| `COCKPIT_GLYPHS=ascii` | the ascii glyph tier |

`COCKPIT_BLESS=1 cargo test` rewrites the snapshots in `tests/golden/`.
