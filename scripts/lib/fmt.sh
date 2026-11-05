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

# A file name as a title: no control character and no bidi mark, which would
# reorder what the eye reads (U+200E-F, U+202A-E, U+2066-9, as UTF-8). tmux
# refuses a window name holding a control character (#366), and a popup border
# draws its title raw. For the title or the -n only: the editor keeps the real path.
plain_name() {
  local s=$1 b
  s=${s//[[:cntrl:]]/}
  for b in $'\xe2\x80\x8e' $'\xe2\x80\x8f' $'\xe2\x80\xaa' $'\xe2\x80\xab' $'\xe2\x80\xac' \
           $'\xe2\x80\xad' $'\xe2\x80\xae' $'\xe2\x81\xa6' $'\xe2\x81\xa7' $'\xe2\x81\xa8' $'\xe2\x81\xa9'; do
    s=${s//"$b"/}
  done
  printf '%s' "$s"
}
