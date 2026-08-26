# Move a window to another session (2026-08-26)

Owner's ask (issue #20): move the current window to another session, or into a
brand new session, from inside tmux, without typing the target by hand. What
stock tmux already does, what the repo already has, what the smallest thing
worth building is, and a plan. The answer turns out to be four lines of tmux
config and no new script. Researched by an Opus agent, checked and filed by
the session lead.

Verified locally on this machine unless marked `[docs]`. tmux 3.7c, fzf 0.44.1,
Ghostty, `mouse on`. Every probe ran on a private server
(`tmux -L probe -f /dev/null ...`, plus a second `-L probeo` server holding a
real client so keypresses and menus behave like keypresses and menus); both were
killed at the end and the owner's `henosis` and `tests` sessions were never
touched.

## 1. What stock tmux already gives

| Command | What it does | Gap for this ask |
|---|---|---|
| `prefix .` = `command-prompt -T target { move-window -t "%%" }` | prompt reading `(move-window)`; Tab pops a completion menu of matching targets (verified: typing `a` then Tab offered `another: (1)`) | you still type the first letters; the client does not follow; the last window out detaches you |
| `choose-tree` (`prefix s`, `prefix w`) | picks a session or window to switch to; `S-Up`/`S-Down` swap windows *within* a session | cannot move a window across sessions at all |
| `move-window -s X -t sess:` | the actual operation, and a bare trailing colon picks the next free index (verified: `-t dst:` landed at 3 with `base-index 1`) | no picker, no client follow |
| `link-window` | same but the window stays in both sessions | not what "move" means |
| `break-pane -t other:` | moves one *pane* into a new window in another session, and it does work cross-session (verified: `break-pane -d -s dst:1.1 -t 'src:'` landed at `src:2`) | wrong granularity, and it splits a window rather than moving it |
| `MouseDown3Status` (right-click a window tab) | stock menu: Swap Left/Right/Marked, Kill, Respawn, Mark, Rename, New After, New At End | nothing crosses sessions |
| `MouseDown3StatusLeft` (right-click the session name) | Next, Previous, Renumber, Rename, Detach, New Session, New Window | nothing moves the current window |

Verdict on the ponytail rule "if stock plus one bind covers 90%, stop": it does
not, but it is closer than issue #20 assumes. `prefix .` has Tab completion, so
the typing is two or three keys, not a full session name. What stock cannot do
at all is the other two thirds of the ask: the client is left behind, and moving
your last window throws you out of tmux. Those are the parts worth fixing.

## 2. Semantics, measured

### 2.1 `move-window` against `link-window`

`move-window` unlinks the window from the source session and links it to the
destination. `link-window` links it to the destination and leaves it in the
source, so the window lives in both, and `unlink-window` undoes it. `[docs]` for
the unlink path, which was not exercised. For "move this window over there",
`move-window` is the command, and a link row would be a second concept in a menu
that exists to do one thing.

### 2.2 The client does not follow

Verified with a real client attached to `src` sitting on `src:0`, `src` also
holding `src:1 keeper`:

```
tmux move-window -t dst:
```

The window arrived at `dst:2`. The client stayed in `src` and landed on
`keeper`. So a bare move makes the window you are working in vanish from under
you. Following has to be asked for.

### 2.3 The source session emptying is the real hazard

An empty session is always destroyed. There is no option to keep one, and
`destroy-unattached` (`off` on the owner's server) is about detached clients,
not about window count. With `detach-on-destroy on` (the owner's value, and
tmux's default), destroying the session a client is attached to detaches that
client.

Verified, and it is as bad as it sounds. A client attached to a one window
session, running `tmux move-window -t dst:` from inside that window: the window
moved, `src` was destroyed, the client list came back empty, and the outer
terminal running the attach exited. Moving your last window drops you out of
tmux entirely.

Verified that you cannot repair this afterwards. `move-window -s @0 -t '=dst:' \;
switch-client -t @0` on a one window source detached the client just the same:
the destroy happens first and the `switch-client` has no client left to move.

Verified fix, and it is the whole trick: switch the client first, then move.

```
switch-client -t '=dst' ; move-window -s @0 -t '=dst:' ; select-window -t @0
```

The client leaves `src` before `src` empties, so the destroy has no attached
client to detach. Result: `src` gone, client on `dst:2`, no detach. The same
sequence on a multi window source left `src` alive and the client on the moved
window. Both cases verified.

Because the client has already left the source session by the time the move
runs, the source window has to be named by its stable `#{window_id}`. A bare
`move-window` with no `-s` would move whatever the destination's current window
happens to be.

### 2.4 Index collisions and renumbering

| Situation | Behaviour (verified) |
|---|---|
| `-t dst:` (bare colon) | next free index, respecting `base-index`. This is the form to use |
| `-t dst:'$'` onto an occupied index | `index in use: 2`, exit 1, nothing moves |
| `-t dst:0 -a` | inserts after, shifting later windows up |
| `-t dst:^ -k` | kills the window at the destination's lowest index and takes its place. Used for the new session row below |
| `renumber-windows on` (the owner's value) after a window moves *out* | does not fire. `src` was left holding 0, 1, 3. It renumbers on window close, not on move out (verified: killing `src:1` did renumber 0,1,2 to 0,1) |

The gap left in the source session is cosmetic and stock already has the cure on
a key nobody presses: `move-window -r`, in the stock right-click session menu as
"Renumber". Not worth automating.

### 2.5 What travels with the window

Verified that both per pane and per window user options survive a move: a pane
carrying `@panestate needs-input` and a window carrying `@winmark yes` arrived
in the destination session with both intact. That matters here, because this
plugin keeps agent state in pane options and review marks in
`@cockpit_review`. Moving an agent's window between sessions does not disturb
the fleet counter, the border colours or `prefix J`.

## 3. Where the gesture lives

### 3.1 The format loop makes a real menu, with no script

`display-menu` takes `name key command` as separate argv words, so a format
cannot produce a variable number of rows on its own. But `run-shell -C` runs a
*tmux* command after expanding its argument as a format, and that argument is
one string the tmux parser then splits. So `#{S:...}` can build the whole menu.
Verified from a keypress with a real client:

```
run-shell -C "display-menu -T t3 -x C -y C #{S:'#{session_name}' '' 'display-message picked' ,}"
```

rendered a menu of sessions. The trailing `,}` is load bearing: the `S:` loop
takes two comma separated forms and uses the second for the *active* session, so
an empty second form omits the session you are already in. Verified with a
client attached: `src` was excluded, `alpha` and `beta` listed. Without a client
there is no active session and nothing is excluded, which is why this has to be
tested through a client rather than from the command line.

Four traps, all found the hard way:

1. `#{window_id}` inside `#{S:}` shadows to *the loop session's* current window
   (verified: `alpha=@0 beta=@1 gamma=@2`). The source window has to be pinned
   outside the loop. A global user option does that, and a global one resolves
   identically in every iteration where a window option would not.
2. `set -g @opt '#{window_id}'` stores the literal string. The menu then expands
   an option whose value is itself a format and the server wedged; the probe run
   had to be killed. `set -gF` expands at set time and stores `@0`. This is the
   same `-F` lesson `cockpit.tmux` already records for `@cockpit_click_line`.
3. `bind-key K cmd1 \; cmd2` typed as shell argv binds only `cmd1` and runs
   `cmd2` once, immediately, at load time. That is exactly how the first probe
   produced "no current client" out of nowhere. `cockpit.tmux` already documents
   this for `prefix R` and already has the cure: write the binding to a temp conf
   file with `{ ... }` braces and `source-file` it.
4. Session names. `'#{session_name}'` broke silently on a session named `it's`:
   the menu simply did not open, no error, no message. `#{qa:session_name}`
   quotes it as a tmux argument (`"it's"`), and using `#{session_id}` (`$4`) for
   every target sidesteps names entirely. Verified: with `#{qa:}` labels and
   `#{session_id}` targets, a session named `it's` both listed and received a
   window, and a session named `has space` did too.

### 3.2 The four placements

| Placement | Concrete form | Cost | Verdict |
|---|---|---|---|
| (a) a key inside the `scripts/tm` fzf popup | `--bind 'ctrl-v:execute(...)'` plus a `--move` mode and a `-p '#{pane_id}'` argument, because inside a popup `$TMUX_PANE` is the popup's own pane | ~25 lines in `tm`, a changed binding in `cockpit.tmux` and in the owner's own `bind f`, a new header line, a new fzf key to keep clear of fzf's own | works, but it is a script and a picker for a list that is usually three items long |
| (b) a `display-menu` built from `#{S:}` | the block in section 4 | 4 lines of config, no file, no script, no install change | recommended |
| (c) a row added to the stock `MouseDown3Status` menu | tmux has no append. You copy the whole stock definition (Swap Left, Swap Right, Swap Marked, Kill, Respawn, Mark, Rename, New After, New At End) and add a row, then own that copy forever | one long brittle line that silently diverges from stock on every tmux release | no |
| (d) a new prefix key running a small script | a script to list sessions and shell out to `display-menu` or `fzf` | a file, an `install.sh` symlink, a README row, a test | strictly more than (b) for the same result |

(b) and (d) are the same gesture; (b) is (d) with the script deleted. That is the
ladder's answer: rung 4, the platform feature covers it.

The key. `prefix .` is the tmux-native choice and the muscle memory is already
there, but it has a second stock use worth keeping: `prefix .` then a bare number
renumbers the current window to that index. Take `prefix M` instead. Stock `M` is
`select-pane -M` (clear the marked pane), the partner of `m` which
`examples/full.tmux.conf:100` already overrides on exactly this reasoning
("overrides the stock 'm' (mark-pane), which almost nobody uses"). `M` for move,
`@cockpit_move_key` rebinds, `off` restores stock, matching
`@cockpit_review_key`, `@cockpit_info_key`, `@cockpit_jump_key` and
`@cockpit_reload_key`.

## 4. Recommendation

One binding. In config file syntax, which is how `cockpit.tmux` has to emit it
(section 3.1 trap 3):

```tmux
bind-key M {
  set -gF @cockpit_move_src "#{window_id}"
  run-shell -C "display-menu -T '#[align=centre] move #{window_index}:#{window_name} to ' -x W -y W #{S:#{qa:session_name} '' 'switch-client -t #{session_id} ; move-window -s #{@cockpit_move_src} -t #{session_id}: ; select-window -t #{@cockpit_move_src}' ,} '' 'new session...' n 'command-prompt -p \"new session:\" \"new-session -d -s %1 ; switch-client -t =%1: ; move-window -s #{@cockpit_move_src} -t =%1:^ -k\"'"
}
```

Reading it back:

- `set -gF` pins the window you pressed the key on, expanded, in a global option.
  It is mutable global state, which is worth saying out loud: it is written and
  read inside a single keypress and never read again, so the ceiling is two
  people pressing `M` on one server in the same instant and one of them moving
  the other's window. On a single user cockpit that is not a real failure mode.
- `#{S:...,}` builds one row per session except the one you are in, labelled with
  `#{qa:session_name}` and targeted by `#{session_id}`.
- Each row switches the client first, then moves, then selects (section 2.3).
- `-x W -y W` puts the menu at the window's own tab on the status line, so it
  points at the thing being moved.
- The `new session...` row prompts for a name, creates the session detached, and
  `-k` has the moved window *replace* the placeholder window rather than sitting
  beside it. No leftover shell, and the moved window keeps index 1 under
  `base-index 1`.
- `=%1:` with a trailing colon, not `=%1`. Without it a session named `a.b` fails
  with `Can't find pane: b`, because `switch-client -t` takes a pane target and
  splits on the dot. `scripts/tm` already learned this exact lesson and says so
  in a comment. With the colon, `a.b` works.

Verified end to end from real keypresses through an attached client:

| Case | Result |
|---|---|
| move to an existing session | `agents:2 logs`, client followed, `work` survived |
| the last window of the attached session into a new session named `solo` | `work` destroyed, no detach, `solo:1 editor panes=1`, client on it |
| only one session exists | menu degrades to just the `new session...` row |
| a session named `it's` in the list, and as the current session | menu opens both ways, move succeeds |
| a session named `has space` | move succeeds, client lands on `[has space]:2` |
| new session name that is empty | nothing created, nothing moved |
| new session name already taken | `duplicate session:`, the sequence aborts, nothing moved |
| new session name `a.b` | works with the trailing colon; without it, a stray placeholder session and the window left behind |

The failure mode of every bad input is the same and it is the right one: tmux
prints an error and aborts the rest of the sequence, verified directly
(`new-session -d -s taken \; display-message 'SECOND RAN'` printed the error and
never ran the second command).

In `cockpit.tmux` it goes next to the other keys, emitted through the temp conf
file technique already used for `prefix R`. `[docs]`: this shell snippet is
written by analogy to the `prefix R` block at `cockpit.tmux:309-338`, not run;
the binding text above was verified by `source-file`ing it into the probe
server and driving it from real keypresses.

```sh
MK=$(tmux show-option -gqv @cockpit_move_key)
case "$MK" in
    off) ;;
    "")  mk=M ;;
    *)   mk=$MK ;;
esac
# then printf the block above with "$mk" into a mktemp file and `tmux source-file` it
```

A user who wants it in their own config instead pastes the same block into
`examples/full.tmux.conf` near `bind m`.

## 5. Not doing

1. Not rebinding `MouseDown3Status`. Appending is not possible; copying the
   stock menu means owning a divergent copy of nine rows across tmux releases.
2. Not writing a script. Four lines of config already do it. A
   `scripts/tmux-move` would add a file, an `install.sh:23` symlink, a README
   row and a shellcheck target for nothing.
3. Not adding a key to the `tm` fzf picker. `tm` is "attach to a session".
   Overloading it means a `--move` mode, a `-p '#{pane_id}'` argument it does not
   take today, and a changed `bind f` in the owner's own config.
4. No `link-window` row. "Copy this window into two sessions" is a different
   feature nobody asked for, and `unlink-window` to undo it is a third.
5. No pane level move. `break-pane -t other:` already does it in one command
   if it is ever wanted, verified working cross-session. It is a different
   gesture on a different granularity.
6. No destination index prompt. The bare `dst:` colon picking the next free
   index is right every time; `prefix .` still exists for the rare exact index.
7. Not renumbering the source session. The gap is cosmetic and
   `move-window -r` is already in the stock session menu.
8. Not rebinding `prefix .`. It keeps the "renumber this window to index N"
   use that the menu cannot cover.
9. Not touching `detach-on-destroy` or `destroy-unattached`. Those are the
   owner's globals and the switch-first ordering makes them irrelevant.
10. No status bar indicator, no per session preview in the menu.
    `display-menu` has no preview pane, and a preview would want a script.

## 6. Test angle

`tests/run.sh` runs every block against a private server. A PATH shim at
`tests/run.sh:31-43` redirects every bare `tmux`, including calls inside the
scripts under test, to a per run socket; `$TMUX`/`$TMUX_PANE` are unset so the
suite never reaches the live server; blocks that need their own server mint one
with `-L "$SOCK" -f /dev/null` and register it in `$SOCKS` so an interrupted run
cleans up. Helpers: `is`, `ok`, `bad`, `skip_`, `need_tmux`, `mktmpd`
(`tests/run.sh:99`), `until_cap` (`:105`) for polling a pane instead of sleeping.

The pattern this feature needs is the *two server* one, because a menu and a
prefix key both need a real client, and `send-keys` into a pane bypasses key
handling entirely (confirmed the hard way: sending `C-b M` to the inner pane did
nothing). `tm: typing filters, ^X kills` at `tests/run.sh:2245` is the model:

```sh
TK="cockpit-tk-$$"; TKO="cockpit-tko-$$"; SOCKS="$SOCKS $TK $TKO"
"$TMUX_REAL" -L "$TK"  -f /dev/null new-session -d -s alpha -x 90 -y 24
"$TMUX_REAL" -L "$TKO" -f /dev/null new-session -d -s o -x 90 -y 26 \
    "$TMUX_REAL -L $TK attach -t alpha"
```

Keys then go to the *outer* server's pane (`send-keys -t "$OP" C-b M`), which is
the terminal the inner client is reading, so the inner tmux sees a real prefix
key. `prefix S opens the session picker` at `:1763` is the lighter model for the
binding-exists assertion (`list-keys -T prefix | awk '$4=="S"'`).

`tests/changed.sh` derives each block's dependencies by grepping the block body
for `scripts/<name>`. A config-only feature has no script name to find, so the
block needs the explicit marker convention on the line *above* its title, as at
`tests/run.sh:222` and `:305`:

```
# tests: cockpit.tmux
echo "== prefix M moves this window to another session =="
```

## 7. Plan

### Ponytail audit

| Asked for | Verdict | Why |
|---|---|---|
| Move the current window to another session | keep | The ask, and stock does not do it without typing |
| Move it into a brand new session | keep | One extra menu row and a `command-prompt` |
| Pick the target without typing | keep | The whole point. `#{S:}` menu |
| The client follows the window | keep | Otherwise the window vanishes from under you |
| Not being detached when the source empties | keep | Verified data loss of your whole tmux session. Fixed by ordering, costs nothing |
| A `scripts/tmux-move` | skip | Four lines of config already do it |
| A key inside `tm` | skip | `tm` attaches; this moves. Different verb, and `tm` would need a pane id it does not take |
| Right-click window menu row | skip | tmux has no append; copying nine stock rows is a maintenance tax |
| `link-window` row | skip | Not what "move" means |
| Pane level move | skip | `break-pane -t other:` is already one command |
| Destination index prompt | skip | Bare `sess:` is right every time |
| Renumbering the source | skip | Cosmetic, and `move-window -r` exists |
| Session name cleaning like `tm`'s `clean()` | replace | A trailing colon on `=%1:` covers the dot case; tmux itself rejects nothing else that matters |
| `@cockpit_move_key` / `off` | keep | Every other cockpit key has it |

### Tasks

Branch `feat/move-window`, in a worktree at `~/.cache/cockpit-wt/move-window`,
never in the main checkout. Each task lands with its failing check first.

1. The binding, in `cockpit.tmux`. `@cockpit_move_key` with `M` as the
   default and `off` to restore stock, emitted through the `mktemp` +
   `source-file` block that `prefix R` already uses at `cockpit.tmux:309-338`,
   because `{ ... }` grouping only survives from real config text.
   Fail first: a new block, marker `# tests: cockpit.tmux`, asserting
   `list-keys -T prefix | awk '$4=="M"'` contains `display-menu`, and that
   `@cockpit_move_key off` leaves stock `select-pane -M` in place, and that
   `@cockpit_move_key W` moves it to `W`. Same shape as `prefix S opens the
   session picker` at `tests/run.sh:1763`.
2. The menu lists the right sessions. Two server rig from `tests/run.sh:2245`.
   Three sessions, client attached to one, `send-keys -t "$OP" C-b M`, then
   `capture-pane` on the outer server.
   Fail first: the capture contains the other two session names and not the
   current one; the title contains `#{window_index}:#{window_name}` of the
   source window.
3. The move follows the client. Same rig, source session holding two
   windows, choose the first row with `Enter`.
   Fail first: `list-windows -a` shows the window in the destination, and
   `display-message -p -c "$CN" '#{session_name}:#{window_index}'` shows the
   client on it. This is the assertion that fails today, since stock leaves the
   client behind.
4. The last window out does not detach you. Same rig, source session holding
   exactly one window.
   Fail first: after the move, `list-clients` is non-empty and the client is on
   the moved window. Without the switch-first ordering this assertion is what
   catches the regression, and it is the one that matters most: getting it wrong
   throws the user out of tmux.
5. The new session row. Choose `n`, type a name, `Enter`.
   Fail first: the named session exists, holds exactly the moved window with
   `panes=1` (no placeholder left over), and the client is on it. Second
   assertion in the same block: a name already taken creates nothing and moves
   nothing; an empty name creates nothing.
6. Hostile names. A session named `it's` and one named `has space` in the
   list, and a new session name containing a dot.
   Fail first: the menu still opens with `it's` present (it does not today
   without `#{qa:}`), and `a.b` receives the window rather than leaving a stray
   placeholder session behind.
7. README and the example config. One row in the key table at
   `README.md:257-262`, in the voice of the rows around it: "`prefix + M` | move
   this window to another session, or a new one, picked from a menu; the client
   follows, and moving your last window will not detach you  `@cockpit_move_key`
   rebinds, `off` restores stock". A short paragraph near the `prefix + S`
   section at `:855` on what happens to a source session that empties. Optionally
   the same block pasted into `examples/full.tmux.conf` near `bind m` for people
   reading that file as documentation.
   Fail first: `grep -q 'cockpit_move_key' README.md`.
8. Gate. `make check-par` green, `shellcheck` clean (the Makefile's
   `SOURCES` picks up `cockpit.tmux` already), and the new block selected by
   `make test-changed` when `cockpit.tmux` changes, which is what the
   `# tests:` marker in task 1 buys.

## Worth knowing regardless

Section 2.3 is a live data-loss path in stock tmux that has nothing to do with
this feature: `prefix .` on your last window today detaches you out of tmux.
The switch-first ordering fixes it for the new key only; `prefix .` keeps the
stock behaviour.
