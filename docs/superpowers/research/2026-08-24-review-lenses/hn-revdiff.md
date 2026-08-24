# Lens: the Hacker News reception of revdiff (and what the thread next door actually says)

Tags: **[E]** = quoted/verified from the source. **[I]** = my inference.

---

## 0. Coverage, and a correction to the brief

**I read 4 of 4 comments — 100% of the thread. [E]**

The brief anticipated a paginated, collapsed, multi-page thread. It is not one. Two
independent sources agree exactly:

- Algolia (`https://hn.algolia.com/api/v1/items/47742437`): walking `children` recursively
  yields **4 comment nodes**, story **17 points**. [E]
- The live HTML (`news.ycombinator.com/item?id=47742437`): **4** `class="athing comtr"` rows,
  **0** `morelink` elements, header reads `17 points` / `4&nbsp;comments`. [E]

There is no "More" link, nothing collapsed, and nothing truncated. The 10.8 KB page is the
whole thread. **The Algolia fallback was not needed to defeat truncation; it was needed to
prove there was nothing to truncate.** [E]

**Consequence for the task:** sections 2 and 3 as specified are largely *empty by fact*. The
thread contains **zero** criticism, **zero** feature requests, **zero** bug reports, **zero**
"I use X instead", **zero** named alternative tools, and **zero** debates. Not one of difit,
hunk, tuicr, plannotator, Conductor, lazygit, delta, difftastic, jj, magit, Zed, Cursor, VS
Code or the GitHub PR flow is mentioned anywhere in it. [E] I will not manufacture a taxonomy
out of four one-line comments.

The honest finding is that **this Show HN did not land** — 17 points and 4 comments is
near-silence by HN standards — and the community signal the brief wants exists, but in a
*different* thread. I found it and read it in full; see §2b and §5. That thread is where the
debates in §3 actually happen.

**One identity note:** the HN submitter is **`bumpa`** (karma 7), not `umputun`. [E] bumpa
submitted `github.com/umputun/revdiff` and commented "Author here". [E] So bumpa is
umputun's HN account, or a very close proxy. [I] Below I attribute to `bumpa`.

---

## 1. The author's own claims (bumpa/umputun)

The only substantial authored text in the thread is the **Show HN body itself**. The author's
sole comment is content-free: *"Author here, happy to answer questions or dig into the
technical details."* [E] **He then never replied again — there were no questions to
answer.** [E] So there is no "what he refused to build and why" and no "what he promised" in
this thread: nobody asked him anything, and he made no commitments. [E]

What he claims in the post: [E, all verbatim or close paraphrase]

- **Problem:** *"reviewing AI-generated code changes without leaving the terminal session
  where the agent runs, annotating what needs to change, and feeding those annotations
  straight back to the agent."* Existing viewers *"pulled me out of the terminal into a
  separate app, or the round-trip back to the agent was clunky."*
- **Workflow (the loop):** from a Claude Code session, `/revdiff main` or *"review diff for
  last 3 commits"* → *"it opens as a terminal overlay on top of the agent session"* → read,
  annotate, quit → *"The annotations go straight back to the agent, which picks them up and
  starts working on them. When the agent is done, revdiff reopens on the new changes so I can
  annotate again - and this loop continues **until I quit without leaving any
  annotations**."*
- **Granularity, stated by the author:** *"drop annotations on any **line, hunk, or file**."*
- **Plan mode:** a separate `revdiff-planning` plugin hooks `ExitPlanMode`; revdiff opens on
  the plan text, you annotate, *"the agent revises the plan before writing any code."*
- **Surface:** two-pane TUI, file tree left, syntax-highlighted diff right; vim nav, `/`
  search, hunk jumping, word-level change highlighting, blame gutters, collapsed diff mode; 7
  themes, remappable keys; git **and Mercurial**.
- **Host integration:** single Go binary, MIT. Terminal-overlay plugins for **tmux, Zellij,
  kitty, wezterm, Kaku, cmux, ghostty, iTerm2, Emacs vterm**; harness plugins for Claude Code,
  **Codex**, and **pi**.
- **Ask:** *"Would love to hear how others approach reviewing AI-generated diffs and PRs."*
  Nobody took him up on it. [E]

**No refusals, no roadmap promises.** [E] The "quit with no annotations = exit the loop"
termination rule is the one design commitment stated. [E]

---

## 2. Community response in *this* thread — the complete set

All four comments, in full. This is not a sample; it is the entire population. [E]

| # | User | Comment (verbatim) | Class |
|---|---|---|---|
| 1 | `bumpa` | "Author here, happy to answer questions or dig into the technical details." | author, no content |
| 2 | `hackmajoris` | "I use it within OpenCode. I'm happy." | praise, unspecific |
| 3 | `r13v` | "Awesome tool, thank you. I use it with Claude Code, mostly **in plan mode** to comment on a plan and then return to CC for further work. It allows me to point out things in the plan that we need to elaborate on **without leaving the terminal**." | praise + workflow |
| 4 | `temphaaa` | "Great work, I like it!" | praise, unspecific |

Counts: **praise 3** (2 of them contentless), **workflow descriptions 1**, **criticism 0**,
**feature requests 0**, **bug reports 0**, **"I use X instead" 0**, **alternatives named
0**, **debates 0**. [E]

The single informative data point: **`r13v` independently confirms that the highest-value
use of revdiff is plan-mode annotation, not diff review.** [E] `hackmajoris` confirms it runs
under a second harness (**OpenCode**) beyond the three the author listed. [E]

### 2b. Where the real discussion is: `Show HN: Codiff` (id 48166275)

Because the assigned thread is empty, I searched HN for the tool cluster. Most of these Show
HNs died quietly — difit 0 comments, tuicr 0 (twice), plannotator 2 and 0 and 0, vdiff 0,
deff 0, GitGlimpse 0, DiffDeck 1, "Local and GitHub Code Review TUI" 1. [E] **The agent-diff-
review category is crowded and almost entirely undiscussed on HN.** [E]

The exception is **"Show HN: Codiff, a local diff review tool"** by **`cpojer`** (Christoph
Nakazawa, ex-Meta, Jest lead), 2026-05-17, **31 points, 21 comments** — I read all 21. [E]
This is the thread that contains the alternatives list, the workflow descriptions, and the
debates the brief is looking for, and it is *about* the same problem revdiff solves.

**Every alternative tool named across both threads**, with what was said: [E]

| Tool | Named by | What they said |
|---|---|---|
| **revdiff** | `magnio` | listed under *"List of vibe coded alternatives"* — no judgement offered |
| **hunk** (modem-dev) | `magnio` | same list, no judgement |
| **diffnav** (dlvhdr) | `magnio` | same list. (Its own 2024 Show HN: *"a diff pager based on delta but with a file tree, à la GitHub"*, 7 pts / 3 comments) |
| **diffx** (wong2) | `magnio` | same list |
| **lumen** (jnsahaj) | `magnio` | same list |
| **codiff** (nkzw-tech) | `cpojer` (author) | built in **16 minutes** by pointing an LLM at diffs.com + trees.software; *"extremely fast for large diffs, beautiful and minimal"*; has file filters, search, **an llm walkthrough mode**, and *"review comments that you can paste back into your llm"* |
| **prereview** (livetemplate) | `realrocker` | built in 2 days to review Claude's changes **on a Hetzner box over Tailscale from his phone**; ships a **Claude skill `/prereview`**; has a **"Hand-off" button to signal claude to process the comments** |
| **delta** | `cpojer` | *"I used to review my own code using git + delta. It started to feel limiting with the amount of code written by llms."* |
| **Kaleidoscope** / **FileMerge** | `stephenr` | hostile: *"Who knew that the killer task for LLMs would be 'recreate existing things that exist without me mentioning them by name, based on a worse alternative as the example'. Kaleidoscope exists. File Merge exists."* |
| **JetBrains diff view** | `usernametaken29` | *"this has just a few features the JetBrains diff view is lacking, notably file search"* |
| **jj** (jujutsu) | `tcoff91` | *"I use jj so I need to be able to point it at commits since jj has no staging"* — a scope/base request |
| **pyor.review** | `othmanosx` | building the same thing as web+desktop |
| **plannotator** | (own thread) | supports **git, jj, p4, GitHub/GitLab PRs**, 9 harnesses (Claude Code, Codex, Copilot CLI, Gemini, OpenCode, Kiro, Droid, Amp, Pi), **100+ contributors**, annotates markdown/URLs/folders/**HTML artifacts** |
| **vibcod.dev** | `d3xt0r123` | a directory tracking vibe-coded tools, with stack / models / "vibe-index" |
| **difit, tuicr, Conductor, lazygit, difftastic, magit, Zed, Cursor, VS Code, GitHub PR flow** | — | **never mentioned in either thread** [E] |

**Workflow descriptions found (how people actually review agent output):** [E]

- `r13v`: **plan mode**, annotate the plan, back to Claude Code. Not commits, not PRs.
- `cpojer`: reviews **locally, pre-commit**, on the accumulated working diff — codiff works on
  *"any staged, unstaged or untracked file"*, and notably **does not yet support specific
  commits** and has *"no support for mutations (like staging or unstaging)"*.
- `realrocker`: reviews **before pushing to remote**, from a phone, against a remote box.
- `tcoff91`: **per-commit** (jj snapshots every command, so commits are the only stable unit).
- `bitomule` (Ask HN 48266275): risk-tiered, not location-tiered — *"UI or low risk code I can
  let the agent do everything. Auth, security, performance? I dig deeper manually or ask agent
  more about what it has done, why..."*
- **Nobody described a PR-per-turn flow, and nobody described worktrees.** [E]

---

## 3. The debates

In the revdiff thread: **none**. [E] In the codiff thread, three real ones:

**(a) TUI vs GUI — the live one, and the one that matters to cockpit.**
`cpojer` is unambiguous and repeats it twice: *"I built this specifically as a GUI because
it's become **too much code to effectively review in the terminal** for me"*, and, replying to
the alternatives list, *"**4 of these are TUIs. I built Codiff specifically because TUI review
became overwhelming to me.**"* [E] `realrocker` agrees independently and goes further: *"LLM
code output is so high that it has become **unreasonable to review the code in terminal or
ide**."* [E] **Where it landed: unopposed.** No one in the thread defended the TUI. The two
people who had each just built a review tool from scratch both concluded the terminal is the
wrong surface at LLM volume. [E]

**(b) Review fatigue / volume as the actual driver.** Both authors say the trigger for
building was a single unreviewably large diff — `cpojer`: *"When looking at a large diff on
Friday..."*, and he built the tool so he could *"finally review the large diff from Friday
that led me to build this"*. [E] `sjhalani7`: *"Been reviewing code diffs on the IDE and it's
been hard."* [E] **Landed: consensus. Volume, not rendering fidelity, is the problem
statement.** [I]

**(c) "Is any of this new?" / vibe-coded churn.** `magnio`: *"**Is diff viewer the 2026 todo
list?**"* [E] `stephenr` attacks the premise that an LLM-built Electron app beats decades-old
native tools: *"You're seriously suggesting that an electron app you vibe coded in two days is
more performant than native code that's been developed and improved and tweaked over literal
decades?"* [E] `cpojer` answers on architecture, not effort — *"Yes. It's built on diffs.com
and trees.software"* — and reframes the category: *"I never wasted time building todo apps.
Now I can build apps that help me do my job that I wouldn't have had the time for without
AI."* [E] **Landed: unresolved, and it decayed into an off-topic argument about whether JS in
an app window counts as "native" and how old the participants are.** [E]

**Not debated anywhere, in either thread:** stdout vs MCP vs hook; annotate vs accept/reject;
hunk-level vs line-level; multi-agent triage. [E] The brief's expectation that these were
litigated on HN is not borne out. [E]

---

## 4. What this means for cockpit

**(a) revdiff as the first backend — mechanism strengthened, popularity claim weakened.**
The thread gives cockpit almost no social proof: 17 points, 4 comments, two contentless
"nice tool" replies. [E] If §7.5 or the roadmap leans on revdiff being a *community-validated
winner*, that support is not there — and the same is true of every sibling tool (difit 0
comments, tuicr 0, hunk unreviewed). **The whole "5/5 winners converged" framing in §4 of the
synthesis rests on repo design, not on demonstrated community adoption.** [I] What *is*
strengthened is the integration seam: the author states the loop contract, the three-level
annotation grammar, and — decisively for cockpit — that revdiff already ships **tmux overlay
plugins** alongside Zellij/kitty/wezterm/ghostty. [E] The pipe in §7.5 is real and the
multiplexer work is already done upstream. [I] Keep revdiff first, on engineering grounds,
and drop any claim that the community picked it.

**(b) The fleet-queue thesis — untouched, and quietly corroborated.** Neither thread mentions
multi-agent triage, many panes, or choosing *which* agent to review. Every tool named is
single-repo, single-diff, single-agent. [E] That is exactly what §4 asserts ("Fleet triage:
absent everywhere — cockpit wins, uniquely"), and 25 comments across the category's two
liveliest threads produced no counterexample. [E] Stronger still: the one thing everyone
*does* agree on is that **volume is the problem** (§3b). A volume problem is a triage problem
before it is a rendering problem — which is the fleet queue's whole premise. [I] **This is
cockpit's most defensible differentiator, and the thread raises my confidence in it.**

**(c) Paste-into-pane vs stdout — §4's table is overstated and should be softened.** The
synthesis asserts *"structured stdout, JSON session API, hook return — 5/5; **no one types
into another process's TTY**"*. But codiff's headline feature is *"review comments that you
**can paste back into your llm**"* [E], and prereview's is a **"Hand-off" button to signal
claude to process the comments** [E]. Human-mediated paste-back is not the outlier the table
implies — it is what two of the newest tools ship as their primary delivery. [E] The genuine
distinction is narrower than "nobody types into a TTY": it is *programmatic* injection into a
foreign TTY vs *clipboard* hand-off. Cockpit's `paste-buffer -p -r -d` that leaves the text
**typed-but-unsent for the human to press Enter** sits between the two, and is arguably the
same human-in-the-loop contract codiff ships, just without the clipboard round trip. [I]
**Recommendation: keep the delivery path, and rewrite that row from "diverges" to
"converges, with a safer implementation."** The atomic `if-shell -F` re-gate remains the
part nobody else has.

**(d) Hunk-level vs line-level — supported, from the author's own mouth.** revdiff's stated
granularity is *"any line, hunk, or file"* [E]; plannotator does line/range/file across git,
jj, p4 and PRs [E]. Nobody argued for line-only. Phase 4's "hunk-level rows with line
drill-in" is the right target, and the file-header rows accepting file-level comments in §7.5
are the cheap two-thirds of it. [I]

**(e) Roadmap items — one is directly challenged.**
- **Phase 5 (first-party Go TUI, ~15 days) takes the heaviest hit in this research.** Two
  practitioners who each independently built a review tool *this year* both abandoned the TUI
  **for the exact reason cockpit would be building one** — LLM diff volume. [E] `cpojer`
  moved from `git + delta` in the terminal to a GUI precisely because terminal review
  *"started to feel limiting with the amount of code written by llms."* [E] The synthesis
  already defers this decision to a gate on product merits; **this thread supplies the first
  external evidence that the answer at that gate should be "no", and that the terminal's
  ceiling is a volume ceiling that a better TUI does not raise.** [I]
- **`@cockpit_review_window` (server-owned window) gains weight** over the modal popup: if the
  complaint is that terminal review is cramped at volume, the fix available to cockpit *within*
  tmux is more screen and persistence, not more fzf. [I]
- **Plan-mode review is under-weighted in the roadmap.** It is the *only* workflow a real user
  volunteered in the revdiff thread [E, `r13v`], it is what revdiff shipped a second plugin for
  [E], and it is plannotator's entire origin story [E]. Diff review is the crowded, contested
  surface; **plan review is where users report the value and where cockpit currently does
  nothing.** [I]
- **Scope/base work (Phase 3 `^B`) is validated:** `tcoff91` needs commit ranges because jj has
  no staging [E], and codiff's inability to target specific commits was the thread's one
  concrete functional complaint [E]. "unstaged-else-staged" is the same trap.
- **Agent-authored / AI first pass (Phase 4 `--seed`) has a named precedent:** codiff ships an
  *"llm walkthrough mode"*. [E]

---

## 5. What cockpit's research has not yet seen

**Tools absent from the landscape doc** [E]:
- **codiff** — `github.com/nkzw-tech/codiff`, by cpojer. GUI/Electron, built on **diffs.com**
  and **trees.software**. File filters, search, **LLM walkthrough mode**, paste-back comments.
  macOS-only at launch (`atmanactive`: *"... for MacOS."*).
- **prereview** — `github.com/livetemplate/prereview`, by realrocker. The **only
  remote/mobile** entry found: review a Hetzner box's changes from an iPhone over Tailscale,
  with a Claude skill `/prereview` and a **"Hand-off" button**. Screenshots: imgur.com/a/zxZo651.
- **diffnav** (dlvhdr) — delta-based pager with a GitHub-like file tree; the design cockpit's
  fzf list is reaching for, already built.
- **diffx** (wong2), **lumen** (jnsahaj), **vdiff**, **deff**, **GitGlimpse**, **DiffDeck**,
  **pyor.review** — the long tail, all essentially undiscussed.
- **diffs.com** and **trees.software** — reusable diff/tree *libraries*. cpojer assembled a
  working reviewer on them **in 16 minutes**. [E] If cockpit ever wants a non-fzf surface,
  these are the buy-vs-build shortcut nobody in the research has priced. [I]
- **vibcod.dev** — directory of vibe-coded tools with stack/model metadata; a cheap way to
  keep watching this category. [E]
- **Kaleidoscope**, **FileMerge**, **JetBrains diff view** — the prior-art counterweight
  (`stephenr`, `usernametaken29`). Worth one look before building any rendering feature. [I]

**Patterns not in the research** [E]:
- **Remote/phone review** of an agent running on a box elsewhere (prereview). Cockpit's
  "state dies with the pane" invariant makes this structurally impossible — a real limit, and
  a real user need. [I]
- **LLM walkthrough mode** (codiff): the agent narrates its own diff before you read it.
  Different from, and cheaper than, the Phase-4 AI first pass.
- **Annotating non-code agent artifacts** — plannotator does markdown, URLs, folders and
  **rendered HTML**. Agent output is not only diffs. [E]
- **Termination rule as a design primitive**: revdiff's loop ends when you *quit leaving no
  annotations*. [E] Cockpit's `^A` approve is an explicit gesture; revdiff's is the absence of
  one. The implicit version is cheaper and worth considering for the re-review delta loop. [I]
- **Risk-tiered review depth** (`bitomule`): full autonomy on UI/low-risk, manual scrutiny on
  auth/security/performance. [E] Cockpit orders the fleet queue by **churn**; ordering by
  *risk* (path globs) is a cheap, differentiated upgrade. [I]

**People to follow** [E]: **cpojer** (Christoph Nakazawa — codiff; the sharpest stated
position on why terminal review fails at volume), **ramoz** (plannotator; 100+ contributors,
9 harnesses — the most mature project in the category), **umputun/bumpa** (revdiff),
**realrocker** (prereview; remote/mobile), **dlvhdr** (diffnav), **magnio** (keeps the
category's index).

---

## 6. Bottom line

The assigned thread is a **null result**: 4 comments, all positive, no substance, author
never engaged. [E] It cannot bear the weight of the questions asked of it, and I'd flag that
any synthesis claim citing "community convergence" from these Show HNs is citing repo READMEs,
not people. [I] The category's real conversation is 30 comments away in the codiff thread, and
its two findings that should change cockpit's plan are: **(1) the people building these tools
are leaving the terminal because of diff volume** — which is a direct argument against Phase 5
and a direct argument *for* the fleet queue as triage; and **(2) paste-back delivery is
mainstream, not deviant** — so §4's harshest "diverges" verdict on cockpit's best code should
be revised. [I]

**Sources** [E]: `news.ycombinator.com/item?id=47742437` (4/4 comments read, via live HTML +
`hn.algolia.com/api/v1/items/47742437`); `item?id=48166275` Codiff (21/21 read);
`item?id=48495970` Plannotator (2/2); `item?id=48266275` Ask HN review-differentiation (2/2);
`item?id=48683061` (1/1); Algolia story/comment search for revdiff, difit, tuicr, plannotator,
diffnav, prereview.
