# Status bar, borders and tabs

## Window labels

For an agent pane the tab shows the **conversation topic**, not the window name.
Claude Code writes it to `pane_title`, so with several agents running you see
what each is working on instead of three tabs all reading `claude`:

```text
 Color scheme for code … ●     Openvpn setup with voi… ○     build ○
```

Non-agent panes fall back to the window name.

The tab reads `pane_title` **inside tmux**, so it works regardless of the outer
terminal. `examples/full.tmux.conf` ships with `set-titles off`: an agent
retitles constantly, and forwarding each change out as an OSC title sequence
(they do not coalesce like cell redraws — measured 609 titles, 27KB/3s under a
storm) stalls Windows Terminal's draw loop until its tmux client freezes. The
in-tmux tab above is unaffected. To let the outer window title track the topic
on a terminal that coalesces fine (Ghostty, kitty, iTerm2), uncomment the two
`set-titles` lines in that file.

## Where it shows up

- **Window tabs** — one marker per window. Worst state in the window wins, so a
  blocked pane is never hidden behind a busy one.
- **Pane border** — the whole top border takes the state colour. Colour is
  pre-attentive: you see which pane needs you without reading anything. A
  blocked pane's border also names what it is asking, so you can tell a
  one-key approval from a decision without switching to it.
- **Active pane's border** — names any *other* window that is blocked, since
  you cannot see its border from here.
- **Status bar** — the `#{agent_fleet}` token counts blocked, errored and
  unreviewed agents across *every* session, not only the one you are attached
  to.

Nothing is drawn when nothing is happening.

## Status-bar tokens

Three tokens, usable in the six options `cockpit.tmux` rewrites when it loads:
`status-left`, `status-right`, `window-status-format`,
`window-status-current-format`, `@catppuccin_directory_text` and
`@catppuccin_gitmux_text`. Each must be set **before** the `run-shell` line.

That list is exhaustive. A token in an option of your own is never rewritten —
it reaches tmux as a name nothing defines and renders as nothing at all, and
pulling your option in with `#{E:}` does not change that.

| token | renders |
| --- | --- |
| `#{agent_where_path}` | `~/path` locally, the ssh destination when remote |
| `#{agent_where_branch}` | the git branch locally, blank when remote |
| `#{agent_fleet}` | `▲N` blocked, `✕N` errored and `◆N` finished-but-unreviewed agents, every session |

`#{agent_fleet}` is the one alert that is not tied to what you can see: window
dots and borders only speak for the session in front of you, so a *detached*
session can sit blocked for an hour without a pixel changing. It looks at the
whole server, and renders nothing at all while nothing is waiting.

It counts **window slots**, not distinct panes. A session made with
`tmux new-session -t` is a second *view* of one set of windows, and
`link-window` puts a single window in two sessions; either way a shared blocked
pane is counted once per view — three grouped views of one blocked agent read
`▲3`. tmux's own `list-panes -a` reports those panes the same way, and a format
has no means to de-duplicate them. So the count can read high on a grouped
setup. It never reads low, which is the direction that would matter: a fleet
rendering nothing is genuinely quiet.

It is pure format syntax — a redraw runs no shell — and it walks nothing
either: the token expands to one precomputed option, `#{E:@cockpit_fleet}`,
which `scripts/agent-state` rewrites at each state transition, off the redraw
path. The whole-server pane walk it replaced ran up to four times per
expansion, and an agent emitting an OSC-2 title forces an expansion on every
keystroke.

## Whole-pane tint

A **blocked** pane also gets a faint warm tint across its entire background
(`window-style`, +2% luminance — noticeable, never harmful to readability).
It is the only state loud enough to earn more than a border.

Its colour is `@cockpit_blocked_bg` (default `#52303a`). The previous
hard-coded `#241a1e` measured 1.03:1 against Catppuccin Mocha — invisible on
the very theme those variable names come from — and *darker* than most bases,
reading as disabled rather than alarmed.

The tint never fires at the pane you are **on**: `window-active-style` pins the
focused pane to the terminal default. An alert that colours the thing you are
already looking at — prompt on screen, border already red — is noise, not
signal. So the tint marks a blocked pane in a split you are *not* focused on,
and nothing else. The explicit `bg=default` is load-bearing; left unset, the
active pane inherits `window-style` and the tint comes back.

## Also included

`scripts/tmux-where` — `pane_current_path` is tmux's **local** view, so a pane
sitting in ssh reports this machine's path and git branch. This prints the ssh
destination instead, and blanks the branch. It exits early on
`#{pane_current_command}` so the common case costs nothing.

It also names a linked **git worktree**, since a branch alone is ambiguous when
you run several agents across worktrees of one repo:

```text
 main                    a normal checkout
 feat-api ⑂api-work      the same repo, linked worktree "api-work"
```

Branch and worktree come from reading `.git/HEAD` directly, with **no `git`
process at all** — a repo's HEAD is a one-line file, and this runs once per pane
on every status redraw. Measured on this repo, that took the branch segment from
6.2 ms to 2.8 ms a call, which is bash's own startup cost and nothing else. git
is still asked for one case it alone can answer: a detached HEAD, whose
short-hash width git decides from the size of the repo.

The one deliberate difference from `git rev-parse` is that this ignores
`safe.directory` — for a status bar, labelling the directory you are sitting in
is the wanted behaviour.
