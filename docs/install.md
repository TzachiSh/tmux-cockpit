# Install

## Install

```sh
git clone <this repo> ~/.tmux/plugins/tmux-cockpit
~/.tmux/plugins/tmux-cockpit/install.sh    # symlinks tm and the two tmux-* commands into ~/.local/bin
```

Add to `~/.tmux.conf` (see `examples/tmux.conf` for the full block; its
`status-right` directory and branch segments read catppuccin's
`@catppuccin_status_*` options, so without that theme they render nothing):

```tmux
run-shell ~/.tmux/plugins/tmux-cockpit/cockpit.tmux
```

That one line also binds the keys below — there is nothing else to add for them.
It must come **after** every option using an `#{agent_*}` token, and before tpm.

If the path contains a space, quote it for the shell *inside* the tmux string —
tmux hands the line to `sh`, which would otherwise split it and fail with 127:

```tmux
run-shell "'~/my plugins/tmux-cockpit/cockpit.tmux'"
```

`install.sh` only ever creates symlinks, skips a name already taken by a real
file or by another tool's link, reports anything it could not create, and
`--uninstall` removes only links that point back into the plugin (and reports
anything it could not remove). `install.sh --help` prints usage and exits.

`install.sh --check` is the doctor. It writes nothing and fixes nothing: one
row per thing that can be wrong (tmux, jq, fzf, bat, `$PATH` order, the
config, the hooks, truecolor, the popup border), the fix beside it, and the
exit is the worst row, 0 ok or skipped, 1 warnings, 2 something is broken.
Rows it cannot answer from where you ran it, such as truecolor outside tmux,
come out `skip` and count as 0. `bootstrap.sh --check` runs the same thing.

`bootstrap.sh --full` is the opt-in tier: it fetches pinned versions of fzf
and bat into `${XDG_BIN_HOME:-~/.local/bin}`, sha256 verified where the
project publishes checksums, and shadows a distro copy rather than replacing
it. Without `--full` it still fetches fzf alone when the one it finds is below
0.73. No sudo, and nothing is downloaded when tmux loads the plugin or when you
press a key. Add `--latest` for each project's newest tag instead of the pins.
Offline it skips the tool and carries on; a checksum it cannot fetch or cannot
match installs nothing. On macOS with brew it runs `brew install fzf bat`.

The board (`prefix + w`) is the one optional extra: a small binary, fetched with
`set -g @cockpit_tui_fetch on` (fetched in the background when the config loads),
`scripts/tui-fetch` by hand, or `make tui`. Everything else here works without it.
See [docs/tui.md](tui.md).

### Or with tpm

It is a normal tpm plugin — an executable `*.tmux` at the repo root — so:

```tmux
set -g @plugin 'catppuccin/tmux'          # declare it FIRST; the example
                                          # block's status-right needs it
set -g @plugin '<owner>/tmux-cockpit'
run '~/.tmux/plugins/tpm/tpm'             # last
```

tpm is the better route, because it runs plugins from that last line — after
every `set -g` in your config, which is exactly the ordering the `#{agent_*}`
tokens need. The one failure mode of the `run-shell` line, loading it too early
and getting the raw token printed in your status bar, cannot happen under tpm.

Order matters against catppuccin specifically: cockpit rewrites
`@catppuccin_directory_text` and `@catppuccin_gitmux_text`, so catppuccin has to
be declared first or it resets them afterwards.

Wire the hooks into `~/.claude/settings.json`:

```sh
~/.tmux/plugins/tmux-cockpit/install.sh --hooks             # merge them in
~/.tmux/plugins/tmux-cockpit/install.sh --hooks --dry-run   # preview the diff first
```

It merges additively — your own hooks are never touched, cockpit's are appended
per event only where missing, and re-running after an upgrade only adds what's
new (a reformatted settings.json, e.g. from `jq -S` or Claude Code rewriting
its own file, is still recognized and never re-appended). Re-run
`install.sh --hooks` after every plugin update: it also removes hooks a newer
version no longer ships, such as the old PreToolUse `review-gate --pretool`. A `settings.json.bak`
is written before the first change (an existing `.bak` is never overwritten).
`--hooks --dry-run` previews the merge without writing or creating anything,
even on a fresh install with no `~/.claude` yet. Requires `jq`; without it, or
without Claude Code's settings.json at that path, merge the `hooks` object
from `examples/claude-hooks.json` by hand:

The hooks and the state each one writes: [states-and-hooks.md](states-and-hooks.md#hooks).

## Requirements

- **tmux >= 3.8**. Below it the plugin says so and binds nothing; `bootstrap.sh`
  builds 3.8 into `~/.local/bin` when the distro's tmux is older.
- **Rust >= 1.88** only to build the board with `make tui`;
  the fetched binary needs neither Rust nor cargo.
- **fzf >= 0.73** for the pickers. Debian 13 ships 0.60.3, Ubuntu 24.04
  0.44.1 and Ubuntu 26.04 0.67.0, all below it, so `bootstrap.sh` fetches
  0.74.3 into `~/.local/bin` when it finds an older one, and
  `install.sh --check` fails below 0.73. `bat` (`batcat` on Debian) is
  optional; `jq` is needed by `install.sh --hooks` and the review
  gate.
- Claude Code (or any agent that can run a command on lifecycle events).
- A Nerd Font is *not* required. The markers are plain Unicode geometry.
