# Inventory: every human-facing screen

Read-only lane, 2026-08-26. Quotes, not paraphrase. Paths relative to the repo root.

## A. Popup-hosted screens (`display-popup`)

| # | trigger | file:line | size | title | command |
|---|---|---|---|---|---|
| 1 | Ctrl+click branch segment | cockpit.tmux:225 | `-w "$PW" -h "$PH"` (70%/70%; `@cockpit_popup_width/_height`, read at :209-210) | `' switch branch · #{b:pane_current_path} '` | `$Q_BRANCH -p $SRC_PANE` |
| 2 | Ctrl+click directory segment | cockpit.tmux:231-233 | same | `' files · #{b:pane_current_path} '` + `-d '#{pane_current_path}'` | `$Q_OPEN --browse` |
| 3 | prefix b | cockpit.tmux:243 | same | as 1 | as 1 |
| 4 | prefix g | cockpit.tmux:257 | same | `' sessions '` | `TM_HEIGHT=100% $Q_TM` |
| 5 | prefix r | cockpit.tmux:285 | same | `' review · #{b:pane_current_path} '` | `$Q_REVIEW -p $SRC_PANE`; with `@cockpit_review_window=on` (:281-283) a real window, no popup, no title |
| 6 | prefix i | cockpit.tmux:298 | same | `' info · #{pane_id} '` | `$Q_REVIEW -p $SRC_PANE --info; read -r _` (the `read` lives in the binding) |
| 7 | prefix f | cockpit.tmux:306 | `-w "$PW" -h 6` (the only popup overriding the option) | `' side fix · #{pane_id} '` | `$Q_FIXER -p $SRC_PANE` |
| 8 | path picker re-entry | tmux-open-path:688-690 | pw/ph re-read at :686-687 | `" open path · #{b:pane_current_path} "` | `$0 --pick <pane>` |
| 9 | agent-requested review | tmux-review:2026-2027 | pw/ph re-read at :2016-2017 | `" review · requested · $t "` (`$t` built at :2025) | `--loop` then `echo $? > "$sess/rc"` |
| 10 | example menu "What is each agent doing?" | full.tmux.conf:120 | `-w 78% -h 40%` | none | `list-panes -a -F … \| column -t ; read -n1 -s` |
| 11 | example menu "Switch branch" | full.tmux.conf:122 | `-w 70% -h 70%` hardcoded | `' switch branch · ##{b:pane_current_path} '` | `~/.local/bin/tmux-branch -p #{pane_id}` |
| 12 | example menu "Sessions..." | full.tmux.conf:123 | `-w 60% -h 50%` | none | `run-shell 'tmux popup -E … ~/.local/bin/tm'` (only use of the `popup` alias) |

`SRC_PANE` (cockpit.tmux:158): `'"$(tmux display-message -p -t "\$${TMUX##*,}" "#{pane_id}")"'`, resolved by the popup's own sh because (:156-157) "display-popup expands formats only in -T and -d; a '#{pane_id}' here was literal text and scripts fell back to $PWD (#38)."

## B. Menus (`display-menu`)

- prefix a, cockpit.tmux:426, `' agents '`, `-x C -y C`. Rows via `row()` at :417: `"switch-client -T prefix ; send-keys -K $3"` (replays the bound key). Gated `version_at_least 3 4`, else display-message at :428.
- prefix M, cockpit.tmux:400, `'#[align=centre] move ##{window_index}:##{window_name} to '`, `-x W -y W`. Rows from `#{S/n:#{qa:session_name} …}` plus `'new session...' n` wrapping `command-prompt -p 'new session:'`.
- prefix m (example), full.tmux.conf:107-128, `"#[align=centre]#{session_name}"`, `-x C -y C`, 15 rows; three `confirm-before -p '… (y/n)'` (:112 :117 :128); two `command-prompt -I` (:115 :124).

Three title styles across three menus.

## C. fzf pickers (eight)

**tm attach**, scripts/tm:182-192: `--height="$HEIGHT"` (TM_HEIGHT or 40%, :177) `--reverse --no-multi --color=16,border:8,scrollbar:8 --info=inline-right --cycle --scroll-off=2 --delimiter=$'\t' --with-nth=2`, prompt `'session> '`, header `'↵ attach   ^X kill   esc cancel'`, preview `tmux list-windows -t "=$(echo {1}):" -F '$PVFMT'` with `--preview-label=' windows ' --preview-label-pos=2 --preview-window='down,50%,border-top'`, bind `ctrl-x:execute-silent('$SELF' --kill-one {1})+reload(…)`. Fallback: `select` after `printf 'Sessions:\n' >&2` (:194-197).

**tm kill**, scripts/tm:134-138: `--multi --height="${TM_HEIGHT:-40%}" --color=16,prompt:red,pointer:red,marker:red,border:8,scrollbar:8 --marker='✗'`, prompt `'kill session> '`, header `'↵ kill   tab mark several   esc cancel'`, no preview. Fallback :140-141.

**tmux-branch**, scripts/tmux-branch:233-241: `--print-query --expect=alt-enter --query="$query" --reverse --no-multi --info=inline-right --cycle --scroll-off=2 --color=16,border:8,scrollbar:8 --delimiter=$'\t' --with-nth=2 --bind 'ctrl-/:toggle-preview,ctrl-u:preview-half-page-up,ctrl-d:preview-half-page-down'`, prompt `"switch> "`, two-line header (`↵ switch   alt-↵ new branch from what you typed   ^/ preview   esc cancel` / `on ${current:-detached HEAD} · $(tail3 "$DIR")`), preview `git log --color=always --oneline --graph --decorate -30 {1}`, `--preview-window="down,$psz,border-top"` (psz from `tput lines`, :221-224). The picker loops (:231) so a failed switch keeps the query. Fallback :196-205. Exit: `--expect` line 2, query line 1, pick line 3 (:252-254).

**review loop**, scripts/tmux-review:1248-1266: `--print-query --expect=ctrl-s,ctrl-b,ctrl-a --disabled --multi --ansi --delimiter=$'\t' --with-nth=4 --reverse --info=inline-right --cycle --scroll-off=2 --color=16,border:8,scrollbar:8`; 11 binds (enter/alt-enter execute-silent(attach)+reload+clear-query; ctrl-r resnap; ctrl-x detach; ctrl-l list; ctrl-e edit; ctrl-y copy; alt-c; alt-y; `ctrl-/` toggle, `alt-u/alt-d` half-page, `alt-up/alt-down`); preview `cd <dir> && tmux-open-path --preview-diff {1} {5}`, `--preview-window="down,55%,border-top,+{5}-/2"`; prompt `'review> '`, `--header-first`, four header lines (keys ×3, then `root: <dir>   base: $label (^B → $nxt)$ndlv   ● pinned   ↺ last round`). No fallback: display-message "the review loop needs fzf" (:1157-1161). Exit codes 20 deliver (:1384), 21 approve (:1362), handed to `run-shell -b` at :1970 because "a popup kills its own children the moment it closes" (:1908-1910).

**esc-confirm**, tmux-review:1282-1284: `fzf --reverse --no-info --prompt='esc? ' --header="$n comment(s) are pinned and not delivered"`, rows leave / keep reviewing / discard. **^S-confirm**, :1380-1381: `--prompt='^S? ' --header="deliver $n comment(s)?"`. Neither passes `--color`.

**fleet queue**, tmux-review:2247-2257: `--delimiter=$'\t' --with-nth=2,3,4,5 --reverse --info=inline-right --cycle --scroll-off=2 --color=16,border:8,scrollbar:8 --bind 'ctrl-/…,ctrl-u…,ctrl-d…' --expect=ctrl-x`, preview `--dir {1}` → `--preview-diff`, `--preview-window="down,$psz,border-top"` (psz = `avail - lrows - 6` at :2215, explained :2211-2214), prompt `'fleet> '`, two-line header (`which agent needs you   [◆ review pending · ↵ pasted, unsent]  state · title · repo⑂branch · ±` / `↵ review, or jump to a clean pane   ^/ preview   ^X kill window   esc cancel`), optional `--sync --bind "start:pos($pos)"` (:2229), `@cockpit_fleet_fzf_options` eval'd last (:2244-2245, confined to this picker :2237-2242). Loops (:2176). No fzf: display-message (:2171-2174).

**open-path list**, tmux-open-path:786-798: `--reverse --info=inline-right --cycle --scroll-off=2 --color=16,border:8,scrollbar:8 --delimiter=$'\t' --with-nth=4 --multi --keep-right --expect=ctrl-o`, binds `ctrl-/`, `ctrl-u/d`, `ctrl-g:change-preview(--preview-diff {2})+change-preview-label( diff )`, `ctrl-f:change-preview(--preview {2} {3})+change-preview-label( content )`, `alt-c` copy paths to `load-buffer -w`; prompt `'open> '`, two-line header; `--preview-label=' content ' --preview-window="down,$pshare,border-top"` (pshare = `avail - lrows - 5`, :771). Fallback prints to **stdout** (:834-836).

**open-path browse**, tmux-open-path:727-740: same, `--bind 'load:down'`, prompt `"$(tail3 "$dir")/ "`, `--preview-window='down,65%,border-top'` (fixed; the only picker not sizing off `tput lines`). No fzf: `note` (:760).

No `--listen`, no `become()`, no `--tmux` anywhere. Only `tm` uses `--height`.

## D. `read` / `printf` prompts and one-shot text screens

- tmux-fixer:100-101: `read -r -p 'side fix (one line, ↵ alone cancels): ' job` / `read -r -p 'hold off commands matching (ERE, ↵ for none): ' guard`; non-tty branch :102.
- tmux-fixer:60-61 failure screen: `'…claude exited %s — the other agent has been told the job did not finish cleanly.\n↵ to close\n'`, `read -r _`.
- tm:58 `read -r -p "New session name (empty cancels): " n`.
- tmux-review:2076-2121 `--info`: 11 printf lines, ends `'\n↵ to close\n'`.
- tmux-review:1433-1437 `--list`: `cat store; printf '\n(press any key)\n'; read -r -n 1 -s`.
- tmux-review:1420-1424 `--resnap`: "press any key".
- tmux-review:2135-2148 fleet ^X: `'kill window %s' / 'kill session %s (last window, %s panes)? [y/N] '`, `read -r -n1`.
- tmux-branch:60 `pause() { printf '\n[any key]'; read -rsn1; }`.
- full.tmux.conf:120 example agent list: `column -t` then `read -n1 -s`, no on-screen hint.

Five close idioms: `[any key]`, `press any key`, `(press any key)`, `↵ to close`, silence; plus `esc cancel` / `esc leave` inside fzf headers.

## E. `display-message` flashes, empty and error states

The rule, tmux-review:1158-1160: "display-message, not stderr: inside display-popup -E stderr of a command that exits at once is rendered to nobody." Same reasoning at tmux-open-path:661-663 and tmux-branch:57-59.

Load-time: cockpit.tmux:124, :201, :213, :237, :428. Runtime in tmux-review: :968 :974 :994 :1160 :1219 :1224 :1303 :1344 :1355 :1359 :1364 :1373 :1513 :1523 :1530 :1545 :1572 :1590 :1663 :1664 :1942 :1984 :2140 :2172 :2180 :2285 :2363. tmux-fixer :93 :96 :106 :109 :121 :123. tmux-jump:57. tmux-open-path:440 `note()`, :598. review-gate:147. Sticky (`-d 0`): only :1663 and :1664.

To stderr instead (invisible in a popup unless something pauses): tm:26 `die()`, tm:140, tm:194, tmux-branch:61 `fail()` (pauses first, so it renders), tmux-review:2008 :2012 :2015 :2065 :2312 :2393. tmux-open-path:835 prints its fallback prompt to stdout while tm and tmux-branch use stderr.

## F. Status line and borders

| | full.tmux.conf | tmux.conf | refresh |
|---|---|---|---|
| pane-border-style / -active | :216 / :217 | :22 / :23 | every redraw |
| pane-border-format | :220 | :24 | every redraw |
| window-style tint | :226 (:229) | :34 (:35) | every redraw |
| window tab dot | :199 / :200 | :42 / :43 | every redraw |
| status-left/right | :249 :253 :258 :260 | :52 (guarded :51) | `status-interval 2` (:232) + 4 `set-hook … refresh-client -S` (:234-237) |

Divergences: full.tmux.conf:199-200 read `@cockpit_win_state` and walk nothing (comment :195-198); tmux.conf:42-43 still run five `#{P:}` walks per window per redraw. tmux.conf:24 appends `#{=/48/…:#{@panestate_msg}}` after `─ NEEDS YOU`; full.tmux.conf:220 does not. tmux.conf:42-43 append the context suffix `#{?#{e|>=:#{@panestate_ctx},80},#[fg=colour208] ◔#{@panestate_ctx}%,}`; full.tmux.conf does not (colour208 is the one hardcoded non-theme colour, :39-41). Both keep tmux's `#[range=control|8]` zoom and `|9` kill buttons in pane-border-format (:220, :24; reason tmux.conf:15-17).

## G. Format-string census

- `#{E:}` cockpit.tmux:78 `#{E:@cockpit_fleet}` substituted by `interpolate()` :80-82; per redraw. Replaced an inline `#{n:#{S:#{W:#{P:}}}}` walk (:63-78, "360 pane visits per redraw with the fleet enabled").
- `#{qa:}` and `#{S/n:}` cockpit.tmux:400, once per prefix M.
- `#{s/^✳ //:}` tmux-review:62 (`AGENT_TITLE_FMT`, per prefix r and `pane_label` :1037); full.tmux.conf:199-200; tmux.conf:42-43 (per redraw).
- `#{m:}` the agent test, chosen over `#{m/r:}` for cost (cockpit.tmux:107-109: "32-36us per test, 25 tests per config, the whole redraw dropped from 16.2ms to 7.7ms at six windows"). Per redraw: full.tmux.conf:199 :200 :216 :217 :220 :226; tmux.conf:22 :23 :24 :34 :42 :43. Per keypress: full.tmux.conf:120; tm:181 (per cursor move); tmux-jump:39; tmux-review:1520 :1662 :1706 :2316. Per hook: agent-state:266 :295 :308 :309 :315. `#{m/r:}` only full.tmux.conf:83.
- `#{q:}` zero live uses; rejection notes cockpit.tmux:54-59 ("passes NEWLINE and TAB straight through"), tmux-where:22, :52.
- `#{P:}` / `#{S:#{W:}}` walks: tm:181 (per cursor move); agent-state:266 :295 :308 :309 :315 (per transition); full.tmux.conf:220 and tmux.conf:24 (per redraw); tmux.conf:42-43 ×5 each (per redraw, the remaining hot walk).
- `#{b:}` popup titles only.

## H. Test coverage (tests/run.sh, 157 blocks: `grep -c 'echo "== ' tests/run.sh`)

Tested: shipped config renders :150; usable as shipped :180; example menu Switch branch :223 :263 :2289; prefix b pane resolution :240; agents menu :2251; move menu :2326 :2356; prefix g + u :2216; prefix r/R :7097; prefix R multi-config :7225; prefix i :7170; popup geometry :2540; branch popup pane id :2557; tm :920 :1566 :2941; tmux-branch :948 :1426 :1448 :1488 :1515 :1794; path picker :868 :1881 :2580 :2622 :2726 :2471 :2912; preview-diff :3714 :5530 :5562 :5594 :5633 :5658; review loop keys :4655 :4689 :4827 :4863 :7296 :7335 :7371 :7448 :7469 :7495 :7530 :7655; ^A :5327; --request :5480; deliver :5683 :5767 :5819 :5856 :5903 :5988 :6079; fleet :6542 :6575 :6824 :6892; fleet ^X :8148; side fix :7948 :7969; review-gate :8031 :8073; border buttons :2131; click gestures :1925 :2096; legend fits 80 cols :4636; preview border :4602.

Not tested: full.tmux.conf:120 popup; all four fzf-missing `select` fallbacks (tm:141, tm:195, tmux-branch:199, tmux-open-path:836; no block runs with fzf off PATH, :2598 notes only the opposite hazard); the browser's `^G/^F` binds; tm's `-h` text.

## I. Inconsistencies

1. tmux-review:1663 ends `'… press Enter there to send; prefix J walks there'`; the jump key is `u` since #43 (cockpit.tmux:264). Comments at tmux-jump:3, :50, agent-state:220, tmux-review:2293 carry the old keys too.
2. tm:37-38 `In the picker: ↵ attach   k kill   / search   esc cancel` vs the live header tm:187 `'↵ attach   ^X kill   esc cancel'`; bare-k mode was removed (:166-171).
3. Four popup geometries: $PW/$PH; `-h 6`; 78%/40%; 70%/70%; 60%/50%.
4. Two popups with no title (full:120, :123).
5. Preview sizing: down,50% (tm), 55% (review), 65% fixed (browse), `avail - lrows - 5` (tmux-branch:223, tmux-open-path:771) vs `- 6` (tmux-review:2215).
6. Preview scroll keys `ctrl-u/d` everywhere but the review loop (`alt-u/d`, reason at :1236-1239: readline owns ^U/^D in a comment field).
7. Only tm's kill picker recolours for destructiveness (tm:136); tmux-review's ^X and the esc-discard picker use the neutral palette.
8. tmux-open-path:835 prints its no-fzf prompt to stdout; tm:140, tm:194, tmux-branch:198 to stderr.
9. full.tmux.conf:122-123 hardcode `~/.local/bin/tmux-branch` and `~/.local/bin/tm`; the plugin's own binds go through `$DIR` (cockpit.tmux:152-154).
