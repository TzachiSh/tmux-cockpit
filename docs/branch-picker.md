# Branch picker and folder browser

Being replaced by the board (#228).

## Click the branch to switch branch

The branch picker and the path picker share their keys: `↵` acts, `esc`
cancels, `^/` toggles the preview and `^U`/`^D` scroll it. The review popup's
two pickers are their own case — see below.

Most-recent branch first, `alt-↵` creates a branch named by what you typed, and
a remote-only branch becomes a local tracking branch. The list marks the branch
you are on with `●` and labels branches held by a linked worktree with `⑂name`
— git will refuse those, and now you can see it before pressing Enter. When git
does refuse — a dirty tree, a held branch — you read its message, press a key,
and you are **back in the picker with your query intact** instead of starting
over. It is `alt-↵` and not
`^N`, which is fzf's own move-down: as a create key it meant navigating with
emacs keys left the picker, and any filter you had typed became a branch name. When git refuses — a dirty
tree, or a branch already checked out in another worktree — you get git's own
message, naming the files or the worktree, and the popup waits for a key.

The plugin binds the key but **cannot mark the click target**: `status-right`
belongs to your config, not to the plugin. Wrap whatever shows the branch:

```tmux
set -ag status-right "#[range=control|1]#{E:@catppuccin_status_gitmux}#[norange]"
```

The **directory segment** works the same way: wrap it in `range=control|2` and
clicking it opens the **folder browser** at the pane's working dir — the same
popup the path picker descends into, entered from the top.

`range=control|1` triggers `MouseDown1Control1`, a key of its own.
`range=user|X` would have taken over `MouseDown1Status` — the key that selects a
window when you click the window list.
