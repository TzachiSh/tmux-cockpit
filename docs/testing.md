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

### What a shard may print

A block prints its `== title ==` and its rows, nothing else. `tests/par.sh`
reads every shard's log once the shards are done, a dead shard's too. A line
that is not a title, a `PASS`, `SKIP` or `FAIL` row, a red row's `want` and
`got`, or a `FATAL` line fails the run (#1153). Such a line is a command's
noise, or an error no row read, and in a red run's log it hides the real one.

A `want` or a `got` can hold a newline. `bad` starts each further line of one
with `     | `, so a red row stays readable and the check can tell its lines
from a stray one. Each counts only in its place: a `want` right after a `FAIL`
row, a `got` right after its `want`, a `     | ` line inside a `want` or a
`got`. The same line anywhere else is named.

Each stray line is one record on par.sh's stderr, its fields separated by
tabs:

```
par.sh: stray line	shard=0002	line=3	block=<title>	text=<the line>
```

`line` counts from the top of that shard's log, `block` is the last title above
the line, and `text` has its colours and control bytes dropped and is cut at
200 bytes. A count line follows the records. A shard's stdout and stderr go to
one log, so a record does not say which stream the line came from.

Only `tests/par.sh` checks, and so `make test-par`, `make check-par`,
`make gate`, `make test-changed` and CI.
`tests/only.sh` and a plain `tests/run.sh` print as they run and check nothing.

### A TMPDIR with a blank

Under a `TMPDIR` whose path holds a space or a tab, a run of `tests/run.sh`
(`tests/only.sh` and each `tests/par.sh` shard too) keeps its wrapper around
tmux outside it, in a folder it makes in the caller's runtime dir,
`$XDG_RUNTIME_DIR/cnb.<6>`, mode 700 (#992). Pane commands in the suite paste
that wrapper's path unquoted, so it must hold no blank. The wrapper's small
marker files sit beside it, `flop/k.*` and `flop/r.*`, and a run started inside
the run makes its own `cnb.<6>` in there. Four blocks each add a stub tmux,
`ct.<6>/tmux`: three board blocks' stay until the run ends, and the #1078
block's goes when that block ends. The run removes the folder
when it ends, also on a FATAL and on HUP, INT or TERM. A run ended by KILL leaves it,
with `flop/tmux` inside. Nothing sweeps it, and it is safe to remove once no
run is going.

With no runtime dir to use, such a run ends at its start and says what it needs:
set `XDG_RUNTIME_DIR` to a folder that exists, can be written to and has no
blank in its path.

### What the suite drops from its caller's environment

The suite drops what its caller's environment could hand to a script under test
(#1078): the folders `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `XDG_BIN_HOME`,
`XDG_CACHE_HOME` and `XDG_DATA_HOME`; the file `COCKPIT_LOG`; the editors
`VISUAL` and `EDITOR`; the switches `COCKPIT_READONLY`, `COCKPIT_PANEL_NOTE`,
`COCKPIT_REVIEW_BASE`, `COCKPIT_PROC_DIR`, `COCKPIT_POLL_KEY`,
`REVDIFF_CONFIG`, `REVDIFF_EXCLUDE`, `CLAUDE_CODE_SESSION_KIND` and
`COCKPIT_ACTION`; and the programs `COCKPIT_TMUX` and `COCKPIT_CLAUDE`. A block
that needs one sets it itself.

It also drops every exported `GIT_` name (#1099), by prefix: `GIT_DIR`,
`GIT_WORK_TREE`, `GIT_INDEX_FILE` and their kin point git at another
repository, `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM` and the numbered
`GIT_CONFIG_KEY_<n>` hand it other settings, `GIT_TEMPLATE_DIR` other hooks,
`GIT_TRACE` a file to append to, `GIT_EDITOR` a program to run. Before that,
a run from a shell that exported `GIT_DIR`, as a git hook's shell does, made a
repository in the folder it named, committed into it twice and failed 383
rows. If you ran the suite that way, look there for a `cockpit-green` file,
two commits titled `init` and worktree entries that are not yours.

`BASH_ENV` and `ENV` go too: every bash the suite starts would source
`BASH_ENV` first, and an interactive sh `ENV`. The shell that runs the blocks
is one of those bashes, so `tests/run.sh` starts itself again without the two
names when either is set, before anything else (#1115). That covers `make
test`, `tests/only.sh` and every `tests/par.sh` shard. What cannot be undone:
each of those first shells, and `only.sh` and `par.sh` themselves, still source
the file once before the run drops it, so a file that writes or starts
something does that once per shell.

And it drops the other names the scripts and the board read, none of which
names a place to write (#1100): `NO_COLOR`, `COCKPIT_GLYPHS`,
`COCKPIT_PANEL_MODEL`, `COCKPIT_PANEL_EFFORT`, `COCKPIT_PANEL_TIMEOUT`,
`COCKPIT_PANEL_MODE`, `COCKPIT_PANEL_DROPPED`, `COCKPIT_PANEL_HIDE_TAKEN`,
`COCKPIT_PANEL_KEY`, `COCKPIT_VIEW_ASK`, `COCKPIT_VIEW_BACK`,
`COCKPIT_BASKET_PANE`, `COCKPIT_FILES_STATE`, `COCKPIT_GITHUB`,
`COCKPIT_TUI_RELEASES`, `COCKPIT_TUI_REPO`, `TM_SID`, `TM_CLIENT`, `TM_PANE`,
`TM_POPUP`, `TM_NEEDS`, `MD_EDIT`, `REVIEW_PANE_ID`, and
`COCKPIT_TEST_TMUX_GUARD`, the tui tests' own guard, which `.cargo/config.toml`
sets for cargo and the suite clears for itself alone. With them go
`TERMUX_VERSION` and `PREFIX` (#1115): the board reads either one, the second
when it names Termux's own tree, as a sign that it runs under Termux, and then
opens the panel as a tablet's. `COCKPIT_TUI_BIN` is still taken from the
caller, on purpose.

Before #1080 a run from a shell that exported one of those nineteen used it. If
you ran the suite that way, you may find three symlinks in your `XDG_BIN_HOME`,
`tm`, `tmux-branch` and `tmux-open-path`, pointing into the checkout you
tested, a file under `$XDG_CACHE_HOME/tmux-cockpit/panel/prompts/`, and lines
from test boards in the file `COCKPIT_LOG` names. The file and the lines are
safe to remove. So are the links, unless the checkout you tested is your plugin
folder: there they are your install's own links, with the same names and the
same targets. With `COCKPIT_TMUX` exported, the suite's test boards sent their
tmux commands to the server that value names. Measured with a stand-in that
reached no server, they asked for its panes and added `wait-for` entries to its
global `@cockpit_board_wake` option; a real server would have answered, so more
may have followed. A board takes its own entry out when it exits; one that was
killed leaves an entry that signals a channel nobody waits on. With
`COCKPIT_CLAUDE` exported, one panel block ran the program it names as
`claude -p`, with the test pane's text: a model call, if it was the real one. A
run with none of the nineteen exported left nothing: everything went under the
suite's own `HOME`, which it deletes.

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

Once the run's exit trap has started, it ignores HUP, INT and TERM, and so does
every command it starts (#1107). No step of the cleanup has a time limit. A
Ctrl-C then does nothing: with the suite's server stopped, so that the
cleanup's `tmux kill-server` waited on it, an INT to the run's process group
left the run running for over a minute. A HUP or TERM to the group, from a
second terminal `kill -TERM -- -<pgid>`, ended that cleanup in about 2 s: the
tmux client handles both itself, and the cleanup went on and removed
everything. Anything else that hangs there, an `rm` or a `find`, needs
`kill -9`, which ends the run where it is. It leaves the run's socket
`cockpit-suite-<pid>` under `$TMUX_TMPDIR/tmux-<uid>`, its state and shim
folders (`tmp.<10>` in `TMPDIR`) and, for a run started with no
`TMUX_TMPDIR`, its `/tmp/cs.<6>` folder, to remove by hand. `kill -9 <pid>`
alone also leaves the hung command running.

`tests/par.sh` does the same (#1127): it traps HUP, INT and TERM itself, and its
exit trap ignores them first, so a second signal no longer leaves its work
folder in `TMPDIR` or its `tests/.par.<pid>.*` shard files behind. The cost:
bash runs a trap only once the command in the foreground is done, and par.sh's
is the xargs that runs every shard. A signal sent to par.sh's pid alone, as
`kill <pid>` or `timeout --foreground` sends it, now waits for that xargs to
finish, every shard still queued included, and par.sh then exits 128 plus the
signal's number with no totals. A signal sent to its process group, a Ctrl-C
at the terminal or a plain `timeout`, reaches xargs and the shards too, and the
run ends promptly.

`tests/only.sh` and `tests/changed.sh` do the same (#1171), so a second signal
no longer leaves only.sh's `tests/.only.*` file in the checkout or changed.sh's
table in `TMPDIR`. The same cost applies to only.sh, whose foreground command is
the run of the blocks it picked. A signal sent to its pid alone waits for that
run to finish. A signal sent to its group ends that run too, and only.sh exits
once the run's own cleanup is done.

`tests/clean-run.sh` (`make tui-test-clean`) and `tests/tui-refusers.sh` do the
same in sh (#1172). They had no trap at all, and dash, the `/bin/sh` of Debian,
Ubuntu and the CI images, runs no EXIT trap when a signal it does not trap ends a
script, so any HUP, INT or TERM left clean-run.sh's two folders and its list, or
tui-refusers.sh's list of binaries, in `TMPDIR`. dash also runs a trap only once
the command in the foreground is done. A signal sent to clean-run.sh's pid alone
now waits for the whole `make tui-test` it runs, and one to tui-refusers.sh's
waits for its `cargo test --no-run` build or its namespace run. Before, the
script ended at once and that command ran on, writing into a folder nobody
removed. A signal sent to the group, a Ctrl-C or a plain `timeout`, reaches the
command too, and the script removes its paths once it ends. A cancelled CI job
is neither: GitHub's runner sends INT and then TERM to the step's shell alone,
then kills the process tree, so neither script cleans up there, before this
change or after it.

Those rows hold only where the suite made the folder. In a folder you named,
other runs' sockets may sit beside this run's, so nothing is asked of it. On
CI one leg, the one marked `bare` in `shell-suite.yml`'s matrix (the
ubuntu-26.04 one today), starts the suite with no `TMUX_TMPDIR` and so runs
the suite's own folder and the rows. The other legs
set a private folder themselves and do not.

### Waiting for a pane's start

A row that starts a pane and then needs its process waits with `pane_runs`
(#1079), not on `#{pane_current_command}`. While tmux cannot read the pane's
foreground process, it names the pane by the first word of its start command,
so a pane started `sleep 30` reads `sleep` before sleep runs. A pane started
`exec bash` reads `bash` already in the `<default-shell> -c` shell that starts
it. `pane_runs <socket> <pane> <comm> <argv...>` reads `#{pane_pid}` and waits
until that process's comm and whole argv are the ones given. On a start with no
`exec`, such as `sleep 30`, it relies on the default shell exec'ing a lone `-c`
command, so that the command becomes the pane's process; bash, dash and zsh do.

A pane started by `new-window -e PATH=…` does not get that `PATH`: tmux puts the
PATH of the client that ran `new-window` over it (tmux's `spawn.c`). A row that
needs a pane to find its own `sleep` or `claude` first sets `PATH` on the tmux
client command itself, `PATH="$dir:$PATH" "$TMUX_REAL" -L … new-window …`, or
puts it in the start command.

### Running the tui tests

Run them with `make tui-test` from the checkout: on the six targets
`.cargo/config.toml` names, cargo's runner gives each test binary a tmux socket
folder of its own and clears your session's variables. With
`COCKPIT_TEST_CLAUDE_GUARD=0`, on another target (#1090), or with a test binary
started directly, the binaries that run tmux refuse until `TMUX_TMPDIR` names a
folder, since a rig's socket would otherwise land beside your own tmux server's.
That was 26 of the 106 when #1066 merged. `tests/tui-refusers.sh` counts them
again, on Linux, without reaching your tmux server: it runs them in a user and
mount namespace, and where it gets none it says so and starts no binary.
In `tui/` the refusal's own command runs them:
`d=$(mktemp -d) && env -u TMUX -u TMUX_PANE -u TMUX_COCKPIT_DIR -u COCKPIT_LOG -u XDG_CACHE_HOME -u XDG_STATE_HOME TMUX_TMPDIR="$d" cargo test`.

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

### Your tmux session, log and cache

A `cargo test` started from a shell inside your tmux session used to hand that
session to its tests (#1066). The rigs put their servers' sockets beside your
server's, since `-L` picks a socket's name and not its folder, and handed your
`TMUX`, `TMUX_PANE` and `TMUX_COCKPIT_DIR`, your live plugin's folder, to those
servers and their panes. Ten binaries read your server's `@cockpit_agents_glob`
and theme through a plain `tmux`, and with `TMUX` unset that read goes to the
default socket, your server's too. Rigs that set `HOME` wrote to an exported
`COCKPIT_LOG`, used the agents cache under an exported `XDG_CACHE_HOME`, and
read the hooks' subagent log and review folder under an exported
`XDG_STATE_HOME`.

The same runner now unsets `TMUX`, `TMUX_PANE`, `TMUX_COCKPIT_DIR`,
`COCKPIT_LOG`, `XDG_CACHE_HOME` and `XDG_STATE_HOME` for each test binary, and
sets `TMUX_TMPDIR` to a folder of its own whatever yours says, which it never
uses or writes to. Every rig starts its server with TERM and HUP at their
default and unblocked, since kill-server is a TERM the server sends itself and a
HUP to each pane: a suite started with them blocked left servers and panes
running (#1061).

That folder is `/tmp/ct.<6>`, made by `mktemp -d`, so mode 700 and a name
nobody can guess, before the binary starts: tmux puts its socket in `/tmp` when
`TMUX_TMPDIR` names a folder that is not there. It is in `/tmp` and nowhere
else, since a socket's path fails past 107 bytes (103 on macOS) and a TMPDIR
runs long. It holds tmux's own `tmux-<uid>` folder and sockets, nothing else.
Where `/tmp` takes no folder the binary is not started, and two lines say
so: mktemp's own, then the runner's. After
the binary it goes by `rmdir`, only if empty, also after a HUP, INT, QUIT or
TERM to the runner, which waits for the binary first. A socket file left in
it is one no rig removed, from a binary that died before its rigs were dropped
or a server started past `fake::Server`: the runner names the folder and fails
the binary with status 1, or with the binary's own status where that failed,
after any report of the claude guard. A rig removes its server's socket file
when it is dropped, even where kill-server failed, so a server that outlives its
rig leaves no socket here for the runner to see. The rows that ask `/proc`
catch it (`fake::assert_no_server_left`), and the runner ends no process. Killed by a signal it does not trap, KILL among
them, the runner leaves its `/tmp/ct.<6>` folder behind, empty. Under a plain
`cargo test` nothing names it. That folder is the cost. Nothing sweeps it, and
it is safe to remove once no test is running. `make tui-test-clean` hands the
runner a list in `COCKPIT_TEST_CLEAN_RUN_CT_LIST`, the gate's own bookkeeping,
and each runner adds its folder's path to it in one write. After the run the
gate names every listed folder still there and fails, and only those: another
gate's folders are on another list. A list that takes no line costs a warning,
never the binary. Exported by hand, the variable gets one path a binary added
to the file it names.

So every test binary runs as on CI. A plain `tmux` in it finds no server, and
the cache and state folders resolve under whatever `HOME` the test has: for a
test that sets no `HOME`, under yours, as on a box that never exported them. Three binaries
write some of them in their own process: panel_env (`TMUX`, `TMUX_PANE`,
`TMUX_TMPDIR`, `XDG_CACHE_HOME`), agents (`XDG_CACHE_HOME`) and data
(`COCKPIT_LOG`). A test binary has one environment lock, `fake::env_lock()`
(#1110), and every write and every check of these runs under it. A check made
under the lock takes its token, `fake::checked_state_held` and
`fake::checked_tmux_held`. The guard-less `fake::checked_state` and
`fake::checked_tmux` take the lock for their read, and on a thread that already
holds it they panic and name the `_held` form, where they would otherwise wait
for themselves. `fake::env_lock()` itself panics on a thread that already holds
it, for the same reason. push keeps an `RwLock` of its own: its readers, the
rigs, run in parallel, and its one writer takes `fake::env_lock()` after its rig
is built, so the order is push's lock, then fake's, as in every rig's server
start. The helpers only check: `fake::checked_tmux`
before any tmux a helper runs, and `fake::checked_state`, or
`fake::checked_state_held` where the rig holds the lock (data, agents and
info_dir), in each rig that sets `HOME`. The product's own tmux calls refuse
too (#1089): under the test guard
(`COCKPIT_TEST_TMUX_GUARD`, docs/tui.md) `data::refuses` lets `data::tmux`,
push's start and the board's tile previews run a plain `tmux` only with `TMUX`
unset and `TMUX_TMPDIR` an existing folder, and each reads anything else
as no answer, so no test reads your server's agent list or theme, whether or not
it reached a check first. Started without the runner from a shell that still carries them, or
with no `TMUX_TMPDIR`, a test binary is refused in three lines: what is set or
unset and what that would do, `make tui-test` and a bare `cargo test` that
clear it, and the command for that binary alone. The cargo command and the
binary's each make a folder with `mktemp -d` for `TMUX_TMPDIR` and leave it
behind when the run ends. That
happens with `COCKPIT_TEST_CLAUDE_GUARD=0` too, which execs the binary as it is
and clears nothing, and on a target with no runner key (#1090).
One write from a test thread stays: `panel_batch.rs` takes `fake::fixed_env`
for `XDG_CONFIG_HOME`, as #948's binaries do.

A test writes the environment only through the test-env crate
(`tui/test-env`, #1132), which `fake` re-exports. The crate holds the lock and
the writers that take its token, `fake::set_var(&held, ..)` and
`fake::remove_var(&held, ..)`, and it sits outside the tui package.
`tui/clippy.toml` names `std::env::set_var`, `std::env::remove_var`,
`libc::setenv`, `libc::unsetenv`, `libc::putenv` and `libc::clearenv`, and
`tui/Cargo.toml` forbids clippy's `disallowed_methods`. The gate's clippy
therefore refuses a raw write anywhere in the package, whether through an alias
or a function pointer, and no `allow` of the lint or of its groups can lift
that.

`tests/tui-clippy-canary.sh` holds that rule, and `make tui-test` runs it after
its clippy line, or `make tui-clippy-canary` alone. It runs clippy the way the
gate does on two examples that only the `clippy-canary` feature builds.
`clippy_canary.rs` calls each of the six paths. `clippy_canary_allow.rs` holds
a write under an inner allow, which only a forbid refuses (E0453). The script
fails unless clippy refuses every call and that allow, each with its own
message on the canary's own line. So the gate goes red when the lint is off by
any route: a `.clippy.toml` beside `clippy.toml`, a renamed table,
`CLIPPY_CONF_DIR`, a level below forbid, or `--cap-lints` in rustflags.
`cargo clippy --all-features` run by hand builds the canaries and shows their
errors. That is the canary doing its job.

`tui/test-env` itself is the one reviewed root of trust. It is about 100 lines
on std alone, every change to it shows in a diff, and nothing lints it: it sits
outside the package so that its own writers can call the raw functions. The lint
keeps every other file of the package from writing the environment except
through the crate. Nothing keeps the crate itself from adding a writer that
takes no token, a function that wraps one of its writers, or a function that
hands out an `EnvHeld` without the lock. Each of those passes every check, so
any change to `tui/test-env` needs a load-bearing review. The rule for that
review: a diff to `tui/test-env` adds no `pub` item that writes the environment
or hands out an `EnvHeld`. Its two writers both take the token, so every write
in the package takes the lock: a binary whose one test writes takes it for the
writes alone, where it serves the type and closes no race (#1132).

`tmux_reach.rs` holds the one rule the lint cannot name: no `extern` block under
`tui/tests` or `tui/examples`. A `setenv` declared there would be a function no
path in `clippy.toml` names.

clippy 1.88 keeps a target's result when only `clippy.toml` changes, so touch
a source file before you check an edit to it. The canary script touches its
own examples for that reason.

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
