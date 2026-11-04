---
name: cockpit-review
description: Ask the human for a code review of this pane's changes through the tmux-cockpit review popup, then act on the comments. Use when you finished a piece of work and want it looked at before going on, or when the user says "review this" / "get a review".
allowed-tools: Bash(*tmux-review*--request*), Bash(tmux set-option -p *@cockpit_dir*)
---

If the tree you work in is not your cwd (a worktree you made with `git
worktree add`, driven with `git -C`), say so once first, or the review shows
the wrong tree:

    tmux set-option -p -t "$TMUX_PANE" @cockpit_dir <path-to-that-worktree>

Run it as a command of its own, after any `cd`: the hook that follows a `cd` sets it to the new cwd.

Keep the `-p`: without it the option is inherited by every pane on the server.

Run this, with a long timeout (the human is reading and annotating; do not
poll, do not retry, do not run anything else meanwhile):

    "${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}/scripts/tmux-review" --request "$TMUX_PANE"

Read the exit code:

- **10** — the review is on stdout. Follow its header: answer every `??`
  question in chat first, then make the edits in one pass, disagree out loud
  where a comment is wrong, and end with one close-out line per comment
  (`<path>:<line> — done | skipped: <reason>`).
- **0** — the human pinned nothing, or left the review for later (their
  comments stay pinned; running this again resumes them), discarded, or
  approved. Say so in one line and continue with what you were doing.
- **1** — the popup could not open (no attached client, the pane is gone), or
  revdiff is not installed or failed (its message is on stderr).
  Report stderr verbatim. Do not retry.

If the command timed out, the popup is still open and nothing is lost: wait
for the human to tell you the review is done, then run the same command
again — it resumes the pinned comments, and their `d` at the prompt hands them to you.
