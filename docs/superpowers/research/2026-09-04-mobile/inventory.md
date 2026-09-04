# repo-inventory: cockpit surfaces that assume a wide client

Lane repo-inventory, issue #97, 2026-09-04. Read-only research on private `-L` sockets.

## Question

Which cockpit surfaces break, truncate, overflow or become unreachable on a client of 60x30 and of 131x58, and where in the code does each come from?

Rig: tmux 3.7c at `~/.local/bin/tmux`, fzf 0.44.1, private `-L` sockets only, a Python pty at each size, frames reconstructed with pyte. Two configs: `conf-example` is `examples/tmux.conf` plus the test palette and the worktree `cockpit.tmux`; `conf-full` is `examples/full.tmux.conf` minus tpm, resurrect and continuum, the shape of the owner's live file. Artifacts under `/tmp/repo-inventory-1957678`.

One rig defect found and corrected mid-run: `/usr/bin/tmux` is 3.4, `~/.local/bin/tmux` is 3.7c. The first key dump used 3.4 and showed no control ranges. Every measurement below was re-taken on 3.7c.

## Inventory

### 1. Status line

| surface | file:line | at 60x30 | at 131x58 | verdict |
|---|---|---|---|---|
| whole status line, 1 window | `examples/full.tmux.conf:264,268,273,275` | `  tests  󰊢  research/mobile-97 ⑂research-mobil 󰃰  07:40` | `  tests  1  bash    ~/.cache/cockpit-wt/research-… 󰊢  research/mobile-97 ⑂research-mobil 󰃰  07:40` | CONFIRMED |
| window tab list | `full.tmux.conf:214-215`, `tmux.conf:52-53` | absent with 1, 2 or 3 windows | all three tabs drawn | CONFIRMED, breaks |
| directory segment, `range=control|2` | `full.tmux.conf:258,268`, live `~/.tmux.conf:233` | not drawn at all | drawn, cols 48-83 | CONFIRMED, breaks |
| branch segment, `range=control|1` | `full.tmux.conf:262,273`, live `~/.tmux.conf:238` | drawn, cut to `⑂research-mobil` | drawn, same cut | CONFIRMED, degraded at both |
| `status-right-length 140` | `full.tmux.conf:254`, live `:219` | never binds, the client is the limit | never binds | CONFIRMED, inert |
| `status-left-length 40` | `full.tmux.conf:253`, live `:218` | never binds, segment is 10 cells | never binds | CONFIRMED, inert |
| `#{agent_fleet}` | `cockpit.tmux:78`, `tmux.conf:74` commented | renders nowhere | renders nowhere | CONFIRMED, matches #58 defect 4 |
| `#{agent_where_path}` / `_branch` | `cockpit.tmux:60-61,141-145` | both run, path segment then dropped by truncation | both drawn | CONFIRMED |

The truncation direction is measurable. At 80 columns the line read `  tests ockpit-wt/research-… 󰊢 …`, so tmux cuts status-right from the left and slices the path mid-word with no ellipsis. Bisect with two windows, owner config:

| client width | window tabs |
|---|---|
| 80 | none |
| 96 | `<a` |
| 104 | window 1 only, `>` marker |
| 112 | window 1 plus `2  ed>` |
| 120 | both tabs whole |

The #58 finding that fleet columns get cut under 150 columns is confirmed at a new width and not re-derived. See `docs/superpowers/research/2026-08-26-ui-audit/README.md` and issue #58 defects 5 and 6.

### 2. Pane border

| surface | file:line | at 60x30 | at 131x58 | verdict |
|---|---|---|---|---|
| `pane-border-status top` | `full.tmux.conf:224`, `tmux.conf:31` | on | on | CONFIRMED |
| NEEDS YOU, owner config, no question text | `full.tmux.conf:235` | `─── NEEDS YOU ───…───[z][x]` fits | same, fits | CONFIRMED |
| NEEDS YOU with the 48-char question | `tmux.conf:34` | `─── NEEDS YOU Should I force-push the rebased branch to orig` and `[z]` and `[x]` are pushed off the line | `…origin…` plus `[z][x]` at the right edge | CONFIRMED, breaks |
| `[z]` `[x]` click targets | `tmux.conf:34`, `full.tmux.conf:235` | present when idle, gone when the pane is blocked | always present | CONFIRMED |

At 60 columns the two mouse-only pane buttons disappear exactly in the state where a phone user most needs them. A 60-column pane inside a wider client behaves the same way, since the format is per pane.
