# Lens: WHEN and HOW we review

Scope: the review *moment*, the *diff base*, the *loop*, the *delivery*, and the *AI first pass*.
Not the picker's visual design (that is the UI lens) and not the tool landscape census (that is
the landscape lens) — both are cited here only where they carry the WHEN/HOW argument.

Evidence tags: `[E]` = verified against a fetched source or a file in this repo; `[I]` = inference.

Star counts / last push verified 2026-08-24 via `gh api repos/OWNER/REPO`:

| project | stars | last push |
|---|---|---|
| anthropics/claude-code | 142,861 | 2026-08-23 |
| BloopAI/vibe-kanban | 27,904 | 2026-04-24 |
| modem-dev/hunk | 8,711 | 2026-08-24 |
| smtg-ai/claude-squad | 8,361 | 2026-08-20 |
| backnotprop/plannotator | 7,985 | 2026-08-24 |
| google/git-appraise | 5,309 | 2023-08-12 (dead) |
| yoshiko-pg/difit | 3,095 | 2026-08-09 |
| umputun/revdiff | 807 | 2026-08-20 |
| nilbuild/diffity | 758 | 2026-07-06 |
| hamelsmu/claude-review-loop | 722 | 2026-03-15 |
| umputun/cc-thingz | 451 | 2026-08-24 |
| IrtezaAsadRizvi/claude-human-review | 15 | 2026-04-27 |

---

## 0. Why "how it works today we can't use it" is correct, mechanically

The owner's sentence is not an aesthetic complaint. Today's flow has **five** load-bearing
omissions, and each one alone would make it unusable at four-to-six panes. Together they mean
the feature can only be exercised by a human who already knows a specific pane needs review, is
willing to lose the rest of the fleet behind a modal popup while they do it, and will then walk
to that pane and press Enter.

1. **There is no review moment.** `prefix r` is the only entry (cockpit.tmux:246-250) `[E]`. The
   plugin already computes the exact signal a review moment needs — `Stop` → `@panestate done`
   (examples/claude-hooks.json), rendered `✓ green "finished, you have not picked it back up"`
   (README.md:26) `[E]` — and never connects it to review. "Finished, you have not picked it back
   up" *is* "needs review" with the wrong label.

2. **The diff base is working-tree-only, and silently so.** `lines_mode` runs
   `git diff` and falls back to `git diff --cached` (scripts/tmux-review:88-92); `fleet_rows`
   admits a pane to the queue only if `git status --porcelain` is non-empty
   (scripts/tmux-review:582) `[E]`. Consequence: **an agent that commits its work vanishes from
   the review queue entirely.** The one thing a well-behaved agent does at the end of a turn is
   the thing that makes it un-reviewable. Anthropic's own local `/code-review` defaults to "your
   branch's commits ahead of its upstream **plus** any uncommitted changes" `[E]`; revdiff's
   plugin auto-detects across four cases (main+dirty → uncommitted; main+clean → last commit;
   feature+clean → vs main; feature+dirty → *asks*) `[E]`.

3. **There is no reviewed state.** Every open rebuilds every row from scratch
   (scripts/tmux-review:280-315). Review pane 3 today, look again in ten minutes, and you re-read
   the same 200 lines to find the 12 that are new. revdiff solves this with `Space` = mark file
   reviewed, and — the important half — the mark **survives a rebase that only shifts line
   numbers but dies when the file's effective text changes** `[E]`. GitHub's `Viewed` checkbox is
   the same rule: "If the file changes after you view the file, it will be unmarked as viewed"
   `[E]`.

4. **The loop does not close.** `^S` delivers, `review_pane` deletes the store
   (scripts/tmux-review:630-645) `[E]`, and nothing records that a review happened, what it said,
   or whether the agent addressed it. There is no "re-review only the delta", no "approved", and
   no way for the *second* review to know about the first.

5. **Delivery costs a context switch per pane.** The batch lands typed-but-unsent
   (scripts/tmux-review:558-561) `[E]`. For one agent that is a defensible safety stance. For six
   it is six popup closes, six pane switches, six Enters — and the human has to remember which
   panes are carrying an un-sent paste. This is the single most-cited reason the feature is not
   used day to day `[I]`.

Two further usability faults specific to WHEN:

6. **The fleet queue is unreachable from an agent pane.** `main_entry` reviews the current pane
   when it is an agent, and only opens the queue otherwise (scripts/tmux-review:705-722) `[E]`. So
   the fleet-wide triage view — the plugin's genuinely unclaimed capability — requires you to
   first navigate *away* from every pane it is about.

7. **The popup is modal and client-owned.** `display-popup -E` (cockpit.tmux:249) `[E]`. While you
   review pane 3, panes 1/2/4/5/6 are invisible and a permission prompt on any of them cannot be
   answered. revdiff ships `REVDIFF_TMUX_WINDOW=1` to escape exactly this class of problem —
   a dropped client tears down a client-owned popup and kills the review mid-flight `[E, landscape
   doc]`. In a fleet plugin the modal review is a self-inflicted denial of the fleet.

And one likely live defect found while reading, relevant to HOW delivery works:

8. **`deliver()` never checks `@panestate`.** Its guards are pane-id shape, the agents glob, and
   store non-emptiness (scripts/tmux-review:471-497), re-checked at paste time by `if-shell -F`
   on the glob only (:558) `[E]`. A pane sitting on a permission prompt (`@panestate needs-input`)
   passes every guard. The review is pasted into a modal dialog, and the human's Enter — the one
   the display-message tells them to press (:560) — is consumed by the dialog as an **approval of
   whatever tool was being asked about**, not as a send of the review `[I: mechanism is clear from
   the code; not reproduced live here]`. The repo's own antipattern doc already scoped delivery to
   "`@panestate` not running" and named the guard set as "`%N` pane id, agents glob match,
   `@panestate` not running" — the third guard was specified and not implemented `[E,
   docs/superpowers/research/2026-08-24-review-antipatterns.md]`.

---

## 1. WHEN should a review happen?

### 1.1 What the community actually converges on

The single clearest finding: **the community has automated the *plan* gate and left the *code*
gate manual or agent-initiated.** Two independent, well-starred projects block on plans; the same
projects do *not* block on diffs.

- **plannotator (7,985★, pushed today)** intercepts `ExitPlanMode` via a **`PermissionRequest`**
  hook with a 345,600-second timeout, opens a UI, and returns approve-or-annotations as the
  hook's decision `[E]`.
- **revdiff-planning (part of revdiff, 807★)** intercepts `ExitPlanMode` via **`PreToolUse`**,
  launches the TUI, and **returns the annotations as the deny reason**, blocking plan
  finalisation until revised. The loop then runs on a **rolling diff: the new plan against the
  previous snapshot** `[E]`.
- The same revdiff project's **code** review (`/revdiff`) is a **skill the human or agent
  invokes**, with no automatic trigger `[E — the plugin README describes no code-review hook, only
  the planning one]`.
- **hunk (8,711★, pushed today)** never blocks at all: the TUI registers with a loopback daemon,
  and the agent *polls* it (`hunk session review --json`) and drives navigation itself `[E]`.

Where the code gate *is* automated, it is at the **turn boundary via `Stop`**:

- **claude-review-loop (722★)**: the `Stop` hook fires when Claude tries to exit, "writes a runner
  script and prompt file, transitions to `addressing`, **blocks exit**"; Codex writes a
  consolidated review to `reviews/review-<id>.md`; Claude reads and addresses it `[E]`. Note the
  explicit phase machine (`task` → `addressing`) persisted in a file.
- **claude-human-review (15★)**: "a stop-hook gate blocks Claude from ending any turn that edited
  files until a review is shown" `[E]`. Low stars — cite the *pattern*, not the project.

And Anthropic's own managed product ships a **three-mode trigger matrix, configurable per repo**,
which is the most direct evidence that no single trigger is right `[E]`:

| mode | when | cost note from the docs |
|---|---|---|
| Once after PR creation | PR opened / marked ready | runs once per PR |
| After every push | every push; **auto-resolves threads when you fix flagged issues** | "runs the most reviews and costs the most" |
| Manual | `@claude review` comment | "useful for high-traffic repos" |

Two more things in that doc are directly about fatigue and belong in this design `[E]`:
- **"Re-review convergence"** is a named, recommended `REVIEW.md` rule: *"after the first review,
  suppress new nits and post Important findings only"* — explicitly to stop "a one-line fix from
  reaching round seven on style alone."
- **Nit volume caps**: *"report at most five nits, mention the rest as a count in the summary."*

Nobody in this landscape gates on `git commit`. That is open territory `[I]`.

### 1.2 Trigger matrix for a 4-6 pane fleet

| # | trigger | mechanism available here | blocks the agent? | fatigue cost at 6 panes | verdict |
|---|---|---|---|---|---|
| a | **turn boundary — agent reports done** | `Stop` hook (already wired to `agent-state done`) → set a `review pending` flag | **no** (flag only) | 1 marker per finished turn; you choose when to act | **ship as the default.** This is the missing review moment. |
| a′ | turn boundary, **blocking** | `Stop` hook exit 2 with the pending review as the reason | yes | agent stalls until a human arrives; at 6 panes that is 6 stalled agents | **opt-in only** (`@cockpit_review_gate on`). Matches claude-review-loop `[E]` but is exactly the approval-fatigue trap at fleet scale `[I]` |
| b | **before commit** | `PreToolUse` matcher `Bash`, `if: "Bash(git commit *)"` — the docs' own example shape `[E]` | yes, with `permissionDecision: deny` + reason | one gate per commit, not per turn — far rarer than (a) | **ship as opt-in, high value.** Commit is where un-reviewed code becomes durable. Nobody does this `[I]` |
| c | **agent asks** | a skill the agent runs that blocks on the popup; the popup's exit code is the tool result | yes, but the agent chose it | zero unsolicited | **ship**, cheap: it is `tmux-review --loop` with stdout instead of a paste. This is revdiff's and difit's whole model `[E]` |
| d | **on-demand (today)** | `prefix r` | no | you must *remember*, and you have no idea which pane | **keep, demote.** Necessary, insufficient. It is the "Manual" row of Anthropic's matrix, shipped alone `[E]` |

**Recommended mix: a (default, non-blocking) + d (always) + b (opt-in) + c (opt-in) + a′ (opt-in,
documented as fleet-hostile).**

The reason (a) must be non-blocking by default is the plugin's own invariant — *hooks never block
the agent that fired it* (scripts/agent-state:33-35 states this explicitly) `[E]` — and, separately,
because six blocked agents is worse than six unreviewed ones. Non-blocking (a) converts review
from "remember to look" into "the fleet tells you", which is precisely the plugin's existing thesis
applied to a new state.

### 1.3 The new attention state

`review pending` must **not** be a fifth value of `@panestate`. That option's ranking
(`needs-input > error > running > done`) is load-bearing for the sticky guard
(scripts/agent-state:109-123) and for the `@cockpit_win_needs` derivation (:181-189), and the
README documents the ranking as a contract (README.md:88-95) `[E]`. Adding a value there means
re-deriving both.

Use a **separate per-pane option**, `@cockpit_review`, rendered as an *additional* marker beside
the existing one — the same way `@panestate_ctx` already appends `◔NN%` to the tab
(examples/tmux.conf:42) `[E]`. Urgency order in the fleet counter: `▲ blocked` > `✕ error` >
`◆ review` — an agent waiting on you outranks one waiting to be read.

---

## 2. WHAT is the diff base?

### 2.1 The rule the tools converge on

revdiff's four-case auto-detect is the best-tested version and it is worth copying almost verbatim
`[E]`:

| repo state | what gets reviewed |
|---|---|
| on main/master, dirty | uncommitted changes |
| on main/master, clean | the last commit |
| on a feature branch, clean | branch diff vs main |
| on a feature branch, dirty | **it asks**: uncommitted only, or the full branch diff |

Anthropic's `/code-review` collapses that to one default: *branch commits ahead of upstream +
uncommitted* `[E]`. `ultrareview` uses *current branch vs default branch + uncommitted + staged*
`[E]`.

Today's plugin implements none of these: it is case 1 only, and case 3 (the common one for a
worktree-per-agent fleet) reads as "no uncommitted changes" and the pane is dropped from the queue
entirely (scripts/tmux-review:582) `[E]`.

### 2.2 How tools avoid re-showing reviewed hunks

Three distinct mechanisms exist, and the good one is content-keyed, not position-keyed:

1. **revdiff `Space` = mark file reviewed.** On reload, "keeps the mark only when the file's
   **effective text diff** is unchanged; rebases that only shift line numbers or surrounding
   context keep it, while changed or removed files lose it" `[E]`. This is the right invalidation
   rule: it survives the noise (line drift) and fails on the signal (content change).
2. **GitHub `Viewed`**: "If the file changes after you view the file, it will be unmarked as
   viewed" `[E]`. Same rule, coarser. GitHub has **no** "show only changes since my last review"
   view — I checked the review docs specifically and it is absent `[E]`. So the industry's most-used
   review tool solves this with per-file marks, *not* with a second diff base.
3. **A rolling snapshot base.** revdiff-planning diffs "new vs previous snapshot" on each loop
   iteration `[E]`. This is the true delta base, and it is used for *plans*, where the artifact is
   a single document.

**Recommendation for this plugin: implement (3) with a git ref, and get (1) for free from it.**
A reviewed-checkpoint tree object gives you an exact `git diff <checkpoint> <worktree>`, which
*is* "only what changed since I reviewed" at line granularity — strictly better than per-file
marks, and cheaper to implement than a mark store because git already does the comparison.

### 2.3 The proposed base resolution (one function, four cases)

```
review_base(dir):
  1. refs/cockpit/reviewed/<key> exists and is an ancestor-or-tree of the current state
        -> base = that ref             # "since my last review" — the delta
  2. else upstream exists (git rev-parse --abbrev-ref @{u})
        -> base = git merge-base @{u} HEAD   # branch work + uncommitted
  3. else default branch exists (origin/HEAD, else main/master)
        -> base = git merge-base <default> HEAD
  4. else
        -> base = HEAD (today's behaviour: working tree only)
```
Rendered in the popup title so it is never ambiguous: `review · repo [since last review]` /
`[vs main]` / `[uncommitted]`. `prefix r` again inside the popup, or a `^B` key, cycles the base —
this is revdiff's "it asks" case 4, expressed as a key rather than a prompt `[I]`.

### 2.4 Where the checkpoint lives — and the invariant that blocks it

**The invariant "state dies with the pane" blocks this, and it should be relaxed — narrowly.**

The brief proposes `refs/cockpit/reviewed/<pane>`. Keying on pane id is the wrong lifetime in both
directions: tmux recycles `%N`, so a ref outlives its pane and can be silently mis-attributed; and
the common real case — pane killed, agent restarted on the same branch, same worktree — *loses*
the checkpoint precisely when you most want it.

The checkpoint is a fact about **the code**, not about the pane. Propose:

```
refs/cockpit/reviewed/<branch-slug>        # written with: git update-ref
```

stored **inside the repo it describes**. This:

- keeps every *attention* signal in tmux options, dying with the pane, exactly as today
  (invariant fully preserved for `@panestate`, `@panestate_msg`, `@cockpit_win_needs`,
  `@cockpit_fleet`);
- adds **no daemon, no server, no database, no polling** — one `git update-ref` per delivered
  review, ~1 ms, from a one-shot script;
- is garbage-collected by the tools that already own that lifetime: `git worktree remove`,
  `git branch -D`, and `git gc` on an unreferenced tree;
- is inspectable and deletable by hand (`git update-ref -d`), which "a file in `$XDG_RUNTIME_DIR`"
  is not.

Note the repo **already** writes state that outlives the pane: the >2048-byte spillover file
(scripts/tmux-review:515-527), whose comment says "the file outlives the pane on purpose; the OS
tmpdir is the only GC" `[E]`. So the invariant is already relaxed once, in the least defensible
direction (an untracked temp file with no GC). Replacing that with a git ref is a net *tightening*.

Purist fallback if the relaxation is refused: store the checkpoint sha in a per-pane option
(`@cockpit_reviewed_sha`) and accept that it is lost on pane death. Cost: the delta base is
unavailable exactly in the restart case; you fall back to case 2 (branch base), which is still far
better than today. State this cost out loud rather than pretending the pure version is equivalent.

---

## 3. HOW the loop closes — the state machine

### 3.1 Diagram

```
   ┌────────────────────────────────────────────────────────────────────┐
   │                                                                    │
   │                    ┌──────────┐                                    │
   │        PostToolUse │          │ (edits)                            │
   │       ┌───────────▶│  dirty   │                                    │
   │       │            │  (no ◆)  │                                    │
   │  ┌────┴────┐       └────┬─────┘                                    │
   │  │  clean  │            │ Stop  [delta vs base non-empty]          │
   │  │   ✓     │            ▼                                          │
   │  └────▲────┘   ┌──────────────────┐                                │
   │       │        │  review-pending  │  ◆  ← the new attention state  │
   │       │        │  @cockpit_review │     (counted in #{agent_fleet})│
   │       │        │     = pending    │                                │
   │       │        └────┬──────┬──────┘                                │
   │       │  prefix r / │      │ PreToolUse git commit  (opt-in gate b)│
   │       │  prefix J   │      └──────────▶ deny + "review first"      │
   │       │             ▼                        │                     │
   │       │      ┌─────────────┐                 │                     │
   │       │      │ review-open │◀────────────────┘                     │
   │       │      │  (fzf/TUI)  │                                       │
   │       │      └──┬───┬───┬──┘                                       │
   │       │    esc  │   │   │  ^S = deliver                            │
   │       │  (keep) │   │   └──────────────┐                           │
   │       │         │   │ ^A = approve     │                           │
   │       │         ▼   │                  ▼                           │
   │       │  review-pending          ┌──────────────┐                  │
   │       │  (store persisted)       │  addressing  │  ◆→ (dim)        │
   │       │         │                │ = delivered  │                  │
   │       │         │                └──────┬───────┘                  │
   │       │         │                       │ agent's next Stop        │
   │       │         │                       ▼                          │
   │       │         │              ┌──────────────────┐                │
   │       │         │              │ re-review-pending│ ◆              │
   │       │         │              │ base = checkpoint│  (delta only)  │
   │       │         │              └────────┬─────────┘                │
   │       │         └────────────────────────┘                         │
   │       │                                                            │
   │       │   ^A approve:  git update-ref refs/cockpit/reviewed/<key>  │
   │       └───────────────  unset @cockpit_review  ────────────────────┘
   └────────────────────────────────────────────────────────────────────┘
```

Five states, three of them just an option value. The whole machine is:

| state | representation | set by | cleared by |
|---|---|---|---|
| dirty | (implicit — no option) | — | — |
| review-pending | `@cockpit_review = pending` (pane opt) | `Stop` hook, if delta non-empty | approve, or delta going empty |
| review-open | the popup being alive | `prefix r` | popup exit |
| addressing | `@cockpit_review = sent` (pane opt) | `^S` delivery | the agent's next `Stop` |
| approved / clean | `refs/cockpit/reviewed/<key>` at the current tree, option unset | `^A` | any further edit that moves the delta |

The comment store moves from `mktemp` + delete-on-exit (scripts/tmux-review:630-645) `[E]` to a
per-pane spool that survives the popup: `$XDG_RUNTIME_DIR/cockpit-review-<key>.md`, 0600, the same
`umask 077 && mktemp` discipline already in place at :515-527. `esc` keeps it (state stays
`pending`); an explicit `^X` discards with a confirm. This is revdiff's rule exactly: history is
saved on signal termination and on annotated quit, and **only** the explicit discard-quit `Q`
saves nothing `[E]`.

### 3.2 "Only the delta" on re-review

Two layers, both cheap:

1. **Base = the checkpoint** (§2.3 case 1) — the diff itself contains only new lines. This is the
   whole mechanism; nothing else is needed for correctness.
2. **Carry the previous review forward, dimmed.** The spool is not deleted at delivery, so on
   re-open a row whose `## path:line (+)` header still matches a prior comment renders `↺` instead
   of `●` — "you said this last round, it is still here". `rows_mode` already builds exactly this
   set lookup (scripts/tmux-review:288-300) `[E]`; it costs one more marker character.
   This is Anthropic's "re-review convergence" rule made visible rather than prompt-engineered
   `[E]`.

---

## 4. HOW delivery should work — the honest ranking

Four options, ranked by day-to-day usability, not purity.

### #1 — Hook-injected `additionalContext` at the agent's own `Stop`. **Ship this.**

The `Stop` hook's documented output includes `additionalContext` ("Add context for next turn") and
it **can block** with exit 2 to prevent Claude stopping and continue the conversation `[E, hooks
docs]`. So:

```
Stop hook → scripts/review-deliver
   if spool for this pane has undelivered comments:
        emit them as additionalContext  (and, if @cockpit_review_gate on, exit 2)
        mark spool delivered; @cockpit_review = sent
   else: exit 0 silently
```

Properties that matter: **zero keystrokes, zero pane switches, zero focus changes, no paste, no
`send-keys`, no Enter.** It lands at a turn boundary by construction — the hook *is* the turn
boundary — so the repo's rule 6 ("deliver at turn boundaries, never send Esc, never auto-submit
into `running`") is satisfied structurally rather than by a guard that can go stale. It works even
when you write the review while the agent is mid-turn: the review simply waits for the next
`Stop`. This is claude-review-loop's exact mechanism, at 722★ `[E]`.

Cost: it only works when the cockpit hooks are installed (`install.sh --hooks`), which is already
the plugin's stated prerequisite — "These are the ONLY thing that writes state" `[E,
examples/claude-hooks.json]`. Requires one new hook entry on `Stop` (a second command alongside
`agent-state done`; the merge in install.sh:170-179 already appends per-event and is additive) `[E]`.

Latency caveat: if the agent is idle at a prompt, `Stop` has already fired and the injection waits
for the *next* turn. Mitigate by falling back to #2 when `@panestate = done` — the review is
delivered as a paste *and* left in the spool, whichever lands first wins `[I]`.

### #2 — Unsent paste (today). **Keep as fallback only.**

Correct and safe; unusable as the primary path at fleet scale (§0.5). Two fixes it needs
regardless, because it will remain the no-hooks path:

- **Add the `@panestate` guard** to both the early check and the `if-shell -F` re-check
  (scripts/tmux-review:471-497, :558) — refuse to paste into `needs-input`. See §0.8.
- Drop the 2048-byte branch (:515) as a *behavioural* fork. Always spool to the file; paste either
  the content or the pointer, but keep one code path so the delivered artifact is the same object
  the hook path reads.

### #3 — Auto-submit at a safe boundary (`paste` + `send-keys Enter`). **Do not ship.**

It is *defensible*: the repo's own antipattern doc scopes the prohibition to "never auto-submit
into `running`" `[E]`, so an Enter gated on `@panestate = done`, re-checked inside the same tmux
command-queue item by `if-shell -F` (the pattern already at :558), is within the letter of the
research. But once #1 exists it buys nothing and carries a real hazard: an Enter that races a
state change lands in a permission dialog as a blind approval (§0.8). Skip it.

### #4 — The agent pulls (a skill running `tmux-review --pull`). **Ship as a complement, not a path.**

hunk's model — the agent polls the session and drives it `[E]` — works because hunk's users are
running an agent that was *told* to review. It cannot be the delivery mechanism for a human-
initiated review of a pane that does not know it is being reviewed. Worth shipping as the
implementation of trigger (c): a skill that runs the popup in the foreground and reads its stdout,
revdiff-style `[E]`.

---

## 5. Reviewer-first-pass: should an AI pre-annotate?

**Yes. This is the strongest convergence in the whole landscape.**

- **Anthropic's Code Review** runs "multiple agents ... in parallel, each looking for a different
  class of issue, then a **verification step** checks candidates against actual code behavior to
  filter out false positives. The results are **deduplicated, ranked by severity**, and posted as
  inline comments on the specific lines" `[E]`. Severities: 🔴 Important / 🟡 Nit / 🟣 Pre-existing.
  Findings are **not blocking** — "don't approve or block your PR, so existing review workflows
  stay intact" `[E]`.
- **`/code-review` locally** runs as a background subagent with its own context window, so it
  doesn't fill the conversation, and reports through a `ReportFindings` list marked
  fixed / skipped / no-change-needed on re-report `[E]`. That is a per-finding lifecycle, which is
  exactly the state this plugin is missing.
- **Conductor** exposes an in-process MCP server so Claude annotates its own diff before the human
  opens it `[E, landscape doc]`. **difit** does the same without MCP via repeatable `--comment`
  JSON `[E, landscape doc]`. **hunk** labels notes by source — "Human and agent notes are labeled
  by source" `[E]`.

So the pattern is uniform: **the AI pass produces *flagged rows*, labelled as AI-authored, ranked
by severity, non-blocking; the human reviews the flags and the rest of the diff is available but
not demanded.**

### Concrete shape here

At the `review-pending` transition, if `@cockpit_review_ai on`:

```
tmux run-shell -b '<dir>/scripts/review-annotate <dir> <spool>'
   -> claude -p '/code-review' --output-format text   (or a project-configured command)
   -> parse findings into the SAME revdiff grammar already in use:
          ## path:line (+)
          > quoted fragment
          [ai:important] the finding text
```

The picker then sorts `[ai:important]` rows first (ahead of even the test-file partition at
scripts/tmux-review:212-219), marks them `⚑`, and the human's `alt-↵` on such a row either edits or
endorses it. The AI's own rows are never delivered unless the human leaves them in the batch — the
human gate stays structurally unskippable, which the repo already lists as a rule `[E,
antipatterns #7]`.

Three cost caveats, stated plainly:

- **It is a second model call per turn.** Anthropic quotes $15-25 and ~20 min for the managed PR
  product `[E]`; local `/code-review` is far cheaper but still a background subagent. Default
  **off**, opt-in per-repo, and run it on the *pending transition*, not on popup open.
- **Never block the popup on it.** `prefix r` must open instantly whether or not the annotate run
  has landed. If it lands later, the rows appear on the next `reload()` — the picker already
  reloads after every comment (scripts/tmux-review:411) `[E]`.
- **Cap the nits.** Anthropic's own guidance is "report at most five nits, mention the rest as a
  count" `[E]`. Ship that cap as the default prompt, or the AI pass reintroduces the fatigue the
  whole design is trying to remove.

---

## 6. Concrete design for this plugin

### 6.1 Hook events

| event | matcher | command | purpose | new? |
|---|---|---|---|---|
| `Stop` | `""` | `agent-state done` | existing | no |
| `Stop` | `""` | `review-gate stop` | set `@cockpit_review pending` if delta ≠ ∅; deliver spooled comments as `additionalContext`; optionally exit 2 | **yes** |
| `PreToolUse` | `Bash`, `if: "Bash(git commit *)"` | `review-gate commit` | opt-in gate (b): `permissionDecision: deny` with reason when `@cockpit_review = pending` | **yes, opt-in** |
| `PostToolUse` | `""` | `agent-state running` | existing; also clears `sent` → nothing (agent is working on it) | no change |
| `SessionEnd` | `""` | `agent-state off` | existing; also unsets `@cockpit_review` | small change |

`install.sh --hooks` merges additively per event and already recognises a reformatted settings.json
without re-appending (install.sh:159-190) `[E]`, so adding a second `Stop` entry is in-contract.
The `if:` condition form on a `Bash` matcher is the hooks docs' own example `[E]`.

Hard constraint carried through: `review-gate` must obey scripts/agent-state's discipline — bounded
`read -t` on stdin, no `jq` dependency on the hot path, silent no-op on anything unparseable
(scripts/agent-state:33-45, 75-88) `[E]`. A wedged hook blocks the agent that fired it.

### 6.2 tmux options

| option | scope | values | written by | read by |
|---|---|---|---|---|
| `@cockpit_review` | pane | `pending` \| `sent` \| (unset) | `review-gate`, `tmux-review` | tab marker, border, `#{agent_fleet}`, `tmux-jump` |
| `@cockpit_review_n` | pane | integer: changed lines in the delta | `review-gate` | tab suffix (optional), queue sort |
| `@cockpit_reviews` | global | precomputed `◆N` fleet segment | `review-gate` | `#{agent_fleet}` |
| `@cockpit_review_gate` | global | `off` (default) \| `on` | user | `review-gate` |
| `@cockpit_review_ai` | global | `off` (default) \| `on` \| a command | user | `review-annotate` |
| `@cockpit_review_base` | global | `auto` (default) \| `worktree` \| `branch` | user | base resolution |

**Freeze discipline is mandatory here.** `@cockpit_reviews` must be precomputed at transition time
in `review-gate`, exactly as `@cockpit_fleet` is at scripts/agent-state:191-213, with the same
"compare-then-set" guard so an unchanged value costs no redraw `[E]`. A live
`#{S:#{W:#{P:}}}` walk for a third counter would re-introduce the freeze the repo just fixed
(scripts/agent-state:149-180) `[E]`. Same for a per-window `@cockpit_win_review` if the banner is
extended.

### 6.3 Keys

| key | where | action | change |
|---|---|---|---|
| `prefix r` | anywhere | **always** the fleet queue, with the current pane preselected if it is an agent | **changed** — removes the "navigate away first" precondition (§0.6) |
| `↵` | queue | review the selected pane | unchanged |
| `prefix J` | anywhere | jump cycle — extend to include `◆ pending` panes **after** all `▲` ones | scripts/tmux-jump:14-16 gains a second pass `[E]` |
| `alt-↵` | picker | attach comment | unchanged |
| `^S` | picker | deliver the batch → `sent` | unchanged |
| `^A` | picker | **approve**: `git update-ref refs/cockpit/reviewed/<key>`, unset `@cockpit_review` | **new** |
| `^B` | picker | cycle the diff base (`since last review` → `vs branch` → `uncommitted`) | **new** |
| `↵` | picker | **open the hunk** around the row (drill-in), not a no-op | **changed** (§7.4) |
| `esc` | picker | leave, **keeping** the spool; state stays `pending` | **changed** (§7.3) |
| `^X` | picker | discard the batch, with a confirm | **new** |

`^A` is the piece that makes the whole machine terminate. Without an approve there is no clean
state, and every pane stays `◆` forever — which is the fatigue failure in its purest form.

### 6.4 Non-modal review

Add `@cockpit_review_window on`: open the review in a real tmux window instead of
`display-popup -E`, so the fleet's borders and tabs stay visible while you read. revdiff ships the
same escape hatch and the landscape doc already argued it should be the default for a
long-lived-pane plugin `[E]`. Delivery already runs via `tmux run-shell -b` to survive popup
teardown (scripts/tmux-review:636-641) `[E]`, so the window path needs no new plumbing — it is
`new-window` instead of `display-popup` at the binding site (cockpit.tmux:249).

---

## 7. What to drop from today's design

1. **Working-tree-only as the sole base** (scripts/tmux-review:88-92, :582). It makes a committing
   agent invisible. Replace with §2.3.
2. **One row per changed line as the primary view** (scripts/tmux-review:150-198). No tool in the
   landscape reviews line-by-line with zero context; revdiff is tree+diff, difit is files-changed,
   hunk navigates *hunks* with `{`/`}` `[E]`. The 5000-row cap (:112-124) is the symptom: the row
   model didn't scale and the fix was a refusal to render. Rows should be **hunks** (drill into
   lines with `↵`), which also retires the bash row-build that has already needed two performance
   rescues (:228-234 O(n²) string append; :289-296 per-row `grep -qxF`) `[E]`.
3. **`esc` discarding the whole store** (:392-396, :630-645). Losing a twelve-comment review to a
   stray keypress is a cliff. revdiff persists on signal termination and discards only on explicit
   `Q` `[E]`.
4. **"Plain `↵` does nothing"** as a permanent stance (:404). Keep *no accept-all* — that argument
   is sound — but a dead primary key reads as a broken picker. Make `↵` drill into the hunk.
5. **The 2048-byte delivery fork** (:515-527) as two behaviours. One spool, always; the paste
   carries content or a pointer, but the artifact is identical either way.
6. **The mktemp-and-delete store lifetime** (:630-645). The spool must outlive the popup for the
   loop to close at all.

---

## 8. Ranked recommendations

Each: **what** / **why** / **cost** / **what it replaces**.

**R1 — `Stop`-hook review moment: a `◆ review pending` attention state.** (cost **M**)
New `scripts/review-gate` on `Stop`, sets `@cockpit_review pending` when the delta vs the base is
non-empty; precomputed `@cockpit_reviews` counter; a fifth marker in the tab and the fleet segment.
*Why:* it is the entire answer to "when". The plugin already knows the turn boundary and already
renders `done ✓` as "you have not picked it back up" (README.md:26) — this names it correctly and
makes the fleet tell you. Convergent with claude-review-loop `[E]` and with Anthropic's own
"trigger per repo" model `[E]`.
*Replaces:* on-demand-only entry; the human's memory as the trigger.

**R2 — Hook-injected delivery via `additionalContext` at `Stop`.** (cost **M**)
Spool comments per pane; `review-gate` emits them into the agent's next turn.
*Why:* removes the pane switch and the Enter — the single biggest day-to-day blocker (§0.5). Lands
at a turn boundary structurally, so no guard can go stale. 722★ precedent `[E]`.
*Replaces:* the unsent paste as the primary path (it stays as the no-hooks fallback).

**R3 — Diff base resolution + the reviewed checkpoint ref.** (cost **M**)
`review_base()` per §2.3; `^A` writes `refs/cockpit/reviewed/<branch-slug>`.
*Why:* fixes the "a committing agent is invisible" hole, gives "only the delta" for free, and
terminates the state machine. Requires the narrowest possible relaxation of "state dies with the
pane" — and replaces an *existing*, worse relaxation (the ungarbage-collected spillover file at
:515-527) `[E]`.
*Replaces:* `git diff` / `git diff --cached` as the only base; the absence of an approve.

**R4 — Fix `deliver()`'s missing `@panestate` guard.** (cost **S**)
Refuse to paste into `needs-input`, in both the early check and the `if-shell -F` re-check.
*Why:* today's Enter-to-send instruction can be consumed by a permission dialog as a blind
approval (§0.8). The repo's own research already specified this guard and it was not implemented
`[E]`.
*Replaces:* a two-guard delivery with a three-guard one. Do this first; it is a bug, not a redesign.

**R5 — `prefix r` always opens the fleet queue, current pane preselected.** (cost **S**)
*Why:* the fleet queue is the plugin's unclaimed capability and it is currently gated behind
"first go somewhere that isn't an agent" (scripts/tmux-review:705-722) `[E]`. One key, one mental
model.
*Replaces:* the two-entry-point resolution in `main_entry`.

**R6 — Hunks as the review unit; lines as a drill-in.** (cost **L**)
*Why:* every surviving tool is hunk-or-file first `[E]`; the 5000-row cap is an admission that the
line model doesn't scale; it retires the two hottest bash loops in the file.
*Replaces:* `lines_mode`'s row model, the 5000-row refusal, and the dead `↵`.

**R7 — `PreToolUse` gate on `git commit` (opt-in).** (cost **M**)
`if: "Bash(git commit *)"` → deny with "a review is pending on this worktree".
*Why:* commit is where un-reviewed code becomes durable, it is far rarer than a turn boundary (so
it does not cause approval fatigue at six panes), and **nobody in the landscape does it** — the
plan gate is taken, the commit gate is not `[I]`.
*Replaces:* nothing; it is additive, and the highest-leverage *blocking* trigger available.

**R8 — Persist the spool; `esc` keeps, `^X` discards with confirm.** (cost **S**)
*Why:* revdiff's rule `[E]`; removes the "lost the whole review" cliff; and it is the prerequisite
for R2 and for delta re-review.
*Replaces:* mktemp-and-delete-on-exit.

**R9 — Non-modal review (`@cockpit_review_window`).** (cost **S**)
*Why:* a modal popup blinds you to the fleet the plugin exists to watch, and a dropped client kills
the review; revdiff ships exactly this escape hatch `[E]`.
*Replaces:* `display-popup -E` as the only chrome (keep it as the default).

**R10 — AI first pass, opt-in (`@cockpit_review_ai`).** (cost **M**)
`claude -p '/code-review'` at the pending transition, findings parsed into the existing revdiff
grammar with an `[ai:severity]` tag and a `⚑` marker, sorted first, nit-capped at five.
*Why:* the most uniform convergence in the landscape — Anthropic's own product, Conductor, difit,
hunk all pre-annotate and label the source `[E]`. It converts "read 300 lines" into "read 6 flags
plus whatever else catches your eye."
*Replaces:* nothing; strictly additive. Default off — it costs a model call per turn `[E]`.

**R11 — Blocking `Stop` gate (`@cockpit_review_gate on`), documented as fleet-hostile.** (cost **S**)
*Why:* it is the strongest guarantee and the exact approval-fatigue trap. Ship it because
single-agent users want it; default it off and say plainly in the README that at six panes it means
six stalled agents.
*Replaces:* nothing; the honest opt-in end of the trigger matrix.

---

## 9. Sources

Fetched and verified this session:
- https://code.claude.com/docs/en/hooks — hook events, input/output fields, `additionalContext`,
  exit-2 blocking, `PermissionRequest`, `if:` matcher example
- https://code.claude.com/docs/en/code-review — trigger matrix, severities, re-review convergence,
  nit caps, `/code-review` local scope and background-subagent behaviour, `ReportFindings`
- https://github.com/umputun/revdiff and https://revdiff.com/docs — base auto-detection, `Space`
  mark-reviewed and its content-keyed invalidation, annotation grammar, exit 10, history storage
- https://github.com/umputun/revdiff/blob/master/plugins/revdiff-planning/README.md — `PreToolUse`
  on `ExitPlanMode`, annotations as deny reason, rolling snapshot diff
- https://github.com/backnotprop/plannotator (+ plannotator.ai docs) — `PermissionRequest` on
  `ExitPlanMode`, 345600 s timeout
- https://raw.githubusercontent.com/hamelsmu/claude-review-loop/main/README.md — `Stop` hook blocks
  exit, `task`→`addressing` phase machine, consolidated review file
- https://raw.githubusercontent.com/modem-dev/hunk/main/docs/agent-workflows.md and
  https://www.hunk.dev/docs/agents/comments-and-annotations/ — loopback daemon session model, agent
  polls, human/agent note labelling, no blocking
- https://docs.github.com/.../reviewing-proposed-changes-in-a-pull-request — pending-review
  batching, `Viewed` checkbox auto-unmarking on change, **no** since-last-review view
- https://github.com/BloopAI/vibe-kanban — board-status review moment, inline comments, approve
  before merge
- https://github.com/IrtezaAsadRizvi/claude-human-review — stop-hook gate pattern (15★; pattern only)

Repo files read: `scripts/tmux-review` (743 lines), `scripts/agent-state`, `scripts/tmux-jump`,
`cockpit.tmux`, `install.sh`, `examples/claude-hooks.json`, `examples/tmux.conf`, `README.md`,
`docs/superpowers/research/2026-08-24-review-tools-landscape.md`,
`docs/superpowers/research/2026-08-24-review-antipatterns.md`.
