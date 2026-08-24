# Lens: tmuxai (alvinunreal/tmuxai) — "check this, if it can help us"

Evaluated 2026-08-24. Clone at `scratchpad/tmuxai` (HEAD `fa16273`, 2026-07-07). All `tmuxai/...`
paths below are that clone; `scripts/...` paths are tmux-cockpit.

**Bottom line up front:** tmuxai is a *different tool solving a different problem* — an AI that
drives one shell pane inside one tmux window. It is not a reviewer, not a fleet monitor, and its
send-to-pane layer is measurably **less safe than what cockpit already ships**. It donates two
ideas and one negative lesson; everything else is a distraction. Do not adopt it, do not import
it, do not run it next to cockpit without pinning its exec pane. [I]

---

## 1. What it is, and how healthy

**What it is** [E]: a single Go binary you run *in a tmux pane*. It captures the text of the other
panes in **the current window only**, sends that plus your message to an LLM, and the model answers
with XML tags (`<ExecCommand>`, `<TmuxSendKeys>`, `<PasteMultilineContent>`) that the binary types
into a designated "exec pane" after a y/n confirmation.
- `tmuxai/internal/process_response.go:22,57-59` — the three action tags.
- `tmuxai/internal/pane_details.go:11-28,43-96` — context is built by walking `list-panes` for the
  current window and inlining each pane's `capture-pane` output as `<pane_content>` XML.
- `tmuxai/README.md` "Observe / Prepare / Watch Mode" — the three modes.
- Three ancillary systems: knowledge bases + a skills registry, web search/fetch, and MCP client
  (`tmuxai/internal/mcp/`). ~17.5k lines of Go including tests. [E]

**Health** (all `gh api`, 2026-08-24) [E]:

| metric | value |
|---|---|
| stars | 1,929 |
| forks / watchers | 119 / 15 |
| created | 2025-04-19 |
| `pushed_at` | 2026-08-22 (branch activity; **default branch HEAD is 2026-07-07**) |
| last *human* commit on main | 2026-06-30 (`Add Greptile CLI review skill`); last feature 2026-05-14 (`configurable status line`) |
| open issues | **3** — two dependabot bumps + "Please provide Windows releases" |
| releases | 20, v1.0.1 (2025-04) → v2.3.1 (2026-07-06); roughly monthly |
| license | Apache-2.0 (cockpit is MIT — copying nontrivial code requires attribution) |
| contributors | 18, but `alvinunreal` 65 commits vs `dependabot` 61; next human has 3 |
| CI | `go test ./...` + golangci-lint on push/PR (`tmuxai/.github/workflows/test.yml`) |

**Read** [I]: healthy-*looking* but low-velocity and bus-factor-1. Of the last 20 commits, 14 are
dependabot. The 3-open-issue count is not a sign of responsiveness so much as of a small user base
actually filing bugs (a stale-bot closes issues: `.github/workflows/stales-issues.yml`). It is
maintained, not thriving. Fine as a reference; a poor bet as a dependency.

---

## 2. Mechanisms we could reuse — honest triage

### 2a. The Go tmux wrapper — **copy ~40 lines, do not import**
`tmuxai/system/tmux.go` (252 lines) + `tmux_send.go` (140) is the whole tmux layer. Every call is a
bare `exec.Command("tmux", ...)` with stdout/stderr buffers. [E]

Worth stealing (all [E]):
- **Function-valued package vars for stubbing**: `var TmuxPanesDetails = func(...)`,
  `var TmuxCapturePane = func(...)` (`system/tmux.go:76,131,157,168`). Their tests reassign these
  and unit-test pane logic with **no live tmux server** (`internal/exec_pane_test.go:211,266,311`).
  This is the single most useful thing in the repo for us: cockpit's `tests/run.sh` needs a real
  server for the equivalent coverage.
- **One `list-panes -F` call, one parse** (`system/tmux.go:79`): six fields in one format string.
- **`buildSplitWindowArgs`** (`system/tmux.go:56-73`, 6 dedicated tests in `system/tmux_test.go`):
  validates user-supplied `split-window` flags against a reserved set (`-t -P -F`) with a
  flags-that-take-a-value table so `-c -F` isn't misread as the reserved `-F`. Genuinely careful
  code, and a good template if cockpit ever accepts user-supplied tmux args.

Reasons **not** to import `github.com/alvinunreal/tmuxai/system` [E]:
- Window-scoped by construction (`TmuxCurrentWindowTarget`, `system/tmux.go:150`); cockpit is
  server-wide. Nothing in the package can enumerate a fleet.
- No `context.Context` anywhere — no timeout, no cancellation. A wedged `tmux` or `ps` blocks forever.
- Coupled to a **global singleton logger** whose package funcs silently no-op when `Init()` wasn't
  called (`tmuxai/logger/logger.go:94-113`). Importing it means every error is logged into a void.
- Zero use of tmux buffers, options, hooks, popups, or control mode — `grep` for
  `load-buffer|paste-buffer|set-option|set-hook|display-popup|run-shell` across all `*.go`: **no
  hits**. Everything cockpit's architecture is built on is absent.
- Fragile parse: `strings.SplitN(line, ",", 6)` on a comma-joined format (`system/tmux.go:104`) —
  a `pane_current_command` containing a comma corrupts the row.

### 2b. Pane observation loop — **anti-pattern, cite it as a warning**
Per pane, per refresh, tmuxai runs `ps -p <pid> -o command=` and, if that looks like a login shell,
`pgrep -P <pid>` plus another `ps` per child (`tmuxai/system/utils.go:21-67`). That's 1–3 process
spawns **per pane per observation**, on top of a `capture-pane` per pane
(`system/types.go:100-107`). In prepare mode the completion detector re-captures every **500 ms**
(`internal/exec_pane.go:136-145`). [E]

This is precisely the cost class the freeze work already removed from cockpit
(`scripts/agent-state:149-171`: whole-server pane walks moved off the redraw path into precomputed
options). tmuxai gets away with it only because it runs in its own process, never on tmux's redraw
path. [I]

**The one salvageable idea**: `GetProcessArgs` gives the *full argv*, so it can tell
`claude --model opus` from a bare `node` — cockpit's `@cockpit_agents_glob` match on
`pane_current_command` cannot. Narrow reuse: run an argv probe **once, at pane-classification
time**, and cache the answer in a per-pane tmux option (the `@panestate` pattern). Never on redraw. [I]

### 2c. Send-to-pane safety — **strictly worse than ours; this is disqualifying**
tmuxai has **no bracketed paste at all**. Its only delivery is `send-keys -l` line by line, then an
explicit `Enter` per line when `autoenter` is set (`tmuxai/system/tmux_send.go:11-56`). Its
"paste multiline" path calls it with `autoenter=true` — i.e. it **executes**
(`internal/process_message.go:316`). [E]

Worse, there is a live keystroke-injection bug in the text path [E]:
```go
// system/tmux_send.go:59-73
func containsSpecialKey(line string) bool {
    if strings.Contains(line, "C-") || strings.Contains(line, "M-") { return true }
    for key := range getSpecialKeys() { if strings.Contains(line, key) { return true } }
```
`getSpecialKeys()` (`:121-140`) contains `"Enter"`, `"Space"`, `"Tab"`, `"End"`, `"Home"`, `"Up"`,
`"Down"`, `"Left"`, `"Right"`. Any line *containing* one of those words routes to
`processLineWithSpecialKeys` (`:77-119`), which emits every whitespace-delimited token equal to one
of those names as a **real keystroke argument to `send-keys`**. So the sentence
`press Enter to continue` types `press`, then presses **Enter**, then types `to continue`.

Cockpit already does the right thing and should not move an inch toward this: `load-buffer -b
<per-invocation-name>` + `paste-buffer -p -r -d` (bracketed, typed-but-unsent), behind a pane-id
shape guard, an `@cockpit_agents_glob` re-check inside `tmux if-shell -F` to close the TOCTOU
window, and a 2048-byte spill to a `umask 077` mktemp file (`scripts/tmux-review`, deliver()).
tmuxai has none of these four guards. [E]

### 2d. "Prepare" mode as a turn-boundary detector — **steal the concept, reject the mechanism**
The idea [E]: overwrite the exec pane's prompt with a sentinel carrying the exit code —
`export PS1='\u@\h:\w[\A][$?]» '` (`internal/exec_pane.go:99-124`) — then reconstruct
command/exit-code/output triples from `capture-pane` text with
`regexp.MustCompile(`.*\[(\d+)\]» ?(.*)$`)` (`internal/exec_pane.go:177`), and treat
"last line ends with `]»`" as "the turn is over" (`internal/exec_pane.go:140`).

Why it matters to us [I]: cockpit's entire state model rides on **Claude Code hooks** calling
`scripts/agent-state`. For an agent that emits no hooks — aider, codex, a plain build loop — cockpit
is blind. A sentinel-based boundary is the generic answer to that.

Why *this* implementation is not the one to copy [E/I]:
- It clobbers the user's prompt and runs `unset PROMPT_COMMAND` for bash (`:112`) — that breaks
  starship, atuin, direnv, and any precmd chain. [E] Directly violates cockpit's non-intrusiveness.
- It is delivered by *typing the export into the pane* (`:122`), i.e. it needs a shell at a prompt.
- Any output line ending in `]»` fakes a boundary — no escaping, no nonce. [E]
- It only works for a **shell** pane. Claude Code is a full-screen TUI that owns the alternate
  screen; there is no PS1 to patch. So the mechanism cannot cover the agents cockpit monitors. [I]
- The detector is a 500 ms capture poll (`:140-145`). [E]

The right transport for the same concept, if we ever want it [I]: OSC 133 semantic prompt marks
(shell integration already emits these in fish/zsh/bash-preexec and several agents), or per-agent
hook shims, or a `pane_pid` child-process watch — all of which keep the user's prompt untouched.
tmuxai is the citation for "why the sentinel is worth having", not the implementation.

### 2e. Config / model layer — **relevant only if we grow an AI reviewer**
viper + mapstructure YAML at `~/.config/tmuxai/config.yaml` with `${ENV}` expansion, and a
`models:` map keyed by alias with a `provider` field spanning openrouter / openai / azure / gemini /
bedrock / github-copilot (`tmuxai/config/config.go:14-152`, `config.example.yaml`). [E] Conventional
and clean; ~800 lines of `internal/ai_client.go` implement the dispatch.

Not applicable to cockpit's current surface: `@cockpit_*` tmux options are the right config
mechanism for a tmux plugin (nothing to read on redraw, composes with themes sourced later). [I]
The one piece worth borrowing *if* an AI reviewer lands: the three-tier command safety model —
user `whitelist_patterns` / `blacklist_patterns` (`config.example.yaml`) layered over a static
`risk_scorer.go` (safe/unknown/danger regex tables, `internal/risk_scorer.go:27-90`) rendered as a
`✓ / ? / !` badge in the confirm prompt (`internal/confirm.go:29-51`). Their own README explicitly
says the score is "for guidance only… should not be relied upon for security decisions" — good
honesty we should copy along with the pattern. [E]

---

## 3. Could tmuxai BE our reviewer/orchestrator? — **No.**

Hard evidence it has nothing to do with review [E]: `grep -rn "git diff\|review" --include=*.go` over
the whole repo returns **zero** functional hits (only the string "preview" and a model name). The
two things that look like review are decoys:
- `tmuxai/.agents/skills/cli-review/SKILL.md` is a **Claude Code skill for people hacking on
  tmuxai** that shells out to the third-party `greptile` CLI. Not a tmuxai feature.
- `tmuxai/tasks/review-history.txt` is a watch-mode prompt about *shell command history*, not code.

And it is structurally the wrong shape for the job [E/I]:
- **Wrong direction.** tmuxai *writes into* a pane it controls; cockpit needs something that *reads
  another agent's git tree* and returns structured comments. tmuxai's only output channels are
  chat text in its own pane and `send-keys` into an exec pane.
- **No structured output.** Responses are XML action tags for terminal actions
  (`internal/process_response.go:22`). There is no "emit a list of findings anchored to
  path:line" — which is the whole content of cockpit's revdiff store grammar.
- **It would auto-submit.** Any delivery path goes through §2c, which presses Enter (and would
  press Enter mid-sentence on the word "Enter"). Cockpit's hardest invariant is
  never auto-submit into an agent.
- **Wrong scope.** Current window only (`internal/pane_details.go:11-14`). No fleet.
- **It would fight for the agent's pane.** See §4.

What you'd actually be after is tmuxai's *provider abstraction* — and the cheap version of that is
`claude -p '<prompt>'` (or any CLI already on the box) piped into a script that emits the same
`## path:line (+|-)` grammar the store already speaks. That is a few dozen lines, no daemon, no
new dependency, and it keeps the human's alt-Enter/^S loop exactly as-is. [I]

---

## 4. Overlap / conflict if both run

| surface | conflict | evidence |
|---|---|---|
| **panes** | **Real and serious.** `GetAvailablePane` picks *the first pane in the window that isn't tmuxai's own* and makes it the exec pane it types into (`internal/exec_pane.go:16-26`). Put tmuxai in a window with a running Claude pane and it will `send-keys` its commands **into your agent**. If no pane qualifies it silently `split-window`s a new one (`:86`, default `-d -h`). | [E] |
| **pane churn** | `TmuxClearPane` does `split-window -vp 100` → `clear-history` → `kill-pane` — it creates and destroys a pane to clear another (`system/tmux.go:216-250`). Every create/destroy fires cockpit's pane hooks and flickers fleet state. | [E] |
| **keys** | **None.** tmuxai installs no `bind-key` and no `set-hook` (grep: zero hits). It reads the keyboard only inside its own process via `eiannone/keyboard` (`internal/countdown.go:18-34`). `prefix r` is safe. | [E] |
| **status line** | **None.** It never touches tmux options or `status-*`. Its `status_line:` config is its own REPL prompt template (`internal/manager.go:219-269`). No collision with `@cockpit_fleet` / status-right. | [E] |
| **buffers** | **None.** It uses no tmux buffers, so no race with cockpit's per-invocation `cockpit-review-<pane>-$$` names. | [E] |
| **perf** | Not on the redraw path, so it cannot reproduce the freeze. But it adds continuous background load (capture + ps + pgrep per pane per turn; 500 ms polling in prepare mode) and, in watch mode, a full model call every `wait_interval` seconds forever. | [E]/[I] |

**Mitigation if someone insists on running both** [I]: always start tmuxai with an explicit
`ForcedExecPaneID` (it validates the pane exists and refuses to use its own chat pane —
`internal/exec_pane.go:44-51`), and keep it in a window with no agent panes.

---

## 5. As a reference architecture for a Go rewrite — mixed, leaning **anti-pattern**

**Patterns worth copying** [E]:
1. `var Fn = func(...)` package-level stubs for every external call → real unit tests with no tmux
   server (`system/tmux.go:76,131,157,168`; `internal/exec_pane_test.go`). Best idea in the repo.
2. Argument-validation-with-tests for user-supplied tmux flags (`system/tmux.go:56-73` +
   `system/tmux_test.go`, 6 cases).
3. Distribution shape: `.goreleaser.yml` + `install.sh` + per-arch release binaries +
   `curl | bash`. A Go cockpit stops being `git clone`-able as a TPM plugin; this repo shows the
   whole answer to that problem, and it is the biggest hidden cost of the rewrite. [I]

**Anti-patterns to avoid, with citations** [E]:
1. **One `exec.Command` per tmux operation, never batched, never control mode.** `TmuxClearPane`
   alone spawns four processes (`system/tmux.go:216-250`). For a server-wide fleet monitor this is
   O(panes) processes per refresh — exactly what cockpit's precompute deleted. A Go rewrite's real
   win would be `tmux -C` control mode or a single `list-panes -a -F`; tmuxai does **neither**.
2. **No `context.Context` in the subprocess layer** — no timeouts, no cancellation, anywhere.
3. **Errors logged *and* returned, then discarded by callers**: `_ = system.TmuxSendCommandToPane(...)`
   at `internal/exec_pane.go:122,123,131`; three consecutive dropped errors at
   `internal/pane_details.go:12-14`.
4. **Silent-no-op global logger** (`logger/logger.go:94-113`): if `Init()` didn't run, every
   `logger.Error` vanishes.
5. **God object**: `Manager` (`internal/manager.go`, ~40 fields) holds config, session overrides,
   tmux state, chat history, MCP, KB, skills — *and prints to stdout* (`m.Println`,
   `internal/process_message.go:236,265,316`). Business logic writing to stdout is untestable.
6. **Recursion used as a loop.** `startWatchMode` calls itself after each cycle
   (`internal/process_message.go:383-405`); `ProcessUserMessage` re-enters itself for follow-ups
   and MCP results (`:92,341-357`). Unbounded stack growth over a long watch, and no place to put
   backpressure. A `for` loop with a ticker and a cancellable context is the correct shape.
7. **Stringly-typed state**: `m.Status == "running" | "waiting" | "done" | ""`, with `""` as the
   sentinel for "stop" (`internal/manager.go:180ff`, `internal/process_message.go:403`).
8. **Goroutine leak per countdown**: each `Countdown` spawns an unmanaged `keyboard.GetKey` reader
   (`internal/countdown.go:26-34`) and watch mode calls it every cycle.
9. The `containsSpecialKey` substring bug (§2c).

**Verdict on the rewrite question** [I]: tmuxai proves the *mechanics* work (shell out to tmux from
Go, stub via function vars, ship via goreleaser) and nothing more. Architecturally cockpit's current
bash design is **ahead of it** on every axis that matters here — bracketed paste, TOCTOU re-checks,
tmux-option state that dies with the pane, zero shell on redraw, server-wide scope. Do not hold
tmuxai up as the target architecture in the Go evaluation; cite it for the three good patterns and
the nine bad ones.

---

## Ranked recommendations

| # | What | Why | Cost | Replaces |
|---|---|---|---|---|
| 1 | **Do not adopt, import, or run tmuxai as part of cockpit.** Close the question. | Different problem (drives a shell pane), window-scoped, no review capability, and a send path that presses Enter mid-sentence (`system/tmux_send.go:59-73`). Time spent integrating is time not spent on the reviewer. | S | The open "can it help us?" thread |
| 2 | **Record the exec-pane hazard in README/docs**: tmuxai (and any tool with the same "grab the first free pane" heuristic) will `send-keys` into a cockpit agent pane. Tell users to pass an explicit exec pane. | `internal/exec_pane.go:16-26` picks the first non-self pane and types into it. One sentence prevents a very bad surprise. | S | Nothing — new |
| 3 | **Adopt the function-var stubbing pattern** for cockpit's tmux touchpoints (bash equivalent: route every `tmux` call through one `_tmux()` indirection that tests can override). | Their `internal/exec_pane_test.go` unit-tests pane logic with no tmux server; cockpit's tests need a live server for the same coverage. Faster, hermetic tests. | M | Parts of `tests/run.sh` that spin a server |
| 4 | **If a turn-boundary detector for non-Claude agents is ever wanted, spec it on OSC 133 / hook shims — explicitly NOT on PS1 rewriting.** File tmuxai's prepare mode as the motivating prior art *and* the cautionary one. | The concept is sound and it's the only cockpit-relevant idea here; the implementation clobbers `PROMPT_COMMAND`, fakes boundaries on any `]»` line, and cannot work for a full-screen TUI agent. | M (spec only; L to build) | Nothing today — cockpit is hook-only |
| 5 | **If an AI-drafted review lands, use `claude -p` (or any local CLI) emitting the existing revdiff grammar — not tmuxai, not a provider abstraction.** Borrow only tmuxai's *safety* shape: user whitelist/blacklist patterns over a static risk table, surfaced as a badge, documented as advisory-only. | Keeps the one-shot / no-daemon invariant, keeps the human's alt-Enter/^S loop, adds no Go dependency. `config.example.yaml` + `internal/risk_scorer.go:27-90` + `internal/confirm.go:29-51` are the template. | M | A hypothetical "run tmuxai as the reviewer" design |
| 6 | **Cache an argv-based agent identity once per pane in a tmux option** (`ps -p <pane_pid> -o command=`), if `@cockpit_agents_glob` on `pane_current_command` ever proves too coarse. Probe at classification time only — never on redraw. | `system/utils.go:21-67` shows argv distinguishes `claude --model opus` from bare `node`; it also shows the cost (1–3 spawns per pane) that forbids doing it live. | S | Nothing — a sharpening of the existing glob |
| 7 | **Feed §5's three good / nine bad patterns to the Go-rewrite lens as a checklist**, with the explicit note that a Go cockpit's win must come from control mode or one `list-panes -a`, since tmuxai's per-call `exec.Command` would be a *regression* against today's precomputed options. | Prevents the rewrite from importing tmuxai's process-per-call model, which is the same cost class the freeze fix just removed (`scripts/agent-state:149-171`). | S | An unexamined "Go is faster" assumption |
