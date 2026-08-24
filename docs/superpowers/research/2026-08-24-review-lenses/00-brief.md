# Shared brief: tmux-cockpit's current review feature

Repo: /home/tzachi/.tmux/plugins/tmux-cockpit (bash + tmux + fzf plugin that monitors many Claude Code agent panes).
READ-ONLY for you: do NOT edit anything in the repo. Write your report ONLY to the output path you were given.

## What exists today (scripts/tmux-review, 743 lines of bash; binding in cockpit.tmux:242-251; README.md:473-560; tests in tests/run.sh)
- `prefix r` opens a tmux display-popup (MODAL, -E) running tmux-review.
- From an agent pane it reviews that pane's cwd; from a non-agent pane it opens a "fleet queue": fzf list of every agent pane with a dirty git tree, ordered by churn, preview = `tmux-open-path --preview-diff`.
- The review itself: `--lines` runs `git diff` (unstaged, else staged), parses it in awk into ONE ROW PER +/- LINE (path, line, fragment, display), test files first, then files by churn desc. No context lines, no hunks, no file tree, no side-by-side. A cap of 5000 rows shows a single "diff too large" row.
- fzf is run with `--disabled` (typed text is the COMMENT, not a filter), `--with-nth=4`, preview = the file's diff via tmux-open-path --preview-diff. Keys: alt-Enter = attach the typed text as a comment on the row under the cursor (execute-silent + reload: the list is rebuilt from scratch — git diff + awk + bash row loop — after EVERY comment), ^/ toggle preview, alt-u/alt-d scroll preview, ^S deliver (exit 20), esc discard. Plain Enter is deliberately a no-op ("no accept-all").
- Comment store: mktemp file, append-only, revdiff grammar: `## path:line (+|-)` / `> quoted fragment` / note. `??` in a comment = question.
- `--compose`: fixed header + store verbatim. `--deliver <pane> <store>`: guards (pane id shape, pane_current_command matches @cockpit_agents_glob, store non-empty) then `tmux load-buffer` + `tmux if-shell -F` re-check + `paste-buffer -p -r -d` = bracketed paste, typed-but-UNSENT into the agent's prompt; the human presses Enter. >2048 bytes → 0600 mktemp file in $XDG_RUNTIME_DIR and only a one-line pointer is pasted. Delivery runs via `tmux run-shell -b` because the popup kills its children on close.
- Invariants the repo holds: no shell on status redraw; state dies with the pane (no daemon/server/db); one-shot scripts; bash 4+; shellcheck + `make check`; never steal focus; never auto-submit into the agent; batch delivery only.
- Prior research (today): docs/superpowers/research/2026-08-24-review-tools-landscape.md (revdiff, difit, hunk, Conductor, Plannotator, Orca, Copilot, Cursor, Zed, Claude Code /code-review, git-appraise, lazygit/tig) and 2026-08-24-review-antipatterns.md. You may read them, but your job is to go BEYOND them and critique what was actually built.

## The owner's concern (verbatim intent)
"The reviewer is not done the best-practice way and is not well designed. Scan the community, look at active, well-starred GitHub projects, and find how we should redesign this — and for performance, why."

## Output contract
Write a markdown report to your assigned path. Be concrete: name projects with star counts and last-commit dates where you can verify them (WebSearch/WebFetch via ToolSearch if you need them; the `gh` CLI is available: `gh api repos/OWNER/REPO --jq '.stargazers_count,.pushed_at'`), cite file:line for repo claims, and tag each claim [E] evidence / [I] inference. End with a ranked list of concrete redesign recommendations for THIS plugin (bash/tmux/fzf constraints in mind), each with: what, why, cost (S/M/L), and what it replaces. Return to the caller ONLY a <=15-line summary; the report file is the deliverable.
