#!/usr/bin/env bash
# One fzf theme for the fzf pickers left (tmux-open-path's path picker and
# folder browser; the rest are cockpit-tui). Sourced, never run. Bash 4.2+ for declare -g.
#
# No `bg`, `list-bg` or `input-bg` anywhere below: the popup's background
# belongs to the user's popup-style. `16` stays the base so an unset key
# degrades to the terminal's own. fzf 0.73+ is required; cockpit.tmux warns.

# -g, not a bare declare: a caller that sources this lib from inside one of its
# own functions (tmux-open-path's op_theme does) would otherwise get a LOCAL
# cache that dies with that call.
declare -gA COCKPIT_THM=()
# One tmux round-trip for the whole palette, not one per colour key: this runs
# on every popup open, and the theme touches ten @thm_ roles. A popup cannot ask
# its client for its theme, so `theme light` (tmux's own option) picks the
# @thm_<role>_light set. The defaults are the plugin's palette
# (scripts/lib/statusbar.conf), so a picker started before it loaded is coloured.
_cockpit_fzf_thm() {
    [ "${#COCKPIT_THM[@]}" -gt 0 ] && return 0
    local k fmt='#{theme}'$'\037' i light=0
    local keys=(fg error surface sel dim accent done review needs running)
    local dark=('#e6e4df' '#ff9e3d' '#2a2f3a' '#3a4252' '#9097a3' '#6cb6ff' \
                '#5fd38d' '#5fd3d3' '#ff5f6d' '#e6c84f')
    local lite=('#1b1e24' '#9a4c00' '#e1e5ec' '#c8d1df' '#5c626c' '#0a5fc2' \
                '#1d7439' '#00737a' '#c4122f' '#806200')
    # \037, not a tab: tab is IFS *whitespace*, so bash collapses a run of them
    # into one delimiter and every unset @thm_ token shifts the colours after
    # it onto the wrong keys. A user who overrides some roles leaves gaps.
    for k in "${keys[@]}"; do fmt="$fmt#{@thm_$k}"$'\037'"#{@thm_${k}_light}"$'\037'; done
    local vals=()
    IFS=$'\037' read -r -a vals <<<"$(tmux display-message -p "$fmt" 2>/dev/null)"
    [ "${vals[0]:-}" = light ] && light=1
    for i in "${!keys[@]}"; do
        if [ "$light" = 1 ]; then
            COCKPIT_THM[${keys[$i]}]=${vals[2 * i + 2]:-${lite[$i]}}
        else
            COCKPIT_THM[${keys[$i]}]=${vals[2 * i + 1]:-${dark[$i]}}
        fi
    done
}

# Sets COCKPIT_FZF_COLOR rather than printing it: called through $(...) the
# palette cache above would fill in a subshell and die with it.
_cockpit_fzf_color() {
    _cockpit_fzf_thm
    local c="16,fg:${COCKPIT_THM[fg]}"
    c="$c,hl:${COCKPIT_THM[error]},fg+:${COCKPIT_THM[fg]}"
    c="$c,bg+:${COCKPIT_THM[surface]},hl+:${COCKPIT_THM[error]}"
    c="$c,info:${COCKPIT_THM[dim]},prompt:${COCKPIT_THM[accent]}"
    c="$c,pointer:${COCKPIT_THM[accent]},marker:${COCKPIT_THM[done]}"
    c="$c,spinner:${COCKPIT_THM[review]},header:${COCKPIT_THM[dim]}"
    c="$c,border:${COCKPIT_THM[surface]}"
    c="$c,preview-border:${COCKPIT_THM[surface]}"
    c="$c,preview-label:${COCKPIT_THM[dim]}"
    c="$c,label:${COCKPIT_THM[dim]},gutter:-1"
    c="$c,scrollbar:${COCKPIT_THM[sel]},query:${COCKPIT_THM[fg]}"
    c="$c,disabled:${COCKPIT_THM[dim]}"
    c="$c,selected-fg:${COCKPIT_THM[fg]}"
    c="$c,selected-bg:${COCKPIT_THM[sel]}"
    c="$c,selected-hl:${COCKPIT_THM[done]}"
    c="$c,header-border:${COCKPIT_THM[surface]}"
    c="$c,footer:${COCKPIT_THM[dim]}"
    c="$c,footer-border:${COCKPIT_THM[surface]}"
    COCKPIT_FZF_COLOR=$c
}

# cockpit_fzf_args [flag...] -- one fzf argument per line, for mapfile.
#
#   mapfile -t THEME < <(cockpit_fzf_args highlight-line "footer=$KEYS")
#   fzf "${THEME[@]}" --prompt=... "${user_options[@]}"
#
# Each argument names an fzf flag WITHOUT its leading dashes, with an
# optional =value. A value is ONE line: a footer
# with a newline arrives as two arguments and fzf rejects the second. Callers
# append a multi-line footer themselves (tmux-review, tmux-open-path do).
# The user's own options go after this call: fzf takes the last occurrence
# of a flag and merges --color per key, so appending is what makes an
# override an override.
cockpit_fzf_args() {
    local f
    _cockpit_fzf_color
    # --gutter: fzf 0.66+ draws ▌ in every row's gutter, which reads as a
    # pointer on all of them.
    printf '%s\n' --ansi --tabstop=1 --pointer=▌ --gutter=' ' --marker=◆ --ellipsis=… \
                  "--color=$COCKPIT_FZF_COLOR"
    for f in "$@"; do printf -- '--%s\n' "$f"; done
}
