# tmux-cockpit
#
#   make test        run the suite
#   make lint        shellcheck every shell file (skipped if not installed)
#   make check       lint + test, the pre-commit gate
#   make install     symlink the commands into ~/.local/bin
#   make uninstall   remove only the symlinks pointing back here
#   make bootstrap   full setup from nothing: packages, plugins, config, hooks
#
# Everything here is shell; there is nothing to build.

SHELL := /bin/bash

# Every executable shell file, whatever its extension. Listed once so a new
# script cannot be added to the repo and quietly skipped by the linter.
SOURCES := cockpit.tmux install.sh bootstrap.sh tests/run.sh tests/fixtures/review/mkfix.sh $(wildcard scripts/*)

.PHONY: all check test lint fmt install uninstall bootstrap help

all: help

help:
	@sed -n '2,10p' $(MAKEFILE_LIST) | sed 's/^# \{0,1\}//'

test:
	@tests/run.sh

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

install:
	@./install.sh

uninstall:
	@./install.sh --uninstall

bootstrap:
	@./bootstrap.sh
