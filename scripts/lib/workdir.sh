#!/usr/bin/env bash
# Sourced, never run. Bash 4+. The tree a pane works in (#470): the dir the
# board's row shows, so prefix r, the board's r and tmux-branch's goto name the
# same one. cockpit-tui owns the rule (fleet::work_dirs: @cockpit_dir, else the
# session transcript's newest cwd, else pane_current_path); this asks it, and
# falls back to the rule before it whenever the binary cannot answer.

# shellcheck source=pane.sh
. "${BASH_SOURCE[0]%/*}/pane.sh" || return 1
# shellcheck source=tui.sh
. "${BASH_SOURCE[0]%/*}/tui.sh" || return 1

# info can wait on `claude agents`, which the binary bounds to 2 s. timeout is
# GNU coreutils; a box without it calls unbounded, as agent-state does.
WORKDIR_TO=()
command -v timeout >/dev/null 2>&1 && WORKDIR_TO=(timeout 3)

# Only an absolute path naming a directory counts: a relative one is read
# against the caller's cwd, and a walk up from it never ends (#474).
workdir_ok() { case ${1-} in /*) [ -d "$1" ] ;; *) return 1 ;; esac; }

# The binary's answer when it passes, else the rule without it: @cockpit_dir when
# it is a directory (a worktree made by hand leaves the cwd in the main
# checkout, #49), else the cwd. Sets WORKDIR: a loop over panes forks nothing.
workdir_pick() {              # <answer> <@cockpit_dir> <pane_current_path>
    if workdir_ok "$1"; then WORKDIR=$1; elif workdir_ok "$2"; then WORKDIR=$2; else WORKDIR=$3; fi
}

# The binary's answer when it gives one: nothing for a missing or stale binary
# (an old one exits 2), a non-zero exit, output jq cannot read, or no jq.
workdir_ask() {               # <cockpit-tui args...> -> its stdout
    local out
    command -v jq >/dev/null 2>&1 && [ -x "$COCKPIT_TUI_BIN" ] || return 0
    out=$(${WORKDIR_TO[@]+"${WORKDIR_TO[@]}"} "$COCKPIT_TUI_BIN" "$@" </dev/null 2>/dev/null) \
        || return 0
    printf '%s' "$out"
}

# The tmux format that tells an agent pane, as the binary's own reads tell it.
WORKDIR_AGENT='#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},1,0}'

# Whether the binary's answer can differ from the rule without it: only for an
# agent pane (it alone has a transcript) whose @cockpit_dir does not hold (one
# that holds is the binary's first pick too). Every other pane forks nothing.
workdir_need() {              # <agent 1|0> <@cockpit_dir>
    [ "$1" = 1 ] && ! workdir_ok "$2"
}

pane_workdir() {              # <pane> -> a directory; nothing when the pane is gone
    local out id agent wt cwd dir='' us=$'\037'
    is_pane "${1-}" || return 0
    # display-message can exit 0 and expand to nothing for a missing pane: the id tells
    out=$(tmux display-message -p -t "$1" \
        "#{pane_id}$us$WORKDIR_AGENT$us#{@cockpit_dir}$us#{pane_current_path}" 2>/dev/null)
    id=${out%%"$us"*}; out=${out#*"$us"}
    [ -n "$id" ] || return 0
    agent=${out%%"$us"*}; out=${out#*"$us"}; wt=${out%%"$us"*}; cwd=${out#*"$us"}
    if workdir_need "$agent" "$wt"; then
        dir=$(workdir_ask info --json "$1" | jq -r '.dir | strings' 2>/dev/null)
    fi
    workdir_pick "$dir" "$wt" "$cwd"; printf '%s\n' "$WORKDIR"
}

# Every agent pane's dir in one call, for a loop over all panes: WORKDIRS[%N].
# The board's --json lists agent panes only; for a shell pane, which has no
# transcript, the binary's rule is the fallback's. workdir_pick checks each one.
declare -A WORKDIRS=()
pane_workdirs() {
    local id dir
    WORKDIRS=()
    # shellcheck disable=SC2034  # read by tmux-branch's goto
    while IFS=$'\t' read -r id dir; do
        is_pane "$id" && WORKDIRS[$id]=$dir
    done < <(workdir_ask --json | jq -r '.[]? | [.id, .dir] | @tsv' 2>/dev/null)
}
