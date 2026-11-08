# Review

## Review the agent's changes

`prefix + r` opens the board; `r` on a row opens that agent's diff in
[revdiff](https://github.com/umputun/revdiff), inside the board's popup. You
comment on lines there, and nothing reaches the agent until you press `d` at the
prompt that follows. revdiff is required: without it `r` says
`cockpit: revdiff not found: install it, see #233` and opens nothing
([install](install.md) has the steps).

The board lists every agent pane, on every session, blocked ones first. Each row
shows its diff stat (`+a -d`, `clean`, `no repo`, `?N new`) and branch, so
"which of my agents needs my eyes" is answered without visiting each pane.

The tree a row diffs is the pane option `@cockpit_dir` when it names a
directory, else the session transcript's newest `cwd`, else the pane's cwd.
`r` reviews that same tree: tmux-review asks `cockpit-tui workdir %N` for it
(#470), for every pane whose `@cockpit_dir` does not name a directory. When the
binary is missing, has no `workdir`, exits non-zero or prints anything but an
absolute directory, it takes `@cockpit_dir`, else the cwd. The call is bounded by
`timeout 3`; on a box without GNU `timeout` (stock macOS) it runs unbounded. An
agent pane with no session id is the one case left apart: `workdir` does not run
`claude agents` to find its transcript, so it gets its launch dir, where the
board's row may show the transcript's tree. An agent that made its worktree by hand
(`git worktree add` from the main checkout) keeps its process, and so its cwd,
in the main checkout and drives the tree with `git -C`; its row would read
clean master. One line from the agent fixes that, and every reader (the board,
`--delta` for the Stop hook's ◆, `prefix + i`) follows it:

```sh
tmux set-option -p -t "$TMUX_PANE" @cockpit_dir ~/.cache/cockpit-wt/my-branch
```

Run it as a command of its own, after any `cd`: the `CwdChanged` hook that follows a `cd` sets
`@cockpit_dir` to the new cwd.

`-p` is not optional: a window or global `@cockpit_dir` is inherited by every
pane, and then every row diffs that one tree.

`EnterWorktree` and `claude --worktree` need none of this; they move the
process itself, and the cwd is the tree.

From an **agent pane** the board opens with that pane selected, so `prefix + r`
there is one `r` from its review. Finishing a review returns to the board, on
the pane you just left; `q` is the way out.

The review diffs the **git root of that tree**. `prefix + i` on the pane shows
that root, a review the agent requested names it in the popup title, and a
delivered batch says `Paths are relative to <root>.` An agent that edits another
worktree by absolute path from a main-checkout pane is reviewed on the wrong
tree, and those are how you notice. The fix is workflow, not the plugin: an agent that
works on a branch lives in that worktree, which is what the `WorktreeCreate`
hook above arranges for `claude --worktree` and `EnterWorktree`.

The board opens in a popup. `set -g @cockpit_review_window on` opens it as a
real window (`new-window -n board`) instead.

The order is that question's answer: agents grouped by session, blocked ones
first (see the board's own docs). A blocked pane is listed even with a clean
tree; there is nothing to review there, only someone to unblock. The row's
`where` is `repo⑂branch`, plus ` · wt <name>` in a linked worktree, which
tells two worktrees of the same repo apart; a detached HEAD reads `@<short sha>`.

The row's leading column carries the review mark: `◆` a finished diff nobody has read
yet (the `Stop` gate flagged it), `↵` a review already pasted into that pane
and not yet sent. Without them the board keeps sending you back to a pane you
have already been through.

`◆` is written by `review-gate`, the second `Stop` hook, at the turn boundary
where the agent finishes: it sets `@cockpit_review = pending` when the pane's
tree still differs from its review base. The mark clears when the agent starts
working again, when that diff goes empty at a later `Stop`, or when you approve
with `a`. The same step marks the window's tab with `◆` and recounts `◆N` in
`#{E:@cockpit_fleet}`, which tells you how many finished agents nobody has read
across every session, attached or not. Nothing polls, and a redraw still runs no shell.

The review is revdiff on the tree, untracked files included, against the base
in force (see `b` below). revdiff's own keys apply there, and `?` lists them: you
comment on a line, a range or a whole file, `q` quits and keeps what you wrote,
`Q` quits without it. Comments pinned in an earlier round are loaded into it, so
it shows what is already pinned. When revdiff exits 10 (it wrote comments),
what it wrote replaces the pinned set. When it exits 0 (no comments, or `Q`),
the pinned set stays as it was. Any other exit ends the review with
`cockpit: revdiff exited N: <its first stderr line>`, and the pinned set stays.

Each comment is stored with the line it points at quoted: the quote it already
had for that anchor, else the line as the tree has it now (a `-` line: as the
base has it). The quote, not the line number, is what the agent finds the spot
by after it edits again.

When revdiff closes, the popup asks for one key. Its first line names the base
(`review · base since last review`), the next says how many comments are
pinned, and the last lists the keys that apply:

| key | when | what it does |
|-----|------|--------------|
| `d` | comments are pinned | deliver the batch to the agent's pane, which the prompt names |
| `a` | nothing is pinned | approve what you just read (below) |
| `r` | always | open revdiff again, on the tree as it is now |
| `b` | another base holds | switch to the next base and open revdiff again against it |
| `x` | comments are pinned | discard them, after a `y/N` |
| any other key | always | leave; the pinned comments stay |

A key from the table pressed outside its "when" does nothing. With comments
pinned the prompt names `l` for leave. Leaving keeps the comments on disk:
`prefix + r` and `r` on that pane pick the review up where you left it and say
`cockpit: resumed N pinned comment(s)`. A review opened on a tree that does not
differ from its base never starts revdiff: it ends with
`cockpit: no uncommitted changes in <dir>`, and drops any comments pinned for
that tree.

**Big files are counted, not hashed.** To tell whether a file changed at all
git hashes the whole of it, about 100 ms at 32 MB and 26 s at 1 GB. So when the
cockpit asks whether a tree changed, a changed or untracked file over **32 MB**
is kept away from git and counted as one binary change instead, though a
tracked file whose size did not change is still hashed once, so git can tell it
changed. The file itself is untouched, and the tree still counts as dirty
because of it, so a pane whose only change is a big file stays on the board and
its review still opens. revdiff is not covered by this: it runs its own git, and
an untracked big file is diffed in full when revdiff shows it. Put core dumps in
`.gitignore`, as git's own templates do, and the review never sees them at all.

**`b` picks what the diff is taken against.** The base is the first of three
that holds: the checkpoint `refs/cockpit/reviewed/<branch>` when a review of
this branch was approved (only what moved since), the merge-base with the
upstream (else `origin/HEAD`'s branch, else `main`, else `master`) when that is
not `HEAD` itself (the branch plus its uncommitted work),
else `HEAD` (today's uncommitted view). An agent that commits as it goes stays
on the board that way instead of dropping out of it the moment it commits. `b`
at the prompt cycles the bases that hold, the prompt's first line names the one
in force (`review · base since last review`), and the choice is kept beside the
comments (`comments.md.base`), so a resumed review reads the same diff it did
before, and so does the `Stop` hook's `◆`.

**`a` approves what you just read.** It writes that tree, untracked files
included, to `refs/cockpit/reviewed/<branch>`, clears the pane's review mark and
removes the review directory: `cockpit: review approved · the next one shows
only what changed`. The next review of the branch diffs against it, so it shows
only what the agent changed after you looked. `a` is offered only with nothing
pinned, since approving a batch nobody sent would throw it away: deliver it with
`d` or discard it with `x` first. Before the repo's first commit there is
nothing to approve against, and `a` says so.

That ref outlives the branch. `git branch -D` and `git worktree remove` both
leave it standing, so a branch deleted and recreated under the same name
inherits the old checkpoint. Nothing collects them; the purge is:

```sh
git for-each-ref --format='%(refname)' refs/cockpit/reviewed | xargs -n1 git update-ref -d
```

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
review of the same tree.

A second review delivered to the same pane before the agent stops replaces the
queued one: the spool is a single file, so the agent is handed the newer batch.
Both are in `delivered.md`. `a` on a later round ends the review directory but
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

- **Nothing approves by default.** Closing revdiff never approves, and Enter
  at the prompt leaves. Approval is its own key, `a`, offered only when nothing
  is pinned, after you have been through the diff in revdiff.
- **Never sends Esc to the agent, never auto-submits.** Esc kills whatever
  the agent is mid-tool on, destroying in-flight work; typed text plus a
  human's own Enter lands at the next turn boundary instead
  (claude-code issues #63190, #30492).
- **No review daemon, server, or port.** What *does* outlive the popup is
  one directory per review under
  `${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/review/<pane>-<root>/`,
  created `0700`, holding `comments.md`, the base `b` chose
  (`comments.md.base`), revdiff's history, the batches already delivered
  (`delivered.md`) and any queued spool. It exists so a dropped SSH client,
  which tears down a client-owned popup, cannot take a twenty-comment review
  with it: `prefix + r` on the same pane resumes it, pinned comments and all.
  It lives until `a` approves the review or you discard it with `x`; a
  delivered batch leaves `delivered.md` standing in it, and a refused delivery
  keeps the store and names it in the message. `prefix + i` prints the
  directory for the pane you are on, with how many comments are pinned and
  delivered.
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
  agent's pane until `d`: one paste per review, not a stream of small
  interruptions while you are still deciding what you think.
- **Never selects, creates, or focuses a pane as a side effect.** Delivery
  targets `-t <pane>` and leaves focus alone; tmux focus-stealing has raced
  and corrupted concurrent pane input before (claude-code issue #49184).
- **Never `send-keys` for the review's own text.** The body goes through
  `load-buffer`/`paste-buffer` only — `send-keys` is reserved for the Enter a
  **human**, never this script, presses.

**The agent can ask.** `install.sh` links every `skills/*` dir into
`~/.claude/skills`, so `/cockpit-review` (or "get this reviewed") runs
`tmux-review --request` from the agent's own pane: one popup, the same
loop, and the batch comes back as the tool's result (exit 10) instead of a
paste. `a` there approves as anywhere else.

You can also leave a requested review for later: `l` at the prompt keeps the
comments pinned and `--request` returns 0, so the agent carries on with nothing
in hand. Running `/cockpit-review` again resumes exactly those comments.
