# Review redesign: what nine lenses found, and what to build instead

Research date: 2026-08-24. Synthesis of ten parallel Opus lens reports, each with its own
measurements and verified sources, kept verbatim under
`docs/superpowers/research/2026-08-24-review-lenses/` (landscape, ui, ux, ai, dev, perf, when-how,
rewrite, tmuxai, tcsm). Working prototypes
the lenses produced — a byte-identical awk parser, a cached re-mark, a numstat-first cap, a
parallel fleet scan — are under `…/prototypes/`. Every star count and last-push date below was
verified with `gh api` on the research date. `[E]` = measured or fetched; `[I]` = inference.
One-page artifact of this document and the execution plan:
https://claude.ai/code/artifact/8682a108-927b-4fa3-aeb2-a2f6e100a84c — execution plans:
`docs/superpowers/plans/2026-08-24-review-redesign-goal.md` (master) and
`2026-08-24-review-phase0-1.md` (phases 0–1, executable).

## 1. Verdict

The owner's sentence — *"how it works today we can't use it"* — is mechanically correct, not a
taste complaint. The review feature has two genuinely original pieces (the fleet queue, and a
delivery path that re-checks the target pane inside tmux's own command queue) bolted onto a
reading surface, a transport, a store lifetime and a trigger model that diverge from **every**
tool that has won this category in the last twelve months. Nine lenses looking from different
directions reproduced **fourteen defects**, seven of them wrong-result or data-loss class, and
measured the performance problem precisely enough to say why: the wrong unit of work (a line),
executed by the wrong tool (bash), re-derived on every keystroke, with the size cap applied after
the expensive step.

The answer is **not** a rewrite in Go or Rust — that premise was measured and falsified
(§6) — and **not** a bigger fzf. It is:

1. fix the reproduced bugs (a day);
2. move the row build into the awk that already runs, cache rows for the popup's life, and
   parallelise the fleet scan (a day or two; prototypes exist and are byte-identical);
3. give the review a **moment** (a `◆ review` attention state set at the agent's `Stop`), a
   **base** (branch/checkpoint, not "unstaged else staged"), a **loop** (spool survives, `^A`
   approve, re-review shows only the delta), and a **delivery that lands without a pane switch**
   (Stop-hook drain when the agent is busy, today's paste when it is idle);
4. keep the fzf list as the zero-dependency fallback and **delegate diff reading** to
   `revdiff` / `tuicr` / `hunk` when one is installed — the plugin's job is fleet triage and
   tmux-native delivery, not diff rendering;
5. only then ask, on product merits and not speed, whether a Go TUI is worth ~15 days.

## 2. What each lens concluded

| Lens | Headline |
|---|---|
| **Landscape** | Field is converged and active: herdr 32,024★ (a tmux-shaped agent multiplexer with a plugin ecosystem, incl. herdr-reviewr 510★), hunk 8,711★, plannotator 7,985★, difit 3,095★, tuicr 2,868★, revdiff 807★ — all pushed within days. `gh search "fzf diff review"` returns zero results: nobody uses fzf as a review surface. Consensus C1–C9 (§4). Alt-only keymap is a documented silent-failure class (hunk #843, opened the same day). |
| **UI** | Ran the popup live. Blank added lines render as invisible rows and the "diff too large" notice renders as an empty popup (tab-collapsing `IFS` read); list is monochrome under a coloured preview; preview always shows hunk 1; 8-row porthole; fixed row split collapses to one row on resize; deleted, renamed and **new files are absent** from the list with no hint. |
| **UX** | 24-stage journey map, 10 severity-4 defects: unfrozen diff moves `●` marks onto lines never commented on; `↵` is a silent no-op right after the sibling picker trained `↵`; `esc` destroys the batch silently; the only "press Enter there" instruction is a sub-second flash naming `%12`; delivery leaves you in an agent pane from which `prefix r` cannot reach the queue; popup swallows all prefix keys, blinding you to the other five agents. |
| **AI / agent** | `deliver()` never reads `@panestate` — a review can be pasted into an open permission dialog, where the human's Enter becomes a blind approval. Paste-into-TTY has essentially zero adoption (agent-mux, 2★); winners intercept hooks or block on stdout. `Stop` + `decision:block`/exit 2 is a first-class turn-boundary delivery primitive. Hybrid: paste when idle, Stop-hook drain when busy. Compose header has five named defects. |
| **Dev** | No artifact in the middle: every reload re-derives the model from live `git diff`. Confirmed: anchor drift; a mixed staged/unstaged tree silently drops the staged half (`git diff HEAD` fixes it); untracked-only panes dead-end. Store grammar has a writer and no reader and is not round-trippable. awk row build verified byte-identical on all 6 suite fixtures, 8.7× faster. Keep bash; keep the hand-rolled parser (no porcelain diff format exists). |
| **Perf** | Attribution at 4900 rows: bash row loop 72%, emit loop 18%, git 5%, awk 5%. One alt-Enter = 0.95–1.16 s (cached re-mark: 18–24 ms, ~40×). Cap runs after the capture: 51 MB lockfile = 5.7 s / 254 MB to print "too large". Preview re-runs 2–3 `git diff` per cursor move. Fleet 50 panes on a real repo = 2.5 s serial, ~350 ms parallel. fzf itself: 36→57 ms from 100→5000 rows. revdiff opens in a flat 121–198 ms. |
| **When & how** | Five load-bearing omissions: no review moment, working-tree-only base (a committing agent vanishes from the queue), no reviewed state, no loop closure, one pane switch + Enter per delivery. Community: plan gates are automated (plannotator/revdiff on `ExitPlanMode`), code gates are `Stop`-hook or manual; nobody gates `git commit`. Proposes a 5-state machine with `@cockpit_review`, `refs/cockpit/reviewed/<branch>`, `^A` approve. |
| **Rewrite** | Startup: `bash -c :` 1.97 ms vs stripped Rust 1.80 ms (0.17 ms apart); the tmux roundtrip (7.5 ms) dominates every option. Every healthy Go/Rust tool in the niche escaped TPM (sesh, gitmux); the one Rust TPM plugin (tmux-thumbs) is dead since 2024. A+E now; if a TUI is ever funded, one **Go** binary, no bash fallback for it. |
| **tmuxai** | Not a reviewer (zero diff/review code), drives a shell pane it owns, `send-keys` path presses Enter on the word "Enter" (`system/tmux_send.go:59-73`). Do not adopt. Worth copying: function-var stubs for hermetic tmux tests; the *idea* of a prompt-sentinel turn boundary (build it on OSC 133, not PS1 rewriting). |
| **tmux-claude-session-manager** (368★, 330 LOC, no tests) | No review surface at all (author reviews in codediff.nvim/lazygit). Zero key/status/hook conflicts with cockpit; coexists but is redundant. One better idea — ask Claude what Claude is doing and resolve identity via `pid→tty→pane` — with one disqualifying cost: `claude agents --json` is 0.5–2.5 s (`--cwd` does not help). **Steal via the hooks cockpit already runs**, not the CLI: stash `transcript_path`/`session_id` from the hook payload into pane options, read `ai-title` from the transcript in 9 ms for tab labels and fleet rows, and the id is the exact `claude --resume` argument. Sort the fleet queue `waiting > done > … > busy`, churn only within a band. Note: the owner's own fork PR (#22: rows.awk, contract tests, ai-title, grouping, background-agents row) was closed unmerged in 20 minutes — that work belongs in cockpit. |

## 3. Reproduced defects (ranked)

Sev 4 = wrong result or data loss; 3 = blocks the task; 2 = costs time.

| # | Defect | Sev | Where | Lens |
|---|---|---|---|---|
| 1 | **Diff is not frozen.** An agent edit mid-review re-anchors the list; `●` moves to a line the human never commented on and the delivered review names the wrong line. | 4 | `rows_mode` → `lines_mode` (`:309`), live `git diff` on every reload | dev §2.2, ux §3 |
| 2 | **`deliver()` ignores `@panestate`.** A pane at `needs-input` passes every guard; the human's Enter lands in a permission dialog. The repo's own antipatterns doc specified this guard. | 4 | `:471-497`, `:558` | ai §5.1, when-how §0.8 |
| 3 | **Mixed staged/unstaged tree drops the staged half silently.** "Unstaged else staged" is written 4× at 2 granularities and disagrees with the preview. | 4 | `:92-98`, `:596-597`; `tmux-open-path:76` | dev §2.3, ui D10 |
| 4 | **New (untracked), deleted and renamed files are absent.** The fleet queue counts `?N new` then sends you to a review that cannot show them. | 4 | `git diff` never lists untracked; `:610` counts them | ui D10, dev §2.4 |
| 5 | **Blank diff lines render as empty rows; the "diff too large" notice renders as an empty popup.** `IFS=$'\t' read` collapses adjacent tabs; the blank line's stored fragment becomes the display string. | 4 | `rows_mode:301`, `lines_mode:122` | ui D1, D2 |
| 6 | **`esc` shreds the store, no confirm; any non-{0,1} fzf exit shreds it; delivery shreds it.** No recovery for a dropped SSH client (client-owned popup dies). | 4 | `:414-417`, `:643` | ux #4, landscape C4, dev §5.2 |
| 7 | **The fleet loop dead-ends.** Delivery leaves you in an agent pane; `prefix r` there reviews that pane again; the queue is unreachable. | 4 | `main_entry:705-722`, `run_loop` hard exits | ux #3, ui D15, when-how §0.6 |
| 8 | **Only instruction to press Enter is a 750–2000 ms status flash naming `%12`.** | 4 | `:560` | ux #5, ui D16 |
| 9 | **`↵` is a silent no-op** in the review, two keystrokes after `↵` selected a pane in the queue; the header text explaining it is the first thing truncated below ~100 cols. | 4 | `:401`, `:407` | ux #2, ui D7 |
| 10 | **Preview never follows the cursor** — keyed on `{1}` (path), always hunk 1; regenerated identically on every cursor move at 40–344 ms each. | 4 | `:365` | ui D3, perf §4 |
| 11 | **Alt-chords are the entire keymap.** Silently dead on terminals without Option-as-Meta / kitty protocol (hunk #843). | 3 | `:402-403` | landscape §2.2 |
| 12 | **Store grammar: writer, no reader, not round-trippable.** `## `→` ## ` escape never un-applied; `> ` body lines indistinguishable from fragments; empty/non-numeric `line` delivers `## f: (+)`. | 3 | `attach:332-342`, `compose:444` | dev §2.5 |
| 13 | **Fragment cut at 160 bytes with no ellipsis is what gets quoted to the agent** — matches nothing in the file, defeating the anchoring invariant. | 3 | `:240` | ui D9 |
| 14 | **Fleet rows omit repo and branch**; two panes in different repos render byte-identical; sort is by churn, blind to `@panestate`. | 3 | `fleet_rows:580-620` | ui D14, ux #10 |

Plus: comments are invisible/uneditable/undeletable after pinning (one `●` for N comments, retractions deliver alongside what they retract); no multi-line; no range/file-level anchors; fixed `psz` starves the list on resize; non-ASCII paths still octal-escaped in the preview one file over; the >2048-byte switch fires at ~13 comments so a thorough review is the one you cannot see before sending.

## 4. Where the community converged, and where we sit

| Axis | Consensus across hunk / tuicr / revdiff / difit / plannotator / herdr-reviewr | tmux-cockpit today | Verdict |
|---|---|---|---|
| Reading unit | file tree + hunk, three-level nav (line/hunk/file) — 5/5 | one +/- line, flat, no context | diverges hard |
| Comment granularity | line / range / hunk / file / review-level — 4/5 | line only | diverges |
| Transport to agent | structured stdout / JSON session API / hook return in the TUIs; **human hand-off is mainstream too** — codiff's headline is "comments you can paste back into your LLM", prereview ships a "Hand-off" button | `paste-buffer` into the agent's prompt, typed-but-unsent | converges, with the safer implementation (atomic re-gate; herdr-reviewr #66 is what programmatic TTY injection looks like without it) — corrected after the HN check, §6c |
| Store lifetime | persisted, ID'd, queryable — 4/5 | `mktemp`, deleted on any cancel | diverges hard (difit #448 is the resulting bug) |
| Agent-authored comments | pre-seeded self-review — 3/5, rising | none | diverges |
| Agent contract | shipped skill file with multiplexer detection — 4/5 | none; the agent gets a paste it was never told about | diverges |
| Scope | uncommitted / last-turn / branch / range / PR — 3/5 | unstaged-else-staged only | diverges |
| Trigger | plan gates automated; code review `Stop`-hook or manual; nobody gates `git commit` | on-demand only | diverges; commit gate is open territory |
| Modality | split pane or server-owned window; popup is the *option* | client-owned modal popup | diverges |
| Keys | plain keys, vim-style; `↵` or a letter comments | alt-chords only, `↵` no-op | diverges, live bug class |
| Anchoring | bare `path:Lnn` | `path:line` **+ quoted fragment** | **cockpit wins** — keep |
| No accept-all | not addressed | `enter:ignore` stance | **cockpit wins** — keep the stance, not the dead key |
| Fleet triage | absent everywhere | churn-ordered dirty-pane queue | **cockpit wins, uniquely** |
| Delivery safety | n/a | atomic `if-shell -F` re-gate at paste time | **cockpit wins** — best code in the plugin |

## 5. Performance: why, with numbers

Measured on this machine (12-core, mawk, bash 5.2, fzf 0.44.1, tmux 3.7c), medians.

**Attribution of one `--lines` parse, 4900 rows / 1 file, 461 ms total:**

```
git diff (capture)     12.9 ms   2.8%
git diff --numstat     11.2 ms   2.4%
awk hunk parse         24.8 ms   5.4%
bash row loop         331.9 ms  72.0%   ← 85 µs/row of pure bash vs 11 µs/row in awk
bash sort+emit loops   81.2 ms  17.6%   ← re-emits every row through printf, one at a time
```

**What the human feels:**

| moment | 1000 rows | 4900 rows | 4998 wide rows | fix → |
|---|---|---|---|---|
| `prefix r` → first frame | 163 ms | 450–753 ms | 1.6–1.7 s | awk-only: 103–136 / 279–321 ms; streaming: 94–130 / 157–245 ms |
| one alt-Enter → `●` appears | 275 ms | 946–1164 ms | ~2 s | cached rows + awk re-mark: **18–24 ms** |
| each cursor move (preview) | 40 ms | 93 ms | 344 ms | memoise per file: ~0 on repeat |
| regenerated lockfile → "too large" | — | — | **5.7–10.4 s, 254 MB** | numstat-first cap: 0.87 s, 103 MB |
| fleet, 50 panes, real repo | 2.5 s serial | | | `xargs -P16`: ~350 ms |
| 20-comment session at 4900 rows | ~4.2 s dead | ~19.5 s dead | | ~0.85 s |

**Baselines on the same diffs:** fzf alone 36→57 ms (100→5000 rows) — innocent; `delta` renders
4900 rows with syntax highlighting in 99 ms; `revdiff` opens its full TUI in 121–198 ms *regardless
of size* because it renders a viewport, not a list.

**The seven architectural reasons** (perf §9): the unit of work is a line and the executor is
bash; nothing is cached and fzf's `reload` contract forces a full rebuild; every fzf bind is a
fresh process (~10–15 ms floor); the parse is done three times (diff, numstat, preview); the
fleet scan is serial and fork-heavy; the cap is on rows not bytes and is applied after the
capture; everything is materialised before the first frame. Real commits in this repo run
**22–61× fewer hunks than changed lines** — hunk-level rows would put every realistic review
in the 10–100-row regime where none of this matters.

## 6. The language question, answered

**No to "replace all with Go or Rust."** Measured: `bash -c :` 1.97 ms, stripped Rust hello
1.80 ms, default Rust binary 2.21 ms (slower than bash — page-in cost). One `tmux show-option`
roundtrip is 7.5 ms and every option pays it. The one slow path (892 ms at the cap) drops to
47 ms in pure awk with byte-identical output. `cockpit.tmux` (352 lines of tmux formats) cannot
be rewritten in any language. The 3,306-line suite asserts tmux rendering and survives a
rewrite nearly unchanged — it just has to be ported. Every healthy Go/Rust tool in this niche
(sesh 2,783★, gitmux 791★) escaped TPM and ships via package managers; the one Rust TPM
plugin (tmux-thumbs 1,095★) needs cargo, has no arm64 mac asset, and is dead since 2024 while
the bash-origin project it replaced (tmux-fingers 1,465★) is still shipping.

**Honest pro-rewrite case:** not speed — (a) killing the macOS bash-3.2/BSD-mktemp class, and
(b) fzf is a hard ceiling on review UX: hunks with context, side-by-side, word-diff, multi-line
comments in place, ranges, a comment gutter. Those are exactly what revdiff/tuicr/hunk already
do. So: **shell out to them when installed** (option E) and, if a first-party TUI is ever
funded on product merits, write it in **Go** (goreleaser + `GOOS/GOARCH` are decisive for a
six-platform TPM distribution), keep `cockpit.tmux` as a bash loader that locates the binary
tmux-fingers-style, and ship fzf as the documented degraded mode — never a second parser to
drift.

**tmuxai:** a distraction for review. **tmux-claude-session-manager:** do not install; take
`ai-title` / `transcript_path` / `session_id` through the existing hooks (9 ms, no CLI) and the
state-first fleet sort; port the owner's fork work (rows.awk, contract tests, background-agents
row) into cockpit rather than rebasing it on a 330-line plugin with no tests. Details in `tcsm.md`.

## 6b. What tmuxai and tmux-claude-session-manager give on the front end — and what to lift

Follow-up question from the owner: not the mechanisms, the *interfaces*. Two further reports
(`tmuxai-ux.md`, `tcsm-ux.md`) inventoried every screen, key and string; TCSM's picker was
rendered live at 140 and 90 columns.

**What each one gives you, in one line each.** TCSM: four surfaces, three strings, nine
options, one key (`prefix u`) that answers *"who needs me?"* — rows sorted needs-you-first,
coloured state glyph + age + location + cwd, and the agent's live screen (the actual
permission prompt) already visible above the list; `↵` jumps, `^X` kills. tmuxai: a chat REPL
with a state glyph in the prompt (`▶ ? ✓ ∞`), a confirm prompt with a risk badge and an
`e`dit option, a countdown with its two keys printed inline, lists with a totals footer, an
`/info` screen with a colour-thresholded budget bar, and failure screens that end with the
exact next command to type.

**Ease ratings (install / first run / day-to-day / other).** TCSM 4 / 4 / 3 / discoverability 3
— a very good 15-minute experience, adequate at 15 hours (0.5–2.5 s to open, no preview
scroll, unconfirmed kill, `↵` does two different things depending on a hidden field, columns
are tab stops that break on long names, empty state is a blank box). tmuxai 4 / 3 / 4 / safety
2 — well-mannered CLI, thin on-ramp (hand-written YAML + API key, error at first message),
safety theatre over an unsafe transport (empty input = yes, whitelist bypass, `--yolo` from chat).

**Side by side with cockpit's own rows (live output):**

```
TCSM      ● waiting     4m   host:1.0   ~/code/webapp           state · age · where · cwd
cockpit   henosis:3 Claude Code              +0 -0  ?2 new       where · pane_title · churn
```

Cockpit's fleet queue is a *dirty-tree* queue, not a *needs-you* queue: a blocked agent with
a clean tree is not in the list at all (`tmux-review:592`), the sort key is churn and every
live row has the same churn, and the middle column is the unset `pane_title` default. TCSM
answers the one question cockpit currently answers worst — and cockpit already has every
input (`@panestate` per pane at zero cost, rendered in `tm`'s preview but not in the queue).

**Ranked front-end steals (what / replaces / cost):**

| # | Steal | From | Replaces in cockpit | Cost |
|---|---|---|---|---|
| 1 | `@panestate` glyph as the first fleet column; sort state-rank then age, churn as tiebreak | TCSM `agents.sh:46-62` | `fleet_rows` row build `:616` and `sort -k1,1nr` `:620` | S |
| 2 | A row per agent unconditionally — blocked-but-clean agents appear | TCSM premise | `[ -n "$st" ] \|\| continue` `:592` | S/M |
| 3 | Confirm-before-deliver with `e`dit: "5 comments · 2 questions · 1.4 KB → %7 … deliver? [y/N/e]" — default **N** | tmuxai `confirm.go:20-139` | the silent path between `^S` and `deliver()` | M |
| 4 | Totals footer under the fleet rows: "4 agents dirty · 182 changed lines · 1 needs input" | tmuxai list idiom | nothing — addition below `--header` `:684` | S |
| 5 | One aligned, copy-pasteable `@cockpit_*` options block near the top of the README | TCSM README:92-102 | options scattered across 694 README lines | S |
| 6 | `prefix i` info popup: version, glob, keys, hooks wired, per-pane state — reads options only | tmuxai `/info` | README troubleshooting prose (:639, :665) | M |
| 7 | Multi-line failure copy in popups: what failed, then the exact command to type | tmuxai `chat_command.go:120` | one-line toasts at `tmux-review:654,660` | S |
| 8 | `@cockpit_fleet_fzf_options` escape hatch (fleet queue and `tm` only — never the review loop, where `↵` has a contract) | TCSM `picker.sh:28-33` | hard-coded fzf lines, users fork for `j/k` | M |
| 9 | Live-screen preview as a third `^/` state on the fleet queue (diff → screen → off); strip trailing blanks, no `follow` | TCSM `picker.sh:40` | nothing — `--preview-diff` stays default | M |
| 10 | Age column from a `date +%s` stamp at the `done`/`needs-input` transition (zero subprocesses) | TCSM `agents.sh:51` | nothing — new column | S |
| 11 | Head-elide paths, `printf %-Ns` every column | TCSM's own defect | the tab-stop columns TCSM shows breaking | S |
| 12 | Examples inside usage strings; a `--pane %N` override for tests | tmuxai `cli.go:120-128` | `tmux-review:56-59` usage; implicit target resolution | S |

**Do not copy:** md5 session names, popup-in-popup race loops, an unconfirmed kill, hiding a
field that changes what `↵` does, `follow` on a capture-pane preview, a YAML config, default-
yes, `--yolo`, a whitelist that bypasses the human, a timer-driven spinner, `/info` that
re-captures every pane (the freeze class cockpit just removed), prefix-matched commands.

Steals 1, 2 and 4 fold into roadmap phase 0; 3, 6, 7 into phase 3; the rest are polish.

## 6c. Reality check against the humans: the Show HN threads

The owner pointed at the revdiff Show HN (`news.ycombinator.com/item?id=47742437`). Verified
twice: **17 points, 4 comments, no pagination** — the author's only reply is "happy to answer
questions" and nobody asked. difit, tuicr, plannotator and diffnav's threads have 0–3 comments
each. The category's only real discussion is `Show HN: Codiff` (id 48166275, 21 comments),
read in full. Report: `hn-revdiff.md`. Four corrections to this synthesis follow from it.

1. **"The winners converged" is a statement about repo design, not adoption.** Star counts and
   pushed-today dates are real; community validation of any single tool is not. Keep revdiff as
   the first backend on *engineering* grounds (identical grammar, shipped tmux overlay, single
   static binary) and drop any claim that the community picked it.
2. **Paste-back is not the outlier §4 originally said.** codiff and prereview both ship human
   hand-off as their primary delivery. The narrow distinction is *programmatic* injection into a
   foreign TTY vs *human-mediated* hand-off; cockpit's typed-but-unsent paste is the latter with
   an atomic re-gate nobody else has. Row re-rated above. The hybrid in §7.4 stands.
3. **Phase 5 (a first-party Go TUI) takes its heaviest hit here.** Two practitioners who each
   built a reviewer this year left the terminal for the exact reason cockpit would build one:
   volume. cpojer: "4 of these are TUIs. I built Codiff specifically because TUI review became
   overwhelming to me"; realrocker: "LLM code output is so high it has become unreasonable to
   review in terminal or IDE." Unopposed in-thread. The terminal's ceiling is a volume ceiling a
   better TUI does not raise — the answer at the phase-5 gate should be "no", and the in-tmux
   fix for volume is more screen and persistence (`@cockpit_review_window`) plus triage, not
   more fzf. The fleet queue — triage before rendering — is the thing 25 comments across both
   threads did not contradict once; every tool named is single-repo, single-diff.
4. **Plan-mode review is under-weighted.** It is the only workflow a real user volunteered
   (`r13v`), revdiff shipped a second plugin for it, and it is plannotator's origin story. Diff
   review is the crowded surface; plan review is where users report the value and cockpit does
   nothing. Candidate for the roadmap: a `PreToolUse`/`PermissionRequest` interception of
   `ExitPlanMode` that opens the plan in the same review popup and returns annotations as the
   deny reason — the same `review-gate` script, one more event.

Also validated: scope/base work (`tcoff91` needs commit ranges because jj has no staging;
codiff's one functional complaint was no commit targeting); hunk/file granularity (revdiff's
own "any line, hunk, or file"); agent-authored first pass (codiff's "LLM walkthrough mode").
New to the research: codiff, prereview (remote/phone review over Tailscale), diffx, lumen,
pyor.review, diffs.com + trees.software (the libraries cpojer built a reviewer on in 16 min);
risk-tiered review depth (`bitomule`) — order the fleet queue by risk globs (auth, migrations,
CI, tests-only), not churn alone.

## 6d. hunk, run live — and the first-backend decision

The owner asked for hunk (8,712★, MIT, TS on OpenTUI, pushed the same hour) to be checked
directly. Installed v0.19.0 into the scratchpad only, driven live at 160 and 90 columns on a
fixture with a deletion, a rename, a new file, a long line and a non-ASCII path. Report:
`hunk.md`.

**As a reader it is the best of the three.** Zero config; correct on every fixture case
including the non-ASCII path and the rename; side-by-side that collapses to a stack at 90
cols keeping both line-number gutters; sidebar with per-file status and ± counts; unshifted
motion (`[`/`]` hunk, `,`/`.` file, `{`/`}` comment); `c` comments and `/` filters as separate
keys; `z` toggles context; `e` opens the file in `$EDITOR` at the anchor; an in-app `?` key
overlay; an agent-context sidecar rendered inline. Time-to-first-frame is **flat** — 0.80 s at
200 rows, 0.92 s at 9,800 rows, no "diff too large" bailout at all. Ratings: install 3 (the
npm package name `hunk` silently resolves to an unrelated 2013 project; the real route is
387 MB via npm or a 49 MB tarball), first run **5**, day-to-day 4, agent integration 3.

**As a citizen it breaks every invariant at once.** Launching the TUI spawns a daemon on
`127.0.0.1:47657` with no opt-out; the daemon **outlives the TUI** (96 MB orphan still
listening minutes after `q`); and the review comments do **not** outlive it — there is no
state file anywhere, so `hunk session comment list` after quit returns "no active session".
A modal popup therefore cannot `hunk; harvest` — the harvest must be a background poller
running *while* the human reviews, plus a `pkill` of another project's daemon afterwards (the
agent's own `pkill -f` matched its own shell and killed its session — match on the binary
path, never the words). The JSON also carries no quoted fragment, so cockpit's one grammar
advantage would need a second `git diff` pass to restore. Fixed cost per popup open: ~0.9 s
and ~270 MB resident, versus 8 ms for cockpit's awk pass on the same 9,800 rows.

**Decision for §10 #4 — wire revdiff first, hunk second as an explicit opt-in "rich reader",
tuicr third.** revdiff's contract (stdout + exit code) is literally cockpit's own `^S → exit
20 → deliver` idiom, its grammar is ours (it does not emit our `> fragment` line itself — no
backend does — but `--annotations` in → `-o` out round-trips it byte-identically), it is one
static binary on a TPM box, and it needs no relaxation of "no daemon". Proving the
`@cockpit_reviewer` seam against the backend whose contract already fits isolates bugs in
*our* switch; wiring hunk first would conflate that with fighting a daemon lifecycle. Offer
hunk once the seam is proven, gated and documented ("this backend runs a daemon"), because
for users who accept that it is the strongest diff reader available.

**Can hunk and revdiff work together?** (`hunk-revdiff-together.md`, live lab with both
installed.) Yes as a documented pairing, no as a product feature, never as two reviewers:
nobody in the community runs both as reviewers; a `c` typed in one never reaches the agent
through the other; two `PreToolUse` hooks on `ExitPlanMode` both fire and the second popup
silently no-ops (a popup opened from inside a popup exits 0 with empty output — reproduced).
The one composition that is real: **cockpit's store is the artifact, revdiff is a lossless
two-way editor over it** (`--annotations=comments.md` in, `-o comments.md` out — byte-identical,
fragment preserved), and hunk is a one-way renderer (`--agent-context notes.json`) or a
read-only `--watch` mirror in a side pane that the README describes and cockpit never
manages. Guards phase 4 must carry: `--history-dir` under cockpit's session dir (revdiff #331
keys history by repo basename); `$EDITOR` re-injected into the popup env; for hunk, capture
the sessionId at launch and probe the port (hunk #527 — an empty `session list` is
indistinguishable from a dead daemon), select by `paneId` not `--repo`, always `--type user`,
reap only PIDs cockpit spawned by resolved binary path. Decision #4 stands.

A companion search (`hunk-revdiff-together-appendix.md`) found seven real setups with both
installed and exactly one written rationale (butaosuinu/fanout: hunk = resident bidirectional
session CLI, revdiff = zero-dependency single binary with exit-code branching — the same split
this synthesis reached). The cleanest deliberate pairing (afontaine's nix config) binds
`prefix+r` → revdiff to annotate and a second key → hunk as a split reader: Config A in the
wild. The one "tried both, kept one" write-up (zimakki: "two overlapping Agent Skills competing
for the same review requests") is the N11 hazard verbatim; the only conflict report on record
is two `ExitPlanMode` interceptors (revdiff #269), confirming N3. herdr-pickr's "reviewers whose
tools aren't installed are hidden" chooser is the shape `@cockpit_reviewer` should take.

**Front-end steals from hunk for the fzf fallback (ranked):** `c` comments / `/` filters as
separate modes — dissolves the dead-Enter class instead of documenting it (S); file-status
vocabulary + per-file ± on header rows (S); unshifted three-level motion incl. `{`/`}`
prev/next comment once `●N` exists (S/M); two aligned line-number gutters that survive 90
cols (S); a `source` field on comments (`user|agent|ai`) now, as the prerequisite for `--seed`
(S); `z` context toggle over cached rows (M); `e` open-at-anchor (S); the agent-context
sidecar schema `{filePath,newLine,summary,rationale}` (M); a SKILL.md whose first line tells
the agent which half of the tool is *not* for it (S); record `pane_id`/`tty`/`repoRoot` in the
phase-2 session dir (S). Do not copy: the daemon, in-memory-only state, `^S` meaning "save one
note" (cockpit's `^S` must keep meaning deliver), mouse-first menus, `npm i -g hunk` in docs.

## 7. Target design

Five pieces, each independently shippable, each respecting the invariants except where a
narrow relaxation is argued.

### 7.1 One artifact in the middle (dev R1/R4/R5)

Replace `store=$(mktemp)` with a session dir:

```
$XDG_STATE_HOME/tmux-cockpit/review/<repo-hash>-<branch-slug>/
  meta          root, pane, head sha, base, taken-at
  diff.tsv      the SNAPSHOT: id, file, hunk, side, line, offset-in-file-diff, text — written once
  comments.md   append-only, revdiff grammar (full: line, range, file-level), parse() = inverse of format()
```

`rows`, `annotate`, `format`, `parse` become pure functions of two files — no git, no tmux, no
fzf — testable in milliseconds with golden fixtures. The picker reads `diff.tsv`, so defect #1
cannot happen; `^R` re-snapshots explicitly and reports which comments went stale (GitHub's
"outdated"). A dropped client no longer loses the review (difit #448, revdiff's history dir).
File layout: `scripts/tmux-review` (~150 lines, tmux/fzf glue) + `scripts/lib/review-diff.awk`
+ `scripts/lib/review-store.sh` (the one module that knows what `## path:line (+)` means).

*Invariant note:* "state dies with the pane" is about processes and GC, not files — the repo
already writes a spillover file that outlives the pane with no GC (`:515-527`). A named,
resumable, bounded session dir is a tightening, not a daemon.

### 7.2 Diff base and the reviewed checkpoint (when-how §2)

```
review_base(dir):
  1. refs/cockpit/reviewed/<branch-slug> exists      → base = that ref     [since my last review]
  2. else upstream exists                            → merge-base @{u} HEAD [branch + uncommitted]
  3. else default branch exists                      → merge-base main HEAD
  4. else                                            → HEAD                 [uncommitted — today]
```

Always `git diff <base>` against the working tree, so staged + unstaged are one set (fixes #3),
plus `git ls-files --others --exclude-standard` synthesised as `+` rows (fixes #4), with `-M`
for renames. The base is shown in the popup title; `^B` cycles it (revdiff's "it asks"). `^A`
approve writes the ref with one `git update-ref` (~1 ms, GC'd by `git worktree remove`/
`branch -D`) — the narrowest relaxation of the invariant, and it *replaces* the un-GC'd temp
file. Purist fallback: `@cockpit_reviewed_sha` per pane, losing the delta only on pane death.

### 7.3 The review moment and the loop (when-how §1, §3)

A new per-pane option `@cockpit_review` ∈ {pending, sent, unset} — **not** a fifth `@panestate`
value (its ranking is load-bearing for the sticky guard and `@cockpit_win_needs`). A second
`Stop`-hook command, `scripts/review-gate`, sets `pending` when the delta vs base is non-empty
(one `git diff --quiet`, bounded, fail-open, never blocks). A precomputed `@cockpit_reviews`
counter renders `◆N` in the fleet segment exactly as `@cockpit_fleet` does today (freeze
discipline: compare-then-set at transition time, never a live pane walk).

```
clean ──(edits)──▶ dirty ──Stop, delta≠∅──▶ review-pending ◆ ──prefix r──▶ review-open
                                                 ▲   │                          │  │
                                                 │   └── PreToolUse git commit  │  │ ^S deliver
                                                 │       (opt-in gate: deny)    │  ▼
                                                 │                              │ addressing (sent)
                                                 │                              │  │ agent's next Stop
                                                 │        esc keeps the spool   │  ▼
                                                 └──────────────────────────────┴ re-review-pending (base = checkpoint, ↺ on prior rows)
                                   ^A approve: git update-ref refs/cockpit/reviewed/<branch>; unset @cockpit_review → clean
```

Trigger mix: **(a) non-blocking `Stop` flag — default; (d) on-demand — always; (b) `PreToolUse`
`Bash(git commit *)` deny while pending — opt-in, nobody in the field does it; (c) agent-invoked
`/cockpit-review` skill that blocks on the popup and reads stdout, exit 10 — opt-in;
(a′) blocking `Stop` gate — opt-in, documented as fleet-hostile.** Anthropic's own product
ships a three-mode trigger matrix per repo; "re-review convergence" (suppress nits after round
one) and a five-nit cap are its named rules.

### 7.4 Delivery that lands (ai §7.A, when-how §4)

Hybrid, chosen atomically by the `if-shell -F` that already exists at `:558`, extended with
`@panestate`:

- pane idle (`done` / unset) → today's bracketed, unsent paste (the only mechanism that works
  when the agent never asked) — now with a **persistent** `REVIEW ↵` badge on that pane via
  `@cockpit_review=sent`, and `prefix J` cycling to it, instead of a 750 ms flash;
- pane `running` / `needs-input` → arm `@cockpit_review_pending=<spool>`; `scripts/review-gate`
  on the agent's next `Stop` clears the option *first* (codex-plugin-cc #248 rewake loop), then
  hands the spool back as the block reason / exit 2 + stderr. Zero keystrokes, zero pane
  switch, turn boundary by construction. Verify on the installed build that `Stop` surfaces
  stderr to the model; else use the JSON form.

Never `send-keys Enter`. The `^S` is the consent gesture; requiring a second one in another
pane is where reviews evaporate. Rewrite the compose header (ai §7.F): scope fence, questions
first, dissent licence, "trust the quoted fragment, not the number", and a close-out line per
comment (`path:line — done | skipped: reason`) so the next review can key on it. Sort comments
by path/line, not pin order. Spool under `.git/cockpit/` (inside the trusted root, dies with
the worktree) and let `reason` carry the whole text — the 2048-byte pointer fork goes away on
the hook path.

### 7.5 The picker, fixed inside fzf 0.44.1 (ui §3, ux §6)

All verified available at the installed version: `↵` attaches (empty query = no-op, so
"no accept-all" holds exactly; `alt-↵` stays as alias); `^E` → `$EDITOR` for multi-line
(re-inject `EDITOR` via `display-popup -e`, popup children inherit a server-era env);
`^X` delete under cursor, `@` nested fzf over the store, `●N` counts; `esc` keeps the spool,
`^X`-confirm discards; preview follows the cursor via a 5th field and
`--preview-window '+{5}-/2'`; `down,55%` split with `lrows = min(nrows, avail/2)`; `--ansi`
colour with padded gutters (marker │ old │ new │ code) and file-header rows carrying the path
once (and accepting file-level comments); two-line `--header-first`; `--info=hidden`;
`change-preview-label` naming the target row; `n`/`p` file jump or a two-level file→lines
list; `\x1f` delimiters end to end; width-derived fragment cap with `…`, untruncated line to
the store; `reload-sync`. Fleet queue: project + branch columns, `printf '%-*s'` padding, state
+ idle columns, sort `needs-input > done > error > running` with churn as tiebreak; `prefix r`
**always** opens the queue with the current pane preselected; after delivery return to the
queue; `@cockpit_review_window on` opens a server-owned window instead of the modal popup.

What fzf cannot do at any version — side-by-side, word-diff, context in the list, wrap,
visual ranges, a comment gutter — is the case for **`@cockpit_reviewer = fzf | revdiff | tuicr |
hunk`**: run the external reviewer in the popup/window, pipe its stdout (revdiff's grammar is
ours; tuicr has `--stdout`; hunk `session comment list --json`) through `parse`, hand the
result to `deliver` unchanged. The seam is a pipe, not a rewrite; lazygit's `pager`, tig's
`diff-highlight` and forgit's `FORGIT_PREVIEW_PAGER` are the precedent. Ship a
`skills/cockpit-review/SKILL.md` so the agent knows the grammar, the `??` rule, the close-out
format, and the timeout-recovery rule (revdiff's SKILL.md verbatim) — 4/5 winners ship one.

## 8. Roadmap

Each phase ships green on its own. Costs are the lenses' S/M/L; days are rough.

| Phase | What | Cost | Fixes |
|---|---|---|---|
| **0 — bugs** (~1 day) | `@panestate` in both delivery guards · `git diff <base>` + untracked rows (drop the 4× fallback) · `\x1f` delimiters · `↵` attaches, alt as alias · `esc` keeps spool, confirm on discard · two-line header · `--ansi` sign colour · width-derived fragment cap, full line stored · `core.quotePath=false` in `--preview-diff` · persistent `REVIEW ↵` badge + `tmux-jump` cycles it · fleet rows show repo + branch, sort by state | S×11 | #2 #3 #4 #5 #6 #8 #9 #11 #13 #14 |
| **1 — perf** (~1–2 days) | numstat-first cap on bytes+rows with pathspec excludes · awk-only row emission (prototype byte-identical on 6 fixtures) · cached rows + awk re-mark on reload · preview memoised per file, one `git diff`, follows cursor · `xargs -P` fleet scan with `--no-optional-locks`, dedup by root, no `grep` fork · batch `agent-state`'s tmux calls with `\;` (20.3 → 3.9 ms) · truncate before ctrl-strip | S/S/S/S-M/M/S/S | 5.7 s→0.87 s cap; 1 s→20 ms per comment; 450→110 ms open; 2.5 s→350 ms fleet |
| **2 — artifact** (~2–3 days) | session dir + `diff.tsv` snapshot · `review-store.sh` with `parse` = inverse of `format`, full grammar (ranges, file-level), round-trip property test · split into glue + awk + store · `^R` re-snapshot with stale report · `^X`/`@`/`●N` · `^E` editor · golden `.tsv` fixtures, de-flake 25 sleeps | M | #1 #10 #12 + comment edit/delete/list |
| **3 — the loop** (~3–4 days) | `review-gate` on `Stop` → `@cockpit_review pending` + precomputed `◆N` · Stop-hook drain delivery, paste when idle · rewritten compose header + close-out grammar · base resolution + `^B` · `^A` approve → `refs/cockpit/reviewed/<branch>` · re-review delta with `↺` · `prefix r` always the queue, loop back after delivery · `@cockpit_review_window` · `/cockpit-review` skill + `--request` (exit 10) · SKILL.md · stash `transcript_path`/`session_id` in pane options from the hook payload; fleet rows and tabs read `ai-title` (9 ms) instead of trusting `pane_title`; pre-warm the queue's git numbers at the `done` transition | M | #7 + "when" + "how" + TCSM steals |
| **4 — delegate** (~2 days) | `@cockpit_reviewer` backend switch (revdiff / tuicr / hunk), stdout → `parse` → `deliver` · `--seed` for agent-authored comments (`◆` marker) · opt-in `PreToolUse git commit` gate · opt-in AI first pass at the pending transition, `[ai:severity]` rows first, five-nit cap, never blocks the popup · `delta`/`difftastic` in `--preview-diff` when present · hunk-level rows with line drill-in (or the two-level list) | M/M/M/M/S/L | side-by-side, ranges, wrap, gutter — free |
| **5 — decision gate** | Only after 0–4, with the perf argument gone: is a first-party diff TUI worth ~15 days + a six-platform release pipeline? If yes: one Go binary, bash loader, fzf as documented degraded mode. If no: done. | L | — |

## 9. Conflicts between lenses, resolved

- **AI first pass:** the AI lens argues against an LLM pass (self-bias, latency, cost; do
  local risk proxies first — assertion-only test churn, new-file flag); when-how and landscape
  show 3/5 winners pre-seed. Resolution: ship the **channel** (`--seed`, labelled `[agent]`)
  and the local proxies in phase 4; an automatic `claude -p /code-review` pass is opt-in, off
  by default, runs at the pending transition, never blocks `prefix r`, nit-capped at five.
- **Store durability vs "state dies with the pane":** all lenses want persistence; the
  invariant forbids daemons and un-GC'd files, not a bounded, addressable session dir. Adopt
  the session dir + one git ref; document the relaxation and what it replaces.
- **Auto-submit:** the AI lens proposes narrowing the invariant to "never send Enter into a
  `running`/`needs-input` pane"; when-how says never ship auto-Enter at all. Resolution: no
  `send-keys Enter` ever; the Stop-hook drain gives turn-boundary delivery without it.
- **Row ordering:** landscape wants spatial (tree) order; the antipatterns doc wants tests
  first. Resolution: tree order within the list with file-header rows; tests-first as a
  **sort mode** (`s` toggles churn / tree / tests-first), and churn stays the fleet queue's
  tiebreak, where triage is the job.
- **Language:** dev, perf and rewrite agree: bash + awk now; Go only for a TUI, only later,
  only on product merits. Rust's 36 ms edge over awk at the cap is imperceptible.

## 10. Decisions for the owner

1. Accept the two narrow invariant relaxations (session dir under `$XDG_STATE_HOME`; one git
   ref per approved review)? The purist fallbacks are named in §7.1–7.2.
2. Non-modal review by default (server-owned window, fleet stays visible) or keep the popup
   default with `@cockpit_review_window` opt-in?
3. `prefix r` becomes "always the fleet queue, current pane preselected" (recommended) — or keep
   the two-entry resolution?
4. Which external reviewer to wire first as `@cockpit_reviewer`? **Answered by §6d: revdiff
   first, hunk second as a documented opt-in (it runs a daemon), tuicr third.** Confirm or
   override.
5. Fund phase 5 at all? Everything the owner asked about — best practice, design, performance —
   is answered by phases 0–4 without it.
6. Add plan-mode review (intercept `ExitPlanMode`, annotate the plan in the same popup, return
   annotations as the deny reason) to phase 3 — the one workflow real users volunteered? If
   yes, it must detect an installed `revdiff-planning` and step aside (§6d).
7. Bring the `peter-fork` TCSM work (rows.awk, contract tests, ai-title, grouping, the
   background-agents row) home into cockpit's fleet queue, where it shares the state model and
   the suite — rather than keeping it downstream of a repo that closed it unmerged?

## 11. Sources

The nine lens reports (with their own source lists) under
`docs/superpowers/research/2026-08-24-review-lenses/`; prior research
`2026-08-24-review-tools-landscape.md` and `2026-08-24-review-antipatterns.md`; the shipped
plan `docs/superpowers/plans/2026-08-24-review-flow.md`; perf harness and prototypes under
`…/review-lenses/prototypes/`.
