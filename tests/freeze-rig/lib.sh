#!/usr/bin/env bash
# tmux-cockpit freeze rig -- shared library.
# Everything runs on a PRIVATE tmux server; nothing touches the user's server.
set -u

RIG_DIR="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO=${REPO:-/home/tzachi/.tmux/plugins/tmux-cockpit}

# ---------------------------------------------------------------- format text
# The shipped formats, lifted verbatim from examples/tmux.conf +
# examples/full.tmux.conf so we measure what users actually run.

GLOB='|claude|codex|aider|cursor|opencode|gemini|crush|'

# the agent guard used everywhere: a pane's state, believed only while the pane
# is actually running an agent
GUARD='#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{@panestate},}'

BORDER_STYLE="fg=#{?#{==:${GUARD},needs-input},#{@thm_red},#{?#{==:${GUARD},error},#{@thm_peach},#{?#{==:${GUARD},running},#{@thm_yellow},#{?#{==:${GUARD},done},#{@thm_green},#{@thm_overlay_0}}}}}"

# the aggregate cross-window banner: a #{S:#{W:#{P:}}} walk of the whole server
BANNER_WALK='#{S:#{W:#{?window_active,,#{?#{m:*needs-input*,#{P:#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{@panestate},} }},#{window_index}:#{window_name} ,}}}}'
BORDER_FORMAT="#{?#{&&:#{pane_active},#{!=:${BANNER_WALK},}},#[fg=#{@thm_red}]#[bold]─ NEEDS YOU ${BANNER_WALK}#[default],#{?pane_active,,#{?#{==:${GUARD},needs-input},#[fg=#{@thm_red}]#[bold]─ NEEDS YOU #{=/48/…:#{@panestate_msg}}#[default],#{?#{==:${GUARD},error},#[fg=#{@thm_peach}]#[bold]─ ERROR #[default],}}}}#{?#{mouse},#[align=right]#{?#{e|>:#{window_panes},1},#[range=control|8][#{?#{window_zoomed_flag},u,z}]#[norange],}#[range=control|9][x]#[norange],}"

# the same format with the banner removed -- the per-pane half only
BORDER_FORMAT_NOBANNER="#{?pane_active,,#{?#{==:${GUARD},needs-input},#[fg=#{@thm_red}]#[bold]─ NEEDS YOU #{=/48/…:#{@panestate_msg}}#[default],#{?#{==:${GUARD},error},#[fg=#{@thm_peach}]#[bold]─ ERROR #[default],}}}#{?#{mouse},#[align=right]#{?#{e|>:#{window_panes},1},#[range=control|8][#{?#{window_zoomed_flag},u,z}]#[norange],}#[range=control|9][x]#[norange],}"

# window tab: five #{P:} walks over the window's own panes
_dot() { printf '#{?#{m:*%s*,#{P:#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{@panestate},} }},#[fg=#{@thm_%s}]%s,' "$1" "$2" "$3"; }
TAB_TEXT=" #{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{?#{==:#{pane_title},#{host_short}},#W,#{=/16/…:#{s/^✳ //:#{pane_title}}}},#W} "
TAB_TEXT+="$(_dot needs-input red ▲)$(_dot error peach ✕)$(_dot running yellow ●)$(_dot done green ✓)"
TAB_TEXT+='#{?#{m:*A*,#{P:#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},A,}}},#[fg=#{@thm_overlay_0}]○,}}}}}'
TAB_TEXT+='#{?#{e|>=:#{@panestate_ctx},80},#[fg=colour208] ◔#{@panestate_ctx}%,}'

# fleet counter: FOUR full server walks per expansion (each count appears twice)
_cnt() { printf '#{n:#{S:#{W:#{P:#{?#{==:#{@panestate},%s},#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},x,},}}}}}' "$1"; }
FLEET="#{?#{e|>:$(_cnt needs-input),0},#[fg=#{@thm_red}#,bold]▲$(_cnt needs-input)#[default] ,}#{?#{e|>:$(_cnt error),0},#[fg=#{@thm_peach}]✕$(_cnt error)#[default] ,}"

TINT_STYLE="bg=#{?#{==:${GUARD},needs-input},#{@cockpit_blocked_bg},default}"

# The two #() shell tokens cockpit.tmux interpolates into status-right. These
# are the only part of the shipped config that spawns a process on redraw.
# The command string embeds #{pane_pid}, so it changes whenever the active
# pane changes -- and format_job_get() treats a changed command as `force`,
# killing and respawning the job immediately instead of honouring its
# once-per-second rate limit.
WHERE="$REPO/scripts/tmux-where"
SHELL_TOKENS="#('$WHERE' path #{pane_pid} '#{pane_current_path}' '#{pane_current_command}') #('$WHERE' branch #{pane_pid} '#{pane_current_path}' '#{pane_current_command}')"

# ---------------------------------------------------------------- server setup
SOCK=""
srv() { tmux -L "$SOCK" "$@"; }

rig_start() {           # rig_start <sockname>
    SOCK="$1"
    tmux -L "$SOCK" kill-server 2>/dev/null
    tmux -L "$SOCK" -f /dev/null new-session -d -s s1 -x 200 -y 62 \; \
        set -g status-interval "${STATUS_INTERVAL:-2}" \; \
        set -g history-limit "${HISTLIMIT:-50000}" \; \
        set -g mouse on \; \
        set -g @cockpit_agents_glob "$GLOB" \; \
        set -g @thm_red '#ff6b6b' \; set -g @thm_peach '#ff9838' \; \
        set -g @thm_yellow '#ffcc33' \; set -g @thm_green '#3dea7d' \; \
        set -g @thm_overlay_0 '#5b6272' \; set -g @cockpit_blocked_bg '#52303a' \; \
        set -g status-left '' \; set -g status-right '' \; \
        set -g window-status-format '#W' \; set -g window-status-current-format '#W'
}

# The command string tmux hands to /bin/sh. exec -a is what makes
# pane_current_command read "claude"; a script *named* claude would not, since
# the kernel puts the interpreter in argv[0].
agent_cmd() {           # agent_cmd <mode> [arg...]
    printf "bash --norc -c 'exec -a claude bash --norc %s/loop.sh %s %s %s %s'" \
        "$RIG_DIR" "$1" "${2:-}" "${3:-}" "${4:-}"
}

# Build S sessions x W windows x P panes, every pane a fake agent.
rig_shape() {           # rig_shape <S> <W> <P>
    local S=$1 W=$2 P=$3 s w p pid
    for ((s=1; s<=S; s++)); do
        [ "$s" = 1 ] || srv new-session -d -s "s$s" -x 200 -y 62
        for ((w=1; w<=W; w++)); do
            [ "$w" = 1 ] || srv new-window -d -t "s$s:"
        done
        for ((w=0; w<W; w++)); do
            for ((p=1; p<P; p++)); do
                srv split-window -d -t "s$s:$w" -l 3 2>/dev/null || \
                srv split-window -d -t "s$s:$w" 2>/dev/null || true
            done
        done
    done
    # respawn-pane, not send-keys: the pane's shell is gone once the agent
    # execs, so there is no prompt left to type into. (The first version of
    # this rig typed into a blocked `read` and measured an idle server.)
    while read -r pid; do
        srv respawn-pane -k -t "$pid" "$(agent_cmd idle)" 2>/dev/null
    done < <(srv list-panes -a -F '#{pane_id}')
    sleep 1
}

# Put a load on the first N panes.
#   spam  -- plain lines, no escape sequences
#   title -- OSC 2 title sets only
#   agent -- both, which is what a real Claude Code pane emits
rig_load() {            # rig_load <none|spam|title|agent> <n_panes>
    local mode=$1 n=$2 i=0 pid cmd
    [ "$mode" = none ] && return 0
    [ "$n" -gt 0 ] || return 0
    case "$mode" in
        spam)  cmd=$(agent_cmd spam  "${BURST:-40}" "${DELAY:-0.05}") ;;
        title) cmd=$(agent_cmd title "${OSC_N:-1}"  "${DELAY:-0.05}") ;;
        agent) cmd=$(agent_cmd agent "${BURST:-40}" "${DELAY:-0.05}" "${OSC_N:-1}") ;;
        *) return 0 ;;
    esac
    while read -r pid; do
        [ "$i" -lt "$n" ] || break
        srv respawn-pane -k -t "$pid" "$cmd" 2>/dev/null
        i=$((i+1))
    done < <(srv list-panes -a -F '#{pane_id}')
    sleep 1
}

# Mark N panes needs-input, taken from the END of the pane list so they land in
# windows that are NOT the active one -- the branch the cross-window banner
# actually renders, and the worst case for the border format.
rig_states() {          # rig_states <n_blocked>
    local n=$1 pid total cut j=0
    total=$(srv list-panes -a -F x | wc -l)
    cut=$((total - n))
    while read -r pid; do
        if [ "$j" -ge "$cut" ]; then
            srv set-option -p -t "$pid" @panestate needs-input
            srv set-option -p -t "$pid" @panestate_msg 'waiting for your answer about the refactor'
        else
            srv set-option -p -t "$pid" @panestate running
        fi
        j=$((j+1))
    done < <(srv list-panes -a -F '#{pane_id}')
}

# Attach a real client on a pty, so redraws actually happen.
CLIENT_PID=""
rig_attach() {
    script -qfc "tmux -L $SOCK attach -t s1" /dev/null >/dev/null 2>&1 &
    CLIENT_PID=$!
    disown 2>/dev/null
    sleep 1.5
    srv list-clients -F '#{client_tty}' 2>/dev/null | head -1
}

rig_cleanup() {
    [ -n "$CLIENT_PID" ] && kill -9 "$CLIENT_PID" 2>/dev/null
    CLIENT_PID=""
    [ -n "$SOCK" ] || return 0
    tmux -L "$SOCK" kill-server 2>/dev/null
    local p
    for p in $(pgrep -f "tmux -L $SOCK" 2>/dev/null); do
        [ "$p" = "$$" ] || kill -9 "$p" 2>/dev/null
    done
    rm -f "/tmp/tmux-$(id -u)/$SOCK" 2>/dev/null
    return 0
}

server_pid() { srv display-message -p '#{pid}' 2>/dev/null; }

# CPU jiffies (utime+stime) the tmux server has burned so far.
cpu_ticks() {           # cpu_ticks <pid>
    [ -n "${1:-}" ] && [ -r "/proc/$1/stat" ] || { echo 0; return; }
    awk '{print $14+$15}' "/proc/$1/stat"
}

# ------------------------------------------------------------------ variants
apply_variant() {       # apply_variant <name>
    case "$1" in
    V0) : ;;                                        # control: no cockpit format
    V1) srv set -g pane-border-status top
        srv set -g pane-border-style "$BORDER_STYLE"
        srv set -g pane-active-border-style "$BORDER_STYLE" ;;
    V2) srv set -g pane-border-status top
        srv set -g pane-border-format "$BORDER_FORMAT_NOBANNER" ;;
    V3) srv set -g pane-border-status top
        srv set -g pane-border-format "$BORDER_FORMAT" ;;
    V4) srv set -g window-status-format "$TAB_TEXT"
        srv set -g window-status-current-format "$TAB_TEXT" ;;
    V5) srv set -g status-right "$FLEET" ;;
    V6) srv set -g window-style "$TINT_STYLE"
        srv set -g window-active-style 'bg=default' ;;
    V7) apply_variant V1; apply_variant V3; apply_variant V4
        apply_variant V5; apply_variant V6 ;;
    V8) srv set -g status-right "$SHELL_TOKENS" ;;
    V9) apply_variant V7; srv set -ag status-right "$SHELL_TOKENS" ;;
    esac
}

variant_desc() {
    case "$1" in
    V0) echo 'control (no cockpit formats)' ;;
    V1) echo 'pane-border-style only' ;;
    V2) echo 'pane-border-format, banner REMOVED' ;;
    V3) echo 'pane-border-format WITH banner' ;;
    V4) echo 'window tabs (5x #{P:} walks)' ;;
    V5) echo 'status-right #{agent_fleet}' ;;
    V6) echo 'window-style tint' ;;
    V7) echo 'FULL shipped config' ;;
    V8) echo 'status-right #() shell tokens' ;;
    V9) echo 'FULL + #() shell tokens' ;;
    esac
}

# Back to a bare server, so variants measured on one shape cannot contaminate
# each other.
reset_variant() {
    srv set -g pane-border-status off
    srv set -gu pane-border-format
    srv set -gu pane-border-style
    srv set -gu pane-active-border-style
    srv set -g window-status-format '#W'
    srv set -g window-status-current-format '#W'
    srv set -g status-right ''
    srv set -gu window-style
    srv set -gu window-active-style
}
