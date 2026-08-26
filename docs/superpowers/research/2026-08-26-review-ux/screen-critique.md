# Review popup readability critique (captures at 120x36 and 80x24)

Fixture: calc.py modified, new.py untracked, lib/util.py with 3 deletions,
2 changed lines and one 200-char line. Ranked most harmful first.

1. **Legend is dense and truncates at 80 columns.** `esc leave ..` and
   `alt-u/alt-d..` are cut mid-word; at 120 columns twelve unrelated keys run
   together. Fix: group by purpose, drop the rarest group first at narrow
   width.
2. **`path:line (+)` is not column-aligned.** The sign lands in a different
   column on every row once paths differ in length; scanning a column of
   signs does not work. fleet_queue already pads its columns. Fix in
   mark_rows: pad `path:line`, show the sign once, coloured.
3. **Rows below the fold have no hint.** `13/13` in the info line but 12
   rows visible with the preview open; the last file's only change sat off
   screen. The scrollbar would show it but is painted black (item 4).
4. **Preview divider and scrollbar render in black** (`\033[30m`), invisible
   on a dark background. Fix: `--color=...,border:8,scrollbar:8` or drop the
   explicit colour.
5. **Marker glyphs are unexplained and glued to the path.** `●lib/util.py`,
   `2lib/util.py:2`. Nothing in the legend says what `●`, a digit, `+` or `↺`
   mean. Fix: a space after the marker and one legend fragment.
6. **Preview has no line numbers and no anchor for the cursor row.** Matching
   `lib/util.py:1` to the hunk means counting from `@@`. Fix: a two-column
   old/new gutter and a `▶` on the row's own line.
7. **`^X` on a row with nothing pinned gives no feedback.** Screen is byte
   identical before and after. Fix: a `display-message`.
8. **Confirm picker leaks strings.** `edit the batch in $EDITOR first` is the
   literal, unexpanded text; `deliver 2 comment(s) to %5` shows a raw pane
   id. Fix: print the editor's basename and the pane's title.
9. **Ragged right edge at 80 columns.** Some rows reach the border, some do
   not, because the fixed display budget ignores the terminal width.
10. **`base: uncommitted` is unexplained.** Show the next `^B` target inline,
    e.g. `base: uncommitted (^B → branch)`.

Not a bug: `alt-d` did nothing because the fixture's diff fit the preview.
`^E` skipped (known broken: the editor writes to fzf's captured stdout).
