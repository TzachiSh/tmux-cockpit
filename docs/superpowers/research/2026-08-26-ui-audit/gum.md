# gum: measured against cockpit's prompts

Opus lane, 2026-08-26. gum v2.0.0 (2026-08-20) linux amd64, run inside a real `display-popup` on a private tmux 3.7c socket, `TERM=tmux-256color`.

## Findings

1. **`gum input` panics at its default width.** `--width=0` (the default) hits `charm.land/bubbles/v2@v2.1.1` textinput `placeholderView`: `p := make([]rune, m.Width()+1)` → "runtime error: makeslice: len out of range", exit 124, nothing rendered. Plain `gum input` is broken because `--placeholder` defaults to "Type something...". Workarounds: `--width=N` (N ≥ 1), `--placeholder=`, or `--value=x`. v2.0.0 is the only 2.x release; no fix shipped; not found filed upstream.
2. Exit codes once worked around: submit 0, Esc 1, Ctrl-C 130, timeout 124, no tty 1. `gum confirm`: yes 0, no 1, Esc 1, Ctrl-C 130 (No and Esc indistinguishable; fine for a kill confirm). `gum filter`: **Esc returns 0 with empty stdout**, so the picker's exit code cannot decide, the exact failure `tmux-review:1283` carries a 12-line comment about.
3. **`gum spin` is the one clean win.** It propagates the wrapped command's exit code exactly (`sh -c 'exit 42'` → 42), passes stdout through when captured, renders the spinner to stderr, needs no theming. Nothing in cockpit does this today. Site: `tmux-fixer` `run_fixer`'s blocking `claude -p` (scripts/tmux-fixer:47-52) shows a frozen pane for the whole run.
4. Cost: 14.3 MB binary, ~60 ms per invocation (fzf 3.6 ms, printf 0); a six-call boxed info screen is 353 ms. Not in Debian/Ubuntu/Fedora archives: Charm's own apt/yum repo (GPG-signed), brew, pacman, nix, winget, scoop. tpm has no binary fetch, so it is "user installs it" or cockpit's own fetcher (install.md). Theming is 12-15 `GUM_*_*` exports vs fzf's one `--color` string; the defaults are pink/purple (212, #7571F9, 99), so an unthemed gum dialog is exactly the "looks like a different program" complaint in pickers.md §1.
5. Worth stealing regardless: `gum version-check '>=2.0.0'` exits 0/1, a ready-made version gate of the kind `fzf_minor()` hand-rolls.

## Verdict

`gum spin` around `tmux-fixer`'s `claude -p`, as an optional dependency with "no spinner" as the free fallback (`command -v gum` → `gum spin --title "side fix: $job" -- claude -p …` else the bare call). Everything else, the two read prompts, the kill confirm, the two fzf confirm pickers, the info screen, the side-fix pane's first printf, is better served by what exists (read, fzf with the `--color`/`--no-input` fixes in pickers.md, printf). The `prefix ?` observer spinner in screens.md §5 is eight lines of `printf '\r'`; gum would add a 14 MB dependency to replace them.
