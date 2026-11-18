# Agent rules for tmux-cockpit

## Never experiment on the live tmux server

Do not install bindings, key-logging shims, hooks, or options on the user's
running tmux server (the attached working session). Rebinding something like
`MouseDown1Status` on the live server hijacks the user's real clicks while they
work, and logging shims flood their session.

Run every experiment on a throwaway private server and kill it after. Run the
sketch in one tool-shell call. Each call is a new shell, so `$$` differs and
`$t` is gone. A later call must carry the socket name and the folder over by
value.

```sh
d=$(mktemp -d); t=$(mktemp -d)
env -u TMUX -u TMUX_PANE -u TMUX_COCKPIT_DIR TMUX_TMPDIR="$t" \
    tmux -L probe$$ -f /dev/null new-session -d -c "$d" ...
# ... then:
env -u TMUX -u TMUX_PANE -u TMUX_COCKPIT_DIR TMUX_TMPDIR="$t" \
    tmux -L probe$$ kill-server; rm -r "$d" "$t"
```

Start the server without the live session's variables (#1072). A tool shell
inherits `TMUX`, `TMUX_PANE` and `TMUX_COCKPIT_DIR` from the owner's pane. With
`TMUX` set, a bare `tmux` talks to the live server. With `TMUX_COCKPIT_DIR` set,
a server whose plugin load stops early keeps the live plugin's folder, and a
probe then runs the live scripts.

Start every tool-shell command line with
`unset TMUX TMUX_PANE TMUX_COCKPIT_DIR`, before anything in it can call tmux.
The tool shell keeps no state, so an unset in an earlier call is gone. The
unset alone does not stop a bare `tmux`. It then uses `/tmp/tmux-<uid>/default`,
the live server's socket. So the same line also exports `TMUX_TMPDIR` set to an
existing empty folder of your own, where a bare `tmux` finds no server (#1091).
A probe that starts an agent or loads the plugin also puts a scratch `HOME` in
front of that command, never exported. With `TMUX_COCKPIT_DIR` unset, the hooks
in `examples/claude-hooks.json` fall back to `$HOME/.tmux/plugins/tmux-cockpit`,
which under the real `HOME` is the live plugin.

Never `eval`, `source` or `sed`-extract any part of `cockpit.tmux` or `scripts/`
in a tool shell. Check plugin lines only by loading a whole copy on a private
`-L` server. #1091 records a lane that ran a `sed` range of `cockpit.tmux`
through `eval` with `TMUX` set, and it rebound the owner's keys on the live
server.

`TMUX_TMPDIR` must name a folder that already exists. tmux keeps its sockets in
`tmux-<uid>` under it. From tmux 3.2 to 3.8 it skips a folder that is not there
and uses `/tmp/tmux-<uid>`, the live server's folder. Give the same
`TMUX_TMPDIR` to every call to that server, the `kill-server` too. Without it,
`-L probe$$` looks in `/tmp/tmux-<uid>` and misses the server or finds another
one.

Put the unsets and the folder in one `env` call. An inner `env -u VAR` undoes an
outer `env VAR=x`, since the inner call runs last. `tests/run.sh` already does
all of this for its own servers.

Start the server and every `new-window` or `split-window` with `-c` set to a
temp directory. Without it, the panes inherit your shell's cwd, which is often
the checkout. A probe of a broken command then writes there: an unquoted
mutation once ran `touch` on a wrapper's argv and left `panel` and `%0` in the
main checkout.

The test suite already follows this: `tests/run.sh` shims `tmux` onto a private
`-L` socket, "never the user's". Add new tests there, on a private socket
registered in `SOCKS`, not against the live server.

Reading live state is fine as long as it is read-only: `list-keys`,
`show-options`, `capture-pane`, `display -p`. From a tool shell, name the live
socket with `-S /tmp/tmux-<uid>/default` for those read-only commands only.
With `TMUX` unset and the private `TMUX_TMPDIR` above, a bare `tmux` finds no
server, and `-L` names a socket in the private folder. `-S` skips that folder,
so a write sent with it reaches the live server too. Never write to the live
server. If you already changed it, save
`tmux -S /tmp/tmux-<uid>/default list-keys -T root` first and restore the stock
bindings immediately afterward.

Branch work goes in a `~/.cache/cockpit-wt/<branch>` worktree; the main checkout
stays on `master`.

## Touch only your own worktree and branch

Several Claude sessions work on this repo at once, each with its own lanes.
The owner of a branch is the session or lane named on its PR: the PR body and
comments open with that name. A worktree belongs to whoever made it for that
branch. `git worktree list` shows where each one is.

- Never `reset`, `merge`, `rebase`, commit or push in another owner's worktree,
  or push to another owner's branch. Unpushed commits live only in the owner's
  worktree, and a `git reset` there loses them.
- If the owner needs master merged in or a fix made, message that session.
- Reading is always allowed: fetching a PR's head, checking it out detached in a
  worktree you create, building and testing it there, and reviewing it.
- If the owner is gone (no running session or lane answers), you may take the
  branch over. Say so in a PR comment first, then work in a new worktree you
  create from the remote branch. From then on you are its owner.

Every session shares the main checkout's `.omc/notepad.md` and
`.omc/project-memory.json`. Start each entry you write there with your session
name. After a `/clear` or a compaction, an entry without your session name is
not your state, even if it looks like it. One session once took over another's
PRs by reading those files as its own.

## Never signal a pid the hook has not verified

A pid published in a tmux option (`@cockpit_tui_pid`) outlives the process that
wrote it -- SIGKILL, a crash and a power cut unpublish nothing -- and the kernel
hands that number to something else. `SIGUSR1`'s default action is *terminate*,
so an unverified `kill` is a way to shoot a random process of the user's. It has
already happened once, to a probe on the live server.

That pid option is gone: since #224 the board is woken through `tmux wait-for`
and no hook signals a pid (`tui/src/push.rs`, docs/tui.md "Push wakeups"). The
rule stays for any pid option added later. Check the name first, in the shape
`scripts/agent-state` used:

```sh
if [ -r "/proc/$pid/comm" ]; then
    read -r comm 2>/dev/null < "/proc/$pid/comm"
elif [ ! -d /proc/self ]; then
    comm=$(ps -o comm= -p "$pid" 2>/dev/null)   # macOS: ps prints a path
fi
[ "${comm##*/}" = cockpit-tui ] && kill -USR1 "$pid"
```

Test `-r` first rather than falling back from a failed `read`: the pid that needs
this guard is exactly the one that is absent from `/proc`, so `read || ps` forks
`ps` on every call. And `2>/dev/null` goes *before* the input redirect, or bash
reports the failure before that redirect applies.

The publisher's half of the bargain is to unpublish on **every** exit path,
signal handlers included.

## When we are stuck, change direction instead of patching more

Digging deeper into an approach that is not working costs more each round. The
job is to notice it and step back.

You are stuck when the same bug survives two or three fix attempts, each fix
needs another patch on top, a test stays flaky, or the behaviour still differs
from what it should be.

Then:

1. Stop and write down what we are trying to do, what we tried, and why each try
   failed.
2. Research before the next attempt. Use web search and prefer primary sources:
   docs, issue trackers, the source of well-known projects. Cite a source for
   each claim. Look at how others solve the same problem, which alternatives or
   better ways exist, and which pitfalls they hit and why.
3. Compare that with our approach. Come back with the options, a recommendation
   and the cost of switching. For a big switch, wait for the owner's OK before
   changing anything.

Say plainly what is uncertain. A guess written as a finding sends the next
attempt the wrong way.
