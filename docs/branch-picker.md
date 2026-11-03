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
worktree carries `⑂name`: git will refuse it, and `↵` says so before git is
asked. When git refuses anyway (a dirty tree, say) you read its message, press a
key, and the popup opens again instead of closing.

The click target is the branch on the active pane's top border (a control
range, `control|3`, acting on release). When the border has no room for it, the
menu's Switch branch row stands in. The **path** beside it (`control|2`), `prefix
+ o` and the menu's Browse folder row open the pane's folder in the editor (see
[path-picker.md](path-picker.md)); the folder browser itself is gone.

With `@cockpit_statusbar off`, mark your own segments with the same ranges; see
[status-bar.md](status-bar.md#a-bar-of-your-own). A control range has a key of
its own, so it never takes over the click that selects a window.
