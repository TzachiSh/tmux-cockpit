# Lens: The Active Community Landscape (August 2026)

Scope: actively-maintained, well-starred GitHub projects that solve "a human reviews an AI
coding agent's changes and feeds comments back to the agent." Every star count and push date
below was verified with `gh api repos/OWNER/REPO` on 2026-08-24. Claims are tagged
**[E]** evidence (verified via API, README, issue text, or repo file) or **[I]** inference.

---

## 1. The verified field

### 1.1 Tier one — the projects that define the category

| Project | Stars | Last push | Created | Lang | What it is |
|---|---:|---|---|---|---|
| `herdrdev/herdr` | **32,024** | 2026-08-24 | 2026-03-27 | Rust | "the runtime your coding agents live on" |
| `modem-dev/hunk` | **8,711** | 2026-08-24 | 2026-03-17 | TypeScript | "Review-first terminal diff viewer for agentic coders" |
| `backnotprop/plannotator` | **7,985** | 2026-08-24 | 2025-12-28 | TypeScript | Annotate agent plans + diffs visually, send feedback back |
| `yoshiko-pg/difit` | **3,095** | 2026-08-09 | 2025-06-29 | TypeScript | Local web server, GitHub-like "Files changed" view |
| `stravu/crystal` | 3,109 | 2026-02-26 | — | TS | Parallel Codex/CC sessions in worktrees (**now renamed Nimbalyst; stale**) |
| `agavra/tuicr` | **2,868** | 2026-08-20 | 2026-01-08 | Rust | "A code review TUI with vim keybindings" |
| `umputun/revdiff` | **807** | 2026-08-20 | 2026-04-01 | Go | "TUI for reviewing diffs, files, and documents with inline annotations" |
| `persiyanov/herdr-reviewr` | **510** | 2026-08-23 | 2026-06-26 | Rust | Review sidebar for herdr: comment on agent's diff, send it back |
| `smarzban/herdr-file-viewer` | 467 | 2026-08-15 | — | — | Git-aware read-only file viewer for herdr |

[E] All figures from `gh api`. Adjacent orchestrators for context: `BloopAI/vibe-kanban`
27,904 (pushed 2026-04-24 — **going stale**), `smtg-ai/claude-squad` 8,361 (2026-08-20),
`sst/opencode` 200,961 (2026-08-24), `charmbracelet/crush` 27,641 (2026-08-24).

### 1.2 The single most important structural finding: herdr

[E] `herdrdev/herdr` has **32,024 stars, created 2026-03-27, pushed today**, and has spawned a
plugin ecosystem of *dozens* of independently-starred repos: `herdr-reviewr` (510),
`collie` (506), `herdr-file-viewer` (467), `herdr-browser` (340), `herdr-remote` (281),
`herdr-plus` (256), `herdr-worktrunk` (104), `herdr-plugin-jj-workspace` (43),
`herdr-plugin-workspace-manager` (35), `herdr-plugin-sesh` (29), plus `herdr-plugin-hunk`,
`herdr-hunk-diff`, `herdr-plannotator`, `herdr-reviewr` forks, and more.

[I] **This is the competitive fact that matters most for tmux-cockpit.** herdr occupies almost
exactly tmux-cockpit's niche — a multiplexer whose panes are agents, with plugin-provided
sidebars — and it grew from zero to 32k stars in five months. Its keybinding is `Ctrl+B`
(issue #3182 is titled "Ctrl+B prefix does nothing while the macOS Korean input method is
active" [E]), i.e. it is deliberately a tmux-shaped replacement. tmux-cockpit's differentiator
is not "review UI" — herdr-reviewr already ships one — it is *running inside real tmux*, with
no daemon, on a machine where the user already lives.

[I] The strategic read: cockpit should not try to out-feature herdr-reviewr's TUI. It should be
the best **thin, tmux-native shim onto the review tools that already won** (hunk, revdiff,
tuicr) — the same way `edmundmiller/herdr-plugin-hunk` (13) and `jhochenbaum/herdr-hunk-diff`
exist as herdr plugins rather than reimplementing hunk. See recommendation R1.

### 1.3 Negative space: nobody uses fzf for this

[E] `gh search repos "fzf diff review" --sort stars` returns **zero results**. Not one project
in the top of this field uses fzf as its review surface. Every tier-one tool is either a
purpose-built TUI (Rust: tuicr, revdiff-in-Go, herdr-reviewr; TS/OpenTUI: hunk) or a local web
server (difit, plannotator).

[I] This is not proof fzf is wrong, but it is a strong prior. fzf is a *picker*: one row is one
selectable item, the typed text is a filter. Review is not a picking problem — it is a *reading*
problem with an annotation overlay. Every project that started from "how do humans read diffs"
converged on a two-pane tree+diff layout; cockpit started from "what widget do I already have"
and got a flat line list. The `--disabled` hack at `scripts/tmux-review:385-392` is the tell:
the design has to *switch off fzf's only real feature* to work.

---

## 2. Mechanism-by-mechanism

### 2.1 revdiff (807, Go, 2026-08-20) — closest analog to cockpit, and it diverges on 4 axes

[E] Navigation: **two-pane TUI** — file tree left, syntax-highlighted diff right. Three
navigation levels: line (`j`/`k`), **hunk (`[`/`]`)**, file (`n`/`p`). Status bar shows filename,
diff stats, current hunk position, line number.

[E] Annotation grammar (essentially identical to cockpit's, which the brief says cockpit
borrowed):
```
## handler.go:43 (+) use errors.Is() instead of direct comparison
## handler.go (file-level) consider splitting this file into smaller modules
```
[E] Notable extras cockpit lacks: **file-level annotations**; `Ctrl+E` opens `$EDITOR` for
multi-line notes; and the keyword `hunk` in an annotation body auto-expands the anchor to the
full hunk range (`handler.go:43-67 (+)`).

[E] Delivery: **structured markdown on stdout by default**, `-o/--output` to a file. The Claude
Code plugin launches revdiff as a terminal overlay (tmux `display-popup`, kitty overlay, wezterm
split, Zellij floating, agterm `overlay open`, AppleScript for ghostty/iTerm2), and **treats exit
code 10 as "annotations captured," feeding stdout directly to Claude.**

[E] `REVDIFF_TMUX_WINDOW=1` opens revdiff in a **persistent server-owned tmux window instead of a
client-owned popup** — explicitly "disconnect-resilient," for when SSH drops mid-review.

[I] Four concrete divergences from cockpit: (a) revdiff has a file tree and hunk navigation,
cockpit has neither; (b) revdiff returns text to the *agent harness* via stdout, cockpit types
text into the *agent's TTY* via `paste-buffer`; (c) revdiff has an escape hatch from the modal
popup for flaky connections, cockpit's popup is unconditionally modal (`-E`); (d) revdiff supports
file-level and hunk-range comments, cockpit is line-only.

[E] Even revdiff's *flat list* is under pressure: issue #280 (open, 2026-07-19, with a ready PR)
— "Supporting tree structure instead of flat list in navigation pane," from a user who says
"Coming from an IDE perspective it's a bit hard to read and understand." Cockpit's list is
strictly flatter than the thing users are asking revdiff to abandon.

[E] revdiff #318 (2026-08-18) is a *lifecycle* bug worth internalizing: while revdiff runs
in-place via `spawnSync(..., {stdio:"inherit"})`, "a parked tool call is indistinguishable from
active work," so herdr's sidebar shows the agent as `working` for the whole review and
`herdr agent wait <pane> --until blocked` never fires. [I] Cockpit has the mirror-image of this
problem: the agent pane is genuinely idle during review, but nothing in cockpit's status marks
"a human is reviewing this pane right now," so a second reviewer (or the same human in another
window) has no signal.

### 2.2 hunk (8,711, TS, pushed today) — daemon + session API

[E] UI: "Multi-file review stream with sidebar navigation," split/stack/responsive auto layouts,
side-by-side diff with inline annotations. Positioned as "optimized for reviewing a full
changeset interactively." Watch mode auto-reloads as the working tree changes (fs events with
polling fallback).

[E] Architecture: "when a Hunk TUI starts, it registers with a **local loopback daemon**"
(default port 47657, `HUNK_MCP_PORT`). Agent-side commands:
- `hunk session review --repo . --json` — file structure + hunk layout, **without the full patch**
- `hunk session navigate --repo . --file <path> --hunk <n>` — moves the *live* window
- `hunk session comment add --repo .` / `comment apply --stdin` — batch comments as JSON
  (`filePath`, `summary`, plus one of `hunk`/`hunkNumber`/`oldLine`/`newLine`)
- `--agent-context notes.json` — sidecar of pre-written agent rationale shown beside the code

[E] Agent onboarding is a skill file: "Tell your agent to add the skill file returned by
`hunk skill path`."

[I] hunk's daemon is exactly what cockpit's invariants forbid ("no daemon/server/db"). But note
*what* the daemon buys: a two-way channel where the agent can **drive the human's cursor**
("navigate to file X hunk 3, here's why I did this"). Cockpit cannot do that, and shouldn't try —
but it's the frontier the top project is exploring.

[E] **hunk issue #843, opened 2026-08-24 — directly indicts cockpit's keymap:** `[keybindings]
"alt"/"option" chords never match on standard (non-Kitty) terminals`. "Any `[keybindings]` entry
written with the `alt`/`option` modifier is **silently unreachable** on a normal terminal. The key
never fires the bound command, and there's no warning/notice that the binding is dead."
Reproduced over SSH from a Windows console with `TERM=xterm-256color`.

[I] Cockpit's *entire* review interaction is alt-chords: `alt-enter` to comment, `alt-u`/`alt-d`
to scroll (`scripts/tmux-review:402-403`, README table). fzf's alt handling is more forgiving
than hunk's config parser, but the failure class is real — Alt is delivered as either ESC-prefix
or the 8th bit depending on terminal, and `Alt` is the Option/Meta key on macOS where many
terminals default to "Option as Meta = off." A cockpit user on default-configured macOS
Terminal.app or iTerm2 gets a review popup where **the only key that does anything is broken**,
with no error. This is the single highest-severity finding in this lens.

### 2.3 tuicr (2,868, Rust, 2026-08-20) — persistent sessions + JSON query API

[E] "GitHub-style continuous diff in the terminal. Scroll through every changed file in one
stream," with a **file tree sidebar**, hunk jumps (`[`/`]`), file jumps (`{`/`}`), and both
unified and `diff_view = "side-by-side"` modes.

[E] Comments at **four granularities**: line, range (visual mode `v`/`V`), file (`C`), and
review-level summary. "Review tracking at file or hunk granularity, **persisted across
sessions**." Reviewed-state toggles: `r`/`R`.

[E] Three exports: clipboard (`y`/`:clip`) as numbered structured markdown with file/line anchors;
`--stdout` piping; and `tuicr review` — "Exposes saved sessions **without opening the TUI**,
enabling agent and script integrations."

[E] The skill file (`skills/tuicr/SKILL.md`) is the most instructive artifact in the whole
landscape. It defines **two distinct workflows**: *user-led review* (agent retrieves the user's
comments) and *agent review* (agent critiques the patch and adds its own comments). The agent
detects the multiplexer and runs the matching wrapper:
`$CMUX_WORKSPACE_ID` → `tuicr-wrapper-cmux.sh`, `$TMUX` → `tuicr-wrapper.sh`,
`$ZELLIJ` → `tuicr-wrapper-zellij.sh`, `$HERDR_ENV=1` → `tuicr-wrapper-herdr.sh`.
Then it **polls** `tuicr review comments --repo <path> --session <slug>` roughly every 30s,
watching for new comment IDs; the JSON carries `id`, `location`, `path`, `content`, and
`comment_type` ∈ {issue, suggestion, note, praise}.

[I] Two things cockpit has no analog for: (a) **comment_type** — a typed severity/intent tag,
which is strictly more information than cockpit's `??`-means-question convention and costs one
extra keystroke; (b) a **durable, queryable comment store with stable IDs** that outlives the
review UI, so the agent can re-read the review after a compaction, and so a second review can
diff against the first.

[E] Community pain: #613 "Improve large diff rendering performance" — a user reviewing "~4000+
lines and/or 50+ files" reports "~5-10 seconds to render" because syntax highlighting runs on the
main thread for the *full* diff; the requested fix is "only syntax highlighting a **sliding window
of 3 screens** at a time" plus backgrounding. [I] This is the exact shape of cockpit's own
problem, and the exact shape of the fix cockpit needs (see R3): the top TUIs are all discovering
that whole-diff work on the render path is the bottleneck, and the answer is windowing.

### 2.4 plannotator (7,985, TS, pushed today) — hook-triggered, the only one that intercepts

[E] Two flows, quoted from the README:
- **Plan:** "Agent calls ExitPlanMode → **PermissionRequest hook fires** → Local server reads plan
  from hook input → Browser opens with review UI → You annotate and approve/deny."
- **Code:** "git diff captures changes (or PR fetched by URL) → Browser opens with diff viewer →
  Annotate lines, stage/unstage files → Send feedback: returned to agent session."

[E] Denial path: "structured feedback sent to agent → Agent revises, **plan diff shows what
changed**." Transport is the agent's existing hook infrastructure — "no separate MCP or external
files required." Supported harnesses: Claude Code, Codex, Copilot CLI, Gemini CLI, OpenCode, Kiro,
Droid, Amp, Pi.

[E] Community signal on triggering, from issue #1308: a user reports "I have a plugin/hook
installed that opens the plannotator-review on **every `git commit`** the agent triggers. The
agent is instructed to pro-actively commit his work in logical bits (just like a human developer
would do), so that I can follow the development process, while letting the agent work mostly in
full autonomy."

[E] Community signal on the review being a *conversation*, from #1287: "I want to **reply to an AI
agent review and/or 'approve/reject' its comments**." And #1308 asks for "direct edits in addition
to suggestions."

[I] plannotator is the only tier-one tool where review is **not on-demand** — it is *interposed*
on an agent lifecycle event, so the human cannot forget to review and the agent cannot proceed
past an unreviewed gate. That is a categorically different value proposition from cockpit's
`prefix r`. Cockpit's fleet queue is a partial answer (it surfaces *which* pane needs eyes) but
it is still pull, not push.

### 2.5 difit (3,095, TS, 2026-08-09) — browser surface, clipboard transport, and pre-seeding

[E] Local web server, GitHub-like Files-changed view; comment by clicking a line or dragging a
range; comments persist in **browser localStorage per commit**. Output via "Copy Prompt" /
"Copy All Prompt":
```
src/components/Button.tsx:L42
Make this variable name more descriptive
src/components/Button.tsx:L42-L48
This section is unnecessary
```

[E] Ships **two** skills: `difit` — "ask the user for a review through difit after code changes";
and `difit-review` — "review a specific diff or PR and **launch difit with findings or
explanations preloaded as comments**."

[I] `difit-review` is the clearest statement of the pre-seeded-self-review pattern: the agent
reviews its own diff first and the human arrives at a *pre-annotated* diff, triaging the agent's
findings rather than starting from a blank page. Combined with hunk's `--agent-context notes.json`
and tuicr's SKILL.md "agent review" mode, that's **three of five** tier-one tools shipping it.

[E] difit's cautionary tale, issue #448: "Review comments can be **silently lost** when the server
is stopped by SIGTERM / a process manager." [I] Cockpit's store is an `mktemp` file deleted on esc
and on any non-{0,1} fzf exit code (`scripts/tmux-review:414-417`), and `review_pane()` chains
`rm -f -- "$store"` after delivery (`:645`). A popup killed by a detach, a terminal resize crash,
or a stray `^C` loses the whole batch with no recovery. Same failure class, and cockpit has no
`localStorage` equivalent to fall back on.

### 2.6 herdr-reviewr (510, Rust, 2026-08-23) — the direct feature competitor

[E] "Your agent writes the code. You read its diff in a pane beside the chat, comment on the
lines, and send the notes back." **Four diff scopes**: uncommitted, branch changes, **last-turn
changes**, and specific commits. Syntax-highlighted; file navigation with per-file +/- stats.
Keys: `v` selection mode → arrows to extend → `c` comment box → `s` sends *all accumulated*
comments to the agent's input. Mouse and drag-select supported. Auto-opens in new worktrees.

[I] "Last-turn changes" is a scope cockpit does not have and probably should. Reviewing
*everything uncommitted* conflates the turn you're reviewing with three turns of prior accepted
work; every re-review re-presents lines you already blessed. tuicr's persisted reviewed-state
solves the same problem from the other end.

[E] Its open issues are a preview of cockpit's future bug list:
- **#66** "Sending comments to a kitty-keyboard-enabled Textual pane **loses formatting or drops
  the payload**" — i.e. *typing text into another program's TTY is fragile*, exactly cockpit's
  `paste-buffer -p -r -d` transport.
- **#63** "Polling re-walks the whole tree every 2s, even when idle" — `diff --numstat -z`,
  name-status diff, and `ls-files --others` per tick, plus a `snapshot_worktree` that "copies the
  real index to a temp index (a **multi-MB disk write per tick** on a large repo)."
- **#81** "Bidirectional communication"; **#51** "Distinguish staged vs unstaged changes in the
  uncommitted scope"; **#26** "Allow staging hunks / files directly from reviewr."

[I] #51 and #26 land on cockpit directly: `lines_mode` uses unstaged diff *else* staged
(`scripts/tmux-review:97-98`), an either/or that silently shows the wrong set the moment the agent
stages anything. herdr-reviewr's users explicitly asked for VS-Code-style split sections instead,
and a fork (`jhao0413/herdr-reviewr`) shipped it.

---

## 3. What is converging as best practice

Ranked by strength of consensus across the tier-one projects.

**C1. Two-pane file-tree + diff, with three-level navigation (line / hunk / file). [E] 5/5.**
revdiff (tree + `[`/`]` + `n`/`p`), tuicr (tree sidebar + `[`/`]` + `{`/`}`), hunk (sidebar +
multi-file stream), herdr-reviewr (Changes pane + diff), difit/plannotator (browser file tree).
Even revdiff's flat list is under an open request to become a tree (#280). **Cockpit has none of
this**: no tree, no hunks, no context lines, no file pane — one row per changed line
(`scripts/tmux-review:9-10`, README:473+).

**C2. Comments at multiple granularities, not just line. [E] 4/5.** tuicr: line/range/file/review.
revdiff: line + file-level + auto hunk-range expansion. difit: line + drag-range. herdr-reviewr:
`v` range selection. **Cockpit is line-only** — "One row per +/- line," and a comment can only
anchor to exactly one such row.

**C3. Structured, machine-readable output the harness consumes — not keystrokes into a TTY.
[E] 5/5.** revdiff: stdout markdown + exit code 10. tuicr: `--stdout`, clipboard, and
`tuicr review comments --json`. hunk: JSON over a session API. plannotator: hook return value.
difit: clipboard prompt block. **Not one tier-one tool types into another process's terminal.**
Cockpit's `paste-buffer -p -r -d` into the agent's prompt is unique in this field —
herdr-reviewr #66 is what that failure looks like when someone else tries it.

**C4. Persistent, queryable comment store that outlives the review UI. [E] 4/5.** tuicr ("persisted
across sessions," `tuicr review comments --session <slug>` with stable comment `id`s), difit
(localStorage per commit), hunk (`state.json`, per issue #821), revdiff (review artifacts on disk,
per issue #331). **Cockpit's store is an `mktemp` deleted on every non-deliver exit**
(`scripts/tmux-review:414-417`, `:645`) — the *least* durable design in the field, and difit #448
shows the resulting data-loss bug is real.

**C5. Pre-seeded agent self-review — the human triages, not authors from blank. [E] 3/5, rising.**
difit's `difit-review` skill preloads the agent's findings as comments; hunk's
`--agent-context notes.json` sidecar; tuicr's SKILL.md "agent review" mode. plannotator #1287 shows
users then want to *reply to and approve/reject* those agent comments. **Cockpit has no concept of
an agent-authored comment** — the store is human-write-only.

**C6. Agent onboarding via a shipped skill file, with multiplexer auto-detection. [E] 4/5.**
`hunk skill path`; `skills/tuicr/SKILL.md` branching on `$CMUX_WORKSPACE_ID`/`$TMUX`/`$ZELLIJ`/
`$HERDR_ENV`; difit's two bundled skills; plannotator's per-agent plugins/skills/hooks for nine
harnesses. **Cockpit ships no skill and no agent-side contract** — the agent receives a paste it
was never told to expect, in a grammar it was never taught.

**C7. Triggering is moving from on-demand toward lifecycle-interposed. [E] 2/5 shipped, but it's
the top project's flagship.** plannotator hooks `ExitPlanMode`/`PermissionRequest`; a user
(#1308) wired it to fire on every agent `git commit`; herdr-reviewr auto-opens in new worktrees.
[I] Cockpit's fleet queue is a genuinely good *pull* answer and is one of its real
differentiators — but it still requires the human to remember to press `prefix r`.

**C8. Scope selection beyond "uncommitted." [E] 3/5.** herdr-reviewr's four scopes including
**last-turn**; tuicr (uncommitted, commit ranges, PRs, and jj/mercurial); revdiff
(`revdiff`, `revdiff HEAD~1`, `revdiff main feature`). **Cockpit is uncommitted-only, unstaged-else-
staged** — the narrowest scope model in the field and, per herdr-reviewr #51, an actively
complained-about conflation.

**C9. Windowed/lazy rendering for large diffs. [E] emerging.** tuicr #613 asks for a sliding
3-screen highlight window; hunk offloads highlighting with `--fast`; herdr-reviewr #63 is a
polling-cost bug. [I] Everyone hits the same wall; the answer everyone reaches for is *do work
only for what's on screen*.

**C10. Non-consensus, in cockpit's favor. [E]** Cockpit's *quoted-fragment anchoring*
(`> the quoted fragment` under `## path:line (+)`, README:504+) is genuinely better than the bare
`path:Lnn` that difit and tuicr emit — a line number goes stale the instant the agent edits again,
a quoted fragment survives. revdiff shares the `##` grammar but not the quote line. **Keep this.**
Likewise "no accept-all key" (`--bind 'enter:ignore'`, `:401`) is a real anti-rubber-stamp stance
no other tool takes; and the *fleet queue* (which of N agents needs eyes, ordered by churn) has no
equivalent in any single-repo reviewer — it is cockpit's strongest original idea.

---

## 4. Where the fzf-line-list design diverges from consensus

| Axis | Consensus (§3) | tmux-cockpit | Verdict |
|---|---|---|---|
| Reading unit | hunk, in a file tree | one +/- line, flat, no context | **Diverges hard** — C1 |
| Context lines | always shown | none | **Diverges** — you cannot review a line you can't see in context |
| Comment granularity | line/range/hunk/file/review | line only | **Diverges** — C2 |
| Transport | structured stdout/JSON to harness | `paste-buffer` into agent TTY | **Diverges** — C3 |
| Store durability | persisted, ID'd, queryable | `mktemp`, deleted on any cancel | **Diverges hard** — C4 |
| Agent-authored comments | 3/5 pre-seed | none | **Diverges** — C5 |
| Agent contract | shipped skill file | none | **Diverges** — C6 |
| Scope | uncommitted / turn / range / PR | unstaged-else-staged only | **Diverges** — C8 |
| Modifier keys | plain keys, vim-style | alt-chords only | **Diverges, and is a live bug class** — hunk #843 |
| Ordering | file tree order (spatial, stable) | tests-first, then churn desc | **Diverges** — [I] a reading order that changes as the agent edits destroys the reviewer's mental map |
| Rerender cost | windowed/lazy | full rebuild after every comment | **Diverges** — C9 |
| Anchoring | `path:Lnn` | `path:line` + quoted fragment | **Cockpit wins** — C10 |
| No accept-all | not addressed | `enter:ignore` | **Cockpit wins** |
| Fleet/multi-agent triage | absent | churn-ordered dirty-pane queue | **Cockpit wins, uniquely** |

### The performance argument, specifically

[E] The design's own comments concede the cost. `scripts/tmux-review:100-111`: "The review popup
is MODAL and runs lines_mode at startup and **on every reload**, so that parse locked the one
client for minutes to hours (measured: a 100k-row single-file diff did not finish in 5 minutes,
while the git diff and the awk themselves cost ~0.03s — **the shell row-build is the whole
cost**)." And `:284-289`: "On a 2000-row diff that grep fired 2000 times per reload, and the
picker reloads after **every pinned comment** — the whole list re-marked from scratch each time."

[I] Both are real fixes to a self-inflicted problem. The architecture is *O(diff) work on every
comment*: `alt-enter` fires `execute-silent(attach)+reload(rows_cmd)` (`:402`), and `rows_cmd`
re-runs `git diff` → awk → a bash `while read` row loop → the marker pass. The 5000-row cap
(`:112`) is not a fix, it is a **capitulation**: past 5000 changed lines cockpit shows a single
"diff too large" row and offers *nothing*. A 50-file agent refactor — precisely the case where a
human most needs help — falls off the cliff. [E] For calibration, the tuicr user in #613
routinely reviews "~4000+ lines and/or 50+ files" and considers 5–10s unacceptable; cockpit
either serves that diff at multi-second-per-keystroke cost or refuses it entirely.

[I] Note the invariant this violates in spirit: "no shell on status redraw" is the repo's own
rule, and the review loop puts *the entire diff pipeline* on the comment-entry redraw path. The
freeze-precompute pattern the repo already adopted elsewhere (per project memory: "move
whole-server pane walks off the redraw path into agent-state-written options") is the same
lesson, unapplied here.

---

## 5. Ranked redesign recommendations

Ordered by (severity × confidence) ÷ cost. Each: **what / why / cost / what it replaces.**

---

**R1. Delegate the review surface: detect and hand off to `hunk`/`revdiff`/`tuicr` when present;
keep the fzf list only as the no-dependency fallback.**
*Why:* [E] Three actively-maintained, purpose-built reviewers (8.7k/2.9k/807 stars, all pushed
within 4 days) already solve the diff-reading problem better than a bash+fzf list ever will, and
[E] all three already know how to be launched into a tmux popup — revdiff *ships* tmux
`display-popup` support and a `REVDIFF_TMUX_WINDOW` mode; tuicr ships `tuicr-wrapper.sh` gated on
`$TMUX`. [I] The herdr ecosystem proves the model: `herdr-plugin-hunk` and `herdr-hunk-diff` are
thin shims, not reimplementations. Cockpit's durable value is *fleet triage + tmux-native
delivery*, not diff rendering. Concretely: `prefix r` resolves a reviewer in order
(`$COCKPIT_REVIEWER` → `revdiff` → `hunk` → `tuicr` → builtin), runs it in the popup, and reads
its **stdout/exit code** (revdiff's exit-10 convention is a ready-made contract).
*Cost:* **M** (detection + per-tool arg/exit-code adapters; the delivery half is unchanged).
*Replaces:* `lines_mode`'s role as the primary surface — and, at a stroke, C1, C2, C8, C9 and the
5000-row cap.

---

**R2. Stop using alt-chords as the only interaction.**
*Why:* [E] hunk #843 (opened today): alt/option chords are "**silently unreachable**" on any
terminal without the Kitty keyboard protocol — no key event, no warning. [I] Cockpit's
`alt-enter` (comment), `alt-u`/`alt-d` (scroll) are its *entire* keymap
(`scripts/tmux-review:402-403`); on default macOS terminals with Option-as-Meta off, or over a
Windows console SSH, the review popup does nothing and gives no diagnostic. Fix: bind a
plain-key primary (the field's consensus is a modeless `c` = comment, entering a small input,
per tuicr `c`/`C` and herdr-reviewr `v`→`c`), keep alt-chords as aliases, and — since fzf can't
report a dead binding — document the Option-as-Meta requirement in the popup header.
*Cost:* **S** if aliasing only; **M** if adopting a `c`-opens-input mode (which also removes the
need for `--disabled`).
*Replaces:* the alt-only keymap and the `--disabled` workaround at `:385-392`.

---

**R3. Make the comment loop O(1) per comment instead of O(diff).**
*Why:* [E] `reload(rows_cmd)` after every `alt-enter` (`:402`) re-runs git diff + awk + the bash
row loop + the marker pass; the code's own comment measures the shell row-build as "the whole
cost" (`:105-107`) and notes the picker "reloads after every pinned comment" (`:287-288`).
[E] tuicr #613 shows the field's answer is windowing, not caching. Cheapest correct fix here:
**build the row list once**, write it to a temp file, and make `reload` a `cat` of that file
piped through a marker pass that reads the store's `##` headers once — the diff is immutable for
the life of the popup (the agent is idle), so re-deriving it per comment is pure waste. Better
still, drop `reload` entirely: fzf can repaint a marker via `transform` on the current list
without re-execing the producer.
*Cost:* **S–M**.
*Replaces:* the reload-the-world binding; and it raises or removes the 5000-row cap (`:112`),
since the expensive pass happens exactly once.

---

**R4. Make the comment store durable and addressable instead of an `mktemp` that dies on cancel.**
*Why:* [E] 4/5 tier-one tools persist reviews (tuicr across sessions with stable comment IDs and
a `tuicr review comments --json` read path; difit in localStorage; hunk in `state.json`; revdiff
as on-disk artifacts), and [E] difit #448 documents exactly the bug cockpit is exposed to:
"Review comments can be **silently lost** when the server is stopped by SIGTERM." Cockpit deletes
the store on esc, on any fzf exit outside {0,1} (`:414-417`), and after delivery (`:645`) — a
detach, a resize crash, or a fat-fingered `^C` destroys an hour of review with no recovery.
Write to `$XDG_STATE_HOME/tmux-cockpit/reviews/<repo>-<pane>.md` instead; keep the last N; make
esc mean "leave it on disk," not "shred it." [E] Learn from revdiff #331 while doing so — keying
artifacts "by repository basename and wall-clock time" causes reviews to read or overwrite each
other; key by repo path hash + pane id.
*Why this stays inside the invariants:* [I] "state dies with the pane (no daemon/server/db)" is
about *processes*, not files — cockpit already writes a store to disk; this only changes *where*
and *when it is deleted*. A crash-recovery file is not a daemon.
*Cost:* **S**.
*Replaces:* `mktemp` + the three unconditional `rm -f -- "$store"` paths.

---

**R5. Ship a skill file so the agent knows the grammar it is about to be handed.**
*Why:* [E] 4/5 tier-one tools ship one (`hunk skill path`, `skills/tuicr/SKILL.md`, difit's two
skills, plannotator's nine per-harness integrations), and [E] tuicr's SKILL.md shows what it
buys: the agent knows the comment schema, knows `comment_type` semantics, and knows to *wait*
rather than assume the turn ended. Cockpit currently pastes a `##`/`>`/note block plus a `??`
question convention into an agent that was never taught any of it, and relies on the model
inferring intent. A `skills/cockpit-review/SKILL.md` (plus the >2048-byte file-pointer path, which
an untaught agent may simply not open) turns a hopeful paste into a contract.
*Cost:* **S**.
*Replaces:* nothing — pure addition, and the highest value-per-line item in this list.

---

**R6. Add hunk-range and file-level comment anchors.**
*Why:* [E] C2 — tuicr has four granularities, revdiff has file-level plus a `hunk` keyword that
auto-expands the anchor to the full hunk range, difit has drag-ranges, herdr-reviewr has `v`
selection. [I] "This whole function is wrong" and "split this file" are the two most common review
comments a human writes, and cockpit can express neither; forcing them onto one arbitrary `+`
line makes the note read as narrower than it is. revdiff's keyword trick is the cheapest possible
implementation and needs no UI change at all.
*Cost:* **S** for revdiff-style keyword expansion (the awk already carries hunk boundaries,
`:126-131`); **M** for a real range-select.
*Replaces:* line-only anchoring.

---

**R7. Change the delivery transport from "type into the agent's TTY" to "hand the harness a file,
and tell it where."**
*Why:* [E] C3 — *no* tier-one tool types keystrokes into another process's terminal;
[E] herdr-reviewr #66 documents what happens when you try ("Sending comments to a
kitty-keyboard-enabled Textual pane **loses formatting or drops the payload**"), and cockpit
already has the 2048-byte bracketed-paste ceiling that forced it to invent a file-pointer
fallback. [I] The right shape is to make the *file* the primary and the paste a one-line pointer
always — which is already the >2048 path — so behavior stops bifurcating at a size threshold and
the payload stops depending on the receiving TTY's paste handling.
*Caveat, stated plainly:* cockpit's "never auto-submit; the human presses Enter" invariant is the
*right* call and is more conservative than revdiff's exit-10 auto-feed. This recommendation keeps
that — only the payload changes from "the whole review" to "a pointer to the whole review."
*Cost:* **S**.
*Replaces:* the size-conditional branch in `--deliver`.

---

**R8. Add scope selection — at minimum `staged | unstaged | both`, ideally `last-turn`.**
*Why:* [E] `lines_mode` takes unstaged *else* staged (`:97-98`), so the moment the agent stages
anything the review silently shows a different set than the human expects. [E] herdr-reviewr #51
is exactly this complaint, with a +1 asking for VS-Code-style split sections "to allow iterating
without the need to commit per iteration," and a fork already shipped it (#26). [E]
herdr-reviewr's **last-turn** scope is the one cockpit would benefit from most: [I] cockpit
already tracks agent state per pane (per the freeze-precompute work), so "changes since this
pane's last agent turn started" is derivable without a daemon.
*Cost:* **S** for staged/unstaged/both; **M** for last-turn.
*Replaces:* the `else` in `:97-98`.

---

**R9. Order rows spatially (file tree order), not tests-first-then-churn.**
*Why:* [E] Every tier-one tool presents files in tree/path order with a stable sidebar (C1).
[I] Cockpit's tests-first-then-churn-desc ordering is a clever triage heuristic, but it means the
reading order is *derived from the diff's shape* — so it can change between reviews of the same
work, and it scatters related changes (a function and its caller, a type and its uses) across the
list. Reviewers build a spatial map; a churn-sorted list denies them one. Keep churn ordering
where it genuinely shines — the **fleet queue** across panes (`fleet_rows`, `:619-621`), which is
a triage problem — and drop it inside a single diff.
*Cost:* **S**.
*Replaces:* the `order[]` sort in `lines_mode`.

---

**R10. Pre-seed the human's review with the agent's own findings.**
*Why:* [E] C5 — difit's `difit-review` skill "launch[es] difit with findings or explanations
preloaded as comments," hunk takes `--agent-context notes.json`, tuicr's SKILL.md defines an
"agent review" mode. [E] plannotator #1287 ("I want to reply to an AI agent review and/or
approve/reject its comments") shows where this goes next. [I] For cockpit this is cheap because
the store is already a plain append-only file with a stable grammar: an agent (or `/code-review`)
writes `##` blocks into the store *before* the popup opens, `rows_mode` already marks commented
rows with `●` (`:272-278`), and the human arrives at a pre-annotated list to triage. This turns
cockpit from "blank-page review" into "triage the agent's self-review," which is where the field
is heading.
*Cost:* **M** (mostly the agent-side contract from R5).
*Replaces:* human-write-only store semantics.

---

**R11. Mark the pane as "under review" in cockpit's own status.**
*Why:* [E] revdiff #318 shows the failure from the other side: with the agent parked in a tool
call, "a parked tool call is indistinguishable from active work," so herdr's sidebar showed
`working` for the whole review and `agent wait --until blocked` never fired. [I] Cockpit's mirror
problem: nothing marks a pane as *currently being reviewed*, so the fleet queue will happily hand
the same pane to a second `prefix r` in another window, and two stores race to deliver into one
prompt. A per-pane tmux option set on entry and cleared on exit (the freeze-precompute pattern the
repo already uses) fixes it without a daemon.
*Cost:* **S**.
*Replaces:* nothing — closes a real multi-window race.

---

**R12. Consider hook-triggered review as an opt-in, on top of `prefix r`.**
*Why:* [E] C7 — plannotator (7,985) interposes on `ExitPlanMode`/`PermissionRequest` so review
cannot be forgotten, and a user (#1308) wired review to fire on every agent `git commit`;
herdr-reviewr auto-opens in new worktrees. [I] Cockpit's fleet queue is a strong *pull* mechanism
and should stay the default — but an opt-in Stop-hook that flips a per-pane "wants review" flag
(which the fleet queue then sorts to the top, and the status bar flags) would give push semantics
without violating "never steal focus": the flag is passive, the human still presses `prefix r`.
*Cost:* **M**.
*Replaces:* nothing; strictly additive.
*[I] Deliberately ranked last:* it is the largest departure from cockpit's "no daemon, one-shot
scripts, never steal focus" character, and R1–R5 deliver far more per unit of risk.

---

## 6. Bottom line

[I] Cockpit's review has three genuinely good ideas — quoted-fragment anchoring, no accept-all
key, and the cross-pane fleet queue (which nothing else in this field has) — bolted onto a
reading surface that diverges from every converged practice in the category, on a transport
nobody else uses, over a store less durable than anything else shipping, with a keymap that has a
documented silent-failure class. The highest-leverage move is not to rebuild the fzf list into a
TUI; it is **R1 + R2 + R3 + R4 + R5** — delegate diff-reading to the tools that won, fix the
keymap, take the diff pipeline off the per-keystroke path, stop shredding the store, and tell the
agent what it is receiving. That is roughly S+S+M+S+S of work and closes every hard divergence
except comment granularity.

## Sources

- [modem-dev/hunk](https://github.com/modem-dev/hunk) · [issue #843](https://github.com/modem-dev/hunk/issues/843) · [agent-workflows.md](https://raw.githubusercontent.com/modem-dev/hunk/main/docs/agent-workflows.md)
- [backnotprop/plannotator](https://github.com/backnotprop/plannotator) · [#1287](https://github.com/backnotprop/plannotator/issues/1287) · [#1308](https://github.com/backnotprop/plannotator/issues/1308)
- [agavra/tuicr](https://github.com/agavra/tuicr) · [SKILL.md](https://raw.githubusercontent.com/agavra/tuicr/main/skills/tuicr/SKILL.md) · [#613](https://github.com/agavra/tuicr/issues/613)
- [umputun/revdiff](https://github.com/umputun/revdiff) · [#280](https://github.com/umputun/revdiff/issues/280) · [#318](https://github.com/umputun/revdiff/issues/318) · [#331](https://github.com/umputun/revdiff/issues/331) · [revdiff.com](https://revdiff.com/) · [Show HN](https://news.ycombinator.com/item?id=47742437)
- [yoshiko-pg/difit](https://github.com/yoshiko-pg/difit) · [#448](https://github.com/yoshiko-pg/difit/issues/448)
- [herdrdev/herdr](https://github.com/herdrdev/herdr) · [persiyanov/herdr-reviewr](https://github.com/persiyanov/herdr-reviewr) · [#26](https://github.com/persiyanov/herdr-reviewr/issues/26) · [#63](https://github.com/persiyanov/herdr-reviewr/issues/63) · [#66](https://github.com/persiyanov/herdr-reviewr/issues/66)
- [smtg-ai/claude-squad](https://github.com/smtg-ai/claude-squad) · [BloopAI/vibe-kanban](https://github.com/BloopAI/vibe-kanban) · [stravu/crystal](https://github.com/stravu/crystal) · [craftzdog/tmux-claude-session-manager](https://github.com/craftzdog/tmux-claude-session-manager)
