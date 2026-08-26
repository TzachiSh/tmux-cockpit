# `^E` in VS Code, local and over Remote-SSH (2026-08-26)

Measured on the owner's box: SSH'd in (`SSH_CONNECTION` in the tmux server
env), `code` = desktop VS Code 1.134 on the remote with `DISPLAY=:0`, no
`~/.vscode-server`, no `/run/user/1000/vscode-ipc-*.sock`. So `code --wait`
from the popup opens on the remote's own monitor today.

## What works

- **Local GUI.** `tmux set-environment -g VISUAL 'code --wait'`. `--wait` blocks
  until the tab closes, cold start included (code.visualstudio.com/docs/configure/command-line).
  The `code` wrapper execs Electron, it does not fork, so the popup blocks.
  **Not `@cockpit_editor`**: `tmux-open-path:553` treats any `@cockpit_editor`
  as a terminal editor (`new-window "$editor +LINE file"`) and never reaches
  its `code --goto` branch, so click-to-open would hang on a GUI window.
- **Remote-SSH, window open.** The shim `~/.vscode-server/bin/<commit>/bin/remote-cli/code`
  dispatches on `$VSCODE_IPC_HOOK_CLI` (vscode-remote-release#6289, #6579) and
  `--wait` works through it (docs/remote/troubleshooting). The tmux server
  lacks the variable, or holds a dead socket: sockets `/run/user/$UID/vscode-ipc-<uuid>.sock`
  are one per connection and never cleaned up (vscode-remote-release#6362,
  vscode#157275). User-owned wrapper, newest socket that listens:

      (wrapper superseded: see vscode-coder.md — sockets live in
      ${XDG_RUNTIME_DIR:-/tmp}, the shim under ~/.vscode-server/cli/servers/*/
      server/bin/remote-cli/code; the corrected script is there)

  (listen probe from chvolkmann/code-connect; `ls -t | head -1` from #6362.)
- **Plain ssh, no VS Code window.** Only `rmate`/Remote VSCode (port 52698 via
  `ssh -R`), last commit 2022-12-07, never adopted by Microsoft (#4037). Not worth building on.

## What cannot work

- `code --remote ssh-remote+host path` runs on the laptop only.
- OSC 52 is clipboard-only; nothing to block on.
- `code tunnel` makes the remote reachable (back to the Remote-SSH case), it does not push to a desktop window.
- `VSCODE_IPC_HOOK_CLI` alone in the tmux env: the shim needs remote-cli on PATH too, else it launches the local Electron GUI.

## Do / don't

- Do put a GUI editor in `VISUAL` (server env), keep `@cockpit_editor` for terminal editors.
- Do probe the socket dir for a listening socket; do call the remote-cli shim by absolute path; do keep a terminal fallback.
- Don't add `-r` to `--wait` (vscode#269502: a new window is "too late to track").
- Don't set a custom `TMPDIR`/`TEMP` in the server env (vscode#225978: `--wait` returns at once).
- Don't detect via `TERM_PROGRAM`, `VSCODE_INJECTION` or `command -v code`; none means "a window is reachable" inside a popup.

## Non-blocking `^E`

Does not fit: `edit_comment` reads and deletes the temp file on the line after the editor returns. Non-blocking would need a durable file and a later sweep, a different store lifecycle.

## Recommendation

No plugin code. README: `@cockpit_editor` is for terminal editors; `tmux set-environment -g VISUAL 'code --wait'` for VS Code; the wrapper above as a "VS Code over Remote-SSH" snippet.
