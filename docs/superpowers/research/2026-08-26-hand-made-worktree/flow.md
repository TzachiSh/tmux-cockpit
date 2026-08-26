# Issue #49: where the tree an agent works in should come from

Research lane: flow (opus, read-only). No code changed. A private tmux server `-L rsrch49` was used for one experiment and killed.

## 1. The flow, and every place the tree is derived from `pane_current_path`

`prefix r` -> `cockpit.tmux:269-283` binds `display-popup -E … "$Q_REVIEW -p $SRC_PANE"` (the popup's own `sh` resolves `$SRC_PANE` from `$TMUX`'s session id, per #38) -> `tmux-review` `main_entry()` (`scripts/tmux-review:2274`) -> always `fleet_queue "$pane"` (section 8 decision #3) -> `fleet_rows()` (`:1687`) -> one `list-panes -a -F` -> per-pane `rev-parse --show-toplevel` (`:1740`) -> `fleet_scan_root()` (`:1828`) -> `tree_diff()` (`:150`) -> `review_base()` (`:207`) -> fzf -> `review_pane "$pane" "$(git_root "$dir")"` (`:2256`).

The kill is `scripts/tmux-review:1793`:

```
[ "$dirty" = 1 ] || [ "$state" = needs-input ] || continue
```

`$dirty` comes from `fleet_scan_root` on the root of `#{pane_current_path}` (`:1707`, `:1740`). Six live claude panes all report `/home/tzachi/.tmux/plugins/tmux-cockpit`, clean master, so every row is dropped and `:2159` prints the empty message.

Sibling callers doing the identical two-line prelude (`display-message -p '#{pane_current_path}'` then `git_root`), all broken the same way for a hand-made worktree:

| site | what breaks |
|---|---|
| `tmux-review:271` `delta_mode` | `--delta` is what `review-gate:228` asks on every Stop. Wrong tree means the pending flag is never set, so `@cockpit_fleet` never counts the agent either |
| `tmux-review:1707` + `:1740` `fleet_rows` | the row itself, plus its repo-branch label |
| `tmux-review:1914` `sess_mode` | `--sess`, used by `tmux-fixer` and `review-gate`: the store is keyed on `pane + cksum(root)`, so a worktree review would collide with the main checkout's |
| `tmux-review:1998` `request` | `--request`, the agent's own ask via `skills/cockpit-review/SKILL.md:10`. An agent in a worktree asking for review gets a popup showing clean master |
| `tmux-review:2045` `info_mode` | `prefix i` reports the wrong dir and base |
| `tmux-review:2213-2215` | the fzf preview re-derives the dir from the pane a second time |
| `tmux-review:2256` | the chosen row re-derives it a third time; the row does not carry it |
| `tmux-fixer:105` | the side-fix pane opens `-c "$root"` in the main checkout and fences the wrong `status --porcelain` |
| `cockpit.tmux:269` | popup title `review · #{b:pane_current_path}` |

Not broken by this and out of scope: `tmux-where` (`:94`, and `examples/*.conf:242-246` `#{agent_where_path}` / `_branch`) reads the pane's real cwd from `/proc` and is correct by definition, it labels where the pane sits. `tmux-branch:48`, `tm:81`, `tmux-open-path:457,689` are about the human's pane, also correct as-is.

## 2. Where the tree should come from

Established first, because it eliminates half the candidate list: in the reported case the agent's cwd is not merely stale, it is right. The claude process really does sit in the main checkout and drives the worktree with `git -C`. Nothing that reads a cwd, at any level, can see the worktree.

| candidate | verdict |
|---|---|
| **(a) pane option written by agent or hook** | The only hook payload field that could carry it is `cwd`, and `cwd` is the session cwd, which for a hand-made `git worktree add` is the main checkout. `review-gate` already parses payload fields without jq (`:75-82`) and could add one line, but it would learn nothing new. A pane option written by the agent itself does work, and costs zero forks and zero tmux round-trips: `fleet_rows` already builds one `-F` format string carrying `@panestate` and `@cockpit_review` (`:1703-1705`); one more `#{?#{@cockpit_dir},…,-}` field rides along free. Correct with two agents in one repo (per-pane), correct when an agent has two worktrees (it declares the current one), and a worktree with no agent simply has no row, which is right for a pane-keyed queue |
| **(b) WorktreeCreate sidecar** | `scripts/worktree-create` writes `<tree>.repo`, and `ls ~/.cache/cockpit-wt` has no sidecars: none of the seven trees went through it. The sidecar also maps tree -> repo, never pane -> tree, which is the direction needed |
| **(c) `git worktree list --porcelain` of the pane's repo** | Cheap in wall clock (measured 3 ms on this repo, 6 trees) but it is one fork per unique root, and `tests/run.sh:6744` pins the fleet at `20 + 19*5` git calls with the comment "the next helper that costs a fork per root fails here on any box". So it costs a deliberate budget bump. Worse, it cannot attribute: on this box it returns 6 trees including a stale `/tmp/tmp.9Yd6liv4eu/wt` and four trees whose agents are gone. With two agent panes on one repo every tree is equally theirs |
| **(d) /proc walk of the pane's children** | Dead. The issue already measured every claude child at the main checkout, and Claude's Bash tool runs a persistent shell at the session cwd, so the children lie in exactly the same way the parent does |
| **(e) Claude session jsonl** | Dead, and measured: `grep -ho '"cwd":"[^"]*"' *.jsonl` over this project's transcripts gives 41177 lines at the main checkout and 1420 at two cockpit-wt trees, and both of those are the two `EnterWorktree` sessions. The hand-made path never records the worktree anywhere |
| **(f) the agent tells cockpit** | This is (a)'s writer. One documented line, `tmux set-option -p -t "$TMUX_PANE" @cockpit_dir <path>`, needs no new script. Same shape `agent-state` already uses |

node-dev (Coder box, no `.git` in the plugin copy): panes there resolve an empty root, `fleet_scan_root` short-circuits to `0\t?\t?\t—\t0` (`:1834-1844`) and they are dropped today. Nothing in any option above changes that, and dropping the dirty filter (below) makes them visible with `?-?`, which is an improvement.

## 3. What Claude Code does to process cwd

**EnterWorktree moves the process-wide cwd.** From the 2.1.246 binary's own validation message at offset 211131702: EnterWorktree is refused from a subagent with a cwd override because "it would mutate the parent session's process-wide working directory". So for a top-level session it calls `process.chdir` (the wrapper `function xi(e){process.chdir(e);try{process.cwd()}catch{}}` at offset 201008223 exists for exactly this, the `process.cwd()` re-read flushing graceful-fs's cached cwd). tmux on Linux reads the foreground process group's `/proc/<pid>/cwd`, so `pane_current_path` follows. This is why issue #3's fix holds for that path.

Two nuances from the same tool description worth having:

- A subagent with a pinned cwd may only enter by `path`, and that switch "only affects this agent, not the parent session", so a subagent's worktree never shows in `pane_current_path`.
- EnterWorktree accepts `path` for a tree "you just created with `git worktree add`", provided it appears in `git worktree list`. The hand-made trees under `~/.cache/cockpit-wt` qualify.

`claude --worktree <name>` was not tested end to end: on a fresh scratch repo it refuses with "Workspace trust not yet accepted", and granting trust means editing the user's `~/.claude.json`. It starts in the worktree by construction, so the question does not really arise.

## 4. Recommendation

Three changes in `scripts/tmux-review`, one in `scripts/tmux-fixer`, no new abstraction and no new script.

**1. One helper, replacing the two-line prelude at five sites.** Beside `git_root()` at `:73`:

```sh
pane_dir() {                  # <pane> -> the tree this pane WORKS in, not the one it sits in
    local pane=$1 out dir wt
    out=$(tmux display-message -p -t "$pane" $'#{pane_current_path}\037#{@cockpit_dir}' 2>/dev/null)
    dir=${out%%$'\037'*}; wt=${out#*$'\037'}
    [ -n "$wt" ] && [ -d "$wt" ] && dir=$wt
    [ -n "$dir" ] || return 1
    git_root "$dir"
}
```

One tmux call, same as today, US-separated the way `info_mode` already does it. Callers: `delta_mode:271`, `sess_mode:1914`, `request:1998`, `fleet_queue:2251-2256`, and `info_mode` (which should show both, "sits in" and "works in"). `fleet_rows` gets `#{?#{@cockpit_dir},#{@cockpit_dir},-}` appended to the format at `:1707` and prefers it over `path` before the `rev-parse` at `:1740`, so the fork budget is untouched. Add `--dir <pane>` to the dispatch (two lines) so `tmux-fixer:105` can route through the same answer instead of re-deriving.

**2. Drop the dirty filter at `:1793`.** Every agent pane gets a row; `fleet_scan_root` already renders a clean tree as an em-dash, and rank 4 sorts the quiet ones to the bottom. This is what answers "if there is nothing to review I still need to see it", it restores `^X kill window` and the preview for a clean agent, and it makes the agent in the hand-made worktree visible today even before anyone sets `@cockpit_dir`. It contradicts one existing test (`tests/run.sh:6439`, "a clean agent pane is not in the queue"), which the issue explicitly overrules.

**3. The message at `:2159` becomes true.** With the filter gone, an empty result means exactly one thing: no pane matched `@cockpit_agents_glob`. Say that, and name the glob.

**Writer for `@cockpit_dir`:** the memory rule that tells agents to run `git worktree add` by hand gains one line, `tmux set-option -p -t "$TMUX_PANE" @cockpit_dir <path>`. There is no `AGENTS.md` or `CLAUDE.md` in this repo, so that rule lives only in the owner's project memory; `README.md` should carry the option too. Worth saying plainly: the zero-code fix for the reported instance is for agents to call `EnterWorktree` with `path` on the tree they just made, which moves the process cwd and needs nothing from cockpit at all. `@cockpit_dir` is what makes cockpit right when they forget.

**Fail-first tests**, all in the existing `== tmux-review --fleet ==` block (`tests/run.sh:6382`), which already has `mk_git_dir` and a control pane:

1. Invert `:6439` to "a clean agent pane is listed, with an em-dash for churn".
2. A pane whose cwd is a clean main checkout with `@cockpit_dir` set to a dirty linked worktree: the row's repo-branch names the worktree dir and its branch, and the +/- column is the worktree's churn. Currently reads `main-main` and an em-dash.
3. `--delta` on that pane exits 0. Currently 1, which is why review-gate never flags pending.
4. `--sess` on that pane differs from `--sess` with the option unset, so the two trees do not share a store.
5. `@cockpit_dir` pointing at a path that does not exist falls back to the pane's own tree rather than erroring.
6. The fork gate at `:6744` stays at `20 + 19*5` unchanged, which is the point of putting the option in the format string.
7. The empty-queue message names the glob (the `--fleet` path prints nothing, so this one goes wherever the queue's fzf-stub tests live, `:6065`).

**Out of scope:** `git worktree list` rows for trees with no pane (add it when someone actually wants to review a tree whose agent is gone, and budget the fork then); /proc and jsonl inference, both measured dead above; `tmux-branch`, `tm`, `tmux-open-path`, `tmux-where`, which are about where a pane sits and are right already; the popup title at `cockpit.tmux:269`, which is cosmetic and already wrong for a different reason (UX lens #22).

## Open questions

- Dropping the dirty filter reverses a section 8 phase-1 decision ("50-pane fleet queue, dirty-only"). On a 50-pane fleet the queue becomes 50 rows instead of a triage list. The issue asks for it; confirm the owner wants it unconditional rather than behind a `^F`-style toggle or a `@cockpit_fleet_all` option.
- Should `@cockpit_dir` also steer `#{agent_where_path}` in the status line, so the pane's own status says which tree it is editing? That is a second reader with a different meaning of "where", and I would leave it alone.
- Trust boundary: `@cockpit_dir` is written by the agent and consumed as a `git -C` dir and a popup `-c`. An agent that can set a pane option can already run anything in that pane, so this adds no reach, but the `[ -d ]` guard should stay.
