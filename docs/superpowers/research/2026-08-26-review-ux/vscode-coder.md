# `^E` in VS Code through Coder (2026-08-26, measured on tzachis/node-dev)

Two facts that break the first wrapper (`vscode-editor.md`, superseded there):

1. A container workspace has no systemd user session: `/run/user/1000` is
   absent, `XDG_RUNTIME_DIR` empty, so VS Code puts its sockets in
   `os.tmpdir()`: `/tmp/vscode-ipc-*.sock` (7 present, 5 listening).
2. Current VS Code installs the server as
   `~/.vscode-server/cli/servers/Stable-<commit>/server/bin/remote-cli/code`
   (the 365-byte shim, VERSION=1.134.0); `~/.vscode-server/bin/<commit>/` is
   the old layout and matches nothing.

Smaller trap: newest-by-mtime server dir is not the one holding the live
sockets (1.134 on disk, a 1.131 window attached for three days). A
version-mismatched CLI answered `--status` with rc 0 and no output; a real
`--wait` open under mismatch is untested.

## Scenarios

| scenario | `^E` in VS Code | `VISUAL` | must be true |
|---|---|---|---|
| plain ssh, no window | no | — | only rmate (dead) |
| Remote-SSH, window open | yes | wrapper | a listening ipc socket + the shim by absolute path |
| Coder via the VS Code extension | yes | wrapper | it IS Remote-SSH: extension pack on `ms-vscode-remote.remote-ssh`, writes `Host coder-vscode.<deployment>--*` with `ProxyCommand coder … vscodessh …` |
| Coder via terminal (`coder ssh ws`) | only while a window is attached to that workspace | wrapper | same box, same `/tmp` |
| Coder code-server (browser) | in principle, untested | `/tmp/code-server/lib/code-server-<v>/lib/vscode/bin/remote-cli/code-server --wait` | its `server-cli.js` has `--wait` and reads `VSCODE_IPC_HOOK_CLI` |

Proof the mechanism holds under Coder: an integrated-terminal process in the
workspace carries `VSCODE_IPC_HOOK_CLI=/tmp/vscode-ipc-….sock`,
`TERM_PROGRAM=vscode`, `PATH` led by `…/server/bin/remote-cli`.

## How a tmux server gets the variable

- Started from a VS Code integrated terminal: inherits socket and PATH;
  `tmux setenv -g VSCODE_IPC_HOOK_CLI "$VSCODE_IPC_HOOK_CLI"; tmux setenv -g VISUAL "$(command -v code) --wait"`.
  Goes stale on reconnect (a new socket per connection).
- Started by `coder_script`/`startup_script`, by `coder ssh`, or outliving a
  reconnect: no variable (agent env is `CODER_*` + `VSCODE_PROXY_URI`). A
  template cannot inject a per-connection socket path. Probing is the answer.

## Wrapper

The final, merged wrapper (cli resolved first so a missing server falls back
instead of `exec ""`, both socket dirs, `nc -zU` or `socat` probe) is in
`vscode-test.md`; copy it from there.

Ceiling: a closed window's socket can keep listening for the server's 8 h
reconnection grace, so "newest listening" is not proof of a live window and
`--wait` can hang until the popup is killed. `nc`/`socat` may be absent from
a workspace image.

## Coder do / don't

- Do treat the extension as Remote-SSH (coder.com/docs/user-guides/workspace-access/vscode).
- Do glob `${XDG_RUNTIME_DIR:-/tmp}`.
- Do keep the wrapper under `$HOME` (persistent volume); `/` and `/tmp` are overlay, rebuilt every start.
- Don't point `VISUAL` at the `code-server` launcher on the agent PATH (`/tmp/coder-script-data/bin/code-server`): it starts a server, it does not open a file.
- Don't expect `coder open vscode` to be `^E`: a `vscode://` URI to the desktop app, a directory, returns at once.
- Don't rely on Coder Desktop / `coder port-forward` / `coder ssh -R`: ports and files, not editor RPC.
- Don't share a workspace and expect one socket to mean one person.
- Don't assume the workspace user is unprivileged (here `coder` has passwordless sudo, template policy).

## README, "Coder" sub-heading

A Coder workspace opened from VS Code is Remote-SSH underneath, so the
snippet above applies with two changes: the IPC sockets are in
`${XDG_RUNTIME_DIR:-/tmp}` (a workspace usually has no `/run/user/$UID`), and
the shim lives at `~/.vscode-server/cli/servers/*/server/bin/remote-cli/code`.
Put the wrapper in `$HOME`, which persists; `/tmp` does not survive a restart.
