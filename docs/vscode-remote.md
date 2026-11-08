# VS Code, Remote-SSH and Coder

## VS Code over Remote-SSH

`VISUAL='code --wait'` is enough on a local machine, and only there. Over
Remote-SSH `code` is not on the `PATH` tmux was started with, and VS Code hands
the remote a fresh `VSCODE_IPC_HOOK_CLI` socket per connection that the tmux
server, started before that connection or outliving it, never has. Nothing to
install: click-to-open finds the shim itself when `@cockpit_editor` is unset
and there is no `code` on the `PATH`. It takes the newest
shim under `~/.vscode-server` (`cli/servers/*/server/bin/remote-cli/code`, or
`bin/*/bin/remote-cli/code` from an older server) and runs it against each
`vscode-ipc-*.sock` in `$XDG_RUNTIME_DIR`, newest first, until a window takes
the file. The shim is its own probe, so no `nc` or `socat` is needed.

Two caveats. A dead socket refuses in about 150 ms, so the walk stops at the 8
newest rather than spending six seconds on a home full of stale ones, and a
window whose socket is older than those 8 is missed. And a closed window's
socket keeps listening through VS Code's reconnection grace, so answering is
not proof of a live window: the file can go to a window that is no longer
there.

From a plain ssh session, with no VS Code on the far end, nothing can reach
the laptop's editor. Use a terminal editor there.

## Coder

A Coder workspace opened from VS Code is Remote-SSH underneath, and there is
nothing extra to do: the shim sits in the same place, and a workspace that has
no `/run/user/$UID` puts its sockets in `$TMPDIR` or `/tmp`, which is where the
walk looks when `$XDG_RUNTIME_DIR` is unset.

## Security

`VISUAL='code --wait'` and the shim the popup finds for itself run entirely as
you: they use the VS Code CLI socket in your own `$XDG_RUNTIME_DIR`, which no
other user can open, and give a same-user process nothing it did not already
have. `/tmp` is only the fallback for a workspace with neither
`$XDG_RUNTIME_DIR` nor `$TMPDIR` set (a Coder container); where either is set
the walk never looks in `/tmp`, where another user could plant a listening
socket. Never run the popup under `sudo`. VS Code's remote server listens on a
loopback TCP port by default, so on a shared host set
`"remote.SSH.remoteServerListenOnSocket": true`, or another local user can
attach to your editor session. Remote-SSH installs and auto-updates
`~/.vscode-server` on that host; some admins do not allow that, ask first.
