# Lens: hunk — as `@cockpit_reviewer` backend, and as a front-end idea source

Evaluated by installing hunk **v0.19.0** into the scratchpad and driving it live in a private
tmux server (`tmux -L lenshunk`, 160x45 and 90x45) against a purpose-built repo containing a
modified source file, a modified test, a deletion, a rename+edit, a new file, a very long line,
and a non-ASCII path (`docs/ünïcode-π/résumé.md`). Every number below is measured on this
machine unless tagged [I].

Tags: **[E]** = evidence I produced or read directly, **[I]** = inference.

---

## 0. Provenance

| Fact | Value |
|---|---|
| Repo | `modem-dev/hunk` |
| Stars | **8,712★** [E] |
| Last push | **2026-08-24T18:47:08Z** — same day [E] |
| License | **MIT** [E] |
| Language | TypeScript, on OpenTUI + `@pierre/diffs` [E] |
| Open issues | 160 [E] |
| Created | 2026-03-17 — five months old [E] |
| Latest release | v0.19.0, 2026-08-16 [E] |
| Release cadence | v0.18.0 → v0.19.0 in 8 days; 5 releases in ~3 weeks [E] |
| Sponsor | Modem (`modem.dev`), commercial backer [E] |

`gh api repos/modem-dev/hunk --jq '.stargazers_count,.pushed_at,.license.spdx_id'` →
`8712 / 2026-08-24T18:47:08Z / MIT` [E].

Maintenance velocity is the highest of the three candidates: commits landing the same hour I
ran the query, an extension API, a published OpenTUI component, a Discord, and a sponsor [E].
That is a project with momentum — and also one whose surface is still moving (the extension API
is explicitly "experimental and may change in breaking ways between minor releases") [E].

---

## 1. Install: the first trap

**The npm package is `hunkdiff`, not `hunk`.** [E]

I ran the command the brief suggested — `npm i -g --prefix … hunk` — and it succeeded, quietly,
in 1 second, installing **`hunk@1.0.6`, "Multipart files, one hunk at a time."**, a 2013 package
by Shannon Moeller with zero relationship to the diff viewer [E]. No error, no warning. A user
following a half-remembered install line gets a silent wrong answer. The correct line, from the
README, is `npm i -g hunkdiff` [E].

This matters for cockpit specifically: any install instruction we ship must say `hunkdiff`, and
any `command -v hunk` probe can be satisfied by the wrong package. A backend probe must be
`hunk --version` and match the expected shape, not `command -v hunk` [I].

Measured install (`npm i -g --prefix … hunkdiff`) [E]:

| Metric | Value |
|---|---|
| Wall time | **16.5 s** |
| Packages | **82** |
| Disk | **387 MB** |
| Platform binary | `hunkdiff-linux-x64/bin/hunk` = **162 MB** single file |
| npm peak RSS during install | 369 MB |
| Release tarball (linux-x64, compressed) | **49 MB** |

The 162 MB binary is a **bun-compiled** executable — `bun` appears as a dependency inside
`node_modules/` [E]. So the npm route ships a Node wrapper that execs a bundled-bun binary.
Alternative install routes exist and avoid npm entirely: `curl -fsSL https://hunk.dev/install.sh | sh`
(downloads the prebuilt binary, verifies checksum, installs to `~/.hunk`), Homebrew, mise, and
Nix [E]. Release assets cover darwin-arm64/x64, linux-arm64/x64, windows-x64 [E].

Stated requirements: Node 18+ for the npm install only; **on x86-64, a CPU with SSE4.2**
(Nehalem 2008+ / Bulldozer 2011+) [E]. That CPU floor is a real, if narrow, portability cliff
that a static Go binary does not have [I].

**Install rating: 3/5.** The `hunk`/`hunkdiff` name mismatch is a genuine footgun; 387 MB via
npm is heavy for a tmux plugin's optional dependency. Offset by four independent install routes,
checksum verification, and a single-file binary that needs no runtime once installed [E].

---

## 2. First run: it is very good

`hunk diff --staged` in a 160x45 pane, ~6 s after send-keys, produced a correct side-by-side
render of all seven files [E]:

```
  File  View  Navigate  Agent  Help                          testrepo staged changes  7 files  +14  -10
 ────────────────────────────────────────────────────────────────────────────────────────────────────
  docs/ünïcode-π/résumé.md                                                                     +1 -1
 ▌@@ -1 +1 @@
 ▌1 - # doc naïve café                        ▌1 + # doc naïve café — updated with émojis 🚀 and ümlauts
  ────────────────────────────────────────────────────────────────────────────────────────────────
  src/brandnew.js (new)                                                                        +3 -0
 ▌@@ -0,0 +1,3 @@
 ▌                                            ▌1 + export function brandNew(a, b) {
  ────────────────────────────────────────────────────────────────────────────────────────────────
  src/legacy.js (deleted)                                                                      +0 -3
 ▌@@ -1,3 +0,0 @@
 ▌1 - // to be deleted                        ▌
```

Everything cockpit gets wrong about hard cases, hunk gets right on the first frame [E]:

- **Non-ASCII path rendered literally** — `docs/ünïcode-π/résumé.md`, not git's
  `"docs/\303\274n\303\257code-\317\200/..."` octal escaping. Cockpit needed a dedicated fix for
  exactly this (`core.quotePath=false`, roadmap phase 0). hunk never had the bug [E].
- **Emoji and combining characters** in content render at correct width [E].
- **File status is typed in the header**: `(new)`, `(deleted)` — cockpit's flat ±line list
  cannot express "this whole file is gone" [E].
- **Per-file ± counts** in the right margin, and a global `7 files +14 -10` in the title bar [E].
- **Deleted-side gutter is drawn empty** rather than omitted, so the two columns stay aligned [E].

With `--sidebar` at 160 cols, a directory-grouped file tree with A/M/D status letters [E]:

```
  docs/ünïcode-π/                  │ docs/ünïcode-π/résumé.md                              +1 -1
   M résumé.md               +1 -1 │▌@@ -1 +1 @@
  src/                             │▌1 - # doc naïve café          ▌1 + # doc naïve café — updated…
   A brandnew.js                +3 │ ──────────────────────────────────────────────────────────
   D legacy.js                  -3 │ src/brandnew.js (new)                                 +3 -0
   A newname.js                 +2 │▌@@ -0,0 +1,3 @@
   D oldname.js                 -2 │▌                              ▌1 + export function brandNew(a, b) {
   M parser.js               +6 -4 │
  tests/                           │
   M parser.test.js             +2 │
```

**Responsive auto layout is real.** Resizing the same session to 90 cols flipped split → unified
automatically, and — the detail worth stealing — it kept **two line-number gutters** (old and
new) in unified mode, so you never lose which side a line belongs to [E]:

```
 ▌@@ -1 +1 @@
 ▌1   -  # doc naïve café
 ▌  1 +  # doc naïve café — updated with émojis 🚀 a
```

**First-run rating: 5/5.** No configuration, no onboarding wizard, correct on every hard case I
threw at it, and the responsive degradation is better than most GUI diff tools [E].

---

## 3. Keymap (captured from the in-app `?` overlay)

Full built-in help, verbatim from the running TUI [E]:

```
Navigation
 Up / Down                move line-by-line
 PageDown / Space / f     page down
 PageUp / b / Shift+Space page up
 d / u                    half page down / up
 [ / ]                    previous / next hunk
 , / .                    previous / next file
 { / }                    previous / next comment
 Left / Right             scroll code sideways (Shift = faster)
 g / Home                 jump to start
 G / End                  jump to end
Mouse
 Wheel                    scroll vertically
 Shift+Wheel              scroll code horizontally
View
 1 / 2 / 0                split / stack / auto
 s / t                    sidebar / theme selector
 a                        toggle AI notes
 z                        toggle unchanged context
 l / w / m / M            lines / wrap / metadata / menu
 e                        open file in $EDITOR
Review
 /                        focus file filter
 c                        create review note
 Tab                      toggle files/filter focus
 F10                      open menus
 r                        reload the review
 q                        quit
```

Three structural lessons for cockpit's fzf fallback [I]:

1. **Three-level navigation is bound to unshifted bracket-family keys** — `[`/`]` hunk,
   `,`/`.` file, `{`/`}` comment. All plain keys, no chords. Cockpit's picker is alt-chorded
   throughout (`scripts/tmux-review:400-402`: `alt-enter`, `alt-u`, `alt-d`) and has *no* file or
   hunk motion at all, because its list has no file or hunk structure [E].
2. **Commenting is a plain letter (`c`) and filtering is `/`.** These are separate modes on
   separate keys. Cockpit fuses them destructively: `fzf --disabled` makes *all* typed text a
   comment (`scripts/tmux-review:398`), which is why `enter:ignore` had to be bound
   (`:400`) and why the header must apologise, "(enter does nothing)" (`:406`) [E]. hunk shows
   the alternative costs one keystroke.
3. **`z` toggles unchanged context.** Cockpit's `--lines` mode emits *only* ±lines with no
   context, permanently (`scripts/tmux-review:77-112`). hunk makes context a runtime toggle
   rather than an architectural amputation [E].

**Day-to-day rating: 4/5.** Excellent motion vocabulary, mouse support, `$EDITOR` handoff,
runtime view toggles. Docked one point for the comment model (§5) rather than the keymap.

---

## 4. Performance: flat, but with a fixed ~0.9 s floor

I built three fixtures the way the perf lens did and measured **time-to-first-frame** as
wall-clock from `send-keys` to the first frame containing diff glyphs, polled at 50 ms [E]:

| Fixture | ± rows | TTFF | TUI+wrapper RSS | daemon RSS |
|---|---|---|---|---|
| perf100 | 200 | **0.799 s** | 151 MB | 94 MB |
| perf1000 | 2,000 | **0.919 s** | 166 MB | 92 MB |
| perf4900 | 9,800 (49 files) | **0.921 s** | 179 MB | 92 MB |

Two readings, and they point opposite ways:

**The good [E]:** TTFF is essentially **flat** — 0.80 s at 200 rows, 0.92 s at 9,800 rows. A
49× increase in diff size costs 15% more time. hunk is paying runtime/bundle startup, not diff
processing. It renders a 9,800-row diff with **no truncation warning and no "diff too large"
bailout** at all — I searched the rendered output for one and found none [E]. Cockpit, by
contrast, gives up entirely at 5,000 rows and shows a single "diff too large" row
(`scripts/tmux-review:110-112`) [E]. On the largest fixture hunk simply worked.

**The bad [E]:** the floor is ~0.9 s and ~**270 MB resident** (179 MB TUI + 92 MB daemon). For
comparison, cockpit's entire row-extraction pipeline on the *same* 9,800-row fixture:

```
$ time (git diff --cached | awk '/^[+-][^+-]/{n++} END{print n" rows"}')
9800 rows
real 0m0.008s
```

**8 ms** [E]. So on cockpit's own workload hunk is ~115× slower to first frame and roughly three
orders of magnitude heavier in memory. That is the honest trade: hunk buys side-by-side,
syntax highlighting, sidebar, and wrap for a fixed ~0.9 s / ~270 MB toll, paid on every popup
open [I]. Whether that is acceptable is a product call, but for the *fleet* case — cockpit's
distinguishing feature, opening a reviewer repeatedly across many agent panes — a 0.9 s modal
open is noticeably worse than the ~110 ms the roadmap targets for phase 1 (§8) [I].

`--fast` exists ("experimentally offload eligible syntax highlighting") but at these sizes the
bottleneck is startup, so it has nothing to bite on [I].

Interaction latency once loaded was not meaningfully measurable through `capture-pane` polling —
my `G` (jump-to-end) probe returned in 24 ms but that number is unreliable because the poll
grep matched a transitional frame [E]. Subjectively, from the captures, redraws were immediate.

---

## 5. The daemon: where hunk collides with cockpit's invariant

This is the decisive finding, and it is worse than the docs imply.

**5.1 Launching the TUI silently spawns a daemon.** Before I started anything, port 47657 was
closed [E]. After a single `hunk diff --staged` in a tmux pane, the process tree was [E]:

```
2503255  47996 KB  node …/hunk-prefix/bin/hunk diff --staged      # npm wrapper
2503263 179444 KB  …/hunkdiff-linux-x64/bin/hunk diff --staged    # the TUI
2503286  94204 KB  …/hunkdiff-linux-x64/bin/hunk daemon serve     # spawned child
```

```
$ ss -ltnp | grep 47657
LISTEN 0 512 127.0.0.1:47657 0.0.0.0:*  users:(("hunk",pid=2503286,fd=12))
```

Three processes, ~320 MB, and a **loopback listener on 127.0.0.1:47657** — none of which the
user asked for [E]. The docs describe this as "When a Hunk TUI starts, it registers with a local
loopback daemon" [E]; there is no flag to disable it.

**5.2 The daemon outlives the TUI.** I pressed `q`. The pane returned to a shell. Then [E]:

```
$ ps -eo pid,rss,etimes,cmd | grep 'hunk (diff|daemon)'
2503286  95952  122  …/hunkdiff-linux-x64/bin/hunk daemon serve
$ ss -ltnp | grep 47657
LISTEN 0 512 127.0.0.1:47657 …  users:(("hunk",pid=2503286,fd=12))
```

The TUI and its npm wrapper are gone; the **daemon is still listening**, orphaned, holding
96 MB [E]. It was still alive and listening minutes later, across multiple further test cycles
[E]. I had to kill it by PID at teardown. This is a direct, unfixable-from-outside violation of
cockpit's stated invariant "no daemon, one-shot scripts, state dies with the pane" (brief §
Invariants) [E].

**5.3 …but the review state does *not* outlive it.** Immediately after quit [E]:

```
$ hunk session list
No active Hunk sessions.
$ hunk session comment list --repo … --type all --json
hunk: No active session matches repoRoot …
```

So we get the worst of both: **the process persists and the data does not.** I searched for a
state file and there is none — no `~/.hunk`, no `~/.local/share/hunk`, no `~/.cache/hunk`, no
`.hunk/` in the repo, and the daemon's open file descriptors are only `/dev/null` and
`/dev/urandom` — no state file held open [E]. The only thing hunk wrote to disk in my entire
session was `~/.config/hunk/state.json`, 52 bytes: `{"version":1,"lastSeenCliVersion":"0.19.0"}`
[E]. Review comments live purely in daemon memory, keyed to a live session, and are discarded
when the TUI exits.

**Answer to the brief's question — "does the session state live in a file we can read after the
TUI exits?" — No.** [E]

**5.4 Stale sessions leak.** After I killed a TUI process without a clean `q`, `hunk session
list` still advertised the dead session (`perf4900 staged changes`) with a stale PID [E]. So the
daemon's registry can lie about what is live.

**5.5 Consequence for the popup model.** Cockpit's popup is modal and blocking
(`cockpit.tmux:247-250`, `display-popup -E`). A wrapper script inside the popup runs
*sequentially*: it cannot harvest comments after `hunk` returns, because by then the session and
its comments are gone. So the naive `hunk …; harvest` pipeline **cannot work** [E].

---

## 6. Making it work anyway: the harvest that does succeed

**6.1 Human notes are hidden behind an undocumented flag.** I typed a note in the TUI (`c`,
text, `^S`). It rendered inline as "Your note". Then [E]:

```
$ hunk session comment list --repo . --json
{ "comments": [] }
```

**Empty.** The default `--type live` returns only *agent*-created comments. The
agent-workflows doc's "Practical defaults" section never mentions this [E]. The human's notes
come back only with `--type user` (or `--type all`, or `review --include-notes`) [E]:

```json
{"comments":[{
  "noteId": "user:1787597977623-1",
  "source": "user",
  "filePath": "src/parser.js",
  "hunkIndex": 0,
  "oldRange": [1, 1],
  "body": "opts.sep default should be documented ?? why not a const",
  "author": "user",
  "createdAt": "2026-08-24T18:59:37.623Z",
  "editable": true
}]}
```

Any integration that used the documented default would silently deliver **zero** human comments
while appearing to work. That is exactly cockpit's failure mode to avoid [I].

**6.2 The JSON maps cleanly onto revdiff grammar.** A note on an added line, and an agent
comment for contrast [E]:

```json
{"noteId":"user:…-2","source":"user","filePath":"src/brandnew.js",
 "hunkIndex":0,"newRange":[2,2],"body":"no input validation here","author":"user"}
{"noteId":"mcp:3ba6012f-…","source":"agent","filePath":"tests/parser.test.js",
 "hunkIndex":0,"newRange":[3,3],"body":"agent: assert the trim option too\n\ncoverage gap",
 "editable":false}
```

Mapping to cockpit's `## path:line (+|-)` / `> fragment` / note [I]:

| cockpit grammar | hunk JSON |
|---|---|
| `path` | `.filePath` (already unquoted, UTF-8 clean) |
| `line` | `.newRange[0]` if present, else `.oldRange[0]` |
| `(+)` / `(-)` | **`newRange` present → `(+)`; `oldRange` present → `(-)`** |
| note body | `.body` (may contain `\n\n`; rationale is appended after a blank line) |
| `??` question rule | preserved verbatim inside `.body` — my test note round-tripped `??` intact [E] |
| `> fragment` | **absent** — must be recovered separately |

The one real gap is the **quoted fragment**, which the synthesis calls out as the axis where
"cockpit wins — keep" (§4). hunk's comment JSON carries no source text, only an anchor. Cockpit
would have to re-derive the fragment from `git diff` by `(path, side, line)`, or pay
`session review --include-patch` [I].

**6.3 A poller does work, and I proved it end-to-end.** Because state dies with the TUI but is
readable while it lives, a background poller can snapshot comments continuously and keep the
last good snapshot after exit. I ran exactly that: launched hunk, started a 0.4 s poll loop,
typed a note in the TUI, quit, and then read the file [E]:

```
=== harvested AFTER quit ===
{"filePath":"docs/ünïcode-π/résumé.md","body":"harvest test note",
 "oldRange":[1,1],"newRange":null}
```

The note survived the TUI's exit **in our file** [E]. The pattern is sound.

**6.4 Poll cost, and the 35× cheaper path.** The obvious implementation is expensive — every
`hunk session …` invocation re-execs the 162 MB binary [E]:

```
$ time (hunk session list)          real 0m0.280s
```

280 ms per poll. At 0.4 s intervals that is ~70% of a core for the entire review. But the CLI is
only a client for a plain HTTP endpoint, and talking to it directly is nearly free [E]:

```
$ time (curl -s -X POST http://127.0.0.1:47657/session-api \
        -H 'content-type: application/json' --data '{"action":"list"}')
{"sessions":[{"sessionId":"71c8c177-…","pid":2516084,"cwd":"…","repoRoot":"…", …
real 0m0.008s
```

**8 ms — 35× cheaper** [E]. I reverse-engineered the comment-list call shape by probing the
endpoint (the docs only show `{"action":"list"}`) [E]:

```bash
# works:
{"action":"comment-list","selector":{"repoRoot":"/abs/path"},"type":"user"}
# rejected — the selector key is repoRoot, not repo:
{"action":"comment-list","selector":{"repo":"…"}} 
  → {"error":"Invalid session API request: selector: Unrecognized key: \"repo\""}
```

This is an **undocumented private wire format**, which the extension API's own stability notice
suggests will move [I]. Depending on it is a maintenance liability; using the 280 ms CLI is the
supported path.

**6.5 hunk is tmux-aware, and that is a genuine gift.** `hunk session get` reports [E]:

```
Terminal: tmux
Location[tty]: /dev/pts/10
Location[tmux]: pane %0
```

hunk identifies **its own tmux pane id**. For cockpit — whose entire delivery guard is
"which pane does this review belong to" (`scripts/tmux-review:471-497`) — a reviewer that
self-reports `%pane` is a meaningfully better contract than parsing `$TMUX_PANE` by hand [I].

**6.6 It does run inside `display-popup -E`.** With an attached client, cockpit's exact
invocation shape worked, and hunk registered the popup's pane [E]:

```
$ tmux display-popup -E -w 95% -h 95% -d …/testrepo 'hunk diff --staged'
$ hunk session list
3bcd335c-…  testrepo staged changes
  terminal: tmux
  location[tmux]: pane %484
```

Two caveats found the hard way [E]: `display-popup` fails with `no current client` when no
client is attached (irrelevant in real use, relevant to any test harness we write); and popup
panes are **not addressable by `capture-pane`** (`can't find pane: %484`) because they are
client-side overlays — which forecloses any cockpit design that wanted to scrape the reviewer's
rendered output [E].

**6.7 The snapshot-export extension is not the escape hatch.** Recent commit #833
("feat(extensions): expose authoritative review snapshots") and the shipped
`examples/extensions/review-snapshot-export` do write full review state to JSON — but it is a
**user-driven command**: press `F9`, then choose a path in a dialog, and it "refuses to
overwrite an existing file" [E]. It is not an on-exit hook. An extension *could* be written
against a lifecycle event to auto-dump on teardown, but that means shipping and maintaining
TypeScript inside a bash/tmux plugin, against an API whose own README warns it "may change in
breaking ways between minor releases" [E]. Cost L, and it puts a TS toolchain in cockpit's
dependency story [I].

**Agent-integration rating: 3/5.** The CLI surface is the richest of the three candidates
(`list`/`get`/`context`/`review`/`navigate`/`reload`/`comment add|apply|list|rm|clear`/
`highlight`), the JSON is clean, and it ships a 214-line `SKILL.md` via `hunk skill path` [E].
But: mandatory daemon, daemon outlives the TUI, state does not, human notes hidden behind an
undocumented flag default, and the only cheap transport is a private wire format.

---

## 7. Integration sketch (what it would actually take)

The honest version is not ~30 lines, because the poller and its teardown are load-bearing.
This is ~40 and is the minimum that is correct [I]:

```bash
# scripts/tmux-review, backend: hunk
# Contract: hunk's state dies with its TUI, so we snapshot while it lives.
hunk_backend() {
    local dir=$1 store=$2 snap && snap=$(mktemp)
    command -v hunk >/dev/null || return 127
    hunk --version >/dev/null 2>&1 || return 127   # NOT `command -v hunk`: npm
                                                   # `hunk` is an unrelated package
    # Poller: keep the last good comment snapshot for as long as the session lives.
    ( while hunk session get --repo "$dir" >/dev/null 2>&1; do
          hunk session comment list --repo "$dir" --type user --json >"$snap.t" 2>/dev/null \
              && [ -s "$snap.t" ] && mv "$snap.t" "$snap"
          sleep 0.4
      done ) & local poll=$!

    hunk diff --sidebar        # blocks; the human reviews and presses q
    kill "$poll" 2>/dev/null; wait "$poll" 2>/dev/null

    # hunk leaves its daemon running. Cockpit's invariant says we clean up.
    pkill -f 'hunkdiff.*daemon serve' 2>/dev/null

    # JSON -> cockpit grammar. Fragment is re-derived from git, not from hunk.
    jq -r '.comments[] |
        (if .newRange then "+|\(.newRange[0])" else "-|\(.oldRange[0])" end) as $a |
        "## \(.filePath):\($a|split("|")[1]) (\($a|split("|")[0]))\n\(.body)\n"' \
        "$snap" >>"$store"
    rm -f "$snap" "$snap.t"
    [ -s "$store" ] && exit 20 || exit 0
}
```

Four things this sketch makes visible, and none of them are cosmetic [I]:

1. `--type user` is mandatory and non-obvious; the documented default silently yields nothing.
2. The `pkill` is cockpit *cleaning up after its dependency*. Pattern-matching on another
   project's process cmdline is fragile, and I hit exactly this hazard during testing: my own
   `pkill -f 'hunk daemon serve'` matched the shell whose command line contained that string and
   killed my session (exit 144) [E]. Any such guard must match on the binary path, not the words.
3. The `> fragment` line — cockpit's one acknowledged advantage over the field (§4) — is simply
   missing, and restoring it needs a second `git diff` pass keyed by `(path, side, line)`.
4. A 0.4 s poll of a 280 ms command is ~70% of a core; the 8 ms HTTP path costs an undocumented
   private wire format instead.

---

## 8. The verdict on hunk as `@cockpit_reviewer`

**Can cockpit run hunk in the popup and harvest comments without a surviving daemon? No.** [E]

- The daemon is spawned unconditionally by the TUI; there is no opt-out flag [E].
- It **outlives the TUI** and keeps 127.0.0.1:47657 bound until killed [E].
- The review state does **not** outlive the TUI, so harvesting requires a live poller [E].
- Therefore cockpit must both *run* a daemon it did not want and *kill* it afterwards, by
  cmdline pattern, to restore its own invariant [I].

That is three violations of "no daemon, one-shot scripts, state dies with the pane" for one
backend. hunk is the best *reader* of the three by a wide margin and the worst *citizen*.

---

## 9. First backend to wire: **revdiff**

| Axis | **revdiff** (807★, Go) | **tuicr** (2,868★, Rust) | **hunk** (8,712★, TS/bun) |
|---|---|---|---|
| Last push | 2026-08-20 [E] | 2026-08-20 [E] | 2026-08-24 [E] |
| Runtime deps | none — static Go binary [I] | none — static Rust binary [I] | **none post-install, but 387 MB via npm / 49 MB tarball; SSE4.2 floor** [E] |
| Daemon | none [I] | none [I] | **mandatory, outlives TUI, :47657** [E] |
| State after exit | stdout, already captured [I] | persisted sessions [I] | **evaporates** [E] |
| Output contract | **stdout + exit 10** [I] | `--stdout` [I] | JSON via daemon CLI, 280 ms/call [E] |
| Grammar | **identical to cockpit's already** [I] | own format [I] | maps cleanly, but **no `> fragment`** [E] |
| Popup | fine [I] | fine [I] | works [E] |
| TTFF | — | — | 0.9 s floor, flat [E] |
| Memory | — | — | ~270 MB [E] |
| Install on a TPM box | drop one binary | drop one binary | npm+Node, or curl-installer, or brew/mise [E] |
| License | — | — | MIT [E] |

**Wire revdiff first.** The reasoning is about the *seam*, not about which tool reads diffs best
[I]:

1. **It matches the contract cockpit already has.** stdout + a distinguished exit code is
   precisely cockpit's own `^S → exit 20 → deliver` idiom (`scripts/tmux-review:421-423`).
   revdiff's grammar is *already* cockpit's grammar, so `parse` is close to `cat`. Phase 4 says
   the seam should be "a pipe, not a rewrite" — with revdiff that is literally true; with hunk
   the "pipe" is a background poller plus a JSON transform plus a process reaper.
2. **It preserves every invariant unchanged.** No daemon to spawn, none to kill, no port, no
   state that outlives the pane. Cockpit does not have to document a relaxation.
3. **Install friction on a TPM machine is the deciding practical fact.** TPM users have bash,
   tmux, git, and often fzf — they do not reliably have Node. A single static binary is a
   `curl`; hunk's default npm route is 82 packages and 387 MB, behind a package name that
   silently resolves to the wrong project [E].
4. **It de-risks the switch itself.** The point of phase 4 is to prove `@cockpit_reviewer` as an
   abstraction. Proving it against the backend whose contract already fits isolates bugs in
   *our* seam. Wiring hunk first would conflate "our switch is wrong" with "we are fighting a
   daemon lifecycle."
5. **The fragment survives.** revdiff speaks cockpit's grammar including quoted fragments; hunk
   drops the one axis where §4 says cockpit beats the field.

**Then hunk second, as the opt-in "rich reader."** It is the strongest TUI here — flat TTFF on
huge diffs, side-by-side, sidebar, responsive, syntax highlighting, tmux pane self-reporting, an
extension API, and same-day commits. Once the seam is proven, `@cockpit_reviewer = hunk` is
worth offering to users who want real side-by-side and will accept a daemon. Gate it behind an
explicit opt-in and document the daemon, because it breaks a documented promise [I]. tuicr third
— persisted sessions are attractive but its format needs a real parser, and 2,868★/Rust offers
no advantage over revdiff for a *first* integration [I].

---

## 10. Steal-list for the fzf fallback (ranked)

| # | Steal | Replaces (cockpit) | Cost |
|---|---|---|---|
| 1 | **`c` comments, `/` filters** — separate keys, separate modes | `fzf --disabled` making all typed text a comment (`scripts/tmux-review:398`), forcing `enter:ignore` (`:400`) and the apologetic header "(enter does nothing)" (`:406`). This is the single highest-value idea here: it dissolves the dead-Enter bug class instead of documenting it. | **S** |
| 2 | **File-status vocabulary + per-file ± counts** — `(new)`, `(deleted)`, `A/M/D`, `+6 -4` | Cockpit's flat ±line rows (`scripts/tmux-review:77-112`) cannot say "this file is gone." Emit file-header rows carrying path, status letter, and counts — already half-planned in §7.5. | **S** |
| 3 | **Unshifted three-level motion** — `[`/`]` hunk, `,`/`.` file, `{`/`}` comment | Cockpit's all-alt chords (`:400-402`) and total absence of file/hunk motion. `{`/`}` "prev/next comment" is free once `●N` marks exist and is the natural review-your-review gesture. | **S/M** |
| 4 | **Two aligned gutters in unified mode** (old ‖ new ‖ code) | The padded-gutter plan in §7.5 — hunk shows the exact column layout that stays readable at 90 cols, and shows that *both* line numbers must survive the collapse. | **S** |
| 5 | **`--type user\|agent\|ai\|live` comment taxonomy** | Cockpit's single flat store. A `source` field costs nothing now and is the prerequisite for phase 4's `--seed` agent comments with the `◆` marker. Adopt the field even before the feature. | **S** |
| 6 | **`z` toggle for unchanged context** | `--lines`' permanent, architectural removal of context (`:77-112`). Make it a runtime toggle over cached rows rather than a property of the extraction. | **M** |
| 7 | **`e` → open file in `$EDITOR` at the anchor** | Nothing — cockpit has no escape to the real file. Complements the planned `^E` (compose-in-editor, §7.5); this is the *read* direction. `display-popup -e` already re-injects `EDITOR`. | **S** |
| 8 | **Agent-context sidecar** (`--agent-context notes.json`, rendered inline, `a` toggles) | Nothing today; this is exactly phase 4's `--seed`. hunk validates the shape: a JSON file of `{filePath, newLine, summary, rationale}` read at launch, displayed beside the diff, toggleable. Copy the schema. | **M** |
| 9 | **Ship `skills/cockpit-review/SKILL.md`** | Nothing — "the agent gets a paste it was never told about" (§4). `hunk skill path` prints a 214-line skill; its opening line ("The TUI is for the user — do NOT run `hunk diff` directly") is the right instinct: tell the agent which half of the tool is *not* for it. | **S** |
| 10 | **Self-reported location in the session record** (`Location[tmux]: pane %0`) | Cockpit's manual pane plumbing through `--deliver` (`:471-497`). If cockpit ever grows a session dir (phase 2), record `pane_id`, `tty`, and `repoRoot` in it the way hunk does. | **S** |

### What NOT to copy

- **The daemon.** Non-negotiable — it is the invariant, and hunk demonstrates the exact failure:
  a 96 MB orphan holding a port after the UI is gone [E].
- **In-memory-only review state.** hunk's comments die with the TUI [E]. Cockpit's `mktemp` store
  is already better, and phase 2's session dir is better still.
- **`^S` as "save one note."** hunk binds `^S` inside the note composer to save a *single*
  comment [E]; cockpit binds `^S` to *deliver the whole batch* (`:399`, `:421`). If cockpit ever
  gains a composer, `^S` must keep meaning "deliver," or muscle memory will send half-written
  reviews.
- **Mouse-first affordances** (menu bar, `F10` menus, click-to-select). They are well done, but
  fzf cannot express them and chasing them would distort the picker [I].
- **A 0.9 s / 270 MB floor.** Whatever cockpit builds natively should stay in the tens of
  milliseconds; hunk's flat scaling is admirable but its constant is 115× cockpit's awk pass [E].
- **`npm i -g hunk` in any documentation.** It installs the wrong package, silently [E].

---

## 11. Ratings summary

| Axis | Score | One-line justification |
|---|---|---|
| Install | **3/5** | Four install routes and checksum verification, undercut by a 387 MB npm tree and a package name that silently resolves to an unrelated 2013 project [E]. |
| First run | **5/5** | Zero config; correct on non-ASCII paths, emoji, deletions, renames, new files; responsive split→stack that keeps both gutters [E]. |
| Day-to-day | **4/5** | Excellent unshifted motion vocabulary, mouse, `$EDITOR`, runtime toggles; loses a point because comments vanish on quit [E]. |
| Agent integration | **3/5** | Richest CLI and cleanest JSON of the three, plus a shipped SKILL.md — but mandatory daemon, orphaned daemon, evaporating state, and human notes hidden behind an undocumented flag default [E]. |

---

## 12. Teardown

Everything started here was stopped and verified [E]:

```
=== hunk procs ===   NONE
=== port 47657 ===   FREE
=== tmux lenshunk === no server running on /tmp/tmux-1000/lenshunk
```

`~/.config/hunk/` (52 bytes, created by this run) was removed. The scratchpad still holds the
387 MB `hunk-prefix/` install, the fixture repos, and `review.json` / `harvest.json`; nothing
outside the scratchpad was modified, and the cockpit repo was not touched.
