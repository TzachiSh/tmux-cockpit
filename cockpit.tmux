#!/usr/bin/env bash
# tmux-cockpit. Load with:  run-shell ~/.tmux/plugins/tmux-cockpit/cockpit.tmux
# Draws the status line and pane borders (scripts/lib/statusbar.conf; off with
# @cockpit_statusbar off), defaults the cockpit options, and binds every cockpit key.
# Keys and options are documented in KEYS.md, docs/status-bar.md and the README.
set -u

# A failed cd must stop here, not bind every key to /scripts/...
DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || exit 1

# COCKPIT_TUI_BIN, TUI_MIN and vnum, before anything reads them.
# shellcheck source=scripts/lib/tui.sh
. "$DIR/scripts/lib/tui.sh" || exit 1
# tpm hides plugin output, so an old tmux only hears this message. A version
# with no leading digit (next-3.9, master) is a build newer than any release.
tmux_version=$(tmux -V 2>/dev/null); tmux_version=${tmux_version#tmux }
case "$tmux_version" in [0-9]*)
    if [ "$(vnum "$tmux_version")" -lt "$(vnum 3.8)" ]; then
        tmux display-message "tmux-cockpit needs tmux 3.8 or newer (found $tmux_version); bootstrap.sh builds it"
        exit 0
    fi ;;
esac

# Single quotes, not printf %q: inside a tmux double-quoted string tmux eats
# %q's backslashes before sh sees them.
sq() { printf "'%s'" "${1//\'/\'\\\'\'}"; }   # close, escaped quote, reopen
# fesc: run-shell expands its command as a format, so a # in the install path
# must reach sh as a #.
# shellcheck source=scripts/lib/fmt.sh
. "$DIR/scripts/lib/fmt.sh" || exit 1

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
# Trailing slashes off: statusbar.conf matches it whole or followed by a / (#296).
home=${HOME:-/nonexistent}
while [ "$home" != / ] && [ "${home%/}" != "$home" ]; do home=${home%/}; done
tmux set-option -gq @cockpit_home_re "$(printf '%s' "$home" | sed 's/[][\\.*^$+?(){}|]/\\&/g')"

# --- status line and borders -----------------------------------------------------
# The script's path, shell-quoted, for the formats and hooks to expand: a format's
# value reaches sh as it is, where a tmux "..." string would eat sq()'s backslashes.
tmux set-option -g @cockpit_where_cmd "$(sq "$DIR/scripts/tmux-where")"
tmux source-file "$DIR/scripts/lib/statusbar.conf"
# NO_COLOR: every slot is the terminal's default colour (the slots are the bar's).
if [ -n "${NO_COLOR:-}" ] && [ "$(tmux show-option -gqv @cockpit_statusbar)" != off ]; then
    set --
    for slot in black white dark-grey light-grey blue red cyan yellow green magenta; do
        set -- "$@" set-option -s "dark-theme-$slot" default ';' set-option -s "light-theme-$slot" default ';'
    done
    tmux "$@"
fi
# The branch is a pane option these hooks push (a shell per event, never per
# redraw). Index 9 leaves a user's own hook at [0]; OSC 133 shells add a push per
# prompt, which catches a `git switch` typed in place.
for h in window-pane-changed session-window-changed client-session-changed \
         after-new-window after-split-window pane-shell-prompt pane-command-finished; do
    tmux set-hook -g "${h}[9]" 'run-shell -b "exec #{@cockpit_where_cmd} branch --set #{pane_id}"'
done
# Moving, killing or closing changes the summaries without a state transition, so
# these hooks recount every window and session (agent-state sweep). Debounced (#258):
# each event bumps @cockpit_sweep_seq in the server, no fork, and hands its sweep the
# new value; a sweep that finds a newer one after its pause leaves the work to it.
tmux set-option -g @cockpit_state_cmd "$(sq "$DIR/scripts/agent-state")"
for h in window-linked window-unlinked pane-exited after-kill-pane session-closed; do
    tmux set-hook -g "${h}[9]" 'set-option -gF @cockpit_sweep_seq "#{e|+:#{@cockpit_sweep_seq},1}" ; run-shell -b "exec #{@cockpit_state_cmd} sweep #{@cockpit_sweep_seq}"'
done
# The gauge follows the active pane, so a focus change can move the session's gauge
# cells (@cockpit_ctx_all, the tab budget). No shell: agent-state's own format.
tmux set-hook -g 'window-pane-changed[8]' \
    'if-shell -F "#{e|!=:#{E:@cockpit_fmt_cells},#{@cockpit_ctx_all}}" { set-option -F @cockpit_ctx_all "#{E:@cockpit_fmt_cells}" }'
# Panes and windows that predate the hooks (a reload), in the background: run-shell
# waits on this script's output, so the job must not hold it open. The sweep also
# writes the formats the hooks above read (@cockpit_fmt_*).
{
    "$DIR/scripts/agent-state" sweep
    tmux list-panes -a -F '#{pane_id}' 2>/dev/null | while IFS= read -r p; do
        "$DIR/scripts/tmux-where" branch --set "$p"
    done
} >/dev/null 2>&1 &
# Stock overflow markers, rewritten: "< " and " >" on desktop; touch has ‹ › instead.
# Skipped when status-format[0] is not stock's (yours, or already rewritten).
if [ "$(tmux show-option -gqv @cockpit_statusbar)" != off ]; then
    sf=$(tmux show-option -gv 'status-format[0]' 2>/dev/null)
    stock='#[list=left-marker]<#[list=right-marker]>'
    case "$sf" in *"$stock"*)
        mark='#[list=left-marker]#{?#{E:@cockpit_is_touch},,< }#[list=right-marker]#{?#{E:@cockpit_is_touch},, >}'
        tmux set-option -g 'status-format[0]' "${sf/"$stock"/"$mark"}" ;;
    esac
fi

# --- keys ---------------------------------------------------------------------
# A key the plugin lets go of gets its stock binding back, or is unbound if stock
# has none (#258). Stock is read from a throwaway server started with no config on
# a private socket, so every key works (Space, ?, digits, punctuation, not only the
# letters a table listed) and a reload cannot mistake an earlier load's binding for
# stock. The server has no session, so it exits after the one call. A fixed short
# base, not $TMPDIR: a socket path past about 104 bytes cannot be bound. Only
# tmux's "unknown key" (stock has none) unbinds. A lookup that fails any other way
# (no dir, no server, no answer) is not "binds nothing": the key stays as it is,
# a message says why, and the 1 sends it to the next load's retry (#290).
give_back() {                 # <key> -> 1 when tmux's own binding could not be read
    local k=$1 d out="" err="" r note cmd
    # tmux reads a lone ; argument as a command separator.
    [ "$k" = ';' ] && k='\;'
    if ! d=$(mktemp -d /tmp/cockpit-gb.XXXXXX 2>/dev/null); then
        err="no temp dir"
    elif out=$(env -u TMUX tmux -S "$d/s" -f /dev/null start-server \; \
            list-keys -T prefix -F "#{key_repeat}"$'\037'"#{key_note}"$'\037'"#{key_command}" -- "$k" 2>"$d/err") \
            && [ -n "$out" ]; then
        rm -rf -- "$d"
        IFS=$'\037' read -r r note cmd <<< "$out"
        [ "$r" = 1 ] && r=-r || r=""
        tmux bind-key ${r:+"$r"} -N "$note" -T prefix "$k" "$cmd"
        return 0
    else
        read -r err < "$d/err"
        rm -rf -- "$d"
        case "$err" in "unknown key: "*) tmux unbind-key -T prefix "$k"; return 0 ;; esac
    fi
    err="tmux-cockpit: could not read tmux's own binding for $k (${err:-no answer})"
    tmux display-message "$(fesc "$err; left it bound, the next load tries again")"
    return 1
}
# Still cockpit's <name> binding: its note, or, from a load before the notes, no
# note and a command only cockpit binds. The second form can go after a release.
owned() {                     # <name> <key>
    local nc
    nc=$(tmux list-keys -T prefix -F "#{key_note}"$'\037'"#{key_command}" "$2" 2>/dev/null)
    case "${nc%%$'\037'*}" in
        "cockpit: $1") return 0 ;;
        "") case "${nc#*$'\037'}" in
                *"$DIR/scripts/"*|*'display-menu -T "#[align=centre]'*) return 0 ;;
            esac ;;
    esac
    return 1
}

# Lowercase keys: a Shift-letter chord dies under Caps Lock or a non-Latin layout.
# Each effective key stays in a variable (empty = off) for the agents menu.
key() {                       # <option> <default> -> the key, empty when off
    local k; k=$(tmux show-option -gqv "$1")
    case "$k" in off) ;; "") printf '%s' "$2" ;; *) printf '%s' "$k" ;; esac
}
# Every cockpit key is bound with a "cockpit: <name>" note and recorded in
# @cockpit_<name>_bound, so the next load knows which keys were its own.
ckey() {                      # <name> <key> <command...>
    local n=$1 k=$2; shift 2
    tmux bind-key -N "cockpit: $n" "$k" "$@"
    tmux set-option -g "@cockpit_${n}_bound" "$k"
}

# Every key is settled before any is bound or given back, so giving an old one
# back can never undo a binding this load makes. @cockpit_ai_key was the agents
# menu's key; it still works, with a notice.
menuk=$(tmux show-option -gqv @cockpit_menu_key)
aik=$(tmux show-option -gqv @cockpit_ai_key)
if [ -z "$menuk" ] && [ -n "$aik" ]; then
    menuk=$aik
    tmux display-message "tmux-cockpit: @cockpit_ai_key is now @cockpit_menu_key"
fi
case "$menuk" in off) menuk="" ;; "") menuk=a ;; esac
sk=$(key @cockpit_sessions_key s)
jk=$(key @cockpit_jump_key u)
ik=$(key @cockpit_info_key i)
# @cockpit_fixer_key off kept stock f; the panel taking f must not undo that.
pdef=f; [ "$(tmux show-option -gqv @cockpit_fixer_key)" = off ] && pdef=""
pk=$(key @cockpit_panel_key "$pdef")
bk=$(key @cockpit_board_key w)
rk=$(key @cockpit_review_key r)
rlk=$(key @cockpit_reload_key R)
mk=$(key @cockpit_move_key M)
fsk=$(key @cockpit_fullscreen_key e)

# The retired side fix's bindings outlive it on a server that only reloads.
tmux list-keys -T prefix -F "#{key_string}"$'\037'"#{key_command}" 2>/dev/null \
    | awk -F'\037' 'index($2, "scripts/tmux-fixer") { print $1 }' \
    | while IFS= read -r k; do give_back "$k"; done
# A reload that turns a key off or moves it gives the old key back, unless it is
# no longer ours: the user bound it since. A key another cockpit key takes this
# load is given back too, then bound again below. One that could not be given
# back waits in @cockpit_giveback ("<name>:<key> ...") for the next load, since
# the <name>'s own record moves on to its new key.
now=" menu:$menuk sessions:$sk jump:$jk info:$ik panel:$pk board:$bk review:$rk"
now="$now reload:$rlk move:$mk fullscreen:$fsk "
todo=$(tmux show-option -gqv @cockpit_giveback)
set -f
for nk in $now; do
    n=${nk%%:*}
    prev=$(tmux show-option -gqv "@cockpit_${n}_bound")
    [ -n "$prev" ] && [ "$prev" != "${nk#*:}" ] || continue
    todo="$todo $n:$prev"
    tmux set-option -gu "@cockpit_${n}_bound"
done
left=""
for nk in $todo; do
    case "$now" in *" $nk "*) continue ;; esac     # its own key again: bound below
    owned "${nk%%:*}" "${nk#*:}" || continue      # the user's now: nothing to give back
    give_back "${nk#*:}" || left="$left $nk"
done
set +f
if [ -n "$left" ]; then tmux set-option -g @cockpit_giveback "${left# }"
else tmux set-option -gu @cockpit_giveback; fi

Q_BRANCH=$(sq "$DIR/scripts/tmux-branch")
Q_OPEN=$(sq "$DIR/scripts/tmux-open-path")
# The pane a popup came from, via the session id $TMUX ends in: display-popup
# expands formats only in -T and -d, and inside it $TMUX_PANE is the popup's (#38).
SRC_PANE='"$(tmux display-message -p -t "\$${TMUX##*,}" "#{pane_id}")"'

if [ "$(tmux show-option -gqv mouse)" != on ]; then
    tmux display-message "tmux-cockpit: the click gestures need 'set -g mouse on'; prefix+b still works"
fi

PW=$(tmux show-option -gqv @cockpit_popup_width);  PW=${PW:-70%}
PH=$(tmux show-option -gqv @cockpit_popup_height); PH=${PH:-70%}

# Browse folder is the menu row's name. prefix o, the path tap and that row open
# the pane's folder in the editor (#228). run-shell expands #{pane_id}; the
# script's path goes through fesc as pick_on's does.
tmux bind-key -T cockpit o run-shell -b "$(fesc "$Q_OPEN") --folder #{pane_id}"
# What is each agent doing: one line per pane, its state or - for a non-agent.
# display-popup expands no format in its command, so list-panes gets the #{...}.
# The columns are tmux's own padding (p16), so a title with spaces stays whole.
tmux bind-key -T cockpit t display-popup -E -w 78% -h 40% -T ' agents ' \
    "tmux list-panes -a -F '#{p16:#{session_name}:#{window_index}} #{p12:#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{@panestate},-}} #{pane_title}'; read -r _"
# tmux keeps its drag flag when a popup eats the release and turns the next press
# into MouseDragEnd1<where>, losing a tab click (#21). Stock leaves this unbound.
tmux bind-key -n MouseDragEnd1Status switch-client -t =
tmux bind-key b display-popup -E -w "$PW" -h "$PH" -T ' switch branch · #{b:pane_current_path} ' "$Q_BRANCH -p $SRC_PANE"

# Replaces stock choose-tree on s; tmux 3.8 took g for its floating-pane menu.
# tm --popup opens the popup itself: its height follows the session count,
# which a display-popup size cannot express.
Q_TM=$(sq "$DIR/scripts/tm")
[ -z "$sk" ] || ckey sessions "$sk" run-shell -b "$(fesc "$Q_TM") --popup '#{client_name}' '#{session_id}' '#{client_width}' '#{client_height}'"

# run-shell sets no $TMUX_PANE, so the pane goes in as an argument (#45).
[ -z "$jk" ] || ckey jump "$jk" run-shell "$(fesc "$(sq "$DIR/scripts/tmux-jump")") '#{pane_id}' '#{client_name}'"

# The info screen is cockpit-tui's own (#252); esc or q closes it, so no `read`.
# The binary reads $COCKPIT_GLYPHS, not @cockpit_glyphs (#293): read at open, so
# setting the option needs no reload.
[ -z "$ik" ] || ckey info "$ik" display-popup -E -w "$PW" -h "$PH" -T ' info · #{pane_id} ' \
    "g=\$(tmux show-option -gqv @cockpit_glyphs); exec env \${g:+COCKPIT_GLYPHS=\"\$g\"} $(sq "$COCKPIT_TUI_BIN") info $SRC_PANE"

Q_PANEL=$(sq "$DIR/scripts/cockpit-panel")
# Read at load like the key: a strict word, since the binary refuses any other.
pmode=$(tmux show-option -gqv @cockpit_panel_mode)
case "$pmode" in
    ''|auto|pc|tablet) ;;
    *) tmux display-message "tmux-cockpit: @cockpit_panel_mode is [$pmode]; use auto, pc or tablet (auto for now)"; pmode=auto ;;
esac
if [ -n "$pk" ]; then
    ckey panel "$pk" display-popup -E -e "COCKPIT_PANEL_MODE=${pmode:-auto}" -w 94% -h 90% \
        -T ' panel · #{pane_id} ' "$Q_PANEL -p $SRC_PANE"
fi

# The board is wider than $PW: a 70% popup on 120 columns fits one tile column.
Q_BOARD=$(sq "$DIR/scripts/cockpit-board")
# The board's jump moves the pressing client, not the most recently active one
# (#296). No form can hand it over as an argument (display-popup expands no
# format in its command), so the key leaves it on the pane it was pressed in.
pin=(set-option -pF @cockpit_board_client '#{client_name}' '\;')
# Bound even without the binary, so the wrapper can say "not installed".
if [ -n "$bk" ]; then
    # Not SRC_PANE, for the same reason as the review window below (#45).
    if [ "$(tmux show-option -gqv @cockpit_board_window)" = on ]; then
        ckey board "$bk" "${pin[@]}" run-shell "tmux new-window -n board -e COCKPIT_PANEL_MODE=${pmode:-auto} $(sq "$(fesc "$Q_BOARD") -p '#{pane_id}'")"
    else
        # No #{session_name}: it named the client's session, not the agent's.
        ckey board "$bk" "${pin[@]}" display-popup -E -e "COCKPIT_PANEL_MODE=${pmode:-auto}" -w 90% -h "$PH" -T ' board ' "$Q_BOARD -p $SRC_PANE"
    fi
fi

# prefix r is the board too (#253): the review queue is the grouped list, and
# `r` on a row opens the review. The same command prefix w runs; the window form
# cannot use SRC_PANE, since new-window activates itself before its command
# runs, so run-shell expands #{pane_id} on the source pane instead (#45).
if [ -n "$rk" ]; then
    if [ "$(tmux show-option -gqv @cockpit_review_window)" = on ]; then
        ckey review "$rk" "${pin[@]}" run-shell "tmux new-window -n board -e COCKPIT_PANEL_MODE=${pmode:-auto} $(sq "$(fesc "$Q_BOARD") -p '#{pane_id}'")"
    else
        ckey review "$rk" "${pin[@]}" display-popup -E -e "COCKPIT_PANEL_MODE=${pmode:-auto}" -w 90% -h "$PH" -T ' board ' "$Q_BOARD -p $SRC_PANE"
    fi
fi

# Required (#199). Offline: one --version, never a fetch, on every load.
tv=$(tui_check)
case $? in
    1) tmux display-message "tmux-cockpit: cockpit-tui is missing; run $DIR/install.sh" ;;
    2) tmux display-message "tmux-cockpit: cockpit-tui $tv is older than $TUI_MIN; run $DIR/install.sh" ;;
esac

# Opt-in: this runs on every config reload, which must not wait on curl.
if [ "$(tmux show-option -gqv @cockpit_tui_fetch)" = on ]; then
    tmux run-shell -b "$(fesc "$(sq "$DIR/scripts/tui-fetch")")"
fi

# Reload sources every file in #{config_files}, resolved once at bind time.
# From argv a bare ; argument ends bind-key itself, so pick_on below passes '\;'.
# The block bindings below go to tmux on stdin (source-file -), not via a file.
if [ -n "$rlk" ]; then
    {
        old_ifs=$IFS
        IFS=,
        set -f
        # shellcheck disable=SC2046 # splitting #{config_files} on comma is the point
        set -- $(tmux display-message -p '#{config_files}' 2>/dev/null)
        set +f
        IFS=$old_ifs
        [ "$#" -gt 0 ] || set -- "${HOME:-}/.tmux.conf"
        printf 'bind-key -N %s %s {\n' "$(sq 'cockpit: reload')" "$(sq "$rlk")"
        for f in "$@"; do
            # -q: a listed but missing /etc/tmux.conf would abort the whole chain.
            printf '    source-file -q %s\n' "$(sq "$f")"
        done
        printf '    display-message "config reloaded"\n'
        printf '}\n'
    } | tmux source-file -
    tmux set-option -g @cockpit_reload_bound "$rlk"
fi

# switch-client first: moving the last window out of the client's session would
# destroy it and detach the client. The source is pinned by id (#{S:} shadows it).
# User text stays data: ##{window_name} expands in display-menu, and the typed name
# goes through %%% once into an option, so #( never runs. Four # for two expansions.
# A session name's # is doubled by backreference: on 3.8 an s/// pattern of a bare #
# matches nothing, and a ## replacement inserts nothing.
if [ -n "$mk" ]; then
    {
        printf 'bind-key -N %s %s {\n' "$(sq 'cockpit: move')" "$(sq "$mk")"
        cat <<'EOF'
    set -gF @cockpit_move_src "#{window_id}"
    set -gF @cockpit_move_cwd "#{pane_current_path}"
    run-shell -C "display-menu -T '#[align=centre] move ##{window_index}:##{window_name} to ' -x W -y W #{S/n:#{s|(#)|\\1\\1|:#{qa:session_name}} '' 'switch-client -t #{session_id} ; move-window -s #{@cockpit_move_src} -t #{session_id}: ; select-window -t #{@cockpit_move_src}' ,} '' 'new session...' n \"command-prompt -p 'new session:' 'set -g @cockpit_move_dst \\\"%%%\\\" ; if-shell -F \\\"####{@cockpit_move_dst}\\\" { new-session -s \\\"####{@cockpit_move_dst}\\\" -c \\\"####{@cockpit_move_cwd}\\\" ; move-window -k -s #{@cockpit_move_src} -t :^ }'\""
}
EOF
    } | tmux source-file -
    tmux set-option -g @cockpit_move_bound "$mk"
fi

# The flag is the state, and unsetting inherits the config's value, so nothing is stashed.
if [ -n "$fsk" ]; then
    {
        printf 'bind-key -N %s %s {\n' "$(sq 'cockpit: fullscreen')" "$(sq "$fsk")"
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
    } | tmux source-file -
    tmux set-option -g @cockpit_fullscreen_bound "$fsk"
fi

# --- the menu and the taps ---------------------------------------------------------
# One menu: prefix a (centred), ≡ on the border and a right-click on the bar (at
# the pointer). A row exists only if nothing on screen does that job: Browse
# folder and Switch branch show when the border has no room for the path or the
# branch. A row replays its action's key (send-keys -K), so a rebound key moves
# its row and an off key drops it. ##: display-menu expands an item's command
# once, so a quote in a path or name reaches the command as data.
rows=()
row() { [ -z "$3" ] || rows+=("$1" "$2" "switch-client -T prefix ; send-keys -K $3"); }
row 'Board of every agent'        w "$bk"
row 'Panel for this pane'         f "$pk"
rows+=('What is each agent doing?' t 'switch-client -T cockpit ; send-keys -K t')
rows+=('')
rows+=('#{?#{E:@cockpit_path_shown},,Browse folder}' o 'switch-client -T cockpit ; send-keys -K o')
row '#{?#{E:@cockpit_branch_shown},,#{?#{E:@cockpit_where_branch},Switch branch,}}' b b
rows+=('')
rows+=('New window' c "new-window -c '##{pane_current_path}'")
rows+=('#{?#{E:@cockpit_is_touch},,Split right}' "\\" "split-window -h -c '##{pane_current_path}'")
rows+=('#{?#{E:@cockpit_is_touch},,Split down}' - "split-window -v -c '##{pane_current_path}'")
row 'Move window to a session...' M "$mk"
# The typed name is staged with %%% (quotes kept) and renamed by tmux-rename,
# which escapes it: rename-window and rename-session expand formats, and a bare
# %% let a " end the string (#289). The id is the one the menu was opened on
# (expanded once here). The path is escaped twice: the menu expands the
# item, then run-shell its command.
Q_REN=$(fesc "$(fesc "$(sq "$DIR/scripts/tmux-rename")")")
rows+=('Rename window' ',' "command-prompt -p '(rename-window)' -I '##W' \"set -g @cockpit_rename_to \\\"%%%\\\" ; run-shell \\\"$Q_REN window '#{window_id}'\\\"\"")
rows+=('#{?#{e|>:#{window_panes},1},Kill pane…,Close window…}' x \
       "confirm-before -p '#{?#{e|>:#{window_panes},1},Kill pane ##P?,Close window ##I?} (y/n)' kill-pane")
rows+=('')
row 'Full screen (hide the bar)'  e "$fsk"
row 'Pane info'                   i "$ik"
rows+=('Rename session' '$' "command-prompt -p '(rename-session)' -I '##S' \"set -g @cockpit_rename_to \\\"%%%\\\" ; run-shell \\\"$Q_REN session '#{session_id}'\\\"\"")
rows+=('Detach' d detach-client)
row 'Reload config'               R "$rlk"
rows+=('Kill session' Q "confirm-before -p 'kill session ##S? (y/n)' kill-session")
tmux bind-key -T cockpit m display-menu -T '#[align=centre]#{session_name}' -x M -y M "${rows[@]}"
if [ -n "$menuk" ]; then
    ckey menu "$menuk" display-menu -T '#[align=centre]#{session_name}' -x C -y C "${rows[@]}"
fi

# Taps act on release: a menu opened on the press is closed by its own release.
# The border's corner, path and branch are control ranges 0-3 (4-6 stay yours);
# stock binds 7 ([f]), 8 ([z]) and 9 ([x]). The window line's ‹ › are user ranges.
tmux unbind-key -q -n MouseDown1Control1 \; unbind-key -q -n MouseDown1Control2
# From argv a bare ; would end bind-key itself, so a two-command binding passes '\;'.
if [ -n "$sk" ]; then
    tmux bind-key -n MouseUp1Control0 switch-client -T prefix '\;' send-keys -K "$sk"
else
    tmux unbind-key -q -n MouseUp1Control0
fi
tmux bind-key -n MouseUp1Control1 switch-client -T cockpit '\;' send-keys -K m
tmux bind-key -n MouseUp1Control2 switch-client -T cockpit '\;' send-keys -K o
tmux bind-key -n MouseUp1Control3 switch-client -T prefix '\;' send-keys -K b
tmux bind-key -n MouseUp1Status if-shell -F '#{==:#{mouse_status_range},cp-prev}' previous-window \
    "if-shell -F '#{==:#{mouse_status_range},cp-next}' next-window"
tmux bind-key -n MouseUp3Status if-shell -F '#{m:cp-*,#{mouse_status_range}}' \
    'switch-client -T cockpit ; send-keys -K m'

# Click a line of output to pick a path off it. The line travels in a pane option
# (-F), since pane output is full of quotes that would tear a shell string apart.
# No mouse-mode guard: agent TUIs keep mouse reporting on, so a guard would hand
# these chorded clicks to the agent. Plain gestures are never bound.
# Bound from argv, not a sourced file: in a tmux "..." string sq()'s '\'' loses its \.
pick_on() {
    tmux bind-key -n "$1" select-pane -t= '\;' set -pF -t= @cockpit_click_line '#{mouse_line}' \
        '\;' run-shell -b "$(fesc "$Q_OPEN") #{pane_id}"
}
pick_on C-DoubleClick1Pane
# Ctrl+right-click: no terminal link handler claims it, unlike Ctrl+left on an OSC 8 link.
pick_on C-MouseDown3Pane
# Alt+click, for terminals that forward it (Ghostty strips Alt).
pick_on M-MouseDown1Pane
