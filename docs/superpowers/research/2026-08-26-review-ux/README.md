# Review popup UX research (2026-08-26)

Owner's complaints about `prefix r`: no way to edit a pinned comment; preview
scrolling is awkward; text cannot be marked; the diff has no line numbers;
the screen is hard to read; `^E` does not work; the `^S` confirm shows `%5`
and a literal `$EDITOR`.

Reproduced in a detached tmux session (120x36 and 80x24) against a fixture
repo; three parallel lenses:

- `layout-survey.md` — how delta, tig, lazygit, difftastic, gh, prr, crit,
  revdiff and GitHub render a commentable diff; three mockups; verdict (a):
  keep list-over-preview, add a two-column gutter, pad the rows, one legend.
- `fzf-0.44.md` — what the installed fzf can do (alt-arrow preview binds,
  `{+n}` under `--multi`, wheel already scrolls the preview, shift+drag
  selects in a popup). Its claim that `execute()` hands the child a tty on
  stdout is wrong: measured, vi prints "Output is not to a terminal".
- `screen-critique.md` — ranked readability defects from the captures.

Root causes found while tracing:

- `^E`: `edit_comment` runs the editor with fzf's captured stdout
  (`out=$(... | fzf ...)`); `^L`/`^R` already write to /dev/tty, `^E` did not.
  With no `$EDITOR`, `$VISUAL` or `@cockpit_editor` set the fallback is `vi`.
- `%5` / `$EDITOR`: `${REVIEW_PANE_ID:-the agent}` and a single-quoted
  printf format in the `^S` picker.
- Editing a pinned comment: `edit_comment` seeds from the query only and
  always appends a new block.
- Ranges: the store grammar already accepts `## path:a-b (+)`; the picker
  never writes one.

## Popups and copy mode, checked against tmux 3.7 (owner's pointer: tmux/tmux#3377)

nicm, 2022-11-05: "you can't currently enter copy mode in a popup." Nothing in
CHANGES through 3.7 adds it; popup entries since are OSC 52 in popups (3.4),
focus events (3.5), nested display-popup modifies the popup (3.6). The
workaround in that thread is a nested session inside the popup
(`display-popup -E 'tmux attach -t x || tmux new -s x'`): the inner tmux owns
the screen, so copy mode and drag-select work, and OSC 52 carries the copy
out. For this picker that also needs fzf `--no-mouse` (else fzf takes the
drag), which loses wheel scrolling of the preview, and leaves a session per
review on the server. Kept as an opt-in idea (`@cockpit_review_select`), not
built; shift+drag is the zero-cost default.
