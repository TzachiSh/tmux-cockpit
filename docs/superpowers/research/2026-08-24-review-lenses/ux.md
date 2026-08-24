# UX lens: the human's end-to-end journey through `prefix r`

Scope: the *journey*, not the pixels. Persona below. Evidence tags: **[E]** = I ran it,
read it, or fetched it; **[I]** = inference from evidence. Severity uses the standard
heuristic-evaluation scale: **1** cosmetic · **2** minor · **3** major (blocks the task or
costs real time) · **4** catastrophe (data loss, dead end, or wrong result delivered).

---

## 0. What I actually measured

Everything below marked [E] came from one of these, not from reading:

| Probe | Setup | Result |
|---|---|---|
| **Popup modality** | nested tmux servers (`-L outer` hosting a client attached to `-L inner`), `display-popup -E` on the inner client, `send-keys C-b n` through the outer pane | Prefix keys are **swallowed by the popup**: active window stayed `one` before and after `prefix n`. tmux 3.7c. [E] |
| **Background panes during a popup** | same rig, a pane printing `TICK-n` every second | Panes **keep running and keep redrawing** behind the popup (TICK 8 → 14 across 5s). The popup only *occludes* them. [E] — this contradicts the widely repeated "panes are not updated while a popup is present" claim, at least on 3.7c. |
| **fzf reload cursor** | fzf 0.44.1, 200 rows, `--reverse --disabled`, `alt-enter:execute-silent+reload+clear-query`, 30 × ↓ then alt-↵ | Cursor **stays on row-031**. fzf preserves the cursor *index* across `reload()`. [E] — a hypothesised catastrophe that is **not** a defect. Credit where due. |
| **Row-build latency** (the pause after every comment) | synthetic repos, `tmux-review --rows` | 480 rows → **0.08s**; 4500 rows → **0.40–0.79s**; >5000 → refuses entirely. [E] |
| **Mid-review agent edit** | 3-line repo, pin a comment on `f.txt:3 (+) GAMMA-EDIT`, then insert a line above, then re-run `--rows` | The `●` marker **moves to a line the human never commented on** (`BETA-EDIT`), and the commented line shows unmarked. Silent. [E] — reproduction transcript in §3, defect #1. |
| **Header truncation** | fzf 0.44.1 at 84 cols and 70 cols (= `@cockpit_popup_width` 70% of a 120- and 100-col terminal) | 84 cols: `…esc discard (ent..` · 70 cols: `…^S deliver   es..`. The destructive key and the no-op explanation truncate first. [E] |
| **Delivery payload threshold** | `--compose` over a growing synthetic store | **~13 comments = 2048 bytes.** At 15 comments the human's review is replaced by a one-line file pointer. [E] |
| **tmux `display-time`** | `show-options -g` on a virgin 3.7c server | **2000 ms.** The only instruction telling you to press Enter lives for two seconds. [E] |

Comparison set, verified via `gh api` on 2026-08-24 [E]:
`modem-dev/hunk` **8,711**★ (pushed today) · `backnotprop/plannotator` **7,985**★ (today) ·
`yoshiko-pg/difit` **3,095**★ (2026-08-09) · `agavra/tuicr` **2,868**★ (2026-08-20, *created
2026-01-08* — the fastest riser in the category) · `umputun/revdiff` **807**★ (2026-08-20) ·
`nilbuild/diffity` **758**★ · `google/git-appraise` 5,309★ but **dead since 2023-08**.

---

## 1. The persona and the journey

> **Dana.** Six Claude Code panes across two tmux sessions, two of them in worktrees. Wants
> to be the gate between each agent and `git commit`. Has 20–40 minutes of attention to
> spend across all six. Lives in tmux; `prefix` is muscle memory; `↵` is the most-trained
> keystroke she owns.

### Journey map

| # | Stage | What Dana does today | Friction | Sev |
|---|---|---|---|---|
| **1** | **Discovery** — "which pane needs review?" | Nothing tells her. The plugin has a whole attention system (`@panestate` → borders, status dots, `prefix J`) and the review feature is wired to **none of it** (`cockpit.tmux:242-251`; `scripts/tmux-review:573-621` reads only `pane_current_command` + `pane_current_path`). She has to *remember* to press `prefix r` on a hunch. | Review is pull-only in a plugin whose entire premise is push notification. Nielsen #1 (visibility of system status). | **4** |
| **2** | **Entry** — `prefix r` | 2 keys. But **which** thing opens depends on where the cursor happens to be sitting (`main_entry`, :705-720). From an agent pane → that pane. From anywhere else → the fleet queue. Dana cannot ask for the queue from an agent pane, or for a specific pane from a shell. | Modal, invisible, non-reversible mode selection. Nielsen #3 (user control). | **3** |
| **3** | **Entry, cont.** — the popup opens | `display-popup -E`. **[E]** Every other pane keeps working behind it, and **every prefix key is swallowed** — Dana cannot glance at pane 5, cannot answer the agent that just hit `needs-input`, cannot even switch windows. Her whole terminal is one review until she leaves it. Agents that block mid-review sit blocked. | Hard modality with no escape hatch, in a *fleet* tool. Nielsen #3, #7 (flexibility). | **4** |
| **4** | **Entry, cont.** — the agent keeps editing | Nothing freezes the diff. Each `reload()` re-runs `git diff` against a moving tree. **[E]** Markers drift onto lines she never commented on, silently. | Nielsen #1, #5 (error prevention). Silent wrong state. | **4** |
| **5** | **Orientation** — "where am I?" | The list gets **~8 rows** (`lrows=$nrows; [ "$lrows" -gt 8 ] && lrows=8`, :381-383) — an 8-row porthole over what is routinely 200–800 rows. One row **per changed line**, path repeated on every row, no file headers, no hunk grouping, no context lines, no collapse, no next-file/next-hunk key. `--disabled` also removes fzf's own search, so she cannot type a filename to get there. Navigation is ↓ held down. | Nielsen #6 (recognition over recall), #8 (aesthetic/minimalist). | **4** |
| **6** | **Orientation, cont.** — the preview | `--preview 'cd DIR && tmux-open-path --preview-diff {1}'` (:364). `{1}` is the **file path only** — the preview always renders that file's diff **from the top** and never scrolls to the line under the cursor. On row 300 of a file, the preview shows line 1. | The one component that could give context, doesn't. Nielsen #1. | **4** |
| **7** | **Orientation, cont.** — progress | No "12 of 341", no reviewed/unreviewed marks, no per-file completion. `--info=inline-right` shows only `341/341`. Both competitors ship this (`revdiff`: `Space` marks a file reviewed + `f`/`F` filter; `tuicr`: `r` file / `R` hunk, persisted across sessions) [E]. | She cannot tell how much is left, so she cannot decide whether to finish. Direct approval-fatigue driver. | **3** |
| **8** | **Commenting** — the surprise | She presses `↵`. It is bound to `ignore` (:401). **Nothing happens.** No bell, no message, no flash. Two seconds earlier, in the *fleet queue of the same popup*, `↵` selected a pane — and `↵` accepts in every other fzf picker this plugin ships. | Textbook **mode error** (Norman). The system trains `↵` then punishes it, and the explanation `(enter does nothing)` is **[E]** the first text truncated off the header at the default 70% popup width. | **4** |
| **9** | **Commenting** — the box | The fzf query line is the comment box. **Single line only.** No newlines, no `$EDITOR` handoff. Readline's `^U`/`^W`/`^A` work but `^U`/`^D` were deliberately moved off preview-scroll *because* they edit the comment (:394) — so Dana has readline editing she is not told about and preview scroll on `alt-u`/`alt-d` she will not guess. Both competitors ship multi-line + editor handoff (`revdiff` `Ctrl+E` → `$EDITOR`; `tuicr` visual-mode ranges + `comment_vim`) [E]. | "Rejection has high friction, approval takes one keystroke" — the approval-fatigue pattern, made structural. | **3** |
| **10** | **Commenting** — no range | One comment = one line. No `v`/`V` visual range, no hunk-level, no file-level comment. revdiff has `A` (file-level) and hunk-range auto-expansion; tuicr has `c`/`C`/`v`/`V` [E]. Dana's real feedback ("this whole function is the wrong shape") has no home. | Forces her to fake it by picking an arbitrary line. Nielsen #7. | **3** |
| **11** | **Commenting** — no undo | The store is append-only by design (:319-343). There is **no key to delete, edit, or even see** a pinned comment. A typo, a wrong row, a duplicate — all permanent until `esc` nukes the entire batch. revdiff: `d` deletes, `@` lists. tuicr: `:summary` lists all pending drafts with `j`/`k` [E]. | Nielsen #3 (undo), #9 (recover from errors). | **4** |
| **12** | **Commenting** — the pause | **[E]** 0.08s at 480 rows (fine), **0.40–0.79s at 4500 rows**, after *every* `alt-↵`. Nielsen's 1.0s flow limit is not breached but the 0.1s "you caused this" limit is, by 8×, at exactly the diff size where she has the most comments to make. | Perceptible stutter compounding across 20 comments. | **2** |
| **13** | **Commenting** — the cliff | **[E]** Over 5000 changed lines the whole tool collapses to one row: *"diff too large to anchor line-by-line — review it directly."* No file list, no per-file drill-down, no way to review the four small files inside the 6000-line diff. | The tool abandons her precisely when the review matters most. Nielsen #9 offers no recovery path. | **3** |
| **14** | **Delivery** — `^S` | Popup closes. **No confirmation, no summary, no "you are sending 7 comments to `main:2 auth-worker`".** Copilot makes the human click a literal **"Submit Feedback"** button after reviewing the batch [E]; Orca and tuicr both show a sticky batch bar before sending. | Irreversible, unpreviewable send. Nielsen #2 (match to real world), #5. | **3** |
| **15** | **Delivery** — the instruction | The *only* thing telling Dana the review is not yet sent is `display-message "review pasted into %12 — press Enter there to send"` (:560) — **[E] a 2000 ms status-line flash** naming an opaque `%12`. No session, no window, no agent name. If she blinked, the review is now sitting typed-but-unsent in a pane she may not be able to identify. | The single most important message in the flow has a two-second half-life and an unreadable address. Nielsen #1, #6. | **4** |
| **16** | **Delivery** — the walk | She must now find `%12` herself (`prefix s`, navigate, `↵` — 4–6 keys across sessions), confirm the pasted text is what she meant, and press `↵`. `prefix J` cannot help: it cycles `needs-input` only (`tmux-jump:15-16`). | 2 context switches + manual pane archaeology, for the *most* time-critical step. | **3** |
| **17** | **Delivery** — >13 comments | **[E]** At 2048 bytes the payload becomes `Read and address every comment in /run/user/1000/tmux-cockpit-review-12-Xk3d9q …`. A thorough review is exactly the one Dana can no longer see before she sends it. | Correct instinct (agent-mux does the same), wrong threshold and no visibility. | **2** |
| **18** | **Delivery** — timing | `deliver()` guards on pane-id shape, agents glob, and non-empty store (:471-494). It does **not** check `@panestate` — the very signal this plugin invented. The repo's own research doc specifies *"Guards: %N pane id, agents glob match, @panestate not running"* (`review-antipatterns.md`, §"Safe delivery") and the shipped code dropped it. A review can land mid-tool-call. | The plugin knows the safe moment and ignores it. | **3** |
| **19** | **Follow-up** — "did it get addressed?" | The store is `rm -f`'d after delivery (:643). Re-opening review on that pane starts **empty, with no `●` marks**. There is no record that a review ever happened, no diff-since-my-review, no "3 of 7 comments still open". | The re-review loop — the whole point of reviewing an *agent* — is entirely unsupported. tuicr persists review state across sessions; revdiff auto-saves to `~/.config/revdiff/history/` [E]. | **4** |
| **20** | **The fleet** — next agent | After pressing `↵` in the agent's pane, Dana is **standing in an agent pane**. `prefix r` there re-reviews *that same pane*. To reach the fleet queue she must first navigate to a non-agent pane. | **The fleet loop is broken at exactly the point it should close.** The flow deposits her in the one location from which the next step is unreachable. | **4** |
| **21** | **The fleet** — ordering | `fleet_rows()` sorts by **churn** (:615-620). Churn is a *volume* proxy, not an *urgency* one. A pane still `running` (diff actively moving — the worst possible target) sorts first if it churned most; a pane sitting at `needs-input`, blocked on Dana right now, is indistinguishable from an idle one. No `@panestate`, no branch, no last-modified age, no test-files-touched flag in the row. | Ordering answers "who typed most", not "who needs me first". | **3** |
| **22** | **The fleet** — the title | The popup title is `' review · #{b:pane_current_path} '` (`cockpit.tmux:249`) — evaluated against **the pane Dana pressed the key from**. In fleet mode that is her shell's directory, and it stays wrong after she picks an agent. | Persistent wrong label on a modal window. Nielsen #1. | **2** |
| **23** | **Escape** | `esc` discards the batch, store and all, with **no confirmation** (:414-417). In fzf, `esc` is the reflex for "close the preview / clear the query / back out one level". revdiff separates these: `q` = quit-and-output, `Q` = discard [E]. | One reflexive keystroke destroys 20 minutes of work. | **4** |
| **24** | **Discoverability** | No `?` help, no key list beyond the truncating header. `--help` exists but is only reachable from a shell, not from inside the popup. revdiff and tuicr both ship `?` overlays [E]. | Nielsen #10 (help and documentation). | **2** |

**Journey totals for one 5-comment review of one of six agents [I], counted from the code paths above:**
≈ **320 keystrokes** (2 to enter · ~3 to pick the pane · ~61 per comment × 5 · 1 to deliver · ~5 to find the pane · 1 to send), **2 forced context switches**, **1 two-second window** to catch the only instruction that matters, and **1 dead end** at the point where the second review should begin.

---

## 2. Where the design is genuinely right (so the redesign doesn't regress it)

- **No accept-all.** Correct and well-argued. The fix below keeps it while removing the mode error.
- **Batch-only delivery.** Matches Copilot's "Submit Feedback" and Orca's stated failure mode [E, landscape doc].
- **Typed-but-unsent, never auto-submit, never steal focus.** Right for the reasons documented.
- **Quoted-fragment anchoring** rather than bare line numbers. Right.
- **Test files first.** A genuine differentiator (Addy Osmani's sharpest heuristic); nobody else does it.
- **The fleet queue exists at all.** Still the unclaimed gap — no tool in the comparison set has one.
- **[E] fzf preserves the cursor across `reload()`.** The in-place comment loop really does cost one popup, as the README claims.

---

## 3. Reproduction: the mid-review drift (defect #1)

```
$ tmux-review --rows . $STORE | cut -f4          # agent idle, one comment pinned
  f.txt:2 (-)  beta
  f.txt:3 (-)  gamma
  f.txt:2 (+)  BETA-EDIT
● f.txt:3 (+)  GAMMA-EDIT      <-- Dana commented on THIS line
  f.txt:4 (+)  delta-new

# ... the agent inserts one line while the popup is open ...

$ tmux-review --rows . $STORE | cut -f4          # after the very next alt-↵ reload
  f.txt:2 (-)  beta
  f.txt:3 (-)  gamma
  f.txt:2 (+)  INSERTED-BY-AGENT
● f.txt:3 (+)  BETA-EDIT       <-- marked, never commented on
  f.txt:4 (+)  GAMMA-EDIT      <-- commented on, now unmarked
  f.txt:5 (+)  delta-new
```

No warning is shown. If Dana now re-comments the unmarked `GAMMA-EDIT`, the delivered
review contains two blocks anchored at `f.txt:3` and `f.txt:4` claiming different things.
The quoted fragment saves the *agent* from acting on the wrong line; it does not save
*Dana* from the wrong screen. **[E]**

---

## 4. Top 10 UX defects, ranked

| # | Defect | Sev | Journey stage |
|---|---|---|---|
| 1 | **The diff is not frozen.** A mid-review agent edit silently re-anchors the list and moves the `●` marks onto lines the human never touched. **[E]** | 4 | 4 |
| 2 | **`↵` is a silent no-op** in a picker whose sibling picker, two keystrokes earlier, uses `↵` to select — and the header text explaining it is **[E]** the first thing truncated at the default popup width. | 4 | 8 |
| 3 | **The loop is a dead end for the fleet.** Delivery leaves Dana standing in an agent pane, and from an agent pane `prefix r` cannot reach the fleet queue. The second review of the session has no path. | 4 | 20 |
| 4 | **No undo, no edit, no visible batch.** Nothing can delete a wrong comment, nothing can show what is pinned, and the only escape (`esc`) destroys everything with no confirmation. Both competitors ship all three. | 4 | 11, 23 |
| 5 | **The delivery instruction is a 2000 ms flash naming `%12`.** [E] The one step the whole design hinges on (a human's own Enter) is announced worse than any other message in the plugin. | 4 | 15 |
| 6 | **No follow-up state.** The store is deleted on delivery; re-review starts blank. "Did the agent address my 7 comments?" is unanswerable by the tool that raised them. | 4 | 19 |
| 7 | **The popup is hard-modal.** [E] Prefix keys are swallowed; the other five agents are unreachable and unanswerable for the duration of a review, in a tool whose thesis is fleet monitoring. | 4 | 3 |
| 8 | **8-row porthole, no orientation.** [E] ~8 visible rows over 200–800 one-line rows, no file grouping, no hunk/file jump, no progress, and search disabled — so ↓ is the only navigation. | 4 | 5, 7 |
| 9 | **The preview never follows the cursor.** [E] `{1}` is the file, so the preview renders the file's diff from line 1 regardless of which row is selected. | 4 | 6 |
| 10 | **The fleet queue answers the wrong question.** Sorted by churn, blind to `@panestate` — it ranks "who typed most", not "who is blocked on me", and puts still-running agents at the top. | 3 | 21 |

Runners-up: the 5000-line cliff with no per-file fallback (3); single-line comments only (3);
no line-range or file-level comments (3); `deliver()` ignoring `@panestate` against the repo's
own research (3); no confirmation screen before send (3); the 2048-byte pointer swap at
~13 comments (2); the wrong popup title (2); no `?` help (2).

---

## 5. How the competition handles the same six moments

| Moment | tmux-cockpit today | revdiff 807★ | tuicr 2,868★ | Copilot / VS Code | Conductor | Claude Code native |
|---|---|---|---|---|---|---|
| **Add a comment** | type in fzf query, `alt-↵` | `a` or **`↵`** on the line; `A` file-level; multi-line; `Ctrl+E` → `$EDITOR` | `c` line, `C` file, `v`/`V` visual range; in-TUI editor, optional vim modal | select range → **"Add Feedback"** | click line → comment | plan-mode: free-text rejection |
| **See your batch** | *nothing* | `@` annotation list; `{`/`}` jump between them | `:summary` — every pending draft, `j`/`k` | comments visible inline in the diff | comment list panel | the plan text itself |
| **Delete a comment** | *nothing* | `d` | (delete/re-add) | delete the thread | delete | n/a |
| **Progress** | *nothing* | `Space` marks file reviewed; `f`/`F` filters | `r` file / `R` hunk, **persisted across sessions**; `✓` on covered commits | resolved comments disappear from the diff | — | — |
| **Send the batch** | `^S` → paste → *find the pane* → `↵` | `q` outputs to stdout (agent's tool result); `Q` discards | `y` / `:clip` copies numbered markdown; `--stdout` pipes | **"Submit Feedback"** button | "add to chat" | approve/reject inline |
| **Safety net** | none — `esc` deletes the store | auto-saves to `~/.config/revdiff/history/` on SIGHUP/SIGTERM | drafts persist per session | drafts persist | persists | — |
| **Modality** | hard-modal `display-popup` | popup **or** `REVDIFF_TMUX_WINDOW=1` server-owned window | **tmux split pane** | side panel | panel | inline |

Two things jump out. **First: the two terminal-native winners both open in a split or a
server-owned window, not a modal popup** — revdiff explicitly offers `REVDIFF_TMUX_WINDOW=1`
because a dropped SSH client tears down a client-owned popup and kills the review mid-flight
[E, landscape doc], and tuicr's documented agent workflow is *"your agent opens tuicr in a
cmux, tmux, Zellij, or Herdr split pane"* [E]. **Second: every one of them binds `↵` (or a
plain letter) to "comment this line".** tmux-cockpit is alone in binding the comment action
to a chord and `↵` to nothing.

---

## 6. Redesigned interaction model

### 6a. States (new, all pane-scoped options — they die with the pane, invariant intact)

| Option | Set by | Read by |
|---|---|---|
| `@cockpit_review_pending` | `deliver()` after a successful paste | status format (badge), `tmux-jump` cycle, fleet queue row |
| `@cockpit_review_base` | popup open (the frozen diff's blob hash) | reload drift detection; "changed since your review" on re-entry |
| `@cockpit_reviewed_files` | `x` in the loop | `●`/`✓` column, progress counter, fleet queue "N files unreviewed" |

### 6b. Keys

**Fleet queue** (unchanged where it works):

| key | action |
|---|---|
| `↵` | review the selected pane |
| `^/`, `^U`/`^D` | preview, scroll |
| `esc` | cancel |

**Review loop** — the one change that matters most is the first row:

| key | action | replaces |
|---|---|---|
| **`↵`** | **attach the typed comment to the row under the cursor** — and with an empty query, do nothing (the no-accept-all property is preserved *exactly*, because Enter now pins a comment, it never approves anything) | `enter:ignore` |
| `alt-↵` | same (kept as an alias so existing muscle memory still works) | — |
| `^E` | open `$EDITOR` on the comment for a multi-line note, then attach | nothing |
| `^X` | delete the comment on the row under the cursor (store rewrite; it is a temp file, not an audit log) | nothing |
| `^L` | full-screen **batch view**: every pinned comment, paged | nothing |
| `alt-/` | toggle search on/off (prompt changes `review>` ⇄ `find>`) so a filename can be typed to jump | nothing |
| `alt-]` / `alt-[` | next / previous **file** | nothing |
| `x` | mark this file reviewed (`✓` on all its rows) | nothing |
| `^/`, `alt-u`/`alt-d` | preview, scroll (unchanged) | — |
| `^S` | **confirm-then-deliver** (see 6c) | bare deliver |
| `esc` | discard — **with a confirmation naming the comment count** | silent nuke |
| `?` | key overlay | nothing |

### 6c. Messages (the three that are wrong today)

1. **Header — stop truncating the important half.** Two lines, short enough to survive 70 cols:
   ```
   ↵ comment   ^E multi-line   ^X delete   ^L batch   x reviewed   ? keys
   review> ▸ 3 comments · file 2/7 · line 41/312
   ```
   The counter line rides as **row 1 of the reload payload with `--header-lines=1`** — fzf
   0.44 has no `transform-header`, but a sticky first input row is re-supplied on every
   `reload()` for free, which is exactly the refresh cadence needed. [I, mechanism verified
   against fzf 0.44 feature set]
2. **Delivery confirmation — replace the 2s flash with a two-step.** `^S` swaps the list for
   a confirm screen: *"Deliver 7 comments to `main:2 · auth-worker` (%12)? ↵ deliver · esc back"*.
   Then, after the paste, set `@cockpit_review_pending` on the target pane so the status bar
   carries a persistent **`REVIEW ↵`** badge and `prefix J` cycles to it. The instruction stops
   being a message that expires and becomes a state Dana can walk to. This is the plugin's own
   attention system, reused — nothing new is invented, and no focus is stolen.
3. **Drift warning — make the silent case loud.** On each `reload()`, compare a cheap hash of
   `git diff` against `@cockpit_review_base`. If it moved, prepend a sticky row:
   *"⚠ the agent edited these files since you opened this review — 3 anchors may have moved; `^R` re-anchor"*
   and suppress `●` marks until she re-anchors. Better still (recommendation 1 below), review
   a **snapshot** so it cannot happen at all.

---

## 7. What requires moving off fzf

**Stays in fzf 0.44.1** (all mechanisms confirmed present in that version):
`↵` rebind · `^E` via `execute($EDITOR …)` · `^X` and `^L` via `execute`/`execute-silent`+`reload` ·
sticky counter via `--header-lines=1` · search toggle via `toggle-search`+`change-prompt` ·
preview-follows-cursor via a 5th field carrying the row's index within the file's diff output and
`--preview-window '+{5}-/2'` · confirm screen (a second, two-row fzf) · `?` overlay via `execute` ·
`@panestate` in the fleet row and its sort · diff freezing · drift detection · `@cockpit_review_pending`
badge · **non-modal launch (a split or server-owned window instead of `display-popup`)** — that is a
launcher change in `cockpit.tmux:242-251`, not an fzf change.

**Cannot be done in fzf, at any version:**
- Side-by-side and word-level intra-line diff.
- Collapsible files/hunks and true diff-with-context rendering (fzf's list is flat rows; the
  preview can render context, but the *list* cannot be a diff).
- Visual-mode line ranges (`v`/`V`) — fzf's `--multi` selects rows but has no contiguous-range
  concept, and `--multi` collides with `--disabled`-as-comment-box.
- A comment gutter rendered *inside* the diff.
- Blame gutter, syntax-aware navigation.

**Verdict [I]:** everything in the top-10 defect list is fixable **without leaving fzf**. What
fzf cannot give is the *rendering* half — the part where revdiff and tuicr are genuinely
better and already exist. The honest split is the one the landscape doc already reached: keep
fzf for the **fleet queue** and **delivery** (the two things only a tmux plugin can do), and
treat a real diff TUI as an optional `@cockpit_review_tui` backend for the annotation step —
shelling out to `revdiff`/`tuicr` when present and falling back to the fzf loop when not.

---

## 8. Ranked recommendations

| # | What | Why (UX) | Cost | Replaces |
|---|---|---|---|---|
| **1** | **Freeze the diff at popup open.** Snapshot `git diff` to a temp file; `--lines`/`--rows` read the snapshot, never the live tree. On `^S`, re-diff and, if it moved, show *"3 of these lines changed since you opened this review"* before delivering. | Kills defect #1 outright, and it is the only defect that makes the tool deliver a **wrong** result rather than a slow one. Also removes the per-reload `git diff` entirely. | **S** | live `git -C "$dir" diff` in `lines_mode` (:92-98) |
| **2** | **Bind `↵` to attach.** Keep `alt-↵` as an alias. Empty query = no-op, so "no accept-all" is preserved exactly. | Removes the single largest mode error, matches every competitor and every other picker in this plugin, and makes the truncating header far less load-bearing. | **S** | `--bind 'enter:ignore'` (:401) |
| **3** | **`^X` delete, `^L` batch view, and a confirm on `esc`.** Store rewrite for `^X`; `execute(pager)` for `^L`; a two-row fzf confirm naming the count for `esc`. | Restores undo, visibility, and error recovery — three Nielsen heuristics currently at zero. Closes the largest capability gap against revdiff (`d`, `@`) and tuicr (`:summary`). | **M** | append-only-with-no-reader store (:319-343); silent `rm -f` on cancel (:414-417) |
| **4** | **Close the fleet loop.** After delivery, return to the fleet queue instead of closing; add `^F` inside the review loop to jump back to the queue; make `prefix r` from an agent pane offer *"review this pane / open the fleet queue"* rather than deciding for her. | Turns 6 separate `prefix r` cycles into one session, and removes the dead end at stage 20 — the defect that most directly caps how many agents Dana can actually gate. | **M** | `main_entry`'s implicit branch (:705-720); `review_pane`'s terminal exit (:630-645) |
| **5** | **Make delivery a state, not a flash.** Confirm screen before the paste; `@cockpit_review_pending` on the target pane afterwards; a status badge; `tmux-jump` cycles review-pending panes as well as `needs-input`. | Replaces a 2000 ms message naming `%12` with a persistent, walkable signal, using machinery the plugin already has. Also gives the "typed-but-unsent" design the visibility it needs to be trustworthy. | **M** | the lone `display-message` at :560; `tmux-jump:15-16`'s `needs-input`-only filter |
| **6** | **Fix orientation: preview follows the cursor, sticky counter, file jump, search toggle.** Emit a 5th field (row index within the file's diff) and use `--preview-window '+{5}-/2'`; `--header-lines=1` counter; `alt-[`/`alt-]`; `alt-/`. | Converts the 8-row porthole from unusable to navigable without changing the row model or leaving fzf. Defects #8 and #9 in one pass. | **M** | `--preview` at :364; `--info=inline-right` as the only progress signal (:400) |
| **7** | **Order the fleet by urgency, not volume.** Add `@panestate` to the `list-panes -F` format (it is already one `#{}` away) and sort `needs-input` → `done` → `error` → `running`, churn as the tiebreak. Show the state and the branch on the row. | The queue's whole job is "who needs me first"; churn answers "who typed most", and puts still-moving diffs at the top. One format string. | **S** | the `sort -k1,1nr` on churn alone (:615-620) |
| **8** | **Guard delivery on `@panestate`.** Add the state check to the `if-shell -F` re-test that already runs at paste time — it costs one more `#{}` in a format string already being evaluated. | Restores the guard the repo's own research specified and the implementation dropped; stops a review landing mid-tool-call. | **S** | `deliver()`'s three guards (:471-494) |
| **9** | **Make the review non-modal:** launch into a split pane or a server-owned window (`@cockpit_review_window`), keeping the popup as the option rather than the only mode. | **[E]** Prefix keys are swallowed by a popup, so a fleet operator is blind to five agents while reviewing one. It is also what both terminal-native competitors do, and it survives a dropped SSH client. | **M** | `display-popup -E` in `cockpit.tmux:242-251` |
| **10** | **Add follow-up state.** Persist the delivered store under the pane (or its digest) instead of `rm -f`; on re-entry, show `↺` on rows that carried a comment last round and *"7 comments delivered 12m ago; 4 of those lines have changed since."* | Makes the re-review loop — the actual reason a human gates an agent — a first-class flow instead of an unaided memory exercise. | **L** | `rm -f -- "$store"` after delivery (:643) |
| **11** | **Multi-line comments via `^E` → `$EDITOR`**, plus file-level comments (`C`) using revdiff's `## path (file-level)` header, which the grammar already reserves. | "Make annotate as cheap as accept" is the approval-fatigue rule; a one-line box is what makes the thorough comment more expensive than the shallow one. Note revdiff's hard-won detail: popup children inherit a **server-era environment**, so `EDITOR`/`VISUAL` must be re-injected via `display-popup -e KEY=VAL`. | **M** | the single-line fzf query as the only input (:396-407) |
| **12** | **Replace the 5000-line cliff with a file-level fallback.** Over the cap, show the **file list** (churn-ordered, test-first) with file-level comments and a "drill into this file" key, instead of one dead row. | The biggest diffs are the ones most in need of triage; today they get the least. Also makes the cap a graceful degradation rather than a refusal. | **M** | the single "(diff too large)" row (:119-123) |
| **13** | **Two-line header + `?` overlay.** | **[E]** At the default 70% width the header truncates away `esc discard` and the `↵` explanation. Free to fix. | **S** | the 95-char single-line header (:407) |
| **14** | **Optional TUI backend.** `@cockpit_review_tui = revdiff|tuicr|fzf`; shell out when the binary exists, keeping the fleet queue and the delivery path as cockpit's own. | Buys side-by-side, word-diff, hunk collapse, ranges, blame, and persisted review state for the cost of an exec — none of which fzf can ever provide — while keeping the two things only this plugin has. | **L** | the fzf review loop as the *only* annotation surface |
