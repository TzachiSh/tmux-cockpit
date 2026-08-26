# Issue flow from tmux: research and plan (2026-08-26)

Owner's ask: bugs and feature requests start as a GitHub (or GitLab) issue the
agent opens; the human wants to open, read, comment on and close that issue
from tmux instead of the browser. What the CLIs give us, what flow to build,
what not to do, what already exists, and a plan for later.

Verified locally on this machine unless marked `[docs]`. gh 2.98.0, fzf 0.44.1,
bat 0.24.0, jq, tmux 3.7c. `glab` is not installed system-wide; the first pass
below (sections 1 to 8) read glab from docs only. Revised the same day after the
owner said GitLab repos are in scope: section 9 verifies glab 1.115.0 against a
real binary in a scratch dir, and sections 2, 3, 7 are patched to match. Where
the first pass said "skip GitLab", section 9 wins. Researched by two Opus
agents, checked and filed by the session lead.

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

Revised. The owner has repos on GitHub and GitLab (gitlab.com and possibly
self-hosted), so the script picks its CLI from the one fact that is free,
local and already true: the origin remote.

```sh
# github.com goes to gh, everything else to glab, and glab's own "not a known
# GitLab host" error is the message for anything that is neither.
issue_cli() {
    case "$(git -C "${1:-.}" remote get-url origin 2>/dev/null)" in
        *github.com[:/]*) printf 'gh\n' ;;
        '') printf 'tmux-issue: no origin remote in %s\n' "${1:-.}" >&2; return 1 ;;
        *)  printf 'glab\n' ;;
    esac
}
```

Verified on real repos: `git@gitlab.com:group/sub/repo.git`,
`https://gitlab.com/group/repo.git`, `https://git.corp.example/group/repo.git`
and `ssh://git@git.corp:2222/x/y.git` go to glab; `git@github.com:o/r.git` and
`ssh://git@github.com/o/r.git` go to gh. The `[:/]` class covers scp-style,
https and `ssh://` with one arm. A GitHub Enterprise host would land on glab
and get a named refusal; when one exists the fix is one more `case` arm.

Self-hosted GitLab cannot be told from a hostname, so nothing tries. glab
refuses with `None of the git remotes configured for this repository point to
a known GitLab host ... Configured remotes: git.corp.example.` and gh with
`none of the git remotes configured for this repository point to a known
GitHub host`. Both exit 1. Nothing we could write is better.

Rejected: a `@cockpit_issue_cli` tmux option (config for a value the remote
already knows), `glab auth status --hostname H` (a `GET /api/v4/user` on every
popup open), and parsing `~/.config/glab-cli/config.yml` (glab writes a
`hosts: gitlab.com:` block on first run with no token, so presence means
nothing, and it is YAML).

Missing CLI: `command -v "$CLI" >/dev/null 2>&1 || { printf 'tmux-issue: this
repo needs %s\n' "$CLI" >&2; exit 1; }`, one guard for both, same shape as the
`command -v fzf` guard at `scripts/tmux-review:950`.

Nothing in the repo parses remotes today (`scripts/tmux-branch:107
switch_remote()` and tmux-review's `origin/HEAD` base are the only touches).

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

Inside is one fzf list, filtered as you type, with `$CLI issue view {1}
--comments` as the preview (same flags on gh and glab), the exact shape of the fleet queue at
`scripts/tmux-review:1975`:

```
issues>  4  Review popup: ^E writes to fzf's pipe...
         3  Review popup diffs the pane cwd...
↵ send to agent pane   ^N comment   ^X close   ^O browser   ^W worktree   ^R refresh   esc
```

- `↵` hands the issue to the pane the popup was opened from, through the existing `deliver()` bracketed paste. That is the one thing a browser cannot do.
- `^N` reads one line in the popup's own tty (`read -r`), then `gh issue comment N --body-file -` or `glab issue note N -m "$msg"` (glab notes take `-m` only, no stdin). This is the findings-go-on-the-issue rule, one keypress.
- `^X` asks y/N, then `$CLI issue close N`, same confirm shape as `kill_window` at `scripts/tmux-review:1877`.
- `^O` runs `$CLI issue view N -w`. Escape hatch, never the default.
- `^W` is the worktree tie-in: `git worktree add -b issue-N-<slug> ~/.cache/cockpit-wt/issue-N-<slug> origin/HEAD`, then `tmux new-window -c` there. No server-side link on either provider (section 9.5). This is the owner's existing rule made into a key.
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
10. Never run a non-interactive `create`/`comment` without `--body` or `--body-file`. Both refuse rather than hang, verified on gh 2.98.0 and glab 1.115.0 (section 9.6).
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
| Start a worktree from an issue | keep | `git worktree add -b` alone, identical on both providers, ties the existing `~/.cache/cockpit-wt` rule to a key. `gh issue develop` skipped (section 9.5) |
| Send an issue to an agent pane | keep | The only capability a browser does not have. Reuses `deliver()` |
| GitLab support | keep | The owner has GitLab repos. Two small `case $CLI` blocks (row feed, ^N) and a `$CLI` variable everywhere else; verified on glab 1.115.0 (section 9) |
| Provider abstraction layer | replace | One 6-line `issue_cli()` `case` on the remote, no config, no probe. Section 2 |
| `@cockpit_issue_cli` override | skip | Config for a value the remote already knows |
| `glab auth status` probe | skip | A network call to learn what the remote string already says |
| Self-hosted host detection | skip | Cannot be told from a hostname; glab's own refusal names the host it saw |
| Issue count in the status bar | skip | Network call on the redraw path. README.md:79 |
| Caching the issue list | skip | `^R` is the cache |
| New labels | replace | Ship with the ten that exist; add `research` only when the filter has something to filter |
| A separate `tmux-issue --new` create mode | skip | `gh issue create --body-file` is already the command |

### Tasks

Each task lands with one failing check first, in `tests/run.sh` as a new
`echo "== ... =="` block unless noted. Suggested branch `feat/issue-flow`, in a
worktree under `~/.cache/cockpit-wt/issue-flow`, never in the main checkout.

1. **Issue templates.** Add `.github/ISSUE_TEMPLATE/bug.md` and `.gitlab/issue_templates/bug.md` with the Symptom / Cause / To decide / Scope headings from #3 and #4.
   Fail-first: both files exist and each has `## Symptom`.
2. **The agent rule.** One paragraph in README (and a repo `CLAUDE.md` if the owner wants one): search first (`gh issue list --state all --search T` / `glab issue list --all --search T`), body from a file, `--body-file` on GitHub or `--description-file` on GitLab, never `--body "$(...)"`, fixed labels, findings as comments.
   Fail-first: `grep -q 'body-file' README.md && grep -q 'description-file' README.md`.
2a. **`issue_cli()` and its stub test.** The function from section 2. Fail-first: a block that creates five throwaway repos with the five remote strings above and asserts the gh/glab mapping, plus the no-remote refusal. No network, no CLI.
2b. **Missing-CLI guard.** `command -v "$CLI"` after detection, one message for both. Fail-first: an empty PATH dir yields `tmux-issue: this repo needs glab`.
3. **`scripts/tmux-issue` rows.** `tmux-issue --rows` prints `number \t labels \t title`, a two-arm `case`: `gh issue list --state all --json number,title,labels --jq '.[]|[.number,(.labels|map(.name)|join(",")),.title]|@tsv'` or `glab issue list --all --output json --jq '.[]|[.iid,(.labels|join(",")),.title]|@tsv'`. No tmux, no fzf.
   Fail-first: stub both `gh` and `glab` on PATH (the `FZFSTUB`/`NOCOL` shape in `tests/run.sh`) and assert the same TSV comes out either way.
4. **The picker.** `tmux-issue [-p pane]` runs fzf over those rows with `--delimiter=$'\t' --with-nth=1,2,3`, a `gh issue view {1} --comments` preview, and a two-line header. Copy the sizing and `--expect` handling from `fleet_queue`.
   Fail-first: `command -v fzf` guard message when fzf is absent, asserted the same way `scripts/tmux-review:950` is tested.
5. **`^N` comment.** `read -r` in the popup tty, then a two-arm `case`: `gh issue comment N --body-file -` or `glab issue note N -m "$msg"`. An empty line is a no-op, not an empty comment.
   Fail-first: stub both, feed an empty line, assert nothing is invoked; feed text, assert gh saw `comment N --body-file -` and glab saw `note N -m`.
6. **`^X` close.** y/N, then `$CLI issue close N`. Anything but `y`/`Y` does nothing.
   Fail-first: stub `gh`, answer `n`, assert no `close` call.
7. **`^O` browser and `^R` refresh.** `$CLI issue view N -w`; refresh re-runs `--rows`.
   Fail-first: stub `gh`, assert `view N -w` reaches the stub.
8. **`↵` send to pane.** Reuse `deliver()`'s guard and paste: refuse a non-agent pane, `load-buffer` plus `paste-buffer -p -r -d`, unsent.
   Fail-first: a non-agent pane gets the refusal message, matching the existing `deliver()` block.
9. **`^W` worktree.** `git worktree add -b issue-N-<slug> ~/.cache/cockpit-wt/issue-N-<slug> origin/HEAD`, then `tmux new-window -c` there. No provider branch. Refuse if the path already exists rather than clobbering it.
   Fail-first: existing directory produces a named refusal and no `git worktree add`.
10. **Binding and install.** `prefix g` in `cockpit.tmux` with `@cockpit_issue_key` and `off`, copying the `@cockpit_review_key` block at `:267`; add `tmux-issue` to `COMMANDS` in `install.sh:23`.
    Fail-first: the shipped-config render block asserts `bind-key g` appears; `install.sh --dry` lists the new link. `prefix g` is unbound in stock tmux and free here (taken: `b i J r R S`).
11. **README and gate.** Key table row, which CLI is picked and the one-time `glab auth login --hostname H` for a self-hosted instance, the anti-patterns paragraph, `make check-par` green, `shellcheck` clean (`SOURCES` in the Makefile picks up `scripts/*` automatically).

### Not in the MVP

No issue creation from the popup, no label editing, no assignees or
milestones, no PR/MR support, no server-side branch link (`gh issue develop`,
`glab mr create --related-issue`), no GitHub Enterprise arm until a host
exists, no `@cockpit_issue_cli` override, no status-bar count, no cache, no
multi-repo view, no new labels, no offline mode, no `gh` extension.

## 9. glab verified (same day revision)

Verified against glab 1.115.0 downloaded from the GitLab package registry into
a scratch dir, never installed. Read paths against public projects worked
without a token, so several `[docs]` items in section 1 are now verified.
Anything needing a token is marked `[docs]`. Running glab once wrote
`~/.config/glab-cli/config.yml`; it was deleted afterwards.

### 9.1 Flags, from the real `--help`

`glab issue` has: board, close, create, delete, list, note, reopen, subscribe,
unsubscribe, update, view.

| Flow step | glab 1.115.0 | gh 2.98.0 | Branch needed |
|---|---|---|---|
| create | `-t --title`, `-d --description`, `--description-file` (`-` = stdin), `-l --label`, `--template` (from `.gitlab/issue_templates/`), `--no-editor`, `-y --yes`, `--web`, `-a`, `-m`, `-c --confidential` | `-t`, `-b`, `-F --body-file`, `-l`, `-T --template`, `-e --editor`, `-w` | yes (agent side only) |
| list | `-A --all`, `-c --closed`, `-l --label`, `--search`, `--in title,description`, `--output json` (short `-O`), `--jq`, `-P --per-page` (30), `-p --page`, `--order`, `-s --sort` | `-s open\|closed\|all`, `-l`, `-S`, `-L`, `--json f1,f2`, `--jq` | yes |
| view | `-c --comments`, `--output json` (short `-F`), `--jq`, `-w --web`, `-P --per-page`, `-s --system-logs` | `-c`, `--json`, `-w` | no |
| note | `-m --message` only | `gh issue comment -b`, `-F` (`-` = stdin), `-e`, `--edit-last` | yes |
| close | id or URL, no other flags | `-c COMMENT`, `-r REASON` | no |
| update | `-l`, `-u --unlabel`, `-t`, `-d`, `--description-file`, `-a`, `-m`, `--lock-discussion` | `gh issue edit --add-label --remove-label` | out of scope |
| branch from issue | none; nearest `glab mr create -i --related-issue N --create-source-branch` | `gh issue develop N -n NAME -b BASE` | dropped, 9.5 |
| api | `glab api EP -X -f -F -H --input --paginate --output json\|ndjson --hostname` | `gh api` | out of scope |

Flag traps a shared script must respect:

- `-e` on `glab issue create` is `--time-estimate`; on gh it is the editor. Never pass a bare `-e` from shared code.
- `--output` is safe on both subcommands but the short letters differ (`-O` on list, `-F` on view), and `-F` on `list` is `--output-format details|ids|urls`, which silently prints the human table. Verified: `glab issue list -F json` printed `Showing 1 open issue...`. Always write `--output json`.
- glab `state` is `opened`/`closed`; gh's `--json state` is `OPEN`/`CLOSED`.
- glab `list` has no `--state`. Default is open, `-A` is open plus closed, `-c` closed only. Verified on a public project: `-A` returned both, default only `opened`.
- Both print an error and exit 1 on a wrong host, fine for a `|| { printf ...; exit 1; }` guard.
- glab posts telemetry (`telemetry: true` in config.yml). Not the script's business; the owner may want it off.

### 9.2 Row feed parity

Both CLIs have a built-in `--jq`, so the jq binary is not needed for either arm.

```sh
gh   issue list --state all --json number,title,labels --jq '.[]|[.number,(.labels|map(.name)|join(",")),.title]|@tsv'
glab issue list --all --output json            --jq '.[]|[.iid,(.labels|join(",")),.title]|@tsv'
```

Verified live against `gitlab-org/cli` with no token; two TSV rows came back.
`glab issue list --output json` is the raw REST array: the number is `iid`
(`id` is the global row id, useless in a URL), `labels` is an array of plain
strings, and `web_url`, `state`, `updated_at` are there. `glab issue view
--output json` returns the same single object.

### 9.3 Per key, side by side

`CLI` is the string from `issue_cli()`, `n` is field 1 of the selected row.

| Key | gh | glab | Shared? |
|---|---|---|---|
| rows | `gh issue list --state all --json ... --jq ...` | `glab issue list --all --output json --jq ...` | branch 1 |
| preview | `gh issue view {1} --comments` | `glab issue view {1} --comments` | `$CLI issue view {1} --comments` |
| ^N comment | `gh issue comment "$n" --body-file -` | `glab issue note "$n" -m "$msg"` | branch 2 |
| ^X close | `gh issue close "$n"` | `glab issue close "$n"` | shared |
| ^O browser | `gh issue view "$n" -w` | `glab issue view "$n" -w` | shared |
| ^W worktree | `git worktree add -b ...` | same | shared |
| ↵ send to pane | no CLI call, `deliver()` gets number and title from the row | same | shared |
| ^R refresh | re-runs rows | same | shared |

Two `case $CLI` blocks, a couple of lines each. That is the whole cost of the
second provider.

### 9.4 Detection

See section 2. Also verified: `GITLAB_HOST` does not override repo resolution.
In a repo whose origin is `https://git.corp.example/group/repo.git`,
`GITLAB_HOST=gitlab.com glab issue list` still refused with `None of the git
remotes ... point to a known GitLab host`. The old note that `glab auth status`
ignores `GITLAB_HOST` (gitlab-org/cli#8065) is stale: closed 2025-12-03.

### 9.5 The ^W worktree gap

`gh issue develop N -n branch` creates a branch on the server and links it to
the issue. glab's nearest thing is `glab mr create --related-issue N
--create-source-branch`, which also opens a merge request. A draft MR every
time a human presses a key to start looking at something is the wrong
operation. Verdict: drop the server-side link on both sides.

```sh
git worktree add -b "issue-$n-$slug" "$HOME/.cache/cockpit-wt/issue-$n-$slug" origin/HEAD
```

One command, branch and worktree together, identical on both providers, no
network, no provider branch, matches the owner's rule. The branch name carries
the issue number anyway. Add `gh issue develop` back only if the owner wants
GitHub's "linked branches" panel populated; that is a one-arm branch, not a
redesign.

### 9.6 Agent-side create on GitLab

Verified fully non-interactive with a real tty and `.gitlab/issue_templates/bug.md` present:

```sh
glab issue create -t "one line title" --description-file docs/.../notes.md -l bug -y
```

No template prompt, no description prompt. `-t` with no description refuses
with `'--Title' and '--description' (or '--template') required for
non-interactive mode` and creates nothing, the same anti-hang guarantee gh
gives. `--description-file -` reads stdin. `--no-editor` is not needed once a
description is supplied; keep `-y` as cheap insurance for older builds.

gitlab-org/cli#1121 is still open (2022-11-25) but now labelled
`documentation`, `UI text`, `type::maintenance`. It is a wording problem, not a
functional one; 1.115.0 behaved correctly in every probe. The section 1 verdict
"glab create is not reliably non-interactive" is stale.

### 9.7 Self-hosted

Once per host, by the owner, by hand: `glab auth login --hostname
git.corp.example`. Token goes to the OS keyring, or to
`~/.config/glab-cli/config.yml` under `hosts: <host>:` with
`--insecure-storage` (`api_protocol`, `api_host`, `subfolder`, `ssh_host`,
`token`). An instance whose API is on another host or port takes `--api-host
host:port --api-protocol https`. The script does nothing: no `GITLAB_HOST`, no
token, no host list, the same way the repo never sees `GH_TOKEN`.
