# tmux-cockpit
#
#   make test         run the suite
#   make test-par     the suite sharded (J=nproc; J=4 make test-par)
#   make check-par    lint beside a sharded suite: make -j2 check-par
#   make gate         the pre-commit gate; a stamp skips it when nothing changed
#   make test-changed only the blocks a change could have broken
#   make lint         shellcheck every shell file (skipped if not installed)
#   make check        lint + test, the merge gate
#   make tui          build the optional Rust view into bin/cockpit-tui
#   make tui-test     its tests, clippy -D warnings and fmt --check
#   make install      symlink the commands into ~/.local/bin
#   make uninstall    remove only the symlinks pointing back here
#   make bootstrap    full setup from nothing: packages, plugins, config, hooks
#
#   tests/par.sh --each   every block group alone: the audit for a block that
#                         only passes beside a neighbour whose variable it reads
#
# Everything here is shell except tui/, and `check` never touches that: a box
# with no cargo must still be able to run the suite.

SHELL := /bin/bash

# Every executable shell file, whatever its extension. Listed once so a new
# script cannot be added to the repo and quietly skipped by the linter.
SOURCES := cockpit.tmux install.sh bootstrap.sh tests/run.sh tests/only.sh tests/par.sh tests/green.sh tests/changed.sh tests/fixtures/review/mkfix.sh $(filter-out scripts/lib,$(wildcard scripts/*)) $(wildcard scripts/lib/*)

.PHONY: all check test test-par check-par gate test-changed lint fmt tui tui-test install uninstall bootstrap help

all: help

help:
	@sed -n '2,17p' $(MAKEFILE_LIST) | sed 's/^# \{0,1\}//'

test:
	@tests/run.sh

# One block group in seconds: make test-only PAT='tmux-review --attach'
test-only:
	@tests/only.sh '$(PAT)'

# The suite sharded over J private tmux servers (J=nproc; J=4 make test-par).
# make test above is untouched: one process, source order.
test-par:
	@tests/par.sh

# shellcheck is advisory here rather than required: this repo is cloned onto
# whatever machine you happen to be on, and a missing linter must not be the
# thing that stops you running the tests.
lint:
	@if command -v shellcheck >/dev/null 2>&1; then \
	    shellcheck $(SOURCES) && echo "shellcheck: clean"; \
	else \
	    echo "shellcheck not installed - skipping (apt install shellcheck)"; \
	fi

check: lint test

# The optional Rust view (issue #61). Deliberately NOT a prerequisite of check:
# the binary is additive, and tests/run.sh skips its block when bin/ is empty.
# --locked everywhere: a build that silently updates Cargo.lock is not the
# build the crossterm "=0.29.0" pin was measured against.
tui:
	@cargo build --release --locked --manifest-path tui/Cargo.toml
	@mkdir -p bin && cp tui/target/release/cockpit-tui bin/cockpit-tui
	@bin/cockpit-tui --version

tui-test:
	@cargo test --locked --manifest-path tui/Cargo.toml
	@cargo clippy --locked --manifest-path tui/Cargo.toml --all-targets -- -D warnings
	@cargo fmt --manifest-path tui/Cargo.toml --check

# lint and test-par are independent, so `make -j2 check-par` runs them
# concurrently and the 17s of lint disappears into the tests' wall clock.
check-par: lint test-par

# Skip the gate entirely when nothing the tests read has changed since it
# last passed (tests/green.sh, a content-keyed manifest); otherwise run the
# full parallel gate and stamp on success.
gate:
	@tests/green.sh check && exit 0; \
	$(MAKE) check-par && tests/green.sh stamp

# Only the blocks a changed file could have broken -- a developer loop, not a
# gate. make gate / make check still run the full suite at merge.
test-changed:
	@PAR_BLOCKS="$$(tests/changed.sh)" tests/par.sh

install:
	@./install.sh

uninstall:
	@./install.sh --uninstall

bootstrap:
	@./bootstrap.sh
