# Lens: can hunk and revdiff work *together*?

Question from the owner, verbatim intent: *"but hunk and revdiff can work TOGETHER, no? check in the
community why / why not, and what not to do."*

Answered against the synthesis §6d / §7.5 (`docs/superpowers/research/2026-08-24-review-redesign.md`),
the hunk lens §5–§9 and the landscape lens §2.1–2.2. Everything below tagged **[E]** was observed in
a throwaway tmux server (`tmux -L cbtest`, tmux 3.7c) with revdiff `v1.12.0-ad8c796` and hunk
`hunkdiff@0.19.0` driven live, or read out of `git clone --depth 1` of `umputun/revdiff` and
`modem-dev/hunk`, or fetched with `gh`. **[I]** is inference.

**Verdict up front.** *Together* is real, but only in one shape and only asymmetrically:
**revdiff annotates-and-delivers; hunk reads and never delivers.** The symmetric configuration —
both installed as reviewers, both harvested — is a trap, and the trap is not philosophical: two
concurrent `display-popup -E` calls on one tmux client make the loser **exit 0 with empty stdout**,
which every consumer in this ecosystem (revdiff's launcher, cockpit's `^S`, plannotator's hook)
reads as *"the human reviewed and had nothing to say."* A silent false-clean review is the worst
possible failure for a review tool, and installing both without a lock manufactures it.

The community reached the same place independently. The one dotfiles repo I found running both
splits them exactly this way — revdiff feeds the agent, hunk opens a split pane (§1.4a) — and the
herdr ecosystem's answer to "several reviewers installed" is `herdr-pickr`, a **chooser** that runs
exactly one per review (`[t] tuicr [h] hunk [d] diff [o] browser`), which is `@cockpit_reviewer`
already shipped by someone else (§1.4b). **So: keep the single-valued backend switch; do not add
`@cockpit_watcher`; do add revdiff's `--annotations` round-trip, which turns the switch from a pipe
into a two-way channel for free (§2.1).**

Verified project facts, today [E] (`gh api repos/OWNER/REPO`):

| repo | stars | pushed |
|---|---|---|
| `modem-dev/hunk` | 8,714 | 2026-08-24T18:47Z |
| `umputun/revdiff` | 808 | 2026-08-20T19:04Z |
| `agavra/tuicr` | 2,868 | 2026-08-20T16:07Z |
| `backnotprop/plannotator` | 7,988 | 2026-08-24T16:34Z |
| `yoshiko-pg/difit` | 3,095 | 2026-08-09T06:45Z |
| `herdrdev/herdr` | 32,039 | 2026-08-24T01:52Z |

---

## 1. Does anyone actually run both?

### 1.1 The two projects do not know each other exists

[E] Neither repo references the other, in either direction, anywhere:

```
$ grep -rniE 'revdiff|tuicr|difit|plannotator|herdr' hunk --exclude-dir=.git | wc -l
4                       # all four hits are `herdr` (an extension-install example
                        # and two benchmark docs); zero for revdiff/tuicr/difit/plannotator
$ grep -rniE 'modem-dev|hunk\.dev|hunkdiff' revdiff --exclude-dir=.git | wc -l
0
```

No "works with", no interop flag, no shared schema, no cross-link. Both ship a *complete* review
loop and both assume they are the only one [I].

### 1.2 The one maintainer-level statement is a refusal to be compared, let alone combined

[E] `modem-dev/hunk` issue **#275**, "question: pls compare/contrast with revdiff", opened
2026-05-10 by `webwesen`, body one line linking `https://github.com/umputun/revdiff`. Closed
2026-06-13 by `benvinegar` (hunk maintainer):

> I'm going to close - revdiff is a cool project, but I want to avoid having a giant infinite
> matrix of comparisons.

[I] This is the whole community posture in one sentence. Users frame these as **alternatives to
choose between**; the maintainers decline even to position them, never mind compose them. There is
no upstream story for running both, and there will not be one.

### 1.3 Search results

[E] Direct code search for the pair returns nothing:

```
$ gh search code 'revdiff hunk'                       → []
$ gh search repos 'revdiff hunk review'               → []
$ gh search code 'hunk-review revdiff'                → (empty)
$ gh search code '"hunk skill path" revdiff'          → (empty)
$ gh search code 'path:.claude/settings.json revdiff' → (empty)
$ gh search code 'revdiff-planning hunkdiff'          → (empty)
$ gh search code 'tuicr revdiff' / 'tuicr hunkdiff' / 'difit revdiff' → (empty)
```

(Control queries confirm the method works: `gh search code 'revdiff'` alone returns five unrelated
repos; a later `hunkdiff` query hit the 403 rate limit, so treat the single-term negatives as
partial [I].)

### 1.4 …but the herdr ecosystem has already run the experiment

The pairing does not exist as a *product*, but the herdr plugin ecosystem — 36 plugins in its
"code review / diff" category alone (`MIDO-ruby7/herdr-plugins-directory`, 10★) [E] — has already
run the experiment at scale, and it converged on one answer: **one reviewer at a time, chosen by a
router; hunk is a surface, not a second reviewer.** Six pieces of evidence.

**(a) One real dotfiles repo runs both, with the roles split exactly as §4 Config A.** [E]
`afontaine.dev/nix`, commit `bb070dd` ("pi: integrate herdr with revdiff, declarative plugins and
stylix theme", tangled.org) declares both, as separate nix modules:

```nix
programs.andrew-pi.revdiff.enable = true;
revdiff = pkgs.callPackage ./revdiff.nix { };
REVDIFF_BIN = lib.getExe revdiff;
...
herdrPluginHunk = pkgs.fetchFromGitHub { ... };
# python3 + hunk plugin (hunk on PATH via programs.hunk)
command = "hunk.diff.worktree-split";
```

The jobs are cleanly divided: revdiff is wired into the *pi coding agent* — it "launches an
interactive annotation pass and feeds annotations back to the agent," bound to a hotkey to "review
working changes" [E]. hunk is bound to a **herdr plugin command, `hunk.diff.worktree-split`** — a
split-pane *view* action [E]. **Nobody harvests hunk. revdiff owns the agent loop; hunk owns the
pane.** This is the only concrete "both installed" instance I could find, and it independently
lands on the recommendation in §4.

**(b) When several reviewers are installed, the community builds a router, not a merge.** [E]
`tomasvarga/herdr-pickr` (14★, 2026-07-13) is literally `@cockpit_reviewer` as a plugin:

> "Ctrl+click a PR/MR link in any pane and pick how to review it — **tuicr, hunk, a plain diff, the
> browser, or your own tool**." · chooser popup: `[t] tuicr [h] hunk [d] diff [o] browser` ·
> "the chosen reviewer opens in a herdr pane" · "reviewers whose tools aren't installed are hidden;
> the browser always works." · "**Auto-route** — skip the chooser and go straight to a default;
> toggle with a keybind."

Exactly one reviewer per review; backends declared as config blocks (`key`, `id`, `name`, `run`,
`where`, `needs`, `platforms`); unavailable backends hidden. [I] This is direct validation of the
`@cockpit_reviewer = fzf | revdiff | hunk | tuicr` switch **and** of the verdict: the field's answer
to "two reviewers" is *choose one, per review*, with a configured default.

**(c) The one shipped hunk-harvest plugin confirms every constraint this lens reproduced.** [E]
`jhochenbaum/herdr-hunk-diff` (87★, pushed 2026-08-22) — "collects your inline review comments, and
sends them back to the agent responsible for the changes" — documents, in its own README:

> "Sending is always explicit. **Closing a review ends its hunk session, so comments cannot be sent
> reliably after the pane has closed.**"
> "Sending comments is explicit through `send-review`; **closing a pane does not send them**."
> "Agent-authored notes require a **live** hunk session."
> "If hunk is visibly open, the agent sandbox may be blocking loopback access to `127.0.0.1:47657`."
> "The index stores **one delivery target per worktree**. If multiple agents share a checkout,
> invoke `send-review` from the intended agent pane or reopen the review from that pane."
> "`placement` supports the four persistent Herdr placements that return a pane ID. **Herdr's modal
> `popup` placement is intentionally excluded because it cannot be reused, addressed, closed, or
> reported through the pane APIs this plugin relies on.**"

[I] Four independent confirmations of this lens's own findings — the volatile session (§3.6), the
loopback dependency (N6), the per-worktree ambiguity (N7/§3.6), and — decisively — **a shipped
plugin that deliberately refuses to run hunk in a modal popup for the same reasons cockpit's popup
would fail** (§3.3, N2). Their solution is the one cockpit should copy for any hunk backend: a
`prefix+shift+s` **explicit send while the session is live**, with "unsent comments" tracked, rather
than a harvest-on-quit. Note also `auto_open = true, on_states = ["idle"]` — open/refresh the review
when an agent finishes a turn — which is cockpit's §8 phase-3 `Stop → pending` gate, arrived at
independently.

**(d) The watcher role is a shipped, popular pattern.** [E] `yuucu/herdr-hunk` toggles exactly
`hunk diff --watch` as an overlay ("closed → open, open elsewhere → reload and focus, focused →
close"), lets agents write into it (`hunk session comment add` / `comment apply`), and hooks
`pane.agent_status_changed` so that "when an agent reaches `done`, the plugin … reloads any live
Hunk session showing it." Its docs say nothing about session cleanup or daemon lifecycle [E] — [I]
nobody in this ecosystem cleans up the daemon; they accept it as ambient.

**(e) Redundancy is how the ecosystem itself frames it.** [E] `herdr-hunk-diff`'s "Prior art"
section on the 511★ `persiyanov/herdr-reviewr`: *"covers similar workflows with its own diff viewer.
This plugin is for users who specifically want **hunk's review UI** and live session API."* The
stated differentiator is the *reader*, not the loop — which is precisely why running two readers is
redundant (§2).

**(f) There is no herdr plugin for revdiff.** [E] Of the 36-plugin review category, at least eight
wrap hunk (`yuucu/herdr-hunk`, `cevr/herdr-hunk`, `goofansu/herdr-hunk`, `tareqmlx/herdr-hunk-viewer`,
`JacquesvanWyk/herdr-hunk`, `edmundmiller/herdr-plugin-hunk`, `scott306lr/herdr-plugin-hunk-autodiff`,
`Allianaab2m/herdr-hunk-gh-diff`, `quantk/herdr-review`), one wraps tuicr (`tomasvarga/herdr-sniffr`),
one wraps plannotator, one routes between them (`herdr-pickr`) — and **zero** wrap revdiff. [I]
revdiff needs no wrapper: its launcher already has a herdr branch built in
(`launch-revdiff.sh`, priority `agterm → tmux → Zellij → herdr → kitty → …`). That asymmetry is the
whole thesis in miniature — **hunk is a surface other tools mount; revdiff is a contract other tools
call.** Mounting a contract is redundant; calling a surface is what the daemon is for.

### 1.5 What the write-ups say when they name both

[E] The comparison content that exists is *substitutive*, not additive. Third-party round-ups
position them as a fork in the road, in the same sentence:

> "revdiff emphasizes structured, parseable output for agent loops, while Hunk emphasizes
> interactive multi-file navigation and watch mode for real-time review."
> — synthesis of `revdiff.com`, `hunk.dev`, `vibecodinghub.org/blog/hunk-review`, `toolhunter.cc`
> (WebSearch, 2026-08-24)

[I] That split is exactly the complementarity the owner is sensing — but nobody has *built* the
combination, and the tools' own designs actively resist it (§3).

---

## 2. Complementary or redundant? Surface-by-surface

The honest answer is **both, on different axes**. They are ~100% redundant as *readers* and
~0% redundant as *loops*.

| surface | revdiff | hunk | overlap |
|---|---|---|---|
| **Reading** | two-pane tree + diff, `--wrap`, `--word-diff`, `--blame`, `--collapsed`/`--compact`, themes, `--line-numbers` [E] | side-by-side/stack auto-layout, sidebar with per-file ± , context toggle, `--watch` fs-event auto-reload [E] | **~100% redundant.** Two full-screen diff readers. Running both means the human reads the same diff twice, in two different keymaps, with two cursors. |
| **Annotating** | line, **line-range** (`43-67`, auto-expanded by the word "hunk"), **file-level**; `^E` → `$EDITOR`; `@` annotation browser [E] | line (old/new), hunk-index; `c` note, rationale field, `--markup` STML (experimental) [E] | **~100% redundant.** Different grammars, same job. |
| **Grammar** | `## path:line[-end] (+\|-\|file-level)` + body, on stdout [E] | JSON: `{noteId, source, filePath, hunkIndex, oldRange/newRange, body, author, createdAt, editable}` [E] | redundant, incompatible |
| **Delivery** | **stdout / `-o` file, exit 10**; `--history-dir` durable auto-save; `O` mid-session flush [E] | **daemon-memory only**, read through `hunk session comment list --json` while the TUI lives; **no state file anywhere** [E, hunk.md §5.3, re-reproduced here] | **not redundant — opposite.** revdiff is one-shot and durable; hunk is live and volatile. |
| **Triggering** | skill (`/revdiff`), plus a *separate* plugin `revdiff-planning` = `PreToolUse`/`ExitPlanMode` hook [E] | nothing. No hooks, no plugin, no commands (§3.1) [E] | **no overlap at all** |
| **Agent→human** | `--description`/`--description-file` (info popup), `--annotations=` pre-seed [E] | `session navigate`, `highlight add --focus`, `--agent-context` sidecar, `comment add/apply` [E] | **hunk is strictly ahead.** Only hunk lets the agent *drive the human's viewport*. revdiff cannot. |
| **Human→agent** | the entire point: annotations are directives, `??` = question [E] | possible but off-label: `comment list --type user` [E] | **revdiff is strictly ahead**, and hunk's default `--type live` returns *zero* human notes (hunk.md §6.1) |
| **Persistence** | `~/.config/revdiff/history/<repo-basename>/*.md`, survives SIGHUP/SIGTERM [E] | none [E] | not redundant |
| **Process cost** | one static Go binary, no daemon, no port [E] | node wrapper + TUI + **daemon on 127.0.0.1:47657**, ~270–320 MB, daemon outlives the TUI [E] | — |

### 2.1 The two things only one of them can do

**hunk only:** a *live* reader. `--watch` re-renders on fs events while the agent is still writing
(reproduced: the sidebar went `2 files +4 −1` → `+6 −1` within ~6 s of an unrelated `git add`, and
a pinned note survived the reload and re-anchored) [E]. And agent-driven navigation — `session
navigate --file X --hunk 2`, `highlight add --start/--end --focus` — a channel revdiff has no
equivalent for [E].

**revdiff only:** a *contract*. stdout + exit 10 is cockpit's own `^S → exit 20 → deliver` idiom;
the review survives the reviewer (`--history-dir`, SIGHUP save); the plan-mode interception exists
and ships. And — new, and load-bearing for §4 — **the store round-trips**:

```
$ cat seed.md
## README.md:2 (+)
> changed
[agent] seeded from cockpit store

## parser.js (file-level)
[agent] file-level seed

$ revdiff --staged --annotations=seed.md --output=out.md   # human quits with q
$ diff seed.md out.md && echo IDENTICAL
IDENTICAL                                                                       # [E]
```

Byte-identical, **and cockpit's `> fragment` line survives untouched** as body text [E]. revdiff can
therefore be used as a pure *editor over cockpit's own store*, not just as a producer.

### 2.2 Two corrections to §6d

1. **[E] revdiff's grammar is NOT "already ours including the fragment line."** Its documented and
   observed output is `## path:line[-end] (type)` + body, with no `>` quoted-fragment line
   (`references/usage.md` "Output Format"; confirmed live — my annotation came back as exactly
   `## README.md:2 (+)\nrevdiff note ?? why changed`). *Both* candidate backends lose the fragment;
   cockpit must re-derive it from `git diff` by `(path, side, line)` for either. What revdiff does
   give, which hunk does not, is that it will *carry a fragment line back through* if cockpit seeds
   it (§2.1) — so the loss is one-way, not two-way [I].
2. **[E] revdiff's `??` rule is upstream, not a cockpit invention.** `usage.md`: "put `??` anywhere
   in the text, or open with `explain`, `remind`, `describe`, `what is`, …". Cockpit's `??`
   convention is already the field standard; keep it verbatim.

---

## 3. Mechanical coexistence: both installed, one Claude Code + tmux box

I installed both skills into a throwaway project (`proj/.claude/skills/{revdiff,hunk-review}/`) and
ran both binaries simultaneously against one repo in a throwaway tmux server.

### 3.1 Plugins and hooks: **no collision, by construction**

[E] Inventory of `umputun/revdiff@HEAD`:

```
.claude-plugin/plugin.json          name "revdiff", skills only, NO hooks key
.claude-plugin/marketplace.json     lists TWO plugins: revdiff (./) and revdiff-planning (./plugins/revdiff-planning)
.claude-plugin/skills/revdiff/SKILL.md + references/{config,usage,install}.md
                                    + scripts/{launch-revdiff,resolve-launcher,detect-ref,read-latest-history,agentdeck-window}.sh
plugins/revdiff-planning/hooks/hooks.json
    → PreToolUse, matcher "ExitPlanMode", command plan-review-hook.py, timeout 345600
plugins/{codex,opencode,pi}/…       other harnesses; opencode also ships commands/revdiff.md
```

[E] Inventory of `modem-dev/hunk@HEAD`:

```
$ find . -name '.claude-plugin' -o -name 'hooks.json' -o -name 'marketplace.json' -o -name 'plugin.json'
(nothing)
$ find . -type d -name commands
(nothing)
skills/{hunk-review,hunk-extensions,hunk-release,hunk-launch-video}/SKILL.md
```

**hunk ships no Claude Code plugin, no hooks, and no slash commands.** Its entire agent surface is
`skills/hunk-review/SKILL.md` (path printed by `hunk skill path`) plus an optional `hunk mcp serve`.

Consequences [I]:

- **No double Stop hook is possible today.** Neither project registers `Stop`. The only hook in the
  pair is revdiff-planning's `PreToolUse`/`ExitPlanMode`, and it is a *separate, opt-in* plugin
  (`/plugin install revdiff-planning@revdiff`) — installing `revdiff` alone installs no hooks.
- **No slash-command name collision.** Skill names are `revdiff` and `hunk-review`.
- **The collision cockpit can create is its own.** §10 #6 proposes cockpit intercept `ExitPlanMode`
  for plan review. That is the exact matcher revdiff-planning claims. Two `PreToolUse` hooks on one
  matcher both run, both would open a popup, and (§3.3) the second popup silently no-ops — so the
  user annotates one plan review while a second, invisible one returns empty. Gate cockpit's
  plan-mode hook on revdiff-planning not being installed, or don't ship it.

### 3.2 The skills contradict each other, in their first ten lines

This is the sharpest finding of the section. The two SKILL.md files describe **inverse loops**:

| | revdiff `SKILL.md` | hunk `skills/hunk-review/SKILL.md` |
|---|---|---|
| who opens the TUI | **the agent**: ":68 1. Launch revdiff in a terminal overlay"; ":137 The launcher blocks until the user finishes reviewing in the TUI overlay … Do NOT use `run_in_background`" [E] | **the user**: ":8 The TUI is for the user -- **do NOT run `hunk diff`, `hunk show`, or other interactive commands directly**"; ":10 If no session exists, **ask the user to launch Hunk** in their terminal first" [E] |
| who writes comments | the **human** ("Each annotation is a directive", ":222) [E] | the **agent** (`comment add` / `comment apply`; "Your role is to narrate: steer the user's view … and leave comments that explain") [E] |
| direction | human → agent | agent → human |
| `allowed-tools` | `[Bash, Read, Edit, Write, Grep, Glob]` [E] | not declared [E] |

And their `description:` frontmatter — the only thing in the agent's skill index — **overlaps on the
exact trigger phrase**:

- revdiff: *"Review diffs, files, and documents with inline annotations in a TUI overlay … Activates
  on "revdiff", "review diff", "review changes", "annotate diff", … "**interactive diff review**", …"* [E]
- hunk-review: *"Interacts with live Hunk diff review sessions via CLI. … Use when the user has a
  Hunk session running **or wants to review diffs interactively**."* [E]

[I] So on a box with both installed, "review my changes interactively" matches both descriptions,
and whichever the model picks it gets a *different, contradictory* instruction about who launches
what. Pick hunk and it will refuse to open anything and ask the user to launch a TUI by hand; pick
revdiff and it will block a Bash call for up to half an hour. There is no arbitration layer. This is
why cockpit's own `skills/cockpit-review/SKILL.md` (§7.5) must open by *claiming the review verb and
naming the others as out of scope* — §6d already recommends "a SKILL.md whose first line tells the
agent which half of the tool is not for it"; this is the concrete reason.

### 3.3 The overlay layer: **only one popup per client, and the loser lies**

Two independent processes each call `tmux display-popup -E` on the same client, 0.4 s apart [E]:

```
A-RAN
B rc=0          ← B's command NEVER RAN. no B-RAN, no B-DONE. exit 0.
A-DONE
A rc=0
```

Same result nested — a `display-popup -E` issued *from inside* a popup [E]:

```
OUTER start
OUTER after-inner rc=0     ← inner command never ran, no error, exit 0
OUTER end
```

[E, tmux 3.7c] tmux permits one popup per client; the second call is discarded **silently, with a
success exit status**. This is the mechanism that turns "both installed" into data loss:

- cockpit's popup (`cockpit.tmux:249`, `display-popup -E`) cannot shell out to
  `launch-revdiff.sh` — that script's tmux branch is itself `tmux display-popup -E`
  (`launch-revdiff.sh:235-243`). Nested → rc 0, empty `$OUTPUT_FILE`, and revdiff's own contract
  says *"exit 0 = no annotations"* → cockpit would report a clean review that never happened.
  **Cockpit must exec the `revdiff` binary directly inside its own popup**, which is what §7.5
  already implies; state it as a hard rule.
- If the agent invokes the revdiff skill while cockpit's review popup is open — entirely plausible
  when both a human and an agent are driving the same pane — the agent's review silently returns
  empty and it concludes the human approved.

Two mitigations, both cheap [I]: (a) a per-client flag (`tmux set-option -t <client> @cockpit_review_active 1`)
checked before opening and cleared on exit; (b) prefer `@cockpit_review_window` (a server-owned
window) over the popup whenever a second reviewer may exist — windows do not have the one-per-client
limit, and revdiff ships the same escape hatch as `REVDIFF_TMUX_WINDOW=1`.

### 3.4 Process, port and file level: **clean**

Ran hunk `--watch` in pane `%1` and revdiff in a separate window against the same staged diff,
concurrently [E]:

- port 47657 stayed bound by hunk's daemon for revdiff's whole lifetime; revdiff binds nothing,
  spawns nothing, writes only its `-o` file [E]
- a `display-popup` opened *over* the live hunk pane and closed again left hunk rendering correctly
  [E]
- revdiff quit with `rc=10` and the expected stdout; hunk was unaffected [E]

There is **no resource conflict**. The conflict is entirely at the *semantic* layer.

### 3.5 …but the two stores are disjoint, and that is the whole problem

One human, one repo, one file, one review session. Two comments [E]:

```
revdiff -o out.md:                    hunk session comment list --type user --json:
## README.md:2 (+)                    { "filePath": "README.md", "newRange": [1,1],
revdiff note ?? why changed             "body": "hunk note about parser", "source": "user" }
```

Neither tool can see the other's note. Different anchors (line 2 vs line 1 — two readers means two
cursors, and the human must re-navigate in each). **Harvest one and half the review is silently
dropped; harvest both and the agent gets two overlapping, differently-anchored review documents.**
There is no third option without cockpit owning the merge.

### 3.6 The daemon, re-reproduced, plus two findings the hunk lens did not reach

- **Orphan confirmed.** After `q`, port 47657 still `LISTEN`, daemon 95 MB RSS, `hunk session list`
  → "No active Hunk sessions.", `comment list` → "No active session matches repoRoot …". Process
  persists, data does not [E]. Killing the *tmux server* did not kill it either [E].
- **NEW: one daemon serves every repo, and it reuses the orphan.** Launching hunk in `repo` and
  `repo2` produced **one** `daemon serve` process — pid `2571054`, the *same orphan* left over from
  the previous review [E]:
  ```
  $ hunk session list        →  repo2 staged changes (pane %1) ; repo staged changes (pane %0)
  $ ps … | grep 'daemon serve' | wc -l   →  1
  ```
- **NEW: killing that daemon silently blinds every other live review.** I killed it while two TUIs
  were up. Both TUIs **kept running and kept displaying their notes on screen** — the human sees a
  healthy review — while `hunk session list` reported "No active Hunk sessions" and `comment list`
  errored [E]. A cockpit teardown `pkill` after *its* review therefore makes *another project's*
  in-progress review unharvestable, with zero visible signal to the human [E].
- **NEW: sessions self-heal, and then `--repo` breaks.** Launching a third TUI respawned the daemon
  and all three sessions re-registered — but now [E]:
  ```
  hunk: Multiple active sessions match repoRoot …/repo2; specify sessionId instead.
  ```
  `--repo` is unusable the moment two panes look at one repo — precisely cockpit's fleet situation.
  The fix exists: `hunk session list --json` carries `terminal.locations[].paneId` (`"paneId": "%1"`),
  so cockpit can key sessions by the pane it spawned [E].

### 3.7 Editor env and node

[E] A `display-popup -E` child's environment contains `TMUX`, `TMUX_PANE`, `TERM` — and **no
`EDITOR`/`VISUAL` at all**. revdiff's launcher solves this by prefixing `/usr/bin/env EDITOR=… VISUAL=…`
(`launch-revdiff.sh:85-99`, comment: *"overlay backends … spawn children from a server/app process
whose env predates user shell rc files"*). Cockpit exec'ing the binary directly inherits that
problem and must do the same, or use `display-popup -e EDITOR=…` (§7.5). [E] hunk's npm wrapper
additionally needs `node` on `PATH` — without it the pane just prints
`/usr/bin/env: 'node': No such file or directory` and returns to a prompt.

---

## 4. Three "together" designs for cockpit, costed

Common ground for all three: cockpit owns `comments.md` in its own grammar (§7.1) and `deliver`
stays the single delivery path.

### Config A — revdiff reviews, hunk watches (**the only shape that works — as documentation, not a feature**)

hunk runs `--watch` in a persistent pane or window beside the agent, as a **read-only live mirror**
of the working tree; cockpit never reads its comments. `prefix r` still opens cockpit's popup, runs
the `revdiff` binary directly, parses `## path:line (type)` off stdout/`-o`, delivers.

- **Buys:** the live side-by-side reader fzf can never be, updating as the agent writes (proven:
  `+4 −1` → `+6 −1` on an fs event, notes re-anchored across the reload [E]); zero change to the
  review path; revdiff's no-daemon, exit-10 contract intact.
- **Costs:** a daemon + ~270 MB for the whole session (amortised, not per review); `node` on PATH;
  two keymaps live at once (hunk `c`/`[`/`]`/`,`/`.`/`z` vs revdiff `a`/`[`/`]`/`n`/`p`/`v`; `{`/`}`
  means prev/next *comment* in hunk and prev/next *annotation* in revdiff — close enough to be
  confusing, not close enough to be muscle memory).
- **Failure modes:**
  - **F1 (certain, §3.5): the human types `c` in the watcher.** That note never reaches the agent.
    Mitigation is documentation plus, ideally, launching the watcher in a mode where commenting is
    visibly not the path — there is no `--no-comments` flag, so this is a docs-only guard [E].
  - **F2 (§3.6): never `pkill` the daemon.** With a watcher alive, cockpit must not clean up after
    hunk at all. Which means Config A *permanently* relaxes "no daemon" for the whole session.
  - **F3 (§3.6): `--repo` ambiguity** if any hunk-backed review also opens on that repo. Key by
    `paneId`.
- **Precedent [E]:** this is a shipped pattern. `yuucu/herdr-hunk` toggles exactly `hunk diff
  --watch` as an overlay and reloads it on `agent_status_changed → done`; `afontaine.dev/nix` runs
  it next to revdiff with the same division of labour (§1.4a, §1.4d).
- **[I] Recommendation: offer it as documentation, not as a feature.** Cockpit's contribution here
  would be a pane-spawn helper; that is a generic `@cockpit_watcher_cmd` at most. Owning a hunk
  lifecycle means owning a daemon, and the invariant is not worth spending on a convenience.

### Config B — `@cockpit_reviewer=hunk` (the §6d "second backend, opt-in")

Unchanged from hunk.md §7–§8: poller while the TUI lives, `--type user` mandatory, fragment
re-derived from git, daemon killed by resolved binary path afterwards. Adds one new blocker found
here: **hunk #527** (open, 2026-07-11) — `hunk session list` returns the same message and exit 0
whether the daemon is down or there are genuinely no sessions, `--json` included. A poller that
loses the daemon mid-review therefore harvests **zero comments and reports success**. Any hunk
backend must capture the sessionId at launch and probe the port, not trust the CLI's empty result.

**Two corrections from the field (§1.4c), both of which cockpit should adopt if it ever ships this:**

1. **Don't harvest on quit; harvest on an explicit key, while the session lives.** `herdr-hunk-diff`
   (87★) uses `prefix+shift+s` = `send-review`, tracks *unsent* comments, and states outright that
   "closing a pane does not send them" [E]. That is strictly better than a 0.4 s poller: no CPU
   burn, no race with teardown, and the human's `^S` already means "deliver" in cockpit. The popup
   would then have to be non-modal (below) so `^S` can be pressed with the reviewer still up — or
   `^S` must be a key hunk forwards, which it is not.
2. **Don't run hunk in a modal popup at all.** `herdr-hunk-diff` *intentionally excludes* herdr's
   modal popup placement "because it cannot be reused, addressed, closed, or reported through the
   pane APIs this plugin relies on" [E]. Every one of those properties is true of tmux
   `display-popup -E` as well (§3.3 popup-in-popup; hunk.md §6.6 not `capture-pane`-addressable).
   A hunk backend implies `@cockpit_review_window` is **mandatory**, not optional.

### Config C — one store, two renderers ("the real composition")

Cockpit's `comments.md` is the contract; each backend is a renderer over it.

- revdiff: `--annotations=comments.md` in → `-o comments.md` out. **Proven byte-identical, fragment
  line preserved** (§2.1) [E]. This closes the loop *losslessly today* and is the single most
  valuable finding in this lens: it makes `--seed` (§8 phase 4, agent-authored `[agent]` rows) free
  on the revdiff path, and it means a cockpit review can be handed to revdiff, edited, and handed
  back with no schema translation at all.
- hunk: `--agent-context notes.json` renders cockpit's rows beside the code (schema
  `{version, summary, files:[{path, summary, annotations:[{newRange|oldRange, summary, rationale, author}]}]}`
  [E]) — but it is **read-only**; edits come back only through the live daemon in a different schema
  with no fragment. The loop is half-closed.

[I] So Config C is real *for revdiff* and aspirational for hunk. Ship it as: cockpit's store is the
artifact; revdiff is a bidirectional editor over it; hunk is a one-way renderer of it.

### Verdict on "together"

**Yes as a documented pairing (Config A + Config C's revdiff round-trip); no as a product feature,
and emphatically no as "two reviewers".**

- Offer `@cockpit_reviewer = fzf | revdiff | hunk | tuicr` exactly as §6d decided — **one** value,
  **one** harvest path, **one** delivery. The field already built this: `herdr-pickr`'s
  `[t] tuicr [h] hunk [d] diff [o] browser` chooser with hidden-if-not-installed backends and an
  auto-route default is the same design, shipped (§1.4b) [E]. Steal two details from it: **hide
  backends whose binary is absent** (`command -v`, not an error at launch) and **allow a per-repo
  override of the default** — cockpit already has the option namespace for both.
- Do **not** ship `@cockpit_watcher`. Document "you can run `hunk diff --watch` in a side pane; it
  is a reader, its comments are not collected" in the README next to the backend switch. That
  captures 100% of the benefit at 0% of the daemon-lifecycle cost [I].
- Do ship the revdiff round-trip (`--annotations` in, `-o` out). It is one flag, it is proven, and
  it turns the backend seam into a genuine two-way channel instead of a one-way pipe.

---

## 5. What NOT to do

Ranked by how quietly it fails. Every one has a reproduction or a citation.

| # | Don't | Evidence |
|---|---|---|
| **N1** | **Don't let two tools deliver into the agent.** One review, one store, one paste. | §3.5 [E]: same file, same session, two disjoint stores, different anchors. Harvest one → half the review vanishes; harvest both → the agent gets two overlapping documents. |
| **N2** | **Don't open a popup while a popup is open — and never call `launch-revdiff.sh` from inside cockpit's popup.** The second call exits **0** with empty output, which every consumer reads as "clean review". | §3.3 [E]: `B rc=0` with `B-RAN` never printed; nested case identical. `launch-revdiff.sh:235` is itself `display-popup -E`. Independently confirmed upstream: `herdr-hunk-diff` *"intentionally excluded"* herdr's modal popup placement *"because it cannot be reused, addressed, closed, or reported through the pane APIs"* [E]. |
| **N3** | **Don't add a cockpit `ExitPlanMode` hook while `revdiff-planning` may be installed.** Two `PreToolUse` hooks on one matcher both fire; the second popup silently no-ops; either can `deny`. | `plugins/revdiff-planning/hooks/hooks.json` [E] + N2 [E]; §10 #6 is the proposal at risk [I]. |
| **N4** | **Don't `pkill -f` on words.** Ever. | Reproduced twice: the hunk lens killed its own session (exit 144); **I killed mine the same way** running `pkill -f 'hunkdiff.*daemon serve'` [E]. At teardown, with **zero** hunk processes alive, `pgrep -fa hunk` still returned two shells and `pgrep -fa hunkdiff` returned one — all of them unrelated monitor shells whose cmdline happened to contain the path `hunk-revdiff-together.md` [E]. hunk #527's own repro instructs `pkill -f hunk` [E]. Kill by resolved binary path (`pgrep -f "^$BINPATH"`), and only PIDs you spawned. |
| **N5** | **Don't kill the hunk daemon at all when anything else may be using it.** One daemon serves every repo on the box, and killing it leaves other people's TUIs **on screen and unharvestable**. | §3.6 [E]: one `daemon serve` for two repos; after the kill both TUIs still rendered their notes while `session list` said "No active Hunk sessions". |
| **N6** | **Don't read "No active Hunk sessions" / exit 0 as "the human had no comments."** | hunk **#527** (open, 2026-07-11, `sambatia`): *"With no daemon running … `hunk session list` prints `No active Hunk sessions.` and exits 0 — byte-identical to the healthy daemon-up-but-zero-sessions case. `--json` mode is equally ambiguous."* [E] |
| **N7** | **Don't select hunk sessions by `--repo`.** | §3.6 [E]: `Multiple active sessions match repoRoot …; specify sessionId instead.` Use `session list --json` → `terminal.locations[].paneId` [E]. |
| **N8** | **Don't call `hunk session comment list` without `--type user`.** The default returns agent comments only — an integration that used the documented default delivers **zero** human notes while appearing to work. | hunk.md §6.1 [E], SKILL.md:  *"`comment list --type user` shows human-authored inline notes; without `--type`, `comment list` preserves the legacy live-agent-comment view"* [E]. |
| **N9** | **Don't let revdiff's default history dir be a recovery path across repos.** Pass an explicit `--history-dir` under cockpit's own per-repo session dir. | revdiff **#331** (open, 2026-08-20, `paskal`): *"Review artifacts are keyed by repository basename and wall-clock time, so reviews can read or overwrite each other."* `app/history/history.go:107` uses `filepath.Base(p.Path)`; `read-latest-history.sh` prints that dir's newest file, so standing in `~/review/proj` the agent is handed `~/code/proj`'s annotations [E]. |
| **N10** | **Don't assume `$EDITOR` exists in the popup.** | §3.7 [E]: popup env has `TMUX`, `TMUX_PANE`, `TERM` and nothing else. revdiff's launcher re-injects via `/usr/bin/env`; cockpit exec'ing the binary must too. |
| **N11** | **Don't ship a review SKILL.md that doesn't claim the verb.** With both installed, "review my changes interactively" matches both descriptions and the two skills give opposite instructions. | §3.2 [E]: hunk *"do NOT run `hunk diff` … ask the user to launch Hunk"* vs revdiff *"Launch revdiff in a terminal overlay … the launcher blocks"*. |
| **N12** | **Don't scrape the reviewer's rendered output.** Popup panes are client-side overlays and are not `capture-pane`-addressable. | hunk.md §6.6 [E]. |
| **N13** | **Don't document `npm i -g hunk`, and don't assume `node` is on PATH.** | hunk.md §1 (the npm name `hunk` resolves to an unrelated 2013 package) [E]; here, the wrapper failed with `/usr/bin/env: 'node': No such file or directory` [E]. |
| **N14** | **Don't expect per-pane isolation of hunk's agent API.** Only the TUI is per-pane; the daemon is global and long-lived. | §3.6 [E]. |
| **N15** | **Don't harvest hunk on quit.** Its session — and every comment in it — ends with the TUI. Deliver on an explicit key while the review is still up. | `herdr-hunk-diff` [E]: *"Sending is always explicit. Closing a review ends its hunk session, so comments cannot be sent reliably after the pane has closed."* Reproduced here: §3.6 [E]. |
| **N16** | **Don't run two full-screen readers and call it a feature.** Two cursors, two keymaps, two `[`/`]` that agree and `,`/`.` that don't, and the human re-navigates the same diff twice. | §2 table [E]; the anchor divergence in §3.5 is what it looks like in practice [E]. |

---

## 6. Sources

- Live lab, this session: `tmux -L cbtest` (tmux 3.7c), `revdiff v1.12.0-ad8c796-20260804T171719`,
  `hunkdiff@0.19.0`; all artifacts under `…/scratchpad/together/lab/` (torn down — `ss -ltnp | grep
  47657` closed, no `hunkdiff` processes, tmux server killed).
- `git clone --depth 1 https://github.com/umputun/revdiff` — `.claude-plugin/`,
  `plugins/revdiff-planning/`, `.claude-plugin/skills/revdiff/{SKILL.md,references/usage.md,scripts/launch-revdiff.sh}`.
- `git clone --depth 1 https://github.com/modem-dev/hunk` — `skills/hunk-review/SKILL.md`,
  `docs/agent-workflows.md`, `examples/3-agent-review-demo/agent-context.json`.
- GitHub: hunk #275, #527, #115; revdiff #331, #262, #324; `gh api repos/*` star/push counts.
- Community/ecosystem (all star counts verified with `gh api`, 2026-08-24):
  `MIDO-ruby7/herdr-plugins-directory` (10★) review category;
  `tomasvarga/herdr-pickr` (14★, 2026-07-13) — the reviewer router;
  `jhochenbaum/herdr-hunk-diff` (87★, 2026-08-22) — README: Known limitations, Round-trip prompts,
  Review target and display, Troubleshooting, Prior art;
  `yuucu/herdr-hunk` (2★, 2026-07-20); `persiyanov/herdr-reviewr` (511★, 2026-08-23);
  `edmundmiller/herdr-plugin-hunk` (13★);
  `afontaine.dev/nix` commit `bb070dd` on tangled.org — the one dotfiles repo running both.
- WebSearch round-ups naming both (`revdiff.com`, `hunk.dev`, `vibecodinghub.org/blog/hunk-review`,
  `toolhunter.cc/tools/revdiff`, `claudepluginhub.com/plugins/umputun-revdiff`).
- Prior lenses: `hunk.md` §1, §5–§9; `landscape.md` §1.2, §2.1–2.3; synthesis §6d, §7.5, §8, §10.

*Note: a parallel community-search agent (`combo-search`) was dispatched for §1 and had not
returned when this report was finalised. Everything in §1 above is first-hand (`gh`, WebSearch,
WebFetch, clones). If that agent surfaces additional dotfiles/HN/Reddit instances, they would add
breadth to §1.4 but are unlikely to change the verdict, which rests on the mechanical findings in
§3 and on `herdr-pickr`/`herdr-hunk-diff`.*
