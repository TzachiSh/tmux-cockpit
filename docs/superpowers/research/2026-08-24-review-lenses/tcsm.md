# Lens: craftzdog/tmux-claude-session-manager (TCSM)

Scope: evaluate TCSM against tmux-cockpit. Claims tagged **[E]** evidence (measured / read / API-verified) or **[I]** inference.

---

## 0. The thing you already know but the brief doesn't say

Two of TCSM's issues are **yours**. `sefr-sefr` filed #22 and #23 [E: `gh api .../issues?state=all --jq '.user.login'`].

- **PR #22** "Title, group, and surface what needs you in the picker" — +482/−55 over 7 files, head `sefr-sefr:peter-fork`, opened 2026-08-18 08:45, **closed unmerged 20 minutes later** [E: `gh api .../pulls/22 --jq '{merged:false, merged_at:null}'`]. Files: `scripts/rows.awk` (new, 101 lines), `test/contract.sh` (new, 185), `scripts/kill.sh`, `scripts/preview.sh`, plus edits to `agents.sh`/`helpers.sh`/`picker.sh`.
- **Issue #23** "Picker open time is dominated by `claude agents --json` under load" — closed by you with *"Filed against the wrong repository by mistake — this was meant for a private fork."* [E].

So: you already have a private fork of TCSM with ai-title, project grouping, `waitingFor`, a background-agents row, and a fixture-driven contract test suite. **The interesting question is not "should we look at TCSM" — it's "why is that work living in a fork of someone else's 330-line plugin instead of in cockpit."** I answer that in §4.

---

## 1. What it is, and how healthy

**Precisely:** a 330-line-of-shell tpm plugin that (a) launches one detached tmux session per project directory running `claude`, shown in a popup, and (b) gives you one fzf popup listing *every running Claude process on the machine* with its self-reported status, a live `capture-pane` preview, jump-to, and kill. It does **not** touch diffs, git, worktrees, or review. [E: full source read; `README.md:5-28`]

**Health** [E: `gh api repos/craftzdog/tmux-claude-session-manager`]:

| metric | value |
|---|---|
| stars | 368 |
| created | 2026-06-12 |
| last push | 2026-08-17 (7 days ago) |
| forks | 47 |
| open issues | 1 (#17, a legitimate bug fix awaiting the author's blessing) |
| releases | 4 — v1.0.0 (07-08), v1.0.1, v1.1.0 (07-09), v1.2.0 (08-17) |
| license | MIT |
| size | ~330 LOC shell + README |
| contributors | craftzdog 25, four others 1–3 each |
| CI / tests | **none** |

Healthy-ish: shipping, responsive, 23 issues closed in 10 weeks, mostly community-driven fixes (command injection #18, launch quoting #19, popup races #15/#17). Owner is a well-followed dev-influencer (devas.life, YouTube), which explains the star curve more than the code does. **Zero tests, zero CI** — your PR #22 would have been the first test file in the repo, and it was closed unmerged without comment [E: no comments on #22].

**Problem it solves that overlaps cockpit:** *"which of my N Claude agents is done / stuck / still working, without visiting each one"* — the author's stated motivation is verbatim *"it's been annoying to check whether any sessions are finished or need my answers by switching windows"* [E: devas.life article]. That is cockpit's entire top-of-README pitch. The overlap is the **fleet-state model**, not the surfaces: TCSM answers it in a modal popup you open on demand; cockpit answers it ambiently in the status bar, tabs and borders and only opens a popup to *act*.

---

## 2. Feature-by-feature

| axis | TCSM | cockpit | verdict |
|---|---|---|---|
| **agent discovery** | `claude agents --json` → `pid`, join `pid→tty` via `ps -Ao pid=,tty=` → `tty→pane` via `tmux list-panes -a` [E: `scripts/agents.sh:20-44`] | `#{pane_current_command}` matched against `@cockpit_agents_glob` [E: `README.md:639-663`] | **TCSM better on correctness, worse on breadth & cost** — see below |
| **state detection** | Claude self-reports; `status ∈ {idle,busy,waiting}` + `waitingFor` [E: docs + live `--json`] | 11 Claude hooks → `scripts/agent-state` → `@panestate` [E: `examples/claude-hooks.json`] | **TCSM's source is better; cockpit's transport is better** |
| **latency of a state read** | 0.49–0.85 s quiet, 1.0–2.5 s loaded [E: `/usr/bin/time` ×3 here; your own #23 measured 2494/1363/1002 ms] | **0 shell spawns on redraw** — tmux reads an option [E: `agent-state:125-138`, README:68-84] | **cockpit wins by ~1000×** |
| **naming / labels** | `name` from `--json` ("tmux-cockpit-eb"); your fork adds `ai-title` from the transcript | `pane_title` (Claude's OSC-2) with `window_name` fallback [E: `tmux-review:582`] | **`ai-title` better than both** — see §3.1 |
| **attention / notification** | `alert-bell` global tmux hook re-emits `\a` into the origin window's pty [E: `scripts/bell.sh:28`] | `▲`/`✕` markers, red border, cross-window `NEEDS YOU` banner, whole-pane tint, `#{agent_fleet}` counter | **cockpit much better** — TCSM's is one bell, no state |
| **what is it blocked on** | `waitingFor` ∈ `permission prompt` / `input needed` / `sandbox request` / `worker request` / `dialog open` [E: agent-view docs] — but TCSM `main` *doesn't render it*; only your fork does | `@panestate_msg`, first 120 chars of the `Notification` payload's `message` [E: `agent-state:71-97`] | **cockpit's is richer text; TCSM's source is a stable enum.** Complementary |
| **resume** | attaches to a long-lived tmux session; never uses `claude --resume` [E: `launch.sh:31`, `picker.sh:74`] | n/a — cockpit doesn't launch agents | **neither uses `--resume`; both leave `sessionId` on the table** |
| **worktrees** | none. Author's answer to "multiple Claudes per project" was literally *"Make a worktree, then"* [E: issue #9 comment]; TCSM has no worktree code at all | `scripts/tmux-where` reads `.git/HEAD` directly, no `git` process, renders `feat-api ⑂api-work` [E: README:583-607] | **cockpit wins outright** |
| **session picker** | none (the agent picker is not a session picker) | `scripts/tm` — attach/create/kill, per-session window preview with `@panestate` beside each agent pane [E: `tm:177-187`] | **different products; no overlap** |
| **review** | **nothing.** Author outsources to codediff.nvim + lazygit [E: devas.life] | `tmux-review`, 743 lines, revdiff-grammar comments pasted back typed-but-unsent | **cockpit is alone here** |
| **safety of paste-back** | n/a | pane-id shape guard + `pane_current_command` re-check inside `tmux if-shell -F` before `paste-buffer` [E: `tmux-review:467-482`] | cockpit only |
| **tests** | 0 | 309 cases, `tests/run.sh` [E: README:609-619] | cockpit wins |

### 2a. Where TCSM's mechanism is genuinely better than ours

**(1) `pid → tty → pane` beats `pane_current_command`.** [E]
Cockpit's is-agent test is `#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}}`. That is a **foreground-process-name** test. It is wrong in three ways TCSM's join is right:
- On macOS a pane often reports the parent shell, never the `claude` child [E: `agents.sh:6-7`, README:220-222, and craftzdog's PR #14 rationale]. Cockpit's own README:665-677 leans on this exact test to solve staleness — so on macOS cockpit can *silently show nothing at all*.
- `claude` wrapped in anything (a `direnv exec`, a `script`, a devcontainer shim, `uv run`) reads as that wrapper.
- Conversely, `pane_current_command == claude` is true for `claude --version` sitting at a prompt.
TCSM's join asks the kernel which pid owns which tty and asks tmux which pane owns which tty. That is exact. **Cost: 2 subprocesses total, not per pane** [E: `agents.sh:32-36` — one `ps`, one `list-panes -a`].

**(2) `waitingFor` is a stable enum; our `@panestate_msg` is string-matching English.** [E]
`agent-state:47-56` matches `*'waiting for your input'*` and `*'needs your permission'*` against the Notification payload. That is Claude's *user-facing prose*. One copy-edit upstream and cockpit's red goes dark, silently. `waitingFor` is documented, enumerated, and versioned. Our fallback-to-`exit 0` on an unrecognised notification (`agent-state:57`) means a wording change fails **closed** — the alert never fires and nothing tells you.

**(3) A row per Claude *process*, not per pane/session.** [E: PR #14 summary]
Cockpit is pane-scoped, which is mostly fine — but a `claude` running inside a pane's *background* (`run_in_background`, a nested agent, a `claude -p` in a script) is invisible to cockpit and visible to TCSM. Marginal today; less marginal as background agents grow.

**(4) It sees `kind: "background"` sessions.** [E: agent-view docs — `kind ∈ {interactive, background}`, `state ∈ {working,blocked,done,failed,stopped}`]
Cockpit has *no* concept of a background agent. `claude agents` dispatches them, they have no pid, no tty, no pane — so cockpit structurally cannot render them, ever. Your own fork already solved this with an aggregate row. This is a real coverage hole, not a nit.

### 2b. Where TCSM is worse

**(1) Latency.** Measured here on a 6-agent machine: `claude agents --json` = **0.85 / 0.63 / 0.49 s** [E]. `--cwd`-filtered is *not* faster: **0.71 / 0.73 s** [E] — the cost is CLI startup + supervisor IPC, not result size, so your #23's "possible direction 2" is dead. For comparison cockpit's whole `tmux-review --fleet` — 5 agent panes, `git status --porcelain` + `git diff --numstat` each — runs in **0.06 s** [E: `/usr/bin/time ×3`]. TCSM pays 10× cockpit's *entire git-walking fleet scan* just to learn who is running. It is unusable on a redraw path and marginal even on a popup.

**(2) Claude-only.** Hard-coded `claude agents`, `claude-` prefix, `.claude/projects`. Cockpit covers `claude|codex|aider|cursor|opencode|gemini|crush` from one option [E: README:641]. Issues #3, #11, #12, #16 all asked for multi-provider; all closed. craftzdog's answer is "fork it".

**(3) The session-per-project popup model is a cul-de-sac.** `launch.sh:19` names the session `claude-$(md5(dir)|cut 8)`. Consequences: an unreadable session name in `choose-tree`; exactly one *launchable* session per directory (a second agent in the same repo must be started by hand as "loose"); a popup-in-popup dance (`list.sh:31-61`) with a 100-iteration poll + two `sleep`s to work around a tmux teardown race; and bug #17, still open, where attaching to a `claude-*` session any other way makes `prefix+u` **detach you out of tmux entirely** [E: #17 repro]. Cockpit's model — agents live in ordinary panes you can see — has none of this.

**(4) No tests, and the author closed the PR that added them.** [E]

**(5) Fragile-by-design bits worth not copying:** `list.sh:41-59` polls `list-clients` 100× at 50 ms then retries `display-popup` 40× — a race worked around, not fixed; `picker.sh:41` `reload(sleep 0.3; ...)` to let the supervisor catch up; `helpers.sh:52` globs `~/.claude/projects/*/<uuid>.jsonl` to avoid reimplementing the cwd→slug encoding (that one is actually *good* — see §3).

---

## 3. What to copy

### 3.1 `ai-title` from the transcript, for tab labels and fleet rows — **highest value, lowest cost**

Cockpit's tab label is `pane_title`, i.e. Claude's OSC-2 title. Two problems, both already documented in your own README:
- Cockpit ships `set-titles off` because OSC title storms froze Windows Terminal — 609 titles / 27 KB in 3 s [E: README:41-51]. You are depending on the exact mechanism you had to defuse.
- It is often useless. Live proof, right now, from your own `--fleet` output: `%458  henosis:3  Claude Code` [E] — a running agent whose label is the literal default.

The transcript carries a maintained one. Records look like:
```json
{"type":"ai-title","aiTitle":"Replace all to go or rust","sessionId":"54971e6a-…"}
```
[E: `~/.claude/projects/-home-tzachi--tmux-plugins-tmux-cockpit/54971e6a-….jsonl`, 12 such records in one session]. Read backwards, first hit wins:
```
tac "$f" | grep -m1 '"ai-title"'   →  0.009 s on a 64 MB project dir  [E: measured]
```
9 ms. That is cheaper than the `git status` cockpit already runs per fleet row.

**But do not get there via `claude agents --json` (0.7 s).** Get there from the hook you already run. Every hook's stdin JSON carries `session_id` **and `transcript_path`** [E: code.claude.com/docs/en/hooks common-fields table + example payload]. `agent-state` already reads stdin in the `notify` arm and already parses one JSON field with pure parameter expansion (`agent-state:71-97`). Extend that: on `SessionStart`/`UserPromptSubmit`, stash `transcript_path` into a pane option (`@panestate_tx`) and `session_id` into `@panestate_sid`. Then:
- the fleet queue reads `ai-title` per row in ~9 ms instead of trusting `pane_title`;
- `#{@panestate_sid}` is the exact argument to `claude --resume` [E: agent-view docs — *"`sessionId` is the full session UUID, usable with `claude --resume`"*];
- `--lines` can anchor a review to a session id, not just a cwd.

Zero new dependencies, zero new subprocesses on the redraw path, no `jq`, no `claude` invocation. TCSM needed `claude agents --json` only because it refuses to install hooks; **you already have the hooks**, which means you can have TCSM's data without TCSM's latency. This is the single clearest steal.

### 3.2 `waitingFor`-style vocabulary for `@panestate_msg`

Not the field itself (you'd need the slow CLI), but the *discipline*: your Notification prose-matching is one upstream copy-edit from silent failure. Two cheap hardenings, both local to `agent-state`:
- Make the `*)` arm at `agent-state:57` record an "unrecognised notification" breadcrumb (a pane option, a counter) instead of `exit 0`. Right now an upstream wording change is indistinguishable from an unrelated notification.
- Cockpit already sees `PermissionRequest` as its own hook [E: `examples/claude-hooks.json`] — that is a *structured* signal for the `permission prompt` case, no prose matching needed. Lean on it and demote the prose match to a fallback for the other `waitingFor` kinds.

### 3.3 The `pid → tty → pane` join — copy it into the *fleet queue only*, never the redraw

`ps -Ao pid=,tty=` + `tmux list-panes -a -F '…#{pane_tty}…'` + one awk = 2 subprocesses, exact, and fixes the macOS `pane_current_command` blindness [E: `agents.sh:32-44`]. But it needs a pid→"is this claude" mapping, which is where TCSM pays the 0.7 s. Cheap variant: `ps -Ao pid=,tty=,comm=` and match `comm` against `@cockpit_agents_glob` — one subprocess, still exact about tty, still multi-provider, and it *does* see a `claude` that isn't the pane's foreground process. Use it in `fleet_rows()` (already a 60 ms shell script) as a **supplement** to the `pane_current_command` test — union, not replacement, so nothing regresses.

### 3.4 Naming: kill the md5 session names, but steal "origin window"

`@claude_origin` (`launch.sh:35`, `picker.sh:68-70`) — record which window a thing was launched *from*, so you can send the client back there. Cockpit has an analogous need in `tmux-review`: after `^S` delivers a batch you are dropped wherever the popup closed, with no route back to the pane you just reviewed. Recording the origin pane at popup-open and offering "jump to the pane you just commented on" is one `select-pane` away.

### 3.5 What to explicitly avoid

- `claude agents --json` **on any path a human waits on** — 0.5–2.5 s, `--cwd` doesn't help [E].
- Session-per-directory with a hash name. Unreadable, one-agent-per-repo, and it created bug #17 and the whole `list.sh` popup-race apparatus.
- Bell forwarding as an attention mechanism. It carries one bit and no state; cockpit's five-state marker set strictly dominates. And the config it demands (`preferredNotifChannel`, `messageIdleNotifThresholdMs` in a *different file*, README:110-141) is exactly the "one-time setup" tax TCSM markets itself as not having.
- `reload(sleep 0.3; …)` and 100-iteration client polls. Cockpit's invariant list ("one-shot scripts, no daemon") is better than this.
- Copying the `capture-pane` preview into cockpit's fleet queue. It's nice in TCSM because TCSM has nothing else to show. Cockpit's fleet preview is `--preview-diff` — *the changes*, which is what a review queue is for. Don't downgrade it to a screen scrape.

---

## 4. Replace / coexist / conflict

**Conflict: essentially none.** [E]
- Keys: TCSM binds `prefix y` (launch) and `prefix u` (list) [E: `claude_session_manager.tmux:12-13`]. Cockpit binds `b`, `S`, `r`, `R`, `J` + two chorded mouse gestures [E: `cockpit.tmux:214-265`]. Disjoint.
- Status line: TCSM writes none. Cockpit rewrites six options. No contact.
- tmux hooks: TCSM sets `set-hook -g alert-bell`. Cockpit sets no tmux hooks; `examples/full.tmux.conf:223-226` sets four `refresh-client` hooks, none of them `alert-bell` [E: grep]. No clobber.
- `settings.json` hooks: TCSM v1.1+ needs **none**, and its own upgrade note says *remove* its old block [E: PR #14 breaking-changes]. Its optional bell hook is an additional `Stop` entry, which coexists with cockpit's `Stop → agent-state done` (Claude runs all matching hooks). No conflict.
- One real friction: TCSM's dedicated `claude-*` sessions would show up in `tm`'s session picker as unreadable `claude-3f9a2b1c` rows [I].

**Replace `tm`? No.** `tm` is a general session launcher (attach/create/kill-multi/name-prompt). TCSM has no session picker at all — its picker lists *agents*. Different objects. The one thing to note is that `tm`'s window preview already shows `@panestate` per agent pane [E: `tm:177`], which is 70% of TCSM's picker rendered for free inside a picker you already have.

**Coexist? Yes, cleanly — but it's redundant.** Everything TCSM shows you, cockpit shows you ambiently without opening anything, except: background agents, the exact `waitingFor` enum, and the macOS `pane_current_command` case. **My read: don't install it. Steal §3.1 and §3.3 into cockpit and you have strictly more than both.**

**The fork question.** Your `peter-fork` work (rows.awk, contract tests, grouping, ai-title, background-agents row) is 482 lines living downstream of a repo whose owner closed it in 20 minutes with no comment and ships no tests. That work is worth more inside cockpit — where it can share `@cockpit_agents_glob`, `tmux-where`, the state model and the 309-case suite — than as a permanent rebase burden on a 330-line plugin you disagree with architecturally. [I, but strongly]

---

## 5. Does it inform the review redesign?

**Directly: barely. Indirectly: two useful things.**

TCSM has zero review surface. craftzdog reviews in codediff.nvim and lazygit [E: devas.life]. So there is no "resume + review" flow to copy and no per-session review moment to copy — **the gap cockpit's `tmux-review` fills is one the highest-profile tmux-Claude plugin doesn't even attempt**, which is a point in favour of the feature existing at all.

What it *does* inform:

1. **The fleet queue is the review's real entry point, and it should be a state-ranked triage list, not a churn-ranked one.** TCSM's picker sorts `rank asc, age asc` where rank is `waiting(0) < idle(1) < ?(2) < busy(3)` [E: `agents.sh:46-60`] — *"what needs you floats up, and whatever just went idle sits at the top of its group."* Cockpit's fleet queue sorts by **churn desc** [E: `tmux-review:622`]. Churn is a proxy for "big diff", not for "ready for review". A `busy` agent's diff is *mid-write* — reviewing it is worse than useless, and cockpit will happily put it first if it's noisy. Sort by `@panestate` first (`done` > `needs-input` > `error` > `running`), churn only within a band. Cockpit already has this state per pane at zero cost; the queue just ignores it. **This is the single most defensible change TCSM argues for.**

2. **Label rows by topic, not by pane.** Your PR #22's own "before" block is the argument: four rows reading `● idle 37m work:2.0 ~/code/foo` tell you how many threads are open and nothing else. Cockpit's fleet rows have the same disease when `pane_title` is unset — `%458 henosis:3 Claude Code` [E, live]. `ai-title` (§3.1) fixes the review queue at the same time it fixes the tabs.

3. **A "review moment" hook exists and neither project uses it.** `Stop` fires at every turn end, and cockpit already wires it to `done` [E: `examples/claude-hooks.json`]. A pane that just went `done` *with a dirty tree* is precisely the review moment. Cockpit could pre-warm the fleet queue's git numbers on that transition (the same precompute pattern `agent-state:149-213` already uses for the banner and fleet counter), so `prefix r` renders from an option instead of walking N repos — turning the queue's 60 ms into ~0 and making it correct-by-transition rather than correct-by-rescan. TCSM cannot do this at all, because it refused hooks.

---

## Bottom line

TCSM is a well-marketed 330-line popup with one genuinely better idea (ask Claude what Claude is doing, and resolve identity through the tty) and one disqualifying cost (0.5–2.5 s to ask). Cockpit is a larger, tested, multi-provider, zero-redraw-cost system with a review feature TCSM's author doesn't believe belongs in tmux. **Take the transcript (`ai-title`, `transcript_path`, `sessionId`) via the hooks you already run — not via the CLI TCSM is stuck with — and take the `waiting`-first sort into the fleet queue. Leave the session-per-hash model, the bell, and the popup-race machinery where they are.**
