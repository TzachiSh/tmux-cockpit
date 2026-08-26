# Left click on a window in the status line flashes a menu (2026-08-26)

Issue #21. Left-clicking a window in the status line flashes a menu for about a
second, then it disappears; the expected result is that the click selects the
window and nothing pops up. This is the research: what tmux 3.7c actually does
with that click, which menu can flash and why, what the repo contributes
(nothing), and the smallest thing to change. Researched by an Opus agent,
checked and filed by the session lead.

Everything below was measured on this machine unless marked `[docs]`. The
owner's live server was read only (`list-keys`, `show-options`, `show-messages`,
`display -p`); every experiment ran on private sockets (`-L clk`, `-L stk`) that
are now killed. Clicks were synthesised by attaching a real pty client with
`TERM=xterm-ghostty` and writing SGR mouse sequences (`\e[<0;X;YM` press,
`\e[<0;X;Ym` release), with `tmux -vv` on the probe server.

## Facts

- tmux 3.7c at `/home/tzachi/.local/bin/tmux`. Ghostty 1.3.1, `xterm-ghostty`.
  `mouse on`, `status-interval 2`, `display-time 2000`, `status on` (one line),
  `status-position bottom`, `pane-border-status top`.
- The live root table's status bindings are byte-identical to a stock 3.7c
  server started with `-f /dev/null`. Both have `MouseDown1Status switch-client
  -t =`, `MouseDown3Status display-menu ...` (window menu), `MouseDown3StatusLeft
  display-menu ...` (session menu), `C-MouseDown1Status swap-window -t @`,
  `M-MouseDown3Status`, `M-MouseDown3StatusLeft`, `WheelUp/DownStatus`. Nothing
  in `~/.tmux.conf`, `cockpit.tmux` or the other plugins rebinds any of them.
- Stock 3.7c also binds two left-button control keys: `MouseDown1Control8
  resize-pane -Z` and `MouseDown1Control9 display-menu -O -T "Kill pane
  #{pane_index}?" -t = -x M -y M Yes y { kill-pane -t = } No n { }`. These are
  tmux's own, not this repo's; they drive the `[z]` and `[x]` buttons that
  `pane-border-format` marks with `#[range=control|8]` and `|9`.
- The repo binds `MouseDown1Control1` (branch picker popup),
  `MouseDown1Control2` (file browser popup), `C-MouseDown1Pane`,
  `C-MouseDown3Pane`, `M-MouseDown1Pane`, `C-DoubleClick1Pane`. None of them is
  a plain left click on the status line's window list.
- Only four options carry a `range=`: `status-right` has `control|2` (directory)
  and `control|1` (branch); `status-format[0..2]` carry the stock `left`,
  `right`, `window|`, `pane|`, `session|`. `window-status-format` and
  `window-status-current-format` carry none.
- `~/.tmux.conf` and `examples/full.tmux.conf` have drifted apart (the live file
  still has the per-redraw `#{P:}` pane walk in `@catppuccin_window_text`, and
  `set-titles on`). Neither difference touches mouse handling.
- Live left click, reproduced under the owner's exact config on a private
  server: `mouse 00 at 13,23` -> `mouse range: window @1` -> `mouse key is
  MouseDown1Status` -> `switch-client -t =`, and the status line went from
  `0:bash* 1:bash` to `0:bash- 1:bash*`. Zero menu bytes in the captured output.
- Live right click on the same cell: the capture taken between press and release
  contains `Swap Left`, `Swap Right`, `Kill`, `Respawn`, `Rename`, `New After`,
  `New At End`. The capture taken after release contains none of them. That is
  the flash, exactly.
- `man tmux`, display-menu: "If the mouse is enabled and the menu is opened from
  a mouse key binding, releasing the mouse button with an item selected chooses
  that item and releasing the mouse button without an item selected closes the
  menu. `-O` changes this behaviour". Upstream, nicm in tmux#4220: "This is how
  tmux menus work, click and hold to open, release to select and close."
- The tmux 3.7 CHANGES file has no entry changing `MouseDown1Status` or menu
  dismissal. Its mouse work is the new `control0` to `control9` ranges and a new
  `message-format` that lets a message overlay part of the status bar instead of
  replacing the whole line.
- Side finding, not part of this bug but worth an issue of its own: the live
  client message log holds its full 1000 entries and spans about one minute,
  because `agent-state` polling floods it with `display-message -p` and
  `show-options -pqv` command clients. `show-messages` therefore cannot diagnose
  a click after the fact on this server.

## H1: Ghostty sends something tmux decodes as another button

Evidence for: it is the only path left standing after H2 to H7 are eliminated,
and the flash's fingerprint (menu on press, gone on release) matches
`MouseDown3Status` and nothing else in this configuration.

Evidence against: SGR decoding itself is correct. `\e[<0;X;YM` on the status row
decodes to `MouseDown1Status` every time; `\e[<2;X;YM` decodes to
`MouseDown3Status`. Ghostty's config here is six lines (theme, font, window
theme, contrast) with nothing mouse related, and it advertises
`bpaste,clipboard,extkeys,focus,RGB,title`, none of which affects mouse
encoding.

Verdict: not settled, and not settleable from outside Ghostty, because the agent
cannot press a physical button. It is the leading explanation. The owner
experiment below decides it in one click.

## H2: `switch-client -t =` on a window target opens an overlay

Evidence against, conclusive: on both a stock server and the owner's config, a
left click on a window tab fires `switch-client -t =`, the target resolves
(`cmd_find_from_mouse: s=$0 p / wl=1 1 w=@1 / wp=%1`), the window changes, and
the byte capture contains no menu and no popup. Swept all 200 columns of a
200-column status line with 13 windows: 43 window-tab clicks, 43
`switch-client`, zero `display-menu`, zero `display-popup`.

Verdict: disproven. `switch-client -t =` is correct and silent.

## H3: a menu opened on MouseDown is dismissed on MouseUp

Evidence for, conclusive: captured directly (window menu present between press
and release, absent after), documented in `man tmux` under display-menu, and
confirmed upstream by the maintainer in tmux#4220. `-O` is the documented opt
out, and it works: the stock `MouseDown1Control9` kill-pane menu carries `-O`
and survived a six second hold.

Verdict: confirmed, and this is the mechanism of the flash. But in this
configuration it is only reachable from `MouseDown3Status`,
`MouseDown3StatusLeft`, `MouseDown3Pane` and their `M-` variants, all button 3.
There is no left-button menu on the status line at all.

## H4: a repo hook, `refresh-client`, or the 2 s redraw closes the overlay

Evidence against, conclusive: held the `MouseDown1Control9` menu open for six
seconds under the owner's config, which spans three `status-interval` ticks and
the `after-select-pane` / `after-select-window` / `client-session-changed` /
`pane-focus-in` `refresh-client -S` hooks. The menu was still drawn at the end.
Overlays are not affected by status redraws. `grep` over `scripts/` and
`cockpit.tmux` finds no `refresh-client`, `source-file`, `kill-server` or
`switch-client` on any mouse path; `agent-state` deliberately does not call
`refresh-client`.

Verdict: disproven. The repo is not closing anything.

## H5: the status formats make a window tab overlap a control range

Evidence against, conclusive: the full-width column sweep, with the owner's
config and 13 windows so the window list is truncated and butts up against
`status-right`, maps cleanly with no overlap:

```
1-6      range=left      -> MouseDown1StatusLeft (unbound)
7-151    range=window @N -> MouseDown1Status
152-187  range=control 2 -> MouseDown1Control2   (directory segment)
188      range=control 1 -> MouseDown1Control1   (branch segment)
```

Every window tab resolves to `range=window` and `MouseDown1Status`. The control
ranges start only where `status-right` starts.

Verdict: disproven.

## H6 (added): a cockpit popup on control|1 or control|2 opens and closes by itself

This was the strongest repo-side candidate: `display-popup -E` running fzf looks
like a menu, and if the script exited fast it would flash for about a second.
Outside a tty both scripts do exit fast (`tmux-branch` 0.04 s,
`tmux-open --browse` 0.00 s).

Evidence against: inside a real popup they do not. Clicked column 188
(`control|1`) on a probe session whose panes sit in `/tmp`, which is not a git
repo, and polled: the `tmux-branch` process tree was alive at 1, 3, 4, 6 and 7
seconds and the popup kept repainting. `control|2` likewise stayed open through
a 9 second observation.

Verdict: disproven. Neither cockpit popup flashes.

## H7 (added): the stock kill-pane menu on control|9

`MouseDown1Control9` is a genuine left-click menu, the only one in the whole
configuration. It is the `[x]` button that `pane-border-format` draws.

Evidence against: it carries `-O`, and measured, it stays open (present in both
the press capture and the release capture, and after a six second hold). And
with `pane-border-status top` the `[x]` sits at the right end of a pane's top
border row, never on the status line.

Verdict: disproven as the flash, but worth knowing it exists: a left click on
`[x]` does open a "Kill pane N?" menu, and that is stock tmux, not this repo.

## Best current explanation

Not a tmux-cockpit bug, and not a tmux bug.

A plain left click on a window in the status line is correct on this exact
setup: it fires `MouseDown1Status` -> `switch-client -t =`, selects the window,
and draws no overlay. The agent could not make it produce a menu from any
column, with either the stock config or the owner's.

The only thing in this configuration that opens a menu on press and erases it on
release is a `MouseDown3*` binding. So the event reaching tmux almost certainly
carries button 3, not button 1, and the dismissal is tmux's documented and
intended press-and-hold menu behaviour, not a regression.

What the agent cannot do is press a physical button in Ghostty, so this needs
one click from the owner to close.

### The 30 second experiment

Rebind only `MouseDown1Status` to a silent logger, click once, read the file,
restore. Nothing else is touched, and the restore line is the stock binding
verbatim.

```sh
tmux bind -n MouseDown1Status run -b 'echo "left range=#{mouse_status_range} line=#{mouse_status_line} x=#{mouse_x}" >> /tmp/click.log'
# now left-click a window in the status line, once, the way that flashes the menu
cat /tmp/click.log
tmux bind -n MouseDown1Status switch-client -t =    # restore, this is the stock binding
```

Verified end to end on the probe server: a click on a window tab appends
`left range=window line=0 x=13`.

How to read it:

| `/tmp/click.log` | menu flashed | Conclusion |
|---|---|---|
| a `range=window` line | no | Everything is correct; the earlier flash was a right click or a stray button 3 |
| a `range=window` line | yes | Something other than tmux drew it; capture again with the log toggle below |
| a `range=control1` or `range=control2` line | yes | The click landed on the branch or directory segment, not a window tab; that is a cockpit popup and this reopens as a real bug |
| empty | yes | The event is not button 1. It is `MouseDown3Status`, and the menu is the stock window menu behaving as documented |

If more detail is wanted, tmux 3.7c toggles verbose logging at runtime with
SIGUSR2 (verified today), with no config change at all:

```sh
kill -USR2 $(tmux display -p '#{pid}')   # on;  log lands in the server's cwd, /home/tzachi
# click once
kill -USR2 $(tmux display -p '#{pid}')   # off, promptly: this server logs heavily
grep -E 'mouse key is|mouse range:' ~/tmux-server-*.log | tail -6
rm -f ~/tmux-server-*.log
```

The lines to read are `mouse range: window @N` and `mouse key is
MouseDown1Status`. Toggle it off within a second or two: `agent-state` polling
already fills a 1000 entry message log per minute on this server, so the debug
log grows fast.

## Fix

Ranked, stop at the first that holds.

1. Nothing. If the log shows `range=window` and no menu, or shows nothing and a
   menu, there is no code to write. tmux menus are press and hold: hold the
   button, move onto the item, release. Zero lines, and it is what the man page
   documents.

2. If the owner wants the right-click menus to stay open after release, that is
   `-O`, exactly as nicm advises in tmux#4220, and it belongs in `~/.tmux.conf`,
   not in the plugin. The stock menu bodies are long, so the lazy form re-binds
   from the current binding rather than pasting them:

   ```sh
   # in ~/.tmux.conf, after the plugins load
   # tmux menus close on button release by design (man tmux, display-menu -O).
   # -O keeps the status-line menus up so they can be read before choosing.
   run-shell 'tmux list-keys -T root | grep -wE "MouseDown3Status|MouseDown3StatusLeft" | sed "s/display-menu /display-menu -O /" | tmux source-file -'
   ```

   `[docs]` on that one-liner: not run. Verified instead that
   `tmux list-keys -T root <key>` prints nothing on 3.7c here (rc 0, no output),
   which is why it greps the full table. Pasting the two `-O` bindings by hand
   is the lazier answer if the sed pipeline misbehaves.

3. Nothing in `cockpit.tmux`. There is no version gate to hang this on, because
   there is nothing to gate.

## Not doing

- Not binding `MouseDown1Status select-window -t =`. The stock `switch-client -t
  =` was measured selecting the window correctly. Rebinding it would be a
  cargo-cult fix for a binding that is not broken, and `select-window` is
  strictly weaker: it cannot follow a click into another session.
- Not rebinding the root mouse table, and not touching `MouseDown3Status`,
  `MouseDown3StatusLeft` or `MouseDown3Pane` from the plugin. The repo's rule
  holds: every plain gesture reaches the user and the application untouched;
  only chorded gestures are claimed.
- Not adding a Ghostty-specific workaround. Ghostty is not proven to be the
  cause, its config here is clean, and terminal-specific hacks in a plugin are
  how the Alt-click mess started.
- Not disabling the mouse, and not touching `status-interval`, the
  `refresh-client -S` hooks, or the precomputed window state. H4 is disproven;
  none of them can close an overlay.
- Not filing an upstream issue. The behaviour is documented in `man tmux` and
  confirmed by the maintainer in tmux#4220. There is nothing to report.
- Not "fixing" the `control|9` kill-pane menu. It is stock tmux and it is
  correct.

## Plan

Branch `fix/status-click-menu`, worktree `~/.cache/cockpit-wt/status-click-menu`
(never `git checkout` in the shared main checkout).

Audit against the lens first. Keep: the regression test, because it is the
only durable output and it guards a real hazard. Skip: any config change, any
plugin binding, any upstream report, until step 1 says otherwise. Replace:
nothing.

1. Ask the owner to run the 30 second experiment above and paste
   `/tmp/click.log` plus whether a menu flashed, onto issue #21. Do this before
   writing any code. Three of the four outcomes mean the branch never gets a
   config change.

2. If the log is empty and a menu flashed, the event is button 3. Close #21 as
   not a bug with the man page quote and the tmux#4220 link. Stop. No branch, no
   worktree, no code.

3. If the log says `range=control1` or `range=control2` with a menu, this is a
   real cockpit bug after all: reopen with that evidence and re-plan. The sweep
   says it should not happen, so treat a hit here as the sweep being wrong about
   the owner's window count and re-run it at their real width first.

4. Regardless of outcome, land the regression test, fail first. In
   `tests/run.sh`, on a private server sourcing `examples/tmux.conf`, assert
   that a left click on a window tab selects the window and draws no overlay:

   a. Write the assertion against a deliberately broken binding
      (`bind -n MouseDown1Status display-menu -T x a a {}`) and confirm it goes
      red. A test that has never failed is not a test.
   b. Restore the stock binding and confirm it goes green.
   c. The driver is the pty harness from this research, kept small: `pty.fork()`,
      `TERM=xterm-ghostty`, `TIOCSWINSZ`, `tmux attach`, write `\e[<0;X;YM` then
      `\e[<0;X;Ym` at the status row, capture between press and release, and
      grep for `Swap Left`. Two assertions: the window index changed, and the
      between-press capture has no menu text.
   d. It must obey the suite's existing rules: private `-L` socket registered in
      `SOCKS`, its own `mktemp -d` state, no `TMUX` leakage into the child, and
      no dependency on the user's live server. Count the extra `fork` against
      the fleet gate's exec budget before touching budgets.

5. Commit the doc and the test together, link both on issue #21, and close it
   with the verdict from step 2 or 3.

6. Separately, open a small issue for the side finding: `agent-state` polling
   fills the 1000 entry client message log in about a minute, which makes
   `show-messages` useless for diagnosing any input event on a live cockpit
   server. That cost real time here.

## Still unverified

Everything else above was measured today.

1. H1 itself. The agent cannot press a physical button in Ghostty, so "the
   event carries button 3" is an inference from eliminating H2 to H7, not a
   measurement. The 30 second experiment settles it.
2. The `-O` re-bind one-liner in Fix step 2, marked `[docs]`. The `-O`
   semantics and that `list-keys -T root <key>` returns nothing on 3.7c were
   verified, the sed-and-source pipeline itself was not run.
3. Whether the owner's live session at its real width and window count maps
   columns the same way as the 200-column, 13-window sweep. Same config, same
   tmux, but not their exact geometry.
4. Ghostty's internal mouse encoding path. Its config here is six unrelated
   lines; no Ghostty source read, no other terminals tested.

## Outcome (2026-08-26, evening): a lost click, not a menu

This section supersedes "Best current explanation", the ranked "Fix" and the
"Not doing" bullets about `MouseDown1Status` and upstream. Everything here was
measured.

The owner ran the experiment with loggers on every status-line button-1 key,
eleven trials. Every trial reads the same:

```
20:03:23 MouseDragEnd1Status range=window x=66
20:03:23 MouseUp1Status      range=window x=66
20:03:23 MouseDown1Status    range=window x=66    <- the second click, a second later
20:03:23 MouseUp1Status      range=window x=66
```

The first press never arrives as `MouseDown1Status`; it arrives as
`MouseDragEnd1Status`, the window does not switch, and the retry works. Zero
`MouseDown3*` lines, so H1 (button 3) is dead. The symptom is a lost click.

Mechanism, from tmux 3.7c `server-client.c` and reproduced on a private
`-f /dev/null` server with `-vv`:

- `server_client_check_mouse` rewrites any non-drag, non-wheel event into
  `MouseDragEnd1<where>` while `c->tty.mouse_drag_flag` is set ("Stop dragging
  if needed"), and that block is the only place the flag is cleared.
- `server_client_handle_key` hands keys to an open overlay (popup or menu)
  BEFORE `check_mouse` runs, so a release eaten by an overlay never clears the
  flag.
- A press whose binding opens a popup or menu, followed by a one-cell pointer
  twitch in the same tty read: the twitch is decoded as a drag start (flag set)
  while the press's command is still queued, the overlay opens, the release
  goes to the overlay, the flag stays set. The next press anywhere on the
  client becomes a DragEnd and is lost. A popup that appears mid-drag
  (`tmux-review --request` from an agent) gets there without the twitch.

Lab log of the lost click, `(1)` being the stale flag:

```
mouse 00 at 14,23 (last 2,23) (1)
down at 14,23
mouse key is MouseDragEnd1Status
mouse 03 at 14,23 (last 14,23) (0)
up at 14,23
mouse key is MouseUp1Status
```

Same click without the twitch: `mouse key is MouseDown1Status`, window
switched. Repro by hand, stock tmux, no plugin:

```sh
tmux -L x -f /dev/null new -d -s t 'sleep 300'; tmux -L x neww -d -t t
tmux -L x bind -n MouseDown1StatusLeft display-popup -E 'sleep 300'
tmux -L o -f /dev/null new -d -s o -x 80 -y 24 'tmux -L x attach -t t'
P=$(tmux -L o list-panes -F '#{pane_id}' | head -1)
tmux -L o send-keys -t $P -H $(printf '\033[<0;2;24M\033[<32;3;24M' | od -An -tx1)   # press + twitch, one write
tmux -L o send-keys -t $P -H $(printf '\033[<0;3;24m' | od -An -tx1)                  # release, into the popup
tmux -L x display-popup -C -c $(tmux -L x list-clients -F '#{client_name}')
tmux -L o send-keys -t $P -H $(printf '\033[<0;15;24M\033[<0;15;24m' | od -An -tx1)  # click the second tab
tmux -L x display -p -t t '#{window_index}'   # 0: the click was lost (1 without the twitch line)
```

Fix, verified in the same harness: stock 3.7c leaves `MouseDragEnd1Status`
unbound, so the plugin claims it with no cost to anyone:

```
bind -n MouseDragEnd1Status switch-client -t =
```

The lost press then selects the tab. A genuine drag released on a tab selects
that tab, which is the natural reading of the gesture. The bug proper is
upstream (the flag should reset when an overlay swallows the release); the
eight lines above are the report, filing it is the owner's call.

Not explained by the log: a box drawn during the lost click. The owner's
original wording said a menu flashed; nothing in eleven trials drew one. Open
until the owner says whether anything is still visible with the fix in.
