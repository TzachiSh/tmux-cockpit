#!/usr/bin/env bash
# VS Code's remote CLI: the shim Remote-SSH/Coder installs, reachable only
# through a live ipc socket. The shim is its own probe: a dead socket fails
# in ~150 ms (rc 1), a live one answers --goto in ~150 ms (rc 0). Measured
# on a Coder workspace, 2026-08-26.
#
# Sourced, never run; pure -- no tmux, no git, like lib/review-store.sh.
# It exists because `code` is NOT on the PATH tmux was started with on those
# hosts: the CLI lives under ~/.vscode-server and needs VSCODE_IPC_HOOK_CLI
# pointing at the socket of a window that is still attached.

# ${HOME:-...}, never a bare $HOME: both callers run under `set -u`, and a
# run-shell from a stripped environment (or a systemd-launched tmux) has none.
vscode_shim() {               # -> newest shim path, or 1
    local s h=${HOME:-/nonexistent}
    # Both layouts, newest first: cli/servers/Stable-<commit>/ is current,
    # bin/<commit>/ is what older servers left behind. A *.staging/ dir is a
    # half-finished download, not something to hand a file to.
    s=$(ls -t "$h"/.vscode-server/cli/servers/*/server/bin/remote-cli/code \
              "$h"/.vscode-server/bin/*/bin/remote-cli/code 2>/dev/null | grep -v '\.staging/' | head -1)
    [ -x "$s" ] && printf '%s\n' "$s"
}

vscode_open() {               # <code args...> -> 0 when a window took it
    local shim s
    shim=$(vscode_shim) || return 1
    # Newest first: the socket of the window you were last in is the one you
    # want the file in. A stale one just costs the ~150 ms of its own refusal.
    while IFS= read -r s; do
        VSCODE_IPC_HOOK_CLI=$s "$shim" "$@" 2>/dev/null && return 0
    done < <(ls -t "${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}"/vscode-ipc-*.sock 2>/dev/null)
    return 1
}
# ponytail: a closed window's socket can stay listening for VS Code's 8 h grace; --wait then hangs until esc.
