#!/usr/bin/env bash
# numstat FIRST, cap check, only then capture the diff
set -uo pipefail
dir=$1
numstat=$(git -C "$dir" -c core.quotePath=false diff --no-color --no-ext-diff --numstat 2>/dev/null)
[ -n "$numstat" ] || numstat=$(git -C "$dir" -c core.quotePath=false diff --no-color --no-ext-diff --cached --numstat 2>/dev/null)
total=0
while IFS=$'\t' read -r a d f; do [ -n "$f" ] || continue
  case "$a" in *[!0-9]*) a=0;; esac; case "$d" in *[!0-9]*) d=0;; esac; total=$((total+a+d)); done <<<"$numstat"
if [ "$total" -gt 5000 ]; then printf '(diff too large)\t\tX\tdiff too large (%s changed lines)\n' "$total"; exit 0; fi
git -C "$dir" -c core.quotePath=false diff --no-color --no-ext-diff >/dev/null
