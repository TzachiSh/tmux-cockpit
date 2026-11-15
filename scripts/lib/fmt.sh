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

# source-file reads its path as a glob(7) pattern, where \ * ? [ are special: a
# folder named a\x was looked up as ax, and a* also loaded its siblings (#1050).
# Each of them goes in behind a \, which glob reads as "this character itself".
# Not stdin (source-file -): that leaves #{current_file} as -, so a file the
# sourced one loads by #{d:current_file} would be missed.
gesc() {
  local s=$1 out='' c
  while [ -n "$s" ]; do
    c=${s:0:1}; s=${s:1}
    case $c in \\ | \* | \? | \[) out+=\\ ;; esac
    out+=$c
  done
  printf '%s' "$out"
}

# Shows <text> on the status line as written: every display-message of a name, a path or
# an error line goes through here (#1096). tmux expands a message as a format, which runs a
# #( ), and then draws it, reading ## as # and #[ as a style, so fesc alone still lost a ##
# and a #[...]. -l (tmux 3.4) skips the expansion; every # doubled comes through the draw
# as one. The display-message flags (-c, -t, -d) go first, the text last. A text that
# starts with - gets a -- before it, or tmux reads it as a flag and shows nothing; any
# other text gets none, so the command stays as it was.
dmsg() {                      # [display-message flags...] <text>
  [ $# -ge 1 ] || { echo 'dmsg: no text to show' >&2; return 2; }
  local t=${!#}
  set -- "${@:1:$#-1}" -l
  case $t in -*) set -- "$@" -- ;; esac
  tmux display-message "$@" "${t//#/##}"
}

# The note every cockpit key is bound with, "cockpit: <name>": cockpit.tmux writes it and
# the review notice reads the jump key's back (#1102), so the text lives here once.
cockpit_key_note() { printf 'cockpit: %s' "$1"; }    # <name>

# The prefix key bound with that note, read from the binding: a record names a key moved by
# hand (#1102, #1112). Two keys noted: the one the record option names, else the first tmux
# lists. tmux compares the note itself, since it prints a note's tab, \037 and newline raw,
# and no separator splits such a line safely (#1124). It prints no line for a key whose
# format comes out empty, so no empty key can match an unset record: measured on tmux 3.8,
# and relied on here. No noted key, or list-keys failing, prints nothing.
cockpit_noted_key() {         # <name> <record option> -> the key
  # The name goes into a tmux format: [a-z0-9-] only, spelled out, since in a UTF-8 locale
  # a range can match more than ASCII (#842). Checked before tmux is asked anything.
  case ${1-} in
    ''|*[!abcdefghijklmnopqrstuvwxyz0123456789-]*)
      echo "cockpit_noted_key: bad name [${1-}]" >&2; return 2 ;;
  esac
  case ${2-} in
    @?*) ;;
    *) echo "cockpit_noted_key: bad record option [${2-}]" >&2; return 2 ;;
  esac
  local r k first=''
  r=$(tmux show-option -gqv "$2" 2>/dev/null)
  while IFS= read -r k; do
    [ "$k" = "$r" ] && { printf '%s' "$k"; return 0; }
    [ -n "$first" ] || first=$k
  done < <(tmux list-keys -T prefix \
      -F "#{?#{==:#{key_note},$(cockpit_key_note "$1")},#{key_string},}" 2>/dev/null)
  printf '%s' "$first"
}

# A file name as a title: no control character and no bidi mark, which would
# reorder what the eye reads (U+061C, U+200E-F, U+202A-E, U+2066-9, as UTF-8). tmux
# refuses a window name holding a control character (#366), and a popup border
# draws its title raw. For the title or the -n only: the editor keeps the real path.
plain_name() {
  local s=$1 b
  s=${s//[[:cntrl:]]/}
  for b in $'\xd8\x9c' $'\xe2\x80\x8e' $'\xe2\x80\x8f' $'\xe2\x80\xaa' $'\xe2\x80\xab' $'\xe2\x80\xac' \
           $'\xe2\x80\xad' $'\xe2\x80\xae' $'\xe2\x81\xa6' $'\xe2\x81\xa7' $'\xe2\x81\xa8' $'\xe2\x81\xa9'; do
    s=${s//"$b"/}
  done
  printf '%s' "$s"
}
