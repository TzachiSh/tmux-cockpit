# Review popup follow-ups: the eight open questions (2026-08-26)

Issue #22. Eight items left over from #4 / PR #19, each a question in three
parts: is it a real need, how would we do it, what do the tools around us do.
The issue's rule: answer the need first, a "no" closes the item. This is the
research for the third part and the decision for the first, per item. Four
Sonnet agents read the sources (tig, lazygit, delta, GitHub/GitLab, Gerrit,
Phabricator, prr, crit, fzf's own docs, tmux's man page); the session lead
checked each claim against the repo's code and made the calls. Sources are
linked inline; anything marked `[repo]` was read from this checkout.

## Decisions

| # | Item | Decision | Why, in one line |
|---|------|----------|------------------|
| 1 | alt-c copies a stripped, cut fragment | task | every tool copies the real line; ours is the only one that copies a display proxy |
| 2 | a saved ^E merges several comments into one | skip | the consumer is one agent reading paragraphs under one anchor; only the `●2` glyph changes |
| 3 | legend truncates below ~90 columns | one-line fix | reflow to 76 cells (an 80-column popup after its border and fzf's pointer margin); the full map already lives in `--help` |
| 4 | a corrupt store is set aside silently | one-line fix | the `.bak` note reaches `tmux display-message` from a `load_store` wrapper `^X` and `^E` both use; the lib stays tmux-free |
| 5 | alt-c on the cap row copies the notice | folded into 1 | the new copy path refuses `CAP_SENTINEL` like `attach` and `^Y` |
| 6 | the fleet picker's border is black | one-line fix | the same `border:8,scrollbar:8` token in the four other pickers; all five draw a `border-top` preview |
| 7 | `▶` overwrites a digit past 9999; ambiguous width | task | `%6` gutter and a neutral-width glyph; six pinned test strings move with it |
| 8 | an in-place edit re-derives the quote from the live file | task | snapshot, never requote: carry the stored quote through `^E`; a "moved" state is #14's |

## 1. alt-c copies a stripped, cut fragment

What ours does `[repo]`: `--bind alt-c` pipes `{+3}` (the list row's display
fragment: control characters stripped, cut at 160 bytes) into `tmux
load-buffer`. Tab-indented code pastes flush left; a long line pastes cut.

What the tools do:

- delta has no copy action; it is a pager, and copying is terminal selection
  over the rendered line. It strips the `+`/`-` markers so a selection pastes
  as code, and renders the real line content, not a synthetic proxy. Its
  line-number gutter gets caught in a selection and users turn it off for
  copying (https://github.com/dandavison/delta/discussions/944,
  https://dandavison.github.io/delta/hyperlinks.html).
- tig has no copy command; the bindings people write shell out to `git show`
  piped to `xclip`/`pbcopy`, so a copy re-fetches the real bytes from git
  rather than reusing the screen (https://github.com/jonas/tig/wiki/Bindings).
- lazygit has an explicit copy (`ctrl-o`, "copy selected text to clipboard")
  that copies the raw diff line verbatim, marker included; users are asking
  for the marker to be stripped, never for less of the line
  (https://github.com/jesseduffield/lazygit/issues/4511,
  https://github.com/jesseduffield/lazygit/issues/1482).
- GitHub's diff view has no per-line copy; selection copies the rendered row
  with its marker, and refined-github's fix makes the marker unselectable
  rather than touching the text
  (https://github.com/refined-github/refined-github/pull/282). `gh pr diff`
  is raw diff text by construction.

Need: yes. Every copy path in the field ends at the real, untruncated line.
Ours is the one that hands over a display proxy, and the people who use
alt-c paste into another agent, where tabs and the tail of the line matter.

How: `attach()` already reads the whole line back from the tree (`sed -n Np`
on `+` rows, `git show HEAD:path` on `-` rows). Hoist that read into one
function and give alt-c a `--copy-lines <dir> <path> <line>...` mode over
`{+1}`/`{+2}` instead of `{+3}`.

## 2. A saved ^E on a row with several comments merges them into one

What ours does `[repo]`: `seed_comment` prints every body on the anchor,
blank-line separated; `edit_comment` writes the buffer back as one record
(`detach` then `attach`), so `●2` becomes `●`. A cancel (unchanged buffer) is
already refused.

What the tools do:

- GitHub keeps one thread per comment origin; several threads can anchor the
  same line and stay separately repliable and resolvable
  (https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/commenting-on-a-pull-request,
  https://github.com/go-gitea/gitea/issues/35279).
- GitLab designed for it: "multiple resolvable discussions per line", with a
  UI distinction between "reply" and "start another thread" because two
  reviewers raise unrelated points on one line
  (https://gitlab.com/gitlab-org/gitlab-ce/issues/32452,
  https://docs.gitlab.com/api/discussions/).
- Gerrit and Phabricator: inline comments are independent objects on a
  line; nothing merges
  (https://www.gerritcodereview.com/2020-11-18-gerrit-news-jun-nov-2020.html).
- prr is a mailing-list style file edited by hand, no per-line record model
  (https://dxuuu.xyz/prr.html). crit is `crit comment path:line body`, one
  call per comment, stored as JSON (https://crit.md/). Neither documents a
  merge.

Need: no. Every platform keeps comments separate because several *people*
comment on one line and each thread resolves on its own. Here one reviewer
pins comments for one agent, and the delivered text is the same either way:
one `## path:line` heading followed by the paragraphs. Nothing downstream
resolves records one at a time. The only visible difference is `●2` vs `●`,
and a picker or a separator grammar inside the editor buffer would be a
subsystem for that glyph. Closed; the README's ^E line says "one buffer per
row" so the behaviour is stated rather than discovered.

## 8. An in-place edit re-derives the quoted line from the live file

What ours does `[repo]`: `edit_cmd` passes the row's display fragment (`{3}`)
to `attach`, which reads the live line and keeps it only if it still starts
with the fragment. If the agent moved the line since the snapshot, the guard
fails and the stored `>` quote shrinks to the 160-byte fragment.

What the tools do:

- GitHub pins a review comment to `original_commit_id` /
  `original_line` / `original_position` and shows it "outdated" when a later
  commit touches the line; it never requotes
  (https://docs.github.com/en/rest/pulls/comments).
- GitLab pins a diff note to a `base_sha`/`start_sha`/`head_sha` triple and
  marks the discussion outdated with a system note; a project setting can
  auto-resolve instead
  (https://gitlab.com/gitlab-org/gitlab-ce/merge_requests/11584).
- Gerrit anchors to a patchset and offers "ported comments" as a read-time
  projection, the stored anchor untouched.
- Phabricator is the one tool that tracks a moved line, by heuristic, renders
  the ported comment "ghostly", and lets users turn it off
  (https://secure.phabricator.com/book/phabricator/article/differential_inlines/).
- prr quotes hunks into its review file at fetch time: a snapshot by
  construction.

Need: yes, narrowly. The stored quote is the review's snapshot and an edit
must not degrade it. Tracking a moved line is Phabricator territory and goes
to the epic (#14) as a `↺`-style state, not here.

How: `seed_comment` hands the stored `frag` to `edit_comment`, which passes it
to `attach` as the fragment. `attach`'s guard then keeps the live line only
when it still starts with the *stored* quote, and otherwise keeps the stored
quote itself, which is exactly the snapshot rule.

## 3. Legend truncates below ~90 columns

What ours does `[repo]`, measured: the three fixed key lines are 77, 53 and
64 cells; the dynamic fourth (`root: … base: … ● pinned ↺ last round ↺ N
delivered earlier`) runs to about 135 with a worktree root. An 80-column
popup leaves 76 cells for a header line: 2 for the popup border, 2 for fzf's
pointer margin. So line 1 lost the tail of `^R refresh`, and line 4 lost the
delivered count, which sat last.

What the tools do: lazygit shows a one-line hint bar and `?` for the full
map; tig has `h`; fzf-based tools (fzf-git, forgit) show little and rely on
docs. fzf 0.44 has `change-header` and `toggle-header` actions, and a help
key can `execute(less)` a text; `?` is typeable here because the query is a
comment, so a help key would have to be `F1` or `alt-h`.

Need: yes at 80 columns, and a 115-column terminal at the default 70% width
is one.

How: reflow, no help screen. `^R refresh` moves to line 2 (64/66/64 cells),
and `$ndlv` moves ahead of the glyph legend on line 4, so the count that
changes between rounds is the part that survives a long root. `--help`
already carries the full key map. A help key is a subsystem for three lines
that fit once rebalanced.

## 7. `▶` overwrites a digit past 9999 lines; `▶` is ambiguous-width

What ours does `[repo]`, measured: a 12345-line file with line 12340 changed
rendered its removed row as `▶2340       │ -12340`; the `%5` gutter overflowed
and the mark ate the first digit.

What the tools do: delta's line-number gutter widens to the file (its
`line-numbers-left-format` defaults to `{nm:^4}` and grows); tig marks the
current line with reverse video, not a glyph; fzf's own `--highlight-line`
arrived in 0.52, we pin 0.44. Unicode's EastAsianWidth lists U+25B6 `▶` as
Ambiguous (two cells in a terminal set to ambiguous-wide) and U+25B8 `▸` as
Neutral.

Need: yes. Lockfiles and generated files run past 9999 lines and get
reviewed.

How: `%6` in the gutter's five `printf`s (the ceiling moves to 99999) and
`▸` for the mark. The six pinned test strings widen with it, plus two new
pins: the 12340 case and a `\ No newline at end of file` row, which the
issue listed as untested. A width sized to the file's largest line number
(delta's and GitLab's shape) needs a pass over the diff before the render
or a numstat read; one column for a ceiling nobody reviews past is the
cheaper answer. The render bytes changed, so the memo cache's version tag
(`gutter1` → `gutter2`) moves with them; a warm cache from before the
upgrade otherwise keeps serving the old gutter.

## 4. A corrupt store is set aside silently under ^E and ^X

What ours does `[repo]`: `parse_store` (`scripts/lib/review-store.sh:70`)
moves an unparseable `comments.md` to `.bak` and prints one line to stderr.
`^X` and `^E` run under fzf's `execute-silent`, which swallows stderr, so the
screen simply loses every mark.

What the tools do:

- git: `fatal: bad config line N in file <path>`, refuses the operation,
  leaves the file where it is
  (https://gist.github.com/dsmith73/cc22031410d6660fd6e999608bb9be91).
- lazygit: prints the YAML error with the offending field and refuses to
  start (https://github.com/jesseduffield/lazygit/issues/4973).
- tig: one `tig warning: ~/.tigrc:N: ...` per bad line, keeps the rest
  (https://man.archlinux.org/man/tigrc.5.en).
- tmux `source-file`: `parse error near ...` on the status line or stderr,
  file untouched (https://github.com/gpakosz/.tmux/issues/753).
- Claude Code: a malformed `settings.json` dropping rules silently is filed
  as a bug (https://github.com/anthropics/claude-code/issues/2835,
  https://github.com/anthropics/claude-code/issues/24823); `~/.claude.json`
  gets a timestamped backup and a prompt
  (https://github.com/anthropics/claude-code/issues/5563).
- revdiff falls the whole input back to raw text rather than dropping
  sections, and auto-saves annotations to a history dir
  (https://github.com/umputun/revdiff).

Nobody renames silently. The two fixed shapes are "refuse, name the line" and
"back up, then say so".

Need: yes. `^S`'s "edit the batch" invites hand edits, and a hand edit is
how a store stops parsing.

How: the `.bak` move stays (the lib's contract is that a hook never fails on
a bad store, so refusing like git is out). The lib also stays tmux-free (a
test pins "touches neither tmux nor git"), so the note is a `tmux
display-message` in `tmux-review`'s one `load_store` wrapper, which `^X` and
`^E` both go through: a non-empty store that parses to nothing and now has a
`.bak` beside it was just set aside. A timestamped
backup name is not worth a line: a second failed parse is the same file
edited again, and the first `.bak` was already superseded by that edit.
`tmux display-message` from inside the popup reaches the status line of the
client that opened it and survives the popup closing.

## 5. alt-c on the `(diff too large)` row copies the notice text

What the tools do: git prints `Binary files a/x and b/y differ` as diff text
and tig, lazygit and delta let it flow through their panes like any line;
GitHub renders large and binary diffs as an inert banner with a "Load diff"
control, nothing to copy
(https://github.com/sindresorhus/refined-github/issues/2043).

Need: cosmetic, and it costs one line once item 1 exists: the new copy path
takes the same `CAP_SENTINEL` refusal `attach` and `^Y` already have. Folded
into 1.

## 6. The fleet picker's border is still black

What ours does `[repo]`: the review loop passes
`--color=16,border:8,scrollbar:8`; the fleet queue (`tmux-review:2164`),
`tm` (two calls), `tmux-branch` and `tmux-open-path` (two calls) pass bare
`--color=16`. All five draw a `--preview-window=...,border-top`, so the
`border` colour is live in each: it is the separator above the preview, not
the popup frame.

What the tools do:

- fzf's base16 theme hard-coded black and white for some elements, the
  border among them, and upstream calls it a rendering bug: fixed in
  https://github.com/junegunn/fzf/pull/4501, reported as
  https://github.com/junegunn/fzf/issues/4560. We are on 0.44, before the
  fix.
- `border` is a documented `--color` key ("Border of the preview window and
  horizontal separators") layered over a named scheme, which is the shape we
  already use (https://junegunn.github.io/fzf/reference/).
- Every themed fzf config pins `border` explicitly in `FZF_DEFAULT_OPTS`;
  catppuccin ships `--color=border:#363a4f,label:#cad3f5`
  (https://github.com/muesli/fzf-catppuccin).
- tmux 3.3+ `popup-border-style` and `popup-border-lines` style the popup's
  own frame, a second border fzf never reads
  (https://github.com/tmux/tmux/issues/2993). fzf's `--tmux ...,border-native`
  picks one frame or the other; it does not blend them
  (https://github.com/junegunn/fzf/blob/master/ADVANCED.md).

Need: yes. It is an upstream bug on the fzf version we pin, in every picker.

How: the same `border:8,scrollbar:8` token on the four bare `--color=16`
calls. Following `popup-border-style` instead is a no: it styles a different
border, and the one fzf paints black is the preview separator inside the
popup, which tmux cannot see. One shared `FZF_COLOR` would need an
environment contract between `cockpit.tmux` and five scripts for one token;
not worth it.

## Not in scope

- The two untested editors from the issue (VS Code Remote-SSH, Coder
  code-server) need a real remote window; nothing here changes them.
- The test-debt list stays with the files it names. The items adjacent to the
  code touched here (`--copy`'s cap assertion, the `\ No newline` gutter
  branch, emptied-file-deletes) get their fail-first test in the same task.
