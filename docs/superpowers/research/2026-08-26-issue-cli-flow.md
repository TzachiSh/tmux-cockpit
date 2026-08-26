# Issue flow from tmux: research and plan (2026-08-26)

Owner's ask: bugs and feature requests start as a GitHub (or GitLab) issue the
agent opens; the human wants to open, read, comment on and close that issue
from tmux instead of the browser. What the CLIs give us, what flow to build,
what not to do, what already exists, and a plan for later.

Verified locally on this machine unless marked `[docs]`. gh 2.98.0, fzf 0.44.1,
bat 0.24.0, jq, tmux 3.7c. `glab` is not installed (`command -v glab` returns
nothing), so every glab line is docs-only. Researched by an Opus agent, checked
and filed by the session lead.

## 1. What the CLIs give us today

`gh auth status`: logged in, token scopes `gist, read:org, repo, workflow`.
`repo` covers issue create/comment/close/label and `gh issue develop`.

| Operation | gh (verified here) | glab `[docs]` | Gap |
|---|---|---|---|
| create | `gh issue create --title T --body B` / `-F, --body-file file` (`-` = stdin) / `-l, --label` / `-T, --template` / `-e, --editor` / `-w, --web` / `-a` / `-m` / `--type` | `glab issue create -t --description/-d`, `--description-file` (`-` = stdin), `-l`, `--template`, `--web`, `--no-editor`, `-y/--yes` | glab has an open bug where `-y` still shows a template and a description prompt (gitlab-org/cli#1121), so glab create is not reliably non-interactive |
| list + filter | `gh issue list -s open\|closed\|all -l LABEL -A author -a assignee -S "query" -L n` | `glab issue list -A/-c -l --author --assignee --search --in --order -P` | parity |
| list JSON | `gh issue list --json number,title,labels,state,updatedAt --jq '...'` (field allow-list plus jq) | `glab issue list --output json --jq EXPR` | glab returns whole objects; no field allow-list |
| view | `gh issue view N`, `-c/--comments`, `--json <30 fields>`, `-w` | `glab issue view N -c`, `-F json`, `--jq`, `-w`, `--page/--per-page` | parity |
| comment | `gh issue comment N -b TEXT` / `-F file` (`-` = stdin) / `-e` / `--edit-last` / `--delete-last` | `glab issue note N -m MSG` (alias `comment`); editor if `-m` omitted | glab has no file/stdin body for notes, only `-m` or an editor |
| edit / label / close | `gh issue edit N --add-label --remove-label -t -b -F`, `gh issue close N -c COMMENT -r completed\|"not planned"\|duplicate`, `gh issue reopen N` | `glab issue update N -l -u -t -d --description-file`, `glab issue close N`, `glab issue reopen N` | glab close has no `--comment` or `--reason` |
| branch from issue | `gh issue develop N [-n NAME] [-b BASE] [-c] [--list]` | none | real gap; closest is `glab mr create --related-issue N --create-source-branch`, which also opens an MR |
| raw API | `gh api repos/OWNER/REPO/issues --jq ... --paginate -X -f -F --input -` | `glab api <endpoint> -X -f -F --input - --paginate --output json\|ndjson` | parity, but the `-f`/`-F` letters mean different things in the two tools |
| self-hosted host | `GH_HOST`, `-R HOST/OWNER/REPO` | `GITLAB_HOST`, `glab auth login --hostname` | glab's `GITLAB_HOST` is ignored by `glab auth status` (gitlab-org/cli#8065) |

Exact commands run here, with output:

- `gh issue list --state all --json number,title,labels,state,updatedAt --jq '.[]|[.number,.state,(.labels|map(.name)|join(",")),.title]|@tsv'` returns two TSV rows, labels empty. This is the picker's feed.
- `gh issue list --state all --search "worktree"` returns only `3`. Dedupe search works.
- `gh issue view 3 --comments` prints an `author:/association:/edited:/status:` header, then `--`, then the comment body.
- `gh api repos/TzachiSh/tmux-cockpit/issues --jq '.[]|"\(.number) \(.title)"'` returns 4, 3 and 2 (2 is closed and does not show in `gh issue list --state all`, worth knowing).
- `gh issue develop 3 --list` prints nothing, so no linked branches yet, and the scope is sufficient.
- `gh issue create </dev/null` and `gh issue create --title x </dev/null` both print ``must provide `--title` and `--body` when not running interactively`` and create nothing. This is the anti-hang guarantee.
- `printf 'body\n' | gh issue create -R TzachiSh/definitely-not-a-real-repo-9z8x --title probe --body-file -` fails only at repo resolution, so `--body-file -` parses without any prompt.
- `gh label list`: accessibility, bug, documentation, duplicate, enhancement, good first issue, help wanted, invalid, question, wontfix. Neither #3 nor #4 carries a label today.

## 2. Provider detection

Lazy verdict first: do not detect anything. `git remote -v` here is
`https://github.com/TzachiSh/tmux-cockpit.git` and every repo the owner touches
is GitHub. `gh` already prints a clear error on a non-GitHub remote, so a
detection branch would only duplicate gh's own message.

If it is ever needed, the whole thing is one line, no abstraction layer:

```sh
case "$(git -C "$dir" remote get-url origin 2>/dev/null)" in *gitlab*) exec glab_path ;; esac
```

Nothing in the repo parses remotes today. `grep -rn 'remote get-url\|github.com'
scripts/ cockpit.tmux` finds only `scripts/tmux-branch:107 switch_remote()`
(origin/foo to a local branch) and tmux-review's `origin/HEAD` base resolution.
Greenfield, and it should stay empty. Self-hosted GitLab needs a host guess as
well, and glab's own `GITLAB_HOST` is buggy, so the abstraction would be wrong
on arrival.

Verdict: GitHub only. One `command -v gh` guard, the same shape as
`command -v fzf` at `scripts/tmux-review:950` and `:1897`.

## 3. The user flow, good

### (a) Agent opens the issue

No new code. The agent already has Bash and a place to write files.

1. Search first: `gh issue list --state all --search "<3 words from the symptom>"`. If a number comes back, comment on it instead.
2. Write the body to a real file it already owns, for example `docs/superpowers/research/<date>-<slug>.md`, then `gh issue create --title "<one line>" --label bug --body-file <that file>`.
3. Print the URL gh returns, and stop. The agent does not label beyond the fixed set and does not close anything.

The one rule that makes this safe is `--body-file`. `gh issue create` refuses
to run without `--title` and `--body`, verified above, so the agent can never
hang a pane on an editor prompt.

### (b) Human reviews from tmux

`prefix g` opens a popup in the existing chrome, `-w "$PW" -h "$PH" -T ' issues
· <repo> '`, same as `prefix b` at `cockpit.tmux:235`.

Inside is one fzf list, filtered as you type, with `gh issue view {1}
--comments` as the preview, the exact shape of the fleet queue at
`scripts/tmux-review:1975`:

```
issues>  4  Review popup: ^E writes to fzf's pipe...
         3  Review popup diffs the pane cwd...
↵ send to agent pane   ^N comment   ^X close   ^O browser   ^W worktree   ^R refresh   esc
```

- `↵` hands the issue to the pane the popup was opened from, through the existing `deliver()` bracketed paste. That is the one thing a browser cannot do.
- `^N` reads one line in the popup's own tty (`read -r`), then `gh issue comment N --body-file -`. This is the findings-go-on-the-issue rule, one keypress.
- `^X` asks y/N, then `gh issue close N`, same confirm shape as `kill_window` at `scripts/tmux-review:1877`.
- `^O` runs `gh issue view N -w`. Escape hatch, never the default.
- `^W` is the worktree tie-in: `gh issue develop N -n <branch>` then `git worktree add ~/.cache/cockpit-wt/<branch> <branch>`, then `tmux new-window -c` there. This is the owner's existing rule made into a key.
- `^R` reloads the list, because nothing is cached.

Every key has a caller. Nothing here needs a second screen, a form, or a state file.

## 4. Bad patterns, what not to do

1. Never call `gh` on the status redraw path. README.md:79 is explicit: "A status redraw runs no shell at all." An issue count in the status bar is a network call per redraw. If it is ever wanted, it goes through the precompute pattern, an option written by an agent hook, never a fork on redraw.
2. Do not wrap every `gh issue` subcommand in a script. `docs/superpowers/research/2026-08-24-review-antipatterns.md:31` already says it: "Don't build a second GitHub: durable tools are the thin ones." `gh issue edit --add-label` typed by hand is fine forever.
3. Do not build a TUI. fzf plus `gh issue view --comments` in a preview window is the whole product.
4. Do not let an agent create without searching. `gh issue list --state all --search "<symptom>"` first, verified working above. Duplicates are the fastest way to make the list useless.
5. Do not paste transcripts into bodies. #3 and #4 are 20 to 30 lines with headings. A 500-line body is unreadable in a preview pane and unreadable on the web.
6. No issue without a decision or a repro. #3's "## To decide" section is the reason it exists. An issue that says only "this is slow" is a note, not an issue.
7. No invented labels. Ten labels exist today and neither open issue uses one. Adding `perf-popup-fzf-render` per issue makes the label filter noise.
8. Never `--web` by default. The whole point is not opening a browser. `-w` behind a key, on purpose.
9. Never put a token in a script or an env line in `cockpit.tmux`. gh reads `~/.config/gh/hosts.yml` itself. Nothing in the repo should ever see `GH_TOKEN`.
10. Never run a non-interactive `create`/`comment` without `--body` or `--body-file`. gh refuses rather than hangs, verified, but glab has a live bug here (#1121) so the rule has to survive a future glab arm.
11. Do not run `gh` outside a tty and then try to prompt. `display-popup -E` gives a real tty, so `read -r` works, but a `run-shell` binding does not. Every interactive key belongs in the popup.
12. No secrets or absolute home paths in bodies. `~/.cache/cockpit-wt/<branch>` is fine, `/home/<user>/...` in a public repo is noise.
13. Do not cache the issue list to disk. It goes stale, it needs invalidation, and `gh issue list` takes well under a second. `^R` is the cache.

## 5. Good patterns

- `.github/ISSUE_TEMPLATE/bug.md` with the four headings #3 and #4 already use: Symptom, Cause, To decide, Scope / Not doing. `gh issue create -T bug` picks it up; the agent gets the shape for free.
- A fixed label set. Ponytail: add nothing yet. `bug`, `enhancement`, `question` and `documentation` already cover both open issues. If the picker's label filter ever has nothing to filter, add exactly one, `research`, for the "findings only, no fix yet" case. Do not add `decision` until a second issue needs it.
- One-line titles that name the surface and the symptom, the style already in use: "Review popup diffs the pane cwd, not the tree the agent works in (worktrees)".
- Body from a file the agent wrote: `gh issue create --body-file docs/.../research.md`, or `--body-file -` from a heredoc. Never `--body "$(cat ...)"`, which loses the exit status and mangles quoting.
- `gh issue view N --comments` straight into the preview window. It is already colorized and paginated by fzf; `| bat --style=plain --language=md` only if a plain pipe reads badly.
- Search before create, always: `gh issue list --state all --search "<terms>"`.
- `--json <fields> --jq '@tsv'` as the picker feed, so the field order is fixed and fzf's `--delimiter=$'\t' --with-nth=` does the column work, exactly as `fleet_queue` does at `scripts/tmux-review:1975`.
- Cache nothing. Refresh on a key.

## 6. What we can reuse today

| Need | Where it already lives |
|---|---|
| Popup geometry and title chrome | `cockpit.tmux:204-205` (`PW`/`PH` from `@cockpit_popup_width`/`_height`), binding shape at `:235` (`display-popup -E -w "$PW" -h "$PH" -T ' ... ' "$Q_X -p '#{pane_id}'"`) |
| Passing the originating pane into the popup | `cockpit.tmux:255-261`, the comment explaining why `-p '#{pane_id}'` is required and `$TMUX_PANE` is wrong inside a popup |
| Key binding with a rebind option and `off` | `cockpit.tmux:267-273` (`@cockpit_review_key`), `:278-284` (`@cockpit_info_key`). Copy verbatim for `@cockpit_issue_key` |
| fzf picker structure: TSV rows, `--with-nth`, `--expect`, sized `--preview-window`, two-line `--header` | `fleet_queue()` at `scripts/tmux-review:1896`, the fzf call at `:1975-1985`, the `psz` sizing at `:1934-1937` |
| The `--expect` result split (key line, then row) | `scripts/tmux-review:1990-1994` |
| A y/N confirm inside a popup | `kill_window()` confirm at `scripts/tmux-review:1877`, and the fzf-based confirms at `:1057` and `:1149` |
| Sending text to an agent pane, typed but unsent | `deliver()` at `scripts/tmux-review:1276`; `tmux load-buffer -b "$bufname" -` at `:1357`, `paste-buffer -p -r -d -b ... -t $pane` at `:1431`, including the busy-pane spool and the agent-pane guard at `:1287-1296` |
| Repo root from a pane | `git_root()` at `scripts/tmux-review:57` |
| Which branches are held by a worktree | `scripts/tmux-branch:169-181` (`git worktree list --porcelain`). Note tmux-branch only reads worktrees, it never creates one, so `^W` adds the first `git worktree add` in the repo |
| Shell-quoting into a tmux command string | `shq()` at `scripts/tmux-review:40` |
| A skill that fences an agent to one command | `skills/cockpit-review/SKILL.md`, `allowed-tools: Bash(*tmux-review*--request*)`. The same fence works for `Bash(gh issue *)` |
| Symlinking a new script | `install.sh:23` `COMMANDS=(tm tmux-branch tmux-open-path)`, loop at `:112` |
| Test blocks | `tests/run.sh`, blocks are `echo "== title =="`; `tests/changed.sh:53` derives dependencies by grepping block bodies for `scripts/<name>` |

Nothing in the repo touches issues today: no `.github/`, no repo `CLAUDE.md`,
no `.claude/`, and `grep -rn 'gh issue' ~/.claude/skills ~/.claude/hooks` finds
nothing. The "issue before fix" rule lives only in the owner's memory file.

## 7. Plan for later

### Ponytail audit

| Asked for | Verdict | Why |
|---|---|---|
| Open an issue from tmux | skip | The agent writes issues, and it already has Bash. A popup that asks a human to type an issue body into a `read -r` is worse than the editor they already have. The gap is the template and the rule, not a script |
| Review issues from tmux | keep | This is the real ask. `gh issue list` piped to fzf with a `gh issue view --comments` preview |
| Comment from tmux | keep | One `read -r` plus `gh issue comment N --body-file -`. Ten lines |
| Close from tmux | keep | Two lines behind a y/N |
| Open in browser | keep | `gh issue view N -w`, one line, escape hatch |
| Start a worktree from an issue | keep | `gh issue develop` plus `git worktree add`, ties the existing `~/.cache/cockpit-wt` rule to a key |
| Send an issue to an agent pane | keep | The only capability a browser does not have. Reuses `deliver()` |
| GitLab support | skip | glab is not installed, no GitLab remote exists, and glab has two open bugs in exactly the flags a script needs |
| Provider abstraction layer | skip | One implementation, one host. Section 2 |
| Issue count in the status bar | skip | Network call on the redraw path. README.md:79 |
| Caching the issue list | skip | `^R` is the cache |
| New labels | replace | Ship with the ten that exist; add `research` only when the filter has something to filter |
| A separate `tmux-issue --new` create mode | skip | `gh issue create --body-file` is already the command |

### Tasks

Each task lands with one failing check first, in `tests/run.sh` as a new
`echo "== ... =="` block unless noted. Suggested branch `feat/issue-flow`, in a
worktree under `~/.cache/cockpit-wt/issue-flow`, never in the main checkout.

1. **Issue template.** Add `.github/ISSUE_TEMPLATE/bug.md` with the Symptom / Cause / To decide / Scope headings from #3 and #4.
   Fail-first: `test -f .github/ISSUE_TEMPLATE/bug.md && grep -q '## Symptom' .github/ISSUE_TEMPLATE/bug.md`.
2. **The agent rule.** One paragraph in README (and a repo `CLAUDE.md` if the owner wants one): search first, body from a file, `--body-file`, never `--body "$(...)"`, fixed labels, findings as comments.
   Fail-first: `grep -q 'body-file' README.md`.
3. **`scripts/tmux-issue` rows.** `tmux-issue --rows` prints `number \t labels \t title` from `gh issue list --json ... --jq '@tsv'`. No tmux, no fzf, testable with a stubbed `gh` on PATH.
   Fail-first: a block that puts a fake `gh` in PATH printing two TSV rows and asserts `tmux-issue --rows` reproduces both, tab-delimited.
4. **The picker.** `tmux-issue [-p pane]` runs fzf over those rows with `--delimiter=$'\t' --with-nth=1,2,3`, a `gh issue view {1} --comments` preview, and a two-line header. Copy the sizing and `--expect` handling from `fleet_queue`.
   Fail-first: `command -v fzf` guard message when fzf is absent, asserted the same way `scripts/tmux-review:950` is tested.
5. **`^N` comment.** `read -r` in the popup tty, then `gh issue comment N --body-file -`. An empty line is a no-op, not an empty comment.
   Fail-first: stub `gh`, feed an empty line, assert nothing is invoked; feed text, assert the stub saw `comment N --body-file -`.
6. **`^X` close.** y/N, then `gh issue close N`. Anything but `y`/`Y` does nothing.
   Fail-first: stub `gh`, answer `n`, assert no `close` call.
7. **`^O` browser and `^R` refresh.** `gh issue view N -w`; refresh re-runs `--rows`.
   Fail-first: stub `gh`, assert `view N -w` reaches the stub.
8. **`↵` send to pane.** Reuse `deliver()`'s guard and paste: refuse a non-agent pane, `load-buffer` plus `paste-buffer -p -r -d`, unsent.
   Fail-first: a non-agent pane gets the refusal message, matching the existing `deliver()` block.
9. **`^W` worktree.** `gh issue develop N -n issue-N-<slug>`, then `git worktree add ~/.cache/cockpit-wt/<branch> <branch>`, then `tmux new-window -c` there. Refuse if the path already exists rather than clobbering it.
   Fail-first: existing directory produces a named refusal and no `git worktree add`.
10. **Binding and install.** `prefix g` in `cockpit.tmux` with `@cockpit_issue_key` and `off`, copying the `@cockpit_review_key` block at `:267`; add `tmux-issue` to `COMMANDS` in `install.sh:23`.
    Fail-first: the shipped-config render block asserts `bind-key g` appears; `install.sh --dry` lists the new link. `prefix g` is unbound in stock tmux and free here (taken: `b i J r R S`).
11. **README and gate.** Key table row, the anti-patterns paragraph, `make check-par` green, `shellcheck` clean (`SOURCES` in the Makefile picks up `scripts/*` automatically).

### Not in the MVP

No GitLab, no provider detection, no issue creation from the popup, no label
editing, no assignees or milestones, no PR support, no status-bar count, no
cache, no multi-repo view, no new labels, no offline mode, no `gh` extension.
