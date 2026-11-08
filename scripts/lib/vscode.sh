#!/usr/bin/env bash
# VS Code's remote CLI shim (Remote-SSH, Coder). Sourced; pure.
# `code` is not on tmux's PATH there: the shim lives under ~/.vscode-server and
# needs VSCODE_IPC_HOOK_CLI naming a live window's socket. A dead one just fails.

# HOME can be unset under a systemd-launched tmux, and callers use set -u.
vscode_shim() {               # -> newest shim path, or 1
    local s h=${HOME:-/nonexistent}
    # Current and older server layouts; *.staging/ is a half-finished download.
    s=$(ls -t "$h"/.vscode-server/cli/servers/*/server/bin/remote-cli/code \
              "$h"/.vscode-server/bin/*/bin/remote-cli/code 2>/dev/null | grep -v '\.staging/' | head -1)
    [ -x "$s" ] && printf '%s\n' "$s"
}

vscode_open() {               # <code args...> -> 0 when a window took it
    local shim s
    shim=$(vscode_shim) || return 1
    # Newest socket first, capped at 8 since each dead one costs a refusal.
    # fd 3: the socket list stays off stdin, which the shim may read.
    while IFS= read -r s <&3; do
        VSCODE_IPC_HOOK_CLI=$s "$shim" "$@" 2>/dev/null && return 0
    done 3< <(ls -t "${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}"/vscode-ipc-*.sock 2>/dev/null | head -8)
    return 1
}
# Known gap: a closed window's socket can listen for hours; the shim then
# exits 0 there, and the walk stops at a window nobody sees.
