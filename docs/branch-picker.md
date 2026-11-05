# Branch popup

`prefix + b` and a tap on the branch on the border open a `cockpit-tui branch`
popup (#246). The keys, the preview and what `scripts/tmux-branch` checks before
it acts are in [tui.md](tui.md#cockpit-tui-branch-246).

If `cockpit-tui` is older than the plugin (it exits 2 with `unknown screen`),
the popup says so and names `make tui` and `scripts/tui-fetch` (#276).

## Click the branch to switch branch

Type to filter. `↵` switches; `alt-↵` creates a branch named by what you typed;
a remote-only branch becomes a local tracking branch. The list is most recent
first and marks the branch you are on with `●`. A branch held by a linked
worktree carries `⑂name`: git would refuse it, so `↵` goes to that worktree
instead. It selects the pane already working there (the tree its board row
shows is inside it), an agent's pane before a shell, and opens a new window in
the tree when no pane is there. It never `cd`s a pane. The header says
`detached HEAD` when no branch is checked out. When git refuses a switch (a dirty
tree, say) you read its message, press a key, and the popup opens again instead
of closing.

The click target is the branch on the active pane's top border (a control
range, `control|3`, acting on release). When the border has no room for it, the
menu's Switch branch row stands in. The **path** beside it (`control|2`), the cockpit table's
`o` and the menu's Browse folder row open the pane's folder in the file browser (`↵` views a file, `alt-e` edits it; see
[path-picker.md](path-picker.md)).

With `@cockpit_statusbar off`, mark your own segments with the same ranges; see
[status-bar.md](status-bar.md#a-bar-of-your-own). A control range has a key of
its own, so it never takes over the click that selects a window.
