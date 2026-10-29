#!/usr/bin/env bash
# tmux-cockpit -- ours, not a tpm plugin. Load with:
#
#   run-shell ~/.tmux/plugins/tmux-cockpit/cockpit.tmux
#
# Agent state is a per-pane tmux option (@panestate), written by Claude Code
# hooks via scripts/agent-state. The border colour, window dots and the
# cross-window alert are all plain tmux formats in ~/.tmux.conf, so a redraw
# costs no shell spawns. Only the ssh-aware path/branch still needs a script.
#
# Tokens it expands in status-left / status-right:
#   #{agent_where_path}   ~/path locally, ssh destination when remote
#   #{agent_where_branch} git branch locally, blank when remote
#   #{agent_fleet}        ▲N blocked, ✕N errored and ◆N with a pending review,
#                         counted across every session on the server; empty
#                         when all is quiet
#
# Keys it binds (see the README for the status-right marker the click needs):
#   MouseDown1Control1    branch picker, on whatever you marked range=control|1
#   prefix + b            the same picker, without the mouse
#   C-DoubleClick1Pane    path picker for the clicked line (also M-MouseDown1Pane
#                         where the terminal forwards Alt+click)
#   prefix + i            what the pane is, waits on and holds -- state, review,
#                         base, session id, review dir
set -u

# -P so DIR is resolved, and a hard exit if it fails: with `set -u` and no -e a
# failed cd left DIR empty and the plugin cheerfully installed bindings pointing
# at /scripts/... and exported a bogus TMUX_COCKPIT_DIR, all with status 0.
DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || exit 1

# tpm hides plugin output, so an old tmux only hears this message.
tmux_version=$(tmux -V 2>/dev/null | sed 's/^tmux //; s/[^0-9.].*$//')
vmaj=${tmux_version%%.*} vmin=${tmux_version#*.}
case "$vmin" in ''|*[!0-9]*) vmin=0 ;; esac
if [ -n "$vmaj" ] && { [ "$vmaj" -lt 3 ] || { [ "$vmaj" -eq 3 ] && [ "$vmin" -lt 8 ]; }; }; then
    tmux display-message "tmux-cockpit needs tmux 3.8 or newer (found $tmux_version); bootstrap.sh builds it"
    exit 0
fi

# Shell-quote a path for the command strings tmux hands to sh. SINGLE quotes,
# not printf %q: the bindings below embed their path inside a tmux
# double-quoted string, where tmux processes backslash escapes and would eat
# %q's before sh ever saw them. Single quotes mean nothing to tmux in there.
sq() { printf "'%s'" "${1//\'/\'\\\'\'}"; }   # close, escaped quote, reopen

interpolate() {
    local s="$1"
    # Build replacements in variables first. Inlining them into ${s//pat/rep}
    # breaks: the replacement text contains "}" (from #{pane_pid}), and bash
    # ends the parameter expansion at that first "}", appending the remainder
    # as literal garbage.
    # $DIR must be quoted here too: sq() fixed the key bindings but this pair
    # kept a bare path, so a plugin directory containing a space made both
    # status tokens run "sh: /home/my: not found" on every redraw -- rendering
    # nothing, reporting nothing.
    local w; w=$(sq "$DIR/scripts/tmux-where")
    # tmux-where only differs from tmux for an ssh pane, so every other pane
    # renders natively; a #() in the untaken #{?} branch never runs.
    local ssh='#{==:#{pane_current_command},ssh}'
    # The ssh job gets only #{pane_pid} and #{pane_id}, closed forms that can
    # never carry a shell metacharacter. The branch job needs the path: it goes
    # single-quoted with each ' made '\'', the one quoting a newline or tab in
    # a cwd cannot break out of (#{q:} leaves sh line continuations behind).
    local qp="'#{s|'|'\\\\''|:pane_current_path}'"
    # exec: dash forks before running the job's one command otherwise.
    local r_path="#{?$ssh,#(exec $w path #{pane_pid} #{pane_id}), #{s|^#{@cockpit_home_re}|~|:pane_current_path}}"
    local r_brch="#{?$ssh,,#(exec $w branch #{pane_pid} $qp -)}"

    # #{agent_fleet} now reads a precomputed option, not a live count. The inline
    # form was #{n:#{S:#{W:#{P:}}}} -- one x per blocked/errored agent pane across
    # every session -- and the format named each count TWICE (a ">0" test and the
    # number), so status-right ran up to four whole-server pane walks per redraw.
    # An agent emitting an OSC-2 title forces a status redraw on every keystroke,
    # which made this the freeze rig's second-worst offender (gate: 360 pane
    # visits per redraw with the fleet enabled). agent-state now recomputes the
    # ▲N ✕N string on every state transition, off the redraw path, and stores it
    # in @cockpit_fleet; #{E:} expands that here so the @thm_* colour tokens it
    # carries resolve at render time -- a theme sourced after the plugin still
    # wins, exactly as the inline walk did. Empty (nothing waiting) renders
    # nothing. The whole-server count still runs high on a session grouped with
    # new-session -t or a window shared by link-window (a format cannot
    # de-duplicate pane ids); documented in the README, and it only ever reads
    # high, which never masks a fleet that needs you.
    local r_fleet="#{E:@cockpit_fleet}"

    s="${s//'#{agent_where_path}'/$r_path}"
    s="${s//'#{agent_where_branch}'/"$r_brch"}"   # quoted: bash 5.2 eats its \\
    s="${s//'#{agent_fleet}'/$r_fleet}"
    printf '%s' "$s"
}

# Rewrite an option in place, only if it still holds an un-interpolated token.
update_option() {
    local opt="$1" val
    val=$(tmux show-option -gqv "$opt")
    [ -n "$val" ] || return 0
    case "$val" in
        *'#{agent_'*) tmux set-option -gq "$opt" "$(interpolate "$val")" ;;
    esac
}

# The commands whose panes count as an agent, as ONE option every format reads,
# rather than the same regex copied into two dozen of them. Only defaulted,
# never overwritten: a user who set their own keeps it.
#
# Leaving it unset is not an option -- tmux treats an empty pattern as matching
# EVERY command, so every pane in the session would be painted as an agent.
if [ -z "$(tmux show-option -gqv @cockpit_agents)" ]; then
    tmux set-option -gq @cockpit_agents '^(claude|codex|aider|cursor|opencode|gemini|crush)$'
fi

# The GLOB form of the agent list, which is what the formats actually match
# against. #{m/r:} recompiles its regex on every evaluation -- measured 32-36us
# per test, 25 tests per config, and the whole redraw dropped from 16.2ms to
# 7.7ms at six windows when switched to #{m:} glob matching (rendering verified
# byte-identical). The public option stays @cockpit_agents; a simple
# ^(a|b|c)$ regex is converted automatically, anything more exotic gets a
# message rather than being silently ignored. Set @cockpit_agents_glob
# directly ("|a|b|c|") to bypass the conversion.
#
# One semantic difference, documented: a pane_current_command containing a
# literal * would false-match under glob. No real process comm looks like that.
if [ -z "$(tmux show-option -gqv @cockpit_agents_glob)" ]; then
    agents=$(tmux show-option -gqv @cockpit_agents)
    case "$agents" in
        '^('*')$')
            inner=${agents#'^('}; inner=${inner%')$'}
            case "$inner" in
                *[^a-zA-Z0-9_\|-]*)
                    tmux display-message "tmux-cockpit: @cockpit_agents is not a plain ^(a|b|c)\$ list; set @cockpit_agents_glob yourself"
                    inner='claude|codex|aider|cursor|opencode|gemini|crush' ;;
            esac ;;
        *)  inner='claude|codex|aider|cursor|opencode|gemini|crush' ;;
    esac
    tmux set-option -gq @cockpit_agents_glob "|$inner|"
fi

# Exported so the Claude Code hooks can find agent-state without a hardcoded path.
tmux set-environment -g TMUX_COCKPIT_DIR "$DIR"
tmux set-environment -gu TMUX_PANESTATE_DIR 2>/dev/null   # the pre-rename name

# Every option that can carry a token, not only the two this theme happens to
# use. The README documents #{agent_where_path} as usable in status-left and
# status-right; those were never visited, so anyone following the README got the
# literal "#{agent_where_path}" printed in their status bar. update_option skips
# anything without a token, so listing an option costs nothing.
# $HOME as a regex for the native path's ~; an option, so no character in it
# can break the format around it.
tmux set-option -gq @cockpit_home_re "$(printf '%s' "${HOME:-/nonexistent}" | sed 's/[][\\.*^$+?(){}|]/\\&/g')"
for opt in status-left status-right \
           window-status-format window-status-current-format \
           @catppuccin_directory_text @catppuccin_gitmux_text; do
    update_option "$opt"
done

# --- keys ---------------------------------------------------------------------
# Bound here, not in ~/.tmux.conf, so installing the plugin is the whole install.
# Paths go through $DIR: the scripts work whether or not install.sh symlinked
# them into ~/.local/bin.

Q_BRANCH=$(sq "$DIR/scripts/tmux-branch")
Q_OPEN=$(sq "$DIR/scripts/tmux-open-path")
Q_REVIEW=$(sq "$DIR/scripts/tmux-review")
# The pane a popup came from: $TMUX ends in the session id, whose active pane
# is where the key fired. display-popup expands formats only in -T and -d; a
# '#{pane_id}' here was literal text and scripts fell back to $PWD (#38).
SRC_PANE='"$(tmux display-message -p -t "\$${TMUX##*,}" "#{pane_id}")"'

# The palette. These are catppuccin's variable names, and a catppuccin user
# already has them -- but the example config reads them unconditionally, and a
# user WITHOUT that theme got no colour at all: tmux rejects the resulting
# "fg=" outright, so the border never coloured and nothing said why. Verified:
# defaulted only when unset, so a theme's own values always win.
while read -r n v; do
    [ -n "$(tmux show-option -gqv "$n")" ] || tmux set-option -gq "$n" "$v"
done <<'PALETTE'
@thm_red #f38ba8
@thm_peach #fab387
@thm_yellow #f9e2af
@thm_green #a6e3a1
@thm_overlay_0 #6c7086
@thm_sky #89dceb
@thm_mauve #cba6f7
@cockpit_blocked_bg #52303a
PALETTE

# Both click gestures need the tmux mouse; without it they die silently, so
# say so once. prefix+b still works either way.
if [ "$(tmux show-option -gqv mouse)" != on ]; then
    tmux display-message "tmux-cockpit: the click gestures need 'set -g mouse on'; prefix+b still works"
fi

# Popup geometry, configurable the same way as @cockpit_agents: set your own
# BEFORE the run-shell line. Percentages or absolute columns/rows, anything
# display-popup accepts.
#   set -g @cockpit_popup_width  60%
#   set -g @cockpit_popup_height 50%
PW=$(tmux show-option -gqv @cockpit_popup_width);  PW=${PW:-70%}
PH=$(tmux show-option -gqv @cockpit_popup_height); PH=${PH:-70%}

# Branch picker. The click half needs the status bar to mark a range -- the
# plugin cannot inject into a status-right it does not own. See the README.
# Same geometry and title style as the path picker: the two popups are siblings
# and should read as one tool. No -b/-S -- the border belongs to the user's
# popup-border-style, and overriding it would fight their theme.
#
# -p $SRC_PANE: inside a popup $TMUX_PANE is the popup's own pseudo-pane, so
# the script cannot ask "what pane am I on" itself.
tmux bind-key -n MouseDown1Control1 display-popup -E -w "$PW" -h "$PH" -T ' switch branch · #{b:pane_current_path} ' "$Q_BRANCH -p $SRC_PANE"
# Clicking the DIRECTORY segment browses the pane's working dir -- the same
# popup the path picker descends into, entered from the top. Wired to
# range=control|2 in status-right, the slot beside the branch's control|1.
# -d is a format (the shell-command is not, see SRC_PANE): the popup starts in
# the pane's directory and --browse takes its $PWD.
tmux bind-key -n MouseDown1Control2 display-popup -E -w "$PW" -h "$PH" \
    -T ' files · #{b:pane_current_path} ' -d '#{pane_current_path}' \
    "$Q_OPEN --browse"
# tmux keeps its drag flag set when a popup or menu eats the release, then
# rewrites the NEXT press anywhere into MouseDragEnd1<where> (#21, 3.7c): a
# tab click was silently lost. Stock leaves this key unbound.
tmux bind-key -n MouseDragEnd1Status switch-client -t =
tmux bind-key b display-popup -E -w "$PW" -h "$PH" -T ' switch branch · #{b:pane_current_path} ' "$Q_BRANCH -p $SRC_PANE"

# Every key below is lowercase (#43): tmux binds the character the terminal
# sends, so a Shift-letter chord dies under Caps Lock or a non-Latin layout.
# Each block leaves its effective key in a lowercase variable (empty = off)
# for the agents menu further down, which replays them.
key() {                       # <option> <default> -> the key, empty when off
    local k; k=$(tmux show-option -gqv "$1")
    case "$k" in off) ;; "") printf '%s' "$2" ;; *) printf '%s' "$k" ;; esac
}

# Replaces stock choose-tree on s; tmux 3.8 took g for its floating-pane menu.
# TM_HEIGHT=100%: inside a popup, fzf's inline 40% leaves half of it empty.
Q_TM=$(sq "$DIR/scripts/tm")
sk=$(key @cockpit_sessions_key s)
[ -z "$sk" ] || tmux bind-key "$sk" display-popup -E -w "$PW" -h "$PH" -T ' sessions ' "TM_HEIGHT=100% $Q_TM"

# Jump to the pane that needs you; press again to cycle through all of them.
# u as in unblock; j is pane-down in the example config.
# run-shell expands #{pane_id} but sets no $TMUX_PANE, so the pane goes in
# as an argument: without it the cycle never knew where it stood (#45).
jk=$(key @cockpit_jump_key u)
[ -z "$jk" ] || tmux bind-key "$jk" run-shell "'$DIR/scripts/tmux-jump' '#{pane_id}'"

# Review an agent's diff. The popup passes -p $SRC_PANE so the script sees
# the pane the popup was opened FROM: inside a popup $TMUX_PANE is the popup's
# own pseudo-pane, and a bare display-message resolves the most-recently-active
# client instead -- with two clients that landed the review on the wrong one.
# The popup's own sh resolves the pane (SRC_PANE above). With the id the
# script opens the fleet queue with that pane preselected. Same popup chrome
# and off-disables as the jump key.
#
# @cockpit_review_window on: a real window instead of the modal popup, so the
# fleet's borders and tabs stay visible while you read (§8 #2: popup default,
# window opt-in, decided again after this phase). Same script, same -p.
# The window form cannot use SRC_PANE: new-window activates the new window
# before its command runs, so the session's active pane is the review
# window's own. run-shell expands #{pane_id} on the source pane instead (#45).
if [ "$(tmux show-option -gqv @cockpit_review_window)" = on ]; then
    R_OPEN=(run-shell)
    R_CMD="tmux new-window -n review $(sq "$Q_REVIEW -p '#{pane_id}'")"
else
    R_OPEN=(display-popup -E -w "$PW" -h "$PH" -T ' review · #{b:pane_current_path} ')
    R_CMD="$Q_REVIEW -p $SRC_PANE"
fi
rk=$(key @cockpit_review_key r)
[ -z "$rk" ] || tmux bind-key "$rk" "${R_OPEN[@]}" "$R_CMD"

# prefix i: what this pane is, waits on, and holds -- state, review, base,
# session id, review dir. Replaces stock i (window info) because it IS the
# window info you want on an agent pane; @cockpit_info_key rebinds, off keeps
# stock. `read` is POSIX, so the popup's sh can wait for ↵ itself.
ik=$(key @cockpit_info_key i)
[ -z "$ik" ] || tmux bind-key "$ik" display-popup -E -w "$PW" -h "$PH" -T ' info · #{pane_id} ' "$Q_REVIEW -p $SRC_PANE --info; read -r _"

# prefix f: a side fix beside a busy agent -- two lines, a fenced claude -p in
# its own pane, the agent's suite command held off and its next Stop held
# until one handover line. Stock f is find-window; @cockpit_fixer_key off keeps it.
Q_FIXER=$(sq "$DIR/scripts/tmux-fixer")
fk=$(key @cockpit_fixer_key f)
[ -z "$fk" ] || tmux bind-key "$fk" display-popup -E -w "$PW" -h 6 -T ' side fix · #{pane_id} ' "$Q_FIXER -p $SRC_PANE"

# prefix w: the board -- every agent pane as a live tile. Stock w is
# choose-tree; @cockpit_board_key rebinds, off keeps stock.
#
# Wider than the pickers' $PW and only wider: a 70% popup on a 120-column
# terminal is an 84-column board, which is one tile column.
Q_BOARD=$(sq "$DIR/scripts/cockpit-board")
bk=$(key @cockpit_board_key w)
# Bound whether or not bin/cockpit-tui exists: the binary arrives by a gesture,
# so the wrapper says "not installed" at press time rather than the key
# appearing only after the next reload.
if [ -n "$bk" ]; then
    # @cockpit_board_window on: a parked window instead of the modal popup,
    # mirroring @cockpit_review_window. new-window activates itself before its
    # command runs, so SRC_PANE would resolve to the board's own pane; run-shell
    # expands #{pane_id} on the source pane instead (#45).
    if [ "$(tmux show-option -gqv @cockpit_board_window)" = on ]; then
        tmux bind-key "$bk" run-shell "tmux new-window -n board $(sq "$Q_BOARD -p '#{pane_id}'")"
    else
        tmux bind-key "$bk" display-popup -E -w 90% -h "$PH" -T ' board · #{session_name} ' "$Q_BOARD -p $SRC_PANE"
    fi
fi

# The binary is optional, and nothing here downloads it. tpm's `prefix U`
# re-runs every *.tmux -- but so does every plain `source-file ~/.tmux.conf`,
# and from in here the two are indistinguishable, so an unconditional fetch
# would put curl's timeout on the tmux startup path. Opt in with
# `set -g @cockpit_tui_fetch on`, or run scripts/tui-fetch by hand.
if [ "$(tmux show-option -gqv @cockpit_tui_fetch)" = on ]; then
    tmux run-shell -b "$(sq "$DIR/scripts/tui-fetch")"
fi

# Reload on R, the shifted pair of the review key: you review all day and
# reload only after editing the config, so the lowercase key goes to review.
# The paths come from what tmux actually loaded, resolved once at bind time:
# a hardcoded ~/.tmux.conf misses XDG setups (~/.config/tmux/tmux.conf), and
# a keypress must not spawn a shell to rediscover a path that cannot change
# for the life of the server.
#
# #{config_files} is EVERY file this run loaded, comma-separated, and on a
# plain (non -f) start it is multi-entry: /etc/tmux.conf first (usually
# absent), then the user's file(s). `cut -d, -f1` picked only that first,
# almost-always-missing entry -- reload silently no-opped on a default
# install. Split on every comma and source each one.
#
# The display-message confirmation has to be bound as PART of the same
# command, and a keypress running `cmd1 \; cmd2` as separate shell/tmux-CLI
# argv words does not do that: unescaped from bind-key's own top-level
# parsing, `;` ends the bind-key command right there, so cmd2 becomes a
# second, independent command that runs once, immediately, at plugin-load
# time -- never on a keypress (this bit the mouse bindings below too; see
# that comment for the reproduction). tmux's `{ ... }` command-grouping
# syntax IS unambiguous, but only when it comes from real config-file text,
# not scattered shell argv -- so build the binding as a small conf file and
# source it, the same technique the mouse bindings below use.
rlconf=$(mktemp) || exit 1
trap 'rm -f "$rlconf"' EXIT HUP INT TERM
{
    old_ifs=$IFS
    IFS=,
    set -f
    # shellcheck disable=SC2046 # splitting #{config_files} on comma is the point
    set -- $(tmux display-message -p '#{config_files}' 2>/dev/null)
    set +f
    IFS=$old_ifs
    [ "$#" -gt 0 ] || set -- "${HOME:-}/.tmux.conf"
    rlk=$(key @cockpit_reload_key R)
    [ -z "$rlk" ] || printf 'bind-key %s {\n' "$(sq "$rlk")"
    if [ -n "$rlk" ]; then
        for f in "$@"; do
            # -q: #{config_files} lists every path tmux tried, not every path
            # that existed (/etc/tmux.conf is almost always in the list and
            # almost never on disk). Without -q, that one missing file's
            # error aborts the whole chained command -- the user's own file
            # right after it would never get sourced either.
            printf '    source-file -q %s\n' "$(sq "$f")"
        done
        printf '    display-message "config reloaded"\n'
        printf '}\n'
    fi
} > "$rlconf"
tmux source-file "$rlconf"
rm -f "$rlconf"

# prefix M: move this window to another session, or a new one, from a menu.
# Stock M is select-pane -M (unmark), the partner of the m that full.tmux.conf
# already takes; @cockpit_move_key rebinds, off keeps stock.
#
# Order is the whole trick: switch-client FIRST, then move-window, then
# select-window. Moving the last window out of the session the client sits in
# destroys that session, and with detach-on-destroy (the default) that throws
# the client out of tmux -- verified, and not repairable after the fact. Once
# the client has left, the source window can only be named by id, so it is
# pinned in a global option before the menu opens (-F: without it the option
# stores the literal format and the menu wedges the server). #{window_id}
# inside #{S:} shadows to the looped session's window, hence the pin.
#
# Every piece of user text stays data. ##{window_name} in the title is left
# for display-menu to expand, so a window named it's cannot tear the command
# apart; sessions are labelled with #{qa:} and targeted by id. The typed
# new-session name goes through %%% (quotes escaped) ONCE, into an option,
# and everything after reads the option: an option value expands as text, so
# #( in the name never runs, and no =name: target exists for a : or . in the
# name to break. Four # because the string is expanded twice before if-shell
# sees it (run-shell -C, then display-menu). new-session without -d switches
# the client itself, so -t :^ is the new session, and -k makes the moved
# window REPLACE its placeholder shell. The if-shell guard is for an empty
# name: 3.7 makes a session with no name instead of refusing (a name of
# exactly 0 is refused too; tmux numbers sessions from 0, so it is taken).
mk=$(key @cockpit_move_key M)
if [ -n "$mk" ]; then
    mvconf=$(mktemp) || exit 1
    trap 'rm -f "$mvconf"' EXIT HUP INT TERM
    {
        printf 'bind-key %s {\n' "$(sq "$mk")"
        cat <<'EOF'
    set -gF @cockpit_move_src "#{window_id}"
    set -gF @cockpit_move_cwd "#{pane_current_path}"
    run-shell -C "display-menu -T '#[align=centre] move ##{window_index}:##{window_name} to ' -x W -y W #{S/n:#{qa:session_name} '' 'switch-client -t #{session_id} ; move-window -s #{@cockpit_move_src} -t #{session_id}: ; select-window -t #{@cockpit_move_src}' ,} '' 'new session...' n \"command-prompt -p 'new session:' 'set -g @cockpit_move_dst \\\"%%%\\\" ; if-shell -F \\\"####{@cockpit_move_dst}\\\" { new-session -s \\\"####{@cockpit_move_dst}\\\" -c \\\"####{@cockpit_move_cwd}\\\" ; move-window -k -s #{@cockpit_move_src} -t :^ }'\""
}
EOF
    } > "$mvconf"
    tmux source-file "$mvconf"
    rm -f "$mvconf"
fi

# prefix e: hide the status line and pane borders for THIS session and window.
# The flag IS the state, so a config that already hides the status line is not
# read as a restore; unsetting inherits whatever the config says, no stash.
fsk=$(key @cockpit_fullscreen_key e)
if [ -n "$fsk" ]; then
    fsconf=$(mktemp) || exit 1
    trap 'rm -f "$fsconf"' EXIT HUP INT TERM
    {
        printf 'bind-key %s {\n' "$(sq "$fsk")"
        cat <<'EOF'
    if -F "#{@cockpit_fs}" {
        set -u @cockpit_fs
        set -u status
        set -uw pane-border-status
    } {
        set @cockpit_fs 1
        set status off
        set -w pane-border-status off
    }
}
EOF
    } > "$fsconf"
    tmux source-file "$fsconf"
    rm -f "$fsconf"
fi

# prefix a: the agents menu, every cockpit action as a row with its letter on
# screen. A row replays the key the action is bound to (send-keys -K): one
# definition per action, a rebound key moves its row, an off key drops it.
# Stock a is unbound; @cockpit_ai_key rebinds (n if you never use
# next-window), off disables.
ak=$(key @cockpit_ai_key a)
if [ -n "$ak" ]; then
    rows=()
    row() { [ -z "$3" ] || rows+=("$1" "$2" "switch-client -T prefix ; send-keys -K $3"); }
    row 'Jump to the pane that needs you' u "$jk"
    row 'Review its diff'                 r "$rk"
    row 'Side fix beside it'              f "$fk"
    row 'Pane info'                       i "$ik"
    row 'Board of every agent'            w "$bk"
    row 'Full screen (hide the chrome)'   e "$fsk"
    rows+=('')
    row 'Switch branch'                   b b
    row 'Move window to a session...'     m "$mk"
    row 'Sessions...'                     s "$sk"
    tmux bind-key "$ak" display-menu -T ' agents ' -x C -y C "${rows[@]}"
fi

# Click a line of output to pick a path off it. -F is load-bearing: without it
# set-option stores the literal "#{mouse_line}". The line travels in a pane
# option rather than on the command line because pane output is full of quotes,
# and those would tear the shell string apart.
# The three commands go in ONE quoted string. Passed as separate argv words,
# "cmd \; cmd" does not separate: tmux keeps only the first command in the
# binding and runs the rest once, at load time.
#
# TWO triggers, because no single one works everywhere:
#
#   Ctrl+double-click  the portable choice, and the only one that works under
#                      Ghostty. Measured: Ghostty forwards plain, Ctrl, double
#                      and Ctrl+double clicks to tmux, but strips Alt entirely
#                      and keeps Shift for its own selection -- so an Alt-only
#                      binding can never fire there. It is also the one of those
#                      that tmux does not already own: plain Ctrl+click is
#                      swap-pane, plain double-click is select-word-and-copy,
#                      and taking either would cost a feature people use.
#                      It pairs with the branch picker, which is Ctrl+click on
#                      the status bar.
#   Alt+click          kept for terminals that DO send it (xterm, kitty,
#                      WezTerm, foot), where it is the lighter gesture. Binding
#                      both costs nothing: whichever one your terminal delivers
#                      is the one that fires.
# NO mouse-mode guard on these two, and that is deliberate -- it was tried and
# it broke the feature where it matters most. Claude Code (and most agent TUIs)
# keep mouse reporting on, so a guard that yields whenever the application
# wants the mouse forwarded the gesture into the agent and the picker never
# fired in exactly the panes this plugin exists for. Verified live:
# mouse_any_flag=1 on every claude pane.
#
# The division that holds up instead: every PLAIN gesture -- clicks, wheel,
# drag, double-click -- is untouched and reaches the application as before; we
# never bind those. The two CHORDED gestures below are someone deliberately
# invoking the picker, and the picker wins. (The cost is alt-click inside vim,
# which plain M-MouseDown1Pane already cost before any guard existed.)
#
# Copy-mode is also deliberately not excluded: #{mouse_line} reads the
# scrolled-back content there (verified: scrolled 15 up, got the line 15 back),
# so you can scroll into old output and pick a path off it.
conf=$(mktemp) || exit 1
# The plain `rm -f` after source-file covers every normal exit -- there is no
# `set -e` here, so a failed source-file still reaches it. It does NOT cover a
# signal: kill the tmux server while it is loading the plugin and the file is
# stranded. Measured: 3 of 40 SIGTERM races left one behind.
trap 'rm -f "$conf"' EXIT HUP INT TERM
{
    # tmux's own C-MouseDown1Pane is swap-pane, and it fires on the FIRST press
    # of a Ctrl+double-click. Swapping moves the second click onto a different
    # pane, so tmux never accumulates the double-click and C-DoubleClick1Pane is
    # never emitted. Measured on a clean 3.7 server: fires with one pane, never
    # with two or three. One pane is why this looked fine by hand.
    #
    # Ctrl+click therefore focuses the pane instead of swapping it. The two
    # cannot coexist -- the conflict is the first click of the double. Set
    # @cockpit_keep_swap_pane to keep tmux's default and lose the picker.
    if [ -z "$(tmux show-option -gqv @cockpit_keep_swap_pane)" ]; then
        printf 'bind-key -n C-MouseDown1Pane select-pane -t=\n'
        printf 'bind-key -n C-DoubleClick1Pane {\n'
        printf '    select-pane -t=\n'
        printf '    set -pF -t= @cockpit_click_line "#{mouse_line}"\n'
        printf '    run-shell -b "%s #{pane_id}"\n' "$Q_OPEN"
        printf '}\n'
    fi
    # Ctrl+RIGHT-click: the gesture no terminal link-handler claims. Claude
    # Code wraps its paths in OSC 8 hyperlinks and Ghostty opens those on the
    # first Ctrl+LEFT-click's release -- before forwarding the event -- so a
    # Ctrl+double-click there opens the picker AND a GUI via xdg-open. Link
    # opening is a left-button feature; the right button reaches only us.
    # C-MouseDown3Pane is unbound in stock tmux, so nothing is displaced.
    printf 'bind-key -n C-MouseDown3Pane {\n'
    printf '    select-pane -t=\n'
    printf '    set -pF -t= @cockpit_click_line "#{mouse_line}"\n'
    printf '    run-shell -b "%s #{pane_id}"\n' "$Q_OPEN"
    printf '}\n'
    # Kept for terminals that do forward Alt+click (xterm, kitty, WezTerm,
    # foot). Ghostty strips it, which is why Ctrl+double-click exists above.
    printf 'bind-key -n M-MouseDown1Pane {\n'
    printf '    select-pane -t=\n'
    printf '    set -pF -t= @cockpit_click_line "#{mouse_line}"\n'
    printf '    run-shell -b "%s #{pane_id}"\n' "$Q_OPEN"
    printf '}\n'
} > "$conf"
tmux source-file "$conf"
rm -f "$conf"

# Last, so it is the message left in the client's one slot rather than one an
# earlier warning overwrites. Once per server: Debian 13 and Ubuntu 24.04 both
# ship 0.44.1, and a nag on every config reload is what makes people stop
# reading them. `command -v` first because fzf is optional -- every picker
# falls back to a numbered menu, and an fzf-less user has nothing to upgrade.
# shellcheck source=scripts/lib/vnum.sh
if command -v fzf >/dev/null 2>&1 && [ -z "$(tmux show-option -gqv @cockpit_fzf_notice)" ] &&
        . "$DIR/scripts/lib/vnum.sh" 2>/dev/null; then
    # Below 0.63 the keys stay bound: only the pickers need it, nothing else does.
    fzf_v=$(fzf --version 2>/dev/null); fzf_v=${fzf_v%% *}
    tmux set-option -gq @cockpit_fzf_notice "${fzf_v:-?}"
    if [ "$(vnum "$fzf_v")" -lt "$(vnum 0.63.0)" ]; then
        tmux display-message "fzf $fzf_v found; tmux-cockpit's pickers need 0.63+, bootstrap.sh fetches it"
    fi
fi
