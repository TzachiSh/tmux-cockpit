# Lens: UI — what the reviewer looks like and how the diff is presented

Method: I built a throwaway git repo with a realistic multi-file agent change (source
file with 2 hunks + a 200-char line, a test file, a whole-file deletion, a rename+edit, a
non-ASCII path `src/café/naïve-ünicode.js`, a doc change, an untracked file), then ran
`scripts/tmux-review --loop` inside a private tmux server (`tmux -L lensui -f /dev/null`)
at several geometries and captured the screen with `capture-pane -p` / `-p -e`. Every
capture block below is verbatim tool output, not a reconstruction. Server killed at the
end. Nothing in the repo was modified.

Fixtures: `$SCRATCH/fixture` (18 rows), `$SCRATCH/f-real` (20 files / 110 rows),
`$SCRATCH/f-4k` (4800 rows), `$SCRATCH/f-big` (12000 rows, over the cap),
`$SCRATCH/f-api-gateway` + `$SCRATCH/f-ui-dashboard` (fleet queue).
Stack: fzf 0.44.1 (debian), tmux 3.7c, git 2.43.0, TERM=tmux-256color.

---

## 1. The screen you actually get

Real default geometry. `cockpit.tmux:201-202` sets `PW=70% PH=70%`, and
`cockpit.tmux:249` opens the review as `display-popup -E -w 70% -h 70%`. On a 160x45
terminal the popup interior is ~110x29. Here it is: [E]

```
     1	review>                                                                                                 18/18
     2	  alt-↵ comment   ^/ preview   alt-u/alt-d scroll   ^S deliver   esc discard (enter does nothing)
     3	>                                                                                                            │
     4	   tests/server.test.js:7 (+)  test('rejects missing auth', () => {                                          │
     5	   tests/server.test.js:8 (+)    expect(handle).toBeDefined();                                               │
     6	   tests/server.test.js:9 (+)  });                                                                           │
     7	   src/server.js:2 (+)  const { verify } = require('./auth');                                                │
     8	   src/server.js:6 (+)    if (!verify(req.headers.authorization)) {
     9	   src/server.js:7 (+)      res.writeHead(401);
    10	   src/server.js:8 (+)      res.end('unauthorized');
    11	   src/server.js:9 (+)      return;
    12	   src/server.js:10 (+)    }
    13	──────────────────────────────────────────────────────────────────────────────────────────────────────────────
    14	 tests/server.test.js | 4 ++++
    15	@@ -3,3 +3,7 @@ const { handle } = require('../src/server');
    16	 test('health', () => {
    17	   expect(1).toBe(1);
    18	 });
    19	+
    20	+test('rejects missing auth', () => {
    21	+  expect(handle).toBeDefined();
    22	+});
    23	
    24	
    25	   ... 6 more blank rows ...
    29
```

Read what that says. On a 29-row screen: 10 rows of list (8 of them usable), 16 rows of
preview of which **7 are blank**. 8 of the change's 18 lines are below the fold on a
*trivial* 15-line change. The cursor's opening position, row 3, is **entirely blank**. [E]

The same run on a full 160x45 pane is worse in proportion: 10 list rows, 32 preview rows,
**23 of them blank** — over half the screen is dead space. [E]

## 2. Concrete UI defects observed

### D1 — Blank rows: every empty added/removed line renders as an invisible row [E]

Row 3 above is `tests/server.test.js:6 (+)` — a blank added line. It renders as nothing.
Two of the fixture's 18 rows (11%) are invisible. Not a rendering quirk: a real,
reproducible field-shift bug.

`rows_mode` (`scripts/tmux-review:301`) reads its input with
`while IFS=$'\t' read -r path line fragment display`. Tab is an **IFS whitespace**
character, so bash collapses a run of tabs into one delimiter. An empty fragment field
therefore shifts everything left:

```
$ printf 'p\t3\t\tDISPLAYTEXT\n' | while IFS=$'\t' read -r a b c d; do ... done
a=[p] b=[3] c=[DISPLAYTEXT] d=[]
```

And so `--rows` emits, for the blank line in `docs/notes.md`:

```
docs/notes.md^I3^Idocs/notes.md:3 (+)  ^I           <- field 3 = the DISPLAY, field 4 = the marker alone
docs/notes.md^I4^IA second line.^I docs/notes.md:4 (+)  A second line.
```

Two visible consequences, both captured:

1. `--with-nth=4` renders only the marker, so the row is a blank line (or `●` after you
   comment on it) — you cannot see which file or line it is:
   ```
     ●src/server.js:22 (+)    server.listen(port, () => console.log('a very long ...
   > ●
     docs/notes.md:4 (+)  A second line.
   ```
2. `{3}` in the `--attach` bind (`scripts/tmux-review:364`) is now the display string, so
   the store's quoted fragment is garbage — verified in the live store file:
   ```
   ## docs/notes.md:3 (+)
   > docs/notes.md:3 (+)
   blank line comment
   ```
   The "anchored to the line's own quoted text, not a bare number" invariant
   (README.md:518-519) is broken for every blank line in every diff.

### D2 — The "diff too large" safety valve renders as an empty popup [E]

Same root cause, worse blast radius. `lines_mode:122` emits the notice as
`'(diff too large)'\t''\t"$big"\t"$big"` — an empty **second** field. On a 12000-line
diff:

```
     1	review>                                                                                                   1/1
     2	  alt-↵ comment   ^/ preview   alt-u/alt-d scroll   ^S deliver   esc discard (enter does nothing)
     3	>
     4
     5
     6	──────────────────────────────────────────────────────────────────────────────────────────────────────────────
     7	gone: (diff too large)
```

The one state that most needs to explain itself — "12000 changed lines, review this
directly" — shows the user a blank row and `gone: (diff too large)`. The carefully
written message at `lines_mode:121` never reaches the screen. [E]

### D3 — The preview never follows the cursor [E]

Cursor on `src/server.js:22`, the file's **second** hunk. Preview shows `@@ -1,7 +1,13 @@`,
the first:

```
    9	   src/server.js:16 (-)    server.listen(port);
   10	>  src/server.js:22 (+)    server.listen(port, () => console.log('a very long line ... one-hundr..
   11
   12	   docs/notes.md:4 (+)  A second line.
   13	──────────────────────────────────────────────────────────────────────────────────────────────────────────────
   14	 src/server.js | 8 +++++++-
   15	@@ -1,7 +1,13 @@
   16	 const http = require('http');
   17	+const { verify } = require('./auth');
   ...
```

`preview_cmd` is `cd <dir> && tmux-open-path --preview-diff {1}` (`:365`) — keyed on the
**path** field only. Every row of a file gets the identical preview, always starting at
hunk 1. On a file with 6 hunks you must `alt-d` manually to find the line you are already
standing on. The list gives you no context and the preview will not take you to it, so the
review has **no working context view at all** — the single thing every reviewer needs.
This is the defect I would fix first, and it is the cheapest one on this list (see R1).

### D4 — The list is monochrome; the preview 12 rows below it is in color [E]

`capture-pane -p -e` on the list rows: nothing but `^[[39m` (default fg) and the
selection's `^[[40m`. No color at all.

```
^[[39m^[[40m ^[[49m  tests/server.test.js:7 (+)  test('rejects missing auth', () => {
^[[40m ^[[49m  src/server.js:7 (+)      res.writeHead(401);
```

The preview, on the same screen:

```
^[[36m@@ -3,3 +3,7 @@^[[39m const { handle } = require('../src/server');
^[[32m+^[[39m
^[[32m+test('rejects missing auth', () => {^[[39m
```

Add and delete are *green and red twelve rows down and monochrome twelve rows up*. In the
list you must read the `(+)` / `(-)` token to know which. `--ansi` is not passed and no
row carries SGR. Every tool in the comparison set (delta, difftastic, tig, lazygit,
gitui, revdiff, hunk) treats add/delete color as the primary channel; this makes it the
*only* channel that is missing. [E]

### D5 — 22-33 columns per row spent restating the filename [E]

`lines_mode:242` builds `disp` as `path:line (sign)  fragment`. On a 110-col popup:

| row | chrome before the code | % of width |
|---|---|---|
| `src/server.js:22 (+)  ` | 22 cols | 20% |
| `tests/server.test.js:7 (+)  ` | 28 cols | 25% |
| `src/café/naïve-ünicode.js:1 (-)  ` | 33 cols | 30% |

repeated on every row of the file, when the row above already said it. No file header
row, no separator, no indentation, no tree — nothing groups a file's rows. GitHub's
"Files changed", lazygit's staging view, tig, revdiff's two-pane tree and hunk's sidebar
all hoist the path to a header exactly once. [I: the mechanism is [E]; the comparison is
the inference]

### D6 — Old and new line numbers share one column with no visual separation [E]

```
   src/server.js:9 (+)      return;
   src/server.js:10 (+)    }
   src/server.js:16 (-)    server.listen(port);
   src/server.js:22 (+)    server.listen(port, () => console.log(...
```

10 → 16 → 22 reads as a monotonic walk down one file. It is not: 10 and 22 are **new**-file
line numbers, 16 is an **old**-file one (`lines_mode:71-72` documents the choice). The two
coordinate systems are distinguished only by the `(-)`, which is monochrome (D4). A
modified line then appears as two unrelated adjacent rows with the *same* number:

```
   src/café/naïve-ünicode.js:1 (-)  module.exports = { accent: 'é' };
   src/café/naïve-ünicode.js:1 (+)  module.exports = { accent: 'é', extra: 'ü' };
   src/newname.js:1 (-)  module.exports = { renamedLater: true };
   src/newname.js:1 (+)  module.exports = { renamedLater: true, andEdited: 1 };
```

Comment on the `(-)` row and you have anchored a review comment to a line that no longer
exists in the file the agent will edit. Nothing on screen warns you. Every side-by-side
and unified renderer in the field pairs these two lines instead (revdiff even ships
intra-line word-diff for exactly this pair). [E for the render; I for the consequence]

### D7 — The comment box is at the top of the screen, the row it lands on is 8 rows below [E]

```
     1	review> this console.log will spam prod, use the logger                                                   18/18
     2	  alt-↵ comment   ^/ preview   alt-u/alt-d scroll   ^S deliver   esc discard (enter does nothing)
     3	   src/server.js:2 (+)  const { verify } = require('./auth');
   ...
    10	>  src/server.js:22 (+)    server.listen(port, () => console.log('a very long line ...
```

The typed text — which *is* the comment — sits on line 1 behind a prompt that reads
`review>`, nine rows away from the highlighted row it will attach to, with nothing
connecting them. The prompt does not name the target. The counter in the corner says
`18/18` and, because `--disabled` is set (`:399`), **can never say anything else** — a
permanent piece of filter chrome in a picker that does not filter.

That "the query line is a comment box" is the one thing a first-time user cannot guess,
and the UI's only hint is the word `comment` inside a 90-char run-on header line
(`:408`) which is itself truncated at any width under ~100 cols:

```
  alt-↵ comment   ^/ preview   alt-u/alt-d scroll   ^S deliver   esc discard (enter doe..     <- 90 cols
  alt-↵ comment   ^/ preview   alt-u/alt-d scroll   ^S ..                                     <- 58 cols
```

At 90 columns — a perfectly normal split pane — `esc discard` is off-screen. fzf 0.44
accepts a multi-line `--header` (verified), so this is a self-inflicted wound.

### D8 — A comment, once attached, is invisible, uneditable, and undeletable [E]

Three comments attached to the same row in sequence. Screen after all three:

```
    10	> ●src/server.js:22 (+)    server.listen(port, () => console.log('a very long line ...
```

One dot. Identical to one comment. Store after all three:

```
## src/server.js:22 (+)
> server.listen(port, () => console.log('a very long line ... column terminal
this console.log will spam prod, use the logger

## src/server.js:22 (+)
> server.listen(port, () => console.log('a very long line ... column terminal
?? why 401 and not 403

## src/server.js:22 (+)
> server.listen(port, () => console.log('a very long line ... column terminal
actually ignore that
```

The third comment was a retraction. It is delivered alongside the thing it retracts, and
the reviewer has no way to know: no comment count anywhere on screen, no list of pending
comments, no way to re-read what you wrote, no edit, no delete, no undo. `attach()` is
append-only by design (`:312-313`) — a sound *durability* decision that was allowed to
become the *interaction* model. revdiff has `d` (delete annotation under cursor) and `@`
(annotation list popup, jump to any of them); GitHub lets you edit a pending review
comment; lazygit lets you unstage. This has none of it. [E]

Also: `●` is glued to the path with no separating space (`rows_mode:309` prints
`marker` immediately followed by `display`), so it reads as part of the filename —
`●src/server.js:22`.

### D9 — A long line is truncated twice, and the truncated text is what gets quoted to the agent [E]

`lines_mode:240` hard-cuts the fragment at 160 chars; fzf then ellipsizes at the pane
width. The row:

```
>  src/server.js:22 (+)    server.listen(port, () => console.log('a very long line that goes on and on and should certainly overflow the width of a one-hundr..
```

The store:

```
> server.listen(port, () => console.log('a very long line that goes on and on and should certainly overflow the width of a one-hundred-and-sixty column terminal
```

The quote stops mid-sentence at exactly 160 bytes with no ellipsis and no closing quote.
The agent is handed a `>` fragment that **does not match any line in its file**, which is
the precise failure the quoted-fragment design exists to prevent (README.md:545-548). No
horizontal scroll, no wrap toggle (`--wrap` is fzf 0.50+, not available at 0.44.1), no
`«»` overflow markers (revdiff has both). [E]

### D10 — Whole categories of agent change are simply absent from the list [E]

`git status --short` on the fixture:

```
 M docs/notes.md
 M "src/café/naïve-ünicode.js"
D  src/legacy-shim.js          <- staged deletion
RM src/oldname.js -> src/newname.js
 M src/server.js
 M tests/server.test.js
?? src/untracked-new.js        <- new file
```

What the review list shows (18 rows): `tests/server.test.js`, `src/server.js`,
`docs/notes.md`, `src/café/naïve-ünicode.js`, `src/newname.js`. Missing:

- **`src/legacy-shim.js`, deleted.** `lines_mode:92-93` is unstaged-first with staged as a
  *fallback*, not a union. Any unstaged change at all makes the entire index invisible. A
  deleted file is unreviewable.
- **`src/untracked-new.js`, brand new.** `git diff` never reports untracked files. **The
  single most common thing an agent produces — a new file — cannot be reviewed at all.**
  The fleet queue counts it (`?1 new`) and then sends you into a review that cannot show
  it.
- **The rename.** `src/newname.js` appears as a plain 1-line modification with no
  indication it used to be `src/oldname.js`. (revdiff lists rename-awareness as a feature:
  `old → new` in the diff header.)

Every one of these is a *presentation* gap as much as a data gap: the popup gives no hint
that anything is hidden. It looks complete.

### D11 — The preview shows octal-escaped non-ASCII paths (a fixed bug, still live one file over) [E]

Fleet-queue preview on the fixture pane:

```
    10	 M docs/notes.md
    11	 M "src/caf\303\251/na\303\257ve-\303\274nicode.js"
   ...
    18	 "src/caf\303\251/na\303\257ve-\303\274nicode.js" | 2 +-
```

`tmux-review`'s `lines_mode` sets `-c core.quotePath=false` and documents why at length
(`:81-91`). `tmux-open-path:69-70` and `:81`, which render the preview *on the same
screen*, do not. The user sees the fix on the left and the bug on the right.

### D12 — Navigation: 8 visible rows, no file jump, no position indicator [E]

A 20-file / 110-row change — an entirely ordinary agent output:

```
     1	review>                                                                                               110/110
     2	  alt-↵ comment   ^/ preview   alt-u/alt-d scroll   ^S deliver   esc discard (enter does nothing)
     3	>  tests/mod1.test.ts:4 (-)  test1 line 4
     4	   tests/mod1.test.ts:4 (+)  T-CHANGED-1
   ...
    12	   tests/mod5.test.ts:4 (+)  T-CHANGED-5
    13	──────────────────────────────────────────────────────────────────────────────────────────────────────────────
```

8 of 110 rows visible (7%). `lrows` is capped at 8 unconditionally (`run_loop:382`)
**regardless of how many rows exist**, so the list never grows and the preview takes the
rest whether it needs it or not.

PageDown works (fzf default) but is not in the header. There is no next-file / prev-file
key — `[`/`]` in revdiff, `n`/`p` for next changed file, `}`/`{` for next annotation.
Reaching `src/mod14.ts` means ~100 Down presses or 12 undocumented PageDowns, landing
mid-file with nothing on screen saying which file or where in the review you are.
`110/110` is a match counter, not a position.

Cursor position *is* preserved across the post-comment `reload()` — verified 10 PageDowns
deep, the `●` appeared on the row under the cursor and the viewport did not move. That
part works. [E]

### D13 — The split is a fixed row count computed once, so any resize collapses the list [E]

`run_loop:381-384`: `avail=$(tput lines)`, `psz=$(( avail - lrows - 5 ))`. `psz` is passed
to fzf as an absolute row count and never recomputed. Started at 160x45 (psz=32), then
resized:

```
=== 80x20 ===
     1	review>                                                                   18/18
     2	  alt-↵ comment   ^/ preview   alt-u/alt-d scroll   ^S deliver   esc discard ..
     3	> ●                                                                            │
     4	────────────────────────────────────────────────────────────────────────────────
     5	 docs/notes.md | 2 ++
   ...
    11	     ... 10 blank rows ...
```

**One list row.** And it is a blank one (D1). At 58x14, still one. fzf honours the
32-row preview request and starves the list to nothing. A percentage
(`down,60%,border-top`) would survive this; a fixed count cannot. tmux popups do not
resize with the client, but a `SIGWINCH` from the terminal, a smaller-than-expected popup,
or `@cockpit_popup_height` set low all reproduce it.

For the record: preview-off gives the whole 18 rows and reads far better —

```
     3
     4	   tests/server.test.js:7 (+)  test('rejects missing auth', () => {
   ...
    17	   src/café/naïve-ünicode.js:1 (-)  module.exports = { accent: 'é' };
    18	   src/café/naïve-ünicode.js:1 (+)  module.exports = { accent: 'é', extra: 'ü' };
    19	   src/newname.js:1 (-)  module.exports = { renamedLater: true };
    20	   src/newname.js:1 (+)  module.exports = { renamedLater: true, andEdited: 1 };
```

— which is the diagnosis, not the cure: the list is starved *and* the preview is useless
(D3), so turning the preview off is currently the better experience.

### D14 — Fleet queue: the one identifier you need is the one that is missing [E]

Four dirty agent panes, two of them in the *same* repo:

```
     1	fleet>                                                                                                    4/4
     2	  which agent needs you   ↵ review it   ^/ preview   esc cancel
     3	> 0:0 bash        +15 -3  ?1 new
     4	  0:3 bash        +15 -3  ?1 new
     5	  0:1 bash        +2 -1  ?1 new
     6	  0:2 bash        +1 -1
     7
     8
     9	──────────────────────────────────────────────────────────────────────────────────────────────────────────────
```

Rows 3 and 4 are byte-identical and are different repos' worth of work in different
windows. **No repo, no directory, no branch appears on any row** — it is in the preview,
one keystroke away, for one row at a time. `fleet_rows:582` builds the title from
`session:window` plus `pane_title`, falling back to `window_name`; the project is never
in it.

Column alignment is accidental tab-stop rendering, and shatters on real window names:

```
> 0:0 bash        +15 -3  ?1 new
  0:1 refactor-the-auth-middleware        +2 -1  ?1 new
  0:2 ui  +1 -1
```

The churn column — the *only* thing this list sorts by and the reason to scan it — is at a
different x on every row. Also: no column legend, no branch, no last-activity/staleness,
and no agent state, even though the cockpit already computes idle/working/waiting
elsewhere. "Which agent needs you" is answered purely by line count, so an agent that has
been sitting idle waiting on you for 20 minutes with a 3-line fix sorts below a
still-typing one with a big refactor. [E for the render; I for the ranking critique]

### D15 — `esc` in the review exits the popup entirely instead of returning to the queue [E, from code]

`fleet_queue` → `review_pane` → `"$SELF" --loop` (`review_pane:636`), and `run_loop` ends
in a hard `exit 0` / `exit 20` (`:416,424,429`). Nothing returns. Reviewing three agents
in a row means `prefix r` three times, re-navigating the queue each time. The queue is
built for a fleet workflow and structurally cannot loop.

### D16 — The one instruction the human must obey is shown for 750 ms [E]

Delivery's only feedback is `display-message 'review pasted into %N — press Enter there to
send'` (`deliver:561`), fired from `tmux run-shell -b` *after* the modal popup has torn
down. `display-time` default is 750 (verified: `show-options -g display-time` → `750`).
The sentence telling the human that the review is sitting unsent in a pane, and that they
must go press Enter, flashes for under a second on the status line of a screen that just
changed. Similarly, the oversized-review path replaces the whole review with `Read and
address every comment in /run/user/1000/tmux-cockpit-review-3-XXXXXX` (`:526`) and the
reviewer never sees a confirmation of what was in it.

### D17 — Performance is *not* the presentation bottleneck [E]

Measured, so the redesign is not misdirected:

| operation | 18 rows | 110 rows | 4800 rows |
|---|---|---|---|
| `--rows` (startup **and** every comment) | 0.054 s | 0.070 s | 0.412 s |

Per-preview render: 25 ms (file diff), 31 ms (fleet dir status+stat). The
`execute-silent+reload` full rebuild after every comment costs 70 ms at realistic sizes —
imperceptible. The 5000-row cap and the O(n²)-to-O(n) work already recorded in the
comments did their job. **Everything above is layout and legibility, not speed.** The one
speed item worth keeping in view is the 0.4 s per-comment reload near the cap, which is
felt but not broken.

---

## 3. Redesigned layout

### 3a. The review list — achievable with fzf 0.44.1 + tmux popups

Verified available at 0.44.1 (probed live, `load:abort` accepted ⇒ option/action parses):
`--preview-window` field offset `+{n}-/2`, `focus` event, `change-preview-label`,
multi-line `--header`, `--header-first`, `--marker`, `--pointer`, `--border-label`,
`--ansi`, `--ellipsis`, `toggle-preview-wrap`, `put()`, `become()`.
Verified **not** available at 0.44.1: `transform()`, `--list-border`, `--gap`, `--footer`,
`--tmux`, `--wrap`, `--highlight-line`, `search()`. Placeholder substitution inside
`change-prompt(...)` does **not** work at 0.44.1 — it printed the literal `{2}:{3}`
(needs `transform-prompt`, fzf 0.45+).

```
┌ review · api-gateway  main  6 files  +15 −3  ?1 new ─────────────── 3 comments ─┐
│ ▸▸ tests/server.test.js                                       +4 −0    2 hunks  │   file header row,
│  ● │  7 │ + │ test('rejects missing auth', () => {                              │   path exactly once
│    │  8 │ + │   expect(handle).toBeDefined();                                   │
│    │  9 │ + │ });                                                               │
│ ▸▸ src/server.js                                              +7 −1    2 hunks  │
│    │  2 │ + │ const { verify } = require('./auth');                             │
│    │  6 │ + │ if (!verify(req.headers.authorization)) {                          │
│  ●2│ 22 │ + │ server.listen(port, () => console.log('a very long line th… »      │   ●2 = 2 comments
│    │ 16 │ − │ server.listen(port);                                              │   − rows dimmed+red
│ ▸▸ src/legacy-shim.js                                    deleted  −2   staged   │   ← D10 now visible
│ ▸▸ src/untracked-new.js                                       new  +1           │   ← D10 now visible
│ ▸▸ src/oldname.js → src/newname.js                        renamed  +1 −1        │   ← D10 now visible
├─ src/server.js  hunk 2/2 ───────────────────────────── line 22 ── 78/140 ───────┤
│  20 │   function start(port) {                                                  │   preview CENTERED
│  21 │     const server = http.createServer(handle);                             │   on the cursor row
│  22 │ + │ server.listen(port, () => console.log('a very long line that goes …   │   ◀ cursor line
│  23 │     return server;                                                        │
│  24 │   }                                                                       │
├─────────────────────────────────────────────────────────────────────────────────┤
│ comment on src/server.js:22 (+) ▸ this console.log will spam prod_              │   comment box, LABELLED,
├─────────────────────────────────────────────────────────────────────────────────┤   adjacent to the target
│ alt-↵ attach   ^E editor (multi-line)   alt-x delete comment   @ list comments  │
│ [ ] hunk   n/p file   ^/ preview   ^S deliver 3   esc discard                   │
└─────────────────────────────────────────────────────────────────────────────────┘
```

What each element fixes, and how it is built on fzf 0.44.1:

- **Aligned columns** — pad in bash with `printf '%-*s'`, never raw tabs. Fixes the
  fleet queue's shattered alignment (D14) and gives the list a scannable number gutter
  (D6).
- **Marker / line / sign / code as four gutters**, sign rendered as a **colored** `+`/`−`
  via `--ansi` with SGR emitted by `lines_mode`; `−` rows additionally dimmed and their
  number right-aligned in a separate old-line column. Fixes D4, D6, D5.
- **File header rows** (`▸▸ path  +a −d  n hunks`) emitted by `lines_mode`, bound to a
  no-op on `alt-enter` so you cannot comment on a header — or better, attach a
  **file-level comment** there, which revdiff's grammar already supports
  (`## path (file-level)`) and this store already almost writes. Fixes D5, and closes
  a grammar gap versus the format the plugin borrowed.
- **Preview centred on the cursor line.** This is the big one and it is a one-line change
  plus one extra field. Proven live at 0.44.1 — a row carrying offset `90` in field 2 with
  `--preview-window="down,12,border-top,+{2}-/2"`:
  ```
    17	──────────────────────────────────────────────────────────────────────────────
    18	84                                                                     84/100
   ...  (90 centred)
    24	90
   ...
    29	95
  ```
  `lines_mode` already walks the diff line by line, so emitting each row's 1-based offset
  within its file's own `git diff` output is free. Fixes D3 outright.
- **Percentage preview split with a floor**, `down,55%,border-top`, and `lrows` allowed to
  grow past 8 (say `min(nrows, avail/2)`). Resize-safe. Fixes D13 and the 7-blank-rows
  waste.
- **`--header-first` + multi-line `--header`**, two short lines instead of one 90-char
  line. Never truncates at 80 cols. Fixes D7's truncation.
- **`^E` → `become()`/`execute()` into `$EDITOR`** on a temp file, then attach its
  contents. That is the only way to write a multi-line comment through an fzf query
  (single-line by construction), and it is exactly revdiff's `Ctrl+E` during annotation
  input. Cost is one `execute()` bind.
- **`●2` count in the marker column** — `rows_mode` already reads every `## ` header into
  an associative array; count instead of test. One-character change, removes the "one dot
  means one to N comments" lie (D8).
- **`@` opens a nested fzf over the store**, `alt-x` deletes the comment under the cursor
  (rewrite the store minus that block, then `reload`). Append-only durability is
  preserved by writing to a `.new` and `mv`-ing. Fixes D8 — the biggest gap versus
  revdiff, GitHub, and every other reviewer in the set.
- **`[` / `]` hunk jump, `n` / `p` file jump** — fzf has no "jump to next row matching a
  predicate", but with file-header rows in the list the same effect is available by
  emitting a hidden field marking header rows and binding `n`/`p` to
  `reload(...)+first`-style tricks, or more simply by making the list *two* modes: a
  file-level list (`n` rows) that `enter`s into a per-file line list. That second shape is
  strictly simpler in fzf and matches the GitHub "Files changed" mental model. Fixes D12.
- **`«` / `»` overflow markers + `--ellipsis '…'`** and raising the 160-char fragment cap
  to something width-derived, with the **untruncated** line written to the store. Fixes D9
  and restores the quoted-anchor invariant.
- **Header bar carries repo, branch, file count, `+/−`, and a live comment count**, and
  `^S deliver 3` names how many. Fixes D7's missing target/state and the `18/18` dead
  counter.
- **`--prompt` naming the target** (`comment on src/server.js:22 (+) ▸`) needs
  `transform-prompt`, fzf **0.45+**. On 0.44.1 the fallback is a static
  `comment ▸` prompt plus `change-preview-label( src/server.js:22 (+) )` on `focus`,
  which *is* available (probed) and puts the target on the border between the box and the
  preview — good enough, one row away instead of nine. This is the one redesign element
  that is version-gated; everything else above runs on 0.44.1.

### 3b. The fleet queue

```
┌ fleet · 4 agents need you ──────────────────────────────────────────────────────┐
│   pane  project          branch            churn      state        idle          │
│ ▸ %1    api-gateway      feat/auth        +15 −3  ?1  ● waiting    12m           │
│   %3    api-gateway      feat/auth        +15 −3  ?1  ◐ working     0m           │
│   %0    ui-dashboard     main              +2 −1  ?1  ● waiting     4m           │
│   %2    docs-site        chore/copy        +1 −1      ◌ idle        41m           │
├─ ui-dashboard · main ───────────────────────────────────────────────────────────┤
│  M docs/notes.md                                                                │
│  M src/café/naïve-ünicode.js                          ← quotePath=false          │
│ ?? src/untracked-new.js                                                          │
├─────────────────────────────────────────────────────────────────────────────────┤
│ ↵ review   ^/ preview   s sort: churn│idle│state   esc cancel                    │
└─────────────────────────────────────────────────────────────────────────────────┘
```

- `basename` of the repo root as the **project** column — the missing identifier (D14).
  It costs one `git -C rev-parse --show-toplevel` per dirty pane, in a loop that already
  runs two `git` calls per pane.
- **Branch** column, one `--abbrev-ref HEAD` per pane, same loop.
- **`printf '%-*s'` padding**, not tabs (D14).
- A one-line **column legend** as row 1 of a multi-line `--header`.
- **Agent state and idle time** as sort keys alongside churn, so "which agent needs you"
  means *waiting for you*, not *has typed the most*. The plugin already computes agent
  state (`agent-state`, and the freeze-precompute options noted in project memory) — this
  queue is the natural consumer.
- **`core.quotePath=false`** on the preview's `status --short` / `--stat`
  (`tmux-open-path:69-70,81`). D11, two flags.

### 3c. What genuinely needs a real TUI (i.e. shell out to revdiff or hunk)

fzf's list is one row = one item, no wrapping, no per-row multi-line rendering, no
independent horizontal scroll, no intra-line highlighting. These cannot be built on it at
any effort:

- **Side-by-side / split view**, and **intra-line word-diff** on a paired ±line — D6's
  real cure rather than its mitigation.
- **Context lines shown inline in the list** (hunk navigation with surrounding context,
  the lazygit / tig / delta model) rather than a list-of-changed-lines plus a separate
  preview.
- **Word wrap with `↪` continuations** for long lines, and horizontal scroll — the real
  cure for D9.
- **Multi-line annotation edited in place**, annotation range selection (`:43-67 (+)`,
  which revdiff's grammar has and this store cannot express), syntax highlighting,
  blame gutter.
- **A file tree pane** alongside the diff.

The relevant fact for the redesign decision: **`umputun/revdiff` (807 stars, pushed
2026-08-20) already does all of the above, already ships a tmux `display-popup` launcher,
already ships a Claude Code plugin, and its README describes its purpose as "reviewing
code changes… without leaving a terminal-based AI coding session (e.g. Claude Code)",
outputting annotations to stdout to be piped into agents.** [E — `gh api`, README fetched]
The `## path:line (+)` grammar this plugin writes is revdiff's own output format, copied —
minus revdiff's `(file-level)` and `:43-67` range forms, plus the plugin's own
`> fragment` line, which revdiff does not emit. `modem-dev/hunk` (8711 stars, pushed
2026-08-24, "Review-first terminal diff viewer for agentic coders") is the other live
option, with a sidebar, split/stack/responsive layouts, and inline agent annotations.

Reference points for the comparison, star counts and last-push verified via `gh api` on
2026-08-24: `junegunn/fzf` 82,636 / 2026-08-24 · `jesseduffield/lazygit` 81,598 /
2026-08-21 · `dandavison/delta` 31,850 / 2026-08-02 · `Wilfred/difftastic` 25,813 /
2026-08-23 · `extrawurst/gitui` 22,427 / 2026-08-04 · `so-fancy/diff-so-fancy` 18,083 /
2026-08-19 · `jonas/tig` 13,317 / 2026-07-27 · `modem-dev/hunk` 8,711 / 2026-08-24 ·
`dlvhdr/diffnav` 1,527 / 2026-08-22 · `umputun/revdiff` 807 / 2026-08-20. [E]

The strategic read, stated plainly as inference [I]: the plugin's genuinely novel,
unduplicated contribution is **the fleet queue** (nothing else answers "which of my twelve
agents needs my eyes") and **the delivery path** (bracketed paste, typed-but-unsent,
pane-identity re-checked inside tmux's own command queue at paste time — that is careful,
correct work no diff viewer does). The line-list reviewer in between is a from-scratch
reimplementation, in bash and a picker, of the least differentiated and most
UI-demanding part of the stack — and it is losing to `revdiff --stdin` on every axis a
reviewer's eyes care about.

---

## 4. Ranked recommendations

Ranked by (user-visible harm removed) ÷ cost. Each names what it replaces.

| # | What | Why | Cost | Replaces |
|---|---|---|---|---|
| **R1** | **Preview follows the cursor.** Emit each row's offset within its file's diff as a 5th field; `--preview-window="down,55%,border-top,+{5}-/2"`. | D3 — today the review has no working context view at all; you comment on a bare line with the wrong hunk on screen. Proven live on fzf 0.44.1. | **S** | the path-keyed preview at `:365`, which shows hunk 1 for every row |
| **R2** | **Stop losing fields to `IFS=$'\t' read`.** Use a non-whitespace delimiter (`\x1f`) end to end, or `read -d ''`, in `rows_mode:301`, `attach`'s call site, and `fleet_rows:589`. | D1 + D2 — blank rows render invisible, blank-line comments store a bogus quote, and the "diff too large" notice shows the user an empty popup. Silent, universal, one root cause. | **S** | the tab-delimited `read` loops |
| **R3** | **Percentage preview split, list allowed past 8 rows.** `down,55%` + `lrows=min(nrows, avail/2)`. | D13 + the 7-to-23 blank preview rows. Today a resize leaves exactly one list row, and a 29-row popup wastes 24% of itself on whitespace while hiding 44% of the change. | **S** | the fixed `psz=$((avail-lrows-5))` and `lrows>8` cap at `:381-384` |
| **R4** | **Color and align the list.** `--ansi`, green `+` / red-dim `−`, padded `printf '%-*s'` gutters (marker │ old-line │ new-line │ code), file header rows so the path appears once. | D4 + D5 + D6 — the list is the only monochrome surface on a screen whose bottom half is in color, spends up to 30% of its width restating the filename, and merges two coordinate systems into one column. | **M** | `disp` at `lines_mode:242` and `--with-nth=4` |
| **R5** | **Show, count, and delete comments.** `●N` in the marker column; `@` = nested fzf over the store; `alt-x` = delete the block under the cursor. | D8 — three contradictory comments on one line, including a retraction, all deliver, and the screen shows one indistinguishable dot. No edit, no delete, no undo, no way to re-read. | **M** | the boolean `commented[]` test at `rows_mode:292-308` and the write-only store |
| **R6** | **Make the comment box legible.** Two-line `--header` with `--header-first`; `change-preview-label` naming the focused target on `focus` (0.44.1-safe); `transform-prompt` naming it in the prompt where fzf ≥0.45; `^E` → `$EDITOR` for multi-line; drop the dead `18/18` via `--info=hidden`. | D7 — the query line *is* the comment box, sits nine rows from its target, is labelled `review>`, and its only explanation is inside a header that truncates below 100 cols. | **M** | the single 90-char `--header` and static `--prompt` at `:407-408` |
| **R7** | **Review the union of unstaged + staged, and include untracked files.** Merge both numstats and both diffs keyed on path; list untracked files as `new` rows from `git status --porcelain`; enable `-M` rename display. | D10 — a deleted file, a rename, and **a brand-new file (the most common agent output) are all unreviewable today**, with nothing on screen saying so. | **M** | the unstaged-first / staged-fallback fallback at `lines_mode:92-98` |
| **R8** | **Fleet queue: project + branch columns, padded alignment, state/idle sort, column legend.** | D14 — two byte-identical rows for different repos; the churn column lands at a different x on every row; "needs you" ranks by line count, not by who is waiting. | **M** | `fleet_rows:580-583`'s title format and `--with-nth=2,3` |
| **R9** | **`core.quotePath=false` in `tmux-open-path`'s `--preview-diff`.** | D11 — the exact bug `lines_mode:81-91` fixed is still rendering `"src/caf\303\251/…"` in the preview twelve rows below the fixed version. | **S** | `tmux-open-path:69-70,81` |
| **R10** | **Fix the truncation contract.** Width-derived display cap with `--ellipsis '…'` and `«»` overflow markers; write the **untruncated** line to the store. | D9 — a 200-char line is quoted to the agent cut at exactly 160 bytes, so the `>` fragment matches nothing in the file, defeating the anchoring invariant the design rests on. | **S** | the hard `frag=${frag:0:160}` at `lines_mode:240` |
| **R11** | **Loop the fleet queue.** Return to the queue after a review instead of exiting the popup. | D15 — the queue exists for a fleet workflow and structurally cannot iterate; three agents means `prefix r` three times. | **M** | the hard `exit` at `run_loop:416,424,429` (needs a return-code contract instead) |
| **R12** | **Make delivery's confirmation durable.** A persistent one-line notice (popup or `display-message -d 0`) instead of a 750 ms flash, and echo the comment count delivered. | D16 — the sentence telling the human they must press Enter to actually send the review shows for under a second, after the popup is gone. | **S** | `deliver:561`'s `display-message` |
| **R13** | **Add `[`/`]` hunk and `n`/`p` file navigation — or restructure as file-list → line-list, two fzf levels.** | D12 — 8 of 110 rows visible, ~100 Down presses to reach the last file, no position indicator, PageDown undocumented. The two-level shape is strictly easier in fzf and is the GitHub "Files changed" model. | **M**(binds) / **L**(restructure) | the flat single-level list |
| **R14** | **Offer `revdiff --stdin` (or `hunk`) as the reviewer when installed, keep the bash list as the no-dependency fallback; keep the fleet queue and the delivery path as the plugin's own.** | Side-by-side, word-diff, wrap, context-in-list, ranges, multi-line annotations in place, syntax highlighting, and a file tree cannot be built on fzf at any cost. revdiff already ships a tmux `display-popup` launcher and a Claude Code plugin for exactly this use case, and this store already speaks (a subset of) its output grammar — so the seam is a pipe, not a rewrite. The fleet queue and the typed-but-unsent delivery guard are the parts nothing else has. | **L** | D3/D6/D9 and half of D5 wholesale; the from-scratch line-list reviewer becomes the fallback path |

**If only three things get done: R1, R2, R3.** All three are S, all three are in
`run_loop`/`rows_mode`, and between them they turn a popup that shows a blank cursor row,
44% of the change, the wrong hunk, and 7 blank rows into one that shows the whole list and
the line you are standing on.
