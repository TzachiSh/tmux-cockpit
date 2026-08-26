# Review popup: big files, VS Code over Remote-SSH, copy in the path picker — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a review that stays fast with a 1 GB `core` file in the tree, a click-to-open and a `^E` that reach VS Code inside a Remote-SSH/Coder session without a user-installed wrapper, and a copy key in the path picker.

**Architecture:** three root-cause fixes, each where the cause lives. (1) `tree_diff` excludes any untracked or stat-modified file over 4 MiB by `:(exclude,literal)` pathspec before git hashes it (git reads a changed file in full to diff it: 26 s per `git diff` for 1 GB, measured; listing + `stat` is 0.00 s), and `lines_mode` names the skipped files on the existing `(diff too large)` sentinel row; `--preview-diff` refuses a file over the same cap before its sha1. Every caller of `tree_diff` (rows, fleet scan, `--copy`, and the `review-gate` Stop hook via `--delta`) is covered by the one change. (2) A shared `scripts/lib/vscode.sh` finds VS Code's remote `code` shim (`~/.vscode-server/cli/servers/*/server/bin/remote-cli/code`, old `bin/*/…` layout too) and runs it against each `vscode-ipc-*.sock` newest-first; the shim IS the probe (dead socket: rc 1 in 144 ms; live: `--goto` returns 0 in 157 ms, measured on the Coder workspace), so no `socat`/`nc`. `open_one` uses it when `code` is not on `PATH`; `editor_cmd` uses it (with `--wait`) when no editor is configured, before falling to `vi`. (3) The path picker gets `alt-c` (copy the marked paths) in both its list and its browser.

**Tech Stack:** bash 4+, tmux ≥ 3.3, fzf 0.44.1, git (`ls-files -o/-m`, pathspec magic `exclude`,`literal`: git ≥ 2.x), POSIX awk. No new dependency. Community precedent for the cap: difftastic `--byte-limit` (1 MB, prints "too large"), GitHub renders no diff over 1 MB, GitLab `diff_max_patch_bytes`.

**Spec:** the measurements in this plan's Task 0 and the owner's three reports (2026-08-26): "no editor found" on the Coder workspace when Ctrl+clicking a path with a Remote-SSH VS Code window open; the review "loads very slow" with a big `core` file; no way to copy in the path picker popup.

**Branch:** `feat/review-bigfile-vscode`, worktree `~/.cache/cockpit-wt/review-bigfile`, off master `efe1535`. Gate on master: `make check` 786 passed / 0 failed.

## Global Constraints

- Work only in the worktree; never edit `~/.tmux/plugins/tmux-cockpit`; no `git stash`.
- `make check` unpiped is the gate; `make test-only PAT='<block title ERE>'` for the TDD cycle; `make lint` clean.
- Every git read on an agent's tree: `--no-optional-locks`, never the real index (`tree_diff`'s temp index).
- `--lines` output for the eight goldens stays byte-identical (`tests/fixtures/review/*.tsv`): none has a file over the cap.
- Hooks never block the agent: `review-gate` has `"timeout": 5`; `tree_diff --numstat` on a tree with a 1 GB untracked file must return in under a second after this plan.
- Tests: private `-L` sockets in `SOCKS`, `mktmpd`/`mktmpf`, `until_cap`/`until_file`, no new fixed `sleep`. Big-file fixtures use `head -c` from `/dev/zero` (no `fallocate`, not on every FS) and stay ≤ 64 MiB.
- Comments in code: 2-3 lines, one fact the code cannot say. A deliberate ceiling gets a `# ponytail:` line.
- Commit messages: lowercase `fix:`/`feat:`/`test:`/`docs:` one-liners, past tense, measured numbers; trailer on its own lines:
  ```
  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01EoVsC8zPynPRR6Xq5kBZCN
  ```
- Ponytail governs the build: shortest correct diff, no abstraction with one caller (the lib has two), deletion over addition.

## Ponytail audit

| asked for / considered | verdict | why |
|---|---|---|
| `core.bigFileThreshold` | **skip** | measured: 26 s with it set to 1m; git still hashes the worktree file to diff it |
| a tmux option for the cap (`@cockpit_review_max_file`) | **skip** | 4 MiB is the review's existing byte cap; one number, one `# ponytail:` line; add the option when someone reviews a 5 MB fixture on purpose |
| a probe via `socat`/`nc`/python | **replace** | the shim fails in 144 ms on a dead socket and succeeds in 157 ms on a live one; the CLI is the probe, zero dependencies |
| keep the README wrapper | **replace** | the plugin does it now; the README keeps one line on `VISUAL` for a GUI editor on the local desktop |
| `--wait` hang on a socket whose window closed (8 h grace) | **accept, noted** | `# ponytail:` line; the popup's esc kills it |
| a separate "skipped files" row per file | **replace** | one sentinel row naming them all, on the `(diff too large)` sentinel every reader already refuses |
| copy in the browser as full paths | **keep** | `$dir/{+}`; the list copies its `~`-abbreviated field |

---

### Task 0: Worktree, plan, baseline

- [ ] **Step 1: Confirm** `cd ~/.cache/cockpit-wt/review-bigfile && git log --oneline -1` → `efe1535`.
- [ ] **Step 2: Commit this plan**
```bash
git add docs/superpowers/plans/2026-08-26-review-bigfile-vscode.md
git commit -m "docs: plan — files over 4 MB are excluded before git hashes them, VS Code's remote shim replaces the wrapper, alt-c copies paths in the picker"
```
Measured baseline (keep for the commit messages): 1 GB untracked `core`: `--lines` 54.14 s, first `--preview-diff core` 55.59 s, `--copy` 26.90 s, `git diff --numstat` alone 26.62 s; `git ls-files -o --exclude-standard -z; git diff --name-only -z` + `stat` 0.00 s; the same diff with `:(exclude)core` 0.00 s.

---

### Task 1: `tree_diff` skips files over 4 MiB before git reads them; the list names them

**Files:**
- Modify: `scripts/tmux-review` — new `big_files()` above `tree_diff()`; `tree_diff()` (the `GIT_INDEX_FILE=$idx git ... diff` calls); `lines_mode()` (emit the notice row before the sorted rows).
- Test: `tests/run.sh` — new block after `== tmux-review --lines: the diff never writes the agent's index ==`.

**Interfaces:**
- Produces: `big_files <dir>` → lines `<bytes>\t<path>` for every untracked (`ls-files -o --exclude-standard`) or stat-modified (`ls-files -m`) file over `BIGCAP` (4 MiB), root-relative paths, NUL-safe input, newline output; empty when none. `BIGCAP=$(( 4 * 1024 * 1024 ))` next to `CAP_SENTINEL`.
- `tree_diff` unchanged signature; when its `"$@"` carries no `--`, it appends `-- . ':(exclude,literal)<path>'` per big file.

- [ ] **Step 1: Write the failing test**

```bash
echo "== tmux-review --lines: a file over 4 MB is skipped before git reads it, and the list says so =="
RB=$(mktmpd)
(
  cd "$RB" || exit
  export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@e GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@e
  git init -q -b main .; printf 'a\n' > f.c; git add -A; git commit -q -m init
  printf 'a\nb\n' > f.c
  head -c $(( 64 * 1024 * 1024 )) /dev/zero > core          # untracked, 64 MiB
  printf 'x\n' > small.txt                                  # untracked, small: still reviewed
) >/dev/null 2>&1
rb_t0=$(date +%s%N)
rbrows=$("$DIR/scripts/tmux-review" --lines "$RB")
rb_ms=$(( ($(date +%s%N) - rb_t0) / 1000000 ))
is "the small change is still a row"      "1" "$(printf '%s\n' "$rbrows" | grep -c $'^f.c\t')"
is "the small untracked file is still a row" "1" "$(printf '%s\n' "$rbrows" | grep -c $'^small.txt\t')"
is "the big file has no rows"             "0" "$(printf '%s\n' "$rbrows" | grep -c $'^core\t')"
case "$rbrows" in
    *"(diff too large)"*"core (64 MB) skipped"*"over 4 MB"*) ok "one notice row names the skipped file and its size" ;;
    *) bad "one notice row names the skipped file and its size" "(diff too large) ... core (64 MB) skipped ... over 4 MB" "$rbrows" ;;
esac
[ "$rb_ms" -lt 1500 ] && ok "and the list comes back in under 1.5 s (${rb_ms} ms)" || bad "and the list comes back in under 1.5 s" "< 1500 ms" "${rb_ms} ms"
is "--numstat (the Stop hook's and the fleet's read) omits it too" "0" "$("$DIR/scripts/tmux-review" --copy "$RB" | grep -c '^diff --git a/core')"
printf 'big\n' > "$RB/f.c"; head -c $(( 64 * 1024 * 1024 )) /dev/zero >> "$RB/f.c"   # a TRACKED file grown past the cap
rbrows2=$("$DIR/scripts/tmux-review" --lines "$RB")
is "a tracked file grown past the cap is skipped the same way" "0" "$(printf '%s\n' "$rbrows2" | grep -c $'^f.c\t')"
case "$rbrows2" in *"f.c (64 MB) skipped"*) ok "and named" ;; *) bad "and named" "f.c (64 MB) skipped" "$rbrows2" ;; esac
rm -rf "$RB"
```
(`--copy <dir>` with no path is `tree_diff "$dir"`, the same call the fleet scan and `--delta` make with `--numstat`; asserting on it keeps the test free of a tmux server.)

- [ ] **Step 2: Run, expect FAIL** — `make test-only PAT='a file over 4 MB is skipped'` (today: `core` has no rows either, git reports it binary, but no notice, and the run takes seconds; the `f.c` grown case has 1 row of 6 million).

- [ ] **Step 3: Implement**

Beside `CAP_SENTINEL`:
```bash
# Files git would have to hash in full to diff: 26 s per diff for 1 GB,
# measured. Same cap as the review's byte cap; difftastic's --byte-limit
# and GitHub's 1 MB "no diff" do the same thing.
BIGCAP=$(( 4 * 1024 * 1024 ))
# ponytail: one number, no option; add @cockpit_review_max_file when someone reviews a 5 MB fixture on purpose.
```
Above `tree_diff()`:
```bash
# Untracked and stat-modified files over BIGCAP, "<bytes>\t<path>". ls-files
# -o/-m never hash; git diff would. 0.00 s on a 1 GB file, measured.
big_files() {                 # <dir>
    local dir=$1 p sz
    { git -C "$dir" --no-optional-locks ls-files -o --exclude-standard -z
      git -C "$dir" --no-optional-locks ls-files -m -z; } 2>/dev/null \
    | while IFS= read -r -d '' p; do
        sz=$(stat -c %s -- "$dir/$p" 2>/dev/null || stat -f %z -- "$dir/$p" 2>/dev/null) || continue
        [ "${sz:-0}" -gt "$BIGCAP" ] && printf '%s\t%s\n' "$sz" "$p"
    done
}
```
In `tree_diff()`, before the two `diff` invocations, build the excludes when the caller passed no pathspec of its own:
```bash
    local -a ex=()
    case " $* " in *' -- '*) ;; *)
        local bp
        while IFS=$'\t' read -r _ bp; do [ -n "$bp" ] && ex+=(":(exclude,literal)$bp"); done < <(big_files "$dir")
        [ "${#ex[@]}" -gt 0 ] && ex=(-- . "${ex[@]}") ;;
    esac
```
and append `"${ex[@]+"${ex[@]}"}"` after `"$@"` in each of the three `git ... diff ... "$@"` calls (the branch one, the plain one, the `--cached` one). `ls-files -m` is stat-based: a file with a touched mtime but same content is listed and stat'ed, nothing more.

In `lines_mode()`, right after the `[ -n "$reason" ]` block and before `[ -n "$diff" ] || return 0`:
```bash
    # The files tree_diff kept away from git, as one notice row on the
    # sentinel every reader already refuses (attach, ^Y, mark_rows).
    local skipped
    skipped=$(big_files "$dir" | awk -F'\t' '{ printf "%s%s (%d MB)", (n++ ? ", " : ""), $2, $1 / 1048576 }')
    if [ -n "$skipped" ]; then
        local note="$skipped skipped — over 4 MB, review it directly"
        printf '%s\t%s\t%s\t%s\t%s\n' "$CAP_SENTINEL" 0 "$note" "$note" 1
    fi
```
(`big_files` runs twice per `--lines`, 0.00 s each; not worth threading.) Note the `[ -n "$diff" ] || return 0` after it: a tree whose only change is the big file shows just the notice row, and `run_loop`'s "no uncommitted changes" exit no longer fires for it (`nrows` is 1). That is the intended behaviour: the reviewer sees *why* there is nothing to anchor.

- [ ] **Step 4: Run the new block, then `make test-only PAT='tmux-review --lines|byte-identical|tmux-review --fleet|--delta'` and `make lint`; expect PASS**

- [ ] **Step 5: Commit**
```bash
git add scripts/tmux-review tests/run.sh
git commit -m "fix: files over 4 MB are kept away from git diff (it hashed a 1 GB core for 26 s per call, 54 s per review, and every Stop hook); the list names them on the notice row"
```

---

### Task 2: `--preview-diff` refuses a file over the cap before hashing it

**Files:**
- Modify: `scripts/tmux-open-path` — the `--preview-diff` branch, before `csum=$(sha1sum ...)`.
- Test: `tests/run.sh` — one assertion in Task 1's block (before its `rm -rf`), or a sibling block.

- [ ] **Step 1: Write the failing test** (append to Task 1's block before `rm -rf "$RB"`)
```bash
rb_t0=$(date +%s%N)
rbpv=$( (cd "$RB" && "$DIR/scripts/tmux-open-path" --preview-diff core) )
rb_ms=$(( ($(date +%s%N) - rb_t0) / 1000000 ))
is "the preview of a big file is a one-line notice" "core (64 MB, not diffed: over 4 MB)" "$rbpv"
[ "$rb_ms" -lt 500 ] && ok "and it answers without hashing the file (${rb_ms} ms)" || bad "and it answers without hashing the file" "< 500 ms" "${rb_ms} ms"
```
- [ ] **Step 2: Run, expect FAIL** (today: `core (binary)` after several seconds)
- [ ] **Step 3: Implement** — in `tmux-open-path`, after `base=${f##*/}` and the git-dir check, before the cache-key block:
```bash
    # Over the review's cap git would hash the whole file (26 s per GB,
    # measured); tmux-review already skips it, so this says why, at once.
    sz=$(stat -c %s -- "$f" 2>/dev/null || stat -f %z -- "$f" 2>/dev/null)
    if [ "${sz:-0}" -gt $(( 4 * 1024 * 1024 )) ]; then
        printf '%s (%d MB, not diffed: over 4 MB)\n' "$base" $(( sz / 1048576 )); exit 0
    fi
```
(The directory branch returns earlier; `-f` files only reach here.) The number 4 MiB is written twice across two scripts; a `# see BIGCAP in tmux-review` comment ties them.
- [ ] **Step 4: Run the block and `make test-only PAT='preview'`, `make lint`; expect PASS**
- [ ] **Step 5: Commit**
```bash
git add scripts/tmux-open-path tests/run.sh
git commit -m "fix: the diff preview of a file over 4 MB is a one-line notice, not a 55 s hash"
```

---

### Task 3: `lib/vscode.sh` — the remote `code` shim, probed by running it; `open_one` and `^E` use it

**Files:**
- Create: `scripts/lib/vscode.sh`
- Modify: `scripts/tmux-open-path` — `open_one()`; a `LIBDIR` line near the top (next to `shq`/`note`); a `--vscode` subcommand in its dispatch.
- Modify: `scripts/tmux-review` — `editor_cmd()`; a `--vscode` arm in the dispatch.
- Test: `tests/run.sh` — one block after `== tmux-open-path: the editor command survives odd filenames ==`'s block; one after Task 1's `--edit` tty block.

**Interfaces:**
- `scripts/lib/vscode.sh`, sourced, pure (no tmux):
```bash
# VS Code's remote CLI: the shim Remote-SSH/Coder installs, reachable only
# through a live ipc socket. The shim is its own probe: a dead socket fails
# in ~150 ms (rc 1), a live one answers --goto in ~150 ms (rc 0). Measured
# on a Coder workspace, 2026-08-26.
vscode_shim() {               # -> newest shim path, or 1
    local s
    s=$(ls -t "$HOME"/.vscode-server/cli/servers/*/server/bin/remote-cli/code \
              "$HOME"/.vscode-server/bin/*/bin/remote-cli/code 2>/dev/null | grep -v '\.staging/' | head -1)
    [ -x "$s" ] && printf '%s\n' "$s"
}
vscode_open() {               # <code args...> -> 0 when a window took it
    local shim s
    shim=$(vscode_shim) || return 1
    for s in $(ls -t "${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}"/vscode-ipc-*.sock 2>/dev/null); do
        VSCODE_IPC_HOOK_CLI=$s "$shim" "$@" 2>/dev/null && return 0
    done
    return 1
}
# ponytail: a closed window's socket can stay listening for VS Code's 8 h grace; --wait then hangs until esc.
```
- `tmux-open-path --vscode <args>`: sources the lib, `vscode_open "$@"`, exit its status. `tmux-review --vscode <args>`: same, but on failure runs `${EDITOR:-vi} "${@: -1}"` (the file is always the last arg) after `tmux display-message "tmux-review: no VS Code window reachable, using vi"`.
- `open_one`: order becomes `@cockpit_editor` → `code` on PATH → shim present (`vscode_shim`) → `$VISUAL/$EDITOR` → "no editor found". The shim branch: `tmux run-shell -b "$(shq "$0") --vscode --goto $(shq "$file:${ln:-1}:${col:-1}") >/dev/null 2>&1"` (folder: `--vscode $(shq "$file")`), then `note "opened ..."` as the `code` branch does.
- `editor_cmd`: `ed=${ed:-${VISUAL:-${EDITOR:-}}}`; if empty and `vscode_shim` succeeds, `ed="$SELF --vscode --wait"`; else `vi`.

- [ ] **Step 1: Write the failing tests**

Fake VS Code install under a fake `$HOME`: the shim is a script that logs `$VSCODE_IPC_HOOK_CLI` and `$*` and exits 0 only when the socket path ends in `live.sock` (for `--wait` it also appends `FROM-VSCODE` to the last argument). Two sockets as plain files, `dead.sock` newer than `live.sock` (`touch -d`).

```bash
echo "== tmux-open-path: with no code on PATH, a click opens through VS Code's remote shim on the live socket =="
VH=$(mktmpd); VLOG="$VH/log"; VSK="$VH/run"; mkdir -p "$VH/.vscode-server/cli/servers/Stable-abc/server/bin/remote-cli" "$VSK"
cat > "$VH/.vscode-server/cli/servers/Stable-abc/server/bin/remote-cli/code" <<'SHIM'
#!/bin/sh
printf 'SOCK=%s ARGS=%s\n' "$VSCODE_IPC_HOOK_CLI" "$*" >> "$VLOG"
case "$VSCODE_IPC_HOOK_CLI" in *live.sock) case " $* " in *' --wait '*) printf 'FROM-VSCODE\n' >> "${@: -1}" ;; esac; exit 0 ;; *) exit 1 ;; esac
SHIM
chmod +x "$VH/.vscode-server/cli/servers/Stable-abc/server/bin/remote-cli/code"
: > "$VSK/vscode-ipc-live.sock"; touch -d '2020-01-01' "$VSK/vscode-ipc-live.sock"; : > "$VSK/vscode-ipc-dead.sock"
: > "$LOG"
env -i PATH="$ETMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$LOG" CLICK_DIR="$EQ" CLICK_LINE="it's.txt:12" \
    bash "$DIR/scripts/tmux-open-path" --pick %1 <<<1 >/dev/null 2>&1
vcmd=$(sed -n 's/^RUNSHELL -b //p' "$LOG" | head -1)
case "$vcmd" in *"--vscode --goto"*"it's.txt:12:"*) ok "the click hands the file to the shim through run-shell" ;; *) bad "the click hands the file to the shim through run-shell" "--vscode --goto .../it's.txt:12:1" "$(cat "$LOG")" ;; esac
: > "$VLOG"; env -i PATH="$ETMUX" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" bash -c "$vcmd"
case "$(cat "$VLOG")" in
    *"SOCK=$VSK/vscode-ipc-dead.sock"*"SOCK=$VSK/vscode-ipc-live.sock ARGS=--goto"*) ok "the newer dead socket is tried first and skipped, the live one takes the file" ;;
    *) bad "the newer dead socket is tried first and skipped, the live one takes the file" "dead then live" "$(cat "$VLOG")" ;;
esac
```
(`$ETMUX` holds only the fake `tmux` and `bash`; `ls`, `head`, `grep` are needed by the lib: symlink them into `$ETMUX` at the top of this block, `for t in ls head grep sort; do ln -sf "$(command -v $t)" "$ETMUX/$t"; done`.)

And for `^E`, after the `--edit` tty block:
```bash
echo "== tmux-review --edit: with nothing configured, ^E opens in VS Code's remote shim when a window is reachable, else vi =="
VE=$(mktmpd); VESTORE="$VE/store.md"
( cd "$VE" && git init -q -b main . && printf 'a\nb\n' > src.go && git add -A && git -c user.name=t -c user.email=t@e commit -qm init && printf 'a\nb\nCHANGED\n' > src.go ) >/dev/null 2>&1
: > "$VLOG"
env -i PATH="$ETMUX:$PATH" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$LOG" \
    "$DIR/scripts/tmux-review" --edit "$VESTORE" src.go 3 CHANGED '' "$VE" </dev/null >/dev/null 2>&1
case "$(cat "$VESTORE" 2>/dev/null)" in *FROM-VSCODE*) ok "^E with no editor configured reaches the live window with --wait" ;; *) bad "^E with no editor configured reaches the live window with --wait" "FROM-VSCODE in the store" "$(cat "$VESTORE" 2>/dev/null; cat "$VLOG")" ;; esac
rm -f "$VSK"/vscode-ipc-live.sock; : > "$VLOG"; : > "$VESTORE"
VI=$(mktmpf); printf '#!/bin/sh\nprintf "FROM-VI\\n" >> "$1"\n' > "$VI"; chmod +x "$VI"
env -i PATH="$ETMUX:$PATH" HOME="$VH" VLOG="$VLOG" XDG_RUNTIME_DIR="$VSK" TMUX_LOG="$LOG" EDITOR="$VI" \
    "$DIR/scripts/tmux-review" --edit "$VESTORE" src.go 3 CHANGED '' "$VE" </dev/null >/dev/null 2>&1
case "$(cat "$VESTORE" 2>/dev/null)" in *FROM-VI*) ok "with no live window it falls back to \$EDITOR" ;; *) bad "with no live window it falls back to \$EDITOR" "FROM-VI" "$(cat "$VESTORE" 2>/dev/null)" ;; esac
```
(The second run sets `EDITOR`, so `editor_cmd` picks it before the shim; assert instead with `EDITOR` unset and the fallback `vi` replaced: put `$VI` on `PATH` as `vi` — `ln -sf "$VI" "$ETMUX/vi"` — and drop `EDITOR=` from that env line. Do that; the assertion text stays.)

- [ ] **Step 2: Run, expect FAIL** — `make test-only PAT='remote shim'`
- [ ] **Step 3: Implement** per Interfaces. `LIBDIR` in tmux-open-path: `LIBDIR="$(dirname "$(readlink -f "$0")")/lib"` (it is symlinked into `~/.local/bin`; `readlink -f` resolves it; keep the same one-hop `readlink` form tmux-review uses if `-f` is a portability concern; GNU and BSD both have `-f`).
- [ ] **Step 4: Run both blocks, `make test-only PAT='tmux-open-path|tmux-review --edit|\^E'`, `make lint`; expect PASS**
- [ ] **Step 5: Commit**
```bash
git add scripts/lib/vscode.sh scripts/tmux-open-path scripts/tmux-review tests/run.sh
git commit -m "feat: click-to-open and ^E reach VS Code through its remote shim when code is not on PATH, probing each ipc socket by running the CLI (dead 144 ms rc 1, live 157 ms rc 0)"
```

---

### Task 4: `alt-c` copies the marked paths in the path picker and the browser

**Files:**
- Modify: `scripts/tmux-open-path` — the pick `fzf` call (the one with `--multi --keep-right`) and `browse()`'s `fzf` call: one bind each, one header word each.
- Test: `tests/run.sh` — a rig block after Task 3's.

- [ ] **Step 1: Write the failing test** (real tmux + fzf; the fake `tmux` from the harness shadows the real one on PATH inside the pane, so `load-buffer` lands in the log)
```bash
echo "== tmux-open-path: alt-c copies the marked paths to the tmux buffer =="
if need_tmux "tmux-open-path alt-c"; then :; else
PC="cockpit-pc-$$"; SOCKS="$SOCKS $PC"; PCD=$(mktmpd); : > "$PCD/a.txt"; : > "$PCD/b.txt"; : > "$PCD/c.txt"
PCS=$(mktmpd); cat > "$PCS/tmux" <<'STUB'
case "$1" in
  show-options) printf '%s\n' "${CLICK_LINE:-}" ;;   # every option query answers the clicked line; only @cockpit_click_line is read here
  display-message) case " $* " in *pane_current_path*) printf '%s\n' "$CLICK_DIR" ;; *window_id*) printf '@0\n' ;; *) printf 'MSG %s\n' "$*" >> "$TMUX_LOG" ;; esac ;;
  load-buffer) cat > "$TMUX_LOG.buf" ;;
  *) printf '%s\n' "$*" >> "$TMUX_LOG" ;;
esac
exit 0
STUB
chmod +x "$PCS/tmux"; for t in bash fzf ls head grep sort sed awk cat tput; do ln -sf "$(command -v $t)" "$PCS/$t"; done
"$TMUX_REAL" -L "$PC" -f /dev/null new-session -d -s t -x 100 -y 30 "env -i PATH=$(printf '%q' "$PCS") HOME=$(printf '%q' "$HOME") TMUX_LOG=$(printf '%q' "$PCS/log") CLICK_DIR=$(printf '%q' "$PCD") CLICK_LINE='a.txt b.txt c.txt' TERM=xterm bash $(printf '%q' "$DIR/scripts/tmux-open-path") --pick %1; sleep 30" >/dev/null 2>&1
PCP=$("$TMUX_REAL" -L "$PC" list-panes -t t: -F '#{pane_id}')
until_cap "$PC" "$PCP" '*open>*' || bad "the picker comes up" "open>" "$("$TMUX_REAL" -L "$PC" capture-pane -p -t "$PCP")"
"$TMUX_REAL" -L "$PC" send-keys -t "$PCP" Tab Tab; until_cap "$PC" "$PCP" '*(2)*'
"$TMUX_REAL" -L "$PC" send-keys -t "$PCP" M-c
until_file "$PCS/log.buf" '*a.txt*'
is "alt-c copies the marked paths, one per line" "$PCD/a.txt"$'\n'"$PCD/b.txt" "$(sed "s|^~|$HOME|" "$PCS/log.buf" 2>/dev/null)"
"$TMUX_REAL" -L "$PC" kill-server >/dev/null 2>&1
fi
```
(The list's field 2 is the `~`-abbreviated absolute path; the assertion un-abbreviates. If the harness's `show-options` stub needs the `@cockpit_click_line` name check to avoid feeding the line to `@cockpit_editor`, copy the `case " $* "` shape from the odd-filenames block instead of the one-liner.)

- [ ] **Step 2: Run, expect FAIL** (no `alt-c` bind: the buffer file never appears)
- [ ] **Step 3: Implement** — pick list: `--bind "alt-c:execute-silent(printf '%s\n' {+2} | tmux load-buffer -w -)"` and header line 1 `↵ open (folder: browse)   tab mark   alt-c copy path   ^O as-is`; browser: `--bind "alt-c:execute-silent(printf '%s\n' {+} | sed \"s|^|$(shq "$dir")/|\" | tmux load-buffer -w -)"` with `--multi` added to that call, header `↵ open / enter dir   tab mark   alt-c copy path   ^O as-is`. Check `browse()`'s `sel` parsing still takes the first selection only when several are marked (`sel=${out#*$'\n'}` then the `case`): keep `↵` on the first marked row; note it in a 2-line comment.
- [ ] **Step 4: Run the block, `make test-only PAT='tmux-open-path'`, `make lint`; expect PASS**
- [ ] **Step 5: Commit**
```bash
git add scripts/tmux-open-path tests/run.sh
git commit -m "feat: alt-c in the path picker and the folder browser copies the marked paths to the tmux buffer and the clipboard"
```

---

### Task 5: README, gate

**Files:** `README.md` — the "Ctrl+double-click" section (tab/alt-c/shift+drag lines), the "Opens in VS Code at the line" paragraph (Remote-SSH now automatic), the review section's `### VS Code over Remote-SSH` (replace the wrapper with two sentences: the plugin finds the shim itself; `VISUAL='code --wait'` only for a GUI on the local desktop), `### Coder` (one sentence: same, sockets in `/tmp`, nothing to install), keep `### Security` but drop the wrapper sentence; a "big files" paragraph in the review section: files over 4 MB are skipped and named; add `core` to `.gitignore`.

- [ ] **Step 1: README edits** (plain words, no em dashes, sentence case). Commit `docs: the review skips files over 4 MB and says which; click-to-open and ^E find VS Code's remote shim on their own; alt-c in the picker; shift+drag in any popup`.
- [ ] **Step 2: Gate** — `make lint`; `make check` twice, unpiped; `bash tests/freeze-rig/run.sh` (expect 696/712 on 121 = master). Rebase onto master if it moved; re-run once after.
- [ ] **Step 3:** do not merge; the owner decides (PR).

---

## Self-review
- Reports → tasks: slow big file → T1 (all `tree_diff` callers incl. the Stop hook) + T2 (preview); "no editor found" on the workspace → T3; copy in the picker → T4; docs → T5.
- Names across tasks: `BIGCAP`, `big_files`, `CAP_SENTINEL` (T1/T2); `vscode_shim`, `vscode_open`, `--vscode` in both scripts, `LIBDIR` in tmux-open-path (T3); `{+2}`/`{+}` (T4).
- Every test block has fixture, command and expected strings inline; the T3 shim mock exits 1 on a dead socket exactly as the real one does.
