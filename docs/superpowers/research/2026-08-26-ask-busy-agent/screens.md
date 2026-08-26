# The five #54 screens, designed

Design lane, 2026-08-26, read-only. Refines the drafts in [ux.md](ux.md) §2 with a shared grid, colour roles, glyph tiers, truncation rules and 80/120-column mockups. The fzf notes target 0.74.3 (latest) with a plain tier; see the #58 audit ([pickers.md](../2026-08-26-ui-audit/pickers.md)) for the shared theme.

## 0. Shared style guide

Grid: label field 8 columns, a 2-column glyph gutter, value at column 11. Today every info label is padded to 10 (tmux-review:2076-2099); moving 2 of those into a glyph gutter costs nothing and lets state/review/doing carry the same glyph the window tab uses. One label breaks the grid: `review dir` (11 wide, 11-space continuation, :2119); rename it `reviews` and set the continuation to 10.

Right rail: one 5-column field, right-flush, holding an elapsed duration only (32s, 7m12s, 1h04m, 3d). Never "ago", never a wall clock. Two spaces minimum before it; a value that would collide truncates to make room. Replaces the draft's mixed "32s" / "41m ago".

Colour roles (Mocha → 256 → 16; `@thm_red/peach/yellow/green/overlay_0` are already defaulted at cockpit.tmux:190-194, the three new ones join that heredoc):

| role | used for | Mocha | 256 | 16 |
|---|---|---|---|---|
| label | the label field | Overlay1 #7f849c | 245 | 2m (dim) |
| value | facts | default fg | | |
| quote | said / asked / question text | Subtext0 #a6adc8 | 250 | default |
| meta | rail, counts, footer hints | Overlay0 #6c7086 | 243 | 90m |
| accent | key glyphs, `/btw`, `ask>` | Lavender #b4befe | 147 | 34m |
| needs | ▲, needs-input rows | Red #f38ba8 | 210 | 31m |
| error | ✕ | Peach #fab387 | 216 | 33m |
| running | ● | Yellow #f9e2af | 223 | 33m |
| done | ✓ | Green #a6e3a1 | 151 | 32m |

Emit via one `sgr <role>` helper in tmux-review that reads the `@thm_*` option and degrades on `#{client_termfeatures}` / `tput colors`. The only ANSI in the repo today is bare `\033[32m` / `\033[31m` (tmux-review:874), the 16-colour rung.

Key hints: `<glyph-or-key> <verb>`, three spaces inside a group, ` · ` between groups: `↵ close   a ask again`. Keys in accent, verbs in meta. No brackets, no "press".

Truncation: always `…` (U+2026), the repo's marker (`#{=/48/…:}` tmux.conf:24, `=/16/…` and `=/34/…` :42-43). Text truncates right; paths truncate left (`…/tmux-cockpit/review/14-3187720301`). A field over its cap overflows instead of truncating only when it is the last column (the existing fleet policy, tmux-review:2183-2186). Count characters, not bytes (`pad_field` documents this, :2124-2131).

Glyphs, three tiers; default stays the shipping BMP set; nerd is decoration behind `@cockpit_glyphs nerd`, ascii behind `@cockpit_glyphs ascii`:

| meaning | unicode (default) | nerd | ascii |
|---|---|---|---|
| needs-input | ▲ | nf-fa-exclam_triangle | ! |
| error | ✕ | nf-fa-times | x |
| running | ● | nf-fa-circle | * |
| done | ✓ | nf-fa-check | + |
| idle | ○ | nf-fa-circle_o | . |
| review pending / sent | ◆ / ↵ | nf-oct-diamond / arrow | # / < |
| branch | ⑂ | nf-dev-git_branch | @ |
| enter | ↵ | nf-md-keyboard_return | <CR> |
| ellipsis | … | … | ... |
| spinner | ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ | same | \|/-\ |

## 1. `prefix i`, the info screen

Rows added: `since` on state, then doing / said / asked, then `ask it`. Two rows changed: `cwd` printed only when it differs from `root` (today unconditional, tmux-review:2077, a duplicate line in the worktree case), and `review dir` → `reviews`. Ordering: `ask it` goes directly under `asked`, not beside `title`; the human reads doing/said/asked and the next thought is "can I ask it".

Terminal 120 (popup 84, inner 82):

```
┌ info · %14 ──────────────────────────────────────────────────────────────────────┐
│pane      %14  henosis:3 Claude Code  (claude)                                    │
│root      ~/.cache/cockpit-wt/lowercase-keys                                      │
│state   ● running                                                            7m12s│
│doing     Bash: shellcheck on cockpit.tmux and tests/run.sh                    32s│
│said      While the suite and rig run: shellcheck on the two scripts, and         │
│          the independent review pass on the diff.                                │
│asked     do it review it test rig and implaments                              41m│
│ask it    /btw <question> in the pane — answers while it works                    │
│review  ◆ pending                                                                 │
│hold      —                                                                       │
│session   098c323f-5ffe-4309-a3dc-996d24d6f00a                                    │
│title     Implement with test review and rig                                      │
│reviews   ~/.local/state/tmux-cockpit/review/14-3187720301                        │
│          0 pinned · 2 delivered · ^A removes it                                  │
│                                                                                  │
│↵ close                                                                           │
└──────────────────────────────────────────────────────────────────────────────────┘
```

Terminal 80 (inner 54):

```
┌ info · %14 ──────────────────────────────────────────┐
│pane      %14  henosis:3 Claude Code  (claude)        │
│root      ~/.cache/cockpit-wt/lowercase-keys          │
│state   ● running                                7m12s│
│doing     Bash: shellcheck on cockpit.tmux and…    32s│
│said      While the suite and rig run: shellcheck     │
│          on the two scripts, and the review pass     │
│asked     do it review it test rig and impla…      41m│
│ask it    /btw <question> in the pane                 │
│review  ◆ pending                                     │
│hold      —                                           │
│session   098c323f-5ffe-4309-a3dc-996d24d6f00a        │
│title     Implement with test review and rig          │
│reviews   …/tmux-cockpit/review/14-3187720301         │
│          0 pinned · 2 delivered · ^A removes it      │
│                                                      │
│↵ close                                               │
└──────────────────────────────────────────────────────┘
```

Labels dim, values default, glyph gutter at columns 9-10 coloured by state. Value budget W−10 on rail-less rows, W−15 on rail rows; W is `tput cols` inside the popup (fleet_queue already reads `tput lines` this way, :2211). `said` is the only wrapping row: word-wrap to 2 lines at indent 10, `…` if it still overflows. Everything else one line, right-truncated; paths left-truncated; `session` never truncated (36 chars fits both widths). `ask it`'s dim tail "— answers while it works" prints only if it fits; the head "/btw <question> in the pane" always prints. Footer `↵ close` (today `↵ to close`, :2121). Height: 16-18 lines now; `-h "$PH"` (cockpit.tmux:298) at 70 % of a 24-row terminal is 16 rows, 14 usable, and clips. Change that one bind to `-h 20`; the info screen has a known line count.

## 2. `prefix r`, the fleet row

The bug this fixes first: caps `w_state 12`, `w_title 40`, `w_loc 32` (tmux-review:2196-2198) plus separators plus stat are ~102 columns of row. The popup is 70 % wide (cockpit.tmux:284) and fzf spends more on chrome, so the row overflows on anything under ~150 columns and fzf silently cuts the right edge: `±`, the number the list exists to compare (:2162-2164), is the first thing lost. The caps must come from `tput cols`.

Three structural changes: the state word becomes a 2-cell glyph pair (status glyph + review mark ◆/↵, :1818), recovering 10 columns, the word moves to the preview header; `fleet_rows` emits `where` and `title` as separate fields (today concatenated in the format, :1717) so the narrow profile can drop one; the pane id stays hidden (`--with-nth=2,3,4,5`, :2248), drop the `%9`/`%14` from the ux.md draft.

fzf options and first versions: `--highlight-line` 0.52, `--tmux` 0.53, `--wrap`/`--wrap-sign` 0.54, `--gap` 0.56, `--gap-line` 0.58, `--style=full` and the `*-border`/`*-label` set 0.58, `--accept-nth` 0.60, `--ghost` 0.61, `--footer` 0.63, `--info=inline-right` 0.42 (in use), `pos()`/`start:` 0.36 (in use). Two notes: 0.58 made a single-character `--delimiter` a plain string (the tab at :2248 is unaffected); 0.66 with `--tmux` makes an explicit `--border` the only one drawn, so pair `--tmux` with `--style=full` if used.

`--tmux` (0.53+) would replace the display-popup wrapper for this key: the bind becomes `run-shell "'$Q_REVIEW' -p '#{pane_id}'"`, which expands `#{pane_id}` on the source pane directly (the pattern cockpit.tmux:279-281 documents for `@cockpit_review_window`), and the `$SRC_PANE` dance disappears; the popup title is lost, `--input-label` / `--list-label` carry it. Below 0.53 `display-popup` as today. (The pickers lane recommends against `--tmux`; owner decision 2 in the #58 README.)

Column budget: R = floor(client_width × 0.7) − 6 (fzf popup border 2 + list border 2 + pointer gutter 2): 50 at 80 columns, 77 at 120, 106 at 160.

| profile | R | mark | where | title | doing | age | loc | ± |
|---|---|---|---|---|---|---|---|---|
| A | < 68 | 2 | - | 20 | 20 | 5 | - | - |
| B | 68-99 | 2 | 10 | 20 | 24 | 5 | - | 10 (last, may overflow) |
| C | ≥ 100 | 2 | 11 | 22 | 28 | 5 | 20 | 10 (last, may overflow) |

Collapse order (first dropped first): loc, then ±, then where. loc goes first because the preview already resolves and shows that root (:2233-2236); when dropped its text moves to the preview's first line: `cockpit⑂lowercase-keys · +214 -63 · running 7m12s`.

R=106 (terminal 160):

```
┌─ fleet ──────────────────────────────────────────────────────────────────────────────────────────────────┐
│fleet>                                                                                                    │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────┘
┌─ which agent needs you ──────────────────────────────────────────────────────────────────────────────────┐
│▲ needs  ● running  ✓ done  ○ idle  ◆ review pending  ↵ pasted                                            │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────┘
┌─ 5 agents · 1 blocked · 1 review pending ────────────────────────────────────────────────────────────────┐
│▲  henosis:1   Openvpn setup          permission: Bash(sudo openvpn --config /etc/openvpn/work.conf       │
│                                    ↳ --daemon --log /var/log/ovpn.log)                                   │
│●◆ henosis:3   Implement with test r… Bash: shellcheck on cockpit…   32s  cockpit⑂lowercase-k…  +214 -63  │
│●  work:2      Fleet gate review      Agent: whole-branch review …  4m12  cockpit⑂fleet-timing  +12 -4  ?…│
│✓  work:4      Left click issue       idle · handover pending        12m  cockpit⑂master        +3 -0     │
│○  work:5      (no title)             —                                   cockpit⑂master        clean     │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────┘
 ↵ review   ^X kill window   ^/ preview   esc cancel
```

R=77 (terminal 120):

```
┌─ fleet ─────────────────────────────────────────────────────────────────────┐
│fleet>                                                                       │
└─────────────────────────────────────────────────────────────────────────────┘
┌─ which agent needs you ─────────────────────────────────────────────────────┐
│▲ needs  ● running  ✓ done  ○ idle  ◆ review pending  ↵ pasted               │
└─────────────────────────────────────────────────────────────────────────────┘
┌─ 5 agents · 1 blocked · 1 review pending ───────────────────────────────────┐
│▲  henosis:1  Openvpn setup        permission: Bash(sudo openvpn --config    │
│                                 ↳ /etc/openvpn/work.conf --daemon)          │
│●◆ henosis:3  Implement with test… Bash: shellcheck on coc…   32s  +214 -63  │
│●  work:2     Fleet gate review    Agent: whole-branch rev…  4m12  +12 -4  ?…│
│✓  work:4     Left click issue     idle · handover pending    12m  +3 -0     │
│○  work:5     (no title)           —                               clean     │
└─────────────────────────────────────────────────────────────────────────────┘
 ↵ review   ^X kill window   ^/ preview   esc cancel
```

R=50 (terminal 80), where, loc and ± gone, question pre-capped to two lines:

```
┌─ fleet ──────────────────────────────────────────┐
│fleet>                                            │
└──────────────────────────────────────────────────┘
┌─ which agent needs you ──────────────────────────┐
│▲ needs ● running ✓ done ◆ review                 │
└──────────────────────────────────────────────────┘
┌─ 5 agents · 1 blocked ───────────────────────────┐
│▲  Openvpn setup        permission: Bash(sudo     │
│                      ↳ openvpn --config /etc/op… │
│●◆ Implement with test… Bash: shellcheck on…   32s│
│●  Fleet gate review    Agent: whole-branch…  4m12│
│✓  Left click issue     idle · handover pen…   12m│
│○  (no title)           —                         │
└──────────────────────────────────────────────────┘
 ↵ review  ^X kill  ^/ preview  esc
```

Wrapping replaces truncation for the question, with a cap: pre-truncate the question to 2 × flex characters at profiles A and B, 3 × at C, then let `--wrap` lay it out; `--wrap-sign=' ↳ '` in meta. `--gap` stays off (one row per agent for separation the wrap sign already gives; the popup is 70 % tall). Chrome: `--input-label=' fleet '`, `--header-label=' which agent needs you '` with the glyph legend as the one-line header, `--list-label` carries the live counts (already computed for `@cockpit_fleet`, cockpit.tmux:63-78, free), `--footer` takes the keys. That drops the header from two lines to one and retires the `psz=$(( avail - lrows - 6 ))` reservation (:2205-2210). `--highlight-line` with `--color current-bg:#{@thm_surface_0}`; `--ghost='filter by title, branch, or state'`; `--accept-nth=1` replaces `pane=${sel%%$'\t'*}` (the `--expect=ctrl-x` key line still parses).

Needs-input rows differ three ways: `▲` and the row from title rightwards in needs red; the question replaces doing, age, loc and ± and takes every remaining column (a blocked pane's ± reads `—` anyway, :1691-1693); rank 0 already sorts them first (:1808). Colour ships as ANSI in the row text plus `--ansi`, because `--color=16` (:2249) governs fzf's chrome, not row content.

Version gate: two tiers, one `case` on the minor version at the top of `fleet_queue`, boundary 0.63 (`--footer`):

```sh
fzf_ver=$(fzf --version 2>/dev/null); fzf_ver=${fzf_ver%% *}
fzf_ver=${fzf_ver#0.}; fzf_ver=${fzf_ver%%.*}
case "$fzf_ver" in ''|*[!0-9]*) fzf_ver=0 ;; esac
if [ "$fzf_ver" -ge 63 ]; then
    fz=(--tmux center,"$PW","$PH" --style=full --highlight-line
        --wrap --wrap-sign=' ↳ '
        --input-label=' fleet ' --header-label=' which agent needs you '
        --list-label=" $counts " --footer="$keys"
        --ghost='filter by title, branch, or state' --accept-nth=1
        --color='list-border:8,input-border:8,header-border:8,footer-border:8')
else
    fz=()                      # today's flags, unchanged
fi
```

The plain fallback is literally today's screen (:2248-2257, display-popup, two-line header, psz reservation, hard truncation, `${sel%%$'\t'*}`). The width profiles, collapse order, glyph tiers and colour roles are shell-side and apply on both tiers. Per-row `doing` is one transcript read per visible row (~0.1 s), owner decision 2 in README.md; if declined, profiles B/C fall back to where · title · loc · ± with the widths above.

## 3. The needs-you message and the opt-in popup

Status message, in agent-state's needs-input arm (:110-145). One `display-message -d 4000` with a tmux format, no fork; `#{pane_tty}` for the bell folds into the batched `display-message -p` the script already runs at :339:

```
 ▲ henosis:1 openvpn needs you · Bash(sudo openvpn --config /et…) · prefix u
```

```
#[fg=#{@thm_red}]▲#[default] #{session_name}:#{window_index} #{=/10/…:#{window_name}} needs you · #{=/32/…:#{@panestate_msg}} · #[fg=#{@thm_overlay_0}]prefix u
```

32, not the border's 48 (tmux.conf:24): the status line also carries the `prefix u` pointer, and 48 pushes past 80 columns. `@panestate_msg` is already control-stripped and capped at 120 (agent-state:126), safe to interpolate. Skipped when the asking pane is the active one (one `#{?}` in the same call). Bell: `printf '\a' > "$tty"`. No popup by default.

Opt-in popup (`@cockpit_needs_popup on`), raised through `run-shell -b` so the hook returns. `-w 56 -h 5 -x R -y S`: bottom-right, above the status line, where the fleet counter lives and the corner least likely to cover the composer being typed into:

```
┌ needs you ─────────────────────────────────────────┐
│ ▲ henosis:1 openvpn · Bash(sudo openvpn --config …)│
│                                                    │
│ ↵ jump   esc dismiss                closes in  7s  │
└────────────────────────────────────────────────────┘
```

Question truncated to 48 to match the border. Countdown without flicker: never redraw the popup; print the two content lines once, then rewrite only the footer in place with `\r`, a fixed-width `%2d`, no newline, waiting on the keyboard rather than sleeping:

```sh
i=10
while [ "$i" -gt 0 ]; do
    printf '\r ↵ jump   esc dismiss                closes in %2ds ' "$i"
    IFS= read -r -s -n1 -t 1 k && break     # a key ends the wait at once
    i=$(( i - 1 ))
done
```

`read -n1` is bash and the repo already relies on it (tmux-review:2148). `k` = ↵ runs tmux-jump, anything else dismisses.

## 4. `prefix f`, the side-fix popup

Is 6 enough? Yes, but take 7: the pane's identity moves into the popup title (display-popup expands `-T`), so the body needs main-is-on + blank + two prompts = 4 rows; one extra row absorbs a wrapped main-is-on line without scrolling the first prompt off. cockpit.tmux:306: `-h 6` → `-h 7`, title `' side fix · #{session_name}:#{window_index} #{pane_id} '`.

```
┌ side fix · henosis:3 %14 ────────────────────────────────────────────────────────┐
│main is on  ● Bash: run the full suite and the rig                          4m12s │
│                                                                                  │
│side fix (↵ cancels): fix the flaky fleet timing test                             │
│hold off (ERE, ↵ none): make (check|test)                                         │
│                                                                                  │
└──────────────────────────────────────────────────────────────────────────────────┘

┌ side fix · henosis:3 %14 ────────────────────────────┐
│main is on  ● Bash: run the full suite and…   4m12s   │
│                                                      │
│side fix (↵ cancels): fix the flaky fleet ti          │
│hold off (ERE, ↵ none): make (check|test)             │
│                                                      │
└──────────────────────────────────────────────────────┘
```

Both prompts get shorter at every width: today 38 and 45 characters (tmux-fixer:100-101); at inner 54 the second leaves nine columns to type an ERE into. `side fix (↵ cancels): ` is 22 and `hold off (ERE, ↵ none): ` is 24, leaving 32 and 30. Noun in value, parenthetical in meta, `: ` in accent; ANSI in a `read -p` prompt is safe (no readline without `-e`). `main is on` uses the same 8/2/11 grid and rail; with no transcript the row is not printed at all, and the popup looks exactly like today's.

What the spawned pane prints first, one printf in `run_fixer` before claude starts:

```
── side fix · from henosis:3 %14 ─────────────────────────────
job   fix the flaky fleet timing test
main  ● Bash: run the full suite and the rig            4m12s
held  make (check|test)   ·   deadline 5m0s
```

`held` omitted when the guard is empty.

## 5. `prefix ?`, the observer ask popup

`-w "$PW" -h 14`, title carries the pane and its ai-title. Three states, one screen.

```
┌ ask · %14 · Implement with test review ──────────────────────────────────────────┐
│ask> is it stuck or waiting on something?                                         │
│                                                                                  │
│  ⠹  asking the observer …  4s                                                    │
│                                                                                  │
└──────────────────────────────────────────────────────────────────────────────────┘

┌ ask · %14 · Implement with test review ──────────────────────────────────────────┐
│ask> is it stuck or waiting on something?                                         │
│                                                                                  │
│  Waiting on a code review report it dispatched earlier. The suite passed         │
│  (1014/0, two runs) and the rig showed no delta vs master. Nothing is            │
│  blocked — the last tool call returned 32s ago.                                  │
│                                                                                  │
│7s · $0.05 · haiku                              ↵ close   a ask again   esc close │
└──────────────────────────────────────────────────────────────────────────────────┘
```

Prompt `ask> ` in accent (`read -r -p 'ask> '`, the shape of prefix f; matches fzf's `fleet> `). Spinner, portable, no gum:

```sh
spin() {                      # <pid>
    [ -t 1 ] || return 0      # a test harness gets deterministic output
    local f='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏' i=0 t0=$SECONDS
    while kill -0 "$1" 2>/dev/null; do
        printf '\r  %s  asking the observer …  %ds' "${f:i++%10:1}" "$(( SECONDS - t0 ))"
        sleep 0.2 2>/dev/null || sleep 1
    done
    printf '\r%*s\r' "$(tput cols)" ''
}
```

`\r` and a fixed-width tail so nothing above repaints; cleared by overwriting with spaces. The `[ -t 1 ]` guard keeps the fail-first tests from matching a spinner frame (pin a count, never a clock). Answer block: indent 2, word-wrapped to `tput cols` − 4, value colour, blank above and below; scrolls if long, never paginates. Footer: `7s · $0.05 · haiku` in meta left (from `--output-format json`'s `total_cost_usd`, `duration_ms`), keys right-flush; `a` clears the answer and re-prompts on the same screen; ↵ and esc close. Idle pane: skip the observer call and print "this pane is idle — type your question into it directly" with `↵ jump   esc close`; spending $0.05 to ask an idle agent is the one case the popup refuses.

Screens 3, 4 and 5 use no fzf.

## Graceful degradation

No truecolor: `sgr` picks by `#{client_termfeatures}` (RGB) → `38;2;R;G;B`, else `tput colors ≥ 256` → `38;5;N`, else the 16-colour column, else empty strings and plain text. Peach and lavender have no 16-colour equivalent (peach folds into yellow, lavender into blue), which is why every state also carries a glyph. The fleet is the sharp case: fzf runs `--color=16`, so row colour is ANSI in the row text plus `--ansi`, degrading with everything else.

No Nerd Font: nothing in the default path needs one. The real hazard is width: ▲ ● ✓ ◆ are East-Asian Ambiguous, and a terminal with an emoji-preferring fallback renders them 2 cells wide; that is why the glyph gutter is 2 columns, so a double-wide glyph eats its own padding and the grid holds. `@cockpit_glyphs ascii` is the one setting the fail-first tests should exercise (byte-stable).

80 columns: every screen has an 80-column mockup and none overflow. The two tight ones are prefix f (30 columns of typing room) and the fleet row (20 columns of doing); the honest answer for KEYS.md is the two settings that exist: `set -g @cockpit_popup_width 90%` (cockpit.tmux:207-210) or `set -g @cockpit_review_window on` (:281). Below 40 usable columns the fleet should say "terminal too narrow — prefix i on the pane instead" rather than render a row that lies about which agent it names.

## Values changed from current code

| site | from | to |
|---|---|---|
| tmux-review:2076-2099 | label field 10 | 8 + 2-column glyph gutter |
| tmux-review:2077 | cwd unconditional | only when ≠ root |
| tmux-review:2119 | `review dir`, 11-space continuation | `reviews`, 10 |
| tmux-review:2121 | `↵ to close` | `↵ close` |
| tmux-review:1717 | where+title concatenated | separate fields |
| tmux-review:1820 | state word | glyph pair |
| tmux-review:2196-2198 | fixed caps 12/40/32 | `tput cols` profiles |
| tmux-review:2249 | | add `--ansi` |
| tmux-review:2205-2210 | psz reservation | retired (footer) |
| tmux-review:2255-2256 | two fixed 95-char header lines | one, per profile |
| cockpit.tmux:284-290 | display-popup | run-shell + `fzf --tmux` (0.53+; owner decision) |
| cockpit.tmux:298 | `-h "$PH"` | `-h 20` |
| cockpit.tmux:306 | `-h 6` | `-h 7`, title gains `#{session_name}:#{window_index}` |
| tmux-fixer:100-101 | prompts 38/45 chars | 22/24 |
| cockpit.tmux:190-194 | palette | + `@thm_overlay_1`, `@thm_subtext_0`, `@thm_lavender` |
