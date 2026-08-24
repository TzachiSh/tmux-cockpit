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

echo "== a directory name can no longer run code through the status job =="
# #{agent_where_path} used to embed pane_current_path inside the #() job with
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
tmux -L "$INNER" set -g status-right "#{agent_where_path}" >/dev/null 2>&1
tmux -L "$INNER" set -g status-interval 1 >/dev/null 2>&1
tmux -L "$INNER" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
tmux -L "$OUTER" -f /dev/null new-session -d -s o -x 100 -y 24 \
    "$TMUX_REAL -L $INNER attach -t s" >/dev/null 2>&1
sleep 2
tmux -L "$INNER" refresh-client >/dev/null 2>&1
sleep 1
if [ -e "$SP/OWNED" ]; then
    bad "a hostile cwd's \$(...) never runs" "no OWNED file" "OWNED file created"
else
    ok "a hostile cwd's \$(...) never runs"
fi
tmux -L "$OUTER" kill-server >/dev/null 2>&1
tmux -L "$INNER" kill-server >/dev/null 2>&1

# A normal cwd must still render, not just fail to execute.
D2=$(mktmpd)
OUTER2="cockpit-rce-outer2-$$"; INNER2="cockpit-rce-inner2-$$"
SOCKS="$SOCKS $OUTER2 $INNER2"
tmux -L "$INNER2" -f /dev/null new-session -d -s s -x 100 -y 24 -c "$D2" \
    'exec -a claude sleep 300' >/dev/null 2>&1
tmux -L "$INNER2" set -g status-right "#{agent_where_path}" >/dev/null 2>&1
tmux -L "$INNER2" set -g status-interval 1 >/dev/null 2>&1
tmux -L "$INNER2" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
tmux -L "$OUTER2" -f /dev/null new-session -d -s o -x 100 -y 24 \
    "$TMUX_REAL -L $INNER2 attach -t s" >/dev/null 2>&1
sleep 2
tmux -L "$INNER2" refresh-client >/dev/null 2>&1
sleep 1
scr=$(tmux -L "$OUTER2" capture-pane -p -t o)
case "$scr" in
    *"$D2"*) ok "a normal cwd still renders its path" ;;
    *) bad "a normal cwd still renders its path" "$D2 on the status line" "$scr" ;;
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
tmux -L "$INNER3" set -g status-right "#{agent_where_path}" >/dev/null 2>&1
tmux -L "$INNER3" set -g status-interval 1 >/dev/null 2>&1
tmux -L "$INNER3" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
tmux -L "$OUTER3" -f /dev/null new-session -d -s o -x 100 -y 24 \
    "$TMUX_REAL -L $INNER3 attach -t s" >/dev/null 2>&1
sleep 2
tmux -L "$INNER3" refresh-client >/dev/null 2>&1
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
# Ctrl+RIGHT-click is the collision-free picker gesture: Ghostty opens OSC 8
# links on the first Ctrl+LEFT-click and a GUI raced the popup; the right
# button reaches only us. Unbound in stock tmux, so nothing is displaced.
case "$("$TMUX_REAL" -L "$DB" list-keys -T root 2>/dev/null | grep C-MouseDown3Pane)" in
    *cockpit_click_line*) ok "ctrl+right-click drives the picker" ;;
    *) bad "ctrl+right-click drives the picker" "the click_line binding" \
         "$("$TMUX_REAL" -L "$DB" list-keys -T root | grep C-MouseDown3Pane)" ;;
esac
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

# --preview-diff used to capture the WHOLE coloured diff into a shell variable
# and run git diff a second time for --stat, both before ever checking the
# result was non-empty. Measured here: a 22.9MB/400000-line single-file diff
# was 3.32s + 191MB RSS on the pre-fix build, 0.76s + 76MB RSS after streaming
# the diff and deriving the stat from --numstat instead. timeout is the
# fail-first teeth -- the pre-fix build reliably misses a 2s budget here.
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
timeout 2 "$DIR/scripts/tmux-open-path" --preview-diff "$GD/big.txt" >"$gdout" 2>/dev/null
gdrc=$?
if [ "$gdrc" -eq 124 ]; then
    bad "a 400000-line single-file diff renders well under a 2s budget" \
        "output within 2s" "timed out (the pre-fix capture-and-rediff)"
else
    ok "a 400000-line single-file diff renders well under a 2s budget"
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
# Drive the block through agent-state, the production path: the banner now reads
# a per-window option agent-state precomputes on a transition, not a live pane
# walk, so a raw set-option would set @panestate but never light the banner.
TMUX="$SOCKDIR/$FR,0,0" "$DIR/scripts/agent-state" needs-input "$OP2" >/dev/null 2>&1
"$TMUX_REAL" -L "$FR" set -p -t "$FP" @panestate running >/dev/null 2>&1
bf=$("$TMUX_REAL" -L "$FR" display-message -p -t "$FP" '#{E:pane-border-format}')
case "$bf" in
    *'#[fg=#f38ba8]#[bold]─ NEEDS YOU'*) ok "the NEEDS YOU banner is red regardless of this pane's state" ;;
    *) bad "the NEEDS YOU banner is red regardless of this pane's state" "a red bold banner" "$bf" ;;
esac
"$TMUX_REAL" -L "$FR" kill-server >/dev/null 2>&1
fi

echo "== the precomputed banner renders byte-for-byte and excludes your own window =="
# A3 moved the cross-window NEEDS YOU walk out of pane-border-format (where a
# redraw re-ran it for every visible pane) into a per-window option agent-state
# writes on transitions. This pins the RENDER against the pre-A3 live walk: same
# text, same colours, the same live window_index:window_name, and the same rule
# that the window you are looking at is never listed -- its own blocked pane
# already spells it out on its own border. The golden strings below were
# captured from the pre-A3 code, so a match is byte-identical before/after.
if need_tmux "banner byte-identical"; then :; else
BX="cockpit-bx-$$"; SOCKS="$SOCKS $BX"
"$TMUX_REAL" -L "$BX" -f /dev/null new-session -d -s s -x 200 -y 50 'exec -a claude sleep 300' >/dev/null 2>&1
for v in thm_red:#ff6b6b thm_peach:#ff9838 thm_yellow:#ffcc33 thm_green:#3dea7d thm_overlay_0:#5b6272; do
    "$TMUX_REAL" -L "$BX" set -g "@${v%%:*}" "${v#*:}" >/dev/null 2>&1; done
"$TMUX_REAL" -L "$BX" source-file "$DIR/examples/tmux.conf" >/dev/null 2>&1
"$TMUX_REAL" -L "$BX" rename-window -t s:0 home >/dev/null 2>&1
"$TMUX_REAL" -L "$BX" new-window -d -t s: -n build 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$BX" new-window -d -t s: -n test  'exec -a claude sleep 300' >/dev/null 2>&1
BXH=$("$TMUX_REAL" -L "$BX" list-panes -t s:home  -F '#{pane_id}' | head -1)
BXB=$("$TMUX_REAL" -L "$BX" list-panes -t s:build -F '#{pane_id}' | head -1)
BXT=$("$TMUX_REAL" -L "$BX" list-panes -t s:test  -F '#{pane_id}' | head -1)
asBX() { TMUX="$SOCKDIR/$BX,0,0" "$DIR/scripts/agent-state" "$1" "$2" >/dev/null 2>&1; }
asBX running "$BXH"; asBX needs-input "$BXB"; asBX needs-input "$BXT"
is "the banner lists the other blocked windows, byte-for-byte" \
   "#[fg=#ff6b6b]#[bold]─ NEEDS YOU 1:build 2:test #[default]#[align=right]#[range=control|9][x]#[norange]" \
   "$("$TMUX_REAL" -L "$BX" display-message -p -t "$BXH" '#{E:pane-border-format}')"
# The window you are on is excluded even when it, too, holds a blocked pane: the
# split adds a second (unfocused) blocked pane to home, and the banner on the
# active pane still names only build and test (home's own pane spells it out).
"$TMUX_REAL" -L "$BX" split-window -d -t "$BXH" 'exec -a claude sleep 300' >/dev/null 2>&1
BXH2=$("$TMUX_REAL" -L "$BX" list-panes -t s:home -F '#{pane_id} #{pane_active}' | awk '$2==0{print $1}')
asBX needs-input "$BXH2"
is "the banner still excludes the window you are on, byte-for-byte" \
   "#[fg=#ff6b6b]#[bold]─ NEEDS YOU 1:build 2:test #[default]#[align=right]#[range=control|8][z]#[norange]#[range=control|9][x]#[norange]" \
   "$("$TMUX_REAL" -L "$BX" display-message -p -t "$BXH" '#{E:pane-border-format}')"
"$TMUX_REAL" -L "$BX" kill-server >/dev/null 2>&1
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
# source-file continues past the later catppuccin/tpm lines that need a real
# install; set-titles sits high in the file, before any of them.
"$TMUX_REAL" -L "$ST" source-file "$DIR/examples/full.tmux.conf" >/dev/null 2>&1
is "full.tmux.conf leaves the outer terminal title off" "off" \
   "$("$TMUX_REAL" -L "$ST" show-option -gv set-titles)"
"$TMUX_REAL" -L "$ST" kill-server >/dev/null 2>&1
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
is "the user's other settings are untouched" "opus" \
   "$(jq -r .model "$HK/.claude/settings.json")"
[ -f "$HK/.claude/settings.json.bak" ] && ok "a backup was written" \
    || bad "a backup was written" "settings.json.bak" "missing"
before=$(cat "$HK/.claude/settings.json")
HOME="$HK" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "a second run changes nothing (idempotent)" "$before" "$(cat "$HK/.claude/settings.json")"
# The same hook with and without an explicit matcher:"" is the SAME hook:
# a live Stop entry carrying the key while the shipped one lacked it made
# every --hooks run append one more duplicate, forever. Seed the matcher-less
# shape and assert the merge recognizes it rather than stacking a twin.
HKN=$(mktmpd); mkdir -p "$HKN/.claude"
jq '.hooks.Stop[0] |= del(.matcher)' "$DIR/examples/claude-hooks.json" \
    | jq '{hooks: {Stop: .hooks.Stop}}' > "$HKN/.claude/settings.json"
HOME="$HKN" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "a matcher-less twin does not duplicate" "1" \
   "$(jq '.hooks.Stop | length' "$HKN/.claude/settings.json")"
rm -rf "$HKN"
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
dupes=$(jq '[.hooks[] | length] | map(select(. > 1)) | length' "$HK7/.claude/settings.json")
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
echo "== prefix J jumps to the pane that needs you =="
if need_tmux "tmux-jump"; then :; else
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
# A shell pane holding stale (or hand-set) @panestate must never be treated as
# blocked -- the same glob guard that keeps it out of #{agent_fleet}. Pressing
# from the LAST agent pane in cycle order (JP3) proves it: if the stale pane
# were counted, cycling would land on IT (the highest pane id, sorted last);
# with the fix it wraps back to the first real agent pane instead.
JP4=$("$TMUX_REAL" -L "$JJ" new-window -t t: -P -F '#{pane_id}' 'sleep 300' 2>/dev/null)
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP4" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP3" "'$DIR/scripts/tmux-jump'" Enter; sleep 2
is "a shell holding stale state is never jumped to" "1" \
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

echo "== the fleet counter reads every session =="
if need_tmux "agent_fleet"; then :; else
FC="cockpit-fc-$$"; SOCKS="$SOCKS $FC"
"$TMUX_REAL" -L "$FC" -f /dev/null new-session -d -s a -x 80 -y 24 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" new-session -d -s b 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" new-window  -t b 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" set -g status-right '[#{agent_fleet}]' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
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
# Read at render time, so a theme sourced after the plugin still wins -- and a
# regression to a hard-coded colour shows up here rather than in someone's eyes.
"$TMUX_REAL" -L "$FC" set -g @thm_red '#ff6b6b' >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$FC" display-message -p -t "$FCP1" \
          "$("$TMUX_REAL" -L "$FC" show-option -gv status-right)")" in
    *'#[fg=#ff6b6b,bold]▲2'*) ok "the count wears the theme's red" ;;
    *) bad "the count wears the theme's red" "#[fg=#ff6b6b,bold]▲2" \
         "$("$TMUX_REAL" -L "$FC" display-message -p -t "$FCP1" \
              "$("$TMUX_REAL" -L "$FC" show-option -gv status-right)")" ;;
esac
asFC running "$FCP1"
asFC running "$FCP2"
asFC running "$FCP3"
is "a quiet fleet renders nothing at all" "[]" "$(fleet)"
# A pane whose agent has exited keeps its last @panestate forever. Unguarded,
# the fleet count would include it and never come back down. agent-state writes
# @panestate regardless, but the precompute counts through the same glob guard,
# so a shell pane never reaches the counter.
"$TMUX_REAL" -L "$FC" new-window -t b 'sleep 300' >/dev/null 2>&1
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
TMUX="$CXTMUX" TMUX_PANE="$CXP" "$DIR/scripts/agent-context" \
    < "$DIR/tests/fixtures/statusline-overshoot.json" >/dev/null
is "an overshooting fallback ratio clamps to 100" "100" \
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
# issue exactly one set-option, not two. A tmux shim ahead of the suite's own
# on PATH logs every set-option call it sees while still forwarding to the
# real binary, so the pane's state stays correct and the count is honest.
if need_tmux "agent-context write guard"; then :; else
CX2="cockpit-cx2-$$"; SOCKS="$SOCKS $CX2"
"$TMUX_REAL" -L "$CX2" -f /dev/null new-session -d -s t 'sleep 300' >/dev/null 2>&1
CXP2=$("$TMUX_REAL" -L "$CX2" list-panes -t t -F '#{pane_id}')
CXTMUX2="$SOCKDIR/$CX2,0,0"
CXS=$(mktmpd)
CXLOG="$CXS/setlog"; : > "$CXLOG"
cat > "$CXS/tmux" <<SHIMEOF
#!/bin/sh
[ "\$1" = "set-option" ] && printf '%s\n' "\$*" >> "$CXLOG"
exec "$TMUX_REAL" "\$@"
SHIMEOF
chmod +x "$CXS/tmux"
PATH="$CXS:$PATH" TMUX="$CXTMUX2" TMUX_PANE="$CXP2" "$DIR/scripts/agent-context" \
    < "$DIR/tests/fixtures/statusline.json" >/dev/null
is "the first call writes the new value" "1" "$(wc -l < "$CXLOG")"
PATH="$CXS:$PATH" TMUX="$CXTMUX2" TMUX_PANE="$CXP2" "$DIR/scripts/agent-context" \
    < "$DIR/tests/fixtures/statusline.json" >/dev/null
is "a second call with the same value writes nothing" "1" "$(wc -l < "$CXLOG")"
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
# hunt machine. timeout is the fail-first teeth -- on the old build this fires.
RVPERF=$(mktmpd)
(
  cd "$RVPERF" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  : > big.txt
  git add -A; git commit -q -m init
  seq 1 5000 > big.txt
) >/dev/null 2>&1
timeout 3 "$DIR/scripts/tmux-review" --lines "$RVPERF" >/dev/null; rvperfrc=$?
if [ "$rvperfrc" -eq 124 ]; then
    bad "a 5000-row under-cap diff parses in well under 3s" \
        "output within 3s" "timed out (the O(n^2) build)"
else
    ok "a 5000-row under-cap diff parses in well under 3s"
fi
rvperfrows=$("$DIR/scripts/tmux-review" --lines "$RVPERF" | grep -c .)
is "the 5000-row diff is fully parsed, not capped (5000 rows, no notice)" \
   "5000" "$rvperfrows"
rm -rf "$RVPERF"

echo "== tmux-review --rows: pinning a comment does not re-grep the store once per row =="
# rows_mode used to fire `grep -qxF <header>` over the whole store for EVERY
# diff row, and the picker reloads the whole marked list after each pinned
# comment. The headers are now read once into a set and each row does an O(1)
# lookup. Correctness first: the ● must land on exactly the commented row and
# be sign-specific -- a (-) row at the same line number must stay blank.
RVMARK=$(mktmpd)
(
  cd "$RVMARK" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\nc\n' > src.go
  git add -A; git commit -q -m init
  printf 'a\nZED\nc\nADDED\n' > src.go
) >/dev/null 2>&1
RVMSTORE=$(mktemp)
# Pin a comment on the +2 (ZED) row -- the diff also has a -2 (b) row at the
# same line number, so this proves the marker is keyed on path:line AND sign.
"$DIR/scripts/tmux-review" --attach "$RVMSTORE" "src.go" "2" "ZED" "fix this line"
rvmarkcol=$("$DIR/scripts/tmux-review" --rows "$RVMARK" "$RVMSTORE" | cut -f4)
rvmarkwant=$(printf '%s\n' ' src.go:2 (-)  b' '●src.go:2 (+)  ZED' ' src.go:4 (+)  ADDED')
is "the ● lands on exactly the commented row, sign-specific" "$rvmarkwant" "$rvmarkcol"
rm -f "$RVMSTORE"; rm -rf "$RVMARK"

# Perf: --rows on a 4000-row diff with a pinned comment. The old per-row grep
# made this ~10.7s here; it is ~0.3s now. timeout is the fail-first teeth.
RVMPERF=$(mktmpd)
(
  cd "$RVMPERF" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  : > big.txt
  git add -A; git commit -q -m init
  seq 1 4000 > big.txt
) >/dev/null 2>&1
RVMPSTORE=$(mktemp)
"$DIR/scripts/tmux-review" --attach "$RVMPSTORE" "big.txt" "100" "100" "a comment"
timeout 5 "$DIR/scripts/tmux-review" --rows "$RVMPERF" "$RVMPSTORE" >/dev/null; rvmprc=$?
if [ "$rvmprc" -eq 124 ]; then
    bad "--rows re-marks a 4000-row diff in well under 5s" \
        "output within 5s" "timed out (the per-row grep)"
else
    ok "--rows re-marks a 4000-row diff in well under 5s"
fi
rm -f "$RVMPSTORE"; rm -rf "$RVMPERF"

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

echo "== tmux-review --attach: no row under the cursor stores nothing =="
# {1} resolves to an empty string when the list has no row (or the match
# under the cursor vanished) -- without a guard this still writes
# "## : (+)" to the store with an empty fragment, and it delivers.
ATSTORE=$(mktemp)
"$DIR/scripts/tmux-review" --attach "$ATSTORE" "" "" "" "a stray comment"
if [ -s "$ATSTORE" ]; then
    bad "an empty path attaches nothing" "an empty store" "$(cat "$ATSTORE")"
else
    ok "an empty path attaches nothing"
fi
rm -f "$ATSTORE"

echo "== tmux-review --loop: alt-enter pins a comment, ctrl-s delivers, esc discards =="
if need_tmux "tmux-review --loop"; then :; else
RVS="cockpit-rv-$$"; SOCKS="$SOCKS $RVS"
RVDIR=$(mktmpd)
(
  cd "$RVDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go
  git add -A; git commit -q -m init
  # A pure addition -- exactly one hunk line -- so the row under the cursor is
  # never ambiguous: with a changed line the diff shows a -old/+new pair and
  # nothing here pins which one fzf highlights by default.
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
RVSTORE="$STATE/review-store-$$"

"$TMUX_REAL" -L "$RVS" -f /dev/null new-session -d -s t -x 100 -y 30 \
    'exec -a claude bash' >/dev/null 2>&1
RVP=$("$TMUX_REAL" -L "$RVS" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" \
    "'$DIR/scripts/tmux-review' --loop '$RVDIR' '$RVSTORE'" Enter

rv2up=0
for i in 1 2 3 4 5 6 7 8 9 10; do
    case "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" in
        *"review>"*) rv2up=1; break ;;
    esac
    sleep 0.5
done
if [ "$rv2up" != 1 ]; then
    bad "the picker comes up" "a visible 'review>' prompt" \
        "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")"
else
    ok "the picker comes up"
fi

# Type a comment and pin it to the highlighted row with alt-enter.
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" "fix the new line"
sleep 0.5
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" M-Enter
sleep 1

case "$(cat "$RVSTORE" 2>/dev/null)" in
    *"## src.go:3 (+)"*"> CHANGED"*"fix the new line"*)
        ok "alt-enter pins path:line, the quoted fragment and the comment" ;;
    *) bad "alt-enter pins path:line, the quoted fragment and the comment" \
         "## src.go:3 (+) / > CHANGED / fix the new line" "$(cat "$RVSTORE" 2>/dev/null)" ;;
esac

case "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" in
    *"●"*) ok "the reloaded list marks the commented row" ;;
    *) bad "the reloaded list marks the commented row" "a ● in the pane" \
         "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" ;;
esac

# The only enforcement of WHAT-NOT-TO-DO rule 1 ("no accept-all"): plain
# enter must neither leave the picker nor touch the store, on a row that
# already has an unrelated pending comment pinned to it.
storebefore=$(cat "$RVSTORE" 2>/dev/null)
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" Enter
sleep 1
case "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" in
    *"review>"*) ok "plain enter does nothing: still in the picker" ;;
    *) bad "plain enter does nothing: still in the picker" "a visible 'review>' prompt" \
         "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" ;;
esac
is "plain enter does nothing: the store is unchanged" \
   "$storebefore" "$(cat "$RVSTORE" 2>/dev/null)"

"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" Escape
sleep 1
if [ -e "$RVSTORE" ]; then
    bad "esc discards: the store is gone" "no such file" "$RVSTORE still exists"
else
    ok "esc discards: the store is gone"
fi

"$TMUX_REAL" -L "$RVS" kill-server >/dev/null 2>&1
rm -rf "$RVDIR"
fi

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
Code review of your uncommitted changes. Address each comment; a comment
containing ?? is a question — answer it in chat, do not edit for it.
Comments are anchored as path:line with the original line quoted.

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
Code review of your uncommitted changes. Address each comment; a comment
containing ?? is a question — answer it in chat, do not edit for it.
Comments are anchored as path:line with the original line quoted.
Paths are relative to /home/agent/project.

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

echo "== tmux-review --deliver: guards refuse before anything is pasted =="
if need_tmux "tmux-review --deliver"; then :; else
RDS="cockpit-rd-$$"; SOCKS="$SOCKS $RDS"
"$TMUX_REAL" -L "$RDS" -f /dev/null new-session -d -s t -x 100 -y 30 \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" split-window -t t 'exec bash' >/dev/null 2>&1
sleep 0.3
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
    "$TMUX_REAL" -L "$RDS" send-keys -t "$NP" \
        "$(shq_test "$DIR/scripts/tmux-review") --deliver $(shq_test "$1") $(shq_test "$2"); echo RC=\$?" Enter
    sleep 0.6
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
    *"Code review of your uncommitted changes"*) \
        bad "an empty store is refused, nothing to deliver" \
            "no composed header in the agent pane" "$(apcap)" ;;
    *) ok "an empty store is refused, nothing to deliver" ;;
esac
is "an empty store is refused (exit 1)" "RC=1" "$(lastrc)"

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

echo "== tmux-review --deliver: concurrent deliveries do not cross-wire panes =="
# A second agent pane and a second control pane -- driving both deliveries
# from their OWN panes, fired back-to-back with no sleep between, is the
# shape that raced under the old fixed `-b cockpit-review` buffer name (tmux
# buffers are server-global): B's load-buffer could overwrite A's payload
# before A's paste-buffer -d ran.
AP2=$("$TMUX_REAL" -L "$RDS" split-window -t t -P -F '#{pane_id}' 'exec -a claude bash')
NP2=$("$TMUX_REAL" -L "$RDS" split-window -t t -P -F '#{pane_id}' 'exec bash')
sleep 0.3

ASTORE="$STATE/deliver-race-a-$$"
BSTORE="$STATE/deliver-race-b-$$"
printf '## a.go:1 (+)\n> a\nALPHA-MARKER-CONTENT\n\n' > "$ASTORE"
printf '## b.go:2 (+)\n> b\nBETA-MARKER-CONTENT\n\n' > "$BSTORE"

"$TMUX_REAL" -L "$RDS" send-keys -t "$NP" \
    "$(shq_test "$DIR/scripts/tmux-review") --deliver $(shq_test "$AP") $(shq_test "$ASTORE")" Enter
"$TMUX_REAL" -L "$RDS" send-keys -t "$NP2" \
    "$(shq_test "$DIR/scripts/tmux-review") --deliver $(shq_test "$AP2") $(shq_test "$BSTORE")" Enter
sleep 0.8

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
    "export XDG_RUNTIME_DIR=$(shq_test "$RUNDIR")" Enter
sleep 0.2
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

"$TMUX_REAL" -L "$RDS" kill-server >/dev/null 2>&1
fi

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
sleep 0.3
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
sleep 0.3
case "$(gcap "$NPG")" in
    *HAZARD-MARKER*) ok "an unconditional paste into the shell DOES land (the hazard is real)" ;;
    *) bad "an unconditional paste into the shell DOES land (the hazard is real)" \
         "HAZARD-MARKER in the shell" "$(gcap "$NPG")" ;;
esac
# Clear the shell's readline of the pasted-but-unsent hazard line before the
# real assertion, so a later match cannot see it.
"$TMUX_REAL" -L "$RG" send-keys -t "$NPG" C-c >/dev/null 2>&1
sleep 0.2

# The fix, byte-for-byte as deliver() runs it: stage the buffer, then let tmux
# re-check #{pane_current_command} in the SAME command-queue item as the paste.
# The pane is a shell at paste time -> refused, and the staged buffer dropped.
printf 'RACE-MARKER-CONTENT\n' | "$TMUX_REAL" -L "$RG" load-buffer -b rg-gate -
"$TMUX_REAL" -L "$RG" if-shell -F -t "$NPG" \
    '#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}}' \
    "paste-buffer -p -r -d -b rg-gate -t $NPG ; display-message pasted" \
    "delete-buffer -b rg-gate ; display-message 'not delivered'"
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
    '#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}}' \
    "paste-buffer -p -r -d -b rg-ok -t $APG ; display-message pasted" \
    "delete-buffer -b rg-ok ; display-message 'not delivered'"
sleep 0.3
case "$(gcap "$APG")" in
    *DELIVERED-MARKER*) ok "the gate still delivers into a pane that is a live agent" ;;
    *) bad "the gate still delivers into a pane that is a live agent" \
         "DELIVERED-MARKER in the agent pane" "$(gcap "$APG")" ;;
esac

"$TMUX_REAL" -L "$RG" kill-server >/dev/null 2>&1
fi

echo "== tmux-review: a delivered review's comment store does not outlive the popup that made it =="
# review_pane()'s own mktemp store -- unlike --deliver's spillover file, this
# one is never named to us. Redirect TMPDIR to a private, otherwise-empty
# directory for the pane driving this, so whatever mktemp creates there is
# unambiguously it.
if need_tmux "tmux-review fleet-to-deliver store cleanup"; then :; else
FD="cockpit-fd-$$"; SOCKS="$SOCKS $FD"
FDTMPDIR="$STATE/fd-tmp-$$"
mkdir -p "$FDTMPDIR"
"$TMUX_REAL" -L "$FD" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$FD" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
CTL2=$("$TMUX_REAL" -L "$FD" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" \
    "export TMPDIR=$(shq_test "$FDTMPDIR")" Enter
sleep 0.2

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
sleep 0.3

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
        *"review>"*) rvup=1; break ;;
    esac
    sleep 0.5
done
if [ "$rvup" != 1 ]; then
    bad "selecting the one row opens its review loop" "a visible 'review>' prompt" \
        "$("$TMUX_REAL" -L "$FD" capture-pane -p -t "$CTL2")"
else
    ok "selecting the one row opens its review loop"
fi

storefile=""
sf=$(ls "$FDTMPDIR" 2>/dev/null | head -1)
[ -n "$sf" ] && storefile="$FDTMPDIR/$sf"
if [ -n "$storefile" ] && [ -e "$storefile" ]; then
    ok "the review's comment store exists while the popup is open"
else
    bad "the review's comment store exists while the popup is open" \
        "one file under $FDTMPDIR" "none"
fi

"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" "fix this"
sleep 0.3
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" M-Enter
sleep 0.5
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" C-s
sleep 1

case "$("$TMUX_REAL" -L "$FD" capture-pane -p -J -t "$FD_AGENT")" in
    *"fix this"*) ok "the review still gets delivered through the fleet path" ;;
    *) bad "the review still gets delivered through the fleet path" \
         "the comment in the agent pane" \
         "$("$TMUX_REAL" -L "$FD" capture-pane -p -J -t "$FD_AGENT")" ;;
esac
if [ -n "$storefile" ] && [ -e "$storefile" ]; then
    bad "the comment store does not outlive the delivered review" \
        "$storefile gone" "$storefile still exists"
else
    ok "the comment store does not outlive the delivered review"
fi

"$TMUX_REAL" -L "$FD" kill-server >/dev/null 2>&1
rm -rf "$FD_DIRTY" "$FDTMPDIR"
fi

echo "== tmux-review: a clean-tree direct pane says so instead of opening an empty picker =="
if need_tmux "tmux-review clean-tree direct pane"; then :; else
MT="cockpit-mt-$$"; SOCKS="$SOCKS $MT"
MT_DIR=$(mktmpd)
(
  cd "$MT_DIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\n' > f
  git add -A; git commit -q -m init
) >/dev/null 2>&1
"$TMUX_REAL" -L "$MT" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$MT_DIR" \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$MT" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
MTP=$("$TMUX_REAL" -L "$MT" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$MT" send-keys -t "$MTP" \
    "'$DIR/scripts/tmux-review'; echo MTDONE=\$?" Enter
sleep 1
mtout=$("$TMUX_REAL" -L "$MT" capture-pane -p -t "$MTP")
case "$mtout" in
    *"review>"*) bad "a clean-tree direct pane never opens an empty picker" \
         "no 'review>' prompt" "$mtout" ;;
    *) ok "a clean-tree direct pane never opens an empty picker" ;;
esac
case "$mtout" in
    *"MTDONE=0"*) ok "and exits 0" ;;
    *) bad "and exits 0" "MTDONE=0" "$mtout" ;;
esac
"$TMUX_REAL" -L "$MT" kill-server >/dev/null 2>&1
rm -rf "$MT_DIR"
fi

echo "== tmux-review --fleet: from a non-agent pane, the queue lists exactly the dirty agent panes =="
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
FQ_CLEAN=$(mktmpd);    mk_git_dir "$FQ_CLEAN" 0      # agent, clean -- excluded
FQ_NONAGENT=$(mktmpd); mk_git_dir "$FQ_NONAGENT" 1   # dirty, but not an agent

"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_DIRTY" 'exec -a claude bash' >/dev/null 2>&1
DIRTY_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_CLEAN" 'exec -a claude bash' >/dev/null 2>&1
CLEAN_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:2 -F '#{pane_id}')
"$TMUX_REAL" -L "$FQ" new-window -t t: -c "$FQ_NONAGENT" 'exec bash' >/dev/null 2>&1
NONAGENT_PANE=$("$TMUX_REAL" -L "$FQ" list-panes -t t:3 -F '#{pane_id}')
sleep 0.3

"$TMUX_REAL" -L "$FQ" send-keys -t "$CTL" "'$DIR/scripts/tmux-review' --fleet" Enter
sleep 0.6
fqout=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")

case "$fqout" in
    *"$DIRTY_PANE"*) ok "the dirty agent pane is in the queue" ;;
    *) bad "the dirty agent pane is in the queue" "$DIRTY_PANE in the rows" "$fqout" ;;
esac
case "$fqout" in
    *"$CLEAN_PANE"*) bad "a clean agent pane is not in the queue" \
         "no $CLEAN_PANE in the rows" "$fqout" ;;
    *) ok "a clean agent pane is not in the queue" ;;
esac
case "$fqout" in
    *"$NONAGENT_PANE"*) bad "a dirty non-agent pane is not in the queue" \
         "no $NONAGENT_PANE in the rows" "$fqout" ;;
    *) ok "a dirty non-agent pane is not in the queue" ;;
esac
rowcount=$(printf '%s\n' "$fqout" | grep -c '^%' || true)
is "the queue lists exactly one row" "1" "$rowcount"

"$TMUX_REAL" -L "$FQ" kill-server >/dev/null 2>&1
rm -rf "$FQ_DIRTY" "$FQ_CLEAN" "$FQ_NONAGENT"
fi

echo "== prefix r reviews, prefix R reloads; off unbinds each =="
if need_tmux "cockpit_review_key"; then :; else
RB="cockpit-rb-$$"; SOCKS="$SOCKS $RB"
"$TMUX_REAL" -L "$RB" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$RB" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; sleep 0.3
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
"$TMUX_REAL" -L "$RB2" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; sleep 0.3
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
fi

printf '  %d passed, %d failed, %d skipped\n' "$pass" "$fail" "$skipped"
[ "$fail" -eq 0 ] || exit 1
