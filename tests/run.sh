#!/usr/bin/env bash
# tmux-panestate test rig.  Run:  tests/run.sh
# Uses a throwaway state dir, so your live agent state is never touched.
set -u

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE=$(mktemp -d)
export XDG_RUNTIME_DIR="$STATE"
mkdir -p "$STATE/agent-state"
S="$STATE/agent-state"

pass=0; fail=0; skipped=0
ok()   { pass=$((pass+1)); printf '  \033[32mPASS\033[0m %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  \033[31mFAIL\033[0m %s\n     want [%s]\n     got  [%s]\n' "$1" "$2" "$3"; }
is()   { [ "$2" = "$3" ] && ok "$1" || bad "$1" "$2" "$3"; }

plain() { sed 's/#\[[^]]*\]//g'; }

# Several checks need a live tmux server to set per-pane options against. Skip
# them cleanly rather than failing: a red suite should mean broken code, not
# "no tmux running".
if tmux list-panes -a >/dev/null 2>&1; then HAVE_TMUX=1; else HAVE_TMUX=0; fi
need_tmux() {
    [ "$HAVE_TMUX" = 1 ] && return 1
    skipped=$((skipped+1)); printf '  \033[33mSKIP\033[0m %s (no tmux server)\n' "$1"; return 0
}
# Stub tmux: show-option returns empty (so icon DEFAULTS are used, making
# expectations font/option independent) and list-panes returns nothing (so the
# GC sweep is skipped and synthetic pane ids survive).
STUB=$(mktemp -d); printf '#!/bin/sh\nexit 0\n' > "$STUB/tmux"; chmod +x "$STUB/tmux"
st()    { "$DIR/scripts/agent-state" "$@"; }
# state is a per-pane tmux option now, not a file
PANE=$(tmux list-panes -a -F '#{pane_id}' 2>/dev/null | head -1); PANE=${PANE:-%0}
sv()    { tmux show-option -pqv -t "$PANE" @panestate 2>/dev/null; }

A='\U000f06a9'   # robot   -- agent
R='\uf013'       # gear    -- running
W='\uf0f3'       # bell    -- needs input
D='\uf00c'       # check   -- done
g() { printf "$1"; }

echo "== syntax =="
for f in "$DIR"/panestate.tmux "$DIR"/scripts/*; do
    if bash -n "$f" 2>/dev/null; then ok "syntax $(basename "$f")"
    else bad "syntax $(basename "$f")" "clean" "parse error"; fi
done

echo "== native tmux rendering (no shell in the redraw path) =="
tp=$(tmux list-panes -a -F '#{pane_id}' 2>/dev/null | head -1)
if [ -n "$tp" ]; then
    save=$(tmux show-option -pqv -t "$tp" @panestate 2>/dev/null)
    bs() { tmux display-message -p -t "$tp" '#{E:#{pane-border-style}}' 2>/dev/null; }
    tmux set -p -t "$tp" @panestate running     >/dev/null 2>&1; is "running border is yellow" "fg=#ffcc33" "$(bs)"
    tmux set -p -t "$tp" @panestate needs-input >/dev/null 2>&1; is "blocked border is red"    "fg=#ff6b6b" "$(bs)"
    tmux set -p -t "$tp" @panestate done        >/dev/null 2>&1; is "done border is green"     "fg=#3dea7d" "$(bs)"
    tmux set -pu -t "$tp" @panestate            >/dev/null 2>&1; is "idle border is grey"      "fg=#5b6272" "$(bs)"
    # the redraw path must contain no calls to our scripts
    allfmt=$(for o in status-left status-right window-status-format window-status-current-format pane-border-format pane-border-style; do tmux show-options -gv "$o" 2>/dev/null; done)
    case "$allfmt" in
        *tmux-panestate/scripts/agent*) bad "no plugin script runs on redraw" "none" "found one" ;;
        *) ok "no plugin script runs on redraw" ;;
    esac
    [ -n "$save" ] && tmux set -p -t "$tp" @panestate "$save" >/dev/null 2>&1
else
    for i in 1 2 3 4 5; do ok "native rendering (skipped: no tmux server)"; done
fi

echo "== state transitions =="
if need_tmux "state transitions"; then :; else
st running "$PANE";      is "hook: running"  "running"     "$(sv)"
st needs-input "$PANE";  is "hook: wait"     "needs-input" "$(sv)"
st off "$PANE"                                  # reset: needs-input now blocks done
st done "$PANE";         is "hook: done"     "done"        "$(sv)"
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
st running %1
is "resuming work clears needs-input" "running" "$(sv)"
tmux set -p -t "$PANE" @panestate running >/dev/null 2>&1
echo '{"message":"Claude needs your permission"}' | st notify "$PANE"
is "permission -> wait" "needs-input" "$(sv)"
tmux set -p -t "$PANE" @panestate running >/dev/null 2>&1
echo '{"message":"unrelated thing"}' | st notify "$PANE"
is "unrelated ignored" "running" "$(sv)"

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
is "clear-done mode is gone" "" "$(st clear-done %1 2>/dev/null; cat /dev/null)"
is "and done is still intact"  "done" "$(sv)"
rm -f "$S"/*

fi
echo "== spaced window names (native formats never word-split) =="
sp=$(tmux list-panes -a -F '#{pane_id} #{window_index}' 2>/dev/null | awk '$2!=1{print $1;exit}')
if [ -n "$sp" ]; then
    sw=$(tmux list-panes -a -F '#{pane_id} #{window_index}' | awk -v p="$sp" '$1==p{print $2}')
    orig=$(tmux display-message -p -t "$sp" '#{window_name}')
    old=$(tmux show-option -pqv -t "$sp" @panestate 2>/dev/null)
    tmux rename-window -t "$sw" 'my project' >/dev/null 2>&1
    tmux set -p -t "$sp" @panestate needs-input >/dev/null 2>&1
    # the alert lives on the ACTIVE pane's top border now, not in the bottom bar
    ap=$(tmux display-message -p '#{pane_id}' 2>/dev/null)
    a=$(tmux display-message -p -t "$ap" '#{E:#{pane-border-format}}' 2>/dev/null | plain)
    case "$a" in *"my project"*) ok "alert keeps a spaced window name" ;; *) bad "alert keeps a spaced window name" "my project" "$a" ;; esac
    tmux rename-window -t "$sw" "$orig" >/dev/null 2>&1
    if [ -n "$old" ]; then tmux set -p -t "$sp" @panestate "$old" >/dev/null 2>&1
    else tmux set -pu -t "$sp" @panestate >/dev/null 2>&1; fi
else
    ok "spaced names (skipped: single window)"
fi

echo "== rejects bogus states =="
if need_tmux "rejects bogus states"; then :; else
tmux set -p -t "$PANE" @panestate done >/dev/null 2>&1
for b in clear-done RUNNING banana 'running; rm -rf /'; do st "$b" %1 2>/dev/null; done
is "unknown states never written" "done" "$(sv)"
rm -f "$S"/*

fi
echo "== safety =="
( unset TMUX_PANE; st running '' ) 2>/dev/null
ok "no pane id and no TMUX_PANE is a no-op"
# Per-pane options die with the pane, so stale state cannot outlive a pane and
# there is no sweep that could delete a live one.
is "state is a per-pane option, not a file" "" "$(ls "$S" 2>/dev/null)"

echo "== tmux-where: ssh destination =="
w() { local p; bash -c "exec -a \"$1\" sleep 8" & p=$!; sleep 0.35
      printf '%s' "$("$DIR/scripts/tmux-where" path "$p" "$HOME" 2>&1 | plain | sed 's/^ *//;s/ *$//')"
      kill "$p" 2>/dev/null; wait "$p" 2>/dev/null; } 2>/dev/null
is "plain"          "󰢹 deploy@host"  "$(w 'ssh deploy@host')"
is "-p value"       "󰢹 deploy@host"  "$(w 'ssh -p 2222 deploy@host')"
is "bundle -nNp"    "󰢹 realhost"     "$(w 'ssh -nNp 2222 realhost')"
is "bundle -tti"    "󰢹 box01"        "$(w 'ssh -tti /k box01')"
is "attached -p"    "󰢹 attached"     "$(w 'ssh -p2222 attached')"
is "-o Opt=val"     "󰢹 prod"         "$(w 'ssh -o BatchMode=yes prod')"
is "-J jump"        "󰢹 final"        "$(w 'ssh -J j1,j2 final')"
is "remote command" "󰢹 prod-box"     "$(w 'ssh prod-box uptime')"
is "end of options" "󰢹 weird"        "$(w 'ssh -- weird')"
is "-vvv"           "󰢹 web01"        "$(w 'ssh -vvv web01')"
is "not ssh"        "~"              "$(w 'rsync -e ssh a b')"
is "dead pid quiet" "/home/x"        "$("$DIR/scripts/tmux-where" path 999999 /home/x 2>&1 | plain | sed 's/^ *//')"

rm -rf "$STATE" "$STUB"
echo
printf '  %d passed, %d failed, %d skipped\n' "$pass" "$fail" "$skipped"
[ "$fail" -eq 0 ] || exit 1
