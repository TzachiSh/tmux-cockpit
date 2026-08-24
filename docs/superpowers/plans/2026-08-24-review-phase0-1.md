# Review Redesign — Phase 0 (bugs) and Phase 1 (perf) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Read the master plan `2026-08-24-review-redesign-goal.md` §Global Constraints, §2 (the rig) and §3 (what not to do) first — they bind every task here.

**Goal:** make today's `prefix r` reviewer correct (fourteen reproduced defects) and fast (one comment in < 60 ms, a 4900-row open in < 200 ms, a lockfile "too large" in < 1.5 s) without changing its architecture, so the owner can use it while phases 2–4 are decided.

**Architecture:** every change is local to `scripts/tmux-review` (lane A, serialized), `scripts/tmux-open-path` (lane B), `scripts/agent-state` + `scripts/tmux-jump` (lane C), `cockpit.tmux` + `README.md` (lane D). Wire formats stay tab-separated; one column is added (a preview offset, field 5). The store grammar is untouched. No new options except `@cockpit_review` (per pane) and `@cockpit_fleet_fzf_options` (global).

**Tech Stack:** bash 4+, tmux 3.7c here (3.3 floor), fzf 0.44.1 (no `transform`, no `--tmux`, no `--wrap`), git, POSIX awk (mawk here), `tests/run.sh` harness (`is`/`ok`/`bad`, `mktmpd`, private `-L` sockets, `timeout` teeth).

**Spec:** `docs/superpowers/research/2026-08-24-review-redesign.md` §3 (defects), §5 (perf), §7.5 (picker), and the lens reports `ui.md`, `ux.md`, `dev.md`, `perf.md`, `ai.md` under `docs/superpowers/research/2026-08-24-review-lenses/`. Prototypes under `…/review-lenses/prototypes/`. One-page artifact of the synthesis + master plan: https://claude.ai/code/artifact/8682a108-927b-4fa3-aeb2-a2f6e100a84c

## Global Constraints

- All of the master plan's Global Constraints, verbatim.
- Line numbers below are as of master `abed845` (`scripts/tmux-review` is 743 lines; anchors: `lines_mode` :77, `cap=5000` :112, `rows_mode` :280, `attach` :319, `run_loop` :354, `enter:ignore` :401, `compose` :436, `deliver` :471, `fleet_rows` :573, `review_pane` :630, `fleet_queue` :652, `main_entry` :705). **Re-locate by `grep -n` before editing** — the file is shared with another session's Wave D lane.
- Test blocks go into `tests/run.sh` next to the existing `== tmux-review … ==` blocks (`:2432` onwards); fixtures via `mktmpd`; agent panes via `exec -a claude bash`; `@cockpit_agents_glob` set explicitly on each private socket; every server appended to `SOCKS`.
- Worktree: `git worktree add ~/.cache/cockpit-wt/review-p0 -b feat/review-p0 master` — every task's commit lands there. Merge to master only after the phase gate.
- Run the gate as `make check` (unpiped). A block can be run alone during development with `bash -c 'source <(sed -n "1,/^echo \"== syntax/p" tests/run.sh | head -n -1); …'` but the merge gate is the whole suite.

---

## Phase 0 — bugs

### Task 1: `deliver()` refuses a pane at `needs-input`

**Files:**
- Modify: `scripts/tmux-review:471-497` (early guards) and `:556-561` (the `if-shell -F` re-gate)
- Test: `tests/run.sh` — new block after `== tmux-review --deliver: guards refuse before anything is pasted ==` (`:3016`)

**Interfaces:**
- Consumes: `@panestate` per pane, written only by `scripts/agent-state`.
- Produces: a third guard. Exit 1 + `display-message "tmux-review: %N is waiting on a permission prompt — answer it first"` when `@panestate` is `needs-input`; the paste-time format re-checks the same condition.

- [ ] **Step 1: Write the failing test** — inside the existing deliver block, after the `rundeliver "$AP" "$RDSTORE"` success assertions and before `kill-server`:

```bash
echo "== tmux-review --deliver: a pane on a permission prompt is refused, not pasted into =="
"$TMUX_REAL" -L "$RDS" send-keys -t "$AP" C-u >/dev/null 2>&1      # clear the earlier paste
"$TMUX_REAL" -L "$RDS" set-option -p -t "$AP" @panestate needs-input >/dev/null 2>&1
printf '## src.go:1 (+)\n> foo\nNI-MARKER fix\n\n' > "$RDSTORE"
rundeliver "$AP" "$RDSTORE"
is "delivery into needs-input exits 1" "RC=1" "$(lastrc)"
case "$(apcap)" in
    *"NI-MARKER"*) bad "nothing is pasted into a needs-input pane" "no NI-MARKER on the agent pane" "$(apcap)" ;;
    *) ok "nothing is pasted into a needs-input pane" ;;
esac
"$TMUX_REAL" -L "$RDS" set-option -p -t "$AP" @panestate done >/dev/null 2>&1
rundeliver "$AP" "$RDSTORE"
case "$(apcap)" in
    *"NI-MARKER"*) ok "the same review lands once the pane is done" ;;
    *) bad "the same review lands once the pane is done" "NI-MARKER on the agent pane" "$(apcap)" ;;
esac
"$TMUX_REAL" -L "$RDS" set-option -pu -t "$AP" @panestate >/dev/null 2>&1
```

- [ ] **Step 2: Run to verify it fails** — `make test 2>&1 | grep -A3 'permission prompt is refused'` → expect `FAIL delivery into needs-input exits 1` (got `RC=0`) and `FAIL nothing is pasted`.

- [ ] **Step 3: Implement** — in `deliver()`, after the `is_agent` guard and before the store guard:

```bash
    # Guard 3: a pane sitting on a permission prompt. The paste would land in
    # the dialog and the human's Enter -- the one this script tells them to
    # press -- becomes a blind approval of whatever tool was being asked about.
    # @panestate is written only by agent-state, so this is the cheap read.
    local pstate
    pstate=$(tmux show-option -pqv -t "$pane" @panestate 2>/dev/null)
    if [ "$pstate" = needs-input ]; then
        tmux display-message \
            "tmux-review: $pane is waiting on a permission prompt — answer it first" \
            2>/dev/null
        return 1
    fi
```

and change the `if-shell -F` format so the same test is re-evaluated at paste time:

```bash
    tmux if-shell -F -t "$pane" \
        "#{&&:#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},#{!=:#{@panestate},needs-input}}" \
        "paste-buffer -p -r -d -b $bufname -t $pane ; display-message 'review pasted into $pane — press Enter there to send'" \
        "delete-buffer -b $bufname ; display-message 'tmux-review: $pane is not an idle agent — not delivered'"
```

- [ ] **Step 4: Run to verify it passes** — the three new `is`/`ok` lines PASS; the existing deliver assertions still PASS.
- [ ] **Step 5: shellcheck** — `shellcheck -x scripts/tmux-review` clean.
- [ ] **Step 6: Commit** — `git commit -am "fix: a review is never pasted into a pane waiting on a permission prompt"`

### Task 2: review everything uncommitted — staged, unstaged, and untracked — from one base

**Files:**
- Modify: `scripts/tmux-review:88-98` (the two git calls in `lines_mode`), `:596-597` (fleet numstat), plus a new helper above `lines_mode`
- Modify: `scripts/tmux-open-path:75-100` (`--preview-diff` base)
- Test: `tests/run.sh` — two new `--lines` blocks and one `--preview-diff` block

**Interfaces:**
- Produces: `tree_diff <dir> [git-diff args…]` — prints `git diff HEAD "$@"` when HEAD exists, else today's unstaged-then-staged fallback. Used by `lines_mode`, `fleet_rows`, and (copied, not sourced — the scripts are one-shot) `tmux-open-path`.
- Produces: untracked files appear in `--lines` as `+` rows, one per line, path from `git ls-files --others --exclude-standard`.

- [ ] **Step 1: Write the failing tests**

```bash
echo "== tmux-review --lines: staged and unstaged changes are one review, not either/or =="
RVMIX=$(mktmpd)
(
  cd "$RVMIX" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\n' > staged.txt; printf 'a\n' > unstaged.txt
  git add -A; git commit -q -m init
  printf 'a\nSTAGED-CHANGE\n' > staged.txt; git add staged.txt
  printf 'a\nUNSTAGED-CHANGE\n' > unstaged.txt
) >/dev/null 2>&1
rvmix=$("$DIR/scripts/tmux-review" --lines "$RVMIX")
case "$rvmix" in *"staged.txt"$'\t'2$'\t'"STAGED-CHANGE"*) ok "the staged half is reviewable" ;;
    *) bad "the staged half is reviewable" "a staged.txt:2 STAGED-CHANGE row" "$rvmix" ;; esac
case "$rvmix" in *"unstaged.txt"$'\t'2$'\t'"UNSTAGED-CHANGE"*) ok "and so is the unstaged half" ;;
    *) bad "and so is the unstaged half" "an unstaged.txt:2 row" "$rvmix" ;; esac
case "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVMIX/staged.txt")" in
    *STAGED-CHANGE*) ok "the preview agrees with the list about the staged file" ;;
    *) bad "the preview agrees with the list about the staged file" "STAGED-CHANGE in the preview" \
         "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVMIX/staged.txt")" ;; esac
rm -rf "$RVMIX"

echo "== tmux-review --lines: a brand-new file is reviewable, line by line =="
RVNEW=$(mktmpd)
(
  cd "$RVNEW" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'seed\n' > seed.txt; git add -A; git commit -q -m init
  printf 'first\nsecond\n' > new.txt
) >/dev/null 2>&1
rvnew=$("$DIR/scripts/tmux-review" --lines "$RVNEW")
case "$rvnew" in *"new.txt"$'\t'1$'\t'"first"*"new.txt"$'\t'2$'\t'"second"*) ok "untracked lines are + rows with their own numbers" ;;
    *) bad "untracked lines are + rows with their own numbers" "new.txt:1 first, new.txt:2 second" "$rvnew" ;; esac
rm -rf "$RVNEW"
```

- [ ] **Step 2: Run to verify they fail** — staged half missing; new.txt absent; preview shows it (that one passes already — keep it as the regression tooth).

- [ ] **Step 3: Implement** — add above `lines_mode`:

```bash
# Everything uncommitted is ONE set: `git diff HEAD` is staged + unstaged in a
# single call. The old "unstaged, else staged" fallback dropped the whole
# index the moment one unstaged edit existed -- reproduced: a staged file was
# invisible to the list while the preview showed it. An unborn HEAD (no
# commits yet) has no HEAD to diff against, so that case keeps the fallback.
tree_diff() {
    local dir=$1; shift
    if git -C "$dir" rev-parse -q --verify HEAD >/dev/null 2>&1; then
        git -C "$dir" -c core.quotePath=false diff --no-color --no-ext-diff HEAD "$@" 2>/dev/null
    else
        local out
        out=$(git -C "$dir" -c core.quotePath=false diff --no-color --no-ext-diff "$@" 2>/dev/null)
        [ -n "$out" ] || out=$(git -C "$dir" -c core.quotePath=false diff --no-color --no-ext-diff --cached "$@" 2>/dev/null)
        printf '%s\n' "$out"
    fi
}
```

In `lines_mode` replace the four lines at `:92-98` with:

```bash
    local diff numstat
    diff=$(tree_diff "$dir")
    numstat=$(tree_diff "$dir" --numstat)
    # Untracked files carry no diff at all; synthesise theirs. Binary and
    # unreadable files are skipped (grep -Iq fails on both).
    local untracked u ulines
    untracked=$(git -C "$dir" ls-files --others --exclude-standard 2>/dev/null)
    while IFS= read -r u; do
        [ -n "$u" ] && [ -f "$dir/$u" ] && grep -Iq . -- "$dir/$u" 2>/dev/null || continue
        ulines=$(wc -l < "$dir/$u")
        numstat="${numstat:+$numstat$'\n'}$ulines"$'\t'"0"$'\t'"$u"
    done <<<"$untracked"
    [ -n "$diff$untracked" ] || return 0
```

After the awk that produces `raw` (`:154-195`), append the synthetic rows:

```bash
    while IFS= read -r u; do
        [ -n "$u" ] && [ -f "$dir/$u" ] && grep -Iq . -- "$dir/$u" 2>/dev/null || continue
        raw="${raw:+$raw$'\n'}$(awk -v f="$u" '{ printf "%s\t%d\t+\t%s\n", f, NR, $0 }' -- "$dir/$u")"
    done <<<"$untracked"
```

In `fleet_rows` replace `:596-597` with `numstat=$(tree_diff "$path" --numstat)`.

In `tmux-open-path --preview-diff`, replace the numstat/cached block (`:75-84`) and the two `git diff` renders with `HEAD`-based calls when HEAD exists (same helper body, inlined — the scripts do not source each other), and for an untracked file render `git diff --no-index -- /dev/null "$f"` from its first `@@`.

- [ ] **Step 4: Run to verify it passes** — the three new blocks PASS; the existing `--lines` blocks (`:2432`, `:2470`, `:2630`, `:2835`) still PASS.
- [ ] **Step 5: shellcheck clean; Commit** — `git commit -am "fix: a review covers staged, unstaged and new files as one set instead of whichever git diff answered first"`

### Task 3: blank lines and the too-large notice stop rendering as empty rows

**Files:**
- Modify: `scripts/tmux-review:300-309` (`rows_mode` read loop), `:121-123` (the cap notice), `:325` (`attach` guard)
- Test: `tests/run.sh` — one new `--rows` block

**Interfaces:**
- Produces: `--rows` splits on single tabs without collapsing; a blank fragment is preserved as an empty field 3; the cap notice carries line `0`; `attach` refuses path `(diff too large)`.

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-review --rows: a blank added line is still a visible, anchorable row =="
RVBL=$(mktmpd)
(
  cd "$RVBL" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > f.md; git add -A; git commit -q -m init
  printf 'a\n\nNEW\nb\n' > f.md
) >/dev/null 2>&1
RVBLS=$(mktmpf)
rvbl=$("$DIR/scripts/tmux-review" --rows "$RVBL" "$RVBLS")
rvblbad=""
while IFS= read -r row; do
    [ -n "$row" ] || continue
    rest=${row#*$'\t'}; rest=${rest#*$'\t'}; disp=${rest#*$'\t'}
    case "$disp" in *"f.md:"*) ;; *) rvblbad="display lost its path: [$row]" ;; esac
done <<<"$rvbl"
is "every row's display names its file, blank fragment or not" "" "$rvblbad"
rvblcount=$(printf '%s\n' "$rvbl" | grep -c .)
is "the blank line is a row of its own" "3" "$rvblcount"
rm -rf "$RVBL" "$RVBLS"
```

- [ ] **Step 2: Run to verify it fails** — the blank row's display is ` ` (marker only).

- [ ] **Step 3: Implement** — replace the `while IFS=$'\t' read -r path line fragment display` loop in `rows_mode` with a non-collapsing split:

```bash
    # Tab is IFS whitespace, so `IFS=$'\t' read` COLLAPSES a run of tabs -- an
    # empty fragment (a blank added line) shifted the display into field 3 and
    # the marker became the whole row. Reproduced: 2 of 18 rows rendered blank.
    # Split by hand instead; an empty field stays an empty field.
    local row rest path line fragment display sign num marker
    while IFS= read -r row; do
        [ -n "$row" ] || continue
        path=${row%%$'\t'*};     rest=${row#*$'\t'}
        line=${rest%%$'\t'*};    rest=${rest#*$'\t'}
        fragment=${rest%%$'\t'*}; display=${rest#*$'\t'}
```

(the rest of the loop body is unchanged). Change the cap notice to carry a line: `printf '%s\t%s\t%s\t%s\n' '(diff too large)' 0 "$big" "$big"`. In `attach()` add `[ "$path" != '(diff too large)' ] || return 0` beside the empty-path guard.

- [ ] **Step 4: Run to verify it passes**; the cap block at `:2470` still PASSES (its notice now has a `0` line field — update its `grep -c .` expectation only if it counts fields).
- [ ] **Step 5: shellcheck clean; Commit** — `git commit -am "fix: a blank added line renders as a row, not as an invisible marker"`

### Task 4: `↵` pins the typed comment; `alt-↵` stays as the alias

**Files:**
- Modify: `scripts/tmux-review:396-408` (`run_loop` binds and header)
- Modify: `README.md:493-507` (the key table and the "Plain ↵ does nothing" paragraph)
- Test: `tests/run.sh` — extend the existing interactive loop block (the one that drives `alt-Enter` around `:2700`; locate with `grep -n "M-Enter\|alt-enter" tests/run.sh`)

**Interfaces:**
- Produces: `enter` bound to `execute-silent(attach)+reload(rows)+clear-query`; with an empty query `attach()` already returns 0 — no accept-all path exists.

- [ ] **Step 1: Write the failing test** — after the existing alt-Enter assertion in that block:

```bash
"$TMUX_REAL" -L "$RL" send-keys -t "$LP" "plain enter comment" Enter
for i in 1 2 3 4 5 6 7 8 9 10; do grep -q "plain enter comment" "$RLSTORE" 2>/dev/null && break; sleep 0.1; done
case "$(cat "$RLSTORE" 2>/dev/null)" in
    *"plain enter comment"*) ok "plain Enter pins the typed comment" ;;
    *) bad "plain Enter pins the typed comment" "the comment in the store" "$(cat "$RLSTORE" 2>/dev/null)" ;; esac
rlbefore=$(wc -c < "$RLSTORE")
"$TMUX_REAL" -L "$RL" send-keys -t "$LP" Enter; sleep 0.3
is "Enter with nothing typed pins nothing (no accept-all)" "$rlbefore" "$(wc -c < "$RLSTORE")"
case "$("$TMUX_REAL" -L "$RL" capture-pane -p -t "$LP")" in
    *"review>"*) ok "and the picker is still open" ;;
    *) bad "and the picker is still open" "the review> prompt" "$("$TMUX_REAL" -L "$RL" capture-pane -p -t "$LP")" ;; esac
```

(`RL`, `LP`, `RLSTORE` are that block's existing names — match them.)

- [ ] **Step 2: Run to verify it fails** — the store never receives "plain enter comment".
- [ ] **Step 3: Implement** — replace `--bind 'enter:ignore'` with

```bash
              --bind "enter:execute-silent(${attach_cmd})+reload(${rows_cmd})+clear-query" \
```

keep the `alt-enter` line, and replace the header with two lines (fzf 0.44 accepts a multi-line `--header`):

```bash
              --header-first \
              --header=$'↵ pin the typed comment   ^S deliver   esc discard (asks first)\n^/ preview   alt-u/alt-d scroll   ↵ with nothing typed does nothing' \
```

README: the key row for `↵` becomes "pin the typed comment (empty: nothing)"; keep the no-accept-all rationale paragraph, reworded: Enter pins, it never approves.

- [ ] **Step 4: Run to verify it passes**; the existing alt-Enter assertions still PASS.
- [ ] **Step 5: shellcheck clean; Commit** — `git commit -am "feat: Enter pins the typed comment; the review's only dead key is gone"`

### Task 5: `esc` asks before discarding a non-empty batch

**Files:**
- Modify: `scripts/tmux-review:354-430` (`run_loop` becomes a loop)
- Test: extend the same interactive block

**Interfaces:**
- Produces: on fzf exit 130 with a non-empty store, a two-row confirm picker (`keep reviewing` / `discard N comment(s)`); `keep` re-enters the list; `discard` deletes the store and exits 0. An empty store still exits 0 immediately.

- [ ] **Step 1: Write the failing test** — after Task 4's assertions:

```bash
"$TMUX_REAL" -L "$RL" send-keys -t "$LP" Escape
for i in 1 2 3 4 5 6 7 8 9 10; do "$TMUX_REAL" -L "$RL" capture-pane -p -t "$LP" | grep -q "discard 2 comment" && break; sleep 0.1; done
case "$("$TMUX_REAL" -L "$RL" capture-pane -p -t "$LP")" in
    *"discard 2 comment"*) ok "esc on a non-empty batch asks, naming the count" ;;
    *) bad "esc on a non-empty batch asks, naming the count" "a 'discard 2 comment(s)' choice" "$("$TMUX_REAL" -L "$RL" capture-pane -p -t "$LP")" ;; esac
"$TMUX_REAL" -L "$RL" send-keys -t "$LP" Enter      # first row = keep reviewing
for i in 1 2 3 4 5 6 7 8 9 10; do "$TMUX_REAL" -L "$RL" capture-pane -p -t "$LP" | grep -q "review>" && break; sleep 0.1; done
is "keep reviewing returns to the list with the store intact" "$rlbefore" "$(wc -c < "$RLSTORE")"
"$TMUX_REAL" -L "$RL" send-keys -t "$LP" Escape; sleep 0.3
"$TMUX_REAL" -L "$RL" send-keys -t "$LP" Down Enter
for i in 1 2 3 4 5 6 7 8 9 10; do [ -f "$RLSTORE" ] || break; sleep 0.1; done
is "discard removes the store" "gone" "$([ -f "$RLSTORE" ] && echo present || echo gone)"
```

- [ ] **Step 2: Run to verify it fails** — the first Escape closes the picker and deletes the store.
- [ ] **Step 3: Implement** — wrap the fzf invocation in `run_loop` in `while :; do … done`; replace the `case "$rc"` block with:

```bash
    case "$rc" in
        0|1) ;;
        *)
            # A cancel with pinned comments asks; losing a twenty-comment review
            # to a reflexive esc is the cliff revdiff avoids with a separate
            # discard key. An empty store has nothing to protect.
            if [ -s "$store" ]; then
                local n choice
                n=$(grep -c '^## ' "$store")
                choice=$(printf 'keep reviewing\ndiscard %d comment(s)\n' "$n" \
                    | fzf --reverse --no-info --prompt='discard? ' \
                          --header="$n comment(s) are pinned and not delivered")
                case "$choice" in
                    discard*) rm -f -- "$store"; exit 0 ;;
                    *) continue ;;            # back to the list, store kept
                esac
            fi
            rm -f -- "$store"; exit 0 ;;
    esac
```

and keep the `ctrl-s` → `exit 20` branch inside the loop.

- [ ] **Step 4: Run to verify it passes**.
- [ ] **Step 5: shellcheck clean; Commit** — `git commit -am "fix: esc asks before throwing away pinned comments"`

### Task 6: add/delete colour in the list

**Files:**
- Modify: `scripts/tmux-review` `rows_mode` (the `printf` of the marked row) and `run_loop` (`--ansi`)
- Test: new `--rows` assertion in Task 3's block

**Interfaces:**
- Produces: `--rows` display field wraps `(+)` in `\e[32m…\e[0m` and `(-)` in `\e[31m…\e[0m`; `--lines` stays plain (goldens depend on it).

- [ ] **Step 1: Write the failing test** — in Task 3's block, before `rm -rf`:

```bash
case "$rvbl" in *$'\e[32m(+)'*) ok "an added row carries green SGR in --rows" ;;
    *) bad "an added row carries green SGR in --rows" "ESC[32m(+) in a row" "$(printf '%s' "$rvbl" | cat -v)" ;; esac
case "$("$DIR/scripts/tmux-review" --lines "$RVBL")" in *$'\e['*) bad "--lines stays plain" "no SGR" "sgr found" ;;
    *) ok "--lines stays plain" ;; esac
```

- [ ] **Step 2: Run to verify it fails**.
- [ ] **Step 3: Implement** — in `rows_mode`, before the final `printf`:

```bash
        case "$sign" in
            +) display=${display/ (+) / $'\e[32m'(+)$'\e[0m' } ;;
            -) display=${display/ (-) / $'\e[31m'(-)$'\e[0m' } ;;
        esac
```

and add `--ansi` to the review-loop fzf call (the fleet queue is untouched).

- [ ] **Step 4–5: green; shellcheck; Commit** — `git commit -am "feat: the review list colours + and - the way the preview already does"`

### Task 7: the store quotes the whole line, not its first 160 bytes

**Files:**
- Modify: `scripts/tmux-review:319-343` (`attach` gains an optional 6th arg `dir`), `:363` (`attach_cmd` passes `$dir`)
- Test: new `--attach` block

**Interfaces:**
- Produces: `tmux-review --attach <store> <path> <line> <fragment> <comment> [dir]`. With `dir`, the quoted line is read from the file (`+`) or from `HEAD:path` (`-`), control-stripped, capped at 1000 bytes, and used when it starts with `fragment`; without `dir` (or on any miss) today's behaviour.

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-review --attach: a long line is quoted whole, so the agent can find it =="
RVLONG=$(mktmpd)
LONGLINE=$(printf 'x%.0s' $(seq 1 220))
(
  cd "$RVLONG" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\n' > f.txt; git add -A; git commit -q -m init
  printf 'a\n%s\n' "$LONGLINE" > f.txt
) >/dev/null 2>&1
RVLS=$(mktmpf)
frag=$("$DIR/scripts/tmux-review" --lines "$RVLONG" | awk -F'\t' '$2==2 {print $3}')
is "the display fragment is still capped" "160" "${#frag}"
"$DIR/scripts/tmux-review" --attach "$RVLS" f.txt 2 "$frag" "too long" "$RVLONG"
is "the stored quote is the full line" "> $LONGLINE" "$(sed -n 2p "$RVLS")"
rm -rf "$RVLONG" "$RVLS"
```

- [ ] **Step 2: Run to verify it fails** — the stored quote is 160 x's.
- [ ] **Step 3: Implement** — in `attach()`: `local dir=${6:-}`; after computing `sign`/`num`:

```bash
    # The display fragment is cut at 160 bytes for the list; the STORE gets the
    # whole line so the `>` quote matches something in the file -- a truncated
    # quote defeats the anchoring it exists for. Read it back from the tree
    # (+) or HEAD (-); fall back to the fragment on any miss.
    local full=""
    if [ -n "$dir" ]; then
        case "$sign" in
            +) full=$(sed -n "${num}p" -- "$dir/$path" 2>/dev/null) ;;
            -) full=$(git -C "$dir" show "HEAD:$path" 2>/dev/null | sed -n "${num}p") ;;
        esac
        full=${full//[$'\001'-$'\037'$'\177']/}
        full=${full:0:1000}
        [ -n "$full" ] && [ "${full#"$fragment"}" != "$full" ] || full=""
    fi
    [ -n "$full" ] && fragment=$full
```

and in `run_loop`: `attach_cmd="$(shq "$SELF") --attach $(shq "$store") {1} {2} {3} {q} $(shq "$dir")"`.

- [ ] **Step 4–5: green; shellcheck; Commit** — `git commit -am "fix: the review quotes the whole line, not the first 160 bytes of it"`

### Task 8: a delivered review is a state you can walk to, not a flash

**Files:**
- Modify: `scripts/tmux-review:556-561` (paste branch sets `@cockpit_review sent`, message names the window, `-d 0`)
- Modify: `scripts/agent-state:144` (a `running` transition unsets `@cockpit_review`)
- Modify: `scripts/tmux-jump:14-16` (cycle `sent` panes after `needs-input` ones)
- Modify: `README.md` (one paragraph under the delivery section)
- Test: extend the deliver block; new `tmux-jump` assertion

**Interfaces:**
- Produces: per-pane option `@cockpit_review` = `sent` after a successful paste; cleared by `agent-state running` (the human pressed Enter, the agent is working). `tmux-jump` visits `needs-input` panes first, then `sent` panes.

- [ ] **Step 1: Write the failing tests** — in the deliver block after the successful-landing assertion:

```bash
is "a landed review marks the pane" "sent" "$("$TMUX_REAL" -L "$RDS" show-option -pqv -t "$AP" @cockpit_review)"
"$TMUX_REAL" -L "$RDS" send-keys -t "$NP" "'$DIR/scripts/agent-state' running $AP" Enter; sleep 0.4
is "the mark clears when the agent starts working" "" "$("$TMUX_REAL" -L "$RDS" show-option -pqv -t "$AP" @cockpit_review)"
"$TMUX_REAL" -L "$RDS" set-option -p -t "$AP" @cockpit_review sent >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" select-pane -t "$NP" >/dev/null 2>&1
"$TMUX_REAL" -L "$RDS" send-keys -t "$NP" "'$DIR/scripts/tmux-jump'" Enter; sleep 0.4
is "prefix J walks to a pane holding an unsent review" "$AP" "$("$TMUX_REAL" -L "$RDS" display-message -p -t t: '#{pane_id}')"
"$TMUX_REAL" -L "$RDS" set-option -pu -t "$AP" @cockpit_review >/dev/null 2>&1
```

- [ ] **Step 2: Run to verify they fail**.
- [ ] **Step 3: Implement** — `deliver()`: before the `if-shell`, `local where; where=$(tmux display-message -p -t "$pane" '#{session_name}:#{window_index} #{window_name}' 2>/dev/null)`; the paste branch becomes

```bash
        "set-option -p -t $pane @cockpit_review sent ; paste-buffer -p -r -d -b $bufname -t $pane ; display-message -d 0 'review pasted into $where ($pane) — press Enter there to send; prefix J walks there'" \
```

`agent-state`: immediately after the line `tmux set-option -p -t "$pane" @panestate "$state"` add `[ "$state" = running ] && tmux set-option -pu -t "$pane" @cockpit_review 2>/dev/null || true`. `tmux-jump`: change the format to `'#{pane_id} #{?#{m:…},#{@panestate},} #{@cockpit_review}'`, read `id state review`, push `needs-input` ids to `blocked`, `sent` ids to a second array `pending`, then `blocked+=("${pending[@]}")` before the cycle.

- [ ] **Step 4–5: green (freeze rig unchanged — no new format on the redraw path); shellcheck; Commit** — `git commit -am "feat: an unsent review is a pane state prefix J can walk to, not a 750 ms flash"`

### Task 9: the fleet queue answers "who needs me", not "who typed most"

**Files:**
- Modify: `scripts/tmux-review:573-621` (`fleet_rows`), `:652-690` (`fleet_queue` display/sort), `:679-685` (fzf options + `@cockpit_fleet_fzf_options`)
- Modify: `README.md` fleet paragraph and key table
- Test: extend the `--fleet` block at `:3445`

**Interfaces:**
- Produces: `--fleet` rows are `pane\tstate\ttitle\trepo⑂branch\tstat`, sorted by state rank (`needs-input` 0, `done` 1, `error` 2, `running` 3, unset 4) then churn desc. A pane with `@panestate = needs-input` is listed even with a clean tree (stat `—`). `@cockpit_fleet_fzf_options` (global) is `eval`ed into extra fzf args for the fleet picker only.

- [ ] **Step 1: Write the failing tests** — in the `--fleet` block, before `kill-server`:

```bash
"$TMUX_REAL" -L "$FQ" set-option -p -t "$CLEAN_PANE" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" set-option -p -t "$DIRTY_PANE" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$FQ" send-keys -t "$CTL" "clear; '$DIR/scripts/tmux-review' --fleet" Enter; sleep 0.6
fqout2=$("$TMUX_REAL" -L "$FQ" capture-pane -p -J -t "$CTL")
is "a blocked agent with a clean tree is listed first" "$CLEAN_PANE" "$(printf '%s\n' "$fqout2" | grep '^%' | head -1 | cut -f1)"
is "its state is in the row" "needs-input" "$(printf '%s\n' "$fqout2" | grep '^%' | head -1 | cut -f2)"
case "$(printf '%s\n' "$fqout2" | grep "^$DIRTY_PANE")" in
    *"$(basename "$FQ_DIRTY")⑂main"*) ok "the row names the repo and branch" ;;
    *) bad "the row names the repo and branch" "$(basename "$FQ_DIRTY")⑂main" "$fqout2" ;; esac
```

- [ ] **Step 2: Run to verify they fail** — the clean pane is excluded; there is no state field.
- [ ] **Step 3: Implement** — in `fleet_rows` add `#{@panestate}` as a field to `fmt` (after `pane_id`), read it as `state`; replace `[ -n "$st" ] || continue` with:

```bash
        st=$(git -C "$path" --no-optional-locks status --porcelain 2>/dev/null)
        [ -n "$st" ] || [ "$state" = needs-input ] || continue
        case "$state" in needs-input) rank=0 ;; done) rank=1 ;; error) rank=2 ;; running) rank=3 ;; *) rank=4 ;; esac
        repo=$(git -C "$path" rev-parse --show-toplevel 2>/dev/null); repo=${repo##*/}
        branch=$(git -C "$path" rev-parse --abbrev-ref HEAD 2>/dev/null)
```

build `stat` as today (or `—` when `st` is empty), and the row as `"$rank"$'\t'"$churn"$'\t'"$id"$'\t'"${state:-—}"$'\t'"$title"$'\t'"${repo:-?}⑂${branch:-?}"$'\t'"$stat"`; sort `-k1,1n -k2,2nr` and `cut -f3-`. In `fleet_queue`: `--with-nth=2,3,4,5`, header `which agent needs you   state · title · repo⑂branch · ±   ↵ review it   ^/ preview   esc cancel`, and before the fzf call:

```bash
    local -a extra=()
    local opt; opt=$(tmux show-option -gqv @cockpit_fleet_fzf_options 2>/dev/null)
    [ -n "$opt" ] && eval "extra=($opt)"
```

passed as `"${extra[@]}"` after the fixed options (navigation picker only — never the review loop, where `enter` has a contract).

- [ ] **Step 4–5: green (the existing three fleet assertions still hold: clean+unset is still excluded); shellcheck; Commit** — `git commit -am "feat: the fleet queue lists who is blocked on you first, with repo and branch on the row"`

### Task 10: the preview stops octal-escaping non-ASCII paths

**Files:**
- Modify: `scripts/tmux-open-path:69-70,81` and the diff renders — add `-c core.quotePath=false` to every `git` call in `--preview-diff`
- Test: new `--preview-diff` assertion in the non-ASCII `--lines` block at `:2630`

- [ ] **Step 1: Write the failing test** — in that block (its fixture already has `café.txt` or equivalent; use its dir variable):

```bash
case "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVNA")" in
    *'\303'*) bad "the directory preview prints the name, not octal" "café in the preview" "$("$DIR/scripts/tmux-open-path" --preview-diff "$RVNA")" ;;
    *) ok "the directory preview prints the name, not octal" ;; esac
```

- [ ] **Step 2–5: FAIL → add the flag → PASS → shellcheck → Commit** — `git commit -am "fix: the preview prints a non-ASCII filename instead of its octal escape"`

### Task 11: the preview follows the cursor

**Files:**
- Modify: `scripts/tmux-review` awk in `lines_mode` (emit an offset), the bash row build (carry it as field 5), `rows_mode` (pass it through), `run_loop` (`--preview-window` offset, and the preview command takes the file only — unchanged)
- Test: new `--lines` block

**Interfaces:**
- Produces: `--lines`/`--rows` rows gain a 5th field: the row's 1-based line within `tmux-open-path --preview-diff <file>` output (1 stat line + the diff from the file's first `@@`). Untracked rows: `line + 1`. Consumers that `read -r path line fragment display` are unaffected (the 5th field lands in `display`'s tail only if they read four names — `rows_mode` must read five).

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-review --lines: every row knows where it sits in its file's preview =="
RVOFF=$(mktmpd)
(
  cd "$RVOFF" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  seq 1 40 > f.txt; git add -A; git commit -q -m init
  sed -i '3s/.*/THREE/; 30s/.*/THIRTY/' f.txt
) >/dev/null 2>&1
rvoff=$("$DIR/scripts/tmux-review" --lines "$RVOFF" | awk -F'\t' '$3=="THIRTY" {print $5}')
want=$(( 1 + $(git -C "$RVOFF" diff HEAD -- f.txt | awk '/^@@/{h=1} h' | grep -n '^+THIRTY' | cut -d: -f1) ))
is "the second hunk's row offset matches the preview's line" "$want" "$rvoff"
rm -rf "$RVOFF"
```

- [ ] **Step 2: Run to verify it fails** — no 5th field.
- [ ] **Step 3: Implement** — in the awk: add `poff = 0` on the `+++` header; on `@@` do `poff++` before `next`; inside a hunk do `poff++` for every line first, then emit `printf "%s\t%d\t+\t%d\t%s\n", file, newl, poff + 1, substr($0, 2)` (and the same shape for `-`). In the bash row build read `path line sign off rest` and append `$'\t'"$off"` to `allrows[ri]`. Synthetic untracked rows: `printf "%s\t%d\t+\t%d\t%s\n", f, NR, NR + 1, $0`. `rows_mode`: split a 5th field `off` and emit it after `display`. `run_loop`: `--preview-window="down,55%,border-top,+{5}-/2"` and `lrows=$nrows; [ "$lrows" -gt $(( avail / 2 )) ] && lrows=$(( avail / 2 ))` (the 8-row cap goes; a percentage split survives a resize).

- [ ] **Step 4–5: green — every `--lines`/`--rows` block still passes (they read ≤4 named fields); shellcheck; Commit** — `git commit -am "feat: the review preview scrolls to the line under the cursor and the list gets half the screen"`

**Phase 0 gate:** `make check` green twice in the worktree; `tests/freeze-rig/run.sh` unchanged; `ls /tmp/tmux-1000 | grep -c cockpit-` unchanged; merge `feat/review-p0` to master; push (standing approval).

---

## Phase 1 — perf

### Task 12: freeze the parser's contract in golden files

**Files:**
- Create: `tests/fixtures/review/mkfix.sh` (builds the six parser fixtures deterministically: `basic`, `mixed`, `untracked`, `blank`, `nonascii`, `funcname`) and `tests/fixtures/review/<name>.tsv` (the current `--lines` output for each)
- Test: `tests/run.sh` — one block that rebuilds each fixture and `cmp`s

- [ ] **Step 1: Write the test**

```bash
echo "== tmux-review --lines: output is byte-identical to the committed goldens =="
for gf in basic mixed untracked blank nonascii funcname; do
    GD=$(mktmpd); "$DIR/tests/fixtures/review/mkfix.sh" "$gf" "$GD" >/dev/null 2>&1
    if "$DIR/scripts/tmux-review" --lines "$GD" | cmp -s - "$DIR/tests/fixtures/review/$gf.tsv"; then ok "golden $gf"
    else bad "golden $gf" "byte-identical" "$("$DIR/scripts/tmux-review" --lines "$GD" | diff - "$DIR/tests/fixtures/review/$gf.tsv" | head -5)"; fi
    rm -rf "$GD"
done
```

- [ ] **Step 2: Write `mkfix.sh`** — a `case "$1"` over the six names, each body the exact fixture-building subshell from the corresponding existing test block (copy them; deterministic author env; `git init -q -b main`).
- [ ] **Step 3: Generate the goldens** from the phase-0 parser: `for f in …; do mkfix.sh $f $d; tmux-review --lines $d > tests/fixtures/review/$f.tsv; done`. Inspect each by eye once.
- [ ] **Step 4: Run — all six PASS. Commit** — `git commit -am "test: golden --lines output for six fixtures, the oracle for the awk rewrite"`

### Task 13: cap before capture, on bytes as well as rows

**Files:**
- Modify: `scripts/tmux-review` `lines_mode` (`:88-124` region after Task 2)
- Test: extend the cap block at `:2470` with a timing tooth and a wide-line fixture

**Interfaces:**
- Produces: order is numstat → count rows and bytes → cap → capture. Row cap 5000 (unchanged); byte cap 4 MB across changed files (`wc -c` on worktree files, `git cat-file -s HEAD:path` for deletions). Notice text gains the reason: `(N changed lines)` or `(N MB)`.

- [ ] **Step 1: Write the failing test** — a 51 MB single-file fixture is too slow for the suite; use 400k rows of `seq`:

```bash
RVLOCK=$(mktmpd)
( cd "$RVLOCK" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'seed\n' > lock.json; git add -A; git commit -q -m init; seq 1 400000 > lock.json ) >/dev/null 2>&1
if timeout 1.5 "$DIR/scripts/tmux-review" --lines "$RVLOCK" > "$STATE/lock.out" 2>/dev/null; then
    ok "a 400k-row lockfile reports too-large in under 1.5s (measured 5.7s before numstat-first)"
else bad "a 400k-row lockfile reports too-large in under 1.5s" "exit within 1.5s" "timeout"; fi
RVWIDE=$(mktmpd)
( cd "$RVWIDE" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'seed\n' > wide.txt; git add -A; git commit -q -m init
  awk 'BEGIN{ s=sprintf("%1500s",""); gsub(/ /,"w",s); for(i=0;i<4000;i++) print s }' > wide.txt ) >/dev/null 2>&1
case "$("$DIR/scripts/tmux-review" --lines "$RVWIDE")" in *"diff too large"*"MB"*) ok "4000 rows of 1.5 KB each cap on bytes" ;;
    *) bad "4000 rows of 1.5 KB each cap on bytes" "a too-large notice naming MB" "$(head -c 200 <<<"$("$DIR/scripts/tmux-review" --lines "$RVWIDE")")" ;; esac
rm -rf "$RVLOCK" "$RVWIDE"
```

- [ ] **Step 2: Run to verify it fails** — the lockfile case times out; the wide case parses.
- [ ] **Step 3: Implement** — move the `numstat=` line and the counting loop above the `diff=` capture; add a byte total in the same loop (`bytes=$(( bytes + $(wc -c < "$dir/$nfile" 2>/dev/null || echo 0) ))`, and for a deleted file `git -C "$dir" cat-file -s "HEAD:$nfile"`); cap when `total -gt 5000` or `bytes -gt 4194304`, emitting the notice with the reason, **then** `diff=$(tree_diff "$dir")`.
- [ ] **Step 4–5: green; goldens still identical; Commit** — `git commit -am "perf: the diff is counted and capped before it is captured — a lockfile answers in under a second instead of 5.7s and 254MB"`

### Task 14: rows come out of awk, not a bash loop

**Files:**
- Modify: `scripts/tmux-review` `lines_mode` — replace the bash row build, churn map, partition and emit loops (`:198-268` pre-phase-0 numbering) with one awk program plus `sort | cut`
- Reference: `docs/superpowers/research/2026-08-24-review-lenses/prototypes/awkonly.sh` (byte-identical on the six goldens *before* Task 2/11's columns; extend it with the offset field and the untracked rows)
- Test: the goldens (Task 12) + the existing fork/quadratic block at `:2530` + a timing tooth

**Interfaces:**
- Consumes: `raw` (path, line, sign, off, text) and `numstat`.
- Produces: identical `--lines` output; measured target 4900 rows < 80 ms (`timeout 0.4` tooth).

- [ ] **Step 1: Write the failing test** — a 4900-row, 1-file fixture (`seq 1 4900 > f`) with `timeout 0.4 "$DIR/scripts/tmux-review" --lines "$RV49" >/dev/null` → `ok "4900 rows build in under 0.4s (measured 427ms bash, 55ms awk)"`.
- [ ] **Step 2: Run to verify it fails** (bash path ~430 ms on this machine; if it passes on a faster box lower the tooth to 0.2).
- [ ] **Step 3: Implement** — the shape (the full program is in the prototype; port it, keeping every existing awk portability rule — no `asorti`, no `gensub`):

```awk
# input 1: numstat (add \t del \t path)  -> churn[path], rank by *test* glob
# input 2: raw rows (path \t line \t sign \t off \t text)
# output: "<part>\t<churn>\t<seq>\t<row>" then sort -t'\t' -k1,1n -k2,2nr -k3,3n | cut -f4-
NR==FNR { if ($1 ~ /^[0-9]+$/) a=$1; else a=0; if ($2 ~ /^[0-9]+$/) d=$2; else d=0; churn[$3]=a+d; next }
{
  p=$1; if (!(p in seen)) { seen[p]=1; order[++nf]=p; part[p]=(p ~ /test/) ? 0 : 1 }
  frag=$5; gsub(/[\001-\037\177]/, "", frag); frag=substr(frag, 1, 160)
  ml=$2; if ($3=="-") ml="-" $2
  disp=sprintf("%s:%s (%s)  %s", p, $2, $3, frag)
  printf "%d\t%09d\t%09d\t%s\t%s\t%s\t%s\t%s\n", part[p], 999999999-churn[p], NR, p, ml, frag, disp, $4
}
```

(the `999999999-churn` trick makes one ascending sort do churn-desc; `seq` = `NR` keeps first-seen order on ties). Call as `awk -F'\t' -v OFS='\t' … <(printf '%s\n' "$numstat") <(printf '%s\n' "$raw") | sort -t "$(printf '\t')" -k1,1n -k2,2 -k3,3 | cut -f4-`. Delete `churn`, `seen/gstart/gend`, `order`, `tests/others`, `order_group` and their comments.
- [ ] **Step 4: Run — six goldens byte-identical (this is the whole test), the timing tooth green, `:2530` block green.**
- [ ] **Step 5: shellcheck; Commit** — `git commit -am "perf: rows are built in the awk that already parses the diff — 4900 rows in 55ms, not 427ms"`

### Task 15: a comment re-marks cached rows instead of re-deriving the diff

**Files:**
- Modify: `scripts/tmux-review` `run_loop` (write the initial rows to `$store.rows`), `rows_mode` (read `$store.rows` when present; marking in awk), `review_pane` (remove `$store.rows` with the store)
- Reference: `prototypes/markcache.sh`
- Test: timing + equivalence block

**Interfaces:**
- Produces: `--rows <dir> <store>` uses `<store>.rows` if it exists (the snapshot of `--lines` taken at popup open); output is identical to the uncached path for the same tree.

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-review --rows: re-marking after a comment reads the cache, not git =="
RV49=$(mktmpd); ( cd "$RV49" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'seed\n' > f; git add -A; git commit -q -m init; seq 1 4900 > f ) >/dev/null 2>&1
RV49S=$(mktmpf); "$DIR/scripts/tmux-review" --lines "$RV49" > "$RV49S.rows"
for i in $(seq 1 500); do printf '## f:%d (+)\n> %d\nc\n\n' "$i" "$i"; done > "$RV49S"
if timeout 0.15 "$DIR/scripts/tmux-review" --rows "$RV49" "$RV49S" > "$STATE/rows.cached" 2>/dev/null; then
    ok "a 500-comment re-mark over 4900 rows takes under 0.15s (measured 946ms before, 24ms after)"
else bad "a 500-comment re-mark over 4900 rows takes under 0.15s" "exit within 0.15s" "timeout"; fi
rm -f "$RV49S.rows"; "$DIR/scripts/tmux-review" --rows "$RV49" "$RV49S" > "$STATE/rows.fresh"
is "the cached and fresh rows are identical" "" "$(cmp "$STATE/rows.cached" "$STATE/rows.fresh" 2>&1)"
is "500 rows are marked" "500" "$(grep -c '●' "$STATE/rows.cached")"
rm -rf "$RV49" "$RV49S"
```

- [ ] **Step 2: Run to verify it fails** (times out).
- [ ] **Step 3: Implement** — in `rows_mode`: `local src; if [ -n "$store" ] && [ -s "$store.rows" ]; then src="$store.rows"; else src=<(lines_mode "$dir"); fi` and replace the bash marking loop with:

```bash
    awk -F'\t' -v OFS='\t' '
        NR==FNR { if ($0 ~ /^## /) c[$0]=1; next }
        { sign = ($2 ~ /^-/) ? "-" : "+"; num = $2; sub(/^-/, "", num)
          m = ("## " $1 ":" num " (" sign ")" in c) ? "●" : " "
          disp = $4
          if (sign == "+") sub(/ \(\+\) /, " \033[32m(+)\033[0m ", disp); else sub(/ \(-\) /, " \033[31m(-)\033[0m ", disp)
          print $1, $2, $3, m disp, $5 }' "${store:-/dev/null}" "$src"
```

In `run_loop`: after `rows_out=$(rows_mode …)` is computed at startup, write `lines_mode "$dir" > "$store.rows"` once (or derive `rows_out` from it). In `review_pane`'s cleanup: `rm -f -- "$store" "$store.rows"`, and in the discard paths too.
- [ ] **Step 4–5: green (Task 3/6 `--rows` assertions still hold — the awk reproduces them); shellcheck; Commit** — `git commit -am "perf: pinning a comment re-marks cached rows in 24ms instead of re-parsing the diff in a second"`

### Task 16: the preview is rendered once per file, not once per cursor move

**Files:** (lane B — parallel with Task 15)
- Modify: `scripts/tmux-open-path --preview-diff` — memoise under `${XDG_RUNTIME_DIR:-/tmp}/tmux-cockpit-preview-$(id -u)/<sha1 of path,HEAD,mtime,size>`; one `git diff HEAD` call feeds both the stat line and the body
- Test: timing + invalidation block

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-open-path --preview-diff: the second render of a file is a cache hit =="
PV=$(mktmpd); ( cd "$PV" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; seq 1 2000 > big.txt; git add -A; git commit -q -m init; sed -i '1000s/.*/CHANGED/' big.txt ) >/dev/null 2>&1
"$DIR/scripts/tmux-open-path" --preview-diff "$PV/big.txt" > "$STATE/pv1"
if timeout 0.05 "$DIR/scripts/tmux-open-path" --preview-diff "$PV/big.txt" > "$STATE/pv2" 2>/dev/null; then ok "a repeat render returns in under 50ms"
else bad "a repeat render returns in under 50ms" "cache hit" "timeout"; fi
is "and is identical" "" "$(cmp "$STATE/pv1" "$STATE/pv2" 2>&1)"
sed -i '1001s/.*/ALSO/' "$PV/big.txt"
case "$("$DIR/scripts/tmux-open-path" --preview-diff "$PV/big.txt")" in *ALSO*) ok "an edited file misses the cache" ;;
    *) bad "an edited file misses the cache" "ALSO in the preview" "stale" ;; esac
rm -rf "$PV"
```

- [ ] **Step 2–5: FAIL → implement (key = `printf '%s\n%s\n%s\n' "$f" "$(git -C "$d" rev-parse HEAD)" "$(stat -c '%Y %s' "$f" 2>/dev/null || stat -f '%m %z' "$f")" | sha1sum`; write with `umask 077` to `key.tmp` then `mv`; cap the cache dir at 200 files by deleting the oldest on write) → PASS → shellcheck → Commit** — `git commit -am "perf: a file's preview renders once per edit, not once per arrow key"`

### Task 17: the fleet scan runs its panes in parallel and never takes the index lock

**Files:** (lane A, after Task 15)
- Modify: `scripts/tmux-review` `fleet_rows` — per-pane body into a function, run via `xargs -P 8` (or `&` + `wait` with per-pane temp files), `--no-optional-locks` on every git call, dedup by repo root, untracked count from `$st` (no `grep` fork)
- Reference: `prototypes/fleet_par.sh`
- Test: extend the `--fleet` block with 20 panes and a `timeout 1` tooth; rows must equal the serial output sorted

- [ ] **Step 1: Write the failing test** — open 20 `exec -a claude bash` windows in 20 fixture repos (a loop over `mk_git_dir`), then `timeout 1 "$DIR/scripts/tmux-review" --fleet` from a control pane via `send-keys` + `lastrc`-style `RC=` marker → `is "20 panes scan in under 1s" "RC=0"` and `is "20 rows" "20"`.
- [ ] **Step 2–5: FAIL (serial ~2 s under suite load) → implement → PASS → shellcheck → Commit** — `git commit -am "perf: the fleet queue scans panes in parallel without taking any repo's index lock"`

### Task 18: `agent-state` batches its tmux round-trips

**Files:** (lane C — parallel with 15–17)
- Modify: `scripts/agent-state` — the set/unset pairs at `:141-153`, `:193-195`, `:230-232`, `:256` are joined into as few `tmux` invocations as the read-then-decide structure allows (`tmux set-option … \; set-option …`), keeping the compare-then-set guards
- Test: a fork-count tooth like the one at `tests/run.sh:2530` (`strace -f -e execve` is not portable; count `tmux` invocations by pointing `PATH` at a counting shim, the pattern at `:856-882`)

- [ ] **Step 1: Write the failing test** — a shim `tmux` that appends to a counter file then execs the real one; run `agent-state running %N` and `agent-state done %N`; assert the count per call ≤ 4 (measured 6 today).
- [ ] **Step 2–5: FAIL → implement → PASS → `tests/freeze-rig/run.sh` unchanged → Commit** — `git commit -am "perf: a hook transition costs four tmux round-trips instead of six"`

**Phase 1 gate:** goldens byte-identical; the four timing teeth green three runs in a row (not once); `make check` green twice; freeze rig unchanged; leak sweep; merge; record the before/after numbers in the merge commit body.

---

## Self-review record

- **Spec coverage:** synthesis §3 defects 1 (Tasks 11+15 partially — full freeze is phase 2's snapshot), 2 (T1), 3 (T2), 4 (T2), 5 (T3), 6 (T5), 7 (phase 3 — `prefix r` always the queue needs the loop-back; T9 gets the queue right), 8 (T8), 9 (T4), 10 (T11), 11 (T4 — plain key first; alt-u/alt-d stay until phase 2's editor key), 12 (phase 2), 13 (T7), 14 (T9). Perf §5 rows: cap (T13), open (T14), comment (T15), preview (T16), fleet (T17), hooks (T18). Front-end steals 1, 2, 4-footer (deferred to phase 3 with the confirm screen), 8 (T9), 10–11 (T9 uses padded columns via `printf '%-*s'` — add to T9's display build).
- **Placeholder scan:** none of "TBD/similar to/add handling"; every step has its code or the exact prototype file it ports.
- **Type consistency:** `tree_diff` (T2) is used in T13/T14/T16; the 5-field row (T11) is consumed by T14/T15; `@cockpit_review` (T8) is read by `tmux-jump` and cleared by `agent-state`; `--attach`'s optional 6th arg (T7) is passed by `run_loop`.
