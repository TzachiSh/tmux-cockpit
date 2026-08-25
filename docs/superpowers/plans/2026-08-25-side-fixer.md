# Side-Fixer Panes + Boundary Handover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the human never presses Esc to redirect a busy agent onto an orthogonal job again — a **side-fixer** (a separate `claude -p` in its own pane, own context, scope-fenced) does the job while the main agent keeps working; the main agent is **held at its next Stop** while the fixer is mid-flight and resumes on that same turn with **one handover line**.

**Architecture:** three pieces, all reusing phase 3's plumbing. (1) `scripts/tmux-fixer` — one script, two entry points: `-p <pane>` (the `prefix f` popup: one-line job, computes the scope fence from the main pane's dirty files, takes the lock, spawns the fixer pane via `tmux split-window -d`, returns) and `--run` (runs *inside* the new pane: owns the lock in an `EXIT` trap, runs `claude -p … --output-format json`, extracts the final `HANDOVER:` line from `.result`, writes `handover`). The model never writes the lock or the handover; the wrapper does — a crash or a forgetful model cannot wedge the main agent. (2) The **lock is a directory** in the review session dir (`<sess>/fixer.lock/` with plain files `pid`, `pane`, `reason`, `deadline`, `session`) — `mkdir` is bash's atomic primitive and needs no JSON parser on a hook path. (3) `scripts/review-gate` (phase 3's Stop hook) gains a **hold** as its first job: no lock → zero added latency; lock → poll 1 s until it clears, the PID dies, or the deadline passes; then the handover line (or the stale warning) goes into `additionalContext` **ahead of** any queued review. Fail-open everywhere. No new hook, no daemon, no subagent.

**Tech Stack:** bash 4+, tmux ≥ 3.3, git, Claude Code **2.1.245** (`claude -p`, `--output-format json` with `.result` — verified live this session; `--session-id <uuid>`; `--permission-mode acceptEdits`; `--allowedTools`/`--disallowedTools` in permission-rule syntax — `Edit(<glob>)` only, `Write(...)` path rules are silently never consulted since v2.1.210; Stop hook `hookSpecificOutput.additionalContext`; per-hook `timeout` and `statusMessage`). `uuidgen` or `/proc/sys/kernel/random/uuid`. No jq on the hook path (the wrapper may use jq to read `.result` — it runs in the fixer's pane, not in a hook; fallback: a `grep -o` on the json).

**Spec:** the owner's brief `docs/superpowers/research/2026-08-25-side-fixer/brief.md` (Goal, Non-goals, Phases 1–3, DON'Ts, Acceptance); Phase 0 facts in `docs/superpowers/research/2026-08-25-side-fixer/hook-facts.md` (C8 spawn via tmux server-side, C9 flags + permission syntax, C10 confirmed negative, E lock semantics) and `…/handoff-check-report.md` (Stop `additionalContext`, 8-block cap, 600 s default timeout, output >10,000 chars auto-saved, `Stop` skips on user interrupt); phase 3's plan `2026-08-25-review-phase3.md` (review-gate, session dir, `--delta`, the drain). Ponytail audit below.

**Branch:** `feat/side-fixer`, worktree `~/.cache/cockpit-wt/side-fixer`, off master **after phase 3 merges** (it builds on `scripts/review-gate`, `session_dir`, `tmux-review --delta`). Sequence in the roadmap: phase 3b.

## Global Constraints

- Everything in phase 3's Global Constraints (copy: `docs/superpowers/plans/2026-08-25-review-phase3.md` §Global Constraints) — no shell on a redraw, hooks fail-open, one-shot scripts, no daemon, quoted paths, private `-L` sockets in tests, no new fixed `sleep`, ponytail.
- **The main agent is never interrupted**: no Esc, no `send-keys`, no synthetic keystrokes into a running turn. The hold happens inside the Stop hook the agent itself fires.
- **The fixer's transcript never reaches the main agent**: exactly one line (`HANDOVER: …`), capped at 300 characters by the wrapper. If the handover is a paragraph, the wrapper truncates it and says so.
- **No block-and-rewake**: one hook invocation poll-holds; `review-gate` never returns `decision: block` for the hold and never exits 2 for it. `stop_hook_active: true` → no hold.
- **Never hold without a deadline, never hold without a lock**: default deadline 300 s, written into the lock at spawn; the hook entry's `timeout` is 330 (above the deadline, below the 600 s default).
- **The fixer never writes the main agent's live edit surface**: the fence is computed at spawn from the main pane's dirty files (`git status --porcelain --no-optional-locks`, tracked and untracked) and handed to `claude -p` as `--disallowedTools 'Edit(./<path>)'` per file, plus the mutating git subcommands denied.
- **Not a Task subagent, not a review channel**: the fixer is a peer process in a pane; review comments keep their own path (queue → Stop drain), which this plan only *orders* after the handover.
- Every hook field used traces to an `[E]` line in `hook-facts.md` / `handoff-check-report.md` (the acceptance clause).

## Ponytail audit — what the brief asked for and this plan changes

| brief item | verdict | why |
|---|---|---|
| `fixer.lock` as a JSON file | **replace** with a lock *directory* of plain files | `mkdir` is the atomic primitive bash has (E, hook-facts); a JSON parser on a hook path is the thing agent-state's discipline forbids. |
| the fixer writes `handover` and releases the lock | **replace**: the wrapper owns both | a model that forgets is the common failure; an `EXIT` trap in `tmux-fixer --run` clears the lock on any exit, and the handover is extracted from `claude -p`'s `.result` (verified live). |
| a `<state-dir>/<pane-id>/` layout | **reuse** phase 3's session dir | `session_dir <pane> <root>` already exists and is 0700; the hook, the popup and the wrapper all find it by `tmux-review --sess <pane>` (one new subcommand, one definition). |
| a new Stop hook for the hold | **skip** — extend `review-gate` | one hook, one script, one ordering (hold → handover → review drain → `◆` flag). Two Stop hooks would run in parallel and the order the brief wants would be undefined. |
| stderr logging "so the human sees why the pane is paused" | **replace** with a pane option + one notice | a Stop hook's stderr is not shown live (B7); `@cockpit_hold=<reason>` is readable from the border/`prefix i`, and one `display-message` says "held: <reason>" when the hold begins. The hook entry's `statusMessage` names the spinner. |
| human types the scope fence | **replace**: computed | "the main agent's live edit surface" *is* its dirty files at spawn time; no prompt for paths. |
| `claude "<prompt>"` interactive fixer | **replace** with `claude -p --output-format json` | the bare form never exits (C9); `-p` ends, prints `.result`, and the pane closes itself on success (stays open with the output on failure, `↵ to close`). |
| Phase 0 as a separate research phase | **done** this session | `hook-facts.md` + `handoff-check-report.md`; the one unanswerable item (Esc during a hold) is a manual smoke step in Task 4. |
| a fixer worktree | **skip** | the fence covers the live edit surface; a worktree would put the fixer's fix (e.g. the test suite) where the main agent cannot see it — the opposite of the goal. Add `@cockpit_fixer_worktree on` when a job needs it. |
| configurable deadline / model / flags | **one option** `@cockpit_fixer_deadline` (seconds, default 300); no model/flags options | YAGNI; the wrapper hardcodes the fence and `acceptEdits`; edit the script when a second policy appears. |

---

### Task 1: The wrapper — `tmux-fixer --run`, the lock directory, the handover, and a fake `claude`

**Files:**
- Create: `scripts/tmux-fixer` (executable), `tests/fixtures/fake-claude` (executable; a `claude` stand-in the suite puts on `PATH`).
- Modify: `scripts/tmux-review` — add `--sess <pane>` (prints `session_dir` for the pane's repo root) to `usage()` and the dispatcher.
- Test: `tests/run.sh` — new block `== tmux-fixer --run: the wrapper owns the lock and the handover; a dead fixer wedges nobody ==`.

**Interfaces:**
- Produces: `tmux-review --sess <pane>` → the session dir path (created 0700 if missing; exit 1 when the pane is gone). `tmux-fixer --run <main-pane> <sess> <job…>`: expects `<sess>/fixer.lock/` to exist (created by Task 2's spawn path — this task's test creates it by hand), writes `pid` (its own `$$`) and `session` (the chosen UUID) into it, runs `claude -p`, writes `<sess>/handover` (one line, ≤300 chars) on a `HANDOVER:` result, removes the lock dir in an `EXIT` trap. `tests/fixtures/fake-claude`: prints a `--output-format json` document whose `.result` is `$FAKE_CLAUDE_RESULT` (default `HANDOVER: fixed the thing; suite 118s → 8s; re-read tests/`) after sleeping `$FAKE_CLAUDE_SLEEP` (default 0) seconds; records its argv to `$FAKE_CLAUDE_ARGS` if set.

- [ ] **Step 1: Write the fake `claude`**

`tests/fixtures/fake-claude`:

```bash
#!/usr/bin/env bash
# A `claude` the suite can afford: honours -p, --output-format json and the
# flags the wrapper passes, prints one result document. Records argv so a test
# can assert the fence; sleeps so a test can hold the lock open.
set -u
[ -n "${FAKE_CLAUDE_ARGS:-}" ] && printf '%s\n' "$@" > "$FAKE_CLAUDE_ARGS"
sleep "${FAKE_CLAUDE_SLEEP:-0}"
res=${FAKE_CLAUDE_RESULT-HANDOVER: fixed the thing; suite 118s → 8s; re-read tests/}
res=${res//\\/\\\\}; res=${res//\"/\\\"}
printf '{"type":"result","subtype":"success","is_error":false,"result":"%s","session_id":"fake-session"}\n' "$res"
```

- [ ] **Step 2: Write the failing test**

```bash
echo "== tmux-fixer --run: the wrapper owns the lock and the handover; a dead fixer wedges nobody =="
FX=$(mktmpd); FXSESS="$FX/sess"; mkdir -p "$FXSESS/fixer.lock"; printf 'fix the tests\n' > "$FXSESS/fixer.lock/reason"
printf '%s\n' $(( $(date +%s) + 300 )) > "$FXSESS/fixer.lock/deadline"
FXBIN=$(mktmpd); ln -s "$DIR/tests/fixtures/fake-claude" "$FXBIN/claude"
FXARGS=$(mktmpf)
PATH="$FXBIN:$PATH" FAKE_CLAUDE_ARGS="$FXARGS" "$DIR/scripts/tmux-fixer" --run %99 "$FXSESS" fix the tests >/dev/null 2>&1
fx_rc=$?
is "the wrapper exits 0 on a HANDOVER result" "0" "$fx_rc"
is "the handover is the one line, verbatim" "HANDOVER: fixed the thing; suite 118s → 8s; re-read tests/" "$(cat "$FXSESS/handover" 2>/dev/null)"
is "the lock is released on exit" "gone" "$([ -d "$FXSESS/fixer.lock" ] && echo present || echo gone)"
case "$(cat "$FXARGS")" in
    *"-p"*"--output-format"*"json"*"--permission-mode"*"acceptEdits"*"--session-id"*) ok "claude -p is called headless, json, acceptEdits, with a chosen session id" ;;
    *) bad "claude -p is called headless, json, acceptEdits, with a chosen session id" "-p … --output-format json … --permission-mode acceptEdits … --session-id" "$(cat "$FXARGS")" ;;
esac
case "$(cat "$FXARGS")" in *"HANDOVER: "*) ok "the prompt tells the fixer the one-line contract" ;; *) bad "the prompt tells the fixer the one-line contract" "HANDOVER: in the prompt" "$(tail -3 "$FXARGS")" ;; esac
# No HANDOVER line: the wrapper writes the fallback and still releases the lock.
mkdir -p "$FXSESS/fixer.lock"; rm -f "$FXSESS/handover"
PATH="$FXBIN:$PATH" FAKE_CLAUDE_RESULT='I did some things and here is a long explanation' "$DIR/scripts/tmux-fixer" --run %99 "$FXSESS" fix >/dev/null 2>&1 </dev/null
is "no HANDOVER line: the handover says the fixer finished without one" "HANDOVER: side task finished without a handover line (fix); re-check anything it may have touched" "$(cat "$FXSESS/handover" 2>/dev/null)"
is "and the lock is still released" "gone" "$([ -d "$FXSESS/fixer.lock" ] && echo present || echo gone)"
# A 400-char handover is cut to 300 and says so.
mkdir -p "$FXSESS/fixer.lock"; rm -f "$FXSESS/handover"
PATH="$FXBIN:$PATH" FAKE_CLAUDE_RESULT="HANDOVER: $(printf 'x%.0s' $(seq 1 400))" "$DIR/scripts/tmux-fixer" --run %99 "$FXSESS" fix >/dev/null 2>&1 </dev/null
is "a long handover is capped at 300 chars" "300" "$(wc -c < "$FXSESS/handover" | tr -d ' ')"
# The wrapper killed mid-run: the EXIT trap still clears the lock (SIGTERM); SIGKILL is Task 3's stale-pid path.
mkdir -p "$FXSESS/fixer.lock"; rm -f "$FXSESS/handover"
PATH="$FXBIN:$PATH" FAKE_CLAUDE_SLEEP=30 "$DIR/scripts/tmux-fixer" --run %99 "$FXSESS" fix >/dev/null 2>&1 </dev/null &
fx_pid=$!
until_file "$FXSESS/fixer.lock/pid" '*[0-9]*'
is "the wrapper records its own pid in the lock" "$fx_pid" "$(cat "$FXSESS/fixer.lock/pid")"
kill "$fx_pid" 2>/dev/null; wait "$fx_pid" 2>/dev/null
is "a terminated wrapper releases the lock (EXIT trap)" "gone" "$([ -d "$FXSESS/fixer.lock" ] && echo present || echo gone)"
pkill -f "fake-claude" 2>/dev/null; rm -rf "$FX" "$FXBIN"
```

(`--sess` is asserted in Task 2's block, where a real pane exists.)

- [ ] **Step 3: Run it to see it fail**

Run: `make test-only PAT='tmux-fixer --run'` — every line FAIL (no such script).

- [ ] **Step 4: Write `scripts/tmux-fixer` (the `--run` half; Task 2 adds `-p`)**

```bash
#!/usr/bin/env bash
# tmux-fixer — a side-fixer pane beside a busy agent, and the wrapper that runs
# inside it. The human spawns a separate `claude -p` to do an orthogonal job
# (fix the tests, regenerate a fixture) instead of interrupting the main agent;
# the main agent is held at its next Stop by review-gate while the lock exists
# and resumes with ONE handover line.
#   tmux-fixer -p <pane>                     the prefix f popup: ask for the job, fence, lock, spawn
#   tmux-fixer --run <pane> <sess> <job…>    inside the fixer pane: own the lock, run claude -p, write the handover
# The lock is a DIRECTORY (mkdir is atomic; no JSON on a hook path):
#   <sess>/fixer.lock/{pid,pane,reason,deadline,session}   <sess>/handover (one line)
set -uo pipefail
SELF="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
REVIEW="$(dirname "$SELF")/tmux-review"
shq() { printf '%q' "$1"; }

# --- --run: the wrapper --------------------------------------------------------
# The MODEL writes nothing here: the wrapper takes the pid slot, runs the fixer,
# extracts the final "HANDOVER: …" line from claude -p's .result, writes the
# handover file, and the EXIT trap releases the lock on every exit — success,
# failure, SIGTERM, a closed pane. A kill -9 skips the trap; review-gate's
# stale-pid check covers that.
run_fixer() {                 # <main-pane> <sess> <job…>
    local pane=$1 sess=$2; shift 2
    local job="$*" lock="$sess/fixer.lock" sid out res rc reason
    [ -d "$lock" ] || { printf 'tmux-fixer: no lock at %s — spawn through `tmux-fixer -p`\n' "$lock" >&2; exit 2; }
    trap 'rm -rf -- "$lock"' EXIT
    printf '%s\n' "$$" > "$lock/pid"
    sid=$(uuidgen 2>/dev/null || cat /proc/sys/kernel/random/uuid 2>/dev/null || printf '%s-%s' "$$" "$(date +%s)")
    printf '%s\n' "$sid" > "$lock/session"
    reason=$(cat -- "$lock/reason" 2>/dev/null)
    printf 'side fix for %s: %s\n(pid %s · deadline %ss · this pane closes itself when done)\n\n' \
        "$pane" "$reason" "$$" "$(( $(cat -- "$lock/deadline" 2>/dev/null || echo 0) - $(date +%s) ))"
    # The fence: every path the main agent has dirty is denied to Edit (Write
    # path rules are never consulted -- permissions.md, v2.1.210+); mutating
    # git is denied outright. acceptEdits so a headless run never stalls on a
    # prompt it cannot answer.
    local -a deny=()
    while IFS= read -r p; do [ -n "$p" ] && deny+=("Edit(./$p)"); done < "$lock/fence"
    for g in commit push checkout switch stash reset rebase merge restore; do deny+=("Bash(git $g *)"); done
    out=$(claude -p "$job

You are a SIDE TASK running beside another agent that is working in this repository right now. Do only this job; do not touch these files, they are the other agent's live edits: $(tr '\n' ' ' < "$lock/fence"). Do not commit. When you are done, your final message must be exactly ONE line that starts with 'HANDOVER: ' and states what changed and the observable delta (for example 'HANDOVER: tests fixed; suite 118s -> 8s; re-read any test file you had open'). No reasoning, no summary, one line." \
        --output-format json --permission-mode acceptEdits --max-turns 60 --session-id "$sid" \
        --allowedTools 'Bash(*)' --disallowedTools "${deny[@]}" 2>&1)
    rc=$?
    # .result without jq on the hook side is not needed here -- this runs in the
    # fixer's own pane; jq if present, a conservative grep otherwise.
    if command -v jq >/dev/null 2>&1; then res=$(printf '%s' "$out" | jq -r '.result // empty' 2>/dev/null)
    else res=$(printf '%s' "$out" | grep -o '"result":"[^"]*"' | head -1 | sed 's/^"result":"//; s/"$//'); fi
    local line
    line=$(printf '%s\n' "$res" | grep '^HANDOVER: ' | tail -1)
    if [ -z "$line" ]; then
        line="HANDOVER: side task finished without a handover line ($reason); re-check anything it may have touched"
        printf '%s\n\n%s\n' "$res" "$line"
    else
        printf '%s\n' "$line"
    fi
    line=${line//[$'\001'-$'\037']/}
    [ "${#line}" -gt 300 ] && line="${line:0:287}… (truncated)"
    (umask 077; printf '%s\n' "$line" > "$sess/handover")
    if [ "$rc" -ne 0 ] || [ -z "$res" ]; then
        printf '\nclaude exited %s\n%s\n\n↵ to close\n' "$rc" "$out"; read -r _ </dev/tty 2>/dev/null
        exit 1
    fi
    exit 0
}

case "${1:-}" in
    --run) shift; run_fixer "${1:-}" "${2:-}" "${@:3}" ;;
    -p) shift; spawn_fixer "${1:-}" ;;       # Task 2
    *) printf 'usage: tmux-fixer -p <pane> | --run <pane> <sess> <job…>\n' >&2; exit 2 ;;
esac
```

(`$lock/fence` is written by Task 2's spawn path; this task's test creates the lock by hand — add `: > "$FXSESS/fixer.lock/fence"` after the `mkdir -p` lines in the test so the wrapper finds an empty fence.) A 300-char cap: `${line:0:287}… (truncated)` is 287 + 13 = 300 characters — the test's `wc -c` counts bytes; the `…` is 3 bytes, so assert `302` bytes, or cap on `${#line}` and assert with `awk '{print length}'`. Pick the character count (`awk`) and keep 300 in both.

`scripts/tmux-review`: `--sess) shift; sess_mode "${1:-}" ;;` with

```bash
sess_mode() {                 # <pane> -> the review session dir for the pane's repo root, created
    local pane=$1 dir root sess
    dir=$(tmux display-message -p -t "$pane" '#{pane_current_path}' 2>/dev/null); [ -n "$dir" ] || return 1
    root=$(git_root "$dir"); sess=$(session_dir "$pane" "$root")
    (umask 077; mkdir -p -- "$sess") && printf '%s\n' "$sess"
}
```

and a `usage()` line: `  tmux-review --sess <pane>             the review session dir for <pane> (tmux-fixer, review-gate)`.

- [ ] **Step 5: Run; commit**

Run: `make test-only PAT='tmux-fixer --run'` — every line PASS; `make lint` clean (both new files are in `SOURCES` by the `scripts/*` glob; add `tests/fixtures/fake-claude` to `SOURCES` explicitly, as `mkfix.sh` is).

```bash
git add scripts/tmux-fixer tests/fixtures/fake-claude scripts/tmux-review tests/run.sh Makefile
git commit -m "feat: tmux-fixer --run owns a side fix's lock and handover — claude -p headless, the fence as Edit() denies, one HANDOVER line, the lock released by an EXIT trap"
```

---

### Task 2: The spawn path — `prefix f`, the fence, the lock, the pane

**Files:**
- Modify: `scripts/tmux-fixer` — `spawn_fixer()`; `cockpit.tmux` — the `f` binding (`@cockpit_fixer_key`, `off`), `@cockpit_fixer_deadline`; `README.md` — a "Side fixes" section (short).
- Test: `tests/run.sh` — new block `== tmux-fixer -p: one popup spawns a fenced side pane, takes the lock first, and refuses a second ==`.

**Interfaces:**
- Produces: `tmux-fixer -p <pane>` — in the popup: `read -r -p 'side fix (one line): '`; empty → exit 0; computes `sess=$(tmux-review --sess <pane>)`; `mkdir "$sess/fixer.lock"` (refuses with a `display-message` naming the running job when it exists — exit 1); writes `reason`, `deadline` (`now + @cockpit_fixer_deadline`, default 300), `pane` (placeholder until the split returns), `fence` (the main pane's dirty paths, one per line, tracked + untracked, from `git -C <root> --no-optional-locks status --porcelain` — the path column, renames' new side); then `tmux split-window -d -v -l 40% -t <pane> -c <root> -P -F '#{pane_id}' "<tmux-fixer> --run <pane> <sess> <job>"` and writes the new pane id into `pane`; prints one `display-message`: `side fix running in <id>: <job>`; exit 0. The lock exists *before* the split (no boundary slips past). `stop_hook_active` handling is Task 3's.

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-fixer -p: one popup spawns a fenced side pane, takes the lock first, and refuses a second =="
if need_tmux "tmux-fixer -p"; then :; else
SF="cockpit-sf-$$"; SOCKS="$SOCKS $SF"; SFSTATE=$(mktmpd); SFDIR=$(mktmpd); SFBIN=$(mktmpd); ln -s "$DIR/tests/fixtures/fake-claude" "$SFBIN/claude"
( cd "$SFDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > api.go; printf 'x\n' > clean.go; git add -A; git commit -q -m init
  printf 'a\nEDITING\n' > api.go; printf 'new\n' > scratch.txt ) >/dev/null 2>&1        # dirty: api.go (tracked), scratch.txt (untracked); clean.go untouched
"$TMUX_REAL" -L "$SF" -f /dev/null new-session -d -s t -x 120 -y 40 -c "$SFDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$SF" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$SF" set-environment -g PATH "$SFBIN:$PATH" >/dev/null 2>&1              # the spawned pane finds the fake claude
SFAP=$("$TMUX_REAL" -L "$SF" list-panes -t t: -F '#{pane_id}')
SFSESS="$SFSTATE/tmux-cockpit/review/${SFAP#%}-$(printf '%s' "$SFDIR" | cksum | cut -d' ' -f1)"
SFARGS=$(mktmpf)
# Drive the popup's command directly (the suite's servers have no client for a popup): the job on stdin.
"$TMUX_REAL" -L "$SF" run-shell "cd $(printf '%q' "$SFDIR") && printf 'fix the tests\\n' | XDG_STATE_HOME=$(printf '%q' "$SFSTATE") FAKE_CLAUDE_ARGS=$(printf '%q' "$SFARGS") FAKE_CLAUDE_SLEEP=3 '$DIR/scripts/tmux-fixer' -p $SFAP; echo SRC=\$?" > "$SFSTATE/out" 2>&1
case "$(cat "$SFSTATE/out")" in *SRC=0*) ok "the spawn returns 0 at once" ;; *) bad "the spawn returns 0 at once" "SRC=0" "$(cat "$SFSTATE/out")" ;; esac
is "the lock exists with the job as its reason" "fix the tests" "$(cat "$SFSESS/fixer.lock/reason" 2>/dev/null)"
is "the fence is the main pane's dirty paths, tracked and untracked" "api.go
scratch.txt" "$(sort "$SFSESS/fixer.lock/fence" 2>/dev/null)"
[ "$(cat "$SFSESS/fixer.lock/deadline" 2>/dev/null)" -gt "$(date +%s)" ] && ok "the deadline is in the future" || bad "the deadline is in the future" "> now" "$(cat "$SFSESS/fixer.lock/deadline" 2>/dev/null)"
SFFP=$(cat "$SFSESS/fixer.lock/pane" 2>/dev/null)
case "$SFFP" in %[0-9]*) ok "the fixer pane id is recorded" ;; *) bad "the fixer pane id is recorded" "%N" "$SFFP" ;; esac
is "the fixer pane runs in the repo root" "$SFDIR" "$("$TMUX_REAL" -L "$SF" display-message -p -t "$SFFP" '#{pane_current_path}' 2>/dev/null)"
until_file "$SFARGS" '*--disallowedTools*'
case "$(cat "$SFARGS")" in *"Edit(./api.go)"*"Edit(./scratch.txt)"*|*"Edit(./scratch.txt)"*"Edit(./api.go)"*) ok "both dirty paths are denied to Edit" ;; *) bad "both dirty paths are denied to Edit" "Edit(./api.go) and Edit(./scratch.txt)" "$(cat "$SFARGS")" ;; esac
case "$(cat "$SFARGS")" in *"Edit(./clean.go)"*) bad "a clean path is not fenced" "no Edit(./clean.go)" "$(cat "$SFARGS")" ;; *) ok "a clean path is not fenced" ;; esac
case "$(cat "$SFARGS")" in *"Bash(git commit *)"*"Bash(git push *)"*) ok "mutating git is denied" ;; *) bad "mutating git is denied" "Bash(git commit *) … Bash(git push *)" "$(cat "$SFARGS")" ;; esac
# A second spawn while the first runs refuses and spawns nothing.
"$TMUX_REAL" -L "$SF" run-shell "cd $(printf '%q' "$SFDIR") && printf 'another\\n' | XDG_STATE_HOME=$(printf '%q' "$SFSTATE") '$DIR/scripts/tmux-fixer' -p $SFAP; echo SRC=\$?" > "$SFSTATE/out2" 2>&1
case "$(cat "$SFSTATE/out2")" in *SRC=1*) ok "a second spawn on a locked pane refuses (exit 1)" ;; *) bad "a second spawn on a locked pane refuses (exit 1)" "SRC=1" "$(cat "$SFSTATE/out2")" ;; esac
is "and the running job's reason is untouched" "fix the tests" "$(cat "$SFSESS/fixer.lock/reason" 2>/dev/null)"
is "and only one fixer pane exists" "2" "$("$TMUX_REAL" -L "$SF" list-panes -a | wc -l)"
# The fixer finishes: handover written, lock gone, pane closed.
for i in $(seq 1 80); do [ -d "$SFSESS/fixer.lock" ] || break; sleep 0.1; done
is "the lock is released when the fixer finishes" "gone" "$([ -d "$SFSESS/fixer.lock" ] && echo present || echo gone)"
is "the handover is one line" "1" "$(wc -l < "$SFSESS/handover" 2>/dev/null | tr -d ' ')"
for i in $(seq 1 40); do [ "$("$TMUX_REAL" -L "$SF" list-panes -a | wc -l)" = 1 ] && break; sleep 0.1; done
is "the fixer pane closes itself on success" "1" "$("$TMUX_REAL" -L "$SF" list-panes -a | wc -l)"
# The binding.
"$TMUX_REAL" -L "$SF" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$SF" list-keys -T prefix 2>/dev/null | awk '$4=="f"')" in *tmux-fixer*) ok "prefix f is bound to the side-fix popup" ;; *) bad "prefix f is bound to the side-fix popup" "display-popup … tmux-fixer" "$("$TMUX_REAL" -L "$SF" list-keys -T prefix | awk '$4=="f"')" ;; esac
"$TMUX_REAL" -L "$SF" kill-server >/dev/null 2>&1; rm -rf "$SFBIN"
fi
```

- [ ] **Step 2: Run it to see it fail** — `make test-only PAT='tmux-fixer -p'`: every line FAIL.

- [ ] **Step 3: Implement `spawn_fixer`**

```bash
# --- -p: the popup ---------------------------------------------------------------
# One line from the human, then: the fence (the main pane's dirty paths -- its
# live edit surface), the LOCK FIRST (mkdir; a second spawn on the same pane
# refuses here), then the pane, spawned by the tmux SERVER (split-window -d),
# so nothing about this popup's lifetime -- or a hook's -- touches the fixer.
spawn_fixer() {               # <main-pane>
    local pane=$1 job sess lock root dl fp
    case "$pane" in %[0-9]*) ;; *) printf 'tmux-fixer: not a pane id: %s\n' "$pane" >&2; exit 2 ;; esac
    sess=$("$REVIEW" --sess "$pane") || { tmux display-message "tmux-fixer: pane $pane is gone" 2>/dev/null; exit 1; }
    lock="$sess/fixer.lock"
    if [ -d "$lock" ]; then
        tmux display-message "tmux-fixer: a side fix is already running for $pane: $(cat -- "$lock/reason" 2>/dev/null) (pane $(cat -- "$lock/pane" 2>/dev/null))" 2>/dev/null
        exit 1
    fi
    if [ -t 0 ]; then read -r -p 'side fix (one line, ↵ alone cancels): ' job; else IFS= read -r job; fi
    job=${job//[$'\001'-$'\037']/}
    [ -n "$job" ] || exit 0
    root=$(git -C "$(tmux display-message -p -t "$pane" '#{pane_current_path}')" rev-parse --show-toplevel 2>/dev/null) || {
        tmux display-message "tmux-fixer: $pane is not in a git repo" 2>/dev/null; exit 1; }
    dl=$(tmux show-option -gqv @cockpit_fixer_deadline); case "$dl" in ''|*[!0-9]*) dl=300 ;; esac
    (umask 077; mkdir -- "$lock") 2>/dev/null || { tmux display-message "tmux-fixer: a side fix is already running for $pane" 2>/dev/null; exit 1; }
    printf '%s\n' "$job" > "$lock/reason"
    printf '%s\n' $(( $(date +%s) + dl )) > "$lock/deadline"
    printf '%s\n' "pending" > "$lock/pane"
    # The fence: every dirty path, tracked or untracked (status's path column;
    # a rename's NEW name). --no-optional-locks: this is the main agent's tree.
    git -C "$root" --no-optional-locks -c core.quotePath=false status --porcelain 2>/dev/null \
        | sed 's/^...//; s/^.* -> //' > "$lock/fence"
    fp=$(tmux split-window -d -v -l 40% -t "$pane" -c "$root" -P -F '#{pane_id}' \
        "$(shq "$SELF") --run $(shq "$pane") $(shq "$sess") $(shq "$job")") || {
        rm -rf -- "$lock"; tmux display-message "tmux-fixer: could not open the fixer pane" 2>/dev/null; exit 1; }
    printf '%s\n' "$fp" > "$lock/pane"
    tmux display-message "side fix running in $fp: $job — $pane is held at its next stop until it finishes" 2>/dev/null
    exit 0
}
```

`cockpit.tmux`, after the info binding:

```bash
# prefix f: a side fix beside a busy agent -- one line, a fenced claude -p in
# its own pane, the agent held at its next Stop until the handover.
Q_FIXER=$(sq "$DIR/scripts/tmux-fixer")
FK=$(tmux show-option -gqv @cockpit_fixer_key)
case "$FK" in
    off) ;;
    "")  tmux bind-key f display-popup -E -w "$PW" -h 5 -T ' side fix · #{pane_id} ' "$Q_FIXER -p '#{pane_id}'" ;;
    *)   tmux bind-key "$FK" display-popup -E -w "$PW" -h 5 -T ' side fix · #{pane_id} ' "$Q_FIXER -p '#{pane_id}'" ;;
esac
```

(stock `prefix f` is `find-window`; `@cockpit_fixer_key off` keeps it — say so in the README's options block.) The `-p` arm's `read` uses `/dev/tty` inside a popup (`[ -t 0 ]` is true there); the test feeds stdin.

- [ ] **Step 4: Run; README; commit**

README "Side fixes" (after the review section): what it is, `prefix f`, the fence, the hold, `@cockpit_fixer_key`, `@cockpit_fixer_deadline`, and the two DON'Ts the brief states (not a review channel; not a subagent).

```bash
git add scripts/tmux-fixer cockpit.tmux README.md tests/run.sh
git commit -m "feat: prefix f spawns a fenced side fixer beside a busy agent — lock first, then the pane, computed fence from the agent's dirty paths, a second spawn refuses"
```

---

### Task 3: The hold — `review-gate` waits for the lock, then hands over one line ahead of any review

**Files:**
- Modify: `scripts/review-gate` — the hold before the drain; `stop_hook_active` parse; `@cockpit_hold`; the payload assembly (handover first, then the review spool). `examples/claude-hooks.json` — the review-gate entry's `"timeout": 330` and `"statusMessage": "cockpit: holding for a side fix"`. `scripts/tmux-review --info` — a `hold` line.
- Test: `tests/run.sh` — new block `== review-gate: holds at Stop while a side fix runs, resumes with one line, fails open on a dead or late fixer ==`; the existing review-gate block's silent-stdin timing assertion stays.

**Interfaces:**
- Produces: on Stop, with `<sess>/fixer.lock` present and `stop_hook_active` not `true`: set `@cockpit_hold=<reason>` on the pane, `display-message "held: <reason> (side fix in <fixer pane>)"`, poll every 1 s while the lock dir exists AND its `pid` is alive AND `now < deadline`; then unset `@cockpit_hold`; `ctx` = the `handover` line if present (consumed: `rm`), else `a side task ("<reason>") was running and did not finish; earlier results may be stale.`; `ctx` gets `Continue with your task.` appended; the review spool text (if armed) follows it. Emitted as `{"hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"<escaped>"}}`, exit 0 (phase 3's Task 5 shape). No lock → the existing path, byte-for-byte.

- [ ] **Step 1: Write the failing test**

```bash
echo "== review-gate: holds at Stop while a side fix runs, resumes with one line, fails open on a dead or late fixer =="
if need_tmux "review-gate hold"; then :; else
RH="cockpit-rh-$$"; SOCKS="$SOCKS $RH"; RHSTATE=$(mktmpd); RHDIR=$(mktmpd)
( cd "$RHDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e; git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init ) >/dev/null 2>&1
"$TMUX_REAL" -L "$RH" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RHDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RH" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
RHAP=$("$TMUX_REAL" -L "$RH" list-panes -t t: -F '#{pane_id}')
RHCTL=$("$TMUX_REAL" -L "$RH" split-window -t t -P -F '#{pane_id}' 'exec bash'); until_cap "$RH" "$RHCTL" '*$*'
RHSESS="$RHSTATE/tmux-cockpit/review/${RHAP#%}-$(printf '%s' "$RHDIR" | cksum | cut -d' ' -f1)"
RHPAY=$(mktmpf); printf '{"session_id":"s","transcript_path":"/nonexistent","cwd":"%s","hook_event_name":"Stop","stop_hook_active":false}\n' "$RHDIR" > "$RHPAY"
RHPAY2=$(mktmpf); sed 's/"stop_hook_active":false/"stop_hook_active":true/' "$RHPAY" > "$RHPAY2"
rh_n() { "$TMUX_REAL" -L "$RH" capture-pane -p -J -t "$RHCTL" | grep -c 'HRC=[0-9]'; }
rh_run() {                    # <payload> [extra env]  -> the hook's stdout in $RHSTATE/out, HRC= on the control pane
    local n0; n0=$(rh_n)
    "$TMUX_REAL" -L "$RH" send-keys -t "$RHCTL" "${2:-} TMUX_PANE=$RHAP XDG_STATE_HOME=$(printf '%q' "$RHSTATE") '$DIR/scripts/review-gate' < $(printf '%q' "$1") > $(printf '%q' "$RHSTATE/out") 2>$(printf '%q' "$RHSTATE/err"); echo HRC=\$?" Enter
    local i; for i in $(seq 1 100); do [ "$(rh_n)" -gt "$n0" ] && return 0; sleep 0.1; done; return 1
}
mklock() { mkdir -p "$RHSESS/fixer.lock"; printf 'fix the tests\n' > "$RHSESS/fixer.lock/reason"; printf '%s\n' "$1" > "$RHSESS/fixer.lock/pid"; printf '%s\n' "$2" > "$RHSESS/fixer.lock/deadline"; printf '%%9\n' > "$RHSESS/fixer.lock/pane"; }
# 1. No lock: the hook is as fast as before and sets no hold.
rh_t0=$SECONDS; rh_run "$RHPAY"; [ $(( SECONDS - rh_t0 )) -le 3 ] && ok "no lock: no added latency" || bad "no lock: no added latency" "≤3s" "$(( SECONDS - rh_t0 ))s"
is "no lock: no hold state" "" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_hold)"
# 2. A live fixer: the hook holds, the pane says why; releasing the lock with a handover resumes it with that one line.
sleep 300 & RHFX=$!
mklock "$RHFX" $(( $(date +%s) + 300 ))
rh_t0=$SECONDS
n0=$(rh_n); "$TMUX_REAL" -L "$RH" send-keys -t "$RHCTL" "TMUX_PANE=$RHAP XDG_STATE_HOME=$(printf '%q' "$RHSTATE") '$DIR/scripts/review-gate' < $(printf '%q' "$RHPAY") > $(printf '%q' "$RHSTATE/out"); echo HRC=\$?" Enter
for i in $(seq 1 40); do [ -n "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_hold)" ] && break; sleep 0.1; done
is "a held pane carries the reason" "fix the tests" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_hold)"
sleep 2; is "and the hook is still holding after 2 s" "$n0" "$(rh_n)"
printf 'HANDOVER: tests fixed; suite 118s -> 8s; re-read tests/\n' > "$RHSESS/handover"; rm -rf "$RHSESS/fixer.lock"; kill "$RHFX" 2>/dev/null
for i in $(seq 1 40); do [ "$(rh_n)" -gt "$n0" ] && break; sleep 0.1; done
[ "$(rh_n)" -gt "$n0" ] && ok "the hook returns within 4 s of the lock clearing" || bad "the hook returns within 4 s of the lock clearing" "HRC" "$("$TMUX_REAL" -L "$RH" capture-pane -p -t "$RHCTL" | tail -3)"
case "$(cat "$RHSTATE/out")" in
    *'"hookSpecificOutput"'*'"additionalContext":"HANDOVER: tests fixed; suite 118s -> 8s; re-read tests/\nContinue with your task."'*) ok "the handover is the additionalContext, one line, then the continue line" ;;
    *) bad "the handover is the additionalContext, one line, then the continue line" "…additionalContext with the HANDOVER line…" "$(cat "$RHSTATE/out")" ;;
esac
is "the hold state is cleared" "" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_hold)"
is "the handover is consumed" "absent" "$([ -e "$RHSESS/handover" ] && echo present || echo absent)"
rh_run "$RHPAY"; is "the next Stop hands over nothing (no replay)" "" "$(grep -o 'HANDOVER[^"]*' "$RHSTATE/out")"
# 3. A dead fixer (kill -9 skipped the trap): stale pid, proceed at once with the warning.
sleep 300 & RHFX=$!; kill -9 "$RHFX"; wait "$RHFX" 2>/dev/null
mklock "$RHFX" $(( $(date +%s) + 300 ))
rh_t0=$SECONDS; rh_run "$RHPAY"; [ $(( SECONDS - rh_t0 )) -le 4 ] && ok "a dead fixer does not hold" || bad "a dead fixer does not hold" "≤4s" "$(( SECONDS - rh_t0 ))s"
case "$(cat "$RHSTATE/out")" in *'a side task ("fix the tests") was running and did not finish; earlier results may be stale.'*) ok "and the agent is warned" ;; *) bad "and the agent is warned" "the stale warning" "$(cat "$RHSTATE/out")" ;; esac
rm -rf "$RHSESS/fixer.lock"
# 4. A late fixer (deadline passed, pid alive): proceed with the warning, the lock left to its owner.
sleep 300 & RHFX=$!; mklock "$RHFX" $(( $(date +%s) - 1 ))
rh_t0=$SECONDS; rh_run "$RHPAY"; [ $(( SECONDS - rh_t0 )) -le 4 ] && ok "an expired deadline does not hold" || bad "an expired deadline does not hold" "≤4s" "$(( SECONDS - rh_t0 ))s"
case "$(cat "$RHSTATE/out")" in *'did not finish'*) ok "and warns" ;; *) bad "and warns" "the stale warning" "$(cat "$RHSTATE/out")" ;; esac
kill "$RHFX" 2>/dev/null; rm -rf "$RHSESS/fixer.lock"
# 5. stop_hook_active: no hold even with a live lock.
sleep 300 & RHFX=$!; mklock "$RHFX" $(( $(date +%s) + 300 ))
rh_t0=$SECONDS; rh_run "$RHPAY2"; [ $(( SECONDS - rh_t0 )) -le 4 ] && ok "stop_hook_active true never holds" || bad "stop_hook_active true never holds" "≤4s" "$(( SECONDS - rh_t0 ))s"
kill "$RHFX" 2>/dev/null; rm -rf "$RHSESS/fixer.lock"
# 6. Ordering: a handover AND a queued review land in one payload, handover first.
printf 'HANDOVER: fixed\n' > "$RHSESS/handover"; printf 'Code review from the human\n\n## f:1 (+)\n> a\nREVIEW-MARKER\n\n' > "$RHSESS/spool.md"
"$TMUX_REAL" -L "$RH" set-option -p -t "$RHAP" @cockpit_review_spool "$RHSESS/spool.md" >/dev/null 2>&1
rh_run "$RHPAY"
case "$(cat "$RHSTATE/out")" in *'HANDOVER: fixed'*'Continue with your task.'*'REVIEW-MARKER'*) ok "handover first, then the queued review, one payload" ;; *) bad "handover first, then the queued review, one payload" "HANDOVER … Continue … REVIEW-MARKER" "$(cat "$RHSTATE/out")" ;; esac
is "the spool was drained too" "" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_review_spool)"
unset -f rh_n rh_run mklock
"$TMUX_REAL" -L "$RH" kill-server >/dev/null 2>&1
fi
```

(Item 6 assumes a handover with no lock still delivers — the wrapper writes `handover` then the trap removes the lock, so a Stop that arrives between the two, or after both, sees the file: consume it whenever it exists.)

- [ ] **Step 2: Run to see it fail** — `make test-only PAT='review-gate: holds'`: the hold assertions FAIL (today the hook returns at once and never sets `@cockpit_hold`).

- [ ] **Step 3: Implement the hold in `review-gate`**

Insert before the drain (`# --- 1. the drain`), after the pane guard and `here=`; it needs the payload's `stop_hook_active`, so move the bounded stdin read (phase 3's block) above it and parse one more field:

```bash
active=$(field stop_hook_active)   # field() strips quotes; a bare true/false reads as "" -- test the raw payload instead:
case "$payload" in *'"stop_hook_active":true'*|*'"stop_hook_active": true'*) active=true ;; *) active=false ;; esac

# --- 0. the hold: a side fix (tmux-fixer) is mid-flight for this pane ----------
# The lock is a directory the fixer's wrapper owns. While it exists, its pid is
# alive and its deadline has not passed, this hook WAITS -- one invocation, one
# poll, no block-and-rewake (the block cap counts decisions, not seconds; the
# hook entry's timeout is 330 s, above the 300 s default deadline). Fail-open:
# a dead pid, a past deadline, a missing file -> proceed at once. Never on a
# stop_hook_active Stop (belt and braces against any rewake).
sess=$("$here/tmux-review" --sess "$pane" 2>/dev/null)
ctx=""
lock="$sess/fixer.lock"
if [ -n "$sess" ] && [ -d "$lock" ] && [ "$active" != true ]; then
    reason=$(cat -- "$lock/reason" 2>/dev/null); reason=${reason//[$'\001'-$'\037']/}
    fpid=$(cat -- "$lock/pid" 2>/dev/null); dl=$(cat -- "$lock/deadline" 2>/dev/null)
    case "$fpid" in ''|*[!0-9]*) fpid=0 ;; esac; case "$dl" in ''|*[!0-9]*) dl=0 ;; esac
    tmux set-option -p -t "$pane" @cockpit_hold "$reason" 2>/dev/null
    tmux display-message "held: $reason (side fix in $(cat -- "$lock/pane" 2>/dev/null)) — $pane resumes when it finishes" 2>/dev/null
    while [ -d "$lock" ] && [ "$(date +%s)" -lt "$dl" ] && { [ "$fpid" -eq 0 ] || kill -0 "$fpid" 2>/dev/null; }; do
        sleep 1
        # the pid is written by the wrapper a moment after the spawn: re-read while 0
        [ "$fpid" -eq 0 ] && { fpid=$(cat -- "$lock/pid" 2>/dev/null); case "$fpid" in ''|*[!0-9]*) fpid=0 ;; esac; }
    done
    tmux set-option -pu -t "$pane" @cockpit_hold 2>/dev/null
    if [ ! -f "$sess/handover" ]; then
        ctx="a side task (\"$reason\") was running and did not finish; earlier results may be stale."
    fi
fi
# The handover is consumed whenever it exists -- with or without a hold (the
# wrapper writes it a moment before its trap removes the lock).
if [ -n "$sess" ] && [ -f "$sess/handover" ] && [ ! -L "$sess/handover" ]; then
    ctx=$(head -c 300 -- "$sess/handover"); ctx=${ctx//[$'\001'-$'\037']/}
    rm -f -- "$sess/handover"
fi
[ -n "$ctx" ] && ctx="$ctx"$'\n'"Continue with your task."
```

Then the drain block (phase 3) becomes: read the spool into `review` (as it does), and the emit step joins them: `payload_ctx="$ctx${ctx:+$'\n\n'}$review"` — emitted once through the JSON escaper as `additionalContext`; exit 0 when either is non-empty; the drain's `agent-state running` call stays. `sess` computed here is reused by the drain if phase 3's code derives it separately — keep one.

`examples/claude-hooks.json`: the review-gate entry becomes `{ "type": "command", "command": "…/scripts/review-gate", "timeout": 330, "statusMessage": "cockpit: review-gate (holding for a side fix if one is running)" }`; update the `_comment` line and the README hooks-table row ("holds while a side fix runs, ≤ the deadline"). Phase 3's install test asserts `.timeout` exists on the review-gate entry — its value is not pinned; if it is, update it to 330.

`tmux-review --info`: after the `review` line, `printf 'hold      %s\n' "${hold:-—}"` with `hold` read from `#{@cockpit_hold}` in the same `display-message` (one more `\037` field) and, if `$sess/fixer.lock` exists: `side fix  <reason>  pane <id>  pid <pid> (<alive|dead>)  deadline in <n>s`.

- [ ] **Step 4: Run; the never-blocks assertion; commit**

Run: `make test-only PAT='review-gate'` — every line PASS, including phase 3's `a silent stdin does not hang the hook` (no lock → no hold → the 2 s read deadline is the only wait). `make check` foreground (tool timeout 600000) once.

```bash
git add scripts/review-gate scripts/tmux-review examples/claude-hooks.json README.md tests/run.sh
git commit -m "feat: review-gate holds a Stop while a side fix runs — one poll, a deadline, fail-open on a dead fixer — and hands over one line ahead of any queued review"
```

---

### Task 4: Docs, rig, leak sweep, manual smoke, gate and merge

**Files:** `README.md` (the "Side fixes" section complete: the flow, the DON'Ts, the manual smoke note), `tests/freeze-rig/RESULTS.txt` (dated heading), no others.

- [ ] **Step 1: Freeze rig** — `bash tests/freeze-rig/run.sh gate` twice: numbers unchanged (one new per-pane option, `@cockpit_hold`, read by nothing on a redraw; record under a dated heading).
- [ ] **Step 2: Leak sweep** — `ls /tmp/tmux-1000 | grep -c cockpit-` unchanged; `find "$XDG_STATE_HOME"/tmux-cockpit -name fixer.lock` prints nothing (the suite's `XDG_STATE_HOME` is temp; check the real one too); `pgrep -fa fake-claude` empty.
- [ ] **Step 3: Manual smoke (the one Phase-0 gap)** — on the owner's live server with the hooks installed: start an agent, give it a long task, `prefix f` → "sleep 120 and say done" (a fake job), watch the main agent finish its turn and its pane show `held: …`; press **Esc in the main pane during the hold** and record what happens (does the hook keep running? does the handover arrive at the next turn?). Write the observed behaviour into the README's "Side fixes" section as a stated fact, not a guess. Then a real job: "the test suite takes 2 min; make it fast" on a scratch repo.
- [ ] **Step 4: Whole-branch review** — a fresh `code-reviewer` (opus) with `git diff master...feat/side-fixer`, this plan, the brief's DON'Ts and Acceptance list as the checklist; every hook field used must trace to an `[E]` line in `hook-facts.md`/`handoff-check-report.md` (the reviewer lists them).
- [ ] **Step 5: Gate** — `make -j2 check-par` twice (phase 3b's fast gate, if merged) or `make check` unpiped twice; `make gate`.
- [ ] **Step 6: Merge** — `git merge --no-ff feat/side-fixer -m "Merge branch 'feat/side-fixer' — side-fixer panes: prefix f, a fenced claude -p beside a busy agent, the Stop held until one handover line"`; `install.sh --hooks` (the review-gate entry's timeout/statusMessage change merges additively? — no: the merge dedupes per group by deep equality, so the *old* review-gate group stays and the new one is appended → two review-gate commands would run. Before merging, decide: `install.sh --hooks` gains nothing; instead the README tells an upgrader to remove the old group, or `install.sh --hooks` learns to replace a group whose command matches by name — the smallest correct change is a `jq` clause that drops any existing entry whose `.command` ends in `/scripts/review-gate` before appending. Do that, with a test in `== install.sh --hooks merges additively ==`.)

---

## Rig — the test list, mapped to the brief's acceptance

| acceptance | test | task |
|---|---|---|
| no fixer → Stop latency unchanged | `no lock: no added latency` (≤3 s incl. the 2 s read deadline); phase 3's silent-stdin assertion | 3 |
| fixer spawned mid-turn → main agent finishes uninterrupted, pauses at Stop with a visible reason | `a held pane carries the reason`, `still holding after 2 s`, the `display-message` | 3 |
| fixer completes → resumes same turn, context grew by one line | `returns within 4 s of the lock clearing`, `additionalContext` = the one line + `Continue with your task.`, `handover is consumed`, `no replay` | 3 |
| fixer `kill -9` → resumes within the deadline, stale warning | `a dead fixer does not hold` + `and the agent is warned` (stale-pid path); `a terminated wrapper releases the lock` (SIGTERM path) | 3, 1 |
| two spawns on one pane → second refuses | `a second spawn on a locked pane refuses (exit 1)`, `only one fixer pane exists` | 2 |
| long fixer (> deadline) → proceeds, warned, not wedged | `an expired deadline does not hold` | 3 |
| the fence | `both dirty paths are denied to Edit`, `a clean path is not fenced`, `mutating git is denied` | 2 |
| the wrapper owns lock + handover | `wrapper exits 0`, `handover verbatim`, `lock released`, `no HANDOVER line → fallback`, `capped at 300` | 1 |
| ordering with a queued review | `handover first, then the queued review, one payload` | 3 |
| `stop_hook_active` guard | `stop_hook_active true never holds` | 3 |
| every hook field traces to an `[E]` | reviewer checklist (Task 4) | 4 |
| freeze rig / leak sweep | Task 4 steps 1–2 | 4 |
| Esc during a hold | manual smoke, README fact | 4 |

## Self-review

**Spec coverage:** brief Goal 1–4 ✔ (Tasks 2, 3); Non-goals respected (not a review channel — only ordered; no orchestrator; a pane, not a subagent); Phase 0 ✔ done (hook-facts + handoff-check; C10 negative); Phase 1 lock ✔ Task 1 (directory, fail-open rules in Task 3, deadline default 300 s); Phase 2 spawn ✔ Task 2 (lock before spawn, `split-window -d`, fence, refuse duplicate, handover contract in the prompt); Phase 3 hold ✔ Task 3 (1–6 of the brief's list; ordering handover-first); DON'Ts ✔ (no interrupt; one line ≤300; no block-and-rewake; deadline+lock guards; fence; no Task subagent; every field `[E]`; review path untouched beyond ordering); Acceptance ✔ the rig table.

**Placeholder scan:** none; every step carries code or an exact command; the hooks-merge dedupe question in Task 4 step 6 is stated with its fix.

**Type consistency:** `tmux-review --sess <pane>` (Task 1) is what Task 2's `spawn_fixer` and Task 3's hold call; lock files `pid pane reason deadline session fence` are written by Task 2 (`reason deadline pane fence`) and Task 1 (`pid session`) and read by Task 3 (`reason pid deadline pane`); `handover` written by Task 1, consumed by Task 3; `@cockpit_hold` set/unset in Task 3, shown by `--info` in Task 3; `FAKE_CLAUDE_RESULT/SLEEP/ARGS` used identically in Tasks 1–2.
