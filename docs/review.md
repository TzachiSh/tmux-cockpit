# Review

Being replaced by the board (#228).

## Review the agent's changes

`prefix + r` opens a popup review of an agent's uncommitted diff. Comments
attach to individual lines, and nothing reaches the agent until you say so —
there is deliberately no key that approves the whole thing at once.

It always opens the **fleet queue**: every agent pane, on every session,
blocked ones first, previewed the same way the path picker's folder preview
shows a directory's `git status`. A pane with nothing to review reads `clean`
(or `no repo`) in the ± column and `↵` on it jumps there instead of opening
an empty review. Nothing else in this plugin answers "which of my agents
needs my eyes"; the queue exists because visiting each pane in turn to find
out does not scale past two or three of them.

The tree a row diffs is the pane's cwd, unless the pane option `@cockpit_dir`
names another. An agent that made its worktree by hand (`git worktree add`
from the main checkout) keeps its process, and so its cwd, in the main
checkout and drives the tree with `git -C`; its row would read clean master.
One line from the agent fixes that, and every reader (the queue, `--delta`
for the Stop hook's ◆, `prefix + i`) follows it:

```sh
tmux set-option -p -t "$TMUX_PANE" @cockpit_dir ~/.cache/cockpit-wt/my-branch
```

`-p` is not optional: a window or global `@cockpit_dir` is inherited by every
pane, and then every row diffs that one tree.

`EnterWorktree` and `claude --worktree` need none of this; they move the
process itself, and the cwd is the tree.

From an **agent pane** the queue opens with that pane already under the
pointer, so `prefix + r` there is still one `↵` from its review — and the
other agents waiting on you stay on screen instead of being replaced by it.
Finishing a review returns to the queue, on the pane you just left, so
reviewing four agents is four `↵` rather than four `prefix + r`; `esc` on the
list is the way out.

The review diffs the **git root of the pane's cwd**, and the header names it
(`root: ~/.cache/cockpit-wt/api-work`). An agent that edits another worktree
by absolute path from a main-checkout pane is reviewed on the wrong tree, and
that line is how you notice. The fix is workflow, not the plugin: an agent that
works on a branch lives in that worktree, which is what the `WorktreeCreate`
hook above arranges for `claude --worktree` and `EnterWorktree`.

The review opens in a popup. `set -g @cockpit_review_window on` opens it as a
real window (`new-window -n review`) instead, if you would rather keep the
fleet's borders and window tabs visible while you read.

The order is that question's answer, and it is deliberately **not** churn.
Agent state ranks first — `needs-input`, then `error`, `running`, `done`, then
panes with no state at all — and churn only breaks ties inside a rank. An
agent parked on a permission prompt is the one actually waiting on you, and it
usually has nothing at all to diff; sorting by churn listed it last, or (with a
clean tree) left it out entirely, and answered "who typed most" instead. A
blocked pane is listed even with a clean tree, and its `±` column reads `—`
rather than `+0 -0`, because there is nothing to review there — only someone to
unblock.

Each row is `state · title · where · ±`, in aligned columns. The `where`
field is `repo⑂branch`, and it is what tells two worktrees of the same repo
apart, which `s:w claude` alone cannot:

```text
needs-input     main:2 claude    cockpit⑂feat/review-p0     —
◆ done          side:0 codex     cockpit⑂main               +142 -30  ?2 new
↵ running       main:4 claude    api⑂fix/token-refresh      +8 -8
```

The state column carries the review mark: `◆` a finished diff nobody has read
yet (the `Stop` gate flagged it), `↵` a review already pasted into that pane
and not yet sent. Without them the queue keeps sending you back to a pane you
have already been through.

`◆` is written by `review-gate`, the second `Stop` hook, at the turn boundary
where the agent finishes: it sets `@cockpit_review = pending` when the pane's
tree still differs from its review base. The mark clears when the agent starts
working again, when that diff goes empty at a later `Stop`, or when you approve
with `^A`. The same step recounts `◆N` in `#{agent_fleet}`, so the status bar
tells you how many finished agents nobody has read across every session,
attached or not. Nothing polls, and a redraw still runs no shell.

`tmux-review --fleet` prints those rows tab-separated with no fzf, if you want
to drive the same list from a script.

The fleet queue — and only it — takes extra fzf options from a global tmux
option, so you can bend the picker to your own taste without patching the
script:

```sh
set -g @cockpit_fleet_fzf_options "--height=80% --pointer='▸'"
```

It is confined to this picker on purpose. In the comment loop `↵` has a
contract (it pins a comment, it never approves) and `^S` is the consent
gesture; one stray `--bind` from a config file would quietly break either one.

The review popup's two pickers do not share one key set — the fleet queue is
a plain list, `↵` picks a pane; the comment loop's typed text *is* a
comment, so the keys that would collide with typing it move elsewhere:

| key | fleet queue | comment loop |
|-----|-------------|--------------|
| `↵` | review the selected pane | pin the typed comment (empty: nothing) |
| `alt-↵` | — | alias for `↵` |
| `^/` | toggle the preview | toggle the preview |
| `^U` / `^D` | scroll the preview | — (would edit the comment instead) |
| `alt-↑` / `alt-↓`, `alt-u` / `alt-d` | — | scroll the preview a line, half a page (the mouse wheel over it scrolls too) |
| `^R` | — | re-snapshot the diff; names any comment the agent moved (it still delivers, quoted) |
| `^X` | kill the window under the cursor, after a y/N that names it and its pane count — the session too, when it is that session's last window (a forgotten agent elsewhere) | delete the comment(s) on the row under the cursor |
| `^L` | — | show every pinned comment in the terminal |
| `^E` | — | open your editor on the typed text; with nothing typed on a commented row, on that comment (save empty to delete it) |
| `⇥` | — | mark the row (any number) |
| `^Y` | — | copy the file under the cursor, then its diff, to the tmux buffer and the clipboard |
| `alt-y` | — | copy the whole review's diff |
| `alt-c` | — | copy the marked rows' lines as the file has them, tabs and all (a `-` row: HEAD's line; or the row under the cursor), to the tmux buffer and the clipboard |
| `^B` | — | cycle the base the diff is taken against |
| `^A` | — | approve what you just read (only with nothing pinned) |
| `^S` | — | deliver the whole batch; asks first |
| `esc` | cancel | leave — the batch stays pinned (asks first) |

Inside the review: `↵` pins whatever you typed as a comment on the diff line
under the cursor — `alt-↵` does the same thing, kept as an alias for
terminals where the plain chord doesn't land. `^E` hands what you have typed
to your editor and pins the file that comes back, so a comment that needs three
lines costs no more than one — an empty file pins nothing, so deleting
everything and saving is the cancel. With nothing typed on a row that already
carries a comment, `^E` opens that comment instead and replaces it; saving an
empty file deletes it. Several comments on one row open as one buffer and
save back as one: the agent reads them as paragraphs under one anchor either
way. `^E` uses `@cockpit_editor` when set
(`set -g @cockpit_editor nano`; a **terminal** editor, because the path picker
opens the same option in a tmux window), else `$VISUAL`, else `$EDITOR`. With
none of the three set it tries VS Code's remote shim, and falls back to `vi`.
A GUI editor goes in `VISUAL`: `tmux set-environment -g VISUAL 'code --wait'`.
The popup's children see the tmux *server's* environment, so a shell `export`
made after the server started is not seen. That shim arm is for the Remote-SSH
or Coder host where there is no `code` on the `PATH` and usually no `$EDITOR`
either, and where `vi` in a popup is the wrong answer next to the VS Code
window the diff is being read in. It takes `--wait`, so the buffer comes back
the way `vi`'s does, and `^S`'s confirm offers
`edit the batch in VS Code first`. From a plain ssh session nothing can reach
the laptop's VS Code. The list reloads in place and marks the row `●` (the
digit instead, when more than one comment sits there), so five comments cost
one popup, not five — the same `execute-silent(...)+reload(...)` shape `tm`'s
kill picker already uses.
`^S` delivers the whole batch, and asks first: it names the count and the
target pane by its title, and offers the batch to your editor as text before it
goes (`deliver N comment(s) to fix the parser (%7)` /
`edit the batch in nano first` /
`keep reviewing`). `esc` asks the same way once anything is pinned
(`leave — N comment(s) stay pinned` / `keep reviewing` / `discard N
comment(s)`), because one reflexive escape should not throw away a
twenty-comment review. `leave` keeps them on disk: `prefix + r` on that pane
picks the review up where you left it.
**`↵` pins a comment — it never approves.** With nothing typed it does
nothing: there is no accept-all key on purpose, a review whose only exit is
one Enter on the whole diff is the rubber stamp this replaces.

The rows are a **snapshot** taken when the popup opens, so an agent that
keeps editing cannot move a `●` onto a line you never touched. When you do
want its newest lines, `^R` takes a fresh snapshot on purpose and names the
comments that no longer sit on a changed line — they still deliver, because
the quoted fragment is what the agent finds them by. A resumed review says the
same thing on open: `resumed 5 pinned comment(s), 2 no longer anchor`.

To copy a piece of the preview, hold **shift** and drag: a tmux popup has no
copy mode, so the terminal's own selection is the one that works there. `^Y`
copies the whole file plus its diff, `alt-y` every file's diff, for pasting
into another agent. `⇥` marks rows and `alt-c` copies their lines as the file has them, one per
row, when what you want is a handful of lines rather than a file. All three go
out as OSC 52, so they reach the system clipboard only if your terminal accepts
clipboard writes; some disable them by default.

**Big files are named, not diffed.** To tell whether a file changed at all git
hashes the whole of it, about 100 ms at 32 MB and 26 s at 1 GB, and the popup
is modal and rebuilds its rows on every reload. So a changed or untracked file
over **32 MB** is kept away from git and reported on one notice row at the top
of the list instead, though a tracked file whose size did not change is still
hashed once, so git can tell it changed:

```text
data/dump.bin (412 MB) skipped — over 32 MB, review it directly
```

The row carries no comment, no `^Y` and no mark glyph, exactly like the row a
diff too large to anchor line-by-line leaves. The file itself is untouched, and
the tree still counts as dirty because of it, so a pane whose only change is a
skipped file stays in the fleet queue and its review opens on that notice
rather than on an empty diff. The path picker's `^G` diff preview refuses any
file over 32 MB, naming the size. Put core dumps in `.gitignore`, as git's own
templates do, and the review never sees them at all.

**`^B` picks what the diff is taken against.** The base is the first of three
that holds: the checkpoint `refs/cockpit/reviewed/<branch>` when a review of
this branch was approved (only what moved since), the merge-base with the
upstream when that is not `HEAD` itself (the branch plus its uncommitted work),
else `HEAD` (today's uncommitted view). An agent that commits as it goes stays
in the queue that way instead of dropping out of it the moment it commits. `^B`
cycles the bases that hold, the picker's header names the one in force
(`base: since last review`), and the choice is remembered beside the comments,
so a resumed review reads the same diff it did before.

**`^A` approves what you just read.** It writes that tree, untracked files
included, to `refs/cockpit/reviewed/<branch>` and removes the review directory.
The next review of the branch diffs against it, so it shows only what the agent
changed after you looked. With comments pinned `^A` refuses and says to deliver
or discard them first: approving a batch nobody sent would throw it away.

That ref outlives the branch. `git branch -D` and `git worktree remove` both
leave it standing, so a branch deleted and recreated under the same name
inherits the old checkpoint. Nothing collects them; the purge is:

```sh
git for-each-ref --format='%(refname)' refs/cockpit/reviewed | xargs -n1 git update-ref -d
```

**`↺` marks a row you commented on in an earlier round** of the same review:
delivered already, with nothing pinned on it now. It reads "you said this last
time and the line is still here", which a fresh diff on its own cannot tell you.

A comment lands as:

```text
## scripts/tm:43 (+)
> the quoted fragment
use errors.Is() instead of direct comparison
```

anchored to the line's own **quoted text**, not a bare number, so it still
points at the right spot after the agent edits again — a bare line number
would not survive that. A comment containing `??` is a **question**: the
composed header tells the agent to answer it in chat, not to edit for it.

**Delivery is two steps, never one.** The finished batch lands **typed but
unsent** in the agent's pane — one bracketed paste, never an Enter — and a
human presses Enter there to actually send it. Live-verified on tmux: the
pane shows the pasted text sitting at the prompt, unexecuted, with and
without a trailing newline on the payload. Over 2048 bytes it lands as a
pointer to a `0600` file in `$XDG_RUNTIME_DIR` instead, so a long review is
not a wall of text typed straight into the agent's own prompt.

**A delivered review is a state, not a flash.** The notice naming the target
window stays up until the next message rather than blinking away in 750 ms,
and the pane itself is marked `@cockpit_review = sent` for as long as the
review sits there unsent. `prefix u` walks to those panes after the ones
whose agent is blocked on you, so a review you pasted and then forgot is
still reachable by a keypress instead of by memory. The mark clears the
moment the agent starts working again — pressing Enter is what fires the
`running` hook — so it never outlives the thing it describes.

**A delivery into a busy pane is queued, not refused.** When the target is
running or sitting on a permission prompt, nothing is pasted: the composed
review is written beside the comment store and the pane is armed with
`@cockpit_review_spool`. `review-gate` hands that text to the agent at its next
turn end as `hookSpecificOutput.additionalContext` on stdout, which the
transcript shows as Stop hook feedback rather than a hook error, and the spool
is disarmed before the hook speaks, so a queued review is handed over once and
never repeated. It costs no keystrokes: no paste, no Enter, no focus change,
and the turn boundary is the hook itself.

That drain is the hook's job and nothing else does it. Without `review-gate`
installed (`install.sh --hooks`), a delivery into a busy pane still archives
the batch to `delivered.md` and still writes the spool, but nothing ever reads
it; install the hook, or deliver again once the pane is idle.

A `Stop` hook does not fire on a user interrupt (Esc). A review queued for an
agent you interrupted therefore waits: nothing hands it over early, and it
lands at the end of the agent's *next* turn — after whatever you prompt next
has already run. `prefix + r` on that pane does not paste it; it opens a fresh
review of the same rows.

A second review delivered to the same pane before the agent stops replaces the
queued one: the spool is a single file, so the agent is handed the newer batch.
Both are in `delivered.md`. `^A` on a later round ends the review directory but
leaves a still-queued spool alone — the review you already sent is in flight,
and it still lands at the agent's next `Stop`.

Delivery can still be **refused**: the target is not an agent pane, or is not a
pane id at all. A busy one is not a refusal, it is the queue above. A refusal
keeps the batch: the comment store is left on disk, and the message names it
along with the `--deliver <pane> <store>` that re-sends it — spelled with the
script's own resolved path, not a bare `tmux-review`, so it pastes back even
from a plugin-only install where nothing is on `$PATH` yet.

The paste is gated a second time inside tmux, in the same command-queue item,
because the shell's own check is a ~50 ms-stale snapshot: a pane that stops
being an idle agent inside that window takes the queued arm rather than the
paste. A pane that is gone by then takes neither arm, and the store is kept
for the by-hand re-delivery.

What this deliberately does **not** do, and why:

- **No whole-diff, one-Enter approval.** The unit of review is the line or
  the hunk, never "the changes" — whole-diff review launders a change as
  reviewed without anyone actually reading it, and skimming has a measured
  ceiling of around 400 lines past which defect detection collapses.
- **Never sends Esc to the agent, never auto-submits.** Esc kills whatever
  the agent is mid-tool on, destroying in-flight work; typed text plus a
  human's own Enter lands at the next turn boundary instead
  (claude-code issues #63190, #30492).
- **No review daemon, server, or port.** What *does* outlive the popup is
  one directory per review under
  `${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/review/<pane>-<root>/`,
  created `0700`, holding `comments.md`, its row snapshot, the base `^B` chose,
  the batches already delivered and any queued spool. It exists so a
  dropped SSH client — which tears down a client-owned popup — cannot take a
  twenty-comment review with it: `prefix + r` on the same pane resumes it,
  marks and all. It lives until `^A` approves the review or you discard it; a
  delivered batch leaves `delivered.md` standing in it, which is what the next
  round's `↺` marks read, and a refused delivery keeps the store and names it
  in the message. `prefix + i` prints the directory for the pane you are on.
  There is no garbage collector; `rm -rf ~/.local/state/tmux-cockpit/review`
  is the purge.
  The spillover file an oversized review lands in is the delivery vehicle,
  not a store — the OS tmpdir is the only garbage collector it gets, the
  same as everything else this plugin writes.
- **Every comment carries `path:line` and the quoted source line**, never a
  line number alone. A bare number goes stale the instant the agent edits
  again — the same staleness that makes a GitHub review comment "outdated"
  the moment the diff it anchored to moves.
- **Delivery is a batch, never a comment at a time.** Nothing reaches the
  agent's pane until `^S` — one paste per review, not a stream of small
  interruptions while you are still deciding what you think.
- **Never selects, creates, or focuses a pane as a side effect.** Delivery
  targets `-t <pane>` and leaves focus alone; tmux focus-stealing has raced
  and corrupted concurrent pane input before (claude-code issue #49184).
- **Never `send-keys` for the review's own text.** The body goes through
  `load-buffer`/`paste-buffer` only — `send-keys` is reserved for the Enter a
  **human**, never this script, presses.
- **Test files sort first, never last** in the diff-line list — a change to
  the tests is read before the change it tests, not after.

**The agent can ask.** `install.sh` links every `skills/*` dir into
`~/.claude/skills`, so `/cockpit-review` (or "get this reviewed") runs
`tmux-review --request` from the agent's own pane: one popup, the same
loop, and the batch comes back as the tool's result (exit 10) instead of a
paste — revdiff's contract. `^A` there approves as anywhere else.

You can also leave a requested review for later: `esc → leave` keeps the
comments pinned and `--request` returns 0, so the agent carries on with nothing
in hand. Running `/cockpit-review` again resumes exactly those comments.
