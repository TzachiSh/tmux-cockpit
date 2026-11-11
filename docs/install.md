# Install

## Install

```sh
git clone <this repo> ~/.tmux/plugins/tmux-cockpit
~/.tmux/plugins/tmux-cockpit/install.sh    # symlinks tm and the two tmux-* commands into ~/.local/bin
```

Add to `~/.tmux.conf` (see `examples/tmux.conf` for the full block):

```tmux
run-shell ~/.tmux/plugins/tmux-cockpit/cockpit.tmux
```

That one line draws the status bar and borders and binds the keys — there is
nothing else to add for them. It must come **after** the cockpit options you
set, and before tpm.

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
row per thing that can be wrong (tmux, cockpit-tui, jq, revdiff, `$PATH` order, the
config, the hooks, truecolor, the popup border), the fix beside it, and the
exit is the worst row, 0 ok or skipped, 1 warnings, 2 something is broken.
Rows it cannot answer from where you ran it, such as truecolor outside tmux,
come out `skip` and count as 0. `bootstrap.sh --check` runs the same thing.

`bootstrap.sh` fetches the pinned revdiff into `${XDG_BIN_HOME:-~/.local/bin}`
when none at the pinned version is on `PATH`, sha256 verified, with no sudo.
Nothing is downloaded when tmux loads the plugin or when you press a key.
Offline it skips the tool and carries on; a checksum it cannot fetch or cannot
match installs nothing. On macOS with brew it runs `brew install` for revdiff.
bat is not used any more (the previews it coloured went in #228), and `--full`
and `--latest` went with it.

The `cockpit-tui` binary is required: `install.sh` fetches it with your `gh`
login or builds it from source, and stops with a message when it can do
neither. It draws the board (`prefix + w`) and the panel (`prefix + f`). See
[tui.md](tui.md).

### Or with tpm

It is a normal tpm plugin — an executable `*.tmux` at the repo root — so:

```tmux
set -g @plugin '<owner>/tmux-cockpit'
run '~/.tmux/plugins/tpm/tpm'             # last
```

tpm is the better route, because it runs plugins from that last line — after
every `set -g` in your config, so the cockpit options you set are in place when
the plugin reads them. No theme plugin is needed: the plugin draws the bar and
its own palette ([status-bar.md](status-bar.md#colours)).

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
from `examples/claude-hooks.json` by hand.

`install.sh --uninstall --hooks` takes the hooks out again, besides removing
the links; `install.sh --hooks --remove` removes only the hooks and keeps the
links (`--remove` needs `--hooks`). It deletes only hook entries whose command
is one of this plugin's scripts (`agent-state`, `review-gate`, `worktree-create`,
`worktree-remove`, or the unslop `SKILL.md` injection) at the shipped
`${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}` path or this checkout's
path, matched whole. A matcher group left empty and an event left empty are
dropped; other hooks, other keys and key order stay. It writes a temp file in
the same directory and renames it over `settings.json`, after a
`settings.json.bak-<date>` backup. A missing file, or one with no cockpit
hooks, is a no-op that says so; invalid JSON is refused and left unchanged.
`--dry-run` with either prints the diff. `install.sh --hooks` adds
them back identically.

The hooks and the state each one writes: [states-and-hooks.md](states-and-hooks.md#hooks).

## Requirements

- **tmux >= 3.8**. Below it the plugin says so and binds nothing; `bootstrap.sh`
  builds 3.8 into `~/.local/bin` when the distro's tmux is older.
- **`cockpit-tui`**, the plugin's own binary. `install.sh` fetches the
  release through your `gh` login when it can reach it, and otherwise builds
  it, which needs **Rust >= 1.88**; the fetched binary needs neither Rust nor
  cargo. `make check` builds it, so the suite needs cargo.
- The shell suite builds an `en_US.UTF-8` locale with **`localedef`**, which needs the
  locale sources (on Debian and Ubuntu, the `locales` package). Without them its
  three UTF-8 digit blocks each fail one row rather than run in the C locale.
- No fzf: every picker is `cockpit-tui`. `jq` is needed by `install.sh --hooks` and
  the review gate.
- **revdiff** for the `prefix + r` review; tested with v1.13.0. Without it
  `prefix + r` says "revdiff not found" and `tmux-review --request` exits 1.
  Install it with `brew install umputun/apps/revdiff`, from a
  [release package](https://github.com/umputun/revdiff/releases) (deb, rpm or
  an archive). With Go 1.26 or newer, v1.13.0 installs as `app`:
  `go install github.com/umputun/revdiff/app@v1.13.0`, then rename it to
  `revdiff`. Other versions may use a different package path; see revdiff's
  README.
- Claude Code (or any agent that can run a command on lifecycle events).
- GNU `timeout` is not required: the hook's payload reads, the agent-view poller's
  `claude agents` and the board's `workdir` call are bounded by `timeout`, else
  Homebrew's `gtimeout`, else a watchdog of the plugin's own (stock macOS).
- A Nerd Font is *not* required. The markers are plain Unicode geometry.
