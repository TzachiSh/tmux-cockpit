# tmux-cockpit
#
#   make test         run the suite
#   make test-par     the suite sharded (J=nproc; J=4 make test-par)
#   make check-par    lint beside a sharded suite: make -j2 check-par
#   make gate         the pre-commit gate; a stamp skips it when nothing changed
#   make test-changed only the blocks a change could have broken
#   make lint         shellcheck every shell file (skipped if not installed)
#   make check        lint + test, the merge gate
#   make install      symlink the commands into ~/.local/bin
#   make uninstall    remove only the symlinks pointing back here
#   make bootstrap    full setup from nothing: packages, plugins, config, hooks
#
# Everything here is shell; there is nothing to build.

SHELL := /bin/bash

# Every executable shell file, whatever its extension. Listed once so a new
# script cannot be added to the repo and quietly skipped by the linter.
SOURCES := cockpit.tmux install.sh bootstrap.sh tests/run.sh tests/only.sh tests/par.sh tests/green.sh tests/changed.sh tests/fixtures/review/mkfix.sh $(filter-out scripts/lib,$(wildcard scripts/*)) $(wildcard scripts/lib/*)

.PHONY: all check test test-par check-par gate test-changed lint fmt install uninstall bootstrap help

all: help

help:
	@sed -n '2,14p' $(MAKEFILE_LIST) | sed 's/^# \{0,1\}//'

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
