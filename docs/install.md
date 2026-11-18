# Install

## Install

```sh
git clone <this repo> ~/.tmux/plugins/tmux-cockpit
~/.tmux/plugins/tmux-cockpit/install.sh    # symlinks tm and the two tmux-* commands into ~/.local/bin
```

Add to `~/.tmux.conf` (see `examples/tmux.conf` for the full block):

```tmux
run-shell '~/.tmux/plugins/tmux-cockpit/cockpit.tmux'
```

That one line draws the status bar and borders and binds the keys — there is
nothing else to add for them. It must come **after** the cockpit options you
set, and before tpm.

Keep the single quotes. tmux hands what is between them to `sh` as it is, and
`sh` turns `~` into your home directory without splitting it, so the line also
loads when the path to your home holds a space. Without the quotes tmux expands
`~` itself and `sh` splits the result at the space: nothing loads. tmux says
nothing when it starts, and `'<path>' returned 127` when you reload the config.

If the plugin sits somewhere else and that path holds a space, put the path in
double quotes inside the single ones, with `$HOME` in place of `~` (`sh` leaves
a `~` inside quotes alone):

```tmux
run-shell '"$HOME/my plugins/tmux-cockpit/cockpit.tmux"'
```

A tmux server started with `HOME` unset or empty does not load the plugin with
either line: `sh` has no home directory to put in place of `~`. Give the full
path there, in single quotes inside double ones so that a space in it holds:
`run-shell "'/full/path/to/tmux-cockpit/cockpit.tmux'"`.

`bootstrap.sh` appends a line of that last shape to an existing config, with
the plugin folder's own path in it, escaped so that tmux and `sh` both read it
as one word whatever the path holds. Before #1046 it appended the path with no
quotes. That older line still loads the plugin from a folder whose path is
plain, and `bootstrap.sh` leaves it alone. From a folder whose path holds a
space, a quote, a `$` or another character `sh` reads, it might not load. There
`bootstrap.sh` names the line's number and prints the line to put in its place,
and `install.sh --check` gives the same as a warning on its config row. Neither
edits your config.

Both say the same about a line that holds the full path in single quotes
alone, or in double quotes alone, with or without `-b`. tmux takes those quotes
off, and `sh` reads the path unquoted, so a space in it splits it. In
`'~/<rest>'` only the rest counts: `sh` puts your home directory in for the `~`
and does not split it (#1303).

`install.sh` only ever creates symlinks, skips a name already taken by a real
file or by another tool's link, reports anything it could not create, and
`--uninstall` removes only links that point back into the plugin (and reports
anything it could not remove). `install.sh --help` prints usage and exits.

A link you make yourself works too, to any of the plugin's scripts: the commands
`install.sh` links (`tm`, `tmux-branch`, `tmux-open-path`), the rest of
`scripts/` with the hooks, and `cockpit.tmux`, `install.sh` and `bootstrap.sh`
themselves. It may have a relative target, as GNU stow makes, or be a link to a
link. Each script follows the chain to the real file, reads a relative target
from the folder of the link that holds it, and finds `lib/`, its sibling scripts
and the plugin folder beside the real file (#1283, #1290). Before that only one
step was followed, a relative target was read from the folder you ran the
command in, and a few scripts took the link's own folder for the plugin's. A
script run by a bare name from its own folder (`bash agent-state`) finds them
too (#1287).

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

When `cockpit-tui` is missing or older than the plugin needs, the plugin says so
on the status line, and that hint waits for you: tmux loads its config before
the first client attaches, so the first client to attach is shown it, and it
stays until you press a key. A reload shows it in place of "config reloaded".
These two are the only hints that wait. The plugin's other messages, such as
the one about `set -g mouse on`, show only to a client attached during the
load, as on a reload.

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

Debian's and Arch's packages install tpm as
`/usr/share/tmux-plugin-manager/tpm`, and `install.sh --check` takes a run
line for that path as tpm's too. A tpm anywhere else counts when its path ends
in `/tpm/tpm`, as a clone of tpm has it. When its config row fails on an
`@plugin` line, the fix column names each check that failed: the line sits in
a config tpm does not read, no line runs tpm, or tpm's plugin folder holds
another copy (#1302).

tpm also reads `@plugin` lines from a file its config sources with
`source-file`, one level down, and tmux runs tpm's run line from such a file
too. `install.sh --check` and `bootstrap.sh` read both there, one level down,
by tpm's own rules: a `~` or `$HOME` at the start of the path is your home, a
glob is expanded, and a path that does not start with `/` is not read. A
`run-shell` line for `cockpit.tmux` in such a file stops `bootstrap.sh` from
appending one beside it. `install.sh --check` does not count it as a load, so
its config row fails (#1304).

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
- GNU `timeout` bounds the hook's payload reads, the agent-view poller's `claude agents`
  and the board's `workdir` call. On macOS, `brew install coreutils` bounds them (as
  `gtimeout`); stock macOS without coreutils runs them unbounded.
- A Nerd Font is *not* required. The markers are plain Unicode geometry.
