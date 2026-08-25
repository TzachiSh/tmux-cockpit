# Side-Fixer Panes + Boundary Handover Implementation Plan (v2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the human never presses Esc to redirect a busy agent onto an orthogonal job again — a **side-fixer** (a separate `claude -p` in its own pane, own context, scope-fenced) does the job while the main agent keeps working; the one unsafe action (running the thing being repaired) is **denied** meanwhile; the main agent is **held at its next Stop** while the fixer is mid-flight and resumes on that same turn with **one handover line**.

**Architecture:** four pieces, no lock file. (1) **Liveness is the pane**: `tmux split-window -d -P -F '#{pane_id}'` records the fixer's pane id; a dead process cannot leave a live pane, so `list-panes -a` is the truth — no PID, no `EXIT` trap, no stale-file logic. "Done" = `handover` exists **or** the pane is gone **or** the deadline passed (the first clause covers a failed fixer whose pane stays open for the human to read). (2) `scripts/tmux-fixer`: `-p <pane>` (the `prefix f` popup: one line for the job, one optional line for the **guard pattern** — the suite command to hold off; computes the scope fence from the main pane's dirty files; records the fixer *before* spawning; spawns; returns) and `--run` (inside the new pane: runs `claude -p … --output-format json`, extracts the final `HANDOVER:` line from `.result`, writes `handover`; the model writes nothing). (3) **PreToolUse gate** (`review-gate --pretool`, matcher `Bash`): while the fixer is alive, a Bash command matching the guard is denied with a reason; the deny **ends the turn by default** (v2.1.210+), so the model cannot route around it in-turn — the Stop that follows is where the handover lands. The no-fixer path is one `stat`. (4) **Stop hold** in `review-gate` (phase 3's hook): no fixer → zero added latency; fixer → poll 1 s until done; the handover line (or the stale warning) goes into `additionalContext` **ahead of** any queued review. Fail-open everywhere. No new daemon, no subagent, no Agent Teams (Phase 0 D: teammates can only be spawned by the lead; `TeammateIdle`/`TaskCompleted` are teams-only; the field's answer to contention is partitioning, not holding).

**Tech Stack:** bash 4+, tmux ≥ 3.3, git, Claude Code **2.1.245**: `claude -p --output-format json` (`.result` verified live), `--session-id`, `--permission-mode acceptEdits`, `--allowedTools`/`--disallowedTools` (`Edit(<glob>)` only — `Write()` path rules are never consulted since v2.1.210), Stop `hookSpecificOutput.additionalContext`, PreToolUse `hookSpecificOutput.permissionDecision: "deny"` + `permissionDecisionReason` (default: the turn ends; `continueOnBlock: true` not set), per-hook `timeout`/`statusMessage`. No jq on a hook path.

**Spec:** the owner's brief v2 `docs/superpowers/research/2026-08-25-side-fixer/brief.md` (Goal, Non-goals, Phase 0 A–D, Phase 1 liveness, Phase 2 spawn, Phase 3a gate, Phase 3b hold, DON'Ts, Acceptance); Phase 0 facts `docs/superpowers/research/2026-08-25-side-fixer/hook-facts.md` (A–E, and §D build-vs-buy) and `…/handoff-check-report.md`; phase 3's plan `2026-08-25-review-phase3.md` (review-gate, session dir, `--delta`, the drain, the JSON escaper). Ponytail audit below.

**Branch:** `feat/side-fixer`, worktree `~/.cache/cockpit-wt/side-fixer`, off master **after phase 3 merges**. Roadmap: phase 3c.

## Global Constraints

- Everything in phase 3's Global Constraints (`docs/superpowers/plans/2026-08-25-review-phase3.md`): no shell on a redraw, hooks fail-open, one-shot scripts, no daemon, quoted paths, private `-L` sockets, no new fixed `sleep`, ponytail.
- **The main agent is never interrupted**: no Esc, no `send-keys`, no synthetic keystrokes. The gate denies one tool call; the hold happens inside the Stop hook the agent itself fires.
- **No lock file.** The fixer's pane is its liveness. Two files carry state: `<sess>/fixer/` (created atomically by `mkdir` at spawn; holds `pane`, `reason`, `deadline`, `fence`, `guard`) and `<sess>/handover` (one line). A stale `fixer/` (pane gone, or `handover` present) is *free* at every reader — spawn guard, gate, hold.
- **The fixer's transcript never reaches the main agent**: one `HANDOVER:` line, ≤300 characters, written by the wrapper.
- **No block-and-rewake**: one Stop invocation poll-holds; never `decision: block`, never exit 2 for the hold. `stop_hook_active: true` → no hold.
- **Never hold without a deadline** (default 300 s in `fixer/deadline`; the review-gate hook entry's `timeout` is 330) **and never hold when nothing is running**.
- **The gate denies only the guarded command**, only while the fixer is alive; everything else runs. The no-fixer path is a single `stat` of a per-pane marker — no git, no tmux, no payload read.
- **The fixer never writes the main agent's live edit surface**: the fence = the main pane's dirty paths at spawn (`git status --porcelain --no-optional-locks`, tracked + untracked) as `--disallowedTools 'Edit(./<path>)'`, plus mutating git subcommands denied.
- Not a Task subagent, not a review channel (the review path is only *ordered* after the handover), not Agent Teams.
- Every hook field used traces to an `[E]` line in `hook-facts.md` / `handoff-check-report.md`.

## Ponytail audit — the brief v2 against this plan

| brief item | verdict | why |
|---|---|---|
| no lock file; pane liveness | **adopt** | one tmux call answers "alive"; PID, trap and stale-cleanup all go. |
| "the fixer writes `handover` and releases the lock" (Phase 2 text) | **replace**: the wrapper writes `handover`; nothing "releases" anything | the model writing a file is the common failure; `.result`'s last `HANDOVER:` line is extracted by the wrapper (verified). There is no lock to release — the pane exits. |
| remain-on-exit caveat: "decide which, and write it down" | **decided**: done = `handover` ∨ pane dead ∨ deadline | a failed fixer keeps its pane open (`↵ to close`) so the human can read why; its `handover` (the fallback line) is written first, so the hold and the gate open on the file, not the pane. Written into `review-gate`'s comment. |
| Phase 3a gate on "the test/suite command" | **adopt, with the pattern from the human** | nothing else knows which command is "the suite": the popup's second line is an ERE (`↵` = no gate). Default ends the turn (v2.1.210+), so no wording engineering — the Stop hold that follows delivers the handover. `continueOnBlock` stays off (an opt-in noted in the README). |
| "log to stderr so the human sees why the pane is paused" | **replace** with `@cockpit_hold` + one `display-message` + the entry's `statusMessage` | a Stop hook's stderr is not shown live (B7). |
| "Writes the lock before spawning" | **keep the order**: `fixer/` is created (and the marker written) before `split-window`; `pane` is filled after | a boundary between the two sees `fixer/` with `pane=pending` → treated as alive until the deadline (the split takes milliseconds). |
| Phase 0 D build-vs-buy | **done** (§D of hook-facts): Agent Teams no (lead-only spawn), teammate hooks teams-only, field silent on holding | the minimal design stands. |
| a fixer worktree | **skip** | the fixer's fix must be visible to the main agent (that is the point); the fence covers the live surface. |
| configurable model/flags | **one option** `@cockpit_fixer_deadline` (default 300) | YAGNI. |
| Phase 4 "fresh reviewer" | **the SDD loop** — a reviewer per task plus the whole-branch review | already the process. |

---

### Task 1: The wrapper — `tmux-fixer --run`, the handover, and a fake `claude`

**Files:**
- Create: `scripts/tmux-fixer` (executable), `tests/fixtures/fake-claude` (executable).
- Modify: `scripts/tmux-review` — `--sess <pane>` (prints the pane's session dir, created 0700); `Makefile` `SOURCES` gains `tests/fixtures/fake-claude`.
- Test: `tests/run.sh` — `== tmux-fixer --run: one HANDOVER line from claude -p's result, the fallback when it forgot, a failure that stays readable ==`.

**Interfaces:**
- Produces: `tmux-review --sess <pane>` → path (exit 1 when the pane is gone). `tmux-fixer --run <main-pane> <sess> <job…>`: reads `<sess>/fixer/fence` and `<sess>/fixer/reason`; runs `claude -p`; writes `<sess>/handover` (one line ≤300 chars) — the extracted `HANDOVER:` line, else the fallback `HANDOVER: side task finished without a handover line (<reason>); re-check anything it may have touched`; exit 0 on success (the pane closes); on `claude` failure prints the output, writes the fallback handover, waits `↵ to close`, exit 1. `tests/fixtures/fake-claude`: honours `-p`/`--output-format json`; `.result` = `$FAKE_CLAUDE_RESULT`; sleeps `$FAKE_CLAUDE_SLEEP`; records argv to `$FAKE_CLAUDE_ARGS`; exits `$FAKE_CLAUDE_RC` (default 0).

- [ ] **Step 1: The fake `claude`**

`tests/fixtures/fake-claude`:

```bash
#!/usr/bin/env bash
# A `claude` the suite can afford: honours -p and --output-format json, prints
# one result document. Records argv so a test can assert the fence; sleeps so
# a test can keep a fixer alive; exits FAKE_CLAUDE_RC so a test can fail one.
set -u
[ -n "${FAKE_CLAUDE_ARGS:-}" ] && printf '%s\n' "$@" > "$FAKE_CLAUDE_ARGS"
sleep "${FAKE_CLAUDE_SLEEP:-0}"
res=${FAKE_CLAUDE_RESULT-HANDOVER: fixed the thing; suite 118s → 8s; re-read tests/}
res=${res//\\/\\\\}; res=${res//\"/\\\"}
printf '{"type":"result","subtype":"success","is_error":false,"result":"%s","session_id":"fake-session"}\n' "$res"
exit "${FAKE_CLAUDE_RC:-0}"
```

- [ ] **Step 2: The failing test**

```bash
echo "== tmux-fixer --run: one HANDOVER line from claude -p's result, the fallback when it forgot, a failure that stays readable =="
FX=$(mktmpd); FXSESS="$FX/sess"; mkdir -p "$FXSESS/fixer"; printf 'fix the tests\n' > "$FXSESS/fixer/reason"; : > "$FXSESS/fixer/fence"
FXBIN=$(mktmpd); ln -s "$DIR/tests/fixtures/fake-claude" "$FXBIN/claude"; FXARGS=$(mktmpf)
PATH="$FXBIN:$PATH" FAKE_CLAUDE_ARGS="$FXARGS" "$DIR/scripts/tmux-fixer" --run %99 "$FXSESS" fix the tests >/dev/null 2>&1 </dev/null
is "a HANDOVER result exits 0" "0" "$?"
is "the handover is that one line, verbatim" "HANDOVER: fixed the thing; suite 118s → 8s; re-read tests/" "$(cat "$FXSESS/handover" 2>/dev/null)"
case "$(cat "$FXARGS")" in *"-p"*"--output-format"*"json"*"--permission-mode"*"acceptEdits"*"--session-id"*) ok "claude -p: headless, json, acceptEdits, a chosen session id" ;; *) bad "claude -p: headless, json, acceptEdits, a chosen session id" "-p … json … acceptEdits … --session-id" "$(cat "$FXARGS")" ;; esac
case "$(cat "$FXARGS")" in *"HANDOVER: "*) ok "the prompt states the one-line contract" ;; *) bad "the prompt states the one-line contract" "HANDOVER: in the prompt" "$(tail -3 "$FXARGS")" ;; esac
rm -f "$FXSESS/handover"
PATH="$FXBIN:$PATH" FAKE_CLAUDE_RESULT='I did things, here is a long explanation' "$DIR/scripts/tmux-fixer" --run %99 "$FXSESS" fix >/dev/null 2>&1 </dev/null
is "no HANDOVER line: the fallback names the job" "HANDOVER: side task finished without a handover line (fix the tests); re-check anything it may have touched" "$(cat "$FXSESS/handover" 2>/dev/null)"
rm -f "$FXSESS/handover"
PATH="$FXBIN:$PATH" FAKE_CLAUDE_RESULT="HANDOVER: $(printf 'x%.0s' $(seq 1 400))" "$DIR/scripts/tmux-fixer" --run %99 "$FXSESS" fix >/dev/null 2>&1 </dev/null
is "a long handover is capped at 300 characters" "300" "$(awk '{ print length }' "$FXSESS/handover")"
rm -f "$FXSESS/handover"
# claude fails: the fallback handover is written FIRST (the hold opens on it), the wrapper exits 1 and waits for ↵ (stdin here is /dev/null, so it returns).
PATH="$FXBIN:$PATH" FAKE_CLAUDE_RC=1 FAKE_CLAUDE_RESULT='' "$DIR/scripts/tmux-fixer" --run %99 "$FXSESS" fix >/dev/null 2>&1 </dev/null
is "a failed claude exits 1" "1" "$?"
case "$(cat "$FXSESS/handover" 2>/dev/null)" in HANDOVER:*"did not"*|HANDOVER:*"without a handover"*) ok "and still leaves a handover so nobody waits on it" ;; *) bad "and still leaves a handover so nobody waits on it" "a HANDOVER: fallback line" "$(cat "$FXSESS/handover" 2>/dev/null)" ;; esac
rm -rf "$FX" "$FXBIN"
```

- [ ] **Step 3: Run to see it fail** — `make test-only PAT='tmux-fixer --run'`: every line FAIL.

- [ ] **Step 4: `scripts/tmux-fixer` (the `--run` half)**

```bash
#!/usr/bin/env bash
# tmux-fixer — a side-fixer pane beside a busy agent, and the wrapper that runs
# inside it. The human spawns a separate `claude -p` for an orthogonal job (fix
# the tests, regenerate a fixture) instead of interrupting the main agent; the
# main agent is held at its next Stop by review-gate while the fixer's PANE is
# alive, and resumes with ONE handover line. There is no lock file: a dead
# process cannot leave a live pane.
#   tmux-fixer -p <pane>                     the prefix f popup: job, guard, fence, spawn
#   tmux-fixer --run <pane> <sess> <job…>    inside the fixer pane: claude -p, then the handover
# State: <sess>/fixer/{pane,reason,deadline,fence,guard} and <sess>/handover.
set -uo pipefail
SELF="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
REVIEW="$(dirname "$SELF")/tmux-review"
shq() { printf '%q' "$1"; }

# --- --run: the wrapper --------------------------------------------------------
# The MODEL writes nothing: the wrapper runs the fixer and turns the last
# "HANDOVER: …" line of claude -p's .result into <sess>/handover -- the ONLY
# thing that reaches the main agent. On any failure the fallback line is
# written FIRST (the hold and the gate open on the file), then the output stays
# on screen for the human (↵ closes the pane).
handover_write() {            # <sess> <line>
    local line=${2//[$'\001'-$'\037']/}
    [ "${#line}" -gt 300 ] && line="${line:0:287}… (truncated)"
    (umask 077; printf '%s\n' "$line" > "$1/handover")
}
run_fixer() {                 # <main-pane> <sess> <job…>
    local pane=$1 sess=$2; shift 2
    local job="$*" f="$sess/fixer" sid out res rc reason line fence
    reason=$(cat -- "$f/reason" 2>/dev/null); reason=${reason:-$job}
    fence=$(tr '\n' ' ' < "$f/fence" 2>/dev/null)
    sid=$(uuidgen 2>/dev/null || cat /proc/sys/kernel/random/uuid 2>/dev/null || printf '%s-%s' "$$" "$(date +%s)")
    printf 'side fix for %s: %s\n(this pane closes itself when done; it stays open if claude fails)\n\n' "$pane" "$reason"
    # The fence: every path the main agent has dirty is denied to Edit (Write
    # path rules are never consulted -- permissions.md, v2.1.210+); mutating
    # git is denied outright; acceptEdits so a headless run never stalls on a
    # prompt nobody can answer.
    local -a deny=()
    while IFS= read -r p; do [ -n "$p" ] && deny+=("Edit(./$p)"); done < "$f/fence"
    for g in commit push checkout switch stash reset rebase merge restore; do deny+=("Bash(git $g *)"); done
    out=$(claude -p "$job

You are a SIDE TASK running beside another agent that is working in this repository right now. Do only this job. Do not touch these files, they are the other agent's live edits: ${fence:-none}. Do not commit. When you are done, your final message must be exactly ONE line that starts with 'HANDOVER: ' and states what changed and the observable delta (for example 'HANDOVER: tests fixed; suite 118s -> 8s; re-read any test file you had open'). No reasoning, no summary, one line." \
        --output-format json --permission-mode acceptEdits --max-turns 60 --session-id "$sid" \
        --allowedTools 'Bash(*)' --disallowedTools "${deny[@]}" 2>&1)
    rc=$?
    if command -v jq >/dev/null 2>&1; then res=$(printf '%s' "$out" | jq -r '.result // empty' 2>/dev/null)
    else res=$(printf '%s' "$out" | grep -o '"result":"[^"]*"' | head -1 | sed 's/^"result":"//; s/"$//'); fi
    line=$(printf '%s\n' "$res" | grep '^HANDOVER: ' | tail -1)
    [ -n "$line" ] || line="HANDOVER: side task finished without a handover line ($reason); re-check anything it may have touched"
    handover_write "$sess" "$line"
    if [ "$rc" -ne 0 ] || [ -z "$res" ]; then
        printf '%s\n\nclaude exited %s — the other agent has been told the job did not finish cleanly.\n↵ to close\n' "$out" "$rc"
        read -r _ 2>/dev/null
        exit 1
    fi
    printf '%s\n' "$line"
    exit 0
}

case "${1:-}" in
    --run) shift; run_fixer "${1:-}" "${2:-}" "${@:3}" ;;
    -p) shift; spawn_fixer "${1:-}" ;;       # Task 2
    *) printf 'usage: tmux-fixer -p <pane> | --run <pane> <sess> <job…>\n' >&2; exit 2 ;;
esac
```

`scripts/tmux-review`: `--sess) shift; sess_mode "${1:-}" ;;` with

```bash
sess_mode() {                 # <pane> -> the review session dir for the pane's repo root, created 0700
    local pane=$1 dir root sess
    dir=$(tmux display-message -p -t "$pane" '#{pane_current_path}' 2>/dev/null); [ -n "$dir" ] || return 1
    root=$(git_root "$dir"); sess=$(session_dir "$pane" "$root")
    (umask 077; mkdir -p -- "$sess") && printf '%s\n' "$sess"
}
```

`usage()`: `  tmux-review --sess <pane>             the review session dir for <pane> (tmux-fixer, review-gate)`.

- [ ] **Step 5: Run; commit** — `make test-only PAT='tmux-fixer --run'` all PASS; `make lint` clean.

```bash
git add scripts/tmux-fixer tests/fixtures/fake-claude scripts/tmux-review tests/run.sh Makefile
git commit -m "feat: tmux-fixer --run turns a headless claude -p into one HANDOVER line — the fence as Edit() denies, a fallback line when it forgot, a failure that stays readable"
```

---

### Task 2: The spawn path — `prefix f`, the fence, the guard, the record, the pane

**Files:**
- Modify: `scripts/tmux-fixer` — `spawn_fixer()`, `fixer_alive()`; `cockpit.tmux` — `prefix f` (`@cockpit_fixer_key`, `off` keeps stock `find-window`), `@cockpit_fixer_deadline`; `README.md` — "Side fixes".
- Test: `tests/run.sh` — `== tmux-fixer -p: one popup records the fixer, fences it, spawns its pane, refuses a second while it lives, frees a stale record ==`.

**Interfaces:**
- Produces: `tmux-fixer -p <pane>`: line 1 = the job (empty → exit 0), line 2 = the guard ERE (`↵` = none); `sess=$(tmux-review --sess <pane>)`; if `<sess>/fixer/` exists and `fixer_alive <sess>` → refuse (`display-message` names the job + pane; exit 1); a stale record is removed; `mkdir "$sess/fixer"` (atomic) → write `reason`, `deadline` (`now + @cockpit_fixer_deadline`, default 300), `guard`, `fence` (dirty paths), `pane=pending`; write the per-pane marker `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/review/fixer-<pane#>` containing `<sess>` (the gate's one-stat fast path); `tmux split-window -d -v -l 40% -t <pane> -c <root> -P -F '#{pane_id}' "<tmux-fixer> --run <pane> <sess> <job>"` → `pane`; one `display-message`; exit 0. `fixer_alive <sess>` (shared by Tasks 3–4, defined here in `tmux-fixer` and mirrored by a 6-line function in `review-gate` — the one duplication, on purpose: a hook does not exec a second script to answer a stat): exit 0 iff `<sess>/fixer/` exists, `<sess>/handover` does not, the deadline has not passed, and the recorded pane is `pending` or present in `tmux list-panes -a -F '#{pane_id}'`.

- [ ] **Step 1: The failing test**

```bash
echo "== tmux-fixer -p: one popup records the fixer, fences it, spawns its pane, refuses a second while it lives, frees a stale record =="
if need_tmux "tmux-fixer -p"; then :; else
SF="cockpit-sf-$$"; SOCKS="$SOCKS $SF"; SFSTATE=$(mktmpd); SFDIR=$(mktmpd); SFBIN=$(mktmpd); ln -s "$DIR/tests/fixtures/fake-claude" "$SFBIN/claude"
( cd "$SFDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > api.go; printf 'x\n' > clean.go; git add -A; git commit -q -m init
  printf 'a\nEDITING\n' > api.go; printf 'new\n' > scratch.txt ) >/dev/null 2>&1
"$TMUX_REAL" -L "$SF" -f /dev/null new-session -d -s t -x 120 -y 40 -c "$SFDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$SF" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$SF" set-environment -g PATH "$SFBIN:$PATH" >/dev/null 2>&1
SFAP=$("$TMUX_REAL" -L "$SF" list-panes -t t: -F '#{pane_id}')
SFSESS="$SFSTATE/tmux-cockpit/review/${SFAP#%}-$(printf '%s' "$SFDIR" | cksum | cut -d' ' -f1)"
SFARGS=$(mktmpf)
sf_spawn() {                  # <job> <guard> <extra-env>  -> SRC= on stdout file $SFSTATE/out
    "$TMUX_REAL" -L "$SF" run-shell "cd $(printf '%q' "$SFDIR") && printf '%s\\n%s\\n' $(printf '%q' "$1") $(printf '%q' "$2") | XDG_STATE_HOME=$(printf '%q' "$SFSTATE") ${3:-} '$DIR/scripts/tmux-fixer' -p $SFAP; echo SRC=\$?" > "$SFSTATE/out" 2>&1
}
sf_spawn 'fix the tests' 'make test|pytest' "FAKE_CLAUDE_ARGS=$(printf '%q' "$SFARGS") FAKE_CLAUDE_SLEEP=3"
case "$(cat "$SFSTATE/out")" in *SRC=0*) ok "the spawn returns 0 at once" ;; *) bad "the spawn returns 0 at once" "SRC=0" "$(cat "$SFSTATE/out")" ;; esac
is "the record holds the job" "fix the tests" "$(cat "$SFSESS/fixer/reason" 2>/dev/null)"
is "and the guard pattern" "make test|pytest" "$(cat "$SFSESS/fixer/guard" 2>/dev/null)"
is "the fence is the main pane's dirty paths, tracked and untracked" "api.go
scratch.txt" "$(sort "$SFSESS/fixer/fence" 2>/dev/null)"
[ "$(cat "$SFSESS/fixer/deadline" 2>/dev/null)" -gt "$(date +%s)" ] && ok "the deadline is in the future" || bad "the deadline is in the future" "> now" "$(cat "$SFSESS/fixer/deadline" 2>/dev/null)"
is "the gate's marker names the session dir" "$SFSESS" "$(cat "$SFSTATE/tmux-cockpit/review/fixer-${SFAP#%}" 2>/dev/null)"
SFFP=$(cat "$SFSESS/fixer/pane" 2>/dev/null)
case "$SFFP" in %[0-9]*) ok "the fixer pane id is recorded" ;; *) bad "the fixer pane id is recorded" "%N" "$SFFP" ;; esac
is "the fixer pane runs in the repo root" "$SFDIR" "$("$TMUX_REAL" -L "$SF" display-message -p -t "$SFFP" '#{pane_current_path}' 2>/dev/null)"
until_file "$SFARGS" '*--disallowedTools*'
case "$(cat "$SFARGS")" in *"Edit(./api.go)"*"Edit(./scratch.txt)"*|*"Edit(./scratch.txt)"*"Edit(./api.go)"*) ok "both dirty paths are denied to Edit" ;; *) bad "both dirty paths are denied to Edit" "Edit(./api.go), Edit(./scratch.txt)" "$(cat "$SFARGS")" ;; esac
case "$(cat "$SFARGS")" in *"Edit(./clean.go)"*) bad "a clean path is not fenced" "no Edit(./clean.go)" "$(cat "$SFARGS")" ;; *) ok "a clean path is not fenced" ;; esac
case "$(cat "$SFARGS")" in *"Bash(git commit *)"*"Bash(git push *)"*) ok "mutating git is denied" ;; *) bad "mutating git is denied" "Bash(git commit *) … Bash(git push *)" "$(cat "$SFARGS")" ;; esac
sf_spawn 'another' '' ''
case "$(cat "$SFSTATE/out")" in *SRC=1*) ok "a second spawn while the fixer lives refuses (exit 1)" ;; *) bad "a second spawn while the fixer lives refuses (exit 1)" "SRC=1" "$(cat "$SFSTATE/out")" ;; esac
is "and the running job is untouched" "fix the tests" "$(cat "$SFSESS/fixer/reason" 2>/dev/null)"
is "and only one fixer pane exists" "2" "$("$TMUX_REAL" -L "$SF" list-panes -a | wc -l)"
# The fixer finishes: handover written, the pane closes itself.
until_file "$SFSESS/handover" '*HANDOVER*' && ok "the handover is written when the fixer finishes" || bad "the handover is written when the fixer finishes" "HANDOVER line" "$(cat "$SFSESS/handover" 2>/dev/null)"
for i in $(seq 1 40); do [ "$("$TMUX_REAL" -L "$SF" list-panes -a | wc -l)" = 1 ] && break; sleep 0.1; done
is "the fixer pane closes itself on success" "1" "$("$TMUX_REAL" -L "$SF" list-panes -a | wc -l)"
# A stale record (handover present, pane gone) does not block the next spawn: it is freed and replaced.
sf_spawn 'second job' '' "FAKE_CLAUDE_SLEEP=2"
case "$(cat "$SFSTATE/out")" in *SRC=0*) ok "a finished fixer's record does not block a new spawn" ;; *) bad "a finished fixer's record does not block a new spawn" "SRC=0" "$(cat "$SFSTATE/out")" ;; esac
is "the new job replaced the old record" "second job" "$(cat "$SFSESS/fixer/reason" 2>/dev/null)"
is "and the old handover was not carried over" "absent" "$([ -e "$SFSESS/handover" ] && echo present || echo absent)"
"$TMUX_REAL" -L "$SF" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
case "$("$TMUX_REAL" -L "$SF" list-keys -T prefix 2>/dev/null | awk '$4=="f"')" in *tmux-fixer*) ok "prefix f is bound to the side-fix popup" ;; *) bad "prefix f is bound to the side-fix popup" "display-popup … tmux-fixer" "$("$TMUX_REAL" -L "$SF" list-keys -T prefix | awk '$4=="f"')" ;; esac
unset -f sf_spawn
"$TMUX_REAL" -L "$SF" kill-server >/dev/null 2>&1; rm -rf "$SFBIN"
fi
```

(The "old handover not carried over": a new spawn on a stale record removes `handover` too — a handover nobody consumed belongs to a job that already ended; the Stop hook consumes a live one before this can happen in practice.)

- [ ] **Step 2: Run to see it fail** — `make test-only PAT='tmux-fixer -p'`: every line FAIL.

- [ ] **Step 3: Implement**

```bash
# --- liveness ------------------------------------------------------------------
# The fixer is alive iff its record exists, no handover has been written, the
# deadline has not passed, and its pane still exists (or is still "pending",
# the milliseconds between the record and the split). No pid, no lock: a dead
# process cannot leave a live pane. Mirrored in review-gate on purpose.
fixer_alive() {               # <sess>
    local f="$1/fixer" p dl
    [ -d "$f" ] || return 1
    [ -f "$1/handover" ] && return 1
    dl=$(cat -- "$f/deadline" 2>/dev/null); case "$dl" in ''|*[!0-9]*) return 1 ;; esac
    [ "$(date +%s)" -lt "$dl" ] || return 1
    p=$(cat -- "$f/pane" 2>/dev/null)
    [ "$p" = pending ] && return 0
    tmux list-panes -a -F '#{pane_id}' 2>/dev/null | grep -qx -- "$p"
}

# --- -p: the popup ---------------------------------------------------------------
# Two lines from the human (the job; the command pattern to hold off, ↵ for
# none), then: the fence, the RECORD (mkdir, atomic -- a second spawn refuses
# here while the first lives; a stale record is freed), the marker the gate
# stats, then the pane, spawned by the tmux SERVER (split-window -d) so nothing
# about this popup's lifetime touches the fixer.
spawn_fixer() {               # <main-pane>
    local pane=$1 job guard sess f root dl fp marker
    case "$pane" in %[0-9]*) ;; *) printf 'tmux-fixer: not a pane id: %s\n' "$pane" >&2; exit 2 ;; esac
    sess=$("$REVIEW" --sess "$pane") || { tmux display-message "tmux-fixer: pane $pane is gone" 2>/dev/null; exit 1; }
    f="$sess/fixer"
    if fixer_alive "$sess"; then
        tmux display-message "tmux-fixer: a side fix is already running for $pane: $(cat -- "$f/reason" 2>/dev/null) (pane $(cat -- "$f/pane" 2>/dev/null))" 2>/dev/null
        exit 1
    fi
    if [ -t 0 ]; then
        read -r -p 'side fix (one line, ↵ alone cancels): ' job
        read -r -p 'hold off commands matching (ERE, ↵ for none): ' guard
    else IFS= read -r job; IFS= read -r guard; fi
    job=${job//[$'\001'-$'\037']/}; guard=${guard//[$'\001'-$'\037']/}
    [ -n "$job" ] || exit 0
    root=$(git -C "$(tmux display-message -p -t "$pane" '#{pane_current_path}')" rev-parse --show-toplevel 2>/dev/null) || {
        tmux display-message "tmux-fixer: $pane is not in a git repo" 2>/dev/null; exit 1; }
    dl=$(tmux show-option -gqv @cockpit_fixer_deadline); case "$dl" in ''|*[!0-9]*) dl=300 ;; esac
    rm -rf -- "$f" "$sess/handover"            # a stale record and its unconsumed handover
    (umask 077; mkdir -- "$f") 2>/dev/null || { tmux display-message "tmux-fixer: a side fix is already running for $pane" 2>/dev/null; exit 1; }
    printf '%s\n' "$job" > "$f/reason"; printf '%s\n' "$guard" > "$f/guard"
    printf '%s\n' $(( $(date +%s) + dl )) > "$f/deadline"; printf 'pending\n' > "$f/pane"
    git -C "$root" --no-optional-locks -c core.quotePath=false status --porcelain 2>/dev/null \
        | sed 's/^...//; s/^.* -> //' > "$f/fence"
    marker="${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/review/fixer-${pane#%}"
    (umask 077; printf '%s\n' "$sess" > "$marker")
    fp=$(tmux split-window -d -v -l 40% -t "$pane" -c "$root" -P -F '#{pane_id}' \
        "$(shq "$SELF") --run $(shq "$pane") $(shq "$sess") $(shq "$job")") || {
        rm -rf -- "$f" "$marker"; tmux display-message "tmux-fixer: could not open the fixer pane" 2>/dev/null; exit 1; }
    printf '%s\n' "$fp" > "$f/pane"
    tmux display-message "side fix running in $fp: $job — $pane is held at its next stop until it finishes${guard:+; commands matching /$guard/ are held off}" 2>/dev/null
    exit 0
}
```

`cockpit.tmux` (after the info binding):

```bash
# prefix f: a side fix beside a busy agent -- two lines, a fenced claude -p in
# its own pane, the agent's suite command held off and its next Stop held
# until one handover line. Stock f is find-window; @cockpit_fixer_key off keeps it.
Q_FIXER=$(sq "$DIR/scripts/tmux-fixer")
FK=$(tmux show-option -gqv @cockpit_fixer_key)
case "$FK" in
    off) ;;
    "")  tmux bind-key f display-popup -E -w "$PW" -h 6 -T ' side fix · #{pane_id} ' "$Q_FIXER -p '#{pane_id}'" ;;
    *)   tmux bind-key "$FK" display-popup -E -w "$PW" -h 6 -T ' side fix · #{pane_id} ' "$Q_FIXER -p '#{pane_id}'" ;;
esac
```

README "Side fixes": the flow in five lines, the two prompts, the fence, the guard, the hold, the options (`@cockpit_fixer_key`, `@cockpit_fixer_deadline`), the DON'Ts (not a review channel, not a subagent), and "a failed fixer keeps its pane open; `↵` closes it".

- [ ] **Step 4: Run; commit**

```bash
git add scripts/tmux-fixer cockpit.tmux README.md tests/run.sh
git commit -m "feat: prefix f spawns a fenced side fixer beside a busy agent — the record and the gate's marker first, then the pane; a second spawn refuses while it lives; liveness is the pane"
```

---

### Task 3: The gate — `review-gate --pretool` denies the guarded command while the fixer lives

**Files:**
- Modify: `scripts/review-gate` — a `--pretool` entry (before the Stop logic; the script takes one optional argument), `fixer_alive()` (mirror), the marker fast path; `examples/claude-hooks.json` — a `PreToolUse` group with `"matcher": "Bash"` running `review-gate --pretool` (`"timeout": 5`); `README.md` hooks table; `install.sh` — no change (a new group appends).
- Test: `tests/run.sh` — `== review-gate --pretool: the guarded command is denied while the fixer lives, one stat when it does not ==`.

**Interfaces:**
- Produces: `review-gate --pretool` reads `$TMUX_PANE`; if the marker `…/review/fixer-<pane#>` is absent → exit 0 (no payload read, no tmux, no git); else `sess=$(cat marker)`; if `! fixer_alive "$sess"` → `rm -f marker`; exit 0; else read the payload (bounded), pull `tool_input.command` (the `"command":"…"` field; an escaped quote truncates it — matching on the prefix is the accepted ceiling), `guard=$(cat "$sess/fixer/guard")`; if `guard` is empty or the command does not match (`grep -Eq`) → exit 0; else print `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"<reason>"}}` with `<reason>` = `the side fix "<job>" is repairing this right now (pane <id>); this run would give invalid results — skip it and continue with your task; you will get a one-line handover when it finishes.` (escaped by the phase-3 JSON escaper), exit 0.

- [ ] **Step 1: The failing test**

```bash
echo "== review-gate --pretool: the guarded command is denied while the fixer lives, one stat when it does not =="
if need_tmux "review-gate --pretool"; then :; else
PG="cockpit-pg-$$"; SOCKS="$SOCKS $PG"; PGSTATE=$(mktmpd); PGDIR=$(mktmpd)
( cd "$PGDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e; git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init ) >/dev/null 2>&1
"$TMUX_REAL" -L "$PG" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$PGDIR" 'exec -a claude bash' >/dev/null 2>&1
PGAP=$("$TMUX_REAL" -L "$PG" list-panes -t t: -F '#{pane_id}')
PGFX=$("$TMUX_REAL" -L "$PG" split-window -d -t t -P -F '#{pane_id}' 'sleep 300')       # a live "fixer" pane
PGSESS="$PGSTATE/tmux-cockpit/review/${PGAP#%}-$(printf '%s' "$PGDIR" | cksum | cut -d' ' -f1)"
mkdir -p "$PGSESS/fixer"; printf 'fix the tests\n' > "$PGSESS/fixer/reason"; printf 'make test|pytest\n' > "$PGSESS/fixer/guard"
printf '%s\n' $(( $(date +%s) + 300 )) > "$PGSESS/fixer/deadline"; printf '%s\n' "$PGFX" > "$PGSESS/fixer/pane"
pg_run() {                    # <command> [marker-present:1|0] -> stdout in $PGSTATE/out, rc in $PGSTATE/rc
    printf '{"session_id":"s","transcript_path":"/x","cwd":"%s","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"%s","description":"d"}}\n' "$PGDIR" "$1" > "$PGSTATE/pay"
    "$TMUX_REAL" -L "$PG" run-shell "TMUX_PANE=$PGAP XDG_STATE_HOME=$(printf '%q' "$PGSTATE") '$DIR/scripts/review-gate' --pretool < $(printf '%q' "$PGSTATE/pay") > $(printf '%q' "$PGSTATE/out"); echo \$? > $(printf '%q' "$PGSTATE/rc")" >/dev/null 2>&1
}
# No marker: exit 0, nothing printed, and it never touched tmux or git (PATH holds neither).
printf '{"tool_input":{"command":"make test"}}\n' > "$PGSTATE/pay"
pg_t0=$EPOCHREALTIME; PATH=/nonexistent TMUX_PANE=$PGAP XDG_STATE_HOME=$PGSTATE /bin/bash "$DIR/scripts/review-gate" --pretool < "$PGSTATE/pay" > "$PGSTATE/out" 2>/dev/null; pg_rc=$?
is "no marker: exit 0" "0" "$pg_rc"; is "no marker: nothing printed" "" "$(cat "$PGSTATE/out")"
awk -v a="$pg_t0" -v b="$EPOCHREALTIME" 'BEGIN { exit !((b - a) < 0.2) }' && ok "no marker: one stat, under 200 ms with no tools on PATH" || bad "no marker: one stat, under 200 ms with no tools on PATH" "<0.2s" "$(awk -v a="$pg_t0" -v b="$EPOCHREALTIME" 'BEGIN{print b-a}')s"
# Marker + live fixer + matching command: deny with a reason that names the job and tells the agent to continue.
printf '%s\n' "$PGSESS" > "$PGSTATE/tmux-cockpit/review/fixer-${PGAP#%}"
pg_run 'make test 2>&1 | tail'
case "$(cat "$PGSTATE/out")" in *'"permissionDecision":"deny"'*'"permissionDecisionReason":"the side fix \"fix the tests\" is repairing this right now'*'continue with your task'*) ok "a guarded command is denied, the reason names the job and says continue" ;; *) bad "a guarded command is denied, the reason names the job and says continue" "deny JSON with the job and 'continue with your task'" "$(cat "$PGSTATE/out")" ;; esac
is "and the hook exits 0 (the JSON is the decision)" "0" "$(cat "$PGSTATE/rc")"
pg_run 'ls -la'
is "an unguarded command passes" "" "$(cat "$PGSTATE/out")"
printf '\n' > "$PGSESS/fixer/guard"; pg_run 'make test'
is "an empty guard denies nothing" "" "$(cat "$PGSTATE/out")"
printf 'make test|pytest\n' > "$PGSESS/fixer/guard"
# The fixer's pane dies: the gate opens at once and the marker is dropped.
"$TMUX_REAL" -L "$PG" kill-pane -t "$PGFX"
pg_run 'make test'
is "a dead fixer denies nothing" "" "$(cat "$PGSTATE/out")"
is "and the marker is gone, so the next call is one stat again" "absent" "$([ -e "$PGSTATE/tmux-cockpit/review/fixer-${PGAP#%}" ] && echo present || echo absent)"
# A handover present (failed fixer, pane kept open) also opens the gate.
PGFX=$("$TMUX_REAL" -L "$PG" split-window -d -t t -P -F '#{pane_id}' 'sleep 300'); printf '%s\n' "$PGFX" > "$PGSESS/fixer/pane"
printf '%s\n' "$PGSESS" > "$PGSTATE/tmux-cockpit/review/fixer-${PGAP#%}"; printf 'HANDOVER: gave up\n' > "$PGSESS/handover"
pg_run 'make test'; is "a written handover opens the gate even with the pane alive" "" "$(cat "$PGSTATE/out")"
unset -f pg_run
"$TMUX_REAL" -L "$PG" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 2: Run to see it fail** — `make test-only PAT='review-gate --pretool'`: `no marker: exit 0` may PASS by accident (unknown arg); the deny lines FAIL.

- [ ] **Step 3: Implement**

At the top of `review-gate`, after the pane guard and `here=`:

```bash
# --- fixer liveness (mirror of tmux-fixer's; a hook does not exec a second script for a stat)
fixer_alive() {               # <sess>
    local f="$1/fixer" p dl
    [ -d "$f" ] || return 1
    [ -f "$1/handover" ] && return 1
    dl=$(cat -- "$f/deadline" 2>/dev/null); case "$dl" in ''|*[!0-9]*) return 1 ;; esac
    [ "$(date +%s)" -lt "$dl" ] || return 1
    p=$(cat -- "$f/pane" 2>/dev/null)
    [ "$p" = pending ] && return 0
    tmux list-panes -a -F '#{pane_id}' 2>/dev/null | grep -qx -- "$p"
}
marker="${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/review/fixer-${pane#%}"

# --- --pretool: the gate (PreToolUse, matcher Bash) ------------------------------
# Fires on EVERY Bash call, so the common case is one stat: no marker, exit.
# With a live fixer and a guard pattern, the guarded command is denied with a
# reason; a deny ends the turn by default (hooks-guide, v2.1.210+), which is
# the point -- no in-turn route-around; the Stop that follows holds and hands
# over. The command is pulled from the payload by the field() idiom; an
# escaped quote inside it truncates the match to the prefix -- accepted.
if [ "${1:-}" = --pretool ]; then
    [ -f "$marker" ] || exit 0
    sess=$(cat -- "$marker" 2>/dev/null)
    if ! fixer_alive "$sess"; then rm -f -- "$marker"; exit 0; fi
    guard=$(cat -- "$sess/fixer/guard" 2>/dev/null); [ -n "$guard" ] || exit 0
    # bounded payload read (the same loop the Stop path uses -- hoist it into read_payload() and call it here and there)
    read_payload
    cmd=$(field command)
    printf '%s' "$cmd" | grep -Eq -- "$guard" || exit 0
    reason="the side fix \"$(cat -- "$sess/fixer/reason")\" is repairing this right now (pane $(cat -- "$sess/fixer/pane")); this run would give invalid results — skip it and continue with your task; you will get a one-line handover when it finishes."
    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"%s"}}\n' "$(json_escape "$reason")"
    exit 0
fi
```

(`read_payload` and `json_escape` are phase 3's — Task 5 of that plan wrote the bounded read inline and the escaper for the Stop payload; hoist both into functions here and reuse them.) `examples/claude-hooks.json`:

```json
    "PreToolUse": [
      { "matcher": "", "hooks": [ { "type": "command", "command": "…/scripts/agent-state running" } ] },
      { "matcher": "Bash", "hooks": [ { "type": "command", "command": "\"${TMUX_COCKPIT_DIR:-$HOME/.tmux/plugins/tmux-cockpit}\"/scripts/review-gate --pretool", "timeout": 5 } ] }
    ],
```

README hooks table: ``| `PreToolUse` (`Bash`) | `review-gate --pretool` — while a side fix runs, denies the command pattern the human named; one stat otherwise |``.

- [ ] **Step 4: Run; commit** — `make test-only PAT='review-gate'` all PASS (the phase-3 review-gate block untouched); `make check` once (foreground, tool timeout 600000).

```bash
git add scripts/review-gate examples/claude-hooks.json README.md tests/run.sh
git commit -m "feat: review-gate --pretool denies the guarded command while a side fix lives — one stat otherwise; a deny ends the turn, the Stop hold does the rest"
```

---

### Task 4: The hold — `review-gate` waits for the fixer, then hands over one line ahead of any review

**Files:**
- Modify: `scripts/review-gate` — the hold before the drain; `stop_hook_active`; `@cockpit_hold`; the record and marker cleanup; the payload assembly. `examples/claude-hooks.json` — the review-gate Stop entry: `"timeout": 330`, `"statusMessage": "cockpit: holding for a side fix"`. `scripts/tmux-review --info` — `hold` and `side fix` lines. `install.sh` — replace-by-command for the review-gate Stop entry (see Step 3).
- Test: `tests/run.sh` — `== review-gate: holds at Stop while a side fix lives, resumes with one line, opens on a dead pane, a handover, or the deadline ==`; the phase-3 silent-stdin assertion stays.

**Interfaces:**
- Produces: on Stop, with `fixer_alive` and `stop_hook_active` ≠ true: `@cockpit_hold=<reason>`, one `display-message`, poll 1 s while `fixer_alive`; then unset `@cockpit_hold`; `ctx` = `handover` (consumed) else `a side task ("<reason>") was running and did not finish; earlier results may be stale.`; then `Continue with your task.`; then the review spool if armed; the record `fixer/` and the marker are removed once delivered. Emitted as `{"hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"…"}}`, exit 0. A `handover` with no live fixer is consumed and delivered too (a fixer that finished between two Stops).

- [ ] **Step 1: The failing test**

```bash
echo "== review-gate: holds at Stop while a side fix lives, resumes with one line, opens on a dead pane, a handover, or the deadline =="
if need_tmux "review-gate hold"; then :; else
RH="cockpit-rh-$$"; SOCKS="$SOCKS $RH"; RHSTATE=$(mktmpd); RHDIR=$(mktmpd)
( cd "$RHDIR" || exit; export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e; git init -q -b main .; printf 'a\n' > f; git add -A; git commit -q -m init ) >/dev/null 2>&1
"$TMUX_REAL" -L "$RH" -f /dev/null new-session -d -s t -x 100 -y 30 -c "$RHDIR" 'exec -a claude bash' >/dev/null 2>&1
"$TMUX_REAL" -L "$RH" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
RHAP=$("$TMUX_REAL" -L "$RH" list-panes -t t: -F '#{pane_id}')
RHCTL=$("$TMUX_REAL" -L "$RH" split-window -t t -P -F '#{pane_id}' 'exec bash'); until_cap "$RH" "$RHCTL" '*$*'
RHSESS="$RHSTATE/tmux-cockpit/review/${RHAP#%}-$(printf '%s' "$RHDIR" | cksum | cut -d' ' -f1)"; RHMARK="$RHSTATE/tmux-cockpit/review/fixer-${RHAP#%}"
RHPAY=$(mktmpf); printf '{"session_id":"s","transcript_path":"/nonexistent","cwd":"%s","hook_event_name":"Stop","stop_hook_active":false}\n' "$RHDIR" > "$RHPAY"
RHPAY2=$(mktmpf); sed 's/"stop_hook_active":false/"stop_hook_active":true/' "$RHPAY" > "$RHPAY2"
rh_n() { "$TMUX_REAL" -L "$RH" capture-pane -p -J -t "$RHCTL" | grep -c 'HRC=[0-9]'; }
rh_start() { "$TMUX_REAL" -L "$RH" send-keys -t "$RHCTL" "TMUX_PANE=$RHAP XDG_STATE_HOME=$(printf '%q' "$RHSTATE") '$DIR/scripts/review-gate' < $(printf '%q' "$1") > $(printf '%q' "$RHSTATE/out") 2>/dev/null; echo HRC=\$?" Enter; }
rh_wait() { local i; for i in $(seq 1 100); do [ "$(rh_n)" -gt "$1" ] && return 0; sleep 0.1; done; return 1; }
rh_run() { local n0; n0=$(rh_n); rh_start "$1"; rh_wait "$n0"; }
rh_record() {                 # <fixer-pane-id> <deadline>  the record + marker as tmux-fixer -p writes them
    mkdir -p "$RHSESS/fixer"; printf 'fix the tests\n' > "$RHSESS/fixer/reason"; : > "$RHSESS/fixer/guard"
    printf '%s\n' "$2" > "$RHSESS/fixer/deadline"; printf '%s\n' "$1" > "$RHSESS/fixer/pane"; printf '%s\n' "$RHSESS" > "$RHMARK"
}
# 1. No fixer: as fast as before, no hold state.
rh_t0=$SECONDS; rh_run "$RHPAY"; [ $(( SECONDS - rh_t0 )) -le 3 ] && ok "no fixer: no added latency" || bad "no fixer: no added latency" "≤3s" "$(( SECONDS - rh_t0 ))s"
is "no fixer: no hold state" "" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_hold)"
# 2. A live fixer pane: hold, visible reason; a handover + the pane closing resumes it with that one line.
RHFX=$("$TMUX_REAL" -L "$RH" split-window -d -t t -P -F '#{pane_id}' 'sleep 300'); rh_record "$RHFX" $(( $(date +%s) + 300 ))
n0=$(rh_n); rh_start "$RHPAY"
for i in $(seq 1 40); do [ -n "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_hold)" ] && break; sleep 0.1; done
is "a held pane carries the reason" "fix the tests" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_hold)"
sleep 2; is "and the hook is still holding after 2 s" "$n0" "$(rh_n)"
printf 'HANDOVER: tests fixed; suite 118s -> 8s; re-read tests/\n' > "$RHSESS/handover"; "$TMUX_REAL" -L "$RH" kill-pane -t "$RHFX"
rh_wait "$n0" && ok "the hook returns within 4 s of the fixer finishing" || bad "the hook returns within 4 s of the fixer finishing" "HRC" "$("$TMUX_REAL" -L "$RH" capture-pane -p -t "$RHCTL" | tail -3)"
case "$(cat "$RHSTATE/out")" in *'"hookSpecificOutput"'*'"additionalContext":"HANDOVER: tests fixed; suite 118s -> 8s; re-read tests/\nContinue with your task."'*) ok "the handover is the additionalContext: one line, then the continue line" ;; *) bad "the handover is the additionalContext: one line, then the continue line" "…additionalContext with the HANDOVER line…" "$(cat "$RHSTATE/out")" ;; esac
is "the hold state is cleared" "" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_hold)"
is "the handover is consumed" "absent" "$([ -e "$RHSESS/handover" ] && echo present || echo absent)"
is "the record is cleared" "absent" "$([ -d "$RHSESS/fixer" ] && echo present || echo absent)"
is "and the gate's marker with it" "absent" "$([ -e "$RHMARK" ] && echo present || echo absent)"
rh_run "$RHPAY"; is "the next Stop hands over nothing (no replay)" "" "$(grep -o 'HANDOVER[^"]*' "$RHSTATE/out")"
# 3. The fixer pane dies with no handover (kill -9 of the process closes the pane): proceed at once, warned.
RHFX=$("$TMUX_REAL" -L "$RH" split-window -d -t t -P -F '#{pane_id}' 'sleep 300'); rh_record "$RHFX" $(( $(date +%s) + 300 )); "$TMUX_REAL" -L "$RH" kill-pane -t "$RHFX"
rh_t0=$SECONDS; rh_run "$RHPAY"; [ $(( SECONDS - rh_t0 )) -le 4 ] && ok "a dead fixer pane does not hold" || bad "a dead fixer pane does not hold" "≤4s" "$(( SECONDS - rh_t0 ))s"
case "$(cat "$RHSTATE/out")" in *'a side task (\"fix the tests\") was running and did not finish; earlier results may be stale.'*) ok "and the agent is warned" ;; *) bad "and the agent is warned" "the stale warning (JSON-escaped quotes)" "$(cat "$RHSTATE/out")" ;; esac
# 4. A stuck fixer (pane alive, deadline passed): proceed, warned.
RHFX=$("$TMUX_REAL" -L "$RH" split-window -d -t t -P -F '#{pane_id}' 'sleep 300'); rh_record "$RHFX" $(( $(date +%s) - 1 ))
rh_t0=$SECONDS; rh_run "$RHPAY"; [ $(( SECONDS - rh_t0 )) -le 4 ] && ok "an expired deadline does not hold" || bad "an expired deadline does not hold" "≤4s" "$(( SECONDS - rh_t0 ))s"
case "$(cat "$RHSTATE/out")" in *'did not finish'*) ok "and warns" ;; *) bad "and warns" "the stale warning" "$(cat "$RHSTATE/out")" ;; esac
"$TMUX_REAL" -L "$RH" kill-pane -t "$RHFX"
# 5. stop_hook_active: no hold even with a live fixer.
RHFX=$("$TMUX_REAL" -L "$RH" split-window -d -t t -P -F '#{pane_id}' 'sleep 300'); rh_record "$RHFX" $(( $(date +%s) + 300 ))
rh_t0=$SECONDS; rh_run "$RHPAY2"; [ $(( SECONDS - rh_t0 )) -le 4 ] && ok "stop_hook_active true never holds" || bad "stop_hook_active true never holds" "≤4s" "$(( SECONDS - rh_t0 ))s"
"$TMUX_REAL" -L "$RH" kill-pane -t "$RHFX"; rm -rf "$RHSESS/fixer" "$RHMARK"
# 6. Ordering: a handover AND a queued review land in one payload, handover first.
printf 'HANDOVER: fixed\n' > "$RHSESS/handover"; printf 'Code review from the human\n\n## f:1 (+)\n> a\nREVIEW-MARKER\n\n' > "$RHSESS/spool.md"
"$TMUX_REAL" -L "$RH" set-option -p -t "$RHAP" @cockpit_review_spool "$RHSESS/spool.md" >/dev/null 2>&1
rh_run "$RHPAY"
case "$(cat "$RHSTATE/out")" in *'HANDOVER: fixed'*'Continue with your task.'*'REVIEW-MARKER'*) ok "handover first, then the queued review, one payload" ;; *) bad "handover first, then the queued review, one payload" "HANDOVER … Continue … REVIEW-MARKER" "$(cat "$RHSTATE/out")" ;; esac
is "the spool was drained too" "" "$("$TMUX_REAL" -L "$RH" show-option -pqv -t "$RHAP" @cockpit_review_spool)"
unset -f rh_n rh_start rh_wait rh_run rh_record
"$TMUX_REAL" -L "$RH" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 2: Run to see it fail** — `make test-only PAT='review-gate: holds'`: the hold assertions FAIL.

- [ ] **Step 3: Implement the hold**

In `review-gate`'s Stop path, after `read_payload` and before the drain:

```bash
case "$payload" in *'"stop_hook_active":true'*|*'"stop_hook_active": true'*) active=true ;; *) active=false ;; esac

# --- 0. the hold: a side fix (tmux-fixer) is alive for this pane ----------------
# While its pane exists, no handover is written and the deadline has not passed,
# this hook WAITS -- one invocation, one poll, no block-and-rewake (the cap
# counts decisions, not seconds; the hook entry's timeout is 330 s, above the
# 300 s default deadline). Fail-open: a dead pane, a past deadline, a missing
# file -> proceed at once. Never on a stop_hook_active Stop.
ctx=""
sess=""; [ -f "$marker" ] && sess=$(cat -- "$marker" 2>/dev/null)
[ -n "$sess" ] || sess=$("$here/tmux-review" --sess "$pane" 2>/dev/null)     # a handover with no marker (a fixer that finished between two Stops)
if [ -n "$sess" ] && [ "$active" != true ] && fixer_alive "$sess"; then
    reason=$(cat -- "$sess/fixer/reason" 2>/dev/null); reason=${reason//[$'\001'-$'\037']/}
    tmux set-option -p -t "$pane" @cockpit_hold "$reason" 2>/dev/null
    tmux display-message "held: $reason (side fix in $(cat -- "$sess/fixer/pane" 2>/dev/null)) — $pane resumes when it finishes" 2>/dev/null
    while fixer_alive "$sess"; do sleep 1; done
    tmux set-option -pu -t "$pane" @cockpit_hold 2>/dev/null
    [ -f "$sess/handover" ] || ctx="a side task (\"$reason\") was running and did not finish; earlier results may be stale."
fi
if [ -n "$sess" ] && [ -f "$sess/handover" ] && [ ! -L "$sess/handover" ]; then
    ctx=$(head -c 400 -- "$sess/handover"); ctx=${ctx//[$'\001'-$'\037']/}
    rm -f -- "$sess/handover"
fi
if [ -n "$ctx" ]; then
    ctx="$ctx"$'\n'"Continue with your task."
    rm -rf -- "$sess/fixer" "$marker"            # delivered: the record and the gate's marker end here
fi
```

Then the drain reads the spool into `review` as phase 3 wrote it, and the emit joins `"$ctx${ctx:+$'\n\n'}$review"` through `json_escape` into one `additionalContext`, exit 0 when non-empty; `agent-state running` stays on the drain path.

`examples/claude-hooks.json`: the review-gate Stop entry → `"timeout": 330, "statusMessage": "cockpit: review-gate (holding for a side fix if one is running)"`. `install.sh`: in the jq merge, before appending, drop any existing entry whose `.command` ends in `/scripts/review-gate` or `/scripts/review-gate --pretool` — a `del(.hooks[$k][] .hooks[] | select(.command | test("/scripts/review-gate")))`-shaped clause on `$theirs` first, so an upgrade *replaces* cockpit's own entries and never duplicates them; one assertion in `== install.sh --hooks merges additively ==`: merging over a settings that holds the phase-3 review-gate entry (timeout 5) yields exactly one review-gate Stop entry with timeout 330. `tmux-review --info`: `hold      <@cockpit_hold or —>` and, when `fixer/` exists, `side fix  <reason>  pane <id> (<alive|gone>)  deadline in <n>s  guard /<ere>/`.

- [ ] **Step 4: Run; commit** — `make test-only PAT='review-gate|merges additively|tmux-review --info'` all PASS (phase 3's silent-stdin assertion included); `make check` once (foreground, tool timeout 600000).

```bash
git add scripts/review-gate scripts/tmux-review examples/claude-hooks.json install.sh README.md tests/run.sh
git commit -m "feat: review-gate holds a Stop while a side fix's pane lives — one poll, a deadline, open on a handover or a dead pane — and hands over one line ahead of any queued review"
```

---

### Task 5: Docs, rig, leak sweep, manual smoke, gate and merge

- [ ] **Step 1: Freeze rig** — `bash tests/freeze-rig/run.sh gate` twice: unchanged (one per-pane option, read by no format).
- [ ] **Step 2: Leak sweep** — sockets unchanged; `find "${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/review" -name fixer -o -name 'fixer-*'` prints nothing on the real state dir; `pgrep -fa fake-claude` empty.
- [ ] **Step 3: Manual smoke (the Phase-0 gaps) — record each as a fact in the README's "Side fixes" section:** (a) `prefix f` on a live agent mid-task with the job "sleep 90 then say done" and guard `make test`; watch the agent finish its turn, the pane show `held: …`, and the handover arrive; (b) while held, press **Esc in the main pane** — does the hook keep running, does the handover land at the next turn?; (c) with the guard set, ask the agent to run `make test` — confirm the deny ends the turn, a `Stop` follows (does it? — the hold then applies), and the agent resumes with the handover rather than routing around; (d) a real job on a scratch repo ("the suite takes 2 min; make it fast").
- [ ] **Step 4: Whole-branch review** — a fresh `code-reviewer` (opus): `git diff master...feat/side-fixer`, this plan, brief v2's DON'Ts and Acceptance as the checklist; the reviewer lists every hook field used and its `[E]` line.
- [ ] **Step 5: Gate** — `make -j2 check-par` twice (phase 3b) and `make check` once unpiped; `make gate`.
- [ ] **Step 6: Merge** — `git merge --no-ff feat/side-fixer -m "Merge branch 'feat/side-fixer' — side-fixer panes: prefix f, a fenced claude -p beside a busy agent, the guarded command denied and the Stop held until one handover line"`; `install.sh --hooks` (replaces cockpit's review-gate entries, appends the PreToolUse group); update the memory file and the ultragoal ledger (`G004-side-fixer`).

---

## Rig — the test list, mapped to brief v2's acceptance

| acceptance | test | task |
|---|---|---|
| no fixer → Stop latency unchanged | `no fixer: no added latency`; phase 3's silent-stdin assertion | 4 |
| fixer spawned mid-turn → finishes uninterrupted, pauses at Stop with a visible reason | `a held pane carries the reason`, `still holding after 2 s`, the `display-message` | 4 |
| fixer completes → resumes same turn, context grew by one line | `returns within 4 s`, `additionalContext` = one line + `Continue with your task.`, `handover consumed`, `no replay`, record + marker cleared | 4 |
| fixer alive, main agent attempts the suite → denied with a reason it defers to | `a guarded command is denied…`, `unguarded passes`, `empty guard denies nothing`; the deny ends the turn (v2.1.210+ default) — manual smoke (c) confirms the agent resumes with the handover | 3, 5 |
| fixer `kill -9` → pane dies, gate opens immediately | `a dead fixer denies nothing` + marker dropped (gate); `a dead fixer pane does not hold` + warned (hold) | 3, 4 |
| fixer stuck but alive → resumes at the deadline with the stale warning | `an expired deadline does not hold` + `and warns` | 4 |
| two spawns on one pane → second refuses | `a second spawn while the fixer lives refuses`, `only one fixer pane` | 2 |
| long fixer (> deadline) → proceeds, warned, not wedged | same as "stuck" | 4 |
| liveness caveat decided (failed fixer keeps its pane) | `a written handover opens the gate even with the pane alive` (gate); the wrapper writes the fallback first (Task 1) | 3, 1 |
| the fence | `both dirty paths denied`, `a clean path is not fenced`, `mutating git denied` | 2 |
| the wrapper's one line | `verbatim`, `fallback names the job`, `capped at 300`, `failed claude leaves a handover` | 1 |
| stale record freed | `a finished fixer's record does not block a new spawn`, `old handover not carried over` | 2 |
| ordering with a queued review | `handover first, then the queued review, one payload` | 4 |
| `stop_hook_active` guard | `stop_hook_active true never holds` | 4 |
| the no-fixer gate path is one stat | `no marker: one stat, under 200 ms with no tools on PATH` | 3 |
| every hook field traces to an `[E]` | reviewer checklist | 5 |
| freeze rig / leak sweep / Esc during a hold | Task 5 steps 1–3 | 5 |

## Self-review

**Spec coverage (brief v2):** Goal 1–4 ✔ (Tasks 2, 3, 4); Non-goals ✔; Phase 0 A–D ✔ done (hook-facts §A–E + §D, handoff-check); Phase 1 liveness ✔ (no lock; `fixer_pane` = `fixer/pane`; the caveat decided: done = handover ∨ dead ∨ deadline); Phase 2 spawn ✔ (record first, `split-window -d`, fence, refuse duplicate, handover contract); Phase 3a gate ✔ Task 3 (guard from the human; deny ends the turn; wording tested for the job name + "continue"); Phase 3b hold ✔ Task 4 (1–6, ordering); DON'Ts ✔; Acceptance ✔ the rig table (the kill -9 row now reads "pane dies → gate opens immediately", as v2 states).

**Placeholder scan:** none; `read_payload`/`json_escape` are named phase-3 functions the implementer hoists (their code exists in `review-gate` after phase 3's Task 5).

**Type consistency:** `tmux-review --sess <pane>` (Task 1) used by Tasks 2 and 4; record files `pane reason deadline fence guard` written in Task 2, read by `fixer_alive` (Tasks 2–4), the gate (guard, reason, pane) and the hold (reason, pane); marker `…/review/fixer-<pane#>` written in Task 2, stat'd in Task 3, removed in Tasks 3–4; `handover` written by Task 1, read by `fixer_alive`, consumed by Task 4; `FAKE_CLAUDE_*` identical across Tasks 1–2.
