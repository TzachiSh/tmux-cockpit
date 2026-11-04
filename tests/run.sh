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
# A login shell in a pane prints Ubuntu's motd once per HOME per day (profile.d/update-motd.sh), a
# banner that lands after a menu is up and overwrites its rows on the outer pane (#392).
: > "$HOME/.hushlogin"

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
# The hooks and the transcript reader take the projects tree from it: the tests use $HOME's.
unset CLAUDE_CONFIG_DIR
# The owner's live VS Code socket must not reach a block (#350).
unset VSCODE_IPC_HOOK_CLI
# The suite never runs the real VS Code (#350). Under WSL the Windows PATH brings
# its code(1) in, and with the private HOME it installs a fresh ~/.vscode-server
# (639 MB) and can open the owner's editor. So: no /mnt/<drive> entry on PATH, and
# a stub code first that only records its argv.
# Every empty entry (it means the cwd) is dropped too, wherever it sits; a glob in
# an entry stays literal.
path_without_mnt() {          # <PATH value> -> the same without /mnt/<drive>/... entries
    local - IFS=: out='' e
    set -f
    for e in $1; do
        case $e in '') continue ;; /mnt/[a-z]|/mnt/[a-z]/*) continue ;; esac
        out=${out:+$out:}$e
    done
    printf '%s' "$out"
}
cat > "$SHIM/code" <<CODEEOF
#!/bin/sh
printf '%s\\n' "\$@" >> "$STATE/code.argv"
exit 0
CODEEOF
chmod +x "$SHIM/code"
PATH="$SHIM:$(path_without_mnt "$PATH")"; export PATH

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
# A cockpit-tui that answers a pick with PICK_KEY (default the first row) and
# draws nothing: the driver for tmux-open-path's tail when no pty is wanted.
# Sets OPTUI, not printed: a $(...) would lose the variable. Shell builtins only,
# since some callers' PATH holds a tmux stub and nothing else.
optui_setup() {
    [ -n "${OPTUI:-}" ] && [ -x "$OPTUI" ] && return 0
    OPTUI=$(mktmpd)/cockpit-tui
    printf '#!/bin/sh\nwhile read -r _; do :; done\nprintf "%%s\\n" "${PICK_KEY-0}"\nexit "${PICK_RC:-0}"\n' > "$OPTUI"
    chmod +x "$OPTUI"
}
review_enter() {              # <socket> <ctl-pane>  tmux-review -p opens the pane's review loop at once
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
    # latest sizes the window to the client that typed last, so a PC attached
    # after a phone gets its full size (#277; reverses #147's smallest).
    is "repo config sets window-size latest" "latest" \
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
# #289: rename-window and rename-session expand formats in the new name, and a
# bare %% let a " close the string. Through the menu, each name lands exactly
# as typed and nothing in it runs.
"$TMUX_REAL" -L "$MI" set-environment -g MIMK "$MIDIR/pwned" >/dev/null 2>&1
MISID=$("$TMUX_REAL" -L "$MI" display-message -p -t t: '#{session_id}')
mi_rename() {                 # <key> <prompt> <format> <name>
    "$TMUX_REAL" -L "$MO" send-keys -t o C-b a
    until_cap "$MO" o "*Rename window*"
    "$TMUX_REAL" -L "$MO" send-keys -t o "$1"
    until_cap "$MO" o "*($2)*"
    "$TMUX_REAL" -L "$MO" send-keys -t o C-u
    "$TMUX_REAL" -L "$MO" send-keys -t o -l -- "$4"
    "$TMUX_REAL" -L "$MO" send-keys -t o Enter
    until_out "$(printf '%q' "$4")" "$TMUX_REAL" -L "$MI" display-message -p -t "$MISID:" "$3" >/dev/null
    is "$2 applies [$4] exactly" "$4" "$("$TMUX_REAL" -L "$MI" display-message -p -t "$MISID:" "$3")"
}
for mi_name in 'w"x; set -g @pwned Q;"' 'w#(touch $MIMK)' 'w#{session_name}' 'w#1 #[x]'; do
    mi_rename , rename-window '#{window_name}' "$mi_name"
done
for mi_name in 's"x; set -g @pwned Q;"' 's#(touch $MIMK)' 's#{session_name}' 's#1'; do
    mi_rename '$' rename-session '#{session_name}' "$mi_name"
done
# The renames' own run-shell finished before each name landed, so a payload
# job had its chance; this run-shell holds its client until that is sure.
"$TMUX_REAL" -L "$MI" run-shell "touch '$MIDIR/after'" >/dev/null 2>&1
until_out yes sh -c "[ -e '$MIDIR/after' ] && echo yes"
is "and no renamed name ran anything" "none " \
   "$([ -e "$MIDIR/pwned" ] && echo ran || echo none) $("$TMUX_REAL" -L "$MI" show-option -gqv @pwned)"
is "and the staging option is cleared" "" "$("$TMUX_REAL" -L "$MI" show-option -gqv @cockpit_rename_to)"
# A rejected id still consumes the staged name: nothing is left for a later call.
"$TMUX_REAL" -L "$MI" set -g @cockpit_rename_to stale >/dev/null 2>&1
"$TMUX_REAL" -L "$MI" run-shell "$(printf %q "$DIR/scripts/tmux-rename") session 'x;y'" >/dev/null 2>&1
is "tmux-rename with a bad id clears the staged name too" "" "$("$TMUX_REAL" -L "$MI" show-option -gqv @cockpit_rename_to)"
"$TMUX_REAL" -L "$MI" rename-session -t "$MISID" t >/dev/null 2>&1
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

echo "== agent-state: a Stop with background subagents in flight is not done (#310) =="
# The main turn ends while background subagents work ("Waiting for 3 background agents to
# finish"). Claude Code wakes the session when each reports back, so that Stop is a pause:
# its payload's background_tasks lists them. The hook payloads below are the ones Claude Code
# pipes in, on a private server, with HOME and CLAUDE_CONFIG_DIR away from the owner's.
if need_tmux "background subagents"; then :; else
BG="cockpit-bg-$$"; SOCKS="$SOCKS $BG"
BGCFG="$(mktmpd)/claude"
bg_real=$(getent passwd "$(id -u)" 2>/dev/null | cut -d: -f6)
[ -n "$bg_real" ] || bg_real=$(eval "printf '%s' ~$(id -un)")
bg_iso=ok
case "$HOME/" in "$bg_real/"|"$bg_real/.claude/"*) bg_iso="HOME=$HOME" ;; esac
case "$BGCFG/" in "$bg_real/.claude/"*) bg_iso="CLAUDE_CONFIG_DIR=$BGCFG" ;; esac
[ -n "$bg_real" ] && [ -n "$HOME" ] || bg_iso="no home to compare"
is "the block runs away from the owner's home and ~/.claude" ok "$bg_iso"
if [ "$bg_iso" != ok ]; then skip_ "background subagents: the rest, not isolated"; else
"$TMUX_REAL" -L "$BG" -f /dev/null new-session -d -s t -x 80 -y 24 \
    'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$BG" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
BGP=$("$TMUX_REAL" -L "$BG" list-panes -t t -F '#{pane_id}')
bgst() {                      # <state> <payload>: one hook, its payload on stdin
    printf '%s\n' "$2" | HOME="$HOME" CLAUDE_CONFIG_DIR="$BGCFG" TMUX="$SOCKDIR/$BG,0,0" \
        "$DIR/scripts/agent-state" "$1" "$BGP" >/dev/null 2>&1; }
bgo() { "$TMUX_REAL" -L "$BG" display-message -p -t "$BGP" "#{$1}"; }
bgis() {                      # <name> <pane> <window> <session needs-input count>
    is "$1: pane" "$2" "$(bgo @panestate)"
    is "$1: window" "$3" "$(bgo @cockpit_win_state)"
    is "$1: session ▲ count" "$4" "$(bgo @cockpit_sess_nb)"; }
bgstop() {                    # <tasks json> [assistant text]: a Stop payload
    printf '{"session_id":"s1","hook_event_name":"Stop","stop_hook_active":false,%s%s%s}' \
        "\"last_assistant_message\":\"${2:-ok}\"," "\"background_tasks\":[$1]," \
        '"session_crons":[]'; }
BGT='{"id":"a1","type":"subagent","status":"running","description":"lane",'
BGT=$BGT'"agent_type":"general-purpose"}'
BGSH='{"id":"b1","type":"shell","status":"running"'
BGU='{"hook_event_name":"UserPromptSubmit","prompt":"go"}'

bgst running "$BGU"
for a in a1 a2 a3; do
    bgst running "{\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"$a\"}"
done
bgst done "$(bgstop "$BGT,$BGT,$BGT")"
bgis "Stop with 3 background subagents" running running 0
bgst running "$BGU"; bgst done "$(bgstop "$BGT,$BGT")"
bgis "a report-back turn ending with 2 left" running running 0
bgst running "$BGU"; bgst done "$(bgstop "")"
bgis "the turn after the last reports back" done done 0

bgcase() {                    # <name> <want> <stop payload>: from running, one Stop
    bgst running "$BGU"; bgst done "$3"; is "$1" "$2" "$(bgo @panestate)"; }
bgcase "a Stop with no background_tasks field is done" done \
    '{"hook_event_name":"Stop","stop_hook_active":false}'
bgcase "a Stop with an empty payload is done" done ""
bgcase "a Stop with an unparseable payload is done" done 'not json {'
bgcase "a shell task alone does not hold the pane" done \
    "$(bgstop "$BGSH"',"description":"tail","command":"tail -f x"}')"
bgcase "an idle teammate alone does not hold the pane" done \
    "$(bgstop '{"id":"t1","type":"teammate","status":"running","description":"researcher"}')"
bgcase "a subagent entry with a space after the colon holds it" running \
    "$(bgstop '{"id":"a1", "type": "subagent", "status": "running"}')"
# Only a structural key counts: inside a JSON string the quotes arrive escaped.
BGQ='said \"type\":\"subagent\" here'
bgcase "subagent text in the message beside a shell task is done" done \
    "$(bgstop "$BGSH}" "$BGQ")"
bgcase "the same text beside a real subagent still holds it" running "$(bgstop "$BGT" "$BGQ")"
bgcase "subagent text in a task's description alone is done" done \
    "$(bgstop "$BGSH"',"description":"\"type\":\"subagent\""}')"
# Only background_tasks counts: a subagent-typed object in any other field is not in flight.
bgcase "a subagent object outside background_tasks, with no such field, is done" done \
    '{"hook_event_name":"Stop","other":{"type":"subagent"}}'
bgcase "the same object before a background_tasks of one shell task is done" done \
    '{"hook_event_name":"Stop","other":{"type":"subagent"},"background_tasks":[{"type":"shell"}]}'
# The match ends with the background_tasks array: a later key never holds the pane, and a
# `]` inside one of the array's strings does not end it early.
bgcase "a subagent object in a key after the array is done" done \
    '{"background_tasks":[],"session_crons":[],"later":[{"type":"subagent","status":"done"}]}'
bgcase "a subagent after a string holding ], in the array still holds it" running \
    "$(bgstop '{"type":"shell","description":"a ], \"b\" ]"},{"type":"subagent"}')"
# A string ending in an escaped backslash ends there: the next entry is still read.
bgcase "a subagent after a command ending in an escaped backslash still holds it" running \
    "$(bgstop "$BGSH"',"command":"dir C:\\tmp\\"},{"type":"subagent"}')"
# Bytes, not characters: under a UTF-8 locale a byte that is not UTF-8 must not end the match.
if locale -a 2>/dev/null | grep -qix 'c\.utf-\?8'; then
    bgst running "$BGU"
    printf '%s\n' "$(bgstop "$BGSH"',"description":"a'$'\377''b"},{"type":"subagent"}')" \
        | LC_ALL=C.UTF-8 TMUX="$SOCKDIR/$BG,0,0" "$DIR/scripts/agent-state" done "$BGP" \
        >/dev/null 2>&1
    is "a subagent after a non-UTF-8 byte holds it under C.UTF-8" running "$(bgo @panestate)"
    # A 1 MB array with no subagent, then the same array ending in one, both under C.UTF-8.
    BGARR=$(yes "{\"type\":\"shell\",\"command\":\"$(printf '%01000d' 0)\"}," \
        | head -n 1000 | tr -d '\n')
    is "the long array has 1000 shell entries" 1000 \
        "$(grep -o '"type":"shell"' <<< "$BGARR" | wc -l | tr -d ' ')"
    for bgw in done running; do
        bgx=${BGARR%,}; [ "$bgw" = running ] && bgx="$BGARR$BGT"
        bgst running "$BGU"
        printf '%s\n' "$(bgstop "$bgx")" | LC_ALL=C.UTF-8 TMUX="$SOCKDIR/$BG,0,0" \
            "$DIR/scripts/agent-state" done "$BGP" >/dev/null 2>&1
        is "a 1 MB array under C.UTF-8 ends $bgw" "$bgw" "$(bgo @panestate)"
    done
else
    skip_ "a subagent after a non-UTF-8 byte holds it under C.UTF-8 (no C.UTF-8 locale)"
    skip_ "the long array has 1000 shell entries (no C.UTF-8 locale)"
    skip_ "a 1 MB array under C.UTF-8 ends done (no C.UTF-8 locale)"
    skip_ "a 1 MB array under C.UTF-8 ends running (no C.UTF-8 locale)"
fi
# A long final message comes before the key and is read whole: no cap cuts the array off.
BGBIG=$(printf '%01048576d' 0)
is "the long message is 1 MB" 1048576 "${#BGBIG}"
bgcase "a subagent after a 1 MB message still holds it" running "$(bgstop "$BGT" "$BGBIG")"
bgcase "an empty array after a 1 MB message is done" done "$(bgstop "" "$BGBIG")"
# stdin held open past the payload: the writer's read -t is a hang guard, released once
# agent-state exits (opened read-write, so a writer gone at its guard cannot block the
# release). Past the newline, head returns there and the whole line is matched.
BGF="$(mktmpd)/hold"; mkfifo "$BGF"; BGIN="${BGF%/*}/in"; mkfifo "$BGIN"
bgheld() {                    # <payload> <end> [dir first on PATH]: done on a held pipe
    { printf '%s%s' "$1" "$2"; read -r -t 30 _ <> "$BGF"; } > "$BGIN" &
    local w=$!
    PATH="${3:+$3:}$PATH" TMUX="$SOCKDIR/$BG,0,0" "$DIR/scripts/agent-state" done "$BGP" \
        < "$BGIN" >/dev/null 2>&1
    printf 'x\n' 1<> "$BGF"; wait "$w"; }
bgst running "$BGU"; bgheld "$(bgstop "$BGT")" $'\n'
is "a Stop with stdin held open past its newline still holds" running "$(bgo @panestate)"
# No newline and stdin held for 10 s (#320): the read gives up at its 2 s bound and the
# Stop is done. Polled for half the hold, so an unbounded read is still waiting when it ends.
if command -v timeout >/dev/null 2>&1; then
    bgst running "$BGU"
    { { printf '%s' "$(bgstop "$BGT")"; read -r -t 10 _ <> "$BGF"; } \
        | TMUX="$SOCKDIR/$BG,0,0" "$DIR/scripts/agent-state" done "$BGP" >/dev/null 2>&1; } &
    bghold=$!
    POLLS=50 until_out done bgo @panestate
    is "a Stop with no newline and stdin held open returns within its bound" done \
        "$(bgo @panestate)"
    printf 'x\n' 1<> "$BGF"; wait "$bghold"
    # A head that passes on part of a line before it is killed: the cut payload is not read.
    BGHD=$(mktmpd); printf '#!/bin/sh\nexec cat\n' > "$BGHD/head"; chmod +x "$BGHD/head"
    bgst running "$BGU"; bgheld "$(bgstop "$BGT")" "" "$BGHD"
    is "a timed-out read that got part of the payload is done" done "$(bgo @panestate)"
else
    skip_ "a Stop with no newline and stdin held open returns within its bound (no timeout)"
    skip_ "a timed-out read that got part of the payload is done (no timeout)"
fi

# Sticky states stay sticky: a subagent's PermissionRequest lands on the main pane.
bgst running "$BGU"
bgst needs-input '{"hook_event_name":"PermissionRequest","agent_id":"a1","tool_name":"Bash"}'
bgst done "$(bgstop "$BGT")"
bgis "a subagent's needs-input through the main Stop" needs-input needs-input 1
bgst done "$(bgstop "")"
bgis "and through the last Stop" needs-input needs-input 1
bgst running "$BGU"; bgst error '{"hook_event_name":"PostToolUseFailure","agent_id":"a1"}'
bgst done "$(bgstop "$BGT")"
bgis "a subagent's error through the main Stop" error error 0

# Concurrent hooks: several report-back Stops and subagent tool calls at once.
bgjobs() {                    # <n> <state> <payload>: n hooks at once, then wait for them
    local i pids=""
    for i in $(seq 1 "$1"); do bgst "$2" "$3" & pids="$pids $!"; done
    # shellcheck disable=SC2086  # one pid per word
    wait $pids; }
bgst running "$BGU"
bgjobs 8 done "$(bgstop "$BGT")"
bgis "8 concurrent Stops with subagents alone" running running 0
bgst needs-input '{"hook_event_name":"PermissionRequest","agent_id":"a1"}'
bgjobs 8 done "$(bgstop "$BGT")" & bgjobs 8 done "$(bgstop "")"
wait $!
bgis "16 concurrent Stops on a blocked pane" needs-input needs-input 1
bgst running "$BGU"; bgjobs 8 done "$(bgstop "")"
bgis "8 concurrent Stops with nothing in flight" done done 0

# A subagent that never stops is listed by every Stop until claude ends; SessionEnd clears.
bgst running "$BGU"; bgst done "$(bgstop "$BGT")"
bgst off '{"hook_event_name":"SessionEnd","reason":"exit"}'
bgis "SessionEnd with a subagent still listed" "" idle 0
# The poller owns an agent-view pane (#174) and has no payload: its done stays done.
bgst running "$BGU"
printf '%s\n' "$(bgstop "$BGT")" | TMUX="$SOCKDIR/$BG,0,0" \
    "$DIR/scripts/agent-state" poll done "$BGP" >/dev/null 2>&1
is "the agent-view poller's done is not held by a payload" done "$(bgo @panestate)"
# Typed at a terminal, done reads no payload: the line typed next is the shell's, not eaten.
# util-linux script(1) gives the call a terminal; read's -t is a hang guard, not a measure.
if script -qec true /dev/null </dev/null >/dev/null 2>&1; then
    bgst running "$BGU"
    out=$(printf 'XYZ\n' | BGA="$DIR/scripts/agent-state" BGP="$BGP" TMUX="$SOCKDIR/$BG,0,0" \
        script -qec 'bash -c '\''"$BGA" done "$BGP" >/dev/null 2>&1
            IFS= read -r -t 5 l; printf "got=%s.\n" "$l"'\''' /dev/null)
    case "$out" in
        *got=XYZ.*) ok "done at a terminal leaves the next typed line to the shell" ;;
        *) bad "done at a terminal leaves the next typed line to the shell" "got=XYZ." "$out" ;;
    esac
    is "and the pane is done" done "$(bgo @panestate)"
else
    skip_ "done at a terminal (no util-linux script)"; skip_ "and the pane is done"
fi
"$TMUX_REAL" -L "$BG" kill-server >/dev/null 2>&1
fi
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

echo "== agent-state: a transition wakes every listed board, in the same round-trip =="
# The push channel's sender (#61, #224). Each board lists `wait-for -S <its
# channel> ; ` in @cockpit_board_wake and agent-state runs that list with
# run-shell -C inside its one tmux call. No pid is read or signalled.
# Known gap, no code: a second ask on an ALREADY-blocked pane exits at the
# sticky needs-input guard (scripts/agent-state) before reaching the sender,
# so that question reaches the board on its 1 s poll rather than on a push.
if need_tmux "agent-state push sender"; then :; else
PS="cockpit-push-$$"; SOCKS="$SOCKS $PS"
"$TMUX_REAL" -L "$PS" -f /dev/null new-session -d -s t -x 80 -y 24 \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PS" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
PSP=$("$TMUX_REAL" -L "$PS" list-panes -t t -F '#{pane_id}')
pstrans() {                   # one real transition on this block's server
    PATH="$SHIM:$PATH" TMUX="$SOCKDIR/$PS,0,0" "$DIR/scripts/agent-state" \
        "$1" "$PSP" >/dev/null 2>&1
}
# Two stand-in boards: a bare waiter on each channel, as the board's own child is.
"$TMUX_REAL" -L "$PS" set -g @cockpit_board_wake \
    'wait-for -S cockpit-board-1 ; wait-for -S cockpit-board-2 ; ' >/dev/null 2>&1
"$TMUX_REAL" -L "$PS" wait-for cockpit-board-1 & PSW1=$!
"$TMUX_REAL" -L "$PS" wait-for cockpit-board-2 & PSW2=$!
sleep 0.2
# running, not done: needs-input is sticky and a done would fail the
# transition's guard, so no transition would happen to push.
pstrans running
psgone() { local i; for i in $(seq 1 "$POLLS"); do kill -0 "$1" 2>/dev/null || return 0; sleep 0.1; done; return 1; }
if psgone "$PSW1" && psgone "$PSW2"; then ok "a transition wakes both listed boards"
else bad "a transition wakes both listed boards" "both waiters returned" "one still waits"; kill "$PSW1" "$PSW2" 2>/dev/null; fi
wait "$PSW1" "$PSW2" 2>/dev/null
# An exact count: the wake rides the one round-trip the transition already
# pays for, so a second tmux call of its own would show up as 2.
PSC=$(mktmpd); PSCLOG="$PSC/calls"; PSERR="$PSC/err"
cat > "$PSC/tmux" <<SHIMEOF
#!/bin/sh
printf '%s\n' "\$*" >> "$PSCLOG"
exec "$TMUX_REAL" "\$@"
SHIMEOF
chmod +x "$PSC/tmux"
pscount() { : > "$PSCLOG"; : > "$PSERR"
    PATH="$PSC:$PATH" TMUX="$SOCKDIR/$PS,0,0" "$DIR/scripts/agent-state" \
        "$1" "$PSP" >/dev/null 2>"$PSERR"
    wc -l < "$PSCLOG" | tr -d ' '; }
is "a transition with boards listed still costs 1 tmux round-trip" "1" "$(pscount needs-input)"
# No board is the common case and must cost nothing more.
"$TMUX_REAL" -L "$PS" set -gu @cockpit_board_wake >/dev/null 2>&1
is "with no board listed the transition still succeeds" "0" "$(pstrans running; echo $?)"
is "and still costs the same 1 round-trip" "1" "$(pscount needs-input)"
is "and says nothing on the hook's stderr" "" "$(cat "$PSERR")"
"$TMUX_REAL" -L "$PS" set -g @cockpit_board_wake '' >/dev/null 2>&1
is "an empty list costs the same 1 round-trip" "1" "$(pscount running)"
is "and says nothing on the hook's stderr" "" "$(cat "$PSERR")"
"$TMUX_REAL" -L "$PS" kill-server >/dev/null 2>&1
fi

echo "== cockpit-board: a state change wakes every running board through tmux wait-for =="
# #224: the real board, waiting on its own channel. A wake is the "push wake"
# line in tui.log; its timestamp against the hook's start is the latency.
if need_tmux "the board's push wake"; then :; else
BW="cockpit-bw-$$"; SOCKS="$SOCKS $BW"; BWD=$(mktmpd)
"$TMUX_REAL" -L "$BW" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$BW" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
BWP=$("$TMUX_REAL" -L "$BW" list-panes -t t -F '#{pane_id}')
bw_trans() {                  # one real transition on this block's server
    PATH="$SHIM:$PATH" TMUX="$SOCKDIR/$BW,0,0" "$DIR/scripts/agent-state" "$1" "$BWP" >/dev/null 2>&1
}
bw_list() { "$TMUX_REAL" -L "$BW" show-option -gqv @cockpit_board_wake; }
bw_open() {                   # <n>  -> BWW, BWPID: board <n> in its own window, its channel listed
    local i
    BWW=$("$TMUX_REAL" -L "$BW" new-window -d -P -F '#{pane_id}' -t t: \
        "exec env XDG_STATE_HOME=$BWD/$1 COCKPIT_LOG=$BWD/tui$1.log $COCKPIT_TUI_BIN board")
    BWPID=$("$TMUX_REAL" -L "$BW" display -p -t "$BWW" '#{pane_pid}')
    for i in $(seq 1 "$POLLS"); do
        case "$(bw_list)" in *"cockpit-board-$BWPID ; "*) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
bw_wakes() { local n; n=$(grep -c 'push wake' "$BWD/tui$1.log" 2>/dev/null); printf '%s' "${n:-0}"; }
bw_woke() {                   # <n> <count>  the board's log reaches <count> push wakes
    local i
    for i in $(seq 1 "$POLLS"); do
        [ "$(bw_wakes "$1")" -ge "$2" ] && return 0
        sleep 0.05
    done
    return 1
}
bw_open 1 || bad "the board lists its channel" "cockpit-board-$BWPID" "$(bw_list)"
BWPID1=$BWPID
# The waiter is armed once the board has run a moment; the latch covers it if not.
sleep 0.5
BWN1=$(bw_wakes 1)
# %N, not %6N: uutils date drops the leading zeros of %6N (0.015 s prints as 153677).
BWT0=$(( $(date +%s%N) / 1000 ))
bw_trans running
# The log's microsecond stamp, not the poll, times it: the poll's own forks would count.
BWMS=-1
bw_woke 1 $((BWN1 + 1)) \
    && BWMS=$(( ($(grep 'push wake' "$BWD/tui1.log" | sed -n "$((BWN1 + 1))p" | cut -d' ' -f1) - BWT0) / 1000 ))
if [ "$BWMS" -ge 0 ] && [ "$BWMS" -lt 500 ]; then ok "a transition wakes the running board within 500 ms"
    printf '    wake latency, hook start to the board loop: %s ms\n' "$BWMS"
else bad "a transition wakes the running board within 500 ms" "a push wake under 500 ms" "${BWMS} ms: $(cat "$BWD/tui1.log" 2>/dev/null)"; fi
is "no pid option is published" "" "$("$TMUX_REAL" -L "$BW" show-option -gqv @cockpit_tui_pid)"

bw_open 2 || bad "a second board lists its channel" "cockpit-board-$BWPID" "$(bw_list)"
BWW2=$BWW
sleep 0.5
BWN1=$(bw_wakes 1); BWN2=$(bw_wakes 2)
bw_trans needs-input
if bw_woke 1 $((BWN1 + 1)) && bw_woke 2 $((BWN2 + 1)); then ok "one transition wakes two boards"
else bad "one transition wakes two boards" "a wake in each log" "$(bw_wakes 1) and $(bw_wakes 2)"; fi

# SIGKILL runs nothing of the board's: its entry stays, and the next board to
# start drops it and still wakes. The pid is this private server's own pane.
kill -9 "$BWPID1" 2>/dev/null
BWI=0; while kill -0 "$BWPID1" 2>/dev/null && [ "$BWI" -lt "$POLLS" ]; do sleep 0.1; BWI=$((BWI+1)); done
case "$(bw_list)" in *"cockpit-board-$BWPID1 ; "*) ok "a SIGKILLed board leaves its entry" ;;
    *) bad "a SIGKILLed board leaves its entry" "cockpit-board-$BWPID1" "$(bw_list)" ;; esac
bw_open 3 || bad "a third board lists its channel" "cockpit-board-$BWPID" "$(bw_list)"
BWW3=$BWW
case "$(bw_list)" in *"cockpit-board-$BWPID1 "*) bad "the next board drops the dead entry" "no cockpit-board-$BWPID1" "$(bw_list)" ;;
    *) ok "the next board drops the dead entry" ;; esac
sleep 0.5
BWN2=$(bw_wakes 2); BWN3=$(bw_wakes 3)
bw_trans running
if bw_woke 3 $((BWN3 + 1)) && bw_woke 2 $((BWN2 + 1)); then ok "after a SIGKILLed board the boards still wake"
else bad "after a SIGKILLed board the boards still wake" "a wake in logs 2 and 3" "$(bw_wakes 2) and $(bw_wakes 3)"; fi

# q unlists: once both boards have gone the list is empty.
"$TMUX_REAL" -L "$BW" send-keys -t "$BWW2" q; "$TMUX_REAL" -L "$BW" send-keys -t "$BWW3" q
BWI=0; while [ -n "$(bw_list)" ] && [ "$BWI" -lt "$POLLS" ]; do sleep 0.1; BWI=$((BWI+1)); done
is "q takes each board's entry off the list" "" "$(bw_list)"
"$TMUX_REAL" -L "$BW" kill-server >/dev/null 2>&1
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
# ~ stands for $HOME only at a path boundary (#296): /x/alice is not under /x/al.
tw_home() { HOME=$1 "$DIR/scripts/tmux-where" path "$2" "$3" "$4" 2>&1 | plain; }
for row in "/x/al|1|/x/alice|bash| /x/alice" "/x/al|1|/x/al/b|bash| ~/b" \
           "/x/al/|1|/x/al/b|bash| ~/b" "/x/al/|1|/x/al|bash| ~" \
           "/x/al|999999|/x/alice|| /x/alice" "/x/al|999999|/x/al/b|| ~/b"; do
    IFS='|' read -r h p d c want <<<"$row"
    is "tmux-where HOME=$h pid $p shows $d as [$want]" "$want" "$(tw_home "$h" "$p" "$d" "$c")"
done
unset -f tw_home

echo "== the bar's path shows ~ only for \$HOME at a path boundary (#296) =="
if need_tmux "home boundary in the bar"; then :; else
HB="cockpit-hb-$$"; SOCKS="$SOCKS $HB"
HBD=$(mktmpd); mkdir -p "$HBD/al/b" "$HBD/alice"
"$TMUX_REAL" -L "$HB" -f /dev/null new-session -d -s t -n alice -c "$HBD/alice" >/dev/null 2>&1
"$TMUX_REAL" -L "$HB" new-window -d -n b -c "$HBD/al/b" >/dev/null 2>&1
"$TMUX_REAL" -L "$HB" new-window -d -n home -c "$HBD/al" >/dev/null 2>&1
hb_path() { "$TMUX_REAL" -L "$HB" display -p -t "t:$1" '#{E:@cockpit_where_path}' 2>/dev/null; }
for h in "$HBD/al" "$HBD/al/"; do
    "$TMUX_REAL" -L "$HB" run-shell "HOME='$h' '$DIR/cockpit.tmux'" >/dev/null 2>&1
    is "HOME=$h: a sibling sharing the prefix, a subdir, HOME itself" \
       "$HBD/alice|~/b|~" "$(hb_path alice)|$(hb_path b)|$(hb_path home)"
done
unset -f hb_path
"$TMUX_REAL" -L "$HB" kill-server >/dev/null 2>&1
rm -rf "$HBD"
fi

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
    # A tmux on tmux-where's PATH that logs its argv: the server's own message log also holds
    # any other client's display-message (a hook, a poller of an earlier block), and counting
    # those made "none of it reached tmux" flake under load.
    TWB=$(mktmpd); TWLOG="$TWB/argv"; : > "$TWLOG"
    printf '#!/bin/sh\nprintf "%%s\\n" "$*" >> "%s"\nexec "%s/tmux" "$@"\n' \
        "$TWLOG" "$SHIM" > "$TWB/tmux"
    chmod +x "$TWB/tmux"
    twh() { PATH="$TWB:$PATH" "$DIR/scripts/tmux-where" "$@"; }
    # argv[0] is what tmux reports as pane_current_command, so a process
    # exec'd under the name ssh must still take the ssh walk (which then reads
    # its first bare argument, 30, as the destination).
    tmux new-window -d -n wherepane 'exec -a ssh sleep 30'
    WP=$(tmux list-panes -a -F '#{pane_id} #{window_name}' | awk '$2=="wherepane"{print $1}')
    WPID=$(tmux display-message -p -t "$WP" '#{pane_pid}')
    for _ in $(seq "$POLLS"); do        # sh -c may not have exec'd yet
        IFS= read -r -d '' a0 2>/dev/null < "/proc/$WPID/cmdline"; [ "$a0" = ssh ] && break; sleep 0.1
    done
    is "an ssh pane is recognised from /proc" " 󰢹 30" "$(twh path "$WPID" "$WP" | plain)"
    is "and has no branch" "" "$(twh branch "$WPID" "$WP" | plain)"
    tmux kill-window -t wherepane
    # pid_max+1 cannot be alive; 999999 can, and would read a stranger's /proc.
    DEADPID=$(( $(cat /proc/sys/kernel/pid_max) + 1 ))
    is "a dead pid fails closed to ssh" " 󰢹 ssh" "$(twh path "$DEADPID" "$PANE" | plain)"
    is "none of it reached tmux" "0" "$(wc -l < "$TWLOG" | tr -d ' ')"
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

echo "== tmux-open-path: a clicked file name is never run as a tmux format (#289) =="
# new-window -n/-c and display-message expand #(): a repo file named for a
# command ran it on the click. Real server, since only tmux knows what it expands.
if need_tmux "open-path format escaping"; then :; else
OX="cockpit-ox-$$"; SOCKS="$SOCKS $OX"; OXD=$(mktmpd)
oxf='x#(tmux${IFS}set${IFS}-g${IFS}@pwned${IFS}yes).txt'   # no spaces: the line splits on them
: > "$OXD/$oxf"
"$TMUX_REAL" -L "$OX" -f /dev/null new-session -d -s t -c "$OXD" -x 100 -y 30 'exec sleep 3600' >/dev/null 2>&1
OXP=$("$TMUX_REAL" -L "$OX" list-panes -t t -F '#{pane_id}')
"$TMUX_REAL" -L "$OX" set -g @cockpit_editor true
"$TMUX_REAL" -L "$OX" set -g remain-on-exit on    # the editor (true) exits at once
"$TMUX_REAL" -L "$OX" set -p -t "$OXP" @cockpit_click_line "error in $oxf:3"
"$TMUX_REAL" -L "$OX" run-shell "$(printf %q "$DIR/scripts/tmux-open-path") $OXP" >/dev/null 2>&1
ox_n=0; until [ "$("$TMUX_REAL" -L "$OX" list-windows -t t 2>/dev/null | wc -l)" -ge 2 ] || [ "$ox_n" -ge 50 ]; do
    sleep 0.1; ox_n=$((ox_n + 1)); done
sleep 0.5    # a #() job, had one started, runs in the background
is "the click opened a window named for the file, literally" "$oxf" \
   "$("$TMUX_REAL" -L "$OX" list-windows -t t -F '#{window_name}' 2>/dev/null | tail -n 1)"
is "and nothing in the name ran" "" "$("$TMUX_REAL" -L "$OX" show -gqv @pwned 2>/dev/null)"
"$TMUX_REAL" -L "$OX" kill-server >/dev/null 2>&1
fi

# group: etmux
echo "== tmux-open-path: the editor command survives odd filenames =="
optui_setup
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
env COCKPIT_TUI_BIN="$OPTUI" PATH="$ETMUX" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt:12" \
    OPEN_EDITOR=nvim "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
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
env -i COCKPIT_TUI_BIN="$OPTUI" PATH="$ETMUX" HOME="$EQ" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt" \
    EDITOR=vi bash "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
case "$(cat "$LOG")" in
    *NEWWIN*vi*) ok "no code(1): falls back to \$EDITOR" ;;
    *) bad "no code(1): falls back to \$EDITOR" "a new window running vi" "$(cat "$LOG")" ;;
esac

# ...and with no editor at all it must say so, not claim success.
: > "$LOG"
env -i COCKPIT_TUI_BIN="$OPTUI" PATH="$ETMUX" HOME="$EQ" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
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
env -i COCKPIT_TUI_BIN="$OPTUI" PATH="$ETMUX" HOME="$HOME" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt:12" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
case "$(cat "$LOG")" in
    *"RUNSHELL run-shell -b"*"--goto"*) ok "code is launched by the server, not the dying popup" ;;
    *) bad "code is launched by the server, not the dying popup" \
         "a run-shell -b carrying code --goto" "$(cat "$LOG")" ;;
esac
# A folder must open as a workspace, not a file spec: --goto on a directory
# parses it as file:line and opens nothing.
: > "$LOG"
env -i COCKPIT_TUI_BIN="$OPTUI" PATH="$ETMUX" HOME="$HOME" TMUX_LOG="$LOG" CLICK_DIR="$DIR" CLICK_LINE="scripts" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
case "$(cat "$LOG")" in
    *"--goto"*) bad "a folder opens whole, not as a --goto spec" "code <dir>" "$(cat "$LOG")" ;;
    *"RUNSHELL run-shell -b"*scripts*) ok "a folder opens whole, not as a --goto spec" ;;
    *) bad "a folder opens whole, not as a --goto spec" "a run-shell carrying the dir" "$(cat "$LOG")" ;;
esac
rm -f "$ETMUX/code"
# One candidate is not a question (#228): it opens at once, with no popup.
: > "$LOG"
env PATH="$ETMUX:$PATH" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt:12" OPEN_EDITOR=nvim \
    "$DIR/scripts/tmux-open-path" %1 >/dev/null 2>&1
case "$(cat "$LOG")" in
    *NEWWIN*) ok "a single candidate opens straight away" ;;
    *) bad "a single candidate opens straight away" "a new-window" "$(cat "$LOG")" ;;
esac
case "$(cat "$LOG")" in
    *POPUP*) bad "and no picker opens for it" "no display-popup" "$(cat "$LOG")" ;;
    *) ok "and no picker opens for it" ;;
esac
# later blocks reuse this stub
ETMUX_TEMPLATE=$(mktmpf); cp "$ETMUX/tmux" "$ETMUX_TEMPLATE"; chmod +x "$ETMUX_TEMPLATE"
rm -rf "$EQ" "$ETMUX"

echo "== tmux-open-path: with no code on PATH, a click opens through VS Code's remote shim on the live socket =="
optui_setup
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
env -i COCKPIT_TUI_BIN="$OPTUI" PATH="$VTMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$VOUT" \
    CLICK_DIR="$VQ" CLICK_LINE="it's.txt:12" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
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
# run-shell expands formats in its command, so a clicked name like h#(touch F)
# must reach the --vscode re-entry with every # doubled; a live # would run it.
VHN='h#(touch${IFS}F)#S.txt'; : > "$VQ/$VHN"; : > "$VOUT"
env -i COCKPIT_TUI_BIN="$OPTUI" PATH="$VTMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$VOUT" \
    CLICK_DIR="$VQ" CLICK_LINE="$VHN:12" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
vhs=$(grep -F 'RUNSHELL' "$VOUT")
is "the remote-shim run-shell carries the hostile name, doubled" "1" "$(printf '%s\n' "$vhs" | grep -c -F 'touch')"
is "and no # in it is live" "0" "$(printf '%s\n' "$vhs" | sed 's/##//g' | grep -c '#')"
VHD='d#(touch${IFS}F)#S'; mkdir "$VQ/$VHD"; : > "$VOUT"
env -i COCKPIT_TUI_BIN="$OPTUI" PATH="$VTMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$VOUT" \
    CLICK_DIR="$VQ" CLICK_LINE="$VHD" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
vhs=$(grep -F 'RUNSHELL' "$VOUT")
is "a hostile directory name reaches the shim's run-shell, doubled, none live" "1 0" \
   "$(printf '%s\n' "$vhs" | grep -c -F 'touch') $(printf '%s\n' "$vhs" | sed 's/##//g' | grep -c '#')"
# The shim sits BETWEEN code(1) and $VISUAL/$EDITOR, so the two things it must
# never displace are the ones above it: an explicit @cockpit_editor, and a real
# code(1) on a host that has one.
: > "$VOUT"
env -i COCKPIT_TUI_BIN="$OPTUI" PATH="$VTMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$VOUT" \
    CLICK_DIR="$VQ" CLICK_LINE="it's.txt" OPEN_EDITOR=nvim \
    bash "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
case "$(cat "$VOUT")" in
    *NEWWIN*nvim*) ok "@cockpit_editor still wins over the shim" ;;
    *) bad "@cockpit_editor still wins over the shim" "a new window running nvim" "$(cat "$VOUT")" ;;
esac
printf '#!/bin/sh\nexit 0\n' > "$VTMUX/code"; chmod +x "$VTMUX/code"
: > "$VOUT"
env -i COCKPIT_TUI_BIN="$OPTUI" PATH="$VTMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$VOUT" \
    CLICK_DIR="$VQ" CLICK_LINE="it's.txt:12" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
case "$(cat "$VOUT")" in
    *"--vscode"*) bad "code(1) on PATH still beats the shim" "code --goto" "$(cat "$VOUT")" ;;
    *"RUNSHELL run-shell -b code --goto"*) ok "code(1) on PATH still beats the shim" ;;
    *) bad "code(1) on PATH still beats the shim" "a run-shell carrying code --goto" "$(cat "$VOUT")" ;;
esac
rm -f "$VTMUX/code"

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
# The picker is cockpit-tui's now (#246); this block is about which directory the
# script hands it, so a stub that names its -C and leaves like esc stands in.
printf '#!/bin/sh\necho "picker for $*" >&2\nexit 1\n' > "$NOCOLD/cockpit-tui"; chmod +x "$NOCOLD/cockpit-tui"
# Without -p: the hazard is real -- it resolves into cliB, not the popup's
# actual origin (cliA), because cliB is the busier client.
bugout=$(env PATH="$NOCOLD" COCKPIT_TUI_BIN="$NOCOLD/cockpit-tui" TMUX="$SOCKPATH,0,0" "$DIR/scripts/tmux-branch" </dev/null 2>&1)
case "$bugout" in
    *"$DB"*) ok "unpinned resolution really does land on the busier client (the hazard is real)" ;;
    *) bad "unpinned resolution really does land on the busier client (the hazard is real)" "$DB" "$bugout" ;;
esac
# With -p pane_id: pinned to the pane the popup actually opened from.
fixout=$(env PATH="$NOCOLD" COCKPIT_TUI_BIN="$NOCOLD/cockpit-tui" TMUX="$SOCKPATH,0,0" "$DIR/scripts/tmux-branch" -p "$paneA" </dev/null 2>&1)
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

echo "== install.sh's closing note reads every config tmux loads (#296) =="
IW=$(mktmpd); mkdir -p "$IW/h/.config/tmux" "$IW/bin"
# The configs the note names: it asks for the run-shell line only when none has it.
iw_note() {
    env -u XDG_CONFIG_HOME HOME="$IW/h" XDG_BIN_HOME="$IW/bin" "$DIR/install.sh" 2>&1 >/dev/null \
        | sed -n 's/^Add this to \(.*\), AFTER.*/\1/p'
}
printf 'set -g mouse on\n' > "$IW/h/.config/tmux/tmux.conf"
printf 'run-shell %s/cockpit.tmux\n' "$DIR" > "$IW/h/.tmux.conf"
is "wired in ~/.tmux.conf beside an unwired XDG config: no note" "" "$(iw_note)"
rm "$IW/h/.tmux.conf"
is "wired in neither: the note names the first config" "$IW/h/.config/tmux/tmux.conf" "$(iw_note)"
unset -f iw_note
rm -rf "$IW"

echo "== install.sh --check: one row per thing that can be wrong, exit on the worst =="
# The box is faked whole -- HOME, $BIN, PATH shims, an empty TMUX_TMPDIR so no
# tmux server is reachable -- so every row is the code's answer, never this
# machine's, and the doctor cannot read the live server.
if ! command -v jq >/dev/null 2>&1; then skip_ "install.sh --check (no jq)"; else
DC=$(mktmpd)
DH="$DC/home"; DBIN="$DC/bin"; DSH="$DC/shim"; DNO="$DC/notmux"
mkdir -p "$DH/.claude" "$DBIN" "$DSH" "$DNO" "$DC/tmuxtmp"
printf '# mine\nrun-shell %s/cockpit.tmux\n' "$DIR" > "$DH/.tmux.conf"
dshim() { printf '#!/bin/sh\necho "%s"\n' "$2" > "$3/$1"; chmod +x "$3/$1"; }
dshim revdiff "version: 1.13.0" "$DSH"
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
is "vnum: 1.12.9 is below the 1.13.0 floor, and only that way" "no yes" \
   "$(dcge 1.12.9 1.13.0) $(dcge 1.13.0 1.12.9)"

DCP="$DBIN:$DSH:$SHIM:/usr/bin:/bin"
before=$(find "$DH" "$DBIN" | sort)
dcout=$(dc "$DCP"); dcrc=$?
is "the doctor writes nothing" "$before" "$(find "$DH" "$DBIN" | sort)"
is "revdiff at the pinned version is an ok row" "1" "$(dcrow "$dcout" "ok revdiff")"
is "no bat row at all (nothing uses it, #228)" "0" "$(printf '%s\n' "$dcout" | dcrows | grep -c ' bat$')"
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
# No optional tool is left (bat went with the previews, #228): a warn row is a
# revdiff older than the tested one.
mkdir -p "$DC/oldrd"; dshim revdiff "version: 1.12.0" "$DC/oldrd"
dcout=$(dc "$DBIN:$DC/oldrd:$SHIM:$DNO"); dcrc=$?
is "an older revdiff is a warn row" "1" "$(dcrow "$dcout" "warn revdiff")"
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
# A retired option still set is a warn row; a config without one prints no row.
is "a config without a retired option has no options row" "0" "$(dcrow "$dcout" "warn options")"
printf '%s\n' 'set -g @cockpit_keep_swap_pane on' >> "$DH/.tmux.conf"
dcout=$(dc "$DCP")
is "a config still setting @cockpit_keep_swap_pane is a warn options row (#223)" "1" "$(dcrow "$dcout" "warn options")"
cp "$DC/conf.bak" "$DH/.tmux.conf"
# A bare unset carries no value after the name, and still names the option.
printf '%s\n' 'set -gu @cockpit_keep_swap_pane' >> "$DH/.tmux.conf"
dcout=$(dc "$DCP")
is "so does a bare set -gu @cockpit_keep_swap_pane" "1" "$(dcrow "$dcout" "warn options")"
cp "$DC/conf.bak" "$DH/.tmux.conf"
printf '%s\n' 'set -g @cockpit_keep_swap_pane_x on' >> "$DH/.tmux.conf"
dcout=$(dc "$DCP")
is "but an option that only starts with the name does not" "0" "$(dcrow "$dcout" "warn options")"
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

echo "== bootstrap.sh: no bat and no --full (#228); the first backup kept =="
# A file:// release tree, a faked box, and a sudo shim that would exit 99 if
# anything reached for it. Nothing here opens a socket, touches the real HOME
# or talks to a tmux server.
if ! command -v curl >/dev/null 2>&1; then skip_ "bootstrap.sh (no curl)"
elif ! command -v sha256sum >/dev/null 2>&1; then skip_ "bootstrap.sh (no sha256sum)"; else
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
chmod +x "$BSH/sudo"
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
mkrel() {                     # mkrel [noasset]
    local a d
    rm -rf "$REL" "$BS/pack"; mkdir -p "$BS/pack"
    # revdiff is fetched on every run, so a release tree without it would fail them all.
    d="$REL/umputun/revdiff/releases/download/v1.13.0"; mkdir -p "$d"
    printf '#!/bin/sh\necho "version: 1.13.0"\n' > "$BS/pack/revdiff"; chmod +x "$BS/pack/revdiff"
    for a in linux_amd64 linux_arm64 darwin_amd64 darwin_arm64; do
        ( cd "$BS/pack" && tar -czf "$d/revdiff_1.13.0_$a.tar.gz" revdiff )
    done
    ( cd "$d" && sha256sum revdiff_1.13.0_*.tar.gz > revdiff_1.13.0_checksums.txt )
}
bssaid() {                    # bssaid <label> <literal line the freeze spells>
    case "$3" in
        *"$2"*) ok "$1" ;;
        *) bad "$1" "$2" "$3" ;;
    esac
}

mkrel
# bat went with tmux-open-path's previews (#228), and --full with it: that tier
# fetched nothing else. A bootstrap run names no bat anywhere.
bsout=$(bs --dry-run); bsrc=$?
is "a run names no bat" "0" "$(printf '%s\n' "$bsout" | grep -ci 'bat\b')"
is "and names no latest-tools step" "0" "$(printf '%s\n' "$bsout" | grep -c 'latest tools')"
is "and the run exits 0" "0" "$bsrc"
bsout=$(bs --full --dry-run); bsrc=$?
is "--full is a no-op: the run still exits 0" "0" "$bsrc"
bssaid "and says so on one line" "bootstrap.sh: --full does nothing now: fzf and bat are not used" "$bsout"
is "and the normal run happened (the revdiff step ran)" "1" "$(printf '%s\n' "$bsout" | grep -c '== revdiff ==')"
bsout=$(bs --latest --dry-run); is "--latest is a no-op too (exit 0)" "0" "$?"
bsclear_check=$(bs --check --yes); is "--check runs alone" "2" "$?"
bssaid "and says so" "--check runs alone" "$bsclear_check"
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

echo "== the path picker fires in a window with a split =="
# Before 3.8, tmux's own C-MouseDown1Pane was swap-pane, fired on the FIRST press
# of a Ctrl+double-click, and moved the second click to another pane, so the
# double-click never formed. 3.8 binds nothing there and the plugin no longer
# overrides it (#223); TWO panes keep this proving that a press does not swap.
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
if [ -x "$DIR/bin/cockpit-tui" ]; then
    tap 12 1
    # the sessions view marks your own session (here).
    if until_cap "$TO" o '*session>*(here)*'; then ok "tapping the session name opens the session picker"
    else bad "tapping the session name opens the session picker" "the picker's prompt and the (here) row" "$("$TMUX_REAL" -L "$TO" capture-pane -p -t o | head -5)"; fi
else
    skip_ "tapping the session name opens the session picker (no bin/cockpit-tui)"
fi
"$TMUX_REAL" -L "$TO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TI" kill-server >/dev/null 2>&1
fi

echo "== a held prefix turns the corner's ≡ into a reversed accent block (#271) =="
# #{client_prefix} in a border format resolves for the client drawing it, and
# tmux redraws borders when the key table changes, so the mark appears on the
# prefix key and clears on the next one, with no wait for status-interval.
if need_tmux "prefix mark"; then :; else
PI="cockpit-pfxi-$$"; PO="cockpit-pfxo-$$"; SOCKS="$SOCKS $PI $PO"
"$TMUX_REAL" -L "$PI" -f /dev/null new-session -d -s cockpit -x 80 -y 20 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$PI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$PO" -f /dev/null new-session -d -s o -x 80 -y 20 \
    "TERM=xterm-256color exec $TMUX_REAL -L $PI attach -t cockpit" >/dev/null 2>&1
until_cap "$PO" o '*≡*cockpit*'
top() { "$TMUX_REAL" -L "$PO" capture-pane -p -e -t o | head -1; }
until_top() {                 # <glob>: the top line, with escapes, matches
    local i
    for i in $(seq 1 "$POLLS"); do
        # shellcheck disable=SC2254
        case "$(top)" in $1) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
RV=$'\033[7m'
case "$(top)" in *"$RV"*) bad "idle: no reversed block" "plain ≡" "$(top)" ;; *) ok "idle: ≡ is plain" ;; esac
"$TMUX_REAL" -L "$PO" send-keys -t o C-b
if until_top "*${RV}*≡*"; then ok "prefix held: ≡ is drawn in reverse"
else bad "prefix held: ≡ is drawn in reverse" "ESC[7m before ≡" "$(top)"; fi
case "$(top)" in *"range"*) bad "range markup is consumed" "no literal range" "$(top)" ;; *) ok "no range markup leaks into the border" ;; esac
"$TMUX_REAL" -L "$PO" send-keys -t o Escape
for _ in $(seq 1 "$POLLS"); do case "$(top)" in *"$RV"*) sleep 0.1 ;; *) break ;; esac; done
case "$(top)" in *"$RV"*) bad "prefix released: the mark clears" "no ESC[7m" "$(top)" ;; *) ok "prefix released: the mark clears" ;; esac
# the tap range works while the prefix is held
"$TMUX_REAL" -L "$PO" send-keys -t o C-b
until_top "*${RV}*" >/dev/null
# shellcheck disable=SC2046
"$TMUX_REAL" -L "$PO" send-keys -t o -H $(printf '\033[<0;5;1M\033[<0;5;1m' | od -An -tx1)
if until_cap "$PO" o '*New window*'; then ok "tapping ≡ with the prefix held opens the menu"
else bad "tapping ≡ with the prefix held opens the menu" "the menu's New window row" "$("$TMUX_REAL" -L "$PO" capture-pane -p -t o | head -3)"; fi
# the format itself: the range wraps the block, the block is reversed accent
F=$("$TMUX_REAL" -L "$PI" show-option -gqv @cockpit_corner_fmt)
case "$F" in
    "#[range=control|1]#{?client_prefix,#[fg=themeblue]#[reverse],#[fg=themewhite]}"*"#[norange]"*) ok "the corner format: range around a prefix-aware ≡ style" ;;
    *) bad "the corner format: range around a prefix-aware ≡ style" "range, then client_prefix style" "$F" ;;
esac
"$TMUX_REAL" -L "$PO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$PI" kill-server >/dev/null 2>&1
fi

echo "== @open-editor still works under its new name =="
optui_setup
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
    env COCKPIT_TUI_BIN="$OPTUI" PATH="$RT" TMUX_LOG="$RT/log" CLICK_DIR="$RN" CLICK_LINE="f.txt" "$@" \
        "$DIR/scripts/tmux-open-path" --pick %1 </dev/null >/dev/null 2>&1
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

echo "== a key's switch-client moves the client that pressed it, not the last active (#296) =="
if need_tmux "switch-client -c"; then :; else
SC="cockpit-sc-$$"; SOCKS="$SOCKS $SC"; SCD=$(mktmpd)
"$TMUX_REAL" -L "$SC" -f /dev/null new-session -d -s a -c "$SCD" >/dev/null 2>&1
"$TMUX_REAL" -L "$SC" new-session -d -s z -c "$SCD" 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$SC" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
SCT=$("$TMUX_REAL" -L "$SC" list-panes -t z -F '#{pane_id}')
SCS=$("$TMUX_REAL" -L "$SC" list-panes -t a -F '#{pane_id}')
"$TMUX_REAL" -L "$SC" set -p -t "$SCT" @panestate needs-input >/dev/null 2>&1
# Two control clients on a, over fifos held open read-write. B attaches last, so
# B is the one a bare switch-client picks; the scripts are handed A.
sc_open() {
    mkfifo "$SCD/$1" && "$TMUX_REAL" -L "$SC" -C attach -t a <> "$SCD/$1" >/dev/null 2>&1 &
}
sc_open A; SCPA=$!
until_out '?*' "$TMUX_REAL" -L "$SC" list-clients -F '#{client_name}'
SCA=$("$TMUX_REAL" -L "$SC" list-clients -F '#{client_name}')
sc_open B; SCPB=$!
until_out "*"$'\n'"*" "$TMUX_REAL" -L "$SC" list-clients -F '#{client_name}'
sc_where() {                  # -> "A=<session> B=<session>"
    "$TMUX_REAL" -L "$SC" list-clients -F '#{client_name} #{client_session}' 2>/dev/null \
        | awk -v a="$SCA" '{ print ($1 == a ? "A" : "B") "=" $2 }' | sort | tr '\n' ' '
}
sc_run() { env TMUX="$SOCKDIR/$SC,0,0" COCKPIT_LOG="$SCD/log" "$@" </dev/null >/dev/null 2>&1; }
sc_run "$DIR/scripts/tmux-jump" "$SCS" "$SCA"
is "tmux-jump moves the client it is given" "A=z B=a " "$(sc_where)"
SCB=$("$TMUX_REAL" -L "$SC" list-clients -F '#{client_name}' | grep -vxF "$SCA")
sc_home() { for c in "$SCA" "$SCB"; do "$TMUX_REAL" -L "$SC" switch-client -c "$c" -t a; done \
    >/dev/null 2>&1; }
sc_home
# The stub board notes the pin it finds, then plays B pressing the key on the same
# pane while this board runs, then jumps.
cat > "$SCD/tui" <<SCTUI
#!/bin/sh
tmux show -pqv -t $SCS @cockpit_board_client > "$SCD/seen"
tmux set -p -t $SCS @cockpit_board_client "$SCB"
echo "jump $SCT" > "\$COCKPIT_ACTION"
SCTUI
chmod +x "$SCD/tui"
# The popup's key leaves its client on the pane; the board takes it at start.
"$TMUX_REAL" -L "$SC" set -p -t "$SCS" @cockpit_board_client "$SCA" >/dev/null 2>&1
sc_run env COCKPIT_TUI_BIN="$SCD/tui" "$DIR/scripts/cockpit-board" -p "$SCS"
is "the board takes the pin at start, so a later press cannot take its jump" "A=z B=a :" \
   "$(sc_where):$(cat "$SCD/seen")"
sc_home
"$TMUX_REAL" -L "$SC" set -pu -t "$SCS" @cockpit_board_client >/dev/null 2>&1
sc_run env COCKPIT_TUI_BIN="$SCD/tui" "$DIR/scripts/cockpit-board" -p "$SCS" -c "$SCA"
is "cockpit-board -c moves that client, whatever the pane holds" "A=z B=a " "$(sc_where)"
# The bindings hand the pressing client over: run-shell expands it at the press.
"$TMUX_REAL" -L "$SC" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
sc_keys() { for k in u w r; do "$TMUX_REAL" -L "$SC" list-keys -T prefix "$k" 2>/dev/null \
    | grep -c "#{client_name}"; done | tr '\n' ' '; }
is "prefix u, w and r pass #{client_name}" "1 1 1 " "$(sc_keys)"
"$TMUX_REAL" -L "$SC" set -g @cockpit_board_window on >/dev/null 2>&1
"$TMUX_REAL" -L "$SC" set -g @cockpit_review_window on >/dev/null 2>&1
"$TMUX_REAL" -L "$SC" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "and so do their window forms" "1 1 1 " "$(sc_keys)"
unset -f sc_open sc_where sc_run sc_keys sc_home
"$TMUX_REAL" -L "$SC" kill-server >/dev/null 2>&1
wait "$SCPA" "$SCPB" 2>/dev/null
rm -rf "$SCD"
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
for key in w f b M e i R; do
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
case "$(ak r)" in *cockpit-board*) ok "giving r back does not undo the review key bound in the same load" ;;
    *) bad "giving r back does not undo the review key bound in the same load" "cockpit-board on r" "$(ak r)" ;; esac
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

echo "== a reload that turns a cockpit key off or moves it gives the old key back (#290) =="
# Every key, not only the menu's and the panel's. "Stock" is what a -f /dev/null
# server of the same tmux binds there, note included; nothing when it binds nothing.
if need_tmux "key give-back"; then :; else
GB="cockpit-gb-$$"; SOCKS="$SOCKS $GB"
GBD=$(mktmpd)
"$TMUX_REAL" -L "$GB" -f /dev/null new-session -d -s t -c "$GBD" >/dev/null 2>&1
# A client, so a load's display-message lands in the message log.
GBO="cockpit-gbo-$$"; SOCKS="$SOCKS $GBO"
"$TMUX_REAL" -L "$GBO" -f /dev/null new-session -d -s o -x 100 -y 30 -c "$GBD" \
    "TERM=xterm-256color exec $TMUX_REAL -L $GB attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$GB" list-clients -F '#{client_name}'
gb_load() { "$TMUX_REAL" -L "$GB" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; }
gb_set() {                    # <name> <key|unset>
    if [ "$2" = unset ]; then "$TMUX_REAL" -L "$GB" set -gu "@cockpit_$1_key"
    else "$TMUX_REAL" -L "$GB" set -g "@cockpit_$1_key" "$2"; fi >/dev/null 2>&1
}
# One call on a fresh socket: a client meeting the last stock server on its way
# out reads nothing (5 in 500 when two calls share one).
gb_on() {                     # <tmux argv...> <key>  the binding, then its note
    local k=${*: -1}; set -- "${@:1:$#-1}"
    "$@" list-keys -T prefix "$k" \; list-keys -T prefix -F '#{key_note}' "$k" 2>/dev/null
}
gb_live()  { gb_on "$TMUX_REAL" -L "$GB" "$1"; }
gb_stock() {
    local d; d=$(mktemp -d "$GBD/stock.XXXXXX") || return
    gb_on "$TMUX_REAL" -S "$d/s" -f /dev/null "$1"; rm -rf "$d"
}
gb_state() { [ "$(gb_live "$1")" = "$(gb_stock "$1")" ] && echo stock || echo cockpit; }
GB_ROWS="menu:a sessions:s jump:u info:i panel:f board:w review:r reload:R move:M fullscreen:e"
gb_load
for row in $GB_ROWS; do
    n=${row%%:*} k=${row#*:}
    before=$(gb_state "$k"); gb_set "$n" off; gb_load
    is "@cockpit_${n}_key off on a reload gives $k back to stock" "cockpit>stock" \
       "$before>$(gb_state "$k")"
    gb_set "$n" unset; gb_load
done
gb_set board off; gb_load
is "an off key leaves no bound record" "" \
   "$("$TMUX_REAL" -L "$GB" show-option -gqv @cockpit_board_bound)"
gb_set board unset; gb_load
for row in $GB_ROWS; do
    n=${row%%:*} k=${row#*:}
    gb_set "$n" F12; gb_load
    is "@cockpit_${n}_key moved to F12 gives $k back and binds F12" "stock>cockpit" \
       "$(gb_state "$k")>$(gb_state F12)"
    gb_set "$n" unset; gb_load
    is "and @cockpit_${n}_key moved back frees F12" "stock" "$(gb_state F12)"
done
# A key the user bound after cockpit let go of it is theirs, not cockpit's to give back.
"$TMUX_REAL" -L "$GB" bind-key w display-message mine >/dev/null 2>&1
gb_set board off; gb_load
is "a reload never gives back a key the user rebound" "display-message mine" \
   "$(gb_live w | grep -o 'display-message mine')"
gb_set board unset; gb_load
# Even one that runs cockpit's own scripts: a key without a note is the user's.
"$TMUX_REAL" -L "$GB" bind-key w run-shell "$DIR/scripts/tm --popup x" >/dev/null 2>&1
gb_set board off; gb_load
is "a user's key running cockpit's own scripts is never given back" "1" \
   "$(gb_live w | grep -c 'scripts/tm --popup x')"
gb_set board unset; gb_load
# The stock binding itself, not read back through the same probe the fix uses.
gb_set board off; gb_load
is "board off gives w back as tmux's own choose-tree -Zw, with its note" \
   "choose-tree -Zw|Choose a window from a list" \
   "$(gb_live w | grep -o 'choose-tree -Zw')|$(gb_live w | grep -o 'Choose a window from a list')"
gb_set board unset; gb_load
# A TMPDIR over 100 characters would make the stock server's socket path too long
# to bind; the read sits on a short base of its own, so w still comes back.
GBL="$GBD/$(printf 'l%.0s' $(seq 1 110))"; mkdir -p "$GBL"
gb_note() { "$TMUX_REAL" -L "$GB" list-keys -T prefix -F '#{key_note}' "$1" 2>/dev/null; }
gb_left() { "$TMUX_REAL" -L "$GB" show-option -gqv @cockpit_giveback; }
gb_set board off
"$TMUX_REAL" -L "$GB" run-shell "TMPDIR='$GBL' '$DIR/cockpit.tmux'" >/dev/null 2>&1
is "a long TMPDIR still gives w back to stock, nothing pending" "choose-tree -Zw|" \
   "$(gb_live w | grep -o 'choose-tree -Zw')|$(gb_left)"
gb_set board unset; gb_load
# A stock read that fails is not "binds nothing": here a stock server that will
# not start, as a socket path past the limit did. The key stays, the record waits.
mkdir -p "$GBD/bin"
printf '#!/bin/sh\nfor a; do [ "$a" = start-server ] && { echo "error connecting (File name too long)" >&2; exit 1; }; done\nexec "%s" "$@"\n' \
    "$TMUX_REAL" > "$GBD/bin/tmux"
chmod +x "$GBD/bin/tmux"
gb_nostock() { "$TMUX_REAL" -L "$GB" run-shell "PATH='$GBD/bin':\"\$PATH\" '$DIR/cockpit.tmux'" >/dev/null 2>&1; }
gb_set board off; gb_nostock
is "a failed stock read leaves w cockpit's and the give-back pending" "cockpit: board|board:w" \
   "$(gb_note w)|$(gb_left)"
is "and says which key and why" "1" "$("$TMUX_REAL" -L "$GB" show-messages 2>/dev/null \
   | grep -v ' command: ' | grep -c "own binding for w (.*too long")"
gb_load
is "the next load gives w back to stock and clears the pending record" \
   "choose-tree -Zw|" "$(gb_live w | grep -o 'choose-tree -Zw')|$(gb_left)"
# Moved, not off: the board's record moves on to F12, the pending one keeps w.
gb_set board unset; gb_load; gb_set board F12; gb_nostock
is "a move whose stock read fails binds F12 and keeps w pending" \
   "cockpit: board|cockpit: board|board:w" "$(gb_note F12)|$(gb_note w)|$(gb_left)"
gb_load
is "and the next load gives w back" "choose-tree -Zw|" \
   "$(gb_live w | grep -o 'choose-tree -Zw')|$(gb_left)"
gb_set board unset; gb_load
# A stock server that starts and answers nothing is a failed read too, never an
# empty binding.
mkdir -p "$GBD/silent"
printf '#!/bin/sh\nfor a; do [ "$a" = start-server ] && exit 0; done\nexec "%s" "$@"\n' \
    "$TMUX_REAL" > "$GBD/silent/tmux"
chmod +x "$GBD/silent/tmux"
gb_set board off
"$TMUX_REAL" -L "$GB" run-shell "PATH='$GBD/silent':\"\$PATH\" '$DIR/cockpit.tmux'" >/dev/null 2>&1
is "a silent stock server leaves w cockpit's, pending, and says no answer" \
   "cockpit: board|board:w|1" "$(gb_note w)|$(gb_left)|$("$TMUX_REAL" -L "$GB" show-messages \
   2>/dev/null | grep -v ' command: ' | grep -c 'own binding for w (no answer)')"
gb_load
is "and the next load gives w back" "choose-tree -Zw|" \
   "$(gb_live w | grep -o 'choose-tree -Zw')|$(gb_left)"
gb_set board unset; gb_load
# /tmp read-only and TMPDIR writable (Termux): the read falls back to TMPDIR.
mkdir -p "$GBD/ro" "$GBD/tt"
# The shim refuses the /tmp template only: the suite's own TMPDIR is under /tmp too.
{ printf '#!/bin/sh\nfor a; do case "$a" in /tmp/cockpit-gb.*) exit 1 ;; esac; done\n'
  printf 'exec "%s" "$@"\n' "$(command -v mktemp)"; } > "$GBD/ro/mktemp"
chmod +x "$GBD/ro/mktemp"
gb_set board off
"$TMUX_REAL" -L "$GB" run-shell \
    "PATH='$GBD/ro':\"\$PATH\" TMPDIR='$GBD/tt' '$DIR/cockpit.tmux'" >/dev/null 2>&1
is "with /tmp unwritable the stock read uses TMPDIR: w back, nothing pending" "choose-tree -Zw|" \
   "$(gb_live w | grep -o 'choose-tree -Zw')|$(gb_left)"
gb_set board unset; gb_load
# ; is a command separator to tmux: the tmux-fixer sweep must still give it back.
"$TMUX_REAL" -L "$GB" bind-key -T prefix '\;' display-popup -E "/old/scripts/tmux-fixer -p %1" \
    >/dev/null 2>&1
gb_load
is "a dead tmux-fixer binding on ; is given back as stock last-pane" "last-pane" \
   "$(gb_live ';' | grep -o last-pane)"
unset -f gb_nostock gb_note gb_left
# Two keys trading places: each is taken this load, so neither is given back.
gb_set sessions u; gb_set jump s; gb_load
is "sessions and jump trading keys keep both bound" "tmux-jump:--popup" \
   "$(gb_live s | grep -o tmux-jump):$(gb_live u | grep -o -- --popup)"
unset -f gb_load gb_set gb_on gb_live gb_stock gb_state
"$TMUX_REAL" -L "$GBO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$GB" kill-server >/dev/null 2>&1
rm -rf "$GBD"
fi

echo "== upgrading from a load before the key notes still gives the old keys back (#290) =="
# The last load before the notes tracked the menu and panel keys and bound them
# with no note. Its cockpit.tmux and scripts, from history; a shallow clone lacks it.
# The ownership test matches the plugin's own path, so both loads share one.
GU_REV=543624d
if need_tmux "key notes upgrade"; then :
elif ! git -C "$DIR" cat-file -e "$GU_REV:cockpit.tmux" 2>/dev/null; then
    skip_ "key notes upgrade (no $GU_REV in this clone)"
else
GU="cockpit-gu-$$"; GU2="cockpit-gu2-$$"; GUO="cockpit-guo-$$"; SOCKS="$SOCKS $GU $GU2 $GUO"
GUD=$(mktmpd); GUD2=$(mktmpd)
for d in "$GUD" "$GUD2"; do
    git -C "$DIR" archive "$GU_REV" cockpit.tmux scripts | tar -x -C "$d"
done
gu_load() { "$TMUX_REAL" -L "$1" run-shell "$2/cockpit.tmux" >/dev/null 2>&1; }
gu_key() { "$TMUX_REAL" -L "$1" list-keys -T prefix "$2" 2>/dev/null; }
# Upgraded in place, as a pull does: the same directory, the new files.
gu_up() { rm -rf "$1/scripts"; cp -r "$DIR/cockpit.tmux" "$DIR/scripts" "$1/"; }
gu_said() { "$TMUX_REAL" -L "$GU" show-messages 2>/dev/null | grep -v ' command: ' \
    | grep -c 'key before the key notes'; }
"$TMUX_REAL" -L "$GU" -f /dev/null new-session -d -s t -c "$GUD" >/dev/null 2>&1
"$TMUX_REAL" -L "$GUO" -f /dev/null new-session -d -s o -x 100 -y 30 -c "$GUD" \
    "TERM=xterm-256color exec $TMUX_REAL -L $GU attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$GU" list-clients -F '#{client_name}'
gu_load "$GU" "$GUD"
is "the old load bound a and f, with no note" "1:1:" \
   "$(gu_key "$GU" a | grep -c display-menu):$(gu_key "$GU" f | grep -c cockpit-panel):$(
      "$TMUX_REAL" -L "$GU" list-keys -T prefix -F '#{key_note}' a)"
"$TMUX_REAL" -L "$GU" set -g @cockpit_menu_key n >/dev/null 2>&1
"$TMUX_REAL" -L "$GU" set -g @cockpit_panel_key F11 >/dev/null 2>&1
gu_up "$GUD"; gu_load "$GU" "$GUD"
is "the old menu key a is free again and the old panel key f is find-window" \
   "|find-window" "$(gu_key "$GU" a)|$(gu_key "$GU" f | grep -o find-window)"
is "and each says so once, and the load marks every key noted" "2:1" \
   "$(gu_said):$("$TMUX_REAL" -L "$GU" show-option -gqv @cockpit_notes)"
gu_load "$GU" "$GUD"
is "a later load says nothing more" "2" "$(gu_said)"
# Past the marker no command is a guess: a user's own centred menu on n stays.
"$TMUX_REAL" -L "$GU" bind-key n display-menu -T '#[align=centre]mine' x x '' >/dev/null 2>&1
"$TMUX_REAL" -L "$GU" set -g @cockpit_menu_key off >/dev/null 2>&1
gu_load "$GU" "$GUD"
is "after the marker a user's centred menu on the old menu key stays" "1" \
   "$(gu_key "$GU" n | grep -c 'align=centre]mine')"
# The guess is the panel's own command: a user's key on f running another of
# cockpit's scripts stays the user's, even on that first load.
"$TMUX_REAL" -L "$GU2" -f /dev/null new-session -d -s t -c "$GUD2" >/dev/null 2>&1
gu_load "$GU2" "$GUD2"
"$TMUX_REAL" -L "$GU2" bind-key f run-shell "$GUD2/scripts/tm --popup x" >/dev/null 2>&1
"$TMUX_REAL" -L "$GU2" set -g @cockpit_panel_key F11 >/dev/null 2>&1
gu_up "$GUD2"; gu_load "$GU2" "$GUD2"
is "the upgrade leaves a user's f running cockpit's tm alone" "1" \
   "$(gu_key "$GU2" f | grep -c 'scripts/tm --popup x')"
# An upgrade load whose stock read fails keeps the guess for the retry.
GU3="cockpit-gu3-$$"; SOCKS="$SOCKS $GU3"; GUD3=$(mktmpd)
git -C "$DIR" archive "$GU_REV" cockpit.tmux scripts | tar -x -C "$GUD3"
# A stock server that will not start, as a socket path past the limit did.
mkdir -p "$GUD3/bin"
printf '#!/bin/sh\nfor a; do [ "$a" = start-server ] && { echo "error connecting" >&2; exit 1; }; done\nexec "%s" "$@"\n' \
    "$TMUX_REAL" > "$GUD3/bin/tmux"
chmod +x "$GUD3/bin/tmux"
"$TMUX_REAL" -L "$GU3" -f /dev/null new-session -d -s t -c "$GUD3" >/dev/null 2>&1
gu_load "$GU3" "$GUD3"
"$TMUX_REAL" -L "$GU3" set -g @cockpit_panel_key F11 >/dev/null 2>&1
gu_up "$GUD3"
"$TMUX_REAL" -L "$GU3" run-shell "PATH='$GUD3/bin':\"\$PATH\" '$GUD3/cockpit.tmux'" >/dev/null 2>&1
is "the failed upgrade load leaves panel:f pending and the marker unset" "panel:f|" \
   "$("$TMUX_REAL" -L "$GU3" show-option -gqv @cockpit_giveback)|$(
      "$TMUX_REAL" -L "$GU3" show-option -gqv @cockpit_notes)"
gu_load "$GU3" "$GUD3"
is "a failed upgrade give-back is retried: f is find-window and nothing is pending" \
   "find-window|" "$(gu_key "$GU3" f | grep -o find-window)|$(
      "$TMUX_REAL" -L "$GU3" show-option -gqv @cockpit_giveback)"
"$TMUX_REAL" -L "$GU3" kill-server >/dev/null 2>&1
rm -rf "$GUD3"
unset -f gu_load gu_key gu_up gu_said
for s in "$GUO" "$GU" "$GU2"; do "$TMUX_REAL" -L "$s" kill-server >/dev/null 2>&1; done
rm -rf "$GUD" "$GUD2"
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
# OP's screen trails the inner client, and under load it can still show the last
# case's prompt ("new session: agents") or menu. A wait for a word that stale
# screen also holds returns at once, the next key lands before the menu is up,
# and Enter picks the menu's first row, agents (#347). So the old menu and
# prompt must be gone first, and the wait is for the menu's own title.
mv_open() {                   # press prefix M and wait for the menu; a no-show is one clean failure
    local i
    for i in $(seq 1 "$POLLS"); do
        case "$("$TMUX_REAL" -L "$MVO" capture-pane -p -t "$1" 2>/dev/null)" in
            *'new session:'*|*'move '[0-9]*:*' to '*) sleep 0.1 ;;
            *) break ;;
        esac
    done
    "$TMUX_REAL" -L "$MVO" send-keys -t "$1" C-b M
    until_cap "$MVO" "$1" '*move [0-9]*:* to *agents*' || bad "the menu opens again" "a menu listing agents" \
        "$("$TMUX_REAL" -L "$MVO" capture-pane -p -t "$1" | tail -3)"
}
mv_client() { "$TMUX_REAL" -L "$MV" list-clients -F '#{session_name}:#{window_name}' 2>/dev/null; }
mv_until() {                  # <pattern> against the client's session:window
    local i
    for i in $(seq 1 "$POLLS"); do
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

echo "== tapping the path on the border opens the pane's folder in the editor =="
# range=control|2 on the border's path replays the cockpit table's o, which opens
# the pane's cwd in the editor (#228).
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
    *"tmux-open-path"*"--folder"*) ok "which browses the pane's folder" ;;
    *) bad "which browses the pane's folder" "a run-shell running --folder" \
         "$("$TMUX_REAL" -L "$DB" list-keys -T cockpit | awk '$4=="o"')" ;;
esac
# The key itself, through a real client: an attached client in the outer
# server's pane gets the cockpit table and an o; a shim editor logs its argv (and must not be run, #363).
DBD=$(mktmpd); DBLOG="$DBD/argv"; : > "$DBLOG"
printf '#!/bin/sh\nfor a in "$@"; do printf "%%s\\n" "$a"; done >> "%s"\n' "$DBLOG" > "$DBD/ed"
chmod +x "$DBD/ed"
"$TMUX_REAL" -L "$DB" set -g @cockpit_editor "$DBD/ed" >/dev/null 2>&1
DBO="cockpit-dbo-$$"; SOCKS="$SOCKS $DBO"
"$TMUX_REAL" -L "$DBO" -f /dev/null new-session -d -s o -x 90 -y 24 -c "$DBD" \
    "$TMUX_REAL -L $DB attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$DB" list-clients -F '#{client_name}'
DBC=$("$TMUX_REAL" -L "$DB" list-clients -F '#{client_name}' | head -1)
"$TMUX_REAL" -L "$DB" switch-client -c "$DBC" -T cockpit >/dev/null 2>&1
"$TMUX_REAL" -L "$DBO" send-keys -t o o
# #363: o opens the file browser popup on the client, not the editor.
until_out '*files>*' "$TMUX_REAL" -L "$DBO" capture-pane -p -t o
case "$("$TMUX_REAL" -L "$DBO" capture-pane -p -t o)" in
    *files\>*) ok "pressing o in the cockpit table opens the file browser popup" ;;
    *) bad "pressing o in the cockpit table opens the file browser popup" "a files> prompt on the client" \
         "$("$TMUX_REAL" -L "$DBO" capture-pane -p -t o)" ;;
esac
is "and the editor was not run" "" "$(cat "$DBLOG")"
"$TMUX_REAL" -L "$DBO" send-keys -t o Escape
until_out 'gone' bash -c "$TMUX_REAL -L $DBO capture-pane -p -t o | grep -q 'files>' || echo gone"
is "esc closes the popup" "" "$("$TMUX_REAL" -L "$DBO" capture-pane -p -t o | grep -c 'files>' | sed 's/^0$//')"
"$TMUX_REAL" -L "$DBO" kill-server >/dev/null 2>&1
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
case "$("$TMUX_REAL" -L "$PG" list-keys -T prefix | awk '$4=="b"')" in
    *'-h "45%" -w "55%"'*) ok "a custom size reaches the binding" ;;
    *) bad "a custom size reaches the binding" '-h 45% -w 55%' \
         "$("$TMUX_REAL" -L "$PG" list-keys -T prefix | awk '$4=="b"')" ;;
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
echo "== tmux-open-path: one match opens at once, several ask (#228) =="
# A single candidate is not a question, a directory included; only a line with
# several candidates reaches the popup.
optui_setup
OS=$(mktmpd); OST=$(mktmpd); : > "$OS/f.txt"; : > "$OS/g.txt"; mkdir "$OS/d"
cp "$ETMUX_TEMPLATE" "$OST/tmux" 2>/dev/null || true
chmod +x "$OST/tmux" 2>/dev/null
if [ ! -x "$OST/tmux" ]; then skip_ "one match opens at once"; skip_ "several ask"
else
ln -s "$(command -v bash)" "$OST/bash"
os_run() { : > "$OST/log"
    env -i PATH="$OST" HOME="$HOME" TMUX_LOG="$OST/log" CLICK_DIR="$OS" \
        CLICK_LINE="$1" OPEN_EDITOR=nvim COCKPIT_TUI_BIN="$OPTUI" \
        bash "$DIR/scripts/tmux-open-path" %1 >/dev/null 2>&1
    cat "$OST/log"; }
case "$(os_run 'f.txt:7')" in
    *NEWWIN*) ok "one match with a line opens at once" ;;
    *) bad "one match with a line opens at once" "the editor, no popup" "$(os_run 'f.txt:7')" ;;
esac
case "$(os_run 'f.txt')" in
    *NEWWIN*) ok "one match without a line opens at once" ;;
    *) bad "one match without a line opens at once" "the editor, no popup" "$(os_run 'f.txt')" ;;
esac
case "$(os_run 'd')" in
    *NEWWIN*) ok "a lone directory opens at once" ;;
    *) bad "a lone directory opens at once" "the editor, no popup" "$(os_run 'd')" ;;
esac
case "$(os_run 'f.txt g.txt')" in
    *POPUP*) ok "two matches ask" ;;
    *) bad "two matches ask" "the picker popup" "$(os_run 'f.txt g.txt')" ;;
esac
# --folder (the cockpit table's o, the menu's Browse folder) opens the folder browser
# popup (#363; it opened the pane's folder in the editor before, #228).
: > "$OST/log"
env -i PATH="$OST" HOME="$HOME" TMUX_LOG="$OST/log" CLICK_DIR="$OS" OPEN_EDITOR=nvim \
    bash "$DIR/scripts/tmux-open-path" --folder %1 >/dev/null 2>&1
case "$(cat "$OST/log")" in
    *NEWWIN*) bad "--folder opens the browser popup, not the editor" "a POPUP running --browse" "$(cat "$OST/log")" ;;
    *POPUP*"--browse %1"*) ok "--folder opens the browser popup, not the editor" ;;
    *) bad "--folder opens the browser popup, not the editor" "a POPUP running --browse" "$(cat "$OST/log")" ;;
esac
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
is "beside agent-state off and session, not instead of them" "3" "$(jq '.hooks.SessionStart | length' "$HK/.claude/settings.json")"
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
    { "matcher": "", "hooks": [ { "type": "command", "command": "\"${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}\"/scripts/review-gate", "timeout": 330 } ] },
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
rm -rf "$HK"

# --uninstall --hooks (#292): remove exactly the entries --hooks added. Every
# run points HOME and XDG_BIN_HOME at a temp dir, never the real settings.json.
echo "== install.sh --uninstall --hooks removes only cockpit's entries =="
if command -v jq >/dev/null 2>&1; then
UH=$(mktmpd); mkdir -p "$UH/.claude"
cat > "$UH/.claude/settings.json" <<'EOF'
{ "model": "opus",
  "hooks": {
    "Stop": [ { "hooks": [ { "type": "command", "command": "my-own-tool" } ] } ],
    "Other": [ { "matcher": "Bash", "hooks": [ { "type": "command", "command": "/usr/bin/scripts/agent-state done" } ] } ] },
  "zkey": { "keep": true } }
EOF
cp "$UH/.claude/settings.json" "$UH/orig.json"
uhrun() { HOME="$UH" XDG_BIN_HOME="$UH/bin" "$DIR/install.sh" "$@"; }
uhrun --hooks >/dev/null 2>&1
cp "$UH/.claude/settings.json" "$UH/merged.json"
out=$(uhrun --uninstall --hooks --dry-run 2>&1)
case "$out" in *agent-state*"dry-run: nothing written"*) ok "--dry-run previews the removal" ;;
    *) bad "--dry-run previews the removal" "a diff naming agent-state" "$out" ;; esac
is "and --dry-run writes nothing" "$(cat "$UH/merged.json")" "$(cat "$UH/.claude/settings.json")"
uhrun >/dev/null 2>&1
out=$(uhrun --hooks --remove 2>&1); is "removal exits clean" "0" "$?"
[ -L "$UH/bin/tm" ] && ok "--hooks --remove leaves the symlinks in place" || bad "--hooks --remove leaves the symlinks in place" "$UH/bin/tm a link" "gone"
uhrun --remove >/dev/null 2>&1; is "--remove without --hooks is a usage error" "2" "$?"
uhrun --uninstall --hooks --remove >/dev/null 2>&1; is "--remove with --uninstall is a usage error" "2" "$?"
case "$out" in *"removed   cockpit hooks"*) ok "and says what it did" ;; *) bad "and says what it did" "removed cockpit hooks" "$out" ;; esac
is "only the cockpit entries are gone" "$(jq -S . "$UH/orig.json")" "$(jq -S . "$UH/.claude/settings.json")"
is "key order is kept" "$(jq -c 'keys_unsorted' "$UH/orig.json")" "$(jq -c 'keys_unsorted' "$UH/.claude/settings.json")"
is "a timestamped backup holds the pre-removal file" "$(cat "$UH/merged.json")" \
   "$(cat "$UH"/.claude/settings.json.bak-* 2>/dev/null)"
uhrun --hooks >/dev/null 2>&1
is "--hooks adds them back identically (round trip)" "$(cat "$UH/merged.json")" "$(cat "$UH/.claude/settings.json")"
uhrun --uninstall --hooks >/dev/null 2>&1
out=$(uhrun --uninstall --hooks 2>&1)
case "$out" in *"no cockpit hooks"*"nothing to remove"*) ok "no cockpit hooks is a no-op that says so" ;;
    *) bad "no cockpit hooks is a no-op that says so" "nothing to remove" "$out" ;; esac

# --hooks replaces only cockpit's own review-gate / agent-state off, never a foreign one.
UF=$(mktmpd); mkdir -p "$UF/.claude"
cat > "$UF/.claude/settings.json" <<'EOF'
{ "hooks": {
  "Stop": [ { "hooks": [ { "type": "command", "command": "\"/home/u/my dir/foo/scripts/review-gate\"" } ] } ],
  "SessionStart": [ { "matcher": "startup|resume|clear", "hooks": [ { "type": "command", "command": "/opt/x/scripts/agent-state off" } ] } ] } }
EOF
HOME="$UF" XDG_BIN_HOME="$UF/bin" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "a foreign review-gate survives --hooks" "1" \
   "$(jq '[.hooks.Stop[].hooks[].command | select(test("my dir/foo/scripts/review-gate"))] | length' "$UF/.claude/settings.json")"
is "a foreign agent-state off survives --hooks" "1" \
   "$(jq '[.hooks.SessionStart[].hooks[].command | select(. == "/opt/x/scripts/agent-state off")] | length' "$UF/.claude/settings.json")"
is "and cockpit's own review-gate was added beside it" "1" \
   "$(jq '[.hooks.Stop[].hooks[].command | select(test("^\"\\$\\{TMUX_COCKPIT_DIR.*/scripts/review-gate$"))] | length' "$UF/.claude/settings.json")"

UB=$(mktmpd); mkdir -p "$UB/.claude"; printf '{ "hooks": [ broken' > "$UB/.claude/settings.json"
out=$(HOME="$UB" XDG_BIN_HOME="$UB/bin" "$DIR/install.sh" --uninstall --hooks 2>&1); urc=$?
is "invalid JSON is refused" "1" "$urc"
case "$out" in *"not valid JSON"*) ok "with a clear message" ;; *) bad "with a clear message" "not valid JSON" "$out" ;; esac
is "and the file is unchanged" '{ "hooks": [ broken' "$(cat "$UB/.claude/settings.json")"
case "$(ls "$UB/.claude")" in *bak*) bad "and no backup is made" "none" "$(ls "$UB/.claude")" ;; *) ok "and no backup is made" ;; esac

UM=$(mktmpd)
out=$(HOME="$UM" XDG_BIN_HOME="$UM/bin" "$DIR/install.sh" --uninstall --hooks 2>&1); is "a missing settings.json exits 0" "0" "$?"
case "$out" in *"nothing to remove"*) ok "and says so" ;; *) bad "and says so" "nothing to remove" "$out" ;; esac
[ -e "$UM/.claude/settings.json" ] && bad "and creates nothing" "no settings.json" "created" || ok "and creates nothing"
else
    skip_ "hooks removal (jq not installed)"
fi

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
#
# The proof is that the script returns while the writer still holds the FIFO,
# not a wall clock: a "<=3 s" bound read 12 s at load 30, the script's own
# start-up and tmux calls being slow, with the 2 s deadline intact (#215). The
# writer holds for 60 s, so only a script that waits for EOF can see it gone.
# The bound is still checked, under 30 s: the deadline is 2 s and the rest is
# slack for load, so a script that waits out most of the writer still fails.
# An interrupted run must not leave the 60 s writer behind: the suite's EXIT
# trap kills it until the block is done, then goes back to plain cleanup.
CXFIFO="$STATE/agent-context-stall.fifo"; mkfifo "$CXFIFO"
( exec 9>"$CXFIFO"; exec sleep 60 ) & holder=$!
trap 'kill "$holder" 2>/dev/null; cleanup' EXIT
start=$SECONDS
TMUX="$CXTMUX" TMUX_PANE="$CXP" "$DIR/scripts/agent-context" >/dev/null 2>&1 < "$CXFIFO"
took=$(( SECONDS - start ))
if kill -0 "$holder" 2>/dev/null && [ "$took" -lt 30 ]; then ok "a stalling writer is abandoned in time"
else bad "a stalling writer is abandoned in time" "back under 30s, while the writer still held on" "${took}s"; fi
kill "$holder" 2>/dev/null; wait "$holder" 2>/dev/null
trap cleanup EXIT
rm -f "$CXFIFO"
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
# unstaged edit existed the whole index went missing from the list. `git diff
# HEAD` is both halves in one call. FAIL-first: on the old code no staged.txt row exists.
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
# dropped from the list WITHOUT a word. FAIL-first: no café-new.txt row at all.
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
# The notice is a row like any other: the same four fields.
is "the cap notice has a row's four fields, so no row is short" \
   "4" "$(printf '%s' "$rvcapout" | awk -F'\t' '{print NF}')"
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
is "the byte cap's notice has a row's four fields too" \
   "4" "$(printf '%s' "$rvwideout" | awk -F'\t' '{print NF}')"
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
rvgoldwant=$(printf '%s\n' \
  'tests/unit_test.go	-2	q	tests/unit_test.go:2 (-)  q' \
  'tests/unit_test.go	2	QQ	tests/unit_test.go:2 (+)  QQ' \
  'app.go	-2	b	app.go:2 (-)  b' \
  'app.go	2	ZED	app.go:2 (+)  ZED' \
  'app.go	4	ADDED	app.go:4 (+)  ADDED')
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

echo "== tmux-review --lines: a subdir pane's cwd still resolves to the repo root =="
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

echo "== repo git config cannot run commands through a review =="
GH=$(mktmpd); GHM="$GH/marker"
# The marker is the whole signal, so a plant never reads stdin (#214): git hands
# core.fsmonitor, textconv and the diff commands its caller's stdin, and a bare
# cat there blocked forever whenever that was an open terminal or socket.
for h in fsmon textconv dcmd extdiff clean proc gpg pager; do
    printf '#!/bin/sh\necho %s >> %s\ncat </dev/null\n' "$h" "$GHM" > "$GH/$h"; chmod +x "$GH/$h"
done
( cd "$GH" && mkdir r && cd r && git init -q -b main . \
  && printf 'a\n' > f.txt && printf '* diff=evil filter=evil\n' > .gitattributes \
  && git add -A && git -c user.name=t -c user.email=t@t commit -qm i && printf 'b\n' >> f.txt \
  && mkdir -p hk && printf '#!/bin/sh\necho hook >> %s\n' "$GHM" > hk/post-checkout && chmod +x hk/post-checkout \
  && git config core.fsmonitor "$GH/fsmon" && git config diff.evil.textconv "$GH/textconv" \
  && git config diff.evil.command "$GH/dcmd" && git config diff.external "$GH/extdiff" \
  && git config filter.evil.clean "$GH/clean" && git config log.showSignature true \
  && git config gpg.program "$GH/gpg" && git config core.hooksPath hk && git config core.pager "$GH/pager" ) >/dev/null 2>&1
# Plain git on purpose: the control that proves the plants fire. Its </dev/null and
# the ones below are the second guard, should a plant ever read stdin again.
git -C "$GH/r" diff HEAD </dev/null >/dev/null 2>&1
case "$(cat "$GHM" 2>/dev/null)" in *fsmon*) ok "the planted config fires under plain git" ;;
    *) bad "the planted config fires under plain git" "fsmon in the marker" "$(cat "$GHM" 2>/dev/null)" ;; esac
rm -f "$GHM"
"$DIR/scripts/tmux-review" --lines "$GH/r" </dev/null >/dev/null 2>&1
is "and nothing it names runs under tmux-review" "" "$(cat "$GHM" 2>/dev/null)"
case "$("$DIR/scripts/tmux-review" --lines "$GH/r" </dev/null 2>/dev/null)" in *f.txt*) ok "while the review still lists the change" ;;
    *) bad "while the review still lists the change" "a row for f.txt" "$("$DIR/scripts/tmux-review" --lines "$GH/r" </dev/null 2>&1 | head -3)" ;; esac

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
pwrcs=$(pwrun --deliver "$PWN" "$PWD_SHIM/store" "$PWD_SHIM"; pwrun --request "$PWN"; pwrun --delta "$PWN"; pwrun -p "$PWN")
is "deliver, request, delta and -p all refuse it" "1 2 1 2 " "$pwrcs"
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
# Both shells' prompts, before anything is typed or pasted into them: see pane B below.
until_cap "$RDS" "$AP" '*[$#]*'
until_cap "$RDS" "$NP" '*[$#]*'

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
    for i in $(seq 1 "$POLLS"); do
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
# pane_current_command reads claude the moment exec -a runs, before bash has read
# its rc files. A paste that lands before readline is up is echoed by the tty,
# unbracketed, and bash then runs its lines: pane B showed only the prompt and
# the review's first line (#215). The prompt is drawn by readline, so wait for it.
until_cap "$RDS" "$AP2" '*[$#]*'
until_cap "$RDS" "$NP2" '*[$#]*'

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
if need_tmux "tmux-review -p deliver store cleanup"; then :; else
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

"$TMUX_REAL" -L "$FD" new-window -d -t t: -c "$FD_DIRTY" 'exec -a claude bash' >/dev/null 2>&1
FD_AGENT=$("$TMUX_REAL" -L "$FD" list-panes -t t:1 -F '#{pane_id}')
# The review below is delivered to this pane by its command, so wait for it.
until_out claude "$TMUX_REAL" -L "$FD" display-message -p -t "$FD_AGENT" '#{pane_current_command}'

# What r on a board row runs: main_entry() with -p, straight into review_pane().
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" "'$DIR/scripts/tmux-review' -p $FD_AGENT" Enter
rvup=0
for i in 1 2 3 4 5 6 7 8 9 10; do
    case "$("$TMUX_REAL" -L "$FD" capture-pane -p -t "$CTL2")" in
        *"review · base"*) rvup=1; break ;;
    esac
    sleep 0.5
done
if [ "$rvup" != 1 ]; then
    bad "tmux-review -p opens that pane's review loop" "a visible 'review · base' prompt" \
        "$("$TMUX_REAL" -L "$FD" capture-pane -p -t "$CTL2")"
else
    ok "tmux-review -p opens that pane's review loop"
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
    *"fix this"*) ok "the review still gets delivered from the board's r" ;;
    *) bad "the review still gets delivered from the board's r" \
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
# Both preconditions, polled rather than slept for: the agent pane's own shell
# has to be up before the review runs, and deliver()'s if-shell reads @panestate back off the pane at paste time.
for i in $(seq 1 20); do
    [ "$("$TMUX_REAL" -L "$RK" display-message -p -t "$RK_AGENT" '#{pane_current_command}')" = claude ] &&
    [ "$("$TMUX_REAL" -L "$RK" show-option -pqv -t "$RK_AGENT" @panestate)" = needs-input ] && break
    sleep 0.1
done

"$TMUX_REAL" -L "$RK" send-keys -t "$RKCTL" "'$DIR/scripts/tmux-review' -p $RK_AGENT" Enter
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

echo "== prefix r opens the board, prefix R reloads; off unbinds each =="
if need_tmux "cockpit_review_key"; then :; else
RB="cockpit-rb-$$"; SOCKS="$SOCKS $RB"
"$TMUX_REAL" -L "$RB" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$RB" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$RB" list-keys -T prefix 2>/dev/null | awk '$4=="r"')" in
    *display-popup*cockpit-board*) ok "prefix r opens the board in a popup by default" ;;
    *) bad "prefix r opens the board in a popup by default" \
         "a display-popup running cockpit-board" \
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
    *cockpit-board*) bad "@cockpit_review_key off leaves r to stock tmux" \
         "no cockpit-board in the binding" \
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
    *new-window*cockpit-board*) ok "@cockpit_review_window on opens the board in a window instead of the popup" ;;
    *) bad "@cockpit_review_window on opens the board in a window instead of the popup" \
         "a new-window running cockpit-board" \
         "$("$TMUX_REAL" -L "$RB3" list-keys -T prefix | awk '$4=="r"')" ;;
esac
# new-window activates the review window before its command runs, so a pane
# resolved from inside it is the review window's own; the bind pins the
# source pane into @cockpit_review_src first and the window reads that (#45).
# A copy of the plugin whose tmux-review is a stub records the -p it got.
RB3O="cockpit-rb3o-$$"; SOCKS="$SOCKS $RB3O"
mkdir -p "$RB3D/p/scripts"; cp "$DIR/cockpit.tmux" "$RB3D/p/"; cp -r "$DIR"/scripts/. "$RB3D/p/scripts/"
printf '#!/bin/sh\nprintf %%s "$*" > %s/args; sleep 30\n' "$RB3D" > "$RB3D/p/scripts/cockpit-board"; chmod +x "$RB3D/p/scripts/cockpit-board"
"$TMUX_REAL" -L "$RB3" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$RB3" run-shell "$RB3D/p/cockpit.tmux" >/dev/null 2>&1
RB3S=$("$TMUX_REAL" -L "$RB3" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$RB3O" -f /dev/null new-session -d -s o -x 100 -y 30 "TERM=xterm-256color exec $TMUX_REAL -L $RB3 attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$RB3" list-clients -F '#{client_name}'
"$TMUX_REAL" -L "$RB3O" send-keys -t o C-b r
until_file "$RB3D/args" '*-p *'
# And the client that pressed it, for the board's jump (#296).
is "the board window is told the pane the key was pressed in, not its own, and the client" \
   "-p $RB3S -c $("$TMUX_REAL" -L "$RB3" list-clients -F '#{client_name}')" \
   "$(cat "$RB3D/args" 2>/dev/null)"
"$TMUX_REAL" -L "$RB3O" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$RB3" kill-server >/dev/null 2>&1
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
# `tui-v0.1.0` died in cargo-dist's plan step (#79) and was never released, so
# it is no longer read as 0.1.0 (#223): it is a tag like any other, and absent.
tfout=$(tf "$TFR" --version tui-v0.1.0)
case "$tfout" in
    *"no release tui-v0.1.0"*) ok "the pre-#79 tui-v tag form is just a missing tag" ;;
    *) bad "the pre-#79 tui-v tag form is just a missing tag" "skip: no release tui-v0.1.0 ..." "$tfout" ;;
esac
tfout=$(tf "$TFR" --version); tfrc=$?
is "--version with no tag is a usage error, not a silent install of the pin" "2" "$tfrc"

# A refusal must shout on EVERY press. Marking it tried would turn the one
# signal that says "these bytes are not ours" into a single line nobody re-reads.
tf_release cockpit-tui-v0.2.0 0.2.0 flip
tfout=$(tf "$TFR" --version cockpit-tui-v0.2.0); tfrc=$?
is "a flipped checksum exits non-zero" "1" "$tfrc"
case "$tfout" in
    *REFUSED*"does not match its published checksum"*) ok "and says so instead of installing" ;;
    *) bad "and says so instead of installing" "REFUSED ... does not match its published checksum" "$tfout" ;;
esac
is "and the working binary is untouched" "cockpit-tui 0.1.0" "$(tf_ver)"
tfout=$(tf "$TFR" --version cockpit-tui-v0.2.0); tfrc=$?
is "the refusal repeats on the next press" "1" "$tfrc"
case "$tfout" in
    *REFUSED*) ok "and is never silenced by a tried marker" ;;
    *) bad "and is never silenced by a tried marker" "REFUSED" "$tfout" ;;
esac
is "a refusal leaves no marker behind" "" "$(tf_mark)"

# A checksum says the bytes arrived intact, not that they are the version the
# tag claims -- fzf's installer runs the same behavioural check.
tf_release cockpit-tui-v0.3.0 9.9.9
tfout=$(tf "$TFR" --version cockpit-tui-v0.3.0); tfrc=$?
is "a tarball whose binary reports another version installs nothing" "1" "$tfrc"
is "and the working binary is still the old one" "cockpit-tui 0.1.0" "$(tf_ver)"

# An uncut tag is the ordinary state before the first release, and it is
# transient: the marker must not lock the next press out of trying again.
: > "$TF/curlcount"
tfout=$(PATH="$TF/curlshim:$PATH"; tf "$TFGONE" --version cockpit-tui-v0.4.0); tfrc=$?
is "a release host it cannot reach exits 0" "0" "$tfrc"
case "$tfout" in
    *"no release cockpit-tui-v0.4.0 (private repository, or offline)"*) ok "and names the tag rather than blaming the network" ;;
    *) bad "and names the tag rather than blaming the network" "skip: no release cockpit-tui-v0.4.0 (private repository, or offline)" "$tfout" ;;
esac
is "and leaves no marker" "" "$(tf_mark)"
tfout=$(PATH="$TF/curlshim:$PATH"; tf "$TFGONE" --version cockpit-tui-v0.4.0)
is "so the next press fetches again instead of saying already tried" "2" "$(wc -l < "$TF/curlcount" | tr -d ' ')"
# A box that could not ask must not fall through to a full cargo build.
tfout=$(PATH="$TF/shim:$TF/curlshim:$PATH"; tf "$TFGONE" --version cockpit-tui-v0.4.0)
case "$tfout" in
    *"needs Rust"*|*CARGO_RAN*) bad "without --required, an unreachable host never starts a source build" "no rustc gate, no cargo" "$tfout" ;;
    *) ok "without --required, an unreachable host never starts a source build" ;;
esac

# Deleting the binary after a good install must reinstall it, not report a
# marker left over from the install that worked.
tf_release cockpit-tui-v0.5.0 0.5.0
tfout=$(tf "$TFR" --version cockpit-tui-v0.5.0)
is "a new version installs over the old one" "cockpit-tui 0.5.0" "$(tf_ver)"
rm -f "$TFB"
tfout=$(tf "$TFR" --version cockpit-tui-v0.5.0)
is "and deleting the binary refetches it" "cockpit-tui 0.5.0" "$(tf_ver)"

# The source fallback. The floor is READ from tui/Cargo.toml, never pinned
# twice: Debian 13 ships rustc 1.85 against ratatui's 1.88, so this refusal is
# the common case and it must not turn into a page of compiler errors.
mkdir -p "$TF/plugin/tui"
printf 'rust-version = "1.88"\n' > "$TF/plugin/tui/Cargo.toml"
tf_release cockpit-tui-v0.6.0 - noasset
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --version cockpit-tui-v0.6.0); tfrc=$?
case "$tfout" in
    *"no binary for this platform"*) ok "a manifest without this platform is a settled answer" ;;
    *) bad "a manifest without this platform is a settled answer" "no binary for this platform" "$tfout" ;;
esac
is "and it is remembered, so the next press does not re-ask" "cockpit-tui-v0.6.0" "$(tf_mark)"
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
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --version cockpit-tui-v0.6.0)
case "$tfout" in
    *"has no release build for"*) ok "the marker skips the download on the next press" ;;
    *) bad "the marker skips the download on the next press" "cockpit-tui-v0.6.0 has no release build for" "$tfout" ;;
esac
case "$tfout" in
    *"needs Rust >= 1.88"*) ok "and still offers the source build, which the marker must never block" ;;
    *) bad "and still offers the source build, which the marker must never block" "needs Rust >= 1.88" "$tfout" ;;
esac
printf 'rust-version = "1.99"\n' > "$TF/plugin/tui/Cargo.toml"
tf_release cockpit-tui-v0.7.0 - noasset
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --version cockpit-tui-v0.7.0)
case "$tfout" in
    *"needs Rust >= 1.99"*) ok "the floor comes from Cargo.toml, not a second pin in the script" ;;
    *) bad "the floor comes from Cargo.toml, not a second pin in the script" "needs Rust >= 1.99" "$tfout" ;;
esac
# --required is the install path (#199): a binary that runs is success, and
# none at all is exit 1 naming both ways out, whether settled or offline.
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --required --version cockpit-tui-v0.7.0); tfrc=$?
is "--required with a working binary left exits 0" "0" "$tfrc"
mv "$TFB" "$TF/keep"
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --required --version cockpit-tui-v0.7.0); tfrc=$?
is "--required with no binary and no way to build one exits 1" "1" "$tfrc"
case "$tfout" in
    *"cockpit-tui is required"*"no Rust >= 1.99"*"gh auth login"*rustup*) ok "and names the floor, gh auth login and rustup" ;;
    *) bad "and names the floor, gh auth login and rustup" "cockpit-tui is required ... Rust >= 1.99 ... gh auth login ... rustup" "$tfout" ;;
esac
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFGONE" --required --version cockpit-tui-v0.4.0); tfrc=$?
is "--required offline with no binary exits 1 too" "1" "$tfrc"
# A gh 404 or no login is transient, yet a required binary must still try
# cargo: a box with Rust and no gh login is the common source install.
mkdir -p "$TF/shim99"
printf '#!/bin/sh\necho "rustc 1.99.0 (0000000 2026-01-01)"\n' > "$TF/shim99/rustc"
printf '#!/bin/sh\necho "$*" >> "%s"\nexit 1\n' "$TF/cargo99" > "$TF/shim99/cargo"
chmod +x "$TF/shim99/rustc" "$TF/shim99/cargo"
tfout=$(PATH="$TF/shim99:$PATH"; tf "$TFGONE" --required --version cockpit-tui-v0.4.0)
case "$(cat "$TF/cargo99" 2>/dev/null)" in
    *"build --release --locked"*) ok "--required with an unreachable release and Rust >= the floor runs cargo" ;;
    *) bad "--required with an unreachable release and Rust >= the floor runs cargo" "cargo build --release --locked" "$tfout" ;;
esac
is "and marks nothing tried, the miss being transient" "no" "$([ "$(tf_mark)" = cockpit-tui-v0.4.0 ] && echo yes || echo no)"
# A binary that runs but is below TUI_MIN is no binary (#269): with the release
# unreachable, --required must still build, not exit 0 on the stale one.
mkdir -p "${TFB%/*}"; printf '#!/bin/sh\necho "cockpit-tui 0.2.0"\n' > "$TFB"; chmod +x "$TFB"
rm -f "$TF/cargo99"
tfout=$(PATH="$TF/shim99:$PATH"; tf "$TFGONE" --required); tfrc=$?
is "and the failed build leaves nothing at TUI_MIN, so it exits 1" "1" "$tfrc"
case "$(cat "$TF/cargo99" 2>/dev/null)" in
    *"build --release --locked"*) ok "--required with a binary below TUI_MIN and no release runs cargo" ;;
    *) bad "--required with a binary below TUI_MIN and no release runs cargo" "cargo build --release --locked" "$tfout" ;;
esac
case "$tfout" in
    *"--required builds from source next"*) ok "and the skip line says the source build follows" ;;
    *) bad "and the skip line says the source build follows" "... --required builds from source next" "$tfout" ;;
esac
# Rust 1.99 is new enough, so the closing line blames the build, not the toolchain (#243).
case "$tfout" in
    *"cockpit-tui is required"*"the source build failed"*) ok "a build that ran and failed is named as such" ;;
    *) bad "a build that ran and failed is named as such" "cockpit-tui is required ... the source build failed" "$tfout" ;;
esac
case "$tfout" in
    *"no Rust >= "*) bad "and it does not claim there is no Rust" "no 'no Rust >=' line" "$tfout" ;;
    *) ok "and it does not claim there is no Rust" ;;
esac
mv "$TF/keep" "$TFB"

# A verified binary that will not execute here is this box's problem, not a
# broken release: source is the way out, so it must not exit 1 like a refusal.
tf_release cockpit-tui-v0.8.0 0.8.0 broken
tfout=$(PATH="$TF/shim:$PATH"; tf "$TFR" --version cockpit-tui-v0.8.0); tfrc=$?
is "a verified binary that cannot run here exits 0" "0" "$tfrc"
case "$tfout" in
    *"does not run here"*"needs Rust >= 1.99"*) ok "and falls through to the source build" ;;
    *) bad "and falls through to the source build" "does not run here ... needs Rust >= 1.99" "$tfout" ;;
esac
is "and leaves the working binary in place" "cockpit-tui 0.5.0" "$(tf_ver)"

tf_release cockpit-tui-v0.9.0 0.9.0 flat
tfout=$(tf "$TFR" --version cockpit-tui-v0.9.0)
is "an archive with no wrapping directory installs too" "cockpit-tui 0.9.0" "$(tf_ver)"
# COCKPIT_TUI_BIN is where it reads and writes, as for the doctor and wrappers.
tfout=$(env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TF/else/ct" COCKPIT_TUI_RELEASES="$TFR" \
        "$TF/plugin/scripts/tui-fetch" --version cockpit-tui-v0.9.0 2>&1)
is "COCKPIT_TUI_BIN names the file it installs" "cockpit-tui 0.9.0" "$("$TF/else/ct" --version 2>/dev/null)"
tfout=$(env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TF/else/ct" "$TF/plugin/scripts/tui-fetch" --version cockpit-tui-v0.9.0 2>&1)
case "$tfout" in
    *"already current"*) ok "and the binary it reads" ;;
    *) bad "and the binary it reads" "already current" "$tfout" ;;
esac
# A bare name is not a path: it falls back to the plugin's own bin/ (#243),
# never a file named ct in whatever directory the fetch ran from.
mkdir -p "$TF/cwd"
tfout=$(cd "$TF/cwd" && env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN=ct COCKPIT_TUI_RELEASES="$TFR" \
        "$TF/plugin/scripts/tui-fetch" --version cockpit-tui-v0.9.0 2>&1)
is "a relative COCKPIT_TUI_BIN falls back to the plugin's bin/" "absent cockpit-tui 0.9.0" \
   "$([ -e "$TF/cwd/ct" ] && echo present || echo absent) $(tf_ver)"
# The check lives in tui.sh, so every reader gets it, not tui-fetch alone.
is "and every script that sources tui.sh resolves it the same way" "$DIR/bin/cockpit-tui" \
   "$(cd "$TF/cwd" && COCKPIT_TUI_BIN=ct bash -c '. "$1/scripts/lib/tui.sh" && printf %s "$COCKPIT_TUI_BIN"' _ "$DIR")"
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

tf_release cockpit-tui-v1.0.0 1.0.0
: > "$TF/curlcount"; : > "$TF/ghlog"
tfout=$(PATH="$TF/ghshim:$TF/curlfake:$PATH" env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" --version cockpit-tui-v1.0.0 2>&1); tfrc=$?
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
tf_release cockpit-tui-v1.1.0 1.1.0
: > "$TF/ghlog"
tfout=$(PATH="$TF/ghshim:$TF/curlfake:$PATH" COCKPIT_TUI_REPO=alice/example \
    env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" --version cockpit-tui-v1.1.0 2>&1)
case "$(cat "$TF/ghlog")" in
    *"-R alice/example"*) ok "COCKPIT_TUI_REPO overrides the repo gh asks" ;;
    *) bad "COCKPIT_TUI_REPO overrides the repo gh asks" "-R alice/example" "$(cat "$TF/ghlog")" ;;
esac

tf_release cockpit-tui-v1.2.0 1.2.0
: > "$TF/curlcount"; : > "$TF/ghlog"
tfout=$(PATH="$TF/ghbad:$TF/curlfake:$PATH" env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" --version cockpit-tui-v1.2.0 2>&1); tfrc=$?
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
tfout=$(PATH="$TF/ghshim:$TF/curlfake:$TF/shim:$PATH" env -u COCKPIT_TUI_OFFLINE COCKPIT_TUI_BIN="$TFB" "$TF/plugin/scripts/tui-fetch" --version cockpit-tui-v9.9.0 2>&1); tfrc=$?
is "a tag gh has no release for still exits 0" "0" "$tfrc"
case "$tfout" in
    *"no release cockpit-tui-v9.9.0 via gh (or offline)"*) ok "and the skip names the gh path" ;;
    *) bad "and the skip names the gh path" "skip: no release cockpit-tui-v9.9.0 via gh (or offline)" "$tfout" ;;
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

echo "== cockpit-tui: docs name only real options =="
# Three README lines went stale on review phase 3 and cost a round; this asserts
# every @cockpit_* / COCKPIT_* name in docs/tui.md is one some code answers to.
# The freeze (#61 workflow §3) is agreed before the code lands, so its names are
# allowed early -- each is listed with the lane that introduces it.
TUI_FROZEN="@cockpit_tui_version:L4 @cockpit_board_key:L6 \
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
case "$(bkey "$B" r)" in *cockpit-board*) ok "prefix r opens the board, with no binary installed" ;;
    *) bad "prefix r opens the board, with no binary installed" "a display-popup running cockpit-board" "$(bkey "$B" r)" ;;
esac
case "$(bkey "$B" a)" in *'Board of every agent'*) ok "the prefix a menu carries a board row that replays w" ;;
    *) bad "the prefix a menu carries a board row that replays w" "a 'Board of every agent' item" "$(bkey "$B" a)" ;;
esac
case "$(bkey "$B" a)" in *'Review its diff'*) bad "the menu has no review row beside the board's" "no 'Review its diff' item" "$(bkey "$B" a)" ;;
    *) ok "the menu has no review row beside the board's" ;;
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
# The reads the loop makes, @cockpit_board_return (from $BD/ret), @cockpit_glyphs
# (#293, unset) and a jump's @cockpit_board_client (#296, unset), are not
# recorded: what is asserted is what ran.
cat > "$BD/rec/tmux" <<BDREC
#!/bin/sh
case "\$*" in "show-option -gqv @cockpit_board_return") cat "$BD/ret" 2>/dev/null; exit 0 ;; esac
case "\$*" in "show-option -gqv @cockpit_glyphs") exit 0 ;; esac
case "\$*" in "show-option -pqv -t "*" @cockpit_board_client ; set-option -pu -t "*) exit 0 ;; esac
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
case "$(wact 'deliver %1')" in *'not wired up yet'*) ok "deliver %1 is reserved and says so" ;;
    *) bad "deliver %1 is reserved and says so" "a display-message" "$(wact 'deliver %1')" ;;
esac
is "only the FIRST line of the action file is ever acted on" \
   "switch-client -t %9;select-window -t %9;select-pane -t %9;" "$(wact $'jump %9\nreview %1')"

# The return trip (D9): review and panel bring the board back on the pane
# they acted on; jump, q and the off switch end it.
bruns() { tr '\n' ';' < "$BD/bargv"; }
wact 'jump %9' >/dev/null
is "jump ends the loop: the board ran once" "board;" "$(bruns)"
wact 'review %1' >/dev/null
is "review %1 brings the board back with --select %1, and no note" "board;board --select %1;" "$(bruns)"
wact 'panel %1' >/dev/null
case "$(bruns)" in "board;board --select %1 --note "*"/note;cockpit: sent to %1;") ok "panel %1 comes back with the panel's last line as the note" ;;
    *) bad "panel %1 comes back with the panel's last line as the note" "board;board --select %1 --note .../note;cockpit: sent to %1;" "$(bruns)" ;;
esac
wact 'deliver %1' >/dev/null
is "deliver, still reserved, ends the loop" "board;" "$(bruns)"
# The binary creates .expect new and refuses one that is there: the loop
# clears the last trip's before each run, so it never stands in for the next.
cat > "$BD/p/bin/cockpit-tui" <<BDBIN
#!/bin/sh
if [ -e "$BD/once" ]; then
    [ -e "\$COCKPIT_ACTION.expect" ] && printf stale >> "$BD/bexp"
    exit 0
fi
: > "$BD/once"
printf 'server\n' > "\$COCKPIT_ACTION.expect"
printf 'review %%1\n' > "\$COCKPIT_ACTION"
BDBIN
chmod +x "$BD/p/bin/cockpit-tui"; rm -f "$BD/once" "$BD/bexp"
PATH="$BD/rec:$PATH" COCKPIT_LOG="$BD/tui.log" COCKPIT_TUI_BIN="$BD/p/bin/cockpit-tui" "$BD/p/scripts/cockpit-board" -p %3 </dev/null >/dev/null 2>&1
is "a trip's .expect is gone before the board runs again" "" "$(cat "$BD/bexp" 2>/dev/null)"
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
   "0 display-message -t %3 cockpit: board needs cockpit-tui: run $(cd -P "$BD/p" && pwd)/install.sh" \
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
# =t: and not t: a bare t is tried as a window name prefix before a session, and
# automatic-rename can name an agent window "tmux" (the fork before its exec).
is "jump ends the loop, on the pane it jumped to" "$RBX" "$("$TMUX_REAL" -L "$RB" display -p -t =t: '#{pane_id}')"
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

echo "== cockpit-board: x asks first, y writes kill %N and its pane and window ids as .expect =="
# The real binary in a real window, with COCKPIT_ACTION as the wrapper sets it:
# the confirm owns the keys (D18), ↵ never sends (D8), and a pane that goes
# while it is open closes it with a note.
if need_tmux "the board's confirm"; then :; else
KB="cockpit-kb-$$"; SOCKS="$SOCKS $KB"; KBD=$(mktmpd)
mkdir -p "$KBD/state"
"$TMUX_REAL" -L "$KB" -f /dev/null new-session -d -s t -x 100 -y 30 'exec sleep 3600' >/dev/null 2>&1
KBP=()
# Both windows are named claude, as agent windows tend to be: the name cannot
# tell them apart, so .expect is the window id.
for n in claude claude; do
    KBP+=("$("$TMUX_REAL" -L "$KB" new-window -d -n "$n" -P -F '#{pane_id}' -t t: 'exec -a claude sleep 3600')")
done
for p in "${KBP[@]}"; do
    "$TMUX_REAL" -L "$KB" set -p -t "$p" @panestate running >/dev/null 2>&1
done
kb_open() {                   # <pane> <action-dir> -> KBW, a board on that pane
    KBW=$("$TMUX_REAL" -L "$KB" new-window -d -P -F '#{pane_id}' -t t: \
        "env COCKPIT_READONLY=1 XDG_STATE_HOME=$KBD/state COCKPIT_ACTION=$2/act $COCKPIT_TUI_BIN board --select $1")
    until_cap "$KB" "$KBW" '*board · 2 agents*'
}
kb_cap() { "$TMUX_REAL" -L "$KB" capture-pane -p -J -t "$KBW" 2>/dev/null; }
kb_gone() {                   # <pane>
    local i
    for i in $(seq 1 "$POLLS"); do
        "$TMUX_REAL" -L "$KB" list-panes -a -F '#{pane_id}' 2>/dev/null | grep -qx "$1" || return 0
        sleep 0.1
    done
    return 1
}
KA=$(mktmpd)
kb_open "${KBP[0]}" "$KA" || bad "the board opens on the first" "board · 2 agents" "$(kb_cap)"
"$TMUX_REAL" -L "$KB" send-keys -t "$KBW" x
until_cap "$KB" "$KBW" "*kill pane ${KBP[0]} (t:1 claude)?*" || bad "x asks, naming the pane" "kill pane ${KBP[0]} (t:1 claude)?" "$(kb_cap)"
case "$(kb_cap)" in *'y kill   n back   esc back   ? keys   q close'*) ok "x asks, naming the pane, and the bar lists only its keys" ;;
    *) bad "x asks, naming the pane, and the bar lists only its keys" "y kill   n back ..." "$(kb_cap)" ;; esac
# ↵ and j do nothing; n goes back with nothing written
"$TMUX_REAL" -L "$KB" send-keys -t "$KBW" Enter j
"$TMUX_REAL" -L "$KB" send-keys -t "$KBW" n
until_cap "$KB" "$KBW" '*x kill*' || bad "n goes back to the board" "the board's bar" "$(kb_cap)"
is "↵ sends nothing and n backs out: no action, no .expect" "open  " \
   "$("$TMUX_REAL" -L "$KB" list-panes -a -F '#{pane_id}' | grep -qx "$KBW" && printf open) $(cat "$KA/act" 2>/dev/null) $(cat "$KA/act.expect" 2>/dev/null)"
"$TMUX_REAL" -L "$KB" send-keys -t "$KBW" x
until_cap "$KB" "$KBW" "*kill pane ${KBP[0]} (t:1 claude)?*"
"$TMUX_REAL" -L "$KB" send-keys -t "$KBW" y
kb_gone "$KBW" || bad "y closes the board" "the window closes" "$(kb_cap)"
is "y writes kill %N" "kill ${KBP[0]}" "$(cat "$KA/act" 2>/dev/null)"
KBW0=$("$TMUX_REAL" -L "$KB" display -p -t "${KBP[0]}" '#{window_id}')
KBW1=$("$TMUX_REAL" -L "$KB" display -p -t "${KBP[1]}" '#{window_id}')
is "and the pane and its window id, not the shared name, as .expect, 0600" "${KBP[0]} $KBW0 600" "$(cat "$KA/act.expect" 2>/dev/null) $(stat -c %a "$KA/act.expect" 2>/dev/null)"
is "which tells it from the other window named claude" "yes" "$([ -n "$KBW0" ] && [ "$KBW0" != "$KBW1" ] && echo yes)"
is "and the board killed nothing itself" "${KBP[0]}" "$("$TMUX_REAL" -L "$KB" list-panes -a -F '#{pane_id}' | grep -x "${KBP[0]}")"

# #288: R is no longer a board key: it writes nothing, and q then closes
KR=$(mktmpd)
kb_open "${KBP[1]}" "$KR"
"$TMUX_REAL" -L "$KB" send-keys -t "$KBW" R q
kb_gone "$KBW" || bad "q closes the board after R" "the window closes" "$(kb_cap)"
is "R writes no action and no .expect" " no" \
   "$(cat "$KR/act" 2>/dev/null) $([ -e "$KR/act.expect" ] && echo yes || echo no)"

# the target goes while it is asked about: the confirm closes with a ✕ note
KG=$(mktmpd)
kb_open "${KBP[1]}" "$KG"
"$TMUX_REAL" -L "$KB" send-keys -t "$KBW" x
until_cap "$KB" "$KBW" "*kill pane ${KBP[1]} (t:2 claude)?*" || bad "x asks about the second" "kill pane ${KBP[1]} (t:2 claude)?" "$(kb_cap)"
"$TMUX_REAL" -L "$KB" kill-pane -t "${KBP[1]}" >/dev/null 2>&1
until_cap "$KB" "$KBW" '*nothing done: t:2 is gone, kill not sent*' || bad "a gone target closes the confirm with a note" "the ✕ note" "$(kb_cap)"
case "$(kb_cap)" in *'kill pane'*) bad "a gone target closes the confirm with a note" "no confirm" "$(kb_cap)" ;;
    *) ok "a gone target closes the confirm with a note" ;; esac
"$TMUX_REAL" -L "$KB" send-keys -t "$KBW" q
kb_gone "$KBW"
is "and nothing was written" "" "$(cat "$KG/act" 2>/dev/null)$(cat "$KG/act.expect" 2>/dev/null)"
"$TMUX_REAL" -L "$KB" kill-server >/dev/null 2>&1
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

echo "== cockpit-board: the filter survives a reopen (board.state filter line) =="
# Step 4 of the board design (D7): / opens a field that filters as you type,
# every printable key is text there, and a filter kept with enter is written to
# board.state, so the next board opens narrowed the same way.
if need_tmux "the board's find"; then :; else
BN="cockpit-bn-$$"; SOCKS="$SOCKS $BN"; BND=$(mktmpd)
"$TMUX_REAL" -L "$BN" -f /dev/null new-session -d -s alpha -x 100 -y 30 'exec sleep 3600' >/dev/null 2>&1
"$TMUX_REAL" -L "$BN" new-session -d -s beta 'exec sleep 3600' >/dev/null 2>&1
for s in alpha beta; do
    bnp=$("$TMUX_REAL" -L "$BN" new-window -d -P -F '#{pane_id}' -t "$s:" 'exec -a claude sleep 3600')
    "$TMUX_REAL" -L "$BN" set -p -t "$bnp" @panestate running >/dev/null 2>&1
done
bn_open() {                   # -> BNW, a board in its own window, its list drawn
    BNW=$("$TMUX_REAL" -L "$BN" new-window -d -P -F '#{pane_id}' -t alpha: \
        "env XDG_STATE_HOME=$BND COCKPIT_LOG=$BND/tui.log $COCKPIT_TUI_BIN board")
    until_cap "$BN" "$BNW" '*2 sessions*'
}
bn_cap() { "$TMUX_REAL" -L "$BN" capture-pane -p -t "$BNW"; }
bn_quit() {                   # q, then wait for the window to go
    local i
    "$TMUX_REAL" -L "$BN" send-keys -t "$BNW" q
    for i in $(seq 1 "$POLLS"); do
        "$TMUX_REAL" -L "$BN" list-panes -a -F '#{pane_id}' 2>/dev/null | grep -qx "$BNW" || return 0
        sleep 0.1
    done
    return 1
}
bn_state() { tr '\n' ';' < "$BND/tmux-cockpit/board.state" 2>/dev/null | sed 's/sel %[0-9]*;//'; }
bn_open || bad "the board opens on the list" "2 sessions" "$(bn_cap)"
"$TMUX_REAL" -L "$BN" send-keys -t "$BNW" /
until_cap "$BN" "$BNW" '*type to filter*' || bad "/ opens the find field" "type to filter" "$(bn_cap)"
# q is text in the field, not close; backspace takes it back out
"$TMUX_REAL" -L "$BN" send-keys -t "$BNW" q
until_cap "$BN" "$BNW" '*/ q▏*' || bad "q types in the find field" "/ q▏" "$(bn_cap)"
"$TMUX_REAL" -L "$BN" send-keys -t "$BNW" BSpace
"$TMUX_REAL" -L "$BN" send-keys -t "$BNW" -l beta
until_cap "$BN" "$BNW" '*1 of 2*' || bad "typing narrows the list" "1 of 2" "$(bn_cap)"
case "$(bn_cap)" in *'▾ alpha'*) bad "a session with no match hides" "no alpha header" "$(bn_cap)" ;;
    *) ok "a session with no match hides" ;; esac
"$TMUX_REAL" -L "$BN" send-keys -t "$BNW" Enter
until_cap "$BN" "$BNW" '*/ beta · 1 of 2*' || bad "enter keeps the filter, named in the title" "/ beta · 1 of 2" "$(bn_cap)"
bn_quit || bad "q closes the board once the field is closed" "the window gone" "$(bn_cap)"
is "board.state keeps the filter after the folds" "cockpit-board-state 1;filter beta;" "$(bn_state)"
bn_open
until_cap "$BN" "$BNW" '*/ beta · 1 of 2*' || bad "a reopened board is narrowed the same way" "/ beta · 1 of 2" "$(bn_cap)"
case "$(bn_cap)" in *'▾ alpha'*|*'▏'*) bad "and opens with the field closed, alpha still hidden" "no alpha, no cursor" "$(bn_cap)" ;;
    *) ok "and opens with the field closed, alpha still hidden" ;; esac
# esc clears a kept filter before it closes the board (7.1)
"$TMUX_REAL" -L "$BN" send-keys -t "$BNW" Escape
until_cap "$BN" "$BNW" '*▾ alpha*' || bad "esc clears the kept filter and the board stays" "▾ alpha" "$(bn_cap)"
bn_quit
is "and the cleared board saves no filter line" "cockpit-board-state 1;" "$(bn_state)"
"$TMUX_REAL" -L "$BN" kill-server >/dev/null 2>&1
fi

echo "== cockpit-tui board: the list's right pane shows the selection's screen (#284) =="
# D1: at 96 columns and over, the selected agent's live screen fills the right
# pane. It follows the selection, never shows another agent's, and under 96
# columns the pane goes and the list takes the width back.
if need_tmux "the board's right pane"; then :; else
PV="cockpit-pv-$$"; SOCKS="$SOCKS $PV"; PVD=$(mktmpd)
"$TMUX_REAL" -L "$PV" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$PVD" 'exec sleep 3600' >/dev/null 2>&1
PVP=()
for s in alpha beta; do
    PVP+=("$("$TMUX_REAL" -L "$PV" new-window -d -P -F '#{pane_id}' -t t: -c "$PVD" \
        "printf 'screen-of-$s\n'; exec -a claude sleep 3600")")
done
for p in "${PVP[@]}"; do
    "$TMUX_REAL" -L "$PV" set -p -t "$p" @panestate running >/dev/null 2>&1
done
PVW=$("$TMUX_REAL" -L "$PV" new-window -d -P -F '#{pane_id}' -t t: -c "$PVD" \
    "env XDG_STATE_HOME=$PVD COCKPIT_LOG=$PVD/tui.log $COCKPIT_TUI_BIN board")
pv_cap() { "$TMUX_REAL" -L "$PV" capture-pane -p -t "$PVW"; }
# the list's first agent is alpha's: the same state, the lower pane id
if until_cap "$PV" "$PVW" '*live · *screen-of-alpha*'; then ok "at 120 columns the right pane shows the selection's screen"
else bad "at 120 columns the right pane shows the selection's screen" "screen-of-alpha" "$(pv_cap)"; fi
case "$(pv_cap)" in
    *screen-of-beta*) bad "and no other agent's" "no screen-of-beta" "$(pv_cap)" ;;
    *) ok "and no other agent's" ;;
esac
# a short screen is drawn from the top, as the pane shows it: its first row is
# the one under the rule (tmux's choose-tree preview does the same)
is "a short screen starts on the row under the rule" "screen-of-alpha" \
    "$(pv_cap | awk '/live · /{getline; print; exit}' | grep -o 'screen-of-alpha')"
"$TMUX_REAL" -L "$PV" send-keys -t "$PVW" j
if until_cap "$PV" "$PVW" '*live · *screen-of-beta*'; then ok "j moves the pane to the next agent's screen"
else bad "j moves the pane to the next agent's screen" "screen-of-beta" "$(pv_cap)"; fi
"$TMUX_REAL" -L "$PV" resize-window -t "$PVW" -x 90 >/dev/null 2>&1
is "the board's window narrows to 90" 90 "$("$TMUX_REAL" -L "$PV" display -p -t "$PVW" '#{window_width}')"
pv_narrow=
for i in $(seq 1 "$POLLS"); do
    case "$(pv_cap)" in *'live ·'*|*screen-of-*|'') sleep 0.1 ;; *'┌ sessions'*) pv_narrow=1; break ;; *) sleep 0.1 ;; esac
done
if [ -n "$pv_narrow" ]; then ok "under 96 columns the pane goes and the list stays"
else bad "under 96 columns the pane goes and the list stays" "the list, no live screen" "$(pv_cap)"; fi
"$TMUX_REAL" -L "$PV" kill-server >/dev/null 2>&1
fi

echo "== cockpit-tui board: a blank first capture replaces reading the screen (#284) =="
# A pane no capture has reached and a blank capture both draw as empty, so the
# redraw gate must carry whether the pick was read (read_gate). Its own fresh
# server, the blank agent first so the board opens on it, no earlier board.
if need_tmux "the right pane's blank first capture"; then :; else
PB="cockpit-pb-$$"; SOCKS="$SOCKS $PB"; PBD=$(mktmpd)
"$TMUX_REAL" -L "$PB" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$PBD" 'exec sleep 3600' >/dev/null 2>&1
# -n: a window tmux renames as its command execs would redraw the board itself
for cmd in "printf '\\033[2J\\033[H'" 'echo other'; do
    pbp=$("$TMUX_REAL" -L "$PB" new-window -d -n "${cmd%% *}" -P -F '#{pane_id}' -t t: -c "$PBD" "$cmd; exec -a claude sleep 3600")
    "$TMUX_REAL" -L "$PB" set -p -t "$pbp" @panestate running >/dev/null 2>&1
done
PBW=$("$TMUX_REAL" -L "$PB" new-window -d -P -F '#{pane_id}' -t t: -c "$PBD" \
    "env XDG_STATE_HOME=$PBD COCKPIT_LOG=$PBD/tui.log $COCKPIT_TUI_BIN board")
if until_cap "$PB" "$PBW" '*live · blank*'; then ok "a blank first capture shows, not reading the screen"
else bad "a blank first capture shows, not reading the screen" "live · blank" "$("$TMUX_REAL" -L "$PB" capture-pane -p -t "$PBW")"; fi
"$TMUX_REAL" -L "$PB" kill-server >/dev/null 2>&1
fi

echo "== cockpit-tui board: an idle board on a busy agent captures once a tick (#284) =="
# The worker is asked again only when the pick changes (repick): an agent whose
# screen changes every 50 ms redraws the board constantly, and asking on each
# redraw would capture as fast as the fetch can run. A logging tmux wrapper
# counts capture-pane calls over 5 s of an idle board: about one a second.
if need_tmux "the right pane's capture rate"; then :; else
PR="cockpit-pr-$$"; SOCKS="$SOCKS $PR"; PRD=$(mktmpd)
printf '#!/bin/sh\nprintf "%%s\\n" "$1" >> %s/calls\nexec %s "$@"\n' "$PRD" "$TMUX_REAL" > "$PRD/tmux"
chmod +x "$PRD/tmux"
"$TMUX_REAL" -L "$PR" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$PRD" 'exec sleep 3600' >/dev/null 2>&1
prp=$("$TMUX_REAL" -L "$PR" new-window -d -P -F '#{pane_id}' -t t: -c "$PRD" \
    "exec -a claude sh -c 'while :; do date +%s%N; sleep 0.05; done'")
"$TMUX_REAL" -L "$PR" set -p -t "$prp" @panestate running >/dev/null 2>&1
PRW=$("$TMUX_REAL" -L "$PR" new-window -d -P -F '#{pane_id}' -t t: -c "$PRD" \
    "env COCKPIT_TMUX=$PRD/tmux XDG_STATE_HOME=$PRD COCKPIT_LOG=$PRD/tui.log $COCKPIT_TUI_BIN board")
if until_cap "$PR" "$PRW" '*live · *line*'; then
    : > "$PRD/calls"
    sleep 5   # a rate: the count over a fixed window is the measurement
    prn=$(grep -cx capture-pane "$PRD/calls")
    if [ "$prn" -le 7 ]; then ok "an idle board on a busy agent captures about once a second ($prn in 5 s)"
    else bad "an idle board on a busy agent captures about once a second" "at most 7 in 5 s" "$prn"; fi
else bad "the busy agent's screen shows" "live · N lines" "$("$TMUX_REAL" -L "$PR" capture-pane -p -t "$PRW")"; fi
"$TMUX_REAL" -L "$PR" kill-server >/dev/null 2>&1
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
is "the panel never lists a board wake channel" "" "$("$TMUX_REAL" -L "$PN" show-option -gqv @cockpit_board_wake)"

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
# the @cockpit_glyphs read (#293) is not an action: unset, unrecorded
[ "\$*" = "show-option -gqv @cockpit_glyphs" ] && exit 0
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

echo "== cockpit-panel: ^Z hides the panel and the next run on the pane brings it back (#278) =="
# The wrapper in a window stands in for the popup prefix f opens: the window
# closing is the popup closing, and a second run on the same pane is prefix f again.
if need_tmux "cockpit-panel hide and resume"; then :; else
PH="cockpit-ph-$$"; SOCKS="$SOCKS $PH"; PHD=$(mktmpd)
mkdir -p "$PHD/bin" "$PHD/state"
# Never asked: nothing in this block sends a request.
printf '#!/bin/sh\ncat >/dev/null\necho "<text>NEVER CALLED</text>"\n' > "$PHD/bin/claude"
chmod +x "$PHD/bin/claude"
"$TMUX_REAL" -L "$PH" -f /dev/null new-session -d -s t -x 120 -y 40 'exec sleep 300' >/dev/null 2>&1
PHENV="env -u COCKPIT_CLAUDE 'PATH=$PHD/bin:$SHIM:/usr/bin:/bin' XDG_STATE_HOME=$PHD/state XDG_CACHE_HOME=$PHD XDG_CONFIG_HOME=$PHD COCKPIT_LOG=$PHD/tui.log"
ph_open() {                   # <target>  -> PHB, the panel's pane, once it has drawn
    PHB=$("$TMUX_REAL" -L "$PH" new-window -d -P -F '#{pane_id}' -t t: "$PHENV $DIR/scripts/cockpit-panel -p $1")
    until_cap "$PH" "$PHB" '*panel*'
}
ph_gone() {                   # <pane>
    until_out '' sh -c "'$TMUX_REAL' -L '$PH' list-panes -a -F '#{pane_id}' | grep -x '$1'"
}
ph_cap() { "$TMUX_REAL" -L "$PH" capture-pane -p -t "$PHB" 2>/dev/null; }
PHA=$("$TMUX_REAL" -L "$PH" new-window -d -P -F '#{pane_id}' -t t: 'exec cat')
PHC=$("$TMUX_REAL" -L "$PH" new-window -d -P -F '#{pane_id}' -t t: 'exec cat')
PHF="$PHD/state/tmux-cockpit/panel/$("$TMUX_REAL" -L "$PH" display -p '#{pid}')-${PHA#%}.state"
ph_open "$PHA"
until_cap "$PH" "$PHB" '*will send*' || bad "the panel opens on Home" "will send" "$(ph_cap)"
"$TMUX_REAL" -L "$PH" send-keys -t "$PHB" 1
until_cap "$PH" "$PHB" '*type what you want*'
"$TMUX_REAL" -L "$PH" send-keys -t "$PHB" -l 'keep this draft'
until_cap "$PH" "$PHB" '*keep this draft*'
"$TMUX_REAL" -L "$PH" send-keys -t "$PHB" C-z
if ph_gone "$PHB"; then ok "^Z closes the panel"; else bad "^Z closes the panel" "the window gone" "$(ph_cap)"; fi
until_out '?*' ls -- "$PHF" >/dev/null
is "and keeps its state in the pane's own 0600 file, version 2" "600 cockpit-panel-state 2" \
   "$({ stat -c %a "$PHF" 2>/dev/null || stat -f %Lp "$PHF"; }) $(head -n 1 "$PHF" 2>/dev/null)"

# Another pane starts fresh and leaves the hidden one alone.
ph_open "$PHC"
until_cap "$PH" "$PHB" '*will send*'
case "$(ph_cap)" in *'keep this draft'*) bad "another pane's panel starts fresh" "Home" "$(ph_cap)" ;;
    *) ok "another pane's panel starts fresh" ;; esac
"$TMUX_REAL" -L "$PH" send-keys -t "$PHB" q
ph_gone "$PHB"
is "and the hidden one stays saved" "yes" "$([ -f "$PHF" ] && echo yes)"

ph_open "$PHA"
if until_cap "$PH" "$PHB" '*back where you left it*keep this draft*'; then
    ok "the next run on the pane brings the draft back, in Compose"
else
    bad "the next run on the pane brings the draft back, in Compose" "back where you left it ... keep this draft" "$(ph_cap)"
fi
is "nothing was pasted into the pane on the way back" "0" \
   "$("$TMUX_REAL" -L "$PH" capture-pane -p -t "$PHA" | grep -c 'keep this draft')"
is "and no paste buffer was loaded" "" "$("$TMUX_REAL" -L "$PH" list-buffers -F '#{buffer_name}' 2>/dev/null)"
is "the run that took the state owns it: the file is gone" "" "$([ -e "$PHF" ] && echo there)"

# Hidden again, and its pane killed: the next panel anywhere drops it and says so.
"$TMUX_REAL" -L "$PH" send-keys -t "$PHB" C-z
ph_gone "$PHB"
until_out '?*' ls -- "$PHF" >/dev/null
"$TMUX_REAL" -L "$PH" kill-pane -t "$PHA"
ph_open "$PHC"
if until_cap "$PH" "$PHB" "*dropped the panel hidden on $PHA*"; then
    ok "a hidden panel whose pane is gone is dropped with a note"
else
    bad "a hidden panel whose pane is gone is dropped with a note" "dropped the panel hidden on $PHA" "$(ph_cap)"
fi
is "and its file is removed" "" "$([ -e "$PHF" ] && echo there)"
"$TMUX_REAL" -L "$PH" send-keys -t "$PHB" q
ph_gone "$PHB"
unset -f ph_open ph_gone ph_cap
"$TMUX_REAL" -L "$PH" kill-server >/dev/null 2>&1
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
    # 4 tall tiles on an 80x24 tty: the button row is the last inside the border, [ close ] third
    'tap-close': b'\x1b[<0;24;23M\x1b[<0;24;23m',
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


# --- tm without a terminal, and no fzf notice --------------------------------
echo "== tm without a terminal lists and asks; cockpit.tmux says nothing about fzf =="
FZTD=$(mktmpd)
# A fake fzf that only answers --version: what the retired notice read.
fzfake() {                    # fzfake <dir> <version string>
    mkdir -p "$1"
    printf '#!/bin/sh\necho "%s"\n' "$2" > "$1/fzf"; chmod +x "$1/fzf"
}
fzold="$FZTD/old"
fzfake "$fzold" "0.44.1 (debian)"
FZNB="$FZTD/nofzf"; mkdir -p "$FZNB"
for c in bash sh git tput readlink sed grep ls head cat awk sort id dirname basename mktemp rm; do
    p=$(command -v "$c" 2>/dev/null) || continue
    ln -sf "$p" "$FZNB/$c"
done
ln -sf "$SHIM/tmux" "$FZNB/tmux"

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
is "tm -k without a terminal prompts on stderr, where the menu goes" "err" \
   "$(fznofzf tmk "$DIR/scripts/tm" -k)"
FZWANT='Sessions:'
is "tm without a terminal prompts on stderr" "err" \
   "$(fznofzf tma "$DIR/scripts/tm")"

# Nothing uses fzf since #267, so an old one on PATH is not worth a word (#346).
FZNS="cockpit-fzfnotice-$$"; SOCKS="$SOCKS $FZNS"
env -u TMUX -u TMUX_PANE PATH="$fzold:$PATH" "$TMUX_REAL" -L "$FZNS" -f /dev/null \
    new-session -d 'sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FZNS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "an old fzf on PATH draws no notice and sets no option" "0 " \
   "$("$TMUX_REAL" -L "$FZNS" show-messages 2>/dev/null | grep -c fzf) $("$TMUX_REAL" -L "$FZNS" show-option -gqv @cockpit_fzf_notice)"
"$TMUX_REAL" -L "$FZNS" kill-server >/dev/null 2>&1
rm -rf "$FZTD"
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
av_gone() {                   # av_gone <pid>: dead; no pid, or no /proc entry to read, is not gone
    # A killed process whose reaper is slow stays a zombie, and kill -0 succeeds on it. Counted
    # as gone here on the hypothesis that one reaper stall made four runs miss at once (#386).
    [ -n "${1:-}" ] || return 1
    kill -0 "$1" 2>/dev/null || return 0
    local st; st=$(cat "/proc/$1/stat" 2>/dev/null) || return 1
    st=${st##*) }; [ "${st:0:1}" = Z ]
}
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
# The sleep must not outlive the poller. The trap records the sleep's pid from $! itself: a sleep
# that records its own pid can be killed before it first runs, and then there is nothing to read.
AVSP="$AVREC/sleep.pid"; rm -f "$AVSP"
av_tm set-option -g @cockpit_bg_poll 3600
av_tm set-option -gu @cockpit_poll_pid
env -u TMUX_PANE TMUX="$AVT" PATH="$AVBIN:$PATH" AV_JSON="$AVF/done.json" \
    bash -c 'set -T; trap "$1" DEBUG; . "$0"' "$DIR/scripts/agent-poll" \
    '[ "$BASH_COMMAND" = "sleeper=\$!" ] && { echo $! > "'"$AVSP"'"; kill -TERM $$; }' \
    </dev/null >/dev/null 2>&1 &
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
# The sleep itself, in one look: a first look that found a tick's short-lived child and a second
# that found nothing left AVCH empty, and `av_gone` then died on its missing pid.
av_wait 'AVCH=$(pgrep -xP "$AVL" sleep | head -1); [ -n "$AVCH" ]'
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
# A client, so a message has somewhere to show and lands in the log.
AVO="cockpit-avo-$$"; SOCKS="$SOCKS $AVO"; AVOD=$(mktmpd)
"$TMUX_REAL" -L "$AVO" -f /dev/null new-session -d -s o -x 80 -y 24 -c "$AVOD" \
    "TERM=xterm-256color exec $TMUX_REAL -L $AV attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$AV" list-clients -F '#{client_name}'
av_err() {                    # av_err <PATH dir> <fixture> [env...] -> @cockpit_poll_err
    local pdir=$1 j=$2; shift 2
    env -u TMUX_PANE TMUX="$AVT" PATH="$pdir" AV_JSON="$AVF/$j" "$@" \
        "$DIR/scripts/agent-poll" --once </dev/null >/dev/null 2>&1
    av_tm show-option -gqv @cockpit_poll_err
}
is "no jq: the reason is recorded" "no jq on PATH" "$(av_err "$AVE1" blocked.json)"
# The record has a reader: a new reason is said once in the message log (#296).
av_said() { av_tm show-messages 2>/dev/null | grep -v ' command: ' | grep -c "cannot poll: $1"; }
is "and is said in a message" "1" "$(av_said 'no jq on PATH')"
av_err "$AVE1" blocked.json >/dev/null
is "and a poll failing the same way says nothing more" "1" "$(av_said 'no jq on PATH')"
is "no claude: the reason is recorded" "no claude on PATH" "$(av_err "$AVE2" blocked.json)"
is "claude failing: the reason is recorded" "claude agents failed" \
   "$(av_err "$AVE3" blocked.json AV_RC=1)"
is "unreadable json: the reason is recorded" "claude agents gave unreadable json" \
   "$(av_err "$AVE3" garbage.json)"
is "a good tick clears it" "" "$(av_err "$AVE3" blocked.json)"
is "and says nothing" "0" \
   "$(av_tm show-messages 2>/dev/null | grep -v ' command: ' | grep -c 'cannot poll: $')"
unset -f av_said
"$TMUX_REAL" -L "$AVO" kill-server >/dev/null 2>&1
av_tm kill-server >/dev/null 2>&1
rm -rf "$AVOD"
fi


echo "== cockpit-tui diffstat and git-flags match the shell they will replace (#199) =="
DSB=$COCKPIT_TUI_BIN
# tree_diff's rows summed the way the board's scan sums them, as diffstat's four
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

# git.sh keeps a static copy of the lists, so the worktree hook runs with no
# binary; these pin what it actually hands git to the binary's.
GD=$(mktmpd); git init -q "$GD/r" >/dev/null 2>&1
git -C "$GD/r" config filter.evil.clean 'echo pwned'; git -C "$GD/r" config filter.evil.required true
git -C "$GD/r" config filter.lfs.smudge 'echo pwned'; git -C "$GD/r" config filter.a/b.process 'echo pwned'
git -C "$GD/r" config filter.x.y.clean 'echo pwned'; git -C "$GD/r" config filter.noted.foo 1
mkdir "$GD/bin"
printf '#!/bin/sh\ncase " $* " in *" --get-regexp "*) exec %s "$@" ;; esac\nprintf "%%s\\n" "$@"\n' "$(command -v git)" > "$GD/bin/git"
chmod +x "$GD/bin/git"
gs_argv() { ( PATH="$GD/bin:$PATH"; . "$DIR/scripts/lib/git.sh"; git -C "$GD/r" "$@" ); }
is "git.sh hands git exactly git-flags before the caller's -C" \
   "$("$DSB" git-flags)" "$(gs_argv rev-parse HEAD | sed '/^-C$/,$d')"
is "git.sh blanks the same drivers as git-flags --filters" \
   "$("$DSB" git-flags --filters "$GD/r" | paste -d' ' - - | LC_ALL=C sort)" \
   "$(gs_argv status | grep -x -B1 'filter\..*' | grep -v '^--$' | paste -d' ' - - | LC_ALL=C sort)"
# The worktree hook needs no binary: a cockpit-tui on PATH and at COCKPIT_TUI_BIN
# that only leaves a mark, and git.sh still hands git every flag.
printf '#!/bin/sh\necho ran >> %s/tuiran\n' "$GD" > "$GD/bin/cockpit-tui"; chmod +x "$GD/bin/cockpit-tui"
is "git.sh never runs cockpit-tui, and still hands git every flag" "$("$DSB" git-flags)" \
   "$(COCKPIT_TUI_BIN="$GD/bin/cockpit-tui" gs_argv status | sed '/^-c$/{N;/\nfilter\./d}' | sed '/^-C$/,$d')$(cat "$GD/tuiran" 2>/dev/null)"
# git splits -c at the first `=`: filter.a=b.clean= would set filter.a, so both refuse.
git -C "$GD/r" config 'filter.a=b.clean' 'echo pwned'
is "a driver named a=b: git.sh and git-flags --filters both refuse, status 1 and no flags" "1: 1:" \
   "$(gs_argv status 2>/dev/null | tr -d '\n'; printf '%s:' "${PIPESTATUS[0]}"
      printf ' '; "$DSB" git-flags --filters "$GD/r" 2>/dev/null | tr -d '\n'; printf '%s:' "${PIPESTATUS[0]}")"
rm -rf "$GD"
unset -f gs_argv
# The repro: the planted clean runs under a blanked -c, and never under git.sh or diffstat.
GD=$(mktmpd); ( git init -q "$GD/r" && cd "$GD/r" && printf 'hi\n' > x && printf 'x filter=a=b\n' > .gitattributes \
  && git config 'filter.a=b.clean' "touch $GD/PWNED" ) >/dev/null 2>&1
git -C "$GD/r" -c 'filter.a=b.clean=' add x >/dev/null 2>&1
is "a=b's plant fires under git -c filter.a=b.clean=" "yes" "$([ -e "$GD/PWNED" ] && echo yes)"
git -C "$GD/r" read-tree --empty; rm -f "$GD/PWNED"
is "git.sh's git add refuses it in one line, status 1" "git.sh: refusing: filter driver name contains '='
1" "$( ( . "$DIR/scripts/lib/git.sh"; git -C "$GD/r" add x ) 2>&1; printf '%s' "$?")"
"$DSB" diffstat "$GD/r" >/dev/null 2>&1
"$DIR/scripts/tmux-review" --lines "$GD/r" >/dev/null 2>&1
is "and nothing ran: git.sh, diffstat and tmux-review --lines" "" "$(ls "$GD/PWNED" 2>/dev/null)"
rm -rf "$GD"
unset -f ds_shell

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

echo "== cockpit-tui info: the base row follows every base mode =="
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
pi_rs()  { XDG_STATE_HOME="$PISTATE" COCKPIT_TMUX="tmux -L $PI" "$COCKPIT_TUI_BIN" info --text "${1:-$PIAP}" 2>&1; }
is "the Rust base row is uncommitted" "base      uncommitted  (HEAD)" "$(pi_rs | grep '^base ')"
# Every field gather() reads from tmux, the spool and the review dir, as a literal.
"$TMUX_REAL" -L "$PI" set-option -p -t "$PIAP" @cockpit_review_spool "$PISTATE/spool.md" >/dev/null 2>&1
"$TMUX_REAL" -L "$PI" set -g @cockpit_tui_version 9.9.9 >/dev/null 2>&1
PIOUT=$(pi_rs)
is "the pane row" "pane      $PIAP  t:0 w  (claude)" "$(printf '%s\n' "$PIOUT" | grep '^pane ')"
is "the cwd row" "cwd       $PIDIR" "$(printf '%s\n' "$PIOUT" | grep '^cwd ')"
is "the root row" "root      $PIDIR" "$(printf '%s\n' "$PIOUT" | grep '^root ')"
is "the state row carries the message" "state     needs-input  · run-make" "$(printf '%s\n' "$PIOUT" | grep '^state ')"
is "the review row names the queued spool" "review    pending  · queued: $PISTATE/spool.md" "$(printf '%s\n' "$PIOUT" | grep '^review ' | head -n 1)"
is "the session row" "session   sid-9" "$(printf '%s\n' "$PIOUT" | grep '^session ')"
is "the title row" "title     Fix-the-parser" "$(printf '%s\n' "$PIOUT" | grep '^title ')"
case "$(printf '%s\n' "$PIOUT" | grep '^tui ')" in
    *"(want 9.9.9 -- run install.sh)") ok "the tui row names a version that differs" ;;
    *) bad "the tui row names a version that differs" "(want 9.9.9 -- run install.sh)" "$(printf '%s\n' "$PIOUT" | grep '^tui ')" ;;
esac
is "the review dir counts pinned and delivered" "1 pinned · 2 delivered · a at the prompt removes it" \
   "$(printf '%s\n' "$PIOUT" | grep -A1 '^review dir' | tail -n 1 | sed 's/^ *//')"
( cd "$PIDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git checkout -q -b feat; printf 'a\nb\n' > f; git commit -q -am two ) >/dev/null 2>&1
case "$(pi_rs | grep '^base ')" in 'base      vs main  ('*) ok "the Rust base row is vs main" ;; *) bad "the Rust base row is vs main" "vs main" "$(pi_rs | grep '^base ')" ;; esac
git -C "$PIDIR" update-ref refs/cockpit/reviewed/feat HEAD
case "$(pi_rs | grep '^base ')" in 'base      since last review  ('*) ok "the Rust base row is since last review" ;; *) bad "the Rust base row is since last review" "since last review" "$(pi_rs | grep '^base ')" ;; esac
printf 'uncommitted\n' > "$PISESS/comments.md.base"
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
# #293: the binary reads $COCKPIT_GLYPHS only, so the popup passes the option on
case "$(pk_i)" in *@cockpit_glyphs*COCKPIT_GLYPHS*) ok "prefix i passes @cockpit_glyphs to the binary as COCKPIT_GLYPHS" ;;
    *) bad "prefix i passes @cockpit_glyphs to the binary as COCKPIT_GLYPHS" "COCKPIT_GLYPHS from @cockpit_glyphs" "$(pk_i)" ;; esac
"$TMUX_REAL" -L "$PK" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$PK" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PK" set -g @cockpit_info_key off >/dev/null 2>&1
"$TMUX_REAL" -L "$PK" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$(pk_i)" in *cockpit-tui*) bad "@cockpit_info_key off leaves i unbound" "stock i" "$(pk_i)" ;; *) ok "@cockpit_info_key off leaves i unbound" ;; esac
"$TMUX_REAL" -L "$PK" kill-server >/dev/null 2>&1
fi

echo "== cockpit-tui info: values from outside cannot move a field or reach the terminal =="
if need_tmux "cockpit-tui info hostile values"; then :; else
PH="cockpit-ph-$$"; SOCKS="$SOCKS $PH"; PHDIR=$(mktmpd)
"$TMUX_REAL" -L "$PH" -f /dev/null new-session -d -s t -n w -x 100 -y 30 -c "$PHDIR" 'exec bash' >/dev/null 2>&1
PHP=$("$TMUX_REAL" -L "$PH" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$PH" set-option -p -t "$PHP" @cockpit_title "$(printf 'a\037b')" >/dev/null 2>&1
"$TMUX_REAL" -L "$PH" set-option -p -t "$PHP" @cockpit_session sid-1 >/dev/null 2>&1
"$TMUX_REAL" -L "$PH" set -g @cockpit_tui_version "$(printf '9.9.9\033[31mred')" >/dev/null 2>&1
phout=$(COCKPIT_TMUX="tmux -L $PH" "$COCKPIT_TUI_BIN" info --text "$PHP" 2>&1)
is "a US byte in a title is a space, and the session row stays put" "title     a b" "$(grep '^title ' <<<"$phout")"
is "the session row is not shifted by it" "session   sid-1" "$(grep '^session ' <<<"$phout")"
case "$phout" in *$'\033'*) bad "an ESC in @cockpit_tui_version never reaches the screen" "no ESC byte" "$(cat -v <<<"$phout")" ;; *) ok "an ESC in @cockpit_tui_version never reaches the screen" ;; esac
"$TMUX_REAL" -L "$PH" kill-server >/dev/null 2>&1
fi

echo "== cockpit-tui board: i opens the info card over the board and esc returns =="
if need_tmux "board i overlay"; then :; else
PO="cockpit-po-$$"; SOCKS="$SOCKS $PO"; PODIR=$(mktmpd)
"$TMUX_REAL" -L "$PO" -f /dev/null new-session -d -s t -n w -x 100 -y 30 -c "$PODIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PO" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$PO" set-option -p -t t:0.0 @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$PO" new-window -d -t t: -n b "COCKPIT_TMUX='tmux -L $PO' '$COCKPIT_TUI_BIN' board" >/dev/null 2>&1
po_has() {                    # <fixed string> -- polled, 5 s at most
    local n=0
    while [ "$n" -lt 50 ]; do
        "$TMUX_REAL" -L "$PO" capture-pane -p -t t:b | grep -qF -- "$1" && return 0
        sleep 0.1; n=$((n+1))
    done
    return 1
}
po_has "1 agent" && ok "the board is up" || bad "the board is up" "1 agent" "$("$TMUX_REAL" -L "$PO" capture-pane -p -t t:b)"
"$TMUX_REAL" -L "$PO" send-keys -t t:b i
po_has "info · %" && ok "i draws the info card over the board" || bad "i draws the info card over the board" "info · %N" "$("$TMUX_REAL" -L "$PO" capture-pane -p -t t:b)"
"$TMUX_REAL" -L "$PO" send-keys -t t:b Escape
n=0; while [ "$n" -lt 50 ] && "$TMUX_REAL" -L "$PO" capture-pane -p -t t:b | grep -qF "info · %"; do sleep 0.1; n=$((n+1)); done
po_has "1 agent" && ok "esc returns to the board" || bad "esc returns to the board" "1 agent" "$("$TMUX_REAL" -L "$PO" capture-pane -p -t t:b)"
case "$("$TMUX_REAL" -L "$PO" capture-pane -p -t t:b)" in *"info · %"*) bad "the card is gone after esc" "no card" "still drawn" ;; *) ok "the card is gone after esc" ;; esac
"$TMUX_REAL" -L "$PO" kill-server >/dev/null 2>&1
fi

echo "== cockpit-board: kill %N re-checks the pane's window id, kills the pane only (#301); request %N is unknown (#288) =="
# The wrapper half of the two verbs, driven by writing the action file directly
# (a stub binary does the writing). A real private server, real windows; the
# only stub besides the binary is tmux-review, which records its argv.
if need_tmux "the board's kill verb"; then :; else
KV="cockpit-kv-$$"; SOCKS="$SOCKS $KV"; KD=$(mktmpd)
mkdir -p "$KD/p" "$KD/state"
cp -r "$DIR"/scripts "$KD/p/"
printf '#!/bin/sh\nprintf "%%s\\n" "$*" >> %s/rv\n[ ! -e %s/rvout ] || cat %s/rvout\nexit "$(cat %s/rvrc 2>/dev/null || echo 0)"\n' "$KD" "$KD" "$KD" "$KD" > "$KD/p/scripts/tmux-review"
chmod +x "$KD/p/scripts/tmux-review"
# The binary acts once, then exits with no action (the ordinary q). It writes
# $COCKPIT_ACTION.expect before the line, as the board does, when asked to.
cat > "$KD/bin" <<KDBIN
#!/bin/sh
printf '%s\n' "\$*" >> "$KD/bargv"
[ "\${5:-}" = "" ] || cat "\$5" >> "$KD/bargv"
if [ -e "$KD/once" ]; then
    # a second trip in the same wrapper, with no .expect of its own
    [ -e "$KD/line2" ] && [ ! -e "$KD/twice" ] || exit 0
    : > "$KD/twice"; cat "$KD/line2" > "\$COCKPIT_ACTION"; exit 0
fi
: > "$KD/once"
if [ -e "$KD/link" ]; then ln -s "$KD/expect" "\$COCKPIT_ACTION.expect"
elif [ -e "$KD/expect" ]; then cp "$KD/expect" "\$COCKPIT_ACTION.expect"; fi
cat "$KD/line" > "\$COCKPIT_ACTION"
KDBIN
chmod +x "$KD/bin"
"$TMUX_REAL" -L "$KV" -f /dev/null new-session -d -s t -x 120 -y 40 'exec sleep 3600' >/dev/null 2>&1
KVHOME=$("$TMUX_REAL" -L "$KV" list-panes -t t -F '#{pane_id}')
kv_win() {                    # <window-name> -> the pane id of a fresh one-pane window
    "$TMUX_REAL" -L "$KV" new-window -d -P -F '#{pane_id}' -n "$1" -t t: 'exec sleep 3600'
}
kv_id() { "$TMUX_REAL" -L "$KV" display-message -p -t "$1" '#{window_id}'; }
kv_alive() { "$TMUX_REAL" -L "$KV" list-panes -a -F '#{pane_id}' 2>/dev/null | grep -qx "$1" && echo alive || echo gone; }
kv_run() {                    # <action-line> [expect]: one trip through the wrapper; the note it left
    rm -f "$KD/once" "$KD/twice" "$KD/line2" "$KD/done" "$KD/link" "$KD/expect" "$KD/rv"; : > "$KD/bargv"; printf '%s\n' "$1" > "$KD/line"
    [ $# -lt 2 ] || printf '%s\n' "$2" > "$KD/expect"
    [ "${KV_LINK:-}" != 1 ] || : > "$KD/link"
    "$TMUX_REAL" -L "$KV" new-window -d -t t: \
        "${KV_SHIM:+export PATH=\"$KV_SHIM:\$PATH\"; }env COCKPIT_TUI_BIN=$KD/bin XDG_STATE_HOME=$KD/state COCKPIT_LOG=$KD/tui.log $KD/p/scripts/cockpit-board -p $KVHOME; echo x > $KD/done"
    until_file "$KD/done" 'x'
    # the second run's argv is `board --select %N --note <file>` and then the note
    sed -n '3,$p' "$KD/bargv"
}

V=$(kv_win vic-one); VW=$(kv_id "$V"); VI=$("$TMUX_REAL" -L "$KV" display-message -p -t "$V" '#{session_name}:#{window_index}')
KVNOTE="✓ cockpit: killed pane $V ($VI vic-one)"
is "kill %N with the shown pane and window ids kills the pane, and the whole ✓ note names pane, place and name" "$KVNOTE" "$(kv_run "kill $V" "$V $VW")"
is "and the pane is gone" "gone" "$(kv_alive "$V")"

# Two windows with one name, a pane moved from the first into the second between
# the confirm and the verb: the id the human confirmed no longer holds the pane,
# and the other window of that name must not die in its place.
A=$(kv_win claude); AW=$(kv_id "$A"); A2=$("$TMUX_REAL" -L "$KV" split-window -d -P -F '#{pane_id}' -t "$A" 'exec sleep 3600')
B=$(kv_win claude); BW=$(kv_id "$B")
"$TMUX_REAL" -L "$KV" join-pane -d -s "$A" -t "$B"
case "$(kv_run "kill $A" "$A $AW")" in "✕ cockpit: not killed: $A is now in window $BW, you confirmed $AW") ok "a pane moved into a window of the same name is refused, naming both ids" ;;
    *) bad "a pane moved into a window of the same name is refused, naming both ids" "✕ ... $A is now in window $BW, you confirmed $AW" "$(sed -n '3,$p' "$KD/bargv")" ;; esac
is "and nothing died: the first window's other pane, the moved pane, the other window" "alive alive alive" \
   "$(kv_alive "$A2") $(kv_alive "$A") $(kv_alive "$B")"

# The same move, but in the gap between the wrapper's check and its kill: a tmux
# shim moves the pane just before the if-shell that kills. The check and the kill
# are one call, so the format goes false and the other window survives.
C=$(kv_win claude); CW=$(kv_id "$C"); C2=$("$TMUX_REAL" -L "$KV" split-window -d -P -F '#{pane_id}' -t "$C" 'exec sleep 3600')
D=$(kv_win claude)
mkdir -p "$KD/shim"
printf '#!/bin/sh\nif [ "$1" = if-shell ] && [ -e %s/race ]; then rm -f %s/race; %s join-pane -d -s %s -t %s; fi\nexec %s "$@"\n' "$KD" "$KD" "$TMUX_REAL" "$C" "$D" "$TMUX_REAL" > "$KD/shim/tmux"
chmod +x "$KD/shim/tmux"; : > "$KD/race"; KV_SHIM=$KD/shim
kv_run "kill $C" "$C $CW" >/dev/null
KV_SHIM=
is "a pane that moves between the check and the kill takes nothing with it" "alive alive alive" "$(kv_alive "$C") $(kv_alive "$C2") $(kv_alive "$D")"
is "and the board is told it was not killed" "✕" "$(sed -n '3,$p' "$KD/bargv" | cut -c1-1)"

V=$(kv_win vic-two)
case "$(kv_run "kill $V" "$V $BW")" in "✕ cockpit: not killed: $V is now in window "*", you confirmed $BW") ok ".expect naming another window's id refuses" ;;
    *) bad ".expect naming another window's id refuses" "✕ ... you confirmed $BW" "$(sed -n '3,$p' "$KD/bargv")" ;; esac
is "and that window survives" "alive" "$(kv_alive "$V")"

case "$(kv_run "kill $V")" in '✕ cockpit: not killed: the board named no window'*) ok "kill %N with no .expect refuses" ;;
    *) bad "kill %N with no .expect refuses" "✕ ... named no window" "$(sed -n '3,$p' "$KD/bargv")" ;; esac
is "and the window survives" "alive" "$(kv_alive "$V")"

case "$(kv_run "kill $V" "")" in '✕ cockpit: not killed: the board named no window'*) ok "an empty .expect refuses" ;;
    *) bad "an empty .expect refuses" "✕ ... named no window" "$(sed -n '3,$p' "$KD/bargv")" ;; esac
case "$(kv_run "kill $V" "vic-two")" in '✕ cockpit: not killed: the board named no window'*) ok "a .expect that is a window name, not an id, refuses" ;;
    *) bad "a .expect that is a window name, not an id, refuses" "✕ ... named no window" "$(sed -n '3,$p' "$KD/bargv")" ;; esac
is "and the window survives both" "alive" "$(kv_alive "$V")"

KV_LINK=1; case "$(kv_run "kill $V" "$V $(kv_id "$V")")" in '✕ cockpit: not killed: the board named no window'*) ok "a .expect that is a symlink is not read" ;;
    *) bad "a .expect that is a symlink is not read" "✕ ... named no window" "$(sed -n '3,$p' "$KD/bargv")" ;; esac
KV_LINK=
is "and that window survives too" "alive" "$(kv_alive "$V")"

# The wrapper clears .expect before every board run: trip 1 leaves V2's id behind
# (a refused kill of V), trip 2 asks to kill V2 with no .expect of its own.
V2=$(kv_win vic-three); V2W=$(kv_id "$V2")
rm -f "$KD/once" "$KD/twice" "$KD/done" "$KD/link" "$KD/rv"; : > "$KD/bargv"
printf 'kill %s\n' "$V" > "$KD/line"; printf '%s %s\n' "$V2" "$V2W" > "$KD/expect"; printf 'kill %s\n' "$V2" > "$KD/line2"
"$TMUX_REAL" -L "$KV" new-window -d -t t: \
    "env COCKPIT_TUI_BIN=$KD/bin XDG_STATE_HOME=$KD/state COCKPIT_LOG=$KD/tui.log $KD/p/scripts/cockpit-board -p $KVHOME 2>$KD/err; echo x > $KD/done"
until_file "$KD/done" 'x'
case "$(tail -n 1 "$KD/bargv")" in '✕ cockpit: not killed: the board named no window'*) ok "a .expect left by the last trip does not stand in for the next action's" ;;
    *) bad "a .expect left by the last trip does not stand in for the next action's" "✕ ... named no window" "$(cat "$KD/bargv")" ;; esac
is "and the window it named survives" "alive" "$(kv_alive "$V2")"

case "$(kv_run "kill %99999" "%99999 @1")" in '✕ cockpit: %99999 is gone, nothing killed') ok "kill of a pane that is gone says so and kills nothing" ;;
    *) bad "kill of a pane that is gone says so and kills nothing" "✕ cockpit: %99999 is gone, nothing killed" "$(sed -n '3,$p' "$KD/bargv")" ;; esac

# #301: a window holding a second pane: only the agent's pane goes, as the
# confirm said, and the window's other pane survives
V=$(kv_win vic-four); V3=$("$TMUX_REAL" -L "$KV" split-window -d -P -F '#{pane_id}' -t "$V" 'exec sleep 3600')
kv_run "kill $V" "$V $(kv_id "$V")" >/dev/null
is "kill %N takes only its pane; the window's other pane survives" "gone alive" "$(kv_alive "$V") $(kv_alive "$V3")"

# #301: an older binary's .expect is a bare @W, and its confirm promised the
# whole window: the pane-only wrapper refuses rather than do less than asked
V=$(kv_win vic-five)
case "$(kv_run "kill $V" "$(kv_id "$V")")" in '✕ cockpit: not killed: the board named no window'*) ok "a bare window id (an older binary's .expect) refuses" ;;
    *) bad "a bare window id (an older binary's .expect) refuses" "✕ ... named no window" "$(sed -n '3,$p' "$KD/bargv")" ;; esac
is "and that pane survives" "alive" "$(kv_alive "$V")"
# and .expect naming another pane in the same window refuses: the ids pair up
V4=$("$TMUX_REAL" -L "$KV" split-window -d -P -F '#{pane_id}' -t "$V" 'exec sleep 3600')
case "$(kv_run "kill $V" "$V4 $(kv_id "$V")")" in '✕ cockpit: not killed: the board named no window'*) ok ".expect naming another pane of the window refuses" ;;
    *) bad ".expect naming another pane of the window refuses" "✕ ... named no window" "$(sed -n '3,$p' "$KD/bargv")" ;; esac
is "and both panes survive" "alive alive" "$(kv_alive "$V") $(kv_alive "$V4")"
case "$(sed -n 2p "$KD/bargv")" in "board --select $V --note "*) ok "the board returns after a kill, selecting the pane that was" ;;
    *) bad "the board returns after a kill, selecting the pane that was" "board --select $V --note <file>" "$(sed -n 2p "$KD/bargv")" ;; esac

# #288: request is no verb of the wrapper's any more. An older binary's
# request %N hits the unknown-verb arm: tmux-review is not run, the board ends.
: > "$KD/rv"
kv_run "request $V" >/dev/null
is "request %N from an older binary runs nothing" "" "$(cat "$KD/rv")"
is "and the board does not come back after it" "board;" "$(cut -d' ' -f1 "$KD/bargv" | tr '\n' ';')"
grep -q 'unknown verb \[request ' "$KD/tui.log" && ok "and the log names the verb it did not know" \
    || bad "and the log names the verb it did not know" "unknown verb [request ...]" "$(tail -n 3 "$KD/tui.log")"
"$TMUX_REAL" -L "$KV" kill-server >/dev/null 2>&1
fi

echo "== cockpit-board: resume %N runs claude --resume <id> in a new pane at the agent's tree (#301) =="
# The wrapper half of `o`, driven by a stub binary that writes the action file
# and .expect, as the kill block does. A stub `claude` on the wrapper's PATH
# (tmux gives a new pane the environment of the client that asked for it)
# records its cwd and argv; RSPWN is the file a format job would create.
if need_tmux "the board's resume verb"; then :; else
RS="cockpit-rs-$$"; SOCKS="$SOCKS $RS"; RSD=$(mktmpd)
mkdir -p "$RSD/stub" "$RSD/state" "$RSD/work" "$RSD/wt" "$RSD/d#(touch \$RSPWN)"
printf '#!/bin/sh\nprintf "[%%s]" "$PWD" "$@" > %s/ran\nexec sleep 3600\n' "$RSD" > "$RSD/stub/claude"
chmod +x "$RSD/stub/claude"
cat > "$RSD/bin" <<RSBIN
#!/bin/sh
printf '%s\n' "\$*" >> "$RSD/bargv"
[ "\${5:-}" = "" ] || cat "\$5" >> "$RSD/bargv"
[ ! -e "$RSD/once" ] || exit 0
: > "$RSD/once"
cp "$RSD/expect" "\$COCKPIT_ACTION.expect"
cat "$RSD/line" > "\$COCKPIT_ACTION"
RSBIN
chmod +x "$RSD/bin"
SID=0b5e7c1a-3f2d-4e8b-9a6c-1d2e3f4a5b6c
RSPWN="$RSD/pwned" "$TMUX_REAL" -L "$RS" -f /dev/null new-session -d -s t -x 120 -y 40 'exec sleep 3600' >/dev/null 2>&1
RSHOME=$("$TMUX_REAL" -L "$RS" list-panes -t t -F '#{pane_id}')
rs_agent() {                  # <session-id> -> an ended agent's pane, in a window of its own, cwd work
    local p
    p=$("$TMUX_REAL" -L "$RS" new-window -d -P -F '#{pane_id}' -c "$RSD/work" -t t: 'exec sleep 3600')
    "$TMUX_REAL" -L "$RS" set -p -t "$p" @panestate needs-input
    "$TMUX_REAL" -L "$RS" set -p -t "$p" @cockpit_session "$1"
    printf '%s' "$p"
}
rs_panes() { "$TMUX_REAL" -L "$RS" display -p -t "$1" '#{window_panes}'; }
rs_run() {                    # <action-line> <expect>: one trip; the note it left
    rm -f "$RSD/once" "$RSD/done" "$RSD/ran"; : > "$RSD/bargv"
    printf '%s\n' "$1" > "$RSD/line"; printf '%s\n' "$2" > "$RSD/expect"
    "$TMUX_REAL" -L "$RS" new-window -d -t t: \
        "env PATH=\"$RSD/stub:\$PATH\" COCKPIT_TUI_BIN=$RSD/bin XDG_STATE_HOME=$RSD/state COCKPIT_LOG=$RSD/tui.log $DIR/scripts/cockpit-board -p $RSHOME; echo x > $RSD/done"
    until_file "$RSD/done" 'x'
    sed -n '3,$p' "$RSD/bargv"
}

R=$(rs_agent "$SID")
rs_run "resume $R" "$SID" >/dev/null
is "resume %N with the session it holds closes the board: one trip, no return" "1" "$(grep -c '^board' "$RSD/bargv")"
until_file "$RSD/ran" '?*'
is "claude --resume <id> runs at the pane's cwd" "[$RSD/work][--resume][$SID]" "$(cat "$RSD/ran" 2>/dev/null)"
is "in a new pane beside it, the agent's pane kept" "2 $R" \
   "$(rs_panes "$R") $("$TMUX_REAL" -L "$RS" list-panes -t "$R" -F '#{pane_id}' | grep -x -- "$R")"

# Claude Code never moves its cwd (#312): the session lives under the pane's
# cwd, so @cockpit_dir, a worktree the agent works in, does not move the resume
R=$(rs_agent "$SID"); "$TMUX_REAL" -L "$RS" set -p -t "$R" @cockpit_dir "$RSD/wt"
rs_run "resume $R" "$SID" >/dev/null; until_file "$RSD/ran" '?*'
is "@cockpit_dir does not move it: it runs at the pane's cwd" "[$RSD/work][--resume][$SID]" "$(cat "$RSD/ran" 2>/dev/null)"

# A cwd named with a tmux format: -c expands formats, so an unescaped name
# would run the #() job and start in the directory its output makes instead.
# The test's own -c spells it ##( for the same reason.
R=$("$TMUX_REAL" -L "$RS" new-window -d -P -F '#{pane_id}' -c "$RSD/d##(touch \$RSPWN)" -t t: 'exec sleep 3600')
"$TMUX_REAL" -L "$RS" set -p -t "$R" @panestate needs-input
"$TMUX_REAL" -L "$RS" set -p -t "$R" @cockpit_session "$SID"
rs_run "resume $R" "$SID" >/dev/null; until_file "$RSD/ran" '?*'
is "a cwd named with #(...) is the directory, not a format" "[$RSD/d#(touch \$RSPWN)][--resume][$SID]" "$(cat "$RSD/ran" 2>/dev/null)"

# A session id that is no UUID never reaches a command line, whatever the pane
# holds: the wrapper checks the shape itself, not only the board.
for bad in "x'; touch \$RSPWN; '" '$(touch $RSPWN)' "$(printf %s "$SID" | tr a-f A-F)" "$SID; touch \$RSPWN" ""; do
    R=$(rs_agent "$bad")
    case "$(rs_run "resume $R" "$bad")" in '✕ cockpit: not resumed: the board named no session id') ok "a hostile session id is refused: [$bad]" ;;
        *) bad "a hostile session id is refused: [$bad]" "✕ ... named no session id" "$(cat "$RSD/bargv")" ;; esac
    is "and nothing ran for it: no pane, no claude" "1 " "$(rs_panes "$R") $(cat "$RSD/ran" 2>/dev/null)"
done

R=$(rs_agent "$SID")
case "$(rs_run "resume $R" "${SID%c}d")" in "✕ cockpit: not resumed: $R no longer holds session ${SID%c}d") ok "a session the pane no longer holds is refused" ;;
    *) bad "a session the pane no longer holds is refused" "✕ ... no longer holds" "$(cat "$RSD/bargv")" ;; esac
is "and nothing ran" "1 " "$(rs_panes "$R") $(cat "$RSD/ran" 2>/dev/null)"
case "$(rs_run "resume %99999" "$SID")" in '✕ cockpit: %99999 is gone, nothing resumed') ok "resume of a pane that is gone says so" ;;
    *) bad "resume of a pane that is gone says so" "✕ cockpit: %99999 is gone, nothing resumed" "$(cat "$RSD/bargv")" ;; esac
# A second copy of a live session is refused: a pane whose command is an
# agent (by @cockpit_agents_glob, or a claude renamed to its version) holding
# the id. The ended panes above held it too and never blocked a resume.
"$TMUX_REAL" -L "$RS" set -g @cockpit_agents_glob '|claude|'
for argv0 in claude 2.1.246; do
    RL=$("$TMUX_REAL" -L "$RS" new-window -d -P -F '#{pane_id}' -t t: "exec -a $argv0 sleep 3600")
    "$TMUX_REAL" -L "$RS" set -p -t "$RL" @panestate running
    "$TMUX_REAL" -L "$RS" set -p -t "$RL" @cockpit_session "$SID"
    R=$(rs_agent "$SID")
    case "$(rs_run "resume $R" "$SID")" in "✕ cockpit: ${SID:0:8} is already running in $RL") ok "a session live in another pane ($argv0) is not resumed twice" ;;
        *) bad "a session live in another pane ($argv0) is not resumed twice" "✕ cockpit: ${SID:0:8} is already running in $RL" "$(cat "$RSD/bargv")" ;; esac
    is "and nothing ran for it" "1 " "$(rs_panes "$R") $(cat "$RSD/ran" 2>/dev/null)"
    "$TMUX_REAL" -L "$RS" kill-pane -t "$RL"
done
# the target itself running an agent again (a stale board's row) is refused too
R=$("$TMUX_REAL" -L "$RS" new-window -d -P -F '#{pane_id}' -c "$RSD/work" -t t: 'exec -a claude sleep 3600')
"$TMUX_REAL" -L "$RS" set -p -t "$R" @panestate running
"$TMUX_REAL" -L "$RS" set -p -t "$R" @cockpit_session "$SID"
case "$(rs_run "resume $R" "$SID")" in "✕ cockpit: ${SID:0:8} is already running in $R") ok "a target running its agent again is not resumed" ;;
    *) bad "a target running its agent again is not resumed" "✕ cockpit: ${SID:0:8} is already running in $R" "$(cat "$RSD/bargv")" ;; esac
"$TMUX_REAL" -L "$RS" kill-pane -t "$R"
# an agent holding the id with no @panestate still blocks: SessionStart and idle
# run `agent-state off`, so the copy `o` just opened has none
RL=$("$TMUX_REAL" -L "$RS" new-window -d -P -F '#{pane_id}' -t t: 'exec -a claude sleep 3600')
"$TMUX_REAL" -L "$RS" set -p -t "$RL" @cockpit_session "$SID"
R=$(rs_agent "$SID")
case "$(rs_run "resume $R" "$SID")" in "✕ cockpit: ${SID:0:8} is already running in $RL") ok "a live agent holding the id with no @panestate is not resumed twice" ;;
    *) bad "a live agent holding the id with no @panestate is not resumed twice" "✕ cockpit: ${SID:0:8} is already running in $RL" "$(cat "$RSD/bargv")" ;; esac
"$TMUX_REAL" -L "$RS" kill-pane -t "$RL"
"$TMUX_REAL" -L "$RS" kill-pane -t "$R" 2>/dev/null
# the same id live under a command that is no agent (a shell left over) does not block
RL=$("$TMUX_REAL" -L "$RS" new-window -d -P -F '#{pane_id}' -t t: 'exec sleep 3600')
"$TMUX_REAL" -L "$RS" set -p -t "$RL" @panestate running
"$TMUX_REAL" -L "$RS" set -p -t "$RL" @cockpit_session "$SID"
R=$(rs_agent "$SID")
rs_run "resume $R" "$SID" >/dev/null; until_file "$RSD/ran" '?*'
is "an ended pane holding the id does not block the resume" "[$RSD/work][--resume][$SID]" "$(cat "$RSD/ran" 2>/dev/null)"
is "no format job ran in the whole block" "absent" "$([ -e "$RSD/pwned" ] && echo present || echo absent)"
"$TMUX_REAL" -L "$RS" kill-server >/dev/null 2>&1
fi

echo "== cockpit-board: an ended agent shows dimmed as ended, offers o, and o writes resume %N (#301) =="
# The real binary on a server that knows its agents (@cockpit_agents_glob, as
# cockpit.tmux sets it): one live claude, one ended pane with a session id,
# one ended pane whose session is no UUID.
if need_tmux "the board's ended rows"; then :; else
EB="cockpit-eb-$$"; SOCKS="$SOCKS $EB"; EBD=$(mktmpd)
mkdir -p "$EBD/state"
# 180 columns: the list beside the right pane (#284) is 64 wide, room for each
# row whole, and the footer has room for o resume
"$TMUX_REAL" -L "$EB" -f /dev/null new-session -d -s t -x 180 -y 30 'exec sleep 3600' >/dev/null 2>&1
"$TMUX_REAL" -L "$EB" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
EBL=$("$TMUX_REAL" -L "$EB" new-window -d -n live -c "$EBD" -P -F '#{pane_id}' -t t: 'exec -a claude sleep 3600')
EBE=$("$TMUX_REAL" -L "$EB" new-window -d -n gone -c "$EBD" -P -F '#{pane_id}' -t t: 'exec sleep 3600')
EBX=$("$TMUX_REAL" -L "$EB" new-window -d -n nosid -c "$EBD" -P -F '#{pane_id}' -t t: 'exec sleep 3600')
"$TMUX_REAL" -L "$EB" set -p -t "$EBL" @panestate running
"$TMUX_REAL" -L "$EB" set -p -t "$EBE" @panestate needs-input
"$TMUX_REAL" -L "$EB" set -p -t "$EBE" @panestate_msg 'Do you want to proceed?'
"$TMUX_REAL" -L "$EB" set -p -t "$EBE" @cockpit_session 0b5e7c1a-3f2d-4e8b-9a6c-1d2e3f4a5b6c
"$TMUX_REAL" -L "$EB" set -p -t "$EBX" @panestate running
"$TMUX_REAL" -L "$EB" set -p -t "$EBX" @cockpit_session 'not-a-uuid'
# A live claude that is not the foreground (an editor over it, say): `claude`
# in the pane's process tree under a foreground `sleep`. Live, so it keeps ▲.
ln -s "$(command -v sleep)" "$EBD/claude"
EBT=$("$TMUX_REAL" -L "$EB" new-window -d -n editing -c "$EBD" -P -F '#{pane_id}' -t t: "sh -c '$EBD/claude 3600 & exec sleep 3600'")
"$TMUX_REAL" -L "$EB" set -p -t "$EBT" @panestate needs-input
"$TMUX_REAL" -L "$EB" set -p -t "$EBT" @panestate_msg 'Allow this edit?'
until_out sleep "$TMUX_REAL" -L "$EB" display -p -t "$EBT" '#{pane_current_command}' \
    || bad "the tree pane's foreground is sleep" "sleep" "$("$TMUX_REAL" -L "$EB" display -p -t "$EBT" '#{pane_current_command}')"
eb_open() {                   # <pane> <action-dir> -> EBW, a board on that pane
    EBW=$("$TMUX_REAL" -L "$EB" new-window -d -P -F '#{pane_id}' -t t: \
        "env COCKPIT_READONLY=1 XDG_STATE_HOME=$EBD/state COCKPIT_ACTION=$2/act $COCKPIT_TUI_BIN board --select $1")
    until_cap "$EB" "$EBW" '*board · 4 agents*'
}
eb_cap() { "$TMUX_REAL" -L "$EB" capture-pane -p -J -t "$EBW" 2>/dev/null; }
eb_row() { eb_cap | grep -- " $1 "; }  # <window-name> -> its list row
EA=$(mktmpd)
eb_open "$EBE" "$EA" || bad "the board opens on the ended agent" "board · 4 agents" "$(eb_cap)"
until_cap "$EB" "$EBW" '*o resume*' || bad "the ended row's footer offers o resume" "o resume" "$(eb_cap)"
case "$(eb_row gone)" in *◌*' ended '*) ok "the ended row says ended under its own glyph" ;; *) bad "the ended row says ended under its own glyph" "◌ ... ended" "$(eb_cap)" ;; esac
case "$(eb_cap)" in *'Do you want'*) bad "an ended needs-input pane shows no message" "no message" "$(eb_cap)" ;;
    *) ok "an ended needs-input pane shows no message" ;; esac
case "$(eb_row gone)" in *▲*) bad "an ended needs-input pane is no ▲" "no ▲" "$(eb_cap)" ;; *) ok "an ended needs-input pane is no ▲" ;; esac
case "$(eb_row editing)" in *▲*'Allow this edit?'*) ok "a claude in the tree, not the foreground, is live and keeps its ▲" ;;
    *) bad "a claude in the tree, not the foreground, is live and keeps its ▲" "▲ ... Allow this edit?" "$(eb_cap)" ;; esac
case "$(eb_cap)" in *' 1 needs you'*) ok "and only it is counted in needs you" ;; *) bad "and only it is counted in needs you" "1 needs you" "$(eb_cap)" ;; esac
# the panes sit outside any repo: a branch name could hold the word
case "$(eb_row live)" in *' ended '*) bad "a live claude is not ended" "running" "$(eb_cap)" ;; *) ok "a live claude is not ended" ;; esac
"$TMUX_REAL" -L "$EB" send-keys -t "$EBW" o
until_file "$EA/act" '?*'
is "o writes resume %N and the session id as .expect" "resume $EBE 0b5e7c1a-3f2d-4e8b-9a6c-1d2e3f4a5b6c" \
   "$(cat "$EA/act" 2>/dev/null) $(cat "$EA/act.expect" 2>/dev/null)"
for p in "$EBL" "$EBX"; do
    EO=$(mktmpd)
    eb_open "$p" "$EO"
    until_cap "$EB" "$EBW" '*x kill*'
    case "$(eb_cap)" in *'o resume'*) bad "no o resume on $p" "no o" "$(eb_cap)" ;; *) ok "no o resume in the footer on $p" ;; esac
    "$TMUX_REAL" -L "$EB" send-keys -t "$EBW" o
    until_cap "$EB" "$EBW" '*resume is for an ended agent*' || bad "o there says why" "resume is for an ended agent" "$(eb_cap)"
    "$TMUX_REAL" -L "$EB" send-keys -t "$EBW" q
    until_out '' sh -c "'$TMUX_REAL' -L '$EB' list-panes -a -F '#{pane_id}' | grep -x -- '$EBW'" \
        || bad "q closes the board" "the window closes" "$(eb_cap)"
    is "and q leaves no action from it" "" "$(cat "$EO/act" 2>/dev/null)$(cat "$EO/act.expect" 2>/dev/null)"
done
"$TMUX_REAL" -L "$EB" kill-server >/dev/null 2>&1
fi

echo "== cockpit-board: @cockpit_glyphs reaches the binary as COCKPIT_GLYPHS (#293) =="
# theme.rs reads only the env, so the wrapper must pass the option on. A stub
# binary records what it got and exits with no action.
if need_tmux "the board's glyphs"; then :; else
GL="cockpit-gl-$$"; SOCKS="$SOCKS $GL"; GD=$(mktmpd)
printf '#!/bin/sh\nprintf "[%%s]" "${COCKPIT_GLYPHS:-}" > %s/g\n' "$GD" > "$GD/bin"; chmod +x "$GD/bin"
"$TMUX_REAL" -L "$GL" -f /dev/null new-session -d -s t -x 100 -y 30 'exec sleep 3600' >/dev/null 2>&1
GLP=$("$TMUX_REAL" -L "$GL" list-panes -t t -F '#{pane_id}')
gl_run() {
    rm -f "$GD/g"
    "$TMUX_REAL" -L "$GL" new-window -d -t t: \
        "env -u COCKPIT_GLYPHS COCKPIT_TUI_BIN=$GD/bin XDG_STATE_HOME=$GD COCKPIT_LOG=$GD/tui.log $DIR/scripts/cockpit-board -p $GLP"
    until_file "$GD/g" '\[*\]'
    cat "$GD/g" 2>/dev/null
}
"$TMUX_REAL" -L "$GL" set -g @cockpit_glyphs ascii
is "@cockpit_glyphs ascii reaches the board as COCKPIT_GLYPHS" "[ascii]" "$(gl_run)"
"$TMUX_REAL" -L "$GL" set -gu @cockpit_glyphs
is "and unset passes nothing" "[]" "$(gl_run)"
# an odd value stays one word: it reaches the binary whole and runs nothing
"$TMUX_REAL" -L "$GL" set -g @cockpit_glyphs "ascii touch $GD/ran"
is "a value with spaces reaches the binary as one word" "[ascii touch $GD/ran]" "$(gl_run)"
[ -e "$GD/ran" ] && bad "and runs nothing" "no $GD/ran" "it exists" || ok "and runs nothing"
"$TMUX_REAL" -L "$GL" kill-server >/dev/null 2>&1
fi

echo "== cockpit-panel: @cockpit_glyphs reaches the binary as COCKPIT_GLYPHS (#293) =="
# The panel's env line carries the glyphs word beside COCKPIT_PANEL_DROPPED
# (#272); the stub records both, so neither can fall out of the merged line.
if need_tmux "the panel's glyphs"; then :; else
GP="cockpit-gp-$$"; SOCKS="$SOCKS $GP"; GPD=$(mktmpd)
printf '#!/bin/sh\nprintf "[%%s] D=[%%s]" "${COCKPIT_GLYPHS:-}" "${COCKPIT_PANEL_DROPPED:-}" > %s/g\n' "$GPD" > "$GPD/bin"; chmod +x "$GPD/bin"
"$TMUX_REAL" -L "$GP" -f /dev/null new-session -d -s t -x 100 -y 30 'exec sleep 3600' >/dev/null 2>&1
GPP=$("$TMUX_REAL" -L "$GP" list-panes -t t -F '#{pane_id}')
GPS=$("$TMUX_REAL" -L "$GP" display -p '#{pid}')
gp_run() {
    rm -f "$GPD/g"; mkdir -p "$GPD/tmux-cockpit/panel"
    : > "$GPD/tmux-cockpit/panel/$GPS-9999.state"     # a hide left by a pane that is gone
    "$TMUX_REAL" -L "$GP" new-window -d -t t: \
        "env -u COCKPIT_GLYPHS COCKPIT_TUI_BIN=$GPD/bin XDG_STATE_HOME=$GPD COCKPIT_LOG=$GPD/tui.log $DIR/scripts/cockpit-panel -p $GPP"
    until_file "$GPD/g" '\[*\]'
    cat "$GPD/g" 2>/dev/null
}
"$TMUX_REAL" -L "$GP" set -g @cockpit_glyphs ascii
is "@cockpit_glyphs ascii reaches the panel as COCKPIT_GLYPHS, beside the dropped hides" "[ascii] D=[%9999]" "$(gp_run)"
"$TMUX_REAL" -L "$GP" set -gu @cockpit_glyphs
is "and unset passes nothing" "[] D=[%9999]" "$(gp_run)"
"$TMUX_REAL" -L "$GP" set -g @cockpit_glyphs "ascii touch $GPD/ran"
is "a value with spaces reaches the panel as one word" "[ascii touch $GPD/ran] D=[%9999]" "$(gp_run)"
[ -e "$GPD/ran" ] && bad "and runs nothing" "no $GPD/ran" "it exists" || ok "and runs nothing"
"$TMUX_REAL" -L "$GP" kill-server >/dev/null 2>&1
fi

# --- sessions view (#245): cockpit-tui sessions and the tm that runs it -------
# A private server of sessions (SVS) and a second one (SVO) whose panes are the
# ptys the picker and tm run in, so nothing here touches the suite's own server.
sv_server() {                 # sv_server <tag> -- alpha, beta (waiting), gamma; ids in SVA SVB SVG
    SVS="cockpit-sv$1-$$"; SVO="cockpit-svo$1-$$"; SOCKS="$SOCKS $SVS $SVO"
    "$TMUX_REAL" -L "$SVS" -f /dev/null new-session -d -s alpha -x 100 -y 30 'exec -a claude sleep 300' >/dev/null 2>&1
    "$TMUX_REAL" -L "$SVS" set -g @cockpit_agents_glob '|claude|'
    "$TMUX_REAL" -L "$SVS" new-session -d -s beta 'exec -a claude sleep 300' >/dev/null 2>&1
    "$TMUX_REAL" -L "$SVS" new-session -d -s gamma 'exec sleep 300' >/dev/null 2>&1
    "$TMUX_REAL" -L "$SVS" set -p -t beta: @panestate needs-input
    "$TMUX_REAL" -L "$SVS" set -p -t beta: @panestate_msg 'Deploy to prod?'
    SVA=$("$TMUX_REAL" -L "$SVS" display-message -p -t alpha: '#{session_id}')
    SVB=$("$TMUX_REAL" -L "$SVS" display-message -p -t beta: '#{session_id}')
    SVG=$("$TMUX_REAL" -L "$SVS" display-message -p -t gamma: '#{session_id}')
}
sv_pane() {                   # sv_pane <dir> <cmd...> -- <cmd> in a pty of SVO; its rc lands in <dir>/rc, stderr in <dir>/err
    local d=$1; shift
    "$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1
    { printf '#!/bin/sh\n'; printf '%q ' "$@"; printf '2> "%s/err"\necho $? > "%s/rc"\nexec sleep 30\n' "$d" "$d"; } > "$d/run"
    chmod +x "$d/run"; : > "$d/err"; rm -f "$d/rc"
    "$TMUX_REAL" -L "$SVO" -f /dev/null new-session -d -s t -x "${SVCOLS:-100}" -y "${SVROWS:-24}" "TERM=xterm-256color $d/run" >/dev/null 2>&1
}
sv_ui() {                     # sv_ui <dir> [tui args...] -- cockpit-tui sessions, the action files under <dir>/act
    local d=$1; shift
    rm -f "$d"/act "$d"/act.*
    sv_pane "$d" env COCKPIT_TMUX="$TMUX_REAL -L $SVS" COCKPIT_ACTION="$d/act" "$DIR/bin/cockpit-tui" sessions "$@"
}
sv_key() { "$TMUX_REAL" -L "$SVO" send-keys -t t "$@"; }
sv_cap() { until_cap "$SVO" t "$1"; }
sv_tm() {                     # sv_tm <dir> [VAR=val...] -- [tm args] -- tm as a login shell would run it, tmux aimed at SVS
    local d=$1; shift
    mkdir -p "$d/bin" "$d/work"
    printf '#!/bin/sh\n[ -z "${SV_ARGS:-}" ] || printf "%%s\\n" "$*" >> "$SV_ARGS"\nexec "%s" -L "%s" "$@"\n' "$TMUX_REAL" "$SVS" > "$d/bin/tmux"; chmod +x "$d/bin/tmux"
    local -a ev=()
    while [ $# -gt 0 ] && [ "$1" != -- ]; do ev+=("$1"); shift; done
    shift
    sv_pane "$d" sh -c 'cd "$1" && shift && exec "$@"' sh "$d/work" \
        env -u TMUX -u TMUX_PANE -u COCKPIT_ACTION PATH="$d/bin:$PATH" "${ev[@]}" "$DIR/scripts/tm" "$@"
}
sv_stub() {                   # sv_stub <dir> -- a picker that prints $SV_LINE, then races $SV_RENAME in before tm acts
    cat > "$1/picker" <<'STUB'
#!/bin/sh
[ -z "${SV_ONCE:-}" ] || { [ ! -e "$SV_ONCE" ] || exit 1; : > "$SV_ONCE"; }
[ -z "${SV_EXPECT:-}" ] || printf '%s\n' "$SV_EXPECT" > "$COCKPIT_ACTION.expect"
[ -z "${SV_TEXT:-}" ] || printf '%s\n' "$SV_TEXT" > "$COCKPIT_ACTION.text"
[ -z "${SV_GLYPHS_OUT:-}" ] || printf '%s' "${COCKPIT_GLYPHS:-}" > "$SV_GLYPHS_OUT"
printf '%s\n' "$SV_LINE" > "$COCKPIT_ACTION"
[ -z "${SV_RENAME:-}" ] || "$SV_TMUX" -L "$SV_SOCK" rename-session -t "$SV_TARGET" "$SV_RENAME"
STUB
    chmod +x "$1/picker"
}
sv_names() { "$TMUX_REAL" -L "$SVS" list-sessions -F '#{session_name}' 2>/dev/null | sort | tr '\n' ' '; }

echo "== sessions view: cockpit-tui sessions through a pty (#245) =="
# Rows come from tmux, keys from /dev/tty, and the answer goes to the action
# file: a picker that drew on stdout or read stdin would fail one of these.
if [ ! -x "$DIR/bin/cockpit-tui" ]; then
    skip_ "cockpit-tui sessions (no bin/cockpit-tui; run make tui)"
elif need_tmux "cockpit-tui sessions"; then :; else
sv_server a; SVD=$(mktmpd)
sv_ui "$SVD" --current "$SVA"
sv_cap '*beta*gamma*alpha*(here)*' && ok "the rows are drawn: waiting first, your own session last and marked" \
    || bad "the rows are drawn: waiting first, your own session last and marked" "beta, gamma, alpha (here)" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -6)"
case "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t)" in
    *"Deploy to prod?"*) ok "the preview shows the question a waiting window asks" ;;
    *) bad "the preview shows the question a waiting window asks" "Deploy to prod?" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -8)" ;;
esac
sv_key Enter; until_file "$SVD/rc" '?*'
is "enter writes switch and the id of the first row" "switch $SVB" "$(cat "$SVD/act" 2>/dev/null)"
is "enter exits 0" "0" "$(cat "$SVD/rc")"
is "a switch leaves no side files" "act" "$(ls "$SVD" | grep '^act' | tr '\n' ' ' | sed 's/ $//')"

sv_ui "$SVD" --current "$SVA"; sv_cap '*gamma*'; sv_key 2; until_file "$SVD/rc" '?*'
is "2 on an empty filter switches to the second row" "switch $SVG" "$(cat "$SVD/act" 2>/dev/null)"

sv_ui "$SVD" --current "$SVA"; sv_cap '*gamma*'; sv_key z e t a; sv_cap '*zeta*creates it*'; sv_key Enter; until_file "$SVD/rc" '?*'
is "enter with no match writes new-session -" "new-session -" "$(cat "$SVD/act" 2>/dev/null)"
is "and the typed name goes in .text" "zeta" "$(cat "$SVD/act.text" 2>/dev/null)"

sv_ui "$SVD" --current "$SVA"; sv_cap '*gamma*'; sv_key g a; sv_cap '*1/1 of 3*'; sv_key M-Enter; until_file "$SVD/rc" '?*'
is "alt-enter creates the typed name although gamma matches" "new-session - ga" "$(cat "$SVD/act" 2>/dev/null) $(cat "$SVD/act.text" 2>/dev/null)"

sv_ui "$SVD" --current "$SVA"; sv_cap '*gamma*'; sv_key C-x; sv_cap '*kill beta?*1 agent (1 waiting)*'
sleep 0.3
is "one ^X names what dies and writes nothing" "" "$(cat "$SVD/act" 2>/dev/null)"
sv_key C-x; until_file "$SVD/rc" '?*'
is "the second ^X writes kill-session and the id" "kill-session $SVB" "$(cat "$SVD/act" 2>/dev/null)"
is "and the shown name goes in .expect" "beta" "$(cat "$SVD/act.expect" 2>/dev/null)"
is "the picker killed nothing itself" "alpha beta gamma " "$(sv_names)"

sv_ui "$SVD" --current "$SVA"; sv_cap '*gamma*'; sv_key End C-x; sv_cap '*your own session*'
sv_key C-x; sleep 0.3
is "your own session refuses a kill, and a second ^X still writes nothing" "" "$(cat "$SVD/act" 2>/dev/null)"
sv_key Escape; sv_key Escape; until_file "$SVD/rc" '?*'
is "esc cancels with exit 1 and no action" "1 0" "$(cat "$SVD/rc") $(ls "$SVD" | grep -c '^act')"

sv_ui "$SVD" --current "$SVA"; sv_cap '*gamma*'; sv_key F2; sv_cap '*rename> beta*'
sv_key BSpace BSpace BSpace BSpace d e l t a; sv_key Enter; until_file "$SVD/rc" '?*'
is "F2 and a new name write rename-session, the id, the name in .text" "rename-session $SVB delta" "$(cat "$SVD/act" 2>/dev/null) $(cat "$SVD/act.text" 2>/dev/null)"
is "and the old name in .expect" "beta" "$(cat "$SVD/act.expect" 2>/dev/null)"

sv_ui "$SVD" --needs; sv_cap '*needs you*beta*'
case "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t)" in
    *gamma*|*alpha*) bad "--needs lists only the sessions waiting or failed" "beta alone" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -6)" ;;
    *) ok "--needs lists only the sessions waiting or failed" ;;
esac
sv_key Escape; until_file "$SVD/rc" '?*'

# No COCKPIT_ACTION: stdout is the only place to answer, and only switch fits.
sv_pane "$SVD" env COCKPIT_TMUX="$TMUX_REAL -L $SVS" "$DIR/bin/cockpit-tui" sessions --current "$SVA"
sv_cap '*gamma*'; sv_key Enter; until_file "$SVD/rc" '?*'
is "without COCKPIT_ACTION a switch is the one line on stdout" "0" "$(cat "$SVD/rc")"
"$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1

SVCOLS=60 SVROWS=14 sv_ui "$SVD" --current "$SVA"
sv_cap '*beta*'
case "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t)" in
    *"(here)"*alpha*|*alpha*"(here)"*) ok "at 60 columns the rows still show your own session" ;;
    *) bad "at 60 columns the rows still show your own session" "alpha (here)" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -8)" ;;
esac
sv_key Escape; until_file "$SVD/rc" '?*'
"$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$SVS" kill-server >/dev/null 2>&1
fi

echo "== sessions view: the picker says when every row it was given had no key (#245) =="
# The note #237's review asked for: an input of tab-led lines opens no screen,
# exits 1, and stderr names how many rows were skipped.
if [ ! -x "$DIR/bin/cockpit-tui" ]; then
    skip_ "pick keyless note (no bin/cockpit-tui; run make tui)"
else
KLD=$(mktmpd)
printf '\tone\n\ttwo\n\tthree\n' | "$DIR/bin/cockpit-tui" pick >"$KLD/out" 2>"$KLD/err"; klrc=$?
is "all rows keyless: exit 1 without a screen" "1" "$klrc"
is "and nothing on stdout" "0" "$(wc -c < "$KLD/out" | tr -d ' ')"
is "and stderr counts the rows it skipped" "cockpit-tui: pick: skipped 3 rows with no key" "$(cat "$KLD/err")"
printf '\tonly\n' | "$DIR/bin/cockpit-tui" pick >/dev/null 2>"$KLD/err"
is "one keyless row is singular" "cockpit-tui: pick: skipped 1 row with no key" "$(cat "$KLD/err")"
fi

echo "== tm sessions picker: switch, create, kill, rename and --needs from a login shell (#245) =="
# No $TMUX: the shape of a shell outside tmux. tm runs in a pty of its own, so
# the picker is the real cockpit-tui; attach-session lands as a client of SVS.
if [ ! -x "$DIR/bin/cockpit-tui" ]; then
    skip_ "tm sessions picker (no bin/cockpit-tui; run make tui)"
elif need_tmux "tm sessions picker"; then :; else
sv_server b; SVD=$(mktmpd)
sv_tm "$SVD" --; sv_cap '*session>*3 sessions*'
sv_key g a m; sv_cap '*1/1 of 3*'; sv_key Enter
until_out gamma "$TMUX_REAL" -L "$SVS" list-clients -F '#{session_name}'
is "enter on a row attaches the login shell to that session" "gamma" "$("$TMUX_REAL" -L "$SVS" list-clients -F '#{session_name}' 2>/dev/null)"
"$TMUX_REAL" -L "$SVS" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1

sv_server b2; SVD=$(mktmpd)
sv_tm "$SVD" --; sv_cap '*session>*3 sessions*'
sv_key f r e s h Space o n e; sv_cap '*fresh one*creates it*'; sv_key Enter
until_out '*fresh_one*' "$TMUX_REAL" -L "$SVS" list-clients -F '#{session_name}'
is "a name nothing matches creates it, cleaned, and attaches" "fresh_one" "$("$TMUX_REAL" -L "$SVS" list-clients -F '#{session_name}' 2>/dev/null)"
is "in the directory tm was run from" "$SVD/work" "$("$TMUX_REAL" -L "$SVS" display-message -p -t fresh_one: '#{session_path}' 2>/dev/null)"
"$TMUX_REAL" -L "$SVS" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1

sv_server b3; SVD=$(mktmpd)
sv_tm "$SVD" --; sv_cap '*session>*3 sessions*'
sv_key g a m m a; sv_cap '*1/1 of 3*'; sv_key C-x; sv_cap '*kill gamma?*'; sv_key C-x
until_out 'alpha beta ' sv_names
is "^X twice kills the session through tm" "alpha beta " "$(sv_names)"
sv_cap '*session>*2 sessions*' && ok "and the picker comes back for the next one" || bad "and the picker comes back for the next one" "2 sessions" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -3)"
sv_key Escape; until_file "$SVD/rc" '?*'
is "esc then ends tm with exit 0" "0" "$(cat "$SVD/rc")"
case "$(cat "$SVD/err")" in *"killed: gamma"*) ok "and tm says what it killed" ;; *) bad "and tm says what it killed" "killed: gamma" "$(cat "$SVD/err")" ;; esac
"$TMUX_REAL" -L "$SVS" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1

sv_server b4; SVD=$(mktmpd)
sv_tm "$SVD" --; sv_cap '*session>*3 sessions*'
sv_key g a m m a; sv_cap '*1/1 of 3*'; sv_key F2; sv_cap '*rename> gamma*'
sv_key BSpace BSpace BSpace BSpace BSpace d e l t a Enter
until_out 'alpha beta delta ' sv_names
is "F2 renames the session through tm" "alpha beta delta " "$(sv_names)"
sv_cap '*session>*3 sessions*'; sv_key Escape; until_file "$SVD/rc" '?*'
case "$(cat "$SVD/err")" in *"renamed: gamma -> delta"*) ok "and tm says what it renamed" ;; *) bad "and tm says what it renamed" "renamed: gamma -> delta" "$(cat "$SVD/err")" ;; esac
"$TMUX_REAL" -L "$SVS" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1

sv_server b5; SVD=$(mktmpd); mkdir "$SVD/tmp"
sv_tm "$SVD" TMPDIR="$SVD/tmp" -- --needs; sv_cap '*needs you*beta*'
case "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t)" in
    *gamma*|*alpha*) bad "tm --needs shows only the sessions waiting or failed" "beta alone" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -6)" ;;
    *) ok "tm --needs shows only the sessions waiting or failed" ;;
esac
sv_key Escape; until_file "$SVD/rc" '?*'
is "esc ends tm with exit 0, nothing on stderr and no temp dir left" "0||" "$(cat "$SVD/rc")|$(cat "$SVD/err")|$(ls -A "$SVD/tmp")"
"$TMUX_REAL" -L "$SVS" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1
fi

echo "== tm sessions picker: tm re-checks the id and the name before it acts (#245) =="
# A stand-in picker writes the action and, when asked, renames the session
# between the print and tm's act: the window a person's ^X ^X can lose.
if need_tmux "tm re-check"; then :; else
sv_server c; SVD=$(mktmpd); sv_stub "$SVD"
svrun() {                     # svrun <env words...> -- run tm against the stub; rc in SVRC, stderr in SVERR
    sv_tm "$SVD" COCKPIT_TUI_BIN="$SVD/picker" SV_ONCE="$SVD/once" SV_TMUX="$TMUX_REAL" SV_SOCK="$SVS" "$@" --
    until_file "$SVD/rc" '?*'; SVRC=$(cat "$SVD/rc" 2>/dev/null); SVERR=$(cat "$SVD/err"); rm -f "$SVD/once"
}
svrun SV_LINE="kill-session $SVG" SV_EXPECT=gamma SV_TARGET="$SVG" SV_RENAME=other
is "a session renamed between the print and the kill is not killed" "alpha beta other " "$(sv_names)"
case "$SVERR" in *"✕"*"now called 'other', not 'gamma'"*"nothing changed"*) ok "and tm refuses with a ✕ note naming both names" ;;
    *) bad "and tm refuses with a ✕ note naming both names" "✕ ... now called 'other', not 'gamma'" "$SVERR" ;; esac
is "and exits 1" "1" "$SVRC"
"$TMUX_REAL" -L "$SVS" rename-session -t "$SVG" gamma

svrun SV_LINE="kill-session $SVG" SV_EXPECT=gamma
is "the same kill with the name unchanged goes through" "alpha beta " "$(sv_names)"
case "$SVERR" in *"killed: gamma"*) ok "and says what it killed" ;; *) bad "and says what it killed" "killed: gamma" "$SVERR" ;; esac
"$TMUX_REAL" -L "$SVS" new-session -d -s gamma 'exec sleep 300'; SVG=$("$TMUX_REAL" -L "$SVS" display-message -p -t gamma: '#{session_id}')

svrun SV_LINE="kill-session $SVG"
is "a kill with no .expect kills nothing" "alpha beta gamma " "$(sv_names)"
svrun SV_LINE="rename-session $SVG" SV_EXPECT=gamma SV_TARGET="$SVG" SV_RENAME=other SV_TEXT=zed
is "a rename races the same way and is refused" "alpha beta other " "$(sv_names)"
"$TMUX_REAL" -L "$SVS" rename-session -t "$SVG" gamma
svrun SV_LINE="rename-session $SVG" SV_EXPECT=gamma SV_TEXT='a.b c'
is "a rename with the name unchanged goes through, cleaned like a new name" "a_b_c alpha beta " "$(sv_names)"
"$TMUX_REAL" -L "$SVS" rename-session -t "$SVG" gamma
svrun SV_LINE="new-session -" SV_TEXT='--x y'
is "new-session - creates the .text name, cleaned of a leading dash and a space" "alpha beta gamma x_y " "$(sv_names)"
"$TMUX_REAL" -L "$SVS" kill-session -t x_y
for bad_line in "switch beta" 'kill-session $1x' 'kill-session beta' 'kill-session $1;x' 'switch $' 'new-session beta' 'frob $1'; do
    svrun SV_LINE="$bad_line" SV_EXPECT=beta
    is "an action line '$bad_line' is refused: exit 1, nothing killed or attached" "1 alpha beta gamma  " \
       "$SVRC $(sv_names) $("$TMUX_REAL" -L "$SVS" list-clients 2>/dev/null)"
done
"$TMUX_REAL" -L "$SVS" set -g @cockpit_glyphs ascii
svrun SV_LINE="switch $SVB" SV_GLYPHS_OUT="$SVD/glyphs"
is "@cockpit_glyphs ascii reaches the picker as COCKPIT_GLYPHS" "ascii" "$(cat "$SVD/glyphs" 2>/dev/null)"
"$TMUX_REAL" -L "$SVS" set -u -g @cockpit_glyphs
svrun TM_SID="$SVA" SV_LINE="kill-session $SVA" SV_EXPECT=alpha
is "tm will not kill the session it was run from, whatever the picker says" "alpha beta gamma " "$(sv_names)"
case "$SVERR" in *"the session you are in"*) ok "and says why" ;; *) bad "and says why" "the session you are in" "$SVERR" ;; esac
"$TMUX_REAL" -L "$SVS" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1
fi

echo "== tm sessions picker: a name is data, never a tmux format (#245) =="
# tmux expands #{...} and #(...) in a session name and in display-message, and
# #(cmd) runs cmd. ${IFS} stands in for the space tm's name cleaner would turn
# into an underscore; the link has no dot, which it would turn into one too.
if need_tmux "tm names are data"; then :; else
sv_server d; SVD=$(mktmpd); sv_stub "$SVD"
HOSTL="/tmp/svp$$"; ln -s "$SVD" "$HOSTL"; HOSTF="$HOSTL/pwned"; : > "$SVD/args"
svrun() {                     # svrun <env words...> -- run tm against the stub; rc in SVRC, stderr in SVERR
    sv_tm "$SVD" COCKPIT_TUI_BIN="$SVD/picker" SV_ONCE="$SVD/once" SV_TMUX="$TMUX_REAL" SV_SOCK="$SVS" SV_ARGS="$SVD/args" "$@" --
    until_file "$SVD/rc" '?*'; SVRC=$(cat "$SVD/rc" 2>/dev/null); SVERR=$(cat "$SVD/err"); rm -f "$SVD/once"
}
sv_lit() { "$TMUX_REAL" -L "$SVS" list-sessions -F '#{session_name}' 2>/dev/null | grep -cxF -- "$1"; }
HN='x#(touch${IFS}'"$HOSTF"')y'
svrun SV_LINE="new-session -" SV_TEXT="$HN"
sleep 0.5
is "new-session with #(cmd) in the name runs nothing" "absent" "$([ -e "$HOSTF" ] && echo present || echo absent)"
is "and the session carries the name literally" "1" "$(sv_lit "$HN")"
svrun SV_LINE="new-session -" SV_TEXT='a#{pid}b'
is "#{pid} in a new name is not expanded" "1" "$(sv_lit 'a#{pid}b')"
svrun SV_LINE="new-session -" SV_TEXT='n#[fg=red]d'
is "#[ in a new name is kept" "1" "$(sv_lit 'n#[fg=red]d')"
svrun SV_LINE="new-session -" SV_TEXT='m##x#'
is "a run of # and a trailing # are kept" "1" "$(sv_lit 'm##x#')"
svrun SV_LINE="new-session -" SV_TEXT='c#[fg=red]d'
is "#[fg=red] in a new name is kept" "1" "$(sv_lit 'c#[fg=red]d')"
svrun SV_LINE="rename-session $SVG" SV_EXPECT=gamma SV_TEXT="$HN"'2'
sleep 0.5
is "rename-session with #(cmd) runs nothing" "absent" "$([ -e "$HOSTF" ] && echo present || echo absent)"
is "and renames to the literal name" "1" "$(sv_lit "$HN"2)"
"$TMUX_REAL" -L "$SVS" rename-session -t "$SVG" 'e##{pid}f'
svrun SV_LINE="rename-session $SVG" SV_EXPECT='e#{pid}f' SV_TEXT=plain
is "a name with #{pid} is renamed when .expect is that name" "1" "$(sv_lit plain)"
# A refusal names the live session: that text must reach display-message escaped.
"$TMUX_REAL" -L "$SVS" rename-session -t "$SVG" 'g##(touch${IFS}'"$HOSTF"')h'
: > "$SVD/args"
svrun TM_POPUP=1 SV_LINE="kill-session $SVG" SV_EXPECT=gamma
sleep 0.5
is "a refusal naming a #(cmd) session runs nothing" "absent" "$([ -e "$HOSTF" ] && echo present || echo absent)"
is "and hands display-message the escaped text" "0 1" "$(grep '^display-message' "$SVD/args" | grep -c '[^#]#(' ) $(grep '^display-message' "$SVD/args" | grep -c '##(')"
printf '%s' "$SVERR" > "$SVD/e.txt"
is "and stderr still shows the name as it is" "1" "$(grep -cF -- 'g#(touch${IFS}' "$SVD/e.txt")"
# The picker draws the same names literally.
"$TMUX_REAL" -L "$SVS" rename-session -t "$SVG" 'k##{pid}##(touch${IFS}'"$HOSTF"')'
if [ -x "$DIR/bin/cockpit-tui" ]; then
    sv_ui "$SVD" --current "$SVA"; sv_cap '*k#{pid}#(touch${IFS}*'
    sv_cap '*k#{pid}#(touch*'; svshot=$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t)
    is "the sessions view draws the name as it is, and runs nothing" "1 absent" \
       "$(printf '%s\n' "$svshot" | grep -cF -- 'k#{pid}#(touch') $([ -e "$HOSTF" ] && echo present || echo absent)"
    sv_key Escape; until_file "$SVD/rc" '?*'
fi
# A waiting session's question may carry the record separators and a newline.
"$TMUX_REAL" -L "$SVS" set -p -t beta: @panestate_msg "$(printf 'Ship it?\037now\nplease\tok')"
if [ -x "$DIR/bin/cockpit-tui" ]; then
    sv_ui "$SVD" --current "$SVA"; sv_cap '*Ship it? now please ok*'
    case "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t)" in
        *'Ship it? now please ok'*beta*|*beta*'Ship it? now please ok'*) ok "a question with a record separator, newline and tab reads as one line" ;;
        *) bad "a question with a record separator, newline and tab reads as one line" "Ship it? now please ok" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -6)" ;;
    esac
    is "and the session rows are not shifted by it" "3" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | grep -cE '^.?. [123] ')"
    sv_key Escape; until_file "$SVD/rc" '?*'
fi
rm -f "$HOSTL"
"$TMUX_REAL" -L "$SVS" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1
fi

echo "== tm --popup: sized to the sessions, the options are ceilings (#245) =="
# A fake tmux answers the one display-message --popup asks and logs the popup.
# Fields: sessions, the needs-you band, @cockpit_touch, width, height.
TP=$(mktmpd)
cat > "$TP/tmux" <<'STUB'
#!/bin/sh
case "$1" in
  display-message) printf '%s\n' "$TP_ANSWER" ;;
  display-popup)   shift; printf '%s\n' "$*" >> "$TP_LOG" ;;
  new-window)      shift; printf 'WINDOW %s\n' "$*" >> "$TP_LOG" ;;
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
is "desktop: 70% wide, rows + 5 under the 14-line floor" "-w 140 -h 14" "$(tpsize '7|3||||' 200 50)"
is "desktop: rows + 5 above it" "-w 140 -h 25" "$(tpsize '20|0||||' 200 50)"
is "desktop: 12 sessions get their 12 rows (prompt, header, footer and border make 17)" "-w 140 -h 17" "$(tpsize '12|0||||' 200 50)"
is "desktop: many sessions stop at 70% of the height" "-w 140 -h 35" "$(tpsize '40|0||||' 200 50)"
is "tablet: 90% wide" "-w 99 -h 14" "$(tpsize '7|3||||' 110 40)"
is "narrow: full width, rows + 5, a 14-line floor (12 inside the border)" "-w 56 -h 14" "$(tpsize '7|3||||' 56 30)"
is "narrow with many sessions: rows + 5" "-w 56 -h 25" "$(tpsize '20|0||||' 56 30)"
is "@cockpit_touch off at a narrow width: the desktop tier" "-w 50 -h 14" "$(tpsize '7|3|off|||' 56 30)"
is "@cockpit_touch on at a desktop width: full width" "-w 200 -h 14" "$(tpsize '7|3|on|||' 200 50)"
is "the size options are ceilings, percent or cells" "-w 120 -h 10" "$(tpsize '7|3||60%|10|' 200 50)"
is "and never floors" "-w 140 -h 14" "$(tpsize '7|3||90%|40|' 200 50)"
tpsize '7|3||||' 200 12 >/dev/null
case "$(cat "$TP/log")" in
    "WINDOW -t \$3: -n sessions TM_POPUP=1 TM_SID=\\\$3 TM_CLIENT=/dev/pts/9 "*/tm) ok "but a window of its session, running tm" ;;
    *) bad "but a window of its session, running tm" "WINDOW -t \$3: -n sessions TM_POPUP=1 ... /tm" "$(cat "$TP/log")" ;;
esac
is "a client of 14 lines gets its popup, at the floor and not 70% of 14" "-w 140 -h 14" "$(tpsize '7|3||||' 200 14)"
tpsize '7|3||||' 200 13 >/dev/null
case "$(cat "$TP/log")" in WINDOW*) ok "and 13 lines is still a window" ;; *) bad "and 13 lines is still a window" "WINDOW ..." "$(cat "$TP/log")" ;; esac
is "--needs sizes for the needs-you band" "-w 56 -h 14" "$(tpsize '7|4||||' 56 30 --needs)"
tpsize '7|3||||' 200 50 >/dev/null
case "$(cat "$TP/log")" in
    "-c /dev/pts/9 -t \$3 -w 140 -h 14 -T  sessions  -E TM_POPUP=1 TM_SID=\\\$3 TM_CLIENT=/dev/pts/9 "*/tm) ok "the popup goes to that client, in its session" ;;
    *) bad "the popup goes to that client, in its session" "-c /dev/pts/9 -t \$3 ... TM_SID=\\\$3 TM_CLIENT=/dev/pts/9 .../tm" "$(cat "$TP/log")" ;;
esac
tpsize '7|3||||' 56 30 --needs >/dev/null
case "$(cat "$TP/log")" in
    *"/tm --needs") ok "--needs rides into the popup" ;;
    *) bad "--needs rides into the popup" "... tm --needs" "$(cat "$TP/log")" ;;
esac

echo "== tm sessions picker: prefix+s popup switches the client, and kill leaves it open (#245) =="
# The real path: tm --popup opens the picker in a popup of a real client. The
# server starts with a leaked $TMUX_PANE, as one launched from inside tmux does (#212).
if [ ! -x "$DIR/bin/cockpit-tui" ]; then
    skip_ "tm popup (no bin/cockpit-tui; run make tui)"
elif need_tmux "tm popup"; then :; else
TQI="cockpit-tmqi-$$"; TQO="cockpit-tmqo-$$"; SOCKS="$SOCKS $TQI $TQO"
TMUX_PANE=%99 "$TMUX_REAL" -L "$TQI" -f /dev/null new-session -d -s one -x 120 -y 30 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TQI" new-session -d -s two 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TQI" new-session -d -s three 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$TQO" -f /dev/null new-session -d -s o -x 120 -y 30 \
    "TERM=xterm-256color exec $TMUX_REAL -L $TQI attach -t one" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$TQI" list-clients -F '#{client_name}'
tqc=$("$TMUX_REAL" -L "$TQI" list-clients -F '#{client_name}')
tqs=$("$TMUX_REAL" -L "$TQI" display-message -p -t one: '#{session_id}')
tqpop() { "$TMUX_REAL" -L "$TQI" run-shell -b "$DIR/scripts/tm --popup '$tqc' '$tqs' 120 30"; }
tqpop
until_cap "$TQO" o: '*session>*one (here)*'
"$TMUX_REAL" -L "$TQO" send-keys -t o: t w o
until_cap "$TQO" o: '*session> two*1/1 of 3*'
"$TMUX_REAL" -L "$TQO" send-keys -t o: Enter
until_out "$tqc two" "$TMUX_REAL" -L "$TQI" list-clients -F '#{client_name} #{session_name}'
is "enter on a row switches the popup's own client, and nests nothing" "$tqc two" \
   "$("$TMUX_REAL" -L "$TQI" list-clients -F '#{client_name} #{session_name}' 2>/dev/null)"
# From "two" the popup marks two as here, and ^X on it refuses.
tqs=$("$TMUX_REAL" -L "$TQI" display-message -p -t two: '#{session_id}')
tqpop
until_cap "$TQO" o: '*session>*two (here)*'
"$TMUX_REAL" -L "$TQO" send-keys -t o: t w o
until_cap "$TQO" o: '*session> two*1/1 of 3*'
"$TMUX_REAL" -L "$TQO" send-keys -t o: C-x
if until_cap "$TQO" o: '*your own session*'; then ok "the popup's own session is the one marked, and ^X on it refuses"
else bad "the popup's own session is the one marked, and ^X on it refuses" "your own session" "$("$TMUX_REAL" -L "$TQO" capture-pane -p -t o: | head -4)"; fi
"$TMUX_REAL" -L "$TQO" send-keys -t o: C-x
sleep 0.3
is "and a second ^X kills nothing" "one three two " "$("$TMUX_REAL" -L "$TQI" list-sessions -F '#{session_name}' 2>/dev/null | sort | tr '\n' ' ')"
# Kill another one: the popup stays open on the rest.
"$TMUX_REAL" -L "$TQO" send-keys -t o: C-u t h r e e C-x
until_cap "$TQO" o: '*kill three?*'
"$TMUX_REAL" -L "$TQO" send-keys -t o: C-x
until_out 'one two ' eval "\"$TMUX_REAL\" -L $TQI list-sessions -F '#{session_name}' 2>/dev/null | sort | tr '\n' ' '"
is "^X twice in the popup kills that session" "one two " "$("$TMUX_REAL" -L "$TQI" list-sessions -F '#{session_name}' 2>/dev/null | sort | tr '\n' ' ')"
until_cap "$TQO" o: '*session>*2 sessions*' && ok "and the popup stays open on the sessions left" \
    || bad "and the popup stays open on the sessions left" "session> ... 2 sessions" "$("$TMUX_REAL" -L "$TQO" capture-pane -p -t o: | head -4)"
"$TMUX_REAL" -L "$TQO" send-keys -t o: Escape
"$TMUX_REAL" -L "$TQO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TQI" kill-server >/dev/null 2>&1
fi

echo "== revdiff: the doctor grades it and bootstrap.sh fetches it, verified =="
# Fake box and a file:// release host: no network, no real HOME, no live tmux.
if ! command -v curl >/dev/null 2>&1; then skip_ "revdiff install (no curl)"
elif ! command -v sha256sum >/dev/null 2>&1; then skip_ "revdiff install (no sha256sum)"
elif ! command -v jq >/dev/null 2>&1; then skip_ "revdiff install (no jq)"; else
RV=$(mktmpd)
RVH="$RV/home"; RVB="$RV/bin"; RVREL="$RV/rel"; RVD="$RVREL/umputun/revdiff/releases/download/v1.13.0"
mkdir -p "$RVH/.tmux/plugins" "$RVB" "$RV/new" "$RV/old" "$RV/unk" "$RV/none" "$RV/tmuxtmp" "$RVD" "$RV/pack"
for d in tpm tmux-sensible tmux-yank tmux-resurrect tmux-continuum tmux; do mkdir -p "$RVH/.tmux/plugins/$d/.git"; done
rvshim() { printf '#!/bin/sh\necho "%s"\n' "$2" > "$3/$1"; chmod +x "$3/$1"; }
rvshim revdiff "version: 1.13.0" "$RV/new"; rvshim revdiff "version: 1.12.9" "$RV/old"
rvshim revdiff "unknown" "$RV/unk"
mkdir -p "$RV/rel" "$RV/relold" "$RV/dev" "$RV/un" "$RV/brew"
rvshim revdiff "dev" "$RV/dev"; mkdir -p "$RV/empty"; printf '#!/bin/sh\nexit 1\n' > "$RV/empty/revdiff"; chmod +x "$RV/empty/revdiff"
printf '#!/bin/sh\ncase "$1" in -s) echo "$FAKE_S" ;; -m) echo "$FAKE_M" ;; *) exec /usr/bin/uname "$@" ;; esac\n' > "$RV/un/uname"
printf '#!/bin/sh\necho "$*" >> "%s"\n' "$RV/brew.log" > "$RV/brew/brew"; chmod +x "$RV/un/uname" "$RV/brew/brew"
rvshim revdiff "version: v1.13.0-abc1234-20260905T044327" "$RV/rel"
rvshim revdiff "version: v1.12.0-abc1234-20260905T044327" "$RV/relold"
rvdc() {                      # rvdc <dir holding the fake revdiff> -- the doctor's rows
    env -u TMUX -u TMUX_PANE HOME="$RVH" XDG_BIN_HOME="$RVB" TMUX_TMPDIR="$RV/tmuxtmp" \
        PATH="$1:$SHIM:/usr/bin:/bin" "$DIR/install.sh" --check 2>&1
}
rvrow() { printf '%s\n' "$1" | sed -n 's/^\[ *\([a-z]*\) *\] *revdiff .*/\1/p'; }
out=$(rvdc "$RV/new"); is "revdiff at the pin is an ok row" "ok" "$(rvrow "$out")"
out=$(rvdc "$RV/rel"); is "a release build's v1.13.0-sha-date is an ok row" "ok" "$(rvrow "$out")"
out=$(rvdc "$RV/relold"); is "and v1.12.0-sha-date is a warn row" "warn" "$(rvrow "$out")"
out=$(rvdc "$RV/dev"); is "an unparseable version (dev) is a warn row" "warn" "$(rvrow "$out")"
is "and is called a go install build" "1" "$(printf '%s\n' "$out" | grep -c 'dev @.*a go install build')"
out=$(rvdc "$RV/empty"); is "a revdiff that prints no version is a warn row" "warn" "$(rvrow "$out")"
is "and is not called a go install build" "1 0" "$(printf '%s\n' "$out" | grep -c 'no version @') $(printf '%s\n' "$out" | grep -c 'go install build')"
# A server holds the PATH it started with: its revdiff is the one prefix r runs, even when
# this shell finds a newer one first.
RVS="env -u TMUX TMUX_TMPDIR=$RV/tmuxtmp $TMUX_REAL -L $SUITE_SOCK -f /dev/null"
$RVS new-session -d 'sleep 300' >/dev/null 2>&1
$RVS set-environment -g PATH "$RV/old:/usr/bin:/bin" >/dev/null 2>&1
is "the revdiff on the server's PATH is the one graded" "warn" "$(rvrow "$(rvdc "$RV/new")")"
$RVS kill-server >/dev/null 2>&1
out=$(rvdc "$RV/old")
is "revdiff below the tested version is a warn row" "warn" "$(rvrow "$out")"
out=$(rvdc "$RV/unk"); is "an unknown version is a warn row" "warn" "$(rvrow "$out")"
out=$(rvdc "$RV/none")
is "a missing revdiff is a fail row" "fail" "$(rvrow "$out")"
is "and its fix names the brew route" "1" "$(printf '%s\n' "$out" | grep -c 'brew install umputun/apps/revdiff')"
is "and the go route" "1" "$(printf '%s\n' "$out" | grep -c 'go install github.com/umputun/revdiff/app@v1.13.0 (rename app to revdiff)')"

# One tarball per published name; the checksums file lists them as upstream does.
rvrel() {                     # rvrel [flip]
    local a
    rm -f "$RVD"/*; printf '#!/bin/sh\necho "version: 1.13.0"\n' > "$RV/pack/revdiff"; chmod +x "$RV/pack/revdiff"
    for a in linux_amd64 linux_arm64 darwin_amd64 darwin_arm64; do
        printf '#!/bin/sh\necho "version: 1.13.0 %s"\n' "$a" > "$RV/pack/revdiff"
        ( cd "$RV/pack" && tar -czf "$RVD/revdiff_1.13.0_$a.tar.gz" revdiff )
    done
    ( cd "$RVD" && sha256sum revdiff_1.13.0_*.tar.gz > revdiff_1.13.0_checksums.txt )
    if [ "${1:-}" = flip ]; then
        awk '{ c=substr($0,1,1); print (c=="0" ? "1" : "0") substr($0,2) }' \
            "$RVD/revdiff_1.13.0_checksums.txt" > "$RV/s2" &&
            mv "$RV/s2" "$RVD/revdiff_1.13.0_checksums.txt"
    fi
}
rvbs() {                      # rvbs <extra PATH dir> <args...> -- bootstrap.sh on the fake box
    local p=$1; shift
    env -u TMUX -u TMUX_PANE HOME="$RVH" XDG_BIN_HOME="$RVB" XDG_CONFIG_HOME="$RVH/.config" \
        TMUX_TMPDIR="$RV/tmuxtmp" COCKPIT_GITHUB="file://$RVREL" \
        PATH="$p:$SHIM:/usr/bin:/bin" "$DIR/bootstrap.sh" "$@" </dev/null 2>&1
}
rvrel
out=$(rvbs "$RV/none" --dry-run); rc=$?
is "--dry-run says it would fetch revdiff" "1" "$(printf '%s\n' "$out" | grep -c 'would: fetch revdiff 1.13.0')"
is "and writes nothing into \$BIN" "" "$(ls -A "$RVB")"
out=$(rvbs "$RV/none"); rc=$?
is "bootstrap installs revdiff into \$BIN" "version: 1.13.0" "$("$RVB/revdiff" --version 2>/dev/null | cut -d' ' -f1-2)"
is "and exits 0" "0" "$rc"
rm -f "$RVB/revdiff"
out=$(rvbs "$RV/new"); rc=$?
is "a revdiff at the pin on PATH is skipped" "absent" "$([ -e "$RVB/revdiff" ] && echo present || echo absent)"
out=$(rvbs "$RV/unk")
is "an unknown-version revdiff is present, not replaced" "absent" "$([ -e "$RVB/revdiff" ] && echo present || echo absent)"
out=$(rvbs "$RV/dev")
is "an unparseable version (dev) is present, not replaced" "absent" "$([ -e "$RVB/revdiff" ] && echo present || echo absent)"
out=$(rvbs "$RV/rel")
is "a v-prefixed release build at the pin is skipped" "absent" "$([ -e "$RVB/revdiff" ] && echo present || echo absent)"
out=$(rvbs "$RV/relold")
is "a v-prefixed one below the pin is fetched" "present" "$([ -e "$RVB/revdiff" ] && echo present || echo absent)"
rm -f "$RVB/revdiff"
out=$(rvbs "$RV/old")
is "one below the pin is fetched" "present" "$([ -e "$RVB/revdiff" ] && echo present || echo absent)"
rm -f "$RVB/revdiff"
rvrel flip
out=$(rvbs "$RV/none"); rc=$?
is "a checksum mismatch installs nothing" "absent" "$([ -e "$RVB/revdiff" ] && echo present || echo absent)"
is "and says so on one line" "1" "$(printf '%s\n' "$out" | grep -c 'FAIL:  revdiff checksum mismatch -- NOT installed')"
is "and the run exits 1" "1" "$rc"
# The uname-to-asset mapping: each fake asset says which one it is.
rvrel
for m in "Linux x86_64 linux_amd64" "Linux aarch64 linux_arm64" "Linux arm64 linux_arm64" \
         "Darwin arm64 darwin_arm64" "Darwin x86_64 darwin_amd64"; do
    set -- $m; rm -f "$RVB/revdiff"
    out=$(FAKE_S=$1 FAKE_M=$2 rvbs "$RV/un:$RV/none")
    is "uname $1 $2 fetches the $3 asset" "version: 1.13.0 $3" "$("$RVB/revdiff" --version 2>/dev/null)"
done
# macOS with brew: brew installs it, and nothing is fetched into $BIN.
rm -f "$RVB/revdiff" "$RV/brew.log"
out=$(FAKE_S=Darwin FAKE_M=arm64 rvbs "$RV/un:$RV/brew:$RV/none")
is "macOS with brew runs brew install umputun/apps/revdiff" "1" "$(grep -cx 'install umputun/apps/revdiff' "$RV/brew.log")"
is "and fetches nothing into \$BIN" "absent" "$([ -e "$RVB/revdiff" ] && echo present || echo absent)"
rm -rf "$RVD"; mkdir -p "$RVD"
out=$(rvbs "$RV/none"); rc=$?
is "offline (no release host) installs nothing" "absent" "$([ -e "$RVB/revdiff" ] && echo present || echo absent)"
is "and says so on one line" "1" "$(printf '%s\n' "$out" | grep -c 'FAIL:  revdiff checksums unreachable')"
rm -rf "$RV"
fi

# --- #246 branch popup: tmux-branch reads cockpit-tui's answer ----------------
# Obsolete fzf blocks deleted by this change: "tmux-branch: usable without
# column(1)", "tmux-branch: builds each row without a fork per branch",
# "tmux-branch: row format is unchanged by the fork removal", "the branch picker
# must not hijack ctrl-n", the `fzrun branch` and "tmux-branch without fzf
# prompts on stderr" rows of the shared fzf theme block, and tmux-branch in the
# --color=16 border loop. They drove fzf or its numbered fallback, which is gone.
br_repo() {                   # br_repo <dir> <bare-remote-dir>
    (
        export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
        git init -q -b main "$1" && cd "$1" || exit 1
        echo main > a; git add a; git commit -q -m init
        git branch old; git branch held; git branch 'feat/$(touch${IFS}x);`id`'; git branch '#(touch${IFS}F)#{pid}'
        git checkout -q -b conf; echo conf > a; git commit -q -am conf; git checkout -q main
        git init -q --bare "$2"; git remote add origin "$2"
        git branch only-remote; git push -q origin only-remote; git branch -q -D only-remote
        git fetch -q origin
    ) >/dev/null 2>&1
}
br_cur() { git -C "$1" branch --show-current; }

echo "== tmux-branch (#246): the popup's keys switch, track, create and refuse =="
if need_tmux "branch popup keys"; then :; else
BRR=$(mktmpd); BRM=$(mktmpd); br_repo "$BRR" "$BRM"
BRW=$(mktmpd); rm -rf "$BRW"; git -C "$BRR" worktree add -q "$BRW" held >/dev/null 2>&1
BRS="cockpit-br246-$$"; SOCKS="$SOCKS $BRS"
# A tmux that logs its argv, first on the panes' PATH: a name in any argument the
# script gives tmux shows up here, which a #(...) racing the check cannot hide.
BRSHIM=$(mktmpd); BRTL="$BRSHIM/argv.log"
printf '#!/bin/sh\nfor a in "$@"; do printf "%%s\\n" "$a"; done >> "%s"\nexec %s "$@"\n' "$BRTL" "$TMUX_REAL" > "$BRSHIM/tmux"
chmod +x "$BRSHIM/tmux"
env PATH="$BRSHIM:$PATH" "$TMUX_REAL" -L "$BRS" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$BRR" >/dev/null 2>&1
BRP=$("$TMUX_REAL" -L "$BRS" list-panes -a -F '#{pane_id}' | head -1)
# A client makes display-message and its kin do something: without one a leak
# through a status message goes nowhere and a test cannot see it.
BRO="cockpit-br246o-$$"; SOCKS="$SOCKS $BRO"
"$TMUX_REAL" -L "$BRO" -f /dev/null new-session -d -s o -x 100 -y 32 \
    "TERM=xterm-256color exec $TMUX_REAL -L $BRS attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$BRS" list-clients -F '#{client_name}'
brn=0
br_run() {                    # start the script in the pane; wait for the picker
    brn=$((brn+1))
    "$TMUX_REAL" -L "$BRS" send-keys -t "$BRP" "'$DIR/scripts/tmux-branch' '$BRR'; echo BRDONE$brn=\$?" Enter
    until_cap "$BRS" "$BRP" '*branch>*'
}
br_done() { until_cap "$BRS" "$BRP" "*BRDONE$brn=[0-9]*"; }
br_type() { "$TMUX_REAL" -L "$BRS" send-keys -t "$BRP" -l "$1"; }
br_key()  { "$TMUX_REAL" -L "$BRS" send-keys -t "$BRP" "$@"; }
br_cap()  { "$TMUX_REAL" -L "$BRS" capture-pane -p -J -t "$BRP"; }

br_run; br_type old; until_cap "$BRS" "$BRP" '*branch> old*'; br_key Enter; br_done
is "↵ on a local branch switches to it" "old" "$(br_cur "$BRR")"
git -C "$BRR" switch -q main

br_run; br_type only-remote; until_cap "$BRS" "$BRP" '*branch> only-remote*'; br_key Enter; br_done
is "↵ on a remote-only branch creates a local one" "only-remote" "$(br_cur "$BRR")"
is "and it tracks the remote" "origin/only-remote" \
   "$(git -C "$BRR" rev-parse --abbrev-ref '@{upstream}' 2>/dev/null)"
git -C "$BRR" switch -q main

br_run; br_type newbr; until_cap "$BRS" "$BRP" '*branch> newbr*'; br_key M-Enter; br_done
is "alt-↵ creates the branch named by the filter" "newbr" "$(br_cur "$BRR")"
git -C "$BRR" switch -q main

br_run; br_type held; until_cap "$BRS" "$BRP" '*branch> held*'
case "$(br_cap)" in
    *⑂*) ok "a branch held by another worktree wears ⑂" ;;
    *) bad "a branch held by another worktree wears ⑂" "a ⑂ mark" "$(br_cap | sed -n 1,6p)" ;;
esac
br_key Enter; br_done
# #303: not a note any more; with no pane in that worktree, a window opens there.
is "↵ on it opens a window in that worktree (#303)" "$BRW" \
   "$("$TMUX_REAL" -L "$BRS" list-panes -a -F '#{window_index} #{pane_current_path}' | sed -n 's/^1 //p')"
"$TMUX_REAL" -L "$BRS" kill-window -t t:1; "$TMUX_REAL" -L "$BRS" select-window -t "$BRP"
is "and nothing was switched" "main" "$(br_cur "$BRR")"

# A name made of shell syntax: printed as itself, handled as itself.
BRH='feat/$(touch${IFS}x);`id`'
br_run; br_type 'feat/'; until_cap "$BRS" "$BRP" '*branch> feat/*'
case "$(br_cap)" in
    *'feat/$(touch${IFS}x);`id`'*) ok "a branch named with shell syntax is drawn exactly" ;;
    *) bad "a branch named with shell syntax is drawn exactly" "$BRH" "$(br_cap | sed -n 1,5p)" ;;
esac
br_key Enter; br_done
is "↵ switches to that exact ref" "$BRH" "$(br_cur "$BRR")"
is "and no command in the name ran" "none" "$([ -e "$BRR/x" ] || [ -e "$HOME/x" ] && echo ran || echo none)"
git -C "$BRR" switch -q main

# tmux formats: a name with #( ) or #{ } must stay text wherever it is shown. The
# popup and git's "Switched to" line are drawn by the pane itself, so the proof is
# that the literal survives to the screen and no #(...) ran.
BRF='#(touch${IFS}F)#{pid}'
br_run; br_type '#{pid}'; until_cap "$BRS" "$BRP" '*branch> #{pid}*'
case "$(br_cap)" in
    *'#(touch${IFS}F)#{pid}'*) ok "a #( ) #{ } branch name is drawn exactly" ;;
    *) bad "a #( ) #{ } branch name is drawn exactly" "$BRF" "$(br_cap | sed -n 1,5p)" ;;
esac
br_key Enter; br_done
is "↵ switches to that exact ref" "$BRF" "$(br_cur "$BRR")"
case "$(br_cap)" in
    *"Switched to branch '$BRF'"*) ok "git's message shows the name unexpanded" ;;
    *) bad "git's message shows the name unexpanded" "Switched to branch '$BRF'" "$(br_cap | tail -5)" ;;
esac
# The argv log is deterministic where the file check is not (#() runs async).
is "and tmux was never handed the name" "0" "$(grep -c -e '#{pid}' -e 'touch' "$BRTL" 2>/dev/null)"
is "(the shim saw the script's own tmux calls)" "yes" "$([ -s "$BRTL" ] && echo yes || echo no)"
# #(...) from a client-side format runs in the SERVER's start dir, not the pane's.
is "and no #( ) in it ran, in the repo, the home dir or the server's start dir" "none" \
   "$([ -e "$BRR/F" ] || [ -e "$HOME/F" ] || [ -e "$PWD/F" ] || [ -e "$DIR/F" ] && echo ran || echo none)"
git -C "$BRR" switch -q main

# git's own refusal is shown verbatim, [any key] returns to the picker.
echo dirty > "$BRR/a"
br_run; br_type conf; until_cap "$BRS" "$BRP" '*branch> conf*'; br_key Enter
if until_cap "$BRS" "$BRP" '*any key*'; then ok "a refused switch shows git's message and [any key]"
else bad "a refused switch shows git's message and [any key]" "[any key]" "$(br_cap | head -8)"; fi
case "$(br_cap)" in
    *overwritten*) ok "git's wording is verbatim" ;;
    *) bad "git's wording is verbatim" "git's 'would be overwritten' error" "$(br_cap | head -8)" ;;
esac
br_key Space
if until_cap "$BRS" "$BRP" '*branch>*'; then ok "any key returns to the picker"
else bad "any key returns to the picker" "the picker again" "$(br_cap | head -4)"; fi
br_key Escape; br_done
git -C "$BRR" checkout -q -- a

# ctrl-n moves down: it never creates a branch from the filter text.
before=$(git -C "$BRR" for-each-ref --format='%(refname)' refs/heads | sort | tr '\n' ' ')
br_run; br_type zzz; until_cap "$BRS" "$BRP" '*branch> zzz*'; br_key C-n Escape; br_done
is "ctrl-n after typing creates no branch" "$before" \
   "$(git -C "$BRR" for-each-ref --format='%(refname)' refs/heads | sort | tr '\n' ' ')"
"$TMUX_REAL" -L "$BRO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$BRS" kill-server >/dev/null 2>&1
git -C "$BRR" worktree remove --force "$BRW" >/dev/null 2>&1
rm -rf "$BRR" "$BRM" "$BRW"
fi

echo "== tmux-branch (#246): the script trusts nothing the binary prints =="
BRR=$(mktmpd); BRM=$(mktmpd); br_repo "$BRR" "$BRM"
BRX=$(mktmpd)
cat > "$BRX/cockpit-tui" <<'EOS'
#!/bin/sh
# prints BR_STUB_OUT once, then leaves like esc, so a refused answer cannot loop
[ -e "$BR_STUB_OUT.used" ] && exit 1
: > "$BR_STUB_OUT.used"; cat "$BR_STUB_OUT"
EOS
chmod +x "$BRX/cockpit-tui"
br_stub() {                   # br_stub <answer> -> the script's output; the repo is left on main
    rm -f "$BRX/out.used"; printf '%s\n' "$1" > "$BRX/out"
    env -u TMUX COCKPIT_TUI_BIN="$BRX/cockpit-tui" BR_STUB_OUT="$BRX/out" \
        "$DIR/scripts/tmux-branch" "$BRR" </dev/null 2>&1
}
br_want() {                   # br_want <label> <answer> <expected text>
    local got; got=$(br_stub "$2")
    case "$got" in
        *"$3"*) ok "$1" ;;
        *) bad "$1" "$3" "$got" ;;
    esac
    is "$1: nothing was checked out" "main" "$(br_cur "$BRR")"
}
br_want "an unknown verb is refused"            'rm -rf /'            "unexpected answer"
br_want "a verb with no name is refused"        'switch'              "not a branch name"
br_want "a name git rejects is refused"         'switch ../x'         "not a branch name"
br_want "a branch that does not exist is refused" 'switch nope'       "no such branch"
br_want "track of a local branch is refused"    'track old'           "no such branch"
br_want "create with an invalid name is refused" 'create bad..name'   "not a branch name"
br_want "two lines are refused"                 $'switch old\nswitch main' "not a branch name"
br_stub 'switch old' >/dev/null
is "a good answer is acted on" "old" "$(br_cur "$BRR")"
git -C "$BRR" switch -q main
br_stub 'switch feat/$(touch${IFS}x);`id`' >/dev/null
is "a metacharacter name is switched to literally" 'feat/$(touch${IFS}x);`id`' "$(br_cur "$BRR")"
is "and no command in it ran" "none" "$([ -e "$BRR/x" ] || [ -e "$HOME/x" ] && echo ran || echo none)"
git -C "$BRR" switch -q main
br_stub 'track origin/only-remote' >/dev/null
is "track makes a local branch from the remote one" "only-remote" "$(br_cur "$BRR")"
git -C "$BRR" switch -q main
br_stub 'create fresh' >/dev/null
is "create makes the branch" "fresh" "$(br_cur "$BRR")"
rm -rf "$BRR" "$BRM" "$BRX"

echo "== tmux-branch (#246): its refusals are unchanged =="
BRN=$(mktmpd); BRR=$(mktmpd); br_repo "$BRR" "$(mktmpd)"
out=$(env -u TMUX "$DIR/scripts/tmux-branch" "$BRN" </dev/null 2>&1); rc=$?
case "$out" in *"not a git repository: $BRN"*) ok "a directory that is not a repo is refused" ;; *) bad "a directory that is not a repo is refused" "not a git repository: $BRN" "$out" ;; esac
is "and exits 1" "1" "$rc"
out=$(env -u TMUX "$DIR/scripts/tmux-branch" -c ssh "$BRR" </dev/null 2>&1); rc=$?
case "$out" in *"inside ssh"*) ok "an ssh pane is refused" ;; *) bad "an ssh pane is refused" "this pane is inside ssh" "$out" ;; esac
is "and exits 1" "1" "$rc"
out=$(env -u TMUX "$DIR/scripts/tmux-branch" "$BRR/missing" </dev/null 2>&1)
case "$out" in *"no such directory"*) ok "a missing directory is refused" ;; *) bad "a missing directory is refused" "no such directory" "$out" ;; esac
out=$(env -u TMUX COCKPIT_TUI_BIN="$BRN/none" "$DIR/scripts/tmux-branch" "$BRR" </dev/null 2>&1); rc=$?
case "$out" in *"cockpit-tui is missing"*) ok "a missing binary is named" ;; *) bad "a missing binary is named" "cockpit-tui is missing" "$out" ;; esac
is "and exits 1" "1" "$rc"
# A relative directory is resolved once: the script cd's, then names the place it is in.
printf '#!/bin/sh\necho "picker for $*" >&2\nexit 1\n' > "$BRN/cockpit-tui"; chmod +x "$BRN/cockpit-tui"
out=$(cd "${BRR%/*}" && env -u TMUX COCKPIT_TUI_BIN="$BRN/cockpit-tui" "$DIR/scripts/tmux-branch" "${BRR##*/}" </dev/null 2>&1)
is "a relative directory reaches the binary as its absolute path" "picker for branch -C $BRR" "$out"
rm -rf "$BRN" "$BRR"

# --- #303 branch popup: a branch another worktree holds jumps there -----------
# cockpit-tui is a stub that answers `goto <path>`, once: the binary's half is in
# tui/tests/branch.rs. Everything here is the script's half, on private servers.
bg_server() {                 # bg_server <socket> <dir>  one session, one pane in <dir>
    "$TMUX_REAL" -L "$1" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$(fesc "$2")" 'exec sleep 300' >/dev/null 2>&1
    "$TMUX_REAL" -L "$1" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
}
bg_pane() {                   # bg_pane <socket> <dir> <cmd>  a new window in <dir>; prints its pane id
    "$TMUX_REAL" -L "$1" new-window -P -F '#{pane_id}' -t t: -c "$(fesc "$2")" "$3" 2>/dev/null   # -c is a format
}
bg_goto() {                   # bg_goto <socket> <origin-pane> <repo> <path>  -> the script's exit code
    local sp mark; sp=$("$TMUX_REAL" -L "$1" display-message -p '#{socket_path}')
    mark=$(mktmpd)
    printf '#!/bin/sh\n[ -e "%s/ran" ] && exit 1\n: > "%s/ran"\nprintf "goto %%s\\n" "$BG_GOTO"\n' "$mark" "$mark" > "$mark/cockpit-tui"
    chmod +x "$mark/cockpit-tui"
    env ${BG_SHIM:+PATH="$BG_SHIM:$PATH"} TMUX="$sp,0,0" COCKPIT_TUI_BIN="$mark/cockpit-tui" BG_GOTO="$4" \
        timeout 30 "$DIR/scripts/tmux-branch" -p "$2" "$3" </dev/null >/dev/null 2>"$mark/err"
    BG_ERR=$(cat "$mark/err")
}
bg_active() { "$TMUX_REAL" -L "$1" display-message -p -t t: '#{pane_id}'; }
bg_windows() { "$TMUX_REAL" -L "$1" list-windows -t t: 2>/dev/null | grep -c .; }

echo "== tmux-branch (#303): ↵ on a branch another worktree holds goes to that worktree =="
if need_tmux "branch goto"; then :; else
# shellcheck source=../scripts/lib/fmt.sh
. "$DIR/scripts/lib/fmt.sh"
BGR=$(mktmpd); BGM=$(mktmpd); br_repo "$BGR" "$BGM"
BGD=$(mktmpd); BGF="$BGD/F"
# A space and a tmux format in the path; -c would run the #( ) if it were not escaped.
BGW="$BGD/wt #(touch $BGF) x"
git -C "$BGR" worktree add -q "$BGW" held >/dev/null 2>&1
mkdir -p "$BGW/sub"
BGS="cockpit-bg303-$$"; SOCKS="$SOCKS $BGS"
bg_server "$BGS" "$BGR"
BGO=$("$TMUX_REAL" -L "$BGS" list-panes -a -F '#{pane_id}' | head -1)
BGP=$(bg_pane "$BGS" "$BGW" 'exec sleep 300')
"$TMUX_REAL" -L "$BGS" select-window -t "$BGO"
bg_goto "$BGS" "$BGO" "$BGR" "$BGW"
is "a goto to a worktree with a pane in it says nothing on stderr" "" "$BG_ERR"
is "it selects the pane already in that worktree" "$BGP" "$(bg_active "$BGS")"
is "and opens no new window" "2" "$(bg_windows "$BGS")"
is "and no #( ) in the path ran" "none" "$([ -e "$BGF" ] && echo ran || echo none)"

# An agent pane in a subdirectory of the tree, in a later window than the shell.
BGA=$(bg_pane "$BGS" "$BGW/sub" 'exec -a claude sleep 300')
until_out claude "$TMUX_REAL" -L "$BGS" display-message -p -t "$BGA" '#{pane_current_command}'
"$TMUX_REAL" -L "$BGS" select-window -t "$BGO"
bg_goto "$BGS" "$BGO" "$BGR" "$BGW"
is "an agent pane is preferred over a shell in the same tree" "$BGA" "$(bg_active "$BGS")"

# An agent whose cwd is the main checkout but whose @cockpit_dir names the worktree.
"$TMUX_REAL" -L "$BGS" kill-pane -t "$BGA"; "$TMUX_REAL" -L "$BGS" kill-pane -t "$BGP"
BGC=$(bg_pane "$BGS" "$BGR" 'exec -a claude sleep 300')
"$TMUX_REAL" -L "$BGS" set-option -p -t "$BGC" @cockpit_dir "$BGW"
until_out claude "$TMUX_REAL" -L "$BGS" display-message -p -t "$BGC" '#{pane_current_command}'
"$TMUX_REAL" -L "$BGS" select-window -t "$BGO"
bg_goto "$BGS" "$BGO" "$BGR" "$BGW"
is "a pane whose @cockpit_dir is the worktree counts, whatever its cwd" "$BGC" "$(bg_active "$BGS")"

# No pane there: a new window in the tree. No pane's cwd is touched.
"$TMUX_REAL" -L "$BGS" kill-pane -t "$BGC"
before=$("$TMUX_REAL" -L "$BGS" list-panes -a -F '#{pane_id} #{pane_current_path}')
bg_goto "$BGS" "$BGO" "$BGR" "$BGW"
is "with no pane there it opens one more window" "2" "$(bg_windows "$BGS")"
is "starting in the worktree, space and #( ) intact" "$BGW" \
   "$("$TMUX_REAL" -L "$BGS" display-message -p -t t: '#{pane_current_path}')"
is "and no #( ) in the path ran" "none" "$([ -e "$BGF" ] && echo ran || echo none)"
is "and the pane it came from keeps its cwd" "$before" \
   "$("$TMUX_REAL" -L "$BGS" list-panes -a -F '#{pane_id} #{pane_current_path}' | grep -F -x -f <(printf '%s\n' "$before"))"

# The line is untrusted: only a worktree of this repo is a place to go.
n=$(bg_windows "$BGS")
bg_goto "$BGS" "$BGO" "$BGR" "$BGD"
case "$BG_ERR" in
    *"not a worktree"*) ok "a directory that is no worktree of the repo is refused, by name" ;;
    *) bad "a directory that is no worktree of the repo is refused, by name" "not a worktree" "$BG_ERR" ;;
esac
is "and opens no window" "$n" "$(bg_windows "$BGS")"
bg_goto "$BGS" "$BGO" "$BGR" "relative/path"
is "a relative path opens no window either" "$n" "$(bg_windows "$BGS")"
"$TMUX_REAL" -L "$BGS" kill-server >/dev/null 2>&1

# A worktree nested inside the checkout: a pane there belongs to it, not to the checkout.
BGN="$BGR/nested"
git -C "$BGR" worktree add -q "$BGN" old >/dev/null 2>&1
BGS2="cockpit-bg303b-$$"; SOCKS="$SOCKS $BGS2"
bg_server "$BGS2" "$BGN"
BGO2=$("$TMUX_REAL" -L "$BGS2" list-panes -a -F '#{pane_id}' | head -1)
bg_goto "$BGS2" "$BGO2" "$BGN" "$BGR"
is "a pane in a nested worktree is not a pane of the enclosing checkout" "2" "$(bg_windows "$BGS2")"
is "and the new window is the checkout" "$BGR" "$("$TMUX_REAL" -L "$BGS2" display-message -p -t t: '#{pane_current_path}')"
"$TMUX_REAL" -L "$BGS2" kill-server >/dev/null 2>&1

# One tree per pane (finding: @cockpit_dir over cwd). An agent that works in the worktree
# from a cwd in the main checkout is a pane of the worktree, so `goto main` skips it.
BGS3="cockpit-bg303c-$$"; SOCKS="$SOCKS $BGS3"
bg_server "$BGS3" "$BGW"
BGO3=$("$TMUX_REAL" -L "$BGS3" list-panes -a -F '#{pane_id}' | head -1)
BGA3=$(bg_pane "$BGS3" "$BGR" 'exec -a claude sleep 300')
"$TMUX_REAL" -L "$BGS3" set-option -p -t "$BGA3" @cockpit_dir "$BGW"
BGM3=$(bg_pane "$BGS3" "$BGR" 'exec sleep 300')
until_out claude "$TMUX_REAL" -L "$BGS3" display-message -p -t "$BGA3" '#{pane_current_command}'
"$TMUX_REAL" -L "$BGS3" select-window -t "$BGO3"
bg_goto "$BGS3" "$BGO3" "$BGR" "$BGR"
is "an agent with cwd main and @cockpit_dir the worktree is not a pane of main" "$BGM3" "$(bg_active "$BGS3")"
"$TMUX_REAL" -L "$BGS3" kill-server >/dev/null 2>&1

# A worktree whose directory was deleted by hand is still listed.
BGG="$BGD/gone"
git -C "$BGR" worktree add -q -b gone303 "$BGG" >/dev/null 2>&1; rm -rf "$BGG"
BGS4="cockpit-bg303d-$$"; SOCKS="$SOCKS $BGS4"
bg_server "$BGS4" "$BGR"
BGO4=$("$TMUX_REAL" -L "$BGS4" list-panes -a -F '#{pane_id}' | head -1)
bg_goto "$BGS4" "$BGO4" "$BGR" "$BGG"
case "$BG_ERR" in
    *"is gone"*) ok "a listed worktree whose directory is gone is refused, by name" ;;
    *) bad "a listed worktree whose directory is gone is refused, by name" "is gone" "$BG_ERR" ;;
esac
is "and opens no window" "1" "$(bg_windows "$BGS4")"

# #[ in a directory name: fesc keeps it, and a sibling that differs by the doubling is not it.
BGX="$BGD/a#[b]c"; mkdir -p "$BGD/a##[b]c"
git -C "$BGR" worktree add -q -b brk303 "$BGX" >/dev/null 2>&1
bg_goto "$BGS4" "$BGO4" "$BGR" "$BGX"
is "a worktree named a#[b]c opens in that directory, not in a##[b]c" "$BGX" \
   "$("$TMUX_REAL" -L "$BGS4" display-message -p -t t: '#{pane_current_path}')"

# git before 2.36 rejects `worktree list -z`: a shim that does the same. The plain porcelain
# is read; a worktree whose path has a newline is cut in two by it and is left out.
BGOLD=$(mktmpd); BGGIT=$(command -v git)
printf '#!/bin/sh\ncase " $* " in *" worktree list "*" -z "*) exit 129 ;; esac\nexec %s "$@"\n' "$BGGIT" > "$BGOLD/git"
chmod +x "$BGOLD/git"
BGP4=$(bg_pane "$BGS4" "$BGW" 'exec sleep 300')
"$TMUX_REAL" -L "$BGS4" select-window -t "$BGO4"
BG_SHIM=$BGOLD bg_goto "$BGS4" "$BGO4" "$BGR" "$BGW"
is "without worktree list -z the worktree is still found, and its pane selected" "$BGP4" "$(bg_active "$BGS4")"
git -C "$BGR" worktree add -q -b nl303 "$BGD/nl"$'\n'"x" >/dev/null 2>&1
BG_SHIM=$BGOLD bg_goto "$BGS4" "$BGO4" "$BGR" "$BGD/nl"
case "$BG_ERR" in
    *"not a worktree"*) ok "and a newline path, cut in two by the plain porcelain, is not taken for a worktree" ;;
    *) bad "and a newline path, cut in two by the plain porcelain, is not taken for a worktree" "not a worktree" "$BG_ERR" ;;
esac

# The two plain new-window paths: the origin pane is gone, and none was given.
BGW3="$BGD/third"; git -C "$BGR" worktree add -q -b third303 "$BGW3" >/dev/null 2>&1
n=$(bg_windows "$BGS4")
bg_goto "$BGS4" "%99" "$BGR" "$BGW3"
is "an origin pane that is gone still gets a window in the worktree" "$((n+1))" "$(bg_windows "$BGS4")"
is "and it starts there" "$BGW3" "$("$TMUX_REAL" -L "$BGS4" display-message -p -t t: '#{pane_current_path}')"
git -C "$BGR" worktree add -q -b fourth303 "$BGD/fourth" >/dev/null 2>&1
bg_goto "$BGS4" "" "$BGR" "$BGD/fourth"
is "with no origin pane at all it opens a window too" "$((n+2))" "$(bg_windows "$BGS4")"
"$TMUX_REAL" -L "$BGS4" kill-server >/dev/null 2>&1

# The deepest worktree holds a pane, whichever order git lists them in. Git lists them by
# path, so a parent always precedes the tree nested in it; a git shim reverses the records.
if command -v python3 >/dev/null 2>&1; then
BGQ=$(mktmpd); BGSH=$(mktmpd); BGGIT=$(command -v git)
git -C "$BGR" worktree add -q -b par303 "$BGQ/par" >/dev/null 2>&1
git -C "$BGR" worktree add -q -b nest303 "$BGQ/par/nest" >/dev/null 2>&1
printf '#!/bin/sh\ncase " $* " in\n *" worktree list "*) %s "$@" | python3 -c %s ;;\n *) exec %s "$@" ;;\nesac\n' \
    "$BGGIT" "'import sys;r=[x for x in sys.stdin.buffer.read().split(b\"\\0\\0\") if x];r.reverse();sys.stdout.buffer.write(b\"\\0\\0\".join(r)+b\"\\0\\0\")'" "$BGGIT" > "$BGSH/git"
chmod +x "$BGSH/git"
is "the shim lists the nested tree before its parent" "$BGQ/par/nest" \
   "$(PATH="$BGSH:$PATH" git -C "$BGR" worktree list --porcelain -z | tr '\0' '\n' | sed -n 's/^worktree //p' | grep "$BGQ" | head -1)"
BGS5="cockpit-bg303e-$$"; SOCKS="$SOCKS $BGS5"
bg_server "$BGS5" "$BGQ/par/nest"
BGO5=$("$TMUX_REAL" -L "$BGS5" list-panes -a -F '#{pane_id}' | head -1)
BG_SHIM=$BGSH bg_goto "$BGS5" "$BGO5" "$BGR" "$BGQ/par"
is "a pane in a tree nested in a linked worktree is not a pane of it, nested listed first" "2" "$(bg_windows "$BGS5")"
"$TMUX_REAL" -L "$BGS5" kill-server >/dev/null 2>&1
else skip_ "no python3 for the git shim"; skip_ "nested-first order"; fi

# Across sessions: the point of the feature. A control-mode client sits on s1; the
# worktree's panes are in s2, in a window that has a second pane and is not current.
# d0 is the first session ($0), which is what the script's $TMUX names: a new-window
# that is not pinned to the origin pane lands there, not in s1.
BGW2="$BGD/second"; git -C "$BGR" worktree add -q -b second303 "$BGW2" >/dev/null 2>&1
BGS6="cockpit-bg303f-$$"; SOCKS="$SOCKS $BGS6"
tx() { "$TMUX_REAL" -L "$BGS6" "$@"; }
tx -f /dev/null new-session -d -s d0 -x 100 -y 30 -c "$BGD" 'exec sleep 300' >/dev/null 2>&1
tx new-session -d -s s1 -x 100 -y 30 -c "$BGR" 'exec sleep 300' >/dev/null 2>&1
tx new-session -d -s s2 -x 100 -y 30 -c "$BGD" 'exec sleep 300' >/dev/null 2>&1
BGO6=$(tx list-panes -t s1 -F '#{pane_id}')
BGT1=$(tx new-window -P -F '#{pane_id}' -t s2: -c "$(fesc "$BGW")" 'exec sleep 300')
BGT2=$(tx split-window -P -F '#{pane_id}' -t "$BGT1" -c "$(fesc "$BGW")" 'exec sleep 300')
BGTW=$(tx display-message -p -t "$BGT1" '#{window_id}')
tx select-window -t s2:0
BGFIFO=$(mktmpd)/in; mkfifo "$BGFIFO"
tx -C attach -t s1 < "$BGFIFO" > /dev/null 2>&1 &
exec 8> "$BGFIFO"
until_out '?*' tx list-clients -F '#{client_name}'
bg_goto "$BGS6" "$BGO6" "$BGR" "$BGW"
is "the client attached to s1 is switched to the worktree's session" "s2" "$(tx list-clients -F '#{client_session}')"
is "and the window is the one holding the pane" "$BGTW" "$(tx display-message -p -t s2: '#{window_id}')"
is "and the pane is the first one in the tree, though the other was active" "$BGT1" "$(tx display-message -p -t s2: '#{pane_id}')"
exec 8>&-
until_out '' tx list-clients -F '#{client_name}'
# no client, so only select-window and select-pane can do it
tx select-window -t s2:0; tx select-pane -t "$BGT2"
bg_goto "$BGS6" "$BGO6" "$BGR" "$BGW"
is "with no client, the window is selected" "$BGTW" "$(tx display-message -p -t s2: '#{window_id}')"
is "and so is the pane" "$BGT1" "$(tx display-message -p -t s2: '#{pane_id}')"
bg_goto "$BGS6" "$BGO6" "$BGR" "$BGW2"
is "a new window lands in the session of the pane the popup came from" "2" "$(tx list-windows -t s1 | grep -c .)"
is "and not in the session \$TMUX names" "1" "$(tx list-windows -t d0 | grep -c .)"
tx kill-server >/dev/null 2>&1
rm -rf "$BGR" "$BGM" "$BGD"
fi

echo "== tm: create NAME in the sessions picker starts in ~/NAME when that is a git repo (#304) =="
# The picker is a stub that answers "new-session -" with NAME; tmux is a stub that logs
# new-session's argv. A pty on stdin makes tm take the picker path.
HR=$(mktmpd); mkdir -p "$HR/home/foo/.git" "$HR/home/plain" "$HR/home/a/b/.git" "$HR/home/#(id)/.git" "$HR/home/my.repo/.git" "$HR/pane" "$HR/.git" "$HR/cwd"
printf 'x' > "$HR/wt" ; mkdir -p "$HR/home/wt"; cp "$HR/wt" "$HR/home/wt/.git"
printf '#!/bin/sh\ncase "$1" in new-session) shift; printf "%%s\\n" "$*" >> "$HR_LOG" ;; list-sessions) echo alpha ;; display-message) case "$*" in *pane_current_path*) echo "$HR_PANE" ;; *) echo 0 ;; esac ;; has-session) exit 1 ;; esac\nexit 0\n' > "$HR/tmux"
printf '#!/bin/sh\nprintf "new-session -\\n" > "$COCKPIT_ACTION"\nprintf "%%s" "$HR_NAME" > "$COCKPIT_ACTION.text"\nexit 0\n' > "$HR/ct"
chmod +x "$HR/tmux" "$HR/ct"
HR_TENV=(-u TMUX)
hr_new() {    # hr_new <typed name> -> the new-session argv tm logged
    : > "$HR/log"
    (cd "$HR/cwd" && env -u TMUX_PANE "${HR_TENV[@]}" PATH="$HR:$PATH" HOME="$HR/home" HR_LOG="$HR/log" HR_NAME="$1" HR_PANE="$HR/pane" \
        COCKPIT_TUI_BIN="$HR/ct" script -qec "$DIR/scripts/tm" /dev/null </dev/null >/dev/null 2>&1)
    cat "$HR/log"
}
hr_c() {       # hr_c <typed name> -> the -c field of new-session, whole
    local l; l=$(hr_new "$1"); printf '%s' "${l##* -c }"
}
hr_is() { is "$1" "$2" "$(hr_c "$3")"; }
hr_is "a git repo at ~/foo is the start dir" "$HR/home/foo" foo
hr_is "a worktree's .git file counts" "$HR/home/wt" wt
hr_is "a dotted repo name matches ~/my.repo (clean turns the dot into _)" "$HR/home/my.repo" my.repo
case $(hr_new my.repo) in *"-s my_repo -c"*) ok "the session is named by the cleaned name" ;; *) bad "the session is named by the cleaned name" "-s my_repo" "$(cat "$HR/log")" ;; esac
hr_is "a plain ~/plain keeps today's dir" "$HR/cwd" plain
hr_is "no ~/nothing keeps today's dir" "$HR/cwd" nothing
# Not one path component: a slash would walk out of ~ ("a/b" is a repo), a dot name is ~ itself.
hr_is "a name with a slash never reaches ~/a/b" "$HR/cwd" a/b
hr_is "'..' names no folder (~/.. is a repo here)" "$HR/cwd" ..
# A name made of format characters: the dir passes through fesc, so # doubles.
hr_is "a hostile name reaches -c with # doubled" "$HR/home/##(id)" '#(id)'
# In a client $TMUX is set and the pane's folder is the default; ~/NAME still wins.
HR_TENV=("TMUX=x,0,1")
hr_is "with TMUX set, ~/foo wins over the pane's folder" "$HR/home/foo" foo
hr_is "with TMUX set, no repo means the pane's folder" "$HR/pane" plain
HR_TENV=(-u TMUX)
rm -rf "$HR"

echo "== a stale cockpit-tui: tm and tmux-branch name the fix, not 'exited 2' (#276) =="
# A binary built before a screen existed prints "unknown screen <name>" and exits 2.
# The stub prints that line; the wrappers must say the binary is old and what to run.
SD=$(mktmpd)
printf '#!/bin/sh\ncase "$1" in list-sessions) echo alpha ;; display-message) printf "%%s\\n" "$*" >> "$SD_LOG" ;; esac\nexit 0\n' > "$SD/tmux"
chmod +x "$SD/tmux"
git -C "$SD" init -q . >/dev/null 2>&1
sd_has() { case $3 in *"$2"*) ok "$1" ;; *) bad "$1" "contains [$2]" "$3" ;; esac; }
sd_stub() {   # sd_stub <exit code> <stderr text>: a cockpit-tui that only fails
    printf '#!/bin/sh\nprintf "%%s\\n" "$SD_ERR" >&2\nexit %s\n' "$1" > "$SD/ct"; chmod +x "$SD/ct"; SD_ERR=$2
}
sd_tm() {     # a pty on stdin so tm takes the picker path
    env -u TMUX PATH="$SD:$PATH" HOME="$SD" SD_ERR="$SD_ERR" SD_LOG="$SD/log" ${SD_POPUP:+TM_POPUP=1} COCKPIT_TUI_BIN="$SD/ct" \
        script -qec "$DIR/scripts/tm -k" /dev/null </dev/null 2>&1
}
sd_br() { (cd "$SD" && env PATH="$SD:$PATH" SD_ERR="$SD_ERR" COCKPIT_TUI_BIN="$SD/ct" "$DIR/scripts/tmux-branch" </dev/null 2>&1); }
SDMSG="cockpit-tui is older than the plugin"
sd_stub 2 $'cockpit-tui: unknown screen sessions\nusage: cockpit-tui <screen>'
sdo=$(sd_tm)
sd_has "tm names the stale binary" "$SDMSG" "$sdo"
sd_has "tm names make tui" "make tui" "$sdo"
sd_has "tm still shows the tui's own stderr" "unknown screen sessions" "$sdo"
case $sdo in *"exited 2"*) bad "tm no longer says 'exited 2'" "no 'exited 2'" "$sdo" ;; *) ok "tm no longer says 'exited 2'" ;; esac
sdo=$(sd_br)
sd_has "tmux-branch names the stale binary" "$SDMSG" "$sdo"
sd_has "tmux-branch names make tui" "make tui" "$sdo"
# One stderr holding the exempt line and a real one: still stale, and both reach the terminal.
sd_stub 2 $'cockpit-tui: warning: something else\ncockpit-tui: unknown screen branch'
sd_has "stale is found among other stderr lines (tm)" "$SDMSG" "$(sd_tm)"
sd_has "stale is found among other stderr lines (tmux-branch)" "$SDMSG" "$(sd_br)"
# Exit 2 alone is not stale: pick exits 2 when stdout is gone.
sd_stub 2 'cockpit-tui: stdout is closed'
sdo=$(sd_tm)
sd_has "plain exit 2 keeps the old message (tm)" "the sessions picker exited 2" "$sdo"
case $sdo in *"$SDMSG"*) bad "plain exit 2 is not called stale (tm)" "no stale text" "$sdo" ;; *) ok "plain exit 2 is not called stale (tm)" ;; esac
sdo=$(sd_br)
sd_has "plain exit 2 shows the tui's stderr (tmux-branch)" "stdout is closed" "$sdo"
case $sdo in *"$SDMSG"*) bad "plain exit 2 is not called stale (tmux-branch)" "no stale text" "$sdo" ;; *) ok "plain exit 2 is not called stale (tmux-branch)" ;; esac
# The words in the middle of another line, or a different exit code, are not the signal.
sd_stub 2 'cockpit-tui: skipped 3 rows: unknown screen'
sdo=$(sd_tm)
case $sdo in *"$SDMSG"*) bad "the phrase mid-line is not stale" "no stale text" "$sdo" ;; *) ok "the phrase mid-line is not stale" ;; esac
sd_stub 1 'cockpit-tui: unknown screen sessions'
sdo=$(sd_tm)
case $sdo in *"$SDMSG"*) bad "exit 1 with the line is not stale" "no stale text" "$sdo" ;; *) ok "exit 1 with the line is not stale" ;; esac
# A popup also gets the message from display-message, through refuse.
: > "$SD/log"; sd_stub 2 'cockpit-tui: unknown screen sessions'; SD_POPUP=1 sd_tm >/dev/null
sd_has "a popup also gets the stale message by display-message" "$SDMSG" "$(cat "$SD/log")"
# The capture file is gone after every exit path.
mkdir "$SD/tmpd"
for sdcase in "2|cockpit-tui: unknown screen branch" "2|cockpit-tui: stdout is closed" "1|oops"; do
    sd_stub "${sdcase%%|*}" "${sdcase#*|}"; TMPDIR=$SD/tmpd sd_br >/dev/null
    is "tmux-branch leaves no capture file (${sdcase%%|*}, ${sdcase#*|})" "" "$(ls -A "$SD/tmpd")"
done
rm -rf "$SD"

echo "== agent-state session: the payload's session_id and transcript_path become pane options (#302) =="
if need_tmux "agent-state session"; then :; else
SSD=$(mktmpd); SSH="$SSD/h"; SSROOT="$SSH/.claude/projects"; mkdir -p "$SSROOT/-p" "$SSROOT/-q/#(touch $SSD/F)"
SSU1=11111111-2222-4333-8444-555555555555; SSU2=aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee
for u in $SSU1 $SSU2; do : > "$SSROOT/-p/$u.jsonl"; done
: > "$SSD/out.jsonl"; : > "$SSROOT/-q/#(touch $SSD/F)/x.jsonl"; ln -s "$SSD/out.jsonl" "$SSROOT/-p/link.jsonl"
ln -s "$SSROOT/-q/#(touch $SSD/F)/x.jsonl" "$SSROOT/-p/viaq.jsonl"
SSTAB=$(printf '%s/-p/t\tab.jsonl' "$SSROOT"); : > "$SSTAB"; SSBS="$SSROOT/-p/b\\s.jsonl"; : > "$SSBS"
SSP=$(tmux new-window -d -P -F '#{pane_id}' 'exec -a claude sleep 600')
ss_opt() { tmux show-option -pqv -t "$SSP" "@cockpit_$1"; }
ss_clear() { tmux set-option -pu -t "$SSP" @cockpit_session; tmux set-option -pu -t "$SSP" @cockpit_transcript; }
ss() {                        # <session_id> <transcript_path> [extra json]  one payload through the hook
    printf '{"session_id":"%s","transcript_path":"%s","hook_event_name":"UserPromptSubmit"%s}\n' "$1" "$2" "${3:-}" \
        | env -u CLAUDE_CONFIG_DIR HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"; }
ss $SSU1 "$SSROOT/-p/$SSU1.jsonl"
is "a valid payload saves the session id" "$SSU1" "$(ss_opt session)"
is "and the transcript path" "$SSROOT/-p/$SSU1.jsonl" "$(ss_opt transcript)"
is "the hook exits 0 and prints nothing" "0:" "$(ss $SSU1 "$SSROOT/-p/$SSU1.jsonl" 2>&1; echo "$?:")"
ss $SSU2 "$SSROOT/-p/$SSU2.jsonl"
is "a second prompt in a new session replaces both" "$SSU2 $SSROOT/-p/$SSU2.jsonl" "$(ss_opt session) $(ss_opt transcript)"
ss $SSU1 "$SSD/gone.jsonl"
is "a new session whose transcript is not valid drops the old transcript (/clear)" "$SSU1 ." "$(ss_opt session) .$(ss_opt transcript)"
ss_clear
ss $SSU1 "$SSD/out.jsonl"
is "a path outside the projects tree is not saved" "" "$(ss_opt transcript)"
ss_clear
ss $SSU1 "$SSROOT/-p/../-p/$SSU1.jsonl"
is "a path with .. is not saved, even when it resolves inside the tree" "" "$(ss_opt transcript)"
ss_clear
ss $SSU1 "$SSROOT/-p/link.jsonl"
is "a symlink out of the tree is not saved" "" "$(ss_opt transcript)"
ss_clear
ss $SSU1 "$SSROOT/-q/#(touch $SSD/F)/x.jsonl"
is "a path with a #(...) component is not saved" "" "$(ss_opt transcript)"
is "and no command in it ran" "none" "$([ -e "$SSD/F" ] && echo ran || echo none)"
ss_clear
ss $SSU1 "$SSROOT/-p/viaq.jsonl"
is "a symlink inside the tree to a path with #(...) is not saved either" "" "$(ss_opt transcript)"
ss_clear
ss $SSU1 "$SSTAB"
is "a path with a control character is not saved" "" "$(ss_opt transcript)"
ss_clear
ss $SSU1 "$SSBS"
is "a path with a backslash is not saved" "" "$(ss_opt transcript)"
ss_clear
ss $SSU1 "$SSROOT/-p"
is "a directory is not saved" "" "$(ss_opt transcript)"
ss_clear
ss $SSU1 "relative/$SSU1.jsonl"
is "a relative path is not saved" "" "$(ss_opt transcript)"
ss_clear
printf '{"transcript_path":"%s"}\n' "$SSROOT/-p/$SSU1.jsonl" | env HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"
is "a payload with no session_id saves no session" "" "$(ss_opt session)"
printf '{"session_id":"%s"}\n' "$SSU1" | env HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"
is "a payload with no transcript_path saves the session alone" "$SSU1 ." "$(ss_opt session) .$(ss_opt transcript)"
ss_clear
for bad in sid-123 "../x" "#(touch $SSD/F)" "${SSU1}0" "${SSU1%?}" ""; do
    ss "$bad" "$SSROOT/-p/$SSU1.jsonl"
    is "session_id '$bad' is not a UUID: not saved" "" "$(ss_opt session)"
done
is "and no command in it ran" "none" "$([ -e "$SSD/F" ] && echo ran || echo none)"
ss_clear
ss $SSU1 "$SSROOT/-p/$SSU1.jsonl" ',"last_assistant_message":"SECRETREPLY","prompt":"SECRETPROMPT"'
is "last_assistant_message and the prompt never reach a tmux option" "0" "$(tmux show-options -p -t "$SSP" | grep -c SECRET)"
ss_clear
SSBIG="$SSD/big.json"; { printf '{"session_id":"%s","transcript_path":"%s","prompt":"' $SSU1 "$SSROOT/-p/$SSU1.jsonl"; head -c 500000 /dev/zero | tr '\0' x; printf '"}\n'; } > "$SSBIG"
cat "$SSBIG" | env HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"
is "a 500 KB prompt after the ids: both still found" "$SSU1 $SSROOT/-p/$SSU1.jsonl" "$(ss_opt session) $(ss_opt transcript)"
ss_unread() { { env HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"; wc -c; } < "$1" | tail -n 1 | tr -d ' '; }
ss_want=$(( $(wc -c < "$SSBIG") - 4096 ))
is "and the hook leaves all but 4096 bytes of it unread" "$ss_want" "$(ss_unread "$SSBIG")"
SSEMO="$SSD/emoji.json"; { printf '{"session_id":"%s","transcript_path":"%s","prompt":"' $SSU1 "$SSROOT/-p/$SSU1.jsonl"; for _ in $(seq 1 20000); do printf '\360\237\230\200\342\202\254\303\251'; done; printf '"}\n'; } > "$SSEMO"
ss_clear
is "a multibyte prompt (UTF-8 locale): the cap is 4096 bytes, not characters" "$(( $(wc -c < "$SSEMO") - 4096 ))" \
   "$(LC_ALL=C.utf8 ss_unread "$SSEMO")"
is "and its ids are found" "$SSU1 $SSROOT/-p/$SSU1.jsonl" "$(ss_opt session) $(ss_opt transcript)"
# The closing quote of transcript_path at byte N: in the cap saved, past it the path is cut.
ss_edge() {                   # <closing-quote byte> <pad char bytes: 1|2|3|4>
    local pre="{\"pad\":\"" mid sfx pad n
    sfx="\",\"session_id\":\"$SSU1\",\"transcript_path\":\"$SSROOT/-p/$SSU1.jsonl"
    n=$(( $1 - ${#pre} - ${#sfx} - 1 ))
    case $2 in 1) mid=x ;; 2) mid=$'\303\251' ;; 3) mid=$'\342\202\254' ;; 4) mid=$'\360\237\230\200' ;; esac
    pad=$(for _ in $(seq 1 $(( n / $2 ))); do printf '%s' "$mid"; done; for _ in $(seq 1 $(( n % $2 ))); do printf x; done)
    printf '%s%s%s"}\n' "$pre" "$pad" "$sfx" | env HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"
}
for w in 1 2 3 4; do
    ss_clear; ss_edge 4096 $w
    is "ids ending with their quote at byte 4096, ${w}-byte chars before: saved" "$SSU1 $SSROOT/-p/$SSU1.jsonl" "$(ss_opt session) $(ss_opt transcript)"
    ss_clear; ss_edge 4100 $w
    is "a transcript_path cut inside by the cap, ${w}-byte chars before: not saved" "$SSU1 ." "$(ss_opt session) .$(ss_opt transcript)"
done
ss_clear
{ printf '{"prompt":"'; head -c 5000 /dev/zero | tr '\0' x; printf '","session_id":"%s","transcript_path":"%s"}\n' $SSU1 "$SSROOT/-p/$SSU1.jsonl"; } \
    | env HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"
is "ids past the 4 KB cap are skipped: nothing is saved" "" "$(ss_opt session)$(ss_opt transcript)"
{ printf '{"session_id":"%s","transcript_path":"%s' $SSU1 "$SSROOT/-p/"; head -c 5000 /dev/zero | tr '\0' x; printf '"}\n'; } \
    | env HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"
is "a transcript_path cut by the cap is not a value: the session alone is saved" "$SSU1 ." "$(ss_opt session) .$(ss_opt transcript)"
ss_clear
SSCFG="$SSD/cfg"; mkdir -p "$SSCFG/projects/-p"; : > "$SSCFG/projects/-p/$SSU2.jsonl"
printf '{"session_id":"%s","transcript_path":"%s"}\n' $SSU2 "$SSCFG/projects/-p/$SSU2.jsonl" \
    | env CLAUDE_CONFIG_DIR="$SSCFG" HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"
is "CLAUDE_CONFIG_DIR's projects dir is the tree when set" "$SSCFG/projects/-p/$SSU2.jsonl" "$(ss_opt transcript)"
ss_clear
printf '{"session_id":"%s","transcript_path":"%s"}\n' $SSU1 "$SSROOT/-p/$SSU1.jsonl" \
    | env CLAUDE_CONFIG_DIR="$SSCFG" HOME="$SSH" "$DIR/scripts/agent-state" session "$SSP"
is "and ~/.claude/projects is then outside it" "" "$(ss_opt transcript)"
ss_clear
tmux set-option -p -t "$SSP" @cockpit_transcript "#(touch $SSD/F)"
is "set-option -p stores #(...) literally, without running it" "#(touch $SSD/F) none" \
   "$(ss_opt transcript) $([ -e "$SSD/F" ] && echo ran || echo none)"
ss_clear
is "the pane's state is untouched" "" "$(ss $SSU1 "$SSROOT/-p/$SSU1.jsonl"; tmux show-option -pqv -t "$SSP" @panestate)"
ss_clear
tmux kill-pane -t "$SSP" >/dev/null 2>&1
rm -rf "$SSD"
fi

echo "== hook wiring: session is saved at SessionStart (not compact) and UserPromptSubmit (#302) =="
sss() { jq -r --arg s "$1" '[.hooks.SessionStart[]
        | select(.hooks[].command | test("agent-state session$"))
        | select((.matcher // "") as $m | $m == "" or ($s | test("^(" + $m + ")$")))] | length' \
        "$DIR/examples/claude-hooks.json"; }
is "SessionStart saves the session on startup, resume, clear and fork (a new session_id)" "1 1 1 1" "$(sss startup) $(sss resume) $(sss clear) $(sss fork)"
is "but not on compact, which keeps its session_id" "0" "$(sss compact)"
is "UserPromptSubmit saves it too" "1" \
   "$(jq '[.hooks.UserPromptSubmit[].hooks[].command | select(test("agent-state session$"))] | length' "$DIR/examples/claude-hooks.json")"

echo "== a name, path or error line never runs as a tmux format (#289) =="
# tmux expands #{...} and #(...) in display-message, run-shell and -T text,
# so a directory, branch or session name like x#(cmd) ran cmd. Every site goes
# through lib/fmt.sh's fesc: the text shows as written and nothing runs.
if need_tmux "format escape"; then :; else
FE="cockpit-fesc-$$"; FEO="cockpit-fesco-$$"; SOCKS="$SOCKS $FE $FEO"; FED=$(mktmpd)
# The payload runs `touch $FEMK`: a path inside a name cannot hold a /.
"$TMUX_REAL" -L "$FE" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$FED" 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FE" set-environment -g FEMK "$FED/pwned" >/dev/null 2>&1
"$TMUX_REAL" -L "$FEO" -f /dev/null new-session -d -s o -x 120 -y 32 "TERM=xterm-256color exec $TMUX_REAL -L $FE attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$FE" list-clients -F '#{client_name}'
FEP=$("$TMUX_REAL" -L "$FE" display-message -p -t t: '#{pane_id}')
FETMUX="$SOCKDIR/$FE,1,0"
fe_msgs() { "$TMUX_REAL" -L "$FE" show-messages 2>/dev/null | grep -v ' command: '; }
fe_ran() { [ -e "$FED/pwned" ] && echo ran || echo none; }
# A positive control first: the payload does run where tmux expands it. tmux
# kills a #() job when the client that expanded it goes, so this run-shell
# holds its client until the job has landed (or 5 s pass).
"$TMUX_REAL" -L "$FE" run-shell "true '#(touch \$FEMK)'; i=0; while [ ! -e '$FED/pwned' ] && [ \$i -lt 50 ]; do sleep 0.1; i=\$((i+1)); done" >/dev/null 2>&1
until_out ran fe_ran; is "control: an unescaped #() in run-shell runs" "ran" "$(fe_ran)"
rm -f "$FED/pwned"

# shellcheck source=../scripts/lib/fmt.sh
. "$DIR/scripts/lib/fmt.sh"
is "fesc doubles # before { and (, keeps #[ and a lone # as tmux reads them" \
    'a##{b}##(c)#[d]##[e]x##' "$(fesc 'a#{b}#(c)#[d]##[e]x#')"
fe_lit='x#(touch $FEMK) #{session_name} ##{y} #[fg=red] # end'
is "and tmux renders the escaped text as written" "$fe_lit" \
    "$("$TMUX_REAL" -L "$FE" display-message -p -t t: "$(fesc "$fe_lit")")"
# The escaped one, holding its client the same way (2 s): nothing may land.
"$TMUX_REAL" -L "$FE" run-shell "printf '%s' '$(fesc 'rs#(touch $FEMK)')'; i=0; while [ ! -e '$FED/pwned' ] && [ \$i -lt 20 ]; do sleep 0.1; i=\$((i+1)); done" >/dev/null 2>&1

# tm: the session name goes through fesc (now from lib/fmt.sh). No space in
# the payload: tm turns one into _.
TMUX=$FETMUX "$DIR/scripts/tm" 'tm#(>$FEMK)' </dev/null >/dev/null 2>&1
until_out '*tm#(>$FEMK)*' "$TMUX_REAL" -L "$FE" list-sessions -F '#{session_name}'
case "$("$TMUX_REAL" -L "$FE" list-sessions -F '#{session_name}')" in
    *'tm#(>$FEMK)'*) ok "tm names a session literally" ;;
    *) bad "tm names a session literally" 'tm#(>$FEMK)' "$("$TMUX_REAL" -L "$FE" list-sessions -F '#{session_name}')" ;;
esac

# review-revdiff: the no-diff message names the repo dir (a worktree is named
# after its branch). A stub revdiff: the message comes before it would run.
FEBIN=$(mktmpd); printf '#!/bin/sh\nexit 0\n' > "$FEBIN/revdiff"; chmod +x "$FEBIN/revdiff"
FER="$FED/x#(touch \$FEMK)"; mkdir -p "$FER"
( cd "$FER" && git init -q -b main . && git -c user.name=t -c user.email=t@e commit -q --allow-empty -m init ) >/dev/null 2>&1
mkdir -p "$FED/rs"
TMUX=$FETMUX PATH="$FEBIN:$PATH" "$DIR/scripts/tmux-review" --loop "$FER" "$FED/rs/comments.md" </dev/null >/dev/null 2>&1
fe_want="cockpit: no uncommitted changes in $FER"
until_out "*no uncommitted changes in $FED/x*" fe_msgs
case "$(fe_msgs)" in
    *"$fe_want"*) ok "review-revdiff's no-diff message shows the dir literally" ;;
    *) bad "review-revdiff's no-diff message shows the dir literally" "$fe_want" "$(fe_msgs | tail -1)" ;;
esac

# review-fleet: the session dir in "could not create" (XDG_STATE_HOME is a file).
FES="$FED/st#(touch \$FEMK)"; : > "$FES"
TMUX=$FETMUX XDG_STATE_HOME=$FES REVIEW_PANE_ID=$FEP "$DIR/scripts/tmux-review" </dev/null >/dev/null 2>&1
until_out "*could not create $FED/st*" fe_msgs
case "$(fe_msgs)" in
    *"cockpit: could not create $FES/"*) ok "review-fleet's could-not-create message shows the path literally" ;;
    *) bad "review-fleet's could-not-create message shows the path literally" "cockpit: could not create $FES/..." "$(fe_msgs | tail -1)" ;;
esac

# Nothing above ran the payload. The run-shell is the one tmux would run, and
# it went first; this control proves the job finished before the check.
"$TMUX_REAL" -L "$FE" run-shell "touch '$FED/after'" >/dev/null 2>&1
until_out yes sh -c "[ -e '$FED/after' ] && echo yes"
is "and no payload ran" "none" "$(fe_ran)"
"$TMUX_REAL" -L "$FEO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$FE" kill-server >/dev/null 2>&1
rm -rf "$FED" "$FEBIN"
fi

echo "== agent-state session: the payload's cwd becomes @cockpit_dir, the tree the board reads (#312) =="
# A claude started in ~ that works in a repo showed "no repo": tmux reports the process cwd,
# which never moves. The hook payload's cwd does. Throwaway HOME, suite-private server.
SCREAL=$(getent passwd "$(id -un)" 2>/dev/null | cut -d: -f6)
if need_tmux "agent-state session cwd"; then :
elif [ -n "${CLAUDE_CONFIG_DIR:-}" ] || [ -z "$SCREAL" ] || [ "${HOME%/}" = "${SCREAL%/}" ]; then
    bad "the cwd block runs on a private HOME with CLAUDE_CONFIG_DIR unset" "HOME!=$SCREAL, unset" \
        "HOME=$HOME CLAUDE_CONFIG_DIR=${CLAUDE_CONFIG_DIR:-}"
else
SCD=$(mktmpd); SCH="$SCD/h"; mkdir -p "$SCH" "$SCD/a" "$SCD/b" "$SCD/w" "$SCD/cut" "$SCD/semi;"
SCU1=11111111-2222-4333-8444-555555555555; SCU2=aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee
SCHASH="$SCD/x#(touch $SCD/F)"; SCTAB=$(printf '%s/t\tab' "$SCD"); SCSOH="$SCD/"$'\001'x
SCDEL="$SCD/"$'\177'x
mkdir -p "$SCHASH" "$SCTAB" "$SCSOH" "$SCDEL" "$SCD/b\\s"; : > "$SCD/file"
SCP=$(tmux new-window -d -P -F '#{pane_id}' 'exec -a claude sleep 600')
sc_dir() { tmux show-option -pqv -t "$SCP" @cockpit_dir; }
sc_clear() { local o; for o in dir cwd session transcript; do tmux set-option -pu -t "$SCP" "@cockpit_$o"; done; }
# Run from $SCD, so the relative row names a directory that exists from where the hook runs.
sc_raw() { ( cd "$SCD" && env -u CLAUDE_CONFIG_DIR HOME="$SCH" "$DIR/scripts/agent-state" session "$SCP" ); }
sc() {                        # <cwd> [session id]  one UserPromptSubmit through the hook
    printf '{"session_id":"%s","cwd":"%s","hook_event_name":"UserPromptSubmit","prompt":"hi"}\n' \
        "${2:-$SCU1}" "$1" | sc_raw; }
scc() {                       # <cwd> <new_cwd>  one CwdChanged
    printf '{"session_id":"%s","cwd":"%s","hook_event_name":"CwdChanged","old_cwd":"/","new_cwd":"%s"}\n' \
        "$SCU1" "$1" "$2" | sc_raw; }
sc "$SCD/a"
is "a cwd that is a directory is saved as @cockpit_dir" "$SCD/a" "$(sc_dir)"
is "the hook exits 0 and prints nothing" "0:" "$(sc "$SCD/a" 2>&1; echo "$?:")"
scc "$SCD/a" "$SCD/b"
is "CwdChanged: new_cwd is the tree" "$SCD/b" "$(sc_dir)"
for row in "gone:$SCD/gone" "a file:$SCD/file" "relative:a" "empty:" "a tab:$SCTAB" \
           "a control char:$SCSOH" "a DEL:$SCDEL" "a backslash:$SCD/b\\s"; do
    sc_clear; tmux set-option -p -t "$SCP" @cockpit_dir "$SCD/w"
    sc "${row#*:}"
    is "cwd ${row%%:*}: not saved, the old @cockpit_dir kept" "$SCD/w" "$(sc_dir)"
done
sc_clear; sc "$SCHASH"
is "a #(...) in the dir is stored as written" "$SCHASH" "$(tmux display-message -p -t "$SCP" '#{@cockpit_dir}')"
is "and no command in it ran" "none" "$([ -e "$SCD/F" ] && echo ran || echo none)"
sc_clear; sc "$SCD/semi;"
is "a dir ending in ; keeps its ; (tmux takes a trailing ; for a separator)" "$SCD/semi;" "$(sc_dir)"
tmux set-option -p -t "$SCP" @cockpit_dir "$SCD/w"; sc "$SCD/semi;"
is "and a prompt again there keeps a hand-set @cockpit_dir: @cockpit_cwd kept its ; too" "$SCD/w" "$(sc_dir)"
sc_clear; tmux set-option -p -t "$SCP" @cockpit_dir "$SCD/w"
printf '{"session_id":"%s","cwd":null,"%s":"x"}\n' "$SCU1" "$SCD/a" | sc_raw
is "a cwd that is not a string is not read" "$SCD/w" "$(sc_dir)"
printf '{"session_id":"%s","cwd":7,"new_cwd":5,"x":"%s"}\n' "$SCU1" "$SCD/a" | sc_raw
is "nor a number in either key" "$SCD/w" "$(sc_dir)"
printf '{"session_id":"%s","cwd":"%s' "$SCU1" "$SCD/cut" | sc_raw
is "a payload that ends inside the cwd (its writer died) is not saved" "$SCD/w" "$(sc_dir)"
# The cap cuts the value right after "$SCD/cut", a real directory: a cut value is not a value.
sc_cut() {                    # <bytes before the cut> <tail>
    local pre='{"pad":"' sfx="\",\"cwd\":\"$SCD/cut" pad
    pad=$(head -c $(( $1 - ${#pre} - ${#sfx} )) /dev/zero | tr '\0' x)
    printf '%s%s%s%s"}\n' "$pre" "$pad" "$sfx" "$2" | sc_raw; }
sc_clear; sc_cut 4095 '"'
is "a cwd whose closing quote is byte 4096 is saved" "$SCD/cut" "$(sc_dir)"
sc_clear; sc_cut 4096 '/deeper"'
is "a cwd cut by the 4 KB cap is not saved, though its prefix is a directory" "" "$(sc_dir)"
sc_clear
printf '{"session_id":"%s",\n"transcript_path":"%s",\n"cwd":"%s"}\n' "$SCU1" "$SCH/t.jsonl" "$SCD/a" | sc_raw
is "a payload over several lines: the cwd after both ids is still read" "$SCD/a" "$(sc_dir)"
sc_clear; sc "$SCD/a"; tmux set-option -p -t "$SCP" @cockpit_dir "$SCD/w"
sc "$SCD/a"
is "a prompt in the same cwd keeps a @cockpit_dir the agent set by hand" "$SCD/w" "$(sc_dir)"
scc "$SCD/a" "$SCD/b"
is "a move takes it over" "$SCD/b" "$(sc_dir)"
tmux set-option -p -t "$SCP" @cockpit_dir "$SCD/w"; sc "$SCD/b" "$SCU2"
is "a new session in the same cwd takes it back" "$SCD/b" "$(sc_dir)"
sc_clear
is "no payload: exit 0, nothing printed, nothing saved" "0:." "$(sc_raw < /dev/null 2>&1; echo "$?:.$(sc_dir)")"
is "no pane: exit 0, nothing printed" "0:" \
   "$(printf '{"cwd":"%s"}\n' "$SCD/a" | env -u TMUX_PANE "$DIR/scripts/agent-state" session 2>&1; echo "$?:")"
is "a pane that is gone: exit 0, nothing printed" "0:" \
   "$(printf '{"cwd":"%s"}\n' "$SCD/a" | "$DIR/scripts/agent-state" session %999999 2>&1; echo "$?:")"
sc_clear; sc "$SCD/a"
is "the pane's state is untouched" "" "$(tmux show-option -pqv -t "$SCP" @panestate)"
is "tmux-review --dir follows the saved tree" "$SCD/a" "$("$DIR/scripts/tmux-review" --dir "$SCP" 2>/dev/null)"
tmux kill-pane -t "$SCP" >/dev/null 2>&1
rm -rf "$SCD"
fi

echo "== hook wiring: CwdChanged saves the cwd (#312) =="
is "CwdChanged runs agent-state session, with no matcher (it takes none)" "1 0" \
   "$(jq -r '[.hooks.CwdChanged[]? | select(.hooks[].command | test("agent-state session$"))]
        | "\(length) \(map(select(has("matcher"))) | length)"' "$DIR/examples/claude-hooks.json")"

echo "== agent-state: @cockpit_subs counts the Stop's background subagents, and the bar's ✦N sums them (#313) =="
# No SubagentStart/Stop counter: #317 found it drifts. Each Stop's background_tasks is the
# registry's own list, so the count is rewritten whole at every Stop. Private server, HOME
# and CLAUDE_CONFIG_DIR away from the owner's.
if need_tmux "subagent count"; then :; else
SB="cockpit-sb-$$"; SOCKS="$SOCKS $SB"; SBCFG="$(mktmpd)/claude"
sb_real=$(getent passwd "$(id -u)" 2>/dev/null | cut -d: -f6)
sb_iso=ok
case "$HOME/" in "$sb_real/"|"$sb_real/.claude/"*) sb_iso="HOME=$HOME" ;; esac
case "$SBCFG/" in "$sb_real/.claude/"*) sb_iso="CLAUDE_CONFIG_DIR=$SBCFG" ;; esac
[ -n "$sb_real" ] && [ -n "$HOME" ] || sb_iso="no home to compare"
is "the block runs away from the owner's home and ~/.claude" ok "$sb_iso"
if [ "$sb_iso" != ok ]; then skip_ "subagent count: the rest, not isolated"; else
"$TMUX_REAL" -L "$SB" -f /dev/null new-session -d -s t -x 80 -y 24 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$SB" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
SBP=$("$TMUX_REAL" -L "$SB" list-panes -t t -F '#{pane_id}')
SBQ=$("$TMUX_REAL" -L "$SB" split-window -d -P -F '#{pane_id}' -t t 'exec -a claude sleep 300')
SBX=$("$TMUX_REAL" -L "$SB" split-window -d -P -F '#{pane_id}' -t t 'exec sleep 300')
sbst() {                      # <state> <payload> [pane]: one hook, its payload on stdin
    printf '%s\n' "$2" | HOME="$HOME" CLAUDE_CONFIG_DIR="$SBCFG" TMUX="$SOCKDIR/$SB,0,0" \
        "$DIR/scripts/agent-state" "$1" "${3:-$SBP}" >/dev/null 2>&1; }
sbn() { "$TMUX_REAL" -L "$SB" show-option -pqv -t "${1:-$SBP}" @cockpit_subs; }
sbo() { "$TMUX_REAL" -L "$SB" display-message -p -t "$SBP" "#{$1}"; }
sbstop() {                    # <tasks json> [assistant text]: a Stop payload
    printf '{"session_id":"s1","hook_event_name":"Stop","last_assistant_message":"%s","background_tasks":[%s],"session_crons":[]}' \
        "${2:-ok}" "$1"; }
SBT='{"id":"a1","type":"subagent","status":"running","description":"lane","agent_type":"general-purpose"}'
SBSH='{"id":"b1","type":"shell","status":"running","description":"npm test"}'
SBTM='{"id":"c1","type":"teammate","status":"running","description":"helper"}'
SBU='{"hook_event_name":"UserPromptSubmit","prompt":"go"}'
sbst running "$SBU"; sbst done "$(sbstop "$SBT,$SBT,$SBT")"
is "a Stop with 3 background subagents writes @cockpit_subs 3" "3" "$(sbn)"
sbst running "$SBU"
is "a report-back UserPromptSubmit leaves it: the others still run" "3" "$(sbn)"
sbst done "$(sbstop "$SBT,$SBSH,$SBT,$SBTM")"
is "only type subagent counts, not a shell or a teammate" "2" "$(sbn)"
sbst done "$(sbstop '{"id":"f1","type":"shell","description":"subagent"},'"$SBT")"
is "a description that reads subagent is not a type" "1" "$(sbn)"
sbst done "$(sbstop "")"
is "an empty array clears it" "" "$(sbn)"
sbst done "$(sbstop "$SBT")"; sbst done '{"hook_event_name":"Stop","stop_hook_active":false}'
is "a Stop with no background_tasks clears it" "" "$(sbn)"
sbst done "$(sbstop "" 'say \"type\":\"subagent\" twice: \"type\": \"subagent\"')"
is "subagent in the message text is not counted" "" "$(sbn)"
sbst done '{"hook_event_name":"Stop","background_tasks":['"$SBSH"'],"later":['"$SBT"']}'
is "nor a subagent object in a key after the array" "" "$(sbn)"
sbst done "$(sbstop '{"id":"d1","type":"shell","description":"a ] b \"]\""},'"$SBT")"
is "a ] inside a string does not end the array" "1" "$(sbn)"
sbst done "$(sbstop '{"id":"e1","type": "subagent","description":"x \"type\":\"subagent\""}')"
is "a space after the colon counts; an escaped one in a string does not" "1" "$(sbn)"
SB12=$(for _ in $(seq 1 12); do printf '%s,' "$SBT"; done); sbst done "$(sbstop "${SB12%,}")"
is "12 subagents: 12" "12" "$(sbn)"
SBBIG=$(head -c 1048576 /dev/zero | tr '\0' x); sbst done "$(sbstop "$SBT,$SBT" "$SBBIG")"
is "a 1 MB final message before the array: still 2" "2" "$(sbn)"
"$TMUX_REAL" -L "$SB" set -p -t "$SBP" @panestate needs-input
sbst done "$(sbstop "$SBT,$SBT,$SBT,$SBT")"
is "a needs-input pane keeps its state" "needs-input" "$(sbo @panestate)"
is "and still takes the count" "4" "$(sbn)"
sbst running "$SBU"
sbst notify '{"hook_event_name":"Notification","notification_type":"idle_prompt","message":"waiting for your input"}'
is "the idle notification goes off" "" "$(sbo @panestate)"
is "and leaves the count: subagents run while the lead idles" "4" "$(sbn)"
printf '%s\n' "$(sbstop "")" | HOME="$HOME" TMUX="$SOCKDIR/$SB,0,0" "$DIR/scripts/agent-state" poll done "$SBP" >/dev/null 2>&1
is "the poller's done writes no count" "4" "$(sbn)"
sbst off '{"hook_event_name":"SessionStart","source":"startup"}'
is "SessionStart (off) clears it: a new process runs no subagent" "" "$(sbn)"
sbst done "$(sbstop "$SBT,$SBT")"; sbst off '{"hook_event_name":"SessionEnd","reason":"exit"}'
is "SessionEnd clears it" "" "$(sbn)"
# The bar: ✦N, the sum over agent panes, last in @cockpit_fleet; a non-agent pane is not one.
sbst done "$(sbstop "$SBT,$SBT,$SBT")"; sbst done "$(sbstop "$SBT,$SBT")" "$SBQ"
"$TMUX_REAL" -L "$SB" set -p -t "$SBX" @cockpit_subs 7
sbst done "$(sbstop "$SBT,$SBT,$SBT")"
sbf() { "$TMUX_REAL" -L "$SB" display-message -p -t "$SBP" '#{E:@cockpit_fleet}' | plain; }
is "the fleet string ends with ✦5 for 3 and 2 running" "✦5 " "$(sbf)"
"$TMUX_REAL" -L "$SB" set -p -t "$SBP" @panestate needs-input; sbst recount '' "$SBP"
is "after the other marks: ▲1 then ✦5" "▲1 ✦5 " "$(sbf)"
# The bar's own touch test, from the file the plugin sources.
"$TMUX_REAL" -L "$SB" set -g @cockpit_is_touch \
    "$(sed -n 's/^set -g @cockpit_is_touch "\(.*\)"$/\1/p' "$DIR/scripts/lib/statusbar.conf")"
"$TMUX_REAL" -L "$SB" set -g @cockpit_touch on
is "touch mode hides it" "▲1 " "$(sbf)"
"$TMUX_REAL" -L "$SB" set -g @cockpit_touch off
is "and shows it again out of touch mode" "▲1 ✦5 " "$(sbf)"
sbst done "$(sbstop "$SBT")"
is "a Stop that holds the pane still refreshes the sum (1 and 2)" "▲1 ✦3 " "$(sbf)"
sbst done "$(sbstop "")" "$SBQ"
is "a Stop that ends one pane's subagents takes them off" "▲1 ✦1 " "$(sbf)"
sbst done "$(sbstop "")"
is "a sticky needs-input pane's last Stop takes the ✦ away" "▲1 " "$(sbf)"
sbst done "$(sbstop "$SBT,$SBT")" "$SBQ"; "$TMUX_REAL" -L "$SB" kill-pane -t "$SBQ"
TMUX="$SOCKDIR/$SB,0,0" "$DIR/scripts/agent-state" sweep >/dev/null 2>&1
is "a killed pane's subagents leave the sum at the sweep" "▲1 " "$(sbf)"
# Compare-then-set: every set redraws. A server hook sees the writes inside the one call.
sbst done "$(sbstop "$SBT,$SBT")"; SBSETS=$(mktmpf)
"$TMUX_REAL" -L "$SB" set-hook -g after-set-option "run-shell 'echo \"#{hook_arguments}\" >> $SBSETS'"
sbst done "$(sbstop "$SBT,$SBT")"
is "an unchanged count is not set again" "0" "$(grep -c '@cockpit_subs' "$SBSETS")"
sbst done "$(sbstop "$SBT,$SBT,$SBT")"
is "a changed one is, once" "1" "$(grep -c '@cockpit_subs 3' "$SBSETS")"
"$TMUX_REAL" -L "$SB" set-hook -gu after-set-option
# A bound, not a clock: CPU time. The count was a regex pass per subagent, 24 s at 3000.
SB3K=$(mktmpf); { printf '{"hook_event_name":"Stop","background_tasks":['
    for _ in $(seq 1 2999); do printf '%s,' "$SBT"; done; printf '%s]}\n' "$SBT"; } > "$SB3K"
cpu_under "3000 subagents are counted in under 1 s of CPU" 1 \
    env TMUX="$SOCKDIR/$SB,0,0" bash -c '"$1" done "$2" < "$3"' _ "$DIR/scripts/agent-state" "$SBP" "$SB3K"
is "and every one of them counts" "3000" "$(sbn)"
rm -f "$SBSETS" "$SB3K"
"$TMUX_REAL" -L "$SB" kill-server >/dev/null 2>&1
fi
fi

echo "== tmux-review: review_base falls back to origin/HEAD when the repo has no main or master =="
# for-each-ref rows were split on a tab IFS, which folds the empty upstream and
# track fields of origin/HEAD's row, so its symref target landed in the wrong
# variable and the fallback never fired. A clone whose default branch is
# "develop" (no local main) shows it: the topic commit must be the review.
ROH=$(mktmpd)
(
  cd "$ROH" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q --bare -b develop remote.git
  git clone -q remote.git work
  cd work || exit
  printf 'a\n' > f; git add -A; git commit -q -m init; git push -q origin develop
  git remote set-head origin develop
  git checkout -q -b topic; printf 'a\nON-TOPIC\n' > f; git commit -q -am topic
) >/dev/null 2>&1
is "origin/HEAD is the base: a branch commit is a review row" "1" \
   "$("$DIR/scripts/tmux-review" --lines "$ROH/work" | grep -c 'ON-TOPIC')"
is "and the base names origin/develop" "vs origin/develop" \
   "$(. "$DIR/scripts/lib/review-diff.sh"; review_base "$ROH/work" | cut -f2)"
# origin/HEAD outranks a local main: main one commit past origin/develop, topic cut from main.
(
  cd "$ROH/work" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git checkout -q develop; git checkout -q -b main; printf 'a\nM\n' > f; git commit -q -am main-moved
  git checkout -q -B topic; printf 'a\nM\nT\n' > f; git commit -q -am topic2
) >/dev/null 2>&1
ROHB=$(. "$DIR/scripts/lib/review-diff.sh"; review_base "$ROH/work")
is "origin/HEAD outranks a local main: the label" "vs origin/develop" "$(printf '%s' "$ROHB" | cut -f2)"
is "and the base is the merge-base with origin/develop, not main" \
   "$(git -C "$ROH/work" rev-parse origin/develop)" "$(printf '%s' "$ROHB" | cut -f1)"
rm -rf "$ROH"

echo "== git.sh refusal is shown by tmux-review (#330) =="
# Every tmux call goes through a shim onto a private socket: never the user's server.
if need_tmux "git.sh refusal in tmux-review"; then :; else
RF="cockpit-rf-$$"; SOCKS="$SOCKS $RF"; GD=$(mktmpd); RFS=$(mktmpd)
( git init -q "$GD/r" && cd "$GD/r" && printf 'hi\n' > x && printf 'x filter=a=b\n' > .gitattributes \
  && git config 'filter.a=b.clean' "touch $GD/PWNED" ) >/dev/null 2>&1
( git init -q -b main "$GD/ok" && cd "$GD/ok" && printf 'a\n' > y && git add y \
  && git -c user.name=t -c user.email=t@t commit -qm i && printf 'a\nb\n' > y ) >/dev/null 2>&1
"$TMUX_REAL" -L "$RF" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$GD/r" 'exec -a claude bash' >/dev/null 2>&1
RFP=$("$TMUX_REAL" -L "$RF" list-panes -t t: -F '#{pane_id}')
RFSESS="$RFS/tmux-cockpit/review/${RFP#%}-$(git -C "$GD/r" rev-parse --show-toplevel | cksum | cut -d' ' -f1)"
mkdir -p "$GD/bin"
printf '#!/bin/sh\ntouch "%s/revdiff-ran"\n' "$GD" > "$GD/bin/revdiff"
cat > "$GD/bin/tmux" <<SHIMEOF
#!/bin/sh
case "\$1" in
    display-popup) for a; do :; done; sh -c "\$a" >/dev/null 2>&1; exit 0 ;;
    display-message) for a; do :; done; case "\$a" in *session_attached*) echo 1; exit 0 ;; esac; printf '%s\\n' "\$a" >> "$GD/msgs" ;;
esac
exec "$TMUX_REAL" -L "$RF" "\$@"
SHIMEOF
chmod +x "$GD/bin/revdiff" "$GD/bin/tmux"
rf() { env -u TMUX PATH="$GD/bin:$PATH" XDG_STATE_HOME="$RFS" "$@"; }
RFL="cockpit: refusing this repo: a git filter driver name contains '=' (see #331)"
# Control: the plant fires when git runs unguarded, so the absences below mean something.
command git -C "$GD/r" add -A >/dev/null 2>&1
is "control: unguarded git add fires the plant" "yes" "$([ -e "$GD/PWNED" ] && echo yes)"
command git -C "$GD/r" read-tree --empty; rm -f "$GD/PWNED"
is "--lines: the refusal on stderr, no rows, status 1" "$RFL
|1|" "$(rf "$DIR/scripts/tmux-review" --lines "$GD/r" 2>&1 >"$GD/rows"; printf '|%s|%s' "$?" "$(cat "$GD/rows")")"
mkdir -p "$RFSESS"; : > "$RFSESS/comments.md"
rf "$DIR/scripts/tmux-review" --loop "$GD/r" "$RFSESS/comments.md" >"$GD/loop.out" 2>&1 </dev/null; rfrc=$?
is "--loop: the refusal on stderr, status 125" "$RFL|125" "$(tr '\n' '|' < "$GD/loop.out" | sed 's/|$//')|$rfrc"
is "--loop: revdiff never started" "" "$(ls "$GD/revdiff-ran" 2>/dev/null)"
rf "$DIR/scripts/tmux-review" --request "$RFP" >"$GD/req.out" 2>"$GD/req.err" </dev/null; rfrc=$?
is "--request: the refusal on stderr, status 1, nothing on stdout" "$RFL|1|" "$(cat "$GD/req.err")|$rfrc|$(cat "$GD/req.out")"
is "the plant never ran under any of them" "" "$(ls "$GD/PWNED" 2>/dev/null)"
# Negatives: a clean-config repo, a non-repo and a missing dir are not refused.
mkdir "$GD/plain"
is "git_refused: false for a repo, a non-repo and a missing dir" "no no no" \
   "$(bash -c '. "$1/git.sh"; for d in "$2" "$3" "$4"; do git_refused "$d" && printf "yes " || printf "no "; done' _ \
      "$DIR/scripts/lib" "$GD/ok" "$GD/plain" "$GD/missing" | sed 's/ $//')"
rf "$DIR/scripts/tmux-review" --lines "$GD/ok" >"$GD/ok.rows" 2>/dev/null; rfrc=$?
is "--lines on a dirty clean repo: rows, status 0" "y|0" "$(head -1 "$GD/ok.rows" | cut -f1)|$rfrc"
git -C "$GD/ok" checkout -q y; : > "$GD/msgs"
rf "$DIR/scripts/tmux-review" --loop "$GD/ok" "$RFS/ok-store" >"$GD/ok.out" 2>&1 </dev/null; rfrc=$?
is "--loop on a clean repo: still 'no uncommitted changes', status 0" "cockpit: no uncommitted changes in $GD/ok|0" "$(head -1 "$GD/msgs")|$rfrc"
fi
rm -rf "${GD:-/nonexistent-gd}"

echo "== tests/only.sh: a pattern with \\( and ^ selects the block, no match is an error (#241) =="
OD=$(mktemp -d)
printf '%s\n' 'pass=0; fail=0; skipped=0' 'echo "== foo (#216) ^x =="' 'echo ran-foo' \
    'echo "== other =="' 'echo ran-other' > "$OD/fx.sh"   # printf: only.sh would split a heredoc at its echo "== " lines
oo=$(ONLY_RUN="$OD/fx.sh" "$DIR/tests/only.sh" 'foo \(#216\) \^x' 2>&1 </dev/null); oorc=$?
is "only.sh: an escaped-paren, caret title is selected, the other block is not" "ran-foo|0" "$(printf '%s' "$oo" | grep '^ran-' | tr '\n' '|' | sed 's/|$//')|$oorc"
oo=$(ONLY_RUN="$OD/fx.sh" "$DIR/tests/only.sh" 'nomatch \(x\)' 2>&1 </dev/null); oorc=$?
is "only.sh: no match exits 2 naming the pattern" "tests/only.sh: no block matches nomatch \\(x\\)|2" "$oo|$oorc"
oo=$(ONLY_RUN="$OD/fx.sh" "$DIR/tests/only.sh" 'zzzq(' 2>&1 </dev/null); oorc=$?
is "only.sh: an invalid regex says bad pattern, exits 4" "tests/only.sh: bad pattern zzzq(|4" "$(printf '%s' "$oo" | tail -1)|$oorc"
rm -rf "$OD"
# ---- #264 (fzf removal) blocks ----
echo "== tmux-open-path (#264): several candidates go through cockpit-tui pick =="
# A real pty and the real cockpit-tui, on a private socket: the picker draws, a
# key is pressed, and the editor shim's log is the only thing the asserts read.
if need_tmux "open-at-line via pick"; then :; else
OL="cockpit-ol-$$"; SOCKS="$SOCKS $OL"
OLD=$(mktmpd); OLLOG="$OLD/log"; OLED="$OLD/ed"; : > "$OLLOG"
mkdir -p "$OLD/w/sub"; : > "$OLD/w/one.txt"; : > "$OLD/w/two.txt"; : > "$OLD/w/three.txt"
printf '#!/bin/sh\necho "OPEN $*" >> %s\n' "$OLLOG" > "$OLED"; chmod +x "$OLED"
"$TMUX_REAL" -L "$OL" -f /dev/null new-session -d -s t -x 100 -y 28 -c "$OLD/w" >/dev/null 2>&1
"$TMUX_REAL" -L "$OL" set -g @cockpit_editor "$OLED" >/dev/null 2>&1
OLP=$("$TMUX_REAL" -L "$OL" list-panes -t t -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$OL" set-option -p -t "$OLP" @cockpit_click_line \
    "one.txt two.txt:7 sub/ three.txt" >/dev/null 2>&1
olrun() {                     # olrun <n> -- start the picker; wait for its rows
    "$TMUX_REAL" -L "$OL" send-keys -t "$OLP" "clear; '$DIR/scripts/tmux-open-path' --pick '$OLP'; echo OLDONE$1=\$?" Enter
    until_cap "$OL" "$OLP" '*three.txt*'
}
oldone() { until_cap "$OL" "$OLP" "*OLDONE$1=[0-9]*"; }
olcap() { "$TMUX_REAL" -L "$OL" capture-pane -p -J -t "$OLP"; }
olkey() { "$TMUX_REAL" -L "$OL" send-keys -t "$OLP" "$@"; }

olrun 1
case "$(olcap)" in
    *"two.txt:7"*"sub/"*) ok "the list shows each candidate relative to the pane, with its :line" ;;
    *) bad "the list shows each candidate relative to the pane, with its :line" \
         "two.txt:7 then sub/" "$(olcap | head -8)" ;;
esac
olkey Down Enter; oldone 1
is "down + enter opens the second candidate at its line" "OPEN +7 $OLD/w/two.txt" "$(cat "$OLLOG")"

: > "$OLLOG"; olrun 2
olkey -l three; until_cap "$OL" "$OLP" '*open> three*|*three*'
olkey Enter; oldone 2
is "typing a filter then enter opens the filtered row, not the first" \
   "OPEN $OLD/w/three.txt" "$(cat "$OLLOG")"

: > "$OLLOG"; olrun 3
olkey Down Down Enter; oldone 3
is "a directory candidate is handed to the editor whole" "OPEN $OLD/w/sub/" "$(cat "$OLLOG")"

: > "$OLLOG"; olrun 4
olkey Escape; oldone 4
is "esc opens nothing and exits 0" "0 " "$(olcap | sed -n 's/.*OLDONE4=//p' | tail -1) $(cat "$OLLOG")"
"$TMUX_REAL" -L "$OL" kill-server >/dev/null 2>&1
fi

echo "== tmux-open-path (#264): the key from pick is checked, a missing binary is named =="
# The stub answers with PICK_KEY. Anything that is not an index into the list is
# refused with a message and nothing opens.
optui_setup
OKT=$(mktmpd); OKL="$OKT/log"; : > "$OKL"; mkdir -p "$OKT/w"; : > "$OKT/w/a.txt"; : > "$OKT/w/b.txt"
cat > "$OKT/tmux" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  show-options)
      case " $* " in
          *@cockpit_click_line*) printf '%s\n' "${CLICK_LINE:-}" ;;
          *@cockpit_editor*)     printf '%s\n' "${OPEN_EDITOR:-}" ;;
      esac ;;
  display-message)
      case " $* " in
          *pane_current_path*) printf '%s\n' "${CLICK_DIR:-}" ;;
          *window_id*)         printf '@0\n' ;;
          *)                   printf 'MSG %s\n' "${!#}" >> "$TMUX_LOG" ;;
      esac ;;
  new-window) printf 'NEWWIN %s\n' "${!#}" >> "$TMUX_LOG" ;;
  display-popup) printf 'POPUP\n' >> "$TMUX_LOG" ;;
  load-buffer) IFS= read -r l; printf 'LOADBUF %s\n' "$l" >> "$TMUX_LOG" ;;
esac
exit 0
STUB
chmod +x "$OKT/tmux"; ln -s "$(command -v bash)" "$OKT/bash"
for t in mktemp rm cat; do ln -s "$(command -v $t)" "$OKT/$t"; done   # the stderr capture
okrun() {                     # okrun <key> [bin] -- the log after a pick answering <key>
    : > "$OKL"
    env -i PATH="$OKT" HOME="$OKT" TMUX_LOG="$OKL" CLICK_DIR="$OKT/w" CLICK_LINE="a.txt b.txt" \
        OPEN_EDITOR=ed PICK_KEY="$1" PICK_RC="${PICK_RC:-0}" COCKPIT_TUI_BIN="${2:-$OPTUI}" \
        bash "$DIR/scripts/tmux-open-path" ${OKARG---pick} %1 </dev/null >/dev/null 2>"$OKT/err"
    echo "rc=$?"; cat "$OKL"
}
is "key 1 opens the second candidate" "rc=0"$'\n'"NEWWIN ed  $OKT/w/b.txt" "$(okrun 1)"
for k in 2 99 -1 abc '0;x' ''; do
    case "$(okrun "$k")" in
        *NEWWIN*) bad "key [$k] is not a candidate: nothing opens" "no new window" "opened" ;;
        *"not a candidate"*) ok "key [$k] is not a candidate: nothing opens, and it says so" ;;
        *) bad "key [$k] is not a candidate: nothing opens, and it says so" "a 'not a candidate' message" "$(okrun "$k")" ;;
    esac
done
case "$(okrun 0 "$OKT/no-such-tui")" in
    *NEWWIN*) bad "no cockpit-tui: nothing opens" "no new window" "opened" ;;
    *"cockpit-tui is missing"*) ok "no cockpit-tui: the message names it, nothing opens" ;;
    *) bad "no cockpit-tui: the message names it, nothing opens" "cockpit-tui is missing" "$(okrun 0 "$OKT/no-such-tui")" ;;
esac
# alt-c (pick's exit 10) copies the absolute path and opens nothing.
is "alt-c copies the chosen candidate's path and opens nothing" \
   "rc=0"$'\n'"LOADBUF $OKT/w/b.txt"$'\n'"MSG cockpit: copied ~/w/b.txt" "$(PICK_RC=10 okrun 1)"
case "$(PICK_RC=10 okrun 7)" in
    *LOADBUF*) bad "alt-c on a key that is not a candidate copies nothing" "no load-buffer" "copied" ;;
    *) ok "alt-c on a key that is not a candidate copies nothing" ;;
esac
# Any other exit of 2 or more is not a choice: say so on stderr, exit 1, open nothing.
PICK_RC=2 okrun 0 >/dev/null
is "pick exiting 2 exits 1 with its own line, opening nothing" "rc=1 yes no" \
   "$(PICK_RC=2 okrun 0 | head -1) $(grep -q 'cockpit: pick exited 2' "$OKT/err" && echo yes || echo no) $(PICK_RC=2 okrun 0 | grep -q NEWWIN && echo yes || echo no)"
# A cockpit-tui that predates --copy is stale, not crashed: the plugin says so.
printf '#!/bin/sh\nwhile read -r _; do :; done\necho "cockpit-tui: pick: unknown argument --copy" >&2\nexit 2\n' > "$OKT/old-tui"
chmod +x "$OKT/old-tui"
case "$(okrun 0 "$OKT/old-tui")" in
    "rc=1"*"older than the plugin"*) ok "a cockpit-tui without --copy is called stale, exit 1" ;;
    *) bad "a cockpit-tui without --copy is called stale, exit 1" "rc=1 and the stale message" "$(okrun 0 "$OKT/old-tui")" ;;
esac
# Missing is said before the popup opens, so the user never sees it flash.
case "$(OKARG='' okrun 0 "$OKT/no-such-tui")" in
    *POPUP*) bad "no cockpit-tui: no popup opens to say so" "no display-popup" "a popup" ;;
    *) ok "no cockpit-tui: no popup opens to say so" ;;
esac
is "no pane argument is a usage error (exit 2)" "2" \
   "$(env -i PATH="$OKT" bash "$DIR/scripts/tmux-open-path" </dev/null >/dev/null 2>&1; echo $?)"
is "--pick with no pane is a usage error too" "2" \
   "$(env -i PATH="$OKT" bash "$DIR/scripts/tmux-open-path" --pick </dev/null >/dev/null 2>&1; echo $?)"

echo "== tmux-open-path (#264): a path with #(...), #{...} and #S in it reaches tmux right =="
# tmux expands #() and #{} in a display-message text, a window name, a start
# directory and a run-shell command, and #(cmd) runs cmd; it does NOT expand a
# new-window command. So -n, -c and the other sinks carry ## for a #, and the
# editor's command carries the real name. A shim editor logs the argv it got; the
# window's own fields come from the real server. An argv-logging tmux first on
# PATH records what the script gave tmux (a #() job would run async, #255).
if need_tmux "hostile path escape"; then :; else
HX="cockpit-hx-$$"; SOCKS="$SOCKS $HX"
HXD=$(mktmpd); HXB="$HXD/bin"; HXL="$HXD/argv.log"; HXA="$HXD/editor.log"
mkdir -p "$HXB" "$HXD/w/d#S"
optui_setup
HXED="$HXD/ed"; printf '#!/bin/sh\nfor a in "$@"; do printf "%%s\\n" "$a"; done >> "%s"\n' "$HXA" > "$HXED"; chmod +x "$HXED"
HN='a#(touch${IFS}F)#{pid}#S.txt'; HW="$HXD/w/d#S"; : > "$HW/$HN"
printf '#!/bin/sh\nfor a in "$@"; do printf "%%s\\n" "$a"; done >> "%s"\nexec %s "$@"\n' "$HXL" "$TMUX_REAL" > "$HXB/tmux"
printf '#!/bin/sh\nexit 0\n' > "$HXB/code"; chmod +x "$HXB/tmux" "$HXB/code"
"$TMUX_REAL" -L "$HX" -f /dev/null new-session -d -s t -x 100 -y 28 -c "$HXD/w" >/dev/null 2>&1
"$TMUX_REAL" -L "$HX" set -g remain-on-exit on >/dev/null 2>&1   # the shim editor exits at once
HXP=$("$TMUX_REAL" -L "$HX" list-panes -t t -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$HX" send-keys -t "$HXP" "cd 'd#S'" Enter
until_out "*d#S" "$TMUX_REAL" -L "$HX" display-message -p -t "$HXP" '#{pane_current_path}'
"$TMUX_REAL" -L "$HX" set-option -p -t "$HXP" @cockpit_click_line "see $HN" >/dev/null 2>&1
hxgo() {                      # hxgo <n> -- run the pick re-entry; wait for it
    : > "$HXL"; : > "$HXA"
    "$TMUX_REAL" -L "$HX" send-keys -t "$HXP" "clear; PATH='$HXB':\$PATH COCKPIT_TUI_BIN='$OPTUI' '$DIR/scripts/tmux-open-path' --pick '$HXP'; echo HXDONE$1=\$?" Enter
    until_cap "$HX" "$HXP" "*HXDONE$1=[0-9]*"
}
# The script's own formats (#{window_id}) are legitimate, so only the lines that
# carry the hostile name count; with every doubled ## taken out, any # left is
# live: #(..) and #{..} but also the one-letter #S, which shq does not touch.
# $1 drops the editor command's line, which must carry the real name.
hxlive() { grep -F 'touch' "$HXL" | grep -v "^$HXED " | sed 's/##//g' | grep -c '#'; }
hxseen() { grep -c -F 'touch' "$HXL"; }
"$TMUX_REAL" -L "$HX" set -g @cockpit_editor "$HXED" >/dev/null 2>&1
hxgo 1
is "terminal editor: the name and directory arguments carry no live # format" "0" "$(hxlive)"
is "and the hostile name did reach tmux (the check saw it)" "yes" "$([ "$(hxseen)" -ge 2 ] && echo yes || echo no)"
is "the editor was handed the real name, not a doubled one" "$HW/$HN" "$(cat "$HXA")"
HXW=$("$TMUX_REAL" -L "$HX" list-windows -t t -F '#{window_index}' | tail -n 1)
is "the window is named for the file, literally" "$HN" \
   "$("$TMUX_REAL" -L "$HX" display-message -p -t "t:$HXW" '#{window_name}')"
is "and it started in the folder, literally" "$HW" \
   "$("$TMUX_REAL" -L "$HX" display-message -p -t "t:$HXW" '#{pane_start_path}')"
"$TMUX_REAL" -L "$HX" set -gu @cockpit_editor >/dev/null 2>&1
hxgo 2
is "code(1): the run-shell command and the status message carry no live # format" "0" "$(hxlive)"
is "and the status message spells every # of the name as ##" "1" \
   "$(grep -c -F "cockpit: opened ${HW//#/##}/a##(touch\${IFS}F)##{pid}##S.txt" "$HXL")"
# The server's run-shell reaches code(1) through PATH: it must be the suite's stub.
if until_out 1 grep -cxF -- "$HW/$HN:1:1" "$STATE/code.argv"; then
    ok "and the server ran the suite's stub code, no other"
else bad "and the server ran the suite's stub code, no other" 1 "$(cat "$STATE/code.argv" 2>/dev/null)"; fi
"$TMUX_REAL" -L "$HX" kill-server >/dev/null 2>&1
fi

echo "== git.sh refusal is shown by tmux-branch (#330) =="
# The fixture of the tmux-review block above, on its own private socket and shim.
if need_tmux "git.sh refusal in tmux-branch"; then :; else
BQ="cockpit-bq-$$"; SOCKS="$SOCKS $BQ"; GD=$(mktmpd)
( git init -q "$GD/r" && cd "$GD/r" && printf 'hi\n' > x && printf 'x filter=a=b\n' > .gitattributes \
  && git config 'filter.a=b.clean' "touch $GD/PWNED" ) >/dev/null 2>&1
( git init -q -b main "$GD/ok" && cd "$GD/ok" && printf 'a\n' > y && git add y \
  && git -c user.name=t -c user.email=t@t commit -qm i && git branch other && git worktree add -q "$GD/ok-wt" other ) >/dev/null 2>&1
"$TMUX_REAL" -L "$BQ" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$GD/r" >/dev/null 2>&1
BQSOCK=$("$TMUX_REAL" -L "$BQ" display-message -p '#{socket_path}')
mkdir "$GD/bin"
printf '#!/bin/sh\nexec "%s" -L "%s" "$@"\n' "$TMUX_REAL" "$BQ" > "$GD/bin/tmux"
# A picker stub: records that it ran and answers with the action line in $GD/answer.
printf '#!/bin/sh\ntouch "%s/picker-ran"\ncat "%s/answer" 2>/dev/null\n' "$GD" "$GD" > "$GD/bin/cockpit-tui"
chmod +x "$GD/bin/tmux" "$GD/bin/cockpit-tui"
bq() { env PATH="$GD/bin:$PATH" COCKPIT_TUI_BIN="$GD/bin/cockpit-tui" TMUX="$BQSOCK,0,0" "$DIR/scripts/tmux-branch" "$@" </dev/null 2>&1; }
BQL="cockpit: refusing this repo: a git filter driver name contains '=' (see #331)"
command git -C "$GD/r" add -A >/dev/null 2>&1
is "control: unguarded git add fires the plant" "yes" "$([ -e "$GD/PWNED" ] && echo yes)"
command git -C "$GD/r" read-tree --empty; rm -f "$GD/PWNED"
printf 'goto %s\n' "$GD/r" > "$GD/answer"
is "refused repo: the refusal line, not 'not a git repository'" "$BQL" "$(bq "$GD/r" | head -1)"
is "the picker never started" "" "$(ls "$GD/picker-ran" 2>/dev/null)"
is "no window was opened" "1" "$("$TMUX_REAL" -L "$BQ" list-windows | grep -c .)"
is "the plant never ran" "" "$(ls "$GD/PWNED" 2>/dev/null)"
# Negative: a normal repo reaches the picker and a goto to its worktree is honoured.
printf 'goto %s\n' "$GD/ok-wt" > "$GD/answer"
bq "$GD/ok" >"$GD/ok.out"
is "normal repo: the picker ran, no refusal" "yes|" "$([ -e "$GD/picker-ran" ] && echo yes)|$(grep -c refusing "$GD/ok.out" | sed 's/^0$//')"
is "and its worktree got a window" "$GD/ok-wt" \
   "$("$TMUX_REAL" -L "$BQ" list-panes -a -F '#{pane_current_path}' | grep -Fx "$GD/ok-wt" | head -1)"
"$TMUX_REAL" -L "$BQ" kill-server >/dev/null 2>&1
fi
rm -rf "${GD:-/nonexistent-gd}"

echo "== cockpit-tui in a popup follows its client's light or dark theme (#240) =="
# The inner client's terminal is a pane of the outer server, and that pane's
# background decides what it reports (DSR 996, mode 2031): #f0f0f0 is light,
# #101010 dark. The picker's value role (its 1/2 count) is #1b1e24 in the light
# set and #e6e4df in the dark; the popup's cells reach the outer pane as truecolor.
if need_tmux "popup client theme"; then :; else
CT="cockpit-ct-$$"; CTO="cockpit-cto-$$"; SOCKS="$SOCKS $CT $CTO"; CTD=$(mktmpd)
printf 'alpha\talpha\nbeta\tbeta\n' > "$CTD/rows"
"$TMUX_REAL" -L "$CT" -f /dev/null new-session -d -s t -x 100 -y 30 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$CT" bind -n F5 display-popup -E -w 60 -h 16 "$(printf '%q pick < %q' "$COCKPIT_TUI_BIN" "$CTD/rows")"
"$TMUX_REAL" -L "$CTO" -f /dev/null new-session -d -s o -x 100 -y 32 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$CTO" set -g window-style 'bg=#f0f0f0'
"$TMUX_REAL" -L "$CTO" respawn-pane -k -t o "TERM=xterm-256color exec $TMUX_REAL -L $CT attach -t t" >/dev/null 2>&1
ct_set() {                    # <light|dark> (the client) <theme option>: the set the popup drew
    local bg='#101010'; [ "$1" = light ] && bg='#f0f0f0'
    "$TMUX_REAL" -L "$CTO" set -g window-style "bg=$bg"
    until_out "$1" "$TMUX_REAL" -L "$CT" list-clients -F '#{client_theme}'
    "$TMUX_REAL" -L "$CT" set -s theme "$2"
    "$TMUX_REAL" -L "$CTO" send-keys -t o F5
    until_cap "$CTO" o '*pick>*'
    local got=none i
    for i in $(seq 1 "$POLLS"); do   # 'pick>' can be up a frame before the rows' colours
        case "$("$TMUX_REAL" -L "$CTO" capture-pane -ep -t o)" in
            *'38;2;27;30;36'*) got=light; break ;;
            *'38;2;230;228;223'*) got=dark; break ;;
        esac
        sleep 0.1
    done
    "$TMUX_REAL" -L "$CTO" send-keys -t o Escape
    # A popup still up would hand its colours to the next case: fail this one.
    for i in $(seq 1 "$POLLS"); do
        case "$("$TMUX_REAL" -L "$CTO" capture-pane -p -t o)" in
            *'pick>'*) sleep 0.1 ;;
            *) printf %s "$got"; return ;;
        esac
    done
    printf 'stuck: the popup did not close'
}
until_out '?*' "$TMUX_REAL" -L "$CT" list-clients -F '#{client_name}'
is "a client reported light gets the light set" "light" "$(ct_set light detect)"
is "a client reported dark gets the dark set" "dark" "$(ct_set dark detect)"
is "theme dark overrides a light client" "dark" "$(ct_set light dark)"
is "theme light overrides a dark client" "light" "$(ct_set dark light)"
is "theme terminal keeps the dark set" "dark" "$(ct_set light terminal)"
"$TMUX_REAL" -L "$CTO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$CT" kill-server >/dev/null 2>&1
fi

echo "== a burst of window events runs a few sweeps, and the counts are right after it (#258) =="
# Each hook event ran its own full sweep, O(N²) over a resurrect restore or a
# kill-session. Debounced: the counts must still be right after the burst's last event.
if need_tmux "sweep debounce"; then :; else
DB="cockpit-db-$$"; SOCKS="$SOCKS $DB"; DBD=$(mktmpd)
dbt() { "$TMUX_REAL" -L "$DB" "$@"; }
dbt -f /dev/null new-session -d -s a -c "$DBD" 'exec -a claude sleep 300' >/dev/null 2>&1
dbt set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
for _ in $(seq 1 59); do dbt new-window -d -t a: 'exec -a claude sleep 300' >/dev/null 2>&1; done
dbt new-session -d -s b -c "$DBD" >/dev/null 2>&1
until_out 60 sh -c "'$TMUX_REAL' -L '$DB' list-panes -t a -F '#{pane_current_command}' | grep -c claude" >/dev/null
dbt set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
for dbi in $(seq 0 59); do dbt set -p -t "a:$dbi" @panestate needs-input >/dev/null 2>&1; done
dbt run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
until_out 60 dbt show-option -gqv @cockpit_fleet_nb >/dev/null
# Every full sweep's first batch carries the format stamp; the shim counts those.
mkdir "$DBD/bin"
printf '#!/bin/sh\n[ "$1 $2 $3" = "if-shell -F 1" ] && case "$4" in "if -F %s#{!=:#{@cockpit_fmt_v}"*) echo x >> "%s/swept" ;; esac\nexec "%s" "$@"\n' \
    "'" "$DBD" "$TMUX_REAL" > "$DBD/bin/tmux"
chmod +x "$DBD/bin/tmux"
dbt set-environment -g PATH "$DBD/bin:$PATH" >/dev/null 2>&1
dbsess() { dbt display-message -p -t "$1" '#{@cockpit_sess_nb}'; }
for dbi in $(seq 0 49); do dbt link-window -d -s "a:$dbi" -t b: >/dev/null 2>&1; done
until_out 50 dbsess b: >/dev/null; sleep 0.5
is "50 linked windows: b counts all 50 waiting, the fleet each link" "50|110" "$(dbsess b:)|$(dbt show-option -gqv @cockpit_fleet_nb)"
dbn=$(grep -c . "$DBD/swept" 2>/dev/null)
is "and far fewer sweeps ran than the 50 events" "yes" "$([ "${dbn:-0}" -ge 1 ] && [ "${dbn:-0}" -lt 10 ] && echo yes || echo "no: ${dbn:-0}")"
: > "$DBD/swept"
for dbi in $(seq 0 49); do dbt kill-window -t "a:$dbi" >/dev/null 2>&1; done
until_out 10 dbsess a: >/dev/null; sleep 0.5
is "50 killed windows: a, b and the fleet count what is left" "10|0|10" "$(dbsess a:)|$(dbsess b:)|$(dbt show-option -gqv @cockpit_fleet_nb)"
dbn=$(grep -c . "$DBD/swept" 2>/dev/null)
is "and far fewer sweeps ran than the kills' events" "yes" "$([ "${dbn:-0}" -ge 1 ] && [ "${dbn:-0}" -lt 10 ] && echo yes || echo "no: ${dbn:-0}")"
# One interactive event still updates, after the pause.
dbt kill-window -t a:55 >/dev/null 2>&1
until_out 9 dbsess a: >/dev/null
is "a single kill still updates the count" "9" "$(dbsess a:)"
# The load's sweep, with no seq, is not debounced: it writes at once.
dbt set -p -t a:56 @panestate error >/dev/null 2>&1
TMUX="$SOCKDIR/$DB,0,0" "$DIR/scripts/agent-state" sweep >/dev/null 2>&1
is "a sweep with no seq writes at once" "1" "$(dbt show-option -gqv @cockpit_fleet_ne)"
# A stale seq (a newer event came) walks nothing and writes nothing.
dbt set -p -t a:57 @panestate error >/dev/null 2>&1
dbt set -g @cockpit_sweep_seq 999 >/dev/null 2>&1
TMUX="$SOCKDIR/$DB,0,0" "$DIR/scripts/agent-state" sweep 998 >/dev/null 2>&1
is "a superseded sweep writes nothing" "1" "$(dbt show-option -gqv @cockpit_fleet_ne)"
TMUX="$SOCKDIR/$DB,0,0" "$DIR/scripts/agent-state" sweep 999 >/dev/null 2>&1
is "the newest one does" "2" "$(dbt show-option -gqv @cockpit_fleet_ne)"
dbt kill-server >/dev/null 2>&1
fi

echo "== a key the plugin lets go of gets its stock binding back, letter or not (#258) =="
# give_back knew stock bindings for prefix letters only, so moving the menu off
# Space, ?, a digit or " left that key unbound.
if need_tmux "non-letter give-back"; then :; else
GB="cockpit-gb-$$"; SOCKS="$SOCKS $GB"; GBD=$(mktmpd)
gbt() { "$TMUX_REAL" -L "$GB" "$@"; }
# The reference: a configless server's own prefix table, with notes.
gbstock() { "$TMUX_REAL" -S "$GBD/ref" -f /dev/null start-server \; list-keys -N -T prefix -- "$1" 2>/dev/null; rm -f "$GBD/ref"; }
gbline() { "$TMUX_REAL" -S "$GBD/ref" -f /dev/null start-server \; list-keys -T prefix -- "$1" 2>/dev/null; rm -f "$GBD/ref"; }
gbt -f /dev/null new-session -d -s t -c "$GBD" >/dev/null 2>&1
gbload() { gbt set -g "$1" "$2" >/dev/null 2>&1; gbt run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; }
for gbk in Space '?' 1 '"' '$' ','; do
    gbload @cockpit_menu_key "$gbk"
    case "$(gbt list-keys -T prefix -- "$gbk")" in *display-menu*) ok "the menu takes [$gbk]" ;;
        *) bad "the menu takes [$gbk]" "display-menu" "$(gbt list-keys -T prefix -- "$gbk")" ;; esac
    # A second load on the same key: the stock given back later is still tmux's, not this binding.
    gbload @cockpit_menu_key "$gbk"
    gbload @cockpit_menu_key a
    is "moving the menu off [$gbk] gives its stock binding back" "$(gbline "$gbk")" "$(gbt list-keys -T prefix -- "$gbk")"
    is "with its note" "$(gbstock "$gbk")" "$(gbt list-keys -N -T prefix -- "$gbk")"
done
# A repeatable stock key keeps its -r; the panel key gives back the same way.
gbload @cockpit_panel_key Up
case "$(gbt list-keys -T prefix Up)" in *cockpit-panel*) ok "the panel takes Up" ;;
    *) bad "the panel takes Up" "cockpit-panel" "$(gbt list-keys -T prefix Up)" ;; esac
gbload @cockpit_panel_key off
is "turning the panel off gives Up back, repeat flag and all" "$(gbline Up)" "$(gbt list-keys -T prefix Up)"
# A key stock leaves unbound is unbound again.
gbload @cockpit_menu_key F5
gbload @cockpit_menu_key a
is "a key stock does not bind is unbound again" "" "$(gbt list-keys -T prefix F5 2>/dev/null)"
# A lookup that cannot run (here a stock server that will not start, as a socket
# path past the limit did) leaves the key alone: unbinding it would lose stock.
mkdir "$GBD/bin"
printf '#!/bin/sh\nfor a; do [ "$a" = start-server ] && { echo "error connecting (File name too long)" >&2; exit 1; }; done\nexec "%s" "$@"\n' \
    "$TMUX_REAL" > "$GBD/bin/tmux"
chmod +x "$GBD/bin/tmux"
gbload @cockpit_menu_key Space
gbt set-environment -g PATH "$GBD/bin:$PATH" >/dev/null 2>&1
gbload @cockpit_menu_key a
case "$(gbt list-keys -T prefix Space)" in *display-menu*) ok "a failed stock lookup leaves the old key bound, not unbound" ;;
    *) bad "a failed stock lookup leaves the old key bound, not unbound" "display-menu still on Space" "$(gbt list-keys -T prefix Space 2>&1)" ;; esac
gbt kill-server >/dev/null 2>&1
fi

echo "== cockpit-tui with no client attached keeps the @thm_* roles (#240) =="
# Theme::load reads #{client_theme} in the same tmux call as the roles, so a
# display-message that failed without a client would lose every role, not just
# the light set. A detached server: the value role must still be @thm_fg.
if [ ! -x "$DIR/bin/cockpit-tui" ]; then
    skip_ "no-client theme roles (no bin/cockpit-tui; run make tui)"
elif need_tmux "no-client theme roles"; then :; else
NC="cockpit-nc-$$"; SOCKS="$SOCKS $NC"; NCD=$(mktmpd)
"$TMUX_REAL" -L "$NC" -f /dev/null new-session -d -s base -c "$NCD" 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$NC" set -g @thm_fg '#123456'
"$TMUX_REAL" -L "$NC" set -g @thm_fg_light '#654321'
is "the server has no client" "" "$("$TMUX_REAL" -L "$NC" list-clients)"
pick_pane "$NC" "$NCD" 80 20 "$(printf 'alpha\talpha\nbeta\tbeta\n')"$'\n'
until_cap "$NC" t '*pick>*'
case "$("$TMUX_REAL" -L "$NC" capture-pane -ep -t t)" in
    *'38;2;18;52;86'*) got=roles ;;
    *'38;2;101;67;33'*) got=light ;;
    *) got=lost ;;
esac
is "no client: the @thm_fg role is drawn, in the dark set" "roles" "$got"
"$TMUX_REAL" -L "$NC" kill-server >/dev/null 2>&1
fi

echo "== the suite never reaches a real VS Code (#350) =="
# A real code(1) with this private HOME installs ~/.vscode-server; PATH must not
# lead to one, and the stub is what a bare `code` runs.
is "PATH has no /mnt/<drive> entry" "0" \
   "$(printf '%s\n' "$PATH" | tr : '\n' | grep -c '^/mnt/[a-z]\(/\|$\)')"
is "a bare code is the stub" "$SHIM/code" "$(command -v code)"
is "path_without_mnt keeps a glob entry literal" "x*:/usr/bin" \
   "$(cd "$STATE" && : > xa && : > xb; path_without_mnt "x*:/usr/bin")"
is "and drops every empty entry, wherever it sits" "/usr/bin:/bin" \
   "$(path_without_mnt ":/usr/bin::/bin:")"
is "path_without_mnt drops a drive and keeps the rest" "/usr/bin:/opt/x:/mnt/data" \
   "$(path_without_mnt "/mnt/c/Program Files/bin:/usr/bin:/mnt/d:/opt/x:/mnt/data")"
code --goto /nowhere:1:1 'a b' >/dev/null 2>&1
is "the stub records one argument per line" "--goto|/nowhere:1:1|a b" \
   "$(tail -n 3 "$STATE/code.argv" 2>/dev/null | paste -sd'|')"
is "and no VS Code server was installed into the suite's HOME" "absent" \
   "$([ -e "$HOME/.vscode-server" ] && echo present || echo absent)"

# ---- #283 (markdown viewer) blocks ----
echo "== tmux-open-path (#283): a lone markdown file opens in the viewer, the rest as before =="
# A tmux stub logs what the script asks of tmux, and a cockpit-tui stub answers
# pick with PICK_KEY and view with VIEW_OUT, logging its own argv. No server.
MV=$(mktmpd); MVL="$MV/log"; mkdir -p "$MV/bin" "$MV/w/d.md"
MVB=$'b\xe2\x80\xaeq\x1b[2Jz.md'   # a bidi override and an escape in the name
for f in a.md B.MD c.markdown e.mdown g.mkd n.md.txt t.txt 'h#(x)#{y}.md' "$MVB"; do : > "$MV/w/$f"; done
cat > "$MV/bin/tmux" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  show-options|show-option)
      case " $* " in
          *@cockpit_click_line*) printf '%s\n' "${CLICK_LINE:-}" ;;
          *@cockpit_editor*)     printf '%s\n' "${OPEN_EDITOR:-}" ;;
          *@cockpit_md_view*)    printf '%s\n' "${MD_VIEW:-}" ;;
      esac ;;
  display-message)
      case " $* " in
          *pane_current_path*) printf '%s\n' "${CLICK_DIR:-}" ;;
          *window_id*)         printf '@0\n' ;;
          *pane_width*)        printf '90 30\n' ;;
          *)                   printf 'MSG %s\n' "${!#}" >> "$TMUX_LOG" ;;
      esac ;;
  new-window) printf 'NEWWIN %s\n' "${!#}" >> "$TMUX_LOG" ;;
  display-popup) shift; { printf 'POPUP'; printf ' [%s]' "$@"; echo; } >> "$TMUX_LOG"; exit "${POPUP_RC:-0}" ;;
  list-clients) [ -z "${NO_CLIENT:-}" ] && printf 'c\n' ;;
esac
exit 0
STUB
cat > "$MV/bin/cockpit-tui" <<'STUB'
#!/bin/sh
if [ "$1" = view ]; then
    { printf 'VIEW'; for a in "$@"; do printf ' [%s]' "$a"; done; echo; } >> "$TMUX_LOG"
    [ -z "${VIEW_ERR:-}" ] || { printf '%s\n' "$VIEW_ERR" >&2; exit "${VIEW_RC:-2}"; }
    printf '%s\n' "${VIEW_OUT-}"; exit "${VIEW_RC:-0}"
fi
while read -r _; do :; done; printf '%s\n' "${PICK_KEY-0}"
STUB
chmod +x "$MV/bin/tmux" "$MV/bin/cockpit-tui"; ln -s "$(command -v bash)" "$MV/bin/bash"
for t in mktemp rm cat; do ln -s "$(command -v $t)" "$MV/bin/$t"; done   # the stderr capture
mvrun() {                     # mvrun <click line> <args...> -- the log after one run
    : > "$MVL"
    env -i PATH="$MV/bin" HOME="$MV" TMUX_LOG="$MVL" CLICK_DIR="$MV/w" CLICK_LINE="$1" OPEN_EDITOR=ed \
        COCKPIT_TUI_BIN="${MV_BIN:-$MV/bin/cockpit-tui}" MD_VIEW="${MD_VIEW:-}" VIEW_OUT="${VIEW_OUT-}" \
        VIEW_ERR="${VIEW_ERR:-}" VIEW_RC="${VIEW_RC:-}" PICK_KEY="${PICK_KEY:-0}" POPUP_RC="${POPUP_RC:-}" NO_CLIENT="${NO_CLIENT:-}" \
        bash "$DIR/scripts/tmux-open-path" "${@:2}" </dev/null >/dev/null 2>"$MV/err"
    cat "$MVL"
}
is "a lone .md opens a popup the pane's size over it, which re-enters with --view" \
   "POPUP [-t] [%1] [-E] [-x] [P] [-y] [P] [-w] [90] [-h] [30] [-d] [$MV/w] [-T] [ a.md ] [$(printf %q "$DIR/scripts/tmux-open-path") --view %1 $MV/w/a.md '']" \
   "$(mvrun 'see a.md' %1)"
for f in B.MD c.markdown e.mdown g.mkd 'a.md:12'; do
    case "$(mvrun "$f" %1)" in
        POPUP*) ok "$f goes to the viewer" ;;
        *) bad "$f goes to the viewer" "a popup" "$(mvrun "$f" %1)" ;;
    esac
done
case "$(mvrun 'a.md:12' %1)" in
    *"--view %1 $MV/w/a.md 12]") ok "and a clicked :line goes with it" ;;
    *) bad "and a clicked :line goes with it" "--view %1 .../a.md 12" "$(mvrun 'a.md:12' %1)" ;;
esac
for f in n.md.txt t.txt d.md; do
    is "$f is not markdown: the editor, at once" "NEWWIN ed  $MV/w/$f" "$(mvrun "$f" %1)"
done
is "@cockpit_md_view off: a .md goes straight to the editor" "NEWWIN ed  $MV/w/a.md" "$(MD_VIEW=off mvrun a.md %1)"
is "no client on the pane's session: the editor, and no popup" "NEWWIN ed  $MV/w/a.md" "$(NO_CLIENT=1 mvrun a.md %1)"
# tmux hands back the popup command's status: 129 is the client detaching
# mid-view, 3 any exit 3. Neither is a refusal, so no editor opens behind them.
for rc in 129 3; do
    case "$(POPUP_RC=$rc mvrun a.md %1)" in
        POPUP*NEWWIN*) bad "a popup closing with status $rc opens nothing more" "the popup only" "the editor too" ;;
        POPUP*) ok "a popup closing with status $rc opens nothing more" ;;
        *) bad "a popup closing with status $rc opens nothing more" "the popup only" "$(POPUP_RC=$rc mvrun a.md %1)" ;;
    esac
done
is "no cockpit-tui: a .md goes straight to the editor" "NEWWIN ed  $MV/w/a.md" "$(MV_BIN="$MV/none" mvrun a.md %1)"
is "the popup's title and directory spell each # as ##" "[-d] [$MV/w] [-T] [ h##(x)##{y}.md ]" \
   "$(mvrun 'h#(x)#{y}.md' %1 | grep -o '\[-d\] \[[^]]*\] \[-T\] \[[^]]*\]')"
is "the popup's title drops a bidi override and control characters" "[-T] [ bq[2Jz.md ]" \
   "$(mvrun "$MVB" %1 | grep -o '\[-T\] \[[^]]*\]')"
# --view is the popup's re-entry: the viewer runs in it, and e comes back here.
is "--view runs the viewer at the line and opens the editor where e left it" \
   "VIEW [view] [--line] [7] [--] [$MV/w/a.md]"$'\n'"NEWWIN ed +12 $MV/w/a.md" \
   "$(VIEW_OUT='edit 12' mvrun '' --view %1 "$MV/w/a.md" 7)"
is "--view without a line passes none" "VIEW [view] [--] [$MV/w/a.md]" "$(mvrun '' --view %1 "$MV/w/a.md" '')"
for o in 'edit 3; touch x' 'edit' 'edit x' 'open 3' 'edit 3 4'; do
    case "$(VIEW_OUT="$o" mvrun '' --view %1 "$MV/w/a.md" '')" in
        *NEWWIN*) bad "the viewer's [$o] is not an edit: nothing opens" "no new window" "opened" ;;
        *) ok "the viewer's [$o] is not an edit: nothing opens" ;;
    esac
done
case "$(VIEW_ERR='cockpit-tui: unknown screen view' mvrun '' --view %1 "$MV/w/a.md" '')" in
    *"NEWWIN ed  $MV/w/a.md"*"older than the plugin"*|*"older than the plugin"*"NEWWIN ed  $MV/w/a.md"*)
        ok "a cockpit-tui without view is called stale, and the editor opens" ;;
    *) bad "a cockpit-tui without view is called stale, and the editor opens" "the stale note and the editor" \
         "$(VIEW_ERR='cockpit-tui: unknown screen view' mvrun '' --view %1 "$MV/w/a.md" '')" ;;
esac
case "$(VIEW_ERR=boom VIEW_RC=3 mvrun '' --view %1 "$MV/w/a.md" '')" in
    *"viewer exited 3"*"NEWWIN ed  $MV/w/a.md"*|*"NEWWIN ed  $MV/w/a.md"*"viewer exited 3"*)
        ok "a viewer that fails says so, and the editor opens" ;;
    *) bad "a viewer that fails says so, and the editor opens" "the note and the editor" \
         "$(VIEW_ERR=boom VIEW_RC=3 mvrun '' --view %1 "$MV/w/a.md" '')" ;;
esac
# From the picker's popup the viewer draws in place: tmux has one popup per client.
is "a .md chosen in the picker opens the viewer in the picker's own popup" \
   "VIEW [view] [--] [$MV/w/a.md]"$'\n'"NEWWIN ed +5 $MV/w/a.md" "$(VIEW_OUT='edit 5' PICK_KEY=0 mvrun 'a.md t.txt' --pick %1)"

echo "== tmux-open-path (#283): the viewer in a real popup, e to the editor, a hostile name runs nothing =="
# The real binary in a real popup: an outer server's pane is the inner server's
# client, keys go in through it, and an editor shim's log is what the asserts read.
if need_tmux "markdown viewer popup"; then :; else
MDI="cockpit-mdi-$$"; MDO="cockpit-mdo-$$"; SOCKS="$SOCKS $MDI $MDO"   # not the move menu's mvo
MVD=$(mktmpd); mkdir -p "$MVD/w"; MVE="$MVD/ed.log"; : > "$MVE"
printf '#!/bin/sh\necho "OPEN $*" >> %s\n' "$MVE" > "$MVD/ed"; chmod +x "$MVD/ed"
MVN='r#(tmux${IFS}set${IFS}-g${IFS}@pwned${IFS}yes).md'   # no spaces: the line splits on them
{ printf '# Heading\n\n'; for i in $(seq 0 40); do printf 'p%s\n\n' "$i"; done; } > "$MVD/w/$MVN"
"$TMUX_REAL" -L "$MDI" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$MVD/w" 'exec sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$MDI" set -g @cockpit_editor "$MVD/ed" >/dev/null 2>&1
"$TMUX_REAL" -L "$MDI" set -g remain-on-exit on >/dev/null 2>&1   # the shim editor exits at once
MVP=$("$TMUX_REAL" -L "$MDI" list-panes -t t -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$MDI" set-option -p -t "$MVP" @cockpit_click_line "see $MVN" >/dev/null 2>&1
"$TMUX_REAL" -L "$MDO" -f /dev/null new-session -d -s o -x 100 -y 32 -c "$MVD" \
    "TERM=xterm-256color exec $TMUX_REAL -L $MDI attach -t t" >/dev/null 2>&1
until_out '?*' "$TMUX_REAL" -L "$MDI" list-clients -F '#{client_name}'
mvclick() { "$TMUX_REAL" -L "$MDI" run-shell -b "$(printf %q "$DIR/scripts/tmux-open-path") $MVP" >/dev/null 2>&1; }
mvcap() { "$TMUX_REAL" -L "$MDO" capture-pane -p -t o; }
mvclick
if until_cap "$MDO" o '*# Heading*p1*'; then ok "a click on a .md shows it rendered in a popup"
else bad "a click on a .md shows it rendered in a popup" "# Heading, then p1" "$(mvcap | head -8)"; fi
# The client's top row is the popup's border, so the -T title is what is
# read here; the viewer's own frame title is a row lower and is not the gate.
case "$(mvcap | head -1)" in
    *"$MVN"*) ok "the popup border's -T title (the client's top row) is the file's name, literally" ;;
    *) bad "the popup border's -T title (the client's top row) is the file's name, literally" "$MVN" "$(mvcap | head -1)" ;;
esac
# rows: # Heading (line 1), gap, p0 (line 3), gap, p1 (line 5): four down, p1 is on top
"$TMUX_REAL" -L "$MDO" send-keys -t o j j j j e
if until_file "$MVE" '*OPEN*'; then
    is "e hands the file to the editor at the top line shown" "OPEN +5 $MVD/w/$MVN" "$(cat "$MVE")"
else bad "e hands the file to the editor at the top line shown" "OPEN +5 $MVD/w/$MVN" "nothing opened"; fi
: > "$MVE"
"$TMUX_REAL" -L "$MDI" set -g @cockpit_md_view off >/dev/null 2>&1
mvclick
if until_file "$MVE" '*OPEN*'; then
    is "@cockpit_md_view off: the click opens the editor, no viewer" "OPEN $MVD/w/$MVN" "$(cat "$MVE")"
else bad "@cockpit_md_view off: the click opens the editor, no viewer" "OPEN $MVD/w/$MVN" "nothing opened"; fi
# The sleep stays: the assertion is that nothing ran, and an absence has no
# observable to poll; a #() job, had one started, runs in the background.
sleep 0.5
is "and nothing in the name ran" "" "$("$TMUX_REAL" -L "$MDI" show -gqv @pwned 2>/dev/null)"
"$TMUX_REAL" -L "$MDO" kill-server >/dev/null 2>&1; "$TMUX_REAL" -L "$MDI" kill-server >/dev/null 2>&1
fi

echo "== tmux-open-path --folder: the file browser, and what is done with the line it prints (#363) =="
# The tap on the pane path and the menu's Browse folder row reach --folder. It
# opens a cockpit-tui files popup; the popup re-enters as --browse, reads one
# line and acts on it. Private server started with -c, a shim code, shim editors
# and a stub cockpit-tui that prints a canned line. The tmux in PATH is a
# wrapper that logs every call before it runs, so "nothing was opened" is read
# from the calls, never from a sleep.
if need_tmux "folder browser"; then :; else
FB=$(mktmpd); FBS="cockpit-fb-$$"; SOCKS="$SOCKS $FBS"
FBD="$FB/proj"; mkdir -p "$FB/bin" "$FBD/sub"
FBF="$FBD/a b's.txt"; : > "$FBF"; : > "$FBD/sub/n.md"; : > "$FBD/sub/n.txt"
FBNL="$FBD/two"$'\n'"lines.txt"; : > "$FBNL"     # a real file whose name is two lines
printf '#!/bin/sh\nprintf "[%%s]" "$0" "$@" >> "%s/code.log"\n' "$FB" > "$FB/bin/code"
for e in ed-cockpit ed-visual ed-editor nvim; do
    printf '#!/bin/sh\nprintf "%%s" "$0" >> "%s/ed.log"; printf " [%%s]" "$@" >> "%s/ed.log"; echo >> "%s/ed.log"\n' "$FB" "$FB" "$FB" > "$FB/bin/$e"
done
cat > "$FB/bin/tmux" <<STUB
#!/bin/sh
printf '%s' "\$1" >> "$FB/calls"; for a in "\$@"; do printf ' [%s]' "\$a" >> "$FB/calls"; done; echo >> "$FB/calls"
# no client is attached to a private server, so a popup cannot open: log it only
[ "\$1" = display-popup ] && exit 0
exec "$TMUX_REAL" -L "$FBS" "\$@"
STUB
# the stub picker: what the browser would print, and how it exits
cat > "$FB/bin/cockpit-tui" <<STUB
#!/bin/sh
printf '%s\n' "\$*" > "$FB/tui.args"
cat "$FB/errtext" >&2
cat "$FB/answer"
exit \$(cat "$FB/rc")
STUB
chmod +x "$FB"/bin/*
# from $FB: a #() that ran would drop its file in the server's cwd, never the checkout
( cd "$FB" && PATH="$FB/bin:$PATH" "$TMUX_REAL" -L "$FBS" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$FBD" 'exec sleep 3600' >/dev/null 2>&1 )
FBP=$("$TMUX_REAL" -L "$FBS" list-panes -t t -F '#{pane_id}')
FBSOCK=$("$TMUX_REAL" -L "$FBS" display-message -p '#{socket_path}')
"$TMUX_REAL" -L "$FBS" set -g @cockpit_is_touch \
    "$(sed -n 's/^set -g @cockpit_is_touch "\(.*\)"$/\1/p' "$DIR/scripts/lib/statusbar.conf")"
# fb <on|off> <printf-format of the answer> [rc] [env...]: one --browse run
fb() {
    local touch=$1 ans=$2 rc=${3:-0}; shift 3
    "$TMUX_REAL" -L "$FBS" set -g @cockpit_touch "$touch"
    "$TMUX_REAL" -L "$FBS" set -g remain-on-exit on    # the shim editors exit at once
    printf %b "$ans" > "$FB/answer"; echo "$rc" > "$FB/rc"; : > "$FB/errtext"
    : > "$FB/calls"; : > "$FB/code.log"; : > "$FB/ed.log"
    ( cd "$FB" && env PATH="$FB/bin:$PATH" COCKPIT_TUI_BIN="$FB/bin/cockpit-tui" TMUX="$FBSOCK,0,0" "$@" \
        "$DIR/scripts/tmux-open-path" --browse "$FBP" </dev/null >"$FB/out" 2>&1 )
}
fbn() { grep -c "^$1 " "$FB/calls"; }          # calls of one tmux subcommand
fbwait() { local n=0; until [ -s "$1" ] || [ "$n" -ge 50 ]; do sleep 0.1; n=$((n + 1)); done; }
fbnotes() { grep '^display-message' "$FB/calls" | grep -vc '\[-p\]'; }   # notes shown, not queries
fbwin() { "$TMUX_REAL" -L "$FBS" list-windows -t t | grep -c .; }

# --folder: the tap opens the browser popup and nothing else
: > "$FB/calls"
env PATH="$FB/bin:$PATH" COCKPIT_TUI_BIN="$FB/bin/cockpit-tui" TMUX="$FBSOCK,0,0" \
    "$DIR/scripts/tmux-open-path" --folder "$FBP" </dev/null >"$FB/out" 2>&1
is "--folder opens one popup" "1" "$(fbn display-popup)"
is "--folder opens no editor and no window" "0|0" "$(fbn run-shell)|$(fbn new-window)"
case "$(grep '^display-popup' "$FB/calls")" in
    *"[-T] [ files"*"--browse $FBP]"*) ok "the popup is titled and re-enters as --browse on the pane" ;;
    *) bad "the popup is titled and re-enters as --browse on the pane" "-T files, --browse $FBP" "$(grep '^display-popup' "$FB/calls")" ;;
esac
: > "$FB/calls"
env PATH="$FB/bin:$PATH" COCKPIT_TUI_BIN="$FB/nope/cockpit-tui" TMUX="$FBSOCK,0,0" \
    "$DIR/scripts/tmux-open-path" --folder "$FBP" </dev/null >"$FB/out" 2>&1
is "--folder without cockpit-tui says so and opens no popup" "1|0" \
   "$(grep -c 'cockpit-tui is missing' "$FB/calls")|$(fbn display-popup)"

# --browse hands the browser the pane's folder
fb off "open $FBF\n" 0
is "the browser is started on the pane's folder" "files $FBD" "$(cat "$FB/tui.args")"

# a non-touch client: open <file> goes the usual way, to VS Code
fb off "open $FBF\n" 0
fbwait "$FB/code.log"
is "non-touch: open goes to code, as the path was opened before" "1|0" "$(fbn run-shell)|$(fbn new-window)"
case "$(cat "$FB/code.log")" in
    *"[--goto][$FBF:1:1]"*) ok "non-touch: code got the exact path" ;;
    *) bad "non-touch: code got the exact path" "--goto $FBF:1:1" "$(cat "$FB/code.log")" ;;
esac

# a touch client: a terminal editor window instead, never VS Code
fb on "open $FBF\n" 0 "VISUAL=$FB/bin/ed-visual" "EDITOR=$FB/bin/ed-editor"
fbwait "$FB/ed.log"
is "touch: one window, no code" "1|0|2" "$(fbn new-window)|$(fbn run-shell)|$(fbwin)"
is "touch: \$VISUAL opens the exact path" "$FB/bin/ed-visual [$FBF]" "$(cat "$FB/ed.log")"
is "touch: code never ran" "" "$(cat "$FB/code.log")"
fb on "open $FBF\n" 0 "EDITOR=$FB/bin/ed-editor"
fbwait "$FB/ed.log"
is "touch: \$EDITOR when there is no \$VISUAL" "$FB/bin/ed-editor [$FBF]" "$(cat "$FB/ed.log")"
fb on "open $FBF\n" 0 "VISUAL=" "EDITOR="
fbwait "$FB/ed.log"
is "touch: nvim when nothing is set" "$FB/bin/nvim [$FBF]" "$(cat "$FB/ed.log")"
"$TMUX_REAL" -L "$FBS" set -g @cockpit_editor "$FB/bin/ed-cockpit"
fb on "open $FBF\n" 0 "VISUAL=$FB/bin/ed-visual"
fbwait "$FB/ed.log"
is "touch: @cockpit_editor beats \$VISUAL" "$FB/bin/ed-cockpit [$FBF]" "$(cat "$FB/ed.log")"
"$TMUX_REAL" -L "$FBS" set -gu @cockpit_editor
# the editor window is named for the file and sits in its folder
# (a .txt: a markdown file goes to the viewer instead, #283)
fb on "open $FBD/sub/n.txt\n" 0 "VISUAL=$FB/bin/ed-visual"
is "touch: the window is named for the file and starts in its folder" "n.txt $FBD/sub" \
   "$("$TMUX_REAL" -L "$FBS" list-windows -t t -F '#{window_name} #{pane_start_path}' | grep -Fx "n.txt $FBD/sub")"
# a markdown file goes to the viewer, drawn in the browser's popup, touch or not (#283)
for fbt in on off; do
    fb "$fbt" "open $FBD/sub/n.md\n" 0 "VISUAL=$FB/bin/ed-visual"
    is "$fbt: a markdown file picked in the browser opens in the viewer" "view -- $FBD/sub/n.md" "$(cat "$FB/tui.args" 2>/dev/null)"
    is "$fbt: and no editor window opens for it" "" \
       "$("$TMUX_REAL" -L "$FBS" list-windows -t t -F '#{window_name}' | grep -Fx n.md)"
done

# names that are tmux formats: -n and -c expand #(), so term_edit escapes both
FBH="$FBD/#(touch\${IFS}PWN)dir"; mkdir -p "$FBH"; FBHF="$FBH/#(touch\${IFS}PWN).txt"; : > "$FBHF"
fb on "open $FBHF\n" 0 "VISUAL=$FB/bin/ed-visual"
fbwait "$FB/ed.log"
is "touch, format-looking names: the editor got the exact path" "$FB/bin/ed-visual [$FBHF]" "$(cat "$FB/ed.log")"
is "touch, format-looking names: the window name and folder are literal" "#(touch\${IFS}PWN).txt $FBH" \
   "$("$TMUX_REAL" -L "$FBS" list-windows -t t -F '#{window_name} #{pane_start_path}' | grep -Fx "#(touch\${IFS}PWN).txt $FBH")"
is "touch, format-looking names: nothing ran" "" "$(find "$FB" "$PWD" -maxdepth 3 -name PWN 2>/dev/null)"

# a touch client with no editor anywhere: one note, no window
mkdir -p "$FB/nobin"
for x in bash cat rm mktemp readlink dirname; do ln -sf "$(command -v $x)" "$FB/nobin/$x"; done
ln -sf "$FB/bin/tmux" "$FB/nobin/tmux"
fb on "open $FBF\n" 0 "PATH=$FB/nobin" "VISUAL=" "EDITOR="
is "touch, no editor found: no window and one note naming the fix" "0|1" \
   "$(fbn new-window)|$(grep -c 'no terminal editor' "$FB/calls")"

# alt-c: the path is copied, nothing opens
fb on "copy $FBF\n" 0
is "copy: the path is in the paste buffer" "$FBF" "$("$TMUX_REAL" -L "$FBS" show-buffer)"
is "copy: nothing opened" "0|0" "$(fbn run-shell)|$(fbn new-window)"

# esc: nothing, and no complaint
fb on "" 1
is "esc: nothing runs and nothing is said" "0|0|0|0" \
   "$(fbn run-shell)|$(fbn new-window)|$(fbn load-buffer)|$(fbnotes)"

# the printed line is untrusted: each of these must open and copy nothing
for row in "two lines|open $FBF\\nopen $FBF" "unknown verb|rm $FBF" "missing path|open $FBD/nope" \
           "relative path|open proj/sub/n.md" "newline in the name|open $FBD/two\\nlines.txt" "no path|open" "empty|" "upper case verb|OPEN $FBF" \
           "missing past dotdot|open $FBD/../nope"; do
    for t in on off; do
        fb "$t" "${row#*|}\n" 0 "VISUAL=$FB/bin/ed-visual"
        is "refused (${row%%|*}, touch $t): no editor, no code, no copy" "0|0|0|1" \
           "$(fbn run-shell)|$(fbn new-window)|$(fbn load-buffer)|$(fbnotes)"
    done
done

# a tui from before this screen existed
fb on "" 2
printf 'cockpit-tui: unknown screen files\n' > "$FB/errtext"
env PATH="$FB/bin:$PATH" COCKPIT_TUI_BIN="$FB/bin/cockpit-tui" TMUX="$FBSOCK,0,0" \
    "$DIR/scripts/tmux-open-path" --browse "$FBP" </dev/null >"$FB/out" 2>&1
is "a stale cockpit-tui is told apart" "1" "$(grep -c 'older than the plugin' "$FB/calls")"
"$TMUX_REAL" -L "$FBS" kill-server >/dev/null 2>&1
fi

echo "== agent-state sub-*: the documented subagent hooks append to a per-session log (#361) =="
# SubagentStart, SubagentStop and TeammateIdle each append one line to
# $XDG_STATE_HOME/tmux-cockpit/subs/<session_id>.log; SessionEnd drops it. No tmux call.
SLD=$(mktmpd); SLS=aaaaaaaa-1111-4222-8333-444444444444; SLT=bbbbbbbb-1111-4222-8333-444444444444
SLDIR="$SLD/state/tmux-cockpit/subs"
sl() {                        # <cmd> <json>  one payload through the hook, exit status last
    printf '%s\n' "$2" | env XDG_STATE_HOME="$SLD/state" "$DIR/scripts/agent-state" "$1" 2>&1
    echo "rc=$?"
}
slp() { printf '{"session_id":"%s","hook_event_name":"%s","agent_id":"%s","agent_type":"x"}' "$1" "$2" "$3"; }
sl_modes() { stat -c '%a' "$1" 2>/dev/null || stat -f '%Lp' "$1"; }
is "sub-start exits 0 and prints nothing" "rc=0" "$(sl sub-start "$(slp $SLS SubagentStart afix-224)")"
sl sub-stop "$(slp $SLS SubagentStop afix-224)" >/dev/null
sl sub-idle "$(slp $SLS TeammateIdle afix-224)" >/dev/null
sl sub-start "$(slp $SLS SubagentStart arev-224)" >/dev/null
is "each event is one line: event and agent id, in order" "start afix-224|stop afix-224|idle afix-224|start arev-224" \
   "$(awk -F'\t' 'NF == 3 { printf "%s%s %s", (NR > 1 ? "|" : ""), $2, $3 }' "$SLDIR/$SLS.log")"
is "and the first field is the epoch second" "0" \
   "$(awk -F'\t' '$1 !~ /^[0-9]+$/ || $1 < 1700000000 { n++ } END { print n + 0 }' "$SLDIR/$SLS.log")"
is "the dir is 0700 and the log 0600" "700 600" "$(sl_modes "$SLDIR") $(sl_modes "$SLDIR/$SLS.log")"
rm -rf "$SLD/state"; (umask 022; sl sub-start "$(slp $SLS SubagentStart a1)" >/dev/null)
is "a permissive umask still gives 0700 and 0600" "700 600" "$(sl_modes "$SLDIR") $(sl_modes "$SLDIR/$SLS.log")"
is "another session has a log of its own" "1 1" \
   "$(sl sub-start "$(slp $SLT SubagentStart b1)" >/dev/null; grep -c . "$SLDIR/$SLS.log") $(grep -c . "$SLDIR/$SLT.log")"
# Anything that is not a plain id is dropped: no file, no output, exit 0.
rm -rf "$SLD/state"
SLLONG=$(printf 'x%.0s' $(seq 1 65))
for row in "path in the agent id|$(slp $SLS SubagentStart '../../evil')" \
           "path in the session id|$(slp '../evil' SubagentStart a1)" \
           "a space in the agent id|$(slp $SLS SubagentStart 'a b')" \
           "an id over 64 bytes|$(slp $SLS SubagentStart "$SLLONG")" \
           "an empty agent id|$(slp $SLS SubagentStart '')" \
           "no agent id|{\"session_id\":\"$SLS\"}" \
           "no session id|{\"agent_id\":\"a1\"}" \
           "not JSON|garbage" "an empty payload|"; do
    rm -rf "$SLD/state"   # one state dir per row: an earlier row's file must not fail a later one
    out=$(sl sub-start "${row#*|}"); n=$(find "$SLD/state" -type f 2>/dev/null | grep -c .)
    is "malformed: ${row%%|*}: exit 0, silent, no log" "rc=0 0" "$out $n"
done
# The payload's own free text cannot forge an id: its quotes arrive escaped.
rm -rf "$SLD/state"
sl sub-stop "{\"session_id\":\"$SLS\",\"last_assistant_message\":\"\\\"agent_id\\\":\\\"evil\\\"\",\"agent_id\":\"real1\"}" >/dev/null
is "an agent_id inside a message that comes before the real one is not taken" "stop real1" "$(awk -F'\t' '{ print $2, $3 }' "$SLDIR/$SLS.log")"
# A symlink where the log goes is never followed.
rm -rf "$SLD/state"; mkdir -p "$SLDIR"; : > "$SLD/victim"; ln -s "$SLD/victim" "$SLDIR/$SLS.log"
is "a symlinked log: exit 0" "rc=0" "$(sl sub-start "$(slp $SLS SubagentStart a1)")"
is "and the link target is untouched" "0" "$(wc -c < "$SLD/victim" | tr -d ' ')"
# A symlink where the dir goes is never followed, by the record or by the sweep.
rm -rf "$SLD/state" "$SLD/target"; mkdir -p "$SLD/state/tmux-cockpit" "$SLD/target"
printf 'keep\n' > "$SLD/target/$SLS.log"; ln -s "$SLD/target" "$SLD/state/tmux-cockpit/subs"
sl sub-start "$(slp $SLS SubagentStart a1)" >/dev/null
is "a symlinked dir: sub-start writes nothing through it" "keep" "$(cat "$SLD/target/$SLS.log")"
sl sub-end "{\"session_id\":\"$SLS\"}" >/dev/null
is "and sub-end deletes nothing through it" "keep" "$(cat "$SLD/target/$SLS.log" 2>/dev/null)"
rm -rf "$SLD/state" "$SLD/target"
# The read is cut at 256 KB: ids after that are never seen, ids before it are.
SLPAD=$(head -c 300000 /dev/zero | tr '\0' x)
sl sub-start "{\"session_id\":\"$SLS\",\"agent_id\":\"near1\",\"m\":\"$SLPAD\"}" >/dev/null
sl sub-start "{\"session_id\":\"$SLT\",\"m\":\"$SLPAD\",\"agent_id\":\"far1\"}" >/dev/null
is "an id before the 256 KB cut is read; one past it is not" "near1|" \
   "$(awk -F'\t' '{ print $3 }' "$SLDIR/$SLS.log" 2>/dev/null)|$(cat "$SLDIR/$SLT.log" 2>/dev/null)"
rm -rf "$SLD/state"
# A writer that never ends the payload must not hold the hook.
rm -rf "$SLD/state"; SLFIFO="$SLD/fifo"; mkfifo "$SLFIFO"
env XDG_STATE_HOME="$SLD/state" "$DIR/scripts/agent-state" sub-start < "$SLFIFO" >/dev/null 2>&1 &
SLPID=$!; exec 8> "$SLFIFO"; printf '{"session_id":"%s","agent_id":"a1"' "$SLS" >&8
slgone() { kill -0 "$SLPID" 2>/dev/null || echo gone; }
until_out gone slgone; is "an unclosed payload does not hold the hook" "gone" "$(slgone)"
exec 8>&-; wait "$SLPID" 2>/dev/null; is "and it still exits 0" "0" "$?"
# SessionEnd: this session's log goes, another's stays, and logs past 7 days are swept.
rm -rf "$SLD/state"
sl sub-start "$(slp $SLS SubagentStart a1)" >/dev/null; sl sub-start "$(slp $SLT SubagentStart b1)" >/dev/null
: > "$SLDIR/old.log"; : > "$SLDIR/recent.log"; : > "$SLDIR/old.keep"; : > "$SLDIR/past7.log"; : > "$SLDIR/edge.log"
touch -d '8 days ago' "$SLDIR/old.log" "$SLDIR/old.keep" 2>/dev/null || touch -t "$(date -v-8d +%Y%m%d%H%M)" "$SLDIR/old.log" "$SLDIR/old.keep"
touch -d '6 days ago' "$SLDIR/recent.log" 2>/dev/null || touch -t "$(date -v-6d +%Y%m%d%H%M)" "$SLDIR/recent.log"
touch -d '169 hours ago' "$SLDIR/past7.log" 2>/dev/null || touch -t "$(date -v-169H +%Y%m%d%H%M)" "$SLDIR/past7.log"
touch -d '167 hours ago' "$SLDIR/edge.log" 2>/dev/null || touch -t "$(date -v-167H +%Y%m%d%H%M)" "$SLDIR/edge.log"
is "sub-end exits 0" "rc=0" "$(sl sub-end "{\"session_id\":\"$SLS\",\"hook_event_name\":\"SessionEnd\",\"reason\":\"exit\"}")"
is "it drops its own session's log and the logs past 7 days (169 h, not 167 h), and nothing else" "b1 edge.log old.keep recent.log" \
   "$(ls "$SLDIR" | sed "s/^$SLT.log$/b1/" | tr '\n' ' ' | sed 's/ $//')"
SLBEFORE=$(ls "$SLDIR" | tr '\n' ' ')
sl sub-end '{}' >/dev/null; sl sub-end '{"session_id":"../x"}' >/dev/null
is "sub-end on no session id, or a malformed one, removes nothing" "$SLBEFORE" "$(ls "$SLDIR" | tr '\n' ' ')"
# No tmux call: a PATH with a tmux that records its use sees none.
SLBIN="$SLD/bin"; mkdir -p "$SLBIN"; printf '#!/bin/sh\ntouch "%s/tmux-ran"\n' "$SLD" > "$SLBIN/tmux"; chmod +x "$SLBIN/tmux"
for c in sub-start sub-stop sub-idle sub-end; do
    printf '%s\n' "$(slp $SLS X a1)" | env PATH="$SLBIN:$PATH" TMUX_PANE=%1 XDG_STATE_HOME="$SLD/state" "$DIR/scripts/agent-state" "$c" >/dev/null 2>&1
done
is "none of the four commands calls tmux" "" "$(ls "$SLD/tmux-ran" 2>/dev/null)"

echo "== hook wiring: the subagent hooks, install.sh --hooks, --check and --uninstall --hooks (#361) =="
if command -v jq >/dev/null 2>&1; then
SLEX="$DIR/examples/claude-hooks.json"
slcmds() { jq -r '.hooks | to_entries[] | .key as $k | .value[].hooks[].command
    | select(test("agent-state ")) | "\($k):" + sub(".*agent-state "; "")' "$1" | sort | tr '\n' ' ' | sed 's/ $//'; }
is "the example wires each new command once, on its event, and keeps the old ones" \
   "SessionEnd:off SessionEnd:sub-end SubagentStart:running SubagentStart:sub-start SubagentStop:sub-stop TeammateIdle:sub-idle TeammateIdle:wake" \
   "$(slcmds "$SLEX" | tr ' ' '\n' | grep -E '^(SessionEnd|SubagentStart|SubagentStop|TeammateIdle):' | tr '\n' ' ' | sed 's/ $//')"
is "every entry of the example holds one handler, so an upgrade keeps no old twin" "0" \
   "$(jq '[.hooks[][] | select((.hooks | length) != 1)] | length' "$SLEX")"
# Upgrade: a box set up by the --hooks before this change keeps its entries, and gains the new
# ones beside them, once, however often --hooks runs.
SLU=$(mktmpd); mkdir -p "$SLU/.claude"; jq '{hooks: .hooks}' "$DIR/tests/fixtures/hooks-pre-361.json" > "$SLU/.claude/settings.json"
slucount() { jq -r --arg c "$1" '[.. | strings | select(test("TMUX_COCKPIT_DIR.*/scripts/agent-state " + $c + "$"))] | length' "$SLU/.claude/settings.json"; }
sluall() { for c in running wake off sub-start sub-stop sub-idle sub-end; do slucount $c; done | tr '\n' ' ' | sed 's/ $//'; }
is "master's hooks as the starting point: running, wake and off as shipped, no sub-* yet" "4 1 2 0 0 0 0" "$(sluall)"
for i in 1 2; do env HOME="$SLU" XDG_BIN_HOME="$SLU/bin" "$DIR/install.sh" --hooks >/dev/null 2>&1; done
is "upgrading, twice: the old counts hold and each sub-* is added once" "4 1 2 1 1 1 1" "$(sluall)"
SLH=$(mktmpd); mkdir -p "$SLH/.claude"
cat > "$SLH/.claude/settings.json" <<'JSON'
{ "hooks": {
  "SubagentStop": [ { "hooks": [ { "type": "command", "command": "my-own-stop" } ] } ],
  "TeammateIdle": [ { "matcher": "", "hooks": [ { "type": "command", "command": "/opt/x/scripts/agent-state sub-idle" } ] } ] } }
JSON
cp "$SLH/.claude/settings.json" "$SLH/orig.json"
slrun() { env -u TMUX -u TMUX_PANE HOME="$SLH" XDG_BIN_HOME="$SLH/bin" XDG_CONFIG_HOME="$SLH/cfg" TMUX_TMPDIR="$SLH" "$DIR/install.sh" "$@" 2>&1; }
slrun --hooks >/dev/null; slrun --hooks >/dev/null
slown() { jq -r --arg c "$1" '[.. | strings | select(test("TMUX_COCKPIT_DIR.*/scripts/agent-state " + $c + "$"))] | length' "$SLH/.claude/settings.json"; }
is "--hooks twice: each of the four commands once in cockpit's own path" "1 1 1 1" \
   "$(for c in sub-start sub-stop sub-idle sub-end; do slown $c; done | tr '\n' ' ' | sed 's/ $//')"
is "and the foreign hooks, the lookalike sub-idle included, are all still there" "1 1" \
   "$(jq '[.. | strings | select(. == "my-own-stop")] | length' "$SLH/.claude/settings.json") $(jq '[.. | strings | select(. == "/opt/x/scripts/agent-state sub-idle")] | length' "$SLH/.claude/settings.json")"
slhooks_dc() { slrun --check | sed -n 's/^\[ *\([a-z]*\) *\] *subhooks.*/\1/p'; }
is "--check: a wired box has an ok subhooks row" "ok" "$(slhooks_dc)"
cp "$SLH/.claude/settings.json" "$SLH/wired.json"
jq 'del(.. | objects | select((.command // "") | test("agent-state sub-")))' "$SLH/wired.json" > "$SLH/.claude/settings.json"
is "--check: cockpit hooks without the four are a warn subhooks row" "warn" "$(slhooks_dc)"
printf '{}' > "$SLH/.claude/settings.json"
is "--check: no cockpit hooks at all is a skip subhooks row (the hooks row speaks)" "skip" "$(slhooks_dc)"
cp "$SLH/wired.json" "$SLH/.claude/settings.json"
slrun --uninstall --hooks >/dev/null
is "--uninstall --hooks removes every cockpit command and nothing foreign" "0 1 1" \
   "$(jq '[.. | strings | select(test("TMUX_COCKPIT_DIR"))] | length' "$SLH/.claude/settings.json") $(jq '[.. | strings | select(. == "my-own-stop")] | length' "$SLH/.claude/settings.json") $(jq '[.. | strings | select(. == "/opt/x/scripts/agent-state sub-idle")] | length' "$SLH/.claude/settings.json")"
is "and the file is the one it started as" "$(jq -S . "$SLH/orig.json")" "$(jq -S . "$SLH/.claude/settings.json")"
fi
echo "== tmux-review --request: the popup title shows a # in the repo path, never expands it (#329) =="
# -T is a tmux format: without fesc a dir named x#(touch MARK) renders as "x".
if need_tmux "tmux-review --request -T"; then :; else
RT9="cockpit-rt9-$$"; SOCKS="$SOCKS $RT9"; RT9D=$(mktmpd); RT9S=$(mktmpd)
RT9R="$RT9D/x#(touch MARK)#S"
mkdir -p "$RT9R"
( cd "$RT9R" && git init -q -b main . && printf 'a\n' > f && git add f \
  && git -c user.name=t -c user.email=t@t commit -qm i && printf 'a\nb\n' > f ) >/dev/null 2>&1
"$TMUX_REAL" -L "$RT9" -f /dev/null new-session -d -s t -x 100 -y 30 -c "${RT9R//#/##}" 'exec -a claude bash' >/dev/null 2>&1
RT9P=$("$TMUX_REAL" -L "$RT9" list-panes -t t: -F '#{pane_id}')
RT9SHIM=$(mktmpd)
cat > "$RT9SHIM/tmux" <<SHIMEOF
#!/bin/sh
case "\$1" in
    display-popup) p=; for a; do [ "\$p" = -T ] && printf '%s' "\$a" > "$RT9S/title"; p=\$a; done; exit 0 ;;
    display-message) for a; do :; done; case "\$a" in *session_attached*) echo 1; exit 0 ;; esac ;;
esac
exec "$TMUX_REAL" -L "$RT9" "\$@"
SHIMEOF
chmod +x "$RT9SHIM/tmux"
PATH="$RT9SHIM:$PATH" XDG_STATE_HOME=$RT9S "$DIR/scripts/tmux-review" --request "$RT9P" >/dev/null 2>&1
RT9ROOT=$(git -C "$RT9R" rev-parse --show-toplevel)
is "the title, drawn by tmux, names the repo path literally" \
   " review · requested · ${RT9ROOT/#"$HOME/"/\~/} " \
   "$("$TMUX_REAL" -L "$RT9" display-message -p "$(cat "$RT9S/title" 2>/dev/null)")"
is "and nothing ran" "no" "$([ -e "$RT9D/MARK" ] || [ -e "$DIR/MARK" ] || [ -e MARK ] && echo yes || echo no)"
"$TMUX_REAL" -L "$RT9" kill-server >/dev/null 2>&1
rm -f "$DIR/MARK" MARK
fi

echo "== tmux-review: the rc 20 --deliver run-shell passes a # in the repo path through literally (#329) =="
# run-shell expands formats: without fesc a dir named x#S... reached --deliver
# as x<session>..., and without its shq'd dir the "Paths are relative" line went.
if need_tmux "tmux-review rc 20 run-shell"; then :; else
RD9="cockpit-rd9-$$"; SOCKS="$SOCKS $RD9"; RD9D=$(mktmpd); RD9S=$(mktmpd)
RD9R="$RD9D/x#S#(touch MARK)"
mkdir -p "$RD9R"
( cd "$RD9R" && git init -q -b main . && printf 'a\n' > f && git add f \
  && git -c user.name=t -c user.email=t@t commit -qm i && printf 'a\nb\n' > f ) >/dev/null 2>&1
RD9B=$(mktmpd); RD9W=$(mktmpd); fake_revdiff "$RD9B"
printf '## f:2 (+)\nRD9-MARKER\n' > "$RD9W/out.1"; echo 10 > "$RD9W/rc.1"
"$TMUX_REAL" -L "$RD9" -f /dev/null new-session -d -s t -x 400 -y 50 -c "${RD9R//#/##}" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RD9" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
RD9P=$("$TMUX_REAL" -L "$RD9" list-panes -t t: -F '#{pane_id}')
until_out claude "$TMUX_REAL" -L "$RD9" display-message -p -t "$RD9P" '#{pane_current_command}'
RD9SHIM=$(mktmpd)
printf '#!/bin/sh\nexec "%s" -L "%s" "$@"\n' "$TMUX_REAL" "$RD9" > "$RD9SHIM/tmux"; chmod +x "$RD9SHIM/tmux"
printf d | PATH="$RD9SHIM:$RD9B:$PATH" FAKE_RD_DIR="$RD9W" XDG_STATE_HOME=$RD9S \
    "$DIR/scripts/tmux-review" -p "$RD9P" >/dev/null 2>&1
RD9ROOT=$(git -C "$RD9R" rev-parse --show-toplevel)
if until_cap "$RD9" "$RD9P" '*RD9-MARKER*'; then ok "the review is delivered through run-shell"
else bad "the review is delivered through run-shell" "RD9-MARKER in the pane" "$("$TMUX_REAL" -L "$RD9" capture-pane -p -J -t "$RD9P")"; fi
case "$("$TMUX_REAL" -L "$RD9" capture-pane -p -J -t "$RD9P")" in
    *"Paths are relative to $RD9ROOT."*) ok "and --deliver got the repo path literally" ;;
    *) bad "and --deliver got the repo path literally" "Paths are relative to $RD9ROOT." \
         "$("$TMUX_REAL" -L "$RD9" capture-pane -p -J -t "$RD9P" | grep -a 'relative')" ;;
esac
is "and nothing ran" "no" "$([ -e "$RD9D/MARK" ] || [ -e "$DIR/MARK" ] || [ -e MARK ] && echo yes || echo no)"
"$TMUX_REAL" -L "$RD9" kill-server >/dev/null 2>&1
rm -f "$DIR/MARK" MARK
fi

echo "== tmux-review: review_base, origin/HEAD outranks a local master when there is no main (#328) =="
# The master arm moved above origin/HEAD answered "vs master" and stayed green.
RLM=$(mktmpd)
(
  cd "$RLM" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q --bare -b develop remote.git
  git clone -q remote.git work
  cd work || exit
  printf 'a\n' > f; git add -A; git commit -q -m init; git push -q origin develop
  git remote set-head origin develop
  git checkout -q -b master; printf 'a\nM\n' > f; git commit -q -am master-moved
  git checkout -q -b topic; printf 'a\nM\nT\n' > f; git commit -q -am topic
) >/dev/null 2>&1
RLMB=$(. "$DIR/scripts/lib/review-diff.sh"; review_base "$RLM/work")
is "a local master, no main: the label is vs origin/develop" "vs origin/develop" "$(printf '%s' "$RLMB" | cut -f2)"
is "and the base is the merge-base with origin/develop, not master" \
   "$(git -C "$RLM/work" rev-parse origin/develop)" "$(printf '%s' "$RLMB" | cut -f1)"
rm -rf "$RLM"

echo "== agent-state: @panestate_since stamps entering needs-input, in the one tmux call (#225) =="
# tm's "waiting Nm" reads the stamp. It is set when the pane ENTERS needs-input, kept while
# the pane stays there (the sticky done/error and a repeat needs-input refuse the
# transition), cleared on any way out, and it rides the transition's one round-trip.
if need_tmux "agent-state @panestate_since"; then :; else
WS="cockpit-since-$$"; SOCKS="$SOCKS $WS"
"$TMUX_REAL" -L "$WS" -f /dev/null new-session -d -s t -x 80 -y 24 \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$WS" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
WSP=$("$TMUX_REAL" -L "$WS" list-panes -t t -F '#{pane_id}')
WSD=$(mktmpd); WSLOG="$WSD/calls"
cat > "$WSD/tmux" <<SHIMEOF
#!/bin/sh
printf '%s\n' "\$*" >> "$WSLOG"
exec "$TMUX_REAL" "\$@"
SHIMEOF
chmod +x "$WSD/tmux"
wscall() { : > "$WSLOG"
    PATH="$WSD:$PATH" TMUX="$SOCKDIR/$WS,0,0" "$DIR/scripts/agent-state" "$@" </dev/null >/dev/null 2>&1
    wc -l < "$WSLOG" | tr -d ' '; }
wssince() { "$TMUX_REAL" -L "$WS" show-option -pqv -t "$WSP" @panestate_since; }
wscall running "$WSP" >/dev/null
is "a running pane carries no stamp" "" "$(wssince)"
wsb=$(date +%s); wsn=$(wscall needs-input "$WSP"); wsa=$(date +%s); ws1=$(wssince)
is "entering needs-input still costs one tmux invocation" "1" "$wsn"
case "$ws1" in ''|*[!0-9]*) ws1=0 ;; esac
is "entering needs-input stamps the epoch second it entered" "yes" \
   "$([ "$ws1" -ge "$wsb" ] && [ "$ws1" -le "$wsa" ] && echo yes || echo "no ($ws1 not in $wsb..$wsa)")"
# Backdated by hand, so "kept" cannot pass by landing in the same second.
"$TMUX_REAL" -L "$WS" set -p -t "$WSP" @panestate_since 1000 >/dev/null 2>&1
wscall needs-input "$WSP" >/dev/null
is "a repeat needs-input keeps the first stamp" "1000" "$(wssince)"
wscall done "$WSP" >/dev/null; wscall error "$WSP" >/dev/null
is "the sticky done and error leave the waiting pane's stamp" "1000" "$(wssince)"
is "the poller's same-state write keeps it too" "1000" "$(wscall poll needs-input "$WSP" >/dev/null; wssince)"
is "leaving for running costs one tmux invocation" "1" "$(wscall running "$WSP")"
is "and clears the stamp" "" "$(wssince)"
wscall needs-input "$WSP" >/dev/null
is "the poller's error out of needs-input clears it" "" "$(wscall poll error "$WSP" >/dev/null; wssince)"
wscall needs-input "$WSP" >/dev/null
# off without idle: the session ended (SessionEnd/SessionStart).
is "an ended session clears it" "" "$(wscall off "$WSP" >/dev/null; wssince)"
"$TMUX_REAL" -L "$WS" kill-server >/dev/null 2>&1
fi

echo "== sessions view: waiting sessions sort longest-first and say 'waiting Nm' (#225) =="
# The stamps are agent-state's @panestate_since, set here by hand on a private server:
# beta has waited 5 minutes, delta 30 seconds, and both sort ahead of the rest.
if [ ! -x "$DIR/bin/cockpit-tui" ]; then
    skip_ "cockpit-tui sessions waiting timers (no bin/cockpit-tui; run make tui)"
elif need_tmux "cockpit-tui sessions waiting timers"; then :; else
sv_server w; SVD=$(mktmpd)
"$TMUX_REAL" -L "$SVS" new-session -d -s delta 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$SVS" set -p -t delta: @panestate needs-input
svnow=$(date +%s)
"$TMUX_REAL" -L "$SVS" set -p -t beta: @panestate_since $((svnow - 300))
"$TMUX_REAL" -L "$SVS" set -p -t delta: @panestate_since $((svnow - 30))
SVCOLS=140 sv_ui "$SVD"
if sv_cap '*beta*waiting 5m*delta*waiting 3?s*gamma*'; then
    ok "the longest wait sorts first, and each waiting row says how long"
else
    bad "the longest wait sorts first, and each waiting row says how long" "beta waiting 5m, delta waiting 3Ns, then gamma" \
        "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -8)"
fi
sv_key Enter; until_file "$SVD/rc" '?*'
is "enter switches to the longest-waiting session" "switch $SVB" "$(cat "$SVD/act" 2>/dev/null)"
SVCOLS=60 sv_ui "$SVD"
sv_cap '*beta*delta*gamma*'
case "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t)" in
    *waiting*) bad "at 60 columns the label goes and the names stay" "no 'waiting'" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -8)" ;;
    *beta*5m*delta*3?s*) ok "at 60 columns the label goes and the names stay" ;;
    *) bad "at 60 columns the label goes and the names stay" "beta 5m, delta 3Ns" "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -8)" ;;
esac
sv_key Escape; until_file "$SVD/rc" '?*'
# A separator inside a hand-set stamp would shift the name fields after it: the
# format strips it, so the stamp still reads and the name stays whole.
"$TMUX_REAL" -L "$SVS" set -p -t beta: @panestate_since "$(printf '%s\037' $((svnow - 300)))"
SVCOLS=140 sv_ui "$SVD"
if sv_cap '*▲ beta *waiting 5m*'; then
    ok "a separator in the stamp neither shifts the name nor loses the wait"
else
    bad "a separator in the stamp neither shifts the name nor loses the wait" "▲ beta ... waiting 5m" \
        "$("$TMUX_REAL" -L "$SVO" capture-pane -p -t t | head -8)"
fi
sv_key Escape; until_file "$SVD/rc" '?*'
"$TMUX_REAL" -L "$SVO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$SVS" kill-server >/dev/null 2>&1
fi

echo "== agent-state sub-taskstop and sub-reconcile: a TaskStop and a Stop's in-flight set in the subagent log (#403) =="
# Payloads as Claude Code 2.1.318 sends them (the #403 rig's hooks.log).
TSD=$(mktmpd); TSS=abb33da9-fbf7-4833-820e-2c21cad69dac; TSLOG="$TSD/state/tmux-cockpit/subs/$TSS.log"
ts() {                        # <cmd> <json>  one payload through the hook, exit status last
    printf '%s\n' "$2" | env XDG_STATE_HOME="$TSD/state" "$DIR/scripts/agent-state" "$1" 2>&1
    echo "rc=$?"
}
tsrows() { awk -F'\t' '{ $1 = ""; sub(/^ /, ""); printf "%s%s", (NR > 1 ? "|" : ""), $0 }' "$TSLOG" 2>/dev/null; }
tsstop() {                    # <tool_input.task_id> <tool_response.task_id> <task_type> <command>
    printf '{"session_id":"%s","hook_event_name":"PostToolUse","tool_name":"TaskStop","tool_input":{"task_id":"%s"},"tool_response":{"message":"Successfully stopped task: %s (%s)","task_id":"%s","task_type":"%s","command":"%s"},"tool_use_id":"toolu_01W2","duration_ms":2}' \
        "$TSS" "$1" "$2" "$4" "$2" "$3" "$4"
}
is "sub-taskstop exits 0 and prints nothing" "rc=0" "$(ts sub-taskstop "$(tsstop sleeper3 a317be06712f142ac local_agent 's3 sleeper')")"
is "a plain agent's stop is keyed by tool_response's task_id, its agent id" "kill a317be06712f142ac" "$(tsrows)"
rm -rf "$TSD/state"
ts sub-taskstop "$(tsstop tm1 t0sm6hcnt in_process_teammate 'i1 named')" >/dev/null
ts sub-taskstop "$(tsstop t0sm6hcnt t0sm6hcnt in_process_teammate 'say \"hi\" }]')" >/dev/null
is "a teammate's stop carries the name the model gave and the description, escapes kept" \
   'kill-tm tm1 i1 named|kill-tm t0sm6hcnt say \"hi\" }]' "$(tsrows)"
rm -rf "$TSD/state"
ts sub-taskstop "{\"session_id\":\"$TSS\",\"tool_name\":\"Bash\",\"tool_input\":{\"task_id\":\"x\"},\"tool_response\":{\"task_id\":\"a1\",\"task_type\":\"local_agent\"}}" >/dev/null
ts sub-taskstop "{\"session_id\":\"$TSS\",\"tool_name\":\"TaskStop\",\"tool_input\":{\"task_id\":\"a1\"},\"tool_response\":\"Error: no task\"}" >/dev/null
ts sub-taskstop "{\"session_id\":\"$TSS\",\"tool_name\":\"TaskStop\",\"tool_input\":{\"task_id\":\"../x\"},\"tool_response\":{\"message\":\"m\",\"task_id\":\"../x\",\"task_type\":\"local_agent\"}}" >/dev/null
ts sub-taskstop "garbage" >/dev/null; ts sub-taskstop "" >/dev/null
is "another tool, a failed stop, a path for an id, garbage: no log" "0" "$(find "$TSD/state" -type f 2>/dev/null | grep -c .)"
tsfin() {                     # <background_tasks array body> [last message]
    printf '{"session_id":"%s","hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"%s","background_tasks":[%s],"session_crons":[]}' "$TSS" "${2:-end}" "$1"
}
is "sub-reconcile exits 0 and prints nothing" "rc=0" "$(ts sub-reconcile "$(tsfin '{"id":"a593c30606c443914","type":"subagent","status":"running","description":"i1 plain","agent_type":"general-purpose"},{"id":"t0sm6hcnt","type":"teammate","status":"running","description":"i1 } named]"},{"id":"b1","type":"shell","status":"running","description":"sleep 9"}' 'x \"background_tasks\":[{\"id\":\"evil\",\"type\":\"subagent\"}]')")"
ts sub-reconcile "$(tsfin '')" >/dev/null
is "the in-flight set: subagent ids, then teammate descriptions; no shell, nothing from the message; an empty set too" \
   "live a593c30606c443914 i1 } named]|live " "$(tsrows)"
rm -rf "$TSD/state"
ts sub-reconcile "{\"session_id\":\"$TSS\",\"hook_event_name\":\"Stop\",\"last_assistant_message\":\"end\"}" >/dev/null
ts sub-reconcile "$(tsfin '{"id":"../x","type":"subagent"}')" >/dev/null
ts sub-reconcile "$(tsfin '{"id":"a1","type":"subagent"}' | head -c 80)" >/dev/null
TSMANY=$(for i in $(seq 1 300); do printf '{"id":"a%016d","type":"subagent"},' "$i"; done)
ts sub-reconcile "$(tsfin "${TSMANY%,}")" >/dev/null
is "no array, a bad id, a cut payload, a set past one atomic write: no log" "0" "$(find "$TSD/state" -type f 2>/dev/null | grep -c .)"
rm -rf "$TSD/state"
ts sub-reconcile "$(tsfin '{"id":"a1","type":"subagent","tags":[]},{"id":"a2","type":"subagent"}')" >/dev/null
ts sub-reconcile "$(tsfin '{"id":"a1","type":"subagent","m":{"k":1}},{"id":"a2","type":"subagent"}')" >/dev/null
is "a nested array or object in an entry writes no row, never a short set" "0" "$(find "$TSD/state" -type f 2>/dev/null | grep -c .)"
TSHUGE=$(for i in $(seq 1 2000); do printf '{"id":"b%d","type":"shell","status":"running","description":"sleep %d"},' "$i" "$i"; done)
TSSHELLS=$(for i in $(seq 1 600); do printf '{"id":"b%d","type":"shell","status":"running","description":"sleep %d"},' "$i" "$i"; done)
TSMANY=$(for i in $(seq 1 600); do printf '{"id":"a%016d","type":"subagent","status":"running"},' "$i"; done)
tst0=$(date +%s)
for a in "$TSHUGE" "$TSSHELLS" "$TSMANY"; do ts sub-reconcile "$(tsfin "${a%,}")" >/dev/null; done
tst1=$(date +%s)
is "2000 tasks, 600 shells, 600 subagents past one write: under 3 s together" "fast" "$([ $((tst1 - tst0)) -lt 3 ] && echo fast || echo "$((tst1 - tst0)) s")"
is "and of them only the shells write a row, an empty set" "live " "$(tsrows)"
rm -rf "$TSD/state"
ts sub-taskstop "$(tsstop b1 b1 local_bash 'i1 named')" >/dev/null
ts sub-taskstop "$(tsstop m1 m1 monitor 'i1 named')" >/dev/null
is "a shell's or a monitor's TaskStop writes no row (only a teammate's is kill-tm)" "0" "$(find "$TSD/state" -type f 2>/dev/null | grep -c .)"
TSBIN="$TSD/bin"; mkdir -p "$TSBIN"; printf '#!/bin/sh\ntouch "%s/tmux-ran"\n' "$TSD" > "$TSBIN/tmux"; chmod +x "$TSBIN/tmux"
printf '%s\n' "$(tsstop a1 a1 local_agent x)" | env PATH="$TSBIN:$PATH" TMUX_PANE=%1 XDG_STATE_HOME="$TSD/state" "$DIR/scripts/agent-state" sub-taskstop >/dev/null 2>&1
printf '%s\n' "$(tsfin '')" | env PATH="$TSBIN:$PATH" TMUX_PANE=%1 XDG_STATE_HOME="$TSD/state" "$DIR/scripts/agent-state" sub-reconcile >/dev/null 2>&1
is "neither calls tmux" "" "$(ls "$TSD/tmux-ran" 2>/dev/null)"

echo "== install.sh --hooks: the TaskStop matcher and the Stop reconcile merge in once, beside a user's own Stop hook (#403) =="
if command -v jq >/dev/null 2>&1; then
TIH=$(mktmpd); mkdir -p "$TIH/.claude"
# A box from before #403: master's cockpit hooks, plus a Stop hook of the user's own.
jq '{hooks: (.hooks | .PostToolUse |= map(select(.matcher != "TaskStop"))
    | .Stop |= (map(select(.hooks[0].command | test("sub-reconcile") | not))
                + [{matcher: "", hooks: [{type: "command", command: "my-own-stop-hook"}]}]))}' \
    "$DIR/examples/claude-hooks.json" > "$TIH/.claude/settings.json"
tirun() { env -u TMUX -u TMUX_PANE HOME="$TIH" XDG_BIN_HOME="$TIH/bin" XDG_CONFIG_HOME="$TIH/cfg" TMUX_TMPDIR="$TIH" "$DIR/install.sh" "$@" 2>&1; }
tisub() { tirun --check | sed -n 's/^\[ *\([a-z]*\) *\] *subhooks *\(.*\)/\1 \2/p' | sed 's/  */ /g; s/ $//'; }
is "--check before: 4 of 6, and it says to re-run --hooks" "warn 4 of 6 subagent hooks merged re-run install.sh --hooks" "$(tisub)"
tirun --hooks >/dev/null; tirun --hooks >/dev/null
tin() { jq -r --arg ev "$1" --arg m "$2" --arg c "$3" '[.hooks[$ev][]? | select((.matcher // "") == $m) | .hooks[].command | select(test("agent-state " + $c + "$"))] | length' "$TIH/.claude/settings.json"; }
is "--hooks twice: sub-taskstop once under PostToolUse TaskStop, sub-reconcile once under Stop" "1 1" \
   "$(tin PostToolUse TaskStop sub-taskstop) $(tin Stop '' sub-reconcile)"
is "and the user's own Stop hook and cockpit's done and review-gate are all still there" "1 1 1" \
   "$(jq '[.. | strings | select(. == "my-own-stop-hook")] | length' "$TIH/.claude/settings.json") $(tin Stop '' done) $(jq '[.hooks.Stop[].hooks[].command | select(test("review-gate$"))] | length' "$TIH/.claude/settings.json")"
is "--check after: all 6" "ok all 6 subagent hooks merged" "$(tisub)"
fi

printf '  %d passed, %d failed, %d skipped\n' "$pass" "$fail" "$skipped"
[ "$fail" -eq 0 ] || exit 1
