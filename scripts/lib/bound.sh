#!/usr/bin/env bash
# bound <secs> <cmd> [args...]: cmd under GNU timeout, else Homebrew coreutils' gtimeout
# (#746). With neither, as on stock macOS without coreutils, it runs unbounded, as these
# reads always have there (docs/install.md). Sourced, never run.
if command -v timeout >/dev/null 2>&1; then BOUND_TO=timeout
elif command -v gtimeout >/dev/null 2>&1; then BOUND_TO=gtimeout
else BOUND_TO=""
fi
bound() {                     # <secs> <cmd> [args...]
    local secs=$1; shift
    if [ -n "$BOUND_TO" ]; then "$BOUND_TO" "$secs" "$@"; else "$@"; fi
}
