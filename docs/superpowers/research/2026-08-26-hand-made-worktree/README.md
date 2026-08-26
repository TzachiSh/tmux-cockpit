# Issue #49: an agent in a hand-made worktree is invisible to prefix r

Preliminary research, 2026-08-26. Two read-only opus lanes: [flow.md](flow.md) (where the tree comes from, every caller that breaks the same way, what Claude does to cwd) and [ux.md](ux.md) (every message the flow can print, where it lands, and what the empty queue should say). No code changed.

## What was measured

- Six claude panes live, all with `pane_current_path` = the main checkout, clean master. One agent's work sits in `~/.cache/cockpit-wt/lowercase-keys` (5 dirty files). `tmux-review --fleet` prints nothing, so prefix r flashes "no agent pane is blocked or has uncommitted changes".
- The claude process cwd really is the main checkout (`/proc/<pid>/cwd`), and so is every child. The agent drives the worktree with `git -C`. Nothing that reads a cwd can see it.
- `EnterWorktree` does `process.chdir` (evidence in the 2.1.246 binary), so issue #3's hook fix holds for that path. `git worktree add` by hand, which the owner's worktree rule prescribes, leaves no sidecar and no cwd change. The two paths were never reconciled.
- Nine call sites derive "the tree the agent works in" from `pane_current_path` (table in flow.md §1): the queue, `--delta` (so review-gate never flags ◆ either), `--sess`, `--request`, `--info`, the preview, the chosen row, `tmux-fixer`, the popup title.
- "running" and "done" are states no surface reports: the queue drops clean panes at `tmux-review:1793`, `@cockpit_fleet` counts only needs-input/error/pending, `tmux-jump` uses the same three predicates.
- Messages: `printf >&2` inside `display-popup -E` is never rendered (2 dead messages). A status flash survives the popup closing for `display-time` (2 s here, 750 ms stock) and on the owner's theme is painted into the middle of status-right, not over it.

## Recommendation, merged

Mechanism (flow.md §4): one `pane_dir()` helper beside `git_root()` that prefers a pane option `@cockpit_dir` over `pane_current_path` when the path exists; `fleet_rows` reads the option in the format string it already builds (no new fork, the 20-pane fleet gate stays at `20 + 19*5`); five callers route through it, `--dir <pane>` for `tmux-fixer`. The writer is one documented line for agents that make worktrees by hand: `tmux set-option -p -t "$TMUX_PANE" @cockpit_dir <path>`. The zero-code alternative for an agent is `EnterWorktree` with `path` on the tree it just made.

Visibility (ux.md §3): delete the dirty filter at `:1793` so every agent pane has a row (measured: 4 rows, 313 ms); split the `—` stat sentinel into `clean` and `no repo`; `↵` on a clean row jumps instead of opening a review that closes itself (section 8 #3, never implemented); header line 2 says `↵ review or jump`.

Messages (ux.md §5): `:2159` becomes a counted statement of what was seen; `:1211` names the tree and branch it read as clean; `:2280` and `:1147` become `display-message`; the rest of the one-line rewrites are in the table.

## Decisions the owner has to make

1. Unfiltered queue always, or behind an option. It reverses the phase-1 "dirty-only" ruling; on a 50-pane fleet the list is 50 rows.
2. Is a clean row reviewable at all (`^B` history), or does `↵` only jump.
3. Whether `@cockpit_fleet` should count running/done so status-right stops rendering empty with agents live. Costs a server walk per counter; separate issue if wanted.
4. Whether `@cockpit_dir` should also steer `#{agent_where_path}`. Both lanes say leave it.

## Fail-first tests (flow.md §4)

Seven assertions in the existing `== tmux-review --fleet ==` block: clean pane listed with `clean`; `@cockpit_dir` on a clean main pane pointing at a dirty worktree gives the worktree's repo⑂branch and churn; `--delta` exits 0 there; `--sess` differs from the option-unset store; a dangling `@cockpit_dir` falls back to the pane's tree; the fork gate is unchanged; the empty-queue message carries counts.
