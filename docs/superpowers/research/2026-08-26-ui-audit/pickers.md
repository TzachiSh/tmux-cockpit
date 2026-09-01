# The fzf pickers: theme, version gate, key hints, rows, previews, glyphs

Read-only lane, 2026-08-26. Installed fzf 0.44.1 (Debian); upstream 0.74.3. Targets 0.63+ per the owner ("use the latest version") with a plain tier that is today's screens. The eight current invocations are quoted in inventory.md §C.

## 1. Critique

Consistency. Six of eight calls share `--reverse --info=inline-right --cycle --scroll-off=2 --color=16,border:8,scrollbar:8` verbatim, copy-pasted, not sourced. Divergences that look accidental:

| picker | --ansi | preview-label | ^/ toggle | preview scroll | multi | pointer/marker |
|---|---|---|---|---|---|---|
| tm attach :184 | no | ' windows ' | absent | absent | no | default/default |
| tm kill :135 | no | none | | | yes | default/'✗' |
| branch :233 | no | absent | yes | ^U/^D | no | default/default |
| review :1248 | yes | absent | yes | alt-u/alt-d | yes | default/default |
| fleet :2248 | no | absent | yes | ^U/^D | no | default/default |
| path :786 | no | ' content ' | yes | ^U/^D | yes | default/default |

- `--ansi` is missing everywhere except the review loop. That is the root cause of "is metadata dimmed": no, and it cannot be, because without `--ansi` fzf prints SGR bytes literally. Any row-level hierarchy starts by adding it to the other five.
- On 0.44.1 `--pointer` and `--marker` are both fzf's default `>` in every picker except tm kill (tm:136 `--marker='✗'`). In the two `--multi` pickers a ⇥-marked row and the cursor row render with the identical glyph in the identical column. (0.53.0 changed the defaults to Unicode bars; the theme should pin both anyway.)
- The selected row is only `bg+` from the `16` base scheme; no `hl+`, `fg+`, `gutter` anywhere. tmux-open-path:406 measured bat's `--highlight-line` at 1.4:1 and rejected it; the pickers' own selection bar fails the same test.
- `^/` toggle-preview is unbound in tm, the one picker whose preview (cross-session agent state, tm:182) is most valuable.
- Preview scroll: `^U/^D` (branch, fleet, path), `alt-u/alt-d` (review, justified at tmux-review:1236-1239: readline owns ^U/^D in a comment field), nothing (tm).
- Header wording: "esc cancel" vs "esc leave"; "tab mark" (path :794) / "tab mark several" (tm :138) / "⇥ mark" (review :1266); three spaces in five headers, ` · ` in the fleet legend (:2255) and the branch context (:239). Prompts `name> ` in five, `$(tail3 "$dir")/ ` in browse, `esc? `/`^S? ` in the confirms.
- The two confirm pickers (tmux-review:1283, :1381) pass no `--color`: fzf's opaque default dark scheme next to siblings that are `--color=16` transparent. The most visible inconsistency: the confirm dialog looks like a different program.

Hierarchy. The review row, tmux-review:868-875: `if (w > 48) w = 48` then `printf "%s\t%s\t%s\t%s %-" w "s  %s %s\t%s\n"`. Field 4 = mark + space + path:line padded to w + two spaces + sign + space + code: at the 48 cap, 55 columns of gutter before the diff text. The popup is `-w 70%` (cockpit.tmux:210), so on a 100-column terminal fzf's content width is ~68 and the code under review gets ~13 columns.

Fleet tab grid (reproduced). `--with-nth=2,3,4,5` (tmux-review:2249) rejoins fields with the original tab, and fzf expands tabs to `--tabstop` (8) from the start of the item. Columns land at 8/24/40 regardless of `pad_field`; the caps 12/40/32 (:2186-2188) are not the real widths; the overflow policy at :2183-2185 shifts a row's remaining columns by a full 8; the known "◆ needs-input" = 13 > 12 case is exactly this. One flag fixes it: `--tabstop=1`.

Ambiguous width. ◆ ● ▲ ○ … are East-Asian Ambiguous; `pad_field` (tmux-review:2129-2133) counts characters, fzf counts them as 1, a terminal set to ambiguous-width=double draws them as 2, and every column drifts one cell per glyph. ⑂ (U+2442) is Narrow but lives in an obscure block; U+E0A0 is likelier to have a glyph in this audience's fonts.

Truncation at 80 columns (header widths vs a 70 % popup, 56 columns): review L1-L3 64-66 fit; review L4 context 118 does not, and the `●`/`↺` legend (the line that explains column 1) is the part cut; fleet L1 (:2255) 94 does not, the column legend is cut; fleet L2 76 marginal; branch L1 73 marginal; path L2 64 fits. fzf cuts headers on the right, so the least skippable content sits in the cut zone while "esc cancel" sits safe.

Previews. Better than expected: tmux-open-path:404-419 already uses bat/batcat with a full fallback, long-line and deep-line guards (:392-403), and a `▸` marker column instead of the rejected `--highlight-line`; `--preview-diff` (tmux-open-path:80+) streams `git diff --color=always` through an awk gutter with a memoised cache. git log colour in branch works (previews always interpret ANSI). The plain ones: tm's `list-windows -F` preview (tm:182, :188) is uncoloured, so `[needs-input]` reads the same as `[idle]` while the status bar colours exactly that keyword.

Empty states are all `display-message` outside the popup, so on prefix r in a clean tree the popup opens, vanishes, and the message lands on a status line nobody was looking at (:1224, :2178, :1159, :2171). tmux-branch:194 `fail` pauses; tmux-open-path:760 `note` likewise. Two conventions for one event.

## 2. Version floor and what it buys (from the CHANGELOG, latest 0.74.3)

| option | first version | buys |
|---|---|---|
| `--highlight-line` | 0.52.0 | the selected row (hierarchy finding #1) |
| `--color current-*` / `selected-*` | 0.52.0 | use `fg+/bg+/hl+` (both tiers); `selected-*` marks ⇥ rows in the two `--multi` pickers |
| default pointer/marker → Unicode bars | 0.53.0 | context for the `>` finding |
| `--tmux` | 0.53.0 | not used, §4 |
| `--wrap`, `--wrap-sign` | 0.54.0 | the review row's 13-column problem, outright |
| default `--ellipsis` → `··` | 0.55.0 | pin `…` on both tiers |
| `--gap[=N]` | 0.56.0 | separates wrapped review rows |
| `--gap-line[=STR]` | 0.58.0 | the ┈ rule in each gap |
| `--style`, `--*-border`, `--*-label` | 0.58.0 | only `--header-border` used |
| `--no-input` | 0.59.0 | the two confirm pickers |
| `--ghost=TEXT` | 0.61.0 | the review loop's `--disabled` query |
| `--color alt-bg` | 0.62.0 | zebra rows for the two column grids |
| `--footer`, `--footer-border/-label`, footer colours | 0.63.0 | keys leave the header; the footer's default border is already `line`, a free rule |
| `--gutter`, `--color ghost` | 0.66.0 | skipped |
| `--color preview-border`, `gutter`, `scrollbar`; `--info=inline-right`; `--tabstop` | ≤ 0.44 | both tiers (verified on 0.44.1) |

Floor = 0.63.0 (`--footer` is the newest option worth having; one gate covers the list). Two tiers, not three: the obvious middle tier at 0.52 has no population (Debian 13 and Ubuntu 24.04 ship 0.44.1; hand installs are current). Two behaviour notes: 0.58.0 made a single-character `--delimiter` a plain string (the tab at :2248 is unaffected; do not switch to a multi-char delimiter later); 0.66.0 with `--tmux` makes an explicit `--border` the only border drawn.

Honesty: plain-tier flags are verified by running on 0.44.1; new-tier flags by changelog. The live-fleet render in README.md §"measured" ran on 0.74.3 with `--style=full`, `--header-border`, `--highlight-line`, `--ellipsis` and the label options, so those are verified too.

## 3. Deliberately not taken

- `--tmux` (0.53). cockpit.tmux owns the popup (:209-210, :225, :243, :257, :285) and passes `-p $SRC_PANE`, which is load-bearing: inside a popup `$TMUX_PANE` is the popup's own pseudo-pane (cockpit.tmux:222-223, tmux-branch:38-45). fzf's popup carries none of that, and `display-popup -T` gives the format-expanded titles. (screens.md argues the other way for prefix r alone; owner decision 2 in README.)
- `--style=full|minimal` (0.58): `full` draws list/input/header boxes inside the tmux popup border, a double frame that fights `popup-border-style`; `minimal` drops the info line. `--list-border`/`--input-border` same reason. (The measured 0.74 render shows the boxes fit; owner decision 3.)
- `--gutter CHAR`, `--color ghost`, `--footer-label`, `alt-bg` on the path pickers: above the floor or no problem to solve.

## 4. Shared theme: `scripts/lib/fzf.sh`

The probe lives in the lib, not cockpit.tmux: one `fzf --version` fork per popup open (~3 ms, off the redraw path), correct after an fzf upgrade without a plugin reload, works for the standalone `~/.local/bin` symlink runs where a load-time option would not exist.

```sh
# scripts/lib/fzf.sh -- sourced by tm, tmux-branch, tmux-review, tmux-open-path.
# One theme for every picker. Sets no `bg`, `list-bg` or `input-bg`: the popup's
# background belongs to the user's popup-style, the same rule cockpit.tmux:220
# states for the border.
COCKPIT_FZF_MIN=63            # --footer; the highest option below needs it

fzf_minor() {                 # fzf is 0.x; 1.x and up is "new enough"
    local v; v=$(fzf --version 2>/dev/null); v=${v%% *}
    case $v in ''|*[!0-9.]*) printf 0; return ;; esac
    [ "${v%%.*}" -gt 0 ] 2>/dev/null && { printf 999; return; }
    v=${v#*.}; printf '%s' "${v%%.*}"
}

cockpit_fzf_opts() {          # [pane_id] -> COCKPIT_FZF_OPTS array
    local rgb=0 pane=${1:-} new=0
    [ "$(fzf_minor)" -ge "$COCKPIT_FZF_MIN" ] && new=1
    case "$(tmux display-message -p ${pane:+-t "$pane"} '#{client_termfeatures}' 2>/dev/null) " in
        *RGB*|*Tc*) rgb=1 ;;
    esac
    # $COLORTERM is NOT the probe: tmux's update-environment does not carry it,
    # so inside a popup it is whatever the server was started with.
    thm() {                   # <@thm_ name> <mocha hex> <256 idx>
        local v; v=$(tmux show-option -gqv "@thm_$1" 2>/dev/null)
        [ "$rgb" = 1 ] && { printf '%s' "${v:-$2}"; return; }
        printf '%s' "$3"
    }
    local c="16\
,fg:$(thm fg          '#cdd6f4' 189)\
,hl:$(thm peach       '#fab387' 216)\
,fg+:$(thm fg         '#cdd6f4' 189):regular\
,bg+:$(thm surface_0  '#313244' 237)\
,hl+:$(thm peach      '#fab387' 216):bold\
,info:$(thm overlay_1 '#7f849c' 244)\
,prompt:$(thm mauve   '#cba6f7' 183)\
,pointer:$(thm mauve  '#cba6f7' 183)\
,marker:$(thm green   '#a6e3a1' 151)\
,spinner:$(thm teal   '#94e2d5' 116)\
,header:$(thm overlay_2 '#9399b2' 247)\
,border:$(thm surface_1 '#45475a' 238)\
,preview-border:$(thm surface_1 '#45475a' 238)\
,preview-label:$(thm overlay_1 '#7f849c' 244)\
,label:$(thm overlay_1 '#7f849c' 244)\
,gutter:-1\
,scrollbar:$(thm surface_2 '#585b70' 240)\
,query:$(thm fg       '#cdd6f4' 189)\
,disabled:$(thm overlay_0 '#6c7086' 243)"

    COCKPIT_FZF_OPTS=(
      --ansi --reverse --cycle --scroll-off=2 --info=inline-right
      --pointer='▌' --marker='◆' --ellipsis='…' --tabstop=1
    )
    if [ "$new" = 1 ]; then
        c="$c\
,selected-fg:$(thm fg     '#cdd6f4' 189)\
,selected-bg:$(thm surface_2 '#585b70' 240)\
,selected-hl:$(thm green  '#a6e3a1' 151)\
,header-border:$(thm surface_1 '#45475a' 238)\
,footer:$(thm overlay_2   '#9399b2' 247)\
,footer-border:$(thm surface_1 '#45475a' 238)"
        COCKPIT_FZF_OPTS+=( --highlight-line --header-border=bottom )
    fi
    COCKPIT_FZF_OPTS+=( --color="$c" )
}
```

Notes: reads the user's `@thm_*` rather than hard-coding Mocha, because full.tmux.conf:171-189 overrides all fourteen tokens; defaults are Mocha when unset. `bg`/`list-bg`/`input-bg` stay unset so the popup shows through; `gutter:-1` likewise; `16` stays the base so an unset key degrades to the terminal's 16 colours. `fg+/bg+/hl+` rather than the 0.52 `current-*` aliases: same effect, one fewer branch. Verified on 0.44.1 that every plain-tier key parses; `list-border` does not exist there. `@cockpit_fleet_fzf_options` still wins: put `"${COCKPIT_FZF_OPTS[@]}"` first and the user's `extra` last (tmux-review:2257 already does); fzf takes the last occurrence of a repeated flag and merges `--color` per key, so `set -g @cockpit_fleet_fzf_options "--pointer='▸' --color=bg+:#45475a"` overrides exactly those keys. Keep the hatch confined to the fleet queue and tm (tmux-review:2239-2243), never the review loop. Per-picker additions on the new tier only (export `COCKPIT_FZF_NEW`): review loop `--wrap --wrap-sign='  ↳ ' --gap --gap-line --ghost='type a comment, ↵ pins it'`; review + fleet `--color=alt-bg:$(thm surface_0)`; the confirm pickers `--no-input`.

## 5. Version gate and advisory

One line in cockpit.tmux beside the tmux-version message at :212-214, once at load:

```sh
# 0.63 is where --footer landed, the newest option the pickers use. Debian 13
# and Ubuntu 24.04 both ship 0.44.1, so most users see this exactly once.
. "$DIR/scripts/lib/fzf.sh"   # fzf_minor and COCKPIT_FZF_MIN, §4
if command -v fzf >/dev/null 2>&1 && [ "$(fzf_minor)" -lt "$COCKPIT_FZF_MIN" ]; then
    tmux display-message "tmux-cockpit: fzf $(fzf --version | cut -d' ' -f1) works, but 0.63+ gets the styled pickers (wrapped diffs, footer key hints). Upgrade: git clone --depth 1 https://github.com/junegunn/fzf ~/.fzf && ~/.fzf/install --bin  ·  brew install fzf  ·  github.com/junegunn/fzf/releases"
fi
```

The gate calls `fzf_minor` (§4) rather than re-deriving the version: sourcing the lib is what the pickers already do, and it keeps one floor constant. Run in a plain shell against a fake `fzf`: `0.74.3` quiet, `0.63.0` quiet, `0.44.1` advisory, `1.2.0` quiet (the `-gt 0` major arm returns 999), and no fzf at all quiet — the `command -v` guard, because README calls fzf optional and every picker falls back to a numbered select menu.

Do not reuse `version_at_least` (cockpit.tmux:175-180): it takes `${v#*.}` as the minor, which on 0.74.3 yields `74.3`, fails its own `*[!0-9]*` test, and becomes 0. Correct for tmux's pre-stripped `3.7`, wrong for three components. Any hand-rolled `${v#0.}` arithmetic has the same defect — `[ 74.3 -ge 63 ]` is not an integer comparison, it errors and falls through. README: "fzf is optional, every picker falls back to a numbered select menu; fzf 0.63+ enables the styled pickers."

## 6. Header / key-hint convention

`--footer` (0.63) changes the shape: keys go in the footer, context in the header. That fixes truncation-eats-the-legend structurally; today the two compete for the same header rows.

Rules: one `--footer` (keys), one `--header` (context); two lines max for a navigation picker, three footer lines for the review loop. One order always: `↵ primary · alt-↵ secondary · ⇥ mark · ^-verbs · ^/ preview · esc exit`. Separator ` · `. Glyphs: `↵`, `⇥`, `^X` (never `ctrl-x`), `alt-c`, `esc`; never the word "tab". One-word verbs: open, switch, attach, kill, pin, mark, edit, list, refresh, deliver, approve, copy, preview, cancel. "esc cancel" everywhere; "esc leave" only in the review loop, where esc does not discard. Leftmost = least skippable (fzf truncates on the right; past ~48 columns is gone at 80).

| picker | footer (keys) | width | header (context) |
|---|---|---|---|
| tm attach | `↵ attach · ^X kill · ^/ preview · esc cancel` | 44 | `<n> sessions · ● attached` |
| tm kill | `↵ kill · ⇥ mark · esc cancel` | 28 | `✗ marks what dies · the attached session is safe here` |
| branch | `↵ switch · alt-↵ new branch · ^/ preview · esc` | 50 | `● on <current> · ⑂ held by a worktree · <tail3 dir>` |
| review L1 | `↵ pin · ⇥ mark · ^E edit · ^X delete · ^L list` | 45 | `● pinned · ↺ sent · base <label> (^B <next>) · <~/…/root>` |
| review L2 | `^Y copy · alt-c lines · alt-y all · ^R refresh` | 45 | |
| review L3 | `^S deliver · ^A approve · ^/ preview · esc leave` | 48 | |
| fleet | `↵ review · ^X kill window · ^/ preview · esc` | 47 | `◆ pending · ↵ unsent · state · title · repo⑂branch · ±` |
| path | `↵ open · ⇥ mark · ^O as-is · esc cancel` | 39 | `^G diff · ^F content · ^/ preview · alt-c copy path` |
| browse | `↵ enter · ^O open as-is · ⇥ mark · esc cancel` | 45 | `<tail3 dir>/` |
| confirm | `↵ choose · esc keep reviewing` | 29 | `<n> comment(s) pinned, not delivered` |

Every line ≤ 48 columns, so it survives an 80-column terminal (a 56-column popup). `--footer` takes multiple lines natively; its default border is `line`, `--header-border=bottom` gives the matching rule: two rules, zero boxes. Plain tier: everything concatenates back into `--header` as today, context last. Fold "↺ N delivered earlier" (tmux-review:1256) into the context line as `↺ N sent`.

## 7. Row layouts at 80 and 120 (content width = popup − 2; a 114-column terminal gives an 80-column popup, 172 gives 120)

Review loop today at 80 (measured from a real `--rows` run):

```
▌◆ scripts/lib/very_long_module_name_here.sh:2    + b chang
  ● scripts/lib/very_long_module_name_here.sh:2    - b
  ↺ newfile.txt                                :1  + x
                                               ^^^ 55 cols of gutter, 21 for the code
```

New tier at 80 (`--wrap --wrap-sign='  ↳ ' --gap --gap-line --highlight-line`):

```
▌◆ scripts/lib/very_long_module_name_here.sh:2 │ + b changed to something
  ↳ quite long indeed
┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
  ● newfile.txt:1                              │ + x
```

Plain tier at 80 (dynamic cap replaces `--wrap`) and 120:

```
▌◆ …_module_name_here.sh:2 │ + b changed to something quite
  ● …_module_name_here.sh:2 │ - b
  ↺ newfile.txt:1           │ + x
  ├─ 24 ──────────────────┤ │ ├──────── 51 ───────────────┤

▌◆ scripts/lib/very_long_module_name_here.sh:2 │ + b changed to something quite long indeed
  ● scripts/lib/very_long_module_name_here.sh:2 │ - b
  ↺ newfile.txt:1                               │ + x
  ├──────────────── 46 ────────────────────────┤ │ ├─────────────── 71 ──────────────────┤
```

Rule: the path column gives way first, from the left (head-ellipsis), which tmux-review:873 already implements (`l = "…" substr(l, length(l)-w+4)`) and which does run: any `path:line` longer than the cap at :868 (`if (w > 48) w = 48`) gets its head cut today. So the mechanism is there and only the constant is wrong — 48 columns of path is a reasonable gutter at 120 and half the screen at 80. Change the cap to `int(width*0.35)` clamped [18,48], width from `tput cols` at build time (as `avail` at :2206); the ellipsis code is untouched. The diff text is never truncated by our hand; fzf cuts it, correctly, because a diff line's left end is the informative end.

Fleet today at 80 (tabstop grid; real column starts 0/16/56 or 64), proposed at 120 (`--tabstop=1` + alt-bg + one-glyph state) and at 80 (state word drops, glyph stays):

```
▌needs-input   claude: fix the popup pane id      tmux-cockpit⑂fix/49    +12/
 ◆ done        claude: rebase onto master         omc⑂main
                                                                    ^ ± off-screen

▌▲ needs-input  claude: fix the popup pane id            tmux-cockpit⑂fix/49       +12 -3
 ✓ done         claude: rebase onto master               omc⑂main                    +1
 ● running      claude: run the suite                    tmux-cockpit⑂master       +40 -8
 ○ idle         shell                                    tmux-cockpit⑂master
 ├2┤├── 12 ───┤├─────────────── 40 ──────────────────┤ ├────── 26 ─────────────┤├─ 8 ─┤

▌▲ claude: fix the popup pane id     cockpit⑂fix/49    +12 -3
 ✓ claude: rebase onto master        omc⑂main            +1
 ● claude: run the suite             cockpit⑂master     +40 -8
 ○ shell                             cockpit⑂master
 ├2┤├────────── 30 ────────────────┤├────── 18 ──────┤├── 8 ─┤
```

Rule: title tail-truncates first; then repo⑂branch head-truncates (`⑂feat/long-name` matters more than a repo name you already know); then the state word disappears. `±` never truncates. `w_title = width - 2 - w_state - w_loc - w_stat - 3`, floor 16. Both tiers: one glyph plus a hard space for state, bounding the ambiguous-width drift to a constant. (screens.md adds the `doing` column and a three-profile collapse order on top of this.)

Path picker at 80: `--keep-right` (tmux-open-path:787) stays; it protects the `basename:line` end. tm at 80: no layout change; `--ansi` so `(attached)` can dim and `+ new session…` can be green.

## 8. Previews

bat is already wired in the one place it belongs (tmux-open-path:404-419), with guards. Do not add it to a diff preview: `--preview-diff` is `git diff --color=always` through the awk gutter with a cache; `bat --language=diff` would re-colour and break the line-number gutter the review rows anchor to. delta would beat both and is a new dependency, skipped.

The one preview to change is tm's (tm:182, :188): `#[fg=…]` in a `list-windows -F` reaches stdout as literal text, so this is a sed:

```
--preview="tmux list-windows -t \"=\$(echo {1}):\" -F '$PVFMT' 2>/dev/null | sed \
  -e 's/\[needs-input\]/\x1b[31m▲ needs-input\x1b[0m/' \
  -e 's/\[error\]/\x1b[33m✕ error\x1b[0m/' \
  -e 's/\[running\]/\x1b[93m● running\x1b[0m/' \
  -e 's/\[done\]/\x1b[32m✓ done\x1b[0m/' \
  -e 's/\[idle\]/\x1b[90m○ idle\x1b[0m/'"
```

Also: tm gets `--bind 'ctrl-/:toggle-preview'`; `--preview-label` where missing: `' log '` (branch :240), `' diff '` (review :1262, fleet :2252).

## 9. Glyphs

README.md:255 promises "A Nerd Font is not required." Plain stays the default; nerd is opt-in.

| kind | plain (default) | nerd | current site |
|---|---|---|---|
| needs input | ▲ | U+F071 | full.tmux.conf:199, :216, fleet |
| error | ✕ | U+F057 | :199 |
| running | ● | U+F111 | :199 |
| done | ✓ | U+F00C | :199 |
| idle | ○ | U+F10C | :199 |
| review pending | ◆ | U+F407 | tmux-review:1719 |
| pasted, unsent | ↵ | U+F0EA | :1719 |
| branch | ⑂ | U+E0A0 | tmux-branch:178, tmux-review:1721 |
| current branch | ● | U+F111 | tmux-branch:184 |
| pinned comment | ● | U+F075 | tmux-review:861 |
| sent last round | ↺ | U+F021 | :861 |
| fzf pointer | ▌ | ▌ | new |
| fzf marker | ◆ | U+F00C | tm:136 only today |
| kill marker | ✗ | U+F1F8 | tm:136 |
| preview target line | ▸ | ▸ | tmux-open-path:412 |

`⑂` is worth changing regardless: U+2442 sits in an obscure block and boxes in several monospace faces; U+E0A0 is in every Powerline/Nerd-patched font; `⌥` / `↳` / `:` are safe plain fallbacks. Pointer `▌` (U+258C, Narrow) reads as selection even where `bg+` is weak.

Where the choice lives: resolve once at plugin load in cockpit.tmux, written back as ordinary options (`@cockpit_ico_needs`, `_error`, `_running`, `_done`, `_idle`, `_pending`, `_unsent`, `_branch`) so the example formats reference `#{@cockpit_ico_needs}` and the scripts read the same options with `show-option -gqv` plus a Unicode default, the pattern cockpit.tmux:209-210 already uses. No second table in a `scripts/lib/icons.sh`. Avoid ambiguous-width glyphs in any padded column until `pad_field` counts cells; one glyph per state is the lazy answer the 80-column mockups assume.

## Top five

1. `--ansi` on the five pickers that lack it. Unblocks every dimming improvement; zero risk.
2. `--tabstop=1` on the fleet queue (tmux-review:2249). Hands the grid back to `pad_field`. One flag, reproduced bug.
3. `--color` on the two confirm pickers (tmux-review:1283, :1381).
4. `--highlight-line` + `--pointer='▌'` (new tier) / `--pointer` + `bg+` (plain tier). The selected row.
5. `--wrap` on the review loop (new tier) or the dynamic path cap at tmux-review:868 (plain tier). Gives the diff text back its columns.
