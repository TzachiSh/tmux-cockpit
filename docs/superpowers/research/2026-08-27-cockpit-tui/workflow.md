# cockpit-tui: the parallel implementation workflow (#61)

Read: ratatui.md §Testing/§Packaging/§Popup/§Phased plan, the demo crate
(`spike/cockpit-tui-demo/`, 1,982 lines / 12 tests / 7 goldens), AGENTS.md,
Makefile, tests/run.sh (prelude + block shape), two house plans
(2026-08-25-side-fixer.md, 2026-08-25-review-phase3.md), 2026-08-26-issue-cli-flow.md,
`git log -40`, `gh pr list --state merged -L 30`, and ten memory rules.

## 0. Ponytail audit — before any lane forks

| asked for | verdict | why |
|---|---|---|
| "L0 crate skeleton" | **replace: L0 is a file move** | the demo already builds, renders, ticks at 13 ms and passes 12 golden tests. L0 copies `spike/cockpit-tui-demo/` into `tui/` and deletes what phase 0 does not ship (`ask.rs`, `fleet.rs`'s interactive `run()`, `info.rs`). Nobody writes a crate from scratch; L1/L2 *harden* code that exists. |
| 8 lanes | **keep 7, fold L7** | phase 2 (inline reply) is a separate issue after phase 1 ships; scoping it now is speculative. It gets a lane number here so the DAG is complete, and an issue only when the board is merged. |
| stub modules in L0 so lanes never collide on `lib.rs` | **keep** | this is the whole reason the lanes are parallel: `lib.rs` lands complete on day one and no lane ever edits it. It is not scaffolding, the modules have bodies. |
| a lane for `tests/run.sh` blocks | **skip** | each lane appends its own block; the seam is "both sides appended" and `bash -n` catches the dropped `fi` (seen on the side-fixer merge). |
| register `cockpit-tui` in `tests/changed.sh`'s `SCRIPTS` | **skip** | `changed.sh` is a developer loop, not a gate; an unmapped block falls through to `ALL`, which is correct-but-slow, and `make gate` runs everything anyway. |
| a new `make` target per lane | **skip** | one `make tui`. `scripts/*` is globbed by `Makefile` `SOURCES`, `green.sh`'s manifest and `make lint` — a new shell script needs zero registration. |
| CI matrix (3 OS × 2 toolchains) | **replace: one ubuntu job** | there is no `.github/` in this repo at all today. One `tui.yml` running `make tui`; cargo-dist's `release.yml` brings the macOS runners when L4 lands. |

## 1. The lanes and the DAG

```
            ┌── L1 data      ─┐
            │   L2 render    ─┤
L0 skeleton ┼── L3 push/hook ─┼── L6 integration ── (L7 phase 2, later issue)
            │   L4 packaging ─┤
            └── L5 docs      ─┘
```
L0 blocks the merge of everything (not the *start*: L2, L4, L5 need no L0 file).
L6 merges last. L1–L5 never touch the same file — that is the design.

### L0 — skeleton (blocks everything, merges first)
- **Inputs:** `spike/cockpit-tui-demo/` from PR #55's worktree
  (`/home/tzachi/.cache/cockpit-wt/issue-54-research/docs/superpowers/research/2026-08-26-ask-busy-agent/spike/cockpit-tui-demo/`).
- **Outputs:** `tui/Cargo.toml`, `tui/src/{main,lib,data,board,theme,push}.rs`,
  `tui/tests/screens.rs` + `tui/tests/golden/*`, `.github/workflows/tui.yml`,
  `Makefile` (`tui:` target), `.gitignore` (`bin/`, `tui/target/`),
  `tests/run.sh` block `== cockpit-tui: --json names every agent pane and nothing else ==`.
- **Contract it publishes (the freeze, §3):** `lib.rs`'s complete module list,
  `data::Pane`, the CLI, `COCKPIT_*`, the action lines, exit codes.
- **Fail-first:** the bash block asserts `--json` has one object per `claude` pane
  with the 14 frozen keys, and **skips** via `skip_` when `bin/cockpit-tui` is absent.
  Run it before the binary exists: it must print SKIP, not PASS and not FAIL.
- **Gate:** `make check` green **with cargo removed from PATH**; `make tui` (build only) and `make tui-test` (cargo test, clippy -D warnings, fmt --check) green;
  `bash -n tests/run.sh`.
- **Size:** ~2,000 lines moved, ~150 new. Half a day.
- **Day one: yes, and it is the only thing on the critical path.** Push the branch
  within hours so the others can fork from it.

### L1 — data layer (parallel)
- **Inputs:** L0's `tui/src/data.rs`.
- **Outputs:** `tui/src/data.rs` only. The 60 s `claude agents --json` cache, the
  `fs::metadata().modified()` gate on transcript tails, `--rows`/`--json` moved
  behind stable formatting, `COCKPIT_READONLY` honoured on every write path.
- **Contract:** `list_panes() -> Option<Vec<Pane>>`, `preview(id:&str, n:usize) -> Option<String>`
  (`None` = capture-pane failed, never an empty preview),
  `hash_rows(&[Pane], extra:&str) -> u64`, `one_pane(id:&str) -> Option<Pane>`,
  `tmux(&[&str]) -> Option<String>`, `log(&str)`. **Signatures are frozen by L0; L1 may
  change bodies only.** JSON shape per §3. The three `Option` returns are freeze
  amendment 1 and its addendum on #61: `None` is "tmux could not be spoken to",
  which is how `--rows`/`--json` reach exit 1 and how the board keeps its last
  good tile list through a transient failure.
- **Fail-first:** (a) a PATH-shim `claude` that appends to a counter file — two
  `list_panes()` calls inside 60 s fork it **once** (the 380 ms fact); (b) a
  transcript whose mtime is untouched is not re-parsed (shim `open` count via a
  fixture with a poisoned second line that would panic if read twice);
  (c) `COCKPIT_READONLY=1` makes zero `set-option` calls (tmux shim log is empty).
- **Gate:** `make tui` + `make tui-test`; the L0 bash block still green against the built binary.
- **Size:** ~120 lines changed, ~90 test. Half a day. **Day one after L0's push.**

### L2 — board renderer (parallel, no tmux at all)
- **Inputs:** L0's `board.rs`, `theme.rs`, `tests/screens.rs`; the `screens`/`ux-rust`
  lanes' UI decisions on #61.
- **Outputs:** `tui/src/board.rs`, `tui/src/theme.rs`, `tui/tests/screens.rs`,
  `tui/tests/golden/board-{80,120,160}.txt`.
- **Contract:** `board::render(f:&mut Frame, area:Rect, v:&View, st:&mut TableState)`
  is pure — it takes `&[Pane]` and never calls `data::tmux`. `board::run() -> Option<String>`
  is the only impure entry and returns an action line from §3.
- **Fail-first:** goldens at 80/120/160 columns showing 1/2/3 columns; a
  `needs-input` tile draws the red border **and its question**; a truncation
  property test over widths 1..40 (never panics, never exceeds the width);
  an ascii-tier golden under `COCKPIT_GLYPHS=ascii`; a test that asserts the
  preview `Paragraph` is built **without** `Wrap` (ratatui #2679).
- **Gate:** `make tui-test` (`make tui` only builds). TestBackend only — no tmux, no popup, no clock.
- **Size:** ~140 lines, ~120 test. One day. **Day one, against the demo crate,
  before L0 even merges.**

### L3 — push + hook integration (parallel)
- **Inputs:** L0's `push.rs`; `scripts/agent-state`.
- **Outputs:** `tui/src/push.rs`, `scripts/agent-state` (one sender), `AGENTS.md`
  (the readonly rule).
- **Contract:** `push::Push::start() -> Push`, `push.pending() -> bool`; publishes
  `@cockpit_tui_pid` on start and **unpublishes on `q`, SIGHUP and SIGTERM**
  (`display-popup -E` tears the child down by signal). The USR1 handler calls
  `libc::raise(libc::SIGWINCH)` — crossterm retries `poll` on EINTR. Sender side,
  in `agent-state`'s existing batched transition:
  `p=$(tmux show-option -gqv @cockpit_tui_pid); [ -n "$p" ] && [ "$(cat /proc/$p/comm 2>/dev/null)" = cockpit-tui ] && kill -USR1 "$p"`.
- **Fail-first:** (a) a bash block where a **stale pid** belonging to `sleep` receives
  nothing (assert `sleep` is still alive after the transition — the exact bug the
  probe caused on the live server); (b) the pid option is unset after the process
  is SIGTERMed; (c) fork count of one `agent-state` transition is unchanged
  (`show-option` rides the existing batch, per `freeze-precompute-pattern`).
- **Gate:** `make tui`; `make check`; **freeze rig**
  `bash tests/freeze-rig/gate.sh tests/freeze-rig/fixed-example.conf` on the branch
  **and on master**, only a delta counts (`freeze-rig-gate-configs`).
- **Size:** ~40 Rust, ~15 bash, ~70 test. Half a day. **Day one after L0's push.**
- **Never** run any of this against the live server (`AGENTS.md`, `never-experiment-on-live-cockpit-server`).

### L4 — packaging (parallel)
- **Inputs:** ratatui.md §Packaging; nothing from L1–L3.
- **Outputs:** `dist-workspace.toml` (or `[workspace.metadata.dist]`),
  `.github/workflows/release.yml`, `scripts/tui-fetch`, `cockpit.tmux` (the
  `run-shell -b` stale check on `prefix R` only), `.gitignore` (`bin/`),
  a `tui` row in `tmux-review --info`.
- **Contract:** the binary is `$PLUGIN_DIR/bin/cockpit-tui`, **never** `~/.local/bin`
  (a test already asserts `install.sh` manages only its own symlinks).
  `@cockpit_tui_version` pins the tag. `scripts/tui-fetch` = uname → asset → `.sha256`
  → verify → `cargo build` gated on `rustc ≥ 1.88` → `display-message` warning,
  keeping whatever binary already exists. Targets: x86_64-musl, x86_64-darwin,
  aarch64-darwin (aarch64-musl needs hand-wired cross linkers, cargo-dist #1581).
- **Fail-first:** (a) a stubbed `curl`/`uname` on PATH: a checksum mismatch leaves the
  old binary in place and exits non-zero; (b) `rustc 1.85` on PATH produces the
  named "needs Rust ≥ 1.88, install rustup" refusal and no compiler spew;
  (c) `install.sh --dry-run` still lists only its own symlinks, `bin/` untouched;
  (d) `--info` prints `tui  <version>  <path>` and `tui  not installed` when absent.
- **Gate:** `make check`; `dist plan` lists three artifacts each with a checksum;
  `shellcheck` clean (`scripts/*` is globbed into `SOURCES`).
- **Size:** ~120 bash, ~60 config, ~80 test. One day. **Day one.**

### L5 — docs (parallel)
- **Inputs:** ratatui.md, screens.md, the `rules` lane's do/do-not list on #61.
- **Outputs:** `docs/tui.md`, `KEYS.md` (one row), `README.md` (one section),
  `AGENTS.md` (the "never write the live server from the TUI" rule, if L3 has not).
- **Contract:** documents §3's freeze verbatim — option names, env vars, action lines,
  exit codes. Says the binary is optional and that a missing binary is a message,
  not an error.
- **Fail-first:** the shipped-config render block already greps `KEYS.md`-adjacent
  strings; add `grep -q 'COCKPIT_ACTION' docs/tui.md` and a check that every option
  name in `docs/tui.md` appears in `cockpit.tmux` or `tui/src/` (catches doc drift —
  three README lines went stale on phase 3 and cost a review round).
- **Gate:** `make check`.
- **Size:** ~200 prose, ~15 test. Half a day. **Day one.**

### L6 — integration (last, after L1–L5 merge)
- **Inputs:** every merged lane.
- **Outputs:** `cockpit.tmux` (the one binding + `@cockpit_board_key` with `off`,
  copying the `@cockpit_review_key` block at `cockpit.tmux:288`; `@cockpit_board_window`
  mirroring `@cockpit_review_window`; the tmux ≥ 3.7 guard; `2>>` the log),
  `examples/tmux.conf` + `examples/full.tmux.conf` rows, `tests/run.sh` binding block.
- **Contract:** the binding passes `-p '#{pane_id}'` (`display-popup` expands only
  `-T` and `-d`, never its shell-command — `popup-needs-pane-id`), sets
  `COCKPIT_ACTION` to an `mktemp`, runs the binary, then dispatches the action line.
  `review`/`fix` go to the existing `tmux-review` / `tmux-fixer`; `jump %N` is
  `switch-client` + `select-window` + `select-pane` on the named pane, byte-identical
  to `scripts/tmux-jump:73-75` and **not** a call to `tmux-jump`, which takes the pane
  you are on and cycles to the *next* blocked one (`scripts/tmux-jump:61-68`), so it
  would leave the board's chosen target for somewhere else (freeze amendment 2, #61).
  Missing binary → one `display-message` and exit 0.
- **Fail-first:** (a) the shipped-config block asserts the bind exists and that
  `@cockpit_board_key off` removes it; (b) a fake `cockpit-tui` on PATH writing
  `jump %9` makes the wrapper run exactly `switch-client -t %9`, `select-window -t %9`,
  `select-pane -t %9` and nothing else — no `tmux-jump` (argv shim);
  (c) tmux 3.6 (a `-V` shim) produces the named refusal, not a broken popup;
  (d) a pane directory named `$(touch /tmp/pwn)` does not run — the existing menu
  injection block's shape.
- **Gate:** `make check` unpiped (`gate-pipelines-mask-failures`), `make gate` stamped,
  freeze rig vs master, **plus the one manual popup run recorded as a comment on #61**
  (screenshot or `capture-pane -e` excerpt, tmux version, terminal, ticks in ms).
- **Size:** ~60 tmux, ~120 test. One day.
- **Board key:** recommend `prefix w` (the board *is* a better `choose-window`),
  rebindable via `@cockpit_board_key`, `off` keeps stock. Free lowercase keys after
  #43: `a u r i f b g` are taken. **The UX lane owns this call** — L6 only owns the
  binding site.

### L7 — phase 2 inline reply (not now)
One-line input over a `needs-input` tile, `↵` spawns `tmux-review --deliver` via
`run-shell -b`, Esc cancels, needs-input tiles sort first. Its own issue after the
board merges. Contract already reserved: the action line `deliver %N` (§3).

## 2. How the agents coordinate

**Before anything:** `git worktree list` + `ListAgents`. The same `/goal` has landed
in two panes before (`check-peer-sessions-before-claiming-issue`); a worktree named
for a lane plus a busy peer whose cwd is in it means the lane is claimed — message
that peer and take the review pass instead.

- **One issue per lane**, opened before the first edit (`issue-before-fix`), title
  `cockpit-tui L<N>: <what>`, body = that lane's row above verbatim, first line
  `Part of #61.` Findings go on the lane's issue as comments, not in a scratch file.
- **One worktree per lane** under `~/.cache/cockpit-wt/<lane>`
  (`branch-work-in-cockpit-wt-worktrees`). The main checkout
  `~/.tmux/plugins/tmux-cockpit` **stays on master** — several sessions share it and
  a `checkout` there swaps files under a running suite
  (`concurrent-sessions-shared-worktree`). Branches: `feat/tui-skeleton`,
  `feat/tui-data`, `feat/tui-board`, `feat/tui-push`, `feat/tui-dist`,
  `docs/tui`, `feat/tui-binding`.
- **No `git stash`, ever.** All worktrees of this repo share one stash stack and a
  concurrent lane's `pop` has already eaten a lane's work
  (`worktrees-share-git-stash-stack`). Fail-first verification uses
  `git show HEAD:path > "$SCRATCH/copy"`, run, restore from the copy.
- **PRs stack on L0, then get retargeted.** L1–L5 open with `--base feat/tui-skeleton`.
  The moment L0 merges: for each lane `gh pr edit <n> --base master`, then
  `git rebase --onto master feat/tui-skeleton`, **then** delete L0's branch. Deleting
  the base first closes the stacked PRs and they cannot be reopened
  (`fleet-timing-gate-branch`, which cost PR #34).
- **Reporting.** A lane agent sends its full report as **text via SendMessage to the
  lead** — subagents cannot write report files since 2026-08-26
  (`named-subagent-reports-via-files`). No report path in any dispatch. After an idle
  notice with no text: nudge once, then re-dispatch.
- **The shared rules doc** is `docs/tui.md` §Rules (L5 writes it, every lane reads it
  before its first commit): the §3 freeze; never `Wrap` the preview; no terminal
  queries, crossterm pinned `=0.29.0`; no mouse in phase 1; no tokio, no `notify`,
  no clap/anyhow; the demo's `ratatui::init()` + `panic = "abort"` pairing (never a
  Drop guard with abort); actions are printed, never executed; every write to a tmux
  server goes through one function that `COCKPIT_READONLY` can disable; tests on
  private `-L` sockets registered in `SOCKS`; no fixed `sleep` and no wall-clock
  assertion — pin a fork count or poll a condition (`fleet-timing-gate-branch`).
- **`tests/run.sh` is the one shared file.** Every lane **appends** its block at the
  end with a unique `== cockpit-tui: … ==` title. Merges are both-sides-appended;
  after every merge run `bash -n tests/run.sh` before the gate — that seam dropped a
  `fi` on the side-fixer merge.
- **Commits:** lowercase `feat:`/`fix:`/`tests:`/`docs:` one-liners in the repo's
  measured past-tense voice, session trailer, no emoji.
- **Merge order:** L0 → (L1, L2, L3, L4, L5 in any order, each rebased on master and
  gated after the rebase) → L6. L6's gate is the merged tree's gate.

## 3. The interface freeze — agreed before any lane forks

L0 lands all of it on day one; after that only an issue comment on #61 changes it.

**Crate location.** `tui/` at the repo root, package `cockpit-tui`, binary
`cockpit-tui`, `edition = "2024"`. *(The `base`/`repo` lane owns the final say; if it
picks another path, L0 changes one line and everything else is path-free.)*

**`data::Pane`** — the demo's struct, frozen field-for-field:
`id, wher, name, cmd, state, title, msg, sess, path, root, spool, pinned, delivered,
pid, review, hold, doing, said, asked, loc, pm, since: Option<u64>,
asked_since: Option<u64>, idle_since: Option<u64>`. Derives
`Default, Clone, Hash, PartialEq` (`Hash` is why the durations are `u64`, not `Duration`).

**`--json`** — an array of objects with exactly these 14 keys, in this order:
`id, where, state, title, doing, said, asked, msg, since, loc, diff, review, session, path`.
`where` is `sess:idx` (the window name is the `name` field); `title` falls back to pane name then `(no title)`;
`since` is seconds or `null`; `diff` is the `±` string; `state` ∈
`needs-input | running | done | error | ""`. Pretty-printed, one object per agent pane,
never a non-agent pane.

**`--rows`** — tab-separated, 8 fields, no header:
`glyph \t id \t where \t title \t (msg if needs-input else doing) \t since \t loc \t diff`.

**Options** (read/written by the binary):
`@cockpit_tui_pid` (written on start, **unset on q/SIGHUP/SIGTERM**),
`@cockpit_tui_version` (written by `tui-fetch`, read by `tmux-review --info`'s doctor row),
`@cockpit_board_window` (`on` = parked window instead of popup),
`@cockpit_board_key` (rebind / `off`),
`@cockpit_tui_fetch` (default **off**; freeze amendment 2 on #61 — inside
`cockpit.tmux` tpm's `prefix U` and a plain `source-file ~/.tmux.conf` are the same
event, so rule 39 and DO NOT 14 can only both hold behind an opt-in; off means
nothing is ever fetched and `scripts/tui-fetch` run by hand stays the manual path),
`@cockpit_popup_height` (the board's popup height; freeze amendment 2 replaced the
fixed 80 %, only the `-w 90%` width is the board's own number).
Read-only, already owned by bash: `@cockpit_session`, `@cockpit_dir`, `@panestate`,
`@panestate_msg`, `@cockpit_review`, `@cockpit_review_spool`, `@cockpit_title`,
`@cockpit_hold`, `@thm_*`.

**Env vars:** `COCKPIT_ACTION` (file the action line is written to; without it the
line goes to stdout), `COCKPIT_TMUX` (override the tmux argv; unset means follow
`$TMUX` to whatever socket the pane is on — verified), `COCKPIT_LOG` (append target;
the binding sets it to `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/tui.log`),
`COCKPIT_READONLY` (any non-empty value: never write to a tmux server, including the
pid), `COCKPIT_GLYPHS=ascii` (the ambiguous-width fallback tier).
The binding runs the binary under `env -u CLAUDECODE` (required inside a popup).

**Action file protocol.** Exactly one line, `\n`-terminated, written to
`$COCKPIT_ACTION` on exit; nothing else is ever written there:
`jump %N` · `review %N` · `fix %N` · `info %N` · `deliver %N` (reserved for L7).
The verb is `[a-z]+`, the argument is a tmux pane id. The binary **executes nothing**.
`info %N` is the fifth verb, accepted by the owner on #61: ux.md §3.1, its footer and
its `?` overlay all bind `i`, so `i` is either bound and emitted or dropped from all
three; the wrapper maps it onto the existing `prefix i` screen in one `case`.

**CLI:** `cockpit-tui board | fleet | info %N | ask %N | --rows | --json | --version | -h`.
Phase 0 ships `--rows --json --version -h`; phase 1 adds `board`. `fleet`/`ask` stay
in the demo and out of the shipped binary until someone asks.

**Log format:** one line per event, `<unix_micros> <text>`, append-only, only when
`COCKPIT_LOG` is set. No rotation (ponytail: add when a file gets big).

**Exit codes:** `0` normal (action written or none), `2` usage error (unknown
subcommand, bad pane id) — matches the demo's `exit(2)`, `1` runtime failure
(tmux unreachable, terminal not a tty). The binding treats a missing binary and a
non-zero exit the same way: one `display-message`, exit 0, never a broken popup.

## 4. Gates and verification

Every lane, before asking for review:
0. Log to a path with `$$` in it. Two suites writing `cmd > same.log` truncate one inode both hold open and interleave; the tail read "1022 passed, 0 failed, exit=0" with a FAIL 800 lines up. The tell: `grep -c '^== ' log` is 2x the distinct block headers (L1, 2026-08-27).
1. `make check` **unpiped** (`gate-pipelines-mask-failures` — `| tail` reported exit 0
   on a red suite), then `make gate` to stamp. Must be green **with cargo absent from
   PATH**: `env PATH=$(echo "$PATH" | tr ':' '\n' | grep -v cargo | paste -sd:) make check`.
2. `make tui` = `cargo build --release` + copy to `bin/` + `--version`; `make tui-test` = `cargo test && cargo clippy --all-targets -- -D warnings &&
   cargo fmt --check`, both from `tui/`. Run both; `make tui` alone runs no test (L6 caught every lane reading it as build+test).
3. `bash -n tests/run.sh` after any merge that touches it.
4. **Freeze rig, required for L3 and L6, offered by everyone else:**
   `bash tests/freeze-rig/gate.sh tests/freeze-rig/fixed-example.conf` on the branch
   **and on master**; only a delta is a finding. The default no-argument run is the
   rig's own expected FAIL and `examples/full.tmux.conf` bails on master too
   (`freeze-rig-gate-configs`) — do not report either as a regression.
5. **The manual popup run, once, on #61** (L6): tmux version, terminal, geometry,
   tick times, the `q` exit, and that `@cockpit_tui_pid` is gone afterwards. Popup +
   pty + keystroke tests are the flaky kind this repo refuses to automate.
6. **A review pass by a separate agent** before every merge — never self-approve in
   the same context. Either `/code-review` on the PR number, or a fresh
   `oh-my-claudecode:code-reviewer`. The `cockpit-review` popup needs `claude` running
   inside a real tmux pane, so check `pstree` first
   (`worktree-guard-and-review-popup`). Fixes from the review get their own commits.
7. L6 additionally runs the leak sweep (no leftover `-L` sockets vs the baseline) and
   `install.sh --hooks --dry-run`.

## 5. Day-one kickoff messages

### L0 — skeleton
```
You own L0 of #61: the cockpit-tui skeleton. Everything else forks from you, so push within hours.
Open the issue first ("cockpit-tui L0: crate skeleton, CI, make tui, the bash block", body = the L0 row of docs/superpowers/plans/<this plan>, first line "Part of #61."), then
  git worktree add ~/.cache/cockpit-wt/tui-skeleton -b feat/tui-skeleton
Never edit ~/.tmux/plugins/tmux-cockpit; it stays on master and other sessions share it.
Scope: copy /home/tzachi/.cache/cockpit-wt/issue-54-research/docs/superpowers/research/2026-08-26-ask-busy-agent/spike/cockpit-tui-demo/ into tui/ (Cargo.toml.txt -> Cargo.toml), delete ask.rs/info.rs and fleet.rs's interactive run() (keep rows_text/rows_json), keep data.rs board.rs theme.rs push.rs main.rs tests/.
Files you own: tui/**, .github/workflows/tui.yml, Makefile (a `tui:` target), .gitignore (bin/, tui/target/), one tests/run.sh block appended at the end.
Contract you publish and freeze (see §3 of the plan): lib.rs's full module list so no other lane ever edits it; data::Pane's fields; the 14 --json keys in order; --rows' 8 tab fields; @cockpit_tui_pid/@cockpit_tui_version/@cockpit_board_window/@cockpit_board_key/@cockpit_tui_fetch/@cockpit_popup_height; COCKPIT_ACTION/TMUX/LOG/READONLY/GLYPHS; the action lines jump|review|fix|info|deliver %N; exit 0/1/2.
Fail-first: the block `== cockpit-tui: --json names every agent pane and nothing else ==` must print SKIP (skip_, never a pass) with no binary present, then PASS once built.
Gate: `make check` green with cargo removed from PATH; `make tui` (build+test+clippy -D warnings+fmt --check); bash -n tests/run.sh. Run it unpiped.
Do not touch: cockpit.tmux, scripts/*, README, KEYS.md, docs/. No binding in this lane.
No git stash (the stash stack is shared across worktrees) - copy files aside instead.
PR --base master. Report as text via SendMessage to the lead; do not write a report file.
```

### L1 — data layer
```
You own L1 of #61: the cockpit-tui data layer. Fork from feat/tui-skeleton once L0 pushes.
Open the issue first ("cockpit-tui L1: session cache, mtime gate, --json contract", "Part of #61."), then
  git worktree add ~/.cache/cockpit-wt/tui-data -b feat/tui-data feat/tui-skeleton
Scope: tui/src/data.rs ONLY. Cache `claude agents --json` for 60 s (it costs 380 ms and used to run every tick); skip the transcript parse when fs::metadata().modified() is unchanged; route every tmux write through one function that COCKPIT_READONLY disables.
Contract, frozen by L0 (the Option returns are freeze amendment 1 and its addendum on #61), bodies only: list_panes() -> Option<Vec<Pane>>, preview(&str, usize) -> Option<String>, hash_rows(&[Pane], &str) -> u64, one_pane(&str) -> Option<Pane>, tmux(&[&str]) -> Option<String>, log(&str). Do not change a signature, a Pane field, or a --json key - open a comment on #61 instead.
Fail-first, all three before the fix: a PATH-shim `claude` counter shows ONE fork across two list_panes() inside 60 s; a transcript whose mtime did not change is not re-read (poison its second line so a re-read panics); COCKPIT_READONLY=1 leaves the tmux shim's log empty.
Gate: make tui; then `make check` unpiped with the built binary present (L0's block must go green, not skip).
Do not touch: board.rs, theme.rs, push.rs, lib.rs, main.rs, cockpit.tmux, scripts/*.
No wall-clock assertions and no sleeps in any test - pin a fork count or poll a condition.
No git stash. PR --base feat/tui-skeleton; retarget to master with `gh pr edit <n> --base master` the moment L0 merges, BEFORE its branch is deleted.
Report as text via SendMessage to the lead.
```

### L2 — board renderer
```
You own L2 of #61: the board renderer. Start now - you need no tmux and no L0 file.
Open the issue first ("cockpit-tui L2: the board grid, goldens, ascii tier", "Part of #61."), then
  git worktree add ~/.cache/cockpit-wt/tui-board -b feat/tui-board feat/tui-skeleton
(work against the demo crate meanwhile: .../spike/cockpit-tui-demo/)
Scope: tui/src/board.rs, tui/src/theme.rs, tui/tests/screens.rs, tui/tests/golden/*. TestBackend only - your code calls no tmux and no clock.
Contract: render(f, area, &View, &mut TableState) is pure over &[Pane]; run() -> Option<String> is the only impure entry and returns one action line (jump|review|fix|info %N). Pane's fields and the golden helper come from L0 and are frozen.
Deliver: 3/2/1 column profiles, ANSI previews, needs-you border, keys tab/h/l/enter/q, dirty-flag redraw (hash_rows), theme read once at startup, 2-column glyph gutter, COCKPIT_GLYPHS=ascii tier.
Fail-first: goldens at 80/120/160 showing 3/2/1 columns; a needs-input tile draws the red border AND its question; a truncation property test over widths 1..40 that never panics or overruns; an ascii-tier golden; a test asserting the preview Paragraph is built WITHOUT Wrap.
Hard rules: never enable Wrap on the preview (ratatui #2679 panics on double-width); send no terminal queries and keep crossterm pinned =0.29.0 (crossterm #1104 wedges the parser inside a popup); no mouse in phase 1 - SGR capture takes the wheel and text selection away.
Gate: make tui (clippy -D warnings, fmt --check). No bash suite work in this lane.
Do not touch: data.rs, push.rs, lib.rs, cockpit.tmux, scripts/*.
No git stash. PR --base feat/tui-skeleton, retarget to master before L0's branch is deleted.
Report as text via SendMessage to the lead.
```

### L3 — push + hook
```
You own L3 of #61: the push channel and its hook. Fork from feat/tui-skeleton once L0 pushes.
Open the issue first ("cockpit-tui L3: SIGUSR1 push, pid verify, readonly guard", "Part of #61."), then
  git worktree add ~/.cache/cockpit-wt/tui-push -b feat/tui-push feat/tui-skeleton
Scope: tui/src/push.rs, one sender in scripts/agent-state's existing batched transition, one AGENTS.md rule.
Contract: Push::start() publishes @cockpit_tui_pid and unpublishes it on q, SIGHUP AND SIGTERM (display-popup -E tears the child down by signal); pending() is a swap on an AtomicBool; the USR1 handler must raise(SIGWINCH) - crossterm retries poll() on EINTR, so a bare flag waits ~68 ms.
Sender: p=$(tmux show-option -gqv @cockpit_tui_pid); [ -n "$p" ] && [ "$(cat /proc/$p/comm 2>/dev/null)" = cockpit-tui ] && kill -USR1 "$p". The comm check is not optional: a probe already signalled a recycled pid on the live server, and USR1's default action is terminate.
Fail-first: a stale pid owned by `sleep` survives a transition untouched; the pid option is unset after SIGTERM; the fork count of one agent-state transition is unchanged (the show-option rides the existing batch).
Gate: make tui; make check unpiped; freeze rig `bash tests/freeze-rig/gate.sh tests/freeze-rig/fixed-example.conf` on your branch AND on master - only a delta counts. The no-argument run is the rig's own expected FAIL; full.tmux.conf bails on master too. Neither is your regression.
NEVER install a hook, binding or option on the live tmux server. Private -L socket, registered in SOCKS, killed after.
Do not touch: board.rs, theme.rs, data.rs, cockpit.tmux bindings.
No git stash. PR --base feat/tui-skeleton, retarget to master before L0's branch is deleted.
Report as text via SendMessage to the lead.
```

### L4 — packaging
```
You own L4 of #61: packaging and the fetch path. Start now - you depend on nothing.
Open the issue first ("cockpit-tui L4: cargo-dist, tui-fetch, the --info row", "Part of #61."), then
  git worktree add ~/.cache/cockpit-wt/tui-dist -b feat/tui-dist feat/tui-skeleton
Scope: cargo-dist config, .github/workflows/release.yml, scripts/tui-fetch, one run-shell -b stale check in cockpit.tmux behind prefix R only, .gitignore (bin/), and a `tui` row in tmux-review --info (the info_mode printf block).
Contract: the binary lives at $PLUGIN_DIR/bin/cockpit-tui, never ~/.local/bin (a test asserts install.sh manages only its own symlinks). @cockpit_tui_version pins the tag. Fetch order: uname -> asset -> .sha256 -> verify -> cargo build gated on rustc >= 1.88 -> display-message warning; keep whatever binary already exists on any failure. Three targets: x86_64-musl, x86_64-darwin, aarch64-darwin (aarch64-musl needs hand-wired cross linkers, cargo-dist #1581).
Fail-first: stub curl/uname on PATH - a checksum mismatch leaves the old binary and exits non-zero; rustc 1.85 yields the named "needs Rust >= 1.88, install rustup" refusal with no compiler spew; install.sh --dry-run still lists only its own symlinks; --info prints the tui row, and "not installed" when absent.
Gate: make check unpiped; `dist plan` lists three artifacts each with a checksum; shellcheck clean (scripts/* is globbed into SOURCES automatically - no Makefile edit needed).
Do not touch: tui/src/**, the board binding (L6 owns cockpit.tmux's binding site).
Never fetch or write anything on the live tmux server; test the stale check on a private -L socket.
No git stash. PR --base feat/tui-skeleton, retarget to master before L0's branch is deleted.
Report as text via SendMessage to the lead.
```

### L5 — docs
```
You own L5 of #61: the docs and the shared rules page. Start now.
Open the issue first ("cockpit-tui L5: docs/tui.md, KEYS.md, README", "Part of #61."), then
  git worktree add ~/.cache/cockpit-wt/tui-docs -b docs/tui feat/tui-skeleton
Scope: docs/tui.md (new), one KEYS.md row, one README section, and the AGENTS.md rule if L3 has not landed it. Prose only - no code, no bindings.
docs/tui.md must carry the §3 freeze VERBATIM, because every lane reads it before its first commit: data::Pane's fields, the 14 --json keys in order, --rows' 8 fields, the option names (including @cockpit_tui_fetch and @cockpit_popup_height), the COCKPIT_* env vars, the action lines jump|review|fix|info|deliver %N, the log format `<unix_micros> <text>`, exit codes 0/1/2. Plus the rules: never Wrap the preview, no terminal queries and crossterm pinned =0.29.0, no mouse in phase 1, no tokio/notify/clap/anyhow, ratatui::init() + panic="abort" (never a Drop guard with abort), actions are printed and never executed, every tmux write behind the COCKPIT_READONLY guard.
Say plainly: the binary is optional; a missing binary is one message and exit 0, and `prefix r` remains the list.
Fail-first: a tests/run.sh assertion that every @cockpit_* option name in docs/tui.md also appears in cockpit.tmux or tui/src/ (three README lines went stale on phase 3 and cost a review round), plus grep -q COCKPIT_ACTION docs/tui.md.
Voice: measured, past tense where something was measured, no marketing, no emoji, tables over paragraphs. Numbers only from the research doc, none invented.
Gate: make check unpiped.
Do not touch: tui/src/**, cockpit.tmux, scripts/*.
No git stash. PR --base feat/tui-skeleton, retarget to master before L0's branch is deleted.
Report as text via SendMessage to the lead.
```

### L6 — integration
```
You own L6 of #61: the binding and the integration gate. Start only after L1-L5 are merged to master.
Open the issue first ("cockpit-tui L6: the board binding, popup/window, tmux >= 3.7 gate", "Part of #61."), then
  git worktree add ~/.cache/cockpit-wt/tui-binding -b feat/tui-binding master
Scope: cockpit.tmux (one binding + @cockpit_board_key with `off`, copying the @cockpit_review_key block at :288; @cockpit_board_window mirroring @cockpit_review_window; a tmux >= 3.7 guard; 2>> the log), examples/tmux.conf and examples/full.tmux.conf rows, one tests/run.sh block.
Contract: the binding passes -p '#{pane_id}' - display-popup expands only -T and -d, never its shell-command, so $TMUX_PANE inside a popup is wrong. It sets COCKPIT_ACTION to an mktemp, runs the binary under env -u CLAUDECODE, then dispatches the one action line: review/fix to the existing tmux-review / tmux-fixer, and jump %N as switch-client + select-window + select-pane on that pane (byte-identical to scripts/tmux-jump:73-75). Do NOT call tmux-jump: it cycles from the pane you are on to the next blocked one (scripts/tmux-jump:61-68) and would ignore the board's chosen target (freeze amendment 2 on #61). Missing binary or non-zero exit: one display-message, exit 0.
Key: recommend `prefix w` (the board is a better choose-window); a u r i f b g are taken. Confirm with the UX lane on #61 before binding.
Fail-first: the shipped-config block asserts the bind exists and that @cockpit_board_key off removes it; a fake cockpit-tui writing "jump %9" makes the wrapper run exactly switch-client -t %9, select-window -t %9, select-pane -t %9 and nothing else - no tmux-jump (argv shim); a tmux -V shim reporting 3.6 produces the named refusal; a pane directory named `$(touch /tmp/pwn)` does not execute (copy the menu-injection block's shape).
Gate: make check unpiped, make gate stamped, freeze rig fixed-example.conf on branch AND master (delta only), leak sweep vs the socket baseline, install.sh --hooks --dry-run. Then the ONE manual popup run recorded as a comment on #61: tmux version, terminal, geometry, tick ms, the q exit, and @cockpit_tui_pid gone afterwards.
Then request a review from a SEPARATE agent (/code-review on the PR, or a fresh code-reviewer) - never self-approve.
No git stash. PR --base master.
Report as text via SendMessage to the lead.
```
