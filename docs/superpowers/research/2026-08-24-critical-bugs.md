# Critical bug hunt — findings (2026-08-24, master @ 851dfdf)

All reproduced on private tmux servers by opus agents. Fix directions verified where noted.

## SECURITY / CORRECTNESS (hunt-correctness)

### C1 [CRITICAL — RCE] arbitrary command execution from a directory name — cockpit.tmux:46-47
`r_path="#($w path #{pane_pid} '#{pane_current_path}' '#{pane_current_command}')"` — tmux expands
`#{pane_current_path}` INSIDE the `#()` job before sh sees it, so a `'` in the cwd closes the
hand-written quote. Fires on every status redraw for anyone using #{agent_where_path}/_branch.
A repo/tarball/npm pkg shipping a dir like `x'$(touch OWNED)'y` runs code when you cd in.
Reach: needs a client attached and the token in status (documented usage). Line 47 identical.
FIX (verified available on 3.7c): tmux's own `#{q:pane_current_path}` escapes sh specials
(`man tmux`: "q: will escape sh(1) special characters") — drop the hand-written quotes entirely.

### C2 [CRITICAL] review comments anchor to the WRONG FILE — tmux-review:142-149
git C-quotes paths with non-ASCII/tab/quote (core.quotePath default on): `--- "a/café.txt"`
doesn't match `/^--- a\//`, so oldfile keeps the PREVIOUS file's name; every row of the odd-named
file is attributed to the file before it (or dropped if it's first). Agent told to edit the wrong
file quoting a line not in it. FIX: run diff with `-c core.quotePath=false` (or unquote), take path
from `diff --git`/`+++` after.

### C3 [CRITICAL] deliver() is-agent guard is a ~50ms TOCTOU — tmux-review:417-481
strace: guard→paste is 46-58ms (a full $SELF --compose re-exec + wc + mktemp + load-buffer sit
between). Kill the agent 30-55ms after ctrl-s → 6/6 review text landed in the SHELL that outlived
the agent. Lands typed-but-unsent under bash bracketed paste (one Enter from running), but it's a
live command line the reviewer never targeted. FIX: make check atomic with the paste via tmux's own
`if-shell -F '#{m:...}' 'paste-buffer ...'` (one command-queue item).

### I1 [IMPORTANT] prefix R never reloads config on a default install — cockpit.tmux:270-277
`#{config_files}` leads with /etc/tmux.conf on any non-`-f` start; `cut -d,-f1` picks it. Binding
becomes `source-file /etc/tmux.conf` (usually absent). Also: multi-config installs skip the 2nd
file; a path with a comma is truncated; the `\; display-message` half ran once at load, not bound
(argv-split trap the file's own :283 comment documents). Tests miss it: fixture uses -f /dev/null
(single-entry). FIX: source EVERY entry (split on comma) in one quoted string.

### I2 [IMPORTANT] --hooks loses a concurrent settings.json write — install.sh:140→239
54ms unlocked read-modify-write. Claude Code rewrites settings.json on every permission approval,
so a --hooks run from a live session drops that write; reported as success. .bak only saves it when
no .bak pre-existed (never overwritten). FIX: O_EXCL lockfile around read→rename, or re-read+remerge
if mtime/size changed.

### I3 [IMPORTANT] stale TMUX_PANE writes state onto an unrelated pane after server restart — agent-state:9-16
kill-server + new server on same socket + a hook process that outlived it (detached/nohup agent) →
env TMUX_PANE=%0 writes @panestate onto whatever %0 is now. Narrow (pane-resident agents die with the
pane). CLEAN (verified): %N is never reused within one server life, so no live resurrection — only the
restart path. FIX: stamp a server PID / socket-birth check, or accept as documented narrow case.

### I4 [IMPORTANT/race] popup entry resolves pane by most-recent CLIENT, not the opener — tmux-review:634, tmux-branch:38
Inside a popup $TMUX_PANE is the popup pseudo-pane, so display-message -p '#{pane_id}' hits client
recency. Common case correct (last client typed into); wrong-target when another client is more
recently active — review could diff/deliver into another session's agent. FIX: resolve via the
popup's own client, or pass #{pane_id} into the binding string (the click bindings already do).

CLEAN (probed): fzf bind injection (loop), #() job injection via display-message/run-shell (3.7c
runs no jobs from either), tmux-open-path open_one printf %q, @panestate_msg literal insertion,
tmux-branch hostile names, store grammar under hostile fragments.

## FREEZE ROOT CAUSE (freeze-research) — inverts the premise
Pane OUTPUT does NOT re-evaluate status/window/border formats (measured: 200k-line flood →
1 pane-border eval, 2 status evals). Output routes through PANE_REDRAW→screen_redraw_pane;
formats live in screen_redraw_screen, reached only under CLIENT_ALLREDRAWFLAGS.
THE REAL TRIGGER: every `tmux set-option` (any option, incl. `-p @user`) ends options_push_changes()
→ server_redraw_client(every client) = a full server-wide redraw, and the full format cost is paid
even when output is byte-identical (grid_compare runs AFTER). One `set -p @panestate` = one full
redraw. Cost 6s/60w/180p: plain 5.75ms, +nested #{S:#{W:#{P:}}} pane-border 10.75ms (the walk is
~5ms, O(total panes), per-client).

### F1 [CRITICAL freeze trigger] agent-context writes @panestate_ctx UNCONDITIONALLY — agent-context:40
Claude Code calls statusLine every render; each write = a full redraw even when the value is
unchanged (measured: 5 identical writes → 5 redraws; unsetting an already-unset option → 0). This is
the highest-frequency unguarded redraw trigger in the plugin. FIX: the same guard agent-state:130
already has — read current, `[ "$cur" = "$pct" ] && exit 0`.

### F2 [IMPORTANT scale] nested #{S:#{W:#{P:}}} in pane-border-format — examples/tmux.conf banner
Dominant per-redraw cost, O(sessions×windows×panes), per visible pane per full redraw, per client.
FIX (nicm's own advice, tmux#2774/#3352): precompute the cross-window NEEDS YOU banner into ONE
global option that agent-state writes on transitions; border format reads that option.

### F3 [MINOR] agent-state issues @panestate + @panestate_msg as separate set calls
6 writes/6 calls → 6 redraws; batched 3 calls (set \; set) → 4. Coalesces + halves spawns. FIX:
one tmux invocation for the paired writes.

DON'Ts (from research): no c0-* (removed 2.1), don't cut history-limit, no refresh-client -S after
set (agent-state:142 documents why), no #() in pane-border-format, never interpolate volatile values
into #() command strings (bypasses the 1Hz job cap via force=1).

## SCALE / PERF (hunt-scale) — all fork-per-item + bash O(n²) string-append

### S1 [CRITICAL] tmux-review --lines is O(n²) with a fork per row — tmux-review:199-200
:199 `disp=$(printf ...)` = one fork PER ROW; :200 `group["$path"]+=` = O(n²) append into one array
elem. Measured single-file: 2000 rows 4.77s, 4000 13.21s, 8000 55.92s, 32000 649s (clean n² tail;
100k-line diff ≈1.8h). End-to-end blank popup: a routine 2000-line agent change = 10.3s.
FIX: `printf -v disp ...`; accumulate rows in an indexed array or temp file, not `+=` on one key.

### S2 [CRITICAL] every pinned comment re-runs the whole parse + a grep fork per row — tmux-review:245-247,338,310
:245 `grep -qxF ... "$store"` per row; :338 alt-enter reload re-execs --rows every comment; :310+333
lines_mode runs twice per open. 2000 rows: 5.43s per --rows; refreezes the picker after EVERY comment.
FIX: read the store's `## ` headers once into an assoc array before the loop; pass the row count out
of lines_mode instead of calling it twice.

### S3 [CRITICAL] tmux-where ssh /proc walk ~0.7s/call, status bar calls it TWICE per redraw — tmux-where:117,138
KIDS[$ppid] string-append per process; reached on ssh panes (the headline ssh path/branch feature).
2400 procs → 677ms × 2 tokens = 1.36s shell PER REDRAW; at status-interval 2 that's ~68% of a core
continuously, and the 4 refresh-client -S hooks re-trigger on focus. THIS is a second freeze cause on
fleet boxes. FIX: build ppid→children with an indexed array, or walk UP from the pane pid instead of
down from every process.

### S4 [IMPORTANT] fleet queue ~2 serial git/pane, no parallelism/cap — tmux-review:518,523-524,537
wall ≈ panes×2×git_latency + 1.6s. 100 panes: warm 1.58s, 150ms/git (cold FS) 31.9s blank popup.
Opens on every prefix+r from a non-agent pane. FIX: bounded `xargs -P`, or one `git status
--porcelain=v1` pass per pane instead of status+numstat+grep.

### S5 [IMPORTANT] tmux-branch forks per branch — tmux-branch:173
5000 branches: 16.87s (for-each-ref is 0.12s; 99% is the loop; printf -v = 1.07s, 12.7×). FIX: printf -v.

### S6 [IMPORTANT] preview long-line guard samples only first 200 lines — tmux-open-path:170-171 vs 177
A 3MB line at line 500 passes the guard (measured over lines 1-200) and dumps 3MB into fzf per cursor
move — the exact failure :167 says the guard prevents. FIX: measure longest over the render window
[first,last], not 1-200. ALSO :75-82 --preview-diff captures full diff into a var + runs git diff
twice; 21MB/400k-line diff = 2.06s + 177MB RSS per arrow key. FIX: stream `git diff|awk|head-400`,
drop the second diff.

### S7 [MINOR] clicked-line token loop uncapped — tmux-open-path:329-354
0.31ms/token linear; 112KB line = 1.59s. Degradation, no hang. FIX: cap like the preview.

CLEAN at scale (hunt-scale, with probes): window tabs/fleet counter/tint (linear ~4× stock, 1.6%/core
at 100w — bounded), agent-state under tool storm (flat 9-11ms, fast-path works), tm 200 sessions,
tmux-jump 25-400 panes, run-shell without -b does NOT block the server on 3.7c, binary repos, 10MB
single-line --lines (linear), --compose 300KB store, 200k-entry dir browser, tmux-where non-ssh
fast path (5.5ms), agent-context at realistic payload sizes.

## FREEZE — the actual trigger (freeze-rig, reconciles freeze-research)
It is NOT log volume. Plain spam (6000 lines/s, no escapes): full config = control, 3% CPU.
The trigger is the OSC-2 TERMINAL-TITLE stream agent TUIs emit. Same 120 panes, titles only:
control 7% → full cockpit config 33% CPU, p95 7→26ms.
Mechanism (tmux 3.7c source): input.c OSC 0/2/7 → server_status_window(); screen_set_title()
(screen.c:264) returns 1 UNCONDITIONALLY (never compares to prev title); server_status_window marks
every client whose SESSION contains the window (not just those viewing it); CLIENT_REDRAWSTATUS implies
REDRAW_PANE_STATUS → re-expands pane-border-format for every visible pane → the banner's
#{S:#{W:#{P:}}} whole-server walk. An agent updating its title = "walk every pane on the server".

Correction to earlier H1: #{&&:} DOES short-circuit, so ONLY the active pane runs the banner walk —
cost is LINEAR in total panes (~0.030ms/pane/redraw), not quadratic. Still the dominant offender.

End-to-end, 400 panes, 4 title-emitting agent panes, round-trip of `display-message -p ok`:
  V0 control                        p50 8ms   p95 11ms   CPU 13%
  V3 border-format WITH banner      p50 47ms  p95 104ms  CPU 69%   ← the freeze
  V5 status-right agent_fleet       p50 11ms  p95 25ms   CPU 49%
  V7 FULL shipped config            p50 113ms p95 143ms  CPU 82%   (12 agents: p50 137ms)
agent_fleet is 2nd-worst and cheap: each counter appears TWICE in the format (test + output) = 2-4
whole-server walks. REGRESSION TESTS MUST EMIT OSC-2, not plain lines (plain lines show nothing).

FIX (converges with freeze-research): move the nested banner walk OUT of pane-border-format into a
precomputed global option that agent-state writes on transitions; guard agent-context's unconditional
write; collapse agent_fleet's double-walk. All three are the "precompute into an option" pattern
(nicm tmux#2774/#3352; catppuccin PR#328).

## HARD-HANG RESOLVED (hunt-hardhang) — it's the modal review popup, not a server deadlock
tmux 3.7c robust against ALL six server-hang mechanisms (HH1 run-shell, HH2 modal, HH3 #() jobs,
HH4 hook storm, HH5 fd/EMFILE, HH6 frozen-tty) — every one reproduced as a NON-hang, server never
worse than ~5ms, recovers. Longest server-unresponsive window across all rigs: ~0.

THE ACTUAL "truly locked, needs kill" = SINGLE-CLIENT modal freeze from prefix+r. Chain:
bind-key r → display-popup -E (modal to the ONE client) → tmux-review lines_mode at popup STARTUP
(:310 wc -l) AND every fzf reload (:250) → the S1 O(n²) parser. Measured single-file: 4k rows 3.8s,
10k 11.5s, 20k 29s; ~100k ≈ minutes, ~500k-1M ≈ the 1.8h figure. Modal the whole time → blank frozen
popup, only kill recovers. A single-client user CANNOT distinguish this from a server hang. Real
trigger under agent load: agent regenerates a big single file (lockfile / vendored bundle / minified
JS / CSV), user hits prefix+r → multi-minute-to-hour lock. Conflated with the 82%-CPU molasses =
"totally locked".
=> The S1/S2 O(n²) fixes (Wave C) ARE the hard-lock fix, not just "blank popup for 10s". PLUS a new
cheap guardrail: CAP --lines above N rows with "diff too large to anchor line-by-line" so a giant
lockfile diff can NEVER wedge the popup. And get lines_mode off the popup-STARTUP critical path
(lazy/background + spinner, esc stays live). Defense-in-depth: bind-key J via run-shell -b.
CAVEAT: HH6 tty test was a Linux pty, not Windows Terminal/ConPTY. If a genuine ALL-CLIENTS hang
exists it's terminal-side; set-titles off remains an untested-on-ConPTY ambient mitigation.

## WINDOWS TERMINAL FREEZE (hunt-hardhang, part 2) — per-client TTY_BLOCK + set-titles OSC storm
No server-wide deadlock on ANY path (tty-drain included): tmux uses per-client TTY_BLOCK backpressure
(~sx*sy*8 bytes) — non-blocking, never a synchronous tty write, a 2nd client stays <7ms responsive
while a throttled 10KB/s client saturates. So a single WT client freezing = its OWN buffer over
TTY_BLOCK while WT can't drain, NOT a server hang. Diagnostic for the user: a 2nd SSH session to the
same tmux stays responsive → confirms per-client drain-stall, not a hang.
AMPLIFIER (the actionable finding): `set -g set-titles on` (full.tmux.conf:40-41) FORWARDS every
active-pane title change to WT as a distinct OSC title seq, UNCOALESCED (unlike cell redraws). Rig:
set-titles ON = 609 title changes → 609 OSC titles (27,295 title bytes, 38KB total/3s); OFF = 0 OSC
titles (9.7KB total/3s). ~4x bytes, ~200 title-OSC/sec under storm. Claude Code updates pane_title
frequently → real stream. WT is documented-slow at high-freq title updates → its read loop stalls →
drains tmux slower → tmux out-buffer crosses TTY_BLOCK → WT client freezes in ALL situations.
FIX (one line, immediately shippable): `set -g set-titles off`. Kills 100% of title-OSC forwarding;
does NOT lose the in-tmux tab topic (catppuccin_window_text reads #{pane_title} INTERNALLY,
full.tmux.conf:178-179 — independent of forwarding); only the outer WT window title stops updating.
Redraw path (borders/tint/status) IS coalesced by tmux (156 B/s), so the banner/fleet fix cuts CPU +
redraw FREQUENCY; the byte relief for WT comes mainly from set-titles off + fewer redraws. Ship both.
Lower-priority WT-hostile: tint full-bg repaint (coalesced, bounded), allow-passthrough (needed),
truecolor SGR bytes (minor).
