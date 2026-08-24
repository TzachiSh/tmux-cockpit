# RimZ-Inspired Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add five RimZ-inspired capabilities to tmux-cockpit — jump-to-blocked-pane, a fleet counter token, the ask text, context health, and an additive hook installer — via five parallel team lanes in isolated worktrees.

**Architecture:** Every feature keeps cockpit's invariant: hooks and keypresses WRITE per-pane tmux options; status redraws only READ them through tmux format strings (zero shell per redraw). New scripts are one-shot bash; the only render-side changes are format strings. No daemon, no state files, state dies with the pane.

**Tech Stack:** bash 4+, tmux ≥ 3.1 formats, fzf (existing), jq (only for the optional `--hooks` installer path), shellcheck 0.10.0, the repo's own tests/run.sh rig (private tmux sockets).

**Spec:** `docs/superpowers/plans/2026-08-24-rimz-improvements-spec.md`

## Global Constraints

- `make check` (shellcheck clean + tests/run.sh all-pass) is the merge gate for every lane.
- No `#(command)` substitutions in any tmux format cockpit sets or documents (redraw runs no shell).
- Pane state options are pane-scoped (`set-option -p`) so they die with the pane; callers validate pane ids as `%N` before writing (agent-state precedent, `scripts/agent-state:13-17`).
- Hook scripts must never block the agent that fired them: bounded reads with a single whole-read deadline (`scripts/agent-state:36-45` is the pattern to copy).
- Comment style: explain constraints and failure modes ("why"), never restate the line.
- Commits: lowercase `feat:`/`fix:`/`perf:` one-liners matching `git log` style; end with the Co-Authored-By/Claude-Session trailer from the session rules.
- **Never edit the main checkout at `~/.tmux/plugins/tmux-cockpit` while lanes run** — concurrent edits corrupt an executing tests/run.sh (see project memory `concurrent-sessions-shared-worktree`). All lane work happens in worktrees.

---

## Team & worktree protocol

Five lanes run in parallel, one worktree each. Per lane the crew is:

| Role | Agent | Job |
|------|-------|-----|
| Implementer | `oh-my-claudecode:executor` (F2 uses `model=opus` — nested-format work is the trickiest) | Executes the lane's tasks below, TDD, commits in the lane branch |
| Reviewer | `oh-my-claudecode:code-reviewer` | Separate pass after the lane's last commit: correctness, quoting, `%N` guards, bounded reads, comment style. Implementer never self-approves |
| Verifier | `oh-my-claudecode:verifier` | Re-runs `make check` + the lane's perf/memory gates from a clean shell; collects evidence before the lane is declared done |

Integration (Task 7) is one `oh-my-claudecode:git-master`-style lane run sequentially by the session lead after all five lanes verify green.

**Worktree setup (each implementer's first act):**

```bash
git -C ~/.tmux/plugins/tmux-cockpit worktree add ~/.cache/cockpit-wt/<lane> -b feat/<lane> master
cd ~/.cache/cockpit-wt/<lane>
make check   # must be green BEFORE any edit; if not, stop and report
```

Lanes: `f1-jump`, `f2-fleet`, `f3-ask`, `f4-context`, `f5-hooks`. The suite is parallel-safe across worktrees (every block runs on private sockets named with `$$`), so all five lanes may run `make check` simultaneously.

## Shared performance & memory protocol

Captured once in Task 0, then enforced per lane:

- **Hook cost gate** (lanes F3, F4): 200 invocations of the touched hook script must stay within +10% of the Task 0 baseline. Method below in Task 0.
- **Redraw cost gate** (lanes F2, F3): expanding the full status-right format 200 times must stay within +10% of baseline (formats are the redraw; there is no shell to measure).
- **One-shot latency** (lane F1): `tmux-jump` completes in < 200ms against a 40-pane server.
- **Leak checks** (every lane, after its suite run):
  ```bash
  ls "/tmp/tmux-$(id -u)/" 2>/dev/null | grep -c '^cockpit-'   # want: 0 (no leaked suite servers)
  pgrep -fc 'cockpit-(suite|jj|fc|ak|cx|hk)-' || true          # want: 0
  git status --short                                            # want: only the lane's intended files
  ```
- **Option-scope memory rule** (reviewer checklist): every new option write is `-p` pane-scoped so tmux frees it with the pane; the ONLY global reads are of `@cockpit_*` config. No file is ever written outside `$XDG_RUNTIME_DIR`/mktemp.

---

### Task 0: Baseline capture (session lead, main checkout, BEFORE branching)

**Files:**
- Create: `docs/superpowers/plans/perf-baseline.txt`

- [ ] **Step 1: Verify the tree is quiet and green**

```bash
cd ~/.tmux/plugins/tmux-cockpit && git status --short   # want: empty (or known-pending lint fixes)
make check                                              # want: green
```

- [ ] **Step 2: Capture hook-cost and redraw baselines**

```bash
S="cockpit-base-$$"; tmux -L "$S" -f /dev/null new-session -d -s t 'sleep 300'
P=$(tmux -L "$S" list-panes -t t -F '#{pane_id}' | head -1)
{
  echo "date: $(date -Iseconds)  host: $(hostname)"
  echo "== agent-state x200 =="
  ( TMUX= ; time ( for i in $(seq 200); do
      tmux -L "$S" set-option -p -t "$P" @panestate running >/dev/null 2>&1
    done ) ) 2>&1
  echo "== status-right expansion x200 =="
  tmux -L "$S" set -g status-right '#{?#{==:#{@panestate},needs-input},X,Y}'
  time ( for i in $(seq 200); do
    tmux -L "$S" display-message -p -t "$P" "$(tmux -L "$S" show-option -gv status-right)" >/dev/null
  done ) 2>&1
} > docs/superpowers/plans/perf-baseline.txt
tmux -L "$S" kill-server
```

- [ ] **Step 3: Commit the baseline**

```bash
git add docs/superpowers/plans/
git commit -m "chore: rimz-improvements spec, plan, and perf baseline"
```

---

### Task 1 (lane f1-jump): `scripts/tmux-jump` — jump to the blocked pane

**Files:**
- Create: `scripts/tmux-jump`
- Modify: `cockpit.tmux` (one binding near the existing `bind-key S` at line ~194), `Makefile` is untouched (SOURCES uses a wildcard), `README.md` (Keys section), `install.sh:18` (`COMMANDS=` stays as-is — jump is prefix-only, not a typed command)
- Test: `tests/run.sh` (new block appended before the final summary section)

**Interfaces:**
- Consumes: `@panestate` pane option (values `needs-input|error|running|done`), `@cockpit_jump_key` global option (unset → `J`, literal `off` → no binding).
- Produces: `scripts/tmux-jump` (no args; reads `$TMUX_PANE` for cycle position; exit 0 always).

- [ ] **Step 1: Read the ground truth**

Read `cockpit.tmux:1-60` to learn the plugin-dir variable name used by `$Q_BRANCH` (grep `Q_BRANCH=`; call it `$DIR` below — substitute the real name) and `tests/run.sh:1-140` for the `is/ok/bad`, `need_tmux`, `SOCKS`, `TMUX_REAL` helpers.

- [ ] **Step 2: Write the failing test block** (append to tests/run.sh before the summary)

```bash
echo "== prefix J jumps to the pane that needs you =="
if need_tmux "tmux-jump"; then :; else
JJ="cockpit-jj-$$"; SOCKS="$SOCKS $JJ"
"$TMUX_REAL" -L "$JJ" -f /dev/null new-session -d -s t -x 80 -y 24 >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" new-window  -t t 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" new-window  -t t 'exec -a claude sleep 300' >/dev/null 2>&1
JP2=$("$TMUX_REAL" -L "$JJ" list-panes -t t:1 -F '#{pane_id}')
JP3=$("$TMUX_REAL" -L "$JJ" list-panes -t t:2 -F '#{pane_id}')
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP3" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" select-window -t t:0 >/dev/null 2>&1
# Drive from inside the session so $TMUX/$TMUX_PANE are real, like a keypress.
JP1=$("$TMUX_REAL" -L "$JJ" list-panes -t t:0 -F '#{pane_id}')
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'" Enter; sleep 1
is "jump lands on the blocked window" "2" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t '#{window_index}')"
# A second blocked pane: pressing again from the first cycles to the next.
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP3" "'$DIR/scripts/tmux-jump'" Enter; sleep 1
is "a second press cycles to the next blocked pane" "1" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t '#{window_index}')"
# Nothing blocked: the script must not move you, and must not error.
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP2" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" set-option -p -t "$JP3" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" select-window -t t:0 >/dev/null 2>&1
"$TMUX_REAL" -L "$JJ" send-keys -t "$JP1" "'$DIR/scripts/tmux-jump'; echo JRC=\$?" Enter; sleep 1
case "$("$TMUX_REAL" -L "$JJ" capture-pane -p -t "$JP1")" in
    *JRC=0*) ok "no blocked pane exits 0 and stays put" ;;
    *) bad "no blocked pane exits 0 and stays put" "JRC=0 in the pane" \
         "$("$TMUX_REAL" -L "$JJ" capture-pane -p -t "$JP1" | tail -3)" ;;
esac
is "and the window did not change" "0" \
   "$("$TMUX_REAL" -L "$JJ" display-message -p -t t '#{window_index}')"
"$TMUX_REAL" -L "$JJ" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 3: Run it, confirm the new tests FAIL** (`tests/run.sh 2>&1 | grep -A3 'jump'` — expect FAILs, everything else still green)

- [ ] **Step 4: Implement `scripts/tmux-jump`**

```bash
#!/usr/bin/env bash
# Jump to the next pane whose agent is waiting on you, across every session.
# Bound to prefix J by cockpit.tmux (@cockpit_jump_key overrides; "off" unbinds).
# One-shot: runs only on the keypress, so a status redraw still costs no shell.
set -u

cur="${TMUX_PANE:-}"
blocked=()
# Sorted so the cycle order is stable between presses; @panestate only ever
# exists on agent panes, so no agents-glob filter is needed here.
while read -r id state; do
    [ "$state" = needs-input ] && blocked+=("$id")
done < <(tmux list-panes -a -F '#{pane_id} #{@panestate}' 2>/dev/null | sort -t% -k2 -n)

if [ ${#blocked[@]} -eq 0 ]; then
    tmux display-message "no agent needs you" 2>/dev/null
    exit 0
fi

# Cycle: from a blocked pane, go to the NEXT one (wrapping); from anywhere
# else, go to the first. Repeated presses therefore visit every blocked pane.
next=${blocked[0]}
for i in "${!blocked[@]}"; do
    if [ "${blocked[$i]}" = "$cur" ]; then
        next=${blocked[$(( (i + 1) % ${#blocked[@]} ))]}
        break
    fi
done

# switch-client needs a client and fails detached (tests, control mode); the
# window/pane selection below still retargets the session, so it is not fatal.
tmux switch-client -t "$next" 2>/dev/null || true
tmux select-window -t "$next" 2>/dev/null
tmux select-pane   -t "$next" 2>/dev/null
exit 0
```

`chmod +x scripts/tmux-jump`. Then the binding in `cockpit.tmux`, next to `bind-key S` (~line 194), using the plugin-dir variable found in Step 1:

```bash
# Jump to the pane that needs you; press again to cycle through all of them.
JK=$(tmux show-option -gqv @cockpit_jump_key)
case "$JK" in
    off) ;;                          # explicitly disabled
    "")  tmux bind-key J run-shell "'$DIR/scripts/tmux-jump'" ;;
    *)   tmux bind-key "$JK" run-shell "'$DIR/scripts/tmux-jump'" ;;
esac
```

- [ ] **Step 5: Run the suite to green** (`tests/run.sh` — all pass, including the 3 new ones)
- [ ] **Step 6: Perf gate** — 40-pane latency:

```bash
S="cockpit-jl-$$"; tmux -L "$S" -f /dev/null new-session -d -s t
for i in $(seq 39); do tmux -L "$S" split-window -t t -d 'sleep 300' \
  || tmux -L "$S" new-window -t t -d 'sleep 300'; done
P=$(tmux -L "$S" list-panes -s -t t -F '#{pane_id}' | head -1)
time tmux -L "$S" send-keys -t "$P" "" \; run-shell -t "$P" "$PWD/scripts/tmux-jump"   # want: real < 0.2s
tmux -L "$S" kill-server
```

- [ ] **Step 7: `make lint` clean, leak checks from the shared protocol, README Keys section gains one line** (`prefix J  jump to the pane that needs you (again: next one) — @cockpit_jump_key rebinds, off disables`)
- [ ] **Step 8: Commit** — `git commit -m "feat: prefix J jumps to the pane that needs you, cycling the fleet"`

---

### Task 2 (lane f2-fleet): `#{agent_fleet}` status token

**Files:**
- Modify: `cockpit.tmux` (`interpolate()` at :33-48 and the `update_option` matcher at :57), `examples/tmux.conf` (status-right example), `README.md` (tokens table near line 102)
- Test: `tests/run.sh` (new block)

**Interfaces:**
- Consumes: `@panestate`, `@cockpit_agents_glob` (same guard expression the banner uses — copy it verbatim from `examples/tmux.conf:197`).
- Produces: the literal token `#{agent_fleet}` usable in any status option set BEFORE the run-shell line; expands to `▲N ` (red) and/or `✕N ` (orange) or empty.

- [ ] **Step 1: Probe the counting trick in isolation** (tmux nesting is the risk; prove it before wiring):

```bash
S="cockpit-nt-$$"; tmux -L "$S" -f /dev/null new-session -d -s a 'sleep 300'
tmux -L "$S" new-window -t a 'sleep 300'; tmux -L "$S" new-session -d -s b 'sleep 300'
for p in $(tmux -L "$S" list-panes -a -F '#{pane_id}' | head -2); do
  tmux -L "$S" set-option -p -t "$p" @panestate needs-input; done
tmux -L "$S" display-message -p \
 '#{n:#{S:#{W:#{P:#{?#{==:#{@panestate},needs-input},x,}}}}}'   # want: 2
tmux -L "$S" kill-server
```

If `n:` refuses the nested expansion on tmux 3.7, fall back to `#{e|+:...}` addition of per-session counts — record which form worked in the commit message.

- [ ] **Step 2: Write the failing test block**

```bash
echo "== the fleet counter reads every session =="
if need_tmux "agent_fleet"; then :; else
FC="cockpit-fc-$$"; SOCKS="$SOCKS $FC"
"$TMUX_REAL" -L "$FC" -f /dev/null new-session -d -s a -x 80 -y 24 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" new-session -d -s b 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" new-window  -t b 'exec -a claude sleep 300' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" set -g @cockpit_agents_glob '|claude|' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" set -g status-right '[#{agent_fleet}]' >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" run-shell "$DIR/cockpit.tmux" >/dev/null 2>&1
FCP1=$("$TMUX_REAL" -L "$FC" list-panes -t a:0 -F '#{pane_id}')
FCP2=$("$TMUX_REAL" -L "$FC" list-panes -t b:0 -F '#{pane_id}')
FCP3=$("$TMUX_REAL" -L "$FC" list-panes -t b:1 -F '#{pane_id}')
"$TMUX_REAL" -L "$FC" set-option -p -t "$FCP1" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" set-option -p -t "$FCP2" @panestate needs-input >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" set-option -p -t "$FCP3" @panestate error       >/dev/null 2>&1
fleet() { "$TMUX_REAL" -L "$FC" display-message -p -t "$FCP1" \
            "$("$TMUX_REAL" -L "$FC" show-option -gv status-right)" | plain; }
is "two blocked and one errored, across sessions" "[▲2 ✕1 ]" "$(fleet)"
"$TMUX_REAL" -L "$FC" set-option -p -t "$FCP1" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" set-option -p -t "$FCP2" @panestate running >/dev/null 2>&1
"$TMUX_REAL" -L "$FC" set-option -p -t "$FCP3" @panestate running >/dev/null 2>&1
is "a quiet fleet renders nothing at all" "[]" "$(fleet)"
"$TMUX_REAL" -L "$FC" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 3: Run to confirm FAIL** (token survives un-interpolated → `[#{agent_fleet}]`)
- [ ] **Step 4: Implement in `cockpit.tmux`** — next to the `r_path`/`r_brch` definitions feeding `interpolate()` (:33-48):

```bash
# One x per blocked/errored agent pane, in ANY session, counted by #{n:}. The
# whole thing is a format string: the counter costs a redraw no shell, ever.
AG='#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}}'
CNT() { printf '#{n:#{S:#{W:#{P:#{?#{&&:%s,#{==:#{@panestate},%s}},x,}}}}}' "$AG" "$1"; }
r_fleet="#{?#{e|>:$(CNT needs-input),0},#[fg=red#,bold]▲$(CNT needs-input)#[default] ,}"
r_fleet="$r_fleet#{?#{e|>:$(CNT error),0},#[fg=colour208]✕$(CNT error)#[default] ,}"
```

and inside `interpolate()` beside the two existing lines (:46-47):

```bash
    s="${s//'#{agent_fleet}'/$r_fleet}"
```

(`update_option`'s `*'#{agent_'*` matcher at :57 already catches the new token.)

- [ ] **Step 5: Run the suite to green**
- [ ] **Step 6: Redraw perf gate** — repeat the Task 0 status-right expansion timing with `[#{agent_fleet}]` interpolated on a 40-pane server; must be within +10% of the Task 0 number (record both in the lane report).
- [ ] **Step 7: Docs** — README tokens list gains `#{agent_fleet}`; `examples/tmux.conf` status-right example includes it with a comment ("blocked/errored agents in EVERY session — the two numbers worth interrupting your focus for").
- [ ] **Step 8: `make check` green; commit** — `git commit -m "feat: #{agent_fleet} counts who needs you across every session, shell-free"`

---

### Task 3 (lane f3-ask): show the ask itself

**Files:**
- Modify: `scripts/agent-state` (notify arm :23-60 and the state-write tail), `examples/tmux.conf:197` (pane-border-format needs-input arm), `README.md`
- Test: `tests/run.sh` (extend the existing hook-driven block that already fakes notify payloads — grep `needs your permission` to find it)

**Interfaces:**
- Consumes: Notification hook JSON on stdin (existing bounded read), `$PANE` test helpers `st`/`sv`.
- Produces: `@panestate_msg` pane option — first `message` value, ≤120 chars, control chars stripped; cleared whenever `running` or `off` is written.

- [ ] **Step 1: Write the failing tests** (inside the existing notify test block, after the current needs-input assertions):

```bash
printf '{"message":"Claude needs your permission to use Bash"}' \
    | "$DIR/scripts/agent-state" notify "$PANE"
is "the ask text is stored beside the alert" \
   "Claude needs your permission to use Bash" \
   "$(tmux show-option -pqv -t "$PANE" @panestate_msg)"
st running "$PANE"
is "resuming work clears the stale ask" "" \
   "$(tmux show-option -pqv -t "$PANE" @panestate_msg)"
# A payload with no message field must not leave an empty-string option behind.
st off "$PANE"; printf '{"foo":1,"message":""}' | "$DIR/scripts/agent-state" notify "$PANE"
is "an empty message stores nothing" "" \
   "$(tmux show-option -pqv -t "$PANE" @panestate_msg)"
```

- [ ] **Step 2: Run to confirm FAIL**
- [ ] **Step 3: Implement in `scripts/agent-state`** — in the notify arm, right after `*'needs your permission'*) state=needs-input ;;` resolves, capture the message; where the script finally writes `@panestate`, add the companion write:

```bash
# The ask itself, for the border: knowing WHAT is asked separates a one-key
# approval from a decision, without switching panes. sed, not jq: the payload
# is small, the schema is one string field, and jq is not a dependency here.
msg=$(printf '%s' "$payload" \
      | sed -n 's/.*"message"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
      | head -1 | tr -d '\000-\037' | cut -c1-120)
```

and beside the state write:

```bash
case "$state" in
    needs-input) [ -n "${msg:-}" ] && tmux set-option -p -t "$pane" @panestate_msg "$msg" ;;
    running|off) tmux set-option -pu -t "$pane" @panestate_msg 2>/dev/null ;;
esac
```

(Deliberately not cleared on `done`: the ranking already refuses to let `done` overwrite `needs-input`, and a done pane has no border alert reading the message.)

- [ ] **Step 4: Run to green**
- [ ] **Step 5: Render** — in `examples/tmux.conf:197`, the needs-input border arm gains ` #{=/48/…:#{@panestate_msg}}` after the `─ NEEDS YOU ` text (both the aggregate-banner arm and the own-pane arm); README's "Where it shows up" section documents it.
- [ ] **Step 6: Hook-cost perf gate** — time 200 notify invocations piping the permission payload; within +10% of 200 plain `agent-state running` calls from Task 0 (one extra `tmux set` and one `sed` are the only additions; the bounded-read deadline dominates both).
- [ ] **Step 7: `make check` green; leak checks; commit** — `git commit -m "feat: the border shows what the agent is asking, not just that it asks"`

---

### Task 4 (lane f4-context): context health via the statusline

**Files:**
- Create: `scripts/agent-context`, `tests/fixtures/statusline.json`
- Modify: `examples/claude-hooks.json` (commented statusLine stanza), `examples/tmux.conf` (tab-suffix example), `README.md`
- Test: `tests/run.sh` (new block)

**Interfaces:**
- Consumes: Claude Code statusLine JSON on stdin; `$TMUX_PANE`.
- Produces: `@panestate_ctx` pane option holding an integer percent (0-100), plus a one-line stdout statusline (`<model> · ctx NN%`); silent no-op on unparseable input.

- [ ] **Step 1: Capture the real schema** (this feature is fixture-driven; do this FIRST):

```bash
mkdir -p /tmp/slcap && cd /tmp/slcap
cat > settings.json <<'EOF'
{ "statusLine": { "type": "command", "command": "tee -a /tmp/slcap/statusline.jsonl >/dev/null" } }
EOF
claude --settings /tmp/slcap/settings.json   # send one trivial prompt, /exit
head -1 /tmp/slcap/statusline.jsonl | jq keys
```

Copy one captured line to `tests/fixtures/statusline.json`. If the context field names differ from the jq chain in Step 4, extend that chain with the REAL key (the fixture keeps the test honest either way). If no context-usage field exists at all in the capture, stop the lane and report — the feature is not implementable against this Claude Code version, and the plan's other lanes are unaffected.

- [ ] **Step 2: Write the failing test block**

```bash
echo "== the statusline shim records context health =="
if need_tmux "agent-context"; then :; else
CX="cockpit-cx-$$"; SOCKS="$SOCKS $CX"
"$TMUX_REAL" -L "$CX" -f /dev/null new-session -d -s t 'sleep 300' >/dev/null 2>&1
CXP=$("$TMUX_REAL" -L "$CX" list-panes -t t -F '#{pane_id}')
TMUX_PANE="$CXP" "$DIR/scripts/agent-context" < "$DIR/tests/fixtures/statusline.json" >/dev/null
ctx=$("$TMUX_REAL" -L "$CX" show-option -pqv -t "$CXP" @panestate_ctx)
case "$ctx" in
    [0-9]|[0-9][0-9]|100) ok "a real payload yields a percent" ;;
    *) bad "a real payload yields a percent" "an integer 0-100" "$ctx" ;;
esac
# Garbage must write nothing and still exit 0 -- a statusline that errors
# would spam the agent's own UI.
"$TMUX_REAL" -L "$CX" set-option -pu -t "$CXP" @panestate_ctx >/dev/null 2>&1
printf 'not json at all' | TMUX_PANE="$CXP" "$DIR/scripts/agent-context" >/dev/null; cxrc=$?
is "garbage exits 0" "0" "$cxrc"
is "and stores nothing" "" "$("$TMUX_REAL" -L "$CX" show-option -pqv -t "$CXP" @panestate_ctx)"
# A writer that stalls must not wedge the statusline: whole-read deadline.
start=$SECONDS; { printf '{'; sleep 5; } | TMUX_PANE="$CXP" "$DIR/scripts/agent-context" >/dev/null
took=$(( SECONDS - start ))
if [ "$took" -le 3 ]; then ok "a stalling writer is abandoned in time"
else bad "a stalling writer is abandoned in time" "<=3s" "${took}s"; fi
"$TMUX_REAL" -L "$CX" kill-server >/dev/null 2>&1
fi
```

- [ ] **Step 3: Run to confirm FAIL**
- [ ] **Step 4: Implement `scripts/agent-context`**

```bash
#!/usr/bin/env bash
# Claude Code statusLine shim: records the pane's context fill so the tmux
# side can warn before compaction, then prints a minimal statusline.
# Contract: NEVER block or error at the agent -- bad input is a silent no-op.
set -u
pane="${TMUX_PANE:-}"
case "$pane" in %[0-9]*) ;; *) exit 0 ;; esac   # same guard as agent-state

# Bounded whole-read, the agent-state pattern: a hook must never hang its agent.
payload="" chunk="" deadline=$(( SECONDS + 2 ))
while left=$(( deadline - SECONDS )); [ "$left" -gt 0 ]; do
    IFS= read -r -t "$left" chunk || { payload="$payload$chunk"; break; }
    payload="$payload$chunk"; chunk=""
done

command -v jq >/dev/null 2>&1 || exit 0
# Field names verified against tests/fixtures/statusline.json (captured from a
# real run); the fallback chain absorbs schema drift between CC versions.
pct=$(jq -r '
    (.context_usage.percent_used
     // .context.used_percent
     // (if (.context_usage.input_tokens? and .context_usage.max_tokens?)
         then (.context_usage.input_tokens / .context_usage.max_tokens * 100)
         else empty end)
     // empty) | floor' 2>/dev/null <<<"$payload") || exit 0
model=$(jq -r '.model.display_name // empty' 2>/dev/null <<<"$payload")

case "$pct" in
    ''|*[!0-9]*) exit 0 ;;
    *) tmux set-option -p -t "$pane" @panestate_ctx "$pct" 2>/dev/null ;;
esac
printf '%s · ctx %s%%\n' "${model:-claude}" "$pct"
exit 0
```

`chmod +x scripts/agent-context`. Adjust the jq chain to the captured schema from Step 1 if needed — the fixture test is the arbiter.

- [ ] **Step 5: Run to green**
- [ ] **Step 6: Wire the examples** — `examples/claude-hooks.json` gains a commented `statusLine` stanza (`"_statusline_comment"` explaining it is optional and replaces any existing statusline); `examples/tmux.conf` window text gains `#{?#{e|>=:#{@panestate_ctx},80},#[fg=colour208] ◔#{@panestate_ctx}%,}`; README documents the option.
- [ ] **Step 7: Perf gate** — 200 fixture invocations vs the Task 0 agent-state baseline +10% (jq is the cost; if it blows the budget, note it in the report — the shim runs once per TURN, not per redraw, so the gate may be relaxed to 5× with reviewer sign-off, documented).
- [ ] **Step 8: `make check` green; leak checks; commit** — `git commit -m "feat: optional statusline shim writes context fill the tabs can warn on"`

---

### Task 5 (lane f5-hooks): `install.sh --hooks` with dry-run diff

**Files:**
- Modify: `install.sh` (argument parsing at :20-26 and a new merge section after the symlink loop), `README.md` (Install section), `examples/claude-hooks.json` (nothing structural — it becomes the merge SOURCE)
- Test: `tests/run.sh` (new block; no tmux needed — pure file work)

**Interfaces:**
- Consumes: `examples/claude-hooks.json` (`.hooks` object), `$HOME/.claude/settings.json`.
- Produces: `install.sh --hooks [--dry-run]`; exit 0 on success/no-op, 1 on failure, 2 on usage error (existing convention at :26).

- [ ] **Step 1: Write the failing test block**

```bash
echo "== install.sh --hooks merges additively =="
HK=$(mktmpd); mkdir -p "$HK/.claude"
cat > "$HK/.claude/settings.json" <<'EOF'
{ "model": "opus", "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "my-own-tool" } ] } ] } }
EOF
if command -v jq >/dev/null 2>&1; then
HOME="$HK" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "the installer exits clean" "0" "$?"
case "$(cat "$HK/.claude/settings.json")" in
    *my-own-tool*) ok "a foreign hook survives the merge" ;;
    *) bad "a foreign hook survives the merge" "my-own-tool still present" "$(head -c200 "$HK/.claude/settings.json")" ;;
esac
case "$(cat "$HK/.claude/settings.json")" in
    *tmux-cockpit*agent-state*) ok "cockpit's hooks arrived" ;;
    *) bad "cockpit's hooks arrived" "an agent-state command" "$(head -c200 "$HK/.claude/settings.json")" ;;
esac
is "the user's other settings are untouched" "opus" \
   "$(jq -r .model "$HK/.claude/settings.json")"
[ -f "$HK/.claude/settings.json.bak" ] && ok "a backup was written" \
    || bad "a backup was written" "settings.json.bak" "missing"
before=$(cat "$HK/.claude/settings.json")
HOME="$HK" "$DIR/install.sh" --hooks >/dev/null 2>&1
is "a second run changes nothing (idempotent)" "$before" "$(cat "$HK/.claude/settings.json")"
# dry-run: shows the diff, writes nothing
HK2=$(mktmpd); mkdir -p "$HK2/.claude"; printf '{}' > "$HK2/.claude/settings.json"
out=$(HOME="$HK2" "$DIR/install.sh" --hooks --dry-run 2>&1)
case "$out" in *agent-state*) ok "dry-run previews the change" ;;
    *) bad "dry-run previews the change" "a diff naming agent-state" "$out" ;; esac
is "dry-run wrote nothing" "{}" "$(cat "$HK2/.claude/settings.json")"
rm -rf "$HK2"
else
    skip_ "hooks merge (jq not installed)"
fi
rm -rf "$HK"
```

- [ ] **Step 2: Run to confirm FAIL** (`--hooks` is currently an unknown option, exit 2)
- [ ] **Step 3: Implement in `install.sh`** — extend the option case (:20-26):

```bash
hooks=0; dry=0
for a in "$@"; do case "$a" in
    "") ;;
    --uninstall) uninstall=1 ;;
    --hooks)     hooks=1 ;;
    --dry-run)   dry=1 ;;
    *) printf 'unknown option: %s\n\nusage: install.sh [--uninstall] [--hooks [--dry-run]]\n' "$a" >&2; exit 2 ;;
esac; done
[ "$dry" -eq 1 ] && [ "$hooks" -eq 0 ] && { printf -- '--dry-run needs --hooks\n' >&2; exit 2; }
```

and after the symlink loop:

```bash
if [ "$hooks" -eq 1 ]; then
    command -v jq >/dev/null 2>&1 || { printf 'jq is required for --hooks (the manual merge in README still works)\n' >&2; exit 1; }
    settings="$HOME/.claude/settings.json"
    mkdir -p "${settings%/*}" || exit 1
    [ -f "$settings" ] || printf '{}\n' > "$settings"
    # Additive, never destructive: their hooks stay; ours are appended per
    # event, skipping any entry already present (string-identical), so the
    # merge is idempotent and a re-run after an upgrade only adds what is new.
    merged=$(jq -s '
        .[0].hooks as $mine | .[1] as $theirs
        | $theirs + { hooks: (
            ($theirs.hooks // {}) as $h
            | $h + ($mine | with_entries(
                .key as $k
                | .value = (($h[$k] // []) + [ .value[]
                    | select(. as $e | (($h[$k] // []) | map(tojson) | index($e|tojson)) == null) ])
              ))
          ) }' "$DIR/examples/claude-hooks.json" "$settings") || { printf 'merge failed; %s untouched\n' "$settings" >&2; exit 1; }
    if [ "$dry" -eq 1 ]; then
        diff -u "$settings" <(printf '%s\n' "$merged") || true
        printf 'dry-run: nothing written\n'
    elif [ "$merged" = "$(cat "$settings")" ]; then
        printf 'hooks     already merged, nothing to do\n'
    else
        cp -- "$settings" "$settings.bak" || exit 1
        printf '%s\n' "$merged" > "$settings" || { cp -- "$settings.bak" "$settings"; exit 1; }
        printf 'merged    cockpit hooks into %s (backup: %s.bak)\n' "$settings" "$settings"
    fi
fi
```

(Note `jq -s` slurps `[claude-hooks.json, settings]`; claude-hooks.json's top level already has only `_comment` and `hooks`, and `.[0].hooks` selects just the hooks — the `_comment` never leaks into settings.)

- [ ] **Step 4: Run to green; verify the existing installer tests still pass** (the symlink behavior must be byte-identical when `--hooks` is absent)
- [ ] **Step 5: README** — Install section replaces "Wire the hooks in ~/.claude/settings.json" with the one-liner `./install.sh --hooks` plus `--dry-run` and the manual path as fallback.
- [ ] **Step 6: `make check` green; commit** — `git commit -m "feat: install.sh --hooks merges the claude hooks additively, with a dry-run diff"`

---

### Task 7: Integration (session lead, sequential, after all five lanes verify)

**Files:** merges only; no new code except conflict resolution in `tests/run.sh` / `cockpit.tmux`.

- [ ] **Step 1: Confirm every lane's verifier evidence** (make check green in its worktree + its perf/memory gate numbers + reviewer approval).
- [ ] **Step 2: Merge in overlap order, gating each merge:**

```bash
cd ~/.tmux/plugins/tmux-cockpit
for b in feat/f5-hooks feat/f1-jump feat/f3-ask feat/f2-fleet feat/f4-context; do
    git merge --no-ff "$b" || { echo "resolve $b, mostly adjacent blocks in tests/run.sh"; break; }
    make check || { echo "STOP: $b broke the gate"; break; }
done
```

The order is ascending file overlap: F5 touches only install.sh; F1 adds a script; F3 edits agent-state; F2 and F4 both touch cockpit.tmux/examples. Conflicts are expected ONLY as adjacent-append collisions at the end of `tests/run.sh` — resolve by keeping both blocks, any order.

- [ ] **Step 3: Full-system perf regression** — rerun the Task 0 measurements, append to `perf-baseline.txt` as `after:` lines; every gate within +10% (F4's documented exception aside).
- [ ] **Step 4: Leak sweep** — the shared protocol's three checks, plus `git worktree remove` for all five lanes and `git branch -d` the merged branches.
- [ ] **Step 5: Final commit of the perf record** — `git commit -m "chore: post-merge perf record for the rimz-inspired features"`. Do NOT push; the user decides.

---

## Self-review record

- **Spec coverage:** F1→Task 1, F2→Task 2, F3→Task 3, F4→Task 4, F5→Task 5; global constraints → Global Constraints + shared protocols; worktree/parallel requirement → Team & worktree protocol; perf/memory → Task 0 + per-lane gates + Task 7. No gaps.
- **Known uncertainty, stated in-plan:** F2's `#{n:}` nesting (Step 1 probe with a fallback), F4's statusline schema (fixture capture is Step 1, with an explicit stop-and-report exit). These are discovery steps with concrete commands, not placeholders.
- **Type consistency:** option names `@panestate`, `@panestate_msg`, `@panestate_ctx`, `@cockpit_jump_key`, token `#{agent_fleet}` are used identically across tasks; test helpers (`is/ok/bad/skip_/need_tmux/plain/mktmpd/st/sv`, `SOCKS`, `TMUX_REAL`, `$DIR`, `$PANE`) match tests/run.sh as read on 2026-08-24.
