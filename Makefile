# tmux-cockpit
#
#   make test         run the suite
#   make test-par     the suite sharded (J=nproc; J=4 make test-par)
#   make check-par    lint beside a sharded suite: make -j2 check-par
#   make gate         the pre-commit gate; a stamp skips it when nothing changed
#   make test-changed only the blocks a change could have broken
#   make lint         shellcheck every shell file (skipped if not installed)
#   make check        lint + test, the merge gate
#   make tui-bin      build the required cockpit-tui into bin/ (the tests run it first)
#   make tui          the same, then print its version
#   make tui-test     its tests, clippy -D warnings, fmt and doc-link checks, on CI's rust-version
#   make tui-test-clean  tui-test in a private TMPDIR and TMUX_TMPDIR; fails on a leftover
#   make install      symlink the commands into ~/.local/bin
#   make uninstall    remove only the symlinks pointing back here
#   make bootstrap    full setup from nothing: packages, plugins, config, hooks
#
#   tests/par.sh --each   every block group alone: the audit for a block that
#                         only passes beside a neighbour whose variable it reads

SHELL := /bin/bash

# Every executable shell file, whatever its extension. Listed once so a new
# script cannot be added to the repo and quietly skipped by the linter.
SOURCES := cockpit.tmux install.sh bootstrap.sh tests/run.sh tests/only.sh tests/par.sh tests/green.sh tests/changed.sh tests/clean-run.sh tests/fixtures/review/mkfix.sh $(filter-out scripts/lib,$(wildcard scripts/*)) $(wildcard scripts/lib/*)

.PHONY: all check test test-par check-par gate test-changed lint fmt tui tui-bin tui-test tui-test-clean install uninstall bootstrap help

all: help

help:
	@sed -n '2,19p' $(MAKEFILE_LIST) | sed 's/^# \{0,1\}//'

test: tui-bin
	@tests/run.sh

# One block group in seconds: make test-only PAT='tmux-review --attach'
test-only:
	@tests/only.sh '$(PAT)'

# The suite sharded over J private tmux servers (J=nproc; J=4 make test-par).
# make test above is untouched: one process, source order.
test-par: tui-bin
	@tests/par.sh

# Advisory: a missing linter must not stop the tests. Extended analysis takes
# 15+ GB on tests/run.sh and OOMs a CI runner, so it is off where supported.
lint:
	@if command -v shellcheck >/dev/null 2>&1; then \
	    xa=; shellcheck --help 2>&1 | grep -q extended-analysis && xa=--extended-analysis=false; \
	    shellcheck $$xa $(SOURCES) && echo "shellcheck: clean"; \
	else \
	    echo "shellcheck not installed - skipping (apt install shellcheck)"; \
	fi

check: lint test

# The required binary (#199): every suite target builds it first.
# --locked everywhere: a build that silently updates Cargo.lock is not the
# build the crossterm "=0.29.0" pin was measured against. install, not cp:
# it replaces the file, where cp's write over a running board is ETXTBSY.
# The binary's dir comes from cargo, not tui/target (#546): scripts/lib/cargo-target.sh.
tui-bin:
	@cargo build --release --locked --manifest-path tui/Cargo.toml
	@td=$$(. scripts/lib/cargo-target.sh && cargo_target_dir tui/Cargo.toml) && \
	mkdir -p bin && install -m 755 "$$td/release/cockpit-tui" bin/cockpit-tui

tui: tui-bin
	@bin/cockpit-tui --version

# The checks run on the toolchain CI installs (tui/Cargo.toml's rust-version,
# #394): a newer local clippy lints less and lets code through that CI then
# rejects. Only through rustup, and only here: tui-bin and the installers keep
# the default toolchain, so offline and cargo-dist builds are untouched.
# `make tui-test TUI_TOOLCHAIN=` opts out; TUI_TOOLCHAIN=1.92 picks another.
TUI_TOOLCHAIN ?= $(if $(shell command -v rustup 2>/dev/null),$(shell sed -n 's/^rust-version *= *"\(.*\)".*/\1/p' tui/Cargo.toml))
TUI_ENV = $(if $(TUI_TOOLCHAIN),RUSTUP_TOOLCHAIN=$(TUI_TOOLCHAIN))

tui-test:
	@$(TUI_ENV) cargo test --locked --manifest-path tui/Cargo.toml
	@$(TUI_ENV) cargo clippy --locked --manifest-path tui/Cargo.toml --all-targets -- -D warnings
	@$(TUI_ENV) cargo fmt --manifest-path tui/Cargo.toml --check
	@# A doc link to a name that does not resolve fails the target (#726).
	@$(TUI_ENV) RUSTDOCFLAGS='-D rustdoc::broken_intra_doc_links' cargo doc -q --locked \
		--manifest-path tui/Cargo.toml --no-deps --document-private-items

# tui-test with TMPDIR and TMUX_TMPDIR on private empty dirs, failing on any
# file, dir or socket a test left behind there (#400).
tui-test-clean:
	@tests/clean-run.sh $(MAKE) -s tui-test

# lint and test-par are independent, so `make -j2 check-par` runs them
# concurrently and the 17s of lint disappears into the tests' wall clock.
check-par: lint test-par

# Skip the gate entirely when nothing the tests read has changed since it
# last passed (tests/green.sh, a content-keyed manifest); otherwise run the
# full parallel gate and stamp on success.
gate: tui-bin
	@tests/green.sh check && exit 0; \
	$(MAKE) check-par && tests/green.sh stamp

# Only the blocks a changed file could have broken -- a developer loop, not a
# gate. make gate / make check still run the full suite at merge.
test-changed: tui-bin
	@PAR_BLOCKS="$$(tests/changed.sh)" tests/par.sh

install:
	@./install.sh

uninstall:
	@./install.sh --uninstall

bootstrap:
	@./bootstrap.sh
