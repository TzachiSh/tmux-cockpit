# Testing

## Tests

```sh
tests/run.sh
```

745 cases. They run against a dedicated scratch pane, never a live one. They skip
rather than fail when no tmux server is running.

The path-extraction cases drive `tmux-open-path --dry-run`, so they need neither
tmux nor an editor. The branch cases build a scratch repo with `git commit-tree`
rather than `git commit`: no user config, no hooks, no signing prompt.

Four faster ways to run them, each for a different point in the loop:

```sh
tests/only.sh PATTERN   # one block group by title, while iterating: seconds
make test-changed       # only the blocks a dirty file could have broken: sub-second to ~20s
make -j2 check-par      # lint beside a 12-way sharded suite, before a commit: ~40s
make gate               # check-par, skipped via a content-keyed stamp if nothing changed: 0.03s green, else ~40s
```

`make test` and `make check` are unchanged and stay the merge gate — one
process, source order. A block marked `# group: serial` runs in its own
serial shard, alone, before the rest. The mark is for a block that measured
green at J=4 and red at J=8 and J=12: one that measures the box's spare CPU as
much as the code. No block carries the mark today. A `cpu_under` budget needs
no mark: each is 3x its site's CPU measured under twelve spinners, so it is
sized for contention already.

### A TMPDIR with a blank

Under a `TMPDIR` whose path holds a space or a tab, a run of `tests/run.sh`
(`tests/only.sh` and each `tests/par.sh` shard too) keeps its wrapper around
tmux outside it, in a folder it makes in the caller's runtime dir,
`$XDG_RUNTIME_DIR/cnb.<6>`, mode 700 (#992). Pane commands in the suite paste
that wrapper's path unquoted, so it must hold no blank. The wrapper's small
marker files sit beside it, `flop/k.*` and `flop/r.*`, and a run started inside
the run makes its own `cnb.<6>` in there. Three board blocks each add a stub
tmux, `ct.<6>/tmux`, which stays until the run ends. The run removes the folder
when it ends, also on a FATAL and on HUP, INT or TERM. A run ended by KILL leaves it,
with `flop/tmux` inside. Nothing sweeps it, and it is safe to remove once no
run is going.

The suite drops what its caller's environment could hand to a script under test:
`XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `XDG_BIN_HOME`, `XDG_CACHE_HOME`,
`XDG_DATA_HOME`, `COCKPIT_LOG`, `VISUAL`, `EDITOR`, and the switches
`COCKPIT_READONLY`, `COCKPIT_PANEL_NOTE`, `COCKPIT_REVIEW_BASE`,
`COCKPIT_PROC_DIR`, `COCKPIT_POLL_KEY`, `REVDIFF_CONFIG`, `REVDIFF_EXCLUDE`,
`CLAUDE_CODE_SESSION_KIND`, `COCKPIT_TMUX`, `COCKPIT_CLAUDE` and
`COCKPIT_ACTION` (#1078). A block that needs one sets it itself. Other names
the scripts and the board read are still left to the caller, none of which
names a place to write: `NO_COLOR`, `COCKPIT_GLYPHS`, the `COCKPIT_PANEL_*`
and `TM_*` switches and a dozen more, listed in #1100.
Before that fix a run from a shell that exported them wrote there. If you ran
the suite that way, you may find three symlinks in your `XDG_BIN_HOME`, `tm`,
`tmux-branch` and `tmux-open-path`, pointing into the checkout you tested (your
own install's links point into your plugin folder and are not these), a file
under `$XDG_CACHE_HOME/tmux-cockpit/panel/prompts/`, and lines from test boards
in the file `COCKPIT_LOG` names. The file and the lines are safe to remove. So
are the links, unless the checkout you tested is your plugin folder: there they
are your install's own links, with the same names and the same targets. With
`COCKPIT_TMUX` exported, the suite's test boards sent their tmux commands to the
server that value names: they listed its panes and added `wait-for` entries to
its global `@cockpit_board_wake` option. A board takes its own entry out when it
exits; one that was killed leaves an entry that signals a channel nobody waits
on. With `COCKPIT_CLAUDE` exported, one panel block ran the program it names as
`claude -p`, with the test pane's text: a model call, if it was the real one. A
run without
those variables exported left nothing: everything went under the suite's own
`HOME`, which it deletes.

With no runtime dir to use, such a run ends at its start and says what it needs:
set `XDG_RUNTIME_DIR` to a folder that exists, can be written to and has no
blank in its path.

### The suite's tmux sockets

Every tmux server the suite starts is on a socket of its own name, under
`TMUX_TMPDIR`. With no `TMUX_TMPDIR`, or one naming a folder that is not there,
tmux would put those sockets in `/tmp/tmux-<uid>`, beside your own server's. So
a run of `tests/run.sh` (`tests/only.sh` and each `tests/par.sh` shard too)
then makes a folder of its own, `/tmp/cs.<6>`, mode 700, and exports it as
`TMUX_TMPDIR` before its first tmux call (#1098). It is in `/tmp` and short
because a socket's path can hold 107 bytes. It holds sockets only. A run inside
the run finds it set and uses it.

A `TMUX_TMPDIR` you set yourself, naming a folder that exists, is kept. A
relative one is kept as that folder's full path: tmux reads the variable in
every call, and a call made after a `cd` would not find a relative one again.

The run removes its folder with `rmdir` when it ends, also on a FATAL and on
HUP, INT or TERM. A socket in it that is neither the suite's own server's nor
a name a block put in `SOCKS` fails the run, with the row "every socket in the
suite's own folder is one cleanup knows". So does anything in the folder beside
tmux's own `tmux-<uid>`. The folder then stays, and the run
names what is in it on stderr: `tests/run.sh: left in its socket folder …`. A
run ended by KILL leaves it too. Nothing sweeps `/tmp/cs.*`. One is safe to
remove once no server answers on a socket inside it.

Those rows hold only where the suite made the folder. In a folder you named,
other runs' sockets may sit beside this run's, so nothing is asked of it. On
CI one leg, the one marked `bare` in `shell-suite.yml`'s matrix (the
ubuntu-26.04 one today), starts the suite with no `TMUX_TMPDIR` and so runs
the suite's own folder and the rows. The other legs
set a private folder themselves and do not.

### The tui tests' claude guard

`cargo test` in `tui/` starts every test binary behind a guard against your own
`claude` (#973). `docs/tui.md`, under Install, says what the guard is and what a
failure prints. This section has its switch and its limits.

The repo's `.cargo/config.toml` sets `COCKPIT_TEST_CLAUDE_GUARD=1` for every
cargo started inside the checkout. `COCKPIT_TEST_CLAUDE_GUARD=0 cargo test`
lifts the guard by hand. The runner then starts each test binary as cargo alone
would, and a test with no stub of its own may start your real `claude`.
`make tui-test` sets the variable to 1 itself and CI runs that target, so an
exported 0 never lifts the gate.

cargo takes one runner, and the guard is the repo's. The config names it under
six targets by their exact names: `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`, `x86_64-apple-darwin` and `aarch64-apple-darwin`.
On any other target no runner applies and the tests run with no guard. The row
`this_run_is_behind_the_guard_where_one_was_asked_for` in
`tui/tests/claude_guard.rs` fails there, and its message names the target and
the two lines to add to the config. Only a run that includes that binary is
loud: a filtered run on such a target, `cargo test --test agents`, has no guard
and says nothing.

A runner of your own meets the repo's in one of two ways. The rule behind
both is cargo's: a key that names the target wins over every `cfg(..)` key,
from whatever file.

- Inside this checkout cargo drops yours without a word, and the guard runs,
  when yours is set in any of these: a `[target.'cfg(..)']` key in your own
  cargo config, a `[target.<triple>]` key in your own cargo config,
  `--config "target.'cfg(..)'.runner=..."` on the command line, or a
  `[target.'cfg(..)']` key in a `.cargo/config.toml` below the repo's. Every
  cargo command still works.
- Yours replaces the repo's, and cargo says nothing, when it is set by
  `CARGO_TARGET_<TRIPLE>_RUNNER`, by `--config target.<triple>.runner=...`, or
  by a `[target.<triple>]` key in a `.cargo/config.toml` below the repo's. The
  row named above then fails and says so, in a run that includes that binary.
  `cargo test --test agents` alone runs no such row and says nothing. To keep
  both, end your runner with
  `exec <checkout>/tui/tests/claude-guard/runner "$@"`. To run without the
  guard, set the switch to 0.

cargo still names the signal a test binary died by (`signal: 6, SIGABRT`),
because the runner then dies by the same signal. That holds for every signal
that ends a process, with one exception: where `/bin/sh` is bash, a death by
QUIT is reported as exit status 131, since bash ignores QUIT. A binary that
exits by its own choice with 128 plus the number of such a signal, 134 for
one, is reported as that signal. A shell cannot tell the two apart.

That has one cost. On a box that pipes core dumps to a program (apport,
systemd-coredump), a test binary that aborts leaves a second report there, of
the runner's shell: the kernel ignores a core limit under a pipe (core(5)).
Where a core is a file the runner sets its limit to 0 and leaves none, so the
binary's own core is not written over.

Everything cargo starts in the checkout goes through that script, `cargo run`
and `cargo run --example` too. They get no guard and keep their arguments,
their `PATH` and their pid. The rest of the environment is the shell's to
hand on. Where `/bin/sh` is dash, a variable whose name is no shell name, one
with a dot or a dash in it, does not reach the binary: dash drops it, and bash
does not. Either shell sets `PWD` when it is missing or stale, and bash also
sets `SHLVL` and drops `_` and a stale `OLDPWD`.

`cargo bench` gets no guard either. Its binaries are the test binaries, run
with `--bench`, and the harness runs no test then. With `--bench` and `--test`
together it does, and the guard is on.

A signal sent to the runner alone (HUP, INT, QUIT, TERM) waits for the test
binary. Any other signal that ends a process, KILL among them, ends the runner
at once, and the binary runs on with nobody left to read the log. The guard's
folder, `cockpit-claude-guard.<6>`, then stays in `TMPDIR`, under dash and
under bash alike: a later start is still refused, and not reported. That folder
is the cost. Nothing sweeps it, and `make tui-test-clean` names it.

A shell cannot trap a signal that was ignored when it started. A suite started
with `&` from a script ignores INT and QUIT, one under nohup ignores HUP, and
the runner then ignores that signal as its binary does: the log is still read,
the folder removed, and the runner ends with the binary's status, and one row
of `claude_guard.rs` starts the runner that way to hold it. The other rows
start the runner with every signal they send at its default and unblocked, so
they hold however the suite was started. So do the rows of `agents.rs` and
`push.rs` that signal a process they started (#1061). Each of the three files
has a row that starts its own binary again with those signals ignored and
blocked, and fails without that reset (#1044, #1061). Those rows, and a test
binary started directly, are what hold the blocked cases: through
`make tui-test` a blocked signal never reaches a test binary, since GNU make
starts its children with an empty mask and dash clears one, and an ignored
USR1 never does either, since make and cargo catch USR1.

Doctests are guarded under cargo 1.92 and not under 1.88, the two measured:
1.88, the toolchain `make tui-test` uses, does not hand a doctest to a runner.
The crate has no doctest today.

## CI

Two workflows run the tests on every pull request and on master and `scratch/**`
pushes, each on both ubuntu-24.04 and ubuntu-26.04, and both legs are required.
`shell-suite` runs `make tui-bin` and then `J=4 make test-par`, on a full clone
so the #290 key-notes block runs. `tui` runs `make tui-test-clean`: tui-test
in a private TMPDIR and TMUX_TMPDIR, failing on a leftover (#400). Both get tmux
3.8, the README's floor, from `.github/actions/tmux`: it builds the official
release
tarball after checking its SHA-256, and caches the install on the version, the
runner image, the checksum and the action's own file, so only the first run on a
new image or after a recipe change compiles it. Both set `COCKPIT_REQUIRE_TMUX=1`,
which says a missing tmux is a failure and not a skip (the tests read it once
#471's test-side change lands). The hosted image has no `xterm-ghostty` terminfo,
so the two phone-pty rows skip there; the suite's final line counts them.

A third workflow, `shell-suite-tmux-master`, runs the same `J=4 make test-par`
on tmux built from tmux's master branch (next-3.9), on ubuntu-24.04 only. On
that tmux a popup is a pane and the session's active one (#494), which 3.8
cannot show. It builds the commit named by `TMUX_MASTER_SHA` at the top of the
workflow and caches the install on that commit, the runner image and the
workflow file's hash. To bump it, paste a newer full sha from tmux's master
there and push: the first run builds it (about 35 s), later runs reuse it.

That job is not required. It carries `continue-on-error`, so a red run leaves
the workflow green, but its check still reads `fail` in `gh pr checks`. The
agents' merge script (`.omc/safe-merge.sh`, outside the repo) lists the check
as optional, so a red or still-running tmux master job never holds a merge. A
red run is still worth reading: it is tmux master breaking the scripts, or
tmux breaking itself, before a release ships it.
