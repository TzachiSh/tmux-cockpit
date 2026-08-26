# Security of VS Code as `^E`'s editor over Remote-SSH (2026-08-26)

Headline: the socket-probing wrapper is not the risk. The exposure that exists
is a VS Code default: the remote server listens on a loopback TCP port, so on
a shared host another local user can attach to your session. Client setting
`"remote.SSH.remoteServerListenOnSocket": true` (needs sshd
`AllowStreamLocalForwarding yes`) closes it. vscode#109475, docs/remote/ssh.

## What runs where

| component | host | user | listens on | reachable by |
|---|---|---|---|---|
| `~/.vscode-server/bin/<commit>/` node server + extension host | remote | the ssh login user (docs/remote/faq) | TCP 127.0.0.1 by default; unix socket with the setting above | any local user (TCP) / same uid (socket) |
| workspace extensions | remote | same user | — | "full access to the source code, remote filesystem, and remote APIs" (api/advanced-topics/remote-extensions) |
| `$XDG_RUNTIME_DIR/vscode-ipc-<uuid>.sock` + `bin/remote-cli/code` | remote | same user | 0700 runtime dir (basedir spec) | same uid; never cleaned up (vscode-remote-release#6362, #7926) |
| `code tunnel` | remote | same user | outbound to `global.rel.tunnels.api.visualstudio.com` | anyone on the same GitHub/Microsoft account, anywhere (docs/remote/tunnels) |

The server is downloaded and auto-updated on every connect from
`update.code.visualstudio.com` / `vscode.download.prss.microsoft.com`
(docs/remote/vscode-server). Removal: "Remote-SSH: Uninstall VS Code Server
from Host…" or delete `~/.vscode-server`.

remote-cli through the ipc socket can open files (`--wait`, `--goto`) and
`--install-extension` on the remote extension host (v1_53 notes): a same-uid
process can already do all of that by other means; another user cannot.

## Do / don't (worst first)

1. Do set `remote.SSH.remoteServerListenOnSocket: true` on any host with other users (vscode#109475).
2. Don't run vscode-server as root: it runs as whoever you ssh in as (docs/remote/faq). No `sudo` near the wrapper or the popup.
3. Don't enable it on shared or production hosts by default; it auto-downloads and auto-updates from Microsoft on every connect, and is "not intended or licensed for use by other clients" (docs/remote/vscode-server).
4. Don't pair it with `ForwardAgent yes` on a host you don't control (ssh_config: anyone who can bypass file permissions there can use your agent).
5. Do treat the extension list as the code-execution channel: `remote.SSH.defaultExtensions` and Settings Sync push extensions to every remote (docs/remote/ssh).
6. Don't expect `AllowTcpForwarding no` to be security (sshd_config: users with a shell can install their own forwarders); Remote-SSH fails without it (vscode-remote-release#92).
7. Do use `DisableForwarding` + `Match`/`AllowUsers` when you mean to forbid it; `ForceCommand` does not limit forwarding (sshd_config).
8. Don't allow `code tunnel` on a managed network unless an outbound Microsoft relay with a personal GitHub/Microsoft identity is acceptable (docs/remote/tunnels).
9. Do set `remote.autoForwardPorts: false` on sensitive hosts (VS Code scans processes and forwards what it finds; v1_54 notes).
10. Do accept Workspace Trust is a client prompt, never a server control.
11. Don't treat `remote.SSH.useLocalServer`/`remotePlatform` as security knobs.
12. Do clean stale `vscode-ipc-*.sock` and old `~/.vscode-server/bin/*` now and then.

Enforceable server-side: who may log in, whether forwarding exists at all
(`DisableForwarding` kills Remote-SSH), egress to the download/tunnel
endpoints. Only policy: socket vs TCP, the extension list, Workspace Trust.

## Coder

`coder config-ssh` writes a ProxyCommand (`--no-wildcard` for per-workspace
`Host` entries visible to VS Code); transport is a WireGuard tunnel to the
workspace agent; the `coder.coder-remote` extension builds on Remote-SSH, so
the same vscode-server + ipc socket model, and the same wrapper, apply inside
the workspace. Workspaces are usually single-user containers (item 1 moot)
that often run as root (item 2 bites). Keep the Coder session token out of
anything the plugin reads.

## Smaller-footprint alternatives

SSHFS/rclone + local VS Code: nothing installed remotely, but no remote
terminal/git/extensions and no way for the remote to open the file for `^E`.
`rmate`: unmaintained. code-server and `code serve-web`: a web server on top
of the same binary, strictly more surface. None beats Remote-SSH for a
blocking `^E`.

## Verdict on the wrapper: safe enough

Keep it in `~/bin`, user-owned, 0755; never under `sudo` (a root-run wrapper
resolving `~/.vscode-server/bin/*` is the one real escalation shape); probe
only `/run/user/$(id -u)/`, never a shared `/tmp` glob; keep the terminal
fallback. Probing for a *listening* socket is safer than exporting a fixed
`VSCODE_IPC_HOOK_CLI` (stale sockets are the documented failure). Caveat, not
security: "newest listening socket" may be a different window than the one
you are looking at.

## README "Security" paragraph

`VISUAL='code --wait'` and the Remote-SSH wrapper run entirely as you: they
use the VS Code CLI socket in your own `$XDG_RUNTIME_DIR`, which no other
user can open, and give a same-user process nothing it did not already have.
Never run the wrapper or the popup under `sudo`. VS Code's remote server
listens on a loopback TCP port by default, so on a shared host set
`"remote.SSH.remoteServerListenOnSocket": true`, or another local user can
attach to your editor session. Remote-SSH installs and auto-updates
`~/.vscode-server` on that host; some admins do not allow that, ask first.

Sources: code.visualstudio.com/docs/remote/{ssh,faq,vscode-server,troubleshooting,tunnels};
api/advanced-topics/remote-extensions; docs/configure/command-line;
vscode-docs remote-release-notes v1_53, v1_54; microsoft/vscode#109475,
#157275, #308612; vscode-remote-release#92, #6362, #7926;
man.openbsd.org/{sshd_config,ssh_config}; freedesktop basedir spec;
coder.com/docs/user-guides/workspace-access/vscode; github.com/coder/vscode-coder.
