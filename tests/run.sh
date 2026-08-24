#!/usr/bin/env bash
# tmux-cockpit test rig.  Run:  tests/run.sh
# Uses a throwaway state dir, so your live agent state is never touched.
set -u

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE=$(mktemp -d)
export XDG_RUNTIME_DIR="$STATE"
mkdir -p "$STATE/agent-state"

pass=0; fail=0; skipped=0
ok()   { pass=$((pass+1)); printf '  \033[32mPASS\033[0m %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  \033[31mFAIL\033[0m %s\n     want [%s]\n     got  [%s]\n' "$1" "$2" "$3"; }
is()   { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "$2" "$3"; fi; }

plain() { sed 's/#\[[^]]*\]//g'; }

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
for v in thm_red:#ff6b6b thm_peach:#ff9838 thm_yellow:#ffcc33 thm_green:#3dea7d thm_overlay_0:#5b6272; do
    "$TMUX_REAL" -L "$SUITE_SOCK" set -g "@${v%%:*}" "${v#*:}" >/dev/null 2>&1
done
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
for f in "$DIR"/cockpit.tmux "$DIR"/install.sh "$DIR"/scripts/*; do
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
for v in thm_red:#ff6b6b thm_peach:#ff9838 thm_yellow:#ffcc33 thm_green:#3dea7d thm_overlay_0:#5b6272; do
    tmux -L "$SOCK" set -g "@${v%%:*}" "${v#*:}" >/dev/null 2>&1
done
tmux -L "$SOCK" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
rp=$(tmux -L "$SOCK" list-panes -a -F '#{pane_id}' 2>/dev/null | head -1)
rbs() { tmux -L "$SOCK" display-message -p -t "$rp" '#{E:#{pane-border-style}}' 2>/dev/null; }
if [ -n "$rp" ]; then
    tmux -L "$SOCK" set -p -t "$rp" @panestate running     >/dev/null 2>&1; is "repo config: running is yellow" "fg=#ffcc33" "$(rbs)"
    tmux -L "$SOCK" set -p -t "$rp" @panestate needs-input >/dev/null 2>&1; is "repo config: blocked is red"    "fg=#ff6b6b" "$(rbs)"
    tmux -L "$SOCK" set -p -t "$rp" @panestate error       >/dev/null 2>&1; is "repo config: error is orange"   "fg=#ff9838" "$(rbs)"
    tmux -L "$SOCK" set -p -t "$rp" @panestate done        >/dev/null 2>&1; is "repo config: done is green"     "fg=#3dea7d" "$(rbs)"
    tmux -L "$SOCK" set -pu -t "$rp" @panestate            >/dev/null 2>&1; is "repo config: idle is grey"      "fg=#5b6272" "$(rbs)"
    # the shipped config must carry the stale-state guard, not just the live one
    case "$(tmux -L "$SOCK" show-options -gv pane-border-style)" in
        *pane_current_command*) ok "repo config guards against stale state" ;;
        *) bad "repo config guards against stale state" "pane_current_command" "absent" ;;
    esac
else
    for i in 1 2 3 4 5 6; do skip_ "repo config (no scratch server)"; done
fi
tmux -L "$SOCK" kill-server >/dev/null 2>&1
fi

echo "== the shipped config is usable as shipped =="
# The block above asserts colours, which pass whether or not the #{agent_*}
# tokens were ever rewritten. They were not: run-shell blocks the command queue,
# so loading cockpit.tmux at the TOP of the file made it read options that did
# not exist yet and skip them, and everyone who copied the block verbatim got
# the literal token printed in their status bar. Source the file the way a user
# does and check the result, not the intent.
if need_tmux "shipped config is usable"; then :; else
SOCK4="cockpit-shipped-$$"; SOCKS="$SOCKS $SOCK4"
tmux -L "$SOCK4" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$SOCK4" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
dt=$(tmux -L "$SOCK4" show-option -gqv @catppuccin_directory_text)
case "$dt" in
    *'#{agent_where_path}'*) bad "sourcing it interpolates the path token" "a #(...) call" "$dt" ;;
    *tmux-where*)            ok  "sourcing it interpolates the path token" ;;
    *)                       bad "sourcing it interpolates the path token" "a #(...) call" "$dt" ;;
esac
gt=$(tmux -L "$SOCK4" show-option -gqv @catppuccin_gitmux_text)
case "$gt" in
    *tmux-where*) ok  "and the branch token" ;;
    *)            bad "and the branch token" "a #(...) call" "$gt" ;;
esac
# Reloading ~/.tmux.conf is routine; -a would stack a second clickable segment
# on every reload, and a second interpolation on top of the first.
tmux -L "$SOCK4" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
is "reloading does not stack the click range" "1" \
   "$(tmux -L "$SOCK4" show-option -gqv status-right | grep -o 'range=control|1' | wc -l)"
is "reloading does not re-interpolate" "1" \
   "$(tmux -L "$SOCK4" show-option -gqv @catppuccin_directory_text | grep -o 'tmux-where' | wc -l)"
# pane_title defaults to the HOSTNAME. Claude sets a title; aider, codex, gemini
# and crush generally do not, so without a guard every one of those tabs reads
# the same hostname instead of its window name.
tmux -L "$SOCK4" new-window -d -n buildwin 'exec -a aider sleep 300' >/dev/null 2>&1
wt=$(tmux -L "$SOCK4" display-message -p -t buildwin '#{E:@catppuccin_window_text}' 2>/dev/null | plain)
case "$wt" in
    *"$(hostname -s 2>/dev/null)"*) bad "an agent with no title shows its window name" "buildwin" "$wt" ;;
    *buildwin*)                     ok  "an agent with no title shows its window name" ;;
    *)                              bad "an agent with no title shows its window name" "buildwin" "$wt" ;;
esac
tmux -L "$SOCK4" kill-server >/dev/null 2>&1
fi

echo "== the shipped config against a real agent pane =="
tp="$PANE"; [ "$HAVE_TMUX" = 1 ] || tp=""
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
echo "== stale state is disbelieved =="
if need_tmux "stale state"; then :; else
# Hooks cannot fire if an agent is killed or crashes, so @panestate can outlive
# the agent. The formats only believe it while the pane is actually running one.
w=$(tmux new-window -d -P -F '#{window_index}' -n cockpit-test 'bash' 2>/dev/null)
if [ -n "$w" ]; then
    sleep 0.6
    tp2=$(tmux list-panes -t ":$w" -F '#{pane_id}' 2>/dev/null | head -1)
    tmux set -p -t "$tp2" @panestate running >/dev/null 2>&1
    is "option is still set"        "running"     "$(tmux show-option -pqv -t "$tp2" @panestate)"
    is "but a shell pane reads idle" "fg=#5b6272" "$(tmux display-message -p -t "$tp2" '#{E:#{pane-border-style}}' 2>/dev/null)"
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
echo "== spaced window names survive the formats =="
if need_tmux "spaced window names"; then :; else
sw=$(tmux new-window -d -P -F '#{window_index}' -n 'my project' 'sleep 30' 2>/dev/null)
if [ -n "$sw" ]; then
    sleep 0.4
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


echo "== tmux-open-path: which token on the line is a file =="
# A scratch tree with the shapes real output prints. --dry-run resolves without
# touching tmux and without opening anything.
OP=$(mktmpd); mkdir -p "$OP/src" "$OP/bin"
: > "$OP/src/foo-bar.ts"; : > "$OP/README.md"; : > "$OP/bin/tm"
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
rm -rf "$OP"

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
case "$(git -C "$BR" switch feature/one 2>&1)" in
    *"already used by worktree"*) ok "worktree conflict names the worktree" ;;
    *) bad "worktree conflict names the worktree" "already used by worktree" "$(git -C "$BR" switch feature/one 2>&1)" ;;
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
for v in thm_red:#ff6b6b thm_peach:#ff9838 thm_yellow:#ffcc33 thm_green:#3dea7d thm_overlay_0:#5b6272; do
    tmux -L "$SOCK3" set -g "@${v%%:*}" "${v#*:}" >/dev/null 2>&1
done
tmux -L "$SOCK3" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
ap=$(tmux -L "$SOCK3" list-panes -a -F '#{pane_id}' 2>/dev/null | head -1)
abs() { tmux -L "$SOCK3" display-message -p -t "$ap" '#{E:#{pane-border-style}}' 2>/dev/null; }
tmux -L "$SOCK3" set -p -t "$ap" @panestate running >/dev/null 2>&1

is "the plugin defaults it rather than leaving it empty" \
   '^(claude|codex|aider|cursor|opencode|gemini|crush)$' \
   "$(tmux -L "$SOCK3" show-option -gqv @cockpit_agents)"
is "and derives the glob list the formats read" \
   '|claude|codex|aider|cursor|opencode|gemini|crush|' \
   "$(tmux -L "$SOCK3" show-option -gqv @cockpit_agents_glob)"
is "with the default, a claude pane is believed" "fg=#ffcc33" "$(abs)"
tmux -L "$SOCK3" set -g @cockpit_agents_glob '|vim|emacs|' >/dev/null 2>&1
is "narrowing it disbelieves the pane"          "fg=#5b6272" "$(abs)"
tmux -L "$SOCK3" set -g @cockpit_agents_glob '|claude|vim|' >/dev/null 2>&1
is "widening it believes the pane again"        "fg=#ffcc33" "$(abs)"
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
: > "$LOG"
env -i PATH="$ETMUX" HOME="$HOME" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt" \
    EDITOR=vi bash "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
case "$(cat "$LOG")" in
    *NEWWIN*vi*) ok "no code(1): falls back to \$EDITOR" ;;
    *) bad "no code(1): falls back to \$EDITOR" "a new window running vi" "$(cat "$LOG")" ;;
esac

# ...and with no editor at all it must say so, not claim success.
: > "$LOG"
env -i PATH="$ETMUX" HOME="$HOME" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt" \
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
case "$(printf '%s' "$mk" | sed -n 2p)" in
    "▸"*) ok "the target line starts with the marker" ;;
    *) bad "the target line starts with the marker" "a leading ▸" "$(printf '%s' "$mk" | sed -n 2p)" ;;
esac
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

# later blocks reuse this stub
ETMUX_TEMPLATE=$(mktmpf); cp "$ETMUX/tmux" "$ETMUX_TEMPLATE"; chmod +x "$ETMUX_TEMPLATE"
rm -rf "$EQ" "$ETMUX"

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

echo "== tm: the kill picker and the name cleaner =="
# Without fzf the picker is a `select`, and Ctrl-D leaves the choice variable
# unset. Under `set -u` the very next line -- ${#doomed[@]} -- aborted the
# script with "unbound variable" instead of exiting quietly.
TMS=$(mktmpd)
for c in readlink bash; do ln -s "$(command -v $c)" "$TMS/$c"; done
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
printf -- '-weird name\n' | env -u TMUX PATH="$TMS" TM_LOG="$TMS/log" "$DIR/scripts/tm" >/dev/null 2>&1
is "a leading dash never reaches tmux" "NEW new-session -d -s weird_name" "$(cat "$TMS/log")"
rm -rf "$TMS"

echo "== cockpit.tmux: every option carrying a token is interpolated =="
# The README documents #{agent_where_path} as usable in status-left/status-right,
# but the rewrite loop only ever visited two @catppuccin_* options, so anyone
# following the README got the literal token printed in their status bar.
if need_tmux "token interpolation"; then :; else
SOCK2="cockpit-interp-$$"; SOCKS="$SOCKS $SOCK2"
tmux -L "$SOCK2" -f /dev/null new-session -d -s t >/dev/null 2>&1
tmux -L "$SOCK2" set -g status-right "L #{agent_where_path} R"     >/dev/null 2>&1
tmux -L "$SOCK2" set -g status-left  "#{agent_where_branch}"       >/dev/null 2>&1
tmux -L "$SOCK2" set -g @catppuccin_gitmux_text "#{agent_where_branch}" >/dev/null 2>&1
tmux -L "$SOCK2" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
for o in status-right status-left @catppuccin_gitmux_text; do
    v=$(tmux -L "$SOCK2" show-option -gqv "$o" 2>/dev/null)
    case "$v" in
        *'#{agent_'*) bad "$o is interpolated" "a #(...) shell call" "$v" ;;
        *tmux-where*) ok  "$o is interpolated" ;;
        *)            bad "$o is interpolated" "a #(...) shell call" "$v" ;;
    esac
done
# An option with no token must be left exactly as it was.
tmux -L "$SOCK2" set -g status-style "bg=red" >/dev/null 2>&1
tmux -L "$SOCK2" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "untouched options stay untouched" "bg=red" "$(tmux -L "$SOCK2" show-option -gqv status-style)"
tmux -L "$SOCK2" kill-server >/dev/null 2>&1
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
rm -rf "$DH"
echo "== install.sh only ever touches its own links =="
IB=$(mktmpd); IL="$IB/link"; ln -s "$DIR" "$IL"; IBIN="$IB/bin"; mkdir -p "$IBIN"
XDG_BIN_HOME="$IBIN" "$IL/install.sh" >/dev/null 2>&1
is "installs through a symlinked plugin dir" "3" "$(ls "$IBIN" | wc -l)"
# DIR came from a LOGICAL pwd while the guard compared readlink -f of the link,
# so reaching the plugin through a symlink -- a dotfiles checkout linked into
# ~/.tmux/plugins is the usual way -- made --uninstall remove nothing, silently.
XDG_BIN_HOME="$IBIN" "$IL/install.sh" --uninstall >/dev/null 2>&1
is "and uninstalls through one too" "0" "$(ls "$IBIN" | wc -l)"
# A same-named symlink owned by another tool must be left alone: "tm" is a name
# plenty of things want, and --uninstall would later have deleted it as well.
XDG_BIN_HOME="$IBIN" "$DIR/install.sh" >/dev/null 2>&1
rm -f "$IBIN/tm"; ln -s /bin/echo "$IBIN/tm"
XDG_BIN_HOME="$IBIN" "$DIR/install.sh" >/dev/null 2>&1
is "leaves another tool's link alone" "/bin/echo" "$(readlink "$IBIN/tm")"
rm -rf "$IB"

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
start=$SECONDS
( for i in 1 2 3 4 5 6 7 8; do printf 'drip %s\n' "$i"; sleep 1; done ) \
    | "$DIR/scripts/agent-state" notify %999999 >/dev/null 2>&1
took=$((SECONDS - start))
if [ "$took" -le 4 ]; then ok "a slow drip does not hold the hook open"
else bad "a slow drip does not hold the hook open" "<=4s" "${took}s"; fi

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
tmux -L "$SOCK5" send-keys -t "$KP" "'$DIR/scripts/tmux-branch' '$KB'" Enter
sleep 2
# The list marks the branch you are ON; the header alone said it before, and
# the current branch looked like a normal destination.
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
sleep 1
tmux -L "$SOCK5" send-keys -t "$KP" C-n      # emacs move-down, NOT create
sleep 1
tmux -L "$SOCK5" send-keys -t "$KP" Escape   # leave without switching
sleep 1
after=$(git -C "$KB" for-each-ref --format='%(refname:short)' refs/heads | sort | tr '\n' ' ')
is "ctrl-n after typing creates no branch" "main second third " "$after"
is "and the checkout is untouched" "main" "$(git -C "$KB" branch --show-current)"
# A branch held by a linked worktree is labelled, and a FAILED action returns
# to the picker instead of dying at "[any key]". The failure driven here is the
# deterministic one -- an invalid name via alt-enter -- because driving a git
# refusal through a pty raced fzf's own startup and the test keyed the wrong
# row. Waiting for the prompt before typing fixes the label half too.
git -C "$KB" worktree add -q "$KB-wt" second 2>/dev/null
tmux -L "$SOCK5" send-keys -t "$KP" "'$DIR/scripts/tmux-branch' '$KB'" Enter
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
    tmux -L "$SOCK5" send-keys -t "$KP" "bad..name"; sleep 0.6
    tmux -L "$SOCK5" send-keys -t "$KP" M-Enter; sleep 1.2     # create fails, pauses
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
    tmux -L "$SOCK5" send-keys -t "$KP" Escape; sleep 0.5
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
# SGR bytes (ESC [ < 16 ; 3 ; 1 M/m -- button 0 plus the ctrl bit) into it.
if need_tmux "path picker with a split"; then :; else
OUTER="cockpit-outer-$$"; INNER="cockpit-inner-$$"
SOCKS="$SOCKS $OUTER $INNER"
"$TMUX_REAL" -L "$INNER" -f /dev/null new-session -d -s t -x 80 -y 24 >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" set -g mouse on >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
"$TMUX_REAL" -L "$INNER" split-window -t t >/dev/null 2>&1
IP=$("$TMUX_REAL" -L "$INNER" list-panes -t t -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$INNER" send-keys -t "$IP" 'echo MARK README.md' Enter >/dev/null 2>&1
sleep 0.4
"$TMUX_REAL" -L "$INNER" set-option -p -t "$IP" @cockpit_click_line "" >/dev/null 2>&1
"$TMUX_REAL" -L "$OUTER" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $INNER attach -t t" >/dev/null 2>&1
OP=$("$TMUX_REAL" -L "$OUTER" list-panes -t o -F '#{pane_id}' | head -1)
sleep 1.5
CDBL='1b 5b 3c 31 36 3b 33 3b 31 4d 1b 5b 3c 31 36 3b 33 3b 31 6d'
# The client has to be up, or the keys go nowhere and the assertion passes for
# the wrong reason -- twice today a test of mine did exactly that.
if "$TMUX_REAL" -L "$OUTER" list-panes -t o >/dev/null 2>&1; then
    "$TMUX_REAL" -L "$OUTER" send-keys -t "$OP" -H $CDBL >/dev/null 2>&1; sleep 0.05
    "$TMUX_REAL" -L "$OUTER" send-keys -t "$OP" -H $CDBL >/dev/null 2>&1; sleep 1.5
    got=$("$TMUX_REAL" -L "$INNER" show-options -p -t "$IP" -qv @cockpit_click_line)
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
out=$(XDG_BIN_HOME="$RO" "$DIR/install.sh" 2>&1); rc=$?
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
"$TMUX_REAL" -L "$GI" -f /dev/null new-session -d -s t -x 80 -y 24 >/dev/null 2>&1
"$TMUX_REAL" -L "$GI" set -g mouse on >/dev/null 2>&1
"$TMUX_REAL" -L "$GI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
GP=$("$TMUX_REAL" -L "$GI" list-panes -t t -F '#{pane_id}' | head -1)
# Turn on mouse reporting in the pane, exactly what Claude Code does.
"$TMUX_REAL" -L "$GI" send-keys -t "$GP" "echo MARK README.md; printf '\\e[?1000h'" Enter >/dev/null 2>&1
sleep 0.5
is "the pane really reports mouse mode" "1" \
   "$("$TMUX_REAL" -L "$GI" display-message -p -t "$GP" '#{mouse_any_flag}')"
"$TMUX_REAL" -L "$GI" set-option -p -t "$GP" @cockpit_click_line "" >/dev/null 2>&1
"$TMUX_REAL" -L "$GO" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $GI attach -t t" >/dev/null 2>&1
GOP=$("$TMUX_REAL" -L "$GO" list-panes -t o -F '#{pane_id}' | head -1)
sleep 1.5
GSEQ='1b 5b 3c 31 36 3b 33 3b 31 4d 1b 5b 3c 31 36 3b 33 3b 31 6d'
"$TMUX_REAL" -L "$GO" send-keys -t "$GOP" -H $GSEQ >/dev/null 2>&1; sleep 0.05
"$TMUX_REAL" -L "$GO" send-keys -t "$GOP" -H $GSEQ >/dev/null 2>&1; sleep 1.5
case "$("$TMUX_REAL" -L "$GI" show-options -p -t "$GP" -qv @cockpit_click_line)" in
    *README.md*) ok "ctrl+double-click fires in a mouse-mode pane" ;;
    *) bad "ctrl+double-click fires in a mouse-mode pane" "the clicked line" \
         "$("$TMUX_REAL" -L "$GI" show-options -p -t "$GP" -qv @cockpit_click_line)" ;;
esac
"$TMUX_REAL" -L "$GO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$GI" kill-server >/dev/null 2>&1
fi

echo "== the border keeps tmux's own buttons =="
# Replacing pane-border-format wholesale silently deleted tmux's right-aligned
# zoom (control|8) and kill (control|9) buttons from every pane border.
if need_tmux "border buttons"; then :; else
BB="cockpit-bb-$$"; SOCKS="$SOCKS $BB"
"$TMUX_REAL" -L "$BB" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
for v in thm_red:#ff6b6b thm_peach:#ff9838 thm_yellow:#ffcc33 thm_green:#3dea7d thm_overlay_0:#5b6272; do
    "$TMUX_REAL" -L "$BB" set -g "@${v%%:*}" "${v#*:}" >/dev/null 2>&1
done
"$TMUX_REAL" -L "$BB" set -g mouse on >/dev/null 2>&1
"$TMUX_REAL" -L "$BB" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
BP=$("$TMUX_REAL" -L "$BB" list-panes -a -F '#{pane_id}' | head -1)
# One pane: no [z] -- a lone pane already fills the window, so the zoom button
# toggled nothing and read as broken. [x] stays: killing the last pane closes
# the window, which is exactly what a close button should do there.
solo=$("$TMUX_REAL" -L "$BB" display-message -p -t t '#{E:pane-border-format}')
case "$solo" in
    *'control|8'*) bad "no zoom button on a lone pane" "control|9 only" "$solo" ;;
    *'control|9'*) ok "no zoom button on a lone pane" ;;
    *) bad "no zoom button on a lone pane" "an x button at least" "$solo" ;;
esac

# Split: the ACTIVE pane suppresses its own words (the prompt and the red
# border already say it -- same reasoning as the tint); a pane you are NOT on
# still spells it out on its border.
"$TMUX_REAL" -L "$BB" split-window -t t 'exec -a claude sleep 300' >/dev/null 2>&1
BP2=$("$TMUX_REAL" -L "$BB" list-panes -t t -F '#{pane_id} #{pane_active}' | awk '$2==1{print $1}')
BP1=$("$TMUX_REAL" -L "$BB" list-panes -t t -F '#{pane_id} #{pane_active}' | awk '$2==0{print $1}')
"$TMUX_REAL" -L "$BB" set -p -t "$BP1" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$BB" set -p -t "$BP2" @panestate needs-input >/dev/null 2>&1
bb=$("$TMUX_REAL" -L "$BB" display-message -p -t "$BP2" '#{E:pane-border-format}')
case "$bb" in *'range=control|8'*) ok "the zoom button survives" ;; *) bad "the zoom button survives" "range=control|8" "$bb" ;; esac
case "$bb" in *'range=control|9'*) ok "the kill button survives" ;; *) bad "the kill button survives" "range=control|9" "$bb" ;; esac
case "$(printf '%s' "$bb" | plain)" in
    *"NEEDS YOU"*) bad "the focused pane spells nothing out" "buttons only" "$(printf '%s' "$bb" | plain)" ;;
    *) ok "the focused pane spells nothing out" ;;
esac
nb=$("$TMUX_REAL" -L "$BB" display-message -p -t "$BP1" '#{E:pane-border-format}' | plain)
case "$nb" in
    *"NEEDS YOU"*) ok "an unfocused blocked pane still spells it out" ;;
    *) bad "an unfocused blocked pane still spells it out" "NEEDS YOU" "$nb" ;;
esac
"$TMUX_REAL" -L "$BB" kill-server >/dev/null 2>&1
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

echo "== prefix S opens the session picker =="
if need_tmux "prefix S"; then :; else
PS="cockpit-ps-$$"; SOCKS="$SOCKS $PS"
"$TMUX_REAL" -L "$PS" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$PS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; sleep 0.3
case "$("$TMUX_REAL" -L "$PS" list-keys -T prefix 2>/dev/null | awk '$4=="S"')" in
    *sessions*) ok "prefix S is bound to the sessions popup" ;;
    *) bad "prefix S is bound to the sessions popup" "a display-popup running tm" \
         "$("$TMUX_REAL" -L "$PS" list-keys -T prefix | awk '$4=="S"')" ;;
esac
"$TMUX_REAL" -L "$PS" kill-server >/dev/null 2>&1
fi

echo "== clicking the directory segment browses the working dir =="
# range=control|2 on the status bar's directory segment opens the folder
# browser at the pane's cwd -- the same popup the path picker descends into,
# entered from the top.
if need_tmux "directory click"; then :; else
DB="cockpit-db-$$"; SOCKS="$SOCKS $DB"
"$TMUX_REAL" -L "$DB" -f /dev/null new-session -d -s t -x 90 -y 24 -c "$DIR" >/dev/null 2>&1
"$TMUX_REAL" -L "$DB" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; sleep 0.3
case "$("$TMUX_REAL" -L "$DB" list-keys -T root 2>/dev/null | grep MouseDown1Control2)" in
    *--browse*) ok "control|2 is bound to the browser popup" ;;
    *) bad "control|2 is bound to the browser popup" "a display-popup running --browse" \
         "$("$TMUX_REAL" -L "$DB" list-keys -T root | grep MouseDown1Control2)" ;;
esac
DBLOG=$(mktmpf); DBED=$(mktmpf)
printf '#!/bin/sh\necho "OPEN $*" >> %s\n' "$DBLOG" > "$DBED"; chmod +x "$DBED"
"$TMUX_REAL" -L "$DB" set -g @cockpit_editor "$DBED" >/dev/null 2>&1
DP=$("$TMUX_REAL" -L "$DB" list-panes -t t -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$DB" send-keys -t "$DP" "'$DIR/scripts/tmux-open-path' --browse '$DIR'" Enter
sleep 2.2
case "$("$TMUX_REAL" -L "$DB" capture-pane -p -t "$DP")" in
    *"enter dir"*) ok "--browse opens the browser at the dir" ;;
    *) bad "--browse opens the browser at the dir" "the browse header" \
         "$("$TMUX_REAL" -L "$DB" capture-pane -p -t "$DP" | head -3)" ;;
esac
"$TMUX_REAL" -L "$DB" send-keys -t "$DP" "LICENSE"; sleep 0.8
"$TMUX_REAL" -L "$DB" send-keys -t "$DP" Enter; sleep 2
is "and a file opens from it" "OPEN $DIR/LICENSE" "$(cat "$DBLOG")"
"$TMUX_REAL" -L "$DB" kill-server >/dev/null 2>&1
fi

echo "== the tint never fires at the pane you are on =="
# The blocked-pane tint is an alert. Painting it onto the pane the user is
# focused on -- with the permission prompt already in front of them and the
# border already red -- is noise, and was reported as exactly that. The
# explicit bg=default on window-active-style is load-bearing: left unset, the
# active pane INHERITS window-style and the tint comes back (verified: 6 tinted
# rows unset, 4 with the explicit default).
if need_tmux "focused-pane tint"; then :; else
TS="cockpit-tint-$$"; SOCKS="$SOCKS $TS"
"$TMUX_REAL" -L "$TS" -f /dev/null new-session -d -s t -x 60 -y 14 'exec -a claude sleep 300' >/dev/null 2>&1
for v in thm_red:#ff6b6b thm_peach:#ff9838 thm_yellow:#ffcc33 thm_green:#3dea7d thm_overlay_0:#5b6272; do
    "$TMUX_REAL" -L "$TS" set -g "@${v%%:*}" "${v#*:}" >/dev/null 2>&1
done
"$TMUX_REAL" -L "$TS" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
"$TMUX_REAL" -L "$TS" split-window -t t 'exec -a claude sleep 300' >/dev/null 2>&1
TP1=$("$TMUX_REAL" -L "$TS" list-panes -t t -F '#{pane_id} #{pane_active}' | awk '$2==0{print $1}')
TP2=$("$TMUX_REAL" -L "$TS" list-panes -t t -F '#{pane_id} #{pane_active}' | awk '$2==1{print $1}')
"$TMUX_REAL" -L "$TS" set -p -t "$TP1" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$TS" set -p -t "$TP2" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$TS" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
is "a blocked pane you are NOT on still tints" "bg=#52303a" \
   "$("$TMUX_REAL" -L "$TS" display-message -p -t "$TP1" '#{E:window-style}')"
is "the pane you are on never tints"           "bg=default" \
   "$("$TMUX_REAL" -L "$TS" display-message -p -t "$TP2" '#{E:window-active-style}')"
case "$("$TMUX_REAL" -L "$TS" show-option -gv window-active-style)" in
    *@panestate*) bad "window-active-style carries no state format" "bg=default" "a @panestate conditional" ;;
    *) ok "window-active-style carries no state format" ;;
esac
"$TMUX_REAL" -L "$TS" kill-server >/dev/null 2>&1
fi

echo "== popup geometry is configurable =="
# One pair of options sizes both popups; set before the run-shell line, same
# pattern as @cockpit_agents. The path picker re-reads them at click time.
if need_tmux "popup geometry"; then :; else
PG="cockpit-geom-$$"; SOCKS="$SOCKS $PG"
"$TMUX_REAL" -L "$PG" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$PG" set -g @cockpit_popup_width 55% >/dev/null 2>&1
"$TMUX_REAL" -L "$PG" set -g @cockpit_popup_height 45% >/dev/null 2>&1
"$TMUX_REAL" -L "$PG" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; sleep 0.3
case "$("$TMUX_REAL" -L "$PG" list-keys -T root | grep MouseDown1Control1)" in
    *'-h "45%" -w "55%"'*) ok "a custom size reaches the binding" ;;
    *) bad "a custom size reaches the binding" '-h 45% -w 55%' \
         "$("$TMUX_REAL" -L "$PG" list-keys -T root | grep MouseDown1Control1)" ;;
esac
"$TMUX_REAL" -L "$PG" kill-server >/dev/null 2>&1
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
sleep 2.2
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
"$TMUX_REAL" -L "$MO" send-keys -t "$MP" Tab; sleep 0.4
"$TMUX_REAL" -L "$MO" send-keys -t "$MP" Tab; sleep 0.4
"$TMUX_REAL" -L "$MO" send-keys -t "$MP" Enter; sleep 2.5
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
sleep 2.2
"$TMUX_REAL" -L "$BR" send-keys -t "$BP" Enter          # the one candidate is a dir
sleep 2
case "$("$TMUX_REAL" -L "$BR" capture-pane -p -t "$BP")" in
    *"enter dir"*) ok "enter on a folder opens the browser, not the editor" ;;
    *) bad "enter on a folder opens the browser, not the editor" "the browse header" \
         "$("$TMUX_REAL" -L "$BR" capture-pane -p -t "$BP" | head -3)" ;;
esac
"$TMUX_REAL" -L "$BR" send-keys -t "$BP" "tmux-where"; sleep 0.8
"$TMUX_REAL" -L "$BR" send-keys -t "$BP" Enter; sleep 2
is "and a file inside it opens" "OPEN $DIR/scripts/tmux-where" "$(cat "$BRLOG")"
"$TMUX_REAL" -L "$BR" kill-server >/dev/null 2>&1
rm -f "$BRLOG" "$BRED"
else
  skip_ "enter on a folder opens the browser, not the editor"; skip_ "and a file inside it opens"
fi
fi

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
sleep 2.2
# A clicked "file:31" must read as file:31 in the list, not as a bare filename
# stripped of the very thing that anchored it.
case "$("$TMUX_REAL" -L "$OO" capture-pane -p -t "$OP")" in
    *"tm:30"*) ok "the row keeps its line number" ;;
    *) bad "the row keeps its line number" "scripts/tm:30" \
         "$("$TMUX_REAL" -L "$OO" capture-pane -p -t "$OP" | sed -n 3p)" ;;
esac
"$TMUX_REAL" -L "$OO" send-keys -t "$OP" Down; sleep 0.4
"$TMUX_REAL" -L "$OO" send-keys -t "$OP" C-o; sleep 2
is "^O hands the folder to the editor whole" "OPEN $DIR/scripts/" "$(cat "$OOLOG")"
"$TMUX_REAL" -L "$OO" kill-server >/dev/null 2>&1
rm -f "$OOLOG" "$OOED"
else
  skip_ "the row keeps its line number"; skip_ "^O hands the folder to the editor whole"
fi
fi

echo "== first-run: no theme, no catppuccin, still works =="
# The example config reads @thm_* unconditionally. A user without catppuccin
# had them unset, the border style collapsed to "fg=" -- which tmux REJECTS --
# and the headline feature silently never coloured anything. The suite itself
# used to set the palette by hand before sourcing, compensating for exactly
# the gap it should have caught; this block sources the config bare.
if need_tmux "bare first run"; then :; else
FR="cockpit-fr-$$"; SOCKS="$SOCKS $FR"
"$TMUX_REAL" -L "$FR" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FR" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
FP=$("$TMUX_REAL" -L "$FR" list-panes -a -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$FR" set -p -t "$FP" @panestate needs-input >/dev/null 2>&1
fb=$("$TMUX_REAL" -L "$FR" display-message -p -t "$FP" '#{E:#{pane-border-style}}')
case "$fb" in
    fg=) bad "the border colours without any theme installed" "a real colour" "the empty fg= tmux rejects" ;;
    fg=#*) ok "the border colours without any theme installed" ;;
    *) bad "the border colours without any theme installed" "fg=#......" "$fb" ;;
esac
# A window with no agent pane must carry NO marker -- ten windows with two
# agents used to show ten circles, and the circle meant two different things.
"$TMUX_REAL" -L "$FR" new-window -d -n plainwin 'sleep 300' >/dev/null 2>&1
pw=$("$TMUX_REAL" -L "$FR" display-message -p -t plainwin '#{E:@catppuccin_window_text}' | plain)
case "$pw" in
    *○*|*▲*|*✓*|*●*|*✕*) bad "a non-agent window carries no marker" "just the name" "$pw" ;;
    *plainwin*) ok "a non-agent window carries no marker" ;;
    *) bad "a non-agent window carries no marker" "the window name" "$pw" ;;
esac
# The cross-window alert is red and bold on its own account, not painted in
# whatever colour the ACTIVE pane's state happens to give the border.
"$TMUX_REAL" -L "$FR" new-window -d -n other 'exec -a claude sleep 300' >/dev/null 2>&1
OP2=$("$TMUX_REAL" -L "$FR" list-panes -t other -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$FR" set -p -t "$OP2" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$FR" set -p -t "$FP" @panestate running >/dev/null 2>&1
bf=$("$TMUX_REAL" -L "$FR" display-message -p -t "$FP" '#{E:pane-border-format}')
case "$bf" in
    *'#[fg=#f38ba8]#[bold]─ NEEDS YOU'*) ok "the NEEDS YOU banner is red regardless of this pane's state" ;;
    *) bad "the NEEDS YOU banner is red regardless of this pane's state" "a red bold banner" "$bf" ;;
esac
"$TMUX_REAL" -L "$FR" kill-server >/dev/null 2>&1
fi

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

echo "== tm: typing filters, ^X kills, the attached session survives =="
# A bare "k" -- the first key of filtering to "kanban" -- used to kill the
# highlighted session instantly, including the one you were attached to.
if need_tmux "tm picker"; then :; else
if command -v fzf >/dev/null 2>&1; then
TK="cockpit-tk-$$"; TKO="cockpit-tko-$$"; SOCKS="$SOCKS $TK $TKO"
"$TMUX_REAL" -L "$TK" -f /dev/null new-session -d -s alpha -x 90 -y 24 >/dev/null 2>&1
"$TMUX_REAL" -L "$TK" new-session -d -s beta >/dev/null 2>&1
# session_attached needs a real CLIENT; a detached server has none, so the
# attach comes through an outer pty server -- the same pattern as every other
# picker test here.
"$TMUX_REAL" -L "$TKO" -f /dev/null new-session -d -s o -x 90 -y 26 \
    "$TMUX_REAL -L $TK attach -t alpha" >/dev/null 2>&1
sleep 1.2
KP2=$("$TMUX_REAL" -L "$TK" list-panes -t alpha -F '#{pane_id}' | head -1)
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" "'$DIR/scripts/tm'" Enter
sleep 2
case "$("$TMUX_REAL" -L "$TK" capture-pane -p -t "$KP2")" in
    *"(attached)"*) ok "the attached session is labelled" ;;
    *) bad "the attached session is labelled" "alpha (attached)" \
         "$("$TMUX_REAL" -L "$TK" capture-pane -p -t "$KP2" | head -4)" ;;
esac
case "$("$TMUX_REAL" -L "$TK" capture-pane -p -t "$KP2")" in
    *" windows "*) ok "sessions preview their windows" ;;
    *) bad "sessions preview their windows" "the ' windows ' label" \
         "$("$TMUX_REAL" -L "$TK" capture-pane -p -t "$KP2" | grep -m1 '^─')" ;;
esac
# typing must FILTER now, not execute: "bet" narrows to beta
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" "bet"; sleep 0.8
is "typing filters instead of killing" "2" "$("$TMUX_REAL" -L "$TK" list-sessions 2>/dev/null | wc -l)"
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" C-x; sleep 1.2
is "^X kills the highlighted session" "alpha" "$("$TMUX_REAL" -L "$TK" list-sessions -F '#{session_name}' 2>/dev/null | tr -d '\n')"
# and ^X on the attached one is refused
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" C-u; sleep 0.4
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" C-x; sleep 1.2
is "the attached session refuses to die from the picker" "alpha" \
   "$("$TMUX_REAL" -L "$TK" list-sessions -F '#{session_name}' 2>/dev/null | tr -d '\n')"
"$TMUX_REAL" -L "$TK" send-keys -t "$KP2" Escape; sleep 0.5
"$TMUX_REAL" -L "$TKO" kill-server >/dev/null 2>&1
"$TMUX_REAL" -L "$TK" kill-server >/dev/null 2>&1
else
  skip_ "the attached session is labelled"; skip_ "typing filters instead of killing"
  skip_ "^X kills the highlighted session"; skip_ "the attached session refuses to die from the picker"
fi
fi

echo "== prefix J jumps to the pane that needs you =="
if need_tmux "tmux-jump"; then :; else
JJ="cockpit-jj-$$"; SOCKS="$SOCKS $JJ"
"$TMUX_REAL" -L "$JJ" -f /dev/null new-session -d -s t -x 80 -y 24 >/dev/null 2>&1
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
# sleep 2, not 1: tmux-jump forks a fresh nested tmux client per send-keys
# invocation here, and under concurrent load (several suites' worth of tmux
# servers on one box) that fork+connect can take noticeably longer than 1s.
JP1=$("$TMUX_REAL" -L "$JJ" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'" Enter; sleep 2
is "jump lands on the blocked window" "2" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')"
# A second blocked pane: pressing again from the first cycles to the next.
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP3" "'$DIR/scripts/tmux-jump'" Enter; sleep 2
is "a second press cycles to the next blocked pane" "1" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')"
# Nothing blocked: the script must not move you, and must not error.
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP3" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" select-window -t t:0 >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'; echo JRC=\$?" Enter; sleep 2
case "$("$TMUX_REAL" -L "$JJ" capture-pane -p -t "$JP1")" in
    *JRC=0*) ok "no blocked pane exits 0 and stays put" ;;
    *) bad "no blocked pane exits 0 and stays put" "JRC=0 in the pane" \
         "$("$TMUX_REAL" -L "$JJ" capture-pane -p -t "$JP1" | tail -3)" ;;
esac
is "and the window did not change" "0" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')"
"$TMUX_REAL" -L "$JJ" kill-server >/dev/null 2>&1
fi

printf '  %d passed, %d failed, %d skipped\n' "$pass" "$fail" "$skipped"
[ "$fail" -eq 0 ] || exit 1
