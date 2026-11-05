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

## CI

Two workflows run the tests on every pull request and on master and `scratch/**`
pushes, each on both ubuntu-24.04 and ubuntu-26.04, and both legs are required.
`shell-suite` runs `make tui-bin` and then `J=4 make test-par`, on a full clone
so the #290 key-notes block runs. `tui` runs `make tui-test`. Both get tmux 3.8,
the README's floor, from `.github/actions/tmux`: it builds the official release
tarball after checking its SHA-256, and caches the install on the version, the
runner image, the checksum and the action's own file, so only the first run on a
new image or after a recipe change compiles it. Both set `COCKPIT_REQUIRE_TMUX=1`,
which says a missing tmux is a failure and not a skip (the tests read it once
#471's test-side change lands). The hosted image has no `xterm-ghostty` terminfo,
so the two phone-pty rows skip there; the suite's final line counts them.
