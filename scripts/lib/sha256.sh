#!/usr/bin/env bash
# Sourced, never run. The sha256 tool bootstrap.sh and tui-fetch check downloads with: sha256sum
# where it is installed, else shasum -a 256 (#1464). Bash 3.2 safe, as bootstrap.sh is.

# 0 when either tool is installed.
sha256_have() { command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1; }

# Checks the "<sha256>  <file>" lines on stdin, as `sha256sum -c -` does.
sha256_c() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum -c -; else shasum -a 256 -c -; fi
}
