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

# A word for a shell (#1189): the value in single quotes, each ' and \ outside them as \'
# and \\. tmux runs a window's and a popup's command with the session's default-shell -c,
# and run-shell's with /bin/sh, so every path in one goes through here. dash, bash and zsh
# read every byte inside '...' as it is; fish reads only \' and \\ there, and neither is
# left inside. A tab, a control byte or a non-ASCII byte stays raw, where printf %q wrote
# $'...', which dash and fish do not read.
shq() {
  local s=$1 out="'" pre
  while [[ $s == *[\'\\]* ]]; do
    pre=${s%%[\'\\]*}; s=${s#"$pre"}
    out+="$pre'\\${s:0:1}'"; s=${s:1}
  done
  printf '%s' "$out$s'"
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

# A key as one argument on a tmux command line. tmux cuts an argument that ends in ; there:
# a lone ; ends the command, and M-; or C-; loses its ;. A \ before that ; keeps it (#1152).
# One call per parse the argument passes through: a key inside a bound command takes two.
cockpit_key_arg() {           # <key>
  [ $# -eq 1 ] || { echo "cockpit_key_arg: one key, got $#" >&2; return 2; }
  case $1 in *';') printf '%s\\;' "${1%;}" ;; *) printf '%s' "$1" ;; esac
}

# A key option as tmux's own config spells a ; key: set -g @cockpit_jump_key '\;' holds the
# two characters \;, which a load bound as ; before #1152. A trailing \; reads as ; (#1209).
cockpit_key_plain() {         # <option value> -> the key
  [ $# -eq 1 ] || { echo "cockpit_key_plain: one value, got $#" >&2; return 2; }
  case $1 in *'\;') printf '%s;' "${1%??}" ;; *) printf '%s' "$1" ;; esac
}

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

# Cockpit's own popups (#1248). tmux next-3.9 makes display-popup a modal floating pane, so a
# list-panes or a #{P:} walk sees it. Every cockpit popup's command starts with the mark, a
# shell assignment its program never reads, and the scans skip a modal pane started so: a
# user's own modal pane still counts. tmux keeps the command whole as the pane's argv, and
# #{pane_start_command} prints it through args_escape, which double-quotes any string with a
# space: so the mark reads back as `"COCKPIT_POPUP=1 ` at the start, whatever follows. The
# flag is read as true or false, never as empty: tmux 3.8 already prints 0 or 1 for it. The
# board holds a copy of the filter (tui/src/data.rs NOT_POPUP). popup_cmd writes the mark.
COCKPIT_POPUP_MARK='COCKPIT_POPUP=1 '
# shellcheck disable=SC2034  # read by the scripts that source this
COCKPIT_NOT_POPUP='#{?pane_modal_flag,#{?#{m:"COCKPIT_POPUP=1 *,#{pane_start_command}},0,1},1}'

# Words for a shell (#1467): each through shq, joined by blanks. tmux runs a window's and a
# popup's one-string command with the session's default-shell -c, and fish refuses a whole line
# that holds sh syntax it lacks ($?, ${...}, a bare a=b). It reads shq'd words as sh does.
shq_words() {                 # <word>... -> the words
  local c='' a
  for a in "$@"; do c+="${c:+ }$(shq "$a")"; done
  printf '%s' "$c"
}

# Every cockpit popup's command (#1248, #1467): the mark, then shq_words. A value the program
# reads that is not one of its words goes in with display-popup -e, outside the command.
popup_cmd() {                 # <word>... -> the command
  printf '%s%s' "$COCKPIT_POPUP_MARK" "$(shq_words "$@")"
}

# A popup's command that runs a shell script (#1441, #1456): /bin/sh runs <script> with the
# args as $1 on, since fish has no read -r and refuses the rest of the syntax a script holds.
popup_sh() {                  # <sh script> <arg>... -> the command
  local s=$1; shift
  popup_cmd /bin/sh -c "$s" sh "$@"
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
