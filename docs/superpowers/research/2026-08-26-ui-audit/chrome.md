# Chrome outside popups: colours, popup frame, tabs, borders, messages, redraw

Read-only lane, 2026-08-26. [V] = verified on throwaway servers (`tmux -L dz1..dz6 -f /dev/null`); the live server was not touched.

## 1. Colour roles

As today:

| state | glyph | token | plugin default (cockpit.tmux) | full.tmux.conf override |
|---|---|---|---|---|
| needs-input | ▲ | @thm_red | #f38ba8 :190 | #ff6b6b :179 |
| error | ✕ | @thm_peach | #fab387 :191 | #ff9838 :181 |
| running | ● | @thm_yellow | #f9e2af :192 | #ffcc33 :182 |
| done | ✓ | @thm_green | #a6e3a1 :193 | #3dea7d :183 |
| idle | ○ | @thm_overlay_0 | #6c7086 :194 | #5b6272 :176 |
| review pending | ◆ | @thm_yellow | agent-state:371 | |
| blocked tint | bg | @cockpit_blocked_bg | #52303a :195 | |
| ctx ≥ 80 % | ◔NN% | colour208 hardcoded | tmux.conf:42-43 only | |

Painted at: tabs full.tmux.conf:199-200 and tmux.conf:42-43; border colour :216-217 / :22-23; border text :220 / :24; tint :226 / :34. The fleet string is built in shell at agent-state:369-371 with tokens left unresolved (:305-307) so a late theme still wins.

Defects:

- C1 `◆` and `●` are the same yellow (agent-state:371 vs the tab formats); "working" and "finished, unread" are opposite states that appear together.
- C2 `▲` is bold in the fleet (agent-state:369) and not in the tabs (full.tmux.conf:199).
- C3 The examples disagree about `◔`: present tmux.conf:42-43, absent full.tmux.conf. The comment defending colour208 (tmux.conf:38-41, "shows up before any theme is sourced") no longer holds: cockpit.tmux:187-196 defaults every `@thm_*` when unset and the tab format expands at render time.
- C4 The examples disagree about the ask text: tmux.conf:24 renders `─ NEEDS YOU #{=/48/…:#{@panestate_msg}}`, full.tmux.conf:220 renders `─ NEEDS YOU ` alone. README.md:17-19 advertises the message; tests/freeze-rig/fixed-example.conf:12 has it.
- C5 full.tmux.conf:171-189 is a different theme from the defaults, so README's colour column (:23-27) describes Mocha, not what the shipped example paints.
- C6 Dead palette: @thm_maroon :180, teal :184, sky :185, blue :186, lavender :187, mauve :188, pink :189 are set and nothing reads them.

Proposed, one urgency ramp and one gauge off it:

| state | glyph | token | Mocha |
|---|---|---|---|
| needs-input | ▲ bold | @thm_red | #f38ba8 |
| error | ✕ | @thm_peach | #fab387 |
| running | ● | @thm_yellow | #f9e2af |
| done | ✓ | @thm_green | #a6e3a1 |
| review pending | ◆ | @thm_sky (new) | #89dceb |
| idle | ○ | @thm_overlay_0 | #6c7086 |
| ctx ≥ 80 % | ◔ | @thm_mauve (new) | #cba6f7 |

Cost: two lines in the PALETTE heredoc (cockpit.tmux:189-196), one token at agent-state:371, `,bold` on the tab `▲` arms, colour208 → `#{@thm_mauve}` in tmux.conf.

16-colour fallback: no detection needed. cockpit.tmux:187-196 sets each `@thm_*` only when unset, so a user value wins, and tmux itself downsamples RGB to 256 when `terminal-features` carries no RGB. Only a true 16-colour terminal needs anything, and there it is six `set -g @thm_* colourN` lines before the run-shell line, plus `set -g @cockpit_blocked_bg default` to disable the tint (renders as `bg=default` through full.tmux.conf:226). Put it in the README under the theme tokens; do not add detection to the example conf.

## 2. Popup chrome

Today nothing sets `popup-style`, `popup-border-style` or `popup-border-lines`. cockpit.tmux:219-220 declines to pass `-b/-S` per popup ("the border belongs to the user's popup-border-style") but neither example sets it, so every popup gets tmux's stock single-line border in the default foreground. docs/superpowers/research/2026-08-26-review-followups.md:259-272 already established popup-border-style is a different border from the one fzf paints.

Every title as shipped: see inventory.md §A and §B. Problems: two popups with no title (full:120, :123); one sentence title (prefix M); two with a kind and no context (`' sessions '`, `' agents '`); one bare context (`#{session_name}`); three geometries for sibling popups.

Proposed, three lines in full.tmux.conf beside the pane-border block (~:209), after the palette:

```
set -g popup-border-lines rounded
set -g popup-border-style "fg=#{@thm_overlay_0}"
set -g popup-style        "bg=#{@thm_bg}"
```

`@thm_bg` is set only at full.tmux.conf:171, so `popup-style` belongs in that file alone; examples/tmux.conf gets the first two plus a commented hint. Rounded, not heavy: the popup is a passive frame; the alert vocabulary stays on the pane border. Not proposed: tinting the popup border with the source pane's state colour (the pane border under it already carries that, and it would repaint on every transition).

Titles, convention `' <kind> · <context> '`: `' sessions · #{session_name} '`, `' agents · #{session_name} '`, `'#[align=centre] move · ##{window_index}:##{window_name} '`, `"#[align=centre] menu · #{session_name} "`, `-T ' agents · ##{session_name} '` for full:120 (doubled `#`, nested in display-menu). Keep `#{pane_id}` for info and side fix: `%N` is what every runtime message prints (review-gate:147, tmux-fixer:123, tmux-review:1663), so the title matches the confirmation.

Sizes, two not five: picker = $PW × $PH (list with a preview); card = $PW × fixed rows (side fix stays `-h 6`, or 7 per screens.md). Then delete the ad-hoc ones by making the menu replay the plugin's key, the pattern full.tmux.conf:111, :116, :127 and cockpit.tmux:417 already use: `:123 "Sessions..." s` → `"switch-client -T prefix ; send-keys -K g"`, `:122 "Switch branch" b` → `"… send-keys -K b"`. Removes two duplicate popup definitions, two hardcoded geometries, one duplicated title, and the rows follow a rebound key automatically.

## 3. Tabs, border text, the truncation ladder

Budgets today: inactive 16 (full.tmux.conf:199, tmux.conf:42), current 34 (:200, :43), fixed. status-left-length 40 (:238), status-right-length 140 (:239), directory `#{=/30/…:}` (:243). A tab is roughly `▎N label ● ` with ~6 columns of overhead; status-right ~52 columns as configured (:253-260); status-left ~9.

80 columns, 3 agent windows, today: tabs need 40 + 22 + 22 = 84, available 80 − 9 − 52 = 19. tmux hard-cuts the window list with no ellipsis; windows 2 and 3 are not on screen. 120 columns, 8 windows: needs 194, available 59; five of eight invisible, including any `▲`. Only 160 columns with 3 windows fits.

Border text today, active pane with another window blocked (full.tmux.conf:220, tmux.conf:24): `─ NEEDS YOU 2:claude-2 ─────[z][x]`. Unfocused blocked pane: tmux.conf:24 `─ NEEDS YOU Claude needs your permission to use Bash ───[x]`, full.tmux.conf:220 `─ NEEDS YOU ───[x]` (defect C4).

Fleet counter: neither example renders it. full.tmux.conf:249-260 never mentions `#{agent_fleet}`; examples/tmux.conf:64 has it commented.

Proposed ladder, two options set once, then one substitution per format [V]:

```
# Per-tab label budget: what is left after the fixed chrome, split across the windows.
# 61 = status-left ~9 + status-right ~52, the same fixed chrome §3 subtracts above.
# Floored at 10 (also the safe arm when client_width is unavailable), capped at 34.
set -g @cockpit_tab_raw "#{e|/:#{e|-:#{client_width},61},#{session_windows}}"
set -g @cockpit_tab_w   "#{?#{e|<:#{E:@cockpit_tab_raw},10},10,#{?#{e|>:#{E:@cockpit_tab_raw},34},34,#{E:@cockpit_tab_raw}}}"
```

Then replace `#{=/16/…:` and `#{=/34/…:` with `#{=/#{E:@cockpit_tab_w}/…:` in both tab formats. Key fact [V]: the width argument of `#{=/N/…:}` may itself be a format (`#{=/#{@w}/…:#{E:@lbl}}` with `@w=6`, `@lbl=abcdefghijkl` → `abcdef…`), and a bare word after the colon is a variable lookup, not a literal. Budgets, `(client_width − 61) / session_windows` with tmux's integer division truncating (which the measured run showed: 64/3 came back 21, not 22), then floor 10 / cap 34 — 80/3: 19/3 = 6 → 10; 80/8: 19/8 = 2 → 10; 120/3: 59/3 = 19; 120/8: 59/8 = 7 → 10; 160/3: 99/3 = 33; 160/8: 99/8 = 12. The three renders below are unchanged by the constant: their longest label is 31 columns, under both 33 and 34.

The ladder alone does not save 80 columns (3 × 16 = 48 > 19), so status-right joins it:

```
# <100 cols: drop the directory segment (nothing to click when nothing shows)
set -g status-right "#{?#{e|<:#{client_width},100},,#[range=control|2]#{E:@catppuccin_status_directory}#[norange]}"
set -g @catppuccin_directory_text "#{=/#{?#{e|<:#{client_width},140},12,30}/…:#{agent_where_path}}"
```

Proposed renders:

```
80 / 3:   henosis │▎1 Colour sch… ●│▎2 Openvpn s… ▲│▎3 build ○│  ▲1  master  21:11
120 / 8:  henosis │▎1 Colour sch… ●│▎2 Openvpn s… ▲│▎3 build ○│▎4 tests ✓│▎5 docs ○│…  ▲1 ◆2  ~/…/tmux-cockpit  master  21:11
160 / 3:  henosis │▎1 Colour scheme for code review p… ●│▎2 Openvpn setup with voi… ▲│▎3 build ○│   ▲1 ◆2  ~/.tmux/plugins/tmux-cockpit  master  21:11
```

Fleet placement: leftmost in status-right, right after the window list; it renders nothing when quiet (README:117), so it costs zero columns in the common case. Add to full.tmux.conf before :253: `set -g status-right "#{agent_fleet}"` then `set -ag status-right "#{?#{e|<:#{client_width},100},,…directory…}"`.

Border text: tmux.conf:24's `#{=/48/…:#{@panestate_msg}}` on a 60-column pane runs into the `[z][x]` buttons; use `#{=/#{?#{e|<:#{pane_width},80},24,48}/…:#{@panestate_msg}}` and port the arm into full.tmux.conf:220 (closes C4).

## 4. Nerd Font / ASCII variants

| role | unicode (today) | nerd (Font Awesome block, unverified here) | plain |
|---|---|---|---|
| needs-input | ▲ | U+F071 | ! |
| error | ✕ | U+F057 | x |
| running | ● | U+F111 | > |
| done | ✓ | U+F00C | * |
| idle | ○ | U+F10C | - |
| review pending | ◆ | U+F06E | ? |
| ctx gauge | ◔ | U+F0E4 | % |

The glyphs live in the user's conf (full.tmux.conf:199-200), so a `@cockpit_icons` switch only works by cockpit.tmux publishing seven option values the example formats reference, plus a read in agent-state (the fleet string is shell-built at :369-371). The design if wanted: one `case` beside the PALETTE heredoc writing `@cockpit_g_needs … @cockpit_g_ctx`; formats read `#{@cockpit_g_needs}`; agent-state picks up its three in the batched display-message at :339-340 (one extra field, free).

Recommendation: README.md:255 already promises "A Nerd Font is not required. The markers are plain Unicode geometry." The nerd set is a downgrade: it trades shape-encoding (README:29-30, survives greyscale and red/green colourblindness) for prettier icons, and the three circle-ish nf glyphs are less distinguishable at one cell than ▲ ✕ ●. Ship the seven options with Unicode defaults (every glyph overridable, one line each) and skip the mode switch. Also the only proposal with real redraw cost (five option lookups per tab per redraw).

## 5. Feedback moments

Six facts from docs/superpowers/research/2026-08-26-hand-made-worktree/ux.md:9-14: a display-message from inside `display-popup -E` survives the popup closing; display-time is 2000 ms here (full.tmux.conf:15), 750 ms stock; on this theme the message is painted into the middle of status-right with no separator (captured: `t 1 sh    tmux-review: no agent pane is blocked or has uncommitted changesockpit ⑂ master ⏰ 21:11`); `printf >&2` inside the popup renders to nobody (the rule is written down at tmux-review:1158-1159, beside the display-message it forces); a message longer than the client is silently cut; `-d 0` stays until a keypress.

What each keypress says today:

| key | message | duration |
|---|---|---|
| prefix u | silence on success; empty: "no agent needs you" tmux-jump:57 | display-time |
| prefix f | "side fix running in %N: <job> — %N is held at its next stop until it finishes[; commands matching /X/ are held off]" tmux-fixer:123 (~110 chars); busy: :96 | display-time |
| review delivered, idle | "review pasted into <where> (%N) — press Enter there to send; prefix J walks there" tmux-review:1663 | -d 0 |
| review delivered, busy | "review queued for <where> (%N) — it lands when the agent next stops" :1664 | -d 0 |
| prefix r, empty | "tmux-review: no pane runs an agent named in @cockpit_agents_glob" :2180 | display-time |
| prefix R | "config reloaded" cockpit.tmux:354 | display-time |
| copy in review | "file + diff copied" / "line(s) copied" / "the whole diff copied" :1199-1204 | display-time |
| hook | "held: <reason> (side fix in %N) — %N resumes when it finishes" review-gate:147 | display-time |

The bug: tmux-review:1663 says "prefix J walks there"; the key is `u` (cockpit.tmux:264, README:273-274, KEYS.md). Read the option, not a literal: `prefix #{?#{@cockpit_jump_key},#{@cockpit_jump_key},u} walks there`. That format still has one wrong arm: cockpit.tmux:264 treats `off` as unbind (`case "$JK" in off) jk= ;;`), so with `@cockpit_jump_key off` the clause renders "prefix off walks there" for a key that is bound to nothing — the whole clause has to drop, `#{?#{==:#{@cockpit_jump_key},off},,; prefix … walks there}`. The fail-first test in README:26 sets the key to `x` and checks for `prefix x`, so it passes over exactly this arm; it needs an `off` case beside it.

Convention: `cockpit: <what happened> · <what you do next>`, one prefix (because of the weld into status-right), hard ceiling 72 characters so it survives an 80-column client. Over budget today: tmux-fixer:123 (split the guard clause onto a second message or drop it), tmux-review:1364 (~120, the update-ref line; deserves `-d 0`). Three tiers: act = `-d 0` (both delivery notices, tmux-fixer:123, refusals carrying a pasteable fix); inform = display-time ("config reloaded", "copied", "no agent needs you"); never = `printf >&2` in a popup — and there is nothing left to convert, every popup path already uses display-message. The six stderr sites in inventory.md:72's census (tmux-review:2008 :2012 :2015 :2065 :2312 :2393) are the CLI and `--request` entry paths, where stderr is the right stream and stays. Do not raise display-time. prefix u stays silent on success: the viewport moving is the acknowledgement.

## 6. Flicker and repaint

The repo's own budget: README.md:79 "A status redraw runs no shell at all"; README:128-133 and agent-state:297-303 (the fleet precompute replaced up to four whole-server walks per expansion; an agent emitting an OSC-2 title forces an expansion per keystroke); tests/freeze-rig/RESULTS.txt:26 (36 / 144 / 360 pane visits per redraw before); agent-state:354-357 (compare-then-set: every set-option, even -p with the same value, forces a server-wide redraw); agent-state:375-380 (no refresh-client: 19.6 ms per transition and it re-ran the two `#()` jobs).

Still repaints too often:

- R1 examples/tmux.conf:42-43 walk live: five `#{P:}` arms per tab per redraw. full.tmux.conf:199-200 were converted to `@cockpit_win_state` (comment :195-198); the portable block the README tells people to copy never got the fix. Largest remaining redraw cost; a straight port of two lines.
- R2 full.tmux.conf:234-237: four hooks (after-select-pane, after-select-window, client-session-changed, pane-focus-in) run `refresh-client -S`, which re-runs the `#()` jobs cockpit.tmux:60-61, :80-81 rewrite `#{agent_where_path}` / `_branch` into. pane-focus-in fires on terminal focus, so alt-tabbing back pays two shell spawns plus ~19.6 ms. With status-interval 2 the path/branch are at most 2 s stale anyway. Drop pane-focus-in, keep the other three.

Do the proposals add redraw? popup chrome: zero (set once). Titles, sizes, replayed menu rows: zero, minus two popup definitions. `◆`→sky, `◔`→mauve, bold `▲`: zero (one token, written on transition, guarded by the compare at :372). 16-colour block: zero. Truncation ladder: small, no shell (two nested `#{e|}` per tab plus two option lookups; `#{e|}` is already on the path at :220; integer arithmetic is an order below the 32-36 µs regex at README:1139). status-right ladder: negative (drops one `#()` job under 100 columns). `#{agent_fleet}` in status-right: zero (precomputed). Border-text ladder: zero. Seven glyph options: the only real cost (five lookups per tab per redraw). Nothing proposed adds a shell spawn, a pane walk, or an unguarded set-option.

## Ranked

1. tmux-review:1663 names `prefix J`; the key is `u`.
2. R1: port the `@cockpit_win_state` arms into examples/tmux.conf:42-43.
3. C4: full.tmux.conf:220 is missing `@panestate_msg`.
4. `#{agent_fleet}` rendered by neither example.
5. C1: `◆` and `●` share `@thm_yellow`.
6. Popup chrome, three lines.
7. Truncation ladder, two options and two substitutions.
8. R2: drop pane-focus-in from full.tmux.conf:237.

Skipped: `@cockpit_icons`; ship seven overridable glyph options instead.
