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
#
# Keys it binds (see the README for the status-right marker the click needs):
#   MouseDown1Control1    branch picker, on whatever you marked range=control|1
#   prefix + b            the same picker, without the mouse
#   C-DoubleClick1Pane    path picker for the clicked line (also M-MouseDown1Pane
#                         where the terminal forwards Alt+click)
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
    local r_path="#($w path #{pane_pid} '#{pane_current_path}' '#{pane_current_command}')"
    local r_brch="#($w branch #{pane_pid} '#{pane_current_path}' '#{pane_current_command}')"
    s="${s//'#{agent_where_path}'/$r_path}"
    s="${s//'#{agent_where_branch}'/$r_brch}"
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

if ! version_at_least 3 3; then
    tmux display-message "tmux-cockpit needs tmux 3.3 or newer (found ${tmux_version:-unknown})"
fi

# Branch picker. The click half needs the status bar to mark a range -- the
# plugin cannot inject into a status-right it does not own. See the README.
# Same geometry and title style as the path picker: the two popups are siblings
# and should read as one tool. No -b/-S -- the border belongs to the user's
# popup-border-style, and overriding it would fight their theme.
if version_at_least 3 7; then
    tmux bind-key -n MouseDown1Control1 display-popup -E -w 80% -h 75% -T ' switch branch ' "$Q_BRANCH"
elif tmux show-option -gqv status-right | grep -q 'range=control|'; then
    # Their config marks a click range this tmux cannot deliver. Say so, or the
    # segment is simply dead with no explanation.
    tmux display-message "tmux-cockpit: clicking the branch needs tmux 3.7 (found ${tmux_version:-unknown}); prefix+b still works"
fi
tmux bind-key b                     display-popup -E -w 80% -h 75% -T ' switch branch ' "$Q_BRANCH"

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
GUARD='#{||:#{pane_in_mode},#{mouse_any_flag}}'
conf=$(mktemp) || exit 1
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
