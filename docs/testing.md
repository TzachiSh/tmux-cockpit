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

cargo takes one runner, and the guard is the repo's. A runner of your own meets
it in one of three ways:

- Set by `CARGO_TARGET_<TRIPLE>_RUNNER`, by a `[target.<triple>]` key, by
  `--config`, or in a `.cargo/config.toml` below the repo's, yours replaces the
  repo's and cargo says nothing. The row
  `this_run_is_behind_the_guard_where_one_was_asked_for` in
  `tui/tests/claude_guard.rs` then fails and says so, in a run that includes
  that binary. `cargo test --test agents` alone runs no such row and says
  nothing. To keep both, end your runner with
  `exec <checkout>/tui/tests/claude-guard/runner "$@"`. To run without the
  guard, set the switch to 0.
- Set under the repo's own key, `[target.'cfg(unix)']`, in your
  `~/.cargo/config.toml`, yours is dropped by cargo inside this checkout
  without a word, and the guard runs. Use one of the forms above.
- Set under another `cfg(...)` key that also matches, cargo refuses to build
  or run anything in the checkout: `cargo build`, `check`, `test`, `run`,
  `clippy` and `doc` all end with ``several matching instances of
  `target.'cfg(..)'.runner` in configurations``, exit 101. So do `make tui-bin`
  and an install from source started there. Move your runner to one of the
  forms above.

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
binary. Another one ends the runner at once and leaves its folder,
`cockpit-claude-guard.<6>`, in `TMPDIR`.

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
