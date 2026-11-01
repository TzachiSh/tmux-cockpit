#!/usr/bin/env bash
# tmux-cockpit. Load with:  run-shell ~/.tmux/plugins/tmux-cockpit/cockpit.tmux
# Expands #{agent_where_path}, #{agent_where_branch} and #{agent_fleet} in the
# status options, defaults the cockpit options, and binds every cockpit key.
# Keys and options are documented in KEYS.md and the README.
set -u

# A failed cd must stop here, not bind every key to /scripts/...
DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || exit 1

# tpm hides plugin output, so an old tmux only hears this message.
tmux_version=$(tmux -V 2>/dev/null | sed 's/^tmux //; s/[^0-9.].*$//')
vmaj=${tmux_version%%.*} vmin=${tmux_version#*.}
case "$vmin" in ''|*[!0-9]*) vmin=0 ;; esac
if [ -n "$vmaj" ] && { [ "$vmaj" -lt 3 ] || { [ "$vmaj" -eq 3 ] && [ "$vmin" -lt 8 ]; }; }; then
    tmux display-message "tmux-cockpit needs tmux 3.8 or newer (found $tmux_version); bootstrap.sh builds it"
    exit 0
fi

# Single quotes, not printf %q: inside a tmux double-quoted string tmux eats
# %q's backslashes before sh sees them.
sq() { printf "'%s'" "${1//\'/\'\\\'\'}"; }   # close, escaped quote, reopen

interpolate() {
    local s="$1"
    # Replacements live in variables: inlined, their "}" would end ${s//pat/rep}.
    local w; w=$(sq "$DIR/scripts/tmux-where")
    # Only ssh panes need the script; a #() in the untaken #{?} branch never runs.
    local ssh='#{==:#{pane_current_command},ssh}'
    # #() injection guard: the path goes single-quoted with ' made '\'', which a
    # newline or tab in a cwd cannot break out of (#{q:} can).
    local qp="'#{s|'|'\\\\''|:pane_current_path}'"
    # exec: dash forks before running the job's one command otherwise.
    local r_path="#{?$ssh,#(exec $w path #{pane_pid} #{pane_id}), #{s|^#{@cockpit_home_re}|~|:pane_current_path}}"
    local r_brch="#{?$ssh,,#(exec $w branch #{pane_pid} $qp -)}"

    # agent-state precomputes the fleet string; #{E:} resolves its colour tokens
    # at render time, so a theme sourced after the plugin still wins.
    local r_fleet="#{E:@cockpit_fleet}"

    s="${s//'#{agent_where_path}'/$r_path}"
    s="${s//'#{agent_where_branch}'/"$r_brch"}"   # quoted: bash 5.2 eats its \\
    s="${s//'#{agent_fleet}'/$r_fleet}"
    printf '%s' "$s"
}

update_option() {
    local opt="$1" val
    val=$(tmux show-option -gqv "$opt")
    [ -n "$val" ] || return 0
    case "$val" in
        *'#{agent_'*) tmux set-option -gq "$opt" "$(interpolate "$val")" ;;
    esac
}

# Never left empty: tmux treats an empty pattern as matching every command.
if [ -z "$(tmux show-option -gqv @cockpit_agents)" ]; then
    tmux set-option -gq @cockpit_agents '^(claude|codex|aider|cursor|opencode|gemini|crush)$'
fi

# The formats match a glob ("|a|b|c|"), since #{m/r:} recompiles its regex on
# every evaluation. Only a plain ^(a|b|c)$ converts; set the glob yourself otherwise.
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

# The Claude Code hooks find agent-state through this, not a hardcoded path.
tmux set-environment -g TMUX_COCKPIT_DIR "$DIR"
tmux set-environment -gu TMUX_PANESTATE_DIR 2>/dev/null

# $HOME as a regex, held in an option so no character in it can break the format.
tmux set-option -gq @cockpit_home_re "$(printf '%s' "${HOME:-/nonexistent}" | sed 's/[][\\.*^$+?(){}|]/\\&/g')"
for opt in status-left status-right \
           window-status-format window-status-current-format \
           @catppuccin_directory_text @catppuccin_gitmux_text; do
    update_option "$opt"
done

# --- keys ---------------------------------------------------------------------
Q_BRANCH=$(sq "$DIR/scripts/tmux-branch")
Q_OPEN=$(sq "$DIR/scripts/tmux-open-path")
Q_REVIEW=$(sq "$DIR/scripts/tmux-review")
# The pane a popup came from, via the session id $TMUX ends in: display-popup
# expands formats only in -T and -d, and inside it $TMUX_PANE is the popup's (#38).
SRC_PANE='"$(tmux display-message -p -t "\$${TMUX##*,}" "#{pane_id}")"'

# catppuccin's names, defaulted only when unset: without them tmux rejects "fg=".
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

if [ "$(tmux show-option -gqv mouse)" != on ]; then
    tmux display-message "tmux-cockpit: the click gestures need 'set -g mouse on'; prefix+b still works"
fi

PW=$(tmux show-option -gqv @cockpit_popup_width);  PW=${PW:-70%}
PH=$(tmux show-option -gqv @cockpit_popup_height); PH=${PH:-70%}

# The click needs a range=control|1 marker in the user's status-right (README).
tmux bind-key -n MouseDown1Control1 display-popup -E -w "$PW" -h "$PH" -T ' switch branch · #{b:pane_current_path} ' "$Q_BRANCH -p $SRC_PANE"
# The directory segment (range=control|2) browses the pane's cwd; -d is a format
# and the shell-command is not, so --browse takes its $PWD.
tmux bind-key -n MouseDown1Control2 display-popup -E -w "$PW" -h "$PH" \
    -T ' files · #{b:pane_current_path} ' -d '#{pane_current_path}' \
    "$Q_OPEN --browse"
# tmux keeps its drag flag when a popup eats the release and turns the next press
# into MouseDragEnd1<where>, losing a tab click (#21). Stock leaves this unbound.
tmux bind-key -n MouseDragEnd1Status switch-client -t =
tmux bind-key b display-popup -E -w "$PW" -h "$PH" -T ' switch branch · #{b:pane_current_path} ' "$Q_BRANCH -p $SRC_PANE"

# Lowercase keys: a Shift-letter chord dies under Caps Lock or a non-Latin layout.
# Each effective key stays in a variable (empty = off) for the agents menu.
key() {                       # <option> <default> -> the key, empty when off
    local k; k=$(tmux show-option -gqv "$1")
    case "$k" in off) ;; "") printf '%s' "$2" ;; *) printf '%s' "$k" ;; esac
}

# Replaces stock choose-tree on s; tmux 3.8 took g for its floating-pane menu.
# TM_HEIGHT=100%: inside a popup, fzf's inline 40% leaves half of it empty.
Q_TM=$(sq "$DIR/scripts/tm")
sk=$(key @cockpit_sessions_key s)
[ -z "$sk" ] || tmux bind-key "$sk" display-popup -E -w "$PW" -h "$PH" -T ' sessions ' "TM_HEIGHT=100% $Q_TM"

# run-shell sets no $TMUX_PANE, so the pane goes in as an argument (#45).
jk=$(key @cockpit_jump_key u)
[ -z "$jk" ] || tmux bind-key "$jk" run-shell "'$DIR/scripts/tmux-jump' '#{pane_id}'"

# The window form cannot use SRC_PANE: new-window activates itself before its
# command runs. run-shell expands #{pane_id} on the source pane instead (#45).
if [ "$(tmux show-option -gqv @cockpit_review_window)" = on ]; then
    R_OPEN=(run-shell)
    R_CMD="tmux new-window -n review $(sq "$Q_REVIEW -p '#{pane_id}'")"
else
    R_OPEN=(display-popup -E -w "$PW" -h "$PH" -T ' review · #{b:pane_current_path} ')
    R_CMD="$Q_REVIEW -p $SRC_PANE"
fi
rk=$(key @cockpit_review_key r)
[ -z "$rk" ] || tmux bind-key "$rk" "${R_OPEN[@]}" "$R_CMD"

ik=$(key @cockpit_info_key i)
[ -z "$ik" ] || tmux bind-key "$ik" display-popup -E -w "$PW" -h "$PH" -T ' info · #{pane_id} ' "$Q_REVIEW -p $SRC_PANE --info; read -r _"

Q_FIXER=$(sq "$DIR/scripts/tmux-fixer")
fk=$(key @cockpit_fixer_key f)
[ -z "$fk" ] || tmux bind-key "$fk" display-popup -E -w "$PW" -h 6 -T ' side fix · #{pane_id} ' "$Q_FIXER -p $SRC_PANE"

# Off until phase 3 moves it to f: stock T is select-pane -T. Needs bin/cockpit-tui.
Q_PANEL=$(sq "$DIR/scripts/cockpit-panel")
pk=$(key @cockpit_panel_key "")
[ -z "$pk" ] || tmux bind-key "$pk" display-popup -E -w 94% -h 90% -T ' panel · #{pane_id} ' "$Q_PANEL -p $SRC_PANE"

# The board is wider than $PW: a 70% popup on 120 columns fits one tile column.
Q_BOARD=$(sq "$DIR/scripts/cockpit-board")
bk=$(key @cockpit_board_key w)
# Bound even without bin/cockpit-tui, so the wrapper can say "not installed".
if [ -n "$bk" ]; then
    # Not SRC_PANE, for the same reason as the review window above (#45).
    if [ "$(tmux show-option -gqv @cockpit_board_window)" = on ]; then
        tmux bind-key "$bk" run-shell "tmux new-window -n board $(sq "$Q_BOARD -p '#{pane_id}'")"
    else
        tmux bind-key "$bk" display-popup -E -w 90% -h "$PH" -T ' board · #{session_name} ' "$Q_BOARD -p $SRC_PANE"
    fi
fi

# Opt-in: this runs on every config reload, which must not wait on curl.
if [ "$(tmux show-option -gqv @cockpit_tui_fetch)" = on ]; then
    tmux run-shell -b "$(sq "$DIR/scripts/tui-fetch")"
fi

# Reload sources every file in #{config_files}, resolved once at bind time.
# Multi-command bindings go through a conf file: from argv, `\;` ends bind-key itself.
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
            # -q: a listed but missing /etc/tmux.conf would abort the whole chain.
            printf '    source-file -q %s\n' "$(sq "$f")"
        done
        printf '    display-message "config reloaded"\n'
        printf '}\n'
    fi
} > "$rlconf"
tmux source-file "$rlconf"
rm -f "$rlconf"

# switch-client first: moving the last window out of the client's session would
# destroy it and detach the client. The source is pinned by id (#{S:} shadows it).
# User text stays data: ##{window_name} expands in display-menu, and the typed name
# goes through %%% once into an option, so #( never runs. Four # for two expansions.
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

# The flag is the state, and unsetting inherits the config's value, so nothing is stashed.
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

# A row replays its action's key (send-keys -K), so a rebound key moves its row
# and an off key drops it.
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

# Click a line of output to pick a path off it. The line travels in a pane option
# (-F), since pane output is full of quotes that would tear a shell string apart.
# No mouse-mode guard: agent TUIs keep mouse reporting on, so a guard would hand
# these chorded clicks to the agent. Plain gestures are never bound.
conf=$(mktemp) || exit 1
trap 'rm -f "$conf"' EXIT HUP INT TERM
{
    # Stock swap-pane fires on the first press and moves the second click to another
    # pane, so the double-click never forms. @cockpit_keep_swap_pane keeps swap-pane.
    if [ -z "$(tmux show-option -gqv @cockpit_keep_swap_pane)" ]; then
        printf 'bind-key -n C-MouseDown1Pane select-pane -t=\n'
        printf 'bind-key -n C-DoubleClick1Pane {\n'
        printf '    select-pane -t=\n'
        printf '    set -pF -t= @cockpit_click_line "#{mouse_line}"\n'
        printf '    run-shell -b "%s #{pane_id}"\n' "$Q_OPEN"
        printf '}\n'
    fi
    # Ctrl+right-click: no terminal link handler claims it, unlike Ctrl+left on an OSC 8 link.
    printf 'bind-key -n C-MouseDown3Pane {\n'
    printf '    select-pane -t=\n'
    printf '    set -pF -t= @cockpit_click_line "#{mouse_line}"\n'
    printf '    run-shell -b "%s #{pane_id}"\n' "$Q_OPEN"
    printf '}\n'
    # Alt+click, for terminals that forward it (Ghostty strips Alt).
    printf 'bind-key -n M-MouseDown1Pane {\n'
    printf '    select-pane -t=\n'
    printf '    set -pF -t= @cockpit_click_line "#{mouse_line}"\n'
    printf '    run-shell -b "%s #{pane_id}"\n' "$Q_OPEN"
    printf '}\n'
} > "$conf"
tmux source-file "$conf"
rm -f "$conf"

# Last, so no later message overwrites it; once per server, not on every reload.
# shellcheck source=scripts/lib/vnum.sh
if command -v fzf >/dev/null 2>&1 && [ -z "$(tmux show-option -gqv @cockpit_fzf_notice)" ] &&
        . "$DIR/scripts/lib/vnum.sh" 2>/dev/null; then
    fzf_v=$(fzf --version 2>/dev/null); fzf_v=${fzf_v%% *}
    tmux set-option -gq @cockpit_fzf_notice "${fzf_v:-?}"
    if [ "$(vnum "$fzf_v")" -lt "$(vnum 0.63.0)" ]; then
        tmux display-message "fzf $fzf_v found; tmux-cockpit's pickers need 0.63+, bootstrap.sh fetches it"
    fi
fi
