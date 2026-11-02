# Branch picker and folder browser

The branch picker becomes a cockpit-tui popup and the folder browser is dropped (#228).

## Click the branch to switch branch

The branch picker and the path picker share their keys: `↵` acts, `esc`
cancels, `^/` toggles the preview and `^U`/`^D` scroll it. The review popup's
two pickers are their own case — see below.

Most-recent branch first, `alt-↵` creates a branch named by what you typed, and
a remote-only branch becomes a local tracking branch. The list marks the branch
you are on with `●` and labels branches held by a linked worktree with `⑂name`
— git will refuse those, and now you can see it before pressing Enter. When git
does refuse — a dirty tree, a held branch — you read its message, press a key,
and you are **back in the picker** (with an empty filter) instead of the popup
closing. It is `alt-↵` and not
`^N`, which is fzf's own move-down: as a create key it meant navigating with
emacs keys left the picker, and any filter you had typed became a branch name. When git refuses — a dirty
tree, or a branch already checked out in another worktree — you get git's own
message, naming the files or the worktree, and the popup waits for a key.

The click target is the branch on the active pane's top border (a control
range, `control|3`, acting on release). The **path** beside it (`control|2`)
opens the **folder browser** at the pane's working dir — the same popup the path
picker descends into, entered from the top. When the border has no room for
them, the menu's Switch branch and Browse folder rows stand in.

With `@cockpit_statusbar off`, mark your own segments with the same ranges; see
[status-bar.md](status-bar.md#a-bar-of-your-own). A control range has a key of
its own, so it never takes over the click that selects a window.
