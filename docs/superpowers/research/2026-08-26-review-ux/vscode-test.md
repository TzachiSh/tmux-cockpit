# Hands-on test of `^E` in VS Code (2026-08-26, on this Mint box)

## Remote-SSH end to end: blocked, not tested
`~/.ssh` here has no private key and no `authorized_keys`; `tzachi-pc` in
ssh config is the laptop (192.168.1.224) the owner sshes FROM. No keys were
added, so no `vscode-server` was provisioned; `~/.vscode-server` still does
not exist. The remote leg below is verified against a mocked
`~/.vscode-server` tree and real AF_UNIX sockets only.

## Verified for real
- Naive `VISUAL='code --wait'` from a clean tmux server (`env -u
  VSCODE_IPC_HOOK_CLI`, through `tmux-review --edit`): a tab opened in 3 s in
  the already-running LOCAL window on `:0`; `--wait` blocked; after typing,
  `ctrl+s`, `ctrl+w` it returned rc 0 and the text landed in the store.
  Cold start and the `TMPDIR` trap (vscode#225978) untested.
- Wrapper probe logic: no socket → fallback; bound-but-not-listening socket
  (the #6362 stale case) skipped; a regular file named `*.sock` skipped; a live
  listener that is the OLDEST file with two newer stale ones → picked. From a
  clean tmux server through `tmux-review --edit` both paths write the store.
- `tmux-open-path` consults `$VISUAL`/`$EDITOR` only when `code` is not
  installed, so `VISUAL='code --wait'` does not disturb click-to-open;
  `@cockpit_editor` non-empty always takes the terminal-editor branch.

## Defects found in the first wrapper (now fixed in vscode-coder.md)
1. Old layout only (`~/.vscode-server/bin/*/…`): with just the current
   `cli/servers/*/server/bin/remote-cli/code` present it ran `exec ""`,
   rc 126, and never reached the fallback: `^E` silently did nothing.
2. Socket dir hardcoded to `/run/user/$(id -u)`; Coder puts them in `/tmp`.
3. `socat` was a hard dependency with a silent failure mode.

## Final wrapper (both agents' versions merged; the one Task 8 copies)

    #!/bin/sh
    # ~/bin/cockpit-code — VISUAL target for ^E over Remote-SSH / Coder.
    cli=$(ls -t "$HOME"/.vscode-server/cli/servers/*/server/bin/remote-cli/code \
                 "$HOME"/.vscode-server/bin/*/bin/remote-cli/code 2>/dev/null | head -1)
    if [ -x "$cli" ]; then
        for s in $(ls -t "${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}"/vscode-ipc-*.sock 2>/dev/null); do
            nc -zU "$s" 2>/dev/null || socat -u OPEN:/dev/null "UNIX-CONNECT:$s" 2>/dev/null || continue
            VSCODE_IPC_HOOK_CLI=$s exec "$cli" --wait "$@"
        done
    fi
    exec "${FALLBACK_EDITOR:-vi}" "$@"

Needs `nc` (OpenBSD netcat, `-U`) or `socat`; with neither, every socket is
skipped and it falls back to the terminal editor. Verified: new layout, old
layout, socket in `$XDG_RUNTIME_DIR`, no `.vscode-server` → fallback, no
socket → fallback. The shipped wrapper reaches `$TMPDIR` only when
`$XDG_RUNTIME_DIR` is unset; the `$TMPDIR` case was verified against the
earlier two-glob version, which probed both directories.

## Left as found
Test server killed, fakes removed, `~/.vscode-server` absent, no repo edits.
A `localhost` line may remain in `~/.ssh/known_hosts`. Screenshots:
scratchpad `vscode-tab.png`, `vscode-naive.png`. Observed unrelated:
`/tmp/cockpit-review-edit.JXYEZw` (09:02, content `FROM-COCKPIT-EDITOR`),
possibly a leak from Task 1's rig block; noted for the final review.
