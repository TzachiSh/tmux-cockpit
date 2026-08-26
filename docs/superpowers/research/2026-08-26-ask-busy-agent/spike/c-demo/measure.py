#!/usr/bin/env python3
"""Latency of a state flip -> repainted row, per wake mode.

usr1: kill -USR1 <@cockpit_fleetc_pid>   winch: resize-window   poll: 1 s floor
t0 is taken after the tmux option is already set, at the instant of the trigger,
so the number is trigger->visible, not tmux's own set-option cost.
"""
import os, subprocess, sys, time, statistics

D = os.path.dirname(os.path.abspath(__file__))
TMUX = ["tmux", "-L", "cdemo", "-f", "/dev/null"]
LOG = os.environ["COCKPIT_LOG"]
PANE = "%2"
WIDTHS = (150, 160)


def tm(*a):
    return subprocess.run(TMUX + list(a), capture_output=True, text=True).stdout.strip()


def rows_lines():
    """(ts, sum) for every 'rows sum=' line in the log."""
    out = []
    with open(LOG) as f:
        for l in f:
            p = l.split()
            if len(p) > 1 and p[1] == "rows":
                out.append((float(p[0]), p[2]))
    return out


def wait_new(after, oldsum, timeout=6.0):
    end = time.time() + timeout
    while time.time() < end:
        for ts, s in reversed(rows_lines()):
            if ts >= after and s != oldsum:
                return ts
        time.sleep(0.002)
    return None


def sample(mode, state, i):
    base = rows_lines()
    oldsum = base[-1][1] if base else ""
    tm("set-option", "-p", "-t", PANE, "@panestate", state)
    if state == "needs-input":
        tm("set-option", "-p", "-t", PANE, "@panestate_msg", "permission: Bash(rm -rf build/)")
    t0 = time.time()
    if mode == "usr1":
        os.kill(int(tm("show-option", "-gqv", "@cockpit_fleetc_pid")), 10)
    elif mode == "winch":
        # alternate two widths so every sample is a real size change
        tm("resize-window", "-t", "cdemo:fleet", "-x", str(WIDTHS[i % 2]), "-y", "40")
    t1 = wait_new(t0, oldsum)
    return None if t1 is None else (t1 - t0) * 1000


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    print(f"{'mode':6} {'n':>3} {'min':>8} {'median':>8} {'max':>8}   (ms)")
    for mode in ("usr1", "winch", "poll"):
        v = []
        for i in range(n):
            r = sample(mode, "needs-input" if i % 2 == 0 else "running", i)
            if r is not None:
                v.append(r)
            time.sleep(1.4)  # let the fallback tick settle between samples
        if v:
            print(f"{mode:6} {len(v):3d} {min(v):8.1f} {statistics.median(v):8.1f} {max(v):8.1f}")
        else:
            print(f"{mode:6} {0:3d}   no samples")


main()
