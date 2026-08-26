# prefix r messages and the empty queue (issue #49, UX lens)

Research lane: UX (opus, read-only). No code changes. Evidence is file:line in the repo, plus observations on a private server (`tmux -L ux49` with the owner's `~/.tmux.conf`; `tmux -L ux49v -f /dev/null` for a virgin baseline), both killed at the end. Live server read-only throughout.

## 0. Six measured facts about where messages land

These decide most of the answers below, so they come first.

1. **A `display-message` issued from inside `display-popup -E` survives the popup's close.** Popup ran `tmux display-message "tmux-review: no agent pane is blocked or has uncommitted changes"` and exited at once. Status line at t+0.4 s and t+1.4 s: message present, popup gone. t+2.6 s: gone. So the empty-queue flash *is* seen, for `display-time` after the popup vanishes.
2. **`display-time` is 2000 ms on the owner's box** (`show-options -g display-time` on the live server; `examples/full.tmux.conf:15` sets it). tmux's own default is 750 ms (measured on virgin `ux49v`). Anyone on stock tmux gets the notice for three quarters of a second.
3. **On the owner's theme the message does not clear the status line, it is painted into the middle of it.** Captured verbatim: `t 1 sh    tmux-review: no agent pane is blocked or has uncommitted changesockpit ⑂ master ⏰ 21:11`. The message runs straight into leftover status-right text with no separator. On the virgin server the same message replaced the whole line cleanly. So the one notice that has to explain a surprising result is, on the owner's actual config, a fragment welded to a truncated path.
4. **stderr from a popup command that exits immediately is rendered to nobody.** Popup ran `printf "..." >&2`; capture at t+0.15 s shows an empty popup body, and nothing after. Every `printf ... >&2` reachable under `prefix r` is therefore dead text.
5. **A message longer than the client width is silently truncated.** 160 `x` plus `END` on a 120-column client rendered exactly 120 `x` and no `END`. No ellipsis, no wrap.
6. **`display-message -d 0` stays until any keypress.** Still on screen 3 s after the popup closed, cleared by one key. Already used at `tmux-review:1650`.

## 1. Inventory of the prefix r flow

Binding: `cockpit.tmux:275` `display-popup -E -w 70% -h 70% -T ' review · #{b:pane_current_path} '` running `tmux-review -p $SRC_PANE`, then `main_entry` (`:2272`) -> `fleet_queue` (`:2149`) -> `fleet_rows` (`:1687`) -> `review_pane` -> `run_loop`.

| # | Message | file:line | Where it appears | How long | Actionable |
|---|---|---|---|---|---|
| 1 | popup title `review · <basename of cwd>` | cockpit.tmux:275 | popup border | while open | no |
| 2 | `tmux-review: no tmux client to resolve a pane from` | 2280 | stderr in popup, then exit | **never seen** (fact 4) | no |
| 3 | `tmux-review: the fleet queue needs fzf` | 2151 | status flash, popup closes | display-time | names the fix |
| 4 | `tmux-review: no agent pane is blocked or has uncommitted changes` | 2159 | status flash, popup closes | display-time | **no** |
| 5 | fleet header, 2 lines, 94 + 51 chars | 2235 | inside popup, above the list | while open | yes |
| 6 | `fleet>` prompt | 2234 | inside popup | while open | yes |
| 7 | `kill window s:w, N panes? [y/N]` | 2136-2145 | inside popup, blocks on a key | until answered | yes |
| 8 | `tmux-review: pane %N is gone` (kill path) | 2122 | flash, queue loops | display-time | no |
| 9 | `tmux-review: pane %N is gone` (open path) | 2253 | flash, queue loops | display-time | no |
| 10 | `tmux-review: resumed N pinned comment(s)[, M no longer anchor (^R lists them)]` | 1206 | flash while popup open | display-time | yes, names `^R` |
| 11 | `tmux-review: no uncommitted changes` | 1211 | flash, loop exits back to queue | display-time | **no** |
| 12 | review header, 4 lines, incl. `root: … base: …` | 1253 | inside popup | while open | yes |
| 13 | `N comment(s) are pinned and not delivered` (esc picker) | 1271 | inside popup, blocks | until answered | yes |
| 14 | `; comments kept at <store> — deliver later with: …` | 1490-1492 | appended to a flash | display-time | yes, copy-pasteable but truncates (fact 5) |
| 15 | `tmux-review: nothing to review against <label>` | 1331 | flash, popup open | display-time | partly |
| 16 | `tmux-review: deliver or discard the N pinned comment(s) first` | 1342 | flash | display-time | yes |
| 17 | `tmux-review: nothing to approve against before the first commit` | 1346 | flash | display-time | no |
| 18 | `tmux-review: could not write refs/cockpit/reviewed/<br> — a stale ref may block it: git update-ref -d …` | 1351 | flash | display-time | yes, best message in the file |
| 19 | `tmux-review: nothing pinned` | 1360 | flash | display-time | no |
| 20 | `deliver N comment(s)?` picker | 1368 | inside popup, blocks | until answered | yes |
| 21 | `tmux-review: nothing pinned on this row` | 963, 983 | flash | display-time | no |
| 22 | `tmux-review: <store> does not parse; set aside as .bak` | 957 | flash | display-time | partly |
| 23 | `tmux-review: cannot find <lib>/review-store.sh` | 966 | flash | display-time | yes |
| 24 | `tmux-review: file + diff copied` / `line(s) copied` / `the whole diff copied` | 1186, 1187, 1191 | flash | display-time | no, confirmation |
| 25 | `review approved — the next one shows only what changes from here` | 1971 | flash | display-time | no, confirmation |
| 26 | `tmux-review: could not create <sess>` | 1929 | flash | display-time | partly |
| 27 | `tmux-review: no VS Code window reachable, using vi` | 2331 | flash | display-time | no |
| 28 | six delivery refusals, each `+ keep_hint` | 1500, 1510, 1517, 1532, 1559, 1577 | flash | display-time | yes |
| 29 | `review pasted into <where> (%N) — press Enter there to send; prefix J walks there` | 1650 | flash, `-d 0` | **until a key** | yes |
| 30 | `review queued for <where> (%N) — it lands when the agent next stops` | 1651 | flash, `-d 0` | until a key | no |
| 31 | `tmux-review: the review loop needs fzf` | 1147 | stderr inside popup | **never seen** | no |

Siblings: `tmux-jump:57` `no agent needs you` (run-shell, not a popup, so always visible for display-time). `tmux-fixer` has one stderr line at `:92` (only reachable outside the popup) and six `display-message` at `:93, 96, 106, 109, 121, 123`; `:123` is the model message in this repo, it names the pane, the job and the consequence. `review-gate:147` `held: <reason> (side fix in %N) — %N resumes when it finishes` fires from a hook, no popup involved.

Two structural defects fall out. `#2` and `#31` are unreachable text: any `printf >&2` on a path that exits inside the popup should be a `display-message`. And rows 4 and 11 are the two messages that fire on the exact paths issue #49 is about, and neither is actionable.

## 2. The empty-queue case

What the user actually needs, ranked by what they cannot currently tell apart:

- how many panes matched `@cockpit_agents_glob` at all (live value: `|claude|codex|aider|cursor|opencode|gemini|crush|`),
- of those, how many had a readable repo root,
- of those, how many read clean against their base,
- and, for the wrong-tree case, *which tree each one was diffed against*.

Today the message names two conditions out of at least four states and implies zero agents. Measured on the live server at 21:14: four agent panes, `@panestate` running/done/unset, every `pane_current_path` the main checkout. `scripts/tmux-review --fleet` prints nothing, rc 0, 99 ms.

The status segment agrees with the queue and is just as silent: `#{agent_fleet}` reads the precomputed `@cockpit_fleet` (cockpit.tmux:78, agent-state:298-317), which counts only `needs-input`, `error` and `@cockpit_review = pending`. Live value read back with `show-option -gqv @cockpit_fleet` is the empty string, so `status-right` renders nothing for four live agents. `tmux-jump` uses the same three predicates (`tmux-jump:31-38`) and would also say `no agent needs you`. Three surfaces, one blind spot: **"running" and "done" are states no surface reports.**

That is why I do not recommend spending a popup screen on a better empty message. The empty state is a symptom; the fix is that the queue should almost never be empty (section 3). Keep it to one honest flash, and make it honest about counts:

    tmux-review: 4 agent panes, none in a readable git tree — prefix J walks them

    tmux-review: no agent pane on this server (glob: |claude|codex|…)

Where: status flash, not a popup screen. A popup that waits for a key costs a keystroke on every miss, and the `--info` precedent (`cockpit.tmux:292`, `… --info; read -r _`) exists only because that popup has content worth reading. There is nothing to read here. If the owner wants it to persist, `-d 0` (fact 6) makes it stay until any key at zero cost, and `tmux-review:1650` is the precedent.

Note the width trap (fact 5): the glob is 44 characters, so the second wording only fits above roughly 100 columns. Drop the glob from the message and put it in the `--info` popup instead.

## 3. Clean agents in the queue

The filter is one line, `tmux-review:1793`:

    [ "$dirty" = 1 ] || [ "$state" = needs-input ] || continue

It contradicts the code around it. The rank table immediately below it (`:1795-1801`) enumerates `needs-input, done, error, running, unset` as five ranks, and `fleet_scan_root` already prints `stat='—'` for a clean tree with the comment "Nothing to review, only someone to unblock" (`:1884`). The ordering and the display were built for clean rows; only the filter drops them.

Of the three options in the brief:

- **A toggle key in fzf.** Costs a header slot, and header line 1 is already 94 characters against a 133-column popup on the owner's 191-column client (`PW` default 70%, cockpit.tmux:209). The code comment at `:2226-2230` records that this line already truncated below 136 terminal columns. Rejected.
- **A second message line naming the clean agents.** A flash cannot be acted on, and it duplicates the list. Rejected.
- **Always list every agent pane.** Delete line 1793. Nothing else in `fleet_rows` changes: phase 2 already runs `fleet_scan_root` once per unique root *before* the filter (`:1772-1780`), so the git work is already paid for. Measured on the live fleet with a patched copy in a scratchpad: stock `--fleet` prints nothing in 99 ms; with 1793 removed it prints four rows in 313 ms, each `state · title · tmux-cockpit⑂master · —`. Titles came out useful ("Left click issue", "Review agent access and flow research").

**Recommend deleting line 1793.** Header line 1 is unchanged. Two follow-on changes are needed to make it not a lie:

1. **`↵` on a clean row must jump, not review.** Today `fleet_queue:2255` calls `review_pane` unconditionally, `run_loop:1209-1214` finds `nrows == 0`, flashes `no uncommitted changes` and exits back to the queue. With clean rows listed that becomes the common outcome. Section 8 decision #3 already ruled on this ("↵ on such a row = jump, not review", `docs/superpowers/plans/2026-08-25-review-section8-decisions.md`) and it was never implemented. It is four lines in `fleet_queue`: if the row's stat column is the clean sentinel, `switch-client`/`select-window`/`select-pane` at the pane and return, the way `tmux-jump:69-72` does.
2. **Split the stat sentinel.** `—` currently means two different things: clean tree (`:1884`) and no git repo at all (`:1840`). Under the filter you almost never saw either; unfiltered, `—` becomes the most common cell. Make them `clean` and `no repo`. Also note the state column defaults to `—` too (`:1801`, `${state:-—}`), so an unset-state clean agent renders `— … —` and neither dash means what the other does. Live pane `%5` has `@panestate` unset right now, so this is not hypothetical.

Header line 2 has room (51 chars) for the changed contract: `↵ review or jump   ^/ preview   ^X kill window   esc cancel`.

## 4. The wrong-tree case

With line 1793 gone, the four live panes render as `tmux-cockpit⑂master … clean`. The agent whose work is in `~/.cache/cockpit-wt/lowercase-keys` on branch `lowercase-keys` shows up as `master`, `clean`. **The `repo⑂branch` column is the tell, and it is already on the row** (`:1802`). It reads wrong only if you know what branch the agent should be on, which the owner does.

The `root:` line does exist, at `tmux-review:1253`, in the review loop's header (`root: ~/.tmux/plugins/tmux-cockpit   base: uncommitted`). **It does not help here**, because `run_loop` exits at `:1209-1214` on `nrows == 0` before that header is ever built. The one line that would name the wrong tree is on the far side of the early exit.

Two cheap fixes, in order of value:

- Make the `no uncommitted changes` flash name the tree it looked at, so the seam is visible without opening anything: `tmux-review: %19 is clean in ~/.tmux/plugins/tmux-cockpit ⑂ master` (see section 5).
- In the preview pane. `fleet_queue:2213-2216` builds the preview by re-asking tmux for `#{pane_current_path}` and resolving the root, then handing it to `tmux-open-path --preview-diff`. On a clean row that preview is empty. Print a two-line banner there instead: the resolved root, the branch, and the other worktrees of the same repo (`git -C "$root" worktree list`, one fork, only on the highlighted row). That is where "the agent's work is in a sibling worktree" becomes visible without any new detection mechanism.

The cwd-detection fix is the mechanism half of #49 and lives in flow.md.

## 5. Rewrites, one line each

| file:line | Today | Proposed |
|---|---|---|
| 2159 | `no agent pane is blocked or has uncommitted changes` | `4 agent panes, none in a readable git tree — prefix J walks them` (counts filled in; `no agent pane on this server` when the glob matched zero) |
| 1211 | `no uncommitted changes` | `%19 is clean in ~/.tmux/plugins/tmux-cockpit ⑂ master` |
| 1331 | `nothing to review against <label>` | `nothing changed since <label> — ^B tries <next base>` |
| 1346 | `nothing to approve against before the first commit` | `^A needs a commit to mark — nothing on this branch yet` |
| 1360 | `nothing pinned` | `nothing pinned — ↵ on a row pins the typed comment` |
| 963, 983 | `nothing pinned on this row` | `nothing pinned on this row — ● marks the ones that are` |
| 2122, 2253 | `pane %N is gone` | `pane %N closed while the list was open — the list refreshed` |
| 2331 | `no VS Code window reachable, using vi` | `no VS Code window reachable — opened vi instead; set @cockpit_editor to pick` |
| 2151 | `the fleet queue needs fzf` | fine as is, names the missing binary |
| 1517 | `nothing to deliver` | `nothing to deliver — the store is empty` |
| 2280, 1147 | stderr | make both `display-message`; as stderr inside the popup they are never rendered (fact 4) |

Rule the audit applied: name the thing that was looked at, not the predicate that failed, and end on the key or command that moves you forward. `:1351` (the stale-ref message) and `tmux-fixer:123` already do this and are the voice to copy.

## Recommendation

Smallest change that stops the owner ever seeing an empty popup while an agent is live, in order:

1. Delete `scripts/tmux-review:1793`. Every agent pane gets a row. Free, the scan already ran. Measured: 4 rows, 313 ms.
2. Split the stat sentinel into `clean` and `no repo` (`:1884`, `:1840`), so the row says which of the two it is.
3. `↵` on a clean row jumps to the pane instead of opening a review that immediately closes (`fleet_queue:2255`, the section 8 #3 ruling, about 4 lines). Update header line 2 to `↵ review or jump`.
4. Rewrite `:2159` and `:1211` per section 5. `:2159` now only fires when no pane matched the glob or none resolved a git root, which is a rare and genuinely different situation, and it should say so with counts.
5. Convert `:2280` and `:1147` from stderr to `display-message`.

Items 1 to 3 are the fix for "if there is nothing to review I still need to see it". Item 4 is the fix for "cockpit says I have no review agent". None of them touches the cwd-detection root cause, and none of them is invalidated by whatever that fix turns out to be.

## Open questions

- Should a clean row still be reachable for review at all? A user may want the popup on a clean tree to read `base: branch` history via `^B`. Suggestion: `↵` jumps, and the review stays available from the pane itself. Needs the owner's call.
- Does `@cockpit_fleet` (`agent-state:298-317`) also need a `running`/`done` count, so `status-right` stops rendering empty with four live agents? It is a whole-server walk per counter and the freeze rig gates that, so this is a real cost, unlike items 1 to 5. Out of scope for #49 unless the owner asks.
- `display-message -d 0` for the empty notice: better (it waits for the owner) or worse (it eats the next keystroke)? Measured to work; not recommended by default.
- The status-line paint artifact (fact 3) is a theme interaction, not a cockpit bug, but it makes every flash in this table harder to read on the owner's box than on a stock server. Worth its own issue if it bothers them.
