# Review popup layout: how terminal reviewers render a commentable diff

## 1. Comparison

| Tool | Gutter format | Comment marker | Key hints | Range selection |
|---|---|---|---|---|
| **delta** `--line-numbers` | two columns, defaults `{nm:^4}⋮` (old) then `{np:^4}│` (new); a context line fills both, a `+` line blanks the left, a `-` line blanks the right | none — pager only | none | none |
| **tig** | `#` toggles a single-column number; interval-numbered in pager/blame | none; no annotation concept at all | `h` opens a help view; no persistent legend | none, but `%(lineno)` is exported to shell binds — the only TUI here that hands a line number to a script |
| **lazygit** staging | hunk view, **no line numbers** | none | persistent bottom bar + `?` panel | `v` enters line-select, arrows extend; shift-arrow does it without `v` |
| **difftastic** | two number columns `old new`, `.` where that side has no line | none | none | none |
| **gh pr review** | no diff at all — `$EDITOR` for one body | n/a | n/a | `--line`/`--start-line` exist only on `gh api`, not the porcelain |
| **gh-dash** | delegates the diff to delta/less | none | footer bar + `?` overlay | none |
| **prr** | diff quoted with `> `, **no numbers**; `@@` headers carry position | an **unquoted line directly under a quoted line** is that line's comment | it is a text file; vim folds mark files done | blank line(s) before the first quoted line opens a span, the comment closes it |
| **crit** (crit.md) | syntax-highlit diff, split per line | comment on any line, JSON in `.crit/` | TUI footer | line / file / general scopes |
| **revdiff** | own TUI | `## path:line (+)` blocks — the grammar this repo already writes | popup TUI keys | `path:43-67`; an annotation containing "hunk" auto-expands to the hunk range |
| **GitHub web** | two gutter columns (old, new); commented lines get a speech-bubble badge, thread renders **inline below the line** | badge + inline thread | hover affordances only | click a line number, shift-click a second, or drag the blue `+` |

Two patterns are worth stealing and nothing else is: **delta's two-column gutter** (the only
format that answers "which side is this number" without a legend) and **prr/GitHub's
comment-directly-under-the-line**.

## 2. Mockups — 120x36

Marker column is today's `mark_rows` output (`●` pinned, `2..9`/`+` counts, `↺` delivered
earlier, blank).

### (a) Minimal — keep list-over-preview, fix legibility

```
 ↵ pin  ⇥ mark  ^E edit  ^X del  ^L list  ^R refresh  ^B base  ^S deliver  ^A approve  esc leave  F1 help
review> ▏                                                                                    ● 2 ↺ 1  3/5
─ calc.py ──────────────────────────────────────────────────────────────────────────────── +4 −0 · churn 4 ─
        2 │ + """add two numbers"""
    ●   7 │ +
>       8 │ + def mul(a, b):
    ↺   9 │ +     return a * b
─ new.py ───────────────────────────────────────────────────────────────────────────────── +1 −0 · churn 1 ─
        1 │ + print("hi")




─ calc.py · uncommitted ─────────────────────────────────────────────────────────────────────── alt-u/alt-d ─
    1    1 │   def add(a, b):
    ·    2 │ + """add two numbers"""
    2    3 │   return a + b
    3    4 │
    ·    5 │ +
    ·    6 │ + def mul(a, b):
 ▶  ·    7 │ +     return a * b
    ·    8 │ +
    ·    9 │ + def mul(a, b):
```

Path moves to a per-file group row (churn is already computed in `lines_mode`), the row becomes
`mark · number · │ · sign · text`, the preview gains delta's two-column gutter plus a `▶` on the
focused line, the 2-line legend collapses to 1 + `F1`, base and counters move to the prompt
line's right and the preview's border label.

fzf needs: `--with-nth=4` (unchanged), `--multi` + `tab:toggle` for `⇥ mark`,
`--preview-window=down,55%,border-top,+{5}-/2` (unchanged), `--border-label`,
`--info=inline-right` (already on). Nothing past 0.44.

### (b) Diff-first — the list IS the numbered diff, comments inline

```
 ↵ pin  ⇥ mark  ^E edit  ^X del  ^N/^P next change  ^R refresh  ^B base  ^S deliver  esc leave  F1 help
review> ▏                                                                                   ● 2 ↺ 1  9/34
 calc.py  +4 −0 ────────────────────────────────────────────────────────────────────────────────────────
    1    1     def add(a, b):
    ·    2  +      """add two numbers"""
             ●  says "add" but the body returns a + b — drop it or say what it returns
    2    3         return a + b
    3    4
    ·    5  +
    ·    6  +  def mul(a, b):
 ▶  ·    7  +      return a * b
             ↺  no test covers mul() — delivered last round, still true
 new.py  +1 −0 ─────────────────────────────────────────────────────────────────────────────────────────
    ·    1  +  print("hi")
```

No preview pane — 33 list rows instead of 14. Context lines become rows, so the changed lines
stop standing out by construction, which is anti-pattern #1's exact risk.

fzf needs: `--multi`, `--track` (0.39, present — the cursor must not jump when a pinned comment
inserts a row and renumbers the list), plus `^N/^P` binds that skip context rows. **fzf cannot
skip rows**; that needs a second row stream or `--nth` filtering, and is the one item here
without a cheap implementation.

### (c) Side — list left, preview right

```
 ↵ pin  ⇥ mark  ^E edit  ^X del  ^L list  ^R refresh  ^B base  ^S deliver  ^A approve  esc  F1 help
review> ▏                                    ● 2  3/5
─ calc.py ────── +4 −0 ─┬─ calc.py · uncommitted ───────────────────────────────────────── alt-u/alt-d ─
      2 │ + """add two nu│     1    1 │   def add(a, b):
  ●   7 │ +              │     ·    2 │ + """add two numbers"""
>     8 │ + def mul(a, b)│     2    3 │   return a + b
  ↺   9 │ +     return a │     3    4 │
─ new.py ─────── +1 −0 ─┤     ·    5 │ +
      1 │ + print("hi")  │     ·    6 │ + def mul(a, b):
                         │  ▶  ·    7 │ +     return a * b
                         │     ·    8 │ +
                         │     ·    9 │ + def mul(a, b):
```

fzf needs `--preview-window=right,66%,border-left,+{5}-/2`; everything else is (a). At 120 cols
the 40-col list truncates fragments hard, and at an 80-col popup it is unusable — so this has to
be width-conditional (`change-preview-window` cycling right/down/hidden on `^/`).

## 3. Recommendation

**(a), with (c) as a `^/` cycle position rather than a separate layout.**

(a) fixes five of the six complaints inside four places: `lines_mode`'s final `printf`
(`scripts/tmux-review`, the `printf "%d\t%09d\t..."` around line 548), `mark_rows` (~line 767),
`--preview-diff` in `scripts/tmux-open-path`, and the `--header` string. Anchoring, the store
grammar, the cap logic and `poff` are all untouched. (b) rewrites all of those and trades the
property that makes the list a work queue — every row is something you have not looked at yet —
for a wall of context. (c) alone breaks at 80 columns.

The preview gutter is one awk pass over the diff `--preview-diff` already prints, reusing the
`@@` parsing `lines_mode` already carries. It must **prefix** lines and never add or remove one,
or every row's `+{5}` preview offset shifts.

### What breaks the "one row per changed line, enter pins the typed text" model

- **Nothing, if `⇥ mark` is additive.** With nothing marked, `enter` still resolves `{1} {2} {3}`
  from the cursor row and `attach` is untouched. With rows marked it must route through a new
  `--attach-range` taking `{+1} {+2}` — plural placeholders expand to *lists*, so `attach`'s
  positional `$2 $3 $4 $5` signature cannot absorb them — and write `## path:first-last (+)`, a
  header revdiff's grammar already parses. Marks spanning two files must be refused, not split.
- **File group rows are selectable.** fzf 0.44 has no non-selectable row. Give the group row
  `path` with line `0` and let `attach` emit `## path (file-level)` — grammar the parser already
  has — rather than filtering it, which is more code for less.
- **`?` cannot be the help key.** With `--disabled` every printable key lands in the query, and
  the query is the comment; binding `?` makes the `??` question convention untypable. Use `F1`
  (or `alt-h`) and print it as `F1 help`.
- **`^E` genuinely cannot edit a pinned comment today.** `edit_comment` (scripts/tmux-review:900)
  seeds its tempfile from `$query` only, never reads the store, and always `attach`es a *new*
  block — the complaint is exact, not perceptual. Fix lives in that function: when the query is
  empty and the row already has a block, seed from it and rewrite that block instead of
  appending. Independent of layout.
- **Both leaks are strings, not layout.** `${REVIEW_PANE_ID:-the agent}` prints `%5` in the `^S`
  confirm (scripts/tmux-review:1141) — resolve it through `tmux display-message -p
  '#{pane_title}'`. On the same line `'edit the batch in $EDITOR first'` is single-quoted
  `printf`, so `$EDITOR` never expands; print the basename of `${VISUAL:-${EDITOR:-vi}}`.
- **Scrolling:** keep `alt-u`/`alt-d` half-page, add `alt-j`/`alt-k` single-line, and put the hint
  in the preview's `--border-label` where the scrolling actually happens, not in the legend.

### fzf 0.44.1 ceilings that shaped this

`transform` is 0.45, `--highlight-line` 0.52, `--tmux` 0.53, `--wrap` 0.54, `--gap` 0.56 — all
absent. So the legend cannot be recomputed per keystroke (fixed per fzf launch, which is fine —
`run_loop` already relaunches on base change), the focused preview line cannot be highlighted
natively (hence passing the row's line into `--preview-diff` and drawing `▶`), rows cannot wrap,
and (b)'s inline comments would have to be real list rows.

## Sources

delta line-numbers + full --help (dandavison.github.io/delta) · tig manual & tigrc(5)
(jonas.github.io/tig) · lazygit Keybindings_en.md + discussion #3460 · difftastic introduction
(difftastic.wilfred.me.uk) · prr (github.com/danobi/prr, dxuuu.xyz/prr.html, dxuuu.xyz/prr2.html)
· crit.md · gh-dash.dev keybindings · github.blog/changelog 2020-02-21 multi-line PR comments,
2026-01-22 improved Files changed · fzf CHANGELOG
