# Shift-letter shortcuts, and move-window in the `m` menu (2026-08-26)

Issue #43. `prefix M` (move window, #20) turned out hard to press: prefix,
then Shift, then a letter. The owner wants it reachable from the `prefix m`
session menu and every Shift-letter shortcut audited against key-binding
practice. Two agents gathered the evidence (repo inventory, external
practice); the menu-row mechanism was measured on private `-L` servers. The
live server was read only.

## Facts

### What we own

| key | table | does | option | since |
|---|---|---|---|---|
| `b` | prefix | branch picker popup | none | `cockpit.tmux:236` |
| `S` | prefix | sessions popup (`scripts/tm`) | none | `cockpit.tmux:242` |
| `J` | prefix | jump to the pane that needs you | `@cockpit_jump_key`, `off` | `cockpit.tmux:245` |
| `r` | prefix | review popup | `@cockpit_review_key`, `off` restores stock | `cockpit.tmux:264` |
| `i` | prefix | pane info | `@cockpit_info_key`, `off` restores stock | `cockpit.tmux:279` |
| `f` | prefix | side fix | `@cockpit_fixer_key`, `off` restores stock | `cockpit.tmux:290` |
| `R` | prefix | reload every loaded config file | `@cockpit_reload_key`, `off` | `cockpit.tmux:331` |
| `M` | prefix | move window menu | `@cockpit_move_key`, `off` restores stock | `cockpit.tmux:378` |

`examples/full.tmux.conf` adds `H/J/K/L` (resize, `-r`, `:74-77`), `V`/`B`
(layouts, `:81-82`), `f` (tm popup, `:90`) and the `m` session menu
(`:106-127`). The plugin loads last, so its `J` and `f` silently win: the
example's resize-down and tm popup are dead on any box running that file.
`examples/tmux.conf` binds no keys.

### What is free

Stock tmux 3.7c binds five uppercase prefix letters in total: `C`, `D`, `E`,
`L`, `M`. Free lowercase in stock: `a b e g h j k u v y`. After
`full.tmux.conf` plus the plugin plus tpm and tmux-yank: free lowercase
`a e g u v`, free uppercase `A F G N O P Q T W X Z`.

Inside the `m` menu the accelerators taken are `\ - z V B x c , X t b s $ d r
Q`; `m n w p j k l a e g u y` are free.

### What the world does

- Stock tmux: `D` choose-client vs `d` detach, `M` clear mark vs `m` mark,
  `L` last session vs `l` last window. Uppercase is the rare or "choose from
  a list" variant of its lowercase twin. `T` (pane title) breaks the rule.
- tmux wiki, Modifier Keys: "tmux doesn't know the keyboard layout, so it
  treats `!` as `!`". tmux binds the character the terminal sends. Shift+M on
  a Hebrew layout is not `M`. Caps Lock inverts it. This is the owner's
  complaint, and it is a correctness gap, not taste.
- Plugins: tpm `I`/`U`, sessionist `C`/`X`/`S`, pain-control `H/J/K/L`
  resize, yank `Y`. Uppercase is common for rare or destructive actions.
  Renames happen with a changelog line (tmux-yank 2.0.0 moved to `Y`, no
  deprecation cycle) or an `@option` the user sets (resurrect, pain-control).
  Our `@cockpit_*_key` options are that second pattern already.
- `display-menu` was added for discoverability (tmux CHANGES). tmux has no
  which-key overlay; `tmux-which-key` exists to fill that. A menu row shows
  its letter; a prefix chord has to be memorised.

### The menu row, measured

A row can replay an existing prefix binding instead of duplicating its body.
On a private server with `full.tmux.conf` plus the tree's `cockpit.tmux`:

```
"Move window..."  m  "switch-client -T prefix ; send-keys -K '#{?#{@cockpit_move_key},#{@cockpit_move_key},M}'"
"Reload config"   r  "switch-client -T prefix ; send-keys -K R"
```

`prefix m m` opened the move menu (`key M: set-option -Fg @cockpit_move_src`
in the log), with `@cockpit_move_key` unset and rebound to `W`. `prefix m r`
ran the plugin's reload (`config reloaded`), which sources every
`#{config_files}` entry, unlike the row's current `source-file ~/.tmux.conf`.
No `#` escalation, no second copy of the `run-shell -C` body. Folding the
body into the row would add a third expansion pass (`####{` becomes eight)
and needs the `set -gF` pin inside one argv word; not worth it.

## Per-key audit

Keep / skip / replace, cheapest first.

| key | now | verdict | why |
|---|---|---|---|
| `M` move | Shift chord | keep the binding, add the `m` menu row `Move window... m` | the row costs one line and replays the binding; `prefix m m` needs no Shift; the `M` key stays for whoever learned it and for `@cockpit_move_key` |
| `J` jump | Shift chord, kills the example's `J` resize-down | replace default with `a` | free in stock and after the full config; "agent" / "attention"; `@cockpit_jump_key J` restores; resize-down comes back for free. tmux-sensible binds `a` to last-window, which the option already covers |
| `R` reload | Shift chord | keep, and make the menu row `Reload config r` replay it | rare action, its daily twin `r` is review: exactly the stock `D`/`d` shape. The row gives a Shift-free path and the smarter reload |
| `S` sessions | Shift chord, no option | replace default with `g`, add `@cockpit_sessions_key` | free after the full config; sessionist's "goto session" mnemonic; `prefix m s` already reaches it. `S` restores via the option |
| `b`, `r`, `i`, `f` | lowercase | keep | nothing to do |
| `H/J/K/L` resize (example) | Shift chords | keep | pain-control convention, `-r` repeat, vim mnemonic; the alternative is mouse drag. `J` revives once jump moves |
| `V`/`B` layouts (example) | Shift chords | delete the prefix keys | the `m` menu already has both rows; deletion over addition |
| menu letters `V B X Q` | Shift inside the menu | keep | the letter is on screen, no memorising; `X` and `Q` are kills and the Shift is a speed bump, sessionist's `X` precedent |

Net: two default renames (`J` to `a`, `S` to `g`), one new option
(`@cockpit_sessions_key`), two menu rows changed in the example, two example
keys deleted. Every renamed key keeps the old letter one `set -g` away.

## Not doing

- Not folding the move body into the menu row. Measured cost above.
- Not renaming `R`. Rare, and the menu row covers the Shift-free path.
- Not touching `b`. Lowercase, no complaint, and adding `@cockpit_branch_key`
  is an option nobody asked for.
- Not shipping the `m` menu from the plugin. It lives in the example config
  and in the owner's `~/.tmux.conf`, which has drifted from the example; the
  owner's file gets the same two rows by hand, or by copying the block.
- Not a which-key overlay. `tmux-which-key` exists for that.
- Not a deprecation cycle. tmux-yank precedent: changelog line plus the
  option.

## Plan

Branch `feat/lowercase-keys`, worktree `~/.cache/cockpit-wt/lowercase-keys`.
Fail-first with the two-server rig from `tests/run.sh:2112`; private `-L`
servers only.

1. `examples/full.tmux.conf`: `Move window... m` row (option-aware, above)
   after `Rename window`; `Reload config r` replays `R`; delete the `V`/`B`
   prefix binds. Tests at `tests/run.sh:223` and `:240` (the `m` menu) must
   stay green; add one assert that `prefix m m` opens the move menu.
2. `cockpit.tmux:245`: jump default `a`; README row and `@cockpit_jump_key`
   text; a `list-keys` assert for the default and for `@cockpit_jump_key J`
   (the inventory found neither exists today).
3. `cockpit.tmux:242`: sessions default `g` behind `@cockpit_sessions_key`,
   `off` disables; update the `prefix S opens the session picker` block at
   `tests/run.sh:2068`; README key table gains the row it never had.
4. README: a short "keys moved" note under the key table (`J` is `a`, `S` is
   `g`, the one-liners to get them back).
5. Owner's `~/.tmux.conf`: the two menu rows, applied after the merge with
   the owner watching, then `prefix R`.
6. `make test-par`, freeze rig on branch and master (`fixed-example.conf`),
   PR with the per-key table in the body.

Open for the owner: `a` and `g` are proposals; `u`/`e` are the other free
letters if either mnemonic does not land.
