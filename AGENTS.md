# Agent rules for tmux-cockpit

## Never experiment on the live tmux server

Do not install bindings, key-logging shims, hooks, or options on the user's
running tmux server (the attached working session). Rebinding something like
`MouseDown1Status` on the live server hijacks the user's real clicks while they
work, and logging shims flood their session.

Run every experiment on a throwaway private server and kill it after:

```sh
d=$(mktemp -d)
tmux -L probe$$ -f /dev/null new-session -d -c "$d" ...
# ... then:
tmux -L probe$$ kill-server; rm -r "$d"
```

Start the server and every `new-window` or `split-window` with `-c` set to a
temp directory. Without it, the panes inherit your shell's cwd, which is often
the checkout. A probe of a broken command then writes there: an unquoted
mutation once ran `touch` on a wrapper's argv and left `panel` and `%0` in the
main checkout.

The test suite already follows this: `tests/run.sh` shims `tmux` onto a private
`-L` socket, "never the user's". Add new tests there, on a private socket
registered in `SOCKS`, not against the live server.

Reading live state is fine as long as it is read-only: `list-keys`,
`show-options`, `capture-pane`, `display -p`. Never write to the live server. If
you already changed it, save `tmux list-keys -T root` first and restore the
stock bindings immediately afterward.

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
