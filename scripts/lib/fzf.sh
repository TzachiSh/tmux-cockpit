#!/usr/bin/env bash
# One fzf theme for every cockpit picker. Sourced, never run. Bash 4+.
#
# No `bg`, `list-bg` or `input-bg` anywhere below: the popup's background
# belongs to the user's popup-style, the same rule cockpit.tmux states for the
# border. `16` stays the base so an unset key degrades to the terminal's own.

# --footer, the newest option any picker asks for. Everything else in the
# styled set landed earlier, so one floor covers the whole list.
COCKPIT_FZF_MIN=0063000

# fzf is 0.x, so the THIRD component decides. cockpit.tmux's version_at_least
# reads ${v#*.} as the minor, which on 0.74.3 is "74.3" -- not an integer, so
# it grades every modern fzf as 0. install.sh and scripts/tui-fetch carry this
# same parser; neither of them is sourceable from here.
cockpit_fzf_tier() {          # -> plain | styled
    if [ -z "${COCKPIT_FZF_TIER:-}" ]; then
        local v a b c IFS=.
        v=$(fzf --version 2>/dev/null); v=${v%% *}; v=${v#v}
        # shellcheck disable=SC2086  # splitting on the dots is the point
        set -- $v
        a=${1:-0}; b=${2:-0}; c=${3:-0}
        a=${a%%[!0-9]*}; b=${b%%[!0-9]*}; c=${c%%[!0-9]*}
        v=$(printf '%d%03d%03d' "${a:-0}" "${b:-0}" "${c:-0}" 2>/dev/null)
        if [ "${v:-0}" -ge "$COCKPIT_FZF_MIN" ] 2>/dev/null; then
            COCKPIT_FZF_TIER=styled
        else
            COCKPIT_FZF_TIER=plain
        fi
    fi
    printf '%s\n' "$COCKPIT_FZF_TIER"
}

# -g, not a bare declare: a caller that sources this lib from inside one of its
# own functions would otherwise get a LOCAL cache that dies with that call.
declare -gA COCKPIT_THM=()
# One tmux round-trip for the whole palette, not one per colour key: this runs
# on every popup open, and the theme touches eleven @thm_ tokens. Defaults are
# Mocha, so a user without a catppuccin theme still gets a coloured picker.
_cockpit_fzf_thm() {
    [ "${#COCKPIT_THM[@]}" -gt 0 ] && return 0
    local k fmt='' i
    local keys=(fg peach surface_0 surface_1 surface_2 overlay_0 overlay_1 \
                overlay_2 mauve green teal)
    local defs=('#cdd6f4' '#fab387' '#313244' '#45475a' '#585b70' '#6c7086' \
                '#7f849c' '#9399b2' '#cba6f7' '#a6e3a1' '#94e2d5')
    # \037, not a tab: tab is IFS *whitespace*, so bash collapses a run of them
    # into one delimiter and every unset @thm_ token shifts the colours after
    # it onto the wrong keys. cockpit.tmux's PALETTE leaves seven of these
    # eleven unset, so the gapped palette is the default install.
    for k in "${keys[@]}"; do fmt="$fmt#{@thm_$k}"$'\037'; done
    local vals=()
    IFS=$'\037' read -r -a vals <<<"$(tmux display-message -p "$fmt" 2>/dev/null)"
    for i in "${!keys[@]}"; do
        COCKPIT_THM[${keys[$i]}]=${vals[$i]:-${defs[$i]}}
    done
}

_cockpit_fzf_color() {
    _cockpit_fzf_thm
    local c="16,fg:${COCKPIT_THM[fg]}"
    c="$c,hl:${COCKPIT_THM[peach]},fg+:${COCKPIT_THM[fg]}"
    c="$c,bg+:${COCKPIT_THM[surface_0]},hl+:${COCKPIT_THM[peach]}"
    c="$c,info:${COCKPIT_THM[overlay_1]},prompt:${COCKPIT_THM[mauve]}"
    c="$c,pointer:${COCKPIT_THM[mauve]},marker:${COCKPIT_THM[green]}"
    c="$c,spinner:${COCKPIT_THM[teal]},header:${COCKPIT_THM[overlay_2]}"
    c="$c,border:${COCKPIT_THM[surface_1]}"
    c="$c,preview-border:${COCKPIT_THM[surface_1]}"
    c="$c,preview-label:${COCKPIT_THM[overlay_1]}"
    c="$c,label:${COCKPIT_THM[overlay_1]},gutter:-1"
    c="$c,scrollbar:${COCKPIT_THM[surface_2]},query:${COCKPIT_THM[fg]}"
    c="$c,disabled:${COCKPIT_THM[overlay_0]}"
    if [ "$(cockpit_fzf_tier)" = styled ]; then
        c="$c,selected-fg:${COCKPIT_THM[fg]}"
        c="$c,selected-bg:${COCKPIT_THM[surface_2]}"
        c="$c,selected-hl:${COCKPIT_THM[green]}"
        c="$c,header-border:${COCKPIT_THM[surface_1]}"
        c="$c,footer:${COCKPIT_THM[overlay_2]}"
        c="$c,footer-border:${COCKPIT_THM[surface_1]}"
    fi
    printf '%s' "$c"
}

# cockpit_fzf_args [flag...] -- one fzf argument per line, for mapfile.
#
#   mapfile -t THEME < <(cockpit_fzf_args highlight-line "footer=$KEYS")
#   fzf "${THEME[@]}" --prompt=... "${user_options[@]}"
#
# Each argument names a styled-tier flag WITHOUT its leading dashes, with an
# optional =value; on the plain tier they are dropped and the caller puts its
# key hints in the last --header line instead. The user's own options go after
# this call: fzf takes the last occurrence of a flag and merges --color per
# key, so appending is what makes an override an override.
cockpit_fzf_args() {
    local f
    printf '%s\n' --ansi --tabstop=1 --pointer=▌ --marker=◆ --ellipsis=… \
                  "--color=$(_cockpit_fzf_color)"
    [ "$(cockpit_fzf_tier)" = styled ] || return 0
    for f in "$@"; do printf -- '--%s\n' "$f"; done
}
