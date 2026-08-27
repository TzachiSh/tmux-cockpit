# Agent rules for tmux-cockpit

## Never experiment on the live tmux server

Do not install bindings, key-logging shims, hooks, or options on the user's
running tmux server (the attached working session). Rebinding something like
`MouseDown1Status` on the live server hijacks the user's real clicks while they
work, and logging shims flood their session.

Run every experiment on a throwaway private server and kill it after:

```sh
tmux -L probe$$ -f /dev/null new-session -d ...   # ... then: tmux -L probe$$ kill-server
```

The test suite already follows this: `tests/run.sh` shims `tmux` onto a private
`-L` socket, "never the user's". Add new tests there, on a private socket
registered in `SOCKS`, not against the live server.

Reading live state is fine as long as it is read-only: `list-keys`,
`show-options`, `capture-pane`, `display -p`. Never write to the live server. If
you already changed it, save `tmux list-keys -T root` first and restore the
stock bindings immediately afterward.

Branch work goes in a `~/.cache/cockpit-wt/<branch>` worktree; the main checkout
stays on `master`.

## Never signal a pid the hook has not verified

A pid published in a tmux option (`@cockpit_tui_pid`) outlives the process that
wrote it -- SIGKILL, a crash and a power cut unpublish nothing -- and the kernel
hands that number to something else. `SIGUSR1`'s default action is *terminate*,
so an unverified `kill` is a way to shoot a random process of the user's. It has
already happened once, to a probe on the live server.

Check the name first, in the shape `scripts/agent-state` uses:

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

The same holds for any pid option added later. The publisher's half of the
bargain is to unpublish on **every** exit path, signal handlers included
(`tui/src/push.rs`).
