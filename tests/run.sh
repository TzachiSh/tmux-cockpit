#!/usr/bin/env bash
# tmux-cockpit test rig.  Run:  tests/run.sh
# Uses a throwaway state dir, so your live agent state is never touched.
set -u

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Required (#199), and make test builds it first. Exported, so a copy of the
# plugin under test still runs the binary this tree built.
export COCKPIT_TUI_BIN=${COCKPIT_TUI_BIN:-$DIR/bin/cockpit-tui}
# Never the network: any tui-fetch a block reaches (install.sh, bootstrap.sh)
# stops before gh, curl or cargo, whatever the pin and Cargo.toml say.
export COCKPIT_TUI_OFFLINE=1
[ -x "$COCKPIT_TUI_BIN" ] || { echo "tests/run.sh: no $COCKPIT_TUI_BIN; run make tui-bin" >&2; exit 1; }
STATE=$(mktemp -d)
export XDG_RUNTIME_DIR="$STATE"
mkdir -p "$STATE/agent-state"

# Private HOME: the example configs load ~/.tmux/plugins/* by path.
export HOME="$STATE/home"
mkdir -p "$HOME/.tmux/plugins"
ln -s "$DIR" "$HOME/.tmux/plugins/tmux-cockpit"

pass=0; fail=0; skipped=0
ok()   { pass=$((pass+1)); printf '  \033[32mPASS\033[0m %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  \033[31mFAIL\033[0m %s\n     want [%s]\n     got  [%s]\n' "$1" "$2" "$3"; }
is()   { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "$2" "$3"; fi; }

plain() { sed 's/#\[[^]]*\]//g'; }
# Columns, not bytes and not characters-if-the-locale-happens-to-be-UTF-8. The
# pickers' lines carry …, ◆, ⑂ and ↵, and awk's length(), bash's ${#s} and
# `wc -m` all count BYTES under the C locale a CI box may well be in. Dropping
# UTF-8 continuation bytes and ANSI runs leaves one count per character.
ESC=$(printf '\033')
ncols() {                     # <string> -> the columns it draws in
    printf '%s' "$1" | LC_ALL=C sed "s/${ESC}\\[[0-9;]*m//g" \
        | LC_ALL=C tr -d '\200-\277' | LC_ALL=C wc -c | tr -d ' '
}

# The whole suite runs against a PRIVATE tmux server, never the user's.
#
# It used to put its scratch pane on the live session -- creating and killing
# windows in the middle of someone's work, and reading global options that
# belong to them. That was intrusive, and it was flaky: two runs at once fought
# over the same window names and one went red for reasons unrelated to the code.
# Reproduced by running the suite twice concurrently.
#
# A PATH shim redirects every bare `tmux`, including the calls inside the
# scripts under test (agent-state talks to tmux itself), so nothing had to be
# rewritten call by call. Blocks that already pass an explicit -L still work:
# tmux takes the LAST -L, and theirs comes after the shim's.
TMUX_REAL=$(command -v tmux) || TMUX_REAL=tmux
SUITE_SOCK="cockpit-suite-$$"
SHIM=$(mktemp -d)
# Deferring when $TMUX is set is load-bearing: tmux exports it to everything it
# runs, so cockpit.tmux launched by a run-shell inside one of the per-block
# servers must talk to THAT server, not this one.
cat > "$SHIM/tmux" <<SHIMEOF
#!/bin/sh
[ -n "\${TMUX:-}" ] && exec "$TMUX_REAL" "\$@"
exec "$TMUX_REAL" -L "$SUITE_SOCK" "\$@"
SHIMEOF
chmod +x "$SHIM/tmux"
# The suite itself runs inside the user's tmux, so its own $TMUX would send
# every bare call straight back to the live server. Drop it.
unset TMUX TMUX_PANE
PATH="$SHIM:$PATH"; export PATH

"$TMUX_REAL" -L "$SUITE_SOCK" -f /dev/null new-session -d -s suite \
    'exec -a claude sleep 3600' >/dev/null 2>&1
# No agent-view poller on the suite's server: a relayed agent_needs_input would start one, and
# it runs the real `claude agents` (#174). The agent-view blocks use servers of their own.
"$TMUX_REAL" -L "$SUITE_SOCK" set -g @cockpit_bg_poll off >/dev/null 2>&1
# The repo's own config, so the rendering assertions below test what we ship.
"$TMUX_REAL" -L "$SUITE_SOCK" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1

# Scratch servers are registered here so an interrupted run does not leave them
# behind. Each block still kills its own; this is the backstop.
SOCKS=""
# kill-server does NOT unlink the socket file -- tmux never removes one, by any
# exit path (measured: kill-server, last kill-session and a natural exit all
# leave it). A suite that mints 17 unique -L names per run therefore left 17
# dead socket inodes in the tmux socket dir every time; ~850 had piled up.
SOCKDIR="${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"
kill_socks() {
    for s in $SOCKS; do
        tmux -L "$s" kill-server >/dev/null 2>&1
        rm -f "$SOCKDIR/$s"
    done
}

# Several checks need a live tmux server to set per-pane options against. Skip
# them cleanly rather than failing: a red suite should mean broken code, not
# "no tmux running".
if "$TMUX_REAL" -V >/dev/null 2>&1; then HAVE_TMUX=1; else HAVE_TMUX=0; fi
# A skip is NOT a pass. Counting them as passes made a tmux-less run read
# "26 passed, 0 failed" while ~30 state-machine assertions never executed.
skip_() { skipped=$((skipped+1)); printf '  \033[33mSKIP\033[0m %s\n' "$1"; }
need_tmux() { [ "$HAVE_TMUX" = 1 ] && return 1; skip_ "$1 (no tmux server)"; return 0; }
st()    { "$DIR/scripts/agent-state" "$@"; }
# Use a dedicated scratch pane, never a live one. It runs a process named
# "claude" so the stale-state guard in the formats believes its @panestate --
# a plain shell pane is correctly disbelieved and every colour assertion fails.
#
# Use a dedicated scratch pane, never a live one. The first live pane belongs to
# a running agent whose own hooks fire concurrently and race the assertions --
# that made the suite intermittently red for reasons unrelated to the code.
PANE=$(tmux list-panes -a -F '#{pane_id}' 2>/dev/null | head -1)
if [ -z "$PANE" ]; then
    printf '  \033[31mFATAL\033[0m private tmux server did not start\n' >&2
    exit 1
fi
# Every mktemp in the suite goes through these, so an interrupted run cleans up
# after itself. The per-block `rm -rf` calls stay -- they keep the footprint
# small DURING the run; this is the backstop for the blocks still in flight.
# Measured before: a SIGINT mid-run stranded ~3 dirs, and ~10 had accumulated.
# The registry is a FILE, not a variable. Every call site is `X=$(mktmpd)`, and
# a command substitution runs in a subshell -- a variable appended there is gone
# the moment it returns, so the first version of this registered nothing at all.
TMPS=$(mktemp)
mktmpd() { local d; d=$(mktemp -d) || return 1; printf '%s\n' "$d" >> "$TMPS"; printf '%s' "$d"; }
mktmpf() { local f; f=$(mktemp)    || return 1; printf '%s\n' "$f" >> "$TMPS"; printf '%s' "$f"; }
# Polled, not slept for. A fixed sleep guesses how long a keystroke takes to
# land; these wait for the thing the next assertion reads and give up after
# $POLLS x 0.1 s. The cap is a hang guard only a red run pays for; 40 (4 s)
# lost the ^O-to-editor chain under five concurrent suites (#117).
# PATTERN is a case-pattern with NO embedded quotes -- a quote in an
# expanded pattern is a literal character: until_cap "$S" "$P" '*review>*'
POLLS=100
until_cap() {                 # <socket> <pane> <pattern>
    local i
    for i in $(seq 1 "$POLLS"); do
        # shellcheck disable=SC2254  # $3 is meant as a glob, not a literal
        case "$("$TMUX_REAL" -L "$1" capture-pane -p -J -t "$2" 2>/dev/null)" in $3) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
review_enter() {              # <socket> <ctl-pane>  the queue opens preselected on the pressed pane; ↵ reviews it
    until_cap "$1" "$2" '*fleet>*' || return 1
    "$TMUX_REAL" -L "$1" send-keys -t "$2" Enter
    until_cap "$1" "$2" '*review · base*'
}
until_file() {                # <file> <pattern>
    local i
    for i in $(seq 1 "$POLLS"); do
        # shellcheck disable=SC2254  # $2 is meant as a glob, not a literal
        case "$(cat -- "$1" 2>/dev/null)" in $2) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
# The same wait when the observable is a command's stdout rather than a pane or
# a file: a tmux format, a session list, a marker file's contents. PATTERN is a
# case-pattern with NO embedded quotes.
until_out() {                 # <pattern> <cmd...>
    local pat=$1 i; shift
    for i in $(seq 1 "$POLLS"); do
        # shellcheck disable=SC2254  # $pat is meant as a glob, not a literal
        case "$("$@" 2>/dev/null)" in $pat) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
# review-gate keys its state files on the pane id AND the server's socket path
# (issue #88). Spelled out here rather than read back from the script: a test
# that asks the code under test for its own answer agrees with any answer.
gate_key() {                  # <socket> <pane-id>
    local s; s=$("$TMUX_REAL" -L "$1" display-message -p '#{socket_path}' 2>/dev/null)
    s=${s//_/__}; printf '%s-%s' "${s//\//_.}" "${2#%}"
}
# A perf budget is CPU time, never the wall clock. Measured on this box (#75):
# under twelve background spinners the wall tooth at the 4900-row site failed
# 10/10 while its CPU sat at 0.23-0.26s. A wall budget grades the box.
CPUOUT="$STATE/cpu.out"; CPUT="$STATE/cpu.time"
cpu_is() {                    # <label> <budget-seconds> <measured-user+sys>
    if awk -v t="$3" -v b="$2" 'BEGIN{exit !(t+0 < b+0)}'; then ok "$1"
    else bad "$1" "under ${2}s of CPU" "${3}s user+sys"; fi
}
# Each budget is 3x its site's CPU measured under twelve spinners, not idle:
# fork-heavy sites cost up to 2.8x more CPU on a busy box, so idle numbers left
# one of them 1.1x of headroom. `timeout` is only a hang guard.
cpu_under() {                 # <label> <budget-seconds> <cmd...>  stdout -> $CPUOUT
    # LC_ALL=C: a comma-decimal locale makes `time` print 0,152 and awk read it
    # as 0, which passes every budget silently.
    local label=$1 budget=$2 rc; local TIMEFORMAT='%U %S' LC_ALL=C; shift 2
    { time timeout 10 "$@" </dev/null >"$CPUOUT" 2>/dev/null; rc=$?; } 2>"$CPUT"
    [ "$rc" -eq 124 ] && { bad "$label" "under ${budget}s of CPU" "hung: killed at 10s"; return; }
    cpu_is "$label" "$budget" "$(awk '{printf "%.3f", $1+$2}' "$CPUT")"
}
cleanup() {
    kill_socks
    "$TMUX_REAL" -L "$SUITE_SOCK" kill-server >/dev/null 2>&1
    rm -f "$SOCKDIR/$SUITE_SOCK"
    rm -rf "$STATE" "$SHIM"
    while read -r t; do
        [ -n "$t" ] || continue
        chmod -R u+w "$t" 2>/dev/null      # the read-only-dir test leaves one at 555
        rm -rf "$t"
    done < "$TMPS"
    rm -f "$TMPS"
}
trap cleanup EXIT
sv()    { tmux show-option -pqv -t "$PANE" @panestate 2>/dev/null; }

echo "== syntax =="
for f in "$DIR"/cockpit.tmux "$DIR"/install.sh "$DIR"/scripts/* "$DIR"/scripts/lib/*; do
    [ -f "$f" ] || continue   # scripts/lib is a directory
    if bash -n "$f" 2>/dev/null; then ok "syntax $(basename "$f")"
    else bad "syntax $(basename "$f")" "clean" "parse error"; fi
done

echo "== the REPO's shipped config renders correctly =="
# Load examples/tmux.conf into a throwaway server. Previously these assertions
# queried the USER'S running server, so a regression in the repo's own formats
# stayed green until someone reloaded the plugin.
if need_tmux "repo config"; then :; else
SOCK="cockpit-test-$$"; SOCKS="$SOCKS $SOCK"
tmux -L "$SOCK" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$SOCK" split-window -t t: 'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$SOCK" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
rp=$(tmux -L "$SOCK" list-panes -t t: -F '#{pane_id}' 2>/dev/null | head -1)
rbs() { tmux -L "$SOCK" display-message -p -t "$rp" '#{E:#{pane-border-style}}' 2>/dev/null; }
if [ -n "$rp" ]; then
    # Only the states that need you colour a border; the rest is the dim role.
    tmux -L "$SOCK" set -p -t "$rp" @panestate needs-input >/dev/null 2>&1; is "repo config: blocked is red"    "fg=themered" "$(rbs)"
    tmux -L "$SOCK" set -p -t "$rp" @panestate error       >/dev/null 2>&1; is "repo config: error is the error slot" "fg=themecyan" "$(rbs)"
    tmux -L "$SOCK" set -p -t "$rp" @panestate running     >/dev/null 2>&1; is "repo config: running is dim"    "fg=themelightgrey" "$(rbs)"
    tmux -L "$SOCK" set -pu -t "$rp" @panestate            >/dev/null 2>&1; is "repo config: idle is dim"       "fg=themelightgrey" "$(rbs)"
    # the shipped config must carry the stale-state guard, not just the live one
    case "$(tmux -L "$SOCK" show-options -gv pane-border-style)" in
        *cockpit_pstate*) ok "repo config guards against stale state" ;;
        *) bad "repo config guards against stale state" "@cockpit_pstate" "absent" ;;
    esac
    # A stray co-client at a different width makes tmux over-draw the narrower
    # one; smallest sizes the window to the smallest client so none is (#144, #145).
    is "repo config sets window-size smallest" "smallest" \
       "$(tmux -L "$SOCK" show-options -gv window-size 2>/dev/null)"
else
    for i in 1 2 3 4 5 6; do skip_ "repo config (no scratch server)"; done
fi
tmux -L "$SOCK" kill-server >/dev/null 2>&1
fi

echo "== the shipped config is usable as shipped =="
# Source the file the way a user does and check the result, not the intent: the
# plugin draws the bar, and a reload (routine) must not stack anything.
if need_tmux "shipped config is usable"; then :; else
SOCK4="cockpit-shipped-$$"; SOCKS="$SOCKS $SOCK4"
tmux -L "$SOCK4" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$SOCK4" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
case "$(tmux -L "$SOCK4" show-option -gqv pane-border-format)" in
    *cockpit_corner*) ok "sourcing it draws the plugin's border" ;;
    *) bad "sourcing it draws the plugin's border" "the corner format" "$(tmux -L "$SOCK4" show-option -gqv pane-border-format | cut -c1-80)" ;;
esac
tmux -L "$SOCK4" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
is "reloading rewrites the overflow markers once" "1" \
   "$(tmux -L "$SOCK4" show-option -gqv 'status-format[0]' | grep -o '#{?#{E:@cockpit_is_touch},,< }' | wc -l)"
is "reloading does not stack the branch hook" "1" \
   "$(tmux -L "$SOCK4" show-hooks -gw window-pane-changed | grep -c 'branch --set')"
# pane_title defaults to the HOSTNAME. Claude sets a title; aider, codex, gemini
# and crush generally do not, so without a guard every one of those tabs reads
# the same hostname instead of its window name.
tmux -L "$SOCK4" new-window -d -n buildwin 'exec -a aider sleep 300' >/dev/null 2>&1
until_out '*aider*' "$TMUX_REAL" -L "$SOCK4" display-message -p -t buildwin '#{pane_current_command}' >/dev/null
tmux -L "$SOCK4" set -g @cockpit_agents_glob '|claude|aider|' >/dev/null 2>&1
wt=$(tmux -L "$SOCK4" display-message -p -t buildwin '#{E:@cockpit_label}' 2>/dev/null)
case "$wt" in
    *"$(hostname -s 2>/dev/null)"*) bad "an agent with no title shows its window name" "buildwin" "$wt" ;;
    *buildwin*)                     ok  "an agent with no title shows its window name" ;;
    *)                              bad "an agent with no title shows its window name" "buildwin" "$wt" ;;
esac
tmux -L "$SOCK4" kill-server >/dev/null 2>&1
fi

# tests: cockpit.tmux
echo "== the menu's Switch branch names the pane it was opened on =="
# A popup is not a pane: its $PWD is the SESSION's start directory (the first
# window's), and a bare display-message inside it can come back empty. The
# prefix b bind passes -p '#{pane_id}' since d64e409; a menu row that defined a
# second popup did not, and showed the first window's tree from any window.
# The row REPLAYS prefix b (#103), so the pane id comes from the one bind.
if need_tmux "menu passes the pane id"; then :; else
SM="cockpit-menu-$$"; SOCKS="$SOCKS $SM"
"$TMUX_REAL" -L "$SM" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$SM" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
smk=$("$TMUX_REAL" -L "$SM" list-keys -T prefix 2>/dev/null | awk '$4=="a"')
case "$smk" in
    *"send-keys -K b"*) ok "the menu's Switch branch replays prefix b, which carries the pane id" ;;
    *) bad "the menu's Switch branch replays prefix b, which carries the pane id" "send-keys -K b in the a menu" "$(printf '%s' "$smk" | cut -c1-160)" ;;
esac
"$TMUX_REAL" -L "$SM" kill-server >/dev/null 2>&1
fi

echo "== prefix b acts on the pane's own directory, not the session's start dir (#38) =="
# display-popup expands no format in its shell-command: the bind's
# '#{pane_id}' reached tmux-branch as literal text and DIR fell back to the
# popup's $PWD, the session's start dir. Only a real key press shows it.
if need_tmux "prefix b pane"; then :; else
PB="cockpit-pb-$$"; PBO="cockpit-pbo-$$"; SOCKS="$SOCKS $PB $PBO"; PBD=$(mktmpd)
( export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  for b in alpha beta; do mkdir -p "$PBD/$b"; ( cd "$PBD/$b" && git init -q -b "$b" . && git commit -q --allow-empty -m init ); done ) >/dev/null 2>&1
"$TMUX_REAL" -L "$PB" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$PBD/alpha" 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$PB" source-file "$DIR/examples/full.tmux.conf" >/dev/null 2>&1
# The example conf loads ~/.tmux/plugins/tmux-cockpit, not this tree: rebind
# from the plugin under test.
"$TMUX_REAL" -L "$PB" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
# A window is named after whatever its pane runs at creation, so this one can
# read "tmux", which a bare "t" window target prefix-matches down to index 0
# instead of session t. Pinned to the name that loses, so the colon is tested.
"$TMUX_REAL" -L "$PB" rename-window -t t:0 tmux >/dev/null 2>&1
"$TMUX_REAL" -L "$PB" new-window -t t: -c "$PBD/beta" 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$PBO" -f /dev/null new-session -d -s o -x 120 -y 32 "TERM=xterm-256color exec $TMUX_REAL -L $PB attach -t t" >/dev/null 2>&1
# Keys sent to the outer pane only reach the inner server once its client is
# up, and one send-keys rather than two: the gap between two calls is wall time.
until_out '?*' "$TMUX_REAL" -L "$PB" list-clients -F '#{client_name}'
"$TMUX_REAL" -L "$PBO" send-keys -t o C-Space b
until_cap "$PBO" o '*on beta*' && ok "prefix b opens on the pane's repo (beta), not the session's start dir (alpha)" \
    || bad "prefix b opens on the pane's repo (beta), not the session's start dir (alpha)" "on beta" "$("$TMUX_REAL" -L "$PBO" capture-pane -p -t o | grep -o 'on [a-z]* ·[^ ]* [^ ]*' | head -1)"
"$TMUX_REAL" -L "$PBO" send-keys -t o Escape; sleep 0.3
"$TMUX_REAL" -L "$PBO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$PB" kill-server >/dev/null 2>&1
fi

echo "== the menu is not a way for a directory name to run tmux commands =="
# display-menu expands an item's command as a format and THEN parses it as
# tmux syntax, so a live #{pane_current_path} inside an item let a directory
# named x';run-shell ...;' run on selection. A bind parses once and is safe;
# the menu has to hand the command the FORMAT (##{...}) to expand itself.
if need_tmux "menu injection"; then :; else
MI="cockpit-mi-$$"; MO="cockpit-mo-$$"; SOCKS="$SOCKS $MI $MO"; MIDIR=$(mktmpd)
MIHOSTILE="$MIDIR/x';set-option -g @pwned INJECTED;display-popup -T '"
mkdir -p "$MIHOSTILE"
"$TMUX_REAL" -L "$MI" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$MIHOSTILE" 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$MI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$MO" -f /dev/null new-session -d -s o -x 120 -y 32 "TERM=xterm-256color exec $TMUX_REAL -L $MI attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$MI" list-clients -F '#{client_name}'
"$TMUX_REAL" -L "$MO" send-keys -t o C-b a
until_cap "$MO" o '*New window*'
"$TMUX_REAL" -L "$MO" send-keys -t o c
# ##{pane_current_path} still reaches new-window as the pane's path: the new
# window opens in the hostile directory itself.
until_out '2' "$TMUX_REAL" -L "$MI" display-message -p -t t: '#{session_windows}' >/dev/null
is "and New window still opens in the pane's own directory" "$MIHOSTILE" \
   "$("$TMUX_REAL" -L "$MI" display-message -p -t t: '#{pane_current_path}')"
# The sleep stays -- the assertion is that nothing ran, and an absence has no
# observable to poll; only elapsed time gives the injection its chance.
sleep 1
is "selecting New window from a hostile directory runs nothing" "" "$("$TMUX_REAL" -L "$MI" show-option -gqv @pwned)"
# A typed name that starts with - is a name, not a flag (rename-window --).
"$TMUX_REAL" -L "$MO" send-keys -t o C-b a
until_cap "$MO" o '*Rename window*'
"$TMUX_REAL" -L "$MO" send-keys -t o ,
until_cap "$MO" o '*(rename-window)*'
"$TMUX_REAL" -L "$MO" send-keys -t o C-u
"$TMUX_REAL" -L "$MO" send-keys -t o -l -- '-x name'
"$TMUX_REAL" -L "$MO" send-keys -t o Enter
until_out '-x name' "$TMUX_REAL" -L "$MI" display-message -p -t t: '#{window_name}' >/dev/null
is "Rename window takes a name that starts with -" "-x name" \
   "$("$TMUX_REAL" -L "$MI" display-message -p -t t: '#{window_name}')"
# The runtime case above proves the mechanism on one row. The others that
# re-expand a format are guarded structurally: the menu must carry the doubled
# form, never the single one that a quote in a cwd or a name turned into a command.
menu=$(sed -n '/^rows=()/,/display-menu/p' "$DIR/cockpit.tmux")
for bad_sub in "-c '#{pane_current_path}'" "-I '#W'" "-I '#S'" "session #S?" "pane #P?" "window #I?"; do
    case "$menu" in *"$bad_sub"*) bad "menu re-expands a format singly: $bad_sub" "no single #{ here" "present" ;; *) ok "menu does not re-expand singly: $bad_sub" ;; esac
done
for good_sub in "-c '##{pane_current_path}'" "-I '##W'" "-I '##S'" "session ##S?" "pane ##P?" "window ##I?"; do
    case "$menu" in *"$good_sub"*) ok "menu doubles the format: $good_sub" ;; *) bad "menu doubles the format: $good_sub" "$good_sub present" "missing" ;; esac
done
"$TMUX_REAL" -L "$MO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$MI" kill-server >/dev/null 2>&1
fi

echo "== the shipped config against a real agent pane =="
tp="$PANE"; [ "$HAVE_TMUX" = 1 ] || tp=""
if [ -n "$tp" ]; then
    save=$(tmux show-option -pqv -t "$tp" @panestate 2>/dev/null)
    bs() { tmux display-message -p -t "$tp" '#{E:#{pane-border-style}}' 2>/dev/null; }
    tmux set -p -t "$tp" @panestate running     >/dev/null 2>&1; is "running border is dim"   "fg=themelightgrey" "$(bs)"
    tmux set -p -t "$tp" @panestate needs-input >/dev/null 2>&1; is "blocked border is red"   "fg=themered" "$(bs)"
    tmux set -p -t "$tp" @panestate error       >/dev/null 2>&1; is "error border is the error slot" "fg=themecyan" "$(bs)"
    tmux set -pu -t "$tp" @panestate            >/dev/null 2>&1; is "idle border is dim"      "fg=themelightgrey" "$(bs)"
    # the redraw path must contain no calls to our scripts
    allfmt=$(for o in status-left status-right window-status-format window-status-current-format pane-border-format pane-border-style; do tmux show-options -gv "$o" 2>/dev/null; done)
    case "$allfmt" in
        *tmux-cockpit/scripts/*) bad "no plugin script runs on redraw" "none" "found one" ;;
        *) ok "no plugin script runs on redraw" ;;
    esac
    [ -n "$save" ] && tmux set -p -t "$tp" @panestate "$save" >/dev/null 2>&1
else
    for i in 1 2 3 4 5; do skip_ "native rendering"; done
fi

echo "== state transitions =="
if need_tmux "state transitions"; then :; else
st running "$PANE";      is "hook: running"  "running"     "$(sv)"
st needs-input "$PANE";  is "hook: wait"     "needs-input" "$(sv)"
# from needs-input, done is refused by design -- see the sticky rule below
st done "$PANE";         is "hook: done blocked while blocked" "needs-input" "$(sv)"
st off "$PANE";          is "hook: off"      ""            "$(sv)"

fi
echo "== notify: the cancel fix =="
if need_tmux "notify"; then :; else
tmux set -p -t "$PANE" @panestate running >/dev/null 2>&1
echo '{"message":"Claude is waiting for your input"}' | st notify "$PANE"
is "idle clears running" "" "$(sv)"
tmux set -p -t "$PANE" @panestate done >/dev/null 2>&1
echo '{"message":"Claude is waiting for your input"}' | st notify "$PANE"
is "idle KEEPS done" "done" "$(sv)"
# Claude emits "waiting for your input" while blocked on a permission prompt,
# so clearing there wiped the alert precisely because it was unanswered.
tmux set -p -t "$PANE" @panestate needs-input >/dev/null 2>&1
echo '{"message":"Claude is waiting for your input"}' | st notify "$PANE"
is "idle KEEPS needs-input" "needs-input" "$(sv)"
st running "$PANE"
is "resuming work clears needs-input" "running" "$(sv)"
tmux set -p -t "$PANE" @panestate running >/dev/null 2>&1
echo '{"message":"Claude needs your permission"}' | st notify "$PANE"
is "permission -> wait" "needs-input" "$(sv)"
printf '{"message":"Claude needs your permission to use Bash"}' \
    | "$DIR/scripts/agent-state" notify "$PANE"
is "the ask text is stored beside the alert" \
   "Claude needs your permission to use Bash" \
   "$(tmux show-option -pqv -t "$PANE" @panestate_msg)"
st running "$PANE"
is "resuming work clears the stale ask" "" \
   "$(tmux show-option -pqv -t "$PANE" @panestate_msg)"
# "needs your permission" can appear WITHOUT a usable message value (missing,
# or an explicit empty string) -- this payload reaches extraction, unlike the
# unrelated-notification arm above, and must CLEAR a stale ask rather than
# leaving a PREVIOUS pane's question standing beside a brand-new alert.
tmux set -p -t "$PANE" @panestate_msg "stale ask from a different pane" >/dev/null 2>&1
printf '{"reason":"Claude needs your permission","message":""}' \
    | "$DIR/scripts/agent-state" notify "$PANE"
is "an empty message clears the stale ask" "" \
   "$(tmux show-option -pqv -t "$PANE" @panestate_msg)"
tmux set -p -t "$PANE" @panestate running >/dev/null 2>&1
echo '{"message":"unrelated thing"}' | st notify "$PANE"
is "unrelated ignored" "running" "$(sv)"
# error is sticky too: an agent that failed and then went idle must keep its flag
tmux set -p -t "$PANE" @panestate error >/dev/null 2>&1
echo '{"message":"Claude is waiting for your input"}' | st notify "$PANE"
is "idle KEEPS error" "error" "$(sv)"

# tmux -t prefix-matches SESSION names, so a non-pane argument could resolve to
# a real pane elsewhere and be written to.
tmux set -pu -t "$PANE" @panestate >/dev/null 2>&1
st error "notapane" 2>/dev/null
is "a non-pane target writes nothing" "" "$(sv)"
st error "$PANE"
is "but a real pane id still works" "error" "$(sv)"
tmux set -pu -t "$PANE" @panestate >/dev/null 2>&1

fi
echo "== notify: notification_type picks the state, the text only without it (#157) =="
if need_tmux "notify notification_type"; then :; else
# <type> <message> <state before> -> the state after
nt() { tmux set -p -t "$PANE" @panestate "$3" >/dev/null 2>&1
    printf '{"message":"%s","notification_type":"%s"}' "$2" "$1" | st notify "$PANE"; sv; }
is "permission_prompt -> needs-input"    "needs-input" "$(nt permission_prompt 'Claude a besoin de votre permission' running)"
is "agent_needs_input -> needs-input"    "needs-input" "$(nt agent_needs_input 'x' running)"
is "elicitation_dialog -> needs-input"   "needs-input" "$(nt elicitation_dialog 'x' running)"
is "idle_prompt clears running"          ""            "$(nt idle_prompt 'Claude attend votre saisie' running)"
is "idle_prompt keeps done"              "done"        "$(nt idle_prompt 'x' done)"
is "idle_prompt keeps needs-input"       "needs-input" "$(nt idle_prompt 'x' needs-input)"
is "idle_prompt keeps error"             "error"       "$(nt idle_prompt 'x' error)"
is "an unknown type is ignored"          "running"     "$(nt auth_success 'x' running)"
# The type wins over the text: English words in a typed payload decide nothing.
is "a typed payload ignores the old text" "running" \
   "$(nt agent_completed 'Claude needs your permission' running)"
is "the typed ask is stored beside the alert" "Claude a besoin de votre permission" \
   "$(nt permission_prompt 'Claude a besoin de votre permission' running >/dev/null
      tmux show-option -pqv -t "$PANE" @panestate_msg)"
# Older Claude Code sends no notification_type: the text still classifies.
tmux set -p -t "$PANE" @panestate running >/dev/null 2>&1
echo '{"message":"Claude needs your permission"}' | st notify "$PANE"
is "no type: the permission text still raises needs-input" "needs-input" "$(sv)"
tmux set -p -t "$PANE" @panestate running >/dev/null 2>&1
echo '{"message":"Claude is waiting for your input"}' | st notify "$PANE"
is "no type: the idle text still clears running" "" "$(sv)"
# A review queued (spool) or sent into an ESC-cancelled pane outlives the idle
# notification about 60 s later; only a session that ends takes it along.
tmux set -p -t "$PANE" @cockpit_review sent >/dev/null 2>&1
tmux set -p -t "$PANE" @cockpit_review_spool /nonexistent >/dev/null 2>&1
is "idle_prompt on a cancelled pane clears it" "" "$(nt idle_prompt 'Claude is waiting for your input' running)"
is "but keeps a sent review" "sent" "$(tmux show-option -pqv -t "$PANE" @cockpit_review)"
is "and a queued one" "/nonexistent" "$(tmux show-option -pqv -t "$PANE" @cockpit_review_spool)"
st off "$PANE"
is "while a session end still clears them" "" \
   "$(tmux show-option -pqv -t "$PANE" @cockpit_review)$(tmux show-option -pqv -t "$PANE" @cockpit_review_spool)"
tmux set -pu -t "$PANE" @panestate >/dev/null 2>&1
fi

echo "== SessionStart runs off on startup, resume and clear, never on compact (#157) =="
# Claude Code matches the SessionStart source against the matcher as a regex;
# compaction mid-work used to run `off` and drop the pane's review mark.
ssm() { jq -r --arg s "$1" '[.hooks.SessionStart[]
        | select(.hooks[].command | test("agent-state off"))
        | select((.matcher // "") as $m | $m == "" or ($s | test("^(" + $m + ")$")))] | length' \
        "$DIR/examples/claude-hooks.json"; }
is "SessionStart off fires on startup, resume and clear" "1 1 1" "$(ssm startup) $(ssm resume) $(ssm clear)"
is "but not on compact" "0" "$(ssm compact)"

# tests: cockpit.tmux
echo "== stale state is disbelieved =="
if need_tmux "stale state"; then :; else
# Hooks cannot fire if an agent is killed or crashes, so @panestate can outlive
# the agent. The formats only believe it while the pane is actually running one.
w=$(tmux new-window -d -P -F '#{window_index}' -n cockpit-test 'bash' 2>/dev/null)
if [ -n "$w" ]; then
    # The border style below is judged off pane_current_command, so wait for
    # the shell to BE the pane's command rather than guess at the spawn.
    until_out bash tmux display-message -p -t ":$w" '#{pane_current_command}'
    tp2=$(tmux list-panes -t ":$w" -F '#{pane_id}' 2>/dev/null | head -1)
    tmux set -p -t "$tp2" @panestate needs-input >/dev/null 2>&1
    is "option is still set"        "needs-input" "$(tmux show-option -pqv -t "$tp2" @panestate)"
    is "but a shell pane reads idle" "fg=themelightgrey" "$(tmux display-message -p -t "$tp2" '#{E:#{pane-border-style}}' 2>/dev/null)"
    tmux kill-window -t ":$w" >/dev/null 2>&1
else
    skip_ "stale state: live pane kept"; skip_ "stale state: dead pane swept"
fi
fi

echo "== the normal completion path =="
if need_tmux "completion path"; then :; else
# running -> done is how EVERY agent finishes. A ranking that blocked it meant
# green could never appear; the bug hid behind a test that reset to off first.
st running "$PANE"; is "agent starts working" "running" "$(sv)"
st done "$PANE";    is "and finishing shows done" "done" "$(sv)"
tmux set -pu -t "$PANE" @panestate >/dev/null 2>&1
st done "$PANE";    is "done from idle also works" "done" "$(sv)"
tmux set -pu -t "$PANE" @panestate >/dev/null 2>&1
fi

echo "== realistic hook sequences =="
if need_tmux "hook sequences"; then :; else
seq_() { tmux set -pu -t "$PANE" @panestate >/dev/null 2>&1; for x in "$@"; do st "$x" "$PANE"; done; sv; }
# A failed subagent used to finish GREEN: SubagentStop->running cleared the error,
# then Stop wrote done. That hook is gone; error now survives to the end.
is "a failed run ends red, not green" "error" "$(seq_ running running error done)"
# Approving a permission fires no hook; PreToolUse now clears the red the moment
# the tool starts, instead of after it finishes.
is "approval clears red when the tool starts" "running" "$(seq_ needs-input running)"
is "a clean run still ends green" "done" "$(seq_ running running done)"
# TeammateIdle -> wake: a teammate's idle notification wakes the lead with no hook.
is "a teammate idling wakes a finished lead" "running" "$(seq_ running done wake)"
is "and a lead that never ran" "running" "$(seq_ wake)"
is "but not one blocked on a prompt" "needs-input" "$(seq_ running needs-input wake)"
is "nor one that errored" "error" "$(seq_ running error wake)"
tmux set-option -p -t "$PANE" @cockpit_review pending >/dev/null 2>&1
st wake "$PANE"; is "and a pending review survives the wake" "pending" "$(tmux show-option -pqv -t "$PANE" @cockpit_review)"
tmux set-option -pu -t "$PANE" @cockpit_review >/dev/null 2>&1
tmux set -pu -t "$PANE" @panestate >/dev/null 2>&1
fi

echo "== state ranking: needs-input > error > running > done =="
if need_tmux "state ranking"; then :; else
rk() { tmux set -p -t "$PANE" @panestate "$1" >/dev/null 2>&1; st "$2" "$PANE" 2>/dev/null; sv; }
is "error cannot demote a block"      "needs-input" "$(rk needs-input error)"
is "done cannot demote an error"      "error"       "$(rk error done)"
is "error does replace running"       "error"       "$(rk running error)"
is "running always wins (work resumed)" "running"   "$(rk needs-input running)"
is "off always clears"                ""            "$(rk needs-input off)"
is "error is a valid state"           "error"       "$(rk done error)"

fi
echo "== needs-input outranks done =="
if need_tmux "needs-input outranks done"; then :; else
# A blocked turn still ends, so Stop fires with the permission prompt still up.
# Letting that write "done" turned a red "answer me" into a green "finished".
tmux set -p -t "$PANE" @panestate needs-input >/dev/null 2>&1
st done "$PANE";     is "Stop cannot demote a blocked pane"  "needs-input" "$(sv)"
st running "$PANE";  is "approving it does clear the block"  "running"     "$(sv)"
tmux set -p -t "$PANE" @panestate needs-input >/dev/null 2>&1
st off "$PANE";      is "session end still clears it"        ""            "$(sv)"

fi
echo "== done survives scanning =="
if need_tmux "done survives scanning"; then :; else
# Switching windows is how you SCAN; the flag must outlive a glance and only
# clear when the pane actually starts working again.
tmux set -p -t "$PANE" @panestate done >/dev/null 2>&1
st running "$PANE";  is "a new prompt clears done"      "running" "$(sv)"
tmux set -p -t "$PANE" @panestate done >/dev/null 2>&1
echo '{"message":"Claude is waiting for your input"}' | st notify "$PANE"
is "idle notification does not clear done" "done" "$(sv)"
is "and done is still intact"  "done" "$(sv)"

fi
# tests: cockpit.tmux
echo "== spaced window names survive the formats =="
if need_tmux "spaced window names"; then :; else
sw=$(tmux new-window -d -P -F '#{window_index}' -n 'my project' 'sleep 30' 2>/dev/null)
if [ -n "$sw" ]; then
    # automatic-rename fires on the pane's command, so waiting for that command
    # to be there is what gives it its chance to clobber the name.
    until_out sleep tmux display-message -p -t ":$sw" '#{pane_current_command}'
    # automatic-rename must not clobber an explicit name, and the format must
    # carry the space through -- the old shell version truncated at it.
    is "explicit name keeps its space" "my project" "$(tmux display-message -p -t ":$sw" '#{window_name}' 2>/dev/null)"
    nm=$(tmux display-message -p -t ":$sw" '#{window_index}:#{window_name} ' 2>/dev/null)
    is "and survives an index:name format" "$sw:my project " "$nm"
    tmux kill-window -t ":$sw" >/dev/null 2>&1
else
    skip_ "spaced name keeps its space"; skip_ "spaced name survives a format"
fi
fi

echo "== rejects bogus states =="
if need_tmux "rejects bogus states"; then :; else
tmux set -p -t "$PANE" @panestate done >/dev/null 2>&1
for b in clear-done RUNNING banana 'running; rm -rf /'; do st "$b" "$PANE" 2>/dev/null; done
is "unknown states never written" "done" "$(sv)"

fi
echo "== safety =="
tmux set -pu -t "$PANE" @panestate >/dev/null 2>&1
( unset TMUX_PANE; st running '' ) 2>/dev/null
is "no pane id and no TMUX_PANE writes nothing" "" "$(sv)"
# Per-pane options die with the pane, so stale state cannot outlive a pane and
# there is no sweep that could delete a live one.

echo "== agent-state: a hook transition costs one tmux round-trip =="
# `running` fires on UserPromptSubmit and on EVERY PreToolUse/PostToolUse
# (examples/claude-hooks.json), and the hook has to return before the tool call
# proceeds -- so every extra `tmux` here is a fork plus a server round-trip on
# the agent's critical path. Measured before the batching: 12 invocations for
# `running`, 10 for `done`, 12 for `needs-input` (one read, one write per
# option, then six more for the three precomputed counters).
#
# Counted with a PATH shim ahead of the suite's own that logs one line per call
# and still execs the real binary, so the pane's state stays correct and the
# count is honest. (`strace -f -e execve` is not portable; the shim is the same
# trick the branch-row and statusline-write blocks use.)
if need_tmux "agent-state round-trips"; then :; else
RT="cockpit-rt-$$"; SOCKS="$SOCKS $RT"
"$TMUX_REAL" -L "$RT" -f /dev/null new-session -d -s t -x 80 -y 24 \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RT" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
RTP=$("$TMUX_REAL" -L "$RT" list-panes -t t -F '#{pane_id}')
RTS=$(mktmpd); RTLOG="$RTS/calls"
cat > "$RTS/tmux" <<SHIMEOF
#!/bin/sh
printf '%s\n' "\$*" >> "$RTLOG"
exec "$TMUX_REAL" "\$@"
SHIMEOF
chmod +x "$RTS/tmux"
# TMUX points agent-state's bare \`tmux\` at this private server, the production
# path the fleet block drives too.
rtcall() { : > "$RTLOG"
    PATH="$RTS:$PATH" TMUX="$SOCKDIR/$RT,0,0" "$DIR/scripts/agent-state" \
        "$1" "$RTP" >/dev/null 2>&1
    wc -l < "$RTLOG" | tr -d ' '; }
rtrun=$(rtcall running)
rtdone=$(rtcall done)
is "running costs one tmux invocation" "1" "$rtrun"
is "done costs one tmux invocation" "1" "$rtdone"
# Batching must not buy the count with correctness: every option the unbatched
# version wrote is still written, with the same value.
is "the pane state still lands" "done" \
   "$("$TMUX_REAL" -L "$RT" show-option -pqv -t "$RTP" @panestate)"
is "the window dot still ranks it" "done" \
   "$("$TMUX_REAL" -L "$RT" show-option -wqv -t "$RTP" @cockpit_win_state)"
rtneeds=$(rtcall needs-input)
is "needs-input costs one tmux invocation" "1" "$rtneeds"
is "the session count still counts" "1" \
   "$("$TMUX_REAL" -L "$RT" show-option -qv -t t: @cockpit_sess_nb)"
is "the fleet counter still counts" '#[fg=themered bold]▲1#[default] ' \
   "$("$TMUX_REAL" -L "$RT" show-option -gqv @cockpit_fleet)"
# The writes happen inside one tmux command now, so a server hook is what sees
# them. Not -b: the hook's shell finishes before the hook's client returns.
RTSETS="$RTS/sets"; : > "$RTSETS"
"$TMUX_REAL" -L "$RT" set-hook -g after-set-option \
    "run-shell 'echo \"#{hook_arguments}\" >> $RTSETS'" >/dev/null 2>&1
# An unchanged state still costs exactly one round-trip and writes nothing.
rtsame=$(rtcall needs-input)
is "an unchanged state costs one call and no writes" "1 0" "$rtsame $(wc -l < "$RTSETS" | tr -d ' ')"
# If the compare ever saw the stored fleet string differently from the fresh one
# the guard would never match, and every transition on the server would rewrite
# the globals -- a server-wide redraw apiece. A second agent pane whose transition
# leaves the counts at 1 must write nothing to them.
"$TMUX_REAL" -L "$RT" new-window -t t: 'exec -a claude bash' >/dev/null 2>&1
RTP2=$("$TMUX_REAL" -L "$RT" list-panes -t t:1 -F '#{pane_id}')
PATH="$RTS:$PATH" TMUX="$SOCKDIR/$RT,0,0" "$DIR/scripts/agent-state" \
    running "$RTP2" >/dev/null 2>&1
is "the hook sees the transition's own writes" "1" "$(grep -c '@panestate running' "$RTSETS")"
is "an unchanged fleet count is never rewritten" "0" \
   "$(grep -c '@cockpit_fleet\|@cockpit_sess_' "$RTSETS")"
is "and the stored count still names its theme slot" \
   '#[fg=themered bold]▲1#[default] ' \
   "$("$TMUX_REAL" -L "$RT" show-option -gqv @cockpit_fleet)"
"$TMUX_REAL" -L "$RT" kill-server >/dev/null 2>&1
fi

echo "== agent-state: a transition wakes cockpit-tui, and never a pid that is not it =="
# The push channel's sender (issue #61). @cockpit_tui_pid outlives the process
# that published it and pids get recycled, and SIGUSR1's default action is
# terminate -- so an unverified kill shoots a stranger's process, as one probe
# already did on the live server.
# Known gap, no code: a second ask on an ALREADY-blocked pane exits at the
# sticky needs-input guard (scripts/agent-state) before reaching the sender,
# so that question reaches the board on its 1 s poll rather than on a push.
if need_tmux "agent-state push sender"; then :; else
PS="cockpit-push-$$"; SOCKS="$SOCKS $PS"
"$TMUX_REAL" -L "$PS" -f /dev/null new-session -d -s t -x 80 -y 24 \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PS" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
PSP=$("$TMUX_REAL" -L "$PS" list-panes -t t -F '#{pane_id}')
PSD=$(mktmpd)
pstrans() {                   # one real transition on this block's server
    PATH="$SHIM:$PATH" TMUX="$SOCKDIR/$PS,0,0" "$DIR/scripts/agent-state" \
        "$1" "$PSP" >/dev/null 2>&1
}
# The recycled-pid stand-in wears cockpit-tui in argv0 and `sleep` in comm, so
# it also pins WHICH name the guard reads: ps argv0 would signal this one.
"$TMUX_REAL" -L "$PS" new-window -d -t t: 'exec -a cockpit-tui sleep 30' >/dev/null 2>&1
PSSLEEP=$("$TMUX_REAL" -L "$PS" list-panes -t t:1 -F '#{pane_pid}')
"$TMUX_REAL" -L "$PS" set -g @cockpit_tui_pid "$PSSLEEP" >/dev/null 2>&1
pstrans running
pstrans needs-input
if kill -0 "$PSSLEEP" 2>/dev/null; then ok "a stale pid that is not cockpit-tui is never signalled"
else bad "a stale pid that is not cockpit-tui is never signalled" "the process still alive" "USR1 killed it"; fi
# The wake half. A symlink named cockpit-tui gives the process that comm, so
# this is the same check the real binary passes; `sleep & wait` because bash
# runs no trap while a foreground child is running.
ln -s "$(command -v bash)" "$PSD/cockpit-tui" 2>/dev/null
PSMARK="$PSD/woke"
"$PSD/cockpit-tui" -c 'trap "printf woke > \"$1\"; exit 0" USR1; sleep 30 & wait' _ "$PSMARK" &
PSPID=$!
"$TMUX_REAL" -L "$PS" set -g @cockpit_tui_pid "$PSPID" >/dev/null 2>&1
# running, not done: needs-input is sticky and a done would fail the
# transition's guard, so no transition would happen to push.
pstrans running
if until_file "$PSMARK" '*woke*'; then ok "a transition signals the running cockpit-tui"
else bad "a transition signals the running cockpit-tui" "woke" "$(cat "$PSMARK" 2>/dev/null)"; fi
# An exact count: the pid rides the one round-trip the transition already pays
# for, so a `tmux show-option` of its own would show up as 2. The ps shim
# beside it pins the other half -- the verify must stay a builtin.
PSC=$(mktmpd); PSCLOG="$PSC/calls"; PSPSLOG="$PSC/pscalls"; PSERR="$PSC/err"
cat > "$PSC/tmux" <<SHIMEOF
#!/bin/sh
printf '%s\n' "\$*" >> "$PSCLOG"
exec "$TMUX_REAL" "\$@"
SHIMEOF
cat > "$PSC/ps" <<SHIMEOF
#!/bin/sh
printf '%s\n' "\$*" >> "$PSPSLOG"
exec "$(command -v ps)" "\$@"
SHIMEOF
chmod +x "$PSC/tmux" "$PSC/ps"
pscount() { : > "$PSCLOG"; : > "$PSPSLOG"; : > "$PSERR"
    PATH="$PSC:$PATH" TMUX="$SOCKDIR/$PS,0,0" "$DIR/scripts/agent-state" \
        "$1" "$PSP" >/dev/null 2>"$PSERR"
    wc -l < "$PSCLOG" | tr -d ' '; }
is "a transition with the view up still costs 1 tmux round-trip" "1" "$(pscount needs-input)"
kill "$PSPID" 2>/dev/null; wait "$PSPID" 2>/dev/null
# An empty option is the common case (no TUI running) and must cost nothing and
# signal nobody -- `kill -USR1 ''` would be an error on every hook in the world.
"$TMUX_REAL" -L "$PS" set -gu @cockpit_tui_pid >/dev/null 2>&1
is "with no pid published the transition still succeeds" "0" "$(pstrans running; echo $?)"
is "and still costs the same 1 round-trip" "1" "$(pscount needs-input)"
# The state the guard exists for: a pid nobody unpublished, because SIGKILL, a
# crash and a power cut all leave @cockpit_tui_pid standing. It is absent from
# /proc, which is where a `read || ps` fallback forked ps on every transition
# and leaked bash's redirect failure onto the hook's real stderr.
sleep 0 & PSDEAD=$!; wait "$PSDEAD" 2>/dev/null
"$TMUX_REAL" -L "$PS" set -g @cockpit_tui_pid "$PSDEAD" >/dev/null 2>&1
is "a dead published pid still costs 1 round-trip" "1" "$(pscount running)"
is "and forks no ps to ask about it" "" "$(cat "$PSPSLOG")"
is "and says nothing on the hook's stderr" "" "$(cat "$PSERR")"
"$TMUX_REAL" -L "$PS" kill-server >/dev/null 2>&1
fi

echo "== agent-state: 20 transitions in parallel leave the fleet counter exact =="
# Hooks run in parallel. A read in one tmux call and the counter write in
# another let a stale count land last; the transition is one command now.
if need_tmux "agent-state parallel transitions"; then :; else
PT="cockpit-par-$$"; SOCKS="$SOCKS $PT"
"$TMUX_REAL" -L "$PT" -f /dev/null new-session -d -s t -x 80 -y 24 \
    'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$PT" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
for i in $(seq 2 20); do
    "$TMUX_REAL" -L "$PT" new-window -d -t t: 'exec -a claude sleep 300' >/dev/null 2>&1
done
PTPANES=$("$TMUX_REAL" -L "$PT" list-panes -a -F '#{pane_id}')
until_out '*claude*' "$TMUX_REAL" -L "$PT" display-message -p -t "t:19" '#{pane_current_command}' >/dev/null
ptall() {                     # <state>: every pane at once, then wait for all
    local p pids=""
    for p in $PTPANES; do
        TMUX="$SOCKDIR/$PT,0,0" "$DIR/scripts/agent-state" "$1" "$p" >/dev/null 2>&1 &
        pids="$pids $!"
    done
    wait $pids                # only ours: the suite may have other jobs running
}
ptall needs-input
is "20 panes blocking at once count ▲20" '#[fg=themered bold]▲20#[default] ' \
   "$("$TMUX_REAL" -L "$PT" show-option -gqv @cockpit_fleet)"
is "and the session and fleet counts agree" "20 20" \
   "$("$TMUX_REAL" -L "$PT" show-option -qv -t t: @cockpit_sess_nb) $("$TMUX_REAL" -L "$PT" show-option -gqv @cockpit_fleet_nb)"
ptall error
ptall running
ptall error
is "20 panes erroring at once after a resume count ✕20" '#[fg=themecyan]✕20#[default] ' \
   "$("$TMUX_REAL" -L "$PT" show-option -gqv @cockpit_fleet)"
ptall off
is "and 20 ending at once clear it" "" "$("$TMUX_REAL" -L "$PT" show-option -gqv @cockpit_fleet)"
"$TMUX_REAL" -L "$PT" kill-server >/dev/null 2>&1
fi

echo "== agent-state recount: ◆N counts panes with a pending review, off clears them =="
if need_tmux "agent-state recount"; then :; else
tmux set-option -p -t "$PANE" @cockpit_review pending >/dev/null 2>&1
st recount "$PANE"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'◆1'*) ok "recount adds ◆1 to the fleet string" ;;
    *) bad "recount adds ◆1 to the fleet string" "…◆1…" "$(tmux show-option -gqv @cockpit_fleet)" ;;
esac
is "...and ◆ to the window's tab flag" "1" "$(tmux show-option -wqv -t "$PANE" @cockpit_win_review)"
tmux set-option -p -t "$PANE" @cockpit_review sent >/dev/null 2>&1
st recount "$PANE"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'◆'*) bad "a sent review is not a pending one" "no ◆" "$(tmux show-option -gqv @cockpit_fleet)" ;;
    *) ok "a sent review is not a pending one" ;;
esac
is "...and the tab flag goes with it" "" "$(tmux show-option -wqv -t "$PANE" @cockpit_win_review)"
tmux set-option -p -t "$PANE" @cockpit_review pending >/dev/null 2>&1
tmux set-option -p -t "$PANE" @cockpit_review_spool /nonexistent >/dev/null 2>&1
tmux set-option -p -t "$PANE" @panestate needs-input >/dev/null 2>&1
st recount "$PANE"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'▲'*'◆1'*) ok "recount runs under a sticky needs-input too, ▲ before ◆" ;;
    *) bad "recount runs under a sticky needs-input too, ▲ before ◆" "▲N … ◆1" "$(tmux show-option -gqv @cockpit_fleet)" ;;
esac
st off "$PANE"
is "off clears the review state" "" "$(tmux show-option -pqv -t "$PANE" @cockpit_review)"
is "off clears a queued spool" "" "$(tmux show-option -pqv -t "$PANE" @cockpit_review_spool)"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'◆'*) bad "and the ◆ is gone" "no ◆" "$(tmux show-option -gqv @cockpit_fleet)" ;;
    *) ok "and the ◆ is gone" ;;
esac
fi

echo "== worktree-create: EnterWorktree lands under ~/.cache/cockpit-wt/<name>, on a branch of that name =="
# issue #3: an agent that works on a branch has to LIVE in that worktree, or
# every cockpit read (review, ◆, fleet, status line) describes the wrong tree.
# The WorktreeCreate hook replaces claude's own .claude/worktrees/ placement;
# claude reads the last stdout line as the path, so anything else goes to stderr.
WCREPO=$(mktmpd); WCCACHE=$(mktmpd)
( cd "$WCREPO" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; git branch old ) >/dev/null 2>&1
wc_run() { ( cd "$WCREPO" && printf '%s' "$1" | XDG_CACHE_HOME="$WCCACHE" "$DIR/scripts/worktree-create" 2>"$WCCACHE/err" ); }
is "a new name gets a worktree under cockpit-wt and prints its path" "$WCCACHE/cockpit-wt/feat-x" \
   "$(wc_run '{"session_id":"s","cwd":"'"$WCREPO"'","hook_event_name":"WorktreeCreate","name":"feat-x"}' | tail -n1)"
is "and that worktree is on a branch of the same name" "feat-x" "$(git -C "$WCCACHE/cockpit-wt/feat-x" branch --show-current 2>/dev/null)"
is "an existing branch is checked out, not recreated" "old" \
   "$(wc_run '{"name": "old", "cwd": "'"$WCREPO"'"}' >/dev/null; git -C "$WCCACHE/cockpit-wt/old" branch --show-current 2>/dev/null)"
is "an existing worktree is reused (claude --worktree <name> reopens it)" "$WCCACHE/cockpit-wt/old" "$(wc_run '{"name":"old"}' | tail -n1)"
is "only the path reaches stdout" "1" "$(wc_run '{"name":"feat-y"}' | grep -c .)"
wc_run '{"name":"../escape"}' >/dev/null; is "a name with .. is refused" "1" "$?"
[ -e "$WCCACHE/escape" ] && bad "a name with .. is refused" "nothing created" "$WCCACHE/escape exists"
wc_run '{"cwd":"x"}' >/dev/null; is "no name is refused" "1" "$?"
wc_run '{"name":"."}' >/dev/null; is "a bare . is refused" "1" "$?"
# rev-parse resolves revision syntax: main@{0} "verified" as an existing
# branch and checked out a detached historical commit instead of a branch.
wc_run '{"name":"main@{0}"}' >/dev/null; is "revision syntax is refused, not resolved" "1" "$?"
[ -e "$WCCACHE/cockpit-wt/main@{0}" ] && bad "and nothing is checked out for it" "nothing" "a tree"
is "HOME unset is a refusal, not a bash error" "1" "$(cd "$WCREPO" && printf '{"name":"h"}' | env -u HOME -u XDG_CACHE_HOME "$DIR/scripts/worktree-create" >/dev/null 2>&1; echo $?)"
# The cache is one namespace for every repo on the machine.
WCREPO2=$(mktmpd); ( cd "$WCREPO2" && git init -q -b main . ) >/dev/null 2>&1
( cd "$WCREPO2" && printf '{"name":"old"}' | XDG_CACHE_HOME="$WCCACHE" "$DIR/scripts/worktree-create" >/dev/null 2>"$WCCACHE/err2" ); is "another repo asking for a taken name is refused" "1" "$?"
case "$(cat "$WCCACHE/err2")" in *"not a worktree of this repo"*) ok "and told why" ;; *) bad "and told why" "not a worktree of this repo" "$(cat "$WCCACHE/err2")" ;; esac
mkdir -p "$WCCACHE/cockpit-wt/plain"
wc_run '{"name":"plain"}' >/dev/null; is "a plain directory of that name is refused, not handed over" "1" "$?"
rm -rf "$WCCACHE/cockpit-wt/feat-y"
is "a removed tree's stale entry does not block the name forever" "$WCCACHE/cockpit-wt/feat-y" "$(wc_run '{"name":"feat-y"}' | tail -n1)"
is "a relative XDG_CACHE_HOME is ignored, not resolved into the repo" "$WCCACHE/.cache/cockpit-wt/rel-x" \
   "$(cd "$WCREPO" && printf '{"name":"rel-x"}' | XDG_CACHE_HOME=relcache HOME="$WCCACHE" "$DIR/scripts/worktree-create" 2>/dev/null | tail -n1)"
# WorktreeRemove fires when claude means to remove (a subagent finished, the
# human chose remove on exit). Mirror its own rule for git-made trees: a clean
# tree and a merged branch go, anything with work in it stays.
wr_run() { ( cd "$WCREPO" && printf '{"session_id":"s","hook_event_name":"WorktreeRemove","worktree_path":"%s"}' "$1" | XDG_CACHE_HOME="$WCCACHE" "$DIR/scripts/worktree-remove" 2>"$WCCACHE/rerr" ); }
wr_run "$WCCACHE/cockpit-wt/feat-y"; is "a clean tree on a merged branch is removed" "0" "$?"
[ -d "$WCCACHE/cockpit-wt/feat-y" ] && bad "and its directory is gone" "gone" "still there"
is "and its branch too" "" "$(git -C "$WCREPO" branch --list feat-y)"
printf 'edit\n' >> "$WCCACHE/cockpit-wt/old/f"
wr_run "$WCCACHE/cockpit-wt/old"; [ -d "$WCCACHE/cockpit-wt/old" ] && ok "a dirty tree is kept" || bad "a dirty tree is kept" "kept" "removed"
git -C "$WCCACHE/cockpit-wt/old" -c user.name=t -c user.email=t@e commit -qam wip
wr_run "$WCCACHE/cockpit-wt/old"; is "a clean tree whose branch has unmerged commits keeps the branch" "old" "$(git -C "$WCREPO" branch --list old | tr -d '*+ ')"
wr_run "/nonexistent/x"; is "a path that is not ours is a no-op, exit 0" "0" "$?"
# claude removes a git worktree ITSELF before the hook runs, and runs the hook
# with cwd inside the deleted directory (measured on 2.1.246 via ExitWorktree),
# so the branch has to be found without a cwd and without the tree.
wc_run '{"name":"feat-z"}' >/dev/null; git -C "$WCREPO" worktree remove "$WCCACHE/cockpit-wt/feat-z" >/dev/null 2>&1
( cd "$WCCACHE/cockpit-wt" && printf '{"worktree_path":"%s"}' "$WCCACHE/cockpit-wt/feat-z" | XDG_CACHE_HOME="$WCCACHE" "$DIR/scripts/worktree-remove" 2>/dev/null )
is "a tree claude already removed still gets its merged branch deleted" "" "$(git -C "$WCREPO" branch --list feat-z)"
[ -e "$WCCACHE/cockpit-wt/feat-z.repo" ] && bad "and the sidecar goes with it" "gone" "still there" || ok "and the sidecar goes with it"
wr_run "$WCREPO"; [ -d "$WCREPO/.git" ] && ok "the main checkout is never removed" || bad "the main checkout is never removed" "kept" "removed"
git -C "$WCREPO" worktree prune >/dev/null 2>&1
# Claude Code can hold the hook's stdin open past the payload; $(cat) waited for EOF forever.
held=$( { printf '{"name":"held"}\n'; sleep 3; } | ( cd "$WCREPO" && XDG_CACHE_HOME="$WCCACHE" timeout 2 "$DIR/scripts/worktree-create" 2>/dev/null ); echo "rc=$?" )
is "worktree-create returns while claude holds stdin open" "$WCCACHE/cockpit-wt/held
rc=0" "$held"
held=$( { printf '{"worktree_path":"/nonexistent/x"}\n'; sleep 3; } | ( XDG_CACHE_HOME="$WCCACHE" timeout 2 "$DIR/scripts/worktree-remove" 2>/dev/null ); echo "rc=$?" )
is "worktree-remove returns while claude holds stdin open" "rc=0" "$held"
is "worktree-remove with HOME unset is a quiet no-op, not a bash error" "0:" \
   "$(printf '{"worktree_path":"/x/cockpit-wt/a"}' | env -u HOME -u XDG_CACHE_HOME "$DIR/scripts/worktree-remove" 2>&1; echo "$?:")"
is "worktree-remove of a tree with no .repo sidecar is quiet, not a bash error" "0:" \
   "$(printf '{"worktree_path":"%s"}' "$WCCACHE/cockpit-wt/nosidecar" | XDG_CACHE_HOME="$WCCACHE" "$DIR/scripts/worktree-remove" 2>&1; echo "$?:")"

echo "== review-gate: hands a queued review over once, flags ◆ on a finished unreviewed diff, never blocks =="
if need_tmux "review-gate"; then :; else
RG="cockpit-rg-$$"; SOCKS="$SOCKS $RG"; RGSTATE=$(mktmpd); RGDIR=$(mktmpd)
(
  cd "$RGDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init
) >/dev/null 2>&1
"$TMUX_REAL" -L "$RG" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RGDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RG" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
RGAP=$("$TMUX_REAL" -L "$RG" list-panes -t t: -F '#{pane_id}')
RGCTL=$("$TMUX_REAL" -L "$RG" split-window -t t -P -F '#{pane_id}' 'exec bash')
until_cap "$RG" "$RGCTL" '*$*'
rg_n() { "$TMUX_REAL" -L "$RG" capture-pane -p -J -S - -t "$RGCTL" | grep -c 'GRC=[0-9]'; }   # -S -: the block runs more hooks than the 30-line pane shows
rg_wait() {                   # <count-before>  until one more finished GRC= marker is on screen (rundeliver's shape)
    local i; for i in $(seq 1 60); do [ "$(rg_n)" -gt "$1" ] && return 0; sleep 0.1; done; return 1
}
rg_run() {                    # <stdin-file>  runs the hook inside the session as the pane's own hook would, RC on the control pane
    local n0; n0=$(rg_n)
    "$TMUX_REAL" -L "$RG" send-keys -t "$RGCTL" \
        "TMUX_PANE=$RGAP XDG_STATE_HOME=$(printf '%q' "$RGSTATE") '$DIR/scripts/review-gate' < $(printf '%q' "$1") > $(printf '%q' "$RGSTATE/out") 2> $(printf '%q' "$RGSTATE/err"); echo GRC=\$?" Enter
    rg_wait "$n0"
}
rg_rc() { "$TMUX_REAL" -L "$RG" capture-pane -p -J -S - -t "$RGCTL" | grep -o 'GRC=[0-9]*' | tail -1; }
RGPAY=$(mktmpf); printf '{"session_id":"sid-123","transcript_path":"%s/tx.jsonl","cwd":"%s","hook_event_name":"Stop","stop_hook_active":false}\n' "$RGSTATE" "$RGDIR" > "$RGPAY"
printf '{"type":"ai-title","aiTitle":"First topic"}\n{"type":"user"}\n{"type":"ai-title","aiTitle":"Fix the parser"}\n{"type":"assistant"}\n' > "$RGSTATE/tx.jsonl"
# Clean tree: nothing to flag; the payload's fields are stashed.
rg_run "$RGPAY"; is "a clean tree exits 0" "GRC=0" "$(rg_rc)"
is "no review is pending on a clean tree" "" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_review)"
is "the newest ai-title becomes the pane's title" "Fix the parser" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_title)"
is "the session id is stashed for prefix i" "sid-123" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_session)"
# Dirty tree: ◆.
printf 'a\nb\n' > "$RGDIR/f"
rg_run "$RGPAY"; is "a finished, unreviewed diff exits 0" "GRC=0" "$(rg_rc)"
is "and flags the pane pending" "pending" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_review)"
case "$("$TMUX_REAL" -L "$RG" show-option -gqv @cockpit_fleet)" in *'◆1'*) ok "the fleet string says ◆1" ;; *) bad "the fleet string says ◆1" "◆1" "$("$TMUX_REAL" -L "$RG" show-option -gqv @cockpit_fleet)" ;; esac
# A queued review: handed over as Stop hook feedback on stdout, exit 0, exactly once.
printf 'Code review from the human\n\n## f:2 (+)\n> b\nDRAIN-MARKER\na "quoted" \\ back\\slash\ttabbed\n\n' > "$RGSTATE/spool.md"
"$TMUX_REAL" -L "$RG" set-option -p -t "$RGAP" @cockpit_review_spool "$RGSTATE/spool.md" >/dev/null 2>&1
rg_run "$RGPAY"; is "a queued review makes the hook exit 0" "GRC=0" "$(rg_rc)"
case "$(cat "$RGSTATE/out")" in
    *'"hookSpecificOutput"'*'"hookEventName":"Stop"'*'"additionalContext":"'*DRAIN-MARKER*)
        ok "and the review rides additionalContext on stdout, where Claude reads it as Stop hook feedback" ;;
    *) bad "and the review rides additionalContext on stdout, where Claude reads it as Stop hook feedback" "DRAIN-MARKER" "$(cat "$RGSTATE/out")" ;;
esac
is "the review's newlines are escaped, not raw, inside the JSON" "1" "$(wc -l < "$RGSTATE/out")"
# The escaper's ORDER is what this pins: a backslash escaped after the quotes
# would double the ones the quote pass just wrote, and a control-char strip run
# before the whitespace passes would swallow the tab instead of escaping it.
rg_esc=""
case "$(cat "$RGSTATE/out")" in *'\"quoted\"'*)  rg_esc="quote" ;;             *) rg_esc="RAW-QUOTE" ;; esac
case "$(cat "$RGSTATE/out")" in *'back\\slash'*) rg_esc="$rg_esc backslash" ;; *) rg_esc="$rg_esc RAW-BACKSLASH" ;; esac
case "$(cat "$RGSTATE/out")" in
    *$'\t'*)           rg_esc="$rg_esc RAW-TAB" ;;
    *'slash\ttabbed'*) rg_esc="$rg_esc tab" ;;
    *)                 rg_esc="$rg_esc LOST-TAB" ;;
esac
is 'and the quote, the backslash and the tab are escaped, not raw' "quote backslash tab" "$rg_esc"
is "and stderr stays empty on a delivery" "" "$(cat "$RGSTATE/err")"
is "the spool option is cleared before speaking" "" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_review_spool)"
is "the spool file is consumed" "absent" "$([ -e "$RGSTATE/spool.md" ] && echo present || echo absent)"
is "the pane reads running again (the agent is about to work)" "running" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @panestate)"
rg_run "$RGPAY"; is "the same Stop fired again delivers nothing (no rewake loop)" "GRC=0" "$(rg_rc)"
is "and stdout is empty the second time" "" "$(cat "$RGSTATE/out")"
is "and stderr is empty the second time" "" "$(cat "$RGSTATE/err")"
# A rewake chain (issue #52): Claude Code's /goal blocks every Stop it judges
# not met, without a cap. Each Stop it causes carries stop_hook_active:true;
# review-gate counts those per pane, ends the turn with continue:false at
# @cockpit_rewake_cap, and a Stop with stop_hook_active:false (a fresh user
# turn) resets the count. `--concede` ends the next turn on the agent's say-so.
RGACT=$(mktmpf); sed 's/"stop_hook_active":false/"stop_hook_active":true/' "$RGPAY" > "$RGACT"
RGKEY=$(gate_key "$RG" "$RGAP")
RGLOOP="$RGSTATE/tmux-cockpit/review/loop-$RGKEY"; RGCF="$RGSTATE/tmux-cockpit/review/concede-$RGKEY"
# --concede runs as the agent's own Bash call, inside the pane: $TMUX is what
# tells it which server's key to write under, and the suite unsets it.
RGTMUX="$("$TMUX_REAL" -L "$RG" display-message -p '#{socket_path}'),0,0"
"$TMUX_REAL" -L "$RG" set -g @cockpit_rewake_cap 3 >/dev/null 2>&1
rg_run "$RGACT"; rg_run "$RGACT"
is "two rewakes under a cap of 3 pass through silently" "" "$(cat "$RGSTATE/out")"
is "and the pane's counter reads 2" "2" "$(cat "$RGLOOP" 2>/dev/null)"
"$TMUX_REAL" -L "$RG" set-option -pu -t "$RGAP" @cockpit_review >/dev/null 2>&1   # cleared: the ◆ below can then only be the capping Stop's
rg_run "$RGACT"; is "the third rewake exits 0" "GRC=0" "$(rg_rc)"
case "$(cat "$RGSTATE/out")" in
    *'"continue":false'*'"stopReason":"'*'3 '*) ok "and ends the turn: continue:false, the count in the reason" ;;
    *) bad "and ends the turn: continue:false, the count in the reason" '"continue":false ... 3' "$(cat "$RGSTATE/out")" ;;
esac
is "and a turn cut short still flags ◆ on the unreviewed diff" "pending" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_review)"
is "the counter is gone once the cap fired" "absent" "$([ -e "$RGLOOP" ] && echo present || echo absent)"
rg_run "$RGACT"; is "one rewake after the cap starts a new count, silently" "" "$(cat "$RGSTATE/out")"
rg_run "$RGPAY"; is "a Stop with stop_hook_active:false resets the count" "absent" "$([ -e "$RGLOOP" ] && echo present || echo absent)"
is "and passes through (no continue:false on a fresh turn)" "" "$(cat "$RGSTATE/out")"
# The concession: the agent names why the goal will not be met; the next Stop
# ends the turn with that line as the reason, whatever stop_hook_active says.
TMUX=$RGTMUX TMUX_PANE=$RGAP XDG_STATE_HOME="$RGSTATE" "$DIR/scripts/review-gate" --concede 'push denied by the classifier; a "human" must run it'
rg_run "$RGPAY"; is "a conceded Stop exits 0" "GRC=0" "$(rg_rc)"
case "$(cat "$RGSTATE/out")" in
    *'"continue":false'*'push denied by the classifier; a \"human\" must run it'*) ok "and ends the turn with the agent's reason, JSON-escaped" ;;
    *) bad "and ends the turn with the agent's reason, JSON-escaped" 'continue:false + the reason' "$(cat "$RGSTATE/out")" ;;
esac
rg_run "$RGPAY"; is "the concession is consumed: the next Stop passes through" "" "$(cat "$RGSTATE/out")"
is "and stderr stayed empty through the cap and the concession" "" "$(cat "$RGSTATE/err")"
# --concede is a foreground call, not a hook: it confirms, or fails out loud.
is "--concede confirms on stdout" "concession recorded; your next turn ends here, with that line as the reason" \
   "$(TMUX=$RGTMUX TMUX_PANE=$RGAP XDG_STATE_HOME="$RGSTATE" "$DIR/scripts/review-gate" --concede 'again')"
TMUX_PANE='' XDG_STATE_HOME="$RGSTATE" "$DIR/scripts/review-gate" --concede 'x' >/dev/null 2>"$RGSTATE/cerr"
is "--concede outside a tmux pane fails, not silently" "1" "$?"
is "and says so on stderr" "nonempty" "$([ -s "$RGSTATE/cerr" ] && echo nonempty || echo empty)"
# Two socket paths that differ only in where the `_` and the `/` sit. `/` -> `_`
# mapped both to one key; `_` -> `__` with `/` -> `_.` is prefix-free, so they
# cannot land on one file. No server needed: --concede writes and exits.
RGENC=$(mktmpd)
for rg_sock in '/tmp/a_/b' '/tmp/a/_b'; do
    TMUX="$rg_sock,0,0" TMUX_PANE=$RGAP XDG_STATE_HOME="$RGENC" "$DIR/scripts/review-gate" --concede "probe $rg_sock" >/dev/null
done
is "two socket paths differing only in _ and / get two keys, not one" "2" \
   "$(ls "$RGENC"/tmux-cockpit/review/ 2>/dev/null | wc -l)"
# No $TMUX, no server to key on: the file would land under a name no Stop hook
# reads, and a concession that silently never fires is the worse failure.
TMUX='' TMUX_PANE=$RGAP XDG_STATE_HOME="$RGSTATE" "$DIR/scripts/review-gate" --concede 'y' >/dev/null 2>"$RGSTATE/cerr"
is "--concede with a pane id but no \$TMUX fails the same way" "1" "$?"
is "and writes no serverless concession" "absent" \
   "$([ -e "$RGSTATE/tmux-cockpit/review/concede--${RGAP#%}" ] && echo present || echo absent)"
# A concession an hour old is a killed agent's, or a pane id reused after a
# server restart: dropped, never fired on whoever holds the id now.
touch -d '-2 hours' "$RGCF"
rg_run "$RGPAY"; is "a stale concession does not end the turn" "" "$(cat "$RGSTATE/out")"
is "and is dropped" "absent" "$([ -e "$RGCF" ] && echo present || echo absent)"
# A concession with no line is the one case that must not go through: the write
# below it expands $2 under set -u, which kills the shell before any handler.
TMUX=$RGTMUX TMUX_PANE=$RGAP XDG_STATE_HOME="$RGSTATE" "$DIR/scripts/review-gate" --concede >/dev/null 2>"$RGSTATE/cerr"
is "--concede with no line fails" "1" "$?"
is "and says so on stderr, not silently" "nonempty" "$([ -s "$RGSTATE/cerr" ] && echo nonempty || echo empty)"
TMUX=$RGTMUX TMUX_PANE=$RGAP XDG_STATE_HOME="$RGSTATE" "$DIR/scripts/review-gate" --concede '' >/dev/null 2>"$RGSTATE/cerr"
is "--concede with an empty line fails the same way" "1" "$?"
is "and says so on stderr too" "nonempty" "$([ -s "$RGSTATE/cerr" ] && echo nonempty || echo empty)"
is "and neither armed a reasonless concession" "absent" "$([ -e "$RGCF" ] && echo present || echo absent)"
# 0 disables: eleven rewakes, one past the default, and nothing fires; then
# with the option unset the twelfth trips the default of 10.
"$TMUX_REAL" -L "$RG" set -g @cockpit_rewake_cap 0 >/dev/null 2>&1
for _ in 1 2 3 4 5 6 7 8 9 10 11; do rg_run "$RGACT"; done
is "@cockpit_rewake_cap 0 disables the cap (11 rewakes, silent)" "" "$(cat "$RGSTATE/out")"
is "and the count still climbs" "11" "$(cat "$RGLOOP" 2>/dev/null)"
"$TMUX_REAL" -L "$RG" set -gu @cockpit_rewake_cap >/dev/null 2>&1
rg_run "$RGACT"
case "$(cat "$RGSTATE/out")" in
    *'"continue":false'*'12 stop-hook rewakes'*'@cockpit_rewake_cap 10'*) ok "the option unset, the default of 10 fires on the next rewake" ;;
    *) bad "the option unset, the default of 10 fires on the next rewake" 'continue:false ... 12 ... cap 10' "$(cat "$RGSTATE/out")" ;;
esac
# An absurd cap is not a disabled one: past intmax `[ -gt ]` errors out, which
# left the cap off and a shell error on the hook's stderr.
printf '9' > "$RGLOOP"
"$TMUX_REAL" -L "$RG" set -g @cockpit_rewake_cap 99999999999999999999 >/dev/null 2>&1
rg_run "$RGACT"
is "an oversized cap leaves no shell error on stderr" "" "$(cat "$RGSTATE/err")"
case "$(cat "$RGSTATE/out")" in
    *'"continue":false'*'@cockpit_rewake_cap 10'*) ok "and falls back to the default of 10, like any other unusable value" ;;
    *) bad "and falls back to the default of 10, like any other unusable value" 'continue:false ... cap 10' "$(cat "$RGSTATE/out")" ;;
esac
# A review queued while the agent worked outlives a capping Stop: ending the
# turn skips the drain, so it is still armed for the Stop after the next prompt.
rg_run "$RGPAY"
printf 'Code review from the human\n\nSPOOL-SURVIVES\n' > "$RGSTATE/spool2.md"
"$TMUX_REAL" -L "$RG" set-option -p -t "$RGAP" @cockpit_review_spool "$RGSTATE/spool2.md" >/dev/null 2>&1
"$TMUX_REAL" -L "$RG" set -g @cockpit_rewake_cap 1 >/dev/null 2>&1
rg_run "$RGACT"
case "$(cat "$RGSTATE/out")" in
    *'"continue":false'*) ok "a capping Stop ends the turn with a review still queued" ;;
    *) bad "a capping Stop ends the turn with a review still queued" '"continue":false' "$(cat "$RGSTATE/out")" ;;
esac
is "and the spool stays armed" "$RGSTATE/spool2.md" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_review_spool)"
is "and its file is untouched" "present" "$([ -e "$RGSTATE/spool2.md" ] && echo present || echo absent)"
"$TMUX_REAL" -L "$RG" set -gu @cockpit_rewake_cap >/dev/null 2>&1
rg_run "$RGPAY"
case "$(cat "$RGSTATE/out")" in
    *SPOOL-SURVIVES*) ok "and the Stop after the human's next prompt hands it over" ;;
    *) bad "and the Stop after the human's next prompt hands it over" "SPOOL-SURVIVES" "$(cat "$RGSTATE/out")" ;;
esac
rg_run "$RGPAY"                 # a fresh turn: resets the count for the stdin cases below
# The two stdin cases, both driven by a background writer on a FIFO. A pipeline
# (`sleep N | review-gate`) would measure bash waiting for the WHOLE pipeline,
# not the hook's read; agent-state's own hang guard above uses this same holder
# shape. %999999 is no pane on any server and TMUX_TMPDIR points at an empty
# dir, so every bare tmux call finds nothing, the hook fails open, and its stdin
# read is the only thing under test -- never the developer's live server.
RGF="$RGSTATE/gate.fifo"; mkfifo "$RGF"
# 1. The payload arrives and the writer then keeps the pipe open, which is what
#    Claude Code actually does. Waiting for EOF here would burn the full
#    deadline on every Stop; the read stops at the fields it wants instead.
( exec 9>"$RGF"; cat "$RGPAY" >&9; sleep 8 ) & rg_holder=$!   # a holder, not a wait
rg_t0=$SECONDS
TMUX_PANE=%999999 TMUX_TMPDIR="$RGSTATE" XDG_STATE_HOME="$RGSTATE" \
    timeout 6 "$DIR/scripts/review-gate" >/dev/null 2>&1 < "$RGF"
rg_grc=$?
kill "$rg_holder" 2>/dev/null; wait "$rg_holder" 2>/dev/null
is "a payload whose writer keeps stdin open is not waited out" "0" "$rg_grc"
[ $(( SECONDS - rg_t0 )) -lt 2 ] && ok "and the read returns as soon as it has both fields ($(( SECONDS - rg_t0 ))s)" || bad "and the read returns as soon as it has both fields" "under the 2s deadline" "$(( SECONDS - rg_t0 ))s"
# 2. Nothing ever arrives: the deadline is the backstop, and it must fire.
( exec 9>"$RGF"; sleep 8 ) & rg_holder=$!                     # a holder, not a wait
rg_t0=$SECONDS
TMUX_PANE=%999999 TMUX_TMPDIR="$RGSTATE" XDG_STATE_HOME="$RGSTATE" \
    timeout 6 "$DIR/scripts/review-gate" >/dev/null 2>&1 < "$RGF"
rg_grc=$?
kill "$rg_holder" 2>/dev/null; wait "$rg_holder" 2>/dev/null
is "a silent stdin does not hang the hook" "0" "$rg_grc"
[ $(( SECONDS - rg_t0 )) -le 4 ] && ok "and it gives up inside its own deadline ($(( SECONDS - rg_t0 ))s)" || bad "and it gives up inside its own deadline" "under 4s" "$(( SECONDS - rg_t0 ))s"
rm -f "$RGF"
# Two servers, one state dir, the same pane id on each (issue #88). Keyed on the
# pane id alone, the second server's first Stop consumed the first server's
# concession and ended the wrong turn with it.
RG2="cockpit-rg2-$$"; SOCKS="$SOCKS $RG2"
"$TMUX_REAL" -L "$RG2" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RGDIR" 'exec -a claude bash' >/dev/null 2>&1
RG2AP=$("$TMUX_REAL" -L "$RG2" list-panes -t t: -F '#{pane_id}')
is "a second server's first pane carries the same id as the first server's" "$RGAP" "$RG2AP"
rg2_run() {                   # <stdin-file>  the same hook, in the OTHER server; run-shell waits
    "$TMUX_REAL" -L "$RG2" run-shell "TMUX_PANE=$RG2AP XDG_STATE_HOME=$(printf '%q' "$RGSTATE") '$DIR/scripts/review-gate' < $(printf '%q' "$1") > $(printf '%q' "$RGSTATE/out2") 2>&1" >/dev/null 2>&1
}
rm -f "$RGSTATE"/tmux-cockpit/review/loop-* "$RGSTATE"/tmux-cockpit/review/concede-*
TMUX=$RGTMUX TMUX_PANE=$RGAP XDG_STATE_HOME="$RGSTATE" "$DIR/scripts/review-gate" --concede 'CROSS-PROBE-88' >/dev/null
rg2_run "$RGPAY"
is "one server's concession does not end the other server's turn" "" "$(cat "$RGSTATE/out2")"
rg_run "$RGPAY"
case "$(cat "$RGSTATE/out")" in
    *'"continue":false'*CROSS-PROBE-88*) ok "and still ends the turn on the server that recorded it" ;;
    *) bad "and still ends the turn on the server that recorded it" 'continue:false + CROSS-PROBE-88' "$(cat "$RGSTATE/out")" ;;
esac
"$TMUX_REAL" -L "$RG"  set -g @cockpit_rewake_cap 3 >/dev/null 2>&1
"$TMUX_REAL" -L "$RG2" set -g @cockpit_rewake_cap 3 >/dev/null 2>&1
rg_run "$RGACT"; rg_run "$RGACT"
is "two rewakes on the first server leave it one under the cap" "2" "$(cat "$RGLOOP" 2>/dev/null)"
rg2_run "$RGACT"
is "and the other server's first rewake is not capped by that count" "" "$(cat "$RGSTATE/out2")"
is "it counts its own" "1" "$(cat "$RGSTATE/tmux-cockpit/review/loop-$(gate_key "$RG2" "$RG2AP")" 2>/dev/null)"
"$TMUX_REAL" -L "$RG" set -gu @cockpit_rewake_cap >/dev/null 2>&1
"$TMUX_REAL" -L "$RG2" kill-server >/dev/null 2>&1
unset -f rg_n rg_wait rg_run rg_rc rg2_run
"$TMUX_REAL" -L "$RG" kill-server >/dev/null 2>&1
fi

echo "== worktree in the branch label =="
wtroot=$(mktmpd)
# Build a throwaway repo instead of borrowing a real one. Pointing this at
# "$HOME/Henosis" made the block skip on every machine without that path, and
# on the one machine that had it the test created a branch and a worktree
# inside live work. Everything here lives under $wtroot and dies with it.
wtrepo="$wtroot/repo"
if git init -q "$wtrepo" >/dev/null 2>&1 &&
   git -C "$wtrepo" -c user.email=test@example.invalid -c user.name=cockpit \
       commit -q --allow-empty -m init >/dev/null 2>&1 &&
   git -C "$wtrepo" worktree add -q "$wtroot/wt" -b cockpit-test-wt >/dev/null 2>&1; then
    b=$("$DIR/scripts/tmux-where" branch 1 "$wtrepo" claude 2>/dev/null | plain)
    case "$b" in *"⑂"*) bad "normal repo shows no worktree mark" "no mark" "$b" ;; *) ok "normal repo shows no worktree mark" ;; esac
    w=$("$DIR/scripts/tmux-where" branch 1 "$wtroot/wt" claude 2>/dev/null | plain)
    case "$w" in *"⑂wt"*) ok "linked worktree is named" ;; *) bad "linked worktree is named" "⑂wt" "$w" ;; esac
    case "$w" in *cockpit-test-wt*) ok "and its branch still shows" ;; *) bad "and its branch still shows" "cockpit-test-wt" "$w" ;; esac
else
    skip_ "no worktree mark on a normal repo"; skip_ "linked worktree is named"; skip_ "worktree branch shows"
fi
is "non-repo path stays empty" "" "$("$DIR/scripts/tmux-where" branch 1 /tmp claude 2>/dev/null | plain)"
rm -rf "$wtroot"

echo "== ssh destination parsing =="
# Drive ssh_target with a real argv ARRAY. The previous tests faked a process
# with `exec -a "ssh -p 2222 host"`, which puts the WHOLE string in argv[0] --
# not what a real ssh looks like. They therefore exercised a code path that no
# longer exists, and would not have caught the -o-value-with-spaces bug.
sed -n '/^ssh_target() {/,/^}/p' "$DIR/scripts/tmux-where" > "$STATE/st.sh"
. "$STATE/st.sh"
# shellcheck disable=SC2034  # ARGV is read by the ssh_target just sourced
sshdest() { ARGV=("$@"); ssh_target; }
is "plain"                 "deploy@host" "$(sshdest ssh deploy@host)"
is "-p with a value"       "deploy@host" "$(sshdest ssh -p 2222 deploy@host)"
is "bundle -nNp"           "realhost"    "$(sshdest ssh -nNp 2222 realhost)"
is "bundle -tti"           "box01"       "$(sshdest ssh -tti /k box01)"
is "attached -p2222"       "attached"    "$(sshdest ssh -p2222 attached)"
is "-o Opt=val"            "prod"        "$(sshdest ssh -o BatchMode=yes prod)"
is "-B bind interface"     "host6"       "$(sshdest ssh -B eth0 host6)"
is "-J jump host"          "final"       "$(sshdest ssh -J j1,j2 final)"
is "-W forward"            "wh"          "$(sshdest ssh -W h:22 wh)"
is "-R remote forward"     "rhost"       "$(sshdest ssh -R 80:localhost:80 rhost)"
is "remote command after"  "prod-box"    "$(sshdest ssh prod-box uptime)"
is "end of options"        "weird"       "$(sshdest ssh -- weird)"
is "-vvv"                  "web01"       "$(sshdest ssh -vvv web01)"
# an -o value containing spaces is ONE argv element; flattening it split the
# value into tokens and the jump host was read as the destination
is "-o value with spaces"  "host12"      "$(sshdest ssh -o 'ProxyCommand=ssh -W %h:%p jump' host12)"

echo "== tmux-where: local rendering =="
is "local path is ~-shortened" " ~"      "$("$DIR/scripts/tmux-where" path 1 "$HOME" bash | plain)"
# A dead pid with cmd=ssh used to print the LOCAL path -- the confidently-wrong
# answer this script exists to prevent, since tmux has already said the pane is
# in ssh. The walk failing says nothing about the pane: the pid may have exited,
# or there may be no /proc at all (macOS, BSD). Fail closed instead.
is "an ssh pane whose walk fails says ssh, not a local path" " 󰢹 ssh" \
   "$("$DIR/scripts/tmux-where" path 999999 /tmp/x ssh 2>&1 | plain)"
is "and reports no branch for it"      "" \
   "$("$DIR/scripts/tmux-where" branch 999999 "$DIR" ssh 2>&1 | plain)"
# The ordinary case is untouched: a non-ssh command never reaches the walk.
is "a normal pane still shows its path" " /tmp/x" \
   "$("$DIR/scripts/tmux-where" path 999999 /tmp/x bash 2>&1 | plain)"
is "HOME unset does not crash" " /tmp/x" "$(env -u HOME "$DIR/scripts/tmux-where" path 1 /tmp/x bash 2>&1 | plain)"

echo "== tmux-where branch --set: the pushed branch, compare-then-set =="
# The bar reads @cockpit_branch, a pane option the hooks push, so a redraw runs no
# shell. Every set-option redraws, so an unchanged branch must write nothing.
if need_tmux "tmux-where --set"; then :; else
WS="cockpit-wset-$$"; SOCKS="$SOCKS $WS"; WSD=$(mktmpd)
mkdir -p "$WSD/repo/.git" "$WSD/plain"
printf 'ref: refs/heads/feat/x\n' > "$WSD/repo/.git/HEAD"
"$TMUX_REAL" -L "$WS" -f /dev/null new-session -d -s s -c "$WSD/repo" >/dev/null 2>&1
"$TMUX_REAL" -L "$WS" split-window -t s: -c "$WSD/plain" >/dev/null 2>&1
WSR=$("$TMUX_REAL" -L "$WS" display-message -p -t s:0.0 '#{pane_id}')
WSP=$("$TMUX_REAL" -L "$WS" display-message -p -t s:0.1 '#{pane_id}')
wset() { TMUX="$SOCKDIR/$WS,0,0" "$DIR/scripts/tmux-where" branch --set "$1" >/dev/null 2>&1; }
wsb() { "$TMUX_REAL" -L "$WS" show-option -pqv -t "$1" @cockpit_branch; }
wset "$WSR"; wset "$WSP"
is "a repo pane gets its branch" "feat/x" "$(wsb "$WSR")"
is "a pane outside a repo gets none" "" "$(wsb "$WSP")"
WSLOG="$WSD/sets"; : > "$WSLOG"
"$TMUX_REAL" -L "$WS" set-hook -g after-set-option \
    "run-shell 'echo \"#{hook_arguments}\" >> $WSLOG'" >/dev/null 2>&1
wset "$WSR"; wset "$WSP"
is "an unchanged branch writes nothing" "0" "$(wc -l < "$WSLOG" | tr -d ' ')"
printf 'ref: refs/heads/main\n' > "$WSD/repo/.git/HEAD"
wset "$WSR"
is "a switched branch lands" "main" "$(wsb "$WSR")"
"$TMUX_REAL" -L "$WS" kill-server >/dev/null 2>&1
fi

echo "== tmux-where: the pane_id form opens no command client =="
# The #() job runs on every status tick, and each display-message it made
# landed in show-messages: the 1000-entry log spanned minutes and no click was
# diagnosable after the fact (issue #24). It now runs for ssh panes only.
if [ -d "/proc/$$" ]; then
    # The serial runner shares one server: past 1000 entries each show-messages
    # evicts an old line and could shift the count below by itself.
    tmux set -g message-limit 100000
    # argv[0] is what tmux reports as pane_current_command, so a process
    # exec'd under the name ssh must still take the ssh walk (which then reads
    # its first bare argument, 30, as the destination).
    tmux new-window -d -n wherepane 'exec -a ssh sleep 30'
    WP=$(tmux list-panes -a -F '#{pane_id} #{window_name}' | awk '$2=="wherepane"{print $1}')
    WPID=$(tmux display-message -p -t "$WP" '#{pane_pid}')
    for _ in $(seq 20); do              # sh -c may not have exec'd yet
        IFS= read -r -d '' a0 2>/dev/null < "/proc/$WPID/cmdline"; [ "$a0" = ssh ] && break; sleep 0.1
    done
    before=$(tmux show-messages | grep -c 'display-message -p -t')
    is "an ssh pane is recognised from /proc" " 󰢹 30" "$("$DIR/scripts/tmux-where" path "$WPID" "$WP" | plain)"
    is "and has no branch" "" "$("$DIR/scripts/tmux-where" branch "$WPID" "$WP" | plain)"
    tmux kill-window -t wherepane
    # pid_max+1 cannot be alive; 999999 can, and would read a stranger's /proc.
    DEADPID=$(( $(cat /proc/sys/kernel/pid_max) + 1 ))
    is "a dead pid fails closed to ssh" " 󰢹 ssh" "$("$DIR/scripts/tmux-where" path "$DEADPID" "$PANE" | plain)"
    after=$(tmux show-messages | grep -c 'display-message -p -t')
    is "none of it reached tmux" "$before" "$after"
else
    skip_ "tmux-where /proc read (no /proc)"
fi

echo "== status path and branch render the same for every kind of pane =="
# Only an ssh pane runs tmux-where now; every other pane renders the path from
# a native format. These are the strings the all-#() version drew.
if need_tmux "status path/branch rendering"; then :; else
RW=$(mktmpd); mkdir -p "$HOME/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddddddddd" "$RW/plain" "$RW/gone"
(
  cd "$RW" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  for r in repo det; do
      git init -q -b main "$r"; c=$(git -C "$r" commit-tree "$(git -C "$r" write-tree)" -m init)
      git -C "$r" update-ref refs/heads/main "$c"
  done
  git -C det checkout -q --detach main
) >/dev/null 2>&1
RWO="cockpit-where-outer-$$"; RWI="cockpit-where-inner-$$"; SOCKS="$SOCKS $RWO $RWI"
tmux -L "$RWI" -f /dev/null new-session -d -s s -n home -x 200 -y 24 -c "$HOME" 'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$RWI" new-window -d -n deep -c "$HOME/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddddddddd" 'exec -a claude sleep 300'
tmux -L "$RWI" new-window -d -n plain -c "$RW/plain" 'exec -a claude sleep 300'
tmux -L "$RWI" new-window -d -n repo -c "$RW/repo" 'exec -a claude sleep 300'
tmux -L "$RWI" new-window -d -n det -c "$RW/det" 'exec -a claude sleep 300'
tmux -L "$RWI" new-window -d -n ssh -c "$RW/repo" 'exec -a ssh sleep 300'
tmux -L "$RWI" new-window -d -n gone -c "$RW/gone" 'exec -a claude sleep 300'
rmdir "$RW/gone"
tmux -L "$RWI" set -g status-right-length 200 >/dev/null 2>&1
tmux -L "$RWI" set -g status-interval 1 >/dev/null 2>&1
# A bar of one's own: the plugin leaves status-right alone and still publishes
# the two options and pushes the branch from its hooks.
tmux -L "$RWI" set -g @cockpit_statusbar off >/dev/null 2>&1
tmux -L "$RWI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
tmux -L "$RWI" set -g status-right "<#{=/30/…:#{E:@cockpit_where_path}}|#{E:@cockpit_where_branch}>" >/dev/null 2>&1
tmux -L "$RWO" -f /dev/null new-session -d -s o -x 200 -y 24 \
    "$TMUX_REAL -L $RWI attach -t s" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$RWI" list-clients -F '#{client_name}'
sha=$(git -C "$RW/det" rev-parse --short HEAD)
for c in "home|<~|>" \
         "deep|<~/aaaaaaaaaa/bbbbbbbbbb/cccccc…|>" \
         "plain|<$RW/plain|>" \
         "repo|<$RW/repo|main>" \
         "det|<$RW/det|@$sha>" \
         "ssh|< 󰢹 300|>" \
         "gone|<$(g="$RW/gone (deleted)"; printf '%s' "${g:0:30}…")|>"; do
    w=${c%%|*}; want=${c#*|}
    tmux -L "$RWI" select-window -t "s:$w" >/dev/null 2>&1
    if until_cap "$RWO" o "*$want*"; then ok "$w pane: $want"
    else bad "$w pane: $want" "$want" "$("$TMUX_REAL" -L "$RWO" capture-pane -p -t o | tail -1)"; fi
done
tmux -L "$RWO" kill-server >/dev/null 2>&1
tmux -L "$RWI" kill-server >/dev/null 2>&1
rm -rf "$HOME/aaaaaaaaaa"
fi


echo "== tmux-open-path: which token on the line is a file =="
# A scratch tree with the shapes real output prints. --dry-run resolves without
# touching tmux and without opening anything.
OP=$(mktmpd); mkdir -p "$OP/src" "$OP/bin"
: > "$OP/src/foo-bar.ts"; : > "$OP/README.md"; : > "$OP/bin/tm"
# A bare-word name at the top level: only such a token can even reach the
# call-wrapper rule, so "tm(12,3)" is what proves tsc's shape is not one.
: > "$OP/tm"
op() { "$DIR/scripts/tmux-open-path" --dry-run "$OP" "$1"; }

is "bare relative path"      "$OP/README.md::"        "$(op 'see README.md ok')"
is "file:line:col"           "$OP/src/foo-bar.ts:31:5" "$(op 'src/foo-bar.ts:31:5 failed')"
is "file:line"               "$OP/src/foo-bar.ts:31:"  "$(op 'at src/foo-bar.ts:31')"
# The word under "src/foo-bar.ts:31" is "foo": word-separators include / . : and
# -, which is why this reads the whole line instead of #{mouse_word}.
is "hyphen does not split it" "$OP/src/foo-bar.ts::"   "$(op 'src/foo-bar.ts')"
# tsc and MSVC print file(12,3). Stripping trailing punctuation ate the ")" and
# the location became unparseable text.
is "tsc file(12,3)"          "$OP/bin/tm:12:3"        "$(op 'bin/tm(12,3): error TS1005')"
# [:punct:] contains / and ~, so a leading-punctuation strip using it removed the
# first character of every absolute and ~ path.
is "absolute path survives"  "$OP/README.md:5:2"      "$(op "absolute $OP/README.md:5:2 ok")"
is "quoted path"             "$OP/README.md::"        "$(op '"README.md":')"
is "bracketed path"          "$OP/bin/tm::"           "$(op '(bin/tm)')"
is "git diff a/ prefix"      "$OP/README.md::"        "$(op '--- a/README.md')"
is "./ is normalised away"   "$OP/bin/tm:1:1"         "$(op './bin/tm:1:1')"
is "a word that is no file"  ""                       "$(op 'npm run build finished')"
is "several files, in order" "$OP/README.md::
$OP/bin/tm:5:"                                        "$(op 'changed README.md and bin/tm:5')"
# Existence is the filter, so a plausible-looking miss is dropped, not opened.
is "missing file is dropped" "$OP/src/foo-bar.ts::"   "$(op 'src/nope.ts and src/foo-bar.ts')"

# Claude Code prints its tool calls as Write(path) / Read(path) / Edit(path).
# The path inside is real; the token as a whole resolves to nothing, so the
# line read as "no file on that line" and no picker ever opened -- on exactly
# the output this plugin exists to click.
is "Write(path)"             "$OP/README.md::"        "$(op 'Write(README.md)')"
is "Read(path:12)"           "$OP/src/foo-bar.ts:12:" "$(op 'Read(src/foo-bar.ts:12)')"
is "Edit(path:12:5)"         "$OP/src/foo-bar.ts:12:5" "$(op 'Edit(src/foo-bar.ts:12:5)')"
# The bullet Claude Code prefixes the line with is its own token; the wrapper
# still has to be unwrapped behind it.
is "bullet then Write(path)" "$OP/README.md::"        "$(op '⏺ Write(README.md) with 2 additions')"
# Existence stays the filter inside the wrapper too.
is "Write(missing) is dropped" ""                     "$(op 'Write(docs/nope.md)')"
# A wrapper is unwrapped, not stripped: file(12,3) is a LOCATION, and reading
# it as a wrapper would look up "12,3" as a filename and lose the line number.
is "file(12,3) is not a wrapper" "$OP/tm:12:3"        "$(op 'tm(12,3): error TS1005')"
# Wrapped and bracketed at once -- the inner token gets the same strip.
is "nested (Write(path))"    "$OP/README.md::"        "$(op '(Write(README.md))')"
rm -rf "$OP"

# tests: scripts/tmux-branch
echo "== tm: a session created from the picker starts in the pane's directory (#41) =="
# tm runs in a popup, whose cwd is the CURRENT session's start directory, and
# new-session without -c takes the command client's cwd: a session made for
# project B while sitting in A started in A and wore A's branch.
if need_tmux "tm start dir"; then :; else
TC="cockpit-tmcwd-$$"; SOCKS="$SOCKS $TC"; TCD=$(mktmpd); mkdir -p "$TCD/a" "$TCD/b"
"$TMUX_REAL" -L "$TC" -f /dev/null new-session -d -s here -c "$TCD/a" 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TC" new-window -a -t here -c "$TCD/b" 'exec sleep 300' >/dev/null 2>&1
tcsid=$("$TMUX_REAL" -L "$TC" display-message -p -t here '#{session_id}')
# The popup shape: $TMUX names the server and session, $TMUX_PANE is unset,
# cwd is the session's start dir. attach-session fails without a tty; the
# session is already made by then.
( cd "$TCD/a" && env -u TMUX_PANE TMUX="/tmp/tmux-$(id -u)/$TC,0,${tcsid#\$}" "$DIR/scripts/tm" made </dev/null >/dev/null 2>&1 )
is "the new session starts where the pane was, not where the old session began" "$TCD/b" \
   "$("$TMUX_REAL" -L "$TC" display-message -p -t made: '#{session_path}' 2>/dev/null)"
# With a real client on "here", popup-shaped tm must switch THAT client, not
# attach a second one inside the popup (#45): in_client() wanted $TMUX_PANE,
# which a popup never has.
TCO="cockpit-tmcwdo-$$"; SOCKS="$SOCKS $TCO"
"$TMUX_REAL" -L "$TCO" -f /dev/null new-session -d -s o -x 80 -y 24 "TERM=xterm-256color exec $TMUX_REAL -L $TC attach -t here" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$TC" list-clients -F '#{client_name}'
( cd "$TCD/a" && env -u TMUX_PANE TMUX="/tmp/tmux-$(id -u)/$TC,0,${tcsid#\$}" timeout 3 "$DIR/scripts/tm" made </dev/null >/dev/null 2>&1 )
is "tm from a popup moves the real client to the session" "made" \
   "$("$TMUX_REAL" -L "$TC" list-clients -F '#{session_name}' 2>/dev/null | head -1)"

# An attach that does not displace the old client leaves it attached forever and
# nothing reaps it: on a Coder workspace a closed tab held one for 41 hours (#145).
# Asserted through a fake tmux on PATH, the same way the pickers' fzf argv is:
# two live clients on one session cannot be built reliably inside the suite.
TMF=$(mktmpd)
cat > "$TMF/tmux" <<'FAKETMUX'
#!/bin/sh
printf '%s\n' "$*" >> "$TMF_LOG"
case "$1" in
  list-sessions)   echo '$4 target' ;;   # the session already exists, so go() skips creation
  display-message) echo 1; exit 0 ;;
  *)               exit 0 ;;
esac
FAKETMUX
chmod +x "$TMF/tmux"
TMF_LOG="$TMF/log"; export TMF_LOG
env -u TMUX -u TMUX_PANE TMF_LOG="$TMF_LOG" PATH="$TMF:$PATH" "$DIR/scripts/tm" target >/dev/null 2>&1
tmfline=$(grep '^attach-session' "$TMF_LOG" 2>/dev/null | head -1)
is "tm attaches with -d so the newest client takes the session" "attach-session -d -t \$4" "$tmfline"
rm -rf "$TMF"
"$TMUX_REAL" -L "$TCO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TC" kill-server >/dev/null 2>&1
fi

echo "== tmux-branch: local and remote branches =="
# Plumbing, not `git commit`: no user config, no hooks, no signing prompt.
BR=$(mktmpd); RM=$(mktmpd)
(
  cd "$BR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; : > a; git add a
  c=$(git commit-tree "$(git write-tree)" -m init); git update-ref refs/heads/main "$c"
  git symbolic-ref HEAD refs/heads/main; git reset -q --hard
  git branch feature/one; git branch remote-only
  git init -q --bare "$RM"; git remote add origin "$RM"
  git push -q origin main feature/one remote-only; git branch -q -D remote-only
  git fetch -q origin
) >/dev/null 2>&1
brs() { git -C "$BR" for-each-ref --format='%(refname:short)' "$@"; }

is "local branch with a slash exists" "feature/one" "$(brs refs/heads | grep feature/one)"
# Routing on the slash sent LOCAL "feature/one" down the remote path and left a
# stray "one" tracking branch behind.
git -C "$BR" switch -q feature/one 2>/dev/null
is "switching to it makes no stray" "" "$(brs refs/heads | grep -x one)"
is "and it is checked out"          "feature/one" "$(git -C "$BR" branch --show-current)"
git -C "$BR" switch -q main 2>/dev/null
# A remote-only branch has to become a local tracking branch.
git -C "$BR" switch -q --track origin/remote-only 2>/dev/null
is "remote-only branch tracks"      "origin/remote-only" \
   "$(git -C "$BR" rev-parse --abbrev-ref '@{upstream}' 2>/dev/null)"
# A branch held by another worktree cannot be checked out; git says which one.
git -C "$BR" switch -q main 2>/dev/null
WT=$(mktmpd); rm -rf "$WT"
git -C "$BR" worktree add -q "$WT" feature/one 2>/dev/null
# git's wording differs across versions; the path does not.
case "$(git -C "$BR" switch feature/one 2>&1)" in
    *"$WT"*) ok "worktree conflict names the worktree" ;;
    *) bad "worktree conflict names the worktree" "a message naming $WT" "$(git -C "$BR" switch feature/one 2>&1)" ;;
esac
git -C "$BR" worktree remove --force "$WT" >/dev/null 2>&1
rm -rf "$BR" "$RM"


echo "== @cockpit_agents drives the formats =="
# Proving the DEFAULT renders correctly is not enough: that passes just as well
# if the option is ignored and the old literal regex is still baked into every
# format. Change the option and the colour has to follow.
if need_tmux "@cockpit_agents"; then :; else
SOCK3="cockpit-agents-$$"; SOCKS="$SOCKS $SOCK3"
tmux -L "$SOCK3" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$SOCK3" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
ap=$(tmux -L "$SOCK3" list-panes -a -F '#{pane_id}' 2>/dev/null | head -1)
abs() { tmux -L "$SOCK3" display-message -p -t "$ap" '#{E:#{pane-border-style}}' 2>/dev/null; }
tmux -L "$SOCK3" set -p -t "$ap" @panestate needs-input >/dev/null 2>&1

is "the plugin defaults it rather than leaving it empty" \
   '^(claude|codex|aider|cursor|opencode|gemini|crush)$' \
   "$(tmux -L "$SOCK3" show-option -gqv @cockpit_agents)"
is "and derives the glob list the formats read" \
   '|claude|codex|aider|cursor|opencode|gemini|crush|' \
   "$(tmux -L "$SOCK3" show-option -gqv @cockpit_agents_glob)"
is "with the default, a claude pane is believed" "fg=themered" "$(abs)"
tmux -L "$SOCK3" set -g @cockpit_agents_glob '|vim|emacs|' >/dev/null 2>&1
is "narrowing it disbelieves the pane"          "fg=themelightgrey" "$(abs)"
tmux -L "$SOCK3" set -g @cockpit_agents_glob '|claude|vim|' >/dev/null 2>&1
is "widening it believes the pane again"        "fg=themered" "$(abs)"
tmux -L "$SOCK3" kill-server >/dev/null 2>&1
fi

echo "== tmux-open-path: the clicked line is never glob-expanded =="
# "for tok in $line" splits with the shell's own rules, which include PATHNAME
# EXPANSION. A line that merely mentioned a wildcard -- "rm *", a compiler note
# with [ab]* in it -- was expanded against the pane's cwd, so alt-clicking it
# opened files nobody named. Existence was supposed to be the filter; globbing
# made the shell invent the candidates before the filter ever ran.
GB=$(mktmpd); : > "$GB/real.txt"; : > "$GB/other.txt"; : > "$GB/a"
# Globbing expands against the PROCESS cwd, while candidates resolve against
# the pane dir, so the bug only shows when they are the same directory -- the
# common run-shell case, where the server was started inside the tree you work
# in. cd into it or the test silently proves nothing.
gb() { ( cd "$GB" && "$DIR/scripts/tmux-open-path" --dry-run "$GB" "$1" ); }
is "a bare * expands to nothing"      "" "$(gb 'error in * somewhere')"
is "a bare ? expands to nothing"      "" "$(gb 'huh ? really')"
is "a glob class expands to nothing"  "" "$(gb 'see [ro]* please')"
# ...while a real file on the same line is still found.
is "a real file beside a glob opens"  "$GB/real.txt::" "$(gb 'rm * then real.txt')"
rm -rf "$GB"

echo "== tmux-open-path: a pathologically long clicked line is bounded, not just slow =="
# The per-token loop is linear (0.31ms/token measured), so a normal clicked
# line -- a handful of tokens -- costs nothing. A 112KB line of 4000
# path-shaped tokens (real output: a wrapped stack trace, an agent dumping a
# wordlist) measured 1.59s in this loop alone, all before the picker even
# opens. Capped at 500 tokens, same idea as the preview's render-window guard.
BIGLINE=$(awk 'BEGIN{s="";for(i=1;i<=4000;i++) s = s "nope" i "xxxxxxxxxxxxxxxx ";print s}')
cpu_under "a 4000-token clicked line is bounded, not just slow" 1.0 \
    "$DIR/scripts/tmux-open-path" --dry-run "$DIR" "$BIGLINE"

# group: etmux
echo "== tmux-open-path: the editor command survives odd filenames =="
# open_one built a shell string by wrapping the path in single quotes. A path
# containing a single quote closed that quote early and the rest of the name was
# read as shell syntax, so the editor opened the wrong file -- or ran a word of
# the filename as a command.
EQ=$(mktmpd); ETMUX=$(mktmpd)
: > "$EQ/it's.txt"
cat > "$ETMUX/tmux" <<'STUB'
#!/usr/bin/env bash
# Just enough tmux for tmux-open-path: answer its queries from the environment
# and record what it tried to run. No server, so this runs anywhere.
case "$1" in
  show-options)
      case " $* " in
          *@cockpit_click_line*)  printf '%s\n' "${CLICK_LINE:-}" ;;
          *@cockpit_open_single*) printf '%s\n' "${OPEN_SINGLE:-}" ;;
          *@cockpit_editor*)      printf '%s\n' "${OPEN_EDITOR:-}" ;;
          *@open-editor*) printf '%s\n' "${OPEN_EDITOR2:-}" ;;
      esac ;;
  display-message)
      case " $* " in
          *pane_current_path*) printf '%s\n' "${CLICK_DIR:-}" ;;
          *window_id*)         printf '@0\n' ;;
          *)                   printf 'MSG %s\n' "$*" >> "$TMUX_LOG" ;;
      esac ;;
  new-window)    printf 'NEWWIN %s\n' "${!#}" >> "$TMUX_LOG" ;;
  display-popup) printf 'POPUP %s\n'  "${!#}" >> "$TMUX_LOG" ;;
  run-shell)     printf 'RUNSHELL %s\n' "$*" >> "$TMUX_LOG" ;;
esac
exit 0
STUB
chmod +x "$ETMUX/tmux"
# env bash shebang: without bash on the stub PATH nothing runs at all and the
# assertions below would pass for the wrong reason.
ln -s "$(command -v bash)" "$ETMUX/bash"
LOG="$ETMUX/log"

: > "$LOG"
env PATH="$ETMUX" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt:12" \
    OPEN_EDITOR=nvim "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
cmd=$(sed -n 's/^NEWWIN //p' "$LOG")
if eval "set -- $cmd" 2>/dev/null; then
    is "a quote in the name reaches the editor intact" "$EQ/it's.txt" "${*: -1}"
    is "and the line number becomes +12"               "+12"          "${2:-}"
else
    bad "a quote in the name reaches the editor intact" "parseable command" "$cmd"
    bad "and the line number becomes +12" "+12" "$cmd"
fi

# With no @open-editor and no code(1) on PATH, fall back to $VISUAL/$EDITOR
# rather than reporting "opened" for a command that was never found.
# HOME is the fixture dir, not the real one. The VS Code shim now sits between
# code(1) and $EDITOR and is looked up under $HOME, so a real home makes what
# these two measure depend on whether the developer happens to have Remote-SSH
# installed. (Today $ETMUX carries no ls/grep/head, so the probe cannot run at
# all and the answer is the same either way -- that is an accident of the stub
# PATH, not something to rest an assertion on.)
: > "$LOG"
env -i PATH="$ETMUX" HOME="$EQ" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt" \
    EDITOR=vi bash "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
case "$(cat "$LOG")" in
    *NEWWIN*vi*) ok "no code(1): falls back to \$EDITOR" ;;
    *) bad "no code(1): falls back to \$EDITOR" "a new window running vi" "$(cat "$LOG")" ;;
esac

# ...and with no editor at all it must say so, not claim success.
: > "$LOG"
env -i PATH="$ETMUX" HOME="$EQ" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
case "$(cat "$LOG")" in
    *"no editor"*) ok "no editor at all: says so instead of claiming success" ;;
    *) bad "no editor at all: says so instead of claiming success" "a 'no editor' message" "$(cat "$LOG")" ;;
esac

# VS Code is launched BY THE SERVER, never by this script. The picker runs
# inside a popup, and a popup kills its process group the instant it closes --
# which is immediately, the script exits right after fzf returns. A plain
# `code ... &` died before it could hand the file to the running window, and
# nothing opened. Proven both ways against a real popup with a real client:
# the & child never survives, the run-shell -b one always does.
printf '#!/bin/sh\nexit 0\n' > "$ETMUX/code"; chmod +x "$ETMUX/code"
: > "$LOG"
env -i PATH="$ETMUX" HOME="$HOME" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt:12" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
case "$(cat "$LOG")" in
    *"RUNSHELL run-shell -b"*"--goto"*) ok "code is launched by the server, not the dying popup" ;;
    *) bad "code is launched by the server, not the dying popup" \
         "a run-shell -b carrying code --goto" "$(cat "$LOG")" ;;
esac
# A folder must open as a workspace, not a file spec: --goto on a directory
# parses it as file:line and opens nothing.
: > "$LOG"
env -i PATH="$ETMUX" HOME="$HOME" TMUX_LOG="$LOG" CLICK_DIR="$DIR" CLICK_LINE="scripts" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
case "$(cat "$LOG")" in
    *"--goto"*) bad "a folder opens whole, not as a --goto spec" "code <dir>" "$(cat "$LOG")" ;;
    *"RUNSHELL run-shell -b"*scripts*) ok "a folder opens whole, not as a --goto spec" ;;
    *) bad "a folder opens whole, not as a --goto spec" "a run-shell carrying the dir" "$(cat "$LOG")" ;;
esac
rm -f "$ETMUX/code"
# One candidate must still reach the picker. This is the behaviour the whole
# gesture exists for -- see what the line resolved to, then decide -- and the
# easiest one to regress back into "just open it".
: > "$LOG"
env PATH="$ETMUX:$PATH" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt:12" \
    "$DIR/scripts/tmux-open-path" %1 >/dev/null 2>&1
case "$(cat "$LOG")" in
    *POPUP*--pick*) ok "a single candidate still opens the picker" ;;
    *) bad "a single candidate still opens the picker" "a display-popup ... --pick" "$(cat "$LOG")" ;;
esac
# ...and nothing is opened behind the popup's back.
case "$(cat "$LOG")" in
    *NEWWIN*) bad "and nothing opens behind the picker" "no new-window" "$(cat "$LOG")" ;;
    *) ok "and nothing opens behind the picker" ;;
esac
# --preview is a documented subcommand, so it gets whatever a user types. A
# non-numeric line reached the arithmetic and `set -u` killed the preview.
PV=$(mktmpd); printf 'one\ntwo\nthree\n' > "$PV/f.txt"
for bad in abc -5 1.5 '' 2; do
    out=$("$DIR/scripts/tmux-open-path" --preview "$PV/f.txt" "$bad" 2>&1)
    case "$out" in
        *"unbound variable"*|*"syntax error"*)
            bad "preview survives line=[$bad]" "a rendered file" "$out" ;;
        *one*) ok "preview survives line=[$bad]" ;;
        *) bad "preview survives line=[$bad]" "the file contents" "$out" ;;
    esac
done
is "preview of a missing file says so" "gone: $PV/nope" "$("$DIR/scripts/tmux-open-path" --preview "$PV/nope")"
# The target line carries a real marker column: bat's own highlight band
# measured 1.4:1 against dark themes -- found by counting rows, not looking.
mk=$("$DIR/scripts/tmux-open-path" --preview "$PV/f.txt" 2 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g')
if command -v batcat >/dev/null 2>&1 || command -v bat >/dev/null 2>&1; then
case "$(printf '%s' "$mk" | sed -n 2p)" in
    "▸"*) ok "the target line starts with the marker" ;;
    *) bad "the target line starts with the marker" "a leading ▸" "$(printf '%s' "$mk" | sed -n 2p)" ;;
esac
else skip_ "the target line starts with the marker (no bat)"; fi
# Debian's batcat is often an older bat than one installed by hand: bat wins.
PVB=$(mktmpd)
for b in bat batcat; do printf '#!/bin/sh\necho %s\n' "$b" > "$PVB/$b"; chmod +x "$PVB/$b"; done
is "the preview runs bat over batcat when both exist" " bat" \
   "$(PATH="$PVB:$PATH" "$DIR/scripts/tmux-open-path" --preview "$PV/f.txt" 2 2>/dev/null)"
case "$(printf '%s' "$mk" | sed -n 1p)" in
    " "*) ok "and its neighbours keep alignment" ;;
    *) bad "and its neighbours keep alignment" "a leading space" "$(printf '%s' "$mk" | sed -n 1p)" ;;
esac
# An empty file has no lines, so the binary test matched nothing and accused it
# of being a blob. A file of only blank lines has lines but no characters, which
# a "." pattern rejects for the same reason.
: > "$PV/empty.txt"; printf '\n\n\n' > "$PV/blanks.txt"
# A minified bundle is one line of megabytes. bat --wrap=never declines to WRAP
# but does not truncate, so the whole line was piped into the preview pane on
# every cursor move -- 3.2MB for one 3MB line, per keystroke.
awk 'BEGIN { s=""; for (i=0;i<20000;i++) s = s "var x=1;"; print s }' > "$PV/min.js"
n=$("$DIR/scripts/tmux-open-path" --preview "$PV/min.js" 2>/dev/null | wc -c)
if [ "$n" -lt 20000 ]; then ok "a minified one-liner is capped, not dumped"
else bad "a minified one-liner is capped, not dumped" "<20000 bytes" "$n bytes"; fi
# ...and a normal file is unaffected: still rendered, still in full.
is "a normal file still renders every line" "3" \
   "$(printf 'a\nb\nc\n' > "$PV/three.txt"; "$DIR/scripts/tmux-open-path" --preview "$PV/three.txt" | grep -c .)"

is "an empty file is not called binary" "empty file" \
   "$("$DIR/scripts/tmux-open-path" --preview "$PV/empty.txt")"
case "$("$DIR/scripts/tmux-open-path" --preview "$PV/blanks.txt")" in
    *binary*) bad "blank lines are not called binary" "the blank lines" "binary file" ;;
    *) ok "blank lines are not called binary" ;;
esac
# Explicit NUL bytes, not /dev/urandom: 200 random bytes are sometimes valid
# text as far as grep is concerned, which made this assertion flaky.
printf 'ELF\0\0\0binary\0payload\n' > "$PV/blob.dat"
case "$("$DIR/scripts/tmux-open-path" --preview "$PV/blob.dat")" in
    *"binary file"*) ok "a real binary still is" ;;
    *) bad "a real binary still is" "binary file" "$("$DIR/scripts/tmux-open-path" --preview "$PV/blob.dat" | head -1)" ;;
esac
rm -rf "$PV"

# The long-line guard used to measure "longest" over a fixed lines 1-200,
# but bat renders whatever RENDER WINDOW the target line puts it in (up to
# 2000). A giant line sitting past line 200 but inside that window sailed
# past the guard and got dumped whole into the preview on every cursor move
# -- the exact failure the guard exists to prevent. Now "longest" is measured
# over the same [first,last] window bat is about to draw.
GL=$(mktmpd)
{
    for i in $(seq 1 499); do printf 'line %d\n' "$i"; done
    head -c 3000000 /dev/zero | tr '\0' 'x'; printf '\n'
    for i in $(seq 501 700); do printf 'line %d\n' "$i"; done
} > "$GL/big.txt"
# line 495 -> above=min(40/3,10)=10 -> window [485,535], which contains the
# 3,000,000-char line at 500.
glout=$("$DIR/scripts/tmux-open-path" --preview "$GL/big.txt" 495 2>/dev/null | wc -c)
if [ "$glout" -lt 50000 ]; then
    ok "a huge line inside the render window (not lines 1-200) is still guarded"
else
    bad "a huge line inside the render window (not lines 1-200) is still guarded" \
        "<50000 bytes" "$glout bytes"
fi
rm -rf "$GL"

# later blocks reuse this stub
ETMUX_TEMPLATE=$(mktmpf); cp "$ETMUX/tmux" "$ETMUX_TEMPLATE"; chmod +x "$ETMUX_TEMPLATE"
rm -rf "$EQ" "$ETMUX"

echo "== tmux-open-path: with no code on PATH, a click opens through VS Code's remote shim on the live socket =="
# Remote-SSH and Coder put no code(1) on tmux's PATH: the CLI is a shim under
# ~/.vscode-server that only reaches a window through VSCODE_IPC_HOOK_CLI, and
# only when that socket is LIVE. The shim is its own probe -- a dead socket
# fails in ~150 ms, a live one answers --goto in ~150 ms -- so the fake below
# exits 1 on every socket but live.sock, and the NEWEST socket is the dead one.
VTMUX=$(mktmpd); VQ=$(mktmpd); VH=$(mktmpd)
VSK="$VH/run"; VLOG="$VH/log"; VSHIM="$VH/.vscode-server/cli/servers/Stable-abc/server/bin/remote-cli"
: > "$VQ/it's.txt"
# Self-contained by design: make test-only extracts a block on its own, so this
# one builds its own tmux stub instead of borrowing another block's.
cat > "$VTMUX/tmux" <<'VSTUB'
#!/usr/bin/env bash
case "$1" in
  show-options)
      case " $* " in
          *@cockpit_click_line*)  printf '%s\n' "${CLICK_LINE:-}" ;;
          *@cockpit_open_single*) printf '%s\n' "${OPEN_SINGLE:-}" ;;
          *@cockpit_editor*)      printf '%s\n' "${OPEN_EDITOR:-}" ;;
          *@open-editor*) printf '%s\n' "${OPEN_EDITOR2:-}" ;;
      esac ;;
  display-message)
      case " $* " in
          *pane_current_path*) printf '%s\n' "${CLICK_DIR:-}" ;;
          *window_id*)         printf '@0\n' ;;
          *)                   printf 'MSG %s\n' "$*" >> "$TMUX_LOG" ;;
      esac ;;
  new-window)    printf 'NEWWIN %s\n' "${!#}" >> "$TMUX_LOG" ;;
  display-popup) printf 'POPUP %s\n'  "${!#}" >> "$TMUX_LOG" ;;
  run-shell)     printf 'RUNSHELL %s\n' "$*" >> "$TMUX_LOG" ;;
esac
exit 0
VSTUB
chmod +x "$VTMUX/tmux"
# env -i below leaves only this dir on PATH, so everything the script and the
# lib run has to be in it: bash for the shebang, ls/head/grep for the probe.
for t in bash ls head grep sort readlink; do ln -sf "$(command -v $t)" "$VTMUX/$t"; done
VOUT="$VTMUX/log"
mkdir -p "$VSHIM" "$VSK"
# /bin/sh is dash here, so no ${@: -1}: the last argument comes off a loop.
cat > "$VSHIM/code" <<'VSHIMEOF'
#!/bin/sh
# The real CLI reads its stdin. If the probe loop leaves the socket list on
# fd 0, this eats the rest of it and the walk stops after one socket.
head -c 200 >/dev/null
printf 'SOCK=%s ARGS=%s\n' "$VSCODE_IPC_HOOK_CLI" "$*" >> "$VLOG"
case "$VSCODE_IPC_HOOK_CLI" in
    *live.sock) case " $* " in
                    *' --wait '*) for a in "$@"; do last=$a; done
                                  printf 'FROM-VSCODE\n' >> "$last" ;;
                esac
                exit 0 ;;
    *) exit 1 ;;
esac
VSHIMEOF
chmod +x "$VSHIM/code"
: > "$VSK/vscode-ipc-live.sock"; touch -t 202001010000 "$VSK/vscode-ipc-live.sock"
: > "$VSK/vscode-ipc-dead.sock"
: > "$VOUT"
env -i PATH="$VTMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$VOUT" \
    CLICK_DIR="$VQ" CLICK_LINE="it's.txt:12" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
vcmd=$(sed -n 's/^RUNSHELL run-shell -b //p' "$VOUT" | head -1)
# The quote in the name is shell-quoted for run-shell (it\'s.txt), so a glob
# for the plain name stops at the escape. The shim's own log below is where the
# path is checked whole -- after sh has put it back together.
case "$vcmd" in
    *"--vscode --goto"*"s.txt:12:1"*) ok "the click hands the file to the shim through run-shell" ;;
    *) bad "the click hands the file to the shim through run-shell" \
         "--vscode --goto .../it's.txt:12:1" "$(cat "$VOUT")" ;;
esac
: > "$VLOG"
# </dev/null, or the mock shim's stdin drain blocks on the suite's own terminal.
# The probe's fd-0 bug survives this redirect: the buggy loop rebinds fd 0 to
# the socket list for the whole `while`, /dev/null and all.
env -i PATH="$VTMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" bash -c "$vcmd" </dev/null
case "$(cat "$VLOG")" in
    *"SOCK=$VSK/vscode-ipc-dead.sock"*"SOCK=$VSK/vscode-ipc-live.sock ARGS=--goto $VQ/it's.txt:12:1"*)
        ok "the newer dead socket is tried first and skipped, the live one takes the file" ;;
    *) bad "the newer dead socket is tried first and skipped, the live one takes the file" \
         "dead then live" "$(cat "$VLOG")" ;;
esac
# The shim sits BETWEEN code(1) and $VISUAL/$EDITOR, so the two things it must
# never displace are the ones above it: an explicit @cockpit_editor, and a real
# code(1) on a host that has one.
: > "$VOUT"
env -i PATH="$VTMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$VOUT" \
    CLICK_DIR="$VQ" CLICK_LINE="it's.txt" OPEN_EDITOR=nvim \
    bash "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
case "$(cat "$VOUT")" in
    *NEWWIN*nvim*) ok "@cockpit_editor still wins over the shim" ;;
    *) bad "@cockpit_editor still wins over the shim" "a new window running nvim" "$(cat "$VOUT")" ;;
esac
printf '#!/bin/sh\nexit 0\n' > "$VTMUX/code"; chmod +x "$VTMUX/code"
: > "$VOUT"
env -i PATH="$VTMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$VOUT" \
    CLICK_DIR="$VQ" CLICK_LINE="it's.txt:12" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
case "$(cat "$VOUT")" in
    *"--vscode"*) bad "code(1) on PATH still beats the shim" "code --goto" "$(cat "$VOUT")" ;;
    *"RUNSHELL run-shell -b code --goto"*) ok "code(1) on PATH still beats the shim" ;;
    *) bad "code(1) on PATH still beats the shim" "a run-shell carrying code --goto" "$(cat "$VOUT")" ;;
esac
rm -f "$VTMUX/code"

echo "== tmux-open-path: alt-c copies the marked paths to the tmux buffer =="
if need_tmux "tmux-open-path alt-c"; then :; else
# Under $HOME on purpose, not mktmpd's /tmp: field 2 of the list is the
# ~-abbreviated path, so only a fixture below $HOME makes the picker's own
# un-abbreviation the thing under test. Removed at the end of the block.
PC="cockpit-pc-$$"; SOCKS="$SOCKS $PC"; PCD="$HOME/.cache/cockpit-test-$$"
mkdir -p "$PCD"; : > "$PCD/a.txt"; : > "$PCD/b.txt"; : > "$PCD/c.txt"
PCS=$(mktmpd); cat > "$PCS/tmux" <<'STUB'
case "$1" in
  show-options) printf '%s\n' "${CLICK_LINE:-}" ;;   # every option query answers the clicked line; only @cockpit_click_line is read here
  display-message) case " $* " in *pane_current_path*) printf '%s\n' "$CLICK_DIR" ;; *window_id*) printf '@0\n' ;; *) printf 'MSG %s\n' "$*" >> "$TMUX_LOG" ;; esac ;;
  load-buffer) printf 'LOADBUF %s\n' "$*" >> "$TMUX_LOG"; cat > "$TMUX_LOG.buf" ;;
  *) printf '%s\n' "$*" >> "$TMUX_LOG" ;;
esac
exit 0
STUB
# sh too: fzf runs an execute-silent() body through `sh -c`, and env -i leaves
# only this dir on PATH -- without it alt-c dies as `exec: "sh": not found`.
chmod +x "$PCS/tmux"; for t in bash sh fzf ls head grep sort sed awk cat tput; do ln -sf "$(command -v $t)" "$PCS/$t"; done
"$TMUX_REAL" -L "$PC" -f /dev/null new-session -d -s t -x 100 -y 30 "env -i PATH=$(printf '%q' "$PCS") HOME=$(printf '%q' "$HOME") TMUX_LOG=$(printf '%q' "$PCS/log") CLICK_DIR=$(printf '%q' "$PCD") CLICK_LINE='a.txt b.txt c.txt' TERM=xterm bash $(printf '%q' "$DIR/scripts/tmux-open-path") --pick %1; sleep 30" >/dev/null 2>&1
PCP=$("$TMUX_REAL" -L "$PC" list-panes -t t: -F '#{pane_id}')
until_cap "$PC" "$PCP" '*open>*' || bad "the picker comes up" "open>" "$("$TMUX_REAL" -L "$PC" capture-pane -p -t "$PCP")"
"$TMUX_REAL" -L "$PC" send-keys -t "$PCP" Tab Tab; until_cap "$PC" "$PCP" '*(2)*'
"$TMUX_REAL" -L "$PC" send-keys -t "$PCP" M-c
until_file "$PCS/log.buf" '*a.txt*'
# Read raw: un-abbreviating HERE would pass whether the bind copies ~/… or the
# absolute path, which is the one thing this block exists to tell apart.
is "alt-c copies the marked paths, one per line, absolute" "$PCD/a.txt"$'\n'"$PCD/b.txt" "$(cat "$PCS/log.buf" 2>/dev/null)"
is "and loads them with -w, so the clipboard gets them too" "LOADBUF load-buffer -w -" "$(grep LOADBUF "$PCS/log" 2>/dev/null)"
"$TMUX_REAL" -L "$PC" kill-server >/dev/null 2>&1; rm -rf "$PCD"
fi

echo "== agent-state: notify must not hang on a silent stdin =="
# The Notification hook piped JSON in and the script read it with cat, which
# waits for EOF. A writer that holds the pipe open without sending -- and Claude
# Code does keep the hook's stdin open -- wedged the hook, and a wedged hook
# blocks the agent that fired it.
FIFO="$STATE/notify.fifo"; mkfifo "$FIFO"
( exec 9>"$FIFO"; sleep 8 ) & holder=$!
timeout 6 "$DIR/scripts/agent-state" notify %999999 >/dev/null 2>&1 < "$FIFO"
rc=$?
kill "$holder" 2>/dev/null; wait "$holder" 2>/dev/null
is "notify gives up rather than wedging the hook" "0" "$rc"
rm -f "$FIFO"

# The bounded read joins the payload line by line, where `cat` handed over the
# whole thing at once. Claude Code pretty-prints its hook JSON, so the marker
# the classifier looks for arrives on its own line, several lines in -- exactly
# the case a line-at-a-time reader can get wrong.
if need_tmux "multi-line notify payload"; then :; else
st notify "$PANE" <<'JSON'
{
  "session_id": "abc123",
  "transcript_path": "/tmp/t.jsonl",
  "message": "Claude needs your permission to use Bash",
  "cwd": "/tmp"
}
JSON
is "a pretty-printed payload is still classified" "needs-input" "$(sv)"
st running "$PANE"
st notify "$PANE" <<'JSON'
{
  "message": "Claude is waiting for your input"
}
JSON
is "and so is a pretty-printed idle payload" "" "$(sv)"
# No trailing newline: the last line only survives because the loop appends
# whatever read left in $chunk after it failed.
printf '%s' '{"message":"Claude needs your permission"}' | st notify "$PANE"
is "a payload with no trailing newline is not lost" "needs-input" "$(sv)"
st off "$PANE"
fi

echo "== tmux-branch: usable without column(1) =="
# The branch list was piped through column -t. Where column is not installed the
# pipe produced nothing and the script reported "no branches yet" -- a confident
# lie about a repo that had several.
CB=$(mktmpd)
(
  cd "$CB" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; : > a; git add a
  c=$(git commit-tree "$(git write-tree)" -m init); git update-ref refs/heads/main "$c"
  git symbolic-ref HEAD refs/heads/main; git reset -q --hard
  git branch feature/one
) >/dev/null 2>&1
NOCOL=$(mktmpd)
for c in git bash; do ln -s "$(command -v $c)" "$NOCOL/$c"; done
out=$(env -u TMUX PATH="$NOCOL" "$DIR/scripts/tmux-branch" "$CB" </dev/null 2>&1)
case "$out" in
    *feature/one*) ok "column missing: the branches are still listed" ;;
    *) bad "column missing: the branches are still listed" "a list naming feature/one" "$out" ;;
esac
rm -rf "$CB" "$NOCOL"

# group: serial
echo "== tmux-branch: builds each row without a fork per branch =="
# The disps loop ran `$(printf ...)` inside a command substitution -- a fork
# per branch. 5000 branches measured 16.9s (for-each-ref itself is 0.12s; 99%
# was the loop). printf -v does the identical formatting inside the running
# shell instead. The fork build is the CPU it burns, so the CPU budget is the
# fail-first teeth: 3s against a measured 1.10s under twelve spinners.
BP=$(mktmpd)
(
  cd "$BP" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; : > a; git add a
  c=$(git commit-tree "$(git write-tree)" -m init); git update-ref refs/heads/main "$c"
  git symbolic-ref HEAD refs/heads/main; git reset -q --hard
  {
    printf '# pack-refs with: peeled fully-peeled sorted\n'
    for i in $(seq 1 5000); do printf '%s refs/heads/br%d\n' "$c" "$i"; done
  } > .git/packed-refs
) >/dev/null 2>&1
# A stub fzf: drains the piped rows (so the real row-build cost is still paid
# in full) and exits 130 (esc), the same code a real esc keypress produces --
# the fastest path back out once fzf would have taken over the terminal.
FZFSTUB=$(mktmpd)
cat > "$FZFSTUB/fzf" <<'EOS'
#!/usr/bin/env bash
cat >/dev/null
exit 130
EOS
chmod +x "$FZFSTUB/fzf"
for c in git bash tput; do
    p=$(command -v "$c" 2>/dev/null) && ln -s "$p" "$FZFSTUB/$c"
done
cpu_under "5000 branches build their rows in well under 3s of CPU" 3.0 \
    env -u TMUX PATH="$FZFSTUB" "$DIR/scripts/tmux-branch" "$BP"
rm -rf "$BP" "$FZFSTUB"

echo "== tmux-branch: row format is unchanged by the fork removal =="
# Same fixture, small enough to read by hand: proves printf -v produced the
# exact same padded columns as the old $(printf ...) subshell, not just that
# it runs fast. Forced through the no-fzf numbered menu, which prints each
# disps row verbatim to stderr -- the same string a real fzf list would show.
BF=$(mktmpd)
(
  cd "$BF" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  # A fixed commit date: the row is read twice below and %(committerdate:relative)
  # flipped "0 seconds ago" -> "1 second ago" across the boundary (1 run in 8).
  export GIT_AUTHOR_DATE=2000-01-01T00:00:00Z GIT_COMMITTER_DATE=2000-01-01T00:00:00Z
  git init -q -b main .; : > a; git add a
  c=$(git commit-tree "$(git write-tree)" -m init); git update-ref refs/heads/main "$c"
  git symbolic-ref HEAD refs/heads/main; git reset -q --hard
) >/dev/null 2>&1
age=$(git -C "$BF" for-each-ref --format='%(committerdate:relative)' refs/heads/main)
want_row=$(printf '%s %-32s %-16s %s' '●' 'main' "($age)" '')
NOCOLB=$(mktmpd)
for c in git bash; do ln -s "$(command -v $c)" "$NOCOLB/$c"; done
out=$(env -u TMUX PATH="$NOCOLB" "$DIR/scripts/tmux-branch" "$BF" </dev/null 2>&1)
case "$out" in
    *"$want_row"*) ok "the row's field widths are byte-identical to the old build" ;;
    *) bad "the row's field widths are byte-identical to the old build" "$want_row" "$out" ;;
esac
rm -rf "$BF" "$NOCOLB"

echo "== tmux-branch: the popup resolves the pane it was opened from, not the busiest client =="
# Inside a popup $TMUX_PANE is the popup's own pseudo-pane, so bare
# `display-message -p` (no -t) cannot resolve "the current client" from
# there and falls back to whichever attached client was most recently
# active -- a DIFFERENT session than the one the popup was opened from, if
# that session's client is busier. Reproduced with two real attached clients
# on one server: cliB attaches after cliA, so cliB is the more recent one.
if need_tmux "branch popup pane resolution"; then :; else
DA=$(mktmpd); DB=$(mktmpd)
mkrepo() {
    cd "$1" || exit
    export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
    git init -q -b main .; : > a; git add a
    c=$(git commit-tree "$(git write-tree)" -m init); git update-ref refs/heads/main "$c"
    git symbolic-ref HEAD refs/heads/main; git reset -q --hard
}
(mkrepo "$DA") >/dev/null 2>&1
(mkrepo "$DB") >/dev/null 2>&1
INNER="cockpit-branch-inner-$$"; OUTERA="cockpit-branch-outera-$$"; OUTERB="cockpit-branch-outerb-$$"
SOCKS="$SOCKS $INNER $OUTERA $OUTERB"
tmux -L "$INNER" -f /dev/null new-session -d -s cliA -c "$DA" >/dev/null 2>&1
tmux -L "$INNER" new-session -d -s cliB -c "$DB" >/dev/null 2>&1
bp_clients() { "$TMUX_REAL" -L "$INNER" list-clients 2>/dev/null | grep -c .; }
# The hazard below needs cliB to be the MORE RECENT client, so count them: the
# second attach is only after the first one has landed.
tmux -L "$OUTERA" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $INNER attach -t cliA" >/dev/null 2>&1
until_out 1 bp_clients
tmux -L "$OUTERB" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $INNER attach -t cliB" >/dev/null 2>&1
until_out 2 bp_clients
paneA=$(tmux -L "$INNER" list-panes -t cliA -F '#{pane_id}')
SOCKPATH=$(tmux -L "$INNER" display-message -p '#{socket_path}')
NOCOLD=$(mktmpd)
for c in git bash tmux; do ln -s "$(command -v $c)" "$NOCOLD/$c"; done
# Without -p: the hazard is real -- it resolves into cliB, not the popup's
# actual origin (cliA), because cliB is the busier client.
bugout=$(env PATH="$NOCOLD" TMUX="$SOCKPATH,0,0" "$DIR/scripts/tmux-branch" </dev/null 2>&1)
case "$bugout" in
    *"$DB"*) ok "unpinned resolution really does land on the busier client (the hazard is real)" ;;
    *) bad "unpinned resolution really does land on the busier client (the hazard is real)" "$DB" "$bugout" ;;
esac
# With -p pane_id: pinned to the pane the popup actually opened from.
fixout=$(env PATH="$NOCOLD" TMUX="$SOCKPATH,0,0" "$DIR/scripts/tmux-branch" -p "$paneA" </dev/null 2>&1)
case "$fixout" in
    *"$DA"*) ok "-p pane_id resolves the pane the popup was opened from" ;;
    *) bad "-p pane_id resolves the pane the popup was opened from" "$DA" "$fixout" ;;
esac
tmux -L "$OUTERA" kill-server >/dev/null 2>&1
tmux -L "$OUTERB" kill-server >/dev/null 2>&1
tmux -L "$INNER" kill-server >/dev/null 2>&1
rm -rf "$DA" "$DB" "$NOCOLD"
fi

echo "== tm: the kill picker and the name cleaner =="
# Without fzf the kill picker lists the sessions and reads a name. An empty
# answer -- Ctrl-D, or no input at all -- must exit quietly under `set -u`.
TMS=$(mktmpd)
for c in readlink bash grep sleep; do ln -s "$(command -v $c)" "$TMS/$c"; done
cat > "$TMS/tmux" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  list-sessions) printf 'alpha\nbeta\n' ;;
  kill-session)  printf 'KILL %s\n' "${*: -1}" >> "$TM_LOG" ;;
esac
exit 0
STUB
chmod +x "$TMS/tmux"
: > "$TMS/log"
out=$(env -u TMUX PATH="$TMS" TM_LOG="$TMS/log" "$DIR/scripts/tm" -k </dev/null 2>&1)
case "$out" in
    *"unbound variable"*) bad "kill picker survives an empty choice" "a quiet exit" "$out" ;;
    # The prompt proves the picker really ran; without it an absent interpreter
    # would satisfy the "no unbound variable" check on its own.
    *"Kill which session"*) ok "kill picker survives an empty choice" ;;
    *) bad "kill picker survives an empty choice" "the picker prompt" "$out" ;;
esac
is "and it killed nothing" "" "$(cat "$TMS/log")"

# tmux reads a leading "-" as an option, so a session named from the prompt
# could not be created at all -- new-session -s -x is a usage error.
cat > "$TMS/tmux" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  list-sessions)   exit 0 ;;
  has-session)     exit 1 ;;
  new-session)     printf 'NEW %s\n' "$*" >> "$TM_LOG" ;;
  display-message) printf '0\n' ;;
esac
exit 0
STUB
: > "$TMS/log"
# HOME is the stub's: the real one may hold a resurrect save, which is the
# next case, not this one.
printf -- '-weird name\n' | env -u TMUX -u XDG_CONFIG_HOME HOME="$TMS" PATH="$TMS" \
    TM_LOG="$TMS/log" "$DIR/scripts/tm" >/dev/null 2>&1
is "a leading dash never reaches tmux" "NEW new-session -d -P -F #{session_id} -s weird_name -c $PWD" "$(cat "$TMS/log")"
# Cancelling the prompt used to create a session named "": ask_name's exit
# only left its $(...) subshell, so go ran with an empty name (#132).
: > "$TMS/log"
env -u TMUX -u XDG_CONFIG_HOME HOME="$TMS" PATH="$TMS" TM_LOG="$TMS/log" \
    "$DIR/scripts/tm" </dev/null >/dev/null 2>&1; tmrc=$?
is "a cancelled name prompt creates nothing" "" "$(cat "$TMS/log")"
is "and exits 0" "0" "$tmrc"

# After a reboot tm asked for a name although resurrect had a save: continuum
# restores on server start and tm never started one (#106). The stub brings
# alpha back only on the SECOND list, so a tm that never waits cannot see it.
cat > "$TMS/tmux" <<'STUB'
#!/bin/sh
# sh, not bash: the cap case forks this a hundred times.
case "$1" in
  list-sessions)
    grep -q NEW "$TM_LOG" || exit 0
    n=0; [ -s "$TM_LOG.polls" ] && read -r n < "$TM_LOG.polls"
    printf '%s\n' "$((n + 1))" > "$TM_LOG.polls"
    read -r ph < "$TM_LOG"; grep -q KILL "$TM_LOG" || printf '%s\n' "${ph##* -s }"
    [ -n "${TM_RESTORE:-}" ] && [ "$n" -ge 1 ] && printf 'alpha\n' ;;
  show-option)
    if [ "$3" = @resurrect-dir ]; then printf '%s\n' "${TM_RDIR:-}"; else printf 'on\n'; fi ;;
  new-session)   printf 'NEW %s\n' "$*" >> "$TM_LOG" ;;
  kill-session)  for a in "$@"; do t=$a; done; printf 'KILL %s\n' "$t" >> "$TM_LOG" ;;
esac
exit 0
STUB
# Neither a save nor resurrect on disk: starting a server to discover that
# costs a whole tmux config load, about a second on this box.
: > "$TMS/log"; rm -f "$TMS/log.polls"
err=$(env -u TMUX -u XDG_CONFIG_HOME HOME="$TMS" PATH="$TMS" TM_RESTORE=1 \
      TM_LOG="$TMS/log" "$DIR/scripts/tm" </dev/null 2>&1 >/dev/null)
is "nothing on disk means no server at all" "" "$(cat "$TMS/log")"
case "$err" in
    *'No tmux sessions running.'*) ok "and the name prompt comes straight away" ;;
    *) bad "and the name prompt comes straight away" "the name prompt" "$err" ;;
esac

mkdir -p "$TMS/.local/share/tmux/resurrect"; : > "$TMS/.local/share/tmux/resurrect/last"
mkdir -p "$TMS/.tmux/plugins/tmux-resurrect"   # from here on, resurrect is installed
: > "$TMS/log"
env -u TMUX HOME="$TMS" PATH="$TMS" TM_RESTORE=1 TM_LOG="$TMS/log" "$DIR/scripts/tm" </dev/null >/dev/null 2>&1
# The placeholder carries tm's pid, so the assertion cannot name it literally.
is "a resurrect save starts the server, then drops the placeholder" \
   "NEW new-session -d -s _restore#
KILL =_restore#" "$(sed 's/_restore[0-9]*/_restore#/' "$TMS/log")"
# Three lists after the placeholder: the poll's first (placeholder only), its
# second (alpha is back, break), and tm's re-read. No wait reads 1, no break 101.
is "and it stopped polling as soon as the restore landed" "3" "$(cat "$TMS/log.polls")"

# The restore can simply never come: @continuum-restore being on is not the
# whole of continuum's gate. Without TM_RESTORE the stub never brings alpha
# back, and sleep is a no-op so the hundred naps cost the suite nothing.
: > "$TMS/log"; rm -f "$TMS/log.polls"
ln -sf "$(type -P true)" "$TMS/sleep"   # -P: command -v would hand back the builtin
err=$(env -u TMUX HOME="$TMS" PATH="$TMS" TM_LOG="$TMS/log" "$DIR/scripts/tm" </dev/null 2>&1 >/dev/null)
case "$err" in
    *'cockpit: no restore after 10s'*) ok "a restore that never lands says so" ;;
    *) bad "a restore that never lands says so" "a cockpit: line" "$err" ;;
esac

# The save need not be in either default directory: resurrect reads
# @resurrect-dir, and tm asks the placeholder server for it (#110). The option
# is quoted, not expanded, because expanding it is tm's job, like resurrect's.
mkdir -p "$TMS/elsewhere"; : > "$TMS/elsewhere/last"
mv "$TMS/.local/share/tmux/resurrect/last" "$TMS/moved-away"
for rdir in '$HOME/elsewhere' '~/elsewhere'; do
  : > "$TMS/log"; rm -f "$TMS/log.polls"
  err=$(env -u TMUX HOME="$TMS" PATH="$TMS" TM_RDIR="$rdir" TM_RESTORE=1 \
        TM_LOG="$TMS/log" "$DIR/scripts/tm" </dev/null 2>&1 >/dev/null)
  case "$err" in
      # The restored session in the picker, which is what the user came for.
      *'Sessions:'*alpha*) ok "@resurrect-dir $rdir names the save tm waits for" ;;
      *) bad "@resurrect-dir $rdir names the save tm waits for" \
             "the restored picker" "$err" ;;
  esac
  is "and it waited for it ($rdir)" "3" "$(cat "$TMS/log.polls" 2>/dev/null)"
done

# That option REPLACES the defaults, it does not add to them: continuum will
# read only it, so a stale save in a default directory promises nothing.
mkdir -p "$TMS/empty-dir"; mv "$TMS/moved-away" "$TMS/.local/share/tmux/resurrect/last"
: > "$TMS/log"; rm -f "$TMS/log.polls"
err=$(env -u TMUX HOME="$TMS" PATH="$TMS" TM_RDIR="$TMS/empty-dir" TM_RESTORE=1 \
      TM_LOG="$TMS/log" "$DIR/scripts/tm" </dev/null 2>&1 >/dev/null)
case "$err" in
    *Restoring*|*'no restore after'*) bad "an empty @resurrect-dir promises nothing" \
                                  "no restore line" "$err" ;;
    *'No tmux sessions running.'*) ok "an empty @resurrect-dir promises nothing" ;;
    *) bad "an empty @resurrect-dir promises nothing" "the name prompt" "$err" ;;
esac
# One list after the placeholder, tm's own re-read: the poll never ran.
is "and it never polled" "1" "$(cat "$TMS/log.polls" 2>/dev/null)"

# resurrect reads ONE default directory, ~/.tmux/resurrect when it exists and
# the XDG path otherwise, so an empty one hides a save under the other.
mkdir -p "$TMS/.tmux/resurrect"          # exists and empty: now THE default
: > "$TMS/log"; rm -f "$TMS/log.polls"
err=$(env -u TMUX HOME="$TMS" PATH="$TMS" TM_RESTORE=1 TM_LOG="$TMS/log" \
      "$DIR/scripts/tm" </dev/null 2>&1 >/dev/null)
case "$err" in
    *Restoring*|*'no restore after'*) bad "an empty default directory waits for nothing" \
                                  "no restore line" "$err" ;;
    *'No tmux sessions running.'*) ok "an empty default directory waits for nothing" ;;
    *) bad "an empty default directory waits for nothing" "the name prompt" "$err" ;;
esac

# TPM under the XDG layout keeps the plugin elsewhere, and that user has a
# save like anyone else.
rm -rf "$TMS/.tmux/plugins"; mkdir -p "$TMS/.config/tmux/plugins/tmux-resurrect"
: > "$TMS/log"; rm -f "$TMS/log.polls"
env -u TMUX -u XDG_CONFIG_HOME HOME="$TMS" PATH="$TMS" TM_LOG="$TMS/log" \
    "$DIR/scripts/tm" </dev/null >/dev/null 2>&1
is "the XDG plugin path counts as installed" "NEW new-session -d -s _restore#
KILL =_restore#" "$(sed 's/_restore[0-9]*/_restore#/' "$TMS/log")"

# TPM can be pointed anywhere, and that user keeps a save where resurrect
# looks like everyone else. Not restoring them is #106 with no message.
rmdir "$TMS/.tmux/resurrect"; rm -rf "$TMS/.config"
mkdir -p "$TMS/dotfiles/tmux/plugins/tmux-resurrect"   # a TPM path nobody probes
: > "$TMS/log"; rm -f "$TMS/log.polls"
err=$(env -u TMUX -u XDG_CONFIG_HOME HOME="$TMS" PATH="$TMS" TM_RESTORE=1 \
      TM_LOG="$TMS/log" "$DIR/scripts/tm" </dev/null 2>&1 >/dev/null)
case "$err" in
    *'Sessions:'*alpha*) ok "a save in the default directory counts as installed" ;;
    *) bad "a save in the default directory counts as installed" \
           "the restored picker" "$err" ;;
esac
rm -rf "$TMS"

echo "== @cockpit_statusbar off leaves your bar alone and still publishes the options =="
# The clean break from the #{agent_*} tokens: nothing rewrites a user's options
# any more. A bar of one's own reads #{E:@cockpit_where_path}, _branch and
# #{E:@cockpit_fleet}, which the plugin sets either way.
if need_tmux "statusbar off"; then :; else
SOCK2="cockpit-sboff-$$"; SOCKS="$SOCKS $SOCK2"
tmux -L "$SOCK2" -f /dev/null new-session -d -s t >/dev/null 2>&1
tmux -L "$SOCK2" set -g @cockpit_statusbar off >/dev/null 2>&1
for o in status-right status-left window-status-format pane-border-format; do
    tmux -L "$SOCK2" set -g "$o" "mine $o" >/dev/null 2>&1
done
tmux -L "$SOCK2" set -g status-style "bg=red" >/dev/null 2>&1
tmux -L "$SOCK2" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
for o in status-right status-left window-status-format pane-border-format; do
    is "$o is left as it was" "mine $o" "$(tmux -L "$SOCK2" show-option -gqv "$o")"
done
is "status-style is left as it was" "bg=red" "$(tmux -L "$SOCK2" show-option -gqv status-style)"
is "and so are tmux's theme slots" "#{?#{e|>=:#{client_colours},256},indianred,red}" \
   "$(tmux -L "$SOCK2" show-option -sv dark-theme-red)"
for o in @cockpit_where_path @cockpit_where_branch; do
    case "$(tmux -L "$SOCK2" show-option -gqv "$o")" in
        *pane_current_command*) ok "$o is published" ;;
        *) bad "$o is published" "a format" "$(tmux -L "$SOCK2" show-option -gqv "$o")" ;;
    esac
done
tmux -L "$SOCK2" set -g status-right "L #{agent_where_path} R" >/dev/null 2>&1
tmux -L "$SOCK2" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "an old token is not rewritten any more" "L #{agent_where_path} R" "$(tmux -L "$SOCK2" show-option -gqv status-right)"
tmux -L "$SOCK2" kill-server >/dev/null 2>&1
fi

echo "== a directory name can no longer run code through the status job =="
# The old #{agent_where_path} token embedded pane_current_path inside a #() job with
# hand-written single quotes -- tmux expands the token INSIDE the job before sh
# ever sees it, so a bare ' in the cwd closed that quote early and a trailing
# $(...) ran as a real command, on every status redraw, for anyone with the
# token in status. An outer tmux server provides the pty for a client attached
# to the inner one (the split-picker test above uses the same pattern) so the
# #() job -- which only runs for an attached client -- actually fires.
if need_tmux "RCE via directory name"; then :; else
SP=$(mktmpd)
MAL="$SP/x'\$(touch $SP/OWNED)'y"
mkdir -p "$MAL"
OUTER="cockpit-rce-outer-$$"; INNER="cockpit-rce-inner-$$"
SOCKS="$SOCKS $OUTER $INNER"
tmux -L "$INNER" -f /dev/null new-session -d -s s -x 100 -y 24 -c "$MAL" \
    'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$INNER" set -g status-interval 1 >/dev/null 2>&1
tmux -L "$INNER" set -g @cockpit_statusbar off >/dev/null 2>&1
tmux -L "$INNER" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
tmux -L "$INNER" set -g status-right "#{E:@cockpit_where_path} #{E:@cockpit_where_branch}" >/dev/null 2>&1
tmux -L "$OUTER" -f /dev/null new-session -d -s o -x 100 -y 24 \
    "$TMUX_REAL -L $INNER attach -t s" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$INNER" list-clients -F '#{client_name}'
tmux -L "$INNER" refresh-client >/dev/null 2>&1
# The sleep stays: the assertion is that the #() job never ran, and an absence
# has no observable to poll. The sibling block below proves the job does fire
# inside this window when the cwd is a normal one.
sleep 1
if [ -e "$SP/OWNED" ]; then
    bad "a hostile cwd's \$(...) never runs" "no OWNED file" "OWNED file created"
else
    ok "a hostile cwd's \$(...) never runs"
fi
tmux -L "$OUTER" kill-server >/dev/null 2>&1
tmux -L "$INNER" kill-server >/dev/null 2>&1

# A normal cwd must still render, not just fail to execute. A repo, so the
# branch the hooks push renders too.
D2=$(mktmpd); git init -q -b main "$D2" >/dev/null 2>&1
OUTER2="cockpit-rce-outer2-$$"; INNER2="cockpit-rce-inner2-$$"
SOCKS="$SOCKS $OUTER2 $INNER2"
tmux -L "$INNER2" -f /dev/null new-session -d -s s -x 100 -y 24 -c "$D2" \
    'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$INNER2" set -g status-interval 1 >/dev/null 2>&1
tmux -L "$INNER2" set -g @cockpit_statusbar off >/dev/null 2>&1
tmux -L "$INNER2" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
tmux -L "$INNER2" set -g status-right "#{E:@cockpit_where_path} #{E:@cockpit_where_branch}" >/dev/null 2>&1
tmux -L "$OUTER2" -f /dev/null new-session -d -s o -x 100 -y 24 \
    "$TMUX_REAL -L $INNER2 attach -t s" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$INNER2" list-clients -F '#{client_name}'
tmux -L "$INNER2" refresh-client >/dev/null 2>&1
until_cap "$OUTER2" o "*$D2 main*"
scr=$(tmux -L "$OUTER2" capture-pane -p -t o)
case "$scr" in
    *"$D2 main"*) ok "a normal cwd still renders its path and branch" ;;
    *) bad "a normal cwd still renders its path and branch" "$D2 main on the status line" "$scr" ;;
esac
tmux -L "$OUTER2" kill-server >/dev/null 2>&1
tmux -L "$INNER2" kill-server >/dev/null 2>&1
fi

echo "== ...and neither can a NEWLINE or TAB, which #{q:} alone does not escape =="
# #{q:} (man tmux: "q: will escape sh(1) special characters") looked like the
# whole fix, but it backslash-escapes quotes/$()/backticks and passes NEWLINE
# and TAB straight through -- verified: #{q:@x} on "a\nb\tc" comes back
# "a\nb\tc" (0a and 09 both raw, no backslash). A newline is a command
# separator to sh and a tab is a word separator, so a cwd built on either
# still ran arbitrary commands, with arguments, through #{q:pane_current_path}
# alone. The real fix stops passing pane_current_path/_command through the
# #() job's command string at all -- only pane_pid and pane_id go in there now,
# both closed forms that can never carry a shell metacharacter.
if need_tmux "RCE via newline/tab in a directory name"; then :; else
SP2=$(mktmpd)
MAL2="$SP2/x
touch	$SP2/marker
y"
mkdir -p "$MAL2"
OUTER3="cockpit-rce-outer3-$$"; INNER3="cockpit-rce-inner3-$$"
SOCKS="$SOCKS $OUTER3 $INNER3"
tmux -L "$INNER3" -f /dev/null new-session -d -s s -x 100 -y 24 -c "$MAL2" \
    'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$INNER3" set -g status-interval 1 >/dev/null 2>&1
tmux -L "$INNER3" set -g @cockpit_statusbar off >/dev/null 2>&1
tmux -L "$INNER3" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
tmux -L "$INNER3" set -g status-right "#{E:@cockpit_where_path} #{E:@cockpit_where_branch}" >/dev/null 2>&1
tmux -L "$OUTER3" -f /dev/null new-session -d -s o -x 100 -y 24 \
    "$TMUX_REAL -L $INNER3 attach -t s" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$INNER3" list-clients -F '#{client_name}'
tmux -L "$INNER3" refresh-client >/dev/null 2>&1
# The sleep stays, same as the $(...) case above: nothing to poll for an
# absence, and the normal-cwd block proves the job fires inside this window.
sleep 1
if [ -e "$SP2/marker" ]; then
    bad "a newline/tab cwd's command never runs" "no marker file" "marker file created"
else
    ok "a newline/tab cwd's command never runs"
fi
tmux -L "$OUTER3" kill-server >/dev/null 2>&1
tmux -L "$INNER3" kill-server >/dev/null 2>&1
fi

echo "== tmux-where: detached HEAD and odd repos =="
DH=$(mktmpd)
(
  cd "$DH" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; : > a; git add a
  c=$(git commit-tree "$(git write-tree)" -m init); git update-ref refs/heads/main "$c"
  git symbolic-ref HEAD refs/heads/main; git reset -q --hard
) >/dev/null 2>&1
sha=$(git -C "$DH" rev-parse HEAD)
is "on a branch, the branch is shown" " main" \
   "$("$DIR/scripts/tmux-where" branch 1 "$DH" claude 2>/dev/null | plain)"
git -C "$DH" checkout -q --detach "$sha" 2>/dev/null
# --abbrev-ref prints the literal word "HEAD" when detached, which reads as a
# branch of that name; the label has to be the hash instead.
d=$("$DIR/scripts/tmux-where" branch 1 "$DH" claude 2>/dev/null | plain)
case "$d" in
    *HEAD*) bad "detached HEAD shows a hash, not the word HEAD" "@${sha:0:7}" "$d" ;;
    *"@${sha:0:7}"*) ok "detached HEAD shows a hash, not the word HEAD" ;;
    *) bad "detached HEAD shows a hash, not the word HEAD" "@${sha:0:7}" "$d" ;;
esac
# A redirect's error goes to stderr unless 2>/dev/null comes before it.
mkdir -p "$DH/locked"; echo "gitdir: x" > "$DH/locked/.git"; chmod 000 "$DH/locked/.git"
is "an unreadable .git file is quiet, not a bash error" "" \
   "$("$DIR/scripts/tmux-where" branch 1 "$DH/locked" claude 2>&1 >/dev/null)"
chmod 600 "$DH/locked/.git"
# A worktree removed after the board resolved its root.
is "the board's scan of a vanished root is quiet" "" \
   "$(bash -c '. "$1/scripts/lib/review-diff.sh"; . "$1/scripts/lib/review-fleet.sh"; fleet_scan_root "$2"' _ "$DIR" "$DH/gone" 2>&1 >/dev/null)"
echo b >> "$DH/a"; chmod 000 "$DH/a"
is "the diff preview of an unreadable changed file is quiet" "" \
   "$("$DIR/scripts/tmux-open-path" --preview-diff "$DH/a" 2>&1 >/dev/null)"
chmod 600 "$DH/a"
rm -rf "$DH"
echo "== install.sh only ever touches its own links =="
IB=$(mktmpd); IL="$IB/link"; ln -s "$DIR" "$IL"; IBIN="$IB/bin"; mkdir -p "$IBIN"
# HOME="$IB" on every call below, and a ~/.claude to find: install.sh also links
# the cockpit-review skill into ~/.claude/skills wherever Claude Code lives, and
# the suite must never write into the real one.
mkdir -p "$IB/.claude"
HOME="$IB" XDG_BIN_HOME="$IBIN" "$IL/install.sh" >/dev/null 2>&1
is "installs through a symlinked plugin dir" "3" "$(ls "$IBIN" | wc -l)"
is "the skill is linked into ~/.claude/skills" "$DIR/skills/cockpit-review" "$(readlink "$IB/.claude/skills/cockpit-review")"
is "and so is every other skills/* dir" "$DIR/skills/unslop" "$(readlink "$IB/.claude/skills/unslop")"
# DIR came from a LOGICAL pwd while the guard compared readlink -f of the link,
# so reaching the plugin through a symlink -- a dotfiles checkout linked into
# ~/.tmux/plugins is the usual way -- made --uninstall remove nothing, silently.
HOME="$IB" XDG_BIN_HOME="$IBIN" "$IL/install.sh" --uninstall >/dev/null 2>&1
is "and uninstalls through one too" "0" "$(ls "$IBIN" | wc -l)"
is "and unlinked on --uninstall" "absent" "$([ -e "$IB/.claude/skills/cockpit-review" ] && echo present || echo absent)"
is "every skill link, not just the first" "absent" "$([ -e "$IB/.claude/skills/unslop" ] && echo present || echo absent)"
# A same-named symlink owned by another tool must be left alone: "tm" is a name
# plenty of things want, and --uninstall would later have deleted it as well.
HOME="$IB" XDG_BIN_HOME="$IBIN" "$DIR/install.sh" >/dev/null 2>&1
rm -f "$IBIN/tm"; ln -s /bin/echo "$IBIN/tm"
HOME="$IB" XDG_BIN_HOME="$IBIN" "$DIR/install.sh" >/dev/null 2>&1
is "leaves another tool's link alone" "/bin/echo" "$(readlink "$IBIN/tm")"
rm -rf "$IB"

echo "== install.sh --check: one row per thing that can be wrong, exit on the worst =="
# The box is faked whole -- HOME, $BIN, PATH shims, an empty TMUX_TMPDIR so no
# tmux server is reachable -- so every row is the code's answer, never this
# machine's, and the doctor cannot read the live server.
if ! command -v jq >/dev/null 2>&1; then skip_ "install.sh --check (no jq)"; else
DC=$(mktmpd)
DH="$DC/home"; DBIN="$DC/bin"; DSH="$DC/shim"; DOLD="$DC/old"; DNO="$DC/notmux"
mkdir -p "$DH/.claude" "$DBIN" "$DSH" "$DOLD" "$DNO" "$DC/tmuxtmp"
printf '# mine\nrun-shell %s/cockpit.tmux\n' "$DIR" > "$DH/.tmux.conf"
dshim() { printf '#!/bin/sh\necho "%s"\n' "$2" > "$3/$1"; chmod +x "$3/$1"; }
dshim fzf "0.74.3 (15f64c49)" "$DSH"
dshim bat "bat 0.26.1 (a3c4)" "$DSH"
dshim fzf "0.44.1 (debian)" "$DOLD"
# The no-tmux PATH carries only what install.sh itself needs, so "no tmux on
# PATH" is the absence itself rather than a shim standing in for it.
for u in bash dirname jq; do ln -s "$(command -v "$u")" "$DNO/$u" 2>/dev/null; done
HOME="$DH" "$DIR/install.sh" --hooks >/dev/null 2>&1
dc() {                        # dc <PATH> -- the doctor's output on the fake box
    env -u TMUX -u TMUX_PANE HOME="$DH" XDG_BIN_HOME="$DBIN" XDG_CONFIG_HOME="$DH/.config" \
        TMUX_TMPDIR="$DC/tmuxtmp" PATH="$1" "$DIR/install.sh" --check 2>&1
}
# Mark and row name only: the found and fix columns are prose and would make
# every rewording a red suite.
dcrows() { sed -n 's/^\[ *\([a-z]*\) *\] *\([^ ]*\).*/\1 \2/p'; }
dcrow() { printf '%s\n' "$1" | dcrows | grep -c -x "$2"; }
# The formats row's count, anchored to the column so rewording the rest is free.
# The run of spaces is a + rather than a literal: the row helper's width is not
# this assertion's business.
dcfound() { printf '%s\n' "$1" | grep -cE "\] formats +$2"; }

# The parser install.sh grades with, so these assert the numbers rather than a
# row's verdict.
. "$DIR/scripts/lib/vnum.sh"
dcge() { [ "$(vnum "$1" 2>/dev/null)" -ge "$(vnum "$2" 2>/dev/null)" ] 2>/dev/null \
         && echo yes || echo no; }
is "vnum: a letter suffix is not a component (3.7c)" "3007000" "$(vnum 3.7c 2>/dev/null)"
is "vnum: 0.73.0 is not below itself" "yes" "$(dcge 0.73.0 0.73.0)"
is "vnum: the debian fzf is below the 0.73.0 floor, and only that way" "no yes" \
   "$(dcge '0.44.1 (debian)' 0.73.0) $(dcge 0.73.0 '0.44.1 (debian)')"

DCP="$DBIN:$DSH:$SHIM:/usr/bin:/bin"
before=$(find "$DH" "$DBIN" | sort)
dcout=$(dc "$DCP"); dcrc=$?
is "the doctor writes nothing" "$before" "$(find "$DH" "$DBIN" | sort)"
is "fzf at the pinned version is an ok row" "1" "$(dcrow "$dcout" "ok fzf")"
is "a wired config is an ok row"            "1" "$(dcrow "$dcout" "ok config")"
is "a config with no #{agent_*} token is an ok row" "1" "$(dcrow "$dcout" "ok tokens")"
is "merged hooks are an ok row"             "1" "$(dcrow "$dcout" "ok hooks")"
# Neither can be answered from a bare shell, and a skip is 0: a doctor that
# failed in CI over a missing client would be ignored everywhere else.
is "no client means truecolor skips"        "1" "$(dcrow "$dcout" "skip truecolor")"
is "no server means popup-border skips"     "1" "$(dcrow "$dcout" "skip popup-border")"
is "the suite's cockpit-tui is an ok row"   "1" "$(dcrow "$dcout" "ok cockpit-tui")"
dctv=$(tmux -V 2>/dev/null); dctv=${dctv#tmux }
if [ "$(dcge "$dctv" 3.8)" = no ]; then
    skip_ "install.sh --check exits 0 on a wired box (this tmux is below 3.8)"
else
    is "and the whole run exits 0"          "0" "$dcrc"
fi
# Required since #199: missing or below TUI_MIN fails the doctor, not warns.
dcout=$(COCKPIT_TUI_BIN="$DC/no-such-tui" dc "$DCP"); dcrc=$?
is "a missing cockpit-tui is a fail row"    "1" "$(dcrow "$dcout" "fail cockpit-tui")"
is "and it exits 2"                         "2" "$dcrc"
dshim cockpit-tui "cockpit-tui 0.0.1" "$DC"
dcout=$(COCKPIT_TUI_BIN="$DC/cockpit-tui" dc "$DCP")
is "one below TUI_MIN is a fail row too"    "1" "$(dcrow "$dcout" "fail cockpit-tui")"
# Debian ships 0.44.1, below the 0.73 the pickers need.
dcout=$(dc "$DOLD:$DCP"); dcrc=$?
is "an fzf below the floor is a fail row"   "1" "$(dcrow "$dcout" "fail fzf")"
is "and it exits 2"                         "2" "$dcrc"
# A server holds the PATH it started with, so its fzf is the one the pickers
# run: an old one there fails even when this shell finds a new one first.
DCS="env -u TMUX TMUX_TMPDIR=$DC/tmuxtmp $TMUX_REAL -L $SUITE_SOCK -f /dev/null"
$DCS new-session -d 'sleep 300' >/dev/null 2>&1
$DCS set-environment -g PATH "$DOLD:$DCP" >/dev/null 2>&1
is "the fzf on the server's PATH is the one graded" "1" "$(dcrow "$(dc "$DCP")" "fail fzf")"
$DCS kill-server >/dev/null 2>&1
# bat is the optional tool left, and a PATH without it is a warn row.
mkdir -p "$DC/nobat"; dshim fzf "0.74.3 (15f64c49)" "$DC/nobat"
dcout=$(dc "$DBIN:$DC/nobat:$SHIM:$DNO"); dcrc=$?
is "a missing optional tool is a warn row" "1" "$(dcrow "$dcout" "warn bat")"
[ "$(dcge "$dctv" 3.8)" = no ] || is "and warnings exit 1" "1" "$dcrc"
dcout=$(dc "$DBIN:$DSH:$DNO"); dcrc=$?
is "no tmux on PATH is a fail row"          "1" "$(dcrow "$dcout" "fail tmux")"
is "and a failure exits 2"                  "2" "$dcrc"
# tmux loads both files, so the run-shell line in either one wires the box.
mkdir -p "$DH/.config/tmux"; printf 'set -g @unrelated 1\n' > "$DH/.config/tmux/tmux.conf"
dcout=$(dc "$DCP")
is "an XDG config beside a wired ~/.tmux.conf is still ok" "1" "$(dcrow "$dcout" "ok config")"
rm -rf "$DH/.config"

# A config predating the precompute derives the dot and the banner at redraw time.
# tests/freeze-rig/gate.sh puts that at 336 pane visits against a 120 budget, so
# the doctor names it rather than leaving a freeze as the only symptom.
cp "$DH/.tmux.conf" "$DC/conf.bak"
dcout=$(dc "$DCP")
is "a config with no redraw formats is an ok formats row" "1" "$(dcrow "$dcout" "ok formats")"
# The plugin's own bar must pass the rule the doctor holds user configs to.
is "the plugin's bar walks no panes on redraw" "0" \
   "$(grep -c '#{P:' "$DIR/scripts/lib/statusbar.conf")"
# Five #{P:} walks per tab for the dot, and the banner as #{S:#{W:#{P:}}}, which
# is a whole-server walk per visible pane. Both shapes, one row.
printf '%s\n' \
  'set -g @catppuccin_window_text " #{?#{m:*needs-input*,#{P:#{@panestate} }},A,}"' \
  'set -g pane-border-format "#{?#{!=:#{S:#{W:#{?#{m:*needs-input*,#{P:#{@panestate} }},x,}}},},NEEDS YOU,}"' \
  >> "$DH/.tmux.conf"
dcout=$(dc "$DCP"); dcrc=$?
is "a pre-precompute config is a warn formats row" "1" "$(dcrow "$dcout" "warn formats")"
# The count, not the prose around it: dcrow exists a few lines up for that reason.
is "and it counts them" "1" "$(dcfound "$dcout" '2 formats ')"
cp "$DC/conf.bak" "$DH/.tmux.conf"
# The old tokens print as literal text now: the doctor points at the migration.
printf '%s\n' '# set -g status-right "#{agent_fleet}"' '    # an indented comment: #{agent_where_path}' \
    'set -g status-right "#{agent_fleet}"' >> "$DH/.tmux.conf"
dcout=$(dc "$DCP")
is "a live #{agent_*} line is a warn tokens row, a commented one (indented too) is not" "1 1" \
   "$(dcrow "$dcout" "warn tokens") $(printf '%s\n' "$dcout" | grep -c 'tokens *1 lines')"
cp "$DC/conf.bak" "$DH/.tmux.conf"
# The option name has to be a whole word. Without that the glob fires on any
# option whose name merely ends in one of these, and the row cries over a
# #{P:} walk the redraw path never sees.
printf '%s\n' 'set -g @my-window-status-format "#{P:#{@panestate} }"' >> "$DH/.tmux.conf"
dcout=$(dc "$DCP")
is "an unrelated option ending in a format name stays ok" "1" "$(dcrow "$dcout" "ok formats")"
cp "$DC/conf.bak" "$DH/.tmux.conf"
# tmux applies the XDG file second, so a walk there is the value that survives
# even when the wired file is the clean one. Reading only the wired file called
# this layout ok while the freeze was live.
mkdir -p "$DH/.config/tmux"
printf '%s\n' 'set -g pane-border-format "#{P:#{@panestate} }"' > "$DH/.config/tmux/tmux.conf"
dcout=$(dc "$DCP")
is "a walk in the unwired XDG config is still a warn row" "1" "$(dcrow "$dcout" "warn formats")"
is "and one walk reads as one format" "1" "$(dcfound "$dcout" '1 format ')"
# A dotfiles checkout symlinks the two paths together, and counting the file
# twice would report two walks for the one line it has.
rm -f "$DH/.config/tmux/tmux.conf"; ln -s "$DH/.tmux.conf" "$DH/.config/tmux/tmux.conf"
printf '%s\n' 'set -g pane-border-format "#{P:#{@panestate} }"' >> "$DH/.tmux.conf"
dcout=$(dc "$DCP")
is "a symlinked config pair counts its walk once" "1" "$(dcfound "$dcout" '1 format ')"
rm -rf "$DH/.config"; cp "$DC/conf.bak" "$DH/.tmux.conf"
# tmux joins a trailing backslash before expanding, so a wrapped format hides its
# walk from anything reading physical lines.
printf 'set -g pane-border-format "#{?pane_active,,\\\n#{P:#{@panestate} }}"\n' >> "$DH/.tmux.conf"
dcout=$(dc "$DCP")
is "a walk on a continuation line is still a warn row" "1" "$(dcrow "$dcout" "warn formats")"
cp "$DC/conf.bak" "$DH/.tmux.conf"
dcout=$(env -u TMUX HOME="$DH" "$DIR/install.sh" --check --hooks 2>&1); dcrc=$?
is "--check runs alone: with --hooks it exits 2" "2" "$dcrc"
case "$dcout" in
    *"--check runs alone"*"usage: install.sh"*) ok "and says which flag it refused, with usage" ;;
    *) bad "and says which flag it refused, with usage" \
           "--check runs alone ... usage: install.sh" "$dcout" ;;
esac
rm -rf "$DC"
fi

echo "== bootstrap.sh --full: verified before it installs, and the first backup kept =="
# A file:// release tree, a faked box, and a sudo shim that would exit 99 if
# anything reached for it. Nothing here opens a socket, touches the real HOME
# or talks to a tmux server.
if ! command -v curl >/dev/null 2>&1; then skip_ "bootstrap.sh --full (no curl)"
elif ! command -v sha256sum >/dev/null 2>&1; then skip_ "bootstrap.sh --full (no sha256sum)"; else
BS=$(mktmpd)
BH="$BS/home"; BB="$BS/bin"; BB2="$BS/bin2"; BSH="$BS/shim"; REL="$BS/rel"; BSBIN=""
mkdir -p "$BH/.tmux/plugins" "$BB" "$BB2" "$BSH" "$BS/tmuxtmp"
# Every plugin already cloned, every package already present, no ~/.claude and
# no claude on PATH: the packages, plugins and hooks steps all do nothing, so
# what runs is the step under test.
for d in tpm tmux-sensible tmux-yank tmux-resurrect tmux-continuum tmux; do
    mkdir -p "$BH/.tmux/plugins/$d/.git"
done
printf '#!/bin/sh\ntouch "%s"\nexit 99\n' "$BS/SUDO_RAN" > "$BSH/sudo"
printf '#!/bin/sh\necho "0.44.1 (debian)"\n' > "$BSH/fzf"
printf '#!/bin/sh\necho "bat 0.24.0"\n' > "$BSH/batcat"
chmod +x "$BSH/sudo" "$BSH/fzf" "$BSH/batcat"
BPATH="$BSH:$SHIM:/usr/bin:/bin"
printf '# mine\nrun-shell %s/cockpit.tmux\n' "$DIR" > "$BH/.tmux.conf"
bs() {                        # bs <args...> -- bootstrap.sh on the faked box
    env -u TMUX -u TMUX_PANE HOME="$BH" XDG_BIN_HOME="${BSBIN:-$BB}" \
        XDG_CONFIG_HOME="$BH/.config" TMUX_TMPDIR="$BS/tmuxtmp" \
        COCKPIT_GITHUB="file://$REL" PATH="$BPATH" "$DIR/bootstrap.sh" "$@" </dev/null 2>&1
}
# One tarball per published asset name, so the block never repeats the uname
# mapping the script under test owns. The binaries are shell scripts: the only
# thing asked of them is --version, which is what a checksum cannot answer.
mkrel() {                     # mkrel [flip|nosums|noasset]
    local a d f="$REL/junegunn/fzf/releases/download/v0.74.3"
    rm -rf "$REL" "$BS/pack"; mkdir -p "$f" "$BS/pack"
    printf '#!/bin/sh\necho "0.74.3 (15f64c49)"\n' > "$BS/pack/fzf"; chmod +x "$BS/pack/fzf"
    for a in linux_amd64 linux_arm64 darwin_amd64 darwin_arm64; do
        ( cd "$BS/pack" && tar -czf "$f/fzf-0.74.3-$a.tar.gz" fzf )
    done
    ( cd "$f" && sha256sum fzf-0.74.3-*.tar.gz > fzf_0.74.3_checksums.txt )
    d="$REL/sharkdp/bat/releases/download/v0.26.1"; mkdir -p "$d"
    for a in x86_64-unknown-linux-musl aarch64-unknown-linux-musl \
             x86_64-apple-darwin aarch64-apple-darwin; do
        rm -rf "$BS/pack"; mkdir -p "$BS/pack/bat-v0.26.1-$a"
        printf '#!/bin/sh\necho "bat 0.26.1 (a3c4)"\n' > "$BS/pack/bat-v0.26.1-$a/bat"
        chmod +x "$BS/pack/bat-v0.26.1-$a/bat"
        ( cd "$BS/pack" && tar -czf "$d/bat-v0.26.1-$a.tar.gz" "bat-v0.26.1-$a" )
    done
    case "${1:-}" in
        flip) awk '{ c=substr($0,1,1); print (c=="0" ? "1" : "0") substr($0,2) }' \
                  "$f/fzf_0.74.3_checksums.txt" > "$f/s2" &&
              mv "$f/s2" "$f/fzf_0.74.3_checksums.txt" ;;
        nosums)  rm -f "$f/fzf_0.74.3_checksums.txt" "$f"/fzf-0.74.3-*.tar.gz ;;
        noasset) rm -f "$f"/fzf-0.74.3-*.tar.gz ;;
    esac
}
bsclear() { rm -f "$BB/fzf" "$BB/bat"; }
bssaid() {                    # bssaid <label> <literal line the freeze spells>
    case "$3" in
        *"$2"*) ok "$1" ;;
        *) bad "$1" "$2" "$3" ;;
    esac
}

mkrel
bsout=$(bs --full); bsrc=$?
is "--full installs both pinned tools into \$BIN" "fzf bat" \
   "$(for t in fzf bat; do [ -x "$BB/$t" ] && printf '%s ' "$t"; done | sed 's/ $//')"
is "and each is the version the pin names" "0.74.3 (15f64c49)" "$("$BB/fzf" --version 2>/dev/null)"
is "and the run exits 0" "0" "$bsrc"
# The probe is $BIN/<tool>, never PATH: the shimmed 0.44.1 fzf sits ahead of
# $BIN here, and probing PATH would re-download both on every run.
bsout=$(bs --full)
# fetch_tool's own lines: tui-fetch says "already current" for cockpit-tui too.
is "a second run fetches nothing" "2" "$(printf '%s\n' "$bsout" | grep -c '^  ok: .* already current')"

# Without --full the 0.44.1 on PATH is below the pickers' 0.73: fzf alone.
bsclear
bsout=$(bs); bsrc=$?
is "without --full an fzf below 0.73 fetches fzf alone" "fzf" \
   "$(for t in fzf bat; do [ -x "$BB/$t" ] && printf '%s ' "$t"; done | sed 's/ $//')"
is "and the run exits 0" "0" "$bsrc"
bsout=$(BPATH="$BB:$BPATH" bs)
case "$bsout" in *"latest tools"*) bad "an fzf at 0.73+ first on PATH fetches nothing" "no latest tools step" "$bsout" ;;
    *) ok "an fzf at 0.73+ first on PATH fetches nothing" ;; esac

bsclear; mkrel flip
bsout=$(bs --full); bsrc=$?
bssaid "a flipped checksum installs nothing" \
       "FAIL:  fzf checksum mismatch -- NOT installed" "$bsout"
is "and \$BIN/fzf stays absent" "absent" "$([ -e "$BB/fzf" ] && echo present || echo absent)"
is "and the run exits 1" "1" "$bsrc"

# Offline is not a failure: the tools are optional and the run has to end 0.
bsclear; mkrel noasset
bsout=$(bs --full); bsrc=$?
bssaid "an asset it cannot fetch is a skip" "skip:  fzf (download failed -- offline?)" "$bsout"
is "and the run still exits 0" "0" "$bsrc"

# Fail closed the other way round: "could not verify" is not "verified". The
# asset is gone too, so taking it before the sums would say skip, not FAIL.
bsclear; mkrel nosums
bsout=$(bs --full)
bssaid "unreachable checksums refuse the install" "FAIL:  fzf checksums unreachable" "$bsout"

mkrel; BSBIN="$BB2"
bsout=$(bs --full --dry-run); BSBIN=""
bssaid "--dry-run names the fetch" "would: fetch fzf 0.74.3" "$bsout"
is "and creates nothing under \$BIN" "0" "$(ls -A "$BB2" | wc -l)"

bsclear; bsout=$(bs --check --full); is "--check with --full exits 2" "2" "$?"
bssaid "and says which flag it refused" "--check runs alone" "$bsout"
is "and fetches nothing" "absent" "$([ -e "$BB/fzf" ] && echo present || echo absent)"
bsout=$(bs --latest); is "--latest without --full exits 2" "2" "$?"
bssaid "and says which flag it needs" "--latest needs --full" "$bsout"
is "no run reached for sudo" "absent" "$([ -e "$BS/SUDO_RAN" ] && echo present || echo absent)"

# The re-append: the run-shell line was removed, so bootstrap appends again --
# and the pristine backup taken the first time must survive it.
printf '# mine\n' > "$BH/.tmux.conf"; rm -f "$BH/.tmux.conf.bak"
bs >/dev/null 2>&1
is "the first append backs the config up" "# mine" "$(cat "$BH/.tmux.conf.bak" 2>/dev/null)"
grep -v cockpit "$BH/.tmux.conf" > "$BH/c2" && mv "$BH/c2" "$BH/.tmux.conf"
printf '# edited since\n' >> "$BH/.tmux.conf"
bsout=$(bs)
is "a re-append keeps the FIRST backup" "# mine" "$(cat "$BH/.tmux.conf.bak" 2>/dev/null)"
bssaid "and says the backup was kept" "(existing backup kept, not overwritten)" "$bsout"
# tmux loads BOTH config files when both exist, so a box wired through
# ~/.tmux.conf with an unrelated XDG tmux.conf is wired (#96). Before the fix
# bootstrap graded the XDG file alone and appended a second run-shell to it.
mkdir -p "$BH/.config/tmux"; printf '# xdg\n' > "$BH/.config/tmux/tmux.conf"
bsout=$(bs)
bssaid "an unrelated XDG tmux.conf beside a wired ~/.tmux.conf is already wired" "already loads cockpit.tmux" "$bsout"
is "and the XDG file is left alone" "# xdg" "$(cat "$BH/.config/tmux/tmux.conf")"
rm -rf "$BS"
fi

echo "== HOME unset must not kill the path picker =="
# set -u plus a bare $HOME: a "~/..." token on a clicked line aborted the script
# outright wherever HOME is missing -- a stripped run-shell, a systemd tmux.
HU=$(mktmpd); : > "$HU/f.txt"
is "a ~ token with HOME unset still resolves the real file" "$HU/f.txt::" \
   "$(env -u HOME "$DIR/scripts/tmux-open-path" --dry-run "$HU" 'see f.txt and ~/nothing' 2>&1)"
is "and the preview does not crash"     "empty file" \
   "$(env -u HOME "$DIR/scripts/tmux-open-path" --preview "$HU/f.txt" 2>&1)"
rm -rf "$HU"

echo "== notify's deadline covers the whole read, not each line =="
# A per-line timeout is not a bound: a writer emitting a line a second keeps the
# hook alive forever, and the contract is that a hook never blocks its agent.
# Graded by the writer's progress, not a clock: once notify exits the pipe has
# no reader, so the next printf dies of SIGPIPE, or fails where that signal is
# ignored, and the drip stops short of 8 (#135).
DL=$(mktmpf)
( for i in 1 2 3 4 5 6 7 8; do printf 'drip %s\n' "$i" || break; echo "$i" >> "$DL"; sleep 1; done ) \
    | "$DIR/scripts/agent-state" notify %999999 >/dev/null 2>&1
rc=${PIPESTATUS[1]}; n=$(wc -l < "$DL")
if [ "$rc" = 0 ] && [ "$n" -lt 8 ]; then ok "a slow drip does not hold the hook open"
else bad "a slow drip does not hold the hook open" "rc 0, fewer than 8 lines drained" "rc $rc, $n lines"; fi

echo "== the branch picker must not hijack ctrl-n =="
# ctrl-n is fzf's own move-down. As an --expect key it instead EXITS fzf, so
# navigating with emacs keys created a branch named by the filter, or tripped
# "type a name before ^N" and closed the popup. Driven through a real pty,
# because this only misbehaves with a terminal attached.
if need_tmux "ctrl-n in the branch picker"; then :; else
if command -v fzf >/dev/null 2>&1; then
KB=$(mktmpd)
(
  cd "$KB" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; : > a; git add a
  c=$(git commit-tree "$(git write-tree)" -m init); git update-ref refs/heads/main "$c"
  git symbolic-ref HEAD refs/heads/main; git reset -q --hard
  git branch second; git branch third
) >/dev/null 2>&1
SOCK5="cockpit-keys-$$"; SOCKS="$SOCKS $SOCK5"
tmux -L "$SOCK5" -f /dev/null new-session -d -s t -x 90 -y 24 -c "$KB" >/dev/null 2>&1
KP=$(tmux -L "$SOCK5" list-panes -a -F '#{pane_id}' | head -1)
# The trailing marker is how the picker's EXIT gets an observable:
# pane_current_command reads "bash" for the script the whole time fzf is up
# (measured), so only the finished command's own output separates them.
tmux -L "$SOCK5" send-keys -t "$KP" "'$DIR/scripts/tmux-branch' '$KB'; echo BRC1=\$?" Enter
# The list marks the branch you are ON; the header alone said it before, and
# the current branch looked like a normal destination.
until_cap "$SOCK5" "$KP" '*● main*'
case "$(tmux -L "$SOCK5" capture-pane -p -t "$KP")" in
    *"● main"*) ok "the current branch is marked in the list" ;;
    *) bad "the current branch is marked in the list" "● main" \
         "$(tmux -L "$SOCK5" capture-pane -p -t "$KP" | sed -n 4p)" ;;
esac
# Type a filter FIRST. With an empty query the old binding only errored, so a
# bare ctrl-n proves nothing -- it is the typed query that used to become a
# branch name. "zzz" matches no branch, exactly the case the create key exists
# for and exactly what a user filtering the list would trip over.
tmux -L "$SOCK5" send-keys -t "$KP" "zzz"
until_cap "$SOCK5" "$KP" '*switch> zzz*'
# fzf reads its keys in order, so it can only exit after it has handled the
# ctrl-n: the picker's own exit is the proof that the create never fired.
tmux -L "$SOCK5" send-keys -t "$KP" C-n Escape   # move-down, NOT create; then leave
until_cap "$SOCK5" "$KP" '*BRC1=[0-9]*'
after=$(git -C "$KB" for-each-ref --format='%(refname:short)' refs/heads | sort | tr '\n' ' ')
is "ctrl-n after typing creates no branch" "main second third " "$after"
is "and the checkout is untouched" "main" "$(git -C "$KB" branch --show-current)"
# A branch held by a linked worktree is labelled, and a FAILED action returns
# to the picker instead of dying at "[any key]". The failure driven here is the
# deterministic one -- an invalid name via alt-enter -- because driving a git
# refusal through a pty raced fzf's own startup and the test keyed the wrong
# row. Waiting for the prompt before typing fixes the label half too.
git -C "$KB" worktree add -q "$KB-wt" second 2>/dev/null
tmux -L "$SOCK5" send-keys -t "$KP" "'$DIR/scripts/tmux-branch' '$KB'; echo BRC2=\$?" Enter
picker_up=0
for i in 1 2 3 4 5 6 7 8 9 10; do
    case "$(tmux -L "$SOCK5" capture-pane -p -t "$KP")" in
        *"switch>"*) picker_up=1; break ;;
    esac
    sleep 0.5
done
case "$(tmux -L "$SOCK5" capture-pane -p -t "$KP")" in
    *"⑂"*) ok "a worktree-held branch is labelled" ;;
    *) bad "a worktree-held branch is labelled" "a ⑂ mark" \
         "$(tmux -L "$SOCK5" capture-pane -p -t "$KP" | sed -n 4,6p)" ;;
esac
if [ "$picker_up" = 1 ]; then
    tmux -L "$SOCK5" send-keys -t "$KP" "bad..name"
    until_cap "$SOCK5" "$KP" '*switch> bad..name*'
    tmux -L "$SOCK5" send-keys -t "$KP" M-Enter                # create fails, pauses
    until_cap "$SOCK5" "$KP" '*any key*'
    tmux -L "$SOCK5" send-keys -t "$KP" Space                  # [any key] ...
    back=0
    for i in 1 2 3 4 5 6 7 8 9 10; do
        sleep 0.5
        case "$(tmux -L "$SOCK5" capture-pane -p -t "$KP")" in
            *"switch>"*) back=1; break ;;
        esac
    done
    if [ "$back" = 1 ]; then ok "a failed action returns to the picker"
    else bad "a failed action returns to the picker" "the picker again" \
             "$(tmux -L "$SOCK5" capture-pane -p -t "$KP" | head -3)"; fi
    # The picker exiting is what proves the failed create is finished, and the
    # assertion below is that it checked nothing out.
    tmux -L "$SOCK5" send-keys -t "$KP" Escape
    until_cap "$SOCK5" "$KP" '*BRC2=[0-9]*'
else
    bad "a failed action returns to the picker" "a visible picker" "never appeared"
fi
is "and the failure checked nothing out" "main" "$(git -C "$KB" branch --show-current)"
git -C "$KB" worktree remove --force "$KB-wt" >/dev/null 2>&1
tmux -L "$SOCK5" kill-server >/dev/null 2>&1
rm -rf "$KB" "$KB-wt"
else
  skip_ "ctrl-n creates no branch"; skip_ "and the checkout is untouched"
fi
fi

echo "== the path picker fires in a window with a split =="
# tmux's own C-MouseDown1Pane is swap-pane, and it fires on the FIRST press of a
# Ctrl+double-click. Swapping moves the second click to another pane, so tmux
# never accumulates the double-click and the binding never runs. With one pane
# there is nothing to swap, which is why this looked fine by hand -- and why the
# test has to use TWO panes or it proves nothing.
#
# Real mouse events, not a simulated binding: an outer tmux server provides the
# pty for a client attached to the inner one, and send-keys -H writes the raw
# SGR bytes (ESC [ < 16 ; 3 ; 2 M/m -- button 0 plus the ctrl bit) into it.
if need_tmux "path picker with a split"; then :; else
OUTER="cockpit-outer-$$"; INNER="cockpit-inner-$$"
SOCKS="$SOCKS $OUTER $INNER"
# The clicked row is printed by the pane's own command, not typed at a shell:
# a real prompt can win the echo race and, from a long cwd, truncates the row
# so README.md never reaches column 3 (#75).
"$TMUX_REAL" -L "$INNER" -f /dev/null new-session -d -s t -x 80 -y 24 \
    'printf "MARK README.md\n"; exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" set -g mouse on >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" split-window -t t >/dev/null 2>&1
IP=$("$TMUX_REAL" -L "$INNER" list-panes -t t -F '#{pane_id}' | head -1)
until_cap "$INNER" "$IP" '*MARK README.md*'
"$TMUX_REAL" -L "$INNER" set-option -p -t "$IP" @cockpit_click_line "" >/dev/null 2>&1
"$TMUX_REAL" -L "$OUTER" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $INNER attach -t t" >/dev/null 2>&1
OP=$("$TMUX_REAL" -L "$OUTER" list-panes -t o -F '#{pane_id}' | head -1)
# The outer pane shows the inner session, so the row being there is the proof
# the client attached AND drew before any mouse byte goes in.
until_cap "$OUTER" "$OP" '*MARK README.md*'
# Row 2: row 1 is the top border the plugin draws.
CDBL='1b 5b 3c 31 36 3b 33 3b 32 4d 1b 5b 3c 31 36 3b 33 3b 32 6d'
# The client has to be up, or the keys go nowhere and the assertion passes for
# the wrong reason -- twice today a test of mine did exactly that.
if "$TMUX_REAL" -L "$OUTER" list-panes -t o >/dev/null 2>&1; then
    # One send-keys, not two: the gap between two calls is wall time on a
    # shared box, and tmux stops reading a double click past ~300ms.
    "$TMUX_REAL" -L "$OUTER" send-keys -t "$OP" -H $CDBL $CDBL >/dev/null 2>&1
    for i in $(seq 1 40); do
        got=$("$TMUX_REAL" -L "$INNER" show-options -p -t "$IP" -qv @cockpit_click_line)
        case "$got" in *README.md*) break ;; esac
        sleep 0.1
    done
    case "$got" in
        *README.md*) ok "ctrl+double-click reaches the picker with 2 panes" ;;
        *) bad "ctrl+double-click reaches the picker with 2 panes" "the clicked line" "$got" ;;
    esac
else
    bad "ctrl+double-click reaches the picker with 2 panes" "an attached client" "no client"
fi
"$TMUX_REAL" -L "$OUTER" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" kill-server >/dev/null 2>&1
fi

echo "== the click bindings survive a plugin path with a quote and a space =="
# The bindings carry the script path shell-quoted, and inside a tmux "..." string
# tmux eats the backslash of '\'' so the shell got ''' instead.
if need_tmux "quoted plugin path"; then :; else
QO="cockpit-qpo-$$"; QI="cockpit-qpi-$$"; SOCKS="$SOCKS $QO $QI"
QPD="$(mktmpd)/it's a dir"; mkdir -p "$QPD"; QLOG=$(mktmpf)
cp -R "$DIR/cockpit.tmux" "$DIR/scripts" "$QPD/"
printf '#!/bin/sh\nprintf "OPEN %%s\\n" "$*" > "%s"\n' "$QLOG" > "$QPD/scripts/tmux-open-path"
"$TMUX_REAL" -L "$QI" -f /dev/null new-session -d -s t -x 80 -y 24 'printf "MARK\n"; exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$QI" set -g mouse on >/dev/null 2>&1
"$TMUX_REAL" -L "$QI" run-shell "\"$QPD/cockpit.tmux\"" >/dev/null 2>&1
QIP=$("$TMUX_REAL" -L "$QI" list-panes -t t -F '#{pane_id}')
"$TMUX_REAL" -L "$QO" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $QI attach -t t" >/dev/null 2>&1
until_cap "$QO" o '*MARK*'
# SGR on the MARK row, row 2 under the top border: ctrl is +16 and meta +8 on the button, so ctrl+right is 18.
# The double click is one send-keys: tmux drops a pair more than ~300ms apart.
for qg in "ctrl+right-click:1b 5b 3c 31 38 3b 32 3b 32 4d" \
          "alt+click:1b 5b 3c 38 3b 32 3b 32 4d 1b 5b 3c 38 3b 32 3b 32 6d" \
          "ctrl+double-click:1b 5b 3c 31 36 3b 32 3b 32 4d 1b 5b 3c 31 36 3b 32 3b 32 6d 1b 5b 3c 31 36 3b 32 3b 32 4d 1b 5b 3c 31 36 3b 32 3b 32 6d"; do
    : > "$QLOG"
    # shellcheck disable=SC2086  # the hex bytes are meant to split
    "$TMUX_REAL" -L "$QO" send-keys -t o -H ${qg#*:} >/dev/null 2>&1
    until_file "$QLOG" '*OPEN*'
    is "${qg%%:*} runs the picker from that path" "OPEN $QIP" "$(cat "$QLOG")"
done
"$TMUX_REAL" -L "$QO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$QI" kill-server >/dev/null 2>&1
fi

# tests: examples/tmux.conf cockpit.tmux
echo "== a left click on a window tab selects it and draws nothing (#21) =="
# Issue #21 saw a menu flash on a plain left click in the status line. Stock
# MouseDown1Status is switch-client -t = and draws nothing; the menus that do
# close on release are all button 3. Pinned with a real click: the SGR press
# is written into an attached client and the screen is read while the button
# is still held, which is the only moment a release-closed menu is visible.
if need_tmux "status-line left click"; then :; else
OUTER="cockpit-clk-outer-$$"; INNER="cockpit-clk-inner-$$"
SOCKS="$SOCKS $OUTER $INNER"
# Both windows run a silent sleep under the same border format, so the pane
# area renders the same in either: anything that differs while the button is
# held is an overlay, whatever its text.
"$TMUX_REAL" -L "$INNER" -f /dev/null new-session -d -s t -x 80 -y 24 -n zero \
    'exec -a claude sleep 300' >/dev/null 2>&1
# Not touch: its ‹ would sit where the popup below is armed.
"$TMUX_REAL" -L "$INNER" set -g @cockpit_touch off >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
# The shipped config loads the INSTALLED plugin; this tree's bindings only
# get on the server through an explicit run-shell, and the check proves it.
"$TMUX_REAL" -L "$INNER" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$INNER" list-keys -T cockpit)" in
    *"$DIR/scripts/tmux-open-path"*) ok "this tree's cockpit.tmux is the one bound" ;;
    *) bad "this tree's cockpit.tmux is the one bound" "$DIR/scripts/tmux-open-path in the cockpit keys" "absent" ;;
esac
# The bar has no status-left; the session name gives the popup below a target.
"$TMUX_REAL" -L "$INNER" set -g status-left '#S ' >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" new-window -d -t t -n TARGET 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$OUTER" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $INNER attach -t t" >/dev/null 2>&1
OP=$("$TMUX_REAL" -L "$OUTER" list-panes -t o -F '#{pane_id}' | head -1)
# Wait for the tab AND the border's ≡: the baseline below covers the border
# row, so it must be painted before the snapshot or the compare races.
if until_cap "$OUTER" "$OP" '*TARGET*' && until_cap "$OUTER" "$OP" '*≡*'; then
    # The tab's cell: the inner status line is the last line of the outer pane.
    # Column counting assumes the default ASCII window-status-format; the
    # status row itself is excluded from the compare (its highlight moves), so
    # a flash confined to that row is not covered here.
    scr=$("$TMUX_REAL" -L "$OUTER" capture-pane -p -t "$OP")
    row=$(printf '%s\n' "$scr" | grep -n TARGET | tail -1 | cut -d: -f1)
    line=$(printf '%s\n' "$scr" | sed -n "${row}p"); pre=${line%%TARGET*}; col=$((${#pre}+1))
    before=$(printf '%s\n' "$scr" | head -n "$((row-1))")
    press=$(printf '\033[<0;%s;%sM' "$col" "$row" | od -An -tx1 | tr -s ' \n' ' ')
    release=$(printf '\033[<0;%s;%sm' "$col" "$row" | od -An -tx1 | tr -s ' \n' ' ')
    # shellcheck disable=SC2086  # the hex bytes are meant to split
    "$TMUX_REAL" -L "$OUTER" send-keys -t "$OP" -H $press >/dev/null 2>&1
    for i in $(seq 1 40); do
        [ "$("$TMUX_REAL" -L "$INNER" display-message -p -t t '#{window_index}')" = 1 ] && break
        sleep 0.1
    done
    is "the click selects the window" "1" \
       "$("$TMUX_REAL" -L "$INNER" display-message -p -t t '#{window_index}')"
    held=$("$TMUX_REAL" -L "$OUTER" capture-pane -p -t "$OP" | head -n "$((row-1))")
    if [ "$held" = "$before" ]; then ok "and draws nothing while the button is held"
    else bad "and draws nothing while the button is held" "the pane area unchanged" \
             "$(diff <(printf '%s\n' "$before") <(printf '%s\n' "$held") | grep '^> *[^ ]' | head -3)"; fi
    # shellcheck disable=SC2086
    "$TMUX_REAL" -L "$OUTER" send-keys -t "$OP" -H $release >/dev/null 2>&1
    # #21's real shape: a press that opens a popup, with a one-cell pointer
    # twitch in the same read, sets tmux's drag flag before the popup exists;
    # the popup eats the release, and the NEXT press anywhere arrives as
    # MouseDragEnd1Status. The plugin binds that key to the same switch-client,
    # so the lost press still selects the tab. Armed here on the session name.
    "$TMUX_REAL" -L "$INNER" bind -n MouseDown1StatusLeft display-popup -E 'exec sleep 300' >/dev/null 2>&1
    twitch=$(printf '\033[<0;2;%sM\033[<32;3;%sM' "$row" "$row" | od -An -tx1 | tr -s ' \n' ' ')
    drop=$(printf '\033[<0;3;%sm' "$row" | od -An -tx1 | tr -s ' \n' ' ')
    # shellcheck disable=SC2086
    "$TMUX_REAL" -L "$OUTER" send-keys -t "$OP" -H $twitch >/dev/null 2>&1
    if until_cap "$OUTER" "$OP" '*│*'; then      # the popup is up: its border is the only box drawn
        # shellcheck disable=SC2086
        "$TMUX_REAL" -L "$OUTER" send-keys -t "$OP" -H $drop >/dev/null 2>&1
        ic=$("$TMUX_REAL" -L "$INNER" list-clients -F '#{client_name}' | head -1)
        "$TMUX_REAL" -L "$INNER" display-popup -C -c "$ic" >/dev/null 2>&1
        for i in $(seq 1 40); do
            case "$("$TMUX_REAL" -L "$OUTER" capture-pane -p -t "$OP")" in *│*) sleep 0.1 ;; *) break ;; esac
        done
        pre=${line%%zero*}; zcol=$((${#pre}+1))
        zpress=$(printf '\033[<0;%s;%sM' "$zcol" "$row" | od -An -tx1 | tr -s ' \n' ' ')
        zrelease=$(printf '\033[<0;%s;%sm' "$zcol" "$row" | od -An -tx1 | tr -s ' \n' ' ')
        # shellcheck disable=SC2086
        "$TMUX_REAL" -L "$OUTER" send-keys -t "$OP" -H $zpress >/dev/null 2>&1
        for i in $(seq 1 40); do
            [ "$("$TMUX_REAL" -L "$INNER" display-message -p -t t '#{window_index}')" = 0 ] && break
            sleep 0.1
        done
        is "a click after a popup ate a release still selects the window" "0" \
           "$("$TMUX_REAL" -L "$INNER" display-message -p -t t '#{window_index}')"
        # shellcheck disable=SC2086
        "$TMUX_REAL" -L "$OUTER" send-keys -t "$OP" -H $zrelease >/dev/null 2>&1
    else
        bad "a click after a popup ate a release still selects the window" "a popup on the client" "none drawn"
    fi
else
    bad "the click selects the window" "an attached client" "no client"
    bad "and draws nothing while the button is held" "an attached client" "no client"
    bad "a click after a popup ate a release still selects the window" "an attached client" "no client"
fi
"$TMUX_REAL" -L "$OUTER" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" kill-server >/dev/null 2>&1
fi

echo "== the fixes review found =="
# agent-state joined payload lines with NO separator, so the tail of one line
# and the head of the next could fuse into a marker that appears in neither.
# (A raw newline INSIDE a JSON string is invalid JSON and cannot reach us, so
# the reverse -- a wrapped message failing to match -- is not a real case.)
if need_tmux "notify line fusion"; then :; else
st running "$PANE"
printf '{\n  "a": "Claude needs your ",\n  "b": "permission to use Bash"\n}\n' | st notify "$PANE"
is "adjacent lines do not fuse into a false match" "running" "$(sv)"
st running "$PANE"
printf '{"message":"Claude needs your permission to use Bash"}\n' | st notify "$PANE"
is "and the real single-line payload still matches" "needs-input" "$(sv)"
st off "$PANE"
fi

# `read` strips spaces and tabs but never \r, so a repo checked out on Windows
# put a raw carriage return into the status line. git rev-parse never did.
CR=$(mktmpd); git init -q "$CR" >/dev/null 2>&1
printf 'ref: refs/heads/main\r\n' > "$CR/.git/HEAD"
is "a CRLF .git/HEAD does not leak a carriage return" " main" \
   "$("$DIR/scripts/tmux-where" branch 1 "$CR" claude)"
rm -rf "$CR"

# printf '%.24s' counts BYTES, so a non-ASCII branch name was cut mid-codepoint
# and invalid UTF-8 reached the status bar.
U8=$(mktmpd); git init -q "$U8" >/dev/null 2>&1
git -C "$U8" -c user.email=t@e -c user.name=t commit -q --allow-empty -m i >/dev/null 2>&1
if git -C "$U8" checkout -q -b "feature/日本語のブランチ名ですとても長い" 2>/dev/null; then
    u=$("$DIR/scripts/tmux-where" branch 1 "$U8" claude)
    if printf '%s' "$u" | iconv -f utf-8 -t utf-8 >/dev/null 2>&1
    then ok "a long unicode branch stays valid UTF-8"
    else bad "a long unicode branch stays valid UTF-8" "valid UTF-8" "$u"; fi
else
    skip_ "a long unicode branch stays valid UTF-8"
fi
rm -rf "$U8"

# tmux-where and tmux-open-path were guarded; tmux-branch was the last one left
# dereferencing a bare $HOME under set -u.
HB=$(mktmpd); git init -q "$HB" >/dev/null 2>&1
git -C "$HB" -c user.email=t@e -c user.name=t commit -q --allow-empty -m i >/dev/null 2>&1
case "$(env -u HOME -u TMUX timeout 5 "$DIR/scripts/tmux-branch" "$HB" </dev/null 2>&1)" in
    *"unbound variable"*) bad "tmux-branch survives HOME unset" "no crash" "HOME: unbound variable" ;;
    *) ok "tmux-branch survives HOME unset" ;;
esac
rm -rf "$HB"

# install.sh printed "linked ..." for every link ln had just failed to create,
# and exited 0.
RO=$(mktmpd); chmod 555 "$RO"
# HOME="$RO" too: install.sh also links the cockpit-review skill into
# ~/.claude/skills, and with the real HOME that one link SUCCEEDED -- printing
# "linked" in a block that asserts none, and leaving a link in the user's own
# skills dir. $RO holds no .claude, so the skill step is skipped here.
out=$(HOME="$RO" XDG_BIN_HOME="$RO" "$DIR/install.sh" 2>&1); rc=$?
chmod 755 "$RO"; rm -rf "$RO"
case "$out" in
    *linked*) bad "a failed link is not reported as linked" "no 'linked' line" "$out" ;;
    *) ok "a failed link is not reported as linked" ;;
esac
if [ "$rc" -ne 0 ]; then ok "and the installer exits non-zero"
else bad "and the installer exits non-zero" "non-zero" "$rc"; fi
# A typo used to install silently.
"$DIR/install.sh" --uninstal >/dev/null 2>&1
is "an unknown option is rejected" "2" "$?"

# interpolate() kept a bare $DIR after sq() fixed the bindings, so a plugin path
# containing a space made both status tokens run a command sh could not find --
# silently, on every redraw, forever.
SPC=$(mktmpd)/"my plugins"; mkdir -p "$SPC"
cp -r "$DIR/scripts" "$SPC/" 2>/dev/null
sh -c "'$SPC/scripts/tmux-where' branch 1 '$DIR' claude" >/dev/null 2>&1
is "a spaced plugin path still runs the status script" "0" "$?"
rm -rf "$(dirname "$SPC")"

echo "== the picker fires even where the app owns the mouse =="
# A guard that yielded to any application with mouse reporting on was tried --
# and it made the picker dead in every Claude pane, because agent TUIs keep
# mouse reporting on. That is the primary place the gesture exists for, and it
# was reported broken within a day. Plain gestures still reach the app (we
# never bind those); the two CHORDED gestures are a deliberate invocation and
# must win.
if need_tmux "mouse-mode pane"; then :; else
GO="cockpit-go-$$"; GI="cockpit-gi-$$"; SOCKS="$SOCKS $GO $GI"
# No prompt in the pane, and mouse reporting on exactly as Claude Code does it.
# An interactive shell prints the cwd, and a long checkout path wraps that
# prompt, pushing MARK off the row the click below aims at.
"$TMUX_REAL" -L "$GI" -f /dev/null new-session -d -s t -x 80 -y 24 \
    "printf 'MARK README.md\\n\\033[?1000h'; sleep 300" >/dev/null 2>&1
"$TMUX_REAL" -L "$GI" set -g mouse on >/dev/null 2>&1
"$TMUX_REAL" -L "$GI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
GP=$("$TMUX_REAL" -L "$GI" list-panes -t t -F '#{pane_id}' | head -1)
until_out 1 "$TMUX_REAL" -L "$GI" display-message -p -t "$GP" '#{mouse_any_flag}'
is "the pane really reports mouse mode" "1" \
   "$("$TMUX_REAL" -L "$GI" display-message -p -t "$GP" '#{mouse_any_flag}')"
"$TMUX_REAL" -L "$GI" set-option -p -t "$GP" @cockpit_click_line "" >/dev/null 2>&1
"$TMUX_REAL" -L "$GO" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $GI attach -t t" >/dev/null 2>&1
GOP=$("$TMUX_REAL" -L "$GO" list-panes -t o -F '#{pane_id}' | head -1)
# The clicked row has to be drawn in the outer pane before any mouse byte goes
# in, or the click lands on a blank screen and reads back empty.
until_cap "$GO" "$GOP" '*MARK README.md*'
# Row 2: row 1 is the top border the plugin draws.
GSEQ='1b 5b 3c 31 36 3b 33 3b 32 4d 1b 5b 3c 31 36 3b 33 3b 32 6d'
# One send-keys, not two: the gap between two calls is wall time on a shared
# box, and tmux stops reading a double click past ~300ms.
"$TMUX_REAL" -L "$GO" send-keys -t "$GOP" -H $GSEQ $GSEQ >/dev/null 2>&1
for i in $(seq 1 40); do
    ggot=$("$TMUX_REAL" -L "$GI" show-options -p -t "$GP" -qv @cockpit_click_line)
    case "$ggot" in *"MARK README.md"*) break ;; esac
    sleep 0.1
done
case "$ggot" in
    *"MARK README.md"*) ok "ctrl+double-click fires in a mouse-mode pane" ;;
    *) bad "ctrl+double-click fires in a mouse-mode pane" "the clicked line" "$ggot" ;;
esac
"$TMUX_REAL" -L "$GO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$GI" kill-server >/dev/null 2>&1
fi

echo "== the border carries [z] on the active pane of a split, and no [x] =="
# Replacing pane-border-format wholesale deleted tmux's zoom button (control|8).
# The redesign keeps it, on the active pane of a split only, and moves kill to
# the menu, behind a confirm: no control|9 on any border.
if need_tmux "border buttons"; then :; else
BB="cockpit-bb-$$"; SOCKS="$SOCKS $BB"
"$TMUX_REAL" -L "$BB" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$BB" set -g mouse on >/dev/null 2>&1
"$TMUX_REAL" -L "$BB" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
# One pane: no [z] -- a lone pane already fills the window, so the zoom button
# toggled nothing and read as broken.
solo=$("$TMUX_REAL" -L "$BB" display-message -p -t t '#{E:pane-border-format}')
case "$solo" in
    *'control|8'*|*'control|9'*) bad "no button on a lone pane" "neither control|8 nor 9" "$solo" ;;
    *) ok "no button on a lone pane" ;;
esac
"$TMUX_REAL" -L "$BB" split-window -t t 'exec -a claude sleep 300' >/dev/null 2>&1
BP2=$("$TMUX_REAL" -L "$BB" list-panes -t t -F '#{pane_id} #{pane_active}' | awk '$2==1{print $1}')
BP1=$("$TMUX_REAL" -L "$BB" list-panes -t t -F '#{pane_id} #{pane_active}' | awk '$2==0{print $1}')
"$TMUX_REAL" -L "$BB" set -p -t "$BP1" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$BB" set -p -t "$BP1" @panestate_msg "Allow Bash?" >/dev/null 2>&1
bb=$("$TMUX_REAL" -L "$BB" display-message -p -t "$BP2" '#{E:pane-border-format}')
case "$bb" in *'range=control|8'*) ok "the active pane of a split has [z]" ;; *) bad "the active pane of a split has [z]" "range=control|8" "$bb" ;; esac
case "$bb" in *'range=control|9'*) bad "and no [x]" "no range=control|9" "$bb" ;; *) ok "and no [x]" ;; esac
nb=$("$TMUX_REAL" -L "$BB" display-message -p -t "$BP1" '#{E:pane-border-format}')
case "$nb" in *'range=control|8'*) bad "the other pane has no buttons" "no range=control|8" "$nb" ;; *) ok "the other pane has no buttons" ;; esac
case "$(printf '%s' "$nb" | plain)" in
    *"▲ Allow Bash?"*) ok "a waiting pane spells out its question" ;;
    *) bad "a waiting pane spells out its question" "▲ Allow Bash?" "$(printf '%s' "$nb" | plain)" ;;
esac
"$TMUX_REAL" -L "$BB" kill-server >/dev/null 2>&1
fi

echo "== the bar's taps act on release: ≡ opens the menu, the name the picker, ‹ steps back (#222) =="
# A menu opened on the press is closed by that press's own release, so every
# cockpit tap is a MouseUp binding. Real clicks into an attached 56-column
# client (touch, auto): the corner reads "──  ≡  ─  <name> ...", so ≡ is
# column 5 and the name starts at column 11; ‹ is column 2 of the window line.
if need_tmux "bar taps"; then :; else
TI="cockpit-tapi-$$"; TO="cockpit-tapo-$$"; SOCKS="$SOCKS $TI $TO"
"$TMUX_REAL" -L "$TI" -f /dev/null new-session -d -s cockpit -x 56 -y 30 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TI" set -g mouse on >/dev/null 2>&1
"$TMUX_REAL" -L "$TI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
for _ in 1 2; do "$TMUX_REAL" -L "$TI" new-window -d -t cockpit: 'exec sleep 300' >/dev/null 2>&1; done
"$TMUX_REAL" -L "$TO" -f /dev/null new-session -d -s o -x 56 -y 30 \
    "TERM=xterm-256color exec $TMUX_REAL -L $TI attach -t cockpit" >/dev/null 2>&1
until_cap "$TO" o '*≡*cockpit*'
tap() {                       # <col> <row>: a press, then its release
    # shellcheck disable=SC2046  # the hex bytes are meant to split
    "$TMUX_REAL" -L "$TO" send-keys -t o -H $(printf '\033[<0;%s;%sM\033[<0;%s;%sm' "$1" "$2" "$1" "$2" | od -An -tx1)
}
tap 5 1
if until_cap "$TO" o '*New window*'; then ok "tapping ≡ opens the menu"
else bad "tapping ≡ opens the menu" "the menu's New window row" "$("$TMUX_REAL" -L "$TO" capture-pane -p -t o | head -3)"; fi
"$TMUX_REAL" -L "$TO" send-keys -t o Escape
until_cap "$TO" o '*≡*cockpit*'
"$TMUX_REAL" -L "$TI" select-window -t cockpit:2 >/dev/null 2>&1
tap 2 30
until_out 1 "$TMUX_REAL" -L "$TI" display-message -p -t cockpit: '#{window_index}' >/dev/null
is "tapping ‹ steps to the previous window" "1" \
   "$("$TMUX_REAL" -L "$TI" display-message -p -t cockpit: '#{window_index}')"
if command -v fzf >/dev/null 2>&1; then
    tap 12 1
    # tm marks your own session ‹here› below 60 columns.
    if until_cap "$TO" o '*session>*‹here›*'; then ok "tapping the session name opens the session picker"
    else bad "tapping the session name opens the session picker" "tm's prompt and the ‹here› row" "$("$TMUX_REAL" -L "$TO" capture-pane -p -t o | head -5)"; fi
else
    skip_ "tapping the session name opens the session picker (no fzf)"
fi
"$TMUX_REAL" -L "$TO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TI" kill-server >/dev/null 2>&1
fi

echo "== @open-editor still works under its new name =="
# Renamed to @cockpit_editor. The old name is unprefixed and generic enough that
# another plugin could reasonably claim it, but people have it in their configs,
# so it keeps working for a release and says so.
RN=$(mktmpd); RT=$(mktmpd); : > "$RN/f.txt"
cat > "$RT/tmux" <<'RSTUB'
#!/usr/bin/env bash
case "$1" in
  show-options)
      case " $* " in
          *@cockpit_click_line*) printf '%s\n' "${CLICK_LINE:-}" ;;
          *@cockpit_editor*)     printf '%s\n' "${NEW_ED:-}" ;;
          *@open-editor*)        printf '%s\n' "${OLD_ED:-}" ;;
      esac ;;
  display-message)
      case " $* " in
          *pane_current_path*) printf '%s\n' "${CLICK_DIR:-}" ;;
          *window_id*)         printf '@0\n' ;;
          *)                   printf 'MSG %s\n' "$*" >> "$TMUX_LOG" ;;
      esac ;;
  new-window) printf 'NEWWIN %s\n' "${!#}" >> "$TMUX_LOG" ;;
esac
exit 0
RSTUB
chmod +x "$RT/tmux"; ln -s "$(command -v bash)" "$RT/bash"
ed_run() { : > "$RT/log"
    env PATH="$RT" TMUX_LOG="$RT/log" CLICK_DIR="$RN" CLICK_LINE="f.txt" "$@" \
        "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
    cat "$RT/log"; }
case "$(ed_run NEW_ED=nvim)" in
    *"NEWWIN nvim"*) ok "@cockpit_editor is used" ;;
    *) bad "@cockpit_editor is used" "a window running nvim" "$(ed_run NEW_ED=nvim)" ;;
esac
out=$(ed_run OLD_ED=vi)
case "$out" in *"NEWWIN vi"*) ok "the old @open-editor still works" ;;
               *) bad "the old @open-editor still works" "a window running vi" "$out" ;; esac
case "$out" in *"renamed @cockpit_editor"*) ok "and says it has been renamed" ;;
               *) bad "and says it has been renamed" "a rename notice" "$out" ;; esac
rm -rf "$RN" "$RT"

echo "== prefix s opens the session picker, prefix u jumps (#43) =="
# Lowercase keys: a Shift chord dies under Caps Lock or a non-Latin layout.
if need_tmux "prefix s / u"; then :; else
PS="cockpit-ps-$$"; SOCKS="$SOCKS $PS"
pk() { "$TMUX_REAL" -L "$PS" list-keys -T prefix 2>/dev/null | awk -v k="$1" '$4==k'; }
"$TMUX_REAL" -L "$PS" -f /dev/null new-session -d -s t >/dev/null 2>&1
# No sleep after any run-shell in this file: run-shell without -b blocks until
# the script exits, and cockpit.tmux's own source-file lands with it (probed
# 10/10 -- the binding is queryable the instant run-shell returns).
"$TMUX_REAL" -L "$PS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
# tm --popup opens the popup itself, sized to the sessions, on the pressing
# client and in its session.
case "$(pk s)" in
    *"run-shell -b"*"/tm' --popup '#{client_name}' '#{session_id}' '#{client_width}' '#{client_height}'"*)
        ok "prefix s is bound to the sessions popup" ;;
    *) bad "prefix s is bound to the sessions popup" "run-shell -b tm --popup <client> <session> <w> <h>" "$(pk s)" ;;
esac
case "$(pk g)" in *--popup*) bad "and g is left to tmux" "no sessions popup on g" "$(pk g)" ;;
    *) ok "and g is left to tmux" ;; esac
is "and S is no longer taken" "" "$(pk S)"
case "$(pk u)" in
    *tmux-jump*) ok "prefix u is bound to jump" ;;
    *) bad "prefix u is bound to jump" "run-shell tmux-jump" "$(pk u)" ;;
esac
is "and J is no longer taken" "" "$(pk J)"
"$TMUX_REAL" -L "$PS" set -g @cockpit_sessions_key S >/dev/null 2>&1
"$TMUX_REAL" -L "$PS" set -g @cockpit_jump_key J >/dev/null 2>&1
"$TMUX_REAL" -L "$PS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(pk S)" in *--popup*) ok "@cockpit_sessions_key S brings the old key back" ;;
    *) bad "@cockpit_sessions_key S brings the old key back" "the popup on S" "$(pk S)" ;; esac
case "$(pk J)" in *tmux-jump*) ok "@cockpit_jump_key J brings the old key back" ;;
    *) bad "@cockpit_jump_key J brings the old key back" "tmux-jump on J" "$(pk J)" ;; esac
"$TMUX_REAL" -L "$PS" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$PS" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$PS" set -g @cockpit_sessions_key off >/dev/null 2>&1
ps_stock=$(pk s)
"$TMUX_REAL" -L "$PS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "@cockpit_sessions_key off keeps stock prefix s" "$ps_stock" "$(pk s)"
"$TMUX_REAL" -L "$PS" kill-server >/dev/null 2>&1
fi

echo "== prefix a is the menu (#43, #222) =="
# One menu, every cockpit action, each row replaying the key it is bound to
# (send-keys -K, tmux 3.4) so a rebound key moves the row with it and an
# `off` key drops its row. Stock a is unbound, so off leaves nothing.
if need_tmux "prefix a"; then :; else
PA="cockpit-pa-$$"; SOCKS="$SOCKS $PA"
ak() { "$TMUX_REAL" -L "$PA" list-keys -T prefix 2>/dev/null | awk -v k="${1:-a}" '$4==k'; }
"$TMUX_REAL" -L "$PA" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(ak)" in
    *display-menu*) ok "prefix a opens a menu" ;;
    *) bad "prefix a opens a menu" "a display-menu" "$(ak)" ;;
esac
for key in w f r b M e i R; do
    case "$(ak)" in
        *"send-keys -K $key"*|*"send-keys -K '$key'"*) ok "the menu replays prefix $key" ;;
        *) bad "the menu replays prefix $key" "send-keys -K $key" "$(ak | grep -o 'send-keys -K [^ ]*' | tr '\n' ' ')" ;;
    esac
done
# ≡ on the border opens the same menu from the cockpit table, at the pointer.
case "$("$TMUX_REAL" -L "$PA" list-keys -T cockpit 2>/dev/null | awk '$4=="m"')" in
    *"display-menu"*"-x M -y M"*) ok "the ≡ tap opens it at the pointer" ;;
    *) bad "the ≡ tap opens it at the pointer" "display-menu -x M -y M" "$("$TMUX_REAL" -L "$PA" list-keys -T cockpit | awk '$4=="m"' | cut -c1-120)" ;;
esac
"$TMUX_REAL" -L "$PA" set -g @cockpit_review_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" set -g @cockpit_move_key W >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(ak)" in
    *"send-keys -K r"*) bad "a review key of off drops the review row" "no send-keys -K r" "$(ak | grep -o 'send-keys -K [^ ]*' | tr '\n' ' ')" ;;
    *) ok "a review key of off drops the review row" ;;
esac
case "$(ak)" in
    *"send-keys -K W"*) ok "a rebound move key moves the row with it" ;;
    *) bad "a rebound move key moves the row with it" "send-keys -K W" "$(ak | grep -o 'send-keys -K [^ ]*' | tr '\n' ' ')" ;;
esac
"$TMUX_REAL" -L "$PA" kill-server >/dev/null 2>&1
# Fresh servers below, never a just-killed name: a new-session there can lose the
# race to the old server's exit, and every later check passes on no server.
PA="cockpit-pa1-$$"; SOCKS="$SOCKS $PA"
"$TMUX_REAL" -L "$PA" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" set -g @cockpit_menu_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "@cockpit_menu_key off binds nothing" "" "$(ak)"
# A reload that moves the key or turns it off gives the old key back.
"$TMUX_REAL" -L "$PA" set -gu @cockpit_menu_key >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" set -g @cockpit_menu_key n >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(ak n)" in *display-menu*) ok "a moved menu key binds the new key" ;;
    *) bad "a moved menu key binds the new key" "the menu on n" "$(ak n)" ;; esac
is "and gives a back" "" "$(ak a)"
"$TMUX_REAL" -L "$PA" set -g @cockpit_menu_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "turning it off on a reload gives n back, as stock next-window" "next-window" \
   "$(ak n | awk '{print $5}')"
# m is stock mark-pane: moving the menu off it must put mark-pane back, not unbind m.
"$TMUX_REAL" -L "$PA" set -g @cockpit_menu_key m >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(ak m)" in *display-menu*) ok "the menu can take m" ;; *) bad "the menu can take m" "the menu on m" "$(ak m)" ;; esac
"$TMUX_REAL" -L "$PA" set -gu @cockpit_menu_key >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(ak m)" in *"select-pane -m"*) ok "and moving it off m gives stock mark-pane back" ;;
    *) bad "and moving it off m gives stock mark-pane back" "select-pane -m" "$(ak m)" ;; esac
# A key this load binds is never given back: the menu moving from r to a must not
# unbind the review key that this same load binds to r.
"$TMUX_REAL" -L "$PA" set -g @cockpit_menu_key r >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" set -g @cockpit_review_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" set -gu @cockpit_menu_key >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" set -gu @cockpit_review_key >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(ak r)" in *tmux-review*) ok "giving r back does not undo the review key bound in the same load" ;;
    *) bad "giving r back does not undo the review key bound in the same load" "tmux-review on r" "$(ak r)" ;; esac
"$TMUX_REAL" -L "$PA" kill-server >/dev/null 2>&1
# The old name still works, and says it was renamed.
PA="cockpit-pa2-$$"; SOCKS="$SOCKS $PA"
"$TMUX_REAL" -L "$PA" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" set -g @cockpit_ai_key n >/dev/null 2>&1
"$TMUX_REAL" -L "$PA" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(ak n)" in *display-menu*) ok "@cockpit_ai_key still picks the key" ;;
    *) bad "@cockpit_ai_key still picks the key" "the menu on n" "$(ak n)" ;; esac
case "$("$TMUX_REAL" -L "$PA" show-messages 2>/dev/null)" in
    *"@cockpit_ai_key is now @cockpit_menu_key"*) ok "and says it was renamed" ;;
    *) bad "and says it was renamed" "a rename notice" "$("$TMUX_REAL" -L "$PA" show-messages | tail -2)" ;;
esac
"$TMUX_REAL" -L "$PA" kill-server >/dev/null 2>&1
fi

echo "== the menu reaches move-window and the plugin's reload; the example keeps v (#43) =="
# prefix a M opens the move menu and prefix a R runs the plugin's R, both by
# replaying the key. The example's own m menu is gone: stock m (mark-pane) is back.
if need_tmux "menu rows"; then :; else
EM="cockpit-em-$$"; EMO="cockpit-emo-$$"; SOCKS="$SOCKS $EM $EMO"
"$TMUX_REAL" -L "$EM" -f "$DIR/examples/full.tmux.conf" new-session -d -s work -n editor -x 120 -y 30 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$EM" new-session -d -s agents 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$EM" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$EM" list-keys -T prefix 2>/dev/null | awk '$4=="m"')" in
    *select-pane\ -m*) ok "prefix m is stock mark-pane again" ;;
    *) bad "prefix m is stock mark-pane again" "select-pane -m" "$("$TMUX_REAL" -L "$EM" list-keys -T prefix | awk '$4=="m"' | cut -c1-120)" ;;
esac
is "prefix V is gone from the example" "" "$("$TMUX_REAL" -L "$EM" list-keys -T prefix 2>/dev/null | awk '$4=="V"')"
is "prefix B is gone from the example" "" "$("$TMUX_REAL" -L "$EM" list-keys -T prefix 2>/dev/null | awk '$4=="B"')"
case "$("$TMUX_REAL" -L "$EM" list-keys -T prefix 2>/dev/null | awk '$4=="v"')" in
    *even-horizontal*even-vertical*) ok "prefix v toggles stacked and side-by-side" ;;
    *) bad "prefix v toggles stacked and side-by-side" "if-shell over both layouts" "$("$TMUX_REAL" -L "$EM" list-keys -T prefix | awk '$4=="v"')" ;;
esac
"$TMUX_REAL" -L "$EMO" -f /dev/null new-session -d -s o -x 120 -y 32 "TERM=xterm-256color exec $TMUX_REAL -L $EM attach -t work" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$EM" list-clients -F '#{client_name}'
"$TMUX_REAL" -L "$EMO" send-keys -t o C-Space a
until_cap "$EMO" o '*Move window*'
"$TMUX_REAL" -L "$EMO" send-keys -t o M
until_cap "$EMO" o '*new session...*' && ok "prefix a M opens the move menu" \
    || bad "prefix a M opens the move menu" "the move menu" "$("$TMUX_REAL" -L "$EMO" capture-pane -p -t o | grep -v '^\s*$' | tail -4)"
# The sleep stays: the menu below is the same one, so a poll for its rows would
# satisfy on the frame this Escape has not repainted yet.
"$TMUX_REAL" -L "$EMO" send-keys -t o Escape; sleep 0.6
"$TMUX_REAL" -L "$EMO" send-keys -t o C-Space a
until_cap "$EMO" o '*Reload config*'
"$TMUX_REAL" -L "$EMO" send-keys -t o R
until_out '*config reloaded*' "$TMUX_REAL" -L "$EM" show-messages
case "$("$TMUX_REAL" -L "$EM" show-messages 2>/dev/null)" in
    *"config reloaded"*) ok "prefix a R runs the plugin's reload" ;;
    *) bad "prefix a R runs the plugin's reload" "config reloaded in the log" "$("$TMUX_REAL" -L "$EM" show-messages | grep -v ' command: ' | tail -2)" ;;
esac
"$TMUX_REAL" -L "$EMO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$EM" kill-server >/dev/null 2>&1
fi

# tests: cockpit.tmux
echo "== prefix M is the move-window menu =="
if need_tmux "prefix M"; then :; else
PM="cockpit-pm-$$"; SOCKS="$SOCKS $PM"
"$TMUX_REAL" -L "$PM" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$PM" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$PM" list-keys -T prefix 2>/dev/null | awk '$4=="M"')" in
    *display-menu*) ok "prefix M opens the move menu" ;;
    *) bad "prefix M opens the move menu" "a display-menu" \
         "$("$TMUX_REAL" -L "$PM" list-keys -T prefix | awk '$4=="M"')" ;;
esac
"$TMUX_REAL" -L "$PM" set -g @cockpit_move_key W >/dev/null 2>&1
"$TMUX_REAL" -L "$PM" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$PM" list-keys -T prefix 2>/dev/null | awk '$4=="W"')" in
    *display-menu*) ok "@cockpit_move_key rebinds it" ;;
    *) bad "@cockpit_move_key rebinds it" "the menu on W" \
         "$("$TMUX_REAL" -L "$PM" list-keys -T prefix | awk '$4=="W"')" ;;
esac
"$TMUX_REAL" -L "$PM" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$PM" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$PM" set -g @cockpit_move_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$PM" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$PM" list-keys -T prefix 2>/dev/null | awk '$4=="M"')" in
    *"select-pane -M"*) ok "off keeps stock select-pane -M" ;;
    *) bad "off keeps stock select-pane -M" "select-pane -M" \
         "$("$TMUX_REAL" -L "$PM" list-keys -T prefix | awk '$4=="M"')" ;;
esac
"$TMUX_REAL" -L "$PM" kill-server >/dev/null 2>&1
fi

# tests: cockpit.tmux
echo "== the move menu follows the client and never detaches it =="
# Stock move-window leaves the client behind, and moving the last window out
# of the session you are attached to destroys it under you: the client is
# detached and you are out of tmux. The binding switches the client first, so
# every case below ends with one client still attached, on the moved window.
# A menu and a prefix key both need a real client, hence the two-server rig.
if need_tmux "move menu"; then :; else
MV="cockpit-mv-$$"; MVO="cockpit-mvo-$$"; SOCKS="$SOCKS $MV $MVO"
"$TMUX_REAL" -L "$MV" -f /dev/null new-session -d -s work -n editor -x 90 -y 24 >/dev/null 2>&1
MVD=$(mktmpd)      # keeper's own dir: the new session made for it must start there, not in work's
"$TMUX_REAL" -L "$MV" new-window -t work -n keeper -c "$MVD" >/dev/null 2>&1
"$TMUX_REAL" -L "$MV" select-window -t work:editor >/dev/null 2>&1
"$TMUX_REAL" -L "$MV" new-session -d -s agents >/dev/null 2>&1
"$TMUX_REAL" -L "$MV" new-session -d -s "it's" >/dev/null 2>&1
"$TMUX_REAL" -L "$MV" new-session -d -s "has space" >/dev/null 2>&1
"$TMUX_REAL" -L "$MV" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$MVO" -f /dev/null new-session -d -s o -x 90 -y 26 \
    "$TMUX_REAL -L $MV attach -t work" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$MV" list-clients -F '#{client_name}'
OP=$("$TMUX_REAL" -L "$MVO" list-panes -t o -F '#{pane_id}' | head -1)
mv_open() {                   # press prefix M and wait for the menu; a no-show is one clean failure
    "$TMUX_REAL" -L "$MVO" send-keys -t "$1" C-b M
    until_cap "$MVO" "$1" '*agents*' || bad "the menu opens again" "a menu listing agents" \
        "$("$TMUX_REAL" -L "$MVO" capture-pane -p -t "$1" | tail -3)"
}
mv_client() { "$TMUX_REAL" -L "$MV" list-clients -F '#{session_name}:#{window_name}' 2>/dev/null; }
mv_until() {                  # <pattern> against the client's session:window
    local i
    for i in $(seq 1 40); do
        # shellcheck disable=SC2254
        case "$(mv_client)" in $1) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" C-b M
if until_cap "$MVO" "$OP" '*agents*'; then
    ok "the menu opens with the other sessions"
    cap=$("$TMUX_REAL" -L "$MVO" capture-pane -p -t "$OP")
    case "$cap" in *"move 0:editor to"*) ok "and names the window it moves" ;;
        *) bad "and names the window it moves" "move 0:editor to" "$(printf '%s\n' "$cap" | grep -m1 move)" ;; esac
    case "$cap" in *"it's"*"has space"*|*"has space"*"it's"*) ok "a quote or a space in a name does not break it" ;;
        *) bad "a quote or a space in a name does not break it" "it's and has space rows" "$(printf '%s\n' "$cap" | grep -m3 "'")" ;; esac
    # The menu's own rows: the border above names the session too.
    case "$(printf '%s\n' "$cap" | grep '│')" in *" work "*) bad "the session you are in is not offered" "no work row" "$(printf '%s\n' "$cap" | grep '│' | grep -m1 ' work ')" ;;
        *) ok "the session you are in is not offered" ;; esac
    "$TMUX_REAL" -L "$MVO" send-keys -t "$OP" Enter
else
    bad "the menu opens with the other sessions" "a menu listing agents" \
        "$("$TMUX_REAL" -L "$MVO" capture-pane -p -t "$OP" | tail -4)"
    skip_ "and names the window it moves"; skip_ "a quote or a space in a name does not break it"
    skip_ "the session you are in is not offered"
fi
# the first row opens highlighted and S/n: sorts rows by name: Enter is agents
if mv_until 'agents:editor'; then ok "the client follows the window"
else bad "the client follows the window" "agents:editor" "$(mv_client)"; fi
case "$("$TMUX_REAL" -L "$MV" list-sessions -F '#{session_name}' 2>/dev/null | tr '\n' ' ')" in
    *work*) ok "the source session survives with its other window" ;;
    *) bad "the source session survives with its other window" "work listed" \
         "$("$TMUX_REAL" -L "$MV" list-sessions -F '#{session_name}' | tr '\n' ' ')" ;;
esac
# the last window out: work now holds only keeper; move it into a NEW session
"$TMUX_REAL" -L "$MV" switch-client -t work: >/dev/null 2>&1
mv_until 'work:*'
mv_open "$OP"
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" n
until_cap "$MVO" "$OP" '*new session:*'
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" solo Enter
if mv_until 'solo:keeper'; then ok "the last window out lands in the new session, client attached"
else bad "the last window out lands in the new session, client attached" "solo:keeper" "$(mv_client)"; fi
is "the new session starts in the moved pane's directory, not the old session's (#45)" "$MVD" \
   "$("$TMUX_REAL" -L "$MV" display-message -p -t solo: '#{session_path}' 2>/dev/null)"
is "the new session holds only the moved window (no placeholder shell)" "keeper:1" \
   "$("$TMUX_REAL" -L "$MV" list-windows -t solo -F '#{window_name}:#{window_panes}' 2>/dev/null | tr '\n' ' ' | sed 's/ $//')"
case "$("$TMUX_REAL" -L "$MV" list-sessions -F '#{session_name}' 2>/dev/null | tr '\n' ' ')" in
    *work*) bad "the emptied source session is gone" "no work" "$("$TMUX_REAL" -L "$MV" list-sessions -F '#{session_name}' | tr '\n' ' ')" ;;
    *) ok "the emptied source session is gone" ;;
esac
# a taken name moves nothing; an empty name moves nothing
mv_open "$OP"
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" n
until_cap "$MVO" "$OP" '*new session:*'
# @cockpit_move_dst is the first command the prompt runs, and the create is the
# rest of the same list: once a later query sees the new value, that list is
# through the queue. The value here is the typed name, "agents".
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" agents Enter
until_out agents "$TMUX_REAL" -L "$MV" show-option -gqv @cockpit_move_dst
is "a taken name creates and moves nothing" "solo:keeper" "$(mv_client)"
mv_open "$OP"
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" n
until_cap "$MVO" "$OP" '*new session:*'
# Same gate, and the empty name is distinguishable: the option holds "agents"
# from the case above until this prompt clears it.
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" Enter
until_out '' "$TMUX_REAL" -L "$MV" show-option -gqv @cockpit_move_dst
is "an empty name creates and moves nothing" "solo:keeper" "$(mv_client)"
is "and the session count is unchanged" "4" "$("$TMUX_REAL" -L "$MV" list-sessions 2>/dev/null | wc -l)"
# hostile targets: a session named it's receives the window; a new a.b does too
mv_open "$OP"
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" Down Down Enter     # agents, has space, it's
if mv_until "it?s:keeper"; then ok "a session named it's receives the window"
else bad "a session named it's receives the window" "it's:keeper" "$(mv_client)"; fi
mv_open "$OP"
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" n
until_cap "$MVO" "$OP" '*new session:*'
"$TMUX_REAL" -L "$MVO" send-keys -t "$OP" a.b Enter
if mv_until 'a.b:keeper'; then ok "a dotted new-session name works"
else bad "a dotted new-session name works" "a.b:keeper" "$(mv_client)"; fi
is "and leaves no stray placeholder session" "a.b agents has space it's" \
   "$("$TMUX_REAL" -L "$MV" list-sessions -F '#{session_name}' 2>/dev/null | tr '\n' ' ' | sed 's/ $//')"
# the typed name is data: a colon, a quote and a #( all become plain names
for hn in 'x:y' 'p"q' '#(touch pwned)'; do
    mv_open "$OP"
    "$TMUX_REAL" -L "$MVO" send-keys -t "$OP" n
    until_cap "$MVO" "$OP" '*new session:*'
    "$TMUX_REAL" -L "$MVO" send-keys -t "$OP" -l "$hn"; "$TMUX_REAL" -L "$MVO" send-keys -t "$OP" Enter
    if mv_until "$hn:keeper"; then ok "a new session named $hn receives the window"
    else bad "a new session named $hn receives the window" "$hn:keeper" "$(mv_client)"; fi
done
[ -e "$DIR/pwned" ] && { bad "and #( in the name never runs" "no file" "pwned created"; rm -f "$DIR/pwned"; } || ok "and #( in the name never runs"
"$TMUX_REAL" -L "$MVO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$MV" kill-server >/dev/null 2>&1
fi

echo "== a session name is data in the move menu =="
# display-menu expands each item's name as a format, so a session named #(cmd)
# ran cmd as the menu opened: the name has to reach it with its # doubled.
if need_tmux "move menu session name"; then :; else
MH="cockpit-mh-$$"; MHO="cockpit-mho-$$"; SOCKS="$SOCKS $MH $MHO"; MHD=$(mktmpd)
# Silent panes: a login shell's banner can land over the menu and hide its rows.
"$TMUX_REAL" -L "$MH" -f /dev/null new-session -d -s work -n editor -x 90 -y 24 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$MH" new-window -t work -n keeper 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$MH" select-window -t work:editor >/dev/null 2>&1
# new-session expands -s as a format itself: ## lands the literal #( name.
"$TMUX_REAL" -L "$MH" new-session -d -s "##(touch $MHD/pwned)" 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$MH" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$MHO" -f /dev/null new-session -d -s o -x 90 -y 26 \
    "$TMUX_REAL -L $MH attach -t work" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$MH" list-clients -F '#{client_name}'
"$TMUX_REAL" -L "$MHO" send-keys -t o C-b M
until_cap "$MHO" o '*new session...*'
# An absence has no observable to poll: give the #() job time to run.
sleep 1
[ -e "$MHD/pwned" ] && bad "opening the menu runs nothing" "no file" "pwned created" \
    || ok "opening the menu runs nothing"
if until_cap "$MHO" o '*#(touch*'; then ok "the menu lists the session by its literal name"
else bad "the menu lists the session by its literal name" "a #(touch row" \
    "$("$TMUX_REAL" -L "$MHO" capture-pane -p -t o | grep -v '^$' | tail -6)"; fi
"$TMUX_REAL" -L "$MHO" send-keys -t o Enter
if until_out "#(touch $MHD/pwned):editor" "$TMUX_REAL" -L "$MH" list-clients -F '#{session_name}:#{window_name}'; then
    ok "and the window moves into it"
else bad "and the window moves into it" "#(touch $MHD/pwned):editor" \
    "$("$TMUX_REAL" -L "$MH" list-clients -F '#{session_name}:#{window_name}')"; fi
"$TMUX_REAL" -L "$MHO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$MH" kill-server >/dev/null 2>&1
fi

echo "== tapping the path on the border browses the working dir =="
# range=control|2 on the border's path opens the folder browser at the pane's
# cwd -- the same popup the path picker descends into, entered from the top.
if need_tmux "directory click"; then :; else
DB="cockpit-db-$$"; SOCKS="$SOCKS $DB"
"$TMUX_REAL" -L "$DB" -f /dev/null new-session -d -s t -x 90 -y 24 -c "$DIR" >/dev/null 2>&1
"$TMUX_REAL" -L "$DB" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
# Ctrl+RIGHT-click is the collision-free picker gesture: Ghostty opens OSC 8
# links on the first Ctrl+LEFT-click and a GUI raced the popup; the right
# button reaches only us. Unbound in stock tmux, so nothing is displaced.
case "$("$TMUX_REAL" -L "$DB" list-keys -T root 2>/dev/null | grep C-MouseDown3Pane)" in
    *cockpit_click_line*) ok "ctrl+right-click drives the picker" ;;
    *) bad "ctrl+right-click drives the picker" "the click_line binding" \
         "$("$TMUX_REAL" -L "$DB" list-keys -T root | grep C-MouseDown3Pane)" ;;
esac
# On release, through the cockpit table the menu's Browse folder row uses too.
case "$("$TMUX_REAL" -L "$DB" list-keys -T root 2>/dev/null | grep MouseUp1Control2)" in
    *"switch-client -T cockpit"*"send-keys -K o"*) ok "control|2 replays the browse key" ;;
    *) bad "control|2 replays the browse key" "switch-client -T cockpit ; send-keys -K o" \
         "$("$TMUX_REAL" -L "$DB" list-keys -T root | grep MouseUp1Control2)" ;;
esac
case "$("$TMUX_REAL" -L "$DB" list-keys -T cockpit 2>/dev/null | awk '$4=="o"')" in
    *--browse*) ok "which opens the browser popup" ;;
    *) bad "which opens the browser popup" "a display-popup running --browse" \
         "$("$TMUX_REAL" -L "$DB" list-keys -T cockpit | awk '$4=="o"')" ;;
esac
DBLOG=$(mktmpf); DBED=$(mktmpf)
printf '#!/bin/sh\necho "OPEN $*" >> %s\n' "$DBLOG" > "$DBED"; chmod +x "$DBED"
"$TMUX_REAL" -L "$DB" set -g @cockpit_editor "$DBED" >/dev/null 2>&1
DP=$("$TMUX_REAL" -L "$DB" list-panes -t t -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$DB" send-keys -t "$DP" "'$DIR/scripts/tmux-open-path' --browse '$DIR'" Enter
until_cap "$DB" "$DP" '*↵ enter*'
case "$("$TMUX_REAL" -L "$DB" capture-pane -p -t "$DP")" in
    *"↵ enter"*) ok "--browse opens the browser at the dir" ;;
    *) bad "--browse opens the browser at the dir" "the browse header" \
         "$("$TMUX_REAL" -L "$DB" capture-pane -p -t "$DP" | head -3)" ;;
esac
# The query has to be in the prompt before Enter, or Enter opens whatever the
# unfiltered list had selected.
"$TMUX_REAL" -L "$DB" send-keys -t "$DP" "LICENSE"
until_cap "$DB" "$DP" '*/ LICENSE*'
"$TMUX_REAL" -L "$DB" send-keys -t "$DP" Enter
until_file "$DBLOG" '*OPEN *'
is "and a file opens from it" "OPEN $DIR/LICENSE" "$(cat "$DBLOG")"
"$TMUX_REAL" -L "$DB" kill-server >/dev/null 2>&1
fi

echo "== popup geometry is configurable =="
# One pair of options sizes both popups; set before the run-shell line, same
# pattern as @cockpit_agents. The path picker re-reads them at click time.
if need_tmux "popup geometry"; then :; else
PG="cockpit-geom-$$"; SOCKS="$SOCKS $PG"
"$TMUX_REAL" -L "$PG" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$PG" set -g @cockpit_popup_width 55% >/dev/null 2>&1
"$TMUX_REAL" -L "$PG" set -g @cockpit_popup_height 45% >/dev/null 2>&1
"$TMUX_REAL" -L "$PG" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$PG" list-keys -T cockpit | awk '$4=="o"')" in
    *'-h "45%" -w "55%"'*) ok "a custom size reaches the binding" ;;
    *) bad "a custom size reaches the binding" '-h 45% -w 55%' \
         "$("$TMUX_REAL" -L "$PG" list-keys -T cockpit | awk '$4=="o"')" ;;
esac
"$TMUX_REAL" -L "$PG" kill-server >/dev/null 2>&1
fi

echo "== branch popup passes the source pane id =="
# Inside a popup $TMUX_PANE is the popup's own pseudo-pane, so tmux-branch
# must be told which pane the key/click actually fired on rather than
# resolving it itself. The prefix+b bind passes -p resolved by the popup's own
# sh from $TMUX's session id (#38), and the branch tap replays that bind; the
# key-press test above proves the resolution, this one pins the shape.
if need_tmux "branch popup pane id"; then :; else
BPI="cockpit-bpi-$$"; SOCKS="$SOCKS $BPI"
"$TMUX_REAL" -L "$BPI" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$BPI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$BPI" list-keys -T prefix 2>/dev/null | awk '$4=="b"')" in
    *'-p \"$(tmux display-message -p -t'*) ok "prefix b passes the source pane id" ;;
    *) bad "prefix b passes the source pane id" '-p "$(tmux display-message -p -t ... "#{pane_id}")" in the bind' \
         "$("$TMUX_REAL" -L "$BPI" list-keys -T prefix | awk '$4=="b"')" ;;
esac
case "$("$TMUX_REAL" -L "$BPI" list-keys -T root | grep MouseUp1Control3)" in
    *"switch-client -T prefix"*"send-keys -K b"*) ok "the branch tap replays prefix b" ;;
    *) bad "the branch tap replays prefix b" "switch-client -T prefix ; send-keys -K b" \
         "$("$TMUX_REAL" -L "$BPI" list-keys -T root | grep MouseUp1Control3)" ;;
esac
"$TMUX_REAL" -L "$BPI" kill-server >/dev/null 2>&1
fi

echo "== tab marks several files and opens them all =="
# --multi: a compiler error naming a header and its source, a git diff naming
# two files -- mark both, open both. Tab already means "mark several" in tm's
# kill picker, so the gesture is one thing suite-wide. Driven through a real
# pty because fzf needs one.
if need_tmux "multi-open"; then :; else
if command -v fzf >/dev/null 2>&1; then
MO="cockpit-mo-$$"; SOCKS="$SOCKS $MO"
MOLOG=$(mktmpf); MOED=$(mktmpf)
printf '#!/bin/sh\necho "OPEN $*" >> %s\n' "$MOLOG" > "$MOED"; chmod +x "$MOED"
"$TMUX_REAL" -L "$MO" -f /dev/null new-session -d -s t -x 90 -y 24 -c "$DIR" >/dev/null 2>&1
"$TMUX_REAL" -L "$MO" set -g @cockpit_editor "$MOED" >/dev/null 2>&1
MP=$("$TMUX_REAL" -L "$MO" list-panes -t t -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$MO" set-option -p -t "$MP" @cockpit_click_line \
    "see README.md and LICENSE and Makefile" >/dev/null 2>&1
"$TMUX_REAL" -L "$MO" send-keys -t "$MP" "'$DIR/scripts/tmux-open-path' --pick '$MP'" Enter
until_cap "$MO" "$MP" '*open>*'
# The prompt must be visible first, or the keys land in a bare shell and the
# assertion could pass without fzf ever running.
case "$("$TMUX_REAL" -L "$MO" capture-pane -p -t "$MP")" in
    *"open>"*) ok "the picker really came up" ;;
    *) bad "the picker really came up" "an open> prompt" \
         "$("$TMUX_REAL" -L "$MO" capture-pane -p -t "$MP" | head -3)" ;;
esac
# The preview separator names the mode, so ^G/^F visibly do something even
# when the two views look alike.
case "$("$TMUX_REAL" -L "$MO" capture-pane -p -t "$MP")" in
    *"─ content "*) ok "the preview names its mode on the separator" ;;
    *) bad "the preview names its mode on the separator" "a ' content ' label" \
         "$("$TMUX_REAL" -L "$MO" capture-pane -p -t "$MP" | grep '^─' | head -1)" ;;
esac
# One send-keys: fzf reads its keys in order, so the gaps between three calls
# were wall time and nothing else. The log is what the assertion reads.
"$TMUX_REAL" -L "$MO" send-keys -t "$MP" Tab Tab Enter
until_file "$MOLOG" '*OPEN*OPEN*'
is "two marked files both open" "2" "$(grep -c OPEN "$MOLOG")"
"$TMUX_REAL" -L "$MO" kill-server >/dev/null 2>&1
rm -f "$MOLOG" "$MOED"
else
  skip_ "the picker really came up"; skip_ "two marked files both open"
fi
fi

echo "== picking a folder browses into it =="
# A directory is a place, not a file. Enter used to hand it straight to the
# editor; now it descends -- .. climbs, a file opens, ^O is the deliberate
# "open this folder in the editor". Driven end to end through a real pty.
if need_tmux "folder browse"; then :; else
if command -v fzf >/dev/null 2>&1; then
BR="cockpit-br-$$"; SOCKS="$SOCKS $BR"
BRLOG=$(mktmpf); BRED=$(mktmpf)
printf '#!/bin/sh\necho "OPEN $*" >> %s\n' "$BRLOG" > "$BRED"; chmod +x "$BRED"
"$TMUX_REAL" -L "$BR" -f /dev/null new-session -d -s t -x 100 -y 28 -c "$DIR" >/dev/null 2>&1
"$TMUX_REAL" -L "$BR" set -g @cockpit_editor "$BRED" >/dev/null 2>&1
BP=$("$TMUX_REAL" -L "$BR" list-panes -t t -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$BR" set-option -p -t "$BP" @cockpit_click_line "look in scripts/ now" >/dev/null 2>&1
"$TMUX_REAL" -L "$BR" send-keys -t "$BP" "'$DIR/scripts/tmux-open-path' --pick '$BP'" Enter
until_cap "$BR" "$BP" '*open>*'
"$TMUX_REAL" -L "$BR" send-keys -t "$BP" Enter          # the one candidate is a dir
until_cap "$BR" "$BP" '*↵ enter*'
case "$("$TMUX_REAL" -L "$BR" capture-pane -p -t "$BP")" in
    *"↵ enter"*) ok "enter on a folder opens the browser, not the editor" ;;
    *) bad "enter on a folder opens the browser, not the editor" "the browse header" \
         "$("$TMUX_REAL" -L "$BR" capture-pane -p -t "$BP" | head -3)" ;;
esac
"$TMUX_REAL" -L "$BR" send-keys -t "$BP" "tmux-where"
# The query has to be in the prompt before Enter, or Enter opens whatever the
# unfiltered list had selected.
until_cap "$BR" "$BP" '*/ tmux-where*'
"$TMUX_REAL" -L "$BR" send-keys -t "$BP" Enter
until_file "$BRLOG" '*OPEN *'
is "and a file inside it opens" "OPEN $DIR/scripts/tmux-where" "$(cat "$BRLOG")"
"$TMUX_REAL" -L "$BR" kill-server >/dev/null 2>&1
rm -f "$BRLOG" "$BRED"
else
  skip_ "enter on a folder opens the browser, not the editor"; skip_ "and a file inside it opens"
fi
fi

# group: serial
echo "== the preview shows what changed, not just that it changed =="
# The bat rewrite kept the thin git gutter but lost the actual diff view. For a
# modified file with no target line, the diff is nearly always why the file is
# on the line you clicked -- lead with it. A target line wins: the jump is the
# point then. ^G forces the diff anywhere, and a clean file says so instead of
# showing an empty pane.
DV=$(mktmpd)
(
  cd "$DV" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'alpha\nbeta\ngamma\n' > f.txt; git add f.txt
  c=$(git commit-tree "$(git write-tree)" -m init); git update-ref refs/heads/main "$c"
  git symbolic-ref HEAD refs/heads/main; git reset -q --hard
  printf 'alpha\nCHANGED\ngamma\n' > f.txt
) >/dev/null 2>&1
pv() { "$DIR/scripts/tmux-open-path" "$@" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g'; }
case "$(pv --preview "$DV/f.txt")" in
    *'+CHANGED'*) ok "a modified file with no line leads with its diff" ;;
    *) bad "a modified file with no line leads with its diff" "+CHANGED" "$(pv --preview "$DV/f.txt" | head -3)" ;;
esac
case "$(pv --preview "$DV/f.txt" 2)" in
    *'+CHANGED'*) bad "a target line beats the diff" "the content window" "a diff" ;;
    *CHANGED*)    ok  "a target line beats the diff" ;;
    *) bad "a target line beats the diff" "the line content" "$(pv --preview "$DV/f.txt" 2 | head -2)" ;;
esac
case "$(pv --preview-diff "$DV/f.txt")" in
    *'+CHANGED'*) ok "^G shows the diff on demand" ;;
    *) bad "^G shows the diff on demand" "+CHANGED" "$(pv --preview-diff "$DV/f.txt" | head -3)" ;;
esac
( cd "$DV" && git checkout -q f.txt ) 2>/dev/null
is "a clean file says so rather than an empty pane" "no uncommitted changes in f.txt" \
   "$(pv --preview-diff "$DV/f.txt")"
case "$(pv --preview "$DV/f.txt")" in
    *alpha*) ok "and its default preview is the content again" ;;
    *) bad "and its default preview is the content again" "the file" "$(pv --preview "$DV/f.txt" | head -2)" ;;
esac
is "outside a repo, the diff view is honest too" "not in a git repository" \
   "$("$DIR/scripts/tmux-open-path" --preview-diff /tmp 2>/dev/null | head -1 | sed 's/\x1b\[[0-9;]*m//g' )"
rm -rf "$DV"

# --preview-diff used to capture the WHOLE coloured diff into a shell variable
# and run git diff a second time for --stat, both before ever checking the
# result was non-empty. Measured here: a 22.9MB/400000-line single-file diff
# was 3.32s + 191MB RSS on the pre-fix build, 0.76s + 76MB RSS after streaming
# the diff and deriving the stat from --numstat instead. Both halves of that
# are the teeth, and maxRSS is the one no amount of box load can move.
GD=$(mktmpd)
(
  cd "$GD" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  awk 'BEGIN{for(i=1;i<=200000;i++) print "line " i " aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}' > big.txt
  git add -A; git commit -q -m init
  awk 'BEGIN{for(i=1;i<=200000;i++) print "line " i " bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}' > big.txt
) >/dev/null 2>&1
gdout=$(mktmpf)
# One measured run, not two: --preview-diff caches its render under
# $XDG_RUNTIME_DIR, so a second pass would grade the cache instead. GNU time
# reports maxRSS and CPU from that single run; POSIX time reports neither.
GDCPU="a 400000-line single-file diff renders in well under 2.5s of CPU"
GDRSS="and it streams that diff instead of capturing it (maxRSS under 120MB)"
if /usr/bin/time -f '%M %U %S' -o /dev/null true 2>/dev/null; then
    /usr/bin/time -f '%M %U %S' -o "$STATE/gd.time" \
        timeout 10 "$DIR/scripts/tmux-open-path" --preview-diff "$GD/big.txt" \
        >"$gdout" 2>/dev/null
    # A killed command burns almost no CPU and holds almost no memory, so both
    # gates below would pass it. GNU time prepends a line when it kills, hence
    # END rather than the first record.
    gdrc=$?
    if [ "$gdrc" -eq 124 ]; then
        bad "$GDCPU" "under 2.5s of CPU" "hung: killed at 10s"
        bad "$GDRSS" "under 120000 kB" "hung: killed at 10s"
    else
        cpu_is "$GDCPU" 2.5 "$(awk 'END{printf "%.3f", $2+$3}' "$STATE/gd.time")"
        gdrss=$(awk 'END{print $1+0}' "$STATE/gd.time")
        if [ "$gdrss" -lt 120000 ]; then ok "$GDRSS"
        else bad "$GDRSS" "under 120000 kB" "$gdrss kB"; fi
    fi
else
    cpu_under "$GDCPU" 2.5 "$DIR/scripts/tmux-open-path" --preview-diff "$GD/big.txt"
    cat "$CPUOUT" > "$gdout"
    skip_ "$GDRSS (no GNU time)"
fi
case "$(head -1 "$gdout")" in
    *'insertion(s)'*) ok "and still leads with a real stat summary" ;;
    *) bad "and still leads with a real stat summary" \
        "N insertion(s), M deletion(s)" "$(head -1 "$gdout")" ;;
esac
rm -rf "$GD"

echo "== ^O opens a folder whole; the list names the line =="
if need_tmux "^O folder open"; then :; else
if command -v fzf >/dev/null 2>&1; then
OO="cockpit-oo-$$"; SOCKS="$SOCKS $OO"
OOLOG=$(mktmpf); OOED=$(mktmpf)
printf '#!/bin/sh\necho "OPEN $*" >> %s\n' "$OOLOG" > "$OOED"; chmod +x "$OOED"
"$TMUX_REAL" -L "$OO" -f /dev/null new-session -d -s t -x 100 -y 28 -c "$DIR" >/dev/null 2>&1
"$TMUX_REAL" -L "$OO" set -g @cockpit_editor "$OOED" >/dev/null 2>&1
OP=$("$TMUX_REAL" -L "$OO" list-panes -t t -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$OO" set-option -p -t "$OP" @cockpit_click_line "scripts/tm:30 and scripts/" >/dev/null 2>&1
"$TMUX_REAL" -L "$OO" send-keys -t "$OP" "'$DIR/scripts/tmux-open-path' --pick '$OP'" Enter
until_cap "$OO" "$OP" '*tm:30*'
# A clicked "file:31" must read as file:31 in the list, not as a bare filename
# stripped of the very thing that anchored it.
case "$("$TMUX_REAL" -L "$OO" capture-pane -p -t "$OP")" in
    *"tm:30"*) ok "the row keeps its line number" ;;
    *) bad "the row keeps its line number" "scripts/tm:30" \
         "$("$TMUX_REAL" -L "$OO" capture-pane -p -t "$OP" | sed -n 3p)" ;;
esac
"$TMUX_REAL" -L "$OO" send-keys -t "$OP" Down C-o
until_file "$OOLOG" '*OPEN *'
is "^O hands the folder to the editor whole" "OPEN $DIR/scripts/" "$(cat "$OOLOG")"
"$TMUX_REAL" -L "$OO" kill-server >/dev/null 2>&1
rm -f "$OOLOG" "$OOED"
else
  skip_ "the row keeps its line number"; skip_ "^O hands the folder to the editor whole"
fi
fi

echo "== first-run: no theme installed, still works =="
# A border style that read an unset @thm_* collapsed to "fg=" -- which tmux
# REJECTS -- and the headline feature silently never coloured anything. The
# plugin's styles name tmux 3.8's theme slots, which always resolve; this block
# sources the config bare.
if need_tmux "bare first run"; then :; else
FR="cockpit-fr-$$"; SOCKS="$SOCKS $FR"
"$TMUX_REAL" -L "$FR" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FR" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
FP=$("$TMUX_REAL" -L "$FR" list-panes -a -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$FR" set -p -t "$FP" @panestate needs-input >/dev/null 2>&1
is "the border colours without any theme installed" "fg=themered" \
   "$("$TMUX_REAL" -L "$FR" display-message -p -t "$FP" '#{E:#{pane-border-style}}')"
case "$("$TMUX_REAL" -L "$FR" show-option -sv dark-theme-red)" in
    *'#{@thm_needs}'*) ok "and the slot reads the palette's needs role" ;;
    *) bad "and the slot reads the palette's needs role" "#{@thm_needs}" "$("$TMUX_REAL" -L "$FR" show-option -sv dark-theme-red)" ;;
esac
is "which the plugin defaults" "#ff5f6d" "$("$TMUX_REAL" -L "$FR" show-option -gv @thm_needs)"
# 16 colours: error and running must not share yellow.
is "a 16-colour client gets error in magenta, running in yellow" "magenta yellow" \
   "$("$TMUX_REAL" -L "$FR" show-option -sv dark-theme-cyan | sed 's/.*,\([a-z]*\)}$/\1/') $("$TMUX_REAL" -L "$FR" show-option -sv dark-theme-yellow | sed 's/.*,\([a-z]*\)}$/\1/')"
# A window with no agent pane must carry NO marker -- ten windows with two
# agents used to show ten circles, and the circle meant two different things.
"$TMUX_REAL" -L "$FR" new-window -d -n plainwin 'sleep 300' >/dev/null 2>&1
pw=$("$TMUX_REAL" -L "$FR" display-message -p -t plainwin '#{E:window-status-format}' | plain)
case "$pw" in
    *○*|*▲*|*✓*|*●*|*✕*|*◆*) bad "a non-agent window carries no marker" "no glyph" "$pw" ;;
    *) ok "a non-agent window carries no marker" ;;
esac
"$TMUX_REAL" -L "$FR" kill-server >/dev/null 2>&1
fi

echo "== the corner's mark: ▲ elsewhere while another session waits, else ✕ for a failed one (#222) =="
if need_tmux "elsewhere mark"; then :; else
EW="cockpit-ew-$$"; SOCKS="$SOCKS $EW"
"$TMUX_REAL" -L "$EW" -f /dev/null new-session -d -s a 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$EW" new-session -d -s b 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$EW" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$EW" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
until_out '*claude*claude*' "$TMUX_REAL" -L "$EW" list-panes -a -F '#{pane_current_command}' >/dev/null
EWA=$("$TMUX_REAL" -L "$EW" list-panes -t a: -F '#{pane_id}'); EWB=$("$TMUX_REAL" -L "$EW" list-panes -t b: -F '#{pane_id}')
asEW() { TMUX="$SOCKDIR/$EW,0,0" "$DIR/scripts/agent-state" "$1" "$2" >/dev/null 2>&1; }
away() { "$TMUX_REAL" -L "$EW" display-message -p -t "$1" '#{E:@cockpit_away}'; }
asEW needs-input "$EWA"; asEW error "$EWB"
is "a session whose own agent waits sees only the other's failure" "e" "$(away a:)"
is "the failed session sees the other one waiting" "w" "$(away b:)"
asEW running "$EWB"
is "and with the other session quiet there is no mark" "" "$(away a:)"
"$TMUX_REAL" -L "$EW" kill-server >/dev/null 2>&1
fi

echo "== the counts follow a moved window, a killed pane and a closed session (#222) =="
# None of these is a state transition, so agent-state never ran: a window moved
# from a to b showed its own agent as "▲ elsewhere" in b. Hooks sweep instead.
if need_tmux "count sweep"; then :; else
SW="cockpit-sw-$$"; SOCKS="$SOCKS $SW"
"$TMUX_REAL" -L "$SW" -f /dev/null new-session -d -s a 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$SW" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$SW" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$SW" new-window -d -t a: 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$SW" new-session -d -s b 'exec -a claude sleep 300' >/dev/null 2>&1
until_out '*claude*claude*claude*' "$TMUX_REAL" -L "$SW" list-panes -a -F '#{pane_current_command}' >/dev/null
SWP=$("$TMUX_REAL" -L "$SW" display-message -p -t a:1 '#{pane_id}')
TMUX="$SOCKDIR/$SW,0,0" "$DIR/scripts/agent-state" needs-input "$SWP" >/dev/null 2>&1
swaway() { "$TMUX_REAL" -L "$SW" display-message -p -t "$1" '#{E:@cockpit_away}'; }
is "before the move, b sees a's agent waiting" "w" "$(swaway b:)"
"$TMUX_REAL" -L "$SW" move-window -s a:1 -t b: >/dev/null 2>&1
until_out w swaway a: >/dev/null
is "after it, a sees it elsewhere and b does not" "w|" "$(swaway a:)|$(swaway b:)"
"$TMUX_REAL" -L "$SW" kill-pane -t "$SWP" >/dev/null 2>&1
until_out '' swaway a: >/dev/null
is "killing the waiting pane clears the mark" "|0" "$(swaway a:)|$("$TMUX_REAL" -L "$SW" show-option -gqv @cockpit_fleet_nb)"
SWP2=$("$TMUX_REAL" -L "$SW" display-message -p -t b:0 '#{pane_id}')
TMUX="$SOCKDIR/$SW,0,0" "$DIR/scripts/agent-state" error "$SWP2" >/dev/null 2>&1
is "a failed agent in b shows in a" "e" "$(swaway a:)"
"$TMUX_REAL" -L "$SW" kill-session -t b >/dev/null 2>&1
until_out '' swaway a: >/dev/null
is "and closing b takes it away" "" "$(swaway a:)"
"$TMUX_REAL" -L "$SW" kill-server >/dev/null 2>&1
fi

echo "== a sweep of 40 windows fits tmux's command limit and writes every count (#222) =="
# Each window's clause inlined the formats (about 2 KB), so above six windows the
# batch passed tmux's 16 KB limit and nothing was written, silently.
if need_tmux "big sweep"; then :; else
BS="cockpit-bigsweep-$$"; SOCKS="$SOCKS $BS"
"$TMUX_REAL" -L "$BS" -f /dev/null new-session -d -s a 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$BS" new-session -d -s b 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$BS" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
for _ in $(seq 1 19); do
    "$TMUX_REAL" -L "$BS" new-window -d -t a: 'exec -a claude sleep 300' >/dev/null 2>&1
    "$TMUX_REAL" -L "$BS" new-window -d -t b: 'exec -a claude sleep 300' >/dev/null 2>&1
done
until_out 40 sh -c "'$TMUX_REAL' -L '$BS' list-panes -a -F '#{pane_current_command}' | grep -c claude" >/dev/null
for bsw in 3 7 18; do "$TMUX_REAL" -L "$BS" set -p -t "a:$bsw" @panestate needs-input >/dev/null 2>&1; done
"$TMUX_REAL" -L "$BS" set -p -t b:11 @panestate error >/dev/null 2>&1
"$TMUX_REAL" -L "$BS" set -p -t b:19 @cockpit_review pending >/dev/null 2>&1
bsout=$(TMUX="$SOCKDIR/$BS,0,0" "$DIR/scripts/agent-state" sweep 2>&1); bsrc=$?
is "the sweep exits 0 and says nothing" "0|" "$bsrc|$bsout"
# No server, no windows: nothing to send, and no unbound-variable death on the
# way (an empty array under set -u kills bash < 4.4).
bsout=$(TMUX="$SOCKDIR/cockpit-nosuch-$$,0,0" "$DIR/scripts/agent-state" sweep 2>&1); bsrc=$?
is "a sweep with no server sends nothing and exits 0 quietly" "0|" "$bsrc|$bsout"
is "every count is written" "3 0 1 3 1" \
   "$("$TMUX_REAL" -L "$BS" display-message -p -t a: '#{@cockpit_sess_nb} #{@cockpit_sess_ne}') $("$TMUX_REAL" -L "$BS" display-message -p -t b: '#{@cockpit_sess_ne}') $("$TMUX_REAL" -L "$BS" show-option -gqv @cockpit_fleet_nb) $("$TMUX_REAL" -L "$BS" show-option -gqv @cockpit_fleet_ne)"
is "and the last window of the second session has its state and review" "idle 1" \
   "$("$TMUX_REAL" -L "$BS" display-message -p -t b:19 '#{@cockpit_win_state} #{@cockpit_win_review}')"
# The gauge cells move with the active pane, through a hook with no shell.
"$TMUX_REAL" -L "$BS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
until_out '*cells*' "$TMUX_REAL" -L "$BS" show-options -g @cockpit_fmt_cells >/dev/null
"$TMUX_REAL" -L "$BS" split-window -d -t a:5 'exec -a claude sleep 300' >/dev/null 2>&1
BSG=$("$TMUX_REAL" -L "$BS" display-message -p -t a:5.1 '#{pane_id}')
"$TMUX_REAL" -L "$BS" set -p -t "$BSG" @panestate_ctx 91 >/dev/null 2>&1
TMUX="$SOCKDIR/$BS,0,0" "$DIR/scripts/agent-state" sweep >/dev/null 2>&1
is "a gauge on a pane that is not its window's active one counts no cells" "" \
   "$("$TMUX_REAL" -L "$BS" show-option -qv -t a: @cockpit_ctx_all)"
"$TMUX_REAL" -L "$BS" select-pane -t "$BSG" >/dev/null 2>&1
until_out 5 "$TMUX_REAL" -L "$BS" show-option -qv -t a: @cockpit_ctx_all >/dev/null
is "focusing it adds its cells, with no sweep" "5" "$("$TMUX_REAL" -L "$BS" show-option -qv -t a: @cockpit_ctx_all)"
"$TMUX_REAL" -L "$BS" kill-server >/dev/null 2>&1
fi

echo "== NO_COLOR sets every theme slot to the terminal's default (#222) =="
# run-shell's jobs get the server's global environment, which is where a
# NO_COLOR from the shell that started tmux lands.
if need_tmux "NO_COLOR slots"; then :; else
NC="cockpit-nc-$$"; SOCKS="$SOCKS $NC"
"$TMUX_REAL" -L "$NC" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$NC" set-environment -g NO_COLOR 1 >/dev/null 2>&1
"$TMUX_REAL" -L "$NC" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "all twenty slots read default" "20" \
   "$("$TMUX_REAL" -L "$NC" show-options -s | grep -c -- '-theme-[a-z-]* default$')"
"$TMUX_REAL" -L "$NC" kill-server >/dev/null 2>&1
fi

echo "== the window-tab dot precomputes the worst state, byte-for-byte per state =="
# A6 moved the per-window agent marker (▲ blocked, ✕ error, ● running, ✓ done,
# ○ idle) out of the window-status-format, where a redraw re-ran its #{P:} pane
# walk for every window, into one keyword @cockpit_win_state that
# scripts/agent-state writes on each transition -- the WORST state among the
# window's panes, ranked needs-input > error > running > done > idle. This
# drives every state through agent-state (the production path) and pins BOTH
# the keyword recorded and the marker the plugin's tab renders from it.
if need_tmux "window dot precompute"; then :; else
WD="cockpit-wd-$$"; SOCKS="$SOCKS $WD"
"$TMUX_REAL" -L "$WD" -f /dev/null new-session -d -s s -x 200 -y 50 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$WD" source-file "$DIR/examples/full.tmux.conf" >/dev/null 2>&1
# Pin the glob to these fake claude panes. full.tmux.conf's own run-shell only
# sets it when empty, so this non-empty value survives that async pass.
"$TMUX_REAL" -L "$WD" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
"$TMUX_REAL" -L "$WD" rename-window -t s:0 idlewin >/dev/null 2>&1
for w in blkwin errwin runwin finwin; do
    "$TMUX_REAL" -L "$WD" new-window -d -t s: -n "$w" 'exec -a claude sleep 300' >/dev/null 2>&1
done
"$TMUX_REAL" -L "$WD" new-window -d -t s: -n plainwin 'sleep 300' >/dev/null 2>&1
# A lower state present alongside the winner must not change the marker: the
# ranking is proved by the pane that should LOSE sitting next to the one that wins.
"$TMUX_REAL" -L "$WD" split-window -d -t s:blkwin 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$WD" split-window -d -t s:errwin 'exec -a claude sleep 300' >/dev/null 2>&1
asWD() { TMUX="$SOCKDIR/$WD,0,0" "$DIR/scripts/agent-state" "$1" "$2" >/dev/null 2>&1; }
p1() { "$TMUX_REAL" -L "$WD" list-panes -t "s:$1" -F '#{pane_id}' | sed -n "${2:-1}p"; }
# idlewin: a bare agent that only ever saw SessionStart -> off must still dot ○.
asWD off "$(p1 idlewin)"
asWD running "$(p1 blkwin 1)"; asWD needs-input "$(p1 blkwin 2)"   # needs-input > running
asWD done "$(p1 errwin 1)";    asWD error       "$(p1 errwin 2)"   # error > done
asWD running "$(p1 runwin)"
asWD done "$(p1 finwin)"
wdren() { "$TMUX_REAL" -L "$WD" display-message -p -t "s:$1" '#{E:@cockpit_marks}'; }
wdst()  { "$TMUX_REAL" -L "$WD" show-option -wqv -t "s:$1" @cockpit_win_state; }
# The keyword agent-state records -- never written by the pre-A6 code, so these
# are the fail-first anchor.
is "the dot keyword ranks needs-input over running" needs-input "$(wdst blkwin)"
is "the dot keyword ranks error over done"          error       "$(wdst errwin)"
is "the dot keyword records running"                running     "$(wdst runwin)"
is "the dot keyword records done"                   done        "$(wdst finwin)"
is "a bare agent's dot keyword is idle"             idle        "$(wdst idlewin)"
is "a non-agent window records no dot keyword"      ""          "$(wdst plainwin)"
# The marker the tab renders from the keyword, each in its theme slot.
is "the blocked window renders a red ▲, byte-for-byte"      " #[fg=themered]▲"        "$(wdren blkwin)"
is "the errored window renders an error ✕, byte-for-byte"   " #[fg=themecyan]✕"       "$(wdren errwin)"
is "the running window renders a yellow ●, byte-for-byte"   " #[fg=themeyellow]●"     "$(wdren runwin)"
is "the done window renders a green ✓, byte-for-byte"       " #[fg=themegreen]✓"      "$(wdren finwin)"
is "the idle agent window renders a dim ○, byte-for-byte"   " #[fg=themelightgrey]○"  "$(wdren idlewin)"
is "the non-agent window renders no marker, byte-for-byte" ""                         "$(wdren plainwin)"
"$TMUX_REAL" -L "$WD" kill-server >/dev/null 2>&1
fi

echo "== the example configs do not forward the agent title storm =="
# set-titles on FORWARDS every agent OSC-2 title change to the outer terminal as
# a distinct, UNCOALESCED title sequence (measured under a title storm: 609 title
# OSCs, 27KB/3s; off = 0). Windows Terminal is documented-slow at high-frequency
# title updates -- its read loop stalls, drains tmux slower, tmux's out-buffer
# crosses the per-client TTY_BLOCK backpressure limit, and the client freezes.
# The in-tmux tab reads #{pane_title} INTERNALLY, so turning the OUTER title off
# costs nothing on the status bar. full.tmux.conf used to ship set-titles on.
if need_tmux "set-titles"; then :; else
ST="cockpit-st-$$"; SOCKS="$SOCKS $ST"
"$TMUX_REAL" -L "$ST" -f /dev/null new-session -d -s t -x 80 -y 24 >/dev/null 2>&1
# source-file continues past the later tpm line that needs a real install;
# set-titles sits high in the file, before it.
"$TMUX_REAL" -L "$ST" source-file "$DIR/examples/full.tmux.conf" >/dev/null 2>&1
is "full.tmux.conf leaves the outer terminal title off" "off" \
   "$("$TMUX_REAL" -L "$ST" show-option -gv set-titles)"
"$TMUX_REAL" -L "$ST" kill-server >/dev/null 2>&1
fi

# group: etmux
echo "== @cockpit_open_single skips the popup for machine output =="
# Opt-in: a lone candidate that carried an explicit :line is a stack trace or
# compiler error, not a guess. "line" opens those directly; "always" opens any
# lone candidate; unset keeps the picker.
OS=$(mktmpd); OST=$(mktmpd); : > "$OS/f.txt"
cp "$ETMUX_TEMPLATE" "$OST/tmux" 2>/dev/null || true
chmod +x "$OST/tmux" 2>/dev/null
if [ ! -x "$OST/tmux" ]; then skip_ "open_single line opens directly"; skip_ "without a line it still asks"; skip_ "always opens any lone file"
else
ln -s "$(command -v bash)" "$OST/bash"
os_run() { : > "$OST/log"
    env -i PATH="$OST" HOME="$HOME" TMUX_LOG="$OST/log" CLICK_DIR="$OS" \
        OPEN_SINGLE="$1" CLICK_LINE="$2" OPEN_EDITOR=nvim \
        bash "$DIR/scripts/tmux-open-path" %1 >/dev/null 2>&1
    cat "$OST/log"; }
case "$(os_run line 'f.txt:7')" in
    *NEWWIN*) ok "open_single line opens directly" ;;
    *) bad "open_single line opens directly" "the editor, no popup" "$(os_run line 'f.txt:7')" ;;
esac
case "$(os_run line 'f.txt')" in
    *POPUP*) ok "without a line it still asks" ;;
    *) bad "without a line it still asks" "the picker popup" "$(os_run line 'f.txt')" ;;
esac
case "$(os_run always 'f.txt')" in
    *NEWWIN*) ok "always opens any lone file" ;;
    *) bad "always opens any lone file" "the editor, no popup" "$(os_run always 'f.txt')" ;;
esac
fi

echo "== tm --rows and the kill rules: needs-you first, your own last, tiers by the list's width =="
# Driven with $FZF_COLUMNS, the width fzf hands a reload. nosgr drops tm's own
# colours only: an agent's escape has lost its ESC byte if tm stripped it.
if need_tmux "tm rows"; then :; else
TR="cockpit-tmrows-$$"; SOCKS="$SOCKS $TR"
trt() { "$TMUX_REAL" -L "$TR" "$@"; }
trt -f /dev/null new-session -d -s here -x 200 -y 50 'exec -a claude sleep 300' >/dev/null 2>&1
trt set -g @cockpit_agents_glob '|claude|'
trt new-session -d -s quiet 'exec sleep 300'
trt new-session -d -s busy 'exec -a claude sleep 300'
trt new-session -d -s waits 'exec -a claude sleep 300'
trt new-session -d -s broken 'exec -a claude sleep 300'
trs() {                       # trs <session> <state> [ask] -- what agent-state leaves behind
    trt set -p -t "$1:" @panestate "$2"; trt set -w -t "$1:" @cockpit_win_state "$2"
    [ -z "${3:-}" ] || trt set -p -t "$1:" @panestate_msg "$3"
}
trid() { trt display-message -p -t "$1:" '#{session_id}'; }
trs here idle; trs busy running; trs broken error 'API Error: 529'
# Escapes, a tab and a newline: agent free text, as another writer could leave it.
trs waits needs-input "$(printf 'Allow \033[31medit\tto src/a.rs?\nmore\177')"
TRX="$SOCKDIR/$TR,0,$(trid here | tr -d '$')"
TRSGR=$(printf '\037\037\037\037\037\037')
nosgr() { sed $'s/\033\\[[0-9;]*m//g'; }
trrows() { env TMUX="$TRX" TM_SGR="$TRSGR" "$@" "$DIR/scripts/tm" --rows | nosgr; }
trw=$(trrows env FZF_COLUMNS=200)
is "the header counts the sessions and the states worth a look" \
   "	5 sessions · ▲1 · ✕1 · ●1" "$(printf '%s\n' "$trw" | head -1)"
is "needs-you first, error second, your own session last" "waits broken here" \
   "$(printf '%s\n' "$trw" | sed -n '2p;3p;6p' | cut -f3 | tr '\n' ' ' | sed 's/ $//')"
is "field 1 is the session id, so a name tmux cannot target still works" "$(trid waits)" \
   "$(printf '%s\n' "$trw" | sed -n 2p | cut -f1)"
is "the jump numbers follow the rows" "1 2 3 4 5" \
   "$(printf '%s\n' "$trw" | sed 1d | cut -f2 | cut -c1 | tr '\n' ' ' | sed 's/ $//')"
is "a wide row: glyph, name, agents, windows, age" \
   "1 ▲ 	waits	                1 agent   1 win       now" "$(printf '%s\n' "$trw" | sed -n 2p | cut -f2-)"
is "your own row says (here) and has no age" \
   "5 ○ 	here	 (here)          1 agent   1 win          " "$(printf '%s\n' "$trw" | sed -n 6p | cut -f2-)"
case "$(printf '%s\n' "$trw" | grep '	quiet	')" in
    *"	quiet	"*"—"*) ok "a session with no agent has a blank glyph and a dash" ;;
    *) bad "a session with no agent has a blank glyph and a dash" "quiet ... —" "$(printf '%s\n' "$trw" | grep quiet)" ;;
esac
# The preview takes half from 120 columns on, and the list's half decides.
is "120 columns leave a 60-column list: still the wide tier" "4" \
   "$(trrows env FZF_COLUMNS=120 | grep -c 'agent')"
is "below 60 the narrow tier drops the counts" "0 5 ○ 	here	‹here›              " \
   "$(trrows env FZF_COLUMNS=59 | grep -c agent) $(trrows env FZF_COLUMNS=59 | grep '	here	' | cut -f2-)"
# fzf searches only the fields it shows, so the whole name must be shown, or a
# filter on its tail misses and ↵ creates a duplicate (review of #210).
trt new-session -d -s my-very-long-project-name-number-2 'exec sleep 300'
if command -v fzf >/dev/null 2>&1; then
    is "a long name is shown whole, so its tail still matches" "my-very-long-project-name-number-2" \
       "$(trrows env FZF_COLUMNS=200 | fzf --filter number-2 --delimiter='	' --with-nth=2.. --nth=2 --header-lines=1 | cut -f3)"
else
    skip_ "a long name is shown whole, so its tail still matches (no fzf)"
fi
trt kill-session -t =my-very-long-project-name-number-2
# One record per session even with a newline in the ask, and no control byte
# of the agent's reaches fzf.
trtouch=$(trrows env FZF_COLUMNS=56 TM_TOUCH=1 | tr '\0' '\036')
is "touch rows are NUL-ended, one per session plus the header" "6" \
   "$(printf '%s' "$trtouch" | tr -cd '\036' | wc -c)"
is "a waiting touch row carries its ask, control bytes stripped" \
   "1 ▲ 	waits	|     Allow [31meditto src/a.rs?more" \
   "$(printf '%s' "$trtouch" | tr '\036' '\n' | sed -n '/	waits	/,/	broken	/p' | sed '$d' | cut -f2- | sed 's/ *$//' | paste -sd '|')"
# The ask is read on its own: a record separator inside it forges nothing, and
# no field reaches arithmetic unchecked (review of #210: PWNED was created).
trs waits needs-input "$(printf 'x\036$9\0371\0371\037$(touch %s/PWNED)\0371\037\037 needs-input \037forged' "$STATE")"
trforge=$(trrows env FZF_COLUMNS=56 TM_TOUCH=1 | tr '\0' '\036')
is "an ask holding the record separator forges no row and runs nothing" "6 no" \
   "$(printf '%s' "$trforge" | tr -cd '\036' | wc -c) $([ -e "$STATE/PWNED" ] && echo yes || echo no)"
trs waits needs-input 'Allow edit?'
# The colour roles (#210 design check): the ○ glyph is dim, age and (here) soft.
trroles=$(env TMUX="$TRX" TM_SGR="$(printf 'N\037E\037R\037D\037M\037S\037')" FZF_COLUMNS=200 "$DIR/scripts/tm" --rows)
case "$(printf '%s\n' "$trroles" | grep '	here	')" in
    *"M○"*"S (here)"*) ok "the idle glyph is dim and (here) is soft" ;;
    *) bad "the idle glyph is dim and (here) is soft" "M○ ... S (here)" "$(printf '%s\n' "$trroles" | grep '	here	')" ;;
esac
case "$(printf '%s\n' "$trroles" | grep '	busy	')" in
    *"S now"*) ok "the age is soft" ;;
    *) bad "the age is soft" "S now" "$(printf '%s\n' "$trroles" | grep '	busy	')" ;;
esac
# --needs keeps the needs-you band; with nothing waiting it shows everything.
trn=$(trrows env FZF_COLUMNS=200 TM_NEEDS=1)
is "--needs lists the waiting and the broken only" "	needs you · ▲1 ✕1|waits broken" \
   "$(printf '%s\n' "$trn" | head -1)|$(printf '%s\n' "$trn" | sed 1d | cut -f3 | tr '\n' ' ' | sed 's/ $//')"
trs waits running; trs broken done
trn=$(trrows env FZF_COLUMNS=200 TM_NEEDS=1)
is "an empty band falls back to the full list, and says so" "	nothing needs you · 5 sessions · ●2 6" \
   "$(printf '%s\n' "$trn" | head -1) $(printf '%s\n' "$trn" | wc -l)"
# Your own session waiting is not a reason to open the band: you are there.
trs here needs-input 'mine'
trn=$(trrows env FZF_COLUMNS=200 TM_NEEDS=1)
is "only your own session waiting still falls back to the full list" "	nothing needs you · 5 sessions · ▲1 · ●2 6" \
   "$(printf '%s\n' "$trn" | head -1) $(printf '%s\n' "$trn" | wc -l)"
trs here idle
# The order freezes: a later reload keeps the rows where they were, and a new
# session goes last.
TRD=$(mktmpd)
tro1=$(trrows env FZF_COLUMNS=200 TM_DIR="$TRD" | sed 1d | cut -f3 | tr '\n' ' ')
trs quiet needs-input 'now me'
trt new-session -d -s late 'exec sleep 300'
tro2=$(trrows env FZF_COLUMNS=200 TM_DIR="$TRD" | sed 1d | cut -f3 | tr '\n' ' ')
is "a reload keeps the first render's order, new sessions last" "${tro1}late " "$tro2"
# The name column fits the longest name, so a long one is never cut and the
# columns after it stay aligned (#210 design check).
trt new-session -d -s a-name-of-exactly-thirty-five-chars 'exec sleep 300'
for c in 110 200; do
    trcol=$(trrows env FZF_COLUMNS=$c | sed 1d | cut -f2- | tr '\t' ' ')
    is "a 35-character name at $c columns: whole, and every row's columns line up" "yes 1" \
       "$(printf '%s\n' "$trcol" | grep -q 'a-name-of-exactly-thirty-five-chars ' && echo yes) $(printf '%s\n' "$trcol" | awk '{print index($0, " win")}' | sort -u | wc -l)"
done
trt kill-session -t =a-name-of-exactly-thirty-five-chars
# Past the room it has, only that row sheds columns, age first, and ◉ moves up
# beside the name: another device on it must stay visible.
trt new-session -d -s this-session-name-is-fifty-characters-long-exactly 'exec sleep 300'
TRL="cockpit-tmrowsl-$$"; SOCKS="$SOCKS $TRL"
"$TMUX_REAL" -L "$TRL" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "TERM=xterm-256color exec $TMUX_REAL -L $TR attach -t this-session-name-is-fifty-characters-long-exactly" >/dev/null 2>&1
until_out '*fifty*' "$TMUX_REAL" -L "$TR" list-clients -F '#{session_name}'
is "a 50-character name at 80 columns keeps ◉ and its counts, and drops its age" \
   "this-session-name-is-fifty-characters-long-exactly	◉ —         1 win  " \
   "$(trrows env FZF_COLUMNS=80 | grep 'fifty' | cut -f3-)"
# At phone width a name past LW - 10 leaves ◉ to fzf's ellipsis, so it goes
# before the name instead (#216).
trph=$(trrows env FZF_COLUMNS=56 | grep 'fifty')
case "$(printf '%s' "$trph" | cut -f2)" in
    *'◉ ') ok "at 56 columns a long attached name carries ◉ before it" ;;
    *) bad "at 56 columns a long attached name carries ◉ before it" "N G◉ " "$(printf '%s' "$trph" | cut -f2)" ;;
esac
is "and none after it" "" "$(printf '%s' "$trph" | cut -f4)"
"$TMUX_REAL" -L "$TRL" kill-server >/dev/null 2>&1
until_out '' "$TMUX_REAL" -L "$TR" list-clients -t this-session-name-is-fifty-characters-long-exactly -F '#{client_name}'
# Not attached: no placeholder space after the name either (#216).
is "an over-long row with nothing attached ends at its name" "" \
   "$(trrows env FZF_COLUMNS=56 | grep 'fifty' | cut -f4)"
trt kill-session -t =this-session-name-is-fifty-characters-long-exactly
# A garbled list never falls through to the name path: "tm --rows" once
# created a session called "rows".
TRG=$(mktmpd)
printf '#!/bin/sh\ncase "$1" in list-sessions) printf %s ;; new-session) echo NEW >> "%s/log" ;; esac\nexit 0\n' \
    "'\$((1/0))\\037x\\037y\\n'" "$TRG" > "$TRG/tmux"
chmod +x "$TRG/tmux"
env -u TMUX PATH="$TRG:$PATH" "$DIR/scripts/tm" --rows >/dev/null 2>&1
is "a garbled session list creates nothing" "" "$(cat "$TRG/log" 2>/dev/null)"

# ^X twice kills, never your own session; attached elsewhere is allowed. The
# fzf bindings print actions, and the kill runs through --kill-armed.
TRK=$(mktmpd); : > "$TRK/armed"
trk() { env TMUX="$TRX" TM_SGR="$TRSGR" TM_DIR="$TRK" FZF_COLUMNS=200 "$DIR/scripts/tm" "$@" 2>/dev/null | nosgr; }
is "^X on your own session refuses" "change-footer:here is this session · detach or use tm -k" "$(trk --arm "$(trid here)")"
case "$(trk --arm "$(trid here)")" in
    *execute-silent*) bad "a second ^X on your own session still refuses" "no kill" "a kill" ;;
    *) ok "a second ^X on your own session still refuses" ;;
esac
# A leaked $TMUX_PANE names some other pane (#212): $TMUX's session is yours.
trmv=$(trt list-panes -t busy: -F '#{pane_id}')
case "$(env TMUX_PANE="$trmv" TMUX="$TRX" TM_SGR="$TRSGR" TM_DIR="$TRK" FZF_COLUMNS=200 "$DIR/scripts/tm" --arm "$(trid here)" | nosgr)" in
    *"is this session"*) ok "a leaked \$TMUX_PANE does not move your own session" ;;
    *) bad "a leaked \$TMUX_PANE does not move your own session" "is this session" "armed" ;;
esac
: > "$TRK/armed"
trs busy needs-input 'ok?'
is "the first ^X arms: the header names what you lose, the footer how to confirm" \
   "change-header(kill busy? 1 waiting · 1 agent)+change-footer:^X again kills · any key cancels" "$(trk --arm "$(trid busy)")"
trs busy running
is "focus staying on the armed row keeps it armed" "" "$(trk --disarm "$(trid busy)")"
trkout=$(trk --disarm "$(trid quiet)")
case "$trkout" in
    "change-header()+change-footer"?"↵ switch"*) ok "moving off the armed row disarms" ;;
    *) bad "moving off the armed row disarms" "change-header()+change-footer(↵ switch ...)" "$trkout" ;;
esac
trk --arm "$(trid busy)" >/dev/null
# A resize redraws the footer, so it must drop the armed kill with it.
trkout=$(trk --resized)
is "a resize disarms" "change-header()+ " "${trkout%%change-footer*} $(cat "$TRK/armed")"
trk --arm "$(trid busy)" >/dev/null
trkout=$(trk --arm "$(trid busy)")
case "$trkout" in
    execute-silent*--kill-armed*+change-header\(\)+change-footer*+reload-sync*) ok "the second ^X kills, then reloads last" ;;
    *) bad "the second ^X kills, then reloads last" "execute-silent(...)+...+reload-sync(...)" "$trkout" ;;
esac
trbusy=$(trid busy)
trk --kill-armed
is "and --kill-armed kills the armed session" "no" \
   "$(trt has-session -t "$trbusy" 2>/dev/null && echo yes || echo no)"
# A client on another device: the confirm says so first, and the kill goes ahead.
TRO="cockpit-tmrowso-$$"; SOCKS="$SOCKS $TRO"
"$TMUX_REAL" -L "$TRO" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "TERM=xterm-256color exec $TMUX_REAL -L $TR attach -t late" >/dev/null 2>&1
until_out '*late*' "$TMUX_REAL" -L "$TR" list-clients -F '#{session_name}'
trlate=$(trid late)
is "an attached session arms with 'attached elsewhere' first" \
   "change-header(kill late? attached elsewhere · no agents)+change-footer:^X again kills · any key cancels" "$(trk --arm "$trlate")"
trk --arm "$trlate" >/dev/null; trk --kill-armed
is "and the confirmed kill takes it" "no" \
   "$(trt has-session -t "$trlate" 2>/dev/null && echo yes || echo no)"
"$TMUX_REAL" -L "$TRO" kill-server >/dev/null 2>&1
# The no-match footer says ↵ creates the query, and goes once something matches.
: > "$TRK/armed"
is "a query nothing matches: the footer says ↵ creates it" 'change-footer:↵ create "new_one" · esc' \
   "$(FZF_MATCH_COUNT=0 FZF_QUERY='new one' trk --result)"
case "$(FZF_MATCH_COUNT=2 FZF_QUERY=ne trk --result)" in
    "change-header()+change-footer"?"↵ switch"*) ok "and a match restores the footer" ;;
    *) bad "and a match restores the footer" "the resting footer" "$(cat "$TRK/armed")" ;;
esac
# Touch: buttons share the footer evenly, the detail of a kill goes up top, and
# a tap counts anywhere in its share.
trkt() { env TMUX="$TRX" TM_SGR="$TRSGR" TM_DIR="$TRK" TM_TOUCH=1 FZF_COLUMNS="${TRKW:-56}" "$DIR/scripts/tm" "$@" | nosgr; }
is "the touch footer: four buttons centred in equal shares" \
   "  [ + new ]    [ kill ]     [ view ]     [ close ]  " "$(trkt --footer)"
trquiet=$(trid quiet)
is "touch arms in the header and the footer" \
   "change-header(kill quiet? no agents)+change-footer:      [ kill quiet ]              [ cancel ]        " \
   "$(trkt --arm "$trquiet")"
is "a tap right of [ cancel ] still cancels" "change-header()+change-footer(  [ + new ]    [ kill ]     [ view ]     [ close ]  )" \
   "$(FZF_CLICK_FOOTER_COLUMN=50 trkt --click "$trquiet")"
is "+ new with an empty query asks for a name" "change-footer:type a name, then tap + new" \
   "$(FZF_CLICK_FOOTER_COLUMN=7 trkt --click "$trquiet")"
# The hint must not swallow the next tap (review of #210).
is "a tap on close after the hint still closes" "change-header()+change-footer(  [ + new ]    [ kill ]     [ view ]     [ close ]  )+abort" \
   "$(FZF_CLICK_FOOTER_COLUMN=45 trkt --click "$trquiet")"
: > "$TRK/armed"
is "+ new with a query creates it" "print(new)+accept" "$(FZF_QUERY=x FZF_CLICK_FOOTER_COLUMN=2 trkt --click "$trquiet")"
is "a tap in the gap left of [ view ] is view's" "toggle-preview" "$(FZF_CLICK_FOOTER_COLUMN=28 trkt --click "$trquiet")"
# Every column of every drawn button is that button: the tap and the drawing
# share one geometry (#210 design check: the left edge of close toggled view).
# A 56-column popup gives fzf 54, where the old split drifted a column.
for trw in 54 56; do
    trfoot=$(TRKW=$trw trkt --footer); trmiss=''
    for trb in '+ new|change-footer:type a name' 'kill|change-header(kill quiet?' 'view|toggle-preview' 'close|abort'; do
        trl=${trb%%|*}; trpre=${trfoot%%"[ $trl ]"*}
        for c in $(seq $((${#trpre} + 1)) $((${#trpre} + ${#trl} + 4))); do
            : > "$TRK/armed"
            case "$(TRKW=$trw FZF_CLICK_FOOTER_COLUMN=$c trkt --click "$trquiet")" in
                "${trb#*|}"*) ;; *) trmiss+=" $trl@$c" ;; esac
        done
    done
    is "at $trw columns each drawn button answers at every one of its columns" "" "$trmiss"
done
: > "$TRK/armed"
"$TMUX_REAL" -L "$TR" kill-server >/dev/null 2>&1
fi

echo "== tm: a name tmux cannot target, and the popup's own client =="
# tmux reads "=\$5x" as a session id, so every -t "=NAME" missed such a session
# and tm made a duplicate (review of #210). A fake tmux logs what tm asks for.
TF=$(mktmpd)
cat > "$TF/tmux" <<'STUB'
#!/bin/sh
printf '%s\n' "$*" >> "$TF_LOG"
case "$1" in
  list-sessions)   printf '%s\n' '$7 $5x' '$8 plain' ;;
  display-message) echo 1 ;;
esac
exit 0
STUB
chmod +x "$TF/tmux"
tfrun() { : > "$TF/log"; env -u TMUX_PANE TF_LOG="$TF/log" PATH="$TF:$PATH" "$@" >/dev/null 2>&1; }
tfrun env -u TMUX "$DIR/scripts/tm" '$5x'
is "tm \$5x attaches the existing session by id, creating nothing" "attach-session -d -t \$7|0" \
   "$(grep '^attach' "$TF/log")|$(grep -c '^new-session' "$TF/log")"
tfrun env -u TMUX "$DIR/scripts/tm" -k '$5x'
is "and tm -k \$5x kills it by id" "kill-session -t \$7" "$(grep '^kill' "$TF/log")"
# From the popup, the switch names the client that pressed prefix+s: a phone
# on the same session must not be the one that moves.
tfrun env TMUX=/tmp/fake,1,8 TM_CLIENT=/dev/pts/9 "$DIR/scripts/tm" plain
is "the popup's switch goes to its own client" "switch-client -c /dev/pts/9 -t \$8" "$(grep '^switch' "$TF/log")"

echo "== tm --popup: shrink-to-fit, the user's size options are ceilings =="
# A fake tmux answers the one display-message --popup asks and logs the popup.
# Fields: sessions, waiting, needs-you band, @cockpit_touch, width, height.
TP=$(mktmpd)
cat > "$TP/tmux" <<'STUB'
#!/bin/sh
case "$1" in
  display-message) printf '%s\n' "$TP_ANSWER" ;;
  display-popup)   shift; printf '%s\n' "$*" >> "$TP_LOG" ;;
esac
exit 0
STUB
chmod +x "$TP/tmux"
tpsize() {                    # tpsize <answer> <cols> <rows> [--needs] -- "-w W -h H"
    local a=$1; shift
    : > "$TP/log"
    env -u TMUX PATH="$TP:$PATH" TP_LOG="$TP/log" TP_ANSWER="$(printf '%s' "$a" | tr '|' '\037')" \
        "$DIR/scripts/tm" --popup /dev/pts/9 '$3' "$@" >/dev/null 2>&1
    sed 's/.*\(-w [0-9]* -h [0-9]*\).*/\1/' "$TP/log"
}
tpspec() { sed -n 's/.*TM_PREVIEW=\([^ ]*\) .*/\1/p' "$TP/log" | tr -d "\\\\"; }
is "desktop: 70% wide, rows + 7 under the 18-line floor" "-w 140 -h 18" "$(tpsize '7|2|3||||' 200 50)"
is "desktop: many sessions stop at 70% of the height" "-w 140 -h 35" "$(tpsize '40|0|0||||' 200 50)"
# The preview lands where fzf puts it: right while half the list is 60+
# columns, below otherwise, whatever the client's tier (review of #210).
is "165 columns: 70% is too narrow for a right preview, so it goes below and every row fits" "-w 115 -h 34" "$(tpsize '14|0|0||||' 165 50)"
is "140 columns: the preview is on the right, so no lines are kept for it" "-w 126 -h 18" "$(tpsize '7|2|3||||' 140 50)"
is "a 60% ceiling at 200 drops the preview below, and the rows still fit" "-w 120 -h 27 down,12,border-top" \
   "$(tpsize '7|2|3||60%||' 200 50) $(tpspec)"
is "tablet: 90% wide, with 12 preview lines and their rule below" "-w 99 -h 27" "$(tpsize '7|2|3||||' 110 40)"
# The rows come first; the preview gets what is left, and under 5 lines it
# starts hidden (#210 design check: 110x24 showed one row).
is "a short tablet keeps the rows and starts the preview hidden" "-w 99 -h 14 hidden,down,50%,border-top" \
   "$(tpsize '7|2|3||||' 110 24) $(tpspec)"
is "@cockpit_touch off on a phone: a 6-line preview below the rows" "-w 50 -h 21 down,6,border-top" \
   "$(tpsize '7|2|3|off|||' 56 30) $(tpspec)"
is "touch below 100 columns: full width, two lines a row, one more per ask" "-w 56 -h 23" "$(tpsize '7|2|3||||' 56 30)"
is "touch keeps a 12-line floor" "-w 56 -h 12" "$(tpsize '2|0|0||||' 56 30)"
is "@cockpit_touch on at a desktop width" "-w 200 -h 23" "$(tpsize '7|2|3|on|||' 200 50)"
is "the size options are ceilings, percent or cells" "-w 120 -h 10" "$(tpsize '7|2|3||60%|10|' 200 50)"
is "and never floors" "-w 140 -h 18" "$(tpsize '7|2|3||90%|40|' 200 50)"
is "--needs sizes for the needs-you band" "-w 56 -h 17" "$(tpsize '7|2|4||||' 56 30 --needs)"
tpsize '7|2|3||||' 200 50 >/dev/null
case "$(cat "$TP/log")" in
    "-c /dev/pts/9 -t \$3 -w 140 -h 18 -T  sessions  -E TM_POPUP=1 TM_TOUCH=0 TM_SID=\\\$3 TM_CLIENT=/dev/pts/9 TM_PREVIEW='' "*/tm) ok "the popup goes to that client, in its session, with the tier" ;;
    *) bad "the popup goes to that client, in its session, with the tier" "-c /dev/pts/9 -t \$3 ... TM_SID=\\\$3 TM_CLIENT=/dev/pts/9" "$(cat "$TP/log")" ;;
esac
tpsize '7|2|3||||' 56 30 --needs >/dev/null
case "$(cat "$TP/log")" in
    *"TM_TOUCH=1 "*"/tm --needs") ok "--needs rides into the popup" ;;
    *) bad "--needs rides into the popup" "... tm --needs" "$(cat "$TP/log")" ;;
esac

echo "== tm: the picker, driven: type-to-create, alt-enter, 1-9, ^X twice =="
# Real fzf in a private pane. A real client on the server, so switch-client
# has someone to move and session_attached means something.
if need_tmux "tm picker"; then :; else
if command -v fzf >/dev/null 2>&1 && . "$DIR/scripts/lib/vnum.sh" &&
   [ "$(vnum "$(fzf --version | cut -d' ' -f1)")" -ge "$(vnum 0.73.0)" ]; then
TK="cockpit-tk-$$"; TKO="cockpit-tko-$$"; SOCKS="$SOCKS $TK $TKO"
# A bare shell: a login shell's banner can outlast every wait below.
"$TMUX_REAL" -L "$TK" -f /dev/null new-session -d -s alpha -x 120 -y 30 'bash --norc --noprofile' >/dev/null 2>&1
"$TMUX_REAL" -L "$TK" set -g default-command 'bash --norc --noprofile'
"$TMUX_REAL" -L "$TK" set -g @cockpit_touch off
"$TMUX_REAL" -L "$TK" new-session -d -s beta >/dev/null 2>&1
"$TMUX_REAL" -L "$TK" new-session -d -s gamma >/dev/null 2>&1
"$TMUX_REAL" -L "$TKO" -f /dev/null new-session -d -s o -x 120 -y 30 \
    "TERM=xterm-256color exec $TMUX_REAL -L $TK attach -t alpha" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$TK" list-clients -F '#{client_name}'
tkon() { "$TMUX_REAL" -L "$TK" list-clients -F '#{session_name}' 2>/dev/null | head -1; }
tkopen() {                    # tkopen -- tm in the client's current pane; prints the pane
    local p; p=$("$TMUX_REAL" -L "$TK" list-clients -F '#{pane_id}' | head -1)
    "$TMUX_REAL" -L "$TK" send-keys -t "$p" "'$DIR/scripts/tm'" Enter
    # Your own row is the last one drawn, so the list is complete once it shows.
    until_cap "$TK" "$p" '*session>*sessions*(here)*'
    printf '%s' "$p"
}
KP2=$(tkopen)
case "$("$TMUX_REAL" -L "$TK" capture-pane -p -t "$KP2")" in
    *"alpha"*"(here)"*) ok "your own session is labelled (here)" ;;
    *) bad "your own session is labelled (here)" "alpha (here)" "$("$TMUX_REAL" -L "$TK" capture-pane -p -t "$KP2" | head -8)" ;;
esac
until_cap "$TK" "$KP2" '*[a-z] · 1 window*no agent*'
case "$("$TMUX_REAL" -L "$TK" capture-pane -p -t "$KP2")" in
    *[a-z]" · 1 window"*"no agent"*) ok "sessions preview their windows, named" ;;
    *) bad "sessions preview their windows, named" "NAME · 1 window" "$("$TMUX_REAL" -L "$TK" capture-pane -p -t "$KP2" | head -8)" ;;
esac
# Typing filters; it never acts. A name nothing matches + Enter creates it.
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" "zeta"
until_cap "$TK" "$KP2" '*session> zeta*'
is "typing filters instead of acting" "3" "$("$TMUX_REAL" -L "$TK" list-sessions 2>/dev/null | wc -l)"
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" Enter
until_out zeta tkon
is "Enter with no match creates the typed name and switches to it" "zeta" "$(tkon)"
# alt-Enter creates even though "alp" matches alpha.
KP2=$(tkopen)
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" alp
until_cap "$TK" "$KP2" '*session> alp*'
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" M-Enter
until_out alp tkon
is "alt-Enter creates the query even when a row matches" "alp" "$(tkon)"
# 2 with an empty query jumps to row 2; the order is the renderer's.
KP2=$(tkopen)
tkx="$SOCKDIR/$TK,0,$("$TMUX_REAL" -L "$TK" display-message -p -t alp: '#{session_id}' | tr -d '$')"
tkwant=$(env TMUX="$tkx" FZF_COLUMNS=120 "$DIR/scripts/tm" --rows | sed -n 3p | cut -f3)
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" 2
until_out "$tkwant" tkon
is "2 on an empty query switches to row 2" "$tkwant" "$(tkon)"
# ^X once arms and kills nothing; ^X again kills.
KP2=$(tkopen)
tkdoomed=$(env TMUX="$SOCKDIR/$TK,0,$("$TMUX_REAL" -L "$TK" display-message -p -t "$(tkon):" '#{session_id}' | tr -d '$')" \
    FZF_COLUMNS=120 "$DIR/scripts/tm" --rows | sed -n 2p | cut -f3)
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" C-x
until_cap "$TK" "$KP2" '*again kills*'
is "one ^X kills nothing" "5" "$("$TMUX_REAL" -L "$TK" list-sessions 2>/dev/null | wc -l)"
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" C-x
until_out 4 eval "\"$TMUX_REAL\" -L $TK list-sessions 2>/dev/null | wc -l"
is "the second ^X kills the highlighted session" "no" \
   "$("$TMUX_REAL" -L "$TK" has-session -t "=$tkdoomed" 2>/dev/null && echo yes || echo no)"
# Your own row, found by an exact query: ^X there refuses, twice.
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" "^$(tkon)\$"
until_cap "$TK" "$KP2" "*session> ^$(tkon)\$*1/4*"
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" C-x
# The second ^X only after the refusal shows: on any other row it would kill.
if until_cap "$TK" "$KP2" '*is this session*'; then
    "$TMUX_REAL" -L "$TK" send-keys -t "$KP2" C-x
    sleep 0.3
    is "your own session refuses to die from the picker" "4" \
       "$("$TMUX_REAL" -L "$TK" list-sessions 2>/dev/null | wc -l)"
else
    bad "your own session refuses to die from the picker" "the refusal footer" \
        "$("$TMUX_REAL" -L "$TK" capture-pane -p -t "$KP2" | tail -12)"
fi
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" Escape
"$TMUX_REAL" -L "$TKO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TK" kill-server >/dev/null 2>&1
else
  for t in "your own session is labelled (here)" "sessions preview their windows, named" \
           "typing filters instead of acting" "Enter with no match creates the typed name and switches to it" \
           "alt-Enter creates the query even when a row matches" "2 on an empty query switches to row 2" \
           "one ^X kills nothing" "the second ^X kills the highlighted session" \
           "your own session refuses to die from the picker"; do skip_ "$t (needs fzf 0.73+)"; done
fi
fi

echo "== tm: Enter in the prefix+s popup switches the client =="
# The real path: tm --popup opens the picker, Enter picks a row. The server
# starts with a leaked $TMUX_PANE, as one launched from inside tmux does (#212).
if need_tmux "tm popup Enter"; then :; else
if command -v fzf >/dev/null 2>&1 && . "$DIR/scripts/lib/vnum.sh" &&
   [ "$(vnum "$(fzf --version | cut -d' ' -f1)")" -ge "$(vnum 0.73.0)" ]; then
TQI="cockpit-tmqi-$$"; TQO="cockpit-tmqo-$$"; SOCKS="$SOCKS $TQI $TQO"
TMUX_PANE=%99 "$TMUX_REAL" -L "$TQI" -f /dev/null new-session -d -s one 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TQI" new-session -d -s two 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TQO" -f /dev/null new-session -d -s o -x 120 -y 30 \
    "TERM=xterm-256color exec $TMUX_REAL -L $TQI attach -t one" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$TQI" list-clients -F '#{client_name}'
tqc=$("$TMUX_REAL" -L "$TQI" list-clients -F '#{client_name}')
"$TMUX_REAL" -L "$TQI" run-shell -b "$DIR/scripts/tm --popup '$tqc' '$("$TMUX_REAL" -L "$TQI" display-message -p -t one: '#{session_id}')' 120 30"
until_cap "$TQO" o: '*session>*(here)*'
"$TMUX_REAL" -L "$TQO" send-keys -t o: two
until_cap "$TQO" o: '*session> two*'
"$TMUX_REAL" -L "$TQO" send-keys -t o: Enter
until_out "$tqc two" "$TMUX_REAL" -L "$TQI" list-clients -F '#{client_name} #{session_name}'
is "Enter on a row switches the popup's own client, and nests nothing" "$tqc two" \
   "$("$TMUX_REAL" -L "$TQI" list-clients -F '#{client_name} #{session_name}' 2>/dev/null)"
"$TMUX_REAL" -L "$TQO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TQI" kill-server >/dev/null 2>&1
else
  skip_ "Enter on a row switches the popup's own client, and nests nothing (needs fzf 0.73+)"
fi
fi

echo "== tm: your own session, from a plain display-popup and from a moved pane =="
# A user's own "display-popup -E tm" binding, on a server whose start env
# leaked a $TMUX_PANE that names a pane in ANOTHER session: that session must
# not become "here", and yours must not become killable (re-review of #210).
if need_tmux "tm in a plain popup"; then :; else
if command -v fzf >/dev/null 2>&1 && . "$DIR/scripts/lib/vnum.sh" &&
   [ "$(vnum "$(fzf --version | cut -d' ' -f1)")" -ge "$(vnum 0.73.0)" ]; then
TLI="cockpit-tmli-$$"; TLO="cockpit-tmlo-$$"; SOCKS="$SOCKS $TLI $TLO"
# real gets %0 and other %1, the pane the leak names.
TMUX_PANE=%1 "$TMUX_REAL" -L "$TLI" -f /dev/null new-session -d -s real -x 120 -y 30 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TLI" new-session -d -s other 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TLI" set -g @cockpit_touch off
"$TMUX_REAL" -L "$TLO" -f /dev/null new-session -d -s o -x 120 -y 30 \
    "TERM=xterm-256color exec $TMUX_REAL -L $TLI attach -t real" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$TLI" list-clients -F '#{client_name}'
tlc=$("$TMUX_REAL" -L "$TLI" list-clients -F '#{client_name}')
"$TMUX_REAL" -L "$TLI" display-popup -c "$tlc" -w 100 -h 28 -E "$DIR/scripts/tm" >/dev/null 2>&1 &
until_cap "$TLO" o: '*session>*2 sessions*'
"$TMUX_REAL" -L "$TLO" send-keys -t o: '^real$'
until_cap "$TLO" o: '*session> ^real$*1/2*'
case "$("$TMUX_REAL" -L "$TLO" capture-pane -p -t o: | grep ' real ')" in
    *"(here)"*) ok "the popup's own session is the one marked (here)" ;;
    *) bad "the popup's own session is the one marked (here)" "real (here)" \
           "$("$TMUX_REAL" -L "$TLO" capture-pane -p -t o: | grep ' real ')" ;;
esac
"$TMUX_REAL" -L "$TLO" send-keys -t o: C-x
if until_cap "$TLO" o: '*real is this session*'; then
    ok "and ^X on it refuses"
else
    bad "and ^X on it refuses" "real is this session" "$("$TMUX_REAL" -L "$TLO" capture-pane -p -t o: | tail -4)"
fi
"$TMUX_REAL" -L "$TLO" send-keys -t o: Escape
# And a pane moved to another session: its $TMUX still names the one it was
# born in, but its tty is the pane's, so the session it now sits in is yours.
TLD=$(mktmpd); : > "$TLD/armed"
"$TMUX_REAL" -L "$TLI" new-window -d -t real: 'bash --norc --noprofile' >/dev/null 2>&1
"$TMUX_REAL" -L "$TLI" move-window -s real:1 -t other:5 >/dev/null 2>&1
tlo=$("$TMUX_REAL" -L "$TLI" display-message -p -t other: '#{session_id}')
"$TMUX_REAL" -L "$TLI" send-keys -t other:5 \
    "TM_DIR=$TLD TM_SGR=\$(printf '\\037\\037\\037\\037\\037\\037') '$DIR/scripts/tm' --arm '$tlo' > $TLD/out" Enter
until_file "$TLD/out" '*session*'
is "a pane moved to another session refuses ^X on its new one" \
   "change-footer:other is this session · detach or use tm -k" "$(sed $'s/\033\\[[0-9;]*m//g' "$TLD/out")"
"$TMUX_REAL" -L "$TLO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TLI" kill-server >/dev/null 2>&1
else
  skip_ "the popup's own session is the one marked (here) (needs fzf 0.73+)"
  skip_ "and ^X on it refuses (needs fzf 0.73+)"
  skip_ "a pane moved to another session refuses ^X on its new one (needs fzf 0.73+)"
fi
fi

echo "== install.sh --hooks merges additively =="
HK=$(mktmpd); mkdir -p "$HK/.claude"
cat > "$HK/.claude/settings.json" <<'EOF'
{ "model": "opus", "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "my-own-tool" } ] } ] } }
EOF
if command -v jq >/dev/null 2>&1; then
HOME="$HK" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "the installer exits clean" "0" "$?"
case "$(cat "$HK/.claude/settings.json")" in
    *my-own-tool*) ok "a foreign hook survives the merge" ;;
    *) bad "a foreign hook survives the merge" "my-own-tool still present" "$(head -c200 "$HK/.claude/settings.json")" ;;
esac
case "$(cat "$HK/.claude/settings.json")" in
    *tmux-cockpit*agent-state*) ok "cockpit's hooks arrived" ;;
    *) bad "cockpit's hooks arrived" "an agent-state command" "$(head -c200 "$HK/.claude/settings.json")" ;;
esac
is "the unslop skill is injected on SessionStart" "1" \
   "$(jq '[.hooks.SessionStart[].hooks[].command | select(test("unslop/SKILL.md"))] | length' "$HK/.claude/settings.json")"
is "beside agent-state off, not instead of it" "2" "$(jq '.hooks.SessionStart | length' "$HK/.claude/settings.json")"
# Run the shipped command itself: the string landing in settings.json says
# nothing about the JSON it emits, and a jq typo would otherwise ship green.
is "and the shipped command emits a SessionStart context" "SessionStart 1" \
   "$(TMUX_COCKPIT_DIR="$DIR" bash -c "$(jq -r '.hooks.SessionStart[].hooks[].command | select(test("unslop/SKILL.md"))' "$HK/.claude/settings.json")" \
      | jq -r '[.hookSpecificOutput.hookEventName, (.hookSpecificOutput.additionalContext | test("Unslop") | if . then 1 else 0 end)] | join(" ")')"
is "the user's other settings are untouched" "opus" \
   "$(jq -r .model "$HK/.claude/settings.json")"
[ -f "$HK/.claude/settings.json.bak" ] && ok "a backup was written" \
    || bad "a backup was written" "settings.json.bak" "missing"
before=$(cat "$HK/.claude/settings.json")
HOME="$HK" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "a second run changes nothing (idempotent)" "$before" "$(cat "$HK/.claude/settings.json")"
is "the shipped Stop hooks are two groups beside the foreign one, agent-state done once, review-gate capped at 5 s" "3 1 5" \
   "$(jq -r '[(.hooks.Stop | length),
             ([.hooks.Stop[].hooks[].command | select(test("agent-state done"))] | length),
             ([.hooks.Stop[].hooks[] | select(.command | test("review-gate")) | .timeout] | add)] | join(" ")' \
       "$HK/.claude/settings.json")"
# The same hook with and without an explicit matcher:"" is the SAME hook:
# a live Stop entry carrying the key while the shipped one lacked it made
# every --hooks run append one more duplicate, forever. Seed the matcher-less
# shape and assert the merge recognizes it rather than stacking a twin.
HKN=$(mktmpd); mkdir -p "$HKN/.claude"
jq '.hooks.Stop[0] |= del(.matcher)' "$DIR/examples/claude-hooks.json" \
    | jq '{hooks: {Stop: .hooks.Stop}}' > "$HKN/.claude/settings.json"
HOME="$HKN" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "a matcher-less twin does not duplicate" "2" \
   "$(jq '.hooks.Stop | length' "$HKN/.claude/settings.json")"
rm -rf "$HKN"
# review-gate's own Stop entry is REPLACED on upgrade, not duplicated: seed a
# settings.json holding an older shape (timeout 330) and assert the
# merge leaves exactly one review-gate Stop entry, at the new timeout. A
# second, FOREIGN entry that merely contains "/scripts/review-gate" as a
# substring (not at the end of its command) must survive untouched -- the
# drop clause is anchored, not a bare substring test.
HKG=$(mktmpd); mkdir -p "$HKG/.claude"
cat > "$HKG/.claude/settings.json" <<'EOF'
{ "hooks": { "Stop": [
    { "matcher": "", "hooks": [ { "type": "command", "command": "/old/path/scripts/review-gate", "timeout": 330 } ] },
    { "matcher": "", "hooks": [ { "type": "command", "command": "/usr/local/bin/mywrap --log /scripts/review-gate-audit.log" } ] }
  ] } }
EOF
HOME="$HKG" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "an upgrade replaces the old review-gate Stop entry, not duplicates it" "1 5" \
   "$(jq -r '[ ([.hooks.Stop[].hooks[] | select(.command | test("/scripts/review-gate$"))] | length),
             ([.hooks.Stop[].hooks[] | select(.command | test("/scripts/review-gate$")) | .timeout] | add) ] | join(" ")' \
       "$HKG/.claude/settings.json")"
case "$(cat "$HKG/.claude/settings.json")" in
    *'/usr/local/bin/mywrap --log /scripts/review-gate-audit.log'*)
        ok "a foreign command merely containing the review-gate path survives (anchored, not substring)" ;;
    *) bad "a foreign command merely containing the review-gate path survives (anchored, not substring)" \
         "mywrap --log /scripts/review-gate-audit.log still present" "$(cat "$HKG/.claude/settings.json")" ;;
esac
rm -rf "$HKG"
# review-gate's PreToolUse entry is no longer shipped: an older install's
# entry is removed, its emptied group with it, and nothing else in PreToolUse.
HKP=$(mktmpd); mkdir -p "$HKP/.claude"
cat > "$HKP/.claude/settings.json" <<'EOF'
{ "hooks": { "PreToolUse": [
    { "matcher": "", "hooks": [ { "type": "command", "command": "\"${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}\"/scripts/agent-state running" } ] },
    { "matcher": "Bash", "hooks": [ { "type": "command", "command": "\"${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}\"/scripts/review-gate --pretool", "timeout": 5 } ] },
    { "matcher": "Bash", "hooks": [ { "type": "command", "command": "my-bash-guard" } ] }
  ] } }
EOF
HOME="$HKP" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "an upgrade removes the old review-gate --pretool entry" "0" \
   "$(jq '[.hooks[][].hooks[].command | select(test("review-gate --pretool"))] | length' "$HKP/.claude/settings.json")"
is "and leaves agent-state running once, the user's Bash hook, and no empty group" "1 1 2" \
   "$(jq -r '[ ([.hooks.PreToolUse[].hooks[].command | select(test("/scripts/agent-state running"))] | length),
             ([.hooks.PreToolUse[] | select(. == {matcher: "Bash", hooks: [{type: "command", command: "my-bash-guard"}]})] | length),
             (.hooks.PreToolUse | length) ] | join(" ")' "$HKP/.claude/settings.json")"
before=$(cat "$HKP/.claude/settings.json")
HOME="$HKP" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "and a second run changes nothing" "$before" "$(cat "$HKP/.claude/settings.json")"
rm -rf "$HKP"
# The pre-#166 SessionStart `agent-state off` had an empty matcher, so it also ran
# on compact; an upgrade must replace it, not keep it beside the new one.
HKS=$(mktmpd); mkdir -p "$HKS/.claude"
cat > "$HKS/.claude/settings.json" <<'EOF'
{ "hooks": {
    "SessionStart": [
      { "matcher": "", "hooks": [ { "type": "command", "command": "\"${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}\"/scripts/agent-state off" } ] },
      { "matcher": "", "hooks": [ { "type": "command", "command": "my-session-start" } ] }
    ],
    "SessionEnd": [ { "matcher": "", "hooks": [ { "type": "command", "command": "my-session-end" } ] } ] } }
EOF
HOME="$HKS" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "an upgrade replaces the empty-matcher SessionStart agent-state off" "1 startup|resume|clear" \
   "$(jq -r '[.hooks.SessionStart[] | select(any(.hooks[]; .command | test("/scripts/agent-state off$")))]
             | "\(length) \(map(.matcher) | join(","))"' "$HKS/.claude/settings.json")"
is "and the user's own SessionStart and SessionEnd hooks are untouched" "1 1" \
   "$(jq -r '[ ([.hooks.SessionStart[] | select(. == {matcher: "", hooks: [{type: "command", command: "my-session-start"}]})] | length),
             ([.hooks.SessionEnd[] | select(. == {matcher: "", hooks: [{type: "command", command: "my-session-end"}]})] | length) ] | join(" ")' \
       "$HKS/.claude/settings.json")"
rm -rf "$HKS"
# dry-run: shows the diff, writes nothing
HK2=$(mktmpd); mkdir -p "$HK2/.claude"; printf '{}' > "$HK2/.claude/settings.json"
out=$(HOME="$HK2" "$DIR/install.sh" --hooks --dry-run 2>&1)
case "$out" in *agent-state*) ok "dry-run previews the change" ;;
    *) bad "dry-run previews the change" "a diff naming agent-state" "$out" ;; esac
is "dry-run wrote nothing" "{}" "$(cat "$HK2/.claude/settings.json")"
# the diff header must name the real settings.json path, not the process
# substitution's /dev/fd/NN plumbing.
case "$out" in *"$HK2/.claude/settings.json"*) ok "and the diff header names the real file" ;;
    *) bad "and the diff header names the real file" "$HK2/.claude/settings.json in the header" "$out" ;; esac
rm -rf "$HK2"

# and dry-run predicts "already merged" exactly like the real run would --
# not an empty diff plus "nothing written" for a file that has nothing left
# to preview.
HK16=$(mktmpd); mkdir -p "$HK16/.claude"
printf '{"model":"opus"}' > "$HK16/.claude/settings.json"
HOME="$HK16" "$DIR/install.sh" --hooks >/dev/null 2>&1
out=$(HOME="$HK16" "$DIR/install.sh" --hooks --dry-run 2>&1)
case "$out" in *"already merged, nothing to do"*) ok "dry-run on an already-merged file predicts the real outcome" ;;
    *) bad "dry-run on an already-merged file predicts the real outcome" "already merged, nothing to do" "$out" ;; esac
rm -rf "$HK16"

# I1: --dry-run used to run the symlink loop, mkdir ~/.claude, and seed
# settings.json before ever reaching the dry gate -- a preview on a totally
# fresh HOME created three links and a config dir, then printed "dry-run:
# nothing written" underneath them.
HK4=$(mktmpd); BIN4=$(mktmpd)
out=$(XDG_BIN_HOME="$BIN4" HOME="$HK4" "$DIR/install.sh" --hooks --dry-run 2>&1)
case "$out" in *agent-state*) ok "dry-run on a fresh home still previews the change" ;;
    *) bad "dry-run on a fresh home still previews the change" "a diff naming agent-state" "$out" ;; esac
is "and it creates no symlinks" "" "$(ls -A "$BIN4" 2>/dev/null)"
if [ -e "$HK4/.claude" ]; then
    bad "and it creates no ~/.claude" "no .claude directory" "one was created"
else
    ok "and it creates no ~/.claude"
fi
rm -rf "$HK4" "$BIN4"

# M5: a dangling settings.json symlink used to pass the file-exists guard,
# and the seed write followed it -- writing outside ~/.claude entirely, at
# whatever the link happened to point to.
HK11=$(mktmpd); mkdir -p "$HK11/.claude"
ln -s "$HK11/nowhere.json" "$HK11/.claude/settings.json"
HOME="$HK11" "$DIR/install.sh" --hooks >/dev/null 2>&1
if [ -e "$HK11/nowhere.json" ]; then
    bad "a dangling settings.json symlink is never written through" "no file at the link target" "one was created"
else
    ok "a dangling settings.json symlink is never written through"
fi
rm -rf "$HK11"

# and a VALID settings.json symlink (dotfiles tracked via chezmoi,
# home-manager, or by hand) keeps being a symlink after the merge, at its
# original mode -- the write lands at the resolved target, not at the link.
HK14=$(mktmpd); mkdir -p "$HK14/.claude" "$HK14/dotfiles"
printf '{"model":"opus"}' > "$HK14/dotfiles/settings.json"
chmod 644 "$HK14/dotfiles/settings.json"
ln -s "$HK14/dotfiles/settings.json" "$HK14/.claude/settings.json"
HOME="$HK14" "$DIR/install.sh" --hooks >/dev/null 2>&1
if [ -L "$HK14/.claude/settings.json" ]; then
    ok "a valid settings.json symlink survives the merge"
else
    bad "a valid settings.json symlink survives the merge" "still a symlink" "replaced by a regular file"
fi
case "$(cat "$HK14/dotfiles/settings.json")" in
    *tmux-cockpit*agent-state*) ok "and the merge landed at the resolved (dotfiles) target" ;;
    *) bad "and the merge landed at the resolved (dotfiles) target" "an agent-state command" "$(head -c200 "$HK14/dotfiles/settings.json")" ;;
esac
mode=$(stat -c '%a' "$HK14/dotfiles/settings.json" 2>/dev/null || stat -f '%Lp' "$HK14/dotfiles/settings.json" 2>/dev/null)
is "and the resolved target keeps its original mode" "644" "$mode"
rm -rf "$HK14"

# I2: a pre-existing settings.json.bak is the user's, not ours to overwrite.
HK5=$(mktmpd); mkdir -p "$HK5/.claude"
printf '{"sentinel":"user-backup"}' > "$HK5/.claude/settings.json.bak"
printf '{"model":"opus"}' > "$HK5/.claude/settings.json"
out=$(HOME="$HK5" "$DIR/install.sh" --hooks 2>&1)
case "$(cat "$HK5/.claude/settings.json.bak")" in
    *user-backup*) ok "a pre-existing .bak is never overwritten" ;;
    *) bad "a pre-existing .bak is never overwritten" "user-backup still present" "$(cat "$HK5/.claude/settings.json.bak")" ;;
esac
# and the success message must not claim a fresh backup it never wrote --
# pointing at a rollback that is actually the user's unrelated .bak content.
case "$out" in *"(backup:"*) bad "and it does not claim a fresh backup" "no (backup: ...) claim" "$out" ;;
    *"existing backup kept"*) ok "and it says the existing backup was kept, not overwritten" ;;
    *) bad "and it says the existing backup was kept, not overwritten" "existing backup kept" "$out" ;;
esac
rm -rf "$HK5"

# I2 (cont.): and once WE'VE written a .bak, a later genuinely-modifying run
# must not replace the pristine original in it with already-merged content.
HK6=$(mktmpd); mkdir -p "$HK6/.claude"
printf '{"model":"opus"}' > "$HK6/.claude/settings.json"
HOME="$HK6" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "the first .bak holds the pristine original" '{"model":"opus"}' "$(cat "$HK6/.claude/settings.json.bak")"
# Drop one cockpit hook so the second run is genuinely modifying, not a
# no-op: a reformat alone (jq -S) would still compare string-identical here
# and never reach the write branch this test is aimed at.
jq 'del(.hooks.Stop)' "$HK6/.claude/settings.json" > "$HK6/.claude/trimmed.json" && mv "$HK6/.claude/trimmed.json" "$HK6/.claude/settings.json"
HOME="$HK6" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "and a second MODIFYING run still leaves the original in .bak" '{"model":"opus"}' "$(cat "$HK6/.claude/settings.json.bak")"
rm -rf "$HK6"

# C1: the write must be atomic (mktemp + rename), never a truncate-in-place --
# a SIGXFSZ mid-write (reproduced with ulimit -f 1) used to leave a truncated,
# invalid settings.json with rollback never running, because by then the
# original was already gone.
HK9=$(mktmpd); mkdir -p "$HK9/.claude"
printf '{"model":"opus"}' > "$HK9/.claude/settings.json"
HOME="$HK9" "$DIR/install.sh" --hooks >/dev/null 2>&1
leftover=$(find "$HK9/.claude" -maxdepth 1 -name 'settings.json.*' ! -name 'settings.json.bak' 2>/dev/null)
is "no temp file is left behind after a successful merge" "" "$leftover"
rm -rf "$HK9"

# C1 (the actual regression pin): "no temp file after success" above also
# passes against the unfixed, non-atomic install.sh -- printf > "$settings"
# leaves no temp file either, because it never created one. Reproduce the
# real failure mode instead: a file-size limit low enough that the write
# itself dies mid-flight (SIGXFSZ), the same mechanism named in the PR's own
# ulimit -f 1 repro. The subshell scopes the limit to this one process tree.
HK15=$(mktmpd); mkdir -p "$HK15/.claude"
printf '{"model":"opus"}' > "$HK15/.claude/settings.json"
before=$(cat "$HK15/.claude/settings.json")
(ulimit -f 1; HOME="$HK15" "$DIR/install.sh" --hooks) >/dev/null 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then
    bad "a write killed mid-flight (ulimit -f 1) does not exit 0" "a nonzero exit" "0"
else
    ok "a write killed mid-flight (ulimit -f 1) does not exit 0"
fi
is "and settings.json is left as the untouched original" "$before" "$(cat "$HK15/.claude/settings.json")"
rm -rf "$HK15"

# I3: index() must be jq's deep, order-insensitive equality -- a plain
# reformat (jq -S, or Claude Code rewriting its own settings) must not make
# every cockpit hook look new and get appended a second time.
HK7=$(mktmpd); mkdir -p "$HK7/.claude"
printf '{"model":"opus"}' > "$HK7/.claude/settings.json"
HOME="$HK7" "$DIR/install.sh" --hooks >/dev/null 2>&1
jq -S . "$HK7/.claude/settings.json" > "$HK7/.claude/reformatted.json" && mv "$HK7/.claude/reformatted.json" "$HK7/.claude/settings.json"
HOME="$HK7" "$DIR/install.sh" --hooks >/dev/null 2>&1
# Counted as duplicates WITHIN an event, not as "more than one group": Stop
# ships two distinct groups (agent-state done, review-gate), and a length test
# would call that legitimate pair a duplication.
dupes=$(jq '[.hooks[] | length - (unique | length)] | add' "$HK7/.claude/settings.json")
is "a reformat-then-merge leaves exactly one cockpit entry per event" "0" "$dupes"
rm -rf "$HK7"

# C1 (fail-closed pin): a malformed settings.json must abort byte-identical,
# never partially merged or silently reseeded.
HK10=$(mktmpd); mkdir -p "$HK10/.claude"
printf '{not valid json' > "$HK10/.claude/settings.json"
before=$(cat "$HK10/.claude/settings.json")
HOME="$HK10" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "a malformed settings.json aborts with exit 1" "1" "$?"
is "and it is left byte-identical" "$before" "$(cat "$HK10/.claude/settings.json")"
if [ -e "$HK10/.claude/settings.json.bak" ]; then
    bad "and no .bak is created on a failed merge" "no .bak" "one was created"
else
    ok "and no .bak is created on a failed merge"
fi
rm -rf "$HK10"

# I2: --hooks reads settings.json once, merges (real jq wall time), then
# writes -- an unlocked read-modify-write. Claude Code rewrites its own
# settings.json on every permission approval, so a --hooks run racing a live
# session can silently drop that write. A real race is ~54ms wide -- too
# narrow to hit reliably from a test -- so a slow-jq PATH shim widens it: it
# sleeps only when called with -s (the merge's own slurp call; the .hooks
# diff calls just above use -S, a different flag, and stay fast), giving a
# background writer time to land mid-merge.
REALJQ=$(command -v jq)
JQSHIM=$(mktmpd)
JQMARK="$STATE/jq-slurp-mark"
rm -f "$JQMARK"
cat > "$JQSHIM/jq" <<EOF
#!/bin/sh
for a in "\$@"; do
    [ "\$a" = "-s" ] && { printf 'slurping\n' > "$JQMARK"; sleep 1; }
done
exec "$REALJQ" "\$@"
EOF
chmod +x "$JQSHIM/jq"

HKR=$(mktmpd); mkdir -p "$HKR/.claude"
printf '{"model":"opus"}' > "$HKR/.claude/settings.json"
(PATH="$JQSHIM:$PATH" HOME="$HKR" "$DIR/install.sh" --hooks >/dev/null 2>&1) &
instpid=$!
# The shim's own marker, not a guessed head start: the concurrent write below
# has to land while the merge is inside the slurp, which is what widens the race.
until_file "$JQMARK" '*slurping*'
# The concurrent writer: a live Claude Code session rewriting settings.json
# mid-merge, e.g. after a permission approval.
printf '{"model":"opus","concurrent_marker":"added-by-race"}' > "$HKR/.claude/settings.json"
wait "$instpid"
instrc=$?
is "a concurrent settings.json write survives a racing --hooks merge" "added-by-race" \
   "$(jq -r '.concurrent_marker // empty' "$HKR/.claude/settings.json" 2>/dev/null)"
if [ "$instrc" -eq 0 ]; then
    bad "the raced run reports failure, not a false success" "a nonzero exit" "0"
else
    ok "the raced run reports failure, not a false success"
fi
# A second, non-racing run must then merge cleanly on top of the concurrent
# writer's content -- the abort is a "re-run", not a dead end.
HOME="$HKR" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "and a follow-up run merges the concurrent write and cockpit's hooks together" "added-by-race" \
   "$(jq -r '.concurrent_marker // empty' "$HKR/.claude/settings.json" 2>/dev/null)"
case "$(cat "$HKR/.claude/settings.json")" in
    *agent-state*) ok "and cockpit's hooks are present too" ;;
    *) bad "and cockpit's hooks are present too" "an agent-state command" "$(cat "$HKR/.claude/settings.json")" ;;
esac
rm -rf "$HKR" "$JQSHIM"
else
    skip_ "hooks merge (jq not installed)"
fi
# --uninstall does NOT touch hooks -- the combination must be a hard usage
# error, not a silent skip of --hooks. This is a pure argument-parsing exit,
# so it needs no jq and must never create or touch settings.json.
HK3=$(mktmpd)
HOME="$HK3" "$DIR/install.sh" --uninstall --hooks >/dev/null 2>&1
is "--uninstall --hooks is a usage error" "2" "$?"
if [ -e "$HK3/.claude/settings.json" ]; then
    bad "--uninstall --hooks touches no settings.json" "no settings.json" "one was created"
else
    ok "--uninstall --hooks touches no settings.json"
fi
rm -rf "$HK3"

# I4: --uninstall on a read-only $BIN used to exit 0 and print nothing when
# rm failed -- the mirror image of the install-side FAILED handling above.
HK8=$(mktmpd); BIN8=$(mktmpd)
XDG_BIN_HOME="$BIN8" HOME="$HK8" "$DIR/install.sh" >/dev/null 2>&1
chmod 555 "$BIN8"
out=$(XDG_BIN_HOME="$BIN8" HOME="$HK8" "$DIR/install.sh" --uninstall 2>&1)
rc_uninstall=$?
chmod 755 "$BIN8"
is "a read-only BIN makes --uninstall exit 1" "1" "$rc_uninstall"
case "$out" in *FAILED*) ok "and it reports FAILED, not silent success" ;;
    *) bad "and it reports FAILED, not silent success" "FAILED in the output" "$out" ;; esac
if [ -L "$BIN8/tm" ]; then
    ok "and the links are still present"
else
    bad "and the links are still present" "tm symlink present" "missing"
fi
rm -rf "$HK8" "$BIN8"

# M7: --help/-h must succeed, not be treated as an unknown option.
out=$("$DIR/install.sh" --help 2>&1)
is "--help exits 0" "0" "$?"
case "$out" in *usage:*) ok "and it prints usage" ;;
    *) bad "and it prints usage" "a usage: line" "$out" ;; esac

rm -rf "$HK"
echo "== prefix u jumps to the pane that needs you =="
if need_tmux "tmux-jump"; then :; else
# The bind runs the script under run-shell, which sets no $TMUX_PANE: read
# from there the cycle never knew where it stood and a press went nowhere.
# Only the real key shows it; typing the script into a pane has a real pane.
JK="cockpit-jk-$$"; JKO="cockpit-jko-$$"; SOCKS="$SOCKS $JK $JKO"
"$TMUX_REAL" -L "$JK" -f /dev/null new-session -d -s t -x 80 -y 24 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$JK" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$JK" new-window -t t: 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$JK" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
for w in 0 1; do "$TMUX_REAL" -L "$JK" set-option -p -t "t:$w" @panestate needs-input >/dev/null 2>&1; done
"$TMUX_REAL" -L "$JK" select-window -t t:0 >/dev/null 2>&1
"$TMUX_REAL" -L "$JKO" -f /dev/null new-session -d -s o -x 80 -y 26 "TERM=xterm-256color exec $TMUX_REAL -L $JK attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$JK" list-clients -F '#{client_name}'
"$TMUX_REAL" -L "$JKO" send-keys -t o C-b u
until_out 1 "$TMUX_REAL" -L "$JK" display-message -p -t t: '#{window_index}'
is "prefix u from the first blocked pane cycles to the second (#45)" "1" \
   "$("$TMUX_REAL" -L "$JK" display-message -p -t t: '#{window_index}')"
"$TMUX_REAL" -L "$JKO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$JK" kill-server >/dev/null 2>&1
JJ="cockpit-jj-$$"; SOCKS="$SOCKS $JJ"
"$TMUX_REAL" -L "$JJ" -f /dev/null new-session -d -s t -x 80 -y 24 >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
# -t t: (not bare t): a detached window's default name is the literal "tmux"
# until a client renames it, and new-window's colonless -t resolves as a
# WINDOW target first -- "t" then prefix-matches "tmux" and tmux tries to
# reuse window 0 instead of appending. The trailing colon pins the target to
# the session, sidestepping that ambiguity.
"$TMUX_REAL" -L "$JJ" new-window  -t t: 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" new-window  -t t: 'exec -a claude bash' >/dev/null 2>&1
JP2=$("$TMUX_REAL" -L "$JJ" list-panes -t t:1 -F '#{pane_id}')
JP3=$("$TMUX_REAL" -L "$JJ" list-panes -t t:2 -F '#{pane_id}')
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP3" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" select-window -t t:0 >/dev/null 2>&1
# Drive from inside the session so $TMUX/$TMUX_PANE are real, like a keypress.
# tmux-jump forks a fresh nested tmux client per invocation here, so wait on
# the window index the assertion reads rather than guess at that fork's cost.
JP1=$("$TMUX_REAL" -L "$JJ" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'" Enter
for i in $(seq 1 40); do
    [ "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')" = 2 ] && break
    sleep 0.1
done
is "jump lands on the blocked window" "2" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')"
# A second blocked pane: pressing again from the first cycles to the next.
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP3" "'$DIR/scripts/tmux-jump'" Enter
until_out 1 "$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}'
is "a second press cycles to the next blocked pane" "1" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')"
# A shell pane holding stale (or hand-set) @panestate must never be treated as
# blocked -- the same glob guard that keeps it out of @cockpit_fleet. Pressing
# from the LAST agent pane in cycle order (JP3) proves it: if the stale pane
# were counted, cycling would land on IT (the highest pane id, sorted last);
# with the fix it wraps back to the first real agent pane instead.
JP4=$("$TMUX_REAL" -L "$JJ" new-window -t t: -P -F '#{pane_id}' 'sleep 300' 2>/dev/null)
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP4" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP3" "'$DIR/scripts/tmux-jump'" Enter
until_out 1 "$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}'
is "a shell holding stale state is never jumped to" "1" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')"
# Nothing blocked, but one agent holds a review the gate flagged: that is
# still somewhere you have to go, so prefix u walks to it -- after the blocked
# panes and after the ones holding a delivered review, never before them.
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP3" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @cockpit_review pending >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" select-window -t t:0 >/dev/null 2>&1
# The marker poll, not a fixed sleep: the typed line's own "JPR=$?" has no
# digit after the =, so only the finished command's output matches.
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'; echo JPR=\$?" Enter
until_cap "$JJ" "$JP1" '*JPR=[0-9]*'
is "prefix u walks to a pane with a pending review when nothing is blocked" "1" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')"
"$TMUX_REAL" -L "$JJ" set-option -pu -t "$JP2" @cockpit_review >/dev/null 2>&1
# Nothing blocked: the script must not move you, and must not error.
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP3" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" select-window -t t:0 >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'; echo JRC=\$?" Enter
until_cap "$JJ" "$JP1" '*JRC=[0-9]*'
case "$("$TMUX_REAL" -L "$JJ" capture-pane -p -t "$JP1")" in
    *JRC=0*) ok "no blocked pane exits 0 and stays put" ;;
    *) bad "no blocked pane exits 0 and stays put" "JRC=0 in the pane" \
         "$("$TMUX_REAL" -L "$JJ" capture-pane -p -t "$JP1" | tail -3)" ;;
esac
is "and the window did not change" "0" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')"
"$TMUX_REAL" -L "$JJ" kill-server >/dev/null 2>&1
fi

echo "== the fleet counter reads every session; each session keeps its own count =="
if need_tmux "fleet counter"; then :; else
FC="cockpit-fc-$$"; SOCKS="$SOCKS $FC"
# Unredirected stderr, deliberately: a `can't find window: 1` from this setup
# used to be swallowed and resurface three assertions later as a short count.
"$TMUX_REAL" -L "$FC" -f /dev/null new-session -d -s a -x 80 -y 24 'exec -a claude sleep 300' >/dev/null
"$TMUX_REAL" -L "$FC" new-session -d -s b 'exec -a claude sleep 300' >/dev/null
# A window is named after whatever its pane runs at creation, so under load
# either of these reads "bash" until the exec lands, and a bare "b" window
# target prefix-matches that name and resolves to index 0 instead of session b.
"$TMUX_REAL" -L "$FC" rename-window -t a:0 bash >/dev/null
"$TMUX_REAL" -L "$FC" rename-window -t b:0 bash >/dev/null
"$TMUX_REAL" -L "$FC" new-window  -t b: 'exec -a claude sleep 300' >/dev/null
"$TMUX_REAL" -L "$FC" set -g @cockpit_agents_glob '|claude|' >/dev/null
"$TMUX_REAL" -L "$FC" set -g status-right '[#{E:@cockpit_fleet}]' >/dev/null
is "the fleet fixture really has three agent panes" "3" \
   "$("$TMUX_REAL" -L "$FC" list-panes -a -F '#{pane_id}' | grep -c .)"
FCP1=$("$TMUX_REAL" -L "$FC" list-panes -t a:0 -F '#{pane_id}')
FCP2=$("$TMUX_REAL" -L "$FC" list-panes -t b:0 -F '#{pane_id}')
FCP3=$("$TMUX_REAL" -L "$FC" list-panes -t b:1 -F '#{pane_id}')
# Drive state through agent-state, the production path: the count is now a
# precomputed @cockpit_fleet option agent-state writes on every transition, not
# a live walk in status-right, so a raw set-option would never move the counter.
asFC() { TMUX="$SOCKDIR/$FC,0,0" "$DIR/scripts/agent-state" "$1" "$2" >/dev/null 2>&1; }
asFC needs-input "$FCP1"
asFC needs-input "$FCP2"
asFC error       "$FCP3"
fleet() { "$TMUX_REAL" -L "$FC" display-message -p -t "$FCP1" \
            "$("$TMUX_REAL" -L "$FC" show-option -gv status-right)" | plain; }
is "two blocked and one errored, across sessions" "[▲2 ✕1 ]" "$(fleet)"
# The bar's "elsewhere" mark is the fleet count minus this session's, so each
# session must hold only its own.
fccount() { "$TMUX_REAL" -L "$FC" display-message -p -t "$1" \
              '#{@cockpit_fleet_nb} #{@cockpit_fleet_ne} #{@cockpit_sess_nb} #{@cockpit_sess_ne}'; }
is "session a: fleet 2 waiting and 1 failed, its own 1 waiting" "2 1 1 0" "$(fccount a:)"
is "session b: its own 1 waiting and 1 failed" "2 1 1 1" "$(fccount b:)"
asFC running "$FCP1"
asFC running "$FCP2"
asFC running "$FCP3"
is "a quiet fleet renders nothing at all" "[]" "$(fleet)"
# A pane whose agent has exited keeps its last @panestate forever. Unguarded,
# the fleet count would include it and never come back down. agent-state writes
# @panestate regardless, but the precompute counts through the same glob guard,
# so a shell pane never reaches the counter.
"$TMUX_REAL" -L "$FC" new-window -t b: 'sleep 300' >/dev/null 2>&1
FCP4=$("$TMUX_REAL" -L "$FC" list-panes -t b:2 -F '#{pane_id}')
asFC needs-input "$FCP4"
is "a shell holding stale state is not counted" "[]" "$(fleet)"
"$TMUX_REAL" -L "$FC" kill-server >/dev/null 2>&1
fi

echo "== the statusline shim records context health =="
if need_tmux "agent-context"; then :; else
CX="cockpit-cx-$$"; SOCKS="$SOCKS $CX"
"$TMUX_REAL" -L "$CX" -f /dev/null new-session -d -s t 'sleep 300' >/dev/null 2>&1
CXP=$("$TMUX_REAL" -L "$CX" list-panes -t t -F '#{pane_id}')
# agent-context calls plain `tmux`, which the suite's PATH shim sends to
# SUITE_SOCK whenever $TMUX is unset -- a different server than this test's
# own private CX socket. A real hook always runs with $TMUX set (tmux exports
# it into every pane), so set it here too: that is what points the shimmed
# `tmux` at this pane's actual server, exactly as it would be in a real pane.
CXTMUX="$SOCKDIR/$CX,0,0"
TMUX="$CXTMUX" TMUX_PANE="$CXP" "$DIR/scripts/agent-context" < "$DIR/tests/fixtures/statusline.json" >/dev/null
ctx=$("$TMUX_REAL" -L "$CX" show-option -pqv -t "$CXP" @panestate_ctx)
case "$ctx" in
    [0-9]|[0-9][0-9]|100) ok "a real payload yields a percent" ;;
    *) bad "a real payload yields a percent" "an integer 0-100" "$ctx" ;;
esac
# The fallback branch sums cache tokens that can legitimately exceed
# context_window_size (1900/1000 here) -- the write must still clamp to the
# promised 0-100, not the raw ratio.
"$TMUX_REAL" -L "$CX" set-option -pu -t "$CXP" @panestate_ctx >/dev/null 2>&1
is "the model and the percent come from the one parse" "Fable 5 · ctx 100%" \
   "$(TMUX="$CXTMUX" TMUX_PANE="$CXP" "$DIR/scripts/agent-context" \
        < "$DIR/tests/fixtures/statusline-overshoot.json")"
is "an overshooting fallback ratio clamps to 100" "100" \
   "$("$TMUX_REAL" -L "$CX" show-option -pqv -t "$CXP" @panestate_ctx)"
# used_percentage counts input tokens only (the statusline docs); the fallback
# must too, or 300 in + 500 out of 1000 reads 80% instead of 30%.
printf '%s' '{"context_window":{"context_window_size":1000,"current_usage":{"input_tokens":100,"output_tokens":500,"cache_creation_input_tokens":100,"cache_read_input_tokens":100}}}' \
    | TMUX="$CXTMUX" TMUX_PANE="$CXP" "$DIR/scripts/agent-context" >/dev/null
is "the fallback counts input tokens, not output" "30" \
   "$("$TMUX_REAL" -L "$CX" show-option -pqv -t "$CXP" @panestate_ctx)"
# Garbage must write nothing and still exit 0 -- a statusline that errors
# would spam the agent's own UI.
"$TMUX_REAL" -L "$CX" set-option -pu -t "$CXP" @panestate_ctx >/dev/null 2>&1
printf 'not json at all' | TMUX="$CXTMUX" TMUX_PANE="$CXP" "$DIR/scripts/agent-context" >/dev/null; cxrc=$?
is "garbage exits 0" "0" "$cxrc"
is "and stores nothing" "" "$("$TMUX_REAL" -L "$CX" show-option -pqv -t "$CXP" @panestate_ctx)"
# A writer that stalls must not wedge the statusline: whole-read deadline.
# A plain `{ stall } | script` pipe is not a reliable way to prove this: on
# this shell/kernel a foreground writer inside the SAME pipeline delays the
# reader's own read -t SIGALRM until the writer actually exits, regardless of
# the timeout -- confirmed with no script involved at all. agent-state's own
# "must not hang on silent stdin" test sidesteps exactly this by holding the
# writer open on a FIFO from a BACKGROUNDED process instead; that reliably
# lets the deadline fire on schedule, so this test does the same.
CXFIFO="$STATE/agent-context-stall.fifo"; mkfifo "$CXFIFO"
( exec 9>"$CXFIFO"; sleep 8 ) & holder=$!
start=$SECONDS
TMUX="$CXTMUX" TMUX_PANE="$CXP" "$DIR/scripts/agent-context" >/dev/null 2>&1 < "$CXFIFO"
took=$(( SECONDS - start ))
kill "$holder" 2>/dev/null; wait "$holder" 2>/dev/null
rm -f "$CXFIFO"
if [ "$took" -le 3 ]; then ok "a stalling writer is abandoned in time"
else bad "a stalling writer is abandoned in time" "<=3s" "${took}s"; fi
"$TMUX_REAL" -L "$CX" kill-server >/dev/null 2>&1
fi

echo "== the statusline shim skips the write when context fill is unchanged =="
# Claude Code calls statusLine every render. Every `tmux set-option`, even -p
# on an unchanged value, forces a full server-wide redraw (measured: 5
# identical writes to @panestate_ctx -> 5 redraws; the read that agent-state
# already does for @panestate costs 0). Two calls with the SAME payload must
# issue exactly one set-option, not two. The compare and the write are one tmux
# call, so a server hook counts the writes and a PATH shim counts the calls.
if need_tmux "agent-context write guard"; then :; else
CX2="cockpit-cx2-$$"; SOCKS="$SOCKS $CX2"
"$TMUX_REAL" -L "$CX2" -f /dev/null new-session -d -s t 'sleep 300' >/dev/null 2>&1
CXP2=$("$TMUX_REAL" -L "$CX2" list-panes -t t -F '#{pane_id}')
CXTMUX2="$SOCKDIR/$CX2,0,0"
CXS=$(mktmpd)
CXLOG="$CXS/setlog"; : > "$CXLOG"; CXCALLS="$CXS/calls"; : > "$CXCALLS"
"$TMUX_REAL" -L "$CX2" set-hook -g after-set-option \
    "run-shell 'echo \"#{hook_arguments}\" >> $CXLOG'" >/dev/null 2>&1
cat > "$CXS/tmux" <<SHIMEOF
#!/bin/sh
printf '%s\n' "\$*" >> "$CXCALLS"
exec "$TMUX_REAL" "\$@"
SHIMEOF
chmod +x "$CXS/tmux"
PATH="$CXS:$PATH" TMUX="$CXTMUX2" TMUX_PANE="$CXP2" "$DIR/scripts/agent-context" \
    < "$DIR/tests/fixtures/statusline.json" >/dev/null
is "the first call writes the new value" "1" "$(wc -l < "$CXLOG")"
PATH="$CXS:$PATH" TMUX="$CXTMUX2" TMUX_PANE="$CXP2" "$DIR/scripts/agent-context" \
    < "$DIR/tests/fixtures/statusline.json" >/dev/null
is "a second call with the same value writes nothing" "1" "$(wc -l < "$CXLOG")"
is "and each call is one tmux invocation" "2" "$(wc -l < "$CXCALLS")"
is "the stored value is still correct" "4" \
   "$("$TMUX_REAL" -L "$CX2" show-option -pqv -t "$CXP2" @panestate_ctx)"
"$TMUX_REAL" -L "$CX2" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --lines: the diff rows a review anchors to =="
RV1=$(mktmpd)
(
  cd "$RV1" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  mkdir -p tests
  printf 'a\nb\nc\n' > src.go
  printf 'x\ny\nz\n' > tests/foo_test.sh
  printf 'gone1\ngone2\n' > deleteme.txt
  git add -A
  git commit -q -m init
  printf 'a\nCHANGED\nc\n' > src.go
  printf 'x\nCHANGED\nz\n' > tests/foo_test.sh
  rm deleteme.txt
) >/dev/null 2>&1

rv1out=$("$DIR/scripts/tmux-review" --lines "$RV1")
is "test file's rows come first" "tests/foo_test.sh" "$(printf '%s\n' "$rv1out" | head -1 | cut -f1)"

rv1bad=""
while IFS=$'\t' read -r p l f _d; do
    [ -n "$p" ] || continue
    case "$l" in
        -[0-9]*|[0-9]*) ;;
        *) rv1bad="bad line field for $p: [$l]" ;;
    esac
    [ -n "$f" ] || rv1bad="empty fragment for $p:$l"
done <<<"$rv1out"
is "every row carries path, numeric line, non-empty fragment" "" "$rv1bad"

case "$rv1out" in
    *"deleteme.txt"$'\t'-[0-9]*$'\t'*) ok "a deletion row's line is the OLD number, prefixed -" ;;
    *) bad "a deletion row's line is the OLD number, prefixed -" \
         "a deleteme.txt row with a -N line field" "$rv1out" ;;
esac
rm -rf "$RV1"

echo "== tmux-review --lines: staged and unstaged changes are one review, not either/or =="
# `git diff` alone answers about the WORKING TREE only, so the moment one
# unstaged edit existed the whole index went missing from the list -- while
# --preview-diff, which falls back to --cached, still showed the staged file.
# The list and the preview disagreed about the same file. `git diff HEAD` is
# both halves in one call. FAIL-first: on the old code no staged.txt row exists.
RVMIX=$(mktmpd)
(
  cd "$RVMIX" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\n' > staged.txt; printf 'a\n' > unstaged.txt
  git add -A; git commit -q -m init
  printf 'a\nSTAGED-CHANGE\n' > staged.txt; git add staged.txt
  printf 'a\nUNSTAGED-CHANGE\n' > unstaged.txt
) >/dev/null 2>&1
rvmix=$("$DIR/scripts/tmux-review" --lines "$RVMIX")
case "$rvmix" in *"staged.txt"$'\t'2$'\t'"STAGED-CHANGE"*) ok "the staged half is reviewable" ;;
    *) bad "the staged half is reviewable" "a staged.txt:2 STAGED-CHANGE row" "$rvmix" ;; esac
case "$rvmix" in *"unstaged.txt"$'\t'2$'\t'"UNSTAGED-CHANGE"*) ok "and so is the unstaged half" ;;
    *) bad "and so is the unstaged half" "an unstaged.txt:2 row" "$rvmix" ;; esac
case "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVMIX/staged.txt")" in
    *STAGED-CHANGE*) ok "the preview agrees with the list about the staged file" ;;
    *) bad "the preview agrees with the list about the staged file" "STAGED-CHANGE in the preview" \
         "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVMIX/staged.txt")" ;; esac
# The directory branch of --preview-diff used to render an
# unstaged-only `git diff --stat`, disagreeing with the list (which shows
# staged+unstaged+untracked as one set since the fix above): staged.txt is
# already reported by `git status --short` in the same output, so a mere
# substring check for its name does not discriminate the bug -- the STAT
# SUMMARY's file count does. FAIL-first: `git diff --stat` (unstaged only)
# counts one file here, `git diff --stat HEAD` (staged+unstaged) counts two.
case "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVMIX")" in
    *"2 files changed"*) ok "the directory preview's stat counts the staged file too" ;;
    *) bad "the directory preview's stat counts the staged file too" "2 files changed in the preview" \
         "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVMIX")" ;; esac
rm -rf "$RVMIX"

echo "== tmux-review --lines: a brand-new file is reviewable, line by line =="
# An untracked file carries no diff at all, so the whole new-code case -- the
# code most worth reviewing -- was invisible to the reviewer. Its rows are
# synthesised from the file itself, every line a + row at its own number.
RVNEW=$(mktmpd)
(
  cd "$RVNEW" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > seed.txt; git add -A; git commit -q -m init
  printf 'first\nsecond\n' > new.txt
  printf 'NEW-LINE\n' > 'café-new.txt'
) >/dev/null 2>&1
rvnew=$("$DIR/scripts/tmux-review" --lines "$RVNEW")
case "$rvnew" in *"new.txt"$'\t'1$'\t'"first"*"new.txt"$'\t'2$'\t'"second"*) ok "untracked lines are + rows with their own numbers" ;;
    *) bad "untracked lines are + rows with their own numbers" "new.txt:1 first, new.txt:2 second" "$rvnew" ;; esac
# ls-files C-quotes a non-ASCII path the same way diff headers do ("caf\303\251-new.txt"),
# and a quoted name is not a file that exists: [ -f ] failed and the file was
# dropped from the list WITHOUT a word -- while --preview-diff, which asks with
# core.quotePath=false, rendered it. The same list-vs-preview disagreement this
# whole block exists to end. FAIL-first: no café-new.txt row at all.
case "$rvnew" in *'café-new.txt'$'\t'1$'\t'"NEW-LINE"*) ok "a new file with a non-ASCII name is listed too, not silently dropped" ;;
    *) bad "a new file with a non-ASCII name is listed too, not silently dropped" \
         "café-new.txt<TAB>1<TAB>NEW-LINE" "$rvnew" ;; esac
# A literal TAB in an untracked path. `ls-files -z` does NOT C-quote -- the -z
# form emits raw bytes, quotePath or not -- so the tab reached the row list as
# a real field separator and every downstream `awk -F'\t'` mis-split the row:
# path=has, line=tab.txt, fragment=the file's first line, offset=+. attach()
# guards only "path non-empty" and "not the cap sentinel", so that row pinned
# and composed into an instruction about a file that does not exist at a line
# that is not a number, and --preview-window=+{5}-/2 got "+" for an offset.
# The wire format is tab-separated: a path carrying a tab cannot go on it, so
# it is dropped -- the same thing already done for binaries and unreadable
# files. (The old `IFS=$'\t' read` row loop collapsed the doubled tab and
# dropped the file silently; the awk rewrite turned it into a plausible,
# DELIVERABLE row, which is worse.)
# FAIL-first: a "has<TAB>tab.txt<TAB>3body line" row is emitted.
printf 'body line\n' > "$RVNEW/has$(printf '\t')tab.txt"
rvtab=$("$DIR/scripts/tmux-review" --lines "$RVNEW")
is "an untracked path holding a literal TAB is dropped, not split into a row" \
   "0" "$(printf '%s\n' "$rvtab" | awk -F'\t' '$1 == "has"' | grep -c .)"
is "...and no row anywhere carries a piece of its name" \
   "0" "$(printf '%s\n' "$rvtab" | grep -c 'tab\.txt')"
case "$rvtab" in *"new.txt"$'\t'1$'\t'"first"*"new.txt"$'\t'2$'\t'"second"*)
        ok "the untracked file beside it still gets both its rows" ;;
    *) bad "the untracked file beside it still gets both its rows" \
         "new.txt:1 first, new.txt:2 second" "$rvtab" ;; esac
rm -rf "$RVNEW"

# group: serial
echo "== tmux-review --lines: an untracked tree is walked once, not four forks per file =="
# The untracked synthesis walked every file FOUR times over -- [ -f ],
# grep -Iq, wc -l and awk -- and did it twice, once in the numstat loop that
# feeds the cap and once in the row loop. rows_mode calls lines_mode, so the
# review popup re-paid the whole cost after every pinned comment. Measured with
# 800 one-line untracked files: the whole `--lines` run took 6.885s before and
# takes 0.134s now (the walk itself 7.9s -> 0.008s in the review's own
# measurement; master, which ignored untracked files entirely, was 0.018s), off
# the single batched `ls-files -z | xargs grep -Il .` list both loops now share.
# The per-file walk is 3200 forks, so CPU is where it shows: 1.4s against a
# measured 0.46s under twelve spinners.
RVMANY=$(mktmpd)
(
  cd "$RVMANY" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > seed.txt; git add -A; git commit -q -m init
  mkdir -p vendor
  i=1; while [ "$i" -le 800 ]; do printf 'line\n' > "vendor/f$i.txt"; i=$((i+1)); done
) >/dev/null 2>&1
cpu_under "800 untracked files list in well under 1.4s of CPU" 1.4 \
    "$DIR/scripts/tmux-review" --lines "$RVMANY"
is "each untracked file still contributes its one + row" \
   "800" "$("$DIR/scripts/tmux-review" --lines "$RVMANY" | grep -c .)"
rm -rf "$RVMANY"

echo "== tmux-review --lines: every row knows where it sits in its file's preview =="
# The preview opened every file at its top, so reviewing a change 900 lines down
# meant scrolling by hand for every row. Each row now carries a 5th field: its
# own 1-based line inside `tmux-open-path --preview-diff <file>` (one stat line,
# then the diff from that file's first @@), which the loop hands to fzf as the
# preview scroll offset. FAIL-first: rows have four fields, so $5 is empty.
RVOFF=$(mktmpd)
(
  cd "$RVOFF" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  seq 1 40 > f.txt; git add -A; git commit -q -m init
  sed -i '3s/.*/THREE/; 30s/.*/THIRTY/' f.txt
) >/dev/null 2>&1
rvoff=$("$DIR/scripts/tmux-review" --lines "$RVOFF" | awk -F'\t' '$3=="THIRTY" {print $5}')
want=$(( 1 + $(git -C "$RVOFF" diff HEAD -- f.txt | awk '/^@@/{h=1} h' | grep -n '^+THIRTY' | cut -d: -f1) ))
is "the second hunk's row offset matches the preview's line" "$want" "$rvoff"
rm -rf "$RVOFF"

# An untracked file's preview has the same two prefixes as a tracked one -- the
# stat line AND the hunk header of its --no-index diff -- so its rows sit two
# lines below their own file line, not one.
RVOFFN=$(mktmpd)
(
  cd "$RVOFFN" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > seed.txt; git add -A; git commit -q -m init
  printf 'first\nsecond\nthird\n' > new.txt
) >/dev/null 2>&1
rvoffn=$("$DIR/scripts/tmux-review" --lines "$RVOFFN" | awk -F'\t' '$1=="new.txt" && $3=="second" {print $5}')
rvoffnwant=$( (cd "$RVOFFN" && "$DIR/scripts/tmux-open-path" --preview-diff new.txt) \
    | awk '{ gsub(/\033\[[0-9;]*m/, ""); print }' | grep -n '│ +second' | cut -d: -f1)
is "an untracked row's offset is its real line in that file's preview" "$rvoffnwant" "$rvoffn"
rm -rf "$RVOFFN"

echo "== tmux-open-path --preview-diff: old and new line numbers on every hunk line, one prefix per line, ▸ on the asked line =="
RPG=$(mktmpd)
(
  cd "$RPG" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  seq 1 40 > f.txt; git add -A; git commit -q -m init
  sed -i '3s/.*/THREE/; 30s/.*/THIRTY/; 10d' f.txt
) >/dev/null 2>&1
rpg=$( (cd "$RPG" && "$DIR/scripts/tmux-open-path" --preview-diff f.txt) | sed 's/\x1b\[[0-9;]*m//g')
rpgraw=$(git -C "$RPG" diff HEAD -- f.txt | awk '/^@@/{h=1} h')
is "the gutter adds no line and removes none" "$(( $(printf '%s\n' "$rpgraw" | wc -l) + 1 ))" "$(printf '%s\n' "$rpg" | wc -l)"
is "a context line carries both numbers"   "     1      1 │  1"      "$(printf '%s\n' "$rpg" | grep '│  1$')"
is "a removed line carries the old number" "     3        │ -3"      "$(printf '%s\n' "$rpg" | grep '│ -3$')"
is "an added line carries the new number"  "            3 │ +THREE" "$(printf '%s\n' "$rpg" | grep '│ +THREE$')"
is "a deletion keeps the old side counting" "    10        │ -10"    "$(printf '%s\n' "$rpg" | grep '│ -10$')"
is "and the new side skips it"              "    11     10 │  11"    "$(printf '%s\n' "$rpg" | grep '│  11$')"
rpgm=$( (cd "$RPG" && "$DIR/scripts/tmux-open-path" --preview-diff f.txt 5) | sed 's/\x1b\[[0-9;]*m//g' | sed -n 5p)
case "$rpgm" in "▸"*) ok "the asked line is marked ▸" ;; *) bad "the asked line is marked ▸" "▸..." "$rpgm" ;; esac
rpgc=$( (cd "$RPG" && "$DIR/scripts/tmux-open-path" --preview-diff f.txt) | sed 's/\x1b\[[0-9;]*m//g' | sed -n 5p)
case "$rpgc" in "▸"*) bad "the mark is not cached" "an unmarked line" "$rpgc" ;; *) ok "the mark is not cached" ;; esac
# Every check above strips ANSI first, so a gutter that printed the stripped
# copy instead of the raw line would pass them all with the colour gone. Peel
# the 13-column prefix back off and the diff's own bytes must be left.
rpgrawc=$(git -C "$RPG" diff --color=always HEAD -- f.txt | awk '/^\033\[[0-9;]*m@@|^@@/{h=1} h')
rpgcol=$( (cd "$RPG" && "$DIR/scripts/tmux-open-path" --preview-diff f.txt) | tail -n +2 | sed 's/^.\{13\} │ //')
is "the gutter is a prefix and nothing else, colour included" "$rpgrawc" "$rpgcol"
# %5 overflowed past 9999 lines, and ▶ (ambiguous-width) then overwrote a
# digit: a 12340 read ▶2340 (#22 item 7). A missing trailing newline is a
# gutter row of its own (test debt from the same issue).
(cd "$RPG" && export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e && seq 1 12345 > big.txt && git add big.txt && git commit -q -m big && sed -i '12340s/.*/X/' big.txt && printf 'tail' >> big.txt) >/dev/null 2>&1
rpgb=$( (cd "$RPG" && "$DIR/scripts/tmux-open-path" --preview-diff big.txt 6) | sed 's/\x1b\[[0-9;]*m//g')
is "a five-digit line number keeps every digit next to the mark" "▸12340        │ -12340" "$(printf '%s\n' "$rpgb" | grep '│ -12340$')"
is "and a no-newline marker gets a blank gutter" "              │ \\ No newline at end of file" "$(printf '%s\n' "$rpgb" | grep 'No newline' | tail -1)"
rm -rf "$RPG"

# group: serial
echo "== tmux-review --lines: a giant diff caps to one notice instead of wedging the popup =="
# The review popup is modal and runs lines_mode at startup and on every reload.
# A regenerated lockfile / vendored bundle is a single-file diff of tens of
# thousands of rows; the per-row shell parse used to lock the one client for
# minutes to hours (measured: 100k rows did not finish in 5 minutes, 8k rows
# = 56s). numstat totals the exact row count at one cheap git call, so past
# the 5000-row cap --lines emits a single "too large" notice and returns at
# once. FAIL-first: on the pre-cap parser this same fixture takes ~30s (the
# timeout below fires) and never emits the notice string at all.
RVCAP=$(mktmpd)
(
  cd "$RVCAP" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > lock.txt
  git add -A; git commit -q -m init
  # 12000 lines replacing the 1 seed line: 12000 added + 1 deleted = 12001
  # rows, over the 5000 cap. On the old parser that is ~20s of shell (measured
  # 6.2s at 6000 rows, 21.2s at 12000), so timeout below has teeth.
  seq 1 12000 > lock.txt
) >/dev/null 2>&1
rvcaptotal=$(git -C "$RVCAP" diff --numstat | awk -F'\t' '{a+=$1; d+=$2} END{print a+d}')
is "the fixture's diff really exceeds the 5000-row cap" "12001" "$rvcaptotal"
# timeout is the fail-first teeth: the fixed path returns in ~0.1s, the old
# O(n) shell build of 12001 rows blows well past 10s and timeout kills it (124).
rvcapout=$(timeout 10 "$DIR/scripts/tmux-review" --lines "$RVCAP"); rvcaprc=$?
if [ "$rvcaprc" -eq 124 ]; then
    bad "a 12001-row diff returns fast, not after a 10s+ parse" \
        "output within 10s" "timed out (the pre-cap parser)"
else
    ok "a 12001-row diff returns fast, not after a 10s+ parse"
fi
case "$rvcapout" in
    *"diff too large to anchor line-by-line (12001 changed lines)"*)
        ok "the cap emits the too-large notice naming the row count" ;;
    *) bad "the cap emits the too-large notice naming the row count" \
         "a 'diff too large ... (12001 changed lines)' notice" "$rvcapout" ;;
esac
is "the capped output is exactly one row (the popup stays trivially small)" \
   "1" "$(printf '%s\n' "$rvcapout" | grep -c .)"
# The notice is a row like any other and reaches the same 5-field split in
# rows_mode, so it carries an offset too rather than arriving one field short.
is "the cap notice carries the offset field too, so no row is short" \
   "5" "$(printf '%s' "$rvcapout" | awk -F'\t' '{print NF}')"
# A diff at or under the cap is untouched -- still fully parsed, row per line.
RVUNDER=$(mktmpd)
(
  cd "$RVUNDER" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > small.txt
  git add -A; git commit -q -m init
  seq 1 100 > small.txt
) >/dev/null 2>&1
rvunderout=$("$DIR/scripts/tmux-review" --lines "$RVUNDER")
case "$rvunderout" in
    *"diff too large"*) bad "an under-cap diff is parsed normally, not capped" \
         "real per-line rows, no notice" "$rvunderout" ;;
    *"small.txt"$'\t'*) ok "an under-cap diff is parsed normally, not capped" ;;
    *) bad "an under-cap diff is parsed normally, not capped" \
         "small.txt rows" "$rvunderout" ;;
esac
rm -rf "$RVCAP" "$RVUNDER"

# Counting is cheap; CAPTURING is what costs. The cap above only helped once
# `diff=$(tree_diff "$dir")` had already run, so a tree the cap was built to
# refuse was still read whole into a bash variable first. Measured here on the
# fixture below (30 regenerated lockfiles, 200k rows each, 38 MB on disk):
# 3.25s and 222 MB peak RSS to produce one notice row, of which `git diff
# --numstat` -- everything the cap decision actually needs -- is 0.75s and
# 32 MB. So numstat comes first, the cap decides, and the full capture happens
# only for a diff that is going to be parsed.
#
# 30 moderate files rather than the one 51 MB lockfile the plan measured: a
# single file that large makes git's OWN diff the bottleneck (365 MB RSS in
# numstat alone), which no ordering can fix, and hides the thing being removed.
# Spread over 30 files git's peak stays at one file's worth and the 222 MB is
# the shell's captured copy -- exactly what the reorder deletes.
# FAIL-first: before the reorder the full `git diff` ran before numstat, so
# the shim log below shows a diff call without --numstat. `timeout 1.5`
# (3.25 s pre-fix) went red under peer load and took the notice check with it
# through an empty rvlock.out (issue #32); every timeout in this block is a
# hang guard now, and the shim is what pins the reorder.
RVLOCK=$(mktmpd)
(
  cd "$RVLOCK" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  for i in $(seq 1 30); do printf 'seed\n' > "lock$i.json"; done
  git add -A; git commit -q -m init
  for i in $(seq 1 30); do seq 1 200000 > "lock$i.json"; done
) >/dev/null 2>&1
RVLOCK_SHIM=$(mktmpd); RVLOCK_LOG="$STATE/rvlock-git-calls"; : > "$RVLOCK_LOG"
cat > "$RVLOCK_SHIM/git" <<SHIMEOF
#!/bin/sh
printf '%s\n' "\$*" >> "$RVLOCK_LOG"
exec "$(command -v git)" "\$@"
SHIMEOF
chmod +x "$RVLOCK_SHIM/git"
if PATH="$RVLOCK_SHIM:$PATH" timeout 10 "$DIR/scripts/tmux-review" --lines "$RVLOCK" > "$STATE/rvlock.out" 2>/dev/null; then
    ok "a 6,000,030-row tree answers with a notice"
else
    bad "a 6,000,030-row tree answers with a notice" "exit 0 within 10s" "rc $?"
fi
# "1" is the positive control: an empty log (shim never reached) must not
# read as "nothing captured".
is "the capped tree is counted once and never captured" "1 0" \
   "$(grep -cE '(^| )diff( |$)' "$RVLOCK_LOG") $(grep -E '(^| )diff( |$)' "$RVLOCK_LOG" | grep -vc -- '--numstat')"
case "$(cat "$STATE/rvlock.out")" in
    *"diff too large to anchor line-by-line (6000030 changed lines)"*)
        ok "the uncaptured cap still names the exact row count numstat totalled" ;;
    *) bad "the uncaptured cap still names the exact row count numstat totalled" \
         "a '(6000030 changed lines)' notice" "$(head -c 200 "$STATE/rvlock.out")" ;;
esac
rm -rf "$RVLOCK"

# Rows are not the only way a diff gets too big to hand a reviewer: 4000 rows
# is comfortably under the 5000-row cap, but at 1.5 KB a row that is 5.7 MB of
# text -- captured, parsed into 4001 shell rows, and rendered into an fzf list
# whose every entry is a 1500-column line. So the cap counts BYTES too, 4 MB
# across the changed files (`wc -c` on the worktree copy, the HEAD blob's size
# for a deletion), and says which limit it hit.
RVWIDE=$(mktmpd)
(
  cd "$RVWIDE" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > wide.txt
  git add -A; git commit -q -m init
  awk 'BEGIN{ s=sprintf("%1500s",""); gsub(/ /,"w",s); for(i=0;i<4000;i++) print s }' > wide.txt
) >/dev/null 2>&1
is "the wide fixture is UNDER the row cap, so only bytes can trip it" "4001" \
   "$(git -C "$RVWIDE" diff --numstat | awk -F'\t' '{a+=$1; d+=$2} END{print a+d}')"
rvwideout=$("$DIR/scripts/tmux-review" --lines "$RVWIDE")
case "$rvwideout" in
    *"diff too large"*" MB)"*)
        ok "4000 rows of 1.5 KB each cap on bytes, and the notice says MB not lines" ;;
    *) bad "4000 rows of 1.5 KB each cap on bytes, and the notice says MB not lines" \
         "a too-large notice naming MB" "$(head -c 200 <<<"$rvwideout")" ;;
esac
is "the byte cap's notice is one row, same shape as the row cap's" \
   "1" "$(printf '%s\n' "$rvwideout" | grep -c .)"
is "the byte cap's notice carries the offset field too, so no row is short" \
   "5" "$(printf '%s' "$rvwideout" | awk -F'\t' '{print NF}')"
# A wide file that stays UNDER 4 MB is still parsed row by row -- the byte cap
# must not swallow an ordinary diff that merely has long lines.
RVNARROW=$(mktmpd)
(
  cd "$RVNARROW" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > narrow.txt
  git add -A; git commit -q -m init
  awk 'BEGIN{ s=sprintf("%1500s",""); gsub(/ /,"w",s); for(i=0;i<100;i++) print s }' > narrow.txt
) >/dev/null 2>&1
case "$("$DIR/scripts/tmux-review" --lines "$RVNARROW")" in
    *"diff too large"*) bad "a 150 KB wide-line diff is under the byte cap and parsed normally" \
         "real per-line rows, no notice" "capped" ;;
    *"narrow.txt"$'\t'*) ok "a 150 KB wide-line diff is under the byte cap and parsed normally" ;;
    *) bad "a 150 KB wide-line diff is under the byte cap and parsed normally" \
         "narrow.txt rows" "no rows at all" ;;
esac
rm -rf "$RVWIDE" "$RVNARROW"

# ...and the byte cap has to measure the DIFF, not the FILE. A whole-file
# `wc -c` refused a one-line version bump in a big lockfile: 4.6 MB of file,
# two changed rows, a diff of about 70 bytes -- nothing expensive about it,
# and the popup answered "review it directly" for a change a reviewer could
# read at a glance. A per-file average -- `size * (add + del) / max(lines, 1)`
# -- was tried next: the wide fixture above still estimated 5.7 MB and capped,
# this one estimated ~74 bytes and did not (that estimator's own failure mode
# is covered below).
# FAIL-first: on the whole-file cap this returns "(4.5 MB)" and no rows.
RVBUMP=$(mktmpd)
(
  cd "$RVBUMP" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  # 130k fixed-width lines: 4,810,000 bytes, well over the 4 MB cap as a file.
  awk 'BEGIN { for (i = 1; i <= 130000; i++) printf "%036d\n", i }' > package-lock.json
  git add -A; git commit -q -m init
  # One line rewritten at the SAME width, so the file's size is unchanged and
  # only the estimate can tell the two fixtures apart.
  awk 'BEGIN { for (i = 1; i <= 130000; i++) printf "%036d\n", (i == 5 ? 42 : i) }' > package-lock.json
) >/dev/null 2>&1
is "the bump fixture is over the 4 MB cap when measured as a whole file" \
   "4810000" "$(wc -c < "$RVBUMP/package-lock.json")"
is "...while its diff is two rows" "2" \
   "$(git -C "$RVBUMP" diff --numstat | awk -F'\t' '{print $1 + $2}')"
rvbumpout=$(timeout 10 "$DIR/scripts/tmux-review" --lines "$RVBUMP")
case "$rvbumpout" in
    *"diff too large"*)
        bad "a one-line bump in a 4.6 MB lockfile is still reviewable" \
            "two anchored package-lock.json rows" "$(head -c 120 <<<"$rvbumpout")" ;;
    *"package-lock.json"$'\t'*)
        ok "a one-line bump in a 4.6 MB lockfile is still reviewable" ;;
    *) bad "a one-line bump in a 4.6 MB lockfile is still reviewable" \
         "two anchored package-lock.json rows" "$(head -c 120 <<<"$rvbumpout")" ;;
esac
is "the bump is exactly its two rows, nothing else" "2" \
   "$(printf '%s\n' "$rvbumpout" | grep -c .)"
rm -rf "$RVBUMP"

# ...and the byte cap has to MEASURE the diff, not estimate it. Two estimators
# were tried here and both broke, in opposite directions:
#
#   size * (add+del) / lines   -- a per-file AVERAGE. On the fixture below
#     (100,000 one-byte lines plus one 5 MB line, only the long line changed)
#     it answers 103 bytes and lets a 10 MB capture through: measured 0.91s,
#     82,148 kB RSS, two 5 MB rows, in the MODAL popup, for a two-row review.
#
#   max(that, maxlen * (add+del))  -- the average, floored by the widest line.
#     It catches the fixture below, but it charges that width to EVERY changed
#     row of any file that merely CONTAINS a long line, and the cap is a
#     per-review TOTAL: a 744-byte, 26-row review was refused as "(6.8 MB)"
#     with an unrelated one-line Go change lost along with it (the block after
#     this one). It also needs `wc -L`, and on a wc without it the whole sized
#     map came back empty and the byte cap disappeared in silence -- measured,
#     4000 rows parsed in 1.02s where the cap had fired before.
#
# So the diff is measured EXACTLY, and bounded so that measuring cannot itself
# be the expensive thing: capture once through `head -c cap+1`, let git die on
# SIGPIPE past that, and what comes back either fits -- and then IS the diff,
# with no second git call -- or proves the cap is exceeded without ever having
# held more than 4 MB. Untracked files have no diff to bound, so their exact
# `wc -c` size is summed and charged first.
# FAIL-first against the max() estimate: BOTH cases below -- it refuses the
# short-line edit that this must show, and the block after it.
RVSKEW=$(mktmpd)
(
  cd "$RVSKEW" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  # tr, not awk's sprintf: mawk's sprintf buffer is 8192 bytes and dies on a
  # 5 MB %*s ("program limit exceeded").
  { awk 'BEGIN { for (i = 0; i < 100000; i++) print "x" }'
    head -c 5000000 /dev/zero | tr '\0' 'L'; printf '\n'; } > mix.txt
  git add -A; git commit -q -m init
  # Only the long line changes, at the same width: the file's size and line
  # count are identical, so only the estimator can tell the two apart.
  { awk 'BEGIN { for (i = 0; i < 100000; i++) print "x" }'
    head -c 5000000 /dev/zero | tr '\0' 'M'; printf '\n'; } > mix.txt
) >/dev/null 2>&1
is "the mixed fixture is two numstat rows, so the ROW cap cannot be what fires" \
   "2" "$(git -C "$RVSKEW" diff --numstat | awk -F'\t' '{print $1 + $2}')"
is "...and its per-file AVERAGE estimate is 103 bytes, far under the 4 MB cap" \
   "103" "$(wc -lc < "$RVSKEW/mix.txt" | awk '{printf "%d\n", $2 * 2 / $1}')"
rvskewout=$(timeout 10 "$DIR/scripts/tmux-review" --lines "$RVSKEW" 2>/dev/null)
case "$rvskewout" in
    *"diff too large"*" MB)"*)
        ok "one 5 MB line among 100k short ones caps on bytes, not on the average" ;;
    *) bad "one 5 MB line among 100k short ones caps on bytes, not on the average" \
         "a too-large notice naming MB" "$(head -c 120 <<<"$rvskewout")" ;;
esac
is "the skewed notice is one row, not two 5 MB ones" \
   "1" "$(printf '%s\n' "$rvskewout" | grep -c .)"
# The other half of the same fixture, and the half an estimate cannot get
# right: the SHORT line changed instead. The diff is a couple of hundred
# bytes and belongs on screen -- but `maxlen * (add+del)` reads the 5 MB line
# it did not touch and refuses the review anyway. Exactly the same file, the
# only difference is which line moved.
{ awk 'BEGIN { for (i = 0; i < 100000; i++) print (i == 5 ? "y" : "x") }'
  head -c 5000000 /dev/zero | tr '\0' 'L'; printf '\n'; } > "$RVSKEW/mix.txt"
is "...and with only a SHORT line changed it is two numstat rows too" \
   "2" "$(git -C "$RVSKEW" diff --numstat | awk -F'\t' '{print $1 + $2}')"
rvskewshort=$(timeout 10 "$DIR/scripts/tmux-review" --lines "$RVSKEW" 2>/dev/null)
case "$rvskewshort" in
    *"diff too large"*) bad "a SHORT-line edit in the same skewed file is shown, not refused" \
         "two anchored mix.txt rows" "$(head -c 120 <<<"$rvskewshort")" ;;
    *"mix.txt"$'\t'*) ok "a SHORT-line edit in the same skewed file is shown, not refused" ;;
    *) bad "a SHORT-line edit in the same skewed file is shown, not refused" \
         "two anchored mix.txt rows" "no rows at all" ;;
esac
is "and it is exactly its two rows" "2" \
   "$(printf '%s\n' "$rvskewshort" | grep -c .)"
rm -rf "$RVSKEW"

# The over-refusal in its realistic shape, and the reason it is Important
# rather than cosmetic: the cap is a per-review TOTAL, so one skewed file
# takes every OTHER file's rows down with it. A JS file of 500 short lines
# plus one 300 KB data-URI line, 12 short lines edited, beside a one-line Go
# change in a file that has nothing to do with it.
#
#   real diff : 26 rows, 744 bytes
#   max()     : 1 row -- "(diff too large) … (6.8 MB) — review it directly"
#   exact     : 26 rows, both main.go rows among them
#
# FAIL-first: the notice, and no main.go rows at all.
RVCROSS=$(mktmpd)
(
  cd "$RVCROSS" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  { awk 'BEGIN { for (i = 1; i <= 500; i++) printf "const v%d = %d;\n", i, i }'
    printf 'const IMG = "data:image/png;base64,'
    head -c 300000 /dev/zero | tr '\0' 'A'; printf '";\n'; } > assets.js
  printf 'package main\n\nfunc main() { println("one") }\n' > main.go
  git add -A; git commit -q -m init
  awk '{ if (NR <= 12 && /^const v/) sub(/= [0-9]+;/, "= 999;"); print }' assets.js > .t
  mv .t assets.js
  printf 'package main\n\nfunc main() { println("two") }\n' > main.go
) >/dev/null 2>&1
is "the cross-file fixture is 26 changed rows, well under the row cap" "26" \
   "$(git -C "$RVCROSS" diff --numstat | awk -F'\t' '{a += $1 + $2} END {print a}')"
is "...and its whole real diff is well under 4 MB" "1" \
   "$(git -C "$RVCROSS" diff | wc -c | awk '{print ($1 < 4194304) ? 1 : 0}')"
rvcrossout=$(timeout 10 "$DIR/scripts/tmux-review" --lines "$RVCROSS" 2>/dev/null)
case "$rvcrossout" in
    *"diff too large"*) bad "a 744-byte review is not refused for containing one long line" \
         "26 anchored rows" "$(head -c 120 <<<"$rvcrossout")" ;;
    *) ok "a 744-byte review is not refused for containing one long line" ;;
esac
is "every row is there, both files" "26" "$(printf '%s\n' "$rvcrossout" | grep -c .)"
is "and the unrelated Go file is not lost with the skewed one" "2" \
   "$(printf '%s\n' "$rvcrossout" | awk -F'\t' '$1 == "main.go"' | grep -c .)"
rm -rf "$RVCROSS"

# The untracked half of the same measure. An untracked file has no diff to
# bound -- every byte of it is an added row -- so it is sized exactly, with
# one batched `wc -c` over the same operand list the row walk uses, and
# charged BEFORE the capture: an unignored 6 MB text file drop must cap on
# its own, without a capture happening at all. Nothing else in the suite
# exercises that sum (the 800-untracked-file block above is one line each).
RVUBIG=$(mktmpd)
(
  cd "$RVUBIG" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > seed.txt; git add -A; git commit -q -m init
  awk 'BEGIN{ s=sprintf("%1500s",""); gsub(/ /,"u",s); for(i=0;i<4000;i++) print s }' > huge.txt
) >/dev/null 2>&1
is "the untracked fixture is 6 MB on disk, and tracked-clean" "6004000" \
   "$(wc -c < "$RVUBIG/huge.txt")"
rvubigout=$(timeout 10 "$DIR/scripts/tmux-review" --lines "$RVUBIG" 2>/dev/null)
case "$rvubigout" in
    *"diff too large"*) ok "a 6 MB untracked file caps on its own exact size" ;;
    *) bad "a 6 MB untracked file caps on its own exact size" \
         "a too-large notice" "$(head -c 120 <<<"$rvubigout")" ;;
esac
is "and that notice is one row, not 4000 untracked ones" "1" \
   "$(printf '%s\n' "$rvubigout" | grep -c .)"
rm -rf "$RVUBIG"

# A tracked BINARY is one LINE of diff, not its size. Without --binary git
# prints "Binary files a/x and b/x differ" and nothing else, whatever the file
# weighs: the fixture below is a rebuilt 5 MB asset beside a one-line code
# change and its whole diff is 240 bytes. Charging the cap the asset's size
# blanked the review of the code change sitting next to it -- the one thing
# there was to review. A "-\t-" row costs the width of that one line, 80
# bytes, and is sized no further.
RVBIN=$(mktmpd)
(
  cd "$RVBIN" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  head -c 5000000 /dev/urandom > asset.bin
  printf 'alpha\nbeta\ngamma\n' > app.go
  git add -A; git commit -q -m init
  head -c 5000000 /dev/urandom > asset.bin
  printf 'alpha\nBETA\ngamma\n' > app.go
) >/dev/null 2>&1
is "the asset really is binary to git, so its numstat row is the '-' one" \
   "-	-	asset.bin" "$(git -C "$RVBIN" diff --numstat HEAD -- asset.bin)"
is "...and the whole diff, 5 MB asset included, is nowhere near the 4 MB cap" \
   "small" "$( [ "$(git -C "$RVBIN" diff --no-color HEAD | wc -c)" -lt 4194304 ] \
              && echo small || echo large )"
rvbinout=$("$DIR/scripts/tmux-review" --lines "$RVBIN")
case "$rvbinout" in
    *"diff too large"*)
        bad "a rebuilt 5 MB asset does not blank the code change beside it" \
            "app.go's two anchored rows" "$(head -c 120 <<<"$rvbinout")" ;;
    *"app.go"$'\t'*)
        ok "a rebuilt 5 MB asset does not blank the code change beside it" ;;
    *) bad "a rebuilt 5 MB asset does not blank the code change beside it" \
         "app.go's two anchored rows" "$(head -c 120 <<<"$rvbinout")" ;;
esac
is "the binary itself contributes no rows, so the output is app.go's two" \
   "2" "$(printf '%s\n' "$rvbinout" | grep -c .)"
# A DELETED binary is the same "-\t-" row with no worktree copy, so it used to
# be charged its whole HEAD blob by the deletion path instead -- the same hole,
# one branch over.
rm -f "$RVBIN/asset.bin"
case "$("$DIR/scripts/tmux-review" --lines "$RVBIN")" in
    *"diff too large"*)
        bad "a DELETED 5 MB asset does not blank it either" \
            "app.go's two anchored rows" "capped" ;;
    *"app.go"$'\t'*) ok "a DELETED 5 MB asset does not blank it either" ;;
    *) bad "a DELETED 5 MB asset does not blank it either" \
         "app.go's two anchored rows" "no rows at all" ;;
esac
rm -rf "$RVBIN"

# group: serial
echo "== tmux-review --lines: builds rows without a fork or a quadratic append per line =="
# The row build used `disp=$(printf ...)` (a subshell fork per row) and
# `group[path]+=<row>` into one associative-array element (bash rebuilds the
# whole growing string on each append -> O(n^2) for a single file). Both are
# gone: printf -v, and one indexed array with per-file [start,end] ranges.
# Correctness: the output is locked to a golden that the pre-fix parser
# produced byte-for-byte (verified out of band), so the refactor cannot shift
# a byte -- ordering, the -N deletion line, the display column, all intact.
RVGOLD=$(mktmpd)
(
  cd "$RVGOLD" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  mkdir -p tests
  printf 'a\nb\nc\n' > app.go
  printf 'p\nq\n' > tests/unit_test.go
  git add -A; git commit -q -m init
  printf 'a\nZED\nc\nADDED\n' > app.go
  printf 'p\nQQ\n' > tests/unit_test.go
) >/dev/null 2>&1
# The trailing column is the row's line in `--preview-diff <file>` output
# (one stat line, then the diff from that file's first @@). Hand-derived and
# cross-checked against the preview itself: app.go renders -b at line 4, +ZED
# at 5, +ADDED at 7; unit_test.go renders -q at 4 and +QQ at 5.
rvgoldwant=$(printf '%s\n' \
  'tests/unit_test.go	-2	q	tests/unit_test.go:2 (-)  q	4' \
  'tests/unit_test.go	2	QQ	tests/unit_test.go:2 (+)  QQ	5' \
  'app.go	-2	b	app.go:2 (-)  b	4' \
  'app.go	2	ZED	app.go:2 (+)  ZED	5' \
  'app.go	4	ADDED	app.go:4 (+)  ADDED	7')
is "the rebuilt --lines output is byte-identical to the pre-fix parser" \
   "$rvgoldwant" "$("$DIR/scripts/tmux-review" --lines "$RVGOLD")"
rm -rf "$RVGOLD"

# Perf: a 5000-row single-file diff (the largest that stays under the cap) is
# ~0.2s now; the pre-fix O(n^2)+fork build was ~5s here and ~13s on the slower
# hunt machine. The budget is CPU, 3x this site's 0.211s under twelve spinners
# (0.146-0.235s idle) -- a wall tooth grades the box, not the build (#75).
RVPERF=$(mktmpd)
(
  cd "$RVPERF" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  : > big.txt
  git add -A; git commit -q -m init
  seq 1 5000 > big.txt
) >/dev/null 2>&1
cpu_under "a 5000-row under-cap diff parses in well under 0.7s of CPU" 0.7 \
    "$DIR/scripts/tmux-review" --lines "$RVPERF"
rvperfrows=$("$DIR/scripts/tmux-review" --lines "$RVPERF" | grep -c .)
is "the 5000-row diff is fully parsed, not capped (5000 rows, no notice)" \
   "5000" "$rvperfrows"
rm -rf "$RVPERF"

# Perf: the rows themselves come out of awk, not a bash `read` loop. The row
# build was four bash passes over the same data (a churn map, the row build,
# the test partition, two emit loops) and bash pays an interpreter step per
# row for every one of them, so the whole function cost 4x its own diff
# parse. Measured here on this 4900-row single-file diff, ten runs each:
# 195-227ms with the bash build, 59-74ms with the awk one -- of which the
# git diff and the parsing awk, both of which were always there, are 44ms.
# So the row build fell from ~165ms to ~19ms.
#
# The tooth is 0.4s of CPU (#75): 0.15-0.17s idle here and 0.23-0.26s under
# both twelve and twenty-four spinners, so it plateaus. It bites the bash
# build less often than the old 0.2s wall tooth did, which is the trade --
# that tooth also rejected the awk build on any box with a peer job running.
RV49=$(mktmpd)
(
  cd "$RV49" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  : > f
  git add -A; git commit -q -m init
  seq 1 4900 > f
) >/dev/null 2>&1
cpu_under "4900 rows build in under 0.4s of CPU" 0.4 \
    "$DIR/scripts/tmux-review" --lines "$RV49"
is "and all 4900 rows are there" "4900" \
   "$("$DIR/scripts/tmux-review" --lines "$RV49" | grep -c .)"
rm -rf "$RV49"

# group: serial
echo "== tmux-review --lines: a non-ASCII filename anchors its own rows, not the previous file's =="
# git C-quotes non-ASCII paths by default (`--- \"a/caf\\303\\251.txt\"`), which
# the bare `a/`/`b/` header match could not read -- so café.txt's rows were
# attributed to the file BEFORE it (or dropped when café was first). Diffing
# with core.quotePath=false keeps the path literal. FAIL-first: on the old code
# café's rows carry alpha.txt / zeta.txt and no row says café at all.
RVQ=$(mktmpd)
(
  cd "$RVQ" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > alpha.txt
  printf 'x\ny\n' > café.txt
  git add -A; git commit -q -m init
  printf 'a\nAA\n' > alpha.txt
  printf 'x\nCC\n' > café.txt
) >/dev/null 2>&1
rvqout=$("$DIR/scripts/tmux-review" --lines "$RVQ")
case "$rvqout" in
    *'café.txt'$'\t''2'$'\t''CC'$'\t'*) ok "café's added row anchors to café.txt at café's own line 2" ;;
    *) bad "café's added row anchors to café.txt at café's own line 2" \
         "café.txt<TAB>2<TAB>CC" "$rvqout" ;;
esac
if printf '%s\n' "$rvqout" | grep -q '^alpha\.txt	.*CC'; then
    bad "café's row is not smeared onto the previous file" \
        "no alpha.txt row carrying café's CC" "$rvqout"
else
    ok "café's row is not smeared onto the previous file"
fi
# git C-quotes non-ASCII paths in `status --short` too, so a directory preview
# (the git status --short / git diff --stat branch) used to render
# "caf\303\251.txt" instead of the real name. FAIL-first: the octal escape
# shows up in the directory preview's output.
case "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVQ")" in
    *'\303'*) bad "the directory preview prints the name, not octal" "café in the preview" \
         "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVQ")" ;;
    *) ok "the directory preview prints the name, not octal" ;; esac
rm -rf "$RVQ"

# The first-file-in-diff variant: when the odd-named file leads the diff, the
# old code had no previous file to fall back to and dropped its rows entirely.
RVQ1=$(mktmpd)
(
  cd "$RVQ1" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'x\ny\n' > café.txt
  printf 'a\nb\n' > zeta.txt
  git add -A; git commit -q -m init
  printf 'x\nCC\n' > café.txt
  printf 'a\nAA\n' > zeta.txt
) >/dev/null 2>&1
rvq1out=$("$DIR/scripts/tmux-review" --lines "$RVQ1")
case "$rvq1out" in
    *'café.txt'$'\t''2'$'\t''CC'$'\t'*) ok "a leading non-ASCII file's rows are not dropped" ;;
    *) bad "a leading non-ASCII file's rows are not dropped" \
         "café.txt<TAB>2<TAB>CC" "$rvq1out" ;;
esac
case "$rvq1out" in
    *'zeta.txt'$'\t''2'$'\t''AA'$'\t'*) ok "the following file still anchors to itself" ;;
    *) bad "the following file still anchors to itself" \
         "zeta.txt<TAB>2<TAB>AA" "$rvq1out" ;;
esac
rm -rf "$RVQ1"

# The 160 cut is 160 BYTES on every awk, and never lands inside a UTF-8
# sequence. Two seams meet here. First, the row-build awk runs under LC_ALL=C
# so `substr` counts bytes in gawk (which counts CHARACTERS in a UTF-8 locale)
# as well as in mawk (which always counted bytes) -- otherwise the same diff
# produced a 160-character fragment on one box and a 160-byte one on the next.
# Second, a byte cut splits multi-byte characters: FAIL-first, line 3 below
# (an ASCII byte then 200 'é') put é #80's lead byte C3 at byte 160 and its
# continuation byte at 161, so the fragment ended in a dangling C3 -- invalid
# UTF-8 handed to an fzf display column and written into the comment store as
# a quote header. Line 2 (200 'é', no prefix) is the aligned case the back-off
# must NOT touch: 80 whole characters, exactly 160 bytes, nothing to trim.
RVU=$(mktmpd)
(
  cd "$RVU" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > wide.txt
  git add -A; git commit -q -m init
  eacc=$(printf 'é%.0s' $(seq 1 200))
  printf 'seed\n%s\nx%s\n' "$eacc" "$eacc" > wide.txt
) >/dev/null 2>&1
rvuout=$("$DIR/scripts/tmux-review" --lines "$RVU")
for rvuln in 2 3; do
    rvufrag=$(printf '%s\n' "$rvuout" | awk -F'\t' -v n="$rvuln" '$2 == n { print $3 }')
    rvubytes=$(printf '%s' "$rvufrag" | wc -c)
    if printf '%s' "$rvufrag" | iconv -f UTF-8 -t UTF-8 >/dev/null 2>&1; then
        ok "the line $rvuln fragment is valid UTF-8, not a split character"
    else
        bad "the line $rvuln fragment is valid UTF-8, not a split character" \
            "a decodable fragment" "$rvubytes bytes ending mid-sequence"
    fi
    if [ "$rvubytes" -le 160 ] && [ "$rvubytes" -gt 0 ]; then
        ok "the line $rvuln fragment is at most 160 bytes ($rvubytes)"
    else
        bad "the line $rvuln fragment is at most 160 bytes" "1..160" "$rvubytes"
    fi
done
# The aligned case loses nothing: 200 'é' cut at 160 bytes is 80 whole
# characters, so a back-off that fired here would be trimming valid output.
is "the aligned fragment keeps all 160 of its bytes" "160" \
   "$(printf '%s\n' "$rvuout" | awk -F'\t' '$2 == 2 { print $3 }' | tr -d '\n' | wc -c)"
rm -rf "$RVU"

rv_src() { cat "$DIR/scripts/tmux-review" "$DIR/scripts/lib/review-diff.sh" "$DIR/scripts/lib/review-revdiff.sh" "$DIR/scripts/lib/review-deliver.sh" "$DIR/scripts/lib/review-fleet.sh"; }
echo "== every picker paints its preview border: fzf 0.44's 16-colour theme draws it black (#22 item 6) =="
for f in tm tmux-branch tmux-open-path tmux-review; do
    if [ "$f" = tmux-review ]; then src=$(rv_src); else src=$(cat "$DIR/scripts/$f"); fi
    is "$f: every --color=16 carries border:8" \
       "$(grep -v '^ *#' <<<"$src" | grep -c -- '--color=16')" \
       "$(grep -v '^ *#' <<<"$src" | grep -c -- '--color=16,[^ ]*border:8')"
done

echo "== review-store: parse is the inverse of format over the full grammar =="
# The one module that knows what "## path:line (+)" means. Sourced, pure:
# no tmux, no git (the boundary test below). A record is a header, an
# optional "> fragment" line, body lines, and a blank separator.
RS_LIB="$DIR/scripts/lib/review-store.sh"
# shellcheck disable=SC1090
if [ -f "$RS_LIB" ] && . "$RS_LIB"; then ok "the library sources"; else bad "the library sources" "scripts/lib/review-store.sh" "missing or errored"; fi
RSD=$(mktmpd); RSF="$RSD/comments.md"
# A corpus that exercises every rule: a range, a deletion, file-level, an
# empty body, a multi-line body, a body line that starts with "## " (the
# one escape), a body line starting with "> ", a path with spaces, ':' and
# non-ASCII, a fragment holding a tab, and a record with no fragment line
# (revdiff writes none).
declare -gA STORE=(); STORE_N=0
store_set() { local i=$1; shift; STORE_N=$((i+1)); while [ $# -ge 2 ]; do STORE[$i.$1]=$2; shift 2; done; }
store_set 0 path src.go line 3 side + frag 'x := 1' body 'tighten this'
store_set 1 path src.go line 10 end 12 side + frag 'for {' body $'unroll?\n\n## not a header\n> not a fragment'
store_set 2 path 'old name.txt' line 4 side - frag $'a\tb' body ''
store_set 3 path 'café:v2/x.py' side file-level body 'rename the module'
store_set 4 path lib.sh line 7 side + body 'no fragment line at all'
format_store "$RSF"; rs1=$(cat "$RSF")
case "$rs1" in *$'## src.go:10-12 (+)\n> for {\nunroll?\n\n ## not a header\n> not a fragment\n\n'*) ok "a range, an escaped ## body line and a > body line format as the grammar says" ;;
    *) bad "a range, an escaped ## body line and a > body line format as the grammar says" "the record verbatim" "$rs1" ;; esac
case "$rs1" in *$'## café:v2/x.py (file-level)\nrename the module\n'*) ok "file-level has no line and no fragment" ;;
    *) bad "file-level has no line and no fragment" "## café:v2/x.py (file-level)" "$rs1" ;; esac
case "$rs1" in *$'## old name.txt:4 (-)\n> a\tb\n\n'*) ok "an empty body is header, fragment, separator" ;;
    *) bad "an empty body is header, fragment, separator" "## old name.txt:4 (-) / > a<TAB>b / blank" "$(printf '%s' "$rs1" | cat -A | head -20)" ;; esac
store_dump() { local k; for k in "${!STORE[@]}"; do printf '%s=%q\n' "$k" "${STORE[$k]}"; done | sort; }
snap=$(store_dump); STORE=(); STORE_N=0
parse_store "$RSF"
is "parse reads back every record" "5" "$STORE_N"
is "parse reads back every field" "$snap" "$(store_dump)"
format_store "$RSF.2"
is "format(parse(format(x))) is byte-identical" "" "$(cmp "$RSF" "$RSF.2" 2>&1)"
# revdiff's own output has no fragment line; ranges, file-level, ?? intact.
printf '## README.md:2 (+)\nrevdiff note ?? why changed\n\n## a.go:5-9 (-)\nold block\n\n## b.go (file-level)\nsplit it\n' > "$RSF.rd"
parse_store "$RSF.rd"
is "revdiff's records parse: count" "3" "$STORE_N"
is "revdiff's records parse: range end" "9" "${STORE[1.end]:-}"
is "revdiff's records parse: no fragment key when there was no fragment line" "unset" "${STORE[0.frag]-unset}"
# Corruption is fail-open: blank state, the file set aside, never a crash.
printf 'garbage before any header\n## src.go:3 (+)\nfine\n' > "$RSF.bad"
parse_store "$RSF.bad" 2>/dev/null; rsrc=$?
is "a corrupted store parses to a blank state and returns 0" "0/0" "$rsrc/$STORE_N"
is "and the corrupted file is set aside as .bak" "garbage before any header" "$(head -1 "$STORE_BAK" 2>/dev/null)"
is "and the original path is gone" "gone" "$([ -e "$RSF.bad" ] && echo present || echo gone)"
rsbak1=$STORE_BAK
printf 'second garbage\n' > "$RSF.bad"; parse_store "$RSF.bad" 2>/dev/null
is "a second bad store gets its own backup" "second garbage" "$(head -1 "$STORE_BAK" 2>/dev/null)"
is "and the first backup is untouched" "garbage before any header" "$(head -1 "$rsbak1" 2>/dev/null)"
printf '## src.go:x (+)\nbad line number\n' > "$RSF.bad2"; parse_store "$RSF.bad2" 2>/dev/null
is "a header with a non-numeric line is corruption" "0" "$STORE_N"
parse_store "$RSD/does-not-exist"; is "a missing store is a blank state" "0" "$STORE_N"
# A record in the shape the loop writes (header, quote, body) parses.
RSATT="$RSD/att"; printf '## src.go:3 (+)\n> x := 1\nfrom attach\n\n' > "$RSATT"
parse_store "$RSATT"; is "a store record in the loop's shape parses" "src.go/3/+/x := 1/from attach" "${STORE[0.path]:-}/${STORE[0.line]:-}/${STORE[0.side]:-}/${STORE[0.frag]:-}/${STORE[0.body]:-}"
# The boundary: nothing but comments may say tmux or git in the pure module.
is "the library touches neither tmux nor git" "0" "$(sed '/^[[:space:]]*#/d' "$RS_LIB" | grep -cE 'tmux|git')"
unset -f store_set store_dump; unset STORE STORE_N STORE_BAK

echo "== tmux-review --compose: header + blocks verbatim, ?? passes through untouched =="
RV3STORE=$(mktemp)
cat > "$RV3STORE" <<'STORE'
## scripts/tm:43 (+)
> the quoted fragment
use errors.Is() instead of direct comparison

## scripts/tmux-branch:12 (-)
> old validation line
?? why was this check removed here

STORE
rv3golden=$(cat <<'GOLDEN'
Code review of your changes, from the human, batched on purpose.

Scope: change only what these comments ask for. Do not refactor, rename,
tidy or commit anything else — a fix that grows the diff defeats the review.
Questions first: a comment containing ?? is a question. Answer every question
in chat before you edit anything, then make the edits in one pass.
Disagree out loud: if a comment is wrong or would break something, say so and
do not make the change. Silent compliance with a wrong comment is the worse
failure.
Anchors: comments are path:line with the original line quoted. The numbers are
from the diff as it was when the review was written and drift after your first
edit — trust the quoted fragment, not the number.
When done, end your reply with one line per comment:
  <path>:<line> — done | skipped: <reason>

## scripts/tm:43 (+)
> the quoted fragment
use errors.Is() instead of direct comparison

## scripts/tmux-branch:12 (-)
> old validation line
?? why was this check removed here

GOLDEN
)
is "the composed review matches the golden text byte-for-byte" \
   "$rv3golden" "$("$DIR/scripts/tmux-review" --compose "$RV3STORE")"

# The same store, but with the root that --deliver threads through in real
# use: one more header line naming it, nothing else in the body changed.
rv3rootgolden=$(cat <<'GOLDEN'
Code review of your changes, from the human, batched on purpose.

Scope: change only what these comments ask for. Do not refactor, rename,
tidy or commit anything else — a fix that grows the diff defeats the review.
Questions first: a comment containing ?? is a question. Answer every question
in chat before you edit anything, then make the edits in one pass.
Disagree out loud: if a comment is wrong or would break something, say so and
do not make the change. Silent compliance with a wrong comment is the worse
failure.
Anchors: comments are path:line with the original line quoted. The numbers are
from the diff as it was when the review was written and drift after your first
edit — trust the quoted fragment, not the number.
Paths are relative to /home/agent/project.
When done, end your reply with one line per comment:
  <path>:<line> — done | skipped: <reason>

## scripts/tm:43 (+)
> the quoted fragment
use errors.Is() instead of direct comparison

## scripts/tmux-branch:12 (-)
> old validation line
?? why was this check removed here

GOLDEN
)
is "the composed review names the root when --compose is given one" \
   "$rv3rootgolden" \
   "$("$DIR/scripts/tmux-review" --compose "$RV3STORE" /home/agent/project)"
rm -f "$RV3STORE"

echo "== tmux-review --lines: a hunk's funcname context does not corrupt the anchor line =="
# git's default xfuncname heuristic appends the nearest preceding line that
# looks like a declaration to the "@@" header -- for Rust that is routinely a
# signature ending "-> T {", and a token scan that looks for "any field
# starting with -" finds that arrow too and overwrites the real -o,c token
# with it.
RV4=$(mktmpd)
(
  cd "$RV4" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'fn compute() -> i32 {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4;\n    let e = 5;\n    a + b\n}\n' > f.rs
  git add -A; git commit -q -m init
  sed -i '5d' f.rs   # removes "    let d = 4;", the old file's line 5
) >/dev/null 2>&1
# Confirm the fixture actually exercises the funcname-context shape before
# trusting the assertion below -- git's heuristic is not this test's to own.
rv4hunk=$(git -C "$RV4" diff --no-color --no-ext-diff | grep '^@@')
case "$rv4hunk" in
    *'-> i32 {'*) ;;
    *) bad "the fixture's own hunk header carries funcname context" \
         "an @@ line ending '-> i32 {'" "$rv4hunk" ;;
esac
rv4out=$("$DIR/scripts/tmux-review" --lines "$RV4")
case "$rv4out" in
    *'f.rs'$'\t''-5'$'\t'*) ok "the deletion is anchored to the real old line, 5" ;;
    *) bad "the deletion is anchored to the real old line, 5" "f.rs<TAB>-5<TAB>..." "$rv4out" ;;
esac
rm -rf "$RV4"

echo "== tmux-review --lines: a body line starting '-- ' does not read as a --- header =="
# Diff prepends its own leading '-' to a deleted line -- a deleted line whose
# OWN text starts "-- " becomes "--- ..." once marked, indistinguishable from
# a real "--- a/file" header by a plain /^--- / match. Reproduced: this row
# vanished outright and the deletion after it anchored two lines too early.
RV6=$(mktmpd)
(
  cd "$RV6" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'line1\n-- a sql comment\nline3\nline4\nline5\nline6\nline7\n' > q.sql
  git add -A; git commit -q -m init
  printf 'line1\nline3\nline4\nCHANGED5\nline6\nline7\n' > q.sql
) >/dev/null 2>&1
rv6out=$("$DIR/scripts/tmux-review" --lines "$RV6")
case "$rv6out" in
    *'q.sql'$'\t''-2'$'\t''-- a sql comment'$'\t'*) \
        ok "the '-- ' row is present, anchored to its real old line, 2" ;;
    *) bad "the '-- ' row is present, anchored to its real old line, 2" \
         "q.sql<TAB>-2<TAB>-- a sql comment<TAB>..." "$rv6out" ;;
esac
case "$rv6out" in
    *'q.sql'$'\t''-5'$'\t''line5'$'\t'*) \
        ok "the later deletion still anchors to its real old line, 5" ;;
    *) bad "the later deletion still anchors to its real old line, 5" \
         "q.sql<TAB>-5<TAB>line5<TAB>..." "$rv6out" ;;
esac
case "$rv6out" in
    *'q.sql'$'\t''4'$'\t''CHANGED5'$'\t'*) \
        ok "the addition anchors to its real new line, 4" ;;
    *) bad "the addition anchors to its real new line, 4" \
         "q.sql<TAB>4<TAB>CHANGED5<TAB>..." "$rv6out" ;;
esac
rm -rf "$RV6"

echo "== tmux-review --lines: a body line starting '++ ' does not read as a +++ header =="
# Same seam, the addition side: an added line whose OWN text starts "++ "
# becomes "+++ ..." once diff marks it. Reproduced: this row vanished and
# reset the running counters, throwing every later anchor off by its length.
RV7=$(mktmpd)
(
  cd "$RV7" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\n' > m.md
  git add -A; git commit -q -m init
  printf 'l1\nl2\nl3\nl4\nl5\n++ added by the patch\nl6\nl7\nl8\nl9\nl10\nCHANGED\nl12\n' > m.md
) >/dev/null 2>&1
rv7out=$("$DIR/scripts/tmux-review" --lines "$RV7")
case "$rv7out" in
    *'m.md'$'\t''6'$'\t''++ added by the patch'$'\t'*) \
        ok "the '++ ' row is present, anchored to its real new line, 6" ;;
    *) bad "the '++ ' row is present, anchored to its real new line, 6" \
         "m.md<TAB>6<TAB>++ added by the patch<TAB>..." "$rv7out" ;;
esac
case "$rv7out" in
    *'m.md'$'\t''-11'$'\t''l11'$'\t'*) \
        ok "the later deletion still anchors to its real old line, 11" ;;
    *) bad "the later deletion still anchors to its real old line, 11" \
         "m.md<TAB>-11<TAB>l11<TAB>..." "$rv7out" ;;
esac
case "$rv7out" in
    *'m.md'$'\t''12'$'\t''CHANGED'$'\t'*) \
        ok "the later addition still anchors to its real new line, 12" ;;
    *) bad "the later addition still anchors to its real new line, 12" \
         "m.md<TAB>12<TAB>CHANGED<TAB>..." "$rv7out" ;;
esac
rm -rf "$RV7"

echo "== tmux-review --lines / preview: a subdir pane's cwd still resolves to the repo root =="
# git -C <dir> diff always emits paths relative to the repo ROOT, not <dir> --
# so a pane whose cwd is a subdirectory used to hand --lines rows that
# --preview-diff (cd <dir> && ...) could never find: "gone: root.txt"
# instead of the real diff.
RV8=$(mktmpd)
(
  cd "$RV8" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  mkdir -p sub
  printf 'root\n' > root.txt
  printf 'one\n' > sub/foo.sh
  git add -A; git commit -q -m init
  printf 'root\nroot\nmore\n' > root.txt
  printf 'one\ntwo\n' > sub/foo.sh
) >/dev/null 2>&1
rv8lines=$("$DIR/scripts/tmux-review" --lines "$RV8/sub")
case "$rv8lines" in
    *'root.txt'$'\t'*) ok "--lines from a subdir pane still anchors the root file" ;;
    *) bad "--lines from a subdir pane still anchors the root file" \
         "a root.txt row" "$rv8lines" ;;
esac
case "$rv8lines" in
    *'sub/foo.sh'$'\t''2'$'\t'*) ok "--lines from a subdir pane anchors a row inside that subdir too" ;;
    *) bad "--lines from a subdir pane anchors a row inside that subdir too" \
         "sub/foo.sh<TAB>2<TAB>..." "$rv8lines" ;;
esac

# Confirms the fixture actually exercises the seam before trusting the fix
# below: --lines' own row path ("root.txt") is repo-root-relative, but a
# preview `cd`'d into the pane's raw cwd (sub/) can't find it there.
rv8rawpreview=$(cd "$RV8/sub" && "$DIR/scripts/tmux-open-path" --preview-diff root.txt)
case "$rv8rawpreview" in
    gone:*) ok "the fixture reproduces the seam: root.txt is not found from the pane's raw cwd" ;;
    *) bad "the fixture reproduces the seam: root.txt is not found from the pane's raw cwd" \
         "gone: root.txt" "$rv8rawpreview" ;;
esac
# The actual fix: resolve the pane's cwd to its repo root first (what
# review_pane/fleet_queue now do before ever calling --loop or previewing),
# then the same row path resolves to a real diff.
rv8root=$(git -C "$RV8/sub" rev-parse --show-toplevel)
rv8rootpreview=$(cd "$rv8root" && "$DIR/scripts/tmux-open-path" --preview-diff root.txt \
    | sed 's/\x1b\[[0-9;]*m//g')
case "$rv8rootpreview" in
    *'+root'*) ok "previewing from the resolved root shows a real diff, not 'gone:'" ;;
    *) bad "previewing from the resolved root shows a real diff, not 'gone:'" \
         "a diff containing +root" "$rv8rootpreview" ;;
esac
rm -rf "$RV8"

echo "== tmux-review --lines: output is byte-identical to the committed goldens =="
for gf in basic mixed untracked blank nonascii funcname hunks spaces; do
    GD=$(mktmpd); "$DIR/tests/fixtures/review/mkfix.sh" "$gf" "$GD" >/dev/null 2>&1
    if "$DIR/scripts/tmux-review" --lines "$GD" | cmp -s - "$DIR/tests/fixtures/review/$gf.tsv"; then ok "golden $gf"
    else bad "golden $gf" "byte-identical" "$("$DIR/scripts/tmux-review" --lines "$GD" | diff - "$DIR/tests/fixtures/review/$gf.tsv" | head -5)"; fi
    rm -rf "$GD"
done

echo "== tmux-review --lines: the diff never writes the agent's index =="
# Every diff now runs against a COPY of the index (git add -N -A on the copy
# folds untracked files into the same diff). The copy is the whole point:
# an `add -N` against the real index would rewrite the agent's own index
# under it mid-commit. Pinned by the index file's mtime and size.
RI=$(mktmpd); "$DIR/tests/fixtures/review/mkfix.sh" untracked "$RI" >/dev/null 2>&1
ri_stamp() { stat -c '%Y %s' "$RI/.git/index" 2>/dev/null || stat -f '%m %z' "$RI/.git/index"; }
touch -d '2020-01-01 00:00:00' "$RI/.git/index" 2>/dev/null || touch -t 202001010000 "$RI/.git/index"
ri_before=$(ri_stamp)
is "untracked rows still come out of one diff" "2" \
   "$("$DIR/scripts/tmux-review" --lines "$RI" | grep -c $'^new.txt\t')"
is "and the real index is byte- and mtime-identical afterwards" "$ri_before" "$(ri_stamp)"
is "no intent-to-add entry leaked into the real index" "" \
   "$(git -C "$RI" ls-files -s -- new.txt)"
unset -f ri_stamp
rm -rf "$RI"

echo "== tmux-review --lines: a file over 32 MB is skipped before git reads it, and the list says so =="
RB=$(mktmpd)
(
  cd "$RB" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f.c; git add -A; git commit -q -m init
  printf 'a\nb\n' > f.c
  head -c $(( 64 * 1024 * 1024 )) /dev/zero > core          # untracked, 64 MiB
  printf 'x\n' > small.txt                                  # untracked, small: still reviewed
) >/dev/null 2>&1
# CPU, not the wall clock: the claim is that the 64 MiB file is skipped rather
# than diffed, and diffing it is CPU. 3x this site's 0.157s under twelve
# spinners (0.109-0.132s idle); a wall bound grades the box's I/O too (#75).
cpu_under "a 64 MiB file is skipped, not diffed: the list is under 0.5s of CPU" 0.5 \
    "$DIR/scripts/tmux-review" --lines "$RB"
rbrows=$(cat "$CPUOUT")
is "the small change is still a row"      "1" "$(printf '%s\n' "$rbrows" | grep -c $'^f.c\t')"
is "the small untracked file is still a row" "1" "$(printf '%s\n' "$rbrows" | grep -c $'^small.txt\t')"
is "the big file has no rows"             "0" "$(printf '%s\n' "$rbrows" | grep -c $'^core\t')"
case "$rbrows" in
    *"(diff too large)"*"core (64 MB) skipped"*"over 32 MB"*) ok "one notice row names the skipped file and its size" ;;
    *) bad "one notice row names the skipped file and its size" "(diff too large) ... core (64 MB) skipped ... over 32 MB" "$rbrows" ;;
esac
is "the full diff omits it too" "0" "$(bash -c '. "$1/git.sh"; . "$1/review-diff.sh"; tree_diff "$2"' _ "$DIR/scripts/lib" "$RB" | grep -c '^diff --git a/core')"
is "and numstat names it as a binary row, unread" "-	-	core" "$(bash -c '. "$1/git.sh"; . "$1/review-diff.sh"; tree_diff "$2" --numstat' _ "$DIR/scripts/lib" "$RB" | grep -P '\tcore$')"
printf 'big\n' > "$RB/f.c"; head -c $(( 64 * 1024 * 1024 )) /dev/zero >> "$RB/f.c"   # a TRACKED file grown past the cap
rbrows2=$("$DIR/scripts/tmux-review" --lines "$RB")
is "a tracked file grown past the cap is skipped the same way" "0" "$(printf '%s\n' "$rbrows2" | grep -c $'^f.c\t')"
case "$rbrows2" in *"f.c (64 MB) skipped"*) ok "and named" ;; *) bad "and named" "f.c (64 MB) skipped" "$rbrows2" ;; esac
# Same conversion: hashing 64 MB is CPU and nothing else here is. 3x this
# site's 0.022s under twelve spinners, in this exact wrapped form.
cpu_under "the preview answers without hashing the file (under 0.07s of CPU)" 0.07 \
    sh -c 'cd "$1" && exec "$2" --preview-diff core' sh "$RB" "$DIR/scripts/tmux-open-path"
rbpv=$(cat "$CPUOUT")
is "the preview of a big file is a one-line notice" "core (64 MB, not diffed: over 32 MB)" "$rbpv"
rm -rf "$RB"

# A stand-in for revdiff, which is interactive and so cannot be driven here. Each
# launch is call N: it logs its argv and the --annotations file it was handed
# (and that file's mode) under $FAKE_RD_DIR, then plays back out.N into the -o
# file and exits with rc.N (default 0) -- revdiff's own contract, exit 10 when
# comments were written. out.N is revdiff's real format: "## path:N (+)" headers,
# "( )" for a context line, "(file-level)", no fragment line, no trailing blank.
fake_revdiff() {              # <bin-dir> -> <bin-dir>/revdiff
    mkdir -p "$1"
    cat > "$1/revdiff" <<'FAKERD'
#!/bin/sh
d=${FAKE_RD_DIR:?}
n=$(cat "$d/n" 2>/dev/null || echo 0); n=$((n + 1)); printf '%s\n' "$n" > "$d/n"
printf '%s\n' "$*" > "$d/argv.$n"
out=; pre=; prev=
for a in "$@"; do
    case "$prev" in -o) out=$a ;; --annotations) pre=$a ;; esac
    prev=$a
done
if [ -n "$pre" ]; then
    cat "$pre" > "$d/pre.$n"
    { stat -c %a "$pre" 2>/dev/null || stat -f %Lp "$pre"; } > "$d/premode.$n"
fi
[ -f "$d/out.$n" ] && [ -n "$out" ] && cat "$d/out.$n" > "$out"
[ -f "$d/err.$n" ] && cat "$d/err.$n" >&2
exit "$(cat "$d/rc.$n" 2>/dev/null || echo 0)"
FAKERD
    chmod +x "$1/revdiff"
}
# The loop with keys typed at its after-revdiff prompt; prints its exit code.
rd_loop() {                   # <keys> <repo> <store> [PATH]
    local rc
    printf '%s' "$1" | PATH="${4:-$RDB:$PATH}" FAKE_RD_DIR="$RDW" \
        "$DIR/scripts/tmux-review" --loop "$2" "$3" >/dev/null 2>"$RDW/stderr"
    rc=$?; printf '%s' "$rc"
}
rd_reset() { rm -rf "$RDW"; mkdir -p "$RDW"; }
rd_last() { awk '{print $NF}' "$RDW/argv.$1"; }

echo "== tmux-review: the review base is since-last-review, else the branch, else uncommitted =="
# revdiff's auto-detect, one function: a checkpoint ref wins, then the
# merge-base with the upstream/default branch when it is not HEAD itself,
# then HEAD (today's view). ^B cycles the modes that hold. Exercised through
# --delta and --lines so the tests see what a reviewer sees.
RB=$(mktmpd)
(
  cd "$RB" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\n' > f; git add -A; git commit -q -m init
  # "topic", not "feat": it has to sort AFTER main and master, because
  # review_base reads its refs from one for-each-ref whose rows come back
  # sorted by name -- a fallback seen before the branch's own row must not be
  # lost to it (it was, once).
  git checkout -q -b topic
  printf 'a\nCOMMITTED-ON-FEAT\n' > f; git commit -q -am topic
) >/dev/null 2>&1
is "a clean feature branch reviews its commits against main" "1" \
   "$("$DIR/scripts/tmux-review" --lines "$RB" | grep -c 'COMMITTED-ON-FEAT')"
printf 'a\nCOMMITTED-ON-FEAT\nUNCOMMITTED\n' > "$RB/f"
is "branch + uncommitted are one set" "2" \
   "$("$DIR/scripts/tmux-review" --lines "$RB" | grep -c 'COMMITTED-ON-FEAT\|UNCOMMITTED')"
is "COCKPIT_REVIEW_BASE=uncommitted narrows it to HEAD" "1" \
   "$(COCKPIT_REVIEW_BASE=uncommitted "$DIR/scripts/tmux-review" --lines "$RB" | grep -c 'COMMITTED-ON-FEAT\|UNCOMMITTED')"
# A checkpoint: the tree as it stands now, under the ref ^A will write.
(
  cd "$RB" || exit
  I=$(mktemp); cp -p .git/index "$I"; GIT_INDEX_FILE=$I git add -A
  git update-ref refs/cockpit/reviewed/topic "$(GIT_INDEX_FILE=$I git write-tree)"; rm -f "$I"
) >/dev/null 2>&1
is "with a checkpoint at the current tree there is nothing to review" "" \
   "$("$DIR/scripts/tmux-review" --lines "$RB")"
printf 'a\nCOMMITTED-ON-FEAT\nUNCOMMITTED\nSINCE\n' > "$RB/f"
is "and after one more edit only that edit is a row" "SINCE" \
   "$("$DIR/scripts/tmux-review" --lines "$RB" | cut -f3)"
is "COCKPIT_REVIEW_BASE=branch still shows the whole branch" "3" \
   "$(COCKPIT_REVIEW_BASE=branch "$DIR/scripts/tmux-review" --lines "$RB" | grep -c .)"

rm -rf "$RB"

echo "== tmux-review --loop: a approves — the ref is the tree, the dir is gone, the next review is the delta =="
if need_tmux "review ^A"; then :; else
RA="cockpit-ra-$$"; SOCKS="$SOCKS $RA"; RASTATE=$(mktmpd); RADIR=$(mktmpd)
RAB=$(mktmpd); RAW=$(mktmpd); fake_revdiff "$RAB"
# The first launch pins one comment; every later launch exits 0 with none.
printf '## src.go:3 (+)\nnot yet\n' > "$RAW/out.1"; echo 10 > "$RAW/rc.1"
RAENV="PATH=$(printf '%q' "$RAB"):\$PATH FAKE_RD_DIR=$(printf '%q' "$RAW")"
(
  cd "$RADIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go; printf 'brand new\n' > new.txt
) >/dev/null 2>&1
"$TMUX_REAL" -L "$RA" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RADIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RA" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
RAAP=$("$TMUX_REAL" -L "$RA" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RA" set-option -p -t "$RAAP" @cockpit_review pending >/dev/null 2>&1
# new-window, not split-window: each control shell gets its own full-height
# window; -p still targets RAAP explicitly, so which window the loop runs from
# is moot.
ra_shell() { local p i; p=$("$TMUX_REAL" -L "$RA" new-window -d -t t: -P -F '#{pane_id}' 'exec bash'); for i in $(seq 1 40); do [ "$("$TMUX_REAL" -L "$RA" display-message -p -t "$p" '#{pane_current_command}')" = bash ] && break; sleep 0.1; done; printf '%s' "$p"; }
RACTL=$(ra_shell)
RASESS="$RASTATE/tmux-cockpit/review/${RAAP#%}-$(printf '%s' "$RADIR" | cksum | cut -d' ' -f1)"
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" "$RAENV XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $RAAP" Enter
review_enter "$RA" "$RACTL"
# One pinned comment (revdiff's first launch wrote it): a must refuse --
# approve means "nothing more to say".
until_cap "$RA" "$RACTL" '*1 comment(s) pinned*' && ok "the comment revdiff wrote is pinned in the session store" || bad "the comment revdiff wrote is pinned in the session store" "1 comment(s) pinned" "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RACTL")"
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" a
# Deliver it (the pane is idle: a paste), then re-open and approve.
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" d
until_cap "$RA" "$RAAP" '*not yet*' && ok "the batch lands in the agent pane" || bad "the batch lands in the agent pane" "not yet" "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RAAP")"
until_file "$RASESS/delivered.md" '*not yet*' && ok "a delivered batch is archived as delivered.md" || bad "a delivered batch is archived as delivered.md" "not yet in delivered.md" "$(ls "$RASESS" 2>/dev/null)"
# a with a pinned comment must have refused: had it approved, the loop would have
# exited (the d above would be typed into a shell) and the ref would exist.
is "a with a pinned comment refused: nothing was approved" "" "$(git -C "$RADIR" rev-parse -q --verify refs/cockpit/reviewed/main 2>/dev/null)"
# done_store() appends to delivered.md and unlinks the store as two statements,
# so the poll above can land between them. Poll the store path itself.
for i in $(seq 1 40); do [ -e "$RASESS/comments.md" ] || break; sleep 0.1; done
is "the store itself is gone" "absent" "$([ -e "$RASESS/comments.md" ] && echo present || echo absent)"
RACTL2=$(ra_shell)
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL2" "$RAENV XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $RAAP" Enter
review_enter "$RA" "$RACTL2"
# approve() works on a cp -p COPY of the index (tree_diff's own discipline) --
# the agent's real index must read back byte-unchanged mtime-wise.
RAIDXBEFORE=$(stat -c %Y "$RADIR/.git/index" 2>/dev/null)
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL2" a
for i in $(seq 1 40); do [ -e "$RASESS" ] || break; sleep 0.1; done
is "a removes the session dir" "gone" "$([ -e "$RASESS" ] && echo present || echo gone)"
is "approve never writes the real index" "$RAIDXBEFORE" "$(stat -c %Y "$RADIR/.git/index" 2>/dev/null)"
is "a clears the pane's review state" "" "$("$TMUX_REAL" -L "$RA" show-option -pqv -t "$RAAP" @cockpit_review)"
RAREF=$(git -C "$RADIR" rev-parse -q --verify refs/cockpit/reviewed/main 2>/dev/null)
is "a wrote the checkpoint ref to a tree" "tree" "$(git -C "$RADIR" cat-file -t "$RAREF" 2>/dev/null)"
is "the tree holds the untracked file too" "1" "$(git -C "$RADIR" ls-tree -r "$RAREF" --name-only 2>/dev/null | grep -c '^new.txt$')"
is "nothing is left to review" "" "$("$DIR/scripts/tmux-review" --lines "$RADIR")"
# --delta asks tmux about the pane, so it runs on THIS server (run-shell), not the suite's.
is "--delta says so (exit 1)" "D=1" "$("$TMUX_REAL" -L "$RA" run-shell "XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' --delta $RAAP; echo D=\$?" 2>/dev/null | tail -1)"
printf 'brand new\nEDITED\n' > "$RADIR/new.txt"
is "an edit to the approved untracked file is the only row" "EDITED" "$("$DIR/scripts/tmux-review" --lines "$RADIR" | cut -f3)"
# --done as a public subcommand: keep_hint()'s own re-delivery command chains
# into it, so a by-hand delivery has to finish the same way as review_pane()'s
# own cleanup does.
DNDIR=$(mktmpd)
printf '## d.go:1 (+)\n> x\nnote\n\n' > "$DNDIR/comments.md"
: > "$DNDIR/comments.md.base"
"$DIR/scripts/tmux-review" --done "$DNDIR/comments.md"
case "$(cat "$DNDIR/delivered.md" 2>/dev/null)" in
    *note*) ok "--done, called directly, archives the batch" ;;
    *) bad "--done, called directly, archives the batch" "note in delivered.md" "$(cat "$DNDIR/delivered.md" 2>/dev/null)" ;;
esac
is "and drops the store" "absent" "$([ -e "$DNDIR/comments.md" ] && echo present || echo absent)"
is "and the base file beside it" "absent" "$([ -e "$DNDIR/comments.md.base" ] && echo present || echo absent)"
# `git branch -D` / `worktree remove` do NOT GC refs/cockpit/reviewed/* (they
# are plain refs, not branch-scoped) -- a ref left at "feat/x" D/F-blocks a
# later `update-ref refs/cockpit/reviewed/feat`. ^A must refuse, not crash or
# silently overwrite; this session has no attached client (as above), so the
# refusal is asserted by its effect, not its display-message wording.
SRDIR=$(mktmpd)
(
  cd "$SRDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b feat .; printf 'x\n' > f; git add -A; git commit -q -m init
  git update-ref refs/cockpit/reviewed/feat/x "$(git rev-parse "HEAD^{tree}")"
  printf 'x\nCHANGED\n' > f
) >/dev/null 2>&1
SRAP=$("$TMUX_REAL" -L "$RA" new-window -d -t t: -c "$SRDIR" -P -F '#{pane_id}' 'exec -a claude bash')
SRSESS="$RASTATE/tmux-cockpit/review/${SRAP#%}-$(printf '%s' "$SRDIR" | cksum | cut -d' ' -f1)"
SRCTL=$(ra_shell)
"$TMUX_REAL" -L "$RA" send-keys -t "$SRCTL" "$RAENV XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $SRAP" Enter
review_enter "$RA" "$SRCTL"
"$TMUX_REAL" -L "$RA" send-keys -t "$SRCTL" a
# Nothing to poll FOR -- a refusal leaves the same file on disk a success
# would remove. Poll for the disappearance instead, up to the same 2s budget
# every other settle in this block uses: a slow but correct approve() still
# reads as present here, a wrongly-succeeding one is caught the moment it
# fires rather than only at a fixed deadline.
srgone=0
for i in $(seq 1 20); do
    [ -e "$SRSESS" ] || { srgone=1; break; }
    sleep 0.1
done
is "a D/F-blocked ref refuses the approve -- the session dir survives" "0" "$srgone"
is "and refs/cockpit/reviewed/feat was never written" "" \
   "$(git -C "$SRDIR" rev-parse -q --verify refs/cockpit/reviewed/feat 2>/dev/null)"
# ^A must not take a review that is already IN FLIGHT. A delivery into a busy
# pane lives in comments.md.deliver with @cockpit_review_spool armed at it, and
# ^A is allowed then (the guard only refuses PINNED comments, and the store is
# empty once the batch went out) -- so the old `rm -rf` on the session dir threw
# the queued review away, left the option pointing at a path that no longer
# existed, and took delivered.md with it. Named files only now; the dir stands
# while the spool is in it and review-gate rmdirs it after the drain.
QSDIR=$(mktmpd)
(
  cd "$QSDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'q\n' > f; git add -A; git commit -q -m init
  printf 'q\nCHANGED\n' > f
) >/dev/null 2>&1
QSAP=$("$TMUX_REAL" -L "$RA" new-window -d -t t: -c "$QSDIR" -P -F '#{pane_id}' 'exec -a claude bash')
QSSESS="$RASTATE/tmux-cockpit/review/${QSAP#%}-$(printf '%s' "$QSDIR" | cksum | cut -d' ' -f1)"
QSCTL=$(ra_shell)
"$TMUX_REAL" -L "$RA" send-keys -t "$QSCTL" "$RAENV XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $QSAP" Enter
review_enter "$RA" "$QSCTL"
# The picker is up, so review_pane has made the dir: drop into it exactly what a
# queued delivery leaves behind, then approve.
printf 'IN-FLIGHT REVIEW\n' > "$QSSESS/comments.md.deliver"
printf 'an earlier batch\n' > "$QSSESS/delivered.md"
"$TMUX_REAL" -L "$RA" set-option -p -t "$QSAP" @cockpit_review_spool "$QSSESS/comments.md.deliver" >/dev/null 2>&1
"$TMUX_REAL" -L "$RA" send-keys -t "$QSCTL" a
for i in $(seq 1 40); do [ -e "$QSSESS/delivered.md" ] || break; sleep 0.1; done
is "a leaves a queued review's spool alone" "IN-FLIGHT REVIEW" "$(cat "$QSSESS/comments.md.deliver" 2>/dev/null)"
is "and the pane's spool option still points at it" "$QSSESS/comments.md.deliver" \
   "$("$TMUX_REAL" -L "$RA" show-option -pqv -t "$QSAP" @cockpit_review_spool)"
is "the session dir stands while the spool is in it" "present" "$([ -d "$QSSESS" ] && echo present || echo gone)"
is "but a took the store and delivered.md" "gone gone" \
   "$([ -e "$QSSESS/comments.md" ] && echo present || echo gone) $([ -e "$QSSESS/delivered.md" ] && echo present || echo gone)"
is "and it still wrote the checkpoint ref" "tree" \
   "$(git -C "$QSDIR" cat-file -t "$(git -C "$QSDIR" rev-parse -q --verify refs/cockpit/reviewed/main 2>/dev/null)" 2>/dev/null)"
# review-gate's own rmdir is what finishes the dir off after the drain.
rm -f "$QSSESS/comments.md.deliver"; rmdir "$QSSESS" 2>/dev/null
is "and the dir goes once the spool is drained" "gone" "$([ -d "$QSSESS" ] && echo present || echo gone)"
unset -f ra_shell
"$TMUX_REAL" -L "$RA" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --request: the agent asks — exit 10 with the review on stdout, 0 on nothing, 1 with no client =="
if need_tmux "tmux-review --request"; then :; else
RQ="cockpit-rq-$$"; SOCKS="$SOCKS $RQ"; RQSTATE=$(mktmpd); RQDIR=$(mktmpd)
(
  cd "$RQDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\n' > f
) >/dev/null 2>&1
"$TMUX_REAL" -L "$RQ" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RQDIR" 'exec -a claude bash' >/dev/null 2>&1
RQAP=$("$TMUX_REAL" -L "$RQ" list-panes -t t: -F '#{pane_id}')
# A detached server has no client to draw a popup on: a launcher error, not a
# "discarded" -- the skill tells them apart by the exit code.
XDG_STATE_HOME=$RQSTATE TMUX='' "$TMUX_REAL" -L "$RQ" run-shell "XDG_STATE_HOME=$(printf '%q' "$RQSTATE") '$DIR/scripts/tmux-review' --request $RQAP; echo RQRC=\$?" >"$RQSTATE/out" 2>&1
case "$(cat "$RQSTATE/out")" in *RQRC=1*) ok "no attached client: exit 1" ;; *) bad "no attached client: exit 1" "RQRC=1" "$(cat "$RQSTATE/out")" ;; esac
# The popup itself needs a client and a human; stand a tmux shim in front of it
# (the suite's own shim technique) that answers `display-popup` by writing the
# exit code the loop would have left behind -- 20 is ^S deliver -- and doing
# nothing else. The human's whole review is then whatever the store already
# holds when the popup "closes"; RQ_NO_RC=1 writes no rc at all, which is esc's
# `leave` and a dropped popup alike.
RQSESS="$RQSTATE/tmux-cockpit/review/${RQAP#%}-$(printf '%s' "$RQDIR" | cksum | cut -d' ' -f1)"
RQSHIM=$(mktmpd)
cat > "$RQSHIM/tmux" <<SHIMEOF
#!/bin/sh
case "\$1" in
    display-popup) [ -n "\${RQ_NO_RC:-}" ] || printf '%s\n' "\${RQ_RC:-20}" > "$RQSESS/rc"; exit 0 ;;
    display-message) for a; do :; done; case "\$a" in *session_attached*) echo 1; exit 0 ;; esac ;;
esac
exec "$TMUX_REAL" -L "$RQ" "\$@"
SHIMEOF
chmod +x "$RQSHIM/tmux"
mkdir -p "$RQSESS"; printf '## f:2 (+)\n> b\nREQUEST-MARKER\n\n' > "$RQSESS/comments.md"
rq_out=$(PATH="$RQSHIM:$PATH" XDG_STATE_HOME=$RQSTATE "$DIR/scripts/tmux-review" --request "$RQAP"); rq_rc=$?
is "comments pinned: exit 10" "10" "$rq_rc"
case "$rq_out" in "Code review of your changes"*"REQUEST-MARKER"*) ok "and the composed review is on stdout" ;; *) bad "and the composed review is on stdout" "header + REQUEST-MARKER" "$rq_out" ;; esac
is "the batch is archived, the store consumed" "1" "$(grep -c REQUEST-MARKER "$RQSESS/delivered.md" 2>/dev/null)"
# The agent has the review in hand, so the pane reads "sent" exactly as it does
# after a paste -- the shim passes set-option through to the real server.
is "and the pane is marked sent, as after a paste" "sent" "$("$TMUX_REAL" -L "$RQ" show-option -pqv -t "$RQAP" @cockpit_review)"
rq_out=$(PATH="$RQSHIM:$PATH" XDG_STATE_HOME=$RQSTATE "$DIR/scripts/tmux-review" --request "$RQAP"); rq_rc=$?
is "nothing pinned: exit 0, nothing printed" "0 " "$rq_rc $rq_out"
# esc's `leave` (Task 8) keeps a non-empty store and does NOT deliver it: the
# batch decides nothing, the recorded exit code does.
printf '## f:2 (+)\n> b\nLEAVE-MARKER\n\n' > "$RQSESS/comments.md"
rq_out=$(RQ_NO_RC=1 PATH="$RQSHIM:$PATH" XDG_STATE_HOME=$RQSTATE "$DIR/scripts/tmux-review" --request "$RQAP"); rq_rc=$?
is "left for later: exit 0, nothing printed" "0 " "$rq_rc $rq_out"
is "and the batch stays pinned" "1" "$(grep -c LEAVE-MARKER "$RQSESS/comments.md" 2>/dev/null)"
# No revdiff: the loop exits 127 and --request says so on stderr and exits 1,
# not the 0 that "left for later" gets. The batch stays where it was.
rq_err=$(RQ_RC=127 PATH="$RQSHIM:$PATH" XDG_STATE_HOME=$RQSTATE "$DIR/scripts/tmux-review" --request "$RQAP" 2>&1 >/dev/null); rq_rc=$?
is "revdiff missing: exit 1" "1" "$rq_rc"
is "and stderr says what to do" "tmux-review: revdiff not found: install it, see #233" "$rq_err"
is "and the batch stays pinned" "1" "$(grep -c LEAVE-MARKER "$RQSESS/comments.md" 2>/dev/null)"
# End to end: the popup runs the real loop with a stand-in revdiff, the human
# presses d, and --request hands the composed review back on stdout with exit 10.
RQB=$(mktmpd); RQW=$(mktmpd); fake_revdiff "$RQB"
printf '## f:2 (+)\nREQ-LOOP-MARKER\n' > "$RQW/out.1"; echo 10 > "$RQW/rc.1"
rm -f "$RQSESS/comments.md"
cat > "$RQSHIM/tmux" <<SHIMEOF
#!/bin/sh
case "\$1" in
    display-popup) for a; do :; done; printf d | sh -c "\$a" >/dev/null 2>&1; exit 0 ;;
    display-message) for a; do :; done; case "\$a" in *session_attached*) echo 1; exit 0 ;; esac ;;
esac
exec "$TMUX_REAL" -L "$RQ" "\$@"
SHIMEOF
rq_out=$(PATH="$RQSHIM:$RQB:$PATH" FAKE_RD_DIR="$RQW" XDG_STATE_HOME=$RQSTATE "$DIR/scripts/tmux-review" --request "$RQAP"); rq_rc=$?
is "the real loop and a stand-in revdiff: exit 10" "10" "$rq_rc"
case "$rq_out" in "Code review of your changes"*"> b"*"REQ-LOOP-MARKER"*) ok "and the composed review is on stdout, quote and comment" ;; *) bad "and the composed review is on stdout, quote and comment" "header, > b, REQ-LOOP-MARKER" "$rq_out" ;; esac
rm -rf "$RQW"; mkdir -p "$RQW"; echo 3 > "$RQW/rc.1"; echo "boom: bad flag" > "$RQW/err.1"
rq_err=$(PATH="$RQSHIM:$RQB:$PATH" FAKE_RD_DIR="$RQW" XDG_STATE_HOME=$RQSTATE "$DIR/scripts/tmux-review" --request "$RQAP" 2>&1 >/dev/null); rq_rc=$?
is "a revdiff that fails: --request exits 1, not 0" "1" "$rq_rc"
is "and prints revdiff's own stderr after the popup" "tmux-review: revdiff failed"$'\n'"boom: bad flag" "$rq_err"
is "and leaves no revdiff.err behind" "no" "$([ -e "$RQSESS/revdiff.err" ] && echo yes || echo no)"
"$TMUX_REAL" -L "$RQ" kill-server >/dev/null 2>&1
fi

echo "== tmux-open-path --preview-diff: a nested file's diff is not lost to path-doubling =="
PD=$(mktmpd)
(
  cd "$PD" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  mkdir -p scripts
  printf 'echo one\n' > scripts/foo.sh
  git add -A; git commit -q -m init
  printf 'echo one\necho two\n' > scripts/foo.sh
) >/dev/null 2>&1
pdout=$(cd "$PD" && "$DIR/scripts/tmux-open-path" --preview-diff scripts/foo.sh)
case "$pdout" in
    *'echo two'*) ok "a nested file's diff shows, not 'no uncommitted changes'" ;;
    *) bad "a nested file's diff shows, not 'no uncommitted changes'" \
         "a diff containing echo two" "$pdout" ;;
esac
# The exact mechanism tmux-review's picker preview uses: cd into the review's
# target dir, then hand tmux-open-path the row's own dir-relative path -- the
# same path --lines already emits, not a hand-typed one.
rv5path=$("$DIR/scripts/tmux-review" --lines "$PD" | head -1 | cut -f1)
is "the --lines row path is dir-relative, exactly what --preview-diff expects" \
   "scripts/foo.sh" "$rv5path"
rv5preview=$(cd "$PD" && "$DIR/scripts/tmux-open-path" --preview-diff "$rv5path")
case "$rv5preview" in
    *'echo two'*) ok "the review-loop preview path shows the same diff" ;;
    *) bad "the review-loop preview path shows the same diff" \
         "a diff containing echo two" "$rv5preview" ;;
esac
rm -rf "$PD"

# group: serial
echo "== tmux-open-path --preview-diff: the second render of a file is a cache hit =="
# Hermetic: XDG_RUNTIME_DIR points at a throwaway dir for every call this
# block makes, so the memoised cache lives and dies with the test.
RTD=$(mktmpd)
PV=$(mktmpd); ( cd "$PV" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; seq 1 2000 > big.txt; git add -A; git commit -q -m init; sed -i '1000s/.*/CHANGED/' big.txt ) >/dev/null 2>&1
XDG_RUNTIME_DIR="$RTD" "$DIR/scripts/tmux-open-path" --preview-diff "$PV/big.txt" > "$STATE/pv1"
if XDG_RUNTIME_DIR="$RTD" timeout 5 "$DIR/scripts/tmux-open-path" --preview-diff "$PV/big.txt" > "$STATE/pv2" 2>/dev/null; then ok "a repeat render exits 0"
else bad "a repeat render exits 0" "rc 0 within 5s" "rc $?"; fi
is "and is identical" "" "$(cmp "$STATE/pv1" "$STATE/pv2" 2>&1)"
# A clock does not prove the cache is READ. On this fixture a HIT costs
# 24-29ms and a MISS 39-43ms, so no budget separates them, and the old
# `timeout 0.05` only ever went red under peer load, emptying pv2 for the cmp
# above (issue #36); timeout 5 is a hang guard. A mutant copy of the script
# whose cache read is dead passes all three assertions above unchanged.
# Only a row that could not have come from anywhere else
# proves the memo was used. Plant one -- the hermetic dir holds exactly the
# single entry the first render wrote -- and demand the next render hand it
# back verbatim. Same trick as the SENTINEL row in the row-snapshot block.
pvcdir="$RTD/tmux-cockpit-preview-$(id -u)"
is "the hermetic cache dir holds exactly the one entry the first render wrote" \
   "1" "$(find "$pvcdir" -mindepth 1 -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d ' ')"
pventry=$(find "$pvcdir" -mindepth 1 -maxdepth 1 -type f 2>/dev/null | head -1)
printf 'PV_CACHE_SENTINEL_NO_RENDER_CAN_PRODUCE_THIS\n' > "$pventry"
is "and a repeat render is served FROM that entry, not re-rendered" \
   "PV_CACHE_SENTINEL_NO_RENDER_CAN_PRODUCE_THIS" \
   "$(XDG_RUNTIME_DIR="$RTD" "$DIR/scripts/tmux-open-path" --preview-diff "$PV/big.txt")"
sed -i '1001s/.*/ALSO/' "$PV/big.txt"
case "$(XDG_RUNTIME_DIR="$RTD" "$DIR/scripts/tmux-open-path" --preview-diff "$PV/big.txt")" in *ALSO*) ok "an edited file misses the cache" ;;
    *) bad "an edited file misses the cache" "ALSO in the preview" "stale" ;; esac
pvgone="$RTD/swept"
XDG_RUNTIME_DIR="$pvgone" "$DIR/scripts/tmux-open-path" --preview-diff "$PV/big.txt" >/dev/null 2>&1
is "a vanished XDG_RUNTIME_DIR is not recreated by the cache (#136)" "" \
   "$(find "$pvgone" 2>/dev/null)"
rm -rf "$PV"

echo "== repo git config cannot run commands through a review or preview =="
GH=$(mktmpd); GHM="$GH/marker"
for h in fsmon textconv dcmd extdiff clean proc gpg pager; do
    printf '#!/bin/sh\necho %s >> %s\ncat\n' "$h" "$GHM" > "$GH/$h"; chmod +x "$GH/$h"
done
( cd "$GH" && mkdir r && cd r && git init -q -b main . \
  && printf 'a\n' > f.txt && printf '* diff=evil filter=evil\n' > .gitattributes \
  && git add -A && git -c user.name=t -c user.email=t@t commit -qm i && printf 'b\n' >> f.txt \
  && mkdir -p hk && printf '#!/bin/sh\necho hook >> %s\n' "$GHM" > hk/post-checkout && chmod +x hk/post-checkout \
  && git config core.fsmonitor "$GH/fsmon" && git config diff.evil.textconv "$GH/textconv" \
  && git config diff.evil.command "$GH/dcmd" && git config diff.external "$GH/extdiff" \
  && git config filter.evil.clean "$GH/clean" && git config log.showSignature true \
  && git config gpg.program "$GH/gpg" && git config core.hooksPath hk && git config core.pager "$GH/pager" ) >/dev/null 2>&1
git -C "$GH/r" diff HEAD >/dev/null 2>&1
case "$(cat "$GHM" 2>/dev/null)" in *fsmon*) ok "the planted config fires under plain git" ;;
    *) bad "the planted config fires under plain git" "fsmon in the marker" "$(cat "$GHM" 2>/dev/null)" ;; esac
rm -f "$GHM"
"$DIR/scripts/tmux-review" --lines "$GH/r" >/dev/null 2>&1
XDG_RUNTIME_DIR="$GH" "$DIR/scripts/tmux-open-path" --preview-diff "$GH/r/f.txt" >/dev/null 2>&1
"$DIR/scripts/tmux-open-path" --preview "$GH/r/f.txt" 1 >/dev/null 2>&1
is "and nothing it names runs under tmux-review or the previews" "" "$(cat "$GHM" 2>/dev/null)"
case "$("$DIR/scripts/tmux-review" --lines "$GH/r" 2>/dev/null)" in *f.txt*) ok "while the review still lists the change" ;;
    *) bad "while the review still lists the change" "a row for f.txt" "$("$DIR/scripts/tmux-review" --lines "$GH/r" 2>&1 | head -3)" ;; esac

echo "== a partial clone's lazy fetch cannot run the repo's planted remote config (#217) =="
# c's HEAD blob is missing, so a diff fetches it through the remote the repo names. The
# old/git shim drops GIT_NO_LAZY_FETCH as git < 2.44 does, leaving only the protocol pin.
LF=$(mktmpd); LFM="$LF/marker"; mkdir "$LF/old" "$LF/hb"
printf '#!/bin/sh\necho up >> %s\nexit 1\n' "$LFM" > "$LF/up"; chmod +x "$LF/up"; ln -s "$LF/up" "$LF/hb/git-remote-foo"
printf '#!/bin/sh\nunset GIT_NO_LAZY_FETCH\nexec %s "$@"\n' "$(command -v git)" > "$LF/old/git"; chmod +x "$LF/old/git"
( export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
  git init -q -b main "$LF/src" && cd "$LF/src" && git config uploadpack.allowFilter true \
  && printf 'a\n' > f.txt && git add -A && git commit -qm 1 \
  && git clone -q --filter=blob:none "file://$LF/src" "$LF/c" \
  && git switch -qc side && printf 'g\n' > g.txt && git add g.txt && git commit -qm g \
  && git switch -qc side2 main && printf 'h\n' > h.txt && git add h.txt && git commit -qm h \
  && git switch -q main && printf 'b\n' >> f.txt && git commit -qam 2 \
  && git clone -q --filter=blob:none "file://$LF/src" "$LF/c2" \
  && cd "$LF/c" && git fetch -q && git reset -q --soft origin/main \
  && git config remote.origin.uploadpack "$LF/up" && git config core.sshCommand "$LF/up" \
  && git config protocol.file.allow always && git config protocol.ssh.allow always \
  && git config protocol.ext.allow always && git config protocol.foo.allow always ) >/dev/null 2>&1
lf_paths() {
    ( . "$DIR/scripts/lib/git.sh"; git -C "$LF/c" diff --numstat HEAD ) >/dev/null 2>&1
    is "not through git.sh's git$1" "" "$(cat "$LFM" 2>/dev/null)"; rm -f "$LFM"
    ( . "$DIR/scripts/lib/git.sh"; . "$DIR/scripts/lib/review-diff.sh"; tree_diff "$LF/c" --numstat ) >/dev/null 2>&1
    is "nor through tree_diff$1" "" "$(cat "$LFM" 2>/dev/null)"; rm -f "$LFM"
    "$DIR/scripts/tmux-review" --lines "$LF/c" >/dev/null 2>&1
    is "nor through tmux-review --lines$1" "" "$(cat "$LFM" 2>/dev/null)"; rm -f "$LFM"
    XDG_RUNTIME_DIR="$(mktmpd)" "$DIR/scripts/tmux-open-path" --preview-diff "$LF/c/f.txt" >/dev/null 2>&1
    is "nor through the path picker's preview diff$1" "" "$(cat "$LFM" 2>/dev/null)"; rm -f "$LFM"
}
# foo:: is a remote helper on PATH (hb/), which no protocol.<p>.allow pin can name.
for lfu in "file://$LF/src" ssh://h/r "ext::$LF/up" foo::x; do
    git -C "$LF/c" config remote.origin.url "$lfu"
    PATH="$LF/hb:$PATH" git -C "$LF/c" diff --numstat HEAD >/dev/null 2>&1
    is "the planted remote fires under plain git (${lfu%%:*})" "up" "$(head -1 "$LFM" 2>/dev/null)"; rm -f "$LFM"
    PATH="$LF/hb:$PATH" lf_paths " (${lfu%%:*})"
    PATH="$LF/old:$LF/hb:$PATH" lf_paths " (${lfu%%:*}, git < 2.44)"
done
# A user's own switch or worktree add still fetches what it checks out; a diff does not.
is "a diff through git.sh's git still cannot fetch a clean clone's blob" "" \
   "$( . "$DIR/scripts/lib/git.sh"; git -C "$LF/c2" diff --numstat main origin/side 2>/dev/null)"
( . "$DIR/scripts/lib/git.sh"; git -C "$LF/c2" switch -q --track origin/side ) >/dev/null 2>&1
is "while switch --track through it fetches the branch's blob" "g" "$(cat "$LF/c2/g.txt" 2>/dev/null)"
( . "$DIR/scripts/lib/git.sh"; git -C "$LF/c2" worktree add -q --detach "$LF/wt" origin/side2 ) >/dev/null 2>&1
is "and so does worktree add" "h" "$(cat "$LF/wt/h.txt" 2>/dev/null)"
rm -rf "$LF"; unset lfu; unset -f lf_paths

echo "== tmux-open-path --preview-diff: two worktrees of one repo never share a cache entry =="
# The key used to be path+HEAD+mtime+size, with "path" whatever the caller
# passed -- and every caller cd's into the review target first and hands a
# DIR-RELATIVE path (the same one --lines emits). Two worktrees of the same
# repo, checked out to the same commit, have identical relative paths and can
# land the same mtime+size on an edit of the same shape -- reproduced: one
# worktree's preview served the other's cached diff. $d/$base (the file's own
# resolved directory, already computed above) is absolute and worktree-
# specific, so it now anchors the key.
WTBASE=$(mktmpd)
(
  cd "$WTBASE" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  mkdir -p sub; printf 'x\n' > sub/f.txt
  git add -A; git commit -q -m init
) >/dev/null 2>&1
wtsha=$(git -C "$WTBASE" rev-parse main)
WTA=$(mktmpd); rm -rf "$WTA"
WTB=$(mktmpd); rm -rf "$WTB"
git -C "$WTBASE" worktree add -q --detach "$WTA" "$wtsha" >/dev/null 2>&1
git -C "$WTBASE" worktree add -q --detach "$WTB" "$wtsha" >/dev/null 2>&1
# Same-second, same-size edits in each -- the axis the old key could not see
# past even before this fix (both are 9 bytes).
sed -i '1s/.*/ONLY_IN_A/' "$WTA/sub/f.txt"
sed -i '1s/.*/ONLY_IN_B/' "$WTB/sub/f.txt"
RTDW=$(mktmpd)
wta_out=$(cd "$WTA" && XDG_RUNTIME_DIR="$RTDW" "$DIR/scripts/tmux-open-path" --preview-diff sub/f.txt)
wtb_out=$(cd "$WTB" && XDG_RUNTIME_DIR="$RTDW" "$DIR/scripts/tmux-open-path" --preview-diff sub/f.txt)
case "$wta_out" in *ONLY_IN_A*) ok "worktree A's preview shows its own edit" ;;
    *) bad "worktree A's preview shows its own edit" "ONLY_IN_A" "$wta_out" ;; esac
case "$wtb_out" in *ONLY_IN_B*) ok "worktree B's preview shows its own edit, not a cache hit from A" ;;
    *) bad "worktree B's preview shows its own edit, not a cache hit from A" "ONLY_IN_B" "$wtb_out" ;; esac
case "$wtb_out" in *ONLY_IN_A*) bad "worktree B's cache never leaks worktree A's content" "no ONLY_IN_A" "$wtb_out" ;;
    *) ok "worktree B's cache never leaks worktree A's content" ;; esac
git -C "$WTBASE" worktree remove -f "$WTA" >/dev/null 2>&1
git -C "$WTBASE" worktree remove -f "$WTB" >/dev/null 2>&1
rm -rf "$WTBASE"

echo "== tmux-open-path --preview-diff: staging alone (no edit) is not served a stale render =="
# An unborn-HEAD repo's --numstat is computed against two different bases
# depending on index state: unstaged (index vs worktree) when the working
# tree still differs from the index, --cached (index vs the implicit empty
# tree) once it doesn't -- two genuinely different renders of the SAME file
# bytes at the SAME mtime, since `git add` never touches the file itself.
# The old key (no index component) could not tell these apart.
UB=$(mktmpd)
(
  cd "$UB" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\n' > f.txt; git add f.txt
  printf 'a\nb\n' > f.txt
) >/dev/null 2>&1
RTDU=$(mktmpd)
ub_pre=$(XDG_RUNTIME_DIR="$RTDU" "$DIR/scripts/tmux-open-path" --preview-diff "$UB/f.txt")
case "$ub_pre" in *'1 insertion(s)'*) ok "before staging, only the unstaged line is the diff" ;;
    *) bad "before staging, only the unstaged line is the diff" "1 insertion(s)" "$ub_pre" ;; esac
( cd "$UB" && git add f.txt ) >/dev/null 2>&1
ub_post=$(XDG_RUNTIME_DIR="$RTDU" "$DIR/scripts/tmux-open-path" --preview-diff "$UB/f.txt")
case "$ub_post" in *'2 insertion(s)'*) ok "staging alone (same bytes, same mtime) is not served the pre-stage cache hit" ;;
    *) bad "staging alone (same bytes, same mtime) is not served the pre-stage cache hit" "2 insertion(s)" "$ub_post" ;; esac
rm -rf "$UB"

echo "== tmux-open-path --preview-diff: a hostile cache dir is never read from or written into =="
# XDG_RUNTIME_DIR falling back to /tmp means the cache dir's own path can
# already be claimed: as a symlink elsewhere, or as a pre-existing dir this
# uid does not own or that is wider than 700. Both must still render
# correctly (uncached) and touch nothing at either path.
HC=$(mktmpd); ( cd "$HC" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'one\n' > h.txt; git add -A; git commit -q -m init; printf 'one\nTWO\n' > h.txt ) >/dev/null 2>&1

RTD4=$(mktmpd); EVILTARGET=$(mktmpd)
ln -s "$EVILTARGET" "$RTD4/tmux-cockpit-preview-$(id -u)"
symout=$(XDG_RUNTIME_DIR="$RTD4" "$DIR/scripts/tmux-open-path" --preview-diff "$HC/h.txt")
case "$symout" in *TWO*) ok "a symlinked cache dir still renders the real diff" ;;
    *) bad "a symlinked cache dir still renders the real diff" "TWO" "$symout" ;; esac
is "and nothing is written through the symlink into its target" "0" \
   "$(find "$EVILTARGET" -mindepth 1 2>/dev/null | wc -l | tr -d ' ')"

RTD5=$(mktmpd); mkdir -m 777 "$RTD5/tmux-cockpit-preview-$(id -u)"
wideout=$(XDG_RUNTIME_DIR="$RTD5" "$DIR/scripts/tmux-open-path" --preview-diff "$HC/h.txt")
case "$wideout" in *TWO*) ok "a world-writable pre-existing cache dir still renders the real diff" ;;
    *) bad "a world-writable pre-existing cache dir still renders the real diff" "TWO" "$wideout" ;; esac
is "and nothing is written into it either" "0" \
   "$(find "$RTD5/tmux-cockpit-preview-$(id -u)" -mindepth 1 2>/dev/null | wc -l | tr -d ' ')"
rm -rf "$HC"

echo "== tmux-review: a pane id that is not %<digits> never reaches tmux =="
# The glob %[0-9]* let "%1;display-message PWNED" through, and deliver() splices
# the pane into an if-shell command string. Every entry point refuses it.
PWD_SHIM=$(mktmpd); PWN='%1;display-message PWNED'
cat > "$PWD_SHIM/tmux" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$@" >> "$TMUX_LOG"; exit 0
STUB
chmod +x "$PWD_SHIM/tmux"; printf '## a:1 (+)\nx\n\n' > "$PWD_SHIM/store"
pwrun() { PATH="$PWD_SHIM:$PATH" TMUX_LOG="$PWD_SHIM/log" "$DIR/scripts/tmux-review" "$@" >/dev/null 2>&1; printf '%s ' "$?"; }
pwrcs=$(pwrun --deliver "$PWN" "$PWD_SHIM/store" "$PWD_SHIM"; pwrun --request "$PWN"; pwrun -p "$PWN" --info
        pwrun --delta "$PWN"; pwrun -p "$PWN")
is "deliver, request, -p --info, delta and -p all refuse it" "1 2 2 1 2 " "$pwrcs"
is "and no tmux call ever carries it" "0" "$(grep -c PWNED "$PWD_SHIM/log" 2>/dev/null || true)"
is "--dir prints nothing for it" "" "$(PATH="$PWD_SHIM:$PATH" TMUX_LOG="$PWD_SHIM/log" "$DIR/scripts/tmux-review" --dir "$PWN" 2>/dev/null)"
rm -rf "$PWD_SHIM"; unset -f pwrun

echo "== tmux-review: a temp git index is removed when the run is killed =="
# tree_diff's mktemp index outlives a TERM unless a trap removes it. The git
# shim TERMs the script mid-add, once the index copy exists.
TDK=$(mktmpd); mkdir -p "$TDK/repo" "$TDK/tmp" "$TDK/bin"
( cd "$TDK/repo" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'b\n' >> f ) >/dev/null 2>&1
cat > "$TDK/bin/git" <<STUB
#!/usr/bin/env bash
case " \$* " in *" add "*) kill -TERM "\$PPID"; exit 0 ;; esac
exec $(command -v git) "\$@"
STUB
chmod +x "$TDK/bin/git"
PATH="$TDK/bin:$PATH" TMPDIR="$TDK/tmp" bash -c '. "$1/git.sh"; . "$1/review-diff.sh"; tree_diff "$2"' _ "$DIR/scripts/lib" "$TDK/repo" >/dev/null 2>&1; tdrc=$?
is "the run was killed mid-diff (exit 143)" "143" "$tdrc"
is "and its temp index is gone" "" "$(ls -A "$TDK/tmp")"
rm -rf "$TDK"

# group: deliver
echo "== tmux-review --deliver: guards refuse before anything is pasted =="
if need_tmux "tmux-review --deliver"; then :; else
RDS="cockpit-rd-$$"; SOCKS="$SOCKS $RDS"
# -y 60: split-window below halves it, and the composed review's header grew
# to 13 lines (ai lens §7.F) -- AP's half needs to clear ~20 pasted lines
# without scrolling, or bash's readline garbles the unexecuted multi-line
# buffer redraw (reproduced: a 15-row pane truncates and interleaves lines).
"$TMUX_REAL" -L "$RDS" -f /dev/null new-session -d -s t -x 100 -y 60 \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" split-window -t t 'exec bash' >/dev/null 2>&1
# The two panes below are picked BY pane_current_command, so that is the thing
# to wait for: sorted, it is exactly one claude and one bash.
rds_cmds() { "$TMUX_REAL" -L "$RDS" list-panes -t t: -F '#{pane_current_command}' | sort | tr '\n' ' '; }
until_out 'bash claude ' rds_cmds
# The suite's private socket has no cockpit.tmux run-shell to default this --
# set it exactly as cockpit.tmux itself would (cockpit.tmux:136).
"$TMUX_REAL" -L "$RDS" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1

AP=$("$TMUX_REAL" -L "$RDS" list-panes -t t: -F '#{pane_id} #{pane_current_command}' \
     | awk '$2 == "claude" { print $1 }')
NP=$("$TMUX_REAL" -L "$RDS" list-panes -t t: -F '#{pane_id} #{pane_current_command}' \
     | awk '$2 == "bash" { print $1 }')

RDSTORE="$STATE/deliver-store-$$"
printf '## src.go:1 (+)\n> foo\nfix this\n\n' > "$RDSTORE"

# Driven from NP, a plain shell pane already inside the session -- exactly the
# way a real popup's `tmux run-shell -b` lands inside the session it targets,
# so a bare `tmux` call in the script resolves $TMUX to this socket with no
# -L threaded through by hand.
rundeliver() {
    # The typed command line itself contains "RC=$?", so count only markers
    # followed by a digit or "-" -- the ones a finished command printed.
    local n0 i; n0=$(npcap | grep -c 'RC=[0-9-]')
    "$TMUX_REAL" -L "$RDS" send-keys -t "$NP" \
        "$(shq_test "$DIR/scripts/tmux-review") --deliver $(shq_test "$1") $(shq_test "$2"); echo RC=\$?" Enter
    for i in $(seq 1 40); do
        [ "$(npcap | grep -c 'RC=[0-9-]')" -gt "$n0" ] && return 0
        sleep 0.1
    done
    return 1
}
shq_test() { printf '%q' "$1"; }
# -J joins soft-wrapped terminal lines back into one logical line -- without
# it, the long spillover-file path in the pointer sentence below wraps mid-
# path at the pane's column width and an exact-substring match on the whole
# sentence breaks on nothing but that cosmetic wrap.
apcap() { "$TMUX_REAL" -L "$RDS" capture-pane -p -J -t "$AP"; }
npcap() { "$TMUX_REAL" -L "$RDS" capture-pane -p -J -t "$NP"; }

# rundeliver's own "echo RC=$?" is the assertion of record for refusal, not
# just the absence of pasted text -- an absence-only check passes just as
# happily if the paste silently landed somewhere else uncaptured, or if the
# command never ran at all. lastrc() reads the LAST such marker off NP,
# in case an earlier call's own "RC=" is still in the same visible screen.
lastrc() { npcap | grep -o 'RC=[0-9-]*' | tail -1; }

rundeliver "not-a-pane-id" "$RDSTORE"
case "$(apcap)" in
    *"fix this"*) bad "a bogus pane id is refused, nothing pastes anywhere" \
         "no review text in the agent pane" "$(apcap)" ;;
    *) ok "a bogus pane id is refused, nothing pastes anywhere" ;;
esac
is "a bogus pane id is refused (exit 1)" "RC=1" "$(lastrc)"

rundeliver "$NP" "$RDSTORE"
case "$(npcap)" in
    *"fix this"*) bad "a non-agent pane is refused (would EXECUTE as shell input)" \
         "no review text in the shell pane" "$(npcap)" ;;
    *) ok "a non-agent pane is refused (would EXECUTE as shell input)" ;;
esac
is "a non-agent pane is refused (exit 1)" "RC=1" "$(lastrc)"

EMPTYSTORE="$STATE/deliver-empty-$$"
: > "$EMPTYSTORE"
rundeliver "$AP" "$EMPTYSTORE"
case "$(apcap)" in
    *"Code review of your changes"*) \
        bad "an empty store is refused, nothing to deliver" \
            "no composed header in the agent pane" "$(apcap)" ;;
    *) ok "an empty store is refused, nothing to deliver" ;;
esac
is "an empty store is refused (exit 1)" "RC=1" "$(lastrc)"

# group: deliver
echo "== tmux-review --deliver: the composed review lands typed-but-unsent =="
rundeliver "$AP" "$RDSTORE"
apafter=$(apcap)
case "$apafter" in
    *"## src.go:1 (+)"*"> foo"*"fix this"*) \
        ok "the composed review appears verbatim in the agent pane" ;;
    *) bad "the composed review appears verbatim in the agent pane" \
         "## src.go:1 (+) / > foo / fix this" "$apafter" ;;
esac
# Unexecuted: bash echoes a bracketed paste back as literal text and stays on
# the SAME prompt line it started from -- no "not found" errors, no second
# fresh prompt below the pasted block.
case "$apafter" in
    *'not found'*) bad "the paste never executes (no command errors)" \
         "no 'not found' anywhere" "$apafter" ;;
    *) ok "the paste never executes (no command errors)" ;;
esac
promptcount=$(printf '%s\n' "$apafter" | grep -c '\$ *$' || true)
is "the paste never executes (still one prompt, no fresh one after Enter)" \
   "0" "$promptcount"

# A delivered review is a STATE, not a 750 ms flash. The pane carries
# @cockpit_review=sent until the human actually sends it, so prefix u can walk
# back to the review still sitting unsent in someone's prompt.
is "a landed review marks the pane" "sent" \
   "$("$TMUX_REAL" -L "$RDS" show-option -pqv -t "$AP" @cockpit_review)"
# Driven from NP, a shell inside the session, so agent-state sees a real $TMUX
# exactly as a Claude Code hook does. Poll the option, never a fixed sleep.
"$TMUX_REAL" -L "$RDS" send-keys -t "$NP" \
    "$(shq_test "$DIR/scripts/agent-state") running $AP" Enter
for i in $(seq 1 20); do
    [ -z "$("$TMUX_REAL" -L "$RDS" show-option -pqv -t "$AP" @cockpit_review)" ] && break
    sleep 0.1
done
is "the mark clears when the agent starts working" "" \
   "$("$TMUX_REAL" -L "$RDS" show-option -pqv -t "$AP" @cockpit_review)"
# And prefix u walks to it. @panestate is `running` here (agent-state just set
# it), so the pane is NOT needs-input: the jump can only come from the review
# mark, not from the blocked-pane path the earlier tmux-jump block covers.
"$TMUX_REAL" -L "$RDS" set-option -p -t "$AP" @cockpit_review sent >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" select-pane -t "$NP" >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" send-keys -t "$NP" "$(shq_test "$DIR/scripts/tmux-jump")" Enter
for i in $(seq 1 20); do
    [ "$("$TMUX_REAL" -L "$RDS" display-message -p -t t: '#{pane_id}')" = "$AP" ] && break
    sleep 0.1
done
is "prefix u walks to a pane holding an unsent review" "$AP" \
   "$("$TMUX_REAL" -L "$RDS" display-message -p -t t: '#{pane_id}')"
"$TMUX_REAL" -L "$RDS" set-option -pu -t "$AP" @cockpit_review >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" select-pane -t "$NP" >/dev/null 2>&1

# group: deliver
echo "== tmux-review --deliver: a busy pane gets the review queued for its next turn boundary =="
"$TMUX_REAL" -L "$RDS" send-keys -t "$AP" C-u >/dev/null 2>&1      # clear the earlier paste
for busy in needs-input running; do
    "$TMUX_REAL" -L "$RDS" set-option -p -t "$AP" @panestate "$busy" >/dev/null 2>&1
    printf '## src.go:1 (+)\n> foo\nQUEUE-MARKER-%s fix\n\n' "$busy" > "$RDSTORE"
    rundeliver "$AP" "$RDSTORE"
    is "delivery into a $busy pane exits 0 (queued, not refused)" "RC=0" "$(lastrc)"
    case "$(apcap)" in
        *"QUEUE-MARKER-$busy"*) bad "nothing is pasted into a $busy pane" "no marker on the agent pane" "$(apcap)" ;;
        *) ok "nothing is pasted into a $busy pane" ;;
    esac
    is "the spool is armed on the pane" "$RDSTORE.deliver" \
       "$("$TMUX_REAL" -L "$RDS" show-option -pqv -t "$AP" @cockpit_review_spool)"
    case "$(cat "$RDSTORE.deliver" 2>/dev/null)" in
        "Code review of your changes"*"QUEUE-MARKER-$busy"*) ok "and the spool holds the composed review" ;;
        *) bad "and the spool holds the composed review" "composed header + marker" "$(cat "$RDSTORE.deliver" 2>/dev/null)" ;;
    esac
    is "the spool is born 0600" "600" "$(stat -c '%a' "$RDSTORE.deliver" 2>/dev/null || stat -f '%Lp' "$RDSTORE.deliver")"
    is "no buffer is left behind" "" "$("$TMUX_REAL" -L "$RDS" list-buffers -F '#{buffer_name}' 2>/dev/null | grep 'cockpit-review')"
    "$TMUX_REAL" -L "$RDS" set-option -pu -t "$AP" @cockpit_review_spool >/dev/null 2>&1
    rm -f "$RDSTORE.deliver"
done
"$TMUX_REAL" -L "$RDS" set-option -p -t "$AP" @panestate done >/dev/null 2>&1
rundeliver "$AP" "$RDSTORE"
case "$(apcap)" in
    *"QUEUE-MARKER-running"*) ok "the same review lands as a paste once the pane is done" ;;
    *) bad "the same review lands as a paste once the pane is done" "marker on the agent pane" "$(apcap)" ;;
esac
is "an idle delivery leaves no spool armed" "" "$("$TMUX_REAL" -L "$RDS" show-option -pqv -t "$AP" @cockpit_review_spool)"
# The spool is written BEFORE the busy/idle choice tmux makes, so the paste
# path writes one too and nobody will ever read it. Left behind, it also kept
# done_store's rmdir from ever emptying the session dir.
is "and no .deliver file behind" "absent" "$([ -e "$RDSTORE.deliver" ] && echo present || echo absent)"
"$TMUX_REAL" -L "$RDS" set-option -pu -t "$AP" @panestate >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" set-option -pu -t "$AP" @cockpit_review >/dev/null 2>&1

# group: deliver
echo "== tmux-review --deliver: concurrent deliveries do not cross-wire panes =="
# A second agent pane and a second control pane -- driving both deliveries
# from their OWN panes, fired back-to-back with no sleep between, is the
# shape that raced under the old fixed `-b cockpit-review` buffer name (tmux
# buffers are server-global): B's load-buffer could overwrite A's payload
# before A's paste-buffer -d ran.
AP2=$("$TMUX_REAL" -L "$RDS" split-window -t t -P -F '#{pane_id}' 'exec -a claude bash')
NP2=$("$TMUX_REAL" -L "$RDS" split-window -t t -P -F '#{pane_id}' 'exec bash')
until_out claude "$TMUX_REAL" -L "$RDS" display-message -p -t "$AP2" '#{pane_current_command}'
until_out bash "$TMUX_REAL" -L "$RDS" display-message -p -t "$NP2" '#{pane_current_command}'

ASTORE="$STATE/deliver-race-a-$$"
BSTORE="$STATE/deliver-race-b-$$"
printf '## a.go:1 (+)\n> a\nALPHA-MARKER-CONTENT\n\n' > "$ASTORE"
printf '## b.go:2 (+)\n> b\nBETA-MARKER-CONTENT\n\n' > "$BSTORE"

"$TMUX_REAL" -L "$RDS" send-keys -t "$NP" \
    "$(shq_test "$DIR/scripts/tmux-review") --deliver $(shq_test "$AP") $(shq_test "$ASTORE")" Enter
"$TMUX_REAL" -L "$RDS" send-keys -t "$NP2" \
    "$(shq_test "$DIR/scripts/tmux-review") --deliver $(shq_test "$AP2") $(shq_test "$BSTORE")" Enter
# Polled: NP2 is a fresh shell still sourcing its rc files, and a fixed 0.8 s
# read pane B before its delivery had run under a J=12 run (issue #44).
until_cap "$RDS" "$AP" '*ALPHA-MARKER-CONTENT*'
until_cap "$RDS" "$AP2" '*BETA-MARKER-CONTENT*'

raceA=$("$TMUX_REAL" -L "$RDS" capture-pane -p -J -t "$AP")
raceB=$("$TMUX_REAL" -L "$RDS" capture-pane -p -J -t "$AP2")

case "$raceA" in
    *"ALPHA-MARKER-CONTENT"*) ok "pane A gets its own review" ;;
    *) bad "pane A gets its own review" "ALPHA-MARKER-CONTENT" "$raceA" ;;
esac
case "$raceA" in
    *"BETA-MARKER-CONTENT"*) bad "pane A's review is not pane B's" \
         "no BETA-MARKER-CONTENT in pane A" "$raceA" ;;
    *) ok "pane A's review is not pane B's" ;;
esac
case "$raceB" in
    *"BETA-MARKER-CONTENT"*) ok "pane B gets its own review" ;;
    *) bad "pane B gets its own review" "BETA-MARKER-CONTENT" "$raceB" ;;
esac
case "$raceB" in
    *"ALPHA-MARKER-CONTENT"*) bad "pane B's review is not pane A's" \
         "no ALPHA-MARKER-CONTENT in pane B" "$raceB" ;;
    *) ok "pane B's review is not pane A's" ;;
esac

# group: deliver
echo "== tmux-review --deliver: an oversized review goes to a file, not the pane =="
# Both the old-name symlink target and the spillover glob below are keyed
# only on the PANE ID ("${AP#%}") -- which tmux hands out per-socket
# starting at %0/%1, so two of THIS suite's own runs going concurrently can
# land on the very same pane id. Route deliver()'s own XDG_RUNTIME_DIR into
# this run's private $STATE (already unique -- mktemp -d, no shared /tmp)
# so the names below can't collide with a sibling run's.
RUNDIR="$STATE/rd-runtime-$$"
mkdir -p "$RUNDIR"
"$TMUX_REAL" -L "$RDS" send-keys -t "$NP" \
    "export XDG_RUNTIME_DIR=$(shq_test "$RUNDIR"); echo XRD=\$?" Enter
# The export has to be in the shell's environment before the delivery below
# reads it, and only the finished command prints a digit after the =.
until_cap "$RDS" "$NP" '*XRD=[0-9]*'
OLDPATH="$RUNDIR/tmux-cockpit-review-${AP#%}.md"
CANARY="$STATE/deliver-canary-$$"
printf 'untouched canary\n' > "$CANARY"
# A symlink planted at the OLD, predictable filename this delivery used to
# write -- if delivery still wrote through a guessable name, this symlink
# would let whoever planted it capture (or corrupt) whatever it points at.
# mktemp's own randomized suffix should make this exact name irrelevant.
ln -sf "$CANARY" "$OLDPATH"

BIGSTORE="$STATE/deliver-big-$$"
: > "$BIGSTORE"
for i in $(seq 1 40); do
    printf '## src.go:%d (+)\n> line %d\nthis comment pads the payload past 2048 bytes on purpose\n\n' \
        "$i" "$i" >> "$BIGSTORE"
done
bigbytes=$("$DIR/scripts/tmux-review" --compose "$BIGSTORE" | wc -c)
if [ "$bigbytes" -le 2048 ]; then
    bad "fixture actually exceeds the 2048-byte threshold" "> 2048 bytes" "$bigbytes bytes"
else
    ok "fixture actually exceeds the 2048-byte threshold ($bigbytes bytes)"
fi
"$TMUX_REAL" -L "$RDS" send-keys -t "$AP" C-u >/dev/null 2>&1  # clear the earlier paste
rundeliver "$AP" "$BIGSTORE"

is "the planted symlink at the old predictable name is never followed" \
   "untouched canary" "$(cat "$CANARY")"
if [ -L "$OLDPATH" ]; then
    ok "the old predictable path is still just the planted symlink"
else
    bad "the old predictable path is still just the planted symlink" \
        "a symlink, unchanged" "no longer a symlink"
fi

BIGFILE=""
for f in "$RUNDIR"/tmux-cockpit-review-"${AP#%}"-*; do
    [ -e "$f" ] && BIGFILE=$f
done
if [ -n "$BIGFILE" ]; then
    ok "the spillover file exists, named with mktemp's own random suffix"
else
    bad "the spillover file exists, named with mktemp's own random suffix" \
        "$RUNDIR/tmux-cockpit-review-${AP#%}-XXXXXX" "no such file"
fi
if [ -f "$BIGFILE" ] && [ ! -L "$BIGFILE" ]; then
    ok "the spillover file is a real file, not a symlink"
else
    bad "the spillover file is a real file, not a symlink" \
        "a regular file" "$([ -L "$BIGFILE" ] && echo symlink || echo missing)"
fi
bigowner=$(stat -c '%U' "$BIGFILE" 2>/dev/null || stat -f '%Su' "$BIGFILE" 2>/dev/null)
is "the spillover file is owned by us" "$(id -un)" "$bigowner"
bigperm=$(stat -c '%a' "$BIGFILE" 2>/dev/null || stat -f '%Lp' "$BIGFILE" 2>/dev/null)
is "the spillover file is 0600" "600" "$bigperm"
is "the spillover file holds the full composed review" \
   "$("$DIR/scripts/tmux-review" --compose "$BIGSTORE")" "$(cat "$BIGFILE")"
bigpaneafter=$(apcap)
case "$bigpaneafter" in
    *"Read and address every comment in $BIGFILE"*) \
        ok "only the short pointer is pasted into the pane, naming the real path" ;;
    *) bad "only the short pointer is pasted into the pane, naming the real path" \
         "Read and address every comment in $BIGFILE" "$bigpaneafter" ;;
esac
case "$bigpaneafter" in
    *"this comment pads the payload"*) \
        bad "the full review text never reaches the pane itself" \
            "no comment body text in the pane" "$bigpaneafter" ;;
    *) ok "the full review text never reaches the pane itself" ;;
esac
rm -f "$BIGFILE" "$OLDPATH" "$CANARY"

# A new spill keeps this pane's two newest older ones and deletes the rest;
# another pane's file is not ours to touch.
for t in 1 2 3; do
    : > "$RUNDIR/tmux-cockpit-review-${AP#%}-old00$t"; touch -t "20200101000$t" "$RUNDIR/tmux-cockpit-review-${AP#%}-old00$t"
done
: > "$RUNDIR/tmux-cockpit-review-99999-old001"; touch -t 202001010001 "$RUNDIR/tmux-cockpit-review-99999-old001"
"$TMUX_REAL" -L "$RDS" send-keys -t "$AP" C-u >/dev/null 2>&1
rundeliver "$AP" "$BIGSTORE"
is "a new spill deletes the oldest of this pane's earlier ones" "gone" \
   "$([ -e "$RUNDIR/tmux-cockpit-review-${AP#%}-old001" ] && echo kept || echo gone)"
is "and keeps the two newest, and another pane's" "kept kept kept" \
   "$(for f in "${AP#%}-old002" "${AP#%}-old003" 99999-old001; do [ -e "$RUNDIR/tmux-cockpit-review-$f" ] && printf 'kept ' || printf 'gone '; done | sed 's/ $//')"
rm -f "$RUNDIR"/tmux-cockpit-review-*

"$TMUX_REAL" -L "$RDS" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --deliver: a window name cannot inject tmux format syntax into the notice =="
# display-message's argument is a tmux FORMAT, not a plain string, and the
# delivery notice interpolates the target's WINDOW NAME -- which is user data.
# Measured on tmux 3.7c, with a client attached:
#   display-message -p 'x=[#{@probe}]'          -> the option's VALUE (expanded)
#   display-message    'x #(touch F) y'         -> F is NOT created (no job runs)
#   set -g status-right '#(touch F)'            -> F IS created
# So the reachable vector is #{...} substitution, not #(); a window named
# w#{@anything} leaks that option's value into the notice, and the same
# substitution is how a format reaches things the reviewer never asked for.
# Doubling every # to ## is the tmux format literal and covers the #() arm too,
# whatever a future tmux decides to run from a message.
#
# Its own server, not $RDS: the notice only lands on a status line if a CLIENT
# is there to draw it, so an OUTER tmux provides the pty for a client attached
# to the INNER one and capture-pane on the outer pane reads the drawn line back
# (the same outer/inner pattern as the RCE-via-directory-name block above).
if need_tmux "tmux-review --deliver format injection"; then :; else
FI_IN="cockpit-fi-in-$$"; FI_OUT="cockpit-fi-out-$$"
SOCKS="$SOCKS $FI_IN $FI_OUT"
FISP=$(mktmpd)
"$TMUX_REAL" -L "$FI_IN" -f /dev/null new-session -d -s t -x 250 -y 24 \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$FI_IN" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$FI_IN" set -g @cockpit_secret LEAKED-SECRET >/dev/null 2>&1
# rename-window EXPANDS its own argument, so the hostile name has to be written
# escaped to be STORED literally; it also turns automatic-rename off for the
# window, so the name sticks for the whole delivery. Verified stored raw:
# display-message -p '#{window_name}' reads back w#{@cockpit_secret}#(...)z --
# a substituted value is not re-expanded, so the READ is safe and only the
# notice that re-interpolates it is not.
"$TMUX_REAL" -L "$FI_IN" rename-window -t t:0 \
    "w##{@cockpit_secret}##(touch $FISP/OWNED)z" >/dev/null 2>&1
FIP=$("$TMUX_REAL" -L "$FI_IN" list-panes -t t: -F '#{pane_id}')
is "the hostile window name is stored verbatim, not expanded at rename time" \
   "w#{@cockpit_secret}#(touch $FISP/OWNED)z" \
   "$("$TMUX_REAL" -L "$FI_IN" display-message -p -t t:0 '#{window_name}')"
"$TMUX_REAL" -L "$FI_OUT" -f /dev/null new-session -d -s o -x 250 -y 24 \
    "$TMUX_REAL -L $FI_IN attach -t t" >/dev/null 2>&1
for i in $(seq 1 30); do
    [ -n "$("$TMUX_REAL" -L "$FI_IN" list-clients -t t: 2>/dev/null)" ] && break
    sleep 0.1
done
FISTORE="$STATE/deliver-fi-$$"
printf '## src.go:1 (+)\n> foo\nFI-MARKER fix\n\n' > "$FISTORE"
TMUX="$SOCKDIR/$FI_IN,0,0" "$DIR/scripts/tmux-review" --deliver "$FIP" "$FISTORE" \
    >/dev/null 2>&1
for i in $(seq 1 30); do
    "$TMUX_REAL" -L "$FI_OUT" capture-pane -p -t o: | grep -q 'review pasted into' && break
    sleep 0.1
done
finotice=$("$TMUX_REAL" -L "$FI_OUT" capture-pane -p -t o: | grep 'review pasted into')
# Asserted FIRST: an empty notice would make every check below pass vacuously
# (no notice, therefore no leaked value in it), which is exactly how a broken
# delivery would read as a green security test.
case "$finotice" in
    '') bad "the delivery notice reaches the attached client's status line" \
         "a 'review pasted into ...' line" "nothing drawn on the status line" ;;
    *)  ok "the delivery notice reaches the attached client's status line" ;;
esac
case "$finotice" in
    *LEAKED-SECRET*) bad "a window name's #{...} is never expanded in the notice" \
         "the literal name, not the option's value" "$finotice" ;;
    *) ok "a window name's #{...} is never expanded in the notice" ;;
esac
case "$finotice" in
    *'w#{@cockpit_secret}'*) ok "the notice shows the window name verbatim" ;;
    *) bad "the notice shows the window name verbatim" \
         "w#{@cockpit_secret} in the notice" "$finotice" ;;
esac
# Fixed wait, not a poll: this is the negative half -- there is no event to
# wait for, only the absence of one. Capped at 0.3s to keep it bounded.
# tmux 3.7c runs no #() job from a display-message at all (probe above), so
# this is a regression tooth rather than a failure this fix turned green.
sleep 0.3
if [ -e "$FISP/OWNED" ]; then
    bad "a window name's #(...) never runs" "no OWNED file" "OWNED file created"
else
    ok "a window name's #(...) never runs"
fi
# Escaping the name must not cost the delivery itself.
case "$("$TMUX_REAL" -L "$FI_IN" capture-pane -p -J -t "$FIP")" in
    *"FI-MARKER fix"*) ok "the review still lands in a hostilely named window" ;;
    *) bad "the review still lands in a hostilely named window" "FI-MARKER fix in the pane" \
         "$("$TMUX_REAL" -L "$FI_IN" capture-pane -p -J -t "$FIP")" ;;
esac
"$TMUX_REAL" -L "$FI_OUT" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$FI_IN"  kill-server >/dev/null 2>&1
fi

# tests: scripts/tmux-review
echo "== tmux-review --deliver: the paste is gated atomically, so a pane that stopped being an agent never gets it =="
# deliver()'s shell-side is-agent check is a ~50ms-stale snapshot -- a compose
# re-exec, a wc, an mktemp and a load-buffer sit between it and the paste. Kill
# the agent in that window (a `claude` foreground child exits, the pane's own
# shell is back) and the old unconditional `paste-buffer` landed the review in
# that shell (the hunt reproduced it 6/6): a live, one-Enter-from-running
# command line the reviewer never targeted. The end-to-end transient is
# timing-bound, so this checks the mechanism deterministically: the exact
# load-buffer + atomic if-shell that deliver() now runs, once against the shell
# that outlived the agent and once against a live agent.
if need_tmux "tmux-review --deliver atomic gate"; then :; else
RG="cockpit-rg-$$"; SOCKS="$SOCKS $RG"
"$TMUX_REAL" -L "$RG" -f /dev/null new-session -d -s t -x 100 -y 30 \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RG" split-window -t t 'exec bash' >/dev/null 2>&1
# The two panes below are picked BY pane_current_command, so that is the wait.
rg_cmds() { "$TMUX_REAL" -L "$RG" list-panes -t t: -F '#{pane_current_command}' | sort | tr '\n' ' '; }
until_out 'bash claude ' rg_cmds
"$TMUX_REAL" -L "$RG" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
APG=$("$TMUX_REAL" -L "$RG" list-panes -t t: -F '#{pane_id} #{pane_current_command}' \
     | awk '$2 == "claude" { print $1 }')
NPG=$("$TMUX_REAL" -L "$RG" list-panes -t t: -F '#{pane_id} #{pane_current_command}' \
     | awk '$2 == "bash" { print $1 }')
gcap() { "$TMUX_REAL" -L "$RG" capture-pane -p -J -t "$1"; }

# The hazard the old code carried: once its stale check had passed, deliver()
# ran a plain `paste-buffer` -- and into a shell that lands.
printf 'HAZARD-MARKER\n' | "$TMUX_REAL" -L "$RG" load-buffer -b rg-hz -
"$TMUX_REAL" -L "$RG" paste-buffer -p -r -d -b rg-hz -t "$NPG"
until_cap "$RG" "$NPG" '*HAZARD-MARKER*'
case "$(gcap "$NPG")" in
    *HAZARD-MARKER*) ok "an unconditional paste into the shell DOES land (the hazard is real)" ;;
    *) bad "an unconditional paste into the shell DOES land (the hazard is real)" \
         "HAZARD-MARKER in the shell" "$(gcap "$NPG")" ;;
esac
# Clear the shell's readline of the pasted-but-unsent hazard line before the
# real assertion, so a later match cannot see it. The sleep stays: a discarded
# readline buffer leaves nothing on screen to poll for.
"$TMUX_REAL" -L "$RG" send-keys -t "$NPG" C-c >/dev/null 2>&1
sleep 0.2

# The gate deliver() actually ships, in one string so this block cannot drift
# from it again: the #{m:...} command test AND #{!=:#{@panestate},needs-input},
# joined with #{&&:...} so tmux evaluates BOTH inside the same command-queue
# item as the paste. This block used to hand-build the single-condition form
# under the "byte-for-byte" comment below, which stopped being true the moment
# guard 3 was folded into the format -- and a typo inside the compound would
# evaluate false and silently never deliver, with nothing here to catch it.
RGGATE='#{&&:#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{!=:#{@panestate},needs-input}}'

# The fix, byte-for-byte as deliver() runs it: stage the buffer, then let tmux
# re-check #{pane_current_command} in the SAME command-queue item as the paste.
# The pane is a shell at paste time -> refused, and the staged buffer dropped.
printf 'RACE-MARKER-CONTENT\n' | "$TMUX_REAL" -L "$RG" load-buffer -b rg-gate -
"$TMUX_REAL" -L "$RG" if-shell -F -t "$NPG" \
    "$RGGATE" \
    "paste-buffer -p -r -d -b rg-gate -t $NPG ; display-message pasted" \
    "delete-buffer -b rg-gate ; display-message 'not delivered'"
# The sleep stays: the assertion is that the marker never reaches the pane, and
# a paste that WOULD have landed needs time on the wire to prove it did not.
sleep 0.3
case "$(gcap "$NPG")" in
    *RACE-MARKER-CONTENT*) bad "the atomic gate refuses the paste into a shell at paste time" \
         "no RACE-MARKER-CONTENT in the shell" "$(gcap "$NPG")" ;;
    *) ok "the atomic gate refuses the paste into a shell at paste time" ;;
esac
if "$TMUX_REAL" -L "$RG" list-buffers 2>/dev/null | grep -q 'rg-gate'; then
    bad "the refused delivery drops its staged buffer" "no rg-gate buffer" "rg-gate still present"
else
    ok "the refused delivery drops its staged buffer"
fi

# The same gated sequence, targeting a live agent, still delivers.
printf 'DELIVERED-MARKER\n' | "$TMUX_REAL" -L "$RG" load-buffer -b rg-ok -
"$TMUX_REAL" -L "$RG" if-shell -F -t "$APG" \
    "$RGGATE" \
    "paste-buffer -p -r -d -b rg-ok -t $APG ; display-message pasted" \
    "delete-buffer -b rg-ok ; display-message 'not delivered'"
until_cap "$RG" "$APG" '*DELIVERED-MARKER*'
case "$(gcap "$APG")" in
    *DELIVERED-MARKER*) ok "the gate still delivers into a pane that is a live agent" ;;
    *) bad "the gate still delivers into a pane that is a live agent" \
         "DELIVERED-MARKER in the agent pane" "$(gcap "$APG")" ;;
esac

# The compound's OTHER arm, which had no coverage anywhere: the pane is still a
# live agent, but it reached needs-input in the same ~50ms window -- guard 3
# passed against a snapshot that is already stale. The paste must not land: a
# permission prompt takes the text into the dialog, where the Enter this script
# tells the human to press becomes a blind approval of whatever the agent asked
# for. @panestate is set on the pane BETWEEN the guard and the gate here, which
# is exactly the race, made deterministic.
"$TMUX_REAL" -L "$RG" set-option -p -t "$APG" @panestate needs-input
printf 'NEEDS-INPUT-MARKER\n' | "$TMUX_REAL" -L "$RG" load-buffer -b rg-ni -
"$TMUX_REAL" -L "$RG" if-shell -F -t "$APG" \
    "$RGGATE" \
    "paste-buffer -p -r -d -b rg-ni -t $APG ; display-message pasted" \
    "delete-buffer -b rg-ni ; display-message 'not delivered'"
# Both arms consume the buffer (paste-buffer -d, or delete-buffer), so its
# disappearance says the queue item RAN without saying which way it went --
# a bounded poll, not a blind wait, and the two assertions below then read a
# settled pane.
for i in $(seq 1 30); do
    "$TMUX_REAL" -L "$RG" list-buffers -F '#{buffer_name}' 2>/dev/null \
        | grep -qxF rg-ni || break
    sleep 0.1
done
case "$(gcap "$APG")" in
    *NEEDS-INPUT-MARKER*) bad "the gate refuses a live agent that reached needs-input" \
         "no NEEDS-INPUT-MARKER in the agent pane" "$(gcap "$APG")" ;;
    *) ok "the gate refuses a live agent that reached needs-input" ;;
esac
if "$TMUX_REAL" -L "$RG" list-buffers -F '#{buffer_name}' 2>/dev/null | grep -qxF rg-ni; then
    bad "the needs-input refusal drops its staged buffer too" "no rg-ni buffer" "rg-ni still present"
else
    ok "the needs-input refusal drops its staged buffer too"
fi
"$TMUX_REAL" -L "$RG" set-option -p -u -t "$APG" @panestate

# B2: the `tmux if-shell` call above returns 0 (the queue accepted it)
# whichever nested branch fired -- so a shell that only inspected THAT exit
# code could not tell a race-refusal from a delivery, and deliver() used to
# fall through with RC=0 either way, silently breaking the exit-1 refusal
# contract every early guard in this same function already honors.
# deliver() now checks the staged buffer's own survival afterward instead:
# the buffer is loaded before the gate runs, and only the DELIVERED branch's
# `paste-buffer -d` consumes it -- so a buffer still present after the call
# returned IS the refusal, readable without re-testing the (already stale)
# pane. rc_from_buffer() below is that check, copied verbatim from deliver().
rc_from_buffer() {
    if "$TMUX_REAL" -L "$RG" list-buffers -F '#{buffer_name}' 2>/dev/null \
         | grep -qxF "$1"; then
        "$TMUX_REAL" -L "$RG" delete-buffer -b "$1" 2>/dev/null
        printf '1\n'; return
    fi
    printf '0\n'
}

# The OLD refused branch deleted the buffer INLINE (the shape deliver() sent
# before this fix) -- by the time the CLI call returns there is nothing left
# to distinguish "refused" from "delivered and consumed", so the RC check
# above reads 0. The hazard is real: this is exactly why deliver() used to
# report a race-refusal as success.
printf 'OLD-RACE\n' | "$TMUX_REAL" -L "$RG" load-buffer -b rg-old -
"$TMUX_REAL" -L "$RG" if-shell -F -t "$NPG" \
    '#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}}' \
    "paste-buffer -p -r -d -b rg-old -t $NPG ; display-message pasted" \
    "delete-buffer -b rg-old ; display-message 'not delivered'"
is "the old inline delete on refusal hides the refusal from an RC check (the hazard is real)" \
   "0" "$(rc_from_buffer rg-old)"

# The fixed refused branch leaves the buffer for deliver()'s own post-check to
# find and clean up -- the same RC=1 contract the early guards already use.
printf 'NEW-RACE\n' | "$TMUX_REAL" -L "$RG" load-buffer -b rg-new -
"$TMUX_REAL" -L "$RG" if-shell -F -t "$NPG" \
    '#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}}' \
    "paste-buffer -p -r -d -b rg-new -t $NPG ; display-message pasted" \
    "display-message 'tmux-review: $NPG is no longer an agent — not delivered'"
is "the fixed refused branch's surviving buffer reports the same RC=1 the early guards use" \
   "1" "$(rc_from_buffer rg-new)"
if "$TMUX_REAL" -L "$RG" list-buffers 2>/dev/null | grep -q 'rg-new'; then
    bad "the RC check also cleans up the buffer it found" "no rg-new buffer" "rg-new still present"
else
    ok "the RC check also cleans up the buffer it found"
fi

# A real delivery -- the buffer is consumed, so the same check reports RC=0.
# Seen red once at load 8 (#127) and never reproduced: "1" only says the buffer
# survived. The refusal arm records the command the gate saw, in the same queue
# item, since a read after the client returns can already say claude again.
printf 'NEW-OK\n' | "$TMUX_REAL" -L "$RG" load-buffer -b rg-newok -
"$TMUX_REAL" -L "$RG" if-shell -F -t "$APG" \
    '#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}}' \
    "paste-buffer -p -r -d -b rg-newok -t $APG ; display-message pasted" \
    "set -p -t $APG -F @rg_seen '#{pane_current_command}' ; \
display-message 'tmux-review: $APG is no longer an agent — not delivered'"
rg_bufs=$("$TMUX_REAL" -L "$RG" list-buffers -F '#{buffer_name}' 2>/dev/null | tr '\n' ' ')
rg_seen=$("$TMUX_REAL" -L "$RG" show -pqv -t "$APG" @rg_seen 2>/dev/null)
rg_cmd=$("$TMUX_REAL" -L "$RG" display -p -t "$APG" '#{pane_current_command}' 2>/dev/null)
rg_scr=$(gcap "$APG" | grep -v "^$" | tail -3)
case "$(rc_from_buffer rg-newok)" in
    0) ok "a real delivery still reports RC=0" ;;
    *) bad "a real delivery still reports RC=0" "0" \
         "1 (buffers [$rg_bufs] pane $APG saw [$rg_seen] now [$rg_cmd] screen [$rg_scr])" ;;
esac

"$TMUX_REAL" -L "$RG" kill-server >/dev/null 2>&1
fi

echo "== tmux-review: a delivered review's comment store does not outlive the popup that made it =="
# review_pane()'s own store -- unlike --deliver's spillover file, this one is
# never named to us. Redirect XDG_STATE_HOME to a private, otherwise-empty
# directory for the pane driving this, so whatever session_dir() creates
# there is unambiguously it.
if need_tmux "tmux-review fleet-to-deliver store cleanup"; then :; else
FD="cockpit-fd-$$"; SOCKS="$SOCKS $FD"
FDTMPDIR="$STATE/fd-tmp-$$"
mkdir -p "$FDTMPDIR"
FDB=$(mktmpd); FDW=$(mktmpd); fake_revdiff "$FDB"
printf '## src.go:3 (+)\nfix this\n' > "$FDW/out.1"; echo 10 > "$FDW/rc.1"
"$TMUX_REAL" -L "$FD" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$FD" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
CTL2=$("$TMUX_REAL" -L "$FD" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" \
    "export XDG_STATE_HOME=$(shq_test "$FDTMPDIR") PATH=$(shq_test "$FDB"):\$PATH FAKE_RD_DIR=$(shq_test "$FDW"); echo XSH=\$?" Enter
# The export has to be in the shell's environment before the review below runs
# there, and only the finished command prints a digit after the =.
until_cap "$FD" "$CTL2" '*XSH=[0-9]*'

FD_DIRTY=$(mktmpd)
(
  cd "$FD_DIRTY" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go
  git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1

# -d: main_entry() resolves its pane with a bare `display-message -p`, no
# -t -- through the session's ACTIVE pane. Without -d, new-window switches
# focus to the window it just made, and the command typed into CTL2 would
# resolve against FD_AGENT instead, reviewing it directly and skipping the
# fleet queue this test means to exercise.
"$TMUX_REAL" -L "$FD" new-window -d -t t: -c "$FD_DIRTY" 'exec -a claude bash' >/dev/null 2>&1
FD_AGENT=$("$TMUX_REAL" -L "$FD" list-panes -t t:1 -F '#{pane_id}')
# The queue below picks this pane by its command, so wait for the command.
until_out claude "$TMUX_REAL" -L "$FD" display-message -p -t "$FD_AGENT" '#{pane_current_command}'

# The fleet queue, from the non-agent control pane -- main_entry()'s own
# routing, the same path a real prefix-R press from a shell pane takes.
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" "'$DIR/scripts/tmux-review'" Enter
fdup=0
for i in 1 2 3 4 5 6 7 8 9 10; do
    case "$("$TMUX_REAL" -L "$FD" capture-pane -p -t "$CTL2")" in
        *"fleet>"*) fdup=1; break ;;
    esac
    sleep 0.5
done
if [ "$fdup" != 1 ]; then
    bad "the fleet queue comes up" "a visible 'fleet>' prompt" \
        "$("$TMUX_REAL" -L "$FD" capture-pane -p -t "$CTL2")"
else
    ok "the fleet queue comes up"
fi

"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" Enter
rvup=0
for i in 1 2 3 4 5 6 7 8 9 10; do
    case "$("$TMUX_REAL" -L "$FD" capture-pane -p -t "$CTL2")" in
        *"review · base"*) rvup=1; break ;;
    esac
    sleep 0.5
done
if [ "$rvup" != 1 ]; then
    bad "selecting the one row opens its review loop" "a visible 'review · base' prompt" \
        "$("$TMUX_REAL" -L "$FD" capture-pane -p -t "$CTL2")"
else
    ok "selecting the one row opens its review loop"
fi

storefile=""
sf=$(ls -d "$FDTMPDIR"/tmux-cockpit/review/*/comments.md 2>/dev/null | head -1)
[ -n "$sf" ] && storefile="$sf"
if [ -n "$storefile" ] && [ -e "$storefile" ]; then
    ok "the review's comment store exists while the popup is open"
else
    bad "the review's comment store exists while the popup is open" \
        "one */comments.md under $FDTMPDIR/tmux-cockpit/review" "none"
fi
fdmode() { stat -c '%a' "$1" 2>/dev/null || stat -f '%Lp' "$1" 2>/dev/null || echo none; }
# The store holds the whole review and its dir the delivered archive: private.
is "the session dir is born 0700" "700" "$(fdmode "${storefile%/*}")"
is "the store is born 0600" "600" "$(fdmode "$storefile")"
is "and the state parent is 0700 too" "700" "$(fdmode "$FDTMPDIR/tmux-cockpit")"

until_cap "$FD" "$CTL2" '*1 comment(s) pinned*'
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" d
until_cap "$FD" "$FD_AGENT" '*fix this*'

case "$("$TMUX_REAL" -L "$FD" capture-pane -p -J -t "$FD_AGENT")" in
    *"fix this"*) ok "the review still gets delivered through the fleet path" ;;
    *) bad "the review still gets delivered through the fleet path" \
         "the comment in the agent pane" \
         "$("$TMUX_REAL" -L "$FD" capture-pane -p -J -t "$FD_AGENT")" ;;
esac
for i in $(seq 1 40); do [ -n "$storefile" ] && [ -e "$storefile" ] || break; sleep 0.1; done
if [ -n "$storefile" ] && [ -e "$storefile" ]; then
    bad "the comment store does not outlive the delivered review" \
        "$storefile gone" "$storefile still exists"
else
    ok "the comment store does not outlive the delivered review"
fi
sessdir="${storefile%/*}"
for i in $(seq 1 40); do [ -s "$sessdir/delivered.md" ] && break; sleep 0.1; done
if [ -d "$sessdir" ]; then
    ok "a delivered review's session dir stays -- the batch is archived, not the dir"
else
    bad "a delivered review's session dir stays -- the batch is archived, not the dir" \
        "$sessdir present" "$sessdir gone"
fi
case "$(cat "$sessdir/delivered.md" 2>/dev/null)" in
    *"fix this"*) ok "and delivered.md holds the delivered comment" ;;
    *) bad "and delivered.md holds the delivered comment" "fix this in delivered.md" "$(cat "$sessdir/delivered.md" 2>/dev/null)" ;;
esac

"$TMUX_REAL" -L "$FD" kill-server >/dev/null 2>&1
rm -rf "$FD_DIRTY" "$FDTMPDIR"
fi

echo "== tmux-review: a delivery into a busy pane is queued — the store is archived, the spool holds the review, nothing is pasted =="
# The mirror of the block above, now for a busy target. review_pane() chains
# the store cleanup after --deliver inside `tmux run-shell -b`, and --deliver
# runs AFTER the popup is torn down -- but deliver() no longer refuses a busy
# pane (needs-input, running): it spools the composed review beside the store
# and arms @cockpit_review_spool, and review-gate drains that spool at the
# agent's next Stop. Because deliver() now reports success (rc=0) even though
# nothing was pasted, review_pane()'s chain still runs --done: the store is
# archived into delivered.md like any other successful delivery, and only the
# spool -- not the store -- carries the review onward from here.
if need_tmux "tmux-review busy-delivery queue"; then :; else
RK="cockpit-rk-$$"; SOCKS="$SOCKS $RK"
RKTMPDIR="$STATE/rk-tmp-$$"
mkdir -p "$RKTMPDIR"
RKB=$(mktmpd); RKW=$(mktmpd); fake_revdiff "$RKB"
printf '## src.go:3 (+)\nkeep me\n' > "$RKW/out.1"; echo 10 > "$RKW/rc.1"
"$TMUX_REAL" -L "$RK" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RK" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
RKCTL=$("$TMUX_REAL" -L "$RK" list-panes -t t:0 -F '#{pane_id}')
# Same XDG_STATE_HOME redirect as the block above: review_pane()'s session
# dir lands under it. The echoed marker is the poll's condition -- `export`
# is a builtin, so the pane's own process never changes and there is nothing
# else to watch for. The typed line carries the literal "RKTMP=%s", not the
# expanded path, so a match on the path is the shell's OUTPUT, not the
# command echo.
"$TMUX_REAL" -L "$RK" send-keys -t "$RKCTL" \
    "export XDG_STATE_HOME=$(shq_test "$RKTMPDIR") PATH=$(shq_test "$RKB"):\$PATH FAKE_RD_DIR=$(shq_test "$RKW"); printf 'RKTMP=%s\\n' \"\$XDG_STATE_HOME\"" Enter
for i in $(seq 1 20); do
    "$TMUX_REAL" -L "$RK" capture-pane -p -J -t "$RKCTL" | grep -qF "RKTMP=$RKTMPDIR" && break
    sleep 0.1
done

RK_DIRTY=$(mktmpd)
(
  cd "$RK_DIRTY" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go
  git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1

"$TMUX_REAL" -L "$RK" new-window -d -t t: -c "$RK_DIRTY" 'exec -a claude bash' >/dev/null 2>&1
RK_AGENT=$("$TMUX_REAL" -L "$RK" list-panes -t t:1 -F '#{pane_id}')
# The target is sitting on a permission prompt -- deliver()'s busy path, the
# state a human is most likely to leave a pane in right after finishing a
# review.
"$TMUX_REAL" -L "$RK" set-option -p -t "$RK_AGENT" @panestate needs-input >/dev/null 2>&1
# Both preconditions, polled rather than slept for: the fleet queue filters on
# pane_current_command, so the agent pane's own shell has to be up before
# --fleet runs (it would otherwise report no dirty agent pane at all), and
# deliver()'s if-shell reads @panestate back off the pane at paste time.
for i in $(seq 1 20); do
    [ "$("$TMUX_REAL" -L "$RK" display-message -p -t "$RK_AGENT" '#{pane_current_command}')" = claude ] &&
    [ "$("$TMUX_REAL" -L "$RK" show-option -pqv -t "$RK_AGENT" @panestate)" = needs-input ] && break
    sleep 0.1
done

"$TMUX_REAL" -L "$RK" send-keys -t "$RKCTL" "'$DIR/scripts/tmux-review'" Enter
rkup=0
for i in $(seq 1 20); do
    case "$("$TMUX_REAL" -L "$RK" capture-pane -p -t "$RKCTL")" in
        *"fleet>"*) rkup=1; break ;;
    esac
    sleep 0.25
done
[ "$rkup" = 1 ] || bad "the fleet queue comes up (queued-delivery case)" \
    "a visible 'fleet>' prompt" "$("$TMUX_REAL" -L "$RK" capture-pane -p -t "$RKCTL")"

"$TMUX_REAL" -L "$RK" send-keys -t "$RKCTL" Enter
for i in $(seq 1 20); do
    case "$("$TMUX_REAL" -L "$RK" capture-pane -p -t "$RKCTL")" in
        *"1 comment(s) pinned"*) break ;;
    esac
    sleep 0.25
done

rkstore=""
# The glob names comments.md itself, so head -1 is just belt-and-suspenders
# against a second session dir.
rksf=$(ls -d "$RKTMPDIR"/tmux-cockpit/review/*/comments.md 2>/dev/null | head -1)
[ -n "$rksf" ] && rkstore="$rksf"
for i in $(seq 1 20); do
    [ -n "$rkstore" ] && grep -q "keep me" "$rkstore" 2>/dev/null && break
    sleep 0.1
done
if [ -n "$rkstore" ] && grep -q "keep me" "$rkstore" 2>/dev/null; then
    ok "the comment is pinned before the delivery is attempted"
else
    bad "the comment is pinned before the delivery is attempted" \
        "'keep me' in the store under $RKTMPDIR" "$(ls -la "$RKTMPDIR" 2>&1)"
fi

# d: the loop exits 20, review_pane fires --deliver through run-shell -b, and
# deliver() queues instead of pasting. Wait for the picker's own process to go
# away first, so the run-shell chain has actually been reached.
"$TMUX_REAL" -L "$RK" send-keys -t "$RKCTL" d
# Bounded poll, not a fixed wait: unlike a real refusal, a queued delivery
# still reports success (rc=0), so review_pane()'s chain still archives the
# store -- its removal rides the same backgrounded run-shell -b as the
# delivery itself.
for i in $(seq 1 40); do [ -n "$rkstore" ] && [ -e "$rkstore" ] || break; sleep 0.1; done
if [ -n "$rkstore" ] && [ -e "$rkstore" ]; then
    bad "a queued delivery still archives the comment store" \
        "$rkstore gone" "$rkstore still exists"
else
    ok "a queued delivery still archives the comment store"
fi
# ...and sheds the snapshot beside it, exactly as a pasted delivery does:
# "<store>.rdin" served the popup, which is already gone. Polled, not slept
# for: the rm rides the same backgrounded `run-shell -b` as the delivery.
rkrowsgone=0
if [ -n "$rkstore" ]; then
    for i in $(seq 1 20); do
        [ -e "$rkstore.rdin" ] || { rkrowsgone=1; break; }
        sleep 0.1
    done
fi
if [ "$rkrowsgone" = 1 ]; then
    ok "and sheds the revdiff scratch beside it"
else
    bad "and sheds the revdiff scratch beside it" \
        "$rkstore.rdin removed" "$(ls -la "$RKTMPDIR" 2>&1)"
fi
rksess="${rkstore%/*}"
for i in $(seq 1 20); do [ -s "$rksess/delivered.md" ] && break; sleep 0.1; done
case "$(cat "$rksess/delivered.md" 2>/dev/null)" in
    *"## src.go:3 (+)"*"keep me"*) ok "and the archived batch holds the pinned comment" ;;
    *) bad "and the archived batch holds the pinned comment" \
         "## src.go:3 (+) / keep me" "$(cat "$rksess/delivered.md" 2>/dev/null)" ;;
esac
# The spool: deliver()'s own queue, the only copy of the review that survives
# --done archiving the store out from under it. review-gate drains it at
# RK_AGENT's next Stop.
rkspool="$rkstore.deliver"
case "$(cat "$rkspool" 2>/dev/null)" in
    "Code review of your changes"*"keep me"*) \
        ok "and the spool holds the composed review with the pinned comment" ;;
    *) bad "and the spool holds the composed review with the pinned comment" \
         "composed header + keep me" "$(cat "$rkspool" 2>/dev/null)" ;;
esac
is "the spool is born 0600" "600" \
   "$(stat -c '%a' "$rkspool" 2>/dev/null || stat -f '%Lp' "$rkspool")"
is "review-gate's handle is armed on the busy pane" "$rkspool" \
   "$("$TMUX_REAL" -L "$RK" show-option -pqv -t "$RK_AGENT" @cockpit_review_spool)"
case "$("$TMUX_REAL" -L "$RK" capture-pane -p -J -t "$RK_AGENT")" in
    *"keep me"*) bad "and nothing was pasted into the needs-input pane" \
         "no review text in the agent pane" \
         "$("$TMUX_REAL" -L "$RK" capture-pane -p -J -t "$RK_AGENT")" ;;
    *) ok "and nothing was pasted into the needs-input pane" ;;
esac

"$TMUX_REAL" -L "$RK" kill-server >/dev/null 2>&1
rm -rf "$RK_DIRTY" "$RKTMPDIR"
fi

echo "== tmux-review: prefix r from an agent pane opens the queue with that pane preselected, and returns to it after a review =="
if need_tmux "prefix r is the queue"; then :; else
PQ="cockpit-pq-$$"; SOCKS="$SOCKS $PQ"; PQSTATE=$(mktmpd); PQA=$(mktmpd); PQB=$(mktmpd)
for d in "$PQA" "$PQB"; do (
  cd "$d" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\n' > f
) >/dev/null 2>&1; done
"$TMUX_REAL" -L "$PQ" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$PQA" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PQ" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
"$TMUX_REAL" -L "$PQ" new-window -t t: -c "$PQB" 'exec -a claude bash' >/dev/null 2>&1
PQAP=$("$TMUX_REAL" -L "$PQ" list-panes -t t:0 -F '#{pane_id}'); PQBP=$("$TMUX_REAL" -L "$PQ" list-panes -t t:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$PQ" set-option -p -t "$PQAP" @panestate done >/dev/null 2>&1
"$TMUX_REAL" -L "$PQ" set-option -p -t "$PQBP" @panestate needs-input >/dev/null 2>&1   # ranks FIRST in the queue
"$TMUX_REAL" -L "$PQ" set-option -p -t "$PQAP" @cockpit_review pending >/dev/null 2>&1
PQB=$(mktmpd); PQW=$(mktmpd); fake_revdiff "$PQB"
PQCTL=$("$TMUX_REAL" -L "$PQ" split-window -t t:0 -P -F '#{pane_id}' 'exec bash'); until_cap "$PQ" "$PQCTL" '*$*'
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" "PATH=$(printf '%q' "$PQB"):\$PATH FAKE_RD_DIR=$(printf '%q' "$PQW") XDG_STATE_HOME=$(printf '%q' "$PQSTATE") '$DIR/scripts/tmux-review' -p $PQAP" Enter
until_cap "$PQ" "$PQCTL" '*fleet>*' && ok "prefix r from an agent pane opens the queue" || bad "prefix r from an agent pane opens the queue" "fleet>" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")"
# The pointer sits on the row of the pane the key was pressed in, not on the
# needs-input pane that sorts first.
case "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")" in
    *"▌ ◆ done"*) ok "with that pane preselected (its row shows ◆ for the pending review)" ;;
    *) bad "with that pane preselected (its row shows ◆ for the pending review)" "▌ ◆ done …" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")" ;;
esac
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" Enter
until_cap "$PQ" "$PQCTL" '*review · base*' && ok "↵ reviews it" || bad "↵ reviews it" "review · base" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")"
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" l
until_cap "$PQ" "$PQCTL" '*fleet>*' && ok "leaving the review returns to the queue" || bad "leaving the review returns to the queue" "fleet>" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")"
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" Escape
until_cap "$PQ" "$PQCTL" '*$*'
"$TMUX_REAL" -L "$PQ" kill-server >/dev/null 2>&1
fi

# group: serial
echo "== tmux-review --fleet: from a non-agent pane, the queue lists every agent pane and only those =="
if need_tmux "tmux-review --fleet"; then :; else
FQ="cockpit-fq-$$"; SOCKS="$SOCKS $FQ"
# Window 0's pane is the CONTROL pane -- a plain shell, the non-agent
# vantage point this step drives the queue from.
"$TMUX_REAL" -L "$FQ" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
CTL=$("$TMUX_REAL" -L "$FQ" list-panes -t t:0 -F '#{pane_id}')

mk_git_dir() {  # mk_git_dir <dir> <dirty:0|1>
    (
      cd "$1" || exit
      export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
      git init -q -b main .
      printf 'a\n' > f
      git add -A; git commit -q -m init
      [ "$2" = 1 ] && printf 'b\n' >> f
    ) >/dev/null 2>&1
}
FQ_DIRTY=$(mktmpd);    mk_git_dir "$FQ_DIRTY" 1      # agent, dirty -- wanted
printf 'x\n' > "$FQ_DIRTY/new1.txt"                  # + three one-line untracked files
printf 'y\n' > "$FQ_DIRTY/new2.txt"
printf 'z\n' > "$FQ_DIRTY/new3.txt"
FQ_CLEAN=$(mktmpd);    mk_git_dir "$FQ_CLEAN" 0      # agent, clean -- listed as clean (#49)
FQ_NONAGENT=$(mktmpd); mk_git_dir "$FQ_NONAGENT" 1   # dirty, but not an agent
# Same rank as FQ_DIRTY (neither has @panestate set, below) but its 5 changed
# lines are all TRACKED, none untracked -- against FQ_DIRTY's 1 tracked + 3
# untracked. The double-count bug inflated FQ_DIRTY's churn to 7 (4 real + the
# untracked FILE count charged again), sorting it above this one; the fix's
# real 4 sorts it below.
FQ_DIRTY2=$(mktmpd);   mk_git_dir "$FQ_DIRTY2" 0
printf 'b\nc\nd\ne\nf\n' >> "$FQ_DIRTY2/f"

"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_DIRTY" 'exec -a claude bash' >/dev/null 2>&1
DIRTY_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_CLEAN" 'exec -a claude bash' >/dev/null 2>&1
CLEAN_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:2 -F '#{pane_id}')
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_NONAGENT" 'exec bash' >/dev/null 2>&1
NONAGENT_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:3 -F '#{pane_id}')
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_DIRTY2" 'exec -a claude bash' >/dev/null 2>&1
DIRTY2_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:4 -F '#{pane_id}')
# The queue picks its rows by pane_current_command, so wait for the four new
# panes to report theirs: three agents and the non-agent shell.
fq_cmds() { "$TMUX_REAL" -L "$FQ" list-panes -s -t t: -F '#{pane_current_command}' | sort | tr '\n' ' '; }
until_out 'bash bash bash claude claude claude ' fq_cmds

# A review pasted into this pane and not yet sent: the row has to SAY so, or
# the queue sends you back to a pane you already reviewed.
"$TMUX_REAL" -L "$FQ" set-option -p -t "$DIRTY_PANE" @cockpit_review sent >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" send-keys -t "$CTL" "'$DIR/scripts/tmux-review' --fleet" Enter
until_cap "$FQ" "$CTL" "*$DIRTY_PANE*"
fqout=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")

case "$fqout" in
    *"$DIRTY_PANE"*) ok "the dirty agent pane is in the queue" ;;
    *) bad "the dirty agent pane is in the queue" "$DIRTY_PANE in the rows" "$fqout" ;;
esac
# #49: a clean agent is still an agent. The queue used to drop it, and with
# every agent in a hand-made worktree reading clean from its cwd, prefix r
# told six live agents "no agent pane".
case "$fqout" in
    *"$CLEAN_PANE"$'\t'*) ok "a clean agent pane is in the queue" ;;
    *) bad "a clean agent pane is in the queue" "$CLEAN_PANE in the rows" "$fqout" ;;
esac
is "and its ± column reads clean, not a dash" "clean" \
   "$(printf '%s\n' "$fqout" | awk -F'\t' -v p="$CLEAN_PANE" '$1 == p { print $5; exit }')"
case "$fqout" in
    *"$NONAGENT_PANE"*) bad "a dirty non-agent pane is not in the queue" \
         "no $NONAGENT_PANE in the rows" "$fqout" ;;
    *) ok "a dirty non-agent pane is not in the queue" ;;
esac
rowcount=$(printf '%s\n' "$fqout" | grep -c '^%' || true)
is "the queue lists exactly three rows" "3" "$rowcount"

# Pins the churn fix: 1 tracked added line (f) + 3 untracked lines (three
# one-line files), folded into the same numstat by tree_diff's intent-to-add
# copy, so the ?? walk below must count the untracked files only toward the
# "?3 new" file-count suffix, not toward churn too (that double-counted it as
# +4 -0 +3 = 7 before the fix).
case "$fqout" in
    *"$DIRTY_PANE"*"+4 -0  ?3 new"*) ok "the dirty row's stat is +4 -0  ?3 new, not double-counted" ;;
    *) bad "the dirty row's stat is +4 -0  ?3 new, not double-counted" \
         "$DIRTY_PANE ... +4 -0  ?3 new" "$fqout" ;;
esac

case "$fqout" in
    *"$DIRTY_PANE"$'\t'"↵ "*) ok "a pane holding a delivered, unsent review reads ↵ in its state column" ;;
    *) bad "a pane holding a delivered, unsent review reads ↵ in its state column" \
         "$DIRTY_PANE<tab>↵ " "$fqout" ;;
esac

# The bite: FQ_DIRTY and FQ_DIRTY2 are the SAME rank here (neither has
# @panestate set) so ordering between them falls straight to churn, real
# churn under the fix (4 vs 5, DIRTY2 wins) or double-counted churn under the
# bug (7 vs 5, DIRTY wins instead). This is the only assertion in the suite
# that can tell the two arithmetics apart -- the stat pin above and every
# other assertion in this block read the same either way, because $stat's
# TEXT was never wrong; only the hidden sort key was.
fq_first=$(printf '%s\n' "$fqout" | grep -n "^$DIRTY2_PANE\|^$DIRTY_PANE" | head -1 | cut -d: -f2 | cut -f1)
is "same-rank rows sort by real churn: +5 tracked outranks +1 tracked + 3 untracked files" \
   "$DIRTY2_PANE" "$fq_first"

# The queue is a triage list, not a churn leaderboard. A pane parked on a
# permission prompt is exactly the one that needs a human -- and it typically
# has NOTHING to diff, so churn-only ordering buried it under whoever happened
# to type the most. State ranks first; churn only breaks ties inside a rank.
"$TMUX_REAL" -L "$FQ" set-option -p -t "$CLEAN_PANE" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" set-option -p -t "$DIRTY_PANE" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" send-keys -t "$CTL" "clear; '$DIR/scripts/tmux-review' --fleet" Enter
# until_cap, not a hand-rolled 20 x 0.1 s: 2 s of re-render lost the row under
# the audit's back-to-back server teardowns and the three assertions below read
# the PREVIOUS render (#125). The cap is a hang guard only a red run pays for,
# so it belongs on the suite's shared POLLS (#117). The first render already
# shows the bare row, so wait for its new state.
until_cap "$FQ" "$CTL" "*$CLEAN_PANE$(printf '\t')needs-input*"
fqout2=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")
is "a blocked agent with a clean tree is listed first" "$CLEAN_PANE" \
   "$(printf '%s\n' "$fqout2" | grep '^%' | head -1 | cut -f1)"
is "its state is in the row" "needs-input" \
   "$(printf '%s\n' "$fqout2" | grep '^%' | head -1 | cut -f2)"
is "and a clean tree reads clean instead of +0 -0" "clean" \
   "$(printf '%s\n' "$fqout2" | grep '^%' | head -1 | cut -f5)"
is "the dirty running agent sorts below it" "$DIRTY_PANE" \
   "$(printf '%s\n' "$fqout2" | grep '^%' | sed -n 2p | cut -f1)"
case "$(printf '%s\n' "$fqout2" | grep "^$DIRTY_PANE")" in
    *"$(basename "$FQ_DIRTY")⑂main"*) ok "the row names the repo and branch" ;;
    *) bad "the row names the repo and branch" "$(basename "$FQ_DIRTY")⑂main" "$fqout2" ;;
esac

# A blocked agent whose cwd is not a git repo at all. fleet_scan_root()
# short-circuits that group with no git call -- but it emitted its unknown
# fields EMPTY, `0\t\t\t—\t0`, and tab is an IFS *whitespace* character: the
# run of three tabs collapsed into one under phase 3's `IFS=$'\t' read`, so
# the row came back shifted (repo=—, branch=0, stat empty) and rendered
# "—⑂0" with a blank ± column. The same trap the pane format guards against
# at fleet_rows' #{@panestate} sentinel. Sentinels on the wire, not empties.
FQ_NOREPO=$(mktmpd)
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_NOREPO" 'exec -a claude bash' >/dev/null 2>&1
NOREPO_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:5 -F '#{pane_id}')
"$TMUX_REAL" -L "$FQ" set-option -p -t "$NOREPO_PANE" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" send-keys -t "$CTL" "clear; '$DIR/scripts/tmux-review' --fleet" Enter
# Polled on the row we assert about, same reason and same helper as above.
until_cap "$FQ" "$CTL" "*$NOREPO_PANE$(printf '\t')*"
fqout3=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")
nrrow=$(printf '%s\n' "$fqout3" | awk -F'\t' -v p="$NOREPO_PANE" '$1 == p { print; exit }')
is "a blocked agent outside a repo reads its repo and branch as unknown" "?⑂?" \
   "$(printf '%s\n' "$nrrow" | cut -f4)"
is "and its ± column reads no repo, not an empty field" "no repo" \
   "$(printf '%s\n' "$nrrow" | cut -f5)"

# An agent that committed on its branch: `git status --porcelain` is clean
# (nothing uncommitted), but the branch itself is ahead of main -- and that
# is still a review worth queuing. Window/pane index is the next FREE one
# (t:6): t:0-t:5 are already CTL/DIRTY/CLEAN/NONAGENT/DIRTY2/NOREPO above.
FQ_AHEAD=$(mktmpd)                                   # agent, clean tree, commits ahead of main -- wanted
(
  cd "$FQ_AHEAD" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init
  git checkout -q -b feat; printf 'b\n' >> f; git commit -q -am feat
) >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_AHEAD" 'exec -a claude bash' >/dev/null 2>&1
AHEAD_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:6 -F '#{pane_id}')
"$TMUX_REAL" -L "$FQ" send-keys -t "$CTL" "clear; '$DIR/scripts/tmux-review' --fleet" Enter
until_cap "$FQ" "$CTL" "*$AHEAD_PANE$(printf '\t')*"
fqout4=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")
case "$fqout4" in
    *"$AHEAD_PANE"*"+1 -0"*) ok "an agent that committed on its branch is still in the queue, with its branch churn" ;;
    *) bad "an agent that committed on its branch is still in the queue, with its branch churn" "$AHEAD_PANE with +1 -0" "$fqout4" ;;
esac

# --delta answers the Stop hook's one question: does this pane's tree still
# differ from its review base? Run through `run-shell` on THIS block's own
# server -- delta_mode's internal `tmux` calls need to land on $FQ, not
# whatever server a bare tmux-review invocation from the suite's own shell
# would resolve against.
is "--delta: a branch ahead of main has something to review" "D=0" \
   "$("$TMUX_REAL" -L "$FQ" run-shell "'$DIR/scripts/tmux-review' --delta $AHEAD_PANE; echo D=\$?" 2>/dev/null | tail -1)"
is "--delta: a clean agent on an unadvanced branch has nothing to review" "D=1" \
   "$("$TMUX_REAL" -L "$FQ" run-shell "'$DIR/scripts/tmux-review' --delta $CLEAN_PANE; echo D=\$?" 2>/dev/null | tail -1)"

# A tree whose ONLY change is a file over the cap. tree_diff keeps it away
# from git, so its --numstat would come back empty and the Stop hook would
# call the pane clean -- the one thing the skip must not do. git itself
# printed "-\t-\tcore" here (its binary row) before the skip existed.
FQ_BIG=$(mktmpd)
(
  cd "$FQ_BIG" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init
  head -c $(( 64 * 1024 * 1024 )) /dev/zero > core   # untracked, over the cap, and nothing else
) >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_BIG" 'exec -a claude bash' >/dev/null 2>&1
BIG_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:7 -F '#{pane_id}')
is "--delta: a tree whose only change is a skipped big file is still dirty" "D=0" \
   "$("$TMUX_REAL" -L "$FQ" run-shell "'$DIR/scripts/tmux-review' --delta $BIG_PANE; echo D=\$?" 2>/dev/null | tail -1)"

# #49: an agent that made its worktree by hand (`git worktree add` from the
# main checkout) keeps its process in the main checkout and drives the tree
# with git -C, so its cwd reads clean master. @cockpit_dir, a pane option the
# agent sets, names the tree it WORKS in, and every reader of a pane's tree
# prefers it. t:8 is the next free window.
FQ_MAIN=$(mktmpd); mk_git_dir "$FQ_MAIN" 0
FQ_WT="$FQ_MAIN-wt"
git -C "$FQ_MAIN" worktree add -q -b feat "$FQ_WT" >/dev/null 2>&1
printf 'b\n' >> "$FQ_WT/f"
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_MAIN" 'exec -a claude bash' >/dev/null 2>&1
WT_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:8 -F '#{pane_id}')
"$TMUX_REAL" -L "$FQ" set-option -p -t "$WT_PANE" @cockpit_dir "$FQ_WT" >/dev/null 2>&1
# A dangling @cockpit_dir (the worktree was removed since) falls back to the cwd.
"$TMUX_REAL" -L "$FQ" set-option -p -t "$CLEAN_PANE" @cockpit_dir "$FQ_CLEAN/gone" >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" send-keys -t "$CTL" "clear; '$DIR/scripts/tmux-review' --fleet" Enter
until_cap "$FQ" "$CTL" "*$WT_PANE$(printf '\t')*"
fqout5=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")
wtrow=$(printf '%s\n' "$fqout5" | awk -F'\t' -v p="$WT_PANE" '$1 == p { print; exit }')
is "@cockpit_dir: the row names the worktree and its branch, not the cwd's" "$(basename "$FQ_WT")⑂feat" \
   "$(printf '%s\n' "$wtrow" | cut -f4)"
is "and its churn is the worktree's" "+1 -0" "$(printf '%s\n' "$wtrow" | cut -f5)"
clrow=$(printf '%s\n' "$fqout5" | awk -F'\t' -v p="$CLEAN_PANE" '$1 == p { print; exit }')
is "a dangling @cockpit_dir falls back to the pane's own tree" "$(basename "$FQ_CLEAN")⑂main" \
   "$(printf '%s\n' "$clrow" | cut -f4)"
is "--delta: the Stop hook sees the worktree's change through @cockpit_dir" "D=0" \
   "$("$TMUX_REAL" -L "$FQ" run-shell "'$DIR/scripts/tmux-review' --delta $WT_PANE; echo D=\$?" 2>/dev/null | tail -1)"
is "--dir: the tree the pane works in" "$FQ_WT" \
   "$("$TMUX_REAL" -L "$FQ" run-shell "'$DIR/scripts/tmux-review' --dir $WT_PANE" 2>/dev/null | tail -1)"
# The store a review of the pane uses: tmux-review's own functions, lifted out
# rather than respelled, applied the way review_pane and --info apply them.
FQSD=$(mktmpf)
{ cat "$DIR/scripts/lib/pane.sh"
  sed -n '/^git_root() {/,/^}/p; /^pane_dir() {/,/^}/p; /^session_dir() {/,/^}/p' "$DIR/scripts/tmux-review"
  printf '%s\n' 'session_dir "$1" "$(git_root "$(pane_dir "$1")")"'; } > "$FQSD"
wtsess=$("$TMUX_REAL" -L "$FQ" run-shell "bash '$FQSD' $WT_PANE" 2>/dev/null | tail -1)
"$TMUX_REAL" -L "$FQ" set-option -pu -t "$WT_PANE" @cockpit_dir >/dev/null 2>&1
cwdsess=$("$TMUX_REAL" -L "$FQ" run-shell "bash '$FQSD' $WT_PANE" 2>/dev/null | tail -1)
is "the worktree review keeps its own store, not the main checkout's" "1" \
   "$(case "$wtsess" in */tmux-cockpit/review/"${WT_PANE#%}"-[0-9]*) [ "$wtsess" != "$cwdsess" ] && echo 1 ;; esac || echo "$wtsess = $cwdsess")"

"$TMUX_REAL" -L "$FQ" kill-server >/dev/null 2>&1
rm -rf "$FQ_DIRTY" "$FQ_CLEAN" "$FQ_NONAGENT" "$FQ_DIRTY2" "$FQ_NOREPO" "$FQ_AHEAD" "$FQ_BIG" "$FQ_WT" "$FQ_MAIN"
fi

echo "== tmux-review --fleet: ranks needs-input, error, running, done, then no state =="
if need_tmux "tmux-review --fleet rank"; then :; else
FO="cockpit-fo-$$"; SOCKS="$SOCKS $FO"; FOD=$(mktmpd)
"$TMUX_REAL" -L "$FO" -f /dev/null new-session -d -s t -c "$FOD" 'exec -a claude sleep 300' >/dev/null 2>&1
for _ in 1 2 3 4; do "$TMUX_REAL" -L "$FO" new-window -t t: -c "$FOD" 'exec -a claude sleep 300' >/dev/null 2>&1; done
"$TMUX_REAL" -L "$FO" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
# shellcheck disable=SC2317,SC2329  # called by until_out
fo_cmds() { "$TMUX_REAL" -L "$FO" list-panes -a -F '#{pane_current_command}' | tr '\n' ' '; }
until_out 'claude claude claude claude claude ' fo_cmds
# Created in an order that matches no ranking, so only the sort can produce the expected one.
mapfile -t fo_p < <("$TMUX_REAL" -L "$FO" list-panes -a -F '#{pane_id}')
fo_i=0
for s in done running '' error needs-input; do
    [ -z "$s" ] || "$TMUX_REAL" -L "$FO" set-option -p -t "${fo_p[fo_i]}" @panestate "$s" >/dev/null 2>&1
    fo_i=$((fo_i + 1))
done
is "the queue's state column reads in the chosen order" "needs-input error running done —" \
   "$("$TMUX_REAL" -L "$FO" run-shell "'$DIR/scripts/tmux-review' --fleet" 2>/dev/null | cut -f2 | tr '\n' ' ' | sed 's/ $//')"
"$TMUX_REAL" -L "$FO" kill-server >/dev/null 2>&1
rm -rf "$FOD"; unset fo_i fo_p; unset -f fo_cmds
fi

echo "== tmux-review --fleet: gathers per-pane git state in parallel =="
# fleet_rows() used to run each pane's git status+numstat SERIALLY, one pane
# after the next -- ~2 git calls/pane. On a cold filesystem (150ms/git,
# measured) 100 panes blanked the popup for ~32s, on every prefix+r from a
# non-agent pane. A fixed-latency git shim stands in for that cold filesystem
# here: real git still runs behind it, so this measures the serial-vs-parallel
# SHAPE, not a fabricated number: the pre-fix script ran them one at a time.
if need_tmux "tmux-review --fleet parallel git"; then :; else
FP="cockpit-fp-$$"; SOCKS="$SOCKS $FP"
FP_DIR=$(mktmpd)
(
  cd "$FP_DIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\n' > f
  git add -A; git commit -q -m init
  printf 'b\n' >> f   # dirty -- every pane below shares this one working tree
) >/dev/null 2>&1
"$TMUX_REAL" -L "$FP" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$FP" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
i=1
while [ "$i" -le 50 ]; do
    # sleep, not bash: 50 shells sourcing rc files are the load spike, and the
    # last one had not exec'd when the scan ran, so it read 49 rows (issue #47).
    "$TMUX_REAL" -L "$FP" new-window -t t: -c "$FP_DIR" 'exec -a claude sleep 3600' >/dev/null 2>&1
    i=$((i + 1))
done
for i in $(seq 1 "$POLLS"); do
    [ "$("$TMUX_REAL" -L "$FP" list-panes -a -F '#{pane_current_command}' | grep -c '^claude$')" -eq 50 ] && break
    sleep 0.1
done
FP_SOCKPATH=$("$TMUX_REAL" -L "$FP" display-message -p '#{socket_path}')

# Graded as a COUNT, not a clock (#117): each shimmed git call records how
# many siblings are live with it. Serial dispatch never sees more than 1; the
# capped parallel path sees up to 8 and never more.
FP_SLOWGIT=$(mktmpd); FP_LIVE=$(mktmpd); FP_LIVEN=$(mktmpf)
FP_REALGIT=$(command -v git)
cat > "$FP_SLOWGIT/git" <<SHIMEOF
#!/bin/sh
: > "$FP_LIVE/\$\$"
ls "$FP_LIVE" | wc -l >> "$FP_LIVEN"
sleep 0.1
"$FP_REALGIT" "\$@"; rc=\$?
rm -f "$FP_LIVE/\$\$"
exit \$rc
SHIMEOF
chmod +x "$FP_SLOWGIT/git"

timeout 15 env TMUX="$FP_SOCKPATH,0,0" PATH="$FP_SLOWGIT:$PATH" \
    "$DIR/scripts/tmux-review" --fleet > "$STATE/fp-out" 2>&1
fp_rc=$?
fp_peak=$(sort -n "$FP_LIVEN" 2>/dev/null | tail -1)

if [ "$fp_rc" -ne 0 ]; then
    bad "the fleet queue gathers 50 panes' git state" "exit 0" "exit $fp_rc"
elif [ "${fp_peak:-0}" -ge 2 ] && [ "$fp_peak" -le 8 ]; then
    ok "the fleet queue gathers 50 panes' git state in parallel, capped at 8 (peak $fp_peak in flight)"
else
    bad "the fleet queue gathers 50 panes' git state in parallel, capped at 8" \
        "2..8 git calls in flight at the peak" "${fp_peak:-0}"
fi
rm -rf "$FP_LIVE" "$FP_LIVEN"
fp_rows=$(grep -c . "$STATE/fp-out" || true)
is "all 50 dirty panes are still in the rows" "50" "$fp_rows"

"$TMUX_REAL" -L "$FP" kill-server >/dev/null 2>&1
rm -rf "$FP_DIR"
fi

# group: serial
echo "== tmux-review --fleet: dedups panes sharing a repo root across 20 panes =="
# fleet_rows() used to cost one status+numstat scan PER PANE even when two
# panes were cd'd into the exact same worktree -- resolving the repo root now
# happens first (fleet_rows:~1180), panes are grouped on it, and
# fleet_scan_root() runs once per UNIQUE root, not once per pane. 20 windows,
# 19 fixture repos -- pane 20 reuses pane 1's directory -- so 20 rows still
# come out, but only 19 `git status` calls should ever run. `timeout 5` is a
# hang guard, not a perf gate: the scan is 0.3-0.7 s on a quiet box, but a
# peer suite or an ssh sync pushed it past the old `timeout 1`, and a killed
# scan left fd-out empty, so the row count read 0 too (issue #18).
if need_tmux "tmux-review --fleet dedup across 20 panes"; then :; else
FD="cockpit-fd-$$"; SOCKS="$SOCKS $FD"
"$TMUX_REAL" -L "$FD" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$FD" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
CTL=$("$TMUX_REAL" -L "$FD" list-panes -t t:0 -F '#{pane_id}')

# mk_git_dir is the fleet queue block's own fixture helper (tests/run.sh
# above, "gathers exactly the dirty agent panes"), reused here as-is.
declare -a FD_DIRS=()
i=1
while [ "$i" -le 19 ]; do
    d=$(mktmpd); mk_git_dir "$d" 1
    FD_DIRS+=("$d")
    i=$((i + 1))
done

i=1
while [ "$i" -le 20 ]; do
    if [ "$i" -eq 20 ]; then
        wd="${FD_DIRS[0]}"           # the dup: shares pane 1's repo root
    else
        wd="${FD_DIRS[$((i - 1))]}"
    fi
    # sleep, not bash: 20 interactive shells sourcing ~/.bashrc (nvm alone is
    # 0.2 s here) ran beside the timed scan and tripled it (0.19 s -> 0.65 s,
    # measured), so the 1 s gate flaked on any box whose rc files cost something.
    "$TMUX_REAL" -L "$FD" new-window -t t: -c "$wd" 'exec -a claude sleep 3600' >/dev/null 2>&1
    i=$((i + 1))
done

# Counts `git status` invocations specifically -- a PATH shim that logs argv
# and still execs the real binary, the same trick the round-trip-count block
# uses (tests/run.sh:389-405) and the fixed-latency shim above stands on.
FD_SHIM=$(mktmpd)
FD_REALGIT=$(command -v git)
FD_LOG="$STATE/fd-git-calls"
: > "$FD_LOG"
cat > "$FD_SHIM/git" <<SHIMEOF
#!/bin/sh
printf '%s\n' "\$*" >> "$FD_LOG"
exec "$FD_REALGIT" "\$@"
SHIMEOF
chmod +x "$FD_SHIM/git"

"$TMUX_REAL" -L "$FD" send-keys -t "$CTL" \
    "PATH='$FD_SHIM:'\"\$PATH\" timeout 5 '$DIR/scripts/tmux-review' --fleet > $STATE/fd-out; echo RC=\$?" Enter
fd_out=""
# 80 x 0.1 s outlasts the timeout 5 above on purpose: a killed scan must
# still print RC=124 here rather than leave fd_out empty.
for i in $(seq 1 80); do
    fd_out=$("$TMUX_REAL" -L "$FD" capture-pane -p -J -t "$CTL")
    # The typed-but-not-yet-run command line already contains the literal
    # text "echo RC=$?" the instant send-keys lands it -- a bare *RC=* match
    # breaks on that unevaluated line before the command has even started.
    # Only a REAL exit code prints digits after the "=", so require one.
    case "$fd_out" in *RC=[0-9]*) break ;; esac
    sleep 0.1
done

is "the 20-pane scan exits 0" "RC=0" \
   "$(printf '%s\n' "$fd_out" | grep -o 'RC=[0-9-]*' | tail -1)"

fd_rows=$(grep -c . "$STATE/fd-out" || true)
is "20 rows for 20 panes even though two share one repo" "20" "$fd_rows"

fd_diff_calls=$(grep -c ' diff ' "$FD_LOG" || true)
is "the shared repo is scanned once -- 19 diffs for 19 unique roots" \
    "19" "$fd_diff_calls"
# The budget above is forks: 19 roots in ~0.5 s is ~25 ms each and one git is
# 3-5 ms. Phase 3's per-review base took a dirty root from 4 forks to 13 and
# blew it. Pinned as a count -- one rev-parse per pane to find its root, five
# per unique root (rev-parse, add -N, for-each-ref, diff, and big_files'
# ls-files, which is what keeps a 1 GB file from costing that root 26 s) -- so
# the next helper that costs a fork per root fails here on any box, not only a
# busy one. The 50-pane block above keeps the wall-clock bar (1818 ms against
# 5000 ms); this one stopped timing itself, see the top of the block.
fd_git_calls=$(grep -c . "$FD_LOG" || true)
# The sixth is lib/git.sh reading the repo's filter drivers once.
is "one fork per pane and six per dirty root, no more" "1" \
   "$([ "$fd_git_calls" -le $((20 + 19 * 6)) ] && echo 1 || echo "$fd_git_calls")"

"$TMUX_REAL" -L "$FD" kill-server >/dev/null 2>&1
for d in "${FD_DIRS[@]}"; do rm -rf "$d"; done
fi

echo "== tmux-review: the popup resolves the pane it was opened from, not the busiest client =="
# Same hazard as tmux-branch's popup resolution (tmux-branch:34-53, tested
# above): inside a popup $TMUX_PANE is the popup's own pseudo-pane, so bare
# `display-message -p` (no -t) falls back to whichever attached client was
# most recently active -- reproduced here with two real attached clients,
# cliB attaching after cliA so cliB is the busier one. Only cliA is an agent
# pane, and prefix r is the queue either way now, so the observable difference
# is the PRESELECT: resolved to cliA the queue opens with that pane under the
# pointer (--sync --bind start:pos(N)), resolved to cliB's plain shell there is
# no such pane in the queue and no bind at all. A stub fzf on PATH records the
# arguments it was invoked with and exits 1 (a harmless no-match/cancel) rather
# than actually opening anything, so this needs no real interactive picker.
if need_tmux "review popup pane resolution"; then :; else
RDA=$(mktmpd); RDB=$(mktmpd)
(
    cd "$RDA" || exit
    export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
    git init -q -b main .
    printf 'a\n' > f
    git add -A; git commit -q -m init
    printf 'b\n' >> f   # dirty -- so review_pane's loop reaches the fzf call
) >/dev/null 2>&1
RINNER="cockpit-review-inner-$$"; ROUTERA="cockpit-review-outera-$$"; ROUTERB="cockpit-review-outerb-$$"
SOCKS="$SOCKS $RINNER $ROUTERA $ROUTERB"
tmux -L "$RINNER" -f /dev/null new-session -d -s cliA -c "$RDA" 'exec -a claude bash' >/dev/null 2>&1
tmux -L "$RINNER" new-session -d -s cliB -c "$RDB" 'exec bash' >/dev/null 2>&1
tmux -L "$RINNER" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
rv_clients() { "$TMUX_REAL" -L "$RINNER" list-clients 2>/dev/null | grep -c .; }
# Counted, not slept for: the hazard needs cliB to be the more recent client,
# so the second attach only starts once the first has landed.
tmux -L "$ROUTERA" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $RINNER attach -t cliA" >/dev/null 2>&1
until_out 1 rv_clients
tmux -L "$ROUTERB" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $RINNER attach -t cliB" >/dev/null 2>&1
until_out 2 rv_clients
rpaneA=$(tmux -L "$RINNER" list-panes -t cliA -F '#{pane_id}')
RSOCKPATH=$(tmux -L "$RINNER" display-message -p '#{socket_path}')

RFZFSTUB=$(mktmpd)
RMARKER="$STATE/review-fzf-marker"
: > "$RMARKER"
cat > "$RFZFSTUB/fzf" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >> "$RMARKER"
exit 1
EOF
chmod +x "$RFZFSTUB/fzf"

# Without -p: unpinned resolution lands on cliB (the busier client, attached
# second) -- a plain shell, which is in nobody's queue, so the list opens with
# no preselect at all. The hazard is real.
: > "$RMARKER"
env PATH="$RFZFSTUB:$PATH" TMUX="$RSOCKPATH,0,0" "$DIR/scripts/tmux-review" </dev/null >/dev/null 2>&1
case "$(cat "$RMARKER")" in
    *'fleet>'*'start:pos('*|*'start:pos('*'fleet>'*)
        bad "unpinned resolution really does land on the busier client (the hazard is real)" \
         "fleet> with no start:pos( in the fzf invocation" "$(cat "$RMARKER")" ;;
    *'fleet>'*) ok "unpinned resolution really does land on the busier client (the hazard is real)" ;;
    *) bad "unpinned resolution really does land on the busier client (the hazard is real)" \
         "fleet> in the fzf invocation" "$(cat "$RMARKER")" ;;
esac
# With -p pane_id: pinned to the pane the popup actually opened from -- the
# agent pane, so the queue opens with that pane's row under the pointer.
: > "$RMARKER"
env PATH="$RFZFSTUB:$PATH" TMUX="$RSOCKPATH,0,0" "$DIR/scripts/tmux-review" -p "$rpaneA" </dev/null >/dev/null 2>&1
case "$(cat "$RMARKER")" in
    *'start:pos('*) ok "-p pane_id resolves the pane the popup was opened from" ;;
    *) bad "-p pane_id resolves the pane the popup was opened from" \
         "start:pos( in the fzf invocation" "$(cat "$RMARKER")" ;;
esac
# #49: ↵ on a clean row jumps to the pane instead of opening a review that
# closes itself with "no uncommitted changes" and lands back on the list
# (section 8 #3). A second window in cliA's session holds a clean agent; the
# stub fzf picks its row; the session's current window must move there and
# tmux-review must exit rather than re-list forever.
RDC=$(mktmpd)
(
    cd "$RDC" || exit
    export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
    git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init
) >/dev/null 2>&1
tmux -L "$RINNER" new-window -d -t cliA: -c "$RDC" 'exec -a claude bash' >/dev/null 2>&1
rpaneC=$(tmux -L "$RINNER" list-panes -t cliA:1 -F '#{pane_id}')
for i in $(seq 1 20); do   # until bash has exec'd as claude, or the row is not built
    [ "$(tmux -L "$RINNER" display-message -p -t "$rpaneC" '#{pane_current_command}' 2>/dev/null)" = claude ] && break
    sleep 0.1
done
cat > "$RFZFSTUB/fzf" <<EOF
#!/usr/bin/env bash
row=\$(grep "^$rpaneC"\$'\\t' || true)
[ -n "\$row" ] || exit 1
printf '\\n%s\\n' "\$row"
EOF
jrc=$(timeout 5 env PATH="$RFZFSTUB:$PATH" TMUX="$RSOCKPATH,0,0" "$DIR/scripts/tmux-review" -p "$rpaneA" </dev/null >/dev/null 2>&1; echo $?)
is "↵ on a clean row leaves the queue instead of re-listing" "0" "$jrc"
is "and the session's current window is now that agent's" "1" \
   "$(tmux -L "$RINNER" display-message -p -t cliA: '#{window_index}' 2>/dev/null)"
# A bare switch-client inside the popup lands on the most recently active
# client -- cliB's, attached second -- and yanks THAT person to cliA.
case "$(tmux -L "$RINNER" list-clients -F '#{client_session}' 2>/dev/null)" in
    *cliB*) ok "and the other client is still on its own session" ;;
    *) bad "and the other client is still on its own session" "a client on cliB" \
         "$(tmux -L "$RINNER" list-clients -F '#{client_name} #{client_session}' 2>/dev/null)" ;;
esac
rm -rf "$RDC"
tmux -L "$ROUTERA" kill-server >/dev/null 2>&1
tmux -L "$ROUTERB" kill-server >/dev/null 2>&1
tmux -L "$RINNER" kill-server >/dev/null 2>&1
rm -rf "$RDA" "$RDB" "$RFZFSTUB"
fi

echo "== prefix r reviews, prefix R reloads; off unbinds each =="
if need_tmux "cockpit_review_key"; then :; else
RB="cockpit-rb-$$"; SOCKS="$SOCKS $RB"
"$TMUX_REAL" -L "$RB" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$RB" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$RB" list-keys -T prefix 2>/dev/null | awk '$4=="r"')" in
    *tmux-review*) ok "prefix r is bound to the review popup by default" ;;
    *) bad "prefix r is bound to the review popup by default" \
         "a display-popup running tmux-review" \
         "$("$TMUX_REAL" -L "$RB" list-keys -T prefix | awk '$4=="r"')" ;;
esac
case "$("$TMUX_REAL" -L "$RB" list-keys -T prefix 2>/dev/null | awk '$4=="R"')" in
    *source-file*) ok "prefix R reloads the config by default" ;;
    *) bad "prefix R reloads the config by default" "a source-file binding" \
         "$("$TMUX_REAL" -L "$RB" list-keys -T prefix | awk '$4=="R"')" ;;
esac
# The reload must source what THIS server loaded (-f /dev/null here), not a
# hardcoded ~/.tmux.conf.
case "$("$TMUX_REAL" -L "$RB" list-keys -T prefix 2>/dev/null | awk '$4=="R"')" in
    */dev/null*) ok "and it sources the file tmux actually loaded" ;;
    *) bad "and it sources the file tmux actually loaded" "/dev/null in the binding" \
         "$("$TMUX_REAL" -L "$RB" list-keys -T prefix | awk '$4=="R"')" ;;
esac
"$TMUX_REAL" -L "$RB" kill-server >/dev/null 2>&1

RB2="cockpit-rb2-$$"; SOCKS="$SOCKS $RB2"
"$TMUX_REAL" -L "$RB2" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$RB2" set -g @cockpit_review_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$RB2" set -g @cockpit_reload_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$RB2" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
# "off" cannot make r EMPTY: stock tmux binds prefix r to refresh-client, and
# not binding ours simply leaves the stock binding standing. Unbound-from-review
# is the real contract.
case "$("$TMUX_REAL" -L "$RB2" list-keys -T prefix 2>/dev/null | awk '$4=="r"')" in
    *tmux-review*) bad "@cockpit_review_key off leaves r to stock tmux" \
         "no tmux-review in the binding" \
         "$("$TMUX_REAL" -L "$RB2" list-keys -T prefix | awk '$4=="r"')" ;;
    *) ok "@cockpit_review_key off leaves r to stock tmux" ;;
esac
is "@cockpit_reload_key off leaves R unbound" "" \
   "$("$TMUX_REAL" -L "$RB2" list-keys -T prefix 2>/dev/null | awk '$4=="R"')"
"$TMUX_REAL" -L "$RB2" kill-server >/dev/null 2>&1

# @cockpit_review_window on: the same script and the same -p, opened as a real
# window instead of the modal popup, so the fleet's borders and tabs stay
# visible while you read.
RB3="cockpit-rb3-$$"; SOCKS="$SOCKS $RB3"; RB3D=$(mktmpd)
"$TMUX_REAL" -L "$RB3" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$RB3" set -g @cockpit_review_window on >/dev/null 2>&1
"$TMUX_REAL" -L "$RB3" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$RB3" list-keys -T prefix 2>/dev/null | awk '$4=="r"')" in
    *new-window*tmux-review*) ok "@cockpit_review_window on opens a window instead of the popup" ;;
    *) bad "@cockpit_review_window on opens a window instead of the popup" \
         "a new-window running tmux-review" \
         "$("$TMUX_REAL" -L "$RB3" list-keys -T prefix | awk '$4=="r"')" ;;
esac
# new-window activates the review window before its command runs, so a pane
# resolved from inside it is the review window's own; the bind pins the
# source pane into @cockpit_review_src first and the window reads that (#45).
# A copy of the plugin whose tmux-review is a stub records the -p it got.
RB3O="cockpit-rb3o-$$"; SOCKS="$SOCKS $RB3O"
mkdir -p "$RB3D/p/scripts"; cp "$DIR/cockpit.tmux" "$RB3D/p/"; cp "$DIR"/scripts/* "$RB3D/p/scripts/" 2>/dev/null
printf '#!/bin/sh\nprintf %%s "$*" > %s/args; sleep 30\n' "$RB3D" > "$RB3D/p/scripts/tmux-review"; chmod +x "$RB3D/p/scripts/tmux-review"
"$TMUX_REAL" -L "$RB3" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$RB3" run-shell "$RB3D/p/cockpit.tmux" >/dev/null 2>&1
RB3S=$("$TMUX_REAL" -L "$RB3" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$RB3O" -f /dev/null new-session -d -s o -x 100 -y 30 "TERM=xterm-256color exec $TMUX_REAL -L $RB3 attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$RB3" list-clients -F '#{client_name}'
"$TMUX_REAL" -L "$RB3O" send-keys -t o C-b r
until_file "$RB3D/args" '*-p *'
is "the review window is told the pane the key was pressed in, not its own" "-p $RB3S" "$(cat "$RB3D/args" 2>/dev/null)"
"$TMUX_REAL" -L "$RB3O" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$RB3" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --info / prefix i: one screen says what the pane is, waits on, and holds =="
if need_tmux "tmux-review --info"; then :; else
IN="cockpit-in-$$"; SOCKS="$SOCKS $IN"; INSTATE=$(mktmpd); INDIR=$(mktmpd)
( cd "$INDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\n' > f ) >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$INDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
INAP=$("$TMUX_REAL" -L "$IN" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @panestate_msg 'run make?' >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @cockpit_review pending >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @cockpit_session sid-9 >/dev/null 2>&1
"$TMUX_REAL" -L "$IN" set-option -p -t "$INAP" @cockpit_title 'Fix the parser' >/dev/null 2>&1
INSESS="$INSTATE/tmux-cockpit/review/${INAP#%}-$(printf '%s' "$INDIR" | cksum | cut -d' ' -f1)"
mkdir -p "$INSESS"; printf '## f:2 (+)\n> b\nx\n\n' > "$INSESS/comments.md"; printf '## f:1 (+)\n> a\ny\n\n## f:1 (+)\n> a\nz\n\n' > "$INSESS/delivered.md"
inout=$("$TMUX_REAL" -L "$IN" run-shell "XDG_STATE_HOME=$(printf '%q' "$INSTATE") '$DIR/scripts/tmux-review' -p $INAP --info" 2>&1)
for want in "pane      $INAP" "cwd       $INDIR" "root      $INDIR" "state     needs-input  · run make?" "review    pending" "base      uncommitted" "session   sid-9" "title     Fix the parser" "1 pinned · 2 delivered"; do
    case "$inout" in *"$want"*) ok "info shows: $want" ;; *) bad "info shows: $want" "$want" "$inout" ;; esac
done
case "$inout" in *'hold      '*) bad "info has no hold row" "none" "$inout" ;; *) ok "info has no hold row" ;; esac
# The root is what the review diffs, and it is not the cwd: a pane parked in
# a subdirectory (or, issue #3, a session that edits another tree by absolute
# path) has to see the tree the popup will actually show.
mkdir -p "$INDIR/sub"
"$TMUX_REAL" -L "$IN" new-window -d -t t: -c "$INDIR/sub" 'exec -a claude bash' >/dev/null 2>&1
INSUB=$("$TMUX_REAL" -L "$IN" list-panes -t t:1 -F '#{pane_id}')
insub=$("$TMUX_REAL" -L "$IN" run-shell "XDG_STATE_HOME=$(printf '%q' "$INSTATE") '$DIR/scripts/tmux-review' -p $INSUB --info" 2>&1)
for want in "cwd       $INDIR/sub" "root      $INDIR"; do
    case "$insub" in *"$want"*) ok "info from a subdirectory shows: $want" ;; *) bad "info from a subdirectory shows: $want" "$want" "$insub" ;; esac
done
# display-message exits 0 on a pane id that does not exist, expanding every
# format to "" -- so a screen built on the exit code described the CALLER's own
# repo (git_root "") as if it were that pane's.
inbad=$("$TMUX_REAL" -L "$IN" run-shell "'$DIR/scripts/tmux-review' -p %999 --info 2>&1" 2>&1)
case "$inbad" in
    *"no such pane %999"*) ok "a pane that does not exist is named, not described" ;;
    *) bad "a pane that does not exist is named, not described" "no such pane %999" "$inbad" ;;
esac
"$TMUX_REAL" -L "$IN" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$IN" list-keys -T prefix 2>/dev/null | awk '$4=="i"')" in
    *display-popup*"cockpit-tui'"*" info "*) ok "prefix i is bound to the info popup" ;;
    *) bad "prefix i is bound to the info popup" "a display-popup running cockpit-tui info" "$("$TMUX_REAL" -L "$IN" list-keys -T prefix | awk '$4=="i"')" ;;
esac
"$TMUX_REAL" -L "$IN" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --info: doing, said and asked rows and the /btw pointer =="
if need_tmux "tmux-review --info doing"; then :; else
ID="cockpit-id-$$"; SOCKS="$SOCKS $ID"; IDSTATE=$(mktmpd); IDDIR=$(mktmpd); IDTREE=$(mktmpd)
"$TMUX_REAL" -L "$ID" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$IDDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$ID" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
IDAP=$("$TMUX_REAL" -L "$ID" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$ID" new-window -d -t t: -c "$IDDIR" 'exec -a vim bash' >/dev/null 2>&1
IDNP=$("$TMUX_REAL" -L "$ID" list-panes -t t:1 -F '#{pane_id}')
# A copy of the tree whose transcript.sh is the fixture: the real reader is another lane's.
id_screen() {                 # <fixture or ""> <pane>  -> stdout screen, stderr after a marker line
    rm -rf "$IDTREE/scripts"; cp -r "$DIR/scripts" "$IDTREE/scripts"
    [ -z "$1" ] || cp "$DIR/tests/fixtures/info-doing/$1" "$IDTREE/scripts/lib/transcript.sh"
    "$TMUX_REAL" -L "$ID" run-shell "XDG_STATE_HOME=$(printf '%q' "$IDSTATE") '$IDTREE/scripts/tmux-review' -p $2 --info 2>&1" 2>&1
}
idout=$(id_screen transcript.sh "$IDAP")
for want in "doing     Bash: run the suite (since 4m12s)" "said      all green so far" "asked     fix the parser (12s)"; do
    case "$idout" in *"$want"*) ok "info shows: $want" ;; *) bad "info shows: $want" "$want" "$idout" ;; esac
done
case "$idout" in
    *"ask it    /btw <question> in the pane (answers while it works)"*) ok "info points an agent pane at /btw" ;;
    *) bad "info points an agent pane at /btw" "ask it    /btw ..." "$idout" ;;
esac
idrows=$(grep -c '^\(doing\|said\|asked\) ' <<<"$idout")
[ "$idrows" = 3 ] && ok "info shows exactly the three transcript rows" || bad "info shows exactly the three transcript rows" 3 "$idrows"
case "$idout" in *"state     "*"doing     "*"review    "*) ok "the rows sit under the state row" ;; *) bad "the rows sit under the state row" "state, doing, review in order" "$idout" ;; esac
# stdout and stderr of the screen into separate files (outside the tree under test).
rm -rf "$IDTREE/scripts"; cp -r "$DIR/scripts" "$IDTREE/scripts"
cp "$DIR/tests/fixtures/info-doing/transcript-noisy.sh" "$IDTREE/scripts/lib/transcript.sh"
"$TMUX_REAL" -L "$ID" run-shell "XDG_STATE_HOME=$(printf '%q' "$IDSTATE") '$IDTREE/scripts/tmux-review' -p $IDAP --info >'$IDSTATE/out' 2>'$IDSTATE/err'" >/dev/null 2>&1
[ ! -s "$IDSTATE/err" ] && ok "a reader that writes to stderr leaves the screen's stderr empty" || bad "a reader that writes to stderr leaves the screen's stderr empty" "empty" "$(cat "$IDSTATE/err")"
case "$(cat "$IDSTATE/out")" in *noise*) bad "reader stderr is not on the screen" "no noise" "$(cat "$IDSTATE/out")" ;; *) ok "reader stderr is not on the screen" ;; esac
case "$(cat "$IDSTATE/out")" in *$'\033'*) bad "an ESC in a transcript field never reaches the screen" "no ESC byte" "$(cat -v "$IDSTATE/out")" ;; *) ok "an ESC in a transcript field never reaches the screen" ;; esac
grep -q 'said      red \[31mtext\[0m end' "$IDSTATE/out" && ok "the text around the ESC survives" || bad "the text around the ESC survives" "said      red [31mtext[0m end" "$(cat -v "$IDSTATE/out")"
idp=$(id_screen transcript-partial.sh "$IDAP")
case "$idp" in
    *"doing     Read: a file"*) ok "a doing row with no age has no since" ;;
    *) bad "a doing row with no age has no since" "doing     Read: a file" "$idp" ;;
esac
case "$idp" in *"since"*|*said*|*asked*) bad "a row the reader left out is not shown" "no since/said/asked" "$idp" ;; *) ok "a row the reader left out is not shown" ;; esac
# The stub (status 1): none of the rows, nothing on stderr, the rest of the screen intact.
idstub=$(id_screen "" "$IDAP")
idrows=$(grep -c '^\(doing\|said\|asked\) ' <<<"$idstub")
[ "$idrows" = 0 ] && ok "a reader that returns 1 shows no transcript rows" || bad "a reader that returns 1 shows no transcript rows" 0 "$idrows"
case "$idstub" in *"tmux-review:"*|*"command not found"*) bad "a reader that returns 1 prints no error" "no error" "$idstub" ;; *) ok "a reader that returns 1 prints no error" ;; esac
case "$idstub" in *"pane      $IDAP"*"state     "*"review    "*) ok "the rest of the screen is unchanged" ;; *) bad "the rest of the screen is unchanged" "pane/state/review/hold" "$idstub" ;; esac
# A pane that is not an agent: no rows, no pointer, even with a reader that has rows.
idn=$(id_screen transcript.sh "$IDNP")
case "$idn" in *"doing     "*|*"/btw"*) bad "a non-agent pane shows neither rows nor /btw" "neither" "$idn" ;; *) ok "a non-agent pane shows neither rows nor /btw" ;; esac
idages=$( . "$DIR/scripts/lib/review-fleet.sh" 2>/dev/null; for t in 45 252 3780 0 x ""; do printf '[%s]' "$(age_short "$t")"; done )
[ "$idages" = "[45s][4m12s][1h3m][0s][][]" ] && ok "ages read 45s, 4m12s, 1h3m" || bad "ages read 45s, 4m12s, 1h3m" "[45s][4m12s][1h3m][0s][][]" "$idages"
idages=$( . "$DIR/scripts/lib/review-fleet.sh" 2>&1; for t in 0100 090 1234567890 1e3 -5; do printf '[%s]' "$(age_short "$t")"; done )
[ "$idages" = "[1m40s][1m30s][][][]" ] && ok "ages are base 10 and capped at 9 digits" || bad "ages are base 10 and capped at 9 digits" "[1m40s][1m30s][][][]" "$idages"
"$TMUX_REAL" -L "$ID" kill-server >/dev/null 2>&1
fi

echo "== prefix R sources every config file tmux loaded, not just the first =="
# -f /dev/null above is single-entry #{config_files} -- it never exercised the
# bug. A start with no -f is multi-entry (tmux tries /etc/tmux.conf, ~/.tmux.conf
# and the XDG path in turn) and, on a fresh HOME, actually sources more than
# one of them -- exactly the shape a default install has.
if need_tmux "cockpit_reload_key multi-config"; then :; else
RC="cockpit-rc-$$"; RCO="cockpit-rco-$$"; SOCKS="$SOCKS $RC $RCO"
RCHOME=$(mktemp -d)
mkdir -p "$RCHOME/.config/tmux"
printf 'set -g @reload_marker_home v1\n' > "$RCHOME/.tmux.conf"
printf 'set -g @reload_marker_xdg v1\n' > "$RCHOME/.config/tmux/tmux.conf"
HOME="$RCHOME" "$TMUX_REAL" -L "$RC" new-session -d -s t -x 80 -y 24 'exec -a claude sleep 300' >/dev/null 2>&1

# Sanity: the fixture really did produce a multi-entry #{config_files}, so a
# green result below is not just an artifact of a single-entry fixture.
cfgfiles=$("$TMUX_REAL" -L "$RC" display-message -p '#{config_files}' 2>/dev/null)
case "$cfgfiles" in
    *,*) ok "fixture really has a multi-entry #{config_files}" ;;
    *) bad "fixture really has a multi-entry #{config_files}" "comma-separated paths" "$cfgfiles" ;;
esac

HOME="$RCHOME" "$TMUX_REAL" -L "$RC" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1

rbind=$("$TMUX_REAL" -L "$RC" list-keys -T prefix 2>/dev/null | awk '$4=="R"')
case "$rbind" in
    *"$RCHOME/.tmux.conf"*) ok "reload binding sources the user's ~/.tmux.conf, not just /etc/tmux.conf" ;;
    *) bad "reload binding sources the user's ~/.tmux.conf, not just /etc/tmux.conf" \
         "$RCHOME/.tmux.conf in the binding" "$rbind" ;;
esac
case "$rbind" in
    *"$RCHOME/.config/tmux/tmux.conf"*) ok "reload binding also sources the second config file, not just the first" ;;
    *) bad "reload binding also sources the second config file, not just the first" \
         "$RCHOME/.config/tmux/tmux.conf in the binding" "$rbind" ;;
esac

# Functional check, not just text in the binding: edit both files on disk,
# fire the bound key FOR REAL, confirm BOTH get re-sourced. This is also the
# check that the display-message half is actually PART of the binding rather
# than running once at plugin-load time (the argv-split trap) -- if it only
# ran at load, list-keys above could still show the right source-file text
# while the key itself did nothing.
#
# plain `send-keys -t <pane>` writes bytes into the PANE's input, which is
# not how a prefix keypress reaches tmux -- prefix lookup happens in the
# attached CLIENT reading its own pty, before anything is sent to a pane. So,
# same technique as the ctrl+double-click test above: an outer tmux provides
# a real pty for a client attached to the inner one, and send-keys -H writes
# the raw bytes (02 = C-b, the default prefix; 52 = 'R') into that pty.
printf 'set -g @reload_marker_home v2\n' > "$RCHOME/.tmux.conf"
printf 'set -g @reload_marker_xdg v2\n' > "$RCHOME/.config/tmux/tmux.conf"
"$TMUX_REAL" -L "$RCO" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $RC attach -t t" >/dev/null 2>&1
ROP=$("$TMUX_REAL" -L "$RCO" list-panes -t o -F '#{pane_id}' | head -1)
until_out '?*' "$TMUX_REAL" -L "$RC" list-clients -F '#{client_name}'
if "$TMUX_REAL" -L "$RCO" list-panes -t o >/dev/null 2>&1; then
    "$TMUX_REAL" -L "$RCO" send-keys -t "$ROP" -H 02 52 >/dev/null 2>&1
    until_out v2 "$TMUX_REAL" -L "$RC" show-option -gqv @reload_marker_home
    is "prefix R actually reloads the home conf" "v2" \
       "$("$TMUX_REAL" -L "$RC" show-option -gqv @reload_marker_home)"
    is "prefix R actually reloads the second (xdg) conf too" "v2" \
       "$("$TMUX_REAL" -L "$RC" show-option -gqv @reload_marker_xdg)"
else
    bad "prefix R actually reloads the home conf" "an attached client" "no client"
    bad "prefix R actually reloads the second (xdg) conf too" "an attached client" "no client"
fi
"$TMUX_REAL" -L "$RCO" kill-server >/dev/null 2>&1

"$TMUX_REAL" -L "$RC" kill-server >/dev/null 2>&1
rm -rf "$RCHOME"
fi

echo "== tests/par.sh: every block runs exactly once across the shards, and a red shard reddens the run =="
# A sandboxed copy, never the live tests/run.sh: par.sh derives DIR from its
# own BASH_SOURCE, so it has to sit under tests/ of a fake repo root, not be
# pointed at the file this suite is itself running from (that would recurse).
PARROOT=$(mktmpd)
mkdir -p "$PARROOT/tests"
cp "$DIR/tests/par.sh" "$PARROOT/tests/par.sh"
# The real prelude verbatim (everything above the first block -- ok/bad/is,
# mktmpd, the private-server setup), then four small fixture blocks: a solo
# pass, two sharing a group (one shard), and one that fails on purpose. Built
# with $Q rather than a literal `echo "== `, so this suite's own block-scanning
# tools never mistake the fixture text below for real blocks in THIS file --
# they match that exact substring at column 0, heredoc or not.
awk '/^echo "== /{exit} {print}' "$DIR/tests/run.sh" > "$PARROOT/tests/run.sh"
Q='"'
{
    printf 'echo %s== fake1: solo ==%s\n' "$Q" "$Q"
    printf 'is "fake1" 1 1\n\n'
    printf '# group: g\n'
    printf 'echo %s== fake2a: grouped ==%s\n' "$Q" "$Q"
    printf 'is "fake2a" 1 1\n\n'
    printf '# group: g\n'
    printf 'echo %s== fake2b: grouped ==%s\n' "$Q" "$Q"
    printf 'is "fake2b" 1 1\n\n'
    printf 'echo %s== fake3: the injected failure ==%s\n' "$Q" "$Q"
    printf 'bad "deliberately red" want got\n'
} >> "$PARROOT/tests/run.sh"

EXPECT_BLOCKS=$(grep -c '^echo "== ' "$PARROOT/tests/run.sh")
# -u PAR_BLOCKS: the ordinals that selected THIS block are still in the
# environment (tests/changed.sh, tests/par.sh --each), and the sandbox has four
# blocks, so the inherited set selected none of them and par.sh printed nothing.
PAROUT=$(env -u PAR_BLOCKS J=3 "$PARROOT/tests/par.sh" 2>&1); PARRC=$?
is "a red shard makes par.sh exit non-zero" "1" "$PARRC"
SUMMARY=$(printf '%s\n' "$PAROUT" | grep -oE '[0-9]+ passed, [0-9]+ failed, [0-9]+ skipped' | tail -1)
COUNTED=$(printf '%s\n' "$SUMMARY" | grep -oE '[0-9]+' | awk '{s+=$1} END{print s+0}')
is "every fixture block is counted exactly once (faithfulness: extracted == grep -c)" "$EXPECT_BLOCKS" "$COUNTED"
is "the injected bad is the one failure, the rest still pass" "3 passed, 1 failed, 0 skipped" "$SUMMARY"

echo "== tests/green.sh: the stamp hits on unchanged content, misses on one changed byte, and tool versions are in the key =="
GROOT=$(mktmpd)
mkdir -p "$GROOT/scripts/lib" "$GROOT/tests" "$GROOT/examples"
cp "$DIR/tests/green.sh" "$GROOT/tests/green.sh"
for f in cockpit.tmux install.sh bootstrap.sh scripts/foo scripts/lib/bar tests/baz examples/qux; do
    printf 'a\n' > "$GROOT/$f"
done
(cd "$GROOT" && bash tests/green.sh check) >/dev/null 2>&1
is "no stamp yet: check misses" "1" "$?"
(cd "$GROOT" && bash tests/green.sh stamp) >/dev/null 2>&1
is "stamp records green" "0" "$?"
case "$(cat "$GROOT/tests/.cache/green" 2>/dev/null)" in
    *"tool "*tmux*) ok "tool versions are recorded in the stamp" ;;
    *) bad "tool versions are recorded in the stamp" "a 'tool ...tmux...' line" "$(cat "$GROOT/tests/.cache/green" 2>/dev/null)" ;;
esac
(cd "$GROOT" && bash tests/green.sh check) >/dev/null 2>&1
is "unchanged content: check hits" "0" "$?"
printf 'a\nb\n' > "$GROOT/scripts/foo"
(cd "$GROOT" && bash tests/green.sh check) >/dev/null 2>&1
is "one changed byte: check misses" "1" "$?"
is "changed lists exactly the file that moved" "scripts/foo" "$(cd "$GROOT" && bash tests/green.sh changed)"

echo "== review-gate: a queued review is the Stop's whole additionalContext, once, and drains on a rewake too =="
if need_tmux "review-gate spool"; then :; else
RH="cockpit-rh-$$"; SOCKS="$SOCKS $RH"; RHSTATE=$(mktmpd); RHDIR=$(mktmpd)
( cd "$RHDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e; git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init ) >/dev/null 2>&1
"$TMUX_REAL" -L "$RH" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RHDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RH" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
RHAP=$("$TMUX_REAL" -L "$RH" list-panes -t t: -F '#{pane_id}')
RHCTL=$("$TMUX_REAL" -L "$RH" split-window -t t -P -F '#{pane_id}' 'exec bash'); until_cap "$RH" "$RHCTL" '*$*'
RHSESS="$RHSTATE/tmux-cockpit/review/${RHAP#%}-$(printf '%s' "$RHDIR" | cksum | cut -d' ' -f1)"
RHPAY=$(mktmpf); printf '{"session_id":"s","transcript_path":"/nonexistent","cwd":"%s","hook_event_name":"Stop","stop_hook_active":false}\n' "$RHDIR" > "$RHPAY"
RHPAY2=$(mktmpf); sed 's/"stop_hook_active":false/"stop_hook_active":true/' "$RHPAY" > "$RHPAY2"
# -S -: the full scrollback, not just the visible screen -- several rh_run calls
# of a long wrapped send-keys command overflow the split pane's own height,
# and a visible-only capture can lose an OLD marker off the top for every NEW
# one that scrolls in, so the count never climbs past n0 and rh_wait spins to
# its ceiling on a command that actually finished at once.
rh_n() { "$TMUX_REAL" -L "$RH" capture-pane -p -S - -J -t "$RHCTL" | grep -c 'HRC=[0-9]'; }
rh_start() { "$TMUX_REAL" -L "$RH" send-keys -t "$RHCTL" "TMUX_PANE=$RHAP XDG_STATE_HOME=$(printf '%q' "$RHSTATE") '$DIR/scripts/review-gate' ${2:-} < $(printf '%q' "$1") > $(printf '%q' "$RHSTATE/out") 2>/dev/null; echo HRC=\$?" Enter; }
rh_wait() { local i; for i in $(seq 1 100); do [ "$(rh_n)" -gt "$1" ] && return 0; sleep 0.1; done; return 1; }
rh_run() { local n0; n0=$(rh_n); rh_start "$1" "${2:-}"; rh_wait "$n0"; }
rh_queue() {                  # the spool tmux-review --deliver leaves when the agent is busy
    mkdir -p "$RHSESS"; printf 'Code review from the human\n\n## f:1 (+)\n> a\nREVIEW-MARKER\n\n' > "$RHSESS/spool.md"
    "$TMUX_REAL" -L "$RH" set-option -p -t "$RHAP" @cockpit_review_spool "$RHSESS/spool.md" >/dev/null 2>&1
}
rh_run "$RHPAY"; is "no queued review: the Stop says nothing" "" "$(cat "$RHSTATE/out")"
rh_queue; rh_run "$RHPAY"
is "a queued review is the additionalContext, verbatim and alone" \
   '{"hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"Code review from the human\n\n## f:1 (+)\n> a\nREVIEW-MARKER"}}' \
   "$(cat "$RHSTATE/out")"
is "the spool option is drained" "" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_review_spool)"
is "and the spool file consumed" "absent" "$([ -e "$RHSESS/spool.md" ] && echo present || echo absent)"
rh_run "$RHPAY"; is "the next Stop hands over nothing (no replay)" "" "$(cat "$RHSTATE/out")"
# A rewake Stop (stop_hook_active) still drains: the cap only ends a chain.
rh_queue; rh_run "$RHPAY2"
case "$(cat "$RHSTATE/out")" in *'"additionalContext":"Code review from the human'*'REVIEW-MARKER"'*) ok "a stop_hook_active Stop delivers a queued review too" ;;
    *) bad "a stop_hook_active Stop delivers a queued review too" "the review as additionalContext" "$(cat "$RHSTATE/out")" ;; esac
is "and drains it" "" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_review_spool)"
# An older install's PreToolUse hook still calls the retired --pretool: it must
# not run the Stop path (drain, rewake count, ◆).
RHPRE=$(mktmpf); printf '{"session_id":"s","transcript_path":"/nonexistent","cwd":"%s","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"make test"}}\n' "$RHDIR" > "$RHPRE"
RHLOOP="$RHSTATE/tmux-cockpit/review/loop-$(gate_key "$RH" "$RHAP")"
rh_queue; printf 2 > "$RHLOOP"; rh_run "$RHPRE" --pretool
is "review-gate --pretool prints nothing" "" "$(cat "$RHSTATE/out")"
is "and leaves the spool option, the spool file and the loop count alone" "$RHSESS/spool.md present 2" \
   "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_review_spool) $([ -e "$RHSESS/spool.md" ] && echo present || echo absent) $(cat "$RHLOOP" 2>/dev/null)"
unset -f rh_n rh_start rh_wait rh_run rh_queue
"$TMUX_REAL" -L "$RH" kill-server >/dev/null 2>&1
fi

echo "== tmux-review fleet: ^x kills the window under the cursor, after a y/N =="
if need_tmux "fleet ^x"; then :; else
KW="cockpit-kw-$$"; SOCKS="$SOCKS $KW"
KWD=$(mktmpd); mk_git_dir "$KWD" 1
"$TMUX_REAL" -L "$KW" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$KW" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
KWCTL=$("$TMUX_REAL" -L "$KW" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$KW" new-window -d -t t: -c "$KWD" 'exec -a claude bash' >/dev/null 2>&1
KWP=$("$TMUX_REAL" -L "$KW" list-panes -t t:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$KW" new-session -d -s solo -c "$KWD" 'exec -a claude bash' >/dev/null 2>&1
KWSOLO=$("$TMUX_REAL" -L "$KW" list-panes -t solo -F '#{pane_id}')
# A stand-in fzf: the first call "presses" ^x on the agent's row, the second escs.
KWB=$(mktmpd)
cat > "$KWB/fzf" <<'KWFZF'
#!/usr/bin/env bash
# The shared theme probes the version before it builds an argv (#105); without
# this arm the probe reads the terminal and eats the next typed line.
case "$1" in --version) echo '0.44.1 (debian)'; exit 0 ;; esac
rows=$(cat)
if [ ! -e "$KWFLAG" ]; then : > "$KWFLAG"; printf 'ctrl-x\n%s\n' "$(grep "^$KWTARGET"$'\t' <<<"$rows")"; exit 0; fi
exit 130
KWFZF
chmod +x "$KWB/fzf"
kwn=0
kw_run() { rm -f "$KWB/flag"; kwn=$((kwn+1))   # kw_run [target]: an unmatched target is ^x on an empty filter
    "$TMUX_REAL" -L "$KW" send-keys -t "$KWCTL" "clear; echo KWSTAR''T$kwn; KWFLAG='$KWB/flag' KWTARGET='${1:-$KWP}' PATH='$KWB:$PATH' '$DIR/scripts/tmux-review'; echo KWEN''D$kwn" Enter
    until_cap "$KW" "$KWCTL" "*KWSTART$kwn*"; }   # past the clear: no stale prompt can match
kw_done() { until_cap "$KW" "$KWCTL" "*KWEND$kwn*"; }
kw_run '%none'; kw_done
case "$("$TMUX_REAL" -L "$KW" capture-pane -p -t "$KWCTL")" in
    *"kill window"*) bad "^x on an empty filter asks nothing" "no prompt" "$("$TMUX_REAL" -L "$KW" capture-pane -p -t "$KWCTL" | grep 'kill window')" ;;
    *) ok "^x on an empty filter asks nothing" ;;
esac
is "and touches no window" "2" "$("$TMUX_REAL" -L "$KW" list-windows -t t -F x | wc -l | tr -d ' ')"
kw_run; until_cap "$KW" "$KWCTL" '*y/N*'
case "$("$TMUX_REAL" -L "$KW" capture-pane -p -t "$KWCTL")" in
    *"kill window t:1"*"[y/N]"*) ok "^x asks first, naming the window" ;;
    *) bad "^x asks first, naming the window" "kill window t:1 ...? [y/N]" \
         "$("$TMUX_REAL" -L "$KW" capture-pane -p -t "$KWCTL" | grep -v '^$' | tail -3)" ;;
esac
"$TMUX_REAL" -L "$KW" send-keys -t "$KWCTL" n; kw_done
is "n leaves the window alone" "2" "$("$TMUX_REAL" -L "$KW" list-windows -t t -F x | wc -l | tr -d ' ')"
kw_run "$KWSOLO"; until_cap "$KW" "$KWCTL" '*y/N*'
case "$("$TMUX_REAL" -L "$KW" capture-pane -p -t "$KWCTL")" in
    *"kill session solo (last window)?"*) ok "a session's last window is named as the session it takes" ;;
    *) bad "a session's last window is named as the session it takes" "kill session solo (last window)? [y/N]" \
         "$("$TMUX_REAL" -L "$KW" capture-pane -p -t "$KWCTL" | grep 'kill ' | tail -1)" ;;
esac
"$TMUX_REAL" -L "$KW" send-keys -t "$KWCTL" y; kw_done
is "and y takes the session" "t" "$("$TMUX_REAL" -L "$KW" list-sessions -F '#{session_name}')"
kw_run; until_cap "$KW" "$KWCTL" '*y/N*'
"$TMUX_REAL" -L "$KW" send-keys -t "$KWCTL" y; kw_done
is "y kills it" "1" "$("$TMUX_REAL" -L "$KW" list-windows -t t -F x | wc -l | tr -d ' ')"
is "and the pane the list was opened from survives" "$KWCTL" "$("$TMUX_REAL" -L "$KW" list-panes -a -F '#{pane_id}')"
"$TMUX_REAL" -L "$KW" kill-server >/dev/null 2>&1
fi

echo "== cockpit-tui: tui-fetch verifies before it installs, and never breaks a working binary =="
# No network: the release host is a file:// tree the block builds itself. The
# binary is a stand-in shell script -- tui-fetch only ever asks it --version,
# which is exactly the check a checksum cannot make. env -u: this block alone
# lifts the suite's offline switch, against a local host and stub gh and curl;
# COCKPIT_TUI_BIN is the copy's own, never the suite's real binary.
if ! command -v curl >/dev/null 2>&1; then skip_ "tui-fetch (no curl)"
elif ! command -v sha256sum >/dev/null 2>&1; then skip_ "tui-fetch (no sha256sum)"; else
TF=$(mktmpd); mkdir -p "$TF/plugin/scripts/lib" "$TF/build" "$TF/curlshim" "$TF/shim"
cp "$DIR/scripts/tui-fetch" "$TF/plugin/scripts/"; cp "$DIR/scripts/lib/vnum.sh" "$DIR/scripts/lib/tui.sh" "$TF/plugin/scripts/lib/"
TFB="$TF/plugin/bin/cockpit-tui"; TFR="file://$TF/rel"; TFGONE="file://$TF/no-such-host"
# Counting curl, not timing it: "a 404 is retried" is a fork count, never a clock.
printf '#!/bin/sh\necho x >> "%s"\nexec %s "$@"\n' "$TF/curlcount" "$(command -v curl)" > "$TF/curlshim/curl"
# An old rustc and a cargo that shouts if the floor check ever lets it run.
printf '#!/bin/sh\necho "rustc 1.85.0 (0000000 2026-01-01)"\n' > "$TF/shim/rustc"
printf '#!/bin/sh\necho CARGO_RAN >&2; exit 1\n' > "$TF/shim/cargo"
chmod +x "$TF/curlshim/curl" "$TF/shim/rustc" "$TF/shim/cargo"
# Every target name gets the same bytes, so the block runs on whatever box the
# suite is on without re-deriving the uname mapping the script under test owns.
tf_release() {                # <tag> <version-it-reports> [flip|flat|broken|noasset]
    local d="$TF/rel/$1" t
    mkdir -p "$d"; rm -rf "$TF/build"; mkdir -p "$TF/build"
    if [ "${3:-}" = noasset ]; then
        # A manifest that lists only somebody else's platform: settled, not transient.
        printf '%s  cockpit-tui-mips-unknown-linux.tar.gz\n' 0000 > "$d/sha256.sum"
        return 0
    fi
    # Measured on dist 0.32.0: the archive is a cockpit-tui-<target>/ directory
    # holding the binary beside LICENSE and README. `flat` pins that an archive
    # without that wrapper still installs; `broken` is a binary that cannot run
    # here at all, which is a different answer from one that names another version.
    if [ "${3:-}" = broken ]; then
        printf '#!/nonexistent-interpreter\n' > "$TF/build/cockpit-tui"
        chmod +x "$TF/build/cockpit-tui"
        ( cd "$TF/build" && tar -czf "$d/a.tgz" cockpit-tui )
    elif [ "${3:-}" = flat ]; then
        printf '#!/bin/sh\necho "cockpit-tui %s"\n' "$2" > "$TF/build/cockpit-tui"
        chmod +x "$TF/build/cockpit-tui"
        ( cd "$TF/build" && tar -czf "$d/a.tgz" cockpit-tui )
    else
        mkdir -p "$TF/build/wrap"
        printf '#!/bin/sh\necho "cockpit-tui %s"\n' "$2" > "$TF/build/wrap/cockpit-tui"
        chmod +x "$TF/build/wrap/cockpit-tui"; : > "$TF/build/wrap/LICENSE"
        ( cd "$TF/build" && tar -czf "$d/a.tgz" wrap )
    fi
    for t in x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin; do
        cp "$d/a.tgz" "$d/cockpit-tui-$t.tar.gz"
    done
    rm -f "$d/a.tgz"
    ( cd "$d" && sha256sum cockpit-tui-*.tar.gz > sha256.sum )
    if [ "${3:-}" = flip ]; then
        awk '{ c=substr($0,1,1); print (c=="0" ? "1" : "0") substr($0,2) }' \
            "$d/sha256.sum" > "$d/s2" && mv "$d/s2" "$d/sha256.sum"
    fi
}
tf() {                        # <release-base-url> <args...>
    local u=$1; shift
    COCKPIT_TUI_RELEASES="$u" env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" "$@" 2>&1
}
tf_ver() { "$TFB" --version 2>/dev/null; }
tf_mark() { cat "$TF/plugin/bin/.tried" 2>/dev/null; }

tf_release cockpit-tui-v0.1.0 0.1.0
tfout=$(tf "$TFR" --version cockpit-tui-v0.1.0); tfrc=$?
is "tui-fetch installs a tarball that matches its published checksum" "0" "$tfrc"
is "and the installed binary is the one the tag named" "cockpit-tui 0.1.0" "$(tf_ver)"
# Deleting the release proves the second run never went looking for it: the
# probe is the binary in bin/, not a name on PATH -- fzf's installer
# re-downloaded everything when a distro copy shadowed its own.
rm -rf "$TF/rel/cockpit-tui-v0.1.0"
tfout=$(tf "$TFR" --version cockpit-tui-v0.1.0); tfrc=$?
case "$tfout" in
    *"already current"*) ok "a second run with a current binary says already current" ;;
    *) bad "a second run with a current binary says already current" "already current" "$tfout" ;;
esac
is "and exits 0 without reaching the release host" "0" "$tfrc"
# The release dir is gone, so this can only match by reducing tag and binary to
# the same string: cargo-dist parses the tag with axotag, which wants the
# PACKAGE name, and `tui-v0.1.0` died in the plan step (#79). The pre-#79 form
# still has to resolve for anyone whose @cockpit_tui_version still holds it.
tfout=$(tf "$TFR" --version tui-v0.1.0)
case "$tfout" in
    *"already current"*) ok "the pre-#79 tui-v tag form reduces to the same version" ;;
    *) bad "the pre-#79 tui-v tag form reduces to the same version" "already current" "$tfout" ;;
esac
tfout=$(tf "$TFR" --version); tfrc=$?
is "--version with no tag is a usage error, not a silent install of the pin" "2" "$tfrc"

# A refusal must shout on EVERY press. Marking it tried would turn the one
# signal that says "these bytes are not ours" into a single line nobody re-reads.
tf_release tui-v0.2.0 0.2.0 flip
tfout=$(tf "$TFR" --version tui-v0.2.0); tfrc=$?
is "a flipped checksum exits non-zero" "1" "$tfrc"
case "$tfout" in
    *REFUSED*"does not match its published checksum"*) ok "and says so instead of installing" ;;
    *) bad "and says so instead of installing" "REFUSED ... does not match its published checksum" "$tfout" ;;
esac
is "and the working binary is untouched" "cockpit-tui 0.1.0" "$(tf_ver)"
tfout=$(tf "$TFR" --version tui-v0.2.0); tfrc=$?
is "the refusal repeats on the next press" "1" "$tfrc"
case "$tfout" in
    *REFUSED*) ok "and is never silenced by a tried marker" ;;
    *) bad "and is never silenced by a tried marker" "REFUSED" "$tfout" ;;
esac
is "a refusal leaves no marker behind" "" "$(tf_mark)"

# A checksum says the bytes arrived intact, not that they are the version the
# tag claims -- fzf's installer runs the same behavioural check.
tf_release tui-v0.3.0 9.9.9
tfout=$(tf "$TFR" --version tui-v0.3.0); tfrc=$?
is "a tarball whose binary reports another version installs nothing" "1" "$tfrc"
is "and the working binary is still the old one" "cockpit-tui 0.1.0" "$(tf_ver)"

# An uncut tag is the ordinary state before the first release, and it is
# transient: the marker must not lock the next press out of trying again.
: > "$TF/curlcount"
tfout=$(PATH="$TF/curlshim:$PATH"; tf "$TFGONE" --version tui-v0.4.0); tfrc=$?
is "a release host it cannot reach exits 0" "0" "$tfrc"
case "$tfout" in
    *"no release tui-v0.4.0 (private repository, or offline)"*) ok "and names the tag rather than blaming the network" ;;
    *) bad "and names the tag rather than blaming the network" "skip: no release tui-v0.4.0 (private repository, or offline)" "$tfout" ;;
esac
is "and leaves no marker" "" "$(tf_mark)"
tfout=$(PATH="$TF/curlshim:$PATH"; tf "$TFGONE" --version tui-v0.4.0)
is "so the next press fetches again instead of saying already tried" "2" "$(wc -l < "$TF/curlcount" | tr -d ' ')"
# A box that could not ask must not fall through to a full cargo build.
tfout=$(PATH="$TF/shim:$TF/curlshim:$PATH"; tf "$TFGONE" --version tui-v0.4.0)
case "$tfout" in
    *"needs Rust"*|*CARGO_RAN*) bad "without --required, an unreachable host never starts a source build" "no rustc gate, no cargo" "$tfout" ;;
    *) ok "without --required, an unreachable host never starts a source build" ;;
esac

# Deleting the binary after a good install must reinstall it, not report a
# marker left over from the install that worked.
tf_release tui-v0.5.0 0.5.0
tfout=$(tf "$TFR" --version tui-v0.5.0)
is "a new version installs over the old one" "cockpit-tui 0.5.0" "$(tf_ver)"
rm -f "$TFB"
tfout=$(tf "$TFR" --version tui-v0.5.0)
is "and deleting the binary refetches it" "cockpit-tui 0.5.0" "$(tf_ver)"

# The source fallback. The floor is READ from tui/Cargo.toml, never pinned
# twice: Debian 13 ships rustc 1.85 against ratatui's 1.88, so this refusal is
# the common case and it must not turn into a page of compiler errors.
mkdir -p "$TF/plugin/tui"
printf 'rust-version = "1.88"\n' > "$TF/plugin/tui/Cargo.toml"
tf_release tui-v0.6.0 - noasset
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --version tui-v0.6.0); tfrc=$?
case "$tfout" in
    *"no binary for this platform"*) ok "a manifest without this platform is a settled answer" ;;
    *) bad "a manifest without this platform is a settled answer" "no binary for this platform" "$tfout" ;;
esac
is "and it is remembered, so the next press does not re-ask" "tui-v0.6.0" "$(tf_mark)"
is "a rustc below the floor still exits 0" "0" "$tfrc"
case "$tfout" in
    *"needs Rust >= 1.88"*"found 1.85.0"*) ok "and names the floor and the rustc it found" ;;
    *) bad "and names the floor and the rustc it found" "needs Rust >= 1.88 ... (found 1.85.0)" "$tfout" ;;
esac
case "$tfout" in
    *CARGO_RAN*) bad "and never reaches the compiler" "no cargo invocation" "$tfout" ;;
    *) ok "and never reaches the compiler" ;;
esac
is "and leaves the working binary in place" "cockpit-tui 0.5.0" "$(tf_ver)"
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --version tui-v0.6.0)
case "$tfout" in
    *"has no release build for"*) ok "the marker skips the download on the next press" ;;
    *) bad "the marker skips the download on the next press" "tui-v0.6.0 has no release build for" "$tfout" ;;
esac
case "$tfout" in
    *"needs Rust >= 1.88"*) ok "and still offers the source build, which the marker must never block" ;;
    *) bad "and still offers the source build, which the marker must never block" "needs Rust >= 1.88" "$tfout" ;;
esac
printf 'rust-version = "1.99"\n' > "$TF/plugin/tui/Cargo.toml"
tf_release tui-v0.7.0 - noasset
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --version tui-v0.7.0)
case "$tfout" in
    *"needs Rust >= 1.99"*) ok "the floor comes from Cargo.toml, not a second pin in the script" ;;
    *) bad "the floor comes from Cargo.toml, not a second pin in the script" "needs Rust >= 1.99" "$tfout" ;;
esac
# --required is the install path (#199): a binary that runs is success, and
# none at all is exit 1 naming both ways out, whether settled or offline.
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --required --version tui-v0.7.0); tfrc=$?
is "--required with a working binary left exits 0" "0" "$tfrc"
mv "$TFB" "$TF/keep"
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --required --version tui-v0.7.0); tfrc=$?
is "--required with no binary and no way to build one exits 1" "1" "$tfrc"
case "$tfout" in
    *"cockpit-tui is required"*"no Rust >= 1.99"*"gh auth login"*rustup*) ok "and names the floor, gh auth login and rustup" ;;
    *) bad "and names the floor, gh auth login and rustup" "cockpit-tui is required ... Rust >= 1.99 ... gh auth login ... rustup" "$tfout" ;;
esac
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFGONE" --required --version tui-v0.4.0); tfrc=$?
is "--required offline with no binary exits 1 too" "1" "$tfrc"
# A gh 404 or no login is transient, yet a required binary must still try
# cargo: a box with Rust and no gh login is the common source install.
mkdir -p "$TF/shim99"
printf '#!/bin/sh\necho "rustc 1.99.0 (0000000 2026-01-01)"\n' > "$TF/shim99/rustc"
printf '#!/bin/sh\necho "$*" >> "%s"\nexit 1\n' "$TF/cargo99" > "$TF/shim99/cargo"
chmod +x "$TF/shim99/rustc" "$TF/shim99/cargo"
tfout=$(PATH="$TF/shim99:$PATH"; tf "$TFGONE" --required --version tui-v0.4.0)
case "$(cat "$TF/cargo99" 2>/dev/null)" in
    *"build --release --locked"*) ok "--required with an unreachable release and Rust >= the floor runs cargo" ;;
    *) bad "--required with an unreachable release and Rust >= the floor runs cargo" "cargo build --release --locked" "$tfout" ;;
esac
is "and marks nothing tried, the miss being transient" "no" "$([ "$(tf_mark)" = tui-v0.4.0 ] && echo yes || echo no)"
mv "$TF/keep" "$TFB"

# A verified binary that will not execute here is this box's problem, not a
# broken release: source is the way out, so it must not exit 1 like a refusal.
tf_release tui-v0.8.0 0.8.0 broken
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --version tui-v0.8.0); tfrc=$?
is "a verified binary that cannot run here exits 0" "0" "$tfrc"
case "$tfout" in
    *"does not run here"*"needs Rust >= 1.99"*) ok "and falls through to the source build" ;;
    *) bad "and falls through to the source build" "does not run here ... needs Rust >= 1.99" "$tfout" ;;
esac
is "and leaves the working binary in place" "cockpit-tui 0.5.0" "$(tf_ver)"

tf_release tui-v0.9.0 0.9.0 flat
tfout=$(tf "$TFR" --version tui-v0.9.0)
is "an archive with no wrapping directory installs too" "cockpit-tui 0.9.0" "$(tf_ver)"
# COCKPIT_TUI_BIN is where it reads and writes, as for the doctor and wrappers.
tfout=$(env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TF/else/ct" COCKPIT_TUI_RELEASES="$TFR" \
        "$TF/plugin/scripts/tui-fetch" --version tui-v0.9.0 2>&1)
is "COCKPIT_TUI_BIN names the file it installs" "cockpit-tui 0.9.0" "$("$TF/else/ct" --version 2>/dev/null)"
tfout=$(env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TF/else/ct" "$TF/plugin/scripts/tui-fetch" --version tui-v0.9.0 2>&1)
case "$tfout" in
    *"already current"*) ok "and the binary it reads" ;;
    *) bad "and the binary it reads" "already current" "$tfout" ;;
esac
# Unpinned (no --version, no option), a newer binary is current: a make
# tui-bin build ahead of the release must not be downgraded to it.
mkdir -p "$TF/tmuxnone" "$TF/tmuxold"
printf '#!/bin/sh\nexit 0\n' > "$TF/tmuxnone/tmux"
printf '#!/bin/sh\necho cockpit-tui-v0.1.0\n' > "$TF/tmuxold/tmux"
chmod +x "$TF/tmuxnone/tmux" "$TF/tmuxold/tmux"
tfout=$(PATH="$TF/tmuxnone:$PATH"; tf "$TFGONE"); tfrc=$?
case "$tfout" in
    *"cockpit-tui 0.9.0 already current"*) ok "unpinned, a binary newer than the floor is already current" ;;
    *) bad "unpinned, a binary newer than the floor is already current" "cockpit-tui 0.9.0 already current" "$tfout" ;;
esac
is "and exits 0 with it in place" "0 cockpit-tui 0.9.0" "$tfrc $(tf_ver)"
# @cockpit_tui_version below TUI_MIN would reinstall a binary the scripts refuse.
tfout=$(PATH="$TF/tmuxold:$PATH"; tf "$TFR"); tfrc=$?
is "an @cockpit_tui_version below TUI_MIN is refused" "1" "$tfrc"
case "$tfout" in
    *"@cockpit_tui_version pins cockpit-tui-v0.1.0, below the required"*) ok "and says which pin and why" ;;
    *) bad "and says which pin and why" "@cockpit_tui_version pins cockpit-tui-v0.1.0, below the required ..." "$tfout" ;;
esac
is "and the working binary is untouched" "cockpit-tui 0.9.0" "$(tf_ver)"

# The repo is private (#84): the plain URL 404s without credentials, so the
# fetch rides the user's own gh login when there is one. The shims serve the
# same fixture tree; fork counts prove which path carried the bytes.
mkdir -p "$TF/ghshim" "$TF/ghbad" "$TF/curlfake"
cat > "$TF/ghshim/gh" <<'GHSHIM'
#!/bin/sh
# auth always passes; download copies the fixture asset. Every argv is logged,
# auth included: which auth subcommand runs is asserted below.
TF="$(cd "$(dirname "$0")/.." && pwd)"
printf '%s\n' "$*" >> "$TF/ghlog"
[ "$1" = auth ] && exit 0
shift 2
tag= pat= out=
while [ $# -gt 0 ]; do
    case "$1" in
        -R) shift 2 ;;
        -p) pat=$2; shift 2 ;;
        -O) out=$2; shift 2 ;;
        *)  tag=$1; shift ;;
    esac
done
exec cp "$TF/rel/$tag/$pat" "$out"
GHSHIM
cat > "$TF/ghbad/gh" <<'GHBAD'
#!/bin/sh
# a gh that is installed but not logged in; a download through it shouts.
TF="$(cd "$(dirname "$0")/.." && pwd)"
[ "$1" = auth ] && exit 1
echo GH_RAN >> "$TF/ghlog"; exit 1
GHBAD
cat > "$TF/curlfake/curl" <<'CURLFAKE'
#!/bin/sh
# the real default URL served from the fixture: no network, one line per fork.
TF="$(cd "$(dirname "$0")/.." && pwd)"
echo x >> "$TF/curlcount"
out=
while [ $# -gt 1 ]; do [ "$1" = -o ] && out=$2; shift; done
exec cp "$TF/rel/${1##*releases/download/}" "$out"
CURLFAKE
chmod +x "$TF/ghshim/gh" "$TF/ghbad/gh" "$TF/curlfake/curl"

# The tmux-review tui row runs --auth on both "not installed" branches, i.e. on
# a keypress: `gh auth token` reads the local config, `gh auth status` would
# validate against api.github.com with no timeout to bound it.
: > "$TF/ghlog"
tfout=$(PATH="$TF/ghshim:$PATH" env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" --auth 2>&1)
is "--auth names the gh login the fetch would ride" "via gh" "$tfout"
case "$(cat "$TF/ghlog")" in
    *"auth status"*) bad "and asks the local token, never the network auth status" "auth token" "$(cat "$TF/ghlog")" ;;
    *"auth token"*)  ok "and asks the local token, never the network auth status" ;;
    *) bad "and asks the local token, never the network auth status" "auth token" "$(cat "$TF/ghlog")" ;;
esac

tf_release tui-v1.0.0 1.0.0
: > "$TF/curlcount"; : > "$TF/ghlog"
tfout=$(PATH="$TF/ghshim:$TF/curlfake:$PATH" env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" --version tui-v1.0.0 2>&1); tfrc=$?
is "an authenticated gh is preferred over the plain URL" "0" "$tfrc"
is "and installs the release it serves" "cockpit-tui 1.0.0" "$(tf_ver)"
case "$tfout" in
    *"via gh"*) ok "and the install line says via gh" ;;
    *) bad "and the install line says via gh" "cockpit-tui 1.0.0 -> ... via gh" "$tfout" ;;
esac
is "and curl is never forked" "0" "$(wc -l < "$TF/curlcount" | tr -d ' ')"
case "$(cat "$TF/ghlog")" in
    *"-R TzachiSh/tmux-cockpit"*) ok "and -R defaults to the plugin's own repo" ;;
    *) bad "and -R defaults to the plugin's own repo" "-R TzachiSh/tmux-cockpit" "$(cat "$TF/ghlog")" ;;
esac
tf_release tui-v1.1.0 1.1.0
: > "$TF/ghlog"
tfout=$(PATH="$TF/ghshim:$TF/curlfake:$PATH" COCKPIT_TUI_REPO=alice/example \
    env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" --version tui-v1.1.0 2>&1)
case "$(cat "$TF/ghlog")" in
    *"-R alice/example"*) ok "COCKPIT_TUI_REPO overrides the repo gh asks" ;;
    *) bad "COCKPIT_TUI_REPO overrides the repo gh asks" "-R alice/example" "$(cat "$TF/ghlog")" ;;
esac

tf_release tui-v1.2.0 1.2.0
: > "$TF/curlcount"; : > "$TF/ghlog"
tfout=$(PATH="$TF/ghbad:$TF/curlfake:$PATH" env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" --version tui-v1.2.0 2>&1); tfrc=$?
is "a gh that is not logged in falls back to curl" "0" "$tfrc"
is "and still installs" "cockpit-tui 1.2.0" "$(tf_ver)"
is "and paid exactly the two curl forks" "2" "$(wc -l < "$TF/curlcount" | tr -d ' ')"
case "$(cat "$TF/ghlog")" in
    *GH_RAN*) bad "and gh is never asked to download" "no gh release download" "$(cat "$TF/ghlog")" ;;
    *) ok "and gh is never asked to download" ;;
esac
case "$tfout" in
    *unauthenticated*) ok "and the install line says unauthenticated" ;;
    *) bad "and the install line says unauthenticated" "cockpit-tui 1.2.0 -> ... unauthenticated" "$tfout" ;;
esac

# A tag gh has no release for: fall to the plain URL, then skip -- never a
# source build, and no marker, so the next press may ask again.
: > "$TF/curlcount"; : > "$TF/ghlog"
tfout=$(PATH="$TF/ghshim:$TF/curlfake:$TF/shim:$PATH" env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" --version tui-v9.9.0 2>&1); tfrc=$?
is "a tag gh has no release for still exits 0" "0" "$tfrc"
case "$tfout" in
    *"no release tui-v9.9.0 via gh (or offline)"*) ok "and the skip names the gh path" ;;
    *) bad "and the skip names the gh path" "skip: no release tui-v9.9.0 via gh (or offline)" "$tfout" ;;
esac
is "and fell through to one curl try" "1" "$(wc -l < "$TF/curlcount" | tr -d ' ')"
case "$tfout" in
    *CARGO_RAN*|*"needs Rust"*) bad "a gh 404 never starts a source build" "no rustc gate, no cargo" "$tfout" ;;
    *) ok "a gh 404 never starts a source build" ;;
esac
is "and leaves no marker" "" "$(tf_mark)"
unset -f tf tf_release tf_ver tf_mark
fi

# tests: scripts/tui-fetch
echo "== the suite never reaches the network: install.sh fetches nothing under COCKPIT_TUI_OFFLINE =="
# A plugin copy with no binary forces a fetch; gh, curl and cargo are recorders
# that fail, so any call is a line here and never a request.
NW=$(mktmpd); mkdir -p "$NW/p" "$NW/stub" "$NW/home" "$NW/bin"
cp "$DIR/install.sh" "$NW/p/"; cp -r "$DIR/scripts" "$NW/p/"
for t in gh curl cargo; do
    printf '#!/bin/sh\necho "%s $*" >> %s/calls\nexit 1\n' "$t" "$NW" > "$NW/stub/$t"; chmod +x "$NW/stub/$t"
done
# -u COCKPIT_TUI_BIN: the copy's own empty bin/, not the suite's current binary.
nw() { PATH="$NW/stub:$PATH" HOME="$NW/home" XDG_BIN_HOME="$NW/bin" "$@" -u COCKPIT_TUI_BIN "$NW/p/install.sh" </dev/null 2>&1; }
nwout=$(nw env); nwrc=$?
is "install.sh with no binary calls no gh, curl or cargo under the suite's switch" "" "$(cat "$NW/calls" 2>/dev/null)"
is "and exits 1, since the binary is required" "1" "$nwrc"
case "$nwout" in
    *"cockpit-tui is required"*) ok "and says the binary is required" ;;
    *) bad "and says the binary is required" "cockpit-tui is required ..." "$nwout" ;;
esac
# The control: without the switch the same run reaches the recorders, so the
# first assertion can fail.
nw env -u COCKPIT_TUI_OFFLINE >/dev/null
case "$(cat "$NW/calls" 2>/dev/null)" in
    *curl*|*gh*) ok "without the switch the same run would have fetched" ;;
    *) bad "without the switch the same run would have fetched" "a gh or curl call" "none" ;;
esac
unset -f nw
rm -rf "$NW"

echo "== cockpit-tui: the tui row in tmux-review --info =="
# A COPY of scripts/, so the row reads that copy's bin/ and the suite never
# writes into the checkout it is running from (several sessions share it).
if need_tmux "tui row in --info"; then :; else
TI="cockpit-ti-$$"; SOCKS="$SOCKS $TI"; TIP=$(mktmpd)
cp -r "$DIR/scripts" "$TIP/"
# -u: run-shell has the server's environment, and the row must read the copy's bin/.
env -u COCKPIT_TUI_BIN "$TMUX_REAL" -L "$TI" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
TIAP=$("$TMUX_REAL" -L "$TI" list-panes -t t: -F '#{pane_id}')
# The optional shim dir pins the row's credential word (#84) whatever gh on
# the box would say; \$PATH is the server's, expanded by its own sh.
ti_info() { "$TMUX_REAL" -L "$TI" run-shell "PATH=\"${1:-/nonexistent}:\$PATH\" '$TIP/scripts/tmux-review' -p $TIAP --info" 2>&1; }
mkdir -p "$TIP/gh" "$TIP/ghbad"
printf '#!/bin/sh\nexit 0\n' > "$TIP/gh/gh"
printf '#!/bin/sh\nexit 1\n' > "$TIP/ghbad/gh"
chmod +x "$TIP/gh/gh" "$TIP/ghbad/gh"
tiout=$(ti_info "$TIP/gh")
case "$tiout" in
    *"tui       not installed  (prefix U fetches it, via gh)"*) ok "an absent binary names the gesture and the gh login it will ride" ;;
    *) bad "an absent binary names the gesture and the gh login it will ride" "tui       not installed  (prefix U fetches it, via gh)" "$tiout" ;;
esac
tiout=$(ti_info "$TIP/ghbad")
case "$tiout" in
    *"tui       not installed  (prefix U fetches it, unauthenticated)"*) ok "and without a gh login, says the fetch is unauthenticated" ;;
    *) bad "and without a gh login, says the fetch is unauthenticated" "tui       not installed  (prefix U fetches it, unauthenticated)" "$tiout" ;;
esac
mkdir -p "$TIP/bin"
printf '#!/bin/sh\necho "cockpit-tui 0.1.0"\n' > "$TIP/bin/cockpit-tui"; chmod +x "$TIP/bin/cockpit-tui"
tiout=$(ti_info)
case "$tiout" in
    *"tui       0.1.0  $TIP/bin/cockpit-tui"*) ok "an installed binary shows its version and its path" ;;
    *) bad "an installed binary shows its version and its path" "tui       0.1.0  $TIP/bin/cockpit-tui" "$tiout" ;;
esac
"$TMUX_REAL" -L "$TI" set -g @cockpit_tui_version cockpit-tui-v0.2.0 >/dev/null 2>&1
tiout=$(ti_info)
case "$tiout" in
    *"tui       0.1.0  $TIP/bin/cockpit-tui  (want 0.2.0 -- prefix U updates it)"*)
        ok "a pinned version the binary does not match reads as stale" ;;
    *) bad "a pinned version the binary does not match reads as stale" \
         "tui       0.1.0  ...  (want 0.2.0 -- prefix U updates it)" "$tiout" ;;
esac
unset -f ti_info
"$TMUX_REAL" -L "$TI" kill-server >/dev/null 2>&1
fi

echo "== cockpit-tui: docs name only real options =="
# Three README lines went stale on review phase 3 and cost a round; this asserts
# every @cockpit_* / COCKPIT_* name in docs/tui.md is one some code answers to.
# The freeze (#61 workflow §3) is agreed before the code lands, so its names are
# allowed early -- each is listed with the lane that introduces it.
TUI_FROZEN="@cockpit_tui_pid:L0 @cockpit_tui_version:L4 @cockpit_board_key:L6 \
@cockpit_board_window:L6 @cockpit_tui_fetch:L6 COCKPIT_ACTION:L0 COCKPIT_TMUX:L0 COCKPIT_LOG:L0 \
COCKPIT_READONLY:L0 COCKPIT_GLYPHS:L0"
if [ ! -f "$DIR/docs/tui.md" ]; then
    bad "docs/tui.md exists" "a file" "missing"
else
    tui_unknown=""
    while read -r n; do
        [ -n "$n" ] || continue
        case " $TUI_FROZEN " in *" $n:"*) continue ;; esac
        grep -rqF -- "$n" "$DIR/cockpit.tmux" "$DIR"/scripts/* "$DIR"/tui/src/* 2>/dev/null \
            || tui_unknown="$tui_unknown $n"
    done <<< "$(grep -oE '@cockpit_[a-z0-9_]+|COCKPIT_[A-Z0-9_]+' "$DIR/docs/tui.md" | sort -u)"
    is "every option and env name in docs/tui.md is real or frozen" "" "$tui_unknown"
    # The action file is the whole reason the binary can stay pure; a doc that
    # loses it lets the next lane reach for stdout, which ratatui already owns.
    case "$(cat "$DIR/docs/tui.md")" in
        *COCKPIT_ACTION*) ok "docs/tui.md documents the action file" ;;
        *) bad "docs/tui.md documents the action file" "COCKPIT_ACTION" "absent" ;;
    esac
fi

echo "== cockpit-tui: --json names every agent pane and nothing else =="
if need_tmux "cockpit-tui --json"; then :; else
TJ="cockpit-tj-$$"; SOCKS="$SOCKS $TJ"
"$TMUX_REAL" -L "$TJ" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
TJSH=$("$TMUX_REAL" -L "$TJ" list-panes -t t:0 -F '#{pane_id}')
# argv0 claude, so pane_current_command reads as an agent; sleep and not bash so
# no rc file runs beside the assertion.
"$TMUX_REAL" -L "$TJ" new-window -d -t t: 'exec -a claude sleep 3600' >/dev/null 2>&1
TJA=$("$TMUX_REAL" -L "$TJ" list-panes -t t:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$TJ" new-window -d -t t: 'exec -a claude sleep 3600' >/dev/null 2>&1
TJB=$("$TMUX_REAL" -L "$TJ" list-panes -t t:2 -F '#{pane_id}')
for p in "$TJA" "$TJB"; do
    "$TMUX_REAL" -L "$TJ" set -p -t "$p" @panestate running     >/dev/null 2>&1
    "$TMUX_REAL" -L "$TJ" set -p -t "$p" @cockpit_title "t $p"   >/dev/null 2>&1
    "$TMUX_REAL" -L "$TJ" set -p -t "$p" @cockpit_session "0-$p" >/dev/null 2>&1
done
# COCKPIT_TMUX still resolves `tmux` through the suite's PATH shim; tmux takes
# the LAST -L, so the binary lands on this block's server, not the suite's
# (tests/run.sh:31). $TJSH carries no option and must not appear.
TJOUT=$(COCKPIT_TMUX="tmux -L $TJ" "$COCKPIT_TUI_BIN" --json 2>/dev/null)
is "--json names both agent panes and not the shell pane" "$TJA $TJB" \
   "$(printf '%s\n' "$TJOUT" | sed -n 's/^    "id": "\(%[0-9]*\)".*/\1/p' | tr '\n' ' ' | sed 's/ $//')"
is "--json carries the 14 frozen keys, in order" \
   "id where state title doing said asked msg since loc diff review session path" \
   "$(printf '%s\n' "$TJOUT" | sed -n 's/^    "\([a-z]*\)":.*/\1/p' | head -14 | tr '\n' ' ' | sed 's/ $//')"
is "--json exits 0 and says nothing about $TJSH" "0 no" \
   "$(COCKPIT_TMUX="tmux -L $TJ" "$COCKPIT_TUI_BIN" --json >/dev/null 2>&1; printf '%s ' "$?"; \
      case "$TJOUT" in *"\"$TJSH\""*) printf yes ;; *) printf no ;; esac)"

# F1: a Bash tool_use whose command holds a TAB used to split --rows into 10
# fields. HOME is redirected so the fixture transcript never touches the real
# ~/.claude; the slug is the cwd with every byte outside [A-Za-z0-9-] as "-".
TJH=$(mktmpd); TJW=$(mktmpd)
TJSLUG=$(printf '%s' "$TJW" | sed 's/[^A-Za-z0-9-]/-/g')
TJUUID="0-$TJA"
mkdir -p "$TJH/.claude/projects/$TJSLUG"
printf '%s\n' \
  "{\"type\":\"assistant\",\"timestamp\":\"2026-08-27T10:00:00.000Z\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{\"command\":\"col1\tcol2\"}}]}}" \
  > "$TJH/.claude/projects/$TJSLUG/$TJUUID.jsonl"
"$TMUX_REAL" -L "$TJ" kill-pane -t "$TJB" >/dev/null 2>&1
"$TMUX_REAL" -L "$TJ" new-window -d -t t: -c "$TJW" 'exec -a claude sleep 3600' >/dev/null 2>&1
TJT=$("$TMUX_REAL" -L "$TJ" list-panes -a -F '#{pane_id} #{pane_current_path}' | awk -v d="$TJW" '$2==d{print $1; exit}')
"$TMUX_REAL" -L "$TJ" set -p -t "$TJT" @panestate running          >/dev/null 2>&1
"$TMUX_REAL" -L "$TJ" set -p -t "$TJT" @cockpit_session "$TJUUID"  >/dev/null 2>&1
is "a TAB in a tool_use keeps --rows at 8 fields" "8" \
   "$(HOME="$TJH" COCKPIT_TMUX="tmux -L $TJ" "$COCKPIT_TUI_BIN" --rows \
      | awk -F'\t' -v p="$TJT" '$2==p{print NF; exit}')"
is "and the tool text survives with the TAB as a space" "Bash: col1 col2" \
   "$(HOME="$TJH" COCKPIT_TMUX="tmux -L $TJ" "$COCKPIT_TUI_BIN" --rows \
      | awk -F'\t' -v p="$TJT" '$2==p{print $5; exit}')"

# F4: the agent command list is @cockpit_agents_glob, which cockpit.tmux
# precomputes -- never a second copy compiled into the binary.
# bash, not sleep: a multicall coreutils (uutils) refuses to run as "zzz".
TJZ=$(mktmpd); ln -sf "$(command -v bash)" "$TJZ/zzz"
"$TMUX_REAL" -L "$TJ" set -g @cockpit_agents_glob '|zzz|' >/dev/null 2>&1
"$TMUX_REAL" -L "$TJ" new-window -d -t t: "exec $TJZ/zzz -c 'read -r _'" >/dev/null 2>&1
"$TMUX_REAL" -L "$TJ" new-window -d -t t: 'exec sleep 3600'    >/dev/null 2>&1
TJIDS=$(COCKPIT_TMUX="tmux -L $TJ" "$COCKPIT_TUI_BIN" --json | sed -n 's/^    "id": "\(%[0-9]*\)".*/\1/p')
TJCMDS=$(for i in $TJIDS; do "$TMUX_REAL" -L "$TJ" display-message -p -t "$i" '#{pane_current_command}'; done | sort -u | tr '\n' ' ')
is "@cockpit_agents_glob names zzz and not sleep" "claude zzz" "$(printf '%s' "$TJCMDS" | sed 's/ $//')"

# F6: "no agents" ([] , exit 0) and "no tmux" (exit 1, no stdout) are different.
TJOUT2=$(COCKPIT_TMUX="tmux -L cockpit-nosuch-$$" "$COCKPIT_TUI_BIN" --json 2>/dev/null); TJRC=$?
is "an unreachable tmux is exit 1 with empty stdout" "1 empty" \
   "$TJRC $([ -z "$TJOUT2" ] && echo empty || echo "[$TJOUT2]")"

"$TMUX_REAL" -L "$TJ" kill-server >/dev/null 2>&1
fi

echo "== prefix w: the board binds, off keeps stock, below 3.8 refuses, and one action line reaches one script =="
# Every script cockpit.tmux names has to be in the tree. The fetch assertions
# below write a stub over their copy, so a deleted scripts/tui-fetch would sail
# through them; a binding pointing at nothing fails only in the user's hands.
bmissing=
while IFS= read -r bs; do
    [ -e "$DIR/$bs" ] || bmissing="$bmissing $bs"
done < <(grep -o '[$]DIR/scripts/[a-z-]*' "$DIR/cockpit.tmux" | sed 's|[$]DIR/||' | sort -u)
is "every script cockpit.tmux names exists in the tree" "" "$bmissing"
if need_tmux "the board binding"; then :; else
BD=$(mktmpd)
mkdir -p "$BD/p/scripts" "$BD/p/bin" "$BD/rec"
cp "$DIR/cockpit.tmux" "$BD/p/"
cp -r "$DIR"/scripts/. "$BD/p/scripts/"
# Nothing in a test may reach the network: the copy's tui-fetch only counts its
# own runs, which is also how "once, in the background" is pinned.
printf '#!/bin/sh\necho ran >> %s/fetched\n' "$BD" > "$BD/p/scripts/tui-fetch"
chmod +x "$BD/p/scripts/tui-fetch"

# cockpit.tmux runs under run-shell, which inherits the SERVER's environment, so
# a shim has to be on PATH when the server STARTS, not when the key is bound.
# This one records every argv AND forwards, so one load proves both the refusal
# message and the missing binding. The -L is forced: with $TMUX unset a bare
# forward would land on the user's own server.
bshim() {                     # <dir> <socket> <version-to-report|"">
    mkdir -p "$1"
    { printf '#!/bin/sh\nprintf "%%s\\n" "$*" >> %s/argv\n' "$BD"
      [ -z "$3" ] || printf '[ "$1" = -V ] && { echo "tmux %s"; exit 0; }\n' "$3"
      printf 'exec %s -L %s "$@"\n' "$TMUX_REAL" "$2"
    } > "$1/tmux"
    chmod +x "$1/tmux"
}
bkey()  { "$TMUX_REAL" -L "$1" list-keys -T prefix 2>/dev/null | awk -v k="$2" '$4==k'; }
# Polled on the LAST binding cockpit.tmux installs -- the board block runs
# before it, so this cannot pass while the code under test is still to come.
bloaded() {
    local i
    for i in $(seq 1 40); do
        "$TMUX_REAL" -L "$1" list-keys -T root 2>/dev/null | grep -q C-MouseDown3Pane && return 0
        sleep 0.1
    done
    return 1
}

# --- the shipped default -------------------------------------------------------
B="cockpit-bw-$$"; SOCKS="$SOCKS $B"; bshim "$BD/s1" "$B" ""
# -u: the server's environment is what cockpit.tmux reads, and this copy has no binary.
env -u COCKPIT_TUI_BIN PATH="$BD/s1:$PATH" "$TMUX_REAL" -L "$B" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$B" run-shell "$BD/p/cockpit.tmux" >/dev/null 2>&1; bloaded "$B"
bw=$(bkey "$B" w)
# This copy has no binary on purpose: a broken install still binds the key,
# and the wrapper says so at press time.
case "$bw" in *cockpit-board*) ok "prefix w opens the board, with no binary installed" ;;
    *) bad "prefix w opens the board, with no binary installed" "a display-popup running cockpit-board" "$bw" ;;
esac
case "$bw" in *'"90%"'*) ok "the board popup is 90% wide, not the pickers' 70%" ;;
    *) bad "the board popup is 90% wide, not the pickers' 70%" "-w 90%" "$bw" ;;
esac
# Only the WIDTH is the board's own: a 70% popup on a 120-column terminal is
# one tile column. The height is whatever the other cockpit popups use.
case "$bw" in *'"70%"'*) ok "and its height is @cockpit_popup_height, like every other cockpit popup" ;;
    *) bad "and its height is @cockpit_popup_height, like every other cockpit popup" "-h 70%" "$bw" ;;
esac
# The client's session is not the agent's, so the title names neither (D9 step 2).
case "$bw" in *'#{session_name}'*) bad "the popup title names no session" '-T " board "' "$bw" ;;
    *'-T " board "'*) ok "the popup title names no session" ;;
    *) bad "the popup title names no session" '-T " board "' "$bw" ;;
esac
# One option drives both screens: the board's popup hands the panel's mode on.
case "$bw" in *'-e COCKPIT_PANEL_MODE=auto'*) ok "the board popup passes -e COCKPIT_PANEL_MODE" ;;
    *) bad "the board popup passes -e COCKPIT_PANEL_MODE" "-e COCKPIT_PANEL_MODE=auto" "$bw" ;;
esac
# display-popup expands only -T and -d, so the pane id cannot be a format here
# (#38): the popup's own sh resolves it.
case "$bw" in *'display-message -p'*) ok "and the binding resolves the source pane itself" ;;
    *) bad "and the binding resolves the source pane itself" "-p \$(tmux display-message -p ...)" "$bw" ;;
esac
case "$(bkey "$B" r)" in *tmux-review*) ok "prefix r still opens the fleet list without the binary" ;;
    *) bad "prefix r still opens the fleet list without the binary" "a display-popup running tmux-review" "$(bkey "$B" r)" ;;
esac
case "$(bkey "$B" a)" in *'Board of every agent'*) ok "the prefix a menu carries a board row that replays w" ;;
    *) bad "the prefix a menu carries a board row that replays w" "a 'Board of every agent' item" "$(bkey "$B" a)" ;;
esac
is "and a plain load fetches nothing" "no" "$([ -e "$BD/fetched" ] && echo yes || echo no)"
# Required since #199: the load says so, offline, and names the fix.
case "$(grep 'display-message tmux-cockpit: cockpit-tui' "$BD/argv" 2>/dev/null)" in
    *"cockpit-tui is missing; run "*/install.sh) ok "a load with no binary says it is missing and names install.sh" ;;
    *) bad "a load with no binary says it is missing and names install.sh" "cockpit-tui is missing; run .../install.sh" \
         "$(grep display-message "$BD/argv" 2>/dev/null)" ;;
esac
"$TMUX_REAL" -L "$B" kill-server >/dev/null 2>&1

# The version check: below TUI_MIN names tui-fetch; a current one says nothing.
# Registered out here: bver runs in a $(…) subshell, which would drop its SOCKS.
SOCKS="$SOCKS cockpit-bw7-0.0.1-$$ cockpit-bw7-99.0.0-$$"
bver() {                      # <version> -> the load's cockpit-tui message, if any
    local b="cockpit-bw7-$1-$$"
    bshim "$BD/s7" "$b" ""; rm -f "$BD/argv"
    printf '#!/bin/sh\necho "cockpit-tui %s"\n' "$1" > "$BD/vtui"; chmod +x "$BD/vtui"
    COCKPIT_TUI_BIN="$BD/vtui" PATH="$BD/s7:$PATH" "$TMUX_REAL" -L "$b" -f /dev/null new-session -d -s t 'sleep 300' >/dev/null 2>&1
    "$TMUX_REAL" -L "$b" run-shell "$BD/p/cockpit.tmux" >/dev/null 2>&1; bloaded "$b"
    grep 'display-message tmux-cockpit: cockpit-tui' "$BD/argv" 2>/dev/null
    "$TMUX_REAL" -L "$b" kill-server >/dev/null 2>&1
}
bmsg=$(bver 0.0.1)
case "$bmsg" in
    *"cockpit-tui 0.0.1 is older than "*"; run "*/install.sh) ok "a binary below TUI_MIN is named on load, with install.sh as the fix" ;;
    *) bad "a binary below TUI_MIN is named on load, with install.sh as the fix" "cockpit-tui 0.0.1 is older than ...; run .../install.sh" "$bmsg" ;;
esac
is "a current binary loads without a word" "" "$(bver 99.0.0)"
unset -f bver

# --- off, the parked window, and the opt-in fetch ------------------------------
# "off" cannot leave w EMPTY: stock tmux binds it to choose-tree, and not
# binding ours simply leaves that standing. Unbound-from-the-board is the
# contract, the same as @cockpit_review_key.
B2="cockpit-bw2-$$"; SOCKS="$SOCKS $B2"; bshim "$BD/s2" "$B2" ""
PATH="$BD/s2:$PATH" "$TMUX_REAL" -L "$B2" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$B2" set -g @cockpit_board_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$B2" run-shell "$BD/p/cockpit.tmux" >/dev/null 2>&1; bloaded "$B2"
case "$(bkey "$B2" w)" in *cockpit-board*) bad "@cockpit_board_key off leaves w to stock tmux" \
        "no cockpit-board in the binding" "$(bkey "$B2" w)" ;;
    *) ok "@cockpit_board_key off leaves w to stock tmux" ;;
esac
case "$(bkey "$B2" a)" in *'Board of every agent'*) bad "and the agents menu drops its row with it" \
        "no board row" "$(bkey "$B2" a)" ;;
    *) ok "and the agents menu drops its row with it" ;;
esac
"$TMUX_REAL" -L "$B2" kill-server >/dev/null 2>&1

B3="cockpit-bw3-$$"; SOCKS="$SOCKS $B3"; bshim "$BD/s3" "$B3" ""
PATH="$BD/s3:$PATH" "$TMUX_REAL" -L "$B3" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$B3" set -g @cockpit_popup_height 44% >/dev/null 2>&1
"$TMUX_REAL" -L "$B3" set -g @cockpit_tui_fetch on >/dev/null 2>&1
"$TMUX_REAL" -L "$B3" run-shell "$BD/p/cockpit.tmux" >/dev/null 2>&1; bloaded "$B3"
case "$(bkey "$B3" w)" in *'"44%"'*) ok "a set @cockpit_popup_height reaches the board too" ;;
    *) bad "a set @cockpit_popup_height reaches the board too" "-h 44%" "$(bkey "$B3" w)" ;;
esac
until_file "$BD/fetched" '*ran*'
is "@cockpit_tui_fetch on runs tui-fetch, exactly once" "1" "$(wc -l < "$BD/fetched" 2>/dev/null)"
# -b, not a plain run-shell: a foreground curl would sit on the config load,
# which is the whole reason the fetch is not on that path.
case "$(grep 'run-shell -b' "$BD/argv" 2>/dev/null)" in *tui-fetch*) ok "and backgrounds it, so a dead network cannot stall the load" ;;
    *) bad "and backgrounds it, so a dead network cannot stall the load" "run-shell -b .../tui-fetch" \
         "$(grep run-shell "$BD/argv" 2>/dev/null)" ;;
esac
"$TMUX_REAL" -L "$B3" kill-server >/dev/null 2>&1

B6="cockpit-bw6-$$"; SOCKS="$SOCKS $B6"; bshim "$BD/s6" "$B6" ""
PATH="$BD/s6:$PATH" "$TMUX_REAL" -L "$B6" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$B6" set -g @cockpit_board_window on >/dev/null 2>&1
"$TMUX_REAL" -L "$B6" run-shell "$BD/p/cockpit.tmux" >/dev/null 2>&1; bloaded "$B6"
case "$(bkey "$B6" w)" in *new-window*cockpit-board*) ok "@cockpit_board_window on parks the board in a window" ;;
    *) bad "@cockpit_board_window on parks the board in a window" "a new-window running cockpit-board" "$(bkey "$B6" w)" ;;
esac
case "$(bkey "$B6" w)" in *'-e COCKPIT_PANEL_MODE=auto'*) ok "and the window gets the panel mode too" ;;
    *) bad "and the window gets the panel mode too" "-e COCKPIT_PANEL_MODE=auto" "$(bkey "$B6" w)" ;;
esac
"$TMUX_REAL" -L "$B6" kill-server >/dev/null 2>&1

# --- the 3.8 floor -------------------------------------------------------------
B4="cockpit-bw4-$$"; SOCKS="$SOCKS $B4"; bshim "$BD/s4" "$B4" 3.7; rm -f "$BD/argv"
PATH="$BD/s4:$PATH" "$TMUX_REAL" -L "$B4" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$B4" run-shell "$BD/p/cockpit.tmux" >/dev/null 2>&1
is "tmux 3.7 binds nothing" "" "$(grep -c 'bind-key' "$BD/argv" 2>/dev/null | grep -v '^0$')"
case "$(cat "$BD/argv" 2>/dev/null)" in *'tmux-cockpit needs tmux 3.8 or newer (found 3.7)'*)
        ok "and says which tmux it needs" ;;
    *) bad "and says which tmux it needs" "needs tmux 3.8 or newer (found 3.7)" \
         "$(grep display-message "$BD/argv" 2>/dev/null)" ;;
esac
"$TMUX_REAL" -L "$B4" kill-server >/dev/null 2>&1

# --- the wrapper: one action line, one script ----------------------------------
# Every script the wrapper can reach is a stub that records its argv, and tmux
# only records: the assertion is what ran, in full, not just that the right
# thing did.
# The one read the loop makes, @cockpit_board_return, answers from $BD/ret and
# is not recorded: what is asserted is what ran.
cat > "$BD/rec/tmux" <<BDREC
#!/bin/sh
case "\$*" in "show-option -gqv @cockpit_board_return") cat "$BD/ret" 2>/dev/null; exit 0 ;; esac
printf '%s\\n' "\$*" >> "$BD/wargv"
exit 0
BDREC
chmod +x "$BD/rec/tmux"
for s in tmux-jump tmux-review cockpit-panel; do
    printf '#!/bin/sh\nprintf "%s %%s\\n" "$*" >> %s/wargv\n' "$s" "$BD" > "$BD/p/scripts/$s"
    chmod +x "$BD/p/scripts/$s"
done
# The panel from the board writes its last line where the loop asks.
printf '#!/bin/sh\nprintf "cockpit-panel %%s\\n" "$*" >> %s/wargv\n[ -z "$COCKPIT_PANEL_NOTE" ] || cat %s/pnote > "$COCKPIT_PANEL_NOTE"\n' "$BD" "$BD" > "$BD/p/scripts/cockpit-panel"
printf 'cockpit: sent to %%1\n' > "$BD/pnote"
# The line travels in a file rather than inside the fake binary's source: the
# cases below are shell syntax, and re-quoting them into a script is the test
# writing the very bug it is checking for.
# The fake binary acts once and then exits with no action, the ordinary q, so a
# verb that returns runs it exactly twice; $BD/bargv is every run's argv, and
# the note it was handed.
wact() {                      # <action-line> -> every argv the wrapper produced
    : > "$BD/wargv"; : > "$BD/bargv"; rm -f "$BD/once"; printf '%s\n' "$1" > "$BD/line"
    cat > "$BD/p/bin/cockpit-tui" <<BDBIN
#!/bin/sh
printf '%s\\n' "\$*" >> "$BD/bargv"
[ "\${5:-}" = "" ] || cat "\$5" >> "$BD/bargv"
[ -e "$BD/once" ] && exit 0
: > "$BD/once"
cat "$BD/line" > "\$COCKPIT_ACTION"
BDBIN
    chmod +x "$BD/p/bin/cockpit-tui"
    # The copy's stub, named: the suite exports its own real binary.
    PATH="$BD/rec:$PATH" COCKPIT_LOG="$BD/tui.log" COCKPIT_TUI_BIN="$BD/p/bin/cockpit-tui" "$BD/p/scripts/cockpit-board" -p %3 </dev/null >/dev/null 2>&1
    tr '\n' ';' < "$BD/wargv"
}
# jump is three tmux calls and NOT tmux-jump: that script cycles to the next
# blocked pane from where you are, and the board has already chosen the pane.
is "jump %9 goes to that pane and runs nothing else" \
   "switch-client -t %9;select-window -t %9;select-pane -t %9;" "$(wact 'jump %9')"
is "review %1 runs tmux-review on it and nothing else" "tmux-review -p %1;" "$(wact 'review %1')"
is "panel %1 runs cockpit-panel on it and nothing else" "cockpit-panel -p %1;" "$(wact 'panel %1')"
case "$(wact 'fix %1')" in
    'display-message -t %3 cockpit: the board sent "fix", which this plugin does not know; prefix U updates the plugin and cockpit-tui;') ok "fix %1, the retired verb, runs nothing and says prefix U updates the binary" ;;
    *) bad "fix %1, the retired verb, runs nothing and says prefix U updates the binary" "one display-message" "$(wact 'fix %1')" ;;
esac
case "$(tail -1 "$BD/tui.log" 2>/dev/null)" in *'cockpit-board: unknown verb [fix %1]') ok "and logs the unknown verb" ;;
    *) bad "and logs the unknown verb" "cockpit-board: unknown verb [fix %1]" "$(tail -1 "$BD/tui.log" 2>/dev/null)" ;; esac
is "info %1 runs the info screen and nothing else" "tmux-review -p %1 --info;" "$(wact 'info %1')"
case "$(wact 'deliver %1')" in *'not wired up yet'*) ok "deliver %1 is reserved and says so" ;;
    *) bad "deliver %1 is reserved and says so" "a display-message" "$(wact 'deliver %1')" ;;
esac
is "only the FIRST line of the action file is ever acted on" \
   "switch-client -t %9;select-window -t %9;select-pane -t %9;" "$(wact $'jump %9\nreview %1')"

# The return trip (D9): review, info and panel bring the board back on the pane
# they acted on; jump, q and the off switch end it.
bruns() { tr '\n' ';' < "$BD/bargv"; }
wact 'jump %9' >/dev/null
is "jump ends the loop: the board ran once" "board;" "$(bruns)"
wact 'review %1' >/dev/null
is "review %1 brings the board back with --select %1, and no note" "board;board --select %1;" "$(bruns)"
wact 'info %1' >/dev/null
is "so does info %1" "board;board --select %1;" "$(bruns)"
wact 'panel %1' >/dev/null
case "$(bruns)" in "board;board --select %1 --note "*"/note;cockpit: sent to %1;") ok "panel %1 comes back with the panel's last line as the note" ;;
    *) bad "panel %1 comes back with the panel's last line as the note" "board;board --select %1 --note .../note;cockpit: sent to %1;" "$(bruns)" ;;
esac
wact 'deliver %1' >/dev/null
is "deliver, still reserved, ends the loop" "board;" "$(bruns)"
printf 'off\n' > "$BD/ret"
is "@cockpit_board_return off: review is one-way again" "tmux-review -p %1;" "$(wact 'review %1')"
is "and the board ran once" "board;" "$(bruns)"
is "@cockpit_board_return off: panel too" "cockpit-panel -p %1;" "$(wact 'panel %1')"
is "and the board ran once again" "board;" "$(bruns)"
rm -f "$BD/ret"
# A board that fails ends the loop rather than spinning on it.
printf '#!/bin/sh\nprintf x >> %s/bfail\nexit 3\n' "$BD" > "$BD/p/bin/cockpit-tui"
: > "$BD/wargv"; rm -f "$BD/bfail"
PATH="$BD/rec:$PATH" COCKPIT_LOG="$BD/tui.log" COCKPIT_TUI_BIN="$BD/p/bin/cockpit-tui" "$BD/p/scripts/cockpit-board" -p %3 </dev/null >/dev/null 2>&1
is "a board that exits non-zero runs once and says so" "x display-message -t %3 cockpit: the board exited 3 — see $BD/tui.log;" \
   "$(cat "$BD/bfail") $(tr '\n' ';' < "$BD/wargv")"

# The binary is the one piece of this plugin that is not shell, so its one line
# of output is re-checked here rather than trusted. Empty means nothing ran.
inject() { wact "${1//@/$BD/pwn}"; [ -e "$BD/pwn" ] && printf pwned; }
is "a verb carrying a command separator runs nothing"        "" "$(inject 'jump; touch @')"
is "a target written as a command substitution runs nothing" "" "$(inject 'jump $(touch @)')"
is "a verb written as a command substitution runs nothing"   "" "$(inject '$(touch @) %9')"
is "a third field on the action line runs nothing"           "" "$(inject 'jump %9 extra')"
is "a target that is not %<digits> runs nothing"             "" "$(inject 'jump %9x')"
is "an upper-case verb runs nothing"                         "" "$(inject 'JUMP %9')"
is "a verb from a newer binary runs nothing, and says so"    "display-message -t %3 cockpit: the board sent \"frobnicate\", which this plugin does not know; prefix U updates the plugin and cockpit-tui;" "$(inject 'frobnicate %9')"
# The log is the assertion, not the argv: an ordinary q and a rejected line both
# run nothing, and only the log tells a version skew from a normal close.
is "an empty action file is the ordinary q, and logs nothing" "0" \
   "$(: > "$BD/tui.log"; wact '' >/dev/null; grep -c 'ignored action line' "$BD/tui.log" 2>/dev/null)"
is "a rejected line leaves a trace instead of a silently dead key" "1" \
   "$(: > "$BD/tui.log"; wact 'jump %9x' >/dev/null; grep -c 'ignored action line' "$BD/tui.log" 2>/dev/null)"

: > "$BD/wargv"
printf '#!/bin/sh\nrm -f "$COCKPIT_ACTION"\n' > "$BD/p/bin/cockpit-tui"; chmod +x "$BD/p/bin/cockpit-tui"
bnoise=$(PATH="$BD/rec:$PATH" COCKPIT_LOG="$BD/tui.log" COCKPIT_TUI_BIN="$BD/p/bin/cockpit-tui" "$BD/p/scripts/cockpit-board" -p %3 </dev/null 2>&1); brc=$?
is "a binary that deletes the action file exits 0, prints nothing, says nothing" "0  " \
   "$brc $bnoise $(cat "$BD/wargv" 2>/dev/null)"

rm -f "$BD/p/bin/cockpit-tui"; : > "$BD/wargv"
PATH="$BD/rec:$PATH" COCKPIT_LOG="$BD/tui.log" COCKPIT_TUI_BIN="$BD/p/bin/cockpit-tui" "$BD/p/scripts/cockpit-board" -p 'not-a-pane' </dev/null >/dev/null 2>&1
case "$(cat "$BD/wargv" 2>/dev/null)" in
    'display-message cockpit: board needs'*) ok "a -p that is not a pane id drops the -t rather than passing it on" ;;
    *) bad "a -p that is not a pane id drops the -t rather than passing it on" \
         "display-message cockpit: board needs ..." "$(cat "$BD/wargv" 2>/dev/null)" ;;
esac
: > "$BD/wargv"
PATH="$BD/rec:$PATH" COCKPIT_LOG="$BD/tui.log" COCKPIT_TUI_BIN="$BD/p/bin/cockpit-tui" "$BD/p/scripts/cockpit-board" -p %3 </dev/null >/dev/null 2>&1
brc=$?
is "a missing binary says so once, on the pane the key was pressed in, and exits 0" \
   "0 display-message -t %3 cockpit: board needs cockpit-tui: run $(cd -P "$BD/p" && pwd)/install.sh; prefix r is the fleet list" \
   "$brc $(cat "$BD/wargv" 2>/dev/null)"

# --- a plugin directory that is shell syntax -----------------------------------
# sq() quotes $DIR into the bind, so pressing the key runs the wrapper and not
# the directory's name. Pressed for real: display-popup needs a client, so an
# outer server provides the pty (the same shape as the RCE-via-directory-name
# block above).
BI=$(mktmpd); BIDIR="$BI/x'\$(touch $BI/OWNED)'y"
mkdir -p "$BIDIR"; cp -r "$BD/p/." "$BIDIR/"
printf '#!/bin/sh\nprintf "%%s\\n" "$*" > %s/args\n' "$BI" > "$BIDIR/scripts/cockpit-board"
chmod +x "$BIDIR/scripts/cockpit-board"
BW_IN="cockpit-bwi-$$"; BW_OUT="cockpit-bwo-$$"; SOCKS="$SOCKS $BW_IN $BW_OUT"
"$TMUX_REAL" -L "$BW_IN" -f /dev/null new-session -d -s s -x 100 -y 30 'exec -a claude sleep 300' >/dev/null 2>&1
# run-shell hands its string to sh, so the conf line quotes the path -- what
# this block owns is that the BINDING quotes it too.
"$TMUX_REAL" -L "$BW_IN" run-shell "$(printf "'%s'" "${BIDIR//\'/\'\\\'\'}/cockpit.tmux")" >/dev/null 2>&1
bloaded "$BW_IN"
BIP=$("$TMUX_REAL" -L "$BW_IN" list-panes -t s:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$BW_OUT" -f /dev/null new-session -d -s o -x 100 -y 32 \
    "TERM=xterm-256color exec $TMUX_REAL -L $BW_IN attach -t s" >/dev/null 2>&1
# The client is attached when the inner server sees one -- polled, not slept for.
for i in $(seq 1 40); do
    [ -n "$("$TMUX_REAL" -L "$BW_IN" list-clients -F x 2>/dev/null)" ] && break
    sleep 0.1
done
"$TMUX_REAL" -L "$BW_OUT" send-keys -t o C-b w
until_file "$BI/args" '*-p*'
is "a plugin directory named like a command substitution still reaches the wrapper" "-p $BIP" \
   "$(cat "$BI/args" 2>/dev/null)"
is "and its name never runs" "no" "$([ -e "$BI/OWNED" ] && echo yes || echo no)"
"$TMUX_REAL" -L "$BW_OUT" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$BW_IN" kill-server >/dev/null 2>&1
fi

echo "== cockpit-board: the return trip lands on the same pane, and a ✕ note stays =="
# The real binary in a real window: the loop, --select and board.state end to
# end. tmux-review is a stub that records its argv; the panel is the real one.
if need_tmux "the board's return trip"; then :; else
RB="cockpit-rb-$$"; SOCKS="$SOCKS $RB"; RBD=$(mktmpd)
mkdir -p "$RBD/p" "$RBD/state"
cp -r "$DIR"/scripts "$RBD/p/"
printf '#!/bin/sh\nprintf "%%s\\n" "$*" >> %s/rv\n' "$RBD" > "$RBD/p/scripts/tmux-review"
chmod +x "$RBD/p/scripts/tmux-review"
"$TMUX_REAL" -L "$RB" -f /dev/null new-session -d -s t -x 120 -y 40 'exec sleep 3600' >/dev/null 2>&1
RBP=()
for i in 1 2 3; do
    RBP+=("$("$TMUX_REAL" -L "$RB" new-window -d -P -F '#{pane_id}' -t t: 'exec -a claude sleep 3600')")
done
for p in "${RBP[@]}"; do
    "$TMUX_REAL" -L "$RB" set -p -t "$p" @panestate running >/dev/null 2>&1
done
# Tiles sort by state, then pane id: all running, so tile 2 is the second pane.
RBX=${RBP[1]}
RBENV="env COCKPIT_TUI_BIN=$COCKPIT_TUI_BIN XDG_STATE_HOME=$RBD/state COCKPIT_LOG=$RBD/tui.log"
rb_open() {                   # -> RBW, the wrapper's pane, with tiles drawn
    RBW=$("$TMUX_REAL" -L "$RB" new-window -d -P -F '#{pane_id}' -t t: "$RBENV $RBD/p/scripts/cockpit-board -p ${RBP[0]}")
    until_cap "$RB" "$RBW" '*board · 3 agents*'
}
rb_gone() {                   # <pane>
    local i
    for i in $(seq 1 "$POLLS"); do
        "$TMUX_REAL" -L "$RB" list-panes -a -F '#{pane_id}' 2>/dev/null | grep -qx "$1" || return 0
        sleep 0.1
    done
    return 1
}
rb_rv() { tr '\n' ';' < "$RBD/rv" 2>/dev/null; }

rb_open || bad "the board opens in a window" "board · 3 agents" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBW")"
"$TMUX_REAL" -L "$RB" send-keys -t "$RBW" 2 r
until_file "$RBD/rv" '*-p*'
is "r on tile 2 reviews its pane" "-p $RBX;" "$(rb_rv)"
# the stub returns at once: the board must come back, on that same pane
until_cap "$RB" "$RBW" '*board · 3 agents*' || bad "r returns to the board" "board · 3 agents" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBW")"
"$TMUX_REAL" -L "$RB" send-keys -t "$RBW" r
until_file "$RBD/rv" '*-p*-p*'
is "r, then the return, lands on the same pane" "-p $RBX;-p $RBX;" "$(rb_rv)"

until_cap "$RB" "$RBW" '*board · 3 agents*'
"$TMUX_REAL" -L "$RB" send-keys -t "$RBW" f
until_cap "$RB" "$RBW" '*will send*' || bad "f opens the panel in the same window" "will send" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBW")"
"$TMUX_REAL" -L "$RB" send-keys -t "$RBW" Escape
until_cap "$RB" "$RBW" '*board · 3 agents*' || bad "esc on the panel's Home returns to the board" "board · 3 agents" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBW")"
"$TMUX_REAL" -L "$RB" send-keys -t "$RBW" r
until_file "$RBD/rv" '*-p*-p*-p*'
is "f, then esc on Home, returns on the same pane" "-p $RBX;-p $RBX;-p $RBX;" "$(rb_rv)"

until_cap "$RB" "$RBW" '*board · 3 agents*'
"$TMUX_REAL" -L "$RB" send-keys -t "$RBW" Enter
rb_gone "$RBW" || bad "jump ends the loop" "the window closes" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBW")"
is "jump ends the loop, on the pane it jumped to" "$RBX" "$("$TMUX_REAL" -L "$RB" display -p -t t '#{pane_id}')"
is "board.state keeps the selection" "cockpit-board-state 1;sel $RBX;" \
   "$(tr '\n' ';' < "$RBD/state/tmux-cockpit/board.state" 2>/dev/null)"
is "created 0600" "600" "$(stat -c %a "$RBD/state/tmux-cockpit/board.state" 2>/dev/null)"

# The next plain prefix w opens on it too; @cockpit_board_return off makes r one-way.
"$TMUX_REAL" -L "$RB" set -g @cockpit_board_return off >/dev/null 2>&1
rb_open
"$TMUX_REAL" -L "$RB" send-keys -t "$RBW" r
until_file "$RBD/rv" '*-p*-p*-p*-p*'
is "a fresh board opens on the saved pane" "-p $RBX" "$(tail -1 "$RBD/rv")"
rb_gone "$RBW" || bad "@cockpit_board_return off keeps r one-way" "the window closes" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBW")"
ok "@cockpit_board_return off keeps r one-way"
"$TMUX_REAL" -L "$RB" set -gu @cockpit_board_return >/dev/null 2>&1

# D20: two boards opened together, one with a success note, one with a ✕ note.
# Once the success note has faded (5 s), the ✕ one must still be there.
printf 'cockpit: sent to %s\n' "$RBX" > "$RBD/ok"
printf '✕ cockpit: the pane now runs bash, not claude; nothing sent\n' > "$RBD/no"
# A pane that is no agent: whatever a board would write over it is a real one.
printf 'cockpit-board-state 1\nsel %%0\n' > "$RBD/state/tmux-cockpit/board.state"
rb_bin() { "$TMUX_REAL" -L "$RB" new-window -d -P -F '#{pane_id}' -t t: "$RBENV COCKPIT_READONLY=1 $COCKPIT_TUI_BIN board --note $1"; }
RBOK=$(rb_bin "$RBD/ok"); RBNO=$(rb_bin "$RBD/no")
until_cap "$RB" "$RBOK" '*cockpit: sent to*' || bad "a success note shows" "cockpit: sent to" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBOK")"
until_cap "$RB" "$RBNO" '*nothing sent · esc dismiss*' || bad "a ✕ note shows with its way out" "· esc dismiss" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBNO")"
rbfaded=
for i in $(seq 1 $((POLLS * 2))); do
    case "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBOK" 2>/dev/null)" in
        *'cockpit: sent to'*) sleep 0.1 ;;
        *'↵ jump'*) rbfaded=1; break ;;
        *) sleep 0.1 ;;
    esac
done
is "the success note fades" "1" "$rbfaded"
case "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBNO")" in
    *'nothing sent · esc dismiss'*) ok "a ✕ note survives the ticks that fade a success note" ;;
    *) bad "a ✕ note survives the ticks that fade a success note" "the ✕ note" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBNO")" ;;
esac
"$TMUX_REAL" -L "$RB" send-keys -t "$RBNO" Escape
until_cap "$RB" "$RBNO" '*↵ jump*' || bad "esc only dismisses the ✕ note" "the key bar" "$("$TMUX_REAL" -L "$RB" capture-pane -p -t "$RBNO")"
is "and leaves the board open" "$RBNO" "$("$TMUX_REAL" -L "$RB" list-panes -a -F '#{pane_id}' | grep -x "$RBNO")"
"$TMUX_REAL" -L "$RB" send-keys -t "$RBOK" q
rb_gone "$RBOK"
is "COCKPIT_READONLY writes no board.state" "sel %0" "$(tail -1 "$RBD/state/tmux-cockpit/board.state")"
"$TMUX_REAL" -L "$RB" send-keys -t "$RBNO" q
"$TMUX_REAL" -L "$RB" kill-server >/dev/null 2>&1
fi

echo "== cockpit-board: folds survive a reopen (board.state fold lines) =="
# Step 3 of the board design: the list folds by tmux session, and the folds
# persist in board.state, so the next board opens folded the same way.
if need_tmux "the board's folds"; then :; else
BF="cockpit-bf-$$"; SOCKS="$SOCKS $BF"; BFD=$(mktmpd)
"$TMUX_REAL" -L "$BF" -f /dev/null new-session -d -s alpha -x 100 -y 30 'exec sleep 3600' >/dev/null 2>&1
"$TMUX_REAL" -L "$BF" new-session -d -s beta 'exec sleep 3600' >/dev/null 2>&1
for s in alpha beta; do
    bfp=$("$TMUX_REAL" -L "$BF" new-window -d -P -F '#{pane_id}' -t "$s:" 'exec -a claude sleep 3600')
    "$TMUX_REAL" -L "$BF" set -p -t "$bfp" @panestate running >/dev/null 2>&1
done
bf_open() {                   # -> BFW, a board in its own window, its list drawn
    BFW=$("$TMUX_REAL" -L "$BF" new-window -d -P -F '#{pane_id}' -t alpha: \
        "env XDG_STATE_HOME=$BFD COCKPIT_LOG=$BFD/tui.log $COCKPIT_TUI_BIN board")
    until_cap "$BF" "$BFW" '*2 sessions*'
}
bf_quit() {                   # q, then wait for the window to go
    local i
    "$TMUX_REAL" -L "$BF" send-keys -t "$BFW" q
    for i in $(seq 1 "$POLLS"); do
        "$TMUX_REAL" -L "$BF" list-panes -a -F '#{pane_id}' 2>/dev/null | grep -qx "$BFW" || return 0
        sleep 0.1
    done
    return 1
}
bf_state() { tr '\n' ';' < "$BFD/tmux-cockpit/board.state" 2>/dev/null | sed 's/sel %[0-9]*;//'; }
bf_open || bad "the board opens on the list" "2 sessions" "$("$TMUX_REAL" -L "$BF" capture-pane -p -t "$BFW")"
"$TMUX_REAL" -L "$BF" send-keys -t "$BFW" -- -
until_cap "$BF" "$BFW" '*▸ beta*' || bad "- folds every group" "▸ beta" "$("$TMUX_REAL" -L "$BF" capture-pane -p -t "$BFW")"
bf_quit
is "board.state keeps one fold line per session" "cockpit-board-state 1;fold alpha;fold beta;" "$(bf_state)"
bf_open
case "$("$TMUX_REAL" -L "$BF" capture-pane -p -t "$BFW")" in
    *'▸ alpha'*'1 folded'*'▸ beta'*'1 folded'*) ok "a reopened board is folded the same way" ;;
    *) bad "a reopened board is folded the same way" "▸ alpha ... ▸ beta, each 1 folded" "$("$TMUX_REAL" -L "$BF" capture-pane -p -t "$BFW")" ;;
esac
"$TMUX_REAL" -L "$BF" send-keys -t "$BFW" =
until_cap "$BF" "$BFW" '*▾ beta*' || bad "= unfolds every group" "▾ beta" "$("$TMUX_REAL" -L "$BF" capture-pane -p -t "$BFW")"
bf_quit
is "and the unfolded board saves no fold line" "cockpit-board-state 1;" "$(bf_state)"
"$TMUX_REAL" -L "$BF" kill-server >/dev/null 2>&1
fi

echo "== cockpit-panel: insert pastes without Enter, send adds it, and the text arrives clean =="
# The panel is the one wrapper that writes into a pane, so it is driven for real:
# a fake claude, a target running cat, the panel in a second window, send-keys.
if need_tmux "cockpit-panel end to end"; then :; else
PN="cockpit-pn-$$"; SOCKS="$SOCKS $PN"; PND=$(mktmpd)
mkdir -p "$PND/bin" "$PND/cache" "$PND/cfg"
# With $PND/hang present the fake records its pid and parent, then never answers.
cat > "$PND/bin/claude" <<PNFAKE
#!/bin/sh
cat >/dev/null
[ -e "$PND/hang" ] && { echo \$\$ > "$PND/cpid"; echo \$PPID > "$PND/ppid"; exec sleep 300; }
cat "$PND/reply"
PNFAKE
chmod +x "$PND/bin/claude"
"$TMUX_REAL" -L "$PN" -f /dev/null new-session -d -s t -x 120 -y 40 'exec sleep 300' >/dev/null 2>&1
# The fake is found on PATH, as the real one would be; COCKPIT_CLAUDE would beat it.
PNENV="env -u COCKPIT_CLAUDE 'PATH=$PND/bin:$SHIM:/usr/bin:/bin' XDG_CACHE_HOME=$PND/cache XDG_CONFIG_HOME=$PND/cfg COCKPIT_LOG=$PND/tui.log"
pn_gone() {                   # <pane>  the wrapper has exited and its window with it
    local i
    for i in $(seq 1 "$POLLS"); do
        "$TMUX_REAL" -L "$PN" list-panes -a -F '#{pane_id}' 2>/dev/null | grep -qx "$1" || return 0
        sleep 0.1
    done
    return 1
}
pn_go() {                     # <target-cmd> <draft>  -> PNA target, PNB panel, at the result view
    PNA=$("$TMUX_REAL" -L "$PN" new-window -d -P -F '#{pane_id}' -t t: "$1")
    PNB=$("$TMUX_REAL" -L "$PN" new-window -d -P -F '#{pane_id}' -t t: "$PNENV $DIR/scripts/cockpit-panel -p $PNA")
    until_cap "$PN" "$PNB" '*will send*' || return 1
    "$TMUX_REAL" -L "$PN" send-keys -t "$PNB" 1
    "$TMUX_REAL" -L "$PN" send-keys -t "$PNB" -l "$2"
    "$TMUX_REAL" -L "$PN" send-keys -t "$PNB" C-s
    until_cap "$PN" "$PNB" '*i insert*'
}
pn_count() { "$TMUX_REAL" -L "$PN" capture-pane -p -t "$1" | grep -cxF "$2"; }
pn_cap() { "$TMUX_REAL" -L "$PN" capture-pane -p -t "$PNB" 2>/dev/null; }

printf '<text>panel says hi</text>' > "$PND/reply"
pn_go 'exec cat' 'say hi' || bad "the panel reaches its result view" "i insert" "$("$TMUX_REAL" -L "$PN" capture-pane -p -t "$PNB")"
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" i
pn_gone "$PNB"
until_cap "$PN" "$PNA" '*panel says hi*'
# cat echoes a pasted line once and prints it again only after Enter.
is "i inserts the answer into the target pane without Enter" "1" "$(pn_count "$PNA" 'panel says hi')"

pn_go 'exec cat' 'say hi' || bad "the panel reaches its result view (send)" "i insert" "$(pn_cap)"
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" s
until_cap "$PN" "$PNB" '*confirm*'
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" Enter
pn_gone "$PNB"
until_cap "$PN" "$PNA" '*panel says hi*panel says hi*'
is "s, after its confirm, sends the answer and presses Enter" "2" "$(pn_count "$PNA" 'panel says hi')"

# A target that turns bracketed paste on shows the markers through the tty's
# echo, so one paste carrying both lines is visible as one 200~ ... 201~ pair.
printf '<text>line one\nline two</text>' > "$PND/reply"
pn_go "printf '\\033[?2004h'; exec cat" 'two lines' || bad "the panel reaches its result view (two lines)" "i insert" "$(pn_cap)"
# cat is not a program known to take a paste whole, so Insert confirms first.
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" i
until_cap "$PN" "$PNB" '*may run each line*confirm*' || bad "a multi-line insert into cat asks first" "may run each line" "$(pn_cap)"
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" Enter
pn_gone "$PNB"
if until_cap "$PN" "$PNA" '*200~line one*line two*201~*'; then
    ok "a multi-line answer arrives as one bracketed paste, its lines in order"
else
    bad "a multi-line answer arrives as one bracketed paste, its lines in order" \
        "^[[200~line one ... line two^[[201~" "$("$TMUX_REAL" -L "$PN" capture-pane -p -t "$PNA")"
fi

printf '<text>ab\033[31mcd\007ef</text>' > "$PND/reply"
pn_go 'exec cat' 'colour' || bad "the panel reaches its result view (colour)" "i insert" "$(pn_cap)"
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" i
pn_gone "$PNB"
until_cap "$PN" "$PNA" '*cdef*'
# An ESC that got through would echo as ^[ and the BEL would vanish silently.
is "control characters never reach the pane" "ab[31mcdef" \
   "$("$TMUX_REAL" -L "$PN" capture-pane -p -t "$PNA" | grep -F cdef)"
is "and the paste buffer is gone afterwards" "" \
   "$("$TMUX_REAL" -L "$PN" list-buffers -F '#{buffer_name}' 2>/dev/null | grep cockpit-panel)"
is "the panel never publishes the board's pid" "" "$("$TMUX_REAL" -L "$PN" show-option -gqv @cockpit_tui_pid)"

# A popup torn down mid-call (SIGTERM, SIGHUP as its pty goes) or a binary killed
# outright (SIGKILL) must not leave its claude running. Pids are checked first.
pn_hang() {                   # <signal>  -> the claude pid, or empty; kills the panel with <signal>
    rm -f "$PND/cpid" "$PND/ppid"; touch "$PND/hang"
    PNA=$("$TMUX_REAL" -L "$PN" new-window -d -P -F '#{pane_id}' -t t: 'exec cat')
    PNB=$("$TMUX_REAL" -L "$PN" new-window -d -P -F '#{pane_id}' -t t: "$PNENV $DIR/scripts/cockpit-panel -p $PNA")
    until_cap "$PN" "$PNB" '*will send*' || return 1
    "$TMUX_REAL" -L "$PN" send-keys -t "$PNB" 1
    "$TMUX_REAL" -L "$PN" send-keys -t "$PNB" -l 'hang'
    "$TMUX_REAL" -L "$PN" send-keys -t "$PNB" C-s
    until_file "$PND/ppid" '[0-9]*' || return 1
    local tui comm=''
    tui=$(cat "$PND/ppid")
    [ -r "/proc/$tui/comm" ] && read -r comm 2>/dev/null < "/proc/$tui/comm"
    [ "$comm" = cockpit-tui ] || return 1
    kill "-$1" "$tui"
    cat "$PND/cpid"
}
pn_dead() {                   # <pid>  gone, or a zombie nobody waits for
    local i st
    for i in $(seq 1 "$POLLS"); do
        st=$(sed 's/^.*) //' "/proc/$1/stat" 2>/dev/null | cut -c1)
        case "$st" in ''|Z) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
if [ ! -d /proc/self ]; then
    skip_ "a killed panel takes its claude with it (no /proc to check pids by name)"
else
    for sig in TERM KILL HUP; do
        pc=$(pn_hang "$sig")
        if [ -z "$pc" ]; then
            bad "SIG$sig on the panel kills its claude" "a running fake claude under cockpit-tui" "none"
        elif pn_dead "$pc"; then
            ok "SIG$sig on the panel kills its claude"
        else
            bad "SIG$sig on the panel kills its claude" "pid $pc gone" "still running"; kill "$pc" 2>/dev/null
        fi
        pn_gone "$PNB"
    done
    is "and a killed panel pastes nothing" "0" "$(pn_count "$PNA" 'hang')"
fi
rm -f "$PND/hang"

# The answer is held while you read it, and the pane can move on meanwhile.
printf '<text>panel says hi</text>' > "$PND/reply"
pn_go "sh -c 'cat; exec sleep 300'" 'say hi' || bad "the panel reaches its result view (pane moves on)" "i insert" "$(pn_cap)"
"$TMUX_REAL" -L "$PN" send-keys -t "$PNA" C-d
for i in $(seq 1 "$POLLS"); do
    [ "$("$TMUX_REAL" -L "$PN" display -p -t "$PNA" '#{pane_current_command}')" = sleep ] && break
    sleep 0.1
done
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" i
pn_gone "$PNB"
is "a pane that stopped running what was captured gets nothing" "0" "$(pn_count "$PNA" 'panel says hi')"
case "$(cat "$PND/tui.log" 2>/dev/null)" in *"now runs [sleep], not [sh]; nothing sent"*) ok "and the log says why" ;;
    *) bad "and the log says why" "now runs [sleep], not [sh]" "$(tail -3 "$PND/tui.log" 2>/dev/null)" ;;
esac

# Steps stay open: the wrapper runs each action and reopens the panel on them.
printf '<steps><step><note>one</note><cmd>echo first</cmd></step><step><note>two</note><cmd>echo second</cmd></step></steps>' > "$PND/reply"
pn_go 'exec cat' 'two steps' || bad "the panel reaches its steps" "i insert" "$(pn_cap)"
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" i
until_cap "$PN" "$PNA" '*echo first*'
if until_cap "$PN" "$PNB" '*step 1 inserted*1 one*inserted*'; then
    ok "after an insert the panel comes back on the steps, the step marked inserted"
else
    bad "after an insert the panel comes back on the steps, the step marked inserted" "step 1 inserted ... inserted" "$(pn_cap)"
fi
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" s
until_cap "$PN" "$PNB" '*confirm*'
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" Enter
# cat holds the inserted line, so the sent step lands after it and Enter echoes both
until_cap "$PN" "$PNA" '*echo firstecho second*echo firstecho second*'
is "the next step, sent from the reopened panel, lands once with its Enter" "2" "$(pn_count "$PNA" 'echo firstecho second')"
until_cap "$PN" "$PNB" '*step 2 sent*' && ok "and it comes back again, saying so" || bad "and it comes back again, saying so" "step 2 sent" "$(pn_cap)"
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" Escape
until_cap "$PN" "$PNB" '*will send*'
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" q
if pn_gone "$PNB"; then ok "leaving without an action ends the loop"; else bad "leaving without an action ends the loop" "the popup gone" "$(pn_cap)"; fi

# Keys typed while the wrapper pastes were aimed at a panel not yet on screen:
# a reflex `s` Enter must not run the next step unseen.
printf '<steps><step><note>one</note><cmd>echo uno</cmd></step><step><note>two</note><cmd>echo dos</cmd></step></steps>' > "$PND/reply"
pn_go 'exec cat' 'two more' || bad "the panel reaches its steps (type-ahead)" "i insert" "$(pn_cap)"
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" s
until_cap "$PN" "$PNB" '*confirm*'
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" Enter
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" s
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" Enter
until_cap "$PN" "$PNB" '*step 1 sent*' || bad "the panel comes back after the send (type-ahead)" "step 1 sent" "$(pn_cap)"
# a sent line shows twice in cat: the echo, then its output
is "keys typed during the handover are dropped, so step 2 never runs" "0:2" \
   "$(pn_count "$PNA" 'echo dos'):$(pn_count "$PNA" 'echo uno')"
case "$(pn_cap)" in *confirm*) bad "and no confirm opened from them" "the steps" "$(pn_cap)" ;; *) ok "and no confirm opened from them" ;; esac
"$TMUX_REAL" -L "$PN" send-keys -t "$PNB" q
pn_gone "$PNB"

# The binding: f by default, and the wrapper with the pressed pane.
"$TMUX_REAL" -L "$PN" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$PN" list-keys -T prefix f 2>/dev/null)" in
    *display-popup*cockpit-panel*' -p '*) ok "@cockpit_panel_key unset binds the panel to f" ;;
    *) bad "@cockpit_panel_key unset binds the panel to f" "display-popup ... cockpit-panel -p" \
         "$("$TMUX_REAL" -L "$PN" list-keys -T prefix f 2>/dev/null)" ;;
esac
"$TMUX_REAL" -L "$PN" set -g @cockpit_panel_key F9 >/dev/null 2>&1
"$TMUX_REAL" -L "$PN" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$PN" list-keys -T prefix F9 2>/dev/null)" in
    *display-popup*'-h "90%" -w "94%"'*cockpit-panel*' -p '*) ok "@cockpit_panel_key F9 opens the panel in a popup on the pressed pane" ;;
    *) bad "@cockpit_panel_key F9 opens the panel in a popup on the pressed pane" "display-popup ... cockpit-panel -p" \
         "$("$TMUX_REAL" -L "$PN" list-keys -T prefix F9 2>/dev/null)" ;;
esac
is "moving the key takes f back" "0" \
   "$("$TMUX_REAL" -L "$PN" list-keys -T prefix f 2>/dev/null | grep -c cockpit-panel)"
"$TMUX_REAL" -L "$PN" set -g @cockpit_panel_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$PN" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "turning the key off on a reload takes the old binding back" "0:" \
   "$("$TMUX_REAL" -L "$PN" list-keys -T prefix F9 2>/dev/null | grep -c cockpit-panel):$("$TMUX_REAL" -L "$PN" show-option -gqv @cockpit_panel_bound)"
pn_keys() { "$TMUX_REAL" -L "$PN" list-keys -T prefix "$1" 2>/dev/null; }
pn_load() { "$TMUX_REAL" -L "$PN" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; }
pn_stock_f() {                # <label>  f is tmux's own find-window prompt again
    case "$(pn_keys f)" in *cockpit*) bad "$1" "command-prompt find-window" "$(pn_keys f)" ;;
        *'command-prompt'*'find-window -Z'*) ok "$1" ;; *) bad "$1" "command-prompt find-window" "$(pn_keys f)" ;; esac
}
"$TMUX_REAL" -L "$PN" set -g @cockpit_panel_key f >/dev/null 2>&1; pn_load
"$TMUX_REAL" -L "$PN" set -g @cockpit_panel_key off >/dev/null 2>&1; pn_load
pn_stock_f "turning the panel off on f gives f back to stock find-window"
# Bindings the retired side fix left on a server that only reloads.
"$TMUX_REAL" -L "$PN" bind-key -T prefix g display-popup -E "/old/scripts/tmux-fixer -p %1" >/dev/null 2>&1
"$TMUX_REAL" -L "$PN" bind-key -T prefix f display-popup -E "/old/scripts/tmux-fixer -p %1" >/dev/null 2>&1
pn_load
# give_back restores tmux's own binding: g is stock 3.8's switch-client -T move.
case "$(pn_keys g)" in *tmux-fixer*) bad "a reload drops a dead tmux-fixer binding on another key, for stock" "switch-client -T move" "$(pn_keys g)" ;;
    *'switch-client -T move'*) ok "a reload drops a dead tmux-fixer binding on another key, for stock" ;;
    *) bad "a reload drops a dead tmux-fixer binding on another key, for stock" "switch-client -T move" "$(pn_keys g)" ;; esac
pn_stock_f "and gives f back to stock find-window"
# @cockpit_fixer_key off was how a user kept stock f; the panel honours it.
"$TMUX_REAL" -L "$PN" set -gu @cockpit_panel_key >/dev/null 2>&1
"$TMUX_REAL" -L "$PN" set -g @cockpit_fixer_key off >/dev/null 2>&1; pn_load
pn_stock_f "@cockpit_fixer_key off with no panel key keeps stock f"
is "and binds no panel key" "" "$("$TMUX_REAL" -L "$PN" list-keys -T prefix 2>/dev/null | grep cockpit-panel)"
"$TMUX_REAL" -L "$PN" set -g @cockpit_panel_key f >/dev/null 2>&1; pn_load
is "an explicit @cockpit_panel_key still wins over it" "1" "$(pn_keys f | grep -c cockpit-panel)"
pn_mode() { pn_keys f | grep -o 'COCKPIT_PANEL_MODE=[a-z]*'; }
is "@cockpit_panel_mode unset hands the panel auto" "COCKPIT_PANEL_MODE=auto" "$(pn_mode)"
"$TMUX_REAL" -L "$PN" set -g @cockpit_panel_mode tablet >/dev/null 2>&1; pn_load
is "@cockpit_panel_mode tablet reaches the popup's environment" "COCKPIT_PANEL_MODE=tablet" "$(pn_mode)"
"$TMUX_REAL" -L "$PN" set -g @cockpit_panel_mode Tablet >/dev/null 2>&1; pn_load
is "a word that is not auto, pc or tablet binds auto" "COCKPIT_PANEL_MODE=auto" "$(pn_mode)"
"$TMUX_REAL" -L "$PN" set -gu @cockpit_panel_mode >/dev/null 2>&1
unset -f pn_keys pn_load pn_stock_f pn_mode
"$TMUX_REAL" -L "$PN" kill-server >/dev/null 2>&1

# The wrapper's grammar, with a stub binary and a tmux that only records.
mkdir -p "$PND/p/scripts" "$PND/p/bin" "$PND/rec"
cp "$DIR/scripts/cockpit-panel" "$PND/p/scripts/"
cp -r "$DIR/scripts/lib" "$PND/p/scripts/"
# `display -p` answers what the pane runs now, from a file the case sets.
cat > "$PND/rec/tmux" <<PNREC
#!/bin/sh
[ "\$1" = display ] && { cat "$PND/now"; exit 0; }
# capture-pane counts its calls; "still" mode answers the same screen each time.
if [ "\$1" = capture-pane ]; then
    echo x >> "$PND/caps"
    [ -e "$PND/still" ] && { echo same; exit 0; }
    wc -l < "$PND/caps"; exit 0
fi
printf '%s\n' "\$*" >> "$PND/pargv"
# With $PND/fail-<cmd> present that command fails the way a gone server does.
[ -e "$PND/fail-\$1" ] && { echo "boom \$1" >&2; exit 1; }
[ "\$1" = load-buffer ] && { cat; echo; } >> "$PND/pargv"
exit 0
PNREC
# The stub plays an attacker too: a .clean planted beside the action file must
# never be what gets pasted.
cat > "$PND/p/bin/cockpit-tui" <<PNSTUB
#!/bin/sh
dir=\${COCKPIT_ACTION%/*}
{ stat -c %a "\$dir" 2>/dev/null || stat -f %Lp "\$dir"; } > "$PND/dmode"
printf evil > "\$COCKPIT_ACTION.clean"
cat "$PND/line" > "\$COCKPIT_ACTION"
[ -e "$PND/text" ] && cat "$PND/text" > "\$COCKPIT_ACTION.text"
cat "$PND/cmd" > "\$COCKPIT_ACTION.cmd"
printf '%s\n' "\$*" >> "$PND/bargv"
[ -e "$PND/crash" ] && exit 3
# A state file only on the first run: the resumed one leaves without a way back.
[ -e "$PND/state" ] && [ "\${3:-}" != --resume ] && printf 'S\n' > "\$COCKPIT_ACTION.state"
exit 0
PNSTUB
chmod +x "$PND/rec/tmux" "$PND/p/bin/cockpit-tui"
pact() {                      # <action-line> <text> [notext] -> every tmux argv the wrapper produced
    : > "$PND/pargv"; printf '%s\n' "$1" > "$PND/line"; printf '%s' "$2" > "$PND/text"
    [ "${3:-}" = notext ] && rm -f "$PND/text"
    printf '%s\n' "${4:-zsh}" > "$PND/cmd"; printf '%s\n' "${5:-zsh}" > "$PND/now"
    PATH="$PND/rec:$PATH" COCKPIT_LOG="$PND/tui.log" COCKPIT_TUI_BIN="$PND/p/bin/cockpit-tui" "$PND/p/scripts/cockpit-panel" -p %5 </dev/null >/dev/null 2>&1
    tr '\n' ';' < "$PND/pargv" | sed "s/cockpit-panel-[0-9]*/B/g"
}
is "send %5 loads the text, pastes it bracketed, deletes the buffer, presses Enter, says so" \
   "load-buffer -b B -;hi;paste-buffer -p -d -b B -t %5;send-keys -t %5 Enter;display-message -t %5 cockpit: sent to %5;" \
   "$(pact 'send %5' hi)"
is "paste %5 is the same without Enter, and says how much went in" \
   "load-buffer -b B -;hi;paste-buffer -p -d -b B -t %5;display-message -t %5 cockpit: inserted 2 chars into %5;" \
   "$(pact 'paste %5' hi)"
: > "$PND/tui.log"; touch "$PND/fail-load-buffer"
is "a failed load-buffer pastes nothing and says so" \
   "load-buffer -b B -;display-message -t %5 cockpit: could not load the paste buffer; nothing sent to %5;" \
   "$(pact 'paste %5' hi)"
rm -f "$PND/fail-load-buffer"
is "and logs tmux's error" "1" "$(grep -c 'load-buffer failed for %5: boom load-buffer' "$PND/tui.log")"
: > "$PND/tui.log"; touch "$PND/fail-paste-buffer"
is "a failed paste-buffer drops the buffer, sends no Enter and says so" \
   "load-buffer -b B -;hi;paste-buffer -p -d -b B -t %5;delete-buffer -b B;display-message -t %5 cockpit: could not paste into %5; nothing sent;" \
   "$(pact 'send %5' hi)"
rm -f "$PND/fail-paste-buffer"
is "and logs tmux's error" "1" "$(grep -c 'paste-buffer failed for %5: boom paste-buffer' "$PND/tui.log")"
is "an empty text pastes nothing" "" "$(pact 'paste %5' '')"
is "a line for another pane than the one opened on pastes nothing" "" "$(pact 'paste %9' hi)"
is "a board verb pastes nothing" "" "$(pact 'jump %5' hi)"
is "a third field pastes nothing" "" "$(pact 'paste %5 x' hi)"
is "a file planted beside the action file is never pasted" "" "$(pact 'paste %5' '')"
is "and the files live in a private directory" "700" "$(pact 'paste %5' hi >/dev/null; cat "$PND/dmode")"
is "a missing text file pastes nothing" "" "$(pact 'send %5' hi notext)"
is "a pane that runs something else by now gets nothing, and is told why" \
   "display-message -t %5 cockpit: the pane now runs zsh, not claude; nothing sent;" "$(pact 'send %5' hi '' claude zsh)"
# From the board the refusal is its note, which must stay one line: the board
# refuses a two-line note, and a pane's command can carry a newline.
: > "$PND/note"; COCKPIT_PANEL_NOTE="$PND/note" pact 'send %5' hi '' claude $'zsh\nevil' >/dev/null
is "a refusal from the board is one ✕ line in the note, never a status message" \
   "1 ✕ cockpit: the pane now runs zsh evil, not claude; nothing sent" "$(wc -l < "$PND/note" | tr -d ' ') $(cat "$PND/note")"
rm -f "$PND/caps"; pact 'send %5' hi >/dev/null
is "send waits for the paste to show before Enter: one capture before, one after" "2" "$(wc -l < "$PND/caps" | tr -d ' ')"
rm -f "$PND/caps"; touch "$PND/still"; pact 'send %5' hi >/dev/null; rm -f "$PND/still"
is "and gives up after 20 polls, then still presses Enter" "21 Enter" \
   "$(wc -l < "$PND/caps" | tr -d ' ') $(grep -c 'send-keys -t %5 Enter' "$PND/pargv" | sed 's/^1$/Enter/')"
: > "$PND/tui.log"
is "COCKPIT_READONLY runs the panel and sends tmux nothing, not even a message" "" "$(COCKPIT_READONLY=1 pact 'send %5' hi)"
is "and logs what it would have done" "1" "$(grep -c 'readonly, not sent: send %5' "$PND/tui.log")"
is "a text that starts with - is pasted as text, not read as a flag" \
   "load-buffer -b B -;-n hi;paste-buffer -p -d -b B -t %5;display-message -t %5 cockpit: inserted 5 chars into %5;" \
   "$(pact 'paste %5' '-n hi')"
rm -f "$PND/bargv"; pact 'paste %5' hi >/dev/null
is "with no state file the binary runs once" "panel %5" "$(cat "$PND/bargv")"
touch "$PND/state"; rm -f "$PND/bargv"
is "a state file beside the action reruns the binary on it, after the paste" \
   "load-buffer -b B -;hi;paste-buffer -p -d -b B -t %5;display-message -t %5 cockpit: inserted 2 chars into %5;load-buffer -b B -;hi;paste-buffer -p -d -b B -t %5;display-message -t %5 cockpit: inserted 2 chars into %5;" \
   "$(pact 'paste %5' hi)"
is "and tells it the paste went in" "panel %5;panel %5 --resume */resume --last ok;" \
   "$(tr '\n' ';' < "$PND/bargv" | sed 's| --resume [^ ]*/resume| --resume */resume|')"
rm -f "$PND/bargv"; pact 'send %5' hi '' claude zsh >/dev/null
is "or why nothing was sent" "panel %5 --resume */resume --last failed:the pane now runs zsh, not claude" \
   "$(sed -n 2p "$PND/bargv" | sed 's| --resume [^ ]*/resume| --resume */resume|')"
rm -f "$PND/bargv"; COCKPIT_READONLY=1 pact 'send %5' hi >/dev/null
is "COCKPIT_READONLY reruns it too, saying nothing was sent" "panel %5 --resume */resume --last failed:COCKPIT_READONLY is set; nothing sent" \
   "$(sed -n 2p "$PND/bargv" | sed 's| --resume [^ ]*/resume| --resume */resume|')"
rm -f "$PND/state"
touch "$PND/crash"; : > "$PND/tui.log"
is "a panel that exits non-zero is reported on the pane" "display-message -t %5 cockpit: the panel exited 3 — see $PND/tui.log;" \
   "$(pact 'paste %5' hi)"
is "and under COCKPIT_READONLY only logged" "" "$(COCKPIT_READONLY=1 pact 'paste %5' hi)"
is "the log says what it would have said" "1" "$(grep -c 'readonly, not said: cockpit: the panel exited 3' "$PND/tui.log")"
rm -f "$PND/crash"
fi

echo "== cockpit-panel: a dead tmux server takes the panel and the board with it =="
# crossterm rereads a hung-up tty forever, so a panel whose server died spun at
# 80% CPU and shrugged off SIGTERM. Pids are named before anything acts on them.
if [ ! -d /proc/self ]; then
    skip_ "a dead server ends the panel (no /proc to check pids by name)"
elif need_tmux "a dead server ends the panel"; then :; else
for oscreen in panel board; do
    OR="cockpit-orph-$oscreen-$$"; SOCKS="$SOCKS $OR"
    "$TMUX_REAL" -L "$OR" -f /dev/null new-session -d -s t -x 100 -y 30 'exec cat' >/dev/null 2>&1
    ora=$("$TMUX_REAL" -L "$OR" list-panes -F '#{pane_id}' | head -1)
    oarg=$ora; [ "$oscreen" = board ] && oarg=
    orb=$("$TMUX_REAL" -L "$OR" new-window -d -P -F '#{pane_id}' -t t: \
        "env COCKPIT_READONLY=1 COCKPIT_ACTION=/nonexistent/act $COCKPIT_TUI_BIN $oscreen $oarg")
    until_cap "$OR" "$orb" '*[a-z]*' || bad "the $oscreen draws before its server dies" "a frame" "$("$TMUX_REAL" -L "$OR" capture-pane -p -t "$orb")"
    orp=$("$TMUX_REAL" -L "$OR" display -p -t "$orb" '#{pane_pid}')
    ocomm=''
    [ -r "/proc/$orp/comm" ] && read -r ocomm 2>/dev/null < "/proc/$orp/comm"
    if [ "$ocomm" != cockpit-tui ]; then
        bad "the $oscreen runs as the pane's own process" "cockpit-tui" "${ocomm:-gone}"
        continue
    fi
    "$TMUX_REAL" -L "$OR" kill-server >/dev/null 2>&1
    if pn_dead "$orp"; then
        ok "the $oscreen exits once its tmux server is gone"
    else
        bad "the $oscreen exits once its tmux server is gone" "pid $orp gone" "still running"
        ocomm=''; [ -r "/proc/$orp/comm" ] && read -r ocomm 2>/dev/null < "/proc/$orp/comm"
        [ "$ocomm" = cockpit-tui ] && kill -KILL "$orp"
    fi
done
fi

echo "== cockpit-panel: a hangup with no SIGHUP still ends the panel and the board =="
# In a session of its own the binary has no controlling tty, so closing the pty
# master sends no SIGHUP: only the hangup watch can end it.
if [ ! -d /proc/self ] || ! command -v python3 >/dev/null 2>&1; then
    skip_ "a hangup with no SIGHUP (needs /proc and python3)"
elif need_tmux "a hangup with no SIGHUP"; then :; else
NH="cockpit-nohup-$$"; SOCKS="$SOCKS $NH"; NHPY=$(mktmpf)
"$TMUX_REAL" -L "$NH" -f /dev/null new-session -d -s t -x 100 -y 30 'exec cat' >/dev/null 2>&1
cat > "$NHPY" <<'NHPY'
import os, pty, select, subprocess, sys, time
tmux, sock, args = sys.argv[1], sys.argv[2], sys.argv[3:]
m, s = pty.openpty()
env = dict(os.environ, COCKPIT_READONLY='1', COCKPIT_ACTION='/nonexistent/act')
env['TMUX'] = subprocess.check_output([tmux, '-L', sock, 'display', '-p', '#{socket_path},#{pid},0']).decode().strip()
p = subprocess.Popen(args, stdin=s, stdout=s, stderr=subprocess.DEVNULL, start_new_session=True, env=env)
os.close(s)
for _ in range(100):                      # polled: until it has drawn something
    if select.select([m], [], [], 0.1)[0] and os.read(m, 65536):
        break
os.close(m)
for _ in range(100):
    if p.poll() is not None:
        print('exited'); sys.exit(0)
    time.sleep(0.1)
comm = open(f'/proc/{p.pid}/comm').read().strip()
print(f'survived {comm}')
if comm == 'cockpit-tui':                 # named before it is signalled
    p.kill()
NHPY
nha=$("$TMUX_REAL" -L "$NH" list-panes -F '#{pane_id}' | head -1)
is "the panel exits on a hangup that brings no signal" "exited" \
   "$(python3 -I "$NHPY" "$TMUX_REAL" "$NH" "$COCKPIT_TUI_BIN" panel "$nha" 2>&1)"
is "the board exits on a hangup that brings no signal" "exited" \
   "$(python3 -I "$NHPY" "$TMUX_REAL" "$NH" "$COCKPIT_TUI_BIN" board 2>&1)"
fi

echo "== cockpit-panel: tablet mode turns the mouse off on every exit path =="
# The modes are written to the tty, so the bytes are read off a pty: after the
# last ?1000h there must be a ?1000l and a ?1006l, whichever way the panel ends.
if [ ! -d /proc/self ] || ! command -v python3 >/dev/null 2>&1; then
    skip_ "the mouse is off on every exit (needs /proc and python3)"
elif need_tmux "the mouse is off on every exit"; then :; else
MX="cockpit-mouse-$$"; SOCKS="$SOCKS $MX"; MXPY=$(mktmpf); MXD=$(mktmpd)
"$TMUX_REAL" -L "$MX" -f /dev/null new-session -d -s t -x 100 -y 30 'exec cat' >/dev/null 2>&1
cat > "$MXPY" <<'MXPY'
import collections, fcntl, os, pty, select, signal, struct, subprocess, sys, termios, time
tmux, sock, act, binary, pane, mode, how = sys.argv[1:8]
runs = int(sys.argv[8]) if len(sys.argv) > 8 else 1
env = dict(os.environ, COCKPIT_ACTION=act, COCKPIT_PANEL_MODE=mode)
env['TMUX'] = subprocess.check_output([tmux, '-L', sock, 'display', '-p', '#{socket_path},#{pid},0']).decode().strip()
send = {
    'q': b'q', 'esc': b'\x1b', 'ctrl-c': b'\x03',
    # 4 tall tiles on an 80x24 tty: the button row is the last inside the border
    'tap-close': b'\x1b[<0;13;23M\x1b[<0;13;23m',
    'flip-then-q': b'3q',
    'action': b'1hi\x14',
}
def once(how):
    m, s = pty.openpty()
    fcntl.ioctl(s, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
    p = subprocess.Popen([binary, 'panel', pane], stdin=s, stdout=s, stderr=subprocess.DEVNULL, start_new_session=True, env=env)
    os.close(s)
    out = b''
    end = time.time() + 10                # a hang guard only: the loop polls for the first frame
    while b'will send' not in out and time.time() < end:
        if select.select([m], [], [], 0.1)[0]:
            try:
                out += os.read(m, 65536)
            except OSError:
                break
    # a tap within 300 ms of a new view is dropped (the double-tap guard): repeat until it ends the panel
    for _ in range(40 if how == 'tap-close' else 0):
        os.write(m, send['tap-close'])
        while select.select([m], [], [], 0.25)[0]:
            try:
                out += os.read(m, 65536)
            except OSError:
                break
        if p.poll() is not None or b'?1049l' in out:
            break
    os.write(m, b'' if how == 'tap-close' else send.get(how, b''))
    if how in ('term', 'hup'):
        if open(f'/proc/{p.pid}/comm').read().strip() == 'cockpit-tui':   # named before it is signalled
            p.send_signal(signal.SIGTERM if how == 'term' else signal.SIGHUP)
    # EIO once the slave is closed: all output is in, though the process may still be exiting
    while select.select([m], [], [], 10)[0]:
        try:
            chunk = os.read(m, 65536)
        except OSError:
            break
        out += chunk
    os.close(m)
    # the slave closing is not the exit: a thread can still be on its way out (#4 flake)
    try:
        p.wait(timeout=10)
    except subprocess.TimeoutExpired:
        if open(f'/proc/{p.pid}/comm').read().strip() == 'cockpit-tui':
            p.kill()
        return 'still running'
    on = out.rfind(b'\x1b[?1000h')
    tail = out[on:] if on >= 0 else b''
    return f"on={int(on >= 0)} off={int(b'[?1000l' in tail and b'[?1006l' in tail)}"
got = collections.Counter(once(how) for _ in range(runs))
print(' '.join(k if runs == 1 else f'{n}x {k}' for k, n in sorted(got.items())))
MXPY
mxa=$("$TMUX_REAL" -L "$MX" list-panes -F '#{pane_id}' | head -1)
mx() { rm -f "$MXD"/act*; python3 -I "$MXPY" "$TMUX_REAL" "$MX" "$MXD/act" "$COCKPIT_TUI_BIN" "$mxa" "$@" 2>&1; }
for how in q esc ctrl-c term hup tap-close action; do
    is "tablet mode: the exit by $how turns the mouse off" "on=1 off=1" "$(mx tablet "$how")"
done
is "and the action still goes" "paste $mxa" "$(mx tablet action >/dev/null; cat "$MXD/act" 2>/dev/null)"
is "pc mode never turns the mouse on" "on=0 off=0" "$(mx pc q)"
is "the Home tile turns it on in pc mode, and q turns it off" "on=1 off=1" "$(mx pc flip-then-q)"
mxe=$(COCKPIT_PANEL_MODE=Tablet "$COCKPIT_TUI_BIN" panel "$mxa" </dev/null 2>&1 >/dev/null); mxr=$?
# a signal exit that was read as "still running" 1 run in 30: 50 in a row, both signals
is "50 SIGTERM exits in a row all turn the mouse off" "50x on=1 off=1" "$(mx tablet term 50)"
is "50 SIGHUP exits in a row all turn the mouse off" "50x on=1 off=1" "$(mx tablet hup 50)"
is "and 50 in pc mode never turn it on" "50x on=0 off=0" "$(mx pc term 50)"
is "a mode that is not auto, pc or tablet is refused" "2 cockpit-tui: COCKPIT_PANEL_MODE is \"Tablet\": use auto, pc or tablet" "$mxr $mxe"
"$TMUX_REAL" -L "$MX" kill-server >/dev/null 2>&1
fi

echo "== cockpit-panel: a tap in the popup acts once, and x copies by OSC 52 =="
# A real popup on a client: the outer server's pane is the client's terminal, so
# an SGR click written there is what a touch screen sends. Coordinates are read
# off the outer capture, 1-based as the wire wants them.
if need_tmux "taps in the popup"; then :; else
TI="cockpit-tapi-$$"; TO="cockpit-tapo-$$"; SOCKS="$SOCKS $TI $TO"; TPD=$(mktmpd)
mkdir -p "$TPD/bin"
printf '#!/bin/sh\ncat >/dev/null\nprintf "<text>copied by tap</text>"\n' > "$TPD/bin/claude"
chmod +x "$TPD/bin/claude"
"$TMUX_REAL" -L "$TI" -f /dev/null new-session -d -s t -x 100 -y 30 'exec cat' >/dev/null 2>&1
# off is the default: an app's OSC 52 reaches tmux's buffers and the terminal only under on
"$TMUX_REAL" -L "$TI" set -g set-clipboard on >/dev/null 2>&1
tpa=$("$TMUX_REAL" -L "$TI" list-panes -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$TO" -f /dev/null new-session -d -s o -x 100 -y 30 \
    "TERM=xterm-256color exec $TMUX_REAL -L $TI attach -t t" >/dev/null 2>&1
"$TMUX_REAL" -L "$TO" set -g set-clipboard on >/dev/null 2>&1
until_cap "$TO" o '*[!\ ]*' >/dev/null
tpc=$("$TMUX_REAL" -L "$TI" list-clients -F '#{client_name}' | head -1)
"$TMUX_REAL" -L "$TI" display-popup -c "$tpc" -w 80 -h 24 -e COCKPIT_PANEL_MODE=tablet -E \
    "env 'PATH=$TPD/bin:$SHIM:/usr/bin:/bin' COCKPIT_LOG=$TPD/tui.log $DIR/scripts/cockpit-panel -p $tpa" >/dev/null 2>&1 &
tp_at() {                     # <label> <M|m> [button]  presses (M) or releases (m) on the label's first cell
    local y x line
    line=$("$TMUX_REAL" -L "$TO" capture-pane -p -t o | grep -n -F -m1 -- "$1") || return 1
    y=${line%%:*}; line=${line#*:}; line=${line%%"$1"*}
    x=$(( $(printf '%s' "$line" | wc -m) + 1 ))
    "$TMUX_REAL" -L "$TO" send-keys -t o -l "$(printf '\033[<%s;%s;%s%s' "${3:-0}" "$x" "$y" "$2")"
}
tp_tap() { tp_at "$1" M && tp_at "$1" m; }
# A tap within 300 ms of a new view is dropped (the double-tap guard), and no
# clock says when that ends: tap until the pattern shows, 0.5 s a try.
tp_tap_until() {              # <label> <socket> <pane> <pattern>
    local i POLLS=5
    for i in $(seq 1 20); do
        tp_tap "$1"
        until_cap "$2" "$3" "$4" && return 0
    done
    return 1
}
tp_cap() { "$TMUX_REAL" -L "$TO" capture-pane -p -t o; }
until_cap "$TO" o '*will send*' || bad "the tablet panel opens in the popup" "will send" "$(tp_cap)"
tp_tap_until Compose "$TO" o '*type: auto*' || bad "a tap on the Compose tile opens it" "type: auto" "$(tp_cap)"
"$TMUX_REAL" -L "$TO" send-keys -t o -l 'say hi'
if tp_tap_until '[ write ]' "$TO" o '*copied by tap*[ copy ]*'; then ok "a tap on [ write ] sends the draft to claude"
else bad "a tap on [ write ] sends the draft to claude" "copied by tap" "$(tp_cap)"; fi
tp_tap_until '[ copy ]' "$TO" o '*sent 13 chars to the clipboard*' || bad "x says what it copied" "sent 13 chars" "$(tp_cap)"
# past the guard now: a release alone is no tap, or insert would close the popup and paste
tp_at '[ insert ]' m
is "a release alone does nothing: the result is still open" "1" "$(tp_cap | grep -c -F '[ insert ]')"
is "the copy reaches tmux as OSC 52 (set-clipboard on)" "copied by tap" \
   "$("$TMUX_REAL" -L "$TI" show-buffer 2>/dev/null)"
is "and the outer terminal is handed it too" "copied by tap" "$("$TMUX_REAL" -L "$TO" show-buffer 2>/dev/null)"
tp_tap_until '[ insert ]' "$TI" "$tpa" '*copied by tap*' || bad "a tap on [ insert ] pastes into the pane" "copied by tap" \
    "$("$TMUX_REAL" -L "$TI" capture-pane -p -t "$tpa")"
is "one tap pastes once" "1" "$("$TMUX_REAL" -L "$TI" capture-pane -p -t "$tpa" | grep -c 'copied by tap')"
"$TMUX_REAL" -L "$TI" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TO" kill-server >/dev/null 2>&1
fi

echo "== the example configs ship the phone wrap override (#99) =="
# A phone terminal that wraps at the last cell scrolls the screen on every
# full-width status repaint (#97), so the override has to be IN the shipped
# config, not only in the README.
if need_tmux "phone wrap override"; then :; else
for phc in tmux.conf full.tmux.conf; do
    PH="cockpit-am-${phc%%.*}-$$"; SOCKS="$SOCKS $PH"
    "$TMUX_REAL" -L "$PH" -f /dev/null new-session -d -s t >/dev/null 2>&1
    "$TMUX_REAL" -L "$PH" source-file "$DIR/examples/$phc" >/dev/null 2>&1
    pho=$("$TMUX_REAL" -L "$PH" show -s terminal-overrides 2>/dev/null)
    case "$pho" in
        *"xterm-256color:am@"*) ok "examples/$phc carries xterm-256color:am@" ;;
        *) bad "examples/$phc carries xterm-256color:am@" "xterm-256color:am@" "$pho" ;;
    esac
    # -a appends, and reloading a config is routine: an unguarded append stacks
    # another copy of the override on every reload, forever.
    "$TMUX_REAL" -L "$PH" source-file "$DIR/examples/$phc" >/dev/null 2>&1
    is "and reloading examples/$phc does not stack a second copy" "1" \
       "$("$TMUX_REAL" -L "$PH" show -s terminal-overrides 2>/dev/null | grep -c 'xterm-256color:am@')"
    "$TMUX_REAL" -L "$PH" kill-server >/dev/null 2>&1
done
fi

echo "== prefix e hides the status line and borders for this session only (#99) =="
# The toggle writes session and window options, never -g: a second client on
# another session keeps its chrome. The restore reads what the config had --
# the global is set to `bottom` here, so a hardcoded `top` fails.
if need_tmux "prefix e full screen"; then :; else
FI="cockpit-fs-$$"; FO="cockpit-fso-$$"; SOCKS="$SOCKS $FI $FO"
"$TMUX_REAL" -L "$FI" -f /dev/null new-session -d -s t -x 100 -y 30 >/dev/null 2>&1
"$TMUX_REAL" -L "$FI" set -g status on >/dev/null 2>&1
"$TMUX_REAL" -L "$FI" set -g pane-border-status bottom >/dev/null 2>&1
# The plugin's own bar sets the border; off, the config's value is the one to keep.
"$TMUX_REAL" -L "$FI" set -g @cockpit_statusbar off >/dev/null 2>&1
"$TMUX_REAL" -L "$FI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
fs_sess() { "$TMUX_REAL" -L "$FI" show-options -t t -v status 2>/dev/null; }
fs_win()  { "$TMUX_REAL" -L "$FI" show-options -w -t t:0 -v pane-border-status 2>/dev/null; }
"$TMUX_REAL" -L "$FO" -f /dev/null new-session -d -s o -x 100 -y 32 \
    "TERM=xterm-256color exec $TMUX_REAL -L $FI attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$FI" list-clients -F '#{client_name}'
"$TMUX_REAL" -L "$FO" send-keys -t o C-b e
until_out off "$TMUX_REAL" -L "$FI" show-options -t t -v status
is "prefix e turns the status line off for the session" "off" "$(fs_sess)"
is "and the pane borders off for the window"            "off" "$(fs_win)"
is "the global status is untouched" "on"     "$("$TMUX_REAL" -L "$FI" show-options -gv status)"
is "the global border is untouched" "bottom" "$("$TMUX_REAL" -L "$FI" show-options -gwv pane-border-status)"
"$TMUX_REAL" -L "$FO" send-keys -t o C-b e
until_out on "$TMUX_REAL" -L "$FI" show-options -t t -v status
is "pressing it again brings the status line back" "on" \
   "$("$TMUX_REAL" -L "$FI" display-message -p -t t '#{status}')"
is "and restores the border the config had, not a hardcoded top" "bottom" \
   "$("$TMUX_REAL" -L "$FI" display-message -p -t t:0 '#{pane-border-status}')"
"$TMUX_REAL" -L "$FO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$FI" kill-server >/dev/null 2>&1
# A fresh server: nothing unbinds a key already bound, so `off` can only be
# read on a server that has not loaded the plugin yet -- the same shape as the
# sessions-key block above.
"$TMUX_REAL" -L "$FI" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$FI" set -g @cockpit_fullscreen_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$FI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "@cockpit_fullscreen_key off binds nothing" "" \
   "$("$TMUX_REAL" -L "$FI" list-keys -T prefix 2>/dev/null | awk '$4=="e"')"
"$TMUX_REAL" -L "$FI" kill-server >/dev/null 2>&1
fi

echo "== prefix e reads the state the config starts in (#99) =="
# Branching on #{status} read a config that already hides the status line as
# "restore": one press turned the status ON, the borders off, and stashed
# nothing, so the borders had nothing to come back to. #99 names that config as
# the status quo on a phone. A flag is the state now: set means hidden.
if need_tmux "prefix e starting state"; then :; else
fs_client() {                 # <inner> <outer>: a real client on the inner server
    "$TMUX_REAL" -L "$2" -f /dev/null new-session -d -s o -x 100 -y 32 \
        "TERM=xterm-256color exec $TMUX_REAL -L $1 attach -t t" >/dev/null 2>&1
    until_out '?*' "$TMUX_REAL" -L "$1" list-clients -F '#{client_name}'
}
FQ="cockpit-fsq-$$"; FQO="cockpit-fsqo-$$"; SOCKS="$SOCKS $FQ $FQO"
"$TMUX_REAL" -L "$FQ" -f /dev/null new-session -d -s t -x 100 -y 30 >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" set -g status off >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" set -g pane-border-status bottom >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" set -g @cockpit_statusbar off >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
fs_client "$FQ" "$FQO"
"$TMUX_REAL" -L "$FQO" send-keys -t o C-b e
until_out 1 "$TMUX_REAL" -L "$FQ" show-options -t t -qv @cockpit_fs
is "starting from status off, the first press hides the borders" "off" \
   "$("$TMUX_REAL" -L "$FQ" show-options -w -t t:0 -v pane-border-status 2>/dev/null)"
is "and marks the session hidden" "1" \
   "$("$TMUX_REAL" -L "$FQ" show-options -t t -qv @cockpit_fs)"
is "and never turns the status line on" "off" \
   "$("$TMUX_REAL" -L "$FQ" display-message -p -t t '#{status}')"
"$TMUX_REAL" -L "$FQO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$FQ" kill-server >/dev/null 2>&1
# status is a value, not a flag: a two-line status restored as `on` costs a row.
FT="cockpit-fst-$$"; FTO="cockpit-fsto-$$"; SOCKS="$SOCKS $FT $FTO"
"$TMUX_REAL" -L "$FT" -f /dev/null new-session -d -s t -x 100 -y 30 >/dev/null 2>&1
"$TMUX_REAL" -L "$FT" set -g status 2 >/dev/null 2>&1
"$TMUX_REAL" -L "$FT" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
fs_client "$FT" "$FTO"
"$TMUX_REAL" -L "$FTO" send-keys -t o C-b e
until_out off "$TMUX_REAL" -L "$FT" display-message -p -t t '#{status}'
"$TMUX_REAL" -L "$FTO" send-keys -t o C-b e
until_out 2 "$TMUX_REAL" -L "$FT" display-message -p -t t '#{status}'
is "a two-line status comes back as two lines, not as on" "2" \
   "$("$TMUX_REAL" -L "$FT" display-message -p -t t '#{status}')"
is "and the flag is cleared, so the next press hides again" "" \
   "$("$TMUX_REAL" -L "$FT" show-options -t t -qv @cockpit_fs)"
"$TMUX_REAL" -L "$FTO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$FT" kill-server >/dev/null 2>&1
fi

echo "== the override stops the status write one cell short of the corner (#99) =="
# The mechanism, driven through a real pty: tmux writes the bottom-right cell
# unless the terminal lacks am, and a terminal that wraps there scrolls. The
# probe replays the client's own bytes through a cursor tracker and reports the
# last column written on the bottom row: 130 with the override, 131 without.
if need_tmux "phone pty column"; then :; elif ! command -v python3 >/dev/null 2>&1; then
    skip_ "the override stops the status one cell short (no python3)"
    skip_ "a desktop TERM still writes the last column (no python3)"
elif ! infocmp xterm-ghostty >/dev/null 2>&1; then
    skip_ "the override stops the status one cell short (no xterm-ghostty terminfo)"
    skip_ "a desktop TERM still writes the last column (no xterm-ghostty terminfo)"
else
PTD=$(mktmpd); PTCONF="$PTD/conf"; PTPY="$PTD/probe.py"
# The override comes out of the shipped file: delete it there and this block
# goes red, which is the only reason the probe is worth its four seconds.
sed -n 's/^ *\(set -as terminal-overrides .*\)$/\1/p' "$DIR/examples/tmux.conf" > "$PTCONF"
cat >> "$PTCONF" <<'PTEOF'
set -g status on
set -g status-position bottom
set -g status-interval 1
set -g status-right "#(date +%T)"
PTEOF
cat > "$PTPY" <<'PTEOF'
# Attach a 131x40 client over a pty and report the last column written on the
# bottom row. Tracks CUP, VPA, CUF, HPA, CR and LF and counts printable cells;
# SGR, OSC and the three-byte \e(B are skipped -- the refute lane's tracker ate
# two bytes of \e(B and fabricated a scroll (verified.md, note).
import fcntl, os, pty, select, struct, sys, termios, time

sock, term, conf = sys.argv[1], sys.argv[2], sys.argv[3]
COLS, ROWS = 131, 40
env = dict(os.environ, TERM=term, COLUMNS=str(COLS), LINES=str(ROWS))
env.pop('TMUX', None)
env.pop('TMUX_PANE', None)
pid, fd = pty.fork()
if pid == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack('HHHH', ROWS, COLS, 0, 0))
    os.execvpe('tmux', ['tmux', '-L', sock, '-f', conf, 'new-session', '-s', 'p'], env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', ROWS, COLS, 0, 0))


def last_column(s):
    row, col, hi, saved, i, n = 1, 1, 0, (1, 1), 0, len(s)
    while i < n:
        c = s[i]
        if c == '\x1b':
            nxt = s[i + 1] if i + 1 < n else ''
            if nxt == '[':
                j = i + 2
                while j < n and not ('@' <= s[j] <= '~'):
                    j += 1
                if j >= n:
                    break
                body, final = s[i + 2:j].replace('?', '').replace('>', ''), s[j]
                par = [int(p) if p.isdigit() else 0 for p in body.split(';')] if body else []
                one = par[0] if par and par[0] else 1
                two = par[1] if len(par) > 1 and par[1] else 1
                if final in 'Hf':
                    row, col = one, two
                elif final == 'd':
                    row = one
                elif final in 'G`':
                    col = one
                elif final == 'C':
                    col += one
                elif final == 'D':
                    col = max(1, col - one)
                elif final == 'A':
                    row = max(1, row - one)
                elif final == 'B':
                    row += one
                i = j + 1
            elif nxt == ']':
                j = i + 2
                while j < n and s[j] != '\x07' and not (s[j] == '\x1b' and j + 1 < n and s[j + 1] == '\\'):
                    j += 1
                i = j + (1 if j < n and s[j] == '\x07' else 2)
            elif nxt in '()*+':
                i += 3
            elif nxt == '7':
                saved = (row, col)
                i += 2
            elif nxt == '8':
                row, col = saved
                i += 2
            else:
                i += 2
        else:
            if c == '\r':
                col = 1
            elif c == '\n':
                row += 1
            elif c == '\b':
                col = max(1, col - 1)
            elif c >= ' ':
                if row >= ROWS and col <= COLS:
                    hi = max(hi, col)
                col += 1
                if col > COLS:
                    col, row = 1, row + 1
            i += 1
        row = min(row, ROWS)
    return hi


buf, start, last = '', time.time(), time.time()
while True:
    now = time.time()
    # Drained until the stream goes quiet, never for a fixed span: a cut mid
    # frame reports a column the client never finished writing.
    if now - start > 3 and now - last > 0.3 and last_column(buf) >= 100:
        break
    if now - start > 20:
        break
    r = select.select([fd], [], [], 0.2)[0]
    if r:
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        buf += chunk.decode('latin-1')
        last = time.time()
print(last_column(buf))
PTEOF
PTS1="cockpit-pty1-$$"; PTS2="cockpit-pty2-$$"; SOCKS="$SOCKS $PTS1 $PTS2"
is "the status write on the bottom row stops at column 130 (xterm-256color)" "130" \
   "$(python3 "$PTPY" "$PTS1" xterm-256color "$PTCONF" 2>/dev/null)"
"$TMUX_REAL" -L "$PTS1" kill-server >/dev/null 2>&1
is "and a desktop TERM still writes column 131 (xterm-ghostty)" "131" \
   "$(python3 "$PTPY" "$PTS2" xterm-ghostty "$PTCONF" 2>/dev/null)"
"$TMUX_REAL" -L "$PTS2" kill-server >/dev/null 2>&1
rm -rf "$PTD"
fi
# --- #103 example configs ---
# A client with a chosen width, without a pty: control mode attaches over a
# fifo opened read-write by this shell, so nothing has to feed it and there is
# no writer process to reap. refresh-client -C is all #{client_width} needs.
cw_open() {                   # <socket> <session> ; prints "<client> <fifo> <pid>"
    local s=$1 t=$2 f c i
    f=$(mktemp -u) || return 1
    mkfifo "$f" || return 1
    "$TMUX_REAL" -L "$s" -C attach -t "$t" <> "$f" >/dev/null 2>&1 &
    local pid=$!
    for i in $(seq 1 40); do
        c=$("$TMUX_REAL" -L "$s" list-clients -F '#{client_name}' 2>/dev/null | head -1)
        [ -n "$c" ] && { printf '%s %s %s' "$c" "$f" "$pid"; return 0; }
        sleep 0.1
    done
    # Nothing is printed on failure, so the caller gets no pid and no path:
    # the half-built client and its fifo have to be reaped right here.
    kill "$pid" >/dev/null 2>&1; rm -f "$f"
    return 1
}
# NEVER `kill "${pid:-0}"`: an empty pid becomes 0, and kill 0 signals the
# whole process group -- the suite itself. Guard, do not default.
cw_close() {                  # <pid> <fifo> ; either may be empty
    [ -n "${1:-}" ] && kill "$1" >/dev/null 2>&1
    [ -n "${2:-}" ] && rm -f "$2"
    return 0
}
cw_size() {                   # <socket> <client> <cols>
    "$TMUX_REAL" -L "$1" refresh-client -C "${3}x30" -t "$2" >/dev/null 2>&1
    until_out "$3" "$TMUX_REAL" -L "$1" display-message -p -c "$2" '#{client_width}'
}

echo "== the border question comes first; the branch, then the path, get what is left (#103, #222) =="
# A fixed width for the question pushed the buttons off a narrow pane (#97),
# and fixed path and branch widths squeezed the question. The budget is the
# pane's own width: the question takes what it needs, the branch and then the
# path get the rest, and the menu's Browse folder and Switch branch rows exist
# exactly when the border has no room for their targets.
if need_tmux "border budget"; then :; else
BQ="cockpit-bq-$$"; SOCKS="$SOCKS $BQ"; BQD=$(mktmpd)
mkdir -p "$BQD/repo/.git"; printf 'ref: refs/heads/feat/jwt-rotation\n' > "$BQD/repo/.git/HEAD"
"$TMUX_REAL" -L "$BQ" -f /dev/null new-session -d -s s -x 200 -y 30 -c "$BQD/repo" 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$BQ" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$BQ" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
BQP=$("$TMUX_REAL" -L "$BQ" list-panes -t s: -F '#{pane_id}')
TMUX="$SOCKDIR/$BQ,0,0" "$DIR/scripts/tmux-where" branch --set "$BQP" >/dev/null 2>&1
read -r bqc bqf bqpid <<<"$(cw_open "$BQ" s)"
if [ -z "${bqc:-}" ]; then skip_ "the border budget (no control client)"; else
bqfit() { "$TMUX_REAL" -L "$BQ" display-message -p -c "$bqc" -t "$BQP" \
            '#{E:@cockpit_path_shown} #{E:@cockpit_branch_shown} #{E:@cockpit_ask_w}'; }
bqrows() { "$TMUX_REAL" -L "$BQ" display-message -p -c "$bqc" -t "$BQP" \
            '[#{?#{E:@cockpit_path_shown},,Browse folder}][#{?#{E:@cockpit_branch_shown},,#{?#{E:@cockpit_where_branch},Switch branch,}}]'; }
cw_size "$BQ" "$bqc" 80
is "a quiet pane at 80 columns shows path and branch" "1 1 0" "$(bqfit)"
is "so the menu has neither row" "[][]" "$(bqrows)"
"$TMUX_REAL" -L "$BQ" set -p -t "$BQP" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$BQ" set -p -t "$BQP" @panestate_msg \
    "Should I force-push the rebased branch to origin and open the PR now" >/dev/null 2>&1
# 80 - 6 for the line ends - 12 for the touch corner (≡ and the session name).
is "a waiting pane gives the question every cell left, the path and branch none" "0 0 62" "$(bqfit)"
is "so the menu offers both" "[Browse folder][Switch branch]" "$(bqrows)"
# A cut that lands on a space is flush with its …, as on the tabs: 62 cells of
# 61 a's, a space and more, end "a…", never "a …".
"$TMUX_REAL" -L "$BQ" set -p -t "$BQP" @panestate_msg "$(printf 'a%.0s' $(seq 61)) bbbbbbbb" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$BQ" display-message -p -c "$bqc" -t "$BQP" '#{E:pane-border-format}' | plain)" in
    *"a …"*) bad "a cut question leaves no space before its …" "…a…" "a space before the …" ;;
    *"a…"*)  ok "a cut question leaves no space before its …" ;;
    *) bad "a cut question leaves no space before its …" "…a…" "$("$TMUX_REAL" -L "$BQ" display-message -p -c "$bqc" -t "$BQP" '#{E:pane-border-format}' | plain)" ;;
esac
"$TMUX_REAL" -L "$BQ" set -p -t "$BQP" @panestate_msg \
    "Should I force-push the rebased branch to origin and open the PR now" >/dev/null 2>&1
cw_size "$BQ" "$bqc" 200
is "at 200 columns the whole question fits beside path and branch" "1 1 68" "$(bqfit)"
fi
cw_close "${bqpid:-}" "${bqf:-}"
"$TMUX_REAL" -L "$BQ" kill-server >/dev/null 2>&1
fi

echo "== under prefix e the menu offers Browse folder and Switch branch again (#222) =="
# Full screen hides the border, and with it the path and branch taps: the rows
# that stand in for them must come back.
if need_tmux "menu under full screen"; then :; else
FM="cockpit-fm-$$"; FMO="cockpit-fmo-$$"; SOCKS="$SOCKS $FM $FMO"; FMD=$(mktmpd)
mkdir -p "$FMD/repo/.git"; printf 'ref: refs/heads/main\n' > "$FMD/repo/.git/HEAD"
"$TMUX_REAL" -L "$FM" -f /dev/null new-session -d -s t -x 200 -y 40 -c "$FMD/repo" 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FM" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
TMUX="$SOCKDIR/$FM,0,0" "$DIR/scripts/tmux-where" branch --set \
    "$("$TMUX_REAL" -L "$FM" display-message -p -t t: '#{pane_id}')" >/dev/null 2>&1
"$TMUX_REAL" -L "$FMO" -f /dev/null new-session -d -s o -x 200 -y 40 \
    "TERM=xterm-256color exec $TMUX_REAL -L $FM attach -t t" >/dev/null 2>&1
until_cap "$FMO" o '*⎇ main*'
"$TMUX_REAL" -L "$FMO" send-keys -t o C-b a
until_cap "$FMO" o '*New window*'
case "$("$TMUX_REAL" -L "$FMO" capture-pane -p -t o)" in
    *"Browse folder"*|*"Switch branch"*) bad "with the border on, neither row is offered" "no row" "a row" ;;
    *) ok "with the border on, neither row is offered" ;;
esac
"$TMUX_REAL" -L "$FMO" send-keys -t o Escape
"$TMUX_REAL" -L "$FMO" send-keys -t o C-b e
until_out 1 "$TMUX_REAL" -L "$FM" show-options -t t -qv @cockpit_fs >/dev/null
"$TMUX_REAL" -L "$FMO" send-keys -t o C-b a
if until_cap "$FMO" o '*Browse folder*Switch branch*'; then ok "under prefix e both rows are back"
else bad "under prefix e both rows are back" "Browse folder and Switch branch" "$("$TMUX_REAL" -L "$FMO" capture-pane -p -t o | grep '│' | head -12)"; fi
"$TMUX_REAL" -L "$FMO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$FM" kill-server >/dev/null 2>&1
fi

echo "== a window tab gets the width it can afford (#103, #222) =="
# The current tab gets (W - reserve)/n - 6, clamped to 12..cap (cap 28 from 160
# columns, else 18); the others share the rest and go compact below 4 cells.
# Formats on #{client_width}: two clients on one session can be two widths.
if need_tmux "tab width ladder"; then :; else
TW="cockpit-tw-$$"; SOCKS="$SOCKS $TW"
"$TMUX_REAL" -L "$TW" -f /dev/null new-session -d -s s -n t -x 200 -y 50 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TW" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$TW" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
twp=$("$TMUX_REAL" -L "$TW" list-panes -t s: -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$TW" set -p -t "$twp" @cockpit_title review-the-ui-audit-ladder >/dev/null 2>&1
# The load's background sweep marks the agent window idle (○); run it here so it
# cannot land mid-test.
TMUX="$SOCKDIR/$TW,0,0" "$DIR/scripts/agent-state" sweep >/dev/null 2>&1
read -r twc twf twpid <<<"$(cw_open "$TW" s)"
if [ -z "${twc:-}" ]; then skip_ "the tab width ladder (no control client)"; else
twtab() { "$TMUX_REAL" -L "$TW" display-message -p -c "$twc" -t s:t '#{E:window-status-current-format}' 2>/dev/null | plain; }
cw_size "$TW" "$twc" 80
is "an 80-column client cuts the current tab at 18 cells" " 0 review-the-ui-audi… ○ " "$(twtab)"
cw_size "$TW" "$twc" 160
is "a 160-column client draws the whole title" " 0 review-the-ui-audit-ladder ○ " "$(twtab)"
for twn in $(seq 2 8); do "$TMUX_REAL" -L "$TW" new-window -d -t s: -n "w$twn" 'exec -a claude sleep 300' >/dev/null 2>&1; done
twfit() { "$TMUX_REAL" -L "$TW" display-message -p -c "$twc" -t s:t '#{E:@cockpit_cur_w} #{E:@cockpit_tab_w} #{E:@cockpit_narrow}'; }
cw_size "$TW" "$twc" 200
is "eight windows at 200 columns keep their labels" "18 18 0" "$(twfit)"
cw_size "$TW" "$twc" 56
is "at 56 the others go compact; the current tab keeps 12" "12 1" "$(twfit | cut -d' ' -f1,3)"
fi
cw_close "${twpid:-}" "${twf:-}"
"$TMUX_REAL" -L "$TW" kill-server >/dev/null 2>&1
fi

echo "== the context gauge adds to its tab, and the budget counts it (#103, #222) =="
# ◔NN% from @panestate_ctx, written by scripts/agent-context, from 80% on. A
# label keeps its cap and the gauge goes on top; the others share what is left
# after every gauge, which agent-context sums into @cockpit_ctx_all.
if need_tmux "context gauge"; then :; else
GG="cockpit-gg-$$"; SOCKS="$SOCKS $GG"
"$TMUX_REAL" -L "$GG" -f /dev/null new-session -d -s s -n t -x 200 -y 50 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$GG" new-window -d -t s: -n u 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$GG" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$GG" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
until_out '*claude*claude*' "$TMUX_REAL" -L "$GG" list-panes -a -F '#{pane_current_command}' >/dev/null
"$TMUX_REAL" -L "$GG" set -p -t s:t @cockpit_title review-the-ui-audit-ladder >/dev/null 2>&1
"$TMUX_REAL" -L "$GG" set -p -t s:u @cockpit_title second-window-with-a-long-name >/dev/null 2>&1
ggctx() {                     # <window> <percent>: the statusLine payload, as Claude sends it
    printf '{"context_window":{"used_percentage":%s},"model":{"display_name":"m"}}' "$2" \
        | TMUX="$SOCKDIR/$GG,0,0" TMUX_PANE="$("$TMUX_REAL" -L "$GG" display-message -p -t "s:$1" '#{pane_id}')" \
          "$DIR/scripts/agent-context" >/dev/null 2>&1
}
# The sweep the window hooks run, now: every agent window idle (○) from the start,
# so no later hook changes a tab under an assertion.
TMUX="$SOCKDIR/$GG,0,0" "$DIR/scripts/agent-state" sweep >/dev/null 2>&1
read -r ggc ggf ggpid <<<"$(cw_open "$GG" s)"
if [ -z "${ggc:-}" ]; then skip_ "the context gauge (no control client)"; else
ggcur() { "$TMUX_REAL" -L "$GG" display-message -p -c "$ggc" -t s:t '#{E:window-status-current-format}' | plain; }
ggoth() { "$TMUX_REAL" -L "$GG" display-message -p -c "$ggc" -t s:u '#{E:window-status-format}' | plain; }
cw_size "$GG" "$ggc" 160
ggctx t 79
is "under 80% there is no gauge" " 0 review-the-ui-audit-ladder ○ " "$(ggcur)"
ggctx t 91; ggctx u 100
is "from 80% the gauge goes on top of a label that keeps its cap" \
   " 0 review-the-ui-audit-ladder ○ ◔91% " "$(ggcur)"
is "and on another tab" " 1 second-window-with-a-long-na… ○ ◔100% " "$(ggoth)"
is "agent-context sums the session's gauge cells" "11" \
   "$("$TMUX_REAL" -L "$GG" show-option -qv -t s: @cockpit_ctx_all)"
cw_size "$GG" "$ggc" 80
is "and the tab budget takes them off the room" "61" \
   "$("$TMUX_REAL" -L "$GG" display-message -p -c "$ggc" '#{E:@cockpit_tab_room}')"
for _ in $(seq 3 9); do "$TMUX_REAL" -L "$GG" new-window -d -t s: 'exec -a claude sleep 300' >/dev/null 2>&1; done
cw_size "$GG" "$ggc" 56
is "a compact tab shows the bare ◔" " 1 ○ ◔ " "$(ggoth)"
# A cut that lands after a word leaves no space before the …: 12 cells of
# "abcdefghijk lmno..." are "abcdefghijk ".
"$TMUX_REAL" -L "$GG" set -p -t s:t @cockpit_title "abcdefghijk lmnopqrstuvwxyz" >/dev/null 2>&1
is "a cut on a word boundary drops the space before the …" " 0 abcdefghijk… ○ ◔91% " "$(ggcur)"
fi
cw_close "${ggpid:-}" "${ggf:-}"
"$TMUX_REAL" -L "$GG" kill-server >/dev/null 2>&1
fi

echo "== the menu's ≡ follows @cockpit_icons; the state glyphs never do (#222) =="
if need_tmux "menu icon tiers"; then :; else
IC="cockpit-ic-$$"; SOCKS="$SOCKS $IC"
"$TMUX_REAL" -L "$IC" -f /dev/null new-session -d -s s -x 120 -y 30 >/dev/null 2>&1
"$TMUX_REAL" -L "$IC" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
icm() { "$TMUX_REAL" -L "$IC" set -g @cockpit_icons "$1" >/dev/null 2>&1
        "$TMUX_REAL" -L "$IC" display-message -p '#{E:@cockpit_i_menu}'; }
is "unicode (unset) draws ≡" "≡" "$(icm '')"
is "ascii draws =" "=" "$(icm ascii)"
is "nerd draws U+F0C9" "$(printf '\357\203\211')" "$(icm nerd)"
is "and the nerd branch icon is U+E0A0" "$(printf '\356\202\240') " \
   "$("$TMUX_REAL" -L "$IC" display-message -p '#{E:@cockpit_i_branch}')"
"$TMUX_REAL" -L "$IC" set -w -t s: @cockpit_win_state needs-input >/dev/null 2>&1
is "and the ▲ stays a ▲ in the ascii tier" " #[fg=themered]▲" \
   "$("$TMUX_REAL" -L "$IC" set -g @cockpit_icons ascii \; display-message -p -t s: '#{E:@cockpit_marks}')"
"$TMUX_REAL" -L "$IC" kill-server >/dev/null 2>&1
fi

echo "== the menu's What is each agent doing? lists every pane with its state (#125, #222) =="
# The row replays the cockpit table's t, a popup over one tmux list-panes. Its
# -F format is resolved here on a server, the same pass the popup's shell runs.
if need_tmux "the agents popup"; then :; else
AGS="cockpit-agents-$$"; SOCKS="$SOCKS $AGS"
"$TMUX_REAL" -L "$AGS" -f /dev/null new-session -d -s ag -x 200 -y 50 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$AGS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$AGS" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$AGS" list-keys -T prefix | awk '$4=="a"')" in
    *"What is each agent doing?"*"send-keys -K t"*) ok "the menu has the row, replaying the cockpit table's t" ;;
    *) bad "the menu has the row, replaying the cockpit table's t" "What is each agent doing? ... send-keys -K t" "absent" ;;
esac
AGF=$("$TMUX_REAL" -L "$AGS" list-keys -T cockpit | awk '$4=="t"' | sed -n "s/.*list-panes -a -F '\([^']*\)'.*/\1/p")
"$TMUX_REAL" -L "$AGS" new-window -d -t ag: 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$AGS" new-window -d -t ag: 'sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$AGS" select-pane -t ag:0 -T 'alpha  two  spaces' >/dev/null 2>&1
"$TMUX_REAL" -L "$AGS" select-pane -t ag:1 -T bravo >/dev/null 2>&1
"$TMUX_REAL" -L "$AGS" select-pane -t ag:2 -T plain >/dev/null 2>&1
"$TMUX_REAL" -L "$AGS" set-option -p -t ag:0 @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$AGS" set-option -p -t ag:1 @panestate running >/dev/null 2>&1
until_out '*claude*claude*sleep*' "$TMUX_REAL" -L "$AGS" list-panes -a -F '#{pane_current_command}' >/dev/null
# The third pane is not an agent: its state column is the - sentinel, not a blank
# that would slide the title one column left.
is "two agent panes with their state, the plain pane with the sentinel, in fixed columns" \
   "ag:0             needs-input  alpha  two  spaces|ag:1             running      bravo|ag:2             -            plain" \
   "$("$TMUX_REAL" -L "$AGS" list-panes -a -F "$AGF" 2>/dev/null | paste -sd'|' -)"
is "and the popup needs no column(1), which split a title on its spaces" "0" \
   "$("$TMUX_REAL" -L "$AGS" list-keys -T cockpit | awk '$4=="t"' | grep -c 'column')"
"$TMUX_REAL" -L "$AGS" kill-server >/dev/null 2>&1
fi

echo "== full.tmux.conf hardcodes no plugin path (#103) =="
# Two menu rows hardcoded ~/.local/bin with their own popup geometry (#58 defect
# 11): a second definition of the plugin's popup drifts from it. Pure file check.
is "full.tmux.conf hardcodes no ~/.local/bin path" "0" \
   "$(grep -c '~/.local/bin' "$DIR/examples/full.tmux.conf")"


# --- #104 fzf theme ---------------------------------------------------------
echo "== the shared fzf theme: one argv, key hints (#104) =="
FZTD=$(mktmpd)
FZLIB="$DIR/scripts/lib/fzf.sh"
# A fake fzf: answers --version, then records its argv one argument per line and
# exits 130 -- the code a real esc returns, so each picker walks its own way out.
# The suffix is the fake's own pid, so a picker that opens twice keeps both files.
fzfake() {                    # fzfake <dir> <version string>
    mkdir -p "$1"
    cat > "$1/fzf" <<EOS
#!/bin/sh
case "\${1:-}" in --version) [ -n "\${FZ_VLOG:-}" ] && printf 'v\n' >> "\$FZ_VLOG"
    printf '%s\n' '$2'; exit 0 ;; esac
cat >/dev/null
[ -n "\${FZ_ARGV:-}" ] && { printf '%s\n' "\$@" > "\$FZ_ARGV.\$\$"; printf '%s\n' "\$#" > "\$FZ_ARGV"n."\$\$"; }
exit 130
EOS
    chmod +x "$1/fzf"
}
fznew="$FZTD/new"; fzold="$FZTD/old"
fzfake "$fznew" "0.74.3 (15f64c49)"
fzfake "$fzold" "0.44.1 (debian)"

# Every picker this lane owns, driven to the point where it hands off to fzf.
FZP="$FZTD/bin"; fzfake "$FZP" "0.74.3 (15f64c49)"
FZNB="$FZTD/nofzf"; mkdir -p "$FZNB"
for c in bash sh git tput readlink sed grep ls head cat awk sort id dirname basename mktemp rm; do
    p=$(command -v "$c" 2>/dev/null) || continue
    ln -sf "$p" "$FZP/$c"; ln -sf "$p" "$FZNB/$c"
done
ln -sf "$SHIM/tmux" "$FZP/tmux"; ln -sf "$SHIM/tmux" "$FZNB/tmux"
FZG=$(mktmpd)
( cd "$FZG" && git init -q -b main . && : > a && git add a &&
  GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e \
      git commit -q -m init ) >/dev/null 2>&1
: > "$FZTD/clicked.txt"
tmux set-option -p -t "$PANE" @cockpit_click_line "see $FZTD/clicked.txt:1" 2>/dev/null

# The suite server is what tm lists and what --pick reads the click line from;
# a lost session here is the picker "not reaching fzf" three assertions later
# (#128), so it is named before any picker runs.
is "the suite server still holds its session before the pickers run" "suite" \
   "$(tmux display-message -p -t "$PANE" '#{session_name}' 2>&1)"
fzrun() {                     # fzrun <tag> <cmd...> -- capture that picker's argv, rc and stderr
    local tag=$1; shift
    env -u TMUX -u TMUX_PANE PATH="$FZP" FZ_ARGV="$FZTD/argv-$tag-$$" \
        FZ_VLOG="$FZTD/vlog-$tag" "$@" </dev/null >/dev/null 2>"$FZTD/err-$tag"
    printf '%s\n' "$?" > "$FZTD/rc-$tag"
    # Name the step when a picker never reaches fzf (#128): its exit code, its
    # stderr and what the suite server listed at that moment, not a count.
    [ -n "$(ls "$FZTD"/argv-"$tag"-"$$".* 2>/dev/null)" ] ||
        bad "$tag reaches fzf" "an argv file" \
            "rc=$(cat "$FZTD/rc-$tag") stderr=[$(tail -c 200 "$FZTD/err-$tag" | tr '\n' ' ')] sessions=[$(tmux list-sessions -F '#{session_id}=#{session_name}' 2>&1 | tr '\n' ' ')]"
}
fzrun tm-attach "$DIR/scripts/tm"
fzrun tm-kill   "$DIR/scripts/tm" -k
fzrun branch    "$DIR/scripts/tmux-branch" "$FZG"
fzrun path      "$DIR/scripts/tmux-open-path" --pick "$PANE"
fzrun browse    "$DIR/scripts/tmux-open-path" --browse "$FZTD"
fzn=0; fzboth=0
for f in "$FZTD"/argv-*-"$$".*; do
    [ -e "$f" ] || continue
    fzn=$((fzn+1))
    grep -qx -- '--ansi' "$f" && grep -qx -- '--tabstop=1' "$f" && fzboth=$((fzboth+1))
done
is "all five pickers reach fzf, and every argv carries --ansi and --tabstop=1" "5 5" "$fzn $fzboth"

# shellcheck disable=SC2002  # cat, so a missing log still counts 0
is "a picker open never probes fzf --version" "0" \
   "$(cat "$FZTD/vlog-tm-attach" 2>/dev/null | grep -c .)"

# fzf merges --color per key and the last occurrence wins, so an override is
# only an override if it is appended after the theme.
fzk=$(cat "$FZTD"/argv-tm-kill-"$$".* 2>/dev/null)
fzti=$(printf '%s\n' "$fzk" | grep -n -- '^--color=16,fg:' | head -1 | cut -d: -f1)
fzui=$(printf '%s\n' "$fzk" | grep -n -- 'prompt:red' | head -1 | cut -d: -f1)
is "the kill picker's red --color lands after the theme's, so it wins" "yes" \
   "$([ -n "$fzti" ] && [ -n "$fzui" ] && [ "$fzui" -gt "$fzti" ] 2>/dev/null && echo yes || echo no)"

# Read off the same fake: the argv fzf would be handed.
fzrun styled "$DIR/scripts/tm"
fzs=$(cat "$FZTD"/argv-styled-"$$".* 2>/dev/null)
fzhas() { printf '%s\n' "$fzs" | grep -q -- "$1" && printf yes || printf no; }
is "styled: --highlight-line, the keys in --footer, no --header, selected-bg" \
   "yes yes no yes" \
   "$(fzhas '^--highlight-line$'; printf ' '; fzhas '^--footer=.*kill'
      printf ' '; fzhas '^--header='; printf ' '; fzhas '^--color=16,.*selected-bg:')"
# The open picker's two key lines must be ONE --footer argument (#116): the
# lib prints one argument per line, so a footer asked of it with a newline
# arrives as two, and fzf rejects the second as an unknown option (exit 2).
# Lines minus argc is the count of embedded newlines the fake was handed.
fzrun styled-open "$DIR/scripts/tmux-open-path" --pick "$PANE"
is "styled: the open picker's two key lines are one --footer argument" "1" \
   "$(( $(cat "$FZTD"/argv-styled-open-"$$".* 2>/dev/null | wc -l) - $(cat "$FZTD"/argv-styled-open-"$$"n.* 2>/dev/null || echo 0) ))"

# A gapped palette: two of the tokens the theme reads set, the rest unset. With
# a whitespace separator the gaps collapse and every later colour shifts onto
# the wrong key.
FZGS="cockpit-fzfgap-$$"; SOCKS="$SOCKS $FZGS"
"$TMUX_REAL" -L "$FZGS" -f /dev/null new-session -d 'sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FZGS" set -g @thm_error '#ff9838' >/dev/null 2>&1
"$TMUX_REAL" -L "$FZGS" set -g @thm_accent '#c9a2ff' >/dev/null 2>&1
FZGB="$FZTD/gapbin"; mkdir -p "$FZGB"
printf '#!/bin/sh\nexec %s -L %s "$@"\n' "$TMUX_REAL" "$FZGS" > "$FZGB/tmux"
chmod +x "$FZGB/tmux"; ln -sf "$FZP/fzf" "$FZGB/fzf"
fzc=$(env -u TMUX -u TMUX_PANE PATH="$FZGB" \
        "$BASH" -c '. "$1" && cockpit_fzf_args' _ "$FZLIB" 2>/dev/null | grep '^--color=')
is "two set tokens with gaps between them: no colour lands on its neighbour's key" \
   "fg:#e6e4df hl:#ff9838 bg+:#2a2f3a prompt:#c9a2ff" \
   "$(printf '%s\n' "$fzc" | tr ',' '\n' | grep -E '^(fg|hl|bg\+|prompt):' | tr '\n' ' ' | sed 's/ $//')"
# tmux's own `theme light` picks the _light roles: a popup cannot ask its client.
"$TMUX_REAL" -L "$FZGS" set -s theme light >/dev/null 2>&1
"$TMUX_REAL" -L "$FZGS" set -g @thm_accent_light '#0b0c0d' >/dev/null 2>&1
fzc=$(env -u TMUX -u TMUX_PANE PATH="$FZGB" \
        "$BASH" -c '. "$1" && cockpit_fzf_args' _ "$FZLIB" 2>/dev/null | grep '^--color=')
is "under theme light the pickers read the light roles and defaults" \
   "fg:#1b1e24 hl:#9a4c00 bg+:#e1e5ec prompt:#0b0c0d" \
   "$(printf '%s\n' "$fzc" | tr ',' '\n' | grep -E '^(fg|hl|bg\+|prompt):' | tr '\n' ' ' | sed 's/ $//')"
"$TMUX_REAL" -L "$FZGS" kill-server >/dev/null 2>&1

# Read off tm's own argv, not composed here: nothing in scripts/ decided the
# order in the composed form, so no production change could turn it red.
fzlast=$(printf '%s\n' "$fzk" | grep -- '^--color=' | tail -1)
is "the last --color fzf sees is the caller's, not the theme's" \
   "--color=prompt:red,pointer:red,marker:red" "$fzlast"

fzh=$(env -u TMUX "$DIR/scripts/tm" -h 2>&1)
is "tm -h names the picker's ^X and no longer a k kill key" "yes no" \
   "$(case "$fzh" in *'^X'*) printf yes ;; *) printf no ;; esac
      case "$fzh" in *'k kill'*) printf ' yes' ;; *) printf ' no' ;; esac)"

# `select` writes its menu and its PS3 prompt to stderr, so the sentence that
# introduces the menu belongs on stderr too: split across two streams, one
# redirect separates the question from its own answers.
fznofzf() {                   # fznofzf <tag> <cmd...> -- which stream got the prompt
    local tag=$1 o e; shift
    env -u TMUX -u TMUX_PANE PATH="$FZNB" "$@" </dev/null \
        >"$FZTD/o-$tag" 2>"$FZTD/e-$tag"
    grep -qF "$FZWANT" "$FZTD/o-$tag" && o=out || o=""
    grep -qF "$FZWANT" "$FZTD/e-$tag" && e=err || e=""
    # Neither stream carried the prompt: say what stderr did carry (#128).
    [ -n "$o$e" ] || printf 'stderr=[%s]\n' "$(tail -c 200 "$FZTD/e-$tag" | tr '\n' ' ')"
    printf '%s%s\n' "$o" "$e"
}
FZWANT='Kill which session'
is "tm -k without fzf prompts on stderr, where the menu goes" "err" \
   "$(fznofzf tmk "$DIR/scripts/tm" -k)"
FZWANT='Sessions:'
is "tm without fzf prompts on stderr" "err" \
   "$(fznofzf tma "$DIR/scripts/tm")"
FZWANT='Switch branch in'
is "tmux-branch without fzf prompts on stderr" "err" \
   "$(fznofzf tmb "$DIR/scripts/tmux-branch" "$FZG")"
FZWANT='Open which path'
is "tmux-open-path without fzf prompts on stderr" "err" \
   "$(fznofzf tmo "$DIR/scripts/tmux-open-path" --pick "$PANE")"

tmux set-option -pu -t "$PANE" @cockpit_click_line 2>/dev/null
FZNS="cockpit-fzfnotice-$$"; SOCKS="$SOCKS $FZNS"
env -u TMUX -u TMUX_PANE PATH="$fzold:$PATH" "$TMUX_REAL" -L "$FZNS" -f /dev/null \
    new-session -d 'sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FZNS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
fzm1=$("$TMUX_REAL" -L "$FZNS" show-messages 2>/dev/null | grep -cF "fzf 0.44.1 found; tmux-cockpit's pickers need 0.73+, bootstrap.sh fetches it")
"$TMUX_REAL" -L "$FZNS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
fzm2=$("$TMUX_REAL" -L "$FZNS" show-messages 2>/dev/null | grep -c 'pickers need 0.73+')
is "the old-fzf notice fires once per server, not again on a reload" "1 1" "$fzm1 $fzm2"
"$TMUX_REAL" -L "$FZNS" kill-server >/dev/null 2>&1
FZNS2="cockpit-fzfquiet-$$"; SOCKS="$SOCKS $FZNS2"
env -u TMUX -u TMUX_PANE PATH="$fznew:$PATH" "$TMUX_REAL" -L "$FZNS2" -f /dev/null \
    new-session -d 'sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FZNS2" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "and a 0.73+ fzf is never told anything" "0" \
   "$("$TMUX_REAL" -L "$FZNS2" show-messages 2>/dev/null | grep -c 'pickers need 0.73+')"
"$TMUX_REAL" -L "$FZNS2" kill-server >/dev/null 2>&1
rm -rf "$FZTD" "$FZG"
# --- #105 review rows ---------------------------------------------------------
# Width-aware review and fleet rows, the tab-safe pane record, and the delivery
# notice that names the configured jump key. Every width here comes from a FAKE
# tput, so a column count is a fixture rather than whatever terminal the suite
# happens to run in.
rr_has() {                    # rr_has <argv-file> <flag> <what>
    if grep -qxF -- "$2" "$1" 2>/dev/null; then ok "$3 is given $2"
    else bad "$3 is given $2" "$2 in its argv" "$(tr '\n' ' ' < "$1" 2>/dev/null)"; fi
}
# Both fakes, and the dir they live in, for every block below that drives a
# picker. A FUNCTION, not three blocks sharing one $RRB: tests/par.sh gives a
# block its own shard carrying the prelude and the top-level functions and
# nothing else, so the shared variable was unbound there and the theme block
# went 20 red under `make check-par` while serial `make check` stayed green
# (#120). Idempotent, so the second and third callers cost one `[ -x ]`.
rr_fakebin() {                # -> $RRB, a bin dir whose tput and fzf are fakes
    RRB="$STATE/rr-bin-$$"
    [ -x "$RRB/fzf" ] && return 0
    mkdir -p "$RRB"
cat > "$RRB/tput" <<'RRTPUT'
#!/bin/sh
case "$1" in
    cols)  printf '%s\n' "${RR_COLS:-80}" ;;
    lines) printf '%s\n' "${RR_LINES:-30}" ;;
    *)     exit 1 ;;
esac
RRTPUT
# A fake fzf: it records the rows it was handed and its own argv, then exits
# 130 (esc) so a picker loop returns instead of waiting for a human.
cat > "$RRB/fzf" <<'RRFZF'
#!/bin/sh
# The theme lib probes the version before it builds an argv, and that probe
# must not read the picker's stdin -- it swallowed the next typed line.
case "$1" in --version)
    [ -n "${RR_VLOG:-}" ] && printf 'v\n' >> "$RR_VLOG"
    printf '%s\n' "${RR_FZF_VERSION:-0.74.3 (devel)}"; exit 0 ;;
esac
n=$(cat "$RR_CAP.n" 2>/dev/null || echo 0); n=$((n + 1)); printf '%s\n' "$n" > "$RR_CAP.n"
for a in "$@"; do printf '%s\n' "$a"; done > "$RR_CAP.argv.$n"
cat > "$RR_CAP.stdin.$n"
[ -f "$RR_CAP.out.$n" ] && cat "$RR_CAP.out.$n"
exit "$(cat "$RR_CAP.rc.$n" 2>/dev/null || echo 130)"
RRFZF
    chmod +x "$RRB/tput" "$RRB/fzf"
}

# Keyed on a fixed path, not on whether $RRDIR is already set: another block
# in the same shard uses that name for a repo of its own, and returning early
# on it would hand this one a tree with no long path in it.
rr_repo() {                   # -> $RRDIR, a repo whose one changed file has a long path
    RRDIR="$STATE/rr-repo-$$"
    [ -d "$RRDIR/.git" ] && return 0
    mkdir -p "$RRDIR"
    (
      cd "$RRDIR" || exit
      export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
      git init -q -b main .
      mkdir -p scripts/lib
      printf 'a\n' > scripts/lib/very_long_module_name_here.sh
      git add -A; git commit -q -m init
      printf 'a\nb changed to something quite long indeed\n' > scripts/lib/very_long_module_name_here.sh
    ) >/dev/null 2>&1
}

echo "== #105: fit_field cuts whole characters, whatever the locale is =="
# ${s:i:n} counts BYTES unless the locale is UTF-8, so under C the head cut
# fell through the ⑂ and left a lone continuation byte the terminal draws as a
# replacement glyph. Run against the real function, on the owner's own
# 37-column repo⑂branch, in the locale that breaks it.
RRFIT="$STATE/rr-fit-$$.sh"
sed -n '/^fit_field() {/,/^}/p' "$DIR/scripts/lib/review-fleet.sh" > "$RRFIT"
rr_fit() { LC_ALL=C bash -c '. "$1"; shift; fit_field "$@"' _ "$RRFIT" "$@"; }
RRLOC='research-mobile-97⑂research/mobile-97'
is "a head cut landing inside the ⑂ drops it rather than half of it" \
   "…research/mobile-97" "$(rr_fit 20 "$RRLOC" head)"
is "a head cut clearing the ⑂ keeps it whole" \
   "…⑂research/mobile-97" "$(rr_fit 22 "$RRLOC" head)"
is "a tail cut leaves no half character either" \
   "research-mobile-97…" "$(rr_fit 19 "$RRLOC")"
is "and a field that fits is returned untouched" "$RRLOC" "$(rr_fit 40 "$RRLOC" head)"

echo "== #105: the fleet's ± column survives a narrow popup and repo⑂branch gives way first =="
if need_tmux "#105 fleet widths"; then :; else
RRF="cockpit-rrf-$$"; SOCKS="$SOCKS $RRF"
RRW=$(mktmpd)
rr_git() {                    # rr_git <dir> <branch>  a repo with one uncommitted line
    mkdir -p "$1"
    (
      cd "$1" || exit
      export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
      git init -q -b "$2" .
      printf 'a\n' > f; git add -A; git commit -q -m init
      printf 'b\n' >> f
    ) >/dev/null 2>&1
}
rr_trim() { local s=$1; printf '%s' "${s%"${s##*[! ]}"}"; }
# The owner's own worktree name: the 37-column loc field the mobile lane
# measured as needing 100 columns today (#97 verified.md, inventory claim 4).
rr_git "$RRW/research-mobile-97" research/mobile-97
rr_fakebin

"$TMUX_REAL" -L "$RRF" -f /dev/null new-session -d -s t -x 200 -y 40 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RRF" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
RRCTL=$("$TMUX_REAL" -L "$RRF" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$RRF" new-window -t t: -n review-rows-fixture -c "$RRW/research-mobile-97" \
    'exec -a claude bash' >/dev/null 2>&1
RRAG=$("$TMUX_REAL" -L "$RRF" list-panes -t t:1 -F '#{pane_id}')
rr_cmds() { "$TMUX_REAL" -L "$RRF" list-panes -s -t t: -F '#{pane_current_command}' | sort | tr '\n' ' '; }
until_out 'bash claude ' rr_cmds
# The widest shape the queue really renders: "◆ needs-input" is 13 columns.
"$TMUX_REAL" -L "$RRF" set-option -p -t "$RRAG" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$RRF" set-option -p -t "$RRAG" @cockpit_review pending >/dev/null 2>&1

rr_queue() {                  # rr_queue <cols>  -> $RRW/cap-<cols>.stdin.1 holds the rows
    local cap="$RRW/cap-$1"
    rm -f "$cap".*
    "$TMUX_REAL" -L "$RRF" send-keys -t "$RRCTL" \
        "PATH='$RRB':\$PATH RR_COLS=$1 RR_CAP='$cap' RR_VLOG='$cap.vlog' '$DIR/scripts/tmux-review' -p $RRCTL; echo rr-done > '$cap.done'" Enter
    until_file "$cap.done" '*rr-done*'
}
rr_field() { awk -F'\t' -v n="$2" 'NR == 1 { print $n; exit }' "$1"; }
rr_queue 100; RRC100="$RRW/cap-100.stdin.1"
rr_queue 60;  RRC60="$RRW/cap-60.stdin.1"
if [ -s "$RRC100" ] && [ -s "$RRC60" ]; then
    rr_stat100=$(rr_field "$RRC100" 5); rr_stat60=$(rr_field "$RRC60" 5)
    rr_loc100=$(rr_field "$RRC100" 4); rr_loc60=$(rr_field "$RRC60" 4)
    is "the ± field reads the same at 60 columns as at 100: it never gives way" "$rr_stat100" "$rr_stat60"
    is "and at 100 it is the tree's real diff stat" "+1 -0" "$rr_stat100"
    is "repo⑂branch is whole at 100" "research-mobile-97⑂research/mobile-97" "$(rr_trim "$rr_loc100")"
    is "repo⑂branch is the field that gives way at 60" "yes" \
       "$( [ "$(ncols "$rr_loc60")" -lt "$(ncols "$rr_loc100")" ] && echo yes \
            || echo "no: $(ncols "$rr_loc60") columns at 60, $(ncols "$rr_loc100") at 100" )"
    # The point of a width-aware row: it fits. fzf keeps 2 columns for its
    # pointer, and --tabstop=1 makes each delimiter exactly one column.
    rr_w60=0
    while IFS= read -r rr_f; do rr_w60=$(( rr_w60 + $(ncols "$rr_f") + 1 )); done \
        <<<"$(cut -f2- "$RRC60" | head -1 | tr '\t' '\n')"
    rr_w60=$(( rr_w60 - 1 ))
    is "the whole row fits the 58 columns fzf has at 60" "yes" \
       "$( [ "${rr_w60:-999}" -le 58 ] && echo yes || echo "no: $rr_w60 columns" )"
    # --tabstop=1 is what makes those widths the real columns: without it fzf
    # lays the rejoined fields on an 8-column grid and pad_field decides
    # nothing (#105 defect 5). --ansi is what lets a row dim at all (defect 8).
    rr_argv="$RRW/cap-100.argv.1"
    for rr_flag in --ansi --tabstop=1; do
        if grep -qxF -- "$rr_flag" "$rr_argv" 2>/dev/null; then ok "the fleet picker is given $rr_flag"
        else bad "the fleet picker is given $rr_flag" "$rr_flag in its argv" "$(tr '\n' ' ' < "$rr_argv")"; fi
    done
    is "and one --color, the shared theme's" "1" "$(grep -c '^--color=' "$rr_argv" 2>/dev/null)"
    is "the user's own options still come last, so they win" "yes" \
       "$( [ "$(grep -n '^--color=' "$rr_argv" | cut -d: -f1)" -lt "$(grep -c . "$rr_argv")" ] && echo yes || echo no )"
    # shellcheck disable=SC2002  # cat, so a missing log still counts 0
    is "a fleet queue open never probes fzf --version" "0" \
       "$(cat "$RRW/cap-100.vlog" 2>/dev/null | grep -c .)"
    # The footer draws its own rule; --header-border=bottom is the matching one
    # above the list. Two rules, zero boxes (pickers.md §6).
    rr_has "$rr_argv" --header-border=bottom "the fleet picker"
else
    bad "the fleet queue hands its rows to fzf under a fake tput" "a capture at 60 and at 100" \
        "100:[$(cat "$RRC100" 2>/dev/null)] 60:[$(cat "$RRC60" 2>/dev/null)]"
fi
"$TMUX_REAL" -L "$RRF" kill-server >/dev/null 2>&1
fi

echo "== #105: a cwd carrying a tab or a newline still renders one fleet row, @cockpit_dir intact =="
if need_tmux "#105 fleet field framing"; then :; else
RRS="cockpit-rrs-$$"; SOCKS="$SOCKS $RRS"
RRSW=$(mktmpd)
rr_git2() {                   # this block's own fixture builder
    mkdir -p "$1"
    (
      cd "$1" || exit
      export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
      git init -q -b "$2" .
      printf 'a\n' > f; git add -A; git commit -q -m init
      printf 'b\n' >> f
    ) >/dev/null 2>&1
}
rr_git2 "$RRSW/tabtree" main
rr_git2 "$RRSW/nltree"  main
RRTABCWD="$RRSW/plain/sub"$'\t'"dir"
RRNLCWD="$RRSW/plain/sub"$'\n'"dir"
mkdir -p "$RRTABCWD" "$RRNLCWD"
"$TMUX_REAL" -L "$RRS" -f /dev/null new-session -d -s t -x 200 -y 40 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RRS" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
RRSCTL=$("$TMUX_REAL" -L "$RRS" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$RRS" new-window -t t: -n tabpane -c "$RRTABCWD" 'exec -a claude bash' >/dev/null 2>&1
RRTABP=$("$TMUX_REAL" -L "$RRS" list-panes -t t:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$RRS" new-window -t t: -n nlpane -c "$RRNLCWD" 'exec -a claude bash' >/dev/null 2>&1
RRNLP=$("$TMUX_REAL" -L "$RRS" list-panes -t t:2 -F '#{pane_id}')
rr_cmds2() { "$TMUX_REAL" -L "$RRS" list-panes -s -t t: -F '#{pane_current_command}' | sort | tr '\n' ' '; }
until_out 'bash claude claude ' rr_cmds2
# The tree each agent really works in (#49). The option is the LAST field on
# the wire, so it is the field a tab in the cwd shifts into.
"$TMUX_REAL" -L "$RRS" set-option -p -t "$RRTABP" @cockpit_dir "$RRSW/tabtree" >/dev/null 2>&1
"$TMUX_REAL" -L "$RRS" set-option -p -t "$RRNLP"  @cockpit_dir "$RRSW/nltree"  >/dev/null 2>&1
RRSOUT="$RRSW/fleet.out"
"$TMUX_REAL" -L "$RRS" send-keys -t "$RRSCTL" \
    "'$DIR/scripts/tmux-review' --fleet > '$RRSOUT' 2>&1; echo rr-done > '$RRSW/fleet.done'" Enter
until_file "$RRSW/fleet.done" '*rr-done*'
is "two agent panes, two rows: the newline in a cwd splits no record" "2" \
   "$(grep -c '^%' "$RRSOUT" 2>/dev/null)"
is "the tab-cwd pane reads its @cockpit_dir tree, not a path cut at the tab" "tabtree⑂main" \
   "$(awk -F'\t' -v p="$RRTABP" '$1 == p { print $4; exit }' "$RRSOUT")"
is "and its ± comes from that tree" "+1 -0" \
   "$(awk -F'\t' -v p="$RRTABP" '$1 == p { print $5; exit }' "$RRSOUT")"
is "the newline-cwd pane reads its @cockpit_dir tree too" "nltree⑂main" \
   "$(awk -F'\t' -v p="$RRNLP" '$1 == p { print $4; exit }' "$RRSOUT")"
"$TMUX_REAL" -L "$RRS" kill-server >/dev/null 2>&1
fi

echo "== #105: the delivery notice names the configured jump key and fits 72 columns =="
if need_tmux "#105 jump key notice"; then :; else
RRJ="cockpit-rrj-$$"; SOCKS="$SOCKS $RRJ"
"$TMUX_REAL" -L "$RRJ" -f /dev/null new-session -d -s t 'sleep 600' >/dev/null 2>&1
# Rendered by tmux itself, not read: a format that looks right in a grep can
# still expand to the wrong text -- which is exactly defect 1.
RRJFMT=$(sed -n "s/^ *local jump='\(.*\)'\$/\1/p" "$DIR/scripts/lib/review-deliver.sh" | head -1)
RRJMSG=$(sed -n 's/^ *local notice_paste="\(.*\)"$/\1/p' "$DIR/scripts/lib/review-deliver.sh" | head -1)
if [ -z "$RRJFMT" ] || [ -z "$RRJMSG" ]; then
    bad "the notice builds its jump clause from a format" "local jump=... and local notice_paste=..." \
        "jump:[$RRJFMT] notice:[$RRJMSG]"
else
    rr_notice() {             # rr_notice <key-or-unset> -> the notice as a client sees it
        if [ "$1" = unset ]; then "$TMUX_REAL" -L "$RRJ" set -gu @cockpit_jump_key >/dev/null 2>&1
        else "$TMUX_REAL" -L "$RRJ" set -g @cockpit_jump_key "$1" >/dev/null 2>&1; fi
        local m=${RRJMSG//\$jump/$RRJFMT}
        "$TMUX_REAL" -L "$RRJ" display-message -p "${m//\$where/work:2 api}"
    }
    case "$(rr_notice x)" in
        *"prefix x"*) ok "with @cockpit_jump_key x the notice says prefix x" ;;
        *) bad "with @cockpit_jump_key x the notice says prefix x" "prefix x" "$(rr_notice x)" ;;
    esac
    case "$(rr_notice off)" in
        *prefix*) bad "with the key off the clause drops out rather than naming an unbound key" \
                      "no prefix clause" "$(rr_notice off)" ;;
        *) ok "with the key off the clause drops out rather than naming an unbound key" ;;
    esac
    case "$(rr_notice unset)" in
        *"prefix u"*) ok "unset, it names the shipped default u, never the pre-#43 J" ;;
        *) bad "unset, it names the shipped default u, never the pre-#43 J" "prefix u" "$(rr_notice unset)" ;;
    esac
    is "and the whole notice fits the 72 columns an 80-column client shows" "yes" \
       "$( [ "$(ncols "$(rr_notice unset)")" -le 72 ] && echo yes \
            || echo "no: $(ncols "$(rr_notice unset)") columns" )"
fi
"$TMUX_REAL" -L "$RRJ" kill-server >/dev/null 2>&1
fi

echo "== #105: one message prefix, and the popup refusals speak through tmux =="
# On this theme a display-message is welded into status-right with no
# separator, so there is one prefix for the whole plugin. The CLI entry paths
# keep printing their own name to stderr: there stderr is the right stream.
# Continuations joined first, then the -p queries dropped: what is left is
# every message this script draws on a status line.
rr_old=$(rv_src | sed -e ':a' -e '/\\$/{N;s/\\\n//;ta}' \
    | grep 'display-message' | grep -v 'display-message -p' | grep -c 'tmux-review:')
is "no status-line message still says tmux-review:" "0" "$rr_old"
rr_cli=$(rv_src | grep -c "printf 'tmux-review:")
is "the CLI paths still name the program on stderr" "yes" \
   "$( [ "$rr_cli" -ge 4 ] && echo yes || echo "no: $rr_cli" )"
# printf >&2 inside display-popup -E renders to nobody. These two refusals are
# the only ones that fire from inside the popup with no fzf there to draw them.
rr_nofzf() {                  # rr_nofzf <args...> -> the tmux calls the script made
    local d="$STATE/rr-nofzf-$$"
    rm -rf "$d"; mkdir -p "$d/bin"
    cat > "$d/bin/tmux" <<RRNOFZF
#!/bin/sh
printf '%s\n' "\$*" >> "$d/log"
exit 0
RRNOFZF
    chmod +x "$d/bin/tmux"
    # Everything the script reaches before the refusal, and nothing else:
    # `command -v fzf` and `command -v revdiff` have to come up empty, and only PATH decides that.
    local t bp
    for t in bash sh basename dirname cat sed awk grep git mktemp readlink id tput; do
        bp=$(command -v "$t") && ln -sf "$bp" "$d/bin/$t"
    done
    ( PATH="$d/bin"; export PATH; "$DIR/scripts/tmux-review" "$@" >/dev/null 2>&1 )
    cat "$d/log" 2>/dev/null
    rm -rf "$d"
}
rr_repo
case "$(rr_nofzf --loop "$RRDIR" "$STATE/rr-nofzf-store")" in
    *"display-message cockpit: "*) ok "the review loop's no-revdiff refusal is a prefixed display-message" ;;
    *) bad "the review loop's no-revdiff refusal is a prefixed display-message" "display-message cockpit: ..." \
         "$(rr_nofzf --loop "$RRDIR" "$STATE/rr-nofzf-store")" ;;
esac
case "$(rr_nofzf -p %0)" in
    *"display-message cockpit: "*) ok "so is the fleet queue's" ;;
    *) bad "so is the fleet queue's" "display-message cockpit: ..." "$(rr_nofzf -p %0)" ;;
esac

# --- #124 config leftovers ---
echo "== a window titled after the host still gets the tab ladder (#124) =="
# An agent pane that never set a title keeps the host as pane_title; its tab
# shows the window name instead, and that name goes through the same budget.
if need_tmux "host-title tab ladder"; then :; else
HT="cockpit-ht124-$$"; SOCKS="$SOCKS $HT"
HTN=abcdefghijabcdefghijabcdef          # 26 columns: over the 18 an 80-column
                                        # client affords, under the 28 cap at 160
"$TMUX_REAL" -L "$HT" -f /dev/null new-session -d -s s -n "$HTN" -x 200 -y 50 'exec -a claude sleep 300' >/dev/null 2>&1
htp=$("$TMUX_REAL" -L "$HT" list-panes -t s: -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$HT" select-pane -t "$htp" -T "$("$TMUX_REAL" -L "$HT" display-message -p '#{host_short}')" >/dev/null 2>&1
"$TMUX_REAL" -L "$HT" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$HT" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
TMUX="$SOCKDIR/$HT,0,0" "$DIR/scripts/agent-state" sweep >/dev/null 2>&1   # the load's sweep, settled
read -r htc htf htpid <<<"$(cw_open "$HT" s)"
if [ -z "${htc:-}" ]; then skip_ "the host-title tab ladder (no control client)"; else
httab() { "$TMUX_REAL" -L "$HT" display-message -p -c "$htc" -t s: '#{E:window-status-current-format}' 2>/dev/null | plain; }
cw_size "$HT" "$htc" 80
is "an 80-column client cuts the host-titled tab to its budget" " 0 abcdefghijabcdefgh… ○ " "$(httab)"
cw_size "$HT" "$htc" 160
is "a 160-column client draws the whole name" " 0 $HTN ○ " "$(httab)"
fi
cw_close "${htpid:-}" "${htf:-}"
"$TMUX_REAL" -L "$HT" kill-server >/dev/null 2>&1
fi

echo "== every tm notice says which plugin is talking (#124) =="
# Freeze 1's cockpit: prefix. #107's two tm notices predate it.
is "tm announces the restore under the prefix" "1" \
   "$(grep -c "printf 'cockpit: Restoring saved sessions" "$DIR/scripts/tm")"
is "and says who found no sessions" "1" \
   "$(grep -c "printf 'cockpit: No tmux sessions running" "$DIR/scripts/tm")"

# --- #125 suite leftovers ---------------------------------------------------
echo "== #125: a killed fleet scan takes its temp dir with it =="
# fleet_rows() mktemps a directory for its root-N/scan-N/idx.PID files and
# removed it only where it RETURNS. The popup is killed far more often than it
# returns, so a suite run left one of those dirs in /tmp every time.
if need_tmux "the fleet scan's temp dir"; then :; else
LKD=$(mktmpd)                     # the run's own TMPDIR, so the count is ours
LKB=$(mktmpd)                     # shims: a private tmux, a git that blocks
LKG=$(mktmpd)                     # somewhere for the panes to sit
LKGO="$LKB/go"; LKSIG="$LKD/sig"
LKS="cockpit-leak-$$"; SOCKS="$SOCKS $LKS"
"$TMUX_REAL" -L "$LKS" -f /dev/null new-session -d -s lk -c "$LKG" \
    'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$LKS" split-window -d -t lk -c "$LKG" \
    'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$LKS" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
printf '#!/bin/sh\nexec %s -L %s "$@"\n' "$TMUX_REAL" "$LKS" > "$LKB/tmux"
# git blocks until the block says go, which holds the scan open long enough to
# kill it. The 30s ceiling is a hang guard nothing reads: without one a shim
# whose go file went with the suite's own cleanup spins forever. Seen once.
{ printf '#!/bin/sh\ni=0\n'
  printf 'while [ ! -e "$LK_GO" ] && [ $i -lt 300 ]; do i=$((i+1)); sleep 0.1; done\n'
  printf 'exec %s "$@"\n' "$(command -v git)"; } > "$LKB/git"
# The EXIT trap's rm records the signals it inherited, which is the only place
# the handler's self-disarm is visible: both codes remove the directory.
{ printf '#!/bin/sh\n'
  printf 'grep SigIgn /proc/self/status >> "$LK_SIG"\n'
  printf 'exec %s "$@"\n' "$(command -v rm)"; } > "$LKB/rm"
chmod +x "$LKB/tmux" "$LKB/git" "$LKB/rm"
lkdirs() { find "$LKD" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' '; }
until_out '*claude*claude*' "$TMUX_REAL" -L "$LKS" list-panes -a -F '#{pane_current_command}'
TMPDIR="$LKD" LK_GO="$LKGO" LK_SIG="$LKSIG" PATH="$LKB:$PATH" \
    "$DIR/scripts/tmux-review" --fleet >/dev/null 2>&1 &
LKPID=$!
lkseen=1; until_out '1' lkdirs || lkseen=0
# Twice, the shape tmux uses when a pane goes. Two signals sent from here are
# coalesced, so what pins the self-disarm is the mask below, not this.
kill -TERM "$LKPID" 2>/dev/null; kill -TERM "$LKPID" 2>/dev/null
# Polled, and the -9 only AFTER it: a signal handler that cleans up but forgets
# to exit leaves the scan running, and a bare `wait` on that hangs the suite.
lkgone=1; until_out '0' lkdirs || lkgone=0
kill -9 "$LKPID" 2>/dev/null
wait "$LKPID" 2>/dev/null
: > "$LKGO"                       # release the blocked git shims
is "the scan's temp dir is there to kill, and gone once it is killed" "1 1" \
   "$lkseen $lkgone"
# 0x4003 is HUP|INT|TERM. Without the handler's disarm this reads 0 while the
# directory still goes, which is why the assertion above cannot see it.
lksig() {                     # the mask the rm inherited, 0 if it recorded none
    local m; m=$(awk '{print $2}' "$LKSIG" 2>/dev/null | head -1)
    printf '%x' $(( 0x${m:-0} & 0x4003 ))
}
is "the EXIT trap's rm runs with HUP, INT and TERM ignored" "4003" "$(lksig)"
"$TMUX_REAL" -L "$LKS" kill-server >/dev/null 2>&1
fi

echo "== #125: ^G shows the diff and ^F the content, in both open-path pickers =="
# The other half of defect 14. Both fzf calls in tmux-open-path carry the pair;
# the argv is read off a fake fzf, the same shape the #104 block uses.
OPD=$(mktmpd)
fzfake "$OPD/bin" "0.44.1 (debian)"
: > "$OPD/opened.txt"
OPWAS=$(tmux show-option -pqv -t "$PANE" @cockpit_click_line 2>/dev/null)
tmux set-option -p -t "$PANE" @cockpit_click_line "see $OPD/opened.txt:1" 2>/dev/null
oprun() {                     # oprun <tag> <tmux-open-path args...>
    local tag=$1; shift
    env -u TMUX -u TMUX_PANE PATH="$OPD/bin:$PATH" FZ_ARGV="$OPD/argv-$tag" \
        "$DIR/scripts/tmux-open-path" "$@" </dev/null >/dev/null 2>&1
}
opkeys() {                    # opkeys <tag> -> "<ctrl-g binds> <ctrl-f binds>"
    local a; a=$(cat "$OPD"/argv-"$1".* 2>/dev/null)
    printf '%s %s' \
        "$(printf '%s\n' "$a" | grep -c '^ctrl-g:.*change-preview-label( diff )$')" \
        "$(printf '%s\n' "$a" | grep -c '^ctrl-f:.*change-preview-label( content )$')"
}
oprun browse --browse "$OPD"
oprun pick   --pick "$PANE"
is "the folder browser binds ^G to the diff preview and ^F back to content" "1 1" \
   "$(opkeys browse)"
is "and so does the open picker"                                            "1 1" \
   "$(opkeys pick)"
if [ -n "$OPWAS" ]; then tmux set-option -p -t "$PANE" @cockpit_click_line "$OPWAS" 2>/dev/null
else tmux set-option -pu -t "$PANE" @cockpit_click_line 2>/dev/null; fi

av_setup() {                  # the fixtures every agent-view block needs; each calls it first
    [ -n "${AVBIN:-}" ] && return 0
    AVF="$DIR/tests/fixtures/agent-view"
    AVBIN=$(mktmpd); ln -s "$AVF/claude" "$AVBIN/claude"
    AVREC=$(mktmpd); AVLOG="$AVREC/calls"; : > "$AVLOG"
    cat > "$AVREC/tmux" <<SHIMEOF
#!/bin/sh
printf '%s\n' "\$*" >> "$AVLOG"
exec "$TMUX_REAL" "\$@"
SHIMEOF
    chmod +x "$AVREC/tmux"
}

echo "== agent-view: a background worker's hooks touch nothing (#174) =="
av_setup
# A hook in an agent-view background worker (CLAUDE_CODE_SESSION_KIND=bg) has no TMUX_PANE and
# the pane belongs to the front end, which relays its own notifications. The worker's hook
# exits 0 and never reaches tmux.
av_bg() {                     # av_bg <script> [args] -> "<rc> <tmux calls>", payload on stdin
    : > "$AVLOG"
    env -u TMUX -u TMUX_PANE CLAUDE_CODE_SESSION_KIND=bg PATH="$AVREC:$PATH" \
        XDG_STATE_HOME="$AVREC/state" "$@" >/dev/null 2>&1
    printf '%s %s' "$?" "$(wc -l < "$AVLOG" | tr -d ' ')"
}
AVPAY='{"notification_type":"agent_needs_input","message":"m","session_id":"s"}'
is "agent-state running in a bg worker: exit 0, no tmux call" "0 0" \
   "$(av_bg "$DIR/scripts/agent-state" running </dev/null)"
is "agent-state notify in a bg worker: exit 0, no tmux call" "0 0" \
   "$(printf '%s' "$AVPAY" | av_bg "$DIR/scripts/agent-state" notify)"
is "agent-state off in a bg worker: exit 0, no tmux call" "0 0" \
   "$(av_bg "$DIR/scripts/agent-state" off </dev/null)"
is "review-gate Stop in a bg worker: exit 0, no tmux call" "0 0" \
   "$(printf '%s' "$AVPAY" | av_bg "$DIR/scripts/review-gate")"
is "agent-context in a bg worker: exit 0, no tmux call" "0 0" \
   "$(av_bg "$DIR/scripts/agent-context" < "$DIR/tests/fixtures/statusline.json")"
# --concede is the agent's own call, not a hook: it still says it recorded nothing.
is "review-gate --concede in a bg worker still refuses, with no tmux call" "1 0" \
   "$(av_bg "$DIR/scripts/review-gate" --concede why </dev/null)"

echo "== agent-view: agent-poll --once writes the worst background state (#174) =="
av_setup
av_srv() {                    # av_srv <tag>: a private server; sets AV AVT AVP
    AV="cockpit-av-$$-$1"; SOCKS="$SOCKS $AV"
    "$TMUX_REAL" -L "$AV" -f /dev/null new-session -d -s t -x 80 -y 24 \
        'exec -a claude bash' >/dev/null 2>&1
    "$TMUX_REAL" -L "$AV" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
    "$TMUX_REAL" -L "$AV" set-environment -g PATH "$AVBIN:$PATH" >/dev/null 2>&1
    AVT="$SOCKDIR/$AV,0,0"
    AVP=$("$TMUX_REAL" -L "$AV" list-panes -t t -F '#{pane_id}')
}
av_tm() { "$TMUX_REAL" -L "$AV" "$@"; }
avo()   { av_tm show-option -pqv -t "${2:-$AVP}" "$1"; }
av_once() {                   # av_once <fixture.json> [env NAME=value...]: one poll, its exit code
    local j=$1; shift
    env -u TMUX_PANE -u CLAUDE_CODE_SESSION_KIND TMUX="$AVT" PATH="$AVBIN:$PATH" \
        AV_JSON="$AVF/$j" "$@" "$DIR/scripts/agent-poll" --once </dev/null >/dev/null 2>&1
    echo $?
}
if need_tmux "agent-view poll"; then :; else
av_srv once
AVP2=$(av_tm new-window -d -P -F '#{pane_id}' 'exec -a claude bash')
AVP3=$(av_tm new-window -d -P -F '#{pane_id}' 'exec -a claude bash')
AVPS=$(av_tm new-window -d -P -F '#{pane_id}' 'sleep 300')
av_tm set-option -p -t "$AVP" @cockpit_agentview 1
av_tm set-option -p -t "$AVP2" @cockpit_agentview 1
av_tm set-option -p -t "$AVPS" @cockpit_agentview 1
av_row() {                    # av_row <name> <fixture> <state before> <want state> <want msg>
    if [ "$3" = needs-input ]; then av_tm set-option -p -t "$AVP" @panestate_msg "stale ask"
    else av_tm set-option -pu -t "$AVP" @panestate_msg; fi
    if [ -n "$3" ]; then av_tm set-option -p -t "$AVP" @panestate "$3"
    else av_tm set-option -pu -t "$AVP" @panestate; fi
    av_once "$2" >/dev/null
    is "$1" "$4|$5" "$(avo @panestate)|$(avo @panestate_msg)"
}
av_row "a blocked session -> needs-input, with its name in the ask" blocked.json "" \
    needs-input "fix the parser needs you"
av_row "all done clears needs-input: the poller is not held by stickiness" done.json needs-input \
    done ""
av_row "a failed and a working session -> error" failed-working.json running error ""
av_row "working and done -> running" working.json done running ""
av_row "a stopped session counts as idle: the state is cleared" stopped.json running "" ""
av_row "no background sessions: the state is cleared" none.json needs-input "" ""
av_row "an interactive session's blocked state is not a background one" \
    other-kinds.json running done ""
av_row "an unknown state is ignored: idle" unknown-state.json running "" ""
av_row "a stale ask is dropped once the pane is no longer blocked" \
    working.json needs-input running ""
av_tm set-option -p -t "$AVP" @panestate running
for AVBAD in garbage.json error-object.json; do
    is "$AVBAD: exit 0, no write" "0 running" "$(av_once "$AVBAD") $(avo @panestate)"
done
is "claude exiting 1: exit 0, no write" "0 running" \
   "$(av_once done.json AV_RC=1) $(avo @panestate)"
# Both marked panes are written; a claude pane that is not marked, and a marked pane that is no
# longer an agent, are not.
av_tm set-option -p -t "$AVP" @panestate running
av_once blocked.json >/dev/null
is "every marked agent pane is written" "needs-input needs-input" \
   "$(avo @panestate) $(avo @panestate "$AVP2")"
is "an unmarked agent pane is left alone" "" "$(avo @panestate "$AVP3")"
is "a marked pane that is no longer an agent is left alone" "" "$(avo @panestate "$AVPS")"
is "...and loses its mark, so a later plain session there is not the poller's" "" \
   "$(avo @cockpit_agentview "$AVPS")"
# The ask is bounded and clean whatever the session is named.
av_tm set-option -pu -t "$AVP" @panestate_msg
av_once hostile-name.json >/dev/null
AVMSG=$(avo @panestate_msg)
is "a hostile name is capped at 120 characters" "120" "${#AVMSG}"
case "$AVMSG" in
    "bad [31m name"xxx*) ok "...with its escape and newline flattened" ;;
    *) bad "...with its escape and newline flattened" "bad [31m namexxx..." "$AVMSG" ;;
esac
# Compare-then-set: an unchanged state writes nothing, since every set redraws.
AVSETS="$AVREC/sets"; : > "$AVSETS"
av_tm set-hook -g after-set-option "run-shell 'echo \"#{hook_arguments}\" >> $AVSETS'"
av_once blocked.json >/dev/null; av_once blocked.json >/dev/null
: > "$AVSETS"; av_once blocked.json >/dev/null
is "a poll that changes nothing writes nothing" "0" "$(wc -l < "$AVSETS" | tr -d ' ')"
av_tm set-hook -gu after-set-option
# Without jq, without claude: no write, and no error.
AVMIN=$(mktmpd); AVMIN2=$(mktmpd); AVMIN3=$(mktmpd)
for c in env bash timeout sleep cat; do
    for m in "$AVMIN" "$AVMIN2" "$AVMIN3"; do ln -s "$(command -v $c)" "$m/$c"; done
done
for m in "$AVMIN" "$AVMIN2" "$AVMIN3"; do ln -s "$TMUX_REAL" "$m/tmux"; done
ln -s "$(command -v jq)" "$AVMIN/jq"; ln -s "$(command -v jq)" "$AVMIN3/jq"
ln -s "$AVF/claude" "$AVMIN/claude"; ln -s "$AVF/claude" "$AVMIN2/claude"
av_tm set-option -p -t "$AVP" @panestate running
av_min() {                    # av_min <PATH dir>: a poll with only that PATH -> "<rc> <state>"
    env -u TMUX_PANE TMUX="$AVT" PATH="$1" AV_JSON="$AVF/blocked.json" \
        "$DIR/scripts/agent-poll" --once </dev/null >/dev/null 2>&1
    printf '%s %s' "$?" "$(avo @panestate)"
}
is "no jq on PATH: exit 0, no write" "0 running" "$(av_min "$AVMIN2")"
is "no claude on PATH: exit 0, no write" "0 running" "$(av_min "$AVMIN3")"
is "with jq and claude both on the minimal PATH the poll does write" "0 needs-input" \
   "$(av_min "$AVMIN")"
av_tm kill-server >/dev/null 2>&1
fi

echo "== agent-view: agent-state poll is compare-then-set and recomputes the counters (#174) =="
av_setup
if need_tmux "agent-view poll verb"; then :; else
av_srv verb
av_st() {
    env -u TMUX_PANE TMUX="$AVT" "$DIR/scripts/agent-state" "$@" </dev/null >/dev/null 2>&1
}
av_st needs-input "$AVP"
is "the hook's needs-input reaches the window state" "needs-input" \
   "$(av_tm show-option -wqv -t "$AVP" @cockpit_win_state)"
av_st poll done "$AVP"
is "poll done lifts a sticky needs-input" "done" "$(avo @panestate)"
is "...and the window state follows" "done" "$(av_tm show-option -wqv -t "$AVP" @cockpit_win_state)"
av_tm set-option -p -t "$AVP" @cockpit_review pending
av_st poll running "$AVP"
is "poll running does not drop a pending review (nobody pressed Enter)" "running pending" \
   "$(avo @panestate) $(avo @cockpit_review)"
av_st poll bogus "$AVP"
av_st poll "" "$AVP"
av_st poll
is "an unknown poll state, an empty one and no pane at all are ignored" "running" \
   "$(avo @panestate)"
av_st poll off "$AVP"
is "poll off clears the state and keeps the pending review" " pending" \
   "$(avo @panestate) $(avo @cockpit_review)"
av_tm kill-server >/dev/null 2>&1
fi

echo "== agent-view: agent_needs_input / agent_completed mark the pane, start one poller (#174) =="
av_setup
if need_tmux "agent-view hook" || [ ! -d /proc/self ]; then :; else
av_srv hook
# AV_RC=1: the poller's first tick fails, so only the hook's own writes show.
av_tm set-environment -g AV_RC 1
av_tm set-option -g @cockpit_bg_poll 3600
av_hook() {                   # av_hook <type>: a front end's relayed notification, as a hook
    : > "$AVLOG"
    printf '{"notification_type":"%s","message":"m1"}' "$1" \
        | env TMUX="$AVT" TMUX_PANE="$AVP" PATH="$AVREC:$PATH" \
            "$DIR/scripts/agent-state" notify >/dev/null 2>&1
}
av_runs() { grep -c '^run-shell -b ' "$AVLOG"; }
av_polls() {                  # live agent-poll processes on this server
    local d n=0
    for d in /proc/[0-9]*; do
        [ "$(cat "$d/comm" 2>/dev/null)" = agent-poll ] || continue
        tr '\0' '\n' < "$d/environ" 2>/dev/null | grep -q "^TMUX=$SOCKDIR/$AV," && n=$((n + 1))
    done
    echo "$n"
}
av_wait() {                   # av_wait '<test command>': polls it up to 30s, the box may be loaded
    local i; for i in $(seq 1 300); do eval "$1" && return 0; sleep 0.1; done; return 1
}
av_published() {              # the published pid is a live agent-poll
    local p; p=$(av_tm show-option -gqv @cockpit_poll_pid)
    [ -n "$p" ] && [ "$(cat "/proc/$p/comm" 2>/dev/null)" = agent-poll ]
}
av_hook permission_prompt
is "an ordinary permission prompt is not an agent view: no mark, no poller" "|0" \
   "$(avo @cockpit_agentview)|$(av_runs)"
av_tm set-option -pu -t "$AVP" @panestate
av_hook agent_completed
is "agent_completed marks the pane" "1" "$(avo @cockpit_agentview)"
is "...writes no state of its own" "" "$(avo @panestate)"
is "...and starts the poller, once" "1" "$(av_runs)"
av_wait av_published
is "the poller publishes its own pid, verified by name" "yes" \
   "$(av_published && echo yes || echo no)"
av_hook agent_needs_input
is "agent_needs_input keeps today's write: needs-input and its text" "needs-input m1" \
   "$(avo @panestate) $(avo @panestate_msg)"
is "a second notification with a live poller starts none" "0" "$(av_runs)"
is "...the hook cost one extra tmux read over the plain notify (msg + batch + read)" "3" \
   "$(wc -l < "$AVLOG" | tr -d ' ')"
is "...and exactly one poller is running" "1" "$(av_polls)"
AVPOLLPID=$(av_tm show-option -gqv @cockpit_poll_pid)
kill -TERM "$AVPOLLPID" 2>/dev/null
av_wait '[ -z "$(av_tm show-option -gqv @cockpit_poll_pid)" ]'
is "SIGTERM unpublishes the poller's pid" "" "$(av_tm show-option -gqv @cockpit_poll_pid)"
# A published pid that is not an agent-poll is not trusted, and not signalled.
sleep 300 & AVSLEEP=$!
av_tm set-option -g @cockpit_poll_pid "$AVSLEEP"
av_hook agent_needs_input
is "a stale pid naming another process is not trusted: a poller starts" "1" "$(av_runs)"
av_wait av_published
is "...and takes the publication over" "yes" \
   "$([ "$(av_tm show-option -gqv @cockpit_poll_pid)" != "$AVSLEEP" ] \
        && av_published && echo yes || echo no)"
is "...and the other process was never signalled" "yes" \
   "$(kill -0 "$AVSLEEP" 2>/dev/null && echo yes || echo no)"
kill "$AVSLEEP" 2>/dev/null; wait "$AVSLEEP" 2>/dev/null
kill -TERM "$(av_tm show-option -gqv @cockpit_poll_pid)" 2>/dev/null
av_wait '[ -z "$(av_tm show-option -gqv @cockpit_poll_pid)" ]'
# off means never: no mark, no poller; the needs-input write is today's.
av_tm set-option -g @cockpit_bg_poll off
av_tm set-option -pu -t "$AVP" @cockpit_agentview
av_hook agent_needs_input
is "@cockpit_bg_poll off: no mark and no poller" "|0|needs-input" \
   "$(avo @cockpit_agentview)|$(av_runs)|$(avo @panestate)"
av_tm kill-server >/dev/null 2>&1
fi

echo "== agent-view: the poller owns one slot per server and lets go on every exit path (#174) =="
av_setup
if need_tmux "agent-view poller" || [ ! -d /proc/self ]; then :; else
av_srv life
av_tm set-environment -g AV_JSON "$AVF/done.json"
av_tm set-option -g @cockpit_bg_poll 1
av_tm set-option -p -t "$AVP" @cockpit_agentview 1
av_start() {                  # a poller outside the hook path; its pid in AVPID
    env -u TMUX_PANE TMUX="$AVT" PATH="$AVBIN:$PATH" AV_JSON="$AVF/done.json" \
        "$DIR/scripts/agent-poll" </dev/null >/dev/null 2>&1 &
    AVPID=$!
}
av_gone() { ! kill -0 "$1" 2>/dev/null; }
av_pub() { av_tm show-option -gqv @cockpit_poll_pid; }
av_start; AVA=$AVPID
av_wait '[ "$(av_pub)" = "$AVA" ]'
is "a poller publishes its own pid" "$AVA" "$(av_pub)"
av_wait '[ "$(avo @panestate)" = done ]'
is "...and its first tick has already written" "done" "$(avo @panestate)"
av_start; AVB=$AVPID
av_wait "av_gone \$AVB" || kill -9 "$AVB"; wait "$AVB" 2>/dev/null
is "a second poller finds the slot taken and exits" "$AVA" "$(av_pub)"
is "...without unpublishing the first one's pid on its way out" "yes" \
   "$(kill -0 "$AVA" 2>/dev/null && echo yes || echo no)"
# The slot is released only if it is still ours: a poller whose slot was taken over lets go
# of nothing (the number written here stands for the later poller's).
av_tm set-option -g @cockpit_poll_pid 999999
kill -TERM "$AVA"; av_wait "av_gone \$AVA" || kill -9 "$AVA"; wait "$AVA" 2>/dev/null
is "a poller whose slot was taken over does not release the new owner's pid" "999999" "$(av_pub)"
av_tm set-option -gu @cockpit_poll_pid
av_start; AVA=$AVPID
av_wait '[ "$(av_pub)" = "$AVA" ]'
av_tm respawn-pane -k -t "$AVP" 'sleep 300'
av_wait "av_gone \$AVA" || kill -9 "$AVA"; wait "$AVA" 2>/dev/null
is "when no marked pane is an agent any more the poller exits" "yes" \
   "$(av_gone "$AVA" && echo yes || echo no)"
is "...and unpublishes" "" "$(av_pub)"
av_tm respawn-pane -k -t "$AVP" 'exec -a claude bash'
av_start; AVC=$AVPID
av_wait '[ "$(av_pub)" = "$AVC" ]'
kill -TERM "$AVC"; av_wait "av_gone \$AVC" || kill -9 "$AVC"; wait "$AVC" 2>/dev/null
is "SIGTERM unpublishes" "" "$(av_pub)"
av_start; AVC=$AVPID
av_wait '[ "$(av_pub)" = "$AVC" ]'
kill -HUP "$AVC"; av_wait "av_gone \$AVC" || kill -9 "$AVC"; wait "$AVC" 2>/dev/null
is "SIGHUP unpublishes" "" "$(av_pub)"
av_tm set-option -g @cockpit_bg_poll off
av_start; AVC=$AVPID
av_wait "av_gone \$AVC" || kill -9 "$AVC"; wait "$AVC" 2>/dev/null
is "@cockpit_bg_poll off: the poller never publishes" "" "$(av_pub)"
av_tm kill-server >/dev/null 2>&1
fi

echo "== agent-view: a plain session after an agent-view visit is not the poller's (#174) =="
av_setup
# The mark outlives the visit: the pane's next plain session would have its states overwritten
# every tick. A hook of the pane's own session, other than the relayed agent_* notifications,
# takes the mark away -- in the same tmux invocation, and even when the state would not change.
if need_tmux "agent-view unmark" || [ ! -d /proc/self ]; then :; else
av_srv plain
av_tm set-environment -g AV_RC 1
av_tm set-option -g @cockpit_bg_poll 3600
av_plain() {                  # av_plain <verb> [args]: a hook of the pane's own session
    : > "$AVLOG"
    printf '{"notification_type":"permission_prompt","message":"Claude needs your permission"}' \
        | env TMUX="$AVT" TMUX_PANE="$AVP" PATH="$AVREC:$PATH" \
            "$DIR/scripts/agent-state" "$@" >/dev/null 2>&1
}
av_hook agent_needs_input
av_wait av_published
is "the visit marks the pane" "1 needs-input" "$(avo @cockpit_agentview) $(avo @panestate)"
av_plain notify
is "a plain permission prompt takes the mark away" "|needs-input" \
   "$(avo @cockpit_agentview)|$(avo @panestate)"
is "...with no tmux call beyond the two a permission prompt always made" "2" \
   "$(wc -l < "$AVLOG" | tr -d ' ')"
is "a poll tick then leaves the plain session's needs-input alone" "needs-input" \
   "$(av_once done.json >/dev/null; avo @panestate)"
# Outside `when`: the pane is already in the state the hook would write, and loses the mark.
av_hook agent_completed
is "marked again, already on needs-input" "1 needs-input" \
   "$(avo @cockpit_agentview) $(avo @panestate)"
av_plain notify
is "a plain prompt for a state it already holds still unmarks" "" "$(avo @cockpit_agentview)"
for AVV in running done off; do
    av_tm set-option -p -t "$AVP" @cockpit_agentview 1
    av_plain "$AVV"
    is "a plain $AVV hook unmarks, in its one tmux invocation" "|1" \
       "$(avo @cockpit_agentview)|$(wc -l < "$AVLOG" | tr -d ' ')"
done
# What is not the pane's own session keeps the mark.
av_tm set-option -p -t "$AVP" @cockpit_agentview 1
env -u TMUX_PANE TMUX="$AVT" "$DIR/scripts/agent-state" running "$AVP" </dev/null >/dev/null 2>&1
is "a tmux hook that passes the pane, with no TMUX_PANE, keeps the mark" "1" \
   "$(avo @cockpit_agentview)"
env -u TMUX_PANE TMUX="$AVT" "$DIR/scripts/agent-state" poll done "$AVP" </dev/null >/dev/null 2>&1
is "the poller's own write keeps the mark" "1" "$(avo @cockpit_agentview)"
av_hook agent_needs_input
is "a relayed agent_needs_input keeps the mark" "1" "$(avo @cockpit_agentview)"
av_hook agent_completed
is "a relayed agent_completed keeps the mark" "1" "$(avo @cockpit_agentview)"
AVK=$(av_tm show-option -gqv @cockpit_poll_pid)
kill -TERM "$AVK" 2>/dev/null
av_wait '[ -z "$(av_tm show-option -gqv @cockpit_poll_pid)" ]'
av_tm kill-server >/dev/null 2>&1
fi

echo "== agent-view: the poller slot is a compare-and-set, rechecked each tick (#174) =="
av_setup
if need_tmux "agent-view slot" || [ ! -d /proc/self ]; then :; else
av_srv slot
av_tm set-environment -g AV_JSON "$AVF/done.json"
av_tm set-option -g @cockpit_bg_poll 1
av_tm set-option -p -t "$AVP" @cockpit_agentview 1
AVRACE=$(mktmpd); ln -s "$AVF/tmux-race" "$AVRACE/tmux"
av_startx() {                 # av_startx [NAME=value...]: a poller of our own; pid in AVPID
    env -u TMUX_PANE TMUX="$AVT" PATH="$AVBIN:$PATH" AV_JSON="$AVF/done.json" "$@" \
        "$DIR/scripts/agent-poll" </dev/null >/dev/null 2>&1 &
    AVPID=$!
}
av_gone() { ! kill -0 "$1" 2>/dev/null; }
av_pub() { av_tm show-option -gqv @cockpit_poll_pid; }
av_reap() {                   # av_reap <pid>: 0 if it left by itself; else kills it and returns 1
    local rc=0
    av_wait "av_gone $1" || { rc=1; kill -9 "$1" 2>/dev/null; }
    wait "$1" 2>/dev/null
    return $rc
}
# Another claimant wins between this poller's read and its write (tmux-race does the write).
av_startx PATH="$AVRACE:$AVBIN:$PATH" AV_REAL="$TMUX_REAL" AV_RACE=999999
AVA=$AVPID; av_reap "$AVA" && AVLEFT=yes || AVLEFT=no
is "an empty slot taken by someone else meanwhile: the poller leaves" "yes" "$AVLEFT"
is "...and the slot is still theirs" "999999" "$(av_pub)"
sleep 300 & AVSLEEP=$!
av_tm set-option -g @cockpit_poll_pid "$AVSLEEP"
av_startx PATH="$AVRACE:$AVBIN:$PATH" AV_REAL="$TMUX_REAL" AV_RACE=999999
AVA=$AVPID; av_reap "$AVA" && AVLEFT=yes || AVLEFT=no
is "a stale slot taken by someone else meanwhile: the poller leaves" "yes" "$AVLEFT"
is "...the slot is still theirs" "999999" "$(av_pub)"
is "...and the process the stale pid named was not signalled" "yes" \
   "$(kill -0 "$AVSLEEP" 2>/dev/null && echo yes || echo no)"
kill "$AVSLEEP" 2>/dev/null; wait "$AVSLEEP" 2>/dev/null
# A poller that loses its slot after the claim notices on its next tick.
av_tm set-option -gu @cockpit_poll_pid
av_startx; AVA=$AVPID
av_wait '[ "$(av_pub)" = "$AVA" ]'
av_tm set-option -g @cockpit_poll_pid 999999
av_reap "$AVA" && AVLEFT=yes || AVLEFT=no
is "a poller whose slot was taken over leaves by itself" "yes" "$AVLEFT"
is "...without touching the new owner's pid" "999999" "$(av_pub)"
# A poller that ends normally has no child to stop: its sleep was reaped by `wait`, and a kill
# of that number would hit whatever the kernel handed it next (AGENTS.md). Traced with bash -x,
# since kill is a builtin: after two full ticks the poller is told to stop by the option.
AVTR="$AVREC/trace"; : > "$AVTR"
env -u TMUX_PANE TMUX="$AVT" PATH="$AVBIN:$PATH" AV_JSON="$AVF/done.json" \
    bash -x "$DIR/scripts/agent-poll" </dev/null >/dev/null 2>"$AVTR" &
AVA=$!
av_wait '[ "$(grep -c "^+* tick\$" "$AVTR")" -ge 2 ]'
av_tm set-option -g @cockpit_bg_poll off
av_reap "$AVA" && AVLEFT=yes || AVLEFT=no
is "a poller told to stop by the option leaves by itself" "yes" "$AVLEFT"
is "...and signals nothing on the way out" "0" "$(grep -c '^+* kill ' "$AVTR")"
is "...after running its exit trap" "1" "$(grep -c '^+* release$' "$AVTR")"
av_tm set-option -g @cockpit_bg_poll 1
# A signal can land after `sleep &` has forked and before `sleeper=$!` has saved the pid. A DEBUG
# trap (set -T carries it into the sourced script) makes that moment exact: it sends the poller
# its own TERM just before that assignment.
# The sleep (a shim that records its pid) must not outlive the poller.
AVSP="$AVREC/sleep.pid"; rm -f "$AVSP"
AVSLB=$(mktmpd); ln -s "$AVF/sleep-pid" "$AVSLB/sleep"
av_tm set-option -g @cockpit_bg_poll 3600
av_tm set-option -gu @cockpit_poll_pid
env -u TMUX_PANE TMUX="$AVT" PATH="$AVSLB:$AVBIN:$PATH" AV_JSON="$AVF/done.json" \
    AV_SLEEPPID="$AVSP" AV_REALSLEEP="$(command -v sleep)" \
    bash -c 'set -T; trap "$1" DEBUG; . "$0"' "$DIR/scripts/agent-poll" \
    '[ "$BASH_COMMAND" = "sleeper=\$!" ] && kill -TERM $$' </dev/null >/dev/null 2>&1 &
AVA=$!
av_reap "$AVA" && AVLEFT=yes || AVLEFT=no
is "a TERM between the fork and the saved pid still ends the poller" "yes" "$AVLEFT"
AVSC=$(cat "$AVSP" 2>/dev/null)
is "...the sleep it had started was reached" "yes" "$([ -n "$AVSC" ] && echo yes || echo no)"
av_wait "av_gone ${AVSC:-0}" && AVLEFT=yes || AVLEFT=no
[ -z "$AVSC" ] || av_gone "$AVSC" || kill "$AVSC" 2>/dev/null
is "...and did not outlive it" "yes" "$AVLEFT"
av_tm set-option -g @cockpit_bg_poll 1
# A HUP that lands right after the claim has published the pid must not leave it there: the
# traps are in place before the claim. The DEBUG trap sends it at the claim's read-back, the
# first command after the set.
AVHUP='case "$BASH_COMMAND" in "[ \"\$(tmux show-option -gqv @cockpit_poll_pid"*)'
AVHUP="$AVHUP kill -HUP \$\$ ;; esac"
av_tm set-option -g @cockpit_bg_poll 3600
av_tm set-option -gu @cockpit_poll_pid
env -u TMUX_PANE TMUX="$AVT" PATH="$AVBIN:$PATH" AV_JSON="$AVF/done.json" \
    bash -c 'set -T; trap "$1" DEBUG; . "$0"' "$DIR/scripts/agent-poll" "$AVHUP" \
    </dev/null >/dev/null 2>&1 &
AVA=$!
av_reap "$AVA" && AVLEFT=yes || AVLEFT=no
is "a HUP right after the claim ends the poller" "yes" "$AVLEFT"
is "...and the slot it had just taken is empty again" "" "$(av_pub)"
av_tm set-option -g @cockpit_bg_poll 1
# Two starts at once: one poller is left, and it holds the slot. From here the tick is an
# hour, so only a signal can end a poller and only release can end its sleep.
av_tm set-option -g @cockpit_bg_poll 3600
av_tm set-option -gu @cockpit_poll_pid
av_startx; AVA=$AVPID; av_startx; AVB=$AVPID
av_alive_count() {
    local n=0; av_gone "$AVA" || n=$((n + 1)); av_gone "$AVB" || n=$((n + 1)); echo "$n"
}
av_wait '[ "$(av_alive_count)" = 1 ]'
is "two simultaneous starts leave exactly one poller" "1" "$(av_alive_count)"
if av_gone "$AVA"; then AVL=$AVB; else AVL=$AVA; fi
is "...and it holds the slot" "$AVL" "$(av_pub)"
# Its sleep child goes with it.
av_wait 'pgrep -P "$AVL" >/dev/null'
AVCH=$(pgrep -P "$AVL" | head -1)
kill -TERM "$AVL"; av_reap "$AVL"
av_wait "av_gone $AVCH" && AVLEFT=yes || AVLEFT=no
av_gone "$AVCH" || kill "$AVCH" 2>/dev/null
is "a poller stopped by a signal leaves no child behind" "yes" "$AVLEFT"
wait "$AVA" "$AVB" 2>/dev/null
av_tm kill-server >/dev/null 2>&1
fi

echo "== agent-view: a poller that cannot read the sessions leaves the reason (#174) =="
av_setup
if need_tmux "agent-view poll_err"; then :; else
av_srv err
av_tm set-option -p -t "$AVP" @cockpit_agentview 1
av_path() {                   # av_path <dir> <jq|no> <claude|no>: a PATH of just what it needs
    local c
    for c in env bash timeout sleep cat; do ln -s "$(command -v $c)" "$1/$c"; done
    ln -s "$TMUX_REAL" "$1/tmux"
    [ "$2" = jq ] && ln -s "$(command -v jq)" "$1/jq"
    [ "$3" = claude ] && ln -s "$AVF/claude" "$1/claude"
    return 0
}
AVE1=$(mktmpd); av_path "$AVE1" no claude
AVE2=$(mktmpd); av_path "$AVE2" jq no
AVE3=$(mktmpd); av_path "$AVE3" jq claude
av_err() {                    # av_err <PATH dir> <fixture> [env...] -> @cockpit_poll_err
    local pdir=$1 j=$2; shift 2
    env -u TMUX_PANE TMUX="$AVT" PATH="$pdir" AV_JSON="$AVF/$j" "$@" \
        "$DIR/scripts/agent-poll" --once </dev/null >/dev/null 2>&1
    av_tm show-option -gqv @cockpit_poll_err
}
is "no jq: the reason is recorded" "no jq on PATH" "$(av_err "$AVE1" blocked.json)"
is "no claude: the reason is recorded" "no claude on PATH" "$(av_err "$AVE2" blocked.json)"
is "claude failing: the reason is recorded" "claude agents failed" \
   "$(av_err "$AVE3" blocked.json AV_RC=1)"
is "unreadable json: the reason is recorded" "claude agents gave unreadable json" \
   "$(av_err "$AVE3" garbage.json)"
is "a good tick clears it" "" "$(av_err "$AVE3" blocked.json)"
av_tm kill-server >/dev/null 2>&1
fi


echo "== transcript reader: doing, said and asked rows (#176) =="
TRF="$DIR/tests/fixtures/transcript-reader"
. "$DIR/scripts/lib/transcript.sh"
trrow() { transcript_doing "$1" 2>/dev/null | awk -F'\t' -v k="$2" -v n="$3" '$1==k { print $n }'; }
trnum() { case "$1" in ''|*[!0-9]*) echo "not-a-number:$1";; *) echo number;; esac; }
is "an open Bash tool_use gives the doing row 'Bash: <description>'" \
   "Bash: Run the widget tests" "$(trrow "$TRF/open.jsonl" doing 2)"
is "the doing row's third field is whole seconds since the tool started" \
   number "$(trnum "$(trrow "$TRF/open.jsonl" doing 3)")"
is "a tool_use with a tool_result after it is closed: no doing row" \
   "" "$(trrow "$TRF/closed.jsonl" doing 2)"
is "the said row is the last assistant text, first line only" \
   "Looking at the widget now." "$(trrow "$TRF/open.jsonl" said 2)"
is "the asked row is the last string prompt" \
   "fix the widget" "$(trrow "$TRF/open.jsonl" asked 2)"
is "the asked row's third field is whole seconds since the prompt" \
   number "$(trnum "$(trrow "$TRF/open.jsonl" asked 3)")"
is "rows come out in the order doing, said, asked" \
   "doing said asked" "$(transcript_doing "$TRF/open.jsonl" 2>/dev/null | cut -f1 | tr '\n' ' ' | sed 's/ $//')"
is "a closed fixture still reports said and asked, status 0" \
   "said asked 0" "$(transcript_doing "$TRF/closed.jsonl" 2>/dev/null | cut -f1 | tr '\n' ' '; echo "${PIPESTATUS[0]}")"
is "a tool without a description falls back to its file_path" \
   "Read: /tmp/x" "$(trrow "$TRF/noisy.jsonl" doing 2)"
is "control characters are stripped and only the first line is kept" \
   "first bell" "$(trrow "$TRF/noisy.jsonl" asked 2)"
TRL="$(mktmpf)"
for n in 160 161 300; do
    jq -nc --argjson n "$n" '{type:"assistant",version:"2.1.316",timestamp:"2026-01-01T10:00:00Z",
        message:{content:[{type:"text",text:([range($n)|"x"]|join(""))}]}}' > "$TRL"
    is "a $n-char text is said as 160 chars" 160 "$(trrow "$TRL" said 2 | tr -d '\n' | wc -m | tr -d ' ')"
done
jq -nc '{type:"assistant",version:"2.1.316",timestamp:"2026-01-01T10:00:00Z",message:{content:[
    {type:"tool_use",id:"t9",name:"Ba\u0007sh",input:{description:([range(300)|"d"]|join(""))}}]}}' > "$TRL"
is "the doing row's tool name and description are stripped and capped at 160 together" \
   "160 Bash" "$(trrow "$TRL" doing 2 | awk '{ print length($0), substr($0, 1, 4) }')"
is "a version older than 2.1.246 is refused: status 1, no output" \
   "1:" "$(transcript_doing "$TRF/old.jsonl" 2>/dev/null; echo -n "$?" | sed 's/$/:/')"
is "a version newer than any known one is still read" \
   "future prompt" "$(trrow "$TRF/newer.jsonl" asked 2)"
is "transcript_tail also refuses the old version" \
   "1:" "$(transcript_tail "$TRF/old.jsonl" 2>/dev/null; echo -n "$?" | sed 's/$/:/')"
# shellcheck disable=SC2123  # emptying the search path is the test
NOJQ=$(PATH=/nonexistent; for f in "transcript_doing $TRF/open.jsonl" "transcript_tail $TRF/open.jsonl" \
        "transcript_file %1"; do $f 2>&1; echo -n "$? "; done)
is "with jq off PATH all three readers print nothing, even on stderr, and return 1" \
   "1 1 1 " "$NOJQ"
EMPTY="$(mktmpf)"
is "an empty transcript: status 1, no output" "1:" "$(transcript_doing "$EMPTY" 2>&1; echo -n "$?:")"
is "a missing transcript: status 1, no output" "1:" \
   "$(transcript_doing "$EMPTY.nope" 2>&1; echo -n "$?:")"
is "a zero-argument call: status 1, no output" "1:1:1:" \
   "$(transcript_doing 2>&1; echo -n "$?:"; transcript_tail 2>&1; echo -n "$?:"; transcript_file 2>&1; echo -n "$?:")"
printf 'not json at all\n{"half":\n' > "$TRL"
is "a transcript that is not JSON: status 1, no output" "1:" "$(transcript_doing "$TRL" 2>&1; echo -n "$?:")"

trver() { sed "s/2\.1\.316/$1/" "$TRF/open.jsonl" > "$TRL"
          transcript_doing "$TRL" 2>/dev/null | cut -f1 | tr '\n' ' '; }
is "version 2.1.99 is older than 2.1.246 (numeric compare): refused" "" "$(trver 2.1.99)"
is "version 2.1.1000 is newer than 2.1.246 (numeric compare): read" "doing said asked " "$(trver 2.1.1000)"
is "version 2.1.250-beta compares on its leading x.y.z: read" "doing said asked " "$(trver 2.1.250-beta)"
is "version 2.1 counts as 2.1.0: refused" "" "$(trver 2.1)"
trpr() {                      # <content> [isMeta]: a real prompt, then this one
    { jq -nc '{type:"user",version:"2.1.316",timestamp:"2026-01-01T10:00:00Z",message:{content:"real prompt"}}'
      jq -nc --arg c "$1" --argjson m "${2:-false}" '{type:"user",version:"2.1.316",isMeta:$m,
          timestamp:"2026-01-01T10:00:05Z",message:{content:$c}}'; } > "$TRL"; }
for c in '<task-notification>done' '<system-reminder>hook' '<local-command-stdout>x' '<command-name>/x'; do
    trpr "$c"
    is "a prompt starting $c is harness-injected: asked and USER skip it" \
       "real prompt|USER: real prompt" "$(trrow "$TRL" asked 2)|$(transcript_tail "$TRL" 2>/dev/null)"
done
trpr "meta text" true
is "an isMeta prompt is skipped by asked and by the tail" \
   "real prompt|USER: real prompt" "$(trrow "$TRL" asked 2)|$(transcript_tail "$TRL" 2>/dev/null)"
trpr '<pasted_content id="1">a long paste'
is "a <pasted_content> prompt shows as [pasted text] in asked and in the tail" \
   "[pasted text]|USER: real prompt"$'\n'"USER: [pasted text]" \
   "$(trrow "$TRL" asked 2)|$(transcript_tail "$TRL" 2>/dev/null)"
trsay() { jq -nc --arg t "$1" '{type:"assistant",version:"2.1.316",timestamp:"2026-01-01T10:00:00Z",
              message:{content:[{type:"text",text:$t}]}}' > "$TRL"; }
trsay $'a\u0085b\u009bc'
is "C1 controls (U+0080-U+009F) are stripped" "abc" "$(trrow "$TRL" said 2)"
trsay $'a\tb'
is "a TAB inside a text field becomes a space" "a b" "$(trrow "$TRL" said 2)"
{ echo '{"type":"assistant","version":"2.1.316","timestamp":"2026-01-01T10:00:00Z","message":{"content":[{"type":"tool_use","id":"far","name":"Bash","input":{"description":"too old to see"}}]}}'
  for _ in $(seq 350); do echo '{"type":"user","version":"2.1.316","timestamp":"2026-01-01T10:00:01Z","message":{"content":"p"}}'; done; } > "$TRL"
is "only the last 300 lines are read: an open tool_use 350 lines back gives no doing row" \
   "" "$(trrow "$TRL" doing 2)"

echo "== transcript reader: tail (#176) =="
TRF="$DIR/tests/fixtures/transcript-reader"
. "$DIR/scripts/lib/transcript.sh"
is "the tail is USER/AGENT/TOOL lines, oldest first" \
   $'USER: fix the widget\nAGENT: Looking at the widget now.\nTOOL Bash: Run the widget tests' \
   "$(transcript_tail "$TRF/open.jsonl" 2>/dev/null)"
is "the tail leaves out lines that are not turns" \
   3 "$(transcript_tail "$TRF/open.jsonl" 2>/dev/null | wc -l | tr -d ' ')"
is "a cap of 60 bytes drops the oldest lines whole" \
   $'TOOL Bash: Run the widget tests' "$(transcript_tail "$TRF/open.jsonl" 60 2>/dev/null)"
is "a cap of 80 keeps the two newest lines" \
   $'AGENT: Looking at the widget now.\nTOOL Bash: Run the widget tests' \
   "$(transcript_tail "$TRF/open.jsonl" 80 2>/dev/null)"
is "the output never exceeds the cap" "ok" \
   "$([ "$(transcript_tail "$TRF/open.jsonl" 80 2>/dev/null | wc -c)" -le 80 ] && echo ok)"
is "a cap smaller than the newest line prints nothing, status 1" "1:" \
   "$(transcript_tail "$TRF/open.jsonl" 5 2>&1; echo -n "$?:")"
is "a non-numeric cap prints nothing, status 1" "1:" \
   "$(transcript_tail "$TRF/open.jsonl" many 2>&1; echo -n "$?:")"

echo "== transcript reader: which transcript (#176) =="
. "$DIR/scripts/lib/transcript.sh"
TRH="$(mktmpd)"; mkdir -p "$TRH/.claude/projects/-some-proj"
: > "$TRH/.claude/projects/-some-proj/sid-aaa.jsonl"
: > "$TRH/.claude/projects/-some-proj/sid-bbb.jsonl"
TRSOCK="cockpit-trf-$$"; SOCKS="$SOCKS $TRSOCK"
tmux_tr() { "$TMUX_REAL" -L "$TRSOCK" "$@"; }
"$TMUX_REAL" -L "$TRSOCK" -f /dev/null new-session -d -s t 'sleep 600 & wait' >/dev/null 2>&1
TRP=$(tmux_tr display-message -p -t t '#{pane_id}' 2>/dev/null)
TRPID=$(tmux_tr display-message -p -t t '#{pane_pid}' 2>/dev/null)
TRBIN="$(mktmpd)"
TRKID=
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    TRKID=$(pgrep -P "$TRPID" sleep | head -1); [ -n "$TRKID" ] && break
    read -r -t 0.2 <> <(:) 2>/dev/null
done
cat > "$TRBIN/claude" <<FAKE
#!/bin/sh
echo '[{"pid":1,"sessionId":"sid-aaa","cwd":"/x","status":"idle"},{"pid":$TRKID,"sessionId":"sid-bbb","cwd":"/y","status":"busy"}]'
FAKE
chmod +x "$TRBIN/claude"
trfile() { ( tmux() { tmux_tr "$@"; }; HOME="$TRH" PATH="$TRBIN:$PATH" transcript_file "$1" 2>&1; echo -n "$?" ); }
is "the fake claude's child pid was found under the pane (rig check)" "yes" \
   "$([ -n "$TRKID" ] && echo yes)"
is "with no @cockpit_session the agents list is joined on the pane's process tree" \
   "$TRH/.claude/projects/-some-proj/sid-bbb.jsonl
0" "$(trfile "$TRP")"
tmux_tr set-option -p -t "$TRP" @cockpit_session sid-aaa 2>/dev/null
is "@cockpit_session wins over the agents list" \
   "$TRH/.claude/projects/-some-proj/sid-aaa.jsonl
0" "$(trfile "$TRP")"
tmux_tr set-option -p -t "$TRP" @cockpit_session sid-gone 2>/dev/null
is "a session id with no transcript file: nothing, status 1" "1" "$(trfile "$TRP")"
tmux_tr set-option -p -t "$TRP" @cockpit_session '../*' 2>/dev/null
is "a session id carrying glob or path characters is refused" "1" "$(trfile "$TRP")"
tmux_tr set-option -pu -t "$TRP" @cockpit_session 2>/dev/null
tmux_tr set-option -p -t "$TRP" @cockpit_session 'sid-*' 2>/dev/null
is "a session id of sid-* (two matching files) is refused, not globbed" "1" "$(trfile "$TRP")"
touch "$TRH/.claude/projects/outside.jsonl"
tmux_tr set-option -p -t "$TRP" @cockpit_session '../outside' 2>/dev/null
is "a session id that climbs out of its project directory is refused" "1" "$(trfile "$TRP")"
tmux_tr set-option -p -t "$TRP" @cockpit_session sid-aaa 2>/dev/null
is "jq off PATH on the @cockpit_session path: nothing, status 1" "1" \
   "$( ( tmux() { tmux_tr "$@"; }; HOME="$TRH" PATH=/nonexistent transcript_file "$TRP" 2>&1; echo -n "$?" ) )"
tmux_tr set-option -pu -t "$TRP" @cockpit_session 2>/dev/null
printf '#!/bin/sh\necho '"'"'[{"pid":%d,"sessionId":"sid-aaa"}]'"'"'\n' "$$" > "$TRBIN/claude"   # a live pid that is not under the pane
is "an agents entry outside the pane's process tree is not joined" "1" "$(trfile "$TRP")"
printf '#!/bin/sh\necho garbage\n' > "$TRBIN/claude"
is "garbage from claude agents: nothing, status 1" "1" "$(trfile "$TRP")"
printf '#!/bin/sh\necho "$1" > "%s/bound"; shift; exec "$@"\n' "$TRBIN" > "$TRBIN/timeout"
printf '#!/bin/sh\necho "[]"\n' > "$TRBIN/claude"; chmod +x "$TRBIN/timeout"
trfile "$TRP" >/dev/null
is "claude agents runs under a 2 s timeout" "2" "$(cat "$TRBIN/bound" 2>/dev/null)"
rm -f "$TRBIN/timeout" "$TRBIN/bound"
tmux_tr kill-server 2>/dev/null
echo "== cockpit-tui diffstat and git-flags match the shell they will replace (#199) =="
DSB=$COCKPIT_TUI_BIN
# tree_diff's rows summed the way fleet_scan_root sums them, as diffstat's four
# columns; review_base forced to HEAD, which is what diffstat with no --base reads.
ds_shell() {
    ( . "$DIR/scripts/lib/git.sh"; . "$DIR/scripts/lib/review-diff.sh"
      COCKPIT_REVIEW_BASE=uncommitted tree_diff "$1" --numstat --summary ) \
    | awk -F'\t' '/^ create mode /{n++; rows++; next}
                  $1 != "" {rows++; a += ($1 ~ /^[0-9]+$/) ? $1 : 0; d += ($2 ~ /^[0-9]+$/) ? $2 : 0}
                  END {pm = rows ? sprintf("+%d -%d", a, d) : "clean"; if (n) pm = pm "  ?" n " new"
                       printf "%d\t%d\t%d\t%s\n", a, d, n, pm}'
}
for gf in basic mixed untracked blank nonascii funcname hunks spaces; do
    GD=$(mktmpd); "$DIR/tests/fixtures/review/mkfix.sh" "$gf" "$GD" >/dev/null 2>&1
    is "diffstat $gf: tree_diff's totals" "$(ds_shell "$GD")" "$("$DSB" diffstat "$GD")"
    rm -rf "$GD"
done
GD=$(mktmpd); ( cd "$GD" && git init -q -b main . && printf 'a\nb\n' > x && printf 'c\n' > y ) >/dev/null 2>&1
is "diffstat on an unborn HEAD: tree_diff's totals, not clean" "$(ds_shell "$GD")" "$("$DSB" diffstat "$GD")"
is "diffstat on a dir that is not a repo: nothing, status 1" "1" \
   "$(rm -rf "$GD/.git"; "$DSB" diffstat "$GD"; printf '%s' "$?")"
rm -rf "$GD"

# git.sh keeps a static copy of the list (a missing binary must not break the
# worktree hook); this pins the copy to the binary.
is "git-flags is git.sh's GIT_SAFE and GIT_NOFETCH plus --no-optional-locks" \
   "$(. "$DIR/scripts/lib/git.sh"; printf '%s\n' --no-optional-locks "${GIT_SAFE[@]}" "${GIT_NOFETCH[@]}")" "$("$DSB" git-flags)"
# The filter overrides still differ until PR 5 moves git.sh onto the binary's:
# git.sh sets clean/smudge=cat and a blank required, and only for the keys the
# repo set; git-flags blanks all four keys of every driver, required=false.
GD=$(mktmpd); git init -q "$GD/r" >/dev/null 2>&1
git -C "$GD/r" config filter.evil.clean 'echo pwned'; git -C "$GD/r" config filter.evil.required true
mkdir "$GD/bin"
printf '#!/bin/sh\n[ "$3" = config ] && exec %s "$@"\nprintf "%%s\\n" "$@"\n' "$(command -v git)" > "$GD/bin/git"
chmod +x "$GD/bin/git"
is "git.sh's filter overrides today (PR 5 aligns them)" "filter.evil.clean=cat filter.evil.required=" \
   "$( ( PATH="$GD/bin:$PATH"; . "$DIR/scripts/lib/git.sh"; git -C "$GD/r" status ) | grep '^filter\.' | tr '\n' ' ' | sed 's/ $//')"
is "git-flags --filters blanks every key of the driver" \
   "filter.evil.clean= filter.evil.smudge= filter.evil.process= filter.evil.required=false" \
   "$("$DSB" git-flags --filters "$GD/r" | grep '^filter\.' | tr '\n' ' ' | sed 's/ $//')"
rm -rf "$GD"
unset -f ds_shell

echo "== cockpit-tui transcript: byte-identical to the jq reader on every fixture (#199) =="
. "$DIR/scripts/lib/transcript.sh"
TPB=$COCKPIT_TUI_BIN; TPF="$DIR/tests/fixtures/transcript-reader"
# The seconds field can tick between the two runs, so both sides read N.
tpnorm() { awk -F'\t' -v OFS='\t' '($1=="doing"||$1=="asked") && $3 ~ /^[0-9]+$/ { $3="N" } 1'; }
for f in "$TPF"/*.jsonl; do
    n=${f##*/}
    is "doing on $n matches the jq reader" \
       "$(transcript_doing "$f" 2>&1 | tpnorm; echo "rc=${PIPESTATUS[0]}")" \
       "$("$TPB" transcript doing "$f" 2>&1 | tpnorm; echo "rc=${PIPESTATUS[0]}")"
    for cap in "" 80 5; do
        is "tail ${cap:-4096} on $n matches the jq reader" \
           "$(transcript_tail "$f" $cap 2>&1; echo "rc=$?")" \
           "$("$TPB" transcript tail "$f" $cap 2>&1; echo "rc=$?")"
    done
done
unset -f tpnorm

echo "== tm: picking a session from a real popup switches the client =="
# A server started from inside another tmux pane keeps that $TMUX_PANE in its
# global environment, and every popup inherits it. It names no pane here, so
# in_client() read no session and go() ran attach-session -d inside the popup:
# a nested client that detached the real one. display-popup blocks until the
# popup closes, hence the &.
if need_tmux "tm from a popup"; then :; else
TPI="cockpit-tmpopi-$$"; TPO="cockpit-tmpopo-$$"; SOCKS="$SOCKS $TPI $TPO"; TPD=$(mktmpd)
TMUX_PANE=%99 "$TMUX_REAL" -L "$TPI" -f /dev/null new-session -d -s one 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TPI" new-session -d -s two 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TPO" -f /dev/null new-session -d -s o -x 100 -y 30 \
    "TERM=xterm-256color exec $TMUX_REAL -L $TPI attach -t one" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$TPI" list-clients -F '#{client_name}'
tpc=$("$TMUX_REAL" -L "$TPI" list-clients -F '#{client_name}' 2>/dev/null)
"$TMUX_REAL" -L "$TPI" display-popup -c "$tpc" -E "$DIR/scripts/tm two; touch $TPD/done" >/dev/null 2>&1 &
until_out 1 sh -c "[ -e '$TPD/done' ] && echo 1"
if [ -e "$TPD/done" ]; then ok "tm returned, so the popup closed"
else bad "tm returned, so the popup closed" "$TPD/done" "tm still running: a nested attach"; fi
until_out "$tpc two" "$TMUX_REAL" -L "$TPI" list-clients -F '#{client_name} #{session_name}'
is "the popup's own client moved to two and is the only client" "$tpc two" \
   "$("$TMUX_REAL" -L "$TPI" list-clients -F '#{client_name} #{session_name}' 2>/dev/null)"
"$TMUX_REAL" -L "$TPO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TPI" kill-server >/dev/null 2>&1
fi

pick_pane() {                 # <socket> <dir> <cols> <rows> <rows-text> [pick args...]  runs `pick` in a fresh pane
    local sock=$1 d=$2 cols=$3 height=$4 text=$5 args; shift 5
    args=; [ $# -eq 0 ] || args=$(printf '%q ' "$@")
    printf '%s' "$text" > "$d/rows"
    {
        printf '#!/bin/sh\nbin=$1; d=$2; shift 2\n'
        printf '"$bin" pick "$@" < "$d/rows" > "$d/out" 2> "$d/err"\n'
        printf 'echo $? > "$d/rc"\nexec sleep 30\n'
    } > "$d/run"
    chmod +x "$d/run"
    "$TMUX_REAL" -L "$sock" -f /dev/null new-session -d -s t -x "$cols" -y "$height" \
        "$d/run '$DIR/bin/cockpit-tui' '$d' $args" >/dev/null 2>&1
}

echo "== cockpit-tui pick: filter, choose and cancel through a pty =="
# The rows come on stdin, so the keys and the screen must be /dev/tty: a picker
# that read keys from stdin would see EOF, and one that drew on stdout would
# put the UI into $(...). The key is the only byte on stdout.
if [ ! -x "$DIR/bin/cockpit-tui" ]; then
    skip_ "cockpit-tui pick (no bin/cockpit-tui; run make tui)"
elif need_tmux "cockpit-tui pick"; then :; else
PKROWS=$(printf 'main\tmain\nfeat/237\tfeat/237-pick  Add a Rust picker\nrelease\trelease/0.2\nqueue\tqueue\n')$'\n'
PKS="cockpit-pk-$$"; SOCKS="$SOCKS $PKS"; PKD=$(mktmpd)
pick_pane "$PKS" "$PKD" 80 20 "$PKROWS" --prompt branch
until_cap "$PKS" t '*branch>*1/4*' && ok "the list is drawn on the tty, four rows" \
    || bad "the list is drawn on the tty, four rows" "branch> 1/4" "$("$TMUX_REAL" -L "$PKS" capture-pane -p -t t | head -3)"
"$TMUX_REAL" -L "$PKS" send-keys -t t R E L
until_cap "$PKS" t '*1/1 of 4*' && ok "typing REL filters case-insensitively to one row" \
    || bad "typing REL filters case-insensitively to one row" "1/1 of 4" "$("$TMUX_REAL" -L "$PKS" capture-pane -p -t t | head -1)"
"$TMUX_REAL" -L "$PKS" send-keys -t t Enter
until_file "$PKD/rc" '?*'
is "enter prints the key of the filtered row and nothing else" "release" "$(cat "$PKD/out" 2>/dev/null)"
is "enter exits 0" "0" "$(cat "$PKD/rc" 2>/dev/null)"
"$TMUX_REAL" -L "$PKS" kill-server >/dev/null 2>&1

PKD=$(mktmpd); pick_pane "$PKS" "$PKD" 80 20 "$PKROWS"
until_cap "$PKS" t '*pick>*1/4*'
"$TMUX_REAL" -L "$PKS" send-keys -t t Down Enter
until_file "$PKD/rc" '?*'
is "down then enter chooses the second row's key" "feat/237" "$(cat "$PKD/out" 2>/dev/null)"
"$TMUX_REAL" -L "$PKS" kill-server >/dev/null 2>&1

PKD=$(mktmpd); pick_pane "$PKS" "$PKD" 80 20 "$PKROWS"
until_cap "$PKS" t '*pick>*1/4*'
"$TMUX_REAL" -L "$PKS" send-keys -t t Escape
until_file "$PKD/rc" '?*'
is "esc exits 1" "1" "$(cat "$PKD/rc" 2>/dev/null)"
is "esc prints nothing on stdout" "0" "$(wc -c < "$PKD/out" | tr -d ' ')"
"$TMUX_REAL" -L "$PKS" kill-server >/dev/null 2>&1

PKD=$(mktmpd); pick_pane "$PKS" "$PKD" 80 20 "" 
until_file "$PKD/rc" '?*'
is "no rows exits 1 without opening a screen" "1" "$(cat "$PKD/rc" 2>/dev/null)"
is "no rows prints nothing on stdout" "0" "$(wc -c < "$PKD/out" | tr -d ' ')"
"$TMUX_REAL" -L "$PKS" kill-server >/dev/null 2>&1

PKD=$(mktmpd); pick_pane "$PKS" "$PKD" 27 11 "$PKROWS"
until_cap "$PKS" t '*make the window larger*' && ok "under 28x12 the floor card is drawn" \
    || bad "under 28x12 the floor card is drawn" "make the window larger" "$("$TMUX_REAL" -L "$PKS" capture-pane -p -t t | head -3)"
"$TMUX_REAL" -L "$PKS" send-keys -t t Enter Escape
until_file "$PKD/rc" '?*'
is "under the floor enter chooses nothing and esc exits 1" "1:0" "$(cat "$PKD/rc" 2>/dev/null):$(wc -c < "$PKD/out" | tr -d ' ')"
"$TMUX_REAL" -L "$PKS" kill-server >/dev/null 2>&1

PKD=$(mktmpd); pick_pane "$PKS" "$PKD" 80 20 "$PKROWS"
until_cap "$PKS" t '*pick>*1/4*'
"$TMUX_REAL" -L "$PKS" send-keys -t t q u e u e Enter
until_file "$PKD/rc" '?*'
is "typing queue is a filter: q is a letter, and enter prints the row's key" "queue:0" "$(cat "$PKD/out" 2>/dev/null):$(cat "$PKD/rc" 2>/dev/null)"
"$TMUX_REAL" -L "$PKS" kill-server >/dev/null 2>&1

PKD=$(mktmpd); pick_pane "$PKS" "$PKD" 80 20 "$(printf '\tx\nk\tok\n')"
until_cap "$PKS" t '*pick>*1/1*'
"$TMUX_REAL" -L "$PKS" send-keys -t t Enter
until_file "$PKD/rc" '?*'
is "a keyless row is skipped and the key of the next one is printed" "k:0" "$(cat "$PKD/out" 2>/dev/null):$(cat "$PKD/rc" 2>/dev/null)"
is "stderr says once how many rows had no key" "cockpit-tui: pick: skipped 1 row with no key" "$(cat "$PKD/err" 2>/dev/null)"
"$TMUX_REAL" -L "$PKS" kill-server >/dev/null 2>&1

PKD=$(mktmpd); pick_pane "$PKS" "$PKD" 80 20 "$PKROWS" --prompt 'two words'
until_cap "$PKS" t '*two words>*' && ok "an argument with a space reaches the picker whole" \
    || bad "an argument with a space reaches the picker whole" "two words>" "$("$TMUX_REAL" -L "$PKS" capture-pane -p -t t | head -1)"
"$TMUX_REAL" -L "$PKS" kill-server >/dev/null 2>&1

PKD=$(mktmpd); pick_pane "$PKS" "$PKD" 80 20 "$PKROWS" --bogus
until_file "$PKD/rc" '?*'
is "an unknown flag exits 2 and names it" "2:1" "$(cat "$PKD/rc" 2>/dev/null):$(grep -c -- '--bogus' "$PKD/err")"
"$TMUX_REAL" -L "$PKS" kill-server >/dev/null 2>&1
fi

echo "== review store: revdiff context-line records parse and round-trip (#232) =="
# Shared setup lives in a function so each block passes alone under tests/only.sh.
rsa_load() { # source the lib, copy the revdiff fixture to a fresh dir, set RSAD/RSAF
    # shellcheck disable=SC1090
    . "$DIR/scripts/lib/review-store.sh"; declare -gA STORE=()
    RSAD=$(mktmpd); RSAF="$RSAD/comments.md"; cp "$DIR/tests/fixtures/store-adapter/${1:-revdiff.md}" "$RSAF"
}
rsa_load; parse_store "$RSAF" 2>"$RSAD/err"
is "a revdiff store with a ( ) record keeps every record" "5" "${STORE_N:-}"
is "and sets no .bak aside" "" "$(ls "$RSAD" | grep '\.bak' || true)"
is "and says nothing on stderr" "" "$(cat "$RSAD/err")"
is "the context record stores side ' '" " " "${STORE[1.side]:-}"
format_store "$RSAD/f1"; grep -qxF '## a.txt:1 ( )' "$RSAD/f1" && ok "a context record formats back as ( )" || bad "a context record formats back as ( )" "## a.txt:1 ( )" "$(cat "$RSAD/f1")"
is "format of the fixture is byte-identical to revdiff's text (modulo the final blank)" "" "$(diff <(cat "$RSAF"; echo) "$RSAD/f1" 2>&1)"
parse_store "$RSAD/f1"; format_store "$RSAD/f2"
is "format(parse(x)) is stable across a second pass" "" "$(cmp "$RSAD/f1" "$RSAD/f2" 2>&1)"

echo "== review store: a missing final blank line parses the same (#232) =="
rsa_load; parse_store "$RSAF"; rsa_n1=${STORE_N:-}; rsa_b1=${STORE[4.body]:-}
printf '\n' >> "$RSAF"; parse_store "$RSAF"
is "with and without the last blank line the count matches" "$rsa_n1" "${STORE_N:-}"
is "and the last body matches" "$rsa_b1" "${STORE[4.body]:-}"

echo "== review store: ingest mode keeps a leading > line as body (#232) =="
rsa_load quoted.md; parse_store_revdiff "$RSAF"
is "ingest: a > first body line is body" "> quoted first"$'\n'"more" "${STORE[0.body]:-}"
is "ingest: no fragment key is set" "unset" "${STORE[0.frag]-unset}"
rsa_load quoted.md; parse_store "$RSAF"
is "cockpit mode: the same line is still a fragment" "quoted first" "${STORE[0.frag]:-}"
is "cockpit mode: the body is the rest" "more" "${STORE[0.body]:-}"
rsa_load quoted.md; printf '## b.txt:9 ( )\ncomment\n' >> "$RSAF"; sed -i 's/^more$/more\n/' "$RSAF"; parse_store_revdiff "$RSAF"
is "ingest: a quoted line in one record does not disturb the next" "2/comment" "${STORE_N:-}/${STORE[1.body]:-}"

echo "== tmux-review --loop: revdiff reviews, the store takes what it wrote (#233) =="
RDB=$(mktmpd); RDW=$(mktmpd); RD=$(mktmpd); RDS=$(mktmpd); fake_revdiff "$RDB"
(
  cd "$RD" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\nb\nc\n' > f; git add -A; git commit -q -m init
  printf 'a\nB\nc\nd\n' > f
) >/dev/null 2>&1
. "$DIR/scripts/lib/review-store.sh"; declare -A STORE=(); STORE_N=0
RDOUT=$'## f:2 (+)\nfix this\n\n## f:2 (-)\nwhy remove b\n\n## f:1 ( )\ncontext note\n\n## f (file-level)\nwhole file note\n'
NORD=$(printf '%s' "$PATH" | tr ':' '\n' | while read -r d; do [ -x "$d/revdiff" ] || printf '%s:' "$d"; done)

# 1. exit 10: the comments land, each with the quote the diff gives it.
rd_reset; printf '%s' "$RDOUT" > "$RDW/out.1"; echo 10 > "$RDW/rc.1"; : > "$RDS/comments.md"
is "revdiff exit 10 then 'l': the loop leaves with the batch pinned (exit 0)" "0" "$(rd_loop l "$RD" "$RDS/comments.md")"
parse_store "$RDS/comments.md"
is "all four comments are in the store" "4" "$STORE_N"
is "a (+) comment: side, line, body" "+/2/fix this" "${STORE[0.side]:-}/${STORE[0.line]:-}/${STORE[0.body]:-}"
is "and its quote is the line as the tree has it" "B" "${STORE[0.frag]:-}"
is "a (-) comment is quoted from the base, not the tree" "-/b" "${STORE[1.side]:-}/${STORE[1.frag]:-}"
is "a context ( ) comment is kept, quoted from the tree" " /a/context note" "${STORE[2.side]:-}/${STORE[2.frag]:-}/${STORE[2.body]:-}"
is "a file-level comment has a path and no quote" "f/unset/whole file note" "${STORE[3.path]:-}/${STORE[3.frag]-unset}/${STORE[3.body]:-}"
rdargs=$(cat "$RDW/argv.1")
is "revdiff gets --untracked, the exit code, -o, --history-dir and --annotations" "--untracked --exit-code-on-annotations -o $RDS/comments.md.rdout --history-dir $RDS/history --annotations $RDS/comments.md.rdin" "$rdargs"
is "the uncommitted view passes no ref: the last argument is the preload" "$RDS/comments.md.rdin" "$(rd_last 1)"
is "nothing but the store is left beside it" "comments.md" "$(ls -A "$RDS")"

# 2. exit 0 with a store already pinned (Q, or no comments): nothing changes.
RDK=$(mktmpf); cp "$RDS/comments.md" "$RDK"; rd_reset
is "revdiff exit 0 (Q discard): the loop leaves, exit 0" "0" "$(rd_loop l "$RD" "$RDS/comments.md")"
cmp -s "$RDS/comments.md" "$RDK" && ok "and the store is byte-identical" || bad "and the store is byte-identical" "unchanged" "$(cat "$RDS/comments.md")"
rd_reset
is "d at the prompt delivers: exit 20" "20" "$(rd_loop d "$RD" "$RDS/comments.md")"
cmp -s "$RDS/comments.md" "$RDK" && ok "and the store is still what was pinned" || bad "and the store is still what was pinned" "unchanged" "$(cat "$RDS/comments.md")"

# 3. a pinned quote survives the round trip; the preload carries no quote lines and is 0600.
rd_reset; printf '## f:2 (+)\n> OLD QUOTE\nfix this\n\n' > "$RDS/comments.md"
printf '## f:2 (+)\nfix this, edited\n' > "$RDW/out.1"; echo 10 > "$RDW/rc.1"
rd_loop l "$RD" "$RDS/comments.md" >/dev/null; parse_store "$RDS/comments.md"
is "an anchor already in the store keeps its quote (the line may have moved)" "OLD QUOTE/fix this, edited" "${STORE[0.frag]:-}/${STORE[0.body]:-}"
is "the preload revdiff was handed has the record" "1" "$(grep -c '^## f:2 (+)$' "$RDW/pre.1")"
is "and no quote line in it" "0" "$(grep -c '^> ' "$RDW/pre.1")"
is "and is born 0600" "600" "$(cat "$RDW/premode.1")"

# 4. the preload name is derivable: a planted symlink is not written through.
rd_reset; RDV=$(mktmpf); printf 'victim\n' > "$RDV"; rm -f "$RDS/comments.md.rdin"; ln -s "$RDV" "$RDS/comments.md.rdin"
rd_loop l "$RD" "$RDS/comments.md" >/dev/null
is "a symlink planted at the preload name is left alone" "victim" "$(cat "$RDV")"

# 5. revdiff output that does not parse changes nothing.
rd_reset; printf 'no header here\n' > "$RDW/out.1"; echo 10 > "$RDW/rc.1"; cp "$RDS/comments.md" "$RDW/before"
rd_loop l "$RD" "$RDS/comments.md" >/dev/null
cmp -s "$RDS/comments.md" "$RDW/before" && ok "an unreadable revdiff output leaves the store as it was" || bad "an unreadable revdiff output leaves the store as it was" "unchanged" "$(cat "$RDS/comments.md")"
is "and leaves no .bak behind" "comments.md" "$(ls -A "$RDS")"

# 6. revdiff missing: one clear message, a non-zero exit, no fallback.
rd_reset; cp "$RDS/comments.md" "$RDW/before"
is "revdiff not on PATH: exit 127" "127" "$(rd_loop l "$RD" "$RDS/comments.md" "$NORD")"
is "and the message says what to do" "revdiff not found: install it, see #233" "$(cat "$RDW/stderr")"
cmp -s "$RDS/comments.md" "$RDW/before" && ok "and the store is untouched" || bad "and the store is untouched" "unchanged" "$(cat "$RDS/comments.md")"

# 7. the base: b relaunches revdiff with the next base's ref.
RDT=$(mktmpd); RDWT=$(mktmpd)
(
  cd "$RDT" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init
  git checkout -q -b topic; printf 'a\nB\n' > f; git commit -q -am topic
  I=$(mktemp); cp -p .git/index "$I"; GIT_INDEX_FILE=$I git add -A
  git update-ref refs/cockpit/reviewed/topic "$(GIT_INDEX_FILE=$I git write-tree)"; rm -f "$I"
  printf 'a\nB\nC\n' > f
) >/dev/null 2>&1
rd_reset
is "b, b, l: three launches and a clean leave" "0" "$(rd_loop bbl "$RDT" "$RDWT/c.md")"
is "three launches" "3" "$(cat "$RDW/n")"
is "since-review launches against the checkpoint tree" "$(git -C "$RDT" rev-parse refs/cockpit/reviewed/topic)" "$(rd_last 1)"
is "b relaunches against the merge-base with main" "$(git -C "$RDT" merge-base main HEAD)" "$(rd_last 2)"
is "b again is the uncommitted view: no ref" "$RDWT/c.md.rdin" "$(rd_last 3)"
is "the choice is remembered beside the store" "uncommitted" "$(cat "$RDWT/c.md.base")"
is "a leave sheds the scratch" "c.md.base" "$(ls -A "$RDWT")"

# 8. approve: only with nothing pinned, and it writes the checkpoint.
rd_reset; : > "$RDS/comments.md"
is "a at the prompt approves: exit 21" "21" "$(rd_loop a "$RD" "$RDS/comments.md")"
is "and the checkpoint ref is a tree" "tree" "$(git -C "$RD" cat-file -t refs/cockpit/reviewed/main 2>/dev/null)"
git -C "$RD" update-ref -d refs/cockpit/reviewed/main
printf '## f:2 (+)\n> B\nkeep\n\n' > "$RDS/comments.md"
is "a with comments pinned does nothing; l leaves" "0" "$(rd_loop al "$RD" "$RDS/comments.md")"
is "and no checkpoint was written" "" "$(git -C "$RD" rev-parse -q --verify refs/cockpit/reviewed/main 2>/dev/null)"
# 9. a file-level comment whose first line is a quote keeps it as body.
rd_reset; mkdir -p "$RDS"; printf '## f (file-level)\n> not a quote\nnote\n\n' > "$RDS/comments.md"
rd_loop rl "$RD" "$RDS/comments.md" >/dev/null; parse_store "$RDS/comments.md"
is "the preload keeps a file-level body's first > line" "1" "$(grep -c '^> not a quote$' "$RDW/pre.1")"
is "and relaunching (r) does not empty the comment" "> not a quote"$'\n'"note" "${STORE[0.body]:-}"
is "it is body, not a fragment" "unset" "${STORE[0.frag]-unset}"

# 10. a revdiff that fails (not 0, 10): its own code and its stderr are kept.
rd_reset; echo 3 > "$RDW/rc.1"; echo "boom: bad flag" > "$RDW/err.1"; rm -f "$RDS/revdiff.err"
is "revdiff exit 3: the loop exits 126, not 'left for later'" "126" "$(rd_loop l "$RD" "$RDS/comments.md")"
is "revdiff's stderr is kept for the caller" "boom: bad flag" "$(cat "$RDS/revdiff.err" 2>/dev/null)"
is "and the loop says it failed" "revdiff exited 3" "$(head -1 "$RDW/stderr")"
rm -f "$RDS/revdiff.err"
rm -rf "$RDK" "$RDB" "$RDW" "$RD" "$RDS" "$RDT" "$RDWT" "$RDV"
unset RDB RDW RD RDS RDT RDWT RDV

echo "== cockpit-tui info: the same fields as tmux-review --info, for every base mode =="
if need_tmux "cockpit-tui info parity"; then :; else
PI="cockpit-pi-$$"; SOCKS="$SOCKS $PI"; PISTATE=$(mktmpd); PIDIR=$(mktmpd)
( cd "$PIDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init ) >/dev/null 2>&1
# -n pins the window name: automatic-rename would change it between the two reads.
"$TMUX_REAL" -L "$PI" -f /dev/null new-session -d -s t -n w -x 100 -y 30 -c "$PIDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PI" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
PIAP=$("$TMUX_REAL" -L "$PI" list-panes -t t: -F '#{pane_id}')
for kv in "@panestate needs-input" "@panestate_msg run-make" "@cockpit_review pending" "@cockpit_session sid-9" "@cockpit_title Fix-the-parser"; do
    "$TMUX_REAL" -L "$PI" set-option -p -t "$PIAP" "${kv%% *}" "${kv#* }" >/dev/null 2>&1
done
PISESS="$PISTATE/tmux-cockpit/review/${PIAP#%}-$(printf '%s' "$PIDIR" | cksum | cut -d' ' -f1)"
mkdir -p "$PISESS"; printf '## f:2 (+)\n> b\nx\n\n' > "$PISESS/comments.md"; printf '## f:1 (+)\n> a\ny\n\n## f:1 (+)\n> a\nz\n\n' > "$PISESS/delivered.md"
pi_sh()  { "$TMUX_REAL" -L "$PI" run-shell "XDG_STATE_HOME=$(printf '%q' "$PISTATE") '$DIR/scripts/tmux-review' -p $PIAP --info" 2>&1; }
pi_rs()  { XDG_STATE_HOME="$PISTATE" COCKPIT_TMUX="tmux -L $PI" "$COCKPIT_TUI_BIN" info --text "${1:-$PIAP}" 2>&1; }
pi_cmp() {                    # <what> -- each field of the shell screen is the Rust screen's
    local shs rss f
    shs=$(pi_sh); rss=$(pi_rs)
    for f in pane cwd root state review base session title; do
        is "$1: $f" "$(grep "^$f " <<<"$shs")" "$(grep "^$f " <<<"$rss")"
    done
    is "$1: review dir and its counts" "$(grep -A1 '^review dir' <<<"$shs")" "$(grep -A1 '^review dir' <<<"$rss")"
    is "$1: the tui row names the same version" "$(grep '^tui ' <<<"$shs" | awk '{print $2}')" "$(grep '^tui ' <<<"$rss" | awk '{print $2}')"
}
pi_cmp "uncommitted base"
is "the Rust base row is uncommitted" "base      uncommitted  (HEAD)" "$(pi_rs | grep '^base ')"
( cd "$PIDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git checkout -q -b feat; printf 'a\nb\n' > f; git commit -q -am two ) >/dev/null 2>&1
pi_cmp "branch base"
case "$(pi_rs | grep '^base ')" in 'base      vs main  ('*) ok "the Rust base row is vs main" ;; *) bad "the Rust base row is vs main" "vs main" "$(pi_rs | grep '^base ')" ;; esac
git -C "$PIDIR" update-ref refs/cockpit/reviewed/feat HEAD
pi_cmp "since-review base"
case "$(pi_rs | grep '^base ')" in 'base      since last review  ('*) ok "the Rust base row is since last review" ;; *) bad "the Rust base row is since last review" "since last review" "$(pi_rs | grep '^base ')" ;; esac
printf 'uncommitted\n' > "$PISESS/comments.md.base"
pi_cmp "a mode b chose"
is "and it is the chosen mode" "base      uncommitted  (HEAD)" "$(pi_rs | grep '^base ')"
is "a pane that is not there is named, exit 1" "cockpit-tui: no such pane %999 1" "$(pi_rs %999) $(pi_rs %999 >/dev/null 2>&1; echo $?)"
is "a word that is not a pane id is refused, exit 2" "2" "$(pi_rs notapane >/dev/null 2>&1; echo $?)"
"$TMUX_REAL" -L "$PI" kill-server >/dev/null 2>&1
fi

echo "== cockpit-tui info: prefix i opens the popup =="
if need_tmux "prefix i popup"; then :; else
PK="cockpit-pk-$$"; SOCKS="$SOCKS $PK"
"$TMUX_REAL" -L "$PK" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PK" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
pk_i() { "$TMUX_REAL" -L "$PK" list-keys -T prefix 2>/dev/null | awk '$4=="i"'; }
case "$(pk_i)" in
    *display-popup*"cockpit-tui'"*" info "*) ok "prefix i is bound to a popup running cockpit-tui info" ;;
    *) bad "prefix i is bound to a popup running cockpit-tui info" "display-popup ... cockpit-tui info" "$(pk_i)" ;;
esac
case "$(pk_i)" in *tmux-review*|*--info*) bad "prefix i no longer runs tmux-review --info" "no tmux-review" "$(pk_i)" ;; *) ok "prefix i no longer runs tmux-review --info" ;; esac
"$TMUX_REAL" -L "$PK" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$PK" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PK" set -g @cockpit_info_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$PK" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(pk_i)" in *cockpit-tui*) bad "@cockpit_info_key off leaves i unbound" "stock i" "$(pk_i)" ;; *) ok "@cockpit_info_key off leaves i unbound" ;; esac
"$TMUX_REAL" -L "$PK" kill-server >/dev/null 2>&1
fi


printf '  %d passed, %d failed, %d skipped\n' "$pass" "$fail" "$skipped"
[ "$fail" -eq 0 ] || exit 1
