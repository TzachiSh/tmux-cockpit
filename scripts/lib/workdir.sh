#!/usr/bin/env bash
# Sourced, never run. Bash 4+. The tree a pane works in (#470): the dir the
# board's row shows, so prefix r, the board's r and tmux-branch's goto name the
# same one. cockpit-tui owns the rule (`cockpit-tui workdir %N`, the board's
# Pane.dir: @cockpit_dir, else the session transcript's newest cwd, else
# pane_current_path); this asks it, and falls back to the rule without the
# transcript whenever the binary cannot answer.

# shellcheck source=pane.sh
. "${BASH_SOURCE[0]%/*}/pane.sh" || return 1
# shellcheck source=tui.sh
. "${BASH_SOURCE[0]%/*}/tui.sh" || return 1

# workdir takes about 12 ms; the bound is a hang guard. timeout is GNU coreutils;
# a box without it calls unbounded, as agent-state does.
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

# The binary's answer, or nothing: for a missing binary, a stale one (no
# `workdir`: exit 2), a pane it does not count as an agent's (exit 1), or any
# other non-zero exit. workdir_pick checks what it prints.
workdir_ask() {               # <pane> -> its dir
    local out
    [ -x "$COCKPIT_TUI_BIN" ] || return 0
    out=$(${WORKDIR_TO[@]+"${WORKDIR_TO[@]}"} "$COCKPIT_TUI_BIN" workdir "$1" \
        </dev/null 2>/dev/null) || return 0
    printf '%s' "$out"
}

# What tells an agent pane, read in one format: the agent glob's verdict, @panestate
# and the command name. Then the pane's @cockpit_dir and cwd.
_wd=$'\037'
WORKDIR_FMT="#{pane_id}$_wd#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},1,0}"
WORKDIR_FMT+="$_wd#{@panestate}$_wd#{pane_current_command}"
WORKDIR_FMT+="$_wd#{@cockpit_dir}$_wd#{pane_current_path}"
unset _wd

# The board's live test (tui/src/data.rs list_fleet, agentish): the glob, a
# version-shaped name (a live Claude renames itself to one like 2.1.246), or a
# @panestate the hooks set. The board's walk of the process tree is left out: it
# forks ps.
workdir_agent() {             # <glob 1|0> <@panestate> <pane_current_command>
    local b=${3##*/} s
    if [ "$1" = 1 ] || [ -n "$2" ]; then return 0; fi
    b=${b#"${b%%[!-]*}"}
    [[ $b =~ ^[0-9][0-9._]*$ ]] || return 1
    s=${b//[0-9]/}
    [ "${#s}" -ge 2 ]
}

# Whether the binary's answer can differ from the rule without it: only for an
# agent pane (it alone has a transcript) whose @cockpit_dir does not hold (one
# that holds is the binary's first pick too). Every other pane forks nothing:
# goto walks every shell pane, and a call each costs more than the walk.
workdir_need() {              # <agent 1|0> <@cockpit_dir>
    [ "$1" = 1 ] && ! workdir_ok "$2"
}

# One WORKDIR_FMT row into WD_ID, WD_AGENT (1|0), WD_OPT and WD_CWD.
workdir_row() {               # <row>
    local r=$1 us=$'\037' glob st cmd
    WD_ID=${r%%"$us"*}; r=${r#*"$us"}
    glob=${r%%"$us"*}; r=${r#*"$us"}
    st=${r%%"$us"*}; r=${r#*"$us"}
    cmd=${r%%"$us"*}; r=${r#*"$us"}
    WD_OPT=${r%%"$us"*}; WD_CWD=${r#*"$us"}
    if workdir_agent "$glob" "$st" "$cmd"; then WD_AGENT=1; else WD_AGENT=0; fi
}

pane_workdir() {              # <pane> -> a directory; nothing when the pane is gone
    local out dir=''
    is_pane "${1-}" || return 0
    # display-message can exit 0 and expand to nothing for a missing pane: the id tells
    out=$(tmux display-message -p -t "$1" "$WORKDIR_FMT" 2>/dev/null)
    workdir_row "$out"
    [ -n "$WD_ID" ] || return 0
    if workdir_need "$WD_AGENT" "$WD_OPT"; then
        dir=$(workdir_ask "$1")
    fi
    workdir_pick "$dir" "$WD_OPT" "$WD_CWD"; printf '%s\n' "$WORKDIR"
}
