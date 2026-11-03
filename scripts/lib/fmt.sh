#!/usr/bin/env bash
# Sourced, never run. Builtins only, bash 3.2 safe: hook.sh loads it too.

# tmux expands #{...} and #(...) in a session name, a start directory, a
# display-message text, a run-shell command and a -T title, and #(cmd) runs
# cmd. Every name, path, branch or error line that reaches one of those goes
# through here: ## is a literal #. A run of # before a [ goes as it is: tmux
# keeps #[ and ##[ verbatim, and doubling them would add a #.
fesc() {
  local s=$1 out='' run='' c
  while [ -n "$s" ]; do
    c=${s:0:1}; s=${s:1}
    if [ "$c" = '#' ]; then run+='#'; continue; fi
    if [ "$c" = '[' ]; then out+=$run; else out+=${run//#/##}; fi
    out+=$c; run=''
  done
  printf '%s' "$out${run//#/##}"
}
