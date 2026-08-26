# fzf 0.44.1 capability check for the review picker

Verified against `man fzf` of the installed 0.44.1 and the fzf CHANGELOG.

## 1. Preview scroll keys

| Action | Bind | Min version |
|---|---|---|
| line | `--bind alt-up:preview-up,alt-down:preview-down` | core (default on shift-up/down) |
| page | `--bind alt-pgup:preview-page-up,alt-pgdn:preview-page-down` | core |
| half page | `preview-half-page-up/down` | core, already bound to alt-u/alt-d |

alt+arrow arrives as ESC[1;3A/B; tmux passes it through with the default
`xterm-keys on`, but the outer terminal must send Alt as Meta.
Mouse wheel over the preview already scrolls it, no bind needed.

## 2. Multi-select

`--multi`, tab/shift-tab mark rows. `{+n}` expands to field n of every marked
row (space separated, each shell-quoted), `{+}` whole rows, `{+f}` a temp file
with the list. `{q}` stays the typed query beside them. `--disabled` does not
affect marking.

## 3. execute() and the tty

Only `execute()`/`execute-silent()` run a child with the terminal; `become()`
(0.38) execve's over fzf and inherits fzf's fds. The report claimed execute()
gives the child a tty on stdout; measured in this repo it does not (the child
gets fzf's own stdout, which run_loop captures), so `</dev/tty >/dev/tty`
inside the command stays required.

## 4. Layout options present in 0.44.1

`--header`, `--header-lines`, `--border-label` (needs `--border`),
`--preview-label`, `--info=STYLE`, `--separator`, `--scrollbar`, `--color`
(e.g. `header:dim,border:8`), `--preview-window` flags `wrap`, `follow`, `~N`,
`+{n}/2`. Absent: `transform` (0.45), `--highlight-line` (0.52), `--tmux`
(0.53), `--wrap` (0.54), `--gap` (0.56).

## 5. Line numbers in the preview

Nothing in fzf; a transform on the preview command's output.

## 6. Text selection and copy in a popup

`--no-mouse` gives native drag-select back but loses click and wheel.
Shift+drag is the terminal-level bypass and works inside a tmux popup.
Copy to a buffer only by shell-out: `execute-silent(printf '%s' {3} | tmux
load-buffer -)`.

## 7. Mouse

Click moves the cursor, double-click accepts. Clicking inside the preview does
not scroll it; the wheel does.
