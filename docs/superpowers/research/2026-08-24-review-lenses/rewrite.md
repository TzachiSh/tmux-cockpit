# Lens: Should tmux-cockpit be rewritten in Go or Rust?

Scope: the whole plugin, not just the reviewer. Owner's ask: *"also check to replace all to go or rust."*

**Verdict up front: No — not "all", and the performance premise for doing so is falsified by measurement.**
The one place a compiled binary genuinely buys something no amount of bash can (a real diff TUI with
hunks, context and multi-line comments) is a *product* decision worth ~15 person-days, not a
*performance* one, and it is scoped to one script out of eight. Everything else that is actually slow
today is slow because of `bash` loops and un-batched `tmux` calls — both fixable in-language, one of
them proven below to be **byte-identical output at 6–19× the speed for ~1 person-day.**

All benchmarks below were run on this machine (12 cores, mawk 1.3.4, rustc 1.98.0, tmux 3.7-class,
Linux). Go is **not** installed (`which go` → nothing); `cargo`/`rustc` are. Rust numbers are therefore
measured; Go numbers are cited from published sources and marked [I].

---

## 0. What the plugin actually is

| file | lines | what it is |
|---|---|---|
| `cockpit.tmux` | 352 | tmux **format strings + key bindings**. Almost no logic. |
| `scripts/tmux-review` | 743 | diff parse, fzf review loop, compose, deliver, fleet queue |
| `scripts/tmux-open-path` | 590 | click→path resolution, previews |
| `scripts/tmux-where` | 256 | status-line `#()` job (path/branch, ssh-aware) |
| `scripts/tmux-branch` | 246 | branch picker |
| `scripts/agent-state` | 220 | Claude Code hook handler, writes tmux options |
| `scripts/tm` | 201 | session picker |
| `scripts/agent-context` | 51 | statusline shim (the only `jq` user) |
| `scripts/tmux-jump` | 38 | cycle to blocked panes |
| `install.sh` + `bootstrap.sh` | 508 | symlinks, hook merge, full machine setup |
| `tests/run.sh` | **3306** | ~83 sections, ~254 assertions, private `-L` tmux servers |

**~3,200 lines of production shell against 3,306 lines of tests.** [E]

The test rig is worth calling out because it dominates every migration estimate:
`tests/run.sh:24-56` builds a **PATH shim** that redirects every bare `tmux` — *including the calls
inside the scripts under test* — to a private `-L cockpit-suite-$$` server, and defers when `$TMUX`
is set so nested servers still work. It then spins ~17 more throwaway servers per run and cleans up
their socket inodes (`tests/run.sh:60-71` — a real bug they hit: ~850 dead sockets had piled up). [E]
This suite tests *tmux integration behaviour*, not shell functions. **A language rewrite does not
make it go away — it has to be ported nearly line-for-line, because what it asserts is "what does
tmux render / what does the pane contain", which is language-neutral.**

---

## 1. Precedents: what the community actually ships

Verified via `gh api repos/OWNER/REPO --jq '.stargazers_count,.pushed_at,.language'` on 2026-08-24. [E]

### tmux plugins (TPM-installed, live in `~/.tmux/plugins`)

| project | stars | last push | language |
|---|---:|---|---|
| tmux-plugins/tpm | 15,019 | 2026-05-17 | **Shell** |
| tmux-plugins/tmux-resurrect | 13,013 | 2024-08-13 | **Shell** |
| catppuccin/tmux | 3,154 | 2026-06-18 | **Shell** |
| tmux-plugins/tmux-continuum | 4,062 | 2024-08-02 | **Shell** |
| tmux-plugins/tmux-sensible | 2,217 | 2024-04-16 | **Shell** |
| sainnhe/tmux-fzf | 1,490 | 2026-08-10 | **Shell** |
| omerxx/tmux-sessionx | 1,365 | 2026-04-18 | **Shell** |
| Morantron/tmux-fingers | 1,465 | 2026-06-22 | Crystal (compiled) |
| fcsonline/tmux-thumbs | 1,095 | **2024-04-20** | Rust |

### tmux-adjacent tools (NOT TPM plugins — standalone binaries you bind yourself)

| project | stars | last push | language | how you install it |
|---|---:|---|---|---|
| joshmedeski/sesh | 2,783 | 2026-08-21 | Go | brew / `go install` / AUR / nix / conda / **9 release assets** |
| arl/gitmux | 791 | 2026-02-17 | Go | brew / `go install` / **6 release assets** |
| jrmoulton/tmux-sessionizer | 687 | 2026-07-07 | Rust | `cargo install` / cargo-dist installer / **13 release assets** |

### The finding that matters

**Every actively-maintained Go/Rust tool in this space escaped TPM entirely.** [E]
`sesh` and `gitmux` are not plugins — they are CLIs installed by a package manager that you then bind
to a tmux key yourself. `gitmux` is the *exact* analogue of `#{agent_where_branch}` (a Go binary
producing a status-line git segment) and its README's first selling point is literally
*"easy: install and forget"* — because it had to solve distribution before it could solve anything else.

**The one Rust project that stayed a TPM plugin is the cautionary tale.** `fcsonline/tmux-thumbs`
(1,095★) is TPM-installable, but its README says: TPM install, *then* "recommended way to install Rust
is from the official download page… `cargo build --release`". Its **last release is 2023-03-11, x86_64
only — no arm64 macOS asset at all**, and the repo has not been pushed since **2024-04**. [E]
Meanwhile `Morantron/tmux-fingers` — the *bash project tmux-thumbs was written to replace* — was
rewritten in Crystal, ships exactly **two** prebuilt binaries (linux-x86_64, macos-arm64), is still
actively pushed (2026-06), and has **more stars** (1,465) than the Rust one that replaced it. [E]

**How a compiled TPM plugin has to be shaped** (`tmux-fingers.tmux`, fetched): [E]

```bash
if command -v "tmux-fingers" &>/dev/null;      then FINGERS_BINARY="tmux-fingers"
elif [[ -f "$CURRENT_DIR/bin/tmux-fingers" ]]; then FINGERS_BINARY="$CURRENT_DIR/bin/tmux-fingers"
fi
if [[ -z "$FINGERS_BINARY" ]]; then
  tmux run-shell -b "bash $CURRENT_DIR/install-wizard.sh"; exit 0
fi
CURRENT_FINGERS_VERSION="$($FINGERS_BINARY version)"
CURRENT_GIT_VERSION=$(cat shard.yml | grep "^version" | ...)
if [ "$SKIP_WIZARD" = "0" ] && [ "$CURRENT_FINGERS_VERSION" != "$CURRENT_GIT_VERSION" ]; then
  tmux run-shell -b "FINGERS_UPDATE=1 bash $CURRENT_DIR/install-wizard.sh"
```

So even the successful compiled plugin keeps **a bash `.tmux` loader**, plus a download wizard, plus a
**version-drift check between the git checkout and the binary** — a whole failure class (`git pull`
updates the tmux formats but not the binary) that does not exist today. [E]

### General-tool precedent (relevant to option E)

lazygit 81,598★ (Go) · gitui 22,427★ (Rust) · delta 31,850★ (Rust) · difftastic 25,813★ (Rust) ·
zellij 35,092★ (Rust) · jj 31,165★ (Rust) · fzf 82,636★ (Go). [E]
These are all *destinations you shell out to*, not libraries you vendor. That is option E's whole case,
and **this repo already does exactly that pattern**: `examples/tmux.conf:45` and `cockpit.tmux:139`
reuse catppuccin's **gitmux slot** (`@catppuccin_gitmux_text`) — `examples/full.tmux.conf:234` comments
it as *"git branch via plain git — reuses the gitmux slot without the gitmux binary"*. [E]

---

## 2. Where a compiled binary would actually pay off — measured

### 2a. Process startup: **no gain whatsoever.** This kills the headline argument.

200 invocations each, identical harness:

| what | ms / invocation |
|---|---:|
| `sh -c :` | **1.665** |
| `rust hello (opt-level=z, stripped, 349 KB)` | **1.804** |
| `/bin/true` | 1.898 |
| `bash -c :` | **1.971** |
| `rust hello (default -O, 4.4 MB)` | 2.206 |
| `jq --version` | 2.281 |
| a real bash script (`set -u`, arg validation, 2 `case`) | 2.263 |

[E] **A stripped Rust hello-world is 1.80 ms; `bash -c ':'` is 1.97 ms. The difference is 0.17 ms.**
The default (unstripped, 4.4 MB) Rust binary is *slower to start than bash* — 2.21 ms — because
page-in cost exceeds anything the interpreter saves. On Linux, `fork`+`exec`+dynamic-link dominates,
and every language pays it identically. The claim "compiled binaries start faster so status-line and
hook calls get cheaper" is **false at this granularity**.

### 2b. What *does* dominate: the tmux client roundtrip

| what | ms |
|---|---:|
| 1× `tmux show-option -gqv` (full client→server→client) | **7.465** |
| 1× `tmux display-message -p <format>` | **5.537** |
| `agent-state running <pane>` end-to-end (~6 tmux calls) | **10.525** |

[E] **A single tmux roundtrip costs ~4× the entire bash interpreter startup.** `agent-state` spends
~10.5 ms per hook, and essentially all of it is talking to the tmux server. A Go or Rust rewrite that
still shells out to `tmux` — which is what every option except a from-scratch protocol client does —
**would save the ~2 ms of interpreter startup and keep the ~8.5 ms of tmux chatter. A ~19% win on one
hook.** Not a reason to rewrite 3,200 lines.

And the actual fix is language-independent and already available:

| | ms |
|---|---:|
| 6 tmux commands as 6 separate client calls | **20.301** |
| the same 6 batched into ONE call with `;` separators | **3.877** |

[E] **5.2× faster, in bash, today, by joining commands with `\;`.** `agent-state` makes ~6 calls
(`show-option`, `set-option`, two `display-message` fleet walks, `show-option`, `set-option`) that are
largely batchable. That is the single highest-leverage perf change in the repo and it involves no new
language.

### 2c. The diff parser: the one real bash bottleneck — and awk already closes it

`tmux-review --lines` (`scripts/tmux-review:75-265`) does `git diff` → one `awk` pass → **a bash
`while read` row-build** with associative arrays, `printf -v`, churn ordering and a test-file
partition. Component split on a 4,800-row diff: `git diff` = 10.7 ms, the `awk` pass = 6.7 ms,
**total = 246–459 ms.** [E] ~95% of the time is the bash loop *after* awk.

This matters more than the raw number, because **`run_loop` rebuilds the entire list via fzf
`reload()` after *every single comment*** (`scripts/tmux-review:399-401`) — so this cost is paid per
keystroke-batch, not once per popup.

I wrote a **portable single-pass `awk` + `sort` + `cut` replacement** (mawk-safe: no `asorti`, no gawk
extensions — sort keys are emitted as a prefix and stripped by `cut`) and a **Rust equivalent**, and
verified both produce output **byte-identical** to the shipped bash (`cmp -s` → IDENTICAL, including
the test-file-first / churn-desc ordering). [E]

| changed rows | **bash (shipped)** | **awk pipeline** | **Rust** |
|---:|---:|---:|---:|
| 48 | 44.9 ms | 6.6 ms | 4.2 ms |
| 200 | 69.7 ms | 4.9 ms | 3.4 ms |
| 496 | 83.5 ms | 27.0 ms | 12.4 ms |
| 1,000 | 135.6 ms | 11.6 ms | 8.3 ms |
| 2,496 | 364.0 ms | 26.8 ms | 11.1 ms |
| **5,000 (the cap)** | **892.2 ms** | **47.4 ms** | **11.0 ms** |

[E] Read the last row carefully. At the code's own 5,000-row cap (`tmux-review:112`), **the shipped
bash takes 0.9 seconds — per comment.** That is a real, felt defect. awk fixes it to 47 ms. Rust
takes it to 11 ms.

**Both awk and Rust are under the ~100 ms perception threshold across the entire legal input range**
(above 5,000 rows the code bails to a single "diff too large" row). So the residual Rust-over-awk gain
is **36 ms that no human can perceive, at the cost of a toolchain, a release pipeline, and a
version-drift failure mode.** The perf case for a native rewrite is real against *bash* and
**entirely captured by awk**, a dependency the repo already has (9 `awk` call sites). [E]

### 2d. The fleet scan: parallelism is real, but it is not a language feature

`fleet_rows` (`tmux-review:576-628`) runs `git status --porcelain` + `git diff --numstat` serially per
agent pane.

| | ms |
|---|---:|
| `git status --porcelain` | 2.33 |
| `git diff --numstat` | 4.21 |
| per-pane serial cost | **6.54** |
| 16 panes, serial | **104.6** |
| **16 panes, `xargs -P8`** | **38.8** |

[E] **2.7× from `xargs -P8`, in bash, today.** Goroutines/threads would land in the same neighbourhood,
because the cost is *`git` process spawn*, not the orchestrating language. Beating 38 ms requires
in-process git (`go-git` / `gix` / `git2`), which trades a correctness-critical dependency
(sparse checkouts, worktrees, submodules, `core.quotePath`, index formats — all of which this repo has
already been bitten by; see the non-ASCII path bug at `tmux-review:75-88`) for ~30 ms. Bad trade.

### 2e. JSON hook payloads: a wash

Only `agent-context` uses `jq` (2 invocations, ~2.3 ms each). `agent-state:57-90` deliberately parses
the `"message"` field with **bash parameter expansion, not jq**, and says why: *"jq is not a dependency
here — and a subprocess per call is exactly what the perf gate below rules out."* [E] Native JSON
would save ~4.6 ms on the statusline shim only. Genuine but tiny.

### 2f. Cross-platform: the *real* non-perf argument for compiling

This is the strongest honest pro-rewrite point.

Five scripts carry an identical **bash-4 guard** (`tmux-review:20-26`, `tmux-where:32-38`,
`tm:10-17`, `tmux-branch:10-17`, `tmux-open-path:26-31`) that hard-exits on macOS's stock bash 3.2
with *"On macOS: brew install bash"*. The comment explains the stakes: without it *"`declare -A` errors,
then the same assignment quietly creates an INDEXED array with an arithmetic subscript"* — silent
wrong answers. [E] The repo is also littered with BSD-vs-GNU scar tissue: `mktemp` template rules
(`tmux-review:497-509`), `readlink -f` absent before macOS 12.3 (`tm:19-22`), `column(1)` optional
(`tests/run.sh:791`), non-GNU awk portability (`tmux-review:145`). [E]

**A compiled binary makes this entire class disappear.** That is worth real money — but note it does
**not** buy you out of tmux-format differences or tmux version gates (3.3 floor, 3.7 for
`MouseDown1Control1`, `cockpit.tmux:165-186`), which are the *other* half of the portability surface
and are language-immune.

### 2g. Testability: partially real, mostly not

Unit-testing a diff parser in Go/Rust is unambiguously nicer than driving tmux. But look at what the
3,306-line suite actually asserts: *"the REPO's shipped config renders correctly"*, *"a directory name
can no longer run code through the status job"*, *"…and neither can a NEWLINE or TAB, which `#{q:}`
alone does not escape"*, *"the picker fires even where the app owns the mouse"*, *"the paste is gated
atomically"*, *"concurrent deliveries do not cross-wire panes"*. [E] **These are tmux-integration
assertions. They survive any rewrite unchanged.** Realistically maybe 15–20% of the suite (the
`--lines` parser cases, the compose grammar) converts to fast unit tests; the rest still needs a live
server.

---

## 3. Where a rewrite costs

1. **Distribution.** Today: `git clone` + one `run-shell` line, or TPM. That is the entire install
   (`README.md:150-185`). A binary means either (a) release assets for ≥6 platform/arch pairs plus a
   download wizard plus a version-drift check (the tmux-fingers shape, verified above), or (b)
   "install a Go/Rust toolchain first" (the tmux-thumbs shape — **stalled since 2024, no arm64 mac
   asset**). Both are strictly worse than what exists.
2. **`cockpit.tmux` cannot be rewritten.** Its 352 lines are ~90% tmux format strings, key bindings,
   version gates and option interpolation. `interpolate()`, the `#{E:@cockpit_fleet}` indirection, the
   `sq()` single-quote rule (*"not `printf %q`: tmux processes backslash escapes and would eat %q's
   before sh ever saw them"*, `cockpit.tmux:31-34`) — **no language replaces any of this.** Any option
   B/C/D ships a bash loader forever.
3. **The 3,306-line suite** must be ported, and mostly to *the same shape* (drive a private tmux
   server, assert on rendered output). This is the largest single line item in every estimate.
4. **`state = tmux options` is language-neutral.** The freeze fix (`agent-state:139-200`) — precompute
   `@cockpit_win_needs` and `@cockpit_fleet` at transition time so a redraw walks no panes — is an
   *architecture* win worth far more than any language change, and it is already done, in bash.
   A rewrite inherits it, improves it by nothing.
5. **Contributor base.** tmux plugin contributors write shell. The 15,019★ plugin manager is shell.
6. **Sunk domain knowledge.** The comment density here is extraordinary and encodes measured findings
   (`#{m/r:}` at 32–36 µs/test → glob matching dropped redraw 16.2 ms → 7.7 ms, `cockpit.tmux:106-112`;
   `refresh-client -S` costing 19.6 ms/transition, `agent-state:215-220`). A rewrite re-litigates
   every one of these against a new substrate.

---

## 4. Decision matrix

Perf gains are measured on this machine except where marked [I].

| | effort (person-days) | risk | perf gain | UX gain | migration path |
|---|---|---|---|---|---|
| **A. Stay bash + targeted fixes** | **2–4** | **very low** | **diff parse 6–19× (892→47 ms at cap); fleet 2.7× (105→39 ms); agent-state ~5× on batched tmux calls (20.3→3.9 ms)** | none directly — but removes a 0.9 s per-comment stall that currently makes the reviewer feel broken | incremental, per-script, suite stays green throughout |
| **B. Everything → one Go binary + thin `cockpit.tmux`** | **25–40** | **high** | ~0 on startup (2b); ~36 ms over option A on the parser — imperceptible; kills bash-4/BSD class | real diff TUI possible (bubbletea) | big bang; must build 6-platform release CI + download wizard + version-drift check before day one |
| **C. Everything → Rust** | **30–50** | **high** | same as B; parser measured 11 ms vs awk 47 ms | same as B (ratatui) | same as B, worse: precedent is tmux-thumbs (stalled 2024, x86_64-only) |
| **D. Hybrid: bash glue + Go binary for review TUI / parser / fleet, bash fallback** | **12–18** (+ permanent ~2× maintenance if the fallback is real) | **medium** | parser + fleet as B | **the real one: hunk navigation, context lines, multi-line comments, inline comment display, live re-render as the agent edits** | additive; binary optional at first. **Drop the "bash fallback" — two implementations of the parser is how they drift.** |
| **E. Shell out to existing binaries (`delta`, `difftastic`, `gitmux`)** | **1–3** | **low** | n/a (preview quality, not speed) | much better `--preview-diff` rendering for free | pure addition, feature-detect and degrade; **repo already uses this pattern for the gitmux slot** |

---

## 5. Go vs Rust, if it happens anyway

| | Go | Rust |
|---|---|---|
| TUI | **bubbletea 44,523★** + lipgloss — the de-facto standard; `sesh` and `gum` are in-family | ratatui 22,360★ + crossterm — excellent, smaller ecosystem |
| diff | `sergi/go-diff` — adequate | `similar` — better quality |
| git | `go-git` — pure-Go but notoriously incomplete vs real git | `gix`/`git2` — `git2` is libgit2 FFI (drops single-file distribution); `gix` still maturing |
| binary size | ~8–12 MB typical [I] | **measured here: 4.4 MB default, 349 KB stripped + `opt-level=z` + `panic=abort`** |
| **cross-compile** | **`GOOS`/`GOARCH`, zero toolchain setup — decisive** | needs `cross`/`zig`; macOS-from-Linux is genuinely painful |
| startup | ~2 ms [I] | **measured 1.80–2.21 ms** (§2a) |
| release tooling | **goreleaser** — `sesh` ships 9 assets, `gitmux` 6 | cargo-dist — `tmux-sessionizer` ships 13 |
| precedent *in this exact niche* | **`sesh` 2,783★ active; `gitmux` 791★ active** | `tmux-thumbs` 1,095★ **dead since 2024**; `tmux-sessionizer` 687★ active |

**If a binary is written, write it in Go.** Cross-compilation and goreleaser are the whole ballgame for
a tool whose distribution problem is "six platform/arch pairs on a TPM clone", and the two healthy
Go precedents (`sesh`, `gitmux`) are the two closest analogues to this plugin that exist. Rust's
measured advantages here — 36 ms on a parser nobody perceives, and a 349 KB binary — do not touch the
actual constraint.

---

## 6. Recommendation

**A + E now. Hold D in reserve, scoped to the review TUI only, and only if "a real diff TUI" is a
product goal you'd fund on its own merits. Reject B and C.**

The owner's question is framed as performance. Measurement says performance is **not** a language
problem here:

- process startup: bash 1.97 ms vs stripped Rust 1.80 ms — **0.17 ms** [E]
- the actual hot cost is the tmux roundtrip (7.5 ms), which **every** option pays [E]
- the one genuinely slow path (892 ms at the diff cap) drops to **47 ms with awk** — **byte-identical
  output, verified with `cmp`** — for ~1 person-day and zero new dependencies [E]
- the fleet scan drops 105 ms → 39 ms with `xargs -P8`, also in bash [E]

Against that, "all Go/Rust" costs 25–50 person-days, replaces a one-line install with a
six-platform release pipeline in an ecosystem whose two biggest projects (tpm 15,019★, tmux-resurrect
13,013★) are shell, keeps a bash `cockpit.tmux` regardless, and follows a precedent
(`tmux-thumbs`) that visibly stalled.

**The honest pro-rewrite argument is not speed — it is (a) killing the macOS bash-3.2 / BSD-mktemp
class, and (b) fzf being a hard ceiling on review UX.** (a) is real but cheap to keep living with.
(b) is real and *unfixable in bash*: fzf's model is a flat list plus a preview, which is exactly why
the current design degenerates to one row per ±line with `--disabled` so typed text becomes the
comment. Hunks, context lines, multi-line comments, inline comment display and live re-render as the
agent edits are all off the table until something other than fzf draws the screen.

### Phase plan (recommended path)

**Phase 1 — parser (≈1 day, do this first).** Replace the bash row-build in `lines_mode`
(`tmux-review:196-265`) with the single-pass awk + `sort` + `cut` pipeline. Prototype exists and is
`cmp`-identical on the 4,800-row corpus; keep the existing `--lines` tests as the equivalence oracle
and add a golden-file case. **892 ms → 47 ms at the cap; kills the per-comment stall.** Raise or drop
the 5,000 cap afterwards — it exists only because the bash loop was quadratic.

**Phase 2 — batch tmux calls (≈0.5 day).** Join `agent-state`'s roundtrips with `\;`. Measured
20.3 ms → 3.9 ms for 6 commands. Highest leverage per line changed in the repo; directly serves the
"hooks never block the agent" invariant.

**Phase 3 — parallelise the fleet scan (≈0.5 day).** `xargs -P` over the per-pane git calls in
`fleet_rows`. 105 ms → 39 ms at 16 panes. Watch output interleaving (emit fixed-width churn-prefixed
rows, sort after — the pipeline already sorts).

**Phase 4 — option E (≈1–2 days).** Feature-detect `delta`/`difftastic` in `--preview-diff` and use it
when present, plain `git diff` otherwise. Free preview quality, zero new hard dependencies, and it is
the pattern the repo already uses for the gitmux slot.

**Phase 5 — decision gate, not a task.** After 1–4, re-ask the question with the perf argument off the
table: *is a real diff TUI (hunks, context, multi-line comments, live re-render) worth ~15 person-days
and a release pipeline?* If yes → one **Go** binary, `cockpit.tmux` stays bash and locates it
tmux-fingers-style (PATH → bundled `bin/` → download wizard, with a version check), goreleaser for 6
targets, and **no bash fallback for the TUI** — ship fzf as the documented degraded mode instead, so
there is never a second implementation of the parser to drift. If no → the plugin is done being
rewritten, and A+E already bought every millisecond that was actually on the table.
