# Review Redesign — Phase 2 (the artifact) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** give one review one place on disk that outlives the popup — a 0700 session dir holding the comment store and its row snapshot — and, on top of the parser that already landed, the four keys the store could never support: `^X` delete, `^L` list, `^E` editor, `^R` re-snapshot with a stale report.

**Architecture:** `review_pane()` swaps `mktemp` for a deterministic dir under `$XDG_STATE_HOME/tmux-cockpit/review/<pane>-<cksum root>/` (owner decision §8 #1, answered yes 2026-08-25). The snapshot that phase 1 already writes (`<store>.rows`) becomes the immutable artifact; `snapshot()` is the one function that writes it and `stale_keys()` the one that joins comments against it. `scripts/lib/review-store.sh` (landed on this branch: `parse_store` = inverse of `format_store`, round-trip tested) is sourced only by the two subcommands that rewrite the store. `attach()`, `compose()`, `deliver()` are untouched.

**Tech Stack:** bash 4+, tmux ≥ 3.3 (popups), fzf 0.44.1 (`execute`), git, POSIX awk (mawk here), `cksum` (POSIX, everywhere). No new dependency.

**Spec:** `docs/superpowers/plans/2026-08-24-review-redesign-goal.md` §1 phase 2 row, §2 rig, §3; `docs/superpowers/research/2026-08-24-review-lenses/dev.md` §5 (the decomposition) and §2.2 (the drift repro); `docs/superpowers/research/2026-08-24-review-lenses/ux.md` lines 184–199 (the keys); `docs/superpowers/plans/2026-08-25-review-section8-decisions.md` (the answers). The ponytail audit below records where this plan deliberately ships less than dev.md §5 asked for.

**Branch:** `feat/phase2-store`, worktree `~/.cache/cockpit-wt/phase2-store`, three commits ahead of master (`6ad8b08` test-only runner, `00f9c0c` the store lib, `83e2c1f` syntax-block fix). All work continues there.

## Global Constraints

- No shell on a status redraw; hooks never block the agent; precomputed counters use compare-then-set at transition time (project memory `freeze-precompute-pattern`). Phase 2 adds no tmux option and no counter.
- State = per-pane tmux options that die with the pane, **plus** the one relaxation the owner approved: a session dir under `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/review/`, created 0700, removed on discard and on successful delivery, kept (and named) on a refused delivery. No git ref in this phase (that is phase 3's `^A`).
- One-shot scripts; no daemon; no server; no port.
- Never `send-keys Enter` into an agent pane. Never paste into `needs-input`. Never steal focus.
- Batch delivery only; `^S` is the consent gesture. `↵` never approves anything. `^X` deletes a comment, never the batch (the batch is discarded through esc's existing confirm).
- Every `tmux` command string quotes its paths (`printf %q` via `shq` in scripts, `sq` in `cockpit.tmux`); post-popup work goes through `tmux run-shell -b`.
- Pure module stays pure: `sed '/^[[:space:]]*#/d' scripts/lib/review-store.sh | grep -c 'tmux\|git'` is `0` (already a test). `tmux-review` sources the lib only inside the subcommands that rewrite the store.
- `bash -n` + `shellcheck -x` clean; `make check` is the gate, run **unpiped** (project memory `gate-pipelines-mask-failures`). `make test-only PAT='<block title ERE>'` runs one block group in seconds (landed on this branch).
- Tests run only on private `-L` sockets; every server a test starts is added to `SOCKS`; every mktemp goes through `mktmpd`/`mktmpf`. **No new fixed `sleep`.** New waits use `until_cap` / `until_file` (Task 1).
- Commits: lowercase `feat:`/`fix:`/`test:` one-liners in the repo's voice (measured, past tense), ending with the session trailer.
- Work only in the worktree. Never edit `~/.tmux/plugins/tmux-cockpit` directly — another session shares it (project memory `concurrent-sessions-shared-worktree`).
- **Ponytail governs the build** (the `ponytail` skill, level full): read the whole flow first, then the shortest diff that works; no rename for its own sake, no abstraction with one caller, no file split with one consumer, deletion over addition; a deliberate ceiling gets a `# ponytail:` comment naming the upgrade path. Every task below is the lazy version on purpose — a worker who finds a shorter correct diff takes it and says so in the summary.

## Ponytail audit — what dev.md §5 asked for and this plan does not build

| dev.md §5 item | verdict | why |
|---|---|---|
| noun-verb subcommands (`snapshot`/`rows`/`annotate`/…) with `--flag` aliases | **skip** | a rename; aliases double the surface. New entry points follow the existing `--flag` style: `--detach`, `--edit`, `--resnap`. |
| `lib/review-diff.awk` split | **skip** | one consumer (`lines_mode`); the boundary test already guards the one lib. Split when a second reader of the awk appears (phase 4's hunk `--agent-context`). |
| `diff.tsv` with `id file hunk side line blob text` + `meta` | **skip** | the snapshot already exists since phase 1 as `<store>.rows` (5 fields); `hunk`/`blob` have no reader in this phase; `meta` has no reader at all (the refusal message names root, pane and store). The session dir holds `comments.md` and `comments.md.rows`. |
| reimplement `attach`/`compose` on the parsed model | **skip** | `>>` append is the crash-safety property; `cat` is the formatter and compose's test pins its bytes. `parse_store` is used where a rewrite is needed (`^X`) and where a join is needed (`^R`, resume). |
| `@` nested fzf over the store | **replace** with `^L` | in the comment loop every printable key is comment text (`--disabled` routes typing to the query) — `@` would eat a character. `^L` = `show-preview+preview(cat store)`: one bind, zero code. |
| `●N` | **keep, one column** | `●` for one, the digit for 2–9, `+` beyond (an honest "more", not a wrong number) — the marker column stays one cell wide (`mark_rows`'s "prepend, never repaint" rule). |
| 25 fixed sleeps → polls | **9 of 21** | the nine that wait for an event a poll can see (typed text, a store write, an `RC=` marker, a paste landing) convert; the twelve that are negative assertions ("nothing pinned", "the gate refused") or pane-spawn settles stay — a poll on "nothing happened" is a sleep with extra steps. Each kept one already says so in a comment or gets one. |
| `^X` = discard the batch with confirm (when-how lens) | **defer to phase 3** | esc's existing confirm already offers `discard N comment(s)`; a second discard key in this phase is two ways to do one thing. Phase 3 revisits when esc's meaning changes to "keep the spool". |
| `tests/only.sh` (landed) | **keep, mark** | 33 lines of awk that make the TDD cycle 3 s instead of 133 s; its brace-depth count is a known ceiling — Task 0 adds the `# ponytail:` line. |

---

### Task 0: Mark the landed runner's ceiling (already on the branch: lib, round-trip test, `test-only`)

**Files:**
- Modify: `tests/only.sh:15-24` (the awk)

**Interfaces:**
- Consumes: nothing.
- Produces: nothing new — `parse_store FILE` / `format_store FILE` over globals `STORE` (assoc, keys `<i>.path .line .end .side .frag .body`) and `STORE_N` already exist in `scripts/lib/review-store.sh` and are what Tasks 3 and 4 call.

- [ ] **Step 1: Add the ceiling comment**

In `tests/only.sh`, directly above the line `/^[A-Za-z_][A-Za-z_0-9]*\(\) *\{/ { infn = 1; depth = 0 }   # a top-level function`, insert:

```bash
    # ponytail: brace depth counts every { and } in the line, strings included;
    # a helper whose body prints a lone brace would end early -- none does today.
    # Upgrade: skip quoted regions, or mark helper ends with a "# }" sentinel.
```

- [ ] **Step 2: Verify the runner still runs**

Run: `cd ~/.cache/cockpit-wt/phase2-store && make test-only PAT='review-store'`
Expected: `  N passed, 0 failed, 0 skipped` with N ≥ 17 and every line `PASS`.

- [ ] **Step 3: Commit**

```bash
git add tests/only.sh
git commit -m "test: only.sh names its brace-depth ceiling"
```

---

### Task 1: Poll helpers, and the nine waits that have an event to wait for

**Files:**
- Modify: `tests/run.sh` — the prelude after `mktmpf()` (before the first `echo "== syntax =="`), then nine `sleep` sites named by block title below.

**Interfaces:**
- Produces: `until_cap <socket> <pane> <glob-pattern>` — polls `capture-pane -p -J` up to 4 s (40 × 0.1 s) until the capture matches the `case` pattern; returns 1 on timeout. `until_file <file> <glob-pattern>` — same over `cat <file>`. Both are prelude functions, so `tests/only.sh` carries them into every block group.

- [ ] **Step 1: Add the helpers to the prelude**

Directly after the line `mktmpf() { local f; f=$(mktemp)    || return 1; printf '%s\n' "$f" >> "$TMPS"; printf '%s' "$f"; }` insert:

```bash
# Polled, not slept for. A fixed sleep guesses how long a keystroke takes to
# land; these wait for the thing the next assertion reads and give up after
# 4 s. PATTERN is a case-pattern: until_cap "$S" "$P" '*review>*'
until_cap() {                 # <socket> <pane> <pattern>
    local i
    for i in $(seq 1 40); do
        case "$("$TMUX_REAL" -L "$1" capture-pane -p -J -t "$2" 2>/dev/null)" in $3) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
until_file() {                # <file> <pattern>
    local i
    for i in $(seq 1 40); do
        case "$(cat -- "$1" 2>/dev/null)" in $2) return 0 ;; esac
        sleep 0.1
    done
    return 1
}
```

- [ ] **Step 2: Convert the nine sites**

Each row: the block title (grep it — line numbers move), the exact line to replace, its replacement.

| block | replace | with |
|---|---|---|
| `tmux-review --loop: alt-enter pins a comment, ctrl-s delivers, esc discards` | the `sleep 0.5` right after `send-keys -t "$RVP" "fix the new line"` | `until_cap "$RVS" "$RVP" '*fix the new line*'` |
| same block | the `sleep 1` right after `send-keys -t "$RVP" M-Enter` (the first M-Enter) | `until_file "$RVSTORE" '*fix the new line*'` |
| `tmux-review --deliver: guards refuse before anything is pasted` — inside `rundeliver()` | the `sleep 0.6` after the `send-keys ... echo RC=\$?" Enter` | see the function below |
| `tmux-review --deliver: the paste is gated atomically...` | the `sleep 0.3` right after `paste-buffer -p -r -d -b rg-hz -t "$NPG"` | `until_cap "$RG" "$NPG" '*HAZARD-MARKER*'` |
| same block | the `sleep 0.3` right after the `if-shell` whose buffer is `rg-ok` (the DELIVERED-MARKER gate) | `until_cap "$RG" "$APG" '*DELIVERED-MARKER*'` |
| `tmux-review: a delivered review's comment store does not outlive the popup that made it` | the `sleep 0.3` right after `send-keys -t "$CTL2" "fix this"` | `until_cap "$FD" "$CTL2" '*fix this*'` |
| same block | the `sleep 0.5` right after `send-keys -t "$CTL2" M-Enter` | `until_cap "$FD" "$CTL2" '*●*'` |
| same block | the `sleep 1` right after `send-keys -t "$CTL2" C-s` | `until_cap "$FD" "$FD_AGENT" '*fix this*'` |
| `tmux-review: a clean-tree direct pane says so instead of opening an empty picker` | the `sleep 1` right after `send-keys ... echo MTDONE=\$?" Enter` | `until_cap "$MT" "$MTP" '*MTDONE=*'` |

`rundeliver()` becomes:

```bash
rundeliver() {
    # The typed command line itself contains "RC=$?", so count only markers
    # followed by a digit or "-" -- the ones a finished command printed.
    local n0 i; n0=$(npcap | grep -c 'RC=[0-9-]')
    "$TMUX_REAL" -L "$RDS" send-keys -t "$NP" \
        "$(shq_test "$DIR/scripts/tmux-review") --deliver $(shq_test "$1") $(shq_test "$2"); echo RC=\$?" Enter
    for i in $(seq 1 40); do
        [ "$(npcap | grep -c 'RC=[0-9-]')" -gt "$n0" ] && return 0
        sleep 0.1
    done
    return 1
}
```

The twelve sites NOT converted, so a later reader does not "finish the job": the two documented negative waits in the loop block (`sleep 0.3` before "Enter with nothing typed pins nothing", `sleep 1` before "plain enter does nothing"); the four 0.3 s pane-spawn settles after `split-window`/`new-window`; the `sleep 0.8` in the concurrent-delivery race (both deliveries must finish, a poll on either passes early); the `sleep 0.2`s after `export TMPDIR/XDG_RUNTIME_DIR` and after `C-c`; the `sleep 0.3` before "a window name's #(...) never runs" and before "the atomic gate refuses" (negative). Add this one-line comment above each of the four spawn settles that lacks one: `# a spawn settle, not an event: nothing to poll until the shell prints`.

- [ ] **Step 3: Run the touched blocks**

Run: `make test-only PAT='tmux-review --loop: alt-enter|--deliver: guards|gated atomically|does not outlive|clean-tree direct'`
Expected: every line `PASS`, `0 failed`. Then run it a second time — same result.

- [ ] **Step 4: Commit**

```bash
git add tests/run.sh
git commit -m "test: nine review waits poll for their event instead of sleeping a guessed interval"
```

---

### Task 2: The session dir — a review outlives the client that opened it

**Files:**
- Modify: `scripts/tmux-review` — `review_pane()` (grep `^review_pane()`), and the snapshot write in `run_loop()` (the `(umask 077; set -C; lines_mode ...)` line).
- Modify: `tests/run.sh` — two existing blocks that locate the store under `$TMPDIR`; one new block.
- Modify: `README.md` — the "Review the agent's changes" section, the paragraph beginning `**No review daemon, server, or state that outlives the popup.**`

**Interfaces:**
- Produces: `session_dir <pane-id> <root>` → prints `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/review/<pane-id without %>-<cksum of root>`. `review_pane` passes `"$sess/comments.md"` as the store to `--loop` and `--deliver`; every other subcommand keeps taking a store path and does not know about the dir. `--loop` still works on a bare store path (the suite's loop tests) — the dir is `review_pane`'s business only.

- [ ] **Step 1: Write the failing test**

Append at the end of `tests/run.sh`, before the summary lines (the final `printf` of pass/fail counts):

```bash
echo "== tmux-review: a review survives the client that opened it =="
# review_pane() used to keep the only copy of a review in a mktemp file
# whose name nothing recorded: a dropped SSH client tears down a
# client-owned popup and took the review with it. Now one review is one
# 0700 directory keyed on pane + repo root, so reopening the same pane
# finds its own comments; discarding removes the directory.
if need_tmux "review session dir"; then :; else
SD="cockpit-sd-$$"; SOCKS="$SOCKS $SD"
SDDIR=$(mktmpd); SDSTATE=$(mktmpd)
(
  cd "$SDDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
"$TMUX_REAL" -L "$SD" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$SDDIR" \
    'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$SD" set -g @cockpit_agents_glob \
    '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
SDAP=$("$TMUX_REAL" -L "$SD" list-panes -t t: -F '#{pane_id}')
sd_shell() {                  # a control pane, ready to take a command
    local p i
    p=$("$TMUX_REAL" -L "$SD" split-window -t t -P -F '#{pane_id}' 'exec bash')
    for i in $(seq 1 40); do
        [ "$("$TMUX_REAL" -L "$SD" display-message -p -t "$p" '#{pane_current_command}')" = bash ] && break
        sleep 0.1
    done
    printf '%s' "$p"
}
SDCTL=$(sd_shell)
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL" \
    "XDG_STATE_HOME=$(printf '%q' "$SDSTATE") '$DIR/scripts/tmux-review' -p $SDAP" Enter
if until_cap "$SD" "$SDCTL" '*review>*'; then ok "the picker comes up for -p <agent pane>"
else bad "the picker comes up for -p <agent pane>" "review>" "$("$TMUX_REAL" -L "$SD" capture-pane -p -t "$SDCTL")"; fi
SDROOT=$(git -C "$SDDIR" rev-parse --show-toplevel)
SDSESS="$SDSTATE/tmux-cockpit/review/${SDAP#%}-$(printf '%s' "$SDROOT" | cksum | cut -d' ' -f1)"
sdmode() { stat -c '%a' "$1" 2>/dev/null || stat -f '%Lp' "$1" 2>/dev/null || echo none; }
is "the session dir is keyed on pane + root and born 0700" "700" "$(sdmode "$SDSESS")"
is "and its parents are private too" "700" "$(sdmode "$SDSTATE/tmux-cockpit")"
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL" "keep me"
until_cap "$SD" "$SDCTL" '*keep me*'
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL" Enter
if until_file "$SDSESS/comments.md" '*keep me*'; then ok "a pinned comment lands in <session>/comments.md"
else bad "a pinned comment lands in <session>/comments.md" "keep me" "$(cat "$SDSESS/comments.md" 2>/dev/null)"; fi
is "the store is 0600" "600" "$(sdmode "$SDSESS/comments.md")"
is "the snapshot sits beside it" "yes" "$([ -s "$SDSESS/comments.md.rows" ] && echo yes || echo no)"
# The client drops: the pane running the picker dies with it.
"$TMUX_REAL" -L "$SD" kill-pane -t "$SDCTL"
is "the review outlives the client" "1" "$(grep -c '^## src.go:3 (+)' "$SDSESS/comments.md" 2>/dev/null)"
SDCTL2=$(sd_shell)
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL2" \
    "XDG_STATE_HOME=$(printf '%q' "$SDSTATE") '$DIR/scripts/tmux-review' -p $SDAP" Enter
if until_cap "$SD" "$SDCTL2" '*●src.go:3 (+)*'; then ok "reopening the same pane resumes its comments, marked"
else bad "reopening the same pane resumes its comments, marked" "●src.go:3 (+)" "$("$TMUX_REAL" -L "$SD" capture-pane -p -t "$SDCTL2")"; fi
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL2" Escape
until_cap "$SD" "$SDCTL2" '*discard 1 comment*'
"$TMUX_REAL" -L "$SD" send-keys -t "$SDCTL2" Down Enter
for i in $(seq 1 40); do [ -e "$SDSESS" ] || break; sleep 0.1; done
is "discarding removes the session dir, not just the store" "gone" "$([ -e "$SDSESS" ] && echo present || echo gone)"
unset -f sd_shell sdmode
"$TMUX_REAL" -L "$SD" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 2: Run it to see it fail**

Run: `make test-only PAT='survives the client'`
Expected: "the picker comes up" PASSes; "the session dir is keyed on pane + root and born 0700" FAILs with `got [none]`; the rest fail after it.

- [ ] **Step 3: Implement `session_dir()` and rewrite `review_pane()`**

In `scripts/tmux-review`, directly above `review_pane()` add:

```bash
# --- the session dir ---------------------------------------------------------
# One review is one directory the popup does not own. `store=$(mktemp)` kept
# the only copy of a review in a file whose name nothing recorded, and a
# dropped SSH client -- which tears down a client-owned display-popup --
# took it along. Keyed on pane + repo root so reopening the same pane finds
# its own comments; 0700 because it holds the whole uncommitted diff. Removed
# when the review is discarded or delivered; kept, and named, when delivery
# is refused. `rm -rf ~/.local/state/tmux-cockpit/review` is the purge.
session_dir() {               # <pane-id> <root>
    printf '%s/tmux-cockpit/review/%s-%s' "${XDG_STATE_HOME:-$HOME/.local/state}" \
        "${1#%}" "$(printf '%s' "$2" | cksum | cut -d' ' -f1)"
}
```

Replace the body of `review_pane()` from `store=$(mktemp) || {` through the closing `}` of the `if [ "$rc" -eq 20 ]` block with:

```bash
    local sess cleanup
    sess=$(session_dir "$pane" "$dir"); store="$sess/comments.md"
    # umask in the subshell: mkdir -p creates the parents at the caller's
    # umask otherwise, and the store is born 0600 the same way.
    (umask 077; mkdir -p -- "$sess" && : >> "$store") || {
        tmux display-message "tmux-review: could not create $sess" 2>/dev/null
        return 1
    }
    "$SELF" --loop "$dir" "$store"
    rc=$?
    if [ "$rc" -eq 20 ]; then
        # The STORE is removed only on success: --deliver refuses AFTER the
        # popup is gone, and the store is the only copy of the review. The
        # snapshot goes either way; the dir goes only once it is empty, so a
        # refused store keeps its directory and the refusal names it.
        cleanup="rc=\$?; rm -f -- $(shq "$store.rows"); if [ \$rc -eq 0 ]; then rm -f -- $(shq "$store"); rmdir -- $(shq "$sess") 2>/dev/null; fi"
        tmux run-shell -b "$(shq "$SELF") --deliver $(shq "$pane") $(shq "$store") $(shq "$dir"); $cleanup"
    else
        rmdir -- "$sess" 2>/dev/null   # empty after a discard; a kept store keeps it
    fi
```

Keep the existing explanatory comment above the `if [ "$rc" -eq 20 ]` block (the one that begins `# deliver() is a public subcommand`) — trim its "row SNAPSHOT is removed either way" paragraph to match the new cleanup string; delete nothing else.

In `run_loop()`, the resumed session's old snapshot must not block the new one — `set -C` refuses an existing path. Change:

```bash
    if [ -n "$store" ]; then
        (umask 077; set -C; lines_mode "$dir" > "$store.rows") 2>/dev/null \
            || rm -f -- "$store.rows"
    fi
```

to:

```bash
    if [ -n "$store" ]; then
        rm -f -- "$store.rows"    # a resumed session's snapshot: set -C would refuse to replace it
        (umask 077; set -C; lines_mode "$dir" > "$store.rows") 2>/dev/null \
            || rm -f -- "$store.rows"
    fi
```

- [ ] **Step 4: Move the two existing popup-store blocks off `$TMPDIR`**

Block `tmux-review: a delivered review's comment store does not outlive the popup that made it`: replace `"export TMPDIR=$(shq_test "$FDTMPDIR")" Enter` with `"export XDG_STATE_HOME=$(shq_test "$FDTMPDIR")" Enter`; replace

```bash
sf=$(ls "$FDTMPDIR" 2>/dev/null | head -1)
[ -n "$sf" ] && storefile="$FDTMPDIR/$sf"
```

with

```bash
sf=$(ls -d "$FDTMPDIR"/tmux-cockpit/review/*/comments.md 2>/dev/null | head -1)
[ -n "$sf" ] && storefile="$sf"
```

and update the block's comment that says the store is a mktemp under `TMPDIR` to say it is `<session>/comments.md` under `XDG_STATE_HOME`. Any later `ls "$FDTMPDIR"` assertion in that block ("nothing left behind") becomes `ls -d "$FDTMPDIR"/tmux-cockpit/review/* 2>/dev/null` — an empty listing is the pass, because a delivered review's directory is removed.

Block `tmux-review: a clean-tree direct pane says so instead of opening an empty picker`: its control pane runs `tmux-review` bare — once the session dir exists that would create a directory under the developer's real `~/.local/state`. Prefix the command it sends with `XDG_STATE_HOME=$(shq_test "$(mktmpd)")` (a `MTSTATE=$(mktmpd)` variable declared beside `MT=`), so the suite never writes outside its own temp dirs.

Block `tmux-review: a REFUSED delivery keeps the pinned comments instead of deleting them`: same substitution for `RKTMPDIR` — `export XDG_STATE_HOME=...; printf 'RKTMP=%s\n' "$XDG_STATE_HOME"`, and wherever it lists `$RKTMPDIR` for the kept store, list `"$RKTMPDIR"/tmux-cockpit/review/*/comments.md`. Its comment ("Same TMPDIR redirect as the block above: review_pane()'s mktemp store is …") becomes "Same XDG_STATE_HOME redirect as the block above: review_pane()'s session dir lands under it".

- [ ] **Step 5: Run the three blocks**

Run: `make test-only PAT='survives the client|does not outlive|REFUSED delivery'`
Expected: all `PASS`, `0 failed`.

- [ ] **Step 6: README**

In the "Review the agent's changes" section, replace the bullet that begins `- **No review daemon, server, or state that outlives the popup.**` with:

```markdown
- **No review daemon, server, or port.** What *does* outlive the popup is
  one directory per review under
  `${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/review/<pane>-<root>/`,
  created `0700`, holding `comments.md` and its row snapshot. It exists so a
  dropped SSH client — which tears down a client-owned popup — cannot take a
  twenty-comment review with it: `prefix + r` on the same pane resumes it,
  marks and all. It is removed when the review is discarded or delivered and
  kept, and named in the message, when delivery is refused. There is no
  garbage collector; `rm -rf ~/.local/state/tmux-cockpit/review` is the purge.
```

- [ ] **Step 7: Lint and commit**

Run: `bash -n scripts/tmux-review && shellcheck -x scripts/tmux-review tests/run.sh`
Expected: no output from shellcheck.

```bash
git add scripts/tmux-review tests/run.sh README.md
git commit -m "feat: a review lives in a 0700 session dir keyed on pane + root, so the client that opened it can drop without taking it along"
```

---

### Task 3: `snapshot()`, `^R` re-snapshot, and the stale report

**Files:**
- Modify: `scripts/tmux-review` — new `snapshot()` and `stale_keys()` above `rows_mode()`; `run_loop()` snapshot call, resume notice, one new `--bind`, header; new `resnap()` above `# --- --compose`; dispatcher `--resnap`; `usage()`.
- Modify: `tests/run.sh` — one new block.
- Modify: `README.md` — the keys table and the paragraph after it.

**Interfaces:**
- Consumes: `session_dir`/`review_pane` from Task 2 (the resumed store), `until_cap`/`until_file` from Task 1.
- Produces: `snapshot <dir> <store>` — writes `<store>.rows` (0600, `O_EXCL`, replacing any previous one; on a refused write leaves no file). `stale_keys <store> <rows-file>` — prints, one per line, every `## path:line (sign)` header in the store that matches no row in the snapshot. `tmux-review --resnap <dir> <store>` — re-snapshots, prints the stale report and waits for one key **only when something is stale**, otherwise prints nothing and returns at once.

- [ ] **Step 1: Write the failing test**

Append before the Task 2 block (keep blocks in the order the features were built):

```bash
echo "== tmux-review --loop: ^R re-snapshots and names the comments the agent moved =="
# The rows are a snapshot (phase 1), so an agent writing mid-review cannot
# move a ● onto a line you never touched -- but it also means the list goes
# stale. ^R takes a new snapshot on purpose and says which comments no
# longer anchor to a changed line (they still deliver: the quoted fragment
# is what the agent finds them by). dev.md §2.2 is the repro.
if need_tmux "tmux-review ^R"; then :; else
RR="cockpit-rr-$$"; SOCKS="$SOCKS $RR"
RRDIR=$(mktmpd)
(
  cd "$RRDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
RRSTORE="$STATE/review-resnap-$$"
"$TMUX_REAL" -L "$RR" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
RRP=$("$TMUX_REAL" -L "$RR" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" "'$DIR/scripts/tmux-review' --loop '$RRDIR' '$RRSTORE'" Enter
until_cap "$RR" "$RRP" '*review>*' || bad "the picker comes up" "review>" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")"
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" "hold this"
until_cap "$RR" "$RRP" '*hold this*'
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" Enter
until_file "$RRSTORE" '*hold this*'
is "--resnap with nothing stale prints nothing and does not wait" "" \
   "$(timeout 5 "$DIR/scripts/tmux-review" --resnap "$RRDIR" "$RRSTORE" </dev/null 2>&1)"
# The agent inserts a line at the top: the pinned +CHANGED at 3 is now at 4.
printf 'TOP\na\nb\nCHANGED\n' > "$RRDIR/src.go"
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" "still here"
until_cap "$RR" "$RRP" '*still here*'
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" C-u    # readline: clear the typed text again
case "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")" in
    *"●src.go:3 (+)"*) ok "until ^R the list is the snapshot: the mark stays where it was pinned" ;;
    *) bad "until ^R the list is the snapshot: the mark stays where it was pinned" "●src.go:3 (+)" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")" ;;
esac
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" C-r
if until_cap "$RR" "$RRP" '*no longer anchor*## src.go:3 (+)*'; then ok "^R names the comment the agent moved"
else bad "^R names the comment the agent moved" "no longer anchor ... ## src.go:3 (+)" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")"; fi
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" Space
if until_cap "$RR" "$RRP" '*src.go:4 (+)*'; then ok "and the list shows the agent's new rows"
else bad "and the list shows the agent's new rows" "src.go:4 (+)" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")"; fi
case "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")" in
    *"●"*) bad "a moved comment marks no row" "no ●" "$("$TMUX_REAL" -L "$RR" capture-pane -p -t "$RRP")" ;;
    *)     ok "a moved comment marks no row" ;;
esac
is "the moved comment is kept in the store, not dropped" "1" "$(grep -c '^## src.go:3 (+)' "$RRSTORE")"
is "--resnap names exactly the stale headers" "## src.go:3 (+)" \
   "$("$DIR/scripts/tmux-review" --resnap "$RRDIR" "$RRSTORE" </dev/null 2>&1 | grep '^## ')"
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" Escape
until_cap "$RR" "$RRP" '*discard 1 comment*'
"$TMUX_REAL" -L "$RR" send-keys -t "$RRP" Down Enter
"$TMUX_REAL" -L "$RR" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 2: Run it to see it fail**

Run: `make test-only PAT='re-snapshots'`
Expected: "--resnap with nothing stale" FAILs with `got [unknown option: --resnap ...]`; "^R names the comment" FAILs.

- [ ] **Step 3: Implement**

Above `rows_mode()` add:

```bash
# The snapshot: lines_mode's rows, written ONCE per popup, re-marked after.
# `umask 077` + `set -C` in the writing subshell, the discipline deliver()
# uses for the oversized-review file: born 0600, O_EXCL refuses a path that
# already exists -- a planted symlink included -- and the `rm -f` first is
# for the resumed session whose previous snapshot would otherwise be refused
# too. A refused write leaves no file, and rows_mode derives from the tree.
snapshot() {                  # <dir> <store>
    [ -n "${2:-}" ] || return 0
    rm -f -- "$2.rows"
    (umask 077; set -C; lines_mode "$1" > "$2.rows") 2>/dev/null || rm -f -- "$2.rows"
}

# Comments whose "## path:line (sign)" header matches no row in the
# snapshot: the agent moved the line since the comment was pinned (dev lens
# §2.2). They still deliver -- the quoted fragment is what the agent anchors
# on -- so this only NAMES them. Streams told apart by FILENAME, as in
# mark_rows: an empty store makes NR == FNR lie.
stale_keys() {                # <store> <rows-file>  -> stale headers, one per line
    local st=${1:-/dev/null} rows=${2:-/dev/null}
    [ -f "$st" ] || st=/dev/null
    [ -f "$rows" ] || rows=/dev/null
    LC_ALL=C awk -F'\t' '
        FILENAME == ARGV[1] { if (substr($0, 1, 3) == "## ") h[++n] = $0; next }
        NF == 0 { next }
        {
            sign = (substr($2, 1, 1) == "-") ? "-" : "+"
            num = $2; sub(/^-/, "", num)
            k["## " $1 ":" num " (" sign ")"] = 1
        }
        END { for (i = 1; i <= n; i++) if (!(h[i] in k)) print h[i] }
    ' "$st" "$rows"
}
```

In `run_loop()`, replace the `if [ -n "$store" ]; then ... rm -f ... fi` snapshot block (the one Task 2 edited) with:

```bash
    snapshot "$dir" "$store"
    # A resumed session (Task 2's dir found comments already pinned) says so,
    # with the count that matters: how many of them the agent has moved.
    if [ -n "$store" ] && [ -s "$store" ]; then
        local nres nstale
        nres=$(grep -c '^## ' -- "$store"); nstale=$(stale_keys "$store" "$store.rows" | grep -c '^## ')
        tmux display-message "tmux-review: resumed $nres pinned comment(s)$([ "$nstale" -gt 0 ] && printf ', %s no longer anchor (^R lists them)' "$nstale")" 2>/dev/null
    fi
```

Move the long comment that stood above the old block (from `# Build the initial list ONCE` down to `# create, with no create-then-chmod window.` and the `# A refused write ...` paragraph) onto `snapshot()` where its subject now lives — keep its first paragraph ("Build the initial list ONCE ...") in `run_loop`, since that is still about `rows_out`.

Still in `run_loop()`, add the command and the bind:

```bash
    resnap_cmd="$(shq "$SELF") --resnap $(shq "$dir") $(shq "$store")"
```

(declare `resnap_cmd` in the `local rows_cmd attach_cmd ...` line) and, after the `alt-enter` bind line:

```bash
                  --bind "ctrl-r:execute(${resnap_cmd})+reload(${rows_cmd})" \
```

Change the header's first line from `↵ pin the typed comment   ^S deliver   esc discard (asks first)` to `↵ pin the typed comment   ^R refresh the diff   ^S deliver   esc discard (asks first)`.

Above `# --- --compose` add:

```bash
# --- --resnap ----------------------------------------------------------------
# Runs inside fzf's execute(): the terminal is ours until we return, and
# reload() re-marks the new snapshot after. Nothing stale: say nothing and
# hand the terminal straight back. Something stale: name it and wait for a
# key, because reload() will redraw over it the moment we return.
resnap() {                    # <dir> <store>
    local dir=$1 store=$2 stale n
    [ -n "$dir" ] && [ -n "$store" ] || { usage; exit 2; }
    snapshot "$dir" "$store"
    stale=$(stale_keys "$store" "$store.rows")
    n=$(printf '%s\n' "$stale" | grep -c '^## ')
    [ "$n" -gt 0 ] || return 0
    printf 'refreshed — %d comment(s) no longer anchor to a changed line (they still deliver, quoted):\n%s\n\npress any key\n' "$n" "$stale"
    read -r -n 1 -s
    return 0
}
```

Dispatcher: after the `--loop)` arm add

```bash
    --resnap)
        shift; resnap "${1:-}" "${2:-}" ;;
```

`usage()`: after the `--loop` line add `  tmux-review --resnap <dir> <store>     re-snapshot the diff; names the comments that moved` and change the picker line to `In the picker: ↵ pin the typed comment on the row under the cursor   ^R refresh   ^S deliver   esc discard`.

- [ ] **Step 4: Run the block, then the whole loop group**

Run: `make test-only PAT='re-snapshots|tmux-review --loop|survives the client|born 0600'`
Expected: all `PASS`, `0 failed`. (The `born 0600` block asserts the snapshot's mode and symlink refusal — it must still pass through `snapshot()`.)

- [ ] **Step 5: README**

In the keys table add a row after `^S`: `| `^R` | — | re-snapshot the diff; names any comment the agent moved (it still delivers, quoted) |`. After the paragraph ending `one Enter on the whole diff is the rubber stamp this replaces.` add:

```markdown
The rows are a **snapshot** taken when the popup opens, so an agent that
keeps editing cannot move a `●` onto a line you never touched. When you do
want its newest lines, `^R` takes a fresh snapshot on purpose and names the
comments that no longer sit on a changed line — they still deliver, because
the quoted fragment is what the agent finds them by. A resumed review says the
same thing on open: `resumed 5 pinned comment(s), 2 no longer anchor`.
```

- [ ] **Step 6: Lint and commit**

Run: `bash -n scripts/tmux-review && shellcheck -x scripts/tmux-review tests/run.sh`
Expected: clean.

```bash
git add scripts/tmux-review tests/run.sh README.md
git commit -m "feat: ^R re-snapshots the diff and names the comments the agent moved; a resumed review reports the same on open"
```

---

### Task 4: `^X` delete under the cursor, the count marker, `^L` list

**Files:**
- Modify: `scripts/tmux-review` — `LIBDIR` beside `OPENPATH`; `mark_rows()` count; new `detach()` after `attach()`; two binds + header in `run_loop()`; dispatcher `--detach`; `usage()`.
- Modify: `tests/run.sh` — one new block.
- Modify: `README.md` — keys table.

**Interfaces:**
- Consumes: `parse_store FILE` / `format_store FILE` over `STORE` / `STORE_N` (`scripts/lib/review-store.sh`); `until_cap`/`until_file`.
- Produces: `tmux-review --detach <store> <path> <line>` — removes every record anchored at `path:line (sign)` (`line` carries the `-` prefix for a deletion, exactly as `--attach` receives it), rewrites the store atomically through `format_store`; no-op on an empty path/line/store. `LIBDIR` — the `scripts/lib` directory resolved through the one symlink `install.sh` may have put on `$PATH`.

- [ ] **Step 1: Write the failing test**

Append before Task 3's block:

```bash
echo "== tmux-review --loop: ^X deletes the comments under the cursor, the marker counts them, ^L lists them =="
# The store had a writer and no reader: three comments on one line showed
# one dot, and nothing could take one back short of discarding the batch.
# parse_store (lib/review-store.sh) is the reader; --detach is its first
# caller. ^L shows the store where the preview is -- no nested picker, and
# no printable key: in this loop every printable key is comment text.
if need_tmux "tmux-review ^X"; then :; else
RX="cockpit-rx-$$"; SOCKS="$SOCKS $RX"
RXDIR=$(mktmpd)
(
  cd "$RXDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
RXSTORE="$STATE/review-detach-$$"
# --detach on its own, through a symlink the way install.sh puts it on PATH:
# the lib has to be found beside the SCRIPT, not beside the link.
RXBIN=$(mktmpd); ln -s "$DIR/scripts/tmux-review" "$RXBIN/tmux-review"
printf '## src.go:3 (+)\n> CHANGED\nfirst\n\n## src.go:3 (+)\n> CHANGED\nsecond\n\n## other.go:9 (-)\n> gone\nkeep me\n\n' > "$RXSTORE"
"$RXBIN/tmux-review" --detach "$RXSTORE" src.go 3
is "--detach removes every record on that anchor and keeps the rest byte-for-byte" \
   $'## other.go:9 (-)\n> gone\nkeep me' "$(cat "$RXSTORE")"
"$RXBIN/tmux-review" --detach "$RXSTORE" other.go -9
is "a deletion's anchor is matched with its sign" "0" "$(grep -c '^## ' "$RXSTORE")"
"$RXBIN/tmux-review" --detach "$RXSTORE" "" ""; rxrc=$?
is "an empty anchor is a no-op, exit 0" "0" "$rxrc"
: > "$RXSTORE"
"$TMUX_REAL" -L "$RX" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
RXP=$("$TMUX_REAL" -L "$RX" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" "'$DIR/scripts/tmux-review' --loop '$RXDIR' '$RXSTORE'" Enter
until_cap "$RX" "$RXP" '*review>*' || bad "the picker comes up" "review>" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" "one"; until_cap "$RX" "$RXP" '*review> one*'
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" Enter; until_file "$RXSTORE" '*one*'
if until_cap "$RX" "$RXP" '*●src.go:3 (+)*'; then ok "one comment: the dot"
else bad "one comment: the dot" "●src.go:3 (+)" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"; fi
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" "two"; until_cap "$RX" "$RXP" '*review> two*'
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" Enter; until_file "$RXSTORE" '*two*'
if until_cap "$RX" "$RXP" '*2src.go:3 (+)*'; then ok "two comments on one row: the count, one column wide"
else bad "two comments on one row: the count, one column wide" "2src.go:3 (+)" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"; fi
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" C-l
if until_cap "$RX" "$RXP" '*## src.go:3 (+)*one*## src.go:3 (+)*two*'; then ok "^L shows the store in the preview"
else bad "^L shows the store in the preview" "both records" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"; fi
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" C-x
for i in $(seq 1 40); do grep -q '^## ' "$RXSTORE" 2>/dev/null || break; sleep 0.1; done
is "^X removes every comment under the cursor" "0" "$(grep -c '^## ' "$RXSTORE")"
if until_cap "$RX" "$RXP" '* src.go:3 (+)*'; then ok "and the row is unmarked again"
else bad "and the row is unmarked again" " src.go:3 (+)" "$("$TMUX_REAL" -L "$RX" capture-pane -p -t "$RXP")"; fi
"$TMUX_REAL" -L "$RX" send-keys -t "$RXP" Escape
for i in $(seq 1 40); do [ -e "$RXSTORE" ] || break; sleep 0.1; done
is "esc with nothing pinned exits without asking and drops the empty store" "gone" "$([ -e "$RXSTORE" ] && echo present || echo gone)"
"$TMUX_REAL" -L "$RX" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 2: Run it to see it fail**

Run: `make test-only PAT='deletes the comments under the cursor'`
Expected: "--detach removes every record" FAILs (`unknown option: --detach`); the loop assertions after "one comment: the dot" fail.

- [ ] **Step 3: Implement**

Beside `OPENPATH=` at the top of `scripts/tmux-review`:

```bash
# The lib sits beside the SCRIPT. install.sh links each script into ~/bin by
# its absolute path, so one readlink hop is the whole resolution; a plain
# run (no link) has nothing to read and keeps $SELF's own directory.
LIBDIR="$(dirname "$(readlink "${BASH_SOURCE[0]}" 2>/dev/null || printf '%s' "$SELF")")/lib"
```

In `mark_rows()`'s awk, change `c[$0] = 1` to `c[$0]++` and replace

```awk
            m = (("## " $1 ":" num " (" sign ")") in c) ? "●" : " "
```

with

```awk
            # One column wide whatever the count: ● for one, the digit past
            # that, 9 for anything more -- so a marked row never shifts.
            n = c["## " $1 ":" num " (" sign ")"] + 0
            m = (n == 0) ? " " : (n == 1) ? "●" : (n > 9) ? "9" : n ""
```

After `attach()` add:

```bash
# The inverse of attach() for one anchor: every record at path:line (sign)
# goes, the rest are written back byte-identical (parse is the declared
# inverse of format -- the round-trip test in tests/run.sh pins it). This
# and nothing else in the script rewrites the store; attach() appends.
detach() {                    # <store> <path> <line-with-sign>
    local store=$1 path=$2 line=$3 sign=+ num i j=0 k
    [ -n "$path" ] && [ -n "$line" ] && [ -s "$store" ] || return 0
    case "$line" in -*) sign=-; num=${line#-} ;; *) num=$line ;; esac
    # shellcheck source=lib/review-store.sh
    . "$LIBDIR/review-store.sh"
    declare -A STORE=() OUT=(); STORE_N=0
    parse_store "$store"
    # A non-empty store that parses to 0 records was set aside as .bak by
    # parse_store: never write 0 records back over it.
    [ "$STORE_N" -gt 0 ] || return 0
    for ((i = 0; i < STORE_N; i++)); do
        [ "${STORE[$i.path]}" = "$path" ] && [ "${STORE[$i.line]:-}" = "$num" ] \
            && [ "${STORE[$i.side]}" = "$sign" ] && continue
        for k in path line end side frag body; do
            [ -n "${STORE[$i.$k]+x}" ] && OUT[$j.$k]=${STORE[$i.$k]}
        done
        j=$((j + 1))
    done
    STORE=(); for k in "${!OUT[@]}"; do STORE[$k]=${OUT[$k]}; done; STORE_N=$j
    format_store "$store"
}
```

In `run_loop()`: declare `detach_cmd` in the `local` line and set

```bash
    detach_cmd="$(shq "$SELF") --detach $(shq "$store") {1} {2}"
```

Add two binds after the `ctrl-r` bind:

```bash
                  --bind "ctrl-x:execute-silent(${detach_cmd})+reload(${rows_cmd})" \
                  --bind "ctrl-l:execute(${list_cmd})" \
```

with `list_cmd="$(shq "$SELF") --list $(shq "$store")"` and a `list_store()` subcommand shaped like `resnap()` (inside fzf's `execute()` stdout is fzf's own pipe, so it `exec >/dev/tty` when stdin is a tty, `cat`s the store, prints `(press any key)` and reads one key). The plan first had `show-preview+preview(cat …)`, then `change-preview-window(+1)` plus a `focus` bind restoring the diff offset — the final whole-branch review measured that the `focus` bind un-hides the preview after `^/`, so the preview window is not used for `^L` at all. (The preview test for `^L` therefore asserts the store text in the terminal, then a key returns to the list; a fail-first test pins that `^/` stays hidden across a cursor move.)

Header first line becomes: `↵ pin the typed comment   ^X delete   ^L list   ^R refresh   ^S deliver   esc discard (asks first)`.

Dispatcher, after `--attach)`:

```bash
    --detach)
        shift; detach "${1:-}" "${2:-}" "${3:-}" ;;
```

`usage()` picker line: `In the picker: ↵ pin the typed comment on the row under the cursor   ^X delete its comments   ^L list   ^R refresh   ^S deliver   esc discard`.

- [ ] **Step 4: Run the group**

Run: `make test-only PAT='deletes the comments|re-snapshots|tmux-review --loop|review-store|re-marking after a comment'`
Expected: all `PASS`, `0 failed`. The `re-marking after a comment reads the cache, not git` block has a timing tooth — it must stay green with the count awk.

- [ ] **Step 5: README**

Keys table, after the `^R` row:

```markdown
| `^X` | — | delete the comment(s) on the row under the cursor |
| `^L` | — | show every pinned comment where the preview is |
```

In the paragraph that begins `Inside the review:`, after `marks the row \`●\`` add `(the digit instead, when more than one comment sits there)`.

- [ ] **Step 6: Lint and commit**

Run: `bash -n scripts/tmux-review && shellcheck -x scripts/tmux-review tests/run.sh`
Expected: clean (`# shellcheck source=` lets `-x` follow the lib).

```bash
git add scripts/tmux-review tests/run.sh README.md
git commit -m "feat: ^X deletes the comments under the cursor through the store parser, the marker counts them, ^L lists them in the preview"
```

---

### Task 5: `^E` — a multi-line comment through `$EDITOR`

**Files:**
- Modify: `scripts/tmux-review` — new `edit_comment()` after `detach()`; bind + header in `run_loop()`; dispatcher `--edit`; `usage()`.
- Modify: `tests/run.sh` — one new block.
- Modify: `README.md` — keys table, one sentence.

**Interfaces:**
- Consumes: `attach <store> <path> <line> <fragment> <comment> <dir>` (unchanged); `until_cap`/`until_file`.
- Produces: `tmux-review --edit <store> <path> <line> <fragment> <query> <dir>` — opens `${VISUAL:-${EDITOR:-vi}}` on a 0600 temp file under `${TMPDIR:-/tmp}` (never beside the store: a crashed editor's leftover must not block the session dir's `rmdir`) pre-filled with `<query>`, then attaches the file's contents (an empty file attaches nothing).

- [ ] **Step 1: Write the failing test**

Append before Task 4's block:

```bash
echo "== tmux-review --loop: ^E hands the comment to \$EDITOR and pins what comes back =="
# The fzf query line is a one-line box; a thorough comment cost more than a
# shallow one. ^E opens $EDITOR on what was typed so far and pins the file.
if need_tmux "tmux-review ^E"; then :; else
RE="cockpit-re-$$"; SOCKS="$SOCKS $RE"
REDIR=$(mktmpd)
(
  cd "$REDIR" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .
  printf 'a\nb\n' > src.go; git add -A; git commit -q -m init
  printf 'a\nb\nCHANGED\n' > src.go
) >/dev/null 2>&1
RESTORE="$STATE/review-edit-$$"; RETMP=$(mktmpd)
REED=$(mktmpf); printf '#!/bin/sh\nprintf "from the editor\\nsecond line\\n" >> "$1"\n' > "$REED"; chmod +x "$REED"
"$TMUX_REAL" -L "$RE" -f /dev/null new-session -d -s t -x 100 -y 30 'exec -a claude bash' >/dev/null 2>&1
REP=$("$TMUX_REAL" -L "$RE" list-panes -t t: -F '#{pane_id}')
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" \
    "TMPDIR=$(printf '%q' "$RETMP") VISUAL= EDITOR=$(printf '%q' "$REED") '$DIR/scripts/tmux-review' --loop '$REDIR' '$RESTORE'" Enter
until_cap "$RE" "$REP" '*review>*' || bad "the picker comes up" "review>" "$("$TMUX_REAL" -L "$RE" capture-pane -p -t "$REP")"
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" "typed first"; until_cap "$RE" "$REP" '*typed first*'
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" C-e
if until_file "$RESTORE" '*## src.go:3 (+)*> CHANGED*typed first*from the editor*second line*'; then
    ok "^E pre-fills the typed text, the editor's file is the comment, multi-line"
else bad "^E pre-fills the typed text, the editor's file is the comment, multi-line" \
     "typed first / from the editor / second line" "$(cat "$RESTORE" 2>/dev/null)"; fi
until_cap "$RE" "$REP" '*●src.go:3 (+)*' && ok "and the row is marked, query cleared" \
    || bad "and the row is marked, query cleared" "●src.go:3 (+)" "$("$TMUX_REAL" -L "$RE" capture-pane -p -t "$REP")"
case "$("$TMUX_REAL" -L "$RE" capture-pane -p -t "$REP")" in
    *"review> typed first"*) bad "the query is cleared after ^E" "review> " "still typed first" ;;
    *) ok "the query is cleared after ^E" ;;
esac
is "no editor temp file is left behind, and none went beside the store" "" "$(ls "$RETMP" "$RESTORE".edit.* 2>/dev/null)"
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" Escape
until_cap "$RE" "$REP" '*discard 1 comment*'
"$TMUX_REAL" -L "$RE" send-keys -t "$REP" Down Enter
"$TMUX_REAL" -L "$RE" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 2: Run it to see it fail**

Run: `make test-only PAT='hands the comment to'`
Expected: "^E pre-fills" FAILs (`got []`).

- [ ] **Step 3: Implement**

After `detach()`:

```bash
# Runs inside fzf's execute(): the terminal is the editor's until it exits.
# The file starts as whatever was typed so far, so ^E continues a comment
# rather than restarting it; an editor that leaves it empty pins nothing
# (attach's own rule). Under $TMPDIR, not beside the store: a killed popup
# leaves the editor's file behind, and a leftover inside the session dir
# would block its rmdir for good. Word-splitting on purpose: EDITOR="code --wait".
edit_comment() {              # <store> <path> <line> <fragment> <query> <dir>
    local store=$1 path=$2 line=$3 fragment=$4 query=$5 dir=$6 tmp
    [ -n "$path" ] && [ -n "$store" ] || return 0
    tmp=$(umask 077; mktemp "${TMPDIR:-/tmp}/cockpit-review-edit.XXXXXX") || return 0
    [ -n "$query" ] && printf '%s\n' "$query" > "$tmp"
    # shellcheck disable=SC2086
    ${VISUAL:-${EDITOR:-vi}} "$tmp"
    attach "$store" "$path" "$line" "$fragment" "$(cat -- "$tmp")" "$dir"
    rm -f -- "$tmp"
}
```

In `run_loop()`: declare `edit_cmd` in the `local` line, set

```bash
    edit_cmd="$(shq "$SELF") --edit $(shq "$store") {1} {2} {3} {q} $(shq "$dir")"
```

and add, after the `ctrl-l` bind:

```bash
                  --bind "ctrl-e:execute(${edit_cmd})+reload(${rows_cmd})+clear-query" \
```

Header becomes two lines that both fit an 82-column popup (70% of 120): `↵ pin the typed comment   ^S deliver   esc discard (asks first)` / `^E editor  ^X delete  ^L list  ^R refresh  ^/ preview  alt-u/alt-d scroll` (the whole-branch review measured the one-line form at 110 columns, cutting `^S deliver` and `esc discard` at the default width).

Dispatcher, after `--detach)`:

```bash
    --edit)
        shift; edit_comment "${1:-}" "${2:-}" "${3:-}" "${4:-}" "${5:-}" "${6:-}" ;;
```

`usage()` picker line gains `^E editor` after `↵ pin ...`.

`cockpit.tmux`: **nothing.** (The plan first had a `display-popup -e EDITOR=… -e VISUAL=…` pass-through here, on the ux lens's claim that popup children see only a server-era environment. Review of the shipped code measured it on tmux 3.7c: under every documented load path `cockpit.tmux` is itself a `run-shell` child of the same server, so the popup child already inherits the same `EDITOR`, and a baked `-e` froze the load-time value over a later `tmux set-environment -g EDITOR`. Deleted. The README names the two real knobs.)

- [ ] **Step 4: Run the group**

Run: `make test-only PAT='hands the comment to|tmux-review --loop|shipped config'`
Expected: all `PASS`, `0 failed`.

- [ ] **Step 5: README**

Keys table, after `^L`: `| `^E` | — | open \`$EDITOR\` on the typed text; the file is the comment (multi-line) |`. In the `Inside the review:` paragraph, after the `alt-↵` sentence add: `\`^E\` hands what you have typed to \`$EDITOR\` and pins the file that comes back, so a comment that needs three lines costs no more than one. The popup's children see the tmux *server's* environment: export \`EDITOR\` before the server starts, or \`tmux set-environment -g EDITOR nvim\` for a running one.`

- [ ] **Step 6: Lint and commit**

Run: `bash -n scripts/tmux-review && shellcheck -x scripts/tmux-review tests/run.sh`
Expected: clean.

```bash
git add scripts/tmux-review tests/run.sh README.md
git commit -m "feat: ^E writes the comment in \$EDITOR"
```

---

### Task 7: Project-to-project moves — two agents, one agent that cd's, the terminal window, and the rig

Added 2026-08-25 at the owner's request after the final review: moving between projects must leave every review where it was and cost the status line nothing extra.

**Files:**
- Modify: `tests/run.sh` — one new block, appended before the Task 2 block.
- Modify: `tests/freeze-rig/gate.sh` — a second measurement after the status-redraw one; `tests/freeze-rig/RESULTS.txt` gets the numbers.
- Modify: this plan's Task 6 (the any-cwd gate step, below).

**Interfaces:**
- Consumes: `session_dir <pane> <root>` (Task 2), `until_cap`/`until_file` (Task 1), the fleet queue (`tmux-review` from a non-agent pane; typing filters on `repo⑂branch`; `↵` opens `review_pane` for the row).
- Produces: nothing new in the scripts — this task is tests and a rig number. If any section fails, the fix goes to `scripts/tmux-review` and is reported, not worked around in the test.

- [ ] **Step 1: The suite block**

Append before the `== tmux-review: a review survives the client that opened it ==` block:

```bash
echo "== tmux-review: moving between projects leaves every review where it was =="
# Three moves the owner makes all day: prefix r on one agent, then on another
# in a different repo; one agent whose cwd moves to another repo (tmux cannot
# change a pane's cwd from outside, so the pane is respawned in the new dir --
# same pane id, which is what the session key is made of); and from a plain
# terminal window through the fleet queue back to a review. Every session dir
# is keyed on pane + root, so none of these can read another's comments.
if need_tmux "review across projects"; then :; else
PM="cockpit-pm-$$"; SOCKS="$SOCKS $PM"
PMSTATE=$(mktmpd); PMBASE=$(mktmpd)
ALPHA="$PMBASE/alpha"; BRAVO="$PMBASE/bravo"
for r in "$ALPHA" "$BRAVO"; do
  f=$(basename "$r").go
  ( mkdir -p "$r" && cd "$r" || exit
    export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
    git init -q -b main .; printf 'a\nb\n' > "$f"; git add -A; git commit -q -m init
    printf 'a\nb\nCHANGED\n' > "$f" ) >/dev/null 2>&1
done
"$TMUX_REAL" -L "$PM" -f /dev/null new-session -d -s t -x 120 -y 30 -c "$ALPHA" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$PM" set -g @cockpit_agents_glob '|claude|codex|aider|cursor|opencode|gemini|crush|' >/dev/null 2>&1
PMA=$("$TMUX_REAL" -L "$PM" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$PM" new-window -d -t t: -c "$BRAVO" 'exec -a claude bash' >/dev/null 2>&1
PMB=$("$TMUX_REAL" -L "$PM" list-panes -t t:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$PM" new-window -d -t t: -c "$PMBASE" 'exec bash' >/dev/null 2>&1   # the terminal window
pm_shell() {                  # a fresh control pane in the terminal window, ready for a command
    local p i
    p=$("$TMUX_REAL" -L "$PM" split-window -t t:2 -P -F '#{pane_id}' 'exec bash')
    for i in $(seq 1 40); do
        [ "$("$TMUX_REAL" -L "$PM" display-message -p -t "$p" '#{pane_current_command}')" = bash ] && break
        sleep 0.1
    done
    printf '%s' "$p"
}
pm_review() {                 # <control-pane> <agent-pane>   opens that agent's review from the control pane
    "$TMUX_REAL" -L "$PM" send-keys -t "$1" \
        "XDG_STATE_HOME=$(printf '%q' "$PMSTATE") '$DIR/scripts/tmux-review' -p $2" Enter
    until_cap "$PM" "$1" '*review>*'
}
pm_sess() {                   # <agent-pane> <root>   the session dir the script will use
    printf '%s/tmux-cockpit/review/%s-%s' "$PMSTATE" "${1#%}" \
        "$(printf '%s' "$(git -C "$2" rev-parse --show-toplevel)" | cksum | cut -d' ' -f1)"
}
pm_cap() { "$TMUX_REAL" -L "$PM" capture-pane -p -t "$1"; }

# (a) two agents, two repos
C=$(pm_shell); pm_review "$C" "$PMA" || bad "A's review comes up" "review>" "$(pm_cap "$C")"
"$TMUX_REAL" -L "$PM" send-keys -t "$C" "for alpha"; until_cap "$PM" "$C" '*for alpha*'
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Enter
until_file "$(pm_sess "$PMA" "$ALPHA")/comments.md" '*for alpha*' && ok "a comment pinned on A lands in A's session" \
    || bad "a comment pinned on A lands in A's session" "for alpha" "$(cat "$(pm_sess "$PMA" "$ALPHA")/comments.md" 2>/dev/null)"
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"                                   # the client drops; A's review stays
C=$(pm_shell); pm_review "$C" "$PMB" || bad "B's review comes up" "review>" "$(pm_cap "$C")"
if until_cap "$PM" "$C" '*bravo.go:3 (+)*'; then ok "prefix r on B shows B's repo"; else bad "prefix r on B shows B's repo" "bravo.go:3 (+)" "$(pm_cap "$C")"; fi
case "$(pm_cap "$C")" in *●*) bad "and none of A's marks" "no ●" "$(pm_cap "$C")" ;; *) ok "and none of A's marks" ;; esac
is "B's session is its own, empty" "0" "$(grep -c '^## ' "$(pm_sess "$PMB" "$BRAVO")/comments.md" 2>/dev/null)"
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
C=$(pm_shell); pm_review "$C" "$PMA"
if until_cap "$PM" "$C" '*●alpha.go:3 (+)*'; then ok "back on A, A's comment is resumed"; else bad "back on A, A's comment is resumed" "●alpha.go:3 (+)" "$(pm_cap "$C")"; fi
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
is "two agents in two repos: two session dirs" "2" "$(ls -d "$PMSTATE"/tmux-cockpit/review/* 2>/dev/null | wc -l)"

# (b) one agent whose cwd moves to the other repo (respawned in place: same pane id)
"$TMUX_REAL" -L "$PM" respawn-pane -k -t "$PMB" -c "$ALPHA" 'exec -a claude bash'
C=$(pm_shell); pm_review "$C" "$PMB"
if until_cap "$PM" "$C" '*alpha.go:3 (+)*'; then ok "an agent that moved to alpha reviews alpha"; else bad "an agent that moved to alpha reviews alpha" "alpha.go:3 (+)" "$(pm_cap "$C")"; fi
case "$(pm_cap "$C")" in *●*) bad "with none of A's marks: same root, another pane, another session" "no ●" "$(pm_cap "$C")" ;; *) ok "with none of A's marks: same root, another pane, another session" ;; esac
"$TMUX_REAL" -L "$PM" send-keys -t "$C" "B in alpha"; until_cap "$PM" "$C" '*B in alpha*'
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Enter
until_file "$(pm_sess "$PMB" "$ALPHA")/comments.md" '*B in alpha*' || bad "B's alpha comment lands in B's alpha session" "B in alpha" "$(cat "$(pm_sess "$PMB" "$ALPHA")/comments.md" 2>/dev/null)"
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
"$TMUX_REAL" -L "$PM" respawn-pane -k -t "$PMB" -c "$BRAVO" 'exec -a claude bash'
C=$(pm_shell); pm_review "$C" "$PMB"
if until_cap "$PM" "$C" '*bravo.go:3 (+)*'; then ok "moved back to bravo, it reviews bravo"; else bad "moved back to bravo, it reviews bravo" "bravo.go:3 (+)" "$(pm_cap "$C")"; fi
case "$(pm_cap "$C")" in *●*) bad "and the alpha comment does not follow it" "no ●" "$(pm_cap "$C")" ;; *) ok "and the alpha comment does not follow it" ;; esac
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
"$TMUX_REAL" -L "$PM" respawn-pane -k -t "$PMB" -c "$ALPHA" 'exec -a claude bash'
C=$(pm_shell); pm_review "$C" "$PMB"
if until_cap "$PM" "$C" '*●alpha.go:3 (+)*'; then ok "back in alpha, its own alpha comment is resumed"; else bad "back in alpha, its own alpha comment is resumed" "●alpha.go:3 (+)" "$(pm_cap "$C")"; fi
"$TMUX_REAL" -L "$PM" kill-pane -t "$C"
"$TMUX_REAL" -L "$PM" respawn-pane -k -t "$PMB" -c "$BRAVO" 'exec -a claude bash'
is "one agent, two roots: one more session dir, none shared" "3" "$(ls -d "$PMSTATE"/tmux-cockpit/review/* 2>/dev/null | wc -l)"

# (c) from the terminal window, through the fleet queue, back to A's review
C=$(pm_shell)
"$TMUX_REAL" -L "$PM" send-keys -t "$C" "XDG_STATE_HOME=$(printf '%q' "$PMSTATE") '$DIR/scripts/tmux-review'" Enter
if until_cap "$PM" "$C" '*fleet>*'; then ok "prefix r from the terminal window is the fleet queue"; else bad "prefix r from the terminal window is the fleet queue" "fleet>" "$(pm_cap "$C")"; fi
if until_cap "$PM" "$C" '*alpha⑂main*bravo⑂main*'; then ok "both projects' agents are listed"; else bad "both projects' agents are listed" "alpha⑂main and bravo⑂main" "$(pm_cap "$C")"; fi
"$TMUX_REAL" -L "$PM" send-keys -t "$C" "alpha"; until_cap "$PM" "$C" '*fleet> alpha*'
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Enter
if until_cap "$PM" "$C" '*●alpha.go:3 (+)*'; then ok "picking A in the queue resumes A's review, marks and all"; else bad "picking A in the queue resumes A's review, marks and all" "●alpha.go:3 (+)" "$(pm_cap "$C")"; fi
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Escape
until_cap "$PM" "$C" '*discard 1 comment*'
"$TMUX_REAL" -L "$PM" send-keys -t "$C" Down Enter
for i in $(seq 1 40); do [ -e "$(pm_sess "$PMA" "$ALPHA")" ] || break; sleep 0.1; done
is "discarding A from the queue path removes only A's dir" "2" "$(ls -d "$PMSTATE"/tmux-cockpit/review/* 2>/dev/null | wc -l)"
unset -f pm_shell pm_review pm_sess pm_cap
"$TMUX_REAL" -L "$PM" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 2: Run it**

Run: `make test-only PAT='moving between projects'`
Expected: every line `PASS`. This block is not fail-first by design — it pins behaviour Tasks 2–5 already built; a red line here is a bug in `scripts/tmux-review` (report it, fix it there, re-run), never a test to weaken.

- [ ] **Step 3: The rig — what a move costs**

In `tests/freeze-rig/gate.sh`, after the status-redraw loop's `min` is known and before the `budget=` line, add a second measurement with the same rule (a move may walk the server at most once):

```bash
# A MOVE — window to window across projects, agent window to a plain terminal
# window, pane to pane — is a status redraw the owner triggers by hand all
# day. Same rule, same budget: at most one server walk per move. Half the
# windows' panes sit in one repo and half in another so the move really
# crosses projects; window W+1 is a plain terminal (no agent).
srv new-window -d -t s1: -n term -c "$LOGDIR" 'bash --norc' 2>/dev/null
w=0
while read -r pid; do
    case $((w % 2)) in 0) d="$LOGDIR/alpha" ;; *) d="$LOGDIR/bravo" ;; esac
    mkdir -p "$d"; srv respawn-pane -k -c "$d" -t "$pid" "$AGENT" 2>/dev/null
    w=$((w + 1))
done < <(srv list-panes -t s1:1 -F '#{pane_id}'; srv list-panes -t s1:2 -F '#{pane_id}')
sleep 1
move_cost() {                 # <tmux move command...>  -> min pane visits over REPS
    local i before after n m=""
    for ((i=0; i<REPS; i++)); do
        srv select-window -t s1:0 2>/dev/null; sleep "$SETTLE"
        before=$(grep -c 'pane loop: %' "$LF" 2>/dev/null); before=${before:-0}
        srv "$@" 2>/dev/null; sleep "$SETTLE"
        after=$(grep -c 'pane loop: %' "$LF" 2>/dev/null); after=${after:-0}
        n=$((after - before)); if [ -z "$m" ] || [ "$n" -lt "$m" ]; then m=$n; fi
    done
    printf '%s' "$m"
}
mv_proj=$(move_cost select-window -t s1:2)          # alpha window -> bravo window
mv_term=$(move_cost select-window -t s1:term)       # agent window -> terminal window
mv_pane=$(move_cost select-pane -t s1:0.1)          # pane -> pane inside a window
```

and extend the report + verdict at the bottom:

```bash
printf 'pane visits per move: project→project %s   agent→terminal window %s   pane→pane %s\n' \
    "$mv_proj" "$mv_term" "$mv_pane"
worst=$min; for n in "$mv_proj" "$mv_term" "$mv_pane"; do [ "$n" -gt "$worst" ] && worst=$n; done
if [ "$worst" -le "$budget" ]; then echo "PASS"; exit 0; fi
awk -v m="$worst" -v p="$total" 'BEGIN{printf "FAIL: %.1f full server walks per redraw or move\n", m/p}'
exit 1
```

(replacing the existing `if [ "$min" -le "$budget" ] … exit 1` tail). Run `bash tests/freeze-rig/run.sh gate` twice; record all four numbers in `tests/freeze-rig/RESULTS.txt` under a dated heading, next to the status-redraw baseline. The rig's status-redraw rule already fails on master (696 visits / 5.8 walks — the pre-existing `tmux.conf` marker walk, project memory `freeze-precompute-pattern`); the phase-2 check is that every number is the same as master's — phase 2 changed no format, so a move must cost exactly what it cost before. If a move number exceeds the redraw number, that is a finding for the owner, not a failure to hide.

- [ ] **Step 4: Commit**

```bash
git add tests/run.sh tests/freeze-rig/gate.sh tests/freeze-rig/RESULTS.txt
git commit -m "test: moving between projects leaves every review where it was; the rig prices a window, terminal and pane move like a redraw"
```

**Task 6 addendum — the suite is green from any cwd:** the gate runs once more from outside the repo — `(cd / && make -C ~/.cache/cockpit-wt/phase2-store check)`, unpiped, last line read by hand — so a `make check` typed from another project's directory is the same gate.

---

### Task 6: Gate and merge

**Files:** none new.

- [ ] **Step 1: Whole-branch review**

Dispatch a fresh `oh-my-claudecode:code-reviewer` (or `superpowers:requesting-code-review`) with `git diff master...feat/phase2-store`, this plan, and the Global Constraints. Findings go to a fresh worker; nothing here is self-approved.

- [ ] **Step 2: The gate, twice, unpiped**

Run: `cd ~/.cache/cockpit-wt/phase2-store && make check` — read the last line yourself; do not pipe it.
Expected: `shellcheck: clean` and `  N passed, 0 failed, K skipped` with N ≥ 544 + the new assertions. Run it again; same result.

- [ ] **Step 3: Leak sweep**

Run: `ls /tmp/tmux-1000 | grep -c cockpit-; pgrep -fa 'hunk|lens'; ss -ltnp | grep 47657; ls ~/.local/state/tmux-cockpit/review 2>/dev/null`
Expected: the socket count is what it was before the run; the other three print nothing (the suite's `XDG_STATE_HOME` redirects keep the real state dir untouched).

- [ ] **Step 4: Freeze rig**

Run: `tests/freeze-rig/run.sh`
Expected: unchanged from master (696 visits / 5.8 walks) — this phase adds no option and no format.

- [ ] **Step 5: Merge**

```bash
cd ~/.tmux/plugins/tmux-cockpit && git merge --no-ff feat/phase2-store -m "Merge branch 'feat/phase2-store' — phase 2: one session dir per review, the store parser, ^X ^L ^E ^R"
make check
```

Expected: the merged tree's gate green, unpiped. Then update the memory file `review-redesign-research.md` status line and the memo's status line with the merge hash.

---

## Self-review

**Spec coverage** (master plan §1 phase 2 row): session dir ✔ Task 2 · `diff.tsv` snapshot → `<store>.rows` via `snapshot()` ✔ Task 3 (named in the audit) · `lib/review-store.sh` parse = inverse of format, full grammar ✔ Task 0 (landed) · `lib/review-diff.awk` ✘ skipped (audit) · noun-verb subcommands ✘ skipped (audit) · `^R` re-snapshot with stale report ✔ Task 3 · `^X` delete, `@` list → `^L`, `●N` ✔ Task 4 · `^E` editor ✔ Task 5 · golden `.tsv` fixtures ✔ phase 1 (8 under `tests/fixtures/review/`) · 25 sleeps → 9 polls ✔ Task 1 (audit) · gate: round-trip property test ✔ landed; mid-review-mutation test ✔ Task 3 (fail-first on `--resnap`; the phase-1 mark-stays assertion is the regression tooth); boundary test ✔ landed, lists the one lib.

**Placeholder scan:** no "TBD", no "similar to Task N", no "add error handling"; every step has its code or its exact command.

**Type consistency:** `until_cap <sock> <pane> <pattern>` / `until_file <file> <pattern>` (Task 1) used identically in Tasks 2–5. `session_dir <pane> <root>` (Task 2) is what Task 2's test recomputes. `snapshot <dir> <store>` and `stale_keys <store> <rows>` (Task 3) are called with those argument orders in `run_loop`, `resnap`. `detach <store> <path> <line>` matches the `{1} {2}` bind and the test's `--detach "$RXSTORE" src.go 3`. `edit_comment <store> <path> <line> <fragment> <query> <dir>` matches the `{1} {2} {3} {q}` bind and the dispatcher. Header text evolves Task 3 → 4 → 5 and each task states the full line.
