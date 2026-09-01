# cockpit-tui: base and folder structure

Read-only research. Nothing edited in any repo.

## 0. The one fact that settles it

**Our demo is already the board. None of the candidates have one.**

`spike/cockpit-tui-demo/src/board.rs:25-97` renders an N-tile grid, one bordered
tile per agent pane, with a **live ANSI preview inside every tile**
(`board.rs:94`, `ansi_to_tui::IntoText`), border colour by `@panestate`
(`board.rs:53-60`), a needs-you strap (`board.rs:67-72`), and 1/2/3 columns by
width (`board.rs:15-23`). Measured 12–20 ms per tick for 3 tiles at 160×40
(`scratchpad/board.log`).

- **workmux**: dashboard is a `Table` + **one** preview for the selected row
  (`src/command/dashboard/ui/dashboard.rs:299`, `:494`, `:585`); its sidebar
  "Tiles" mode is a vertical `List` of text cards in a 25-col column
  (`src/command/sidebar/ui.rs:807-808`, `mod.rs:45-47`) with no preview. No 2D
  grid, no N simultaneous previews anywhere.
- **tmux-agent-sidebar**: `capture-pane` never touches the TUI render path at
  all — its ANSI parser (`src/cli/capture/ansi.rs`) renders to **HTML** for
  website screenshots (`src/cli/capture/render_html.rs`). The sidebar shows
  metadata rows only.
- **recon**: 4,807 LOC session picker, list + preview, no grid.

The expensive, uncertain part of phase 1 — a grid of live previews inside a
render budget — is the part every candidate is missing and our demo has done.

---

## 1. Candidate-by-candidate

### our demo — `spike/cockpit-tui-demo/`

| | |
|---|---|
| licence | ours, MIT-compatible by construction |
| size | 1,982 LOC (`src` 1,770 + `tests/screens.rs` 212, `wc -l` on master), 7 goldens |
| deps | 4: `ratatui 0.30.2`, `ansi-to-tui 8.0.1`, `serde_json`, `libc` (`Cargo.toml.txt:7-11`) |
| binary | **803 KB** glibc (803,776 B), **910 KB** musl — both already built; decimal kB, so the same two binaries rules.md 5 reports as 785 KiB / 889 KiB |

Has, that cockpit needs:
- board/tiles + per-tile ANSI preview — `board.rs:25-97`
- **cockpit's own data model, natively**: `data.rs:364` reads
  `@panestate`, `@cockpit_title`, `@panestate_msg`, `@cockpit_session`,
  `@cockpit_review`, `@cockpit_hold`, `@cockpit_review_spool` in **one**
  `list-panes -a -F` (`data.rs:367`)
- review store counts straight off the spool dir (`data.rs:284-299`)
- theme from the server's `@thm_*` with a truecolor/256 decision taken from
  `#{client_termfeatures}` rather than a terminal query that hangs inside a
  popup (`theme.rs:1-3, 46-64`) — 2 tmux calls, once per process
- glyph tiers incl. ascii (`theme.rs:104-113`), matching `screens.md §0`
- push wakeups: `SIGUSR1` + a `raise(SIGWINCH)` trick because crossterm retries
  its poll on EINTR — **measured 68 ms vs 25 ms** (`push.rs:12-18`); pid
  published as `@cockpit_tui_pid` and unset on Drop (`push.rs:25, 38`)
- the action contract: the binary never mutates tmux, it prints an action for a
  shell wrapper (`main.rs:1-3, 38-47`; `wrap.sh:8-18`)
- `--rows` / `--json` headless modes (`main.rs:16-23`) — free bash-suite hooks
- golden `TestBackend` snapshots, 12 tests, `COCKPIT_BLESS=1` to re-bless
  (`tests/screens.rs:12-29`); fixtures are literals, no tmux, no clock
- private-server discipline already wired: `demo.sh` uses `tmux -L demo`,
  `capture.sh` a third `-L capture` server — matches `AGENTS.md`'s rule
- `COCKPIT_TMUX` indirection (`data.rs:12-18`) — this is what lets the bash
  suite's PATH shim work unchanged (see §3)

Missing / to write: per-tile capture budget for large fleets, resize handling in
`board::run`, board goldens (only fleet/info/ask are blessed), `.tmux` fetch,
release workflow, docs.

Conflicts with cockpit's model: **none.** It was written against it.

Kept vs written for phase 1: **keep ~894** (`board.rs` 160, `data.rs` 512,
`theme.rs` 127, `push.rs` 40, `main.rs` 48, `lib.rs` 7), plus `fleet/info/ask`
(862) carried free for phase 2. **Write ~400–600** (budget, resize, board
goldens, `.tmux`, release, docs).

### tmux-agent-sidebar (hiroppy)

MIT (`LICENSE`, © 2026 hiroppy). 32,070 LOC `src` + 7,849 tests. ratatui 0.30.2,
crossterm 0.29, `vte`, `insta` — all pinned with `=` (`Cargo.toml:14-28`).

Genuinely valuable, and mostly *plumbing*:
- ~~`#{q:}` quoting + the matching backslash unescaper —
  `src/tmux/query.rs:102-104` and `:526-558`~~ **rejected by measurement.** The
  problem is real: our demo splits on a raw `\t` (`data.rs:376`), and a title or
  prompt containing a tab **silently shifts every field**. The fix is not this one.
  verified.md tmux B/C measured that tmux never octal-escapes format output (so the
  unescaper decodes an escape that was never applied, and corrupts any value holding
  a backslash) and that `#{q:}` passes every control character raw while escaping
  space — the two characters that threaten cockpit survive it and spaces get mangled.
  Decision 3 settled on a raw `\037` separator plus in-format
  `#{s/<NL>/ /:#{s/<TAB>/ /:#{s/<US>/ /:…}}}` on free-form fields, no unescaper
  (rules 15/16, DO NOT 11).
- `src/process.rs:20-99` — one `ps -eo pid=,ppid=,comm=,args=`, child map, BFS.
  Ours already does the pid→session BFS (`data.rs:229-256`) but without
  comm/args, so it cannot tell a dead agent from a live one.
- `hook.sh` (43 lines) — late-binding wrapper, 5-way binary probe then `exec`,
  `exit 0` when absent so the agent session never sees a hook failure.
- `install-wizard.sh:64-68` — macOS `com.apple.quarantine`/`provenance` xattr
  strip + `codesign --force --sign -`, or Sequoia SIGKILLs the downloaded binary.
- `fixtures/scenarios/common/_lib.sh:45-78` — unset `TMUX` **then** export
  `TMUX_TMPDIR`; `ln -sf /bin/sleep .../claude` to satisfy a `ps` liveness sweep.
- `options.rs:183-245` — thread-local mock of pane get/set/unset. This is how
  1,024 unit tests run with no tmux server.

Conflicts: **`@pane_*` is its hook's private namespace** — 24 keys
(`options.rs:14-81`) written only by its own `hook` subcommand and swept by
`clear_agent_pane_state` (`query.rs:361-382`). Adopting its read side means
adopting its hook binary and `hooks.json`; it cannot coexist with cockpit's
`@panestate`/`@cockpit_*` writers. Port the *shape*, never the keys.

Risks: **lock-step field constants** — `query.rs:19-32` (`MIN_FIELDS = 28`,
`PANE_LINE_OFFSET = 6`) and `:38-63` (22 hand-numbered indices with
`// absolute N` comments) must track `pane_format()` at `:68-100`; the comment at
`:19-21` says so outright. Copy the format without the index modules and you get
a silent misparse, not a compile error. **A 16 ms poll floor** at `src/app.rs:63`
(`.min(Duration::from_millis(16))`) wakes the process ~62×/s forever even fully
idle, with nothing else needing it (spinner 200 ms, refresh 1 s) — I could not
confirm an issue #85 from this checkout, whose history is one squashed commit
`e2474e4`; the line itself is the evidence. **`state.rs` is a 1,724-line god
object** with pet-animation counters interleaved among real fields
(`:42-102`) — copy nothing from `src/state/`. Its release pipeline downloads
with **no checksum at all** (`install-wizard.sh:78-87`).

Fork: **no.** ~14k LOC to delete (pet 1,447; git+PR 1,580; worktree ~900;
notices 1,789; setup 1,414; desktop notify 555; HTML capture 354; row-shaped UI
~1,800) and the deletion is not clean. Copy ~800 lines.

### workmux (raine)

MIT (© 2025 workmux contributors). **84,619 LOC** across 170 files; tests are
18,798 lines of **Python**, not Rust. 41 direct deps, 351 in the lockfile
(`Cargo.toml:26-70`).

Its product is a worktree/multiplexer orchestrator with ~50 subcommands
(`src/cli.rs`) — sandbox 10,072, workflow 6,995, config 6,223 (`.workmux.yaml`),
agent installers 3,647 (`src/agent_setup/{claude,codex,copilot,opencode,antigravity,gemini,grok}.rs`),
git 3,248 + github 1,543, plus zellij/kitty/wezterm backends 3,238.

Data model conflict is direct: state is **JSON files on disk**, one per pane —
`~/.local/state/workmux/agents/{backend}__{instance}__{pane_id}.json`
(`src/state/store.rs:13,72,86`; `src/state/types.rs:40-44`), atomic tmp+rename
(`src/state/mod.rs:25-30`). tmux options are only a **status-line render
channel** (`src/multiplexer/tmux.rs:1109-1140`). Cockpit's model is the
inverse — the pane option *is* the state. Adopting workmux's store would mean
running its `workmux set-window-status` hook (`src/command/set_window_status.rs:15-25`)
as our writer.

Extra fork liability: `[patch.crates-io] crossterm = { git = ".../raine/crossterm" }`
(`Cargo.toml:78-79`).

No `.tmux` file, no TPM support at all; `scripts/install.sh:4` curls a release
tarball. Release is hand-rolled, not cargo-dist (`.github/workflows/release.yml`),
and its frame tests are two `TestBackend` `assert!(contains)` calls
(`ui/dashboard.rs:830-847`), no snapshots.

Fork: **no.** Keep ~3,000 / delete ~72,000 / rewrite ~9,000. The deletion pass
alone is larger than writing the thing.

Worth copying (~1,600 LOC, MIT, dependency-light):

| path | LOC | why |
|---|---|---|
| `src/command/dashboard/ansi.rs` | 43 | `parse_ansi_to_lines` with a raw-line fallback; ours has no fallback (`board.rs:94` swallows to `unwrap_or_default`) |
| `src/tmux_style.rs` | 274 | parses tmux `#[fg=red,bold]` → ratatui `Style`; cockpit already speaks this dialect |
| `src/state/types.rs:90-117` | ~30 | **as a schema reference**: `pane_pid` + `boot_id` guard against pane-id recycling and server restart |
| `src/cmd.rs` | 161 | the `Cmd` builder |

Its event loop shape is worth retyping, not extracting: one `mpsc` channel
(`dashboard/mod.rs:148`), a blocking `event::read()` thread (`:185-191`), draw
then `recv_timeout` on the nearest deadline then drain to coalesce (`:204-230`).

### recon (gavra)

MIT. 4,807 LOC, ratatui **0.29** + crossterm 0.28 event-stream + **tokio**
(`Cargo.toml:7-9`) + clap + chrono + toml + dirs — 4 deps we do not want in a
popup binary. A Claude-session picker: list + detail, no grid. Its whole tmux
integration is `src/tmux.rs`, 250 lines. Its `tmux.conf` is 5 `display-popup -E`
binds. Nothing here we do not already have, and its async runtime is a
regression for a process that lives for the length of a keypress.

### ratatui event-driven template

MIT. **272 LOC** total (`event-driven/template/src/`: `app.rs` 95, `event.rs` 125,
`main.rs` 13, `ui.rs` 39), deps `ratatui 0.30.2` + `crossterm 0.29` +
`color-eyre`, release profile already `lto/opt-level="s"/strip`
(`event-driven/template/Cargo.toml:14-19`). It is a good skeleton and our demo
already **is** that skeleton plus 1,700 lines of cockpit. Starting here means
deleting the demo and retyping it. The only thing to take is the `event.rs`
crossterm-thread + channel shape, if the board's poll loop
(`board.rs:131`, a flat 100 ms `event::poll`) ever needs to become event-driven.

---

## 2. Recommendation

**Promote the demo. Vendor ~250 lines of hardening from tmux-agent-sidebar and
workmux. Fork nothing.**

Reasons, in order:

1. It is the only candidate that has the board (§0), and the board is phase 1.
2. It is the only candidate whose data model *is* cockpit's — `@panestate`,
   `@cockpit_*`, the review spool, `~/.claude/projects/<slug>/<sess>.jsonl`
   (`data.rs:87-91`). Every other base would need that layer written anyway, on
   top of unpicking theirs.
3. 4 deps and an 803 KB binary against workmux's 351 crates and
   tmux-agent-sidebar's 40k lines. A key-press-lifetime popup should be small.
4. The contract that keeps the bash suite the source of truth — binary decides,
   shell acts (`main.rs:1-3`) — is already implemented and tested.

Exact files to copy in:

| from | file:line | LOC | for |
|---|---|---|---|
| ~~tmux-agent-sidebar~~ | ~~`src/tmux/query.rs:102-104` + `:526-558`~~ | ~~~40~~ | **do not copy** — `#{q:}` + unescaper, refuted by verified.md tmux B/C; `data.rs:376`'s tab-split is still the bug, fixed instead by decision 3's `\037` + in-format `#{s/…}` |
| tmux-agent-sidebar | `src/process.rs:20-99` | ~80 | `ps` walk with comm/args, for liveness |
| tmux-agent-sidebar | `hook.sh` | 43 | late-binding wrapper, s/tmux-agent-sidebar/cockpit-tui/ |
| tmux-agent-sidebar | `install-wizard.sh:64-68` | ~5 | macOS xattr strip + ad-hoc codesign |
| tmux-agent-sidebar | `fixtures/scenarios/common/_lib.sh:45-78` | ~35 | `TMUX_TMPDIR` isolation + `ln -sf /bin/sleep claude` |
| tmux-agent-sidebar | `src/tmux/options.rs:183-245` | ~60 | thread-local option mock, for unit tests without a server |
| workmux | `src/command/dashboard/ansi.rs:1-43` | 43 | ANSI parse **with raw-line fallback** |
| workmux | `src/state/types.rs:90-117` | ref only | `pane_pid`+`boot_id` recycle/restart guard |
| tmux-cmdline | `tmux-cmdline.tmux:24-97` | ~55 | target detection, checksum-verified fetch, cargo fallback, failure memo — the model for §4 |

**Do not** copy tmux-agent-sidebar's `query.rs:68-100` `pane_format()` (drags the
lock-step index modules), anything from its `src/state/`, or its `@pane_*` keys.
**Do not** copy tmux-popup-control's `.tmux` — it has no fetch at all, it runs
`go mod vendor; make build` on first load (`main.tmux:8-15`). Its only useful
idea is generating bindings into a temp file and `source-file`-ing it once
(`main.tmux:78-99`), which cockpit already does better with `sq()`
(`cockpit.tmux:36`) and heredoc-free `printf` blocks (`cockpit.tmux:344, 396`).

---

## 3. Folder structure — **in-repo, `tui/`**

```
tmux-cockpit/
  tui/                       # the crate
    Cargo.toml
    Cargo.lock               # committed: a plugin build must be reproducible
    src/{main,lib,board,data,theme,push,fleet,info,ask}.rs
    tests/screens.rs
    tests/golden/*.txt
  bin/                       # gitignored: fetched or built binary + .tried marker
  scripts/cockpit-board      # the wrapper (demo's wrap.sh, promoted)
  cockpit.tmux               # + the ~37-line block in §4
  Makefile                   # + `tui`, `tui-test`; `check` unchanged
  tests/run.sh               # + `== cockpit-tui ==` block, skipped if no binary
  docs/tui.md
  .github/workflows/release-tui.yml
```

`.gitignore` gains one line: `bin/`.

`Makefile` — `check` stays `lint test` (`Makefile:51`), cargo-free:

```make
tui:                     # build the binary into bin/
	@cargo build --release --manifest-path tui/Cargo.toml
	@mkdir -p bin && cp tui/target/release/cockpit-tui bin/

tui-test:
	@cargo test --manifest-path tui/Cargo.toml
```

`SOURCES` (`Makefile:20`) gains `scripts/cockpit-board`; it is already a
`$(wildcard scripts/*)` so it is picked up automatically — which is the point of
that line's comment.

`tests/run.sh` block, in the repo's existing `echo "== ... =="` style
(`run.sh:143` onward):

```sh
echo "== cockpit-tui =="
if [ -x "$DIR/bin/cockpit-tui" ]; then
    # --rows is headless and reads $COCKPIT_TMUX -> the PATH shim's private socket
    got=$("$DIR/bin/cockpit-tui" --rows | wc -l)
    ...
else
    skipped=$((skipped+1)); echo "  SKIP cockpit-tui (no binary)"
fi
```

**The PATH shim already works, for free.** `run.sh:38-44` puts a fake `tmux` on
`$PATH` that redirects to the suite's private `-L` socket; `data.rs:12-22` runs
`Command::new("tmux")`, which resolves through `$PATH`. No `COCKPIT_TMUX` needs
setting and no call site needs rewriting — the same reason the shim was chosen
for the shell scripts (`run.sh:31-34`).

### In-repo vs a separate `tmux-cockpit-tui` repo

**In-repo.** Trade-offs, honestly:

| | `tui/` in-repo | separate repo |
|---|---|---|
| release cadence | tied to the plugin's tag — *correct*, because the binary reads `@panestate`/`@cockpit_*` and the review spool layout, which the plugin owns. A version skew here is a silent misparse. | independent, and now you own a compatibility matrix |
| tpm clone size | `tui/src` ≈ 60 KB of text. `Cargo.lock` ~30 KB. Goldens 4 KB. **Under 100 KB** against a 65 KB README already in the repo. Non-issue. | smaller clone, but the `.tmux` must then fetch source *or* binary from elsewhere |
| CI cost | cargo jobs only on `tui/**` paths; `make check` untouched | two CI configs, two release flows |
| bash suite | one `run.sh` block, shim works unchanged | the suite would have to fetch a binary to test against, or skip always |
| the demo | promoted with `git mv`-shaped history | copied across a repo boundary, provenance lost |

The clone-size argument is the only one favouring a split, and it is ~100 KB.
The version-skew argument against a split is a correctness argument.

---

## 4. `.tmux` integration sketch

Append to `cockpit.tmux`, after the existing `version_at_least` helper
(`cockpit.tmux:174-177`) and `sq` (`:36`).

**On the version gate: no new floor is needed.** The board is
`display-popup -E -T`, which is the plugin's *existing* 3.3 floor
(`cockpit.tmux:168, 212-213`). The 3.7 gate in the repo is for
`MouseDown1Control1` / `#[range=control|N]` (`cockpit.tmux:224-237`) and does
not apply. Asking for 3.7 here would exclude working installs for nothing.

```sh
# ---- cockpit-tui: the board. Absent binary => no key, everything else works.
TUI_VER=$(tmux show-option -gqv @cockpit_tui_version); TUI_VER=${TUI_VER:-v0.1.0}
TUI="$DIR/bin/cockpit-tui"

tui_target() {
    case "$(uname -s):$(uname -m)" in
        Darwin:arm64|Darwin:aarch64) echo aarch64-apple-darwin ;;
        Darwin:x86_64)               echo x86_64-apple-darwin ;;
        Linux:aarch64|Linux:arm64)   echo aarch64-unknown-linux-musl ;;
        Linux:x86_64|Linux:amd64)    echo x86_64-unknown-linux-musl ;;
        *) return 1 ;;
    esac
}

tui_fetch() {
    local t a u tmp
    t=$(tui_target) || return 1
    a="cockpit-tui-$t.tar.gz"
    u="https://github.com/TzachiSh/tmux-cockpit/releases/download/$TUI_VER/$a"
    command -v curl >/dev/null 2>&1 || return 1
    tmp=$(mktemp -d) || return 1
    trap 'rm -rf "$tmp"' RETURN
    curl -fsSL --max-time 30 "$u"        -o "$tmp/$a"        || return 1
    curl -fsSL --max-time 30 "$u.sha256" -o "$tmp/$a.sha256" || return 1
    ( cd "$tmp" && sha256sum -c "$a.sha256" ) >/dev/null 2>&1 || return 1
    mkdir -p "$DIR/bin" && tar -xzf "$tmp/$a" -C "$DIR/bin"
}

# One attempt per version, remembered: a dead network must not re-download on
# every `source-file ~/.tmux.conf`.
if [ ! -x "$TUI" ] && [ "$(cat "$DIR/bin/.tried" 2>/dev/null)" != "$TUI_VER" ]; then
    mkdir -p "$DIR/bin"; printf '%s\n' "$TUI_VER" > "$DIR/bin/.tried"
    tui_fetch || { command -v cargo >/dev/null 2>&1 && [ -d "$DIR/tui" ] &&
        cargo build --release --manifest-path "$DIR/tui/Cargo.toml" &&
        cp "$DIR/tui/target/release/cockpit-tui" "$TUI"; } || true
fi

bk=$(tmux show-option -gqv @cockpit_board_key); bk=${bk:-U}
if [ -x "$TUI" ] && [ -n "$bk" ]; then
    tmux bind-key "$bk" display-popup -E -w "$PW" -h "$PH" -T ' board ' \
        "$(sq "$DIR/scripts/cockpit-board") -p $SRC_PANE"
fi
# No binary and no key is the degraded state, not an error: the status line's
# #{agent_fleet} and prefix+u still answer "who needs me".
```

37 lines. Notes:

- `@cockpit_tui_version` defaults **in `cockpit.tmux` itself** (line 1 above), the
  same shape as every other `@cockpit_*` key default (`cockpit.tmux:255, 261,
  288, 294, 302`). Not in `examples/tmux.conf`, so an existing user's conf keeps
  working; `examples/full.tmux.conf` documents it.
- `-p $SRC_PANE` is mandatory, not decorative: `display-popup` expands only `-T`
  and `-d`, never its shell-command, so the pane id must be passed and resolved
  by the wrapper (issue #38 / PR #40).
- `mktemp -d` + `trap ... RETURN` scopes the cleanup to the function, unlike
  tmux-cmdline's `trap ... EXIT` (`tmux-cmdline.tmux:65`), which would fire at
  the end of the whole plugin load.
- musl, not gnu, for Linux: the 910 KB musl binary is already proven to build
  (`target/x86_64-unknown-linux-musl/release/cockpit-tui`) and a plugin lands on
  whatever glibc the machine has.
- macOS needs `install-wizard.sh:64-68`'s xattr strip + `codesign --force --sign -`
  added to `tui_fetch` before this ships to a Mac.
- `make check` stays cargo-free: nothing above runs at `make` time, and
  `check: lint test` (`Makefile:51`) is untouched.

---

## 5. Risks, with evidence

**Our demo (recommended path)**
- `data.rs:376` splits the `list-panes` line on a raw `\t` while `FIELDS`
  (`data.rs:364`) uses unquoted `#{...}`. A window name or a `@panestate_msg`
  containing a tab shifts every subsequent field. `data.rs:377` guards on
  `f.len() < 13`, which catches too-few but not mis-aligned. Fix it before phase 1
  ships — but **not** with tmux-agent-sidebar's `#{q:}` + `split_tmux_fields`
  (`query.rs:102-104, 526-558`), which verified.md tmux B/C refuted: use decision 3's
  raw `\037` separator with in-format `#{s/…}` sanitising of the free-form fields.
- **Per-tick cost scales with tiles.** `board.rs:118` calls `preview()` for
  *every* pane every second, each an exec of `capture-pane`
  (`data.rs:427`); `data.rs:403` runs `read_transcript` per pane, which reads
  64 KB of JSONL and parses it. 12–20 ms at 3 tiles (`board.log`) says nothing
  about 30. `git_facts` is memoised per path (`data.rs:404`) and the agents map
  has a 60 s TTL (`data.rs:190-213`) — previews and transcripts are not. Pin the
  exec count in a gate before touching budgets, as the fleet gate does.
- `board::run` never re-reads terminal size on resize; it samples
  `term.size()` inside the 1 s tick (`board.rs:110`), so a resize shows stale
  geometry for up to a second. `fleet` treats `Event::Resize` as a refetch
  (`push.rs:3-5`); board does not.
- `board.rs:94` `unwrap_or_default()` turns an ANSI parse failure into a blank
  tile with no signal. workmux's `ansi.rs` falls back to raw lines.
- No board goldens — 12 tests cover fleet/info/ask only (`tests/screens.rs`).
- `push.rs:17` `raise(SIGWINCH)` inside a signal handler: it buys 43 ms
  (`push.rs:15`) and is the documented reason, but it is a non-obvious
  async-signal path worth a comment surviving any refactor.

**tmux-agent-sidebar as a base**
- `src/app.rs:63` — the 16 ms poll floor, ~62 wakeups/s idle.
- `query.rs:19-63` vs `:68-100` — lock-step field constants; desync is a silent
  misparse, not a compile error.
- `options.rs:14-81` — 24 `@pane_*` keys owned by its hook; cannot coexist with
  cockpit's `@panestate`/`@cockpit_*` writers.
- `state.rs:42-102` — pet counters interleaved with real state in a 1,724-line
  struct; nothing in `src/state/` extracts cleanly.
- `install-wizard.sh:78-87` — downloads with no checksum and no signature.
- `capture-pane` never reaches the TUI path (`src/cli/capture/`) — the feature we
  need is the one it lacks.

**workmux as a base**
- File-based state store (`src/state/store.rs:13`) is the inverse of cockpit's
  pane-option model; adopting it means adopting `workmux set-window-status`
  (`src/command/set_window_status.rs:15-25`) as our hook writer.
- `Cargo.toml:78-79` — a git fork of crossterm.
- 84,619 LOC of which ~72,000 is product to delete; `src/config.rs` alone is
  6,223 lines of YAML surface.
- Tests are Python (`tests/conftest.py:322-334`); a Rust crate in a bash repo
  does not want a third language.
- `dashboard/app/preview.rs:7,16-30` — 200 lines captured for **one** pane every
  500 ms. Nothing there tells us what N tiles cost.

**recon** — tokio + clap + chrono + toml for a popup-lifetime process; ratatui
0.29 (`Cargo.toml:7`) is a version behind everything else here.

**Separate repo** — the only real risk is version skew: the binary parses
`@panestate`, `@cockpit_review_spool` and the review spool's `.pin`/`.sent`
convention (`data.rs:284-299`), all owned by the plugin. Split them and a
mismatched pair fails silently rather than loudly.

**`.tmux` fetch, any variant** — an unreachable network on plugin load must not
block tmux startup: the `.tried` marker above bounds it to one attempt per
version, and `curl --max-time 30` bounds that attempt. Without both, every
`source-file ~/.tmux.conf` pays a full timeout.

---

## 6. Verification of the §4 sketch

Run on throwaway `-L tuiprobe$$` servers, killed after; the live server was
never touched (`AGENTS.md`).

- `bash -n`: OK. `shellcheck -s bash`: clean (with the repo's real `sq()` from
  `cockpit.tmux:36`).
- **degrade** — no binary, release 404s: `curl: (22) ... 404`, plugin load
  continues, `prefix U` bound to a popup: **0**.
- **no-retry** — second load with the `.tried` marker present (`v0.1.0`):
  curl attempts **0**.
- **bind** — binary present: `prefix U` bound to a popup: **1**, expanding to
  `display-popup -E -T " board " -h "80%" -w "80%" "'<DIR>/scripts/cockpit-board' -p #{pane_id}"`
  — `sq()` quoting intact, `#{pane_id}` passed through unexpanded for the
  wrapper to resolve (#38).

- **extract-and-install** — **not verified here, and measured wrong by L4** (freeze
  amendment on #61, `dist` 0.32.0 against a stand-in crate). The sketch extracts the
  tarball straight into `bin/`; a cargo-dist archive is not flat.
  `cockpit-tui-<target>.tar.gz` unpacks to a `cockpit-tui-<target>/` directory holding
  the binary beside `LICENSE` and `README.md`, so the sketch would have installed a
  directory where the binary belongs. `scripts/tui-fetch` looks the binary up inside
  the archive instead.

Not yet verified: macOS target detection and the xattr/codesign step (no Mac
here), and the `cargo build` fallback branch.

The `.tried` marker above is gone: review removed it before the first release. It
bounded the retry, but it also locked a user out of the first release that fixed
whatever had failed, and it silenced the checksum-tamper alarm on every load after
the first.

---

# ADDENDUM — awesome-ratatui (2026-08-27)

Source: https://raw.githubusercontent.com/ratatui/awesome-ratatui/main/README.md (563 lines).
All six named apps cloned shallowly into scratchpad/src/. Verdict unchanged: keep the demo.

## A. The six agent/tmux dashboards

| app | stars | last push | licence | LOC (hand-written) | ratatui/crossterm | tiled board? |
|---|---|---|---|---|---|---|
| claudectl | 197 | 2026-07-10 | MIT | 80,515 (~60,200 prod) | 0.29 / 0.28 | no — 55/45 table+detail |
| thurbox | 48 | 2026-08-26 | MIT | 112,601 (~66,691 prod + 7,683 Lua) | 0.30 / 0.29 | no — list + 1 preview |
| bosun | 43 | 2026-08-24 | MIT | 27,658 src + 1,273 tests | 0.30 / 0.29 | no — list + 1 preview |
| crmux | 27 | 2026-08-15 | MIT | 9,343 (4,091 prod) | 0.30 / 0.28 | **YES** — real grid, N live |
| agent-console | 16 | 2026-08-19 | Apache-2.0 + MIT | 19,967 (~13,236 prod) | 0.30 / 0.29 | no — text cards + 1 PTY |
| iris-tui | 6 | 2026-07-08 | MIT | 3,967 | 0.29 (via re-export) | no — 38/62 list+detail |

**crmux is the only one with the board.** `ui.rs:23-34` `compute_grid` (`cols = width/80`
clamped, `rows = n.div_ceil(cols)`), grid branch at `ui.rs:477-500`, `ansi_to_tui` per
visible pane per frame at `ui.rs:515-518`. But it costs **two tmux forks per pane per
frame** (`app.rs:566-598`: `capture_pane_content` + `query_tmux_cursor`) on a **50 ms
poll** (`app.rs:688`) — ~480 forks/s at 12 panes, with no dirty-check and no push wakeup.
Our demo hashes and skips the redraw (`board.rs:119-123`) and has SIGUSR1 push
(`push.rs`). Its detector also lives in an external crate (`tmux-claude-state 0.4.1`,
`Cargo.toml:19`) from a different repo.

**Two disqualifiers shared by the rest.** (1) Wrong unit: bosun and thurbox key on
*sessions they created*, on *their own private socket* — `bosun`: `MONITOR_SESSION`
+ `DEFAULT_SESSION_PREFIX = "bosun-"` + `DEFAULT_TMUX_SOCKET = "bosun"` (`config.rs:64,77`);
`thurbox`: `TMUX_SOCKET = "thurbox"`, one session named `thurbox`, window prefixes
`tb-`/`tbs-` (`agent/tmux.rs:22-27,60-64,82-88`). `list-panes` appears **zero times** in
bosun. Our board must enumerate every agent pane on the *user's default* server.
(2) One emulator, not N: bosun holds `embed: Option<EmbedTerminal>` — singular
(`app.rs:2200`); thurbox has N `vt100::Parser`s but paints one (`terminal/mod.rs:1350,1383,1412`).
Neither gives evidence on what N concurrent tiles cost.

claudectl's whole tmux surface is **80 lines** (`terminals/tmux.rs`), never calls
`capture-pane`, and keys on PID-from-`ps`. agent-console has **zero tmux** — it
reimplements a multiplexer in `pty.rs` (6,046 prod LOC). iris-tui has **zero tmux**.

**Fork estimates, all six: delete 90–99%, then still write the board.** Best case is
crmux at ~5% kept (~260 of 4,776 prod lines).

### Worth copying (~1,900 LOC, all MIT except agent-console's Apache/MIT)

| from | file:line | LOC | why |
|---|---|---|---|
| bosun | `src/tmux/detector/**` | **1,428** | The prize. Pure fns over (bytes, plain, prev, title, cmd) — `mod.rs:1-8` guarantees no tmux/fs coupling. 41 tests included. Claude detector (`detector/claude.rs:63-107`) identifies Claude by its renamed process (`2.1.220` → `2_1_220`, regex `^\d[\d_]*_[\d_]*$`, `claude.rs:120-152`) and treats the braille spinner in `#{pane_title}` as authoritative because it can't go stale (`claude.rs:76-84`). Swap `session_name` → pane id. |
| bosun | `src/util/hysteresis.rs` | 159 | Streak smoother, stops glyph flicker. 8 tests. |
| crmux | `tcs/.../claude_state.rs:74-272` + `tests/claude_state_test.rs` | 200 + 511 | Second opinion on the same problem, and the **fixture library** is the real value — real pane text per state. |
| thurbox | `src/agent/tmux.rs:1599-1615` | 17 | `history_seed_bytes` — `capture-pane -e -p -J` → vt100 input (`\n`→`\r\n`). |
| thurbox | `extensions/hooks/claude.json` | 57 | Hook wiring incl. the blocked detector: `case "$(cat)" in *permission*|*approval*) ... blocked`. |
| iris-tui | `src/bridge.rs` | **661** | **Phase 2/3, and the best single file found.** A finished file-based PreToolUse approval broker: request/decision dirs, 8 s heartbeat so a dead supervisor never wedges a session, runtime arm/disarm marker, 25 s timeout that defers to Claude's native prompt, phantom GC, `IRIS_BASE_DIR` test seam. Deps: `dirs` + `serde_json`. Turns a needs-you tile into an actionable one. |
| claudectl | `crates/claudectl-core/src/transcript.rs` | 200 | Pure serde_json Claude-JSONL line parser. |
| claudectl | `crates/claudectl-core/src/models.rs` | 201 | Model pricing table + `shorten_model()`. |
| agent-console | `src/main.rs:902-930` | 29 | `session_card_layout` — cleanest grid math seen: cols-from-width, remainder-distributed widths, scroll-to-keep-selection-visible. |

**One design idea worth more than the code:** thurbox replaces polling with a control-mode
*format subscription* — `refresh-client -B '<name>:%*:#{@thurbox_state}'`
(`agent/control_mode/mod.rs:790-810`), delivering `%subscription-changed` per pane. That is
push-based per-pane state for free. Phase 2 candidate; our SIGUSR1 already covers phase 1.

**The control-mode trap:** bosun holds a real long-lived `tmux -C attach-session`
(`control_client.rs:60-74`) and its 548-line parser is clean — but it **parses `%output` and
throws it away** (`tmux_actor.rs:698-716`, comment admits it), so that path has never run in
anger. It also never reconnects: EOF → `Notification::Exit` → parks on
`std::future::pending()` forever (`control_client.rs:99`, `tmux_actor.rs:679-684,718-725`).
And `%output` only streams for panes in *attached* sessions — the invisible-window problem
`capture-pane` polling sidesteps. Cost: bosun's own fast tick captures the **focused session
only** (`tmux_actor.rs:1400`) because all-sessions was ~60 tmux processes/sec.

Two lessons, one of which did not survive: `capture-pane` wants `-J` or you split
mid-ANSI-sequence (`client.rs:300-306`) — that one holds. The other, "use a **printable**
field separator, never `\x1f`, because tmux 3.4+ octal-escapes control chars in format
output" (bosun `client.rs:492-498`, hence its `|||`), is **refuted**: verified.md tmux B
measured format output raw through `list-panes -F`, `display-message -p`, `show-options -v`
and `show-environment`; only the `show-options` listing form escapes. bosun's unescaper
decodes an escape tmux never applied.

One bug to not inherit: crmux's `splitn(4, ' ')` on an unquoted `list-panes` format
(`tcs/.../tmux.rs:27-34,54-63`) **silently drops any pane whose `pane_current_path` contains a
space**. Same class as our `data.rs:376` tab-split. Decision 3's `\037` + in-format `#{s/…}`
fixes both; `#{q:}` does not (verified.md tmux C: it escapes the space and passes the
control characters raw, which is backwards for us).

## B. Widget crates — adopt ZERO for phase 1, one for phase 2

ratatui 0.30.2 = `ratatui-core ^0.1.2` + `ratatui-widgets ^0.3.2` (+ crossterm/macros/termina/
termion/termwiz shims), so any crate requiring `ratatui-core ^0.1` / `ratatui-widgets ^0.3` is
0.30-clean and pulls no second ratatui. Demo baseline: 4 deps, 187 crates in `Cargo.lock`.

| crate | latest | ratatui 0.30? | verdict |
|---|---|---|---|
| **tui-term** | 0.3.4 | ✅ core ^0.1 / widgets ^0.3 | **No.** Wants a `vt100::Screen`, i.e. a live byte stream — a pty + `tmux attach` + OS thread per tile (bosun `embed_terminal.rs:183-296`) or control-mode `%output`. **Measured: `capture-pane -e` emits only SGR, zero cursor-motion/erase sequences** — tmux already did the emulation, so vt100 would re-emulate a rendered screen. Adds 2 deps and N ptys to replace `ansi-to-tui`, which is already correct. |
| **tui-input** | 0.15.4 | ✅ ratatui ^0.30.2 (opt) | **Yes — phase 2 only.** Headless, `unicode-width`-correct cursor/edit handling for the reply line: ~150 lines we would otherwise get wrong on wide chars. No ratatui coupling required. |
| tui-textarea | 0.7.0 | ❌ ratatui ^0.29, crossterm ^0.25/^0.28 | **No.** Last published 2024-10-22; would pull a second ratatui. If a multiline editor is ever needed use the maintained fork `ratatui-textarea` 0.9.2. The phase-2 reply line is one line, so tui-input beats both. |
| tui-popup | 0.7.6 | ✅ | **No.** A centred bordered rect is ~10 lines of `Layout` + `Clear`; the board already draws bordered blocks (`board.rs:53`). |
| tui-scrollview | 0.6.7 | ✅ | **No.** Tiles don't scroll — the board pages selection. Revisit only if a tile gains its own scrollback. |
| tui-widget-list | 0.15.3 | ✅ | **No.** Variable-height `List`; the board is a grid and `fleet.rs` already uses `Table` + `TableState`. |
| throbber-widgets-tui | 0.11.1 | ✅ ratatui ^0.30 | **No.** A spinner is `FRAMES[i % FRAMES.len()]`. Better: read Claude's *own* braille spinner out of `#{pane_title}` (bosun `claude.rs:76-84`) — real agent state, not our animation. |
| tui-logger | 0.18.3 | ✅ ratatui ^0.30 | **No.** `COCKPIT_LOG` already appends timings to a file (`data.rs:30-37`); an in-TUI log pane is debugging furniture in a popup that lives one keypress. |
| tui-prompts | 0.6.7 | ✅ | **No.** Also pulls `ratatui-macros`. The board's only prompt is y/n. |
| ratatui-image | 11.0.6 | ✅ | **No**, as the owner said. Sixel in a tmux popup, for a text board. |
| tui-big-text | 0.8.9 | ✅ ratatui ^0.30 | **No**, as the owner said. |
| rat-widget | 3.2.1 | — (upd 2026-03-09) | **No.** A 40+ widget kitchen sink with its own focus and event handling — the largest thing on the list, for a board that draws bordered blocks. |

No "dashboard/grid/tiles" widget crate exists in the list; every grid found in the wild is
~30 lines of hand-rolled `div_ceil` + remainder distribution (crmux `ui.rs:23-34`,
agent-console `main.rs:902-930`, ours `board.rs:15-23,43-47`). Copy agent-console's 29 lines
if we want scroll-to-keep-selection-visible; otherwise ours already works.

**Net: the demo stays at 4 deps through phase 1, 5 in phase 2 (tui-input).**

## C. Verification

- `capture-pane -e` SGR-only claim: throwaway `-L capchk2$$` server, pane painted with
  `\033[2J`, `\033[5;10H`, `\033[2;3H`, `\033[7;1H` + SGR. Captured output contained
  `[0m [1m [31m [32m [39m [44m [49m` and **0** matches for `\x1b\[[0-9;]*[HfABCDJK]`.
  Cursor moves came back as leading spaces on the right rows. Server killed.
- Crate versions/deps: crates.io API (`/api/v1/crates/{c}` and `/{c}/{v}/dependencies`),
  2026-08-27. Stars/licences: GitHub API, same date.
