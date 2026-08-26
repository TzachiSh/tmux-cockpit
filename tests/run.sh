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
# Polled, not slept for. A fixed sleep guesses how long a keystroke takes to
# land; these wait for the thing the next assertion reads and give up after
# 4 s. PATTERN is a case-pattern with NO embedded quotes -- a quote in an
# expanded pattern is a literal character: until_cap "$S" "$P" '*review>*'
until_cap() {                 # <socket> <pane> <pattern>
    local i
    for i in $(seq 1 40); do
        # shellcheck disable=SC2254  # $3 is meant as a glob, not a literal
        case "$("$TMUX_REAL" -L "$1" capture-pane -p -J -t "$2" 2>/dev/null)" in $3) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
review_enter() {              # <socket> <ctl-pane>  the queue opens preselected on the pressed pane; ↵ reviews it
    until_cap "$1" "$2" '*fleet>*' || return 1
    "$TMUX_REAL" -L "$1" send-keys -t "$2" Enter
    until_cap "$1" "$2" '*review>*'
}
until_file() {                # <file> <pattern>
    local i
    for i in $(seq 1 40); do
        # shellcheck disable=SC2254  # $2 is meant as a glob, not a literal
        case "$(cat -- "$1" 2>/dev/null)" in $2) return 0 ;; esac
        sleep 0.1
    done
    return 1
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

# tests: examples/tmux.conf cockpit.tmux
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
# tests: cockpit.tmux
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

echo "== agent-state: a hook transition costs at most four tmux round-trips =="
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
if [ "${rtrun:-99}" -le 4 ]; then ok "running costs at most 4 tmux invocations"
else bad "running costs at most 4 tmux invocations" "<=4" "$rtrun"; fi
if [ "${rtdone:-99}" -le 4 ]; then ok "done costs at most 4 tmux invocations"
else bad "done costs at most 4 tmux invocations" "<=4" "$rtdone"; fi
# Batching must not buy the count with correctness: every option the unbatched
# version wrote is still written, with the same value.
is "the pane state still lands" "done" \
   "$("$TMUX_REAL" -L "$RT" show-option -pqv -t "$RTP" @panestate)"
is "the window dot still ranks it" "done" \
   "$("$TMUX_REAL" -L "$RT" show-option -wqv -t "$RTP" @cockpit_win_state)"
rtneeds=$(rtcall needs-input)
if [ "${rtneeds:-99}" -le 4 ]; then ok "needs-input costs at most 4 tmux invocations"
else bad "needs-input costs at most 4 tmux invocations" "<=4" "$rtneeds"; fi
is "the banner boolean still flips" "1" \
   "$("$TMUX_REAL" -L "$RT" show-option -wqv -t "$RTP" @cockpit_win_needs)"
is "the fleet counter still counts" '#[fg=#{@thm_red}#,bold]▲1#[default] ' \
   "$("$TMUX_REAL" -L "$RT" show-option -gqv @cockpit_fleet)"
# An unchanged state still costs exactly one round-trip: the read that decides
# nothing changed, and no writes at all.
rtsame=$(rtcall needs-input)
is "an unchanged state costs one read and no writes" "1" "$rtsame"
# The fleet string is now read back through the same batched format rather than
# through show-option, and it carries #{...} colour tokens. If the format
# re-expanded them the compare-then-set guard would never match and every
# transition on the server would rewrite the global -- a server-wide redraw
# apiece. A second agent pane whose transition leaves the count at 1 must write
# nothing to @cockpit_fleet.
"$TMUX_REAL" -L "$RT" new-window -t t: 'exec -a claude bash' >/dev/null 2>&1
RTP2=$("$TMUX_REAL" -L "$RT" list-panes -t t:1 -F '#{pane_id}')
: > "$RTLOG"
PATH="$RTS:$PATH" TMUX="$SOCKDIR/$RT,0,0" "$DIR/scripts/agent-state" \
    running "$RTP2" >/dev/null 2>&1
is "an unchanged fleet count is never rewritten" "0" \
   "$(grep -c '@cockpit_fleet ' "$RTLOG")"
is "and the stored count still carries its colour tokens" \
   '#[fg=#{@thm_red}#,bold]▲1#[default] ' \
   "$("$TMUX_REAL" -L "$RT" show-option -gqv @cockpit_fleet)"
"$TMUX_REAL" -L "$RT" kill-server >/dev/null 2>&1
fi

echo "== agent-state recount: ◆N counts panes with a pending review, off clears them =="
if need_tmux "agent-state recount"; then :; else
tmux set-option -p -t "$PANE" @cockpit_review pending >/dev/null 2>&1
st recount "$PANE"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'◆1'*) ok "recount adds ◆1 to the fleet string" ;;
    *) bad "recount adds ◆1 to the fleet string" "…◆1…" "$(tmux show-option -gqv @cockpit_fleet)" ;;
esac
tmux set-option -p -t "$PANE" @cockpit_review sent >/dev/null 2>&1
st recount "$PANE"
case "$(tmux show-option -gqv @cockpit_fleet)" in
    *'◆'*) bad "a sent review is not a pending one" "no ◆" "$(tmux show-option -gqv @cockpit_fleet)" ;;
    *) ok "a sent review is not a pending one" ;;
esac
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
rg_n() { "$TMUX_REAL" -L "$RG" capture-pane -p -J -t "$RGCTL" | grep -c 'GRC=[0-9]'; }
rg_wait() {                   # <count-before>  until one more finished GRC= marker is on screen (rundeliver's shape)
    local i; for i in $(seq 1 60); do [ "$(rg_n)" -gt "$1" ] && return 0; sleep 0.1; done; return 1
}
rg_run() {                    # <stdin-file>  runs the hook inside the session as the pane's own hook would, RC on the control pane
    local n0; n0=$(rg_n)
    "$TMUX_REAL" -L "$RG" send-keys -t "$RGCTL" \
        "TMUX_PANE=$RGAP XDG_STATE_HOME=$(printf '%q' "$RGSTATE") '$DIR/scripts/review-gate' < $(printf '%q' "$1") > $(printf '%q' "$RGSTATE/out") 2> $(printf '%q' "$RGSTATE/err"); echo GRC=\$?" Enter
    rg_wait "$n0"
}
rg_rc() { "$TMUX_REAL" -L "$RG" capture-pane -p -J -t "$RGCTL" | grep -o 'GRC=[0-9]*' | tail -1; }
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
is "and stderr stays empty on a handover" "" "$(cat "$RGSTATE/err")"
is "the spool option is cleared before speaking" "" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @cockpit_review_spool)"
is "the spool file is consumed" "absent" "$([ -e "$RGSTATE/spool.md" ] && echo present || echo absent)"
is "the pane reads running again (the agent is about to work)" "running" "$("$TMUX_REAL" -L "$RG" show-option -pqv -t "$RGAP" @panestate)"
rg_run "$RGPAY"; is "the same Stop fired again delivers nothing (no rewake loop)" "GRC=0" "$(rg_rc)"
is "and stdout is empty the second time" "" "$(cat "$RGSTATE/out")"
is "and stderr is empty the second time" "" "$(cat "$RGSTATE/err")"
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
unset -f rg_n rg_wait rg_run rg_rc
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

echo "== tmux-open-path: a pathologically long clicked line is bounded, not just slow =="
# The per-token loop is linear (0.31ms/token measured), so a normal clicked
# line -- a handful of tokens -- costs nothing. A 112KB line of 4000
# path-shaped tokens (real output: a wrapped stack trace, an agent dumping a
# wordlist) measured 1.59s in this loop alone, all before the picker even
# opens. Capped at 500 tokens, same idea as the preview's render-window guard.
BIGLINE=$(awk 'BEGIN{s="";for(i=1;i<=4000;i++) s = s "nope" i "xxxxxxxxxxxxxxxx ";print s}')
timeout 0.6 "$DIR/scripts/tmux-open-path" --dry-run "$DIR" "$BIGLINE" >/dev/null 2>&1
biglinerc=$?
if [ "$biglinerc" -eq 124 ]; then
    bad "a 4000-token clicked line is bounded, not just slow" \
        "output within 0.6s" "timed out (the uncapped per-token loop)"
else
    ok "a 4000-token clicked line is bounded, not just slow"
fi

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

echo "== tmux-branch: builds each row without a fork per branch =="
# The disps loop ran `$(printf ...)` inside a command substitution -- a fork
# per branch. 5000 branches measured 16.9s (for-each-ref itself is 0.12s; 99%
# was the loop). printf -v does the identical formatting inside the running
# shell instead. timeout is the fail-first teeth: it fires on the fork build.
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
timeout 2 env -u TMUX PATH="$FZFSTUB" "$DIR/scripts/tmux-branch" "$BP" </dev/null >/dev/null 2>&1
brc=$?
if [ "$brc" -eq 124 ]; then
    bad "5000 branches build their rows well under 2s" \
        "exit within 2s" "timed out (the fork-per-branch build)"
else
    ok "5000 branches build their rows well under 2s"
fi
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
tmux -L "$OUTERA" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $INNER attach -t cliA" >/dev/null 2>&1
sleep 1
tmux -L "$OUTERB" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $INNER attach -t cliB" >/dev/null 2>&1
sleep 1
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
# HOME="$IB" on every call below, and a ~/.claude to find: install.sh also links
# the cockpit-review skill into ~/.claude/skills wherever Claude Code lives, and
# the suite must never write into the real one.
mkdir -p "$IB/.claude"
HOME="$IB" XDG_BIN_HOME="$IBIN" "$IL/install.sh" >/dev/null 2>&1
is "installs through a symlinked plugin dir" "3" "$(ls "$IBIN" | wc -l)"
is "the skill is linked into ~/.claude/skills" "$DIR/skills/cockpit-review" "$(readlink "$IB/.claude/skills/cockpit-review")"
# DIR came from a LOGICAL pwd while the guard compared readlink -f of the link,
# so reaching the plugin through a symlink -- a dotfiles checkout linked into
# ~/.tmux/plugins is the usual way -- made --uninstall remove nothing, silently.
HOME="$IB" XDG_BIN_HOME="$IBIN" "$IL/install.sh" --uninstall >/dev/null 2>&1
is "and uninstalls through one too" "0" "$(ls "$IBIN" | wc -l)"
is "and unlinked on --uninstall" "absent" "$([ -e "$IB/.claude/skills/cockpit-review" ] && echo present || echo absent)"
# A same-named symlink owned by another tool must be left alone: "tm" is a name
# plenty of things want, and --uninstall would later have deleted it as well.
HOME="$IB" XDG_BIN_HOME="$IBIN" "$DIR/install.sh" >/dev/null 2>&1
rm -f "$IBIN/tm"; ln -s /bin/echo "$IBIN/tm"
HOME="$IB" XDG_BIN_HOME="$IBIN" "$DIR/install.sh" >/dev/null 2>&1
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

echo "== branch popup passes the source pane id =="
# Inside a popup $TMUX_PANE is the popup's own pseudo-pane, so tmux-branch
# must be told which pane the key/click actually fired on rather than
# resolving it itself. Both the prefix+b bind and the click gesture pass
# -p '#{pane_id}', expanded by tmux at bind time on the originating pane.
if need_tmux "branch popup pane id"; then :; else
BPI="cockpit-bpi-$$"; SOCKS="$SOCKS $BPI"
"$TMUX_REAL" -L "$BPI" -f /dev/null new-session -d -s t >/dev/null 2>&1
"$TMUX_REAL" -L "$BPI" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; sleep 0.3
case "$("$TMUX_REAL" -L "$BPI" list-keys -T prefix 2>/dev/null | awk '$4=="b"')" in
    *"-p '#{pane_id}'"*) ok "prefix b passes the source pane id" ;;
    *) bad "prefix b passes the source pane id" "-p '#{pane_id}' in the bind" \
         "$("$TMUX_REAL" -L "$BPI" list-keys -T prefix | awk '$4=="b"')" ;;
esac
case "$("$TMUX_REAL" -L "$BPI" list-keys -T root | grep MouseDown1Control1)" in
    *"-p '#{pane_id}'"*) ok "the branch click passes the source pane id" ;;
    *) bad "the branch click passes the source pane id" "-p '#{pane_id}' in the bind" \
         "$("$TMUX_REAL" -L "$BPI" list-keys -T root | grep MouseDown1Control1)" ;;
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

echo "== the window-tab dot precomputes the worst state, byte-for-byte per state =="
# A6 moved full.tmux.conf's per-window agent marker (▲ blocked, ✕ error,
# ● running, ✓ done, ○ idle) out of the catppuccin window-status-format, where a
# redraw re-ran its #{P:} pane walk for every window, into one keyword
# @cockpit_win_state that scripts/agent-state writes on each transition -- the
# WORST state among the window's panes, ranked needs-input > error > running >
# done > idle. This drives every state through agent-state (the production path)
# and pins BOTH the keyword recorded and the marker full.tmux.conf renders from
# it. The golden renders were captured from the pre-A6 live walk, so a match is
# byte-identical before/after; and pre-A6 the keyword is never written, so the
# keyword rows fail-first if the precompute is reverted.
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
wdren() { "$TMUX_REAL" -L "$WD" display-message -p -t "s:$1" '#{E:@catppuccin_window_text}'; }
wdst()  { "$TMUX_REAL" -L "$WD" show-option -wqv -t "s:$1" @cockpit_win_state; }
# The keyword agent-state records -- never written by the pre-A6 code, so these
# are the fail-first anchor.
is "the dot keyword ranks needs-input over running" needs-input "$(wdst blkwin)"
is "the dot keyword ranks error over done"          error       "$(wdst errwin)"
is "the dot keyword records running"                running     "$(wdst runwin)"
is "the dot keyword records done"                   done        "$(wdst finwin)"
is "a bare agent's dot keyword is idle"             idle        "$(wdst idlewin)"
is "a non-agent window records no dot keyword"      ""          "$(wdst plainwin)"
# The marker full.tmux.conf renders from the keyword, byte-for-byte against the
# pre-A6 live walk (goldens captured from it: same glyph, same colour, same text).
is "the blocked window renders a red ▲, byte-for-byte"     " blkwin #[fg=#ff6b6b]▲"   "$(wdren blkwin)"
is "the errored window renders a peach ✕, byte-for-byte"   " errwin #[fg=#ff9838]✕"   "$(wdren errwin)"
is "the running window renders a yellow ●, byte-for-byte"  " runwin #[fg=#ffcc33]●"   "$(wdren runwin)"
is "the done window renders a green ✓, byte-for-byte"      " finwin #[fg=#3dea7d]✓"   "$(wdren finwin)"
is "the idle agent window renders a grey ○, byte-for-byte" " idlewin #[fg=#5b6272]○"  "$(wdren idlewin)"
is "the non-agent window renders no marker, byte-for-byte" " plainwin "               "$(wdren plainwin)"
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
# source-file continues past the later catppuccin/tpm lines that need a real
# install; set-titles sits high in the file, before any of them.
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
cat > "$JQSHIM/jq" <<EOF
#!/bin/sh
for a in "\$@"; do
    [ "\$a" = "-s" ] && sleep 1
done
exec "$REALJQ" "\$@"
EOF
chmod +x "$JQSHIM/jq"

HKR=$(mktmpd); mkdir -p "$HKR/.claude"
printf '{"model":"opus"}' > "$HKR/.claude/settings.json"
(PATH="$JQSHIM:$PATH" HOME="$HKR" "$DIR/install.sh" --hooks >/dev/null 2>&1) &
instpid=$!
sleep 0.3
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
# Nothing blocked, but one agent holds a review the gate flagged: that is
# still somewhere you have to go, so prefix J walks to it -- after the blocked
# panes and after the ones holding a delivered review, never before them.
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP3" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @cockpit_review pending >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" select-window -t t:0 >/dev/null 2>&1
# The marker poll, not a fixed sleep: the typed line's own "JPR=$?" has no
# digit after the =, so only the finished command's output matches.
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'; echo JPR=\$?" Enter
until_cap "$JJ" "$JP1" '*JPR=[0-9]*'
is "prefix J walks to a pane with a pending review when nothing is blocked" "1" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t: '#{window_index}')"
"$TMUX_REAL" -L "$JJ" set-option -pu -t "$JP2" @cockpit_review >/dev/null 2>&1
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

echo "== tmux-review --lines: an untracked tree is walked once, not four forks per file =="
# The untracked synthesis walked every file FOUR times over -- [ -f ],
# grep -Iq, wc -l and awk -- and did it twice, once in the numstat loop that
# feeds the cap and once in the row loop. rows_mode calls lines_mode, so the
# review popup re-paid the whole cost after every pinned comment. Measured with
# 800 one-line untracked files: the whole `--lines` run took 6.885s before and
# takes 0.134s now (the walk itself 7.9s -> 0.008s in the review's own
# measurement; master, which ignored untracked files entirely, was 0.018s), off
# the single batched `ls-files -z | xargs grep -Il .` list both loops now share.
# timeout is the fail-first teeth -- on the per-file walk this fires.
RVMANY=$(mktmpd)
(
  cd "$RVMANY" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > seed.txt; git add -A; git commit -q -m init
  mkdir -p vendor
  i=1; while [ "$i" -le 800 ]; do printf 'line\n' > "vendor/f$i.txt"; i=$((i+1)); done
) >/dev/null 2>&1
timeout 1 "$DIR/scripts/tmux-review" --lines "$RVMANY" >/dev/null; rvmanyrc=$?
if [ "$rvmanyrc" -eq 124 ]; then
    bad "800 untracked files list in well under 1s" \
        "output within 1s" "timed out (the per-file fork walk)"
else
    ok "800 untracked files list in well under 1s"
fi
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

echo "== tmux-open-path --preview-diff: old and new line numbers on every hunk line, one prefix per line, ▶ on the asked line =="
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
is "a context line carries both numbers"   "    1     1 │  1"      "$(printf '%s\n' "$rpg" | grep '│  1$')"
is "a removed line carries the old number" "    3       │ -3"      "$(printf '%s\n' "$rpg" | grep '│ -3$')"
is "an added line carries the new number"  "          3 │ +THREE" "$(printf '%s\n' "$rpg" | grep '│ +THREE$')"
is "a deletion keeps the old side counting" "   10       │ -10"    "$(printf '%s\n' "$rpg" | grep '│ -10$')"
is "and the new side skips it"              "   11    10 │  11"    "$(printf '%s\n' "$rpg" | grep '│  11$')"
rpgm=$( (cd "$RPG" && "$DIR/scripts/tmux-open-path" --preview-diff f.txt 5) | sed 's/\x1b\[[0-9;]*m//g' | sed -n 5p)
case "$rpgm" in "▶"*) ok "the asked line is marked ▶" ;; *) bad "the asked line is marked ▶" "▶..." "$rpgm" ;; esac
rpgc=$( (cd "$RPG" && "$DIR/scripts/tmux-open-path" --preview-diff f.txt) | sed 's/\x1b\[[0-9;]*m//g' | sed -n 5p)
case "$rpgc" in "▶"*) bad "the mark is not cached" "an unmarked line" "$rpgc" ;; *) ok "the mark is not cached" ;; esac
# Every check above strips ANSI first, so a gutter that printed the stripped
# copy instead of the raw line would pass them all with the colour gone. Peel
# the 11-column prefix back off and the diff's own bytes must be left.
rpgrawc=$(git -C "$RPG" diff --color=always HEAD -- f.txt | awk '/^\033\[[0-9;]*m@@|^@@/{h=1} h')
rpgcol=$( (cd "$RPG" && "$DIR/scripts/tmux-open-path" --preview-diff f.txt) | tail -n +2 | sed 's/^.\{11\} │ //')
is "the gutter is a prefix and nothing else, colour included" "$rpgrawc" "$rpgcol"
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
# mark_rows rebuilds every display field out of path/line/fragment. The cap
# row is a notice, not an anchor -- rebuilt it reads "(diff too large):0  + ..."
# with a green sign, a code row the user cannot pin and that names no file.
is "the cap row is shown as the plain notice, not a rebuilt path:0 row" \
   "$(printf '%s' "$rvcapout" | cut -f4)" \
   "$("$DIR/scripts/tmux-review" --rows "$RVCAP" /dev/null | cut -f4)"
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
# FAIL-first: before the reorder this returns in 3.25s and timeout 1.5 kills it.
RVLOCK=$(mktmpd)
(
  cd "$RVLOCK" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  for i in $(seq 1 30); do printf 'seed\n' > "lock$i.json"; done
  git add -A; git commit -q -m init
  for i in $(seq 1 30); do seq 1 200000 > "lock$i.json"; done
) >/dev/null 2>&1
if timeout 1.5 "$DIR/scripts/tmux-review" --lines "$RVLOCK" > "$STATE/rvlock.out" 2>/dev/null; then
    ok "a 6,000,030-row tree answers in under 1.5s (3.25s before the cap moved ahead of the capture)"
else
    bad "a 6,000,030-row tree answers in under 1.5s" \
        "the notice within 1.5s" "timed out -- the diff is still captured before the cap"
fi
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
rvbumpout=$(timeout 1.5 "$DIR/scripts/tmux-review" --lines "$RVBUMP")
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
rvskewout=$(timeout 1.5 "$DIR/scripts/tmux-review" --lines "$RVSKEW" 2>/dev/null)
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
rvskewshort=$(timeout 1.5 "$DIR/scripts/tmux-review" --lines "$RVSKEW" 2>/dev/null)
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
rvcrossout=$(timeout 1.5 "$DIR/scripts/tmux-review" --lines "$RVCROSS" 2>/dev/null)
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
rvubigout=$(timeout 1.5 "$DIR/scripts/tmux-review" --lines "$RVUBIG" 2>/dev/null)
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

# Perf: the rows themselves come out of awk, not a bash `read` loop. The row
# build was four bash passes over the same data (a churn map, the row build,
# the test partition, two emit loops) and bash pays an interpreter step per
# row for every one of them, so the whole function cost 4x its own diff
# parse. Measured here on this 4900-row single-file diff, ten runs each:
# 195-227ms with the bash build, 59-74ms with the awk one -- of which the
# git diff and the parsing awk, both of which were always there, are 44ms.
# So the row build fell from ~165ms to ~19ms.
#
# The tooth is 0.2 -- the rig's convention, ~3x the expected time -- not the
# 0.4 the plan wrote: 0.4 is where the bash build sat on the plan's machine
# (427ms) and this box runs it in ~210ms, so 0.4 passed the very code this
# test exists to reject. Know what 0.2 does and does not catch: probed
# against the bash build it failed 2 runs in 3 and passed the third, so it
# rejects that build most of the time but is not a proof. 0.15 was probed
# too (0/5 on bash, 5/5 on awk) and rejected as the tooth: it bites harder,
# but the awk build needs only a 2.2x slowdown to cross it, and a false
# failure on a loaded gate box costs more than the third bash run it would
# have caught.
RV49=$(mktmpd)
(
  cd "$RV49" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  : > f
  git add -A; git commit -q -m init
  seq 1 4900 > f
) >/dev/null 2>&1
if timeout 0.2 "$DIR/scripts/tmux-review" --lines "$RV49" >/dev/null 2>&1; then
    ok "4900 rows build in under 0.2s (measured 210ms bash, 65ms awk)"
else
    bad "4900 rows build in under 0.2s (measured 210ms bash, 65ms awk)" \
        "rows within 0.2s" "timed out (the bash row build)"
fi
is "and all 4900 rows are there" "4900" \
   "$("$DIR/scripts/tmux-review" --lines "$RV49" | grep -c .)"
rm -rf "$RV49"

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
rvmarkwant=$(printf '%s\n' '  src.go:2  '$'\e[31m''-'$'\e[0m'' b' '● src.go:2  '$'\e[32m''+'$'\e[0m'' ZED' '  src.go:4  '$'\e[32m''+'$'\e[0m'' ADDED')
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

# group: serial
echo "== tmux-review --rows: re-marking after a comment reads the cache, not git =="
# The picker reloads the WHOLE marked list after every pinned comment, and
# rows_mode re-derived that list from `git diff` + the full row build every
# single time. The rows themselves cannot change while the popup is open, so
# run_loop snapshots --lines into "$store.rows" once at open and each reload
# only re-marks that snapshot. Measured on a 4900-row diff with 500 comments
# pinned: 306ms per reload before, 15ms after. The snapshot must also be
# INVISIBLE -- same bytes as re-deriving the rows from the tree.
RV49=$(mktmpd); ( cd "$RV49" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'seed\n' > f; git add -A; git commit -q -m init; seq 1 4900 > f ) >/dev/null 2>&1
RV49S=$(mktmpf); "$DIR/scripts/tmux-review" --lines "$RV49" > "$RV49S.rows"
for i in $(seq 1 500); do printf '## f:%d (+)\n> %d\nc\n\n' "$i" "$i"; done > "$RV49S"
if timeout 0.15 "$DIR/scripts/tmux-review" --rows "$RV49" "$RV49S" > "$STATE/rows.cached" 2>/dev/null; then
    ok "a 500-comment re-mark over 4900 rows takes under 0.15s (measured 306ms before, 15ms after)"
else bad "a 500-comment re-mark over 4900 rows takes under 0.15s" "exit within 0.15s" "timeout"; fi
# The timing tooth alone does not prove the snapshot is READ. A version that
# writes it and still re-derives every row passes it, now that the uncached
# path is 68ms rather than the 306ms it was -- 0.15s no longer separates
# cached from uncached, only awk marking from bash marking. A row that exists
# ONLY in the snapshot does separate them: there is no way for it to reach the
# output except by being read back out of the file.
printf 'SENTINEL\t1\tx\tSENTINEL:1 (+)  x\t1\n' >> "$RV49S.rows"
if "$DIR/scripts/tmux-review" --rows "$RV49" "$RV49S" | grep -q '^SENTINEL'; then
    ok "a row that exists only in the snapshot is served from it"
else
    bad "a row that exists only in the snapshot is served from it" \
        "a row starting SENTINEL" "no SENTINEL row -- the rows were re-derived"
fi
rm -f "$RV49S.rows"; "$DIR/scripts/tmux-review" --rows "$RV49" "$RV49S" > "$STATE/rows.fresh"
is "the cached and fresh rows are identical" "" "$(cmp "$STATE/rows.cached" "$STATE/rows.fresh" 2>&1)"
is "500 rows are marked" "500" "$(grep -c '●' "$STATE/rows.cached")"
rm -rf "$RV49" "$RV49S"

echo "== tmux-review --loop: the row snapshot is born 0600 and never follows a planted symlink =="
# "<store>.rows" holds the WHOLE uncommitted diff and its name is DERIVABLE --
# one fixed suffix on a store path anyone watching a shared /tmp can see. A
# plain `>` was wrong twice: it created the file at the caller's umask (0664
# here, the diff world-readable while the 0600 store sits right beside it),
# and it FOLLOWED a symlink already planted at that name, writing the diff
# into someone else's file and truncating what was there. The write is
# `umask 077` + `set -C` now: O_CREAT|O_EXCL refuses any existing path, a
# symlink included, and what it does create is born 0600.
#
# No tmux and no real fzf here: run_loop only needs SOMETHING named fzf on
# PATH, and the snapshot is deleted on the way out -- so the stand-in picker
# is what looks at it, from inside the "popup", and then escs.
# Two directories on purpose: the stand-in picker and its logs must NOT live
# inside the tree being reviewed, or they show up as untracked rows and the
# row counts below start measuring the fixture instead of the diff.
RVSN=$(mktmpd); RVSNW=$(mktmpd); mkdir -p "$RVSNW/bin"
(
  cd "$RVSN" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\nc\n' > f
) >/dev/null 2>&1
cat > "$RVSNW/bin/fzf" <<'RVSNFZF'
#!/bin/sh
{ stat -c '%a' "$RVSNSTORE.rows" 2>/dev/null ||
  stat -f '%Lp' "$RVSNSTORE.rows" 2>/dev/null || echo none; } > "$RVSNLOG"
wc -l | tr -d ' ' > "$RVSNLOG.n"      # the rows the picker was actually handed
exit 130
RVSNFZF
chmod +x "$RVSNW/bin/fzf"

RVSNST=$(mktmpf)
RVSNSTORE="$RVSNST" RVSNLOG="$RVSNW/mode1" PATH="$RVSNW/bin:$PATH" \
    "$DIR/scripts/tmux-review" --loop "$RVSN" "$RVSNST" </dev/null >/dev/null 2>&1
is "the row snapshot is born 0600, not at the caller's umask" "600" "$(cat "$RVSNW/mode1" 2>/dev/null)"

# The plant: a symlink of the derivable name, pointing at a file the popup has
# no business touching.
RVSNV=$(mktmpf); printf 'victim contents\n' > "$RVSNV"
RVSNST2=$(mktmpf); ln -s "$RVSNV" "$RVSNST2.rows"
RVSNSTORE="$RVSNST2" RVSNLOG="$RVSNW/mode2" PATH="$RVSNW/bin:$PATH" \
    "$DIR/scripts/tmux-review" --loop "$RVSN" "$RVSNST2" </dev/null >/dev/null 2>&1
is "a planted symlink is not written through" "victim contents" "$(cat "$RVSNV" 2>/dev/null)"
is "the plant is unlinked, not left to be read back as rows" "none" "$(cat "$RVSNW/mode2" 2>/dev/null)"
# Refusing the cache is not refusing the review: rows_mode falls back to the
# tree, so the picker was still handed the same rows it gets uncached. A
# hardening that quietly emptied the list would pass every assertion above.
rvsnwant=$("$DIR/scripts/tmux-review" --lines "$RVSN" | grep -c .)
is "and the review still opened, with every row, uncached" "$rvsnwant" "$(cat "$RVSNW/mode2.n" 2>/dev/null)"
is "the unplanted run got the same rows from its snapshot" "$rvsnwant" "$(cat "$RVSNW/mode1.n" 2>/dev/null)"
rm -f "$RVSNV" "$RVSNST" "$RVSNST2"; rm -rf "$RVSN" "$RVSNW"

echo "== tmux-review --rows: a blank added line is still a visible, anchorable row =="
RVBL=$(mktmpd)
(
  cd "$RVBL" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > f.md; git add -A; git commit -q -m init
  printf 'a\n\nNEW\nb\n' > f.md
) >/dev/null 2>&1
RVBLS=$(mktmpf)
rvbl=$("$DIR/scripts/tmux-review" --rows "$RVBL" "$RVBLS")
rvblbad=""
while IFS= read -r row; do
    [ -n "$row" ] || continue
    rest=${row#*$'\t'}; rest=${rest#*$'\t'}; disp=${rest#*$'\t'}
    case "$disp" in *"f.md:"*) ;; *) rvblbad="display lost its path: [$row]" ;; esac
done <<<"$rvbl"
is "every row's display names its file, blank fragment or not" "" "$rvblbad"
rvblcount=$(printf '%s\n' "$rvbl" | grep -c .)
is "the blank line is a row of its own" "2" "$rvblcount"
case "$rvbl" in *$'\e[32m+'*) ok "an added row carries green SGR in --rows" ;;
    *) bad "an added row carries green SGR in --rows" "ESC[32m+ in a row" "$(printf '%s' "$rvbl" | cat -v)" ;; esac
case "$("$DIR/scripts/tmux-review" --lines "$RVBL")" in *$'\e['*) bad "--lines stays plain" "no SGR" "sgr found" ;;
    *) ok "--lines stays plain" ;; esac
rm -rf "$RVBL" "$RVBLS"

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

echo "== tmux-review --attach: the cap's '(diff too large)' row stores nothing =="
# C0: that row (lines_mode's giant-diff cap) has no real file or line behind
# it -- alt-enter on it used to write "## (diff too large): (+)" to the
# store anyway, a comment anchored to nothing that delivered as a bogus
# instruction the agent could not act on.
CAPSTORE=$(mktemp)
"$DIR/scripts/tmux-review" --attach "$CAPSTORE" "(diff too large)" "" \
    "diff too large to anchor line-by-line" "fix this somehow"
if [ -s "$CAPSTORE" ]; then
    bad "the cap sentinel row attaches nothing" "an empty store" "$(cat "$CAPSTORE")"
else
    ok "the cap sentinel row attaches nothing"
fi
rm -f "$CAPSTORE"

echo "== tmux-review --attach: a long line is quoted whole, so the agent can find it =="
RVLONG=$(mktmpd)
LONGLINE=$(printf 'x%.0s' $(seq 1 220))
(
  cd "$RVLONG" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\n' > f.txt; git add -A; git commit -q -m init
  printf 'a\n%s\n' "$LONGLINE" > f.txt
) >/dev/null 2>&1
RVLS=$(mktmpf)
frag=$("$DIR/scripts/tmux-review" --lines "$RVLONG" | awk -F'\t' '$2==2 {print $3}')
is "the display fragment is still capped" "160" "${#frag}"
"$DIR/scripts/tmux-review" --attach "$RVLS" f.txt 2 "$frag" "too long" "$RVLONG"
is "the stored quote is the full line" "> $LONGLINE" "$(sed -n 2p "$RVLS")"

# fzf STRIPS leading whitespace from a {n} field replacement, so the fragment
# attach() is actually handed for an indented row is the DE-INDENTED one -- and
# the stale-fragment guard, a plain prefix test against the untrimmed file line,
# then rejected every indented line and fell back to the trimmed, 160-byte
# display fragment. Every line inside a function body is indented, so the
# guard was inert for essentially all real code; the block above only passed
# because its fixture line starts at column 1. Measured through a real fzf
# 0.44.1 before the fix: a 224-char indented line stored a 156-char quote with
# the indentation gone. The fragment here is passed TRIMMED, exactly as fzf
# hands it over.
INDENTED="    $LONGLINE"
printf 'a\n%s\n%s\n' "$LONGLINE" "$INDENTED" > "$RVLONG/f.txt"
ifrag=$("$DIR/scripts/tmux-review" --lines "$RVLONG" | awk -F'\t' '$2==3 {print $3}')
ifrag=${ifrag#"${ifrag%%[![:space:]]*}"}
is "fzf's trimmed fragment is the de-indented 156-byte tail" "156" "${#ifrag}"
"$DIR/scripts/tmux-review" --attach "$RVLS" f.txt 3 "$ifrag" "indented" "$RVLONG"
is "an indented line is stored whole, indentation and all" \
   "> $INDENTED" "$(sed -n 6p "$RVLS")"
rm -rf "$RVLONG" "$RVLS"

echo "== tmux-review --detach: a row with nothing pinned says so instead of doing nothing in silence =="
RXN=$(mktmpd)
printf '#!/bin/sh\necho "$*" >> %s/log\n' "$RXN" > "$RXN/tmux"; chmod +x "$RXN/tmux"
printf '## src.go:3 (+)\n> x\nkeep\n\n' > "$RXN/store"
PATH="$RXN:$PATH" "$DIR/scripts/tmux-review" --detach "$RXN/store" src.go 9
case "$(cat "$RXN/log" 2>/dev/null)" in
    *"nothing pinned on this row"*) ok "^X on an uncommented row reports it" ;;
    *) bad "^X on an uncommented row reports it" "display-message ... nothing pinned on this row" "$(cat "$RXN/log" 2>/dev/null)" ;;
esac
is "and the store is untouched" "1" "$(grep -c '^## src.go:3 (+)' "$RXN/store")"
: > "$RXN/log"; : > "$RXN/empty"
PATH="$RXN:$PATH" "$DIR/scripts/tmux-review" --detach "$RXN/empty" src.go 3
case "$(cat "$RXN/log" 2>/dev/null)" in
    *"nothing pinned on this row"*) ok "so does ^X on an empty store" ;;
    *) bad "so does ^X on an empty store" "display-message ... nothing pinned on this row" "$(cat "$RXN/log" 2>/dev/null)" ;;
esac

echo "== tmux-review --copy: the file as it is now, then its diff; no path: the whole review's diff =="
RCP=$(mktmpd)
(
  cd "$RCP" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\n' > f; printf 'n\n' > g
) >/dev/null 2>&1
rcp=$("$DIR/scripts/tmux-review" --copy "$RCP" f)
case "$rcp" in
    "### f"*"a"$'\n'"b"*"### diff f"*"+b"*) ok "one path: the whole file, then its diff" ;;
    *) bad "one path: the whole file, then its diff" "### f / a b / ### diff f / +b" "$rcp" ;;
esac
rcpall=$("$DIR/scripts/tmux-review" --copy "$RCP")
case "$rcpall" in
    *"+b"*"+n"*|*"+n"*"+b"*) ok "no path: every file's diff, untracked included" ;;
    *) bad "no path: every file's diff, untracked included" "+b and +n" "$rcpall" ;;
esac
is "the cap row copies nothing" "" "$("$DIR/scripts/tmux-review" --copy "$RCP" '(diff too large)' | head -c 1)"

echo "== tmux-review --loop: ⇥ marks rows and alt-c copies their text, one line per row =="
if need_tmux "tmux-review alt-c"; then :; else
MC="cockpit-mc-$$"; SOCKS="$SOCKS $MC"; MCDIR=$(mktmpd); MCSTORE="$STATE/review-multicopy-$$"
(
  cd "$MCDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > src.go; git add -A; git commit -q -m init; printf 'a\nFIRST\nSECOND\nTHIRD\n' > src.go
) >/dev/null 2>&1
"$TMUX_REAL" -L "$MC" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
MCP=$("$TMUX_REAL" -L "$MC" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$MC" set-buffer OLD
"$TMUX_REAL" -L "$MC" send-keys -t "$MCP" "'$DIR/scripts/tmux-review' --loop '$MCDIR' '$MCSTORE'" Enter
until_cap "$MC" "$MCP" '*review>*' || bad "the picker comes up" "review>" "$("$TMUX_REAL" -L "$MC" capture-pane -p -t "$MCP")"
# ⇥ is fzf's own toggle+down (man fzf, KEY BINDINGS: `toggle+down  ctrl-i (tab)`),
# so two Tabs mark the first two rows and leave the cursor on the third.
"$TMUX_REAL" -L "$MC" send-keys -t "$MCP" Tab Tab; until_cap "$MC" "$MCP" '*(2)*'   # fzf's inline info shows the mark count in parentheses
"$TMUX_REAL" -L "$MC" send-keys -t "$MCP" M-c
until_buffer() { local i=0; while [ $i -lt 50 ]; do [ "$("$TMUX_REAL" -L "$MC" show-buffer 2>/dev/null)" != OLD ] && return 0; sleep 0.1; i=$((i+1)); done; return 1; }
until_buffer
is "alt-c copies the marked rows' text, one per line, in mark order" $'FIRST\nSECOND' "$("$TMUX_REAL" -L "$MC" show-buffer)"
# --multi must not change what ↵ pins: {1} {2} {3} are still the CURSOR row,
# so with FIRST and SECOND marked and the cursor on THIRD (src.go:4) it pins
# exactly one comment, and on THIRD -- never on the marked rows.
"$TMUX_REAL" -L "$MC" send-keys -t "$MCP" "one comment only"; until_cap "$MC" "$MCP" '*one comment only*'
"$TMUX_REAL" -L "$MC" send-keys -t "$MCP" Enter
until_file "$MCSTORE" '*one comment only*'
is "↵ with two rows marked still pins exactly one comment, on the cursor row" "1" "$(grep -c '^## src.go:4 (+)' "$MCSTORE" 2>/dev/null)"
is "and pins nothing anywhere else" "1" "$(grep -c '^## ' "$MCSTORE" 2>/dev/null)"
"$TMUX_REAL" -L "$MC" send-keys -t "$MCP" Escape; until_cap "$MC" "$MCP" '*discard 1 comment*'
"$TMUX_REAL" -L "$MC" send-keys -t "$MCP" Down Down Enter; until_cap "$MC" "$MCP" '*$*'
"$TMUX_REAL" -L "$MC" kill-server >/dev/null 2>&1
unset -f until_buffer
fi

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
until_cap "$RVS" "$RVP" '*fix the new line*'
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" M-Enter
until_file "$RVSTORE" '*fix the new line*'

case "$(cat "$RVSTORE" 2>/dev/null)" in
    *"## src.go:3 (+)"*"> CHANGED"*"fix the new line"*)
        ok "alt-enter pins path:line, the quoted fragment and the comment" ;;
    *) bad "alt-enter pins path:line, the quoted fragment and the comment" \
         "## src.go:3 (+) / > CHANGED / fix the new line" "$(cat "$RVSTORE" 2>/dev/null)" ;;
esac

until_cap "$RVS" "$RVP" '*● src.go:3 *'
case "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" in
    *"● src.go:3 "*) ok "the reloaded list marks the commented row" ;;
    *) bad "the reloaded list marks the commented row" "a ● src.go:3 row in the pane" \
         "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" ;;
esac

# Plain enter is now the same pin action as alt-enter (alt-enter stays as an
# alias). Type a second comment and pin it with plain enter this time.
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" "plain enter comment" Enter
for i in 1 2 3 4 5 6 7 8 9 10; do grep -q "plain enter comment" "$RVSTORE" 2>/dev/null && break; sleep 0.1; done
case "$(cat "$RVSTORE" 2>/dev/null)" in
    *"plain enter comment"*) ok "plain Enter pins the typed comment" ;;
    *) bad "plain Enter pins the typed comment" "the comment in the store" "$(cat "$RVSTORE" 2>/dev/null)" ;; esac
rvbefore=$(wc -c < "$RVSTORE")
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" Enter
# Negative assertion ("nothing pinned") -- there is no event to poll for, so
# this is a bounded fixed wait, capped at 0.3s, not a blind sleep.
sleep 0.3
is "Enter with nothing typed pins nothing (no accept-all)" "$rvbefore" "$(wc -c < "$RVSTORE")"
case "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" in
    *"review>"*) ok "and the picker is still open" ;;
    *) bad "and the picker is still open" "the review> prompt" "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" ;; esac

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

# esc on a non-empty batch ASKS instead of discarding outright -- the whole
# point: one reflexive escape must not throw away a twenty-comment review.
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" Escape
for i in $(seq 1 20); do
    "$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP" | grep -q "discard 2 comment" && break
    sleep 0.1
done
case "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" in
    *"discard 2 comment"*) ok "esc on a non-empty batch asks, naming the count" ;;
    *) bad "esc on a non-empty batch asks, naming the count" \
         "a 'discard 2 comment(s)' choice" \
         "$("$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP")" ;;
esac

# Row 2 -- keep reviewing (row 1 is now "leave", covered by the ^S confirm
# block below). Back to the list, every pinned comment still there.
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" Down Enter
# fzf RESTORES the screen it covered when it exits, so the list's own text is
# back on the pane before the second picker is actually up -- a content poll
# cannot tell the restored screen from the live one. Sync on the pane's
# foreground process instead: fzf gone, then fzf again.
for i in $(seq 1 30); do
    [ "$("$TMUX_REAL" -L "$RVS" display-message -p -t "$RVP" '#{pane_current_command}')" = fzf ] || break
    sleep 0.1
done
for i in $(seq 1 30); do
    [ "$("$TMUX_REAL" -L "$RVS" display-message -p -t "$RVP" '#{pane_current_command}')" = fzf ] && break
    sleep 0.1
done
is "keep reviewing returns to the list with the store intact" \
   "$rvbefore" "$(wc -c < "$RVSTORE" 2>/dev/null)"

# Row 3 -- discard. Only this deliberate, third answer deletes the store.
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" Escape
for i in $(seq 1 20); do
    "$TMUX_REAL" -L "$RVS" capture-pane -p -t "$RVP" | grep -q "discard 2 comment" && break
    sleep 0.1
done
"$TMUX_REAL" -L "$RVS" send-keys -t "$RVP" Down Down Enter
for i in $(seq 1 20); do [ -e "$RVSTORE" ] || break; sleep 0.1; done
if [ -e "$RVSTORE" ]; then
    bad "esc discards: the store is gone" "no such file" "$RVSTORE still exists"
else
    ok "esc discards: the store is gone"
fi

"$TMUX_REAL" -L "$RVS" kill-server >/dev/null 2>&1
rm -rf "$RVDIR"
fi

echo "== tmux-review --loop: an fzf that cannot start exits instead of spinning forever =="
# `while :` re-entered the picker on every fzf exit outside {0,1}. With a
# non-empty store that path runs the discard-confirm picker first -- and when
# fzf cannot start at all (no tty), THAT one fails too, leaving $choice empty,
# which fell to `*) continue` and re-entered the loop to fail again. Measured
# before the fix: rc=124 (never exits) and 172 lines of "inappropriate ioctl
# for device" in 5 seconds, burning a fork per iteration. --loop is a
# documented public subcommand, so any invocation whose tty goes away with
# comments pinned busy-loops. setsid + </dev/null is the no-tty condition
# deterministically; the confirm picker's own exit code now decides, and
# anything that is neither a choice (0) nor esc (130) keeps the store and
# exits 0.
RVSPIN=$(mktmpd)
(
  cd "$RVSPIN" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > f.txt; git add -A; git commit -q -m init
  printf 'a\nZED\n' > f.txt
) >/dev/null 2>&1
RVSPINSTORE=$(mktmpf)
printf '## f.txt:2 (+)\n> ZED\nlook again\n\n' > "$RVSPINSTORE"
rvspinwant=$(cat "$RVSPINSTORE")
timeout 5 setsid "$DIR/scripts/tmux-review" --loop "$RVSPIN" "$RVSPINSTORE" \
    </dev/null >/dev/null 2>&1
rvspinrc=$?
if [ "$rvspinrc" -eq 124 ]; then
    bad "--loop gives up when fzf cannot start, instead of spinning" \
        "an exit within 5s" "timed out (the while : busy-loop)"
else
    ok "--loop gives up when fzf cannot start, instead of spinning"
fi
is "the pinned comments are kept, not shredded, on that exit" \
   "$rvspinwant" "$(cat "$RVSPINSTORE" 2>/dev/null)"
rm -rf "$RVSPIN" "$RVSPINSTORE"

echo "== tmux-review --loop: ^S asks first — deliver, edit the batch, or keep reviewing; esc leaves the batch pinned =="
if need_tmux "review ^S confirm"; then :; else
CS="cockpit-cs-$$"; SOCKS="$SOCKS $CS"; CSSTATE=$(mktmpd); CSDIR=$(mktmpd)
(
  cd "$CSDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init; printf 'a\nb\n' > f
) >/dev/null 2>&1
CSED=$(mktmpf); printf '#!/bin/sh\nprintf "EDITED-BY-EDITOR\\n" >> "$1"\n' > "$CSED"; chmod +x "$CSED"
"$TMUX_REAL" -L "$CS" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$CSDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$CS" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
CSAP=$("$TMUX_REAL" -L "$CS" list-panes -t t: -F '#{pane_id}')
CSCTL=$("$TMUX_REAL" -L "$CS" split-window -t t -P -F '#{pane_id}' 'exec bash'); until_cap "$CS" "$CSCTL" '*$*'
CSSESS="$CSSTATE/tmux-cockpit/review/${CSAP#%}-$(printf '%s' "$CSDIR" | cksum | cut -d' ' -f1)"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" "EDITOR=$(printf '%q' "$CSED") XDG_STATE_HOME=$(printf '%q' "$CSSTATE") '$DIR/scripts/tmux-review' -p $CSAP" Enter
review_enter "$CS" "$CSCTL"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" "one"; until_cap "$CS" "$CSCTL" '*one*'; "$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Enter; until_file "$CSSESS/comments.md" '*one*'
# esc: leave, batch kept, dir kept.
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Escape
until_cap "$CS" "$CSCTL" '*leave — 1 comment*' && ok "esc offers to leave with the batch pinned" || bad "esc offers to leave with the batch pinned" "leave — 1 comment(s) stay pinned" "$("$TMUX_REAL" -L "$CS" capture-pane -p -t "$CSCTL")"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Enter
# Leaving the review drops back into the queue it was opened from; esc there
# is what returns the control pane to its shell prompt.
until_cap "$CS" "$CSCTL" '*fleet>*'
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Escape
until_cap "$CS" "$CSCTL" '*$*'
is "leaving keeps the store" "1" "$(grep -c '^## ' "$CSSESS/comments.md" 2>/dev/null)"
# Back in: ^S asks; edit runs $EDITOR on the whole store; then deliver.
# `clear;` first: fzf restores the screen it covered when it exits, so without
# it the pane still shows the first open's "review>" list (verified: it does)
# and the wait below would match that stale text instead of the real re-open.
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" "clear; EDITOR=$(printf '%q' "$CSED") XDG_STATE_HOME=$(printf '%q' "$CSSTATE") '$DIR/scripts/tmux-review' -p $CSAP" Enter
review_enter "$CS" "$CSCTL"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" C-s
until_cap "$CS" "$CSCTL" "*deliver 1 comment(s) to *($CSAP)*" && ok "^S asks before delivering, naming the pane" || bad "^S asks before delivering, naming the pane" "deliver 1 comment(s) to *($CSAP)" "$("$TMUX_REAL" -L "$CS" capture-pane -p -t "$CSCTL")"
until_cap "$CS" "$CSCTL" "*edit the batch in ${CSED##*/} first*" && ok "and names the editor it will use, not a literal \$EDITOR" || bad "and names the editor it will use, not a literal \$EDITOR" "edit the batch in ${CSED##*/} first" "$("$TMUX_REAL" -L "$CS" capture-pane -p -t "$CSCTL")"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Down Enter      # edit the batch in $EDITOR first
until_file "$CSSESS/comments.md" '*EDITED-BY-EDITOR*' && ok "edit hands the whole store to \$EDITOR" || bad "edit hands the whole store to \$EDITOR" "EDITED-BY-EDITOR in the store" "$(cat "$CSSESS/comments.md")"
until_cap "$CS" "$CSCTL" '*review>*' && ok "and returns to the list" || bad "and returns to the list" "review>" "$("$TMUX_REAL" -L "$CS" capture-pane -p -t "$CSCTL")"
"$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" C-s; until_cap "$CS" "$CSCTL" '*deliver 1 comment*'; "$TMUX_REAL" -L "$CS" send-keys -t "$CSCTL" Enter
until_cap "$CS" "$CSAP" '*EDITED-BY-EDITOR*' && ok "deliver pastes the edited batch" || bad "deliver pastes the edited batch" "EDITED-BY-EDITOR in the agent pane" "$("$TMUX_REAL" -L "$CS" capture-pane -p -t "$CSAP")"
"$TMUX_REAL" -L "$CS" kill-server >/dev/null 2>&1
fi

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
is "and the corrupted file is set aside as .bak" "garbage before any header" "$(head -1 "$RSF.bad.bak" 2>/dev/null)"
is "and the original path is gone" "gone" "$([ -e "$RSF.bad" ] && echo present || echo gone)"
printf '## src.go:x (+)\nbad line number\n' > "$RSF.bad2"; parse_store "$RSF.bad2" 2>/dev/null
is "a header with a non-numeric line is corruption" "0" "$STORE_N"
parse_store "$RSD/does-not-exist"; is "a missing store is a blank state" "0" "$STORE_N"
# Today's writer (attach) produces records this parser reads.
RSATT="$RSD/att"; "$DIR/scripts/tmux-review" --attach "$RSATT" src.go 3 'x := 1' 'from attach' >/dev/null 2>&1
parse_store "$RSATT"; is "attach()'s store parses" "src.go/3/+/x := 1/from attach" "${STORE[0.path]:-}/${STORE[0.line]:-}/${STORE[0.side]:-}/${STORE[0.frag]:-}/${STORE[0.body]:-}"
# The boundary: nothing but comments may say tmux or git in the pure module.
is "the library touches neither tmux nor git" "0" "$(sed '/^[[:space:]]*#/d' "$RS_LIB" | grep -cE 'tmux|git')"
unset -f store_set store_dump; unset STORE STORE_N

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
rb_t0=$(date +%s%N)
rbrows=$("$DIR/scripts/tmux-review" --lines "$RB")
rb_ms=$(( ($(date +%s%N) - rb_t0) / 1000000 ))
is "the small change is still a row"      "1" "$(printf '%s\n' "$rbrows" | grep -c $'^f.c\t')"
is "the small untracked file is still a row" "1" "$(printf '%s\n' "$rbrows" | grep -c $'^small.txt\t')"
is "the big file has no rows"             "0" "$(printf '%s\n' "$rbrows" | grep -c $'^core\t')"
case "$rbrows" in
    *"(diff too large)"*"core (64 MB) skipped"*"over 32 MB"*) ok "one notice row names the skipped file and its size" ;;
    *) bad "one notice row names the skipped file and its size" "(diff too large) ... core (64 MB) skipped ... over 32 MB" "$rbrows" ;;
esac
[ "$rb_ms" -lt 1500 ] && ok "and the list comes back in under 1.5 s (${rb_ms} ms)" || bad "and the list comes back in under 1.5 s" "< 1500 ms" "${rb_ms} ms"
is "--numstat (the Stop hook's and the fleet's read) omits it too" "0" "$("$DIR/scripts/tmux-review" --copy "$RB" | grep -c '^diff --git a/core')"
printf 'big\n' > "$RB/f.c"; head -c $(( 64 * 1024 * 1024 )) /dev/zero >> "$RB/f.c"   # a TRACKED file grown past the cap
rbrows2=$("$DIR/scripts/tmux-review" --lines "$RB")
is "a tracked file grown past the cap is skipped the same way" "0" "$(printf '%s\n' "$rbrows2" | grep -c $'^f.c\t')"
case "$rbrows2" in *"f.c (64 MB) skipped"*) ok "and named" ;; *) bad "and named" "f.c (64 MB) skipped" "$rbrows2" ;; esac
rb_t0=$(date +%s%N)
rbpv=$( (cd "$RB" && "$DIR/scripts/tmux-open-path" --preview-diff core) )
rb_ms=$(( ($(date +%s%N) - rb_t0) / 1000000 ))
is "the preview of a big file is a one-line notice" "core (64 MB, not diffed: over 32 MB)" "$rbpv"
[ "$rb_ms" -lt 500 ] && ok "and it answers without hashing the file (${rb_ms} ms)" || bad "and it answers without hashing the file" "< 500 ms" "${rb_ms} ms"
rm -rf "$RB"

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

if need_tmux "review base ^B"; then :; else
RBS="cockpit-rbs-$$"; SOCKS="$SOCKS $RBS"; RBSTATE=$(mktmpd)
"$TMUX_REAL" -L "$RBS" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RB" 'exec bash' >/dev/null 2>&1
RBP=$("$TMUX_REAL" -L "$RBS" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RBS" send-keys -t "$RBP" \
    "XDG_STATE_HOME=$(printf '%q' "$RBSTATE") '$DIR/scripts/tmux-review' --loop $(printf '%q' "$RB") $(printf '%q' "$RBSTATE/c.md")" Enter
until_cap "$RBS" "$RBP" '*base: since last review*' && ok "the header names the base" || bad "the header names the base" "base: since last review" "$("$TMUX_REAL" -L "$RBS" capture-pane -p -t "$RBP")"
"$TMUX_REAL" -L "$RBS" send-keys -t "$RBP" C-b
until_cap "$RBS" "$RBP" '*base: vs main*' && ok "^B cycles to the branch base and re-snapshots" || bad "^B cycles to the branch base and re-snapshots" "base: vs main" "$("$TMUX_REAL" -L "$RBS" capture-pane -p -t "$RBP")"
is "the choice is remembered beside the store" "branch" "$(cat "$RBSTATE/c.md.base")"
# The preview is a separate tmux-open-path subprocess, memoised by its own
# content cache -- COCKPIT_REVIEW_BASE_REV has to be in that cache's key, or
# this would keep serving the since-review render (just SINCE, no committed
# line) after ^B switched the base. COMMITTED-ON-FEAT is only in the diff
# against main, never against the since-review checkpoint.
until_cap "$RBS" "$RBP" '*COMMITTED-ON-FEAT*' \
    && ok "the preview re-renders against the new base, not a cache hit from the old one" \
    || bad "the preview re-renders against the new base, not a cache hit from the old one" \
        "COMMITTED-ON-FEAT in the preview" "$("$TMUX_REAL" -L "$RBS" capture-pane -p -t "$RBP")"
"$TMUX_REAL" -L "$RBS" send-keys -t "$RBP" Escape
# Polled, not slept for: esc on an empty store discards straight away
# (run_loop's own drop_store), which now has to shed ".base" beside the store
# too -- left behind, it would make the NEXT review of this same store resume
# a discarded review's base instead of starting at auto.
rbbasegone=0
for i in $(seq 1 20); do
    [ -e "$RBSTATE/c.md.base" ] || { rbbasegone=1; break; }
    sleep 0.1
done
if [ "$rbbasegone" = 1 ]; then
    ok "a discard sheds the base file beside the store, not just the store"
else
    bad "a discard sheds the base file beside the store, not just the store" \
        "$RBSTATE/c.md.base gone" "still present"
fi
"$TMUX_REAL" -L "$RBS" kill-server >/dev/null 2>&1
fi
rm -rf "$RB"

echo "== tmux-review --loop: ^A approves — the ref is the tree, the dir is gone, the next review is the delta =="
if need_tmux "review ^A"; then :; else
RA="cockpit-ra-$$"; SOCKS="$SOCKS $RA"; RASTATE=$(mktmpd); RADIR=$(mktmpd)
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
# new-window, not split-window: a split pane in a 30-row window shrinks to a
# handful of rows once RACTL2 joins it too, and fzf silently drops header
# lines (the base/↺ line first) once the picker does not fit -- reproduced.
# Each control shell gets its own full-height window instead; -p still
# targets RAAP explicitly, so which window the picker runs from is moot.
ra_shell() { local p i; p=$("$TMUX_REAL" -L "$RA" new-window -d -t t: -P -F '#{pane_id}' 'exec bash'); for i in $(seq 1 40); do [ "$("$TMUX_REAL" -L "$RA" display-message -p -t "$p" '#{pane_current_command}')" = bash ] && break; sleep 0.1; done; printf '%s' "$p"; }
RACTL=$(ra_shell)
RASESS="$RASTATE/tmux-cockpit/review/${RAAP#%}-$(printf '%s' "$RADIR" | cksum | cut -d' ' -f1)"
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" "XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $RAAP" Enter
review_enter "$RA" "$RACTL"
# One pinned comment: ^A must refuse -- approve means "nothing more to say".
# Down first: new.txt sorts before src.go (lines_mode's own order), and the
# comment has to land on src.go for the ↺ assertions below to mean anything.
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" Down
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" "not yet"; until_cap "$RA" "$RACTL" '*not yet*'
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" Enter; until_file "$RASESS/comments.md" '*not yet*'
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" C-a
# This session has no attached client (new-session -d), so the refusal's own
# tmux display-message has nothing to render to -- confirmed a genuine no-op
# without one, not a bug to chase here. Assert the refusal's EFFECT instead:
# the picker stays open and the pinned comment is still there, marked.
until_cap "$RA" "$RACTL" '*● src.go:3 *' && ok "^A with a pinned comment refuses -- the picker stays open, the comment survives" || bad "^A with a pinned comment refuses -- the picker stays open, the comment survives" "● src.go:3 " "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RACTL")"
is "and nothing was approved" "" "$(git -C "$RADIR" rev-parse -q --verify refs/cockpit/reviewed/main 2>/dev/null)"
# The picker is genuinely back in its own prompt (not fzf mid-relaunch) before
# the next key goes in -- closes a race where C-s lands while fzf is still
# tearing down and re-drawing from the refusal.
until_cap "$RA" "$RACTL" '*review>*'
# Deliver it (the pane is idle: a paste), then re-open and approve.
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" C-s
until_cap "$RA" "$RACTL" '*deliver 1 comment*'; "$TMUX_REAL" -L "$RA" send-keys -t "$RACTL" Enter
until_cap "$RA" "$RAAP" '*not yet*' && ok "the batch lands in the agent pane" || bad "the batch lands in the agent pane" "not yet" "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RAAP")"
until_file "$RASESS/delivered.md" '*not yet*' && ok "a delivered batch is archived as delivered.md" || bad "a delivered batch is archived as delivered.md" "not yet in delivered.md" "$(ls "$RASESS" 2>/dev/null)"
is "the store itself is gone" "absent" "$([ -e "$RASESS/comments.md" ] && echo present || echo absent)"
RACTL2=$(ra_shell)
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL2" "XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $RAAP" Enter
review_enter "$RA" "$RACTL2"
until_cap "$RA" "$RACTL2" '*↺ src.go:3 *' && ok "on re-open the delivered comment's row reads ↺" || bad "on re-open the delivered comment's row reads ↺" "↺ src.go:3 " "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RACTL2")"
until_cap "$RA" "$RACTL2" '*1 delivered earlier*' && ok "and the header counts it" || bad "and the header counts it" "↺ 1 delivered earlier" "$("$TMUX_REAL" -L "$RA" capture-pane -p -t "$RACTL2")"
# approve() works on a cp -p COPY of the index (tree_diff's own discipline) --
# the agent's real index must read back byte-unchanged mtime-wise.
RAIDXBEFORE=$(stat -c %Y "$RADIR/.git/index" 2>/dev/null)
"$TMUX_REAL" -L "$RA" send-keys -t "$RACTL2" C-a
for i in $(seq 1 40); do [ -e "$RASESS" ] || break; sleep 0.1; done
is "^A removes the session dir" "gone" "$([ -e "$RASESS" ] && echo present || echo gone)"
is "approve never writes the real index" "$RAIDXBEFORE" "$(stat -c %Y "$RADIR/.git/index" 2>/dev/null)"
is "^A clears the pane's review state" "" "$("$TMUX_REAL" -L "$RA" show-option -pqv -t "$RAAP" @cockpit_review)"
RAREF=$(git -C "$RADIR" rev-parse -q --verify refs/cockpit/reviewed/main 2>/dev/null)
is "^A wrote the checkpoint ref to a tree" "tree" "$(git -C "$RADIR" cat-file -t "$RAREF" 2>/dev/null)"
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
"$TMUX_REAL" -L "$RA" send-keys -t "$SRCTL" "XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $SRAP" Enter
review_enter "$RA" "$SRCTL"
"$TMUX_REAL" -L "$RA" send-keys -t "$SRCTL" C-a
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
"$TMUX_REAL" -L "$RA" send-keys -t "$QSCTL" "XDG_STATE_HOME=$(printf '%q' "$RASTATE") '$DIR/scripts/tmux-review' -p $QSAP" Enter
review_enter "$RA" "$QSCTL"
# The picker is up, so review_pane has made the dir: drop into it exactly what a
# queued delivery leaves behind, then approve.
printf 'IN-FLIGHT REVIEW\n' > "$QSSESS/comments.md.deliver"
printf 'an earlier batch\n' > "$QSSESS/delivered.md"
"$TMUX_REAL" -L "$RA" set-option -p -t "$QSAP" @cockpit_review_spool "$QSSESS/comments.md.deliver" >/dev/null 2>&1
"$TMUX_REAL" -L "$RA" send-keys -t "$QSCTL" C-a
for i in $(seq 1 40); do [ -e "$QSSESS/delivered.md" ] || break; sleep 0.1; done
is "^A leaves a queued review's spool alone" "IN-FLIGHT REVIEW" "$(cat "$QSSESS/comments.md.deliver" 2>/dev/null)"
is "and the pane's spool option still points at it" "$QSSESS/comments.md.deliver" \
   "$("$TMUX_REAL" -L "$RA" show-option -pqv -t "$QSAP" @cockpit_review_spool)"
is "the session dir stands while the spool is in it" "present" "$([ -d "$QSSESS" ] && echo present || echo gone)"
is "but ^A took the store and delivered.md" "gone gone" \
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
    display-popup) [ -n "\${RQ_NO_RC:-}" ] || printf '20\n' > "$RQSESS/rc"; exit 0 ;;
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
if XDG_RUNTIME_DIR="$RTD" timeout 0.05 "$DIR/scripts/tmux-open-path" --preview-diff "$PV/big.txt" > "$STATE/pv2" 2>/dev/null; then ok "a repeat render returns in under 50ms"
else bad "a repeat render returns in under 50ms" "cache hit" "timeout"; fi
is "and is identical" "" "$(cmp "$STATE/pv1" "$STATE/pv2" 2>&1)"
# The 50ms tooth alone does not prove the cache is READ. On this fixture a
# HIT costs 24-29ms and a MISS 39-43ms, so 50ms separates neither: a mutant
# copy of the script whose cache read is dead passes all three assertions
# above unchanged. Only a row that could not have come from anywhere else
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
rm -rf "$PV"

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
# a spawn settle, not an event: nothing to poll until the shell prints
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
# @cockpit_review=sent until the human actually sends it, so prefix J can walk
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
# And prefix J walks to it. @panestate is `running` here (agent-state just set
# it), so the pane is NOT needs-input: the jump can only come from the review
# mark, not from the blocked-pane path the earlier tmux-jump block covers.
"$TMUX_REAL" -L "$RDS" set-option -p -t "$AP" @cockpit_review sent >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" select-pane -t "$NP" >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" send-keys -t "$NP" "$(shq_test "$DIR/scripts/tmux-jump")" Enter
for i in $(seq 1 20); do
    [ "$("$TMUX_REAL" -L "$RDS" display-message -p -t t: '#{pane_id}')" = "$AP" ] && break
    sleep 0.1
done
is "prefix J walks to a pane holding an unsent review" "$AP" \
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
# a spawn settle, not an event: nothing to poll until the shell prints
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
until_cap "$RG" "$NPG" '*HAZARD-MARKER*'
case "$(gcap "$NPG")" in
    *HAZARD-MARKER*) ok "an unconditional paste into the shell DOES land (the hazard is real)" ;;
    *) bad "an unconditional paste into the shell DOES land (the hazard is real)" \
         "HAZARD-MARKER in the shell" "$(gcap "$NPG")" ;;
esac
# Clear the shell's readline of the pasted-but-unsent hazard line before the
# real assertion, so a later match cannot see it.
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
printf 'NEW-OK\n' | "$TMUX_REAL" -L "$RG" load-buffer -b rg-newok -
"$TMUX_REAL" -L "$RG" if-shell -F -t "$APG" \
    '#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}}' \
    "paste-buffer -p -r -d -b rg-newok -t $APG ; display-message pasted" \
    "display-message 'tmux-review: $APG is no longer an agent — not delivered'"
is "a real delivery still reports RC=0" "0" "$(rc_from_buffer rg-newok)"

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
"$TMUX_REAL" -L "$FD" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$FD" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
CTL2=$("$TMUX_REAL" -L "$FD" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" \
    "export XDG_STATE_HOME=$(shq_test "$FDTMPDIR")" Enter
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
# a spawn settle, not an event: nothing to poll until the shell prints
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
sf=$(ls -d "$FDTMPDIR"/tmux-cockpit/review/*/comments.md 2>/dev/null | head -1)
[ -n "$sf" ] && storefile="$sf"
if [ -n "$storefile" ] && [ -e "$storefile" ]; then
    ok "the review's comment store exists while the popup is open"
else
    bad "the review's comment store exists while the popup is open" \
        "one */comments.md under $FDTMPDIR/tmux-cockpit/review" "none"
fi

"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" "fix this"
until_cap "$FD" "$CTL2" '*fix this*'
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" M-Enter
until_cap "$FD" "$CTL2" '*● src.go:3 *'
"$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" C-s
until_cap "$FD" "$CTL2" '*deliver 1 comment*'; "$TMUX_REAL" -L "$FD" send-keys -t "$CTL2" Enter
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
    "export XDG_STATE_HOME=$(shq_test "$RKTMPDIR"); printf 'RKTMP=%s\\n' \"\$XDG_STATE_HOME\"" Enter
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
        *"review>"*) break ;;
    esac
    sleep 0.25
done

rkstore=""
# The glob names comments.md itself, not its ".rows" snapshot beside it, so
# head -1 is just belt-and-suspenders against a second session dir.
rksf=$(ls -d "$RKTMPDIR"/tmux-cockpit/review/*/comments.md 2>/dev/null | head -1)
[ -n "$rksf" ] && rkstore="$rksf"
if [ -n "$rkstore" ] && [ -s "$rkstore.rows" ]; then
    ok "the row snapshot is on disk while the popup is open"
else
    bad "the row snapshot is on disk while the popup is open" \
        "$rkstore.rows non-empty" "$(ls -la "$RKTMPDIR" 2>&1)"
fi
"$TMUX_REAL" -L "$RK" send-keys -t "$RKCTL" "keep me"
for i in $(seq 1 20); do
    "$TMUX_REAL" -L "$RK" capture-pane -p -t "$RKCTL" | grep -q "review> keep me" && break
    sleep 0.1
done
"$TMUX_REAL" -L "$RK" send-keys -t "$RKCTL" M-Enter
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

# ^S: the loop exits 20, review_pane fires --deliver through run-shell -b, and
# deliver() queues instead of pasting. Wait for the picker's own process to go
# away first, so the run-shell chain has actually been reached.
"$TMUX_REAL" -L "$RK" send-keys -t "$RKCTL" C-s
until_cap "$RK" "$RKCTL" '*deliver 1 comment*'; "$TMUX_REAL" -L "$RK" send-keys -t "$RKCTL" Enter
for i in $(seq 1 30); do
    [ "$("$TMUX_REAL" -L "$RK" display-message -p -t "$RKCTL" '#{pane_current_command}')" = fzf ] || break
    sleep 0.1
done
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
# "<store>.rows" served the popup, which is already gone. Polled, not slept
# for: the rm rides the same backgrounded `run-shell -b` as the delivery.
rkrowsgone=0
if [ -n "$rkstore" ]; then
    for i in $(seq 1 20); do
        [ -e "$rkstore.rows" ] || { rkrowsgone=1; break; }
        sleep 0.1
    done
fi
if [ "$rkrowsgone" = 1 ]; then
    ok "and sheds the row snapshot beside it"
else
    bad "and sheds the row snapshot beside it" \
        "$rkstore.rows removed" "$(ls -la "$RKTMPDIR" 2>&1)"
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
PQCTL=$("$TMUX_REAL" -L "$PQ" split-window -t t:0 -P -F '#{pane_id}' 'exec bash'); until_cap "$PQ" "$PQCTL" '*$*'
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" "XDG_STATE_HOME=$(printf '%q' "$PQSTATE") '$DIR/scripts/tmux-review' -p $PQAP" Enter
until_cap "$PQ" "$PQCTL" '*fleet>*' && ok "prefix r from an agent pane opens the queue" || bad "prefix r from an agent pane opens the queue" "fleet>" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")"
# The pointer sits on the row of the pane the key was pressed in, not on the
# needs-input pane that sorts first.
case "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")" in
    *"> ◆ done"*) ok "with that pane preselected (its row shows ◆ for the pending review)" ;;
    *) bad "with that pane preselected (its row shows ◆ for the pending review)" "> ◆ done …" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")" ;;
esac
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" Enter
until_cap "$PQ" "$PQCTL" '*review>*' && ok "↵ reviews it" || bad "↵ reviews it" "review>" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")"
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" Escape
until_cap "$PQ" "$PQCTL" '*fleet>*' && ok "esc from the review returns to the queue" || bad "esc from the review returns to the queue" "fleet>" "$("$TMUX_REAL" -L "$PQ" capture-pane -p -t "$PQCTL")"
"$TMUX_REAL" -L "$PQ" send-keys -t "$PQCTL" Escape
until_cap "$PQ" "$PQCTL" '*$*'
"$TMUX_REAL" -L "$PQ" kill-server >/dev/null 2>&1
fi

# group: serial
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
printf 'x\n' > "$FQ_DIRTY/new1.txt"                  # + three one-line untracked files
printf 'y\n' > "$FQ_DIRTY/new2.txt"
printf 'z\n' > "$FQ_DIRTY/new3.txt"
FQ_CLEAN=$(mktmpd);    mk_git_dir "$FQ_CLEAN" 0      # agent, clean -- excluded
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
# a spawn settle, not an event: nothing to poll until the shell prints
sleep 0.3

# A review pasted into this pane and not yet sent: the row has to SAY so, or
# the queue sends you back to a pane you already reviewed.
"$TMUX_REAL" -L "$FQ" set-option -p -t "$DIRTY_PANE" @cockpit_review sent >/dev/null 2>&1
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
is "the queue lists exactly two rows" "2" "$rowcount"

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
# Polled, not slept for: the row we are waiting on is the blocked pane's, and
# its absence is also the RED failure -- so the loop bounds the wait rather
# than deciding the outcome.
fqout2=""
for i in $(seq 1 20); do
    fqout2=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")
    case "$fqout2" in *"$CLEAN_PANE"$'\t'*) break ;; esac
    sleep 0.1
done
is "a blocked agent with a clean tree is listed first" "$CLEAN_PANE" \
   "$(printf '%s\n' "$fqout2" | grep '^%' | head -1 | cut -f1)"
is "its state is in the row" "needs-input" \
   "$(printf '%s\n' "$fqout2" | grep '^%' | head -1 | cut -f2)"
is "and a clean tree shows no churn instead of +0 -0" "—" \
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
# Polled on the row we assert about, same reason as the loop above.
fqout3=""
for i in $(seq 1 20); do
    fqout3=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")
    case "$fqout3" in *"$NOREPO_PANE"$'\t'*) break ;; esac
    sleep 0.1
done
nrrow=$(printf '%s\n' "$fqout3" | awk -F'\t' -v p="$NOREPO_PANE" '$1 == p { print; exit }')
is "a blocked agent outside a repo reads its repo and branch as unknown" "?⑂?" \
   "$(printf '%s\n' "$nrrow" | cut -f4)"
is "and its ± column reads —, not an empty field" "—" \
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
fqout4=""
for i in $(seq 1 20); do
    fqout4=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")
    case "$fqout4" in *"$AHEAD_PANE"$'\t'*) break ;; esac
    sleep 0.1
done
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

"$TMUX_REAL" -L "$FQ" kill-server >/dev/null 2>&1
rm -rf "$FQ_DIRTY" "$FQ_CLEAN" "$FQ_NONAGENT" "$FQ_DIRTY2" "$FQ_NOREPO" "$FQ_AHEAD" "$FQ_BIG"
fi

echo "== tmux-review --fleet: gathers per-pane git state in parallel =="
# fleet_rows() used to run each pane's git status+numstat SERIALLY, one pane
# after the next -- ~2 git calls/pane. On a cold filesystem (150ms/git,
# measured) 100 panes blanked the popup for ~32s, on every prefix+r from a
# non-agent pane. A fixed-latency git shim stands in for that cold filesystem
# here: real git still runs behind it, so this measures the serial-vs-parallel
# SHAPE, not a fabricated number. 50 panes x 2 calls x 100ms serial = ~10s;
# verified against the pre-fix script directly (11.16s, unbounded). Bounded
# parallel dispatch should land well under that.
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
    "$TMUX_REAL" -L "$FP" new-window -t t: -c "$FP_DIR" 'exec -a claude bash' >/dev/null 2>&1
    i=$((i + 1))
done
FP_SOCKPATH=$("$TMUX_REAL" -L "$FP" display-message -p '#{socket_path}')

FP_SLOWGIT=$(mktmpd)
FP_REALGIT=$(command -v git)
cat > "$FP_SLOWGIT/git" <<SHIMEOF
#!/bin/sh
sleep 0.1
exec "$FP_REALGIT" "\$@"
SHIMEOF
chmod +x "$FP_SLOWGIT/git"

fp_t0=$(date +%s%N)
timeout 15 env TMUX="$FP_SOCKPATH,0,0" PATH="$FP_SLOWGIT:$PATH" \
    "$DIR/scripts/tmux-review" --fleet > "$STATE/fp-out" 2>&1
fp_rc=$?
fp_t1=$(date +%s%N)
fp_ms=$(( (fp_t1 - fp_t0) / 1000000 ))

if [ "$fp_rc" -ne 0 ]; then
    bad "the fleet queue gathers 50 panes' git state" "exit 0" "exit $fp_rc"
elif [ "$fp_ms" -lt 5000 ]; then
    ok "the fleet queue gathers 50 panes' git state well under the old serial time ($fp_ms ms)"
else
    bad "the fleet queue gathers 50 panes' git state well under the old serial time" \
        "< 5000 ms" "$fp_ms ms"
fi
fp_rows=$(grep -c . "$STATE/fp-out" || true)
is "all 50 dirty panes are still in the rows" "50" "$fp_rows"

"$TMUX_REAL" -L "$FP" kill-server >/dev/null 2>&1
rm -rf "$FP_DIR"
fi

# group: serial
echo "== tmux-review --fleet: dedups panes sharing a repo root and scans 20 panes under a second =="
# fleet_rows() used to cost one status+numstat scan PER PANE even when two
# panes were cd'd into the exact same worktree -- resolving the repo root now
# happens first (fleet_rows:~1180), panes are grouped on it, and
# fleet_scan_root() runs once per UNIQUE root, not once per pane. 20 windows,
# 19 fixture repos -- pane 20 reuses pane 1's directory -- so 20 rows still
# come out, but only 19 `git status` calls should ever run. `timeout 1` is the
# hard timing gate (real git on tiny fixture repos, no shim latency): the
# pre-fix bounded-parallel dispatcher (cap 8) already clears this easily, so
# it is the status-call COUNT below, not this timing, that is expected to go
# red against the pre-fix script.
if need_tmux "tmux-review --fleet dedup + 20-pane timing"; then :; else
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
    "$TMUX_REAL" -L "$FD" new-window -t t: -c "$wd" 'exec -a claude bash' >/dev/null 2>&1
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
    "PATH='$FD_SHIM:'\"\$PATH\" timeout 1 '$DIR/scripts/tmux-review' --fleet > $STATE/fd-out; echo RC=\$?" Enter
fd_out=""
for i in $(seq 1 50); do
    fd_out=$("$TMUX_REAL" -L "$FD" capture-pane -p -J -t "$CTL")
    # The typed-but-not-yet-run command line already contains the literal
    # text "echo RC=$?" the instant send-keys lands it -- a bare *RC=* match
    # breaks on that unevaluated line before the command has even started.
    # Only a REAL exit code prints digits after the "=", so require one.
    case "$fd_out" in *RC=[0-9]*) break ;; esac
    sleep 0.1
done

is "20 panes scan in under 1s" "RC=0" \
   "$(printf '%s\n' "$fd_out" | grep -o 'RC=[0-9-]*' | tail -1)"

fd_rows=$(grep -c . "$STATE/fd-out" || true)
is "20 rows for 20 panes even though two share one repo" "20" "$fd_rows"

fd_diff_calls=$(grep -c ' diff ' "$FD_LOG" || true)
is "the shared repo is scanned once -- 19 diffs for 19 unique roots" \
    "19" "$fd_diff_calls"
# The budget above is forks: 19 roots in 1 s is ~50 ms each and one git is
# 3-5 ms. Phase 3's per-review base took a dirty root from 4 forks to 13 and
# blew it. Pinned as a count -- one rev-parse per pane to find its root, five
# per unique root (rev-parse, add -N, for-each-ref, diff, and big_files'
# ls-files, which is what keeps a 1 GB file from costing that root 26 s) -- so
# the next helper that costs a fork per root fails here on any box, not only a
# busy one. The wall-clock gates above are unchanged and still hold: 20 panes
# under 1 s, 50 panes 1818 ms against a 5000 ms bar.
fd_git_calls=$(grep -c . "$FD_LOG" || true)
is "one fork per pane and five per dirty root, no more" "1" \
   "$([ "$fd_git_calls" -le $((20 + 19 * 5)) ] && echo 1 || echo "$fd_git_calls")"

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
tmux -L "$ROUTERA" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $RINNER attach -t cliA" >/dev/null 2>&1
sleep 1
tmux -L "$ROUTERB" -f /dev/null new-session -d -s o -x 80 -y 24 \
    "$TMUX_REAL -L $RINNER attach -t cliB" >/dev/null 2>&1
sleep 1
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
tmux -L "$ROUTERA" kill-server >/dev/null 2>&1
tmux -L "$ROUTERB" kill-server >/dev/null 2>&1
tmux -L "$RINNER" kill-server >/dev/null 2>&1
rm -rf "$RDA" "$RDB" "$RFZFSTUB"
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

# @cockpit_review_window on: the same script and the same -p, opened as a real
# window instead of the modal popup, so the fleet's borders and tabs stay
# visible while you read.
RB3="cockpit-rb3-$$"; SOCKS="$SOCKS $RB3"
"$TMUX_REAL" -L "$RB3" -f /dev/null new-session -d -s t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$RB3" set -g @cockpit_review_window on >/dev/null 2>&1
"$TMUX_REAL" -L "$RB3" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; sleep 0.3
case "$("$TMUX_REAL" -L "$RB3" list-keys -T prefix 2>/dev/null | awk '$4=="r"')" in
    *new-window*tmux-review*) ok "@cockpit_review_window on opens a window instead of the popup" ;;
    *) bad "@cockpit_review_window on opens a window instead of the popup" \
         "a new-window running tmux-review" \
         "$("$TMUX_REAL" -L "$RB3" list-keys -T prefix | awk '$4=="r"')" ;;
esac
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
for want in "pane      $INAP" "state     needs-input  · run make?" "review    pending" "base      uncommitted" "session   sid-9" "title     Fix the parser" "1 pinned · 2 delivered"; do
    case "$inout" in *"$want"*) ok "info shows: $want" ;; *) bad "info shows: $want" "$want" "$inout" ;; esac
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
    *tmux-review*--info*) ok "prefix i is bound to the info popup" ;;
    *) bad "prefix i is bound to the info popup" "a display-popup running tmux-review --info" "$("$TMUX_REAL" -L "$IN" list-keys -T prefix | awk '$4=="i"')" ;;
esac
"$TMUX_REAL" -L "$IN" kill-server >/dev/null 2>&1
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

HOME="$RCHOME" "$TMUX_REAL" -L "$RC" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1; sleep 0.3

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
sleep 1
if "$TMUX_REAL" -L "$RCO" list-panes -t o >/dev/null 2>&1; then
    "$TMUX_REAL" -L "$RCO" send-keys -t "$ROP" -H 02 52 >/dev/null 2>&1
    sleep 0.5
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

echo "== tmux-review --loop: ^E hands the comment to \$EDITOR and pins what comes back =="
# The fzf query line is a one-line box; a thorough comment cost more than a
# shallow one. ^E opens $EDITOR on what was typed so far and pins the file.
if need_tmux "tmux-review ^E"; then :; else
RE="cockpit-re-$$"; SOCKS="$SOCKS $RE"
REDIR=$(mktmpd)
(
  cd "$REDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
RESTORE="$STATE/review-edit-$$"; RETMP=$(mktmpd)
REED=$(mktmpf); printf '#!/bin/sh\nprintf "from the editor\\nsecond line\\n" >> "$1"\n' > "$REED"; chmod +x "$REED"
"$TMUX_REAL" -L "$RE" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
REP=$("$TMUX_REAL" -L "$RE" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" \
    "TMPDIR=$(printf '%q' "$RETMP") VISUAL= EDITOR=$(printf '%q' "$REED") '$DIR/scripts/tmux-review' --loop '$REDIR' '$RESTORE'" Enter
until_cap "$RE" "$REP" '*review>*' || bad "the picker comes up" "review>" "$("$TMUX_REAL" -L "$RE" capture-pane -p -t "$REP")"
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" "typed first"; until_cap "$RE" "$REP" '*typed first*'
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" C-e
if until_file "$RESTORE" '*## src.go:3 (+)*> CHANGED*typed first*from the editor*second line*'; then
    ok "^E pre-fills the typed text, the editor's file is the comment, multi-line"
else bad "^E pre-fills the typed text, the editor's file is the comment, multi-line" \
     "typed first / from the editor / second line" "$(cat "$RESTORE" 2>/dev/null)"; fi
until_cap "$RE" "$REP" '*● src.go:3 *' && ok "and the row is marked, query cleared" \
    || bad "and the row is marked, query cleared" "● src.go:3 " "$("$TMUX_REAL" -L "$RE" capture-pane -p -t "$REP")"
case "$("$TMUX_REAL" -L "$RE" capture-pane -p -t "$REP")" in
    *"review> typed first"*) bad "the query is cleared after ^E" "review> " "still typed first" ;;
    *) ok "the query is cleared after ^E" ;;
esac
is "no editor temp file is left behind, and none went beside the store" "" "$(ls "$RETMP"/* "$RESTORE".edit.* 2>/dev/null)"
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" Escape
until_cap "$RE" "$REP" '*discard 1 comment*'
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" Down Down Enter
"$TMUX_REAL" -L "$RE" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --edit: the editor gets the terminal even while fzf holds stdout, and @cockpit_editor wins =="
# fzf's execute() runs its command with stdin on the tty and stdout on fzf's
# own stdout -- which run_loop captures. vi printed "Output is not to a
# terminal" and ate every key after. `| cat` below is that exact shape.
if need_tmux "tmux-review --edit tty"; then :; else
ET="cockpit-et-$$"; SOCKS="$SOCKS $ET"; ETDIR=$(mktmpd); ETTMP=$(mktmpd); ETSTORE="$STATE/review-edit-tty-$$"
(
  cd "$ETDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\nb\n' > src.go; git add -A; git commit -q -m init; printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
# The -t 1 test runs BEFORE the append: `if ...; fi >> "$1"` redirects the
# condition too, and then reports a pipe however the caller left stdout.
ETED=$(mktmpf); printf '#!/bin/sh\nif [ -t 1 ]; then r=STDOUT-IS-TTY; else r=STDOUT-IS-A-PIPE; fi\necho "$r" >> "$1"\n' > "$ETED"; chmod +x "$ETED"
ETED2=$(mktmpf); printf '#!/bin/sh\necho FROM-COCKPIT-EDITOR >> "$1"\n' > "$ETED2"; chmod +x "$ETED2"
"$TMUX_REAL" -L "$ET" -f /dev/null new-session -d -s t -x 100 -y 30 'exec bash' >/dev/null 2>&1
ETP=$("$TMUX_REAL" -L "$ET" list-panes -t t: -F '#{pane_id}'); until_cap "$ET" "$ETP" '*$*'
"$TMUX_REAL" -L "$ET" send-keys -t "$ETP" "TMPDIR=$(printf '%q' "$ETTMP") VISUAL= EDITOR=$(printf '%q' "$ETED") '$DIR/scripts/tmux-review' --edit $(printf '%q' "$ETSTORE") src.go 3 CHANGED '' $(printf '%q' "$ETDIR") | cat; echo ETRC=\$?" Enter
# ETRC=0, not ETRC=: the send-keys line echoes into the pane as `echo ETRC=$?`,
# so the looser glob matches the typed command and reads the store too early.
until_cap "$ET" "$ETP" '*ETRC=0*'
case "$(cat "$ETSTORE" 2>/dev/null)" in
    *STDOUT-IS-TTY*) ok "stdin a tty, stdout a pipe: the editor still writes to the terminal" ;;
    *) bad "stdin a tty, stdout a pipe: the editor still writes to the terminal" "STDOUT-IS-TTY" "$(cat "$ETSTORE" 2>/dev/null)" ;;
esac
"$TMUX_REAL" -L "$ET" set -g @cockpit_editor "$ETED2"
"$TMUX_REAL" -L "$ET" send-keys -t "$ETP" "clear; TMPDIR=$(printf '%q' "$ETTMP") VISUAL= EDITOR=$(printf '%q' "$ETED") '$DIR/scripts/tmux-review' --edit $(printf '%q' "$ETSTORE") src.go 3 CHANGED '' $(printf '%q' "$ETDIR") | cat; echo ETRC2=\$?" Enter
until_cap "$ET" "$ETP" '*ETRC2=0*'
case "$(cat "$ETSTORE" 2>/dev/null)" in
    *FROM-COCKPIT-EDITOR*) ok "@cockpit_editor wins over \$EDITOR, same chain as the path picker" ;;
    *) bad "@cockpit_editor wins over \$EDITOR, same chain as the path picker" "FROM-COCKPIT-EDITOR" "$(cat "$ETSTORE" 2>/dev/null)" ;;
esac
is "no editor temp file is left behind" "" "$(ls "$ETTMP"/* 2>/dev/null)"
"$TMUX_REAL" -L "$ET" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --edit: with nothing configured, ^E opens in VS Code's remote shim when a window is reachable, else vi =="
# On a Remote-SSH/Coder host there is no code(1) and usually no $EDITOR either,
# and ^E fell back to vi inside the popup -- next to the VS Code window the
# person is actually reading the diff in. The shim takes it with --wait, so the
# buffer comes back the same way vi's does. No live window: vi, and say so.
VETM=$(mktmpd); VEH=$(mktmpd); VE=$(mktmpd)
VESK="$VEH/run"; VELOG="$VEH/log"; VELOGT="$VETM/log"; VESTORE="$VEH/store.md"
VESHIM="$VEH/.vscode-server/cli/servers/Stable-abc/server/bin/remote-cli"
cat > "$VETM/tmux" <<'VESTUB'
#!/usr/bin/env bash
case "$1" in
  show-options)  printf '\n' ;;
  display-message) printf 'MSG %s\n' "$*" >> "$TMUX_LOG" ;;
esac
exit 0
VESTUB
chmod +x "$VETM/tmux"
mkdir -p "$VESHIM" "$VESK"
cat > "$VESHIM/code" <<'VESHIMEOF'
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
VESHIMEOF
chmod +x "$VESHIM/code"
: > "$VESK/vscode-ipc-live.sock"; touch -t 202001010000 "$VESK/vscode-ipc-live.sock"
: > "$VESK/vscode-ipc-dead.sock"
# `vi` on PATH is the fallback the last assertion measures: $EDITOR unset, so
# editor_cmd's own last resort is the bare name.
printf '#!/bin/sh\nprintf "FROM-VI\\n" >> "$1"\n' > "$VETM/vi"; chmod +x "$VETM/vi"
(
  cd "$VE" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\nb\n' > src.go; git add -A; git commit -q -m init; printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
: > "$VELOG"; : > "$VELOGT"
env -i PATH="$VETM:$PATH" HOME="$VEH" VLOG="$VELOG" XDG_RUNTIME_DIR="$VESK" TMUX_LOG="$VELOGT" \
    "$DIR/scripts/tmux-review" --edit "$VESTORE" src.go 3 CHANGED '' "$VE" </dev/null >/dev/null 2>&1
case "$(cat "$VESTORE" 2>/dev/null)" in
    *FROM-VSCODE*) ok "^E with no editor configured reaches the live window with --wait" ;;
    *) bad "^E with no editor configured reaches the live window with --wait" \
         "FROM-VSCODE in the store" "$(cat "$VESTORE" 2>/dev/null; cat "$VELOG")" ;;
esac
# The shim command carries the script's own path, and $ed is word-split on
# purpose (EDITOR="code --wait") -- so an install under "~/my dotfiles" split
# it into two words and ^E ran a command that does not exist.
: > "$VELOG"; : > "$VELOGT"; : > "$VESTORE"
mkdir -p "$VEH/my dotfiles"; cp -r "$DIR/scripts" "$VEH/my dotfiles/"
env -i PATH="$VETM:$PATH" HOME="$VEH" VLOG="$VELOG" XDG_RUNTIME_DIR="$VESK" TMUX_LOG="$VELOGT" \
    "$VEH/my dotfiles/scripts/tmux-review" --edit "$VESTORE" src.go 3 CHANGED '' "$VE" </dev/null >/dev/null 2>&1
case "$(cat "$VESTORE" 2>/dev/null)" in
    *FROM-VSCODE*) ok "and from an install path with a space in it" ;;
    *) bad "and from an install path with a space in it" \
         "FROM-VSCODE in the store" "$(cat "$VESTORE" 2>/dev/null; cat "$VELOG")" ;;
esac
rm -f "$VESK"/vscode-ipc-live.sock; : > "$VELOG"; : > "$VELOGT"; : > "$VESTORE"
env -i PATH="$VETM:$PATH" HOME="$VEH" VLOG="$VELOG" XDG_RUNTIME_DIR="$VESK" TMUX_LOG="$VELOGT" \
    "$DIR/scripts/tmux-review" --edit "$VESTORE" src.go 3 CHANGED '' "$VE" </dev/null >/dev/null 2>&1
case "$(cat "$VESTORE" 2>/dev/null)" in
    *FROM-VI*) ok "with no live window it falls back to \$EDITOR" ;;
    *) bad "with no live window it falls back to \$EDITOR" "FROM-VI" "$(cat "$VESTORE" 2>/dev/null)" ;;
esac
case "$(cat "$VELOGT" 2>/dev/null)" in
    *"no VS Code window reachable"*) ok "and the status line says which editor it used instead" ;;
    *) bad "and the status line says which editor it used instead" \
         "a display-message naming vi" "$(cat "$VELOGT" 2>/dev/null)" ;;
esac

echo "== tmux-review --loop: ^E with nothing typed on a commented row edits that comment instead of adding one =="
if need_tmux "tmux-review ^E edit in place"; then :; else
RI="cockpit-ri-$$"; SOCKS="$SOCKS $RI"; RIDIR=$(mktmpd); RITMP=$(mktmpd); RISTORE="$STATE/review-edit-inplace-$$"
(
  cd "$RIDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\nb\n' > src.go; git add -A; git commit -q -m init; printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
printf '## src.go:3 (+)\n> CHANGED\nold text\n\n' > "$RISTORE"
RIED=$(mktmpf); printf '#!/bin/sh\nsed -i "s/old text/new text/" "$1"\n' > "$RIED"; chmod +x "$RIED"
"$TMUX_REAL" -L "$RI" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
RIP=$("$TMUX_REAL" -L "$RI" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RI" send-keys -t "$RIP" \
    "TMPDIR=$(printf '%q' "$RITMP") VISUAL= EDITOR=$(printf '%q' "$RIED") '$DIR/scripts/tmux-review' --loop '$RIDIR' '$RISTORE'" Enter
until_cap "$RI" "$RIP" '*review>*' || bad "the picker comes up" "review>" "$("$TMUX_REAL" -L "$RI" capture-pane -p -t "$RIP")"
"$TMUX_REAL" -L "$RI" send-keys -t "$RIP" C-e
if until_file "$RISTORE" '*new text*'; then ok "^E opened the editor on the pinned text and the edit landed"
else bad "^E opened the editor on the pinned text and the edit landed" "new text" "$(cat "$RISTORE" 2>/dev/null)"; fi
is "the comment was replaced, not appended" "1" "$(grep -c '^## ' "$RISTORE")"
is "and the old text is gone" "0" "$(grep -c 'old text' "$RISTORE")"
"$TMUX_REAL" -L "$RI" send-keys -t "$RIP" Escape
until_cap "$RI" "$RIP" '*discard 1 comment*'
"$TMUX_REAL" -L "$RI" send-keys -t "$RIP" Down Down Enter
"$TMUX_REAL" -L "$RI" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --loop: ^E then quitting the editor unchanged writes nothing to the store =="
# In-place editing is detach+attach, and two comments on one row come back as
# ONE buffer -- so a ^E someone quits out of used to silently merge them, ●2
# to ●, with no undo. :q has to be a cancel.
if need_tmux "tmux-review ^E cancel"; then :; else
RQ="cockpit-rq-$$"; SOCKS="$SOCKS $RQ"; RQDIR=$(mktmpd); RQTMP=$(mktmpd); RQSTORE="$STATE/review-edit-cancel-$$"
(
  cd "$RQDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\nb\n' > src.go; git add -A; git commit -q -m init; printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
printf '## src.go:3 (+)\n> CHANGED\nfirst\n\n## src.go:3 (+)\n> CHANGED\nsecond\n\n' > "$RQSTORE"
cp -- "$RQSTORE" "$RQSTORE.before"
# The editor leaves its file untouched -- :q -- and says so in a file of its
# own, so the buffer stays byte-identical and the wait still has something to
# poll.
RQMARK=$(mktmpf)
RQED=$(mktmpf); printf '#!/bin/sh\necho RAN > %s\nexit 0\n' "$(printf '%q' "$RQMARK")" > "$RQED"; chmod +x "$RQED"
"$TMUX_REAL" -L "$RQ" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
RQP=$("$TMUX_REAL" -L "$RQ" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RQ" send-keys -t "$RQP" \
    "TMPDIR=$(printf '%q' "$RQTMP") VISUAL= EDITOR=$(printf '%q' "$RQED") '$DIR/scripts/tmux-review' --loop '$RQDIR' '$RQSTORE'" Enter
until_cap "$RQ" "$RQP" '*review>*' || bad "the picker comes up" "review>" "$("$TMUX_REAL" -L "$RQ" capture-pane -p -t "$RQP")"
"$TMUX_REAL" -L "$RQ" send-keys -t "$RQP" C-e
until_file "$RQMARK" '*RAN*' || bad "^E ran the editor" "RAN" "$(cat "$RQMARK" 2>/dev/null)"
# ctrl-e is execute()+reload+clear-query and fzf reads no key while execute()
# runs, so a query that shows the text typed AFTER ^E is proof edit_comment
# already returned -- the store is settled, whatever it decided to write.
"$TMUX_REAL" -L "$RQ" send-keys -t "$RQP" "ZZQ"
until_cap "$RQ" "$RQP" '*review> ZZQ*' || bad "the picker takes keys again" "review> ZZQ" "$("$TMUX_REAL" -L "$RQ" capture-pane -p -t "$RQP")"
is "an unchanged buffer leaves the store byte-identical" "" "$(cmp -- "$RQSTORE" "$RQSTORE.before" 2>&1)"
is "two comments stay two" "2" "$(grep -c '^## src.go:3 (+)' "$RQSTORE")"
"$TMUX_REAL" -L "$RQ" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --loop: ^X deletes the comments under the cursor, the marker counts them, ^L lists them =="
# The store had a writer and no reader: three comments on one line showed
# one dot, and nothing could take one back short of discarding the batch.
# parse_store (lib/review-store.sh) is the reader; --detach is its first
# caller. ^L cats the store to the terminal, not the preview window -- a
# `focus` bind reapplying the preview's offset on every selection change
# turned out to silently un-hide the preview after ^/ (I1). No printable
# key to dismiss it either: in this loop every printable key is comment text.
if need_tmux "tmux-review ^X"; then :; else
RX="cockpit-rx-$$"; SOCKS="$SOCKS $RX"
RXDIR=$(mktmpd)
(
  cd "$RXDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  # 40 added lines, not one: ^L's own bind used to leave --preview-window's
  # +{5}-/2 (the DIFF row's scroll offset) in force over the store listing,
  # so a row more than a screen into the diff showed the LAST line of the
  # store and nothing else -- and a fixture that changes one line can never
  # reach that offset, so the round trip below pins the row it comments on
  # to the LAST added line (src.go:42, offset in the 40s) on purpose.
  { printf 'a\nb\n'; seq -f 'line%g' 1 40; } > src.go
) >/dev/null 2>&1
RXSTORE="$STATE/review-detach-$$"
# --detach on its own, through a symlink the way install.sh puts it on PATH:
# the lib has to be found beside the SCRIPT, not beside the link.
RXBIN=$(mktmpd); ln -s "$DIR/scripts/tmux-review" "$RXBIN/tmux-review"
printf '## src.go:3 (+)\n> CHANGED\nfirst\n\n## src.go:3 (+)\n> CHANGED\nsecond\n\n## other.go:9 (-)\n> gone\nkeep me\n\n' > "$RXSTORE"
"$RXBIN/tmux-review" --detach "$RXSTORE" src.go 3
is "--detach removes every record on that anchor and keeps the rest byte-for-byte" \
   $'## other.go:9 (-)\n> gone\nkeep me' "$(cat "$RXSTORE")"
"$RXBIN/tmux-review" --detach "$RXSTORE" other.go -9
is "a deletion's anchor is matched with its sign" "0" "$(grep -c '^## ' "$RXSTORE")"
"$RXBIN/tmux-review" --detach "$RXSTORE" "" ""; rxrc=$?
is "an empty anchor is a no-op, exit 0" "0" "$rxrc"
# parse_store's OWN reaction to a broken grammar is to move the file to
# .bak and hand back STORE_N=0 -- detach() used to trust that as "nothing to
# remove" and format_store the (now-blank) state straight back over the
# original path: a store that fails to parse got .bak'd AND replaced with an
# empty file, silently, under execute-silent. The unescaped "## " body line
# below is what breaks the grammar (parse_store reads it as a new header).
RXBAD="$STATE/review-detach-bad-$$"
printf '## src.go:1 (+)\n> a\nfirst body line\n## this line breaks the grammar\n\n' > "$RXBAD"
RXBADORIG=$(cat "$RXBAD")
"$RXBIN/tmux-review" --detach "$RXBAD" src.go 1; rxbadrc=$?
is "--detach still exits 0 when the store fails to parse" "0" "$rxbadrc"
is "and does not recreate the original path with an empty store" \
   "gone" "$([ -e "$RXBAD" ] && echo present || echo gone)"
is "the original content survives untouched in .bak" "$RXBADORIG" "$(cat "$RXBAD.bak" 2>/dev/null)"
: > "$RXSTORE"
"$TMUX_REAL" -L "$RX" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
RXP=$("$TMUX_REAL" -L "$RX" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" "'$DIR/scripts/tmux-review' --loop '$RXDIR' '$RXSTORE'" Enter
until_cap "$RX" "$RXP" '*review>*' || bad "the picker comes up" "review>" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"
# The row under the cursor at open is the FIRST added line (src.go:3); walk
# down to the LAST one (src.go:42) before pinning, so the row commented on
# below is the one whose diff-preview offset is deep enough (~44) to have
# been the vacuous-pass case (offset ~4) in the first version of this test.
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" -N 39 Down
until_cap "$RX" "$RXP" '*src.go:42 *+*' || bad "walked down to the last added line" "src.go:42 (+)" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" "one"; until_cap "$RX" "$RXP" '*review> one*'
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" Enter; until_file "$RXSTORE" '*one*'
if until_cap "$RX" "$RXP" '*● src.go:42 *'; then ok "one comment: the dot"
else bad "one comment: the dot" "● src.go:42 " "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"; fi
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" "two"; until_cap "$RX" "$RXP" '*review> two*'
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" Enter; until_file "$RXSTORE" '*two*'
if until_cap "$RX" "$RXP" '*2 src.go:42 *'; then ok "two comments on one row: the count, one column wide"
else bad "two comments on one row: the count, one column wide" "2 src.go:42 " "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"; fi
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" C-l
if until_cap "$RX" "$RXP" '*## src.go:42 (+)*one*## src.go:42 (+)*two*(press any key)*'; then ok "^L lists the store in the terminal, from the top, on a row deep in the diff"
else bad "^L lists the store in the terminal, from the top, on a row deep in the diff" "both records, (press any key)" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"; fi
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" Space
if until_cap "$RX" "$RXP" '*review>*2 src.go:42 *'; then ok "and the list is back, the marked row visible"
else bad "and the list is back, the marked row visible" "review> ... 2 src.go:42 " "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"; fi
# I1 regression, fail-first: the deleted `focus` bind fired on every
# selection change and reapplied the preview window, which is what silently
# un-hid the preview right after ^/ turned it off (measured on fzf 0.44.1).
# "+line" (the diff preview's own added-line prefix) is the preview-only
# marker, not "@@" the hunk header: the cursor sits on src.go:42 here, and
# --preview-window's +{5}-/2 follow scrolls the hunk header itself off
# screen even while the preview is fully visible, at ANY scroll offset --
# "+line" stays on screen because the hunk runs 40 lines. Neither the row
# list ("line40", no leading +) nor ^L's terminal listing ("> line40")
# ever prints that substring, so it stays preview-only. Gated on the
# positive first: a preview that never rendered must fail loudly, not
# pass by never showing the marker either way.
until_cap "$RX" "$RXP" '*+line*' \
    || bad "^/ hides the preview and a cursor move does not bring it back" "+line visible (a rendered preview to hide)" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" C-_   # tmux's own encoding of ctrl-/
rxcap=""
for i in $(seq 1 40); do
    rxcap="$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"
    case "$rxcap" in *'+line'*) sleep 0.1 ;; *) break ;; esac
done
case "$rxcap" in
    *'+line'*) bad "^/ hides the preview and a cursor move does not bring it back" "no +line right after ^/" "$rxcap" ;;
    *)
        "$TMUX_REAL" -L "$RX" send-keys -t "$RXP" Up
        "$TMUX_REAL" -L "$RX" send-keys -t "$RXP" Down
        rxstill=""
        for i in $(seq 1 10); do
            rxstill="$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"
            case "$rxstill" in *'+line'*) break ;; esac
            sleep 0.1
        done
        case "$rxstill" in
            *'+line'*) bad "^/ hides the preview and a cursor move does not bring it back" "still no +line after Up/Down" "$rxstill" ;;
            *) ok "^/ hides the preview and a cursor move does not bring it back" ;;
        esac ;;
esac
# Up then Down over adjacent rows (no wrap) lands back on the commented row,
# so ^X below still targets the anchor the two comments were pinned to.
until_cap "$RX" "$RXP" '*2 src.go:42 *' || bad "back on the commented row before ^X" "2 src.go:42 " "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" C-x
for i in $(seq 1 40); do grep -q '^## ' "$RXSTORE" 2>/dev/null || break; sleep 0.1; done
is "^X removes every comment under the cursor" "0" "$(grep -c '^## ' "$RXSTORE")"
if until_cap "$RX" "$RXP" '*  src.go:42 *+*'; then ok "and the row is unmarked again"
else bad "and the row is unmarked again" "  src.go:42 +" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"; fi
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" Escape
for i in $(seq 1 40); do [ -e "$RXSTORE" ] || break; sleep 0.1; done
is "esc with nothing pinned exits without asking and drops the empty store" "gone" "$([ -e "$RXSTORE" ] && echo present || echo gone)"
"$TMUX_REAL" -L "$RX" kill-server >/dev/null 2>&1
fi

echo "== tmux-review --loop: ^R re-snapshots and names the comments the agent moved =="
# The rows are a snapshot (phase 1), so an agent writing mid-review cannot
# move a ● onto a line you never touched -- but it also means the list goes
# stale. ^R takes a new snapshot on purpose and says which comments no
# longer anchor to a changed line (they still deliver: the quoted fragment
# is what the agent finds them by). dev.md §2.2 is the repro.
if need_tmux "tmux-review ^R"; then :; else
RR="cockpit-rr-$$"; SOCKS="$SOCKS $RR"
RRDIR=$(mktmpd)
(
  cd "$RRDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
RRSTORE="$STATE/review-resnap-$$"
"$TMUX_REAL" -L "$RR" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
RRP=$("$TMUX_REAL" -L "$RR" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" "'$DIR/scripts/tmux-review' --loop '$RRDIR' '$RRSTORE'" Enter
until_cap "$RR" "$RRP" '*review>*' || bad "the picker comes up" "review>" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")"
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" "hold this"
until_cap "$RR" "$RRP" '*hold this*'
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" Enter
until_file "$RRSTORE" '*hold this*'
is "--resnap with nothing stale prints nothing and does not wait" "rc=0" \
   "$(timeout 5 "$DIR/scripts/tmux-review" --resnap "$RRDIR" "$RRSTORE" </dev/null 2>&1; echo "rc=$?")"
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" C-r
if until_cap "$RR" "$RRP" '*● src.go:3 *'; then
    case "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")" in
        *"no longer anchor"*|*"press any key"*)
            bad "^R with nothing stale returns to the list without a report" "no report text" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")" ;;
        *) ok "^R with nothing stale returns to the list without a report" ;;
    esac
else
    bad "^R with nothing stale returns to the list without a report" "● src.go:3 " "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")"
fi
# The agent inserts a line at the top: the pinned +CHANGED at 3 is now at 4.
printf 'TOP\na\nb\nCHANGED\n' > "$RRDIR/src.go"
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" "still here"
until_cap "$RR" "$RRP" '*still here*'
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" C-u    # readline: clear the typed text again
case "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")" in
    *"● src.go:3 "*) ok "until ^R the list is the snapshot: the mark stays where it was pinned" ;;
    *) bad "until ^R the list is the snapshot: the mark stays where it was pinned" "● src.go:3 " "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")" ;;
esac
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" C-r
if until_cap "$RR" "$RRP" '*no longer anchor*## src.go:3 (+)*'; then ok "^R names the comment the agent moved"
else bad "^R names the comment the agent moved" "no longer anchor ... ## src.go:3 (+)" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")"; fi
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" Space
if until_cap "$RR" "$RRP" '*src.go:4 *+*'; then ok "and the list shows the agent's new rows"
else bad "and the list shows the agent's new rows" "src.go:4 +" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")"; fi
case "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP" | grep -v '● pinned')" in
    *"●"*) bad "a moved comment marks no row" "no ●" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")" ;;
    *)     ok "a moved comment marks no row" ;;
esac
is "the moved comment is kept in the store, not dropped" "1" "$(grep -c '^## src.go:3 (+)' "$RRSTORE")"
is "--resnap names exactly the stale headers" "## src.go:3 (+)" \
   "$("$DIR/scripts/tmux-review" --resnap "$RRDIR" "$RRSTORE" </dev/null 2>&1 | grep '^## ')"
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" Escape
until_cap "$RR" "$RRP" '*discard 1 comment*'
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" Down Down Enter
"$TMUX_REAL" -L "$RR" kill-server >/dev/null 2>&1
fi

echo "== tmux-review: moving between projects leaves every review where it was =="
# Three moves the owner makes all day: prefix r on one agent, then on another
# in a different repo; one agent whose cwd moves to another repo (tmux cannot
# change a pane's cwd from outside, so the pane is respawned in the new dir --
# same pane id, which is what the session key is made of); and from a plain
# terminal window through the fleet queue back to a review. Every session dir
# is keyed on pane + root, so none of these can read another's comments.
if need_tmux "review across projects"; then :; else
PM="cockpit-pm-$$"; SOCKS="$SOCKS $PM"
PMSTATE=$(mktmpd); PMBASE=$(mktmpd)
ALPHA="$PMBASE/alpha"; BRAVO="$PMBASE/bravo"
for r in "$ALPHA" "$BRAVO"; do
  f=$(basename "$r").go
  ( mkdir -p "$r" && cd "$r" || exit
    export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
    git init -q -b main .; printf 'a\nb\n' > "$f"; git add -A; git commit -q -m init
    printf 'a\nb\nCHANGED\n' > "$f" ) >/dev/null 2>&1
done
"$TMUX_REAL" -L "$PM" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$ALPHA" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PM" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
PMA=$("$TMUX_REAL" -L "$PM" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$PM" new-window -d -t t: -c "$BRAVO" 'exec -a claude bash' >/dev/null 2>&1
PMB=$("$TMUX_REAL" -L "$PM" list-panes -t t:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$PM" new-window -d -t t: -c "$PMBASE" 'exec bash' >/dev/null 2>&1   # the terminal window
pm_shell() {                  # a fresh control pane in the terminal window, ready for a command
    local p i
    p=$("$TMUX_REAL" -L "$PM" split-window -t t:2 -P -F '#{pane_id}' 'exec bash')
    for i in $(seq 1 40); do
        [ "$("$TMUX_REAL" -L "$PM" display-message -p -t "$p" '#{pane_current_command}')" = bash ] && break
        sleep 0.1
    done
    printf '%s' "$p"
}
pm_review() {                 # <control-pane> <agent-pane>   opens that agent's review from the control pane
    "$TMUX_REAL" -L "$PM" send-keys -t "$1" \
        "XDG_STATE_HOME=$(printf '%q' "$PMSTATE") '$DIR/scripts/tmux-review' -p $2" Enter
    review_enter "$PM" "$1"
}
pm_sess() {                   # <agent-pane> <root>   the session dir the script will use
    printf '%s/tmux-cockpit/review/%s-%s' "$PMSTATE" "${1#%}" \
        "$(printf '%s' "$(git -C "$2" rev-parse --show-toplevel)" | cksum | cut -d' ' -f1)"
}
pm_cap() { "$TMUX_REAL" -L "$PM" capture-pane -p -t "$1"; }

# (a) two agents, two repos
C=$(pm_shell); pm_review "$C" "$PMA" || bad "A's review comes up" "review>" "$(pm_cap "$C")"
"$TMUX_REAL" -L "$PM" send-keys -t "$C" "for alpha"; until_cap "$PM" "$C" '*for alpha*'
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Enter
until_file "$(pm_sess "$PMA" "$ALPHA")/comments.md" '*for alpha*' && ok "a comment pinned on A lands in A's session" \
    || bad "a comment pinned on A lands in A's session" "for alpha" "$(cat "$(pm_sess "$PMA" "$ALPHA")/comments.md" 2>/dev/null)"
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"                                   # the client drops; A's review stays
C=$(pm_shell); pm_review "$C" "$PMB" || bad "B's review comes up" "review>" "$(pm_cap "$C")"
if until_cap "$PM" "$C" '*bravo.go:3 *+*'; then ok "prefix r on B shows B's repo"; else bad "prefix r on B shows B's repo" "bravo.go:3 (+)" "$(pm_cap "$C")"; fi
case "$(pm_cap "$C" | grep -v '● pinned')" in *●*) bad "and none of A's marks" "no ●" "$(pm_cap "$C")" ;; *) ok "and none of A's marks" ;; esac
is "B's session is its own, empty" "0" "$(grep -c '^## ' "$(pm_sess "$PMB" "$BRAVO")/comments.md" 2>/dev/null)"
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
C=$(pm_shell); pm_review "$C" "$PMA"
if until_cap "$PM" "$C" '*● alpha.go:3 *'; then ok "back on A, A's comment is resumed"; else bad "back on A, A's comment is resumed" "● alpha.go:3 " "$(pm_cap "$C")"; fi
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
is "two agents in two repos: two session dirs" "2" "$(ls -d "$PMSTATE"/tmux-cockpit/review/* 2>/dev/null | wc -l)"

# (b) one agent whose cwd moves to the other repo (respawned in place: same pane id)
"$TMUX_REAL" -L "$PM" respawn-pane -k -t "$PMB" -c "$ALPHA" 'exec -a claude bash'
C=$(pm_shell); pm_review "$C" "$PMB"
if until_cap "$PM" "$C" '*alpha.go:3 *+*'; then ok "an agent that moved to alpha reviews alpha"; else bad "an agent that moved to alpha reviews alpha" "alpha.go:3 (+)" "$(pm_cap "$C")"; fi
case "$(pm_cap "$C" | grep -v '● pinned')" in *●*) bad "with none of A's marks: same root, another pane, another session" "no ●" "$(pm_cap "$C")" ;; *) ok "with none of A's marks: same root, another pane, another session" ;; esac
"$TMUX_REAL" -L "$PM" send-keys -t "$C" "B in alpha"; until_cap "$PM" "$C" '*B in alpha*'
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Enter
until_file "$(pm_sess "$PMB" "$ALPHA")/comments.md" '*B in alpha*' || bad "B's alpha comment lands in B's alpha session" "B in alpha" "$(cat "$(pm_sess "$PMB" "$ALPHA")/comments.md" 2>/dev/null)"
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
"$TMUX_REAL" -L "$PM" respawn-pane -k -t "$PMB" -c "$BRAVO" 'exec -a claude bash'
C=$(pm_shell); pm_review "$C" "$PMB"
if until_cap "$PM" "$C" '*bravo.go:3 *+*'; then ok "moved back to bravo, it reviews bravo"; else bad "moved back to bravo, it reviews bravo" "bravo.go:3 (+)" "$(pm_cap "$C")"; fi
case "$(pm_cap "$C" | grep -v '● pinned')" in *●*) bad "and the alpha comment does not follow it" "no ●" "$(pm_cap "$C")" ;; *) ok "and the alpha comment does not follow it" ;; esac
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
"$TMUX_REAL" -L "$PM" respawn-pane -k -t "$PMB" -c "$ALPHA" 'exec -a claude bash'
C=$(pm_shell); pm_review "$C" "$PMB"
if until_cap "$PM" "$C" '*● alpha.go:3 *'; then ok "back in alpha, its own alpha comment is resumed"; else bad "back in alpha, its own alpha comment is resumed" "● alpha.go:3 " "$(pm_cap "$C")"; fi
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
"$TMUX_REAL" -L "$PM" respawn-pane -k -t "$PMB" -c "$BRAVO" 'exec -a claude bash'
is "one agent, two roots: one more session dir, none shared" "3" "$(ls -d "$PMSTATE"/tmux-cockpit/review/* 2>/dev/null | wc -l)"

# (c) from the terminal window, through the fleet queue, back to A's review
# -p, like every other call here: the shipped binding is
# `tmux-review -p '#{pane_id}'` (cockpit.tmux:258), so prefix r ALWAYS names
# the pane it was pressed from. Bare, main_entry falls back to best-effort
# client recency, and this server has no attached client to be recent -- tmux
# then answers with the current window's active pane (the agent in t:0), not
# the pane the command runs in. That fallback is not what prefix r does.
C=$(pm_shell)
"$TMUX_REAL" -L "$PM" send-keys -t "$C" "XDG_STATE_HOME=$(printf '%q' "$PMSTATE") '$DIR/scripts/tmux-review' -p $C" Enter
if until_cap "$PM" "$C" '*fleet>*'; then ok "prefix r from the terminal window is the fleet queue"; else bad "prefix r from the terminal window is the fleet queue" "fleet>" "$(pm_cap "$C")"; fi
if until_cap "$PM" "$C" '*alpha⑂main*bravo⑂main*'; then ok "both projects' agents are listed"; else bad "both projects' agents are listed" "alpha⑂main and bravo⑂main" "$(pm_cap "$C")"; fi
"$TMUX_REAL" -L "$PM" send-keys -t "$C" "alpha"; until_cap "$PM" "$C" '*fleet> alpha*'
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Enter
if until_cap "$PM" "$C" '*● alpha.go:3 *'; then ok "picking A in the queue resumes A's review, marks and all"; else bad "picking A in the queue resumes A's review, marks and all" "● alpha.go:3 " "$(pm_cap "$C")"; fi
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Escape
until_cap "$PM" "$C" '*discard 1 comment*'
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Down Down Enter
for i in $(seq 1 40); do [ -e "$(pm_sess "$PMA" "$ALPHA")" ] || break; sleep 0.1; done
is "discarding A from the queue path removes only A's dir" "2" "$(ls -d "$PMSTATE"/tmux-cockpit/review/* 2>/dev/null | wc -l)"
unset -f pm_shell pm_review pm_sess pm_cap
"$TMUX_REAL" -L "$PM" kill-server >/dev/null 2>&1
fi

echo "== tmux-review: a review survives the client that opened it =="
# review_pane() used to keep the only copy of a review in a mktemp file
# whose name nothing recorded: a dropped SSH client tears down a
# client-owned popup and took the review with it. Now one review is one
# 0700 directory keyed on pane + repo root, so reopening the same pane
# finds its own comments; discarding removes the directory.
if need_tmux "review session dir"; then :; else
SD="cockpit-sd-$$"; SOCKS="$SOCKS $SD"
SDDIR=$(mktmpd); SDSTATE=$(mktmpd)
(
  cd "$SDDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
"$TMUX_REAL" -L "$SD" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$SDDIR" \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$SD" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
SDAP=$("$TMUX_REAL" -L "$SD" list-panes -t t: -F '#{pane_id}')
sd_shell() {                  # a control pane, ready to take a command
    local p i
    p=$("$TMUX_REAL" -L "$SD" split-window -t t -P -F '#{pane_id}' 'exec bash')
    for i in $(seq 1 40); do
        [ "$("$TMUX_REAL" -L "$SD" display-message -p -t "$p" '#{pane_current_command}')" = bash ] && break
        sleep 0.1
    done
    printf '%s' "$p"
}
SDCTL=$(sd_shell)
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL" \
    "XDG_STATE_HOME=$(printf '%q' "$SDSTATE") '$DIR/scripts/tmux-review' -p $SDAP" Enter
if review_enter "$SD" "$SDCTL"; then ok "the picker comes up for -p <agent pane>"
else bad "the picker comes up for -p <agent pane>" "review>" "$("$TMUX_REAL" -L "$SD" capture-pane -p -t "$SDCTL")"; fi
SDROOT=$(git -C "$SDDIR" rev-parse --show-toplevel)
SDSESS="$SDSTATE/tmux-cockpit/review/${SDAP#%}-$(printf '%s' "$SDROOT" | cksum | cut -d' ' -f1)"
sdmode() { stat -c '%a' "$1" 2>/dev/null || stat -f '%Lp' "$1" 2>/dev/null || echo none; }
is "the session dir is keyed on pane + root and born 0700" "700" "$(sdmode "$SDSESS")"
is "and its parents are private too" "700" "$(sdmode "$SDSTATE/tmux-cockpit")"
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL" "keep me"
until_cap "$SD" "$SDCTL" '*keep me*'
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL" Enter
if until_file "$SDSESS/comments.md" '*keep me*'; then ok "a pinned comment lands in <session>/comments.md"
else bad "a pinned comment lands in <session>/comments.md" "keep me" "$(cat "$SDSESS/comments.md" 2>/dev/null)"; fi
is "the store is 0600" "600" "$(sdmode "$SDSESS/comments.md")"
is "the snapshot sits beside it" "yes" "$([ -s "$SDSESS/comments.md.rows" ] && echo yes || echo no)"
# The client drops: the pane running the picker dies with it.
"$TMUX_REAL" -L "$SD" kill-pane -t "$SDCTL"
is "the review outlives the client" "1" "$(grep -c '^## src.go:3 (+)' "$SDSESS/comments.md" 2>/dev/null)"
SDCTL2=$(sd_shell)
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL2" \
    "XDG_STATE_HOME=$(printf '%q' "$SDSTATE") '$DIR/scripts/tmux-review' -p $SDAP" Enter
review_enter "$SD" "$SDCTL2"
if until_cap "$SD" "$SDCTL2" '*● src.go:3 *'; then ok "reopening the same pane resumes its comments, marked"
else bad "reopening the same pane resumes its comments, marked" "● src.go:3 " "$("$TMUX_REAL" -L "$SD" capture-pane -p -t "$SDCTL2")"; fi
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL2" Escape
until_cap "$SD" "$SDCTL2" '*discard 1 comment*'
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL2" Down Down Enter
for i in $(seq 1 40); do [ -e "$SDSESS" ] || break; sleep 0.1; done
is "discarding removes the session dir, not just the store" "gone" "$([ -e "$SDSESS" ] && echo present || echo gone)"
unset -f sd_shell sdmode
"$TMUX_REAL" -L "$SD" kill-server >/dev/null 2>&1
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
PAROUT=$(J=3 "$PARROOT/tests/par.sh" 2>&1); PARRC=$?
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

printf '  %d passed, %d failed, %d skipped\n' "$pass" "$fail" "$skipped"
[ "$fail" -eq 0 ] || exit 1
