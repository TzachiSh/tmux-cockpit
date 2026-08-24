# Lens: TCSM — FRONT END (what it gives you, and how it feels)

Companion to `tcsm.md` (mechanisms — read that first; not repeated here). This one is
surfaces only: every popup, row, key, string, option, install step. Claims tagged
**[E]** evidence (read from source / rendered live / measured) or **[I]** inference.

Source read at `scratchpad/tcsm` @ v1.2.0. Everything below rendered live on a private
tmux server (`tmux -L tcsmui -f /dev/null new -d -x 140 -y 40`, killed after) by feeding
a hand-made rows file in `agents.sh`'s exact TSV shape into `picker.sh`'s exact `fzf`
invocation, against real panes so `capture-pane` previews resolve. **I cannot view the
two images in `docs/` — `screenshot.jpg` (3800×2138) and `bell-forwarding.png`
(1112×526) [E: header parse]. Everything visual below is either my own render or the
README/article prose describing them.**

---

## 1. The complete surface inventory

TCSM has exactly **four** user-facing surfaces. That is the whole product.

| # | surface | invoked by | file:line |
|---|---|---|---|
| 1 | **Launch popup** — a `claude` session attached inside `display-popup` | `prefix y` | `launch.sh:37` |
| 2 | **Agent picker** — fzf list + live preview, in `display-popup` | `prefix u` | `list.sh:25-28` → `picker.sh:38` |
| 3 | **`display-message` toasts** — 4 distinct strings | error paths | below |
| 4 | **A bell** on the origin window's status tab | `alert-bell` hook | `bell.sh:29` |

No status-bar segment, no borders, no window-name decoration, no banner, no counter.
TCSM writes **nothing** to the ambient UI [E: grep for `status`/`set -g status` in the
repo returns nothing]. If the picker is closed, the only thing that can tell you an
agent needs you is the tmux bell highlight on one window tab.

---

## 2. Surface 1 — the launch popup (`prefix y`)

```
launch.sh:19   session="${prefix}$(session_hash "$path")"    →  claude-3f9a2b1c
launch.sh:31   tmux new-session -d -s "$session" -c "$path" "$cmd"
launch.sh:35   tmux set-option -t "$session" @claude_origin "$window"
launch.sh:37   tmux display-popup -w 90% -h 90% -E "tmux attach-session -t '$session'"
```

**What you see:** a 90%×90% popup containing a plain `claude` TUI. No title bar text —
`display-popup` is called **without `-T`**, so the popup border is unlabelled [E:
`launch.sh:37`]. Compare cockpit, which titles every popup (`cockpit.tmux:226`
`-T ' switch branch · #{b:pane_current_path} '`, `:249` `-T ' review · … '`). TCSM's
popup gives you no clue which directory's agent you are looking at once it's open —
you have to read it off Claude's own header.

**How you leave it:** `prefix d` (stock detach). The README documents this as if it were
a plugin key [E: `README.md:71`, row `prefix + d | Close the popup and go back to your
window, Claude session keeps running`]. It is not; it is tmux's own binding, and it works
because the popup is holding a real attached client. Honest framing, but it means the
"three keys" of the plugin are really two plus a borrowed one.

**Re-press `prefix y` while inside a Claude popup:**

```
🫪 Popup window already open
```

[E: `launch.sh:22`]. That is the literal string, emoji included — a `🫪` (U+1FAEA,
"shaking face"), which is the wrong emoji for the message and renders as tofu on many
terminals [I]. The message is also **wrong about what it did**: it did nothing, and it
does not tell you the useful thing (which session you're already in, or that `prefix u`
would let you switch).

Other strings on this path:

```
tmux-claude-session-manager: $path no longer exists     launch.sh:28
```

**Ease notes.** Good: one key, zero arguments, works from any pane, idempotent
(re-press re-attaches rather than spawning a second agent). Bad: the session name is
`claude-3f9a2b1c` [E: `helpers.sh:21-32` md5 of the path], so the moment you open
`choose-tree` or any session picker, your agents are a wall of hex. And exactly one
launchable agent per directory — a second in the same repo has to be started by hand,
and then shows up as `loose`.

---

## 3. Surface 2 — the agent picker (`prefix u`). **The whole product.**

### 3.1 The exact invocation

```sh
picker.sh:38  sel=$("$DIR/agents.sh" | fzf --ansi --delimiter='\t' --with-nth=5,6,7,8 \
picker.sh:39    --reverse --cycle --header='Claude agents · enter: jump · ctrl-x: kill' \
picker.sh:40    --preview='tmux capture-pane -ept {2}' --preview-window='up,70%,follow' \
picker.sh:41    --bind="ctrl-x:execute-silent(kill {3})+reload(sleep 0.3; $self --list)"
```

Row shape from `agents.sh:57`:

```
rank \t pane_id \t pid \t kind \t icon \t age(%5s) \t loc \t path
  0      %1      12841  loose   ●waiting   4m    host:1.0  ~/code/webapp
  └──────┴───────┴──────┴─ hidden ─┘  └──────── shown (--with-nth=5,6,7,8) ────────┘
```

### 3.2 Live render, 140×40 (default 90%×90% popup ≈ 126×36)

Rendered here, real fzf 0.44.1, real `capture-pane` preview against a real pane:

```
╭──────────────────────────────────────────────────────────────────────────────────────╮
│ > refactor the auth middleware to use the new token store                      15/40 │
│                                                                                      │
│ ⏺ Read(src/auth/middleware.ts)                                                       │
│   ⎿  Read 214 lines                                                                  │
│                                                                                      │
│ ⏺ Update(src/auth/middleware.ts)                                                     │
│   ⎿  Updated with 3 additions and 7 removals                                         │
│                                                                                      │
│ Do you want to make this edit to middleware.ts?                                      │
│ ❯ 1. Yes                                                                             │
│   2. Yes, and don't ask again this session                                          ││
│   3. No, tell Claude what to do differently (esc)                                   ││
│                                                                                     ││
│ ╭─────────────────────────────────────────────╮                                     ││
│ │ >                                           │                                     ││
│ ╰─────────────────────────────────────────────╯                                     ││
│   ? for shortcuts                                                                   ││
│                                                                                     ││
│              (… 10 more blank lines: the source pane's unused screen …)             ││
╰──────────────────────────────────────────────────────────────────────────────────────╯
>
  6/6 ───────────────────────────────────────────────────────────────────────────────────
  Claude agents · enter: jump · ctrl-x: kill
> ● waiting          4m   host:1.0        ~/code/webapp
  ● waiting         11m   host:5.0        ~/dev/mobile-app
  ● idle             2m   host:2.0        ~/code/api
  ● idle            37m   host:4.0        ~/notes
  ●   ?               -   host:0.0        ~/.tmux/plugins/tmux-cockpit
  ● working          0m   host:3.0        ~/code/docs-site
```

[E: `tmux capture-pane -pt %0` of the live render]

Colours (lost in ASCII): `●` is **yellow** for waiting, **green** for idle, **red** for
working, **grey** for `?` [E: `agents.sh:46-49`, `\033[33m`/`32m`/`31m`/`90m`]. The
word beside it is padded to 7 chars so the columns line up (`idle   `, `  ?    `).

**The single best thing in this UI, stated plainly:** the top row is the agent that
needs you, and its live screen is already on display above it showing you *the actual
permission prompt*. From keypress to "I can answer this" is one keystroke and zero
reading. That is the product.

### 3.3 Live render, 90 columns (narrow pane / small popup)

```
╭────────────────────────────────────────────────────────────────────────────────────────╮
│                                                                                  15/40 │
│                                                                                        │
│ > refactor the auth middleware                                                         │
│ ⏺ Update(src/auth/middleware.ts)                                                       │
│   ⎿  +3 -7                                                                             │
│ Do you want to make this edit?                                                         │
│ ❯ 1. Yes                                                                              ││
│   2. Yes, and dont ask again                                                          ││
│   3. No, tell Claude what to do (esc)                                                 ││
│ ╭───────────────────────────────╮                                                     ││
│ │ >                             │                                                     ││
│ ╰───────────────────────────────╯                                                     ││
│                       (… blank remainder …)                                           ││
╰────────────────────────────────────────────────────────────────────────────────────────╯
>
  2/2 ───────────────────────────────────────────────────────────────────────────────────
  Claude agents · enter: jump · ctrl-x: kill
> ● waiting          4m   host:1.0        ~/work/acme-platform/services/billing-gateway..
  ● idle             2m   very-long-session-name-here:12.3        ~/code/api
```

[E: same method, server restarted at `-x 90 -y 40`]

**Two real defects visible only when you run it, both confirmed live:**

1. **The columns are not columns — they are tab stops.** `agents.sh` pads only the age
   field (`%5s`, `agents.sh:57`). `loc` and `path` are joined by fzf with the raw `\t`,
   so `very-long-session-name-here:12.3` shoves the path column eight characters right
   and the grid breaks [E: row 2 above vs row 1]. With 6+ agents across sessions with
   different name lengths, the path column zig-zags.
2. **Long paths truncate at the tail** — `…/billing-gateway..` [E]. The leaf directory
   is the identifying part of a path, and it is exactly what gets eaten. `~/w/a/s/billing-
   gateway/internal/adapters` (head-elide) would be strictly better, and cockpit already
   has the machinery for it (`#{b:pane_current_path}` in its popup titles,
   `cockpit.tmux:226`).

### 3.4 The empty state — a dead end

`agents.sh` `exit 0`s silently when `claude agents --json` fails or returns nothing
(`agents.sh:20`, `:23`). `picker.sh` pipes that empty stream straight into fzf. Live:

```
╭────────────────────────────────────────────────────────────────────────────────────────╮
│                                                                                        │
│                        (a full-height, entirely empty preview box)                     │
│                                                                                        │
╰────────────────────────────────────────────────────────────────────────────────────────╯
>
  0/0 ───────────────────────────────────────────────────────────────────────────────────
  Claude agents · enter: jump · ctrl-x: kill
```

[E: rendered with `< /dev/null`]

You get a large empty popup and `0/0`. It does not distinguish "no agents running"
from "`claude agents` failed", "Claude is older than 2.1.139", or "the supervisor is
down". You press `esc` and are none the wiser. Cockpit does this **right** and should
keep doing it: `tmux-review:660` `"tmux-review: no agent pane has uncommitted changes"`,
and `tmux-jump:19` `"no agent needs you"` — both say what happened and cost nothing.

### 3.5 Keys inside the picker

| key | action | where |
|---|---|---|
| `↑`/`↓`, `ctrl-j/k` | move (fzf default, `--cycle` wraps) | `picker.sh:39` |
| *any printable* | fuzzy filter (fzf default — **not** `--disabled`) | — |
| `enter` | jump to the agent | `picker.sh:44-74` |
| `ctrl-x` | `kill {3}` — SIGTERM the pid — then `reload(sleep 0.3; …)` | `picker.sh:41` |
| `esc` / `ctrl-c` | close | fzf default |
| — | **no preview scrolling. no preview toggle.** | [E: absent from `:38-42`] |

That last row matters. The preview is 70% of the popup, it is `follow`-anchored to the
bottom, and there is **no way to scroll up in it**. If the thing you need to read
scrolled off, your only move is to jump to the pane. Cockpit binds
`ctrl-/:toggle-preview, ctrl-u/ctrl-d:preview-half-page` on the fleet queue
(`tmux-review:681`) and `alt-u/alt-d` in the review loop (`:403`) — cockpit is ahead here
and should not regress.

**`ctrl-x` is unconfirmed and irreversible.** One keystroke SIGTERMs a Claude process.
No "kill %1 (webapp)?" prompt, no undo, and — because `kind` is hidden — no indication
that killing a `dedicated` agent also destroys its tmux session (its last window dies
with the process). The `sleep 0.3` before the reload (`picker.sh:41`) is visible as a
third of a second where the row you just killed is still on screen [E: read; not timed].
Cockpit's `tm` had this exact class of accident and fixed it deliberately — see the
comment at `tm:161-166`: *"a bare `k` — the first key of filtering to `kanban` — killed
the highlighted session instantly … Kill lives on ^X now"*. Cockpit additionally
**labels and refuses to kill the attached session** (`tm:169` appends `(attached)`).
TCSM has no such guard.

### 3.6 The hidden field that decides everything

`--with-nth=5,6,7,8` hides fields 1–4, which includes `kind` ∈ `dedicated|loose`
(`agents.sh:52`). But `kind` selects between two **completely different** `enter`
behaviours (`picker.sh:51-74`):

- `loose` → `switch-client` + `select-window` + `select-pane`; the popup closes and you
  are looking at the pane, in place.
- `dedicated` → move the *parent* client to `@claude_origin`, then
  `tmux attach-session` **inside the picker popup you are still in** — so the agent
  appears in a 90% popup, not in your layout.

Same key, same-looking row, two different outcomes and two different "where am I now"
answers. There is no glyph, no colour, no column telling you which you'll get. **This is
the single largest usability hole in the picker** [E: `picker.sh:38` `--with-nth` vs
`:51`/`:62`].

### 3.7 Header wording

```
Claude agents · enter: jump · ctrl-x: kill
```

Titles the object, then lists two keys, `·`-separated. Compare cockpit:

```
tmux-review:685   which agent needs you   ↵ review it   ^/ preview   esc cancel
tm:184            ↵ attach   ^X kill   esc cancel
tmux-review:407   alt-↵ comment   ^/ preview   alt-u/alt-d scroll   ^S deliver   esc discard (enter does nothing)
```

Cockpit's are better on three counts, and the gap is worth naming: cockpit uses the
**glyph** (`↵`, `^X`) rather than the spelled word, it names the **outcome** rather than
the verb (`review it`, not `review`), and it always documents **`esc`** — TCSM never tells
you how to get out. TCSM is better on exactly one: it names the object ("Claude agents"),
which cockpit's `tm` prompt does only via `session> `.

### 3.8 Prompts

TCSM: `> ` (fzf default, never set) [E: no `--prompt` in `picker.sh:38-42`].
Cockpit: `fleet> `, `session> `, `kill session> `, `review> ` (`tmux-review:684`,
`tm:182`, `tm:133`, `tmux-review:406`). Cockpit's typed prompt is a free, always-visible
"which picker am I in" label. TCSM's bare `>` gives you nothing — inside an unlabelled
popup, with a header that scrolls out of mind, the picker has no persistent identity.

---

## 4. Surface 3 — the message strings, in full

The complete user-visible vocabulary of the plugin outside the picker:

```
launch.sh:22   🫪 Popup window already open
launch.sh:28   tmux-claude-session-manager: $path no longer exists
picker.sh:21   tmux-claude-session-manager: $tool is required for the picker
```

Three strings. The dependency one (`picker.sh:19-24`, loops `fzf jq claude`) is the good
one — it names the missing tool and exits 0 rather than dumping a shell error into a
popup that instantly closes. Cockpit does the same shape (`tmux-review:657`
`"tmux-review: the fleet queue needs fzf"`, `tm:127` silently falls back to bash
`select`). Even trade.

The `🫪` string is the only one with a tone, and it's the weakest.

---

## 5. Surface 4 — the bell

A bell rung inside a `claude-*` session is re-emitted into the origin window's pty
(`bell.sh:29` `printf '\a' >"$tty"`), so tmux paints that window's status tab with
`window-status-bell-style`. That is the entire ambient signal: **one bit, on one tab, with
no state and no memory** — the highlight clears when you visit the window, and it cannot
tell "finished" from "needs permission".

It also isn't free. Making Claude *ring* takes two settings **in two different files**
(README:110-141):

```json
~/.claude/settings.json     { "preferredNotifChannel": "terminal_bell" }
~/.claude.json              { "messageIdleNotifThresholdMs": 0 }
```

…plus README prose explaining that `iterm2`/`kitty`/`ghostty` channels *don't* produce a
real `\a`, that `auto` picks by terminal so you must pin it, and that the second key
lives in the global config because *"settings.json ignores this key and drops it
silently"* [E: README:130-131]. Then an alternative section offers a `Stop` hook instead,
with a 200-character `printf '\a' > /dev/tty || …` command to paste (README:158).

This directly contradicts the plugin's headline promise, **"Status needs no
configuration … there are no hooks to install"** (README:27-28). The status column needs
none; the only thing that can notify you when the popup is *closed* needs two files, a
documented footgun about silent key-dropping, and optionally a hook after all. **[E]**

Cockpit's equivalent surface is not comparable in size: `▲`/`✕` markers, a red pane
border, the whole-pane tint (`@cockpit_blocked_bg`, README:627), the cross-window
`NEEDS YOU` banner, `#{agent_fleet}` counter, and `prefix J` to walk them
(`tmux-jump:1-40`). All from hooks cockpit already installs.

---

## 6. Config surface — 9 options, one copy-paste block

This is TCSM's best *documentation* artefact, reproduced verbatim (README:92-102):

```tmux
set -g @claude_launch_key     'y'        # prefix key: launch/open for current dir
set -g @claude_list_key       'u'        # prefix key: open the picker
set -g @claude_command        'claude'   # command run in new sessions
set -g @claude_args           ''         # extra args appended to the command
set -g @claude_session_prefix 'claude-'  # tmux session name prefix
set -g @claude_popup_width     '90%'     # popup width
set -g @claude_popup_height    '90%'     # popup height
set -g @claude_fzf_options    ''         # extra options passed to the fzf picker
set -g @claude_forward_bell   'on'       # highlight the origin window on a bell
```

Nine options, defaults shown, one-line comment each, **aligned**, and it is
copy-pasteable as-is into `tmux.conf` — the defaults being the values means pasting the
whole block is a no-op. Every knob the plugin has is in one screen.

Cockpit has **no such block**. Its options are prose-scattered across 694 README lines:
`@cockpit_agents` at :644, `@cockpit_open_single` at :333, `@cockpit_popup_width/height`
at :345, `@cockpit_blocked_bg` at :627, `@cockpit_editor` at :466, and the key overrides
(`@cockpit_jump_key`, `@cockpit_review_key`, `@cockpit_reload_key`) only as trailing
clauses inside a keybinding table's cells (README:253-255) [E: grep `@cockpit_` README.md].
To learn cockpit's knobs you must read the manual. That is a real, cheap gap. **[E]**

### 6.1 `@claude_fzf_options` — the escape hatch, and its worked example

`picker.sh:32-33` `eval`s the option into an array and splices it into fzf. The README
then spends 35 lines (:178-211) on **one** worked example: a modal vim-keys picker.

| key | action | (README:198-205) |
|---|---|---|
| `j`/`k` | move down/up |
| `i`/`a` | switch to **filter** mode |
| `x` | kill the highlighted agent |
| `q` | close the picker |
| `enter` | jump (both modes) |
| `esc` | filter → nav |

Implemented with `--bind 'i:unbind(j,k,q,i,a,x)+change-prompt(filter> )'` and the
inverse on `esc`. It also exports **`$CLAUDE_PICKER`** (`picker.sh:28`) purely so a
user's own binding can call `reload($CLAUDE_PICKER --list)` — and then documents the
tmux-quoting trap (`\$CLAUDE_PICKER` in a double-quoted `set -g` value, bare in a
single-quoted one, README:208-211).

**This is a genuinely thoughtful piece of design and cockpit has no equivalent.** All
five cockpit pickers hard-code their fzf lines (`tmux-review:401-407`, `:679-685`,
`tm:180-187`, plus tmux-branch/tmux-open-path). A user who wants `j`/`k` has to fork.
Note the irony: cockpit *deliberately* chose "typing filters" over a command mode after a
data-loss incident (`tm:161-166`), and TCSM's flagship customisation example is exactly
the mode cockpit removed — so cockpit should steal the *hook*, not the *example*.

---

## 7. Install & onboarding, step by step

```
1.  set -g @plugin 'craftzdog/tmux-claude-session-manager'     (~/.tmux.conf)
2.  prefix + I
3.  prefix + y                                                  → an agent is running
4.  prefix + u                                                  → the picker
```

Four steps, one of them typing a line, two of them keypresses. Prerequisites are `tmux
≥3.2`, `fzf`, `jq`, `claude ≥2.1.139`, bash — listed with links and a
`claude --version` hint (README:30-37).

The README puts a **keybinding-collision warning immediately under the install block**
(README:49-52): *"by default the plugin binds `prefix`+`y` (launch) and `prefix`+`u`
(list). If your config binds those elsewhere, either change the options below, or make
sure the plugin loads after your own bindings."* Anticipating the first failure mode at
the point of failure is good README craft. (It is also necessary: `y` and `u` are heavily
contested — `y` is copy in vi-mode configs, `u` is a common "urls"/`tmux-fzf-url`
binding [I].)

Cockpit's install is longer and materially harder: clone → `install.sh` (symlinks three
commands into `~/.local/bin`) → a `run-shell` line **that must be ordered after every
`#{agent_*}`-using option and before tpm** → a quoting caveat for paths with spaces →
`install.sh --hooks` to merge 11 Claude hooks into `~/.claude/settings.json` → a
catppuccin ordering constraint (README:150-199). Six steps, three of them
order-sensitive. Every one of those steps buys something TCSM doesn't have; none of them
is free.

---

## 8. The docs, the screenshots, and the article

**I cannot view the images.** From the README prose around them:

- `docs/screenshot.jpg` (3800×2138) is the hero image at README:3, wrapped in a link to
  `https://youtu.be/NnTV6r4l5D0`. Its surrounding text is the feature bullet list
  (README:13-25). From those bullets it is the picker: rows of agents with coloured status
  dots and a live preview of one agent's screen [I from README:13-19].
- `docs/bell-forwarding.png` (1112×526) sits under **"### Making Claude ring the bell"**
  (README:112) and its surrounding text is the two-file settings explanation, so it is a
  diagram of the bell path (dedicated session → `alert-bell` hook → origin window tab)
  rather than a screenshot [I].

**The article** — `devas.life/i-made-a-claude-code-session-manager-for-tmux/` [E:
WebFetch; the URL in the brief 404s]. Motivation, verbatim: *"it's been annoying to check
whether any sessions are finished or need my answers by switching windows."* It frames the
whole thing through Christoph Nakazawa's *"working with coding agents is basically the
same as working in a large organization"* — tight feedback loops. The picker is described
with 🔴 working / 🟡 waiting / 🟢 idle, live preview in the right column, attention-first
sort, `enter` to jump, `ctrl-x` to kill.

**The article is stale in a way that matters for discoverability.** It states that
*"Claude Code hooks write status (`@claude_state`) directly to tmux session options"* and
that users *"wire up Claude Code hooks by adding a block to `~/.claude/settings.json`"*
[E: WebFetch]. v1.1+ removed all of that for `claude agents --json`, and the repo's own
upgrade note tells users to *delete* that block [E: `tcsm.md` §4, PR #14 breaking
changes]. The highest-traffic entry point to this plugin — 368 stars' worth of
referral — instructs you to install hooks the plugin no longer reads. It also says the
preview is in the *right* column; it is `up,70%` [E: `picker.sh:40`].

---

## 9. Honest ease-of-use ratings

### Install — **4 / 5**
One `set -g @plugin` line + `prefix I`. Zero hooks, zero `settings.json`, zero
post-install commands [E: README:39-52]. Four external prerequisites, all common, all
listed with version floors. −1 because the two default keys are contested enough that
the README has to warn about them on the same screen (README:49-52), and because the
warning's own remedy ("load the plugin after your bindings") is load-order advice a tpm
user cannot easily follow.

### First run — **4 / 5**
`prefix u` works immediately with no configuration, and the status column is populated
on the first press [E: README:27-28 and `agents.sh:20` — no state to warm up]. The
header names both keys you need. −1 entirely for the empty state (§3.4): the most likely
first-run failure — Claude older than 2.1.139, or nothing running yet — renders as a
silent empty box, with the one diagnostic string (`$tool is required`) covering only a
*missing binary*, not a *too-old* one, despite the README specifying a version floor
[E: `picker.sh:19-24` checks `command -v claude`, never `claude --version`].

### Day-to-day — **3 / 5**
The core loop is genuinely excellent: one key, needs-you first, the prompt already
readable in the preview, `enter` to answer. Against that: **0.5–2.5 s** to open on every
press [E: measured in `tcsm.md` §2b] — long enough to feel, on a key you press dozens of
times a day; no preview scrolling at all (§3.5); wobbling columns and tail-truncated
paths (§3.3); an unconfirmed irreversible `ctrl-x`; and `enter` doing one of two very
different things with no way to predict which (§3.6). Also: the picker is **modal-only**.
Close it and you are blind again until a bell — which requires the two-file setup the
plugin says it doesn't need (§5).

### Discoverability — **3 / 5**
In-app: the fzf header names `enter` and `ctrl-x` and is always visible; that covers the
picker. But `esc` is never mentioned anywhere; the bare `>` prompt gives the popup no
identity; and `dedicated`/`loose` is load-bearing and invisible. Out-of-app: the README
is well-organised (Usage table → Options block → "How it works" narrative) and its option
block is exemplary — but the *article*, which is how most people arrive, documents a
mechanism the plugin removed two versions ago (§8). A user who follows the article
installs hooks that do nothing and gets no error.

**Composite: a very good 15-minute experience and a merely adequate 15-hour one.** Every
weakness above is a consequence of the same choice — everything lives inside one modal
popup — and every strength is a consequence of ruthless scope: four surfaces, three
strings, nine options.

---

## 10. Direct comparison: TCSM picker vs cockpit fleet queue vs `tm`

### The rows, side by side

```
TCSM  agents.sh:57 / picker.sh:38   (--with-nth=5,6,7,8)
  ● waiting          4m   host:1.0   ~/code/webapp
  ─┬──────         ──┬─   ───┬────   ──────┬──────
   status glyph+word  age   location       cwd

cockpit  tmux-review:616 / :678     (--with-nth=2,3)      ← LIVE, right now
  henosis:2 tmux-open-path and tmux-branch picker UX      +0 -0  ?2 new
  henosis:3 Claude Code                                   +0 -0  ?2 new
  henosis:4 Replace all to go or rust                     +0 -0  ?2 new
  ────┬────  ──────────────┬───────────────             ──────┬──────
   location            pane_title / window_name            git churn

cockpit  tm:169 / :181             (--with-nth=2)
  henosis (attached)
  ──┬───   ────┬────
  session   attached marker
     preview:  2: nvim  [needs-input]     ← tm:177 PVFMT, @panestate per agent pane
               3: claude  [running]
```

[E: TCSM rows rendered live; cockpit rows are real output of `./scripts/tmux-review
--fleet` on this machine]

### What each row answers

| question | TCSM | cockpit fleet | cockpit `tm` |
|---|---|---|---|
| **is it waiting on me?** | ✅ glyph, first column, sorted first | ❌ **not in the row at all** | ✅ but only in the *preview*, per window |
| how long since it moved | ✅ age column | ❌ | ❌ |
| where is it | ✅ `loc` + `cwd` | ✅ `session:window` | ✅ session name |
| what is it *doing* | ✅ live screen preview | ✅✅ **the actual diff** (`--preview-diff`) | ⚬ window list |
| how much has it changed | ❌ | ✅ `+a -d ?n new` | ❌ |
| what is it working *on* | ⚬ cwd only | ⚬ `pane_title`, often useless | ❌ |

### Three findings from putting them next to each other

**(1) Cockpit's fleet queue is a *dirty-tree* queue, not a *needs-you* queue.**
`tmux-review:591-592`:

```sh
st=$(git -C "$path" status --porcelain 2>/dev/null)
[ -n "$st" ] || continue
```

An agent sitting on a permission prompt with a **clean tree is not in the list at all.**
`prefix r` from a non-agent pane opens the fleet queue (`tmux-review:711-718`), and it
will tell you `"no agent pane has uncommitted changes"` while three agents are blocked.
Cockpit answers "who needs me" via `prefix J` (`tmux-jump`), which teleports you to one
pane and shows you **no list** — you cannot survey, only cycle. TCSM's `prefix u` is a
strictly better shape for that question. **[E]**

**(2) Cockpit's fleet sort has, right now, nothing to sort by.** `tmux-review:620`
sorts `-k1,1nr` on churn. All three live rows are `+0 -0 ?2 new` — churn 2, 2, 2 [E].
The order is whatever `list-panes -a` emitted. And `@panestate` — which cockpit has, per
pane, at zero cost, and which `tm`'s preview already renders (`tm:177`) — is not consulted
by `fleet_rows` at all.

**(3) The middle column is cockpit's weakest and TCSM's strongest-by-default.** Live
row 2 reads `henosis:3 Claude Code` [E] — the literal unset default. TCSM never has this
problem because it doesn't try: it shows the **cwd**, which is always meaningful, never
truncated at the head, and needs no cooperation from the agent. (`tcsm.md` §3.1's
`ai-title` is the better fix; the cwd is the free one available today.)

---

## 11. Ranked steal-list for cockpit

Ranked by (value / cost). Each names what it replaces, with a file:line.

---

### 1. **Put `@panestate` in the fleet row, and sort by it** — cost **S** — [E]

**Steal:** `agents.sh:46-49` (glyph + word, colour-coded) and `:60`
(`sort -k1,1n -k6,6n` — rank first, age within rank).

**Replaces:** `tmux-review:616` (row build, no state field) and `:620`
(`sort -k1,1nr`, churn only).

`fleet_rows`'s `list-panes -a -F` format already reads `@cockpit_agents_glob`; adding
`#{@panestate}` costs **zero extra subprocesses** — it is one more `\t`-field in a format
string tmux is already expanding. Then rank `needs-input(0) < done(1) < error(2) <
running(3)`, sort rank-then-churn, and render the glyph column cockpit already owns
(`▲`/`✕`, README:621-637) as the leftmost field.

TCSM's comment at `agents.sh:61-62` is the design rationale, and it is right: *"rank asc
(what needs you floats up), then age asc so whatever just went idle sits at the top of its
group."* Reviewing a `running` agent's diff is worse than useless — it is mid-write.

**Highest value in this document.** It turns a churn list into a triage list, and cockpit
already has every input.

---

### 2. **Make `prefix r`'s fleet queue include blocked-but-clean agents** — cost **S/M** — [E]

**Steal:** the *premise* of `agents.sh` — a row per agent, unconditionally.

**Replaces:** `tmux-review:592` `[ -n "$st" ] || continue`, which silently drops them.

Keep the git columns; just don't make a dirty tree the ticket for entry. A row with
`needs-input` and an empty stat is exactly the row you most want to see. This is what
makes #1 actually pay off, and together they give cockpit TCSM's `prefix u` **without a
new key** — `prefix r` from a non-agent pane is already that gesture (`tmux-review:711`).
Cost is S if you accept `git status` still running per row; M if you want to skip the git
walk for clean panes (you can't know they're clean without it — but you *can* order the
walk so blocked panes render first).

---

### 3. **`esc` in every header, and a real prompt on every picker** — cost **S** — [E]

**Steal:** nothing — this is cockpit's own convention, applied consistently. TCSM's
omission (`picker.sh:39` header has no `esc`; no `--prompt` at all) is the negative
example.

**Audit:** cockpit is already good (`tmux-review:684-685` `fleet> ` +
`esc cancel`; `tm:182-184` `session> ` + `esc cancel`). Verify tmux-branch and
tmux-open-path match, and that the two `display-popup -T` titles stay in sync with the
prompts. Cheap consistency work; mentioned only because TCSM shows what its absence costs
inside an unlabelled popup.

---

### 4. **One aligned, copy-pasteable options block in the README** — cost **S** — [E]

**Steal:** README:92-102 wholesale — the format, the alignment, the "defaults shown"
promise, one comment per line.

**Replaces:** nothing — it *adds* the thing cockpit lacks. Today `@cockpit_agents`
(README:644), `@cockpit_open_single` (:333), `@cockpit_popup_width/height` (:345),
`@cockpit_blocked_bg` (:627), `@cockpit_editor` (:466) and the three `*_key` overrides
(:253-255, buried in table cells) are scattered over 694 lines. One block near the top,
prose kept where it is for the ones that need it.

Highest doc-value-per-minute item here. It is also the honest answer to "cockpit is hard
to configure": it mostly isn't — it's hard to *discover* the configuration.

---

### 5. **`@cockpit_fzf_options` — a user escape hatch for the pickers** — cost **M** — [E]

**Steal:** `picker.sh:31-33` (`eval` into an array, splice into fzf) and `:28`
(export `$CLAUDE_PICKER` so a user binding can call `reload`).

**Replaces:** the hard-coded fzf lines at `tmux-review:396-407`, `tmux-review:679-685`,
`tm:180-187`, plus tmux-branch/tmux-open-path. Users who want `j`/`k`, a different
preview position, or their own `--color` currently have to fork.

**Two cockpit-specific cautions.** (a) `eval` of a tmux option is an injection surface;
cockpit's threat model already treats tmux options as trusted (`@cockpit_agents_glob`),
so this is consistent, but it should be documented as such. (b) The review loop binds
`enter:ignore` deliberately (`tmux-review:401`) and `alt-enter` to comment — a user
override that rebinds `enter` would break the review contract. So: allow the option on the
**fleet queue and `tm`** (pure navigation), and refuse or ignore it on `--loop`, where a
key means something irreversible. Per-picker options (`@cockpit_fleet_fzf_options`) is the
safer shape.

---

### 6. **`capture-pane` preview as a *toggle*, never the default** — cost **M** — [I]

**Steal:** `picker.sh:40` `--preview='tmux capture-pane -ept {2}'`.

**Replaces:** nothing — it is a second preview *mode* on the fleet queue, alongside
`--preview-diff` (`tmux-review:673-675`).

The diff is right for a review queue and must stay the default; `tcsm.md` §3.5 is correct
that swapping it out would be a downgrade. But a blocked agent's diff is often empty, and
what you need is *the question on its screen*. Bind it as a **third preview state** on the
existing `ctrl-/` cycle (`tmux-review:681`): diff → screen → off. Add the preview
scrolling TCSM lacks — cockpit already binds `ctrl-u`/`ctrl-d` there, so it comes free.

One live gotcha to handle: `capture-pane -ep` emits the source pane's **full height
including trailing blank lines** — 40 lines for a 40-row pane with 18 lines of content
[E: measured]. With `--preview-window=…,follow` that renders as an empty preview. Use
`capture-pane -ep -S -N` or strip trailing blanks; do not copy `follow` blindly.

---

### 7. **The age column** — cost **S** — [I]

**Steal:** `agents.sh:51` + `helpers.sh:49-57` — last-activity as the mtime of the
session's transcript, found by glob so you never reproduce Claude's cwd→slug encoding.
Renders `4m`, `37m`, `-` for never.

**Replaces:** nothing — a new column on the fleet row (`tmux-review:616`).

"This one went idle 37 minutes ago" is a different and often better review signal than
"this one has the biggest diff". Cockpit can get it more cheaply than TCSM does: cockpit
already runs the hooks, so a `done`/`needs-input` transition can stamp `date +%s` into a
pane option and the row reads it with **zero** subprocesses — where TCSM pays a `stat` per
agent (`agents.sh:26-28`). Wait for `@panestate_tx` (`tcsm.md` §3.1) if that lands; the
option-stamp works today either way.

---

### 8. **Head-elide long paths; pad the columns** — cost **S** — [E]

**Steal:** the *lesson*, not the code — TCSM gets both wrong (§3.3) and cockpit is
about to inherit the first as it adds columns.

`~/work/acme-platform/services/billing-gateway..` loses the identifying leaf.
Head-elide (`…/billing-gateway/internal/adapters`) instead, and `printf %-Ns` every column
rather than relying on tab stops — TCSM pads only `age` (`agents.sh:57` `%5s`) and its
grid visibly breaks on a long session name [E, rendered]. Relevant to cockpit the moment
#1 and #7 add a state glyph and an age column beside the existing `session:window`.

---

### 9. **A titled launch popup, if cockpit ever launches agents** — cost **S** — [I]

**Steal:** `launch.sh:35` `@claude_origin` — record the window you launched from.
**Don't steal** the untitled popup (`launch.sh:37`, no `-T`).

Cockpit doesn't launch agents, so this is conditional. The *transferable* half is
`@claude_origin` applied to review: after `^S` delivers a batch, cockpit drops you
wherever the popup closed (`tmux-review:637-644` hands delivery to `run-shell -b`
precisely so it outlives the popup). Stamping the origin pane at popup-open and offering
"jump back to the pane you just reviewed" is one `select-pane`. Already noted in
`tcsm.md` §3.4 — repeated here because it is a **UX** gap, not a mechanism one.

---

## 12. What NOT to copy

**md5 session names** (`helpers.sh:21-32`, `launch.sh:19` → `claude-3f9a2b1c`). Unreadable
in `choose-tree`, in `tm`'s own picker, and in the picker's own `loc` column. It also
caps you at one launchable agent per directory. Cockpit's model — agents in ordinary,
named panes — is better and must not regress. Note the concrete interop cost if you *run*
TCSM alongside cockpit: `tm`'s session list (`tm:169`) would fill with hex rows [I].

**The popup-race loops** (`list.sh:41-59`): a 100-iteration `list-clients` poll at 50 ms,
then `sleep 0.1`, then up to 40 `display-popup` retries with a 1-second heuristic for
"did it actually open". Plus `picker.sh:41` `reload(sleep 0.3; …)`. These are worked-around
races, and they exist only because TCSM opens popups **inside** popups. Cockpit's
"one-shot scripts, no daemon, no polling" invariant is worth more than any feature in this
document.

**Unconfirmed destructive keys.** `ctrl-x` → `kill {3}` with no prompt, no undo, and — with
`kind` hidden — no warning that it destroys a tmux session too (`picker.sh:41` + `:38`).
Cockpit already learned this (`tm:161-166`) and additionally protects the attached session
(`tm:169`). Keep the guard; if a kill key ever lands on the fleet queue, confirm it.

**`follow` on a `capture-pane` preview** — renders blank for any pane not full to the
bottom [E: measured, §11.6].

**Hiding a field that changes what `enter` does** (§3.6). If cockpit ever gives the fleet
queue two outcomes, the row must say which.

**"No configuration required" as a claim** while the only ambient signal needs two files
and a documented silent-key-drop (§5). Cockpit's README is honest about its six install
steps; that honesty is an asset, not a liability to fix.

---

## 13. One-paragraph verdict

TCSM's front end is a masterclass in scope: four surfaces, three strings, nine options,
one key that answers one question — *"who needs me?"* — with the answer sorted to the top
and the blocking prompt already legible above it. That question is the one cockpit
currently answers **worst**: `prefix J` teleports without a list, and `prefix r`'s fleet
queue filters on a dirty tree (`tmux-review:592`) and sorts on churn (`:620`), so a
blocked agent with a clean tree is invisible and three equally-dirty agents arrive in
arbitrary order [E: live output]. Cockpit already owns every input needed to fix that —
`@panestate` per pane, at zero redraw cost, already rendered inside `tm`'s preview
(`tm:177`) — and steals #1 and #2 close the gap without a new key, a new dependency, or a
single subprocess. Everything else here is polish: the options block (#4), the escape
hatch (#5), the age column (#7). And the parts of TCSM that feel worst in daily use —
the 0.5–2.5 s open, the popup-in-popup races, the hex session names — are all downstream
of architecture cockpit already rejected.
