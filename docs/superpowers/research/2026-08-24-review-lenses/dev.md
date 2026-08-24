# Lens: DEVELOPMENT — `scripts/tmux-review` as software

Architecture critique, module decomposition, test strategy, migration path.
Evidence tags: **[E]** = verified in this repo / measured on this machine / gh-verified. **[I]** = inference.

Environment this was measured on: bash 5.2.21, tmux 3.7c, fzf **0.44.1**, git 2.43.0, awk = **mawk 1.3.4**. [E]
The fzf floor matters and is load-bearing below: `reload-sync`/`--listen` (0.36) and `become` (0.38) are in range; `transform` (0.45) and `--tmux` (0.53) are **not**. The repo already pins itself to 0.44 in prose (`scripts/tmux-branch:216`: "fzf 0.44 has no print() action"). [E]

---

## 0. Verdict in one paragraph

The *delivery* half of this script is genuinely excellent and should not be touched — `deliver()` (`scripts/tmux-review:471-562`) is a careful, live-verified, TOCTOU-closed piece of tmux engineering with real tests behind it. The *review* half is a 400-line dataflow with **no artifact in the middle**: every fzf reload re-derives the entire model from `git diff`, the comment-store grammar has a writer and no reader, and the row's identity is a tuple recomputed from a moving source. That single missing seam — an immutable per-session snapshot — is the root cause of two confirmed correctness bugs, the 5000-row cap, the 38.8% comment density, and the reason hunk navigation / comment edit / approve-file are each a cross-cutting change rather than a local one. The fix is not a rewrite and not a new language. It is one artifact, one parser, and moving ~70 lines of bash into the awk that is already running.

---

## 1. What is actually here (measured)

| metric | value | evidence |
|---|---|---|
| total lines | 743 | `wc -l scripts/tmux-review` [E] |
| code lines (non-blank, non-comment) | **407** | [E] |
| comment lines | **288 (38.8%)** | [E] |
| blank | 48 | [E] |
| functions | 12 | [E] |
| subcommands in the bottom `case` | 8 + `-h` + `""` | `:722-743` [E] |
| `lines_mode()` | `:77-269` — 192 lines, **111 code** | [E] |
| `deliver()` | `:471-562` — 91 lines, 49 code | [E] |
| `run_loop()` | `:354-430` — 76 lines, 45 code | [E] |
| comment blocks ≥3 lines | **34** | [E] |
| review test sections in `tests/run.sh` | 20 of 79 (25%) | [E] |
| assertions in suite | 243 | [E] |
| fixed `sleep`s in the review test range (2191-3306) | **25 of the suite's 50** | [E] |
| shellcheck | clean (`shellcheck -x`, rc 0) | [E] |

So: one file, one quarter of the test suite, and half of the suite's timing-dependent waits. This is already the single heaviest component in the plugin (`tmux-open-path` is 590 lines but has no state machine).

---

## 2. The structural defect: there is no artifact

### 2.1 The pipeline re-derives everything, every reload

```
git diff ──awk(:154-195)──> flat rows ──bash loop(:234-268)──> ordered rows ──fzf
   ▲                                                                          │
   └──────────────── reload(rows_cmd) on EVERY alt-Enter (:402) ──────────────┘
```

`rows_cmd` at `:362` is `"$SELF" --rows <dir> <store>`, and `rows_mode()` at `:309` calls `lines_mode "$dir"` — which shells out to `git diff` again (`:92-98`). So **every pinned comment re-runs git, re-runs awk, re-runs the whole row build, and re-derives the marker set.** [E]

Measured on this machine (synthetic repo, 40 files):

| rows | `--lines` | `--rows` (the per-comment reload cost) |
|---|---|---|
| 80 | 0.076s | 0.078s |
| 4000 | 0.548s | 0.404s |

[E] At the 5000-row cap that is roughly half a second of dead UI per comment, in a **modal popup**, with no `reload-sync` so the list flickers. It's tolerable; it is not the problem. The problem is what re-deriving *means*.

### 2.2 CONFIRMED BUG — comment anchors drift mid-review

Because the source of truth is live `git diff`, and the agent **is still writing while you review it**, the row identity `(path, line, sign)` is not stable for the life of a review session.

Reproduced on this machine [E]:

```
$ tmux-review --rows $B $S
f.txt   4   ADDED    f.txt:4 (+)  ADDED
$ tmux-review --attach $S f.txt 4 ADDED "this is wrong"
f.txt   4   ADDED   ●f.txt:4 (+)  ADDED          <- pinned, marked

# the agent inserts one line at the top of f.txt
$ tmux-review --rows $B $S
f.txt   1   NEWTOP   f.txt:1 (+)  NEWTOP
f.txt   5   ADDED    f.txt:5 (+)  ADDED          <- the ● is GONE
$ cat $S
## f.txt:4 (+)
> ADDED
this is wrong
$ sed -n 4p f.txt
c                                                <- line 4 is now unrelated
```

Two failures at once:
1. **The ● marker vanishes** — the reviewer sees an unreviewed row and comments again. Duplicate comments on the same line, no dedup anywhere.
2. **The delivered review points at the wrong line.** The composed payload says "`## f.txt:4 (+)`" and the agent's line 4 is `c`.

`README.md:521-524` claims the quoted fragment means a comment "still points at the right spot after the agent edits again — a bare line number would not survive that." That's a claim about the *agent's* fuzzy matching, not about the tool: **no code reconciles the fragment against the line.** The header still leads with the stale number, and the agent will read the number first. [E for the code; I for the agent's behavior]

This is the defect that most justifies the redesign, and it is *purely* a consequence of not snapshotting.

### 2.3 CONFIRMED BUG — a mixed staged/unstaged tree silently loses the staged half

The rule "unstaged first, staged as the fallback" is encoded **four times**, at **two different granularities**:

- `scripts/tmux-review:92-93` — per **repo** (`--lines`)
- `scripts/tmux-review:97-98` — per **repo** (numstat)
- `scripts/tmux-review:596-597` — per **repo** (fleet churn)
- `scripts/tmux-open-path:76` — per **file** (the preview)

[E] They disagree. Reproduced:

```
$ git status --porcelain
M  staged.txt          <- staged
 M unstaged.txt        <- unstaged

$ tmux-review --lines $d
unstaged.txt	2	UNSTAGED-CHANGE	unstaged.txt:2 (+)  UNSTAGED-CHANGE
                       # staged.txt is INVISIBLE. Not reviewable. Not commentable.

$ tmux-open-path --preview-diff staged.txt
@@ -1 +1,2 @@
 a
+STAGED-CHANGE         # ...but the preview shows it happily
```

Because the repo-level `git diff` was non-empty, the `||` fallback never fires and the entire staged half of the tree is dropped from the review. The reviewer believes they reviewed the agent's changes. This is exactly the "confident wrong answer" the codebase's own comments warn about (`scripts/tmux-open-path:53-56`). [E]

**The one-call fix**: `git diff HEAD` returns both. Verified [E]:
```
$ git diff HEAD --numstat
1	0	staged.txt
1	0	unstaged.txt
```
That deletes 4 lines of fallback logic *and* fixes the bug. The rule was never "unstaged else staged"; it was always "everything uncommitted", and `HEAD` is how git spells that.

### 2.4 CONFIRMED BUG — the fleet queue lists panes with nothing to review

`fleet_rows()` counts untracked files into churn (`:610-614`) so a pane with only new files sorts in. But `git diff` never shows untracked files, so `--lines` returns zero rows and `run_loop` bails with "no uncommitted changes" (`:375-379`). Reproduced [E]:

```
$ git status --porcelain
?? untracked.py            <- fleet queue lists this pane
$ tmux-review --lines $B
(exit 0, 0 rows)           <- selecting it dead-ends
```

Fix: synthesize rows for untracked files (`git ls-files --others --exclude-standard`, every line a `+` at its own number). Do **not** use `git add -N`, which mutates the index the agent is concurrently using. [I]

### 2.5 CONFIRMED SMELL — the store grammar has a writer, no reader, and is not round-trippable

The store format is claimed to be "revdiff's, verbatim" (`:317`). It is written in exactly one place (`attach()`, `:332-342`) and read in exactly two, each reading a *different subset*:

- `rows_mode():292-297` reads only lines matching `## *` — as opaque strings, to build a marker set.
- `compose():444` is `cat`. It does not parse. It does not validate.
- The third reader is the **agent LLM**, which has no contract at all.

Three consumers, zero shared parser. And the format is lossy in both directions [E]:

```
$ tmux-review --attach $S f.go 10 code '## not a header'
$ tmux-review --attach $S f.go 11 code2 '> looks like a fragment'
$ tmux-review --compose $S | tail -7
## f.go:10 (+)
> code
 ## not a header            <- escape applied on write, NEVER un-applied
                            <- the agent receives a leading space the human didn't type
## f.go:11 (+)
> code2
> looks like a fragment     <- indistinguishable from a fragment line; no escape at all
```

And the writer does not validate its own grammar:
```
$ tmux-review --attach $S f.txt "" frag note   →  "## f.txt: (+)"      (empty line number, delivers)
$ tmux-review --attach $S f.txt abc frag note  →  "## f.txt:abc (+)"   (non-numeric, delivers)
```
[E] `attach()` guards `path` and `comment` (`:325-326`) but never `line`. The `--attach ""` case *is* tested (`tests/run.sh:2446-2457`) — the empty-`line` case is not.

**Compare with the tool it copied.** `umputun/revdiff` (807★, pushed 2026-08-20, Go) [E, gh-verified] has `app/annotation/store.go` with `Store.FormatOutput` and `app/annotation/parse.go` whose doc comment reads *"`Parse` reads the markdown produced by `Store.FormatOutput`"* — a literal inverse pair, with a header regex:

```
^## (.+?)(?::(\d+)(?:-(\d+))?)? \((file-level|\+|-| )\)$
```

Two record types tmux-review's writer **cannot emit** and its readers therefore never see: **line ranges** (`path:43-67`) and **file-level** annotations (`## path (file-level)`). Those are precisely the two shapes "hunk navigation" and "approve-file" would need. The grammar was adopted as a string template; the *model behind it* was not.

`modem-dev/hunk` (8,711★, pushed today, TS) [E, gh-verified] goes further and makes the seam a named module: `src/session/reviewProtocol.ts`, whose own header says it is *"the review wire schema: what one review consumer may say to the producer serving it"*, kept browser-safe and guarded by a dedicated boundary test (`scripts/source-boundaries.test.ts`) that fails if it ever imports transport code. Their `docs/browser-review-seam-audit.md` names the exact bug class tmux-review has today: a hand-copied union that drifts out of sync with its producer.

### 2.6 State is threaded through positional args, and the tuples are all different shapes

```
git_root   <dir>                                    :46
lines_mode <dir>                                    :78
rows_mode  <dir> <store>                            :281
attach     <store> <path> <line> <fragment> <comment>  :320
run_loop   <dir> <store>                            :355
compose    <store> <root>                           :437
deliver    <pane> <store> <root>                    :472
review_pane <pane> <dir>                            :631
```
[E] Eight functions, eight different argument tuples over the same four concepts (pane, dir/root, store, row). `dir` and `root` are the same value at every call site — `review_pane` is called as `review_pane "$pane" "$(git_root "$dir")"` at `:695` and `:716` — but `deliver` receives it under the name `root` and `run_loop` under the name `dir`, and `:643` passes `$dir` into `--deliver`'s `root` slot. Nothing enforces that they agree; a future caller that forgets `git_root` gets root-relative paths that don't resolve, which is the exact bug `:37-45` documents already having been fixed once.

### 2.7 Re-exec is fine; the *implicit* API surface it creates is not

Re-exec via `$SELF` (`:30`) is correct and idiomatic — `tm:187` and `tmux-open-path:488-494` do the same, and fzf's `reload`/`execute-silent` genuinely need a fresh command. But the subcommands invented for it (`--rows`, `--attach`) are labelled "internal" (`:271`) while being (a) reachable from any shell, (b) unvalidated, and (c) **tested as public API** (`tests/run.sh:2343-2457`). They are a public interface by accident. Compare `forgit` (5,070★, 2026-08-21) [E, gh-verified], which makes the split explicit: `forgit.plugin.sh` is a 260-line shim that forwards every `forgit::x` to `bin/git-forgit x` — thin entrypoint, fat dispatcher, one documented boundary.

### 2.8 Cross-file duplication of the same three facts

| duplicated thing | sites |
|---|---|
| the is-agent tmux format `#{m:*\|#{pane_current_command}\|*,#{@cockpit_agents_glob}}` | `tmux-review:482,559,579,713` + `agent-state`×3, `tmux-jump`, `tm` — **9 copies** [E] |
| popup preview sizing `avail=$(tput lines …); psz=$((avail-lrows-5)); [ $psz -lt 5 ] && psz=5` | `tmux-review:380-383`, `tmux-review:666-669`, `tmux-branch:202-205`, `tmux-open-path:520` — **4 copies** [E] |
| the `-N` → `(sign, num)` decode of the tool's own wire format | `tmux-review:302-305` and `:329-331` — 2 copies, 30 lines apart [E] |
| the fzf house style `--reverse --info=inline-right --cycle --scroll-off=2 --color=16` | 7 copies across 4 scripts [E] |
| "unstaged first, staged fallback" | 4 copies at **2 different granularities** — see §2.3, this one is a live bug [E] |

The last one proves duplication here is not cosmetic.

---

## 3. Are the ~60 comment blocks explaining or apologizing?

288 of 743 lines (38.8%) are comments; 34 blocks of ≥3 lines; the longest is 28 lines (`:125-152`). [E]

Read them and they sort cleanly into three kinds:

**(a) Genuine design rationale — keep.** `:37-45` (why git_root exists), `:271-279` (why `--rows` is its own subcommand), `:345-353` (why enter is a no-op), `:448-470` (the delivery guard ladder). These say *why*, not *what*, and nothing else can say them.

**(b) Regression narratives — should live in the test.** 15 comment lines carry past-tense bug prose: "Reproduced:", "measured:", "the old", "used to", "Found by tracing", "the hunt reproduced it 6/6". [E] Examples: `:139` ("Reproduced: a deletion at old line 5 was anchored to -3"), `:150-152` (the `-- ` body line), `:228` ("measured 2000 rows 4.77s, 8000 rows 55.9s"), `:546` ("the hunt reproduced it 6/6"). **These are duplicated into `tests/run.sh` almost verbatim** — `tests/run.sh:2624` retells the `-- ` story, `:2229-2237` retells the cap measurement, `:2994-3003` retells the TOCTOU. `grep -icE 'reproduced|used to|measured|the old|FAIL-first' tests/run.sh` → **51 lines**. [E] Two copies of every war story means the next fix updates one and silently invalidates the other. The right shape: the code comment states the *invariant* in one line and names the test; the test holds the narrative next to the fixture that proves it.

**(c) Apology for the implementation.** `:221-230` explains that an associative-array string append is O(n²) and that `$(printf)` forks per row. `:100-111` explains that the shell row build costs minutes so a cap was added. `:284-290` explains that a per-row `grep -qxF` was replaced with a set. All three describe workarounds for **bash being the wrong tool for the inner loop** — and all three disappear entirely if the loop moves into the awk that is already running (§5.1). A comment that exists to explain why the code is slow is a design smell wearing a docstring.

Net: I'd expect ~110-130 comment lines in the redesigned file, not 288, with strictly *more* rationale preserved.

---

## 4. Test strategy — what is covered, what is not

### Covered well [E]
20 sections, and the hard-won ones are real: byte-for-byte golden output (`:2311-2316`), fail-first timeouts as regression teeth on both perf fixes (`:2254`, `:2331`, `:2380`), the funcname-context anchor bug (`:2594`), `-- `/`++ ` body lines (`:2624`, `:2659`), non-ASCII paths in both positions (`:2389`, `:2427`), the subdir→root resolution (`:2693`), and four separate `--deliver` tests including a live concurrent cross-wire test (`:2867`) and the atomic re-gate (`:2994`). That is genuinely good discipline and above what any Group-B peer does (`fzf-git.sh` 1,176★ and `ThePrimeagen/tmux-sessionizer` 462★ ship **zero** tests; the latter has been stale since 2025-06-19). [E, gh-verified]

### Not covered — the gaps, ranked by what they'd have caught

| gap | would have caught |
|---|---|
| **mixed staged+unstaged tree** | §2.3 — silent loss of the staged half. **No fixture stages anything.** [E] |
| **untracked-only pane** | §2.4 — the dead-end fleet row. The fleet fixture (`:3223-3226`) only ever appends to a tracked file. [E] |
| **mid-review tree mutation** | §2.2 — anchor drift and marker loss. Nothing in the suite writes to the fixture *between* two `--rows` calls. [E] |
| **`--attach` with a non-numeric or empty `line`** | §2.5 — `"## f.txt: (+)"` delivers. The `path`-empty case is tested; `line` is not. [E] |
| **store round-trip** | §2.5 — the `## `→` ## ` escape is never un-applied; a `> `-leading body line is indistinguishable from a fragment. Untestable today because there is no reader to round-trip *through*. |
| **fleet at scale** | The fixture is 3 panes. No test asserts the scan's wall time, and none has two panes in the same repo (which double-scans it). [E] |
| **csi-u / unicode in a comment** | `--deliver` is tested with ASCII markers only; the header comment at `:456-459` claims csi-u was live-verified out of band, but nothing in the suite pins it. Similarly no test types a non-ASCII *comment* (paths are covered, comment bodies are not). [E] |
| **the interactive loop beyond one comment** | One `alt-Enter`, one `C-s`, one `Escape`. No test pins **two** comments (which is where reload-after-reload and duplicate-anchor behavior lives), and none pins `alt-u`/`alt-d`/`^/`. [E] |
| **fzf absent** | `run_loop:357` and `fleet_queue:653` both refuse without fzf. Neither refusal is tested. `tmux-branch` has a `select`-based fallback (`tmux-branch:184-193`); review has none, and no test says so. [E] |

### Flakiness surface
25 fixed `sleep`s in the review range — half the suite's total. [E] The polling loops (`:2484-2489`, `:3120-3128`) are the good pattern; the bare `sleep 0.5` / `sleep 1` after each `send-keys` are the flaky ones. Every one of those is a *capture-pane assertion in disguise* and should use the same `for i in 1..10; case $(capture-pane) in <expected>) break` shape already written twice in the file.

### The strategy the redesign enables
Once §5 lands, the ratio inverts. Today the only fast, hermetic tests are `--lines` and `--compose`; everything else needs a live tmux server. After the split:

- **`snapshot` → golden TSV.** Pure function of a git fixture. All 6 existing parser fixtures become golden-file comparisons against a committed `.tsv` — the shape `laktak/extrakto` (1,137★, Python) uses: `tests/test_get_paths.py` + `tests/assets/` golden fixtures. [E, gh-verified]
- **`rows` → pure join.** No git, no tmux, no fzf. Feed it a hand-written `diff.tsv` + `comments.md` and assert the marker column. Every marker/ordering test loses its git fixture and its 0.4s.
- **`format`/`parse` round-trip property test.** For a table of adversarial comment bodies (`## x`, `> x`, empty, 4KB, `??`, CJK, a NUL-free control char), assert `parse(format(x)) == x`. This is the single test that does not exist today and cannot exist without §5.2.
- **The tmux-dependent set shrinks to what genuinely needs a server**: `deliver` (already 5 sections, all good) and one end-to-end fleet→loop→deliver smoke. Everything else moves to the fast lane.

---

## 5. Proposed decomposition

### The seam, stated once

> **One wire format in, one out.** In: an immutable `diff.tsv` snapshot, taken once per review session. Out: a `comments.md` in revdiff's *full* grammar, with a parser that is the declared inverse of the formatter. Everything between them is a pure function of those two files and touches neither git nor tmux.

### 5.1 Move the row build into the awk that is already running — **proven, byte-identical, 8.7× faster**

`lines_mode` currently splits work across three stages: awk emits flat rows (`:154-195`), bash rebuilds them with display strings and a churn map (`:198-245`), bash partitions and re-emits by index range (`:249-268`). That is **111 code lines** and every performance comment in §3(c) is about the middle stage.

awk can do all three in one pass with a decorate-sort-undecorate tail. I prototyped it (`scratchpad/lines2.sh`, ~35 lines) and verified it against every fixture in the suite:

| fixture (from `tests/run.sh`) | result |
|---|---|
| golden test-first ordering (`:2289-2316`) | **byte-identical** [E] |
| non-ASCII path, mid-diff (`:2389`) | identical [E] |
| non-ASCII path, leading (`:2427`) | identical [E] |
| `-- ` body line (`:2624`) | identical [E] |
| `++ ` body line (`:2659`) | identical [E] |
| whole-file deletion + funcname context (`:2594`) | identical [E] |

Measured:

| rows | current `--lines` | prototype | speedup |
|---|---|---|---|
| 5,000 (the cap) | 0.653s | **0.075s** | 8.7× [E] |
| 100,000 | >5 min (per `:104-106`) | **0.670s** | ~450× [E] |

Consequences:
- The **5000-row cap becomes a UX choice, not a perf workaround.** At 0.67s for 100k rows the cap can be raised to ~50k or replaced with an honest "this is 40k lines, are you sure" row. The 24 lines at `:100-123` become ~4.
- The `declare -A` churn map, the `gstart`/`gend` index-range trick, the `printf -v` fork avoidance, and their ~30 lines of apologetic comments (`:221-230`, `:100-111`) all delete.
- **Caveat**: measured on mawk 1.3.4. gawk and macOS's BWK awk will be slower (I'd guess 2-4×, still ≪ bash). [I] The suite's existing `timeout` teeth already guard the regression on whatever awk is present.

**Do not, however, replace the awk parser itself.** I checked: there is **no porcelain/machine-readable format for git diff hunk bodies**. `--numstat`, `-z`, `--raw`, `--line-prefix` are documented machine surfaces for *file-level* data; `--word-diff=porcelain` governs only word-diff rendering; a general `git diff --porcelain` does not exist. [E, verified against git-scm docs] Every surveyed tool hand-parses unified diff: delta `src/handlers/hunk.rs`, tig `src/diff.c`, revdiff `app/diff/diff.go`, hunk `src/core/changeset/fromPatch.ts` + `hunkHeader.ts`. [E, gh-verified] The hand-rolled awk parser is **the correct decision**, well-tested, and should stay. Feed it more (`git diff HEAD`, §2.3) and ask it for more (hunk ids, §5.3); don't replace it.

### 5.2 Introduce the two artifacts and the session directory

Replace `store=$(mktemp)` (`:632`) with `session=$(mktemp -d)` containing:

```
$session/
  meta          root=<abs>  pane=<%N>  head=<sha>  taken=<epoch>
  diff.tsv      the SNAPSHOT — written once, never rewritten
  comments.md   append-only, revdiff grammar (unchanged on disk)
```

`diff.tsv`, one row per reviewable line, **named columns, stable order**:

```
id      monotonic row index — the row's identity, forever
file    repo-root-relative path
hunk    hunk ordinal within the file (currently discarded at :185-193)
side    + | -
line    line number on `side`
blob    the file's pre-image blob sha (drift detection)
text    the fragment, control-stripped, capped at 160
```

This single change fixes §2.2 outright: the picker reads `diff.tsv`, not git, so the row set cannot move under a live review. A new key (`^R`) can *explicitly* re-snapshot and report which comments went stale — which is a feature (`revdiff` and GitHub both call this "outdated") rather than a silent corruption.

It also gives back what `:312-318` claims and does not deliver: **crash recovery.** Today the store is a `mktemp` whose name is recorded nowhere, so a dropped SSH client (which tears down a client-owned `display-popup`) orphans the file *and* loses the review — nothing can find it again. `revdiff` saves annotations to `~/.config/revdiff/history/<repo>/` on SIGHUP/SIGTERM for exactly this reason, and offers `REVDIFF_TMUX_WINDOW=1` to use a **server-owned tmux window instead of a client-owned popup** so a dropped client cannot kill a review. [E, from `docs/superpowers/research/2026-08-24-review-tools-landscape.md:96-118`] The prior research named both; the build took neither. A named session dir under `${XDG_STATE_HOME:-~/.local/state}/tmux-cockpit/review/<pane>-<sha>/` makes resume a one-liner.

### 5.3 The subcommand surface

Today: `--fleet --lines --rows --attach --loop --compose --deliver` — a flat namespace mixing public contract (`--fleet`, `--compose`, `--deliver`), internal re-entry (`--rows`, `--attach`), and one of each (`--lines`, `--loop`). Proposed, one noun-verb surface over the session:

| command | pure? | replaces |
|---|---|---|
| `snapshot <dir> <session>` | git only | `--lines` (`:77-269`) |
| `rows <session>` | **pure** — no git, no tmux | `--rows` (`:280-310`) |
| `annotate <session> <id> <text>` | pure | `--attach` (`:319-343`) |
| `format <session>` | pure | `--compose` (`:436-446`) |
| `parse <session>` | **pure — new**, the declared inverse of `format` | *(does not exist)* |
| `loop <session>` | fzf + tmux | `--loop` (`:354-430`) |
| `deliver <pane> <session>` | tmux | `--deliver` (`:471-562`) — **unchanged** |
| `fleet` | tmux + git | `--fleet` (`:573-621`) |

Every command takes **`<session>` and nothing else** except the two entry points. That is the fix for §2.6: one opaque handle replaces eight ad-hoc tuples, and `root` can never disagree with `dir` because it lives in `meta`.

`rows`, `annotate`, `format`, `parse` are **pure functions of two files** — testable with no git fixture, no tmux server, no fzf, in milliseconds.

### 5.4 File layout

Not one file, not fifteen. Three, following `forgit`'s thin-shim/fat-dispatcher split and `sainnhe/tmux-fzf`'s one-script-per-mode (1,490★, `main.sh` + `scripts/{session,window,pane,…}.sh`) [E, gh-verified]:

```
scripts/tmux-review          ~150 lines  entry resolution, dispatch, fleet, loop (the tmux/fzf glue)
scripts/lib/review-diff.awk  ~60 lines   the parser+formatter, one pass, snapshot out
scripts/lib/review-store.sh  ~120 lines  format() / parse() / annotate(), the ONE grammar module
```

`review-store.sh` is the module `hunk` calls `reviewProtocol.ts` and `revdiff` splits as `store.go`/`parse.go`. It is sourced, not re-exec'd, and it is the only file that knows what `## path:line (+)` means. A shellcheck directive plus one boundary test (`grep -L 'tmux\|git' scripts/lib/*` must list both) enforces that the pure modules stay pure — the same guard `hunk`'s `scripts/source-boundaries.test.ts` implements. [E, gh-verified]

### 5.5 What this makes cheap that is expensive today

| feature | today | after |
|---|---|---|
| **hunk navigation** | awk discards the hunk header after extracting counters (`:185-193`); adding a 5th field shifts `--with-nth=4` (`:399`), `attach_cmd`'s `{1} {2} {3}` (`:363`), `rows_mode`'s 4-field `read` (`:300`), and 3+ golden strings in the suite | `hunk` is already a column; `--with-nth` names a field; a `]`/`[` bind is a `jump-to next id where hunk != current` |
| **comment edit / delete** | requires writing the first-ever reader of the grammar, which must be the exact inverse of `attach():332-342` **including** the `## `→` ## ` escape that is never un-applied (§2.5) | `parse` exists and is round-trip tested; edit = parse, replace by id, rewrite |
| **approve-file** | rows are flat; the per-file `[start,end]` ranges are local to `lines_mode` and thrown away (`:231,:263`); revdiff's `(file-level)` record type is not emitted | `file` is a column; revdiff's `## path (file-level)` header is already in the grammar's regex |
| **line ranges** (`path:43-67`) | writer cannot emit them; revdiff's parser accepts them | one `format` branch; revdiff even auto-expands to the hunk range when the note contains the word "hunk" |
| **agent-seeded comments** | no reader → cannot ingest a pre-written `comments.md` | `parse` makes it free; `difit` exposes this as `--comment '{...}'` and `Conductor` as an MCP tool [E, from prior research] |

---

## 6. Should the review core be a different language, or shell out to revdiff/hunk?

### Different language for the core: **no.**
The whole plugin's stated invariants are one-shot scripts, no daemon, no build step, bash 4+ (`scripts/tmux-review:21-25`). A Go or Rust core means a build step or a vendored binary in a **tmux plugin cloned via TPM onto arbitrary machines** — the Makefile's own comment already concedes "this repo is cloned onto machines that may not have shellcheck" (`Makefile:28`). Rewriting the core is a distribution problem far larger than the bug it fixes. The awk prototype (§5.1) gets 8.7× and 450× in the two places that matter without leaving the shell. Note also the cautionary data point: `ThePrimeagen/tmux-sessionizer` (462★) is a 344-line single bash file with no tests and **has been stale since 2025-06-19**; `sesh` (2,783★, Go, 34 packages, **66 test files** + mockery mocks) and `extrakto` (1,137★, Python, pytest + golden fixtures) are the ones still moving. [E, gh-verified] The differentiator is *decomposition and tests*, not the language — extrakto is a tmux plugin in Python with a pytest suite and it works fine.

### `@cockpit_reviewer` shelling out to revdiff/hunk/difit: **yes, but as a second entry point, not a swap.**
This is the strongest dependency-strategy option and the seam is already half-built.

- `revdiff` (807★, Go, single static binary, no runtime deps) **already ships a tmux launcher** — `scripts/launch-revdiff.sh` does `tmux display-popup -E -w -h -T -d "$CWD" -- sh -c "$REVDIFF_CMD"`, which is byte-for-byte the shape `cockpit.tmux:249` already uses. It writes its annotations to **stdout in the same markdown grammar this tool already adopted**, and to `~/.config/revdiff/history/` on signal. [E, from prior research + gh-verified tree]
- That means the integration is: run revdiff in the popup, capture stdout, feed it to `parse` (§5.3), hand it to `deliver` **unchanged**. The bash path stays as the zero-dependency fallback. `deliver()` — the part of this script that is actually best-in-class and has no equivalent in revdiff (revdiff pipes to stdout; it has no "paste typed-but-unsent into a live agent pane, atomically re-gated" story) — becomes the plugin's differentiated contribution rather than a subroutine of a picker.

**Precedent for the pluggable-backend pattern** [E, gh-verified]: `lazygit` (81,598★) exposes `git.paging.pager` accepting `delta`, `diff-so-fancy`, `ydiff`, or `difft`; `tig` (13,317★) has a `diff-highlight` tigrc option that shells to git's own program; `forgit` (5,070★) has `FORGIT_PREVIEW_PAGER` specifically because TUI pagers break in fzf's non-TTY preview. Delegating the *renderer* while owning the *integration* is the mainstream design in this exact niche.

**What a clean seam looks like:**

```
                       ┌─ tmux-review snapshot ─> diff.tsv ─> rows ─> fzf ─┐
@cockpit_reviewer=bash │                                                   ├─> comments.md ─> deliver
                       └─ revdiff / hunk / difit (popup or server) ────────┘
                                          ^ stdout in revdiff grammar, piped through `parse`
```

One format in (`diff.tsv`, or the backend's own diff acquisition), one format out (`comments.md`). `deliver` never learns which backend ran. `difit`'s harvest endpoint (`curl .../api/comments-output`) is the same seam over HTTP and is the right answer for the *fleet* case specifically — reviewing pane 3 while pane 5 keeps working is something a modal blocking popup structurally cannot do. [E, from prior research]

---

## 7. Performance — the fleet scan

`fleet_rows()` (`:573-621`) runs **2 serial git calls per agent pane** (`:591` status, `:596-597` numstat), inside the modal popup, **before any UI appears**, with **no cap, no parallelism, no dedup by repo root**, and **no progress indicator or cancel**.

Measured on this machine (tmpfs, warm cache) [E]:

| repo | per-pane | 12 panes serial | 12 panes parallel (`&`+`wait`) |
|---|---|---|---|
| this repo (46 files) | 6ms | 0.077s | 0.017s |
| synthetic 20,000 files | 44ms | **0.586s** | **0.196s** (3.0×) |

On a real cold monorepo on spinning disk or NFS, `git status` is 0.5-2s and 12 panes is 12-48s of frozen modal popup you cannot escape. Three fixes, all small:

1. **Parallelize.** `( … ) &` per pane into per-pane tmpfiles, `wait`, then sort. 3× measured, and it scales with pane count instead of against it. [E]
2. **`git --no-optional-locks`.** `:591` and `:596` both refresh the index, which takes the index lock — **racing the agent's own git in the same repo.** `--no-optional-locks` is git's documented flag for exactly this (background/monitor callers) and is absent from all four call sites. [E]
3. **Dedup by repo root.** Multiple agent panes commonly share one repo (or worktrees of one). Today each pane pays the full scan independently and the queue shows N identical-churn rows. Compute per-root once, join to panes. [I]

Also worth noting: this repo's own `docs/superpowers/research/2026-08-24-critical-bugs.md:109` already files this as **S4 [IMPORTANT] "fleet queue ~2 serial git/pane, no parallelism/cap"** — it is a known-open item, and the measurements above are the teeth for it.

---

## 8. Ranked recommendations

Each: **what** / **why** / **cost** / **what it replaces**.

---

**R1 — Snapshot the diff once per review session (`diff.tsv` + a session dir).** — cost **M**
*Why:* fixes the confirmed anchor-drift + marker-loss bug (§2.2), makes `rows` a pure function so the per-comment reload drops from 0.4s to ~10ms, and gives the crash recovery `:312-318` already claims. Enables an explicit `^R` re-snapshot with a stale-comment report instead of silent corruption.
*Replaces:* `rows_mode()` calling `lines_mode()` at `:309`; the `mktemp` store at `:632`; the re-derived identity at `:302-307`.

**R2 — Use `git diff HEAD` (plus untracked synthesis) instead of the unstaged/staged fallback.** — cost **S**
*Why:* two confirmed bugs at once — the silently-dropped staged half (§2.3, verified) and the dead-end fleet row for untracked-only panes (§2.4, verified). Deletes 4 lines of fallback and one whole category of granularity mismatch.
*Replaces:* `:92-93`, `:97-98`, `:596-597`; align `tmux-open-path:76` to match.

**R3 — Move the row build into the awk; keep the awk parser.** — cost **S** (prototype exists, byte-identical on all 6 fixtures)
*Why:* 8.7× at the 5000-row cap, ~450× at 100k, measured. Deletes ~70 lines of bash *and* the ~30 comment lines apologizing for them. Turns the cap from a perf workaround into a UX choice. There is no porcelain diff format — hand-parsing is correct and every peer does it; the mistake was doing the *rest* in bash.
*Replaces:* `:198-268` and the cap rationale at `:100-123`.

**R4 — Write `parse()` as the declared inverse of `format()`, in one grammar module.** — cost **M**
*Why:* today three consumers read three different subsets of a format nobody validates, and the `## `→` ## ` escape is applied on write and never un-applied (§2.5, verified). This is `revdiff`'s `store.go`/`parse.go` pair and `hunk`'s `reviewProtocol.ts`, both explicitly built to prevent the drift this file already has. Unlocks comment edit/delete, agent-seeded reviews, and `revdiff`'s two missing record types (ranges, file-level).
*Replaces:* `compose()`'s `cat` at `:444`; `rows_mode`'s ad-hoc `## `-prefix scan at `:292-297`; the duplicated sign decode at `:302-305`/`:329-331`.

**R5 — Split into `tmux-review` (glue) + `lib/review-diff.awk` + `lib/review-store.sh`, with a `<session>`-only argument convention.** — cost **M**
*Why:* eight functions with eight different argument tuples over four concepts (§2.6); `--rows`/`--attach` are internal-by-comment and public-by-test. `forgit`'s thin-shim/fat-dispatcher and `tmux-fzf`'s script-per-mode are the reference shapes. Makes ~4 of the 8 subcommands testable with no git, no tmux, no fzf.
*Replaces:* the flat 743-line file and its bottom `case` at `:722-743`.

**R6 — Parallelize + `--no-optional-locks` + dedup the fleet scan.** — cost **S**
*Why:* 3× measured at 12 panes on a 20k-file repo; unbounded and un-cancellable in a modal popup on a cold monorepo. `--no-optional-locks` also stops the scan racing the agent's own index lock. Already filed in-repo as S4.
*Replaces:* the serial loop at `:588-616`.

**R7 — Close the test gaps that map 1:1 to R1/R2/R4, and de-flake.** — cost **M**
*Why:* every confirmed bug in §2 is a missing fixture, not a missing insight: no test stages anything, no test leaves a file untracked, no test mutates the tree between two `--rows` calls, no test round-trips the store, no test passes a non-numeric `line`. Plus 25 fixed `sleep`s (half the suite's total) that should be the polling loops already written twice in the same file.
*Replaces:* nothing — pure addition; but ~8 existing sections lose their git fixtures and get faster once `rows` is pure.

**R8 — `@cockpit_reviewer` backend switch (bash | revdiff | hunk | difit), bash as default.** — cost **L**
*Why:* revdiff already emits this exact grammar on stdout and already ships a `display-popup -E` launcher identical to `cockpit.tmux:249`; `parse()` (R4) is the only adapter needed. Precedent is strong (lazygit's `git.paging.pager`, tig's `diff-highlight`, forgit's `FORGIT_PREVIEW_PAGER`). Keeps this plugin's genuine differentiator — `deliver()` — and stops it competing on diff rendering, which it will lose.
*Replaces:* nothing; adds a second path. **Do R1-R5 first** — the seam R8 plugs into is exactly the artifact R1 and the parser R4 create.

**R9 — Adopt `reload-sync` for the comment reload; consider a server-owned window over a client-owned popup.** — cost **S**
*Why:* `reload-sync` (fzf ≥0.36, available at the installed 0.44.1) removes the visible flicker/race on every alt-Enter; `transform` (0.45) and `--tmux` (0.53) are **not** available at this floor, so don't design for them. Separately, `display-popup` is client-owned: a dropped SSH client kills a review in flight — which is why revdiff offers `REVDIFF_TMUX_WINDOW=1`. For a plugin whose premise is long-lived agent panes, a server-owned window is arguably the right default, not the opt-in.
*Replaces:* `reload(...)` at `:402`; optionally the popup binding at `cockpit.tmux:249-250`.

**R10 — Demote the regression narratives from code comments into the tests.** — cost **S**
*Why:* 15 past-tense bug-story lines in the script are duplicated almost verbatim in `tests/run.sh` (which carries 51 such lines). Two copies of one story means the next fix invalidates one silently. Keep the *why*; move the *what happened* next to the fixture that proves it. Expect 288 → ~120 comment lines with **more** rationale preserved, because R3 deletes the apologies outright.
*Replaces:* `:125-152`, `:100-111`, `:221-230`, `:284-290`, `:385-395` (partially — the design rationale in each stays, the war story goes).

---

## 9. Migration path

Every step is independently shippable, keeps the suite green, and none is a rewrite.

**Step 0 — freeze the contract.** Commit the current `--lines` output for all 6 parser fixtures as golden `.tsv` files under `tests/fixtures/review/`. The suite already has one such golden inline (`tests/run.sh:2311-2316`); externalize it and add the other five. *Nothing changes behaviorally.* This is the safety net for steps 1-3.

**Step 1 — R3 + R2 (S, S).** Swap `lines_mode`'s body for the awk one-pass (prototype verified byte-identical against all 6 goldens) and change the git invocation to `git diff HEAD`. Step 0's goldens catch any drift; add the two new fixtures (mixed staged/unstaged, untracked-only) as **fail-first** — they go red on today's code and green after. `-269 lines → ~40`, and the cap constant becomes a tunable.

**Step 2 — R5 mechanical split (M).** Move the awk to `scripts/lib/review-diff.awk` and the grammar to `scripts/lib/review-store.sh`. Keep every `--flag` name working as an alias to the new noun-verb command so `cockpit.tmux:249`, the fzf binds, and all 20 test sections keep passing untouched. *Pure refactor; the suite is the proof.*

**Step 3 — R4 (M).** Write `parse()` in `review-store.sh` and the round-trip property test. Reimplement `format()` on top of the parsed model. Decide the escape question explicitly (my recommendation: escape `## ` and `> ` on write, un-escape on read, and test both). Now `compose` validates instead of `cat`-ing.

**Step 4 — R1 (M).** Introduce the session dir. `snapshot` writes `diff.tsv` once; `rows` becomes a join over `diff.tsv` + parsed comments and stops calling git. Add `^R` re-snapshot with a stale-comment report. Add the mid-review-mutation test (fail-first: it reproduces §2.2 exactly as I did above). Point the session dir at `$XDG_STATE_HOME` and add resume.

**Step 5 — R6 + R9 + R7 + R10 (S/S/M/S).** Independent, parallelizable, no ordering constraint between them.

**Step 6 — R8 (L), only after 1-5.** `@cockpit_reviewer=revdiff` runs revdiff in the popup, pipes stdout through `parse`, hands the result to `deliver` unchanged. If the seam from steps 3-4 is right, this is roughly 30 lines. If it needs more than that, the seam is wrong and step 6 is the test that says so.

**Do not touch `deliver()` (`:471-562`) in any step.** It is the best-engineered function in the plugin — the pane-id shape guard, the glob re-check, the per-invocation buffer name, the `if-shell -F` atomic re-gate that closes the ~50ms TOCTOU, the `umask 077` spillover, the BSD-mktemp template detail — all live-verified, all tested (5 sections, `tests/run.sh:2775-3067`). Every proposal above routes *into* it and changes nothing about it.
