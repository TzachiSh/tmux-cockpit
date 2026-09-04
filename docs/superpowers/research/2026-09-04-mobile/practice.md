# phone-practice: what tmux-from-a-phone users do

Lane phone-practice, issue #97, 2026-09-04. Read-only research on a private `-L` socket, tmux 3.7c.

## Question

Same tmux config for the 191x45 desktop and the phone, or a different command for the phone? Short answer: same config file, different entry command. The config can adapt by width and that mechanism works, but it would not have fired on this phone. The size isolation the phone needs cannot be done with a format at all.

## Practice

| Practice | Source (quoted) | Applies here? |
|---|---|---|
| Turn the status line off on the phone | "Hide status bar (session name is in my shell prompt)" `set -g status off` ([elliotbonneville.com](https://elliotbonneville.com/phone-to-mac-persistent-terminal/)) | No. Cockpit is the status line. Shrinking it is the option, not deleting it. |
| Bind scroll to a bare key, no prefix | "`bind -n PageUp copy-mode -eu`" and "Termius on iOS sends PageUp when you swipe down with two fingers." (same) | Yes. Termius sends PageUp and the owner's key row has Pg Up. |
| `set -g mouse on` for touch scroll | "`set -g mouse on`" to enable scroll gestures (same) | Already on in the live config. |
| One window per task, do not split panes | "panes are cramped" on phones, adopt "a one-window-per-task layout" ([getmoshi.app](https://getmoshi.app/guides/tmux)) | Yes, as guidance. Not a config change. |
| Short window names, renumber windows | "renumber windows when one closes, so the quick-access row stays dense" `set -g renumber-windows on` (same) | Yes, cheap. Already on in the live config. |
| App launches straight into tmux | "Adding startup commands is as easy as going to Host settings, scrolling to bottom and inserting command we want to use: tmux a -t session_name" ([docs.blink.sh](https://docs.blink.sh/basics/tips-and-tricks)) | Yes. This is exactly where a phone entry command goes. |
| Sticky modifier row is how phones send Ctrl | "A soft keyboard has no key you can hold down while tapping another." and "Every serious mobile terminal solves this the same way: an accessory row above the keyboard with sticky modifier keys you tap once, then tap the letter." ([mtmux.com](https://mtmux.com/blog/tmux-from-phone)) | Yes, and the owner's screenshot shows the row. |
| Remap the prefix away from Ctrl on phones | Blink: "sending `ctrl-space` when hitting `ctrl-space`, as opposed to bringing up iPadOS's emoji menu." ([docs.blink.sh](https://docs.blink.sh/basics/tips-and-tricks)) | No. The owner's Termius row has a sticky Ctrl, so Ctrl-Space is two taps. |
| Shared window shrinks to the smallest client, aggressive-resize frees it | "if two clients both select the same window then tmux will shrink the window to the smallest client's size" (search snippet, page not fetched) | Half right (corrected 2026-09-04, refute pass). The shrink is CONFIRMED. `aggressive-resize` frees the window only under `window-size smallest`, and only on the next window switch; under the owner's `latest` it does nothing. |

No source found recommends a width-conditional status line. None of the guides read mentions `#{client_width}`. The practice in the wild is to turn the status off, not to shrink it.

## Manual checks

tmux 3.7c at `/home/tzachi/.local/bin/tmux`, all quotes from `man tmux` on this box.

| Item | Manual says | Verdict |
|---|---|---|
| Per-client options | "Each option belongs to one or multiple scopes (server, session, window, and pane)" | CONFIRMED. No client scope. |
| Per-client flags | `attach-session -f`: "ignore-size / the client does not affect the size of other clients" | CONFIRMED, and the issue missed it. Flags are per client even though options are not. |
| `#{client_width}` / `#{client_height}` | "client_width / Width of client", "client_height / Height of client" | CONFIRMED, resolved per client during status drawing. |
| `#{>:a,b}` | "String comparisons may be expressed by prefixing two comma-separated alternatives by '==', '!=', '<', '>', '<=' or '>=' and a colon." | CONFIRMED as a string compare. A trap, see below. |
| `#{e|>:a,b}` | "The available operators are: addition '+', subtraction '-', multiplication '*', division '/', modulus 'm' or '%' ... and numeric comparison operators '==', '!=', '<', '<=', '>' and '>='." | CONFIRMED. Required for any width threshold. |
| `#{=/N/…:}` truncation | "'#{=/5/...:pane_title}' will append '...' if the pane title is more than five characters" | CONFIRMED, rendered per client. |
| `status-format` | "Specify the format to be used for each line of the status line." | CONFIRMED, expanded per client. |
| `window-size` | "if smallest, the size of the smallest. If manual, the size of a new window is set from the default-size option ... With latest, tmux uses the size of the client that had the most recent activity." | CONFIRMED, measured table under Shape b. |
| `aggressive-resize` | "resize the window to the size of the smallest or largest session ... for which it is the current window, rather than the session to which it is attached" | CONFIRMED as written, but it changed nothing in the test because the test ran under `window-size latest`, which ignores it. It bites only under `smallest` or `largest` (corrected 2026-09-04). |
| `display-popup -w/-h` percent | "-w and -h give the width and height - both may be a percentage (followed by '%')" | CONFIRMED, percent scales per client. 90%/60% inside a 60x30 client gave a 52x16 usable interior; the box is 54x18 with one border cell each side. Measure with `stty size` inside the popup: `#{popup_width}` and `#{popup_height}` are not formats in 3.7c and expand empty (refute pass, 2026-09-04). |
| `display-popup -w` as a format | not documented as a format | CONFIRMED negative. `-w '#{client_width}'` did not run. |
| `new-session -t` grouped | "Sessions in the same group share the same set of windows ... The current and previous window and any session options remain independent" | CONFIRMED, and window size is not among the independent things. This sinks Shape b's grouped variant. |
| `set -w` per window | "a set of window options is attached to each window and a set of pane options to each pane" | CONFIRMED. `window-size` is a window option. |
| `resize-window` | "-A sets the size of the largest session containing the window; -a the size of the smallest. This command will automatically set window-size to manual in the window options." | CONFIRMED, pinned a window to 131x58 and flipped that window's `window-size` to manual. |
| `refresh-client -C` | "-C sets the width and height of a control mode client or of a window for a control mode client" | CONFIRMED negative. On a normal client: `not a control client`. tmux cannot correct a client that lies about its size. |
| `bind -n` root table | "The root table is used for keys pressed without the prefix key ... -n is an alias for -T root." | CONFIRMED. |
| `status [off|on|2|3|4|5]` | "Using on gives a status line one row in height; 2, 3, 4 or 5 more rows." | CONFIRMED. Each extra row cost the window a row: 60x28 at `status 2`, 60x27 at `status 3`. |
| Mouse on the status line | location suffixes "Status", "StatusLeft", "StatusRight", "StatusDefault" | CONFIRMED, a tap selected a window with no modifier. |
| `terminal-overrides` | "Allow terminal descriptions read using terminfo(5) to be overridden." | Read, not exercised. Carries no size information. |
| `terminal-features` | "intended for classes of functionality supported in a standard way but not reported by terminfo(5)" | Read, not exercised. Carries no size information. |

The issue's constraint line said options are per session or server and never per client. CONFIRMED for options. REFUTED for client flags: `attach-session -f ignore-size` is a per-client lever the constraint wrongly ruled out. Measured under Shape b.

## The string-compare trap

`#{>:...}` compares strings. `#{>:60,120}` is true because the character 6 sorts after 1. Measured on a private socket:

```
client_width=60:  BIG (string)   SMALL (numeric)
client_width=191: BIG (string)   BIG (numeric)
```

Any width threshold has to use the arithmetic form, `#{?#{e|>:#{client_width},120},wide,narrow}`. The lane's first draft used the string form and silently rendered the full desktop status on a 60-column client. The wrong form fails quietly and looks correct on the desktop.

## Shape a: same config, width-aware

Two clients on one session, 191x45 and 60x30, rendered different status lines at the same moment with no second session and no new command.

```
set -g status-right '#{?#{e|>:#{client_width},120},FULL cpu 12% mem 4.1G net eth0 #{host} ,#{?#{e|>:#{client_width},70},MED #{host} ,SM }}%H:%M'
set -g window-status-format ' #{?#{e|>:#{client_width},120},#I:#{=/24/…:window_name},#I:#{=/6/…:window_name}} '
set -g window-status-current-format ' #{?#{e|>:#{client_width},120},#I:#{=/24/…:window_name},#I:#{=/6/…:window_name}} '
set -g status-left '[#{?#{e|>:#{client_width},120},#{session_name},#{=/3/:session_name}}] '
rename-window a-very-long-window-name-here
```

Rendered, captured from each client's own pty stream with escapes stripped:

```
desktop-191x45: '[base]  0:a-very-long-window-name-…FULL cpu 12% mem 4.1G net eth0 Tzachi-lep 07:37'
phone-60x30:    '[bas]  0:a-very…SM 07:37'
```

Cross-checked with `display-message -c <client> -p`, which expands in that client's context. CONFIRMED.

Two things sink it as the answer to this issue. The trap above, cheap once known. And, decisive: it would not have fired on the owner's phone. A client attached at the recorded 131x100 evaluated the threshold at 120 to the wide branch:

```
client reports: /dev/pts/8 131x100
width conditional at threshold 120 on this client: FULL(wide)
```

The phone is not narrow. At 131 columns it renders the full status line. What it is, is short and dishonest about it: 58 rows visible against 100 reported. No `#{client_height}` conditional rescues this either, because the client reports 100 and the conditional believes it.

## Shape b: separate command

Three variants tested. They do not behave alike, and the one the issue's decision text implies is the one that fails.

### window-size, both clients on the same session

Desktop at 191x45, phone at 60x30. Window dimensions after each setting:

| window-size | window, desktop alone | window, phone attached |
|---|---|---|
| latest (owner's current) | 191x44 | 60x29 |
| smallest | 191x44 | 60x29 |
| largest | 191x44 | 191x44 |
| manual | 191x45 | 191x45 |

`default-size` on that server was 80x24, but the session had been created with `-x 191 -y 45`, which sets `default-size` for the session, so manual held 191x45.

`manual` freezes the window at whatever size it already holds. It held 191x45 here only because the option was in force before any client had sized the window. Set `window-size manual` after the 191x45 client is already attached and the window stays at 191x44 instead (refute pass, 2026-09-04).

`latest` and `smallest` destroy the desktop layout the moment the phone attaches. That is the size half of the reported defect, reproduced. The owner is on `latest`.

### Grouped session: FAILED

`new-session -t base -s phone`, the variant the issue's decision text implies:

```
C0 group made:                base:0 191x44  |  phone:0 191x44
   sessions: base group=base attached=2  |  phone group=base attached=0
C1 phone attaches to 'phone': base:0 60x29   |  phone:0 60x29
C2 phone rotated to 45x26:    base:0 45x25   |  phone:0 45x25
```

CONFIRMED negative. A group shares the windows themselves, so sizing one sizes the other. Window size is a window property, not a session option, so it is not independent. `window-size manual` on the grouped pair sent both to 191x44 together, still not independent (corrected from 191x45, 2026-09-04).

### Independent session: WORKED

`new-session -d -s phone -x 60 -y 30`, no `-t`:

```
D0 desktop alone:                 base:0 191x44
D1 phone session made:            base:0 191x44  |  phone:0 60x30
D2 phone attached to own session: base:0 191x44  |  phone:0 60x29
D3 phone rotated to 45x26:        base:0 191x44  |  phone:0 45x25
   clients: /dev/pts/15 base 191x45  |  /dev/pts/17 phone 45x26
```

CONFIRMED. Full isolation under `window-size latest`, no option change. The desktop never moved off 191x44 while the phone attached, detached and rotated.

The one way found to break it was linking a window into both sessions on purpose:

```
F aggressive-resize=on,  phone on the SHARED window: base:0 60x29 | phone:0 60x29 | phone:9 60x29
F aggressive-resize=off, phone on the SHARED window: base:0 60x29 | phone:0 60x29 | phone:9 60x29
```

`link-window -s base:0 -t phone:9` makes phone:9 and base:0 one window object, so the shared window drops to 60x29 the moment a client attaches to the phone session, even while that client is looking at a different window. Under the owner's `window-size latest`, `aggressive-resize` changes nothing, on or off. It is not inert everywhere: under `window-size smallest` with `aggressive-resize on`, the shared window returns to 191x44 as soon as the phone switches away from it, so the search-snippet claim is CONFIRMED for `smallest` and REFUTED for `latest` (refute pass, 2026-09-04, matrix in verified.md). Rule: do not share windows between the desktop session and the phone session, and do not expect `aggressive-resize` to rescue you while `window-size` is `latest`.

### attach -f ignore-size: WORKED, same session, no second session

```
B0 desktop alone:              base:0 191x44
B1 phone attached ignore-size: base:0 191x44
   clients: /dev/pts/6  191x45 [attached,focused,UTF-8]
            /dev/pts/15  60x30 [attached,focused,ignore-size,UTF-8]
B2 phone rotated to 45x26:     base:0 191x44
```

CONFIRMED. The flag shows in `#{client_flags}` and the window never moved. This is the per-client lever the issue's constraint line said did not exist.

Cost: the phone views a 191-wide window through a 60-wide porthole and pans with `refresh-client -L/-R`. Unusable as the everyday path, useful as a safety net for an accidental attach.

### Capture handed to the terminal lane

Where tmux addresses the cursor immediately before writing the status text, on a client claiming 131x100:

```
status-position=bottom  '...\r\n\x1b[K\r\n\x1b[K\x1b[30m\x1b[42m\r\n[base] 0:bash*\x1b[108X\x1b[108C'
status-position=top     '...\r\n\x1b[K\r\n\x1b[K\x1b[30m\x1b[42m\x1b[H[base] 0:bash*\x1b[108X\x1b[108C'
```

At the bottom, tmux emits no absolute cursor address. It walks down with `\r\n` and writes the status wherever the cursor lands. On a terminal whose real height is 58 but which accepted 100 rows, every `\r\n` past row 58 scrolls the visible region, so the status lands wherever the scroll left it and each redraw leaves another copy. That matches the 3-then-4 stacked copies in the two screenshots. At the top, tmux emits `\x1b[H`, an absolute home, which a client lying about its height cannot displace.

`set -g status-position top` is therefore a one-word candidate mitigation. The mechanism call belongs to the terminal lane.

tmux cannot paper over the lie from the server side: `refresh-client -t <client> -C 131x58` returned `not a control client`. `resize-window -t base:0 -x 131 -y 58` pinned the window and flipped its `window-size` to manual, but the client stayed 131x100 and the status still targets the client's last row.

## Keys without modifiers

The 07:10 screenshot shows the Termius Android extended keyboard row across three rows: four arrows, `/`, `@`, Paste, gear, overflow; `^C`, `^Z`, `^B`, attach, shift-tab, Alt (underlined, sticky), Home, End; Home, End, Pg Up, Pg Dn, Delete, Alt, Ctrl (underlined, sticky), Tab.

| Need | What this phone offers | Taps |
|---|---|---|
| Ctrl-Space prefix | sticky Ctrl, then space | 2 |
| Alt-arrow | sticky Alt, then arrow | 2 |
| Scroll back | Pg Up on the row, or a two-finger swipe in Termius | 1 |

The prefix is reachable in two taps. Termius documents the row as three sections: "You may slide the keyboard horizontally to browse through the keys." (https://github.com/smanask/Termius-Documentation/blob/master/android/features/extended_keyboard.md). Blink on iOS needs an explicit remap for the same key (https://docs.blink.sh/basics/tips-and-tricks).

What tmux offers with no modifier at all:

| Mechanism | Manual | Tested |
|---|---|---|
| Tap the status line window list | mouse location suffixes `Status`, `StatusLeft`, `StatusRight`, `StatusDefault`, plus `MouseDown1`/`MouseUp1` | CONFIRMED. With `mouse on`, an SGR press+release at row 30 col 9 on a 60x30 client moved the current window from 2 to 0. |
| `display-menu` | each item has a name, a single-key shortcut and a command | Read, not exercised. Reachable from a mouse binding. |
| `display-popup -w 90% -h 60%` | percent units | CONFIRMED. 52x16 popup inside a 60x30 client, scales per client with no conditional. |
| `bind -n` root table | "-n is an alias for -T root" | CONFIRMED. |
| Bare keys the phone row already has | Pg Up, Pg Dn, Home, End, Tab, arrows | One tap each. |

One published phone config binds exactly that: "`bind -n PageUp copy-mode -eu`", with "Termius on iOS sends PageUp when you swipe down with two fingers." (https://elliotbonneville.com/phone-to-mac-persistent-terminal/)

## Recommendation

Same config file, one extra entry command. The command creates or attaches an independent session, not a grouped one:

```
tmux new-session -A -s phone
```

One line, no new file, no option changes. It is the only tested shape that kept the desktop at 191x44 while the phone attached, detached and rotated.

Riders:

- Do not link or share windows between the desktop session and the phone session. That is the single way found to break the isolation, regardless of `aggressive-resize`.
- Consider `set -g status-position top`, pending the terminal lane's verdict on the ghost-stacking capture. One word, and the status line becomes absolutely addressed instead of walked to.

Skip the width conditionals for now. They work, but the owner's phone reports 131 columns, so a `>120` threshold classifies it as wide and nothing changes. The defect is height, not width. If the mechanism ever ships, use the `e|` arithmetic form.

## Rejected

| Option | Why |
|---|---|
| Grouped session, `new-session -t base -s phone` | Tested and failed. Groups share windows, so the desktop still went to 60x29 and then 45x25. |
| Width-conditional status alone | Works mechanically, but the phone is 131 columns wide. |
| `window-size largest` | Protects the desktop, but the phone then views a 191-wide window it cannot read. Fallback only if windows ever get shared. |
| `window-size manual` globally | Freezes every window at `default-size` and breaks ordinary desktop resizing. |
| `attach -f ignore-size` as the primary path | Smallest possible change, but the phone pans a 191-wide viewport with `refresh-client -L/-R`. Safety net for an accidental attach. |
| A second tmux config file for the phone | Two files to keep in sync for a difference tmux expresses in one command. |
| Forcing the phone's reported size from the server | Not possible. `refresh-client -C` returned `not a control client`. |
| `resize-window` as the ghosting fix | Pinned the window to 131x58, but the client stayed 131x100 and the status still targets the client's last row. |
| Remapping the prefix off Ctrl-Space | The sticky Ctrl in the owner's key row already makes it two taps. |
| A phone-specific TUI or app | Out of scope per the issue's non-goals. |

## Tools

| Step | Tool | Reason |
|---|---|---|
| Read issue 97 | Bash, `gh issue view 97` | The brief said read it first |
| Dump the manual | Bash, `man tmux \| col -bx` to a `/tmp` file with lane name and pid | Grep-able and quotable |
| Extract 18 manual sections | Bash, `grep -n` then `sed -n` | Quote exact lines |
| 4 web searches, 4 page fetches | WebSearch, WebFetch | URLs with quoted sentences |
| View `phone-0710.png` | Read | The Termius key row and the stacked status copies |
| 11 experiments on 10 private sockets | Bash plus a python3 pty harness | Two clients of different sizes need real ptys; `capture-pane` excludes the status line, so each client's own pty stream was read |

Sockets, all `-f /dev/null`, never the live server, all killed: `pp1956619`, `pp2-1956619` through `pp9-1956619`, `ppA-1956619`, `ppB-1956619`. Scripts kept as evidence: `/tmp/phone-practice-1956619-rig.py`, `-exp1.py` through `-exp11.py`.

## Leak sweep

```
$ ls /tmp/tmux-1000/ | grep 1956619
(none)
$ ps -eo pid,args | grep -E "tmux -L pp" | grep -v grep
(none)
$ tmux ls
henosis: 2 windows (created Wed Aug 26 07:37:43 2026) (attached)
tests: 1 windows (created Wed Aug 26 08:09:32 2026) (attached)
```

## Did / Did not / Could do next

Did: verified 18 manual items against tmux 3.7c; ran 11 experiments across 10 private sockets; measured both candidate shapes end to end. Found four things the issue lacked: the string-versus-numeric trap, the per-client `ignore-size` flag that refutes the issue's constraint line, the grouped-session failure, and the bottom-status cursor-walk capture.

Did not: touch any repo file or the live server. Fetch the tmux-users list or tmux GitHub issues on small clients. Exercise `display-menu`, `terminal-overrides`, `terminal-features`. Reproduce the `aggressive-resize` claim from the search snippet (PLAUSIBLE).

Could do next: have the owner run `tput cols; tput lines` inside the phone pane, since the recommendation turns on width being honest while height is not, inferred from the issue's numbers rather than measured on the device. Test `set -g status-position top` against the real phone. Measure whether a two-row status reads better than truncation at 131 columns, given each extra row costs the window a row.

Sources: https://elliotbonneville.com/phone-to-mac-persistent-terminal/ , https://getmoshi.app/guides/tmux , https://docs.blink.sh/basics/tips-and-tricks , https://mtmux.com/blog/tmux-from-phone , https://github.com/smanask/Termius-Documentation/blob/master/android/features/extended_keyboard.md
