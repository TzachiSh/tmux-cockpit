#!/usr/bin/env bash
# One fzf theme for every cockpit picker. Sourced, never run. Bash 4+.
#
# No `bg`, `list-bg` or `input-bg` anywhere below: the popup's background
# belongs to the user's popup-style, the same rule cockpit.tmux states for the
# border. `16` stays the base so an unset key degrades to the terminal's own.

# --footer, the newest option any picker asks for. Everything else in the
# styled set landed earlier, so one floor covers the whole list.
COCKPIT_FZF_MIN=0063000

# shellcheck source=vnum.sh
. "${BASH_SOURCE[0]%/*}/vnum.sh"

cockpit_fzf_tier() {          # -> plain | styled, and caches it in COCKPIT_FZF_TIER
    # A case, not a -z test: a caller may pre-set the variable, and anything
    # that is not one of the two words is junk to be probed over, not honoured.
    case "${COCKPIT_FZF_TIER:-}" in plain|styled) ;; *)
        local v
        v=$(vnum "$(fzf --version 2>/dev/null)" 2>/dev/null)
        if [ "${v:-0}" -ge "$COCKPIT_FZF_MIN" ] 2>/dev/null; then
            COCKPIT_FZF_TIER=styled
        else
            COCKPIT_FZF_TIER=plain
        fi
    ;; esac
    printf '%s\n' "$COCKPIT_FZF_TIER"
}

# -g, not a bare declare: a caller that sources this lib from inside one of its
# own functions (tmux-open-path's op_theme does) would otherwise get a LOCAL
# cache that dies with that call.
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

# Sets COCKPIT_FZF_COLOR rather than printing it: called through $(...) the
# two caches above would fill in a subshell and die with it, which is three
# fzf --version forks per picker open instead of one.
_cockpit_fzf_color() {
    _cockpit_fzf_thm
    cockpit_fzf_tier >/dev/null
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
    if [ "$COCKPIT_FZF_TIER" = styled ]; then
        c="$c,selected-fg:${COCKPIT_THM[fg]}"
        c="$c,selected-bg:${COCKPIT_THM[surface_2]}"
        c="$c,selected-hl:${COCKPIT_THM[green]}"
        c="$c,header-border:${COCKPIT_THM[surface_1]}"
        c="$c,footer:${COCKPIT_THM[overlay_2]}"
        c="$c,footer-border:${COCKPIT_THM[surface_1]}"
    fi
    COCKPIT_FZF_COLOR=$c
}

# cockpit_fzf_args [flag...] -- one fzf argument per line, for mapfile.
#
#   COCKPIT_FZF_TIER=$(cockpit_fzf_tier)     # warm the memo in THIS shell
#   mapfile -t THEME < <(cockpit_fzf_args highlight-line "footer=$KEYS")
#   fzf "${THEME[@]}" --prompt=... "${user_options[@]}"
#
# The warm-up line is part of the convention: the process substitution is a
# subshell, and a memo filled there dies with it, one fzf --version per call.
# Each argument names a styled-tier flag WITHOUT its leading dashes, with an
# optional =value; on the plain tier they are dropped and the caller puts its
# key hints in the last --header line instead. A value is ONE line: a footer
# with a newline arrives as two arguments and fzf rejects the second. Callers
# append a multi-line footer themselves (tmux-review, tmux-open-path do).
# The user's own options go after this call: fzf takes the last occurrence
# of a flag and merges --color per key, so appending is what makes an
# override an override.
cockpit_fzf_args() {
    local f
    _cockpit_fzf_color                    # also fills COCKPIT_FZF_TIER
    printf '%s\n' --ansi --tabstop=1 --pointer=▌ --marker=◆ --ellipsis=… \
                  "--color=$COCKPIT_FZF_COLOR"
    [ "$COCKPIT_FZF_TIER" = styled ] || return 0
    for f in "$@"; do printf -- '--%s\n' "$f"; done
}
