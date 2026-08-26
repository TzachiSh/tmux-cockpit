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
    # Only #{pane_pid} and #{pane_id} go into this command string -- both are
    # closed forms (digits, or a % plus digits) that can never carry a shell
    # metacharacter, so tmux-where can fetch pane_current_path/_command itself
    # (from /proc via pane_pid; one tmux call as the fallback) as DATA, never
    # as text sh re-parses.
    # #{q:} looked like the fix (man tmux: "q: will escape sh(1) special
    # characters") and was tried first, but it escapes quotes/$()/backticks
    # and passes NEWLINE and TAB straight through -- a command separator and a
    # word separator to sh -- so a cwd built on either still ran arbitrary
    # commands on every status redraw with #{q:} in place. Verified: #{q:@x}
    # on "a\nb\tc" comes back "a\nb\tc" (0a and 09 both raw, no backslash).
    local r_path="#($w path #{pane_pid} #{pane_id})"
    local r_brch="#($w branch #{pane_pid} #{pane_id})"

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
    s="${s//'#{agent_where_branch}'/$r_brch}"
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

# Version gate. Measured against real 3.4 and 3.7c binaries, not guessed:
#
#   3.4  per-pane pane-border-style with a format WORKS, #{m/r:} with an option
#        as the pattern works, display-popup -T works. The README used to claim
#        3.5 was the floor because the border "silently does nothing" on 3.4 --
#        it does not; that claim was wrong.
#   3.3  display-popup -T (the popup titles) first appears here. This is the
#        real floor.
#   3.7  MouseDown1Control1 and #[range=control|N] first appear. Below this,
#        binding it fails with "unknown key" and the range marker is ignored,
#        so Ctrl+clicking the branch segment cannot work. Everything else does.
#
# The message matters because tpm discards plugin stdout AND stderr, so a bare
# "unknown key" line would vanish and the user would just have a dead segment.
tmux_version=$(tmux -V 2>/dev/null | sed 's/^tmux //; s/[^0-9.].*$//')
version_at_least() {           # version_at_least <major> <minor>
    local maj=${tmux_version%%.*} min=${tmux_version#*.}
    [ -n "$maj" ] || return 0                     # unparseable: assume modern
    case "$min" in ''|*[!0-9]*) min=0 ;; esac
    [ "$maj" -gt "$1" ] || { [ "$maj" -eq "$1" ] && [ "$min" -ge "$2" ]; }
}

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

if ! version_at_least 3 3; then
    tmux display-message "tmux-cockpit needs tmux 3.3 or newer (found ${tmux_version:-unknown})"
fi

# Branch picker. The click half needs the status bar to mark a range -- the
# plugin cannot inject into a status-right it does not own. See the README.
# Same geometry and title style as the path picker: the two popups are siblings
# and should read as one tool. No -b/-S -- the border belongs to the user's
# popup-border-style, and overriding it would fight their theme.
#
# -p '#{pane_id}': inside a popup, $TMUX_PANE is the popup's own pseudo-pane,
# so tmux-branch asking the client "what pane am I on" resolves to whichever
# CLIENT was most recently active, not the pane this binding actually fired
# on. tmux expands #{pane_id} at bind time, on the pane the key/click landed
# on, the same way #{pane_current_path} already does for the title above.
if version_at_least 3 7; then
    tmux bind-key -n MouseDown1Control1 display-popup -E -w "$PW" -h "$PH" -T ' switch branch · #{b:pane_current_path} ' "$Q_BRANCH -p '#{pane_id}'"
    # Clicking the DIRECTORY segment browses the pane's working dir -- the same
    # popup the path picker descends into, entered from the top. Wired to
    # range=control|2 in status-right, the slot beside the branch's control|1.
    tmux bind-key -n MouseDown1Control2 display-popup -E -w "$PW" -h "$PH" \
        -T ' files · #{b:pane_current_path} ' \
        "$Q_OPEN --browse '#{pane_current_path}'"
elif tmux show-option -gqv status-right | grep -q 'range=control|'; then
    # Their config marks a click range this tmux cannot deliver. Say so, or the
    # segment is simply dead with no explanation.
    tmux display-message "tmux-cockpit: clicking the branch needs tmux 3.7 (found ${tmux_version:-unknown}); prefix+b still works"
fi
tmux bind-key b display-popup -E -w "$PW" -h "$PH" -T ' switch branch · #{b:pane_current_path} ' "$Q_BRANCH -p '#{pane_id}'"

# Session picker in the same popup chrome. prefix+S is unbound in stock tmux
# (lowercase s is choose-tree); TM_HEIGHT=100% because inside a popup the popup
# is the frame and fzf's inline 40%% would leave the bottom half empty.
Q_TM=$(sq "$DIR/scripts/tm")
tmux bind-key S display-popup -E -w "$PW" -h "$PH" -T ' sessions ' "TM_HEIGHT=100% $Q_TM"

# Jump to the pane that needs you; press again to cycle through all of them.
JK=$(tmux show-option -gqv @cockpit_jump_key)
case "$JK" in
    off) ;;                          # explicitly disabled
    "")  tmux bind-key J run-shell "'$DIR/scripts/tmux-jump'" ;;
    *)   tmux bind-key "$JK" run-shell "'$DIR/scripts/tmux-jump'" ;;
esac

# Review an agent's diff. The popup passes -p '#{pane_id}' so the script sees
# the pane the popup was opened FROM: inside a popup $TMUX_PANE is the popup's
# own pseudo-pane, and a bare display-message resolves the most-recently-active
# client instead -- with two clients that landed the review on the wrong one.
# tmux expands #{pane_id} at bind time in the originating pane. With the id the
# script opens the fleet queue with that pane preselected. Same popup chrome
# and off-disables as the jump key.
#
# @cockpit_review_window on: a real window instead of the modal popup, so the
# fleet's borders and tabs stay visible while you read (§8 #2: popup default,
# window opt-in, decided again after this phase). Same script, same -p.
if [ "$(tmux show-option -gqv @cockpit_review_window)" = on ]; then
    R_OPEN=(new-window -n review)
else
    R_OPEN=(display-popup -E -w "$PW" -h "$PH" -T ' review · #{b:pane_current_path} ')
fi
RK=$(tmux show-option -gqv @cockpit_review_key)
case "$RK" in
    off) ;;                          # explicitly disabled
    "")  tmux bind-key r "${R_OPEN[@]}" "$Q_REVIEW -p '#{pane_id}'" ;;
    *)   tmux bind-key "$RK" "${R_OPEN[@]}" "$Q_REVIEW -p '#{pane_id}'" ;;
esac

# prefix i: what this pane is, waits on, and holds -- state, review, base,
# session id, review dir. Replaces stock i (window info) because it IS the
# window info you want on an agent pane; @cockpit_info_key rebinds, off keeps
# stock. `read` is POSIX, so the popup's sh can wait for ↵ itself.
IK=$(tmux show-option -gqv @cockpit_info_key)
case "$IK" in
    off) ;;
    "")  tmux bind-key i display-popup -E -w "$PW" -h "$PH" -T ' info · #{pane_id} ' "$Q_REVIEW -p '#{pane_id}' --info; read -r _" ;;
    *)   tmux bind-key "$IK" display-popup -E -w "$PW" -h "$PH" -T ' info · #{pane_id} ' "$Q_REVIEW -p '#{pane_id}' --info; read -r _" ;;
esac

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
    RLK=$(tmux show-option -gqv @cockpit_reload_key)
    case "$RLK" in
        off) ;;                      # explicitly disabled
        "")  printf 'bind-key R {\n' ;;
        *)   printf 'bind-key %s {\n' "$RLK" ;;
    esac
    if [ "$RLK" != off ]; then
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
MK=$(tmux show-option -gqv @cockpit_move_key)
case "$MK" in
    off) ;;                          # explicitly disabled
    "")  mk=M ;;
    *)   mk=$MK ;;
esac
if [ "$MK" != off ]; then
    mvconf=$(mktemp) || exit 1
    trap 'rm -f "$mvconf"' EXIT HUP INT TERM
    {
        printf 'bind-key %s {\n' "$(sq "$mk")"
        cat <<'EOF'
    set -gF @cockpit_move_src "#{window_id}"
    run-shell -C "display-menu -T '#[align=centre] move ##{window_index}:##{window_name} to ' -x W -y W #{S/n:#{qa:session_name} '' 'switch-client -t #{session_id} ; move-window -s #{@cockpit_move_src} -t #{session_id}: ; select-window -t #{@cockpit_move_src}' ,} '' 'new session...' n \"command-prompt -p 'new session:' 'set -g @cockpit_move_dst \\\"%%%\\\" ; if-shell -F \\\"####{@cockpit_move_dst}\\\" { new-session -s \\\"####{@cockpit_move_dst}\\\" ; move-window -k -s #{@cockpit_move_src} -t :^ }'\""
}
EOF
    } > "$mvconf"
    tmux source-file "$mvconf"
    rm -f "$mvconf"
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
