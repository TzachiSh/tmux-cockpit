# Board and panel: combined design

Design for the next version of the board and the panel. Nothing here is
implemented. It reconciles two drafts, a tablet one and a PC one, against an
audit of what ships today. The three are kept as appendices:

- [audit.md](audit.md): the audit of the current board and panel, with 25 live captures.
- [tablet.md](tablet.md): the tablet portrait draft, mockups at 60 and 40 columns.
- [pc.md](pc.md): the PC draft, mockups at 100 and 180 columns.
- [captures/](captures/): the raw captures pasted in section 2.

The appendices are verbatim. Where they mention `gen.py`, `gen_tablet.py` or
`feed.sh` "next to this file", those scripts were not copied. Where an appendix
and this document disagree, this document wins. Section 0 lists what
revision 2 changed and why.

## Contents

0. [Revision 2: practices and changes](#0-revision-2-practices-and-changes)
1. [Scope and assumptions](#1-scope-and-assumptions)
2. [Current state](#2-current-state)
3. [Audit summary](#3-audit-summary)
4. [Decisions](#4-decisions)
5. [User flows](#5-user-flows)
6. [Mockups](#6-mockups)
7. [Key map](#7-key-map)
8. [Contract changes for #61](#8-contract-changes-for-61)
9. [Trade-offs and open questions](#9-trade-offs-and-open-questions)
10. [Implementation plan](#10-implementation-plan)
11. [Research: how terminal apps do it](#11-research-how-terminal-apps-do-it)

## 0. Revision 2: practices and changes

Revision 1 settled the structure: groups, folding, the quick answer and the
return trip. Revision 2 checks it against how well-known terminal apps handle
colour, keys, focus, feedback, and empty and error states. The findings and
their sources are in [section 11](#11-research-how-terminal-apps-do-it). From
them come ten practices (0.1) and the changes listed in 0.2.

Each change is marked ⟦rev2: P#⟧ where it lands, with the practice that drove
it. No ruling and no frozen rule in `docs/tui.md` is reversed. Two changes
touch a ruling (C8, C11) and one needs a word with session tmux-cockpit-79
(C13); all three are flagged **needs OK** in 0.2. Everything else either
follows a ruling the mockups had not caught up with, or adds something the
rulings left open.

One finding does not carry over. Most apps in section 11 draw with the
terminal's 16 colours so the user's theme decides. This repo reaches the same
goal another way, under a frozen rule: it emits truecolor from the `@thm_*`
tmux options, so the user's tmux theme decides, and tmux downgrades the colour
per client (`docs/tui.md`, "Emit truecolor unconditionally"). Revision 2 keeps
that rule.

The switch at the top of this page redraws every mockup with the shipped
roles (D17) in Catppuccin Mocha, the dark default, in Catppuccin Latte, a
light `@thm_*` set, or under `NO_COLOR`. Every screen should read in all
three. Latte shows why D17 needs its last rule: `@thm_overlay_0` grey on a
light background is hard to read.

**Approved, with an extension (2026-11-02).** The owner approved revision 2.
Epic #228 replaces the shell screens (`prefix i`, `prefix r`'s fleet, the fzf
pickers) with the board. This document's steps 1-4 and 8 are built by the
panel lane; #228 adds the info overlay, fleet columns, a session view, a branch
popup and a Rust review screen on top
([#228](https://github.com/TzachiSh/tmux-cockpit/issues/228)). D7, 9.1 and
open question 12 were updated for it.

### 0.1 Practices for the board and the panel

| # | practice | from (section 11) | rev 1 already |
|---|---|---|---|
| P1 | Let the user's theme pick the colours, through named roles in one theme file. Never a colour from outside it. Here the theme is the `@thm_*` tmux options. | lazygit, helix, k9s, Textual's ANSI themes | yes: `theme.rs` roles |
| P2 | Never let colour carry a state alone. Every state has a glyph or a word, and the screens must work under `NO_COLOR`, with the selection still visible. | WCAG 1.4.1, no-color.org, lazygit's selected-line options | glyphs yes; the `NO_COLOR` selection was not drawn |
| P3 | Nothing a user must read relies on SGR dim alone, or on a grey with too little contrast. Dim may add to a glyph or a word, never replace one. | Auggie #169, Windows Terminal #20109, yazi's docs | no: D10 dims the held list |
| P4 | One key bar per screen, for whatever has focus, built from the same table as `?`. Hide keys that do nothing here. | Textual Footer, Bubbles `help`, k9s, aerc | D14 says so; the mockups still showed `? help` and border hints |
| P5 | Hold the selection by identity. A refresh never moves the row a key is about to act on. | lazygit #4116, tig's NEWS | D5 yes; nothing said about the sheet and the confirm |
| P6 | Confirm only what cannot be undone, and let the confirm own the keyboard while it is open. | clig.dev, k9s delete vs kill, lazygit PR #4284, lazygit #5966 | D8 yes; key ownership unstated |
| P7 | Answer within 100 ms. Successes may fade; a failure stays until dismissed and says how to fix it. (Putting the fix last is our inference from lazygit #4372, where the useful line sat out of reach at the bottom.) | clig.dev, Textual toasts, ncdu, lazygit #4372 | D9 fades every note after 5 s |
| P8 | An empty screen says what to do next and names the key. | clig.dev (the only source; see 11.3) | the empty board yes; find with no match not drawn |
| P9 | Both key families, plain keys first. `q` and `^C` always exit, as in tmux's choosers. Avoid keys tmux or the terminal can swallow. | lazygit, yazi, ncdu, tmux(1), tmux FAQ, htop #1047 | mostly; Compose's `F1` is a gap |
| P10 | Show the jump key and the position where they act: digits on rows, `N-M of T` on the list. | lazygit `showPanelJumps`, `showListFooter` | D6 yes; the 60-column mockups numbered past 9 |

### 0.2 What changed

| # | change | why | where | needs OK |
|---|---|---|---|---|
| C1 | The page draws mockups as a terminal would: the shipped role colours on the state glyphs, a band on the selected row, the accent on the focused frame, and a Mocha/Latte/`NO_COLOR` switch. | P1, P2, P3: a palette is checked on a dark theme, a light one and without colour, not one | this page | |
| C2 | New D17: writes down today's role per state from `theme.rs`, how each reads without colour, and two rules: focus never shares a hue with a state, and must-read text never sits in a low-contrast grey or in dim alone. | P1, P2, P3 | 4, D17 | |
| C3 | New D18: while the answer sheet or its confirm is open, the list holds still and the confirm owns the keyboard. A changed prompt or a gone pane closes the confirm with a note and never retargets it. | P5, P6: lazygit #4116 is the same class of bug, a fetch moving the selection a delete then hit | 4, D18; 6.4 | |
| C4 | New D19: one key bar on the board. On pc the board's frame borders lose their hints (`┗ ↵ answer ━━`), and the footer follows the focus: fold keys on a header, pick keys in the sheet, `y`/`n` in the confirm. The panel keeps its border hints for now (D10 freezes its 40/60/80/120 goldens). | P4: Textual and Bubbles build the footer from the live key table; two bars can disagree | 4, D19; the pc board mockups in 6 | |
| C5 | `? help` becomes `? keys` in every pc mockup in section 6. | P4; D14 already ruled this, and the mockups predated it | 6 | |
| C6 | The 60-column mockups number only the first nine rows on screen, and find renumbers its matches. | P10; D6 already ruled this, and closes the gap listed under section 6 | 6.1, 6.2, 6.3 | |
| C7 | The `—` on idle rows becomes the word `idle`. | P2: a dash says nothing a `NO_COLOR` user can use | 6 | |
| C8 | New D20: a return-trip note (D9) that reports a failure or a refusal stays until the next key; success notes keep D9's 5 s; the 2-second notes of D8 and D12 do not change. Section 8's `--note` and loop lines follow. New mockup of a refused quick answer. | P7: Textual keeps timed toasts for brief supplementary news, not failures | 4, D20; 6.5; 8 | yes, D9's "5 s" and section 8 |
| C9 | New D21: find with no match is a designed screen at 60 and 100 columns. It keeps the field, says what find reads, and names `⌫`, `^U` and `esc`. | P8 | 4, D21; 6.3 | |
| C10 | The pc quick-answer sheet and confirm at 100 columns, the gap section 6 listed. The confirm names the agent, what gets typed, the re-check and where you land; `↵` is off. | P6, P7 | 6.4 | |
| C11 | D10's held list is no longer "dimmed". It keeps its frame in the plain frame colour and reads `sessions · held`, while the docked panel's frame takes the focus colour. | P3: Windows Terminal draws dim like normal text and mosh at full brightness, so "dimmed" may show nothing | 4, D10 | yes, a wording change in a ruling |
| C12 | New D22: every action has a plain-letter or `^` key, no `Alt` chords, and `esc` is never the only way out. The sheet gains `n` back. Compose's `F1` help becomes open question 18. | P9 | 4, D22; 6.4; 7.1; 9.2 | |
| C13 | Two new open questions: the `F1` in Compose (18), and who owns the palette values (19). | P1, P9 | 9.2 | yes, 19 is a cross-session split |
| C14 | New section 11: the research, with a source per point, each marked evidence or opinion. | | 11 | |
| C15 | Trade-offs for D18 and D20 in 9.1, and the steps in section 10 that build D17 to D22. | | 9.1, 10 | |

## 1. Scope and assumptions

Two screens of `cockpit-tui`:

- **The board.** `prefix w`, which runs `cockpit-tui board` through
  `scripts/cockpit-board` in a popup. Today it is a grid of live tiles, one per
  agent pane, blocked first.
- **The panel.** `prefix f` on a pane, or `f` on the board, which runs
  `cockpit-tui panel %N` through `scripts/cockpit-panel`. Claude writes, fixes
  or explains for that pane, and the wrapper pastes the answer in.

Two targets:

| target | screen | input | the board's area |
|---|---|---|---|
| tablet portrait | Termux or similar, often over SSH | touch plus a soft keyboard | 40 to 60 columns, 12 to 60 rows (12 with the keyboard up) |
| PC | a desktop terminal | keyboard first | 100 to 200 columns |

`tablet` and `pc` are the values of `@cockpit_panel_mode`. Today only the panel
reads it; under this design the board reads it too (D13).

Assumptions:

- Design only. No code, test or doc outside `docs/design/` changes with this
  document.
- The frozen contracts in `docs/tui.md` ("Rules") still hold unless section 8
  names the change. Each change there needs a comment on #61.
- The hard rules hold: actions are printed and a shell wrapper runs them, no
  terminal queries, tests on private `-L` sockets, no clock in a sort key.
- Glyphs are today's (`▲ ✕ ● ✓ ○ ◆`), plus `▾`/`▸` for open and folded groups.

## 2. Current state

The real output of master, captured on a private server: 6 fake agents in
sessions api, web and infra, with empty previews because they run `sleep`.
The files are in [captures/](captures/).

### 2.1 The board

Board at 60 columns (`captures/board-60.txt`):

```
 board · 6 agents · 2 needs you               1-5 of 6   ▼1
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃Do you want to run npm test?                              ┃
┃                                                          ┃
┃                                                          ┃
┃                                                          ┃
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ 2 ▲  web:2 · docs ───────────────────────────────────────┐
│plan ready: approve?                                      │
│                                                          │
│                                                          │
└ ↵ answer ────────────────────────────────────────────────┘
┌ 3 ✕  infra:0 · terraform ────────────────────────────────┐
│—                                                         │
│                                                          │
│                                                          │
│                                                          │
└──────────────────────────────────────────────────────────┘
┌ 4 ●  api:1 · tests ──────────────────────────────────────┐
│—                                                         │
│                                                          │
│                                                          │
└──────────────────────────────────────────────────────────┘
┌ 5 ●  web:0 · ui ─────────────────────────────────────────┐
│—                                                         │
│                                                          │
│                                                          │
│                                                          │
└──────────────────────────────────────────────────────────┘
 ↵ jump   ? help   q close
```

Board at 180 columns (`captures/board-180.txt`):

```
 board · 6 agents · 2 needs you
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓┌ 2 ▲  web:2 · docs ───────────────────────────────────────┐┌ 3 ✕  infra:0 · terraform ────────────────────────────────┐
┃Do you want to run npm test?                              ┃│plan ready: approve?                                      ││—                                                         │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┃                                                          ┃│                                                          ││                                                          │
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└ ↵ answer ────────────────────────────────────────────────┘└──────────────────────────────────────────────────────────┘
┌ 4 ●  api:1 · tests ──────────────────────────────────────┐┌ 5 ●  web:0 · ui ─────────────────────────────────────────┐┌ 6 ✓  web:1 · checkout ───────────────────────────────────┐
│—                                                         ││—                                                         ││—                                                         │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
│                                                          ││                                                          ││                                                          │
└──────────────────────────────────────────────────────────┘└──────────────────────────────────────────────────────────┘└──────────────────────────────────────────────────────────┘
 ↵ jump   a ask   r review   f panel   i info   ? help   q close
```

The board's `?` at 60 columns (`captures/board-60-help.txt`):

```
 board · 6 agents · 2 needs you               1-5 of 6   ▼1
┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃Do you want to run npm test?                              ┃
┃                                                          ┃
┃                                                          ┃
┃                                                          ┃
┗ ↵ answer ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
┌ 2 ▲  web:2 · docs ───────────────────────────────────────┐
│plan ready: approve?                                      │
╭ keys ────────────────────────────────────────────────────╮
│ move                       act                           │
│ h l  ← →  column           ↵   jump to this pane, close  │
│ j k  ↑ ↓  row              a   answer / ask this agent   │
│ ⇥ ⇧⇥      next / previous  r   review its diff           │
│ 1-9       tile by number   f   panel for this pane       │
│ g G       first / last     i   info for this pane        │
│ u         next blocked                                   │
│ [ ]       page             view                          │
│                            ?   this help                 │
│                            ^L  refresh now               │
│                            q   close   ·   esc close ove │
╰ any key closes ──────────────────────────────────────────╯
└──────────────────────────────────────────────────────────┘
┌ 5 ●  web:0 · ui ─────────────────────────────────────────┐
│—                                                         │
│                                                          │
│                                                          │
│                                                          │
└──────────────────────────────────────────────────────────┘
 ↵ jump   ? help   q close
```

### 2.2 The panel, pc mode

Home at 60 (`captures/panel-pc-60-home.txt`):

```
╭ panel · %0 · server ─────────────────────────────────────╮
│ ▎1 ★ Write a prompt    a task for Claude Code from your… │
│  2   Compose           type once: fix spelling, write, … │
│  3   Explain           what is on this screen, and what… │
│  4   Tablet mode       big tiles, tap buttons, the mouse │
│  5   Close                                               │
│                                                          │
│ server · claude · /tmp                                   │
│ will send: screen 4 lines, 0 hidden                      │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
╰ 1-5 pick  ? keys  esc close ─────────────────────────────╯
```

Home at 180 (`captures/panel-pc-180-home.txt`):

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ ▎1 ★ Write a prompt    a task for Claude Code from your rough words                                                                                                              │
│  2   Compose           type once: fix spelling, write, or insert                                                                                                                 │
│  3   Explain           what is on this screen, and what next                                                                                                                     │
│  4   Tablet mode       big tiles, tap buttons, the mouse                                                                                                                         │
│  5   Close                                                                                                                                                                       │
│                                                                                                                                                                                  │
│ server · claude · /tmp                                                                                                                                                           │
│ will send: screen 4 lines, 0 hidden                                                                                                                                              │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
╰ 1-5 pick  ? keys  esc close ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

Compose at 60 (`captures/panel-pc-60-compose.txt`):

```
╭ panel · %0 · server ─────────────────────────────────────╮
│ type: auto   model: auto                                 │
│ make the flaky test in b.test.ts deterministic▏          │
│                                                          │
│ → haiku · short request - the fast model is enough       │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
╰ ^S write  ^F fix spelling  F1 keys  esc home ────────────╯
```

Compose at 180 (`captures/panel-pc-180-compose.txt`):

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ type: auto   model: auto                                                                                                                                                         │
│ make the flaky test in b.test.ts deterministic▏                                                                                                                                  │
│                                                                                                                                                                                  │
│ → haiku · short request - the fast model is enough                                                                                                                               │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
╰ ^S write  ^F fix spelling  ^T insert as typed  ⇥ type  ^N model  F1 keys  esc home ──────────────────────────────────────────────────────────────────────────────────────────────╯
```

`?` at 60 (`captures/panel-pc-60-help.txt`):

```
╭ keys ────────────────────────────────────────────────────╮
│ ^S F2     write it with Claude                           │
│ ^F        fix the spelling                               │
│ ^T        insert as typed                                │
│ ⇥         next output type                               │
│ ^N        next model                                     │
│ ↵         new line                                       │
│ ⌫ ^W ^U   delete a char, a word, all before              │
│ ← →       move; ^← ^→ by a word                          │
│ Home End  line start, end; ^A ^E too                     │
│ Del       delete the char after                          │
│ F1        this help                                      │
│ esc       back to home                                   │
│ ^C        close the panel                                │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
╰ any key closes ──────────────────────────────────────────╯
```

`?` at 180 (`captures/panel-pc-180-help.txt`):

```
╭ keys ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ ^S F2     write it with Claude                                                                                                                                                   │
│ ^F        fix the spelling                                                                                                                                                       │
│ ^T        insert as typed                                                                                                                                                        │
│ ⇥         next output type                                                                                                                                                       │
│ ^N        next model                                                                                                                                                             │
│ ↵         new line                                                                                                                                                               │
│ ⌫ ^W ^U   delete a char, a word, all before                                                                                                                                      │
│ ← →       move; ^← ^→ by a word                                                                                                                                                  │
│ Home End  line start, end; ^A ^E too                                                                                                                                             │
│ Del       delete the char after                                                                                                                                                  │
│ F1        this help                                                                                                                                                              │
│ esc       back to home                                                                                                                                                           │
│ ^C        close the panel                                                                                                                                                        │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
╰ any key closes ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

### 2.3 The panel, tablet mode

Home at 60 (`captures/panel-tablet-60-home.txt`):

```
╭ panel · %0 · server ─────────────────────────────────────╮
│ ╭──────────────────────────────────────────────────────╮ │
│ │ ▎1 ★ Write a prompt    a task for Claude Code from … │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  2   Compose           type once: fix spelling, wri… │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  3   Explain           what is on this screen, and … │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  4   PC mode           keys only: the terminal sele… │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  5   Close                                           │ │
│ ╰──────────────────────────────────────────────────────╯ │
│                                                          │
│ server · claude · /tmp                                   │
│ will send: screen 4 lines, 0 hidden                      │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ keys ] [ close ]                                       │
╰──────────────────────────────────────────────────────────╯
```

Home at 180 (`captures/panel-tablet-180-home.txt`):

```
╭ panel · %0 · server ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │ ▎1 ★ Write a prompt    a task for Claude Code from your rough words                                                                                                          │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │  2   Compose           type once: fix spelling, write, or insert                                                                                                             │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │  3   Explain           what is on this screen, and what next                                                                                                                 │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │  4   PC mode           keys only: the terminal selects text again                                                                                                            │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮ │
│ │  5   Close                                                                                                                                                                   │ │
│ ╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯ │
│                                                                                                                                                                                  │
│ server · claude · /tmp                                                                                                                                                           │
│ will send: screen 4 lines, 0 hidden                                                                                                                                              │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│ [ keys ] [ close ]                                                                                                                                                               │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

## 3. Audit summary

The detail, the file:line references and the other captures are in
[audit.md](audit.md). "Fix" names the decision in section 4 (`D1`-`D16`) that
answers the problem.

### 3.1 The board

| screen | keep | problem | fix (design §) |
|---|---|---|---|
| grid | whole tiles only, never half cut; the 48-column minimum tile; blocked tiles stand out (red border, `▲`, `↵ answer`) | three equal columns at 180 leave about 60% of the cells blank; height is shared equally whatever the content; 1 to 3 columns with nothing in between; the worker under-fetches the preview by one row | D1 two panes at ≥96, D2 tiles kept behind `v` |
| header | `needs you` in the needs colour; the `N-M of T ▲ ▼` indicator | no per-state or per-session counts; the right indicator overlaps the left text when narrow; the documented `· stale 34s` header and card are not implemented | D14 the panel's `titles()` rule, D16 stale header and card |
| tile | the `(state, pane id)` order; the 5-column rail; `trunc_middle` on `doing` | a reserved `said` row is blank on every non-blocked tile; `—` takes a whole row; the question shows twice and its options are cut; only blocked tiles carry an action hint; `▎` means tmux's active pane here and the selection in the panel | D1 one row per agent plus one preview, D8 options on the sheet, D14 `▎` is the selection |
| preview | ANSI kept, no wrap, the 64 KB cap, `capture failed` vs empty | top-aligned, so short panes leave the bottom empty; long lines clip with no marker; a wide TUI shows only its left half | D1 one large preview of the selection (116 columns at 180) |
| footer | hints drop whole, never cut | at 60 columns it falls from 7 hints to 3 and `a r f i` vanish; it always says `a ask`, even where `a` jumps; separators and verbs differ from the panel's | D14 one priority footer, state-aware |
| help | one table for dispatch and help; the grouping; any key closes it | no narrow layout and no scroll (cut mid-word at 40); a card over the live grid; `q close · esc close overlay` misleads; no legend for the glyphs | D14 full-popup help, one column, scrolls |
| empty | the text | the card sits at the top, not the centre; its advice is cut at 40 columns, not wrapped | D16 centred and wrapped |
| floors | none worth keeping | "terminal too narrow" also at 100x4, where the width is fine; the text differs from the doc; no remedy key; a 5-row floor is not a usable floor | D14 one floor card; D13 tablet floor 28x12; pc `too short` |
| narrow portrait | one column; blocked tiles stay first | one agent per 5-8 rows, so a phone sees 6 agents; the blocked question loses its end at 1 header row; no list mode; no touch | D1 list with a card, D13 tablet mode |
| scroll | `follow()` keeps the selection visible | `]` moves the selection a whole page; the last page is mostly blank; digits stop at 9 | D6 digits on screen, D7 find, `]` scrolls and the selection only clamps |
| grouping, search | the "no clock in the sort" rule | no `/`, no session jump, sessions interleaved | D3, D4, D7 |
| acting | `↵` jumps | `a` only jumps or prints a note; `deliver` is a stub; every action but `a` ends the board | D8 quick answer, D9 return trip |

### 3.2 The panel

| screen | keep | problem | fix (design §) |
|---|---|---|---|
| Home | numbered tiles with descriptions; `★` on the recommended one; `will send:`; tall tablet tiles that fall back to 1 row | does not know the agent is blocked; names it `%0 · server`, not the board's `▲ api:0 · server`; at 180 everything sits in the left 70 columns; the cwd line is cut mid-path | D11 agent context and the Answer tile, D10 docked layouts |
| Compose | the live model route; the 1-cell `▏` cursor; the footer that drops `^N model` | `?` is text, so help is `F1`; the input has no frame at 100 and 180; tablet buttons wrap unevenly; 3 rows of buttons at 40 with the keyboard up | D10 framed draft at ≥110; tablet `[ more ]` (tablet.md 5.1 P5) |
| Busy | spinner, verb, model, elapsed, "times out at", "still working…" after 30 s | shows nothing of the pane, which may have moved on | not addressed; see 9.2 |
| Question | `N-M of T` in the border; the selection stays on screen | `g` is best guess here and first tile on the board | D12 `^G` |
| Result | footers drop whole and keep `esc` and `?` | most of the width blank at 120+; `r` retries a paid call one keypress from the board's `r` review | D10 two columns at ≥110, D12 `^R` |
| Steps | `✓ inserted` after a resume | `○ step 1 inserted` uses the board's idle glyph | not addressed; see 9.2 |
| Answer | the armed `esc` | no pc golden for the armed state; width unused at 120+ | D10 answer beside the screen it read |
| Confirm | `↵ is off: risky or multi-line`; the tablet yes/no pair at the top | does not name the agent or where you land | tablet.md 5.1 P6, kept under D11 |
| Error | names the fix ("run claude … /login, then r retries") | `esc back` does not say back to where | D12 updates the retry text to `^R` |
| help | full-popup, one column, scrolls, `esc` rows name their destination | on Home 11 of 14 rows are blank | D14 makes it the shared help for both screens |
| floor card | one "larger or rotate" card | plain text in a big empty box; no tap way out, by design | D14 shared with the board |

### 3.3 The handoff

| | keep | problem | fix (design §) |
|---|---|---|---|
| board → panel | one popup per client, the panel runs inside the board's | the popup keeps the board's title, which names the client's session; the board's size, not the panel's; tile number, session, state and question are lost; `COCKPIT_PANEL_MODE` is not passed, so tablet mode is lost | D10, D11, D13 |
| panel → board | none | no way back; after `f`, `r` or `i` the popup closes and `prefix w` starts over with the selection reset | D9 |
| keys across screens | `esc` names where it goes in the panel | `r`, `i`, `f`, `g` mean different things one keypress apart; help is `? help` on one and `? keys` on the other; `▎` means two things | D12, D14 |

### 3.4 The ranked top 10

From [audit.md](audit.md) Part 4, with where this design answers each.

| # | improvement | answered by |
|---|---|---|
| 1 | return to the board after `f`, `r` and `i`, selection restored | D9 |
| 2 | search, filter and session grouping; digits past 9 | D3, D4, D6, D7 |
| 3 | carry the agent's context into the panel; pass `COCKPIT_PANEL_MODE` | D11, D13 |
| 4 | reclaim dead rows in tiles; show a blocked agent's options | D1 (one row per agent), D8 (the options sheet). The tiles behind `v` keep today's rows; see 9.2 |
| 5 | a list for narrow and portrait | D1 |
| 6 | use the space at 180+ | D1 two panes, D10 docked panel |
| 7 | the panel's chrome on the board | D14 |
| 8 | a tablet mode for the board | D13 |
| 9 | a per-agent action menu and a real `deliver` | D8; the tablet card's buttons (tablet.md 4.5). No chooser on `↵`: every action has a key in the footer |
| 10 | state screens, doc drift, key clashes | D12, D14, D16 |

## 4. Decisions

Each decision says where it came from. **Ruling** means the drafts disagreed
and the owner decided; the losing option is named so nobody reopens it by
accident. **Both** means the drafts agree. **Reconciled** means the drafts
differ and this document picks, with the reason.

**D1. Layout by mode and width.** *Ruling.*
- pc mode, board area ≥ 96 columns: two panes. A grouped list on the left,
  `clamp(w/3 + 8, 40, 64)` wide (41 at 100, 64 at 180), and one large live
  preview of the selection on the right (pc.md 0, 1).
- pc mode, under 96 columns: the list alone. `↵` jumps, `f` opens the panel
  full-area.
- tablet mode: the list, with the selected agent opened into a card in place
  (thick border, preview, a row of buttons). Every other agent is one row
  (tablet.md 1, 3).

**D2. Tiles stay, behind `v`.** *Ruling.* `v` toggles tiles and list in either
mode. pc.md dropped tiles (pc.md 9.1); tablet.md kept them as the pc default.
Neither is the outcome: the list is the default in both modes and today's grid
is one key away. In the tiles layout every key keeps today's meaning, and `?`
is generated for the layout on screen.

**D3. Groups and order.** *Ruling, tablet.md's two bands.*
1. One group per tmux session, header `▾ api ──── 4 · ▲1 ●1 ✓1 ○1`.
2. Sessions holding a `▲` or `✕` agent come first, then the rest, each band by
   session name. A group moves only when it starts or stops needing you.
   pc.md's "sort by most urgent agent" is dropped: it reshuffles groups every
   time an agent goes from running to done.
3. Inside a group, `(state rank, pane id)`, today's key. No clock.

**D4. Folding.** *Ruling on the keys; reconciled on what a folded group shows.*
- Keys: `h` folds the selected group, `l` unfolds it, `space` toggles it, `-`
  folds all, `=` unfolds all. tablet.md's `z`, `c` and `e` are dropped. `↵` on
  a header toggles too.
- A folded group shows its header with per-state counts plus **every** `▲` and
  `✕` row it holds. tablet.md showed only the single most urgent row; pc.md's
  rule wins because it keeps the promise "folding never hides an agent waiting
  on you", and `u` then never has to unfold anything.
- Nothing unfolds by itself (tablet.md 3.5): a new session appears unfolded, a
  folded session that gains a `▲` stays folded and shows the row. A digit,
  `u`, or a pick from find that lands in a folded group unfolds it.
- Folds persist in `board.state` (D9).

**D5. Selection.** *Both.* The selection is a pane id, or a session name on a
header, never an index. On refresh it is looked up by id and its screen row is
held where it fits, so the viewport moves and the cursor does not. A pane that
is gone: the row now at its position in the same session, else that session's
header, else the first row. Folding the selected agent's group moves the
selection to the header, unless the agent is `▲`/`✕` and stays visible. On
open: the selection passed in (D9), else the first `▲` agent, else the
client's pane, else the first row.

**D6. Digits and navigation.** *Ruling, pc.md's digits.*
- `1`-`9` select the first nine agent rows on screen, renumbered as the list
  scrolls. Selects, never jumps. tablet.md's numbers over all agents with a
  1-second second digit are dropped (no clock in input handling).
- `/`, `u` (next `▲`), and `n`/`p` (next and previous session header) reach
  the rest. Taking `n`/`p` frees `⇥` to keep today's meaning, next agent in
  reading order, skipping headers; tablet.md's "`⇥` = next header" is dropped.
- `[` `]` `PgUp` `PgDn` scroll a screenful; the selection only clamps to stay
  on screen (audit 1.10).

**D7. Find.** *Both, merged.* `/` opens a field and filters as you type over
session, window index and name, title, the state word (`needs`, `error`,
`running`, `done`, `idle`), and the message or doing line. Space-separated
terms must all match (tablet.md 4.4). Groups with no match hide; groups with a
match show unfolded for the find, their folds kept. In the field `↑↓` move,
`↵` keeps the filter, `esc` clears it, `q` types, `^C` closes the board.
tablet mode adds state chips that type their word, and `[ keep ]`/`[ clear ]`.
The filter persists in `board.state`. It stays a substring narrowing: fuzzy
ranking and multi-select are not designed. `prefix r`'s fleet goes away under
epic #228, so the board is the one fleet view; acting on several agents is one
at a time through the D9 loop. ⟦rev2: #228⟧

**D8. Quick answer.** *Ruling.*
1. In both modes, `a` on a `▲` agent reads the numbered options from the
   capture (`❯ 1. Yes   2. ...`) and shows them as a sheet.
2. A digit, a tap or `↵` on an option opens a confirm. `y` sends, or the big
   `yes` at the top in tablet mode. `n` and `esc` go back. `↵` never sends.
3. The board writes `deliver %N` with the digit in `$COCKPIT_ACTION.text` and
   the prompt it parsed beside it (section 8). Before typing the digit the
   wrapper captures the pane again and checks it still shows the same prompt;
   if not, it types nothing and says why.
4. With no options found, the sheet says so and offers `f` panel and `↵` jump
   (tablet.md 4.6). `f` on the sheet opens the panel either way.
- Dropped: pc.md's armed preview that sends on the digit with no confirm, its
  `answer %N <digit>` verb (the reserved `deliver` slot is used instead), and
  its `o` key (use `f`). `a` on an agent that is not `▲` keeps today's 2-second
  note.

**D9. The return trip.** *Ruling.*
- `scripts/cockpit-board` becomes a loop, the way `scripts/cockpit-panel`'s
  steps loop already runs. After `review`, `info`, `panel` and `deliver` it
  writes the wrapper's last line to a note file and runs the board again with
  `--select %N --note <file>`. After `jump`, `q`, or an exit with no action the
  loop ends.
- Folds, the kept filter and the last selection persist in
  `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/board.state`, so both the
  reopened board and the next `prefix w` look the same. A file, not a tmux
  option: no write to the server.
- The reopened board draws its first frame from `board.state` before the first
  fetch, marked `· stale 0s` until the fetch lands, so the trip never flashes
  a loading card.
- The note shows for 5 s in the top border (tablet) or the footer row (pc).
  A failure note stays until a key (D20). ⟦rev2: P7⟧
  Its texts are pc.md 5.3.
- tablet.md's `--resume <state> --last <outcome>` and `COCKPIT_RETURN`, and
  pc.md's `COCKPIT_BOARD_SELECT`/`COCKPIT_BOARD_NOTE`, are replaced by the two
  flags and the one state file.

**D10. Where the panel is drawn.** *Ruling.*

| opened | mode, board area | the panel |
|---|---|---|
| `f` on the board | pc, ≥ 164 columns | docked in the right pane; the list stays on the left, held, under a thin frame titled `sessions · held`, never SGR-dimmed ⟦rev2: P3⟧ |
| `f` on the board | pc, under 164 | full-area, replacing the board, with a crumb `board ▸ web:2 docs ▸ panel` on row 0 |
| `f` on the board | tablet | replaces the board, in tablet mode; never docks |
| `prefix f` | any | standalone, as today |

Docked, the list is held: a draft has exactly one target. `esc` on Home gives
the keys back to the list in-process. At ≥ 110 columns the panel's views use
two columns (pc.md 10.2): Home shows what Claude will see, Compose frames the
draft, Result, Steps, Confirm and Answer put the agent's screen beside the
text. The 40/60/80/120 goldens do not change.

**D11. The panel knows its agent.** *Ruling on the tile and the Close label;
reconciled on the rest.*
- Title: the board's name, `panel · ▲ api:0 · server` (tablet.md P1, pc.md
  10.2). The pane id moves to the meta line.
- On a `▲` agent, Home's first tile is `▲ Answer: <the question>`. It opens the
  D8 sheet when options are found, else Compose with the question as context
  (pc.md). The other tiles move down one digit.
- Opened from the board, Close becomes `Back to board`, and `esc` on Home is
  named `board`.
- The Tablet mode tile is hidden when docked, since the docked panel shares the
  board's process and mode. It stays everywhere else.
- Confirm names the agent and where you land, `send this to api:0 · server`,
  `after it lands: back to the board` (tablet.md P6).
- Context reaches the panel as `COCKPIT_PANEL_FROM=board` and
  `COCKPIT_PANEL_CTX=<state>\t<sess:idx>\t<title>\t<msg>` (pc.md's names, which
  match the existing `COCKPIT_PANEL_*` variables; tablet.md's `COCKPIT_FROM`
  and `COCKPIT_AGENT` are dropped). Opened with `prefix f`, the panel reads the
  same fields itself from `@panestate` and `@panestate_msg`.

**D12. Panel keys that clash with the board.** *Ruling.* Retry `r` becomes
`^R`; best guess `g` becomes `^G`. A bare `r` or `g` in the panel shows a
2-second note naming the new key. `i` (insert) and `f` (follow up) keep their
panel meanings: neither sends an Enter or spends a call by itself, and the
frame title names the screen (tablet.md 10.1, pc.md 12.2).

**D13. Tablet mode on the board.** *Ruling on the option; both on the rest.*
- The board reads `@cockpit_panel_mode` through `COCKPIT_PANEL_MODE`, so one
  option drives both screens. The board's popup now passes it, as the panel's
  binding already does.
- In tablet mode the board captures the mouse with the panel's rules: a tap is
  a bare left press, the wheel scrolls, every tap runs a key through the same
  dispatch, a tap within 300 ms of a changed hit map is dropped (800 ms after a
  reopen), what confirms is drawn at the top, buttons replace the footer, and
  the floor is the 28x12 card. Keys keep working.
- pc mode never captures the mouse.

**D14. One chrome for both screens.** *Both, with one pick.*
- One `footer()`: hints in priority order, dropped one at a time, `?` and `q`
  always kept. The `a` label follows the selection (`answer` on `▲`, `ask`
  otherwise); on a header the act hints become `space fold   l open`.
- One `?`: full-popup, one column, scrollable with `↑↓`, generated from the
  key table, with a legend for the glyphs.
- One floor card: `make the window larger or rotate`. pc's list adds a
  `too short` message for under 8 rows instead of today's wrong "too narrow".
- `▎` means the selection on both screens.
- On a text-entry screen (the panel's Compose, the find field, #228's picker)
  every printable key is text. There `esc` and `^C` replace `q`, and `F1`
  replaces `?`, until open question 18 adds a second help key that every
  text-entry screen takes. ⟦rev2: P9⟧
- The help label is `? keys` on both screens. pc.md kept `? help` on the board;
  tablet.md's single label wins because the audit lists the mismatch as a
  problem.
- How tmux's active pane is marked once `▎` is taken differs between the
  drafts (`◂` at the row end in pc.md, an underlined title in tablet.md). Open,
  see 9.2.

**D15. The board from a standalone panel.** *tablet.md only, kept.* In tablet
mode, Home of a panel opened with `prefix f` carries `[ board ]`, which exits
with the new action `board %N` and opens the board with that agent selected.
pc mode has no key for it yet (9.2).

**D16. State screens.** *Both.* The `· stale 34s` header and the "tmux is not
answering" card are implemented as `docs/tui.md` describes. The empty card is
centred and wraps. Loading stays `reading the fleet …`.

**D17. Colour roles.** *New in revision 2. It writes down what `theme.rs`
ships and adds two rules; the frozen colour rules stand.* ⟦rev2: P1 P2 P3⟧
Colours come from the `@thm_*` tmux options, emitted as truecolor and
downgraded by tmux per client, as `docs/tui.md` freezes. The roles below are
the R7 palette from #256 (#222), in `tui/src/theme.rs`. Every option has a
dark value and a `_light` one. Values belong to that file, which session
tmux-cockpit-79 owns (open question 19, settled).

| shows | glyph | `Theme` role | `@thm_*` | without colour (`NO_COLOR`) |
|---|---|---|---|---|
| needs you | `▲` | `needs` | `@thm_needs` | the glyph, bold |
| error | `✕` | `error` | `@thm_error` | the glyph, bold |
| running | `●` | `running` | `@thm_running` | the glyph |
| done | `✓` | `done` | `@thm_done` | the glyph |
| idle | `○` | `meta` | `@thm_dim` | the glyph and the word `idle` |
| review pending | `◆` | `sky` | `@thm_review` | the glyph |
| focus | the focused pane's frame | `accent` | `@thm_accent` | the frame in bold |
| muted text that must be read | | `meta`, `label` | `@thm_dim` (≥ 4.5:1) | plain, or dim beside a glyph or word |
| decoration only | separator dots, borders, rules | `faint` | `@thm_faint` (about 3:1) | the glyphs |
| selection | `▎` and a band | `sel`, added by the panel lane when the list uses it | `@thm_sel` | `▎` plus reverse video |

The rules:
- **Focus never shares a hue with a state.** `accent` (blue) is the only
  frame colour that means focus; the nearest state hue is `review`.
- **Must-read text uses `fg` or `@thm_dim` (`meta`, `label`), both ≥ 4.5:1 on
  both themes.** It never relies on dim alone. Under `NO_COLOR`, dim stays one
  of the three fallback attributes, as frozen, but only on text that also has
  a glyph or a word.
- **`faint` is decoration only:** never the focus frame, never a selected row,
  never the only copy of anything.
- **Selected-row text uses `fg`.** `@thm_dim` on `@thm_sel` is only 3.4/4.0:1.

**D18. The target holds still.** *New in revision 2.* ⟦rev2: P5 P6⟧
- While the quick-answer sheet or its confirm is open, a refresh updates ages
  and the preview, but it does not reorder, fold or remove rows.
- The confirm owns the keyboard: `y`, `n`, `esc`, `?`, `q` and `^C`. Every
  other key does nothing, and the footer lists `y send 2   n back   esc back
  ? keys   q close`.
- If the agent's prompt changes or its pane goes while the confirm is open,
  the confirm closes itself and says why. It never moves to another agent.
- In tablet mode the tap guard (D13) still applies on top.

**D19. One key bar on the board.** *New in revision 2, completing D14.* ⟦rev2: P4⟧
- On pc the board's footer row is the only place its keys are listed. Its
  frames' bottom borders carry no hints.
- The footer is built from the key table for whatever has focus: the list, a
  header (`space unfold` or `space fold`, whichever applies, then `l open`,
  `- fold all` and so on), the sheet (`1-3 pick ...`), the confirm, the find
  field.
- Keys that do nothing in that place are left out, not greyed.
- When the list does not fit, its title shows `N-M of T` (P10).
- Tablet keeps its button row as the bar, built from the same table.
- Not the panel, yet. Its borders (`╰ 1-4 pick   ? keys   esc board`) keep
  their hints, since D10 freezes its 40/60/80/120 goldens. A later pass can
  move them; see open question 20.

**D20. Failures stay.** *New in revision 2; needs OK, as it changes D9's
timing for failures and two lines of section 8.* ⟦rev2: P7⟧
- This is about the return-trip note of D9 only. A note that reports success
  (`✓ sent …`, `✓ answered …`) fades after 5 s, as D9 says. The 2-second notes
  of D8 (`a` on an agent that is not `▲`) and D12 (bare `r`, `g`) do not change.
- A note that reports a failure or a refusal (`✕ not sent …`, a wrapper error)
  stays until the next key. The wrapper marks it by starting the line with
  `✕`; the board keys on that (section 8).
- On pc the note takes the footer row. It does not trap the keys: the next key
  dismisses the note and also does its job, except `esc`, which only
  dismisses. The note ends with the keys, `esc dismiss` first and the one that
  fixes it last:
  `✕ not sent: server asks something new since you picked 2 · esc dismiss · a answers again`.

**D21. Find with no match.** *New in revision 2.* ⟦rev2: P8⟧
The field stays. The list area says `no agent matches "<text>"`, what find
reads (session, name, title, state word, last line), and the keys `⌫` edits,
`^U` clears and `esc` clears. On pc the right pane reads `no agent selected`.
`↵` keeps the filter as typed, and the list title shows `0 of 13`.

**D22. Keys that survive tmux and the terminal.** *New in revision 2.* ⟦rev2: P9⟧
- Every action has a plain letter, a digit or a `^` key. No `Alt` chords: some
  terminals and tmux take `Alt-[` (yazi #2587).
- `esc` is never the only way out. A bare `esc` waits for tmux's
  `escape-time`: 10 ms by default since tmux 3.5, 500 ms before that (tmux FAQ). So `q` closes the board, `n` backs out of the sheet
  and the confirm, `^U` clears the find field, and Home's `Back to board` tile
  has a digit.
- F-keys depend on the terminal (htop #1047), so no action is on an F-key
  alone. Compose's `F1` help is the one exception today; see open question 18.

## 5. User flows

### 5.1 PC board, end to end

1. `prefix w`. The board opens on the first `▲` agent, `api:0 server`. Its
   question and live screen fill the right pane (≥ 96 columns).
2. Find an agent, any of: `j`/`k` down the list; a digit for a row on screen;
   `u` for the next `▲`; `n`/`p` session by session; `/docs ↵`.
3. Fold the noise: `-` folds every group (`▲` and `✕` rows stay), `space`
   toggles one, `=` opens all.
4. Act on the selection: `↵` jump, `a` answer, `r` review, `f` panel, `i` info.
5. After `↵` you are in the pane and the popup is gone. After `a`, `r`, `f` or
   `i`, the board comes back on the same agent with a note (5.4).
6. `q` closes from the list, `^C` from anywhere, `esc` steps out one level at a
   time: overlay, find field, filter, board.

### 5.2 Tablet board, end to end

Keyboard:

1. `prefix w`. The list, the first `▲` agent selected and opened as a card.
2. Move with `j`/`k`, `⇥`, `]`, a digit, `u`, `n`/`p`, or `/` and a word.
3. `h` folds the selected agent's group, `l` unfolds it, `-` folds all for the
   triage view, `=` unfolds all.
4. `↵` jump, `a` answer, `r` review, `f` panel, `i` info. All but `↵` return
   to the board.

Touch:

1. Tap the Termux extra key that sends `prefix w` (added next to the `prefix f`
   key in `examples/termux.properties`).
2. Tap `▲ 2 need you` in the header or `[ ▲ next ]`, swipe to scroll, or tap
   `[ find ]` and a state chip.
3. Tap a session header to fold or unfold it; `[ fold all ]` for the triage
   view.
4. Tap an agent row: its card opens in place. Tap a card button: `[ answer ]`,
   `[ panel ]`, `[ review ]`, `[ info ]`, `[ jump ]`.

### 5.3 Quick answer from the board

1. On the board, select a `▲` agent, `api:0 server`.
2. `a`, or tap `[ answer ]`. The sheet lists the options read from its screen.
3. `2`, or tap `2 Yes, don't ask again`. The confirm opens: `y`, or the big
   `yes, send 2` at the top in tablet mode.
4. The board writes `deliver %3` and exits. The wrapper captures `%3` again,
   checks the prompt is the same, and types `2`.
5. The board reopens on `api:0`, now `●`, with the note
   `✓ answered 2 to api:0 server · it is running`. `u` goes to the next `▲`.

If the prompt changed between 3 and 4, nothing is typed and the note says so.

### 5.4 Board → panel → board

1. On the board, select `web:2 docs`. `f`.
2. pc at ≥ 164 columns: the panel docks in the right pane and the list holds.
   pc under 164: the panel takes the board's area under a crumb line. tablet:
   the board writes `panel %14`, exits, and `cockpit-board` runs
   `cockpit-panel -p %14` in the same popup with `COCKPIT_PANEL_FROM=board`,
   `COCKPIT_PANEL_CTX` and `COCKPIT_PANEL_MODE` set.
3. Home is titled `panel · ▲ web:2 · docs`; on a `▲` agent its first tile is
   `▲ Answer: …`, and its last is `Back to board`.
4. Compose: type the instruction, `^S`. Busy, maybe a Question, then Result or
   Steps.
5. `s` send. Multi-line or risky, so Confirm: `y`.
6. The panel exits with `send %14`; `cockpit-panel` pastes and presses Enter;
   its last line goes to the note file.
7. `cockpit-board` reruns `cockpit-tui board --select %14 --note <file>`. The
   first frame comes from `board.state`; the note reads
   `✓ sent 8 lines to web:2 docs · it is running again, ▲ 1 still needs you`.

Leaving without acting: `esc` on Home (named `esc board`), `Back to board`, or
`[ board ]`. Docked, that returns to the list in-process; otherwise the panel
exits with no action and the loop reopens the board. `q` in the panel ends the
loop and the popup.

### 5.5 Panel opened directly with `prefix f`

1. `prefix f` on a pane. The standalone popup. The title and the Answer tile
   come from the pane's own `@panestate` and `@panestate_msg`. At ≥ 110
   columns, Home shows the pane's screen in a second column.
2. Compose or Explain, then Result, Steps or Answer, then `i` or `s`, then
   Confirm when needed. Retry is `^R`.
3. The popup closes and the wrapper's `display-message` says
   `cockpit: sent to %3`, as today. No board opens: you were never on it.
4. tablet mode only: `[ board ]` on Home exits with `board %3`; `cockpit-panel`
   opens the board with `%3` selected, and the 5.4 loop applies from there.

## 6. Mockups

Copied from the drafts, with the width in each heading. Where a width is
missing for a screen, the note says so. Revision 2 edited them in place where
a ruling had moved past the drafts, and added the screens marked ⟦rev2: …⟧:

- Only the first nine agent rows on screen carry a digit (D6), and find
  renumbers its matches. ⟦rev2: P10⟧
- `? help` reads `? keys` (D14). ⟦rev2: P4⟧
- On the pc board the frames' bottom borders carry no key hints; the footer is
  the one bar, and on a selected header it lists the fold keys (D19). The
  panel's borders keep theirs for now. ⟦rev2: P4⟧
- An idle row says `idle` instead of `—`. ⟦rev2: P2⟧

Two known differences are left:

- tablet.md's folded groups show one urgent row each (each happens to have
  only one). Under D4 a folded group shows every `▲` and `✕` row.
- The help screens in 6.8 still list keys D4 and D6 dropped (`z`, `c` `e`, the
  two-digit numbers). Under D14 `?` is generated from the key table, so the
  built screen cannot show them.

### 6.1 The board, grouped list

60 columns, tablet, card open (tablet.md 4.1):

```
╭ board · 12 agents · ▲ 2 need you · ✕ 1 ──────────────────╮
│▾ api ──────────────────────────────────── 4 · ▲1 ●1 ✓1 ○1│
│┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ 4m12 ┓│
│┃Bash command                                            ┃│
│┃npm test -- --watch=false                               ┃│
│┃Do you want to proceed?                                 ┃│
│┃❯ 1. Yes   2. Yes, don't ask again   3. No              ┃│
│┃ [ jump ] [ answer ] [ review ] [ panel ] [ info ]      ┃│
│┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛│
│  2 ●  :1  tests      Bash: cargo test --workspace      2m│
│  3 ✓◆ :3  migrate    done · review pending            12m│
│  4 ○  :4  lint       idle                            1h04│
│▾ infra ───────────────────────────────────── 3 · ✕1 ●1 ✓1│
│  5 ✕  :0  terraform  plan failed: provider auth exp…   6m│
│  6 ●  :1  helm       Edit charts/api/values.yaml      40s│
│  7 ✓  :2  dns        done · 3 records updated         25m│
│▾ web ────────────────────────────────────────── 3 · ▲1 ●2│
│  8 ▲  :2  docs       plan ready: approve?            1m30│
│  9 ●  :0  ui         Edit src/App.tsx                 15s│
│    ●  :1  e2e        Bash: npx playwright test         3m│
│▾ ops ────────────────────────────────────────── 2 · ●1 ○1│
│    ●  :0  backup     Bash: restic snapshots            8m│
│    ○  :1  notes      idle                              2h│
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ find ] [ ▲ next ] [ fold all ] [ keys ] [ close ]      │
╰──────────────────────────────────────────────────────────╯
```

100 columns, pc, two panes (pc.md 1):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                              
┌ sessions ─────────────────────────────┐┏ ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━ needs you · 4m12 ┓
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ ~/src/api · feat/rate-limit · %3   ◆ review pending     ┃
│▎ 1 ▲◆ 0 server   Do you want t…  4m12 │┃ asked  Do you want to run npm test?                     ┃
│  2 ●  1 tests    Bash: cargo t…  1m03 │┃ said   I added the token-bucket limiter and wired it i… ┃
│  3 ●  3 migrate  Edit src/db/m…   22s │┃                                                         ┃
│  4 ✓◆ 4 auth     Pushed PR #23…   12m │┃ ── live · last 17 lines ─────────────────────────────── ┃
│  ▾ web ────────────────── ▲1 ●1 ✓1 ○1 │┃ ⏺ I added the token-bucket limiter and wired it into th ┃
│  5 ▲  2 docs     plan ready: a…   38s │┃   suite before I commit.                                ┃
│  6 ●  0 ui       Bash: pnpm te…  3m40◂│┃                                                         ┃
│  7 ✓◆ 1 checkout Opened PR #88…   25m │┃ ⏺ Bash(npm test -- --runInBand)                         ┃
│  8 ○  5 a11y     idle                 │┃                                                         ┃
│  ▾ infra ─────────────────── ✕1 ●1 ○1 │┃  Bash command                                           ┃
│  9 ✕  0 terrafo… terraform pla…  6m02 │┃                                                         ┃
│    ●  1 dns      Bash: dig +sh…    9s │┃    npm test -- --runInBand                              ┃
│    ○  2 k8s      idle                 │┃    Run the API test suite serially                      ┃
│  ▾ ml ───────────────────────── ●1 ✓1 │┃                                                         ┃
│    ●  0 train    Bash: python …   41m │┃  Do you want to proceed?                                ┃
│    ✓  1 eval     Wrote eval/te…  2h05 │┃  ❯ 1. Yes                                               ┃
│                                       │┃    2. Yes, and don't ask again for npm test commands in ┃
│                                       │┃    3. No, and tell Claude what to do differently (esc)  ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ↵ jump   a answer   r review   f panel   i info   / find   space fold   ? keys   q close           
```

180 columns, pc (pc.md 1):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                                                                                                              
┌ sessions ────────────────────────────────────────────────────┐┏ ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ needs you · 4m12 ┓
│  ▾ api ──────────────────────────────────────────── ▲1 ●2 ✓1 │┃ ~/src/api · feat/rate-limit · %3   ◆ review pending                                                              ┃
│▎ 1 ▲◆ 0 server     Do you want to run npm test?         4m12 │┃ asked  Do you want to run npm test?                                                                              ┃
│  2 ●  1 tests      Bash: cargo test --workspace         1m03 │┃ said   I added the token-bucket limiter and wired it into the router.                                            ┃
│  3 ●  3 migrate    Edit src/db/migrate.rs                22s │┃                                                                                                                  ┃
│  4 ✓◆ 4 auth       Pushed PR #231, waiting on review     12m │┃ ── live · last 23 lines ──────────────────────────────────────────────────────────────────────────────────────── ┃
│  ▾ web ───────────────────────────────────────── ▲1 ●1 ✓1 ○1 │┃ ⏺ I added the token-bucket limiter and wired it into the router. Running the                                     ┃
│  5 ▲  2 docs       plan ready: approve?                  38s │┃   suite before I commit.                                                                                         ┃
│  6 ●  0 ui         Bash: pnpm test Board                3m40◂│┃                                                                                                                  ┃
│  7 ✓◆ 1 checkout   Opened PR #88: card form a11y         25m │┃ ⏺ Bash(npm test -- --runInBand)                                                                                  ┃
│  8 ○  5 a11y       idle                                      │┃                                                                                                                  ┃
│  ▾ infra ────────────────────────────────────────── ✕1 ●1 ○1 │┃  Bash command                                                                                                    ┃
│  9 ✕  0 terraform  terraform plan: state lock held      6m02 │┃                                                                                                                  ┃
│    ●  1 dns        Bash: dig +short api.internal          9s │┃    npm test -- --runInBand                                                                                       ┃
│    ○  2 k8s        idle                                      │┃    Run the API test suite serially                                                                               ┃
│  ▾ ml ──────────────────────────────────────────────── ●1 ✓1 │┃                                                                                                                  ┃
│    ●  0 train      Bash: python train.py --epochs 40     41m │┃  Do you want to proceed?                                                                                         ┃
│    ✓  1 eval       Wrote eval/test_report.md            2h05 │┃  ❯ 1. Yes                                                                                                        ┃
│                                                              │┃    2. Yes, and don't ask again for npm test commands in /home/tzachi/src/api                                     ┃
│                                                              │┃    3. No, and tell Claude what to do differently (esc)                                                           ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
└──────────────────────────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ↵ jump   a answer   r review   f panel   i info   / find   space fold   ? keys   q close                                                                                           
```

### 6.2 Folded groups

60 columns (tablet.md 4.2):

```
╭ board · 12 agents · ▲ 2 need you · ✕ 1 ──────────────────╮
│▾ api ──────────────────────────────────── 4 · ▲1 ●1 ✓1 ○1│
│┏ 1 ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ 4m12 ┓│
│┃Bash command                                            ┃│
│┃npm test -- --watch=false                               ┃│
│┃Do you want to proceed?                                 ┃│
│┃❯ 1. Yes   2. Yes, don't ask again   3. No              ┃│
│┃ [ jump ] [ answer ] [ review ] [ panel ] [ info ]      ┃│
│┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛│
│  2 ●  :1  tests      Bash: cargo test --workspace      2m│
│  3 ✓◆ :3  migrate    done · review pending            12m│
│  4 ○  :4  lint       idle                            1h04│
│▸ infra ───────────────────────────────────── 3 · ✕1 ●1 ✓1│
│  5 ✕  :0  terraform  plan failed: provider auth exp…   6m│
│▸ web ────────────────────────────────────────── 3 · ▲1 ●2│
│  6 ▲  :2  docs       plan ready: approve?            1m30│
│▸ ops ────────────────────────────────────────── 2 · ●1 ○1│
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ find ] [ ▲ next ] [ fold all ] [ keys ] [ close ]      │
╰──────────────────────────────────────────────────────────╯
```

100 columns (pc.md 2):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                              
┌ sessions ─────────────────────────────┐┏ ▸ infra · 3 agents ━━━━━━━━━━━━━━━━━━━━━━━━━━━━ 1 error ┓
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ ~/src/infra · 3 windows                                 ┃
│  1 ▲◆ 0 server   Do you want t…  4m12 │┃                                                         ┃
│  2 ●  1 tests    Bash: cargo t…  1m03 │┃ ✕  0 terraform  terraform plan: state lock held  6m02   ┃
│  3 ●  3 migrate  Edit src/db/m…   22s │┃ ●  1 dns        Bash: dig +short api.internal      9s   ┃
│  4 ✓◆ 4 auth     Pushed PR #23…   12m │┃ ○  2 k8s        idle                                    ┃
│  ▸ web ─────── 3 folded · ▲1 ●1 ✓1 ○1 │┃                                                         ┃
│  5 ▲  2 docs     plan ready: a…   38s │┃ the newest lines of the most urgent agent:              ┃
│▎ ▸ infra ──────── 2 folded · ✕1 ●1 ○1 │┃                                                         ┃
│  6 ✕  0 terrafo… terraform pla…  6m02 │┃   ⏺ Bash(terraform plan -var-file=staging.tfvars)       ┃
│  ▸ ml ────────────── 2 folded · ●1 ✓1 │┃     ⎿  Error: Error acquiring the state lock            ┃
│                                       │┃        Lock Info:                                       ┃
│                                       │┃          ID:        7f1c2a90-3d1e-4b8a-9c55-0e2f6a1b7d4 ┃
│                                       │┃          Path:      cockpit-tf-state/staging/terraform. ┃
│                                       │┃          Who:       ci@runner-14                        ┃
│                                       │┃          Created:   2026-11-02 08:51:07 UTC             ┃
│                                       │┃                                                         ┃
│                                       │┃   ⏺ The plan failed: CI holds the state lock. I stopped ┃
│                                       │┃     force-unlocking it.                                 ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 space unfold   l open   - fold all   = open all   n p sessions   / find   ? keys   q close         
```

180 columns (pc.md 2):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                                                                                                              
┌ sessions ────────────────────────────────────────────────────┐┏ ▸ infra · 3 agents ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ 1 error ┓
│  ▾ api ──────────────────────────────────────────── ▲1 ●2 ✓1 │┃ ~/src/infra · 3 windows                                                                                          ┃
│  1 ▲◆ 0 server     Do you want to run npm test?         4m12 │┃                                                                                                                  ┃
│  2 ●  1 tests      Bash: cargo test --workspace         1m03 │┃ ✕  0 terraform  terraform plan: state lock held                                                           6m02   ┃
│  3 ●  3 migrate    Edit src/db/migrate.rs                22s │┃ ●  1 dns        Bash: dig +short api.internal                                                               9s   ┃
│  4 ✓◆ 4 auth       Pushed PR #231, waiting on review     12m │┃ ○  2 k8s        idle                                                                                             ┃
│  ▸ web ────────────────────────────── 3 folded · ▲1 ●1 ✓1 ○1 │┃                                                                                                                  ┃
│  5 ▲  2 docs       plan ready: approve?                  38s │┃ the newest lines of the most urgent agent:                                                                       ┃
│▎ ▸ infra ─────────────────────────────── 2 folded · ✕1 ●1 ○1 │┃                                                                                                                  ┃
│  6 ✕  0 terraform  terraform plan: state lock held      6m02 │┃   ⏺ Bash(terraform plan -var-file=staging.tfvars)                                                                ┃
│  ▸ ml ───────────────────────────────────── 2 folded · ●1 ✓1 │┃     ⎿  Error: Error acquiring the state lock                                                                     ┃
│                                                              │┃        Lock Info:                                                                                                ┃
│                                                              │┃          ID:        7f1c2a90-3d1e-4b8a-9c55-0e2f6a1b7d42                                                         ┃
│                                                              │┃          Path:      cockpit-tf-state/staging/terraform.tfstate                                                   ┃
│                                                              │┃          Who:       ci@runner-14                                                                                 ┃
│                                                              │┃          Created:   2026-11-02 08:51:07 UTC                                                                      ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃   ⏺ The plan failed: CI holds the state lock. I stopped here rather than                                         ┃
│                                                              │┃     force-unlocking it.                                                                                          ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
└──────────────────────────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 space unfold   l open   - fold all   = open all   n p sessions   / find   ? keys   q close                                                                                         
```

### 6.3 Find (`/`)

60 columns, typing (tablet.md 4.4):

```
╭ board · find · 3 of 12 ──────────────────────────────────╮
│ / test▏                                                  │
│▾ api ───────────────────────────────────── 2 of 4 · ▲1 ●1│
│▎ 1 ▲  :0  server     Do you want to run npm test?    4m12│
│  2 ●  :1  tests      Bash: cargo test --workspace      2m│
│▾ web ──────────────────────────────────────── 1 of 3 · ●1│
│  3 ●  :1  e2e        Bash: npx playwright test         3m│
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ ▲ needs ] [ ✕ error ] [ ● running ] [ ✓ done ]         │
│ [ keep ] [ clear ]                                       │
╰──────────────────────────────────────────────────────────╯
```

100 columns (pc.md 3):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                             / test · 4 of 13 
┌ sessions ──────────────────── 4 of 13 ┐┏ ●  api:1 · tests ━━━━━━━━━━━━━━━━━━━━━━━ running · 1m03 ┓
│ / test▏                               │┃ ~/src/api · feat/rate-limit · %4                        ┃
├───────────────────────────────────────┤┃ doing  Bash: cargo test --workspace                     ┃
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ said   Running the whole workspace before the limiter … ┃
│  1 ▲◆ 0 server   Do you want t…  4m12 │┃                                                         ┃
│▎ 2 ●  1 tests    Bash: cargo t…  1m03 │┃ ── live · last 17 lines ─────────────────────────────── ┃
│  ▾ web ────────────────── ▲1 ●1 ✓1 ○1 │┃ ⏺ Bash(cargo test --workspace)                          ┃
│  3 ●  0 ui       Bash: pnpm te…  3m40◂│┃   ⎿  Compiling api-core v0.4.2                          ┃
│  ▾ ml ───────────────────────── ●1 ✓1 │┃      Compiling api-server v0.4.2                        ┃
│  4 ✓  1 eval     Wrote eval/te…  2h05 │┃      Running unittests src/lib.rs                       ┃
│                                       │┃      test limiter::refills_after_window ... ok          ┃
│                                       │┃      test limiter::rejects_burst ... ok                 ┃
│                                       │┃      test routes::health ... ok                         ┃
│                                       │┃                                                         ┃
│                                       │┃ ✻ Waiting for 212 tests… (1m03s · esc to interrupt)     ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 type to filter   ↑↓ ⇥ move   ↵ keep the filter   esc clear                                         
```

180 columns (pc.md 3):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                                                                                             / test · 4 of 13 
┌ sessions ─────────────────────────────────────────── 4 of 13 ┐┏ ●  api:1 · tests ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ running · 1m03 ┓
│ / test▏                                                      │┃ ~/src/api · feat/rate-limit · %4                                                                                 ┃
├──────────────────────────────────────────────────────────────┤┃ doing  Bash: cargo test --workspace                                                                              ┃
│  ▾ api ──────────────────────────────────────────── ▲1 ●2 ✓1 │┃ said   Running the whole workspace before the limiter lands.                                                     ┃
│  1 ▲◆ 0 server     Do you want to run npm test?         4m12 │┃                                                                                                                  ┃
│▎ 2 ●  1 tests      Bash: cargo test --workspace         1m03 │┃ ── live · last 23 lines ──────────────────────────────────────────────────────────────────────────────────────── ┃
│  ▾ web ───────────────────────────────────────── ▲1 ●1 ✓1 ○1 │┃ ⏺ Bash(cargo test --workspace)                                                                                   ┃
│  3 ●  0 ui         Bash: pnpm test Board                3m40◂│┃   ⎿  Compiling api-core v0.4.2                                                                                   ┃
│  ▾ ml ──────────────────────────────────────────────── ●1 ✓1 │┃      Compiling api-server v0.4.2                                                                                 ┃
│  4 ✓  1 eval       Wrote eval/test_report.md            2h05 │┃      Running unittests src/lib.rs                                                                                ┃
│                                                              │┃      test limiter::refills_after_window ... ok                                                                   ┃
│                                                              │┃      test limiter::rejects_burst ... ok                                                                          ┃
│                                                              │┃      test routes::health ... ok                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃ ✻ Waiting for 212 tests… (1m03s · esc to interrupt)                                                              ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
└──────────────────────────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 type to filter   ↑↓ ⇥ move   ↵ keep the filter   esc clear                                                                                                                         
```

Find with no match, 60 columns, drawn for revision 2 (D21). ⟦rev2: P8⟧

```
╭ board · find · 0 of 12 ──────────────────────────────────╮
│ / terrafrom▏                                             │
│                                                          │
│ no agent matches "terrafrom"                             │
│ find reads the session, name, title, state word          │
│ and last line. ⌫ edits, ^U clears, esc clears.           │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ ▲ needs ] [ ✕ error ] [ ● running ] [ ✓ done ]         │
│ [ keep ] [ clear ]                                       │
╰──────────────────────────────────────────────────────────╯
```

The same at 100 columns. ⟦rev2: P8⟧

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                        / terrafrom · 0 of 13 
┌ sessions ──────────────────── 0 of 13 ┐┌ no agent selected ──────────────────────────────────────┐
│ / terrafrom▏                          ││ the agent you pick shows here.                          │
├───────────────────────────────────────┤│                                                         │
│                                       ││                                                         │
│ no agent matches "terrafrom"          ││                                                         │
│                                       ││                                                         │
│ find reads the session, name, title,  ││                                                         │
│ state word and last line.             ││                                                         │
│                                       ││                                                         │
│ ⌫ edits   ^U clears   esc clears      ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
│                                       ││                                                         │
└───────────────────────────────────────┘└─────────────────────────────────────────────────────────┘
 type to filter   ⌫ edit   ^U clear   ↵ keep   esc clear                                            
```

### 6.4 Quick answer and its confirm

60 columns, the sheet (tablet.md 4.6):

```
╭ answer · ▲ api:0 · server ───────────────────────────────╮
│ Bash command                                             │
│ npm test -- --watch=false                                │
│ Do you want to proceed?                                  │
│                                                          │
│ ╭──────────────────────────────────────────────────────╮ │
│ │ ▎1 Yes                                               │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  2 Yes, don't ask again                              │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  3 No                                                │ │
│ ╰──────────────────────────────────────────────────────╯ │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ panel ] [ jump ] [ keys ] [ back ]                     │
╰──────────────────────────────────────────────────────────╯
```

60 columns, the confirm (tablet.md 4.6):

```
╭ answer · ▲ api:0 · server ───────────────────────────────╮
│ ╭─────────────────────────╮ ╭──────────────────────────╮ │
│ │       yes, send 2       │ │            no            │ │
│ ╰─────────────────────────╯ ╰──────────────────────────╯ │
│ send 2 to api:0 · server?                                │
│                                                          │
│   2. Yes, don't ask again                                │
│                                                          │
│ it types 2 into the prompt, nothing else. you land       │
│ back on the board with server selected.                  │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ keys ]                                                 │
╰──────────────────────────────────────────────────────────╯
```

100 columns (pc.md 5.4). This draws pc.md's armed preview, which D8 replaces
with the sheet and the confirm below; kept for the record:

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                              
┌ sessions ─────────────────────────────┐┏ ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━ needs you · 4m12 ┓
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ ~/src/api · feat/rate-limit · %3   ◆ review pending     ┃
│▎ 1 ▲◆ 0 server   Do you want t…  4m12 │┃ asked  Do you want to run npm test?                     ┃
│  2 ●  1 tests    Bash: cargo t…  1m03 │┃ said   I added the token-bucket limiter and wired it i… ┃
│  3 ●  3 migrate  Edit src/db/m…   22s │┃                                                         ┃
│  4 ✓◆ 4 auth     Pushed PR #23…   12m │┃ ── live · last 17 lines ─────────────────────────────── ┃
│  ▾ web ────────────────── ▲1 ●1 ✓1 ○1 │┃ ⏺ I added the token-bucket limiter and wired it into th ┃
│  5 ▲  2 docs     plan ready: a…   38s │┃   suite before I commit.                                ┃
│  6 ●  0 ui       Bash: pnpm te…  3m40◂│┃                                                         ┃
│  7 ✓◆ 1 checkout Opened PR #88…   25m │┃ ⏺ Bash(npm test -- --runInBand)                         ┃
│  8 ○  5 a11y     idle                 │┃                                                         ┃
│  ▾ infra ─────────────────── ✕1 ●1 ○1 │┃  Bash command                                           ┃
│  9 ✕  0 terrafo… terraform pla…  6m02 │┃                                                         ┃
│    ●  1 dns      Bash: dig +sh…    9s │┃    npm test -- --runInBand                              ┃
│    ○  2 k8s      idle                 │┃    Run the API test suite serially                      ┃
│  ▾ ml ───────────────────────── ●1 ✓1 │┃                                                         ┃
│    ●  0 train    Bash: python …   41m │┃  Do you want to proceed?                                ┃
│    ✓  1 eval     Wrote eval/te…  2h05 │┃  ❯ 1. Yes                                               ┃
│                                       │┃    2. Yes, and don't ask again for npm test commands in ┃
│                                       │┃    3. No, and tell Claude what to do differently (esc)  ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 1 yes   2 yes, always npm test   3 no…   esc back                                                  
```

180 columns (pc.md 5.4), same caveat:

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                                                                                                              
┌ sessions ────────────────────────────────────────────────────┐┏ ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ needs you · 4m12 ┓
│  ▾ api ──────────────────────────────────────────── ▲1 ●2 ✓1 │┃ ~/src/api · feat/rate-limit · %3   ◆ review pending                                                              ┃
│▎ 1 ▲◆ 0 server     Do you want to run npm test?         4m12 │┃ asked  Do you want to run npm test?                                                                              ┃
│  2 ●  1 tests      Bash: cargo test --workspace         1m03 │┃ said   I added the token-bucket limiter and wired it into the router.                                            ┃
│  3 ●  3 migrate    Edit src/db/migrate.rs                22s │┃                                                                                                                  ┃
│  4 ✓◆ 4 auth       Pushed PR #231, waiting on review     12m │┃ ── live · last 23 lines ──────────────────────────────────────────────────────────────────────────────────────── ┃
│  ▾ web ───────────────────────────────────────── ▲1 ●1 ✓1 ○1 │┃ ⏺ I added the token-bucket limiter and wired it into the router. Running the                                     ┃
│  5 ▲  2 docs       plan ready: approve?                  38s │┃   suite before I commit.                                                                                         ┃
│  6 ●  0 ui         Bash: pnpm test Board                3m40◂│┃                                                                                                                  ┃
│  7 ✓◆ 1 checkout   Opened PR #88: card form a11y         25m │┃ ⏺ Bash(npm test -- --runInBand)                                                                                  ┃
│  8 ○  5 a11y       idle                                      │┃                                                                                                                  ┃
│  ▾ infra ────────────────────────────────────────── ✕1 ●1 ○1 │┃  Bash command                                                                                                    ┃
│  9 ✕  0 terraform  terraform plan: state lock held      6m02 │┃                                                                                                                  ┃
│    ●  1 dns        Bash: dig +short api.internal          9s │┃    npm test -- --runInBand                                                                                       ┃
│    ○  2 k8s        idle                                      │┃    Run the API test suite serially                                                                               ┃
│  ▾ ml ──────────────────────────────────────────────── ●1 ✓1 │┃                                                                                                                  ┃
│    ●  0 train      Bash: python train.py --epochs 40     41m │┃  Do you want to proceed?                                                                                         ┃
│    ✓  1 eval       Wrote eval/test_report.md            2h05 │┃  ❯ 1. Yes                                                                                                        ┃
│                                                              │┃    2. Yes, and don't ask again for npm test commands in /home/tzachi/src/api                                     ┃
│                                                              │┃    3. No, and tell Claude what to do differently (esc)                                                           ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
└──────────────────────────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 1 yes   2 yes, always npm test   3 no…   esc back                                                                                                                                  
```

The D8 sheet at 100 columns, drawn for revision 2. The list keeps its thin
frame; the sheet has the focus frame. ⟦rev2: P6 P7⟧

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                              
┌ sessions ─────────────────────────────┐┏ answer · ▲ api:0 · server ━━━━━━━━━━━━━━━━━━━ 3 options ┓
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ Bash command                                            ┃
│▎ 1 ▲◆ 0 server   Do you want t…  4m12 │┃   npm test -- --runInBand                               ┃
│  2 ●  1 tests    Bash: cargo t…  1m03 │┃   Run the API test suite serially                       ┃
│  3 ●  3 migrate  Edit src/db/m…   22s │┃                                                         ┃
│  4 ✓◆ 4 auth     Pushed PR #23…   12m │┃ Do you want to proceed?                                 ┃
│  ▾ web ────────────────── ▲1 ●1 ✓1 ○1 │┃                                                         ┃
│  5 ▲  2 docs     plan ready: a…   38s │┃ ▎1  Yes                                                 ┃
│  6 ●  0 ui       Bash: pnpm te…  3m40◂│┃  2  Yes, and don't ask again for npm test commands in…  ┃
│  7 ✓◆ 1 checkout Opened PR #88…   25m │┃  3  No, and tell Claude what to do differently          ┃
│  8 ○  5 a11y     idle                 │┃                                                         ┃
│  ▾ infra ─────────────────── ✕1 ●1 ○1 │┃ read from api:0's screen 2s ago. Before anything is     ┃
│  9 ✕  0 terrafo… terraform pla…  6m02 │┃ typed, the wrapper reads the screen again.              ┃
│    ●  1 dns      Bash: dig +sh…    9s │┃                                                         ┃
│    ○  2 k8s      idle                 │┃                                                         ┃
│  ▾ ml ───────────────────────── ●1 ✓1 │┃                                                         ┃
│    ●  0 train    Bash: python …   41m │┃                                                         ┃
│    ✓  1 eval     Wrote eval/te…  2h05 │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 1-3 pick   j k move   ↵ choose   f panel   n back   esc back   ? keys   q close                    
```

Its confirm at 100 columns. The list title says `held` and the footer lists
only the keys that work (D18). ⟦rev2: P5 P6⟧

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                              
┌ sessions · held ──────────────────────┐┏ answer · ▲ api:0 · server ━━━━━━━━━━━━━━━━━━━━━ confirm ┓
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ send 2 to api:0 · server?                               ┃
│▎ 1 ▲◆ 0 server   Do you want t…  4m12 │┃                                                         ┃
│  2 ●  1 tests    Bash: cargo t…  1m03 │┃   2. Yes, and don't ask again for npm test commands in  ┃
│  3 ●  3 migrate  Edit src/db/m…   22s │┃      /home/tzachi/src/api                               ┃
│  4 ✓◆ 4 auth     Pushed PR #23…   12m │┃                                                         ┃
│  ▾ web ────────────────── ▲1 ●1 ✓1 ○1 │┃ it types 2 and nothing else. The prompt is read again   ┃
│  5 ▲  2 docs     plan ready: a…   38s │┃ first; if it changed, nothing is typed and a note says s┃
│  6 ●  0 ui       Bash: pnpm te…  3m40◂│┃ after: back on this list, server selected.              ┃
│  7 ✓◆ 1 checkout Opened PR #88…   25m │┃                                                         ┃
│  8 ○  5 a11y     idle                 │┃ ↵ is off here, so a held Enter cannot send.             ┃
│  ▾ infra ─────────────────── ✕1 ●1 ○1 │┃ the list holds still while this is open.                ┃
│  9 ✕  0 terrafo… terraform pla…  6m02 │┃                                                         ┃
│    ●  1 dns      Bash: dig +sh…    9s │┃                                                         ┃
│    ○  2 k8s      idle                 │┃                                                         ┃
│  ▾ ml ───────────────────────── ●1 ✓1 │┃                                                         ┃
│    ●  0 train    Bash: python …   41m │┃                                                         ┃
│    ✓  1 eval     Wrote eval/te…  2h05 │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 y send 2   n back   esc back   ? keys   q close                                                    
```

Still missing: the sheet and the confirm at 180.

### 6.5 Back on the board with the note

60 columns, after the quick answer (tablet.md 4.7):

```
╭ board · sent 2 to api:0 · server ────────────────────────╮
│▸ infra ───────────────────────────────────── 3 · ✕1 ●1 ✓1│
│  1 ✕  :0  terraform  plan failed: provider auth exp…   6m│
│▸ web ────────────────────────────────────────── 3 · ▲1 ●2│
│  2 ▲  :2  docs       plan ready: approve?            1m30│
│▾ api ─────────────────────────────────────── 4 · ●2 ✓1 ○1│
│┏ 3 ●  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ 2s ┓│
│┃Bash: npm test -- --watch=false                         ┃│
│┃> api@1.4.0 test                                        ┃│
│┃PASS src/routes.test.ts (41 tests)                      ┃│
│┃RUNS src/db.test.ts                                     ┃│
│┃ [ jump ] [ review ] [ panel ] [ info ]                 ┃│
│┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛│
│  4 ●  :1  tests      Bash: cargo test --workspace      2m│
│  5 ✓◆ :3  migrate    done · review pending            12m│
│  6 ○  :4  lint       idle                            1h04│
│▸ ops ────────────────────────────────────────── 2 · ●1 ○1│
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│                                                          │
│ [ find ] [ ▲ next ] [ fold all ] [ keys ] [ close ]      │
╰──────────────────────────────────────────────────────────╯
```

100 columns, after a panel send (pc.md 5.1):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                              
┌ sessions ─────────────────────────────┐┏ ▲  web:2 · docs ━━━━━━━━━━━━━━━━━━━━━━━ needs you · 38s ┓
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ ~/src/web · docs/mdx · %14                              ┃
│  1 ▲◆ 0 server   Do you want t…  4m12 │┃ asked  plan ready: approve?                             ┃
│  2 ●  1 tests    Bash: cargo t…  1m03 │┃ said   Plan: move /docs to the new site generator.      ┃
│  3 ●  3 migrate  Edit src/db/m…   22s │┃                                                         ┃
│  4 ✓◆ 4 auth     Pushed PR #23…   12m │┃ ── live · last 17 lines ─────────────────────────────── ┃
│  ▾ web ────────────────── ▲1 ●1 ✓1 ○1 │┃ ⏺ Plan: move /docs to the new site generator            ┃
│▎ 5 ▲  2 docs     plan ready: a…   38s │┃   1. Port the 41 pages under docs/ to MDX               ┃
│  6 ●  0 ui       Bash: pnpm te…  3m40◂│┃   2. Replace the /v1 URLs with the new slugs            ┃
│  7 ✓◆ 1 checkout Opened PR #88…   25m │┃   3. Delete the old Jekyll config                       ┃
│  8 ○  5 a11y     idle                 │┃                                                         ┃
│  ▾ infra ─────────────────── ✕1 ●1 ○1 │┃  Ready to code?                                         ┃
│  9 ✕  0 terrafo… terraform pla…  6m02 │┃  ❯ 1. Yes, and auto-accept edits                        ┃
│    ●  1 dns      Bash: dig +sh…    9s │┃    2. Yes, and manually approve edits                   ┃
│    ○  2 k8s      idle                 │┃    3. No, keep planning                                 ┃
│  ▾ ml ───────────────────────── ●1 ✓1 │┃                                                         ┃
│    ●  0 train    Bash: python …   41m │┃                                                         ┃
│    ✓  1 eval     Wrote eval/te…  2h05 │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ✓ sent 8 lines to web:2 docs · it is running again, ▲ 1 still needs you                            
```

180 columns, after a panel send (pc.md 5.1):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                                                                                                              
┌ sessions ────────────────────────────────────────────────────┐┏ ▲  web:2 · docs ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ needs you · 38s ┓
│  ▾ api ──────────────────────────────────────────── ▲1 ●2 ✓1 │┃ ~/src/web · docs/mdx · %14                                                                                       ┃
│  1 ▲◆ 0 server     Do you want to run npm test?         4m12 │┃ asked  plan ready: approve?                                                                                      ┃
│  2 ●  1 tests      Bash: cargo test --workspace         1m03 │┃ said   Plan: move /docs to the new site generator.                                                               ┃
│  3 ●  3 migrate    Edit src/db/migrate.rs                22s │┃                                                                                                                  ┃
│  4 ✓◆ 4 auth       Pushed PR #231, waiting on review     12m │┃ ── live · last 23 lines ──────────────────────────────────────────────────────────────────────────────────────── ┃
│  ▾ web ───────────────────────────────────────── ▲1 ●1 ✓1 ○1 │┃ ⏺ Plan: move /docs to the new site generator                                                                     ┃
│▎ 5 ▲  2 docs       plan ready: approve?                  38s │┃   1. Port the 41 pages under docs/ to MDX                                                                        ┃
│  6 ●  0 ui         Bash: pnpm test Board                3m40◂│┃   2. Replace the /v1 URLs with the new slugs                                                                     ┃
│  7 ✓◆ 1 checkout   Opened PR #88: card form a11y         25m │┃   3. Delete the old Jekyll config                                                                                ┃
│  8 ○  5 a11y       idle                                      │┃                                                                                                                  ┃
│  ▾ infra ────────────────────────────────────────── ✕1 ●1 ○1 │┃  Ready to code?                                                                                                  ┃
│  9 ✕  0 terraform  terraform plan: state lock held      6m02 │┃  ❯ 1. Yes, and auto-accept edits                                                                                 ┃
│    ●  1 dns        Bash: dig +short api.internal          9s │┃    2. Yes, and manually approve edits                                                                            ┃
│    ○  2 k8s        idle                                      │┃    3. No, keep planning                                                                                          ┃
│  ▾ ml ──────────────────────────────────────────────── ●1 ✓1 │┃                                                                                                                  ┃
│    ●  0 train      Bash: python train.py --epochs 40     41m │┃                                                                                                                  ┃
│    ✓  1 eval       Wrote eval/test_report.md            2h05 │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
└──────────────────────────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ✓ sent 8 lines to web:2 docs · it is running again, ▲ 1 still needs you                                                                                                            
```

100 columns, a quick answer the wrapper refused because the prompt changed
(D20). The note stays until a key, and the next key also does its job; the
right pane shows the new prompt. ⟦rev2: P7⟧

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                              
┌ sessions ─────────────────────────────┐┏ ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━ needs you · 4m12 ┓
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ ~/src/api · feat/rate-limit · %3   ◆ review pending     ┃
│▎ 1 ▲◆ 0 server   Do you want t…  4m12 │┃ asked  Do you want to make this edit to limiter.rs?     ┃
│  2 ●  1 tests    Bash: cargo t…  1m03 │┃                                                         ┃
│  3 ●  3 migrate  Edit src/db/m…   22s │┃ ── live · last 9 lines ──────────────────────────────── ┃
│  4 ✓◆ 4 auth     Pushed PR #23…   12m │┃ ⏺ Update(src/limiter.rs)                                ┃
│  ▾ web ────────────────── ▲1 ●1 ✓1 ○1 │┃   ⎿  +12 -3                                             ┃
│  5 ▲  2 docs     plan ready: a…   38s │┃                                                         ┃
│  6 ●  0 ui       Bash: pnpm te…  3m40◂│┃  Edit file  src/limiter.rs                              ┃
│  7 ✓◆ 1 checkout Opened PR #88…   25m │┃                                                         ┃
│  8 ○  5 a11y     idle                 │┃  Do you want to make this edit to limiter.rs?           ┃
│  ▾ infra ─────────────────── ✕1 ●1 ○1 │┃  ❯ 1. Yes                                               ┃
│  9 ✕  0 terrafo… terraform pla…  6m02 │┃    2. Yes, allow all edits during this session          ┃
│    ●  1 dns      Bash: dig +sh…    9s │┃    3. No, and tell Claude what to do differently        ┃
│    ○  2 k8s      idle                 │┃                                                         ┃
│  ▾ ml ───────────────────────── ●1 ✓1 │┃                                                         ┃
│    ●  0 train    Bash: python …   41m │┃                                                         ┃
│    ✓  1 eval     Wrote eval/te…  2h05 │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ✕ not sent: server asks something new since you picked 2 · esc dismiss · a answers again           
```

### 6.6 Panel Home with the agent's context

60 columns, tablet, opened from the board on a `▲` agent (tablet.md 5.2):

```
╭ panel · ▲ api:0 · server ────────────────────────────────╮
│ ╭──────────────────────────────────────────────────────╮ │
│ │ ▎1 ▲ Answer: Do you want to run npm test?            │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  2 ★ Write a prompt    a task for Claude Code from … │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  3   Compose           type once: fix spelling, wri… │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  4   Explain           what is on this screen, and … │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  5   PC mode           keys only: the terminal sele… │ │
│ ╰──────────────────────────────────────────────────────╯ │
│ ╭──────────────────────────────────────────────────────╮ │
│ │  6   Back to board                                   │ │
│ ╰──────────────────────────────────────────────────────╯ │
│                                                          │
│ claude · /home/u/api · ▲ for 4m12                        │
│ will send: screen 6 lines, 0 hidden                      │
│                                                          │
│                                                          │
│ [ keys ] [ board ] [ close ]                             │
╰──────────────────────────────────────────────────────────╯
```

100 columns, pc, full-area from the board (pc.md 10.3):

```text
 board ▸ infra:0 terraform ▸ panel                                                                  
╭ panel · infra:0 terraform · claude ──────────────────────────────────────────────────────────────╮
│ ✕ error · terraform plan: state lock held                                                        │
│                                                                                                  │
│  ▎1 ★ Write a prompt  a task for Claude Code from your rough words                               │
│   2   Compose         type once: fix spelling, write, or insert                                  │
│   3   Explain         what is on this screen, and what next                                      │
│   4   Back to board                                                                              │
│                                                                                                  │
│ claude · ~/src/infra/envs/staging · main                                                         │
│ will send: screen 14 lines, 1 hidden                                                             │
│                                                                                                  │
│ ──────────────────────────────────────────────────────────────────────────────────────────────── │
│   ⏺ Bash(terraform plan -var-file=staging.tfvars)                                                │
│     ⎿  Error: Error acquiring the state lock                                                     │
│        Lock Info:                                                                                │
│          ID:        7f1c2a90-3d1e-4b8a-9c55-0e2f6a1b7d42                                         │
│          Path:      cockpit-tf-state/staging/terraform.tfstate                                   │
│          Who:       ci@runner-14                                                                 │
│          Created:   2026-11-02 08:51:07 UTC                                                      │
│                                                                                                  │
│   ⏺ The plan failed: CI holds the state lock. I stopped here rather than                         │
│     force-unlocking it.                                                                          │
│                                                                                                  │
│                                                                                                  │
╰ 1-4 pick  ? keys  esc board ─────────────────────────────────────────────────────────────────────╯
```

180 columns, pc, standalone `prefix f` (pc.md 10.3):

```text
╭ panel · infra:0 terraform · claude ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ ✕ error · terraform plan: state lock held                            │ what Claude will see                                                                                      │
│                                                                      │                                                                                                           │
│  ▎1 ★ Write a prompt  a task for Claude Code from your rough words   │   ⏺ Bash(terraform plan -var-file=staging.tfvars)                                                         │
│   2   Compose         type once: fix spelling, write, or insert      │     ⎿  Error: Error acquiring the state lock                                                              │
│   3   Explain         what is on this screen, and what next          │        Lock Info:                                                                                         │
│   4   Tablet mode     big tiles, tap buttons, the mouse              │          ID:        7f1c2a90-3d1e-4b8a-9c55-0e2f6a1b7d42                                                  │
│   5   Close                                                          │          Path:      cockpit-tf-state/staging/terraform.tfstate                                            │
│                                                                      │          Who:       ci@runner-14                                                                          │
│ claude · ~/src/infra/envs/staging · main                             │          Created:   2026-11-02 08:51:07 UTC                                                               │
│ will send: screen 14 lines, 1 hidden                                 │                                                                                                           │
│                                                                      │   ⏺ The plan failed: CI holds the state lock. I stopped here rather than                                  │
│                                                                      │     force-unlocking it.                                                                                   │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
│                                                                                                                                                                                  │
╰ 1-5 pick  ? keys  esc close ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

### 6.7 Panel docked (pc, 180)

Opened with `f` on `infra:0` (pc.md 10.3). Docked exists only at ≥ 164
columns, so there is no 60 or 100 variant:

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                                                                                                              
┌ sessions · held ─────────────────────────────────────────────┐╭ panel · infra:0 terraform · claude ──────────────────────────────────────────────────────────────────────────────╮
│  ▾ api ──────────────────────────────────────────── ▲1 ●2 ✓1 ││ ✕ error · terraform plan: state lock held                            │ what Claude will see                      │
│  1 ▲◆ 0 server     Do you want to run npm test?         4m12 ││                                                                      │                                           │
│  2 ●  1 tests      Bash: cargo test --workspace         1m03 ││  ▎1 ★ Write a prompt  a task for Claude Code from your rough words   │   ⏺ Bash(terraform plan -var-file=stagin… │
│  3 ●  3 migrate    Edit src/db/migrate.rs                22s ││   2   Compose         type once: fix spelling, write, or insert      │     ⎿  Error: Error acquiring the state … │
│  4 ✓◆ 4 auth       Pushed PR #231, waiting on review     12m ││   3   Explain         what is on this screen, and what next          │        Lock Info:                         │
│  ▾ web ───────────────────────────────────────── ▲1 ●1 ✓1 ○1 ││   4   Back to board                                                  │          ID:        7f1c2a90-3d1e-4b8a-9… │
│  5 ▲  2 docs       plan ready: approve?                  38s ││                                                                      │          Path:      cockpit-tf-state/sta… │
│  6 ●  0 ui         Bash: pnpm test Board                3m40◂││ claude · ~/src/infra/envs/staging · main                             │          Who:       ci@runner-14          │
│  7 ✓◆ 1 checkout   Opened PR #88: card form a11y         25m ││ will send: screen 14 lines, 1 hidden                                 │          Created:   2026-11-02 08:51:07 … │
│  8 ○  5 a11y       idle                                      ││                                                                      │                                           │
│  ▾ infra ────────────────────────────────────────── ✕1 ●1 ○1 ││                                                                      │   ⏺ The plan failed: CI holds the state … │
│▎ 9 ✕  0 terraform  terraform plan: state lock held      6m02 ││                                                                      │     force-unlocking it.                   │
│    ●  1 dns        Bash: dig +short api.internal          9s ││                                                                                                                  │
│    ○  2 k8s        idle                                      ││                                                                                                                  │
│  ▸ ml ───────────────────────────────────── 2 folded · ●1 ✓1 ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
│                                                              ││                                                                                                                  │
└──────────────────────────────────────────────────────────────┘╰ 1-4 pick  ? keys  esc board ─────────────────────────────────────────────────────────────────────────────────────╯
 panel ›  its keys are on its border   the list is held until esc board                                                                                                             
```

### 6.8 Help

60 columns (tablet.md 4.8):

```
╭ keys ────────────────────────────────────────────────────╮
│ move                       act on the selected agent     │
│ j k  ↑ ↓   row             ↵   jump to it, close         │
│ ⇥ ⇧⇥       next group      a   answer / ask it           │
│ 1-9 …      agent by number r   review its diff           │
│ u          next needs you  f   panel for it              │
│ g G        first / last    i   info                      │
│ [ ]        page            space  preview on / off       │
│                                                          │
│ fold                       view                          │
│ h ←  l →   fold / unfold   /   find (esc clears)         │
│ z          toggle group    v   tiles / list              │
│ c  e       fold / unfold   ?   this help                 │
│            all             ^L  refresh now               │
│                            q   close · esc back          │
╰ any key or tap closes ───────────────────────────────────╯
```

100 columns (pc.md 7):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                              
┌ sessions ─────────────────────────────┐┏ ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━ needs you · 4m12 ┓
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ ~/src/api · feat/rate-limit · %3   ◆ review pending     ┃
│▎ 1 ▲◆ 0 serve╭ keys ──────────────────────────────────────────────────────────────╮              ┃
│  2 ●  1 tests│ move                                act                            │d wired it i… ┃
│  3 ●  3 migra│ j k  ↑ ↓  row                       ↵   jump (on a header: fold)   │              ┃
│  4 ✓◆ 4 auth │ ⇥ ⇧⇥      next / previous agent     a   answer / ask this agent    │───────────── ┃
│  ▾ web ──────│ n p       next / previous session   r   review its diff            │ed it into th ┃
│  5 ▲  2 docs │ 1-9       agent by number           f   panel for this pane        │              ┃
│  6 ●  0 ui   │ g G       first / last              i   info for this pane         │              ┃
│  7 ✓◆ 1 check│ u         next blocked                                             │              ┃
│  8 ○  5 a11y │ [ ]       page                      view                           │              ┃
│  ▾ infra ────│                                     ?   this help                  │              ┃
│  9 ✕  0 terra│ groups                              ^L  refresh now                │              ┃
│    ●  1 dns  │ h ←    fold, or go to header        q   close · esc closes overlay │              ┃
│    ○  2 k8s  │ l →    unfold, or first agent                                      │              ┃
│  ▾ ml ───────│ space  fold / unfold                                               │              ┃
│    ●  0 train│ -  =   fold all / unfold all                                       │              ┃
│    ✓  1 eval │                                                                    │              ┃
│              │ find                                                               │t commands in ┃
│              │ /      filter as you type                                          │rently (esc)  ┃
│              │ esc    clear the filter                                            │              ┃
│              ╰ any key closes ────────────────────────────────────────────────────╯              ┃
│                                       │┃                                                         ┃
└───────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ↵ jump   a answer   r review   f panel   i info   / find   space fold   ? keys   q close           
```

180 columns (pc.md 7):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                                                                                                              
┌ sessions ────────────────────────────────────────────────────┐┏ ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ needs you · 4m12 ┓
│  ▾ api ──────────────────────────────────────────── ▲1 ●2 ✓1 │┃ ~/src/api · feat/rate-limit · %3   ◆ review pending                                                              ┃
│▎ 1 ▲◆ 0 server     Do you want to run npm test?         4m12 │┃ asked  Do you want to run npm test?                                                                              ┃
│  2 ●  1 tests      Bash: cargo test --workspace         1m03 │┃ said   I added the token-bucket limiter and wired it into the router.                                            ┃
│  3 ●  3 migrate    Edit src/db/migrate.rs                22s │┃                                                                                                                  ┃
│  4 ✓◆ 4 auth       Pushed PR #231, waiting on review     12m │┃ ── live · last 23 lines ──────────────────────────────────────────────────────────────────────────────────────── ┃
│  ▾ web ───────────────────────────────────────── ▲1 ●1 ✓1 ○1 │┃ ⏺ I added the token-bucket limiter and wired it into the router. Running the                                     ┃
│  5 ▲  2 docs       plan ready: approve?                  38s │┃   suite before I commit.                                                                                         ┃
│  6 ●  0 ui         Bash: pnpm test Bo╭ keys ──────────────────────────────────────────────────────────────────────────────────────────────╮                                      ┃
│  7 ✓◆ 1 checkout   Opened PR #88: car│ move                                groups                          act                            │                                      ┃
│  8 ○  5 a11y       idle              │ j k  ↑ ↓  row                       h ←    fold, or go to header    ↵   jump (on a header: fold)   │                                      ┃
│  ▾ infra ────────────────────────────│ ⇥ ⇧⇥      next / previous agent     l →    unfold, or first agent   a   answer / ask this agent    │                                      ┃
│  9 ✕  0 terraform  terraform plan: st│ n p       next / previous session   space  fold / unfold            r   review its diff            │                                      ┃
│    ●  1 dns        Bash: dig +short a│ 1-9       agent by number           -  =   fold all / unfold all    f   panel for this pane        │                                      ┃
│    ○  2 k8s        idle              │ g G       first / last                                              i   info for this pane         │                                      ┃
│  ▾ ml ───────────────────────────────│ u         next blocked              find                                                           │                                      ┃
│    ●  0 train      Bash: python train│ [ ]       page                      /      filter as you type       view                           │                                      ┃
│    ✓  1 eval       Wrote eval/test_re│                                     esc    clear the filter         ?   this help                  │                                      ┃
│                                      │                                                                     ^L  refresh now                │i                                     ┃
│                                      │                                                                     q   close · esc closes overlay │                                      ┃
│                                      ╰ any key closes ────────────────────────────────────────────────────────────────────────────────────╯                                      ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
│                                                              │┃                                                                                                                  ┃
└──────────────────────────────────────────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ↵ jump   a answer   r review   f panel   i info   / find   space fold   ? keys   q close                                                                                           
```

### 6.9 Empty and floor

No agent panes, 60 columns (tablet.md 4.9):

```
╭ board ───────────────────────────────────────────────────╮
│                                                          │
│ no agent panes                                           │
│                                                          │
│ cockpit tracks a pane once an agent runs in it.          │
│ start an agent in any pane and it shows up here.         │
│                                                          │
╰ q close ─────────────────────────────────────────────────╯
```

No agent panes, 100 columns (pc.md 8):

```text
 board · 0 agents                                                                                   
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                   ╭ board ─────────────────────────────────────────────────────╮                   
                   │                                                            │                   
                   │ no agent panes                                             │                   
                   │                                                            │                   
                   │ cockpit tracks a pane once an agent runs in it.            │                   
                   │ start an agent in any pane and it shows up here.           │                   
                   │                                                            │                   
                   ╰ q close ───────────────────────────────────────────────────╯                   
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
                                                                                                    
```

No agent panes, 180 columns (pc.md 8):

```text
 board · 0 agents                                                                                                                                                                   
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                           ╭ board ─────────────────────────────────────────────────────╮                                                           
                                                           │                                                            │                                                           
                                                           │ no agent panes                                             │                                                           
                                                           │                                                            │                                                           
                                                           │ cockpit tracks a pane once an agent runs in it.            │                                                           
                                                           │ start an agent in any pane and it shows up here.           │                                                           
                                                           │                                                            │                                                           
                                                           ╰ q close ───────────────────────────────────────────────────╯                                                           
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
                                                                                                                                                                                    
```

Below the tablet floor, 26 columns (tablet.md 4.10):

```
╭ board ─────────────────╮
│ make the window larger │
│ or rotate              │
│ esc or q closes        │
│                        │
│                        │
│                        │
│                        │
│                        │
╰────────────────────────╯
```

Short screen, 100x10 (pc.md 8):

```text
 board · 13 agents · 4 sessions · 2 need you · 1 error                                              
┌ sessions ────────────────── 1-6 of 17 ┐┏ ▲  api:0 · server ━━━━━━━━━━━━━━━━━━━━ needs you · 4m12 ┓
│  ▾ api ───────────────────── ▲1 ●2 ✓1 │┃ ── live · last 5 lines ──────────────────────────────── ┃
│▎ 1 ▲◆ 0 server   Do you want t…  4m12 │┃                                                         ┃
│  2 ●  1 tests    Bash: cargo t…  1m03 │┃  Do you want to proceed?                                ┃
│  3 ●  3 migrate  Edit src/db/m…   22s │┃  ❯ 1. Yes                                               ┃
│  4 ✓◆ 4 auth     Pushed PR #23…   12m │┃    2. Yes, and don't ask again for npm test commands in ┃
│  ▾ web ────────────────── ▲1 ●1 ✓1 ○1 │┃    3. No, and tell Claude what to do differently (esc)  ┃
└────────────────────── ▼ 11 more · ▲1 ─┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ↵ jump   a answer   r review   f panel   i info   / find   space fold   ? keys   q close           
```

Missing: a floor or short-screen mockup at 60 and at 180.

## 7. Key map

Marks are against `KEYS.md` and the key table in `docs/tui.md`. `KEYS.md`
lists only the prefix keys, which do not change: `prefix w` and `prefix f`
keep their meaning. Unmarked rows are unchanged.

### 7.1 The board, list layout

The tiles layout (`v`) keeps today's table from `docs/tui.md`, plus `v`, `/`
and the return trip.

| key | does | change |
|---|---|---|
| `j` `k` `↑` `↓` | next / previous row, headers included | CHANGED: was tile below / above |
| `h` `←` | fold the selected group | CHANGED: was column left |
| `l` `→` | unfold the selected group; on an open header, enter it | CHANGED: was column right |
| `space` | toggle the selected group | NEW |
| `-` / `=` | fold all / unfold all | NEW |
| `⇥` `⇧⇥` | next / previous agent, skipping headers, wrapping | |
| `n` `p` | next / previous session header | NEW |
| `1`-`9` | the Nth agent row on screen. Selects, does not jump | CHANGED: was absolute tile 1-9 |
| `g` `G` | first / last row | |
| `u` | next `▲` agent, wrapping | |
| `[` `]` `PgUp` `PgDn` | scroll a screenful; the selection only clamps | CHANGED: the selection no longer moves a page |
| `/` | find (D7) | NEW |
| `↵` | jump to the pane and close; on a header, toggle the fold | CHANGED on headers |
| `a` | on `▲`: the quick answer sheet. Otherwise the 2-second note | CHANGED: was jump on `▲` |
| `r` `f` `i` | review / panel / info; the board comes back after | CHANGED: the return |
| `v` | tiles / list | NEW |
| `?` | keys, full-popup, scrolls | CHANGED: label `? keys`, was `? help` |
| `^L` | refetch and redraw now | |
| `q` | close; in the find field it types | CHANGED in the field |
| `esc` | dismiss a failure note (D20), else close the overlay, else the find field, else the filter, else the board | CHANGED: the ladder ⟦rev2: P7⟧ |
| `^C` | close from anywhere, the find field included | NEW |

Not adopted from tablet.md: `z` (toggle), `c` (fold all), `e` (unfold all), the
two-digit number. Not adopted from pc.md: `o` on the answer sheet.

Inside the quick answer sheet: `1`-`9` pick and open the confirm, `↑↓` `j` `k`
move, `↵` opens the confirm for the selected option, `f` opens the panel, `n` and
`esc` go back (D22). ⟦rev2: P9⟧ In the confirm: `y` sends, `n` and `esc` go back, `?` and `q` work as
everywhere, and `↵` and every other key do nothing (D18). ⟦rev2: P6⟧

Inside the find field: letters type, `↑↓` `^N` `^P` `⇥` `⇧⇥` move, `⌫` edits,
`^U` clears, `↵` keeps, `esc` clears.

### 7.2 The panel

| key | does | change |
|---|---|---|
| `^R` | retry the call (Result, Steps, Error) | NEW: replaces `r` |
| `r` | a 2-second note, `^R retries here · r reviews on the board` | CHANGED: was retry |
| `^G` | best guess (Question) | NEW: replaces `g` |
| `g` | a 2-second note naming `^G` | CHANGED: was best guess |
| `1`-`N` on Home, `▲` agent | `1` is `▲ Answer: …`; every other tile one digit higher | CHANGED |
| `Close` tile, opened from the board | `Back to board`, same digit | CHANGED |
| `esc` on Home, opened from the board | back to the board (`esc board`) | CHANGED |
| `Tablet mode` tile, docked | hidden; `Back to board` takes its digit | CHANGED |
| `[ board ]`, tablet mode | from the board: back. Standalone: action `board %N` | NEW |
| `[ more ]`, tablet Compose under 16 rows | the rest of Compose's buttons | NEW |
| everything else | unchanged | |

`r` retry and `g` best guess are REMOVED as keys; they survive only as the
notes above.

### 7.3 Tablet tap map, the board

| element | tap runs |
|---|---|
| `▲ 2 need you` in the header | `u` |
| the filter chip `[ /test ✕ ]` | `esc` (clear the filter) |
| a group header | `space` on that group |
| an agent row | select it, which opens its card |
| the open card's top or preview | nothing (the second tap of a double tap) |
| card buttons | `↵` `a` `r` `f` `i` |
| button bar | `/` `u` `-` `?` `q` |
| find chips | type the state word |
| `[ keep ]` `[ clear ]` | `↵` `esc` |
| an answer option | its digit |
| `yes, send N` / `no` | `y` / `n` |
| wheel | scroll 3 rows; the selection clamps to stay on screen |
| anything, with `?` open | `esc` |
| anything, under the floor | nothing |

The guard drops a tap within 300 ms of a frame whose hit map changed (a fold,
the card opening, a find narrowing, a reorder, an overlay), 800 ms after a
reopen. A tick that only changes an age starts no guard. Keys are never
dropped. A property test from 28x12 to 120x40 checks that no confirming cell
sits where the element that opened it was.

The panel's tap map is unchanged apart from the buttons in 7.2.

## 8. Contract changes for #61

Each line changes something `docs/tui.md` freezes, so each needs a comment on
#61 before its step lands.

**Rules.**
- "No mouse on the board" becomes "no mouse on the board or the panel except in
  tablet mode", with the panel's restore on every exit path.
- The 40-column refusal applies to the pc tiles layout only. The pc list keeps
  it and adds `too short` under 8 rows; tablet mode uses the 28x12 card.

**Flags.**
- `cockpit-tui board --select %N`: open with that pane selected.
- `cockpit-tui board --note <file>`: show the file's one line as the note for
  5 s, or until the next key when the line starts with `✕` (D20). ⟦rev2: P7⟧ Read with the panel's state-file rules (no symlink, regular file, small,
  created new with `O_NOFOLLOW`, mode 0600).

**Environment and options.**
- The board reads `COCKPIT_PANEL_MODE`; the board's popup gets
  `-e COCKPIT_PANEL_MODE=<mode>`. `@cockpit_panel_mode` keeps its name and now
  governs both screens.
- `COCKPIT_PANEL_FROM=board` and
  `COCKPIT_PANEL_CTX=<state>\t<sess:idx>\t<title>\t<msg>`, set by
  `cockpit-board` for the panel.
- New options, proposed by pc.md: `@cockpit_board_return` (`on`; `off` keeps
  today's one-way actions) and `@cockpit_panel_dock` (`auto` docks at ≥ 164;
  `off` always exits with `panel %N`).
- The board's popup title drops `#{session_name}`, which named the client's
  session, not the agent's.

**State file.** `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-cockpit/board.state`:
a `cockpit-board-state 1` head, the last selection, the kept filter, one
`fold <session>` line per folded session. Created new with `O_NOFOLLOW`, mode
0600, at most 64 KiB, read back strictly; anything off means a fresh board and
a note. `COCKPIT_READONLY` skips the write.

**Actions.**
- `deliver %N`, reserved today, goes live. The digit goes in
  `$COCKPIT_ACTION.text`, like `paste` and `send`. The prompt the board parsed
  goes in `$COCKPIT_ACTION.prompt`, written before the action line under the
  same rules as `.cmd`. The wrapper compares it with a fresh capture and runs
  `send-keys -t %N <digit>` only when they match. The binary still executes
  nothing.
- `board %N`, from a standalone tablet panel: `cockpit-panel` opens the board
  with `--select %N`.
- Docked, the panel's `paste %N` and `send %N` are printed by the board's
  process, and `cockpit-board` runs `cockpit-panel`'s paste half.

**The cockpit-board loop.** `scripts/cockpit-board` loops like
`scripts/cockpit-panel`'s steps loop. After `review`, `info`, `panel` and
`deliver` it writes the wrapper's last line to the note file and reruns the
board with `--select` and `--note`. It ends on `jump`, on an exit with no
action, on a failure of the board itself, or when `@cockpit_board_return` is
`off`. An action the wrapper refused, such as a `deliver` whose prompt changed,
is not a failure: the board comes back with a `✕` note (D20). ⟦rev2: P7⟧ `info` drops
its bare `read -r _`.

**Panel keys and Home.**
- `r` retry becomes `^R`; `g` best guess becomes `^G`.
- On a `▲` agent Home gains `▲ Answer: …` as tile 1, and every other digit
  moves up one (Close goes from 5 to 6 on a Claude Code or shell pane).
- Opened from the board, `Close` is `Back to board`; docked, the Tablet mode
  tile is hidden.

**Code shape.**
- `board::render()` returns a hit map, as `view::render()` does.
- The shared chrome (footer, help, floor card) needs a home both screens can
  call. Either a new module, which changes the frozen module list, or `pub`
  functions in `panel::view`. Step 1 decides.

## 9. Trade-offs and open questions

### 9.1 Trade-offs taken

- **Urgency over spatial memory.** Hot sessions sort first, so a group moves
  when it starts or stops needing you. The two bands, the anchored scroll and
  the tap guard keep moves rare and harmless. A pinned "needs you" strip was
  rejected: it shows every blocked agent twice, puts two cursor targets on one
  pane, and costs rows on a 12-row keyboard-up screen.
- **Folded groups show every `▲`/`✕` row.** A folded group can grow past 2
  rows when several agents in one session are blocked. Accepted, so that
  folding never hides one.
- **The card is on by default in tablet mode.** It costs about 6 rows; the list
  scrolls sooner on 40x24.
- **Digits only reach nine rows on screen.** With 13 agents, four rows have no
  digit until scrolled to. `/`, `u` and `n`/`p` cover the rest, without a
  clock in input handling.
- **`h`/`l` mean fold in the list and column in the tiles.** Tree-view
  convention (ranger, lf, nvim-tree) in the list; `?` is generated per layout.
- **`i` and `f` still differ between the screens.** They are the panel's main
  verbs and neither sends an Enter or spends a call by itself; the two that
  did, `r` and `g`, moved. Renaming them would break the panel's tablet buttons
  and four sets of goldens.
- **`a` no longer jumps on a `▲` agent.** `↵` still does, and the footer's
  `↵ jump` stays visible.
- **The quick answer parses a screen.** A misread is the risk. The sheet spells
  every option it found, the confirm repeats the pick, and the wrapper checks
  the prompt again before typing. Anything it cannot read falls back to the
  panel or a jump.
- **Reopen latency.** Each return is a new process and a fresh fleet read
  (380 ms measured). The first frame comes from `board.state`. A board that
  never exits would have to run the paste itself and break "actions are
  printed".
- **The docked list is held.** Firing the panel at two agents takes two trips.
  A draft must have exactly one target.
- **Two widths of panel.** Docked at ≥ 164, full-area below. The goldens gain
  `-dock-180` and `-full-100` variants; the 40/60/80/120 goldens must not
  change.
- **Find stays a substring filter.** With `prefix r` gone (#228), the board's
  find is the only search over the fleet. Substring narrowing over a few dozen
  rows is enough; fuzzy ranking and multi-select are left out. ⟦rev2: #228⟧
- **`@cockpit_board_return on` changes what `r` from the board does today.**
  You end on the board, not the pane. Worth a release note.

- **The list holds still under an open confirm (D18).** Ages and the preview
  stay live, but a row that should move waits until the confirm closes.
  Accepted: the other way moves the target under `y`. ⟦rev2: P5⟧
- **A failure note takes the pc footer until a key (D20).** The key bar, `? keys`
  and `q close` included, is gone for that moment, against D14's "always
  kept". Every key still works, and the first one brings the bar back. ⟦rev2: P7⟧
- **Colour stays with the tmux theme (D17).** Most apps in section 11 draw with
  the terminal's 16 colours. This repo follows `@thm_*` instead, so a user on
  a light terminal needs a light `@thm_*` set. D17's contrast rule limits the
  damage when they have not set one. ⟦rev2: P1 P3⟧

### 9.2 Open questions

1. **How is tmux's active pane marked** now that `▎` is the selection: `◂` at
   the row end (pc.md) or an underlined title (tablet.md)? `◂` survives
   `NO_COLOR` and the ascii tier without an attribute.
2. **Does the pc board adopt the panel's rounded frame** (tablet.md mockups) or
   keep a header row and a footer row (pc.md mockups)?
3. **Card on/off in tablet mode.** tablet.md used `space` on an agent; D4 gives
   `space` to the group fold. Another key, or no toggle?
4. **Does the `v` choice persist** in `board.state`, per mode?
5. **Quick answer check:** compare the prompt's text or a hash of it? And should
   `a` on a `▲` agent with no options open Compose (pc.md) instead of the
   panel-or-jump sheet?
6. **`u` and errors.** `u` visits `▲` only. Should `✕` agents get their own key,
   or join the cycle after the `▲` ones?
7. **`=` on Termux.** `-` is on the default extra-keys row, `=` is not. Touch
   users have `[ fold all ]`; is an `[ open all ]` button needed?
8. **Does Termux raise the soft keyboard on a tap** while mouse tracking is on?
   If not, `[ find ]` needs a keyboard key on the extra-keys row. Needs a device
   test.
9. **Narrow pc default.** Under 96 columns pc shows the list alone. Should
   `auto` also pick tablet's card layout there?
10. **An alias for the option.** `@cockpit_panel_mode` now governs the board.
    Add `@cockpit_tablet_mode` before more surfaces read it?
11. **A board key in a standalone pc panel**, like tablet's `[ board ]`? `w` is
    free in the panel.
12. *Settled with #228.* **`↵` on a session header** stays fold (D4). Actions
    on other targets keep the one-line `<verb> <target>` shape with a tmux id:
    `%N` pane, `$N` session, `@N` window, checked against `^[%$@][0-9]+$`, never
    a name. #228's session view uses `switch $N` (ends the loop, like `jump`),
    `kill-session $N` and `rename-session $N` (confirmed in the TUI; the shown
    name goes in `.expect` and the wrapper re-checks it), and `new-session -`
    (the name in `.text`). One action per file: no multi-target lines. ⟦rev2: #228⟧
13. **`@cockpit_board_order name`** as an escape hatch if the two bands still
    move too much.
14. **Folds per machine or per tmux server.** The state file is per machine.
15. **Preview scroll** (`^U` `^D` over `capture-pane -S`) for a question longer
    than the pane. Not designed.
16. **Not addressed here:** Busy showing the pane; the `○` glyph in Steps'
    resume note; the tiles' dead rows (audit 1.3) inside the `v` layout.
17. **The tablet popup.** tablet.md proposes opening the board's popup at the
    panel's tablet size and with `-B`, since both screens draw their own frame.
    Adopt both, or only the size?
18. **Help inside Compose.** `?` types there, so help is `F1`, which some
    terminals and Termux do not send (htop #1047). Add a second key, such as
    `^O`, or a `[ keys ]` button on tablet and leave pc on `F1`? ⟦rev2: P9⟧
19. *Settled in #256.* **Who owns the palette values.** Session
    tmux-cockpit-79 owns `theme.rs`. It took D17's rules, added `@thm_sel` and
    `@thm_faint`, and D17's table now lists the R7 names. ⟦rev2: P1⟧
20. **The panel's border hints.** D19 leaves them, because D10 freezes the
    panel's goldens. Move them to a footer in the step that touches those
    goldens anyway (step 9), or leave them? ⟦rev2: P4⟧

## 10. Implementation plan

Each step is one PR, mergeable on its own, and useful without the ones after
it. Every test follows `docs/tui.md`'s rules: goldens in `tui/tests/golden/`,
`tests/run.sh` blocks on a private `-L` socket registered in `SOCKS`, a count
or a polled condition and never a `sleep`.

Lane C (`refactor/panel-risky`) moves the panel's action writing and risk
checks out of `tui/src/panel/app.rs` into new `panel/action.rs` and
`panel/risky.rs`. Steps 5, 6, 7 and 9 edit that code and **wait for lane C to
merge**. Steps 1-4 and 8 touch the board, the scripts, and at most a few lines
of `panel/view.rs`, and can land first.

**Step 1. Shared chrome.**
- Does: one `footer()` with priority drop, one full-popup scrolling `?`, one
  floor card, used by both screens. The board's footer becomes state-aware;
  `▎` becomes the board's selection mark; the board says `? keys`; the board
  gets a `too short` message; the empty card is centred and wraps; the stale
  header and card (D16).
- Files: `tui/src/board.rs`, `tui/src/panel/view.rs`, the module that owns the
  chrome (section 8), `docs/tui.md`.
- Tests: unit tests for the footer's drop order at each width; goldens
  `board-{60,100,180}.txt` for footer and help, `board-help-{60,100,180}.txt`,
  `board-short-100.txt`, `board-empty-{60,100,180}.txt`, `board-stale-100.txt`;
  every existing panel golden unchanged.

**Step 2. The return trip.**
- Does: `--select %N` and `--note <file>`; `board.state` holds the selection
  (folds and filter join it in steps 3 and 4); `scripts/cockpit-board` loops
  after `review`, `info` and `panel`; `@cockpit_board_return`; the board's
  popup passes `COCKPIT_PANEL_MODE` and drops `#{session_name}`.
- Files: `tui/src/main.rs`, `tui/src/board.rs`, `scripts/cockpit-board`,
  `scripts/cockpit-panel` (its last line to the note file), `cockpit.tmux`,
  `docs/tui.md`.
- Tests: unit tests for the state file's strict read (symlink, oversize, bad
  head, unknown pane); `run.sh` blocks: `r` then return lands on the same
  pane; `f` then `esc` on Home returns; `jump` ends the loop;
  `@cockpit_board_return off` keeps today's behaviour; the popup's `-e
  COCKPIT_PANEL_MODE`. Golden `board-note-100.txt`.

**Step 3. Session groups, folding and order.**
- Does: the grouped list as the default layout (D1's list, without the right
  pane), D3 order, D4 folding and keys, D5 selection, `n`/`p`, D6 digits,
  folds in `board.state`, `v` back to tiles.
- Files: `tui/src/board.rs`, `docs/tui.md`.
- Tests: unit tests for the order (bands, no clock, a state change moves a
  group only across bands), fold rules (urgent rows stay, nothing unfolds by
  itself), selection survival on refresh, removal and fold, digits on screen
  after a scroll; goldens `board-list-{60,100,180}.txt`,
  `board-folded-{60,100,180}.txt`, `board-tiles-100.txt` (the `v` layout);
  `run.sh`: folds survive a reopen.

**Step 4. Find.**
- Does: `/`, the field and its keys, the `esc` ladder, `^C`, the filter in
  `board.state`.
- Files: `tui/src/board.rs`, `docs/tui.md`.
- Tests: unit tests for matching (terms AND, state words, hidden and
  temporarily unfolded groups), selection on narrowing and clearing, `q` typing
  in the field; goldens `board-find-{60,100,180}.txt` and
  `board-find-none-100.txt`.

**Step 5. The panel knows its agent.** *Waits for lane C.*
- Does: the title, `▲ Answer: …` as Home's first tile (opening Compose with the
  question until step 6 adds the sheet), `Back to board`, `esc board`,
  `COCKPIT_PANEL_FROM` and `COCKPIT_PANEL_CTX`, `@panestate` read when
  standalone, Confirm naming the agent.
- Files: `tui/src/panel/app.rs`, `tui/src/panel/view.rs`, `tui/src/panel.rs`,
  `scripts/cockpit-board`, `docs/tui.md`.
- Tests: unit tests for Home's digits with and without a `▲` agent and from the
  board; `tui/tests/panel_env.rs` for the two variables and malformed values;
  goldens `panel-home-{60,100,180}` from the board, on a `▲` agent;
  `run.sh`: `f` from the board passes the context.

**Step 6. Quick answer (`deliver`).** *Waits for lane C.*
- Does: the options parser, the sheet and its confirm in both modes,
  `deliver %N` with `.text` and `.prompt`, the wrapper's prompt check and
  `send-keys`, the return with its note. The Answer tile opens the sheet.
- Files: `tui/src/board.rs`, `tui/src/panel/action.rs` (lane C's), a parser
  beside `tui/src/panel/parse.rs`, `scripts/cockpit-board`, `docs/tui.md`.
- Tests: unit tests for the parser on real Claude Code prompts and on screens
  with no options; goldens `board-answer-{60,100,180}.txt`,
  `board-answer-confirm-{60,100,180}.txt`; `run.sh` on a private socket: the
  digit lands on the fake agent, a changed prompt types nothing, and
  `COCKPIT_READONLY` types nothing.

**Step 7. PC two panes and the docked panel.** *Waits for lane C.*
- Does: the right pane at ≥ 96, the docked panel at ≥ 164, the full-area panel
  with its crumb below that, the held list, the panel's two-column views at
  ≥ 110, `@cockpit_panel_dock`.
- Files: `tui/src/board.rs`, `tui/src/panel/app.rs`, `tui/src/panel/view.rs`,
  `scripts/cockpit-board`, `docs/tui.md`.
- Tests: goldens `board-list-{100,180}.txt` updated with the preview pane,
  `panel-*-dock-180.txt`, `panel-*-full-100.txt`; the 40/60/80/120 panel
  goldens unchanged; unit test that the held list ignores keys; `run.sh`: a
  docked `send` pastes and the board reopens.

**Step 8. The board's tablet mode.**
- Does: the board reads `COCKPIT_PANEL_MODE`, the card and its buttons, mouse
  capture with restore on every exit path, the tap map, the guard, the 28x12
  floor card, `board::render()`'s hit map, the Termux extra key for
  `prefix w`.
- Files: `tui/src/board.rs`, `tui/src/backend.rs` (mouse init for the board),
  `examples/termux.properties`, `docs/tui.md`.
- Tests: unit tests for taps through the key dispatch and the guard with the
  clock passed in; the property test from 28x12 to 120x40 that no confirming
  cell shares a position with what opened it; goldens
  `board-tablet-{40,60}.txt` and one at 100; a test that every exit path writes
  the mouse-off sequence.

**Step 9. Panel key changes.** *Waits for lane C.*
- Does: `^R` retry, `^G` best guess, the notes on bare `r` and `g`, the Error
  text that names `^R`.
- Files: `tui/src/panel/app.rs`, `tui/src/panel/view.rs`, `docs/tui.md`.
- Tests: unit tests for the keys and the notes; every panel golden whose footer
  names `r retry` or `g` regenerated at 40/60/80/120, and the reason in the
  commit message.

**Revision 2 in the steps.** ⟦rev2: P1-P10⟧ No new step; each addition rides
on the step that already touches the code.
- Step 1: D17's two rules and D19. Goldens `board-nocolor-100.txt` (`NO_COLOR`,
  selection in reverse) and the footer per focus (list, header); no hints on
  the board's frame borders. A unit test that each footer lists only keys the
  dispatch accepts in that place.
- Step 2: D20. The wrapper starts a failure line with `✕`. Unit tests: such a
  note stays until a key, and that key still runs, except `esc`, which only
  dismisses.
- Step 4: D21. Golden `board-find-none-60.txt` beside the 100 one.
- Step 6: D18 and the sheet's `n`. Unit tests: a refresh with the confirm open
  neither reorders nor removes rows; a changed prompt closes the confirm; keys
  other than `y` `n` `esc` `?` `q` `^C` do nothing. `run.sh`: a refused
  `deliver` returns to the board with a `✕` note. Its goldens use the list
  layout; the 100-column sheet and confirm in 6.4, which sit in the right
  pane, become goldens in step 7.
- Step 8: D19 for the tablet button row.
- D22 has no step of its own: each step's key tests check that `esc` is never
  the only way out.

## 11. Research: how terminal apps do it

Gathered in November 2026 from docs, READMEs, man pages, GitHub issues and a
few write-ups. Each point is tagged **evidence** (docs, a standard, or users in
issues) or **opinion** (one author's taste, or an inference). Links marked
*mirror* are third-party copies of GitHub issues that came up in search
results but were not opened; treat them as weaker.

### 11.1 Colour and visual style

What works:

- **Terminal palette first.** lazygit defaults to the 16 named colours plus
  bold, so the user's theme sets the look; hex is optional. Its focused border
  is green and bold, and unfocused borders use the terminal default.
  [lazygit Config.md](https://github.com/jesseduffield/lazygit/blob/master/docs/Config.md). *evidence*
- helix themes are built on the terminal's 16 colours, with a palette table and
  `inherits`; `ui.selection` and `ui.cursor` are separate keys.
  [helix themes](https://docs.helix-editor.com/themes.html). *evidence*
- k9s uses `default` to mean transparent, so the terminal background shows.
  [k9s skins](https://k9scli.io/topics/skins/). *evidence*
- Textual first refused ANSI themes, then shipped `ansi-dark` and `ansi-light`,
  saying that mixing RGB and ANSI makes legibility hard to guarantee.
  [PR #6513](https://github.com/Textualize/textual/pull/6513),
  [issue #1460](https://github.com/willmcgugan/textual/issues/1460). *evidence*
- Textual's design system uses semantic roles (primary, warning, error,
  success) and builds muted text as a 70% blend, not SGR dim.
  [Textual design guide](https://textual.textualize.io/guide/design/). *evidence*
- **Light and dark.** lazygit asks the terminal whether its background is dark,
  assumes dark without an answer, and offers an override. yazi has separate
  dark and light slots. [lazygit Config.md](https://github.com/jesseduffield/lazygit/blob/master/docs/Config.md),
  [yazi theme](https://yazi-rs.github.io/docs/configuration/theme/). *evidence*
- **Tiers.** btop drops from truecolor to the 256 cube to a 16-colour tty theme,
  and lets you pick braille or block graphs for fonts that lack braille.
  [btop README](https://github.com/aristocratos/btop/blob/main/README.md). *evidence*
- **Selected row.** lazygit lets you pick a background, `reverse`, or bold only.
  gitui can turn its selection foreground off.
  [lazygit Config.md](https://github.com/jesseduffield/lazygit/blob/master/docs/Config.md),
  [gitui THEMES.md](https://github.com/gitui-org/gitui/blob/master/THEMES.md). *evidence*
- **Icons opt-in.** lazygit's Nerd Font icons are off until you set
  `nerdFontsVersion`
  ([Config.md](https://github.com/jesseduffield/lazygit/blob/master/docs/Config.md));
  broot added a Nerd Font theme as an opt-in after a long
  thread about font fallback. [broot #333](https://github.com/Canop/broot/issues/333). *evidence*
- htop ships several schemes, among them Monochromatic, Black on White and Light
  Terminal. [htop ColorsPanel.c, a mirror](https://portrait.gitee.com/mirrors/htop/blob/main/ColorsPanel.c). *evidence for the list.*
  One reader finds Black on White the easiest to read.
  [blog](https://clfs.org/~kb0iic/kmandla/kmandla.wordpress.com/2009/12/15/customizing-htop). *opinion*

What goes wrong:

- **Dim.** SGR 2 is a blend toward the background, not a palette colour, so no
  theme can fix it. Muted text becomes unreadable.
  [Auggie #169](https://github.com/augmentcode/auggie/issues/169). *evidence*
  Windows Terminal draws dim like normal text on light themes.
  [terminal #20109](https://github.com/microsoft/terminal/issues/20109). *evidence, from the search snippet*
  mosh draws it at full brightness, so hints look like input.
  [mirror of a Claude Code issue](https://claudeissues.com/issue/86515-bug-hint-placeholder-text-uses-sgr-2-faint-which-renders-at-full-brightness-on-t). *evidence, mirror*
- **One polarity.** Bright-black "muted" text nearly vanishes on light palettes;
  hard-coded white disappears on Solarized Light.
  [mirror](https://claudeissues.com/issue/49839-dim-secondary-text-unreadable-on-light-terminal-themes-uses-ansi-bright-black-wh),
  [mirror](https://claudeissues.com/issue/38386-white-text-in-planning-mode-is-invisible-on-light-terminal-themes-e-g-iterm2-sol). *evidence, mirrors*
  Bright green and bright yellow are usually invisible on light backgrounds.
  [ble.sh #478](https://github.com/akinomyoga/ble.sh/issues/478). *evidence*
  fish found yellow bad on light and blue bad on dark.
  [fish PR #10758](https://github.com/fish-shell/fish-shell/pull/10758). *evidence*
- **Under tmux.** Some runtimes see `$TMUX` and cap colour at 256 even with
  `COLORTERM=truecolor`. [nodejs/node #62404](https://github.com/nodejs/node/issues/62404). *evidence*
- **Fonts.** Default Consolas shows boxes for TUI glyphs.
  [mirror](https://claudeissues.com/issue/61988-feature-suggest-a-nerd-font-during-onboarding-for-windows-users-default-consolas). *evidence, mirror*

Standards:

- `NO_COLOR` set to anything non-empty means no ANSI colour. It does not forbid
  bold, underline or italic, and user config may override it.
  [no-color.org](https://no-color.org/). *evidence*
- `CLICOLOR_FORCE` overrides `CLICOLOR`, and `NO_COLOR` overrides both.
  [CMake docs](https://cmake.org/cmake/help/latest/envvar/CLICOLOR_FORCE.html). *evidence*
- WCAG 1.4.1: colour must not be the only way information is shown.
  [Understanding 1.4.1](https://w3c.github.io/wcag/understanding/use-of-color.html). *evidence; written for the web, applied to terminals by inference*
- "No colour pair is safe on every background": only a few of the 16 read on
  both light and dark. [pretty-error #7](https://github.com/AriaMinaei/pretty-error/issues/7). *opinion, thread comments*

### 11.2 Keys, focus and feedback

What works:

- **Both key families.** lazygit, yazi and ncdu bind arrows and `hjkl` by
  default. [lazygit](https://github.com/jesseduffield/lazygit/blob/master/docs/Config.md),
  [yazi quick start](https://yazi-rs.github.io/docs/quick-start/),
  [ncdu(1)](https://dev.yorhel.nl/ncdu/man). *evidence*
  fzf exits on `esc`, `^C` and `^G`, so nobody needs a manual to leave.
  [fzf README](https://github.com/junegunn/fzf/blob/master/README.md). *evidence*
- **Jump keys you can see.** lazygit's number keys jump to panels, and
  `showPanelJumps` prints the key in each panel's title. Its list footer shows
  `5 of 20`. [lazygit Config.md](https://github.com/jesseduffield/lazygit/blob/master/docs/Config.md). *evidence*
- **Footer from the key table.** Textual's Footer is generated from the focused
  widget's bindings and hides or dims what is unavailable.
  [Textual Footer](https://textual.textualize.io/widgets/footer/). *evidence*
  Bubbles `help` renders a one-line short help and a full help from the same
  key map, and skips disabled bindings.
  [bubbles/help](https://pkg.go.dev/github.com/charmbracelet/bubbles/help). *evidence*
  k9s and aerc put the keys for the current context behind `?`.
  [k9s commands](https://k9scli.io/topics/commands/),
  [aerc-binds(5)](https://www.mankier.com/5/aerc-binds). *evidence*
- **An exit that cannot be taken away.** Textual's priority bindings cannot be
  overridden by a widget, so quit always works.
  [Textual input](https://textual.textualize.io/guide/input/). *evidence*
- **Confirm by severity.** k9s confirms delete on `^D` and kills without asking
  on `^K`, after users deleted a context by accident.
  [k9s commands](https://k9scli.io/topics/commands/), [k9s #21](https://github.com/derailed/k9s/issues/21). *evidence*
  ncdu confirms delete by default and quit only on request.
  [ncdu(1)](https://dev.yorhel.nl/ncdu/man). *evidence*
- **Errors where they happened.** ncdu marks a directory that had read errors in
  the list itself. [ncdu(1)](https://dev.yorhel.nl/ncdu/man). *evidence*
- **Feedback.** lazygit's bottom line doubles as the loader
  ([Config.md](https://github.com/jesseduffield/lazygit/blob/master/docs/Config.md)).
  Textual keeps timed toasts for brief, supplementary news.
  [Textual 0.30 notifications](https://textual.textualize.io/blog/2023/07/17/textual-0300-adds-desktop-style-notifications/). *evidence*
- **tmux's own choosers.** Enter chooses, `q` exits, `v` toggles the preview,
  `x` kills after a confirm. [tmux(1)](https://man.archlinux.org/man/tmux.1). *evidence*

What goes wrong:

- **The target moves.** A background fetch moved lazygit's selection, so a
  delete could hit another branch.
  [lazygit #4116](https://github.com/jesseduffield/lazygit/issues/4116),
  [#2037](https://github.com/jesseduffield/lazygit/issues/2037). *evidence*
  tig fixed position restore on reload more than once.
  [tig NEWS](https://github.com/jonas/tig/blob/master/NEWS.adoc). *evidence*
- **Confirm fatigue.** Users asked to turn off lazygit's force-push and
  stacked-push prompts; the maintainer proposed asking less often instead.
  [#5966](https://github.com/jesseduffield/lazygit/issues/5966),
  [#6144](https://github.com/jesseduffield/lazygit/issues/6144). *evidence; the proposal is the maintainer's view*
  Users asked for `y` to confirm again. [#2611](https://github.com/jesseduffield/lazygit/issues/2611). *evidence*
- **Confirms that leak keys.** lazygit's global keys stayed live under an open
  confirm until a fix. [PR #4284](https://github.com/jesseduffield/lazygit/pull/4284). *evidence*
- **Errors you cannot read.** A failed hook's popup scrolls one line at a time
  with the useful line at the bottom.
  [lazygit #4372](https://github.com/jesseduffield/lazygit/issues/4372). *evidence*
- **Filters that forget.** k9s loses the filter on back-navigation.
  [k9s #3220](https://github.com/derailed/k9s/issues/3220). *evidence*
- **Help that lies.** yazi's which-key list shows keys that were disabled.
  [yazi #1880](https://github.com/sxyazi/yazi/issues/1880). *evidence*
- **Keys the terminal eats.** `Alt-[` clashes with tmux in yazi; htop's F-keys
  fail on some terminals; tmux makes a bare `esc` wait for `escape-time`, 500 ms
  in the FAQ revision the research read. tmux 3.5 and later default to 10 ms
  (a fresh 3.8 server reports 10, checked on a private socket).
  [yazi #2587](https://github.com/sxyazi/yazi/discussions/2587),
  [htop #1047](https://github.com/htop-dev/htop/issues/1047),
  [tmux FAQ, the revision read](https://github.com/tmux/tmux/wiki/FAQ/0f5f2246c9bd6f26cf063801da24a9e582a23163). *evidence*
- **Vim keys bolted on.** gitui defaults to arrows; users keep asking for
  `hjkl`, and the vim key file produced duplicate-binding errors.
  [gitui #227](https://github.com/gitui-org/gitui/issues/227),
  [#1202](https://github.com/extrawurst/gitui/issues/1202). *evidence*

Guidelines, from [clig.dev](https://clig.dev/) (widely cited, but one set of
authors): confirm in proportion to the damage; print something within 100 ms,
since silence looks broken; rewrite errors for humans and say how to fix them;
use red sparingly; let `^C` always work; suggest the next command; never let
colour be the only signal and honour `NO_COLOR`.

### 11.3 Gaps

- No app or guideline found covers empty-state design beyond clig.dev's
  "suggest the next step". P8 rests on that alone.
- No source compared borders against whitespace. The frame choices in this
  document are taste. *opinion*
- Nothing solid on mouse support. On minimum sizes, only single threads:
  lazygit embedded in a small window says "Not enough space to render panels"
  ([toggleterm #156](https://github.com/akinsho/toggleterm.nvim/issues/156)).
  The tablet rules (D13) come from this repo's own testing.
- Not covered: ranger, bottom, gum's colour guidance, and tmux's
  `confirm-before`. `display-menu` is covered only through a secondary quote of
  the man page ([lesssalt](https://lesssalt.srht.site/tmux/tmux_menu/)).
